//! Connexion par QR code (lot C2) de bout en bout : le vrai
//! `net::remote_auth` face à une fausse passerelle d'authentification à
//! distance et une fausse API, sur 127.0.0.1. Rien ne quitte la machine.
//!
//! Forme du protocole : docs.discord.food, « Remote Authentication ›
//! Desktop ». Le faux serveur chiffre avec la clé publique reçue, comme
//! Discord, et vérifie la preuve du nonce.

use aws_lc_rs::rsa::{OaepPublicEncryptingKey, PrivateDecryptingKey, KeySize, PublicEncryptingKey, OAEP_SHA256_MGF1SHA256};
use ferrocord::net::remote_auth::{start, RemoteAuthEndpoints};
use ferrocord::state::remote_auth::{RemoteEvent, RemoteProblem};
use ferrocord::state::usersettings::b64;
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::{
    handshake::server::{Request, Response},
    http::StatusCode,
    protocol::{frame::coding::CloseCode, CloseFrame},
    Message,
};

/// Jeton factice (évidemment faux) rendu par la fausse API.
const FAKE_TOKEN: &str = "faux.jeton.qr-DE-TEST-0123456789";
const FAKE_TICKET: &str = "ticket-factice-DE-TEST";
const FAKE_NONCE: &[u8] = b"nonce-factice-DE-TEST";
const USER_ID: u64 = 1_000_000_000_000_000_042;

#[derive(Clone, Copy, PartialEq)]
enum Script {
    /// Tout le déroulé, jusqu'au jeton.
    Full,
    /// Ticket, puis refus sur le téléphone.
    Cancel,
    /// Battements de cœur, puis fermeture 4003.
    Expire,
    /// Nonce chiffré avec une autre clé : indéchiffrable par le client.
    ForeignNonce,
    /// Le serveur attend un autre nonce que celui qu'il a chiffré : preuve refusée (4001).
    ProofRejected,
    /// Empreinte qui n'est pas celle de la clé du client.
    WrongFingerprint,
    /// L'échange du ticket exige un captcha.
    Captcha,
    /// Origine autre que `https://discord.com` : connexion refusée.
    OriginChecked,
}

#[derive(Default)]
struct Seen {
    heartbeats: AtomicUsize,
    proofs: AtomicUsize,
    exchanges: AtomicUsize,
    origin: Mutex<Option<String>>,
    fingerprint: Mutex<String>,
}

fn b64url(data: &[u8]) -> String {
    b64::encode(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

fn seal(key: &OaepPublicEncryptingKey, plain: &[u8]) -> String {
    let mut out = vec![0u8; key.ciphertext_size()];
    let sealed = key.encrypt(&OAEP_SHA256_MGF1SHA256, plain, &mut out, None).unwrap();
    b64::encode(sealed)
}

/// Lance la fausse passerelle et la fausse API ; rend les adresses.
async fn fake(script: Script, seen: Arc<Seen>) -> RemoteAuthEndpoints {
    let ws = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let http = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let ws_addr = ws.local_addr().unwrap();
    let http_addr = http.local_addr().unwrap();
    let client_key: Arc<Mutex<Option<OaepPublicEncryptingKey>>> = Arc::default();

    let (key, s) = (client_key.clone(), seen.clone());
    tokio::spawn(async move {
        let Ok((tcp, _)) = ws.accept().await else { return };
        let s2 = s.clone();
        // Forme du rappel imposée par tungstenite (réponse d'erreur HTTP).
        #[allow(clippy::result_large_err)]
        let cb = move |req: &Request, resp: Response| {
            let origin = req.headers().get("Origin").and_then(|v| v.to_str().ok()).map(str::to_string);
            *s2.origin.lock().unwrap() = origin.clone();
            if req.uri().query() != Some("v=2") || (script == Script::OriginChecked && origin.as_deref() != Some("https://discord.com")) {
                let mut r = tokio_tungstenite::tungstenite::handshake::server::ErrorResponse::new(None);
                *r.status_mut() = StatusCode::FORBIDDEN;
                return Err(r);
            }
            Ok(resp)
        };
        let Ok(mut ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await else { return };
        let send = |v: Value| Message::Text(v.to_string().into());
        let interval = if script == Script::Expire { 1000 } else { 41_250 };
        ws.send(send(json!({ "op": "hello", "heartbeat_interval": interval, "timeout_ms": 120_000 }))).await.unwrap();
        let close = |code: u16| Message::Close(Some(CloseFrame { code: CloseCode::from(code), reason: "".into() }));
        while let Some(Ok(msg)) = ws.next().await {
            let Message::Text(text) = msg else { continue };
            let v: Value = serde_json::from_str(text.as_str()).unwrap();
            match v["op"].as_str().unwrap() {
                "heartbeat" => {
                    s.heartbeats.fetch_add(1, Ordering::SeqCst);
                    ws.send(send(json!({ "op": "heartbeat_ack" }))).await.unwrap();
                    if script == Script::Expire && s.heartbeats.load(Ordering::SeqCst) >= 2 {
                        let _ = ws.send(close(4003)).await;
                        return;
                    }
                }
                "init" => {
                    let der = b64::decode(v["encoded_public_key"].as_str().unwrap()).unwrap();
                    let public = OaepPublicEncryptingKey::new(PublicEncryptingKey::from_der(&der).unwrap()).unwrap();
                    let fingerprint = b64url(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &der).as_ref());
                    *key.lock().unwrap() = Some(public);
                    let encrypted_nonce = if script == Script::ForeignNonce {
                        let other = PrivateDecryptingKey::generate(KeySize::Rsa2048).unwrap();
                        seal(&OaepPublicEncryptingKey::new(other.public_key()).unwrap(), FAKE_NONCE)
                    } else {
                        seal(key.lock().unwrap().as_ref().unwrap(), FAKE_NONCE)
                    };
                    ws.send(send(json!({ "op": "nonce_proof", "encrypted_nonce": encrypted_nonce }))).await.unwrap();
                    *s.fingerprint.lock().unwrap() = fingerprint;
                }
                "nonce_proof" => {
                    s.proofs.fetch_add(1, Ordering::SeqCst);
                    let expected = if script == Script::ProofRejected { b64url(b"un autre nonce") } else { b64url(FAKE_NONCE) };
                    if v["nonce"].as_str() != Some(expected.as_str()) {
                        let _ = ws.send(close(4001)).await;
                        return;
                    }
                    let mine = s.fingerprint.lock().unwrap().clone();
                    let fingerprint = if script == Script::WrongFingerprint { b64url(&[7u8; 32]) } else { mine };
                    ws.send(send(json!({ "op": "pending_remote_init", "fingerprint": fingerprint }))).await.unwrap();
                    let payload = format!("{USER_ID}:0:05145cc5646fbcba277b6d5ea2030610:Testeur");
                    let sealed = seal(key.lock().unwrap().as_ref().unwrap(), payload.as_bytes());
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if script == Script::Expire {
                        continue;
                    }
                    ws.send(send(json!({ "op": "pending_ticket", "encrypted_user_payload": sealed }))).await.unwrap();
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    if script == Script::Cancel {
                        ws.send(send(json!({ "op": "cancel" }))).await.unwrap();
                        let _ = ws.send(close(1000)).await;
                        return;
                    }
                    ws.send(send(json!({ "op": "pending_login", "ticket": FAKE_TICKET }))).await.unwrap();
                    let _ = ws.send(close(1000)).await;
                    return;
                }
                other => panic!("opération inattendue : {other}"),
            }
        }
    });

    let (key, s) = (client_key, seen);
    tokio::spawn(async move {
        while let Ok((mut tcp, _)) = http.accept().await {
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // En-têtes, puis le corps annoncé.
            let (head, body) = loop {
                let n = tcp.read(&mut chunk).await.unwrap();
                buf.extend_from_slice(&chunk[..n]);
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_string();
                    let len = head.lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").map(|v| v.trim().parse::<usize>().unwrap())).unwrap_or(0);
                    while buf.len() < i + 4 + len {
                        let n = tcp.read(&mut chunk).await.unwrap();
                        buf.extend_from_slice(&chunk[..n]);
                    }
                    break (head, buf[i + 4..i + 4 + len].to_vec());
                }
            };
            s.exchanges.fetch_add(1, Ordering::SeqCst);
            assert!(head.starts_with("POST /users/@me/remote-auth/login "), "{head}");
            assert!(!head.to_ascii_lowercase().contains("authorization"), "aucun jeton n'est envoyé");
            assert!(head.contains("Ferrocord/"), "User-Agent de Ferrocord");
            let v: Value = serde_json::from_slice(&body).unwrap();
            assert_eq!(v["ticket"], FAKE_TICKET);
            let (status, reply) = if script == Script::Captcha {
                ("400 Bad Request", json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "site", "captcha_service": "hcaptcha" }))
            } else {
                ("200 OK", json!({ "encrypted_token": seal(key.lock().unwrap().as_ref().unwrap(), FAKE_TOKEN.as_bytes()) }))
            };
            let reply = reply.to_string();
            let out = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}", reply.len());
            tcp.write_all(out.as_bytes()).await.unwrap();
        }
    });

    RemoteAuthEndpoints {
        gateway: format!("ws://{ws_addr}"),
        api: format!("http://{http_addr}"),
        origin: "https://discord.com".into(),
        step_timeout: Duration::from_secs(5),
        log: Arc::new(|_: &str| {}),
        identity: Default::default(),
    }
}

struct Run {
    events: Vec<RemoteEvent>,
    log: Vec<String>,
    seen: Arc<Seen>,
}

/// Une tentative complète face au faux serveur : tous ses événements et son journal.
async fn run(script: Script) -> Run {
    let seen = Arc::new(Seen::default());
    let mut ep = fake(script, seen.clone()).await;
    let log = Arc::new(Mutex::new(Vec::new()));
    let l = log.clone();
    ep.log = Arc::new(move |line: &str| l.lock().unwrap().push(line.to_string()));
    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    let login = start(&tokio::runtime::Handle::current(), ep, Arc::new(move || {
        w.fetch_add(1, Ordering::SeqCst);
    }));
    let mut events = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !events.last().is_some_and(RemoteEvent::is_final) {
        assert!(std::time::Instant::now() < deadline, "pas de fin : {events:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
        events.extend(login.poll());
    }
    assert!(wakes.load(Ordering::SeqCst) >= events.len(), "chaque événement réveille l'interface");
    let log = log.lock().unwrap().clone();
    Run { events, log, seen }
}

/// Aucune trace des secrets dans le journal ni dans les événements affichables.
fn assert_no_secret(r: &Run) {
    let nonce = String::from_utf8_lossy(FAKE_NONCE).to_string();
    let shown: Vec<String> = r.log.iter().cloned().chain(r.events.iter().map(|e| format!("{e:?}"))).collect();
    for line in &shown {
        for secret in [FAKE_TOKEN, "qr-DE-TEST", FAKE_TICKET, nonce.as_str()] {
            assert!(!line.contains(secret), "secret dans « {line} »");
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn full_flow_gives_the_token_and_logs_no_secret() {
    let r = run(Script::Full).await;
    let [RemoteEvent::Qr { fingerprint, .. }, RemoteEvent::User(user), RemoteEvent::Token(token)] = &r.events[..] else {
        panic!("{:?}", r.events);
    };
    assert_eq!(fingerprint.len(), 43);
    assert_eq!((user.id, user.name.as_str()), (USER_ID, "Testeur"));
    assert_eq!(user.avatar.map(|h| h.to_string()).as_deref(), Some("05145cc5646fbcba277b6d5ea2030610"));
    assert_eq!(token.expose(), FAKE_TOKEN);
    assert_eq!(r.seen.proofs.load(Ordering::SeqCst), 1);
    assert_eq!(r.seen.exchanges.load(Ordering::SeqCst), 1);
    assert_eq!(r.seen.origin.lock().unwrap().as_deref(), Some("https://discord.com"));
    assert!(r.log.iter().any(|l| l.contains("QR prêt")), "{:?}", r.log);
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn refusal_on_the_phone_is_reported() {
    let r = run(Script::Cancel).await;
    assert!(matches!(&r.events[..], [RemoteEvent::Qr { .. }, RemoteEvent::User(_), RemoteEvent::Cancelled]), "{:?}", r.events);
    assert_eq!(r.seen.exchanges.load(Ordering::SeqCst), 0);
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn expiry_after_heartbeats_is_reported() {
    let r = run(Script::Expire).await;
    assert!(matches!(&r.events[..], [RemoteEvent::Qr { .. }, RemoteEvent::Expired]), "{:?}", r.events);
    assert!(r.seen.heartbeats.load(Ordering::SeqCst) >= 2);
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nonce_sealed_for_another_key_is_refused_before_any_qr() {
    let r = run(Script::ForeignNonce).await;
    assert!(matches!(&r.events[..], [RemoteEvent::Failed(RemoteProblem::Protocol)]), "{:?}", r.events);
    assert_eq!(r.seen.proofs.load(Ordering::SeqCst), 0, "aucune preuve envoyée");
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wrong_proof_is_rejected_by_the_server_and_no_qr_is_shown() {
    let r = run(Script::ProofRejected).await;
    assert!(matches!(&r.events[..], [RemoteEvent::Failed(RemoteProblem::Closed(4001))]), "{:?}", r.events);
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_fingerprint_is_never_shown() {
    let r = run(Script::WrongFingerprint).await;
    assert!(matches!(&r.events[..], [RemoteEvent::Failed(RemoteProblem::Protocol)]), "{:?}", r.events);
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn captcha_is_reported_never_solved() {
    let r = run(Script::Captcha).await;
    assert!(matches!(&r.events[..], [RemoteEvent::Qr { .. }, RemoteEvent::User(_), RemoteEvent::Failed(RemoteProblem::Captcha)]), "{:?}", r.events);
    assert_eq!(r.seen.exchanges.load(Ordering::SeqCst), 1, "un seul essai");
    assert_no_secret(&r);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_required_origin_is_sent() {
    let r = run(Script::OriginChecked).await;
    assert!(matches!(r.events.last(), Some(RemoteEvent::Token(_))), "{:?}", r.events);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dropping_the_attempt_closes_the_connection() {
    let seen = Arc::new(Seen::default());
    let ep = fake(Script::Expire, seen.clone()).await;
    let login = start(&tokio::runtime::Handle::current(), ep, Arc::new(|| {}));
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !login.poll().iter().any(|e| matches!(e, RemoteEvent::Qr { .. })) {
        assert!(std::time::Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    drop(login);
    // Plus aucun battement de cœur après l'abandon.
    let before = seen.heartbeats.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    assert_eq!(seen.heartbeats.load(Ordering::SeqCst), before);
}
