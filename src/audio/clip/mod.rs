//! Sons courts joués dans la sortie audio : sons du tableau (MP3, parfois
//! Ogg/Opus) et messages vocaux (Ogg/Opus). Tout est ramené à 48 kHz, en
//! mono ou stéréo, rangé en `i16` (moitié moins de mémoire que des `f32`).
//!
//! Le décodage se fait hors des fils audio et du fil d'interface ; les sons
//! décodés sont gardés dans un [`Cache`] borné en octets.

pub mod mp3;
pub mod ogg;
pub mod record;

use crate::audio::engine::{MAX_DECODED, SAMPLE_RATE};
use crate::audio::io::resample::Resampler;
use std::collections::VecDeque;
use std::sync::Arc;

/// Durée décodée au plus (10 min) : au-delà, le reste est ignoré.
pub const MAX_FRAMES: usize = SAMPLE_RATE as usize * 600;
/// Points de l'onde au plus (docs.discord.food, « Voice Messages » : 256).
pub const WAVEFORM_POINTS: usize = 256;
/// Un point de l'onde toutes les 100 ms (même page), avant réduction.
pub const WAVEFORM_EVERY_MS: u32 = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipError {
    /// Fichier illisible ou vide.
    Format,
    /// Format reconnu mais non pris en charge (voies, codec).
    Unsupported,
}

/// Son décodé, à 48 kHz, voies entrelacées.
#[derive(Clone, Debug, PartialEq)]
pub struct Pcm {
    /// 1 ou 2.
    pub channels: usize,
    pub data: Arc<[i16]>,
}

impl Pcm {
    pub fn frames(&self) -> usize {
        self.data.len() / self.channels.max(1)
    }

    pub fn duration_ms(&self) -> u32 {
        (self.frames() as u64 * 1000 / u64::from(SAMPLE_RATE)) as u32
    }

    fn bytes(&self) -> usize {
        self.data.len() * 2
    }

    /// Trame `i` en stéréo (`[0.0; 2]` au-delà de la fin).
    pub fn stereo(&self, i: usize) -> [f32; 2] {
        const K: f32 = 1.0 / 32768.0;
        match self.channels {
            1 => {
                let s = self.data.get(i).map_or(0.0, |s| f32::from(*s) * K);
                [s, s]
            }
            _ => [self.data.get(2 * i).map_or(0.0, |s| f32::from(*s) * K), self.data.get(2 * i + 1).map_or(0.0, |s| f32::from(*s) * K)],
        }
    }
}

fn to_i16(x: f32) -> i16 {
    (x.clamp(-1.0, 1.0) * 32767.0).round() as i16
}

/// Des `f32` entrelacés à `rate` vers 48 kHz (`channels` : 1 ou 2).
fn to_48k(samples: &[f32], rate: u32, channels: usize) -> Vec<i16> {
    let mut rs = Resampler::new(rate, SAMPLE_RATE, channels);
    if rs.is_passthrough() {
        return samples.iter().map(|s| to_i16(*s)).collect();
    }
    let ch = rs.channels();
    let mut out = Vec::with_capacity(rs.max_output(samples.len() / ch) * ch);
    let mut buf = vec![0.0f32; rs.max_output(4096) * ch];
    for chunk in samples.chunks(4096 * ch) {
        let mut at = 0;
        while at < chunk.len() {
            let (read, written) = rs.process(&chunk[at..], &mut buf);
            out.extend(buf[..written * ch].iter().map(|s| to_i16(*s)));
            if read == 0 && written == 0 {
                break;
            }
            at += read * ch;
        }
    }
    out
}

/// Ogg/Opus vers PCM : décodeur stéréo de libopus, `pre_skip` retiré.
fn decode_opus(data: &[u8]) -> Result<Pcm, ClipError> {
    let s = ogg::read_opus(data)?;
    let mut dec = crate::audio::engine::VoiceDecoder::new().ok_or(ClipError::Unsupported)?;
    let channels = usize::from(s.channels);
    let mut out: Vec<i16> = Vec::new();
    let mut buf = vec![0.0f32; MAX_DECODED * 2];
    let mut skip = usize::from(s.pre_skip);
    for p in &s.packets {
        let n = dec.decode(p, &mut buf);
        let drop = skip.min(n);
        skip -= drop;
        for f in buf[2 * drop..2 * n].as_chunks::<2>().0 {
            out.push(to_i16(f[0]));
            if channels == 2 {
                out.push(to_i16(f[1]));
            }
        }
        if out.len() / channels >= MAX_FRAMES {
            break;
        }
    }
    if out.is_empty() {
        return Err(ClipError::Format);
    }
    Ok(Pcm { channels, data: out.into() })
}

/// Décode un son (Ogg/Opus d'après sa signature, sinon MP3). Long : à faire
/// hors du fil d'interface et des fils audio.
pub fn decode(data: Vec<u8>) -> Result<Pcm, ClipError> {
    if data.starts_with(b"OggS") {
        return decode_opus(&data);
    }
    let d = mp3::decode(data, MAX_FRAMES)?;
    let data = to_48k(&d.samples, d.rate, d.channels);
    if data.is_empty() {
        return Err(ClipError::Format);
    }
    Ok(Pcm { channels: d.channels, data: data.into() })
}

/// Onde à partir de crêtes (0 à 1) relevées à intervalles réguliers :
/// réduite à [`WAVEFORM_POINTS`] points au plus (crête de chaque groupe).
pub fn waveform_from_peaks(peaks: &[f32]) -> Vec<u8> {
    if peaks.is_empty() {
        return Vec::new();
    }
    let group = peaks.len().div_ceil(WAVEFORM_POINTS);
    peaks.chunks(group).map(|g| (g.iter().fold(0.0f32, |m, p| m.max(*p)).clamp(0.0, 1.0) * 255.0).round() as u8).collect()
}

/// Onde d'un son décodé : une crête toutes les 100 ms, puis réduction.
pub fn waveform(pcm: &Pcm) -> Vec<u8> {
    let per = (SAMPLE_RATE * WAVEFORM_EVERY_MS / 1000) as usize;
    let peaks: Vec<f32> = (0..pcm.frames().div_ceil(per))
        .map(|w| {
            (w * per..((w + 1) * per).min(pcm.frames())).fold(0.0f32, |m, i| {
                let [l, r] = pcm.stereo(i);
                m.max(l.abs()).max(r.abs())
            })
        })
        .collect();
    waveform_from_peaks(&peaks)
}

/// Sons décodés gardés, du moins récemment servi au plus récent. Borné en
/// octets : à l'ajout, les plus anciens partent jusqu'à ce que le nouveau
/// tienne ; un son plus gros que la borne n'est pas gardé.
pub struct Cache {
    items: VecDeque<(String, Pcm)>,
    bytes: usize,
    max_bytes: usize,
}

impl Cache {
    pub fn new(max_bytes: usize) -> Self {
        Cache { items: VecDeque::new(), bytes: 0, max_bytes }
    }

    pub fn get(&mut self, key: &str) -> Option<Pcm> {
        let i = self.items.iter().position(|(k, _)| k == key)?;
        let item = self.items.remove(i)?;
        let pcm = item.1.clone();
        self.items.push_back(item);
        Some(pcm)
    }

    pub fn insert(&mut self, key: String, pcm: Pcm) {
        if let Some(i) = self.items.iter().position(|(k, _)| *k == key)
            && let Some((_, old)) = self.items.remove(i)
        {
            self.bytes -= old.bytes();
        }
        if pcm.bytes() > self.max_bytes {
            return;
        }
        while self.bytes + pcm.bytes() > self.max_bytes {
            let Some((_, old)) = self.items.pop_front() else { break };
            self.bytes -= old.bytes();
        }
        self.bytes += pcm.bytes();
        self.items.push_back((key, pcm));
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
}

#[cfg(test)]
mod tests;
