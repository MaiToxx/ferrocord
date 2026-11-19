//! Tâche d'une connexion vocale : passerelle (WebSocket) et transport UDP
//! dans une seule boucle, reprise (op 7) après une coupure.

use super::{
    close_action,
    crypt::{Cipher, Mode},
    e2ee::{FrameCrypto, Outgoing},
    rtp, video, CloseReason, ConnKind, VoiceConfig, VoiceCtl, VoiceEvent, VoiceParams,
};
use crate::video::{Codec, EncodedFrame};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{BTreeSet, HashMap},
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio::{
    net::{TcpStream, UdpSocket},
    sync::mpsc,
    time::Instant,
};
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message as WsMsg},
    MaybeTlsStream, WebSocketStream,
};

type Ws = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// Les paquets de la passerelle vocale sont petits ; les messages MLS (DAVE)
/// grossissent avec le nombre de participants, sans approcher cette limite.
const MAX_MESSAGE: usize = 1 << 20;
const DISCOVERY_TRIES: u32 = 5;
const SPEAKING_MICROPHONE: u64 = 1;
/// Sans émission, une trame de silence part à ce rythme : sans trafic
/// sortant, les routeurs (NAT) oublient la correspondance UDP et plus rien
/// n'arrive pendant les silences.
const KEEPALIVE: Duration = Duration::from_secs(5);
/// Ouverture du WebSocket vocal (TCP, TLS, poignée de main) : au-delà, échec.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// Attente du Hello (op 8) après l'ouverture : sans lui, aucun battement ne
/// part et une connexion muette ne serait jamais détectée.
const HELLO_TIMEOUT: Duration = Duration::from_secs(15);

pub(super) async fn run(p: VoiceParams, mut e2ee: Box<dyn FrameCrypto>, cfg: VoiceConfig, mut ctl: mpsc::Receiver<VoiceCtl>, ev: mpsc::Sender<VoiceEvent>) {
    let reason = drive(&p, e2ee.as_mut(), &cfg, &mut ctl, &ev).await;
    let _ = ev.send(VoiceEvent::Closed(reason)).await;
}

/// État qui survit aux reprises.
#[derive(Default)]
struct State {
    /// Dernier numéro de séquence reçu (`seq_ack`), -1 avant le premier.
    seq_ack: i64,
    ssrc: u32,
    udp: Option<UdpSocket>,
    cipher: Option<Cipher>,
    rtp_seq: u16,
    rtp_ts: u32,
    nonce: u32,
    speaking: bool,
    ssrcs: HashMap<u32, u64>,
    users: BTreeSet<u64>,
    /// Clé de session reçue : une coupure se répare par une reprise.
    identified: bool,
    /// La dernière connexion a abouti (Session Description ou Resumed).
    live: bool,
    /// Dernier paquet RTP envoyé (maintien UDP).
    last_rtp: Option<Instant>,
    video: video::Link,
    /// Vidéo annoncée par `VoiceCtl::VideoState` : (largeur, hauteur, images/s).
    want_video: Option<(u32, u32, u32)>,
    /// Une image n'a pas pu partir (connexion ou DAVE pas prêts) : rien ne
    /// part plus avant une image clé, que les suivantes ne se décodent pas sans elle.
    video_gap: bool,
}

enum End {
    /// Coupure à réparer par une reprise.
    Lost,
    Stop(CloseReason),
}

struct Ctx<'a> {
    p: &'a VoiceParams,
    cfg: &'a VoiceConfig,
    e2ee: &'a mut dyn FrameCrypto,
    ev: &'a mpsc::Sender<VoiceEvent>,
    st: State,
    /// Codec d'envoi annoncé à l'appelant (qui suppose H.264 tant qu'on ne lui dit rien).
    told_codec: Codec,
}

async fn drive(p: &VoiceParams, e2ee: &mut dyn FrameCrypto, cfg: &VoiceConfig, ctl: &mut mpsc::Receiver<VoiceCtl>, ev: &mpsc::Sender<VoiceEvent>) -> CloseReason {
    let Some(url) = cfg.hosts.url(&p.endpoint) else {
        return CloseReason::BadEndpoint;
    };
    let mut cx = Ctx { p, cfg, e2ee, ev, st: State { seq_ack: -1, ..State::default() }, told_codec: Codec::H264 };
    let mut failures = 0u32;
    loop {
        let ws_cfg = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
        let end = match crate::net::identity::ws_request(url.as_str(), &cfg.ws_headers) {
            // Ouverture plafonnée : une poignée de main qui ne répond plus ne
            // doit pas laisser l'appel « en cours de connexion » sans fin.
            Ok(req) => match tokio::time::timeout(CONNECT_TIMEOUT, tokio_tungstenite::connect_async_with_config(req, Some(ws_cfg), false)).await {
                Ok(Ok((ws, _))) => session(ws, &mut cx, ctl).await,
                Ok(Err(_)) | Err(_) => End::Lost,
            },
            Err(_) => End::Lost,
        };
        if let End::Stop(reason) = end {
            return reason;
        }
        if std::mem::take(&mut cx.st.live) {
            failures = 0;
        }
        failures += 1;
        if failures > cfg.max_resumes {
            return CloseReason::Unreachable;
        }
        // Attente doublée à chaque échec, avec gigue (jamais à heure fixe).
        let wait = crate::net::backoff::equal_jitter(cfg.reconnect_base.saturating_mul(1 << (failures - 1).min(5)), crate::net::backoff::unit());
        // Pendant l'attente, l'audio à envoyer est jeté ; une demande d'arrêt est servie.
        let deadline = Instant::now() + wait;
        loop {
            tokio::select! {
                _ = tokio::time::sleep_until(deadline) => break,
                c = ctl.recv() => match c {
                    None | Some(VoiceCtl::Disconnect) => return CloseReason::Requested,
                    Some(VoiceCtl::Speaking(b)) => cx.st.speaking = b,
                    Some(VoiceCtl::VideoState { active, width, height, fps }) => cx.st.want_video = active.then_some((width, height, fps)),
                    Some(VoiceCtl::SinkWants(w)) => {
                        let _ = cx.st.video.set_wants(&w);
                    }
                    Some(VoiceCtl::Opus(_) | VoiceCtl::Video(_) | VoiceCtl::RequestKeyframe(_)) => {}
                },
            }
        }
    }
}

struct Heartbeat {
    interval: Duration,
    next: Instant,
    /// Nonce et départ du heartbeat sans accusé.
    pending: Option<(u64, Instant)>,
}

struct Discovery {
    mode: Mode,
    tries: u32,
    next: Instant,
}

async fn sleep_opt(at: Option<Instant>) {
    match at {
        Some(t) => tokio::time::sleep_until(t).await,
        None => std::future::pending().await,
    }
}

async fn recv_udp(sock: Option<&UdpSocket>, buf: &mut [u8]) -> usize {
    match sock {
        // Une erreur (ICMP « port injoignable » sous Windows) n'arrête rien.
        Some(s) => s.recv(buf).await.unwrap_or(0),
        None => std::future::pending().await,
    }
}

async fn session(mut ws: Ws, cx: &mut Ctx<'_>, ctl: &mut mpsc::Receiver<VoiceCtl>) -> End {
    let first = if cx.st.identified {
        json!({"op": 7, "d": {
            "server_id": cx.server_id(),
            "session_id": cx.p.session_id,
            "token": cx.p.token.expose(),
            "seq_ack": cx.st.seq_ack,
        }})
    } else {
        // Nouvelle identification : on repart de zéro (UDP, clé, participants).
        let screen = matches!(cx.p.kind, ConnKind::Stream { .. });
        let video = video::Link::new(cx.cfg.video_codecs.clone(), screen);
        cx.st = State { seq_ack: -1, speaking: cx.st.speaking, want_video: cx.st.want_video, video, ..State::default() };
        let mut v = json!({"op": 0, "d": {
            "server_id": cx.server_id(),
            "user_id": cx.p.user_id.to_string(),
            "session_id": cx.p.session_id,
            "token": cx.p.token.expose(),
            "max_dave_protocol_version": cx.e2ee.max_protocol_version(),
        }});
        if cx.st.video.enabled() {
            v["d"]["video"] = json!(true);
            v["d"]["streams"] = video::identify_streams(screen);
        }
        if screen {
            v["d"]["channel_id"] = json!(cx.p.channel_id.to_string());
        }
        v
    };
    if ws.send(WsMsg::text(first.to_string())).await.is_err() {
        return End::Lost;
    }
    let mut hb: Option<Heartbeat> = None;
    let mut disc: Option<Discovery> = None;
    let mut buf = vec![0u8; 2048];
    let hello_by = Instant::now() + HELLO_TIMEOUT;
    loop {
        let hb_at = hb.as_ref().map(|h| h.next);
        let disc_at = disc.as_ref().map(|d| d.next);
        let keep_at = if cx.st.cipher.is_some() && !cx.st.speaking { cx.st.last_rtp.map(|t| t + KEEPALIVE) } else { None };
        let step = tokio::select! {
            m = ws.next() => match m {
                Some(Ok(WsMsg::Text(t))) => cx.on_json(&mut ws, t.as_str(), &mut hb, &mut disc).await,
                Some(Ok(WsMsg::Binary(b))) => cx.on_binary(&mut ws, &b).await,
                Some(Ok(WsMsg::Close(f))) => match close_action(f.map(|f| u16::from(f.code))) {
                    None => Err(End::Lost),
                    Some(r) => Err(End::Stop(r)),
                },
                Some(Ok(_)) => Ok(()),
                Some(Err(_)) | None => Err(End::Lost),
            },
            _ = sleep_opt(hb_at) => cx.heartbeat(&mut ws, &mut hb).await,
            // Pas de Hello : connexion muette, à reprendre.
            _ = tokio::time::sleep_until(hello_by), if hb.is_none() => Err(End::Lost),
            _ = sleep_opt(disc_at) => cx.retry_discovery(&mut disc).await,
            // La trame de silence n'est jamais chiffrée de bout en bout.
            _ = sleep_opt(keep_at) => {
                cx.send_rtp(&rtp::SILENCE).await;
                Ok(())
            }
            c = ctl.recv() => match c {
                None | Some(VoiceCtl::Disconnect) => {
                    let _ = ws.close(None).await;
                    Err(End::Stop(CloseReason::Requested))
                }
                Some(VoiceCtl::Opus(f)) => cx.send_audio(&mut ws, &f).await,
                Some(VoiceCtl::Speaking(b)) => cx.set_speaking(&mut ws, b).await,
                Some(VoiceCtl::Video(f)) => cx.send_video(&f).await,
                Some(VoiceCtl::VideoState { active, width, height, fps }) => cx.set_video(&mut ws, active.then_some((width, height, fps))).await,
                Some(VoiceCtl::RequestKeyframe(user)) => {
                    if let Some(p) = cx.st.video.request_keyframe(user) {
                        cx.send_rtcp(&p).await;
                    }
                    Ok(())
                }
                Some(VoiceCtl::SinkWants(w)) => match cx.st.video.set_wants(&w) {
                    Some(op) => Ctx::send(&mut ws, op).await,
                    None => Ok(()),
                },
            },
            n = recv_udp(cx.st.udp.as_ref(), &mut buf) => cx.on_udp(&mut ws, &buf[..n], &mut disc).await,
        };
        if let Err(end) = step {
            return end;
        }
    }
}

type Step = Result<(), End>;

#[derive(Deserialize)]
struct Envelope {
    op: u8,
    #[serde(default)]
    d: Value,
    seq: Option<i64>,
}

fn snowflake(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => s.parse().ok(),
        v => v.as_u64(),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

impl Ctx<'_> {
    fn server_id(&self) -> String {
        self.p.guild_id.unwrap_or(self.p.channel_id).to_string()
    }

    async fn emit(&mut self, e: VoiceEvent) -> Step {
        self.ev.send(e).await.map_err(|_| End::Stop(CloseReason::Requested))
    }

    /// `video_codec` de la Session Description ou Update : l'encodeur est
    /// prévenu quand le codec d'envoi change (y compris d'une connexion à la
    /// suivante).
    async fn video_session(&mut self, d: &Value) -> Step {
        self.st.video.on_session(d);
        match self.st.video.send_codec() {
            Some(c) if c != self.told_codec => {
                self.told_codec = c;
                self.emit(VoiceEvent::SendCodec(c)).await
            }
            _ => Ok(()),
        }
    }

    async fn send(ws: &mut Ws, v: Value) -> Step {
        ws.send(WsMsg::text(v.to_string())).await.map_err(|_| End::Lost)
    }

    async fn flush(ws: &mut Ws, out: Vec<Outgoing>) -> Step {
        for o in out {
            let m = match o {
                Outgoing::Json(v) => WsMsg::text(v.to_string()),
                Outgoing::Binary(op, data) => {
                    let mut b = Vec::with_capacity(1 + data.len());
                    b.push(op);
                    b.extend_from_slice(&data);
                    WsMsg::binary(b)
                }
            };
            ws.send(m).await.map_err(|_| End::Lost)?;
        }
        Ok(())
    }

    async fn on_json(&mut self, ws: &mut Ws, text: &str, hb: &mut Option<Heartbeat>, disc: &mut Option<Discovery>) -> Step {
        let Ok(env) = serde_json::from_str::<Envelope>(text) else {
            return Ok(());
        };
        if let Some(s) = env.seq {
            self.st.seq_ack = s;
        }
        let d = &env.d;
        match env.op {
            8 => {
                let ms = d["heartbeat_interval"].as_f64().unwrap_or(0.0).clamp(1.0, 60_000.0);
                let interval = Duration::from_secs_f64(ms / 1000.0);
                *hb = Some(Heartbeat { interval, next: Instant::now() + interval, pending: None });
            }
            2 => return self.on_ready(d, disc).await,
            4 => {
                let mode = d["mode"].as_str().and_then(Mode::from_name).ok_or(End::Stop(CloseReason::Unsupported))?;
                let key: Option<Vec<u8>> = d["secret_key"].as_array().map(|a| a.iter().filter_map(|b| b.as_u64().and_then(|b| u8::try_from(b).ok())).collect());
                let key: [u8; 32] = key.and_then(|k| k.try_into().ok()).ok_or(End::Stop(CloseReason::Unsupported))?;
                self.st.cipher = Some(Cipher::new(mode, key).ok_or(End::Stop(CloseReason::Unsupported))?);
                self.st.last_rtp = Some(Instant::now());
                let first = !self.st.identified;
                self.st.identified = true;
                self.st.live = true;
                let mut out = Vec::new();
                self.e2ee.on_op(4, Some(d), None, &mut out);
                Self::flush(ws, out).await?;
                if self.st.speaking {
                    self.send_speaking(ws, true).await?;
                }
                self.video_session(d).await?;
                if let Some((w, h, fps)) = self.st.want_video
                    && self.st.video.enabled()
                {
                    Self::send(ws, self.st.video.set_state(true, w, h, fps)).await?;
                }
                if first {
                    self.emit(VoiceEvent::Ready { ssrc: self.st.ssrc }).await?;
                }
            }
            5 => {
                if let (Some(user), Some(ssrc)) = (snowflake(&d["user_id"]), d["ssrc"].as_u64().and_then(|s| u32::try_from(s).ok())) {
                    self.st.ssrcs.insert(ssrc, user);
                    let speaking = d["speaking"].as_u64().unwrap_or(0) != 0;
                    self.emit(VoiceEvent::Speaking { user_id: user, speaking }).await?;
                }
            }
            6 => {
                if let Some(h) = hb.as_mut()
                    && let Some((nonce, at)) = h.pending
                    && d["t"].as_u64() == Some(nonce)
                {
                    h.pending = None;
                    let ms = u32::try_from(at.elapsed().as_millis()).unwrap_or(u32::MAX);
                    self.emit(VoiceEvent::Ping(ms)).await?;
                }
            }
            9 => self.st.live = true,
            12 => {
                if let Some((user_id, active)) = self.st.video.on_video_op(d, self.p.user_id) {
                    // Nouvelle SSRC d'une personne dont la qualité est réglée : op 15 refaite.
                    if active
                        && self.st.video.wanted(user_id)
                        && let Some(op) = self.st.video.wants_op()
                    {
                        Self::send(ws, op).await?;
                    }
                    self.emit(VoiceEvent::VideoState { user_id, active }).await?;
                }
            }
            // Session Update : le codec vidéo à envoyer peut changer.
            14 => self.video_session(d).await?,
            11 => {
                let ids: Vec<u64> = d["user_ids"].as_array().map(|a| a.iter().filter_map(snowflake).collect()).unwrap_or_default();
                for id in ids {
                    if id != self.p.user_id && self.st.users.insert(id) {
                        self.emit(VoiceEvent::UserJoined(id)).await?;
                    }
                }
                self.users_changed();
            }
            13 => {
                if let Some(id) = snowflake(&d["user_id"]) {
                    self.st.ssrcs.retain(|_, u| *u != id);
                    self.st.video.user_left(id);
                    if self.st.users.remove(&id) {
                        self.emit(VoiceEvent::UserLeft(id)).await?;
                    }
                    self.users_changed();
                }
            }
            op @ 21..=31 => {
                let mut out = Vec::new();
                self.e2ee.on_op(op, Some(d), None, &mut out);
                Self::flush(ws, out).await?;
            }
            _ => {}
        }
        Ok(())
    }

    fn users_changed(&mut self) {
        let list: Vec<u64> = self.st.users.iter().copied().collect();
        self.e2ee.on_users(&list);
    }

    async fn on_ready(&mut self, d: &Value, disc: &mut Option<Discovery>) -> Step {
        let ssrc = d["ssrc"].as_u64().and_then(|s| u32::try_from(s).ok());
        let ip = d["ip"].as_str().and_then(|s| s.parse::<IpAddr>().ok());
        let port = d["port"].as_u64().and_then(|p| u16::try_from(p).ok());
        let (Some(ssrc), Some(ip), Some(port)) = (ssrc, ip, port) else {
            return Err(End::Stop(CloseReason::Unreachable));
        };
        if !self.cfg.hosts.udp_ok(ip) {
            return Err(End::Stop(CloseReason::BadEndpoint));
        }
        let modes: Vec<&str> = d["modes"].as_array().map(|a| a.iter().filter_map(Value::as_str).collect()).unwrap_or_default();
        let mode = Mode::choose(&modes).ok_or(End::Stop(CloseReason::Unsupported))?;
        let local: SocketAddr = match (self.cfg.hosts, ip) {
            // Tests : jamais d'écoute hors de la boucle locale.
            (super::VoiceHosts::Loopback, _) => (IpAddr::from([127, 0, 0, 1]), 0).into(),
            (_, IpAddr::V4(_)) => (IpAddr::from([0, 0, 0, 0]), 0).into(),
            (_, IpAddr::V6(_)) => (IpAddr::from([0u16; 8]), 0).into(),
        };
        let sock = UdpSocket::bind(local).await.map_err(|_| End::Stop(CloseReason::Unreachable))?;
        // Socket « connectée » : seuls les paquets du serveur vocal sont lus.
        sock.connect((ip, port)).await.map_err(|_| End::Stop(CloseReason::Unreachable))?;
        let _ = sock.send(&rtp::discovery_request(ssrc)).await;
        self.st.ssrc = ssrc;
        self.st.video.on_ready(d, ssrc);
        self.st.udp = Some(sock);
        *disc = Some(Discovery { mode, tries: 1, next: Instant::now() + self.cfg.discovery_timeout });
        Ok(())
    }

    async fn retry_discovery(&mut self, disc: &mut Option<Discovery>) -> Step {
        let Some(d) = disc.as_mut() else { return Ok(()) };
        if d.tries >= DISCOVERY_TRIES {
            return Err(End::Stop(CloseReason::Unreachable));
        }
        d.tries += 1;
        d.next = Instant::now() + self.cfg.discovery_timeout;
        if let Some(s) = &self.st.udp {
            let _ = s.send(&rtp::discovery_request(self.st.ssrc)).await;
        }
        Ok(())
    }

    async fn on_binary(&mut self, ws: &mut Ws, b: &[u8]) -> Step {
        // Serveur → client : séquence (16 bits), opcode, charge utile.
        if b.len() < 3 {
            return Ok(());
        }
        self.st.seq_ack = i64::from(u16::from_be_bytes([b[0], b[1]]));
        let op = b[2];
        if (21..=31).contains(&op) {
            let mut out = Vec::new();
            self.e2ee.on_op(op, None, Some(&b[3..]), &mut out);
            Self::flush(ws, out).await?;
        }
        Ok(())
    }

    async fn heartbeat(&mut self, ws: &mut Ws, hb: &mut Option<Heartbeat>) -> Step {
        let Some(h) = hb.as_mut() else { return Ok(()) };
        // Heartbeat précédent jamais accusé : connexion morte, on reprend.
        if h.pending.is_some() {
            return Err(End::Lost);
        }
        let nonce = now_ms();
        h.pending = Some((nonce, Instant::now()));
        h.next = Instant::now() + h.interval;
        Self::send(ws, json!({"op": 3, "d": {"t": nonce, "seq_ack": self.st.seq_ack}})).await
    }

    async fn on_udp(&mut self, ws: &mut Ws, pkt: &[u8], disc: &mut Option<Discovery>) -> Step {
        if let Some(d) = disc.as_ref() {
            if let Some((address, port)) = rtp::discovery_response(pkt) {
                let mode = d.mode;
                *disc = None;
                let mut select = json!({"op": 1, "d": {"protocol": "udp", "data": {"address": address, "port": port, "mode": mode.name()}}});
                if self.st.video.enabled() {
                    select["d"]["codecs"] = video::select_codecs(self.st.video.offers());
                }
                return Self::send(ws, select).await;
            }
            return Ok(());
        }
        if rtp::is_rtcp(pkt) {
            return self.on_rtcp(pkt).await;
        }
        if let Some(p) = rtp::parse_any(pkt)
            && p.payload_type != rtp::PAYLOAD_TYPE
        {
            return self.on_video_rtp(&p).await;
        }
        let (Some(cipher), Some(p)) = (self.st.cipher.as_ref(), rtp::parse(pkt)) else {
            return Ok(());
        };
        let Some(&user_id) = self.st.ssrcs.get(&p.ssrc) else { return Ok(()) };
        let Some(frame) = p.open(cipher) else { return Ok(()) };
        // La trame de silence n'est jamais chiffrée de bout en bout.
        let opus = if frame == rtp::SILENCE {
            frame
        } else {
            match self.e2ee.decrypt(user_id, &frame) {
                Some(f) => f,
                None => return Ok(()),
            }
        };
        // Lecteur en retard : la trame est perdue plutôt que de bloquer le réseau.
        match self.ev.try_send(VoiceEvent::Audio { user_id, seq: p.seq, timestamp: p.timestamp, opus }) {
            Err(mpsc::error::TrySendError::Closed(_)) => Err(End::Stop(CloseReason::Requested)),
            _ => Ok(()),
        }
    }

    async fn send_speaking(&mut self, ws: &mut Ws, on: bool) -> Step {
        let flags = if on { SPEAKING_MICROPHONE } else { 0 };
        Self::send(ws, json!({"op": 5, "d": {"speaking": flags, "delay": 0, "ssrc": self.st.ssrc}})).await
    }

    async fn set_speaking(&mut self, ws: &mut Ws, on: bool) -> Step {
        if on == self.st.speaking {
            return Ok(());
        }
        self.st.speaking = on;
        if self.st.cipher.is_none() {
            // Envoyé avec la clé de session (Session Description).
            return Ok(());
        }
        if on {
            return self.send_speaking(ws, true).await;
        }
        for _ in 0..5 {
            self.send_rtp(&rtp::SILENCE).await;
        }
        self.send_speaking(ws, false).await
    }

    async fn send_audio(&mut self, ws: &mut Ws, opus: &[u8]) -> Step {
        if self.st.cipher.is_none() {
            return Ok(());
        }
        // Le serveur exige un Speaking avant tout audio.
        if !self.st.speaking {
            self.st.speaking = true;
            self.send_speaking(ws, true).await?;
        }
        if let Some(frame) = self.e2ee.encrypt(opus) {
            self.send_rtp(&frame).await;
        }
        Ok(())
    }

    async fn send_rtp(&mut self, payload: &[u8]) {
        let (Some(cipher), Some(sock)) = (self.st.cipher.as_ref(), self.st.udp.as_ref()) else { return };
        let header = rtp::header(self.st.rtp_seq, self.st.rtp_ts, self.st.ssrc);
        if let Some(pkt) = cipher.seal(&header, payload, self.st.nonce) {
            let _ = sock.send(&pkt).await;
        }
        self.st.last_rtp = Some(Instant::now());
        self.st.rtp_seq = self.st.rtp_seq.wrapping_add(1);
        self.st.rtp_ts = self.st.rtp_ts.wrapping_add(rtp::FRAME_SAMPLES);
        self.st.nonce = self.st.nonce.wrapping_add(1);
    }

    /// Paquet RTP vidéo ou RTX déjà construit : chiffrement de transport et envoi.
    async fn send_sealed(&mut self, header: &[u8], payload: &[u8]) {
        let (Some(cipher), Some(sock)) = (self.st.cipher.as_ref(), self.st.udp.as_ref()) else { return };
        if let Some(pkt) = cipher.seal(header, payload, self.st.nonce) {
            let _ = sock.send(&pkt).await;
        }
        self.st.nonce = self.st.nonce.wrapping_add(1);
    }

    async fn send_rtcp(&mut self, plain: &[u8]) {
        let (Some(cipher), Some(sock)) = (self.st.cipher.as_ref(), self.st.udp.as_ref()) else { return };
        if let Some(pkt) = video::rtcp::seal(cipher, plain, self.st.nonce) {
            let _ = sock.send(&pkt).await;
        }
        self.st.nonce = self.st.nonce.wrapping_add(1);
    }

    async fn set_video(&mut self, ws: &mut Ws, want: Option<(u32, u32, u32)>) -> Step {
        self.st.want_video = want;
        if self.st.cipher.is_none() || !self.st.video.enabled() {
            // Envoyé avec la clé de session (Session Description).
            return Ok(());
        }
        let (w, h, fps) = want.unwrap_or_default();
        Self::send(ws, self.st.video.set_state(want.is_some(), w, h, fps)).await
    }

    async fn send_video(&mut self, f: &EncodedFrame) -> Step {
        if self.st.cipher.is_none() || self.st.want_video.is_none() {
            self.st.video_gap = true;
            return Ok(());
        }
        // Après une image perdue, la première à partir est une image clé : elle
        // est demandée à l'encodeur, et les images intermédiaires sont jetées.
        if self.st.video_gap && !f.keyframe {
            return match self.ev.try_send(VoiceEvent::KeyframeRequested) {
                Err(mpsc::error::TrySendError::Closed(_)) => Err(End::Stop(CloseReason::Requested)),
                _ => Ok(()),
            };
        }
        // DAVE pas encore prêt : l'image ne part pas (jamais en clair).
        let Some(data) = self.e2ee.encrypt_video(f.codec, &f.data) else {
            self.st.video_gap = true;
            return Ok(());
        };
        self.st.video_gap = false;
        let (pkts, sr) = self.st.video.send_frame(f.codec, f.ts90k, &data, std::time::Instant::now());
        for p in pkts {
            self.send_sealed(&p.header, &p.payload).await;
        }
        if let Some(sr) = sr {
            self.send_rtcp(&sr).await;
        }
        Ok(())
    }

    /// RTCP : seuls les NACK, PLI et FIR visant notre vidéo comptent ; le reste
    /// (rapports de réception de l'audio, RTCP non authentifié) est ignoré.
    async fn on_rtcp(&mut self, pkt: &[u8]) -> Step {
        let Some(plain) = self.st.cipher.as_ref().and_then(|c| video::rtcp::open(c, pkt)) else { return Ok(()) };
        let fb = self.st.video.on_rtcp(&plain);
        for r in fb.retransmit {
            self.send_sealed(&r.header, &r.payload).await;
        }
        if fb.keyframe && let Err(mpsc::error::TrySendError::Closed(_)) = self.ev.try_send(VoiceEvent::KeyframeRequested) {
            return Err(End::Stop(CloseReason::Requested));
        }
        Ok(())
    }

    async fn on_video_rtp(&mut self, p: &rtp::Packet<'_>) -> Step {
        if !self.st.video.owns(p.ssrc) {
            return Ok(());
        }
        let Some(payload) = self.st.cipher.as_ref().and_then(|c| p.open(c)) else { return Ok(()) };
        let now = std::time::Instant::now();
        let Some(r) = self.st.video.on_rtp(video::RtpMeta { ssrc: p.ssrc, payload_type: p.payload_type, seq: p.seq, ts: p.timestamp, marker: p.marker }, &payload, now) else { return Ok(()) };
        for c in &r.rtcp {
            self.send_rtcp(c).await;
        }
        let Some(codec) = r.codec else { return Ok(()) };
        for f in r.frames {
            // Image illisible ou perdue faute de place : le décodeur attendra une image clé.
            let Some(data) = self.e2ee.decrypt_video(r.user_id, codec, &f.data) else {
                self.st.video.lost_frame(r.user_id);
                continue;
            };
            let frame = EncodedFrame { codec, keyframe: f.keyframe, ts90k: f.ts, data };
            match self.ev.try_send(VoiceEvent::Video { user_id: r.user_id, frame }) {
                Err(mpsc::error::TrySendError::Closed(_)) => return Err(End::Stop(CloseReason::Requested)),
                Err(mpsc::error::TrySendError::Full(_)) => self.st.video.lost_frame(r.user_id),
                Ok(()) => {}
            }
        }
        Ok(())
    }
}
