//! Textes du lot T13 : sondages, messages système, salons d'annonces,
//! cartes d'invitation. Module enfant de `table` (il en reprend les macros),
//! tenu à part pour la fusion.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        VOTE = "Voter", "Vote";
        REMOVE_VOTE = "Retirer mon vote", "Remove vote";
        END_POLL = "Terminer le sondage", "End poll";
        POLL_ENDED = "Sondage terminé", "Poll closed";
        SELECT_ONE = "Choisissez une réponse", "Select one answer";
        SELECT_MANY = "Choisissez une ou plusieurs réponses", "Select one or more answers";
        SHOW_VOTERS = "Voir les votants", "Show voters";
        NO_VOTERS = "Aucun votant.", "No voters.";
        COUNTS_UNKNOWN = "Résultats pas encore connus", "Results not known yet";
        POLL_TIP = "Sondage", "Poll";
        CREATE_POLL = "Créer un sondage", "Create a poll";
        QUESTION = "Question", "Question";
        QUESTION_HINT = "Que voulez-vous demander\u{a0}?", "What question do you want to ask?";
        ANSWERS = "Réponses", "Answers";
        ANSWER_HINT = "Saisissez votre réponse", "Type your answer";
        ADD_ANSWER = "Ajouter une réponse", "Add another answer";
        REMOVE_ANSWER = "Retirer cette réponse", "Remove this answer";
        DURATION = "Durée", "Duration";
        MULTI = "Autoriser plusieurs réponses", "Allow multiple answers";
        POST = "Publier", "Post";
        CANCEL = "Annuler", "Cancel";
        CLOSE = "Fermer", "Close";
        ACTION_VOTE = "Vote", "Vote";
        ACTION_END = "Fin du sondage", "Ending the poll";
        ACTION_CREATE = "Création du sondage", "Creating the poll";
        ACTION_PUBLISH = "Publication du message", "Publishing the message";
        ACTION_FOLLOW = "Abonnement au salon", "Following the channel";
        SYS_DISCOVERY_DISQUALIFIED = "Ce serveur a été retiré de la page Découverte.", "This server has been removed from Server Discovery.";
        SYS_DISCOVERY_REQUALIFIED = "Ce serveur peut de nouveau figurer sur la page Découverte.", "This server is eligible for Server Discovery again.";
        SYS_DISCOVERY_WARNING = "Ce serveur ne remplit plus les conditions de la page Découverte.", "This server no longer meets the Server Discovery requirements.";
        SYS_INVITE_REMINDER = "Invitez vos amis pour faire vivre ce serveur.", "Invite your friends to bring this server to life.";
        SYS_AUTOMOD = "AutoMod a bloqué un message.", "AutoMod blocked a message.";
        FEW_SECONDS = "quelques secondes", "a few seconds";
        PUBLISH = "Publier", "Publish";
        PUBLISHED = "Publié", "Published";
        PUBLISHED_OK = "Message publié dans les salons qui suivent celui-ci.", "Message published to the following channels.";
        FOLLOW = "Suivre", "Follow";
        FOLLOW_TITLE = "Suivre ce salon d'annonces", "Follow this announcement channel";
        FOLLOW_TIP = "Ses messages publiés s'afficheront dans le salon choisi.", "Its published messages will appear in the channel you pick.";
        NO_FOLLOW_TARGET = "Aucun salon de vos serveurs où vous pouvez gérer les webhooks.", "No channel in your servers where you can manage webhooks.";
        FOLLOWED = "Salon suivi.", "Channel followed.";
        FROM_FOLLOWED = "Publié depuis un salon suivi", "Published from a followed channel";
        SERVER_TAG = "SERVEUR", "SERVER";
        INVITE_HEAD = "Vous avez été invité à rejoindre un serveur", "You've been invited to join a server";
        INVITE_GROUP = "Invitation à un groupe", "Group invite";
        INVITE_FRIEND = "Invitation d'ami", "Friend invite";
        INVITE_LOADING = "Chargement de l'invitation…", "Loading invite…";
        INVITE_INVALID = "Invitation invalide ou expirée", "Invalid or expired invite";
        INVITE_CAPTCHA = "Discord demande une vérification\u{a0}: rejoignez ce serveur depuis l'application officielle.", "Discord requires a verification: join this server from the official app.";
        JOIN = "Rejoindre", "Join";
        JOINED = "Rejoint", "Joined";
        OPEN = "Ouvrir", "Open";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        failed(action, reason) = "{action} impossible\u{a0}: {reason}", "{action} failed: {reason}";
        percent(n) = "{n}\u{a0}%", "{n}%";
        voters_of(answer) = "Votants\u{a0}: {answer}", "Voters: {answer}";
        command_used(name, command) = "{name} a utilisé {command}", "{name} used {command}";
        join_0(name) = "{name} vient d'arriver.", "{name} just arrived.";
        join_1(name) = "Bienvenue, {name}\u{a0}!", "Welcome, {name}!";
        join_2(name) = "{name} a rejoint le serveur.", "{name} joined the server.";
        join_3(name) = "{name} est là. Dites bonjour\u{a0}!", "{name} is here. Say hi!";
        join_4(name) = "Faites de la place, {name} arrive.", "Make some room, {name} is here.";
        join_5(name) = "{name} vient de se joindre à nous.", "{name} just joined us.";
        join_6(name) = "Un nouveau visage\u{a0}: {name}.", "A new face: {name}.";
        join_7(name) = "{name} a passé la porte.", "{name} walked in.";
        recipient_add(name, target) = "{name} a ajouté {target} au groupe.", "{name} added {target} to the group.";
        recipient_remove(name, target) = "{name} a retiré {target} du groupe.", "{name} removed {target} from the group.";
        recipient_left(name) = "{name} a quitté le groupe.", "{name} left the group.";
        call_started(name) = "{name} a lancé un appel.", "{name} started a call.";
        call_lasted(name, duration) = "{name} a lancé un appel qui a duré {duration}.", "{name} started a call that lasted {duration}.";
        call_missed(name) = "Vous avez manqué un appel de {name}.", "You missed a call from {name}.";
        channel_name(name, value) = "{name} a renommé le salon\u{a0}: {value}", "{name} changed the channel name: {value}";
        channel_icon(name) = "{name} a changé l'icône du groupe.", "{name} changed the group icon.";
        pinned(name) = "{name} a épinglé un message dans ce salon.", "{name} pinned a message to this channel.";
        boost(name) = "{name} a boosté le serveur\u{a0}!", "{name} boosted the server!";
        boost_times(name, n) = "{name} a boosté le serveur {n} fois\u{a0}!", "{name} boosted the server {n} times!";
        boost_tier(name, guild, tier) = "{name} a boosté le serveur\u{a0}! {guild} passe au niveau {tier}\u{a0}!", "{name} boosted the server! {guild} has reached Level {tier}!";
        follow_add(name, value) = "{name} a ajouté {value} à ce salon\u{a0}: ses publications s'afficheront ici.", "{name} added {value} to this channel: its published messages will show up here.";
        thread_created(name, value) = "{name} a créé un fil\u{a0}: {value}", "{name} started a thread: {value}";
        stage_start(name, value) = "{name} a lancé la scène {value}", "{name} started the stage {value}";
        stage_end(name, value) = "{name} a terminé la scène {value}", "{name} ended the stage {value}";
        stage_speaker(name) = "{name} intervient maintenant sur la scène.", "{name} is now a speaker.";
        stage_topic(name, value) = "{name} a changé le sujet de la scène\u{a0}: {value}", "{name} changed the stage topic: {value}";
        role_subscription(name) = "{name} a pris un abonnement de rôle.", "{name} joined a role subscription.";
        app_subscription(name) = "{name} s'est abonné à une application du serveur.", "{name} subscribed to a server app.";
        purchase(name) = "{name} a fait un achat sur le serveur.", "{name} made a purchase in the server.";
        incident_on(name) = "{name} a activé les mesures de sécurité du serveur.", "{name} enabled the server security actions.";
        incident_off(name) = "{name} a désactivé les mesures de sécurité du serveur.", "{name} disabled the server security actions.";
        incident_raid(name) = "{name} a signalé un raid sur le serveur.", "{name} reported a raid in the server.";
        incident_false(name) = "{name} a signalé une fausse alerte.", "{name} reported a false alarm.";
        poll_won(name, question, answer, votes, total) = "Le sondage de {name} est terminé («\u{a0}{question}\u{a0}»)\u{a0}: {answer} l'emporte avec {votes} sur {total}.", "{name}'s poll has closed (“{question}”): {answer} won with {votes} of {total}.";
        poll_tie(name, question) = "Le sondage de {name} est terminé («\u{a0}{question}\u{a0}»)\u{a0}: égalité.", "{name}'s poll has closed (“{question}”): it's a tie.";
        poll_empty(name, question) = "Le sondage de {name} est terminé («\u{a0}{question}\u{a0}»)\u{a0}: aucun vote.", "{name}'s poll has closed (“{question}”): no votes.";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        votes = ["{n} vote", "{n} votes"], ["{n} vote", "{n} votes"];
        minutes = ["{n} minute", "{n} minutes"], ["{n} minute", "{n} minutes"];
        minutes_left = ["{n} minute restante", "{n} minutes restantes"], ["{n} minute left", "{n} minutes left"];
        hours_left = ["{n} heure restante", "{n} heures restantes"], ["{n} hour left", "{n} hours left"];
        days_left = ["{n} jour restant", "{n} jours restants"], ["{n} day left", "{n} days left"];
        hours = ["{n} heure", "{n} heures"], ["{n} hour", "{n} hours"];
        days = ["{n} jour", "{n} jours"], ["{n} day", "{n} days"];
        online = ["{n} en ligne", "{n} en ligne"], ["{n} online", "{n} online"];
        members = ["{n} membre", "{n} membres"], ["{n} member", "{n} members"];
    }
}
