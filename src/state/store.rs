//! État non visuel de l'application : serveurs, MP, cache de messages,
//! non-lus, mentions, permissions, sélection. Les commandes à envoyer au
//! réseau sont accumulées dans une file que l'interface vide (`take_commands`).

use super::event::{Cmd, Event, HISTORY_PAGE};
use crate::i18n::Lang;
use super::model::{ChannelKind, Dm, Guild, Message, User};
use super::notice::{Notice, Problem, Status};
use super::perms::{base_permissions, with_base, Permissions, Verdict};

/// Réactions, réponses, transferts, épingles, modification, suppression (lots T5/T6).
pub mod social;
/// Envoi de fichiers (lot T8).
mod upload;
/// MP du compte système de Discord : lecture seule (lot « messages »).
pub mod official;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

// Relations et accusés de lecture : module enfant pour tenir les non-lus
// par les fonctions privées du magasin.
#[path = "store_relations.rs"]
mod relations_glue;
// Fils et forums (lot T10) : module enfant, pour les permissions et les non-lus.
#[path = "store_threads.rs"]
mod threads_glue;
// Recherche et saut vers un message (lot T12) : module enfant, pour la
// fenêtre de l'historique et le rendu des messages.
#[path = "store_search.rs"]
mod search_glue;
pub use search_glue::Jump;
// Commandes et composants des applications (lot T11).
#[path = "store_interactions.rs"]
mod interactions_glue;
pub use interactions_glue::CommandRef;
// Sondages, messages système, annonces, invitations (lot T13).
#[path = "store_polls.rs"]
mod polls_glue;
// Activité de jeu et ma présence (lot C3).
#[path = "store_activity.rs"]
mod activity_glue;
// Historique hors ligne affiché avant READY (lot H).
/// Lot FF : mentions de salon et de rôle, limite d'âge.
#[path = "store_ff.rs"]
mod ff_glue;
pub use ff_glue::Sensitive;
#[path = "store_offline.rs"]
mod offline_glue;
// Journal des messages supprimés et modifiés (plugin, lot E1).
#[path = "store_journal.rs"]
mod journal_glue;
// Traduction affichée sous un message (plugin, lot E3).
#[path = "store_translate.rs"]
mod translate_glue;

/// Messages gardés en mémoire par salon.
pub const MAX_CACHED_MESSAGES: usize = 500;
/// Nombre de salons dont l'historique reste en mémoire.
pub const MAX_CACHED_CHANNELS: usize = 20;

/// Écart d'horloge avec Discord jugé absurde au-delà (ms) : un jour.
const CLOCK_OFFSET_MAX: u64 = 86_400_000;
/// Variation d'écart ignorée (ms) : l'en-tête `Date` est à la seconde près.
const CLOCK_OFFSET_STEP: u64 = 2_000;
/// Un refus d'envoi (403) moins de 5 min après la fin d'une exclusion est
/// encore attribué à l'exclusion.
const RECENT_TIMEOUT: i64 = 5 * 60_000;

/// Messages en direct comptés au plus pendant que la fenêtre est remontée
/// (au-delà, le compteur du bouton ne grandit plus).
const MAX_MISSED: usize = 10_000;

/// Version du cache d'un salon. Tirée d'un compteur commun à tout le
/// processus, strictement croissant et jamais réutilisé : un salon vidé puis
/// recréé (READY, éviction, reconnexion) ne retrouve jamais une version déjà
/// vue par l'interface, même si plusieurs événements passent sans image.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Rev(u64);

impl Rev {
    fn next() -> Rev {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Rev(NEXT.fetch_add(1, Ordering::Relaxed))
    }

    fn bump(&mut self) {
        *self = Rev::next();
    }
}

impl Default for Rev {
    fn default() -> Self {
        Rev::next()
    }
}

#[derive(Default)]
pub struct ChanState {
    pub msgs: VecDeque<Message>,
    pub loaded: bool,
    /// Dernière page (sans `before`) demandée : premier chargement ou retour au présent.
    pub loading: bool,
    pub more: bool,
    /// Échec du premier chargement ou d'une page plus ancienne (affiché en haut).
    pub error: Option<Problem>,
    /// Échec d'une page plus récente ou du retour au présent (affiché en bas).
    pub error_newer: Option<Problem>,
    pub last_view: Option<Instant>,
    /// Texte d'origine des messages (de la fenêtre et de `live`) dont le
    /// rendu dépend de la langue ou de l'horloge (heure `<t:…>`, mention
    /// rendue par un mot traduit, message système) : seuls ceux-là sont
    /// refaits, une fois, quand l'une ou l'autre change.
    pub stamped: HashMap<u64, String>,
    /// Historique non demandé faute de READ_MESSAGE_HISTORY : seuls les
    /// messages arrivés depuis l'ouverture sont affichés.
    pub history_denied: bool,
    /// Change à chaque modification de `msgs` (ajout, retrait, texte refait) :
    /// l'interface ne revoit ses hauteurs de messages que lorsqu'il change.
    pub rev: Rev,
    /// Les messages les plus récents ne sont plus en mémoire (fenêtre remontée
    /// loin dans l'historique) : ils sont rechargés en redescendant.
    pub newer: bool,
    /// Page plus ancienne demandée : `before` envoyé (le premier message d'alors).
    asked_older: Option<u64>,
    /// Page plus récente demandée : `after` envoyé (le dernier message d'alors).
    asked_newer: Option<u64>,
    /// Messages arrivés en direct pendant que `newer` est vrai, sans les
    /// miens ni les doublons ; un message supprimé en sort.
    missed: HashSet<u64>,
    /// Les derniers de ces messages, rendus au retour au présent (au plus une page).
    live: VecDeque<Message>,
    /// Premier message arrivé pendant l'absence, à l'ouverture du salon :
    /// la ligne « Nouveaux messages » le précède. Oublié quand on change de salon.
    pub new_marker: Option<u64>,
    /// (premier, dernier) message affiché, donné par l'interface : jamais évincés.
    viewing: Option<(u64, u64)>,
    /// Page autour d'un message demandée (saut, lot T12) : `around` envoyé.
    asked_around: Option<u64>,
    /// Une dernière page partie avant le saut est périmée : ignorée à son arrivée.
    skip_present: bool,
    /// Messages venus de l'historique hors ligne (ou d'avant READY) : affichés
    /// mais à recharger ; la dernière page les remplace (lot H).
    pub offline: bool,
}

/// Côté de la liste où une page d'historique manque.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    /// En haut : premier chargement ou page plus ancienne.
    Top,
    /// En bas : page plus récente ou retour au présent.
    Bottom,
}

impl ChanState {
    /// Termine la demande `before` / `after` si c'est bien celle en cours
    /// (sinon : réponse périmée, rien ne change) ; renvoie son côté.
    fn finish(&mut self, before: Option<u64>, after: Option<u64>) -> Option<Side> {
        match (before, after) {
            (_, Some(a)) => (self.asked_newer == Some(a)).then(|| {
                self.asked_newer = None;
                Side::Bottom
            }),
            (Some(b), None) => (self.asked_older == Some(b)).then(|| {
                self.asked_older = None;
                Side::Top
            }),
            (None, None) => self.loading.then(|| {
                self.loading = false;
                if self.loaded { Side::Bottom } else { Side::Top }
            }),
        }
    }

    pub fn contains(&self, id: u64) -> bool {
        self.msgs.binary_search_by_key(&id, |m| m.id).is_ok()
    }

    /// Page plus ancienne demandée, pas encore reçue.
    pub fn loading_older(&self) -> bool {
        self.asked_older.is_some()
    }

    /// Page plus récente demandée, pas encore reçue.
    pub fn loading_newer(&self) -> bool {
        self.asked_newer.is_some()
    }

    /// Messages arrivés en direct hors de la fenêtre (compteur du bouton).
    pub fn missed(&self) -> usize {
        self.missed.len()
    }

    /// (premier, dernier) message affiché à la dernière image.
    pub fn viewing(&self) -> Option<(u64, u64)> {
        self.viewing
    }

    /// Insère en gardant l'ordre croissant des identifiants (l'arrivée peut
    /// être légèrement désordonnée).
    fn insert_sorted(&mut self, m: Message) {
        let at = self.msgs.partition_point(|x| x.id < m.id);
        if self.msgs.get(at).is_some_and(|x| x.id == m.id) {
            return;
        }
        self.msgs.insert(at, m);
        self.rev.bump();
    }

    /// Ajoute à la fin les messages plus récents que le dernier en mémoire.
    fn append_newer(&mut self, page: impl IntoIterator<Item = Message>) {
        let mut page: Vec<Message> = page.into_iter().collect();
        page.sort_by_key(|m| m.id);
        let mut back = self.msgs.back().map_or(0, |m| m.id);
        for m in page {
            if m.id > back {
                back = m.id;
                self.msgs.push_back(m);
                self.rev.bump();
            }
        }
    }

    /// Retour au présent : les messages gardés pendant l'absence rejoignent la fenêtre.
    fn back_to_present(&mut self) {
        self.newer = false;
        self.missed.clear();
        let live = std::mem::take(&mut self.live);
        self.append_newer(live);
    }

    /// Ramène le cache à `MAX_CACHED_MESSAGES` sans toucher aux messages
    /// affichés ni à `keep` (la page qui vient d'arriver : la jeter la ferait
    /// redemander sans fin). `old_first` : les plus anciens partent d'abord
    /// (on lit le présent) ; sinon les plus récents (on remonte
    /// l'historique), et il faudra les recharger en redescendant. Sans
    /// historique lisible, rien ne pourrait être rechargé : seuls les plus
    /// anciens partent, affichés ou non.
    fn trim(&mut self, old_first: bool, keep: Option<(u64, u64)>) {
        // Rien d'affiché : tout peut partir.
        let (lo, hi) = self.viewing.unwrap_or((u64::MAX, 0));
        let kept = |id: u64| keep.is_some_and(|(a, b)| (a..=b).contains(&id));
        while self.msgs.len() > MAX_CACHED_MESSAGES {
            let front = self.msgs.front().map_or(0, |m| m.id);
            let back = self.msgs.back().map_or(0, |m| m.id);
            let front_free = !kept(front) && (front < lo || self.history_denied);
            let back_free = !kept(back) && back > hi && !self.history_denied;
            let take_front = if old_first { front_free } else { front_free && !back_free };
            let gone = if take_front {
                self.more = !self.history_denied;
                self.msgs.pop_front()
            } else if back_free {
                self.newer = true;
                self.msgs.pop_back()
            } else {
                // Tout ce qui reste est à l'écran ou vient d'arriver : on dépasse un peu.
                break;
            };
            if let Some(m) = gone {
                self.stamped.remove(&m.id);
                self.rev.bump();
            }
        }
    }

    /// Message arrivé en direct pendant que la fenêtre a quitté le présent :
    /// gardé pour le retour au présent, compté s'il n'est pas de moi.
    /// `raw` : son texte d'origine s'il dépend de la langue ou de l'horloge
    /// (gardé dans `stamped` pour le refaire, ici comme après le retour).
    fn miss(&mut self, m: Message, mine: bool, raw: Option<String>) {
        if !mine && self.missed.len() < MAX_MISSED {
            self.missed.insert(m.id);
        }
        if !self.live.iter().any(|x| x.id == m.id) {
            if let Some(raw) = raw {
                self.stamped.insert(m.id, raw);
            }
            self.live.push_back(m);
            if self.live.len() > HISTORY_PAGE
                && let Some(gone) = self.live.pop_front()
            {
                self.stamped.remove(&gone.id);
            }
        }
    }
}

/// Conséquence d'un événement que seule l'interface sait traiter.
#[derive(Debug, PartialEq, Eq)]
pub enum Effect {
    Toast(Notice),
    /// La connexion est perdue pour de bon : retour à l'écran de connexion.
    Fatal { problem: Problem, bad_token: bool },
    /// L'historique du salon affiché vient d'arriver.
    ScrollToBottom,
}

/// Pourquoi l'envoi est impossible dans un salon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendBlock {
    /// Exclusion temporaire jusqu'à cet instant (ms Unix).
    Timeout { until_ms: i64 },
    NoPermission,
    /// MP du compte système de Discord : réservé aux notifications officielles.
    Official,
}

/// Permissions calculées pour un serveur. Un salon absent de `perms` a des
/// permissions inconnues (et se comporte comme avant ce calcul).
#[derive(Default)]
struct GuildAccess {
    /// Tous les salons du serveur au moment du calcul (pour défaire `chan_guild`).
    chans: Vec<u64>,
    perms: HashMap<u64, Permissions>,
    /// Salons et catégories masqués.
    hidden: HashSet<u64>,
    /// Exclusion temporaire en cours au moment du calcul.
    timeout_until: Option<i64>,
}

#[derive(Default)]
pub struct Store {
    pub me: Option<User>,
    pub guilds: Vec<Guild>,
    pub dms: Vec<Dm>,
    /// `None` = messages privés
    pub guild: Option<u64>,
    pub channel: Option<u64>,
    pub chans: HashMap<u64, ChanState>,
    /// Salons non lus et mentionnés (MP compris). Privés : toute
    /// modification passe par `mark_unread` / `clear_unread`, qui tiennent
    /// `unread_index` à jour.
    unread: HashSet<u64>,
    mentioned: HashSet<u64>,
    /// Mentions non lues par salon mentionné (pastille chiffrée des lignes de
    /// salon) : compte de Discord au READY, puis une par message reçu.
    mention_counts: HashMap<u64, u32>,
    pub status: Status,
    /// Heures des balises `<t:…>` en 12 h plutôt qu'en 24 h.
    clock12: bool,
    /// Langue des textes rendus à la réception (donnée par l'interface : le
    /// magasin ne lit pas la langue courante).
    lang: Lang,
    outbox: Vec<Cmd>,
    /// Permissions par serveur, recalculées seulement par les événements
    /// qui les changent (jamais à l'affichage).
    access: HashMap<u64, GuildAccess>,
    /// Serveur de chaque salon de serveur.
    chan_guild: HashMap<u64, u64>,
    /// Fin de la plus proche exclusion temporaire en cours (tiré de `access`).
    timeout_end: Option<i64>,
    /// Par serveur : (salons non lus visibles, salons mentionnés visibles).
    /// Lu tel quel par le rail ; changé seulement quand un non-lu ou les
    /// permissions changent.
    unread_index: HashMap<u64, (u32, u32)>,
    /// Par salon non lu : premier message arrivé depuis la dernière visite
    /// (position de la ligne « Nouveaux messages » à la prochaine ouverture).
    unread_from: HashMap<u64, u64>,
    /// Écart (ms) entre l'horloge de Discord (en-tête `Date`) et la nôtre.
    pub(super) clock_offset: i64,
    /// Dernière heure de Discord reçue, telle quelle (même absurde), avec
    /// l'instant monotone de sa réception : la seule horloge que la machine
    /// ne peut pas fausser (étape 2 de la migration du jeton, B1).
    discord_clock: Option<(i64, std::time::Instant)>,
    /// Messages non envoyés ou non confirmés, par salon : leur texte n'est
    /// jamais perdu (affichés au-dessus de la saisie, « Réessayer » / « Supprimer »).
    pub unsent: HashMap<u64, Vec<Unsent>>,
    /// Envois partis, pas encore confirmés (salon, nonce, texte) : montrés
    /// estompés sous le fil jusqu'à la réponse. Borné : `MAX_SENDING`.
    pub sending: Vec<(u64, u64, String)>,
    /// Dernier nonce donné : chaque envoi a le sien.
    last_nonce: u64,
    /// Réactions, épingles, emojis, réponses et transferts en attente (lots T5/T6).
    social: social::Social,
    /// Qui écrit en ce moment (TYPING_START).
    pub typing: super::typing::Typing,
    /// Instant de mon dernier envoi par salon (mode lent), voir `compose.rs`.
    pub(super) last_sent: HashMap<u64, i64>,
    /// Utilisateurs, membres et présences (partagé avec le réseau, voir `people`).
    pub people: super::people::PeopleHub,
    /// Liste des membres (panneau de droite) : `state::members`.
    pub members: super::members::Members,
    /// Réglages du compte (synchronisés, notifications par serveur, profil…).
    pub account: super::usersettings::AccountState,
    /// Amis, demandes, bloqués, membres des MP, présences des amis.
    pub relations: super::relations::Relations,
    /// États de lecture de Discord et accusés à envoyer.
    pub reads: super::reads::Reads,
    /// Envois de fichiers en cours ou en échec (lot T8).
    pub uploads: super::upload::Uploads,
    /// Fils de discussion et forums (`state::threads`).
    pub threads: super::threads::Threads,
    thread_failure: Option<(super::threads::ThreadAction, Problem)>,
    /// Autocollants des serveurs (sélecteur, `state::emoji`).
    pub stickers: super::emoji::Stickers,
    /// Rejoindre, quitter, créer un serveur, invitations (`state::servers`).
    pub servers: super::servers::Servers,
    /// Recherche de messages affichée (lot T12).
    pub search: super::search::Search,
    /// Boîte de réception : mentions récentes (`state::inbox`).
    pub inbox: super::inbox::Inbox,
    /// Commandes et composants des applications (`state::interactions`).
    pub interactions: super::interactions::Interactions,
    /// Sondages, invitations, annonces (lot T13, `store_polls.rs`).
    pub polls: super::polls::Polls,
    pub invites: super::invites::Invites,
    pub announce: super::announce::Announce,
    /// Fenêtre « Paramètres du serveur » : données du serveur ouvert (lot A8 + A9).
    pub guild_admin: super::admin::guild::GuildAdmin,
    /// Salons et catégories (lot A2, `state::admin::channels`).
    pub chan_admin: super::admin::channels::ChannelAdmin,
    /// Rôles et membres d'un serveur (lot A3, `state::admin::roles`).
    pub roles_admin: super::admin::roles::RolesAdmin,
    /// Modération, journal d'audit, AutoMod, sécurité (lots A4 et A5).
    pub moderation: super::admin::moderation::Moderation,
    /// Événements programmés, accueil des nouveaux membres (lot A7).
    pub events: super::admin::events::Events,
    pub community: super::admin::community::Community,
    /// Emojis, autocollants et sons du serveur (lot A6, `state::admin::expressions`).
    pub expressions: super::admin::expressions::Expressions,
    /// Vocal des serveurs (phase 4, `state::voice`).
    pub voice: super::voice::VoiceStates,
    /// Appels privés, états vocaux, tableau de sons (lot V6, `state::calls`).
    pub calls: super::calls::Calls,
    /// Salons de conférence et statut des salons vocaux (lot V6, `state::stage`).
    pub stage: super::stage::Stage,
    /// Caméras, partage, tuiles, images (phase 5, `state::video`).
    pub video: super::video::VideoStates,
    /// Directs en cours (phase 5, `state::stream`).
    pub stream: super::stream::Streams,
    /// Jeux détectés et ma présence (lot C3, `state::activity`).
    pub activity: super::activity::Activities,
    /// Sélecteur de GIF, favoris et fréquence du compte (lot G, `state::gifs`).
    pub gifs: super::gifs::Gifs,
    /// Effets de profil connus et possédés (lot ND, `state::effects`).
    pub effects: super::effects::Effects,
    /// Choix du tag de serveur (lot ND).
    pub tags: super::tags::Tags,
    /// Widgets de jeux du profil (éditeur de profil).
    pub widgets: super::widgets::Widgets,
    /// Boutique en consultation (lot ND).
    pub shop: super::shop::Shop,
    /// Inventaire du compte (lot ND).
    pub inventory: super::inventory::Inventory,
    pub suggestions: super::suggestions::Suggestions,
    pub saved: super::saved::SavedMessages,
    pub safety_hub: super::usersettings::Remote<super::safety::SafetyHub>,
    pub appdir: super::appdir::AppDir,
    pub verification: super::verification::Verification,
    pub summaries: super::summaries::Summaries,
    pub discovery: super::discovery::Discovery,
    /// Quêtes et solde d'Orbes (lot ND, `state::quests`).
    pub quests: super::quests::Quests,
    /// Historique hors ligne affiché avant READY (lot H, `state::offline`).
    offline: super::offline::Shown,
}

/// Envois en attente de confirmation montrés à la fois.
const MAX_SENDING: usize = 16;
/// Messages non envoyés gardés par salon, et salons concernés, au plus :
/// au-delà, les plus anciens sont oubliés.
pub const MAX_UNSENT_PER_CHANNEL: usize = 20;
pub const MAX_UNSENT_CHANNELS: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unsent {
    pub nonce: u64,
    pub content: String,
    pub state: UnsentState,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnsentState {
    /// Pas parti : la raison est affichée.
    Failed(Problem),
    /// Peut-être parti (délai dépassé) : confirmé par MESSAGE_CREATE, ou à
    /// réessayer (même nonce, pas de doublon) ou à abandonner.
    Unconfirmed,
    /// Nouvel essai en cours.
    Retrying,
}

/// Instant présent (ms Unix), pour les exclusions temporaires.
pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

impl Store {
    pub fn new() -> Self {
        Self::default()
    }

    /// Relevé de mémoire (banc de l'aperçu, lot « ram ») : rend le magasin
    /// partie par partie ; `after` reçoit le nom de chacune une fois libérée,
    /// pour que l'appelant lise ce que le tas a rendu entre deux appels.
    pub fn release_by_part(mut self, mut after: impl FnMut(&'static str)) {
        macro_rules! part {
            ($($field:ident),* $(,)?) => {
                $(
                    drop(std::mem::take(&mut self.$field));
                    after(stringify!($field));
                )*
            };
        }
        part!(
            guilds, dms, chans, unread, mentioned, mention_counts, outbox, access, chan_guild, unread_index, unread_from, unsent, sending, social, typing, last_sent, people, members, account, relations, reads, uploads, threads,
            stickers, servers, search, inbox, interactions, polls, invites, announce, guild_admin, chan_admin, roles_admin, moderation, events, community, expressions, voice, calls, stage, video, stream, activity, gifs, effects,
            tags, widgets, shop, inventory, suggestions, saved, safety_hub, appdir, verification, summaries, discovery, quests,
        );
    }

    /// Oublie tout ce qui vient du compte (déconnexion). Le statut est gardé.
    pub fn clear(&mut self) {
        self.me = None;
        self.guilds.clear();
        self.dms.clear();
        self.chans.clear();
        self.unread.clear();
        self.mentioned.clear();
        self.mention_counts.clear();
        self.guild = None;
        self.channel = None;
        self.outbox.clear();
        self.access.clear();
        self.chan_guild.clear();
        self.timeout_end = None;
        self.unread_index.clear();
        self.unread_from.clear();
        self.unsent.clear();
        self.sending.clear();
        self.social.reset();
        self.typing.clear();
        self.last_sent.clear();
        // Les tables de la connexion fermée sont oubliées ; la suivante apporte les siennes.
        self.people = super::people::PeopleHub::default();
        self.members.clear();
        self.account = Default::default();
        self.relations.clear();
        self.reads.clear();
        self.uploads.clear();
        self.threads.clear();
        self.thread_failure = None;
        self.stickers.clear();
        self.servers.clear();
        self.search.clear();
        self.inbox.clear();
        self.interactions.clear();
        self.polls.clear();
        self.invites.clear();
        self.announce.clear();
        self.guild_admin = Default::default();
        self.chan_admin.clear();
        self.roles_admin.clear();
        self.moderation.clear();
        self.events.clear();
        self.community.clear();
        self.expressions.clear();
        self.voice.clear();
        self.calls.clear();
        self.stage.clear();
        self.video.clear();
        self.stream.clear();
        self.activity.session_reset();
        self.gifs.clear();
        self.effects.clear();
        self.tags.clear();
        self.widgets.clear();
        self.shop.clear();
        self.inventory.clear();
        self.suggestions.clear();
        self.saved.clear();
        self.safety_hub = super::usersettings::Remote::Idle;
        self.appdir.clear();
        self.verification.clear();
        self.summaries.clear();
        self.discovery.clear();
        self.quests.clear();
        self.offline = Default::default();
    }

    pub fn take_commands(&mut self) -> Vec<Cmd> {
        let mut cmds = std::mem::take(&mut self.outbox);
        cmds.extend(self.account.take_commands().into_iter().map(Cmd::Account));
        cmds.extend(self.activity_commands().into_iter().map(Cmd::Activity));
        // MP du compte système : aucune écriture ne sort, quel que soit le chemin pris.
        self.drop_official_writes(&mut cmds);
        cmds
    }

    pub fn push_command(&mut self, cmd: Cmd) {
        self.outbox.push(cmd);
    }

    pub fn apply(&mut self, ev: Event) -> Option<Effect> {
        match ev {
            Event::Status(s) => self.status = s,
            Event::PeopleHub(hub) => {
                self.people = hub;
                self.keep_friend_presences();
            }
            Event::DmOpened { channel_id } => self.dm_opened(channel_id),
            Event::MeUpdate(u) => {
                if self.me.as_ref().is_some_and(|me| me.id == u.id) {
                    self.me = Some(u);
                }
            }
            Event::Ready { me, guilds, dms } => {
                self.activity.on_ready();
                self.me = Some(me);
                self.guilds = guilds;
                self.dms = dms;
                // Session neuve : plus rien n'est souscrit.
                self.members.clear();
                // Session neuve : l'historique en cache peut être périmé. Vidé
                // avant le calcul des permissions, qui sinon redemanderait
                // l'historique des salons devenus lisibles. Historique hors
                // ligne affiché : gardé à l'écran, à recharger (lot H).
                self.offline_ready();
                self.social.reset();
                // Les fils de la session arrivent juste après (`ThreadEvent::Sync`).
                self.threads.clear();
                self.stickers.clear();
                // Appels et états vocaux : ceux de la session neuve arrivent ensuite (lot V6).
                self.calls.session_reset();
                let lost = self.refresh_permissions();
                match self.channel.filter(|c| self.channel_exists(*c) && !self.is_hidden(*c)) {
                    Some(c) => self.ensure_history(c),
                    None => {
                        self.channel = None;
                        if self.guild.is_some_and(|g| !self.guilds.iter().any(|x| x.id == g)) {
                            self.guild = None;
                        }
                    }
                }
                return lost;
            }
            Event::GuildCreate(g) => {
                let id = g.id;
                match self.guilds.iter_mut().find(|x| x.id == id) {
                    Some(slot) => *slot = g,
                    None => self.guilds.push(g),
                }
                let eff = self.guild_changed(id);
                self.guild_arrived(id);
                return eff;
            }
            Event::GuildUpdate { id, owner_id, icon } => {
                if let (Some(g), Some(icon)) = (self.guild_mut(id), icon) {
                    g.icon = icon;
                }
                if let (Some(g), Some(owner)) = (self.guild_mut(id), owner_id) {
                    g.owner_id = owner;
                    return self.guild_changed(id);
                }
            }
            Event::GuildRolesUnreadable { guild_id } => {
                if let Some(g) = self.guild_mut(guild_id) {
                    g.roles_unreadable = true;
                    return self.guild_changed(guild_id);
                }
            }
            Event::ServerTime(server_ms) => {
                self.discord_clock = Some((server_ms, std::time::Instant::now()));
                return self.set_server_time(server_ms, now_ms());
            }
            Event::SendError { channel_id, nonce, content, error, forbidden } => {
                let reason = match self.recent_timeout(channel_id) {
                    Some(until_ms) if forbidden => Problem::Timeout { until_ms },
                    _ => error,
                };
                self.sending.retain(|s| s.1 != nonce);
                self.keep_unsent(channel_id, nonce, content, UnsentState::Failed(reason.clone()));
                self.forget_sent(channel_id);
                return Some(Effect::Toast(Notice::NotSent(reason)));
            }
            Event::SendUnconfirmed { channel_id, nonce, content } => {
                self.sending.retain(|s| s.1 != nonce);
                self.keep_unsent(channel_id, nonce, content, UnsentState::Unconfirmed);
                return Some(Effect::Toast(Notice::Unconfirmed));
            }
            Event::SendConfirmed { channel_id, nonce } => {
                self.sending.retain(|s| s.1 != nonce);
                self.uploads.confirmed(nonce);
                if let Some(list) = self.unsent.get_mut(&channel_id) {
                    list.retain(|u| u.nonce != nonce);
                    if list.is_empty() {
                        self.unsent.remove(&channel_id);
                    }
                }
            }
            Event::GuildDelete(id) => {
                self.guilds.retain(|g| g.id != id);
                self.threads.remove_guild(id);
                self.stickers.forget_guild(id);
                self.events.forget_guild(id);
                self.community.forget_guild(id);
                self.expressions.forget_guild(id);
                if self.guild == Some(id) {
                    self.guild = None;
                    self.channel = None;
                }
                return self.guild_changed(id);
            }
            Event::RoleUpsert { guild_id, role } => {
                if let Some(g) = self.guild_mut(guild_id) {
                    match g.roles.iter_mut().find(|r| r.id == role.id) {
                        Some(slot) => *slot = role,
                        None => g.roles.push(role),
                    }
                    return self.guild_changed(guild_id);
                }
            }
            Event::RoleDelete { guild_id, role_id } => {
                if let Some(g) = self.guild_mut(guild_id) {
                    g.roles.retain(|r| r.id != role_id);
                    // Discord retire aussi le rôle des membres : sans cela il
                    // manquerait et rendrait les permissions inconnues.
                    if let Some(m) = g.me.as_mut() {
                        m.roles.retain(|r| *r != role_id);
                    }
                    return self.guild_changed(guild_id);
                }
            }
            Event::RoleUnreadable { guild_id, role_id } => {
                if let Some(g) = self.guild_mut(guild_id) {
                    g.roles.retain(|r| r.id != role_id);
                    return self.guild_changed(guild_id);
                }
            }
            Event::MemberUpdate { guild_id, user_id, member } => {
                if self.me.as_ref().is_some_and(|u| u.id == user_id)
                    && let Some(g) = self.guild_mut(guild_id)
                {
                    g.me = member;
                    return self.guild_changed(guild_id);
                }
            }
            Event::DmUpsert { dm, known } => match self.dms.iter_mut().find(|d| d.id == dm.id) {
                // Seul ce que le paquet apporte remplace ce qui est connu.
                Some(slot) => {
                    if known.name {
                        slot.name = dm.name;
                    }
                    if !dm.recipients.is_empty() {
                        slot.recipients = dm.recipients;
                    }
                    if known.avatar {
                        slot.avatar = dm.avatar;
                    }
                }
                None => self.dms.insert(0, dm),
            },
            Event::ChannelUpsert { guild_id, channel } => {
                if let Some(g) = self.guild_mut(guild_id) {
                    match g.channels.iter_mut().find(|c| c.id == channel.id) {
                        Some(slot) => *slot = channel,
                        None => g.channels.push(channel),
                    }
                    super::model::sort_channels(&mut g.channels);
                    return self.guild_changed(guild_id);
                }
            }
            Event::ChannelDelete { guild_id, id } => {
                self.clear_unread(id);
                self.relations_forget(id);
                self.threads.remove_parent(id);
                match guild_id {
                    Some(gid) => {
                        if let Some(g) = self.guild_mut(gid) {
                            g.channels.retain(|c| c.id != id);
                            self.guild_changed(gid);
                        }
                    }
                    None => self.dms.retain(|d| d.id != id),
                }
                self.chans.remove(&id);
                if self.channel == Some(id) {
                    self.channel = None;
                }
            }
            Event::MessageCreate(mut m) => {
                let (rendered, localized) = self.render(&m);
                let raw = std::mem::replace(&mut m.content, rendered);
                // Mon message arrive par la passerelle avant la réponse de l'envoi : sa ligne estompée s'en va.
                if self.me.as_ref().is_some_and(|u| u.id == m.author_id)
                    && let Some(i) = self.sending.iter().position(|s| s.0 == m.channel_id && s.2.trim() == raw.trim())
                {
                    self.sending.remove(i);
                }
                self.render_meta(&mut m);
                self.thread_on_message(&mut m);
                // `/tts` reçu (lot FF2, F27) : lu à voix haute seulement dans le salon
                // affiché, si le réglage du compte « Commande /tts » le demande, et
                // jamais pour quelqu'un de bloqué ni pour mes propres messages.
                if m.meta.tts
                    && self.channel == Some(m.channel_id)
                    && self.me.as_ref().is_some_and(|u| u.id != m.author_id)
                    && self.account.bool(super::usersettings::schema::Key::TtsCommand).unwrap_or(false)
                    && !self.relations.relation(m.author_id).is_some_and(|r| r.kind == super::relations::RelKind::Blocked)
                    && self.social.fresh_tts.len() < social::MAX_TTS
                {
                    let text: String = raw.chars().take(social::MAX_TTS_CHARS).collect();
                    self.social.fresh_tts.push(format!("{} : {text}", m.author));
                }
                let raw = localized.then_some(raw);
                let me =self.me.as_ref().map(|u| u.id).unwrap_or(0);
                self.reads_on_message(m.channel_id, m.id, m.author_id == me);
                // Mode discret : mon message vaut lecture (Discord accuse à l'envoi), pastilles comprises.
                if m.author_id == me && self.reads.manual() {
                    self.clear_unread(m.channel_id);
                }
                if m.guild_id.is_none()
                    && let Some(pos) = self.dms.iter().position(|d| d.id == m.channel_id)
                {
                    let mut dm = self.dms.remove(pos);
                    dm.last_message = m.id;
                    self.dms.insert(0, dm);
                }
                if (self.channel != Some(m.channel_id) || self.reads.manual()) && m.author_id != me && self.thread_counts_unread(m.channel_id) {
                    // Salon, catégorie ou serveur muet : ni non-lu ni pastille, sauf
                    // mention directe (comme Discord) ; @everyone peut être ignoré.
                    let parent = self.chan_guild.get(&m.channel_id).and_then(|g| self.guilds.iter().find(|x| x.id == *g)).and_then(|g| g.channels.iter().find(|c| c.id == m.channel_id)).and_then(|c| c.parent);
                    let muted = self.account.notifs.muted(m.guild_id, m.channel_id, parent, now_ms());
                    let everyone = m.mention_everyone && !self.account.notifs.suppresses_everyone(m.guild_id);
                    let pinged = (m.guild_id.is_none() && !muted) || everyone || m.mentions.iter().any(|(id, _)| *id == me) || super::notify::role_ping(self, &m);
                    if pinged || !muted {
                        self.mark_unread(m.channel_id, pinged);
                        self.unread_from.entry(m.channel_id).or_insert(m.id);
                    }
                }
                let mine = m.author_id == me;
                self.typing.stop(m.channel_id, m.author_id);
                if let Some(cs) = self.chans.get_mut(&m.channel_id)
                    && cs.loaded
                    && !cs.contains(m.id)
                {
                    if cs.newer {
                        // Fenêtre remontée dans l'historique : le message n'est pas
                        // contigu à ceux en mémoire. Il est compté, et gardé pour le
                        // retour au présent (les derniers seulement), avec son
                        // texte d'origine.
                        cs.miss(m, mine, raw);
                    } else {
                        if let Some(raw) = &raw {
                            cs.stamped.insert(m.id, raw.clone());
                        }
                        let id = m.id;
                        // Fenêtre pleine dont le haut est affiché : c'est ce message
                        // (le plus récent) qui partira, et il sera compté comme manqué.
                        let spare = (cs.msgs.len() >= MAX_CACHED_MESSAGES).then(|| m.clone());
                        cs.insert_sorted(m);
                        cs.trim(true, None);
                        if let Some(m) = spare
                            && !cs.contains(id)
                        {
                            cs.miss(m, mine, raw);
                        }
                    }
                }
            }
            Event::MessageUpdate { channel_id, id, content } => {
                // Le message peut être dans la fenêtre ou dans la file des messages
                // arrivés pendant qu'elle est remontée.
                let found = self.chans.get(&channel_id).and_then(|cs| cs.msgs.iter().chain(cs.live.iter()).find(|m| m.id == id).cloned());
                if let Some(mut m) = found {
                    m.content = content;
                    let (rendered, localized) = self.render(&m);
                    let raw = localized.then(|| m.content.clone());
                    if let Some(cs) = self.chans.get_mut(&channel_id) {
                        match raw {
                            Some(raw) => cs.stamped.insert(id, raw),
                            None => cs.stamped.remove(&id),
                        };
                        if let Some(slot) = cs.msgs.iter_mut().find(|m| m.id == id) {
                            slot.content = rendered.clone();
                            slot.edited = true;
                            cs.rev.bump();
                        }
                        if let Some(slot) = cs.live.iter_mut().find(|m| m.id == id) {
                            slot.content = rendered;
                            slot.edited = true;
                        }
                    }
                }
            }
            Event::MessageDelete { channel_id, id } => {
                self.social_deleted(id);
                if let Some(cs) = self.chans.get_mut(&channel_id) {
                    let before = cs.msgs.len();
                    cs.msgs.retain(|m| m.id != id);
                    if cs.msgs.len() != before {
                        cs.rev.bump();
                    }
                    cs.live.retain(|m| m.id != id);
                    cs.missed.remove(&id);
                    cs.stamped.remove(&id);
                }
            }
            Event::MessageMedia { channel_id, id, attachments, embeds } => {
                if let Some(cs) = self.chans.get_mut(&channel_id) {
                    let mut changed = false;
                    for (window, slot) in cs.msgs.iter_mut().map(|m| (true, m)).chain(cs.live.iter_mut().map(|m| (false, m))) {
                        if slot.id != id {
                            continue;
                        }
                        if let Some(a) = &attachments {
                            slot.attachments.clone_from(a);
                        }
                        if let Some(e) = &embeds {
                            slot.embeds.clone_from(e);
                        }
                        changed |= window;
                    }
                    if changed {
                        cs.rev.bump();
                    }
                }
            }
            Event::History { channel_id, before, messages } => {
                let (page, raws) = self.render_page(messages);
                match before {
                    // Dernière page (premier chargement ou retour au présent) :
                    // elle remplace la fenêtre, qui touche alors le présent.
                    None => {
                        if self.skip_present(channel_id) {
                            return None;
                        }
                        let cs = self.chans.entry(channel_id).or_default();
                        cs.loading = false;
                        cs.error = None;
                        cs.error_newer = None;
                        // Les textes d'origine des messages gardés pour le
                        // retour au présent restent : ils rejoignent la fenêtre.
                        let live = &cs.live;
                        cs.stamped.retain(|id, _| live.iter().any(|m| m.id == *id));
                        cs.stamped.extend(raws);
                        cs.more = page.len() == HISTORY_PAGE;
                        cs.msgs = page.into();
                        cs.loaded = true;
                        cs.offline = false;
                        cs.rev.bump();
                        cs.back_to_present();
                        if self.channel == Some(channel_id) {
                            return Some(Effect::ScrollToBottom);
                        }
                    }
                    Some(b) => {
                        let cs = self.chans.get_mut(&channel_id)?;
                        // Réponse à une autre demande que la nôtre (session
                        // précédente, salon recréé) : ignorée.
                        if cs.asked_older != Some(b) {
                            return None;
                        }
                        cs.asked_older = None;
                        // La fenêtre a changé depuis la demande (retour au présent,
                        // suppression) : coller la page laisserait un trou invisible.
                        if cs.msgs.front().map(|m| m.id) != Some(b) {
                            return None;
                        }
                        cs.error = None;
                        cs.more = page.len() == HISTORY_PAGE;
                        cs.stamped.extend(raws);
                        let keep = page.first().zip(page.last()).map(|(a, z)| (a.id, z.id));
                        for m in page.into_iter().rev().filter(|m| m.id < b) {
                            cs.msgs.push_front(m);
                        }
                        cs.rev.bump();
                        cs.trim(false, keep);
                    }
                }
            }
            Event::HistoryAfter { channel_id, after, messages } => {
                let (page, raws) = self.render_page(messages);
                let cs = self.chans.get_mut(&channel_id)?;
                if cs.asked_newer != Some(after) {
                    return None;
                }
                cs.asked_newer = None;
                // Fenêtre revenue au présent ou changée depuis la demande : page périmée.
                if !cs.newer || cs.msgs.back().map(|m| m.id) != Some(after) {
                    return None;
                }
                cs.error_newer = None;
                cs.stamped.extend(raws);
                let full = page.len() == HISTORY_PAGE;
                let keep = page.first().zip(page.last()).map(|(a, z)| (a.id, z.id));
                cs.append_newer(page);
                if !full {
                    // Plus rien après : retour au présent, avec ce qui est arrivé entre-temps.
                    cs.back_to_present();
                }
                cs.trim(true, keep);
            }
            Event::HistoryError { channel_id, before, after, error } => {
                // Seule la demande qui a échoué est terminée ; l'erreur s'affiche
                // du côté de la liste où la page manque.
                if before.is_none() && after.is_none() && self.skip_present(channel_id) {
                    return None;
                }
                let cs = self.chans.get_mut(&channel_id)?;
                match cs.finish(before, after) {
                    Some(Side::Top) => cs.error = Some(error),
                    Some(Side::Bottom) => cs.error_newer = Some(error),
                    None => {}
                }
            }
            Event::HistoryCancelled { channel_id, before, after } => {
                // Salon quitté avant le départ de la demande : son chargement
                // (passé ou présent) se termine, sans erreur ; elle sera refaite.
                if before.is_none() && after.is_none() && self.skip_present(channel_id) {
                    return None;
                }
                if let Some(cs) = self.chans.get_mut(&channel_id) {
                    cs.finish(before, after);
                }
            }
            Event::GuildEmojis { guild_id, emojis } => {
                if let Some(g) = self.guild_mut(guild_id) {
                    g.emojis = emojis;
                }
            }
            Event::TypingStart { channel_id, user_id, name } => {
                let name = name.or_else(|| self.typer_name(channel_id, user_id));
                let now = self.now();
                self.typing.start(channel_id, user_id, name, now);
            }
            Event::Relations(e) => return self.apply_relations(e),
            Event::Error(e) => return Some(Effect::Toast(Notice::Error(e))),
            Event::Fatal { problem, bad_token } => return Some(Effect::Fatal { problem, bad_token }),
            Event::Social(e) => return self.apply_social(e),
            Event::MemberList(u) => self.members.apply(u, &self.guilds, self.lang),
            Event::GroupRecipients { channel_id, users } => self.members.set_recipients(channel_id, users),
            Event::MemberListUnavailable => self.members.unavailable = true,
            Event::Account(e) => return self.account.apply(e).map(Effect::Toast),
            Event::Upload(e) => self.apply_upload(e),
            Event::Threads(e) => return self.apply_threads(e),
            Event::Stickers(e) => self.stickers.apply(e),
            Event::Servers(e) => return self.apply_servers(e),
            Event::Roles(e) => return self.apply_roles(e),
            Event::Search(e) => return self.apply_search(e),
            Event::Inbox(e) => self.apply_inbox(e),
            Event::Interactions(e) => return self.apply_interactions(e),
            Event::Polls(e) => self.apply_polls(e),
            Event::Invites(e) => self.apply_invites(e),
            Event::Announce(e) => self.apply_announce(e),
            Event::GuildAdmin(e) => self.apply_guild_admin(e),
            Event::ChannelAdmin(e) => return self.apply_chan_admin(e),
            Event::Moderation(e) => self.apply_moderation(e),
            Event::GuildEvents(e) => self.apply_guild_events(e),
            Event::Community(e) => self.apply_community(e),
            Event::Expressions(e) => return self.apply_expressions(e),
            Event::Voice(e) => {
                self.apply_voice(e);
                self.video_follow_voice();
            }
            Event::Stream(e) => self.apply_stream(e),
            Event::Calls(e) => return self.apply_calls(e),
            Event::Stage(e) => return self.apply_stage(e),
            Event::Activity(e) => self.activity.apply(e),
            Event::Gifs(e) => return self.apply_gifs(e),
            Event::Effects(e) => self.apply_effects(e),
            Event::Tags(e) => self.apply_tags(e),
            Event::Widgets(e) => self.apply_widgets(e),
            Event::Shop(e) => self.apply_shop(e),
            Event::Inventory(e) => self.apply_inventory(e),
            Event::Suggestions(e) => self.apply_suggestions(e),
            Event::Saved(e) => self.apply_saved(e),
            Event::SafetyHub(e) => self.apply_safety_hub(e),
            Event::AppDir(e) => self.apply_appdir(e),
            Event::Verification(e) => self.apply_verification(e),
            Event::Summaries(e) => self.apply_summaries(e),
            Event::Discovery(e) => self.apply_discovery(e),
            Event::Quests(e) => self.apply_quests(e),
        }
        None
    }

    fn guild_mut(&mut self, id: u64) -> Option<&mut Guild> {
        self.guilds.iter_mut().find(|g| g.id == id)
    }

    /// Page d'historique rendue (balises remplacées) et remise en ordre
    /// croissant quel que soit l'ordre reçu, avec le texte d'origine des
    /// messages qui contiennent une heure.
    fn render_page(&self, messages: Vec<Message>) -> (Vec<Message>, Vec<(u64, String)>) {
        let mut raws = Vec::new();
        let mut page: Vec<Message> = messages
            .into_iter()
            .map(|mut m| {
                let (rendered, localized) = self.render(&m);
                let raw = std::mem::replace(&mut m.content, rendered);
                self.render_meta(&mut m);
                self.thread_stub(&mut m);
                if localized {
                    raws.push((m.id, raw));
                }
                m
            })
            .collect();
        page.sort_by_key(|m| m.id);
        page.dedup_by_key(|m| m.id);
        (page, raws)
    }

    pub fn channel_exists(&self, id: u64) -> bool {
        self.dms.iter().any(|d| d.id == id) || self.guilds.iter().any(|g| g.channels.iter().any(|c| c.id == id)) || self.threads.get(id).is_some()
    }

    pub fn channel_name(&self, id: u64) -> Option<&str> {
        self.guilds
            .iter()
            .flat_map(|g| g.channels.iter())
            .find(|c| c.id == id)
            .map(|c| c.name.as_str())
            .or_else(|| self.threads.get(id).map(|t| t.name.as_str()))
    }

    /// Salon d'annonces ? (choisit son symbole)
    pub fn is_announcement(&self, id: u64) -> bool {
        self.guilds.iter().flat_map(|g| g.channels.iter()).any(|c| c.id == id && c.kind == ChannelKind::Announcement)
    }

    pub fn current_guild(&self) -> Option<&Guild> {
        let gid = self.guild?;
        self.guilds.iter().find(|g| g.id == gid)
    }

    // ------------------------------------------------------------------ permissions

    /// Ai-je la permission `p` dans ce salon ? Lecture d'un cache : aucun calcul.
    /// `Inconnu` pour les MP et pour tout salon dont les informations manquent.
    pub fn can(&self, channel: u64, p: Permissions) -> Verdict {
        match self.channel_perms(channel) {
            Some(perms) if perms.contains(p) => Verdict::Permis,
            Some(_) => Verdict::Refuse,
            None => Verdict::Inconnu,
        }
    }

    /// Permissions effectives en cache (`None` = inconnues).
    pub fn channel_perms(&self, channel: u64) -> Option<Permissions> {
        let g = self.chan_guild.get(&channel)?;
        self.access.get(g)?.perms.get(&channel).copied()
    }

    /// Salon ou catégorie masqué (VIEW_CHANNEL refusé de façon certaine).
    pub fn is_hidden(&self, channel: u64) -> bool {
        self.chan_guild.get(&channel).and_then(|g| self.access.get(g)).is_some_and(|a| a.hidden.contains(&channel))
    }

    /// Raison pour laquelle on ne peut pas écrire ici, s'il y en a une sûre.
    pub fn send_block(&self, channel: u64) -> Option<SendBlock> {
        if self.is_official_dm(channel) {
            return Some(SendBlock::Official);
        }
        if self.can(channel, Permissions::SEND_MESSAGES) != Verdict::Refuse {
            return None;
        }
        let timeout = self.chan_guild.get(&channel).and_then(|g| self.access.get(g)).and_then(|a| a.timeout_until);
        Some(match timeout {
            Some(until_ms) => SendBlock::Timeout { until_ms },
            None => SendBlock::NoPermission,
        })
    }

    /// Met un message dans la file d'envoi, sauf si l'envoi est refusé de
    /// façon certaine. Renvoie `true` si le message part.
    pub fn send_message(&mut self, channel_id: u64, content: String) -> bool {
        if self.send_block(channel_id).is_some() {
            return false;
        }
        let nonce = self.next_nonce();
        if self.sending.len() >= MAX_SENDING {
            self.sending.remove(0);
        }
        self.sending.push((channel_id, nonce, content.clone()));
        self.outbox.push(Cmd::Send { channel_id, content, nonce });
        self.note_sent(channel_id);
        // Envoi depuis une fenêtre remontée loin : le présent est rechargé pour
        // que le message envoyé apparaisse sous les yeux.
        self.return_to_present(channel_id);
        true
    }

    /// Nonce d'un nouvel envoi : un snowflake de l'instant présent (forme
    /// des nonces des clients de Discord, 19 chiffres au plus : sous la
    /// limite documentée de 25 caractères), strictement croissant.
    fn next_nonce(&mut self) -> u64 {
        let ms = u64::try_from(self.now()).unwrap_or(0).saturating_sub(super::model::DISCORD_EPOCH);
        self.last_nonce = (ms << 22).max(self.last_nonce + 1);
        self.last_nonce
    }

    /// Garde le texte d'un envoi non abouti (ou met à jour son état).
    fn keep_unsent(&mut self, channel_id: u64, nonce: u64, content: String, state: UnsentState) {
        if !self.unsent.contains_key(&channel_id) && self.unsent.len() >= MAX_UNSENT_CHANNELS {
            // Le salon dont le plus vieux message non envoyé est le plus ancien.
            if let Some(oldest) = self.unsent.iter().min_by_key(|(_, l)| l.first().map(|u| u.nonce)).map(|(c, _)| *c) {
                self.unsent.remove(&oldest);
            }
        }
        let list = self.unsent.entry(channel_id).or_default();
        match list.iter_mut().find(|u| u.nonce == nonce) {
            Some(u) => u.state = state,
            None => {
                if list.len() >= MAX_UNSENT_PER_CHANNEL {
                    list.remove(0);
                }
                list.push(Unsent { nonce, content, state });
            }
        }
    }

    /// Nouvel essai d'un message non envoyé, avec le même nonce (Discord ne
    /// le crée pas une seconde fois s'il était déjà parti). `false` si
    /// l'envoi est refusé de façon certaine.
    pub fn retry_unsent(&mut self, channel_id: u64, nonce: u64) -> bool {
        if self.send_block(channel_id).is_some() {
            return false;
        }
        let Some(u) = self.unsent.get_mut(&channel_id).and_then(|l| l.iter_mut().find(|u| u.nonce == nonce)) else { return false };
        if u.state == UnsentState::Retrying {
            return false;
        }
        u.state = UnsentState::Retrying;
        let content = u.content.clone();
        let cmd = self.resend(channel_id, content, nonce);
        self.outbox.push(cmd);
        true
    }

    /// Abandonne un message non envoyé (ou non confirmé).
    pub fn discard_unsent(&mut self, channel_id: u64, nonce: u64) {
        if let Some(list) = self.unsent.get_mut(&channel_id) {
            list.retain(|u| u.nonce != nonce);
            if list.is_empty() {
                self.unsent.remove(&channel_id);
            }
        }
    }

    /// Fin de la plus proche exclusion temporaire en cours (ms Unix), pour
    /// que l'interface se réveille à ce moment-là.
    pub fn next_timeout_end(&self) -> Option<i64> {
        self.timeout_end
    }

    /// Recalcule les serveurs dont l'exclusion temporaire est terminée à
    /// `now`. Ne fait rien (et ne parcourt rien) s'il n'y en a pas.
    pub fn expire_timeouts(&mut self, now: i64) -> Option<Effect> {
        if self.next_timeout_end().is_none_or(|t| t > now) {
            return None;
        }
        let ended: Vec<u64> = self.access.iter().filter(|(_, a)| a.timeout_until.is_some_and(|t| t <= now)).map(|(g, _)| *g).collect();
        for g in &ended {
            self.recompute_guild(*g);
        }
        self.after_permissions_change()
    }

    /// Recalcule toutes les permissions (READY, ou serveurs modifiés à la
    /// main dans les tests). Renvoie la notification de perte d'accès au
    /// salon ouvert, s'il y a lieu.
    pub fn refresh_permissions(&mut self) -> Option<Effect> {
        self.access.clear();
        self.chan_guild.clear();
        let ids: Vec<u64> = self.guilds.iter().map(|g| g.id).collect();
        for id in ids {
            self.recompute_guild(id);
        }
        self.after_permissions_change()
    }

    /// Instant présent selon Discord (ms Unix) : notre horloge corrigée de
    /// l'écart mesuré sur l'en-tête `Date` des réponses REST.
    /// Heure de Discord maintenant (ms Unix), d'après le dernier en-tête
    /// `Date` reçu, avancée par l'horloge monotone ; `None` avant toute
    /// réponse HTTP. Contrairement à [`Store::now`], aucun écart n'est jugé
    /// absurde : une horloge locale en avance de plusieurs jours n'y change rien.
    pub fn discord_now_ms(&self) -> Option<i64> {
        let (ms, at) = self.discord_clock?;
        Some(ms.saturating_add(i64::try_from(at.elapsed().as_millis()).unwrap_or(i64::MAX)))
    }

    pub fn now(&self) -> i64 {
        now_ms().saturating_add(self.clock_offset)
    }

    /// Heure de Discord reçue (`server_ms`) alors que notre horloge marquait
    /// `local_ms`. L'en-tête n'est précis qu'à la seconde : un écart qui varie
    /// de moins de 2 s est ignoré, et un écart de plus d'un jour est jugé
    /// absurde. Si l'écart change, les exclusions sont réévaluées.
    fn set_server_time(&mut self, server_ms: i64, local_ms: i64) -> Option<Effect> {
        let offset = server_ms.saturating_sub(local_ms);
        if offset.unsigned_abs() > CLOCK_OFFSET_MAX || offset.abs_diff(self.clock_offset) < CLOCK_OFFSET_STEP {
            return None;
        }
        self.clock_offset = offset;
        let timed: Vec<u64> = self.guilds.iter().filter(|g| g.me.as_ref().is_some_and(|m| m.timeout_until.is_some())).map(|g| g.id).collect();
        if timed.is_empty() {
            return None;
        }
        for g in timed {
            self.recompute_guild(g);
        }
        self.after_permissions_change()
    }

    /// Fin d'exclusion de mon membre dans le serveur de ce salon, si elle
    /// n'est pas passée depuis plus de `RECENT_TIMEOUT` (horloges imprécises).
    fn recent_timeout(&self, channel: u64) -> Option<i64> {
        let g = self.chan_guild.get(&channel)?;
        let until = self.guilds.iter().find(|x| x.id == *g)?.me.as_ref()?.timeout_until?;
        (until > self.now() - RECENT_TIMEOUT).then_some(until)
    }

    fn guild_changed(&mut self, id: u64) -> Option<Effect> {
        self.recompute_guild(id);
        // Rôles changés : noms des groupes et couleurs de la liste des membres.
        self.members.relabel(&self.guilds, self.lang);
        self.after_permissions_change()
    }

    /// Recalcule le cache d'un serveur (ou l'efface s'il n'existe plus).
    fn recompute_guild(&mut self, id: u64) {
        if let Some(old) = self.access.remove(&id) {
            for c in old.chans {
                if self.chan_guild.get(&c) == Some(&id) {
                    self.chan_guild.remove(&c);
                }
            }
        }
        let Some(g) = self.guilds.iter().find(|g| g.id == id) else { return };
        let mut a = GuildAccess { chans: g.channels.iter().map(|c| c.id).collect(), ..Default::default() };
        for c in &a.chans {
            self.chan_guild.insert(*c, id);
        }
        if let Some(me) = self.me.as_ref().map(|u| u.id)
            && let Some(base) = base_permissions(g, me)
        {
            let now = self.now();
            for c in &g.channels {
                if let Some(p) = with_base(g, me, base, c, now) {
                    a.perms.insert(c.id, p);
                }
            }
            let hidden = |c: u64| a.perms.get(&c).is_some_and(|p| !p.contains(Permissions::VIEW_CHANNEL));
            for c in &g.channels {
                let hide = if c.kind == ChannelKind::Category {
                    let mut children = g.channels.iter().filter(|x| x.parent == Some(c.id) && x.kind != ChannelKind::Category).peekable();
                    // Catégorie vide : elle suit sa propre permission.
                    if children.peek().is_none() { hidden(c.id) } else { children.all(|x| hidden(x.id)) }
                } else {
                    hidden(c.id)
                };
                if hide {
                    a.hidden.insert(c.id);
                }
            }
            let exempt = base == Permissions::ALL;
            a.timeout_until = g.me.as_ref().filter(|m| !exempt && m.timed_out(now)).and_then(|m| m.timeout_until);
        }
        // Fils : permissions tirées de leur salon parent (lot T10).
        for c in &g.channels {
            for th in self.threads.children(c.id).iter().filter_map(|t| self.threads.get(*t)) {
                a.chans.push(th.id);
                self.chan_guild.insert(th.id, id);
                if let Some(p) = a.perms.get(&c.id).map(|p| super::threads::thread_permissions(*p, th)) {
                    a.perms.insert(th.id, p);
                    if !p.contains(Permissions::VIEW_CHANNEL) {
                        a.hidden.insert(th.id);
                    }
                }
            }
        }
        self.access.insert(id, a);
        // Salons vocaux (phase 4, `state::voice`).
        self.recompute_voice(id);
    }

    /// Suites d'un changement de permissions : quitter le salon ouvert s'il
    /// est devenu invisible, charger l'historique devenu lisible.
    fn after_permissions_change(&mut self) -> Option<Effect> {
        self.timeout_end = self.access.values().filter_map(|a| a.timeout_until).min();
        self.rebuild_unread_index();
        let reopen: Vec<u64> = self
            .chans
            .iter()
            .filter(|(id, cs)| cs.history_denied && self.can(**id, Permissions::READ_MESSAGE_HISTORY) != Verdict::Refuse)
            .map(|(id, _)| *id)
            .collect();
        for id in reopen {
            self.chans.remove(&id);
            if self.channel == Some(id) {
                self.ensure_history(id);
            }
        }
        if let Some(c) = self.channel
            && self.is_hidden(c)
        {
            self.channel = None;
            return Some(Effect::Toast(Notice::LostAccess));
        }
        None
    }

    /// Texte gardé pour un message : sa source telle quelle, sauf pour un
    /// message système sans texte (phrase dans la langue du magasin).
    pub fn render_content(&self, m: &Message) -> String {
        self.render(m).0
    }

    /// Rendu et, en second, s'il dépend de la langue : message système.
    fn render(&self, m: &Message) -> (String, bool) {
        if let Some(kind) = m.system {
            // Phrase du lot T13 (`system_msgs`), faite une fois.
            return (self.system_text(kind, m), true);
        }
        // Le reste garde sa source : balises et markdown sont mis en forme
        // par `markdown::parse_message`, une fois, à la première mise en page.
        (m.content.clone(), false)
    }

    /// Option 12 h (`true`) ou 24 h. Les balises `<t:…>` restent dans la
    /// source et suivent l'option à la mise en forme (`markdown`).
    pub fn set_clock12(&mut self, on: bool) {
        if self.clock12 == on {
            return;
        }
        self.clock12 = on;
        self.rerender();
    }

    /// Langue des textes rendus à la réception (messages système, balises).
    /// Les messages concernés sont refaits une fois, à partir de leur texte d'origine.
    pub fn set_language(&mut self, lang: Lang) {
        if self.lang == lang {
            return;
        }
        self.lang = lang;
        self.rerender();
        self.members.relabel(&self.guilds, lang);
    }

    pub fn language(&self) -> Lang {
        self.lang
    }

    /// Refait les messages dont le rendu dépend de la langue ou de l'horloge,
    /// dans la fenêtre comme parmi ceux gardés pour le retour au présent.
    fn rerender(&mut self) {
        let mut redo = Vec::new();
        for (cid, cs) in &self.chans {
            for (id, raw) in &cs.stamped {
                let m = match cs.msgs.binary_search_by_key(id, |m| m.id) {
                    Ok(i) => cs.msgs.get(i),
                    Err(_) => cs.live.iter().find(|m| m.id == *id),
                };
                if let Some(m) = m {
                    redo.push((*cid, Message { content: raw.clone(), ..m.clone() }));
                }
            }
        }
        for (cid, m) in redo {
            let rendered = self.render_content(&m);
            let Some(cs) = self.chans.get_mut(&cid) else { continue };
            if let Ok(i) = cs.msgs.binary_search_by_key(&m.id, |x| x.id)
                && let Some(slot) = cs.msgs.get_mut(i)
            {
                slot.content = rendered;
                cs.rev.bump();
            } else if let Some(slot) = cs.live.iter_mut().find(|x| x.id == m.id) {
                slot.content = rendered;
            }
        }
    }

    // ------------------------------------------------------------------ navigation

    pub fn select_channel(&mut self, id: u64) {
        // Salon marqué non lu à la main (lot M), quitté ou rouvert : il se relit.
        self.reads.release();
        if self.channel != Some(id) {
            // La ligne « Nouveaux messages » ne vaut que pour une visite.
            if let Some(cs) = self.channel.and_then(|c| self.chans.get_mut(&c)) {
                cs.new_marker = None;
            }
            let marker = self.unread_from.get(&id).copied();
            self.chans.entry(id).or_default().new_marker = marker;
        }
        self.channel = Some(id);
        // Mode discret : le salon ouvert reste non lu jusqu'au clic sur « Marquer comme lu ».
        if !self.reads.manual() {
            self.clear_unread(id);
        }
        self.ensure_history(id);
        self.chans.entry(id).or_default().last_view = Some(Instant::now());
        self.evict_old_channels();
    }

    /// Ouvre la liste des messages privés.
    pub fn select_dms(&mut self) {
        self.guild = None;
        self.channel = None;
    }

    /// Ouvre un serveur et son premier salon visible. Renvoie `true` si un salon a été sélectionné.
    pub fn select_guild(&mut self, id: u64) -> bool {
        if self.guild == Some(id) {
            return false;
        }
        self.guild = Some(id);
        self.channel = None;
        let first = self
            .guilds
            .iter()
            .find(|g| g.id == id)
            .and_then(|g| g.channels.iter().find(|c| c.kind != ChannelKind::Category && !c.kind.is_forum() && !self.is_hidden(c.id)))
            .map(|c| c.id);
        match first {
            Some(c) => {
                self.select_channel(c);
                true
            }
            None => false,
        }
    }

    /// Demande l'historique s'il n'est ni chargé ni en cours ; sans
    /// READ_MESSAGE_HISTORY, rien n'est demandé et le salon n'affichera que
    /// les nouveaux messages.
    pub fn ensure_history(&mut self, id: u64) {
        // Un forum n'a pas de messages : la vue forum liste ses publications.
        if self.is_forum(id) {
            return;
        }
        let denied = self.can(id, Permissions::READ_MESSAGE_HISTORY) == Verdict::Refuse;
        let cs = self.chans.entry(id).or_default();
        // Historique hors ligne : affiché, et la dernière page est demandée.
        if (cs.loaded && !cs.offline) || cs.loading {
            return;
        }
        if denied {
            if std::mem::take(&mut cs.offline) {
                cs.msgs.clear();
                cs.rev.bump();
            }
            cs.history_denied = true;
            cs.loaded = true;
            cs.more = false;
            return;
        }
        cs.loading = true;
        cs.error = None;
        self.outbox.push(Cmd::History { channel_id: id, before: None });
    }

    /// L'historique de ce salon peut-il être demandé ? Sans
    /// READ_MESSAGE_HISTORY, aucune page n'est demandée, dans aucun sens.
    fn history_readable(&self, channel_id: u64) -> bool {
        self.can(channel_id, Permissions::READ_MESSAGE_HISTORY) != Verdict::Refuse
            && self.chans.get(&channel_id).is_some_and(|cs| cs.loaded && !cs.history_denied)
    }

    /// Demande la page qui précède le plus ancien message en mémoire. Une
    /// seule demande à la fois, et aucune quand le début du salon est atteint.
    /// Renvoie `true` si la demande part.
    pub fn load_older(&mut self, channel_id: u64) -> bool {
        if !self.history_readable(channel_id) {
            return false;
        }
        let Some(cs) = self.chans.get_mut(&channel_id) else { return false };
        if cs.loading || cs.asked_older.is_some() || !cs.more {
            return false;
        }
        let Some(before) = cs.msgs.front().map(|m| m.id) else { return false };
        cs.asked_older = Some(before);
        cs.error = None;
        self.outbox.push(Cmd::History { channel_id, before: Some(before) });
        true
    }

    /// Demande la page qui suit le plus récent message en mémoire, quand la
    /// fenêtre a quitté le présent. Une seule demande à la fois.
    pub fn load_newer(&mut self, channel_id: u64) -> bool {
        if !self.history_readable(channel_id) {
            return false;
        }
        let Some(cs) = self.chans.get_mut(&channel_id) else { return false };
        if !cs.newer || cs.asked_newer.is_some() || cs.loading {
            return false;
        }
        let Some(after) = cs.msgs.back().map(|m| m.id) else { return false };
        cs.asked_newer = Some(after);
        cs.error_newer = None;
        self.outbox.push(Cmd::HistoryAfter { channel_id, after });
        true
    }

    /// « Revenir au présent » : la dernière page remplace la fenêtre remontée.
    /// Sans effet si la fenêtre touche déjà le présent ; renvoie `true` si la
    /// demande part. Une page plus ancienne en cours sera ignorée à son arrivée.
    pub fn return_to_present(&mut self, channel_id: u64) -> bool {
        if !self.history_readable(channel_id) {
            return false;
        }
        let Some(cs) = self.chans.get_mut(&channel_id) else { return false };
        // Historique hors ligne dont le rafraîchissement a échoué : « Réessayer ».
        if (!cs.newer && !cs.offline) || cs.loading {
            return false;
        }
        cs.loading = true;
        cs.error_newer = None;
        self.outbox.push(Cmd::History { channel_id, before: None });
        true
    }

    /// Messages affichés (premier et dernier) : l'éviction n'y touche pas.
    pub fn set_viewing(&mut self, channel_id: u64, range: Option<(u64, u64)>) {
        if let Some(cs) = self.chans.get_mut(&channel_id) {
            cs.viewing = range;
        }
    }

    /// Libère l'historique des salons consultés il y a longtemps.
    fn evict_old_channels(&mut self) {
        if self.chans.len() <= MAX_CACHED_CHANNELS {
            return;
        }
        let mut by_age: Vec<(u64, Option<Instant>)> = self
            .chans
            .iter()
            .filter(|(id, cs)| Some(**id) != self.channel && !cs.loading)
            .map(|(id, cs)| (*id, cs.last_view))
            .collect();
        by_age.sort_by_key(|(_, t)| *t);
        let excess = self.chans.len() - MAX_CACHED_CHANNELS;
        for (id, _) in by_age.into_iter().take(excess) {
            self.chans.remove(&id);
        }
    }

    /// (non-lu, mention) d'un serveur, sans compter les salons masqués :
    /// une seule recherche dans `unread_index`.
    pub fn guild_has_unread(&self, g: &Guild) -> (bool, bool) {
        self.unread_index.get(&g.id).map_or((false, false), |(u, m)| (*u > 0, *m > 0))
    }

    /// Salons mentionnés visibles d'un serveur (pastille du rail et des dossiers).
    pub fn guild_mentions(&self, id: u64) -> u32 {
        self.unread_index.get(&id).map_or(0, |(_, m)| *m)
    }

    pub fn unread(&self) -> &HashSet<u64> {
        &self.unread
    }

    pub fn mentioned(&self) -> &HashSet<u64> {
        &self.mentioned
    }

    /// Mentions non lues de ce salon (0 s'il n'est pas mentionné, 1 au moins sinon).
    pub fn mention_count(&self, channel: u64) -> u32 {
        if self.mentioned.contains(&channel) { self.mention_counts.get(&channel).copied().unwrap_or(1).max(1) } else { 0 }
    }

    /// Compte de mentions annoncé par Discord pour un salon déjà marqué.
    pub(super) fn set_mention_count(&mut self, channel: u64, n: u32) {
        if n > 0 && self.mentioned.contains(&channel) {
            self.mention_counts.insert(channel, n);
        }
    }

    /// Serveur dont les pastilles comptent ce salon (aucun s'il est masqué ou privé).
    fn counted_in(&self, c: u64) -> Option<u64> {
        let g = *self.chan_guild.get(&c)?;
        (!self.is_hidden(c)).then_some(g)
    }

    fn mark_unread(&mut self, c: u64, ping: bool) {
        let g = self.counted_in(c);
        let new_unread = self.unread.insert(c);
        let new_ping = ping && self.mentioned.insert(c);
        if ping {
            let n = self.mention_counts.entry(c).or_insert(0);
            *n = n.saturating_add(1);
        }
        if let Some(g) = g
            && (new_unread || new_ping)
        {
            let e = self.unread_index.entry(g).or_default();
            e.0 += u32::from(new_unread);
            e.1 += u32::from(new_ping);
        }
    }

    fn clear_unread(&mut self, c: u64) {
        self.unread_from.remove(&c);
        self.mention_counts.remove(&c);
        let g = self.counted_in(c);
        let (was_unread, was_ping) = (self.unread.remove(&c), self.mentioned.remove(&c));
        if let Some(g) = g
            && let Some(e) = self.unread_index.get_mut(&g)
        {
            e.0 = e.0.saturating_sub(u32::from(was_unread));
            e.1 = e.1.saturating_sub(u32::from(was_ping));
            if *e == (0, 0) {
                self.unread_index.remove(&g);
            }
        }
    }

    /// Reconstruit `unread_index` (après un changement de permissions ou de
    /// salons) : parcourt les non-lus, pas les salons.
    fn rebuild_unread_index(&mut self) {
        let mut index: HashMap<u64, (u32, u32)> = HashMap::new();
        for c in &self.unread {
            if let Some(g) = self.counted_in(*c) {
                index.entry(g).or_default().0 += 1;
            }
        }
        for c in &self.mentioned {
            if let Some(g) = self.counted_in(*c) {
                index.entry(g).or_default().1 += 1;
            }
        }
        self.unread_index = index;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::notice::SystemKind;
    use crate::state::event::DmKnown;
    use crate::i18n;
    use crate::state::model::{Channel, ChannelKind};

    const CHAN: u64 = 100;
    const ME: u64 = 1;

    /// Heure rendue en français (langue du magasin par défaut).
    fn clock(ms: i64, full: bool, h24: bool) -> String {
        i18n::time::clock(Lang::Fr, ms, full, h24)
    }

    /// Ce qui est rendu à la réception suit la langue : refait une fois au
    /// changement (message système) ; la source des autres reste telle
    /// quelle (balises mises en mots par `markdown`, à l'affichage).
    #[test]
    fn language_change_rerenders_received_texts_once() {
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        let mut joined = msg(13, CHAN, 2, "");
        joined.system = Some(SystemKind::Joined);
        s.apply(Event::History {
            channel_id: CHAN,
            before: None,
            messages: vec![msg(10, CHAN, 2, "rdv <t:1700000000> ok"), msg(11, CHAN, 2, "<@77> et <@&5> dans <#555>"), msg(12, CHAN, 2, "texte simple"), joined],
        });
        let content = |s: &Store, id: u64| s.chans[&CHAN].msgs.iter().find(|m| m.id == id).map(|m| m.content.clone()).unwrap_or_default();
        assert_eq!(content(&s, 11), "<@77> et <@&5> dans <#555>");
        assert_eq!(content(&s, 13), crate::state::system_msgs::text(Lang::Fr, SystemKind::Joined, &msg(13, CHAN, 2, ""), Default::default()));
        let rev = s.chans[&CHAN].rev;
        s.set_language(Lang::En);
        assert_eq!(content(&s, 10), "rdv <t:1700000000> ok");
        assert_eq!(content(&s, 11), "<@77> et <@&5> dans <#555>");
        assert_eq!(content(&s, 12), "texte simple");
        assert_eq!(content(&s, 13), crate::state::system_msgs::text(Lang::En, SystemKind::Joined, &msg(13, CHAN, 2, ""), Default::default()));
        assert_ne!(s.chans[&CHAN].rev, rev, "les hauteurs seront refaites");
        let rev = s.chans[&CHAN].rev;
        s.set_language(Lang::En);
        assert_eq!(s.chans[&CHAN].rev, rev, "même langue : rien n'est refait");
        // Arrivé après le changement : directement dans la nouvelle langue.
        let mut pinned = msg(14, CHAN, 2, "");
        pinned.system = Some(SystemKind::Pinned);
        s.apply(Event::MessageCreate(pinned));
        assert_eq!(content(&s, 14), crate::state::system_msgs::text(Lang::En, SystemKind::Pinned, &msg(14, CHAN, 2, ""), Default::default()));
        s.set_language(Lang::Fr);
        assert_eq!(content(&s, 14), crate::state::system_msgs::text(Lang::Fr, SystemKind::Pinned, &msg(14, CHAN, 2, ""), Default::default()));
        assert_eq!(content(&s, 10), "rdv <t:1700000000> ok");
        assert_eq!(content(&s, 11), "<@77> et <@&5> dans <#555>");
    }

    /// Commandes sans leur nonce (choisi d'après l'heure).
    fn no_nonce(cmds: Vec<Cmd>) -> Vec<Cmd> {
        cmds.into_iter()
            .map(|c| match c {
                Cmd::Send { channel_id, content, .. } => Cmd::Send { channel_id, content, nonce: 0 },
                other => other,
            })
            .collect()
    }

    fn sent_nonce(s: &mut Store) -> u64 {
        match s.take_commands().as_slice() {
            [Cmd::Send { nonce, .. }] => *nonce,
            other => panic!("un envoi attendu : {other:?}"),
        }
    }

    #[test]
    fn nonces_are_unique_increasing_and_short() {
        let mut s = Store::new();
        let mut last = 0;
        for _ in 0..100 {
            assert!(s.send_message(CHAN, "x".into()));
            let n = sent_nonce(&mut s);
            assert!(n > last && n.to_string().len() <= 25);
            last = n;
        }
    }

    #[test]
    fn failed_send_keeps_the_text_until_retried_or_discarded() {
        let mut s = Store::new();
        assert!(s.send_message(CHAN, "important".into()));
        let nonce = sent_nonce(&mut s);
        let eff = s.apply(Event::SendError { channel_id: CHAN, nonce, content: "important".into(), error: Problem::Api("Trop de requêtes".into()), forbidden: false });
        assert_eq!(eff, Some(Effect::Toast(Notice::NotSent(Problem::Api("Trop de requêtes".into())))));
        assert_eq!(s.unsent[&CHAN], vec![Unsent { nonce, content: "important".into(), state: UnsentState::Failed(Problem::Api("Trop de requêtes".into())) }]);
        // Réessayer : même texte, même nonce ; un second clic ne renvoie rien.
        assert!(s.retry_unsent(CHAN, nonce));
        assert!(!s.retry_unsent(CHAN, nonce));
        assert_eq!(s.take_commands(), vec![Cmd::Send { channel_id: CHAN, content: "important".into(), nonce }]);
        assert_eq!(s.unsent[&CHAN][0].state, UnsentState::Retrying);
        // Nouvel échec : la même entrée, pas de doublon.
        s.apply(Event::SendError { channel_id: CHAN, nonce, content: "important".into(), error: Problem::Api("Réseau".into()), forbidden: false });
        assert_eq!(s.unsent[&CHAN].len(), 1);
        // Confirmé : retiré.
        s.apply(Event::SendConfirmed { channel_id: CHAN, nonce });
        assert!(!s.unsent.contains_key(&CHAN));
        // Supprimer.
        s.apply(Event::SendUnconfirmed { channel_id: CHAN, nonce: 7, content: "peut-être".into() });
        assert_eq!(s.unsent[&CHAN][0].state, UnsentState::Unconfirmed);
        s.discard_unsent(CHAN, 7);
        assert!(s.unsent.is_empty());
    }

    #[test]
    fn unsent_messages_are_bounded() {
        let mut s = Store::new();
        for c in 0..MAX_UNSENT_CHANNELS as u64 + 5 {
            for n in 0..MAX_UNSENT_PER_CHANNEL as u64 + 5 {
                s.apply(Event::SendError { channel_id: c, nonce: c * 1000 + n, content: "x".into(), error: Problem::Api("e".into()), forbidden: false });
            }
        }
        assert!(s.unsent.len() <= MAX_UNSENT_CHANNELS);
        assert!(s.unsent.values().all(|l| l.len() <= MAX_UNSENT_PER_CHANNEL));
        // Les plus récents sont gardés.
        assert!(s.unsent.contains_key(&(MAX_UNSENT_CHANNELS as u64 + 4)));
    }

    #[test]
    fn cancelled_history_leaves_no_error() {
        let mut s = Store::new();
        s.ensure_history(CHAN);
        s.take_commands();
        s.apply(Event::HistoryCancelled { channel_id: CHAN, before: None, after: None });
        let cs = &s.chans[&CHAN];
        assert!(!cs.loading && cs.error.is_none() && !cs.loaded);
        // Rouvert : redemandé.
        s.ensure_history(CHAN);
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
    }

    fn msg(id: u64, channel_id: u64, author_id: u64, content: &str) -> Message {
        Message {
            id,
            channel_id,
            guild_id: Some(9),
            author_id,
            author: format!("u{author_id}"),
            author_avatar: crate::state::model::Avatar::default_for(author_id),
            content: content.into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    /// Les balises `<t:…>` restent dans la source : l'option 12 h ne refait
    /// rien dans le magasin, elle s'applique à la mise en forme.
    #[test]
    fn clock_option_leaves_sources_untouched() {
        let ms = 1_700_000_000_000;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        s.apply(Event::History { channel_id: CHAN, before: None, messages: vec![msg(10, CHAN, 2, "rdv <t:1700000000:t> ok"), msg(11, CHAN, 2, "sans heure")] });
        s.apply(Event::MessageCreate(msg(12, CHAN, 2, "<t:1700000000>")));
        let rev = s.chans[&CHAN].rev;
        s.set_clock12(true);
        assert_eq!(s.chans[&CHAN].rev, rev);
        assert!(s.chans[&CHAN].stamped.is_empty());
        let m = &s.chans[&CHAN].msgs[0];
        assert_eq!(m.content, "rdv <t:1700000000:t> ok");
        let shown = |h24: bool| -> String {
            let doc = crate::state::markdown::parse_message(&s, m, Lang::Fr, h24);
            doc.blocks.iter().flat_map(|b| b.spans.iter().map(|sp| sp.text.as_str())).collect()
        };
        assert_eq!(shown(false), format!("rdv {} ok", clock(ms, false, false)));
        assert_eq!(shown(true), format!("rdv {} ok", clock(ms, false, true)));
    }

    fn store() -> Store {
        let mut s = Store::new();
        s.me = Some(User::new(ME, "moi".into()));
        s.guilds.push(Guild {
            id: 9,
            name: "G".into(),
            channels: vec![Channel { id: CHAN, name: "général".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 }],
            owner_id: None,
            roles: Vec::new(),
            me: None,
            roles_unreadable: false,
            icon: None,
            emojis: Vec::new(),
            premium_tier: 0,
            boosts: 0,
            banner: None, tag: None,
        });
        s.refresh_permissions();
        s
    }

    fn ids(s: &Store, ch: u64) -> Vec<u64> {
        s.chans[&ch].msgs.iter().map(|m| m.id).collect()
    }

    fn history(s: &mut Store, before: Option<u64>, range: std::ops::Range<u64>) -> Option<Effect> {
        let messages = range.map(|i| msg(i, CHAN, 2, "x")).collect();
        s.apply(Event::History { channel_id: CHAN, before, messages })
    }

    // ------------------------------------------------------------ texte des messages

    /// Texte mis en forme d'un message reçu, mentions mises en mots par le magasin.
    fn render(s: &Store, content: &str, mentions: Vec<(u64, String)>) -> String {
        let mut m = msg(1, CHAN, 2, content);
        m.mentions = mentions;
        m.guild_id = Some(9);
        let doc = crate::state::markdown::parse_message(s, &m, Lang::Fr, true);
        doc.blocks.iter().flat_map(|b| b.spans.iter().map(|sp| sp.text.as_str())).collect()
    }

    /// Le magasin garde la source telle quelle (copie, modification) ; seule la mise en forme la remplace.
    #[test]
    fn source_is_kept_as_received() {
        let s = store();
        let m = msg(1, CHAN, 2, "**salut** <@42> <t:1700000000>");
        assert_eq!(s.render_content(&m), "**salut** <@42> <t:1700000000>");
    }

    #[test]
    fn render_user_mentions() {
        let s = store();
        let men = vec![(42, "Alice".to_string())];
        assert_eq!(render(&s, "salut <@42> !", men.clone()), "salut @Alice !");
        assert_eq!(render(&s, "<@!42>", men.clone()), "@Alice");
        assert_eq!(render(&s, "<@77>", men), "@utilisateur");
        assert_eq!(render(&s, "<@&5> go", vec![]), "@rôle go");
    }

    #[test]
    fn render_roles_with_their_color() {
        let mut s = store();
        s.guilds[0].roles.push(crate::state::model::Role { id: 5, name: "Modo".into(), color: 0x3498db, position: 1, permissions: Permissions::NONE, mentionable: false });
        let mut m = msg(1, CHAN, 2, "<@&5>");
        m.guild_id = Some(9);
        let doc = crate::state::markdown::parse_message(&s, &m, Lang::Fr, true);
        let span = &doc.blocks[0].spans[0];
        assert_eq!(span.text, "@Modo");
        assert_eq!(span.kind, crate::state::markdown::SpanKind::Mention(crate::state::markdown::Mention::Role { id: 5, color: 0x3498db }));
    }

    #[test]
    fn render_channels() {
        let s = store();
        assert_eq!(render(&s, "voir <#100>", vec![]), "voir #général");
        assert_eq!(render(&s, "voir <#555>", vec![]), "voir #salon");
    }

    #[test]
    fn render_custom_emojis() {
        let s = store();
        assert_eq!(render(&s, "<:rust:123> et <a:dance:456>", vec![]), ":rust: et :dance:");
    }

    #[test]
    fn render_timestamps() {
        let s = store();
        let ms = 1_700_000_000_000;
        let long = i18n::time::stamp(Lang::Fr, ms, now_ms(), 'f', true);
        assert_eq!(render(&s, "le <t:1700000000:f>", vec![]), format!("le {long}"));
        assert_eq!(render(&s, "<t:1700000000>", vec![]), long);
        assert_eq!(render(&s, "<t:1700000000:t>", vec![]), clock(ms, false, true));
        // Valeur qui déborderait en millisecondes : laissée telle quelle, sans paniquer.
        assert_eq!(render(&s, "<t:9223372036854775807>", vec![]), "<t:9223372036854775807>");
    }

    #[test]
    fn render_unknown_and_unclosed_tags() {
        let s = store();
        assert_eq!(render(&s, "a <b> c", vec![]), "a <b> c");
        assert_eq!(render(&s, "2 < 3 et <@42", vec![]), "2 < 3 et <@42");
        assert_eq!(render(&s, "<#100> puis <", vec![]), "#général puis <");
        assert_eq!(render(&s, "<>", vec![]), "<>");
    }

    // ------------------------------------------------------------ apply

    #[test]
    fn history_initial_loads_and_scrolls_when_selected() {
        let mut s = store();
        s.select_channel(CHAN);
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
        assert!(s.chans[&CHAN].loading);

        let eff = history(&mut s, None, 1..(HISTORY_PAGE as u64 + 1));
        assert_eq!(eff, Some(Effect::ScrollToBottom));
        let cs = &s.chans[&CHAN];
        assert!(cs.loaded && !cs.loading && cs.more);
        assert_eq!(cs.msgs.len(), HISTORY_PAGE);
    }

    #[test]
    fn history_initial_not_selected_no_scroll_and_short_page() {
        let mut s = store();
        let eff = history(&mut s, None, 1..4);
        assert_eq!(eff, None);
        assert!(!s.chans[&CHAN].more);
        assert_eq!(ids(&s, CHAN), vec![1, 2, 3]);
    }

    #[test]
    fn history_before_prepends_in_order() {
        let page = HISTORY_PAGE as u64;
        let mut s = store();
        history(&mut s, None, 1000..1000 + page);
        assert!(s.load_older(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: Some(1000) }]);
        assert!(s.chans[&CHAN].loading_older());
        // Désordre et doublon reçus : remis en ordre, doublon ignoré.
        let messages = vec![msg(7, CHAN, 2, "x"), msg(5, CHAN, 2, "x"), msg(6, CHAN, 2, "x"), msg(1000, CHAN, 2, "x")];
        let eff = s.apply(Event::History { channel_id: CHAN, before: Some(1000), messages });
        assert_eq!(eff, None);
        assert_eq!(ids(&s, CHAN)[..5], [5, 6, 7, 1000, 1001]);
        assert_eq!(s.chans[&CHAN].msgs.len(), HISTORY_PAGE + 3);
        assert!(!s.chans[&CHAN].loading_older());
        assert!(!s.chans[&CHAN].more);
        // Début du salon atteint : plus aucune demande.
        assert!(!s.load_older(CHAN));
        assert!(s.take_commands().is_empty());
    }

    /// Chargement de l'historique ancien : une seule demande à la fois, et
    /// aucune nouvelle tant que la précédente n'a pas abouti (même en échec).
    #[test]
    fn load_older_is_requested_once() {
        let page = HISTORY_PAGE as u64;
        let mut s = store();
        history(&mut s, None, 1000..1000 + page);
        for _ in 0..5 {
            s.load_older(CHAN);
        }
        assert_eq!(s.take_commands().len(), 1);
        s.apply(Event::HistoryError { channel_id: CHAN, before: Some(1000), after: None, error: Problem::Api("oups".into()) });
        assert!(!s.chans[&CHAN].loading_older());
        assert_eq!(s.chans[&CHAN].error.as_ref(), Some(&Problem::Api("oups".into())));
        // « Réessayer » : une nouvelle demande, une seule.
        assert!(s.load_older(CHAN));
        assert!(!s.load_older(CHAN));
        assert_eq!(s.take_commands().len(), 1);
        assert!(s.chans[&CHAN].error.is_none());
        // Salon inconnu ou pas encore chargé : rien.
        assert!(!s.load_older(4242));
    }

    /// Fenêtre glissante : en remontant loin, les plus récents (non affichés)
    /// sont évincés et rechargés en redescendant ; la mémoire reste bornée.
    #[test]
    fn cache_window_slides_both_ways() {
        let page = HISTORY_PAGE as u64;
        let max = MAX_CACHED_MESSAGES;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        let top = 1_000_000;
        history(&mut s, None, top..top + page);
        // Remonte 20 pages : jamais plus de `max` messages en mémoire.
        for k in 1..=20u64 {
            let front = s.chans[&CHAN].msgs.front().map_or(0, |m| m.id);
            let first = s.chans[&CHAN].msgs.front().map(|m| m.id);
            s.set_viewing(CHAN, first.map(|f| (f, f + 3)));
            assert!(s.load_older(CHAN), "page {k}");
            let cmds = s.take_commands();
            assert_eq!(cmds, vec![Cmd::History { channel_id: CHAN, before: Some(front) }]);
            history(&mut s, Some(front), front - page..front);
            let cs = &s.chans[&CHAN];
            assert!(cs.msgs.len() <= max, "{}", cs.msgs.len());
            // Les messages affichés restent.
            assert!(first.is_some_and(|f| cs.contains(f)));
        }
        let cs = &s.chans[&CHAN];
        assert!(cs.newer, "les plus récents ont été évincés");
        assert_eq!(cs.msgs.len(), max);
        assert_eq!(cs.msgs.back().map(|m| m.id), Some(top + page - 1 - (21 * page - max as u64)));
        // Un message en direct pendant ce temps : compté, pas ajouté au milieu.
        s.apply(Event::MessageCreate(msg(top + page + 5, CHAN, 2, "direct")));
        assert_eq!(s.chans[&CHAN].missed(), 1);
        assert!(!s.chans[&CHAN].contains(top + page + 5));
        // Redescente : page suivante demandée, une à la fois.
        let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
        let last = s.chans[&CHAN].msgs.back().map(|m| m.id);
        s.set_viewing(CHAN, last.map(|l| (l - 3, l)));
        assert!(s.load_newer(CHAN));
        assert!(!s.load_newer(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::HistoryAfter { channel_id: CHAN, after: back }]);
        let next: Vec<Message> = (back + 1..back + 1 + page).rev().map(|i| msg(i, CHAN, 2, "x")).collect();
        s.apply(Event::HistoryAfter { channel_id: CHAN, after: back, messages: next });
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.msgs.len(), max);
        assert_eq!(cs.msgs.back().map(|m| m.id), Some(back + page));
        assert!(cs.more, "les plus anciens sont repartis");
        assert!(cs.newer);
        assert!(cs.msgs.iter().zip(cs.msgs.iter().skip(1)).all(|(a, b)| a.id < b.id));
        // Dernière page (incomplète) : retour au présent, message en direct compris.
        // Pages suivantes jusqu'à une page incomplète (ici vide).
        let mut pages = 0;
        while s.chans[&CHAN].newer {
            pages += 1;
            assert!(pages < 30);
            let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
            let last = s.chans[&CHAN].msgs.back().map(|m| m.id);
            s.set_viewing(CHAN, last.map(|l| (l - 3, l)));
            assert!(s.load_newer(CHAN));
            s.take_commands();
            let next: Vec<Message> = (back + 1..(back + 1 + page).min(top + page)).map(|i| msg(i, CHAN, 2, "x")).collect();
            s.apply(Event::HistoryAfter { channel_id: CHAN, after: back, messages: next });
            assert!(s.chans[&CHAN].msgs.len() <= max);
        }
        let cs = &s.chans[&CHAN];
        assert!(!cs.newer);
        assert_eq!(cs.missed(), 0);
        assert_eq!(cs.msgs.back().map(|m| m.id), Some(top + page + 5));
        assert!(cs.msgs.len() <= max);
        assert!(!s.load_newer(CHAN));
    }

    /// « Revenir au présent » : la dernière page remplace la fenêtre.
    #[test]
    fn return_to_present_reloads_latest_page() {
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        history(&mut s, None, 1..4);
        assert!(!s.return_to_present(CHAN), "déjà au présent");
        if let Some(c) = s.chans.get_mut(&CHAN) {
            c.newer = true;
        }
        assert!(s.return_to_present(CHAN));
        assert!(!s.return_to_present(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
        assert_eq!(history(&mut s, None, 50..53), Some(Effect::ScrollToBottom));
        assert!(!s.chans[&CHAN].newer);
        assert_eq!(ids(&s, CHAN), vec![50, 51, 52]);
    }

    /// Un message en direct affiché en haut de la fenêtre pleine n'est pas
    /// évincé : c'est le plus récent qui part, et la fenêtre quitte le présent.
    #[test]
    fn live_overflow_keeps_viewed_messages() {
        let mut s = store();
        history(&mut s, None, 1..(MAX_CACHED_MESSAGES as u64 + 1));
        s.set_viewing(CHAN, Some((1, 5)));
        s.apply(Event::MessageCreate(msg(10_000, CHAN, 2, "x")));
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.msgs.front().map(|m| m.id), Some(1));
        assert_eq!(cs.msgs.len(), MAX_CACHED_MESSAGES);
        assert!(cs.newer);
        // Le message arrivé (sorti aussitôt de la fenêtre) est compté comme manqué.
        assert_eq!(cs.missed(), 1);
        // Rien d'affiché : le plus ancien part, comme avant.
        let mut s = store();
        history(&mut s, None, 1..(MAX_CACHED_MESSAGES as u64 + 1));
        s.apply(Event::MessageCreate(msg(10_000, CHAN, 2, "x")));
        assert_eq!(s.chans[&CHAN].msgs.front().map(|m| m.id), Some(2));
        assert!(!s.chans[&CHAN].newer);
    }

    /// Remonte la fenêtre de `pages` pages (vue en haut) : les plus récents sont évincés.
    fn climb(s: &mut Store, pages: usize) {
        let page = HISTORY_PAGE as u64;
        for _ in 0..pages {
            let front = s.chans[&CHAN].msgs.front().map_or(0, |m| m.id);
            s.set_viewing(CHAN, Some((front, front + 3)));
            assert!(s.load_older(CHAN));
            s.take_commands();
            history(s, Some(front), front - page..front);
        }
    }

    /// Un message parti reste « en cours d'envoi » (montré estompé) jusqu'à
    /// sa confirmation, son refus, ou son retour par la passerelle ; borné.
    #[test]
    fn sending_messages_are_tracked_until_they_land() {
        let mut s = store();
        s.select_channel(CHAN);
        assert!(s.send_message(CHAN, "un".into()) && s.send_message(CHAN, "deux".into()) && s.send_message(CHAN, "trois".into()));
        let nonces: Vec<u64> = s.sending.iter().map(|x| x.1).collect();
        assert_eq!(s.sending.iter().map(|x| x.2.as_str()).collect::<Vec<_>>(), ["un", "deux", "trois"]);
        s.apply(Event::SendConfirmed { channel_id: CHAN, nonce: nonces[0] });
        s.apply(Event::MessageCreate(msg(9_000_001, CHAN, ME, "deux")));
        assert_eq!(s.sending.iter().map(|x| x.2.as_str()).collect::<Vec<_>>(), ["trois"], "confirmé, puis revenu par la passerelle");
        s.apply(Event::SendUnconfirmed { channel_id: CHAN, nonce: nonces[2], content: "trois".into() });
        assert!(s.sending.is_empty(), "non confirmé : il passe dans les messages non envoyés");
        for i in 0..(MAX_SENDING + 5) {
            s.send_message(CHAN, format!("m{i}"));
        }
        assert_eq!(s.sending.len(), MAX_SENDING);
    }

    /// Envoyer un message depuis une fenêtre remontée loin : le présent est
    /// rechargé et le message envoyé y apparaît.
    #[test]
    fn sending_from_far_up_returns_to_present() {
        let page = HISTORY_PAGE as u64;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        let top = 1_000_000;
        history(&mut s, None, top..top + page);
        climb(&mut s, 12);
        assert!(s.chans[&CHAN].newer);
        assert!(s.send_message(CHAN, "coucou".into()));
        assert_eq!(no_nonce(s.take_commands()), vec![Cmd::Send { channel_id: CHAN, content: "coucou".into(), nonce: 0 }, Cmd::History { channel_id: CHAN, before: None }]);
        // Mon message arrive avant la page : gardé, sans compter comme manqué.
        s.apply(Event::MessageCreate(msg(top + page + 1, CHAN, ME, "coucou")));
        assert_eq!(s.chans[&CHAN].missed(), 0);
        assert_eq!(history(&mut s, None, top..top + page), Some(Effect::ScrollToBottom));
        let cs = &s.chans[&CHAN];
        assert!(!cs.newer);
        assert_eq!(cs.msgs.back().map(|m| m.id), Some(top + page + 1));
        // Au présent, l'envoi ne recharge rien.
        assert!(s.send_message(CHAN, "encore".into()));
        assert_eq!(s.take_commands().len(), 1);
    }

    /// Chaque sens a son état : un échec vers le présent ne bloque pas le
    /// passé et s'affiche en bas ; chaque drapeau n'est levé que par la
    /// réponse de sa propre demande.
    /// Demandes annulées (salon quitté) : chacune ne termine que son propre
    /// chargement, passé ou présent, sans afficher d'erreur ; une annulation
    /// périmée ne change rien.
    #[test]
    fn cancelled_pages_end_only_their_own_loading() {
        let page = HISTORY_PAGE as u64;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        let top = 1_000_000;
        history(&mut s, None, top..top + page);
        climb(&mut s, 12);
        let front = s.chans[&CHAN].msgs.front().map_or(0, |m| m.id);
        let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
        s.set_viewing(CHAN, Some((front + 100, front + 103)));
        assert!(s.load_older(CHAN) && s.load_newer(CHAN));
        s.take_commands();
        // Annulation d'une autre demande : rien ne change.
        s.apply(Event::HistoryCancelled { channel_id: CHAN, before: Some(front + 1), after: None });
        assert!(s.chans[&CHAN].loading_older() && s.chans[&CHAN].loading_newer());
        // Vers le présent : seul le bas se termine.
        s.apply(Event::HistoryCancelled { channel_id: CHAN, before: None, after: Some(back) });
        let cs = &s.chans[&CHAN];
        assert!(cs.loading_older() && !cs.loading_newer());
        // Vers le passé : le haut se termine.
        s.apply(Event::HistoryCancelled { channel_id: CHAN, before: Some(front), after: None });
        let cs = &s.chans[&CHAN];
        assert!(!cs.loading_older() && !cs.loading_newer() && !cs.loading);
        assert!(cs.error.is_none() && cs.error_newer.is_none());
        // Les deux sens sont redemandés au retour.
        assert!(s.load_older(CHAN) && s.load_newer(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: Some(front) }, Cmd::HistoryAfter { channel_id: CHAN, after: back }]);
        // Retour au présent annulé : terminé, sans erreur.
        assert!(s.return_to_present(CHAN));
        s.apply(Event::HistoryCancelled { channel_id: CHAN, before: None, after: None });
        let cs = &s.chans[&CHAN];
        assert!(!cs.loading && cs.error.is_none() && cs.error_newer.is_none());
    }

    #[test]
    fn history_errors_are_per_direction() {
        let page = HISTORY_PAGE as u64;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        let top = 1_000_000;
        history(&mut s, None, top..top + page);
        climb(&mut s, 12);
        let front = s.chans[&CHAN].msgs.front().map_or(0, |m| m.id);
        let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
        s.set_viewing(CHAN, Some((front + 100, front + 103)));
        assert!(s.load_older(CHAN));
        assert!(s.load_newer(CHAN));
        assert!(!s.load_older(CHAN), "une à la fois vers le passé");
        s.take_commands();
        // L'échec de la page plus récente ne termine que celle-là.
        s.apply(Event::HistoryError { channel_id: CHAN, before: None, after: Some(back), error: Problem::Api("réseau".into()) });
        let cs = &s.chans[&CHAN];
        assert!(cs.loading_older() && !cs.loading_newer());
        assert_eq!((cs.error.as_ref(), cs.error_newer.as_ref()), (None, Some(&Problem::Api("réseau".into()))));
        assert!(!s.load_older(CHAN), "toujours une seule vers le passé");
        // Un échec d'une autre demande (repère différent) ne change rien.
        s.apply(Event::HistoryError { channel_id: CHAN, before: Some(front + 1), after: None, error: Problem::Api("autre".into()) });
        assert!(s.chans[&CHAN].loading_older());
        // La page plus ancienne arrive : le passé continue malgré l'erreur du bas.
        history(&mut s, Some(front), front - page..front);
        assert!(!s.chans[&CHAN].loading_older());
        assert_eq!(s.chans[&CHAN].error_newer.as_ref(), Some(&Problem::Api("réseau".into())));
        // « Réessayer » du bas : la page plus récente, depuis le bord actuel.
        let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
        assert!(s.load_newer(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::HistoryAfter { channel_id: CHAN, after: back }]);
        assert!(s.chans[&CHAN].error_newer.is_none());
        // Échec du retour au présent : affiché en bas aussi.
        s.apply(Event::HistoryAfter { channel_id: CHAN, after: back, messages: Vec::new() });
        assert!(!s.chans[&CHAN].newer);
        let mut s2 = store();
        s2.select_channel(CHAN);
        s2.take_commands();
        history(&mut s2, None, top..top + page);
        climb(&mut s2, 12);
        assert!(s2.return_to_present(CHAN));
        s2.apply(Event::HistoryError { channel_id: CHAN, before: None, after: None, error: Problem::Api("panne".into()) });
        let cs = &s2.chans[&CHAN];
        assert!(!cs.loading && cs.loaded);
        assert_eq!((cs.error.as_ref(), cs.error_newer.as_ref()), (None, Some(&Problem::Api("panne".into()))));
    }

    /// Une page qui arrive après un READY ou un retour au présent, ou dont le
    /// repère n'est plus le bord de la fenêtre, est ignorée : rien n'est collé
    /// au mauvais endroit.
    #[test]
    fn stale_pages_are_ignored() {
        let page = HISTORY_PAGE as u64;
        let top = 1_000_000;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        history(&mut s, None, top..top + page);
        assert!(s.load_older(CHAN));
        s.take_commands();
        // READY : salon vidé puis rechargé.
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: s.guilds.clone(), dms: Vec::new() });
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
        history(&mut s, None, top + 10..top + 10 + page);
        let before = ids(&s, CHAN);
        // L'ancienne page arrive : ignorée (ni collée, ni comptée comme réponse).
        history(&mut s, Some(top), top - page..top);
        assert_eq!(ids(&s, CHAN), before);
        assert!(s.load_older(CHAN), "la nouvelle fenêtre peut demander sa page");
        s.take_commands();
        history(&mut s, Some(top + 10), top + 10 - page..top + 10);
        assert_eq!(s.chans[&CHAN].msgs.front().map(|m| m.id), Some(top + 10 - page));
        // Page plus ancienne demandée, puis retour au présent avant sa réponse.
        climb(&mut s, 11);
        assert!(s.chans[&CHAN].newer);
        let front = s.chans[&CHAN].msgs.front().map_or(0, |m| m.id);
        s.set_viewing(CHAN, Some((front, front + 3)));
        assert!(s.load_older(CHAN));
        assert!(s.return_to_present(CHAN));
        s.take_commands();
        history(&mut s, None, top + 10..top + 10 + page);
        let present = ids(&s, CHAN);
        history(&mut s, Some(front), front - page..front);
        assert_eq!(ids(&s, CHAN), present, "page d'avant le retour au présent ignorée");
        assert!(!s.chans[&CHAN].loading_older(), "sa réponse termine bien sa demande");
        // Page plus récente dont le repère n'est plus le dernier message.
        climb(&mut s, 11);
        let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
        s.set_viewing(CHAN, Some((back - 3, back)));
        assert!(s.load_newer(CHAN));
        s.take_commands();
        s.apply(Event::MessageDelete { channel_id: CHAN, id: back });
        let now = ids(&s, CHAN);
        s.apply(Event::HistoryAfter { channel_id: CHAN, after: back, messages: (back + 1..back + 1 + page).map(|i| msg(i, CHAN, 2, "x")).collect() });
        assert_eq!(ids(&s, CHAN), now);
        assert!(!s.chans[&CHAN].loading_newer());
    }

    /// Compteur du bouton : ni mes messages, ni les doublons ; un message
    /// supprimé en sort ; une modification d'un message en attente est gardée.
    #[test]
    fn missed_counter_and_live_queue() {
        let page = HISTORY_PAGE as u64;
        let top = 1_000_000;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        history(&mut s, None, top..top + page);
        climb(&mut s, 12);
        let (a, b, c) = (top + page + 1, top + page + 2, top + page + 3);
        s.apply(Event::MessageCreate(msg(a, CHAN, 2, "a")));
        s.apply(Event::MessageCreate(msg(a, CHAN, 2, "a")));
        s.apply(Event::MessageCreate(msg(b, CHAN, ME, "moi")));
        s.apply(Event::MessageCreate(msg(c, CHAN, 3, "c")));
        assert_eq!(s.chans[&CHAN].missed(), 2);
        s.apply(Event::MessageDelete { channel_id: CHAN, id: c });
        assert_eq!(s.chans[&CHAN].missed(), 1);
        s.apply(Event::MessageUpdate { channel_id: CHAN, id: a, content: "a modifié".into() });
        // Retour au présent : la page rendue ne contient pas encore les messages en direct.
        assert!(s.return_to_present(CHAN));
        history(&mut s, None, top..top + page);
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.missed(), 0);
        let last: Vec<(u64, &str, bool)> = cs.msgs.iter().rev().take(2).map(|m| (m.id, m.content.as_str(), m.edited)).collect();
        assert_eq!(last, vec![(b, "moi", false), (a, "a modifié", true)]);
        assert!(!cs.contains(c));
    }

    /// Messages arrivés pendant que la fenêtre est remontée : leur texte
    /// d'origine est gardé ; un changement de langue avant ou après le retour
    /// au présent les refait.
    #[test]
    fn missed_messages_follow_language_changes() {
        let page = HISTORY_PAGE as u64;
        let top = 1_000_000;
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        history(&mut s, None, top..top + page);
        climb(&mut s, 12);
        let (a, b) = (top + page + 1, top + page + 2);
        let mut joined = msg(a, CHAN, 2, "");
        joined.system = Some(SystemKind::Joined);
        s.apply(Event::MessageCreate(joined));
        s.apply(Event::MessageCreate(msg(b, CHAN, 3, "vu <@77>")));
        // Changement pendant l'absence : refait dans la file.
        s.set_language(Lang::En);
        assert!(s.return_to_present(CHAN));
        history(&mut s, None, top..top + page);
        let content = |s: &Store, id: u64| s.chans[&CHAN].msgs.iter().find(|m| m.id == id).map(|m| m.content.clone()).unwrap_or_default();
        assert_eq!(content(&s, a), crate::state::system_msgs::text(Lang::En, SystemKind::Joined, &msg(a, CHAN, 2, ""), Default::default()));
        assert_eq!(content(&s, b), "vu <@77>");
        // Et encore après le rattachement.
        s.set_language(Lang::Fr);
        assert_eq!(content(&s, a), crate::state::system_msgs::text(Lang::Fr, SystemKind::Joined, &msg(a, CHAN, 2, ""), Default::default()));
        assert_eq!(content(&s, b), "vu <@77>");
    }

    /// Seuls les messages système gardent un texte d'origine à refaire :
    /// les balises restent dans la source, mises en mots à l'affichage.
    #[test]
    fn only_language_dependent_messages_are_kept_for_rerendering() {
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        let mut known = msg(10, CHAN, 2, "salut <@77> dans <#100>");
        known.mentions = vec![(77, "Bob".into())];
        let mut joined = msg(16, CHAN, 2, "");
        joined.system = Some(SystemKind::Joined);
        s.apply(Event::History {
            channel_id: CHAN,
            before: None,
            messages: vec![known, msg(11, CHAN, 2, "<@78>"), msg(12, CHAN, 2, "<#555>"), msg(13, CHAN, 2, "<@&5>"), msg(14, CHAN, 2, "<t:1700000000>"), joined],
        });
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.msgs[0].content, "salut <@77> dans <#100>");
        let kept: Vec<u64> = cs.stamped.keys().copied().collect();
        assert_eq!(kept, vec![16]);
        s.apply(Event::MessageUpdate { channel_id: CHAN, id: 14, content: "<#100>".into() });
        assert_eq!(s.chans[&CHAN].stamped.len(), 1);
    }

    /// Page qui vient d'arriver jamais jetée par l'éviction (sinon elle serait
    /// redemandée sans fin) : vue en bas d'une fenêtre pleine, la page plus
    /// ancienne reste et la fenêtre dépasse un peu.
    #[test]
    fn trim_keeps_the_page_that_just_arrived() {
        let page = HISTORY_PAGE as u64;
        let mut s = store();
        history(&mut s, None, 10_000..10_000 + MAX_CACHED_MESSAGES as u64);
        if let Some(c) = s.chans.get_mut(&CHAN) {
            c.more = true;
        }
        let back = s.chans[&CHAN].msgs.back().map_or(0, |m| m.id);
        // Tout est affiché (fenêtre immense) : ni l'un ni l'autre bout n'est libre.
        s.set_viewing(CHAN, Some((10_000, back)));
        assert!(s.load_older(CHAN));
        s.take_commands();
        history(&mut s, Some(10_000), 10_000 - page..10_000);
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.msgs.front().map(|m| m.id), Some(10_000 - page));
        assert_eq!(cs.msgs.len(), MAX_CACHED_MESSAGES + HISTORY_PAGE);
        // Vue au milieu : la page reste, ce sont les plus récents qui partent.
        let mut s = store();
        history(&mut s, None, 10_000..10_000 + MAX_CACHED_MESSAGES as u64);
        if let Some(c) = s.chans.get_mut(&CHAN) {
            c.more = true;
        }
        s.set_viewing(CHAN, Some((10_200, 10_203)));
        assert!(s.load_older(CHAN));
        s.take_commands();
        history(&mut s, Some(10_000), 10_000 - page..10_000);
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.msgs.front().map(|m| m.id), Some(10_000 - page));
        assert_eq!(cs.msgs.len(), MAX_CACHED_MESSAGES);
        assert!(cs.newer && !cs.contains(back));
    }

    /// Ligne « Nouveaux messages » : premier message arrivé pendant
    /// l'absence, posée à l'ouverture, oubliée en quittant le salon.
    #[test]
    fn new_marker_is_set_on_open_and_cleared_on_leave() {
        let mut s = store();
        history(&mut s, None, 1..4);
        s.apply(Event::MessageCreate(msg(10, CHAN, 2, "a")));
        s.apply(Event::MessageCreate(msg(11, CHAN, 2, "b")));
        // Mes propres messages ne comptent pas.
        s.apply(Event::MessageCreate(msg(12, CHAN, ME, "moi")));
        s.select_channel(CHAN);
        assert_eq!(s.chans[&CHAN].new_marker, Some(10));
        // Rester dans le salon ne la change pas ; un message lu ici n'en pose pas.
        s.select_channel(CHAN);
        s.apply(Event::MessageCreate(msg(13, CHAN, 2, "c")));
        assert_eq!(s.chans[&CHAN].new_marker, Some(10));
        s.channel = None;
        s.select_channel(CHAN);
        assert_eq!(s.chans[&CHAN].new_marker, None);
        assert!(s.unread_from.is_empty());
    }

    /// Chaque modification du cache change sa version (hauteurs à revoir).
    #[test]
    fn rev_changes_with_cache() {
        let mut s = store();
        history(&mut s, None, 1..4);
        let mut last = s.chans[&CHAN].rev;
        let mut step = |s: &mut Store, e: Event| {
            s.apply(e);
            let r = s.chans[&CHAN].rev;
            assert!(r > last);
            last = r;
        };
        step(&mut s, Event::MessageCreate(msg(5, CHAN, 2, "x")));
        step(&mut s, Event::MessageUpdate { channel_id: CHAN, id: 2, content: "y".into() });
        step(&mut s, Event::MessageDelete { channel_id: CHAN, id: 2 });
        // Arrivée légèrement désordonnée : insérée à sa place.
        step(&mut s, Event::MessageCreate(msg(4, CHAN, 2, "x")));
        assert_eq!(ids(&s, CHAN), vec![1, 3, 4, 5]);
    }

    #[test]
    fn message_create_appends_dedups_and_marks_unread() {
        let mut s = store();
        history(&mut s, None, 1..3);
        let mut m = msg(5, CHAN, 2, "<#100>");
        m.mentions = vec![(ME, "moi".into())];
        s.apply(Event::MessageCreate(m.clone()));
        s.apply(Event::MessageCreate(m));
        assert_eq!(ids(&s, CHAN), vec![1, 2, 5]);
        assert_eq!(s.chans[&CHAN].msgs[2].content, "<#100>");
        assert!(s.unread.contains(&CHAN));
        assert!(s.mentioned.contains(&CHAN));
    }

    #[test]
    fn message_create_own_or_selected_is_not_unread() {
        let mut s = store();
        s.apply(Event::MessageCreate(msg(5, CHAN, ME, "moi")));
        assert!(s.unread.is_empty());
        s.select_channel(CHAN);
        s.apply(Event::MessageCreate(msg(6, CHAN, 2, "autre")));
        assert!(s.unread.is_empty() && s.mentioned.is_empty());
    }

    #[test]
    fn message_create_ignored_when_history_not_loaded() {
        let mut s = store();
        s.apply(Event::MessageCreate(msg(5, CHAN, 2, "x")));
        assert!(!s.chans.contains_key(&CHAN));
    }

    #[test]
    fn message_create_caps_cache_at_500() {
        let mut s = store();
        history(&mut s, None, 1..4);
        assert!(!s.chans[&CHAN].more);
        for id in 4..=(MAX_CACHED_MESSAGES as u64) {
            s.apply(Event::MessageCreate(msg(id, CHAN, 2, "x")));
        }
        assert_eq!(s.chans[&CHAN].msgs.len(), MAX_CACHED_MESSAGES);
        assert!(!s.chans[&CHAN].more);

        s.apply(Event::MessageCreate(msg(1000, CHAN, 2, "x")));
        s.apply(Event::MessageCreate(msg(1001, CHAN, 2, "x")));
        let cs = &s.chans[&CHAN];
        assert_eq!(cs.msgs.len(), MAX_CACHED_MESSAGES);
        assert!(cs.more);
        assert_eq!(cs.msgs.front().map(|m| m.id), Some(3));
        assert_eq!(cs.msgs.back().map(|m| m.id), Some(1001));
    }

    #[test]
    fn message_create_dm_moves_conversation_to_top() {
        let mut s = store();
        s.dms = vec![
            Dm::new(50, "a".into(), 50),
            Dm::new(51, "b".into(), 51),
        ];
        let mut m = msg(900, 51, 2, "coucou");
        m.guild_id = None;
        s.apply(Event::MessageCreate(m));
        assert_eq!(s.dms[0].id, 51);
        assert_eq!(s.dms[0].last_message, 900);
        // Un MP compte toujours comme mention.
        assert!(s.mentioned.contains(&51));
    }

    #[test]
    fn message_delete_removes_only_target() {
        let mut s = store();
        history(&mut s, None, 1..4);
        s.apply(Event::MessageDelete { channel_id: CHAN, id: 2 });
        s.apply(Event::MessageDelete { channel_id: CHAN, id: 99 });
        s.apply(Event::MessageDelete { channel_id: 12345, id: 1 });
        assert_eq!(ids(&s, CHAN), vec![1, 3]);
    }

    #[test]
    fn message_update_rerenders_and_marks_edited() {
        let mut s = store();
        history(&mut s, None, 1..3);
        s.apply(Event::MessageUpdate { channel_id: CHAN, id: 2, content: "<:ok:1>".into() });
        let m = &s.chans[&CHAN].msgs[1];
        assert_eq!(m.content, "<:ok:1>");
        assert!(m.edited);
    }

    #[test]
    fn error_and_fatal_become_effects() {
        let mut s = store();
        assert_eq!(s.apply(Event::Error(Problem::Api("oups".into()))), Some(Effect::Toast(Notice::Error(Problem::Api("oups".into())))));
        assert_eq!(
            s.apply(Event::Fatal { problem: Problem::BadToken, bad_token: true }),
            Some(Effect::Fatal { problem: Problem::BadToken, bad_token: true })
        );
    }

    /// `/tts` reçu (F27) : gardé pour la lecture seulement dans le salon affiché,
    /// quand le réglage du compte est connu et le demande.
    #[test]
    fn received_tts_is_queued_only_in_the_open_channel_when_the_setting_asks() {
        let mut s = store();
        s.channel = Some(CHAN);
        let tts = |id, channel, author| {
            let mut m = msg(id, channel, author, "bonjour");
            m.meta.tts = true;
            m
        };
        s.apply(Event::MessageCreate(tts(50, CHAN, 2)));
        assert!(s.take_tts().is_empty(), "réglages du compte inconnus");
        s.account.apply(crate::state::usersettings::AccountEvent::Proto { bytes: Some(crate::state::usersettings::proto::Msg::default().encode()), partial: false });
        s.apply(Event::MessageCreate(tts(51, CHAN, 2)));
        s.apply(Event::MessageCreate(tts(52, CHAN, ME)));
        s.apply(Event::MessageCreate(tts(53, CHAN + 1, 2)));
        s.apply(Event::MessageCreate(msg(54, CHAN, 2, "sans tts")));
        assert_eq!(s.take_tts(), ["u2 : bonjour".to_string()]);
        assert!(s.take_tts().is_empty());
    }

    #[test]
    fn ready_keeps_existing_channel_and_reloads_history() {
        let mut s = store();
        s.guild = Some(9);
        s.select_channel(CHAN);
        s.take_commands();
        history(&mut s, None, 1..3);
        let guilds = s.guilds.clone();
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds, dms: Vec::new() });
        assert_eq!(s.channel, Some(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);

        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: Vec::new(), dms: Vec::new() });
        assert_eq!(s.channel, None);
        assert_eq!(s.guild, None);
    }

    #[test]
    fn select_guild_opens_first_text_channel() {
        let mut s = store();
        s.guilds[0].channels.insert(0, Channel { id: 7, name: "cat".into(), kind: ChannelKind::Category, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 });
        assert!(s.select_guild(9));
        assert_eq!(s.channel, Some(CHAN));
        assert!(!s.select_guild(9));
    }

    #[test]
    fn eviction_keeps_at_most_max_channels() {
        let mut s = store();
        for id in 0..(MAX_CACHED_CHANNELS as u64 + 5) {
            history_for(&mut s, 1000 + id);
            s.select_channel(1000 + id);
        }
        assert!(s.chans.len() <= MAX_CACHED_CHANNELS);
        assert!(s.chans.contains_key(&s.channel.unwrap_or_default()));
    }

    fn history_for(s: &mut Store, ch: u64) {
        s.apply(Event::History { channel_id: ch, before: None, messages: vec![msg(1, ch, 2, "x")] });
    }

    // ------------------------------------------------------------ compléments

    fn chan(id: u64, kind: ChannelKind, position: i32, parent: Option<u64>) -> Channel {
        Channel { id, name: format!("c{id}"), kind, position, parent, overwrites: Some(Vec::new()), slowmode: 0 }
    }

    fn guild(id: u64) -> Guild {
        Guild::new(id, format!("g{id}"), Vec::new())
    }

    #[test]
    fn guild_delete_resets_selection_only_for_current() {
        let mut s = store();
        s.guilds.push(guild(8));
        s.guild = Some(9);
        s.channel = Some(CHAN);
        s.apply(Event::GuildDelete(8));
        assert_eq!((s.guild, s.channel), (Some(9), Some(CHAN)));
        assert_eq!(s.guilds.len(), 1);
        s.apply(Event::GuildDelete(9));
        assert_eq!((s.guild, s.channel), (None, None));
        assert!(s.guilds.is_empty());
    }

    #[test]
    fn channel_delete_guild_channel() {
        let mut s = store();
        s.guild = Some(9);
        s.select_channel(CHAN);
        history(&mut s, None, 1..3);
        s.apply(Event::ChannelDelete { guild_id: Some(9), id: CHAN });
        assert!(s.guilds[0].channels.is_empty());
        assert!(!s.chans.contains_key(&CHAN));
        assert_eq!(s.channel, None);
        assert_eq!(s.guild, Some(9));
    }

    #[test]
    fn channel_delete_other_keeps_selection_and_dm_removed_without_guild() {
        let mut s = store();
        s.dms = vec![Dm::new(50, "a".into(), 50), Dm::new(51, "b".into(), 51)];
        s.channel = Some(CHAN);
        s.apply(Event::ChannelDelete { guild_id: None, id: 50 });
        assert_eq!(s.dms.iter().map(|d| d.id).collect::<Vec<_>>(), vec![51]);
        assert_eq!(s.channel, Some(CHAN));
        assert_eq!(s.guilds[0].channels.len(), 1);
    }

    #[test]
    fn dm_upsert_renames_in_place_or_inserts_on_top() {
        let mut s = store();
        s.dms = vec![Dm::new(50, "a".into(), 50), Dm::new(51, "b".into(), 51)];
        s.apply(Event::DmUpsert { dm: Dm::new(51, "nouveau".into(), 999), known: DmKnown::ALL });
        let d: Vec<(u64, &str, u64)> = s.dms.iter().map(|d| (d.id, d.name.as_str(), d.last_message)).collect();
        // Seul le nom change : ni la position ni le dernier message.
        assert_eq!(d, vec![(50, "a", 50), (51, "nouveau", 51)]);
        s.apply(Event::DmUpsert { dm: Dm::new(52, "c".into(), 52), known: DmKnown::default() });
        assert_eq!(s.dms[0].id, 52);
        assert_eq!(s.dms.len(), 3);
    }

    /// Icône de serveur et avatar de MP suivent les mises à jour.
    #[test]
    fn guild_icon_and_dm_avatar_updates() {
        use crate::state::model::{Avatar, ImageHash, ImageRef};
        let hash = ImageHash::synthetic(1);
        let icon = ImageRef::GuildIcon { guild_id: 9, hash };
        let mut s = store();
        s.apply(Event::GuildUpdate { id: 9, owner_id: None, icon: Some(Some(icon)) });
        assert_eq!(s.guilds[0].icon, Some(icon));
        s.apply(Event::GuildUpdate { id: 9, owner_id: None, icon: None });
        assert_eq!(s.guilds[0].icon, Some(icon), "absente du paquet : inchangée");
        s.apply(Event::GuildUpdate { id: 9, owner_id: None, icon: Some(None) });
        assert_eq!(s.guilds[0].icon, None);

        s.dms = vec![Dm::new(50, "a".into(), 50)];
        let avatar = Avatar::of_user(7, Some(hash), None);
        s.apply(Event::DmUpsert { dm: Dm { avatar, ..Dm::new(50, "a".into(), 50) }, known: DmKnown::ALL });
        assert_eq!(s.dms[0].avatar, avatar);
        // Paquet partiel (ni destinataires ni icône) : avatar et nom connus gardés.
        s.apply(Event::DmUpsert { dm: Dm::new(50, "Conversation".into(), 50), known: DmKnown::default() });
        assert_eq!((s.dms[0].name.as_str(), s.dms[0].avatar), ("a", avatar));
        // Seule l'icône fournie (groupe) : l'avatar change, le nom reste.
        let other = Avatar::default_for(50);
        s.apply(Event::DmUpsert { dm: Dm { avatar: other, ..Dm::new(50, "x".into(), 50) }, known: DmKnown { name: false, avatar: true } });
        assert_eq!((s.dms[0].name.as_str(), s.dms[0].avatar), ("a", other));
    }

    #[test]
    fn channel_upsert_replaces_and_sorts() {
        let mut s = store();
        s.apply(Event::ChannelUpsert { guild_id: 9, channel: chan(5, ChannelKind::Category, 0, None) });
        s.apply(Event::ChannelUpsert { guild_id: 9, channel: chan(6, ChannelKind::Text, 0, Some(5)) });
        let ids = |s: &Store| s.guilds[0].channels.iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(&s), vec![CHAN, 5, 6]);
        // Le salon passe dans la catégorie : remplacé (pas dupliqué) et retrié.
        let mut moved = chan(CHAN, ChannelKind::Text, 1, Some(5));
        moved.name = "renommé".into();
        s.apply(Event::ChannelUpsert { guild_id: 9, channel: moved });
        assert_eq!(ids(&s), vec![5, 6, CHAN]);
        assert_eq!(s.channel_name(CHAN), Some("renommé"));
        // Serveur inconnu : ignoré.
        s.apply(Event::ChannelUpsert { guild_id: 777, channel: chan(1, ChannelKind::Text, 0, None) });
        assert_eq!(s.guilds.len(), 1);
        assert_eq!(ids(&s).len(), 3);
    }

    #[test]
    fn guild_create_replaces_or_appends() {
        let mut s = store();
        let mut g = guild(9);
        g.name = "remplacé".into();
        s.apply(Event::GuildCreate(g));
        assert_eq!(s.guilds.len(), 1);
        assert_eq!(s.guilds[0].name, "remplacé");
        assert!(s.guilds[0].channels.is_empty());
        s.apply(Event::GuildCreate(guild(10)));
        assert_eq!(s.guilds.iter().map(|g| g.id).collect::<Vec<_>>(), vec![9, 10]);
    }

    #[test]
    fn history_error_sets_error_and_stops_loading() {
        let mut s = store();
        s.select_channel(CHAN);
        assert!(s.chans[&CHAN].loading);
        s.apply(Event::HistoryError { channel_id: CHAN, before: None, after: None, error: Problem::Api("refusé".into()) });
        let cs = &s.chans[&CHAN];
        assert!(!cs.loading && !cs.loaded);
        assert_eq!(cs.error.as_ref(), Some(&Problem::Api("refusé".into())));
        // Une page reçue ensuite efface l'erreur.
        history(&mut s, None, 1..2);
        assert_eq!(s.chans[&CHAN].error, None);
    }

    /// Aperçu de lien ajouté par Discord : embeds remplacés, pièces jointes
    /// gardées (absentes du paquet), message pas marqué modifié, vue redessinée.
    #[test]
    fn message_media_replaces_embeds_only() {
        use crate::state::model::{Attachment, Embed};
        let mut s = store();
        history(&mut s, None, 1..3);
        let att = vec![Attachment::file("a.txt", "u")];
        s.apply(Event::MessageMedia { channel_id: CHAN, id: 2, attachments: Some(att.clone()), embeds: None });
        let rev = s.chans[&CHAN].rev;
        s.apply(Event::MessageMedia { channel_id: CHAN, id: 2, attachments: None, embeds: Some(vec![Embed::text(Some("T"), None)]) });
        let m = &s.chans[&CHAN].msgs[1];
        assert_eq!(m.attachments, att);
        assert_eq!(m.embeds.len(), 1);
        assert!(!m.edited);
        assert!(s.chans[&CHAN].rev > rev);
        // Message inconnu : rien ne change.
        let rev = s.chans[&CHAN].rev;
        s.apply(Event::MessageMedia { channel_id: CHAN, id: 99, attachments: Some(Vec::new()), embeds: None });
        assert_eq!(s.chans[&CHAN].rev, rev);
    }

    #[test]
    fn message_update_absent_is_ignored() {
        let mut s = store();
        history(&mut s, None, 1..3);
        s.apply(Event::MessageUpdate { channel_id: CHAN, id: 99, content: "z".into() });
        s.apply(Event::MessageUpdate { channel_id: 4242, id: 1, content: "z".into() });
        assert!(s.chans[&CHAN].msgs.iter().all(|m| m.content == "x" && !m.edited));
        assert!(!s.chans.contains_key(&4242));
    }

    #[test]
    fn mentions_everyone_direct_or_none() {
        let mut s = store();
        s.apply(Event::MessageCreate(msg(1, 200, 2, "rien")));
        assert!(s.unread.contains(&200) && !s.mentioned.contains(&200));

        let mut m = msg(2, 201, 2, "@everyone");
        m.mention_everyone = true;
        s.apply(Event::MessageCreate(m));
        assert!(s.mentioned.contains(&201));

        let mut m = msg(3, 202, 2, "<@1>");
        m.mentions = vec![(ME, "moi".into())];
        s.apply(Event::MessageCreate(m));
        assert!(s.mentioned.contains(&202));

        let mut m = msg(4, 203, 2, "<@5>");
        m.mentions = vec![(5, "autre".into())];
        s.apply(Event::MessageCreate(m));
        assert!(s.unread.contains(&203) && !s.mentioned.contains(&203));

        // Ses propres messages, même en MP, ne comptent pas.
        let mut m = msg(5, 204, ME, "moi");
        m.guild_id = None;
        s.apply(Event::MessageCreate(m));
        assert!(!s.unread.contains(&204) && !s.mentioned.contains(&204));

        // Ouvrir le salon efface non-lu et mention.
        s.select_channel(202);
        assert!(!s.unread.contains(&202) && !s.mentioned.contains(&202));
    }

    #[test]
    fn ensure_history_only_once() {
        let mut s = store();
        s.ensure_history(CHAN);
        s.ensure_history(CHAN); // en cours de chargement
        assert_eq!(s.take_commands().len(), 1);
        history(&mut s, None, 1..2);
        s.ensure_history(CHAN); // déjà chargé
        s.select_channel(CHAN);
        assert!(s.take_commands().is_empty());
    }

    #[test]
    fn clear_empties_command_queue_and_state() {
        let mut s = store();
        s.guild = Some(9);
        s.select_channel(CHAN);
        s.push_command(Cmd::Send { channel_id: CHAN, content: "x".into(), nonce: 1 });
        s.status = Status::Connected;
        s.clear();
        assert!(s.take_commands().is_empty());
        assert!(s.me.is_none() && s.guilds.is_empty() && s.chans.is_empty());
        assert_eq!((s.guild, s.channel), (None, None));
        assert_eq!(s.status, Status::Connected);
    }

    #[test]
    fn eviction_drops_oldest_and_spares_current_and_loading() {
        let mut s = store();
        // Horodatages dans le futur : tous plus récents que l'Instant::now() que
        // select_channel donne au salon courant, qui serait donc le premier
        // évincé s'il n'était pas épargné.
        let base = Instant::now() + std::time::Duration::from_secs(1000);
        let n = MAX_CACHED_CHANNELS as u64 + 2;
        for id in 1..=n {
            let cs = s.chans.entry(id).or_default();
            cs.loaded = true;
            cs.last_view = Some(base + std::time::Duration::from_secs(id));
        }
        // Le plus ancien est en cours de chargement : épargné.
        if let Some(cs) = s.chans.get_mut(&1) {
            cs.loaded = false;
            cs.loading = true;
        }
        s.select_channel(2);
        assert!(s.take_commands().is_empty());
        assert_eq!(s.chans.len(), MAX_CACHED_CHANNELS);
        assert!(s.chans.contains_key(&1) && s.chans.contains_key(&2));
        assert!(!s.chans.contains_key(&3) && !s.chans.contains_key(&4));
        assert!((5..=n).all(|id| s.chans.contains_key(&id)));
    }
    // ------------------------------------------------------------ permissions

    mod permissions {
        use super::*;
        use crate::state::model::{Member, Overwrite, OverwriteKind, Role};
        use crate::state::perms::{Permissions as P, Verdict};

        const G: u64 = 9;
        const CAT: u64 = 50;
        const SECRET: u64 = 51;
        const READONLY: u64 = 52;
        const NOHIST: u64 = 53;
        const EMPTY_CAT: u64 = 60;
        const DENIED_CAT: u64 = 61;
        const OPEN_IN_DENIED: u64 = 62;
        const R_MOD: u64 = 700;
        const BASE: P = P(P::VIEW_CHANNEL.0 | P::SEND_MESSAGES.0 | P::READ_MESSAGE_HISTORY.0);

        fn deny(id: u64, p: P) -> Overwrite {
            Overwrite { id, kind: OverwriteKind::Role, allow: P::NONE, deny: p }
        }

        fn ch(id: u64, kind: ChannelKind, parent: Option<u64>, ows: Vec<Overwrite>) -> Channel {
            Channel { id, name: format!("c{id}"), kind, position: id as i32, parent, overwrites: Some(ows), slowmode: 0 }
        }

        /// Serveur aux permissions connues : une catégorie secrète et son
        /// salon, un salon libre, un salon en lecture seule, un salon sans
        /// historique, une catégorie vide et une catégorie refusée contenant
        /// un salon ouvert.
        fn known() -> Store {
            let mut s = store();
            let g = &mut s.guilds[0];
            g.roles = vec![
                Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: BASE, mentionable: false },
                Role { id: R_MOD, name: "Mod".into(), color: 0, position: 1, permissions: P::NONE, mentionable: false },
            ];
            g.owner_id = Some(2);
            g.me = Some(Member::default());
            g.channels = vec![
                ch(CAT, ChannelKind::Category, None, vec![deny(G, P::VIEW_CHANNEL)]),
                ch(SECRET, ChannelKind::Text, Some(CAT), vec![deny(G, P::VIEW_CHANNEL), Overwrite { id: R_MOD, kind: OverwriteKind::Role, allow: P::VIEW_CHANNEL, deny: P::NONE }]),
                ch(CHAN, ChannelKind::Text, None, Vec::new()),
                ch(READONLY, ChannelKind::Announcement, None, vec![deny(G, P::SEND_MESSAGES)]),
                ch(NOHIST, ChannelKind::Text, None, vec![deny(G, P::READ_MESSAGE_HISTORY)]),
                ch(EMPTY_CAT, ChannelKind::Category, None, Vec::new()),
                ch(DENIED_CAT, ChannelKind::Category, None, vec![deny(G, P::VIEW_CHANNEL)]),
                ch(OPEN_IN_DENIED, ChannelKind::Text, Some(DENIED_CAT), Vec::new()),
            ];
            s.refresh_permissions();
            s
        }

        fn set_me(s: &mut Store, m: Member) -> Option<Effect> {
            s.apply(Event::MemberUpdate { guild_id: G, user_id: ME, member: Some(m) })
        }

        #[test]
        fn hidden_channels_and_categories() {
            let s = known();
            assert!(s.is_hidden(SECRET) && s.is_hidden(CAT));
            // Catégorie vide : seule sa propre permission compte.
            assert!(!s.is_hidden(EMPTY_CAT));
            // Catégorie refusée mais avec un salon visible : affichée.
            assert!(!s.is_hidden(DENIED_CAT) && !s.is_hidden(OPEN_IN_DENIED));
            assert!(!s.is_hidden(CHAN) && !s.is_hidden(READONLY) && !s.is_hidden(NOHIST));
            assert_eq!(s.can(READONLY, P::SEND_MESSAGES), Verdict::Refuse);
            assert_eq!(s.send_block(READONLY), Some(SendBlock::NoPermission));
            assert_eq!(s.can(CHAN, P::SEND_MESSAGES), Verdict::Permis);
            assert_eq!(s.can(424242, P::SEND_MESSAGES), Verdict::Inconnu);
        }

        #[test]
        fn unknown_member_changes_nothing() {
            let mut s = known();
            s.guilds[0].me = None;
            s.refresh_permissions();
            assert!(!s.is_hidden(SECRET) && !s.is_hidden(CAT));
            assert_eq!(s.can(READONLY, P::SEND_MESSAGES), Verdict::Inconnu);
            assert!(s.send_message(READONLY, "x".into()));
            s.select_channel(NOHIST);
            assert_eq!(no_nonce(s.take_commands()), vec![Cmd::Send { channel_id: READONLY, content: "x".into(), nonce: 0 }, Cmd::History { channel_id: NOHIST, before: None }]);
            // Sans moi (READY pas encore reçu), rien n'est calculé non plus.
            let mut s = known();
            s.me = None;
            s.refresh_permissions();
            assert_eq!(s.can(CHAN, P::VIEW_CHANNEL), Verdict::Inconnu);
        }

        #[test]
        fn send_refused_without_permission() {
            let mut s = known();
            assert!(!s.send_message(READONLY, "non".into()));
            assert!(s.send_message(CHAN, "oui".into()));
            assert_eq!(no_nonce(s.take_commands()), vec![Cmd::Send { channel_id: CHAN, content: "oui".into(), nonce: 0 }]);
        }

        #[test]
        fn history_denied_then_granted() {
            let mut s = known();
            s.select_channel(NOHIST);
            assert!(s.take_commands().is_empty());
            assert!(s.chans[&NOHIST].history_denied);
            // Les nouveaux messages s'affichent.
            s.apply(Event::MessageCreate(msg(5, NOHIST, 2, "direct")));
            assert_eq!(ids(&s, NOHIST), vec![5]);
            // Permission rendue (dérogation retirée) : l'historique est demandé.
            s.apply(Event::ChannelUpsert { guild_id: G, channel: ch(NOHIST, ChannelKind::Text, None, Vec::new()) });
            assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: NOHIST, before: None }]);
            assert!(!s.chans[&NOHIST].history_denied && s.chans[&NOHIST].loading);
        }

        /// Sans READ_MESSAGE_HISTORY : aucune page demandée, dans aucun sens,
        /// même après 600 messages reçus en direct avec la vue tout en haut
        /// (la fenêtre ne quitte jamais le présent : rien ne pourrait la recharger).
        #[test]
        fn no_history_request_in_any_direction_without_permission() {
            let mut s = known();
            s.select_channel(NOHIST);
            assert!(s.take_commands().is_empty());
            for i in 0..600u64 {
                s.apply(Event::MessageCreate(msg(10 + i, NOHIST, 2, "x")));
                let front = s.chans[&NOHIST].msgs.front().map_or(0, |m| m.id);
                s.set_viewing(NOHIST, Some((front, front + 3)));
                assert!(!s.load_older(NOHIST) && !s.load_newer(NOHIST) && !s.return_to_present(NOHIST), "message {i}");
                if i % 100 == 0 {
                    assert!(s.send_message(NOHIST, "x".into()));
                }
            }
            let cmds = s.take_commands();
            assert!(cmds.iter().all(|c| matches!(c, Cmd::Send { .. })), "{cmds:?}");
            let cs = &s.chans[&NOHIST];
            assert!(!cs.newer && !cs.more && cs.missed() == 0);
            assert_eq!(cs.msgs.len(), MAX_CACHED_MESSAGES);
            assert_eq!(cs.msgs.back().map(|m| m.id), Some(609));
        }

        #[test]
        fn hidden_unread_does_not_count_and_first_visible_is_opened() {
            let mut s = known();
            let mut m = msg(1, SECRET, 2, "<@1>");
            m.mentions = vec![(ME, "moi".into())];
            s.apply(Event::MessageCreate(m));
            assert_eq!(s.guild_has_unread(&s.guilds[0]), (false, false));
            // Le premier salon (SECRET) est masqué : c'est le suivant qui s'ouvre.
            assert!(s.select_guild(G));
            assert_eq!(s.channel, Some(CHAN));
            // Rôle accordé : le salon et ses marques réapparaissent.
            set_me(&mut s, Member { roles: vec![R_MOD], ..Default::default() });
            assert!(!s.is_hidden(SECRET) && !s.is_hidden(CAT));
            assert_eq!(s.guild_has_unread(&s.guilds[0]), (true, true));
        }

        #[test]
        fn open_channel_lost_is_left() {
            let mut s = known();
            set_me(&mut s, Member { roles: vec![R_MOD], ..Default::default() });
            s.guild = Some(G);
            s.select_channel(SECRET);
            let eff = set_me(&mut s, Member::default());
            assert_eq!(eff, Some(Effect::Toast(Notice::LostAccess)));
            assert_eq!((s.guild, s.channel), (Some(G), None));
            // Rôle supprimé : retiré de mon membre aussi, permissions toujours connues.
            set_me(&mut s, Member { roles: vec![R_MOD], ..Default::default() });
            assert!(!s.is_hidden(SECRET));
            s.apply(Event::RoleDelete { guild_id: G, role_id: R_MOD });
            assert!(s.is_hidden(SECRET));
            assert_eq!(s.guilds[0].me.as_ref().map(|m| m.roles.len()), Some(0));
        }

        #[test]
        fn unreadable_role_makes_holders_unknown() {
            let mut s = known();
            set_me(&mut s, Member { roles: vec![R_MOD], ..Default::default() });
            s.apply(Event::RoleUnreadable { guild_id: G, role_id: R_MOD });
            assert_eq!(s.can(CHAN, P::VIEW_CHANNEL), Verdict::Inconnu);
            assert!(!s.is_hidden(CAT));
            // Un rôle illisible que je n'ai pas ne change rien pour moi.
            let mut s = known();
            s.apply(Event::RoleUnreadable { guild_id: G, role_id: R_MOD });
            assert!(s.is_hidden(SECRET));
        }

        #[test]
        fn other_members_and_unknown_guilds_are_ignored() {
            let mut s = known();
            s.apply(Event::MemberUpdate { guild_id: G, user_id: 2, member: Some(Member { roles: vec![R_MOD], ..Default::default() }) });
            assert!(s.is_hidden(SECRET));
            s.apply(Event::MemberUpdate { guild_id: 777, user_id: ME, member: Some(Member::default()) });
            s.apply(Event::RoleUpsert { guild_id: 777, role: Role { id: 1, name: String::new(), color: 0, position: 0, permissions: P::ALL, mentionable: false } });
            s.apply(Event::GuildUpdate { id: 777, owner_id: Some(Some(ME)), icon: None });
            assert_eq!(s.guilds.len(), 1);
            // GUILD_UPDATE sans propriétaire : rien ne change.
            s.apply(Event::GuildUpdate { id: G, owner_id: None, icon: None });
            assert_eq!(s.guilds[0].owner_id, Some(2));
            // Rôle mis à jour : @everyone perd l'envoi.
            s.apply(Event::RoleUpsert { guild_id: G, role: Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: P::VIEW_CHANNEL, mentionable: false } });
            assert_eq!(s.send_block(CHAN), Some(SendBlock::NoPermission));
        }

        #[test]
        fn timeout_blocks_until_it_ends() {
            let mut s = known();
            let until = now_ms() + 3_600_000;
            set_me(&mut s, Member { timeout_until: Some(until), ..Default::default() });
            assert_eq!(s.send_block(CHAN), Some(SendBlock::Timeout { until_ms: until }));
            assert_eq!(s.can(CHAN, P::READ_MESSAGE_HISTORY), Verdict::Permis);
            assert_eq!(s.next_timeout_end(), Some(until));
            // Avant la fin : rien n'est recalculé.
            assert_eq!(s.expire_timeouts(until - 1), None);
            assert_eq!(s.send_block(CHAN), Some(SendBlock::Timeout { until_ms: until }));
            // Fin atteinte (simulée en avançant la date de fin dans le passé,
            // car le calcul lit l'horloge réelle).
            if let Some(m) = s.guilds[0].me.as_mut() {
                m.timeout_until = Some(now_ms() - 1);
            }
            assert_eq!(s.expire_timeouts(until), None);
            assert_eq!(s.send_block(CHAN), None);
            assert_eq!(s.next_timeout_end(), None);
        }

        #[test]
        fn guild_delete_and_clear_forget_permissions() {
            let mut s = known();
            s.apply(Event::GuildDelete(G));
            assert!(!s.is_hidden(SECRET));
            assert_eq!(s.can(CHAN, P::VIEW_CHANNEL), Verdict::Inconnu);
            let mut s = known();
            s.clear();
            assert!(!s.is_hidden(SECRET) && s.next_timeout_end().is_none());
        }

        #[test]
        fn ready_requests_history_once_when_it_becomes_readable() {
            let mut s = known();
            s.guild = Some(G);
            s.select_channel(NOHIST);
            assert!(s.take_commands().is_empty());
            let mut guilds = s.guilds.clone();
            if let Some(c) = guilds[0].channels.iter_mut().find(|c| c.id == NOHIST) {
                c.overwrites = Some(Vec::new());
            }
            s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds, dms: Vec::new() });
            assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: NOHIST, before: None }]);
        }

        #[test]
        fn ready_drops_hidden_selection() {
            let mut s = known();
            set_me(&mut s, Member { roles: vec![R_MOD], ..Default::default() });
            s.guild = Some(G);
            s.select_channel(SECRET);
            s.take_commands();
            let mut guilds = s.guilds.clone();
            guilds[0].me = Some(Member::default());
            let eff = s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds, dms: Vec::new() });
            // Même notification que pour une perte d'accès en cours de session.
            assert_eq!(eff, Some(Effect::Toast(Notice::LostAccess)));
            assert_eq!((s.guild, s.channel), (Some(G), None));
            assert!(s.take_commands().is_empty());
        }

        /// Index recalculé à partir de zéro : salons non lus et mentionnés
        /// visibles, par serveur.
        fn exhaustive(s: &Store) -> HashMap<u64, (u32, u32)> {
            let mut out: HashMap<u64, (u32, u32)> = HashMap::new();
            for g in &s.guilds {
                for c in g.channels.iter().filter(|c| !s.is_hidden(c.id)) {
                    let (u, m) = (s.unread.contains(&c.id), s.mentioned.contains(&c.id));
                    if u || m {
                        let e = out.entry(g.id).or_default();
                        e.0 += u32::from(u);
                        e.1 += u32::from(m);
                    }
                }
            }
            out
        }

        #[test]
        fn unread_index_stays_consistent() {
            let mut s = known();
            let check = |s: &Store, step: &str| {
                assert_eq!(s.unread_index, exhaustive(s), "{step}");
                assert_eq!(s.guild_has_unread(&s.guilds[0]), exhaustive(s).get(&G).map_or((false, false), |(u, m)| (*u > 0, *m > 0)), "{step}");
            };
            let ping = |id, ch| {
                let mut m = msg(id, ch, 2, "<@1>");
                m.mentions = vec![(ME, "moi".into())];
                m
            };
            s.apply(Event::MessageCreate(msg(1, CHAN, 2, "a")));
            s.apply(Event::MessageCreate(msg(2, CHAN, 2, "b")));
            check(&s, "ajout");
            s.apply(Event::MessageCreate(ping(3, SECRET)));
            s.apply(Event::MessageCreate(ping(4, NOHIST)));
            check(&s, "ajout masqué");
            set_me(&mut s, Member { roles: vec![R_MOD], ..Default::default() });
            check(&s, "de nouveau visible");
            assert_eq!(s.unread_index.get(&G), Some(&(3, 2)));
            set_me(&mut s, Member::default());
            check(&s, "masqué");
            s.guild = Some(G);
            s.select_channel(CHAN);
            check(&s, "retrait");
            s.apply(Event::ChannelDelete { guild_id: Some(G), id: NOHIST });
            check(&s, "salon supprimé");
            assert!(!s.unread().contains(&NOHIST));
            let mut guilds = s.guilds.clone();
            guilds[0].me = Some(Member { roles: vec![R_MOD], ..Default::default() });
            s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds, dms: Vec::new() });
            check(&s, "READY");
            assert_eq!(s.unread_index.get(&G), Some(&(1, 1)));
            s.apply(Event::GuildDelete(G));
            assert_eq!(s.unread_index, exhaustive(&s));
            assert!(s.unread_index.is_empty());
        }

        #[test]
        fn server_clock_ends_timeout_early() {
            let mut s = known();
            let local = now_ms();
            let until = local + 30_000;
            set_me(&mut s, Member { timeout_until: Some(until), ..Default::default() });
            assert!(s.send_block(CHAN).is_some());
            // Variation de moins de 2 s ou écart de plus d'un jour : ignorés.
            assert_eq!(s.set_server_time(local + 1_000, local), None);
            assert_eq!(s.set_server_time(local + 2 * 86_400_000, local), None);
            assert_eq!(s.clock_offset, 0);
            assert!(s.send_block(CHAN).is_some());
            // Discord est une minute en avance sur nous : l'exclusion est finie.
            s.set_server_time(local + 60_000, local);
            assert_eq!(s.clock_offset, 60_000);
            assert_eq!(s.send_block(CHAN), None);
            assert!(s.now() >= local + 60_000);
            assert_eq!(s.next_timeout_end(), None);
        }

        /// L'heure de Discord est gardée telle quelle, même à plusieurs
        /// jours de la nôtre (horloge locale en avance, B1).
        #[test]
        fn discord_clock_is_kept_even_when_far_from_ours() {
            let mut s = Store::new();
            assert_eq!(s.discord_now_ms(), None, "aucune réponse HTTP");
            let server = now_ms() - 3 * 86_400_000;
            s.apply(Event::ServerTime(server));
            let seen = s.discord_now_ms().unwrap_or_default();
            assert!(seen >= server && seen < server + 60_000, "{seen} / {server}");
            assert_eq!(s.clock_offset, 0, "écart absurde : pas appliqué aux exclusions");
        }

        #[test]
        fn forbidden_send_during_timeout_shows_reason() {
            let mut s = known();
            let until = now_ms() + 3_600_000;
            set_me(&mut s, Member { timeout_until: Some(until), ..Default::default() });
            let reason = Notice::NotSent(Problem::Timeout { until_ms: until });
            let eff = s.apply(Event::SendError { channel_id: CHAN, nonce: 1, content: "x".into(), error: Problem::Api("refus".into()), forbidden: true });
            assert_eq!(eff, Some(Effect::Toast(reason)));
            // Autre erreur : son propre texte.
            let eff = s.apply(Event::SendError { channel_id: CHAN, nonce: 1, content: "x".into(), error: Problem::Api("délai".into()), forbidden: false });
            assert_eq!(eff, Some(Effect::Toast(Notice::NotSent(Problem::Api("délai".into())))));
            // Exclusion finie depuis longtemps : le refus n'est plus attribué.
            set_me(&mut s, Member { timeout_until: Some(now_ms() - RECENT_TIMEOUT - 60_000), ..Default::default() });
            let eff = s.apply(Event::SendError { channel_id: CHAN, nonce: 1, content: "x".into(), error: Problem::Api("refus".into()), forbidden: true });
            assert_eq!(eff, Some(Effect::Toast(Notice::NotSent(Problem::Api("refus".into())))));
        }

        #[test]
        fn unreadable_roles_or_owner_make_guild_unknown() {
            let mut s = known();
            s.apply(Event::GuildRolesUnreadable { guild_id: G });
            assert_eq!(s.can(CHAN, P::VIEW_CHANNEL), Verdict::Inconnu);
            assert!(!s.is_hidden(SECRET));
            let mut s = known();
            s.apply(Event::GuildUpdate { id: G, owner_id: Some(None), icon: None });
            assert_eq!(s.guilds[0].owner_id, None);
            assert!(!s.is_hidden(SECRET) && s.send_block(READONLY).is_none());
        }
    }
}
