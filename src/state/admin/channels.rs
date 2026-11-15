//! Salons et catégories (lot A2) : créer (salon privé compris), modifier
//! (vue d'ensemble, dérogations de permissions, synchronisation avec la
//! catégorie), cloner, supprimer, déplacer par glisser-déposer.
//!
//! Chaque écriture suit un geste ; une seule action de chaque sorte à la
//! fois. Rien n'est supposé : la liste des salons change par CHANNEL_CREATE,
//! CHANNEL_UPDATE et CHANNEL_DELETE (`Event::ChannelUpsert` / `ChannelDelete`),
//! les détails de la fenêtre « Modifier le salon » par la réponse de Discord
//! ou par CHANNEL_UPDATE. Le réseau est dans `net::admin::channels`.

use crate::state::event::{Cmd, Event};
use crate::state::model::{Channel, ChannelKind, Overwrite, OverwriteKind};
use crate::state::notice::Problem;
use crate::state::perms::{base_permissions, with_base, Permissions, Verdict};
use crate::state::store::{Effect, Store};
use std::collections::{HashSet, VecDeque};

/// Nom d'un salon : 1 à 100 caractères (Create Guild Channel).
pub const NAME_MAX: usize = 100;
/// Sujet : 1024 caractères pour un salon textuel, 4096 pour un forum (Modify Channel).
pub const TOPIC_MAX: usize = 1024;
pub const FORUM_TOPIC_MAX: usize = 4096;
/// Mode lent : 6 heures au plus.
pub const SLOWMODE_MAX: u32 = 21_600;
/// Durées d'archivage automatique des fils permises (minutes).
pub const ARCHIVE_DURATIONS: [u32; 4] = [60, 1440, 4320, 10_080];
/// Étiquettes d'un forum : 20 au plus, nom de 50 caractères au plus.
pub const TAGS_MAX: usize = 20;
pub const TAG_NAME_MAX: usize = 50;
/// Débit d'un salon vocal (bit/s) : 8 kbps au moins (Modify Channel).
pub const BITRATE_MIN: u32 = 8000;
/// Limite d'utilisateurs d'un salon vocal : 0 (aucune) à 99.
pub const USER_LIMIT_MAX: u32 = 99;
/// Codes de type de l'API des salons vocaux et des scènes.
pub const KIND_VOICE: u8 = 2;
pub const KIND_STAGE: u8 = 13;

/// Débit maximal (bit/s) : 96 kbps, puis 128, 256 et 384 kbps selon le niveau
/// de boost du serveur ; 64 kbps pour une scène (Channel Object, `bitrate`).
pub fn bitrate_max(premium_tier: u8, stage: bool) -> u32 {
    if stage {
        return 64_000;
    }
    match premium_tier {
        0 => 96_000,
        1 => 128_000,
        2 => 256_000,
        _ => 384_000,
    }
}

/// Région vocale proposée (List Voice Regions).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Region {
    pub id: String,
    pub name: String,
}

/// Type d'un salon à créer (codes de la documentation, Channel Types).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NewKind {
    Text,
    /// Vocal et scène : créés comme objets seulement (le vocal viendra en phase 4).
    Voice,
    Category,
    Announcement,
    Stage,
    Forum,
    Media,
}

impl NewKind {
    pub fn code(self) -> u8 {
        match self {
            NewKind::Text => 0,
            NewKind::Voice => 2,
            NewKind::Category => 4,
            NewKind::Announcement => 5,
            NewKind::Stage => 13,
            NewKind::Forum => 15,
            NewKind::Media => 16,
        }
    }

    pub fn of(kind: ChannelKind) -> NewKind {
        match kind {
            ChannelKind::Text => NewKind::Text,
            ChannelKind::Announcement => NewKind::Announcement,
            ChannelKind::Category => NewKind::Category,
            ChannelKind::Forum => NewKind::Forum,
            ChannelKind::Media => NewKind::Media,
        }
    }

    fn voice_like(self) -> bool {
        matches!(self, NewKind::Voice | NewKind::Stage)
    }

    /// Accepte un sujet (Create Guild Channel : textuel, annonces, forum, média, scène).
    pub fn has_topic(self) -> bool {
        !matches!(self, NewKind::Voice | NewKind::Category)
    }
}

/// Salon à créer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewChannel {
    pub name: String,
    pub kind: NewKind,
    pub parent: Option<u64>,
    pub overwrites: Vec<Overwrite>,
    pub topic: String,
    pub nsfw: bool,
    pub slowmode: u32,
}

/// Étiquette d'un forum (`id` absent : nouvelle).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Tag {
    pub id: Option<u64>,
    pub name: String,
    pub moderated: bool,
    pub emoji_id: Option<u64>,
    pub emoji_name: Option<String>,
}

/// Réaction par défaut d'un forum : emoji personnalisé ou Unicode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Reaction {
    Custom(u64),
    Unicode(String),
}

/// Ce que la fenêtre « Modifier le salon » montre, tel que Discord l'a donné.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Details {
    pub id: u64,
    pub guild_id: Option<u64>,
    /// Code de type de l'API.
    pub kind: u8,
    pub name: String,
    pub topic: String,
    pub nsfw: bool,
    pub slowmode: u32,
    /// Durée d'archivage par défaut des fils (minutes) ; `None` : celle de Discord.
    pub auto_archive: Option<u32>,
    pub tags: Vec<Tag>,
    pub reaction: Option<Reaction>,
    /// Salon vocal ou scène : débit (bit/s).
    pub bitrate: Option<u32>,
    /// Salon vocal : nombre maximal de personnes (0 : aucune limite).
    pub user_limit: Option<u32>,
    /// Région imposée ; `None` : choisie par Discord.
    pub rtc_region: Option<String>,
    /// Qualité vidéo : 1 automatique, 2 toujours 720p.
    pub video_quality: Option<u8>,
}

/// Modification d'un salon : seuls les champs présents partent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Patch {
    pub name: Option<String>,
    /// Textuel (0) <-> annonces (5), seule conversion permise.
    pub kind: Option<u8>,
    pub topic: Option<String>,
    pub nsfw: Option<bool>,
    pub slowmode: Option<u32>,
    pub auto_archive: Option<u32>,
    pub overwrites: Option<Vec<Overwrite>>,
    pub tags: Option<Vec<Tag>>,
    /// `Some(None)` : retirer la réaction par défaut.
    pub reaction: Option<Option<Reaction>>,
    pub bitrate: Option<u32>,
    pub user_limit: Option<u32>,
    /// `Some(None)` : région automatique.
    pub rtc_region: Option<Option<String>>,
    pub video_quality: Option<u8>,
}

impl Patch {
    pub fn is_empty(&self) -> bool {
        *self == Patch::default()
    }
}

/// Une entrée de Modify Guild Channel Positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Move {
    pub id: u64,
    pub position: i32,
    /// `Some(p)` : nouveau parent (`None` : hors catégorie).
    pub parent: Option<Option<u64>>,
    /// Dérogations alignées sur le nouveau parent.
    pub lock: bool,
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChanCmd {
    /// `GET /channels/{id}`.
    Load { channel_id: u64 },
    /// `POST /guilds/{id}/channels`.
    Create { guild_id: u64, new: NewChannel },
    /// `PATCH /channels/{id}`.
    Modify { channel_id: u64, patch: Patch },
    /// `DELETE /channels/{id}`.
    Delete { channel_id: u64 },
    /// `PATCH /guilds/{id}/channels`.
    Positions { guild_id: u64, moves: Vec<Move> },
    /// `GET /voice/regions` (réglages d'un salon vocal).
    Regions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChanAction {
    Load,
    Create,
    Save,
    Delete,
    Move,
    Regions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChanFailure {
    Problem(Problem),
    /// Salon inconnu (supprimé entre-temps).
    NotFound,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum ChanEvent {
    /// Réponse de `Load` ou `Modify`, ou CHANNEL_CREATE / CHANNEL_UPDATE.
    Details(Details),
    /// Réponse de `Modify` (les détails suivent).
    Saved { channel_id: u64 },
    Created { guild_id: u64, channel_id: u64 },
    Deleted { channel_id: u64 },
    Moved { guild_id: u64 },
    Regions(Vec<Region>),
    Failed { action: ChanAction, failure: ChanFailure },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChanNotice {
    Saved,
    Deleted { name: String },
    Failed { action: ChanAction, failure: ChanFailure },
}

#[derive(Default)]
pub struct ChannelAdmin {
    pending: HashSet<ChanAction>,
    /// Détails du salon ouvert dans « Modifier le salon » (ou à cloner).
    pub details: Option<Details>,
    /// Salon dont les détails sont attendus.
    watching: Option<u64>,
    notices: VecDeque<ChanNotice>,
    done: Option<ChanAction>,
    /// Salon créé, à ouvrir dès que CHANNEL_CREATE l'apporte.
    pub open_when_ready: Option<u64>,
    /// Nom du salon en cours de suppression, pour le message.
    deleting: Option<String>,
    /// Régions vocales, lues une fois (elles ne dépendent pas du compte).
    pub regions: Option<Vec<Region>>,
}

impl ChannelAdmin {
    pub fn clear(&mut self) {
        *self = ChannelAdmin { regions: self.regions.take(), ..ChannelAdmin::default() };
    }

    pub fn pending(&self, a: ChanAction) -> bool {
        self.pending.contains(&a)
    }

    pub fn take_notice(&mut self) -> Option<ChanNotice> {
        self.notices.pop_front()
    }

    pub fn take_done(&mut self) -> Option<ChanAction> {
        self.done.take()
    }

    /// Détails de ce salon s'ils sont arrivés.
    pub fn details_of(&self, channel: u64) -> Option<&Details> {
        self.details.as_ref().filter(|d| d.id == channel)
    }

    /// Fenêtre fermée : plus aucun détail attendu.
    pub fn forget(&mut self) {
        self.watching = None;
        self.details = None;
    }
}

/// Nom accepté (espaces aux bords retirés), ou `None`.
pub fn valid_name(name: &str) -> Option<String> {
    let n = name.trim();
    (1..=NAME_MAX).contains(&n.chars().count()).then(|| n.to_string())
}

/// Les deux listes portent-elles les mêmes dérogations (ordre indifférent,
/// dérogations vides ignorées) ?
pub fn same_overwrites(a: &[Overwrite], b: &[Overwrite]) -> bool {
    let norm = |v: &[Overwrite]| {
        let mut v: Vec<(bool, u64, u64, u64)> =
            v.iter().filter(|o| o.allow.0 != 0 || o.deny.0 != 0).map(|o| (o.kind == OverwriteKind::Member, o.id, o.allow.0, o.deny.0)).collect();
        v.sort_unstable();
        v
    };
    norm(a) == norm(b)
}

/// État d'une permission dans une dérogation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tri {
    Allow,
    Neutral,
    Deny,
}

pub fn tri(o: &Overwrite, p: Permissions) -> Tri {
    if o.allow.contains(p) {
        Tri::Allow
    } else if o.deny.contains(p) {
        Tri::Deny
    } else {
        Tri::Neutral
    }
}

pub fn set_tri(o: &mut Overwrite, p: Permissions, t: Tri) {
    o.allow = o.allow & !p;
    o.deny = o.deny & !p;
    match t {
        Tri::Allow => o.allow = o.allow | p,
        Tri::Deny => o.deny = o.deny | p,
        Tri::Neutral => {}
    }
}

/// Dérogations d'un salon privé : @everyone ne le voit pas, les rôles et
/// membres choisis le voient (et s'y connectent pour un vocal). `me` :
/// ajouté s'il n'est pas administrateur, pour ne pas s'en exclure.
pub fn private_overwrites(guild_id: u64, kind: NewKind, roles: &[u64], members: &[u64], me: Option<u64>) -> Vec<Overwrite> {
    let see = if kind.voice_like() { Permissions::VIEW_CHANNEL | Permissions::CONNECT } else { Permissions::VIEW_CHANNEL };
    let mut out = vec![Overwrite { id: guild_id, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: see }];
    for r in roles.iter().filter(|r| **r != guild_id) {
        out.push(Overwrite { id: *r, kind: OverwriteKind::Role, allow: see, deny: Permissions::NONE });
    }
    let mut ms: Vec<u64> = members.to_vec();
    if let Some(me) = me
        && !ms.contains(&me)
    {
        ms.push(me);
    }
    ms.dedup();
    for m in ms {
        out.push(Overwrite { id: m, kind: OverwriteKind::Member, allow: see, deny: Permissions::NONE });
    }
    out
}

/// Où tombe l'objet glissé dans la liste des salons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drop {
    Before(u64),
    After(u64),
    /// Sur l'en-tête d'une catégorie : en tête de cette catégorie.
    Into(u64),
}

fn sorted_ids(chs: &[&Channel]) -> Vec<u64> {
    let mut v: Vec<(i32, u64)> = chs.iter().map(|c| (c.position, c.id)).collect();
    v.sort_unstable();
    v.into_iter().map(|(_, id)| id).collect()
}

/// Positions renumérotées de 0 : les entrées dont la position change.
fn renumber(order: &[u64], chs: &[Channel]) -> Vec<Move> {
    order
        .iter()
        .enumerate()
        .filter_map(|(i, id)| {
            let pos = i32::try_from(i).unwrap_or(i32::MAX);
            let c = chs.iter().find(|c| c.id == *id)?;
            (c.position != pos).then_some(Move { id: *id, position: pos, parent: None, lock: false })
        })
        .collect()
}

/// Déplacements à envoyer pour déposer `dragged` à `drop` ; `None` si rien
/// ne change ou si le dépôt n'a pas de sens. Un seul changement de parent
/// par requête (règle de Discord) : c'est toujours le cas ici.
pub fn plan_move(chs: &[Channel], dragged: u64, drop: Drop) -> Option<Vec<Move>> {
    let d = chs.iter().find(|c| c.id == dragged)?;
    let target = match drop {
        Drop::Before(t) | Drop::After(t) | Drop::Into(t) => t,
    };
    if target == dragged {
        return None;
    }
    let t = chs.iter().find(|c| c.id == target)?;
    if d.kind == ChannelKind::Category {
        // Une catégorie ne se range qu'entre catégories.
        let cat = if t.kind == ChannelKind::Category { t.id } else { t.parent? };
        let cats: Vec<&Channel> = chs.iter().filter(|c| c.kind == ChannelKind::Category).collect();
        let mut order: Vec<u64> = sorted_ids(&cats).into_iter().filter(|id| *id != dragged).collect();
        let at = order.iter().position(|id| *id == cat)?;
        let after = matches!(drop, Drop::After(_)) && t.kind == ChannelKind::Category || t.kind != ChannelKind::Category;
        order.insert(if after { at + 1 } else { at }, dragged);
        let moves = renumber(&order, chs);
        return (!moves.is_empty()).then_some(moves);
    }
    let (parent, at_top) = match drop {
        Drop::Into(_) if t.kind == ChannelKind::Category => (Some(t.id), true),
        Drop::Into(_) => return None,
        _ if t.kind == ChannelKind::Category => return None,
        _ => (t.parent, false),
    };
    let siblings: Vec<&Channel> = chs.iter().filter(|c| c.kind != ChannelKind::Category && c.parent == parent && c.id != dragged).collect();
    let mut order = sorted_ids(&siblings);
    let at = if at_top {
        0
    } else {
        let i = order.iter().position(|id| *id == target)?;
        if matches!(drop, Drop::After(_)) { i + 1 } else { i }
    };
    order.insert(at, dragged);
    let moved_parent = d.parent != parent;
    let mut moves = renumber(&order, chs);
    if moved_parent {
        // Synchronisé avec son ancienne catégorie : il le reste avec la nouvelle.
        let lock = parent.is_some() && d.parent.and_then(|p| chs.iter().find(|c| c.id == p)).is_some_and(|p| d.overwrites.as_deref().zip(p.overwrites.as_deref()).is_some_and(|(a, b)| same_overwrites(a, b)));
        let pos = i32::try_from(at).unwrap_or(i32::MAX);
        match moves.iter_mut().find(|m| m.id == dragged) {
            Some(m) => {
                m.parent = Some(parent);
                m.lock = lock;
            }
            None => moves.push(Move { id: dragged, position: pos, parent: Some(parent), lock }),
        }
    }
    (!moves.is_empty()).then_some(moves)
}

impl Store {
    fn chan_cmd(&mut self, action: ChanAction, cmd: ChanCmd) -> bool {
        if !self.chan_admin.pending.insert(action) {
            return false;
        }
        self.push_command(Cmd::ChannelAdmin(cmd));
        true
    }

    fn guild_of_channel(&self, channel: u64) -> Option<&crate::state::model::Guild> {
        self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel))
    }

    fn base_of(&self, guild: u64) -> Option<Permissions> {
        let (me, g) = (self.me.as_ref()?, self.guilds.iter().find(|g| g.id == guild)?);
        base_permissions(g, me.id)
    }

    /// « Gérer les salons » au niveau du serveur (créer, réordonner). Inconnu : proposé (Discord répondra).
    pub fn can_manage_channels(&self, guild: u64) -> bool {
        self.me.is_some() && self.guilds.iter().any(|g| g.id == guild) && self.base_of(guild).is_none_or(|p| p.contains(Permissions::MANAGE_CHANNELS))
    }

    /// Modifier ou supprimer ce salon (MANAGE_CHANNELS, dérogations comprises).
    pub fn can_edit_channel(&self, channel: u64) -> bool {
        if self.voice.channel(channel).is_some() {
            return self.voice.can(channel, Permissions::MANAGE_CHANNELS);
        }
        self.guild_of_channel(channel).is_some() && self.can(channel, Permissions::MANAGE_CHANNELS) != Verdict::Refuse
    }

    /// Débit maximal de ce salon vocal, d'après le niveau de boost de son serveur.
    pub fn voice_bitrate_max(&self, channel: u64) -> u32 {
        match self.voice.channel(channel) {
            Some(c) => bitrate_max(self.guilds.iter().find(|g| g.id == c.guild_id).map_or(0, |g| g.premium_tier), c.stage),
            None => bitrate_max(0, false),
        }
    }

    /// Régions vocales pour « Modifier le salon » : demandées une fois, à l'ouverture.
    pub fn load_voice_regions(&mut self) -> bool {
        self.chan_admin.regions.is_none() && self.chan_cmd(ChanAction::Regions, ChanCmd::Regions)
    }

    /// Modifier les dérogations de ce salon (« Gérer les permissions » : MANAGE_ROLES).
    pub fn can_edit_overwrites(&self, channel: u64) -> bool {
        self.can_edit_channel(channel) && self.can(channel, Permissions::MANAGE_ROLES) != Verdict::Refuse
    }

    /// Me faut-il une dérogation à mon nom pour voir un salon privé ? (Ni
    /// propriétaire ni administrateur.)
    pub fn private_needs_me(&self, guild: u64) -> Option<u64> {
        let me = self.me.as_ref()?.id;
        (self.base_of(guild) != Some(Permissions::ALL)).then_some(me)
    }

    /// Avec ces dérogations, perdrais-je l'accès à ce salon (le voir ou en
    /// gérer les permissions) que j'ai aujourd'hui ?
    pub fn would_lose_access(&self, channel: u64, overwrites: &[Overwrite]) -> bool {
        let Some(g) = self.guild_of_channel(channel) else { return false };
        let (Some(me), Some(c)) = (self.me.as_ref().map(|u| u.id), g.channels.iter().find(|c| c.id == channel)) else { return false };
        let Some(base) = base_permissions(g, me) else { return false };
        let need = Permissions::VIEW_CHANNEL | Permissions::MANAGE_ROLES;
        let now = self.now();
        let before = with_base(g, me, base, c, now).is_some_and(|p| p.contains(need));
        let mut after = c.clone();
        after.overwrites = Some(overwrites.to_vec());
        let after = with_base(g, me, base, &after, now).is_some_and(|p| p.contains(need));
        before && !after
    }

    /// Ce salon porte-t-il les mêmes dérogations que sa catégorie ? `None` sans catégorie.
    pub fn synced_with_category(&self, channel: u64) -> Option<bool> {
        let g = self.guild_of_channel(channel)?;
        let c = g.channels.iter().find(|c| c.id == channel)?;
        let p = g.channels.iter().find(|x| Some(x.id) == c.parent)?;
        Some(c.overwrites.as_deref().zip(p.overwrites.as_deref()).is_some_and(|(a, b)| same_overwrites(a, b)))
    }

    /// Crée un salon ; `false` si le nom ne convient pas ou si une création est en cours.
    pub fn create_channel(&mut self, guild_id: u64, mut new: NewChannel) -> bool {
        let Some(name) = valid_name(&new.name) else { return false };
        new.name = name;
        new.slowmode = new.slowmode.min(SLOWMODE_MAX);
        if new.kind == NewKind::Category {
            new.parent = None;
        }
        self.chan_cmd(ChanAction::Create, ChanCmd::Create { guild_id, new })
    }

    /// Demande les détails d'un salon (fenêtre « Modifier le salon », clonage).
    pub fn load_channel(&mut self, channel_id: u64) -> bool {
        self.chan_admin.watching = Some(channel_id);
        if self.chan_admin.details.as_ref().is_some_and(|d| d.id != channel_id) {
            self.chan_admin.details = None;
        }
        self.chan_cmd(ChanAction::Load, ChanCmd::Load { channel_id })
    }

    /// Enregistre une modification ; `false` si rien ne change ou si un
    /// enregistrement est en cours.
    pub fn save_channel(&mut self, channel_id: u64, mut patch: Patch) -> bool {
        if patch.name.as_ref().is_some_and(|n| valid_name(n).is_none()) {
            return false;
        }
        patch.name = patch.name.and_then(|n| valid_name(&n));
        patch.slowmode = patch.slowmode.map(|s| s.min(SLOWMODE_MAX));
        patch.auto_archive = patch.auto_archive.filter(|d| ARCHIVE_DURATIONS.contains(d));
        let max = self.voice_bitrate_max(channel_id);
        patch.bitrate = patch.bitrate.map(|b| b.clamp(BITRATE_MIN, max));
        patch.user_limit = patch.user_limit.map(|l| l.min(USER_LIMIT_MAX));
        patch.video_quality = patch.video_quality.filter(|q| matches!(q, 1 | 2));
        if let Some(tags) = patch.tags.as_mut() {
            tags.retain(|t| !t.name.trim().is_empty());
            tags.truncate(TAGS_MAX);
        }
        if patch.is_empty() {
            return false;
        }
        self.chan_cmd(ChanAction::Save, ChanCmd::Modify { channel_id, patch })
    }

    /// Aligne les dérogations de ce salon sur celles de sa catégorie.
    pub fn sync_with_category(&mut self, channel_id: u64) -> bool {
        let Some(ows) = self.guild_of_channel(channel_id).and_then(|g| {
            let c = g.channels.iter().find(|c| c.id == channel_id)?;
            g.channels.iter().find(|x| Some(x.id) == c.parent)?.overwrites.clone()
        }) else {
            return false;
        };
        self.save_channel(channel_id, Patch { overwrites: Some(ows), ..Patch::default() })
    }

    /// Supprime un salon (après confirmation dans l'interface).
    pub fn delete_channel(&mut self, channel_id: u64) -> bool {
        let name = self.channel_name(channel_id).unwrap_or_default().to_string();
        if !self.chan_cmd(ChanAction::Delete, ChanCmd::Delete { channel_id }) {
            return false;
        }
        self.chan_admin.deleting = Some(name);
        true
    }

    /// Clone un salon (type, catégorie, dérogations, mode lent, et sujet et
    /// NSFW si ses détails sont arrivés) sous le nom `name`.
    pub fn clone_channel(&mut self, channel_id: u64, name: &str) -> bool {
        let Some((guild_id, c)) = self.guild_of_channel(channel_id).and_then(|g| Some((g.id, g.channels.iter().find(|c| c.id == channel_id)?.clone()))) else {
            return false;
        };
        let d = self.chan_admin.details_of(channel_id);
        let new = NewChannel {
            name: name.to_string(),
            kind: NewKind::of(c.kind),
            parent: c.parent,
            overwrites: c.overwrites.clone().unwrap_or_default(),
            topic: d.map(|d| d.topic.clone()).unwrap_or_default(),
            nsfw: d.is_some_and(|d| d.nsfw),
            slowmode: c.slowmode,
        };
        self.create_channel(guild_id, new)
    }

    /// Dépose `dragged` à `drop` dans la liste des salons de `guild` : la
    /// seule écriture du glisser-déposer, au relâchement.
    pub fn move_channel(&mut self, guild_id: u64, dragged: u64, drop: Drop) -> bool {
        let Some(moves) = self.guilds.iter().find(|g| g.id == guild_id).and_then(|g| plan_move(&g.channels, dragged, drop)) else { return false };
        self.chan_cmd(ChanAction::Move, ChanCmd::Positions { guild_id, moves })
    }

    pub(crate) fn apply_chan_admin(&mut self, e: ChanEvent) -> Option<Effect> {
        let s = &mut self.chan_admin;
        match e {
            ChanEvent::Saved { .. } => {
                s.pending.remove(&ChanAction::Save);
                s.done = Some(ChanAction::Save);
                s.notices.push_back(ChanNotice::Saved);
            }
            ChanEvent::Details(d) => {
                if s.watching == Some(d.id) {
                    s.pending.remove(&ChanAction::Load);
                    s.details = Some(d);
                }
            }
            ChanEvent::Created { channel_id, .. } => {
                s.pending.remove(&ChanAction::Create);
                s.done = Some(ChanAction::Create);
                s.open_when_ready = Some(channel_id);
            }
            ChanEvent::Deleted { channel_id } => {
                s.pending.remove(&ChanAction::Delete);
                s.done = Some(ChanAction::Delete);
                let name = s.deleting.take().unwrap_or_default();
                s.notices.push_back(ChanNotice::Deleted { name });
                if s.watching == Some(channel_id) {
                    s.forget();
                }
                // Comme CHANNEL_DELETE (qui suivra, et ne trouvera plus rien).
                let guild_id = self.guild_of_channel(channel_id).map(|g| g.id);
                return self.apply(Event::ChannelDelete { guild_id, id: channel_id });
            }
            ChanEvent::Moved { .. } => {
                s.pending.remove(&ChanAction::Move);
            }
            ChanEvent::Regions(list) => {
                s.pending.remove(&ChanAction::Regions);
                s.regions = Some(list);
            }
            ChanEvent::Failed { action, failure } => {
                s.pending.remove(&action);
                if action == ChanAction::Delete {
                    s.deleting = None;
                }
                s.notices.push_back(ChanNotice::Failed { action, failure });
            }
        }
        None
    }

    /// Salon créé à l'instant et arrivé par la passerelle : ouvert (une fois).
    pub fn open_created_channel(&mut self) -> bool {
        let Some(id) = self.chan_admin.open_when_ready else { return false };
        let Some(c) = self.guilds.iter().flat_map(|g| g.channels.iter()).find(|c| c.id == id) else { return false };
        self.chan_admin.open_when_ready = None;
        if c.kind == ChannelKind::Category {
            return false;
        }
        self.select_channel(id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Guild, Member, Role, User};

    const G: u64 = 100;
    const ME: u64 = 1;
    const CAT_A: u64 = 10;
    const CAT_B: u64 = 20;
    const P: Permissions = Permissions::VIEW_CHANNEL;

    fn ch(id: u64, kind: ChannelKind, position: i32, parent: Option<u64>, ows: Vec<Overwrite>) -> Channel {
        Channel { id, name: format!("c{id}"), kind, position, parent, overwrites: Some(ows), slowmode: 0 }
    }

    fn deny_everyone() -> Vec<Overwrite> {
        vec![Overwrite { id: G, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: P }]
    }

    /// Hors catégorie : 1, 2 ; catégorie A (synchronisée) : 11, 12 ; B : 21.
    fn channels() -> Vec<Channel> {
        use ChannelKind::*;
        let mut v = vec![
            ch(1, Text, 0, None, vec![]),
            ch(2, Text, 1, None, vec![]),
            ch(CAT_A, Category, 0, None, deny_everyone()),
            ch(11, Text, 0, Some(CAT_A), deny_everyone()),
            ch(12, Forum, 1, Some(CAT_A), vec![]),
            ch(CAT_B, Category, 1, None, vec![]),
            ch(21, Text, 0, Some(CAT_B), vec![]),
        ];
        crate::state::model::sort_channels(&mut v);
        v
    }

    fn store(owner: bool, my_perms: Permissions) -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "g".into(), channels());
        g.owner_id = Some(if owner { ME } else { 999 });
        g.roles = vec![Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: my_perms, mentionable: false }];
        g.me = Some(Member::default());
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![g], dms: vec![] });
        s
    }

    #[test]
    fn move_within_and_across_categories() {
        let chs = channels();
        // 12 au-dessus de 11, même catégorie : deux positions changent, pas de parent.
        let m = plan_move(&chs, 12, Drop::Before(11)).unwrap_or_default();
        assert_eq!(m, vec![Move { id: 12, position: 0, parent: None, lock: false }, Move { id: 11, position: 1, parent: None, lock: false }]);
        // Sur lui-même, ou à sa place : rien.
        assert_eq!(plan_move(&chs, 12, Drop::After(11)), None);
        assert_eq!(plan_move(&chs, 12, Drop::Before(12)), None);
        // 11 (synchronisé avec A) vers B : parent changé, dérogations alignées sur B.
        let m = plan_move(&chs, 11, Drop::After(21)).unwrap_or_default();
        assert_eq!(m, vec![Move { id: 11, position: 1, parent: Some(Some(CAT_B)), lock: true }]);
        // 12 (non synchronisé) sur l'en-tête de B : en tête, sans alignement.
        let m = plan_move(&chs, 12, Drop::Into(CAT_B)).unwrap_or_default();
        assert_eq!(m, vec![Move { id: 12, position: 0, parent: Some(Some(CAT_B)), lock: false }, Move { id: 21, position: 1, parent: None, lock: false }]);
        // Hors catégorie : parent vidé, pas d'alignement possible.
        let m = plan_move(&chs, 21, Drop::Before(1)).unwrap_or_default();
        assert!(m.contains(&Move { id: 21, position: 0, parent: Some(None), lock: false }), "{m:?}");
        assert_eq!(m.len(), 3, "1 et 2 descendent");
        // Un salon ne se dépose pas « avant » une catégorie.
        assert_eq!(plan_move(&chs, 1, Drop::Before(CAT_A)), None);
        // Un seul changement de parent par requête.
        assert!(plan_move(&chs, 12, Drop::Into(CAT_B)).unwrap_or_default().iter().filter(|m| m.parent.is_some()).count() <= 1);
    }

    #[test]
    fn move_categories() {
        let chs = channels();
        let m = plan_move(&chs, CAT_B, Drop::Before(CAT_A)).unwrap_or_default();
        assert_eq!(m, vec![Move { id: CAT_B, position: 0, parent: None, lock: false }, Move { id: CAT_A, position: 1, parent: None, lock: false }]);
        // Déposée sur un salon de B : après B.
        assert_eq!(plan_move(&chs, CAT_A, Drop::Before(21)), Some(vec![Move { id: CAT_B, position: 0, parent: None, lock: false }, Move { id: CAT_A, position: 1, parent: None, lock: false }]));
        assert_eq!(plan_move(&chs, CAT_A, Drop::Into(CAT_A)), None);
        assert_eq!(plan_move(&chs, CAT_A, Drop::Before(1)), None, "salon hors catégorie");
    }

    #[test]
    fn overwrite_states_and_private_channels() {
        let mut o = Overwrite { id: 5, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: Permissions::NONE };
        set_tri(&mut o, P, Tri::Deny);
        assert_eq!(tri(&o, P), Tri::Deny);
        set_tri(&mut o, P, Tri::Allow);
        assert_eq!((o.allow, o.deny), (P, Permissions::NONE));
        set_tri(&mut o, P, Tri::Neutral);
        assert_eq!(tri(&o, P), Tri::Neutral);

        let ows = private_overwrites(G, NewKind::Text, &[7, G], &[], Some(ME));
        assert_eq!(ows.len(), 3, "@everyone, le rôle 7, moi");
        assert_eq!(ows[0].deny, P);
        assert!(ows.iter().any(|o| o.kind == OverwriteKind::Member && o.id == ME && o.allow == P));
        let voice = private_overwrites(G, NewKind::Voice, &[], &[ME], Some(ME));
        assert_eq!(voice.len(), 2);
        assert!(voice[0].deny.contains(Permissions::CONNECT));

        assert!(same_overwrites(&deny_everyone(), &[Overwrite { id: 3, kind: OverwriteKind::Member, allow: Permissions::NONE, deny: Permissions::NONE }, deny_everyone()[0].clone()]));
        assert!(!same_overwrites(&deny_everyone(), &[]));
    }

    #[test]
    fn permissions_and_access_warning() {
        let s = store(false, Permissions::VIEW_CHANNEL | Permissions::MANAGE_CHANNELS | Permissions::MANAGE_ROLES);
        assert!(s.can_manage_channels(G));
        assert!(s.can_edit_overwrites(1));
        assert_eq!(s.private_needs_me(G), Some(ME));
        // Refuser VIEW_CHANNEL à @everyone sur #1 : je ne le verrais plus.
        assert!(s.would_lose_access(1, &deny_everyone()));
        let mut keep = deny_everyone();
        keep.push(Overwrite { id: ME, kind: OverwriteKind::Member, allow: P, deny: Permissions::NONE });
        assert!(!s.would_lose_access(1, &keep));
        assert_eq!(s.synced_with_category(11), Some(true));
        assert_eq!(s.synced_with_category(12), Some(false));
        assert_eq!(s.synced_with_category(1), None);

        // Propriétaire : jamais d'avertissement ni de dérogation à mon nom.
        let s = store(true, Permissions::NONE);
        assert!(!s.would_lose_access(1, &deny_everyone()));
        assert_eq!(s.private_needs_me(G), None);

        let s = store(false, Permissions::VIEW_CHANNEL);
        assert!(!s.can_manage_channels(G));
        assert!(!s.can_edit_channel(1));
    }

    #[test]
    fn one_action_of_each_kind_and_events() {
        let mut s = store(true, Permissions::NONE);
        let new = |name: &str| NewChannel { name: name.into(), kind: NewKind::Text, parent: Some(CAT_A), overwrites: vec![], topic: String::new(), nsfw: false, slowmode: 99_999 };
        assert!(!s.create_channel(G, new("   ")), "nom vide");
        assert!(s.create_channel(G, new(" salon ")));
        assert!(!s.create_channel(G, new("autre")), "déjà en cours");
        let cmds = s.take_commands();
        let [Cmd::ChannelAdmin(ChanCmd::Create { new: n, .. })] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!((n.name.as_str(), n.slowmode), ("salon", SLOWMODE_MAX));
        s.apply(Event::ChannelAdmin(ChanEvent::Created { guild_id: G, channel_id: 50 }));
        assert!(!s.open_created_channel(), "pas encore arrivé");
        s.apply(Event::ChannelUpsert { guild_id: G, channel: ch(50, ChannelKind::Text, 2, Some(CAT_A), vec![]) });
        assert!(s.open_created_channel());
        assert_eq!(s.channel, Some(50));
        // L'ouverture demande l'historique : sans rapport ici.
        s.take_commands();

        assert!(!s.save_channel(1, Patch::default()), "rien à enregistrer");
        assert!(s.save_channel(1, Patch { slowmode: Some(5), auto_archive: Some(7), ..Patch::default() }));
        let cmds = s.take_commands();
        assert_eq!(cmds, vec![Cmd::ChannelAdmin(ChanCmd::Modify { channel_id: 1, patch: Patch { slowmode: Some(5), ..Patch::default() } })]);
        s.apply(Event::ChannelAdmin(ChanEvent::Failed { action: ChanAction::Save, failure: ChanFailure::Problem(Problem::MfaRequired) }));
        assert_eq!(s.chan_admin.take_notice(), Some(ChanNotice::Failed { action: ChanAction::Save, failure: ChanFailure::Problem(Problem::MfaRequired) }));

        // Synchroniser 12 avec A : les dérogations de A partent.
        assert!(s.sync_with_category(12));
        let cmds = s.take_commands();
        assert_eq!(cmds, vec![Cmd::ChannelAdmin(ChanCmd::Modify { channel_id: 12, patch: Patch { overwrites: Some(deny_everyone()), ..Patch::default() } })]);

        // Détails : seulement ceux du salon ouvert.
        assert!(s.load_channel(11));
        s.apply(Event::ChannelAdmin(ChanEvent::Details(Details { id: 2, ..Details::default() })));
        assert!(s.chan_admin.details.is_none());
        s.apply(Event::ChannelAdmin(ChanEvent::Details(Details { id: 11, topic: "sujet".into(), ..Details::default() })));
        assert_eq!(s.chan_admin.details_of(11).map(|d| d.topic.as_str()), Some("sujet"));

        // Clonage : type, catégorie, dérogations et sujet du salon.
        assert!(s.clone_channel(11, "copie"));
        let cmds = s.take_commands();
        let Some(Cmd::ChannelAdmin(ChanCmd::Create { new: n, .. })) = cmds.last() else { panic!("{cmds:?}") };
        assert_eq!((n.parent, n.topic.as_str(), n.overwrites.clone()), (Some(CAT_A), "sujet", deny_everyone()));

        // Suppression : le salon disparaît dès la réponse.
        assert!(s.delete_channel(2));
        assert_eq!(s.take_commands(), vec![Cmd::ChannelAdmin(ChanCmd::Delete { channel_id: 2 })]);
        s.apply(Event::ChannelAdmin(ChanEvent::Deleted { channel_id: 2 }));
        assert_eq!(s.chan_admin.take_notice(), Some(ChanNotice::Deleted { name: "c2".into() }));
        assert!(!s.channel_exists(2));

        // Glisser-déposer : une requête, au dépôt.
        assert!(s.move_channel(G, 12, Drop::Before(11)));
        assert!(!s.move_channel(G, 12, Drop::Before(11)), "déjà en cours");
        let cmds = s.take_commands();
        assert!(matches!(cmds.as_slice(), [Cmd::ChannelAdmin(ChanCmd::Positions { guild_id: G, .. })]), "{cmds:?}");
        s.apply(Event::ChannelAdmin(ChanEvent::Moved { guild_id: G }));
        assert!(!s.chan_admin.pending(ChanAction::Move));
    }
}
