//! Réglages connus de `PreloadedUserSettings` (type 1) : où ils sont dans le
//! message et comment ils sont codés.
//!
//! Source des numéros de champ : définitions protobuf publiées par la
//! communauté, `discord-userdoccers/discord-protos`,
//! `discord_protos/discord_users/v1/PreloadedUserSettings.proto` (lues le
//! 13 novembre 2026), et docs.discord.food, « User Settings Proto ».

use super::proto::{wrapper, DecodeError, Msg, Value};

/// Champs de premier niveau de `PreloadedUserSettings`.
pub mod top {
    pub const VERSIONS: u32 = 1;
    pub const TEXT_AND_IMAGES: u32 = 6;
    pub const NOTIFICATIONS: u32 = 7;
    pub const PRIVACY: u32 = 8;
    pub const STATUS: u32 = 11;
    pub const LOCALIZATION: u32 = 12;
    pub const APPEARANCE: u32 = 13;
    pub const GUILD_FOLDERS: u32 = 14;
}

/// Codage d'un réglage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `google.protobuf.BoolValue`, avec la valeur que Discord prend quand il est absent.
    BoolWrap(bool),
    /// `bool` simple (absent = faux).
    Bool,
    /// `google.protobuf.UInt32Value`, avec sa valeur par défaut.
    U32Wrap(u32),
    /// `google.protobuf.StringValue`.
    StrWrap,
    /// Énumération (absente = 0).
    Enum,
}

/// Valeur d'un réglage, telle que codée (une négation éventuelle est faite
/// par l'interface, qui choisit le libellé).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SValue {
    Bool(bool),
    U32(u32),
    Str(String),
    Enum(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    // Confidentialité (`privacy`).
    GuildDmsRestricted,
    MessageRequestsRestricted,
    FriendSources,
    ShowCurrentGame,
    // Filtre de contenu (`text_and_images`).
    ExplicitGuilds,
    ExplicitFriendDm,
    ExplicitOtherDm,
    DmSpamFilter,
    // Texte et images.
    InlineAttachments,
    InlineEmbeds,
    RenderEmbeds,
    RenderReactions,
    AnimateEmoji,
    GifAutoPlay,
    AnimateStickers,
    ConvertEmoticons,
    ImageDescriptions,
    CommandSuggestions,
    TtsCommand,
    MessageCompact,
    // Notifications du compte.
    InAppNotifications,
    GoLiveNotify,
    BurstReactionNotify,
    // Statut, langue, apparence.
    Status,
    Locale,
    Theme,
    // Paramètres « Avancé », « Données et confidentialité », « Activités ».
    DeveloperMode,
    AccessibilityDetection,
    ActivityJoinFriends,
    ActivityJoinVoice,
    ActivityGuildsDefault,
}

impl Key {
    /// Chemin (numéros de champ depuis la racine) et codage.
    pub const fn def(self) -> (&'static [u32], Kind) {
        use top::*;
        use Kind::*;
        match self {
            Key::GuildDmsRestricted => (&[PRIVACY, 4], Bool),
            Key::MessageRequestsRestricted => (&[PRIVACY, 17], BoolWrap(false)),
            // Absent : toutes les sources (1<<1 | 1<<2 | 1<<3).
            Key::FriendSources => (&[PRIVACY, 11], U32Wrap(FRIEND_ALL)),
            Key::ShowCurrentGame => (&[STATUS, 3], BoolWrap(true)),
            Key::ExplicitGuilds => (&[TEXT_AND_IMAGES, 29, 1], Enum),
            Key::ExplicitFriendDm => (&[TEXT_AND_IMAGES, 29, 2], Enum),
            Key::ExplicitOtherDm => (&[TEXT_AND_IMAGES, 29, 3], Enum),
            Key::DmSpamFilter => (&[TEXT_AND_IMAGES, 27], Enum),
            Key::InlineAttachments => (&[TEXT_AND_IMAGES, 9], BoolWrap(true)),
            Key::InlineEmbeds => (&[TEXT_AND_IMAGES, 10], BoolWrap(true)),
            Key::RenderEmbeds => (&[TEXT_AND_IMAGES, 12], BoolWrap(true)),
            Key::RenderReactions => (&[TEXT_AND_IMAGES, 13], BoolWrap(true)),
            Key::AnimateEmoji => (&[TEXT_AND_IMAGES, 14], BoolWrap(true)),
            Key::GifAutoPlay => (&[TEXT_AND_IMAGES, 11], BoolWrap(true)),
            Key::AnimateStickers => (&[TEXT_AND_IMAGES, 15], U32Wrap(0)),
            Key::ConvertEmoticons => (&[TEXT_AND_IMAGES, 21], BoolWrap(true)),
            Key::ImageDescriptions => (&[TEXT_AND_IMAGES, 7], BoolWrap(false)),
            Key::CommandSuggestions => (&[TEXT_AND_IMAGES, 8], BoolWrap(true)),
            Key::TtsCommand => (&[TEXT_AND_IMAGES, 16], BoolWrap(true)),
            Key::MessageCompact => (&[TEXT_AND_IMAGES, 17], BoolWrap(false)),
            Key::InAppNotifications => (&[NOTIFICATIONS, 1], BoolWrap(true)),
            Key::GoLiveNotify => (&[NOTIFICATIONS, 2], BoolWrap(true)),
            Key::BurstReactionNotify => (&[NOTIFICATIONS, 4], BoolWrap(true)),
            Key::Status => (&[STATUS, 1], StrWrap),
            Key::Locale => (&[LOCALIZATION, 1], StrWrap),
            Key::Theme => (&[APPEARANCE, 1], Enum),
            Key::DeveloperMode => (&[APPEARANCE, 2], Bool),
            Key::AccessibilityDetection => (&[PRIVACY, 7], Bool),
            // Absent : on (ce que montre l'application officielle sur un compte neuf).
            Key::ActivityJoinFriends => (&[PRIVACY, 1], BoolWrap(true)),
            Key::ActivityJoinVoice => (&[PRIVACY, 2], BoolWrap(true)),
            // 0 : partagée partout, 1 : pas dans les grands serveurs, 2 : nulle part.
            Key::ActivityGuildsDefault => (&[PRIVACY, 14], Enum),
        }
    }

    pub fn top(self) -> u32 {
        self.def().0[0]
    }
}

/// `friend_source_flags` : amis d'amis, membres des serveurs, tout le monde.
pub const FRIEND_MUTUAL_FRIENDS: u32 = 1 << 1;
pub const FRIEND_MUTUAL_GUILDS: u32 = 1 << 2;
pub const FRIEND_NO_RELATION: u32 = 1 << 3;
pub const FRIEND_ALL: u32 = FRIEND_MUTUAL_FRIENDS | FRIEND_MUTUAL_GUILDS | FRIEND_NO_RELATION;

/// Valeur d'un réglage dans `root` (valeur par défaut s'il est absent).
pub fn read(root: &Msg, key: Key) -> Result<SValue, DecodeError> {
    let (path, kind) = key.def();
    let (leaf, parents) = path.split_last().ok_or(DecodeError::FieldZero)?;
    let parent = root.at_path(parents)?.unwrap_or_default();
    Ok(match kind {
        Kind::Bool => SValue::Bool(parent.varint(*leaf).unwrap_or(0) != 0),
        Kind::Enum => SValue::Enum(parent.varint(*leaf).unwrap_or(0)),
        Kind::BoolWrap(d) => SValue::Bool(match parent.message(*leaf)? {
            Some(w) => w.varint(1).unwrap_or(0) != 0,
            None => d,
        }),
        Kind::U32Wrap(d) => SValue::U32(match parent.message(*leaf)? {
            Some(w) => u32::try_from(w.varint(1).unwrap_or(0)).unwrap_or(u32::MAX),
            None => d,
        }),
        Kind::StrWrap => SValue::Str(parent.message(*leaf)?.and_then(|w| w.string(1)).unwrap_or_default()),
    })
}

/// Écrit `value` dans `root` ; seul ce champ change. Une valeur d'un autre
/// type que celui du réglage est refusée.
pub fn write(root: &mut Msg, key: Key, value: &SValue) -> Result<(), DecodeError> {
    let (path, kind) = key.def();
    let (leaf, parents) = path.split_last().ok_or(DecodeError::FieldZero)?;
    let new = match (kind, value) {
        (Kind::Bool, SValue::Bool(b)) => Some(Value::Varint(u64::from(*b))),
        (Kind::Enum, SValue::Enum(v)) => Some(Value::Varint(*v)),
        (Kind::BoolWrap(_), SValue::Bool(b)) => Some(Value::Len(wrapper(Value::Varint(u64::from(*b))).encode())),
        (Kind::U32Wrap(_), SValue::U32(v)) => Some(Value::Len(wrapper(Value::Varint(u64::from(*v))).encode())),
        (Kind::StrWrap, SValue::Str(s)) => Some(Value::Len(wrapper(Value::Len(s.as_bytes().to_vec())).encode())),
        _ => None,
    };
    let Some(new) = new else { return Err(DecodeError::WireType(0)) };
    let leaf = *leaf;
    root.edit_path(parents, move |m| {
        // Scalaire à zéro : omis, comme le fait l'encodeur officiel.
        if matches!((kind, &new), (Kind::Bool | Kind::Enum, Value::Varint(0))) {
            m.remove(leaf);
        } else {
            m.set(leaf, new);
        }
    })
}

/// Statut personnalisé (`status.custom_status`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CustomStatus {
    pub text: String,
    /// Emoji Unicode (`emoji_name`) ; un emoji personnalisé a aussi un identifiant.
    pub emoji_name: String,
    pub emoji_id: u64,
    /// Fin (ms Unix), 0 : jamais.
    pub expires_at_ms: u64,
}

impl CustomStatus {
    pub fn is_empty(&self) -> bool {
        self.text.is_empty() && self.emoji_name.is_empty() && self.emoji_id == 0
    }

    pub fn expired(&self, now_ms: i64) -> bool {
        self.expires_at_ms != 0 && i64::try_from(self.expires_at_ms).unwrap_or(i64::MAX) <= now_ms
    }
}

pub fn read_custom_status(root: &Msg) -> Result<Option<CustomStatus>, DecodeError> {
    let Some(m) = root.at_path(&[top::STATUS, 2])? else { return Ok(None) };
    Ok(Some(CustomStatus {
        text: m.string(1).unwrap_or_default(),
        emoji_id: m.fixed64(2).unwrap_or(0),
        emoji_name: m.string(3).unwrap_or_default(),
        expires_at_ms: m.fixed64(4).unwrap_or(0),
    }))
}

/// Remplace le statut personnalisé (`None` : retiré). Les champs non gérés
/// ici (libellé…) d'un statut existant sont gardés quand on le modifie.
pub fn write_custom_status(root: &mut Msg, status: Option<&CustomStatus>, now_ms: i64) -> Result<(), DecodeError> {
    root.edit_path(&[top::STATUS], |s| match status {
        None => s.remove(2),
        Some(c) => {
            let mut m = s.message(2).ok().flatten().unwrap_or_default();
            let set_str = |m: &mut Msg, n: u32, v: &str| if v.is_empty() { m.remove(n) } else { m.set(n, Value::Len(v.as_bytes().to_vec())) };
            let set_fix = |m: &mut Msg, n: u32, v: u64| if v == 0 { m.remove(n) } else { m.set(n, Value::I64(v)) };
            set_str(&mut m, 1, &c.text);
            set_fix(&mut m, 2, c.emoji_id);
            set_str(&mut m, 3, &c.emoji_name);
            set_fix(&mut m, 4, c.expires_at_ms);
            set_fix(&mut m, 5, u64::try_from(now_ms).unwrap_or(0));
            s.set(2, Value::Len(m.encode()));
        }
    })
}

/// Dossier de serveurs (`guild_folders.folders`) ; un serveur hors dossier
/// est un dossier anonyme d'un seul serveur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Folder {
    pub id: Option<i64>,
    pub name: Option<String>,
    pub color: Option<u64>,
    pub guilds: Vec<u64>,
}

pub fn read_folders(root: &Msg) -> Result<Vec<Folder>, DecodeError> {
    let Some(f) = root.message(top::GUILD_FOLDERS)? else { return Ok(Vec::new()) };
    let mut out = Vec::new();
    for field in f.fields.iter().filter(|x| x.num == 1) {
        let Value::Len(b) = &field.value else { continue };
        let m = Msg::decode(b)?;
        out.push(Folder {
            guilds: m.repeated_fixed64(1),
            id: m.message(2)?.and_then(|w| w.varint(1)).map(|v| v as i64),
            name: m.message(3)?.and_then(|w| w.string(1)).filter(|s| !s.is_empty()),
            color: m.message(4)?.and_then(|w| w.varint(1)),
        });
    }
    Ok(out)
}

/// `versions.data_version` : sert à `required_data_version` à l'écriture.
pub fn data_version(root: &Msg) -> Option<u32> {
    root.message(top::VERSIONS).ok().flatten()?.varint(3).and_then(|v| u32::try_from(v).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bool_wrap(v: bool) -> Value {
        Value::Len(wrapper(Value::Varint(u64::from(v))).encode())
    }

    #[test]
    fn absent_settings_take_discord_defaults() {
        let m = Msg::default();
        assert_eq!(read(&m, Key::InlineEmbeds).unwrap(), SValue::Bool(true));
        assert_eq!(read(&m, Key::MessageCompact).unwrap(), SValue::Bool(false));
        assert_eq!(read(&m, Key::GuildDmsRestricted).unwrap(), SValue::Bool(false));
        assert_eq!(read(&m, Key::FriendSources).unwrap(), SValue::U32(FRIEND_ALL));
        assert_eq!(read(&m, Key::Locale).unwrap(), SValue::Str(String::new()));
        assert_eq!(read(&m, Key::ExplicitOtherDm).unwrap(), SValue::Enum(0));
        assert_eq!(read_custom_status(&m).unwrap(), None);
        assert!(read_folders(&m).unwrap().is_empty());
    }

    #[test]
    fn write_then_read_every_kind() {
        let mut m = Msg::default();
        for (k, v) in [
            (Key::InlineEmbeds, SValue::Bool(false)),
            (Key::GuildDmsRestricted, SValue::Bool(true)),
            (Key::FriendSources, SValue::U32(FRIEND_MUTUAL_GUILDS)),
            (Key::Locale, SValue::Str("fr".into())),
            (Key::ExplicitGuilds, SValue::Enum(2)),
            (Key::Status, SValue::Str("dnd".into())),
        ] {
            write(&mut m, k, &v).unwrap();
            assert_eq!(read(&m, k).unwrap(), v, "{k:?}");
        }
        assert!(write(&mut m, Key::InlineEmbeds, &SValue::Str("x".into())).is_err());
        // Faux dans une enveloppe : enveloppe présente mais vide (pas « absent »).
        let t = m.message(top::TEXT_AND_IMAGES).unwrap().unwrap();
        assert_eq!(t.bytes(10), Some(&[][..]));
        // Booléen simple remis à faux : champ omis.
        write(&mut m, Key::GuildDmsRestricted, &SValue::Bool(false)).unwrap();
        assert!(!m.message(top::PRIVACY).unwrap().unwrap().has(4));
    }

    #[test]
    fn writing_one_setting_keeps_its_neighbours_and_unknowns() {
        let mut text = Msg::default();
        text.set(9, bool_wrap(false));
        text.set(777, Value::Len(b"futur".to_vec()));
        let mut root = Msg::default();
        root.set(top::TEXT_AND_IMAGES, Value::Len(text.encode()));
        root.set(99, Value::Varint(42));
        write(&mut root, Key::GifAutoPlay, &SValue::Bool(false)).unwrap();
        let t = root.message(top::TEXT_AND_IMAGES).unwrap().unwrap();
        assert_eq!(t.bytes(777), Some(&b"futur"[..]));
        assert_eq!(read(&root, Key::InlineAttachments).unwrap(), SValue::Bool(false));
        assert_eq!(read(&root, Key::GifAutoPlay).unwrap(), SValue::Bool(false));
        assert_eq!(root.varint(99), Some(42));
    }

    #[test]
    fn custom_status_round_trip_keeps_label() {
        let mut cs = Msg::default();
        cs.set(1, Value::Len(b"ancien".to_vec()));
        cs.set(6, Value::Len(wrapper(Value::Len(b"libelle".to_vec())).encode()));
        let mut status = Msg::default();
        status.set(1, Value::Len(wrapper(Value::Len(b"idle".to_vec())).encode()));
        status.set(2, Value::Len(cs.encode()));
        let mut root = Msg::default();
        root.set(top::STATUS, Value::Len(status.encode()));
        let new = CustomStatus { text: "En pause".into(), emoji_name: "☕".into(), emoji_id: 0, expires_at_ms: 1_700_000_000_000 };
        write_custom_status(&mut root, Some(&new), 1_600_000_000_000).unwrap();
        assert_eq!(read_custom_status(&root).unwrap(), Some(new.clone()));
        let raw = root.at_path(&[top::STATUS, 2]).unwrap().unwrap();
        assert!(raw.has(6), "libellé inconnu gardé");
        assert_eq!(raw.fixed64(5), Some(1_600_000_000_000));
        assert_eq!(read(&root, Key::Status).unwrap(), SValue::Str("idle".into()));
        write_custom_status(&mut root, None, 0).unwrap();
        assert_eq!(read_custom_status(&root).unwrap(), None);
        assert!(new.expired(1_800_000_000_000) && !new.expired(1_600_000_000_000));
    }

    #[test]
    fn folders_and_version() {
        let mut ids = Vec::new();
        ids.extend_from_slice(&5u64.to_le_bytes());
        ids.extend_from_slice(&6u64.to_le_bytes());
        let mut folder = Msg::default();
        folder.set(1, Value::Len(ids));
        folder.set(2, Value::Len(wrapper(Value::Varint(77)).encode()));
        folder.set(3, Value::Len(wrapper(Value::Len(b"Jeux".to_vec())).encode()));
        let mut lone = Msg::default();
        lone.set(1, Value::I64(9));
        let mut gf = Msg::default();
        gf.fields.push(super::super::proto::Field { num: 1, value: Value::Len(folder.encode()) });
        gf.fields.push(super::super::proto::Field { num: 1, value: Value::Len(lone.encode()) });
        let mut versions = Msg::default();
        versions.set(3, Value::Varint(12));
        let mut root = Msg::default();
        root.set(top::GUILD_FOLDERS, Value::Len(gf.encode()));
        root.set(top::VERSIONS, Value::Len(versions.encode()));
        let f = read_folders(&root).unwrap();
        assert_eq!(f.len(), 2);
        assert_eq!((f[0].id, f[0].name.as_deref(), f[0].guilds.clone()), (Some(77), Some("Jeux"), vec![5, 6]));
        assert_eq!((f[1].id, f[1].guilds.clone()), (None, vec![9]));
        assert_eq!(data_version(&root), Some(12));
    }
}
