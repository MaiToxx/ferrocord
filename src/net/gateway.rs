//! Passerelle WebSocket : session, battement de cœur, reprise, dispatch.
//!
//! Chaque connexion a son propre contexte de décompression (`inflate`) et son
//! propre limiteur d'envoi (`gwlimit`) : tout envoi passe par [`GwSink`].
//! Un paquet volumineux (READY de plusieurs Mo) est décompressé et décodé
//! sur un fil bloquant, pendant que la boucle continue de battre.

use super::gwlimit::{IdentifyLimiter, Priority, SendLimiter, COMMANDS_PER_WINDOW, WINDOW};
use super::inflate::{InflateError, Inflater};
use super::raw::{
    Hello, Payload, RawChannel, RawChannelMark, RawGuild, RawGuildMember, RawGuildUpdate, RawIdChannel, RawMessage, RawMessageUpdate, RawReady, RawRoleDelete, align_merged,
    RawRoleEvent, RawEmojisUpdate, RawTyping, into_emojis,
};
use super::members::Pacer;
use super::{wall_clock_ms, BoxError, Endpoints, Out, ResumeHosts};
use crate::state::members::Subscription;
use crate::state::model::{Avatar, Dm, Guild, User};
use crate::state::{Cause, Event, Problem, Status};
use futures_util::{Sink, SinkExt, StreamExt};
use serde_json::json;
use std::{collections::HashMap, collections::VecDeque, time::Duration};
use tokio::{task::JoinHandle, time::Instant};
use tokio_tungstenite::tungstenite::{self, protocol::WebSocketConfig, Bytes, Message as WsMsg, Utf8Bytes};

#[derive(Clone)]
pub(super) struct Resume {
    session_id: String,
    /// `None` : adresse reçue refusée, la reprise passe par la passerelle par défaut.
    url: Option<String>,
}

/// Ce qui survit d'une connexion à la suivante.
#[derive(Clone, Default)]
pub(super) struct SessionState {
    resume: Option<Resume>,
    seq: Option<u64>,
    /// Mon identifiant (lu dans READY) : sert à trouver mon membre dans GUILD_CREATE.
    me: Option<u64>,
}

impl SessionState {
    fn resumable(&self) -> bool {
        self.resume.is_some() && self.seq.is_some()
    }
}

/// Attente maximale entre deux tentatives de connexion.
const MAX_RECONNECT_DELAY: Duration = Duration::from_secs(60);
/// Intervalle de heartbeat minimal accepté (Discord annonce environ 41 s).
const MIN_HEARTBEAT_MS: u64 = 1000;
/// Au-delà, un message compressé est décompressé hors de la boucle.
const OFFLOAD_COMPRESSED: usize = 64 << 10;
/// Au-delà, un paquet est décodé hors de la boucle.
const OFFLOAD_PACKET: usize = 256 << 10;
/// Commandes en attente d'envoi, au plus (lots suivants : présence, souscriptions).
const MAX_QUEUED: usize = 256;

/// Ouverture de la connexion (TCP, TLS, WebSocket) : au-delà, nouvel essai.
/// Sans ce plafond, une poignée de main TLS qui ne répond plus (réseau changé
/// en cours de route) laissait la tâche attendre sans fin.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// Attente du Hello après l'ouverture (Discord l'envoie aussitôt).
const HELLO_TIMEOUT: Duration = Duration::from_secs(20);
/// Envoi d'un paquet : au-delà, la connexion est tenue pour morte (tampon
/// d'émission plein vers un pair qui ne lit plus).
const SEND_TIMEOUT: Duration = Duration::from_secs(20);
/// Une session n'est « stable » (compteur d'échecs remis à zéro) qu'après
/// `reconnect_base` × ce facteur : 30 s en production. Une session acceptée
/// puis coupée aussitôt, en boucle, voit donc son attente croître.
const STABLE_FACTOR: u32 = 30;
/// Après un code 4008 (débit) ou 4015 (trop de sessions) : attente d'au moins
/// `reconnect_base` × ce facteur (60 s en production, la fenêtre de débit).
const SLOW_FACTOR: u32 = 60;
/// Reprises de suite refusées (connexion ouverte, ni RESUMED ni op 9) avant
/// d'abandonner la session pour un Identify neuf.
const RESUMES_REFUSED_MAX: u32 = 3;
/// Reprises de suite dont l'adresse (`resume_gateway_url`) ne répond pas
/// avant de repasser par la passerelle par défaut.
const RESUMES_UNREACHABLE_MAX: u32 = 3;
/// Marge au-delà de l'intervalle de heartbeat avant de conclure que
/// l'horloge a sauté (veille de l'ordinateur) : au moins 10 s.
const STALL_SLACK: Duration = Duration::from_secs(10);

/// Délai avant la prochaine tentative : `base` doublé à chaque échec
/// consécutif, plafonné à `MAX_RECONNECT_DELAY` (la gigue s'y ajoute :
/// `backoff::equal_jitter`).
fn reconnect_delay(base: Duration, failures: u32) -> Duration {
    base.checked_mul(1 << failures.min(6)).unwrap_or(MAX_RECONNECT_DELAY).min(MAX_RECONNECT_DELAY)
}

/// Le battement arrive-t-il si tard que l'ordinateur a dû dormir (ou que la
/// boucle est restée bloquée) ? `mono` : temps écoulé depuis le battement
/// précédent à l'horloge monotone ; `wall` : à l'horloge murale (`None` si
/// elle a reculé). L'une ou l'autre suffit : selon le système, l'horloge
/// monotone s'arrête ou non pendant la veille.
fn stalled(mono: Duration, wall: Option<Duration>, period: Duration) -> bool {
    let limit = period + (period / 2).max(STALL_SLACK);
    mono > limit || wall.is_some_and(|w| w > limit)
}

/// Où en est une tentative : sert à compter les échecs sans confondre un
/// réseau absent (rien à reprocher à la session) et une reprise refusée.
#[derive(Default)]
struct Progress {
    /// WebSocket ouvert.
    connected: bool,
    /// READY ou RESUMED reçu.
    established: bool,
}

/// Cause de déconnexion typée, transportée dans un `BoxError` jusqu'au statut.
#[derive(Debug)]
struct Lost(Cause);

impl std::fmt::Display for Lost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self.0)
    }
}

impl std::error::Error for Lost {}

/// Cause d'une session terminée par une erreur : la nôtre si elle est
/// typée, sinon le texte (technique) de l'erreur du transport.
fn cause_of(e: BoxError) -> Cause {
    match e.downcast::<Lost>() {
        Ok(l) => l.0,
        Err(e) => Cause::Transport(e.to_string()),
    }
}

impl From<InflateError> for Cause {
    fn from(e: InflateError) -> Cause {
        match e {
            InflateError::Corrupt => Cause::CorruptStream,
            InflateError::TooLarge => Cause::TooLargePacket,
        }
    }
}

enum End {
    Reconnect,
    /// Discord demande de ralentir (4008, 4015) : reprise après une longue attente.
    Slow,
    /// Flux compressé illisible : les connexions suivantes ne compressent plus.
    BadCompression(InflateError),
    Fatal { problem: Problem, bad_token: bool },
}

/// Souscriptions demandées par l'interface, et leur rythme (`members::Pacer`).
/// Survit aux reconnexions : après un nouvel Identify, tout est renvoyé.
pub(super) struct Subs {
    rx: tokio::sync::mpsc::UnboundedReceiver<Subscription>,
    open: bool,
    pacer: Pacer,
    /// Compte bot : les listes passent par les intents, rien n'est envoyé.
    bot: bool,
    /// Vocal (phase 4) : commandes qui donnent une op 4, état tenu, op 4
    /// reçue avant READY/RESUMED (envoyée une fois la session établie).
    voice: Option<tokio::sync::mpsc::UnboundedReceiver<crate::state::voice::VoiceCmd>>,
    op4: crate::state::voice::Op4State,
    op4_pending: Option<String>,
    /// Vidéo (phase 5) : caméra (dans l'op 4) et directs (ops 18, 19, 20).
    video: Option<tokio::sync::mpsc::UnboundedReceiver<crate::state::video::VideoCmd>>,
    gw_video: super::streamgw::GwVideo,
    /// Ma présence (lot C3) : op 3, au plus une par `games::PRESENCE_GAP`.
    presence_rx: Option<tokio::sync::mpsc::UnboundedReceiver<crate::state::activity::OwnPresence>>,
    presence: super::games::PresenceGate,
    /// Identité annoncée (Identify, en-têtes du WebSocket) ; par défaut : Ferrocord.
    identity: std::sync::Arc<super::identity::Identity>,
    ws_headers: Vec<(&'static str, String)>,
    /// Compteurs de la connexion (sessions ouvertes, Identify).
    health: std::sync::Arc<super::health::Health>,
}

impl Subs {
    pub(super) fn new(rx: tokio::sync::mpsc::UnboundedReceiver<Subscription>, token: &str) -> Self {
        Subs { rx, open: true, pacer: Pacer::default(), bot: token.starts_with("Bot "), voice: None, op4: Default::default(), op4_pending: None, video: None, gw_video: Default::default(), presence_rx: None, presence: Default::default(), identity: std::sync::Arc::new(super::identity::Identity::ferrocord()), ws_headers: Vec::new(), health: Default::default() }
    }

    /// Identité de la connexion et en-têtes d'ouverture du WebSocket (`net::identity`).
    pub(super) fn with_identity(mut self, identity: std::sync::Arc<super::identity::Identity>, ws_headers: Vec<(&'static str, String)>) -> Self {
        self.identity = identity;
        self.ws_headers = ws_headers;
        self
    }

    pub(super) fn with_health(mut self, health: std::sync::Arc<super::health::Health>) -> Self {
        self.health = health;
        self
    }

    /// Compte utilisateur seulement : un bot n'a pas de jeu détecté.
    pub(super) fn with_presence(mut self, rx: tokio::sync::mpsc::UnboundedReceiver<crate::state::activity::OwnPresence>) -> Self {
        if !self.bot {
            self.presence_rx = Some(rx);
        }
        self
    }

    pub(super) fn with_voice(mut self, rx: tokio::sync::mpsc::UnboundedReceiver<crate::state::voice::VoiceCmd>) -> Self {
        self.voice = Some(rx);
        self
    }

    pub(super) fn with_video(mut self, rx: tokio::sync::mpsc::UnboundedReceiver<crate::state::video::VideoCmd>) -> Self {
        self.video = Some(rx);
        self
    }

    fn offer(&mut self, s: Subscription, out: &Out) {
        if self.bot {
            out.send(Event::MemberListUnavailable);
        } else {
            self.pacer.want(s, std::time::Instant::now());
        }
    }
}

pub(super) async fn gateway_task(token: String, ep: Endpoints, out: Out, mut subs: Subs) {
    let mut st = SessionState::default();
    let mut failures: u32 = 0;
    let mut compress = ep.compress;
    let mut identifies = IdentifyLimiter::new(ep.identify_interval, ep.identify_per_day);
    let stable_after = ep.reconnect_base.saturating_mul(STABLE_FACTOR);
    let (mut refused, mut unreachable) = (0u32, 0u32);
    loop {
        out.send(Event::Status(if st.resume.is_some() { Status::Resuming } else { Status::Connecting }));
        let resuming = st.resumable();
        if !resuming {
            // Identify espacés (5 s au moins) quel que soit le chemin qui y mène.
            let now = std::time::Instant::now();
            let at = identifies.next_allowed(now);
            if identifies.daily_cap_reached(now) {
                out.send(Event::Status(Status::IdentifyCap { retry_ms: wall_clock_ms(at) }));
            }
            tokio::time::sleep_until(Instant::from_std(at)).await;
        }
        // Toute session qui n'atteint pas READY/RESUMED compte comme un échec :
        // aucun chemin (fermeture après Hello, op 7 ou op 9 répétés, heartbeats
        // non accusés) ne permet de boucle de reconnexion serrée contre Discord.
        let mut prog = Progress::default();
        let started = Instant::now();
        let result = session(&token, &ep, &mut st, compress, &mut identifies, &out, &mut prog, &mut subs).await;
        // Remise à zéro seulement après une session qui a tenu : une session
        // acceptée puis coupée aussitôt, en boucle, n'est pas un succès.
        let stable = prog.established && started.elapsed() >= stable_after;
        if stable {
            failures = 0;
        }
        // Reprise qui n'aboutit pas : ne pas la retenter sans fin.
        if prog.established {
            (refused, unreachable) = (0, 0);
        } else if resuming && prog.connected {
            refused += 1;
            if refused >= RESUMES_REFUSED_MAX {
                // Ni RESUMED ni op 9, plusieurs fois : session neuve (l'Identify
                // reste espacé par `identifies`).
                st.resume = None;
                st.seq = None;
                (refused, unreachable) = (0, 0);
            }
        } else if resuming {
            unreachable += 1;
            if unreachable >= RESUMES_UNREACHABLE_MAX
                && let Some(r) = st.resume.as_mut()
            {
                // L'adresse de reprise ne répond pas : passerelle par défaut.
                r.url = None;
            }
        }
        let mut min_wait = Duration::ZERO;
        match result {
            Ok(End::Reconnect) => {
                if !stable {
                    failures += 1;
                }
            }
            Ok(End::Slow) => {
                failures += 1;
                min_wait = ep.reconnect_base.saturating_mul(SLOW_FACTOR).min(MAX_RECONNECT_DELAY);
            }
            Ok(End::BadCompression(InflateError::TooLarge)) => {
                // Pas une corruption : la compression reste ; erreur de session
                // ordinaire, avec attente croissante.
                failures += 1;
                out.send(Event::Status(Status::PacketTooLarge));
            }
            Ok(End::BadCompression(e)) => {
                compress = false;
                failures += 1;
                out.send(Event::Status(Status::Disconnected { cause: Cause::from(e), no_compression: true }));
            }
            Ok(End::Fatal { problem, bad_token }) => {
                out.send(Event::Fatal { problem, bad_token });
                return;
            }
            Err(e) => {
                failures += 1;
                out.send(Event::Status(Status::Disconnected { cause: cause_of(e), no_compression: false }));
            }
        }
        // Gigue : jamais de nouvel essai à heure fixe (1 s, 2 s, 4 s pile).
        let wait = super::backoff::equal_jitter(reconnect_delay(ep.reconnect_base, failures), super::backoff::unit()).max(min_wait);
        tokio::time::sleep(wait).await;
    }
}

fn identify(token: &str) -> String {
    let mut d = json!({
        "token": token,
        "properties": { "os": std::env::consts::OS, "browser": "Ferrocord", "device": "Ferrocord" },
        // Compression par paquet : incompatible avec celle du transport.
        "compress": false,
        "large_threshold": 50,
    });
    // Jeton de bot : les "intents" sont obligatoires.
    // GUILDS | GUILD_EXPRESSIONS | GUILD_MESSAGES | GUILD_MESSAGE_REACTIONS | GUILD_MESSAGE_TYPING |
    // DIRECT_MESSAGES | DIRECT_MESSAGE_REACTIONS | DIRECT_MESSAGE_TYPING | MESSAGE_CONTENT
    // (aucun privilégié sauf le dernier, déjà demandé)
    if token.starts_with("Bot ") {
        d["intents"] = json!(1 | 8 | 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768);
    }
    json!({ "op": 2, "d": d }).to_string()
}

/// Adresse de connexion ; le paramètre `compress` est le seul ajout du lot F8.
fn gateway_url(base: &str, compress: bool) -> String {
    let mut url = format!("{}/?v=10&encoding=json", base.trim_end_matches('/'));
    if compress {
        url.push_str("&compress=zlib-stream");
    }
    url
}

// ====================================================================== envoi

/// Seule porte de sortie vers la passerelle : chaque envoi est compté
/// (120 par 60 s), les heartbeats ont des places réservées et passent
/// avant les commandes en attente.
struct GwSink<S> {
    sink: S,
    limiter: SendLimiter,
    queue: VecDeque<String>,
    /// Heure à laquelle la première commande en attente pourra partir.
    retry_at: Option<std::time::Instant>,
}

impl<S> GwSink<S>
where
    S: Sink<WsMsg, Error = tungstenite::Error> + Unpin,
{
    fn new(sink: S, heartbeat_interval: Duration) -> Self {
        Self { sink, limiter: SendLimiter::new(COMMANDS_PER_WINDOW, WINDOW, heartbeat_interval), queue: VecDeque::new(), retry_at: None }
    }

    async fn raw(&mut self, text: String) -> Result<(), BoxError> {
        #[cfg(debug_assertions)]
        super::shapes::record_sent(&text);
        match tokio::time::timeout(SEND_TIMEOUT, self.sink.send(WsMsg::text(text))).await {
            Ok(r) => Ok(r?),
            Err(_) => Err(Lost(Cause::Transport("envoi : délai dépassé".into())).into()),
        }
    }

    /// Fermeture propre, au mieux (une seconde) : `1000` invalide la session
    /// côté Discord, tout autre code la laisse reprendre (documentation).
    async fn close(&mut self, code: u16) {
        let frame = tungstenite::protocol::CloseFrame { code: code.into(), reason: Utf8Bytes::from_static("") };
        let _ = tokio::time::timeout(Duration::from_secs(1), self.sink.send(WsMsg::Close(Some(frame)))).await;
    }

    async fn heartbeat(&mut self, seq: Option<u64>) -> Result<(), BoxError> {
        // Les places réservées rendent l'attente théorique (un serveur qui
        // demanderait des heartbeats en rafale) ; elle évite la déconnexion.
        while let Some(at) = self.limiter.try_send(Priority::Heartbeat, std::time::Instant::now()) {
            tokio::time::sleep_until(Instant::from_std(at)).await;
        }
        self.raw(json!({ "op": 1, "d": seq }).to_string()).await
    }

    /// Met une commande en file ; `false` si la file est pleine.
    fn queue(&mut self, text: String) -> bool {
        if self.queue.len() >= MAX_QUEUED {
            return false;
        }
        self.queue.push_back(text);
        true
    }

    /// Envoie les commandes en file que le limiteur laisse passer.
    async fn flush(&mut self) -> Result<(), BoxError> {
        self.retry_at = None;
        while !self.queue.is_empty() {
            match self.limiter.try_send(Priority::Normal, std::time::Instant::now()) {
                None => {
                    if let Some(t) = self.queue.pop_front() {
                        self.raw(t).await?;
                    }
                }
                Some(at) => {
                    self.retry_at = Some(at);
                    break;
                }
            }
        }
        Ok(())
    }
}

// ====================================================================== réception

/// Contenu d'un message WebSocket à traiter.
enum Input {
    Text(Utf8Bytes),
    Compressed(Bytes),
    /// Paquet déjà décompressé, à décoder.
    Packet(Vec<u8>),
}

impl Input {
    /// Trop lourd pour la boucle (qui doit rester libre de battre) ?
    fn heavy(&self) -> bool {
        match self {
            Input::Text(t) => t.len() > OFFLOAD_PACKET,
            Input::Compressed(b) => b.len() > OFFLOAD_COMPRESSED,
            Input::Packet(_) => true,
        }
    }
}

/// Ce que la boucle doit faire après un paquet.
#[derive(Debug, PartialEq, Eq)]
enum Control {
    Nothing,
    Hello(u64),
    BadHello,
    Dispatched { established: bool },
    HeartbeatRequest,
    Ack,
    Reconnect,
    InvalidSession { resumable: bool },
    /// Paquet complet mais volumineux, resté dans le décompresseur : à décoder hors de la boucle.
    Offload,
}

/// État que le décodage modifie, déplacé tel quel vers un fil bloquant.
struct Work {
    inflater: Option<Inflater>,
    st: SessionState,
}

/// `inline` : appelé depuis la boucle (un gros paquet est alors renvoyé en `Offload`).
fn process(w: &mut Work, input: Input, inline: bool, hosts: ResumeHosts, out: &Out) -> Result<Control, InflateError> {
    match input {
        Input::Text(t) => {
            // Texte au milieu d'un paquet compressé : le flux est incohérent.
            if w.inflater.as_ref().is_some_and(Inflater::pending) {
                return Err(InflateError::Corrupt);
            }
            Ok(handle_packet(t.as_bytes(), &mut w.st, hosts, out))
        }
        Input::Compressed(b) => {
            // Binaire sans compression demandée : rien à en faire.
            let Some(inf) = w.inflater.as_mut() else { return Ok(Control::Nothing) };
            match inf.feed(&b)? {
                None => Ok(Control::Nothing),
                Some(p) if inline && p.len() > OFFLOAD_PACKET => Ok(Control::Offload),
                Some(p) => Ok(handle_packet(p, &mut w.st, hosts, out)),
            }
        }
        Input::Packet(p) => Ok(handle_packet(&p, &mut w.st, hosts, out)),
    }
}

/// Lit l'enveloppe sans construire d'arbre JSON, puis traite le paquet.
fn handle_packet(data: &[u8], st: &mut SessionState, hosts: ResumeHosts, out: &Out) -> Control {
    let Ok(text) = std::str::from_utf8(data) else { return Control::Nothing };
    let Ok(p) = serde_json::from_str::<Payload>(text) else { return Control::Nothing };
    match p.op {
        0 => {
            if p.s.is_some() {
                st.seq = p.s;
            }
            let (Some(kind), Some(d)) = (p.t, p.d) else { return Control::Nothing };
            let established = matches!(kind, "READY" | "RESUMED");
            #[cfg(debug_assertions)]
            super::shapes::record(kind, d.get());
            dispatch(kind, d.get(), hosts, &mut st.resume, &mut st.me, out);
            Control::Dispatched { established }
        }
        1 => Control::HeartbeatRequest,
        7 => Control::Reconnect,
        9 => Control::InvalidSession { resumable: p.d.map(|d| d.get()) == Some("true") },
        10 => match p.d.and_then(|d| serde_json::from_str::<Hello>(d.get()).ok()) {
            Some(h) => Control::Hello(h.heartbeat_interval),
            None => Control::BadHello,
        },
        11 => Control::Ack,
        _ => Control::Nothing,
    }
}

enum Frame {
    Input(Input),
    Close(Option<u16>),
    Ignore,
}

fn frame(msg: WsMsg) -> Frame {
    match msg {
        WsMsg::Text(t) => Frame::Input(Input::Text(t)),
        WsMsg::Binary(b) => Frame::Input(Input::Compressed(b)),
        WsMsg::Close(f) => Frame::Close(f.map(|f| u16::from(f.code))),
        _ => Frame::Ignore,
    }
}

/// Décodage déporté, et copie de l'état de session d'avant le paquet : si le
/// décodage panique, la session reprend depuis cette copie (numéro de
/// séquence d'avant : le paquet sera renvoyé par la reprise).
struct Job {
    handle: JoinHandle<(Work, Result<Control, InflateError>)>,
    backup: SessionState,
}

impl Job {
    /// Attend la fin ; en cas de panique, l'état d'avant (sans décompresseur :
    /// la connexion n'est plus utilisable) et l'erreur.
    async fn finish(self) -> JobOutcome {
        let joined = self.handle.await;
        Self::outcome(joined, self.backup)
    }

    fn outcome(joined: Result<(Work, Result<Control, InflateError>), tokio::task::JoinError>, backup: SessionState) -> JobOutcome {
        joined.map_err(|e| Box::new((Work { inflater: None, st: backup }, Cause::DecodeInterrupted(e.to_string()))))
    }
}

type JobOutcome = Result<(Work, Result<Control, InflateError>), Box<(Work, Cause)>>;

#[allow(clippy::too_many_arguments)]
async fn session(
    token: &str,
    ep: &Endpoints,
    st: &mut SessionState,
    compress: bool,
    identifies: &mut IdentifyLimiter,
    out: &Out,
    prog: &mut Progress,
    subs: &mut Subs,
) -> Result<End, BoxError> {
    let base = st.resume.as_ref().and_then(|r| r.url.clone()).unwrap_or_else(|| ep.gateway.clone());
    let url = gateway_url(&base, compress);
    // Le paquet READY d'un compte utilisateur peut peser plusieurs Mo.
    let cfg = WebSocketConfig::default()
        .max_message_size(Some(ep.max_payload))
        .max_frame_size(Some(ep.max_payload));
    let connecting = tokio_tungstenite::connect_async_with_config(super::identity::ws_request(&url, &subs.ws_headers)?, Some(cfg), false);
    let Ok(opened) = tokio::time::timeout(CONNECT_TIMEOUT, connecting).await else {
        return Err(Lost(Cause::Transport("connexion : délai dépassé".into())).into());
    };
    let (ws, _) = opened?;
    prog.connected = true;
    // Contexte de décompression neuf à chaque connexion.
    let mut work = Some(Work { inflater: compress.then(|| Inflater::new(ep.max_payload)), st: std::mem::take(st) });
    let mut job: Option<Job> = None;
    let result = run(ws, token, ep, &mut work, &mut job, identifies, out, &mut prog.established, subs).await;
    // Un décodage en cours détient l'état de la session : on le récupère.
    if let Some(j) = job.take() {
        work = Some(match j.finish().await {
            Ok((w, _)) => w,
            Err(b) => b.0,
        });
    }
    if let Some(w) = work {
        *st = w.st;
    }
    result
}

#[allow(clippy::too_many_arguments)]
async fn run<W>(
    ws: W,
    token: &str,
    ep: &Endpoints,
    work: &mut Option<Work>,
    job: &mut Option<Job>,
    identifies: &mut IdentifyLimiter,
    out: &Out,
    established: &mut bool,
    subs: &mut Subs,
) -> Result<End, BoxError>
where
    W: futures_util::Stream<Item = Result<WsMsg, tungstenite::Error>> + Sink<WsMsg, Error = tungstenite::Error> + Unpin,
{
    let (sink, mut stream) = ws.split();
    let hosts = ep.resume_hosts;
    let Some(w) = work.as_mut() else { return Err(Lost(Cause::NoSessionState).into()) };

    // 1) Hello
    let hello_by = Instant::now() + HELLO_TIMEOUT;
    let interval = loop {
        let Ok(next) = tokio::time::timeout_at(hello_by, stream.next()).await else {
            return Err(Lost(Cause::Transport("Hello : délai dépassé".into())).into());
        };
        let msg = match next {
            Some(m) => m?,
            None => return Ok(End::Reconnect),
        };
        let input = match frame(msg) {
            Frame::Input(i) => i,
            Frame::Close(code) => return Ok(close_reason(code, &mut w.st.resume, &mut w.st.seq)),
            Frame::Ignore => continue,
        };
        match process(w, input, false, hosts, out) {
            Ok(Control::Hello(iv)) => break iv,
            Ok(Control::BadHello) => return Err(Lost(Cause::BadHello).into()),
            Ok(_) => {}
            Err(e) => return Ok(End::BadCompression(e)),
        }
    };
    // Anomalie de protocole (et 0 ferait paniquer `interval_at`) : erreur de
    // session, donc attente croissante ; le jeton n'est pas envoyé.
    if interval < MIN_HEARTBEAT_MS {
        return Err(Lost(Cause::BadHeartbeat { ms: interval }).into());
    }
    let period = Duration::from_millis(interval);
    let mut gw = GwSink::new(sink, period);

    // 2) Identify ou Resume
    let identified = !w.st.resumable();
    subs.health.session(identified);
    let first = match (w.st.resume.as_ref(), w.st.seq) {
        (Some(r), Some(s)) => json!({ "op": 6, "d": { "token": token, "session_id": r.session_id, "seq": s } }).to_string(),
        _ => {
            identifies.record(std::time::Instant::now());
            // Mode bureau : paquet du module d'identité ; sinon celui d'origine.
            subs.identity.identify(token).unwrap_or_else(|| identify(token))
        }
    };
    gw.queue(first);
    gw.flush().await?;

    // 3) Boucle principale
    // Premier battement à un moment au hasard de l'intervalle (documentation :
    // « heartbeat_interval * jitter »), puis à intervalle fixe. Un battement
    // manqué n'est pas rattrapé en rafale : le rythme reprend, décalé.
    let mut hb = tokio::time::interval_at(Instant::now() + period.mul_f64(super::backoff::unit()), period);
    hb.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last_beat = (Instant::now(), std::time::SystemTime::now());
    let mut acked = true;
    let mut last_seq = w.st.seq;
    loop {
        let retry = gw.retry_at;
        let control = tokio::select! {
            _ = hb.tick() => {
                // Horloge qui a sauté (veille, reprise de l'ordinateur) : la
                // connexion est morte côté Discord, inutile d'y écrire puis
                // d'attendre un intervalle de plus. Reprise tout de suite.
                let now = (Instant::now(), std::time::SystemTime::now());
                let late = stalled(now.0.saturating_duration_since(last_beat.0), now.1.duration_since(last_beat.1).ok(), period);
                last_beat = now;
                if late {
                    gw.close(CLOSE_RESUMABLE).await;
                    return Ok(End::Reconnect);
                }
                // Accusé peut-être pas encore lu pendant un décodage : on attend la fin.
                if !acked && job.is_none() {
                    // Connexion zombie : fermée avec un code qui garde la session
                    // (ni 1000 ni 1001), puis reprise.
                    gw.close(CLOSE_RESUMABLE).await;
                    return Ok(End::Reconnect);
                }
                acked = false;
                gw.heartbeat(last_seq).await?;
                continue;
            }
            _ = tokio::time::sleep_until(Instant::from_std(retry.unwrap_or_else(std::time::Instant::now))), if retry.is_some() => {
                gw.flush().await?;
                continue;
            }
            s = subs.rx.recv(), if subs.open => {
                match s {
                    Some(s) => subs.offer(s, out),
                    None => subs.open = false,
                }
                continue;
            }
            // Vocal (phase 4) : op 4 après un geste ; avant READY/RESUMED, gardée.
            v = async {
                match subs.voice.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            }, if subs.voice.is_some() => {
                match v {
                    Some(c) => {
                        subs.gw_video.on_voice(&c);
                        if let Some(p) = subs.op4.on_cmd(&c).map(|p| subs.gw_video.op4(&p)) {
                            if *established {
                                if gw.queue(p) {
                                    gw.flush().await?;
                                }
                            } else {
                                subs.op4_pending = Some(p);
                            }
                        }
                    }
                    None => subs.voice = None,
                }
                continue;
            }
            // Vidéo (phase 5) : caméra et directs après un geste. Avant
            // READY/RESUMED, l'op 4 est gardée ; les directs, abandonnés
            // (la session neuve n'en a aucun, le magasin non plus).
            v = async {
                match subs.video.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            }, if subs.video.is_some() => {
                match v {
                    Some(c) => {
                        let camera = matches!(c, crate::state::video::VideoCmd::SetCamera(_));
                        let current = subs.op4.current();
                        let packets = subs.gw_video.on_cmd(&c, current);
                        if *established {
                            let mut sent = false;
                            for p in packets {
                                sent |= gw.queue(p);
                            }
                            if sent {
                                gw.flush().await?;
                            }
                        } else if camera {
                            subs.op4_pending = packets.into_iter().last();
                        }
                    }
                    None => subs.video = None,
                }
                continue;
            }
            // Ma présence (lot C3) : gardée, envoyée par le bras suivant.
            p = async {
                match subs.presence_rx.as_mut() {
                    Some(rx) => rx.recv().await,
                    None => std::future::pending().await,
                }
            }, if subs.presence_rx.is_some() => {
                match p {
                    Some(p) => subs.presence.want(p),
                    None => subs.presence_rx = None,
                }
                continue;
            }
            _ = tokio::time::sleep_until(Instant::from_std(subs.presence.due().unwrap_or_else(std::time::Instant::now))), if *established && subs.presence.due().is_some() => {
                if let Some(p) = subs.presence.take(std::time::Instant::now())
                    && gw.queue(p)
                {
                    gw.flush().await?;
                }
                continue;
            }
            // Souscription à la liste des membres, au rythme du `Pacer`, après READY/RESUMED.
            _ = tokio::time::sleep_until(Instant::from_std(subs.pacer.due().unwrap_or_else(std::time::Instant::now))), if *established && subs.pacer.due().is_some() => {
                let now = std::time::Instant::now();
                if subs.pacer.payload(now).is_some_and(|p| gw.queue(p)) {
                    subs.pacer.sent(now);
                    gw.flush().await?;
                } else {
                    subs.pacer.later(now);
                }
                continue;
            }
            done = async {
                match job.as_mut() {
                    Some(j) => (&mut j.handle).await,
                    None => std::future::pending().await,
                }
            }, if job.is_some() => {
                let backup = job.take().map(|j| j.backup).unwrap_or_default();
                match Job::outcome(done, backup) {
                    Ok((w, res)) => {
                        last_seq = w.st.seq;
                        *work = Some(w);
                        res
                    }
                    Err(b) => {
                        let (w, e) = *b;
                        // État d'avant le paquet, cohérent : reconnexion et reprise.
                        *work = Some(w);
                        return Err(Lost(e).into());
                    }
                }
            }
            msg = stream.next(), if job.is_none() => {
                let msg = match msg {
                    Some(m) => m?,
                    None => return Ok(End::Reconnect),
                };
                let Some(w) = work.as_mut() else { return Err(Lost(Cause::NoSessionState).into()) };
                let input = match frame(msg) {
                    Frame::Input(i) => i,
                    Frame::Close(code) => return Ok(close_reason(code, &mut w.st.resume, &mut w.st.seq)),
                    Frame::Ignore => continue,
                };
                if input.heavy() {
                    spawn_job(work, job, input, hosts, out);
                    continue;
                }
                let res = process(w, input, true, hosts, out);
                last_seq = w.st.seq;
                if res == Ok(Control::Offload) {
                    let packet = w.inflater.as_mut().map(Inflater::take_packet).unwrap_or_default();
                    spawn_job(work, job, Input::Packet(packet), hosts, out);
                    continue;
                }
                res
            }
        };
        let Some(w) = work.as_mut() else { return Err(Lost(Cause::NoSessionState).into()) };
        match control {
            Err(e) => return Ok(End::BadCompression(e)),
            Ok(Control::Dispatched { established: true }) => {
                // Nouvelle session : le serveur n'a plus aucune souscription.
                if identified && !*established {
                    subs.pacer.reset(std::time::Instant::now());
                    // Vocal (phase 4) : Discord a oublié mon salon ; le READY le retire aussi du magasin.
                    subs.op4.forget_channel();
                    subs.op4_pending = None;
                    // Vidéo (phase 5) : caméra et directs oubliés aussi.
                    subs.gw_video.forget();
                    // Présence (lot C3) : la session neuve ne la connaît pas.
                    subs.presence.new_session();
                }
                if !*established
                    && let Some(p) = subs.op4_pending.take()
                    && gw.queue(p)
                {
                    gw.flush().await?;
                }
                *established = true;
            }
            Ok(Control::HeartbeatRequest) => gw.heartbeat(last_seq).await?,
            Ok(Control::Ack) => acked = true,
            Ok(Control::Reconnect) => {
                gw.close(CLOSE_RESUMABLE).await;
                return Ok(End::Reconnect);
            }
            Ok(Control::InvalidSession { resumable }) => {
                if !resumable {
                    w.st.resume = None;
                    w.st.seq = None;
                }
                // Documentation de Discord (Gateway, Invalid Session) : attendre
                // une durée au hasard entre 1 et 5 s avant de s'identifier de nouveau.
                let wait = super::backoff::between(ep.invalid_session_wait, ep.invalid_session_wait.saturating_mul(INVALID_SESSION_SPREAD), super::backoff::unit());
                tokio::time::sleep(wait).await;
                gw.close(if resumable { CLOSE_RESUMABLE } else { CLOSE_NORMAL }).await;
                return Ok(End::Reconnect);
            }
            Ok(_) => {}
        }
    }
}

/// Décode `input` sur un fil bloquant, avec l'état de la session.
fn spawn_job(work: &mut Option<Work>, job: &mut Option<Job>, input: Input, hosts: ResumeHosts, out: &Out) {
    let Some(mut w) = work.take() else { return };
    let backup = w.st.clone();
    let out = out.clone();
    let handle = tokio::task::spawn_blocking(move || {
        let res = process(&mut w, input, false, hosts, &out);
        (w, res)
    });
    *job = Some(Job { handle, backup });
}

/// Code de fermeture envoyé quand la session doit pouvoir être reprise
/// (« any close code besides 1000 or 1001 », documentation).
const CLOSE_RESUMABLE: u16 = 4000;
/// Fermeture normale : Discord oublie la session.
const CLOSE_NORMAL: u16 = 1000;
/// Attente après une session invalidée : de `invalid_session_wait` à cinq
/// fois cette durée (1 à 5 s en production).
const INVALID_SESSION_SPREAD: u32 = 5;

/// Que faire d'un code de fermeture reçu (documentation de Discord, « Gateway
/// Close Event Codes » ; 4015 : docs.discord.food, « Opcodes and Close
/// Codes », lue le 15 novembre 2026).
fn close_reason(code: Option<u16>, resume: &mut Option<Resume>, seq: &mut Option<u64>) -> End {
    match code.unwrap_or(0) {
        // Jeton refusé : arrêt définitif, jamais de nouvel essai.
        4004 => End::Fatal { problem: Problem::BadToken, bad_token: true },
        c @ (4010..=4014) => End::Fatal { problem: Problem::Refused { code: c }, bad_token: false },
        // Séquence invalide, session expirée : session neuve.
        4007 | 4009 => {
            *resume = None;
            *seq = None;
            End::Reconnect
        }
        // Débit dépassé, trop de sessions : la session reste, mais on attend
        // une fenêtre entière avant de revenir.
        4008 | 4015 => End::Slow,
        // 1000, 1001, 4000 à 4003, 4005, sans code, inconnu : reprise tentée
        // (si Discord a oublié la session, il répond op 9 et l'Identify suit,
        // espacé par le limiteur).
        _ => End::Reconnect,
    }
}

/// `me` : mon identifiant, écrit à READY et lu par GUILD_CREATE.
pub(super) fn dispatch(kind: &str, d: &str, hosts: ResumeHosts, resume: &mut Option<Resume>, me: &mut Option<u64>, out: &Out) {
    // Journal de performance : réveils attribués à ce paquet (rien s'il est inactif).
    let _source = crate::perf::wakes::Source::new(kind);
    // CHANNEL_UPDATE d'un serveur qui n'est pas à l'écran, visibilité du salon
    // inchangée : tous les événements du paquet attendent la prochaine image.
    let _calm = (kind == "CHANNEL_UPDATE")
        .then(|| serde_json::from_str::<RawChannelMark>(d).ok())
        .flatten()
        .and_then(|c| out.interest.calm_channel_update(c.guild_id?, c.id, c.mark()).then(|| out.calm()));
    // READY, GUILD_CREATE : serveurs découpés une fois pour tous les décodeurs
    // du paquet (lot « long ») ; rien pour les autres paquets.
    let shared = super::raw::Shared::of(kind, d);
    // Réactions, épingles, emojis (lots T5/T6) : décodés à part.
    super::social::dispatch(kind, d, out);
    // Fils et forums (lot T10).
    super::threads::dispatch(kind, d, *me, out);
    // Votes des sondages, drapeaux d'annonce (lot T13).
    super::polls::dispatch(kind, d, out);
    // Paramètres du serveur, webhooks, intégrations (lot A8 + A9).
    super::admin::dispatch(kind, d, out);
    match kind {
        "READY" => {
            let mut r: RawReady = match serde_json::from_str(d) {
                Ok(r) => r,
                Err(e) => return out.send(Event::Error(Problem::ReadyUnreadable(e.to_string()))),
            };
            super::people::ready(&r, &out.people);
            let url = hosts.check(&r.resume_gateway_url);
            // Les interactions (lot T11) portent la session qui en recevra les événements.
            out.send(Event::Interactions(crate::state::interactions::InteractionEvent::Session(crate::state::interactions::SessionId(r.session_id.clone()))));
            *resume = Some(Resume { session_id: r.session_id, url });
            *me = Some(r.user.id);
            let account = super::account::from_ready(r.user_settings_proto.take(), r.user_guild_settings.take());
            // `merged_members` n'est lu que si son alignement sur `guilds`
            // est vraisemblable : sinon l'appariement serait une supposition.
            let mut merged = align_merged(&r.guilds, r.merged_members).into_iter();
            let users: HashMap<u64, (String, Avatar)> = r.users.iter().map(|u| (u.id, (u.display(), u.avatar()))).collect();
            let order: HashMap<u64, usize> = r
                .user_settings
                .unwrap_or_default()
                .guild_positions
                .iter()
                .enumerate()
                .filter_map(|(i, id)| Some((id.parse().ok()?, i)))
                .collect();
            let me_id = Some(r.user.id);
            let mut emojis = Vec::new();
            let mut threads = Vec::new();
            let mut stickers = Vec::new();
            let mut roles = Vec::new();
            let mut boosts = Vec::new();
            let mut guilds: Vec<Guild> = r
                .guilds
                .into_iter()
                .filter_map(|mut g| {
                    let mine = merged.next().flatten();
                    super::social::take_emojis(&mut g, &mut emojis);
                    super::threads::take(&mut g, &mut threads);
                    super::stickers::take(&mut g, &mut stickers);
                    super::admin::roles::take(&g, &mut roles);
                    super::admin::boost(&g, &mut boosts);
                    g.into_guild(me_id, mine)
                })
                .collect();
            guilds.sort_by_key(|g| order.get(&g.id).copied().unwrap_or(usize::MAX));
            let groups: Vec<_> = r.private_channels.iter().filter_map(|c| c.group_recipients(&users)).collect();
            let mut dms: Vec<Dm> = r.private_channels.into_iter().filter_map(|c| c.into_dm(&users)).collect();
            dms.sort_by_key(|d| std::cmp::Reverse(d.last_message));
            out.send(Event::Ready { me: User { id: r.user.id, name: r.user.display(), avatar: r.user.avatar() }, guilds, dms });
            for (channel_id, users) in groups {
                out.send(Event::GroupRecipients { channel_id, users });
            }
            threads.into_iter().for_each(|e| out.send(e));
            for e in account {
                out.send(e);
            }
            super::relations::ready(d, out);
            out.send(Event::Status(Status::Connected));
            super::social::send_emojis(emojis, out);
            super::stickers::send(stickers, out);
            roles.into_iter().for_each(|e| out.send(e));
            super::admin::send_boosts(boosts, out);
        }
        "RESUMED" => out.send(Event::Status(Status::Connected)),
        "GUILD_CREATE" => {
            let mut emojis = Vec::new();
            let mut threads = Vec::new();
            let mut stickers = Vec::new();
            let mut roles = Vec::new();
            let mut boosts = Vec::new();
            if let Some(g) = shared.guild().and_then(|g| g.read::<RawGuild>().ok()).and_then(|mut g| {
                super::social::take_emojis(&mut g, &mut emojis);
                super::threads::take(&mut g, &mut threads);
                super::stickers::take(&mut g, &mut stickers);
                super::admin::roles::take(&g, &mut roles);
                super::admin::boost(&g, &mut boosts);
                super::people::guild(&g, &out.people);
                g.into_guild(*me, None)
            }) {
                out.send(Event::GuildCreate(g));
                threads.into_iter().for_each(|e| out.send(e));
                super::social::send_emojis(emojis, out);
                super::stickers::send(stickers, out);
                roles.into_iter().for_each(|e| out.send(e));
                super::admin::send_boosts(boosts, out);
            }
        }
        "GUILD_UPDATE" => {
            if let Ok(g) = serde_json::from_str::<RawGuildUpdate>(d) {
                out.send(Event::GuildUpdate { id: g.id, owner_id: g.owner_id(), icon: g.icon() });
            }
        }
        "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
            if let Ok(r) = serde_json::from_str::<RawRoleEvent>(d) {
                let guild_id = r.guild_id;
                match r.role.0 {
                    Some(raw) => {
                        let role_id = raw.id;
                        match raw.into_role() {
                            Some(role) => out.send(Event::RoleUpsert { guild_id, role }),
                            // Permissions illisibles : l'ancienne version du rôle ne
                            // vaut plus, il est retiré et les permissions des membres
                            // qui le portent deviennent inconnues.
                            None => out.send(Event::RoleUnreadable { guild_id, role_id }),
                        }
                    }
                    // Rôle illisible jusqu'à son identifiant : on ne sait plus
                    // lequel a changé, les rôles du serveur deviennent inconnus.
                    None => out.send(Event::GuildRolesUnreadable { guild_id }),
                }
            }
        }
        "GUILD_ROLE_DELETE" => {
            if let Ok(r) = serde_json::from_str::<RawRoleDelete>(d) {
                out.send(match r.role_id {
                    Some(role_id) => Event::RoleDelete { guild_id: r.guild_id, role_id },
                    None => Event::GuildRolesUnreadable { guild_id: r.guild_id },
                });
            }
        }
        "GUILD_MEMBER_UPDATE" => {
            // Tout membre va aux profils ; seul le mien va au magasin (permissions).
            let m = serde_json::from_str::<RawGuildMember>(d);
            if let Ok(m) = &m {
                super::people::member_update(m, out);
            }
            if let Ok(m) = m
                && let (Some(guild_id), Some(user_id)) = (m.guild_id, m.user_id())
                && me.is_none_or(|me| me == user_id)
            {
                out.send(Event::MemberUpdate { guild_id, user_id, member: m.to_member() });
            }
        }
        "GUILD_DELETE" => {
            if let Ok(g) = serde_json::from_str::<RawIdChannel>(d)
                && !g.unavailable
            {
                out.send(Event::GuildDelete(g.id));
            }
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            if let Ok(c) = serde_json::from_str::<RawChannel>(d) {
                match c.guild_id {
                    Some(guild_id) => {
                        if let Some(channel) = c.into_channel() {
                            out.send(Event::ChannelUpsert { guild_id, channel });
                        }
                        // Détails pour « Modifier le salon » (lot A2).
                        if let Some(e) = super::admin::channels::gateway(d) {
                            out.send(e);
                        }
                    }
                    None => {
                        if let Some((user_id, channel_id)) = c.dm_peer() {
                            out.people.lock().note_dm(user_id, channel_id);
                        }
                        let group = c.group_recipients(&HashMap::new());
                        if let Some((dm, known)) = c.into_dm_update() {
                            out.send(Event::DmUpsert { dm, known });
                        }
                        if let Some((channel_id, users)) = group {
                            out.send(Event::GroupRecipients { channel_id, users });
                        }
                        if let Some(e) = super::relations::private_channel(d) {
                            out.send(Event::Relations(e));
                        }
                    }
                }
            }
        }
        "CHANNEL_DELETE" => {
            if let Ok(c) = serde_json::from_str::<RawIdChannel>(d) {
                out.send(Event::ChannelDelete { guild_id: c.guild_id, id: c.id });
            }
        }
        "MESSAGE_CREATE" => {
            if let Ok(m) = serde_json::from_str::<RawMessage>(d) {
                super::people::message(&m, &out.people);
                let confirmed = m.own_nonce(*me);
                if let Some(e) = m.thread_event() {
                    out.send(e);
                }
                out.send(Event::MessageCreate(m.into_message()));
                // Mon message, avec le nonce d'un envoi resté sans réponse : il est bien parti.
                if let Some((channel_id, nonce)) = confirmed {
                    out.send(Event::SendConfirmed { channel_id, nonce });
                }
            }
        }
        "MESSAGE_UPDATE" => {
            if let Ok(m) = serde_json::from_str::<RawMessageUpdate>(d) {
                let (channel_id, id) = (m.channel_id, m.id);
                if let Some(content) = m.content {
                    out.send(Event::MessageUpdate { channel_id, id, content });
                }
                let attachments = m.attachments.map(super::media::attachments);
                let embeds = m.embeds.map(super::media::embeds);
                if attachments.is_some() || embeds.is_some() {
                    out.send(Event::MessageMedia { channel_id, id, attachments, embeds });
                }
            }
        }
        "MESSAGE_DELETE" => {
            if let Ok(m) = serde_json::from_str::<RawIdChannel>(d)
                && let Some(channel_id) = m.channel_id
            {
                out.send(Event::MessageDelete { channel_id, id: m.id });
            }
        }
        "GUILD_STICKERS_UPDATE" => super::stickers::update(d, out),
        "GUILD_EMOJIS_UPDATE" => {
            if let Ok(u) = serde_json::from_str::<RawEmojisUpdate>(d) {
                out.send(Event::GuildEmojis { guild_id: u.guild_id, emojis: into_emojis(u.emojis) });
            }
        }
        "TYPING_START" => {
            // Le mien n'est pas affiché.
            if let Ok(t) = serde_json::from_str::<RawTyping>(d)
                && me.is_none_or(|me| me != t.user_id)
            {
                out.send(Event::TypingStart { channel_id: t.channel_id, user_id: t.user_id, name: t.name() });
            }
        }
        // Présences : magasin des profils (`people`) et page Amis (`relations`).
        "PRESENCE_UPDATE" => {
            super::people::presence_update(d, out);
            super::relations::dispatch(kind, d, out);
        }
        "GUILD_MEMBER_LIST_UPDATE" => {
            if let Some(u) = super::members::decode_into(d, &out.people) {
                out.send_list(u);
            }
        }
        "READY_SUPPLEMENTAL" => {
            super::people::supplemental(d, out);
            super::relations::dispatch(kind, d, out);
        }
        // Mon compte modifié, ici ou sur un autre appareil : nom et avatar suivent
        // (sans cela, ils restaient ceux du READY jusqu'à la reconnexion).
        "USER_UPDATE" => {
            if let Ok(u) = serde_json::from_str::<super::raw::RawUser>(d)
                && me.is_none_or(|me| me == u.id)
                && let Some(info) = super::people::user_info(&u)
            {
                out.people.lock().upsert_user(info);
                out.send(Event::MeUpdate(User { id: u.id, name: u.display(), avatar: u.avatar() }));
            }
        }
        "USER_NOTE_UPDATE" => super::profile::note_update(d, out),
        "USER_CONNECTIONS_UPDATE" => out.send(Event::Account(crate::state::usersettings::AccountEvent::ConnectionsChanged)),
        "FRIEND_SUGGESTION_CREATE" | "FRIEND_SUGGESTION_DELETE" => super::suggestions::dispatch(kind, d, out),
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" | "RELATIONSHIP_REMOVE" | "MESSAGE_ACK" | "CHANNEL_RECIPIENT_ADD"
        | "CHANNEL_RECIPIENT_REMOVE" => super::relations::dispatch(kind, d, out),
        // Réglages du compte ; le reste est ignoré sans décodage.
        _ => {
            super::account::dispatch(kind, d, out);
            super::gifs::dispatch(kind, d, out);
            super::quests::dispatch(kind, d, out);
        }
    }
    // Commandes et composants (lot T11), après MESSAGE_UPDATE : une réponse
    // différée résolue ne compte pas comme une modification.
    super::interactions::dispatch(kind, d, out);
    // Modération (A4, A5), événements et fonctions du serveur (A7), après
    // READY et GUILD_CREATE.
    super::admin::dispatch_after(kind, d, &shared, out);
    // Vocal des serveurs (phase 4), après READY et GUILD_CREATE.
    super::voicegw::dispatch(kind, d, &shared, out);
    // Appels, états vocaux, conférences, sons joués (lot V6), après READY.
    super::calls::dispatch(kind, d, &shared, out);
    // Directs (phase 5), après READY.
    super::streamgw::dispatch(kind, d, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        mpsc, Arc,
    };

    /// `Out` relié à un canal local ; le compteur mesure les réveils.
    fn out() -> (Out, mpsc::Receiver<Event>, Arc<AtomicUsize>) {
        let (tx, rx) = mpsc::channel();
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        (Out { tx, wake: Arc::new(move || { w.fetch_add(1, Ordering::SeqCst); }), people: Default::default(), watch: Default::default(), interest: Default::default() }, rx, wakes)
    }

    fn resumable() -> (Option<Resume>, Option<u64>) {
        (Some(Resume { session_id: "s".into(), url: Some("wss://r".into()) }), Some(42))
    }

    // ------------------------------------------------------------ décodage déporté

    #[tokio::test]
    async fn panicking_decode_restores_the_previous_session() {
        let backup = SessionState { resume: Some(Resume { session_id: "s".into(), url: None }), seq: Some(41), me: Some(1) };
        let handle = tokio::task::spawn_blocking(|| -> (Work, Result<Control, InflateError>) { panic!("décodeur fictif") });
        let Err(b) = (Job { handle, backup }).finish().await else { panic!("échec attendu") };
        let (w, e) = *b;
        // Reprise possible au numéro d'avant : Discord renverra le paquet.
        assert!(w.st.resumable() && w.st.seq == Some(41) && w.st.me == Some(1));
        assert!(w.inflater.is_none());
        assert!(matches!(&e, Cause::DecodeInterrupted(d) if d.contains("panic")), "{e:?}");
    }

    #[test]
    fn heavy_inputs_are_decoded_off_the_loop() {
        assert!(Input::Text(Utf8Bytes::from("x".repeat(OFFLOAD_PACKET + 1))).heavy());
        assert!(!Input::Text(Utf8Bytes::from("{}")).heavy());
        assert!(Input::Compressed(Bytes::from(vec![0u8; OFFLOAD_COMPRESSED + 1])).heavy());
        assert!(!Input::Compressed(Bytes::from(vec![0u8; 16])).heavy());
    }

    // ------------------------------------------------------------ reconnect_delay

    #[test]
    fn reconnect_delay_doubles_and_caps_at_60s() {
        let s = Duration::from_secs;
        let base = Endpoints::default().reconnect_base;
        assert_eq!(base, s(1), "valeur de production inchangée");
        let delays: Vec<Duration> = (0..9).map(|f| reconnect_delay(base, f)).collect();
        assert_eq!(delays, vec![s(1), s(2), s(4), s(8), s(16), s(32), s(60), s(60), s(60)]);
        assert_eq!(reconnect_delay(base, u32::MAX), s(60));
        assert_eq!(reconnect_delay(Duration::MAX, 3), s(60));
        // Session invalidée : de 1 à 5 s, au hasard (documentation de Discord).
        assert_eq!(Endpoints::default().invalid_session_wait, s(1));
        assert_eq!(INVALID_SESSION_SPREAD, 5);
        // La gigue ne dépasse jamais le plafond et ne descend pas sous la moitié.
        for u in [0.0, 0.3, 0.999_999] {
            let d = crate::net::backoff::equal_jitter(reconnect_delay(base, 9), u);
            assert!(d >= s(30) && d <= s(60), "{d:?}");
        }
    }

    #[test]
    fn a_clock_jump_is_told_from_an_ordinary_beat() {
        let (s, hb) = (Duration::from_secs, Duration::from_millis(41_250));
        // Battement à l'heure, ou en retard de quelques secondes (machine chargée).
        assert!(!stalled(hb, Some(hb), hb));
        assert!(!stalled(hb + s(15), Some(hb + s(15)), hb));
        // Veille : l'une des deux horloges a avancé de bien plus qu'un intervalle.
        assert!(stalled(hb, Some(s(3600)), hb), "horloge monotone arrêtée pendant la veille");
        assert!(stalled(s(3600), Some(s(3600)), hb));
        assert!(stalled(s(3600), None, hb), "horloge murale reculée : la monotone suffit");
        // Horloge murale reculée (réglage de l'heure) : pas un saut.
        assert!(!stalled(hb, None, hb));
        // Intervalle très court (tests) : la marge de 10 s évite les fausses alertes.
        assert!(!stalled(s(5), Some(s(5)), Duration::from_millis(50)));
    }

    // ------------------------------------------------------------ close_reason

    #[test]
    fn close_4004_is_fatal_bad_token() {
        let (mut r, mut s) = resumable();
        assert!(matches!(close_reason(Some(4004), &mut r, &mut s), End::Fatal { bad_token: true, .. }));
    }

    #[test]
    fn close_4010_to_4014_is_fatal_without_bad_token() {
        for code in 4010..=4014 {
            let (mut r, mut s) = resumable();
            match close_reason(Some(code), &mut r, &mut s) {
                End::Fatal { problem, bad_token } => {
                    assert!(!bad_token);
                    assert_eq!(problem, Problem::Refused { code });
                }
                End::Reconnect | End::Slow | End::BadCompression(_) => panic!("{code} devrait être fatal"),
            }
        }
    }

    #[test]
    fn close_4008_and_4015_slow_down_but_keep_the_session() {
        for code in [4008, 4015] {
            let (mut r, mut s) = resumable();
            assert!(matches!(close_reason(Some(code), &mut r, &mut s), End::Slow), "{code}");
            assert!(r.is_some() && s == Some(42), "{code} : la reprise doit être gardée");
        }
    }

    #[test]
    fn close_4007_4009_reconnect_without_resume() {
        for code in [4007, 4009] {
            let (mut r, mut s) = resumable();
            assert!(matches!(close_reason(Some(code), &mut r, &mut s), End::Reconnect));
            assert!(r.is_none(), "{code} : la reprise doit être abandonnée");
            assert_eq!(s, None);
        }
    }

    #[test]
    fn close_other_or_none_reconnect_keeping_resume() {
        for code in [Some(1000), Some(1001), Some(1006), Some(4000), Some(4001), Some(4002), Some(4003), Some(4005), Some(4999), None] {
            let (mut r, mut s) = resumable();
            assert!(matches!(close_reason(code, &mut r, &mut s), End::Reconnect));
            assert!(r.is_some(), "{code:?} : la reprise doit être gardée");
            assert_eq!(s, Some(42));
        }
    }

    // ------------------------------------------------------------ identify

    fn parse(s: &str) -> Value {
        serde_json::from_str(s).unwrap_or_else(|e| panic!("JSON produit invalide : {e}"))
    }

    #[test]
    fn identify_user_token_has_no_intents() {
        let v = parse(&identify("faux-jeton"));
        assert_eq!(v["op"], 2);
        assert_eq!(v["d"]["token"], "faux-jeton");
        assert!(v["d"].get("intents").is_none());
        assert_eq!(v["d"]["compress"], false);
    }

    #[test]
    fn identify_bot_token_has_intents() {
        let v = parse(&identify("Bot faux-jeton"));
        assert_eq!(v["op"], 2);
        assert_eq!(v["d"]["token"], "Bot faux-jeton");
        assert_eq!(v["d"]["intents"], 1 | 8 | 512 | 1024 | 2048 | 4096 | 8192 | 16384 | 32768);
    }

    // ------------------------------------------------------------ dispatch

    const READY: &str = r#"{
        "user": { "id": "1", "username": "moi", "global_name": "Moi" },
        "session_id": "sess-1",
        "resume_gateway_url": "wss://reprise.discord.gg",
        "guilds": [
            { "id": "10", "name": "A" },
            { "id": "20", "name": "B" },
            { "id": "30", "properties": { "name": "C" } },
            { "id": "40", "unavailable": true },
            { "id": "50", "name": "E" }
        ],
        "user_settings": { "guild_positions": ["30", "40", "10", "pas-un-id"] },
        "private_channels": [
            { "id": "100", "type": 1, "last_message_id": "500", "recipient_ids": ["7"] },
            { "id": "101", "type": 3, "last_message_id": "900", "recipient_ids": ["7", "8"] },
            { "id": "102", "type": 1, "recipient_ids": ["9"] },
            { "id": "103", "type": 0 }
        ],
        "users": [
            { "id": "7", "username": "bob" },
            { "id": "8", "username": "eve", "global_name": "Eve" }
        ],
        "presences": [ { "ignored": true } ]
    }"#;

    #[test]
    fn dispatch_ready() {
        let (out, rx, wakes) = out();
        let mut resume = None;
        dispatch("READY", READY, ResumeHosts::Discord, &mut resume, &mut None, &out);

        let r = resume.unwrap_or_else(|| panic!("reprise attendue"));
        assert_eq!((r.session_id.as_str(), r.url.as_deref()), ("sess-1", Some("wss://reprise.discord.gg/")));

        let all: Vec<Event> = rx.try_iter().collect();
        assert_eq!(wakes.load(Ordering::SeqCst), all.len());
        // Session des interactions (lot T11), avant le READY.
        assert!(matches!(&all[0], Event::Interactions(crate::state::interactions::InteractionEvent::Session(s)) if s.0 == "sess-1"), "{:?}", all[0]);
        let all: Vec<Event> = all.into_iter().filter(|e| !matches!(e, Event::Threads(_) | Event::Interactions(_) | Event::GuildEvents(_) | Event::Community(_) | Event::Voice(_) | Event::Stream(_))).collect();
        // Destinataires du MP de groupe (liste des membres) : à part.
        let (groups, events): (Vec<Event>, Vec<Event>) = all.into_iter().partition(|e| matches!(e, Event::GroupRecipients { .. }));
        assert!(matches!(groups.as_slice(), [Event::GroupRecipients { channel_id: 101, users }] if users.len() == 2), "{groups:?}");
        assert_eq!(events.len(), 2);
        let Event::Ready { me, guilds, dms } = &events[0] else { panic!("READY attendu, reçu {:?}", events[0]) };
        assert_eq!((me.id, me.name.as_str()), (1, "Moi"));

        // Ordre du compte d'abord, puis les absents dans l'ordre reçu ; 40 indisponible est écarté.
        let gids: Vec<u64> = guilds.iter().map(|g| g.id).collect();
        assert_eq!(gids, vec![30, 10, 20, 50]);
        assert_eq!(guilds[0].name, "C");

        let d: Vec<(u64, &str, u64)> = dms.iter().map(|d| (d.id, d.name.as_str(), d.last_message)).collect();
        assert_eq!(d, vec![(101, "bob, Eve", 900), (100, "bob", 500), (102, "", 102)]);

        assert!(matches!(&events[1], Event::Status(Status::Connected)));
    }

    /// Événements du compte venus d'un autre appareil : mon nom et mon avatar,
    /// une note, les comptes liés. Aucun n'était lu avant.
    #[test]
    fn account_events_from_another_device_are_followed() {
        let (out, rx, _) = out();
        let mut me = Some(1);
        let mut send = |kind: &str, d: &str| dispatch(kind, d, ResumeHosts::Discord, &mut None, &mut me, &out);
        send("USER_UPDATE", r#"{ "id": "1", "username": "moi_fictif", "global_name": "Moi Nouveau", "avatar": null }"#);
        // Un autre que moi, ou un paquet sans nom : rien.
        send("USER_UPDATE", r#"{ "id": "2", "username": "autre_fictif" }"#);
        send("USER_UPDATE", r#"{ "id": "1" }"#);
        send("USER_CONNECTIONS_UPDATE", "{}");
        send("USER_NOTE_UPDATE", r#"{ "id": "5", "note": "vue ailleurs" }"#);
        send("USER_NOTE_UPDATE", r#"{ "note": "sans personne" }"#);
        let events: Vec<Event> = rx.try_iter().collect();
        assert!(
            matches!(events.as_slice(), [Event::MeUpdate(u), Event::Account(crate::state::usersettings::AccountEvent::ConnectionsChanged)] if u.id == 1 && u.name == "Moi Nouveau"),
            "{events:?}"
        );
        let people = out.people.lock();
        assert_eq!(people.user(1).and_then(|u| u.global_name.clone()).as_deref(), Some("Moi Nouveau"));
        assert_eq!(people.profiles().note(5).and_then(|n| n.shown()), Some("vue ailleurs"));
    }

    #[test]
    fn dispatch_invalid_ready_emits_error() {
        let (out, rx, _) = out();
        let mut resume = None;
        dispatch("READY", r#"{ "user": { "id": "1" } }"#, ResumeHosts::Discord, &mut resume, &mut None, &out);
        dispatch("READY", "pas du json", ResumeHosts::Discord, &mut resume, &mut None, &out);
        let events: Vec<Event> = rx.try_iter().collect();
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|e| matches!(e, Event::Error(Problem::ReadyUnreadable(_)))));
        assert!(resume.is_none());
    }

    #[test]
    fn dispatch_unknown_or_bad_events_emit_nothing() {
        let (out, rx, wakes) = out();
        let mut resume = None;
        dispatch("PRESENCE_UPDATE", "{ pas même du json", ResumeHosts::Discord, &mut resume, &mut None, &out);
        dispatch("TYPING_START", r#"{ "channel_id": "1" }"#, ResumeHosts::Discord, &mut resume, &mut None, &out);
        dispatch("MESSAGE_CREATE", r#"{ "id": "x" }"#, ResumeHosts::Discord, &mut resume, &mut None, &out);
        dispatch("GUILD_DELETE", r#"{ "id": "5", "unavailable": true }"#, ResumeHosts::Discord, &mut resume, &mut None, &out);
        dispatch("MESSAGE_UPDATE", r#"{ "id": "5", "channel_id": "6" }"#, ResumeHosts::Discord, &mut resume, &mut None, &out);
        dispatch("MESSAGE_DELETE", r#"{ "id": "5" }"#, ResumeHosts::Discord, &mut resume, &mut None, &out);
        assert_eq!(rx.try_iter().count(), 0);
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
        assert!(resume.is_none());
    }

    // ------------------------------------------------------------ réveils (lot « perf »)

    /// MESSAGE_UPDATE complet, comme Discord l'envoie quand il ajoute l'aperçu d'un lien.
    fn full_update(channel: u64, id: u64) -> String {
        format!(r#"{{ "id": "{id}", "channel_id": "{channel}", "guild_id": "7", "content": "https://exemple.invalid/a", "pinned": false, "flags": 0, "attachments": [], "embeds": [{{ "type": "link", "title": "Aperçu" }}], "edited_timestamp": null }}"#)
    }

    /// Mesure du 15 novembre 2026 : chaque MESSAGE_UPDATE d'un salon qui n'est
    /// pas à l'écran réveillait trois fois (épinglage, drapeaux, composants).
    #[test]
    fn a_message_update_elsewhere_never_wakes() {
        let (out, rx, wakes) = out();
        out.interest.show(Some(10), Some(1));
        out.interest.show_messages(Some((100, 200)));
        let (mut resume, mut me) = (None, Some(1));
        let send = |kind: &str, d: &str, resume: &mut Option<Resume>, me: &mut Option<u64>| dispatch(kind, d, ResumeHosts::Discord, resume, me, &out);
        // Autre salon, puis salon ouvert hors de l'écran : cinq événements chacun, aucun réveil.
        send("MESSAGE_UPDATE", &full_update(30, 5), &mut resume, &mut me);
        send("MESSAGE_UPDATE", &full_update(10, 50), &mut resume, &mut me);
        send("MESSAGE_DELETE", r#"{ "id": "5", "channel_id": "30", "guild_id": "7" }"#, &mut resume, &mut me);
        assert_eq!(rx.try_iter().count(), 11, "l'état est mis à jour à la prochaine image");
        assert_eq!(wakes.load(Ordering::SeqCst), 0);
        // Message affiché dans le salon ouvert : réveil aussitôt.
        send("MESSAGE_UPDATE", &full_update(10, 150), &mut resume, &mut me);
        assert_eq!(rx.try_iter().count(), 5);
        assert_eq!(wakes.swap(0, Ordering::SeqCst), 5);
        send("MESSAGE_DELETE", r#"{ "id": "150", "channel_id": "10", "guild_id": "7" }"#, &mut resume, &mut me);
        assert_eq!(wakes.swap(0, Ordering::SeqCst), 1);
    }

    #[test]
    fn a_channel_update_of_a_hidden_guild_waits() {
        let (out, rx, wakes) = out();
        out.interest.show(Some(10), Some(1));
        out.interest.show_guilds(&[7]);
        let (mut resume, mut me) = (None, Some(1));
        let update = |guild: u64, name: &str, deny: &str| format!(r#"{{ "id": "40", "guild_id": "{guild}", "type": 0, "name": "{name}", "position": 1, "parent_id": null, "topic": "sujet", "permission_overwrites": [{{ "id": "{guild}", "type": 0, "allow": "0", "deny": "{deny}" }}] }}"#);
        let mut send = |d: &str| {
            dispatch("CHANNEL_UPDATE", d, ResumeHosts::Discord, &mut resume, &mut me, &out);
            (rx.try_iter().count(), wakes.swap(0, Ordering::SeqCst))
        };
        // Serveur 9, pas à l'écran. Premier paquet du salon : réveil (visibilité inconnue).
        let (n, w) = send(&update(9, "a", "0"));
        assert!(n >= 2 && w == n, "{n} événements, {w} réveils");
        // Renommé, même visibilité : tous les événements attendent.
        assert_eq!(send(&update(9, "b", "0")), (n, 0));
        // Permissions changées (le salon peut apparaître ou disparaître) : réveil.
        assert_eq!(send(&update(9, "b", "1024")), (n, n));
        assert_eq!(send(&update(9, "c", "1024")), (n, 0));
        // Le gardien du paquet calme ne déborde pas sur le paquet suivant.
        dispatch("MESSAGE_DELETE", r#"{ "id": "150", "channel_id": "10" }"#, ResumeHosts::Discord, &mut None, &mut None, &out);
        assert_eq!((rx.try_iter().count(), wakes.swap(0, Ordering::SeqCst)), (1, 1));
    }

    // ------------------------------------------------------------ permissions

    fn run(packets: &[(&str, &str)]) -> (Vec<Event>, Option<u64>) {
        let (out, rx, _) = out();
        let (mut resume, mut me) = (None, None);
        for (kind, d) in packets {
            dispatch(kind, d, ResumeHosts::Discord, &mut resume, &mut me, &out);
        }
        // Fils et forums (lot T10) : vérifiés par `net::threads::tests`.
        (rx.try_iter().filter(|e| !matches!(e, Event::Threads(_) | Event::Interactions(_) | Event::Voice(_))).collect(), me)
    }

    const READY_MERGED: &str = r#"{
        "user": { "id": "1", "username": "moi" },
        "session_id": "s", "resume_gateway_url": "wss://gateway.discord.gg",
        "guilds": [
            { "id": "10", "properties": { "name": "A", "owner_id": "2" }, "roles": [{ "id": "10", "permissions": "1024" }] },
            { "id": "20", "unavailable": true },
            { "id": "30", "properties": { "name": "C", "owner_id": "2" }, "roles": [{ "id": "30", "permissions": "0" }, { "id": "99", "permissions": "8" }] }
        ],
        "merged_members": [ [{ "user_id": "1", "roles": [] }], [], [{ "user_id": "1", "roles": ["99"] }] ]
    }"#;

    fn ready_guilds(events: &[Event]) -> Vec<Guild> {
        match events.first() {
            Some(Event::Ready { guilds, .. }) => guilds.clone(),
            other => panic!("READY attendu : {other:?}"),
        }
    }

    #[test]
    fn ready_merged_members_follow_guild_order() {
        let (events, me) = run(&[("READY", READY_MERGED)]);
        assert_eq!(me, Some(1));
        let guilds = ready_guilds(&events);
        // Le serveur indisponible garde sa place dans `merged_members`.
        let mine: Vec<(u64, Option<Vec<u64>>)> = guilds.iter().map(|g| (g.id, g.me.as_ref().map(|m| m.roles.clone()))).collect();
        assert_eq!(mine, vec![(10, Some(Vec::new())), (30, Some(vec![99]))]);
        assert_eq!(guilds[0].owner_id, Some(2));
    }

    #[test]
    fn ready_merged_members_of_wrong_length_are_ignored() {
        let ready = READY_MERGED.replace(r#"[{ "user_id": "1", "roles": [] }], [], "#, "");
        let (events, _) = run(&[("READY", &ready)]);
        assert!(ready_guilds(&events).iter().all(|g| g.me.is_none()));
    }

    #[test]
    fn guild_create_after_ready_finds_my_member() {
        let create = r#"{ "id": "40", "name": "D", "owner_id": "2", "roles": [{ "id": "40", "permissions": "3072" }],
            "members": [{ "user": { "id": "1", "username": "moi" }, "roles": [] }] }"#;
        let (events, _) = run(&[("READY", READY_MERGED), ("GUILD_CREATE", create)]);
        let Some(Event::GuildCreate(g)) = events.iter().find(|e| matches!(e, Event::GuildCreate(_))) else { panic!("GUILD_CREATE attendu") };
        assert_eq!(g.me.as_ref().map(|m| m.roles.len()), Some(0));
        // Sans READY préalable, mon membre ne peut pas être reconnu.
        let (events, _) = run(&[("GUILD_CREATE", create)]);
        assert!(matches!(&events[0], Event::GuildCreate(g) if g.me.is_none()));
    }

    #[test]
    fn role_member_and_guild_events() {
        let (events, _) = run(&[
            ("READY", READY_MERGED),
            ("GUILD_ROLE_CREATE", r#"{ "guild_id": "10", "role": { "id": "11", "name": "R", "permissions": "8", "position": 1, "color": 255 } }"#),
            ("GUILD_ROLE_UPDATE", r#"{ "guild_id": "10", "role": { "id": "12", "name": "R", "permissions": "illisible" } }"#),
            ("GUILD_ROLE_DELETE", r#"{ "guild_id": "10", "role_id": "11" }"#),
            ("GUILD_MEMBER_UPDATE", r#"{ "guild_id": "10", "user": { "id": "1" }, "roles": ["11"], "nick": null }"#),
            // Un autre membre : pas transmis.
            ("GUILD_MEMBER_UPDATE", r#"{ "guild_id": "10", "user": { "id": "3" }, "roles": [] }"#),
            ("GUILD_MEMBER_UPDATE", r#"{ "guild_id": "10", "user": { "id": "1" }, "roles": [], "communication_disabled_until": "?" }"#),
            ("GUILD_UPDATE", r#"{ "id": "10", "name": "A", "owner_id": "1" }"#),
            ("GUILD_UPDATE", r#"{ "id": "10", "properties": { "owner_id": "2" } }"#),
            ("GUILD_ROLE_DELETE", r#"{ "guild_id": "10" }"#),
            ("GUILD_ROLE_UPDATE", r#"{ "guild_id": "10", "role": { "id": 12, "permissions": "8" } }"#),
            ("GUILD_UPDATE", r#"{ "id": "10", "owner_id": 1 }"#),
            ("GUILD_ROLE_UPDATE", r#"{ "role": { "id": "12", "permissions": "8" } }"#),
        ]);
        // Administration (lots A8 + A9, A3, A7) : testée dans `net::admin`.
        let events: Vec<Event> = events.into_iter().filter(|e| !matches!(e, Event::GuildAdmin(_) | Event::Roles(_) | Event::GuildEvents(_) | Event::Community(_) | Event::Stream(_))).collect();
        let rest: Vec<String> = events.iter().skip(2).map(|e| format!("{e:?}")).collect();
        assert_eq!(rest.len(), 10, "{rest:#?}");
        assert!(matches!(&events[2], Event::RoleUpsert { guild_id: 10, role } if role.id == 11 && role.color == 255 && role.permissions.0 == 8));
        assert!(matches!(&events[3], Event::RoleUnreadable { guild_id: 10, role_id: 12 }));
        assert!(matches!(&events[4], Event::RoleDelete { guild_id: 10, role_id: 11 }));
        assert!(matches!(&events[5], Event::MemberUpdate { guild_id: 10, user_id: 1, member: Some(m) } if m.roles == vec![11]));
        assert!(matches!(&events[6], Event::MemberUpdate { user_id: 1, member: None, .. }));
        assert!(matches!(&events[7], Event::GuildUpdate { id: 10, owner_id: Some(Some(1)), .. }));
        assert!(matches!(&events[8], Event::GuildUpdate { id: 10, owner_id: Some(Some(2)), .. }));
        // Suppression ou rôle de forme inattendue : rôles du serveur inconnus.
        assert!(matches!(&events[9], Event::GuildRolesUnreadable { guild_id: 10 }));
        assert!(matches!(&events[10], Event::GuildRolesUnreadable { guild_id: 10 }));
        // Propriétaire illisible : « inconnu », pas « absent ».
        assert!(matches!(&events[11], Event::GuildUpdate { id: 10, owner_id: Some(None), .. }));
    }

    /// READY minimal : `guilds` et `merged_members` donnés en JSON.
    /// Lot « ram » : un serveur décodé ne garde pas la réserve de sa forme
    /// brute (les tableaux étaient convertis sur place : un salon brut est
    /// trois fois plus gros qu'un salon, et les salons vocaux sont écartés).
    #[test]
    fn ready_guilds_keep_no_spare_capacity() {
        let list = |n: usize, one: &dyn Fn(usize) -> String| (0..n).map(one).collect::<Vec<_>>().join(",");
        let channels = list(40, &|i| format!(r#"{{ "id": "{}", "type": {}, "name": "salon-{i}", "permission_overwrites": [{{ "id": "10", "type": 0, "allow": "0", "deny": "1024" }}] }}"#, 100 + i, if i % 4 == 0 { 2 } else { 0 }));
        let roles = list(20, &|i| format!(r#"{{ "id": "{}", "name": "rôle {i}", "permissions": "1024" }}"#, 10 + i));
        let emojis = list(33, &|i| format!(r#"{{ "id": "{}", "name": "emoji_{i}", "roles": ["11"] }}"#, 500 + i));
        let guilds = format!(r#"[{{ "id": "10", "properties": {{ "name": "A" }}, "channels": [{channels}], "roles": [{roles}], "emojis": [{emojis}] }}]"#);
        let g = &ready_with(&guilds, r#"[[{ "user_id": "1", "roles": [] }]]"#)[0];
        assert_eq!((g.channels.len(), g.roles.len(), g.emojis.len()), (30, 20, 33));
        assert_eq!((g.channels.capacity(), g.roles.capacity(), g.emojis.capacity()), (30, 20, 33));
        assert!(g.channels.iter().all(|c| c.name.capacity() == c.name.len() && c.overwrites.as_ref().is_some_and(|o| o.capacity() == o.len())));
        assert!(g.roles.iter().all(|r| r.name.capacity() == r.name.len()));
        assert!(g.emojis.iter().all(|e| e.name.capacity() == e.name.len() && e.roles.capacity() == e.roles.len()));
    }

    fn ready_with(guilds: &str, merged: &str) -> Vec<Guild> {
        let ready = format!(
            r#"{{ "user": {{ "id": "1", "username": "moi" }}, "session_id": "s", "resume_gateway_url": "wss://gateway.discord.gg",
                 "guilds": {guilds}, "merged_members": {merged} }}"#
        );
        let (events, _) = run(&[("READY", &ready)]);
        ready_guilds(&events)
    }

    fn my_roles(guilds: &[Guild]) -> Vec<(u64, Option<Vec<u64>>)> {
        guilds.iter().map(|g| (g.id, g.me.as_ref().map(|m| m.roles.clone()))).collect()
    }

    const TWO_GUILDS: &str = r#"[
        { "id": "10", "properties": { "owner_id": "2" }, "roles": [{ "id": "10", "permissions": "1024" }, { "id": "11", "permissions": "0" }] },
        { "id": "20", "properties": { "owner_id": "2" }, "roles": [{ "id": "20", "permissions": "1024" }, { "id": "21", "permissions": "0" }] }
    ]"#;

    #[test]
    fn merged_members_misaligned_are_ignored_entirely() {
        // Entrées permutées : le rôle 21 n'existe pas dans le serveur 10.
        let g = ready_with(TWO_GUILDS, r#"[ [{ "user_id": "1", "roles": ["21"] }], [{ "user_id": "1", "roles": [] }] ]"#);
        assert_eq!(my_roles(&g), vec![(10, None), (20, None)]);
        // Mauvais `guild_id` sur une entrée, même sans rôle.
        let g = ready_with(TWO_GUILDS, r#"[ [{ "user_id": "1", "guild_id": "20", "roles": [] }], [{ "user_id": "1", "roles": ["21"] }] ]"#);
        assert_eq!(my_roles(&g), vec![(10, None), (20, None)]);
        // Alignement confirmé par des rôles : le membre sans rôle est cru.
        let g = ready_with(TWO_GUILDS, r#"[ [{ "user_id": "1", "roles": [] }], [{ "user_id": "1", "roles": ["21"] }] ]"#);
        assert_eq!(my_roles(&g), vec![(10, Some(vec![])), (20, Some(vec![21]))]);
        // … ou par un `guild_id` correct.
        let g = ready_with(TWO_GUILDS, r#"[ [{ "user_id": "1", "guild_id": "10", "roles": [] }], [{ "user_id": "1", "roles": [] }] ]"#);
        assert_eq!(my_roles(&g), vec![(10, Some(vec![])), (20, Some(vec![]))]);
    }

    #[test]
    fn merged_members_without_any_hint_leave_roleless_unknown() {
        // Aucun `guild_id`, aucun rôle : rien ne prouve l'alignement.
        let g = ready_with(TWO_GUILDS, r#"[ [{ "user_id": "1", "roles": [] }], [{ "user_id": "1", "roles": [] }] ]"#);
        assert_eq!(my_roles(&g), vec![(10, None), (20, None)]);
    }

    #[test]
    fn damaged_ready_still_connects() {
        // Types inattendus, champs manquants, null : le READY aboutit, et
        // chaque élément abîmé rend inconnu ce qui en dépend.
        let guilds = r#"[
            { "id": "10", "properties": { "owner_id": "2" },
              "roles": [{ "id": "10", "permissions": "1024" }, { "id": 11, "permissions": ["x"] }, { "permissions": "8" }, null],
              "channels": [
                { "id": "100", "type": 0, "name": "a", "permission_overwrites": [{ "id": "10", "type": "rôle", "allow": "0", "deny": "1024" }] },
                { "id": "101", "type": 0, "name": "b", "permission_overwrites": [null, { "id": "10", "type": 0, "allow": {}, "deny": "1024" }] },
                { "id": "102", "type": 0, "name": "c", "permission_overwrites": "x" },
                { "id": "103", "type": 0, "name": "d", "permission_overwrites": [{ "id": "10", "type": 0, "allow": "0", "deny": "1024" }] }
              ] },
            { "id": "20", "properties": { "owner_id": 2, "name": 5 }, "roles": "x" },
            { "id": "30", "properties": null, "roles": [{ "id": "30", "permissions": "1024" }],
              "members": [{ "user": { "id": "1" }, "roles": [30], "nick": 3 }, 7] },
            { "id": "40", "properties": { "owner_id": "2" }, "roles": [{ "id": "40", "permissions": "1024", "name": null, "color": "rouge", "position": null }] }
        ]"#;
        let merged = r#"[ [{ "user_id": "1", "roles": [] }], [{ "user_id": "1", "roles": null }], [], [{ "user_id": "1", "guild_id": "40", "roles": [], "communication_disabled_until": 12 }] ]"#;
        let g = ready_with(guilds, merged);
        assert_eq!(g.len(), 4);
        // Serveur 10 : des rôles illisibles, donc rôles inconnus.
        assert!(g[0].roles_unreadable);
        assert_eq!(g[0].roles.len(), 1);
        let ows: Vec<Option<usize>> = g[0].channels.iter().map(|c| c.overwrites.as_ref().map(Vec::len)).collect();
        // Dérogation de type inconnu, élément null, liste illisible : salon
        // inconnu ; la dérogation lisible est gardée.
        assert_eq!(ows, vec![None, None, None, Some(1)]);
        // Serveur 20 : liste de rôles illisible, membre aux rôles null.
        assert!(g[1].roles_unreadable && g[1].roles.is_empty() && g[1].me.is_none());
        assert_eq!(g[1].owner_id, Some(2));
        // Serveur 30 : rôles en nombres au lieu de chaînes, membre inconnu.
        assert!(g[2].me.is_none() && !g[2].roles_unreadable);
        // Serveur 40 : champs secondaires du rôle tolérés, exclusion illisible
        // (membre inconnu).
        assert!(!g[3].roles_unreadable && g[3].roles[0].color == 0);
        assert!(g[3].me.is_none());
        // `merged_members` d'une forme inattendue : READY quand même.
        assert!(ready_with("[]", r#"{ "a": 1 }"#).is_empty());
    }
}
