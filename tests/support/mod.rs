//! Faux serveur Discord local pour les tests de bout en bout : passerelle
//! WebSocket en clair et mini serveur HTTP/1.1, tous deux sur 127.0.0.1 avec
//! un port attribué par le système. Rien ne sort de la machine.
//!
//! Le `Client` relie le vrai `net::connect` au vrai `state::Store` comme le
//! fait l'interface : chaque événement est appliqué au magasin, puis les
//! commandes que le magasin a accumulées sont transmises au réseau.

#![allow(dead_code)]

use ferrocord::net::{self, cdn::CdnHosts, Connection, Endpoints, ResumeHosts};
use ferrocord::state::{Cmd, Effect, Event, Status, Store};
use flate2::{Compress, Compression, FlushCompress};
use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
    sync::{mpsc, Notify},
    time::Instant,
};
use tokio_tungstenite::{
    tungstenite::{
        handshake::server::{Request as WsRequest, Response as WsResponse},
        protocol::{frame::coding::CloseCode, CloseFrame, WebSocketConfig},
        Message as WsMsg,
    },
    WebSocketStream,
};

/// Faux serveur vocal (lot V1).
pub mod voice;
/// Faux périphérique audio (lot V7).
pub mod audio;
/// Fausse source vidéo et faux codecs (lot M5).
pub mod video;

/// Délai maximum pour toute attente : un test bloqué échoue au lieu de pendre.
pub const WAIT: Duration = Duration::from_secs(5);

/// Jeton utilisateur factice (évidemment faux).
pub const USER_TOKEN: &str = "faux.jeton.utilisateur-DE-TEST";
/// Jeton de bot factice.
pub const BOT_TOKEN: &str = "Bot faux.jeton.bot-DE-TEST";

pub async fn within<T>(what: &str, fut: impl Future<Output = T>) -> T {
    match tokio::time::timeout(WAIT, fut).await {
        Ok(v) => v,
        Err(_) => panic!("délai dépassé ({WAIT:?}) : {what}"),
    }
}

/// Lit `tests/fixtures/<name>.json`.
pub fn fixture(name: &str) -> Value {
    let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : JSON invalide : {e}"))
}

/// Délais courts pour que la suite soit rapide (les valeurs de production
/// restent celles d'`Endpoints::default()`).
pub fn endpoints(gw: &FakeGateway, http: &FakeHttp) -> Endpoints {
    Endpoints {
        gateway: gw.url("/gw"),
        api: http.api(),
        reconnect_base: Duration::from_millis(10),
        invalid_session_wait: Duration::from_millis(10),
        resume_hosts: ResumeHosts::Loopback,
        cdn: http.cdn(),
        cdn_hosts: CdnHosts::Loopback,
        compress: true,
        max_payload: net::MAX_PAYLOAD,
        identify_interval: Duration::from_millis(10),
        identify_per_day: net::IDENTIFY_PER_DAY,
        identity: Default::default(),
    }
}

// ====================================================================== passerelle

pub struct FakeGateway {
    port: u16,
    conns: mpsc::UnboundedReceiver<GwConn>,
}

impl FakeGateway {
    // La signature du rappel de poignée de main est imposée par tungstenite.
    #[allow(clippy::result_large_err)]
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|e| panic!("bind passerelle : {e}"));
        let port = listener.local_addr().map(|a| a.port()).unwrap_or_else(|e| panic!("adresse : {e}"));
        let (tx, conns) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                let tx = tx.clone();
                tokio::spawn(async move {
                    let path = Arc::new(Mutex::new(String::new()));
                    let p = path.clone();
                    let seen = Arc::new(Mutex::new(Vec::new()));
                    let h = seen.clone();
                    let cb = move |req: &WsRequest, resp: WsResponse| {
                        if let Ok(mut p) = p.lock() {
                            *p = req.uri().to_string();
                        }
                        if let Ok(mut h) = h.lock() {
                            *h = req.headers().iter().map(|(k, v)| (k.as_str().to_ascii_lowercase(), v.to_str().unwrap_or_default().to_string())).collect();
                        }
                        Ok(resp)
                    };
                    let cfg = WebSocketConfig::default().max_message_size(None).max_frame_size(None);
                    if let Ok(ws) = tokio_tungstenite::accept_hdr_async_with_config(tcp, cb, Some(cfg)).await {
                        let path = path.lock().map(|p| p.clone()).unwrap_or_default();
                        // Comme Discord : compression du transport si le client la demande.
                        let deflate = path.contains("compress=zlib-stream").then(|| Compress::new(Compression::default(), true));
                        let headers = seen.lock().map(|h| h.clone()).unwrap_or_default();
                        let _ = tx.send(GwConn { ws, path, headers, auto_ack: true, deflate, fragments: 1, bytes_sent: 0 });
                    }
                });
            }
        });
        Self { port, conns }
    }

    /// Adresse `ws://` du faux serveur ; `path` distingue la passerelle
    /// normale (`/gw`) de l'adresse de reprise (`/resume`).
    pub fn url(&self, path: &str) -> String {
        format!("ws://127.0.0.1:{}{path}", self.port)
    }

    /// Attend la prochaine connexion du client.
    pub async fn accept(&mut self) -> GwConn {
        within("connexion du client à la passerelle", self.conns.recv())
            .await
            .unwrap_or_else(|| panic!("passerelle arrêtée"))
    }

    /// Connexion déjà arrivée, sans attendre.
    pub fn try_accept(&mut self) -> Option<GwConn> {
        self.conns.try_recv().ok()
    }
}

/// Une connexion WebSocket du client, pilotée par le test.
pub struct GwConn {
    ws: WebSocketStream<TcpStream>,
    /// Chemin et requête demandés par le client (`/gw/?v=10&encoding=json`).
    pub path: String,
    /// En-têtes de la demande d'ouverture (noms en minuscules).
    pub headers: Vec<(String, String)>,
    /// Accuser automatiquement les heartbeats reçus pendant `recv_op`.
    pub auto_ack: bool,
    /// Contexte zlib de la connexion (`compress=zlib-stream` demandé).
    deflate: Option<Compress>,
    /// Nombre de messages WebSocket sur lesquels chaque paquet compressé est réparti.
    pub fragments: usize,
    /// Octets envoyés au client (charge utile WebSocket).
    pub bytes_sent: usize,
}

/// Compresse `data` dans le flux zlib `z`, terminé par un vidage synchrone
/// (`00 00 FF FF`), comme le fait Discord.
pub fn deflate_packet(z: &mut Compress, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() / 4 + 64);
    let mut input = data;
    loop {
        if out.len() == out.capacity() {
            out.reserve(out.capacity().max(64));
        }
        let before = z.total_in();
        z.compress_vec(input, &mut out, FlushCompress::Sync).unwrap_or_else(|e| panic!("compression : {e}"));
        input = &input[(z.total_in() - before) as usize..];
        if input.is_empty() && out.len() < out.capacity() {
            return out;
        }
    }
}

impl GwConn {
    pub async fn send(&mut self, v: Value) {
        self.send_text(&v.to_string()).await;
    }

    /// Envoie un paquet déjà sérialisé (compressé si la connexion l'est).
    pub async fn send_text(&mut self, text: &str) {
        match self.deflate.as_mut() {
            Some(z) => {
                let c = deflate_packet(z, text.as_bytes());
                let n = self.fragments.max(1);
                let size = c.len().div_ceil(n).max(1);
                for part in c.chunks(size) {
                    self.send_binary(part.to_vec()).await;
                }
            }
            None => {
                self.bytes_sent += text.len();
                self.ws.send(WsMsg::text(text)).await.unwrap_or_else(|e| panic!("envoi passerelle : {e}"));
            }
        }
    }

    /// Compresse `text` tout de suite (le contexte avance) pour l'envoyer
    /// plus tard sans délai : un gros paquet peut être préparé avant Hello.
    pub fn prepare(&mut self, text: &str) -> Vec<u8> {
        let z = self.deflate.as_mut().unwrap_or_else(|| panic!("connexion non compressée"));
        deflate_packet(z, text.as_bytes())
    }

    /// Message binaire brut (morceau compressé, ou données corrompues).
    pub async fn send_binary(&mut self, data: Vec<u8>) {
        self.bytes_sent += data.len();
        self.ws.send(WsMsg::binary(data)).await.unwrap_or_else(|e| panic!("envoi passerelle : {e}"));
    }

    /// La connexion est-elle compressée ?
    pub fn compressed(&self) -> bool {
        self.deflate.is_some()
    }

    /// Les paquets suivants partent en texte, sans compression.
    pub fn stop_compressing(&mut self) {
        self.deflate = None;
    }

    /// Paquet compressé dont quelques octets sont abîmés en route.
    pub async fn send_corrupt(&mut self, v: Value) {
        let z = self.deflate.as_mut().unwrap_or_else(|| panic!("connexion non compressée"));
        let mut c = deflate_packet(z, v.to_string().as_bytes());
        let mid = c.len() / 2;
        for b in c.iter_mut().skip(mid.saturating_sub(4)).take(8) {
            *b ^= 0x5A;
        }
        self.send_binary(c).await;
    }

    pub async fn hello(&mut self, heartbeat_interval: u64) {
        self.send(json!({ "op": 10, "d": { "heartbeat_interval": heartbeat_interval }, "s": null, "t": null })).await;
    }

    pub async fn dispatch(&mut self, t: &str, s: u64, d: Value) {
        self.send(json!({ "op": 0, "t": t, "s": s, "d": d })).await;
    }

    /// Envoie un paquet de `tests/fixtures/` avec le numéro de séquence `s`.
    pub async fn dispatch_fixture(&mut self, name: &str, s: u64) {
        let mut p = fixture(name);
        p["s"] = json!(s);
        self.send(p).await;
    }

    pub async fn heartbeat_ack(&mut self) {
        self.send(json!({ "op": 11 })).await;
    }

    /// op 1 envoyé par le serveur : le client doit répondre tout de suite.
    pub async fn request_heartbeat(&mut self) {
        self.send(json!({ "op": 1, "d": null })).await;
    }

    pub async fn reconnect(&mut self) {
        self.send(json!({ "op": 7, "d": null })).await;
    }

    pub async fn invalid_session(&mut self, resumable: bool) {
        self.send(json!({ "op": 9, "d": resumable })).await;
    }

    pub async fn close(&mut self, code: u16) {
        let frame = CloseFrame { code: CloseCode::from(code), reason: "".into() };
        let _ = self.ws.send(WsMsg::Close(Some(frame))).await;
    }

    /// Coupe la connexion TCP sans fermeture WebSocket.
    pub fn drop_abruptly(self) {
        drop(self.ws);
    }

    /// Prochain paquet JSON du client.
    pub async fn recv(&mut self) -> Value {
        let ws = &mut self.ws;
        within("paquet du client", async {
            loop {
                match ws.next().await {
                    Some(Ok(WsMsg::Text(t))) => {
                        return serde_json::from_str(t.as_str()).unwrap_or_else(|e| panic!("JSON client invalide : {e}"));
                    }
                    Some(Ok(WsMsg::Close(f))) => panic!("le client a fermé la connexion : {f:?}"),
                    Some(Ok(_)) => {}
                    Some(Err(e)) => panic!("erreur passerelle : {e}"),
                    None => panic!("le client a coupé la connexion"),
                }
            }
        })
        .await
    }

    /// Prochain paquet d'opcode `op` ; les heartbeats intercalés sont
    /// accusés si `auto_ack`, puis ignorés.
    pub async fn recv_op(&mut self, op: u64) -> Value {
        loop {
            let p = self.recv().await;
            if p["op"] == op {
                return p;
            }
            if p["op"] == 1 {
                if self.auto_ack {
                    self.heartbeat_ack().await;
                }
                continue;
            }
            panic!("op {op} attendu, reçu {p}");
        }
    }

    /// Hello puis Identify ; renvoie le `d` de l'Identify.
    pub async fn hello_identify(&mut self, heartbeat_interval: u64) -> Value {
        self.hello(heartbeat_interval).await;
        self.recv_op(2).await["d"].take()
    }

    /// Hello puis Resume ; renvoie le `d` du Resume.
    pub async fn hello_resume(&mut self, heartbeat_interval: u64) -> Value {
        self.hello(heartbeat_interval).await;
        self.recv_op(6).await["d"].take()
    }

    /// Attend que le client abandonne la connexion sans avoir rien envoyé.
    pub async fn expect_closed_silently(&mut self) {
        let ws = &mut self.ws;
        within("fermeture silencieuse par le client", async {
            loop {
                match ws.next().await {
                    Some(Ok(WsMsg::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(WsMsg::Text(t))) => panic!("le client n'aurait rien dû envoyer : {t}"),
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
    }

    /// Attend que le client abandonne la connexion (fermeture ou coupure).
    pub async fn expect_closed(&mut self) {
        let ws = &mut self.ws;
        within("fermeture par le client", async {
            loop {
                match ws.next().await {
                    Some(Ok(WsMsg::Close(_))) | Some(Err(_)) | None => return,
                    Some(Ok(_)) => {}
                }
            }
        })
        .await
    }
}

/// Paquet READY de `fixture` dont l'adresse de reprise pointe vers le faux serveur.
pub fn ready_with_resume(gw: &FakeGateway, fixture_name: &str) -> Value {
    let mut p = fixture(fixture_name);
    p["d"]["resume_gateway_url"] = json!(gw.url("/resume"));
    p
}

/// READY de `ready_user_classic` grossi à `guilds` serveurs (copies du
/// premier, identifiants uniques) : plusieurs Mo pour quelques milliers.
pub fn big_ready(gw: &FakeGateway, guilds: usize) -> Value {
    let mut p = ready_with_resume(gw, "ready_user_classic");
    let template = p["d"]["guilds"][0].clone();
    let list: Vec<Value> = (0..guilds as u64)
        .map(|i| {
            let mut g = template.clone();
            g["id"] = json!((2100000000000000000 + i).to_string());
            g["name"] = json!(format!("Serveur fictif n° {i}"));
            if let Some(chans) = g["channels"].as_array_mut() {
                for (k, c) in chans.iter_mut().enumerate() {
                    c["id"] = json!((3100000000000000000 + i * 100 + k as u64).to_string());
                    c["topic"] = json!(format!("Sujet fictif {i}-{k}, un peu de texte pour ressembler à un vrai salon"));
                }
            }
            g
        })
        .collect();
    p["d"]["guilds"] = json!(list);
    p["d"]["user_settings"] = json!({});
    p
}

// ====================================================================== HTTP

/// Requête reçue par le faux serveur HTTP.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    /// Chemin avec la requête (`/api/v10/channels/1/messages?limit=50`).
    pub path: String,
    pub authorization: Option<String>,
    /// Tous les en-têtes reçus (noms en minuscules), dans l'ordre.
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub received_at: Instant,
    /// Requêtes reçues mais pas encore répondues à l'arrivée de celle-ci.
    pub in_flight: usize,
}

impl Request {
    pub fn json(&self) -> Value {
        serde_json::from_str(&self.body).unwrap_or_else(|e| panic!("corps JSON invalide ({e}) : {}", self.body))
    }
}

#[derive(Clone, Debug)]
pub struct Response {
    pub status: u16,
    pub body: String,
    pub delay: Duration,
    /// En-tête `Date` (absent par défaut).
    pub date: Option<String>,
    /// Corps binaire (remplace `body`) et son type.
    pub bytes: Option<(Vec<u8>, String)>,
    /// En-têtes ajoutés tels quels (`location`…).
    pub extra: Vec<(String, String)>,
    /// Corps de cette taille envoyé sans `Content-Length`, puis connexion fermée.
    pub stream: Option<usize>,
    /// Connexion fermée au milieu du corps (la requête est arrivée, la
    /// réponse est perdue).
    pub cut: bool,
}

impl Response {
    /// 429 du pare-feu : page HTML, sans en-tête de limite, avec `Retry-After`.
    pub fn firewall(retry_after_secs: u64) -> Self {
        Self { body: "<html>error code: 1015</html>".into(), ..Self::raw(429, Value::Null) }.header("retry-after", &retry_after_secs.to_string())
    }

    /// Réponse coupée en plein corps.
    pub fn cut(self) -> Self {
        Self { cut: true, ..self }
    }

    pub fn ok(body: Value) -> Self {
        Self::raw(200, body)
    }

    /// Erreur au format de l'API (`{"message", "code"}`).
    pub fn error(status: u16, message: &str) -> Self {
        Self::raw(status, json!({ "message": message, "code": 0 }))
    }

    pub fn rate_limited(retry_after: f64) -> Self {
        Self::raw(429, json!({ "message": "You are being rate limited.", "retry_after": retry_after, "global": false }))
    }

    /// Réponse quelconque (corps JSON arbitraire, éventuellement malformé).
    pub fn raw(status: u16, body: Value) -> Self {
        Self { status, body: body.to_string(), delay: Duration::ZERO, date: None, bytes: None, extra: Vec::new(), stream: None, cut: false }
    }

    /// Image (ou tout autre corps binaire) servie avec ce type.
    pub fn bytes(status: u16, body: Vec<u8>, content_type: &str) -> Self {
        Self { bytes: Some((body, content_type.into())), ..Self::raw(status, Value::Null) }
    }

    /// Ajoute un en-tête.
    pub fn header(mut self, name: &str, value: &str) -> Self {
        self.extra.push((name.into(), value.into()));
        self
    }

    /// `len` octets envoyés sans `Content-Length` (fin = fermeture de la connexion).
    pub fn streamed(len: usize) -> Self {
        Self { stream: Some(len), ..Self::raw(200, Value::Null) }
    }

    /// En-têtes de limite de débit d'un compartiment (`reset_after` en secondes).
    pub fn bucket(self, hash: &str, limit: u32, remaining: u32, reset_after: f64) -> Self {
        self.header("x-ratelimit-bucket", hash)
            .header("x-ratelimit-limit", &limit.to_string())
            .header("x-ratelimit-remaining", &remaining.to_string())
            .header("x-ratelimit-reset-after", &reset_after.to_string())
    }

    /// 429 de la limite globale.
    pub fn global_limited(retry_after: f64) -> Self {
        Self::raw(429, json!({ "message": "You are being rate limited.", "retry_after": retry_after, "global": true }))
            .header("x-ratelimit-global", "true")
            .header("x-ratelimit-scope", "global")
    }

    /// Ajoute l'en-tête `Date` (heure du serveur).
    pub fn dated(mut self, date: &str) -> Self {
        self.date = Some(date.into());
        self
    }

    /// Répond après `d` (simule un serveur lent, pas une synchronisation).
    pub fn delayed(mut self, d: Duration) -> Self {
        self.delay = d;
        self
    }
}

struct Route {
    method: String,
    path: String,
    responses: VecDeque<Response>,
}

#[derive(Default)]
struct HttpState {
    routes: Mutex<Vec<Route>>,
    log: Mutex<Vec<Request>>,
    in_flight: AtomicUsize,
}

impl HttpState {
    /// Réponse de la route correspondante ; la dernière réponse d'une route
    /// est répétée indéfiniment.
    fn respond(&self, method: &str, path: &str) -> Response {
        let path = path.split('?').next().unwrap_or(path);
        let Ok(mut routes) = self.routes.lock() else { return Response::error(500, "verrou") };
        match routes.iter_mut().find(|r| r.method == method && r.path == path) {
            Some(r) if r.responses.len() > 1 => r.responses.pop_front().unwrap_or_else(|| Response::error(500, "vide")),
            Some(r) => r.responses.front().cloned().unwrap_or_else(|| Response::error(500, "vide")),
            None => Response::error(404, "Unknown route"),
        }
    }
}

pub struct FakeHttp {
    port: u16,
    state: Arc<HttpState>,
    rx: mpsc::UnboundedReceiver<Request>,
}

impl FakeHttp {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap_or_else(|e| panic!("bind HTTP : {e}"));
        let port = listener.local_addr().map(|a| a.port()).unwrap_or_else(|e| panic!("adresse : {e}"));
        let state = Arc::new(HttpState::default());
        let (tx, rx) = mpsc::unbounded_channel();
        let st = state.clone();
        tokio::spawn(async move {
            while let Ok((tcp, _)) = listener.accept().await {
                tokio::spawn(serve_http(tcp, st.clone(), tx.clone()));
            }
        });
        Self { port, state, rx }
    }

    /// Racine de l'API telle que la verrait le client (`…/api/v10`).
    pub fn api(&self) -> String {
        format!("http://127.0.0.1:{}/api/v10", self.port)
    }

    /// Racine du faux CDN (`http://127.0.0.1:port/cdn`).
    pub fn cdn(&self) -> String {
        format!("http://127.0.0.1:{}/cdn", self.port)
    }

    /// Programme les réponses à `method path` (chemin sans requête), dans l'ordre.
    pub fn on(&self, method: &str, path: &str, responses: impl IntoIterator<Item = Response>) {
        let responses: VecDeque<Response> = responses.into_iter().collect();
        assert!(!responses.is_empty(), "au moins une réponse");
        if let Ok(mut routes) = self.state.routes.lock() {
            routes.retain(|r| !(r.method == method && r.path == path));
            routes.push(Route { method: method.into(), path: path.into(), responses });
        }
    }

    pub async fn next_request(&mut self) -> Request {
        within("requête HTTP du client", self.rx.recv())
            .await
            .unwrap_or_else(|| panic!("serveur HTTP arrêté"))
    }

    /// Toutes les requêtes reçues jusqu'ici.
    pub fn requests(&self) -> Vec<Request> {
        self.state.log.lock().map(|l| l.clone()).unwrap_or_default()
    }
}

/// HTTP/1.1 minimal : une requête après l'autre sur la même connexion
/// (keep-alive), corps lu d'après `Content-Length`.
async fn serve_http(tcp: TcpStream, st: Arc<HttpState>, tx: mpsc::UnboundedSender<Request>) {
    let mut io = BufReader::new(tcp);
    loop {
        let mut line = String::new();
        match io.read_line(&mut line).await {
            Ok(0) | Err(_) => return,
            Ok(_) => {}
        }
        let mut parts = line.split_whitespace();
        let (method, path) = (parts.next().unwrap_or_default().to_string(), parts.next().unwrap_or_default().to_string());
        let mut authorization = None;
        let mut headers = Vec::new();
        let mut len = 0usize;
        loop {
            let mut h = String::new();
            if io.read_line(&mut h).await.unwrap_or(0) == 0 {
                return;
            }
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some((name, value)) = h.split_once(':') {
                let value = value.trim();
                headers.push((name.trim().to_ascii_lowercase(), value.to_string()));
                match name.trim().to_ascii_lowercase().as_str() {
                    "authorization" => authorization = Some(value.to_string()),
                    "content-length" => len = value.parse().unwrap_or(0),
                    _ => {}
                }
            }
        }
        let mut body = vec![0; len];
        if io.read_exact(&mut body).await.is_err() {
            return;
        }
        let in_flight = st.in_flight.fetch_add(1, Ordering::SeqCst);
        let req = Request {
            method,
            path,
            authorization,
            headers,
            body: String::from_utf8_lossy(&body).into_owned(),
            received_at: Instant::now(),
            in_flight,
        };
        if let Ok(mut log) = st.log.lock() {
            log.push(req.clone());
        }
        let _ = tx.send(req.clone());
        let resp = st.respond(&req.method, &req.path);
        if !resp.delay.is_zero() {
            tokio::time::sleep(resp.delay).await;
        }
        // Décompté avant l'écriture : le client ne peut pas voir la réponse
        // avant que la requête soit sortie du compteur.
        st.in_flight.fetch_sub(1, Ordering::SeqCst);
        let mut extra: String = resp.extra.iter().map(|(n, v)| format!("{n}: {v}\r\n")).collect();
        if let Some(d) = &resp.date {
            extra.push_str(&format!("date: {d}\r\n"));
        }
        let tcp = io.get_mut();
        if let Some(len) = resp.stream {
            // Corps sans longueur annoncée : le client ne peut s'arrêter qu'en comptant.
            let head = format!("HTTP/1.1 {} X\r\ncontent-type: image/png\r\nconnection: close\r\n{extra}\r\n", resp.status);
            if tcp.write_all(head.as_bytes()).await.is_err() {
                return;
            }
            let chunk = vec![0u8; 64 << 10];
            let mut sent = 0;
            while sent < len {
                let n = chunk.len().min(len - sent);
                if tcp.write_all(&chunk[..n]).await.is_err() {
                    return;
                }
                sent += n;
            }
            return;
        }
        let (body, ctype) = match &resp.bytes {
            Some((b, t)) => (b.as_slice(), t.as_str()),
            None => (resp.body.as_bytes(), "application/json"),
        };
        // Coupure : la longueur annoncée dépasse ce qui est envoyé.
        let announced = body.len() + if resp.cut { 1000 } else { 0 };
        let head = format!("HTTP/1.1 {} X\r\ncontent-type: {ctype}\r\n{extra}content-length: {announced}\r\n\r\n", resp.status);
        if tcp.write_all(head.as_bytes()).await.is_err() || tcp.write_all(body).await.is_err() || resp.cut {
            return;
        }
    }
}

// ====================================================================== client

/// Le vrai client réseau relié au vrai magasin, comme dans l'interface.
pub struct Client {
    conn: Connection,
    notify: Arc<Notify>,
    pub store: Store,
    pub effects: Vec<Effect>,
}

impl Client {
    pub fn start(token: &str, ep: Endpoints) -> Self {
        let notify = Arc::new(Notify::new());
        let n = notify.clone();
        let conn = net::connect(&tokio::runtime::Handle::current(), token.into(), ep, Arc::new(move || n.notify_one()));
        Self { conn, notify, store: Store::new(), effects: Vec::new() }
    }

    async fn next_event(&mut self) -> Event {
        loop {
            if let Ok(e) = self.conn.events.try_recv() {
                return e;
            }
            self.notify.notified().await;
        }
    }

    /// Applique les événements reçus jusqu'au premier qui satisfait `pred`
    /// (inclus) et le renvoie sous forme de texte de débogage.
    pub async fn until(&mut self, what: &str, pred: impl Fn(&Event) -> bool) -> String {
        let label = format!("événement attendu : {what}");
        within(&label, async {
            loop {
                let e = self.next_event().await;
                let hit = pred(&e);
                let text = format!("{e:?}");
                self.apply(e);
                if hit {
                    return text;
                }
            }
        })
        .await
    }

    fn apply(&mut self, e: Event) {
        if let Some(eff) = self.store.apply(e) {
            self.effects.push(eff);
        }
        self.flush();
    }

    /// Transmet au réseau les commandes accumulées par le magasin.
    pub fn flush(&mut self) {
        for c in self.store.take_commands() {
            self.send(c);
        }
    }

    pub fn send(&self, c: Cmd) {
        let _ = self.conn.cmd.send(c);
    }

    /// Envoie un message avec un nonce unique (comme le magasin) ; rend le nonce.
    pub fn post(&self, channel_id: u64, text: &str) -> u64 {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1 << 40);
        let nonce = NEXT.fetch_add(1, Ordering::SeqCst);
        self.send(Cmd::Send { channel_id, content: text.into(), nonce });
        nonce
    }

    /// Événements déjà arrivés, appliqués sans attendre.
    pub fn drain(&mut self) {
        while let Ok(e) = self.conn.events.try_recv() {
            self.apply(e);
        }
    }
}

// ====================================================================== scénario commun

/// Identifiants des jeux de données (`tests/fixtures/`).
pub mod ids {
    pub const ME: u64 = 1000000000000000001;
    pub const ALICE: u64 = 1000000000000000002;
    pub const ALPHA: u64 = 2000000000000000001;
    pub const BETA: u64 = 2000000000000000002;
    pub const GAMMA: u64 = 2000000000000000003;
    pub const DISCUSSIONS: u64 = 3000000000000000010;
    pub const GENERAL: u64 = 3000000000000000011;
    pub const HORS_SUJET: u64 = 3000000000000000012;
    pub const ANNONCES: u64 = 3000000000000000013;
    pub const NOUVEAU: u64 = 3000000000000000015;
    pub const MODERATION: u64 = 3000000000000000016;
    pub const ARCHIVES: u64 = 3000000000000000020;
    pub const ANCIEN: u64 = 3000000000000000021;
    pub const EQUIPE: u64 = 3000000000000000022;
    pub const COULISSES: u64 = 3000000000000000023;
    pub const ACCUEIL: u64 = 3000000000000000031;
    pub const SALON_GAMMA: u64 = 3000000000000000041;
    pub const DM_ALICE: u64 = 4000000000000000001;
    pub const DM_GROUP: u64 = 4000000000000000002;
}

/// Intervalle de heartbeat assez long pour qu'aucun heartbeat périodique
/// ne vienne perturber un test qui ne s'y intéresse pas.
pub const QUIET: u64 = 60_000;

pub struct Setup {
    pub gw: FakeGateway,
    pub http: FakeHttp,
    pub client: Client,
    pub conn: GwConn,
    /// `d` de l'Identify reçu.
    pub identify: Value,
}

/// Démarre les faux serveurs et le client, fait Hello → Identify → READY
/// (`ready` = nom du jeu de données, avec s = 1) et attend que le magasin
/// ait reçu READY.
pub async fn connected(token: &str, ready: &str) -> Setup {
    connected_with(token, ready, |_| {}).await
}

/// Comme [`connected`], en modifiant d'abord le paquet READY.
pub async fn connected_with(token: &str, ready: &str, edit: impl FnOnce(&mut Value)) -> Setup {
    let mut gw = FakeGateway::start().await;
    let http = FakeHttp::start().await;
    let mut client = Client::start(token, endpoints(&gw, &http));
    let mut conn = gw.accept().await;
    let identify = conn.hello_identify(QUIET).await;
    let mut p = ready_with_resume(&gw, ready);
    edit(&mut p);
    conn.send(p).await;
    client.until("READY", |e| matches!(e, Event::Ready { .. })).await;
    client.until("statut Connecté", |e| matches!(e, Event::Status(Status::Connected))).await;
    Setup { gw, http, client, conn, identify }
}

/// Chemin REST des messages d'un salon.
pub fn messages_path(channel: u64) -> String {
    format!("/api/v10/channels/{channel}/messages")
}
