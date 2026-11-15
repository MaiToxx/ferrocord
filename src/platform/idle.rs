//! Horloge de dernière entrée (clavier ou souris) de la session Windows,
//! pour le statut « inactif » automatique (`net::idle`). Un seul appel
//! Win32, sans équivalent sûr dans `std` ni dans les dépendances.

/// Valeur de l'horloge système (`GetTickCount`) à la dernière entrée de
/// l'utilisateur, toutes applications confondues. `None` hors de Windows ou
/// si le système refuse : pas de statut inactif automatique.
pub fn last_input_tick() -> Option<u32> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
        let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
        // SAFETY: `info` est une structure valide, vivante pendant l'appel,
        // dont `cbSize` est renseigné comme l'exige l'API ; l'appel n'écrit
        // que dans cette structure.
        let ok = unsafe { GetLastInputInfo(&mut info) }.as_bool();
        ok.then_some(info.dwTime)
    }
    #[cfg(not(windows))]
    None
}
