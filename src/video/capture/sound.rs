//! Son d'un partage, en mono à 48 kHz comme le micro, pour passer par le
//! même moteur (`voice::AudioBackend`) :
//! - fenêtre : le son de son application seule (boucle WASAPI par
//!   processus, `process_sound`) ;
//! - écran : tout le son de l'ordinateur sauf celui de Ferrocord (les voix
//!   de l'appel ne repartent pas vers l'appel).
//!
//! Windows trop ancien pour la boucle par processus (avant 10 build 20348) :
//! pour un écran seulement, repli sur la boucle de la sortie par défaut
//! (`cpal`, sans `unsafe`, Ferrocord compris) ; pour une fenêtre, pas de son.

use super::{Source, SourceKind};
use crate::audio::io::{AudioError, CaptureFn, PlaybackFn};
use crate::voice::{AudioBackend, AudioStreams};

/// Le son d'une source partagée à la place du micro ; la lecture est
/// ignorée (une connexion de partage ne fait qu'envoyer).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareSound {
    source: Source,
}

impl ShareSound {
    /// `None` pour une caméra (pas de son à partager).
    pub fn for_source(source: &Source) -> Option<ShareSound> {
        (source.kind != SourceKind::Camera).then(|| ShareSound { source: source.clone() })
    }
}

impl AudioBackend for ShareSound {
    fn open(&self, _input: Option<&str>, _output: Option<&str>, capture: CaptureFn, _playback: PlaybackFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        open(&self.source, capture).map(|s| Box::new(s) as Box<dyn AudioStreams>)
    }
}

/// Capture du son en cours ; elle s'arrête quand on la lâche.
pub struct SoundHandle {
    #[cfg(windows)]
    inner: Inner,
}

#[cfg(windows)]
enum Inner {
    Process(super::process_sound::Running),
    System(imp::Running),
}

impl AudioStreams for SoundHandle {
    fn take_error(&self) -> Option<AudioError> {
        #[cfg(windows)]
        return match &self.inner {
            Inner::Process(r) => r.take_error(),
            Inner::System(r) => r.take_error(),
        };
        #[cfg(not(windows))]
        None
    }
}

/// Ouvre la capture du son de `source` ; `on_capture` reçoit du mono à
/// 48 kHz sur le fil audio.
pub fn open(source: &Source, on_capture: CaptureFn) -> Result<SoundHandle, AudioError> {
    #[cfg(windows)]
    {
        use super::process_sound::{Running, Target};
        use std::sync::{Arc, Mutex};
        let target = match source.kind {
            SourceKind::Camera => return Err(AudioError::NoDevice { input: true }),
            SourceKind::Screen => Target::Exclude(std::process::id()),
            SourceKind::Window => Target::Include(super::raw_id(source).and_then(super::screen::window_pid).ok_or(AudioError::NoDevice { input: true })?),
        };
        // Un seul rappel pour la boucle par processus et son repli.
        let shared = Arc::new(Mutex::new(on_capture));
        let relay = |s: Arc<Mutex<CaptureFn>>| -> CaptureFn {
            Box::new(move |m: &[f32]| {
                if let Ok(mut cb) = s.try_lock() {
                    cb(m);
                }
            })
        };
        match Running::start(target, relay(shared.clone())) {
            Ok(r) => Ok(SoundHandle { inner: Inner::Process(r) }),
            Err(_) if source.kind == SourceKind::Screen => imp::Running::start(relay(shared)).map(|r| SoundHandle { inner: Inner::System(r) }),
            Err(e) => Err(e),
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (source, on_capture);
        Err(AudioError::NoDevice { input: false })
    }
}

/// Convertit les trames entrelacées du périphérique en mono à 48 kHz.
pub struct Downmixer {
    channels: usize,
    rs: crate::audio::io::resample::Resampler,
    mono: Vec<f32>,
    out: Vec<f32>,
}

/// Trames du périphérique traitées d'un coup.
const CHUNK: usize = 1024;
const RATE: u32 = 48_000;

impl Downmixer {
    pub fn new(channels: usize, rate: u32) -> Self {
        let rs = crate::audio::io::resample::Resampler::new(rate, RATE, 1);
        let out = vec![0.0; rs.max_output(CHUNK)];
        Downmixer { channels: channels.max(1), rs, mono: vec![0.0; CHUNK], out }
    }

    pub fn feed(&mut self, data: &[f32], cb: &mut CaptureFn) {
        for block in data.chunks(CHUNK * self.channels) {
            let mut n = 0;
            for (m, f) in self.mono.iter_mut().zip(block.chunks_exact(self.channels)) {
                *m = crate::audio::io::downmix(f);
                n += 1;
            }
            let (_, written) = self.rs.process(&self.mono[..n], &mut self.out);
            if written > 0 {
                cb(&self.out[..written]);
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::{CaptureFn, Downmixer};
    use crate::audio::io::AudioError;
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample};
    use std::sync::{mpsc, Arc, Mutex, MutexGuard};
    use std::thread::JoinHandle;

    fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn backend(e: impl std::fmt::Display) -> AudioError {
        AudioError::Backend(e.to_string())
    }

    enum Ctl {
        /// La sortie a changé ou disparu : rouvrir sur celle par défaut.
        Lost,
        Stop,
    }

    struct Shared {
        cb: Mutex<(CaptureFn, Downmixer, Vec<f32>)>,
        error: Mutex<Option<AudioError>>,
    }

    pub(super) struct Running {
        ctl: mpsc::Sender<Ctl>,
        thread: Option<JoinHandle<()>>,
        shared: Arc<Shared>,
    }

    /// Boucle sur la sortie par défaut : `cpal` capture en boucle quand on
    /// ouvre une entrée sur un périphérique de sortie.
    fn build(shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, AudioError> {
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or(AudioError::NoDevice { input: false })?;
        let supported = device.default_output_config().map_err(backend)?;
        let config = supported.config();
        {
            let mut g = locked(&shared.cb);
            g.1 = Downmixer::new(usize::from(config.channels), config.sample_rate);
            g.2 = vec![0.0; 4096 * usize::from(config.channels.max(1))];
        }
        fn typed<T: SizedSample>(device: &cpal::Device, config: cpal::StreamConfig, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, AudioError>
        where
            f32: FromSample<T>,
        {
            let s = shared.clone();
            let tx = ctl.clone();
            device
                .build_input_stream::<T, _, _>(
                    config,
                    move |data: &[T], _| {
                        // Jamais d'attente sur le fil temps réel.
                        if let Ok(mut g) = s.cb.try_lock() {
                            let (cb, mix, scratch) = &mut *g;
                            for part in data.chunks(scratch.len().max(1)) {
                                for (d, x) in scratch.iter_mut().zip(part) {
                                    *d = f32::from_sample_(*x);
                                }
                                mix.feed(&scratch[..part.len()], cb);
                            }
                        }
                    },
                    move |e: cpal::Error| {
                        if matches!(e.kind(), ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated | ErrorKind::DeviceChanged | ErrorKind::BackendError) {
                            let _ = tx.send(Ctl::Lost);
                        }
                    },
                    None,
                )
                .map_err(backend)
        }
        let stream = match supported.sample_format() {
            SampleFormat::F32 => typed::<f32>(&device, config, shared, ctl),
            SampleFormat::I16 => typed::<i16>(&device, config, shared, ctl),
            SampleFormat::I32 => typed::<i32>(&device, config, shared, ctl),
            other => Err(AudioError::Unsupported(other.to_string())),
        }?;
        stream.play().map_err(backend)?;
        Ok(stream)
    }

    impl Running {
        pub(super) fn start(cb: CaptureFn) -> Result<Self, AudioError> {
            let shared = Arc::new(Shared { cb: Mutex::new((cb, Downmixer::new(2, 48_000), Vec::new())), error: Mutex::new(None) });
            let (ctl, rx) = mpsc::channel();
            let (ready_tx, ready) = mpsc::channel();
            let (s, tx) = (shared.clone(), ctl.clone());
            let thread = std::thread::Builder::new()
                .name("ferrocord-son-partage".into())
                .spawn(move || {
                    let mut stream = match build(&s, &tx) {
                        Ok(st) => Some(st),
                        Err(e) => {
                            let _ = ready_tx.send(Err(e));
                            return;
                        }
                    };
                    let _ = ready_tx.send(Ok(()));
                    while let Ok(msg) = rx.recv() {
                        match msg {
                            Ctl::Stop => return,
                            Ctl::Lost => {
                                stream = None;
                                match build(&s, &tx) {
                                    Ok(st) => stream = Some(st),
                                    Err(e) => *locked(&s.error) = Some(e),
                                }
                            }
                        }
                    }
                    drop(stream);
                })
                .map_err(backend)?;
            match ready.recv() {
                Ok(Ok(())) => Ok(Running { ctl, thread: Some(thread), shared }),
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
            locked(&self.shared.error).take()
        }
    }

    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.ctl.send(Ctl::Stop);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn stereo_44k1_becomes_mono_48k() {
        let got = Arc::new(Mutex::new(Vec::new()));
        let sink = got.clone();
        let mut cb: CaptureFn = Box::new(move |m: &[f32]| sink.lock().unwrap().extend_from_slice(m));
        let mut mix = Downmixer::new(2, 44_100);
        // Une seconde de stéréo : gauche 0,5, droite 0,1 → mono 0,3.
        let data: Vec<f32> = (0..44_100).flat_map(|_| [0.5f32, 0.1]).collect();
        mix.feed(&data, &mut cb);
        let out = got.lock().unwrap();
        // Le rééchantillonneur peut sortir un échantillon d'avance (arrondi de phase).
        assert!((47_900..=48_100).contains(&out.len()), "{} échantillons", out.len());
        assert!(out[100..].iter().all(|s| (s - 0.3).abs() < 1e-3));
    }
}
