//! Textes des fils de discussion et des salons forum (lot T10). Module enfant
//! de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        JOIN = "Rejoindre", "Join";
        LEAVE = "Quitter le fil", "Leave Thread";
        ARCHIVE = "Archiver", "Archive";
        UNARCHIVE = "Désarchiver", "Unarchive";
        ARCHIVED = "Archivé", "Archived";
        LOCKED = "Verrouillé", "Locked";
        PRIVATE = "Fil privé", "Private thread";
        CREATE_THREAD = "Créer un fil", "Create Thread";
        THREAD_NAME_HINT = "Nom du fil", "Thread name";
        CREATE = "Créer", "Create";
        CANCEL = "Annuler", "Cancel";
        SHOW_THREADS = "Afficher les fils", "Show threads";
        HIDE_THREADS = "Masquer les fils", "Hide threads";
        NEW_POST = "Nouvelle publication", "New Post";
        POST_TITLE_HINT = "Titre", "Title";
        POST_MESSAGE_HINT = "Message", "Message";
        PUBLISH = "Publier", "Post";
        SEARCH_POSTS = "Rechercher des publications", "Search posts";
        SORT_ACTIVITY = "Activité récente", "Recent activity";
        SORT_CREATED = "Date de création", "Creation date";
        ALL_TAGS = "Toutes les étiquettes", "All tags";
        LOAD_ARCHIVED = "Charger les publications archivées", "Load archived posts";
        NO_MORE_ARCHIVED = "Toutes les publications archivées sont chargées.", "All archived posts are loaded.";
        NO_POSTS = "Aucune publication.", "No posts.";
        PINNED_POST = "Épinglée", "Pinned";
        OPEN_THREAD = "Ouvrir le fil", "Open thread";
        BACK_TO_PARENT = "Revenir au salon", "Back to channel";
        ACTION_JOIN = "Rejoindre le fil", "Joining the thread";
        ACTION_LEAVE = "Quitter le fil", "Leaving the thread";
        ACTION_ARCHIVE = "Archivage", "Archiving";
        ACTION_CREATE = "Création du fil", "Creating the thread";
        ACTION_POST = "Publication", "Posting";
        ACTION_ARCHIVED = "Chargement des publications archivées", "Loading archived posts";
        TITLE_REQUIRED = "Donnez un titre et un message à la publication.", "Give the post a title and a message.";
        PANEL_TITLE = "Fils", "Threads";
        TAB_ACTIVE = "Actifs", "Active";
        TAB_ARCHIVED = "Archivés", "Archived";
        NO_ACTIVE = "Aucun fil actif dans ce salon.", "There are no active threads in this channel.";
        NO_ARCHIVED = "Aucun fil archivé.", "No archived threads.";
        LOAD_MORE = "Charger plus de fils", "Load more threads";
        LOADING_THREADS = "Chargement des fils…", "Loading threads…";
        LOAD_FAILED = "Les fils archivés n'ont pas pu être chargés.", "Archived threads couldn't be loaded.";
        RETRY = "Réessayer", "Try again";
        JOINED = "Rejoint", "Joined";
        MARK_READ = "Marquer comme lu", "Mark As Read";
        COPY_LINK = "Copier le lien", "Copy Link";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        in_parent(name) = "dans #{name}", "in #{name}";
        message_in(name) = "Envoyer un message dans {name}", "Message {name}";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        by_author(name) = "par {name}", "by {name}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        messages = ["{n} message", "{n} messages"], ["{n} message", "{n} messages"];
        replies = ["{n} réponse", "{n} réponses"], ["{n} reply", "{n} replies"];
        members = ["{n} membre", "{n} membres"], ["{n} member", "{n} members"];
    }
}
