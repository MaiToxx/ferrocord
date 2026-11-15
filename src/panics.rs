//! Paniques des analyseurs de données venues de l'extérieur.
//!
//! Le profil release est en `panic = "unwind"` pour une seule raison : qu'une
//! panique d'un décodeur d'image tiers (ou de l'analyseur du thème) sur un
//! fichier hostile devienne un simple échec, via [`contain`]. Partout
//! ailleurs, [`install`] garde le comportement de `panic = "abort"` : le
//! message habituel, puis arrêt immédiat du processus, sans déroulement. Aucun
//! verrou ne peut donc être empoisonné par une panique hors de [`contain`],
//! et le code passé à [`contain`] ne prend aucun verrou.

use std::{cell::Cell, panic::AssertUnwindSafe};

thread_local! {
    /// Vrai pendant un appel à [`contain`] sur ce fil.
    static CONTAINED: Cell<bool> = const { Cell::new(false) };
}

/// Exécute `f` ; si `f` panique, renvoie `None` au lieu d'arrêter
/// l'application. Réservé aux fonctions pures sur des données reçues
/// (décodage, analyse) : leur résultat partiel est jeté.
pub fn contain<T>(f: impl FnOnce() -> T) -> Option<T> {
    let outer = CONTAINED.replace(true);
    // `f` ne laisse rien derrière elle : tout ce qu'elle a touché est jeté avec sa panique.
    let r = std::panic::catch_unwind(AssertUnwindSafe(f));
    CONTAINED.set(outer);
    r.ok()
}

/// Une panique est-elle en cours d'interception sur ce fil ?
pub fn is_contained() -> bool {
    CONTAINED.get()
}

/// À appeler une fois au démarrage : une panique hors de [`contain`] affiche
/// son message puis arrête le processus (comme `panic = "abort"`) ; une
/// panique interceptée est seulement signalée sur la sortie d'erreur.
pub fn install() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if is_contained() {
            let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
            eprintln!("ferrocord : panique d'un analyseur interceptée ({at}) ; donnée ignorée");
            return;
        }
        // Rapport de plantage local (lot E5) : écrit avant l'arrêt, si le plugin est activé.
        crate::crash::on_panic(info);
        default(info);
        std::process::abort();
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_become_none_and_the_flag_is_restored() {
        assert_eq!(contain(|| 3), Some(3));
        assert!(!is_contained());
        let r: Option<()> = contain(|| panic!("analyseur de test"));
        assert_eq!(r, None);
        assert!(!is_contained(), "drapeau rétabli après la panique");
        // Imbriqué : l'appel intérieur ne relâche pas l'extérieur.
        let r = contain(|| {
            let inner: Option<()> = contain(|| panic!("intérieur"));
            (inner, is_contained())
        });
        assert_eq!(r, Some((None, true)));
        assert!(!is_contained());
    }
}
