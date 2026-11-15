//! Textes des événements programmés, de l'accueil des nouveaux membres et
//! des pages Communauté des paramètres du serveur (lot A7). Module enfant de
//! `table` (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        EVENTS = "Événements", "Events";
        CREATE_EVENT = "Créer un événement", "Create Event";
        EDIT_EVENT = "Modifier l'événement", "Edit Event";
        NO_EVENTS = "Aucun événement à venir.", "No upcoming events.";
        LOADING = "Chargement…", "Loading…";
        LIVE = "En cours", "Happening now";
        COMPLETED = "Terminé", "Ended";
        CANCELED = "Annulé", "Canceled";
        INTERESTED = "Ça m'intéresse", "Interested";
        NOT_INTERESTED = "Ça ne m'intéresse plus", "Not interested";
        COPY_LINK = "Copier le lien", "Copy Link";
        LINK_COPIED = "Lien de l'événement copié.", "Event link copied.";
        EDIT = "Modifier", "Edit";
        START = "Commencer", "Start Event";
        END = "Terminer", "End Event";
        CANCEL_EVENT = "Annuler l'événement", "Cancel Event";
        DELETE = "Supprimer", "Delete";
        CONFIRM = "Confirmer", "Confirm";
        CLOSE = "Fermer", "Close";
        SAVE = "Enregistrer", "Save";
        CREATE = "Créer", "Create";
        CANCEL = "Annuler", "Cancel";
        PLACE = "Lieu", "Location";
        STAGE = "Salon de conférence", "Stage Channel";
        VOICE = "Salon vocal", "Voice Channel";
        EXTERNAL = "Ailleurs", "Somewhere Else";
        CHANNEL = "Salon", "Channel";
        NO_VENUE = "Aucun salon de ce type sur ce serveur.", "No channel of this kind on this server.";
        LOCATION_HINT = "Adresse, lien…", "Address, link…";
        NAME = "Nom de l'événement", "Event Topic";
        START_DATE = "Début", "Start";
        END_DATE = "Fin", "End";
        HAS_END = "Heure de fin", "End time";
        DATE_HINT = "2026-11-21", "2026-11-21";
        TIME_HINT = "20:30", "20:30";
        DESCRIPTION = "Description", "Description";
        REPEAT = "Répétition", "Repeat";
        REPEAT_NONE = "Ne se répète pas", "Does not repeat";
        REPEAT_DAILY = "Tous les jours", "Every day";
        REPEAT_WEEKDAYS = "Du lundi au vendredi", "Every weekday (Monday to Friday)";
        REPEAT_WEEKLY = "Chaque semaine", "Weekly";
        REPEAT_BIWEEKLY = "Toutes les deux semaines", "Every other week";
        REPEAT_MONTHLY = "Chaque mois", "Monthly";
        REPEAT_YEARLY = "Chaque année", "Annually";
        NTH_1 = "premier", "first";
        NTH_2 = "deuxième", "second";
        NTH_3 = "troisième", "third";
        NTH_4 = "quatrième", "fourth";
        NTH_5 = "cinquième", "fifth";
        ERR_NAME = "Le nom doit compter de 1 à 100 caractères.", "The topic must be 1 to 100 characters long.";
        ERR_DESCRIPTION = "La description dépasse 1000 caractères.", "The description is longer than 1000 characters.";
        ERR_LOCATION = "Le lieu doit compter de 1 à 100 caractères.", "The location must be 1 to 100 characters long.";
        ERR_END_REQUIRED = "Un événement ailleurs doit avoir une heure de fin.", "An event somewhere else needs an end time.";
        ERR_END_BEFORE = "La fin doit venir après le début.", "The end must come after the start.";
        ERR_START_PAST = "Le début doit être dans le futur.", "The start must be in the future.";
        ERR_CHANNEL = "Choisissez un salon.", "Choose a channel.";
        ERR_DATE = "Date ou heure illisible (forme attendue 2026-11-21 et 20:30).", "Unreadable date or time (expected like 2026-11-21 and 20:30).";
        BUSY = "Une action est déjà en cours.", "An action is already in progress.";
        NOT_FOUND = "Cet élément n'existe plus.", "This item no longer exists.";
        ACT_LOAD = "Chargement des événements", "Loading events";
        ACT_CREATE = "Création de l'événement", "Creating the event";
        ACT_EDIT = "Modification de l'événement", "Editing the event";
        ACT_STATUS = "Changement de statut", "Status change";
        ACT_DELETE = "Suppression de l'événement", "Deleting the event";
        ACT_INTEREST = "Inscription à l'événement", "Event subscription";
        ACT_OCCURRENCE = "Modification de la prochaine fois", "Next occurrence change";
        ACT_WELCOME = "Lecture de l'écran de bienvenue", "Loading the welcome screen";
        ACT_SAVE_WELCOME = "Enregistrement de l'écran de bienvenue", "Saving the welcome screen";
        ACT_ONBOARDING = "Lecture de l'intégration", "Loading onboarding";
        ACT_SAVE_ONBOARDING = "Enregistrement de l'intégration", "Saving onboarding";
        ACT_RESPOND = "Enregistrement des réponses", "Saving answers";
        GROUP_COMMUNITY = "Communauté", "Community";
        PAGE_EVENTS = "Événements", "Events";
        PAGE_WELCOME = "Écran de bienvenue", "Welcome Screen";
        PAGE_ONBOARDING = "Intégration", "Onboarding";
        PAGE_COMMUNITY = "Découverte et communauté", "Discovery and Community";
        ALL_PAGES = "Toutes les pages", "All pages";
        WELCOME_ENABLED = "Montrer l'écran de bienvenue aux nouveaux membres", "Show the welcome screen to new members";
        WELCOME_DESCRIPTION = "Description (140 caractères au plus)", "Description (up to 140 characters)";
        WELCOME_CHANNELS = "Salons recommandés (5 au plus)", "Recommended channels (up to 5)";
        ADD_CHANNEL = "Ajouter un salon", "Add a channel";
        REMOVE = "Retirer", "Remove";
        EMOJI = "Emoji (facultatif)", "Emoji (optional)";
        CHANNEL_DESCRIPTION = "Ce qu'on y fait", "What happens there";
        WELCOME_NONE = "Ce serveur n'a pas d'écran de bienvenue.", "This server has no welcome screen.";
        WELCOME_COMMUNITY_ONLY = "L'écran de bienvenue est réservé aux serveurs communautaires.", "The welcome screen is for Community servers only.";
        ERR_TOO_LONG = "Un texte est trop long.", "A text is too long.";
        ERR_TOO_MANY = "Trop d'éléments.", "Too many items.";
        ERR_EMPTY = "Un titre, une description ou un salon obligatoire manque.", "A required title, description or channel is missing.";
        ERR_NO_OPTION = "Chaque question doit avoir au moins une option.", "Each question needs at least one option.";
        ERR_ANSWER = "Répondez aux questions obligatoires (une seule réponse aux questions à choix unique).", "Answer the required questions (only one answer to single-choice questions).";
        SAVED = "Enregistré.", "Saved.";
        ANSWERS_SAVED = "Réponses enregistrées.", "Answers saved.";
        CHANNELS_ROLES = "Salons et rôles", "Channels & Roles";
        BROWSE_CHANNELS = "Parcourir les salons", "Browse Channels";
        COVER = "Image de couverture", "Cover Image";
        SKIP_NEXT = "Sauter la prochaine fois", "Skip Next Occurrence";
        UNSKIP_NEXT = "Rétablir la prochaine fois", "Restore Next Occurrence";
        NEXT_SKIPPED = "La prochaine fois est annulée.", "The next occurrence is canceled.";
        COVER_PICK = "Choisir une image…", "Choose an Image…";
        COVER_CHOSEN = "Nouvelle image choisie", "New image chosen";
        COVER_KEEP = "Ne pas la changer", "Keep the current one";
        COVER_UNREADABLE = "Ce fichier n'est pas une image lisible.", "This file is not a readable image.";
        BROWSE_SHOW_ALL = "Afficher tous les salons", "Show All Channels";
        BROWSE_SHOW_ALL_TIP = "Décoché\u{a0}: la liste ne montre que les salons suivis.", "Unchecked: the list only shows the channels you follow.";
        BROWSE_FOLLOW = "Suivi", "Following";
        BROWSE_BUSY = "Enregistrement…", "Saving…";
        BROWSE_TIP = "Choisissez ce qui vous intéresse\u{a0}: Discord vous donne les rôles et les salons qui vont avec.", "Pick what interests you: Discord gives you the matching roles and channels.";
        REQUIRED = "obligatoire", "required";
        SINGLE = "une seule réponse", "pick one";
        ONBOARDING_OFF = "Ce serveur n'utilise pas l'intégration.", "This server doesn't use onboarding.";
        ONB_ENABLED = "Intégration activée", "Onboarding enabled";
        ONB_MODE = "Critères", "Criteria";
        MODE_DEFAULT = "Salons par défaut seulement", "Default channels only";
        MODE_ADVANCED = "Salons par défaut et questions", "Default channels and questions";
        DEFAULT_CHANNELS = "Salons par défaut", "Default channels";
        QUESTIONS = "Questions", "Questions";
        ADD_QUESTION = "Ajouter une question", "Add a question";
        ADD_OPTION = "Ajouter une option", "Add an option";
        QUESTION_TITLE = "Question", "Question";
        OPTION_TITLE = "Titre de l'option", "Option title";
        OPTION_DESCRIPTION = "Description (facultative)", "Description (optional)";
        SINGLE_SELECT = "Une seule réponse", "Single choice";
        REQUIRED_Q = "Obligatoire", "Required";
        IN_ONBOARDING = "Posée à l'arrivée", "Asked when joining";
        DROPDOWN = "Liste déroulante", "Dropdown";
        ROLES = "Rôles donnés", "Roles given";
        CHANNELS_GIVEN = "Salons ajoutés", "Channels added";
        MOVE_UP = "Monter", "Move up";
        MOVE_DOWN = "Descendre", "Move down";
        ONB_TIP = "Quand l'intégration est activée, Discord exige au moins un salon par défaut où @everyone peut écrire.", "When onboarding is enabled, Discord requires at least one default channel where @everyone can send messages.";
        COMMUNITY_ON = "Ce serveur est un serveur communautaire.", "This is a Community server.";
        COMMUNITY_OFF = "Ce serveur n'est pas un serveur communautaire.", "This is not a Community server.";
        ENABLE_COMMUNITY = "Gérer la communauté dans l'application officielle", "Manage Community in the official app";
        ENABLE_COMMUNITY_TIP = "Activer ou désactiver la communauté change la vérification, le filtre du contenu et des salons\u{a0}: cela se fait dans l'application officielle, qui l'explique pas à pas.", "Turning Community on or off changes verification, the content filter and channels: do it in the official app, which walks you through it.";
        RULES_CHANNEL = "Salon des règles", "Rules channel";
        UPDATES_CHANNEL = "Salon des mises à jour de la communauté", "Community updates channel";
        NONE = "Aucun", "None";
        OPEN = "Ouvrir", "Open";
        FEATURES = "Fonctions du serveur", "Server features";
        DISCOVERY_ON = "Le serveur est visible dans la découverte de serveurs.", "The server is listed in Server Discovery.";
        DISCOVERY_OFF = "Le serveur n'est pas dans la découverte de serveurs.", "The server isn't listed in Server Discovery.";
        SERVER_DESCRIPTION = "Description du serveur", "Server description";
        UNKNOWN_CHANNEL = "salon inconnu", "unknown channel";
        NOT_LOADED = "Pas encore chargé.", "Not loaded yet.";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        events_of(name) = "Événements de {name}", "Events in {name}";
        welcome_to(name) = "Bienvenue sur {name}", "Welcome to {name}";
        voice_at(name) = "Salon vocal\u{a0}: {name}", "Voice channel: {name}";
        stage_at(name) = "Conférence\u{a0}: {name}", "Stage: {name}";
        until(when) = "jusqu'à {when}", "until {when}";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        repeat_weekly(day) = "Chaque semaine, le {day}", "Weekly on {day}";
        repeat_biweekly(day) = "Toutes les deux semaines, le {day}", "Every other week on {day}";
        repeat_monthly(nth, day) = "Chaque mois, le {nth} {day}", "Monthly on the {nth} {day}";
        repeat_yearly(day, month) = "Chaque année, le {day} {month}", "Annually on {month} {day}";
        confirm_delete(name) = "Supprimer «\u{a0}{name}\u{a0}»\u{a0}? C'est définitif.", "Delete “{name}”? This can't be undone.";
        confirm_cancel(name) = "Annuler «\u{a0}{name}\u{a0}»\u{a0}? Les membres le verront annulé.", "Cancel “{name}”? Members will see it as canceled.";
        options_of(n, max) = "{n} sur {max}", "{n} of {max}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        interested = ["{n} personne intéressée", "{n} personnes intéressées"], ["{n} person interested", "{n} people interested"];
    }
}
