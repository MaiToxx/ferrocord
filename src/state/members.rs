//! Liste des membres (panneau de droite) : listes de serveur reçues par
//! GUILD_MEMBER_LIST_UPDATE après une souscription (comptes utilisateur), et
//! destinataires des MP de groupe.
//!
//! Formes et règles (non documentées par Discord, voir
//! `docs/passerelle-utilisateur.md`) : une liste est une suite plate où chaque
//! groupe (rôle affiché à part, « en ligne », « hors ligne ») occupe un index,
//! suivi de ses membres. On ne souscrit qu'à des plages de 100 index, au plus
//! trois, dont toujours `[0, 99]` ; seules ces plages sont gardées en mémoire.
//!
//! L'état de présence gardé ici est celui que donne la liste (statut et
//! activité de chaque ligne) : ce n'est pas un magasin de présence général.

use crate::i18n::{f, t, Lang};
use super::model::{Avatar, Guild, Role, User};
use super::store::Store;
use super::Cmd;
use std::{
    collections::{BTreeMap, HashMap},
    ops::Range,
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
};

/// Taille d'une plage souscrite (index), comme le client officiel.
pub const RANGE: u32 = 100;
/// Plages souscrites par salon, au plus (`[0, 99]` compris).
pub const MAX_RANGES: usize = 3;
/// Listes gardées en mémoire (toutes serveurs confondus) ; au-delà, la moins
/// récemment touchée est oubliée.
pub const MAX_LISTS: usize = 4;
/// Liens salon → liste retenus ; au-delà, ils sont tous oubliés (ils se
/// réapprennent à la souscription suivante).
pub const MAX_CHANNEL_LINKS: usize = 256;
/// MP de groupe dont les destinataires sont gardés ; au-delà, le plus ancien
/// arrivé est oublié.
pub const MAX_GROUP_DMS: usize = 200;
/// Mises à jour de liste reçues sans réveiller l'interface, au plus : la
/// suivante la réveille (les événements en attente ne s'accumulent pas sans fin).
pub const QUIET_FLUSH: u32 = 256;

/// Groupe d'une liste : rôle affiché à part, ou en ligne / hors ligne.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GroupId {
    Role(u64),
    Online,
    Offline,
}

/// Statut tel que le donne la liste.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MemberStatus {
    Online,
    Idle,
    Dnd,
    #[default]
    Offline,
}

impl MemberStatus {
    /// Valeur de `presence.status` (documentée : online, idle, dnd, offline ;
    /// « invisible » n'est jamais vu des autres et vaut hors ligne).
    pub fn parse(s: &str) -> Self {
        match s {
            "online" => Self::Online,
            "idle" => Self::Idle,
            "dnd" => Self::Dnd,
            _ => Self::Offline,
        }
    }
}

/// Types d'activité documentés (Activity Types : 0 à 5).
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
    pub fn from_code(code: u8) -> Option<Self> {
        Some(match code {
            0 => Self::Playing,
            1 => Self::Streaming,
            2 => Self::Listening,
            3 => Self::Watching,
            4 => Self::Custom,
            5 => Self::Competing,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub name: String,
    /// Statut personnalisé : son texte.
    pub state: Option<String>,
}

/// Une ligne de membre. `color` et `line` sont calculés par le magasin à la
/// réception (rôle le plus haut, activité dans la langue courante).
#[derive(Clone, Debug, PartialEq)]
pub struct ListMember {
    pub user_id: u64,
    pub name: String,
    pub avatar: Avatar,
    pub roles: Vec<u64>,
    pub status: MemberStatus,
    pub activity: Option<Activity>,
    pub bot: bool,
    /// 0xRRGGBB du rôle coloré le plus haut, 0 = aucun.
    pub color: u32,
    /// Activité en une ligne, déjà mise en mots.
    pub line: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ListItem {
    Group(GroupId),
    Member(ListMember),
    /// Élément illisible : sa place compte, la ligne reste en attente.
    Unknown,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ListOp {
    Sync { range: (u32, u32), items: Vec<ListItem> },
    Insert { index: u32, item: ListItem },
    Update { index: u32, item: ListItem },
    Delete { index: u32 },
    Invalidate { range: (u32, u32) },
    /// Opération illisible : on ne sait plus ce qui a bougé, tout est oublié.
    Unreadable,
}

/// Un paquet GUILD_MEMBER_LIST_UPDATE décodé.
#[derive(Clone, Debug, PartialEq)]
pub struct ListUpdate {
    pub guild_id: u64,
    /// `"everyone"` ou une empreinte des permissions du salon.
    pub id: String,
    pub member_count: u32,
    pub online_count: u32,
    pub groups: Vec<(GroupId, u32)>,
    pub ops: Vec<ListOp>,
}

impl ListUpdate {
    /// Index touchés par le paquet (bornes incluses) : une insertion ou une
    /// suppression décale tout ce qui suit.
    pub fn span(&self) -> Option<(u32, u32)> {
        self.ops.iter().fold(None, |acc, op| {
            let (lo, hi) = match op {
                ListOp::Sync { range, .. } | ListOp::Invalidate { range } => *range,
                ListOp::Update { index, .. } => (*index, *index),
                ListOp::Insert { index, .. } | ListOp::Delete { index } => (*index, u32::MAX),
                ListOp::Unreadable => (0, u32::MAX),
            };
            Some(match acc {
                None => (lo, hi),
                Some((a, b)) => (a.min(lo), b.max(hi)),
            })
        })
    }
}

/// Ce que l'interface veut recevoir : un serveur, et la liste d'un de ses
/// salons avec ses plages (`None` : plus aucune liste de ce serveur).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subscription {
    pub guild_id: u64,
    pub channel: Option<(u64, Vec<(u32, u32)>)>,
}

/// Plages à souscrire pour les lignes visibles : toujours `[0, 99]`, puis les
/// plages de 100 qui couvrent la vue, trois au plus.
pub fn ranges_for(visible: Option<Range<u32>>) -> Vec<(u32, u32)> {
    let mut out = vec![(0, RANGE - 1)];
    if let Some(v) = visible.filter(|v| v.end > v.start) {
        for chunk in v.start / RANGE..=(v.end - 1) / RANGE {
            if out.len() >= MAX_RANGES {
                break;
            }
            if chunk > 0 {
                let lo = chunk.saturating_mul(RANGE);
                out.push((lo, lo.saturating_add(RANGE - 1)));
            }
        }
    }
    out
}

/// Titre de groupe, à son index, avec son texte rendu.
#[derive(Clone, Debug, PartialEq)]
pub struct Header {
    pub index: u32,
    pub group: GroupId,
    pub label: String,
}

/// Ce qu'il y a à un index de la liste.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Row<'a> {
    Header(&'a Header),
    Member(&'a ListMember),
    /// Hors des plages reçues : ligne d'attente.
    Pending,
}

#[derive(Clone, Debug)]
pub struct MemberList {
    pub guild_id: u64,
    pub id: String,
    pub member_count: u32,
    pub online_count: u32,
    groups: Vec<(GroupId, u32)>,
    headers: Vec<Header>,
    /// Lignes reçues, par index ; seulement dans `ranges`.
    rows: BTreeMap<u32, ListItem>,
    len: u32,
    ranges: Vec<(u32, u32)>,
    /// Change à chaque modification.
    pub rev: u64,
    /// Change quand la forme (longueur, titres) change.
    pub shape: u64,
    touched: u64,
}

impl MemberList {
    fn new(guild_id: u64, id: String) -> Self {
        MemberList {
            guild_id,
            id,
            member_count: 0,
            online_count: 0,
            groups: Vec::new(),
            headers: Vec::new(),
            rows: BTreeMap::new(),
            len: 0,
            ranges: ranges_for(None),
            rev: 0,
            shape: 0,
            touched: 0,
        }
    }

    /// Nombre d'index (titres compris).
    pub fn len(&self) -> u32 {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn headers(&self) -> &[Header] {
        &self.headers
    }

    /// Lignes de membres gardées en mémoire.
    pub fn loaded(&self) -> usize {
        self.rows.values().filter(|r| matches!(r, ListItem::Member(_))).count()
    }

    pub fn row(&self, index: u32) -> Row<'_> {
        match self.rows.get(&index) {
            Some(ListItem::Member(m)) => Row::Member(m),
            Some(ListItem::Group(g)) => match self.headers.iter().find(|h| h.group == *g) {
                Some(h) => Row::Header(h),
                None => Row::Pending,
            },
            Some(ListItem::Unknown) => Row::Pending,
            None => match self.headers.binary_search_by_key(&index, |h| h.index) {
                Ok(i) => Row::Header(&self.headers[i]),
                Err(_) => Row::Pending,
            },
        }
    }

    /// Index est-il un titre (reçu, ou calculé à partir des groupes) ?
    pub fn is_header(&self, index: u32) -> bool {
        matches!(self.row(index), Row::Header(_))
    }

    /// Titres et longueur tirés des groupes. Un groupe vide n'occupe aucun index.
    fn relayout(&mut self, roles: &[Role], lang: Lang) {
        let mut headers = Vec::with_capacity(self.groups.len());
        let mut i: u32 = 0;
        for (g, count) in self.groups.iter().filter(|(_, c)| *c > 0) {
            headers.push(Header { index: i, group: *g, label: group_label(*g, *count, roles, lang) });
            i = i.saturating_add(1).saturating_add(*count);
        }
        if headers != self.headers || i != self.len {
            self.headers = headers;
            self.len = i;
            self.shape = self.shape.wrapping_add(1);
        }
    }

    fn shift_from(&mut self, index: u32, up: bool) {
        let tail = self.rows.split_off(&index);
        for (k, v) in tail {
            let k = if up { k.saturating_add(1) } else { k.saturating_sub(1) };
            self.rows.insert(k, v);
        }
    }

    fn apply_op(&mut self, op: ListOp, roles: &[Role], lang: Lang) {
        match op {
            ListOp::Sync { range: (lo, hi), items } => {
                self.clear_range(lo, hi);
                for (k, item) in items.into_iter().enumerate() {
                    let Ok(k) = u32::try_from(k) else { break };
                    let index = lo.saturating_add(k);
                    if index > hi {
                        break;
                    }
                    self.rows.insert(index, finish(item, roles, lang));
                }
            }
            ListOp::Insert { index, item } => {
                self.shift_from(index, true);
                self.rows.insert(index, finish(item, roles, lang));
            }
            ListOp::Update { index, item } => {
                self.rows.insert(index, finish(item, roles, lang));
            }
            ListOp::Delete { index } => {
                self.rows.remove(&index);
                self.shift_from(index.saturating_add(1), false);
            }
            ListOp::Invalidate { range: (lo, hi) } => self.clear_range(lo, hi),
            ListOp::Unreadable => self.rows.clear(),
        }
    }

    fn clear_range(&mut self, lo: u32, hi: u32) {
        if lo > hi {
            return;
        }
        let mut tail = self.rows.split_off(&lo);
        if let Some(after) = hi.checked_add(1) {
            let mut rest = tail.split_off(&after);
            self.rows.append(&mut rest);
        }
        tail.clear();
    }

    /// Éviction : ce qui sort des plages souscrites est oublié.
    fn prune(&mut self) {
        let ranges = &self.ranges;
        self.rows.retain(|k, _| ranges.iter().any(|(lo, hi)| (lo..=hi).contains(&k)));
    }

    fn relabel(&mut self, roles: &[Role], lang: Lang) {
        for item in self.rows.values_mut() {
            if let ListItem::Member(m) = item {
                m.line = activity_line(m.activity.as_ref(), lang);
                m.color = top_color(&m.roles, roles);
            }
        }
        self.headers.clear();
        self.relayout(roles, lang);
        self.rev = self.rev.wrapping_add(1);
    }
}

fn group_label(g: GroupId, count: u32, roles: &[Role], lang: Lang) -> String {
    let name = match g {
        GroupId::Online => t::MEMBERS_ONLINE.get(lang).to_string(),
        GroupId::Offline => t::MEMBERS_OFFLINE.get(lang).to_string(),
        GroupId::Role(id) => match roles.iter().find(|r| r.id == id) {
            Some(r) if !r.name.is_empty() => r.name.clone(),
            _ => t::UNKNOWN_ROLE.get(lang).to_string(),
        },
    };
    f::member_group(lang, name.to_uppercase(), count)
}

/// Couleur du rôle coloré le plus haut parmi `mine`.
pub fn top_color(mine: &[u64], roles: &[Role]) -> u32 {
    roles
        .iter()
        .filter(|r| r.color != 0 && mine.contains(&r.id))
        .max_by_key(|r| (r.position, std::cmp::Reverse(r.id)))
        .map_or(0, |r| r.color)
}

/// Statut personnalisé s'il a un texte, sinon la première autre activité.
pub fn activity_line(a: Option<&Activity>, lang: Lang) -> Option<String> {
    let a = a?;
    Some(match a.kind {
        ActivityKind::Custom => a.state.clone().filter(|s| !s.trim().is_empty())?,
        _ if a.name.trim().is_empty() => return None,
        ActivityKind::Playing => f::activity_playing(lang, &a.name),
        ActivityKind::Streaming => f::activity_streaming(lang, &a.name),
        ActivityKind::Listening => f::activity_listening(lang, &a.name),
        ActivityKind::Watching => f::activity_watching(lang, &a.name),
        ActivityKind::Competing => f::activity_competing(lang, &a.name),
    })
}

/// Choisit l'activité affichée parmi celles d'une présence.
pub fn pick_activity(all: Vec<Activity>) -> Option<Activity> {
    let custom = all.iter().position(|a| a.kind == ActivityKind::Custom && a.state.as_deref().is_some_and(|s| !s.trim().is_empty()));
    let other = all.iter().position(|a| a.kind != ActivityKind::Custom && !a.name.trim().is_empty());
    let i = custom.or(other)?;
    all.into_iter().nth(i)
}

fn finish(item: ListItem, roles: &[Role], lang: Lang) -> ListItem {
    match item {
        ListItem::Member(mut m) => {
            m.color = top_color(&m.roles, roles);
            m.line = activity_line(m.activity.as_ref(), lang);
            ListItem::Member(m)
        }
        g => g,
    }
}

/// Ce que l'interface affiche de la liste, lu par la passerelle pour décider
/// si une mise à jour de liste mérite un réveil (règle 1.3 : aucune image
/// pour une liste masquée ou une plage qui n'est pas à l'écran).
#[derive(Debug, Default)]
pub struct ListWatch {
    /// Faux tant que l'interface ne filtre pas : tout réveille (tests, outils).
    filter: AtomicBool,
    /// Serveur dont la liste est à l'écran (0 : aucune).
    guild: AtomicU64,
    lo: AtomicU32,
    hi: AtomicU32,
    quiet: AtomicU32,
}

impl ListWatch {
    /// À appeler à chaque image : liste à l'écran (serveur, index visibles), ou rien.
    pub fn show(&self, shown: Option<(u64, Range<u32>)>) {
        self.filter.store(true, Ordering::Relaxed);
        match shown {
            Some((g, r)) => {
                self.lo.store(r.start, Ordering::Relaxed);
                self.hi.store(r.end, Ordering::Relaxed);
                self.guild.store(g, Ordering::Relaxed);
            }
            None => self.guild.store(0, Ordering::Relaxed),
        }
    }

    /// Faut-il réveiller l'interface pour cette mise à jour ?
    pub fn wants(&self, u: &ListUpdate) -> bool {
        if !self.filter.load(Ordering::Relaxed) {
            return true;
        }
        let g = self.guild.load(Ordering::Relaxed);
        let (lo, hi) = (self.lo.load(Ordering::Relaxed), self.hi.load(Ordering::Relaxed));
        let seen = g != 0 && g == u.guild_id && u.span().is_some_and(|(a, b)| a < hi && b >= lo);
        if seen || self.quiet.fetch_add(1, Ordering::Relaxed) + 1 >= QUIET_FLUSH {
            self.quiet.store(0, Ordering::Relaxed);
            return true;
        }
        false
    }
}

/// Listes des membres et destinataires des MP de groupe.
#[derive(Debug, Default)]
pub struct Members {
    lists: Vec<MemberList>,
    /// Liste de chaque salon, apprise à la première SYNC qui suit sa souscription.
    links: HashMap<u64, String>,
    /// Salon souscrit dont la liste n'est pas encore connue.
    awaiting: Option<u64>,
    /// Dernière souscription demandée au réseau.
    wanted: Option<Subscription>,
    /// Destinataires des MP de groupe (moi non compris), par salon.
    recipients: HashMap<u64, Vec<User>>,
    recipient_order: Vec<u64>,
    /// Compte bot : Discord n'envoie pas ces listes (les bots ont les intents).
    pub unavailable: bool,
    watch: Arc<ListWatch>,
    clock: u64,
}

impl Members {
    pub fn watch(&self) -> Arc<ListWatch> {
        self.watch.clone()
    }

    /// Déconnexion ou nouvelle session : rien n'est plus souscrit.
    pub fn clear(&mut self) {
        let watch = self.watch.clone();
        *self = Members { watch, ..Default::default() };
    }

    pub fn wanted(&self) -> Option<&Subscription> {
        self.wanted.as_ref()
    }

    /// Liste à afficher pour ce salon : celle qui lui est liée, sinon la
    /// plus récente du serveur (le temps que la première SYNC arrive).
    pub fn list_for(&self, guild_id: u64, channel_id: u64) -> Option<&MemberList> {
        match self.links.get(&channel_id) {
            Some(id) => self.lists.iter().find(|l| l.guild_id == guild_id && l.id == *id),
            None => self.lists.iter().filter(|l| l.guild_id == guild_id).max_by_key(|l| l.touched),
        }
    }

    pub fn recipients(&self, channel_id: u64) -> Option<&[User]> {
        self.recipients.get(&channel_id).map(Vec::as_slice)
    }

    pub fn set_recipients(&mut self, channel_id: u64, users: Vec<User>) {
        if !self.recipients.contains_key(&channel_id) {
            if self.recipient_order.len() >= MAX_GROUP_DMS && !self.recipient_order.is_empty() {
                let old = self.recipient_order.remove(0);
                self.recipients.remove(&old);
            }
            self.recipient_order.push(channel_id);
        }
        self.recipients.insert(channel_id, users);
    }

    /// Nouvelle souscription voulue ; `Some` si elle diffère de la précédente
    /// (à envoyer au réseau).
    pub fn want(&mut self, sub: Subscription) -> Option<Subscription> {
        if self.wanted.as_ref() == Some(&sub) {
            return None;
        }
        let channel = sub.channel.as_ref().map(|(c, _)| *c);
        if channel != self.wanted.as_ref().and_then(|w| w.channel.as_ref()).map(|(c, _)| *c) {
            self.awaiting = channel.filter(|c| !self.links.contains_key(c));
        }
        if let Some((c, ranges)) = &sub.channel
            && let Some(l) = self.links.get(c).cloned().and_then(|id| self.lists.iter_mut().find(|l| l.guild_id == sub.guild_id && l.id == id))
        {
            l.ranges = ranges.clone();
            l.prune();
        }
        self.wanted = Some(sub.clone());
        Some(sub)
    }

    pub fn apply(&mut self, u: ListUpdate, guilds: &[Guild], lang: Lang) {
        let roles: &[Role] = guilds.iter().find(|g| g.id == u.guild_id).map_or(&[], |g| g.roles.as_slice());
        let wanted = self.wanted.as_ref().filter(|w| w.guild_id == u.guild_id).and_then(|w| w.channel.clone());
        if let (Some(ch), Some((wc, _))) = (self.awaiting, &wanted)
            && ch == *wc
            && u.ops.iter().any(|o| matches!(o, ListOp::Sync { .. }))
        {
            if self.links.len() >= MAX_CHANNEL_LINKS {
                self.links.clear();
            }
            self.links.insert(ch, u.id.clone());
            self.awaiting = None;
        }
        self.clock += 1;
        let i = match self.lists.iter().position(|l| l.guild_id == u.guild_id && l.id == u.id) {
            Some(i) => i,
            None => {
                if self.lists.len() >= MAX_LISTS
                    && let Some(old) = self.lists.iter().enumerate().min_by_key(|(_, l)| l.touched).map(|(i, _)| i)
                {
                    let gone = self.lists.swap_remove(old);
                    self.links.retain(|_, id| *id != gone.id);
                }
                self.lists.push(MemberList::new(u.guild_id, u.id.clone()));
                self.lists.len() - 1
            }
        };
        let linked = wanted.as_ref().and_then(|(c, r)| (self.links.get(c) == Some(&u.id)).then(|| r.clone()));
        let l = &mut self.lists[i];
        l.touched = self.clock;
        if let Some(r) = linked {
            l.ranges = r;
        }
        l.member_count = u.member_count;
        l.online_count = u.online_count;
        l.groups = u.groups;
        for op in u.ops {
            l.apply_op(op, roles, lang);
        }
        l.relayout(roles, lang);
        l.prune();
        l.rev = l.rev.wrapping_add(1);
    }

    /// Langue changée, ou rôles changés : textes et couleurs refaits une fois.
    pub fn relabel(&mut self, guilds: &[Guild], lang: Lang) {
        for l in &mut self.lists {
            let roles: &[Role] = guilds.iter().find(|g| g.id == l.guild_id).map_or(&[], |g| g.roles.as_slice());
            l.relabel(roles, lang);
        }
    }
}

impl Store {
    /// Souscription voulue par l'interface à cette image : liste du salon
    /// ouvert (`shown`) et lignes visibles. Une commande ne part que si elle
    /// change. Liste masquée, ou hors d'un serveur : la liste souscrite
    /// avant est quittée, et rien n'est demandé si aucune ne l'était.
    pub fn want_members(&mut self, shown: bool, visible: Option<Range<u32>>) {
        if self.members.unavailable {
            return;
        }
        let sub = match (self.guild, self.channel) {
            (Some(g), Some(c)) if shown => Subscription { guild_id: g, channel: Some((c, ranges_for(visible))) },
            _ => match self.members.wanted() {
                Some(w) if w.channel.is_some() => Subscription { guild_id: w.guild_id, channel: None },
                _ => return,
            },
        };
        if let Some(s) = self.members.want(sub) {
            self.push_command(Cmd::Subscribe(s));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Permissions;

    fn member(id: u64, name: &str) -> ListItem {
        ListItem::Member(ListMember {
            user_id: id,
            name: name.into(),
            avatar: Avatar::default_for(id),
            roles: Vec::new(),
            status: MemberStatus::Online,
            activity: None,
            bot: false,
            color: 0,
            line: None,
        })
    }

    fn names(l: &MemberList) -> Vec<(u32, String)> {
        (0..l.len())
            .map(|i| {
                (
                    i,
                    match l.row(i) {
                        Row::Header(h) => format!("[{:?}]", h.group),
                        Row::Member(m) => m.name.clone(),
                        Row::Pending => "…".into(),
                    },
                )
            })
            .collect()
    }

    fn update(ops: Vec<ListOp>, groups: Vec<(GroupId, u32)>) -> ListUpdate {
        ListUpdate { guild_id: 1, id: "everyone".into(), member_count: 10, online_count: 3, groups, ops }
    }

    fn subscribed(m: &mut Members, channel: u64) {
        m.want(Subscription { guild_id: 1, channel: Some((channel, ranges_for(None))) });
    }

    #[test]
    fn ranges_always_start_with_the_first_hundred() {
        assert_eq!(ranges_for(None), vec![(0, 99)]);
        assert_eq!(ranges_for(Some(0..30)), vec![(0, 99)]);
        assert_eq!(ranges_for(Some(180..230)), vec![(0, 99), (100, 199), (200, 299)]);
        assert_eq!(ranges_for(Some(450..470)), vec![(0, 99), (400, 499)]);
        // Une vue immense ne dépasse pas trois plages.
        assert_eq!(ranges_for(Some(0..1000)).len(), MAX_RANGES);
        assert_eq!(ranges_for(Some(5..5)), vec![(0, 99)]);
    }

    #[test]
    fn sync_insert_update_delete_invalidate() {
        let mut m = Members::default();
        subscribed(&mut m, 9);
        let groups = vec![(GroupId::Online, 2), (GroupId::Offline, 1)];
        let sync = ListOp::Sync {
            range: (0, 99),
            items: vec![ListItem::Group(GroupId::Online), member(1, "a"), member(2, "b"), ListItem::Group(GroupId::Offline), member(3, "c")],
        };
        m.apply(update(vec![sync], groups), &[], Lang::Fr);
        let l = m.list_for(1, 9).unwrap_or_else(|| panic!("liste"));
        assert_eq!(names(l).into_iter().map(|x| x.1).collect::<Vec<_>>(), ["[Online]", "a", "b", "[Offline]", "c"]);

        // « d » arrive en ligne : insertion à 2, tout ce qui suit descend.
        let ins = ListOp::Insert { index: 2, item: member(4, "d") };
        m.apply(update(vec![ins], vec![(GroupId::Online, 3), (GroupId::Offline, 1)]), &[], Lang::Fr);
        let l = m.list_for(1, 9).unwrap_or_else(|| panic!("liste"));
        assert_eq!(names(l).into_iter().map(|x| x.1).collect::<Vec<_>>(), ["[Online]", "a", "d", "b", "[Offline]", "c"]);

        let upd = ListOp::Update { index: 1, item: member(1, "a2") };
        let del = ListOp::Delete { index: 3 };
        m.apply(update(vec![upd, del], vec![(GroupId::Online, 2), (GroupId::Offline, 1)]), &[], Lang::Fr);
        let l = m.list_for(1, 9).unwrap_or_else(|| panic!("liste"));
        assert_eq!(names(l).into_iter().map(|x| x.1).collect::<Vec<_>>(), ["[Online]", "a2", "d", "[Offline]", "c"]);

        m.apply(update(vec![ListOp::Invalidate { range: (0, 99) }], vec![(GroupId::Online, 2), (GroupId::Offline, 1)]), &[], Lang::Fr);
        let l = m.list_for(1, 9).unwrap_or_else(|| panic!("liste"));
        // Titres toujours connus (tirés des groupes), membres en attente.
        assert_eq!(names(l).into_iter().map(|x| x.1).collect::<Vec<_>>(), ["[Online]", "…", "…", "[Offline]", "…"]);
        assert_eq!(l.loaded(), 0);
    }

    #[test]
    fn only_subscribed_ranges_are_kept() {
        let mut m = Members::default();
        m.want(Subscription { guild_id: 1, channel: Some((9, ranges_for(Some(150..170)))) });
        let groups = vec![(GroupId::Online, 499)];
        let items = |base: u64| (0..100).map(|i| member(base + i, "x")).collect::<Vec<_>>();
        m.apply(
            update(
                vec![
                    ListOp::Sync { range: (0, 99), items: items(0) },
                    ListOp::Sync { range: (100, 199), items: items(100) },
                    ListOp::Sync { range: (300, 399), items: items(300) },
                ],
                groups.clone(),
            ),
            &[],
            Lang::Fr,
        );
        let l = m.list_for(1, 9).unwrap_or_else(|| panic!("liste"));
        // La plage 300-399 n'est pas souscrite : rien n'en reste.
        assert_eq!(l.loaded(), 200);
        assert!(matches!(l.row(350), Row::Pending));
        // La vue redescend en haut : la plage 100-199 est quittée et oubliée.
        m.want(Subscription { guild_id: 1, channel: Some((9, ranges_for(Some(0..20)))) });
        assert_eq!(m.list_for(1, 9).map(MemberList::loaded), Some(100));
    }

    #[test]
    fn lists_are_bounded() {
        let mut m = Members::default();
        for g in 0..(MAX_LISTS as u64 + 3) {
            let mut u = update(vec![ListOp::Sync { range: (0, 99), items: vec![member(1, "a")] }], vec![(GroupId::Online, 1)]);
            u.guild_id = 100 + g;
            m.apply(u, &[], Lang::Fr);
        }
        assert_eq!(m.lists.len(), MAX_LISTS);
        assert!(m.list_for(100, 1).is_none(), "la plus ancienne est oubliée");
        assert!(m.list_for(100 + MAX_LISTS as u64 + 2, 1).is_some());
        for c in 0..(MAX_GROUP_DMS as u64 + 5) {
            m.set_recipients(c, Vec::new());
        }
        assert_eq!(m.recipients.len(), MAX_GROUP_DMS);
        assert!(m.recipients(0).is_none());
    }

    #[test]
    fn channel_learns_its_list_from_the_first_sync() {
        let mut m = Members::default();
        subscribed(&mut m, 9);
        let mut u = update(vec![ListOp::Sync { range: (0, 99), items: vec![ListItem::Group(GroupId::Online), member(1, "a")] }], vec![(GroupId::Online, 1)]);
        u.id = "123".into();
        m.apply(u, &[], Lang::Fr);
        // Autre liste du même serveur, reçue sans souscription en attente : pas liée à 9.
        let mut other = update(vec![ListOp::Sync { range: (0, 99), items: vec![ListItem::Group(GroupId::Online), member(2, "b")] }], vec![(GroupId::Online, 1)]);
        other.id = "everyone".into();
        m.apply(other, &[], Lang::Fr);
        assert_eq!(m.list_for(1, 9).map(|l| l.id.as_str()), Some("123"));
        // Salon pas encore lié : la liste la plus récente du serveur.
        assert_eq!(m.list_for(1, 10).map(|l| l.id.as_str()), Some("everyone"));
    }

    #[test]
    fn colors_lines_and_labels() {
        let role = |id: u64, color: u32, position: i32| Role { id, name: format!("r{id}"), color, position, permissions: Permissions(0), mentionable: false };
        let roles = [role(1, 0xff0000, 1), role(2, 0x00ff00, 5), role(3, 0, 9)];
        assert_eq!(top_color(&[1, 2, 3], &roles), 0x00ff00, "le plus haut rôle coloré");
        assert_eq!(top_color(&[3], &roles), 0);
        let play = Activity { kind: ActivityKind::Playing, name: "Jeu".into(), state: None };
        let custom = Activity { kind: ActivityKind::Custom, name: "Custom Status".into(), state: Some("En pause".into()) };
        assert_eq!(activity_line(Some(&play), Lang::Fr).as_deref(), Some("Joue à Jeu"));
        assert_eq!(activity_line(Some(&play), Lang::En).as_deref(), Some("Playing Jeu"));
        assert_eq!(pick_activity(vec![play.clone(), custom.clone()]), Some(custom));
        assert_eq!(pick_activity(vec![Activity { kind: ActivityKind::Custom, name: "x".into(), state: None }, play.clone()]), Some(play));
        assert_eq!(group_label(GroupId::Role(2), 3, &roles, Lang::Fr), "R2 — 3");
        assert_eq!(group_label(GroupId::Offline, 12, &roles, Lang::En), "OFFLINE — 12");
    }

    #[test]
    fn span_and_watch() {
        let u = update(vec![ListOp::Update { index: 40, item: member(1, "a") }], Vec::new());
        assert_eq!(u.span(), Some((40, 40)));
        let ins = update(vec![ListOp::Insert { index: 5, item: member(1, "a") }], Vec::new());
        assert_eq!(ins.span(), Some((5, u32::MAX)));
        let w = ListWatch::default();
        assert!(w.wants(&u), "sans filtre, tout réveille");
        w.show(Some((1, 0..30)));
        assert!(!w.wants(&u), "ligne 40 hors de la vue");
        assert!(w.wants(&ins), "une insertion au-dessus décale la vue");
        w.show(None);
        assert!(!w.wants(&ins), "liste masquée");
        // Pas plus de QUIET_FLUSH paquets sans réveil.
        let woken = (0..QUIET_FLUSH * 2).filter(|_| w.wants(&u)).count();
        assert_eq!(woken, 2);
    }

    #[test]
    fn store_pushes_a_command_only_on_change() {
        let mut s = Store::new();
        s.guild = Some(1);
        s.channel = Some(9);
        s.want_members(true, Some(0..20));
        s.want_members(true, Some(3..25));
        let cmds = s.take_commands();
        assert_eq!(cmds, vec![Cmd::Subscribe(Subscription { guild_id: 1, channel: Some((9, vec![(0, 99)])) })]);
        // Panneau masqué : la liste du salon est quittée, le serveur reste.
        s.want_members(false, None);
        assert_eq!(s.take_commands(), vec![Cmd::Subscribe(Subscription { guild_id: 1, channel: None })]);
        // Messages privés : rien de plus à quitter.
        s.guild = None;
        s.channel = None;
        s.want_members(true, None);
        assert!(s.take_commands().is_empty());
        s.members.unavailable = true;
        s.guild = Some(2);
        s.channel = Some(3);
        s.want_members(true, None);
        assert!(s.take_commands().is_empty(), "compte bot : jamais de souscription");
    }
}
