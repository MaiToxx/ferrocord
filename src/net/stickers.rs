//! Autocollants (lot T4) : `sticker_items` des messages, `stickers` des
//! serveurs (GUILD_CREATE, READY), GUILD_STICKERS_UPDATE. Formes vérifiées
//! dans la documentation publique :
//! https://discord.com/developers/docs/resources/sticker (Sticker Object,
//! Sticker Item Object, Sticker Format Types) et
//! https://discord.com/developers/docs/events/gateway-events#guild-stickers-update.
//! L'envoi passe par Create Message (`sticker_ids`, `net/social.rs`).

use super::raw::{Item, RawGuild};
use super::Out;
use crate::state::emoji::{GuildSticker, StickerEvent, StickerFormat, StickerItem};
use crate::state::Event;
use serde::Deserialize;

/// Autocollant ou élément d'autocollant : seuls les champs communs servent.
#[derive(Deserialize)]
pub(super) struct RawSticker {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    format_type: Option<u64>,
    #[serde(default)]
    available: Option<bool>,
}

/// Noms plus longs que celui permis par Discord (30) : coupés.
const MAX_NAME: usize = 64;

impl RawSticker {
    fn item(self) -> Option<(StickerItem, bool)> {
        let id = self.id?.parse().ok()?;
        let format = StickerFormat::from_type(self.format_type?)?;
        let name: String = self.name.unwrap_or_default().chars().take(MAX_NAME).collect();
        Some((StickerItem { id, name, format }, self.available.unwrap_or(true)))
    }
}

/// Autocollants lisibles d'un message (les illisibles sont ignorés).
pub(super) fn items(list: Option<Vec<Item<RawSticker>>>) -> Vec<StickerItem> {
    list.unwrap_or_default().into_iter().filter_map(|i| i.0?.item()).map(|(s, _)| s).collect()
}

fn guild_list(list: Vec<Item<RawSticker>>) -> Vec<GuildSticker> {
    let mut list: Vec<GuildSticker> = list.into_iter().filter_map(|i| i.0?.item()).map(|(s, available)| GuildSticker { id: s.id, name: s.name, format: s.format, available }).collect();
    // Gardée toute la session : sans la réserve du tableau brut (lot « ram »).
    list.shrink_to_fit();
    list
}

/// Retire les autocollants d'un serveur reçu, à publier après lui ([`send`]).
pub(super) fn take(g: &mut RawGuild, into: &mut Vec<(u64, Vec<GuildSticker>)>) {
    if let Some(list) = g.stickers.take()
        && !g.unavailable
    {
        into.push((g.id, guild_list(list)));
    }
}

pub(super) fn send(list: Vec<(u64, Vec<GuildSticker>)>, out: &Out) {
    for (guild_id, stickers) in list {
        out.send(Event::Stickers(StickerEvent::Guild { guild_id, stickers }));
    }
}

#[derive(Deserialize)]
struct RawStickersUpdate {
    guild_id: String,
    #[serde(default)]
    stickers: Vec<Item<RawSticker>>,
}

/// GUILD_STICKERS_UPDATE : la liste complète remplace l'ancienne.
pub(super) fn update(d: &str, out: &Out) {
    if let Some((guild_id, list)) = parse_update(d) {
        out.send(Event::Stickers(StickerEvent::Guild { guild_id, stickers: list }));
    }
}

fn parse_update(d: &str) -> Option<(u64, Vec<GuildSticker>)> {
    let u: RawStickersUpdate = serde_json::from_str(d).ok()?;
    Some((u.guild_id.parse().ok()?, guild_list(u.stickers)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"));
        v["d"].to_string()
    }

    #[test]
    fn message_sticker_items() {
        let m = crate::net::decode_history(&format!("[{}]", d("message_create_sticker"))).unwrap_or_default();
        let s = &m.first().unwrap_or_else(|| panic!("message")).meta.stickers;
        let got: Vec<(u64, &str, StickerFormat)> = s.iter().map(|s| (s.id, s.name.as_str(), s.format)).collect();
        // Le format inconnu (9) et l'élément sans identifiant sont ignorés.
        assert_eq!(got, [(7000000000000000701, "Wave", StickerFormat::Apng), (7000000000000000704, "Lottie", StickerFormat::Lottie), (7000000000000000705, "gif", StickerFormat::Gif)]);
    }

    #[test]
    fn guild_stickers_update() {
        let (g, list) = parse_update(&d("guild_stickers_update")).unwrap_or_else(|| panic!("mise à jour"));
        assert_eq!(g, 2000000000000000001);
        assert_eq!(list.len(), 2);
        assert_eq!((list[0].id, list[0].format, list[0].available), (7000000000000000701, StickerFormat::Png, true));
        assert!(!list[1].available);
        assert_eq!(parse_update(r#"{ "guild_id": "x" }"#), None);
        // Par la passerelle : un événement pour le magasin.
        let ev = crate::net::decode_dispatch("GUILD_STICKERS_UPDATE", &d("guild_stickers_update"));
        assert!(matches!(&ev[..], [Event::Stickers(StickerEvent::Guild { guild_id: 2000000000000000001, stickers })] if stickers.len() == 2), "{ev:?}");
    }

    #[test]
    fn guild_create_carries_stickers() {
        let mut g: RawGuild = serde_json::from_str(&d("guild_create_stickers")).unwrap_or_else(|e| panic!("serveur : {e}"));
        let mut into = Vec::new();
        take(&mut g, &mut into);
        assert_eq!(into.len(), 1);
        assert_eq!(into[0].1.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Salut"]);
        assert!(g.stickers.is_none());
        let ev = crate::net::decode_dispatch("GUILD_CREATE", &d("guild_create_stickers"));
        assert!(ev.iter().any(|e| matches!(e, Event::GuildCreate(_))), "{ev:?}");
        assert!(ev.iter().any(|e| matches!(e, Event::Stickers(StickerEvent::Guild { stickers, .. }) if stickers.len() == 1)), "{ev:?}");
    }
}