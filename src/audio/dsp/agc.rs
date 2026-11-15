//! Contrôle automatique du gain : ramène la voix vers un niveau cible,
//! sans gonfler le silence, et sans jamais écrêter (limiteur doux).

use super::level::rms_db;

/// Niveau visé pour la voix (efficace, dB pleine échelle).
pub const TARGET_DB: f32 = -18.0;
const MAX_GAIN_DB: f32 = 20.0;
const MIN_GAIN_DB: f32 = -20.0;
/// Sous ce niveau, la trame est du silence ou du bruit : le gain ne monte pas.
const ACTIVITY_DB: f32 = -50.0;
/// Baisse rapide (une voix forte est corrigée en quelques trames)…
const ATTACK: f32 = 0.3;
/// …remontée lente : 0,1 dB par trame de 20 ms, soit 5 dB/s.
const RELEASE_DB: f32 = 0.1;
/// Au-delà, le limiteur arrondit le signal au lieu de l'écrêter.
const KNEE: f32 = 0.9;
/// En f32, tanh atteint 1 : plafond explicite, sous la pleine échelle.
const CEILING: f32 = 0.999;

#[derive(Clone, Debug, Default)]
pub struct Agc {
    gain_db: f32,
}

fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

impl Agc {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.gain_db = 0.0;
    }

    pub fn gain_db(&self) -> f32 {
        self.gain_db
    }

    pub fn process(&mut self, x: &mut [f32]) {
        if x.is_empty() {
            return;
        }
        let level = rms_db(x);
        let before = self.gain_db;
        if level > ACTIVITY_DB {
            let wanted = (TARGET_DB - level).clamp(MIN_GAIN_DB, MAX_GAIN_DB);
            self.gain_db = if wanted < before { before + (wanted - before) * ATTACK } else { (before + RELEASE_DB).min(wanted) };
        }
        // Gain interpolé sur la trame : pas de marche audible.
        let (g0, g1) = (db_to_lin(before), db_to_lin(self.gain_db));
        let step = (g1 - g0) / x.len() as f32;
        for (i, s) in x.iter_mut().enumerate() {
            *s = soft_limit(*s * (g0 + step * i as f32));
        }
    }
}

/// Limiteur doux : identité sous `KNEE`, puis tend vers 1 sans l'atteindre.
fn soft_limit(s: f32) -> f32 {
    let a = s.abs();
    if a <= KNEE {
        return s;
    }
    let room = 1.0 - KNEE;
    (KNEE + room * ((a - KNEE) / room).tanh()).min(CEILING).copysign(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_frames(amp: f32, frames: usize) -> Vec<Vec<f32>> {
        (0..frames).map(|f| (0..960).map(|i| amp * (2.0 * std::f32::consts::PI * 440.0 * (f * 960 + i) as f32 / 48_000.0).sin()).collect()).collect()
    }

    fn run(amp: f32, seconds: usize) -> (Agc, Vec<f32>) {
        let mut agc = Agc::new();
        let mut last = Vec::new();
        for mut f in sine_frames(amp, seconds * 50) {
            agc.process(&mut f);
            last = f;
        }
        (agc, last)
    }

    #[test]
    fn quiet_voice_is_raised_toward_the_target() {
        // -43 dB efficace : relevé au maximum (+20 dB) en quelques secondes.
        let (agc, last) = run(0.01, 6);
        assert!((agc.gain_db() - MAX_GAIN_DB).abs() < 0.5, "{}", agc.gain_db());
        assert!(rms_db(&last) > -25.0);
        // -28 dB : atteint la cible.
        let (_, last) = run(0.056, 6);
        assert!((rms_db(&last) - TARGET_DB).abs() < 1.5, "{}", rms_db(&last));
    }

    #[test]
    fn loud_voice_is_lowered_without_clipping() {
        let (agc, last) = run(0.95, 2);
        assert!(agc.gain_db() < -8.0, "{}", agc.gain_db());
        assert!((rms_db(&last) - TARGET_DB).abs() < 2.0);
        let mut burst = vec![1.0f32; 960];
        Agc::new().process(&mut burst);
        assert!(burst.iter().all(|s| s.abs() < 1.0));
    }

    #[test]
    fn silence_is_not_pumped_up() {
        let (agc, _) = run(0.001, 5);
        assert_eq!(agc.gain_db(), 0.0);
    }

    #[test]
    fn limiter_is_continuous() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert!((soft_limit(KNEE + 1e-4) - KNEE).abs() < 1e-3);
        assert!(soft_limit(-5.0) > -1.0 && soft_limit(-5.0) < -0.99);
    }
}
