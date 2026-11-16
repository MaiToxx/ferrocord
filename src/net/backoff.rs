//! Attentes avec gigue (lot « fiable ») : un client qui retente à heure fixe
//! (1 s, 2 s, 4 s pile) se reconnaît, et mille clients coupés ensemble
//! reviennent ensemble. Fonctions pures : le hasard est passé en paramètre
//! (`unit`, dans [0, 1[), pour des tests déterministes.

use std::time::Duration;

/// Nombre au hasard dans [0, 1[ (générateur du système ; à défaut, les clés
/// de hachage aléatoires de la bibliothèque standard : il ne s'agit que
/// d'étaler des attentes).
pub fn unit() -> f64 {
    let mut bytes = [0u8; 8];
    if aws_lc_rs::rand::fill(&mut bytes).is_err() {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u8(1);
        bytes = h.finish().to_be_bytes();
    }
    // 53 bits : tout ce qu'un f64 représente exactement.
    (u64::from_be_bytes(bytes) >> 11) as f64 / (1u64 << 53) as f64
}

fn clamp_unit(u: f64) -> f64 {
    if u.is_finite() { u.clamp(0.0, 1.0) } else { 0.0 }
}

/// Gigue « égale » : entre la moitié de `d` et `d`. Jamais plus que `d` (le
/// plafond tient), jamais moins que sa moitié (pas de nouvel essai immédiat).
pub fn equal_jitter(d: Duration, unit: f64) -> Duration {
    let half = d / 2;
    half + half.mul_f64(clamp_unit(unit))
}

/// Durée au hasard entre `min` et `max` (bornes dans le désordre tolérées).
pub fn between(min: Duration, max: Duration, unit: f64) -> Duration {
    let (lo, hi) = if min <= max { (min, max) } else { (max, min) };
    lo + (hi - lo).mul_f64(clamp_unit(unit))
}

/// Attente avant le `attempt`-ième nouvel essai (à partir de 0) : `base`
/// doublé à chaque fois, plafonné à `cap`, avec gigue égale.
pub fn retry_delay(base: Duration, attempt: u32, cap: Duration, unit: f64) -> Duration {
    equal_jitter(base.checked_mul(1 << attempt.min(16)).unwrap_or(cap).min(cap), unit)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_stays_in_range_and_varies() {
        let draws: Vec<f64> = (0..64).map(|_| unit()).collect();
        assert!(draws.iter().all(|u| (0.0..1.0).contains(u)), "{draws:?}");
        assert!(draws.iter().any(|u| (u - draws[0]).abs() > 1e-9), "tirages tous identiques");
    }

    #[test]
    fn jitter_never_exceeds_the_delay_nor_halves_below() {
        let s = Duration::from_secs;
        assert_eq!(equal_jitter(s(8), 0.0), s(4));
        assert_eq!(equal_jitter(s(8), 0.5), s(6));
        assert!(equal_jitter(s(8), 0.999_999) < s(8));
        // Hasard hors bornes ou illisible : borné, jamais de panique.
        assert_eq!(equal_jitter(s(8), 7.0), s(8));
        assert_eq!(equal_jitter(s(8), -1.0), s(4));
        assert_eq!(equal_jitter(s(8), f64::NAN), s(4));
        assert_eq!(equal_jitter(Duration::ZERO, 0.5), Duration::ZERO);
    }

    #[test]
    fn between_and_retry_delay() {
        let s = Duration::from_secs;
        assert_eq!(between(s(1), s(5), 0.0), s(1));
        assert_eq!(between(s(1), s(5), 0.5), s(3));
        assert_eq!(between(s(5), s(1), 1.0), s(5));
        let d: Vec<Duration> = (0..5).map(|a| retry_delay(s(1), a, s(10), 0.999_999_999)).collect();
        assert!(d[0] <= s(1) && d[1] > s(1) && d[1] <= s(2) && d[3] <= s(8) && d[4] <= s(10) && d[4] > s(9), "{d:?}");
        assert_eq!(retry_delay(Duration::MAX, 40, s(10), 0.0), s(5));
    }
}
