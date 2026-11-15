//! Textes des paramètres du serveur : emojis, autocollants et sons (lot A6).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la
//! fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        EXPRESSIONS = "Expressions", "Expressions";
        PAGE_EMOJIS = "Emojis", "Emoji";
        PAGE_STICKERS = "Autocollants", "Stickers";
        PAGE_SOUNDS = "Sons", "Soundboard";
        UPLOAD_EMOJI = "Envoyer un emoji", "Upload Emoji";
        UPLOAD_STICKER = "Envoyer un autocollant", "Upload Sticker";
        UPLOAD_SOUND = "Envoyer un son", "Upload Sound";
        EMOJI_LIMITS = "PNG, JPEG, WebP ou GIF. L'image est ramenée à 128 × 128 (256 Kio au plus). Nom\u{a0}: 2 à 32 lettres, chiffres ou _.", "PNG, JPEG, WebP or GIF. The image is resized to 128 × 128 (256 KB max). Name: 2 to 32 letters, digits or _.";
        STICKER_LIMITS = "PNG, APNG ou GIF de 320 × 320, 500 Kio au plus. Une autre image fixe est convertie en PNG.", "PNG, APNG or GIF at 320 × 320, 500 KB max. Other still images are converted to PNG.";
        SOUND_LIMITS = "MP3 ou Ogg, 512 Kio et 5,2 secondes au plus.", "MP3 or Ogg, 512 KB and 5.2 seconds max.";
        STATIC_EMOJIS = "Emojis", "Emoji";
        ANIMATED_EMOJIS = "Emojis animés", "Animated Emoji";
        COL_IMAGE = "Image", "Image";
        COL_NAME = "Nom", "Name";
        COL_AUTHOR = "Envoyé par", "Uploaded by";
        NAME = "Nom", "Name";
        DESCRIPTION = "Description", "Description";
        TAGS = "Emoji associé", "Related emoji";
        TAGS_HINT = "nom d'un emoji, par ex. wave", "an emoji name, e.g. wave";
        VOLUME = "Volume", "Volume";
        SOUND_EMOJI = "Emoji (facultatif)", "Emoji (optional)";
        SOUND_EMOJI_HINT = "un emoji, ou le nom d'un emoji du serveur", "an emoji, or the name of a server emoji";
        ROLES = "Rôles autorisés", "Allowed roles";
        ALL_MEMBERS = "Tous les membres", "Everyone";
        EDIT = "Modifier", "Edit";
        SAVE = "Enregistrer", "Save";
        SEND = "Envoyer", "Upload";
        CANCEL = "Annuler", "Cancel";
        DELETE = "Supprimer", "Delete";
        CONFIRM_DELETE = "Supprimer définitivement", "Delete permanently";
        REFRESH = "Actualiser", "Refresh";
        CLOSE = "Fermer", "Close";
        LOADING = "Chargement…", "Loading…";
        READING_FILE = "Lecture du fichier…", "Reading file…";
        NO_EMOJIS = "Aucun emoji pour l'instant.", "No emoji yet.";
        NO_STICKERS = "Aucun autocollant pour l'instant.", "No stickers yet.";
        NO_SOUNDS = "Aucun son pour l'instant.", "No sounds yet.";
        UNAVAILABLE = "indisponible (boosts perdus)", "unavailable (boosts lost)";
        NO_DIALOG = "La boîte de dialogue de fichiers n'existe pas sur ce système.", "The file dialog isn't available on this system.";
        UNREADABLE = "Ce fichier est illisible ou d'un format non pris en charge.", "This file can't be read or its format isn't supported.";
        TOO_BIG = "Ce fichier est trop volumineux.", "This file is too large.";
        BAD_EMOJI_NAME = "Nom invalide\u{a0}: 2 à 32 lettres, chiffres ou _.", "Invalid name: 2 to 32 letters, digits or _.";
        BAD_STICKER_NAME = "Nom invalide\u{a0}: 2 à 30 caractères.", "Invalid name: 2 to 30 characters.";
        BAD_SOUND_NAME = "Nom invalide\u{a0}: 2 à 32 caractères.", "Invalid name: 2 to 32 characters.";
        BAD_DESCRIPTION = "Description invalide\u{a0}: vide, ou 2 à 100 caractères.", "Invalid description: empty, or 2 to 100 characters.";
        BAD_TAGS = "Indiquez l'emoji associé.", "Enter the related emoji.";
        BAD_SOUND_EMOJI = "Emoji inconnu\u{a0}: un emoji Unicode, ou le nom d'un emoji du serveur.", "Unknown emoji: a Unicode emoji, or the name of a server emoji.";
        NO_SLOT = "Plus aucun emplacement libre.", "No free slot left.";
        BUSY = "Une action est déjà en cours.", "An action is already in progress.";
        FORBIDDEN = "Vous n'avez pas la permission de gérer les expressions de ce serveur.", "You don't have permission to manage this server's expressions.";
        CAPTCHA = "Discord demande une vérification (captcha)\u{a0}: faites-le dans l'application officielle.", "Discord asks for a verification (captcha): do it in the official app.";
        NOT_FOUND = "cet élément n'existe plus", "this item no longer exists";
        ACTION_LOAD = "Chargement de la liste", "Loading the list";
        ACTION_CREATE = "Envoi", "Upload";
        ACTION_EDIT = "Modification", "Edit";
        ACTION_DELETE = "Suppression", "Deletion";
        DONE_CREATE = "Envoyé.", "Uploaded.";
        DONE_EDIT = "Modifications enregistrées.", "Changes saved.";
        DONE_DELETE = "Supprimé.", "Deleted.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        slots(used, total) = "{used} sur {total} emplacements utilisés", "{used} of {total} slots used";
        tier(n) = "Niveau de boost {n}", "Boost level {n}";
        too_long(s) = "Ce son dure {s}\u{a0}s\u{a0}; 5,2\u{a0}s au plus.", "This sound lasts {s} s; 5.2 s max.";
        duration(s) = "Durée\u{a0}: {s}\u{a0}s", "Duration: {s} s";
        percent(n) = "{n}\u{a0}%", "{n}%";
        delete_question(name) = "Supprimer «\u{a0}{name}\u{a0}»\u{a0}? Ce n'est pas réversible.", "Delete \"{name}\"? This can't be undone.";
    }
}
