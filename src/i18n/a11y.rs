//! Textes de l'accessibilité (lot C5) : noms lus par les lecteurs d'écran,
//! sélection des messages au clavier, bandeau du mode streamer. Module
//! enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        ACT_MSG_REPLY = "Message sélectionné\u{a0}: répondre", "Selected message: reply";
        ACT_MSG_EDIT = "Message sélectionné\u{a0}: modifier", "Selected message: edit";
        ACT_MSG_REACT = "Message sélectionné\u{a0}: ajouter une réaction", "Selected message: add reaction";
        ACT_MSG_PIN = "Message sélectionné\u{a0}: épingler ou désépingler", "Selected message: pin or unpin";
        ACT_MSG_COPY = "Message sélectionné\u{a0}: copier le texte", "Selected message: copy text";
        ACT_MSG_DELETE = "Message sélectionné\u{a0}: supprimer", "Selected message: delete";
        MSG_PINNED = "épinglé", "pinned";
        MSG_EDITED = "modifié", "edited";
        MSG_SPOILER = "[spoiler]", "[spoiler]";
        MSG_COPIED = "Texte du message copié.", "Message text copied.";
        STREAMER_BANNER = "Le mode streamer est activé\u{a0}: vos informations personnelles sont masquées.", "Streamer mode is on: your personal information is hidden.";
        STREAMER_DISABLE = "Désactiver", "Disable";
    }
}

/// Modèles à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        message(author, time, text) = "{author}, {time}\u{a0}: {text}", "{author}, {time}: {text}";
        reactions(list) = "Réactions\u{a0}: {list}", "Reactions: {list}";
        reaction(emoji, n) = "{emoji} {n}", "{emoji} {n}";
        attachments(list) = "Pièces jointes\u{a0}: {list}", "Attachments: {list}";
    }
}
