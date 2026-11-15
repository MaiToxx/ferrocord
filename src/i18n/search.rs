//! Textes de la recherche de messages (lot T12). Module enfant de `table`
//! (il en reprend les macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        SEARCH_HINT = "Rechercher", "Search";
        SEARCH_TIP = "Rechercher des messages (filtres\u{a0}: de, dans, contient…)", "Search messages (filters: from, in, has…)";
        SEARCHING = "Recherche…", "Searching…";
        INDEXING = "Discord prépare la recherche ici\u{a0}; nouvel essai dans un instant…", "Discord is indexing this place; trying again shortly…";
        NOT_INDEXED = "La recherche n'est pas encore prête ici. Réessayez dans quelques minutes.", "Search isn't ready here yet. Try again in a few minutes.";
        NO_RESULTS = "Aucun résultat.", "No results.";
        SORT_NEWEST = "Récents", "Newest";
        SORT_OLDEST = "Anciens", "Oldest";
        SORT_RELEVANCE = "Pertinence", "Most relevant";
        PREVIOUS = "Précédente", "Previous";
        NEXT = "Suivante", "Next";
        CLOSE_SEARCH = "Fermer la recherche", "Close search";
        FILTERS = "Filtres de recherche", "Search options";
        UNKNOWN_CHANNEL = "Ce salon n'est pas accessible dans cette session.", "This channel isn't available in this session.";
        NOT_FOUND = "Message introuvable\u{a0}: il a peut-être été supprimé.", "Message not found: it may have been deleted.";
        NO_HISTORY = "Vous ne pouvez pas lire l'historique de ce salon.", "You can't read this channel's history.";
        EMPTY_QUERY = "Tapez des mots ou un filtre.", "Type some words or a filter.";
        TOO_LONG = "Recherche trop longue (1024 caractères au plus).", "Search too long (1024 characters at most).";
        NO_CHANNELS_HERE = "Le filtre «\u{a0}dans\u{a0}» ne s'applique pas aux messages privés.", "The “in” filter doesn't apply to direct messages.";
        FILTER_USER = "un utilisateur", "a user";
        FILTER_HAS = "lien, intégration, fichier, image, vidéo, son, autocollant ou sondage", "link, embed, file, image, video, sound, sticker or poll";
        FILTER_CHANNEL = "un salon", "a channel";
        FILTER_BEFORE = "avant une date (AAAA-MM-JJ)", "before a date (YYYY-MM-DD)";
        FILTER_AFTER = "après une date (AAAA-MM-JJ)", "after a date (YYYY-MM-DD)";
        FILTER_DURING = "pendant une date (AAAA-MM-JJ)", "on a date (YYYY-MM-DD)";
        FILTER_PINNED = "épinglé\u{a0}: oui ou non", "pinned: true or false";
        FILTER_AUTHOR_TYPE = "utilisateur, bot ou webhook", "user, bot or webhook";
        TODAY = "aujourd'hui", "today";
        YES = "oui", "true";
        NO = "non", "false";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        unknown_user(name) = "Personne de ce nom n'est connue ici\u{a0}: «\u{a0}{name}\u{a0}». Choisissez-la dans la liste.", "Nobody by that name is known here: “{name}”. Pick them from the list.";
        ambiguous_user(name) = "Plusieurs personnes s'appellent «\u{a0}{name}\u{a0}»\u{a0}: choisissez dans la liste.", "Several people are called “{name}”: pick from the list.";
        unknown_channel(name) = "Aucun salon de ce nom\u{a0}: «\u{a0}{name}\u{a0}».", "No channel by that name: “{name}”.";
        ambiguous_channel(name) = "Plusieurs salons s'appellent «\u{a0}{name}\u{a0}»\u{a0}: choisissez dans la liste.", "Several channels are called “{name}”: pick from the list.";
        bad_date(value) = "Date illisible\u{a0}: «\u{a0}{value}\u{a0}» (AAAA-MM-JJ).", "Unreadable date: “{value}” (YYYY-MM-DD).";
        bad_value(key, value) = "Valeur inconnue pour {key}\u{a0}: «\u{a0}{value}\u{a0}».", "Unknown value for {key}: “{value}”.";
        page_of(page, pages) = "Page {page} sur {pages}", "Page {page} of {pages}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        results = ["{n} résultat", "{n} résultats"], ["{n} result", "{n} results"];
    }
}
