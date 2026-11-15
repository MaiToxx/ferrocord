//! Activité de jeu (lot C3) face au faux serveur : liste des jeux
//! détectables (`GET /games/detectable`, non documentée), op 3 quand un jeu
//! démarre puis s'arrête, statut invisible, adresse d'un bouton d'activité
//! (`GET …/activities/{app}/metadata`, non documentée).

use crate::support::*;
use ferrocord::state::activity::{ActivityEvent, RunningGame};
use ferrocord::state::Event;
use serde_json::json;
use std::time::Duration;

const LIST: &str = "/api/v10/games/detectable";

/// READY utilisateur avec les réglages synchronisés de `rest_settings_proto`
/// (statut inactif, statut personnalisé « En pause », jeu affiché).
async fn with_settings() -> Setup {
    let mut s = connected_with(USER_TOKEN, "ready_user_classic", |p| {
        p["d"]["user_settings_proto"] = fixture("rest_settings_proto")["settings"].clone();
    })
    .await;
    // La connexion a déjà demandé la liste (dans 60 s) : nouvelle demande sans attente.
    s.client.store.activity.load_delay = Some(Duration::ZERO);
    s.client.store.activity.on_ready();
    s
}

fn running(name: &str, start_ms: i64) -> Event {
    Event::Activity(ActivityEvent::Running(vec![RunningGame { app_id: Some(6000000000000000020), name: name.into(), start_ms }]))
}

#[tokio::test]
async fn detectable_list_then_op3_when_a_game_starts_and_stops() {
    let mut s = with_settings().await;
    s.http.on("GET", LIST, [Response::ok(fixture("rest_games_detectable"))]);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", LIST));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-super-properties"), "aucun en-tête du client officiel");
    s.client.until("liste", |e| matches!(e, Event::Activity(ActivityEvent::Detectable(_)))).await;
    assert!(s.client.store.activity.index.as_ref().is_some_and(|i| !i.is_empty()));

    // Un jeu démarre : op 3 avec le statut synchronisé et le statut personnalisé.
    s.client.store.apply(running("PUBG", 1_700_000_000_000));
    s.client.flush();
    let p = s.conn.recv_op(3).await;
    assert_eq!(
        p["d"],
        json!({ "since": 0, "status": "idle", "afk": false, "activities": [
            { "name": "Custom Status", "type": 4, "state": "En pause" },
            { "name": "PUBG", "type": 0, "application_id": "6000000000000000020", "timestamps": { "start": 1_700_000_000_000_i64 } }
        ] })
    );
    // Rien de neuf : rien n'est renvoyé. Puis le jeu s'arrête (op 3 après l'écart minimal de 4 s).
    tokio::time::sleep(Duration::from_secs(2)).await;
    s.client.flush();
    s.client.store.apply(Event::Activity(ActivityEvent::Running(Vec::new())));
    s.client.flush();
    let p = s.conn.recv_op(3).await;
    assert_eq!(p["d"]["activities"], json!([{ "name": "Custom Status", "type": 4, "state": "En pause" }]));
}

#[tokio::test]
async fn setting_off_or_unknown_list_sends_nothing() {
    let mut s = with_settings().await;
    // Liste indisponible : pas de détection automatique, aucune op 3 sans jeu.
    s.http.on("GET", LIST, [Response::error(500, "panne")]);
    s.client.flush();
    s.http.next_request().await;
    s.client.until("échec", |e| matches!(e, Event::Activity(ActivityEvent::DetectableFailed))).await;
    assert!(s.client.store.activity.failed);
    s.client.flush();
    let quiet = tokio::time::timeout(Duration::from_millis(300), s.conn.recv()).await;
    assert!(quiet.is_err(), "aucun paquet : {quiet:?}");
}

#[tokio::test]
async fn activity_button_url_is_asked_after_a_click() {
    let mut s = with_settings().await;
    let path = "/api/v10/users/1000000000000000002/sessions/0a1b2c3d4e5f/activities/6000000000000000001/metadata";
    s.http.on("GET", path, [Response::ok(fixture("rest_activity_metadata"))]);
    s.client.store.activity_button(1000000000000000002, "0a1b2c3d4e5f", Some(6000000000000000001), 0);
    s.client.flush();
    let r = loop {
        let r = s.http.next_request().await;
        if r.path != LIST {
            break r;
        }
    };
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", path));
    s.client.until("adresse", |e| matches!(e, Event::Activity(ActivityEvent::ButtonUrl(_)))).await;
    assert_eq!(s.client.store.activity.open_url.as_deref(), Some("https://exemple.invalid/rejoindre"));
}
