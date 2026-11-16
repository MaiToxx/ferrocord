//! Utilisateurs, membres et présences : ce que les événements documentés
//! apprennent sur les autres (objet `user` complet des messages, objets
//! `member`, `PRESENCE_UPDATE`, `presences` de GUILD_CREATE et du READY), et
//! ce que la carte de profil demande à l'ouverture (profil complet,
//! `state::profiles` ; à défaut `GET /users/{id}`, `GET /guilds/{id}/members/{id}`).
//!
//! Partagé entre le réseau et l'interface ([`PeopleHub`]) plutôt que transmis
//! par événements : un flot de présences (des milliers par seconde sur un gros
//! serveur) ne doit ni remplir la file des événements ni réveiller
//! l'interface. Le réseau écrit ici sous le verrou et ne réveille l'interface
//! que si un utilisateur affiché à la dernière image a changé ([`People::watched`]).
//!
//! Tout est borné ; politique d'éviction : quand une table dépasse sa
//! capacité, le huitième le plus anciennement mis à jour est retiré d'un
//! coup (coût amorti constant), sauf les utilisateurs affichés.

use super::event::Cmd;
use super::model::{Avatar, ImageHash};
use super::profiles::{ProfileCmd, ProfileTables};
use super::store::Store;
use std::{
    collections::{HashMap, HashSet},
    fmt,
    hash::Hash,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

/// Utilisateurs gardés (nom, avatar, badges…).
pub const MAX_USERS: usize = 5_000;
/// Membres de serveur gardés (un par couple serveur-utilisateur).
pub const MAX_MEMBERS: usize = 5_000;
/// Présences gardées.
pub const MAX_PRESENCES: usize = 5_000;
/// Activités gardées par présence (le reste est ignoré).
pub const MAX_ACTIVITIES: usize = 8;
/// Correspondances MP à deux ↔ correspondant.
pub const MAX_DMS: usize = 5_000;
/// Longueur gardée d'un texte reçu dans une présence (nom de jeu, statut
/// personnalisé…), en caractères.
pub const MAX_TEXT: usize = 128;

/// Statut en ligne (champ `status` de la présence, documenté :
/// `online`, `idle`, `dnd`, `offline`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    Online,
    Idle,
    Dnd,
    Offline,
}

impl Status {
    /// `invisible` n'est jamais diffusé aux autres (ils reçoivent `offline`) ;
    /// accepté par prudence. Toute autre valeur : inconnue.
    pub fn parse(s: &str) -> Option<Status> {
        match s {
            "online" => Some(Status::Online),
            "idle" => Some(Status::Idle),
            "dnd" => Some(Status::Dnd),
            "offline" | "invisible" => Some(Status::Offline),
            _ => None,
        }
    }
}

/// Statut par plateforme (`client_status`) ; `None` : pas connecté depuis
/// cette plateforme (ou inconnu).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ClientStatus {
    pub desktop: Option<Status>,
    pub mobile: Option<Status>,
    pub web: Option<Status>,
}

/// Type d'activité (documenté : 0 joue, 1 en direct, 2 écoute, 3 regarde,
/// 4 statut personnalisé, 5 participe).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Playing,
    Streaming,
    Listening,
    Watching,
    Custom,
    Competing,
}

impl ActivityKind {
    pub fn from_code(code: u64) -> Option<ActivityKind> {
        Some(match code {
            0 => ActivityKind::Playing,
            1 => ActivityKind::Streaming,
            2 => ActivityKind::Listening,
            3 => ActivityKind::Watching,
            4 => ActivityKind::Custom,
            5 => ActivityKind::Competing,
            _ => return None,
        })
    }
}

/// Emoji d'une activité : Unicode (`name` seul) ou personnalisé (`id`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivityEmoji {
    pub name: Option<String>,
    pub id: Option<u64>,
    /// Emoji personnalisé animé (joué selon le réglage des emojis animés).
    pub animated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub name: String,
    pub details: Option<String>,
    pub state: Option<String>,
    pub emoji: Option<ActivityEmoji>,
    /// Début (ms Unix, `timestamps.start`).
    pub start_ms: Option<i64>,
    /// Présence enrichie (images, fin, boutons…), seulement si l'activité
    /// en porte (lot C3).
    pub rich: Option<Box<Rich>>,
}

/// Image d'une activité : adresse construite par `net` (CDN de Discord
/// seulement ; `None` pour une image d'un autre site, jamais contacté) et
/// texte affiché au survol.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ActivityImage {
    pub url: Option<String>,
    pub text: Option<String>,
}

/// Champs de la présence enrichie (documentation de docs.discord.food,
/// « Presence » › Activity Object).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rich {
    pub app_id: Option<u64>,
    /// Fin (ms Unix, `timestamps.end`) : barre de progression d'une écoute.
    pub end_ms: Option<i64>,
    pub large: Option<ActivityImage>,
    pub small: Option<ActivityImage>,
    /// Libellés des boutons (2 au plus) ; leurs adresses se demandent au clic.
    pub buttons: Vec<String>,
    /// Session de l'autre client (pour l'adresse des boutons).
    pub session_id: Option<String>,
    /// Morceau Spotify (`sync_id`), lettres et chiffres seulement.
    pub sync_id: Option<String>,
    /// Écoute Spotify.
    pub spotify: bool,
    /// Adresse d'un direct (`url`, Twitch ou YouTube).
    pub url: Option<String>,
    /// Groupe (`party.size`) : joueurs présents, places.
    pub party: Option<(u32, u32)>,
}

impl Activity {
    pub fn rich(&self) -> Option<&Rich> {
        self.rich.as_deref()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Presence {
    pub status: Status,
    pub client: ClientStatus,
    pub activities: Vec<Activity>,
}

impl Presence {
    /// Statut personnalisé (activité de type 4), s'il y en a un.
    pub fn custom(&self) -> Option<&Activity> {
        self.activities.iter().find(|a| a.kind == ActivityKind::Custom)
    }
}

/// Utilisateur tel que décrit par l'objet `user` complet de l'API. Les champs
/// en `Option<Option<…>>` ne sont pas toujours fournis : `None` = inconnu
/// (un objet qui ne les porte pas n'efface pas ce qu'on savait), `Some(None)`
/// = absent du profil.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UserInfo {
    pub id: u64,
    pub username: String,
    pub global_name: Option<String>,
    /// Ancien discriminant (« 1234 ») ; `None` pour le nouveau système de noms (« 0 »).
    pub discriminator: Option<String>,
    pub avatar: Avatar,
    pub banner: Option<Option<ImageHash>>,
    /// Couleur d'accent 0xRRGGBB.
    pub accent: Option<Option<u32>>,
    /// `public_flags` (badges publics).
    pub flags: Option<u64>,
    pub bot: bool,
    pub system: bool,
    /// Décoration, plaque, tag de serveur, style du nom (lot P).
    pub look: super::cosmetics::Look,
}

impl UserInfo {
    /// Nom global s'il existe, sinon nom d'utilisateur.
    pub fn display_name(&self) -> &str {
        match &self.global_name {
            Some(n) if !n.is_empty() => n,
            _ => &self.username,
        }
    }

    /// Nom d'utilisateur tel que Discord l'écrit : `nom` ou `nom#1234`.
    pub fn handle(&self) -> String {
        match &self.discriminator {
            Some(d) => format!("{}#{d}", self.username),
            None => self.username.clone(),
        }
    }

    /// Les champs que `new` ne connaît pas gardent leur valeur.
    fn merge(&mut self, new: UserInfo) {
        let (banner, accent, flags) = (new.banner.or(self.banner), new.accent.or(self.accent), new.flags.or(self.flags));
        let look = self.look.merged(new.look);
        *self = UserInfo { banner, accent, flags, look, ..new };
    }
}

/// Badges publics documentés (User Object › User Flags), dans l'ordre d'affichage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Badge {
    Staff,
    Partner,
    CertifiedModerator,
    Hypesquad,
    HypesquadBravery,
    HypesquadBrilliance,
    HypesquadBalance,
    BugHunter,
    BugHunterGold,
    ActiveDeveloper,
    VerifiedDeveloper,
    EarlySupporter,
    VerifiedBot,
}

impl Badge {
    const BITS: [(u32, Badge); 13] = [
        (0, Badge::Staff),
        (1, Badge::Partner),
        (18, Badge::CertifiedModerator),
        (2, Badge::Hypesquad),
        (6, Badge::HypesquadBravery),
        (7, Badge::HypesquadBrilliance),
        (8, Badge::HypesquadBalance),
        (3, Badge::BugHunter),
        (14, Badge::BugHunterGold),
        (22, Badge::ActiveDeveloper),
        (17, Badge::VerifiedDeveloper),
        (9, Badge::EarlySupporter),
        (16, Badge::VerifiedBot),
    ];

    /// Badges d'un champ `public_flags` ; les bits non documentés sont ignorés.
    pub fn of(flags: u64) -> Vec<Badge> {
        Self::BITS.iter().filter(|(bit, _)| flags & (1 << bit) != 0).map(|(_, b)| *b).collect()
    }
}

/// Membre d'un serveur (objet `member`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemberInfo {
    pub nick: Option<String>,
    /// `None` : rôles absents ou illisibles (rien n'est affiché).
    pub roles: Option<Vec<u64>>,
    /// Arrivée sur le serveur (ms Unix).
    pub joined_at: Option<i64>,
    /// Avatar propre au serveur.
    pub avatar: Option<ImageHash>,
    /// Début du boost (ms Unix).
    pub premium_since: Option<i64>,
    /// Fin de l'exclusion temporaire (ms Unix).
    pub timeout_until: Option<i64>,
    /// Bannière propre au serveur (lot P).
    pub banner: Option<ImageHash>,
    /// Décoration d'avatar propre au serveur (lot P).
    pub decoration: Option<super::cosmetics::Asset>,
}

/// Table bornée ; chaque entrée porte l'instant de sa dernière écriture.
pub(super) struct Lru<K, V> {
    map: HashMap<K, (V, u64)>,
    cap: usize,
    tick: u64,
}

impl<K: Copy + Eq + Hash, V> Lru<K, V> {
    pub(super) fn new(cap: usize) -> Self {
        Lru { map: HashMap::new(), cap, tick: 0 }
    }

    pub(super) fn get(&self, k: &K) -> Option<&V> {
        self.map.get(k).map(|(v, _)| v)
    }

    pub(super) fn get_mut(&mut self, k: &K) -> Option<&mut V> {
        self.tick += 1;
        let tick = self.tick;
        self.map.get_mut(k).map(|(v, t)| {
            *t = tick;
            v
        })
    }

    /// `keep` : entrées à ne pas évincer.
    pub(super) fn insert(&mut self, k: K, v: V, keep: impl Fn(&K) -> bool) {
        self.tick += 1;
        self.map.insert(k, (v, self.tick));
        if self.map.len() > self.cap {
            self.evict(keep);
        }
    }

    /// Retire au moins le huitième le plus ancien de la capacité (sans
    /// jamais toucher à ce que `keep` protège).
    fn evict(&mut self, keep: impl Fn(&K) -> bool) {
        let mut old: Vec<(u64, K)> = self.map.iter().filter(|(k, _)| !keep(k)).map(|(k, (_, t))| (*t, *k)).collect();
        let n = (self.cap / 8).max(1).max(self.map.len().saturating_sub(self.cap)).min(old.len());
        if n == 0 {
            return;
        }
        old.select_nth_unstable_by_key(n - 1, |(t, _)| *t);
        for (_, k) in &old[..n] {
            self.map.remove(k);
        }
    }

    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    pub(super) fn clear(&mut self) {
        self.map.clear();
    }
}

/// Ce qu'on sait des autres utilisateurs. Accès par [`PeopleHub::lock`].
pub struct People {
    users: Lru<u64, UserInfo>,
    members: Lru<(u64, u64), MemberInfo>,
    presences: Lru<u64, Presence>,
    /// Correspondant d'un MP à deux → identifiant du MP, et l'inverse.
    dm_by_user: Lru<u64, u64>,
    dm_peer: Lru<u64, u64>,
    /// Profils complets et notes (`state::profiles`).
    profiles: ProfileTables,
    /// Utilisateurs dont l'interface a lu la présence ou le profil pendant
    /// l'image en cours ; deviennent `watched` à la fin de l'image.
    seen: HashSet<u64>,
    /// Utilisateurs affichés à la dernière image : seuls leurs changements
    /// réveillent l'interface.
    watched: HashSet<u64>,
    /// Amis (relations) : leur présence n'est jamais évincée, la page Amis
    /// la lit (filtre « En ligne ») même quand ils ne sont pas affichés.
    kept: HashSet<u64>,
    /// Change à chaque présence modifiée (la page Amis refait son filtre).
    presence_rev: u64,
}

impl Default for People {
    fn default() -> Self {
        People {
            users: Lru::new(MAX_USERS),
            members: Lru::new(MAX_MEMBERS),
            presences: Lru::new(MAX_PRESENCES),
            dm_by_user: Lru::new(MAX_DMS),
            dm_peer: Lru::new(MAX_DMS),
            profiles: ProfileTables::default(),
            seen: HashSet::new(),
            watched: HashSet::new(),
            kept: HashSet::new(),
            presence_rev: 0,
        }
    }
}

impl People {
    // ------------------------------------------------------------ écriture (réseau)

    /// Renvoie vrai si l'utilisateur est affiché et que ce qu'on en sait a changé.
    pub fn upsert_user(&mut self, u: UserInfo) -> bool {
        let id = u.id;
        let changed = match self.users.get_mut(&id) {
            Some(old) => {
                let before = old.clone();
                old.merge(u);
                *old != before
            }
            None => {
                let watched = &self.watched;
                self.users.insert(id, u, |k| watched.contains(k));
                true
            }
        };
        changed && self.watched.contains(&id)
    }

    /// Rôles d'un membre vus dans la liste des membres (lot P : couleur des
    /// noms dans la discussion). Le reste de ce qu'on sait du membre est gardé.
    pub fn note_roles(&mut self, guild_id: u64, user_id: u64, roles: &[u64]) {
        match self.members.get_mut(&(guild_id, user_id)) {
            Some(m) if m.roles.as_deref() == Some(roles) => {}
            Some(m) => m.roles = Some(roles.to_vec()),
            None => {
                let watched = &self.watched;
                self.members.insert((guild_id, user_id), MemberInfo { roles: Some(roles.to_vec()), ..Default::default() }, |(_, u)| watched.contains(u));
            }
        }
    }

    /// Renvoie vrai si l'utilisateur est affiché et que son membre a changé.
    pub fn upsert_member(&mut self, guild_id: u64, user_id: u64, m: MemberInfo) -> bool {
        let changed = match self.members.get_mut(&(guild_id, user_id)) {
            Some(old) if *old == m => false,
            Some(old) => {
                *old = m;
                true
            }
            None => {
                let watched = &self.watched;
                self.members.insert((guild_id, user_id), m, |(_, u)| watched.contains(u));
                true
            }
        };
        changed && self.watched.contains(&user_id)
    }

    /// Renvoie vrai si l'utilisateur est affiché et que sa présence a changé :
    /// c'est le seul cas où un PRESENCE_UPDATE réveille l'interface.
    pub fn set_presence(&mut self, user_id: u64, p: Presence) -> bool {
        let changed = match self.presences.get_mut(&user_id) {
            Some(old) if *old == p => false,
            Some(old) => {
                *old = p;
                true
            }
            None => {
                let (watched, kept) = (&self.watched, &self.kept);
                self.presences.insert(user_id, p, |k| watched.contains(k) || kept.contains(k));
                true
            }
        };
        if changed {
            self.presence_rev = self.presence_rev.wrapping_add(1);
        }
        changed && self.watched.contains(&user_id)
    }

    /// MP à deux avec `user_id`.
    pub fn note_dm(&mut self, user_id: u64, channel_id: u64) {
        self.dm_by_user.insert(user_id, channel_id, |_| false);
        self.dm_peer.insert(channel_id, user_id, |_| false);
    }

    pub fn profiles_mut(&mut self) -> &mut ProfileTables {
        &mut self.profiles
    }

    // ------------------------------------------------------------ lecture (interface)

    pub fn user(&self, id: u64) -> Option<&UserInfo> {
        self.users.get(&id)
    }

    /// Membres connus de ce serveur qui le boostent : (utilisateur, début du boost).
    pub fn boosters(&self, guild_id: u64) -> Vec<(u64, i64)> {
        self.members.map.iter().filter(|((g, _), _)| *g == guild_id).filter_map(|((_, u), (m, _))| Some((*u, m.premium_since?))).collect()
    }

    pub fn member(&self, guild_id: u64, user_id: u64) -> Option<&MemberInfo> {
        self.members.get(&(guild_id, user_id))
    }

    /// Membres connus d'un serveur, dans un ordre quelconque (pages
    /// d'administration, lot A3 : parcours de la table entière, à ne pas
    /// refaire à chaque image).
    pub fn members_of(&self, guild_id: u64) -> impl Iterator<Item = (u64, &MemberInfo)> + '_ {
        self.members.map.iter().filter(move |((g, _), _)| *g == guild_id).map(|((_, u), (m, _))| (*u, m))
    }

    /// Présence connue ; l'utilisateur compte comme affiché à cette image.
    pub fn presence(&mut self, user_id: u64) -> Option<&Presence> {
        self.seen.insert(user_id);
        self.presences.get(&user_id)
    }

    /// Statut connu ; l'utilisateur compte comme affiché à cette image.
    pub fn status(&mut self, user_id: u64) -> Option<Status> {
        self.presence(user_id).map(|p| p.status)
    }

    /// L'utilisateur est affiché à cette image (carte de profil ouverte…).
    pub fn watch(&mut self, user_id: u64) {
        self.seen.insert(user_id);
    }

    pub fn profiles(&self) -> &ProfileTables {
        &self.profiles
    }

    pub fn dm_with(&self, user_id: u64) -> Option<u64> {
        self.dm_by_user.get(&user_id).copied()
    }

    /// Correspondant d'un MP à deux.
    pub fn dm_peer(&self, channel_id: u64) -> Option<u64> {
        self.dm_peer.get(&channel_id).copied()
    }

    /// Fin d'une image : ce qui a été lu devient ce qui est affiché.
    pub fn end_frame(&mut self) {
        if self.seen != self.watched {
            std::mem::swap(&mut self.seen, &mut self.watched);
        }
        self.seen.clear();
    }

    /// Amis dont la présence est gardée hors éviction (remplace la liste).
    pub fn keep_presences(&mut self, ids: impl IntoIterator<Item = u64>) {
        self.kept = ids.into_iter().collect();
    }

    /// Statut connu, sans compter l'utilisateur comme affiché (filtre d'une liste).
    pub fn peek_status(&self, user_id: u64) -> Option<Status> {
        self.presences.get(&user_id).map(|p| p.status)
    }

    pub fn presence_rev(&self) -> u64 {
        self.presence_rev
    }

    pub fn is_watched(&self, user_id: u64) -> bool {
        self.watched.contains(&user_id)
    }

    /// (utilisateurs, membres, présences) gardés.
    pub fn sizes(&self) -> (usize, usize, usize) {
        (self.users.len(), self.members.len(), self.presences.len())
    }

    fn clear(&mut self) {
        self.users.clear();
        self.members.clear();
        self.presences.clear();
        self.dm_by_user.clear();
        self.dm_peer.clear();
        self.profiles.clear();
        self.seen.clear();
        self.watched.clear();
        self.kept.clear();
    }
}

/// [`People`] partagé entre le fil du réseau et celui de l'interface. Chaque
/// connexion crée le sien et le remet au magasin (`Event::PeopleHub`) : rien
/// ne survit d'un compte à l'autre.
#[derive(Clone, Default)]
pub struct PeopleHub(Arc<Mutex<People>>);

impl PeopleHub {
    /// Un verrou empoisonné (panique d'un autre fil pendant une écriture) ne
    /// rend pas les données dangereuses : au pire une entrée à moitié à jour.
    pub fn lock(&self) -> MutexGuard<'_, People> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn same(&self, other: &PeopleHub) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }

    /// Oublie tout (déconnexion).
    pub fn clear(&self) {
        self.lock().clear();
    }
}

impl fmt::Debug for PeopleHub {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (u, m, p) = self.lock().sizes();
        write!(f, "PeopleHub {{ utilisateurs: {u}, membres: {m}, présences: {p} }}")
    }
}

/// Texte reçu ramené à [`MAX_TEXT`] caractères.
pub fn clip(s: &str) -> String {
    match s.char_indices().nth(MAX_TEXT) {
        Some((i, _)) => s[..i].to_string(),
        None => s.to_string(),
    }
}

impl Store {
    /// « Envoyer un message » sur une carte de profil : ouvre le MP existant,
    /// sinon demande sa création (`POST /users/@me/channels`, geste de
    /// l'utilisateur ; Discord rend le MP existant s'il y en a un).
    pub fn message_user(&mut self, user_id: u64) {
        let known = self.people.lock().dm_with(user_id);
        match known.filter(|c| self.dms.iter().any(|d| d.id == *c)) {
            Some(c) => self.dm_opened(c),
            None => self.push_command(Cmd::OpenDm { user_id }),
        }
    }

    /// Ouverture d'une carte : profil complet, une fois par utilisateur et
    /// par serveur, redemandé si la carte est rouverte plus de
    /// `profiles::PROFILE_TTL_MS` après (jamais en masse).
    pub fn want_profile(&mut self, user_id: u64, guild_id: Option<u64>) {
        let now = self.now();
        if self.people.lock().profiles_mut().begin(user_id, guild_id, now) {
            self.push_command(Cmd::Profile(ProfileCmd::Fetch { user_id, guild_id }));
        }
    }

    /// MP créé ou retrouvé : il devient la conversation affichée.
    pub(super) fn dm_opened(&mut self, channel_id: u64) {
        if !self.dms.iter().any(|d| d.id == channel_id) {
            return;
        }
        self.select_dms();
        self.select_channel(channel_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::Dm;
    use crate::state::Event;

    fn user(id: u64, name: &str) -> UserInfo {
        UserInfo {
            id,
            username: name.into(),
            global_name: None,
            discriminator: None,
            avatar: Avatar::default_for(id),
            banner: None,
            accent: None,
            flags: None,
            bot: false,
            system: false,
            look: Default::default(),
        }
    }

    fn online() -> Presence {
        Presence { status: Status::Online, client: ClientStatus { desktop: Some(Status::Online), ..Default::default() }, activities: Vec::new() }
    }

    #[test]
    fn status_values() {
        assert_eq!(Status::parse("online"), Some(Status::Online));
        assert_eq!(Status::parse("idle"), Some(Status::Idle));
        assert_eq!(Status::parse("dnd"), Some(Status::Dnd));
        assert_eq!(Status::parse("offline"), Some(Status::Offline));
        assert_eq!(Status::parse("invisible"), Some(Status::Offline));
        assert_eq!(Status::parse("ONLINE"), None);
        assert_eq!(ActivityKind::from_code(4), Some(ActivityKind::Custom));
        assert_eq!(ActivityKind::from_code(6), None);
    }

    #[test]
    fn badges_follow_documented_bits() {
        assert_eq!(Badge::of(0), Vec::new());
        assert_eq!(Badge::of(1 << 22 | 1 << 6), vec![Badge::HypesquadBravery, Badge::ActiveDeveloper]);
        // Bits non documentés (ou retirés de l'affichage) : ignorés.
        assert_eq!(Badge::of(1 << 10 | 1 << 19 | 1 << 40), Vec::new());
    }

    #[test]
    fn partial_user_keeps_what_was_known() {
        let mut p = People::default();
        let mut full = user(1, "alice");
        full.banner = Some(Some(ImageHash::synthetic(1)));
        full.accent = Some(Some(0x112233));
        full.flags = Some(4);
        p.upsert_user(full);
        let mut later = user(1, "alice2");
        later.global_name = Some("Alice".into());
        p.upsert_user(later);
        let u = p.user(1).cloned().unwrap_or_else(|| panic!("utilisateur attendu"));
        assert_eq!((u.username.as_str(), u.display_name()), ("alice2", "Alice"));
        assert_eq!((u.banner, u.accent, u.flags), (Some(Some(ImageHash::synthetic(1))), Some(Some(0x112233)), Some(4)));
        // Bannière retirée (null) : connue comme absente.
        let mut removed = user(1, "alice2");
        removed.banner = Some(None);
        p.upsert_user(removed);
        assert_eq!(p.user(1).and_then(|u| u.banner), Some(None));
        let mut old = user(2, "bob");
        old.discriminator = Some("0042".into());
        assert_eq!(old.handle(), "bob#0042");
        assert_eq!(user(3, "eve").handle(), "eve");
    }

    #[test]
    fn roles_seen_in_the_member_list_keep_the_rest() {
        let mut p = People::default();
        // Membre inconnu : créé avec ses seuls rôles.
        p.note_roles(9, 1, &[3, 4]);
        assert_eq!(p.member(9, 1).and_then(|m| m.roles.clone()), Some(vec![3, 4]));
        // Membre connu : pseudo gardé, rôles remplacés.
        p.upsert_member(9, 2, MemberInfo { nick: Some("Ali".into()), roles: Some(vec![1]), ..Default::default() });
        p.note_roles(9, 2, &[5]);
        let m = p.member(9, 2).cloned().unwrap_or_default();
        assert_eq!((m.nick.as_deref(), m.roles), (Some("Ali"), Some(vec![5])));
    }

    /// Un objet `user` partiel (sans parures) ne retire pas celles qu'on connaît.
    #[test]
    fn partial_user_keeps_known_look() {
        use crate::state::cosmetics::{Asset, Look};
        let mut p = People::default();
        let asset = Asset::decoration("dec_test");
        assert!(asset.is_some());
        p.upsert_user(UserInfo { look: Look { decoration: Some(asset), ..Default::default() }, ..user(1, "a") });
        p.upsert_user(user(1, "a"));
        assert_eq!(p.user(1).and_then(|u| u.look.decoration()), asset);
        // `null` explicite : retirée.
        p.upsert_user(UserInfo { look: Look { decoration: Some(None), ..Default::default() }, ..user(1, "a") });
        assert_eq!(p.user(1).and_then(|u| u.look.decoration()), None);
    }

    #[test]
    fn only_watched_changes_ask_for_a_redraw() {
        let mut p = People::default();
        assert!(!p.set_presence(1, online()), "personne n'est affiché");
        // Lu pendant une image : affiché à partir de la fin de l'image.
        assert_eq!(p.status(1), Some(Status::Online));
        assert!(!p.is_watched(1));
        p.end_frame();
        assert!(p.is_watched(1));
        assert!(!p.set_presence(1, online()), "rien n'a changé");
        let idle = Presence { status: Status::Idle, ..online() };
        assert!(p.set_presence(1, idle.clone()));
        assert!(!p.set_presence(2, idle), "utilisateur non affiché");
        assert!(p.upsert_user(user(1, "a")));
        assert!(!p.upsert_user(user(1, "a")));
        assert!(!p.upsert_user(user(2, "b")));
        assert!(p.upsert_member(9, 1, MemberInfo::default()));
        assert!(!p.upsert_member(9, 1, MemberInfo::default()));
        // Image suivante sans lui : il n'est plus affiché.
        p.end_frame();
        assert!(!p.is_watched(1));
        assert!(!p.set_presence(1, online()));
    }

    #[test]
    fn tables_are_bounded_and_spare_watched_users() {
        let mut p = People::default();
        p.watch(7);
        p.end_frame();
        p.set_presence(7, online());
        for id in 100..100 + 3 * MAX_PRESENCES as u64 {
            p.set_presence(id, online());
            p.upsert_user(user(id, "x"));
            p.upsert_member(1, id, MemberInfo::default());
        }
        let (u, m, n) = p.sizes();
        assert!(u <= MAX_USERS && m <= MAX_MEMBERS && n <= MAX_PRESENCES, "{u} {m} {n}");
        assert!(n >= MAX_PRESENCES - MAX_PRESENCES / 8);
        assert!(p.presences.get(&7).is_some(), "utilisateur affiché gardé");
        // Les plus récents restent, les plus anciens partent.
        let last = 100 + 3 * MAX_PRESENCES as u64 - 1;
        assert!(p.presences.get(&last).is_some() && p.presences.get(&100).is_none());
    }

    #[test]
    fn clip_counts_characters() {
        assert_eq!(clip("abc"), "abc");
        let long = "é".repeat(MAX_TEXT + 10);
        assert_eq!(clip(&long).chars().count(), MAX_TEXT);
    }

    #[test]
    fn store_opens_known_dm_or_asks_for_one() {
        let mut s = Store::new();
        let hub = PeopleHub::default();
        s.apply(Event::PeopleHub(hub.clone()));
        assert!(s.people.same(&hub));
        s.dms.push(Dm::new(40, "alice".into(), 40));
        hub.lock().note_dm(5, 40);
        s.message_user(5);
        assert_eq!((s.guild, s.channel), (None, Some(40)));
        assert!(s.take_commands().iter().all(|c| !matches!(c, Cmd::OpenDm { .. })));
        s.message_user(6);
        assert_eq!(s.take_commands(), vec![Cmd::OpenDm { user_id: 6 }]);
        // MP créé par Discord : il est ouvert.
        s.dms.push(Dm::new(41, "bob".into(), 41));
        s.apply(Event::DmOpened { channel_id: 41 });
        assert_eq!(s.channel, Some(41));
        // MP inconnu : rien ne change.
        s.apply(Event::DmOpened { channel_id: 99 });
        assert_eq!(s.channel, Some(41));
    }

    #[test]
    fn store_asks_each_profile_once() {
        let mut s = Store::new();
        let fetch = |guild_id| Cmd::Profile(ProfileCmd::Fetch { user_id: 5, guild_id });
        s.want_profile(5, Some(9));
        assert_eq!(s.take_commands(), vec![fetch(Some(9))]);
        s.want_profile(5, Some(9));
        assert!(s.take_commands().is_empty());
        s.want_profile(5, None);
        s.want_profile(5, Some(10));
        assert_eq!(s.take_commands(), vec![fetch(None), fetch(Some(10))]);
    }
}
