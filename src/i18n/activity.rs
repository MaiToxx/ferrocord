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
        SCAN_TIP = "Les noms des programmes en cours sont relevés toutes les 10\u{a0}secondes, sur cet ordinateur seulement, sans ouvrir aucun programme\u{a0}; seul le nom du jeu part chez Discord.", "Running program names are checked every 10 seconds, on this computer only, without opening any program; only the game's name is sent to Discord.";
        SEC_RECENT = "Jeux vus récemment", "Recently seen games";
        NONE_RECENT = "Aucun jeu vu pour l'instant.", "No game seen yet.";
        RECENT_TIP = "Annoncer ce jeu aux autres quand il tourne\u{a0}; coupé, il n'est jamais envoyé à Discord", "Announce this game to others while it runs; when off, it is never sent to Discord";
        HIDDEN_TAG = "masqué, jamais annoncé", "hidden, never announced";
        NOW_TAG = "en cours", "running";
        SHARE_GAME = "Partager l'écran de ce jeu", "Share this game's screen";
        CARD_TIP = "Activité détectée\u{a0}: ouvrir les réglages d'activité", "Detected activity: open activity settings";
        JUST_STARTED = "à l'instant", "just now";
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
        // Carte d'activité des profils (lot « profils ») : titre selon le genre, menu « … ».
        HEAD_PLAYING = "Joue à", "Playing";
        HEAD_STREAMING = "En direct", "Live";
        HEAD_LISTENING = "Écoute", "Listening to";
        HEAD_WATCHING = "Regarde", "Watching";
        HEAD_COMPETING = "Participe à", "Competing in";
        CARD_MENU = "Options de l'activité", "Activity options";
        MENU_SETTINGS = "Réglages d'activité", "Activity settings";
        MENU_FULL = "Voir toute l'activité", "View all activity";
        MENU_COPY_APP = "Copier l'identifiant de l'application", "Copy application ID";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        elapsed(time) = "{time} écoulées", "{time} elapsed";
        by(artist) = "par {artist}", "by {artist}";
        on_album(album) = "sur {album}", "on {album}";
        party(now, max) = "{now} sur {max}", "{now} of {max}";
        running_since(time) = "En cours depuis {time}", "Running for {time}";
        since_minutes(n) = "depuis {n}\u{a0}min", "for {n} min";
        since_hours(h, m) = "depuis {h}\u{a0}h\u{a0}{m}", "for {h} h {m} min";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        known = ["{n} jeu reconnaissable par Discord", "{n} jeux reconnaissables par Discord"], ["{n} game Discord can detect", "{n} games Discord can detect"];
    }
}
