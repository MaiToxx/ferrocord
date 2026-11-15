//! Autocollants (stickers) : ceux d'un message (`sticker_items`) et ceux des
//! serveurs (GUILD_CREATE, GUILD_STICKERS_UPDATE), proposés par le sélecteur.
//! Forme : https://discord.com/developers/docs/resources/sticker.

use std::collections::HashMap;

/// `format_type` d'un autocollant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StickerFormat {
    Png,
    /// PNG animé : la première image est affichée.
    Apng,
    /// Animation vectorielle : pas de décodeur, le nom est affiché.
    Lottie,
    Gif,
}

impl StickerFormat {
    pub fn from_type(n: u64) -> Option<Self> {
        Some(match n {
            1 => StickerFormat::Png,
            2 => StickerFormat::Apng,
            3 => StickerFormat::Lottie,
            4 => StickerFormat::Gif,
            _ => return None,
        })
    }

    /// Une image peut être affichée (tout sauf Lottie).
    pub fn has_image(self) -> bool {
        self != StickerFormat::Lottie
    }
}

/// Autocollant d'un message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct StickerItem {
    pub id: u64,
    pub name: String,
    pub format: StickerFormat,
}

/// Autocollant d'un serveur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildSticker {
    pub id: u64,
    pub name: String,
    pub format: StickerFormat,
    /// `false` : perdu avec les boosts, inutilisable.
    pub available: bool,
}

impl GuildSticker {
    pub fn item(&self) -> StickerItem {
        StickerItem { id: self.id, name: self.name.clone(), format: self.format }
    }
}

/// Réseau -> magasin.
#[derive(Debug, PartialEq, Eq)]
pub enum StickerEvent {
    /// Liste complète des autocollants d'un serveur (remplace l'ancienne).
    Guild { guild_id: u64, stickers: Vec<GuildSticker> },
}

/// Autocollants par serveur. Borné par le nombre de serveurs (et par
/// Discord : au plus 60 par serveur) ; vidé à chaque READY.
#[derive(Default)]
pub struct Stickers {
    by_guild: HashMap<u64, Vec<GuildSticker>>,
}

impl Stickers {
    pub fn clear(&mut self) {
        self.by_guild.clear();
    }

    pub fn guild(&self, guild_id: u64) -> &[GuildSticker] {
        self.by_guild.get(&guild_id).map_or(&[], Vec::as_slice)
    }

    pub fn forget_guild(&mut self, guild_id: u64) {
        self.by_guild.remove(&guild_id);
    }

    pub fn apply(&mut self, ev: StickerEvent) {
        match ev {
            StickerEvent::Guild { guild_id, stickers } => {
                self.by_guild.insert(guild_id, stickers);
            }
        }
    }
}
