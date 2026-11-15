//! Réveils de l'interface décidés par le réseau, notés seulement quand le
//! journal de performance est actif (`FERROCORD_PERF=1`).
//!
//! La passerelle note, pour chaque événement envoyé à l'interface, s'il l'a
//! réveillée, s'il attend l'image suivante ou s'il est abandonné, avec le
//! type du paquet (`MESSAGE_CREATE`…). Rien d'autre : ni identifiant, ni
//! texte. Journal inactif : une lecture atomique par événement, rien de plus.

use std::{
    cell::RefCell,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

static ON: AtomicBool = AtomicBool::new(false);
static NOTES: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// Au-delà, les notes les plus récentes sont perdues (interface jamais réveillée).
const MAX_NOTES: usize = 4096;

thread_local! {
    /// Type du paquet de passerelle en cours de décodage sur ce fil.
    static SOURCE: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Ce que la passerelle a fait d'un événement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wake {
    /// L'interface est réveillée tout de suite.
    Now,
    /// L'événement attend la prochaine image (rien ne change à l'écran).
    Deferred,
    /// L'événement est abandonné (sans effet visible, et périmé plus tard).
    Dropped,
}

impl Wake {
    fn key(self) -> &'static str {
        match self {
            Wake::Now => "reveil",
            Wake::Deferred => "differe",
            Wake::Dropped => "ignore",
        }
    }
}

pub fn enable() {
    ON.store(true, Ordering::Relaxed);
}

pub fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// Nom sûr pour le journal : minuscules, chiffres, `.` et `_` seulement, 48 caractères au plus.
pub fn clean(raw: &str) -> String {
    raw.chars()
        .take(48)
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '.' || c == '_' { c } else { '_' }
        })
        .collect()
}

/// Paquet de passerelle décodé sur ce fil, jusqu'à la fin du décodage
/// (fin de vie du gardien).
pub struct Source(bool);

impl Source {
    pub fn new(kind: &str) -> Source {
        if !on() {
            return Source(false);
        }
        SOURCE.with(|s| *s.borrow_mut() = Some(clean(kind)));
        Source(true)
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        if self.0 {
            SOURCE.with(|s| *s.borrow_mut() = None);
        }
    }
}

/// Note ce que la passerelle a fait d'un événement (source : le paquet en
/// cours sur ce fil, ou `rest` hors de la passerelle).
pub fn note(what: Wake) {
    if !on() {
        return;
    }
    let source = SOURCE.with(|s| s.borrow().clone()).unwrap_or_else(|| "rest".into());
    if let Ok(mut v) = NOTES.lock()
        && v.len() < MAX_NOTES
    {
        v.push(format!("passerelle.{}.{source}", what.key()));
    }
}

/// Notes depuis le dernier appel.
pub fn take() -> Vec<String> {
    NOTES.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
}

/// Clé d'une cause de redessin d'egui : les deux derniers éléments du chemin
/// du fichier (jamais le dossier de l'utilisateur) et la ligne.
pub fn cause_key(file: &str, line: u32) -> String {
    let mut parts = file.rsplit(['/', '\\']);
    let name = parts.next().unwrap_or_default();
    let dir = parts.next().unwrap_or_default();
    format!("cause.{}_{}_{line}", clean(dir), clean(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_hold_no_path_nor_odd_characters() {
        assert_eq!(cause_key(r"C:\Users\Quelqu'un\.cargo\registry\src\x\egui-0.37.0\src\containers\area.rs", 708), "cause.containers_area.rs_708");
        assert_eq!(cause_key("src/ui/mod.rs", 387), "cause.ui_mod.rs_387");
        assert_eq!(cause_key("lib.rs", 1), "cause._lib.rs_1");
        assert_eq!(clean("MESSAGE_CREATE"), "message_create");
        assert_eq!(clean("é/\\ x"), "____x");
        assert_eq!(clean(&"A".repeat(100)).len(), 48);
    }
}
