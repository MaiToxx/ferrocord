//! Textes des plugins « fenêtres » (lot E2) : onglets de salons, vue scindée,
//! fenêtre détachée, palette de commandes, zoom par salon.
//! Module enfant de `table` (mêmes macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Registre des plugins.
        TABS = "Onglets de salons", "Channel tabs";
        TABS_ABOUT = "Une barre d'onglets au-dessus de la discussion\u{a0}: clic molette, Ctrl+clic ou «\u{a0}Ouvrir dans un onglet\u{a0}» sur un salon ou un message privé. Ctrl+Tab, Ctrl+1 à 9, Ctrl+W\u{a0}; les onglets sont retenus d'une session à l'autre, pour chaque compte.", "A tab bar above the conversation: middle-click, Ctrl+click or “Open in a tab” on a channel or a direct message. Ctrl+Tab, Ctrl+1 to 9, Ctrl+W; tabs are remembered between sessions, for each account.";
        SPLIT = "Vue scindée", "Split view";
        SPLIT_ABOUT = "Deux discussions côte à côte\u{a0}: «\u{a0}Ouvrir à droite\u{a0}» dans le menu d'un salon ou d'un onglet. La séparation se déplace à la souris et est retenue\u{a0}; un clic dans une discussion la rend active.", "Two conversations side by side: “Open to the right” in the menu of a channel or a tab. The divider can be dragged and is remembered; clicking a conversation makes it the active one.";
        DETACH = "Fenêtres détachées", "Detached windows";
        DECK = "Stream Deck", "Stream Deck";
        DECK_ABOUT = "Micro, casque, appui-pour-parler, tableau de sons, caméra, partage d'écran, raccrocher, répondre ou refuser un appel depuis un Stream Deck d'Elgato, avec l'état réel sur les touches. À l'activation, Ferrocord pose son plugin dans Stream Deck (à relancer une fois) et ouvre une commande locale, limitée à ces actions\u{a0}; rien ne sort de cet ordinateur.", "Microphone, headset, push-to-talk, soundboard, camera, screen share, hang up, answer or decline a call from an Elgato Stream Deck, with the real state on the keys. When turned on, Ferrocord installs its plugin into Stream Deck (restart it once) and opens a local control channel limited to these actions; nothing leaves this computer.";
        DECK_INSTALLED = "Plugin Stream Deck installé\u{a0}: quittez puis relancez Stream Deck, les touches «\u{a0}Ferrocord\u{a0}» apparaîtront dans sa liste d'actions.", "Stream Deck plugin installed: quit and restart Stream Deck, the “Ferrocord” keys will show up in its action list.";
        DECK_ABSENT = "Stream Deck n'a pas été trouvé sur cet ordinateur\u{a0}: installez-le, puis coupez et réactivez ce plugin.", "Stream Deck was not found on this computer: install it, then turn this plugin off and on again.";
        DECK_FAILED = "Le plugin n'a pas pu être écrit dans le dossier de Stream Deck.", "The plugin could not be written into the Stream Deck folder.";
        DETACH_ABOUT = "«\u{a0}Ouvrir dans une fenêtre\u{a0}»\u{a0}: un salon ou un message privé dans sa propre fenêtre, avec sa discussion et sa saisie. Sa place et sa taille sont retenues.", "“Open in a window”: a channel or a direct message in its own window, with its conversation and its message box. Its position and size are remembered.";
        PALETTE = "Palette de commandes", "Command palette";
        PALETTE_ABOUT = "Ctrl+Maj+P (ou «\u{a0}>\u{a0}» dans le sélecteur rapide)\u{a0}: toutes les actions de l'application au clavier, par une recherche floue.", "Ctrl+Shift+P (or “>” in the quick switcher): every action of the app from the keyboard, through a fuzzy search.";
        ZOOM = "Zoom par salon", "Per-channel zoom";
        ZOOM_ABOUT = "Ctrl+molette, Ctrl+= et Ctrl+- changent la taille du texte de la discussion affichée seulement, retenue pour chaque salon\u{a0}; Ctrl+0 la remet à 100\u{a0}%. Coupé, ces raccourcis règlent le zoom de toute l'interface.", "Ctrl+wheel, Ctrl+= and Ctrl+- change the text size of the conversation on screen only, remembered for each channel; Ctrl+0 resets it to 100%. When off, these shortcuts set the zoom of the whole interface.";

        // Menus d'un salon, d'un message privé, d'un onglet.
        OPEN_IN_TAB = "Ouvrir dans un onglet", "Open in a tab";
        OPEN_RIGHT = "Ouvrir à droite", "Open to the right";
        OPEN_WINDOW = "Ouvrir dans une fenêtre", "Open in a window";
        TAB_CLOSE = "Fermer l'onglet", "Close tab";
        TAB_CLOSE_OTHERS = "Fermer les autres onglets", "Close other tabs";
        TAB_PIN = "Épingler l'onglet", "Pin tab";
        TAB_UNPIN = "Désépingler l'onglet", "Unpin tab";
        TAB_LIST = "Tous les onglets", "All tabs";
        TAB_FRIENDS = "Amis", "Friends";
        TAB_UNKNOWN = "Salon indisponible", "Unavailable channel";
        TAB_UNREAD = "non lu", "unread";

        // Vue scindée et fenêtre détachée.
        PANE_CLOSE = "Fermer cette discussion", "Close this conversation";
        PANE_WRITE_HERE = "Cliquez pour écrire ici", "Click to write here";
        PANE_DRAFT = "Brouillon en attente\u{a0}: cliquez pour le reprendre", "Draft waiting: click to resume it";
        PANE_TO_MAIN = "Ouvrir dans la fenêtre principale", "Open in the main window";
        PANE_SEND_TIP = "Entrée pour envoyer, Maj+Entrée pour aller à la ligne", "Enter to send, Shift+Enter for a new line";
        PANE_TOO_MANY = "Trop de fenêtres détachées ouvertes.", "Too many detached windows are open.";
        PANE_FORUM = "Ce salon s'ouvre dans la fenêtre principale.", "This channel opens in the main window.";
        PANE_ELSEWHERE = "Cette discussion est affichée dans la fenêtre principale.", "This conversation is shown in the main window.";

        // Palette de commandes.
        PALETTE_HINT = "Tapez une commande", "Type a command";
        PALETTE_NONE = "Aucune commande ne correspond.", "No command matches.";
        PALETTE_TIP = "Entrée pour lancer, flèches pour choisir, Échap pour fermer.", "Enter to run, arrows to choose, Esc to close.";
        G_NAV = "Navigation", "Navigation";
        G_SETTINGS = "Paramètres", "Settings";
        G_THEME = "Thème", "Theme";
        G_STATUS = "Statut", "Status";
        G_ACCOUNT = "Compte", "Account";
        G_PLUGIN = "Plugins", "Plugins";
        G_VOICE = "Voix", "Voice";
        G_VIEW = "Affichage", "View";
        G_ACTION = "Action", "Action";
        CMD_MARK_ALL_READ = "Marquer tout comme lu", "Mark everything as read";
        CMD_QUESTS = "Ouvrir les quêtes", "Open quests";
        CMD_FRIENDS = "Ouvrir la page Amis", "Open the Friends page";
        CMD_COMPACT = "Basculer le mode compact (une ligne par message)", "Toggle compact mode (one line per message)";
        CMD_AVATARS = "Afficher ou masquer les avatars", "Show or hide avatars";
        CMD_ZOOM_IN = "Agrandir le texte de ce salon", "Make this channel's text larger";
        CMD_ZOOM_OUT = "Réduire le texte de ce salon", "Make this channel's text smaller";
        CMD_ZOOM_RESET = "Remettre le texte de ce salon à 100\u{a0}%", "Reset this channel's text to 100%";
        CMD_TAB_NEW = "Nouvel onglet", "New tab";
        CMD_TAB_NEXT = "Onglet suivant", "Next tab";
        CMD_TAB_PREV = "Onglet précédent", "Previous tab";
        CMD_SPLIT_CLOSE = "Fermer la vue scindée", "Close the split view";
        CMD_LOGOUT = "Se déconnecter…", "Log out…";
        CMD_ADD_ACCOUNT = "Ajouter un compte…", "Add an account…";
        CMD_PALETTE = "Palette de commandes", "Command palette";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        go_to(name) = "Aller à {name}", "Go to {name}";
        settings_page(page) = "Paramètres\u{a0}: {page}", "Settings: {page}";
        theme(name) = "Thème\u{a0}: {name}", "Theme: {name}";
        status(name) = "Statut\u{a0}: {name}", "Status: {name}";
        account(name) = "Changer de compte\u{a0}: {name}", "Switch account: {name}";
        plugin_on(name) = "Activer le plugin «\u{a0}{name}\u{a0}»", "Turn on the “{name}” plugin";
        plugin_off(name) = "Couper le plugin «\u{a0}{name}\u{a0}»", "Turn off the “{name}” plugin";
        zoom_now(n) = "Texte de ce salon\u{a0}: {n}\u{a0}%", "This channel's text: {n}%";
        tab_mentions(name, n) = "{name}, {n} mention(s)", "{name}, {n} mention(s)";
        tab_unread(name) = "{name}, non lu", "{name}, unread";
        window_title(name) = "{name} - Ferrocord", "{name} - Ferrocord";
        message_to(name) = "Envoyer un message à {name}", "Message {name}";
    }
}
