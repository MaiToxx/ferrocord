//! Connexion par QR code (lot C2) : passerelle d'authentification à distance
//! de Discord, puis échange du ticket contre le jeton.
//!
//! Protocole NON DOCUMENTÉ par Discord, forme vérifiée sur docs.discord.food,
//! « Remote Authentication › Desktop »
//! (<https://docs.discord.food/remote-authentication/desktop>) :
//!
//! - `wss://remote-auth-gateway.discord.gg/?v=2`, en-tête `Origin` exigé
//!   (`https://discord.com`, sinon refus) ; charges JSON plates, champ `op` ;
//! - `hello` { `heartbeat_interval`, `timeout_ms` } → `init`
//!   { `encoded_public_key` : SPKI DER en base64, RSA-OAEP 2048 } ;
//! - `nonce_proof` { `encrypted_nonce` } → `nonce_proof` { `nonce` : nonce
//!   déchiffré, base64url sans `=` } ;
//! - `pending_remote_init` { `fingerprint` : SHA-256 de la clé publique en
//!   base64url } → QR `https://discord.com/ra/<empreinte>` ;
//! - `pending_ticket` { `encrypted_user_payload` } → `pending_login`
//!   { `ticket` } ou `cancel` ; `heartbeat` / `heartbeat_ack` ; fermeture
//!   4003 à l'expiration ;
//! - `POST /users/@me/remote-auth/login` { `ticket` } → { `encrypted_token` }.
//!
//! RSA-OAEP avec SHA-256 (et MGF1 SHA-256), sans étiquette. La clé privée
//! est créée en mémoire pour une seule tentative et n'est jamais écrite :
//! elle disparaît avec la tâche. L'identité annoncée est celle de
//! `net::identity` : en mode Ferrocord, seuls son User-Agent et l'`Origin`
//! exigé partent ; en mode bureau, le User-Agent et les en-têtes du client
//! de bureau (décision de l'utilisateur du 14 novembre 2026).
//!
//! Le journal ne dit que les étapes : ni clé, ni nonce, ni ticket, ni jeton.

use super::ratelimit::{self, Outcome, RateLimits, Refused, Route};
use super::Wake;
use crate::account::Token;
use crate::state::remote_auth::{RemoteEvent, RemoteLogin, RemoteProblem, RemoteUser};
use crate::state::usersettings::b64;
use crate::state::Problem;
use aws_lc_rs::encoding::AsDer;
use aws_lc_rs::rsa::{KeySize, OaepPrivateDecryptingKey, PrivateDecryptingKey, OAEP_SHA256_MGF1SHA256};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use tokio::runtime::Handle;
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::{client::IntoClientRequest, http::HeaderValue, protocol::WebSocketConfig, Message};

/// Ligne du journal (étapes seulement, jamais un secret).
pub type Log = Arc<dyn Fn(&str) + Send + Sync>;

/// Où se connecter. La valeur par défaut est celle de Discord ; les tests
/// construisent la leur (faux serveur sur 127.0.0.1).
#[derive(Clone)]
pub struct RemoteAuthEndpoints {
    /// Passerelle, sans le `/?v=2` final.
    pub gateway: String,
    /// Racine de l'API REST, sans `/` final.
    pub api: String,
    /// En-tête `Origin` exigé par la passerelle.
    pub origin: String,
    /// Attente maximale de `hello` et de chaque étape avant le QR.
    pub step_timeout: Duration,
    pub log: Log,
    /// Identité annoncée (`net::identity`) ; par défaut : celle de Ferrocord.
    pub identity: super::identity::Config,
}

impl Default for RemoteAuthEndpoints {
    fn default() -> Self {
        Self {
            gateway: "wss://remote-auth-gateway.discord.gg".into(),
            api: super::Endpoints::default().api,
            origin: "https://discord.com".into(),
            step_timeout: Duration::from_secs(20),
            log: Arc::new(|_line: &str| {
                #[cfg(debug_assertions)]
                eprintln!("[qr] {_line}");
            }),
            identity: super::identity::Config::default(),
        }
    }
}

/// Plus gros message accepté : les charges de ce protocole font quelques Kio.
const MAX_MESSAGE: usize = 64 << 10;
/// Durée de vie d'une session annoncée par `hello`, bornée (quelques minutes en pratique).
const MAX_SESSION: Duration = Duration::from_secs(15 * 60);
/// Marge après la fin annoncée avant de conclure à l'expiration nous-mêmes.
const EXPIRY_GRACE: Duration = Duration::from_secs(5);
/// Échange du ticket : attente maximale dans le limiteur, puis de la réponse.
const EXCHANGE_WAIT: Duration = Duration::from_secs(30);
/// Essais de l'échange refusé pour limite de débit (429).
const ATTEMPTS: u32 = 3;
const CLOSE_EXPIRED: u16 = 4003;

/// Arrête la tâche quand la tentative est lâchée.
struct Abort(JoinHandle<()>);

impl Drop for Abort {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Lance une tentative : ses événements arrivent par [`RemoteLogin::poll`],
/// `wake` est appelé à chacun.
pub fn start(rt: &Handle, ep: RemoteAuthEndpoints, wake: Wake) -> RemoteLogin {
    let (tx, rx) = mpsc::channel();
    let task = rt.spawn(async move {
        let send = |e: RemoteEvent| {
            let _ = tx.send(e);
            wake();
        };
        let last = match run(&ep, &send).await {
            Ok(e) | Err(e) => e,
        };
        (ep.log)(match &last {
            RemoteEvent::Token(_) => "connexion acceptée",
            RemoteEvent::Cancelled => "refusée sur le téléphone",
            RemoteEvent::Expired => "session expirée",
            _ => "échec",
        });
        send(last);
    });
    RemoteLogin::new(rx, Box::new(Abort(task)))
}

type Step<T> = Result<T, RemoteEvent>;

fn protocol<T>(_: T) -> RemoteEvent {
    RemoteEvent::Failed(RemoteProblem::Protocol)
}

fn network(e: impl std::fmt::Display) -> RemoteEvent {
    RemoteEvent::Failed(RemoteProblem::Network(e.to_string()))
}

/// Base64url sans remplissage (RFC 4648 § 5).
fn b64url(data: &[u8]) -> String {
    b64::encode(data).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// Clé de cette tentative, en mémoire seulement.
struct Keys {
    private: OaepPrivateDecryptingKey,
    /// SubjectPublicKeyInfo DER.
    spki: Vec<u8>,
}

impl Keys {
    /// 2048 bits : quelques dizaines de millisecondes, hors de la boucle.
    async fn generate() -> Step<Keys> {
        tokio::task::spawn_blocking(|| {
            let key = PrivateDecryptingKey::generate(KeySize::Rsa2048).map_err(protocol)?;
            let spki = key.public_key().as_der().map_err(protocol)?.as_ref().to_vec();
            Ok(Keys { private: OaepPrivateDecryptingKey::new(key).map_err(protocol)?, spki })
        })
        .await
        .map_err(protocol)?
    }

    fn fingerprint(&self) -> String {
        b64url(aws_lc_rs::digest::digest(&aws_lc_rs::digest::SHA256, &self.spki).as_ref())
    }

    /// Déchiffre un champ base64 ; le tampon rendu est à effacer par l'appelant
    /// s'il contient un secret.
    fn decrypt(&self, field: &str) -> Step<Vec<u8>> {
        let sealed = b64::decode(field).ok_or(RemoteEvent::Failed(RemoteProblem::Protocol))?;
        let mut out = vec![0u8; self.private.min_output_size()];
        let n = self.private.decrypt(&OAEP_SHA256_MGF1SHA256, &sealed, &mut out, None).map_err(protocol)?.len();
        out.truncate(n);
        Ok(out)
    }
}

/// Efface un tampon qui a contenu un secret (précaution : d'autres copies
/// ont pu exister ailleurs en mémoire).
fn wipe(buf: &mut [u8]) {
    buf.fill(0);
    // `write_volatile` exigerait de l'`unsafe` : `black_box` empêche
    // l'optimiseur de retirer l'écriture avant la libération.
    std::hint::black_box(&buf);
}

#[derive(Deserialize)]
struct Op {
    op: String,
    #[serde(default)]
    heartbeat_interval: Option<u64>,
    #[serde(default)]
    timeout_ms: Option<u64>,
    #[serde(default)]
    encrypted_nonce: Option<String>,
    #[serde(default)]
    fingerprint: Option<String>,
    #[serde(default)]
    encrypted_user_payload: Option<String>,
    #[serde(default)]
    ticket: Option<String>,
}

type Ws = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// Prochain message utile (les pings sont gérés par tungstenite). Fermeture :
/// expiration (4003) ou échec avec son code.
async fn next_op(ws: &mut Ws, seen_user: bool) -> Step<Op> {
    loop {
        match ws.next().await {
            Some(Ok(Message::Text(t))) => return serde_json::from_str::<Op>(t.as_str()).map_err(protocol),
            Some(Ok(Message::Close(frame))) => {
                let code = frame.map_or(1005, |f| u16::from(f.code));
                return Err(match code {
                    CLOSE_EXPIRED => RemoteEvent::Expired,
                    // Fin normale après la lecture du QR sans `cancel` : refus.
                    1000 if seen_user => RemoteEvent::Cancelled,
                    c => RemoteEvent::Failed(RemoteProblem::Closed(c)),
                });
            }
            Some(Ok(_)) => {}
            Some(Err(e)) => return Err(network(e)),
            None => return Err(network("connection closed")),
        }
    }
}

async fn send_json(ws: &mut Ws, v: serde_json::Value) -> Step<()> {
    ws.send(Message::Text(v.to_string().into())).await.map_err(network)
}

async fn run(ep: &RemoteAuthEndpoints, send: &(impl Fn(RemoteEvent) + Sync)) -> Step<RemoteEvent> {
    let log = &ep.log;
    let keys = Keys::generate().await?;
    let mut req = format!("{}/?v=2", ep.gateway).into_client_request().map_err(network)?;
    req.headers_mut().insert("Origin", HeaderValue::from_str(&ep.origin).map_err(protocol)?);
    req.headers_mut().insert("User-Agent", HeaderValue::from_str(&ep.identity.user_agent()).map_err(protocol)?);
    let cfg = WebSocketConfig::default().max_message_size(Some(MAX_MESSAGE)).max_frame_size(Some(MAX_MESSAGE));
    let step = ep.step_timeout;
    let (mut ws, _) = tokio::time::timeout(step, tokio_tungstenite::connect_async_with_config(req, Some(cfg), false))
        .await
        .map_err(|_| network("connection timed out"))?
        .map_err(network)?;
    log("connecté à la passerelle");

    let hello = tokio::time::timeout(step, next_op(&mut ws, false)).await.map_err(|_| network("no hello"))??;
    let (Some(interval), Some(timeout)) = (hello.heartbeat_interval.filter(|_| hello.op == "hello"), hello.timeout_ms) else {
        return Err(RemoteEvent::Failed(RemoteProblem::Protocol));
    };
    let interval = Duration::from_millis(interval.clamp(1_000, 120_000));
    let expires = Instant::now() + Duration::from_millis(timeout).min(MAX_SESSION);
    log("hello reçu");
    send_json(&mut ws, serde_json::json!({ "op": "init", "encoded_public_key": b64::encode(&keys.spki) })).await?;

    let mut beat = tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
    let deadline = tokio::time::sleep_until(tokio::time::Instant::from_std(expires + EXPIRY_GRACE));
    tokio::pin!(deadline);
    let mut acked = true;
    let mut proved = false;
    let mut shown = false;
    let mut seen_user = false;
    loop {
        let op = tokio::select! {
            op = next_op(&mut ws, seen_user) => op?,
            _ = beat.tick() => {
                if !acked {
                    return Err(network("heartbeat not acknowledged"));
                }
                acked = false;
                send_json(&mut ws, serde_json::json!({ "op": "heartbeat" })).await?;
                continue;
            }
            _ = &mut deadline => return Err(RemoteEvent::Expired),
        };
        match op.op.as_str() {
            "heartbeat_ack" => acked = true,
            "nonce_proof" if !proved => {
                let mut nonce = keys.decrypt(op.encrypted_nonce.as_deref().unwrap_or_default())?;
                let proof = b64url(&nonce);
                wipe(&mut nonce);
                send_json(&mut ws, serde_json::json!({ "op": "nonce_proof", "nonce": proof })).await?;
                proved = true;
                log("preuve envoyée");
            }
            // L'empreinte doit être celle de notre clé : sinon le QR mènerait
            // à la session de quelqu'un d'autre.
            "pending_remote_init" if proved && !shown => {
                let fingerprint = op.fingerprint.unwrap_or_default();
                if fingerprint != keys.fingerprint() {
                    return Err(RemoteEvent::Failed(RemoteProblem::Protocol));
                }
                shown = true;
                log("QR prêt");
                send(RemoteEvent::Qr { fingerprint, expires });
            }
            "pending_ticket" if shown => {
                let mut raw = keys.decrypt(op.encrypted_user_payload.as_deref().unwrap_or_default())?;
                let user = std::str::from_utf8(&raw).ok().and_then(RemoteUser::parse);
                wipe(&mut raw);
                let user = user.ok_or(RemoteEvent::Failed(RemoteProblem::Protocol))?;
                seen_user = true;
                log("QR lu, confirmation attendue");
                send(RemoteEvent::User(user));
            }
            "pending_login" if shown => {
                let ticket = op.ticket.filter(|t| !t.is_empty()).ok_or(RemoteEvent::Failed(RemoteProblem::Protocol))?;
                let _ = ws.close(None).await;
                log("ticket reçu, échange");
                return exchange(ep, &keys, ticket).await.map(RemoteEvent::Token);
            }
            "cancel" => return Ok(RemoteEvent::Cancelled),
            // Étape hors d'ordre (preuve demandée deux fois, ticket avant le
            // QR…) : la session n'est pas celle qu'on croit.
            "nonce_proof" | "pending_remote_init" | "pending_ticket" | "pending_login" | "hello" => {
                return Err(RemoteEvent::Failed(RemoteProblem::Protocol));
            }
            // Opération inconnue (ajout futur de Discord) : ignorée.
            _ => {}
        }
    }
}

/// Corps d'un 429 : attente et portée globale.
fn parse_429(text: &str) -> (Option<Duration>, bool, bool) {
    let v = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
    (ratelimit::seconds(v["retry_after"].as_f64()), v["global"].as_bool().unwrap_or(false), v.is_object())
}

/// Ticket → jeton, par le limiteur (un seul appel par connexion acceptée,
/// déclenchée par l'utilisateur sur son téléphone). Aucun jeton n'est envoyé.
async fn exchange(ep: &RemoteAuthEndpoints, keys: &Keys, ticket: String) -> Step<Token> {
    // Mêmes en-têtes que le reste de l'API (mode bureau : ceux du client de bureau).
    let who = super::identity::Identity::new(&ep.identity, false);
    let http = crate::platform::http_builder()
        .user_agent(who.user_agent())
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(network)?;
    let limits = Arc::new(RateLimits::default());
    let route = Route { template: "POST /users/@me/remote-auth/login", major: 0 };
    let url = format!("{}/users/@me/remote-auth/login", ep.api);
    let body = serde_json::json!({ "ticket": ticket });
    for _ in 0..ATTEMPTS {
        let slot = match limits.acquire(route, EXCHANGE_WAIT, std::future::pending()).await {
            Ok(s) => s,
            Err(Refused::Blocked(_) | Refused::TooLong | Refused::Full | Refused::Cancelled) => {
                return Err(RemoteEvent::Failed(RemoteProblem::Api(Problem::TooManyRequests)));
            }
        };
        let resp = who.rest(http.post(&url), &reqwest::Method::POST).json(&body).timeout(EXCHANGE_WAIT).send().await.map_err(network)?;
        let status = resp.status().as_u16();
        let headers = ratelimit::Headers::read(|n| resp.headers().get(n).and_then(|v| v.to_str().ok()));
        let text = resp.text().await.map_err(network)?;
        let mut outcome = Outcome { status, headers, ..Outcome::default() };
        if status == 429 {
            (outcome.retry_after, outcome.global_body, outcome.api_body) = parse_429(&text);
        }
        let (_, blocked) = slot.complete(&outcome);
        (ep.log)(&format!("échange : statut {status}"));
        match status {
            200..=299 => return token_of(keys, text),
            429 if blocked.is_none() => continue,
            429 => return Err(RemoteEvent::Failed(RemoteProblem::Api(Problem::TooManyRequests))),
            _ => {
                let v = serde_json::from_str::<serde_json::Value>(&text).ok();
                if v.as_ref().is_some_and(|v| v.get("captcha_key").is_some() || v.get("captcha_sitekey").is_some()) {
                    return Err(RemoteEvent::Failed(RemoteProblem::Captcha));
                }
                let p = v.and_then(|v| v["message"].as_str().map(|m| Problem::Api(m.chars().take(200).collect()))).unwrap_or(Problem::Http(status));
                return Err(RemoteEvent::Failed(RemoteProblem::Api(p)));
            }
        }
    }
    Err(RemoteEvent::Failed(RemoteProblem::Api(Problem::TooManyRequests)))
}

/// `encrypted_token` déchiffré, contrôlé comme un jeton saisi. Le texte
/// clair n'est gardé que dans l'enveloppe [`Token`].
fn token_of(keys: &Keys, mut text: String) -> Step<Token> {
    #[derive(Deserialize)]
    struct Reply {
        encrypted_token: String,
    }
    let sealed = serde_json::from_str::<Reply>(&text).map(|r| r.encrypted_token);
    let mut bytes = std::mem::take(&mut text).into_bytes();
    wipe(&mut bytes);
    let mut plain = keys.decrypt(&sealed.map_err(protocol)?)?;
    let token = std::str::from_utf8(&plain).ok().and_then(|s| Token::parse(s).ok());
    wipe(&mut plain);
    token.ok_or(RemoteEvent::Failed(RemoteProblem::Protocol))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_has_no_padding() {
        assert_eq!(b64url(&[0xfb, 0xff]), "-_8");
        assert_eq!(b64url(b"f"), "Zg");
    }

    #[test]
    fn decrypts_what_the_public_key_sealed() {
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let keys = rt.block_on(Keys::generate()).unwrap_or_else(|_| panic!("clé"));
        let public = aws_lc_rs::rsa::PublicEncryptingKey::from_der(&keys.spki).unwrap();
        let public = aws_lc_rs::rsa::OaepPublicEncryptingKey::new(public).unwrap();
        let mut sealed = vec![0u8; public.ciphertext_size()];
        let sealed = public.encrypt(&OAEP_SHA256_MGF1SHA256, b"bonjour", &mut sealed, None).unwrap();
        assert_eq!(keys.decrypt(&b64::encode(sealed)).ok().as_deref(), Some(&b"bonjour"[..]));
        assert!(keys.decrypt("pas du base64 !").is_err());
        assert!(keys.decrypt(&b64::encode(b"trop court")).is_err());
        assert_eq!(keys.fingerprint().len(), 43, "SHA-256 en base64url sans remplissage");
    }
}
