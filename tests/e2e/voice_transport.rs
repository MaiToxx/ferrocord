//! Connexion vocale (lot V1) face au faux serveur vocal : poignée de main,
//! audio chiffré dans les deux sens et les deux modes, reprise, fermetures,
//! relais des opcodes DAVE.

use crate::support::voice::*;
use crate::support::within;
use ferrocord::net::voice::{
    self,
    e2ee::{FrameCrypto, Outgoing, Passthrough},
    rtp, CloseReason, VoiceCtl, VoiceEvent,
};
use serde_json::{json, Value};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::mpsc;

const AES: &str = "aead_aes256_gcm_rtpsize";
const XCHACHA: &str = "aead_xchacha20_poly1305_rtpsize";
const OTHER: u64 = 41_771_983_423_143_937;
const OTHER_SSRC: u32 = 777;

/// Prochain événement qui n'est pas un ping.
async fn next(rx: &mut mpsc::Receiver<VoiceEvent>) -> VoiceEvent {
    loop {
        match within("événement vocal", rx.recv()).await {
            Some(VoiceEvent::Ping(_)) => {}
            Some(e) => return e,
            None => panic!("connexion vocale arrêtée sans Closed"),
        }
    }
}

fn start(server: &FakeVoice, crypto: Box<dyn FrameCrypto>) -> (mpsc::Sender<VoiceCtl>, mpsc::Receiver<VoiceEvent>) {
    voice::spawn_with(&tokio::runtime::Handle::current(), server.params(), crypto, config())
}

async fn ctl(tx: &mpsc::Sender<VoiceCtl>, c: VoiceCtl) {
    tx.send(c).await.unwrap_or_else(|_| panic!("connexion arrêtée"));
}

/// Poignée de main, puis l'autre participant se présente (op 11 et 5).
async fn joined(server: &FakeVoice, c: &mut VoiceConn, rx: &mut mpsc::Receiver<VoiceEvent>, modes: &[&str]) -> Handshake {
    let h = server.handshake(c, modes, 0).await;
    assert_eq!(next(rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });
    c.send(json!({"op": 11, "seq": 1, "d": {"user_ids": [OTHER.to_string()]}})).await;
    assert_eq!(next(rx).await, VoiceEvent::UserJoined(OTHER));
    c.send(json!({"op": 5, "seq": 2, "d": {"user_id": OTHER.to_string(), "ssrc": OTHER_SSRC, "speaking": 1}})).await;
    assert_eq!(next(rx).await, VoiceEvent::Speaking { user_id: OTHER, speaking: true });
    h
}

/// Envoi et réception chiffrés dans `mode`.
async fn audio_both_ways(modes: &[&str], expected: &str) {
    let mut server = FakeVoice::start().await;
    let (tx, mut rx) = start(&server, Box::new(Passthrough));
    let mut c = server.accept().await;
    assert_eq!(c.path, "/?v=8");
    let h = joined(&server, &mut c, &mut rx, modes).await;
    assert_eq!(h.select["protocol"], "udp");
    assert_eq!(h.select["data"]["address"], "203.0.113.9");
    assert_eq!(h.select["data"]["mode"], expected);

    // Identify : tout ce que demande la documentation, DAVE absent (0).
    let d = &h.identify;
    assert_eq!(d["server_id"], GUILD.to_string());
    assert_eq!(d["user_id"], ME.to_string());
    assert_eq!(d["session_id"], SESSION);
    assert_eq!(d["token"], VOICE_TOKEN);
    assert_eq!(d["max_dave_protocol_version"], 0);

    // Envoi : Speaking d'abord, puis séquence +1 et horodatage +960.
    ctl(&tx, VoiceCtl::Opus(b"trame-1".to_vec())).await;
    ctl(&tx, VoiceCtl::Opus(b"trame-2".to_vec())).await;
    let s = c.recv_op(5).await;
    assert_eq!(s["d"], json!({"speaking": 1, "delay": 0, "ssrc": MY_SSRC}));
    assert_eq!(server.recv_audio(&h.cipher).await, (0, 0, MY_SSRC, b"trame-1".to_vec()));
    assert_eq!(server.recv_audio(&h.cipher).await, (1, 960, MY_SSRC, b"trame-2".to_vec()));

    // Fin de parole : cinq trames de silence, puis Speaking 0.
    ctl(&tx, VoiceCtl::Speaking(false)).await;
    for i in 0..5u16 {
        let (seq, ts, _, opus) = server.recv_audio(&h.cipher).await;
        assert_eq!((seq, ts, opus.as_slice()), (2 + i, 960 * u32::from(2 + i), &rtp::SILENCE[..]));
    }
    assert_eq!(c.recv_op(5).await["d"]["speaking"], 0);

    // Réception : le RTCP est ignoré, l'extension d'en-tête retirée.
    server.send_rtcp(&h).await;
    server.send_audio(&h, 999, 1, 1, 0, b"ssrc inconnu").await;
    server.send_audio(&h, OTHER_SSRC, 10, 9600, 1, b"bonjour").await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Audio { user_id: OTHER, seq: 10, timestamp: 9600, opus: b"bonjour".to_vec() });

    // Départ de l'autre : son SSRC n'est plus reconnu.
    c.send(json!({"op": 13, "seq": 3, "d": {"user_id": OTHER.to_string()}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::UserLeft(OTHER));
    server.send_audio(&h, OTHER_SSRC, 11, 10560, 2, b"parti").await;

    // Ping : nouveau Hello court, heartbeat avec seq_ack, accusé.
    c.hello(20.0).await;
    let hb = c.wait_heartbeat().await;
    assert_eq!(hb["seq_ack"], 3);
    match within("ping", rx.recv()).await {
        Some(VoiceEvent::Ping(ms)) => assert!(ms < 5000),
        e => panic!("ping attendu, reçu {e:?}"),
    }

    ctl(&tx, VoiceCtl::Disconnect).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Closed(CloseReason::Requested));
    c.expect_closed().await;
    server.expect_no_conn(Duration::from_millis(100)).await;
}

#[tokio::test]
async fn voice_handshake_and_audio_aes_gcm() {
    audio_both_ways(&[XCHACHA, AES, "xsalsa20_poly1305"], AES).await;
}

/// Après un silence, l'horodatage saute le temps écoulé (horloge du micro) :
/// la reprise ne paraît pas en retard à ceux qui écoutent.
#[tokio::test]
async fn voice_timestamp_follows_the_clock_across_silence() {
    let mut server = FakeVoice::start().await;
    let (tx, mut rx) = start(&server, Box::new(Passthrough));
    let mut c = server.accept().await;
    let h = joined(&server, &mut c, &mut rx, &[AES]).await;
    ctl(&tx, VoiceCtl::Opus(b"un".to_vec())).await;
    ctl(&tx, VoiceCtl::Opus(b"deux".to_vec())).await;
    assert_eq!(server.recv_audio(&h.cipher).await.1, 0);
    assert_eq!(server.recv_audio(&h.cipher).await.1, 960, "trames qui se suivent : +960");
    tokio::time::sleep(std::time::Duration::from_millis(400)).await;
    ctl(&tx, VoiceCtl::Opus(b"trois".to_vec())).await;
    let (seq, ts, _, opus) = server.recv_audio(&h.cipher).await;
    assert_eq!((seq, opus.as_slice()), (2, &b"trois"[..]));
    let frames = (ts - 960) / 960;
    assert!((ts - 960) % 960 == 0 && (18..=40).contains(&frames), "0,4 s de silence : {frames} trames sautées");
    ctl(&tx, VoiceCtl::Disconnect).await;
}

#[tokio::test]
async fn voice_audio_xchacha20() {
    audio_both_ways(&["xsalsa20_poly1305_lite", XCHACHA], XCHACHA).await;
}

#[tokio::test]
async fn voice_no_common_mode_is_unsupported() {
    let mut server = FakeVoice::start().await;
    let (_tx, mut rx) = start(&server, Box::new(Passthrough));
    let mut c = server.accept().await;
    c.hello(30_000.0).await;
    c.recv_op(0).await;
    c.send(json!({"op": 2, "d": {"ssrc": 1, "ip": "127.0.0.1", "port": server.udp_port(), "modes": ["xsalsa20_poly1305"]}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Closed(CloseReason::Unsupported));
}

#[tokio::test]
async fn voice_resumes_after_cut() {
    let mut server = FakeVoice::start().await;
    let (tx, mut rx) = start(&server, Box::new(Passthrough));
    let mut c = server.accept().await;
    let h = joined(&server, &mut c, &mut rx, &[AES]).await;
    ctl(&tx, VoiceCtl::Opus(b"avant".to_vec())).await;
    c.recv_op(5).await;
    assert_eq!(server.recv_audio(&h.cipher).await.0, 0);

    // Coupure TCP sans fermeture : reprise (op 7) avec le dernier seq reçu.
    c.drop_abruptly();
    let mut c = server.accept().await;
    let r = c.recv_op(7).await["d"].take();
    assert_eq!(r, json!({"server_id": GUILD.to_string(), "session_id": SESSION, "token": VOICE_TOKEN, "seq_ack": 2}));
    c.hello(30_000.0).await;
    c.send(json!({"op": 9, "d": null})).await;

    // Même UDP, même clé, séquence continue ; l'autre est toujours reconnu.
    ctl(&tx, VoiceCtl::Opus(b"apres".to_vec())).await;
    assert_eq!(server.recv_audio(&h.cipher).await, (1, 960, MY_SSRC, b"apres".to_vec()));
    server.send_audio(&h, OTHER_SSRC, 5, 4800, 7, b"toujours la").await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Audio { user_id: OTHER, seq: 5, timestamp: 4800, opus: b"toujours la".to_vec() });

    // Serveur vocal tombé (4015) : reprise aussi.
    c.close(4015).await;
    let mut c = server.accept().await;
    c.recv_op(7).await;
    drop(tx);
    assert_eq!(next(&mut rx).await, VoiceEvent::Closed(CloseReason::Requested));
}

#[tokio::test]
async fn voice_final_close_codes() {
    for (code, reason) in [
        (4014, CloseReason::Disconnected(4014)),
        (4022, CloseReason::Disconnected(4022)),
        (4006, CloseReason::SessionExpired(4006)),
        (4004, CloseReason::Refused(4004)),
        (4017, CloseReason::Refused(4017)),
    ] {
        let mut server = FakeVoice::start().await;
        let (_tx, mut rx) = start(&server, Box::new(Passthrough));
        let mut c = server.accept().await;
        server.handshake(&mut c, &[AES], 0).await;
        assert_eq!(next(&mut rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });
        c.close(code).await;
        assert_eq!(next(&mut rx).await, VoiceEvent::Closed(reason), "code {code}");
        server.expect_no_conn(Duration::from_millis(100)).await;
    }
}

#[tokio::test]
async fn voice_unreachable_after_failed_resumes() {
    let mut server = FakeVoice::start().await;
    let (_tx, mut rx) = start(&server, Box::new(Passthrough));
    let mut c = server.accept().await;
    server.handshake(&mut c, &[AES], 0).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });
    c.drop_abruptly();
    // Chaque reprise est coupée avant d'aboutir : trois essais (config), puis abandon.
    for _ in 0..3 {
        let mut c = server.accept().await;
        c.recv_op(7).await;
        c.drop_abruptly();
    }
    assert_eq!(next(&mut rx).await, VoiceEvent::Closed(CloseReason::Unreachable));
}

#[tokio::test]
async fn voice_rejects_foreign_endpoint() {
    let server = FakeVoice::start().await;
    let mut p = server.params();
    p.endpoint = "evil.tld:443".into();
    let (_tx, mut rx) = voice::spawn_with(&tokio::runtime::Handle::current(), p, Box::new(Passthrough), config());
    assert_eq!(next(&mut rx).await, VoiceEvent::Closed(CloseReason::BadEndpoint));
}

/// Ce que voit le faux chiffrement de bout en bout.
#[derive(Default)]
struct Seen {
    ops: Vec<(u8, Option<Value>, Option<Vec<u8>>)>,
    users: Vec<Vec<u64>>,
}

struct FakeDave(Arc<Mutex<Seen>>);

impl FrameCrypto for FakeDave {
    fn max_protocol_version(&self) -> u16 {
        1
    }

    fn on_op(&mut self, op: u8, json: Option<&Value>, binary: Option<&[u8]>, out: &mut Vec<Outgoing>) {
        if let Ok(mut s) = self.0.lock() {
            s.ops.push((op, json.cloned(), binary.map(<[u8]>::to_vec)));
        }
        match op {
            24 => out.push(Outgoing::Json(json!({"op": 23, "d": {"transition_id": 7}}))),
            25 => out.push(Outgoing::Binary(26, b"paquet-de-cles".to_vec())),
            _ => {}
        }
    }

    fn on_users(&mut self, connected: &[u64]) {
        if let Ok(mut s) = self.0.lock() {
            s.users.push(connected.to_vec());
        }
    }

    fn encrypt(&mut self, opus: &[u8]) -> Option<Vec<u8>> {
        let mut v = vec![0xDA];
        v.extend_from_slice(opus);
        Some(v)
    }

    fn decrypt(&mut self, _user_id: u64, frame: &[u8]) -> Option<Vec<u8>> {
        frame.strip_prefix(&[0xDA]).map(<[u8]>::to_vec)
    }

    fn encrypt_video(&mut self, _codec: ferrocord::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        Some(frame.to_vec())
    }

    fn decrypt_video(&mut self, _user_id: u64, _codec: ferrocord::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        Some(frame.to_vec())
    }

    fn verification_code(&self, _user_id: u64) -> Option<String> {
        None
    }
}

#[tokio::test]
async fn voice_dave_ops_are_relayed() {
    let mut server = FakeVoice::start().await;
    let seen = Arc::new(Mutex::new(Seen::default()));
    let (tx, mut rx) = start(&server, Box::new(FakeDave(seen.clone())));
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 1).await;
    assert_eq!(h.identify["max_dave_protocol_version"], 1);
    assert_eq!(next(&mut rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });

    c.send(json!({"op": 11, "seq": 4, "d": {"user_ids": [OTHER.to_string(), ME.to_string()]}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::UserJoined(OTHER));
    // JSON (op 24) puis binaire (séquence 9, op 25) : réponses relayées.
    c.send(json!({"op": 24, "seq": 5, "d": {"transition_id": 7, "epoch": 1}})).await;
    assert_eq!(c.recv_op(23).await["d"]["transition_id"], 7);
    let mut bin = vec![0, 9, 25];
    bin.extend_from_slice(b"expediteur");
    c.send_binary(bin).await;
    let mut want = vec![26];
    want.extend_from_slice(b"paquet-de-cles");
    assert_eq!(c.recv().await, Msg::Binary(want));
    c.hello(20.0).await;
    assert_eq!(c.wait_heartbeat().await["seq_ack"], 9);

    // Trames chiffrées de bout en bout avant le transport, et l'inverse.
    ctl(&tx, VoiceCtl::Opus(b"e2e".to_vec())).await;
    c.recv_op(5).await;
    assert_eq!(server.recv_audio(&h.cipher).await.3, b"\xDAe2e".to_vec());
    c.send(json!({"op": 5, "d": {"user_id": OTHER.to_string(), "ssrc": OTHER_SSRC, "speaking": 1}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Speaking { user_id: OTHER, speaking: true });
    server.send_audio(&h, OTHER_SSRC, 1, 960, 0, b"\xDAsalut").await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Audio { user_id: OTHER, seq: 1, timestamp: 960, opus: b"salut".to_vec() });
    // La trame de silence passe sans déchiffrement de bout en bout.
    server.send_audio(&h, OTHER_SSRC, 2, 1920, 1, &rtp::SILENCE).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Audio { user_id: OTHER, seq: 2, timestamp: 1920, opus: rtp::SILENCE.to_vec() });

    let s = seen.lock().map(|s| (s.ops.clone(), s.users.clone())).unwrap_or_default();
    let ops: Vec<u8> = s.0.iter().map(|o| o.0).collect();
    assert_eq!(ops, [4, 24, 25]);
    assert_eq!(s.0[0].1.as_ref().map(|d| d["dave_protocol_version"].clone()), Some(json!(1)));
    assert_eq!(s.0[2].2.as_deref(), Some(&b"expediteur"[..]));
    // Moi exclu de la liste des participants.
    assert_eq!(s.1, vec![vec![OTHER]]);
}
