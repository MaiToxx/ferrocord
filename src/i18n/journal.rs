//! Textes des plugins locaux du lot E1 : journal des messages supprimés et
//! modifiés, mots-clés surveillés, recherche locale, export d'une
//! conversation. Module enfant de `table`, mêmes macros. (Les noms et les
//! descriptions des plugins sont dans `plugins.rs`.)

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Journal des messages supprimés et modifiés.
        DELETED = "Message supprimé", "Deleted message";
        VERSIONS_TIP = "Voir les versions précédentes", "Show earlier versions";
        VERSIONS_HIDE_TIP = "Masquer les versions précédentes", "Hide earlier versions";
        VERSIONS = "Versions précédentes", "Earlier versions";
        COPY_TEXT = "Copier le texte", "Copy text";
        LOG_SECTION = "Journal des messages", "Message log";
        LOG_CLEAR = "Effacer le journal", "Erase the log";
        LOG_CLEAR_TIP = "Efface les messages supprimés et les versions précédentes gardés sur cet ordinateur, pour tous les comptes.", "Erases the deleted messages and earlier versions kept on this computer, for every account.";
        LOG_CLEARED = "Journal des messages effacé.", "Message log erased.";
        LOG_OFF = "Le journal est désactivé (Paramètres > Plugins)\u{a0}: rien n'est gardé.", "The log is disabled (Settings > Plugins): nothing is kept.";
        LOG_MEMORY_ONLY = "Sur ce système, le journal ne peut pas être chiffré\u{a0}: il reste en mémoire et disparaît à la fermeture.", "On this system the log cannot be encrypted: it stays in memory and is gone when the app closes.";

        // Mots-clés surveillés.
        KW_SECTION = "Mots-clés surveillés", "Watched keywords";
        KW_TIP = "Un message reçu qui contient un de ces mots donne une notification, une ligne dans la boîte de réception (onglet «\u{a0}Pour vous\u{a0}») et le mot est surligné. La casse est ignorée. Rien n'est demandé à Discord.", "A received message containing one of these words gives a notification, a line in the inbox (“For You” tab), and the word is highlighted. Case is ignored. Nothing is asked of Discord.";
        KW_HINT = "Mot ou expression", "Word or phrase";
        KW_ADD = "Ajouter", "Add";
        KW_WHOLE = "Mot entier", "Whole word";
        KW_WHOLE_TIP = "Ne répond pas quand le mot fait partie d'un mot plus long.", "Does not match when the word is part of a longer word.";
        KW_ANYWHERE = "Partout", "Everywhere";
        KW_SCOPE = "Où surveiller", "Where to watch";
        KW_REMOVE = "Retirer", "Remove";
        KW_NONE = "Aucun mot-clé pour l'instant.", "No keyword yet.";
        KW_EXCLUDED = "Salons exclus", "Excluded channels";
        KW_EXCLUDED_TIP = "Pour exclure un salon\u{a0}: clic droit sur le salon, «\u{a0}Ne pas y surveiller les mots-clés\u{a0}».", "To exclude a channel: right-click it, “Don't watch keywords here”.";
        KW_EXCLUDE = "Ne pas y surveiller les mots-clés", "Don't watch keywords here";
        KW_UNKNOWN_CHANNEL = "Salon inconnu", "Unknown channel";
        KW_UNKNOWN_GUILD = "Serveur inconnu", "Unknown server";
        KW_INBOX = "Mots-clés", "Keywords";
        KW_CLEAR = "Tout effacer", "Clear all";
        KW_SEE = "Voir", "View";

        // Recherche locale.
        LS_TOGGLE = "Local", "Local";
        LS_TOGGLE_TIP = "Recherche locale\u{a0}: dans les messages déjà reçus sur cet ordinateur, pendant la frappe, sans rien demander à Discord", "Local search: through messages already received on this computer, as you type, asking Discord nothing";
        LS_HINT = "Rechercher en local", "Search locally";
        LS_MODE_DISCORD = "Discord", "Discord";
        LS_MODE_LOCAL = "Sur cet ordinateur", "On this computer";
        LS_MODE_DISCORD_TIP = "Recherche de Discord\u{a0}: tout l'historique, demandé à Discord", "Discord search: the whole history, asked of Discord";

        // Export d'une conversation.
        EX_MENU = "Exporter la conversation…", "Export conversation…";
        EX_TITLE = "Exporter la conversation", "Export conversation";
        EX_NOTE = "Les messages déjà sur cet ordinateur sont exportés. S'il en manque dans la période, l'historique est demandé à Discord page par page, au rythme normal du client.", "Messages already on this computer are exported. If some are missing in the period, history is requested from Discord page by page, at the client's normal pace.";
        EX_FORMAT = "Format", "Format";
        EX_HTML = "Page HTML (lisible hors ligne)", "HTML page (readable offline)";
        EX_TEXT = "Texte brut", "Plain text";
        EX_PERIOD = "Période", "Period";
        EX_FROM = "Du", "From";
        EX_TO = "Au", "To";
        EX_DATE_HINT = "AAAA-MM-JJ", "YYYY-MM-DD";
        EX_PERIOD_TIP = "Vide\u{a0}: pas de limite.", "Empty: no limit.";
        EX_LOCAL_ONLY = "Seulement ce qui est déjà sur cet ordinateur", "Only what is already on this computer";
        EX_LOCAL_ONLY_TIP = "Rien n'est demandé à Discord.", "Nothing is requested from Discord.";
        EX_FILES = "Inclure les pièces jointes", "Include attachments";
        EX_FILES_TIP = "Leur nombre et leur taille sont annoncés avant tout téléchargement.", "Their number and size are announced before anything is downloaded.";
        EX_GO = "Exporter…", "Export…";
        EX_PICKING = "Choisissez où enregistrer le fichier…", "Choose where to save the file…";
        EX_OFFLINE = "Hors ligne\u{a0}: l'historique plus ancien n'a pas été demandé.", "Offline: older history was not requested.";
        EX_CAPPED = "Plafond atteint\u{a0}: les messages les plus anciens de la période n'y sont pas.", "Limit reached: the oldest messages of the period are not included.";
        EX_PAGE_FAILED = "Une page d'historique n'est pas arrivée.", "A page of history did not arrive.";
        EX_RETRY = "Réessayer", "Retry";
        EX_ANYWAY = "Exporter ce qui est rassemblé", "Export what was gathered";
        EX_DOWNLOAD = "Télécharger et exporter", "Download and export";
        EX_WITHOUT = "Exporter sans les fichiers", "Export without the files";
        EX_TOO_BIG = "C'est trop pour un export\u{a0}: réduisez la période ou exportez sans les fichiers.", "That is too much for one export: narrow the period or export without the files.";
        EX_WRITING = "Écriture du fichier…", "Writing the file…";
        EX_DONE = "Export terminé.", "Export finished.";
        EX_CLOSE = "Fermer", "Close";
        EX_BAD_RANGE = "La date de fin précède la date de début, ou une date ne se lit pas.", "The end date is before the start date, or a date cannot be read.";
        EX_PERIOD_ALL = "Tout l'historique disponible", "All available history";
        EX_EDITED = "(modifié)", "(edited)";
        EX_NOT_INCLUDED = "(non inclus)", "(not included)";
        EX_UNAVAILABLE = "L'export n'est pas disponible sur ce système.", "Export is not available on this system.";
        LS_UNSUPPORTED = "La recherche locale ne connaît pas ce filtre (épinglé, type d'auteur, intégration, autocollant, sondage).", "Local search does not know this filter (pinned, author type, embed, sticker, poll).";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        deleted_at(when) = "Message supprimé ({when})", "Deleted message ({when})";
        version_at(when) = "Jusqu'au {when}", "Until {when}";
        log_kept(deleted, edited) = "Gardés\u{a0}: {deleted} supprimés, {edited} modifiés.", "Kept: {deleted} deleted, {edited} edited.";
        kw_place(word, place) = "«\u{a0}{word}\u{a0}» · {place}", "“{word}” · {place}";
        kw_word(word) = "Mot-clé «\u{a0}{word}\u{a0}»", "Keyword “{word}”";
        ls_note(count) = "Recherche locale\u{a0}: {count} messages gardés sur cet ordinateur. Rien n'est demandé à Discord.", "Local search: {count} messages kept on this computer. Nothing is asked of Discord.";
        ex_bad_date(value) = "Date illisible\u{a0}: «\u{a0}{value}\u{a0}» (attendu AAAA-MM-JJ).", "Unreadable date: “{value}” (expected YYYY-MM-DD).";
        ex_reading(count) = "Lecture de l'historique… {count} messages rassemblés.", "Reading history… {count} messages gathered.";
        ex_files_ask(count, size) = "{count} pièces jointes, {size} à télécharger depuis Discord.", "{count} attachments, {size} to download from Discord.";
        ex_downloading(done, total) = "Téléchargement des pièces jointes\u{a0}: {done} sur {total}.", "Downloading attachments: {done} of {total}.";
        ex_done(count, path) = "{count} messages enregistrés dans {path}.", "{count} messages saved to {path}.";
        ex_done_files(saved, failed) = "Pièces jointes\u{a0}: {saved} copiées, {failed} en échec.", "Attachments: {saved} copied, {failed} failed.";
        ex_write_failed(why) = "Le fichier n'a pas pu être écrit ({why}).", "The file could not be written ({why}).";
        ex_period_from(from) = "Depuis le {from}", "Since {from}";
        ex_period_to(to) = "Jusqu'au {to}", "Until {to}";
        ex_period(from, to) = "Du {from} au {to}", "From {from} to {to}";
        ex_summary(count, when) = "{count} messages, exporté le {when}", "{count} messages, exported on {when}";
        ex_text_line(when, author) = "[{when}] {author}\u{a0}: ", "[{when}] {author}: ";
        ex_text_file(name, place) = "Fichier\u{a0}: {name} ({place})", "File: {name} ({place})";
        kw_line(author, word) = "{author} a écrit «\u{a0}{word}\u{a0}»", "{author} wrote “{word}”";
    }
}
