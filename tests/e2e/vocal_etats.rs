//! Vocal des serveurs (phase 4, lot V4) face au faux serveur : états
//! vocaux reçus, op 4 envoyée sur la passerelle principale (rejoindre,
//! muet, sourdine, quitter), permissions, modération vocale avec la raison
//! dans `X-Audit-Log-Reason`. Aucune connexion vocale : rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::voice::{JoinBlock, VoiceConnState, VoiceEvent, VoiceMod};
use ferrocord::state::Event;
use serde_json::{json, Value};

const VOCAL: u64 = 2000000000000000004;
const SALON: u64 = 3000000000000000051;
const PETIT: u64 = 3000000000000000052;
const ALPHA_VOCAL: u64 = 3000000000000000014;
const DANA: u64 = 1000000000000000005;

/// Session ouverte, puis le serveur vocal arrive (GUILD_CREATE).
async fn with_voice_guild() -> Setup {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch_fixture("guild_create_voice", 2).await;
    s.client.until("salons vocaux", |e| matches!(e, Event::Voice(VoiceEvent::Guild { guild_id: VOCAL, .. }))).await;
    s
}

#[tokio::test]
async fn voice_states_arrive_and_op4_follows_join_mute_and_leave() {
    let mut s = with_voice_guild().await;
    let v = &s.client.store.voice;
    assert_eq!(v.channels(VOCAL).iter().map(|c| c.id).collect::<Vec<_>>(), vec![SALON, PETIT, 3000000000000000053]);
    assert_eq!(v.members(SALON).count(), 3);
    assert!(v.state(DANA).is_some_and(|d| d.self_mute && d.self_video));
    // Salon vocal d'Alpha (READY) : connu aussi.
    assert_eq!(v.channels(ALPHA).len(), 1);

    assert_eq!(s.client.store.voice_join(SALON), Ok(()));
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"], json!({ "guild_id": VOCAL.to_string(), "channel_id": SALON.to_string(), "self_mute": false, "self_deaf": false }));
    assert_eq!(s.client.store.voice.conn, VoiceConnState::Joining);

    // Discord répond : mon état vocal (session), puis le serveur vocal.
    s.conn.dispatch_fixture("voice_state_update", 3).await;
    s.client.until("mon état vocal", |e| matches!(e, Event::Voice(VoiceEvent::StateUpdate { user_id: ME, .. }))).await;
    let mut server = fixture("voice_server_update");
    server["d"]["token"] = "jeton-vocal-de-test".into();
    s.conn.dispatch("VOICE_SERVER_UPDATE", 4, server["d"].clone()).await;
    let seen = s.client.until("serveur vocal", |e| matches!(e, Event::Voice(VoiceEvent::Server { .. }))).await;
    assert!(!seen.contains("jeton-vocal-de-test"), "jeton masqué dans les journaux : {seen}");
    let VoiceConnState::Connecting { endpoint, token, session_id } = &s.client.store.voice.conn else { panic!("{:?}", s.client.store.voice.conn) };
    assert_eq!((endpoint.as_str(), token.expose(), session_id.as_str()), ("voix-fictive.example.invalid:443", "jeton-vocal-de-test", "session-vocale-fictive"));
    assert_eq!(s.client.store.voice.members(SALON).count(), 4);

    // Sourdine : muet aussi, dans la même op 4.
    s.client.store.voice_set_deaf(true);
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!((p["d"]["self_mute"].clone(), p["d"]["self_deaf"].clone()), (Value::Bool(true), Value::Bool(true)));

    // Changer de salon : une op 4 vers le nouveau.
    s.client.store.voice_set_deaf(false);
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"]["self_deaf"], false);
    assert_eq!(s.client.store.voice_join(PETIT), Ok(()));
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"]["channel_id"], PETIT.to_string());

    s.client.store.voice_leave();
    s.client.flush();
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"], json!({ "guild_id": VOCAL.to_string(), "channel_id": null, "self_mute": false, "self_deaf": false }));
    assert_eq!(s.client.store.voice.conn, VoiceConnState::Idle);

    // Quelqu'un part : retiré de la liste.
    s.conn.dispatch("VOICE_STATE_UPDATE", 5, json!({ "guild_id": VOCAL.to_string(), "channel_id": null, "user_id": DANA.to_string(), "session_id": "s-dana", "deaf": false, "mute": false, "self_deaf": false, "self_mute": false, "self_video": false, "suppress": false, "request_to_speak_timestamp": null })).await;
    s.client.until("départ de Dana", |e| matches!(e, Event::Voice(VoiceEvent::StateUpdate { user_id: DANA, state: None, .. }))).await;
    assert!(s.client.store.voice.state(DANA).is_none());
}

#[tokio::test]
async fn permissions_block_joining_and_nothing_is_sent() {
    // Alpha, membre connu : @everyone voit et écrit, mais pas de CONNECT.
    let mut s = connected(USER_TOKEN, "ready_user_roles").await;
    s.client.until("salons vocaux d'Alpha", |e| matches!(e, Event::Voice(VoiceEvent::Guild { guild_id: ALPHA, .. }))).await;
    assert!(s.client.store.voice.perms(ALPHA_VOCAL).is_some(), "permissions connues");
    assert_eq!(s.client.store.voice_join(ALPHA_VOCAL), Err(JoinBlock::NoConnect));
    assert!(s.client.store.take_commands().is_empty());
    assert!(!s.client.store.voice_can_moderate(ALICE, VoiceMod::Mute(true)), "Alice n'est pas en vocal sur Alpha");
    // Rien n'est parti : le prochain paquet est un heartbeat demandé.
    s.conn.request_heartbeat().await;
    assert_eq!(s.conn.recv().await["op"], 1);
}

#[tokio::test]
async fn moderation_patches_the_member_with_the_reason() {
    let mut s = with_voice_guild().await;
    let member = format!("/api/v10/guilds/{VOCAL}/members/{DANA}");
    s.http.on("PATCH", &member, [Response::ok(json!({})), Response::error(403, "Missing Permissions")]);
    // Propriétaire du serveur : tout est permis, vers un salon où je peux me connecter.
    assert!(s.client.store.voice_can_moderate(DANA, VoiceMod::Move(PETIT)));
    assert!(s.client.store.voice_moderate(DANA, VoiceMod::Mute(true), Some("bruit de fond".into())));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", member.as_str()));
    assert_eq!(r.json(), json!({ "mute": true }));
    assert_eq!(r.headers.iter().find(|(n, _)| n == "x-audit-log-reason").map(|(_, v)| v.as_str()), Some("bruit%20de%20fond"));
    s.client.until("modération faite", |e| matches!(e, Event::Voice(VoiceEvent::Moderated { .. }))).await;

    assert!(s.client.store.voice_moderate(DANA, VoiceMod::Disconnect, None));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "channel_id": null }));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-audit-log-reason"));
    s.client.until("modération refusée", |e| matches!(e, Event::Voice(VoiceEvent::ModFailed { .. }))).await;
}
