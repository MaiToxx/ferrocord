//! Textes de l'activité de jeu et de la présence enrichie (lot C3).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SEC_DETECTED = "Jeux détectés", "Detected games";
        NONE_RUNNING = "Aucun jeu détecté en ce moment.", "No game detected right now.";
        DETECTION_OFF = "Détection coupée\u{a0}: activez «\u{a0}Afficher l'activité en cours\u{a0}» pour montrer vos jeux.", "Detection is off: turn on “Share your activity” to show your games.";
        LIST_LOADING = "Chargement de la liste des jeux de Discord…", "Loading Discord's game list…";
        LIST_FAILED = "Liste des jeux de Discord indisponible\u{a0}: seuls les programmes ajoutés sont reconnus.", "Discord's game list is unavailable: only added programs are recognized.";
        SCAN_TIP = "Les programmes en cours sont relevés toutes les 20\u{a0}secondes, sur cet ordinateur seulement\u{a0}; seul le nom du jeu part chez Discord.", "Running programs are checked every 20 seconds, on this computer only; only the game's name is sent to Discord.";
        UNSUPPORTED = "Détection des jeux indisponible sur ce système\u{a0}: aucun programme n'est relevé.", "Game detection is unavailable on this system: no program is checked.";
        INVISIBLE = "Statut invisible\u{a0}: personne ne voit votre activité.", "Invisible status: nobody sees your activity.";
        SEC_ADDED = "Programmes ajoutés", "Added programs";
        NONE_ADDED = "Aucun programme ajouté.", "No program added.";
        ADD = "Ajouter un programme…", "Add a program…";
        ADD_TIP = "Choisir un programme (.exe) à montrer comme jeu quand il tourne", "Pick a program (.exe) to show as a game while it runs";
        PICK_TITLE = "Programme à montrer comme jeu", "Program to show as a game";
        PICK_FILTER = "Programmes", "Programs";
        REMOVE = "Retirer", "Remove";
        ADDED_TAG = "ajouté à la main", "added manually";
        ALREADY = "Ce programme est déjà dans la liste.", "This program is already listed.";
        TOO_MANY = "Liste pleine\u{a0}: retirez un programme d'abord.", "List full: remove a program first.";
        LISTEN_SPOTIFY = "Écouter sur Spotify", "Play on Spotify";
        WATCH_STREAM = "Regarder le direct", "Watch stream";
        BUTTON_FAILED = "L'adresse de ce bouton est indisponible.", "This button's link is unavailable.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        elapsed(time) = "{time} écoulées", "{time} elapsed";
        by(artist) = "par {artist}", "by {artist}";
        on_album(album) = "sur {album}", "on {album}";
        running_since(time) = "En cours depuis {time}", "Running for {time}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        known = ["{n} jeu reconnaissable par Discord", "{n} jeux reconnaissables par Discord"], ["{n} game Discord can detect", "{n} games Discord can detect"];
    }
}
