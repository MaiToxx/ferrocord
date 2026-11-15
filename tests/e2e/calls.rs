//! Appels privés, conférences, statut des salons vocaux et tableau de sons
//! (lot V6), face au faux serveur. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::calls::{CallAction, CallEvent, Loadable};
use ferrocord::state::stage::{Role, StageAction, StageEvent};
use ferrocord::state::voice::{VoiceEvent, VoiceState};
use ferrocord::state::Event;
use serde_json::{json, Value};

const CLUB: u64 = 2000000000000000601;
const SCENE: u64 = 3000000000000000601;
const VOCAL: u64 = 3000000000000000602;
const BOB: u64 = 1000000000000000003;
const QUACK: u64 = 7000000000000000001;

fn calls(e: &Event) -> Option<&CallEvent> {
    match e {
        Event::Calls(x) => Some(x),
        _ => None,
    }
}

fn stage(e: &Event) -> Option<&StageEvent> {
    match e {
        Event::Stage(x) => Some(x),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

fn d(name: &str) -> Value {
    fixture(name)["d"].clone()
}

#[tokio::test]
async fn incoming_call_rings_then_decline_stops_my_ringing() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch("CALL_CREATE", 30, d("call_create")).await;
    s.client.until("appel", |e| matches!(calls(e), Some(CallEvent::Upsert { .. }))).await;
    assert_eq!(s.client.store.incoming_calls(), vec![DM_ALICE]);
    assert_eq!(s.client.store.voice.members(DM_ALICE).map(|v| v.user_id).collect::<Vec<_>>(), vec![ALICE]);

    let path = format!("/api/v10/channels/{DM_ALICE}/call/stop-ringing");
    s.http.on("POST", &path, [no_content()]);
    assert!(s.client.store.decline_call(DM_ALICE));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("POST", path.as_str(), json!({ "recipients": [ME.to_string()] })));
    s.client.until("refus", |e| matches!(calls(e), Some(CallEvent::Done { action: CallAction::StopRinging, .. }))).await;
    assert!(s.client.store.incoming_calls().is_empty());

    s.conn.dispatch("CALL_UPDATE", 31, d("call_update")).await;
    s.client.until("mise à jour", |e| matches!(calls(e), Some(CallEvent::Upsert { voice_states: None, .. }))).await;
    assert!(s.client.store.calls.call(DM_ALICE).is_some_and(|c| c.ringing.is_empty()));
    assert_eq!(s.client.store.voice.count(DM_ALICE), 1, "CALL_UPDATE garde les états connus");
    s.conn.dispatch("CALL_DELETE", 32, d("call_delete")).await;
    s.client.until("fin", |e| matches!(calls(e), Some(CallEvent::Delete { .. }))).await;
    assert!(s.client.store.calls.call(DM_ALICE).is_none());
}

#[tokio::test]
async fn start_call_joins_then_rings_once_the_call_exists() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.client.store.start_call(DM_ALICE);
    s.client.flush();
    // « Appeler » rejoint le salon du MP : op 4 sans serveur.
    let p = s.conn.recv_op(4).await;
    assert_eq!(p["d"], json!({ "guild_id": null, "channel_id": DM_ALICE.to_string(), "self_mute": false, "self_deaf": false }));
    let path = format!("/api/v10/channels/{DM_ALICE}/call/ring");
    s.http.on("POST", &path, [no_content()]);
    // Discord crée l'appel quand je rejoins le salon (connexion vocale, V7).
    let mut call = d("call_create");
    call["ringing"] = json!([]);
    call["voice_states"] = json!([{ "user_id": ME.to_string(), "channel_id": DM_ALICE.to_string(), "suppress": false }]);
    s.conn.dispatch("CALL_CREATE", 30, call).await;
    s.client.until("appel", |e| matches!(calls(e), Some(CallEvent::Upsert { .. }))).await;
    let r = s.http.next_request().await;
    assert_eq!((r.path.as_str(), r.json()), (path.as_str(), json!({ "recipients": null })));
    s.client.until("sonnerie", |e| matches!(calls(e), Some(CallEvent::Done { action: CallAction::Ring, .. }))).await;
    assert!(s.client.store.incoming_calls().is_empty(), "je ne sonne pas pour mon propre appel");
    assert_eq!(s.http.requests().iter().filter(|r| r.path == path).count(), 1);
}

#[tokio::test]
async fn stage_hands_speakers_and_end() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch("GUILD_CREATE", 30, d("guild_create_stage")).await;
    s.client.until("conférences", |e| matches!(stage(e), Some(StageEvent::Guild { guild_id: CLUB, .. }))).await;
    assert_eq!(s.client.store.stage.instance(SCENE).map(|i| i.topic.as_str()), Some("Questions du jeudi"));
    assert_eq!(s.client.store.stage.status(VOCAL), Some("On joue aux échecs"));
    assert!(s.client.store.voice_room(SCENE).is_some_and(|r| r.venue.stage));
    assert_eq!(s.client.store.stage_people(SCENE).iter().map(|(_, r)| *r).collect::<Vec<_>>(), vec![Role::Speaker, Role::Hand, Role::Listener]);
    assert!(s.client.store.stage_moderator(CLUB), "propriétaire");

    // Je rejoins la scène comme auditeur, puis je lève la main.
    let me = json!({ "guild_id": CLUB.to_string(), "channel_id": SCENE.to_string(), "user_id": ME.to_string(), "suppress": true });
    s.conn.dispatch("VOICE_STATE_UPDATE", 31, me).await;
    s.client.until("moi", |e| matches!(e, Event::Voice(VoiceEvent::StateUpdate { user_id: ME, .. }))).await;
    let mine = format!("/api/v10/guilds/{CLUB}/voice-states/@me");
    s.http.on("PATCH", &mine, [no_content()]);
    assert_eq!(s.client.store.raise_hand(SCENE, true), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", mine.as_str()));
    assert_eq!(r.json()["channel_id"], SCENE.to_string());
    assert!(r.json()["request_to_speak_timestamp"].as_str().is_some_and(|t| t.ends_with('Z')));
    s.client.until("main", |e| matches!(stage(e), Some(StageEvent::Done { busy: (StageAction::Hand, _, _) }))).await;
    s.conn.dispatch("VOICE_STATE_UPDATE", 32, d("voice_state_update_stage")).await;
    s.client.until("main de Bob", |e| matches!(e, Event::Voice(VoiceEvent::StateUpdate { user_id: BOB, .. }))).await;

    // Modératrice : inviter Bob à parler.
    let bob = format!("/api/v10/guilds/{CLUB}/voice-states/{BOB}");
    s.http.on("PATCH", &bob, [no_content()]);
    assert_eq!(s.client.store.set_speaker(SCENE, Some(BOB), true), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.path.as_str(), r.json()), (bob.as_str(), json!({ "channel_id": SCENE.to_string(), "suppress": false })));
    s.client.until("orateur", |e| matches!(stage(e), Some(StageEvent::Done { busy: (StageAction::Speaker, _, _) }))).await;

    // Fin refusée (A2F), puis acceptée.
    let end = format!("/api/v10/stage-instances/{SCENE}");
    s.http.on("DELETE", &end, [Response::raw(403, json!({ "code": 60003, "message": "Two factor is required for this operation" })), no_content()]);
    assert_eq!(s.client.store.end_stage(SCENE), Ok(()));
    s.client.flush();
    s.client.until("A2F", |e| matches!(stage(e), Some(StageEvent::Failed { problem: Some(ferrocord::state::Problem::MfaRequired), .. }))).await;
    assert!(s.client.store.stage.take_notice().is_some());
    assert!(s.client.store.stage.instance(SCENE).is_some(), "rien d'inventé");
    assert_eq!(s.client.store.end_stage(SCENE), Ok(()));
    s.client.flush();
    s.client.until("fin", |e| matches!(stage(e), Some(StageEvent::Delete { .. }))).await;
    assert!(s.client.store.stage.instance(SCENE).is_none());
}

#[tokio::test]
async fn start_stage_and_voice_status() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let mut g = d("guild_create_stage");
    g["stage_instances"] = json!([]);
    s.conn.dispatch("GUILD_CREATE", 30, g).await;
    s.client.until("serveur", |e| matches!(stage(e), Some(StageEvent::Guild { guild_id: CLUB, .. }))).await;
    s.http.on("POST", "/api/v10/stage-instances", [Response::ok(d("stage_instance_create"))]);
    assert_eq!(s.client.store.start_stage(SCENE, "Questions du jeudi", true), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "channel_id": SCENE.to_string(), "topic": "Questions du jeudi", "privacy_level": 2, "send_start_notification": true }));
    s.client.until("conférence", |e| matches!(stage(e), Some(StageEvent::Done { .. }))).await;
    assert!(s.client.store.stage.instance(SCENE).is_some());

    // Statut : il faut être dans le salon.
    assert!(s.client.store.set_voice_status(VOCAL, "Échecs").is_ok(), "propriétaire : MANAGE_CHANNELS suffit");
    let path = format!("/api/v10/channels/{VOCAL}/voice-status");
    s.http.on("PUT", &path, [no_content()]);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PUT", path.as_str(), json!({ "status": "Échecs" })));
    s.conn.dispatch("VOICE_CHANNEL_STATUS_UPDATE", 31, d("voice_channel_status_update")).await;
    s.client.until("statut", |e| matches!(stage(e), Some(StageEvent::Status { .. }))).await;
    assert_eq!(s.client.store.stage.status(VOCAL), Some("Partie d'échecs en cours"));
}

#[tokio::test]
async fn soundboard_lists_play_and_effects() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch("GUILD_CREATE", 30, d("guild_create_stage")).await;
    s.client.until("serveur", |e| matches!(stage(e), Some(StageEvent::Guild { guild_id: CLUB, .. }))).await;
    s.http.on("GET", "/api/v10/soundboard-default-sounds", [Response::ok(fixture("rest_soundboard_default_sounds"))]);
    s.http.on("GET", &format!("/api/v10/guilds/{CLUB}/soundboard-sounds"), [Response::ok(fixture("rest_guild_soundboard_sounds"))]);
    s.client.store.load_soundboard(Some(CLUB));
    s.client.flush();
    s.client.until("sons par défaut", |e| matches!(calls(e), Some(CallEvent::DefaultSounds(_)))).await;
    s.client.until("sons du serveur", |e| matches!(calls(e), Some(CallEvent::Done { action: CallAction::GuildSounds, .. }))).await;
    assert!(matches!(s.client.store.calls.default_sounds(), Loadable::Ready(l) if l.len() == 3));
    assert!(s.client.store.expressions.sounds(CLUB).is_some_and(|l| !l.is_empty()));

    // Pas connecté : rien ne part ; connecté : le son part.
    assert!(!s.client.store.play_sound(VOCAL, QUACK, None));
    let me = VoiceState { guild_id: Some(CLUB), channel_id: VOCAL, user_id: ME, ..Default::default() };
    s.client.store.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(CLUB), user_id: ME, state: Some(me), session_id: None }));
    let path = format!("/api/v10/channels/{VOCAL}/send-soundboard-sound");
    s.http.on("POST", &path, [no_content()]);
    assert!(s.client.store.play_sound(VOCAL, QUACK, None));
    s.client.flush();
    // Les deux listes demandées avant sont passées.
    let r = loop {
        let r = s.http.next_request().await;
        if r.path == path {
            break r;
        }
    };
    assert_eq!((r.path.as_str(), r.json()), (path.as_str(), json!({ "sound_id": QUACK.to_string() })));
    s.client.until("son joué", |e| matches!(calls(e), Some(CallEvent::Done { action: CallAction::SendSound, .. }))).await;

    s.conn.dispatch("VOICE_CHANNEL_EFFECT_SEND", 31, d("voice_channel_effect_send")).await;
    s.client.until("effet", |e| matches!(calls(e), Some(CallEvent::Effect(_)))).await;
    let now = s.client.store.now();
    assert_eq!(s.client.store.calls.recent_effects(VOCAL, now).count(), 1);
    let plays = s.client.store.take_sound_plays();
    assert_eq!(plays.iter().map(|p| (p.channel_id, p.sound_id)).collect::<Vec<_>>(), vec![(VOCAL, 7000000000000000003)]);
}
