//! Limiteur doux du mixage (la mesure de niveau est `dsp::level::rms_db`).

/// Crête visée après réduction de gain (environ -0,9 dBFS).
const CEILING: f32 = 0.9;
/// Remontée du gain vers 1 par trame de 20 ms (constante d'environ 200 ms).
const RELEASE: f32 = 0.1;

/// Réduit le gain d'une trame dès que sa crête dépasse le plafond (la trame
/// entière sert d'anticipation, d'où une attaque immédiate sans dépassement),
/// puis le remonte lentement, en rampe le long de la trame pour ne pas claquer.
/// Un coude doux, par sécurité, garde tout échantillon sous 1.
pub struct Limiter {
    gain: f32,
}

impl Default for Limiter {
    fn default() -> Self {
        Self { gain: 1.0 }
    }
}

impl Limiter {
    pub fn apply(&mut self, buf: &mut [f32]) {
        let peak = buf.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        let target = if peak > CEILING { CEILING / peak } else { 1.0 };
        let mut to = if target < self.gain { target } else { (self.gain + (1.0 - self.gain) * RELEASE).min(target) };
        if to > 0.999 {
            to = 1.0;
        }
        let from = if to < self.gain { to } else { self.gain };
        self.gain = to;
        if from == 1.0 && to == 1.0 {
            return;
        }
        let n = buf.len().max(1) as f32;
        for (i, x) in buf.iter_mut().enumerate() {
            let g = from + (to - from) * (i as f32 / n);
            *x = soft_clip(*x * g);
        }
    }
}

fn soft_clip(x: f32) -> f32 {
    let a = x.abs();
    if a <= CEILING {
        return x;
    }
    // Un peu sous 1 - CEILING : en f32, tanh sature à 1 et le résultat toucherait 1.
    let knee = 0.99 - CEILING;
    (CEILING + knee * ((a - CEILING) / knee).tanh()).copysign(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limiteur_empeche_l_ecretage_et_laisse_le_reste() {
        let mut l = Limiter::default();
        let mut quiet: Vec<f32> = (0..1920).map(|i| 0.3 * (i as f32 * 0.05).sin()).collect();
        let orig = quiet.clone();
        l.apply(&mut quiet);
        assert_eq!(quiet, orig);
        for _ in 0..20 {
            // Trois voix pleines additionnées : crête 3.
            let mut loud: Vec<f32> = (0..1920).map(|i| 3.0 * (i as f32 * 0.05).sin()).collect();
            l.apply(&mut loud);
            assert!(loud.iter().all(|x| x.abs() < 1.0));
        }
        // Le gain remonte ensuite progressivement.
        let mut after = orig.clone();
        l.apply(&mut after);
        assert!(after.iter().all(|x| x.abs() < 0.3 + 1e-6));
        for _ in 0..100 {
            after = orig.clone();
            l.apply(&mut after);
        }
        assert!((after[100] - orig[100]).abs() < 0.01);
    }
}
