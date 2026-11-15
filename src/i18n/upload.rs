//! Textes de l'envoi de fichiers (lot T8). Module enfant de `table` (il en
//! reprend les macros), tenu à part pour que les ajouts des lots parallèles
//! ne se mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        ATTACH = "Joindre des fichiers", "Attach files";
        SPOILER = "Spoiler", "Spoiler";
        EDIT = "Modifier", "Edit";
        EDIT_TITLE = "Pièce jointe", "Attachment";
        FILE_NAME = "Nom du fichier", "File name";
        ALT_TEXT = "Texte alternatif", "Alt text";
        ALT_HINT = "Décrivez le fichier pour qui ne peut pas le voir", "Describe the file for people who can't see it";
        REMOVE = "Retirer", "Remove";
        CLOSE = "Fermer", "Close";
        CANCEL = "Annuler", "Cancel";
        RETRY = "Réessayer", "Retry";
        DELETE = "Supprimer", "Delete";
        CANCELLED = "Envoi annulé", "Upload cancelled";
        CANCELLING = "Annulation…", "Cancelling…";
        WAITING = "En attente…", "Waiting…";
        TOO_MANY = "Un message compte au plus 10 fichiers.", "A message can have at most 10 files.";
        CHECKING_LIMIT = "Lecture de votre limite d'envoi auprès de Discord\u{a0}: réessayez dans un instant.", "Checking your upload limit with Discord: try again in a moment.";
        BUSY = "Trop d'envois en cours\u{a0}: attendez la fin de l'un d'eux.", "Too many uploads in progress: wait for one to finish.";
        BLOCKED = "Vous ne pouvez pas envoyer de fichiers ici.", "You can't send files here.";
        MAYBE_SENT = "Peut-être envoyé (pas de réponse de Discord)", "Maybe sent (no answer from Discord)";
        SERVER_TOO_LARGE = "Discord refuse ces fichiers\u{a0}: trop volumineux", "Discord refused these files: too large";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        too_large(name, limit) = "«\u{a0}{name}\u{a0}» dépasse la limite de {limit} par fichier.", "“{name}” is over the {limit} per-file limit.";
        over_limit(limit) = "Plus de {limit}", "Over {limit}";
        unreadable(name) = "Fichier illisible\u{a0}: {name}", "Can't read file: {name}";
        changed(name) = "Fichier modifié depuis l'ajout\u{a0}: {name}", "File changed since it was added: {name}";
        not_sent(reason) = "Non envoyé\u{a0}: {reason}", "Not sent: {reason}";
        progress(sent, total) = "{sent} sur {total}", "{sent} of {total}";
        drop_here(target) = "Déposez les fichiers pour les joindre à {target}", "Drop files to attach them to {target}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        files = ["{n} fichier", "{n} fichiers"], ["{n} file", "{n} files"];
    }
}
