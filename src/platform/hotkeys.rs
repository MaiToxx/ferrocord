//! Raccourcis globaux (F22) : des combinaisons qui marchent quand la fenêtre
//! n'a pas le clavier, pour couper le micro ou le casque depuis un jeu.
//!
//! Windows : `RegisterHotKey`, sur un fil à lui qui fait tourner la boucle de
//! messages où arrivent les `WM_HOTKEY`. Aucune frappe n'est lue : Windows ne
//! prévient que pour les combinaisons inscrites. En contrepartie, une
//! combinaison inscrite n'arrive plus aux autres applications tant qu'elle
//! l'est, et une combinaison déjà prise par une autre application est refusée.

use std::sync::{Arc, Mutex};

/// Combinaison à inscrire ; `id` revient par [`take`] quand elle est frappée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Binding {
    pub id: i32,
    /// Code de touche virtuelle de Windows.
    pub vk: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

impl Binding {
    /// Combinaison acceptable comme raccourci global : avec Ctrl ou Alt, ou
    /// une touche de fonction. Une lettre seule, même avec Maj, serait
    /// retirée à la saisie de toutes les autres applications.
    pub fn safe(&self) -> bool {
        const VK_F1: u32 = 0x70;
        const VK_F24: u32 = 0x87;
        self.ctrl || self.alt || (VK_F1..=VK_F24).contains(&self.vk)
    }
}

/// Raccourcis frappés et pas encore repris par l'interface.
static FIRED: Mutex<Vec<i32>> = Mutex::new(Vec::new());
/// Borne : l'interface reprend la file à chaque image.
const MAX_FIRED: usize = 64;

/// Reprend les raccourcis frappés depuis le dernier appel, dans l'ordre.
pub fn take() -> Vec<i32> {
    FIRED.lock().map(|mut f| std::mem::take(&mut *f)).unwrap_or_default()
}

#[cfg_attr(not(windows), allow(dead_code))]
fn fired(id: i32) {
    if let Ok(mut f) = FIRED.lock()
        && f.len() < MAX_FIRED
    {
        f.push(id);
    }
}

pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[cfg(windows)]
mod imp {
    use super::{fired, Binding, Wake};
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use windows::Win32::Foundation::{LPARAM, WPARAM};
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, PeekMessageW, PostThreadMessageW, MSG, PM_NOREMOVE, WM_HOTKEY, WM_QUIT};

    pub struct Hotkeys {
        thread_id: u32,
        thread: Option<JoinHandle<()>>,
        /// Combinaisons refusées par Windows (déjà prises ailleurs).
        pub refused: Vec<i32>,
    }

    impl Hotkeys {
        /// Inscrit les combinaisons. `None` : le fil n'a pas pu démarrer.
        pub fn start(bindings: Vec<Binding>, wake: Wake) -> Option<Hotkeys> {
            let (tx, rx) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("ferrocord-raccourcis".into())
                .spawn(move || {
                    let mut msg = MSG::default();
                    // SAFETY: `msg` est une variable locale valide ; l'appel crée
                    // la file de messages du fil (pour WM_HOTKEY et WM_QUIT).
                    let id = unsafe {
                        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                        GetCurrentThreadId()
                    };
                    let mut held = Vec::new();
                    let mut refused = Vec::new();
                    for b in &bindings {
                        let mut mods = MOD_NOREPEAT;
                        for (on, m) in [(b.ctrl, MOD_CONTROL), (b.alt, MOD_ALT), (b.shift, MOD_SHIFT)] {
                            if on {
                                mods = HOT_KEY_MODIFIERS(mods.0 | m.0);
                            }
                        }
                        // SAFETY: aucun pointeur ; sans fenêtre, le raccourci est
                        // lié à ce fil, qui le retire avant de finir.
                        if unsafe { RegisterHotKey(None, b.id, mods, b.vk) }.is_ok() {
                            held.push(b.id);
                        } else {
                            refused.push(b.id);
                        }
                    }
                    let _ = tx.send((id, refused));
                    // SAFETY: boucle de messages du fil ; `msg` reste une variable
                    // locale valide. Les raccourcis sont retirés sur ce même fil.
                    unsafe {
                        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                            if msg.message == WM_HOTKEY {
                                fired(msg.wParam.0 as i32);
                                wake();
                            }
                        }
                        for id in held {
                            let _ = UnregisterHotKey(None, id);
                        }
                    }
                })
                .ok()?;
            match rx.recv() {
                Ok((thread_id, refused)) => Some(Hotkeys { thread_id, thread: Some(thread), refused }),
                Err(_) => {
                    let _ = thread.join();
                    None
                }
            }
        }
    }

    impl Drop for Hotkeys {
        fn drop(&mut self) {
            // SAFETY: simple envoi d'un message au fil des raccourcis, identifié
            // par son numéro ; aucun pointeur.
            let posted = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }.is_ok();
            if posted && let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::{Binding, Wake};

    pub struct Hotkeys {
        pub refused: Vec<i32>,
    }

    impl Hotkeys {
        pub fn start(_bindings: Vec<Binding>, _wake: Wake) -> Option<Hotkeys> {
            None
        }
    }
}

pub use imp::Hotkeys;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_combinations_that_do_not_steal_typing_are_safe() {
        let b = |vk, ctrl, alt, shift| Binding { id: 1, vk, ctrl, alt, shift };
        assert!(b(0x4D, true, false, true).safe(), "Ctrl+Maj+M");
        assert!(b(0x4D, false, true, false).safe(), "Alt+M");
        assert!(b(0x79, false, false, false).safe(), "F10 seule");
        assert!(!b(0x4D, false, false, false).safe(), "M seule");
        assert!(!b(0x4D, false, false, true).safe(), "Maj+M");
        assert!(!b(0x0D, false, false, false).safe(), "Entrée");
    }

    #[test]
    fn fired_shortcuts_are_taken_once() {
        // Seul test à toucher la file (les raccourcis réels ne sont jamais inscrits pendant les tests).
        let _ = take();
        fired(7);
        fired(9);
        assert_eq!(take(), vec![7, 9]);
        assert!(take().is_empty());
    }
}
