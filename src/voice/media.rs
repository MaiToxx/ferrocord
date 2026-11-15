//! Les deux fils audio d'un appel : capture → volume d'entrée → traitement
//! → porte → Opus → connexion vocale ; trames reçues → gigue, mixage →
//! volume de sortie → lecture.
//!
//! Aucun verrou dans les rappels : le moteur est coupé en deux moitiés
//! (`audio::engine::{Capture, Playback}`), une par fil. Les réglages
//! arrivent par des atomiques ([`Shared`]), les trames reçues par une file
//! `std::sync::mpsc` (sans verrou côté lecteur), le son joué (référence de
//! l'annulation d'écho) par la file `audio::io::ring::Ring`.

use crate::audio::clip::Pcm;
use crate::audio::dsp::{DspSettings, VoiceChain};
use super::recorder::{RecMsg, RecTx};
use crate::audio::engine::{Capture, Gate, MicTap, PeerTap, Playback, SILENCE};
use crate::audio::io::{ring::Ring, AudioError, CaptureFn, PlaybackFn, Streams};
use crate::audio::Processor;
use crate::i18n::table::audio::{f as af, t as at};
use crate::net::voice::VoiceCtl;
use crate::state::audio_prefs::{AudioPrefs, InputMode};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::mpsc as tmpsc;

const RATE_PER_MS: usize = 48;
/// Son joué gardé pour l'annulation d'écho : trois trames au plus.
const MAX_FAR: usize = 3 * crate::audio::engine::FRAME;
/// Volume d'entrée appliqué par morceaux de cette taille (tampon préalloué).
const CAP_CHUNK: usize = 4096;
const PLAY_CHUNK: usize = 2048;
/// Qui parle : relevé toutes les 100 ms de son joué.
const SPEAKING_EVERY: usize = 100 * RATE_PER_MS;

/// Périphériques ouverts ; tout s'arrête quand on les lâche.
pub trait AudioStreams: Send {
    /// Dernière erreur depuis l'ouverture (périphérique perdu…), une fois.
    fn take_error(&self) -> Option<AudioError>;
}

impl AudioStreams for Streams {
    fn take_error(&self) -> Option<AudioError> {
        Streams::take_error(self)
    }
}

/// Qui ouvre les périphériques : Windows en vrai, un faux dans les tests.
pub trait AudioBackend: Send + Sync {
    fn open(&self, input: Option<&str>, output: Option<&str>, capture: CaptureFn, playback: PlaybackFn) -> Result<Box<dyn AudioStreams>, AudioError>;
    /// Lecture seule (hors appel : direct regardé, sons courts).
    fn open_output(&self, output: Option<&str>, playback: PlaybackFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        self.open(None, output, Box::new(|_| {}), playback)
    }
    /// Capture seule (message vocal).
    fn open_input(&self, input: Option<&str>, capture: CaptureFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        self.open(input, None, capture, Box::new(|o: &mut [f32]| o.fill(0.0)))
    }
}

/// Les périphériques de Windows (`audio::io`).
pub struct Devices;

impl AudioBackend for Devices {
    fn open(&self, input: Option<&str>, output: Option<&str>, capture: CaptureFn, playback: PlaybackFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        crate::audio::io::open(input, output, capture, playback).map(|s| Box::new(s) as Box<dyn AudioStreams>)
    }
    fn open_output(&self, output: Option<&str>, playback: PlaybackFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        crate::audio::io::open_output(output, playback).map(|s| Box::new(s) as Box<dyn AudioStreams>)
    }
    fn open_input(&self, input: Option<&str>, capture: CaptureFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        crate::audio::io::open_input(input, capture).map(|s| Box::new(s) as Box<dyn AudioStreams>)
    }
}

/// Texte d'une erreur audio, dans la langue de l'interface.
pub(super) fn error_text(e: &AudioError) -> String {
    let lang = crate::i18n::language();
    match e {
        AudioError::NoDevice { input: true } => at::ERR_NO_INPUT.get(lang).to_string(),
        AudioError::NoDevice { input: false } => at::ERR_NO_OUTPUT.get(lang).to_string(),
        AudioError::DeviceLost { input: true, name } => af::lost_input(lang, name),
        AudioError::DeviceLost { input: false, name } => af::lost_output(lang, name),
        AudioError::Unsupported(m) => af::unsupported(lang, m),
        AudioError::Backend(m) if crate::audio::io::is_access_denied(m) => at::ERR_MIC_PRIVACY.get(lang).to_string(),
        AudioError::Backend(m) => af::audio_error(lang, m),
    }
}

/// Réglages lus par les fils audio à chaque rappel.
pub(super) struct Shared {
    input_gain: AtomicU32,
    output_gain: AtomicU32,
    muted: AtomicBool,
    deafened: AtomicBool,
    voice_mode: AtomicBool,
    auto: AtomicBool,
    threshold: AtomicU32,
    ptt: AtomicBool,
    release_ms: AtomicU32,
    dsp: AtomicU16,
    /// Égaliseur des voix (`audio::dsp::eq::pack`).
    eq: AtomicU32,
    /// Enregistrement local (plugin, lot E4) : le fil d'écriture, et un
    /// compteur qui change quand il change. Les fils audio ne regardent le
    /// verrou que ce jour-là (et sans attendre) : hors enregistrement, une
    /// lecture d'atomique par rappel.
    rec: Mutex<Option<RecTx>>,
    rec_gen: AtomicU32,
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Relaxed))
}

impl Shared {
    pub(super) fn new(s: &super::Settings, ptt: bool) -> Self {
        let sh = Shared {
            input_gain: AtomicU32::new(0),
            output_gain: AtomicU32::new(0),
            muted: AtomicBool::new(false),
            deafened: AtomicBool::new(false),
            voice_mode: AtomicBool::new(true),
            auto: AtomicBool::new(true),
            threshold: AtomicU32::new(0),
            ptt: AtomicBool::new(ptt),
            release_ms: AtomicU32::new(0),
            dsp: AtomicU16::new(0),
            eq: AtomicU32::new(0),
            rec: Mutex::new(None),
            rec_gen: AtomicU32::new(0),
        };
        sh.set(s);
        sh
    }

    pub(super) fn set(&self, s: &super::Settings) {
        let p = &s.prefs;
        self.input_gain.store(p.input_gain().to_bits(), Relaxed);
        self.output_gain.store(p.output_gain().to_bits(), Relaxed);
        self.muted.store(s.muted, Relaxed);
        self.deafened.store(s.deafened, Relaxed);
        self.voice_mode.store(p.mode == InputMode::Voice, Relaxed);
        self.auto.store(p.auto_threshold, Relaxed);
        self.threshold.store(f32::from(p.threshold_db).to_bits(), Relaxed);
        self.release_ms.store(u32::from(p.ptt_release_ms), Relaxed);
        self.dsp.store(p.dsp().with(s.extras).bits(), Relaxed);
        self.eq.store(crate::audio::dsp::eq::pack(s.extras.eq), Relaxed);
    }

    pub(super) fn set_ptt(&self, down: bool) {
        self.ptt.store(down, Relaxed);
    }

    /// Branche (ou débranche) le fil d'écriture de l'enregistrement local.
    pub(super) fn set_rec(&self, tx: Option<RecTx>) {
        *self.rec.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = tx;
        self.rec_gen.fetch_add(1, Relaxed);
    }

    /// Sur un fil audio : le fil d'écriture, s'il a changé depuis `seen`.
    /// Verrou pris ailleurs à cet instant : on réessaie au rappel suivant.
    fn rec_changed(&self, seen: &mut u32) -> Option<Option<RecTx>> {
        let g = self.rec_gen.load(Relaxed);
        if g == *seen {
            return None;
        }
        let tx = self.rec.try_lock().ok()?.clone();
        *seen = g;
        Some(tx)
    }
}

/// Vers le fil de lecture.
pub(super) enum PlayMsg {
    Packet { user_id: u64, seq: u16, opus: Vec<u8> },
    Volume(u64, f32),
    Muted(u64, bool),
    /// Son court mixé à la sortie (phase 6) à partir de la trame `pos` ;
    /// `deafen` : coupé par la sourdine (sons du tableau).
    Clip { id: u64, pcm: Pcm, pos: usize, volume: f32, deafen: bool },
    StopClip(u64),
}

/// Sons courts joués en même temps, au plus (le plus ancien cède sa place).
const MAX_CLIPS: usize = 16;

struct Playing {
    id: u64,
    pcm: Pcm,
    pos: usize,
    volume: f32,
    deafen: bool,
}

/// Ajoute les sons courts à `out` (stéréo entrelacé) ; ceux qui sont finis partent.
fn mix_clips(clips: &mut Vec<Playing>, out: &mut [f32], deafened: bool) {
    for c in clips.iter_mut() {
        let silent = c.deafen && deafened;
        for lr in out.as_chunks_mut::<2>().0 {
            if !silent {
                let [l, r] = c.pcm.stereo(c.pos);
                lr[0] += l * c.volume;
                lr[1] += r * c.volume;
            }
            c.pos += 1;
        }
    }
    clips.retain(|c| c.pos < c.pcm.frames());
}

/// Des fils audio vers la tâche de l'appel.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum AudioUp {
    /// J'émets, ou plus.
    Me(bool),
    /// Qui parle parmi les autres (trié).
    Others(Vec<u64>),
}

/// La chaîne de traitement, réglée depuis l'interface sans verrou.
struct Tunable {
    chain: VoiceChain,
    shared: Arc<Shared>,
    bits: u16,
}

impl Processor for Tunable {
    fn process(&mut self, mono: &mut [f32], far_end: &[f32]) {
        let b = self.shared.dsp.load(Relaxed);
        if b != self.bits {
            self.bits = b;
            self.chain.set_settings(DspSettings::from_bits(b));
        }
        self.chain.process(mono, far_end);
    }
}

/// Rappel de capture.
fn capture_fn(shared: Arc<Shared>, net: tmpsc::Sender<VoiceCtl>, up: tmpsc::UnboundedSender<AudioUp>, far: Arc<Ring>) -> CaptureFn {
    let mut eng = Capture::new();
    let bits = shared.dsp.load(Relaxed);
    eng.set_processor(Some(Box::new(Tunable { chain: VoiceChain::new(DspSettings::from_bits(bits)), shared: shared.clone(), bits })));
    let mut gate = Gate::default();
    eng.set_gate(gate);
    let mut muted = false;
    // Échantillons qui restent à émettre après le relâchement de l'appui.
    let mut release_left = 0usize;
    let mut spoke = false;
    let mut buf = vec![0.0f32; CAP_CHUNK];
    let mut rec_seen = 0u32;
    let s = shared;
    Box::new(move |mono: &[f32]| {
        if let Some(tx) = s.rec_changed(&mut rec_seen) {
            eng.set_tap(tx.map(|tx| {
                Box::new(move |pcm: &[f32]| {
                    let _ = tx.send(RecMsg::Mic(pcm.to_vec()));
                }) as MicTap
            }));
        }
        // Touche tenue dans la fenêtre, ou n'importe où (crochet clavier global).
        let down = s.ptt.load(Relaxed) || super::ptt_hook::pressed();
        if down {
            release_left = s.release_ms.load(Relaxed) as usize * RATE_PER_MS;
        }
        let want = if s.voice_mode.load(Relaxed) {
            Gate::Voice { auto: s.auto.load(Relaxed), threshold_db: load(&s.threshold) }
        } else {
            Gate::PushToTalk { pressed: down || release_left > 0 }
        };
        if want != gate {
            gate = want;
            eng.set_gate(gate);
        }
        let m = s.muted.load(Relaxed);
        if m != muted {
            muted = m;
            eng.set_muted(m);
        }
        let gain = load(&s.input_gain);
        for chunk in mono.chunks(CAP_CHUNK) {
            let b = &mut buf[..chunk.len()];
            for (d, x) in b.iter_mut().zip(chunk) {
                *d = x * gain;
            }
            let frames = eng.push_with(b, |f| {
                far.trim_to(MAX_FAR);
                let n = far.pop(f);
                f[n..].fill(0.0);
            });
            for f in frames {
                // Les silences de fin partent avec l'arrêt de l'émission (connexion vocale).
                if f.as_slice() != SILENCE {
                    // File pleine (réseau en retard) : la trame est perdue plutôt que d'attendre.
                    let _ = net.try_send(VoiceCtl::Opus(f));
                }
            }
        }
        if !down {
            release_left = release_left.saturating_sub(mono.len());
        }
        let now = eng.speaking_now();
        if now != spoke {
            spoke = now;
            if !now {
                let _ = net.try_send(VoiceCtl::Speaking(false));
            }
            let _ = up.send(AudioUp::Me(now));
        }
    })
}

/// Rappel de lecture.
fn playback_fn(shared: Arc<Shared>, rx: mpsc::Receiver<PlayMsg>, up: tmpsc::UnboundedSender<AudioUp>, far: Arc<Ring>) -> PlaybackFn {
    let mut eng = Playback::new();
    let mut mono = vec![0.0f32; PLAY_CHUNK];
    let mut since = 0usize;
    let mut speaking: Vec<u64> = Vec::new();
    let mut clips: Vec<Playing> = Vec::with_capacity(MAX_CLIPS);
    let mut eq = 0u32;
    let mut rec_seen = 0u32;
    let s = shared;
    Box::new(move |out: &mut [f32]| {
        if let Some(tx) = s.rec_changed(&mut rec_seen) {
            eng.set_tap(tx.map(|tx| {
                Box::new(move |who: Option<u64>, stereo: &[f32]| {
                    let _ = tx.send(match who {
                        Some(user) => RecMsg::Peer(user, stereo.as_chunks::<2>().0.iter().map(|s| (s[0] + s[1]) * 0.5).collect()),
                        None => RecMsg::Tick,
                    });
                }) as PeerTap
            }));
        }
        let e = s.eq.load(Relaxed);
        if e != eq {
            eq = e;
            eng.set_eq(crate::audio::dsp::eq::unpack(e));
        }
        while let Ok(m) = rx.try_recv() {
            match m {
                PlayMsg::Packet { user_id, seq, opus } => eng.receive(user_id, seq, 0, &opus),
                PlayMsg::Volume(u, v) => eng.set_user_volume(u, v),
                PlayMsg::Muted(u, b) => eng.set_user_muted(u, b),
                PlayMsg::Clip { id, pcm, pos, volume, deafen } => {
                    if clips.len() == MAX_CLIPS {
                        clips.remove(0);
                    }
                    clips.push(Playing { id, pcm, pos, volume, deafen });
                }
                PlayMsg::StopClip(id) => clips.retain(|c| c.id != id),
            }
        }
        let deafened = s.deafened.load(Relaxed);
        eng.set_deafened(deafened);
        eng.pull_playback(out);
        mix_clips(&mut clips, out, deafened);
        super::player::mix(out);
        let gain = load(&s.output_gain);
        for chunk in out.chunks_mut(2 * PLAY_CHUNK) {
            let frames = chunk.len() / 2;
            for (m, lr) in mono.iter_mut().zip(chunk.as_chunks_mut::<2>().0) {
                lr[0] = (lr[0] * gain).clamp(-1.0, 1.0);
                lr[1] = (lr[1] * gain).clamp(-1.0, 1.0);
                *m = (lr[0] + lr[1]) * 0.5;
            }
            far.push(&mono[..frames]);
        }
        since += out.len() / 2;
        if since >= SPEAKING_EVERY {
            since = 0;
            let now = eng.speaking_users();
            if now != speaking {
                speaking = now.clone();
                let _ = up.send(AudioUp::Others(now));
            }
        }
    })
}

/// Périphériques ouverts pour un appel.
pub(super) struct Media {
    streams: Option<Box<dyn AudioStreams>>,
    play: mpsc::Sender<PlayMsg>,
    pub(super) input: Option<String>,
    pub(super) output: Option<String>,
}

impl Media {
    /// Ouvre les périphériques choisis (peut prendre une seconde : hors de
    /// la boucle de l'appel). `users` : volumes et muets réglés par personne.
    pub(super) fn open(
        backend: &dyn AudioBackend,
        prefs: &AudioPrefs,
        shared: Arc<Shared>,
        net: tmpsc::Sender<VoiceCtl>,
        up: tmpsc::UnboundedSender<AudioUp>,
        users: &HashMap<u64, (f32, bool)>,
    ) -> (Media, Option<AudioError>) {
        let (tx, rx) = mpsc::channel();
        for (u, (v, m)) in users {
            let _ = tx.send(PlayMsg::Volume(*u, *v));
            let _ = tx.send(PlayMsg::Muted(*u, *m));
        }
        let far = Arc::new(Ring::new(48_000));
        let capture = capture_fn(shared.clone(), net, up.clone(), far.clone());
        let playback = playback_fn(shared, rx, up, far);
        let (input, output) = (prefs.input_device.clone(), prefs.output_device.clone());
        let (streams, err) = match backend.open(input.as_deref(), output.as_deref(), capture, playback) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        (Media { streams, play: tx, input, output }, err)
    }

    /// Sortie seule, hors appel (son d'un direct regardé, sons courts) : le
    /// micro n'est pas ouvert. Peut prendre une seconde.
    pub(super) fn open_output(backend: &dyn AudioBackend, prefs: &AudioPrefs, shared: Arc<Shared>) -> (Media, Option<AudioError>) {
        let (tx, rx) = mpsc::channel();
        // Personne n'écoute « qui parle » hors appel.
        let (up, _) = tmpsc::unbounded_channel();
        let playback = playback_fn(shared, rx, up, Arc::new(Ring::new(MAX_FAR)));
        let output = prefs.output_device.clone();
        let (streams, err) = match backend.open_output(output.as_deref(), playback) {
            Ok(s) => (Some(s), None),
            Err(e) => (None, Some(e)),
        };
        (Media { streams, play: tx, input: None, output }, err)
    }

    /// Aucun périphérique ouvert.
    pub(super) fn closed() -> Media {
        let (play, _) = mpsc::channel();
        Media { streams: None, play, input: None, output: None }
    }

    pub(super) fn send(&self, m: PlayMsg) {
        let _ = self.play.send(m);
    }

    /// File du fil de lecture (le son des directs regardés y passe aussi).
    pub(super) fn player(&self) -> mpsc::Sender<PlayMsg> {
        self.play.clone()
    }

    pub(super) fn take_error(&self) -> Option<AudioError> {
        self.streams.as_ref()?.take_error()
    }

    pub(super) fn is_open(&self) -> bool {
        self.streams.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::engine::{Engine, FRAME};
    use crate::voice::Settings;

    fn sine(n: usize, amp: f32) -> Vec<f32> {
        (0..n).map(|i| amp * (i as f32 * 0.05).sin()).collect()
    }

    fn settings(f: impl FnOnce(&mut AudioPrefs)) -> Settings {
        let mut prefs = AudioPrefs { noise_suppression: false, echo_cancellation: false, auto_gain: false, auto_threshold: false, threshold_db: -40, ..Default::default() };
        f(&mut prefs);
        Settings { muted: false, deafened: false, prefs, extras: Default::default() }
    }

    struct Rig {
        shared: Arc<Shared>,
        cap: CaptureFn,
        net: tmpsc::Receiver<VoiceCtl>,
        up: tmpsc::UnboundedReceiver<AudioUp>,
    }

    fn rig(s: &Settings) -> Rig {
        let shared = Arc::new(Shared::new(s, false));
        let (ntx, net) = tmpsc::channel(512);
        let (utx, up) = tmpsc::unbounded_channel();
        let cap = capture_fn(shared.clone(), ntx, utx, Arc::new(Ring::new(48_000)));
        Rig { shared, cap, net, up }
    }

    fn drain(r: &mut Rig) -> Vec<VoiceCtl> {
        let mut v = Vec::new();
        while let Ok(c) = r.net.try_recv() {
            v.push(c);
        }
        v
    }

    #[test]
    fn capture_sends_opus_then_stop_without_silences() {
        let mut r = rig(&settings(|_| {}));
        (r.cap)(&sine(FRAME * 10, 0.5));
        let sent = drain(&mut r);
        assert_eq!(sent.len(), 10);
        assert!(sent.iter().all(|c| matches!(c, VoiceCtl::Opus(f) if f.len() > 3)));
        assert_eq!(r.up.try_recv().ok(), Some(AudioUp::Me(true)));
        // Silence au-delà du maintien : fin de l'émission, sans trames de silence ici.
        (r.cap)(&vec![0.0; FRAME * 30]);
        let sent = drain(&mut r);
        assert_eq!(sent.last(), Some(&VoiceCtl::Speaking(false)));
        assert!(sent.iter().all(|c| !matches!(c, VoiceCtl::Opus(f) if f.as_slice() == SILENCE)));
        assert_eq!(r.up.try_recv().ok(), Some(AudioUp::Me(false)));
    }

    #[test]
    fn muted_and_input_volume() {
        let mut r = rig(&settings(|_| {}));
        r.shared.set(&Settings { muted: true, ..settings(|_| {}) });
        (r.cap)(&sine(FRAME * 5, 0.5));
        assert!(drain(&mut r).is_empty());
        // Volume d'entrée à 0 : la voix ne passe plus la porte.
        let mut r = rig(&settings(|p| p.input_volume = 0));
        (r.cap)(&sine(FRAME * 5, 0.5));
        assert!(drain(&mut r).is_empty());
    }

    #[test]
    fn push_to_talk_with_release_delay() {
        let s = settings(|p| {
            p.mode = InputMode::PushToTalk;
            p.ptt_release_ms = 100;
        });
        let mut r = rig(&s);
        let quiet = vec![0.0f32; FRAME];
        (r.cap)(&quiet);
        assert!(drain(&mut r).is_empty(), "touche relâchée : rien");
        r.shared.set_ptt(true);
        (r.cap)(&sine(FRAME * 2, 0.5));
        assert_eq!(drain(&mut r).len(), 2);
        r.shared.set_ptt(false);
        // 100 ms de délai : encore 5 trames (par rappels de 10 ms), puis l'arrêt.
        let mut opus = 0;
        let mut stopped = false;
        for _ in 0..20 {
            (r.cap)(&sine(FRAME / 2, 0.5));
            for c in drain(&mut r) {
                match c {
                    VoiceCtl::Opus(_) => opus += 1,
                    VoiceCtl::Speaking(false) => stopped = true,
                    _ => {}
                }
            }
        }
        assert!((4..=6).contains(&opus), "{opus}");
        assert!(stopped);
    }

    #[test]
    fn clips_are_mixed_with_their_volume_and_the_output_volume() {
        let s = settings(|p| p.output_volume = 50);
        let shared = Arc::new(Shared::new(&s, false));
        let (tx, rx) = mpsc::channel();
        let (utx, _up) = tmpsc::unbounded_channel();
        let mut play = playback_fn(shared.clone(), rx, utx, Arc::new(Ring::new(48_000)));
        let pcm = Pcm { channels: 1, data: vec![16384i16; 1500].into() };
        let _ = tx.send(PlayMsg::Clip { id: 1, pcm: pcm.clone(), pos: 500, volume: 0.5, deafen: true });
        let mut out = vec![0.0f32; FRAME * 2];
        play(&mut out);
        // 0,5 (son) × 0,5 (volume du son) × 0,5 (volume de sortie), sur 1 000 trames restantes.
        assert!((out[0] - 0.125).abs() < 1e-3 && (out[1] - 0.125).abs() < 1e-3, "{}", out[0]);
        play(&mut out);
        assert!((out[2 * 39] - 0.125).abs() < 1e-3 && out[2 * 40] == 0.0, "fin du son");
        // Sourdine : un son du tableau se tait, un message vocal non.
        shared.set(&Settings { deafened: true, ..s });
        let _ = tx.send(PlayMsg::Clip { id: 2, pcm: pcm.clone(), pos: 0, volume: 1.0, deafen: true });
        let _ = tx.send(PlayMsg::Clip { id: 3, pcm, pos: 0, volume: 1.0, deafen: false });
        play(&mut out);
        assert!((out[0] - 0.25).abs() < 1e-3, "{}", out[0]);
        let _ = tx.send(PlayMsg::StopClip(3));
        play(&mut out);
        assert!(out.iter().all(|x| *x == 0.0));
    }

    /// Enregistrement local, de bout en bout sans périphérique : les deux
    /// rappels audio alimentent le fil d'écriture, et le fichier se relit.
    /// Sans enregistrement, aucune prise n'est posée ; après l'arrêt, plus
    /// rien ne part.
    #[test]
    fn recording_taps_both_audio_threads_and_the_file_reads_back() {
        use crate::state::call_rec::CallRecUp;
        let s = settings(|p| p.mode = InputMode::PushToTalk);
        let shared = Arc::new(Shared::new(&s, true));
        let (ntx, _net) = tmpsc::channel(512);
        let (utx, _up) = tmpsc::unbounded_channel();
        let far = Arc::new(Ring::new(48_000));
        let mut cap = capture_fn(shared.clone(), ntx, utx.clone(), far.clone());
        let (ptx, prx) = mpsc::channel();
        let mut play = playback_fn(shared.clone(), prx, utx, far);
        let mut enc = Engine::new();
        enc.set_gate(Gate::PushToTalk { pressed: true });
        let pk: Vec<Vec<u8>> = enc.push_capture(&sine(FRAME * 100, 0.3));
        let mut out = vec![0.0f32; FRAME * 2];
        let mut seq = 0u16;
        let mut second = |cap: &mut CaptureFn, play: &mut PlaybackFn| {
            for p in pk.iter().take(50) {
                cap(&sine(FRAME, 0.2));
                let _ = ptx.send(PlayMsg::Packet { user_id: 9, seq, opus: p.clone() });
                seq = seq.wrapping_add(1);
                play(&mut out);
            }
        };

        // Avant : rien n'est posé, rien ne part.
        second(&mut cap, &mut play);
        let dir = std::env::temp_dir().join(format!("ferrocord-test-enr-appel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let (rtx, rrx) = mpsc::channel();
        let cfg = crate::voice::recorder::Config { dir: dir.clone(), base: "appel".into(), tracks: true, me: 1, names: vec![(9, "Neuf".into())], me_label: "moi".into(), stream_label: "direct".into() };
        shared.set_rec(Some(rtx.clone()));
        second(&mut cap, &mut play);
        shared.set_rec(None);
        let _ = rtx.send(RecMsg::Stop);
        // Après l'arrêt : les rappels retirent leur prise et n'envoient plus rien.
        second(&mut cap, &mut play);
        drop(rtx);
        let queued: Vec<RecMsg> = rrx.try_iter().collect();
        let stop = queued.iter().position(|m| matches!(m, RecMsg::Stop)).unwrap();
        assert_eq!(stop, queued.len() - 1, "rien après l'arrêt");
        assert_eq!(queued.iter().filter(|m| matches!(m, RecMsg::Tick)).count(), 50);
        assert_eq!(queued.iter().filter(|m| matches!(m, RecMsg::Mic(_))).count(), 50);

        let (tx, rx) = mpsc::channel();
        for m in queued {
            let _ = tx.send(m);
        }
        let mut up = None;
        crate::voice::recorder::run(cfg, rx, |u| up = Some(u));
        let Some(CallRecUp::Done { file, files, ms }) = up else { panic!("{up:?}") };
        assert_eq!((files, ms), (3, 1000));
        for path in [file, dir.join("appel - Neuf.ogg"), dir.join("appel - moi.ogg")] {
            let pcm = crate::audio::clip::decode(std::fs::read(&path).unwrap()).unwrap();
            let x: Vec<f32> = pcm.data.iter().map(|v| f32::from(*v) / 32768.0).collect();
            let rms = (x[14_400..43_200].iter().map(|v| v * v).sum::<f32>() / 28_800.0).sqrt();
            assert!(pcm.duration_ms().abs_diff(1000) <= 20 && rms > 0.05, "{} : {} ms, niveau {rms}", path.display(), pcm.duration_ms());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Égaliseur : à plat (plugin coupé), le son joué est le même au bit près ;
    /// réglé, il change, et suit un changement fait pendant l'appel.
    #[test]
    fn equalizer_follows_the_shared_setting() {
        let mut enc = Engine::new();
        enc.set_gate(Gate::PushToTalk { pressed: true });
        // Un sinus grave (99 Hz), dans la bande du plateau des graves.
        let low: Vec<f32> = (0..FRAME * 30).map(|i| 0.25 * (i as f32 * 0.013).sin()).collect();
        let pk: Vec<Vec<u8>> = enc.push_capture(&low);
        let run = |first: [i8; 3], then: [i8; 3]| {
            let s = Settings { extras: crate::audio::dsp::Extras { eq: first, ..Default::default() }, ..settings(|_| {}) };
            let shared = Arc::new(Shared::new(&s, false));
            let (tx, rx) = mpsc::channel();
            let (utx, _up) = tmpsc::unbounded_channel();
            let mut play = playback_fn(shared.clone(), rx, utx, Arc::new(Ring::new(48_000)));
            let mut all = Vec::new();
            let mut out = vec![0.0f32; FRAME * 2];
            for (i, p) in pk.iter().enumerate() {
                if i == 15 {
                    shared.set(&Settings { extras: crate::audio::dsp::Extras { eq: then, ..Default::default() }, ..s.clone() });
                }
                let _ = tx.send(PlayMsg::Packet { user_id: 9, seq: i as u16, opus: p.clone() });
                play(&mut out);
                all.extend_from_slice(&out);
            }
            all
        };
        let flat = run([0; 3], [0; 3]);
        let half = FRAME * 2 * 15;
        let late = run([0; 3], [9, 0, 0]);
        assert_eq!(flat[..half], late[..half], "avant le réglage : rien ne change");
        assert_ne!(flat[half..], late[half..], "réglé pendant l'appel");
        let boosted = run([9, 0, 0], [9, 0, 0]);
        let rms = |x: &[f32]| (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt();
        let gain = rms(&boosted[half..]) / rms(&flat[half..]);
        // +9 dB de graves : environ 2,8 fois plus fort.
        assert!(gain > 2.0 && gain < 3.2, "gain {gain}");
        assert!(boosted.iter().all(|v| v.abs() < 1.0));
    }

    #[test]
    fn playback_mixes_with_output_volume_deafen_and_reports_speakers() {
        let s = settings(|p| p.output_volume = 50);
        let shared = Arc::new(Shared::new(&s, false));
        let (tx, rx) = mpsc::channel();
        let (utx, mut up) = tmpsc::unbounded_channel();
        let far = Arc::new(Ring::new(48_000));
        let mut play = playback_fn(shared.clone(), rx, utx, far.clone());
        let mut enc = Engine::new();
        enc.set_gate(Gate::PushToTalk { pressed: true });
        let pk: Vec<Vec<u8>> = enc.push_capture(&sine(FRAME * 20, 0.5));
        let mut out = vec![0.0f32; FRAME * 2];
        let mut peak = 0.0f32;
        for (i, p) in pk.iter().enumerate() {
            let _ = tx.send(PlayMsg::Packet { user_id: 9, seq: i as u16, opus: p.clone() });
            play(&mut out);
            peak = out.iter().fold(peak, |m, x| m.max(x.abs()));
        }
        assert!(peak > 0.1 && peak <= 0.3, "volume de sortie 50 % : {peak}");
        assert!(!far.is_empty(), "référence d'écho remplie");
        assert_eq!(up.try_recv().ok(), Some(AudioUp::Others(vec![9])));
        shared.set(&Settings { deafened: true, ..s });
        for (i, p) in pk.iter().enumerate() {
            let _ = tx.send(PlayMsg::Packet { user_id: 9, seq: (100 + i) as u16, opus: p.clone() });
            play(&mut out);
        }
        assert!(out.iter().all(|x| *x == 0.0), "sourdine");
    }
}
