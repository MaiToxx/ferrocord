//! Rééchantillonnage en continu entre la fréquence du périphérique et
//! 48 kHz : interpolation cubique (Catmull-Rom), deux échantillons de
//! retard, aucune allocation. Suffisant pour la voix ; un périphérique déjà
//! à 48 kHz (le cas courant sous Windows) n'est pas touché.

/// Canaux au plus : mono (capture) ou stéréo (lecture).
pub const MAX_CHANNELS: usize = 2;

#[derive(Clone, Debug)]
pub struct Resampler {
    channels: usize,
    /// Échantillons d'entrée par échantillon de sortie.
    step: f64,
    /// Position de la prochaine sortie entre `hist[1]` et `hist[2]`.
    frac: f64,
    /// Quatre dernières trames d'entrée, la plus récente à la fin.
    hist: [[f32; MAX_CHANNELS]; 4],
}

impl Resampler {
    /// `channels` est ramené entre 1 et [`MAX_CHANNELS`].
    pub fn new(from_hz: u32, to_hz: u32, channels: usize) -> Self {
        let step = if from_hz == 0 || to_hz == 0 { 1.0 } else { f64::from(from_hz) / f64::from(to_hz) };
        Resampler { channels: channels.clamp(1, MAX_CHANNELS), step, frac: 1.0, hist: [[0.0; MAX_CHANNELS]; 4] }
    }

    pub fn channels(&self) -> usize {
        self.channels
    }

    pub fn is_passthrough(&self) -> bool {
        self.step == 1.0
    }

    /// Trames de sortie au plus pour `frames` trames d'entrée (taille des tampons).
    pub fn max_output(&self, frames: usize) -> usize {
        (frames as f64 / self.step).ceil() as usize + 2
    }

    /// Lit `input` (entrelacé) et écrit dans `out` (entrelacé) jusqu'à
    /// épuisement de l'un ou remplissage de l'autre. Renvoie (trames lues,
    /// trames écrites) ; ce qui n'est pas lu le sera à l'appel suivant.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) -> (usize, usize) {
        let ch = self.channels;
        let (in_frames, out_frames) = (input.len() / ch, out.len() / ch);
        if self.is_passthrough() {
            let n = in_frames.min(out_frames);
            out[..n * ch].copy_from_slice(&input[..n * ch]);
            return (n, n);
        }
        let (mut read, mut written) = (0, 0);
        loop {
            while self.frac < 1.0 {
                if written == out_frames {
                    return (read, written);
                }
                let t = self.frac as f32;
                for c in 0..ch {
                    out[written * ch + c] = cubic(self.hist[0][c], self.hist[1][c], self.hist[2][c], self.hist[3][c], t);
                }
                written += 1;
                self.frac += self.step;
            }
            if read == in_frames {
                return (read, written);
            }
            self.frac -= 1.0;
            self.hist.rotate_left(1);
            for c in 0..ch {
                self.hist[3][c] = input[read * ch + c];
            }
            read += 1;
        }
    }
}

fn cubic(p0: f32, p1: f32, p2: f32, p3: f32, t: f32) -> f32 {
    let a = -0.5 * p0 + 1.5 * p1 - 1.5 * p2 + 0.5 * p3;
    let b = p0 - 2.5 * p1 + 2.0 * p2 - 0.5 * p3;
    let c = -0.5 * p0 + 0.5 * p2;
    ((a * t + b) * t + c) * t + p1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine(freq: f64, rate: f64, n: usize, phase_samples: f64) -> Vec<f32> {
        (0..n).map(|i| (2.0 * std::f64::consts::PI * freq * (i as f64 - phase_samples) / rate).sin() as f32).collect()
    }

    #[test]
    fn same_rate_is_untouched() {
        let mut r = Resampler::new(48_000, 48_000, 2);
        let x: Vec<f32> = (0..20).map(|i| i as f32).collect();
        let mut out = [0.0; 20];
        assert_eq!(r.process(&x, &mut out), (10, 10));
        assert_eq!(&out[..], &x[..]);
    }

    #[test]
    fn upsampling_44k1_gives_the_same_tone_at_48k() {
        let x = sine(1000.0, 44_100.0, 44_100, 0.0);
        let mut r = Resampler::new(44_100, 48_000, 1);
        let mut out = vec![0.0; r.max_output(x.len())];
        let (read, written) = r.process(&x, &mut out);
        assert_eq!(read, 44_100);
        assert!(written.abs_diff(48_000) <= 2, "{written}");
        // Deux échantillons d'entrée de retard, exprimés à 48 kHz.
        let want = sine(1000.0, 48_000.0, written, 2.0 * 48_000.0 / 44_100.0);
        let err = out[100..written].iter().zip(&want[100..]).map(|(a, b)| (a - b).abs()).fold(0.0, f32::max);
        assert!(err < 0.01, "écart {err}");
    }

    #[test]
    fn downsampling_in_small_pieces_matches_one_go() {
        let x = sine(440.0, 96_000.0, 9_600, 0.0);
        let mut whole = Resampler::new(96_000, 48_000, 1);
        let mut a = vec![0.0; whole.max_output(x.len())];
        let (_, na) = whole.process(&x, &mut a);
        // Petits tampons de sortie : la lecture reprend où elle s'était arrêtée.
        let mut pieces = Resampler::new(96_000, 48_000, 1);
        let (mut b, mut pos) = (Vec::new(), 0);
        let mut chunk = [0.0f32; 7];
        while pos < x.len() {
            let (r, w) = pieces.process(&x[pos..(pos + 33).min(x.len())], &mut chunk);
            b.extend_from_slice(&chunk[..w]);
            pos += r;
        }
        assert!(na.abs_diff(4_800) <= 2);
        assert_eq!(&a[..na], &b[..]);
    }

    #[test]
    fn stereo_channels_stay_apart() {
        let mut r = Resampler::new(44_100, 48_000, 2);
        let x: Vec<f32> = (0..2_000).flat_map(|_| [0.5, -0.25]).collect();
        let mut out = vec![0.0; 2 * r.max_output(1_000)];
        let (_, w) = r.process(&x, &mut out);
        for f in out[20..2 * w].chunks(2) {
            assert!((f[0] - 0.5).abs() < 1e-5 && (f[1] + 0.25).abs() < 1e-5, "{f:?}");
        }
    }
}
