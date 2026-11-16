//! Textes des notifications de bureau, des sons, de la pastille et de la
//! zone de notification (lot S5). Module enfant de `table` (il en reprend
//! les macros), tenu à part pour que les ajouts des lots parallèles ne se
//! mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SECTION_DESKTOP = "Bureau", "Desktop";
        DESKTOP_TIP = "Réglages de cet ordinateur (fichier notify.toml)\u{a0}: ils ne partent pas chez Discord. Les réglages par serveur et par salon décident quels messages notifient\u{a0}; rien ne notifie en statut Ne pas déranger, ni le salon ouvert quand la fenêtre est au premier plan.", "Settings of this computer (notify.toml file): they aren't sent to Discord. Per-server and per-channel settings decide which messages notify; nothing notifies in Do Not Disturb, nor the open channel while the window is in front.";
        TOASTS = "Notifications de bureau", "Desktop notifications";
        TOASTS_TIP = "Une notification de Windows par message qui vous notifie (au plus une par salon toutes les quelques secondes). Un clic ouvre le salon.", "A Windows notification for each message that notifies you (at most one per channel every few seconds). Clicking opens the channel.";
        SOUNDS = "Sons", "Sounds";
        SOUNDS_TIP = "Messages, mentions, vocal et appels", "Messages, mentions, voice and calls";
        VOLUME = "Volume des sons", "Sound volume";
        VOLUME_TIP = "De 0 à 100", "From 0 to 100";
        TEST_SOUND = "Écouter", "Listen";
        BADGE = "Pastille", "Badge";
        BADGE_TIP = "Nombre de mentions non lues dans le titre de la fenêtre et sur l'icône de la zone de notification\u{a0}; la fenêtre clignote dans la barre des tâches à chaque mention.", "Unread mention count in the window title and on the notification area icon; the window flashes in the taskbar on each mention.";
        TRAY = "Icône dans la zone de notification", "Notification area icon";
        TRAY_TIP = "Près de l'horloge\u{a0}: afficher ou masquer la fenêtre, statut, quitter.", "Next to the clock: show or hide the window, status, quit.";
        CLOSE_TO_TRAY = "Fermer dans la zone de notification", "Close to the notification area";
        CLOSE_TO_TRAY_TIP = "La croix de la fenêtre la masque au lieu de quitter\u{a0}; «\u{a0}Quitter\u{a0}» dans le menu de l'icône ferme Ferrocord.", "The window's close button hides it instead of quitting; “Quit” in the icon's menu closes Ferrocord.";
        TRAY_SHOW = "Afficher Ferrocord", "Show Ferrocord";
        TRAY_HIDE = "Masquer Ferrocord", "Hide Ferrocord";
        TRAY_STATUS = "Statut", "Status";
        TRAY_QUIT = "Quitter Ferrocord", "Quit Ferrocord";
        // Canal des notifications d'Android (réglages du système).
        CHANNEL_MESSAGES = "Messages", "Messages";
        ATTACHMENT = "Pièce jointe", "Attachment";
        EMBED = "Contenu intégré", "Embed";
        SPOILER = "(spoiler)", "(spoiler)";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        save_failed(reason) = "Réglages de bureau non enregistrés\u{a0}: {reason}", "Desktop settings not saved: {reason}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        unread_mentions = ["{n} mention non lue", "{n} mentions non lues"], ["{n} unread mention", "{n} unread mentions"];
    }
}
