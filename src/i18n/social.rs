//! Textes de la page Amis, des MP de groupe et des accusés de lecture. Module
//! enfant de `table` (il en reprend les macros), tenu à part pour que les
//! ajouts des lots parallèles ne se mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        FRIENDS = "Amis", "Friends";
        TAB_ONLINE = "En ligne", "Online";
        TAB_ALL = "Tous", "All";
        TAB_PENDING = "En attente", "Pending";
        TAB_BLOCKED = "Bloqués", "Blocked";
        ADD_FRIEND = "Ajouter un ami", "Add Friend";
        ADD_FRIEND_DETAIL = "Ajoutez un ami grâce à son nom d'utilisateur Discord.", "You can add friends with their Discord username.";
        USERNAME_HINT = "Nom d'utilisateur", "Username";
        SEND_REQUEST = "Envoyer la demande", "Send Friend Request";
        SEARCH_HINT = "Rechercher", "Search";
        STATUS_ONLINE = "En ligne", "Online";
        STATUS_IDLE = "Inactif", "Idle";
        STATUS_DND = "Ne pas déranger", "Do Not Disturb";
        STATUS_OFFLINE = "Hors ligne", "Offline";
        SHARING = "En partage d'écran", "Sharing their screen";
        INCOMING = "Demande d'ami reçue", "Incoming Friend Request";
        INCOMING_STRANGER = "Demande d'ami reçue, aucun ami ni serveur en commun", "Incoming Friend Request, no mutual friends or servers";
        OUTGOING = "Demande d'ami envoyée", "Outgoing Friend Request";
        BLOCKED = "Bloqué", "Blocked";
        IGNORED = "Ignoré", "Ignored";
        MESSAGE = "Message", "Message";
        ACCEPT = "Accepter", "Accept";
        DECLINE = "Refuser", "Ignore";
        CANCEL_REQUEST = "Annuler", "Cancel";
        REMOVE_FRIEND = "Retirer", "Remove";
        BLOCK = "Bloquer", "Block";
        UNBLOCK = "Débloquer", "Unblock";
        UNIGNORE = "Ne plus ignorer", "Unignore";
        YES = "Oui", "Yes";
        NO = "Non", "No";
        NONE_ONLINE = "Aucun ami en ligne pour l'instant.", "No friends online right now.";
        NONE_ALL = "Pas encore d'amis.", "No friends yet.";
        NONE_PENDING = "Aucune demande d'ami en attente.", "No pending friend requests.";
        NONE_BLOCKED = "Personne n'est bloqué ni ignoré.", "Nobody is blocked or ignored.";
        NO_MATCH = "Aucun résultat.", "No results.";
        NEW_GROUP = "Nouveau groupe", "New Group";
        CATEGORIES = "Catégories", "Categories";
        CAT_EVERYONE = "Toutes", "All";
        CAT_NEW = "Nouvelle catégorie", "New category";
        CAT_NAME_HINT = "Nom de la catégorie", "Category name";
        CAT_CREATE = "Créer", "Create";
        CAT_RENAME = "Renommer", "Rename";
        CAT_DELETE = "Supprimer la catégorie", "Delete category";
        CAT_CANCEL = "Annuler", "Cancel";
        CAT_OTHERS = "Sans catégorie", "Uncategorized";
        CAT_FOLD = "replier", "collapse";
        CAT_UNFOLD = "déplier", "expand";
        CAT_NONE_YET = "Aucune catégorie. Créez-en une depuis la page Amis.", "No category yet. Create one from the Friends page.";
        CAT_EMPTY = "Personne dans cette catégorie. Clic droit sur un ami, puis «\u{a0}Catégories\u{a0}».", "Nobody in this category. Right-click a friend, then “Categories”.";
        CAT_TAKEN = "Ce nom est vide ou déjà pris.", "This name is empty or already taken.";
        NEW_GROUP_DETAIL = "Choisissez jusqu'à 9 amis.", "Pick up to 9 friends.";
        CREATE_GROUP = "Créer le groupe", "Create Group";
        GROUP_SETTINGS = "Membres du groupe", "Group Members";
        ADD_MEMBER = "Ajouter", "Add";
        ADD_MEMBER_HINT = "Ajouter un ami au groupe", "Add a friend to the group";
        REMOVE_MEMBER = "Retirer du groupe", "Remove From Group";
        RENAME = "Renommer", "Rename";
        GROUP_NAME_HINT = "Nom du groupe", "Group name";
        LEAVE_GROUP = "Quitter le groupe", "Leave Group";
        CLOSE_DM = "Fermer la conversation", "Close DM";
        OWNER = "Propriétaire", "Owner";
        YOU = "Vous", "You";
        GROUP_FULL = "Un groupe compte au plus 10 membres.", "A group can have at most 10 members.";
        CLOSE = "Fermer", "Close";
        MARK_READ = "Marquer comme lu", "Mark As Read";
        MARK_GUILD_READ = "Marquer le serveur comme lu", "Mark Server As Read";
        CAPTCHA = "Discord demande une vérification (captcha)\u{a0}: faites cette action dans l'application officielle.", "Discord asks for a verification (captcha): do this in the official app.";
        REFUSED = "Discord a refusé cette action", "Discord refused this action";
        ACTION_ADD_FRIEND = "Demande d'ami", "Friend request";
        ACTION_ACCEPT = "Acceptation", "Accepting";
        ACTION_BLOCK = "Blocage", "Blocking";
        ACTION_REMOVE = "Retrait", "Removal";
        ACTION_UNIGNORE = "Fin de l'ignorance", "Unignoring";
        ACTION_OPEN_DM = "Ouverture de la conversation", "Opening the conversation";
        ACTION_CREATE_GROUP = "Création du groupe", "Creating the group";
        ACTION_ADD_MEMBER = "Ajout au groupe", "Adding to the group";
        ACTION_REMOVE_MEMBER = "Retrait du groupe", "Removing from the group";
        ACTION_RENAME = "Renommage", "Renaming";
        ACTION_CLOSE = "Fermeture", "Closing";
        ACTION_ACK = "Accusé de lecture", "Read receipt";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        playing(name) = "Joue à {name}", "Playing {name}";
        streaming(name) = "En direct\u{a0}: {name}", "Streaming {name}";
        listening(name) = "Écoute {name}", "Listening to {name}";
        watching(name) = "Regarde {name}", "Watching {name}";
        competing(name) = "Participe à {name}", "Competing in {name}";
        confirm_remove(name) = "Retirer {name} de vos amis\u{a0}?", "Remove {name} from your friends?";
        confirm_block(name) = "Bloquer {name}\u{a0}?", "Block {name}?";
        confirm_leave(name) = "Quitter «\u{a0}{name}\u{a0}»\u{a0}?", "Leave “{name}”?";
        request_sent(name) = "Demande d'ami envoyée à {name}.", "Friend request sent to {name}.";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        tab_count(tab, n) = "{tab} ({n})", "{tab} ({n})";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        friends = ["{n} ami", "{n} amis"], ["{n} friend", "{n} friends"];
        selected = ["{n} sélectionné", "{n} sélectionnés"], ["{n} selected", "{n} selected"];
        members = ["{n} membre", "{n} membres"], ["{n} member", "{n} members"];
    }
}
