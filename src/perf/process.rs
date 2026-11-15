//! Âge du processus (depuis sa création par Windows), pour situer le début
//! de `run` dans le démarrage mesuré de l'extérieur (`tools/mesures.ps1`) :
//! chargement de l'exécutable et des DLL, initialisation avant `main`.

use std::time::{SystemTime, UNIX_EPOCH};
use windows::Win32::{
    Foundation::FILETIME,
    System::Threading::{GetCurrentProcess, GetProcessTimes},
};

/// Intervalles de 100 ns entre 1601 (origine des FILETIME) et 1970.
const EPOCH_1601: u64 = 116_444_736_000_000_000;

/// Millisecondes écoulées depuis la création du processus ; `None` si Windows ne la donne pas.
pub fn age_ms() -> Option<f64> {
    let mut created = FILETIME::default();
    let (mut exit, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    // SAFETY: GetCurrentProcess rend un pseudo-handle constant, valable sans
    // fermeture ; les quatre pointeurs désignent des FILETIME locaux, vivants
    // pendant tout l'appel, que la fonction se contente de remplir.
    unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut exit, &mut kernel, &mut user) }.ok()?;
    let created = (u64::from(created.dwHighDateTime) << 32) | u64::from(created.dwLowDateTime);
    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    let now = EPOCH_1601 + u64::try_from(now.as_nanos() / 100).ok()?;
    Some(now.checked_sub(created)? as f64 / 10_000.0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn age_is_positive_and_plausible() {
        let a = super::age_ms().expect("âge du processus");
        assert!((0.0..24.0 * 3600.0 * 1000.0).contains(&a), "{a}");
    }
}
