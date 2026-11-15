//! Textes de la disposition mobile (phase 7, lot P2).
//! Module enfant de `table`, mêmes macros.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        TAB_HOME = "Accueil", "Home";
        TAB_DMS = "Messages", "Messages";
        TAB_INBOX = "Notifications", "Notifications";
        TAB_YOU = "Vous", "You";
        BACK = "Retour", "Back";
        MEMBERS = "Membres", "Members";
        PULL = "Tirez pour charger les messages plus anciens", "Pull to load older messages";
        RELEASE = "Relâchez pour charger les messages plus anciens", "Release to load older messages";
        LOADING = "Chargement des messages plus anciens…", "Loading older messages…";
        SETTINGS = "Paramètres", "Settings";
        MY_PROFILE = "Mon profil", "My Profile";
        LAYOUT = "Disposition", "Layout";
        LAYOUT_AUTO = "Automatique", "Automatic";
        LAYOUT_DESKTOP = "Bureau", "Desktop";
        LAYOUT_MOBILE = "Mobile", "Mobile";
        LAYOUT_TIP = "Automatique\u{a0}: mobile sur Android ou dans une fenêtre de moins de 600\u{a0}points de large.", "Automatic: mobile on Android or in a window less than 600 points wide.";
        KEYBOARD_MARGIN = "Marge du clavier virtuel", "On-screen keyboard margin";
        KEYBOARD_TIP = "Espace gardé sous la zone de saisie quand le clavier est ouvert, si le système ne donne pas sa hauteur.", "Space kept under the message box while the keyboard is open, when the system doesn't give its height.";
    }
}
