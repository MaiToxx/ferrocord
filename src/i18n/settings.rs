//! Textes de la fenêtre Paramètres et des réglages du compte (même règles
//! que [`super`] : français puis anglais, ponctuation propre à chaque langue).

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        TITLE = "Paramètres", "Settings";
        SEARCH = "Rechercher un réglage", "Search settings";
        NO_RESULT = "Aucun réglage trouvé", "No setting found";
        ALL_SETTINGS = "Tous les réglages", "All settings";
        ESC_KEY = "ÉCHAP", "ESC";
        LOG_OUT = "Déconnexion", "Log Out";
        GROUP_BILLING = "Facturation", "Billing";
        GROUP_EXPERIENCE = "Expérience", "Experience";
        GROUP_GAMES = "Jeux et applications", "Games & apps";
        // Colonne des catégories, à la façon de Discord (lot réglages).
        SEARCH_HINT = "Rechercher", "Search";
        NAV_ACCOUNT = "Compte", "Account";
        SUB_ACCOUNT_INFO = "Infos du compte", "Account info";
        SUB_PASSWORD = "Mot de passe et sécurité", "Password & security";
        EDIT_PROFILES = "Modifier les profils", "Edit profiles";
        FOOT_LICENSES = "Licences", "Licenses";

        PAGE_ACCOUNT = "Mon compte", "My account";
        PAGE_PROFILE = "Profils", "Profiles";
        PAGE_STATUS = "Statut", "Status";
        PAGE_PRIVACY = "Confidentialité et sécurité", "Privacy & safety";
        PAGE_APPS = "Applications autorisées", "Authorized apps";
        PAGE_DEVICES = "Appareils", "Devices";
        PAGE_CONNECTIONS = "Comptes liés", "Connections";
        PAGE_BILLING = "Nitro et facturation", "Nitro & billing";
        PAGE_TEXT_IMAGES = "Texte et images", "Text & images";
        PAGE_NOTIFICATIONS = "Notifications", "Notifications";
        PAGE_LANGUAGE = "Langue", "Language";
        PAGE_FOLDERS = "Serveurs et dossiers", "Servers & folders";
        PAGE_FERROCORD = "Ferrocord", "Ferrocord";

        // Commun.
        LOADING = "Chargement…", "Loading…";
        SAVING = "Enregistrement…", "Saving…";
        SAVE = "Enregistrer", "Save";
        CANCEL = "Annuler", "Cancel";
        REFRESH = "Actualiser", "Refresh";
        YES = "Oui", "Yes";
        NO = "Non", "No";
        NONE = "Aucun", "None";
        NOTHING = "Rien à afficher.", "Nothing to show.";
        UNKNOWN = "Inconnu", "Unknown";
        DEFAULT = "Par défaut", "Default";
        BUSY = "Un enregistrement de ce réglage est déjà en cours\u{a0}: attendez la réponse de Discord.", "This setting is already being saved: wait for Discord's answer.";
        NOT_LOADED = "Réglages du compte pas encore reçus\u{a0}: rien n'a été modifié.", "Account settings not received yet: nothing was changed.";
        OPEN_IN_DISCORD = "Ouvrir dans Discord", "Open in Discord";
        IN_DISCORD_ONLY = "Dans Discord seulement", "In Discord only";
        IN_DISCORD_ONLY_TIP = "Ces actions demandent votre mot de passe, un code de vérification ou un paiement\u{a0}: Ferrocord ne les fait pas. Le bouton ouvre Discord dans le navigateur (Paramètres utilisateur).", "These actions need your password, a verification code or a payment: Ferrocord doesn't do them. The button opens Discord in the browser (User Settings).";
        NEEDS_PASSWORD = "Demande votre mot de passe\u{a0}: à faire dans Discord.", "Needs your password: do it in Discord.";
        NEEDS_CODE = "Demande un code de vérification\u{a0}: à faire dans Discord.", "Needs a verification code: do it in Discord.";
        SYNCED_TIP = "Réglages de votre compte Discord\u{a0}: ils valent aussi sur vos autres appareils.", "Your Discord account settings: they also apply on your other devices.";
        PROTO_MISSING = "Discord n'a pas envoyé les réglages du compte à la connexion.", "Discord didn't send the account settings when connecting.";
        PROTO_UNREADABLE = "Réglages du compte illisibles\u{a0}: ils sont affichés comme inconnus et Ferrocord ne les modifie pas.", "Unreadable account settings: they're shown as unknown and Ferrocord won't change them.";
        LOAD_FROM_DISCORD = "Les lire depuis Discord", "Read them from Discord";

        // Mon compte.
        USERNAME = "Nom d'utilisateur", "Username";
        USERNAME_TIP = "Identifiant unique du compte", "Unique account identifier";
        DISPLAY_NAME = "Nom d'affichage", "Display name";
        DISPLAY_NAME_TIP = "Nom montré aux autres (32 caractères au plus\u{a0}; vide\u{a0}: nom d'utilisateur)", "Name shown to others (32 characters at most; empty: username)";
        EMAIL = "E-mail", "Email";
        EMAIL_TIP = "Masqué par défaut", "Hidden by default";
        PHONE = "Téléphone", "Phone";
        PHONE_TIP = "Masqué par défaut", "Hidden by default";
        REVEAL = "Afficher", "Reveal";
        HIDE = "Masquer", "Hide";
        VERIFIED = "Vérifié", "Verified";
        VERIFIED_TIP = "Adresse e-mail confirmée", "Email address confirmed";
        MFA = "Authentification à deux facteurs", "Two-factor authentication";
        MFA_TIP = "Code demandé en plus du mot de passe", "Code asked in addition to the password";
        MFA_ON = "Activée", "Enabled";
        MFA_OFF = "Désactivée", "Disabled";
        MFA_MANAGE = "Gérer l'authentification à deux facteurs", "Manage two-factor authentication";
        NITRO = "Nitro", "Nitro";
        NITRO_TIP = "Abonnement du compte", "Account subscription";
        NITRO_NONE = "Sans abonnement", "No subscription";
        NITRO_CLASSIC = "Nitro Classic", "Nitro Classic";
        NITRO_FULL = "Nitro", "Nitro";
        NITRO_BASIC = "Nitro Basic", "Nitro Basic";
        CHANGE_USERNAME = "Changer de nom d'utilisateur", "Change username";
        CHANGE_EMAIL = "Changer d'adresse e-mail", "Change email";
        CHANGE_PASSWORD = "Changer de mot de passe", "Change password";
        DELETE_ACCOUNT = "Désactiver ou supprimer le compte", "Disable or delete account";
        SAVED_ACCOUNTS = "Comptes enregistrés dans Ferrocord", "Accounts saved in Ferrocord";
        SAVED_ACCOUNTS_TIP = "Changer de compte, en ajouter un, se déconnecter\u{a0}: dans le sélecteur de compte (clic sur votre nom, en bas à gauche).", "Switch account, add one, log out: in the account switcher (click your name, bottom left).";
        OPEN_ACCOUNT_SWITCHER = "Ouvrir le sélecteur de compte", "Open the account switcher";

        // Profil.
        PRONOUNS = "Pronoms", "Pronouns";
        PRONOUNS_TIP = "40 caractères au plus", "40 characters at most";
        BIO = "À propos de moi", "About me";
        BIO_TIP = "190 caractères au plus", "190 characters at most";
        ACCENT = "Couleur du profil", "Profile color";
        ACCENT_TIP = "Couleur d'accent de votre profil (case décochée\u{a0}: aucune)", "Accent color of your profile (unchecked: none)";
        AVATAR_BANNER = "Avatar et bannière", "Avatar and banner";
        SERVER_NICK = "Pseudo sur un serveur", "Server nickname";
        SERVER_NICK_TIP = "Nom affiché sur un serveur seulement (vide\u{a0}: votre nom d'affichage). Certains serveurs ne permettent pas de le changer.", "Name shown on one server only (empty: your display name). Some servers don't allow changing it.";
        CHOOSE_SERVER = "Choisir un serveur", "Choose a server";
        NICK_HINT = "Pseudo", "Nickname";

        // Statut.
        ONLINE_STATUS = "Statut", "Status";
        ONLINE_STATUS_TIP = "Ce que les autres voient à côté de votre nom", "What others see next to your name";
        STATUS_ONLINE = "En ligne", "Online";
        STATUS_IDLE = "Inactif", "Idle";
        STATUS_DND = "Ne pas déranger", "Do Not Disturb";
        STATUS_INVISIBLE = "Invisible", "Invisible";
        SHOW_GAME = "Afficher l'activité en cours", "Share your activity";
        SHOW_GAME_TIP = "Montrer le jeu ou l'application en cours dans votre statut", "Show the game or app you're using in your status";
        CUSTOM_STATUS = "Statut personnalisé", "Custom status";
        CUSTOM_STATUS_NONE = "Aucun statut personnalisé.", "No custom status.";
        CUSTOM_STATUS_EXPIRED = "Le statut personnalisé a expiré.", "The custom status has expired.";
        CUSTOM_STATUS_TEXT = "Texte", "Text";
        CUSTOM_STATUS_TIP = "128 caractères au plus", "128 characters at most";
        CUSTOM_STATUS_EMOJI = "Emoji", "Emoji";
        CUSTOM_STATUS_EMOJI_TIP = "Un emoji Unicode, collé tel quel (facultatif)", "A Unicode emoji, pasted as is (optional)";
        CUSTOM_STATUS_EXPIRY = "Effacer après", "Clear after";
        CUSTOM_STATUS_EXPIRY_TIP = "Le statut disparaît tout seul après ce délai", "The status disappears by itself after this delay";
        EXPIRE_NEVER = "Ne pas effacer", "Don't clear";
        EXPIRE_30M = "30 minutes", "30 minutes";
        EXPIRE_1H = "1 heure", "1 hour";
        EXPIRE_4H = "4 heures", "4 hours";
        EXPIRE_24H = "24 heures", "24 hours";
        CLEAR_STATUS = "Retirer le statut", "Clear status";

        // Confidentialité.
        SECTION_DMS = "Messages privés", "Direct messages";
        GUILD_DMS = "MP des membres des serveurs", "DMs from server members";
        GUILD_DMS_TIP = "Autoriser par défaut les messages privés des membres des serveurs que vous rejoignez", "Allow direct messages from members of servers you join, by default";
        MESSAGE_REQUESTS = "Restreindre les demandes de message", "Restrict message requests";
        MESSAGE_REQUESTS_TIP = "Réglage par défaut des demandes de message venant de membres des serveurs", "Default setting for message requests from server members";
        SPAM_FILTER = "Filtre des MP indésirables", "Spam DM filter";
        SPAM_FILTER_TIP = "Messages privés suspects rangés à part", "Suspicious direct messages filed separately";
        SPAM_OFF = "Désactivé", "Off";
        SPAM_NON_FRIENDS = "Personnes hors de mes amis", "People who aren't my friends";
        SPAM_ALL = "Tout le monde", "Everyone";
        FRIEND_REQUESTS = "Qui peut vous demander en ami", "Who can add you as a friend";
        FRIEND_REQUESTS_TIP = "«\u{a0}Tout le monde\u{a0}» inclut les deux autres.", "“Everyone” includes the other two.";
        FRIENDS_EVERYONE = "Tout le monde", "Everyone";
        FRIENDS_EVERYONE_TIP = "Toute personne, même sans ami ni serveur en commun", "Anyone, even without a friend or server in common";
        FRIENDS_OF_FRIENDS = "Amis d'amis", "Friends of friends";
        FRIENDS_OF_FRIENDS_TIP = "Personnes ayant un ami en commun avec vous", "People with a friend in common with you";
        FRIENDS_SERVER_MEMBERS = "Membres des serveurs", "Server members";
        FRIENDS_SERVER_MEMBERS_TIP = "Personnes ayant un serveur en commun avec vous", "People with a server in common with you";
        EXPLICIT_FILTER = "Filtre de contenu explicite", "Explicit content filter";
        EXPLICIT_FILTER_TIP = "Ce que Discord fait des images explicites qu'il détecte. «\u{a0}Par défaut\u{a0}»\u{a0}: bloquées dans les MP d'inconnus, affichées ailleurs.", "What Discord does with explicit images it detects. “Default”: blocked in DMs from strangers, shown elsewhere.";
        EXPLICIT_FRIENDS = "MP de mes amis", "DMs from friends";
        EXPLICIT_FRIENDS_TIP = "Messages privés de vos amis", "Direct messages from your friends";
        EXPLICIT_OTHERS = "MP d'autres personnes", "DMs from others";
        EXPLICIT_OTHERS_TIP = "Messages privés de personnes hors de vos amis", "Direct messages from people who aren't your friends";
        EXPLICIT_GUILDS = "Salons des serveurs", "Server channels";
        EXPLICIT_GUILDS_TIP = "Messages des salons de serveurs", "Messages in server channels";
        REDACT_SHOW = "Afficher", "Show";
        REDACT_BLUR = "Flouter", "Blur";
        REDACT_BLOCK = "Bloquer", "Block";

        // Applications, appareils, comptes liés.
        APPS_TIP = "Applications et robots autorisés à accéder à votre compte. Révoquer leur retire cet accès.", "Apps and bots allowed to access your account. Revoking removes that access.";
        REVOKE = "Révoquer", "Revoke";
        DEVICES_TIP = "Sessions ouvertes sur votre compte. Ferrocord ne sait pas laquelle est la sienne\u{a0}: déconnecter celle de Ferrocord vous ramène à l'écran de connexion.", "Sessions logged in to your account. Ferrocord can't tell which one is its own: logging out Ferrocord's brings you back to the login screen.";
        LOGOUT_SESSION = "Déconnecter", "Log out";
        LOGOUT_SESSION_CONFIRM = "Déconnecter cet appareil\u{a0}?", "Log out this device?";
        CONNECTIONS_TIP = "Comptes d'autres services liés à votre compte Discord (lecture seule).", "Accounts from other services linked to your Discord account (read-only).";
        ON_PROFILE = "affiché sur le profil", "shown on profile";
        HIDDEN_FROM_PROFILE = "masqué du profil", "hidden from profile";
        REVOKED = "révoqué", "revoked";
        MANAGE_CONNECTIONS = "Lier ou délier un compte", "Link or unlink an account";
        MANAGE_CONNECTIONS_TIP = "Demande une connexion au service concerné\u{a0}: à faire dans Discord.", "Needs signing in to that service: do it in Discord.";

        // Facturation.
        BILLING = "Facturation", "Billing";
        BILLING_TIP = "Achats et abonnements ne se font pas dans Ferrocord\u{a0}: les avantages Nitro déjà acquis restent valables.", "Purchases and subscriptions aren't made in Ferrocord: Nitro perks you already have still apply.";
        NITRO_STATE = "Abonnement", "Subscription";
        NITRO_STATE_TIP = "Ce que le profil du compte indique. Ferrocord ne vend ni ne modifie aucun abonnement.", "What the account profile says. Ferrocord neither sells nor changes any subscription.";
        NITRO_UNKNOWN = "Pas encore connu", "Not known yet";
        NITRO_MANAGE_TIP = "S'abonner, changer ou résilier\u{a0}: dans Discord.", "Subscribe, change or cancel: in Discord.";
        BILLING_MANAGE_TIP = "Moyens de paiement et historique\u{a0}: dans Discord.", "Payment methods and history: in Discord.";
        GIFTS = "Cadeaux et boutique", "Gifts and shop";
        GIFTS_TIP = "Offrir, utiliser un code, boutique\u{a0}: dans Discord.", "Gift, redeem a code, shop: in Discord.";

        // Apparence de Discord.
        DISCORD_THEME_SECTION = "Apparence de votre compte Discord", "Your Discord account's appearance";
        DISCORD_THEME = "Thème de Discord", "Discord theme";
        DISCORD_THEME_TIP = "Thème choisi dans l'application officielle (lecture seule)", "Theme chosen in the official app (read-only)";
        THEME_DARK = "Sombre", "Dark";
        THEME_LIGHT = "Clair", "Light";
        THEME_DARKER = "Plus sombre", "Darker";
        THEME_MIDNIGHT = "Minuit", "Midnight";
        COMPACT = "Mode compact", "Compact mode";
        COMPACT_TIP = "Messages sur une ligne dans l'application officielle (lecture seule)", "One-line messages in the official app (read-only)";
        APPLY_TO_FERROCORD = "Appliquer à Ferrocord", "Apply to Ferrocord";

        // Texte et images.
        SECTION_MEDIA = "Images et vidéos", "Images and videos";
        INLINE_ATTACHMENTS = "Afficher les pièces jointes", "Show attachments";
        INLINE_ATTACHMENTS_TIP = "Images et vidéos envoyées directement dans la discussion", "Images and videos posted directly in chat";
        INLINE_EMBEDS = "Afficher les images des liens", "Show images from links";
        INLINE_EMBEDS_TIP = "Images et vidéos des liens collés dans la discussion", "Images and videos from links posted in chat";
        IMAGE_DESCRIPTIONS = "Afficher les descriptions d'images", "Show image descriptions";
        IMAGE_DESCRIPTIONS_TIP = "Texte de remplacement des images, quand il existe", "Alternative text of images, when there is one";
        SECTION_EMBEDS = "Aperçus de liens", "Link previews";
        RENDER_EMBEDS = "Afficher les aperçus de liens", "Show link previews";
        RENDER_EMBEDS_TIP = "Titre et résumé des sites dont un lien est collé", "Title and summary of linked websites";
        SECTION_EMOJI = "Emojis et réactions", "Emoji and reactions";
        RENDER_REACTIONS = "Afficher les réactions", "Show reactions";
        RENDER_REACTIONS_TIP = "Réactions emoji sous les messages", "Emoji reactions under messages";
        ANIMATE_EMOJI = "Animer les emojis", "Animate emoji";
        ANIMATE_EMOJI_TIP = "Emojis animés en mouvement", "Animated emoji play";
        GIF_AUTOPLAY = "Lire les GIF automatiquement", "Autoplay GIFs";
        GIF_AUTOPLAY_TIP = "Quand la fenêtre est active", "When the window is focused";
        CONVERT_EMOTICONS = "Convertir les émoticônes", "Convert emoticons";
        CONVERT_EMOTICONS_TIP = "Par exemple, «\u{a0}:)\u{a0}» devient un emoji", "For example, “:)” becomes an emoji";
        ANIMATE_STICKERS = "Animer les autocollants", "Animate stickers";
        ANIMATE_STICKERS_TIP = "Quand les autocollants animés bougent", "When animated stickers play";
        STICKERS_ALWAYS = "Toujours", "Always";
        STICKERS_HOVER = "Au survol", "On hover";
        STICKERS_NEVER = "Jamais", "Never";
        SECTION_COMMANDS = "Commandes", "Commands";
        COMMAND_SUGGESTIONS = "Suggérer les commandes", "Suggest commands";
        COMMAND_SUGGESTIONS_TIP = "Proposer les commandes des applications pendant la saisie", "Suggest app commands while typing";
        TTS = "Commande /tts", "/tts command";
        TTS_TIP = "Lire à voix haute les messages envoyés avec /tts", "Read aloud messages sent with /tts";

        // Notifications.
        SERVER_NOTIFICATIONS = "Par serveur et par salon", "Per server and channel";
        SERVER_NOTIFICATIONS_TIP = "Clic droit sur un serveur, un salon ou le bouton des messages privés\u{a0}: rendre muet, niveau de notification, @everyone. Un salon muet n'apparaît plus comme non lu, sauf si l'on vous mentionne.", "Right-click a server, a channel or the direct messages button: mute, notification level, @everyone. A muted channel no longer shows as unread, unless you're mentioned.";
        SECTION_ACCOUNT_NOTIFICATIONS = "Notifications du compte", "Account notifications";
        IN_APP_NOTIFICATIONS = "Notifications dans l'application", "In-app notifications";
        IN_APP_NOTIFICATIONS_TIP = "Bandeaux affichés dans l'application officielle", "Banners shown in the official app";
        BURST_REACTIONS = "Super Réactions", "Super Reactions";
        BURST_REACTIONS_TIP = "Être prévenu des Super Réactions à vos messages", "Be notified of Super Reactions to your messages";
        GO_LIVE = "Prévenir mes amis de mes directs", "Notify friends when I go live";
        GO_LIVE_TIP = "Vos amis sont prévenus quand vous lancez un direct", "Your friends are notified when you start streaming";
        NOTIFY_LEVEL = "Notifications", "Notifications";
        LEVEL_ALL = "Tous les messages", "All messages";
        LEVEL_MENTIONS = "Mentions seulement", "Only @mentions";
        LEVEL_NOTHING = "Rien", "Nothing";
        LEVEL_INHERIT = "Comme le serveur", "Use server default";
        SUPPRESS_EVERYONE = "Ignorer @everyone et @here", "Suppress @everyone and @here";
        SUPPRESS_ROLES = "Ignorer les mentions de rôles", "Suppress all role @mentions";
        MUTE_SERVER = "Rendre le serveur muet", "Mute server";
        MUTE_CHANNEL = "Rendre le salon muet", "Mute channel";
        MUTE_DMS = "Rendre les MP muets", "Mute direct messages";
        UNMUTE = "Réactiver", "Unmute";
        MUTE_15M = "Pendant 15 minutes", "For 15 minutes";
        MUTE_1H = "Pendant 1 heure", "For 1 hour";
        MUTE_3H = "Pendant 3 heures", "For 3 hours";
        MUTE_8H = "Pendant 8 heures", "For 8 hours";
        MUTE_24H = "Pendant 24 heures", "For 24 hours";
        MUTE_FOREVER = "Jusqu'à ce que je le réactive", "Until I turn it back on";

        // Langue.
        ACCOUNT_LOCALE = "Langue du compte Discord", "Discord account language";
        ACCOUNT_LOCALE_TIP = "Langue des e-mails de Discord et de l'application officielle (la langue de Ferrocord se règle au-dessus).", "Language of Discord's emails and of the official app (Ferrocord's language is set above).";

        // Dossiers.
        FOLDERS_TIP = "Ordre des serveurs et dossiers de votre compte (lecture seule).", "Server order and folders of your account (read-only).";
        UNNAMED_FOLDER = "Dossier sans nom", "Unnamed folder";

        // Ferrocord.
        VERSION = "Version", "Version";
        VERSION_TIP = "Version de Ferrocord", "Ferrocord version";
        UNOFFICIAL = "Client non officiel", "Unofficial client";
        UNOFFICIAL_TIP = "Utiliser un client tiers avec un compte utilisateur enfreint les conditions d'utilisation de Discord et peut entraîner la suspension du compte. Chaque réglage modifié ici passe par des points d'API non documentés.", "Using a third-party client with a user account breaks Discord's terms of service and can get the account suspended. Each setting changed here goes through undocumented API endpoints.";

        // Notifications passagères.
        GEAR_TIP = "Paramètres (Ctrl+, ouvre l'apparence)", "Settings (Ctrl+, opens appearance)";
        OFFLINE = "Connectez-vous à un compte pour voir ces réglages.", "Log in to an account to see these settings.";
        SETTING_OUTDATED = "Réglage modifié ailleurs entre-temps\u{a0}: rien n'a été changé, les valeurs affichées sont à jour.", "Setting changed elsewhere in the meantime: nothing was changed, the values shown are up to date.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        load_failed(reason) = "Lecture impossible\u{a0}: {reason}", "Couldn't load: {reason}";
        setting_not_saved(reason) = "Réglage non enregistré, valeur rétablie\u{a0}: {reason}", "Setting not saved, previous value restored: {reason}";
        session_seen(place, when) = "{place}, vue le {when}", "{place}, last seen {when}";
        scopes(list) = "Accès\u{a0}: {list}", "Access: {list}";
        revoke_confirm(name) = "Révoquer l'accès de {name}\u{a0}?", "Revoke {name}'s access?";
        custom_status_until(when) = "Le statut s'effacera {when}.", "The status will clear {when}.";
        unmute_until(when) = "Réactiver (muet {when})", "Unmute (muted {when})";
        unknown_server(id) = "Serveur {id}", "Server {id}";
        apply_theme_tip(preset) = "Remplace le thème de Ferrocord par le préréglage «\u{a0}{preset}\u{a0}» avec le même mode de messages.", "Replaces Ferrocord's theme with the “{preset}” preset using the same message mode.";
    }
}
