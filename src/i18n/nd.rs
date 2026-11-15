//! Textes des fonctions non documentées (lot ND) : effets de profil, quêtes,
//! Orbes. Module enfant de `table` (il en reprend les macros), tenu à part
//! pour les fusions.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        // Quêtes.
        QUESTS = "Quêtes", "Quests";
        TAB_AVAILABLE = "Disponibles", "Available";
        TAB_CLAIMED = "Réclamées", "Claimed";
        HEAD_AVAILABLE = "Quêtes disponibles", "Available Quests";
        HEAD_CLAIMED = "Quêtes réclamées", "Claimed Quests";
        SORT_SUGGESTED = "Suggérées", "Suggested";
        SORT_NEWEST = "Les plus récentes", "Most Recent";
        SORT_ENDING = "Bientôt terminées", "Expiring Soon";
        FILTER = "Filtre", "Filter";
        FILTER_REWARDS = "Récompense", "Reward";
        FILTER_TASKS = "Tâche", "Task";
        FILTER_ORBS = "Orbes", "Orbs";
        FILTER_COSMETIC = "Parure de profil", "Profile cosmetic";
        FILTER_IN_GAME = "Objet en jeu ou code", "In-game item or code";
        FILTER_NITRO = "Nitro", "Nitro";
        FILTER_PLAY = "Jouer", "Play";
        FILTER_VIDEO = "Regarder une vidéo", "Watch a video";
        FILTER_OTHER = "Autre objectif", "Other goal";
        FILTER_RESET = "Tout afficher", "Show all";
        NONE_MATCH = "Aucune quête ne correspond au filtre.", "No quests match the filter.";
        PROMOTED_BY = "Promotion par", "Promoted by";
        MORE = "Plus d'options", "More options";
        COPY_LINK = "Copier le lien de la quête", "Copy Quest Link";
        LINK_COPIED = "Lien copié.", "Link copied.";
        HIDE = "Masquer cette quête", "Hide this Quest";
        HIDE_TIP = "Masquée dans Ferrocord seulement, jusqu'à la fin de la session\u{a0}: rien n'est dit à Discord.", "Hidden in Ferrocord only, until the session ends: nothing is sent to Discord.";
        PLAY = "Jouer", "Play";
        PLAY_TIP = "Ouvre la page prévue par la quête dans le navigateur. Ferrocord ne déclare rien à Discord\u{a0}: la progression se fait dans l'application officielle.", "Opens the page provided by the quest in your browser. Ferrocord reports nothing to Discord: progress is made in the official app.";
        JOIN_SERVER = "Rejoindre le serveur", "Join Server";
        LEARN_MORE = "En savoir plus", "Learn More";
        START = "Commencer la quête", "Start Quest";
        CLAIMED_DONE = "Récompense réclamée", "Reward claimed";
        DETAILS = "Détails de la quête", "Quest details";
        DETAIL_TASKS = "Tâches", "Tasks";
        CLOSE = "Fermer", "Close";
        NITRO_ENTRY = "Ton Nitro", "Your Nitro";
        REFRESH = "Actualiser", "Refresh";
        LOADING = "Chargement des quêtes…", "Loading quests…";
        NONE_AVAILABLE = "Aucune quête n'est proposée à ce compte pour l'instant.", "No quests are offered to this account right now.";
        NONE_CLAIMED = "Aucune quête terminée pour l'instant.", "No completed quests yet.";
        FAILED = "Les quêtes n'ont pas pu être chargées.", "Quests could not be loaded.";
        USER_ONLY = "Les quêtes ne sont pas proposées aux comptes de bot.", "Quests are not available to bot accounts.";
        ACCEPT = "Accepter la quête", "Accept Quest";
        CLAIM = "Réclamer la récompense", "Claim Reward";
        CLAIM_OFFICIAL = "Réclamer dans Discord", "Claim in Discord";
        CLAIM_OFFICIAL_TIP = "Cette récompense demande de choisir une console\u{a0}: le choix se fait dans l'application officielle.", "This reward requires picking a console: that is done in the official app.";
        OPEN_OFFICIAL = "Ouvrir dans Discord", "Open in Discord";
        FINISH_OFFICIAL = "À terminer dans Discord", "Complete in Discord";
        FINISH_OFFICIAL_TIP = "Ferrocord ne déclare aucune progression à votre place\u{a0}: la quête se joue ou se regarde dans l'application officielle, et la progression affichée ici est celle que Discord renvoie.", "Ferrocord never reports progress on your behalf: the quest is played or watched in the official app, and the progress shown here is what Discord returns.";
        SHOW_CODE = "Afficher le code", "Show Code";
        COPY_CODE = "Copier le code", "Copy Code";
        CODE_COPIED = "Code copié.", "Code copied.";
        BUSY = "En cours…", "Working…";
        STAGE_ACCEPTED = "Acceptée", "Accepted";
        STAGE_COMPLETED = "Terminée", "Completed";
        STAGE_CLAIMED = "Récompense réclamée", "Reward claimed";
        STAGE_EXPIRED = "Échue", "Expired";
        CAPTCHA = "Discord demande une vérification (captcha) pour cette action\u{a0}: Ferrocord ne la contourne pas. Faites-la dans l'application officielle.", "Discord asks for a verification (captcha) for this action: Ferrocord does not bypass it. Do it in the official app.";
        REFUSED = "Discord a refusé cette action.", "Discord refused this action.";
        BLOCKED = "Discord n'autorise pas de nouvelle quête pour l'instant.", "Discord does not allow accepting a new quest right now.";
        ORBS = "Orbes", "Orbs";
        ORBS_TIP = "Solde d'Orbes annoncé par Discord. Les Orbes se dépensent dans la boutique de l'application officielle.", "Orbs balance as announced by Discord. Orbs are spent in the official app's shop.";
        TASK_PLAY_DESKTOP = "Jouer sur ordinateur", "Play on desktop";
        TASK_STREAM = "Diffuser le jeu à un ami", "Stream the game to a friend";
        TASK_XBOX = "Jouer sur Xbox", "Play on Xbox";
        TASK_PLAYSTATION = "Jouer sur PlayStation", "Play on PlayStation";
        TASK_VIDEO = "Regarder la vidéo", "Watch the video";
        TASK_VIDEO_MOBILE = "Regarder la vidéo sur mobile", "Watch the video on mobile";
        TASK_ACTIVITY = "Jouer à l'activité", "Play the activity";
        TASK_ACHIEVEMENT = "Atteindre l'objectif en jeu", "Reach the in-game goal";
        TASK_OTHER = "Tâche à faire dans Discord", "Task to do in Discord";
        TASKS_ANY = "Une seule de ces tâches suffit\u{a0}:", "Any one of these tasks is enough:";
        TASKS_ALL = "Toutes ces tâches sont demandées\u{a0}:", "All of these tasks are required:";
        REWARD = "Récompense", "Reward";
        REWARD_GENERIC = "la récompense", "the reward";
        REWARD_ORBS = "Orbes", "Orbs";
        // Effets de profil.
        EFFECT = "Effet de profil", "Profile Effect";
        EFFECT_NONE = "Aucun", "None";
        EFFECT_TIP = "Effets que possède ce compte. Les effets s'achètent dans la boutique de l'application officielle.", "Effects this account owns. Effects are bought in the official app's shop.";
        EFFECT_LOADING = "Chargement des effets…", "Loading effects…";
        EFFECT_EMPTY = "Ce compte ne possède aucun effet de profil.", "This account owns no profile effects.";
        EFFECT_FAILED = "Les effets n'ont pas pu être chargés.", "Effects could not be loaded.";
        // Tag de serveur.
        TAG = "Tag de serveur", "Server Tag";
        TAG_NONE = "Aucun", "None";
        TAG_REFUSED = "Discord a refusé ce choix.", "Discord refused this choice.";
        LOOK_NONE = "Aucune", "None";
        // Boutique en consultation.
        SHOP = "Boutique", "Shop";
        SHOP_TIP = "La boutique de Discord, en consultation. Ferrocord n'achète rien\u{a0}: les prix et les achats sont dans l'application officielle.", "Discord's shop, view only. Ferrocord never buys anything: prices and purchases are in the official app.";
        SHOP_OFFICIAL = "Voir dans Discord", "View in Discord";
        SHOP_OFFICIAL_TIP = "Ouvre la boutique de Discord dans le navigateur.", "Opens Discord's shop in the browser.";
        SHOP_LOADING = "Chargement de la boutique…", "Loading the shop…";
        SHOP_EMPTY = "La boutique ne propose rien pour l'instant.", "The shop has nothing on offer right now.";
        SHOP_FAILED = "La boutique n'a pas pu être chargée.", "The shop could not be loaded.";
        SHOP_REFUSED = "Discord ne montre pas la boutique à ce compte.", "Discord does not show the shop to this account.";
        SHOP_DECORATION = "Décoration d'avatar", "Avatar Decoration";
        SHOP_NAMEPLATE = "Plaque", "Nameplate";
        SHOP_FRAME = "Cadre de profil", "Profile Frame";
        SHOP_BUNDLE = "Lot", "Bundle";
        SHOP_OTHER = "Objet", "Item";
        SHOP_TAB = "Catalogue", "Catalog";
        // Inventaire, en lecture.
        INVENTORY = "Inventaire", "Inventory";
        INVENTORY_TIP = "Ce que ce compte possède d'après Discord\u{a0}: achats, cadeaux reçus, récompenses. Lecture seule\u{a0}: pour offrir ou utiliser un objet, passez par l'application officielle.", "What this account owns according to Discord: purchases, received gifts, rewards. Read only: to gift or use an item, go through the official app.";
        INVENTORY_LOADING = "Chargement de l'inventaire…", "Loading the inventory…";
        INVENTORY_EMPTY = "Ce compte ne possède rien pour l'instant.", "This account owns nothing yet.";
        INVENTORY_FAILED = "L'inventaire n'a pas pu être chargé.", "The inventory could not be loaded.";
        INVENTORY_REFUSED = "Discord ne montre pas l'inventaire à ce compte.", "Discord does not show the inventory to this account.";
        INVENTORY_MINE = "Possédé", "Owned";
        INVENTORY_GIFTABLE = "Cadeaux à offrir", "Gifts to give";
        ORIGIN_PURCHASE = "Achat", "Purchase";
        ORIGIN_SUBSCRIPTION = "Abonnement", "Subscription";
        ORIGIN_GIFT = "Cadeau reçu", "Received gift";
        ORIGIN_QUEST = "Récompense de quête", "Quest reward";
        ORIGIN_ORBS = "Échange d'Orbes", "Orbs exchange";
        SHOP_PRICE_TIP = "Prix de base annoncé par Discord, à titre indicatif. Le prix réel (taxes, réduction Nitro) est celui de l'application officielle.", "Base price as announced by Discord, for reference. The actual price (taxes, Nitro discount) is the one in the official app.";
        DISCOVER = "Découvrir des serveurs", "Explore servers";
        DISCOVER_HINT = "Rechercher un serveur public", "Search public servers";
        DISCOVER_SEARCH = "Rechercher", "Search";
        DISCOVER_ALL = "Tous", "All";
        DISCOVER_LOADING = "Chargement des serveurs…", "Loading servers…";
        DISCOVER_EMPTY = "Aucun serveur ne correspond.", "No server matches.";
        DISCOVER_FAILED = "La liste des serveurs n'a pas pu être chargée.", "The server list could not be loaded.";
        DISCOVER_REFUSED = "Discord refuse la découverte de serveurs à ce compte. Elle reste disponible dans le navigateur.", "Discord does not allow Server Discovery for this account. It is still available in the browser.";
        DISCOVER_OFFICIAL_TIP = "Ouvre la découverte de serveurs de Discord dans le navigateur", "Opens Discord Server Discovery in the browser";
        DISCOVER_JOIN = "Rejoindre…", "Join…";
        DISCOVER_JOIN_TIP = "Ouvre «\u{a0}Rejoindre un serveur\u{a0}» avec l'adresse de ce serveur", "Opens “Join a Server” with this server's address";
        DISCOVER_PAGE = "Voir la page", "View page";
        DISCOVER_PAGE_TIP = "Ouvre la page publique du serveur dans le navigateur", "Opens the server's public page in the browser";
        DISCOVER_OPEN = "Ouvrir", "Open";
        DISCOVER_JOINED = "déjà rejoint", "already joined";
        SUGGESTIONS = "Suggestions d'amis", "Friend suggestions";
        SAVED_TAB = "Enregistrés", "Saved";
        SAVE_MESSAGE = "Enregistrer le message", "Save message";
        REMIND_HOUR = "Me le rappeler dans 1 heure", "Remind me in 1 hour";
        REMIND_DAY = "Me le rappeler demain", "Remind me tomorrow";
        SAVED_DONE = "Message enregistré.", "Message saved.";
        REMINDER_DONE = "Rappel enregistré chez Discord.", "Reminder saved with Discord.";
        SAVED_REFUSED = "Discord a refusé d'enregistrer ce message.", "Discord refused to save this message.";
        SAVED_UNAVAILABLE = "Discord ne donne pas les messages enregistrés à ce compte.", "Discord does not provide saved messages for this account.";
        SAVED_EMPTY = "Aucun message enregistré. Le menu d'un message permet de l'enregistrer ou de demander un rappel.", "No saved message. A message's menu lets you save it or ask for a reminder.";
        SAVED_NO_TEXT = "(message sans texte ou supprimé)", "(message without text, or deleted)";
        SAVED_REMOVE = "Retirer", "Remove";
        SAFETY = "État du compte", "Account standing";
        SAFETY_TIP = "Ce que le centre de sécurité de Discord dit de ce compte. Rien n'est demandé tant que vous ne cliquez pas.", "What Discord's Safety Hub says about this account. Nothing is requested until you click.";
        SAFETY_CHECK = "Vérifier auprès de Discord", "Check with Discord";
        SAFETY_REFUSED = "Discord ne donne pas l'état de ce compte.", "Discord does not provide this account's standing.";
        SAFETY_NONE = "Aucune infraction en cours.", "No active violation.";
        SAFETY_MORE = "En savoir plus", "Learn more";
        SAFETY_APPEAL = "Les recours et les vérifications se font dans l'application officielle.", "Appeals and verifications are done in the official app.";
        STANDING_GOOD = "Tout va bien", "All good";
        STANDING_LIMITED = "Compte limité", "Limited";
        STANDING_VERY_LIMITED = "Compte très limité", "Very limited";
        STANDING_AT_RISK = "Compte en danger", "At risk";
        STANDING_SUSPENDED = "Compte suspendu", "Suspended";
        STANDING_UNKNOWN = "État inconnu de Ferrocord", "Standing unknown to Ferrocord";
        DISCOVER_TITLE = "Découvrir", "Discover";
        DISCOVER_SERVERS = "Serveurs", "Servers";
        DISCOVER_APPS = "Applications", "Apps";
        APPS_HINT = "Rechercher une application", "Search apps";
        APPS_LOADING = "Chargement des applications…", "Loading apps…";
        APPS_EMPTY = "Aucune application ne correspond.", "No app matches.";
        APPS_FAILED = "Le répertoire d'applications n'a pas pu être chargé.", "The App Directory could not be loaded.";
        APPS_REFUSED = "Discord refuse le répertoire d'applications à ce compte. Il reste disponible dans le navigateur.", "Discord does not allow the App Directory for this account. It is still available in the browser.";
        APPS_PAGE = "Voir la fiche", "View page";
        APPS_PAGE_TIP = "Ouvre la fiche de l'application dans le navigateur. L'ajout se fait là-bas.", "Opens the app's page in the browser; adding it happens there";
        VERIFY = "Règles et vérification", "Rules and verification";
        VERIFY_NONE = "Ce serveur n'a pas de vérification des membres.", "This server has no member verification.";
        VERIFY_AGREE = "J'ai lu et j'accepte les règles", "I have read and agree to the rules";
        VERIFY_UNSUPPORTED = "Ce champ ne peut pas être rempli ici. Terminez la vérification dans l'application officielle.", "This field cannot be filled in here. Finish the verification in the official app.";
        VERIFY_SEND = "Envoyer", "Submit";
        VERIFY_SUBMITTED = "Votre demande est envoyée. Elle attend une décision du serveur.", "Your request has been sent. It is waiting for the server's decision.";
        VERIFY_APPROVED = "Votre demande est acceptée.", "Your request has been approved.";
        VERIFY_REJECTED = "Votre demande a été refusée. Vous pouvez en envoyer une nouvelle.", "Your request was rejected. You can send a new one.";
        VERIFY_REFUSED = "Discord a refusé l'envoi.", "Discord refused the request.";
        JOIN_REQUESTS = "Demandes d'adhésion", "Join requests";
        JOIN_REQUESTS_SHOW = "Voir les demandes", "View requests";
        JOIN_REQUESTS_EMPTY = "Aucune demande en attente.", "No pending request.";
        JOIN_APPROVE = "Accepter", "Approve";
        JOIN_REJECT = "Refuser…", "Reject…";
        JOIN_REJECT_CONFIRM = "Refuser la demande", "Reject request";
        JOIN_REASON_HINT = "Motif du refus (facultatif)", "Reason for rejecting (optional)";
        SUMMARIES = "Résumés du salon", "Channel summaries";
        SUMMARIES_TIP = "Résumés produits par Discord (IA) pour ce salon. Ferrocord les affiche tels quels.", "Summaries generated by Discord (AI) for this channel. Ferrocord shows them as is.";
        SUMMARIES_EMPTY = "Discord n'a aucun résumé pour ce salon. Les résumés n'existent que sur les serveurs qui les ont activés.", "Discord has no summary for this channel. Summaries only exist on servers that enabled them.";
        SUMMARIES_REFUSED = "Discord ne fournit pas de résumés pour ce salon.", "Discord does not provide summaries for this channel.";
        SUGGESTION_ADD = "Ajouter", "Add friend";
        SUGGESTION_DISMISS = "Ignorer", "Dismiss";
    }
}

/// Textes à trous.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        by_publisher(game, publisher) = "{game} · {publisher}", "{game} · {publisher}";
        ends(date) = "Jusqu'au {date}", "Ends {date}";
        ended(date) = "Terminée le {date}", "Ended {date}";
        progress_time(done, total) = "{done} sur {total}", "{done} of {total}";
        progress_count(done, total) = "{done} / {total}", "{done} / {total}";
        blocked_until(date) = "Discord n'autorise pas de nouvelle quête avant le {date}.", "Discord does not allow accepting a new quest before {date}.";
        task_minutes(task, minutes) = "{task} ({minutes})", "{task} ({minutes})";
        eyebrow(name) = "Quête\u{a0}: {name}", "Quest: {name}";
        claim_title(reward) = "Récupère {reward}", "Claim {reward}";
        promoted_by(publisher) = "Promotion par {publisher}", "Promoted by {publisher}";
        ends_short(date) = "Finit le {date}", "Ends {date}";
        ended_short(date) = "Finie le {date}", "Ended {date}";
        hidden_show(n) = "Réafficher les quêtes masquées ({n})", "Show hidden quests ({n})";
        do_play(game, span, reward) = "Joue à {game} pendant {span} pour gagner {reward}.", "Play {game} for {span} to win {reward}.";
        do_stream(game, span, reward) = "Diffuse {game} à un ami pendant {span} pour gagner {reward}.", "Stream {game} to a friend for {span} to win {reward}.";
        do_video(game, reward) = "Regarde la vidéo de {game} pour gagner {reward}.", "Watch the {game} video to win {reward}.";
        do_goal(game, reward) = "Atteins l'objectif dans {game} pour gagner {reward}.", "Reach the goal in {game} to win {reward}.";
        effect_named(name) = "Effet\u{a0}: {name}", "Effect: {name}";
        unnamed_item(id) = "Objet n°\u{a0}{id}", "Item #{id}";
        since(what, date) = "{what} · depuis le {date}", "{what} · since {date}";
        until(what, date) = "{what} · jusqu'au {date}", "{what} · until {date}";
        consumed(what) = "{what} · utilisé", "{what} · used";
        suggested_from(user, name, platform) = "{user} · {name} sur {platform}", "{user} · {name} on {platform}";
        reminder_on(date) = "Rappel le {date}", "Reminder on {date}";
        reminder_due(date) = "Rappel échu depuis le {date}", "Reminder due since {date}";
        safety_until(date) = "Mesures en vigueur jusqu'au {date}", "Measures in effect until {date}";
        verify_title(server) = "Règles de {server}", "{server} rules";
        verify_rejected(reason) = "Votre demande a été refusée\u{a0}: {reason}", "Your request was rejected: {reason}";
    }
}

/// Textes avec un nombre.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        server_members = ["{n} membre", "{n} membres"], ["{n} member", "{n} members"];
        online = ["{n} en ligne", "{n} en ligne"], ["{n} online", "{n} online"];
        messages = ["{n} message", "{n} messages"], ["{n} message", "{n} messages"];
        people = ["{n} personne", "{n} personnes"], ["{n} person", "{n} people"];
        servers = ["Dans {n} serveur", "Dans {n} serveurs"], ["In {n} server", "In {n} servers"];
        guild_violations = ["{n} infraction sur un serveur dont vous êtes responsable", "{n} infractions sur des serveurs dont vous êtes responsable"], ["{n} violation on a server you are responsible for", "{n} violations on servers you are responsible for"];
        mutual_friends = ["{n} ami en commun", "{n} amis en commun"], ["{n} mutual friend", "{n} mutual friends"];
        orbs = ["{n} Orbe", "{n} Orbes"], ["{n} Orb", "{n} Orbs"];
        minutes = ["{n}\u{a0}min", "{n}\u{a0}min"], ["{n} min", "{n} min"];
        seconds = ["{n}\u{a0}s", "{n}\u{a0}s"], ["{n} s", "{n} s"];
        minutes_long = ["{n} minute", "{n} minutes"], ["{n} minute", "{n} minutes"];
        seconds_long = ["{n} seconde", "{n} secondes"], ["{n} second", "{n} seconds"];
        claimable = ["{n} récompense à réclamer", "{n} récompenses à réclamer"], ["{n} reward to claim", "{n} rewards to claim"];
    }
}
