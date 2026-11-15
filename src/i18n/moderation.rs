//! Textes de la modération, du journal d'audit, d'AutoMod et de la page
//! Sécurité (lots A4 et A5). Module enfant de `table` (il en reprend les
//! macros), tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        GROUP_MODERATION = "Modération", "Moderation";
        ALL_PAGES = "Toutes les pages", "All pages";
        PAGE_SAFETY = "Sécurité", "Safety Setup";
        PAGE_AUTOMOD = "AutoMod", "AutoMod";
        PAGE_AUDIT = "Journal d'audit", "Audit Log";
        PAGE_BANS = "Bannissements", "Bans";
        NO_PAGE = "Vous n'avez accès à aucune page de ce serveur.", "You can't access any page of this server.";
        LOADING = "Chargement…", "Loading…";
        LOAD_MORE = "Charger plus", "Load more";
        REFRESH = "Actualiser", "Refresh";
        CANCEL = "Annuler", "Cancel";
        CONFIRM = "Confirmer", "Confirm";
        SAVE = "Enregistrer", "Save";
        DELETE = "Supprimer", "Delete";
        EDIT = "Modifier", "Edit";
        REASON = "Raison", "Reason";
        REASON_HINT = "Facultative, visible dans le journal d'audit", "Optional, shown in the audit log";

        // Actions sur un membre.
        MENU_KICK = "Expulser", "Kick";
        MENU_BAN = "Bannir", "Ban";
        MENU_TIMEOUT = "Exclure temporairement", "Timeout";
        MENU_UNTIMEOUT = "Retirer l'exclusion", "Remove Timeout";
        KICK_TIP = "Ce membre pourra revenir avec une nouvelle invitation.", "They will be able to rejoin with a new invite.";
        BAN_TIP = "Ce membre ne pourra plus revenir tant que le bannissement n'est pas levé.", "They won't be able to rejoin unless the ban is revoked.";
        TIMEOUT_TIP = "Pendant cette durée, ce membre ne pourra ni écrire, ni réagir, ni parler en vocal.", "For this long, they won't be able to send messages, react or speak in voice.";
        UNTIMEOUT_TIP = "Ce membre pourra de nouveau écrire et parler.", "They will be able to talk again.";
        DELETE_HISTORY = "Supprimer ses messages", "Delete message history";
        DONT_DELETE = "Ne rien supprimer", "Don't delete any";
        PREV_HOUR = "De la dernière heure", "Previous hour";
        DURATION = "Durée", "Duration";

        // Suppression en masse.
        MENU_SELECT = "Sélectionner", "Select";
        SELECTION_TIP = "Cliquez sur des messages pour les ajouter ou les retirer (100 au plus).", "Click messages to add or remove them (100 at most).";
        SELECTION_FULL = "100 messages au plus par suppression.", "100 messages at most per deletion.";
        BULK_NOTE = "Discord ne supprime d'un seul coup que 2 à 100 messages de moins de 14 jours, et seulement pour un bot\u{a0}: les autres sont supprimés un par un, plus lentement.", "Discord only deletes 2 to 100 messages younger than 14 days at once, and only for bots: the others are deleted one by one, more slowly.";

        // Bannissements.
        SEARCH_BANS = "Rechercher un banni", "Search bans";
        NO_BANS = "Personne n'est banni de ce serveur.", "Nobody is banned from this server.";
        NO_MATCH = "Aucun résultat.", "No results.";
        UNBAN = "Débannir", "Revoke Ban";
        NO_REASON = "Sans raison", "No reason given";
        BULK_BAN = "Bannissement en masse", "Mass ban";
        BULK_BAN_TIP = "Identifiants des utilisateurs à bannir, un par ligne (200 au plus).", "IDs of the users to ban, one per line (200 at most).";
        BULK_BAN_BAD = "Seuls des identifiants (nombres) sont acceptés, 200 au plus.", "Only IDs (numbers) are accepted, 200 at most.";

        // Journal d'audit.
        FILTER_USER = "Auteur", "Filter by user";
        FILTER_ACTION = "Action", "Filter by action";
        ALL_USERS = "Tous", "All users";
        ALL_ACTIONS = "Toutes", "All actions";
        NO_ENTRIES = "Aucune entrée.", "No entries.";
        CHANGES = "Changements", "Changes";
        NO_CHANGES = "Aucun changement détaillé.", "No detailed changes.";
        SOMEONE = "Quelqu'un", "Someone";
        AUTOMOD = "AutoMod", "AutoMod";
        V_CREATE = "créé", "created";
        V_UPDATE = "modifié", "updated";
        V_DELETE = "supprimé", "deleted";
        VL_CREATE = "création", "created";
        VL_UPDATE = "modification", "updated";
        VL_DELETE = "suppression", "deleted";
        N_CHANNEL = "le salon", "the channel";
        N_ROLE = "le rôle", "the role";
        N_INVITE = "l'invitation", "the invite";
        N_WEBHOOK = "le webhook", "the webhook";
        N_EMOJI = "l'emoji", "the emoji";
        N_INTEGRATION = "l'intégration", "the integration";
        N_STAGE = "la scène", "the stage";
        N_STICKER = "l'autocollant", "the sticker";
        N_EVENT = "l'événement", "the event";
        N_THREAD = "le fil", "the thread";
        N_SOUND = "le son", "the sound";
        N_RULE = "la règle AutoMod", "the AutoMod rule";
        N_PERMS = "les permissions du salon", "the permissions of the channel";
        K_GUILD = "Serveur", "Server";
        K_CHANNELS = "Salons", "Channels";
        K_PERMS = "Permissions de salon", "Channel permissions";
        K_KICK = "Expulsion", "Kick";
        K_PRUNE = "Ménage des membres", "Prune";
        K_BAN = "Bannissement", "Ban";
        K_UNBAN = "Débannissement", "Unban";
        K_MEMBER = "Membre", "Member";
        K_MEMBER_ROLES = "Rôles d'un membre", "Member roles";
        K_MOVE = "Déplacement vocal", "Voice move";
        K_DISCONNECT = "Déconnexion vocale", "Voice disconnect";
        K_BOT = "Ajout de bot", "Bot added";
        K_ROLES = "Rôles", "Roles";
        K_INVITES = "Invitations", "Invites";
        K_WEBHOOKS = "Webhooks", "Webhooks";
        K_EMOJIS = "Emojis", "Emojis";
        K_MESSAGE_DELETE = "Suppression de message", "Message deleted";
        K_BULK_DELETE = "Suppression en masse", "Bulk delete";
        K_PIN = "Épinglage", "Message pinned";
        K_UNPIN = "Désépinglage", "Message unpinned";
        K_INTEGRATIONS = "Intégrations", "Integrations";
        K_STICKERS = "Autocollants", "Stickers";
        K_EVENTS = "Événements", "Events";
        K_THREADS = "Fils", "Threads";
        K_RULES = "Règles AutoMod", "AutoMod rules";
        K_AUTOMOD_BLOCK = "AutoMod, message bloqué", "AutoMod blocked a message";
        K_AUTOMOD_ALERT = "AutoMod, alerte", "AutoMod alert";
        K_AUTOMOD_TIMEOUT = "AutoMod, exclusion", "AutoMod timeout";

        // AutoMod.
        RULES = "Règles", "Rules";
        NO_RULES = "Aucune règle pour l'instant.", "No rules yet.";
        NEW_RULE = "Nouvelle règle", "Create rule";
        TR_KEYWORD = "Mots-clés personnalisés", "Custom keywords";
        TR_SPAM = "Contenu suspect de spam", "Suspected spam content";
        TR_PRESET = "Listes prédéfinies", "Commonly flagged words";
        TR_MENTION = "Spam de mentions", "Mention spam";
        TR_PROFILE = "Noms des membres", "Member profile names";
        TR_OTHER = "Règle gérée par Discord", "Rule managed by Discord";
        ENABLED = "Activée", "Enabled";
        DISABLED = "Désactivée", "Disabled";
        RULE_NAME = "Nom de la règle", "Rule name";
        KEYWORDS = "Mots-clés (un par ligne ou séparés par des virgules, * au début ou à la fin)", "Keywords (one per line or comma-separated, * at the start or end)";
        REGEX = "Expressions régulières (une par ligne, 10 au plus)", "Regex patterns (one per line, 10 at most)";
        ALLOW = "Mots autorisés", "Allowed words";
        PRESET_PROFANITY = "Grossièretés", "Profanity";
        PRESET_SEXUAL = "Contenu sexuel", "Sexual content";
        PRESET_SLURS = "Insultes", "Slurs";
        MENTION_LIMIT = "Mentions par message au plus", "Mentions per message at most";
        RAID_PROTECTION = "Détecter les raids de mentions", "Detect mention raids";
        RESPONSES = "Réponses", "Responses";
        A_BLOCK = "Bloquer le message", "Block message";
        A_CUSTOM = "Message affiché à l'auteur (facultatif)", "Custom message shown to the author (optional)";
        A_ALERT = "Envoyer une alerte dans", "Send an alert to";
        A_TIMEOUT = "Exclure temporairement l'auteur", "Timeout the member";
        A_QUARANTINE = "Bloquer les interactions du membre", "Block the member's interactions";
        PICK_CHANNEL = "Choisir un salon", "Choose a channel";
        EXEMPT_ROLES = "Rôles exemptés", "Exempt roles";
        EXEMPT_CHANNELS = "Salons exemptés", "Exempt channels";
        E_NAME = "Donnez un nom à la règle (100 caractères au plus).", "Give the rule a name (100 characters at most).";
        E_EMPTY = "Ajoutez au moins un mot-clé, un motif ou une liste.", "Add at least one keyword, pattern or list.";
        E_KEYWORDS = "1000 mots-clés au plus, de 60 caractères au plus.", "1000 keywords at most, 60 characters each at most.";
        E_REGEX = "10 motifs au plus, de 260 caractères au plus.", "10 patterns at most, 260 characters each at most.";
        E_ALLOW = "Trop de mots autorisés, ou un mot trop long.", "Too many allowed words, or one is too long.";
        E_MENTIONS = "Entre 1 et 50 mentions.", "Between 1 and 50 mentions.";
        E_NO_ACTION = "Choisissez au moins une réponse.", "Choose at least one response.";
        E_ACTION = "Une réponse n'est pas possible pour ce type de règle, ou sa valeur est hors limites.", "A response isn't allowed for this rule type, or its value is out of range.";
        E_ALERT = "Choisissez le salon des alertes.", "Choose the alert channel.";
        E_EXEMPT = "20 rôles et 50 salons exemptés au plus.", "20 exempt roles and 50 exempt channels at most.";
        E_UNSUPPORTED = "Ce type de règle se modifie dans l'application officielle.", "This rule type is edited in the official app.";
        EXECUTIONS = "Alertes reçues", "Recent AutoMod alerts";

        // Sécurité.
        VERIFICATION = "Niveau de vérification", "Verification level";
        VERIFICATION_TIP = "Ce que doit remplir un membre avant d'écrire.", "What members must meet before they can talk.";
        V0 = "Aucun, sans restriction", "None, unrestricted";
        V1 = "Faible, e-mail vérifié", "Low, verified email";
        V2 = "Moyen, inscrit depuis plus de 5 minutes", "Medium, registered for over 5 minutes";
        V3 = "Élevé, membre depuis plus de 10 minutes", "High, member for over 10 minutes";
        V4 = "Très élevé, téléphone vérifié", "Highest, verified phone";
        EXPLICIT = "Filtre de contenu explicite", "Explicit image filter";
        EXPLICIT_TIP = "Médias analysés et supprimés s'ils sont explicites.", "Media scanned and deleted if explicit.";
        E0 = "Ne rien analyser", "Don't scan any media";
        E1 = "Membres sans rôle", "Members without roles";
        E2 = "Tous les membres", "All members";
        NOTIFS = "Notifications par défaut", "Default notifications";
        NOTIFS_TIP = "Pour les membres qui n'ont rien réglé.", "For members who haven't set their own.";
        N0 = "Tous les messages", "All messages";
        N1 = "@mentions seulement", "Only @mentions";
        MFA = "Authentification à deux facteurs des modérateurs", "2FA requirement for moderation";
        MFA_ON = "Exigée", "Required";
        MFA_OFF = "Non exigée", "Not required";
        MFA_TIP = "Se change dans l'application officielle\u{a0}: Discord y demande un code.", "Changed in the official app: Discord asks for a code there.";
        OPEN_IN_DISCORD = "Ouvrir dans Discord", "Open in Discord";
        RAIDS = "Raids", "Raids";
        RAIDS_TIP = "Pause des nouvelles adhésions par invitation et des MP entre membres, 24 h au plus.", "Pause joins through invites and DMs between members, 24 h at most.";
        INVITES = "Invitations", "Invites";
        DMS = "Messages privés", "Direct messages";
        ACTIVE = "Actifs", "Active";
        PAUSE = "Mettre en pause", "Pause";
        RESUME = "Reprendre", "Resume";
        FOR = "Pendant", "For";

        // Messages de fin.
        DONE_KICK = "Membre expulsé.", "Member kicked.";
        DONE_BAN = "Membre banni.", "Member banned.";
        DONE_UNBAN = "Bannissement levé.", "Ban revoked.";
        DONE_TIMEOUT = "Exclusion temporaire mise à jour.", "Timeout updated.";
        DONE_RULE = "Règle enregistrée.", "Rule saved.";
        DONE_RULE_DELETE = "Règle supprimée.", "Rule deleted.";
        DONE_SAFETY = "Réglage enregistré.", "Setting saved.";
        DONE_INCIDENTS = "Pause des invitations et des MP mise à jour.", "Invite and DM pause updated.";
        FORBIDDEN = "Discord refuse\u{a0}: permission manquante ou rôle trop bas.", "Discord refused: missing permission or role too low.";
        ACT_KICK = "Expulsion", "Kick";
        ACT_BAN = "Bannissement", "Ban";
        ACT_UNBAN = "Débannissement", "Unban";
        ACT_TIMEOUT = "Exclusion temporaire", "Timeout";
        ACT_LOAD = "Chargement", "Loading";
        ACT_BULK_BAN = "Bannissement en masse", "Mass ban";
        ACT_BULK_DELETE = "Suppression des messages", "Message deletion";
        ACT_RULE = "Enregistrement de la règle", "Saving the rule";
        ACT_RULE_DELETE = "Suppression de la règle", "Deleting the rule";
        ACT_SAFETY = "Réglage de sécurité", "Safety setting";
        ACT_INCIDENTS = "Pause des invitations", "Invite pause";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        kick_title(name) = "Expulser {name}", "Kick {name}";
        ban_title(name) = "Bannir {name}", "Ban {name}";
        timeout_title(name) = "Exclure temporairement {name}", "Timeout {name}";
        untimeout_title(name) = "Retirer l'exclusion de {name}", "Remove timeout for {name}";
        timed_out_until(until) = "Exclu {until}", "Timed out {until}";
        previous(span) = "Des dernières {span}", "Previous {span}";
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        bulk_partial(deleted, failed) = "{deleted} supprimés, {failed} non supprimés.", "{deleted} deleted, {failed} not deleted.";
        bulk_banned(banned, failed) = "{banned} bannis, {failed} échecs.", "{banned} banned, {failed} failed.";
        reason_is(reason) = "Raison\u{a0}: {reason}", "Reason: {reason}";
        change(key, old, new) = "{key}\u{a0}: {old} > {new}", "{key}: {old} > {new}";
        change_set(key, new) = "{key}\u{a0}: {new}", "{key}: {new}";
        change_unset(key, old) = "{key}\u{a0}: {old} retiré", "{key}: {old} removed";
        label(kind, verb) = "{kind}, {verb}", "{kind}, {verb}";
        did(user, verb, noun, target) = "{user} a {verb} {noun} {target}", "{user} {verb} {noun} {target}";
        guild_update(user) = "{user} a modifié le serveur", "{user} updated the server";
        kick(user, target) = "{user} a expulsé {target}", "{user} kicked {target}";
        prune(user) = "{user} a fait le ménage des membres inactifs", "{user} pruned inactive members";
        ban(user, target) = "{user} a banni {target}", "{user} banned {target}";
        unban(user, target) = "{user} a levé le bannissement de {target}", "{user} unbanned {target}";
        member_update(user, target) = "{user} a modifié le membre {target}", "{user} updated the member {target}";
        member_roles(user, target) = "{user} a modifié les rôles de {target}", "{user} updated the roles of {target}";
        member_move(user) = "{user} a déplacé des membres en vocal", "{user} moved members in voice";
        member_disconnect(user) = "{user} a déconnecté des membres du vocal", "{user} disconnected members from voice";
        bot_add(user, target) = "{user} a ajouté le bot {target}", "{user} added the bot {target}";
        message_delete(user, target) = "{user} a supprimé un message de {target}", "{user} deleted a message by {target}";
        bulk_delete(user) = "{user} a supprimé des messages en masse", "{user} bulk deleted messages";
        pin(user, target) = "{user} a épinglé un message de {target}", "{user} pinned a message by {target}";
        unpin(user, target) = "{user} a désépinglé un message de {target}", "{user} unpinned a message by {target}";
        commands(user) = "{user} a modifié les permissions des commandes", "{user} updated command permissions";
        onboarding(user) = "{user} a modifié l'accueil du serveur", "{user} updated the onboarding";
        automod_block(target) = "AutoMod a bloqué un message de {target}", "AutoMod blocked a message by {target}";
        automod_alert(target) = "AutoMod a signalé un message de {target}", "AutoMod flagged a message by {target}";
        automod_timeout(target) = "AutoMod a exclu temporairement {target}", "AutoMod timed out {target}";
        automod_quarantine(target) = "AutoMod a restreint {target}", "AutoMod restricted {target}";
        other(user, n) = "{user} a effectué l'action n° {n}", "{user} performed action #{n}";
        confirm_delete_rule(name) = "Supprimer la règle {name}\u{a0}? C'est définitif.", "Delete the rule {name}? This can't be undone.";
        execution(user, matched) = "{user}\u{a0}: «\u{a0}{matched}\u{a0}»", "{user}: “{matched}”";
        paused_until(until) = "En pause {until}", "Paused {until}";
        raid_detected(time) = "Raid détecté\u{a0}: {time}", "Raid detected: {time}";
        confirm_pause(what, span) = "Mettre en pause {what} pendant {span}\u{a0}? Cela se voit par tout le serveur.", "Pause {what} for {span}? The whole server will see it.";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        seconds = ["{n} seconde", "{n} secondes"], ["{n} second", "{n} seconds"];
        minutes = ["{n} minute", "{n} minutes"], ["{n} minute", "{n} minutes"];
        hours = ["{n} heure", "{n} heures"], ["{n} hour", "{n} hours"];
        days = ["{n} jour", "{n} jours"], ["{n} day", "{n} days"];
        weeks = ["{n} semaine", "{n} semaines"], ["{n} week", "{n} weeks"];
        selected = ["{n} message sélectionné", "{n} messages sélectionnés"], ["{n} message selected", "{n} messages selected"];
        confirm_bulk_delete = ["Supprimer ce message\u{a0}? C'est définitif.", "Supprimer ces {n} messages\u{a0}? C'est définitif."], ["Delete this message? This can't be undone.", "Delete these {n} messages? This can't be undone."];
        bulk_deleted = ["{n} message supprimé.", "{n} messages supprimés."], ["{n} message deleted.", "{n} messages deleted."];
        ban_n = ["Bannir {n} utilisateur", "Bannir {n} utilisateurs"], ["Ban {n} user", "Ban {n} users"];
        confirm_bulk_ban = ["Bannir {n} utilisateur de ce serveur\u{a0}?", "Bannir ces {n} utilisateurs de ce serveur\u{a0}?"], ["Ban {n} user from this server?", "Ban these {n} users from this server?"];
        bans = ["{n} banni", "{n} bannis"], ["{n} ban", "{n} bans"];
        keywords = ["{n} mot-clé", "{n} mots-clés"], ["{n} keyword", "{n} keywords"];
        mentions = ["{n} mention au plus", "{n} mentions au plus"], ["{n} mention at most", "{n} mentions at most"];
    }
}
