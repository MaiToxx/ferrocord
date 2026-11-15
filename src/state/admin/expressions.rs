//! Emojis, autocollants et sons d'un serveur (lot A6) : listes, emplacements
//! selon le niveau de boost, ajout, modification, suppression. Chaque
//! écriture suit un geste ; la liste affichée vient de la passerelle
//! (GUILD_EMOJIS_UPDATE, GUILD_STICKERS_UPDATE, GUILD_SOUNDBOARD_SOUND_*),
//! les listes REST n'apportent que ce que la passerelle ne donne pas
//! (auteur, description). Le réseau est dans `net::admin::expressions`.
//!
//! Permission : MANAGE_GUILD_EXPRESSIONS. CREATE_GUILD_EXPRESSIONS n'est pas
//! retenue : d'après docs.discord.food (« Permissions », note 3), elle n'est
//! honorée que pour les clients qui annoncent un numéro de version récent
//! de l'application officielle, ce que Ferrocord ne fait pas.

use super::super::event::{Cmd, Event};
use super::super::model::Emoji;
use super::super::notice::Problem;
use super::super::perms::Permissions;
use super::super::store::{Effect, Store};
use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

/// Noms : 2 à 32 caractères (emoji, son), 2 à 30 (autocollant).
pub const NAME_MIN: usize = 2;
pub const EMOJI_NAME_MAX: usize = 32;
pub const STICKER_NAME_MAX: usize = 30;
pub const SOUND_NAME_MAX: usize = 32;
/// Description d'un autocollant : vide, ou 2 à 100 caractères.
pub const STICKER_DESC_MAX: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Emoji,
    Sticker,
    Sound,
}

/// Emoji associé à un son : Unicode, ou emoji personnalisé du serveur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoundEmoji {
    Unicode(String),
    Custom(u64),
}

/// Fichier d'un autocollant (corps `multipart/form-data`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StickerFile {
    pub name: String,
    pub mime: &'static str,
    pub bytes: Arc<[u8]>,
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprCmd {
    /// `GET /guilds/{id}/emojis`, `…/stickers`, `…/soundboard-sounds`.
    List { guild_id: u64, kind: Kind },
    /// `POST /guilds/{id}/emojis` ; `image` : adresse `data:`.
    CreateEmoji { guild_id: u64, name: String, image: String, roles: Vec<u64> },
    /// `PATCH /guilds/{id}/emojis/{emoji}`.
    EditEmoji { guild_id: u64, id: u64, name: String, roles: Vec<u64> },
    /// `POST /guilds/{id}/stickers` (multipart).
    CreateSticker { guild_id: u64, name: String, description: String, tags: String, file: StickerFile },
    /// `PATCH /guilds/{id}/stickers/{sticker}`.
    EditSticker { guild_id: u64, id: u64, name: String, description: String, tags: String },
    /// `POST /guilds/{id}/soundboard-sounds` ; `sound` : adresse `data:` ; `volume` en pour cent.
    CreateSound { guild_id: u64, name: String, sound: String, volume: u8, emoji: Option<SoundEmoji> },
    /// `PATCH /guilds/{id}/soundboard-sounds/{sound}`.
    EditSound { guild_id: u64, id: u64, name: String, volume: u8, emoji: Option<SoundEmoji> },
    /// `DELETE` de l'emoji, de l'autocollant ou du son.
    Delete { guild_id: u64, kind: Kind, id: u64 },
}

impl ExprCmd {
    pub fn guild_id(&self) -> u64 {
        match self {
            ExprCmd::List { guild_id, .. }
            | ExprCmd::CreateEmoji { guild_id, .. }
            | ExprCmd::EditEmoji { guild_id, .. }
            | ExprCmd::CreateSticker { guild_id, .. }
            | ExprCmd::EditSticker { guild_id, .. }
            | ExprCmd::CreateSound { guild_id, .. }
            | ExprCmd::EditSound { guild_id, .. }
            | ExprCmd::Delete { guild_id, .. } => *guild_id,
        }
    }

    /// Ce que la commande occupe (une seule action à la fois par élément).
    pub fn busy(&self) -> Busy {
        let (kind, op, item) = match self {
            ExprCmd::List { kind, .. } => (*kind, Op::Load, None),
            ExprCmd::CreateEmoji { .. } => (Kind::Emoji, Op::Create, None),
            ExprCmd::CreateSticker { .. } => (Kind::Sticker, Op::Create, None),
            ExprCmd::CreateSound { .. } => (Kind::Sound, Op::Create, None),
            ExprCmd::EditEmoji { id, .. } => (Kind::Emoji, Op::Edit, Some(*id)),
            ExprCmd::EditSticker { id, .. } => (Kind::Sticker, Op::Edit, Some(*id)),
            ExprCmd::EditSound { id, .. } => (Kind::Sound, Op::Edit, Some(*id)),
            ExprCmd::Delete { kind, id, .. } => (*kind, Op::Delete, Some(*id)),
        };
        Busy { guild_id: self.guild_id(), kind, op, item }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Op {
    Load,
    Create,
    Edit,
    Delete,
}

/// Action en cours : une seule par élément (ou par liste, ou par ajout).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Busy {
    pub guild_id: u64,
    pub kind: Kind,
    pub op: Op,
    /// Élément visé (`None` : chargement ou ajout).
    pub item: Option<u64>,
}

/// Pourquoi Discord a refusé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Problem(Problem),
    /// Captcha demandé : jamais contourné, à faire dans l'application officielle.
    Captcha,
    /// L'élément n'existe plus (404).
    NotFound,
}

/// Auteur d'un élément (champ `user`, donné avec la permission de gestion).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Author {
    pub id: u64,
    pub name: String,
}

/// Autocollant tel que la liste REST le décrit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StickerInfo {
    pub id: u64,
    pub description: String,
    /// Emoji associé (nom Unicode, champ `tags`).
    pub tags: String,
    pub author: Option<Author>,
}

/// Son du tableau de sons.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sound {
    pub id: u64,
    pub name: String,
    /// Volume en pour cent (0 à 100).
    pub volume: u8,
    pub emoji: Option<SoundEmoji>,
    pub available: bool,
    pub author: Option<Author>,
}

/// Fonctions qui agrandissent les emplacements ; le niveau de boost est
/// `Guild::premium_tier` (seule source, lot A8 + A9).
pub use super::Boost;

/// Emplacements d'un serveur.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slots {
    /// Emojis fixes, et autant d'animés.
    pub emoji: usize,
    pub stickers: usize,
    pub sounds: usize,
}

impl Boost {
    /// Tableaux de docs.discord.food (« Emoji », « Sticker », « Soundboard ») :
    /// emojis 50/100/150/250 (200 avec MORE_EMOJI), autocollants 5/15/30/60
    /// (60 avec MORE_STICKERS), sons 8/24/36/48 (96 avec MORE_SOUNDBOARD).
    pub fn slots(self, tier: u8) -> Slots {
        let t = usize::from(tier.min(3));
        let emoji = [50, 100, 150, 250][t].max(if self.more_emoji { 200 } else { 0 });
        let stickers = if self.more_stickers { 60 } else { [5, 15, 30, 60][t] };
        let sounds = if self.more_sounds { 96 } else { [8, 24, 36, 48][t] };
        Slots { emoji, stickers, sounds }
    }
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum ExprEvent {
    /// Liste REST des emojis : la liste du serveur et leurs auteurs.
    Emojis { guild_id: u64, emojis: Vec<Emoji>, authors: Vec<(u64, Author)> },
    /// Liste REST des autocollants : descriptions, emojis associés, auteurs.
    Stickers { guild_id: u64, list: Vec<StickerInfo> },
    /// Liste REST, ou GUILD_SOUNDBOARD_SOUNDS_UPDATE : remplace la liste.
    Sounds { guild_id: u64, list: Vec<Sound> },
    /// GUILD_SOUNDBOARD_SOUND_CREATE / UPDATE.
    SoundUpsert { guild_id: u64, sound: Sound },
    /// GUILD_SOUNDBOARD_SOUND_DELETE.
    SoundDelete { guild_id: u64, id: u64 },
    /// Fonctions qui agrandissent les emplacements (READY, GUILD_CREATE, GUILD_UPDATE).
    Boost { guild_id: u64, boost: Boost },
    /// Écriture acceptée ; `id` : l'élément (créé ou modifié, ou supprimé).
    /// `author` (création, si Discord le donne) et `sticker` (description
    /// et emoji associé de l'autocollant) complètent ce que la passerelle n'apporte pas.
    Done { busy: Busy, id: Option<u64>, author: Option<Author>, sticker: Option<StickerInfo> },
    Failed { busy: Busy, failure: Failure },
}

/// Message à afficher (mis en mots par l'interface).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ExprNotice {
    Done(Kind, Op),
    Failed(Kind, Op, Failure),
}

/// Liste REST d'un serveur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Loaded {
    #[default]
    Loading,
    Ready,
    Failed(Failure),
}

/// Ce que la saisie a de faux (rien n'est envoyé).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Invalid {
    /// Nom trop court, trop long, ou avec d'autres caractères que lettres, chiffres et `_`.
    Name,
    Description,
    /// Emoji associé manquant ou inconnu.
    Tags,
    /// Plus d'emplacement libre.
    NoSlot,
    /// Même action déjà en cours.
    Busy,
    /// Pas la permission (ou serveur inconnu).
    Forbidden,
}

/// Données de la page, par serveur. Bornées par le nombre de serveurs et par
/// les plafonds de Discord par serveur ; oubliées avec le serveur, et toutes
/// à la déconnexion.
#[derive(Default)]
pub struct Expressions {
    boosts: HashMap<u64, Boost>,
    emoji_authors: HashMap<u64, HashMap<u64, Author>>,
    sticker_info: HashMap<u64, HashMap<u64, StickerInfo>>,
    sounds: HashMap<u64, Vec<Sound>>,
    loaded: HashMap<(u64, Kind), Loaded>,
    busy: HashSet<Busy>,
    notices: VecDeque<ExprNotice>,
}

/// Messages en attente, au plus (les plus anciens partent).
const MAX_NOTICES: usize = 16;

impl Expressions {
    pub fn clear(&mut self) {
        *self = Expressions::default();
    }

    pub fn forget_guild(&mut self, g: u64) {
        self.boosts.remove(&g);
        self.emoji_authors.remove(&g);
        self.sticker_info.remove(&g);
        self.sounds.remove(&g);
        self.loaded.retain(|(id, _), _| *id != g);
        self.busy.retain(|b| b.guild_id != g);
    }

    pub fn boost(&self, g: u64) -> Boost {
        self.boosts.get(&g).copied().unwrap_or_default()
    }

    pub fn loaded(&self, g: u64, kind: Kind) -> Option<&Loaded> {
        self.loaded.get(&(g, kind))
    }

    pub fn emoji_author(&self, g: u64, id: u64) -> Option<&Author> {
        self.emoji_authors.get(&g)?.get(&id)
    }

    pub fn sticker_info(&self, g: u64, id: u64) -> Option<&StickerInfo> {
        self.sticker_info.get(&g)?.get(&id)
    }

    /// Sons connus du serveur (`None` : jamais chargés).
    pub fn sounds(&self, g: u64) -> Option<&[Sound]> {
        self.sounds.get(&g).map(Vec::as_slice)
    }

    pub fn is_busy(&self, b: &Busy) -> bool {
        self.busy.contains(b)
    }

    /// Un ajout de ce genre est en cours dans ce serveur.
    pub fn creating(&self, g: u64, kind: Kind) -> bool {
        self.is_busy(&Busy { guild_id: g, kind, op: Op::Create, item: None })
    }

    /// Une modification ou une suppression de cet élément est en cours.
    pub fn item_busy(&self, g: u64, kind: Kind, id: u64) -> bool {
        [Op::Edit, Op::Delete].iter().any(|op| self.is_busy(&Busy { guild_id: g, kind, op: *op, item: Some(id) }))
    }

    pub fn take_notice(&mut self) -> Option<ExprNotice> {
        self.notices.pop_front()
    }

    fn notice(&mut self, n: ExprNotice) {
        if self.notices.len() >= MAX_NOTICES {
            self.notices.pop_front();
        }
        self.notices.push_back(n);
    }
}

/// Nom d'emoji ou de son : lettres, chiffres et `_` (règle de l'application
/// officielle), longueur bornée.
pub fn valid_name(name: &str, max: usize, word_only: bool) -> Option<String> {
    let n = name.trim();
    let len = n.chars().count();
    let chars_ok = !word_only || n.chars().all(|c| c.is_alphanumeric() || c == '_');
    ((NAME_MIN..=max).contains(&len) && chars_ok).then(|| n.to_string())
}

/// Nom proposé à partir d'un nom de fichier : extension retirée, autres
/// caractères remplacés par `_`, coupé à `max`.
pub fn name_from_file(file: &str, max: usize) -> String {
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let mut out: String = stem.chars().map(|c| if c.is_alphanumeric() || c == '_' { c } else { '_' }).take(max).collect();
    while out.chars().count() < NAME_MIN {
        out.push('_');
    }
    out
}

impl Store {
    /// MANAGE_GUILD_EXPRESSIONS dans ce serveur. Permissions inconnues :
    /// proposé (Discord répondra).
    /// Emplacements d'expressions du serveur : niveau de `Guild` et fonctions reçues.
    pub fn expr_slots(&self, guild: u64) -> Slots {
        self.expressions.boost(guild).slots(self.guild_tier(guild))
    }

    /// Niveau de boost du serveur (`Guild::premium_tier`, 0 si inconnu).
    pub fn guild_tier(&self, guild: u64) -> u8 {
        self.guilds.iter().find(|g| g.id == guild).map_or(0, |g| g.premium_tier)
    }

    pub fn can_manage_expressions(&self, guild: u64) -> bool {
        // Droits inconnus (`Authority::Unknown`) : rien n'est proposé (règle commune).
        self.guild_has(guild, Permissions::MANAGE_GUILD_EXPRESSIONS)
    }

    fn expr_cmd(&mut self, cmd: ExprCmd) -> Result<(), Invalid> {
        if !self.can_manage_expressions(cmd.guild_id()) {
            return Err(Invalid::Forbidden);
        }
        if !self.expressions.busy.insert(cmd.busy()) {
            return Err(Invalid::Busy);
        }
        self.push_command(Cmd::Expressions(cmd));
        Ok(())
    }

    /// Demande la liste REST (auteurs, descriptions, sons) ; à l'ouverture
    /// d'une page et sur « Actualiser ».
    pub fn load_expressions(&mut self, guild_id: u64, kind: Kind) -> bool {
        let ok = self.expr_cmd(ExprCmd::List { guild_id, kind }).is_ok();
        if ok {
            self.expressions.loaded.insert((guild_id, kind), Loaded::Loading);
        }
        ok
    }

    /// Emojis fixes et animés du serveur (dans cet ordre).
    pub fn emoji_counts(&self, guild: u64) -> (usize, usize) {
        let list = self.guilds.iter().find(|g| g.id == guild).map_or(&[][..], |g| g.emojis.as_slice());
        let animated = list.iter().filter(|e| e.animated).count();
        (list.len() - animated, animated)
    }

    pub fn create_emoji(&mut self, guild_id: u64, name: &str, image: String, animated: bool, roles: Vec<u64>) -> Result<(), Invalid> {
        let name = valid_name(name, EMOJI_NAME_MAX, true).ok_or(Invalid::Name)?;
        let (fixed, moving) = self.emoji_counts(guild_id);
        if (if animated { moving } else { fixed }) >= self.expr_slots(guild_id).emoji {
            return Err(Invalid::NoSlot);
        }
        self.expr_cmd(ExprCmd::CreateEmoji { guild_id, name, image, roles })
    }

    pub fn edit_emoji(&mut self, guild_id: u64, id: u64, name: &str, roles: Vec<u64>) -> Result<(), Invalid> {
        let name = valid_name(name, EMOJI_NAME_MAX, true).ok_or(Invalid::Name)?;
        self.expr_cmd(ExprCmd::EditEmoji { guild_id, id, name, roles })
    }

    fn sticker_fields(name: &str, description: &str, tags: &str) -> Result<(String, String, String), Invalid> {
        let name = valid_name(name, STICKER_NAME_MAX, false).ok_or(Invalid::Name)?;
        let description = description.trim().to_string();
        let n = description.chars().count();
        if n == 1 || n > STICKER_DESC_MAX {
            return Err(Invalid::Description);
        }
        let tags = tags.trim().to_string();
        if tags.is_empty() || tags.chars().count() > 200 {
            return Err(Invalid::Tags);
        }
        Ok((name, description, tags))
    }

    pub fn create_sticker(&mut self, guild_id: u64, name: &str, description: &str, tags: &str, file: StickerFile) -> Result<(), Invalid> {
        let (name, description, tags) = Self::sticker_fields(name, description, tags)?;
        if self.stickers.guild(guild_id).len() >= self.expr_slots(guild_id).stickers {
            return Err(Invalid::NoSlot);
        }
        self.expr_cmd(ExprCmd::CreateSticker { guild_id, name, description, tags, file })
    }

    pub fn edit_sticker(&mut self, guild_id: u64, id: u64, name: &str, description: &str, tags: &str) -> Result<(), Invalid> {
        let (name, description, tags) = Self::sticker_fields(name, description, tags)?;
        self.expr_cmd(ExprCmd::EditSticker { guild_id, id, name, description, tags })
    }

    pub fn create_sound(&mut self, guild_id: u64, name: &str, sound: String, volume: u8, emoji: Option<SoundEmoji>) -> Result<(), Invalid> {
        let name = valid_name(name, SOUND_NAME_MAX, false).ok_or(Invalid::Name)?;
        let used = self.expressions.sounds(guild_id).map_or(0, <[Sound]>::len);
        if used >= self.expr_slots(guild_id).sounds {
            return Err(Invalid::NoSlot);
        }
        self.expr_cmd(ExprCmd::CreateSound { guild_id, name, sound, volume: volume.min(100), emoji })
    }

    pub fn edit_sound(&mut self, guild_id: u64, id: u64, name: &str, volume: u8, emoji: Option<SoundEmoji>) -> Result<(), Invalid> {
        let name = valid_name(name, SOUND_NAME_MAX, false).ok_or(Invalid::Name)?;
        self.expr_cmd(ExprCmd::EditSound { guild_id, id, name, volume: volume.min(100), emoji })
    }

    /// Supprime un élément (après confirmation de l'utilisateur).
    pub fn delete_expression(&mut self, guild_id: u64, kind: Kind, id: u64) -> Result<(), Invalid> {
        self.expr_cmd(ExprCmd::Delete { guild_id, kind, id })
    }

    pub(in crate::state) fn apply_expressions(&mut self, e: ExprEvent) -> Option<Effect> {
        let x = &mut self.expressions;
        match e {
            ExprEvent::Emojis { guild_id, emojis, authors } => {
                x.busy.remove(&Busy { guild_id, kind: Kind::Emoji, op: Op::Load, item: None });
                x.loaded.insert((guild_id, Kind::Emoji), Loaded::Ready);
                x.emoji_authors.insert(guild_id, authors.into_iter().collect());
                // Réponse de Discord, plus récente que le GUILD_CREATE : même règle que GUILD_EMOJIS_UPDATE.
                return self.apply(Event::GuildEmojis { guild_id, emojis });
            }
            ExprEvent::Stickers { guild_id, list } => {
                x.busy.remove(&Busy { guild_id, kind: Kind::Sticker, op: Op::Load, item: None });
                x.loaded.insert((guild_id, Kind::Sticker), Loaded::Ready);
                x.sticker_info.insert(guild_id, list.into_iter().map(|s| (s.id, s)).collect());
            }
            ExprEvent::Sounds { guild_id, list } => {
                if x.busy.remove(&Busy { guild_id, kind: Kind::Sound, op: Op::Load, item: None }) {
                    x.loaded.insert((guild_id, Kind::Sound), Loaded::Ready);
                }
                // La passerelle n'a pas les auteurs : ceux déjà connus sont gardés.
                let old = x.sounds.remove(&guild_id).unwrap_or_default();
                let list = list
                    .into_iter()
                    .map(|mut s| {
                        if s.author.is_none() {
                            s.author = old.iter().find(|o| o.id == s.id).and_then(|o| o.author.clone());
                        }
                        s
                    })
                    .collect();
                x.sounds.insert(guild_id, list);
            }
            ExprEvent::SoundUpsert { guild_id, mut sound } => {
                // Liste jamais chargée : rien à compléter.
                if let Some(list) = x.sounds.get_mut(&guild_id) {
                    match list.iter_mut().find(|s| s.id == sound.id) {
                        Some(slot) => {
                            if sound.author.is_none() {
                                sound.author = slot.author.take();
                            }
                            *slot = sound;
                        }
                        None => list.push(sound),
                    }
                }
            }
            ExprEvent::SoundDelete { guild_id, id } => {
                if let Some(list) = x.sounds.get_mut(&guild_id) {
                    list.retain(|s| s.id != id);
                }
            }
            ExprEvent::Boost { guild_id, boost } => {
                x.boosts.insert(guild_id, boost);
            }
            ExprEvent::Done { busy, id, author, sticker } => {
                x.busy.remove(&busy);
                if let (Some(id), Some(a)) = (id, author) {
                    match busy.kind {
                        Kind::Emoji => {
                            x.emoji_authors.entry(busy.guild_id).or_default().insert(id, a);
                        }
                        Kind::Sound => {
                            if let Some(s) = x.sounds.get_mut(&busy.guild_id).and_then(|l| l.iter_mut().find(|s| s.id == id)) {
                                s.author = Some(a);
                            }
                        }
                        Kind::Sticker => {}
                    }
                }
                if let Some(info) = sticker {
                    x.sticker_info.entry(busy.guild_id).or_default().insert(info.id, info);
                }
                x.notice(ExprNotice::Done(busy.kind, busy.op));
            }
            ExprEvent::Failed { busy, failure } => {
                x.busy.remove(&busy);
                if busy.op == Op::Load {
                    x.loaded.insert((busy.guild_id, busy.kind), Loaded::Failed(failure.clone()));
                }
                x.notice(ExprNotice::Failed(busy.kind, busy.op, failure));
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Guild, User};

    const G: u64 = 50;

    fn store(me_owner: bool) -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "Serveur".into(), Vec::new());
        g.owner_id = Some(if me_owner { 1 } else { 2 });
        s.apply(Event::Ready { me: User::new(1, "moi".into()), guilds: vec![g], dms: Vec::new() });
        s
    }

    fn emoji(id: u64, animated: bool) -> Emoji {
        Emoji { id, name: format!("e{id}"), animated, available: true, roles: Vec::new() }
    }

    #[test]
    fn slots_follow_boost_level_and_features() {
        let s = |tier, e, st, so| Boost { more_emoji: e, more_stickers: st, more_sounds: so, ..Boost::default() }.slots(tier);
        assert_eq!(s(0, false, false, false), Slots { emoji: 50, stickers: 5, sounds: 8 });
        assert_eq!(s(1, false, false, false), Slots { emoji: 100, stickers: 15, sounds: 24 });
        assert_eq!(s(2, false, false, false), Slots { emoji: 150, stickers: 30, sounds: 36 });
        assert_eq!(s(3, false, false, false), Slots { emoji: 250, stickers: 60, sounds: 48 });
        assert_eq!(s(0, true, true, true), Slots { emoji: 200, stickers: 60, sounds: 96 });
        assert_eq!(s(3, true, false, false).emoji, 250);
        assert_eq!(s(9, false, false, false).emoji, 250, "niveau inconnu : le plus haut");
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(valid_name(" chat_1 ", 32, true).as_deref(), Some("chat_1"));
        assert_eq!(valid_name("é", 32, true), None);
        assert_eq!(valid_name("un nom", 32, true), None);
        assert_eq!(valid_name("un nom", 32, false).as_deref(), Some("un nom"));
        assert_eq!(valid_name(&"a".repeat(33), 32, true), None);
        assert_eq!(name_from_file("mon chat.gif", 32), "mon_chat");
        assert_eq!(name_from_file("x.png", 32), "x_");
        assert_eq!(name_from_file(&format!("{}.png", "a".repeat(40)), 32).len(), 32);
    }

    #[test]
    fn writes_need_permission_and_one_at_a_time() {
        let mut s = store(false);
        // Mon membre est inconnu : droits inconnus, rien n'est proposé ni envoyé.
        assert!(!s.can_manage_expressions(G));
        assert_eq!(s.create_emoji(G, "chat", "data:".into(), false, Vec::new()), Err(Invalid::Forbidden));
        // Permission connue : proposé.
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == G) {
            g.roles = vec![crate::state::model::Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: Permissions::MANAGE_GUILD_EXPRESSIONS, mentionable: false }];
            g.me = Some(crate::state::model::Member::default());
        }
        assert!(s.can_manage_expressions(G));
        assert_eq!(s.create_emoji(G, "x", "data:".into(), false, Vec::new()), Err(Invalid::Name));
        assert_eq!(s.create_emoji(G, "chat", "data:".into(), false, Vec::new()), Ok(()));
        assert_eq!(s.create_emoji(G, "chien", "data:".into(), false, Vec::new()), Err(Invalid::Busy));
        assert_eq!(s.delete_expression(G, Kind::Emoji, 7), Ok(()));
        assert!(s.expressions.item_busy(G, Kind::Emoji, 7));
        let cmds = s.take_commands();
        assert_eq!(cmds.len(), 2);
        assert_eq!(cmds[1], Cmd::Expressions(ExprCmd::Delete { guild_id: G, kind: Kind::Emoji, id: 7 }));
        // Permissions connues sans MANAGE_GUILD_EXPRESSIONS : refusé, rien n'est envoyé.
        let mut s = store(false);
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == G) {
            g.roles = vec![crate::state::model::Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: Permissions::VIEW_CHANNEL, mentionable: false }];
            g.me = Some(crate::state::model::Member::default());
        }
        assert!(!s.can_manage_expressions(G));
        assert_eq!(s.delete_expression(G, Kind::Sound, 1), Err(Invalid::Forbidden));
        assert!(s.take_commands().is_empty());
    }

    #[test]
    fn slots_are_checked_before_sending() {
        let mut s = store(true);
        s.apply(Event::GuildEmojis { guild_id: G, emojis: (0..50).map(|i| emoji(i, false)).collect() });
        assert_eq!(s.emoji_counts(G), (50, 0));
        assert_eq!(s.create_emoji(G, "plein", "data:".into(), false, Vec::new()), Err(Invalid::NoSlot));
        assert_eq!(s.create_emoji(G, "anime", "data:".into(), true, Vec::new()), Ok(()));
        // Niveau 1 : une seule source, `Guild::premium_tier`.
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == G) {
            g.premium_tier = 1;
        }
        s.apply(Event::Expressions(ExprEvent::Done { busy: Busy { guild_id: G, kind: Kind::Emoji, op: Op::Create, item: None }, id: Some(99), author: None, sticker: None }));
        assert_eq!(s.create_emoji(G, "plus", "data:".into(), false, Vec::new()), Ok(()));
        // Sons : 8 emplacements au niveau 0 (ici 1 : 24).
        if let Some(g) = s.guilds.iter_mut().find(|g| g.id == G) {
            g.premium_tier = 0;
        }
        let snd = |id| Sound { id, name: format!("s{id}"), volume: 100, emoji: None, available: true, author: None };
        s.apply(Event::Expressions(ExprEvent::Sounds { guild_id: G, list: (0..8).map(snd).collect() }));
        assert_eq!(s.create_sound(G, "son", "data:".into(), 100, None), Err(Invalid::NoSlot));
        assert_eq!(s.create_sticker(G, "a", "", "smile", StickerFile { name: "a.png".into(), mime: "image/png", bytes: Arc::from(vec![]) }), Err(Invalid::Name));
        assert_eq!(s.create_sticker(G, "ok", "x", "smile", StickerFile { name: "a.png".into(), mime: "image/png", bytes: Arc::from(vec![]) }), Err(Invalid::Description));
        assert_eq!(s.create_sticker(G, "ok", "", "", StickerFile { name: "a.png".into(), mime: "image/png", bytes: Arc::from(vec![]) }), Err(Invalid::Tags));
    }

    #[test]
    fn lists_come_from_rest_then_gateway() {
        let mut s = store(true);
        assert!(s.load_expressions(G, Kind::Emoji));
        assert!(!s.load_expressions(G, Kind::Emoji), "déjà en cours");
        assert_eq!(s.expressions.loaded(G, Kind::Emoji), Some(&Loaded::Loading));
        let alice = Author { id: 3, name: "Alice".into() };
        s.apply(Event::Expressions(ExprEvent::Emojis { guild_id: G, emojis: vec![emoji(1, false), emoji(2, true)], authors: vec![(1, alice.clone())] }));
        assert_eq!(s.expressions.loaded(G, Kind::Emoji), Some(&Loaded::Ready));
        assert_eq!(s.emoji_counts(G), (1, 1));
        assert_eq!(s.expressions.emoji_author(G, 1), Some(&alice));
        // La passerelle remplace la liste ; les auteurs restent.
        s.apply(Event::GuildEmojis { guild_id: G, emojis: vec![emoji(1, false)] });
        assert_eq!(s.emoji_counts(G), (1, 0));
        assert_eq!(s.expressions.emoji_author(G, 1), Some(&alice));

        // Sons : liste, puis événements de la passerelle.
        let snd = |id, author: Option<Author>| Sound { id, name: format!("s{id}"), volume: 50, emoji: Some(SoundEmoji::Unicode("🔥".into())), available: true, author };
        s.apply(Event::Expressions(ExprEvent::SoundUpsert { guild_id: G, sound: snd(9, None) }));
        assert_eq!(s.expressions.sounds(G), None, "jamais chargée : rien d'inventé");
        s.apply(Event::Expressions(ExprEvent::Sounds { guild_id: G, list: vec![snd(1, Some(alice.clone()))] }));
        s.apply(Event::Expressions(ExprEvent::SoundUpsert { guild_id: G, sound: Sound { name: "renommé".into(), ..snd(1, None) } }));
        s.apply(Event::Expressions(ExprEvent::SoundUpsert { guild_id: G, sound: snd(2, None) }));
        let list = s.expressions.sounds(G).unwrap_or_default();
        assert_eq!((list.len(), list[0].name.as_str(), list[0].author.as_ref()), (2, "renommé", Some(&alice)));
        s.apply(Event::Expressions(ExprEvent::SoundDelete { guild_id: G, id: 1 }));
        assert_eq!(s.expressions.sounds(G).map(<[Sound]>::len), Some(1));

        // Échec d'un chargement : affiché à la place de la liste.
        assert!(s.load_expressions(G, Kind::Sticker));
        let busy = Busy { guild_id: G, kind: Kind::Sticker, op: Op::Load, item: None };
        s.apply(Event::Expressions(ExprEvent::Failed { busy, failure: Failure::Problem(Problem::MfaRequired) }));
        assert_eq!(s.expressions.loaded(G, Kind::Sticker), Some(&Loaded::Failed(Failure::Problem(Problem::MfaRequired))));
        assert_eq!(s.expressions.take_notice(), Some(ExprNotice::Failed(Kind::Sticker, Op::Load, Failure::Problem(Problem::MfaRequired))));
        s.apply(Event::GuildDelete(G));
        assert_eq!(s.expressions.sounds(G), None);
    }
}
