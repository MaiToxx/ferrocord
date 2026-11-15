//! Textes du rail des serveurs (dossiers, glisser-déposer) et des fenêtres
//! rejoindre, créer, quitter, inviter (lot S7 + A1). Module enfant de
//! `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        ADD_SERVER = "Ajouter un serveur", "Add a Server";
        CREATE_SERVER = "Créer un serveur", "Create a Server";
        JOIN_SERVER = "Rejoindre un serveur", "Join a Server";
        DISCOVER = "Découvrir des serveurs (navigateur)", "Explore servers (browser)";
        CREATE_TIP = "Votre serveur est l'endroit où vous retrouvez vos amis. Créez le vôtre ou partez d'un modèle.", "Your server is where you and your friends hang out. Make yours or start from a template.";
        CREATE_MY_OWN = "Créer le mien", "Create My Own";
        FROM_TEMPLATE = "Depuis un modèle", "From a Template";
        HAVE_INVITE = "Vous avez déjà une invitation\u{a0}?", "Have an invite already?";
        SERVER_NAME = "Nom du serveur", "Server name";
        ICON_PICK = "Choisir une icône", "Choose an icon";
        ICON_REMOVE = "Retirer l'icône", "Remove icon";
        ICON_UNREADABLE = "Cette image est illisible.", "This image can't be read.";
        TEMPLATE_LINK = "Lien ou code du modèle", "Template link or code";
        TEMPLATE_HINT = "discord.new/abcd1234", "discord.new/abcd1234";
        TEMPLATE_CHANNELS = "Salons du modèle", "Template channels";
        PREVIEW = "Aperçu", "Preview";
        CREATE = "Créer", "Create";
        CANCEL = "Annuler", "Cancel";
        BACK = "Retour", "Back";
        INVITE_LINK = "Lien ou code d'invitation", "Invite link or code";
        INVITE_HINT = "discord.gg/abcd123", "discord.gg/abcd123";
        JOIN_TIP = "Une invitation ressemble à discord.gg/abcd123, discord.com/invite/abcd123 ou abcd123.", "Invites look like discord.gg/abcd123, discord.com/invite/abcd123 or abcd123.";
        JOIN = "Rejoindre", "Join Server";
        INVITE_PEOPLE = "Inviter des personnes", "Invite People";
        MANAGE_INVITES = "Invitations", "Invites";
        LEAVE_SERVER = "Quitter le serveur", "Leave Server";
        LEAVE = "Quitter", "Leave";
        CHANNEL = "Salon", "Channel";
        EXPIRE_AFTER = "Expire après", "Expire after";
        MAX_USES = "Nombre maximal d'utilisations", "Max number of uses";
        TEMPORARY = "Adhésion temporaire", "Grant temporary membership";
        TEMPORARY_TIP = "Les membres temporaires sont expulsés à leur déconnexion, sauf si un rôle leur a été donné.", "Temporary members are kicked when they disconnect unless they have been assigned a role.";
        GENERATE = "Générer un lien", "Generate a New Link";
        COPY = "Copier", "Copy";
        COPIED = "Lien copié.", "Link copied.";
        NEVER = "Jamais", "Never";
        NO_LIMIT = "Sans limite", "No limit";
        NO_INVITE_CHANNEL = "Vous n'avez le droit d'inviter dans aucun salon de ce serveur.", "You can't create invites in any channel of this server.";
        INVITER = "Créée par", "Inviter";
        CODE = "Code", "Invite code";
        USES = "Utilisations", "Uses";
        EXPIRES = "Expire", "Expires";
        REVOKE = "Révoquer", "Revoke";
        CONFIRM_REVOKE = "Confirmer", "Confirm";
        NO_INVITES = "Aucune invitation active.", "No active invites.";
        LOADING = "Chargement…", "Loading…";
        REFRESH = "Actualiser", "Refresh";
        CLOSE = "Fermer", "Close";
        TEMPORARY_SHORT = "temporaire", "temporary";
        FOLDER_SETTINGS = "Paramètres du dossier", "Folder Settings";
        FOLDER_NAME_HINT = "Nom du dossier", "Folder name";
        FOLDER_COLOR = "Couleur du dossier", "Folder color";
        FOLDER_DEFAULT_COLOR = "Couleur par défaut", "Default color";
        FOLDER_UNGROUP = "Retirer le dossier", "Remove Folder";
        MARK_FOLDER_READ = "Marquer le dossier comme lu", "Mark Folder As Read";
        SAVE = "Enregistrer", "Save";
        ACTION_JOIN = "Rejoindre le serveur", "Joining the server";
        ACTION_LEAVE = "Quitter le serveur", "Leaving the server";
        ACTION_CREATE = "Création du serveur", "Creating the server";
        ACTION_TEMPLATE = "Lecture du modèle", "Reading the template";
        ACTION_INVITE = "Création de l'invitation", "Creating the invite";
        ACTION_INVITES = "Chargement des invitations", "Loading invites";
        ACTION_REVOKE = "Révocation de l'invitation", "Revoking the invite";
        CAPTCHA = "Discord demande une vérification (captcha)\u{a0}: faites-le dans l'application officielle.", "Discord asks for a verification (captcha): do it in the official app.";
        NOT_FOUND = "invitation ou modèle invalide, expiré ou épuisé", "invalid, expired or used-up invite or template";
        INVITES_DISABLED = "les invitations de ce serveur sont suspendues", "invites are paused for this server";
        BAD_CODE = "Ce n'est pas un lien ni un code valide.", "This isn't a valid link or code.";
        OWNER_CANNOT_LEAVE = "Vous êtes propriétaire de ce serveur\u{a0}: transférez-le ou supprimez-le dans l'application officielle.", "You own this server: transfer or delete it in the official app.";
        FOLDERS_UNKNOWN = "Réglages synchronisés pas encore reçus\u{a0}: l'ordre du rail ne peut pas être enregistré.", "Synced settings not received yet: the server order can't be saved.";
        FOLDERS_BUSY = "Un réglage est en cours d'enregistrement, réessayez dans un instant.", "A setting is being saved, try again in a moment.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        joined(name) = "Vous avez rejoint {name}.", "You joined {name}.";
        joined_verify(name) = "Vous avez rejoint {name}. Ce serveur demande de remplir son formulaire d'adhésion dans l'application officielle.", "You joined {name}. This server asks you to complete its membership form in the official app.";
        left(name) = "Vous avez quitté {name}.", "You left {name}.";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        leave_question(name) = "Quitter {name}\u{a0}? Vous ne pourrez revenir qu'avec une nouvelle invitation.", "Leave {name}? You won't be able to rejoin unless you are re-invited.";
        default_name(name) = "Serveur de {name}", "{name}'s server";
        invite_to(name) = "Inviter des personnes sur {name}", "Invite friends to {name}";
        invites_of(name) = "Invitations de {name}", "Invites for {name}";
        uses_of(n, max) = "{n} sur {max}", "{n} of {max}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        minutes = ["{n} minute", "{n} minutes"], ["{n} minute", "{n} minutes"];
        hours = ["{n} heure", "{n} heures"], ["{n} hour", "{n} hours"];
        days = ["{n} jour", "{n} jours"], ["{n} day", "{n} days"];
        uses = ["{n} utilisation", "{n} utilisations"], ["{n} use", "{n} uses"];
        used = ["Utilisé {n} fois", "Utilisé {n} fois"], ["Used {n} time", "Used {n} times"];
    }
}
