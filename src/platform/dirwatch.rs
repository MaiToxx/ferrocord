//! Signal « quelque chose a changé dans ce dossier » (lot « cpu »).
//!
//! La surveillance de `theme.toml` et de `settings.toml` relevait la date du
//! fichier quatre fois par seconde : sur un vrai compte au repos, ces deux
//! fils coûtaient 109 ms de processeur par minute, le premier poste après
//! les images. Windows sait prévenir d'un changement dans un dossier
//! (`FindFirstChangeNotificationW`) : interroger ce signal ne touche pas au
//! système de fichiers (un appel, sans ouverture ni lecture), et le fichier
//! n'est relevé que quand le signal est levé.
//!
//! Ailleurs que sous Windows, [`DirSignal::open`] rend `None` : l'appelant
//! garde son relevé périodique.

use std::path::Path;

/// Abonnement aux changements d'un dossier (création, renommage, écriture,
/// taille des fichiers qu'il contient, sans ses sous-dossiers).
pub struct DirSignal {
    #[cfg(windows)]
    handle: windows::Win32::Foundation::HANDLE,
}

// SAFETY: le descripteur d'un abonnement n'est lié à aucun fil : Windows
// permet de l'attendre et de le fermer depuis n'importe lequel. `DirSignal`
// n'offre aucun accès partagé (`changed` prend `&mut self`).
#[cfg(windows)]
unsafe impl Send for DirSignal {}

impl DirSignal {
    /// `None` si le dossier n'existe pas (encore) ou si le système refuse.
    #[cfg(windows)]
    pub fn open(dir: &Path) -> Option<DirSignal> {
        use std::os::windows::ffi::OsStrExt;
        use windows::core::PCWSTR;
        use windows::Win32::Storage::FileSystem::{FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE};
        let wide: Vec<u16> = dir.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` est une chaîne UTF-16 terminée par zéro, vivante
        // pendant l'appel ; le descripteur rendu est possédé par `DirSignal`,
        // qui le ferme (`Drop`).
        let handle = unsafe { FindFirstChangeNotificationW(PCWSTR(wide.as_ptr()), false, FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE) }.ok()?;
        (!handle.is_invalid()).then_some(DirSignal { handle })
    }

    #[cfg(not(windows))]
    pub fn open(_dir: &Path) -> Option<DirSignal> {
        None
    }

    /// Le dossier a-t-il changé depuis le dernier appel ? Ne bloque jamais.
    /// `None` : l'abonnement est perdu (dossier supprimé, disque retiré) ;
    /// l'appelant doit revenir au relevé périodique.
    #[cfg(windows)]
    pub fn changed(&mut self) -> Option<bool> {
        use windows::Win32::Foundation::{WAIT_OBJECT_0, WAIT_TIMEOUT};
        use windows::Win32::Storage::FileSystem::FindNextChangeNotification;
        use windows::Win32::System::Threading::WaitForSingleObject;
        // SAFETY: `handle` est un abonnement ouvert, possédé par `self` ;
        // attente de durée nulle, sans autre effet.
        match unsafe { WaitForSingleObject(self.handle, 0) } {
            WAIT_TIMEOUT => Some(false),
            WAIT_OBJECT_0 => {
                // SAFETY: même descripteur, toujours ouvert ; réarme le signal
                // (les changements survenus entre-temps le lèveront de nouveau).
                unsafe { FindNextChangeNotification(self.handle) }.ok()?;
                Some(true)
            }
            _ => None,
        }
    }

    #[cfg(not(windows))]
    pub fn changed(&mut self) -> Option<bool> {
        None
    }
}

#[cfg(windows)]
impl Drop for DirSignal {
    fn drop(&mut self) {
        // SAFETY: descripteur ouvert par `open`, fermé une seule fois ici.
        unsafe {
            let _ = windows::Win32::Storage::FileSystem::FindCloseChangeNotification(self.handle);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn dir(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-signal-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("dossier de test");
        d
    }

    fn raised(s: &mut DirSignal) -> bool {
        let until = Instant::now() + Duration::from_secs(5);
        while Instant::now() < until {
            if s.changed() == Some(true) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    #[test]
    fn quiet_until_a_file_is_written_then_raised_once_per_change() {
        let d = dir("ecriture");
        let file = d.join("theme.toml");
        std::fs::write(&file, "a = 1\n").expect("écriture");
        let mut s = DirSignal::open(&d).expect("abonnement");
        // Rien n'a changé depuis l'abonnement.
        assert_eq!(s.changed(), Some(false));
        // Écriture en place, puis remplacement par renommage (éditeurs, écriture atomique).
        std::fs::write(&file, "a = 2\n").expect("écriture");
        assert!(raised(&mut s), "une écriture doit lever le signal");
        while s.changed() == Some(true) {}
        std::fs::write(d.join("theme.tmp"), "a = 3\n").expect("écriture");
        std::fs::rename(d.join("theme.tmp"), &file).expect("renommage");
        assert!(raised(&mut s), "un remplacement doit lever le signal");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_missing_directory_gives_no_signal() {
        let d = std::env::temp_dir().join(format!("ferrocord-signal-absent-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(DirSignal::open(&d).is_none());
    }
}
