//! Textes de la page « Plugins » et du registre (`crate::plugins`).
//! Module enfant de `table`, mêmes macros. Les lots suivants y ajoutent le
//! nom et la description de leurs plugins.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Page.
        PAGE = "Plugins", "Plugins";
        GROUP = "Ferrocord", "Ferrocord";
        INTRO = "Les fonctions propres à Ferrocord. Un plugin désactivé ne coûte rien\u{a0}: ni mémoire, ni calcul.", "Features that are Ferrocord's own. A disabled plugin costs nothing: no memory, no processing.";
        SEARCH = "Rechercher un plugin", "Search plugins";
        CAT_ALL = "Tous", "All";
        CAT_APPEARANCE = "Apparence", "Appearance";
        CAT_MESSAGES = "Messages", "Messages";
        CAT_PRIVACY = "Confidentialité", "Privacy";
        CAT_ACTIVITY = "Activité", "Activity";
        CAT_VOICE = "Voix", "Voice";
        CAT_NAVIGATION = "Navigation", "Navigation";
        CAT_SYSTEM = "Système", "System";
        DISABLE_ALL = "Tout désactiver", "Disable all";
        DISABLE_ALL_TITLE = "Désactiver tous les plugins\u{a0}?", "Disable all plugins?";
        DISABLE_ALL_BODY = "Toutes les fonctions de cette page seront coupées. L'historique hors ligne gardé sur cet ordinateur sera effacé.", "Every feature on this page will be turned off. The offline history kept on this computer will be erased.";
        NONE_FOUND = "Aucun plugin ne correspond.", "No plugin matches.";
        RESTART = "Redémarrage nécessaire", "Restart required";
        RESTART_TIP = "Ce changement s'applique au prochain lancement de Ferrocord.", "This change applies the next time Ferrocord starts.";
        RESTART_PENDING = "Relancez Ferrocord pour appliquer vos changements.", "Restart Ferrocord to apply your changes.";
        DEFAULTS = "Valeurs par défaut", "Defaults";
        OFF_HERE = "Désactivé dans Paramètres > Plugins\u{a0}: ces réglages sont gardés mais sans effet.", "Disabled in Settings > Plugins: these settings are kept but have no effect.";

        // Plugins inscrits.
        OFFLINE = "Historique hors ligne", "Offline history";
        OFFLINE_ABOUT = "Garde les derniers messages des salons récents, chiffrés sur cet ordinateur, pour les afficher dès le lancement et sans réseau. Désactiver efface tout.", "Keeps recent messages of recently opened channels, encrypted on this computer, to show them right at startup and without a network. Disabling erases everything.";
        RPC = "Rich Presence locale", "Local Rich Presence";
        RPC_ABOUT = "Les jeux et logiciels de cet ordinateur affichent leur activité dans votre profil par le tube local discord-ipc.", "Games and software on this computer show their activity in your profile through the local discord-ipc pipe.";
        SPEAK = "Lecture à voix haute", "Read aloud";
        SPEAK_ABOUT = "Ajoute «\u{a0}Lire à voix haute\u{a0}» au menu d'un message, avec la voix de Windows. Rien ne quitte l'ordinateur.", "Adds “Read aloud” to a message's menu, using the Windows voice. Nothing leaves the computer.";
        SPEAK_AUTHOR = "Annoncer l'auteur", "Say the author's name";
        SPEAK_AUTHOR_TIP = "Le nom de l'auteur est lu avant le message.", "The author's name is read before the message.";
        BACKDROP = "Fond personnalisé", "Custom background";
        BACKDROP_ABOUT = "Image de fond et dégradé derrière la discussion ou toute l'application. Ils se règlent dans Apparence.", "Background image and gradient behind the chat or the whole app. Set them up in Appearance.";
        ANIM = "Animations de l'interface", "Interface animations";
        ANIM_ABOUT = "Fondus, glissements et rebonds des menus, des fenêtres, des messages et des pastilles. L'intensité se règle dans Accessibilité.", "Fades, slides and bounces of menus, windows, messages and badges. Set the intensity in Accessibility.";
        AUTO_THEME = "Thème du jour et de la nuit", "Day and night theme";
        AUTO_THEME_ABOUT = "Bascule seule entre deux thèmes, selon l'heure ou le mode clair ou sombre de Windows.", "Switches between two themes by itself, by the clock or the Windows light or dark mode.";
        FONTS = "Polices installées", "Installed fonts";
        FONTS_ABOUT = "Remplace la police de l'interface et celle du code par des polices de cet ordinateur, choisies dans Apparence.", "Replaces the interface and code fonts with fonts from this computer, chosen in Appearance.";
        LOG = "Journal des messages", "Message log";
        LOG_ABOUT = "Garde sur cet ordinateur, chiffrés, les messages supprimés ou modifiés que vous aviez déjà reçus\u{a0}: un message supprimé reste affiché, atténué, et «\u{a0}(modifié)\u{a0}» déplie les versions précédentes. Attention\u{a0}: vous conservez ce que leurs auteurs ont voulu retirer. S'efface dans Données et confidentialité.", "Keeps on this computer, encrypted, deleted or edited messages you had already received: a deleted message stays visible, dimmed, and “(edited)” unfolds the earlier versions. Warning: you keep what their authors meant to remove. Erase it in Data & Privacy.";
        LOG_DAYS = "Durée de conservation (jours)", "Keep for (days)";
        LOG_DAYS_TIP = "Au-delà, les messages gardés sont effacés.", "Kept messages are erased after that.";
        LOG_FILES = "Garder les pièces jointes", "Keep attachments";
        LOG_FILES_TIP = "Les images déjà dans le cache restent visibles\u{a0}; rien n'est téléchargé pour le journal.", "Images already in the cache stay visible; nothing is downloaded for the log.";
        LOG_BOTS = "Ignorer les bots", "Ignore bots";
        LOG_BOTS_TIP = "Les messages des bots et des applications ne sont pas gardés.", "Messages from bots and apps are not kept.";
        KEYWORDS = "Mots-clés surveillés", "Watched keywords";
        KEYWORDS_ABOUT = "Un message reçu qui contient un mot de votre liste donne une notification et une ligne dans la boîte de réception, et le mot est surligné dans le fil. La liste se règle dans Paramètres > Notifications. Rien n'est demandé à Discord.", "A received message containing a word from your list gives a notification and an inbox line, and the word is highlighted in the feed. Edit the list in Settings > Notifications. Nothing is asked of Discord.";
        LOCAL_SEARCH = "Recherche locale", "Local search";
        LOCAL_SEARCH_ABOUT = "Cherche pendant la frappe dans les messages déjà reçus sur cet ordinateur, sans rien demander à Discord\u{a0}: bouton «\u{a0}Local\u{a0}» à côté du champ de recherche. L'index est borné\u{a0}; il n'est enregistré (chiffré) que si l'historique hors ligne est activé.", "Searches as you type through messages already received on this computer, asking Discord nothing: “Local” button next to the search field. The index is bounded; it is saved (encrypted) only when offline history is enabled.";
        EXPORT = "Export d'une conversation", "Conversation export";
        EXPORT_ABOUT = "Menu d'un salon ou d'un message privé\u{a0}: «\u{a0}Exporter la conversation…\u{a0}» en page HTML lisible hors ligne ou en texte brut, sur la période choisie. Part de ce qui est déjà sur cet ordinateur\u{a0}; l'historique manquant est lu page par page, au rythme normal, avec un bouton Annuler. Les pièces jointes ne sont téléchargées que si vous le demandez.", "Menu of a channel or direct message: “Export conversation…” as an HTML page readable offline or as plain text, for the chosen period. Starts from what is already on this computer; missing history is read page by page, at the normal pace, with a Cancel button. Attachments are downloaded only if you ask.";
        CRASH = "Rapport de plantage local", "Local crash report";
        CRASH_ABOUT = "Si Ferrocord s'arrête brutalement, un fichier lisible est écrit dans le dossier «\u{a0}plantages\u{a0}» de cet ordinateur (date, version, endroit du code, derniers événements techniques), et proposé au lancement suivant. Il ne contient ni jeton, ni message, ni nom de compte, et rien n'est jamais envoyé. Les dix derniers sont gardés.", "If Ferrocord stops unexpectedly, a readable file is written to this computer's “plantages” folder (date, version, place in the code, last technical events) and offered at the next start. It holds no token, no message, no account name, and nothing is ever sent. The last ten are kept.";
        MICA = "Mica et transparence de Windows 11", "Windows 11 Mica and transparency";
        MICA_ABOUT = "La matière de Windows 11 (Mica ou acrylique) se voit derrière la barre des serveurs et la colonne des salons, sous un voile de la couleur du thème. Demande Windows 11 22H2 ou plus récent\u{a0}; ailleurs, rien ne change.", "The Windows 11 material (Mica or acrylic) shows behind the server bar and the channel column, under a veil of the theme colour. Needs Windows 11 22H2 or later; elsewhere nothing changes.";
        MICA_MATERIAL = "Matière", "Material";
        MICA_MATERIAL_TIP = "Mica reprend discrètement le fond d'écran\u{a0}; l'acrylique laisse voir, floues, les fenêtres de derrière.", "Mica subtly picks up the wallpaper; acrylic shows the windows behind, blurred.";
        MICA_OPT_MICA = "Mica", "Mica";
        MICA_OPT_ACRYLIC = "Acrylique", "Acrylic";
        MICA_VEIL = "Voile du thème (%)", "Theme veil (%)";
        MICA_VEIL_TIP = "Part de la couleur du thème posée sur la matière\u{a0}: plus elle est haute, plus le texte se détache.", "Share of the theme colour laid over the material: the higher, the more the text stands out.";
        UNAVAILABLE = "Non disponible sur ce système", "Not available on this system";
        UNAVAILABLE_TIP = "Ce système ne permet pas cette fonction\u{a0}: l'activer ne change rien.", "This system does not allow this feature: enabling it changes nothing.";
        SHARE_THEMES = "Thèmes partageables", "Shareable themes";
        SHARE_THEMES_ABOUT = "Exporte le thème en cours (couleurs, dégradé, rayons, densité, polices, image de fond si vous le voulez) dans un seul fichier .ferrotheme, et importe ceux qu'on vous donne, avec un aperçu avant de les appliquer\u{a0}: section «\u{a0}Thèmes partageables\u{a0}» de la page Apparence, ou fichier déposé sur la fenêtre. Quelques thèmes sont fournis.", "Exports the current theme (colours, gradient, radii, density, fonts, background image if you wish) to a single .ferrotheme file, and imports the ones you are given, with a preview before applying: “Shareable themes” section of the Appearance page, or a file dropped on the window. A few themes are included.";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        enabled(n, total) = "Activés\u{a0}: {n} sur {total}", "Enabled: {n} of {total}";
        settings_of(name) = "Réglages de «\u{a0}{name}\u{a0}»", "“{name}” settings";
    }
}
