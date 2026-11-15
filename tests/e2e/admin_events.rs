//! Événements programmés, écran de bienvenue et intégration (lot A7), face
//! au faux serveur. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::admin::community::{CommunityAction, CommunityEvent, Loadable, WelcomeChannel, WelcomeScreen};
use ferrocord::state::admin::events::{EventAction, EventDraft, EventEvent, Failure, Place, Status};
use ferrocord::state::Event;
use serde_json::{json, Value};

const SOIREE: u64 = 5000000000000000001;
const PIQUE_NIQUE: u64 = 5000000000000000002;
const COMMUNITY_GUILD: u64 = 2000000000000000501;

fn events(e: &Event) -> Option<&EventEvent> {
    match e {
        Event::GuildEvents(x) => Some(x),
        _ => None,
    }
}

fn community(e: &Event) -> Option<&CommunityEvent> {
    match e {
        Event::Community(x) => Some(x),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

fn scheduled() -> String {
    format!("/api/v10/guilds/{ALPHA}/scheduled-events")
}

#[tokio::test]
async fn list_is_loaded_with_my_subscriptions_then_interest_is_sent() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &scheduled(), [Response::ok(fixture("rest_guild_scheduled_events"))]);
    let mine = json!([{ "guild_scheduled_event_id": PIQUE_NIQUE.to_string(), "user_id": ME.to_string(), "response": 1 }, { "guild_scheduled_event_id": "5000000000000000003", "guild_scheduled_event_exception_id": "9", "response": 1 }]);
    s.http.on("GET", "/api/v10/users/@me/scheduled-events", [Response::ok(mine)]);
    assert!(s.client.store.load_events(ALPHA));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", format!("{}?with_user_count=true", scheduled()).as_str()));
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("/api/v10/users/@me/scheduled-events?guild_ids={ALPHA}"));
    s.client.until("liste", |e| matches!(events(e), Some(EventEvent::Loaded { .. }))).await;
    let g = s.client.store.events.guild(ALPHA).cloned().unwrap_or_default();
    assert_eq!(g.list.len(), 3, "l'entrée illisible est ignorée");
    assert!(g.loaded);
    assert!(s.client.store.event_interested(ALPHA, PIQUE_NIQUE));
    assert!(!s.client.store.event_interested(ALPHA, SOIREE), "inscription à une seule occurrence ignorée");
    assert_eq!(s.client.store.find_event(ALPHA, SOIREE).and_then(|e| e.user_count), Some(12));

    let path = format!("{}/{SOIREE}/users/@me", scheduled());
    s.http.on("PUT", &path, [no_content()]);
    assert!(s.client.store.set_event_interest(ALPHA, SOIREE, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", path.as_str()));
    s.client.until("intéressé", |e| matches!(events(e), Some(EventEvent::Interest { on: true, .. }))).await;
    assert!(s.client.store.event_interested(ALPHA, SOIREE));
    // Le nombre ne bouge qu'avec la passerelle.
    assert_eq!(s.client.store.find_event(ALPHA, SOIREE).and_then(|e| e.user_count), Some(12));
    let mut add = fixture("guild_scheduled_event_user_add")["d"].clone();
    add["guild_scheduled_event_id"] = SOIREE.to_string().into();
    add["user_id"] = ME.to_string().into();
    s.conn.dispatch("GUILD_SCHEDULED_EVENT_USER_ADD", 40, add).await;
    s.client.until("ajout", |e| matches!(events(e), Some(EventEvent::UserAdd { .. }))).await;
    assert_eq!(s.client.store.find_event(ALPHA, SOIREE).and_then(|e| e.user_count), Some(13));
}

#[tokio::test]
async fn create_sends_the_documented_body_and_gateway_adds_events() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let created = fixture("guild_scheduled_event_create")["d"].clone();
    s.http.on("POST", &scheduled(), [Response::ok(created)]);
    let start = 1_900_000_000_000; // 2030-03-17T17:46:40Z
    let draft = EventDraft { name: " Lecture ".into(), description: String::new(), start_ms: start, end_ms: Some(start + 3_600_000), place: Place::External("Médiathèque".into()), recurrence: None, image: None };
    assert_eq!(s.client.store.create_event(ALPHA, &draft), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "POST");
    assert_eq!(
        r.json(),
        json!({ "name": "Lecture", "description": null, "scheduled_start_time": "2030-03-17T17:46:40.000Z", "scheduled_end_time": "2030-03-17T18:46:40.000Z",
                "entity_type": 3, "channel_id": null, "entity_metadata": { "location": "Médiathèque" }, "privacy_level": 2 })
    );
    s.client.until("créé", |e| matches!(events(e), Some(EventEvent::Done { action: EventAction::Create, .. }))).await;
    assert!(s.client.store.find_event(ALPHA, 5000000000000000004).is_some());

    // Un autre événement arrive par la passerelle.
    let mut d = fixture("guild_scheduled_event_create")["d"].clone();
    d["id"] = "5000000000000000009".into();
    s.conn.dispatch("GUILD_SCHEDULED_EVENT_CREATE", 41, d).await;
    s.client.until("passerelle", |e| matches!(events(e), Some(EventEvent::Upsert(ev)) if ev.id == 5000000000000000009)).await;
    assert_eq!(s.client.store.events.upcoming(ALPHA), 2);
    s.conn.dispatch("GUILD_SCHEDULED_EVENT_DELETE", 42, json!({ "id": "5000000000000000009", "guild_id": ALPHA.to_string() })).await;
    s.client.until("supprimé", |e| matches!(events(e), Some(EventEvent::Deleted { .. }))).await;
    assert!(s.client.store.find_event(ALPHA, 5000000000000000009).is_none());
}

#[tokio::test]
async fn mfa_refusal_is_reported_as_such() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &scheduled(), [Response::ok(fixture("rest_guild_scheduled_events"))]);
    s.http.on("GET", "/api/v10/users/@me/scheduled-events", [Response::ok(json!([]))]);
    s.client.store.load_events(ALPHA);
    s.client.flush();
    s.client.until("liste", |e| matches!(events(e), Some(EventEvent::Loaded { .. }))).await;
    s.http.next_request().await;
    s.http.next_request().await;
    let path = format!("{}/{SOIREE}", scheduled());
    s.http.on("PATCH", &path, [Response::raw(403, json!({ "code": 60003, "message": "Two factor is required for this operation" }))]);
    assert!(s.client.store.set_event_status(ALPHA, SOIREE, Status::Canceled));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("PATCH", json!({ "status": 4 })));
    s.client.until("A2F", |e| matches!(events(e), Some(EventEvent::Failed { failure: Failure::Problem(ferrocord::state::Problem::MfaRequired), .. }))).await;
    assert_eq!(s.http.requests().iter().filter(|r| r.method == "PATCH").count(), 1, "jamais retenté");
    assert_eq!(s.client.store.find_event(ALPHA, SOIREE).map(|e| e.status), Some(Status::Scheduled), "rien d'inventé");
    assert!(s.client.store.events.take_notice().is_some());
}

#[tokio::test]
async fn community_guild_features_and_onboarding_answers() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch("GUILD_CREATE", 30, fixture("guild_create_community")["d"].clone()).await;
    s.client.until("communauté", |e| matches!(community(e), Some(CommunityEvent::Info { guild_id, .. }) if *guild_id == COMMUNITY_GUILD)).await;
    let info = s.client.store.guild_info(COMMUNITY_GUILD).cloned().unwrap_or_default();
    assert!(info.has("COMMUNITY") && info.has("GUILD_ONBOARDING"));
    assert_eq!((info.rules_channel, info.updates_channel), (Some(3000000000000000502), Some(3000000000000000503)));
    assert_eq!(s.client.store.community.venues(COMMUNITY_GUILD).len(), 2);

    // Premières réponses : POST.
    let path = format!("/api/v10/guilds/{COMMUNITY_GUILD}/onboarding");
    let mut o = fixture("rest_guild_onboarding");
    o["responses"] = json!([]);
    s.http.on("GET", &path, [Response::ok(o)]);
    assert!(s.client.store.load_onboarding(COMMUNITY_GUILD));
    s.client.flush();
    s.client.until("intégration", |e| matches!(community(e), Some(CommunityEvent::Onboarding { .. }))).await;
    assert!(matches!(s.client.store.community.onboarding.get(&COMMUNITY_GUILD), Some(Loadable::Ready(_))));
    let answers = format!("{path}-responses");
    s.http.on("POST", &answers, [no_content()]);
    assert_eq!(s.client.store.respond_onboarding(COMMUNITY_GUILD, &[6000000000000000011]), Ok(()));
    s.client.flush();
    let _ = s.http.next_request().await; // GET
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", answers.as_str()));
    let b = r.json();
    assert_eq!(b["onboarding_responses"], json!(["6000000000000000011"]));
    assert!(b["onboarding_prompts_seen"]["6000000000000000001"].is_number());
    assert!(b["onboarding_responses_seen"]["6000000000000000022"].is_number());
    s.client.until("répondu", |e| matches!(community(e), Some(CommunityEvent::Responded { .. }))).await;
    match s.client.store.community.onboarding.get(&COMMUNITY_GUILD) {
        Some(Loadable::Ready(o)) => assert_eq!(o.responses, vec![6000000000000000011]),
        other => panic!("intégration : {other:?}"),
    }
    assert_eq!(s.client.store.events.guild(COMMUNITY_GUILD).map(|g| g.list.len()), Some(1), "événement lu dans GUILD_CREATE");
}

#[tokio::test]
async fn welcome_screen_is_saved_then_reloaded() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{COMMUNITY_GUILD}/welcome-screen");
    s.http.on("PATCH", &path, [Response::ok(fixture("rest_welcome_screen"))]);
    s.http.on("GET", &path, [Response::ok(fixture("rest_welcome_screen"))]);
    let screen = WelcomeScreen { description: "Bonjour".into(), channels: vec![WelcomeChannel { channel_id: 3000000000000000502, description: "Lisez les règles".into(), emoji_id: None, emoji_name: None }] };
    assert_eq!(s.client.store.save_welcome(COMMUNITY_GUILD, true, &screen), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "PATCH");
    assert_eq!(
        r.json(),
        json!({ "enabled": true, "description": "Bonjour", "welcome_channels": [{ "channel_id": "3000000000000000502", "description": "Lisez les règles", "emoji_id": null, "emoji_name": null }] })
    );
    s.client.until("enregistré", |e| matches!(community(e), Some(CommunityEvent::Saved { action: CommunityAction::SaveWelcome, .. }))).await;
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", path.as_str()), "relu, rien d'inventé");
    s.client.until("relu", |e| matches!(community(e), Some(CommunityEvent::Welcome { .. }))).await;
}
