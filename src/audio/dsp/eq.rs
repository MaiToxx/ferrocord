//! Égaliseur à trois bandes des voix entendues (plugin « Égaliseur des
//! voix », lot E4) : plateau grave à 200 Hz, cloche médium à 1,2 kHz,
//! plateau aigu à 5 kHz, de -12 à +12 dB chacun (filtres biquadratiques du
//! « Audio EQ Cookbook » de R. Bristow-Johnson).
//!
//! Il s'applique au mixage des voix, avant le limiteur du moteur : une
//! bande poussée ne sature donc jamais la sortie. À plat (plugin coupé ou
//! préréglage « Plat »), [`Eq3::process`] rend la main sans rien toucher ;
//! rien n'est alloué, à aucun moment.

use std::f32::consts::PI;

const RATE: f32 = 48_000.0;
/// Bornes d'une bande, en dB.
pub const GAIN_MAX: i8 = 12;
const LOW_HZ: f32 = 200.0;
const MID_HZ: f32 = 1_200.0;
const HIGH_HZ: f32 = 5_000.0;
const Q: f32 = 0.707;

/// Clés des préréglages (réglage `prereglage` du plugin).
pub const PRESET_FLAT: &str = "plat";
pub const PRESET_CLEAR: &str = "voix_claires";
pub const PRESET_BASS: &str = "graves";
pub const PRESET_SOFT: &str = "aigus_doux";
pub const PRESET_RADIO: &str = "radio";
pub const PRESET_NIGHT: &str = "nuit";
pub const PRESET_CUSTOM: &str = "perso";

/// Gains (grave, médium, aigu) d'un préréglage ; `custom` pour « Personnalisé ».
pub fn preset_gains(preset: &str, custom: [i8; 3]) -> [i8; 3] {
    let g = match preset {
        PRESET_CLEAR => [-2, 2, 4],
        PRESET_BASS => [6, 0, 0],
        PRESET_SOFT => [0, 0, -6],
        PRESET_RADIO => [-9, 4, -5],
        PRESET_NIGHT => [-8, 0, -1],
        PRESET_CUSTOM => custom,
        _ => [0, 0, 0],
    };
    g.map(|x| x.clamp(-GAIN_MAX, GAIN_MAX))
}

/// Trois gains rangés dans un mot (réglage porté au fil de lecture par un atomique).
pub fn pack(g: [i8; 3]) -> u32 {
    u32::from(g[0] as u8) | u32::from(g[1] as u8) << 8 | u32::from(g[2] as u8) << 16
}

pub fn unpack(b: u32) -> [i8; 3] {
    [b as u8 as i8, (b >> 8) as u8 as i8, (b >> 16) as u8 as i8].map(|x| x.clamp(-GAIN_MAX, GAIN_MAX))
}

/// Coefficients normalisés (a0 = 1).
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
}

impl Biquad {
    const UNIT: Biquad = Biquad { b: [1.0, 0.0, 0.0], a: [0.0, 0.0] };

    fn norm(b: [f32; 3], a: [f32; 3]) -> Biquad {
        Biquad { b: [b[0] / a[0], b[1] / a[0], b[2] / a[0]], a: [a[1] / a[0], a[2] / a[0]] }
    }

    fn peaking(hz: f32, db: f32) -> Biquad {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * hz / RATE;
        let alpha = w.sin() / (2.0 * Q);
        let c = w.cos();
        Biquad::norm([1.0 + alpha * a, -2.0 * c, 1.0 - alpha * a], [1.0 + alpha / a, -2.0 * c, 1.0 - alpha / a])
    }

    /// Plateau ; `high` : côté aigu.
    fn shelf(hz: f32, db: f32, high: bool) -> Biquad {
        let a = 10f32.powf(db / 40.0);
        let w = 2.0 * PI * hz / RATE;
        let c = w.cos();
        let k = 2.0 * a.sqrt() * (w.sin() / (2.0 * Q));
        // Le plateau aigu est le grave avec cos → -cos et les termes impairs changés de signe.
        let s = if high { -1.0 } else { 1.0 };
        let c = c * s;
        let b = [a * ((a + 1.0) - (a - 1.0) * c + k), s * 2.0 * a * ((a - 1.0) - (a + 1.0) * c), a * ((a + 1.0) - (a - 1.0) * c - k)];
        let d = [(a + 1.0) + (a - 1.0) * c + k, s * -2.0 * ((a - 1.0) + (a + 1.0) * c), (a + 1.0) + (a - 1.0) * c - k];
        Biquad::norm(b, d)
    }
}

/// L'égaliseur : trois filtres en série, sur du stéréo entrelacé.
pub struct Eq3 {
    gains: [i8; 3],
    bands: [Biquad; 3],
    /// Mémoire des filtres : par bande, par voie.
    z: [[[f32; 2]; 2]; 3],
}

impl Default for Eq3 {
    fn default() -> Self {
        Self::new()
    }
}

impl Eq3 {
    pub fn new() -> Self {
        Eq3 { gains: [0; 3], bands: [Biquad::UNIT; 3], z: [[[0.0; 2]; 2]; 3] }
    }

    pub fn gains(&self) -> [i8; 3] {
        self.gains
    }

    /// Aucune bande n'est réglée : rien n'est traité.
    pub fn is_flat(&self) -> bool {
        self.gains == [0; 3]
    }

    /// Règle les trois bandes (dB) ; une bande qui change repart sans mémoire.
    pub fn set(&mut self, gains: [i8; 3]) {
        let gains = gains.map(|g| g.clamp(-GAIN_MAX, GAIN_MAX));
        for (i, &gain) in gains.iter().enumerate() {
            if gain == self.gains[i] {
                continue;
            }
            let db = f32::from(gain);
            self.bands[i] = match (i, gain) {
                (_, 0) => Biquad::UNIT,
                (0, _) => Biquad::shelf(LOW_HZ, db, false),
                (1, _) => Biquad::peaking(MID_HZ, db),
                _ => Biquad::shelf(HIGH_HZ, db, true),
            };
            self.z[i] = [[0.0; 2]; 2];
        }
        self.gains = gains;
    }

    /// Égalise `stereo` (entrelacé) sur place.
    pub fn process(&mut self, stereo: &mut [f32]) {
        if self.is_flat() {
            return;
        }
        for (i, f) in self.bands.iter().enumerate() {
            if self.gains[i] == 0 {
                continue;
            }
            let z = &mut self.z[i];
            for frame in stereo.as_chunks_mut::<2>().0 {
                for (x, z) in frame.iter_mut().zip(z.iter_mut()) {
                    // Forme directe II transposée.
                    let y = f.b[0] * *x + z[0];
                    z[0] = f.b[1] * *x - f.a[0] * y + z[1];
                    z[1] = f.b[2] * *x - f.a[1] * y;
                    *x = y;
                }
            }
        }
        // Mémoire dénormalisée après un long silence : remise à zéro.
        for band in self.z.iter_mut() {
            for ch in band.iter_mut() {
                for v in ch.iter_mut() {
                    if v.abs() < 1e-20 {
                        *v = 0.0;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::level::rms_db;

    fn tone(hz: f32, n: usize) -> Vec<f32> {
        (0..n).flat_map(|i| [0.1 * (2.0 * PI * hz * i as f32 / RATE).sin(); 2]).collect()
    }

    /// Gain mesuré (dB) d'un sinus à `hz` à travers l'égaliseur.
    fn gain_at(eq: &mut Eq3, hz: f32) -> f32 {
        let x = tone(hz, 48_000);
        let mut y = x.clone();
        for f in y.chunks_mut(1920) {
            eq.process(f);
        }
        rms_db(&y[9_600..]) - rms_db(&x[9_600..])
    }

    #[test]
    fn flat_is_untouched() {
        let mut eq = Eq3::new();
        assert!(eq.is_flat());
        let x = tone(440.0, 4_800);
        let mut y = x.clone();
        eq.process(&mut y);
        assert_eq!(x, y);
        eq.set(preset_gains(PRESET_FLAT, [9, 9, 9]));
        assert!(eq.is_flat());
        eq.set(preset_gains("inconnu", [9, 9, 9]));
        assert!(eq.is_flat());
    }

    #[test]
    fn each_band_moves_its_own_range() {
        let mut eq = Eq3::new();
        eq.set([6, 0, 0]);
        let (low, mid, high) = (gain_at(&mut eq, 60.0), gain_at(&mut eq, 1_200.0), gain_at(&mut eq, 8_000.0));
        assert!((low - 6.0).abs() < 0.7 && mid.abs() < 0.7 && high.abs() < 0.3, "graves +6 : {low:.2} {mid:.2} {high:.2}");

        let mut eq = Eq3::new();
        eq.set([0, -9, 0]);
        let (low, mid, high) = (gain_at(&mut eq, 60.0), gain_at(&mut eq, 1_200.0), gain_at(&mut eq, 12_000.0));
        assert!(low.abs() < 0.5 && (mid + 9.0).abs() < 0.5 && high.abs() < 0.7, "médiums -9 : {low:.2} {mid:.2} {high:.2}");

        let mut eq = Eq3::new();
        eq.set([0, 0, 12]);
        let (low, mid, high) = (gain_at(&mut eq, 100.0), gain_at(&mut eq, 600.0), gain_at(&mut eq, 16_000.0));
        assert!(low.abs() < 0.3 && mid.abs() < 1.0 && (high - 12.0).abs() < 0.7, "aigus +12 : {low:.2} {mid:.2} {high:.2}");
    }

    #[test]
    fn presets_and_packing() {
        assert_eq!(preset_gains(PRESET_CUSTOM, [-12, 3, 12]), [-12, 3, 12]);
        assert_eq!(preset_gains(PRESET_CUSTOM, [-99, 0, 99]), [-12, 0, 12]);
        for p in [PRESET_FLAT, PRESET_CLEAR, PRESET_BASS, PRESET_SOFT, PRESET_RADIO, PRESET_NIGHT] {
            let g = preset_gains(p, [1, 1, 1]);
            assert_eq!(unpack(pack(g)), g, "{p}");
            assert_eq!(g == [0; 3], p == PRESET_FLAT, "{p}");
        }
        assert_eq!(unpack(pack([-12, 0, 12])), [-12, 0, 12]);
        assert_eq!(pack([0; 3]), 0);
    }

    #[test]
    fn stays_finite_and_stable() {
        let mut eq = Eq3::new();
        eq.set([12, 12, 12]);
        let mut x = tone(300.0, 48_000);
        eq.process(&mut x);
        assert!(x.iter().all(|s| s.is_finite() && s.abs() < 1.0));
        // Retour à plat : plus aucun traitement.
        eq.set([0; 3]);
        let quiet = tone(300.0, 960);
        let mut y = quiet.clone();
        eq.process(&mut y);
        assert_eq!(quiet, y);
    }
}
