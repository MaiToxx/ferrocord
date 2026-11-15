//! Enregistrement d'un message vocal : mono à 48 kHz → Opus 32 kbit/s par
//! trames de 20 ms → Ogg ; crête toutes les 100 ms pour l'onde. C'est ce
//! qu'envoient les clients officiels (docs.discord.food, « Voice Messages » :
//! « 1 channel, 48000 Hz, 32 kbps Opus stream in an OGG container »).

use super::{ogg, waveform_from_peaks, WAVEFORM_EVERY_MS};
use crate::audio::engine::{FRAME, MAX_PACKET, SAMPLE_RATE};
use opus::{Application, Bitrate, Channels, Encoder, Signal};

/// Retard de l'encodeur à 48 kHz (6,5 ms), à jeter à la lecture.
const PRE_SKIP: u16 = 312;
/// Enregistrement le plus long (10 min) : au-delà, la suite est ignorée.
pub const MAX_MS: u32 = 600_000;
const PEAK_EVERY: usize = (SAMPLE_RATE * WAVEFORM_EVERY_MS / 1000) as usize;

/// Message vocal prêt à partir.
#[derive(Clone, Debug, PartialEq)]
pub struct Recorded {
    pub ogg: Vec<u8>,
    pub duration_ms: u32,
    /// Un octet par point, 256 points au plus.
    pub waveform: Vec<u8>,
}

pub struct Recorder {
    enc: Encoder,
    frame: Vec<f32>,
    packets: Vec<Vec<u8>>,
    out: Vec<u8>,
    peaks: Vec<f32>,
    peak: f32,
    peak_n: usize,
}

impl Recorder {
    pub fn new() -> Option<Self> {
        let mut enc = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).ok()?;
        let _ = enc.set_bitrate(Bitrate::Bits(32_000));
        let _ = enc.set_signal(Signal::Voice);
        Some(Recorder { enc, frame: Vec::with_capacity(FRAME), packets: Vec::new(), out: vec![0; MAX_PACKET], peaks: Vec::new(), peak: 0.0, peak_n: 0 })
    }

    pub fn elapsed_ms(&self) -> u32 {
        (self.packets.len() * FRAME * 1000 / SAMPLE_RATE as usize) as u32
    }

    fn full(&self) -> bool {
        self.elapsed_ms() >= MAX_MS
    }

    /// Ajoute de la capture (mono, 48 kHz).
    pub fn push(&mut self, mono: &[f32]) {
        for s in mono {
            if self.full() {
                return;
            }
            self.peak = self.peak.max(s.abs());
            self.peak_n += 1;
            if self.peak_n == PEAK_EVERY {
                self.peaks.push(self.peak);
                (self.peak, self.peak_n) = (0.0, 0);
            }
            self.frame.push(*s);
            if self.frame.len() == FRAME {
                self.encode();
            }
        }
    }

    fn encode(&mut self) {
        if let Ok(n) = self.enc.encode_float(&self.frame, &mut self.out) {
            self.packets.push(self.out[..n].to_vec());
        }
        self.frame.clear();
    }

    /// Fichier Ogg/Opus, durée et onde ; `None` si rien n'a été enregistré.
    pub fn finish(mut self, serial: u32) -> Option<Recorded> {
        if !self.frame.is_empty() && !self.full() {
            self.frame.resize(FRAME, 0.0);
            self.encode();
        }
        if self.peak_n > 0 {
            self.peaks.push(self.peak);
        }
        if self.packets.is_empty() {
            return None;
        }
        let duration_ms = self.elapsed_ms();
        let ogg = ogg::write_opus(&self.packets, 1, PRE_SKIP, FRAME as u64, serial);
        Some(Recorded { ogg, duration_ms, waveform: waveform_from_peaks(&self.peaks) })
    }
}
