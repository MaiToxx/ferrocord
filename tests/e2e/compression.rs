//! Compression du transport (`zlib-stream`), repli sans compression,
//! paquets volumineux, rafales, espacement des Identify.

use crate::support::{ids::*, *};
use ferrocord::state::{Cause, Event, Status};
use serde_json::{json, Value};
use std::{
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

const COMPRESSED: &str = "/gw/?v=10&encoding=json&compress=zlib-stream";
const RESUME_PLAIN: &str = "/resume/?v=10&encoding=json";

fn message(id: u64, content: &str) -> Value {
    let mut p = fixture("message_create_simple");
    p["d"]["id"] = json!(id.to_string());
    p["d"]["content"] = json!(content);
    p["d"]
        .take()
}

#[tokio::test]
async fn session_is_compressed_by_default() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    assert_eq!(s.conn.path, COMPRESSED);
    assert!(s.conn.compressed());
    s.conn.dispatch("MESSAGE_CREATE", 2, message(5000000000000000400, "compressé")).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    // Le contexte partagé : un paquet presque identique au précédent coûte très peu.
    let before = s.conn.bytes_sent;
    s.conn.dispatch("MESSAGE_CREATE", 3, message(5000000000000000401, "compressé")).await;
    s.client.until("second MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    let cost = s.conn.bytes_sent - before;
    let plain = json!({ "op": 0, "t": "MESSAGE_CREATE", "s": 3, "d": message(5000000000000000401, "compressé") }).to_string().len();
    assert!(cost * 5 < plain, "{cost} octets compressés pour {plain} en clair");
}

#[tokio::test]
async fn packets_split_across_many_frames() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let mut client = Client::start(USER_TOKEN, endpoints(&gw, &http));
    let mut conn = gw.accept().await;
    // Hello, READY et la suite coupés chacun en 7 messages WebSocket.
    conn.fragments = 7;
    conn.hello_identify(QUIET).await;
    conn.send(ready_with_resume(&gw, "ready_user_classic")).await;
    client.until("READY", |e| matches!(e, Event::Ready { .. })).await;
    for (i, n) in [2usize, 3, 50].into_iter().enumerate() {
        conn.fragments = n;
        conn.dispatch("MESSAGE_CREATE", 2 + i as u64, message(5000000000000000500 + i as u64, &format!("morceaux {n}"))).await;
        let text = client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
        assert!(text.contains(&format!("morceaux {n}")), "{text}");
    }
    conn.request_heartbeat().await;
    assert_eq!(conn.recv_op(1).await["d"], 4);
}

#[tokio::test]
async fn corrupt_frame_reconnects_without_compression_and_resumes() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch("MESSAGE_CREATE", 2, message(5000000000000000600, "avant")).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    s.conn.send_corrupt(json!({ "op": 0, "t": "MESSAGE_CREATE", "s": 3, "d": message(5000000000000000601, "perdu") })).await;
    s.client.until("statut de repli", |e| matches!(e, Event::Status(Status::Disconnected { cause: Cause::CorruptStream, no_compression: true }))).await;
    s.conn.expect_closed().await;

    // Reprise par l'adresse de reprise, sans compression, au dernier numéro bien lu.
    let mut conn = s.gw.accept().await;
    assert_eq!(conn.path, RESUME_PLAIN);
    assert!(!conn.compressed());
    let d = conn.hello_resume(QUIET).await;
    assert_eq!((d["session_id"].as_str(), d["seq"].as_u64()), (Some("session-fictive-0001"), Some(2)));
    // Discord rejoue le paquet perdu.
    conn.dispatch("MESSAGE_CREATE", 3, message(5000000000000000601, "rejoué")).await;
    conn.dispatch("RESUMED", 4, json!({})).await;
    s.client.until("message rejoué", |e| matches!(e, Event::MessageCreate(m) if m.content == "rejoué")).await;
    s.client.until("Connecté", |e| matches!(e, Event::Status(Status::Connected))).await;

    // Les connexions suivantes restent sans compression.
    conn.reconnect().await;
    let mut again = s.gw.accept().await;
    assert_eq!(again.path, RESUME_PLAIN);
    again.hello_resume(QUIET).await;
}

#[tokio::test]
async fn server_that_stops_compressing_after_resume_is_understood() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.reconnect().await;
    let mut conn = s.gw.accept().await;
    // Compression demandée, mais le serveur répond en texte.
    assert!(conn.compressed());
    conn.stop_compressing();
    conn.hello_resume(QUIET).await;
    conn.dispatch("RESUMED", 2, json!({})).await;
    conn.dispatch("MESSAGE_CREATE", 3, message(5000000000000000700, "en clair")).await;
    s.client.until("message en clair", |e| matches!(e, Event::MessageCreate(m) if m.content == "en clair")).await;
    conn.request_heartbeat().await;
    assert_eq!(conn.recv_op(1).await["d"], 3);
}

#[tokio::test]
async fn text_in_the_middle_of_a_compressed_packet_is_a_broken_stream() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Début d'un paquet compressé, puis un paquet texte : flux incohérent.
    let mut z = flate2::Compress::new(flate2::Compression::default(), true);
    let mut half = deflate_packet(&mut z, br#"{"op":0,"t":"MESSAGE_CREATE","s":2,"d":{}}"#);
    half.truncate(half.len() - 4);
    s.conn.send_binary(half).await;
    s.conn.stop_compressing();
    s.conn.dispatch("MESSAGE_CREATE", 2, message(5000000000000000800, "texte")).await;
    s.client.until("statut de repli", |e| matches!(e, Event::Status(Status::Disconnected { cause: Cause::CorruptStream, no_compression: true }))).await;
    let conn = s.gw.accept().await;
    assert_eq!(conn.path, RESUME_PLAIN);
}

#[tokio::test]
async fn decompression_bomb_is_refused() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let ep = ferrocord::net::Endpoints { max_payload: 1 << 20, ..endpoints(&gw, &http) };
    let mut client = Client::start(USER_TOKEN, ep);
    // 16 Mo d'espaces une fois décompressés, quelques Ko sur le fil.
    let bomb = format!(r#"{{"op":0,"t":"PRESENCE_UPDATE","s":1,"d":{{"x":"{}"}}}}"#, " ".repeat(16 << 20));
    // Deux fois de suite : un paquet trop gros n'est pas une corruption, la
    // compression est gardée, et le client ne boucle pas (il réessaie après
    // une pause qui grandit).
    for _ in 0..2 {
        let mut conn = gw.accept().await;
        assert_eq!(conn.path, COMPRESSED);
        conn.hello_identify(QUIET).await;
        let before = conn.bytes_sent;
        conn.send_text(&bomb).await;
        assert!(conn.bytes_sent - before < 64 << 10);
        client.until("statut", |e| matches!(e, Event::Status(Status::PacketTooLarge))).await;
        conn.expect_closed().await;
    }
    let mut next = gw.accept().await;
    assert_eq!(next.path, COMPRESSED);
    next.hello_identify(QUIET).await;
}

#[tokio::test]
async fn big_ready_is_decoded_while_heartbeats_keep_beating() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let mut client = Client::start(USER_TOKEN, endpoints(&gw, &http));
    let mut conn = gw.accept().await;
    // Tout est compressé d'avance : le faux serveur doit accuser chaque
    // heartbeat à temps, sans être occupé à compresser.
    let ready = big_ready(&gw, 2500).to_string();
    assert!(ready.len() > 4 << 20, "READY de {} octets", ready.len());
    let hello = conn.prepare(&json!({ "op": 10, "d": { "heartbeat_interval": 1000 } }).to_string());
    let wire = conn.prepare(&ready);
    assert!(wire.len() * 4 < ready.len(), "{} octets compressés pour {}", wire.len(), ready.len());
    conn.send_binary(hello).await;
    conn.recv_op(2).await;
    conn.send_binary(wire).await;

    // Pendant le décodage, la boucle de la passerelle continue de battre :
    // les heartbeats arrivent (toutes les 1 s), accusés au fil de l'eau.
    // Borne large (2,5 intervalles) pour une machine très chargée : un
    // décodage fait sur la boucle la bloquerait bien plus longtemps en
    // profil de test. Le test unitaire `heavy_inputs_are_decoded_off_the_loop`
    // vérifie le déport sans mesurer de durée.
    let start = Instant::now();
    let mut last = Instant::now();
    let mut beats = 0;
    while start.elapsed() < Duration::from_millis(2600) {
        let p = conn.recv().await;
        assert_eq!(p["op"], 1, "{p}");
        let gap = last.elapsed();
        assert!(beats == 0 || gap < Duration::from_millis(2500), "heartbeat en retard : {gap:?}");
        last = Instant::now();
        beats += 1;
        conn.heartbeat_ack().await;
    }
    assert!(beats >= 2, "{beats}");
    client.until("READY", |e| matches!(e, Event::Ready { .. })).await;
    assert_eq!(client.store.guilds.len(), 2500);
}

#[tokio::test]
async fn burst_of_5000_small_events_loses_nothing() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    const N: u64 = 5000;
    for i in 0..N {
        let mut p = fixture("message_create_simple");
        p["s"] = json!(2 + i);
        p["d"]["id"] = json!((5000000000000010000 + i).to_string());
        p["d"]["channel_id"] = json!(GENERAL.to_string());
        s.conn.send(p).await;
    }
    s.conn.request_heartbeat().await;
    let seen = AtomicUsize::new(0);
    let last = 5000000000000010000 + N - 1;
    s.client
        .until("dernier message", |e| match e {
            Event::MessageCreate(m) => {
                seen.fetch_add(1, Ordering::SeqCst);
                m.id == last
            }
            _ => false,
        })
        .await;
    assert_eq!(seen.load(Ordering::SeqCst), N as usize, "aucun événement perdu ni en double");
    // Le magasin garde un nombre borné de messages par salon.
    let cached = s.client.store.chans.get(&GENERAL).map_or(0, |c| c.msgs.len());
    assert!(cached <= ferrocord::state::store::MAX_CACHED_MESSAGES, "{cached}");
    // La demande de heartbeat, arrivée derrière la rafale, porte le dernier numéro.
    assert_eq!(s.conn.recv_op(1).await["d"], 1 + N);
}

#[tokio::test]
async fn identifies_are_spaced() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let gap = Duration::from_millis(400);
    let ep = ferrocord::net::Endpoints { identify_interval: gap, reconnect_base: Duration::from_millis(1), ..endpoints(&gw, &http) };
    let _client = Client::start(USER_TOKEN, ep);
    // (heure juste avant l'envoi de Hello, heure de réception de l'Identify)
    let mut times = Vec::new();
    for _ in 0..3 {
        let mut conn = gw.accept().await;
        let hello_at = Instant::now();
        conn.hello_identify(QUIET).await;
        times.push((hello_at, Instant::now()));
        // Session invalidée, non reprenable : nouvel Identify.
        conn.invalid_session(false).await;
        conn.expect_closed().await;
    }
    for w in times.windows(2) {
        // L'Identify n est parti après le Hello n (prise avant son envoi), et
        // l'Identify n + 1 au moins `gap` plus tard : borne sans marge, quelle
        // que soit la charge de la machine.
        let spacing = w[1].1 - w[0].0;
        assert!(spacing >= gap, "Identify trop rapprochés : {spacing:?}");
    }
}

#[tokio::test]
async fn daily_identify_cap_is_announced() {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let ep = ferrocord::net::Endpoints { identify_per_day: 2, reconnect_base: Duration::from_millis(1), ..endpoints(&gw, &http) };
    let mut client = Client::start(USER_TOKEN, ep);
    for _ in 0..2 {
        let mut conn = gw.accept().await;
        conn.hello_identify(QUIET).await;
        conn.invalid_session(false).await;
        conn.expect_closed().await;
    }
    // Troisième session du jour : le client ne se connecte pas et annonce
    // l'heure du nouvel essai.
    let before = chrono::Utc::now().timestamp_millis();
    let text = client.until("statut du plafond", |e| matches!(e, Event::Status(Status::IdentifyCap { .. }))).await;
    // Heure murale du nouvel essai : 24 h après la plus ancienne des deux
    // sessions, donc dans le futur, à un peu moins d'un jour d'ici.
    let retry_ms: i64 = text
        .split("retry_ms: ")
        .nth(1)
        .and_then(|s| s.split(|c: char| !c.is_ascii_digit()).next())
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| panic!("retry_ms illisible : {text}"));
    let day = 24 * 3600 * 1000;
    assert!(retry_ms > before + day - 60_000 && retry_ms <= chrono::Utc::now().timestamp_millis() + day, "nouvel essai à {retry_ms}, maintenant {before} : {text}");
    assert!(gw.try_accept().is_none(), "aucune connexion au-delà du plafond");
}
