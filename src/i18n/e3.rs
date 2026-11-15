//! Textes des plugins « discret et saisie » (lot E3) : mode discret,
//! brouillons persistants, messages programmés, traduction d'un message.
//! Module enfant de `table` (mêmes macros), tenu à part pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Mode discret.
        STEALTH = "Mode discret", "Stealth mode";
        STEALTH_ABOUT = "Trois réglages indépendants\u{a0}: ne pas dire aux autres que vous écrivez, ne marquer un salon comme lu qu'au clic sur «\u{a0}Marquer comme lu\u{a0}», et flouter les images et vidéos jusqu'au survol ou au clic. Ce sont des requêtes en moins, jamais en plus.", "Three independent settings: don't tell others you are typing, only mark a channel as read when you click “Mark As Read”, and blur images and videos until hovered or clicked. These are requests left out, never added.";
        STEALTH_TYPING = "Ne pas envoyer «\u{a0}en train d'écrire\u{a0}»", "Don't send “is typing”";
        STEALTH_TYPING_TIP = "Les autres ne voient plus l'indicateur pendant que vous tapez.", "Others no longer see the indicator while you type.";
        STEALTH_READ = "Ne pas marquer comme lu automatiquement", "Don't mark as read automatically";
        STEALTH_READ_TIP = "Un salon ouvert reste non lu, ici et sur vos autres appareils, tant que vous ne cliquez pas «\u{a0}Marquer comme lu\u{a0}» dans son en-tête. Envoyer un message le marque comme lu chez Discord.", "An open channel stays unread, here and on your other devices, until you click “Mark As Read” in its header. Sending a message marks it as read on Discord's side.";
        STEALTH_BLUR = "Flouter les médias", "Blur media";
        STEALTH_BLUR_TIP = "Images, GIF, vignettes et affiches des vidéos sont brouillés dans le fil.", "Images, GIFs, thumbnails and video posters are blurred in the feed.";
        STEALTH_BLUR_GUILDS = "Flouter seulement dans les serveurs", "Blur in servers only";
        STEALTH_BLUR_GUILDS_TIP = "Les médias des messages privés restent nets.", "Media in direct messages stay sharp.";
        STEALTH_REVEAL = "Dévoiler un média flouté", "Reveal a blurred media";
        STEALTH_REVEAL_TIP = "Au survol, il se brouille de nouveau quand la souris le quitte\u{a0}; au clic, il reste net.", "On hover, it blurs again when the pointer leaves; on click, it stays sharp.";
        STEALTH_REVEAL_HOVER = "Au survol", "On hover";
        STEALTH_REVEAL_CLICK = "Au clic", "On click";
        STEALTH_MARK_READ_TIP = "Marquer comme lu (mode discret)", "Mark As Read (stealth mode)";

        // Brouillons persistants.
        DRAFTS = "Brouillons persistants", "Persistent drafts";
        DRAFTS_ABOUT = "Le texte que vous n'avez pas envoyé dans un salon, et la réponse en préparation, sont retrouvés au prochain lancement. Gardés sur cet ordinateur, chiffrés comme l'historique hors ligne\u{a0}; les pièces jointes en attente ne le sont pas. Un crayon marque les salons qui ont un brouillon. Désactiver efface les fichiers.", "Text you have not sent in a channel, and the reply in progress, are back the next time you start. Kept on this computer, encrypted like the offline history; pending attachments are not. A pencil marks channels that have a draft. Disabling erases the files.";
        DRAFTS_DAYS = "Durée de conservation (jours)", "Keep for (days)";
        DRAFTS_DAYS_TIP = "Un brouillon qui n'a pas changé depuis plus longtemps est effacé.", "A draft left unchanged for longer is erased.";
        // Messages programmés (envoi confirmé).
        SCHED = "Programmation de messages (envoi confirmé)", "Message scheduling (confirmed sending)";
        SCHED_ABOUT = "Menu «\u{a0}+\u{a0}» de la saisie\u{a0}: «\u{a0}Programmer…\u{a0}» garde un message sur cet ordinateur avec une date et une heure. L'envoi demande toujours votre confirmation\u{a0}: à l'heure dite, rien ne part tout seul, une notification et un bandeau vous proposent «\u{a0}Envoyer\u{a0}», «\u{a0}Reporter\u{a0}» ou «\u{a0}Supprimer\u{a0}». Si Ferrocord était fermé, le bandeau apparaît au lancement suivant.", "The composer's “+” menu: “Schedule…” keeps a message on this computer with a date and time. Sending always asks for your confirmation: when the time comes, nothing is sent by itself, a notification and a banner offer “Send”, “Postpone” or “Delete”. If Ferrocord was closed, the banner shows the next time it starts.";
        SCHED_MENU = "Programmer…", "Schedule…";
        SCHED_LIST = "Messages programmés", "Scheduled messages";
        SCHED_TITLE_NEW = "Programmer un message", "Schedule a message";
        SCHED_TITLE_EDIT = "Modifier le message programmé", "Edit scheduled message";
        SCHED_TEXT = "Message", "Message";
        SCHED_DATE = "Date", "Date";
        SCHED_DATE_HINT = "2026-12-24", "2026-12-24";
        SCHED_TIME = "Heure", "Time";
        SCHED_TIME_HINT = "18h30", "18:30";
        SCHED_IN_1H = "Dans 1 heure", "In 1 hour";
        SCHED_TONIGHT = "Ce soir, 20:00", "Tonight, 20:00";
        SCHED_TOMORROW = "Demain, 09:00", "Tomorrow, 09:00";
        SCHED_CONFIRM_NOTE = "Rien n'est envoyé tout seul\u{a0}: à l'heure dite, Ferrocord vous prévient et attend votre clic sur «\u{a0}Envoyer\u{a0}».", "Nothing is sent by itself: when the time comes, Ferrocord notifies you and waits for your click on “Send”.";
        SCHED_DO = "Programmer", "Schedule";
        SCHED_EDIT = "Modifier", "Edit";
        SCHED_BAD_DATE = "Date ou heure illisible. Exemple\u{a0}: 2026-12-24 et 18:30.", "Unreadable date or time. Example: 2026-12-24 and 18:30.";
        SCHED_PAST = "Cette date est déjà passée.", "This date is already in the past.";
        SCHED_NO_TEXT = "Le message est vide.", "The message is empty.";
        SCHED_FULL = "Trop de messages programmés\u{a0}: supprimez-en un d'abord.", "Too many scheduled messages: delete one first.";
        SCHED_NO_COMMAND = "Cette commande agit tout de suite\u{a0}: elle ne se programme pas.", "This command acts right away: it cannot be scheduled.";
        SCHED_SEND = "Envoyer", "Send";
        SCHED_SEND_NOW = "Envoyer maintenant", "Send now";
        SCHED_POSTPONE = "Reporter", "Postpone";
        SCHED_P10 = "De 10 minutes", "By 10 minutes";
        SCHED_P60 = "D'une heure", "By one hour";
        SCHED_PDAY = "À demain, même heure", "Until tomorrow, same time";
        SCHED_NONE = "Aucun message programmé.", "No scheduled message.";
        SCHED_DUE = "À envoyer", "Ready to send";
        SCHED_TOAST_TITLE = "Message programmé", "Scheduled message";
        // Traduction d'un message.
        TR = "Traduction d'un message", "Message translation";
        TR_ABOUT = "Clic droit sur un message\u{a0}: «\u{a0}Traduire\u{a0}». La traduction s'affiche sous le message, avec «\u{a0}Afficher l'original\u{a0}». Attention\u{a0}: le texte du message est envoyé au service de traduction que vous choisissez dans les réglages (une adresse compatible LibreTranslate, auto-hébergée ou publique). Rien n'est contacté tant que vous n'avez pas choisi un service et lu l'avertissement\u{a0}; une requête par clic, jamais automatiquement.", "Right-click a message: “Translate”. The translation shows under the message, with “Show original”. Warning: the message text is sent to the translation service you choose in the settings (a LibreTranslate-compatible address, self-hosted or public). Nothing is contacted until you have chosen a service and read the warning; one request per click, never automatically.";
        TR_URL = "Adresse du service (compatible LibreTranslate)", "Service address (LibreTranslate-compatible)";
        TR_URL_TIP = "L'adresse complète du service, qui commence par https (http accepté pour un service sur cet ordinateur ou le réseau local). Vide\u{a0}: aucun service, rien n'est contacté.", "For example https://translate.example.org, or http://127.0.0.1:5000 for a service on this computer. Empty: no service, nothing is contacted.";
        TR_TARGET = "Traduire vers", "Translate to";
        TR_TARGET_TIP = "La langue d'origine est reconnue par le service.", "The source language is detected by the service.";
        TR_LANG_UI = "La langue de Ferrocord", "Ferrocord's language";
        TR_L_FR = "Français", "French";
        TR_L_EN = "Anglais", "English";
        TR_L_ES = "Espagnol", "Spanish";
        TR_L_DE = "Allemand", "German";
        TR_L_IT = "Italien", "Italian";
        TR_L_PT = "Portugais", "Portuguese";
        TR_L_NL = "Néerlandais", "Dutch";
        TR_L_PL = "Polonais", "Polish";
        TR_L_RU = "Russe", "Russian";
        TR_L_UK = "Ukrainien", "Ukrainian";
        TR_L_TR = "Turc", "Turkish";
        TR_L_JA = "Japonais", "Japanese";
        TR_L_KO = "Coréen", "Korean";
        TR_L_ZH = "Chinois", "Chinese";
        TR_L_AR = "Arabe", "Arabic";
        TR_ACK = "J'ai lu l'avertissement", "I have read the warning";
        TR_ACK_TIP = "Le texte de chaque message que vous traduisez est envoyé à ce service, qui peut le lire et le conserver. Ne traduisez pas ce qui doit rester privé, sauf vers un service que vous hébergez.", "The text of every message you translate is sent to this service, which can read and keep it. Do not translate what must stay private, except with a service you host yourself.";
        TR_MENU = "Traduire", "Translate";
        TR_ORIGINAL = "Afficher l'original", "Show original";
        TR_PENDING = "Traduction en cours…", "Translating…";
        TR_F_TIMEOUT = "Traduction impossible\u{a0}: le service n'a pas répondu à temps.", "Translation failed: the service did not answer in time.";
        TR_F_NETWORK = "Traduction impossible\u{a0}: le service est injoignable.", "Translation failed: the service cannot be reached.";
        TR_F_REFUSED = "Traduction refusée\u{a0}: le service demande une clé d'API valide.", "Translation refused: the service requires a valid API key.";
        TR_F_BUSY = "Traduction refusée\u{a0}: trop de demandes, réessayez plus tard.", "Translation refused: too many requests, try again later.";
        TR_F_SERVICE = "Traduction impossible\u{a0}: le service a répondu par une erreur.", "Translation failed: the service answered with an error.";
        TR_F_ANSWER = "Traduction impossible\u{a0}: réponse illisible. L'adresse est-elle celle d'un service compatible LibreTranslate\u{a0}?", "Translation failed: unreadable answer. Is the address a LibreTranslate-compatible service?";
        TR_SETUP = "Choisissez d'abord un service de traduction et lisez l'avertissement\u{a0}: Paramètres > Plugins, roue dentée de «\u{a0}Traduction d'un message\u{a0}».", "First choose a translation service and read the warning: Settings > Plugins, the gear on “Message translation”.";
        TR_WAIT = "Deux traductions sont déjà en cours\u{a0}: attendez leur réponse.", "Two translations are already running: wait for their answer.";
        TR_READY = "Service prêt\u{a0}: «\u{a0}Traduire\u{a0}» enverra le texte du message à cette adresse.", "Service ready: “Translate” will send the message text to this address.";
        TR_NO_SERVICE = "Aucun service choisi\u{a0}: rien n'est contacté.", "No service chosen: nothing is contacted.";
        TR_BAD_SCHEME = "Adresse refusée\u{a0}: elle doit commencer par https (http seulement vers cet ordinateur ou le réseau local).", "Address refused: it must start with https (http only for this computer or the local network).";
        TR_BAD_URL = "Adresse illisible\u{a0}: seulement l'adresse du service, sans identifiant ni paramètre.", "Unreadable address: only the service address, with no credentials or parameters.";
        TR_NEEDS_ACK = "Activez «\u{a0}J'ai lu l'avertissement\u{a0}» pour que «\u{a0}Traduire\u{a0}» fonctionne.", "Turn on “I have read the warning” for “Translate” to work.";
        TR_KEY = "Clé d'API (facultative)", "API key (optional)";
        TR_KEY_HINT = "Aucune clé", "No key";
        TR_KEY_KEPT = "Une clé est enregistrée", "A key is saved";
        TR_KEY_TIP = "Demandée par certains services publics. Gardée chiffrée sur cet ordinateur, jamais affichée ni écrite dans un journal.", "Required by some public services. Kept encrypted on this computer, never shown or written to a log.";
        TR_KEY_SAVE = "Enregistrer la clé", "Save key";
        TR_KEY_ERASE = "Effacer la clé", "Erase key";
    }
}

/// Textes à paramètres.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        sched_banner(place) = "Message programmé pour {place}", "Scheduled message for {place}";
        sched_more(n) = "+{n} en attente", "+{n} waiting";
        sched_for(place) = "Pour {place}", "For {place}";
        sched_done(place, when) = "Message programmé pour {place}, {when}. Vous confirmerez l'envoi.", "Message scheduled for {place}, {when}. You will confirm sending.";
        sched_toast_body(place) = "Pour {place}\u{a0}: ouvrez Ferrocord pour confirmer l'envoi.", "For {place}: open Ferrocord to confirm sending.";
        tr_head(from, to) = "Traduit ({from} > {to})", "Translated ({from} > {to})";
        tr_head_to(to) = "Traduit ({to})", "Translated ({to})";
    }
}
