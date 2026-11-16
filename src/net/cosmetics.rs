//! Parures des profils (lot P) : décoration d'avatar, plaque nominative,
//! tag de serveur, style du nom, couleurs de rôle.
//!
//! Formes vérifiées le 14 novembre 2026 :
//! - documentation officielle, « User Object » : `avatar_decoration_data`
//!   (`asset`, `sku_id`), `collectibles.nameplate` (`sku_id`, `asset`,
//!   `label`, `palette`), `primary_guild` (`identity_guild_id`,
//!   `identity_enabled`, `tag`, `badge`) ;
//! - documentation officielle, « Guild Member Object » : `banner`,
//!   `avatar_decoration_data` ;
//! - docs.discord.food, « Users » : `display_name_styles` (`font_id`,
//!   `effect_id`, `colors`), non documenté par Discord.
//!
//! Ce sont des champs d'objets déjà reçus (messages, membres, présences,
//! profil) : aucune requête n'est ajoutée pour eux. Chaque champ est lu à
//! part ; une forme inattendue vaut « pas de parure ».

use super::raw::{de_lenient, de_loose_id};
use crate::state::cosmetics::{Asset, GuildTag, Look, NameEffect, NameStyle, Nameplate, Palette};
use crate::state::model::ImageHash;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};

/// Champ présent (même `null` ou illisible) : `Some`, avec sa valeur si
/// elle se lit. Absent : `None` par `#[serde(default)]`.
pub(super) fn de_present<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<Option<T>>, D::Error> {
    super::raw::with_raw(d, |raw| Some(serde_json::from_str::<T>(raw).ok()))
}

#[derive(Deserialize)]
pub(super) struct RawDecoration {
    #[serde(default, deserialize_with = "de_lenient")]
    asset: Option<String>,
}

impl RawDecoration {
    pub(super) fn asset(&self) -> Option<Asset> {
        Asset::decoration(self.asset.as_deref()?)
    }
}

#[derive(Deserialize)]
struct RawNameplate {
    #[serde(default, deserialize_with = "de_lenient")]
    asset: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    palette: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawCollectibles {
    #[serde(default, deserialize_with = "de_lenient")]
    nameplate: Option<RawNameplate>,
}

impl RawCollectibles {
    fn nameplate(&self) -> Option<Nameplate> {
        let n = self.nameplate.as_ref()?;
        Some(Nameplate { asset: Asset::path(n.asset.as_deref()?)?, palette: n.palette.as_deref().and_then(Palette::parse) })
    }
}

#[derive(Deserialize)]
pub(super) struct RawPrimaryGuild {
    #[serde(default, deserialize_with = "de_loose_id")]
    identity_guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    identity_enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    tag: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    badge: Option<String>,
}

impl RawPrimaryGuild {
    /// Tag affiché : seulement si l'utilisateur le porte (`identity_enabled`
    /// vrai ; `false` : retiré par lui, `null` : retiré par Discord).
    fn tag(&self) -> Option<GuildTag> {
        if self.identity_enabled != Some(true) {
            return None;
        }
        GuildTag::new(self.identity_guild_id?, self.tag.as_deref()?, self.badge.as_deref().and_then(ImageHash::parse))
    }
}

#[derive(Deserialize)]
pub(super) struct RawNameStyle {
    #[serde(default, deserialize_with = "de_lenient")]
    font_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    effect_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    colors: Option<Vec<u64>>,
}

impl RawNameStyle {
    fn style(&self) -> Option<NameStyle> {
        let colors: Vec<u32> = self.colors.as_deref()?.iter().filter_map(|c| u32::try_from(*c).ok()).collect();
        NameStyle::new(self.font_id.and_then(|f| u16::try_from(f).ok()).unwrap_or(0), NameEffect::from_code(self.effect_id.unwrap_or(0)), &colors)
    }
}

/// Les quatre champs de parure d'un objet `user`, tels que reçus.
pub(super) fn look(
    decoration: &Option<Option<RawDecoration>>,
    collectibles: &Option<Option<RawCollectibles>>,
    primary_guild: &Option<Option<RawPrimaryGuild>>,
    name_style: &Option<Option<RawNameStyle>>,
) -> Look {
    Look {
        decoration: decoration.as_ref().map(|d| d.as_ref().and_then(RawDecoration::asset)),
        nameplate: collectibles.as_ref().map(|c| c.as_ref().and_then(RawCollectibles::nameplate)),
        tag: primary_guild.as_ref().map(|p| p.as_ref().and_then(RawPrimaryGuild::tag)),
        name_style: name_style.as_ref().map(|s| s.as_ref().and_then(RawNameStyle::style)),
    }
}

#[cfg(test)]
mod tests {
    use super::super::raw::{RawGuildMember, RawUser};
    use super::*;

    fn fixture(name: &str) -> serde_json::Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    /// Jeu d'essai synthétique `rest_user_cosmetics.json` : objet `user` paré.
    #[test]
    fn user_fixture_carries_every_cosmetic() {
        let u: RawUser = serde_json::from_value(fixture("rest_user_cosmetics")["user"].clone()).unwrap_or_else(|e| panic!("{e}"));
        let look = u.look();
        assert_eq!(look.decoration().map(Asset::name).as_deref(), Some("a_dec0000000000000000000000000d1"));
        let plate = look.nameplate().unwrap_or_else(|| panic!("plaque"));
        assert_eq!((plate.asset.name().as_str(), plate.palette), ("nameplates/nameplates/fictive/", Some(Palette::Cobalt)));
        let tag = look.tag().unwrap_or_else(|| panic!("tag"));
        assert_eq!((tag.text(), tag.guild_id), ("RUST", 2_000_000_000_000_000_001));
        assert!(tag.badge.is_some());
        let style = look.name_style().unwrap_or_else(|| panic!("style"));
        assert_eq!((style.font_id, style.effect, style.colors()), (12, NameEffect::Gradient, &[0xff7a59, 0x8a5cf0][..]));
    }

    /// Absent : inconnu ; `null`, forme inattendue, tag retiré : pas de parure.
    #[test]
    fn absent_null_and_odd_shapes() {
        let look = |json: &str| serde_json::from_str::<RawUser>(json).unwrap_or_else(|e| panic!("{e}")).look();
        assert_eq!(look(r#"{ "id": "1", "username": "u" }"#), Look::default());
        let none = Look { decoration: Some(None), nameplate: Some(None), tag: Some(None), name_style: Some(None) };
        assert_eq!(look(r#"{ "id": "1", "username": "u", "avatar_decoration_data": null, "collectibles": null, "primary_guild": null, "display_name_styles": null }"#), none);
        let odd = r#"{ "id": "1", "username": "u",
            "avatar_decoration_data": { "asset": "../../etc", "sku_id": 7 },
            "collectibles": { "nameplate": { "asset": "https://exemple.invalid/x/", "palette": 3 } },
            "primary_guild": { "identity_guild_id": "9", "identity_enabled": false, "tag": "ABCD", "badge": null },
            "display_name_styles": { "font_id": "x", "effect_id": 2, "colors": [] } }"#;
        assert_eq!(look(odd), none);
        let plain = r#"{ "id": "1", "username": "u", "collectibles": {}, "primary_guild": { "identity_guild_id": "9", "identity_enabled": true, "tag": "AB", "badge": "pas-une-empreinte" } }"#;
        let l = look(plain);
        assert_eq!(l.nameplate, Some(None));
        assert_eq!(l.tag().map(|t| (t.text().to_string(), t.badge)), Some(("AB".into(), None)));
    }

    /// Membre : bannière et décoration propres au serveur.
    #[test]
    fn member_fixture_carries_guild_banner_and_decoration() {
        let m: RawGuildMember = serde_json::from_value(fixture("rest_user_cosmetics")["member"].clone()).unwrap_or_else(|e| panic!("{e}"));
        assert!(m.banner.as_deref().and_then(ImageHash::parse).is_some_and(|h| h.animated));
        assert_eq!(m.avatar_decoration_data.as_ref().and_then(RawDecoration::asset).map(Asset::name).as_deref(), Some("dec1000000000000000000000000d2"));
    }
}
