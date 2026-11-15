//! Salons et catégories (lot A2) : créer, modifier, supprimer, déplacer,
//! refus à double authentification et mises à jour par la passerelle, face
//! au faux serveur. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::admin::channels::{ChanEvent, ChanFailure, Drop, NewChannel, NewKind, Patch};
use ferrocord::state::Event;
use serde_json::{json, Value};

fn chan(e: &Event) -> Option<&ChanEvent> {
    match e {
        Event::ChannelAdmin(c) => Some(c),
        _ => None,
    }
}

fn id_of(m: &Value) -> Option<u64> {
    m["id"].as_str()?.parse().ok()
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

fn created(id: &str, name: &str, kind: u8, parent: Option<&str>) -> Value {
    json!({ "id": id, "guild_id": ALPHA.to_string(), "type": kind, "name": name, "position": 5, "parent_id": parent, "permission_overwrites": [], "nsfw": false, "topic": null, "rate_limit_per_user": 0, "flags": 0 })
}

fn new(name: &str, kind: NewKind, parent: Option<u64>) -> NewChannel {
    NewChannel { name: name.into(), kind, parent, overwrites: Vec::new(), topic: String::new(), nsfw: false, slowmode: 0 }
}

#[tokio::test]
async fn create_a_channel_in_a_category_then_a_category() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{ALPHA}/channels");
    s.http.on("POST", &path, [Response::ok(created("3000000000000000901", "aide", 15, Some("3000000000000000010"))), Response::ok(created("3000000000000000902", "Projets", 4, None))]);
    let mut forum = new(" aide ", NewKind::Forum, Some(DISCUSSIONS));
    forum.topic = "Questions".into();
    assert!(s.client.store.create_channel(ALPHA, forum));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", path.as_str()));
    // Sans dérogations : le salon reste synchronisé avec sa catégorie.
    assert_eq!(r.json(), json!({ "name": "aide", "type": 15, "parent_id": DISCUSSIONS.to_string(), "topic": "Questions" }));
    s.client.until("créé", |e| matches!(chan(e), Some(ChanEvent::Created { .. }))).await;

    // Une catégorie n'a jamais de parent, même si l'appelant en donne un.
    assert!(s.client.store.create_channel(ALPHA, new("Projets", NewKind::Category, Some(DISCUSSIONS))));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "name": "Projets", "type": 4 }));
    s.client.until("catégorie", |e| matches!(chan(e), Some(ChanEvent::Created { .. }))).await;
    assert!(!s.client.store.create_channel(ALPHA, new("   ", NewKind::Text, None)), "nom vide refusé sans requête");
}

#[tokio::test]
async fn load_then_modify_sends_only_the_changes() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/channels/{GENERAL}");
    let mut after = fixture("rest_channel_forum");
    after["id"] = GENERAL.to_string().into();
    after["guild_id"] = ALPHA.to_string().into();
    s.http.on("GET", &path, [Response::ok(after.clone())]);
    s.http.on("PATCH", &path, [Response::ok(after)]);
    assert!(s.client.store.load_channel(GENERAL));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", path.as_str()));
    s.client.until("détails", |e| matches!(chan(e), Some(ChanEvent::Details(_)))).await;
    assert_eq!(s.client.store.chan_admin.details.as_ref().map(|d| d.topic.as_str()), Some("Posez vos questions ici."));

    let patch = Patch { topic: Some("Nouveau sujet".into()), slowmode: Some(99_999), auto_archive: Some(1234), ..Patch::default() };
    assert!(s.client.store.save_channel(GENERAL, patch));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", path.as_str()));
    // Mode lent borné à 6 h ; durée d'archivage hors liste retirée.
    assert_eq!(r.json(), json!({ "topic": "Nouveau sujet", "rate_limit_per_user": 21600 }));
    s.client.until("enregistré", |e| matches!(chan(e), Some(ChanEvent::Saved { .. }))).await;
    assert!(!s.client.store.save_channel(GENERAL, Patch::default()), "rien à envoyer");
}

#[tokio::test]
async fn delete_removes_the_channel() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/channels/{HORS_SUJET}");
    s.http.on("DELETE", &path, [Response::ok(created(&HORS_SUJET.to_string(), "hors-sujet", 0, Some("3000000000000000010")))]);
    assert!(s.client.store.delete_channel(HORS_SUJET));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", path.as_str()));
    s.client.until("supprimé", |e| matches!(chan(e), Some(ChanEvent::Deleted { .. }))).await;
    let alpha = s.client.store.guilds.iter().find(|g| g.id == ALPHA);
    assert!(alpha.is_some_and(|g| !g.channels.iter().any(|c| c.id == HORS_SUJET)));
}

#[tokio::test]
async fn drop_sends_one_positions_request() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{ALPHA}/channels");
    s.http.on("PATCH", &path, [no_content(), no_content()]);
    // #hors-sujet au-dessus de #général : même catégorie, aucun parent envoyé.
    assert!(s.client.store.move_channel(ALPHA, HORS_SUJET, Drop::Before(GENERAL)));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", path.as_str()));
    let body = r.json();
    let list = body.as_array().cloned().unwrap_or_default();
    let of = |id: u64| list.iter().find(|m| id_of(m) == Some(id)).cloned().unwrap_or(Value::Null);
    assert_eq!(of(HORS_SUJET), json!({ "id": HORS_SUJET.to_string(), "position": 0 }));
    assert_eq!(of(GENERAL), json!({ "id": GENERAL.to_string(), "position": 1 }));
    s.client.until("déplacé", |e| matches!(chan(e), Some(ChanEvent::Moved { .. }))).await;

    // #général dans Archives : nouveau parent, permissions synchronisées.
    assert!(s.client.store.move_channel(ALPHA, GENERAL, Drop::Into(ARCHIVES)));
    s.client.flush();
    let r = s.http.next_request().await;
    let body = r.json();
    let moved = body.as_array().and_then(|l| l.iter().find(|m| id_of(m) == Some(GENERAL)).cloned()).unwrap_or(Value::Null);
    assert_eq!(moved["parent_id"], ARCHIVES.to_string());
    assert_eq!(moved["lock_permissions"], true);
    assert_eq!(body.as_array().map(|l| l.iter().filter(|m| m.get("parent_id").is_some()).count()), Some(1), "un seul changement de parent par requête");
    s.client.until("déplacé 2", |e| matches!(chan(e), Some(ChanEvent::Moved { .. }))).await;
}

#[tokio::test]
async fn two_factor_refusal_is_reported() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/channels/{GENERAL}");
    s.http.on("DELETE", &path, [Response::raw(401, json!({ "message": "Two factor is required for this operation", "code": 60003 }))]);
    assert!(s.client.store.delete_channel(GENERAL));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("refus", |e| matches!(chan(e), Some(ChanEvent::Failed { failure: ChanFailure::Problem(ferrocord::state::Problem::MfaRequired), .. }))).await;
    assert!(s.client.store.guilds.iter().any(|g| g.channels.iter().any(|c| c.id == GENERAL)), "rien n'est retiré");
    assert_eq!(s.http.requests().len(), 1, "jamais retenté");
}

#[tokio::test]
async fn gateway_updates_overwrites_and_positions() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let mut d = created(&GENERAL.to_string(), "général", 0, Some("3000000000000000010"));
    d["position"] = 7.into();
    d["topic"] = "Sujet venu de la passerelle".into();
    d["permission_overwrites"] = json!([{ "id": ALPHA.to_string(), "type": 0, "allow": "0", "deny": "2048" }]);
    s.conn.dispatch("CHANNEL_UPDATE", 40, d).await;
    s.client.until("mis à jour", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    let c = s.client.store.guilds.iter().find(|g| g.id == ALPHA).and_then(|g| g.channels.iter().find(|c| c.id == GENERAL).cloned());
    let c = c.unwrap_or_else(|| panic!("#général absent"));
    assert_eq!(c.position, 7);
    assert_eq!(c.overwrites.as_ref().map(|o| o.len()), Some(1));

    s.conn.dispatch("CHANNEL_DELETE", 41, created(&HORS_SUJET.to_string(), "hors-sujet", 0, Some("3000000000000000010"))).await;
    s.client.until("supprimé", |e| matches!(e, Event::ChannelDelete { .. })).await;
    assert!(!s.client.store.guilds.iter().any(|g| g.channels.iter().any(|c| c.id == HORS_SUJET)));
}

/// Salon vocal (phase 10, lot S) : régions lues une fois, réglages bornés
/// avant l'envoi, seuls les champs modifiés partent.
#[tokio::test]
async fn voice_channel_regions_and_bounded_settings() {
    const VOCAL: u64 = 3000000000000000014;
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/channels/{VOCAL}");
    let mut details = fixture("rest_channel_voice");
    details["id"] = VOCAL.to_string().into();
    details["guild_id"] = ALPHA.to_string().into();
    s.http.on("GET", "/api/v10/voice/regions", [Response::ok(fixture("rest_voice_regions"))]);
    s.http.on("GET", &path, [Response::ok(details.clone())]);
    s.http.on("PATCH", &path, [Response::ok(details)]);

    assert!(s.client.store.load_voice_regions());
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/voice/regions"));
    s.client.until("régions", |e| matches!(chan(e), Some(ChanEvent::Regions(_)))).await;
    let regions = s.client.store.chan_admin.regions.clone().unwrap_or_default();
    assert_eq!(regions.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["brazil", "rotterdam", "us-east"]);
    assert!(!s.client.store.load_voice_regions(), "une seule lecture");

    assert!(s.client.store.load_channel(VOCAL));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("détails", |e| matches!(chan(e), Some(ChanEvent::Details(_)))).await;
    let d = s.client.store.chan_admin.details.clone().unwrap_or_default();
    assert_eq!((d.bitrate, d.user_limit, d.rtc_region.as_deref()), (Some(64_000), Some(8), Some("rotterdam")));

    // Serveur sans boost : 96 kbps au plus ; 99 personnes au plus ; qualité inconnue retirée.
    let patch = Patch { bitrate: Some(500_000), user_limit: Some(500), rtc_region: Some(None), video_quality: Some(7), ..Patch::default() };
    assert!(s.client.store.save_channel(VOCAL, patch));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", path.as_str()));
    assert_eq!(r.json(), json!({ "bitrate": 96000, "user_limit": 99, "rtc_region": null }));
    s.client.until("enregistré", |e| matches!(chan(e), Some(ChanEvent::Saved { .. }))).await;
}
