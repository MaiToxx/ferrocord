//! Sélecteur de GIF, favoris du compte et super réaction (lot G).
//!
//! Points d'API réservés aux comptes utilisateur, non documentés par
//! Discord. Formes vérifiées sur docs.discord.food :
//!
//! Page « Integration » :
//! - « List Trending GIF Categories » : `GET /gifs/trending`, paramètres
//!   `media_format` et `locale` ; réponse `{ categories: [{ name, src }],
//!   gifs: [GIF] }` ;
//! - « List Trending GIFs » : `GET /gifs/trending-gifs`, paramètres `limit`
//!   (20 à 500), `media_format`, `locale` ; réponse : liste de GIF ;
//! - « Search GIFs » : `GET /gifs/search`, paramètres `q`, `limit` (20 à
//!   500, 100 par défaut), `media_format`, `locale` ; réponse : liste de GIF ;
//! - « Track Selected GIF » : `POST /gifs/select`, corps `{ id, q }`, 204 ;
//! - objet GIF : `id`, `title`, `url`, `src`, `gif_src`, `preview`, `width`,
//!   `height` ; formats : `mp4`, `tinymp4`, `webm`, `gif`, `mediumgif`,
//!   `tinygif`, `nanogif`, `webp`…
//!
//! Page « User Settings Proto » : `GET` et `PATCH
//! /users/@me/settings-proto/2` (type `FRECENCY`), corps `settings` (base64)
//! et `required_data_version` ; événement « User Settings Proto Update ».
//!
//! Page « Message », « Create Reaction » : `PUT
//! /channels/{id}/messages/{id}/reactions/{emoji}/@me?type=1` (type 1 :
//! `BURST`) ; « Delete Own Reaction » : `DELETE
//! …/reactions/{emoji}/{reaction_type}/@me`.
//!
//! Chaque requête suit un geste (la recherche à la frappe attend un silence
//! du clavier, `state::gifs`) et passe par `api` (limiteur commun) ; aucun
//! en-tête propre au client officiel. Un compte bot n'appelle rien.

use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::gifs::{Category, Gif, GifCmd, GifEvent, LIMIT, MEDIA_FORMAT};
use crate::state::usersettings::{b64, ProtoReply};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une demande avant de partir.
const MAX_WAIT: Duration = Duration::from_secs(10);
/// Type « Frecency » des réglages protobuf (docs.discord.food).
const FRECENCY: u8 = 2;
const FRECENCY_PATH: &str = "/users/@me/settings-proto/2";
/// Longueur maximale d'une adresse ou d'un nom reçus.
const MAX_TEXT: usize = 512;
/// GIF et catégories lus au plus dans une réponse.
const MAX_ITEMS: usize = 500;

#[derive(Deserialize)]
struct RawGif {
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    src: Option<String>,
    #[serde(default)]
    gif_src: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[derive(Deserialize)]
struct RawCategory {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    src: Option<String>,
}

#[derive(Deserialize)]
struct RawTrending {
    #[serde(default)]
    categories: Vec<serde_json::Value>,
    #[serde(default)]
    gifs: Vec<serde_json::Value>,
}

/// Adresse `https` simple, envoyable dans un message et affichable.
fn web_url(s: Option<String>) -> Option<String> {
    s.filter(|s| s.len() <= MAX_TEXT && s.starts_with("https://") && !s.chars().any(|c| c.is_whitespace() || c.is_control()))
}

fn gif_of(v: serde_json::Value) -> Option<Gif> {
    let g: RawGif = serde_json::from_value(v).ok()?;
    let url = web_url(g.url)?;
    let src = web_url(g.src)?;
    let id = g.id.unwrap_or_default().chars().take(64).collect();
    Some(Gif { id, url, src, gif_src: web_url(g.gif_src).unwrap_or_default(), width: g.width.unwrap_or(0), height: g.height.unwrap_or(0) })
}

/// Liste de GIF (recherche, tendances) ; les entrées illisibles sont
/// écartées. `None` : la réponse n'est pas une liste.
pub fn decode_gif_list(body: &str) -> Option<Vec<Gif>> {
    let list: Vec<serde_json::Value> = serde_json::from_str(body).ok()?;
    Some(list.into_iter().take(MAX_ITEMS).filter_map(gif_of).collect())
}

/// Réponse de `GET /gifs/trending` : catégories et GIF du moment.
pub fn decode_gif_trending(body: &str) -> Option<(Vec<Category>, Vec<Gif>)> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    let t: RawTrending = serde_json::from_str(body).ok()?;
    let categories = t
        .categories
        .into_iter()
        .take(MAX_ITEMS)
        .filter_map(|v| {
            let c: RawCategory = serde_json::from_value(v).ok()?;
            let name: String = c.name.filter(|n| !n.trim().is_empty())?.chars().take(crate::state::gifs::MAX_QUERY).collect();
            Some(Category { name, src: web_url(c.src)? })
        })
        .collect();
    Some((categories, t.gifs.into_iter().take(MAX_ITEMS).filter_map(gif_of).collect()))
}

/// Adresse d'une demande `/gifs/*`, paramètres encodés.
fn gifs_url(root: &str, path: &str, params: &[(&str, &str)]) -> Option<String> {
    let mut u = reqwest::Url::parse(&format!("{root}{path}")).ok()?;
    u.query_pairs_mut().extend_pairs(params);
    Some(u.into())
}

fn decode_proto_reply(body: &str) -> Option<ProtoReply> {
    #[derive(Deserialize)]
    struct Raw {
        settings: Option<String>,
        #[serde(default)]
        out_of_date: bool,
    }
    let r: Raw = serde_json::from_str(body).ok()?;
    Some(ProtoReply { bytes: r.settings.as_deref().and_then(b64::decode), out_of_date: r.out_of_date })
}

/// USER_SETTINGS_PROTO_UPDATE de type 2 (favoris, fréquence) ; les autres
/// types appartiennent à `account`.
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    #[derive(Deserialize)]
    struct Payload {
        #[serde(rename = "type")]
        kind: u8,
        proto: String,
    }
    #[derive(Deserialize)]
    struct Update {
        settings: Payload,
        #[serde(default)]
        partial: bool,
    }
    if kind == "USER_SETTINGS_PROTO_UPDATE"
        && let Ok(u) = serde_json::from_str::<Update>(d)
        && u.settings.kind == FRECENCY
    {
        out.send(Event::Gifs(GifEvent::Frecency { bytes: b64::decode(&u.settings.proto), partial: u.partial }));
    }
}

const fn route(template: &'static str, major: u64) -> Route {
    Route { template, major }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: GifCmd) {
    let gifs = |e: GifEvent| Event::Gifs(e);
    let call = |r: Route, m: Method, url: String, body: Option<serde_json::Value>| {
        let ctx = ctx.clone();
        async move { api(&ctx, r, m, &url, body, MAX_WAIT, std::future::pending()).await }
    };
    let root = ctx.api_root.as_str();
    let limit = LIMIT.to_string();
    if ctx.is_bot() {
        let ev = match cmd {
            GifCmd::Categories { .. } => gifs(GifEvent::Categories(Err(Problem::Forbidden))),
            GifCmd::Trending { round, .. } | GifCmd::Search { round, .. } => gifs(GifEvent::Results { round, gifs: Err(Problem::Forbidden) }),
            GifCmd::Select { .. } => return,
            GifCmd::LoadFrecency => gifs(GifEvent::FrecencyLoadFailed(Problem::Forbidden)),
            GifCmd::PatchFrecency { id, .. } => gifs(GifEvent::FrecencyWritten { id, result: Err(Problem::Forbidden) }),
            GifCmd::SuperReact { message_id, emoji, .. } => gifs(GifEvent::SuperReactDone { message_id, emoji, error: Some(Problem::Forbidden) }),
        };
        return ctx.out.send(ev);
    }
    let ev = match cmd {
        GifCmd::Categories { locale } => {
            let Some(url) = gifs_url(root, "/gifs/trending", &[("media_format", MEDIA_FORMAT), ("locale", locale)]) else { return };
            match call(route("GET /gifs/trending", 0), Method::GET, url, None).await.map(|b| decode_gif_trending(&b)) {
                Ok(Some((categories, now))) => gifs(GifEvent::Categories(Ok((categories, now.into_iter().next().map(|g| g.src))))),
                Ok(None) => gifs(GifEvent::Categories(Err(Problem::Api(String::new())))),
                Err(e) => e.into_event(|p| gifs(GifEvent::Categories(Err(p)))),
            }
        }
        GifCmd::Trending { round, locale } => {
            let Some(url) = gifs_url(root, "/gifs/trending-gifs", &[("media_format", MEDIA_FORMAT), ("limit", &limit), ("locale", locale)]) else { return };
            results(round, call(route("GET /gifs/trending-gifs", 0), Method::GET, url, None).await)
        }
        GifCmd::Search { round, query, locale } => {
            let Some(url) = gifs_url(root, "/gifs/search", &[("q", &query), ("media_format", MEDIA_FORMAT), ("limit", &limit), ("locale", locale)]) else { return };
            results(round, call(route("GET /gifs/search", 0), Method::GET, url, None).await)
        }
        GifCmd::Select { id, query } => {
            // Sans réponse utile : un échec ne change rien pour l'utilisateur
            // (sauf un jeton mort, annoncé comme partout).
            let url = format!("{root}/gifs/select");
            match call(route("POST /gifs/select", 0), Method::POST, url, Some(json!({ "id": id, "q": query }))).await {
                Err(ApiError::Unauthorized) => Event::Fatal { problem: Problem::BadToken, bad_token: true },
                _ => return,
            }
        }
        GifCmd::LoadFrecency => match call(route("GET /users/@me/settings-proto/{}", 0), Method::GET, format!("{root}{FRECENCY_PATH}"), None).await {
            // Réponse illisible : réglages inconnus, jamais réécrits.
            Ok(b) => gifs(GifEvent::Frecency { bytes: decode_proto_reply(&b).and_then(|r| r.bytes), partial: false }),
            Err(e) => e.into_event(|p| gifs(GifEvent::FrecencyLoadFailed(p))),
        },
        GifCmd::PatchFrecency { id, body, required_version } => {
            let mut payload = json!({ "settings": b64::encode(&body) });
            if let Some(v) = required_version {
                payload["required_data_version"] = json!(v);
            }
            match call(route("PATCH /users/@me/settings-proto/{}", 0), Method::PATCH, format!("{root}{FRECENCY_PATH}"), Some(payload)).await {
                // Réponse illisible : l'écriture a eu lieu, les valeurs envoyées sont gardées.
                Ok(b) => gifs(GifEvent::FrecencyWritten { id, result: Ok(decode_proto_reply(&b).unwrap_or(ProtoReply { bytes: None, out_of_date: false })) }),
                Err(e) => e.into_event(|p| gifs(GifEvent::FrecencyWritten { id, result: Err(p) })),
            }
        }
        GifCmd::SuperReact { channel_id, message_id, emoji, add } => {
            let base = format!("{root}/channels/{channel_id}/messages/{message_id}/reactions/{}", super::social::emoji_path(&emoji));
            let (method, url, template) = if add {
                (Method::PUT, format!("{base}/@me?type=1"), "PUT /channels/{}/messages/{}/reactions/{}/@me")
            } else {
                (Method::DELETE, format!("{base}/1/@me"), "DELETE /channels/{}/messages/{}/reactions/{}/{}/@me")
            };
            match call(route(template, channel_id), method, url, None).await {
                Ok(_) => gifs(GifEvent::SuperReactDone { message_id, emoji, error: None }),
                Err(e) => e.into_event(|p| gifs(GifEvent::SuperReactDone { message_id, emoji, error: Some(p) })),
            }
        }
    };
    ctx.out.send(ev);
}

fn results(round: u64, res: Result<String, ApiError>) -> Event {
    match res.map(|b| decode_gif_list(&b)) {
        Ok(Some(list)) => Event::Gifs(GifEvent::Results { round, gifs: Ok(list) }),
        Ok(None) => Event::Gifs(GifEvent::Results { round, gifs: Err(Problem::Api(String::new())) }),
        Err(e) => e.into_event(|p| Event::Gifs(GifEvent::Results { round, gifs: Err(p) })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_fixture_decodes_and_bad_entries_are_dropped() {
        let list = decode_gif_list(include_str!("../../tests/fixtures/rest_gifs_search.json")).unwrap_or_default();
        // Écartés : l'entrée sans adresse, celle dont le média n'est pas en https, et le nombre.
        assert_eq!(list.len(), 3, "{list:?}");
        let g = &list[0];
        assert_eq!((g.id.as_str(), g.width, g.height), ("7000000000000000001", 220, 124));
        assert_eq!(g.url, "https://tenor.com/view/chat-synthetique-gif-7000000000000000001");
        assert!(g.src.ends_with(".gif") && g.gif_src.starts_with("https://media.tenor.com/"));
        // Champs absents : valeurs par défaut.
        assert_eq!((list[2].width, list[2].gif_src.as_str()), (0, ""));
        assert!(decode_gif_list("{\"pas\": \"une liste\"}").is_none());
    }

    #[test]
    fn trending_fixture_decodes() {
        let (cats, gifs) = decode_gif_trending(include_str!("../../tests/fixtures/rest_gifs_trending.json")).unwrap_or_default();
        assert_eq!(cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["bravo", "merci", "rire"], "catégorie sans image écartée");
        assert_eq!(gifs.len(), 1);
        assert!(decode_gif_trending("[]").is_none());
        assert_eq!(decode_gif_trending("{}"), Some((Vec::new(), Vec::new())));
    }

    #[test]
    fn query_is_encoded() {
        let u = gifs_url("https://discord.com/api/v10", "/gifs/search", &[("q", "chat & chien/é"), ("media_format", MEDIA_FORMAT), ("limit", "50"), ("locale", "fr")]);
        assert_eq!(u.as_deref(), Some("https://discord.com/api/v10/gifs/search?q=chat+%26+chien%2F%C3%A9&media_format=tinygif&limit=50&locale=fr"));
    }

    #[test]
    fn frecency_update_is_routed_by_type() {
        let (tx, rx) = std::sync::mpsc::channel();
        let out = Out { tx, wake: Arc::new(|| {}), people: Default::default(), watch: Default::default(), interest: Default::default() };
        dispatch("USER_SETTINGS_PROTO_UPDATE", r#"{"settings":{"type":2,"proto":"CAE="},"partial":true}"#, &out);
        dispatch("USER_SETTINGS_PROTO_UPDATE", r#"{"settings":{"type":1,"proto":"CAE="},"partial":false}"#, &out);
        dispatch("MESSAGE_CREATE", "{}", &out);
        drop(out);
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(matches!(&events[..], [Event::Gifs(GifEvent::Frecency { bytes: Some(_), partial: true })]), "{events:?}");
        let r = decode_proto_reply(r#"{"settings":"CAE=","out_of_date":true}"#).unwrap_or(ProtoReply { bytes: None, out_of_date: false });
        assert!(r.out_of_date && r.bytes.is_some());
    }
}
