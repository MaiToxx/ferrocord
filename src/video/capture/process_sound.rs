//! Son d'un seul arbre de processus (boucle WASAPI par processus, Windows 10
//! 20348 et plus) : celui de la fenêtre partagée, ou tout sauf Ferrocord
//! pour un écran (les voix de l'appel ne repartent pas vers l'appel).
//!
//! Seul fichier de la capture avec de l'`unsafe` : Windows n'offre cette
//! capture que par `ActivateAudioInterfaceAsync` (paramètres passés dans un
//! PROPVARIANT brut) et `IAudioClient`/`IAudioCaptureClient`, dont toutes
//! les méthodes sont `unsafe` dans la crate `windows`. Aucune voie WinRT ni
//! crate sûre ne l'expose. Tout tient dans le fil « ferrocord-son-processus ».

use super::sound::Downmixer;
use crate::audio::io::{AudioError, CaptureFn};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;
use windows::core::{implement, Interface, Ref, HRESULT, IUnknown};
use windows::Win32::Foundation::{CloseHandle, HANDLE, WAIT_OBJECT_0};
use windows::Win32::Media::Audio::{
    ActivateAudioInterfaceAsync, IActivateAudioInterfaceAsyncOperation, IActivateAudioInterfaceCompletionHandler, IActivateAudioInterfaceCompletionHandler_Impl, IAudioCaptureClient, IAudioClient, AUDCLNT_BUFFERFLAGS_SILENT,
    AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM, AUDCLNT_STREAMFLAGS_EVENTCALLBACK, AUDCLNT_STREAMFLAGS_LOOPBACK, AUDIOCLIENT_ACTIVATION_PARAMS, AUDIOCLIENT_ACTIVATION_PARAMS_0,
    AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK, AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE, VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, WAVEFORMATEX,
};
use windows::Win32::System::Com::StructuredStorage::{PROPVARIANT, PROPVARIANT_0, PROPVARIANT_0_0, PROPVARIANT_0_0_0};
use windows::Win32::System::Com::{CoInitializeEx, BLOB, COINIT_MULTITHREADED};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::Variant::VT_BLOB;

/// Format demandé à Windows (il convertit) : flottants 32 bits, stéréo, 48 kHz.
const CHANNELS: u16 = 2;
const RATE: u32 = 48_000;
const WAVE_FORMAT_IEEE_FLOAT: u16 = 3;
/// Tampon de Windows : 100 ms, en unités de 100 ns.
const BUFFER_100NS: i64 = 1_000_000;
const ACTIVATE_WAIT: Duration = Duration::from_secs(5);
/// Réveil au plus tard pour voir l'arrêt demandé.
const WAIT_MS: u32 = 100;

/// Quel son capter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// Ce processus et ses enfants (l'application d'une fenêtre partagée).
    Include(u32),
    /// Tout sauf ce processus et ses enfants (Ferrocord, pour un écran).
    Exclude(u32),
}

#[implement(IActivateAudioInterfaceCompletionHandler)]
struct Activated(mpsc::SyncSender<()>);

impl IActivateAudioInterfaceCompletionHandler_Impl for Activated_Impl {
    fn ActivateCompleted(&self, _: Ref<IActivateAudioInterfaceAsyncOperation>) -> windows::core::Result<()> {
        let _ = self.0.try_send(());
        Ok(())
    }
}

fn backend(e: windows::core::Error) -> AudioError {
    AudioError::Backend(e.message())
}

/// Client audio en boucle sur `target`, prêt à démarrer.
fn activate(target: Target) -> Result<IAudioClient, AudioError> {
    let (pid, mode) = match target {
        Target::Include(p) => (p, PROCESS_LOOPBACK_MODE_INCLUDE_TARGET_PROCESS_TREE),
        Target::Exclude(p) => (p, PROCESS_LOOPBACK_MODE_EXCLUDE_TARGET_PROCESS_TREE),
    };
    let mut params = AUDIOCLIENT_ACTIVATION_PARAMS {
        ActivationType: AUDIOCLIENT_ACTIVATION_TYPE_PROCESS_LOOPBACK,
        Anonymous: AUDIOCLIENT_ACTIVATION_PARAMS_0 { ProcessLoopbackParams: AUDIOCLIENT_PROCESS_LOOPBACK_PARAMS { TargetProcessId: pid, ProcessLoopbackMode: mode } },
    };
    let blob = BLOB { cbSize: std::mem::size_of::<AUDIOCLIENT_ACTIVATION_PARAMS>() as u32, pBlobData: (&raw mut params).cast() };
    // Jamais détruit : la destruction d'un PROPVARIANT libère son contenu
    // (`PropVariantClear`), or ce BLOB pointe sur `params`, qui est sur la
    // pile. Le libérer abîmait le tas : Ferrocord plantait au lancement d'un
    // partage avec le son (0xc0000374, constaté par l'utilisateur, 0.1.28).
    let prop = std::mem::ManuallyDrop::new(PROPVARIANT {
        Anonymous: PROPVARIANT_0 { Anonymous: std::mem::ManuallyDrop::new(PROPVARIANT_0_0 { vt: VT_BLOB, wReserved1: 0, wReserved2: 0, wReserved3: 0, Anonymous: PROPVARIANT_0_0_0 { blob } }) },
    });
    let (tx, done) = mpsc::sync_channel(1);
    let handler: IActivateAudioInterfaceCompletionHandler = Activated(tx).into();
    // SAFETY: `prop` et `params` (pointé par le BLOB) vivent jusqu'à la fin de
    // la fonction, après la fin de l'activation attendue ci-dessous ; Windows
    // copie les paramètres pendant l'appel. L'IID est celui de `IAudioClient`.
    let op = unsafe { ActivateAudioInterfaceAsync(VIRTUAL_AUDIO_DEVICE_PROCESS_LOOPBACK, &IAudioClient::IID, Some(&raw const *prop), &handler) }.map_err(backend)?;
    done.recv_timeout(ACTIVATE_WAIT).map_err(|_| AudioError::Backend("activation timeout".into()))?;
    let mut hr = HRESULT(0);
    let mut unknown: Option<IUnknown> = None;
    // SAFETY: les deux pointeurs visent des variables locales valides ;
    // l'opération est terminée (rappel reçu).
    unsafe { op.GetActivateResult(&mut hr, &mut unknown) }.map_err(backend)?;
    hr.ok().map_err(backend)?;
    unknown.ok_or(AudioError::NoDevice { input: true })?.cast::<IAudioClient>().map_err(backend)
}

fn capture_loop(client: &IAudioClient, stop: &AtomicBool, cb: &mut CaptureFn) -> Result<(), AudioError> {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_IEEE_FLOAT,
        nChannels: CHANNELS,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * u32::from(CHANNELS) * 4,
        nBlockAlign: CHANNELS * 4,
        wBitsPerSample: 32,
        cbSize: 0,
    };
    let flags = AUDCLNT_STREAMFLAGS_LOOPBACK | AUDCLNT_STREAMFLAGS_EVENTCALLBACK | AUDCLNT_STREAMFLAGS_AUTOCONVERTPCM;
    // SAFETY: `format` est une WAVEFORMATEX complète (cbSize 0) qui vit
    // pendant l'appel ; pas de GUID de session.
    unsafe { client.Initialize(AUDCLNT_SHAREMODE_SHARED, flags, BUFFER_100NS, 0, &format, None) }.map_err(backend)?;
    // SAFETY: événement anonyme sans attributs ; fermé plus bas.
    let event: HANDLE = unsafe { CreateEventW(None, false, false, None) }.map_err(backend)?;
    let mut run = || -> Result<(), AudioError> {
        // SAFETY: `event` est un événement valide jusqu'à `CloseHandle`, après l'arrêt du client.
        unsafe { client.SetEventHandle(event) }.map_err(backend)?;
        // SAFETY: client initialisé ; service demandé par son type.
        let capture: IAudioCaptureClient = unsafe { client.GetService() }.map_err(backend)?;
        // SAFETY: client initialisé avec son événement.
        unsafe { client.Start() }.map_err(backend)?;
        let mut mix = Downmixer::new(usize::from(CHANNELS), RATE);
        let mut silence: Vec<f32> = Vec::new();
        // Boucle à part : quelle que soit sa fin (arrêt demandé ou erreur du
        // périphérique), le client est arrêté avant que l'événement soit fermé.
        let mut pump = || -> Result<(), AudioError> {
        while !stop.load(Ordering::Relaxed) {
            // SAFETY: `event` est valide (voir plus haut).
            if unsafe { WaitForSingleObject(event, WAIT_MS) } != WAIT_OBJECT_0 {
                continue;
            }
            loop {
                // SAFETY: client démarré.
                let next = unsafe { capture.GetNextPacketSize() }.map_err(backend)?;
                if next == 0 {
                    break;
                }
                let (mut data, mut frames, mut bflags) = (std::ptr::null_mut::<u8>(), 0u32, 0u32);
                // SAFETY: pointeurs vers des variables locales ; le tampon
                // rendu est lu avant `ReleaseBuffer`.
                unsafe { capture.GetBuffer(&mut data, &mut frames, &mut bflags, None, None) }.map_err(backend)?;
                let n = frames as usize * usize::from(CHANNELS);
                if bflags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32) != 0 || data.is_null() {
                    silence.resize(n, 0.0);
                    mix.feed(&silence[..n], cb);
                } else {
                    // SAFETY: Windows rend `frames` trames du format demandé
                    // (f32 stéréo, alignées sur 4 octets), valides jusqu'à
                    // `ReleaseBuffer` ; la tranche ne sort pas de ce bloc.
                    let samples = unsafe { std::slice::from_raw_parts(data.cast::<f32>(), n) };
                    mix.feed(samples, cb);
                }
                // SAFETY: rend exactement ce que `GetBuffer` a donné.
                unsafe { capture.ReleaseBuffer(frames) }.map_err(backend)?;
            }
        }
        Ok(())
        };
        let pumped = pump();
        // SAFETY: client démarré.
        let _ = unsafe { client.Stop() };
        pumped
    };
    let result = run();
    // SAFETY: l'événement n'est plus utilisé (client arrêté ou jamais démarré).
    let _ = unsafe { CloseHandle(event) };
    result
}

/// Capture en cours ; s'arrête quand on la lâche.
pub(super) struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    error: Arc<std::sync::Mutex<Option<AudioError>>>,
}

impl Running {
    pub(super) fn start(target: Target, mut cb: CaptureFn) -> Result<Self, AudioError> {
        let stop = Arc::new(AtomicBool::new(false));
        let error = Arc::new(std::sync::Mutex::new(None));
        let (ready_tx, ready) = mpsc::channel();
        let (s, err) = (stop.clone(), error.clone());
        let thread = std::thread::Builder::new()
            .name("ferrocord-son-processus".into())
            .spawn(move || {
                // SAFETY: initialise COM (MTA) pour ce fil seulement, avant tout appel COM.
                let _ = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
                let client = match activate(target) {
                    Ok(c) => c,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                };
                let _ = ready_tx.send(Ok(()));
                if let Err(e) = capture_loop(&client, &s, &mut cb) {
                    *err.lock().unwrap_or_else(|p| p.into_inner()) = Some(e);
                }
            })
            .map_err(|e| AudioError::Backend(e.to_string()))?;
        match ready.recv() {
            Ok(Ok(())) => Ok(Running { stop, thread: Some(thread), error }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(AudioError::Backend("sound thread stopped".into()))
            }
        }
    }

    pub(super) fn take_error(&self) -> Option<AudioError> {
        self.error.lock().unwrap_or_else(|p| p.into_inner()).take()
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La capture du son de tout sauf ce processus s'ouvre et se ferme sans
    /// abîmer la mémoire (plantage au lancement d'un partage avec le son,
    /// constaté par l'utilisateur, 0.1.28). Aucun son n'est joué ; sur un
    /// Windows sans cette capture, l'ouverture échoue proprement.
    #[test]
    fn la_capture_s_ouvre_et_se_ferme_plusieurs_fois() {
        for _ in 0..20 {
            if let Ok(r) = Running::start(Target::Exclude(std::process::id()), Box::new(|_| {})) {
                std::thread::sleep(Duration::from_millis(20));
                drop(r);
            }
            // Le tas est sollicité entre deux ouvertures : une libération
            // fautive s'y voit.
            let v: Vec<Vec<u8>> = (0..200).map(|k| vec![0u8; 64 + k]).collect();
            drop(v);
        }
    }
}
