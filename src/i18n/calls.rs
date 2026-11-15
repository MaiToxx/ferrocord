//! Textes des appels privés, des salons de conférence, du statut des salons
//! vocaux et du tableau de sons (lot V6). Module enfant de `table` (il en
//! reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        START_CALL = "Démarrer un appel vocal", "Start Voice Call";
        CALLING = "Appel en cours de lancement…", "Starting call…";
        INCOMING = "Appel entrant", "Incoming Call";
        ACCEPT = "Accepter", "Accept";
        DECLINE = "Refuser", "Decline";
        JOIN = "Rejoindre", "Join";
        LEAVE = "Raccrocher", "Disconnect";
        IN_CALL = "Appel en cours", "Ongoing call";
        RINGING = "sonnerie…", "ringing…";
        STOP_RINGING = "Arrêter la sonnerie", "Stop Ringing";
        RING = "Faire sonner", "Ring";
        UNAVAILABLE = "Appel indisponible (panne chez Discord)", "Call unavailable (Discord outage)";
        MUTED = "micro coupé", "muted";
        NOBODY = "Personne pour l'instant.", "Nobody here yet.";
        VOICE_ROOM = "Salon vocal", "Voice Channel";
        STAGE = "Conférence", "Stage";
        LIVE = "En direct", "Live";
        NO_STAGE = "Aucune conférence en cours.", "No stage running.";
        START_STAGE = "Démarrer la conférence", "Start Stage";
        END_STAGE = "Terminer la conférence", "End Stage";
        EDIT_TOPIC = "Modifier le sujet", "Edit Topic";
        TOPIC_HINT = "De quoi allez-vous parler\u{a0}?", "What will you talk about?";
        NOTIFY_EVERYONE = "Prévenir @everyone", "Notify @everyone";
        SPEAKERS = "Orateurs", "Speakers";
        HANDS = "Mains levées", "Requested to speak";
        LISTENERS = "Auditeurs", "Audience";
        RAISE_HAND = "Lever la main", "Request to Speak";
        LOWER_HAND = "Baisser la main", "Lower Hand";
        INVITE_SPEAK = "Inviter à parler", "Invite to Speak";
        TO_AUDIENCE = "Rendre auditeur", "Move to Audience";
        SPEAK = "Prendre la parole", "Speak";
        BACK_TO_AUDIENCE = "Redevenir auditeur", "Move Me to Audience";
        STATUS = "Statut", "Status";
        SET_STATUS = "Définir le statut", "Set Status";
        STATUS_HINT = "Que se passe-t-il ici\u{a0}?", "What's happening here?";
        CLEAR = "Effacer", "Clear";
        SAVE = "Enregistrer", "Save";
        CANCEL = "Annuler", "Cancel";
        CLOSE = "Fermer", "Close";
        SOUNDBOARD = "Tableau de sons", "Soundboard";
        DEFAULT_SOUNDS = "Sons par défaut", "Default Sounds";
        SERVER_SOUNDS = "Sons du serveur", "Server Sounds";
        LOADING = "Chargement…", "Loading…";
        LOAD_FAILED = "Impossible de charger les sons.", "Couldn't load sounds.";
        NO_SOUNDS = "Aucun son.", "No sounds.";
        SOUNDS_NEED_VOICE = "Rejoignez le salon, micro ouvert, pour jouer un son.", "Join the channel with your mic on to play sounds.";
        TOPIC_INVALID = "Le sujet doit faire de 1 à 120 caractères.", "The topic must be 1 to 120 characters long.";
        STATUS_INVALID = "Le statut fait 500 caractères au plus.", "The status can be at most 500 characters long.";
        CAPTCHA = "Discord demande une vérification\u{a0}: faites-le dans l'application officielle.", "Discord asks for a verification: please do it in the official app.";
        A_RING = "Faire sonner", "Ringing";
        A_STOP_RINGING = "Arrêter la sonnerie", "Stopping the ring";
        A_SOUNDS = "Charger les sons", "Loading sounds";
        A_PLAY = "Jouer le son", "Playing the sound";
        A_START = "Démarrer la conférence", "Starting the stage";
        A_TOPIC = "Changer le sujet", "Changing the topic";
        A_END = "Terminer la conférence", "Ending the stage";
        A_HAND = "Lever ou baisser la main", "Raising or lowering your hand";
        A_SPEAKER = "Changer d'orateur", "Changing speakers";
        A_STATUS = "Changer le statut", "Changing the status";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        calling_you(name) = "{name} vous appelle…", "{name} is calling you…";
        failed(action, reason) = "{action}\u{a0}: échec ({reason})", "{action} failed ({reason})";
        played(name, sound) = "{name} a joué «\u{a0}{sound}\u{a0}»", "{name} played “{sound}”";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        in_room = ["{n} personne", "{n} personnes"], ["{n} person", "{n} people"];
    }
}
