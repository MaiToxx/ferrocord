//! Textes des commandes, composants et fenêtres des applications (lot T11).
//! Module enfant de `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        COMMANDS = "Commandes", "Commands";
        LOADING_COMMANDS = "Chargement des commandes…", "Loading commands…";
        NO_COMMANDS = "Aucune commande ne correspond.", "No matching commands.";
        SEND = "Envoyer", "Send";
        CANCEL = "Annuler", "Cancel";
        CLOSE = "Fermer", "Close";
        REQUIRED = "obligatoire", "required";
        YES = "Vrai", "True";
        NO = "Faux", "False";
        NOT_SET = "Non renseigné", "Not set";
        ATTACHMENT_UNSUPPORTED = "Pièce jointe\u{a0}: à envoyer depuis l'application officielle.", "Attachment: send it from the official app.";
        ATTACHMENT_PICK = "Choisir un fichier…", "Choose a File…";
        ATTACHMENT_UNREADABLE = "Ce fichier ne peut pas être lu.", "This file can't be read.";
        INTEGER_EXPECTED = "Nombre entier attendu", "Whole number expected";
        NUMBER_EXPECTED = "Nombre attendu", "Number expected";
        SEARCH = "Rechercher", "Search";
        ONLY_YOU = "Vous seul pouvez voir ceci", "Only you can see this";
        DISMISS = "Ignorer le message", "Dismiss message";
        APPS = "Applications", "Apps";
        APP_BADGE = "APPLI", "APP";
        CHOOSE = "Faire un choix", "Make a selection";
        CONFIRM = "Valider", "Confirm";
        PREMIUM = "Achat\u{a0}: dans l'application officielle", "Purchase: in the official app";
        FILL_REQUIRED = "Remplissez les champs obligatoires.", "Fill in the required fields.";
        NO_RESPONSE = "L'application n'a pas répondu.", "The application did not respond.";
        NOT_CONNECTED = "Connexion pas encore établie\u{a0}: réessayez dans un instant.", "Not connected yet: try again in a moment.";
        COMMAND_GONE = "Cette commande n'est plus disponible.", "This command is no longer available.";
        SUGGESTIONS = "Propositions de l'application", "Suggestions from the application";
        CLEAR = "Effacer", "Clear";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        thinking(name) = "{name} réfléchit…", "{name} is thinking…";
        used(user, command) = "{user} a utilisé /{command}", "{user} used /{command}";
        sending(command, app) = "Envoi de /{command} à {app}…", "Sending /{command} to {app}…";
        sending_action(app) = "Envoi à {app}…", "Sending to {app}…";
        no_response(app) = "{app} n'a pas répondu.", "{app} did not respond.";
        failed(app, reason) = "Échec de l'interaction avec {app}\u{a0}: {reason}", "Interaction with {app} failed: {reason}";
        failed_code(app, code) = "L'interaction avec {app} a échoué (raison {code}).", "Interaction with {app} failed (reason {code}).";
        min_length(n) = "Au moins {n} caractères", "At least {n} characters";
        between(min, max) = "Entre {min} et {max}", "Between {min} and {max}";
        at_least(min) = "Au moins {min}", "At least {min}";
        at_most(max) = "Au plus {max}", "At most {max}";
        pick_between(min, max) = "Choisissez de {min} à {max} éléments", "Pick {min} to {max} items";
    }
}
