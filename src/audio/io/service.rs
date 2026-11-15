//! Service audio réel de la page « Voix et vidéo » (trait
//! `state::audio_prefs::AudioService`) : liste des périphériques dans un fil
//! éphémère, et « Test du micro » : la voix captée passe par le volume
//! d'entrée, la porte de détection et le traitement, puis revient dans la
//! sortie choisie. Hors du test, aucun fil ne tourne.

use super::ring::Ring;
use super::{inputs, open, outputs, Streams};
use crate::audio::dsp::level::{rms_db, VoiceGate, FLOOR_DB};
use crate::audio::dsp::{DspSettings, VoiceChain};
use crate::audio::Processor;
use crate::state::audio_prefs::{AudioEvent, AudioService, InputMode, Meter, AudioPrefs};
use std::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Mutex};

const FRAME: usize = 960;
/// Retard de l'écoute de soi borné à 100 ms (on jette le surplus).
const MAX_LOOP: usize = 4_800;
/// Son joué gardé pour l'annulation d'écho : trois trames au plus.
const MAX_FAR: usize = 3 * FRAME;
const PLAY_CHUNK: usize = 2_048;

/// Réglages lus par les fils audio à chaque trame (sans verrou).
struct Shared {
    input_gain: AtomicU32,
    output_gain: AtomicU32,
    dsp: AtomicU16,
    voice_mode: AtomicBool,
    auto: AtomicBool,
    threshold: AtomicU32,
    level: AtomicU32,
    gate_threshold: AtomicU32,
    open: AtomicBool,
}

impl Shared {
    fn new(p: &AudioPrefs) -> Self {
        let s = Shared {
            input_gain: AtomicU32::new(0),
            output_gain: AtomicU32::new(0),
            dsp: AtomicU16::new(0),
            voice_mode: AtomicBool::new(true),
            auto: AtomicBool::new(true),
            threshold: AtomicU32::new(0),
            level: AtomicU32::new(FLOOR_DB.to_bits()),
            gate_threshold: AtomicU32::new(0),
            open: AtomicBool::new(false),
        };
        s.set(p);
        s
    }

    fn set(&self, p: &AudioPrefs) {
        self.input_gain.store(p.input_gain().to_bits(), Relaxed);
        self.output_gain.store(p.output_gain().to_bits(), Relaxed);
        // Plugins vocaux (RNNoise) : le test fait entendre ce que l'appel enverra.
        self.dsp.store(p.dsp().with(crate::audio::dsp::Extras::current()).bits(), Relaxed);
        self.voice_mode.store(p.mode == InputMode::Voice, Relaxed);
        self.auto.store(p.auto_threshold, Relaxed);
        self.threshold.store(f32::from(p.threshold_db).to_bits(), Relaxed);
    }

    fn load(a: &AtomicU32) -> f32 {
        f32::from_bits(a.load(Relaxed))
    }
}

#[derive(Default)]
struct Slot {
    generation: u64,
    streams: Option<Streams>,
}

pub struct DeviceService {
    wake: Arc<dyn Fn() + Send + Sync>,
    tx: mpsc::Sender<AudioEvent>,
    rx: mpsc::Receiver<AudioEvent>,
    slot: Arc<Mutex<Slot>>,
    shared: Option<Arc<Shared>>,
}

impl DeviceService {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        DeviceService { wake, tx, rx, slot: Arc::default(), shared: None }
    }

    /// Ferme le test en cours (la fermeture attend les fils audio : faite à part).
    fn close(&mut self) -> u64 {
        self.shared = None;
        let mut slot = self.slot.lock().unwrap_or_else(|p| p.into_inner());
        slot.generation += 1;
        if let Some(s) = slot.streams.take() {
            let _ = std::thread::Builder::new().name("ferrocord-audio-stop".into()).spawn(move || drop(s));
        }
        slot.generation
    }
}

/// Rappels du test : capture → volume, porte, traitement → file → lecture.
fn loopback(shared: Arc<Shared>, prefs: &AudioPrefs) -> (super::CaptureFn, super::PlaybackFn) {
    let to_ear = Arc::new(Ring::new(48_000));
    let played = Arc::new(Ring::new(48_000));
    let (ear_in, played_out) = (to_ear.clone(), played.clone());
    let mut dsp = shared.dsp.load(Relaxed);
    let mut chain = VoiceChain::new(DspSettings::from_bits(dsp));
    let mut gate = VoiceGate::new(prefs.auto_threshold, f32::from(prefs.threshold_db));
    let mut frame = vec![0.0f32; FRAME];
    let mut far = vec![0.0f32; FRAME];
    let mut filled = 0;
    let s = shared.clone();
    let capture = move |mono: &[f32]| {
        for x in mono {
            frame[filled] = *x;
            filled += 1;
            if filled < FRAME {
                continue;
            }
            filled = 0;
            let gain = Shared::load(&s.input_gain);
            frame.iter_mut().for_each(|v| *v *= gain);
            let bits = s.dsp.load(Relaxed);
            if bits != dsp {
                dsp = bits;
                chain.set_settings(DspSettings::from_bits(bits));
            }
            played.trim_to(MAX_FAR);
            let n = played.pop(&mut far);
            far[n..].iter_mut().for_each(|v| *v = 0.0);
            // Même ordre que l'appel (`audio::engine::Capture`) : traitement,
            // puis la porte sur la voix traitée, pour que le seuil réglé ici
            // vaille tel quel en appel.
            chain.process(&mut frame, &far);
            let level = rms_db(&frame);
            gate.auto = s.auto.load(Relaxed);
            gate.threshold_db = Shared::load(&s.threshold);
            let open = gate.update(level) || !s.voice_mode.load(Relaxed);
            s.level.store(level.to_bits(), Relaxed);
            s.gate_threshold.store(gate.threshold().to_bits(), Relaxed);
            s.open.store(open, Relaxed);
            if !open {
                frame.iter_mut().for_each(|v| *v = 0.0);
            }
            to_ear.push(&frame);
        }
    };
    let mut mono = vec![0.0f32; PLAY_CHUNK];
    let playback = move |stereo: &mut [f32]| {
        ear_in.trim_to(MAX_LOOP);
        let gain = Shared::load(&shared.output_gain);
        for out in stereo.chunks_mut(2 * PLAY_CHUNK) {
            let frames = out.len() / 2;
            let n = ear_in.pop(&mut mono[..frames]);
            mono[n..frames].iter_mut().for_each(|v| *v = 0.0);
            for (o, v) in out.as_chunks_mut::<2>().0.iter_mut().zip(mono.iter_mut()) {
                *v = (*v * gain).clamp(-1.0, 1.0);
                o[0] = *v;
                o[1] = *v;
            }
            played_out.push(&mono[..frames]);
        }
    };
    (Box::new(capture), Box::new(playback))
}

impl AudioService for DeviceService {
    fn list_devices(&mut self) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("ferrocord-audio-list".into()).spawn(move || {
            let _ = tx.send(AudioEvent::Devices { inputs: inputs(), outputs: outputs() });
            wake();
        });
    }

    fn start_test(&mut self, prefs: &AudioPrefs) {
        let generation = self.close();
        let shared = Arc::new(Shared::new(prefs));
        self.shared = Some(shared.clone());
        let (slot, tx, wake, prefs) = (self.slot.clone(), self.tx.clone(), self.wake.clone(), prefs.clone());
        // L'ouverture peut prendre une seconde (pilotes Bluetooth) : hors du fil d'interface.
        let _ = std::thread::Builder::new().name("ferrocord-audio-open".into()).spawn(move || {
            let (capture, playback) = loopback(shared, &prefs);
            let opened = open(prefs.input_device.as_deref(), prefs.output_device.as_deref(), capture, playback);
            let mut slot = slot.lock().unwrap_or_else(|p| p.into_inner());
            match opened {
                Ok(s) if slot.generation == generation => slot.streams = Some(s),
                // Arrêté entre-temps.
                Ok(s) => {
                    drop(slot);
                    drop(s);
                }
                Err(e) if slot.generation == generation => {
                    let _ = tx.send(AudioEvent::TestError(e));
                    wake();
                }
                Err(_) => {}
            }
        });
    }

    fn update_test(&mut self, prefs: &AudioPrefs) {
        if let Some(s) = &self.shared {
            s.set(prefs);
        }
    }

    fn stop_test(&mut self) {
        self.close();
    }

    fn testing(&self) -> bool {
        self.shared.is_some()
    }

    fn meter(&self) -> Option<Meter> {
        let s = self.shared.as_ref()?;
        Some(Meter { level_db: Shared::load(&s.level), threshold_db: Shared::load(&s.gate_threshold), open: s.open.load(Relaxed) })
    }

    fn poll(&mut self) -> Vec<AudioEvent> {
        let mut out: Vec<AudioEvent> = self.rx.try_iter().collect();
        if let Ok(slot) = self.slot.try_lock()
            && let Some(e) = slot.streams.as_ref().and_then(Streams::take_error)
        {
            out.push(AudioEvent::TestError(e));
        }
        // Un test qui n'a pas pu s'ouvrir n'est plus en cours.
        if out.iter().any(|e| matches!(e, AudioEvent::TestError(_))) && self.slot.try_lock().is_ok_and(|s| s.streams.is_none()) {
            self.shared = None;
        }
        out
    }
}

impl Drop for DeviceService {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Le chemin du test sans périphérique : la voix captée ressort, au
    /// volume de sortie, et la porte se ferme sur le silence.
    #[test]
    fn loopback_hears_the_voice_and_gates_silence() {
        let prefs = AudioPrefs { noise_suppression: false, echo_cancellation: false, auto_gain: false, auto_threshold: false, threshold_db: -40, output_volume: 50, ..Default::default() };
        let shared = Arc::new(Shared::new(&prefs));
        let (mut capture, mut playback) = loopback(shared.clone(), &prefs);
        let tone: Vec<f32> = (0..FRAME * 4).map(|i| 0.5 * (i as f32 * 0.1).sin()).collect();
        capture(&tone);
        assert!(shared.open.load(Relaxed));
        assert!(Shared::load(&shared.level) > -10.0);
        let mut out = vec![0.0f32; 2 * FRAME];
        playback(&mut out);
        let peak = out.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        assert!(peak > 0.2 && peak <= 0.3, "volume de sortie à 50 % : {peak}");
        assert!(out.chunks(2).all(|f| f[0] == f[1]));

        // Silence au-delà du maintien de la porte : plus rien n'est rendu.
        let silence = vec![0.0f32; FRAME * 30];
        capture(&silence);
        assert!(!shared.open.load(Relaxed));
        let mut out = vec![1.0f32; 2 * FRAME];
        for _ in 0..40 {
            playback(&mut out);
        }
        assert!(out.iter().all(|v| *v == 0.0));
    }
}
