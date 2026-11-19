//! Textes du sélecteur de partage (caméra, écran, fenêtre ; lot M3).
//! Module enfant de `table`, mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        TITLE = "Partager", "Share";
        TAB_WINDOWS = "Applications", "Applications";
        TAB_SCREENS = "Écran entier", "Entire screen";
        TAB_CAMERAS = "Appareils", "Devices";
        LOADING = "Recherche de ce qui peut être partagé…", "Looking for what can be shared…";
        NO_SCREENS = "Aucun écran à partager.", "No screen to share.";
        NO_WINDOWS = "Aucune fenêtre à partager.", "No window to share.";
        NO_CAMERAS = "Aucune caméra branchée.", "No camera plugged in.";
        CAMERA_TILE_TIP = "La caméra ne s'allume qu'une fois choisie.", "The camera only turns on once chosen.";
        PREVIEW_LOADING = "Préparation de l'aperçu…", "Preparing the preview…";
        PREVIEW_FAILED = "Aperçu impossible pour cette source.", "No preview for this source.";
        QUALITY = "Qualité", "Quality";
        Q_720_30 = "720p, 30\u{a0}images/s", "720p, 30 fps";
        Q_1080_30 = "1080p, 30\u{a0}images/s", "1080p, 30 fps";
        Q_1080_60 = "1080p, 60\u{a0}images/s", "1080p, 60 fps";
        Q_SOURCE = "Source (taille d'origine), 30\u{a0}images/s", "Source (original size), 30 fps";
        Q_SOURCE_60 = "Source (taille d'origine), 60\u{a0}images/s", "Source (original size), 60 fps";
        QN_HD = "HD", "HD";
        QN_FHD = "Full HD", "Full HD";
        QN_FHD_60 = "Full HD fluide", "Smooth Full HD";
        QN_SOURCE = "Source", "Source";
        QN_SOURCE_60 = "Source fluide", "Smooth Source";
        RES_SOURCE = "Taille d'origine", "Original size";
        WITH_SOUND = "son", "sound";
        SETTINGS_TIP = "Qualité et son du partage", "Share quality and sound";
        SOUND = "Partager le son", "Share sound";
        SOUND_TIP_SCREEN = "Tout le son de l'ordinateur, sauf celui de Ferrocord (Windows ancien\u{a0}: Ferrocord compris).", "All of the computer's sound except Ferrocord's (older Windows: Ferrocord's included).";
        SOUND_TIP_WINDOW = "Le son de cette application seule (Windows\u{a0}11).", "This app's sound only (Windows 11).";
        BACK = "Retour", "Back";
        CAMERA_ON = "Allumer la caméra", "Turn on camera";
        ERR_CAMERA_PRIVACY = "Windows bloque l'accès à la caméra. Ouvrez Paramètres > Confidentialité et sécurité > Caméra, activez «\u{a0}Accès à la caméra\u{a0}» et «\u{a0}Autoriser les applications de bureau à accéder à votre caméra\u{a0}», puis réessayez.", "Windows is blocking camera access. Open Settings > Privacy & security > Camera, turn on “Camera access” and “Let desktop apps access your camera”, then try again.";
        ERR_GONE = "Cette source n'existe plus (fenêtre fermée, écran ou caméra débranché).", "This source is gone (window closed, screen or camera unplugged).";
        ERR_UNSUPPORTED = "Ce Windows ne permet pas cette capture.", "This version of Windows can't capture this.";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        screen_n(n, name) = "Écran {n}\u{a0}: {name}", "Screen {n}: {name}";
        screen_short(n) = "Écran {n}", "Screen {n}";
        quality_detail(res, fps) = "{res} · {fps}\u{a0}images/s", "{res} · {fps} fps";
        capture_error(reason) = "Erreur de capture\u{a0}: {reason}", "Capture error: {reason}";
    }
}
