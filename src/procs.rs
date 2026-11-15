//! Programmes en cours (lot C3, détection des jeux). Seul endroit qui
//! interroge le système sur les autres processus ; `unsafe` limité aux
//! appels Win32 qui l'exigent (aucune voie sûre dans `std` ni dans les
//! dépendances déjà présentes), chaque bloc justifié.
//!
//! Deux étages, pour rester léger : [`list`] ne rend que les identifiants
//! et les noms de fichier (un instantané ToolHelp, sans ouvrir aucun
//! processus) ; [`details`] ouvre un processus en lecture limitée pour son
//! chemin et son heure de lancement, et n'est appelé que pour un nom de
//! fichier qui peut être un jeu, une fois par processus.

/// Processus vu par l'instantané.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proc {
    pub pid: u32,
    /// Nom de fichier de l'exécutable (`jeu.exe`).
    pub file: String,
}

/// Chemin complet et heure de lancement (ms Unix) d'un processus.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Details {
    pub path: String,
    pub started_ms: Option<i64>,
}

#[cfg(windows)]
pub use win::{details, list};

#[cfg(not(windows))]
pub fn list() -> Vec<Proc> {
    Vec::new()
}

#[cfg(not(windows))]
pub fn details(_pid: u32) -> Option<Details> {
    None
}

#[cfg(windows)]
mod win {
    use super::{Details, Proc};
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};

    /// Processus au-delà : ignorés (borne de sûreté, un poste en a quelques centaines).
    const MAX_PROCS: usize = 8192;

    /// Ferme le descripteur à la sortie de portée.
    struct Owned(HANDLE);

    impl Drop for Owned {
        fn drop(&mut self) {
            // SAFETY: le descripteur vient d'un appel réussi qui le donne à
            // l'appelant, il n'est fermé qu'ici, une seule fois.
            unsafe {
                let _ = CloseHandle(self.0);
            }
        }
    }

    fn utf16(buf: &[u16]) -> String {
        let end = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
        String::from_utf16_lossy(&buf[..end])
    }

    pub fn list() -> Vec<Proc> {
        let mut out = Vec::new();
        // SAFETY: appel sans pointeur ; le descripteur rendu est possédé par `Owned`.
        let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return out };
        let snap = Owned(snap);
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        // SAFETY: `e` est une structure valide dont `dwSize` est renseigné,
        // comme l'exige l'API ; le descripteur reste ouvert pendant l'appel.
        let mut ok = unsafe { Process32FirstW(snap.0, &mut e) }.is_ok();
        while ok && out.len() < MAX_PROCS {
            if e.th32ProcessID != 0 {
                out.push(Proc { pid: e.th32ProcessID, file: utf16(&e.szExeFile) });
            }
            // SAFETY: mêmes garanties que pour `Process32FirstW`.
            ok = unsafe { Process32NextW(snap.0, &mut e) }.is_ok();
        }
        out
    }

    fn filetime_ms(ft: FILETIME) -> Option<i64> {
        let t = (u64::from(ft.dwHighDateTime) << 32) | u64::from(ft.dwLowDateTime);
        // Intervalles de 100 ns depuis 1601 ; 11 644 473 600 s jusqu'à 1970.
        let ms = i64::try_from(t / 10_000).ok()? - 11_644_473_600_000;
        (ms > 0).then_some(ms)
    }

    pub fn details(pid: u32) -> Option<Details> {
        // SAFETY: appel sans pointeur ; le descripteur rendu est possédé par `Owned`.
        let h = Owned(unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?);
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        // SAFETY: `buf` est valide pour `len` caractères, `len` est mis à
        // jour par l'API avec la longueur écrite (sans le zéro final).
        unsafe { QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len) }.ok()?;
        let path = String::from_utf16_lossy(buf.get(..len as usize)?);
        let (mut created, mut exited, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default(), FILETIME::default());
        // SAFETY: quatre structures locales valides, descripteur ouvert avec
        // le droit de lecture limitée que demande l'API.
        let started_ms = unsafe { GetProcessTimes(h.0, &mut created, &mut exited, &mut kernel, &mut user) }.ok().and_then(|_| filetime_ms(created));
        Some(Details { path, started_ms })
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn lists_this_process() {
            let me = std::process::id();
            let all = list();
            let mine = all.iter().find(|p| p.pid == me).unwrap_or_else(|| panic!("ce processus est listé"));
            assert!(mine.file.to_lowercase().ends_with(".exe"), "{}", mine.file);
            let d = details(me).unwrap_or_else(|| panic!("détails de ce processus"));
            assert!(d.path.to_lowercase().ends_with(&mine.file.to_lowercase()), "{}", d.path);
            let now = chrono::Utc::now().timestamp_millis();
            assert!(d.started_ms.is_some_and(|ms| ms <= now && ms > now - 24 * 3600 * 1000));
            assert_eq!(details(u32::MAX - 3), None);
        }
    }
}
