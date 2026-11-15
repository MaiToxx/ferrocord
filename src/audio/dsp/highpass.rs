//! Filtre passe-haut (Butterworth d'ordre 2) : retire le continu, les
//! souffles et les vibrations du bureau sous la voix.

/// Fréquence de coupure : sous la fondamentale des voix graves (~85 Hz).
pub const CUTOFF_HZ: f64 = 80.0;

#[derive(Clone, Debug)]
pub struct HighPass {
    b: [f64; 3],
    a: [f64; 2],
    // Forme directe II transposée ; en f64, car une coupure aussi basse
    // devant 48 kHz rend le filtre sensible aux arrondis.
    z: [f64; 2],
}

impl HighPass {
    pub fn new(sample_rate: f64, cutoff: f64) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * cutoff / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / std::f64::consts::SQRT_2;
        let a0 = 1.0 + alpha;
        let b0 = (1.0 + cos) / 2.0 / a0;
        HighPass { b: [b0, -2.0 * b0, b0], a: [-2.0 * cos / a0, (1.0 - alpha) / a0], z: [0.0; 2] }
    }

    pub fn voice() -> Self {
        Self::new(48_000.0, CUTOFF_HZ)
    }

    pub fn reset(&mut self) {
        self.z = [0.0; 2];
    }

    pub fn process(&mut self, x: &mut [f32]) {
        let [b0, b1, b2] = self.b;
        let [a1, a2] = self.a;
        let [mut z0, mut z1] = self.z;
        for s in x.iter_mut() {
            let input = f64::from(*s);
            let y = b0 * input + z0;
            z0 = b1 * input - a1 * y + z1;
            z1 = b2 * input - a2 * y;
            *s = y as f32;
        }
        self.z = [z0, z1];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::level::rms_db;

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n).map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / 48_000.0).sin()).collect()
    }

    /// Gain en régime établi (la première seconde est ignorée).
    fn gain_db(freq: f32) -> f32 {
        let mut x = sine(freq, 0.5, 96_000);
        let before = rms_db(&x[48_000..]);
        HighPass::voice().process(&mut x);
        rms_db(&x[48_000..]) - before
    }

    #[test]
    fn cuts_rumble_and_keeps_voice() {
        assert!(gain_db(30.0) < -12.0, "30 Hz : {} dB", gain_db(30.0));
        assert!(gain_db(1000.0).abs() < 0.5, "1 kHz : {} dB", gain_db(1000.0));
        assert!(gain_db(300.0).abs() < 0.5, "300 Hz : {} dB", gain_db(300.0));
        let mut dc = vec![0.3f32; 48_000];
        HighPass::voice().process(&mut dc);
        assert!(dc[47_000..].iter().all(|s| s.abs() < 1e-3), "le continu disparaît");
    }
}
