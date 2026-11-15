//! Textes du lot FF (manques fonctionnels de la vérification finale) :
//! messages masqués, statut inactif, menu « + », menus de la saisie et des
//! liens, commandes intégrées, sons, limite d'âge. Module enfant de `table`
//! (il en reprend les macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Messages des personnes bloquées ou ignorées (F1).
        SHOW_MESSAGE = "Afficher le message", "Show message";
        SHOW_MESSAGES = "Afficher les messages", "Show messages";
        HIDE_MESSAGE = "Masquer le message", "Hide message";
        HIDE_MESSAGES = "Masquer les messages", "Hide messages";

        // Menu « + » de la zone de saisie (V1).
        PLUS_TIP = "Envoyer un fichier, et plus", "Upload a file, and more";
        PLUS_FILE = "Envoyer un fichier", "Upload a File";
        PLUS_THREAD = "Créer un fil", "Create Thread";
        PLUS_POLL = "Créer un sondage", "Create Poll";
        PLUS_APPS = "Utiliser une application", "Use Apps";
        PLUS_VOICE = "Enregistrer un message vocal", "Record a Voice Message";

        // Menus contextuels de la saisie et des liens (F7).
        CUT = "Couper", "Cut";
        COPY = "Copier", "Copy";
        PASTE = "Coller", "Paste";
        SELECT_ALL = "Tout sélectionner", "Select All";
        OPEN_LINK = "Ouvrir le lien", "Open Link";
        COPY_LINK = "Copier le lien", "Copy Link";

        // Commandes intégrées (F10) et `@silent` (F9).
        BUILTIN = "Intégré", "Built-In";
        CMD_SHRUG = "Ajoute un haussement d'épaules à votre message", "Appends a shrug to your message";
        CMD_TABLEFLIP = "Ajoute une table renversée à votre message", "Appends a flipped table to your message";
        CMD_UNFLIP = "Ajoute une table remise en place à votre message", "Appends a table put back to your message";
        CMD_ME = "Affiche le texte avec emphase", "Displays text with emphasis";
        CMD_SPOILER = "Marque votre message comme spoiler", "Marks your message as a spoiler";
        CMD_TTS = "Fait lire le message à voix haute aux membres qui regardent le salon", "Reads the message aloud to members viewing the channel";
        CMD_NICK = "Change votre pseudo sur ce serveur", "Changes your nickname on this server";
        NICK_ONLY_SERVER = "Le pseudo ne se change que sur un serveur.", "Nicknames can only be changed in a server.";
        NICK_SENT = "Changement de pseudo envoyé.", "Nickname change sent.";
        SILENT_TIP = "Message envoyé avec @silent, sans notification", "Sent with @silent, without a notification";

        // Sons par événement (F11).
        SOUNDS_SECTION = "Sons", "Sounds";
        SOUNDS_TIP = "Un interrupteur par son. Ce sont les sons de Windows\u{a0}; le volume et l'interrupteur général sont réglés plus haut.", "One switch per sound. These are Windows sounds; the volume and the main switch are set above.";
        SND_MESSAGE = "Message", "Message";
        SND_MENTION = "Mention", "Mention";
        SND_DEAFEN = "Casque coupé", "Deafen";
        SND_UNDEAFEN = "Casque rétabli", "Undeafen";
        SND_MUTE = "Micro coupé", "Mute";
        SND_UNMUTE = "Micro rétabli", "Unmute";
        SND_JOIN = "Arrivée dans le salon vocal", "User Join";
        SND_LEAVE = "Départ du salon vocal", "User Leave";
        SND_CALL_OUT = "Appel sortant", "Outgoing Ring";
        SND_RING = "Appel entrant", "Incoming Ring";
        SND_STREAM_START = "Direct commencé", "Stream Started";
        SND_STREAM_STOP = "Direct arrêté", "Stream Stopped";
        SND_FRIEND = "Demande d'ami", "Friend Request";
        SND_EVENT = "Événement qui commence", "Event Starting";
        SND_REACTION = "Réaction à un de vos messages", "Reaction to Your Message";
        SND_REMINDER = "Rappel d'un message enregistré", "Saved Message Reminder";
        SOMEONE = "Quelqu'un", "Someone";
        REMINDER_TITLE = "Rappel", "Reminder";
        REMINDER_BODY = "Un message enregistré vous attend dans la boîte de réception.", "A saved message is waiting in your inbox.";
        FRIEND_REQUEST_BODY = "vous a envoyé une demande d'ami.", "sent you a friend request.";

        // Salons à limite d'âge et médias sensibles (F8).
        AGE_TITLE = "Salon soumis à une limite d'âge", "Age-Restricted Channel";
        AGE_BODY = "Ce salon peut contenir des contenus réservés aux adultes. Confirmez-vous vouloir y entrer\u{a0}?", "This channel may contain content for adults only. Are you sure you want to enter?";
        AGE_CONTINUE = "Continuer", "Continue";
        AGE_BACK = "Retour", "Go Back";
        SENSITIVE = "Contenu sensible", "Sensitive content";
        SENSITIVE_BLOCKED = "Contenu sensible bloqué", "Sensitive content blocked";

        // Forums (F25).
        FORUM_GUIDELINES = "Consignes de publication", "Post Guidelines";

        // Mentions de salon et de rôle (F3).
        // Fenêtre de notifications du serveur : les trois derniers interrupteurs (F12).
        NOTIF_HIGHLIGHTS = "Supprimer les mentions surlignées", "Suppress Highlights";
        NOTIF_EVENTS = "Rendre muets les nouveaux événements", "Mute New Events";
        NOTIF_MOBILE = "Notifications push mobiles", "Mobile Push Notifications";

        // Fils : verrou, fil privé, liste des membres (F13).
        THREAD_LOCK = "Verrouiller le fil", "Lock Thread";
        THREAD_UNLOCK = "Déverrouiller le fil", "Unlock Thread";
        THREAD_MEMBERS = "Membres du fil", "Thread Members";
        THREAD_PRIVATE = "Fil privé", "Private Thread";
        THREAD_PRIVATE_NOTE = "Seules les personnes que vous invitez et les modérateurs peuvent le voir.", "Only people you invite and moderators can see it.";
        ACTION_LOCK = "Verrouillage", "Locking";
        ACTION_THREAD_MEMBERS = "Lecture des membres du fil", "Loading thread members";

        // Fenêtre « Inviter des amis » (F14).
        INVITE_FRIENDS = "Inviter des amis", "Invite Friends";
        INVITE_FRIENDS_HINT = "Rechercher un ami", "Search for friends";
        INVITE_NO_FRIEND = "Aucun ami ne correspond.", "No friends match.";
        INVITE_ONE = "Inviter", "Invite";
        INVITE_SENT = "Envoyé", "Sent";

        // Haut de la liste des salons et menu du serveur (F15).
        TOP_MEMBERS = "Membres", "Members";
        TOP_BOOSTS = "Boosts du serveur", "Server Boosts";
        MENU_CREATE_CHANNEL = "Créer un salon", "Create Channel";
        MENU_CREATE_CATEGORY = "Créer une catégorie", "Create Category";
        MENU_CREATE_EVENT = "Créer un événement", "Create Event";
        MENU_EDIT_PROFILE = "Modifier le profil du serveur", "Edit Server Profile";

        // Onglet « Ajouter » de la page Amis (retour de l'utilisateur, lot FF2) : les textes de Discord.
        ADD_TAB = "Ajouter", "Add Friend";
        ADD_DETAIL = "Tu peux ajouter des amis grâce à leurs noms d'utilisateur Discord.", "You can add friends with their Discord usernames.";
        ADD_NAME_HINT = "Entre un nom d'utilisateur", "Enter a username";
        ADD_SEND = "Envoyer une demande d'ami", "Send Friend Request";
        ADD_NOTE_HINT = "Personnalise ta demande (facultatif)", "Personalize your request (optional)";
        ADD_NOTE_TIP = "Ce que tu écris ici apparaîtra aussi dans vos MP si vous devenez ami(e)s.", "What you write here will also show up in your DMs if you become friends.";
        ADD_OTHER = "Autres endroits où se faire des amis", "Other Places to Make Friends";
        ADD_OTHER_DETAIL = "Aucun nom d'utilisateur sous la main\u{a0}? Consulte notre liste de serveurs publics qui inclut tout ce qui se fait, des jeux vidéo à la cuisine, en passant par la musique, les animes et bien plus encore.", "Don't have a username on hand? Check out our list of public servers that includes everything from gaming to cooking, music, anime and more.";
        ADD_DISCOVER = "Explore des serveurs en mode Découverte", "Explore Discoverable Servers";

        // Recherche : onglets et recherches récentes (F16).
        SEARCH_RECENT = "Recherches récentes", "Recent Searches";
        SEARCH_RECENT_CLEAR = "Effacer", "Clear";
        SEARCH_TAB_MESSAGES = "Messages", "Messages";
        SEARCH_TAB_IMAGES = "Images", "Images";
        SEARCH_TAB_VIDEOS = "Vidéos", "Videos";
        SEARCH_TAB_LINKS = "Liens", "Links";
        SEARCH_TAB_FILES = "Fichiers", "Files";

        // Emojis verrouillés (F26).
        EMOJI_LOCKED = "Réservé à Nitro", "Nitro only";
        EMOJI_LOCKED_TIP = "Sans Nitro, cet emoji ne s'utilise que sur son serveur.", "Without Nitro, this emoji can only be used in its own server.";

        // Groupes : icône, lien d'invitation, transfert de propriété (F20).
        GROUP_ICON = "Changer l'icône", "Change Icon";
        GROUP_ICON_UNREADABLE = "Cette image n'a pas pu être lue.", "This image could not be read.";
        GROUP_INVITE = "Lien d'invitation", "Invite Link";
        GROUP_INVITE_COPIED = "Lien d'invitation copié, valable 24 heures.", "Invite link copied, valid for 24 hours.";
        GROUP_TRANSFER = "Transférer la propriété du groupe", "Transfer Group Ownership";
        ACTION_GROUP_EDIT = "Modification du groupe", "Editing the group";

        // Reste de F27 : statut effacé aujourd'hui, domaines de confiance.
        EXPIRE_TODAY = "Aujourd'hui", "Today";
        TRUSTED_DOMAINS = "Domaines de confiance", "Trusted Domains";
        TRUSTED_DOMAINS_TIP = "Les liens vers ces domaines s'ouvrent sans confirmation. Cette liste reste sur cet ordinateur.", "Links to these domains open without confirmation. This list stays on this computer.";
        TRUSTED_DOMAINS_NONE = "Aucun domaine de confiance pour l'instant.", "No trusted domains yet.";
        TRUSTED_DOMAIN_HINT = "exemple.com", "example.com";
        TRUSTED_DOMAIN_ADD = "Ajouter", "Add";
        TRUSTED_DOMAIN_REMOVE = "Retirer", "Remove";

        // Carte de profil (F24).
        CARD_ADD_ROLE = "Ajouter un rôle", "Add Role";

        // Bloc de code (V9).
        COPY_CODE = "Copier", "Copy";

        // Débit de la lecture à voix haute (F30).
        SPEAK_RATE = "Débit de la voix (%)", "Speech rate (%)";
        SPEAK_RATE_TIP = "Débit normal à 100. Vaut aussi pour les messages /tts lus à leur arrivée.", "100 is the normal rate. Also applies to /tts messages read as they arrive.";

        ROLE_NONE_KNOWN = "Aucun membre connu avec ce rôle pour l'instant. Faites défiler la liste des membres pour en découvrir.", "No known members with this role yet. Scroll the member list to discover some.";
    }
}

/// Textes à trous.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        role_members(role) = "Membres avec le rôle {role}", "Members with the {role} role";
        reacted(emoji) = "a réagi avec {emoji} à votre message.", "reacted with {emoji} to your message.";
        thread_members_title(thread) = "Membres de {thread}", "Members of {thread}";
        boost_level(level) = "Niveau {level}", "Level {level}";
        group_transfer_confirm(name) = "Transférer la propriété du groupe à {name}\u{a0}? Vous ne pourrez pas revenir en arrière seul.", "Transfer ownership of this group to {name}? You cannot undo this on your own.";
        event_started(server) = "L'événement commence sur {server}.", "The event is starting in {server}.";
    }
}

/// Textes avec un nombre.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        blocked_messages = ["{n} message bloqué", "{n} messages bloqués"], ["{n} blocked message", "{n} blocked messages"];
        ignored_messages = ["{n} message ignoré", "{n} messages ignorés"], ["{n} ignored message", "{n} ignored messages"];
        boosts = ["{n} boost", "{n} boosts"], ["{n} boost", "{n} boosts"];
        known_members = ["{n} membre connu", "{n} membres connus"], ["{n} known member", "{n} known members"];
    }
}
