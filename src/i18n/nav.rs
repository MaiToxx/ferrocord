//! Textes de la navigation : sélecteur rapide, boîte de réception,
//! raccourcis clavier (lot navigation). Module enfant de `table` (il en
//! reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SWITCHER_HINT = "Où voulez-vous aller\u{a0}?", "Where would you like to go?";
        SWITCHER_TIP = "Astuce\u{a0}: commencez par «\u{a0}#\u{a0}» pour les salons, «\u{a0}@\u{a0}» pour les personnes, «\u{a0}*\u{a0}» pour les serveurs, «\u{a0}!\u{a0}» pour les salons vocaux.", "Tip: start with “#” for channels, “@” for people, “*” for servers, “!” for voice channels.";
        NO_MATCH = "Aucun résultat.", "No results.";
        KIND_GUILD = "Serveur", "Server";
        KIND_DM = "Message privé", "Direct message";
        KIND_GROUP = "Groupe", "Group";
        KIND_FRIEND = "Ami", "Friend";
        INBOX = "Boîte de réception", "Inbox";
        TAB_MENTIONS = "Mentions", "Mentions";
        TAB_UNREADS = "Non lus", "Unreads";
        ROLE_MENTIONS = "Mentions de rôles", "Role mentions";
        EVERYONE = "@everyone et @here", "@everyone and @here";
        THIS_SERVER = "Ce serveur seulement", "This server only";
        LOAD_MORE = "Charger plus", "Load more";
        LOADING = "Chargement…", "Loading…";
        RETRY = "Réessayer", "Retry";
        NO_MENTIONS = "Aucune mention ces 7 derniers jours.", "No mentions in the last 7 days.";
        NO_UNREADS = "Tout est lu.", "You're all caught up.";
        REMOVE_MENTION = "Retirer de la liste", "Remove from the list";
        MENTIONED = "Vous êtes mentionné", "You are mentioned";
        SHORTCUTS = "Raccourcis clavier", "Keyboard shortcuts";
        SHORTCUTS_EDITABLE = "Raccourcis modifiables", "Editable shortcuts";
        MOUSE_NOTE = "Boutons 4 et 5 de la souris\u{a0}: revenir en arrière, aller en avant.", "Mouse buttons 4 and 5: go back, go forward.";
        TYPING_NOTE = "Une touche seule (sans Ctrl ni Alt) ne se déclenche pas pendant la saisie d'un texte.", "A key alone (without Ctrl or Alt) does nothing while you are typing.";
        FILE_NOTE = "Enregistrés dans keys.toml, à côté de theme.toml.", "Saved in keys.toml, next to theme.toml.";
        EDIT_IN_SETTINGS = "Modifier les raccourcis", "Edit shortcuts";
        NO_CHORD = "Aucun", "None";
        PRESS_KEYS = "Appuyez sur les touches… (Échap\u{a0}: annuler)", "Press the keys… (Esc: cancel)";
        CHANGE = "Modifier", "Change";
        REMOVE = "Retirer", "Remove";
        RESET = "Par défaut", "Default";
        RESET_ALL = "Tout remettre par défaut", "Reset all";
        MOD_CTRL = "Ctrl", "Ctrl";
        MOD_ALT = "Alt", "Alt";
        MOD_SHIFT = "Maj", "Shift";
        KEY_ESCAPE = "Échap", "Esc";
        KEY_PAGE_UP = "Page préc.", "Page Up";
        KEY_PAGE_DOWN = "Page suiv.", "Page Down";
        KEY_ENTER = "Entrée", "Enter";
        KEY_SPACE = "Espace", "Space";
        KEY_BACKSPACE = "Retour arrière", "Backspace";
        KEY_DELETE = "Suppr", "Delete";
        KEY_HOME = "Origine", "Home";
        KEY_END = "Fin", "End";
        KEY_TAB = "Tab", "Tab";
        ACT_QUICK_SWITCHER = "Sélecteur rapide", "Quick switcher";
        ACT_INBOX = "Boîte de réception", "Inbox";
        ACT_SHORTCUTS = "Liste des raccourcis", "Shortcut list";
        ACT_SETTINGS = "Paramètres (Apparence)", "Settings (Appearance)";
        ACT_PREV_CHANNEL = "Salon précédent", "Previous channel";
        ACT_NEXT_CHANNEL = "Salon suivant", "Next channel";
        ACT_PREV_UNREAD = "Salon non lu précédent", "Previous unread channel";
        ACT_NEXT_UNREAD = "Salon non lu suivant", "Next unread channel";
        ACT_PREV_GUILD = "Serveur précédent", "Previous server";
        ACT_NEXT_GUILD = "Serveur suivant", "Next server";
        ACT_BACK = "Revenir en arrière", "Go back";
        ACT_FORWARD = "Aller en avant", "Go forward";
        ACT_MARK_READ = "Marquer le salon comme lu", "Mark channel as read";
        ACT_MARK_GUILD_READ = "Marquer le serveur comme lu", "Mark server as read";
        ACT_REPLY_LAST = "Répondre au dernier message", "Reply to the last message";
        ACT_PAGE_UP = "Remonter d'une page", "Scroll up one page";
        ACT_PAGE_DOWN = "Descendre d'une page", "Scroll down one page";
        ACT_EMOJIS = "Sélecteur d'emojis", "Emoji picker";
        ACT_PINS = "Messages épinglés", "Pinned messages";
        ACT_MEMBERS = "Liste des membres", "Member list";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        with_chord(label, chord) = "{label} ({chord})", "{label} ({chord})";
        conflict(other) = "Même combinaison que «\u{a0}{other}\u{a0}»", "Same keys as “{other}”";
        load_failed(reason) = "Mentions indisponibles\u{a0}: {reason}", "Mentions unavailable: {reason}";
        keys_bad_line(line) = "keys.toml\u{a0}: ligne {line} ignorée", "keys.toml: line {line} ignored";
        keys_read_failed(reason) = "keys.toml illisible ({reason})\u{a0}: raccourcis par défaut", "keys.toml unreadable ({reason}): default shortcuts";
        keys_save_failed(reason) = "keys.toml non enregistré\u{a0}: {reason}", "keys.toml not saved: {reason}";
        in_guild(channel, guild) = "#{channel} \u{b7} {guild}", "#{channel} \u{b7} {guild}";
    }
}
