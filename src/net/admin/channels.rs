//! Salons et catégories (lot A2) : requêtes et décodage.
//!
//! Formes vérifiées le 13 novembre 2026, toutes documentées par Discord
//! (docs.discord.com, « Guild » : Create Guild Channel, Modify Guild Channel
//! Positions ; docs.discord.food, « Channels » : Get Channel, Modify Channel,
//! Delete Channel, objets Forum Tag et Default Reaction) :
//! - `GET /channels/{id}` : l'objet Channel ;
//! - `POST /guilds/{id}/channels` (`name`, `type`, `topic`, `parent_id`,
//!   `permission_overwrites`, `nsfw`, `rate_limit_per_user`) : le salon créé ;
//! - `PATCH /channels/{id}` (champs modifiés seulement) : le salon modifié ;
//! - `DELETE /channels/{id}` : le salon supprimé ;
//! - `PATCH /guilds/{id}/channels` (tableau de `id`, `position`, `parent_id`,
//!   `lock_permissions` ; un seul `parent_id` par requête) : 204 ;
//! - `GET /voice/regions` (docs.discord.com, « Voice » : List Voice Regions) :
//!   tableau d'objets Voice Region (`id`, `name`, `deprecated`, `custom`).
//!
//! Salon vocal ou scène (phase 10, lot S) : `bitrate`, `user_limit`,
//! `rtc_region` (nul : automatique) et `video_quality_mode` (1 automatique,
//! 2 720p) de l'objet Channel, lus et modifiés par les mêmes requêtes.
//!
//! Toutes suivent un geste et passent par le limiteur commun (`api`).

use super::super::raw::{de_lenient, de_loose_id, Item};
use super::super::ratelimit::Route;
use super::super::rest::{api, ApiError, Ctx};
use crate::state::admin::channels::{ChanAction, ChanCmd, ChanEvent, ChanFailure, Details, Move, NewChannel, Patch, Reaction, Region, Tag};
use crate::state::model::{Overwrite, OverwriteKind};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
struct RawTag {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    moderated: Option<bool>,
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
}

#[derive(Deserialize)]
struct RawReaction {
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
}

#[derive(Deserialize)]
struct RawDetails {
    #[serde(deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u8>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    topic: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    nsfw: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    rate_limit_per_user: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    default_auto_archive_duration: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    available_tags: Option<Vec<Item<RawTag>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    default_reaction_emoji: Option<RawReaction>,
    #[serde(default, deserialize_with = "de_lenient")]
    bitrate: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    user_limit: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    rtc_region: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    video_quality_mode: Option<u8>,
}

#[derive(Deserialize)]
struct RawRegion {
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    deprecated: Option<bool>,
}

/// Régions proposées : celles que Discord ne déconseille pas.
fn regions(body: &str) -> Option<Vec<Region>> {
    let list = serde_json::from_str::<Vec<Item<RawRegion>>>(body).ok()?;
    Some(
        list.into_iter()
            .filter_map(|r| r.0)
            .filter(|r| r.deprecated != Some(true))
            .filter_map(|r| {
                let id = r.id.filter(|i| !i.is_empty())?;
                Some(Region { name: r.name.filter(|n| !n.is_empty()).unwrap_or_else(|| id.clone()), id })
            })
            .collect(),
    )
}

fn reaction(r: RawReaction) -> Option<Reaction> {
    match (r.emoji_id, r.emoji_name.filter(|n| !n.is_empty())) {
        (Some(id), _) => Some(Reaction::Custom(id)),
        (None, Some(n)) => Some(Reaction::Unicode(n)),
        _ => None,
    }
}

/// Objet Channel d'un serveur (réponse ou passerelle) ; `None` s'il n'en est pas un.
fn details(body: &str) -> Option<Details> {
    let r = serde_json::from_str::<RawDetails>(body).ok()?;
    Some(Details {
        id: r.id?,
        guild_id: r.guild_id,
        kind: r.kind?,
        name: r.name.unwrap_or_default(),
        topic: r.topic.unwrap_or_default(),
        nsfw: r.nsfw.unwrap_or(false),
        slowmode: r.rate_limit_per_user.unwrap_or(0),
        auto_archive: r.default_auto_archive_duration,
        // Une étiquette sans identifiant ni nom n'est pas gardée (elle ne peut être réécrite telle quelle).
        tags: r
            .available_tags
            .unwrap_or_default()
            .into_iter()
            .filter_map(|t| t.0)
            .filter_map(|t| {
                Some(Tag { id: Some(t.id?), name: t.name?, moderated: t.moderated.unwrap_or(false), emoji_id: t.emoji_id, emoji_name: t.emoji_name.filter(|n| !n.is_empty()) })
            })
            .collect(),
        reaction: r.default_reaction_emoji.and_then(reaction),
        bitrate: r.bitrate,
        user_limit: r.user_limit,
        rtc_region: r.rtc_region.filter(|x| !x.is_empty()),
        video_quality: r.video_quality_mode,
    })
}

/// CHANNEL_CREATE / CHANNEL_UPDATE d'un salon de serveur : ses détails, pour
/// la fenêtre « Modifier le salon » si elle le montre (le magasin trie).
pub(in crate::net) fn gateway(d: &str) -> Option<Event> {
    details(d).filter(|x| x.guild_id.is_some()).map(|x| Event::ChannelAdmin(ChanEvent::Details(x)))
}

fn overwrite(o: &Overwrite) -> Value {
    let kind = match o.kind {
        OverwriteKind::Role => 0,
        OverwriteKind::Member => 1,
    };
    json!({ "id": o.id.to_string(), "type": kind, "allow": o.allow.0.to_string(), "deny": o.deny.0.to_string() })
}

fn tag(t: &Tag) -> Value {
    let mut v = json!({
        "name": t.name.trim(),
        "moderated": t.moderated,
        "emoji_id": t.emoji_id.map(|i| i.to_string()),
        "emoji_name": if t.emoji_id.is_some() { None } else { t.emoji_name.clone() },
    });
    if let Some(id) = t.id {
        v["id"] = id.to_string().into();
    }
    v
}

fn create_body(n: &NewChannel) -> Value {
    let mut b = Map::new();
    b.insert("name".into(), n.name.clone().into());
    b.insert("type".into(), n.kind.code().into());
    if let Some(p) = n.parent {
        b.insert("parent_id".into(), p.to_string().into());
    }
    // Sans dérogations, Discord reprend celles de la catégorie (salon synchronisé).
    if !n.overwrites.is_empty() {
        b.insert("permission_overwrites".into(), n.overwrites.iter().map(overwrite).collect());
    }
    if n.kind.has_topic() && !n.topic.trim().is_empty() {
        b.insert("topic".into(), n.topic.trim().into());
    }
    if n.nsfw {
        b.insert("nsfw".into(), true.into());
    }
    if n.slowmode > 0 && n.kind.has_topic() {
        b.insert("rate_limit_per_user".into(), n.slowmode.into());
    }
    Value::Object(b)
}

fn patch_body(p: &Patch) -> Value {
    let mut b = Map::new();
    if let Some(v) = &p.name {
        b.insert("name".into(), v.clone().into());
    }
    if let Some(v) = p.kind {
        b.insert("type".into(), v.into());
    }
    if let Some(v) = &p.topic {
        b.insert("topic".into(), v.clone().into());
    }
    if let Some(v) = p.nsfw {
        b.insert("nsfw".into(), v.into());
    }
    if let Some(v) = p.slowmode {
        b.insert("rate_limit_per_user".into(), v.into());
    }
    if let Some(v) = p.auto_archive {
        b.insert("default_auto_archive_duration".into(), v.into());
    }
    if let Some(v) = &p.overwrites {
        b.insert("permission_overwrites".into(), v.iter().map(overwrite).collect());
    }
    if let Some(v) = &p.tags {
        b.insert("available_tags".into(), v.iter().map(tag).collect());
    }
    if let Some(v) = &p.reaction {
        let r = match v {
            Some(Reaction::Custom(id)) => json!({ "emoji_id": id.to_string(), "emoji_name": null }),
            Some(Reaction::Unicode(n)) => json!({ "emoji_id": null, "emoji_name": n }),
            None => Value::Null,
        };
        b.insert("default_reaction_emoji".into(), r);
    }
    if let Some(v) = p.bitrate {
        b.insert("bitrate".into(), v.into());
    }
    if let Some(v) = p.user_limit {
        b.insert("user_limit".into(), v.into());
    }
    if let Some(v) = &p.rtc_region {
        b.insert("rtc_region".into(), v.clone().into());
    }
    if let Some(v) = p.video_quality {
        b.insert("video_quality_mode".into(), v.into());
    }
    Value::Object(b)
}

fn positions_body(moves: &[Move]) -> Value {
    moves
        .iter()
        .map(|m| {
            let mut v = json!({ "id": m.id.to_string(), "position": m.position });
            if let Some(p) = m.parent {
                v["parent_id"] = p.map(|p| p.to_string()).into();
                if m.lock {
                    v["lock_permissions"] = true.into();
                }
            }
            v
        })
        .collect()
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn request(cmd: &ChanCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        ChanCmd::Load { channel_id } => r("GET /channels/{}", *channel_id, Method::GET, format!("/channels/{channel_id}"), None),
        ChanCmd::Create { guild_id, new } => r("POST /guilds/{}/channels", *guild_id, Method::POST, format!("/guilds/{guild_id}/channels"), Some(create_body(new))),
        ChanCmd::Modify { channel_id, patch } => r("PATCH /channels/{}", *channel_id, Method::PATCH, format!("/channels/{channel_id}"), Some(patch_body(patch))),
        ChanCmd::Delete { channel_id } => r("DELETE /channels/{}", *channel_id, Method::DELETE, format!("/channels/{channel_id}"), None),
        ChanCmd::Positions { guild_id, moves } => r("PATCH /guilds/{}/channels", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/channels"), Some(positions_body(moves))),
        ChanCmd::Regions => r("GET /voice/regions", 0, Method::GET, "/voice/regions".to_string(), None),
    }
}

fn action(cmd: &ChanCmd) -> ChanAction {
    match cmd {
        ChanCmd::Load { .. } => ChanAction::Load,
        ChanCmd::Create { .. } => ChanAction::Create,
        ChanCmd::Modify { .. } => ChanAction::Save,
        ChanCmd::Delete { .. } => ChanAction::Delete,
        ChanCmd::Positions { .. } => ChanAction::Move,
        ChanCmd::Regions => ChanAction::Regions,
    }
}

/// Réponse reçue : les événements à publier (`Err` : réponse sans la forme attendue).
fn answer(cmd: &ChanCmd, body: &str) -> Result<Vec<ChanEvent>, ChanFailure> {
    let unreadable = || ChanFailure::Problem(Problem::Api(String::new()));
    match cmd {
        ChanCmd::Load { .. } => details(body).map(|d| vec![ChanEvent::Details(d)]).ok_or_else(unreadable),
        // Enregistré même si la réponse est illisible : CHANNEL_UPDATE suivra.
        ChanCmd::Modify { channel_id, .. } => Ok(std::iter::once(ChanEvent::Saved { channel_id: *channel_id }).chain(details(body).map(ChanEvent::Details)).collect()),
        ChanCmd::Create { guild_id, .. } => details(body).map(|d| vec![ChanEvent::Created { guild_id: *guild_id, channel_id: d.id }]).ok_or_else(unreadable),
        ChanCmd::Delete { channel_id } => Ok(vec![ChanEvent::Deleted { channel_id: *channel_id }]),
        ChanCmd::Positions { guild_id, .. } => Ok(vec![ChanEvent::Moved { guild_id: *guild_id }]),
        ChanCmd::Regions => regions(body).map(|l| vec![ChanEvent::Regions(l)]).ok_or_else(unreadable),
    }
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: ChanCmd) {
    let action = action(&cmd);
    let failed = |failure| Event::ChannelAdmin(ChanEvent::Failed { action, failure });
    let req = request(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    match api(&ctx, req.route, req.method, &url, req.body, ACTION_MAX_WAIT, std::future::pending()).await {
        Ok(body) => match answer(&cmd, &body) {
            Ok(events) => events.into_iter().for_each(|e| ctx.out.send(Event::ChannelAdmin(e))),
            Err(f) => ctx.out.send(failed(f)),
        },
        Err(ApiError::NotFound(_)) => ctx.out.send(failed(ChanFailure::NotFound)),
        Err(e) => ctx.out.send(e.into_event(|p| failed(ChanFailure::Problem(p)))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::channels::NewKind;
    use crate::state::Permissions;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn request_shapes() {
        let new = NewChannel {
            name: "secret".into(),
            kind: NewKind::Text,
            parent: Some(9),
            overwrites: vec![Overwrite { id: 5, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: Permissions::VIEW_CHANNEL }],
            topic: " sujet ".into(),
            nsfw: true,
            slowmode: 10,
        };
        let c = request(&ChanCmd::Create { guild_id: 5, new });
        assert_eq!((c.method, c.path.as_str(), c.route.major), (Method::POST, "/guilds/5/channels", 5));
        assert_eq!(
            c.body,
            Some(json!({ "name": "secret", "type": 0, "parent_id": "9", "permission_overwrites": [{ "id": "5", "type": 0, "allow": "0", "deny": "1024" }], "topic": "sujet", "nsfw": true, "rate_limit_per_user": 10 }))
        );
        // Catégorie, vocal : ni sujet ni mode lent ; sans dérogations, rien n'est envoyé (synchronisé).
        let cat = NewChannel { name: "Cat".into(), kind: NewKind::Category, parent: None, overwrites: vec![], topic: "x".into(), nsfw: false, slowmode: 5 };
        assert_eq!(request(&ChanCmd::Create { guild_id: 5, new: cat }).body, Some(json!({ "name": "Cat", "type": 4 })));
        let voice = NewChannel { name: "Vocal".into(), kind: NewKind::Voice, parent: Some(9), overwrites: vec![], topic: String::new(), nsfw: false, slowmode: 0 };
        assert_eq!(request(&ChanCmd::Create { guild_id: 5, new: voice }).body, Some(json!({ "name": "Vocal", "type": 2, "parent_id": "9" })));

        let patch = Patch {
            name: Some("n".into()),
            kind: Some(5),
            topic: Some(String::new()),
            slowmode: Some(0),
            auto_archive: Some(1440),
            tags: Some(vec![Tag { id: Some(3), name: "Aide".into(), moderated: true, emoji_id: None, emoji_name: Some("❓".into()) }, Tag { name: "Neuve".into(), ..Tag::default() }]),
            reaction: Some(None),
            ..Patch::default()
        };
        let m = request(&ChanCmd::Modify { channel_id: 7, patch });
        assert_eq!((m.method, m.path.as_str()), (Method::PATCH, "/channels/7"));
        assert_eq!(
            m.body,
            Some(json!({
                "name": "n", "type": 5, "topic": "", "rate_limit_per_user": 0, "default_auto_archive_duration": 1440,
                "available_tags": [
                    { "id": "3", "name": "Aide", "moderated": true, "emoji_id": null, "emoji_name": "❓" },
                    { "name": "Neuve", "moderated": false, "emoji_id": null, "emoji_name": null }
                ],
                "default_reaction_emoji": null
            }))
        );
        let p = request(&ChanCmd::Modify { channel_id: 7, patch: Patch { reaction: Some(Some(Reaction::Custom(42))), nsfw: Some(false), ..Patch::default() } });
        assert_eq!(p.body, Some(json!({ "nsfw": false, "default_reaction_emoji": { "emoji_id": "42", "emoji_name": null } })));

        let moves = vec![Move { id: 1, position: 0, parent: Some(Some(9)), lock: true }, Move { id: 2, position: 1, parent: None, lock: false }, Move { id: 3, position: 2, parent: Some(None), lock: false }];
        let pos = request(&ChanCmd::Positions { guild_id: 5, moves });
        assert_eq!((pos.method, pos.path.as_str()), (Method::PATCH, "/guilds/5/channels"));
        assert_eq!(
            pos.body,
            Some(json!([
                { "id": "1", "position": 0, "parent_id": "9", "lock_permissions": true },
                { "id": "2", "position": 1 },
                { "id": "3", "position": 2, "parent_id": null }
            ]))
        );
        let d = request(&ChanCmd::Delete { channel_id: 7 });
        assert_eq!((d.method, d.path.as_str(), d.body), (Method::DELETE, "/channels/7", None));
        assert_eq!(request(&ChanCmd::Load { channel_id: 7 }).method, Method::GET);
    }

    #[test]
    fn channel_details_are_read() {
        let ev = answer(&ChanCmd::Load { channel_id: 1 }, &fixture("rest_channel_forum.json"));
        let Ok(evs) = ev else { panic!("{ev:?}") };
        let [ChanEvent::Details(d)] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!((d.id, d.kind, d.name.as_str(), d.topic.as_str()), (3000000000000000501, 15, "entraide", "Posez vos questions ici."));
        assert_eq!((d.nsfw, d.slowmode, d.auto_archive), (false, 30, Some(4320)));
        assert_eq!(d.tags.len(), 2, "l'étiquette illisible est écartée");
        assert_eq!(d.tags[0], Tag { id: Some(3000000000000000601), name: "Résolu".into(), moderated: true, emoji_id: None, emoji_name: Some("✅".into()) });
        assert_eq!(d.tags[1].emoji_id, Some(4000000000000000001));
        assert_eq!(d.reaction, Some(Reaction::Unicode("👍".into())));
        // Champs inconnus ou absents : sans échec.
        let ev = answer(&ChanCmd::Load { channel_id: 1 }, r#"{ "id": "5", "type": 0, "nouveau": [1, 2], "topic": null }"#);
        assert!(matches!(ev.as_deref(), Ok([ChanEvent::Details(Details { id: 5, .. })])), "{ev:?}");
        assert!(answer(&ChanCmd::Load { channel_id: 1 }, "[]").is_err());
        // Création : l'identifiant du salon créé.
        let new = NewChannel { name: "x".into(), kind: NewKind::Forum, parent: None, overwrites: vec![], topic: String::new(), nsfw: false, slowmode: 0 };
        let ev = answer(&ChanCmd::Create { guild_id: 2, new }, &fixture("rest_channel_forum.json"));
        assert!(matches!(ev.as_deref(), Ok([ChanEvent::Created { guild_id: 2, channel_id: 3000000000000000501 }])), "{ev:?}");
        // Enregistré même sans corps lisible.
        let ev = answer(&ChanCmd::Modify { channel_id: 4, patch: Patch::default() }, "");
        assert!(matches!(ev.as_deref(), Ok([ChanEvent::Saved { channel_id: 4 }])), "{ev:?}");
        // Passerelle : seulement les salons de serveur.
        assert!(gateway(&fixture("rest_channel_forum.json")).is_some());
        assert!(gateway(r#"{ "id": "5", "type": 1 }"#).is_none());
    }

    #[test]
    fn voice_channel_settings_and_regions() {
        let ev = answer(&ChanCmd::Load { channel_id: 1 }, &fixture("rest_channel_voice.json"));
        let Ok(evs) = ev else { panic!("{ev:?}") };
        let [ChanEvent::Details(d)] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!((d.kind, d.bitrate, d.user_limit, d.rtc_region.as_deref(), d.video_quality), (2, Some(64_000), Some(8), Some("rotterdam"), Some(2)));
        // Région nulle : automatique ; champs absents : inconnus.
        let ev = answer(&ChanCmd::Load { channel_id: 1 }, r#"{ "id": "5", "type": 2, "rtc_region": null, "bitrate": "?" }"#);
        assert!(matches!(ev.as_deref(), Ok([ChanEvent::Details(Details { rtc_region: None, bitrate: None, user_limit: None, video_quality: None, .. })])), "{ev:?}");

        let patch = Patch { bitrate: Some(96_000), user_limit: Some(0), rtc_region: Some(None), video_quality: Some(1), ..Patch::default() };
        let m = request(&ChanCmd::Modify { channel_id: 7, patch });
        assert_eq!(m.body, Some(json!({ "bitrate": 96000, "user_limit": 0, "rtc_region": null, "video_quality_mode": 1 })));
        let m = request(&ChanCmd::Modify { channel_id: 7, patch: Patch { rtc_region: Some(Some("japan".into())), ..Patch::default() } });
        assert_eq!(m.body, Some(json!({ "rtc_region": "japan" })));

        let r = request(&ChanCmd::Regions);
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::GET, "/voice/regions", None));
        let ev = answer(&ChanCmd::Regions, &fixture("rest_voice_regions.json"));
        let Ok(evs) = ev else { panic!("{ev:?}") };
        let [ChanEvent::Regions(list)] = evs.as_slice() else { panic!("{evs:?}") };
        // Région déconseillée, sans identifiant ou illisible : écartée.
        assert_eq!(list.iter().map(|r| (r.id.as_str(), r.name.as_str())).collect::<Vec<_>>(), vec![("brazil", "Brazil"), ("rotterdam", "Rotterdam"), ("us-east", "US East")]);
        assert!(answer(&ChanCmd::Regions, "{}").is_err());
    }
}
