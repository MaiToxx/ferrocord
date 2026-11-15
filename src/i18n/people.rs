//! Textes du lot U : menu complet d'un utilisateur, pseudo d'ami, demandes de
//! message, « Actifs en ce moment », colonne et en-tête des messages privés.
//! Module enfant de `table` (il en reprend les macros), tenu à part pour que
//! les ajouts des lots parallèles ne se mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        CALL = "Appeler", "Call";
        ADD_NOTE = "Ajouter une note", "Add Note";
        ADD_FRIEND_NICK = "Ajouter un pseudo d'ami", "Add Friend Nickname";
        EDIT_FRIEND_NICK = "Changer le pseudo d'ami", "Change Friend Nickname";
        INVITE_TO_SERVER = "Inviter sur le serveur", "Invite to Server";
        CHANGE_NICK = "Changer le pseudo", "Change Nickname";
        ROLES = "Rôles", "Roles";
        IGNORE = "Ignorer", "Ignore";
        MUTE_DM = "Mettre la conversation en sourdine", "Mute Conversation";
        COPY_ROLE_ID = "Copier l'identifiant du rôle", "Copy Role ID";
        NICK_TITLE = "Pseudo d'ami", "Friend Nickname";
        NICK_DETAIL = "Ce pseudo n'est visible que de vous, dans vos messages privés.", "This nickname is only visible to you, in your direct messages.";
        NICK_RESET = "Retirer le pseudo", "Reset Nickname";
        SAVE = "Enregistrer", "Save";
        CANCEL = "Annuler", "Cancel";
        REQUESTS = "Demandes de message", "Message Requests";
        TAB_REQUESTS = "Demandes", "Requests";
        TAB_SPAM = "Spam", "Spam";
        NONE_REQUESTS = "Aucune demande de message en attente.", "No pending message requests.";
        NONE_SPAM = "Aucune demande dans le dossier de spam.", "No requests in the spam folder.";
        REQUEST_HINT = "Ouvrir la conversation pour lire le message", "Open the conversation to read the message";
        ACTIVE_NOW = "Actifs en ce moment", "Active Now";
        ACTIVE_NONE_TITLE = "C'est calme pour l'instant…", "It's quiet for now…";
        ACTIVE_NONE = "Dès qu'un ami lancera un jeu ou une autre activité, vous le verrez ici.", "As soon as a friend starts a game or another activity, you will see it here.";
        MORE = "Autres actions", "More";
        FIND = "Rechercher ou lancer une conversation", "Find or start a conversation";
        DMS = "Messages privés", "Direct Messages";
        NEW_DM = "Créer un MP", "Create DM";
        VIDEO_CALL = "Démarrer un appel vidéo", "Start Video Call";
        ADD_TO_DM = "Ajouter des amis au MP", "Add Friends to DM";
        SHOW_PROFILE = "Profil de l'utilisateur", "User Profile";
        ACTION_IGNORE = "Mise à l'écart", "Ignoring";
        ACTION_NICK = "Changement du pseudo d'ami", "Changing the friend nickname";
        ACTION_ACCEPT_REQUEST = "Acceptation de la demande", "Accepting the request";
        ACTION_REJECT_REQUEST = "Refus de la demande", "Ignoring the request";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        count(label, n) = "{label} — {n}", "{label} — {n}";
    }
}
