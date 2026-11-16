//! Textes du lot M (phase 10) : menu et barre de survol d'un message, début
//! de salon, bandeau des nouveaux messages, lecteur vidéo et audio. Module
//! enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        MARK_UNREAD = "Marquer comme non lu", "Mark Unread";
        MORE = "Plus", "More";
        SUPPRESS_EMBEDS = "Supprimer les intégrations", "Remove All Embeds";
        REPORT = "Signaler dans Discord (navigateur)", "Report in Discord (browser)";
        COPY_MEDIA_LINK = "Copier le lien du média", "Copy Media Link";
        OPEN_MEDIA = "Ouvrir le média dans le navigateur", "Open Media in Browser";
        SPEAK = "Lire le message à voix haute", "Speak Message";
        NEW_BADGE = "NOUVEAU", "NEW";
        MARK_AS_READ = "Marquer comme lu", "Mark As Read";
        NO_UNREAD_STATE = "Ce compte n'a pas d'états de lecture.", "This account has no read states.";
        // Lecteur vidéo et audio.
        PLAY = "Lecture", "Play";
        PAUSE = "Pause", "Pause";
        MUTE = "Couper le son", "Mute";
        UNMUTE = "Rétablir le son", "Unmute";
        VOLUME = "Volume", "Volume";
        SEEK = "Position", "Position";
        FULLSCREEN = "Plein écran", "Full Screen";
        EXIT_FULLSCREEN = "Quitter le plein écran (Échap)", "Exit Full Screen (Esc)";
        OPEN_IN_BROWSER = "Ouvrir dans le navigateur", "Open in Browser";
        PLAYER_LOADING = "Chargement…", "Loading…";
        PLAYER_UNSUPPORTED = "Lecture impossible ici\u{a0}: format non pris en charge.", "Can't play here: unsupported format.";
        PLAYER_TOO_BIG = "Fichier trop gros pour le lecteur intégré.", "File too large for the built-in player.";
        CLOSE_PLAYER = "Fermer le lecteur", "Close Player";
        PLAYER_NO_DECODER = "Lecture impossible ici\u{a0}: aucun décodeur.", "Can't play here: no decoder.";
        // Images : copier, enregistrer (lot VID).
        COPY_IMAGE = "Copier l'image", "Copy Image";
        SAVE_IMAGE = "Enregistrer l'image", "Save Image";
        OPEN_LINK = "Ouvrir le lien", "Open Link";
        IMAGE_COPIED = "Image copiée dans le presse-papiers.", "Image copied to the clipboard.";
        IMAGE_SAVED = "Image enregistrée.", "Image saved.";
        // Liens externes (lot VID).
        WATCH_IN_BROWSER = "Regarder dans le navigateur", "Watch in Browser";
        LEAVING_TITLE = "Vous quittez Discord", "Leaving Discord";
        LEAVING_BODY = "Ce lien vous emmène vers le site suivant\u{a0}:", "This link is taking you to the following website:";
        LEAVING_VISIT = "Visiter le site", "Visit Site";
        LEAVING_BACK = "Retour", "Go Back";
        // MP du compte système de Discord et ses notifications (lot « messages »).
        OFFICIAL_TAG = "OFFICIEL", "OFFICIAL";
        OFFICIAL_ONLY = "Ce salon est réservé aux notifications officielles de Discord.", "This thread is reserved for official Discord notifications.";
        OFFICIAL_NEVER = "Discord ne te demandera jamais ton mot de passe ou ton token de compte.", "Discord will never ask you for your password or account token.";
        OFFICIAL_DM = "Message officiel de Discord", "Official Discord Message";
        CREATED_ON = "Créé le", "Created on";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        welcome_channel(name) = "Bienvenue dans #{name}\u{a0}!", "Welcome to #{name}!";
        channel_start(name) = "C'est le début du salon #{name}.", "This is the start of the #{name} channel.";
        dm_start(name) = "C'est le début de l'historique de vos messages privés avec {name}.", "This is the beginning of your direct message history with {name}.";
        new_since(count, time) = "{count} depuis {time}", "{count} since {time}";
        player_failed(why) = "Lecture impossible\u{a0}: {why}", "Can't play: {why}";
        image_failed(why) = "Image non récupérée\u{a0}: {why}", "Couldn't get the image: {why}";
        trust_domain(domain) = "Faire confiance aux liens vers {domain} désormais", "Trust {domain} links from now on";
    }
}
