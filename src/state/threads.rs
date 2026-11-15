//! Fils de discussion (types 10, 11, 12) et salons forum ou média (15, 16) :
//! modèle, événements, commandes et magasin des fils (lot T10).
//!
//! Un fil se comporte comme un salon pour tout le reste (messages, saisie,
//! réactions) : le magasin central lui donne des permissions tirées de son
//! salon parent (`thread_permissions`) et le range sous ce parent.
//!
//! Mémoire bornée : au plus `MAX_THREADS` fils gardés. Au-delà, partent
//! d'abord les fils archivés que je n'ai pas rejoints, puis les autres fils
//! non rejoints, du plus ancien au plus récent (dernière activité) ; un fil
//! rejoint ou ouvert ne part jamais.

use super::notice::Problem;
use super::perms::{implicit, Permissions};
use super::social::Emoji;
use super::usersettings::notif::Level;
use std::collections::{HashMap, HashSet};

/// Fils gardés en mémoire, tous serveurs confondus.
pub const MAX_THREADS: usize = 5000;
/// Fils archivés demandés par page (documenté : 2 à 100, 50 par défaut).
pub const ARCHIVED_PAGE: usize = 25;
/// Longueur maximale d'un nom de fil (documenté : 1 à 100 caractères).
pub const MAX_NAME: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThreadKind {
    /// Fil d'un salon d'annonces (10).
    Announcement,
    /// Fil public (11), dont les publications de forum.
    Public,
    /// Fil privé (12) : visible seulement de ses membres et des modérateurs.
    Private,
}

impl ThreadKind {
    pub fn from_code(code: u8) -> Option<ThreadKind> {
        match code {
            10 => Some(ThreadKind::Announcement),
            11 => Some(ThreadKind::Public),
            12 => Some(ThreadKind::Private),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Thread {
    pub id: u64,
    pub guild_id: u64,
    pub parent_id: u64,
    pub name: String,
    pub kind: ThreadKind,
    pub owner_id: Option<u64>,
    pub archived: bool,
    pub locked: bool,
    /// Durée d'inactivité avant archivage automatique (minutes).
    pub auto_archive: u32,
    /// Dernier changement d'état d'archivage (ms Unix).
    pub archived_at: Option<i64>,
    /// Création (ms Unix) : `create_timestamp`, sinon tirée de l'identifiant.
    pub created_at: i64,
    /// Messages, sans le message de départ ni les messages supprimés.
    pub message_count: u32,
    pub member_count: u32,
    /// Étiquettes du forum appliquées à cette publication.
    pub applied_tags: Vec<u64>,
    pub last_message_id: Option<u64>,
    /// Mode lent (secondes).
    pub slowmode: u32,
    /// Publication épinglée en tête du forum (drapeau 1 << 1).
    pub pinned: bool,
    /// J'en suis membre (je l'ai rejoint ou j'y ai été ajouté).
    pub joined: bool,
}

impl Thread {
    /// Dernière activité : dernier message, sinon création.
    pub fn activity(&self) -> u64 {
        self.last_message_id.unwrap_or(self.id).max(self.id)
    }

    pub fn stub(&self) -> ThreadStub {
        ThreadStub { id: self.id, name: self.name.clone(), messages: self.message_count, archived: self.archived }
    }
}

/// Étiquette disponible d'un forum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ForumTag {
    pub id: u64,
    pub name: String,
    /// Réservée aux membres qui ont MANAGE_THREADS.
    pub moderated: bool,
    pub emoji: Option<Emoji>,
}

/// Ce qu'un salon forum ou média apporte en plus d'un salon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Forum {
    pub id: u64,
    pub guild_id: u64,
    pub tags: Vec<ForumTag>,
    /// Réaction proposée par défaut sous les publications.
    pub default_reaction: Option<Emoji>,
    /// Salon média (16) plutôt que forum (15).
    pub media: bool,
}

/// Fil né d'un message, résumé sous ce message (« N messages »).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ThreadStub {
    pub id: u64,
    pub name: String,
    pub messages: u32,
    pub archived: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ThreadAction {
    Join,
    Leave,
    Archive,
    Create,
    Post,
    Archived,
    /// Réglage de notification du fil (lot N).
    Notify,
    /// Message de départ d'une publication.
    Starter,
    /// Verrouillage ou déverrouillage (lot FF2).
    Lock,
    /// Liste des membres du fil (lot FF2).
    Members,
}

/// Membres d'un fil demandés au plus par lecture (documenté : 1 à 100).
pub const MEMBERS_PAGE: usize = 100;

/// Réseau -> magasin.
#[derive(Debug)]
pub enum ThreadEvent {
    /// THREAD_CREATE, THREAD_UPDATE ou réponse REST. `joined` : `None` si le
    /// paquet ne dit pas si j'en suis membre (rien ne change alors).
    Upsert { thread: Thread, joined: Option<bool> },
    /// THREAD_DELETE.
    Delete { id: u64 },
    /// Fils actifs d'un serveur : GUILD_CREATE (`parents` absent, tout le
    /// serveur) ou THREAD_LIST_SYNC (`parents` : salons resynchronisés).
    /// Les fils actifs absents de la liste ne le sont plus. `joined` : ceux
    /// dont je suis membre.
    Sync { guild_id: u64, parents: Option<Vec<u64>>, threads: Vec<Thread>, joined: Vec<u64> },
    /// THREAD_MEMBER_UPDATE (moi) ou THREAD_MEMBERS_UPDATE qui m'ajoute ou me retire.
    Joined { thread_id: u64, joined: bool },
    /// THREAD_MEMBERS_UPDATE.
    Members { thread_id: u64, count: u32 },
    /// Forums d'un serveur (READY, GUILD_CREATE) : remplacent les anciens.
    Forums { guild_id: u64, forums: Vec<Forum> },
    /// Forum créé ou modifié (CHANNEL_CREATE, CHANNEL_UPDATE).
    Forum(Forum),
    /// Sujets des salons d'un serveur (READY, GUILD_CREATE) : remplacent les anciens.
    Topics { guild_id: u64, topics: Vec<(u64, String)> },
    /// Sujet d'un salon créé ou modifié (`None` : plus de sujet).
    Topic { guild_id: u64, channel_id: u64, topic: Option<String> },
    /// Salons à limite d'âge d'un serveur (READY, GUILD_CREATE) : remplacent les anciens.
    AgeGates { guild_id: u64, channels: Vec<u64> },
    /// Limite d'âge d'un salon créé ou modifié.
    AgeGate { guild_id: u64, channel_id: u64, on: bool },
    /// Message de départ d'une publication (vide : introuvable ou refusé).
    Starter { thread_id: u64, starter: Starter },
    /// Page de fils archivés publics d'un salon (`before` : celui de la demande).
    Archived { parent_id: u64, before: Option<i64>, threads: Vec<Thread>, joined: Vec<u64>, more: bool },
    /// Fil ou publication créé à ma demande : à ouvrir.
    Created { thread_id: u64 },
    /// Mes réglages de notification de fils : (fil, drapeaux de membre). Seuls
    /// les fils dont un réglage est choisi sont cités.
    Notifs(Vec<(u64, u64)>),
    /// Membres d'un fil : (identifiant, nom affiché), première page.
    MemberList { thread_id: u64, members: Vec<(u64, String)> },
    Failed { action: ThreadAction, thread_id: Option<u64>, problem: Problem },
}

/// Magasin -> réseau. Chaque commande suit un geste de l'utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ThreadCmd {
    Join { thread_id: u64 },
    Leave { thread_id: u64 },
    SetArchived { thread_id: u64, archived: bool },
    /// Page de fils archivés publics, avant `before` (ms Unix ; aucun : les plus récents).
    Archived { parent_id: u64, before: Option<i64> },
    CreateFromMessage { channel_id: u64, message_id: u64, name: String },
    /// Fil public sans message d'origine (menu « + » de la saisie) ;
    /// `announcement` : dans un salon d'annonces (type 10 au lieu de 11).
    CreateStandalone { channel_id: u64, name: String, announcement: bool },
    /// Fil privé (type 12), sans message d'origine (lot FF2).
    CreatePrivate { channel_id: u64, name: String },
    /// Verrouille ou déverrouille un fil (lot FF2).
    SetLocked { thread_id: u64, locked: bool },
    /// Première page des membres d'un fil (lot FF2).
    Members { thread_id: u64 },
    CreatePost { forum_id: u64, name: String, content: String, tags: Vec<u64> },
    /// Réglage de notification du fil : drapeaux de membre à écrire.
    Notify { thread_id: u64, flags: u64 },
    /// Message de départ d'une publication (galerie d'un forum, grille d'un salon média).
    Starter { thread_id: u64 },
}

/// Message de départ d'une publication : extrait et vignette de sa carte.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Starter {
    /// Début du texte (au plus `STARTER_TEXT` caractères).
    pub text: String,
    /// Première image jointe ou intégrée : adresse (proxy média) et dimensions.
    pub image: Option<(String, u32, u32)>,
}

/// Longueur de l'extrait gardé.
pub const STARTER_TEXT: usize = 300;
/// Messages de départ gardés ; au-delà, tout est oublié et redemandé à l'affichage.
const MAX_STARTERS: usize = 400;
/// Demandes de message de départ en vol à la fois.
const STARTERS_IN_FLIGHT: usize = 3;

/// Drapeaux de membre d'un fil qui règlent ses notifications
/// (docs.discord.food, « Thread Member Flags »).
pub const NOTIFY_ALL: u64 = 1 << 1;
pub const NOTIFY_MENTIONS: u64 = 1 << 2;
pub const NOTIFY_NOTHING: u64 = 1 << 3;

/// Niveau que disent des drapeaux de membre ; `None` : aucun n'est choisi.
pub fn notif_level(flags: u64) -> Option<Level> {
    if flags & NOTIFY_NOTHING != 0 {
        Some(Level::Nothing)
    } else if flags & NOTIFY_MENTIONS != 0 {
        Some(Level::Mentions)
    } else if flags & NOTIFY_ALL != 0 {
        Some(Level::All)
    } else {
        None
    }
}

/// Drapeau à écrire pour ce niveau (`Inherit` n'existe pas pour un fil).
pub fn notif_flags(level: Level) -> Option<u64> {
    match level {
        Level::All => Some(NOTIFY_ALL),
        Level::Mentions => Some(NOTIFY_MENTIONS),
        Level::Nothing => Some(NOTIFY_NOTHING),
        Level::Inherit => None,
    }
}

/// Chargement des fils archivés d'un salon (vue forum).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Archive {
    pub loading: bool,
    /// Il reste des pages (vrai tant qu'aucune n'a été reçue).
    pub more: bool,
    /// Archivage le plus ancien reçu : repère de la page suivante.
    pub oldest: Option<i64>,
    pub error: Option<Problem>,
}

/// Permissions dans un fil, tirées de celles du salon parent : SEND_MESSAGES
/// y est remplacé par SEND_MESSAGES_IN_THREADS ; un fil privé n'est visible
/// que de ses membres et de qui a MANAGE_THREADS ; un fil verrouillé
/// n'accepte de messages que de qui a MANAGE_THREADS (documentation de
/// Discord, « Permissions » et « Threads »).
pub fn thread_permissions(parent: Permissions, t: &Thread) -> Permissions {
    if !parent.contains(Permissions::VIEW_CHANNEL) {
        return Permissions::NONE;
    }
    let manage = parent.contains(Permissions::MANAGE_THREADS);
    if t.kind == ThreadKind::Private && !t.joined && !manage {
        return Permissions::NONE;
    }
    let mut p = parent & !Permissions::SEND_MESSAGES;
    if parent.contains(Permissions::SEND_MESSAGES_IN_THREADS) && (!t.locked || manage) {
        p = p | Permissions::SEND_MESSAGES;
    }
    implicit(p)
}

/// Membres d'un fil : identifiant et nom affiché.
type MemberNames = Vec<(u64, String)>;

#[derive(Default)]
pub struct Threads {
    map: HashMap<u64, Thread>,
    /// Fils de chaque salon parent, du plus récemment actif au plus ancien.
    by_parent: HashMap<u64, Vec<u64>>,
    forums: HashMap<u64, Forum>,
    /// Sujet des salons qui en ont un : (serveur, texte). Borné par les salons du compte.
    topics: HashMap<u64, (u64, String)>,
    /// Salons à limite d'âge, et leur serveur (lot FF, F8). Borné par les salons du compte.
    age_gates: HashMap<u64, u64>,
    /// Ceux dont j'ai passé l'écran d'entrée, pour la session.
    age_passed: HashSet<u64>,
    archive: HashMap<u64, Archive>,
    /// Écritures en cours (une seule par fil et par action).
    pending: HashSet<(ThreadAction, u64)>,
    /// Niveau de notification choisi pour un fil (aucun : celui du serveur).
    /// Oublié avec le fil ; borné comme `map`.
    notif: HashMap<u64, Level>,
    /// Messages de départ des publications affichées en galerie (`None` :
    /// demandé, pas encore reçu). Borné : `MAX_STARTERS`.
    starters: HashMap<u64, Option<Starter>>,
    /// Liste des membres du fil dont la fenêtre est ouverte (`None` :
    /// demandée, pas encore reçue). Une seule à la fois.
    member_list: Option<(u64, Option<MemberNames>)>,
    /// Change à chaque modification : la vue forum ne refait son tri que là.
    pub rev: u64,
    /// Fils ou publications créés à ma demande depuis le début (le brouillon
    /// d'une publication n'est effacé qu'une fois celle-ci créée).
    pub created: u64,
}

impl Threads {
    pub fn clear(&mut self) {
        *self = Threads { rev: self.rev + 1, created: self.created, ..Default::default() };
    }

    pub fn get(&self, id: u64) -> Option<&Thread> {
        self.map.get(&id)
    }

    /// Message de départ reçu de cette publication.
    pub fn starter(&self, id: u64) -> Option<&Starter> {
        self.starters.get(&id)?.as_ref()
    }

    /// Faut-il demander le message de départ de `id` ? Vrai une seule fois,
    /// et pas plus de `STARTERS_IN_FLIGHT` demandes à la fois.
    pub fn ask_starter(&mut self, id: u64) -> bool {
        if self.starters.contains_key(&id) || self.starters.values().filter(|s| s.is_none()).count() >= STARTERS_IN_FLIGHT {
            return false;
        }
        if self.starters.len() >= MAX_STARTERS {
            self.starters.retain(|_, s| s.is_none());
        }
        self.starters.insert(id, None);
        true
    }

    pub fn set_starter(&mut self, id: u64, starter: Starter) {
        self.starters.insert(id, Some(starter));
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Fils de ce salon, du plus récemment actif au plus ancien.
    pub fn children(&self, parent: u64) -> &[u64] {
        self.by_parent.get(&parent).map_or(&[], Vec::as_slice)
    }

    pub fn forum(&self, id: u64) -> Option<&Forum> {
        self.forums.get(&id)
    }

    /// Sujet du salon, s'il en a un.
    pub fn topic(&self, channel: u64) -> Option<&str> {
        self.topics.get(&channel).map(|(_, t)| t.as_str())
    }

    pub fn set_topics(&mut self, guild: u64, topics: Vec<(u64, String)>) {
        self.topics.retain(|_, (g, _)| *g != guild);
        self.topics.extend(topics.into_iter().map(|(c, t)| (c, (guild, t))));
    }

    /// Salon à limite d'âge dont l'écran d'entrée n'a pas été passé.
    pub fn age_gated(&self, channel: u64) -> bool {
        self.age_gates.contains_key(&channel) && !self.age_passed.contains(&channel)
    }

    /// Salon marqué « limite d'âge » (passé ou non).
    pub fn age_restricted(&self, channel: u64) -> bool {
        self.age_gates.contains_key(&channel)
    }

    /// « Continuer » sur l'écran d'entrée.
    pub fn pass_age_gate(&mut self, channel: u64) {
        self.age_passed.insert(channel);
    }

    pub fn set_age_gates(&mut self, guild: u64, channels: Vec<u64>) {
        self.age_gates.retain(|_, g| *g != guild);
        self.age_gates.extend(channels.into_iter().map(|c| (c, guild)));
        let gates = &self.age_gates;
        self.age_passed.retain(|c| gates.contains_key(c));
    }

    pub fn set_age_gate(&mut self, guild: u64, channel: u64, on: bool) {
        if on {
            self.age_gates.insert(channel, guild);
        } else {
            self.age_gates.remove(&channel);
            self.age_passed.remove(&channel);
        }
    }

    pub fn set_topic(&mut self, guild: u64, channel: u64, topic: Option<String>) {
        match topic {
            Some(t) => self.topics.insert(channel, (guild, t)),
            None => self.topics.remove(&channel),
        };
    }

    pub fn archive(&self, parent: u64) -> Option<&Archive> {
        self.archive.get(&parent)
    }

    /// Niveau de notification choisi pour ce fil, s'il y en a un.
    pub fn notif(&self, id: u64) -> Option<Level> {
        self.notif.get(&id).copied()
    }

    /// Retient le niveau que disent ces drapeaux de membre (aucun bit : rien ne change).
    pub fn set_notif(&mut self, id: u64, flags: u64) {
        if let Some(l) = notif_level(flags) {
            if self.notif.len() >= MAX_THREADS && !self.notif.contains_key(&id) {
                let map = &self.map;
                self.notif.retain(|t, _| map.contains_key(t));
            }
            if self.notif.insert(id, l) != Some(l) {
                self.touch();
            }
        }
    }

    /// Membres connus du fil `id` : `None` si rien n'est demandé pour lui,
    /// `Some(None)` pendant la lecture.
    pub fn member_list(&self, id: u64) -> Option<Option<&[(u64, String)]>> {
        self.member_list.as_ref().filter(|(t, _)| *t == id).map(|(_, l)| l.as_deref())
    }

    pub(super) fn ask_members(&mut self, id: u64) {
        self.member_list = Some((id, None));
    }

    pub(super) fn set_member_list(&mut self, id: u64, members: Vec<(u64, String)>) {
        if self.member_list.as_ref().is_some_and(|(t, _)| *t == id) {
            self.member_list = Some((id, Some(members)));
        }
    }

    /// Fenêtre fermée, ou lecture refusée : la liste est oubliée.
    pub fn forget_members(&mut self) {
        self.member_list = None;
    }

    pub fn pending(&self, action: ThreadAction, id: u64) -> bool {
        self.pending.contains(&(action, id))
    }

    /// Marque une écriture en cours ; `false` si elle l'était déjà.
    pub(super) fn begin(&mut self, action: ThreadAction, id: u64) -> bool {
        self.pending.insert((action, id))
    }

    pub(super) fn end(&mut self, action: ThreadAction, id: u64) {
        self.pending.remove(&(action, id));
    }

    fn touch(&mut self) {
        self.rev = self.rev.wrapping_add(1);
    }

    fn reindex(&mut self, parent: u64) {
        let map = &self.map;
        let mut ids: Vec<u64> = map.values().filter(|t| t.parent_id == parent).map(|t| t.id).collect();
        ids.sort_by_key(|id| std::cmp::Reverse(map.get(id).map_or(0, Thread::activity)));
        if ids.is_empty() {
            self.by_parent.remove(&parent);
        } else {
            self.by_parent.insert(parent, ids);
        }
    }

    /// Ajoute ou remplace un fil ; `joined` absent : l'appartenance connue est gardée.
    /// Renvoie le serveur touché.
    pub fn upsert(&mut self, mut t: Thread, joined: Option<bool>, keep: Option<u64>) -> u64 {
        let old = self.map.get(&t.id);
        t.joined = joined.or(old.map(|o| o.joined)).unwrap_or(false);
        // Un paquet partiel (THREAD_UPDATE) peut ne pas porter le dernier message.
        if t.last_message_id.is_none() {
            t.last_message_id = old.and_then(|o| o.last_message_id);
        }
        let old_parent = old.map(|o| o.parent_id);
        let (id, parent, guild) = (t.id, t.parent_id, t.guild_id);
        self.map.insert(id, t);
        if let Some(p) = old_parent.filter(|p| *p != parent) {
            self.reindex(p);
        }
        self.reindex(parent);
        self.evict(keep);
        self.touch();
        guild
    }

    pub fn remove(&mut self, id: u64) -> Option<Thread> {
        self.notif.remove(&id);
        let t = self.map.remove(&id)?;
        self.reindex(t.parent_id);
        self.touch();
        Some(t)
    }

    /// Fils actifs resynchronisés (voir `ThreadEvent::Sync`).
    pub fn sync(&mut self, guild_id: u64, parents: Option<&[u64]>, threads: Vec<Thread>, joined: &[u64], keep: Option<u64>) {
        let fresh: HashSet<u64> = threads.iter().map(|t| t.id).collect();
        let stale: Vec<u64> = self
            .map
            .values()
            .filter(|t| t.guild_id == guild_id && !t.archived && !fresh.contains(&t.id))
            .filter(|t| parents.is_none_or(|p| p.contains(&t.parent_id)))
            .map(|t| t.id)
            .collect();
        let mut touched: HashSet<u64> = HashSet::new();
        for id in stale {
            if let Some(t) = self.map.remove(&id) {
                touched.insert(t.parent_id);
            }
        }
        for mut t in threads {
            t.joined = joined.contains(&t.id) || self.map.get(&t.id).is_some_and(|o| o.joined && parents.is_some());
            if t.last_message_id.is_none() {
                t.last_message_id = self.map.get(&t.id).and_then(|o| o.last_message_id);
            }
            touched.insert(t.parent_id);
            self.map.insert(t.id, t);
        }
        for p in touched {
            self.reindex(p);
        }
        self.evict(keep);
        self.touch();
    }

    /// Change mon appartenance ; renvoie le serveur du fil s'il est connu.
    pub fn set_joined(&mut self, id: u64, joined: bool) -> Option<u64> {
        let t = self.map.get_mut(&id)?;
        if t.joined == joined {
            return None;
        }
        t.joined = joined;
        let g = t.guild_id;
        self.touch();
        Some(g)
    }

    pub fn set_members(&mut self, id: u64, count: u32) {
        if let Some(t) = self.map.get_mut(&id)
            && t.member_count != count
        {
            t.member_count = count;
            self.touch();
        }
    }

    /// Message reçu dans un fil : compte et dernière activité. Renvoie le fil
    /// s'il est connu (et s'il a changé).
    pub fn note_message(&mut self, channel: u64, message: u64, delta: i32) -> Option<&Thread> {
        let t = self.map.get_mut(&channel)?;
        let parent = t.parent_id;
        if delta > 0 {
            if t.last_message_id.is_some_and(|l| l >= message) {
                return None;
            }
            t.last_message_id = Some(message);
            t.message_count = t.message_count.saturating_add(1);
        } else {
            t.message_count = t.message_count.saturating_sub(1);
        }
        self.reindex(parent);
        self.touch();
        self.map.get(&channel)
    }

    /// Salon parent supprimé : ses fils partent avec lui.
    pub fn remove_parent(&mut self, parent: u64) {
        let before = self.map.len();
        self.map.retain(|_, t| t.parent_id != parent);
        self.by_parent.remove(&parent);
        self.forums.remove(&parent);
        self.archive.remove(&parent);
        self.topics.remove(&parent);
        self.age_gates.remove(&parent);
        self.age_passed.remove(&parent);
        if self.map.len() != before {
            self.touch();
        }
    }

    pub fn remove_guild(&mut self, guild: u64) {
        self.map.retain(|_, t| t.guild_id != guild);
        let map = &self.map;
        self.by_parent.retain(|_, ids| ids.iter().any(|id| map.contains_key(id)));
        self.topics.retain(|_, (g, _)| *g != guild);
        self.age_gates.retain(|_, g| *g != guild);
        self.forums.retain(|_, f| f.guild_id != guild);
        let forums = &self.forums;
        self.archive.retain(|p, _| forums.contains_key(p) || map.values().any(|t| t.parent_id == *p));
        self.touch();
    }

    pub fn set_forums(&mut self, guild: u64, forums: Vec<Forum>) {
        self.forums.retain(|_, f| f.guild_id != guild);
        for f in forums {
            self.forums.insert(f.id, f);
        }
        self.touch();
    }

    pub fn set_forum(&mut self, f: Forum) {
        self.forums.insert(f.id, f);
        self.touch();
    }

    /// Demande de page d'archives : `Some(before)` si elle peut partir.
    pub(super) fn ask_archived(&mut self, parent: u64) -> Option<Option<i64>> {
        let a = self.archive.entry(parent).or_insert(Archive { more: true, ..Default::default() });
        if a.loading || !a.more {
            return None;
        }
        a.loading = true;
        a.error = None;
        let before = a.oldest;
        self.touch();
        Some(before)
    }

    /// Page d'archives reçue ; ignorée si elle ne répond pas à la demande en cours.
    pub fn archived_page(&mut self, parent: u64, before: Option<i64>, threads: Vec<Thread>, joined: &[u64], more: bool, keep: Option<u64>) {
        let Some(a) = self.archive.get_mut(&parent) else { return };
        if !a.loading || a.oldest != before {
            return;
        }
        a.loading = false;
        a.more = more && !threads.is_empty();
        let oldest = threads.iter().filter_map(|t| t.archived_at).min();
        if let Some(o) = oldest {
            a.oldest = Some(a.oldest.map_or(o, |x| x.min(o)));
        } else {
            a.more = false;
        }
        for mut t in threads {
            // Un fil déjà connu (actif entre-temps) garde ce que la passerelle a dit.
            if self.map.contains_key(&t.id) {
                continue;
            }
            t.joined = joined.contains(&t.id);
            self.map.insert(t.id, t);
        }
        self.reindex(parent);
        self.evict(keep);
        self.touch();
    }

    pub(super) fn archived_failed(&mut self, parent: u64, problem: Problem) {
        if let Some(a) = self.archive.get_mut(&parent) {
            a.loading = false;
            a.error = Some(problem);
            self.touch();
        }
    }

    /// Ramène la table à `MAX_THREADS` (politique en tête du module).
    fn evict(&mut self, keep: Option<u64>) {
        if self.map.len() <= MAX_THREADS {
            return;
        }
        let mut victims: Vec<(bool, u64, u64, u64)> =
            self.map.values().filter(|t| !t.joined && Some(t.id) != keep).map(|t| (!t.archived, t.activity(), t.id, t.parent_id)).collect();
        victims.sort_unstable();
        let excess = self.map.len() - MAX_THREADS;
        let mut parents = HashSet::new();
        for (_, _, id, parent) in victims.into_iter().take(excess) {
            self.map.remove(&id);
            parents.insert(parent);
        }
        for p in parents {
            self.reindex(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub(crate) fn thread(id: u64, parent: u64) -> Thread {
        Thread {
            id,
            guild_id: 1,
            parent_id: parent,
            name: format!("fil {id}"),
            kind: ThreadKind::Public,
            owner_id: None,
            archived: false,
            locked: false,
            auto_archive: 1440,
            archived_at: None,
            created_at: 0,
            message_count: 0,
            member_count: 0,
            applied_tags: Vec::new(),
            last_message_id: None,
            slowmode: 0,
            pinned: false,
            joined: false,
        }
    }

    use Permissions as P;

    #[test]
    fn starters_are_asked_once_and_a_few_at_a_time() {
        let mut t = Threads::default();
        assert!(t.ask_starter(1) && t.ask_starter(2) && t.ask_starter(3));
        assert!(!t.ask_starter(4), "trois demandes en vol au plus");
        assert!(!t.ask_starter(1), "déjà demandé");
        assert!(t.starter(1).is_none());
        t.set_starter(1, Starter { text: "a".into(), image: None });
        assert_eq!(t.starter(1).map(|s| s.text.as_str()), Some("a"));
        assert!(t.ask_starter(4) && !t.ask_starter(1), "reçu : jamais redemandé");
        // Mémoire bornée : au-delà, les messages reçus sont oubliés (redemandés à l'affichage).
        for id in 2..=4 {
            t.set_starter(id, Starter::default());
        }
        for id in 10..(10 + MAX_STARTERS as u64 + 5) {
            assert!(t.ask_starter(id));
            t.set_starter(id, Starter::default());
        }
        assert!(t.starters.len() <= MAX_STARTERS);
    }

    #[test]
    fn permissions_follow_the_parent() {
        let parent = P(P::VIEW_CHANNEL.0 | P::SEND_MESSAGES.0 | P::READ_MESSAGE_HISTORY.0 | P::ATTACH_FILES.0);
        let mut t = thread(10, 2);
        // Sans SEND_MESSAGES_IN_THREADS, on lit sans écrire (ni pièces jointes).
        let p = thread_permissions(parent, &t);
        assert!(p.contains(P::VIEW_CHANNEL) && !p.contains(P::SEND_MESSAGES) && !p.contains(P::ATTACH_FILES));
        // Avec, on écrit même sans SEND_MESSAGES dans le parent.
        let parent2 = P((parent.0 & !P::SEND_MESSAGES.0) | P::SEND_MESSAGES_IN_THREADS.0);
        assert!(thread_permissions(parent2, &t).contains(P::SEND_MESSAGES));
        // Verrouillé : seulement avec MANAGE_THREADS.
        t.locked = true;
        assert!(!thread_permissions(parent2, &t).contains(P::SEND_MESSAGES));
        assert!(thread_permissions(P(parent2.0 | P::MANAGE_THREADS.0), &t).contains(P::SEND_MESSAGES));
        // Privé : invisible sans en être membre ni modérateur.
        let mut private = thread(11, 2);
        private.kind = ThreadKind::Private;
        assert_eq!(thread_permissions(parent2, &private), P::NONE);
        private.joined = true;
        assert!(thread_permissions(parent2, &private).contains(P::VIEW_CHANNEL));
        // Parent invisible : fil invisible ; administrateur : tout.
        assert_eq!(thread_permissions(P::SEND_MESSAGES_IN_THREADS, &t), P::NONE);
        assert_eq!(thread_permissions(P::ALL, &private), P::ALL);
    }

    #[test]
    fn children_are_sorted_by_activity_and_upsert_keeps_membership() {
        let mut s = Threads::default();
        s.upsert(thread(10 << 22, 2), Some(true), None);
        let mut b = thread(11 << 22, 2);
        b.last_message_id = Some(12 << 22);
        s.upsert(b, None, None);
        s.upsert(thread(13 << 22, 2), None, None);
        assert_eq!(s.children(2), &[13 << 22, 11 << 22, 10 << 22]);
        // THREAD_UPDATE sans membre : rejoint reste vrai, dernier message gardé.
        s.upsert(thread(10 << 22, 2), None, None);
        assert!(s.get(10 << 22).is_some_and(|t| t.joined));
        s.upsert(thread(11 << 22, 2), None, None);
        assert_eq!(s.get(11 << 22).and_then(|t| t.last_message_id), Some(12 << 22));
        // Un message dans le plus ancien le fait remonter et le compte.
        assert!(s.note_message(10 << 22, 20 << 22, 1).is_some());
        assert_eq!(s.children(2)[0], 10 << 22);
        assert_eq!(s.get(10 << 22).map(|t| t.message_count), Some(1));
        // Même message reçu deux fois : compté une fois.
        assert!(s.note_message(10 << 22, 20 << 22, 1).is_none());
    }

    #[test]
    fn sync_drops_active_threads_that_are_gone() {
        let mut s = Threads::default();
        s.upsert(thread(10, 2), None, None);
        s.upsert(thread(11, 3), None, None);
        let mut archived = thread(12, 2);
        archived.archived = true;
        s.upsert(archived, None, None);
        // THREAD_LIST_SYNC du seul salon 2 : 10 n'est plus actif, 11 (autre salon) reste, l'archivé aussi.
        s.sync(1, Some(&[2]), vec![thread(13, 2)], &[13], None);
        assert!(s.get(10).is_none() && s.get(11).is_some() && s.get(12).is_some());
        assert!(s.get(13).is_some_and(|t| t.joined));
        // GUILD_CREATE : tout le serveur.
        s.sync(1, None, vec![], &[], None);
        assert!(s.get(11).is_none() && s.get(13).is_none() && s.get(12).is_some());
    }

    #[test]
    fn eviction_spares_joined_and_open_threads() {
        let mut s = Threads::default();
        s.upsert(thread(1, 2), Some(true), None);
        s.upsert(thread(2, 2), None, None);
        for i in 0..MAX_THREADS as u64 {
            s.upsert(thread(100 + i, 3), None, Some(2));
        }
        assert_eq!(s.len(), MAX_THREADS);
        assert!(s.get(1).is_some() && s.get(2).is_some());
        assert!(s.get(100).is_none(), "le plus ancien non rejoint part");
    }

    #[test]
    fn archived_pages_follow_the_oldest_and_ignore_stale_answers() {
        let mut s = Threads::default();
        assert_eq!(s.ask_archived(2), Some(None));
        assert_eq!(s.ask_archived(2), None, "une seule demande à la fois");
        let mut a = thread(20, 2);
        a.archived = true;
        a.archived_at = Some(5_000);
        let mut b = thread(21, 2);
        b.archived = true;
        b.archived_at = Some(3_000);
        // Réponse à une autre demande : ignorée.
        s.archived_page(2, Some(9), vec![a.clone()], &[], true, None);
        assert!(s.get(20).is_none());
        s.archived_page(2, None, vec![a, b], &[21], true, None);
        assert!(s.get(21).is_some_and(|t| t.joined));
        assert_eq!(s.ask_archived(2), Some(Some(3_000)));
        s.archived_page(2, Some(3_000), vec![], &[], false, None);
        assert_eq!(s.archive(2).map(|a| a.more), Some(false));
        assert_eq!(s.ask_archived(2), None, "plus rien à charger");
    }
}
