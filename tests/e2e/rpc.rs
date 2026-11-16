//! Rich Presence locale (lot R) de bout en bout : un faux jeu se branche
//! sur un tube local (préfixe propre au test, jamais `discord-ipc-`),
//! envoie SET_ACTIVITY, et l'activité part dans l'op 3 du faux serveur ;
//! elle en est retirée quand le jeu se débranche.

use crate::support::*;
use ferrocord::rpc::{encode, OP_FRAME, OP_HANDSHAKE};
use ferrocord::state::activity::ActivityEvent;
use ferrocord::state::rpc::RpcEvent;
use ferrocord::state::Event;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::windows::named_pipe::{ClientOptions, NamedPipeClient};

async fn read(c: &mut NamedPipeClient) -> Value {
    let mut head = [0u8; 8];
    c.read_exact(&mut head).await.unwrap_or_else(|e| panic!("{e}"));
    let mut body = vec![0u8; u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize];
    c.read_exact(&mut body).await.unwrap_or_else(|e| panic!("{e}"));
    serde_json::from_slice(&body).unwrap_or(Value::Null)
}

fn rpc(e: &Event, f: impl Fn(&RpcEvent) -> bool) -> bool {
    matches!(e, Event::Activity(ActivityEvent::Rpc(r)) if f(r))
}

#[tokio::test]
async fn fake_game_on_a_local_pipe_reaches_op3() {
    // READY utilisateur avec « Afficher l'activité en cours » et le statut inactif.
    let mut s = connected_with(USER_TOKEN, "ready_user_classic", |p| {
        p["d"]["user_settings_proto"] = fixture("rest_settings_proto")["settings"].clone();
    })
    .await;
    let prefix = format!("ferrocord-test-e2e-{}-", std::process::id());
    s.client.store.activity.rpc.pipe_prefix = Some(prefix.clone());
    s.client.store.activity.rpc.enabled = true;
    s.client.flush();
    s.client.until("tube ouvert", |e| rpc(e, |r| *r == RpcEvent::Listening(Some(0)))).await;

    let mut game = ClientOptions::new().open(format!(r"\\.\pipe\{prefix}0")).unwrap_or_else(|e| panic!("{e}"));
    game.write_all(&encode(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "6000000000000000040" }).to_string())).await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read(&mut game).await["evt"], "READY");
    let set = fixture("rpc/set_activity");
    game.write_all(&encode(OP_FRAME, &set.to_string())).await.unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(read(&mut game).await["cmd"], "SET_ACTIVITY");
    s.client.until("activité", |e| rpc(e, |r| matches!(r, RpcEvent::Activity { activity: Some(_), .. }))).await;

    // L'écart entre deux présences est tiré entre 4 et 7 s : on laisse passer
    // 3 s avant l'attente, elle-même bornée à 5 s.
    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    let p = s.conn.recv_op(3).await;
    let rich = p["d"]["activities"].as_array().and_then(|a| a.iter().find(|a| a["application_id"] == "6000000000000000040").cloned());
    let rich = rich.unwrap_or_else(|| panic!("activité Rich Presence dans l'op 3 : {p}"));
    assert_eq!(rich["details"], "Donjon des Brumes");
    assert_eq!(rich["timestamps"], json!({ "start": 1_700_000_000_000_i64, "end": 1_700_000_900_000_i64 }));
    assert_eq!(rich["buttons"], json!(["Site du jeu"]));
    assert_eq!(rich["metadata"], json!({ "button_urls": ["https://exemple.invalid/jeu"] }));
    assert!(rich.get("secrets").is_none(), "secrets jamais transmis");
    assert_eq!(p["d"]["status"], "idle", "statut synchronisé gardé");

    // Le jeu se débranche : son activité est retirée (après l'écart de 4 s de l'op 3).
    drop(game);
    s.client.until("départ", |e| rpc(e, |r| matches!(r, RpcEvent::Disconnected { .. }))).await;
    let p = s.conn.recv_op(3).await;
    let left = p["d"]["activities"].as_array().is_some_and(|a| a.iter().all(|a| a.get("application_id").is_none()));
    assert!(left, "activité retirée : {p}");
}
