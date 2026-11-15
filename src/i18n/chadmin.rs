//! Textes des salons et catégories (lot A2) : créer, modifier, permissions,
//! cloner, supprimer, déplacer ; fenêtre « Paramètres du serveur », page
//! Salons. Module enfant de `table` (il en reprend les macros), tenu à part
//! pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        COLLAPSE_CATEGORY = "Replier la catégorie", "Collapse Category";
        EXPAND_CATEGORY = "Déplier la catégorie", "Expand Category";
        COLLAPSE_ALL = "Replier toutes les catégories", "Collapse All Categories";
        HIDE_MUTED = "Masquer les salons muets", "Hide Muted Channels";
        OPEN_CHAT = "Ouvrir la discussion", "Open Chat";
        CREATE_INVITE = "Créer une invitation", "Create Invite";
        THREADS_TIP = "Fils", "Threads";
        NOTIF_TIP = "Paramètres de notification", "Notification Settings";
        TOPIC_TIP = "Afficher le sujet en entier", "Show the full topic";
        BITRATE = "Débit binaire", "Bitrate";
        BITRATE_TIP = "Au-delà de 64\u{a0}kbps, les personnes dont la connexion est lente peuvent être gênées.", "Going above 64kbps may adversely affect people on poor connections.";
        USER_LIMIT = "Limite d'utilisateurs", "User Limit";
        USER_LIMIT_TIP = "Nombre de personnes qui peuvent se connecter à ce salon vocal. Ceux qui peuvent déplacer des membres passent outre.", "Limits the number of users that can connect to this voice channel. Users with the Move Members permission ignore this limit.";
        NO_LIMIT = "Aucune limite", "No limit";
        REGION = "Région imposée", "Region Override";
        REGION_AUTO = "Automatique", "Automatic";
        REGION_TIP = "Tous les participants seront connectés à cette région, où qu'ils se trouvent.", "Everyone in this channel will be connected to this region, wherever they are.";
        VIDEO_QUALITY = "Qualité vidéo", "Video Quality";
        VIDEO_AUTO = "Automatique", "Auto";
        VIDEO_FULL = "720p", "720p";
        VIDEO_QUALITY_TIP = "Qualité de la caméra pour tous les participants. «\u{a0}Automatique\u{a0}» l'adapte à la connexion de chacun.", "Sets camera video quality for all channel participants. Auto adapts it to each connection.";
        ACTION_REGIONS = "Lecture des régions vocales", "Loading voice regions";
        EDIT_CHANNEL = "Modifier le salon", "Edit Channel";
        EDIT_CATEGORY = "Modifier la catégorie", "Edit Category";
        CREATE_CHANNEL = "Créer un salon", "Create Channel";
        CREATE_CATEGORY = "Créer une catégorie", "Create Category";
        CLONE_CHANNEL = "Cloner le salon", "Clone Channel";
        CLONE = "Cloner", "Clone";
        CLONE_TIP = "Le nouveau salon reprendra le type, la catégorie, les permissions, le mode lent, le sujet et la limite d'âge de celui-ci.", "The new channel will have the same type, category, permissions, slowmode, topic and age restriction as this one.";
        COPY_LINK = "Copier le lien", "Copy Link";
        LINK_COPIED = "Lien copié.", "Link copied.";
        DELETE_CHANNEL = "Supprimer le salon", "Delete Channel";
        DELETE_CATEGORY = "Supprimer la catégorie", "Delete Category";
        DELETE = "Supprimer", "Delete";
        DELETE_IRREVERSIBLE = "Cette action est irréversible.", "This cannot be undone.";
        DELETE_CATEGORY_NOTE = "Les salons de cette catégorie seront gardés, sans catégorie.", "The channels in this category will be kept, without a category.";
        TYPE_NAME = "Saisissez le nom pour confirmer", "Type the name to confirm";
        CANCEL = "Annuler", "Cancel";
        CREATE = "Créer", "Create";
        PAGE_OVERVIEW = "Vue d'ensemble", "Overview";
        PAGE_PERMISSIONS = "Permissions", "Permissions";
        PAGE_INVITES = "Invitations", "Invites";
        PAGE_INTEGRATIONS = "Intégrations", "Integrations";
        ALL_PAGES = "Toutes les pages", "All pages";
        CHANNEL_NAME = "Nom du salon", "Channel Name";
        CATEGORY_NAME = "Nom de la catégorie", "Category Name";
        TOPIC = "Sujet du salon", "Channel Topic";
        TOPIC_HINT = "Dites à tout le monde comment utiliser ce salon\u{a0}!", "Let everyone know how to use this channel!";
        GUIDELINES = "Consignes de publication", "Post Guidelines";
        SLOWMODE = "Mode lent", "Slowmode";
        SLOWMODE_TIP = "Les membres ne pourront envoyer qu'un message, ou créer qu'un fil, par intervalle, sauf s'ils peuvent gérer le salon ou les messages.", "Members will be restricted to sending one message and creating one thread per this interval, unless they have Manage Channel or Manage Messages permissions.";
        OFF = "Désactivé", "Off";
        NSFW = "Salon soumis à une limite d'âge", "Age-Restricted Channel";
        NSFW_TIP = "Les membres devront confirmer qu'ils sont majeurs pour voir le contenu de ce salon.", "Users will need to confirm they are of over legal age to view the content in this channel.";
        ANNOUNCEMENT = "Salon des annonces", "Announcement Channel";
        ANNOUNCEMENT_TIP = "D'autres serveurs pourront suivre ce salon et recevoir ses publications. Réservé aux serveurs Communauté.", "Other servers can follow this channel and receive its posts. Community servers only.";
        ARCHIVE = "Masquer après inactivité", "Hide After Inactivity";
        ARCHIVE_TIP = "Les nouveaux fils ne s'afficheront plus dans la liste après cette durée sans activité.", "New threads will stop showing in the channel list after this period of inactivity.";
        TAGS = "Étiquettes", "Tags";
        TAGS_TIP = "Les étiquettes aident à trier les publications (20 au plus). Une étiquette réservée aux modérateurs ne peut être posée que par ceux qui gèrent les fils.", "Tags help sort posts (20 at most). A moderated tag can only be applied by members who can manage threads.";
        TAG_NAME = "Nom de l'étiquette", "Tag name";
        TAG_EMOJI = "Emoji", "Emoji";
        TAG_MODERATED = "Modérateurs seulement", "Moderators only";
        ADD_TAG = "Ajouter une étiquette", "Add Tag";
        REMOVE = "Retirer", "Remove";
        DEFAULT_REACTION = "Réaction par défaut", "Default Reaction";
        DEFAULT_REACTION_TIP = "Emoji proposé sous chaque publication (un emoji Unicode\u{a0}; vide\u{a0}: aucun).", "Emoji shown under each post (a Unicode emoji; empty: none).";
        CUSTOM_EMOJI = "Emoji personnalisé du serveur", "Server custom emoji";
        UNSAVED = "Attention, il reste des modifications non enregistrées\u{a0}!", "Careful — you have unsaved changes!";
        RESET = "Réinitialiser", "Reset";
        SAVE = "Enregistrer les modifications", "Save Changes";
        SAVED = "Modifications enregistrées.", "Changes saved.";
        LOADING = "Chargement…", "Loading…";
        RETRY = "Réessayer", "Try again";
        SYNC_NOW = "Synchroniser maintenant", "Sync Now";
        ROLES_MEMBERS = "Rôles et membres", "Roles/Members";
        ADD_ROLE_MEMBER = "Ajouter un rôle ou un membre", "Add a role or member";
        SEARCH_HINT = "Rechercher un rôle ou un membre", "Search roles or members";
        ROLES = "Rôles", "Roles";
        MEMBERS = "Membres", "Members";
        MEMBERS_NOTE = "Membres affichés dans la liste des membres de ce serveur.", "Members shown in this server's member list.";
        NO_MATCH = "Aucun résultat.", "No results.";
        REMOVE_OVERWRITE = "Retirer ce rôle ou ce membre", "Remove this role or member";
        ALLOW = "Autoriser", "Allow";
        NEUTRAL = "Neutre (hériter)", "Neutral (inherit)";
        DENY = "Refuser", "Deny";
        OVERWRITES_UNKNOWN = "Les permissions de ce salon n'ont pas pu être lues\u{a0}: modifiez-les dans l'application officielle.", "This channel's permissions couldn't be read: edit them in the official app.";
        NO_MANAGE_ROLES = "Il vous faut la permission «\u{a0}Gérer les permissions\u{a0}» pour modifier les permissions de ce salon.", "You need the Manage Permissions permission to edit this channel's permissions.";
        LOSE_ACCESS = "Avec ces réglages, vous perdrez l'accès à ce salon (le voir ou en gérer les permissions).", "With these settings you will lose access to this channel (seeing it or managing its permissions).";
        SAVE_ANYWAY = "Enregistrer quand même", "Save Anyway";
        INVITES_TIP = "Créez une invitation vers ce salon, ou voyez et révoquez les invitations du serveur.", "Create an invite to this channel, or see and revoke the server's invites.";
        INVITE_PEOPLE = "Inviter des personnes", "Invite People";
        SERVER_INVITES = "Invitations du serveur", "Server Invites";
        CHANNEL_TYPE = "Type de salon", "Channel Type";
        KIND_TEXT = "Texte", "Text";
        KIND_TEXT_TIP = "Envoyez des messages, des images, des GIF, des emojis, des avis et des blagues", "Send messages, images, GIFs, emoji, opinions, and puns";
        KIND_VOICE = "Vocal", "Voice";
        KIND_VOICE_TIP = "Discutez ensemble en vocal, en vidéo ou en partage d'écran", "Hang out together with voice, video, and screen share";
        KIND_FORUM = "Forum", "Forum";
        KIND_FORUM_TIP = "Créez un espace propice aux discussions organisées", "Create a space for organized discussions";
        KIND_ANNOUNCEMENT = "Annonces", "Announcement";
        KIND_ANNOUNCEMENT_TIP = "Informations importantes que d'autres serveurs peuvent suivre (serveurs Communauté)", "Important updates that other servers can follow (Community servers)";
        KIND_STAGE = "Scène", "Stage";
        KIND_STAGE_TIP = "Événements audio avec un public (serveurs Communauté)", "Audio events with an audience (Community servers)";
        KIND_MEDIA = "Média", "Media";
        KIND_MEDIA_TIP = "Publications d'images et de vidéos (serveurs Communauté)", "Image and video posts (Community servers)";
        PRIVATE_CHANNEL = "Salon privé", "Private Channel";
        PRIVATE_CATEGORY = "Catégorie privée", "Private Category";
        PRIVATE_TIP = "Seuls les rôles choisis (et vous) pourront le voir.", "Only selected roles (and you) will be able to view it.";
        WHO_CAN_SEE = "Qui peut y accéder\u{a0}?", "Who can access it?";
        NO_ROLES = "Ce serveur n'a pas d'autre rôle que @everyone.", "This server has no role other than @everyone.";
        ACTION_LOAD = "Lecture du salon", "Loading the channel";
        ACTION_CREATE = "Création du salon", "Creating the channel";
        ACTION_SAVE = "Enregistrement", "Saving";
        ACTION_DELETE = "Suppression", "Deleting";
        ACTION_MOVE = "Déplacement", "Moving";
        NOT_FOUND = "salon introuvable (supprimé entre-temps\u{a0}?)", "channel not found (deleted in the meantime?)";
        PAGE_CHANNELS = "Salons", "Channels";
        CHANNELS_TIP = "Glissez-déposez les salons et les catégories dans la liste des salons pour les réordonner.", "Drag and drop channels and categories in the channel list to reorder them.";
        EDIT = "Modifier", "Edit";
        NO_CATEGORY = "Sans catégorie", "No category";
        EVERYONE = "@everyone", "@everyone";
        GROUP_GENERAL = "Permissions générales du salon", "General Channel Permissions";
        GROUP_MEMBERSHIP = "Permissions d'adhésion", "Membership Permissions";
        GROUP_TEXT = "Permissions des salons textuels", "Text Channel Permissions";
        GROUP_VOICE = "Permissions des salons vocaux", "Voice Channel Permissions";
        GROUP_STAGE = "Permissions des salons de scène", "Stage Channel Permissions";
        GROUP_EVENTS = "Permissions des événements", "Events Permissions";
        GROUP_APPS = "Permissions des applications", "Apps Permissions";
        P_VIEW_CHANNEL = "Voir le salon", "View Channel";
        D_VIEW_CHANNEL = "Permet aux membres de voir ce salon. Le refuser à @everyone le rend privé.", "Allows members to view this channel. Denying it to @everyone makes it private.";
        P_MANAGE_CHANNELS = "Gérer le salon", "Manage Channel";
        D_MANAGE_CHANNELS = "Permet de changer le nom, le sujet et les réglages du salon, et de le supprimer.", "Allows members to change this channel's name, topic and settings, and to delete it.";
        P_MANAGE_ROLES = "Gérer les permissions", "Manage Permissions";
        D_MANAGE_ROLES = "Permet de changer les permissions de ce salon.", "Allows members to change this channel's permissions.";
        P_MANAGE_WEBHOOKS = "Gérer les webhooks", "Manage Webhooks";
        D_MANAGE_WEBHOOKS = "Permet de créer, modifier et supprimer des webhooks, qui publient des messages d'autres applications.", "Allows members to create, edit and delete webhooks, which post messages from other apps.";
        P_CREATE_INSTANT_INVITE = "Créer une invitation", "Create Invite";
        D_CREATE_INSTANT_INVITE = "Permet d'inviter de nouvelles personnes sur ce serveur.", "Allows members to invite new people to this server.";
        P_SEND_MESSAGES = "Envoyer des messages et créer des publications", "Send Messages and Create Posts";
        D_SEND_MESSAGES = "Permet d'envoyer des messages dans ce salon, et de créer des publications dans un forum.", "Allows members to send messages in this channel and create posts in a forum.";
        P_SEND_MESSAGES_IN_THREADS = "Envoyer des messages dans les fils et les publications", "Send Messages in Threads and Posts";
        D_SEND_MESSAGES_IN_THREADS = "Permet de répondre dans les fils et les publications.", "Allows members to reply in threads and posts.";
        P_CREATE_PUBLIC_THREADS = "Créer des fils publics", "Create Public Threads";
        D_CREATE_PUBLIC_THREADS = "Permet de créer des fils que tout le salon peut voir.", "Allows members to create threads everyone in the channel can view.";
        P_CREATE_PRIVATE_THREADS = "Créer des fils privés", "Create Private Threads";
        D_CREATE_PRIVATE_THREADS = "Permet de créer des fils sur invitation.", "Allows members to create invite-only threads.";
        P_EMBED_LINKS = "Intégrer des liens", "Embed Links";
        D_EMBED_LINKS = "Permet d'afficher un aperçu des liens envoyés.", "Allows links that members share to show embedded content.";
        P_ATTACH_FILES = "Joindre des fichiers", "Attach Files";
        D_ATTACH_FILES = "Permet d'envoyer des fichiers et des médias.", "Allows members to upload files or media.";
        P_ADD_REACTIONS = "Ajouter des réactions", "Add Reactions";
        D_ADD_REACTIONS = "Permet d'ajouter de nouvelles réactions aux messages (répondre à une réaction existante reste permis).", "Allows members to add new reactions to messages (reacting with an existing one is still allowed).";
        P_USE_EXTERNAL_EMOJIS = "Utiliser des emojis externes", "Use External Emoji";
        D_USE_EXTERNAL_EMOJIS = "Permet d'utiliser les emojis d'autres serveurs.", "Allows members to use emoji from other servers.";
        P_USE_EXTERNAL_STICKERS = "Utiliser des autocollants externes", "Use External Stickers";
        D_USE_EXTERNAL_STICKERS = "Permet d'utiliser les autocollants d'autres serveurs.", "Allows members to use stickers from other servers.";
        P_MENTION_EVERYONE = "Mentionner @everyone, @here et tous les rôles", "Mention @everyone, @here and All Roles";
        D_MENTION_EVERYONE = "Permet de notifier tout le monde, les membres en ligne, et n'importe quel rôle.", "Allows members to notify everyone, online members, and any role.";
        P_MANAGE_MESSAGES = "Gérer les messages", "Manage Messages";
        D_MANAGE_MESSAGES = "Permet de supprimer les messages des autres et d'en retirer les aperçus.", "Allows members to delete messages by other members and remove their embeds.";
        P_PIN_MESSAGES = "Épingler des messages", "Pin Messages";
        D_PIN_MESSAGES = "Permet d'épingler et de désépingler des messages.", "Allows members to pin and unpin messages.";
        P_BYPASS_SLOWMODE = "Ignorer le mode lent", "Bypass Slowmode";
        D_BYPASS_SLOWMODE = "Permet d'envoyer des messages sans attendre la fin du mode lent.", "Allows members to send messages without waiting for slowmode.";
        P_MANAGE_THREADS = "Gérer les fils et les publications", "Manage Threads and Posts";
        D_MANAGE_THREADS = "Permet de renommer, verrouiller, archiver et supprimer les fils et les publications.", "Allows members to rename, lock, archive and delete threads and posts.";
        P_READ_MESSAGE_HISTORY = "Voir les anciens messages", "Read Message History";
        D_READ_MESSAGE_HISTORY = "Permet de lire les messages envoyés avant l'arrivée du membre.", "Allows members to read messages sent before they arrived.";
        P_SEND_TTS_MESSAGES = "Envoyer des messages de synthèse vocale", "Send Text-to-Speech Messages";
        D_SEND_TTS_MESSAGES = "Permet d'envoyer des messages lus à voix haute (/tts).", "Allows members to send messages read aloud (/tts).";
        P_SEND_VOICE_MESSAGES = "Envoyer des messages vocaux", "Send Voice Messages";
        D_SEND_VOICE_MESSAGES = "Permet d'envoyer des messages vocaux enregistrés.", "Allows members to send recorded voice messages.";
        P_SEND_POLLS = "Créer des sondages", "Create Polls";
        D_SEND_POLLS = "Permet de créer des sondages.", "Allows members to create polls.";
        P_USE_APPLICATION_COMMANDS = "Utiliser les commandes de l'application", "Use Application Commands";
        D_USE_APPLICATION_COMMANDS = "Permet d'utiliser les commandes des applications et des robots.", "Allows members to use commands from apps and bots.";
        P_USE_EMBEDDED_ACTIVITIES = "Utiliser les activités", "Use Activities";
        D_USE_EMBEDDED_ACTIVITIES = "Permet de lancer des activités.", "Allows members to launch activities.";
        P_USE_EXTERNAL_APPS = "Utiliser des applications externes", "Use External Apps";
        D_USE_EXTERNAL_APPS = "Permet aux applications ajoutées par les membres de répondre publiquement.", "Allows apps added by members to post public responses.";
        P_CONNECT = "Se connecter", "Connect";
        D_CONNECT = "Permet de rejoindre le salon vocal et d'entendre les autres.", "Allows members to join the voice channel and hear others.";
        P_SPEAK = "Parler", "Speak";
        D_SPEAK = "Permet de parler dans le salon vocal.", "Allows members to talk in the voice channel.";
        P_STREAM = "Vidéo", "Video";
        D_STREAM = "Permet de partager sa caméra, son écran ou un jeu.", "Allows members to share their video, screen or a game.";
        P_USE_SOUNDBOARD = "Utiliser la soundboard", "Use Soundboard";
        D_USE_SOUNDBOARD = "Permet de jouer des sons de la soundboard du serveur.", "Allows members to play sounds from the server soundboard.";
        P_USE_EXTERNAL_SOUNDS = "Utiliser des sons externes", "Use External Sounds";
        D_USE_EXTERNAL_SOUNDS = "Permet de jouer des sons d'autres serveurs.", "Allows members to play sounds from other servers.";
        P_USE_VAD = "Utiliser la détection de la voix", "Use Voice Activity";
        D_USE_VAD = "Sans elle, les membres doivent utiliser «\u{a0}Appuyer pour parler\u{a0}».", "Without it, members must use Push-to-Talk.";
        P_PRIORITY_SPEAKER = "Voix prioritaire", "Priority Speaker";
        D_PRIORITY_SPEAKER = "Baisse le volume des autres quand ce membre parle.", "Lowers the volume of others when this member is talking.";
        P_MUTE_MEMBERS = "Rendre les membres muets", "Mute Members";
        D_MUTE_MEMBERS = "Permet de couper le micro d'autres membres pour tout le monde.", "Allows members to mute other members for everyone.";
        P_DEAFEN_MEMBERS = "Mettre les membres en sourdine", "Deafen Members";
        D_DEAFEN_MEMBERS = "Permet d'empêcher d'autres membres d'entendre le salon.", "Allows members to stop other members from hearing the channel.";
        P_MOVE_MEMBERS = "Déplacer des membres", "Move Members";
        D_MOVE_MEMBERS = "Permet de déplacer ou de déconnecter des membres d'un salon vocal.", "Allows members to move or disconnect members from a voice channel.";
        P_SET_VOICE_CHANNEL_STATUS = "Définir le statut du salon vocal", "Set Voice Channel Status";
        D_SET_VOICE_CHANNEL_STATUS = "Permet de changer le statut du salon vocal.", "Allows members to change the voice channel status.";
        P_REQUEST_TO_SPEAK = "Demander à parler", "Request to Speak";
        D_REQUEST_TO_SPEAK = "Permet de demander la parole sur une scène.", "Allows members to request to speak on a stage.";
        P_CREATE_EVENTS = "Créer des événements", "Create Events";
        D_CREATE_EVENTS = "Permet de créer des événements.", "Allows members to create events.";
        P_MANAGE_EVENTS = "Gérer les événements", "Manage Events";
        D_MANAGE_EVENTS = "Permet de modifier et d'annuler les événements.", "Allows members to edit and cancel events.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        kbps(n) = "{n}\u{a0}kbps", "{n}kbps";
        delete_title(name) = "Supprimer «\u{a0}{name}\u{a0}»", "Delete “{name}”";
        clone_title(name) = "Cloner #{name}", "Clone #{name}";
        create_in(name) = "dans {name}", "in {name}";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        deleted(name) = "«\u{a0}{name}\u{a0}» a été supprimé.", "“{name}” was deleted.";
        synced_with(name) = "Permissions synchronisées avec la catégorie {name}.", "Permissions synced with category: {name}.";
        not_synced(name) = "Permissions non synchronisées avec la catégorie {name}.", "Permissions not synced with category: {name}.";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        seconds = ["{n} seconde", "{n} secondes"], ["{n} second", "{n} seconds"];
        minutes = ["{n} minute", "{n} minutes"], ["{n} minute", "{n} minutes"];
        hours = ["{n} heure", "{n} heures"], ["{n} hour", "{n} hours"];
        days = ["{n} jour", "{n} jours"], ["{n} day", "{n} days"];
        weeks = ["{n} semaine", "{n} semaines"], ["{n} week", "{n} weeks"];
        users = ["{n} utilisateur", "{n} utilisateurs"], ["{n} user", "{n} users"];
    }
}
