//! Appui-pour-parler global : crochet clavier bas niveau de Windows
//! (`WH_KEYBOARD_LL`), pour que la touche marche quand la fenêtre n'a pas le
//! focus. Seul fichier `unsafe` du vocal : l'API Win32 l'exige, et aucune
//! crate déjà présente ne l'enveloppe.
//!
//! Posé seulement pendant un appel en appui-pour-parler (l'orchestrateur
//! tient le [`Hook`] et le lâche à la fin), sur un fil à lui qui fait tourner
//! la boucle de messages exigée par Windows. Le crochet ne regarde que la
//! touche choisie (`ptt_keys::filter`) : aucune autre frappe n'est lue ni
//! retenue, et toutes sont transmises telles quelles au reste du système.

use super::ptt_keys::Target;
use std::sync::atomic::{AtomicBool, Ordering};

/// Touche choisie enfoncée (lu par le fil de capture de l'appel).
static PRESSED: AtomicBool = AtomicBool::new(false);

/// La touche de l'appui-pour-parler est tenue, où que soit le focus.
pub fn pressed() -> bool {
    PRESSED.load(Ordering::Relaxed)
}

#[cfg(windows)]
mod imp {
    use super::super::ptt_keys::{filter, Mods, Target};
    use super::PRESSED;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::thread::JoinHandle;
    use windows::Win32::Foundation::{HINSTANCE, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::Threading::GetCurrentThreadId;
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, UnhookWindowsHookEx, HC_ACTION, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WM_QUIT,
    };

    /// Touche surveillée, lue par le rappel (qui ne peut rien capturer) :
    /// code en bas, modificateurs en bits 32 à 34, bit 35 = actif.
    static TARGET: AtomicU64 = AtomicU64::new(0);
    const ACTIVE: u64 = 1 << 35;

    fn pack(t: &Target) -> u64 {
        u64::from(t.vk) | (u64::from(t.ctrl) << 32) | (u64::from(t.alt) << 33) | (u64::from(t.shift) << 34) | ACTIVE
    }

    fn unpack(v: u64) -> Option<Target> {
        (v & ACTIVE != 0).then_some(Target { vk: v as u32, ctrl: v & (1 << 32) != 0, alt: v & (1 << 33) != 0, shift: v & (1 << 34) != 0 })
    }

    fn down(vk: u16) -> bool {
        // SAFETY: GetAsyncKeyState ne prend qu'un entier et ne touche à
        // aucune mémoire de l'appelant.
        (unsafe { GetAsyncKeyState(i32::from(vk)) } as u16) & 0x8000 != 0
    }

    fn mods() -> Mods {
        Mods { ctrl: down(VK_CONTROL.0), alt: down(VK_MENU.0), shift: down(VK_SHIFT.0) }
    }

    unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code == HC_ACTION as i32
            && let Some(t) = unpack(TARGET.load(Ordering::Relaxed))
        {
            // SAFETY: pour WH_KEYBOARD_LL et HC_ACTION, Windows garantit que
            // `lparam` pointe un KBDLLHOOKSTRUCT valide le temps de l'appel ;
            // on n'en lit qu'un champ, sans le garder.
            let vk = unsafe { (*(lparam.0 as *const KBDLLHOOKSTRUCT)).vkCode };
            if let Some(p) = filter(&t, wparam.0 as u32, vk, mods) {
                PRESSED.store(p, Ordering::Relaxed);
            }
        }
        // SAFETY: les paramètres reçus sont transmis tels quels au crochet
        // suivant, comme l'exige Windows ; la frappe n'est jamais retenue.
        unsafe { CallNextHookEx(None, code, wparam, lparam) }
    }

    pub struct Hook {
        thread_id: u32,
        thread: Option<JoinHandle<()>>,
    }

    impl Hook {
        pub fn start(target: Target) -> Option<Hook> {
            TARGET.store(pack(&target), Ordering::Relaxed);
            let (tx, rx) = mpsc::channel();
            let thread = std::thread::Builder::new()
                .name("ferrocord-appui".into())
                .spawn(move || {
                    // SAFETY: appels Win32 sans pointeur gardé : le module de
                    // l'exécutable (jamais libéré), le rappel `hook_proc`
                    // (fonction statique) ; le crochet est retiré sur ce même
                    // fil avant sa fin.
                    let hook = unsafe {
                        let module = GetModuleHandleW(None).ok();
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), module.map(|m| HINSTANCE(m.0)), 0)
                    };
                    let Ok(hook) = hook else {
                        let _ = tx.send(None);
                        return;
                    };
                    let mut msg = MSG::default();
                    // SAFETY: `msg` est une variable locale valide ; l'appel
                    // crée la file de messages du fil (pour WM_QUIT).
                    let id = unsafe {
                        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
                        GetCurrentThreadId()
                    };
                    let _ = tx.send(Some(id));
                    // SAFETY: boucle de messages exigée par les crochets bas
                    // niveau ; `msg` reste une variable locale valide.
                    unsafe {
                        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {}
                        let _ = UnhookWindowsHookEx(hook);
                    }
                })
                .ok()?;
            match rx.recv() {
                Ok(Some(thread_id)) => Some(Hook { thread_id, thread: Some(thread) }),
                _ => {
                    let _ = thread.join();
                    TARGET.store(0, Ordering::Relaxed);
                    None
                }
            }
        }

    }

    impl Drop for Hook {
        fn drop(&mut self) {
            TARGET.store(0, Ordering::Relaxed);
            // SAFETY: simple envoi d'un message au fil du crochet, identifié
            // par son numéro ; aucun pointeur.
            let posted = unsafe { PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0)) }.is_ok();
            if posted && let Some(t) = self.thread.take() {
                let _ = t.join();
            }
            PRESSED.store(false, Ordering::Relaxed);
        }
    }
}

#[cfg(not(windows))]
mod imp {
    use super::super::ptt_keys::Target;

    pub struct Hook;

    impl Hook {
        pub fn start(_target: Target) -> Option<Hook> {
            None
        }
    }
}

pub use imp::Hook;

/// Le crochet voulu : posé, remplacé ou retiré. Un échec de pose est
/// retenu avec sa touche : pas de nouvel essai à chaque image.
pub(super) fn sync(hook: &mut Option<(Target, Option<Hook>)>, want: Option<Target>) {
    if hook.as_ref().map(|h| h.0) == want {
        return;
    }
    *hook = None;
    if let Some(t) = want {
        *hook = Some((t, Hook::start(t)));
    }
}
