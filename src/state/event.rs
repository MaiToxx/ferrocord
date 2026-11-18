//! Messages échangés entre le réseau et le magasin. Ils vivent dans `state/`
//! pour que le magasin ne dépende pas de `net/` : c'est `net/` qui les importe.

use super::model::{Attachment, Channel, Dm, Embed, Emoji, Guild, ImageRef, Member, Message, Role, User};
use super::notice::{Problem, Status};
use super::people::PeopleHub;
use super::usersettings::{AccountCmd, AccountEvent};

/// Taille d'une page d'historique : une page pleine signifie qu'il en reste.
pub const HISTORY_PAGE: usize = 50;

/// Réseau -> interface
#[derive(Debug)]
pub enum Event {
    Status(Status),
    /// Tables des utilisateurs, membres et présences de cette connexion
    /// (partagées avec le réseau) ; premier événement de chaque connexion.
    PeopleHub(PeopleHub),
    /// MP créé ou retrouvé à la demande de l'utilisateur (« Envoyer un message ») : à ouvrir.
    DmOpened { channel_id: u64 },
    Ready { me: User, guilds: Vec<Guild>, dms: Vec<Dm> },
    /// USER_UPDATE : mon compte a changé (nom, avatar), ici ou sur un autre appareil.
    MeUpdate(User),
    GuildCreate(Guild),
    /// GUILD_UPDATE : propriétaire et icône. `owner_id` : `None` si absent du
    /// paquet (rien ne change), `Some(None)` si illisible (propriétaire inconnu).
    /// `icon` : `None` si absent (rien ne change), `Some(None)` si retirée ou illisible.
    GuildUpdate { id: u64, owner_id: Option<Option<u64>>, icon: Option<Option<ImageRef>> },
    /// Un rôle de ce serveur est arrivé illisible (identifiant compris) :
    /// mes permissions y deviennent inconnues.
    GuildRolesUnreadable { guild_id: u64 },
    /// Heure de Discord (ms Unix), lue dans l'en-tête `Date` d'une réponse REST.
    ServerTime(i64),
    GuildDelete(u64),
    RoleUpsert { guild_id: u64, role: Role },
    RoleDelete { guild_id: u64, role_id: u64 },
    /// Rôle reçu avec des permissions illisibles : retiré du serveur mais pas
    /// de mon membre, dont les permissions deviennent inconnues.
    RoleUnreadable { guild_id: u64, role_id: u64 },
    /// GUILD_MEMBER_UPDATE ; le magasin ignore les membres autres que moi.
    /// `member` vaut `None` si le paquet était illisible : mes permissions
    /// dans ce serveur deviennent inconnues.
    MemberUpdate { guild_id: u64, user_id: u64, member: Option<Member> },
    /// MP créé ou modifié ; `known` : ce que le paquet apporte vraiment (un
    /// CHANNEL_UPDATE peut n'avoir ni destinataires ni icône).
    DmUpsert { dm: Dm, known: DmKnown },
    ChannelUpsert { guild_id: u64, channel: Channel },
    ChannelDelete { guild_id: Option<u64>, id: u64 },
    MessageCreate(Message),
    MessageUpdate { channel_id: u64, id: u64, content: String },
    MessageDelete { channel_id: u64, id: u64 },
    /// MESSAGE_UPDATE qui apporte des pièces jointes ou des embeds (aperçu
    /// de lien ajouté par Discord, pièce jointe retirée) : `None` = absent du
    /// paquet, rien ne change. Ne marque pas le message comme modifié.
    MessageMedia { channel_id: u64, id: u64, attachments: Option<Vec<Attachment>>, embeds: Option<Vec<Embed>> },
    History { channel_id: u64, before: Option<u64>, messages: Vec<Message> },
    /// Page de messages postérieurs à un message (`after`), en réponse à `Cmd::HistoryAfter`.
    /// `after` : le repère demandé (une page dont le repère n'est plus le bord
    /// de la fenêtre en mémoire est périmée et ignorée).
    HistoryAfter { channel_id: u64, after: u64, messages: Vec<Message> },
    /// Envoi refusé ou impossible (le message n'est pas parti) ; `content` est
    /// rendu à l'utilisateur ; `forbidden` : refus 403 (permission ou
    /// exclusion temporaire).
    SendError { channel_id: u64, nonce: u64, content: String, error: Problem, forbidden: bool },
    /// Issue inconnue (délai dépassé après l'envoi) : le message est peut-être
    /// parti. Un nouvel essai avec le même `nonce` ne crée pas de doublon.
    SendUnconfirmed { channel_id: u64, nonce: u64, content: String },
    /// Discord a confirmé le message de ce `nonce` (réponse, ou MESSAGE_CREATE).
    SendConfirmed { channel_id: u64, nonce: u64 },
    /// Échec d'une page d'historique ; `before` / `after` : ceux de la demande
    /// (aucun des deux : dernière page), pour ne terminer que celle-là.
    HistoryError { channel_id: u64, before: Option<u64>, after: Option<u64>, error: Problem },
    /// Demande d'historique abandonnée avant de partir (salon quitté) : rien à
    /// afficher. `before` / `after` : ceux de la demande, dont seul le
    /// chargement se termine (passé ou présent).
    HistoryCancelled { channel_id: u64, before: Option<u64>, after: Option<u64> },
    /// GUILD_EMOJIS_UPDATE : la liste complète remplace l'ancienne.
    GuildEmojis { guild_id: u64, emojis: Vec<Emoji> },
    /// TYPING_START ; `name` : nom affiché si le paquet le donne (serveur).
    TypingStart { channel_id: u64, user_id: u64, name: Option<String> },
    /// Relations, MP de groupe, présences des amis, accusés de lecture.
    Relations(super::relations::RelationsEvent),
    Error(Problem),
    /// Connexion impossible à rétablir (jeton invalide, etc.)
    Fatal { problem: Problem, bad_token: bool },
    /// Réactions, épingles, modification, suppression (lots T5/T6).
    Social(super::social::SocialEvent),
    /// GUILD_MEMBER_LIST_UPDATE (comptes utilisateur, après souscription).
    MemberList(super::members::ListUpdate),
    /// Destinataires d'un MP de groupe (moi non compris).
    GroupRecipients { channel_id: u64, users: Vec<User> },
    /// Compte bot : les listes de membres ne sont pas envoyées.
    MemberListUnavailable,
    /// Réglages du compte (`state::usersettings`).
    Account(AccountEvent),
    /// Envoi de fichiers : progression, fin, échec (`state::upload`).
    Upload(super::upload::UploadEvent),
    /// Fils de discussion et forums (`state::threads`).
    Threads(super::threads::ThreadEvent),
    /// Autocollants des serveurs (`state::emoji`).
    Stickers(super::emoji::StickerEvent),
    /// Rejoindre, quitter, créer un serveur, invitations (`state::servers`).
    Servers(super::servers::ServerEvent),
    /// Recherche de messages et saut vers un message (`state::search`).
    Search(super::search::SearchEvent),
    /// Mentions récentes de la boîte de réception (`state::inbox`).
    Inbox(super::inbox::InboxEvent),
    /// Commandes et composants des applications (`state::interactions`).
    Interactions(super::interactions::InteractionEvent),
    /// Sondages, invitations, annonces (lot T13).
    Polls(super::polls::PollEvent),
    Invites(super::invites::InviteEvent),
    Announce(super::announce::AnnounceEvent),
    /// Paramètres du serveur, webhooks, intégrations (lot A8 + A9).
    GuildAdmin(super::admin::guild::AdminEvent),
    /// Salons et catégories : création, modification, suppression, déplacement (lot A2).
    ChannelAdmin(super::admin::channels::ChanEvent),
    /// Rôles et membres d'un serveur (lot A3, `state::admin::roles`).
    Roles(super::admin::roles::RolesEvent),
    /// Modération, journal d'audit, AutoMod, sécurité (lots A4 et A5).
    Moderation(super::admin::moderation::ModEvent),
    /// Événements programmés, accueil des nouveaux membres (lot A7).
    GuildEvents(super::admin::events::EventEvent),
    Community(super::admin::community::CommunityEvent),
    /// Emojis, autocollants et sons du serveur (lot A6).
    Expressions(super::admin::expressions::ExprEvent),
    /// Vocal : salons, états, ma connexion (phase 4, `state::voice`).
    Voice(super::voice::VoiceEvent),
    /// Appels privés, états vocaux, sons joués (lot V6).
    Calls(super::calls::CallEvent),
    /// Conférences et statut des salons vocaux (lot V6).
    Stage(super::stage::StageEvent),
    /// Directs : STREAM_*, connexion du direct (phase 5, `state::stream`).
    Stream(super::stream::StreamEvent),
    /// Jeux détectables, jeux en cours, boutons d'activité (lot C3).
    Activity(super::activity::ActivityEvent),
    /// Sélecteur de GIF, favoris et fréquence du compte, super réactions (lot G, `state::gifs`).
    Gifs(super::gifs::GifEvent),
    /// Effets de profil : configuration, effets possédés (lot ND, `state::effects`).
    Effects(super::effects::EffectEvent),
    /// Quêtes et solde d'Orbes (lot ND, `state::quests`).
    Quests(super::quests::QuestEvent),
    /// Choix du tag de serveur (lot ND, `state::tags`).
    Tags(super::tags::TagEvent),
    /// Widgets de jeux du profil (`state::widgets`).
    Widgets(super::widgets::WidgetEvent),
    /// Boutique en consultation (lot ND, `state::shop`).
    Shop(super::shop::ShopEvent),
    /// Inventaire du compte, en lecture (lot ND, `state::inventory`).
    Inventory(super::inventory::InventoryEvent),
    /// Suggestions d'amis (lot ND, `state::suggestions`).
    Suggestions(super::suggestions::SuggestionEvent),
    /// Messages enregistrés et rappels (lot ND, `state::saved`).
    Saved(super::saved::SavedEvent),
    /// Centre de sécurité, en lecture (lot ND, `state::safety`).
    SafetyHub(super::safety::SafetyEvent),
    /// Répertoire d'applications, en lecture (lot ND, `state::appdir`).
    AppDir(super::appdir::AppDirEvent),
    /// Vérification des membres et demandes d'adhésion (lot ND, `state::verification`).
    Verification(super::verification::VerifyEvent),
    /// Résumés de salon, en lecture (lot ND, `state::summaries`).
    Summaries(super::summaries::SummaryEvent),
    /// Découverte de serveurs, en lecture (lot ND, `state::discovery`).
    Discovery(super::discovery::DiscoveryEvent),
}

/// Interface -> réseau
#[derive(Debug, PartialEq, Eq)]
pub enum Cmd {
    History { channel_id: u64, before: Option<u64> },
    /// Page qui suit `after` : la fenêtre du cache redescend vers le présent.
    HistoryAfter { channel_id: u64, after: u64 },
    /// `nonce` : choisi par le magasin, gardé pour un nouvel essai (Discord
    /// refuse de créer deux messages du même nonce, `enforce_nonce`).
    Send { channel_id: u64, content: String, nonce: u64 },
    /// Réactions, réponses, transferts, épingles, modification, suppression (lots T5/T6).
    Social(super::social::SocialCmd),
    /// « En train d'écrire » dans ce salon (pendant la frappe, 8 s d'écart au moins).
    Typing { channel_id: u64 },
    /// « Envoyer un message » depuis une carte de profil : `POST /users/@me/channels`.
    OpenDm { user_id: u64 },
    /// Profil complet et note d'un utilisateur (`state::profiles`).
    Profile(super::profiles::ProfileCmd),
    /// Souscription à la liste des membres (passerelle, op 37).
    Subscribe(super::members::Subscription),
    /// Réglages du compte (`state::usersettings`).
    Account(AccountCmd),
    /// Écriture sociale ou accusé de lecture (toujours après un geste).
    Relations(super::relations::RelationsCmd),
    /// Message avec pièces jointes, ou annulation de son envoi (`state::upload`).
    Upload(super::upload::UploadCmd),
    /// Fils : rejoindre, quitter, archiver, créer, archives d'un forum.
    Threads(super::threads::ThreadCmd),
    /// Rejoindre, quitter, créer un serveur, invitations (`state::servers`).
    Servers(super::servers::ServerCmd),
    /// Recherche de messages, historique autour d'un message (`state::search`).
    Search(super::search::SearchCmd),
    /// Mentions récentes : une page, ou le retrait d'une mention (`state::inbox`).
    Inbox(super::inbox::InboxCmd),
    /// Index des commandes, interactions avec une application (`state::interactions`).
    Interactions(super::interactions::InteractionCmd),
    /// Sondages, invitations, annonces (lot T13).
    Polls(super::polls::PollCmd),
    Invites(super::invites::InviteCmd),
    Announce(super::announce::AnnounceCmd),
    /// Paramètres du serveur, webhooks, intégrations (lot A8 + A9).
    GuildAdmin(super::admin::guild::AdminCmd),
    /// Salons et catégories (lot A2).
    ChannelAdmin(super::admin::channels::ChanCmd),
    /// Rôles et membres d'un serveur (lot A3, `state::admin::roles`).
    Roles(super::admin::roles::RolesCmd),
    /// Modération, journal d'audit, AutoMod, sécurité (lots A4 et A5).
    Moderation(super::admin::moderation::ModCmd),
    /// Événements programmés, accueil des nouveaux membres (lot A7).
    GuildEvents(super::admin::events::EventCmd),
    Community(super::admin::community::CommunityCmd),
    /// Emojis, autocollants et sons du serveur (lot A6).
    Expressions(super::admin::expressions::ExprCmd),
    /// Vocal : op 4, volumes, appui-pour-parler, modération (phase 4, `state::voice`).
    Voice(super::voice::VoiceCmd),
    /// Sonnerie des appels, tableau de sons (lot V6).
    Calls(super::calls::CallCmd),
    /// Conférences et statut des salons vocaux (lot V6).
    Stage(super::stage::StageCmd),
    /// Caméra (op 4), directs (op 18, 19, 20), tuiles (phase 5, `state::video`).
    Video(super::video::VideoCmd),
    /// Liste des jeux, sondage, ma présence (op 3), boutons (lot C3).
    Activity(super::activity::ActivityCmd),
    /// GIF (tendances, recherche, choix), favoris du compte, super réaction (lot G, `state::gifs`).
    Gifs(super::gifs::GifCmd),
    /// Configuration d'un effet de profil, effets possédés (lot ND, `state::effects`).
    Effects(super::effects::EffectCmd),
    /// Quêtes : liste, accepter, réclamer, code (lot ND, `state::quests`).
    Quests(super::quests::QuestCmd),
    /// Choix du tag de serveur (lot ND, `state::tags`).
    Tags(super::tags::TagCmd),
    /// Widgets de jeux du profil : noms des jeux, suggestions, écriture (`state::widgets`).
    Widgets(super::widgets::WidgetCmd),
    /// Boutique en consultation (lot ND, `state::shop`).
    Shop(super::shop::ShopCmd),
    /// Inventaire du compte, en lecture (lot ND, `state::inventory`).
    Inventory(super::inventory::InventoryCmd),
    /// Suggestions d'amis (lot ND, `state::suggestions`).
    Suggestions(super::suggestions::SuggestionCmd),
    /// Messages enregistrés et rappels (lot ND, `state::saved`).
    Saved(super::saved::SavedCmd),
    /// Centre de sécurité, en lecture (lot ND, `state::safety`).
    SafetyHub(super::safety::SafetyCmd),
    /// Répertoire d'applications, en lecture (lot ND, `state::appdir`).
    AppDir(super::appdir::AppDirCmd),
    /// Vérification des membres et demandes d'adhésion (lot ND, `state::verification`).
    Verification(super::verification::VerifyCmd),
    /// Résumés de salon, en lecture (lot ND, `state::summaries`).
    Summaries(super::summaries::SummaryCmd),
    /// Découverte de serveurs, en lecture (lot ND, `state::discovery`).
    Discovery(super::discovery::DiscoveryCmd),
}

/// Champs d'un MP réellement fournis par le paquet reçu : les autres gardent
/// leur valeur connue (un paquet partiel n'efface ni le nom ni l'avatar).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DmKnown {
    pub name: bool,
    pub avatar: bool,
}

impl DmKnown {
    pub const ALL: DmKnown = DmKnown { name: true, avatar: true };
}
