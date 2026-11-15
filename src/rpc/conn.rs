//! Une application branchée : lecture des trames dans sa propre tâche (la
//! lecture d'une trame ne doit pas être coupée en deux par un minuteur),
//! réponses, et remontée des activités au magasin, au plus une par
//! [`ACTIVITY_GAP`] (la dernière reçue gagne).

use super::proto::{read_frame, Frame, Proto};
use super::{ProcName, Sink, ACTIVITY_GAP, HANDSHAKE_WAIT};
use crate::state::rpc::{RpcActivity, RpcEvent};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt};
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::Instant;

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Sert une connexion jusqu'à sa fin ; le magasin apprend son départ.
pub async fn session<S>(stream: S, conn: u64, sink: Sink, proc_name: ProcName)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    session_with(stream, conn, sink, proc_name, HANDSHAKE_WAIT, ACTIVITY_GAP).await;
}

async fn session_with<S>(stream: S, conn: u64, sink: Sink, proc_name: ProcName, handshake_wait: Duration, gap: Duration)
where
    S: AsyncRead + AsyncWrite + Send + 'static,
{
    let (mut r, mut w) = tokio::io::split(stream);
    let (tx, mut rx) = mpsc::channel::<Frame>(4);
    let reader = tokio::spawn(async move {
        while let Ok(f) = read_frame(&mut r).await {
            if tx.send(f).await.is_err() {
                break;
            }
        }
    });
    let mut proto = Proto::default();
    let handshake_by = Instant::now() + handshake_wait;
    let mut pending: Option<(Option<u32>, Option<RpcActivity>)> = None;
    let mut next_at = Instant::now();
    // Programme déjà lu pour ce pid (lu une fois : il ne change pas).
    let mut known: Option<(u32, Option<String>)> = None;
    loop {
        let flush_at = pending.is_some().then_some(next_at);
        tokio::select! {
            f = rx.recv() => {
                let Some(f) = f else { break };
                let out = proto.handle(&f);
                let mut failed = false;
                for reply in &out.replies {
                    failed |= w.write_all(reply).await.is_err();
                }
                failed |= w.flush().await.is_err();
                if let Some(app_id) = out.ready {
                    sink(RpcEvent::Connected { conn, app_id, since_ms: now_ms() });
                }
                if out.activity.is_some() {
                    pending = out.activity;
                }
                if out.close || failed {
                    break;
                }
            }
            _ = tokio::time::sleep_until(handshake_by), if !proto.ready() => break,
            _ = tokio::time::sleep_until(flush_at.unwrap_or(next_at)), if flush_at.is_some() => {
                let Some((pid, activity)) = pending.take() else { continue };
                let process = match (pid, &known) {
                    (Some(p), Some((k, name))) if *k == p => name.clone(),
                    (Some(p), _) => {
                        let name = tokio::task::spawn_blocking(move || proc_name(p)).await.ok().flatten();
                        known = Some((p, name.clone()));
                        name
                    }
                    (None, _) => None,
                };
                sink(RpcEvent::Activity { conn, process, activity: activity.map(Box::new) });
                next_at = Instant::now() + gap;
            }
        }
    }
    reader.abort();
    let _ = w.shutdown().await;
    if proto.ready() {
        sink(RpcEvent::Disconnected { conn });
    }
}

#[cfg(test)]
mod tests {
    use super::super::proto::{encode, OP_FRAME, OP_HANDSHAKE, OP_PING, OP_PONG};
    use super::*;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};
    use std::time::Duration;
    use tokio::io::{AsyncReadExt, DuplexStream};

    fn recorder() -> (Sink, Arc<Mutex<Vec<RpcEvent>>>) {
        let log: Arc<Mutex<Vec<RpcEvent>>> = Arc::default();
        let l = log.clone();
        (Arc::new(move |e| l.lock().unwrap_or_else(|p| p.into_inner()).push(e)), log)
    }

    fn fake_proc(pid: u32) -> Option<String> {
        (pid == 4242).then(|| "Jeu Fictif".to_string())
    }

    async fn recv(c: &mut DuplexStream) -> (u32, Value) {
        let mut head = [0u8; 8];
        c.read_exact(&mut head).await.unwrap_or_else(|e| panic!("{e}"));
        let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
        let mut body = vec![0u8; u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize];
        c.read_exact(&mut body).await.unwrap_or_else(|e| panic!("{e}"));
        (op, serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    async fn send(c: &mut DuplexStream, op: u32, v: Value) {
        c.write_all(&encode(op, &v.to_string())).await.unwrap_or_else(|e| panic!("{e}"));
    }

    fn activity(details: &str) -> Value {
        json!({ "cmd": "SET_ACTIVITY", "args": { "pid": 4242, "activity": { "details": details } }, "nonce": details })
    }

    const GAP: Duration = Duration::from_millis(150);

    #[tokio::test]
    async fn activities_are_coalesced_and_cleared_on_leave() {
        let (sink, log) = recorder();
        let (mut client, server) = tokio::io::duplex(1 << 16);
        let task = tokio::spawn(session_with(server, 3, sink, fake_proc, HANDSHAKE_WAIT, GAP));
        send(&mut client, OP_HANDSHAKE, json!({ "v": 1, "client_id": "77" })).await;
        assert_eq!(recv(&mut client).await.1["evt"], "READY");
        send(&mut client, OP_PING, json!({ "p": 1 })).await;
        assert_eq!(recv(&mut client).await.0, OP_PONG);
        for d in ["un", "deux", "trois"] {
            send(&mut client, OP_FRAME, activity(d)).await;
            assert_eq!(recv(&mut client).await.1["nonce"], d, "chaque commande a sa réponse");
        }
        // Une première tout de suite, puis la dernière à la fin de l'écart.
        tokio::time::sleep(GAP * 4).await;
        drop(client);
        let _ = task.await;
        let log = log.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let details: Vec<Option<String>> = log
            .iter()
            .filter_map(|e| match e {
                RpcEvent::Activity { activity, process, .. } => {
                    assert_eq!(process.as_deref(), Some("Jeu Fictif"));
                    Some(activity.as_ref().and_then(|a| a.details.clone()))
                }
                _ => None,
            })
            .collect();
        assert!(matches!(log.first(), Some(RpcEvent::Connected { conn: 3, app_id: 77, .. })));
        assert!(details.len() <= 2 && details.last() == Some(&Some("trois".into())), "{details:?}");
        assert_eq!(log.last(), Some(&RpcEvent::Disconnected { conn: 3 }));
    }

    #[tokio::test]
    async fn silent_client_is_dropped_without_events() {
        let (sink, log) = recorder();
        let (_client, server) = tokio::io::duplex(1024);
        let task = tokio::spawn(session_with(server, 1, sink, fake_proc, GAP, GAP));
        let ended = tokio::time::timeout(Duration::from_secs(5), task).await;
        assert!(ended.is_ok(), "poignée de main attendue, pas indéfiniment");
        assert!(log.lock().unwrap_or_else(|p| p.into_inner()).is_empty());
    }
}
