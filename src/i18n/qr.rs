//! Textes de la connexion par QR code (phase 6, lot C2).
//! Module enfant de `table`, mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        WITH_QR = "Se connecter avec un QR code", "Log in with a QR code";
        WITH_TOKEN = "Utiliser un jeton", "Use a token";
        HOW = "Sur votre téléphone, ouvrez Discord, puis Paramètres › Scanner le QR code.", "On your phone, open Discord, then Settings › Scan QR Code.";
        PREPARING = "Préparation du QR code…", "Preparing the QR code…";
        CONFIRM_HINT = "Le QR code a été lu.", "The QR code was scanned.";
        RENEW = "Nouveau QR code", "New QR code";
        EXPIRED = "Le QR code a expiré.", "The QR code has expired.";
        CANCELLED = "Connexion refusée sur le téléphone.", "Login was denied on the phone.";
        CAPTCHA = "Discord demande un captcha, que Ferrocord ne résout pas. Connectez-vous dans l'application officielle de Discord, puis réessayez plus tard.", "Discord is asking for a captcha, which Ferrocord does not solve. Log in with the official Discord app, then try again later.";
        PROTOCOL = "Réponse inattendue de Discord\u{a0}: connexion abandonnée par prudence.", "Unexpected answer from Discord: login dropped to be safe.";
        UNAVAILABLE = "La connexion par QR code n'est pas disponible ici.", "QR code login is not available here.";
        QR_LABEL = "QR code de connexion", "Login QR code";
        SHOW_QR = "Afficher le QR code", "Show QR code";
        HIDE_QR = "Masquer le QR code", "Hide QR code";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        confirm(name) = "Connexion de {name}\u{a0}: confirmez sur votre téléphone.", "Logging in as {name}: confirm on your phone.";
        expires_seconds(n) = "Expire dans {n}\u{a0}s", "Expires in {n} s";
        expires_minutes(n) = "Expire dans {n}\u{a0}min", "Expires in {n} min";
        network(detail) = "Connexion impossible\u{a0}: {detail}", "Could not connect: {detail}";
        closed(code) = "Discord a fermé la connexion (code {code}).", "Discord closed the connection (code {code}).";
        refused(reason) = "Discord a refusé la connexion\u{a0}: {reason}", "Discord refused the login: {reason}";
    }
}
