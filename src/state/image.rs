//! Références aux images du CDN gardées dans le modèle : empreintes
//! (`avatar`, `icon`) déjà transmises par Discord, et avatar par défaut.
//! Aucune adresse ici : `net::cdn` les construit.

use std::fmt;

/// Empreinte d'une image du CDN : 32 chiffres hexadécimaux en minuscules,
/// précédés de `a_` si une version animée existe. Toute autre forme est
/// refusée à la lecture : une adresse n'est jamais construite à partir d'un
/// texte reçu tel quel.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ImageHash {
    pub animated: bool,
    bytes: [u8; 16],
}

impl ImageHash {
    pub fn parse(s: &str) -> Option<Self> {
        let (animated, hex) = match s.strip_prefix("a_") {
            Some(rest) => (true, rest),
            None => (false, s),
        };
        if hex.len() != 32 || !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
            return None;
        }
        let mut bytes = [0u8; 16];
        for (i, slot) in bytes.iter_mut().enumerate() {
            *slot = u8::from_str_radix(hex.get(2 * i..2 * i + 2)?, 16).ok()?;
        }
        Some(ImageHash { animated, bytes })
    }

    /// Empreinte fabriquée (aperçu hors ligne, tests).
    pub fn synthetic(seed: u64) -> Self {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&seed.to_be_bytes());
        bytes[8..].copy_from_slice(&(!seed).to_be_bytes());
        ImageHash { animated: false, bytes }
    }
}

impl fmt::Display for ImageHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.animated {
            f.write_str("a_")?;
        }
        for b in self.bytes {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

/// Image du CDN de Discord, désignée par ce qu'elle représente. Sert aussi
/// de clé aux caches d'images. Les lots suivants (pièces jointes, emojis)
/// y ajouteront leurs variantes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageRef {
    /// Avatar d'un utilisateur (`avatars/{user_id}/{hash}`).
    Avatar { user_id: u64, hash: ImageHash },
    /// Icône d'un serveur (`icons/{guild_id}/{hash}`).
    GuildIcon { guild_id: u64, hash: ImageHash },
    /// Icône d'un MP de groupe (`channel-icons/{channel_id}/{hash}`).
    GroupIcon { channel_id: u64, hash: ImageHash },
    /// Image d'une pièce jointe ou d'un embed (`state::media`) : son adresse
    /// est reçue avec le message et passée à la demande. `owner` : pièce
    /// jointe ou message ; `slot` : place dans le message ; `rev` : empreinte
    /// de l'adresse.
    Media { owner: u64, slot: u16, rev: u32 },
    /// Bannière de profil (`banners/{user_id}/{hash}`).
    Banner { user_id: u64, hash: ImageHash },
    /// Avatar propre à un serveur (`guilds/{guild_id}/users/{user_id}/avatars/{hash}`).
    MemberAvatar { guild_id: u64, user_id: u64, hash: ImageHash },
    /// Bannière propre à un serveur (`guilds/{guild_id}/users/{user_id}/banners/{hash}`).
    MemberBanner { guild_id: u64, user_id: u64, hash: ImageHash },
    /// Icône d'un badge de profil (`badge-icons/{hash}`, PNG seulement).
    BadgeIcon { hash: ImageHash },
    /// Emoji Unicode en image (Twemoji, `state::emoji`).
    Twemoji(super::emoji::Unicode),
    /// Emoji personnalisé d'un serveur (`emojis/{id}`) ; un animé est servi en image fixe.
    CustomEmoji { id: u64 },
    /// Autocollant (`stickers/{id}`) ; `gif` : servi en GIF.
    Sticker { id: u64, gif: bool },
    /// Autocollant Lottie : une animation vectorielle (`stickers/{id}.json`),
    /// dessinée par `images::lottie`.
    LottieSticker { id: u64 },
    /// Icône d'un rôle (`role-icons/{role_id}/{hash}`, lot A3).
    RoleIcon { role_id: u64, hash: ImageHash },
    /// Couverture d'un événement programmé (`guild-events/{event_id}/{hash}`, lot A7).
    EventCover { event_id: u64, hash: ImageHash },
    /// Décoration d'avatar (`avatar-decoration-presets/{asset}.png`, APNG si
    /// elle est animée ; lot P).
    Decoration { asset: super::cosmetics::Asset },
    /// Plaque nominative : image fixe (`assets/collectibles/{asset}static.png`)
    /// et, sous le pointeur, son animation (`…/{asset}asset.webm`, lot plaques).
    Nameplate { asset: super::cosmetics::Asset },
    /// Badge d'un tag de serveur (`guild-tag-badges/{guild_id}/{hash}.png`).
    TagBadge { guild_id: u64, hash: ImageHash },
    /// Icône ou couverture d'une application (`app-icons/{application_id}/{hash}`,
    /// documentation « Image Formatting » : Application Icon, Application Cover).
    AppIcon { app_id: u64, hash: ImageHash },
}

/// Nombre d'avatars par défaut de Discord.
pub const DEFAULT_AVATARS: u8 = 6;

/// Numéro de l'avatar par défaut, d'après la documentation « Image
/// Formatting » : `(id >> 22) % 6` pour le nouveau système de noms,
/// `discriminator % 5` pour un ancien discriminant (autre que « 0 »).
pub fn default_avatar_index(user_id: u64, discriminator: Option<&str>) -> u8 {
    match discriminator.and_then(|d| d.parse::<u16>().ok()).filter(|d| *d != 0) {
        Some(d) => (d % 5) as u8,
        None => ((user_id >> 22) % u64::from(DEFAULT_AVATARS)) as u8,
    }
}

/// Avatar à afficher : image du CDN s'il y en a une, et dans tous les cas
/// le numéro de l'avatar par défaut (0 à 5), dessiné localement en attendant
/// l'image ou à sa place (aucune requête pour lui).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Avatar {
    pub image: Option<ImageRef>,
    pub default: u8,
}

impl Avatar {
    /// Avatar d'un utilisateur ; `hash` vient du champ `avatar`.
    pub fn of_user(user_id: u64, hash: Option<ImageHash>, discriminator: Option<&str>) -> Self {
        Avatar { image: hash.map(|hash| ImageRef::Avatar { user_id, hash }), default: default_avatar_index(user_id, discriminator) }
    }

    /// Avatar par défaut seul, tiré d'un identifiant.
    pub fn default_for(id: u64) -> Self {
        Self::of_user(id, None, None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_hash_parse_and_display() {
        let h = "0123456789abcdef0123456789abcdef";
        let p = ImageHash::parse(h).unwrap_or_else(|| panic!("empreinte valide"));
        assert!(!p.animated);
        assert_eq!(p.to_string(), h);
        let a = ImageHash::parse(&format!("a_{h}")).unwrap_or_else(|| panic!("empreinte animée valide"));
        assert!(a.animated);
        assert_eq!(a.to_string(), format!("a_{h}"));
        assert_ne!(a, p);
        for bad in [
            "",
            "a_",
            "0123456789abcdef0123456789abcde",
            "0123456789abcdef0123456789abcdef0",
            "0123456789ABCDEF0123456789ABCDEF",
            "../../../0123456789abcdef01234567",
            "0123456789abcdef0123456789abcdeg",
            "b_0123456789abcdef0123456789abcdef",
            "a_a_0123456789abcdef0123456789abcd",
            "0123456789abcdef0123456789abcd%2f",
            "\u{ff10}123456789abcdef0123456789abcde",
        ] {
            assert_eq!(ImageHash::parse(bad), None, "{bad:?}");
        }
        let s = ImageHash::synthetic(42);
        assert_eq!(ImageHash::parse(&s.to_string()), Some(s));
    }

    #[test]
    fn default_avatar_index_cases() {
        // Nouveau système : (id >> 22) % 6.
        assert_eq!(default_avatar_index(0, None), 0);
        assert_eq!(default_avatar_index(5 << 22, None), 5);
        assert_eq!(default_avatar_index(6 << 22, Some("0")), 0);
        assert_eq!(default_avatar_index((7 << 22) | 0x3F_FFFF, None), 1);
        // Exemple de la documentation (identifiant 175928847299117063).
        assert_eq!(default_avatar_index(175_928_847_299_117_063, None), ((175_928_847_299_117_063u64 >> 22) % 6) as u8);
        // Ancien discriminant : discriminant % 5.
        assert_eq!(default_avatar_index(5 << 22, Some("1337")), 2);
        assert_eq!(default_avatar_index(5 << 22, Some("0004")), 4);
        // Illisible ou « 0000 » : nouveau système.
        assert_eq!(default_avatar_index(5 << 22, Some("abc")), 5);
        assert_eq!(default_avatar_index(5 << 22, Some("0000")), 5);
        for id in [0, 1, u64::MAX, 1 << 63] {
            assert!(default_avatar_index(id, None) < DEFAULT_AVATARS);
        }
        let a = Avatar::of_user(7, ImageHash::parse("0123456789abcdef0123456789abcdef"), None);
        assert!(matches!(a.image, Some(ImageRef::Avatar { user_id: 7, .. })));
        assert_eq!(Avatar::default_for(7).image, None);
    }
}
