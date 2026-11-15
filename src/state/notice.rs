//! Statuts, erreurs et notifications sous forme de valeurs typées : le
//! réseau et le magasin ne choisissent aucun mot, l'interface les met en
//! mots dans la langue courante (`ui/words.rs`).

/// État de la connexion, affiché dans l'en-tête tant qu'il n'est pas `Connected`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    /// Aucune connexion lancée (rien à afficher).
    #[default]
    Idle,
    Connecting,
    Resuming,
    /// Session établie : l'interface ne l'affiche pas.
    Connected,
    /// Plafond journalier d'Identify atteint ; nouvel essai à cette heure (ms Unix).
    IdentifyCap { retry_ms: i64 },
    /// Paquet trop gros pour être lu : nouvelle tentative.
    PacketTooLarge,
    /// Session perdue, nouvelle tentative (sans compression si `no_compression`).
    Disconnected { cause: Cause, no_compression: bool },
}

/// Cause d'une déconnexion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Cause {
    NoSessionState,
    BadHello,
    CorruptStream,
    TooLargePacket,
    BadHeartbeat { ms: u64 },
    /// Décodage hors de la boucle interrompu ; détail technique (anglais, tel quel).
    DecodeInterrupted(String),
    /// Erreur du transport (WebSocket, TLS…) : texte technique, tel quel.
    Transport(String),
}

/// Échec d'une requête ou de la connexion.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    /// Jeton refusé (code 4004 ou HTTP 401).
    BadToken,
    /// Refus 403 d'un salon.
    Forbidden,
    /// Attente au-delà du plafond, ou trop de requêtes en attente.
    TooManyRequests,
    /// Trop de salons avec des messages en attente d'envoi.
    TooManyQueues,
    /// Erreur réseau ; détail technique (anglais, tel quel).
    Network(String),
    /// Statut HTTP sans message lisible.
    Http(u16),
    /// Message d'erreur renvoyé par Discord (dans sa langue), ou réponse
    /// illisible : texte venu de l'extérieur, affiché tel quel.
    Api(String),
    /// Pare-feu de Discord : rien ne part avant cette heure (ms Unix).
    Blocked { retry_ms: i64 },
    /// Connexion refusée par Discord (codes 4010 à 4014).
    Refused { code: u16 },
    /// READY illisible ; détail technique.
    ReadyUnreadable(String),
    /// Exclusion temporaire jusqu'à cette heure (ms Unix).
    Timeout { until_ms: i64 },
    /// Le serveur exige l'authentification à deux facteurs pour cette
    /// action (code 60003) : à faire dans l'application officielle.
    MfaRequired,
    /// Modification du profil refusée pour une raison reconnue.
    Profile(ProfileRefusal),
}

/// Refus d'une modification du profil, reconnu au corps d'erreur de Discord
/// (`net::account::refusal`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileRefusal {
    /// Nom d'affichage ou pseudo changé trop souvent.
    NameRateLimit,
    /// Avatar ou bannière changés trop souvent.
    ImageRateLimit,
    ImageTooLarge,
    /// Réservé aux abonnés Nitro.
    Nitro,
}

/// Notification à afficher brièvement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Notice {
    /// Message non envoyé, pour cette raison.
    NotSent(Problem),
    /// Envoi non confirmé : le message est peut-être parti.
    Unconfirmed,
    /// Le salon ouvert est devenu inaccessible.
    LostAccess,
    Error(Problem),
    /// Réglage du compte refusé par Discord : rétabli à sa valeur d'avant.
    SettingNotSaved(Problem),
    /// Réglage modifié ailleurs entre-temps : l'écriture a été écartée.
    SettingOutdated,
}

/// Message système (le texte affiché dépend de la langue). Types de
/// l'objet Message documentés ; leur texte est dans `state::system_msgs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemKind {
    Joined,
    Boosted,
    Pinned,
    Other,
    RecipientAdd,
    RecipientRemove,
    Call,
    ChannelName,
    ChannelIcon,
    /// Boost qui fait passer le serveur à ce niveau (1 à 3).
    BoostTier(u8),
    FollowAdd,
    DiscoveryDisqualified,
    DiscoveryRequalified,
    DiscoveryWarning,
    ThreadCreated,
    InviteReminder,
    AutoMod,
    RoleSubscription,
    StageStart,
    StageEnd,
    StageSpeaker,
    StageTopic,
    AppSubscription,
    IncidentOn,
    IncidentOff,
    IncidentRaid,
    IncidentFalseAlarm,
    Purchase,
    PollResult,
}
