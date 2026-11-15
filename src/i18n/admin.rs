//! Textes de la fenêtre « Paramètres du serveur » (lot A8 + A9 : vue
//! d'ensemble, webhooks, intégrations, boosts, statistiques, propriété).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        TITLE = "Paramètres du serveur", "Server Settings";
        MENU = "Paramètres du serveur", "Server Settings";
        ALL_PAGES = "Toutes les pages", "All pages";
        GROUP_APPS = "Applications", "Apps";
        GROUP_COMMUNITY = "Communauté", "Community";
        GROUP_MONETIZATION = "Monétisation", "Monetization";
        PAGE_OVERVIEW = "Vue d'ensemble", "Overview";
        PAGE_INTEGRATIONS = "Intégrations", "Integrations";
        PAGE_WEBHOOKS = "Webhooks", "Webhooks";
        PAGE_BOOSTS = "Statut des boosts", "Server Boost Status";
        PAGE_INSIGHTS = "Statistiques du serveur", "Server Insights";
        PAGE_OWNERSHIP = "Propriété et suppression", "Ownership and Deletion";
        LOADING = "Chargement…", "Loading…";
        REFRESH = "Actualiser", "Refresh";
        COPY = "Copier", "Copy";
        COPIED = "Copié.", "Copied.";
        CANCEL = "Annuler", "Cancel";
        REASON = "Raison (journal d'audit, facultative)", "Reason (audit log, optional)";

        // Vue d'ensemble.
        SERVER_NAME = "Nom du serveur", "Server name";
        NAME_INVALID = "Le nom doit compter de 2 à 100 caractères.", "The name must be 2 to 100 characters long.";
        ICON = "Icône du serveur", "Server icon";
        ICON_TIP = "Image carrée, envoyée en 512 × 512", "Square image, sent at 512 × 512";
        BANNER = "Bannière du serveur", "Server banner";
        BANNER_TIP = "Image en 16:9, en haut de la liste des salons", "16:9 image, shown above the channel list";
        BANNER_LOCKED = "Débloquée au niveau 2 de boost", "Unlocked at Boost Level 2";
        SPLASH = "Arrière-plan des invitations", "Invite background";
        SPLASH_TIP = "Image en 16:9 derrière les pages d'invitation", "16:9 image behind invite pages";
        SPLASH_LOCKED = "Débloqué au niveau 1 de boost", "Unlocked at Boost Level 1";
        IMAGE_SET = "Image définie", "Image set";
        IMAGE_NONE = "Aucune image", "No image";
        IMAGE_READY = "Nouvelle image prête, envoyée à l'enregistrement", "New image ready, sent when you save";
        IMAGE_WILL_GO = "Image retirée à l'enregistrement", "Image removed when you save";
        CHANGE_IMAGE = "Changer…", "Change…";
        REMOVE_IMAGE = "Retirer", "Remove";
        USE_CROP = "Utiliser ce cadrage", "Use this crop";
        CROP_TIP = "Faites glisser l'image pour choisir le cadre.", "Drag the image to choose the frame.";
        PICK_TITLE = "Choisir une image", "Choose an image";
        IMAGE_UNREADABLE = "Cette image est illisible.", "This image can't be read.";
        DESCRIPTION = "Description", "Description";
        DESCRIPTION_TIP = "Présentation du serveur (serveurs communautaires et Découverte)", "Server description (Community servers and Discovery)";
        SYSTEM_CHANNEL = "Salon des messages système", "System messages channel";
        SYSTEM_TIP = "Où Discord annonce les arrivées, les boosts et les conseils", "Where Discord posts joins, boosts and tips";
        NO_CHANNEL = "Aucun salon", "No channel";
        SYS_JOIN = "Message de bienvenue aléatoire quand quelqu'un rejoint le serveur", "Send a random welcome message when someone joins this server";
        SYS_JOIN_REPLIES = "Proposer de répondre aux messages de bienvenue avec un autocollant", "Prompt members to reply to welcome messages with a sticker";
        SYS_BOOSTS = "Message quand quelqu'un booste le serveur", "Send a message when someone boosts this server";
        SYS_TIPS = "Conseils utiles pour configurer le serveur", "Send helpful tips for server setup";
        AFK_CHANNEL = "Salon AFK", "Inactive channel";
        AFK_TIP = "Les membres inactifs en vocal y sont déplacés.", "Idle voice members are moved there.";
        AFK_TIMEOUT = "Délai d'inactivité", "Inactive timeout";
        NOTIFS = "Notifications par défaut", "Default notification settings";
        NOTIFS_TIP = "Pour les membres qui n'ont rien réglé eux-mêmes", "For members who have not set their own";
        NOTIFY_ALL = "Tous les messages", "All messages";
        NOTIFY_MENTIONS = "Seulement les @mentions", "Only @mentions";
        LOCALE = "Langue préférée", "Preferred locale";
        LOCALE_TIP = "Langue de la Découverte et des messages de Discord sur ce serveur", "Language used by Discovery and Discord notices for this server";
        VANITY = "Lien d'invitation personnalisé", "Custom invite link";
        VANITY_TIP = "Il se modifie dans l'application officielle.", "It can be changed in the official app.";
        VANITY_LOCKED = "Débloqué au niveau 3 de boost", "Unlocked at Boost Level 3";
        VANITY_NONE = "Aucun lien personnalisé", "No custom invite link";
        UNSAVED = "Attention, des modifications ne sont pas enregistrées.", "Careful, you have unsaved changes.";
        RESET = "Réinitialiser", "Reset";
        SAVE = "Enregistrer les modifications", "Save Changes";
        SAVING = "Enregistrement…", "Saving…";
        SAVED = "Paramètres du serveur enregistrés.", "Server settings saved.";

        // Webhooks.
        WEBHOOKS_TIP = "Les webhooks publient dans vos salons des messages venus d'autres sites.", "Webhooks post messages from other sites into your channels.";
        NEW_WEBHOOK = "Nouveau webhook", "New Webhook";
        NO_WEBHOOKS = "Aucun webhook.", "No webhooks.";
        WEBHOOK_NAME = "Nom", "Name";
        WEBHOOK_CHANNEL = "Salon", "Channel";
        WEBHOOK_AVATAR = "Avatar", "Avatar";
        URL = "Adresse du webhook", "Webhook URL";
        URL_HIDDEN = "Masquée\u{a0}: elle permet à quiconque de publier dans ce salon.", "Hidden: anyone with it can post in this channel.";
        SHOW = "Afficher", "Show";
        HIDE = "Masquer", "Hide";
        COPY_URL = "Copier l'URL", "Copy Webhook URL";
        URL_COPIED = "Adresse du webhook copiée. Gardez-la secrète.", "Webhook URL copied. Keep it secret.";
        SAVE_WEBHOOK = "Enregistrer", "Save";
        EDIT = "Modifier", "Edit";
        DELETE_WEBHOOK = "Supprimer le webhook", "Delete Webhook";
        CONFIRM_DELETE = "Supprimer", "Delete";
        FOLLOWER = "Abonnement à un salon d'annonces", "Channel follower";
        APP_WEBHOOK = "Webhook d'une application", "Application webhook";
        WEBHOOK_CREATED = "Webhook créé.", "Webhook created.";
        WEBHOOK_DELETED = "Webhook supprimé.", "Webhook deleted.";
        ALL_CHANNELS_HOOKS = "Tous les salons", "All channels";

        // Intégrations.
        INTEGRATIONS_TIP = "Robots, applications et comptes liés ajoutés à ce serveur.", "Bots, apps and connected accounts added to this server.";
        BOTS = "Robots et applications", "Bots and Apps";
        LINKED = "Comptes liés", "Connected accounts";
        NO_INTEGRATIONS = "Aucune intégration.", "No integrations.";
        REMOVE = "Retirer", "Remove";
        CONFIRM_REMOVE = "Retirer définitivement", "Remove for good";
        INTEGRATION_REMOVED = "Intégration retirée.", "Integration removed.";
        DISABLED = "désactivée", "disabled";
        COMMAND_PERMS = "Permissions des commandes", "Command permissions";
        SHOW_PERMS = "Voir les permissions des commandes", "View command permissions";
        PERMS_READ_ONLY = "Lecture seule\u{a0}: les modifier demande une autorisation OAuth2 de l'application, à donner dans l'application officielle.", "Read only: changing them needs an OAuth2 grant for the app, given in the official app.";
        NO_OVERRIDES = "Aucune permission particulière\u{a0}: réglages par défaut de l'application.", "No overrides: the app's default settings apply.";
        ALL_COMMANDS = "Toutes les commandes", "All commands";
        ALLOWED = "autorisé", "allowed";
        DENIED = "refusé", "denied";
        EVERYONE = "@everyone", "@everyone";
        ALL_CHANNELS = "Tous les salons", "All channels";

        // Boosts.
        NO_LEVEL = "Aucun niveau", "No level";
        MAX_LEVEL = "Niveau maximal atteint", "Maximum level reached";
        PERKS = "Avantages débloqués", "Unlocked perks";
        NEXT_PERKS = "Au niveau suivant", "At the next level";
        PERK_ANIMATED_ICON = "Icône animée", "Animated server icon";
        PERK_SPLASH = "Arrière-plan des invitations", "Invite background";
        PERK_BANNER = "Bannière du serveur", "Server banner";
        PERK_ROLE_ICONS = "Icônes de rôle", "Role icons";
        PERK_VANITY = "Lien d'invitation personnalisé", "Custom invite link";
        PERK_ANIMATED_BANNER = "Bannière animée", "Animated server banner";
        BOOSTERS = "Boosters connus", "Known boosters";
        BOOSTERS_TIP = "Membres vus avec un boost actif\u{a0}: Discord ne donne pas la liste complète.", "Members seen with an active boost: Discord does not give the full list.";
        NO_BOOSTERS = "Aucun booster connu pour l'instant.", "No known boosters yet.";
        PROGRESS_BAR = "Afficher la barre de progression des boosts", "Show Boost progress bar";
        PROGRESS_BAR_TIP = "Sous le nom du serveur, pour tous les membres", "Under the server name, for every member";
        BOOST = "Booster ce serveur (application officielle)", "Boost This Server (official app)";
        BOOST_TIP = "Les achats se font dans l'application officielle.", "Purchases happen in the official app.";

        // Statistiques, propriété.
        INSIGHTS_TIP = "Discord ne publie pas de route pour les statistiques d'un serveur\u{a0}: elles s'ouvrent dans le navigateur.", "Discord has no public route for server insights: they open in your browser.";
        OPEN_INSIGHTS = "Ouvrir dans le navigateur", "Open in browser";
        TRANSFER = "Transférer la propriété", "Transfer Ownership";
        TRANSFER_TIP = "Exige souvent l'authentification à deux facteurs\u{a0}: à faire dans l'application officielle.", "Often requires two-factor authentication: do it in the official app.";
        DELETE_SERVER = "Supprimer le serveur", "Delete Server";
        DELETE_SERVER_TIP = "Définitif\u{a0}; exige souvent l'authentification à deux facteurs\u{a0}: à faire dans l'application officielle.", "Permanent; often requires two-factor authentication: do it in the official app.";
        OPEN_IN_DISCORD = "Ouvrir dans Discord", "Open in Discord";
        OWNER_ONLY = "Réservé au propriétaire du serveur.", "Only the server owner can do this.";

        // Échecs.
        MFA_REQUIRED = "L'authentification à deux facteurs est requise pour cette action, à faire dans l'application officielle.", "Two-factor authentication is required for this action: do it in the official app.";
        FORBIDDEN = "Vous n'avez pas la permission de faire cela sur ce serveur.", "You don't have permission to do this on this server.";
        NOT_FOUND = "Introuvable (supprimé entre-temps\u{a0}?).", "Not found (deleted in the meantime?).";
        UNREADABLE = "Réponse de Discord illisible.", "Unreadable reply from Discord.";
        ACTION_LOAD = "Chargement du serveur", "Loading the server";
        ACTION_SAVE = "Enregistrement", "Saving";
        ACTION_VANITY = "Lecture du lien personnalisé", "Reading the custom link";
        ACTION_WEBHOOKS = "Lecture des webhooks", "Reading webhooks";
        ACTION_SAVE_WEBHOOK = "Enregistrement du webhook", "Saving the webhook";
        ACTION_DELETE_WEBHOOK = "Suppression du webhook", "Deleting the webhook";
        ACTION_INTEGRATIONS = "Lecture des intégrations", "Reading integrations";
        ACTION_REMOVE_INTEGRATION = "Retrait de l'intégration", "Removing the integration";
        ACTION_COMMAND_PERMS = "Lecture des permissions", "Reading permissions";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        voice_channel(id) = "Salon vocal {id}", "Voice channel {id}";
        afk_minutes(n) = "{n} min", "{n} min";
        afk_hour(n) = "{n} h", "{n} hr";
        vanity_link(code) = "discord.gg/{code}", "discord.gg/{code}";
        delete_webhook_question(name) = "Supprimer le webhook {name}\u{a0}? Les messages déjà publiés restent.", "Delete the webhook {name}? Messages already posted stay.";
        follows(source) = "Suit {source}", "Follows {source}";
        created_by(name) = "Créé par {name}", "Created by {name}";
        added_by(name) = "Ajoutée par {name}", "Added by {name}";
        remove_question(name) = "Retirer {name} du serveur\u{a0}? Son robot quitte le serveur et ses commandes disparaissent.", "Remove {name} from the server? Its bot leaves and its commands disappear.";
        channel_hooks(name) = "Webhooks de #{name}", "Webhooks of #{name}";
        command(name) = "/{name}", "/{name}";
        role(name) = "@{name}", "@{name}";
        channel(name) = "#{name}", "#{name}";
        user(id) = "Membre {id}", "Member {id}";
        level(n) = "Niveau {n}", "Level {n}";
        perk_emoji(n) = "{n} emplacements d'emojis", "{n} emoji slots";
        perk_stickers(n) = "{n} emplacements d'autocollants", "{n} sticker slots";
        perk_soundboard(n) = "{n} sons de soundboard", "{n} soundboard slots";
        perk_audio(n) = "Qualité audio {n} kb/s", "{n} kbps audio quality";
        perk_upload(size) = "Envoi de fichiers jusqu'à {size} pour tous", "{size} upload limit for everyone";
        perk_stream(h, fps) = "Diffusion en {h}p à {fps} images/s", "{h}p {fps} fps streams";
        boosting_since(date) = "depuis le {date}", "since {date}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        boosts = ["{n} boost", "{n} boosts"], ["{n} Boost", "{n} Boosts"];
        boosts_left = ["Encore {n} boost pour le niveau suivant", "Encore {n} boosts pour le niveau suivant"], ["{n} more Boost for the next level", "{n} more Boosts for the next level"];
        members = ["{n} membre", "{n} membres"], ["{n} member", "{n} members"];
        vanity_uses = ["Utilisé {n} fois", "Utilisé {n} fois"], ["Used {n} time", "Used {n} times"];
    }
}
