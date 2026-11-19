//! Textes de la vue d'appel, de la vidéo et des directs (phase 5, lot M4).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SHOW_CHAT = "Discussion", "Chat";
        SHOW_CHAT_TIP = "Afficher la discussion du salon", "Show the channel chat";
        SHOW_CALL = "Revenir à l'appel", "Back to the call";
        SHRINK = "Réduire l'appel", "Minimize call";
        SHRINK_TIP = "Revenir à la conversation, l'appel en bandeau au-dessus", "Back to the conversation, with the call in a strip above";
        EXPAND = "Agrandir l'appel", "Expand call";
        CAMERA_ON = "Allumer la caméra", "Turn on camera";
        CAMERA_OFF = "Éteindre la caméra", "Turn off camera";
        SHARE = "Partager l'écran", "Share your screen";
        STOP_SHARE = "Arrêter le partage", "Stop sharing";
        CHANGE_SOURCE = "Changer de source", "Change source";
        STOP = "Arrêter", "Stop";
        LEAVE = "Quitter l'appel", "Leave call";
        JOIN = "Rejoindre le vocal", "Join voice";
        LIVE = "EN DIRECT", "LIVE";
        WATCH = "Regarder", "Watch";
        STOP_WATCHING = "Arrêter de regarder", "Stop watching";
        POP_OUT = "Détacher dans une fenêtre", "Pop out";
        FULLSCREEN = "Plein écran (F11)", "Full screen (F11)";
        EXIT_FULLSCREEN = "Quitter le plein écran (Échap)", "Exit full screen (Esc)";
        ON_TOP = "Toujours au-dessus", "Always on top";
        CONNECTING = "Connexion au direct…", "Connecting to the stream…";
        WAITING = "En attente de l'image…", "Waiting for video…";
        PAUSED = "Direct en pause", "Stream paused";
        CAMERA_ACTIVE = "Caméra allumée", "Camera on";
        EMPTY = "Personne dans ce salon pour l'instant.", "Nobody is here yet.";
        BACK_TO_GRID = "Revenir à la grille", "Back to grid";
        END_FULL = "Ce direct est plein.", "This stream is full.";
        END_UNAUTHORIZED = "Vous n'avez pas accès à ce direct.", "You don't have access to this stream.";
        END_RATE = "Discord limite les directs sur ce serveur pour l'instant.", "Discord is limiting streams in this server for now.";
        END_UNAVAILABLE = "Le direct est indisponible (panne de Discord).", "The stream is unavailable (Discord outage).";
        NO_CAMERA = "Aucune caméra n'est branchée.", "No camera is plugged in.";
        NO_ENCODER = "Cet ordinateur ne sait pas encoder la vidéo en H.264.", "This computer can't encode H.264 video.";
        NO_DECODER = "Cet ordinateur ne sait pas décoder la vidéo reçue (H.264).", "This computer can't decode the received video (H.264).";
        SOURCE_ENDED = "La source partagée a disparu (fenêtre fermée, écran ou caméra débranché).", "The shared source is gone (window closed, screen or camera unplugged).";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        sharing(what) = "Vous partagez\u{a0}: {what}", "You're sharing: {what}";
        camera_failed(why) = "Caméra éteinte\u{a0}: {why}", "Camera turned off: {why}";
        share_failed(why) = "Partage arrêté\u{a0}: {why}", "Sharing stopped: {why}";
        video_error(why) = "Erreur vidéo\u{a0}: {why}", "Video error: {why}";
        lowered(width, height) = "La carte graphique refuse cette définition\u{a0}: vidéo envoyée en {width}×{height}.", "The graphics card refuses this resolution: sending {width}×{height} instead.";
        share_sound_failed(why) = "Le son du partage ne passe pas\u{a0}: {why}", "Shared audio isn't working: {why}";
        screen_of(name) = "Écran de {name}", "{name}'s screen";
        ended(reason) = "Le direct s'est arrêté ({reason}).", "The stream ended ({reason}).";
        stream_failed(why) = "Échec du direct\u{a0}: {why}", "Stream failed: {why}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        viewers = ["{n} spectateur", "{n} spectateurs"], ["{n} viewer", "{n} viewers"];
        people = ["{n} personne", "{n} personnes"], ["{n} person", "{n} people"];
    }
}
