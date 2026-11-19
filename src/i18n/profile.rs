//! Textes du profil complet, de la note et du menu d'un utilisateur. Module
//! enfant de `table` (il en reprend les macros), tenu à part pour que les
//! ajouts des lots parallèles ne se mêlent pas.

/// Textes constants.
pub mod t {
    use super::super::super::Text;

    texts! {
        FULL_PROFILE = "Voir le profil complet", "View Full Profile";
        ABOUT_ME = "À propos de moi", "About Me";
        TAB_ABOUT = "À propos", "About Me";
        TAB_ACTIVITY = "Activité", "Activity";
        TAB_SERVERS = "Serveurs en commun", "Mutual Servers";
        TAB_FRIENDS = "Amis en commun", "Mutual Friends";
        NOTE = "Note", "Note";
        NOTE_HINT = "Cliquez pour ajouter une note", "Click to add a note";
        NOTE_SAVING = "Enregistrement…", "Saving…";
        NOTE_FAILED = "Note non enregistrée.", "Note not saved.";
        NITRO_SINCE = "Nitro depuis", "Nitro Since";
        GUILD_BOOST_SINCE = "Boost de serveurs depuis", "Server Boosting Since";
        CONNECTIONS = "Comptes liés", "Connections";
        DOMAIN = "Domaine", "Domain";
        VERIFIED = "Vérifié", "Verified";
        PRONOUNS = "Pronoms", "Pronouns";
        NO_ACTIVITY = "Aucune activité pour l'instant.", "No activity right now.";
        NO_SERVERS = "Aucun serveur en commun.", "No servers in common.";
        NO_FRIENDS = "Aucun ami en commun.", "No friends in common.";
        UNAVAILABLE = "Discord ne fournit pas ces informations pour ce profil.", "Discord does not share this information for this profile.";
        LOADING = "Chargement…", "Loading…";
        CAPTCHA = "Discord demande une vérification (captcha)\u{a0}: ouvrez ce profil dans l'application officielle.", "Discord asks for a verification (captcha): open this profile in the official app.";
        NOTE_CAPTCHA = "Discord demande une vérification (captcha)\u{a0}: modifiez cette note dans l'application officielle.", "Discord asks for a verification (captcha): edit this note in the official app.";
        CLOSE = "Fermer", "Close";
        MENU_PROFILE = "Profil", "Profile";
        MENU_MESSAGE = "Message", "Message";
        MENU_MENTION = "Mentionner", "Mention";
        MENU_NOTE = "Note", "Note";
        MENU_ADD_FRIEND = "Ajouter en ami", "Add Friend";
        MENU_ACCEPT = "Accepter la demande d'ami", "Accept Friend Request";
        MENU_CANCEL = "Annuler la demande d'ami", "Cancel Friend Request";
        MENU_REMOVE = "Retirer l'ami", "Remove Friend";
        MENU_BLOCK = "Bloquer", "Block";
        MENU_UNBLOCK = "Débloquer", "Unblock";
        MENU_COPY_ID = "Copier l'identifiant", "Copy User ID";
        YES = "Oui", "Yes";
        NO = "Non", "No";
        // Carte de profil (lot profil) : infobulle des badges, boutons de la bannière.
        MEMBER_SINCE = "Membre depuis", "Member Since";
        CREATED_ON = "Créé le", "Created on";
        BIO_MORE = "Afficher la bio complète", "View Full Bio";
        BIO_LESS = "Réduire la bio", "Collapse bio";
        BADGES_MORE = "Afficher tous les badges", "Show all badges";
        GAME_COLLECTION = "Collection de jeux", "Game Collection";
        ROLES = "Rôles", "Roles";
        BADGE_ACCOUNT_AGE = "Ancienneté du compte", "Account Age";
        BADGE_SUBSCRIBER_SINCE = "Abonné depuis", "Subscriber since";
        BADGE_BOOST_SINCE = "Boost de serveur depuis", "Server boosting since";
        TIER_BRONZE = "Bronze", "Bronze";
        TIER_SILVER = "Argent", "Silver";
        TIER_GOLD = "Or", "Gold";
        TIER_PLATINUM = "Platine", "Platinum";
        TIER_DIAMOND = "Diamant", "Diamond";
        TIER_EMERALD = "Émeraude", "Emerald";
        TIER_RUBY = "Rubis", "Ruby";
        TIER_OPAL = "Opale", "Opal";
        ON_MOBILE = "En ligne sur mobile", "Online on mobile";
        FRIEND_ADD = "Ajouter en ami", "Add Friend";
        FRIEND_YES = "Ami", "Friend";
        FRIEND_SENT = "Demande d'ami envoyée", "Friend Request Sent";
        FRIEND_RECEIVED = "Demande d'ami reçue", "Friend Request Received";
        MORE = "Plus", "More";
        CUSTOM_STATUS = "Statut personnalisé", "Custom Status";
        PANEL_ACTIVITY = "Activité en cours", "Current Activity";
        PANEL_INPUT_MENU = "Options d'entrée", "Input Options";
        PANEL_OUTPUT_MENU = "Options de sortie", "Output Options";
        WINDOWS_DEFAULT = "Paramètres audio Windows par défaut", "Windows Default Audio Settings";
        INPUT_PROFILE = "Profil d'entrée audio", "Input Profile";
        PROFILE_ISOLATION = "Isolement de la voix", "Voice Isolation";
        PROFILE_STUDIO = "Studio", "Studio";
        PROFILE_CUSTOM = "Personnalisé", "Custom";
        NOISE_RNN = "Suppression du bruit (RNNoise)", "Noise suppression (RNNoise)";
        VOICE_SETTINGS = "Paramètres vocaux", "Voice Settings";
    }
}

/// Textes à paramètres nommés.
pub mod f {
    use super::super::super::{fill::fill, Lang};

    templates! {
        legacy(name) = "Anciennement {name}", "Formerly {name}";
        tab_count(tab, n) = "{tab} ({n})", "{tab} ({n})";
        tenure(years, months) = "{years}, {months}", "{years}, {months}";
        tier_rank(n, of) = "Palier {n} sur {of}", "Tier {n} of {of}";
        next_tier(name, date) = "Prochain palier\u{a0}: {name}, le {date}", "Next tier: {name}, on {date}";
        boost_level(n) = "Niveau {n}", "Level {n}";
        connection(service, name) = "{service}\u{a0}: {name}", "{service}: {name}";
    }
}

/// Pluriels.
pub mod p {
    use super::super::super::{fill::fill, number, Lang};

    plurals! {
        servers = ["{n} serveur en commun", "{n} serveurs en commun"], ["{n} mutual server", "{n} mutual servers"];
        friends = ["{n} ami en commun", "{n} amis en commun"], ["{n} mutual friend", "{n} mutual friends"];
        years = ["{n} an", "{n} ans"], ["{n} year", "{n} years"];
        months = ["{n} mois", "{n} mois"], ["{n} month", "{n} months"];
        days = ["{n} jour", "{n} jours"], ["{n} day", "{n} days"];
    }
}

/// Nom d'un service de comptes liés (marques, identiques dans les deux
/// langues) ; `None` : service inconnu, son code est affiché.
pub fn service(kind: &str) -> Option<&'static str> {
    Some(match kind {
        "amazon-music" => "Amazon Music",
        "battlenet" => "Battle.net",
        "bluesky" => "Bluesky",
        "bungie" => "Bungie.net",
        "crunchyroll" => "Crunchyroll",
        "ebay" => "eBay",
        "epicgames" => "Epic Games",
        "facebook" => "Facebook",
        "github" => "GitHub",
        "instagram" => "Instagram",
        "leagueoflegends" => "League of Legends",
        "mastodon" => "Mastodon",
        "paypal" => "PayPal",
        "playstation" | "playstation-stg" => "PlayStation Network",
        "reddit" => "Reddit",
        "riotgames" => "Riot Games",
        "roblox" => "Roblox",
        "samsung" => "Samsung Galaxy",
        "skype" => "Skype",
        "soundcloud" => "SoundCloud",
        "spotify" => "Spotify",
        "steam" => "Steam",
        "tiktok" => "TikTok",
        "twitch" => "Twitch",
        "twitter" => "X",
        "xbox" => "Xbox",
        "youtube" => "YouTube",
        _ => return None,
    })
}
