//! Caméra et directs (phase 5, lot M4) face au faux serveur : `self_video`
//! dans l'op 4, ops 18, 19 et 20 sur la passerelle principale, événements
//! STREAM_CREATE, STREAM_UPDATE, STREAM_SERVER_UPDATE et STREAM_DELETE.
//! Aucune connexion vidéo : rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::stream::{stream_key, EndReason, StreamConnState, StreamEvent};
use ferrocord::state::video::Tile;
use ferrocord::state::voice::VoiceEvent;
use ferrocord::state::Event;
use ferrocord::video::Quality;
use serde_json::json;

const VOCAL: u64 = 2000000000000000004;
const SALON: u64 = 3000000000000000051;
const EVE: u64 = 1000000000000000006;

/// Connecté au salon vocal de « Serveur Vocal » (op 4 envoyée, état reçu).
async fn in_voice() -> Setup {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch_fixture("guild_create_voice", 2).await;
    s.client.until("salons vocaux", |e| matches!(e, Event::Voice(VoiceEvent::Guild { guild_id: VOCAL, .. }))).await;
    assert_eq!(s.client.store.voice_join(SALON), Ok(()));
    s.client.flush();
    s.conn.recv_op(4).await;
    s.conn.dispatch_fixture("voice_state_update", 3).await;
    s.client.until("mon état vocal", |e| matches!(e, Event::Voice(VoiceEvent::StateUpdate { user_id: ME, .. }))).await;
    s
}

#[tokio::test]
async fn camera_and_my_stream_use_op4_18_and_19() {
    let mut s = in_voice().await;
    assert!(s.client.store.video_set_camera(true));
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"], json!({ "guild_id": VOCAL.to_string(), "channel_id": SALON.to_string(), "self_mute": false, "self_deaf": false, "self_video": true }));
    // Muet ensuite : la caméra reste dans l'op 4.
    s.client.store.voice_set_mute(true);
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!((p["d"]["self_mute"].clone(), p["d"]["self_video"].clone()), (json!(true), json!(true)));

    let q = Quality { width: 1920, height: 1080, fps: 30 };
    assert!(s.client.store.video_start_share("ecran-1", "Écran 1", q, false));
    s.client.flush();
    let p = s.conn.recv_op(18).await;
    assert_eq!(p["d"], json!({ "type": "guild", "guild_id": VOCAL.to_string(), "channel_id": SALON.to_string(), "preferred_region": null }));
    // Aussitôt annoncé « en cours » (op 22), comme le client.
    let p = s.conn.recv_op(22).await;
    assert_eq!(p["d"], json!({ "stream_key": stream_key(Some(VOCAL), SALON, ME), "paused": false }));

    // Discord répond : création, puis serveur du direct (jeton ajouté ici).
    let mine = stream_key(Some(VOCAL), SALON, ME);
    s.conn.dispatch("STREAM_CREATE", 4, json!({ "stream_key": mine, "rtc_server_id": "6000000000000000009", "region": "rotterdam", "viewer_ids": [], "paused": false })).await;
    s.client.until("mon direct créé", |e| matches!(e, Event::Stream(StreamEvent::Create { .. }))).await;
    s.conn.dispatch("STREAM_SERVER_UPDATE", 5, json!({ "stream_key": mine, "endpoint": "direct-fictif.example.invalid:443", "token": "jeton-direct-de-test" })).await;
    let seen = s.client.until("serveur du direct", |e| matches!(e, Event::Stream(StreamEvent::Server { .. }))).await;
    assert!(!seen.contains("jeton-direct-de-test"), "jeton masqué dans les journaux : {seen}");
    let c = s.client.store.stream.get(&mine).cloned().unwrap_or_default();
    let StreamConnState::Connecting { endpoint, token, rtc_server_id } = &c.state else { panic!("{c:?}") };
    assert_eq!((endpoint.as_str(), token.expose(), *rtc_server_id), ("direct-fictif.example.invalid:443", "jeton-direct-de-test", 6000000000000000009));
    assert!(c.mine);

    s.client.store.video_stop_share();
    s.client.flush();
    let p = s.conn.recv_op(19).await;
    assert_eq!(p["d"], json!({ "stream_key": mine }));
    assert!(s.client.store.video.share.is_none() && s.client.store.stream.get(&mine).is_none());

    // Quitter le salon éteint la caméra : l'op 4 de départ n'en parle plus.
    s.client.store.voice_leave();
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"]["channel_id"], serde_json::Value::Null);
    assert!(p["d"].get("self_video").is_none(), "{p}");
    assert!(!s.client.store.video.camera);
}

#[tokio::test]
async fn watching_a_stream_follows_stream_events() {
    let mut s = in_voice().await;
    let eve = stream_key(Some(VOCAL), SALON, EVE);
    assert!(s.client.store.call_tiles(SALON).iter().any(|t| t.tile == Tile::Stream(eve.clone())), "Ève est en direct");
    assert!(s.client.store.video_watch(&eve));
    s.client.flush();
    let p = s.conn.recv_op(20).await;
    assert_eq!(p["d"], json!({ "stream_key": eve }));

    s.conn.dispatch_fixture("stream_create", 4).await;
    s.client.until("direct d'Ève", |e| matches!(e, Event::Stream(StreamEvent::Create { .. }))).await;
    assert_eq!(s.client.store.stream.viewers(&eve), &[ME]);
    s.conn.dispatch_fixture("stream_update", 5).await;
    s.client.until("spectateurs", |e| matches!(e, Event::Stream(StreamEvent::Update { .. }))).await;
    assert_eq!(s.client.store.stream.viewers(&eve), &[ME, ALICE]);
    assert!(s.client.store.stream.get(&eve).is_some_and(|c| c.paused));

    s.client.store.video_stop_watching(&eve);
    s.client.flush();
    let p = s.conn.recv_op(19).await;
    assert_eq!(p["d"], json!({ "stream_key": eve }));

    // Nouvel essai : direct plein, Discord le ferme et c'est annoncé.
    assert!(s.client.store.video_watch(&eve));
    s.client.flush();
    s.conn.recv_op(20).await;
    s.conn.dispatch_fixture("stream_delete", 6).await;
    s.client.until("direct fermé", |e| matches!(e, Event::Stream(StreamEvent::Delete { .. }))).await;
    assert!(!s.client.store.video.watching(&eve));
    assert_eq!(s.client.store.stream.take_ended(), Some((eve, EndReason::Full)));
}
