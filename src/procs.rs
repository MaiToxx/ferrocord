//! Programmes en cours (lot C3, détection des jeux). Seul endroit qui
//! interroge le système sur les autres processus ; `unsafe` limité aux
//! appels Win32 qui l'exigent (aucune voie sûre dans `std` ni dans les
//! dépendances déjà présentes), chaque bloc justifié.
//!
//! La détection des jeux n'ouvre AUCUN processus : [`for_each`] parcourt un
//! instantané ToolHelp (identifiant et nom de fichier, lisibles même pour un
//! programme protégé par un anti-triche ou lancé en administrateur) sans rien
//! allouer, et [`foreground_pid`] demande à quel processus appartient la
//! fenêtre de premier plan. [`list`] en est la forme allouée (installation).
//!
//! [`details`] ouvre un processus en lecture limitée pour son chemin et son
//! heure de lancement : réservé aux exemplaires de Ferrocord lui-même
//! (installation) et à l'application qui s'est branchée d'elle-même sur le
//! tube de la Rich Presence. Jamais pour un jeu : un anti-triche ou un
//! programme élevé le refuse, et le jeu restait alors invisible (défaut
//! corrigé par le lot « activité »).

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
pub use win::{details, for_each, foreground_pid};

/// Programmes en cours (forme allouée de [`for_each`]).
pub fn list() -> Vec<Proc> {
    let mut out = Vec::new();
    for_each(|pid, name| out.push(Proc { pid, file: String::from_utf16_lossy(name) }));
    out
}

/// Sans relevé possible : aucun programme.
#[cfg(not(windows))]
pub fn for_each(_see: impl FnMut(u32, &[u16])) {}

#[cfg(not(windows))]
pub fn foreground_pid() -> Option<u32> {
    None
}

#[cfg(not(windows))]
pub fn details(_pid: u32) -> Option<Details> {
    None
}

#[cfg(windows)]
mod win {
    use super::Details;
    use windows::core::PWSTR;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
    use windows::Win32::System::Threading::{GetProcessTimes, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

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

    /// Appelle `see(identifiant, nom de fichier en UTF-16)` pour chaque
    /// programme en cours. Un instantané ToolHelp : aucun processus n'est
    /// ouvert, rien n'est alloué ici (la structure vit sur la pile).
    pub fn for_each(mut see: impl FnMut(u32, &[u16])) {
        // SAFETY: appel sans pointeur ; le descripteur rendu est possédé par `Owned`.
        let Ok(snap) = (unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }) else { return };
        let snap = Owned(snap);
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        // SAFETY: `e` est une structure valide dont `dwSize` est renseigné,
        // comme l'exige l'API ; le descripteur reste ouvert pendant l'appel.
        let mut ok = unsafe { Process32FirstW(snap.0, &mut e) }.is_ok();
        let mut n = 0;
        while ok && n < MAX_PROCS {
            if e.th32ProcessID != 0 {
                let end = e.szExeFile.iter().position(|c| *c == 0).unwrap_or(e.szExeFile.len());
                see(e.th32ProcessID, &e.szExeFile[..end]);
                n += 1;
            }
            // SAFETY: mêmes garanties que pour `Process32FirstW`.
            ok = unsafe { Process32NextW(snap.0, &mut e) }.is_ok();
        }
    }

    /// Processus de la fenêtre de premier plan (le jeu auquel on joue, s'il
    /// y en a plusieurs). Deux questions au gestionnaire de fenêtres : le
    /// processus n'est ni ouvert ni lu.
    pub fn foreground_pid() -> Option<u32> {
        // SAFETY: appel sans argument ; rend une poignée de fenêtre, nulle s'il n'y en a pas.
        let window = unsafe { GetForegroundWindow() };
        if window.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        // SAFETY: `pid` est un entier local valide pendant l'appel ; une
        // poignée périmée fait seulement rendre 0.
        let thread = unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        (thread != 0 && pid != 0).then_some(pid)
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

        /// Ce processus-ci seulement (le vrai instantané des programmes
        /// n'est pris que par un test, dans `net::games`).
        #[test]
        fn details_of_this_process() {
            let me = std::process::id();
            let d = details(me).unwrap_or_else(|| panic!("détails de ce processus"));
            assert!(d.path.to_lowercase().ends_with(".exe"), "{}", d.path);
            let now = chrono::Utc::now().timestamp_millis();
            assert!(d.started_ms.is_some_and(|ms| ms <= now && ms > now - 24 * 3600 * 1000));
            assert_eq!(details(u32::MAX - 3), None);
            let _ = foreground_pid();
        }
    }
}
