//! Lot G face au faux serveur : sélecteur de GIF (`/gifs/*`, non
//! documentés), favoris du compte (`settings-proto/2`), super réaction.

use crate::support::{ids::*, *};
use ferrocord::state::frecency::{synthetic, FavGif, State};
use ferrocord::state::gifs::{GifEvent, View, SEARCH_DELAY};
use ferrocord::state::social::{Emoji, SocialEvent};
use ferrocord::state::usersettings::{b64, proto::Msg, Refusal, Remote};
use ferrocord::state::Event;
use serde_json::json;
use std::time::{Duration, Instant};

const TRENDING: &str = "/api/v10/gifs/trending";
const TRENDING_GIFS: &str = "/api/v10/gifs/trending-gifs";
const SEARCH: &str = "/api/v10/gifs/search";
const SELECT: &str = "/api/v10/gifs/select";
const FRECENCY: &str = "/api/v10/users/@me/settings-proto/2";
const BOB_MSG: u64 = 5000000000000000201;

fn no_official_headers(r: &Request) {
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-super-properties"), "aucun en-tête du client officiel");
}

fn settings(bytes: &[u8]) -> serde_json::Value {
    json!({ "settings": b64::encode(bytes) })
}

#[tokio::test]
async fn gif_picker_categories_typed_search_trending_and_selection() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", FRECENCY, [Response::ok(settings(&synthetic(&[], &[], &[], 4)))]);
    s.http.on("GET", TRENDING, [Response::ok(fixture("rest_gifs_trending"))]);
    s.client.store.gifs_open("fr");
    s.client.store.gifs_open("fr");
    s.client.flush();
    let mut seen = [s.http.next_request().await, s.http.next_request().await];
    seen.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(seen[0].path, "/api/v10/gifs/trending?media_format=tinygif&locale=fr");
    assert_eq!(seen[1].path, FRECENCY);
    seen.iter().for_each(no_official_headers);
    s.client.until("catégories", |e| matches!(e, Event::Gifs(GifEvent::Categories(_)))).await;
    let Remote::Ready(cats) = &s.client.store.gifs.categories else { panic!("catégories absentes") };
    assert_eq!(cats.iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["bravo", "merci", "rire"]);
    assert!(s.client.store.gifs.trending_src.as_deref().is_some_and(|u| u.starts_with("https://media.tenor.com/")));

    // Frappe : rien ne part avant le silence du clavier, puis une seule recherche.
    let t0 = Instant::now();
    s.client.store.gifs_typed("ch", t0);
    s.client.store.gifs_typed("chat & chien", t0 + Duration::from_millis(150));
    assert!(s.client.store.gifs_tick(t0 + Duration::from_millis(200), "fr").is_some());
    s.client.flush();
    assert!(!s.http.requests().iter().any(|r| r.path.starts_with(SEARCH)), "aucune requête pendant la frappe");
    s.http.on("GET", SEARCH, [Response::ok(fixture("rest_gifs_search"))]);
    assert!(s.client.store.gifs_tick(t0 + Duration::from_millis(150) + SEARCH_DELAY, "fr").is_none());
    assert!(s.client.store.gifs_tick(t0 + Duration::from_secs(9), "fr").is_none());
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/gifs/search?q=chat+%26+chien&media_format=tinygif&limit=50&locale=fr"));
    no_official_headers(&r);
    s.client.until("résultats", |e| matches!(e, Event::Gifs(GifEvent::Results { .. }))).await;
    assert_eq!(s.client.store.gifs.view, View::Search("chat & chien".into()));
    assert_eq!(s.client.store.gifs.results.len(), 3, "entrées illisibles écartées");
    assert_eq!(s.http.requests().iter().filter(|r| r.path.starts_with(SEARCH)).count(), 1);

    // Envoi d'un résultat : son adresse en message, puis le choix signalé avec la recherche.
    let gif = s.client.store.gifs.results[0].clone();
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(fixture("message_create_simple")["d"].clone())]);
    s.http.on("POST", SELECT, [Response::ok(json!({}))]);
    assert!(s.client.store.send_message(GENERAL, gif.url.clone()));
    s.client.store.gifs_selected(&gif.id);
    s.client.flush();
    let mut seen = [s.http.next_request().await, s.http.next_request().await];
    seen.sort_by(|a, b| a.path.cmp(&b.path));
    assert_eq!(seen[0].json()["content"], gif.url.as_str());
    assert_eq!((seen[1].method.as_str(), seen[1].path.as_str()), ("POST", SELECT));
    assert_eq!(seen[1].json(), json!({ "id": "7000000000000000001", "q": "chat & chien" }));
    no_official_headers(&seen[1]);

    // Tendances ; une réponse qui n'est pas une liste est une erreur affichée.
    s.http.on("GET", TRENDING_GIFS, [Response::ok(json!({ "pas": "une liste" }))]);
    s.client.store.gifs_trending("en-US");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, "/api/v10/gifs/trending-gifs?media_format=tinygif&limit=50&locale=en-US");
    s.client.until("échec", |e| matches!(e, Event::Gifs(GifEvent::Results { gifs: Err(_), .. }))).await;
    assert!(s.client.store.gifs.error.is_some() && !s.client.store.gifs.loading);
}

#[tokio::test]
async fn favorites_are_read_written_and_follow_the_gateway() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let kept = FavGif { url: "https://tenor.com/view/garde-synthetique".into(), src: "https://media.tenor.com/gardeAAAAC/garde.gif".into(), width: 220, height: 124, order: 3, video: false };
    assert_eq!(s.client.store.toggle_fav_emoji("fire"), Err(Refusal::Unknown), "rien n'est écrit avant la lecture");
    s.http.on("GET", FRECENCY, [Response::ok(settings(&synthetic(std::slice::from_ref(&kept), &["joy"], &[("fire", 9)], 7)))]);
    s.client.store.expressions_open();
    s.client.store.expressions_open();
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", FRECENCY));
    s.client.until("favoris", |e| matches!(e, Event::Gifs(GifEvent::Frecency { .. }))).await;
    let f = &s.client.store.gifs.frecency;
    assert_eq!((f.gifs.len(), f.emojis.clone(), f.frequent.clone()), (1, vec!["joy".to_string()], vec!["fire".to_string()]));
    assert_eq!(s.http.requests().iter().filter(|r| r.path == FRECENCY).count(), 1, "lus une seule fois");

    // Étoile : seul le champ des GIF favoris part, en entier, avec la version connue.
    let new = FavGif { url: "https://tenor.com/view/nouveau-synthetique".into(), src: "https://media.tenor.com/nouveauAAAAC/nouveau.gif".into(), width: 220, height: 220, order: 0, video: false };
    s.http.on("PATCH", FRECENCY, [Response::ok(json!({ "settings": "" }))]);
    assert_eq!(s.client.store.toggle_fav_gif(&new), Ok(()));
    assert_eq!(s.client.store.toggle_fav_emoji("fire"), Err(Refusal::Busy));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", FRECENCY));
    no_official_headers(&r);
    let body = r.json();
    assert_eq!(body["required_data_version"], 7);
    let sent = Msg::decode(&b64::decode(body["settings"].as_str().unwrap_or_default()).unwrap_or_default()).unwrap_or_default();
    assert_eq!(sent.top_numbers(), [2], "favorite_gifs seul");
    s.client.until("écrit", |e| matches!(e, Event::Gifs(GifEvent::FrecencyWritten { .. }))).await;
    let urls: Vec<&str> = s.client.store.gifs.frecency.gifs.iter().map(|g| g.url.as_str()).collect();
    assert_eq!(urls, ["https://tenor.com/view/nouveau-synthetique", "https://tenor.com/view/garde-synthetique"]);

    // Refus de Discord : rétabli, avec un message.
    s.http.on("PATCH", FRECENCY, [Response::error(500, "panne")]);
    assert_eq!(s.client.store.toggle_fav_emoji("fire"), Ok(()));
    assert!(s.client.store.gifs.frecency.is_fav_emoji("fire"));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("refus", |e| matches!(e, Event::Gifs(GifEvent::FrecencyWritten { result: Err(_), .. }))).await;
    assert_eq!(s.client.store.gifs.frecency.emojis, ["joy"]);

    // Modifiés depuis un autre client : la passerelle fait foi.
    let theirs = synthetic(&[], &["tada", "joy"], &[], 9);
    s.conn.dispatch("USER_SETTINGS_PROTO_UPDATE", 40, json!({ "settings": { "type": 2, "proto": b64::encode(&theirs) }, "partial": false })).await;
    s.client.until("mise à jour", |e| matches!(e, Event::Gifs(GifEvent::Frecency { .. }))).await;
    assert_eq!(s.client.store.gifs.frecency.emojis, ["tada", "joy"]);
    assert!(s.client.store.gifs.frecency.gifs.is_empty());
    assert!(matches!(s.client.store.gifs.frecency.state(), State::Ready(_)));
}

#[tokio::test]
async fn super_reaction_is_sent_with_its_type_and_follows_the_gateway() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.http.next_request().await;
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;
    let fire = || Emoji::Unicode("🔥".into());
    let base = format!("/api/v10/channels/{GENERAL}/messages/{BOB_MSG}/reactions/%F0%9F%94%A5");

    s.http.on("PUT", &format!("{base}/@me"), [Response::ok(json!({}))]);
    assert!(s.client.store.super_react(GENERAL, BOB_MSG, fire()));
    assert!(!s.client.store.super_react(GENERAL, BOB_MSG, fire()), "déjà en cours");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", format!("{base}/@me?type=1").as_str()));
    no_official_headers(&r);
    s.client.until("acceptée", |e| matches!(e, Event::Gifs(GifEvent::SuperReactDone { error: None, .. }))).await;
    // Le compte suit l'écho de la passerelle.
    let echo = json!({ "user_id": ME.to_string(), "channel_id": GENERAL.to_string(), "message_id": BOB_MSG.to_string(), "guild_id": ALPHA.to_string(), "emoji": { "id": null, "name": "🔥" }, "burst": true, "burst_colors": ["#ff8800"], "type": 1 });
    s.conn.dispatch("MESSAGE_REACTION_ADD", 41, echo).await;
    s.client.until("écho", |e| matches!(e, Event::Social(SocialEvent::Reaction { burst: true, .. }))).await;
    let mine = |s: &Setup| s.client.store.find_message(GENERAL, BOB_MSG).and_then(|m| m.meta.reactions.iter().find(|r| r.emoji.same(&fire())).map(|r| (r.burst, r.me_burst, r.normal)));
    assert_eq!(mine(&s), Some((1, true, 0)));

    // Deuxième geste : la mienne est retirée (type 1 dans le chemin).
    s.http.on("DELETE", &format!("{base}/1/@me"), [Response::error(403, "interdit")]);
    assert!(s.client.store.super_react(GENERAL, BOB_MSG, fire()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", format!("{base}/1/@me").as_str()));
    s.client.until("refus", |e| matches!(e, Event::Gifs(GifEvent::SuperReactDone { error: Some(_), .. }))).await;
    assert_eq!(mine(&s), Some((1, true, 0)), "refusée : rien ne change");
}

#[tokio::test]
async fn bot_account_sends_nothing() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    s.client.store.gifs_open("fr");
    s.client.store.gifs_search("chat", "fr");
    s.client.store.gifs_selected("1");
    s.client.flush();
    s.client.until("refus local", |e| matches!(e, Event::Gifs(GifEvent::Results { gifs: Err(_), .. }))).await;
    assert!(!s.http.requests().iter().any(|r| r.path.contains("/gifs/") || r.path.contains("settings-proto")), "aucune requête");
}
