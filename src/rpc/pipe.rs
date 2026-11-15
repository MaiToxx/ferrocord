//! Tubes nommés de Windows (`\\.\pipe\discord-ipc-{n}`), par
//! `tokio::net::windows::named_pipe` : aucun `unsafe` de notre côté.
//!
//! Le premier tube libre est pris avec `first_pipe_instance` : si un autre
//! programme (le client officiel, le plus souvent sur le 0) a déjà créé ce
//! nom, la création échoue et le numéro suivant est essayé ; personne ne
//! peut non plus se glisser dans un tube que nous tenons. Les clients
//! distants (réseau) sont refusés (valeur par défaut de tokio).

use super::{session, ProcName, Sink, PIPES};
use crate::state::rpc::{RpcEvent, MAX_CLIENTS};
use std::sync::Arc;
use std::time::Duration;
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use tokio::sync::{watch, Semaphore};
use tokio::task::JoinSet;

/// Attente avant de réessayer après une erreur du système sur le tube.
const RETRY: Duration = Duration::from_secs(5);

fn pipe_name(prefix: &str, n: u8) -> String {
    format!(r"\\.\pipe\{prefix}{n}")
}

/// Premier tube libre, de 0 à 9.
fn open_first(prefix: &str) -> Option<(u8, NamedPipeServer)> {
    (0..PIPES).find_map(|n| ServerOptions::new().first_pipe_instance(true).create(pipe_name(prefix, n)).ok().map(|s| (n, s)))
}

pub(super) async fn serve(mut rx: watch::Receiver<Option<String>>, sink: Sink, proc_name: ProcName) {
    let mut was_open = false;
    loop {
        let wanted = rx.borrow_and_update().clone();
        match wanted {
            Some(prefix) => {
                tokio::select! {
                    _ = listen(&prefix, sink.clone(), proc_name) => {}
                    r = rx.changed() => {
                        // `listen` abandonné : tube fermé, applications débranchées.
                        sink(RpcEvent::Listening(None));
                        if r.is_err() {
                            return;
                        }
                    }
                }
                was_open = true;
            }
            None => {
                if was_open {
                    sink(RpcEvent::Listening(None));
                    was_open = false;
                }
                if rx.changed().await.is_err() {
                    return;
                }
            }
        }
    }
}

/// Écoute jusqu'à être abandonné (ne se termine jamais d'elle-même).
async fn listen(prefix: &str, sink: Sink, proc_name: ProcName) {
    let (n, mut server) = loop {
        match open_first(prefix) {
            Some(found) => break found,
            None => {
                // Tous pris : on le dit, puis on réessaie de temps en temps.
                sink(RpcEvent::Listening(None));
                tokio::time::sleep(RETRY * 6).await;
            }
        }
    };
    sink(RpcEvent::Listening(Some(n)));
    let name = pipe_name(prefix, n);
    let slots = Arc::new(Semaphore::new(MAX_CLIENTS));
    // Abandonné avec `listen` : chaque connexion en cours est coupée.
    let mut conns = JoinSet::new();
    let mut next_conn = 0u64;
    loop {
        let Ok(slot) = slots.clone().acquire_owned().await else { return };
        if server.connect().await.is_err() {
            // Client parti avant d'être servi : instance neuve.
            match ServerOptions::new().create(&name) {
                Ok(s) => server = s,
                Err(_) => tokio::time::sleep(RETRY).await,
            }
            continue;
        }
        // L'instance suivante doit exister avant que celle-ci soit servie,
        // pour qu'un autre programme trouve le tube ouvert.
        let next = loop {
            match ServerOptions::new().create(&name) {
                Ok(s) => break s,
                Err(_) => tokio::time::sleep(RETRY).await,
            }
        };
        let stream = std::mem::replace(&mut server, next);
        next_conn += 1;
        let (sink, conn) = (sink.clone(), next_conn);
        conns.spawn(async move {
            session(stream, conn, sink, proc_name).await;
            drop(slot);
        });
        while conns.try_join_next().is_some() {}
    }
}

#[cfg(test)]
mod tests {
    use super::super::proto::{encode, OP_FRAME, OP_HANDSHAKE};
    use super::*;
    use serde_json::{json, Value};
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::windows::named_pipe::ClientOptions;

    /// Préfixe propre au test : jamais les vrais tubes `discord-ipc-`.
    fn prefix(tag: &str) -> String {
        format!("ferrocord-test-ipc-{}-{tag}-", std::process::id())
    }

    fn recorder() -> (Sink, Arc<Mutex<Vec<RpcEvent>>>) {
        let log: Arc<Mutex<Vec<RpcEvent>>> = Arc::default();
        let l = log.clone();
        (Arc::new(move |e| l.lock().unwrap_or_else(|p| p.into_inner()).push(e)), log)
    }

    async fn until(log: &Arc<Mutex<Vec<RpcEvent>>>, what: &str, f: impl Fn(&RpcEvent) -> bool) {
        for _ in 0..500 {
            if log.lock().unwrap_or_else(|p| p.into_inner()).iter().any(&f) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("attendu : {what} ; reçu : {:?}", log.lock().unwrap_or_else(|p| p.into_inner()));
    }

    async fn read(c: &mut tokio::net::windows::named_pipe::NamedPipeClient) -> (u32, Value) {
        let mut head = [0u8; 8];
        c.read_exact(&mut head).await.unwrap_or_else(|e| panic!("{e}"));
        let mut body = vec![0u8; u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize];
        c.read_exact(&mut body).await.unwrap_or_else(|e| panic!("{e}"));
        (u32::from_le_bytes([head[0], head[1], head[2], head[3]]), serde_json::from_slice(&body).unwrap_or(Value::Null))
    }

    #[tokio::test]
    async fn fake_client_on_a_local_pipe() {
        let p = prefix("client");
        // Le tube 0 est déjà pris (comme par le client officiel) : le serveur prend le 1.
        let _taken = ServerOptions::new().first_pipe_instance(true).create(pipe_name(&p, 0)).unwrap_or_else(|e| panic!("{e}"));
        let (sink, log) = recorder();
        let (tx, rx) = watch::channel(Some(p.clone()));
        let server = tokio::spawn(serve(rx, sink, |_| Some("Jeu Fictif".into())));
        until(&log, "tube 1", |e| *e == RpcEvent::Listening(Some(1))).await;

        let mut c = ClientOptions::new().open(pipe_name(&p, 1)).unwrap_or_else(|e| panic!("{e}"));
        c.write_all(&encode(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "6000000000000000040" }).to_string())).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&mut c).await.1["evt"], "READY");
        let set = json!({ "cmd": "SET_ACTIVITY", "args": { "pid": 1, "activity": { "details": "Menu" } }, "nonce": "a" });
        c.write_all(&encode(OP_FRAME, &set.to_string())).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&mut c).await.1["nonce"], "a");
        let auth = json!({ "cmd": "AUTHORIZE", "args": { "client_id": "1", "scopes": ["rpc"] }, "nonce": "b" });
        c.write_all(&encode(OP_FRAME, &auth.to_string())).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&mut c).await.1["evt"], "ERROR", "AUTHORIZE refusé");
        until(&log, "activité", |e| matches!(e, RpcEvent::Activity { activity: Some(a), process: Some(n), .. } if a.details.as_deref() == Some("Menu") && n == "Jeu Fictif")).await;

        // Une deuxième application en même temps, puis la première s'en va.
        let mut c2 = ClientOptions::new().open(pipe_name(&p, 1)).unwrap_or_else(|e| panic!("{e}"));
        c2.write_all(&encode(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "2" }).to_string())).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(read(&mut c2).await.1["evt"], "READY");
        drop(c);
        until(&log, "départ", |e| *e == RpcEvent::Disconnected { conn: 1 }).await;

        // Réglage coupé : tube fermé.
        let _ = tx.send(None);
        until(&log, "fermeture", |e| *e == RpcEvent::Listening(None)).await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(ClientOptions::new().open(pipe_name(&p, 1)).is_err(), "plus de tube");
        drop(tx);
        let _ = tokio::time::timeout(Duration::from_secs(5), server).await;
    }
}
