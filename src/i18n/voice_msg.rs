//! Textes des messages vocaux et des sons courts (phase 6). Module enfant
//! de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        PLAY = "Écouter", "Play";
        PAUSE = "Pause", "Pause";
        VOICE_MESSAGE = "Message vocal", "Voice message";
        RECORD = "Enregistrer un message vocal", "Record a voice message";
        RECORDING = "Enregistrement…", "Recording…";
        SEND = "Envoyer", "Send";
        CANCEL = "Annuler", "Cancel";
        NOTHING_RECORDED = "Rien n'a été enregistré.", "Nothing was recorded.";
        BAD_ADDRESS = "adresse refusée", "address refused";
        UNREADABLE = "format illisible", "unreadable format";
        TOO_BIG = "fichier trop gros", "file too large";
        NO_OUTPUT = "aucune sortie audio", "no audio output";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        play_failed(why) = "Lecture impossible\u{a0}: {why}", "Can't play: {why}";
        record_failed(why) = "Enregistrement impossible\u{a0}: {why}", "Can't record: {why}";
        send_failed(why) = "Envoi du message vocal impossible\u{a0}: {why}", "Can't send the voice message: {why}";
    }
}
