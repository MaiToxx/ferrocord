//! Partie pure de l'appui-pour-parler global : touche choisie (nom de
//! touche d'egui) vers code de touche virtuelle de Windows, et tri des
//! événements du crochet clavier. Seule la touche choisie est regardée :
//! toute autre touche est ignorée sans rien en retenir.
// Hors Windows, pas de crochet clavier : seuls les tests s'en servent.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::state::voice_msg::PttKey;

/// Messages du crochet (WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP).
pub const WM_KEYDOWN: u32 = 0x0100;
pub const WM_KEYUP: u32 = 0x0101;
pub const WM_SYSKEYDOWN: u32 = 0x0104;
pub const WM_SYSKEYUP: u32 = 0x0105;

/// Touche surveillée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub vk: u32,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Modificateurs tenus au moment où la touche est enfoncée.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
}

/// Code de touche virtuelle d'un nom de touche d'egui (`Key::name`). Les
/// signes de ponctuation suivent la disposition américaine (VK_OEM_*).
pub fn vk_of(name: &str) -> Option<u32> {
    let b = name.as_bytes();
    if let [c] = b
        && (c.is_ascii_uppercase() || c.is_ascii_digit())
    {
        return Some(u32::from(*c));
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x70 + n - 1);
    }
    Some(match name {
        "Space" => 0x20,
        "Enter" => 0x0D,
        "Tab" => 0x09,
        "Backspace" => 0x08,
        "Escape" => 0x1B,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        "Home" => 0x24,
        "End" => 0x23,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "Left" => 0x25,
        "Up" => 0x26,
        "Right" => 0x27,
        "Down" => 0x28,
        "Semicolon" => 0xBA,
        "Plus" | "Equals" => 0xBB,
        "Comma" => 0xBC,
        "Minus" => 0xBD,
        "Period" => 0xBE,
        "Slash" => 0xBF,
        "Backtick" => 0xC0,
        "OpenBracket" => 0xDB,
        "Backslash" => 0xDC,
        "CloseBracket" => 0xDD,
        "Quote" => 0xDE,
        _ => return None,
    })
}

pub fn target(k: &PttKey<'_>) -> Option<Target> {
    Some(Target { vk: vk_of(k.key)?, ctrl: k.ctrl, alt: k.alt, shift: k.shift })
}

/// Ce que devient l'appui pour un événement du crochet : `Some(true)` la
/// touche choisie est enfoncée avec ses modificateurs, `Some(false)` elle
/// est relâchée, `None` rien ne change (autre touche, autre message). Les
/// modificateurs ne sont lus (`mods`) que pour la touche choisie.
pub fn filter(t: &Target, msg: u32, vk: u32, mods: impl FnOnce() -> Mods) -> Option<bool> {
    if vk != t.vk {
        return None;
    }
    match msg {
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let m = mods();
            Some(m.ctrl == t.ctrl && m.alt == t.alt && m.shift == t.shift)
        }
        WM_KEYUP | WM_SYSKEYUP => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(k: &str) -> PttKey<'_> {
        PttKey { key: k, ctrl: false, alt: false, shift: false }
    }

    #[test]
    fn names_to_virtual_keys() {
        assert_eq!(vk_of("V"), Some(0x56));
        assert_eq!(vk_of("0"), Some(0x30));
        assert_eq!(vk_of("F1"), Some(0x70));
        assert_eq!(vk_of("F24"), Some(0x87));
        assert_eq!(vk_of("F25"), None);
        assert_eq!(vk_of("Backtick"), Some(0xC0));
        assert_eq!(vk_of("Down"), Some(0x28));
        assert_eq!(vk_of("v"), None);
        assert_eq!(vk_of("Copy"), None);
        assert_eq!(target(&key("Space")), Some(Target { vk: 0x20, ctrl: false, alt: false, shift: false }));
    }

    #[test]
    fn only_the_chosen_key_is_looked_at() {
        let t = Target { vk: 0x56, ctrl: true, alt: false, shift: false };
        let ctrl = || Mods { ctrl: true, ..Mods::default() };
        let never = || -> Mods { panic!("modificateurs lus pour une autre touche") };
        // Une autre touche : rien, et les modificateurs ne sont pas lus.
        assert_eq!(filter(&t, WM_KEYDOWN, 0x41, never), None);
        assert_eq!(filter(&t, WM_KEYUP, 0x41, never), None);
        assert_eq!(filter(&t, WM_KEYDOWN, 0x56, ctrl), Some(true));
        assert_eq!(filter(&t, WM_SYSKEYDOWN, 0x56, ctrl), Some(true));
        // Mauvais modificateurs : pas d'appui.
        assert_eq!(filter(&t, WM_KEYDOWN, 0x56, Mods::default), Some(false));
        assert_eq!(filter(&t, WM_KEYDOWN, 0x56, || Mods { ctrl: true, shift: true, alt: false }), Some(false));
        // Relâchée : toujours la fin de l'appui.
        assert_eq!(filter(&t, WM_KEYUP, 0x56, never), Some(false));
        assert_eq!(filter(&t, WM_SYSKEYUP, 0x56, never), Some(false));
        assert_eq!(filter(&t, 0x0200, 0x56, never), None);
    }
}
