//! Faux serveur vocal : passerelle vocale (WebSocket en clair) et transport
//! UDP, tous deux sur 127.0.0.1. Le chiffrement du transport est celui de
//! `net::voice::crypt`, dont les primitives sont vérifiées sur les vecteurs
//! de référence (tests unitaires).

#![allow(dead_code)]

use super::within;
use ferrocord::net::voice::{
    crypt::{Cipher, Mode},
    rtp, video, ConnKind, Secret, VoiceConfig, VoiceHosts, VoiceParams,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream, UdpSocket},
    sync::mpsc,
};
use tokio_tungstenite::{
    tungstenite::{
        handshake::server::{Request as WsRequest, Response as WsResponse},
        protocol::{frame::coding::CloseCode, CloseFrame},
        Message as WsMsg,
    },
    WebSocketStream,
};

/// Jeton vocal factice.
pub const VOICE_TOKEN: &str = "faux-jeton-vocal-DE-TEST";
pub const SESSION: &str = "session-vocale-test";
pub const GUILD: u64 = 81_384_788_765_712_384;
pub const CHANNEL: u64 = 81_384_788_765_712_385;
pub const ME: u64 = 53_908_232_506_183_680;
/// SSRC que le faux serveur m'attribue.
pub const MY_SSRC: u32 = 4242;
pub const KEY: [u8; 32] = [0x5A; 32];

pub struct FakeVoice {
    port: u16,
    conns: mpsc::UnboundedReceiver<VoiceConn>,
    pub udp: UdpSocket,
}

/// Message reçu du client.
#[derive(Debug, PartialEq)]
pub enum Msg {
    Json(Value),
    Binary(Vec<u8>),
}

pub struct VoiceConn {
    ws: WebSocketStream<TcpStream>,
    pub path: String,
    /// `d` du dernier heartbeat reçu (accusé automatiquement).
    pub last_heartbeat: Option<Value>,
}

impl FakeVoice {
    // La signature du rappel de poignée de main est imposée par tungstenite.
    #[allow(clippy::result_large_err)]
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|e| panic!("bind : {e}"));
        let port = listener.local_addr().map(|a| a.port()).unwrap_or_else(|e| panic!("adresse : {e}"));
        let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap_or_else(|e| panic!("bind UDP : {e}"));
        let (tx, conns) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let path = Arc::new(Mutex::new(String::new()));
                    let p = path.clone();
                    let cb = move |req: &WsRequest, resp: WsResponse| {
                        if let Ok(mut p) = p.lock() {
                            *p = req.uri().to_string();
                        }
                        Ok(resp)
                    };
                    if let Ok(ws) = tokio_tungstenite::accept_hdr_async(tcp, cb).await {
                        let path = path.lock().map(|p| p.clone()).unwrap_or_default();
                        let _ = tx.send(VoiceConn { ws, path, last_heartbeat: None });
                    }
                });
            }
        });
        FakeVoice { port, conns, udp }
    }

    pub fn endpoint(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn udp_port(&self) -> u16 {
        self.udp.local_addr().map(|a| a.port()).unwrap_or(0)
    }

    pub fn params(&self) -> VoiceParams {
        VoiceParams {
            guild_id: Some(GUILD),
            channel_id: CHANNEL,
            user_id: ME,
            session_id: SESSION.into(),
            endpoint: self.endpoint(),
            token: Secret::new(VOICE_TOKEN.into()),
            kind: ConnKind::Voice,
        }
    }

    pub async fn accept(&mut self) -> VoiceConn {
        within("connexion à la passerelle vocale", self.conns.recv()).await.unwrap_or_else(|| panic!("serveur arrêté"))
    }

    /// Aucune nouvelle connexion pendant `d`.
    pub async fn expect_no_conn(&mut self, d: Duration) {
        if let Ok(Some(_)) = tokio::time::timeout(d, self.conns.recv()).await {
            panic!("le client n'aurait pas dû se reconnecter");
        }
    }

    /// Prochain datagramme reçu et son expéditeur.
    pub async fn recv_udp(&self) -> (Vec<u8>, SocketAddr) {
        let mut buf = vec![0u8; 2048];
        let (n, from) = within("datagramme du client", self.udp.recv_from(&mut buf)).await.unwrap_or_else(|e| panic!("UDP : {e}"));
        buf.truncate(n);
        (buf, from)
    }

    /// Prochain paquet audio du client : (séquence, horodatage, SSRC, trame claire).
    pub async fn recv_audio(&self, cipher: &Cipher) -> (u16, u32, u32, Vec<u8>) {
        let (pkt, _) = self.recv_udp().await;
        let p = rtp::parse(&pkt).unwrap_or_else(|| panic!("paquet RTP illisible : {pkt:?}"));
        let opus = p.open(cipher).unwrap_or_else(|| panic!("déchiffrement impossible"));
        (p.seq, p.timestamp, p.ssrc, opus)
    }

    /// Hello, Identify (renvoyé), Ready, découverte d'adresse, Select
    /// Protocol (renvoyé), Session Description dans le mode choisi.
    pub async fn handshake(&self, c: &mut VoiceConn, modes: &[&str], dave: u16) -> Handshake {
        c.hello(30_000.0).await;
        let identify = c.recv_op(0).await["d"].take();
        c.send(json!({"op": 2, "d": {"ssrc": MY_SSRC, "ip": "127.0.0.1", "port": self.udp_port(), "modes": modes, "heartbeat_interval": 1}})).await;
        let (req, client) = self.recv_udp().await;
        assert_eq!(rtp::discovery_request_ssrc(&req), Some(MY_SSRC), "requête de découverte");
        self.udp.send_to(&rtp::discovery_reply(MY_SSRC, "203.0.113.9", client.port()), client).await.unwrap_or_else(|e| panic!("UDP : {e}"));
        let select = c.recv_op(1).await["d"].take();
        let mode = select["data"]["mode"].as_str().and_then(Mode::from_name).unwrap_or_else(|| panic!("mode : {select}"));
        c.send(json!({"op": 4, "d": {"mode": mode.name(), "secret_key": KEY.to_vec(), "dave_protocol_version": dave}})).await;
        let cipher = Cipher::new(mode, KEY).unwrap_or_else(|| panic!("clé"));
        Handshake { identify, select, client, cipher }
    }

    /// Paquet audio chiffré venant de `ssrc`, avec une extension d'en-tête
    /// d'un mot (comme les clients officiels).
    pub async fn send_audio(&self, h: &Handshake, ssrc: u32, seq: u16, ts: u32, counter: u32, opus: &[u8]) {
        let mut header = rtp::header(seq, ts, ssrc).to_vec();
        header[0] |= 0x10;
        header.extend_from_slice(&[0xBE, 0xDE, 0, 1]);
        let mut payload = vec![0x10, 0x7F, 0, 0];
        payload.extend_from_slice(opus);
        let pkt = h.cipher.seal(&header, &payload, counter).unwrap_or_else(|| panic!("chiffrement"));
        self.udp.send_to(&pkt, h.client).await.unwrap_or_else(|e| panic!("UDP : {e}"));
    }

    /// Rapport RTCP (à ignorer par le client).
    pub async fn send_rtcp(&self, h: &Handshake) {
        let mut rr = vec![0x81, 201, 0, 7];
        rr.extend_from_slice(&[0; 28]);
        self.udp.send_to(&rr, h.client).await.unwrap_or_else(|e| panic!("UDP : {e}"));
    }

    /// Paquet RTP quelconque (vidéo, RTX) chiffré pour le transport.
    pub async fn send_sealed(&self, h: &Handshake, header: &[u8], payload: &[u8], counter: u32) {
        let pkt = h.cipher.seal(header, payload, counter).unwrap_or_else(|| panic!("chiffrement"));
        self.udp.send_to(&pkt, h.client).await.unwrap_or_else(|e| panic!("UDP : {e}"));
    }

    /// RTCP (en clair) chiffré pour le transport : en-tête de 8 octets authentifié.
    pub async fn send_rtcp_sealed(&self, h: &Handshake, plain: &[u8], counter: u32) {
        let pkt = video::rtcp::seal(&h.cipher, plain, counter).unwrap_or_else(|| panic!("chiffrement RTCP"));
        self.udp.send_to(&pkt, h.client).await.unwrap_or_else(|e| panic!("UDP : {e}"));
    }

    /// Prochain paquet du client, classé : RTP (type, marqueur, séquence,
    /// horodatage, SSRC, charge claire) ou RTCP en clair.
    pub async fn recv_media(&self, cipher: &Cipher) -> Media {
        let (pkt, _) = self.recv_udp().await;
        if rtp::is_rtcp(&pkt) {
            return Media::Rtcp(video::rtcp::open(cipher, &pkt).unwrap_or_else(|| panic!("RTCP illisible")));
        }
        let p = rtp::parse_any(&pkt).unwrap_or_else(|| panic!("paquet RTP illisible : {pkt:?}"));
        let payload = p.open(cipher).unwrap_or_else(|| panic!("déchiffrement impossible"));
        Media::Rtp { pt: p.payload_type, marker: p.marker, seq: p.seq, ts: p.timestamp, ssrc: p.ssrc, payload }
    }
}

#[derive(Debug)]
pub enum Media {
    Rtp { pt: u8, marker: bool, seq: u16, ts: u32, ssrc: u32, payload: Vec<u8> },
    Rtcp(Vec<u8>),
}

pub struct Handshake {
    pub identify: Value,
    pub select: Value,
    pub client: SocketAddr,
    pub cipher: Cipher,
}

impl VoiceConn {
    pub async fn send(&mut self, v: Value) {
        let _ = self.ws.send(WsMsg::text(v.to_string())).await;
    }

    pub async fn send_binary(&mut self, b: Vec<u8>) {
        let _ = self.ws.send(WsMsg::binary(b)).await;
    }

    pub async fn hello(&mut self, interval_ms: f64) {
        self.send(json!({"op": 8, "d": {"heartbeat_interval": interval_ms}})).await;
    }

    pub async fn close(&mut self, code: u16) {
        let frame = CloseFrame { code: CloseCode::from(code), reason: "".into() };
        let _ = self.ws.send(WsMsg::Close(Some(frame))).await;
    }

    pub fn drop_abruptly(self) {
        drop(self.ws);
    }

    /// Prochain message du client ; les heartbeats sont accusés et retenus, pas renvoyés.
    pub async fn recv(&mut self) -> Msg {
        loop {
            let m = within("message du client", self.ws.next()).await;
            match m {
                Some(Ok(WsMsg::Text(t))) => {
                    let v: Value = serde_json::from_str(t.as_str()).unwrap_or_else(|e| panic!("JSON client invalide : {e}"));
                    if v["op"] == 3 {
                        let nonce = v["d"]["t"].clone();
                        self.last_heartbeat = Some(v["d"].clone());
                        self.send(json!({"op": 6, "d": {"t": nonce}})).await;
                        continue;
                    }
                    return Msg::Json(v);
                }
                Some(Ok(WsMsg::Binary(b))) => return Msg::Binary(b.to_vec()),
                Some(Ok(WsMsg::Close(f))) => panic!("le client a fermé : {f:?}"),
                Some(Ok(_)) => {}
                Some(Err(e)) => panic!("erreur WebSocket : {e}"),
                None => panic!("le client a coupé"),
            }
        }
    }

    pub async fn recv_op(&mut self, op: u64) -> Value {
        match self.recv().await {
            Msg::Json(v) if v["op"] == op => v,
            other => panic!("op {op} attendu, reçu {other:?}"),
        }
    }

    /// Attend un heartbeat (accusé) ; renvoie son `d`.
    pub async fn wait_heartbeat(&mut self) -> Value {
        self.last_heartbeat = None;
        loop {
            let m = within("heartbeat du client", self.ws.next()).await;
            if let Some(Ok(WsMsg::Text(t))) = m {
                let v: Value = serde_json::from_str(t.as_str()).unwrap_or_default();
                if v["op"] == 3 {
                    self.send(json!({"op": 6, "d": {"t": v["d"]["t"].clone()}})).await;
                    // Heartbeats lents ensuite : le test n'a plus à les accuser.
                    self.hello(30_000.0).await;
                    return v["d"].clone();
                }
            } else if !matches!(m, Some(Ok(_))) {
                panic!("connexion perdue en attendant un heartbeat");
            }
        }
    }

    /// Attend que le client ferme la connexion.
    pub async fn expect_closed(&mut self) {
        within("fermeture par le client", async {
            loop {
                match self.ws.next().await {
                    Some(Ok(WsMsg::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
    }
}

/// Réglages courts pour les tests.
pub fn config() -> VoiceConfig {
    VoiceConfig {
        hosts: VoiceHosts::Loopback,
        reconnect_base: Duration::from_millis(10),
        max_resumes: 3,
        min_heartbeat: Duration::from_millis(1),
        discovery_timeout: Duration::from_millis(500),
        video_codecs: ferrocord::net::voice::video::default_offers(),
        ws_headers: Vec::new(),
    }
}
