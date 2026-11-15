//! Textes de la fenêtre « Paramètres du serveur » : pages Rôles et Membres,
//! éditeur de rôle, permissions, rôles et pseudo d'un membre (lot A3).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Fenêtre.
        PAGE_ROLES = "Rôles", "Roles";
        PAGE_MEMBERS = "Membres", "Members";
        GROUP_USER_MANAGEMENT = "Gestion des membres", "User Management";

        // Liste des rôles.
        ROLES_INTRO = "Utilisez les rôles pour regrouper les membres du serveur et leur donner des permissions.", "Use roles to group your server members and assign permissions.";
        SEARCH_ROLES = "Rechercher des rôles", "Search Roles";
        CREATE_ROLE = "Créer un rôle", "Create Role";
        NEW_ROLE_NAME = "nouveau rôle", "new role";
        DEFAULT_PERMISSIONS = "Permissions par défaut", "Default Permissions";
        EVERYONE_DETAIL = "@everyone · s'applique à tous les membres du serveur", "@everyone · applies to all server members";
        DRAG_TIP = "Faites glisser pour changer l'ordre des rôles", "Drag to reorder roles";
        KNOWN_TIP = "Membres connus de Ferrocord (liste des membres, messages, recherche)", "Members known to Ferrocord (member list, messages, search)";
        EDIT = "Modifier", "Edit";
        DUPLICATE = "Dupliquer", "Duplicate";
        DELETE = "Supprimer", "Delete";
        CANCEL = "Annuler", "Cancel";
        BACK = "Retour", "Back";
        CLOSE = "Fermer", "Close";
        NO_ROLE_FOUND = "Aucun rôle trouvé.", "No roles found.";
        LOCKED = "Vous ne pouvez pas modifier ce rôle, il est égal ou supérieur à votre rôle le plus élevé.", "You can't edit this role because it is equal to or above your highest role.";
        MANAGED = "Ce rôle est géré par une intégration. Il ne peut être ni supprimé ni donné à la main.", "This role is managed by an integration. It can't be deleted or assigned manually.";
        MANAGED_BOT = "Rôle d'un bot", "Bot role";
        MANAGED_BOOSTER = "Rôle des boosters du serveur", "Server Booster role";
        MANAGED_OTHER = "Rôle géré", "Managed role";

        // Éditeur.
        TAB_DISPLAY = "Affichage", "Display";
        TAB_PERMISSIONS = "Permissions", "Permissions";
        ROLE_NAME = "Nom du rôle", "Role Name";
        ROLE_COLOR = "Couleur du rôle", "Role Color";
        COLOR_DETAIL = "Les membres prennent la couleur du rôle le plus élevé qu'ils ont.", "Members use the color of the highest role they have.";
        DEFAULT_COLOR = "Couleur par défaut", "Default color";
        CUSTOM_COLOR = "Couleur personnalisée", "Custom color";
        GRADIENT = "Dégradé", "Gradient";
        GRADIENT_DETAIL = "Couleurs en dégradé de ce rôle, modifiables dans l'application officielle.", "This role's gradient colors, editable in the official app.";
        ROLE_ICON = "Icône du rôle", "Role Icon";
        ROLE_ICON_DETAIL = "Une image ou un emoji Unicode affiché à côté du nom des membres.", "An image or Unicode emoji shown next to members' names.";
        ROLE_ICON_LOCKED = "Les icônes de rôle demandent le niveau 2 de boost du serveur.", "Role icons require server boost level 2.";
        ICON_PICK = "Choisir une image", "Choose an image";
        ICON_REMOVE = "Retirer l'icône", "Remove icon";
        ICON_UNREADABLE = "Cette image est illisible.", "This image can't be read.";
        ICON_CURRENT = "Icône actuelle", "Current icon";
        EMOJI = "Emoji", "Emoji";
        EMOJI_HINT = "Un emoji Unicode", "A Unicode emoji";
        HOIST = "Afficher les membres ayant ce rôle séparément des membres en ligne", "Display role members separately from online members";
        MENTIONABLE = "Permettre à tout le monde de @mentionner ce rôle", "Allow anyone to @mention this role";
        MENTIONABLE_DETAIL = "Les membres qui ont la permission de mentionner @everyone peuvent toujours le mentionner.", "Members with permission to mention @everyone can always ping this role.";
        UNSAVED = "Attention, il reste des modifications non enregistrées\u{a0}!", "Careful, you have unsaved changes!";
        RESET = "Réinitialiser", "Reset";
        SAVE = "Enregistrer les modifications", "Save Changes";
        SEARCH_PERMISSIONS = "Rechercher des permissions", "Search permissions";
        CLEAR_PERMISSIONS = "Effacer les permissions", "Clear permissions";
        NO_PERMISSION_FOUND = "Aucune permission trouvée.", "No permissions found.";
        ADMIN_WARNING = "Attention, un administrateur a toutes les permissions et passe outre les permissions des salons. C'est une permission dangereuse à accorder.", "Careful, an administrator has every permission and bypasses channel permissions. This is a dangerous permission to grant.";
        NOT_GRANTABLE = "Vous ne pouvez pas accorder une permission que vous n'avez pas.", "You can't grant a permission you don't have.";
        EVERYONE_NOTE = "Ces permissions s'appliquent à tous les membres du serveur.", "These permissions apply to every member of the server.";

        // Membres d'un rôle.
        ADD_MEMBERS = "Ajouter des membres", "Add Members";
        SEARCH_MEMBERS = "Rechercher des membres", "Search Members";
        SEARCH_SERVER = "Chercher sur le serveur", "Search the server";
        ADD = "Ajouter", "Add";
        REMOVE = "Retirer", "Remove";
        NO_ROLE_MEMBERS = "Aucun membre connu n'a ce rôle. Ferrocord ne connaît que les membres déjà vus (liste des membres, messages, recherche).", "No known member has this role. Ferrocord only knows members it has already seen (member list, messages, search).";
        NO_MEMBER_FOUND = "Aucun membre trouvé.", "No members found.";
        SEARCHING = "Recherche…", "Searching…";

        // Page Membres.
        MEMBERS_INTRO = "Membres connus de Ferrocord. Cherchez sur le serveur pour en trouver d'autres.", "Members known to Ferrocord. Search the server to find others.";
        FILTER_ROLE = "Filtrer par rôle", "Filter by role";
        ALL_ROLES = "Tous les rôles", "All roles";
        COL_NAME = "Nom", "Name";
        COL_JOINED = "Membre depuis", "Member since";
        COL_ROLES = "Rôles", "Roles";
        UNKNOWN_DATE = "—", "—";
        NO_MEMBERS = "Aucun membre connu.", "No known members.";
        SORT_NAME = "Trier par nom", "Sort by name";
        SORT_NEWEST = "Arrivés récemment", "Newest members";
        SORT_OLDEST = "Membres les plus anciens", "Oldest members";

        // Rôles et pseudo d'un membre.
        MENU_ROLES = "Rôles", "Roles";
        MENU_NICK = "Changer le pseudo", "Change Nickname";
        CARD_ROLES = "Gérer les rôles", "Manage roles";
        MEMBER_ROLES_UNKNOWN = "Les rôles de ce membre ne sont pas encore connus.", "This member's roles aren't known yet.";
        NICK_HINT = "Pseudo (vide pour le retirer)", "Nickname (empty to remove it)";
        NICK_RESET = "Retirer le pseudo", "Reset Nickname";

        // Fins d'actions.
        SAVED_CREATE = "Rôle créé.", "Role created.";
        SAVED_EDIT = "Modifications enregistrées.", "Changes saved.";
        SAVED_DELETE = "Rôle supprimé.", "Role deleted.";
        SAVED_REORDER = "Ordre des rôles enregistré.", "Role order saved.";
        SAVED_NICK = "Pseudo modifié.", "Nickname changed.";
        NAME_INVALID = "Rien n'a été envoyé. Le nom d'un rôle compte de 1 à 100 caractères.", "Nothing was sent. A role name has 1 to 100 characters.";
        REFUSED = "Discord a refusé (permission manquante ou rôle trop haut)", "Discord refused (missing permission or role too high)";
        ACTION_CREATE = "Création du rôle", "Creating the role";
        ACTION_EDIT = "Enregistrement du rôle", "Saving the role";
        ACTION_DELETE = "Suppression du rôle", "Deleting the role";
        ACTION_REORDER = "Déplacement du rôle", "Moving the role";
        ACTION_MEMBER_ROLE = "Changement des rôles du membre", "Changing the member's roles";
        ACTION_NICK = "Changement du pseudo", "Changing the nickname";
        ACTION_SEARCH = "Recherche de membres", "Searching members";

        // Groupes de permissions.
        GROUP_GENERAL = "Permissions générales du serveur", "General Server Permissions";
        GROUP_MEMBERSHIP = "Permissions des membres", "Membership Permissions";
        GROUP_TEXT = "Permissions des salons textuels", "Text Channel Permissions";
        GROUP_VOICE = "Permissions des salons vocaux", "Voice Channel Permissions";
        GROUP_APPS = "Permissions des applications", "Apps Permissions";
        GROUP_STAGE = "Permissions des salons de conférence", "Stage Channel Permissions";
        GROUP_EVENTS = "Permissions des événements", "Events Permissions";
        GROUP_ADVANCED = "Permissions avancées", "Advanced Permissions";

        // Permissions : nom, puis description.
        P_VIEW_CHANNEL = "Voir les salons", "View Channels";
        D_VIEW_CHANNEL = "Permet aux membres de voir les salons par défaut (sauf les salons privés).", "Allows members to view channels by default (excluding private channels).";
        P_MANAGE_CHANNELS = "Gérer les salons", "Manage Channels";
        D_MANAGE_CHANNELS = "Permet de créer, modifier et supprimer des salons.", "Allows members to create, edit, or delete channels.";
        P_MANAGE_ROLES = "Gérer les rôles", "Manage Roles";
        D_MANAGE_ROLES = "Permet de créer des rôles et de modifier ou supprimer ceux qui sont plus bas que son rôle le plus élevé.", "Allows members to create new roles and edit or delete roles lower than their highest role.";
        P_CREATE_GUILD_EXPRESSIONS = "Créer des expressions", "Create Expressions";
        D_CREATE_GUILD_EXPRESSIONS = "Permet d'ajouter des emojis, autocollants et sons personnalisés au serveur.", "Allows members to add custom emoji, stickers, and sounds to this server.";
        P_MANAGE_GUILD_EXPRESSIONS = "Gérer les expressions", "Manage Expressions";
        D_MANAGE_GUILD_EXPRESSIONS = "Permet de modifier ou supprimer les emojis, autocollants et sons du serveur.", "Allows members to edit or remove custom emoji, stickers, and sounds in this server.";
        P_VIEW_AUDIT_LOG = "Voir les logs du serveur", "View Audit Log";
        D_VIEW_AUDIT_LOG = "Permet de voir qui a fait quels changements sur le serveur.", "Allows members to view a record of who made which changes in this server.";
        P_VIEW_GUILD_INSIGHTS = "Voir les analyses du serveur", "View Server Insights";
        D_VIEW_GUILD_INSIGHTS = "Permet de voir les statistiques du serveur.", "Allows members to view the server's analytics.";
        P_VIEW_CREATOR_MONETIZATION_ANALYTICS = "Voir les analyses des abonnements", "View Server Subscription Insights";
        D_VIEW_CREATOR_MONETIZATION_ANALYTICS = "Permet de voir les statistiques des abonnements au serveur.", "Allows members to view analytics for server subscriptions.";
        P_MANAGE_WEBHOOKS = "Gérer les webhooks", "Manage Webhooks";
        D_MANAGE_WEBHOOKS = "Permet de créer, modifier et supprimer des webhooks, qui publient des messages d'autres applications.", "Allows members to create, edit, or delete webhooks, which post messages from other apps.";
        P_MANAGE_GUILD = "Gérer le serveur", "Manage Server";
        D_MANAGE_GUILD = "Permet de changer le nom et la région du serveur, de voir les invitations et d'ajouter des bots.", "Allows members to change the server's name and region, view all invites, and add bots.";
        P_CREATE_INSTANT_INVITE = "Créer une invitation", "Create Invite";
        D_CREATE_INSTANT_INVITE = "Permet d'inviter de nouvelles personnes sur le serveur.", "Allows members to invite new people to this server.";
        P_CHANGE_NICKNAME = "Changer le pseudo", "Change Nickname";
        D_CHANGE_NICKNAME = "Permet de changer son propre pseudo sur ce serveur.", "Allows members to change their own nickname on this server.";
        P_MANAGE_NICKNAMES = "Gérer les pseudos", "Manage Nicknames";
        D_MANAGE_NICKNAMES = "Permet de changer le pseudo des autres membres.", "Allows members to change the nicknames of other members.";
        P_KICK_MEMBERS = "Expulser des membres", "Kick Members";
        D_KICK_MEMBERS = "Permet de retirer des membres du serveur (ils peuvent revenir avec une invitation).", "Allows members to remove other members from this server (they can rejoin with an invite).";
        P_BAN_MEMBERS = "Bannir des membres", "Ban Members";
        D_BAN_MEMBERS = "Permet de bannir définitivement des membres et d'effacer leurs derniers messages.", "Allows members to permanently ban other members and delete their recent messages.";
        P_MODERATE_MEMBERS = "Exclure temporairement des membres", "Timeout Members";
        D_MODERATE_MEMBERS = "Permet d'empêcher un membre d'écrire, de réagir et de parler pendant un temps donné.", "Allows members to stop a user from chatting, reacting, or speaking for a set time.";
        P_SEND_MESSAGES = "Envoyer des messages et créer des publications", "Send Messages and Create Posts";
        D_SEND_MESSAGES = "Permet d'envoyer des messages dans les salons textuels et de créer des publications dans les forums.", "Allows members to send messages in text channels and create posts in forum channels.";
        P_SEND_MESSAGES_IN_THREADS = "Envoyer des messages dans les fils et les publications", "Send Messages in Threads and Posts";
        D_SEND_MESSAGES_IN_THREADS = "Permet de répondre dans les fils et les publications.", "Allows members to send messages in threads and in posts.";
        P_CREATE_PUBLIC_THREADS = "Créer des fils publics", "Create Public Threads";
        D_CREATE_PUBLIC_THREADS = "Permet de créer des fils visibles par tous ceux qui voient le salon.", "Allows members to create threads that everyone in a channel can view.";
        P_CREATE_PRIVATE_THREADS = "Créer des fils privés", "Create Private Threads";
        D_CREATE_PRIVATE_THREADS = "Permet de créer des fils sur invitation seulement.", "Allows members to create invite-only threads.";
        P_EMBED_LINKS = "Intégrer des liens", "Embed Links";
        D_EMBED_LINKS = "Montre un aperçu des liens envoyés par les membres.", "Allows links that members share to show embedded content.";
        P_ATTACH_FILES = "Joindre des fichiers", "Attach Files";
        D_ATTACH_FILES = "Permet d'envoyer des fichiers et des images.", "Allows members to upload files or media.";
        P_ADD_REACTIONS = "Ajouter des réactions", "Add Reactions";
        D_ADD_REACTIONS = "Permet d'ajouter de nouvelles réactions aux messages.", "Allows members to add new emoji reactions to a message.";
        P_USE_EXTERNAL_EMOJIS = "Utiliser des emojis externes", "Use External Emoji";
        D_USE_EXTERNAL_EMOJIS = "Permet d'utiliser les emojis d'autres serveurs (avec Nitro).", "Allows members to use emoji from other servers (with Nitro).";
        P_USE_EXTERNAL_STICKERS = "Utiliser des autocollants externes", "Use External Stickers";
        D_USE_EXTERNAL_STICKERS = "Permet d'utiliser les autocollants d'autres serveurs (avec Nitro).", "Allows members to use stickers from other servers (with Nitro).";
        P_MENTION_EVERYONE = "Mentionner @everyone, @here et tous les rôles", "Mention @everyone, @here, and All Roles";
        D_MENTION_EVERYONE = "Permet de prévenir tous les membres d'un salon, et de mentionner les rôles non mentionnables.", "Allows members to notify everyone in a channel, and to mention roles that aren't mentionable.";
        P_MANAGE_MESSAGES = "Gérer les messages", "Manage Messages";
        D_MANAGE_MESSAGES = "Permet de supprimer les messages des autres membres.", "Allows members to delete messages by other members.";
        P_PIN_MESSAGES = "Épingler des messages", "Pin Messages";
        D_PIN_MESSAGES = "Permet d'épingler et de désépingler des messages.", "Allows members to pin or unpin messages.";
        P_BYPASS_SLOWMODE = "Ignorer le mode lent", "Bypass Slowmode";
        D_BYPASS_SLOWMODE = "Permet d'envoyer des messages sans attendre le délai du mode lent.", "Allows members to send messages without waiting for slowmode.";
        P_MANAGE_THREADS = "Gérer les fils et les publications", "Manage Threads and Posts";
        D_MANAGE_THREADS = "Permet de renommer, supprimer, fermer et ouvrir les fils et les publications.", "Allows members to rename, delete, close, and open threads and posts.";
        P_READ_MESSAGE_HISTORY = "Voir les anciens messages", "Read Message History";
        D_READ_MESSAGE_HISTORY = "Permet de lire les messages envoyés avant son arrivée dans le salon.", "Allows members to read messages sent before they arrived.";
        P_SEND_TTS_MESSAGES = "Envoyer des messages de synthèse vocale", "Send Text-to-Speech Messages";
        D_SEND_TTS_MESSAGES = "Permet d'envoyer des messages lus à voix haute à ceux qui ont le salon ouvert.", "Allows members to send messages read aloud to anyone viewing the channel.";
        P_SEND_VOICE_MESSAGES = "Envoyer des messages vocaux", "Send Voice Messages";
        D_SEND_VOICE_MESSAGES = "Permet d'envoyer des messages vocaux.", "Allows members to send voice messages.";
        P_SEND_POLLS = "Créer des sondages", "Create Polls";
        D_SEND_POLLS = "Permet de créer des sondages.", "Allows members to create polls.";
        P_CONNECT = "Se connecter", "Connect";
        D_CONNECT = "Permet de rejoindre les salons vocaux et d'entendre les autres.", "Allows members to join voice channels and hear others.";
        P_SPEAK = "Parler", "Speak";
        D_SPEAK = "Permet de parler dans les salons vocaux.", "Allows members to talk in voice channels.";
        P_STREAM = "Vidéo", "Video";
        D_STREAM = "Permet de partager sa caméra, son écran ou un jeu.", "Allows members to share their video, screen, or a game.";
        P_USE_SOUNDBOARD = "Utiliser la soundboard", "Use Soundboard";
        D_USE_SOUNDBOARD = "Permet de jouer les sons du serveur dans les salons vocaux.", "Allows members to play sounds from the server soundboard.";
        P_USE_EXTERNAL_SOUNDS = "Utiliser des sons externes", "Use External Sounds";
        D_USE_EXTERNAL_SOUNDS = "Permet de jouer les sons d'autres serveurs (avec Nitro).", "Allows members to use sounds from other servers (with Nitro).";
        P_USE_VAD = "Utiliser la détection de voix", "Use Voice Activity";
        D_USE_VAD = "Permet de parler sans appuyer sur une touche. Sans elle, le Push-to-Talk est obligatoire.", "Allows members to speak by simply talking. Without it, Push-to-Talk is required.";
        P_PRIORITY_SPEAKER = "Voix prioritaire", "Priority Speaker";
        D_PRIORITY_SPEAKER = "Baisse le volume des autres quand ce membre parle.", "Lowers the volume of others when this member is talking.";
        P_MUTE_MEMBERS = "Rendre les membres muets", "Mute Members";
        D_MUTE_MEMBERS = "Permet de couper le micro des autres membres pour tout le monde.", "Allows members to mute other members in voice channels for everyone.";
        P_DEAFEN_MEMBERS = "Mettre en sourdine des membres", "Deafen Members";
        D_DEAFEN_MEMBERS = "Permet d'empêcher d'autres membres d'entendre dans les salons vocaux.", "Allows members to deafen other members in voice channels.";
        P_MOVE_MEMBERS = "Déplacer des membres", "Move Members";
        D_MOVE_MEMBERS = "Permet de déconnecter ou de déplacer des membres entre les salons vocaux.", "Allows members to disconnect or move other members between voice channels.";
        P_SET_VOICE_CHANNEL_STATUS = "Définir le statut du salon vocal", "Set Voice Channel Status";
        D_SET_VOICE_CHANNEL_STATUS = "Permet de changer le statut d'un salon vocal.", "Allows members to set a voice channel's status.";
        P_USE_APPLICATION_COMMANDS = "Utiliser les commandes de l'application", "Use Application Commands";
        D_USE_APPLICATION_COMMANDS = "Permet d'utiliser les commandes des applications, dont les commandes slash et les menus contextuels.", "Allows members to use commands from applications, including slash commands and context menu commands.";
        P_USE_EMBEDDED_ACTIVITIES = "Utiliser les Activités", "Use Activities";
        D_USE_EMBEDDED_ACTIVITIES = "Permet de lancer des Activités.", "Allows members to use Activities.";
        P_USE_EXTERNAL_APPS = "Utiliser des applications externes", "Use External Apps";
        D_USE_EXTERNAL_APPS = "Permet aux applications installées par les membres de répondre publiquement.", "Allows apps that members have added to post public responses.";
        P_REQUEST_TO_SPEAK = "Demande de parole", "Request to Speak";
        D_REQUEST_TO_SPEAK = "Permet de demander la parole dans les salons de conférence.", "Allows requests to speak in Stage channels.";
        P_CREATE_EVENTS = "Créer des événements", "Create Events";
        D_CREATE_EVENTS = "Permet de créer des événements programmés.", "Allows members to create scheduled events.";
        P_MANAGE_EVENTS = "Gérer les événements", "Manage Events";
        D_MANAGE_EVENTS = "Permet de modifier et d'annuler les événements programmés.", "Allows members to edit and cancel scheduled events.";
        P_ADMINISTRATOR = "Administrateur", "Administrator";
        D_ADMINISTRATOR = "Donne toutes les permissions et passe outre les permissions des salons. Permission dangereuse.", "Grants every permission and bypasses channel permissions. This is a dangerous permission.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        edit_role(name) = "Modifier le rôle — {name}", "Edit Role — {name}";
        manage_members(n) = "Gérer les membres ({n})", "Manage Members ({n})";
        delete_question(name) = "Supprimer le rôle «\u{a0}{name}\u{a0}»\u{a0}? Cette action est irréversible.", "Delete the role “{name}”? This can't be undone.";
        remove_question(role, member) = "Retirer le rôle «\u{a0}{role}\u{a0}» de {member}\u{a0}?", "Remove the role “{role}” from {member}?";
        roles_of(name) = "Rôles de {name}", "Roles of {name}";
        nick_of(name) = "Pseudo de {name}", "Nickname of {name}";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        tier(n) = "Niveau de boost actuel\u{a0}: {n}", "Current boost level: {n}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        known_members = ["{n} membre connu", "{n} membres connus"], ["{n} known member", "{n} known members"];
    }
}
