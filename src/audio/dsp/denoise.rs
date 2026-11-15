//! Suppression de bruit spectrale légère, en Rust pur (remplace RNNoise /
//! `nnnoiseless`, 1,11 Mio, à la demande de l'utilisateur).
//!
//! Transformée de Fourier à court terme : fenêtres de 512 échantillons
//! (10,7 ms) avancées de 256, fenêtre de Hann racine à l'analyse et à la
//! synthèse (la somme des recouvrements vaut 1). Par raie :
//! - bruit de fond estimé par une moyenne glissante des raies qui en ont
//!   l'air (moins de 4 fois le bruit), qui ne remonte que lentement sinon (≈ 3 dB/s) ;
//! - gain de Wiener sur un rapport signal/bruit « dirigé par la décision »
//!   (Ephraim-Malah), lissé d'une fenêtre à l'autre, avec un plancher de
//!   -20 dB pour ne pas donner de « bruit musical ».
//!
//! Tout est préalloué dans [`Denoiser::new`] : rien n'alloue en temps réel.
//! Latence ajoutée : 512 échantillons (10,7 ms). La chaîne ne voit qu'un
//! [`Processor`] ([`new_processor`]) : changer de débruiteur ne touche que
//! ce fichier.

use crate::audio::Processor;
use std::f32::consts::PI;

const N: usize = 512;
const HOP: usize = N / 2;
const BINS: usize = N / 2 + 1;
/// Plancher du gain (-20 dB).
const MIN_GAIN: f32 = 0.1;
/// Poids du passé dans le rapport signal/bruit a priori.
const DD_ALPHA: f32 = 0.96;
/// Remontée du bruit de fond par fenêtre (187,5 fenêtres/s : ≈ 3 dB/s).
const NOISE_RISE: f32 = 1.0035;
/// Une raie sous ce multiple du bruit estimé est comptée comme du bruit.
const NOISE_LIKE: f32 = 4.0;
/// Poids du passé dans la moyenne du bruit (≈ 0,1 s).
const NOISE_AVG: f32 = 0.95;

/// Transformée de Fourier rapide complexe de taille [`N`] (radix 2, en place).
struct Fft {
    cos: Vec<f32>,
    sin: Vec<f32>,
    rev: Vec<usize>,
}

impl Fft {
    fn new() -> Self {
        let bits = N.trailing_zeros();
        let rev = (0..N).map(|i| i.reverse_bits() >> (usize::BITS - bits)).collect();
        let cos = (0..N / 2).map(|k| (2.0 * PI * k as f32 / N as f32).cos()).collect();
        let sin = (0..N / 2).map(|k| (2.0 * PI * k as f32 / N as f32).sin()).collect();
        Fft { cos, sin, rev }
    }

    /// `inverse` : sans la division par `N` (faite par l'appelant).
    fn run(&self, re: &mut [f32], im: &mut [f32], inverse: bool) {
        for (i, &j) in self.rev.iter().enumerate() {
            if j > i {
                re.swap(i, j);
                im.swap(i, j);
            }
        }
        let sign = if inverse { 1.0 } else { -1.0 };
        let mut len = 2;
        while len <= N {
            let step = N / len;
            for start in (0..N).step_by(len) {
                for k in 0..len / 2 {
                    let (wr, wi) = (self.cos[k * step], sign * self.sin[k * step]);
                    let (a, b) = (start + k, start + k + len / 2);
                    let tr = re[b] * wr - im[b] * wi;
                    let ti = re[b] * wi + im[b] * wr;
                    re[b] = re[a] - tr;
                    im[b] = im[a] - ti;
                    re[a] += tr;
                    im[a] += ti;
                }
            }
            len *= 2;
        }
    }
}

pub struct Denoiser {
    fft: Fft,
    window: Vec<f32>,
    /// Dernières `N` entrées ; `filled` : combien sont arrivées.
    input: Vec<f32>,
    filled: usize,
    /// Recouvrement-addition des fenêtres traitées.
    ola: Vec<f32>,
    /// Sortie prête (une avancée) et position de lecture.
    ready: Vec<f32>,
    pos: usize,
    re: Vec<f32>,
    im: Vec<f32>,
    noise: Vec<f32>,
    /// Puissance « propre » estimée à la fenêtre précédente (gain² × puissance).
    clean_prev: Vec<f32>,
    started: bool,
}

impl Default for Denoiser {
    fn default() -> Self {
        Self::new()
    }
}

impl Denoiser {
    pub fn new() -> Self {
        let window = (0..N).map(|n| (0.5 - 0.5 * (2.0 * PI * n as f32 / N as f32).cos()).sqrt()).collect();
        Denoiser {
            fft: Fft::new(),
            window,
            input: vec![0.0; N],
            filled: N - HOP,
            ola: vec![0.0; N],
            ready: vec![0.0; HOP],
            pos: 0,
            re: vec![0.0; N],
            im: vec![0.0; N],
            noise: vec![0.0; BINS],
            clean_prev: vec![0.0; BINS],
            started: false,
        }
    }

    /// Débruite `x` sur place (longueur quelconque).
    pub fn run(&mut self, x: &mut [f32]) {
        for s in x.iter_mut() {
            self.input[self.filled] = *s;
            self.filled += 1;
            *s = self.ready[self.pos];
            self.pos += 1;
            if self.filled == N {
                self.frame();
                self.input.copy_within(HOP.., 0);
                self.filled = N - HOP;
                self.pos = 0;
            }
        }
    }

    fn frame(&mut self) {
        for i in 0..N {
            self.re[i] = self.input[i] * self.window[i];
            self.im[i] = 0.0;
        }
        self.fft.run(&mut self.re, &mut self.im, false);
        for k in 0..BINS {
            let p = self.re[k] * self.re[k] + self.im[k] * self.im[k];
            let n = &mut self.noise[k];
            if !self.started {
                *n = p;
            } else if p < NOISE_LIKE * *n {
                // Probablement du bruit : moyenne glissante (et non un minimum,
                // qui sous-estimerait un bruit qui fluctue).
                *n = NOISE_AVG * *n + (1.0 - NOISE_AVG) * p;
            } else {
                // Probablement de la voix : le bruit ne remonte que lentement.
                *n *= NOISE_RISE;
            }
            *n = n.max(1e-10);
            let noise = *n;
            let post = p / noise;
            let prior = DD_ALPHA * self.clean_prev[k] / noise + (1.0 - DD_ALPHA) * (post - 1.0).max(0.0);
            let g = (prior / (1.0 + prior)).max(MIN_GAIN);
            self.clean_prev[k] = g * g * p;
            self.re[k] *= g;
            self.im[k] *= g;
            // Spectre d'un signal réel : symétrie hermitienne.
            if k > 0 && k < N / 2 {
                self.re[N - k] *= g;
                self.im[N - k] *= g;
            }
        }
        self.started = true;
        self.fft.run(&mut self.re, &mut self.im, true);
        let scale = 1.0 / N as f32;
        for i in 0..N {
            self.ola[i] += self.re[i] * scale * self.window[i];
        }
        for (r, o) in self.ready.iter_mut().zip(&self.ola[..HOP]) {
            *r = o.clamp(-1.0, 1.0);
        }
        self.ola.copy_within(HOP.., 0);
        self.ola[N - HOP..].fill(0.0);
    }
}

impl Processor for Denoiser {
    fn process(&mut self, mono: &mut [f32], _far_end: &[f32]) {
        self.run(mono);
    }
}

/// Le débruiteur de la chaîne de traitement.
pub fn new_processor() -> Box<dyn Processor> {
    Box::new(Denoiser::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::level::rms_db;

    /// Bruit blanc reproductible (générateur congruentiel).
    fn noise(seed: u32, amp: f32, n: usize) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amp * ((s >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    fn tone(start: usize, n: usize, amp: f32) -> Vec<f32> {
        (start..start + n).map(|i| amp * (2.0 * PI * 440.0 * i as f32 / 48_000.0).sin()).collect()
    }

    #[test]
    fn transparent_reconstruction_without_noise_reduction() {
        // Après un silence, un signal fort passe presque intact (gain ≈ 1), retardé de N.
        let mut x = vec![0.0f32; 9_600];
        x.extend(tone(0, 48_000, 0.5));
        let mut y = x.clone();
        let mut d = Denoiser::new();
        for f in y.chunks_mut(960) {
            d.run(f);
        }
        let err: f32 = x[10_000..50_000].iter().zip(&y[10_000 + N..50_000 + N]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.05, "écart {err}");
    }

    #[test]
    fn steady_noise_is_reduced_and_voice_kept() {
        let mut x = noise(7, 0.05, 48_000 * 3);
        let before = rms_db(&x[48_000..]);
        let mut d = Denoiser::new();
        for frame in x.chunks_mut(960) {
            d.run(frame);
        }
        let after = rms_db(&x[48_000..]);
        assert!(before - after > 10.0, "bruit : avant {before:.1} dB, après {after:.1} dB");

        // Une voix (sinus) arrive dans le même bruit : elle garde son niveau.
        let n = noise(9, 0.05, 48_000);
        let v = tone(0, 48_000, 0.3);
        let mut mix: Vec<f32> = n.iter().zip(&v).map(|(a, b)| a + b).collect();
        for frame in mix.chunks_mut(960) {
            d.run(frame);
        }
        let (kept, orig) = (rms_db(&mix[9_600..]), rms_db(&v[9_600..]));
        assert!((kept - orig).abs() < 1.5, "voix : {orig:.1} dB → {kept:.1} dB");
    }

    #[test]
    fn odd_chunk_sizes_and_bounded_output() {
        let mut x = noise(3, 0.9, 10_000);
        let mut d = Denoiser::new();
        let mut i = 0;
        for n in [1usize, 7, 441, 960, 3, 2000].iter().cycle() {
            if i >= x.len() {
                break;
            }
            let end = (i + n).min(x.len());
            d.run(&mut x[i..end]);
            i = end;
        }
        assert!(x.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
    }
}
