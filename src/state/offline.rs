//! Historique hors ligne (lot H) : ce qui est gardé sur le disque des salons
//! récemment ouverts, pour l'afficher au démarrage avant READY, et sans réseau.
//!
//! Ici, seulement le modèle et sa forme texte (JSON) : aucun disque, aucun
//! chiffrement (`crate::offline` s'en charge, dans son fil). Ce qui est
//! gardé suffit à l'affichage : moi, les serveurs et leurs salons visibles,
//! les MP, et au plus [`MAX_PER_CHANNEL`] messages de [`MAX_CHANNELS`]
//! salons, le tout sous [`MAX_BYTES`]. Aucun jeton, aucune permission, aucun
//! rôle : hors ligne, les permissions sont inconnues, et seuls les salons
//! visibles au moment de l'enregistrement y figurent.

use super::model::{Attachment, Avatar, Channel, ChannelKind, Dm, Guild, ImageHash, ImageRef, Message, User};
use super::store::{ChanState, Store};
use serde::{Deserialize, Serialize};

/// Version du format : un autre numéro est ignoré (rien n'est affiché).
pub const VERSION: u32 = 1;
/// Salons gardés au plus, les plus récemment ouverts d'abord : autant que
/// le magasin en garde en mémoire.
pub const MAX_CHANNELS: usize = super::store::MAX_CACHED_CHANNELS;
/// Messages gardés par salon au plus (les plus récents) : une page.
pub const MAX_PER_CHANNEL: usize = super::HISTORY_PAGE;
/// Taille du texte au plus : au-delà, les salons les moins récents partent.
pub const MAX_BYTES: usize = 4 << 20;

/// Instantané gardé pour un compte.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Snapshot {
    pub version: u32,
    pub me: SUser,
    pub guilds: Vec<SGuild>,
    pub dms: Vec<SDm>,
    /// Serveur et salon affichés à l'enregistrement.
    pub guild: Option<u64>,
    pub channel: Option<u64>,
    /// Du plus récemment ouvert au moins récent.
    pub chans: Vec<SChan>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SUser {
    pub id: u64,
    pub name: String,
    pub avatar: SAvatar,
}

/// Avatar : empreinte de l'image (texte de Discord, relu par `ImageHash::parse`)
/// et numéro de l'avatar par défaut.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SAvatar {
    /// `u` : utilisateur, `g` : icône de groupe ; propriétaire de l'image.
    pub of: Option<(char, u64)>,
    pub hash: Option<String>,
    pub default: u8,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SGuild {
    pub id: u64,
    pub name: String,
    pub icon: Option<String>,
    pub channels: Vec<SChannel>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SChannel {
    pub id: u64,
    pub name: String,
    /// 0 texte, 1 annonces, 2 catégorie, 3 forum, 4 média.
    pub kind: u8,
    pub position: i32,
    pub parent: Option<u64>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SDm {
    pub id: u64,
    pub name: String,
    pub last_message: u64,
    pub avatar: SAvatar,
    pub recipients: Vec<(u64, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SChan {
    pub id: u64,
    /// Des messages plus anciens existent (chargés en remontant, une fois en ligne).
    pub more: bool,
    pub msgs: Vec<SMsg>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SMsg {
    pub id: u64,
    pub guild: Option<u64>,
    pub author_id: u64,
    pub author: String,
    pub avatar: SAvatar,
    pub content: String,
    pub mentions: Vec<(u64, String)>,
    pub everyone: bool,
    pub edited: bool,
    pub files: Vec<SFile>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SFile {
    pub id: u64,
    pub name: String,
    pub url: String,
    pub proxy_url: String,
    pub size: u64,
    pub content_type: Option<String>,
    pub width: u32,
    pub height: u32,
    pub description: Option<String>,
    pub spoiler: bool,
    pub animated: bool,
}

impl SAvatar {
    fn of(a: &Avatar) -> SAvatar {
        let (of, hash) = match a.image {
            Some(ImageRef::Avatar { user_id, hash }) => (Some(('u', user_id)), Some(hash.to_string())),
            Some(ImageRef::GroupIcon { channel_id, hash }) => (Some(('g', channel_id)), Some(hash.to_string())),
            _ => (None, None),
        };
        SAvatar { of, hash, default: a.default }
    }

    fn avatar(&self) -> Avatar {
        let hash = self.hash.as_deref().and_then(ImageHash::parse);
        let image = match (self.of, hash) {
            (Some(('u', user_id)), Some(hash)) => Some(ImageRef::Avatar { user_id, hash }),
            (Some(('g', channel_id)), Some(hash)) => Some(ImageRef::GroupIcon { channel_id, hash }),
            _ => None,
        };
        Avatar { image, default: self.default % super::model::DEFAULT_AVATARS }
    }
}

fn kind_code(k: ChannelKind) -> u8 {
    match k {
        ChannelKind::Text => 0,
        ChannelKind::Announcement => 1,
        ChannelKind::Category => 2,
        ChannelKind::Forum => 3,
        ChannelKind::Media => 4,
    }
}

fn kind_of(code: u8) -> ChannelKind {
    match code {
        1 => ChannelKind::Announcement,
        2 => ChannelKind::Category,
        3 => ChannelKind::Forum,
        4 => ChannelKind::Media,
        _ => ChannelKind::Text,
    }
}

impl SFile {
    fn of(a: &Attachment) -> SFile {
        SFile {
            id: a.id,
            name: a.name.clone(),
            url: a.url.clone(),
            proxy_url: a.proxy_url.clone(),
            size: a.size,
            content_type: a.content_type.clone(),
            width: a.width,
            height: a.height,
            description: a.description.clone(),
            spoiler: a.spoiler,
            animated: a.animated,
        }
    }

    fn attachment(&self) -> Attachment {
        let a = Attachment {
            id: self.id,
            name: self.name.clone(),
            url: self.url.clone(),
            proxy_url: self.proxy_url.clone(),
            size: self.size,
            content_type: self.content_type.clone(),
            width: self.width,
            height: self.height,
            description: self.description.clone(),
            ..Attachment::default()
        }
        .analysed(0);
        Attachment { spoiler: a.spoiler || self.spoiler, animated: a.animated || self.animated, ..a }
    }
}

impl SMsg {
    pub(crate) fn of(m: &Message, raw: Option<&String>) -> SMsg {
        SMsg {
            id: m.id,
            guild: m.guild_id,
            author_id: m.author_id,
            author: m.author.clone(),
            avatar: SAvatar::of(&m.author_avatar),
            content: raw.unwrap_or(&m.content).clone(),
            mentions: m.mentions.clone(),
            everyone: m.mention_everyone,
            edited: m.edited,
            files: m.attachments.iter().map(SFile::of).collect(),
        }
    }

    /// Message affichable (sans réactions, réponse ni embeds : ils arrivent avec la page fraîche).
    pub fn message(&self, channel_id: u64) -> Message {
        Message {
            id: self.id,
            channel_id,
            guild_id: self.guild,
            author_id: self.author_id,
            author: self.author.clone(),
            author_avatar: self.avatar.avatar(),
            content: self.content.clone(),
            attachments: self.files.iter().map(SFile::attachment).collect(),
            embeds: Vec::new(),
            mentions: self.mentions.clone(),
            mention_everyone: self.everyone,
            edited: self.edited,
            system: None,
            meta: Default::default(),
        }
    }
}

impl Snapshot {
    /// Instantané du magasin ; `None` avant toute session (rien à garder).
    pub fn capture(store: &Store) -> Option<Snapshot> {
        let me = store.me.as_ref()?;
        let guilds = store
            .guilds
            .iter()
            .map(|g| SGuild {
                id: g.id,
                name: g.name.clone(),
                icon: match g.icon {
                    Some(ImageRef::GuildIcon { hash, .. }) => Some(hash.to_string()),
                    _ => None,
                },
                channels: g
                    .channels
                    .iter()
                    .filter(|c| !store.is_hidden(c.id))
                    .map(|c| SChannel { id: c.id, name: c.name.clone(), kind: kind_code(c.kind), position: c.position, parent: c.parent })
                    .collect(),
            })
            .collect();
        let dms = store
            .dms
            .iter()
            .map(|d| SDm { id: d.id, name: d.name.clone(), last_message: d.last_message, avatar: SAvatar::of(&d.avatar), recipients: d.recipients.clone() })
            .collect();
        let mut open: Vec<(u64, &ChanState)> = store
            .chans
            .iter()
            .filter(|(id, cs)| cs.loaded && !cs.history_denied && !cs.msgs.is_empty() && store.channel_exists(**id) && !store.is_hidden(**id))
            .map(|(id, cs)| (*id, cs))
            .collect();
        // Le salon affiché d'abord, puis du plus récemment ouvert au moins récent.
        open.sort_by_key(|(id, cs)| (Some(*id) != store.channel, std::cmp::Reverse(cs.last_view), *id));
        let chans = open
            .into_iter()
            .take(MAX_CHANNELS)
            .map(|(id, cs)| {
                let keep: Vec<&Message> = cs.msgs.iter().filter(|m| m.system.is_none() && !m.meta.is_gone()).collect();
                let cut = keep.len().saturating_sub(MAX_PER_CHANNEL);
                SChan { id, more: cs.more || cut > 0, msgs: keep[cut..].iter().map(|m| SMsg::of(m, cs.stamped.get(&m.id))).collect() }
            })
            .collect();
        Some(Snapshot { version: VERSION, me: SUser { id: me.id, name: me.name.clone(), avatar: SAvatar::of(&me.avatar) }, guilds, dms, guild: store.guild, channel: store.channel, chans })
    }

    /// Texte à enregistrer, sous [`MAX_BYTES`] : les salons les moins récents
    /// partent tant qu'il dépasse.
    pub fn encode(mut self) -> Vec<u8> {
        loop {
            let bytes = serde_json::to_vec(&self).unwrap_or_default();
            if bytes.len() <= MAX_BYTES || self.chans.is_empty() {
                return bytes;
            }
            // Retrait par moitié de l'excédent estimé : pas de boucle quadratique.
            let over = bytes.len() - MAX_BYTES;
            let per = bytes.len() / self.chans.len().max(1);
            let drop = (over / per.max(1)).max(1).min(self.chans.len());
            self.chans.truncate(self.chans.len() - drop);
        }
    }

    /// Relit un instantané ; abîmé ou d'une autre version : `None`.
    pub fn decode(bytes: &[u8]) -> Option<Snapshot> {
        let s: Snapshot = serde_json::from_slice(bytes).ok()?;
        (s.version == VERSION && s.me.id != 0).then_some(s)
    }

    pub fn user(&self) -> User {
        User { id: self.me.id, name: self.me.name.clone(), avatar: self.me.avatar.avatar() }
    }

    pub fn guild_models(&self) -> Vec<Guild> {
        self.guilds
            .iter()
            .map(|g| {
                let channels = g
                    .channels
                    .iter()
                    // Hors ligne, permissions inconnues : `None` garde le salon visible.
                    .map(|c| Channel { id: c.id, name: c.name.clone(), kind: kind_of(c.kind), position: c.position, parent: c.parent, overwrites: None, slowmode: 0 })
                    .collect();
                let mut model = Guild::new(g.id, g.name.clone(), channels);
                model.icon = g.icon.as_deref().and_then(ImageHash::parse).map(|hash| ImageRef::GuildIcon { guild_id: g.id, hash });
                model
            })
            .collect()
    }

    pub fn dm_models(&self) -> Vec<Dm> {
        self.dms
            .iter()
            .map(|d| Dm { id: d.id, name: d.name.clone(), last_message: d.last_message, avatar: d.avatar.avatar(), recipients: d.recipients.clone() })
            .collect()
    }
}

/// Instantané déjà converti en modèles (lot C9) : la conversion (un millier
/// de messages, tous les salons) se fait dans le fil de l'historique, et
/// l'image qui l'affiche ne fait plus que le ranger dans le magasin.
#[derive(Debug)]
pub struct Prepared {
    pub me: User,
    pub guilds: Vec<Guild>,
    pub dms: Vec<Dm>,
    pub guild: Option<u64>,
    pub channel: Option<u64>,
    /// Salon, reste-t-il plus ancien, messages triés sans doublon ; du plus
    /// récemment ouvert au moins récent.
    pub chans: Vec<(u64, bool, Vec<Message>)>,
}

impl Snapshot {
    pub fn prepare(&self) -> Prepared {
        let chans = self
            .chans
            .iter()
            .map(|c| {
                let mut msgs: Vec<Message> = c.msgs.iter().map(|m| m.message(c.id)).collect();
                msgs.sort_by_key(|m| m.id);
                msgs.dedup_by_key(|m| m.id);
                (c.id, c.more, msgs)
            })
            .collect();
        Prepared { me: self.user(), guilds: self.guild_models(), dms: self.dm_models(), guild: self.guild, channel: self.channel, chans }
    }
}

/// Instantané affiché avant READY, et pour quel compte (`Store::offline`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Shown {
    pub user: Option<u64>,
}
