//! Pointeur pendant un glisser-déposer de fichiers venu du système.
//!
//! Tant qu'un fichier est tiré au-dessus de la fenêtre, Windows n'envoie ni
//! mouvement de souris ni touche à la fenêtre : la position et l'état de Maj
//! connus de l'interface datent d'avant le geste. Ils sont donc demandés au
//! système, au moment où l'interface en a besoin.

/// Position du pointeur dans la zone cliente de la fenêtre principale, en
/// pixels physiques, et touche Maj enfoncée. `None` : fenêtre inconnue, ou
/// système sans cette question (l'interface se rabat sur ce qu'elle sait).
#[cfg(windows)]
pub fn pointer() -> Option<(f32, f32, bool)> {
    use windows::Win32::Foundation::{HWND, POINT};
    use windows::Win32::Graphics::Gdi::ScreenToClient;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

    let hwnd = super::backdrop::window();
    if hwnd == 0 {
        return None;
    }
    let mut pt = POINT::default();
    // SAFETY: `pt` est sur la pile, vivant pendant les deux appels ; `hwnd` est
    // la fenêtre principale retenue à sa création (une fenêtre détruite fait
    // seulement échouer `ScreenToClient`). `GetAsyncKeyState` ne prend qu'un entier.
    let (ok, shift) = unsafe { (GetCursorPos(&mut pt).is_ok() && ScreenToClient(HWND(hwnd as *mut _), &mut pt).as_bool(), (GetAsyncKeyState(i32::from(VK_SHIFT.0)) as u16) & 0x8000 != 0) };
    ok.then_some((pt.x as f32, pt.y as f32, shift))
}

#[cfg(not(windows))]
pub fn pointer() -> Option<(f32, f32, bool)> {
    None
}
