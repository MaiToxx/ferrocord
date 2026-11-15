//! Textes du lot N (phase 10) : raccourcis ajoutés, fenêtre des
//! notifications d'un serveur, réglage par fil, onglet « Pour vous »,
//! notifications de bureau. Module enfant de `table` (il en reprend les
//! macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        ACT_SEARCH = "Rechercher", "Search";
        ACT_UPLOAD = "Joindre un fichier", "Upload a file";
        ACT_GIFS = "Ouvrir les GIF", "Open the GIF picker";
        ACT_STICKERS = "Ouvrir les autocollants", "Open the sticker picker";
        ACT_CREATE_GUILD = "Créer ou rejoindre un serveur", "Create or join a server";
        ACT_THREADS = "Ouvrir les fils du salon", "Open the channel's threads";
        ACT_HELP = "Ouvrir l'aide de Discord", "Open Discord's help";
        ACT_OLDEST_UNREAD = "Aller au plus ancien message non lu", "Jump to the oldest unread message";
        ACT_ANSWER_CALL = "Répondre à l'appel entrant", "Answer the incoming call";
        ACT_DECLINE_CALL = "Refuser l'appel entrant", "Decline the incoming call";
        ACT_START_CALL = "Démarrer un appel dans le message privé ouvert", "Start a call in the open direct message";
        FIXED_TITLE = "Raccourcis fixes", "Fixed shortcuts";
        FIX_GUILD_N = "Aller aux messages privés (1) ou au serveur de ce rang", "Go to direct messages (1) or to the server at that position";
        FIX_BOLD = "Gras sur le texte sélectionné", "Bold the selected text";
        FIX_ITALIC = "Italique sur le texte sélectionné", "Italicize the selected text";
        FIX_UNDERLINE = "Souligner le texte sélectionné", "Underline the selected text";
        FIX_ZONES = "Passer d'une zone de la fenêtre à la suivante", "Move to the next area of the window";
        FIX_FOCUS = "Passer à l'élément suivant (Maj\u{a0}: précédent)", "Move to the next element (Shift: previous)";

        ACTION_THREAD_NOTIFY = "Réglage des notifications du fil", "Changing the thread's notifications";
        THREAD_NOTIFY = "Notifications", "Notifications";
        THREAD_NOTIFY_TIP = "Notifications de ce fil", "This thread's notifications";
        THREAD_DEFAULT = "Aucun réglage choisi\u{a0}: celui du serveur s'applique.", "No setting chosen: the server's applies.";

        OPEN_WINDOW = "Paramètres de notification", "Notification Settings";
        DONE = "Terminé", "Done";
        MUTE_NOTE = "Un serveur muet ne signale plus ses messages non lus, sauf si vous êtes mentionné.", "A muted server no longer shows unread messages unless you are mentioned.";
        SERVER_LEVEL = "Notifications du serveur", "Server Notification Settings";
        OVERRIDES = "Exceptions par salon", "Notification Overrides";
        OVERRIDES_NOTE = "Un salon ou une catégorie réglé ici ne suit plus le réglage du serveur.", "A channel or category set here no longer follows the server's setting.";
        ADD_HINT = "Chercher un salon ou une catégorie", "Search for a channel or category";
        NO_OVERRIDE = "Aucune exception pour l'instant.", "No overrides yet.";
        NO_CHANNEL = "Aucun salon de ce nom.", "No channel with that name.";
        COL_ALL = "Tout", "All";
        COL_MENTIONS = "Mentions", "Mentions";
        COL_NOTHING = "Rien", "Nothing";
        COL_MUTE = "Muet", "Mute";
        COL_DEFAULT = "Réglage du serveur", "Server setting";
        REMOVE_OVERRIDE = "Retirer l'exception", "Remove the override";
        CATEGORY = "Catégorie", "Category";

        TAB_FOR_YOU = "Pour vous", "For You";
        FOR_YOU_EMPTY = "Rien de neuf pour vous.", "Nothing new for you.";
        REQUESTS = "Demandes d'ami", "Friend requests";
        EVENTS = "Événements", "Events";
        EVENT_LIVE = "En cours", "Happening now";
        SEE = "Voir", "View";

        TOAST_PREVIEW = "Montrer le texte du message", "Show the message text";
        TOAST_PREVIEW_TIP = "Désactivé, la notification dit seulement qui a écrit et où.", "When off, the notification only says who wrote and where.";
        TOAST_HIDDEN = "Nouveau message", "New message";
        TOAST_HIDDEN_MENTION = "Vous a mentionné", "Mentioned you";
        TOAST_DMS = "Messages privés", "Direct Messages";
    }
}

/// Textes à trous.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        window_title(guild) = "Notifications de {guild}", "{guild} notifications";
        mute_guild(guild) = "Rendre {guild} muet", "Mute {guild}";
        muted_until(when) = "Muet {when}", "Muted {when}";
        friend_request(name) = "{name} vous a envoyé une demande d'ami.", "{name} sent you a friend request.";
        event_soon(name, guild, when) = "{name} \u{b7} {guild} \u{b7} {when}", "{name} \u{b7} {guild} \u{b7} {when}";
    }
}
