//! Raccourcis `.lnk` (menu Démarrer, bureau) derrière une petite interface :
//! le vrai par l'interface COM du Shell de Windows (`IShellLinkW`, seule voie
//! pour écrire un `.lnk` ; `unsafe` limité à ces appels), et un faux pour
//! les tests, qui n'écrivent jamais dans le vrai menu Démarrer.

use std::io;
use std::path::Path;

/// Ce qu'un raccourci lance.
#[derive(Clone, Copy, Debug)]
pub struct Shortcut<'a> {
    pub target: &'a Path,
    /// Dossier de travail (celui de l'exécutable).
    pub dir: &'a Path,
    pub description: &'a str,
}

pub trait Shell {
    /// Écrit (ou remplace) le raccourci `lnk`.
    fn create(&mut self, lnk: &Path, s: &Shortcut<'_>) -> io::Result<()>;
}

/// Faux Shell (tests) : un fichier texte qui nomme la cible.
#[derive(Clone, Copy, Debug, Default)]
pub struct PlainShell;

impl Shell for PlainShell {
    fn create(&mut self, lnk: &Path, s: &Shortcut<'_>) -> io::Result<()> {
        std::fs::write(lnk, format!("{}\n{}\n{}\n", s.target.display(), s.dir.display(), s.description))
    }
}

/// Le Shell de Windows.
#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsShell;

#[cfg(windows)]
impl Shell for WindowsShell {
    fn create(&mut self, lnk: &Path, s: &Shortcut<'_>) -> io::Result<()> {
        // Dans un fil à part : COM y est ouvert puis refermé sans rien
        // changer à celui du fil appelant (la fenêtre a le sien).
        std::thread::scope(|scope| scope.spawn(|| win::create(lnk, s)).join()).map_err(|_| io::Error::other("raccourci : fil interrompu"))?.map_err(|e| io::Error::other(format!("raccourci : {e}")))
    }
}

#[cfg(windows)]
mod win {
    use super::Shortcut;
    use std::path::Path;
    use windows::core::{Interface, HSTRING};
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IPersistFile, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED};
    use windows::Win32::UI::Shell::{IShellLinkW, ShellLink};

    /// Referme COM à la sortie, après les interfaces (déclaré avant elles).
    struct Com;

    impl Drop for Com {
        fn drop(&mut self) {
            // SAFETY: appelé une fois, dans le fil qui a réussi `CoInitializeEx`.
            unsafe { CoUninitialize() };
        }
    }

    pub(super) fn create(lnk: &Path, s: &Shortcut<'_>) -> windows::core::Result<()> {
        // SAFETY: premier appel COM de ce fil, créé pour cela ; pas de pointeur.
        unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) }.ok()?;
        let _com = Com;
        let (target, dir, description, file) = (HSTRING::from(s.target), HSTRING::from(s.dir), HSTRING::from(s.description), HSTRING::from(lnk));
        // SAFETY: COM est ouvert dans ce fil ; `ShellLink` est la classe du
        // Shell qui implémente `IShellLinkW`.
        let link: IShellLinkW = unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER) }?;
        // SAFETY: chaque argument est une chaîne UTF-16 terminée par zéro
        // (`HSTRING`), vivante pendant l'appel ; l'interface les copie.
        unsafe {
            link.SetPath(&target)?;
            link.SetWorkingDirectory(&dir)?;
            link.SetIconLocation(&target, 0)?;
            link.SetDescription(&description)?;
        }
        let persist: IPersistFile = link.cast()?;
        // SAFETY: même garantie pour le chemin du fichier à écrire.
        unsafe { persist.Save(&file, true) }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// Le vrai Shell, mais dans un dossier temporaire : jamais le menu Démarrer.
    #[test]
    fn windows_writes_a_real_shortcut_file() {
        let dir = std::env::temp_dir().join(format!("ferrocord-raccourci-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let target = dir.join("ferrocord.exe");
        std::fs::write(&target, b"MZ").unwrap();
        let lnk = dir.join("Ferrocord.lnk");
        let s = Shortcut { target: &target, dir: &dir, description: "Client Discord natif et léger (non officiel)" };
        WindowsShell.create(&lnk, &s).unwrap();
        // Remplacer un raccourci existant ne doit pas échouer.
        WindowsShell.create(&lnk, &s).unwrap();
        let bytes = std::fs::read(&lnk).unwrap();
        // En-tête d'un fichier .lnk : taille 0x4C puis l'identifiant de classe du raccourci.
        assert_eq!(&bytes[..8], &[0x4C, 0, 0, 0, 0x01, 0x14, 0x02, 0x00]);
        // Le nom de la cible y figure en UTF-16.
        let name: Vec<u8> = "ferrocord.exe".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert!(bytes.windows(name.len()).any(|w| w == name));
        // L'icône est celle de la cible : drapeau « emplacement d'icône » (bit 6 des drapeaux, à
        // l'octet 0x14) et index 0 (à l'octet 0x38), le groupe d'icônes de l'exécutable.
        assert_ne!(bytes[0x14] & 0x40, 0, "emplacement d'icône absent");
        assert_eq!(&bytes[0x38..0x3C], &[0, 0, 0, 0], "index de l'icône");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
