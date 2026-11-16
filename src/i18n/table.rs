//! Table des textes : français puis anglais. Ponctuation propre à chaque
//! langue : en français, espace insécable (`\u{a0}`) avant « : ; ? ! » et à
//! l'intérieur des guillemets « » ; en anglais, guillemets “ ”.

macro_rules! texts {
    ($( $(#[doc = $doc:literal])* $K:ident = $fr:literal, $en:literal; )+) => {
        $( $(#[doc = $doc])* pub const $K: Text = Text([$fr, $en]); )+

        /// Tous les textes constants : (clé, texte).
        #[cfg(test)]
        pub(crate) const ALL: &[(&str, Text)] = &[$( (stringify!($K), $K) ),+];
    };
}

macro_rules! templates {
    ($( $(#[doc = $doc:literal])* $name:ident($($arg:ident),+) = $fr:literal, $en:literal; )+) => {
        $(
            $(#[doc = $doc])*
            pub fn $name(lang: Lang, $($arg: impl std::fmt::Display),+) -> String {
                const T: [&str; 2] = [$fr, $en];
                fill(T[lang.index()], &[$( (stringify!($arg), &$arg as &dyn std::fmt::Display) ),+])
            }
        )+

        /// Tous les modèles : (clé, paramètres, textes).
        #[cfg(test)]
        pub(crate) const ALL: &[(&str, &[&str], [&str; 2])] = &[$( (stringify!($name), &[$(stringify!($arg)),+], [$fr, $en]) ),+];
    };
}

macro_rules! plurals {
    ($( $(#[doc = $doc:literal])* $name:ident = [$fr1:literal, $frn:literal], [$en1:literal, $enn:literal]; )+) => {
        $(
            $(#[doc = $doc])*
            pub fn $name(lang: Lang, n: u64) -> String {
                const F: [[&str; 2]; 2] = [[$fr1, $frn], [$en1, $enn]];
                fill(F[lang.index()][usize::from(!lang.singular(n))], &[("n", &number(lang, n))])
            }
        )+

        /// Tous les pluriels : (clé, [[singulier, pluriel] ; langue]).
        #[cfg(test)]
        pub(crate) const ALL: &[(&str, [[&str; 2]; 2])] = &[$( (stringify!($name), [[$fr1, $frn], [$en1, $enn]]) ),+];
    };
}

/// Textes constants.
pub mod t {
    use crate::i18n::Text;

    texts! {
        // Langue.
        LANGUAGE = "Langue", "Language";
        LANGUAGE_DETAIL = "Langue de l'interface\u{a0}; «\u{a0}Système\u{a0}» suit la langue de Windows", "Interface language; “System” follows the Windows language";
        LANGUAGE_SYSTEM = "Système", "System";
        LANGUAGE_FR = "Français", "Français";
        LANGUAGE_EN = "English", "English";

        // Connexion et statut.
        CONNECTED = "Connecté", "Connected";
        CONNECTING = "Connexion…", "Connecting…";
        RESUMING = "Reprise de la session…", "Resuming session…";
        PACKET_TOO_LARGE = "Discord a envoyé un paquet trop gros pour être lu, nouvelle tentative…", "Discord sent a packet too large to read, retrying…";
        NO_SESSION_STATE = "état de session absent", "missing session state";
        BAD_HELLO = "Hello illisible", "unreadable Hello";
        CORRUPT_STREAM = "flux compressé illisible", "unreadable compressed stream";
        TOO_LARGE_PACKET = "paquet décompressé trop gros", "decompressed packet too large";

        // Erreurs du réseau.
        BAD_TOKEN = "Jeton invalide ou expiré.", "Invalid or expired token.";
        FORBIDDEN = "Vous n'avez pas accès à ce salon", "You don't have access to this channel";
        TOO_MANY = "Trop de requêtes, réessayez plus tard", "Too many requests, try again later";
        TOO_MANY_QUEUES = "Trop d'envois en attente, réessayez plus tard", "Too many messages waiting to be sent, try again later";

        // Magasin.
        LOST_ACCESS = "Vous n'avez plus accès à ce salon.", "You no longer have access to this channel.";
        SEND_UNCONFIRMED = "Envoi non confirmé\u{a0}: le message est peut-être parti", "Sending not confirmed: the message may have been sent";
        UNKNOWN_USER = "utilisateur", "user";
        UNKNOWN_ROLE = "rôle", "role";
        UNKNOWN_CHANNEL = "salon", "channel";
        SYSTEM_JOINED = "a rejoint le serveur.", "joined the server.";
        SYSTEM_BOOSTED = "a boosté le serveur\u{a0}!", "boosted the server!";
        SYSTEM_PINNED = "a épinglé un message.", "pinned a message.";
        SYSTEM_OTHER = "(message système)", "(system message)";

        // Dates.
        TODAY = "Aujourd'hui", "Today";
        YESTERDAY = "Hier", "Yesterday";

        // Fenêtre principale.
        MESSAGE_TOO_LONG = "Message trop long (2\u{a0}000 caractères max).", "Message too long (2,000 characters max).";
        CANNOT_SEND = "Vous ne pouvez pas envoyer de message dans ce salon.", "You can't send messages in this channel.";
        /// Saisie : nom inconnu dans « X écrit… ».
        SOMEONE = "Quelqu'un", "Someone";
        SEVERAL_TYPING = "Plusieurs personnes écrivent…", "Several people are typing…";
        /// Saisie : détails des propositions de l'autocomplétion.
        ROLE_DETAIL = "Rôle", "Role";
        EVERYONE_DETAIL = "Prévient tout le monde", "Notifies everyone";
        HERE_DETAIL = "Prévient les personnes en ligne", "Notifies everyone online";
        /// Infobulle de la marque « brouillon » d'un salon.
        DRAFT_TIP = "Brouillon en cours", "Unsent draft";
        DIRECT_MESSAGES = "Messages privés", "Direct Messages";
        /// Nom d'un MP reçu sans nom ni destinataire connu.
        UNNAMED_DM = "Conversation", "Conversation";
        /// Nom d'un serveur reçu sans nom.
        UNNAMED_GUILD = "Serveur", "Server";
        DM_PILL = "MP", "DM";
        NO_CONVERSATIONS = "Aucune conversation", "No conversations";
        LOG_OUT_TIP = "Se déconnecter (le jeton enregistré est oublié)", "Log out (the saved token is forgotten)";
        APPEARANCE_TIP = "Apparence (Ctrl+,)", "Appearance (Ctrl+,)";
        PICK_CHANNEL = "Choisissez un salon ou une conversation", "Select a channel or a conversation";

        // Discussion.
        UNCONFIRMED_ROW = "Envoi non confirmé\u{a0}:", "Not confirmed:";
        RETRYING_ROW = "Nouvel essai en cours\u{a0}:", "Retrying:";
        DELETE = "Supprimer", "Delete";
        RETRY = "Réessayer", "Retry";
        NO_PERMISSION_TO_SEND = "Vous n'avez pas la permission d'envoyer des messages dans ce salon", "You don't have permission to send messages in this channel";
        JUMP_TO_PRESENT = "Revenir au présent", "Jump to present";
        NEW_LINE = "Nouveaux", "New";
        HISTORY_DENIED = "Vous n'avez pas la permission de lire l'historique de ce salon\u{a0}: seuls les nouveaux messages s'affichent.", "You don't have permission to read this channel's history: only new messages are shown.";
        NO_MESSAGES = "Aucun message pour l'instant.", "No messages yet.";
        HISTORY_START = "Début de l'historique du salon.", "Beginning of the channel's history.";
        EDITED = "(modifié)", "(edited)";
        COPY_TEXT = "Copier le texte", "Copy text";
        COPY_ID = "Copier l'identifiant", "Copy message ID";

        // Actions sur les messages (lots T5/T6).
        REPLY = "Répondre", "Reply";
        ADD_REACTION = "Ajouter une réaction", "Add reaction";
        EDIT_MESSAGE = "Modifier", "Edit";
        DELETE_MESSAGE = "Supprimer le message", "Delete message";
        PIN_MESSAGE = "Épingler", "Pin message";
        UNPIN_MESSAGE = "Désépingler", "Unpin message";
        FORWARD = "Transférer", "Forward";
        COPY_LINK = "Copier le lien du message", "Copy message link";
        MENTION_AUTHOR = "Mentionner l'auteur", "Mention author";
        CANCEL_REPLY = "Annuler la réponse (Échap)", "Cancel reply (Esc)";
        EDIT_HINT = "Échap pour annuler, Entrée pour enregistrer", "Escape to cancel, Enter to save";
        CANCEL = "Annuler", "Cancel";
        SAVE = "Enregistrer", "Save";
        DELETE_QUESTION = "Voulez-vous vraiment supprimer ce message\u{a0}?", "Are you sure you want to delete this message?";
        DELETE_TIP = "Astuce\u{a0}: Maj + clic sur «\u{a0}Supprimer\u{a0}» supprime sans confirmation.", "Tip: Shift + click on “Delete” skips this confirmation.";
        REPLY_DELETED = "Le message d'origine a été supprimé", "Original message was deleted";
        REPLY_UNKNOWN = "Message impossible à charger", "Message could not be loaded";
        REPLY_ATTACHMENT = "Pièce jointe", "Attachment";
        FORWARDED = "Transféré", "Forwarded";
        FORWARD_LABEL = "(message transféré)", "(forwarded message)";
        FORWARD_TITLE = "Transférer vers…", "Forward to…";
        SEARCH_CHANNELS = "Rechercher un salon", "Search channels";
        PINS_TITLE = "Messages épinglés", "Pinned messages";
        PINS_TIP = "Messages épinglés de ce salon", "Pinned messages of this channel";
        PINS_EMPTY = "Aucun message épinglé dans ce salon.", "This channel doesn't have any pinned messages.";
        LOAD_MORE = "Charger plus", "Load more";
        JUMP = "Aller au message", "Jump";
        LOADING = "Chargement…", "Loading…";
        SERVER_EMOJI_HINT = "Nom d'un emoji du serveur", "Server emoji name";
        SUPER_REACTION = "Super-réaction", "Super reaction";
        // Pièces jointes, embeds, visionneuse.
        SPOILER = "SPOILER", "SPOILER";
        GIF_BADGE = "GIF", "GIF";
        ALT_BADGE = "ALT", "ALT";
        CLICK_TO_LOAD = "Cliquer pour charger", "Click to load";
        IMAGE_UNAVAILABLE = "Image indisponible", "Image unavailable";
        OPEN = "Ouvrir", "Open";
        OPEN_EXTERNAL_TIP = "Ouvrir avec l'application par défaut (navigateur)", "Open with the default app (browser)";
        VIDEO = "Vidéo", "Video";
        AUDIO = "Audio", "Audio";
        OPEN_ORIGINAL = "Ouvrir l'original", "Open original";
        COPY_IMAGE_LINK = "Copier le lien", "Copy link";
        VIEWER_HINT = "Molette\u{a0}: zoom, glisser\u{a0}: déplacer, Échap\u{a0}: fermer", "Wheel: zoom, drag: move, Esc: close";
        // Profils et présence.
        STATUS_ONLINE = "En ligne", "Online";
        STATUS_IDLE = "Inactif", "Idle";
        STATUS_DND = "Ne pas déranger", "Do Not Disturb";
        STATUS_OFFLINE = "Hors ligne", "Offline";
        PLATFORM_DESKTOP = "Ordinateur", "Desktop";
        PLATFORM_MOBILE = "Mobile", "Mobile";
        PLATFORM_WEB = "Navigateur", "Web";
        PROFILE_DISCORD_SINCE = "Membre de Discord depuis", "Discord member since";
        PROFILE_SERVER_SINCE = "Membre du serveur depuis", "Server member since";
        PROFILE_BOOSTING_SINCE = "Booste le serveur depuis", "Boosting this server since";
        PROFILE_ROLES = "Rôles", "Roles";
        PROFILE_NO_ROLES = "Aucun rôle", "No roles";
        PROFILE_ACTIVITY = "Activité", "Activity";
        PROFILE_SEND_MESSAGE = "Envoyer un message", "Send message";
        PROFILE_COPY_ID = "Copier l'identifiant", "Copy user ID";
        PROFILE_ID = "Identifiant", "User ID";
        PROFILE_BOT = "Bot", "Bot";
        PROFILE_SYSTEM = "Système", "System";
        PROFILE_OPEN = "Voir le profil", "View profile";
        BADGE_STAFF = "Équipe Discord", "Discord Staff";
        BADGE_PARTNER = "Propriétaire d'un serveur partenaire", "Partnered Server Owner";
        BADGE_MODERATOR = "Ancien des programmes de modération", "Moderator Programs Alumni";
        BADGE_HYPESQUAD = "Événements HypeSquad", "HypeSquad Events";
        BADGE_BRAVERY = "HypeSquad Bravery", "HypeSquad Bravery";
        BADGE_BRILLIANCE = "HypeSquad Brilliance", "HypeSquad Brilliance";
        BADGE_BALANCE = "HypeSquad Balance", "HypeSquad Balance";
        BADGE_BUG_HUNTER = "Chasseur de bugs", "Bug Hunter";
        BADGE_BUG_HUNTER_GOLD = "Chasseur de bugs (or)", "Gold Bug Hunter";
        BADGE_ACTIVE_DEVELOPER = "Développeur actif", "Active Developer";
        BADGE_VERIFIED_DEVELOPER = "Développeur de bot vérifié de la première heure", "Early Verified Bot Developer";
        BADGE_EARLY_SUPPORTER = "Soutien de la première heure", "Early Supporter";
        BADGE_VERIFIED_BOT = "Bot vérifié", "Verified Bot";

        // Panneau Apparence.
        APPEARANCE = "Apparence", "Appearance";
        CLOSE_TIP = "Fermer (Échap)", "Close (Esc)";
        THEME_IMPORTED = "Thème importé.", "Theme imported.";
        RESET_FILE = "Réinitialiser le fichier", "Reset file";
        RESET_FILE_TIP = "Remplace theme.toml par le thème affiché\u{a0}; l'ancien fichier est d'abord copié dans theme.toml.bak", "Replaces theme.toml with the displayed theme; the old file is first copied to theme.toml.bak";
        ESSENTIALS = "Essentiel", "Essentials";
        RESET = "Réinitialiser", "Reset";
        RESET_TIP = "Revenir à toutes les valeurs du préréglage", "Restore all of the preset's values";
        PRESET = "Préréglage", "Preset";
        PRESET_TIP = "Point de départ\u{a0}; les réglages ci-dessous le modifient", "Starting point; the settings below modify it";
        ADVANCED = "Avancé", "Advanced";
        SHARE_THEME = "Partager un thème", "Share a theme";
        IMPORT = "Importer", "Import";
        IMPORT_TIP = "Lire ce fichier et l'appliquer", "Read this file and apply it";
        EXPORT = "Exporter", "Export";
        EXPORT_TIP = "Écrire le thème actuel dans ce fichier", "Write the current theme to this file";
        PREVIEW_NOTHING_SAVED = "Aperçu\u{a0}: rien n'est enregistré.", "Preview: nothing is saved.";
        CONTRAST_TEXT = "texte", "text";
        CONTRAST_MUTED = "texte atténué", "muted text";
        CONTRAST_FAINT = "texte discret", "faint text";

        // Thème : préréglages, sections, choix.
        PRESET_ROUILLE = "Rouille", "Rust";
        PRESET_MINUIT = "Minuit", "Midnight";
        PRESET_CLAIR = "Clair", "Light";
        PRESET_CONTRASTE = "Contraste élevé", "High contrast";
        PRESET_COMPACT = "Compact", "Compact";
        PRESET_AERE = "Aéré", "Spacious";
        PRESET_SOMBRE = "Sombre", "Dark";
        PRESET_CENDRE = "Cendre", "Ash";
        PRESET_ONYX = "Onyx", "Onyx";
        TITLEBAR_INTEGRATED = "Intégrée", "Built-in";
        TITLEBAR_NATIVE = "Windows", "Windows";
        FONT_FIGTREE = "Figtree", "Figtree";
        FONT_SYSTEM = "Segoe UI (système)", "Segoe UI (system)";
        WINDOW_MINIMIZE = "Réduire", "Minimize";
        WINDOW_MAXIMIZE = "Agrandir", "Maximize";
        WINDOW_RESTORE = "Niveau inférieur", "Restore down";
        WINDOW_CLOSE = "Fermer", "Close";
        HELP = "Aide", "Help";
        GALLERY_BUTTONS = "Boutons", "Buttons";
        GALLERY_FIELD = "Champ de formulaire", "Form field";
        GALLERY_FIELD_HINT = "Texte d'essai", "Sample text";
        GALLERY_CONTROLS = "Réglages", "Controls";
        GALLERY_MENU = "Menu", "Menu";
        GALLERY_MODAL = "Fenêtre modale", "Dialog";
        GALLERY_MODAL_BODY = "Le pied porte les boutons, alignés à droite, le principal au bout.", "The footer holds the buttons, right-aligned, the main one last.";
        GALLERY_OPTION = "Option", "Option";
        GALLERY_MORE = "Plus", "More";
        SECTION_COLORS = "Couleurs", "Colors";
        SECTION_TYPOGRAPHY = "Typographie", "Typography";
        SECTION_SHAPES = "Formes", "Shapes";
        SECTION_SPACING = "Espacements", "Spacing";
        SECTION_LAYOUT = "Mise en page", "Layout";
        SECTION_RICH_TEXT = "Texte mis en forme", "Formatted text";
        DENSITY_AIRY = "Aérée", "Spacious";
        DENSITY_NORMAL = "Normale", "Normal";
        DENSITY_COMPACT = "Compacte", "Compact";
        MESSAGES_GROUPED = "Groupés", "Grouped";
        MESSAGES_COMPACT = "Une ligne", "Single line";
        CLOCK_24 = "24\u{a0}h", "24-hour";
        CLOCK_12 = "12\u{a0}h (AM/PM)", "12-hour (AM/PM)";
        RAIL_PILLS = "Pastilles", "Pills";
        RAIL_LIST = "Liste", "List";
        SHAPE_ROUND = "Ronds", "Round";
        SHAPE_SQUARE = "Carrés arrondis", "Rounded squares";
        AVATARS_SHOWN = "Affichés", "Shown";
        AVATARS_HIDDEN = "Masqués", "Hidden";
        MEDIA_AUTO = "Automatiquement", "Automatically";
        MEDIA_CLICK = "Au clic", "On click";
        TYPING_SENT = "Envoyé", "Sent";
        TYPING_NEVER = "Jamais envoyé", "Never sent";
        STATUS_DOTS_SHOWN = "Affichées", "Shown";
        STATUS_DOTS_HIDDEN = "Masquées", "Hidden";
        MEMBERS_SHOWN = "À droite", "On the right";
        MEMBERS_HIDDEN = "Masquée", "Hidden";

        // Liste des membres.
        MEMBERS = "Membres", "Members";
        MEMBERS_ONLINE = "En ligne", "Online";
        MEMBERS_OFFLINE = "Hors ligne", "Offline";
        MEMBERS_SHOW_TIP = "Afficher la liste des membres", "Show the member list";
        MEMBERS_HIDE_TIP = "Masquer la liste des membres", "Hide the member list";
        MEMBERS_LOADING = "Chargement de la liste des membres…", "Loading the member list…";
        MEMBERS_BOT = "Discord n'envoie pas la liste des membres à un compte bot.", "Discord doesn't send the member list to a bot account.";
        BOT_TAG = "BOT", "BOT";
        APP_TAG = "APP", "APP";

        // Fichier de thème.
        THEME_PARSER_PANIC = "fichier illisible (erreur interne de l'analyseur)", "unreadable file (internal parser error)";
        THEME_BASE_NOT_STRING = "«\u{a0}base\u{a0}» doit être un nom de préréglage entre guillemets", "“base” must be a preset name in quotes";
        THEME_SYNTAX = "syntaxe TOML invalide", "invalid TOML syntax";
        THEME_COLOR_LIST = "liste de couleurs attendue", "list of colors expected";
        THEME_QUOTED = "texte entre guillemets attendu", "quoted text expected";
        THEME_INT_TOO_BIG = "entier trop grand", "integer too large";
        THEME_BAD_NUMBER = "nombre illisible", "unreadable number";
        THEME_FINITE = "nombre fini attendu", "finite number expected";
        KIND_STRING = "un texte", "a string";
        KIND_INTEGER = "un entier", "an integer";
        KIND_FLOAT = "un nombre", "a number";
        KIND_BOOLEAN = "un booléen", "a boolean";
        KIND_DATETIME = "une date", "a date";
        KIND_ARRAY = "une liste", "an array";
        KIND_TABLE = "une table", "a table";
        THEME_FILE_HEADER = "# Thème Ferrocord.\n# Modifiable dans un éditeur de texte\u{a0}: l'application relit ce fichier dès\n# qu'il change. Les clés absentes prennent la valeur du préréglage «\u{a0}base\u{a0}».\n# Couleurs\u{a0}: \"#rrggbb\" (ou \"#rrggbbaa\" avec transparence).\n",
            "# Ferrocord theme.\n# Editable in a text editor: the application reloads this file as soon as\n# it changes. Missing keys take the value of the “base” preset.\n# Colors: \"#rrggbb\" (or \"#rrggbbaa\" with transparency).\n";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use crate::i18n::{fill::fill, Lang};

    templates! {
        // Statut.
        identify_cap(time) = "Trop de connexions aujourd'hui, nouvel essai à {time}", "Too many connections today, retrying at {time}";
        disconnected(reason) = "Déconnecté ({reason}), nouvelle tentative…", "Disconnected ({reason}), retrying…";
        disconnected_no_compression(reason) = "Déconnecté ({reason}), nouvelle tentative sans compression…", "Disconnected ({reason}), retrying without compression…";
        bad_heartbeat_interval(ms) = "heartbeat_interval anormal ({ms} ms)", "abnormal heartbeat_interval ({ms} ms)";
        decode_interrupted(error) = "décodage interrompu\u{a0}: {error}", "decoding interrupted: {error}";

        // Erreurs.
        network(error) = "Réseau\u{a0}: {error}", "Network: {error}";
        http_status(status) = "Erreur HTTP {status}", "HTTP error {status}";
        refused(code) = "Connexion refusée par Discord (code {code}).", "Connection refused by Discord (code {code}).";
        ready_unreadable(error) = "READY illisible\u{a0}: {error}", "Unreadable READY: {error}";
        blocked(time) = "Discord limite temporairement cette connexion, nouvel essai à {time}", "Discord is temporarily limiting this connection, retrying at {time}";
        timeout_reason(until) = "exclusion temporaire {until}", "timed out {until}";
        not_sent(reason) = "Message non envoyé\u{a0}: {reason}", "Message not sent: {reason}";

        // Dates.
        today_at(time) = "Aujourd'hui à {time}", "Today at {time}";
        yesterday_at(time) = "Hier à {time}", "Yesterday at {time}";
        today_at_mid(time) = "aujourd'hui à {time}", "today at {time}";
        date_at(date, time) = "{date} à {time}", "{date} at {time}";
        until_time(time) = "jusqu'à {time}", "until {time}";
        until_date(date, time) = "jusqu'au {date} à {time}", "until {date} at {time}";
        ago(duration) = "il y a {duration}", "{duration} ago";
        in_time(duration) = "dans {duration}", "in {duration}";

        // Discussion.
        not_sent_row(reason) = "Non envoyé ({reason})\u{a0}:", "Not sent ({reason}):";
        message_dm(name) = "Envoyer un message à @{name}", "Message @{name}";
        message_channel(name) = "Envoyer un message dans #{name}", "Message #{name}";
        timed_out_composer(until) = "Exclusion temporaire {until}\u{a0}: vous ne pouvez pas envoyer de messages", "Timed out {until}: you can't send messages";
        compact_author(name) = "{name}\u{a0}:", "{name}:";
        replying_to(name) = "Réponse à {name}", "Replying to {name}";
        reply_author(name) = "@{name}", "@{name}";
        size_bytes(n) = "{n}\u{a0}o", "{n} B";
        size_kb(n) = "{n}\u{a0}Ko", "{n} KB";
        size_mb(n) = "{n}\u{a0}Mo", "{n} MB";
        size_gb(n) = "{n}\u{a0}Go", "{n} GB";

        // Saisie.
        typing_one(name) = "{name} écrit…", "{name} is typing…";
        typing_two(a, b) = "{a} et {b} écrivent…", "{a} and {b} are typing…";
        typing_three(a, b, c) = "{a}, {b} et {c} écrivent…", "{a}, {b} and {c} are typing…";
        slowmode_on(time) = "Mode lent\u{a0}: {time} entre deux messages", "Slowmode: {time} between messages";
        slowmode_left(time) = "Mode lent\u{a0}: encore {time}", "Slowmode: {time} left";
        slowmode_refused(time) = "Mode lent\u{a0}: vous pourrez envoyer un message dans {time}.", "Slowmode: you can send a message in {time}.";
        secs(n) = "{n}\u{a0}s", "{n} s";
        mins_secs(m, s) = "{m}\u{a0}min {s}\u{a0}s", "{m} min {s} s";
        hours_mins(h, m) = "{h}\u{a0}h {m}\u{a0}min", "{h} h {m} min";

        // Profils et présence.
        playing(name) = "Joue à {name}", "Playing {name}";
        streaming(name) = "En direct\u{a0}: {name}", "Streaming {name}";
        listening(name) = "Écoute {name}", "Listening to {name}";
        watching(name) = "Regarde {name}", "Watching {name}";
        competing(name) = "Participe à {name}", "Competing in {name}";
        activity_since(time) = "depuis {time}", "since {time}";
        platform_status(platform, status) = "{platform}\u{a0}: {status}", "{platform}: {status}";
        timed_out_until(until) = "Exclusion temporaire {until}", "Timed out {until}";

        // Liste des membres.
        member_group(name, count) = "{name} — {count}", "{name} — {count}";
        activity_playing(name) = "Joue à {name}", "Playing {name}";
        activity_streaming(name) = "En direct\u{a0}: {name}", "Streaming {name}";
        activity_listening(name) = "Écoute {name}", "Listening to {name}";
        activity_watching(name) = "Regarde {name}", "Watching {name}";
        activity_competing(name) = "Participe à {name}", "Competing in {name}";

        // Panneau Apparence.
        saved_in(path) = "Enregistré dans {path} (relu dès qu'il change).", "Saved to {path} (reloaded whenever it changes).";
        read_from(path) = "Lu depuis {path} (aperçu\u{a0}: rien n'est enregistré).", "Read from {path} (preview: nothing is saved).";
        low_contrast(name, ratio, min) = "Contraste faible\u{a0}: {name} sur le fond principal = {ratio}:1 (au moins {min}:1 conseillé)", "Low contrast: {name} on the main background = {ratio}:1 (at least {min}:1 recommended)";

        // Fichier de thème.
        theme_line(line, message) = "ligne {line}\u{a0}: {message}", "line {line}: {message}";
        theme_unknown_preset(name, list) = "préréglage inconnu «\u{a0}{name}\u{a0}» (attendu\u{a0}: {list})", "unknown preset “{name}” (expected: {list})";
        theme_bad_version(max) = "«\u{a0}version\u{a0}» doit être un entier de 1 à {max}", "“version” must be an integer from 1 to {max}";
        theme_unknown_section(section, list) = "section inconnue «\u{a0}{section}\u{a0}» (attendu\u{a0}: base, {list})", "unknown section “{section}” (expected: base, {list})";
        theme_not_section(section) = "«\u{a0}{section}\u{a0}» doit être une section [{section}]", "“{section}” must be a [{section}] section";
        theme_unknown_key(name, section) = "clé inconnue «\u{a0}{name}\u{a0}» dans [{section}]", "unknown key “{name}” in [{section}]";
        theme_field(section, name, message) = "{section}.{name}\u{a0}: {message}", "{section}.{name}: {message}";
        theme_syntax_detail(detail) = "syntaxe TOML invalide (détail\u{a0}: {detail})", "invalid TOML syntax (detail: {detail})";
        theme_color_count(expected, found) = "nombre de couleurs attendu\u{a0}: {expected}, trouvé\u{a0}: {found}", "number of colors expected: {expected}, found: {found}";
        theme_out_of_range(value, min, max) = "{value} hors bornes (de {min} à {max})", "{value} out of range ({min} to {max})";
        theme_unknown_value(value, list) = "valeur «\u{a0}{value}\u{a0}» inconnue (attendu\u{a0}: {list})", "unknown value “{value}” (expected: {list})";
        theme_number_expected(kind) = "nombre attendu, trouvé\u{a0}: {kind}", "number expected, found: {kind}";
        theme_color_expected(kind) = "couleur \"#rrggbb\" attendue, trouvé\u{a0}: {kind}", "color \"#rrggbb\" expected, found: {kind}";
        theme_bad_color(value) = "couleur illisible «\u{a0}{value}\u{a0}» (attendu \"#rrggbb\" ou \"#rrggbbaa\")", "unreadable color “{value}” (expected \"#rrggbb\" or \"#rrggbbaa\")";
        theme_file_presets(list) = "# Préréglages\u{a0}: {list}.\n\n", "# Presets: {list}.\n\n";
        theme_file_range(detail, min, max) = "# {detail} (de {min} à {max})\n", "# {detail} ({min} to {max})\n";
        theme_file_choices(detail, list) = "# {detail}\u{a0}: {list}\n", "# {detail}: {list}\n";
    }
}

/// Pluriels : `{n}` est le nombre écrit selon la langue.
pub mod p {
    use crate::i18n::{fill::fill, number, Lang};

    plurals! {
        new_messages = ["{n} nouveau message", "{n} nouveaux messages"], ["{n} new message", "{n} new messages"];

        // Durées des horodatages relatifs `<t:…:R>`.
        seconds = ["{n}\u{a0}seconde", "{n}\u{a0}secondes"], ["{n} second", "{n} seconds"];
        minutes = ["{n}\u{a0}minute", "{n}\u{a0}minutes"], ["{n} minute", "{n} minutes"];
        hours = ["{n}\u{a0}heure", "{n}\u{a0}heures"], ["{n} hour", "{n} hours"];
        days = ["{n}\u{a0}jour", "{n}\u{a0}jours"], ["{n} day", "{n} days"];
        months = ["{n}\u{a0}mois", "{n}\u{a0}mois"], ["{n} month", "{n} months"];
        years = ["{n}\u{a0}an", "{n}\u{a0}ans"], ["{n} year", "{n} years"];
        more_reactors = ["et {n} autre", "et {n} autres"], ["and {n} other", "and {n} others"];
        too_long_by = ["{n} caractère de trop", "{n} caractères de trop"], ["{n} character too many", "{n} characters too many"];
    }
}

/// Textes relus mot à mot (comptes, coffre, réglages, fichiers surveillés) :
/// typographie française d'origine gardée (voir `verbatim.rs`).
#[path = "verbatim.rs"]
pub mod verbatim;
/// Fenêtre Paramètres et réglages du compte (fichier à part, mêmes macros).
#[path = "settings.rs"]
pub mod s;
/// Amis, MP de groupe, accusés de lecture.
#[path = "social.rs"]
pub mod social;
/// Envoi de fichiers (lot T8).
#[path = "upload.rs"]
pub mod upload;
/// Notifications de bureau, sons, pastille, zone de notification.
#[path = "notify.rs"]
pub mod notify;
/// Fils de discussion et salons forum (lot T10).
#[path = "threads.rs"]
pub mod threads;

#[path = "profile.rs"]
pub mod profile;
/// Menu d'un utilisateur, demandes de message, « Actifs en ce moment » (lot U).
#[path = "people.rs"]
pub mod people;
/// Carte de mon compte et édition rapide du profil.
#[path = "quick_profile.rs"]
pub mod quick;
/// Sélecteur d'emojis et autocollants.
#[path = "emoji.rs"]
pub mod emoji;
/// Pages Accessibilité, Mode streamer, Avancés, Données, Activité, À propos, envoi d'avatar.
#[path = "settings2.rs"]
pub mod s2;
/// Rail des serveurs, dossiers, rejoindre, créer, inviter (lot S7 + A1).
#[path = "guilds.rs"]
pub mod guilds;
/// Recherche de messages (lot T12).
#[path = "search.rs"]
pub mod search;
/// Sélecteur rapide, boîte de réception, raccourcis clavier (lot navigation).
#[path = "nav.rs"]
pub mod nav;

#[path = "interactions.rs"]
pub mod interactions;
/// Sondages, messages système, annonces, invitations (lot T13).
#[path = "polls.rs"]
pub mod polls;
/// Fenêtre « Paramètres du serveur » (lot A8 + A9).
#[path = "admin.rs"]
pub mod admin;
/// Salons et catégories (lot A2).
#[path = "chadmin.rs"]
pub mod chadmin;
/// Paramètres du serveur : rôles, permissions, membres (lot A3).
#[path = "roles.rs"]
pub mod roles;
/// Modération, journal d'audit, AutoMod, sécurité (lots A4 et A5).
#[path = "moderation.rs"]
pub mod moderation;
/// Événements programmés, accueil des nouveaux membres, pages Communauté (lot A7).
#[path = "events.rs"]
pub mod events;
/// Paramètres du serveur : emojis, autocollants et sons (lot A6).
#[path = "expressions.rs"]
pub mod expressions;

#[path = "voice.rs"]
pub mod voice;
/// Appels privés, conférences, statut des salons vocaux, tableau de sons (lot V6).
#[path = "calls.rs"]
pub mod calls;
/// Périphériques audio et traitement de la voix, page Voix et vidéo (lot V5).
#[path = "audio.rs"]
pub mod audio;
/// Vue d'appel, vidéo et directs (phase 5, lot M4).
#[path = "video.rs"]
pub mod video;
/// Sélecteur de partage : écrans, fenêtres, caméras (lot M3).
#[path = "share.rs"]
pub mod share;
/// Installation, liens, mises à jour et licences (lot C6).
#[path = "install.rs"]
pub mod install;
/// Accessibilité, sélection des messages au clavier, bandeau du mode streamer (lot C5).
#[path = "a11y.rs"]
pub mod a11y;
/// Activité de jeu et présence enrichie (lot C3).
#[path = "activity.rs"]
pub mod activity;
/// Rich Presence locale (lot R).
#[path = "rpc.rs"]
pub mod rpc;
/// Messages vocaux et sons courts (phase 6).
#[path = "voice_msg.rs"]
pub mod voice_msg;
/// Connexion par QR code (phase 6, lot C2).
#[path = "qr.rs"]
pub mod qr;
/// Disposition mobile (phase 7, lot P2).
#[path = "mobile.rs"]
pub mod mobile;
/// Historique hors ligne (lot H).
#[path = "offline.rs"]
pub mod offline;
/// Messages et fil (phase 10, lot M).
#[path = "messages.rs"]
pub mod messages;
/// Sélecteur de GIF, favoris, super réactions (lot G).
#[path = "gifs.rs"]
pub mod gifs;
/// Notifications, raccourcis, « Pour vous » (lot N).
#[path = "lotn.rs"]
pub mod lotn;
/// Vocal et administration (groupe 2, lot V).
#[path = "lotv.rs"]
pub mod lotv;
/// Restes du groupe 3 (lot R).
#[path = "restes.rs"]
pub mod restes;
/// Personnalisation du client (lot PERSO).
#[path = "perso.rs"]
pub mod perso;
/// Fonctions non documentées : effets de profil, quêtes, Orbes (lot ND).
#[path = "nd.rs"]
pub mod nd;
/// Manques fonctionnels de la vérification finale (lot FF).
#[path = "ff.rs"]
pub mod ff;
/// Page « Plugins » et registre des plugins (lot P0).
#[path = "plugins.rs"]
pub mod plug;
/// Plugins locaux : journal, mots-clés, recherche locale, export (lot E1).
#[path = "journal.rs"]
pub mod journal;
/// Plugins vocaux : RNNoise, égaliseur, enregistrement, incrustation (lot E4).
#[path = "e4.rs"]
pub mod e4;
/// Thèmes partageables, Mica, rapport de plantage (lot E5).
#[path = "e5.rs"]
pub mod e5;
/// Plugins « discret et saisie » : mode discret, brouillons, messages programmés, traduction (lot E3).
#[path = "e3.rs"]
pub mod e3;
/// Onglets, vue scindée, fenêtres détachées, palette, zoom par salon (lot E2).
#[path = "e2.rs"]
pub mod e2;
