//! Textes de l'historique hors ligne (lot H).
//! Module enfant de `table`, mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SECTION = "Historique hors ligne", "Offline history";
        KEEP = "Garder les derniers messages sur cet ordinateur", "Keep recent messages on this computer";
        KEEP_TIP = "Les derniers messages des salons récemment ouverts sont gardés, chiffrés pour votre session Windows, pour s'afficher dès le lancement et sans réseau. Désactiver efface tout.", "Recent messages of recently opened channels are kept, encrypted for your Windows session, so they show right at startup and without a network. Turning this off erases everything.";
        CLEAR = "Effacer maintenant", "Erase now";
        CLEAR_TIP = "Efface l'historique gardé de tous les comptes sur cet ordinateur. Il se reconstitue à l'usage si l'option reste active.", "Erases the kept history of every account on this computer. It builds up again with use while the option stays on.";
        CLEARED = "Historique hors ligne effacé.", "Offline history erased.";
        UNAVAILABLE = "Pas disponible sur ce système\u{a0}: la clé ne peut pas y être protégée.", "Not available on this system: the key cannot be protected here.";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        shown(ms) = "Historique hors ligne affiché ({ms} ms après l'ouverture de la session).", "Offline history shown ({ms} ms after the session opened).";
    }
}
