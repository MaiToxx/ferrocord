//! Textes du lot « restes » (groupe 3) : bandeau d'une demande de message,
//! panneau de profil d'un MP, invitation envoyée dans le MP, galerie des
//! forums, lecture à voix haute, emoji du statut. Module enfant de `table`
//! (il en reprend les macros), tenu à part pour que les ajouts des lots
//! parallèles ne se mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        FMT_BOLD = "Gras", "Bold";
        FMT_ITALIC = "Italique", "Italic";
        FMT_STRIKE = "Barré", "Strikethrough";
        FMT_CODE = "Code", "Code";
        FMT_SPOILER = "Spoiler", "Spoiler";
        STICKER_HINTS_CLOSE = "Fermer les suggestions d'autocollants", "Close sticker suggestions";
        STICKER_HINT_SEND = "Envoyer l'autocollant {}", "Send the {} sticker";
        ACCEPT_REQUEST = "Accepter", "Accept";
        ACT_CREATE_GROUP = "Créer un groupe privé", "Create a group DM";
        PROFILE_HIDE = "Masquer le profil de l'utilisateur", "Hide User Profile";
        PROFILE_SHOW = "Afficher le profil de l'utilisateur", "Show User Profile";
        OWNER = "Propriétaire du serveur", "Server Owner";
        VIEW_LIST = "Liste", "List";
        VIEW_GALLERY = "Galerie", "Gallery";
        SPEAK = "Lire le message à voix haute", "Speak Message";
        STOP_SPEAKING = "Arrêter la lecture", "Stop Speaking";
        BANNER_REMOVE_ONLY = "Sans Nitro, la bannière peut seulement être retirée.", "Without Nitro, the banner can only be removed.";
        STATUS_EMOJI = "Choisir l'emoji du statut", "Pick the status emoji";
        STATUS_EMOJI_CLEAR = "Retirer l'emoji", "Remove the emoji";
        INVITE_NO_CHANNEL = "Aucun salon de ce serveur ne vous permet de créer une invitation.", "No channel of this server lets you create an invite.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        request_banner(name) = "{name} souhaite vous envoyer un message. Acceptez la demande pour lui répondre.", "{name} wants to send you a message. Accept the request to reply.";
        invite_sent(guild, name) = "Invitation à {guild} envoyée à {name}.", "Invite to {guild} sent to {name}.";
    }
}
