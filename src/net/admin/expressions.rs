//! Emojis, autocollants et sons d'un serveur (lot A6). Routes documentées
//! par Discord (ressources Emoji, Sticker et Soundboard), forme vérifiée
//! sur docs.discord.food le 13 novembre 2026 :
//! - `GET /guilds/{id}/emojis` (avec `user` pour qui gère les expressions),
//!   `POST` (`name`, `image` en `data:`, `roles`), `PATCH /guilds/{id}/emojis/{emoji}`
//!   (`name`, `roles`), `DELETE` (204) ;
//! - `GET /guilds/{id}/stickers` (tableau), `POST` en `multipart/form-data`
//!   (`name`, `description`, `tags`, `file`), `PATCH` (`name`, `description`,
//!   `tags`), `DELETE` (204) ;
//! - `GET /guilds/{id}/soundboard-sounds` (`{ "items": [...] }`), `POST`
//!   (`name`, `sound` en `data:`, `volume` de 0 à 1, `emoji_id`, `emoji_name`),
//!   `PATCH`, `DELETE` (204) ;
//! - passerelle : GUILD_SOUNDBOARD_SOUND_CREATE / UPDATE (objet son),
//!   GUILD_SOUNDBOARD_SOUND_DELETE (`sound_id`, `guild_id`),
//!   GUILD_SOUNDBOARD_SOUNDS_UPDATE (`guild_id`, `soundboard_sounds`), et les
//!   fonctions du serveur de GUILD_UPDATE (`features` ; le niveau de boost est dans `Guild`).
//!
//! Tout part sur un geste, par le limiteur commun.

use super::super::raw::{de_lenient, de_loose_id, into_emojis, Item, RawEmoji};
use super::super::ratelimit::Route;
use super::super::rest::{api_req, ApiError, Ctx};
use super::super::Out;
use crate::state::admin::expressions::{Author, Boost, Busy, ExprCmd, ExprEvent, Failure, Kind, Sound, SoundEmoji, StickerFile, StickerInfo};
use crate::state::Event;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
struct RawAuthor {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    global_name: Option<String>,
}

fn author(u: Option<RawAuthor>) -> Option<Author> {
    let u = u?;
    let name = u.global_name.filter(|n| !n.is_empty()).or(u.username)?;
    Some(Author { id: u.id?, name })
}

/// Ce qu'une réponse apporte en plus d'un emoji, d'un autocollant ou d'un son.
#[derive(Deserialize)]
struct RawExtra {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawAuthor>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    tags: Option<String>,
}

impl RawExtra {
    fn sticker(self) -> Option<StickerInfo> {
        Some(StickerInfo { id: self.id?, description: self.description.unwrap_or_default(), tags: self.tags.unwrap_or_default(), author: author(self.user) })
    }
}

#[derive(Deserialize)]
struct RawSound {
    #[serde(default, deserialize_with = "de_loose_id")]
    sound_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    volume: Option<f64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    available: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawAuthor>,
}

impl RawSound {
    fn into_sound(self) -> Option<Sound> {
        let emoji = match (self.emoji_id, self.emoji_name.filter(|n| !n.is_empty())) {
            (Some(id), _) => Some(SoundEmoji::Custom(id)),
            (None, Some(n)) => Some(SoundEmoji::Unicode(n)),
            (None, None) => None,
        };
        let volume = self.volume.filter(|v| v.is_finite()).map_or(100, |v| (v.clamp(0.0, 1.0) * 100.0).round() as u8);
        Some(Sound { id: self.sound_id?, name: self.name.unwrap_or_default(), volume, emoji, available: self.available.unwrap_or(true), author: author(self.user) })
    }
}

#[derive(Deserialize)]
struct RawSoundList {
    #[serde(default)]
    items: Vec<Item<RawSound>>,
}

fn sounds(list: Vec<Item<RawSound>>) -> Vec<Sound> {
    list.into_iter().filter_map(|s| s.0?.into_sound()).collect()
}

/// Fonctions qui agrandissent les emplacements (le niveau : `Guild::premium_tier`).
pub(super) fn boost_of<'a>(features: impl Iterator<Item = &'a str>) -> Boost {
    Boost::from_features(features)
}

enum Body {
    None,
    Json(Value),
    /// Corps `multipart/form-data` et sa frontière.
    Form(String, Vec<u8>),
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Body,
}

fn ids(list: &[u64]) -> Vec<String> {
    list.iter().map(u64::to_string).collect()
}

fn sound_body(name: &str, volume: u8, emoji: &Option<SoundEmoji>) -> Value {
    let (id, unicode) = match emoji {
        Some(SoundEmoji::Custom(id)) => (Value::String(id.to_string()), Value::Null),
        Some(SoundEmoji::Unicode(s)) => (Value::Null, Value::String(s.clone())),
        None => (Value::Null, Value::Null),
    };
    json!({ "name": name, "volume": f64::from(volume.min(100)) / 100.0, "emoji_id": id, "emoji_name": unicode })
}

/// Nom de fichier sûr dans un en-tête `Content-Disposition`.
fn safe_file_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') { c } else { '_' }).take(64).collect();
    if s.is_empty() { "fichier".into() } else { s }
}

/// Corps multipart de Create Guild Sticker.
fn sticker_form(name: &str, description: &str, tags: &str, file: &StickerFile) -> (String, Vec<u8>) {
    // Frontière absente du fichier (vérifié ; changée sinon).
    let mut n = file.bytes.len() as u64 ^ 0x5EED_F00D;
    let boundary = loop {
        let b = format!("----ferrocord{n:016x}");
        if !file.bytes.windows(b.len()).any(|w| w == b.as_bytes()) {
            break b;
        }
        n = n.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
    };
    let mut out = Vec::with_capacity(file.bytes.len() + 512);
    for (k, v) in [("name", name), ("description", description), ("tags", tags)] {
        out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    let fname = safe_file_name(&file.name);
    out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{fname}\"\r\nContent-Type: {}\r\n\r\n", file.mime).as_bytes());
    out.extend_from_slice(&file.bytes);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    (boundary, out)
}

fn request(cmd: &ExprCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Body| Request { route: Route { template, major }, method, path, body };
    let g = cmd.guild_id();
    match cmd {
        ExprCmd::List { kind: Kind::Emoji, .. } => r("GET /guilds/{}/emojis", g, Method::GET, format!("/guilds/{g}/emojis"), Body::None),
        ExprCmd::List { kind: Kind::Sticker, .. } => r("GET /guilds/{}/stickers", g, Method::GET, format!("/guilds/{g}/stickers"), Body::None),
        ExprCmd::List { kind: Kind::Sound, .. } => r("GET /guilds/{}/soundboard-sounds", g, Method::GET, format!("/guilds/{g}/soundboard-sounds"), Body::None),
        ExprCmd::CreateEmoji { name, image, roles, .. } => {
            r("POST /guilds/{}/emojis", g, Method::POST, format!("/guilds/{g}/emojis"), Body::Json(json!({ "name": name, "image": image, "roles": ids(roles) })))
        }
        ExprCmd::EditEmoji { id, name, roles, .. } => {
            r("PATCH /guilds/{}/emojis/{}", g, Method::PATCH, format!("/guilds/{g}/emojis/{id}"), Body::Json(json!({ "name": name, "roles": ids(roles) })))
        }
        ExprCmd::CreateSticker { name, description, tags, file, .. } => {
            let (boundary, bytes) = sticker_form(name, description, tags, file);
            r("POST /guilds/{}/stickers", g, Method::POST, format!("/guilds/{g}/stickers"), Body::Form(boundary, bytes))
        }
        ExprCmd::EditSticker { id, name, description, tags, .. } => r(
            "PATCH /guilds/{}/stickers/{}",
            g,
            Method::PATCH,
            format!("/guilds/{g}/stickers/{id}"),
            Body::Json(json!({ "name": name, "description": description, "tags": tags })),
        ),
        ExprCmd::CreateSound { name, sound, volume, emoji, .. } => {
            let mut body = sound_body(name, *volume, emoji);
            body["sound"] = Value::String(sound.clone());
            r("POST /guilds/{}/soundboard-sounds", g, Method::POST, format!("/guilds/{g}/soundboard-sounds"), Body::Json(body))
        }
        ExprCmd::EditSound { id, name, volume, emoji, .. } => {
            r("PATCH /guilds/{}/soundboard-sounds/{}", g, Method::PATCH, format!("/guilds/{g}/soundboard-sounds/{id}"), Body::Json(sound_body(name, *volume, emoji)))
        }
        ExprCmd::Delete { kind, id, .. } => {
            let (template, what) = match kind {
                Kind::Emoji => ("DELETE /guilds/{}/emojis/{}", "emojis"),
                Kind::Sticker => ("DELETE /guilds/{}/stickers/{}", "stickers"),
                Kind::Sound => ("DELETE /guilds/{}/soundboard-sounds/{}", "soundboard-sounds"),
            };
            r(template, g, Method::DELETE, format!("/guilds/{g}/{what}/{id}"), Body::None)
        }
    }
}

/// Réponse reçue : l'événement à publier (`None` : réponse sans la forme attendue).
pub(in crate::net) fn answer(cmd: &ExprCmd, body: &str) -> Option<ExprEvent> {
    let guild_id = cmd.guild_id();
    let busy = cmd.busy();
    Some(match cmd {
        ExprCmd::List { kind: Kind::Emoji, .. } => {
            let emojis = into_emojis(serde_json::from_str::<Vec<Item<RawEmoji>>>(body).ok()?);
            let extra: Vec<Item<RawExtra>> = serde_json::from_str(body).ok()?;
            let authors = extra.into_iter().filter_map(|e| e.0).filter_map(|e| Some((e.id?, author(e.user)?))).collect();
            ExprEvent::Emojis { guild_id, emojis, authors }
        }
        ExprCmd::List { kind: Kind::Sticker, .. } => {
            let list: Vec<Item<RawExtra>> = serde_json::from_str(body).ok()?;
            ExprEvent::Stickers { guild_id, list: list.into_iter().filter_map(|e| e.0?.sticker()).collect() }
        }
        ExprCmd::List { kind: Kind::Sound, .. } => ExprEvent::Sounds { guild_id, list: sounds(serde_json::from_str::<RawSoundList>(body).ok()?.items) },
        ExprCmd::Delete { id, .. } => ExprEvent::Done { busy, id: Some(*id), author: None, sticker: None },
        ExprCmd::CreateSound { .. } | ExprCmd::EditSound { .. } => {
            // Réponse illisible après un succès : l'action a bien eu lieu.
            let s = serde_json::from_str::<RawSound>(body).ok().and_then(RawSound::into_sound);
            ExprEvent::Done { busy, id: s.as_ref().map(|s| s.id), author: s.and_then(|s| s.author), sticker: None }
        }
        ExprCmd::CreateEmoji { .. } | ExprCmd::EditEmoji { .. } | ExprCmd::CreateSticker { .. } | ExprCmd::EditSticker { .. } => {
            let e = serde_json::from_str::<RawExtra>(body).ok();
            let id = e.as_ref().and_then(|e| e.id);
            let is_sticker = busy.kind == Kind::Sticker;
            let (author, sticker) = match e {
                Some(e) if is_sticker => (None, e.sticker()),
                Some(e) => (author(e.user), None),
                None => (None, None),
            };
            ExprEvent::Done { busy, id, author, sticker }
        }
    })
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: ExprCmd) {
    let busy: Busy = cmd.busy();
    let failed = |failure| Event::Expressions(ExprEvent::Failed { busy, failure });
    let req = request(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    let build = |rb: reqwest::RequestBuilder| match &req.body {
        Body::None => rb,
        Body::Json(v) => rb.json(v),
        Body::Form(boundary, bytes) => rb.header(reqwest::header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}")).body(bytes.clone()),
    };
    let ev = match api_req(&ctx, req.route, req.method.clone(), &url, build, ACTION_MAX_WAIT, std::future::pending()).await {
        Ok(body) => match answer(&cmd, &body) {
            Some(e) => Event::Expressions(e),
            None => failed(Failure::Problem(crate::state::Problem::Api(String::new()))),
        },
        Err(ApiError::Captcha) => failed(Failure::Captcha),
        Err(ApiError::NotFound(_)) => failed(Failure::NotFound),
        Err(e) => e.into_event(|p| failed(Failure::Problem(p))),
    };
    ctx.out.send(ev);
}

#[derive(Deserialize)]
struct RawSoundDelete {
    #[serde(default, deserialize_with = "de_loose_id")]
    sound_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
}

#[derive(Deserialize)]
struct RawSoundsUpdate {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default)]
    soundboard_sounds: Vec<Item<RawSound>>,
}

#[derive(Deserialize)]
struct RawPremium {
    #[serde(default, deserialize_with = "de_lenient")]
    features: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct RawGuildPremium {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    features: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    properties: Option<RawPremium>,
}

/// Événements de la passerelle de ce domaine.
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    if let Some(e) = decode(kind, d) {
        out.send(Event::Expressions(e));
    }
}

fn decode(kind: &str, d: &str) -> Option<ExprEvent> {
    match kind {
        "GUILD_SOUNDBOARD_SOUND_CREATE" | "GUILD_SOUNDBOARD_SOUND_UPDATE" => {
            let raw: RawSound = serde_json::from_str(d).ok()?;
            let guild_id = raw.guild_id?;
            Some(ExprEvent::SoundUpsert { guild_id, sound: raw.into_sound()? })
        }
        "GUILD_SOUNDBOARD_SOUND_DELETE" => {
            let raw: RawSoundDelete = serde_json::from_str(d).ok()?;
            Some(ExprEvent::SoundDelete { guild_id: raw.guild_id?, id: raw.sound_id? })
        }
        "GUILD_SOUNDBOARD_SOUNDS_UPDATE" => {
            let raw: RawSoundsUpdate = serde_json::from_str(d).ok()?;
            Some(ExprEvent::Sounds { guild_id: raw.guild_id?, list: sounds(raw.soundboard_sounds) })
        }
        "GUILD_UPDATE" => {
            let raw: RawGuildPremium = serde_json::from_str(d).ok()?;
            let features = raw.features.or(raw.properties.and_then(|p| p.features))?;
            Some(ExprEvent::Boost { guild_id: raw.id?, boost: boost_of(features.iter().map(String::as_str)) })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    fn json_of(r: &Request) -> Option<Value> {
        match &r.body {
            Body::Json(v) => Some(v.clone()),
            _ => None,
        }
    }

    #[test]
    fn request_shapes() {
        let c = request(&ExprCmd::CreateEmoji { guild_id: 5, name: "chat".into(), image: "data:image/png;base64,AA==".into(), roles: vec![7] });
        assert_eq!((c.method.clone(), c.path.as_str(), c.route.major), (Method::POST, "/guilds/5/emojis", 5));
        assert_eq!(json_of(&c), Some(json!({ "name": "chat", "image": "data:image/png;base64,AA==", "roles": ["7"] })));
        let e = request(&ExprCmd::EditEmoji { guild_id: 5, id: 9, name: "chien".into(), roles: vec![] });
        assert_eq!((e.method.clone(), e.path.as_str()), (Method::PATCH, "/guilds/5/emojis/9"));
        assert_eq!(json_of(&e), Some(json!({ "name": "chien", "roles": [] })));
        let s = request(&ExprCmd::CreateSound { guild_id: 5, name: "ding".into(), sound: "data:audio/ogg;base64,AA==".into(), volume: 50, emoji: Some(SoundEmoji::Unicode("🔔".into())) });
        assert_eq!(s.path, "/guilds/5/soundboard-sounds");
        assert_eq!(json_of(&s), Some(json!({ "name": "ding", "sound": "data:audio/ogg;base64,AA==", "volume": 0.5, "emoji_id": null, "emoji_name": "🔔" })));
        let s = request(&ExprCmd::EditSound { guild_id: 5, id: 3, name: "dong".into(), volume: 100, emoji: Some(SoundEmoji::Custom(8)) });
        assert_eq!((s.method.clone(), s.path.as_str()), (Method::PATCH, "/guilds/5/soundboard-sounds/3"));
        assert_eq!(json_of(&s), Some(json!({ "name": "dong", "volume": 1.0, "emoji_id": "8", "emoji_name": null })));
        let t = request(&ExprCmd::EditSticker { guild_id: 5, id: 4, name: "Salut".into(), description: "".into(), tags: "wave".into() });
        assert_eq!(json_of(&t), Some(json!({ "name": "Salut", "description": "", "tags": "wave" })));
        for (kind, path) in [(Kind::Emoji, "/guilds/5/emojis/2"), (Kind::Sticker, "/guilds/5/stickers/2"), (Kind::Sound, "/guilds/5/soundboard-sounds/2")] {
            let d = request(&ExprCmd::Delete { guild_id: 5, kind, id: 2 });
            assert_eq!((d.method, d.path.as_str()), (Method::DELETE, path));
        }
        assert_eq!(request(&ExprCmd::List { guild_id: 5, kind: Kind::Sound }).path, "/guilds/5/soundboard-sounds");
    }

    #[test]
    fn sticker_multipart_is_well_formed() {
        let file = StickerFile { name: "mon \"chat\".png".into(), mime: "image/png", bytes: Arc::from(&b"\x89PNG-octets"[..]) };
        let r = request(&ExprCmd::CreateSticker { guild_id: 5, name: "Chat".into(), description: "Un chat".into(), tags: "cat".into(), file });
        let Body::Form(boundary, bytes) = r.body else { panic!("multipart attendu") };
        let text = String::from_utf8_lossy(&bytes);
        assert!(text.starts_with(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"name\"\r\n\r\nChat\r\n")), "{text}");
        assert!(text.contains("name=\"tags\"\r\n\r\ncat\r\n"));
        assert!(text.contains("filename=\"mon__chat_.png\"\r\nContent-Type: image/png\r\n\r\n\u{FFFD}PNG-octets\r\n"), "{text}");
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn lists_are_read_leniently() {
        let cmd = ExprCmd::List { guild_id: 2000000000000000001, kind: Kind::Emoji };
        let Some(ExprEvent::Emojis { emojis, authors, .. }) = answer(&cmd, &fixture("rest_guild_emojis.json")) else { panic!("emojis") };
        assert_eq!(emojis.iter().map(|e| (e.name.as_str(), e.animated)).collect::<Vec<_>>(), [("ferris", false), ("danse", true)]);
        assert_eq!(emojis[1].roles, vec![2000000000000000201]);
        assert_eq!(authors, vec![(7000000000000000801, Author { id: 1000000000000000003, name: "Alice".into() })]);
        let cmd = ExprCmd::List { guild_id: 2000000000000000001, kind: Kind::Sticker };
        let Some(ExprEvent::Stickers { list, .. }) = answer(&cmd, &fixture("rest_guild_stickers.json")) else { panic!("autocollants") };
        assert_eq!(list.len(), 1);
        assert_eq!((list[0].description.as_str(), list[0].tags.as_str(), list[0].author.as_ref().map(|a| a.name.as_str())), ("Un salut", "wave", Some("bob")));
        let cmd = ExprCmd::List { guild_id: 2000000000000000001, kind: Kind::Sound };
        let Some(ExprEvent::Sounds { list, .. }) = answer(&cmd, &fixture("rest_guild_soundboard_sounds.json")) else { panic!("sons") };
        assert_eq!(list.len(), 2, "l'entrée sans identifiant est écartée");
        assert_eq!((list[0].name.as_str(), list[0].volume, list[0].emoji.clone()), ("ding", 60, Some(SoundEmoji::Unicode("🔔".into()))));
        assert_eq!((list[1].emoji.clone(), list[1].author.is_none(), list[1].available), (Some(SoundEmoji::Custom(7000000000000000801)), true, false));
        assert!(answer(&cmd, "\"x\"").is_none());
    }

    #[test]
    fn writes_report_the_item() {
        let cmd = ExprCmd::CreateEmoji { guild_id: 1, name: "x".into(), image: String::new(), roles: vec![] };
        let ev = answer(&cmd, r#"{ "id": "42", "name": "x", "user": { "id": "3", "username": "alice" } }"#);
        assert!(matches!(ev, Some(ExprEvent::Done { id: Some(42), author: Some(Author { id: 3, .. }), .. })), "{ev:?}");
        // 204 sans corps : réussi quand même.
        let ev = answer(&ExprCmd::Delete { guild_id: 1, kind: Kind::Sound, id: 9 }, "");
        assert!(matches!(ev, Some(ExprEvent::Done { id: Some(9), .. })), "{ev:?}");
        let cmd = ExprCmd::EditSticker { guild_id: 1, id: 4, name: "s".into(), description: String::new(), tags: "t".into() };
        let ev = answer(&cmd, r#"{ "id": "4", "description": "neuf", "tags": "fire" }"#);
        assert!(matches!(&ev, Some(ExprEvent::Done { sticker: Some(s), .. }) if s.description == "neuf" && s.tags == "fire"), "{ev:?}");
    }

    #[test]
    fn gateway_events() {
        let d = r#"{ "guild_id": "5", "sound_id": "9", "name": "ding", "volume": 0.25, "emoji_name": "🔔", "available": true, "user_id": "3" }"#;
        let ev = decode("GUILD_SOUNDBOARD_SOUND_CREATE", d);
        assert!(matches!(&ev, Some(ExprEvent::SoundUpsert { guild_id: 5, sound }) if sound.id == 9 && sound.volume == 25), "{ev:?}");
        assert!(matches!(decode("GUILD_SOUNDBOARD_SOUND_DELETE", r#"{ "guild_id": "5", "sound_id": "9" }"#), Some(ExprEvent::SoundDelete { guild_id: 5, id: 9 })));
        let ev = decode("GUILD_SOUNDBOARD_SOUNDS_UPDATE", &format!(r#"{{ "guild_id": "5", "soundboard_sounds": [{d}, 3] }}"#));
        assert!(matches!(&ev, Some(ExprEvent::Sounds { guild_id: 5, list }) if list.len() == 1), "{ev:?}");
        let ev = decode("GUILD_UPDATE", r#"{ "id": "5", "name": "x", "premium_tier": 2, "features": ["MORE_STICKERS", "COMMUNITY"] }"#);
        assert!(matches!(ev, Some(ExprEvent::Boost { guild_id: 5, boost: Boost { more_stickers: true, more_emoji: false, .. } })), "{ev:?}");
        assert!(decode("GUILD_UPDATE", r#"{ "id": "5", "name": "x" }"#).is_none(), "fonctions absentes : rien");
        assert!(decode("GUILD_SOUNDBOARD_SOUND_DELETE", "{}").is_none());
        // Par la passerelle complète : GUILD_CREATE apporte aussi le niveau.
        let ev = crate::net::decode_dispatch("GUILD_CREATE", r#"{ "id": "5", "name": "x", "premium_tier": 3, "features": ["MORE_EMOJI"], "channels": [], "roles": [] }"#);
        assert!(ev.iter().any(|e| matches!(e, Event::Expressions(ExprEvent::Boost { guild_id: 5, boost: Boost { more_emoji: true, .. } }))), "{ev:?}");
    }
}
