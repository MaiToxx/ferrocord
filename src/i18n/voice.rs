//! Textes du vocal des serveurs (phase 4, lot V4). Module enfant de `table`
//! (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        ACT_MUTE = "Couper ou réactiver le micro", "Toggle mute";
        ACT_DEAFEN = "Mettre ou retirer la sourdine", "Toggle deafen";
        ACT_PTT = "Appui pour parler (touche tenue)", "Push to talk (hold)";
        MUTE = "Couper le micro", "Mute";
        UNMUTE = "Réactiver le micro", "Unmute";
        DEAFEN = "Mettre en sourdine", "Deafen";
        UNDEAFEN = "Retirer la sourdine", "Undeafen";
        JOINING = "Connexion au vocal…", "Connecting to voice…";
        CONNECTED = "Connecté au vocal", "Voice connected";
        LEAVE = "Se déconnecter du vocal", "Disconnect from voice";
        NO_CONNECT = "Vous n'avez pas la permission de rejoindre ce salon vocal.", "You don't have permission to join this voice channel.";
        FULL = "Ce salon vocal est plein.", "This voice channel is full.";
        GONE = "Ce salon vocal n'existe plus.", "This voice channel no longer exists.";
        CANT_SPEAK = "Vous ne pouvez pas parler dans ce salon.", "You can't speak in this channel.";
        SERVER_MUTED = "Rendu muet par le serveur", "Server muted";
        SERVER_DEAFENED = "Mis en sourdine par le serveur", "Server deafened";
        SELF_MUTED = "Micro coupé", "Muted";
        SELF_DEAFENED = "Sourdine", "Deafened";
        VIDEO = "Caméra allumée", "Camera on";
        LIVE = "EN DIRECT", "LIVE";
        LIVE_TIP = "Partage son écran", "Sharing their screen";
        HAND = "Demande la parole", "Requested to speak";
        VOICE_TIP = "Salon vocal", "Voice channel";
        STAGE_TIP = "Salon de conférence", "Stage channel";
        LOCKED_TIP = "Vous ne pouvez pas rejoindre ce salon", "You can't join this channel";
        FULL_TIP = "Salon plein", "Channel full";
        USER_VOLUME = "Volume de l'utilisateur", "User volume";
        LOCAL_MUTE = "Rendre muet (pour moi)", "Mute (for me)";
        SERVER_MUTE = "Rendre muet sur le serveur", "Server mute";
        SERVER_UNMUTE = "Rétablir la voix sur le serveur", "Server unmute";
        SERVER_DEAFEN = "Mettre en sourdine sur le serveur", "Server deafen";
        SERVER_UNDEAFEN = "Retirer la sourdine du serveur", "Server undeafen";
        MOVE_TO = "Déplacer vers", "Move to";
        DISCONNECT = "Déconnecter du vocal", "Disconnect from voice";
        REASON = "Raison (journal d'audit, facultative)", "Reason (audit log, optional)";
        CONFIRM = "Confirmer", "Confirm";
        CANCEL = "Annuler", "Cancel";
        DONE = "C'est fait.", "Done.";
        UNKNOWN_USER = "Utilisateur inconnu", "Unknown user";
        // Connexion vocale (lot V7).
        NO_ANSWER = "Discord n'a pas répondu", "Discord did not answer";
        UNSUPPORTED = "aucun chiffrement commun avec le serveur vocal", "no encryption mode in common with the voice server";
        BAD_ENDPOINT = "adresse du serveur vocal refusée", "voice server address refused";
        UNREACHABLE = "serveur vocal injoignable", "voice server unreachable";
        AUDIO_FAILED = "Impossible d'ouvrir les périphériques audio.", "Couldn't open the audio devices.";
        CODES = "Codes de vérification", "Verification codes";
        CODES_TIP = "Comparez ces codes avec ceux des autres participants pour vérifier le chiffrement de bout en bout.", "Compare these codes with the other participants' to verify end-to-end encryption.";
        CALL_CODE = "Code de l'appel", "Call code";
        NO_E2EE = "Cet appel n'est pas chiffré de bout en bout.", "This call isn't end-to-end encrypted.";
        CODES_LOADING = "Calcul des codes…", "Computing codes…";
        CODE_UNKNOWN = "indisponible", "unavailable";
        CLOSE = "Fermer", "Close";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        ping(ms) = "{ms}\u{a0}ms", "{ms} ms";
        place(channel, guild) = "{channel} / {guild}", "{channel} / {guild}";
        limit(count, max) = "{count}/{max}", "{count}/{max}";
        failed(why) = "Échec de la connexion vocale\u{a0}: {why}", "Voice connection failed: {why}";
        refused(code) = "refusée par le serveur vocal (code {code})", "refused by the voice server (code {code})";
        volume(pct) = "{pct}\u{a0}%", "{pct}%";
        mod_failed(why) = "Action refusée\u{a0}: {why}", "Action refused: {why}";
        confirm_mute(name) = "Rendre {name} muet sur tout le serveur\u{a0}?", "Server mute {name}?";
        confirm_unmute(name) = "Rétablir la voix de {name} sur le serveur\u{a0}?", "Server unmute {name}?";
        confirm_deafen(name) = "Mettre {name} en sourdine sur tout le serveur\u{a0}?", "Server deafen {name}?";
        confirm_undeafen(name) = "Retirer la sourdine de {name} sur le serveur\u{a0}?", "Server undeafen {name}?";
        confirm_move(name, channel) = "Déplacer {name} vers {channel}\u{a0}?", "Move {name} to {channel}?";
        confirm_disconnect(name) = "Déconnecter {name} du vocal\u{a0}?", "Disconnect {name} from voice?";
    }
}
