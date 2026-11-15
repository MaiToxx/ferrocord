//! Modèle affiché par l'interface. Les identifiants (snowflakes) sont
//! stockés en `u64`, pas en `String`.

use super::notice::SystemKind;
use super::perms::Permissions;
pub use super::image::{default_avatar_index, Avatar, ImageHash, ImageRef, DEFAULT_AVATARS};
pub use super::media::{Attachment, Embed, EmbedAuthor, EmbedField, EmbedFooter, EmbedKind, EmbedMedia, EmbedProvider, EmbedSlot, MediaKind};
use std::collections::HashMap;

pub const DISCORD_EPOCH: u64 = 1_420_070_400_000;

#[derive(Clone, Debug)]
pub struct User {
    pub id: u64,
    pub name: String,
    pub avatar: Avatar,
}

impl User {
    /// Utilisateur sans avatar personnalisé (avatar par défaut tiré de l'identifiant).
    pub fn new(id: u64, name: String) -> Self {
        User { id, name, avatar: Avatar::default_for(id) }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelKind {
    Text,
    Announcement,
    Category,
    /// Salon forum (15) : des publications (fils), pas de messages.
    Forum,
    /// Salon média (16) : un forum de publications illustrées.
    Media,
}

impl ChannelKind {
    /// Forum ou média : on y lit des publications, pas des messages.
    pub fn is_forum(self) -> bool {
        matches!(self, ChannelKind::Forum | ChannelKind::Media)
    }
}

#[derive(Clone, Debug)]
pub struct Channel {
    pub id: u64,
    pub name: String,
    pub kind: ChannelKind,
    pub position: i32,
    pub parent: Option<u64>,
    /// Dérogations de permissions du salon ; `None` si elles n'ont pas pu
    /// être lues (le salon reste alors visible et utilisable).
    pub overwrites: Option<Vec<Overwrite>>,
    /// Mode lent : secondes à attendre entre deux messages (`rate_limit_per_user`), 0 sans.
    pub slowmode: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OverwriteKind {
    Role,
    Member,
}

/// Dérogation de permissions d'un salon, pour un rôle ou pour un membre.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Overwrite {
    pub id: u64,
    pub kind: OverwriteKind,
    pub allow: Permissions,
    pub deny: Permissions,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Role {
    pub id: u64,
    pub name: String,
    /// 0xRRGGBB, 0 = pas de couleur.
    pub color: u32,
    pub position: i32,
    pub permissions: Permissions,
    /// Tout le monde peut le mentionner (`<@&id>`).
    pub mentionable: bool,
}

/// Emoji personnalisé d'un serveur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emoji {
    pub id: u64,
    pub name: String,
    pub animated: bool,
    /// `false` : inutilisable (abonnement du serveur perdu).
    pub available: bool,
    /// Réservé à ces rôles (vide : à tous).
    pub roles: Vec<u64>,
}

/// Mon membre dans un serveur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Member {
    pub roles: Vec<u64>,
    pub nick: Option<String>,
    /// Fin de l'exclusion temporaire (ms Unix).
    pub timeout_until: Option<i64>,
}

impl Member {
    pub fn timed_out(&self, now_ms: i64) -> bool {
        self.timeout_until.is_some_and(|t| t > now_ms)
    }
}

#[derive(Clone, Debug)]
pub struct Guild {
    pub id: u64,
    pub name: String,
    pub channels: Vec<Channel>,
    pub owner_id: Option<u64>,
    /// Rôles du serveur (@everyone porte l'identifiant du serveur).
    pub roles: Vec<Role>,
    /// Mon membre ; `None` si le paquet reçu ne le contenait pas.
    pub me: Option<Member>,
    /// Un rôle reçu était illisible : mes permissions y sont inconnues.
    pub roles_unreadable: bool,
    /// Icône du serveur (`None` : initiales).
    pub icon: Option<ImageRef>,
    /// Emojis personnalisés (GUILD_CREATE, GUILD_EMOJIS_UPDATE).
    pub emojis: Vec<Emoji>,
    /// Niveau de boost (0 à 3) et nombre de boosts (`premium_tier`,
    /// `premium_subscription_count`).
    pub premium_tier: u8,
    pub boosts: u32,
    /// Bannière du serveur, montrée en tête de la colonne des salons
    /// (`banners/{guild_id}/{hash}`, même chemin que celle d'un profil).
    pub banner: Option<ImageRef>,
    /// Tag du serveur (`profile.tag`, `profile.badge` ; lot ND) : mon compte peut l'afficher.
    pub tag: Option<super::cosmetics::GuildTag>,
}

impl Guild {
    /// Serveur sans rôles ni membre connus (permissions inconnues).
    pub fn new(id: u64, name: String, channels: Vec<Channel>) -> Self {
        Guild { id, name, channels, owner_id: None, roles: Vec::new(), me: None, roles_unreadable: false, icon: None, emojis: Vec::new(), premium_tier: 0, boosts: 0, banner: None, tag: None }
    }
}

#[derive(Clone, Debug)]
pub struct Dm {
    pub id: u64,
    pub name: String,
    pub last_message: u64,
    /// MP à deux : avatar du correspondant ; groupe : icône du groupe, et
    /// avatar par défaut tiré de l'identifiant du salon.
    pub avatar: Avatar,
    /// Destinataires connus (identifiant, nom affiché), pour l'autocomplétion.
    pub recipients: Vec<(u64, String)>,
}

impl Dm {
    /// Conversation sans image (avatar par défaut tiré de l'identifiant).
    pub fn new(id: u64, name: String, last_message: u64) -> Self {
        Dm { id, name, last_message, avatar: Avatar::default_for(id), recipients: Vec::new() }
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    pub id: u64,
    pub channel_id: u64,
    pub guild_id: Option<u64>,
    pub author_id: u64,
    pub author: String,
    pub author_avatar: Avatar,
    pub content: String,
    pub attachments: Vec<Attachment>,
    pub embeds: Vec<Embed>,
    pub mentions: Vec<(u64, String)>,
    pub mention_everyone: bool,
    pub edited: bool,
    /// Message système sans texte : son texte (dans la langue courante) est
    /// mis dans `content` par le magasin.
    pub system: Option<SystemKind>,
    /// Réactions, réponse, transfert, épingle (lots T5/T6).
    pub meta: super::social::MessageMeta,
}

/// Horodatage (ms Unix) encodé dans un snowflake.
pub fn snowflake_ms(id: u64) -> i64 {
    ((id >> 22) + DISCORD_EPOCH) as i64
}

/// Trie les salons dans l'ordre d'affichage : salons sans catégorie,
/// puis chaque catégorie suivie de ses salons.
pub fn sort_channels(chs: &mut [Channel]) {
    let cat_pos: HashMap<u64, i32> = chs
        .iter()
        .filter(|c| c.kind == ChannelKind::Category)
        .map(|c| (c.id, c.position))
        .collect();
    chs.sort_by_key(|c| {
        let (group_pos, group_id, child) = if c.kind == ChannelKind::Category {
            (c.position, c.id, 0)
        } else {
            match c.parent.and_then(|p| cat_pos.get(&p).map(|pos| (*pos, p))) {
                Some((pos, p)) => (pos, p, 1),
                None => (-1, 0, 1),
            }
        };
        (group_pos, group_id, child, c.position, c.id)
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(id: u64, kind: ChannelKind, position: i32, parent: Option<u64>) -> Channel {
        Channel { id, name: id.to_string(), kind, position, parent, overwrites: Some(Vec::new()), slowmode: 0 }
    }

    #[test]
    fn snowflake_epoch_and_known_value() {
        assert_eq!(snowflake_ms(0), DISCORD_EPOCH as i64);
        // Exemple de la documentation Discord : 175928847299117063 -> 2016-04-30 11:18:25.796 UTC
        assert_eq!(snowflake_ms(175_928_847_299_117_063), 1_462_015_105_796);
        // Les bits bas (worker, processus, incrément) n'influent pas.
        assert_eq!(snowflake_ms((5 << 22) | 0x3F_FFFF), DISCORD_EPOCH as i64 + 5);
    }

    #[test]
    fn sort_channels_groups_by_category() {
        use ChannelKind::*;
        let mut chs = vec![
            ch(10, Text, 0, Some(2)),       // dans la catégorie B
            ch(2, Category, 1, None),       // catégorie B
            ch(1, Category, 0, None),       // catégorie A
            ch(11, Announcement, 0, Some(1)),
            ch(12, Text, 1, Some(1)),
            ch(20, Text, 5, None),          // sans catégorie
            ch(21, Text, 3, None),          // sans catégorie
            ch(30, Text, 0, Some(999)),     // parent inconnu : traité comme sans catégorie
            ch(13, Text, 1, Some(1)),       // même position que 12 : départage par id
        ];
        sort_channels(&mut chs);
        let ids: Vec<u64> = chs.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![30, 21, 20, 1, 11, 12, 13, 2, 10]);
    }
}
