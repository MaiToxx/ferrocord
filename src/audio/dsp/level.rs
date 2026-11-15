//! Niveau d'une trame et porte de détection de la voix (seuil fixe ou
//! « automatique » d'après le bruit de fond).

/// Plancher des niveaux (silence numérique).
pub const FLOOR_DB: f32 = -100.0;

/// Niveau efficace d'une trame, en dB pleine échelle.
pub fn rms_db(x: &[f32]) -> f32 {
    if x.is_empty() {
        return FLOOR_DB;
    }
    let sum: f64 = x.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    let mean = sum / x.len() as f64;
    if mean <= 1e-10 { FLOOR_DB } else { ((10.0 * mean.log10()) as f32).max(FLOOR_DB) }
}

/// Seuil « automatique » : au-dessus du bruit de fond d'autant.
const AUTO_MARGIN_DB: f32 = 15.0;
/// Bornes du seuil automatique : jamais si bas qu'un souffle ouvre la
/// porte, jamais si haut qu'une voix normale ne l'ouvre plus.
const AUTO_MIN_DB: f32 = -70.0;
const AUTO_MAX_DB: f32 = -30.0;
/// Remontée du bruit de fond, par trame de 20 ms (1,5 dB/s) : une voix
/// continue ne passe pas pour du bruit, un bruit nouveau est appris en
/// quelques secondes.
const FLOOR_RISE_DB: f32 = 0.03;
/// Trames gardées ouvertes après la dernière trame forte (300 ms) : les
/// fins de mots ne sont pas coupées.
const HOLD_FRAMES: u32 = 15;

/// Porte de la détection de la voix, une décision par trame.
#[derive(Clone, Debug)]
pub struct VoiceGate {
    pub auto: bool,
    pub threshold_db: f32,
    floor_db: f32,
    hold: u32,
}

impl VoiceGate {
    pub fn new(auto: bool, threshold_db: f32) -> Self {
        VoiceGate { auto, threshold_db, floor_db: -60.0, hold: 0 }
    }

    /// Seuil en vigueur (celui réglé, ou celui déduit du bruit de fond).
    pub fn threshold(&self) -> f32 {
        if self.auto { (self.floor_db + AUTO_MARGIN_DB).clamp(AUTO_MIN_DB, AUTO_MAX_DB) } else { self.threshold_db }
    }

    /// Niveau de la trame ; renvoie « porte ouverte ».
    pub fn update(&mut self, level_db: f32) -> bool {
        // Le bruit de fond descend tout de suite, remonte lentement.
        self.floor_db = if level_db < self.floor_db { level_db.max(FLOOR_DB) } else { (self.floor_db + FLOOR_RISE_DB).min(level_db) };
        if level_db >= self.threshold() {
            self.hold = HOLD_FRAMES;
            return true;
        }
        if self.hold > 0 {
            self.hold -= 1;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels() {
        assert_eq!(rms_db(&[]), FLOOR_DB);
        assert_eq!(rms_db(&[0.0; 960]), FLOOR_DB);
        assert!((rms_db(&[1.0; 960])).abs() < 1e-4);
        assert!((rms_db(&[0.1; 960]) + 20.0).abs() < 1e-3);
    }

    #[test]
    fn auto_gate_learns_the_room_and_opens_on_voice() {
        let mut g = VoiceGate::new(true, -40.0);
        // 10 s d'un bruit de fond à -55 dB : rien ne passe.
        for _ in 0..500 {
            assert!(!g.update(-55.0));
        }
        assert!(!g.update(-55.0));
        assert!((g.threshold() - (-40.0)).abs() < 1.0, "seuil {}", g.threshold());
        assert!(g.update(-25.0), "une voix ouvre la porte");
        // Fin de mot gardée, puis fermeture.
        assert!(g.update(-60.0));
        for _ in 0..HOLD_FRAMES {
            g.update(-60.0);
        }
        assert!(!g.update(-60.0));
    }

    #[test]
    fn manual_threshold_is_used_as_is() {
        let mut g = VoiceGate::new(false, -20.0);
        assert!(!g.update(-25.0));
        assert!(g.update(-19.0));
        assert_eq!(g.threshold(), -20.0);
    }
}
