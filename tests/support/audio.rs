//! Faux périphérique audio pour l'orchestrateur du vocal : un fil qui, toutes
//! les 10 ms, donne au rappel de capture un sinus (d'amplitude réglable) et
//! demande 10 ms au rappel de lecture, dont il retient la crête. Aucun vrai
//! périphérique n'est ouvert.

#![allow(dead_code)]

use ferrocord::audio::io::{AudioError, CaptureFn, PlaybackFn};
use ferrocord::voice::{AudioBackend, AudioStreams};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::SeqCst};
use std::sync::Arc;
use std::time::Duration;

const CHUNK: usize = 480;

#[derive(Default)]
pub struct FakeAudio {
    /// Amplitude du « micro » (bits d'un f32).
    mic: AtomicU32,
    /// Plus grande valeur jouée depuis la dernière remise à zéro (bits d'un f32).
    peak: AtomicU32,
    /// Flux ouverts en ce moment, et ouverts depuis le début.
    pub open_now: AtomicUsize,
    pub opened: AtomicUsize,
}

impl FakeAudio {
    pub fn new() -> Arc<Self> {
        Arc::new(FakeAudio::default())
    }

    pub fn set_mic(&self, amp: f32) {
        self.mic.store(amp.to_bits(), SeqCst);
    }

    /// Crête jouée depuis le dernier appel (remise à zéro).
    pub fn take_peak(&self) -> f32 {
        f32::from_bits(self.peak.swap(0, SeqCst))
    }
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    dev: Arc<FakeAudio>,
}

impl AudioStreams for Running {
    fn take_error(&self) -> Option<AudioError> {
        None
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.dev.open_now.fetch_sub(1, SeqCst);
    }
}

/// Le faux périphérique, partagé avec le test.
pub struct Backend(pub Arc<FakeAudio>);

impl AudioBackend for Backend {
    fn open(&self, _input: Option<&str>, _output: Option<&str>, mut capture: CaptureFn, mut playback: PlaybackFn) -> Result<Box<dyn AudioStreams>, AudioError> {
        let dev = self.0.clone();
        dev.open_now.fetch_add(1, SeqCst);
        dev.opened.fetch_add(1, SeqCst);
        let stop = Arc::new(AtomicBool::new(false));
        let (s, d) = (stop.clone(), dev.clone());
        let thread = std::thread::spawn(move || {
            let mut phase = 0usize;
            let mut mic = vec![0.0f32; CHUNK];
            let mut out = vec![0.0f32; CHUNK * 2];
            while !s.load(SeqCst) {
                let amp = f32::from_bits(d.mic.load(SeqCst));
                for m in mic.iter_mut() {
                    *m = amp * (phase as f32 * 2.0 * std::f32::consts::PI * 300.0 / 48_000.0).sin();
                    phase += 1;
                }
                capture(&mic);
                playback(&mut out);
                let peak = out.iter().fold(0.0f32, |m, x| m.max(x.abs()));
                let _ = d.peak.try_update(SeqCst, SeqCst, |p| Some(f32::from_bits(p).max(peak).to_bits()));
                std::thread::sleep(Duration::from_millis(10));
            }
        });
        Ok(Box::new(Running { stop, thread: Some(thread), dev }))
    }
}
