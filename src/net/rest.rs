//! API REST : appels HTTP passés par le limiteur de débit par compartiment
//! (`ratelimit`), tâche de commandes.

use super::health::{Health, Refusal, Refusals};
use super::ratelimit::{self, Outcome, RateLimits, Refused, Route};
use super::raw::RawMessage;
use super::{wall_clock_ms, Out};
use crate::state::activity::ActivityCmd;
use crate::state::social::{SendKind, SocialCmd};
use crate::state::upload::UploadCmd;
use crate::state::{Cmd, Event, Problem, HISTORY_PAGE};
use serde_json::json;
use std::{
    collections::HashMap,
    future::Future,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
use tokio::sync::{mpsc as tmpsc, watch};

// Écritures sociales et accusés : module enfant pour passer par `api` (même
// limiteur, mêmes erreurs) sans l'exposer ailleurs.
#[path = "relations_rest.rs"]
mod relations;
// Messages avec pièces jointes (lot T8) : même file d'envoi, même limiteur.
#[path = "upload.rs"]
mod upload;
// Interactions avec options pièce jointe (lot fin-texte-restes).
pub(super) use upload::multipart_in_memory;

/// Longueur maximale d'un message d'erreur du serveur gardé pour l'affichage.
const SERVER_MESSAGE_MAX: usize = 300;

/// Message d'erreur du serveur, tel qu'il peut être affiché ou journalisé :
/// borné, et sans le jeton (un serveur ou un intermédiaire qui renverrait
/// l'en-tête `Authorization` dans sa réponse ne le fait pas entrer dans un
/// journal).
fn server_message(message: &str, token: &str) -> String {
    let bare = token.strip_prefix("Bot ").unwrap_or(token);
    let clean = if bare.is_empty() { message.to_string() } else { message.replace(bare, "<jeton masqué>") };
    clean.chars().take(SERVER_MESSAGE_MAX).collect()
}

pub(super) enum ApiError {
    Unauthorized,
    Forbidden,
    /// Requête abandonnée avant de partir (salon quitté pendant l'attente).
    Cancelled,
    /// Discord bloque la connexion jusqu'à cette heure : rien n'est retenté.
    Blocked(Instant),
    /// La requête est peut-être arrivée (délai dépassé, connexion coupée
    /// après l'envoi) : son issue est inconnue.
    Unknown(Problem),
    Other(Problem),
    /// 404 : ressource inconnue (message de Discord gardé).
    NotFound(Problem),
    /// Discord exige un captcha (réponse portant `captcha_key`) : jamais
    /// contourné, l'action est à faire dans l'application officielle.
    Captcha,
}

impl ApiError {
    /// Événement à publier : un 401 veut dire que le jeton est mort, donc
    /// retour à l'écran de connexion (comme le code 4004) ; les autres
    /// erreurs passent par `other` avec leur texte.
    pub(super) fn into_event(self, other: impl FnOnce(Problem) -> Event) -> Event {
        match self {
            ApiError::Unauthorized => Event::Fatal { problem: Problem::BadToken, bad_token: true },
            ApiError::Forbidden => other(Problem::Forbidden),
            ApiError::Cancelled => other(Problem::Api(String::new())),
            ApiError::Blocked(until) => other(Problem::Blocked { retry_ms: wall_clock_ms(until) }),
            ApiError::Unknown(e) | ApiError::Other(e) | ApiError::NotFound(e) => other(e),
            ApiError::Captcha => other(Problem::Http(400)),
        }
    }
}

/// Nombre d'essais d'une requête refusée pour limite de débit (429).
const ATTEMPTS: u32 = 3;
/// Nouveaux essais d'une LECTURE après une coupure ou un 500, 502, 503, 504.
/// Les écritures ne sont jamais retentées d'elles-mêmes : un message garde
/// son `nonce` et c'est l'utilisateur qui réessaie (pas de doublon).
const TRANSIENT_RETRIES: u32 = 2;
const RETRY_BASE: Duration = Duration::from_millis(500);
const RETRY_CAP: Duration = Duration::from_secs(4);
/// Taille maximale lue d'une réponse de l'API (la plus grosse attendue, une
/// page de 100 messages avec leurs intégrations, pèse quelques centaines de Kio).
const MAX_BODY: usize = 16 << 20;
/// Ouverture d'une connexion HTTP : au-delà, échec (et non 30 s d'attente).
pub(super) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Redirections de l'API : suivies seulement vers le même hôte (même schéma,
/// même port), cinq au plus. Vers un autre hôte, la réponse 3xx est rendue
/// telle quelle : `X-Super-Properties`, la langue et le fuseau ne partent
/// jamais chez un tiers (reqwest ne retire de lui-même que `Authorization`).
pub(super) fn same_host_redirects() -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(|attempt| {
        let same = attempt.previous().last().is_some_and(|from| same_origin(from, attempt.url()));
        if same && attempt.previous().len() <= 5 { attempt.follow() } else { attempt.stop() }
    })
}

fn same_origin(a: &reqwest::Url, b: &reqwest::Url) -> bool {
    a.scheme() == b.scheme() && a.host_str() == b.host_str() && a.port_or_known_default() == b.port_or_known_default()
}
/// Attente maximale d'une page d'historique avant de partir : au-delà,
/// l'échec est annoncé tout de suite (l'utilisateur peut réessayer).
const HISTORY_MAX_WAIT: Duration = Duration::from_secs(10);
/// Attente maximale d'un message avant de partir.
const SEND_MAX_WAIT: Duration = Duration::from_secs(30);
/// Attente maximale d'un « en train d'écrire » : au-delà, il ne dit plus rien.
const TYPING_MAX_WAIT: Duration = Duration::from_secs(2);
/// Une requête sans réponse libère sa place dans le compartiment.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Salons ayant une file d'envoi en même temps, au plus.
const MAX_SEND_QUEUES: usize = 64;

/// Heure d'un en-tête HTTP `Date` (« Tue, 15 Nov 1994 08:12:31 GMT ») en ms Unix.
fn http_date_ms(value: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc2822(value).ok().map(|d| d.timestamp_millis())
}

/// En-tête standard `Retry-After` : des secondes, ou une date HTTP.
fn http_retry_after(value: &str, now_ms: i64) -> Option<Duration> {
    let v = value.trim();
    if let Ok(s) = v.parse::<u64>() {
        return ratelimit::seconds(Some(s as f64));
    }
    let at = http_date_ms(v)?;
    ratelimit::seconds(Some((at - now_ms) as f64 / 1000.0))
}

/// Ce que partagent toutes les requêtes d'une connexion.
pub(super) struct Ctx {
    http: reqwest::Client,
    token: String,
    pub(super) api_root: String,
    pub(super) out: Out,
    limits: Arc<RateLimits>,
    /// En-têtes du client annoncé (`net::identity`).
    identity: Arc<super::identity::Identity>,
    /// Compteurs de la connexion (4xx, 429, nouveaux essais…).
    health: Arc<Health>,
    /// Lectures refusées récemment (403, 404) : pas redemandées en boucle.
    refusals: std::sync::Mutex<Refusals>,
}

impl Ctx {
    fn refusals(&self) -> std::sync::MutexGuard<'_, Refusals> {
        // Verrou empoisonné : la table reste cohérente (chaque opération est d'un bloc).
        self.refusals.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Le jeton est-il celui du compte `user_id` ? Sa première partie est
    /// l'identifiant du compte en base64 (rien d'autre n'en est lu).
    pub(super) fn is_me(&self, user_id: u64) -> bool {
        let head = self.token.split('.').next().unwrap_or_default();
        let (mut bits, mut n, mut out) = (0u32, 0u8, String::new());
        for c in head.bytes() {
            let v = match c {
                b'A'..=b'Z' => c - b'A',
                b'a'..=b'z' => c - b'a' + 26,
                b'0'..=b'9' => c - b'0' + 52,
                b'+' | b'-' => 62,
                b'/' | b'_' => 63,
                _ => break,
            };
            bits = (bits << 6) | u32::from(v);
            n += 6;
            if n >= 8 {
                n -= 8;
                out.push(char::from(((bits >> n) & 0xFF) as u8));
            }
        }
        out.parse::<u64>().is_ok_and(|id| id == user_id)
    }

    /// Compte bot : les points d'API réservés aux comptes utilisateur ne sont pas appelés.
    pub(super) fn is_bot(&self) -> bool {
        self.token.starts_with("Bot ")
    }

    /// La connexion s'annonce comme l'application de bureau (`net::identity`).
    pub(super) fn identity_is_desktop(&self) -> bool {
        self.identity.is_desktop()
    }
}

/// Corps d'un 429 : `retry_after` (secondes), `global`, et s'il s'agit d'un
/// objet JSON de l'API (et non d'une page du pare-feu).
fn parse_429(text: &str) -> (Option<Duration>, bool, bool) {
    let v = serde_json::from_str::<serde_json::Value>(text).unwrap_or_default();
    (ratelimit::seconds(v["retry_after"].as_f64()), v["global"].as_bool().unwrap_or(false), v.is_object())
}

fn mfa_required(text: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(text).is_ok_and(|v| v["code"].as_u64() == Some(60003))
}

/// Erreur d'envoi : jamais partie (connexion impossible), ou issue inconnue.
fn network_error(e: reqwest::Error) -> ApiError {
    let unsent = e.is_connect() || e.is_builder();
    let text = e.without_url().to_string();
    if unsent { ApiError::Other(Problem::Network(text)) } else { ApiError::Unknown(Problem::Network(text)) }
}

pub(super) async fn api(
    ctx: &Ctx,
    route: Route,
    method: reqwest::Method,
    url: &str,
    body: Option<serde_json::Value>,
    max_wait: Duration,
    cancel: impl Future<Output = ()>,
) -> Result<String, ApiError> {
    let build = |req: reqwest::RequestBuilder| match &body {
        Some(b) => req.json(b),
        None => req,
    };
    api_req(ctx, route, method, url, build, max_wait, cancel).await
}

/// Comme [`api`], le corps étant posé par `build` à chaque essai (un corps
/// en flux ne se rejoue pas : il est refait après un 429).
pub(super) async fn api_req(
    ctx: &Ctx,
    route: Route,
    method: reqwest::Method,
    url: &str,
    mut build: impl FnMut(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    max_wait: Duration,
    cancel: impl Future<Output = ()>,
) -> Result<String, ApiError> {
    tokio::pin!(cancel);
    // Lecture sans effet de bord : seule sorte de requête retentée d'elle-même
    // après une coupure ou un 5xx, et seule retenue par la mémoire négative.
    let read = matches!(method, reqwest::Method::GET | reqwest::Method::HEAD);
    if read && let Some(refusal) = ctx.refusals().lookup(url, Instant::now()) {
        // Déjà refusée il y a peu : même réponse, sans requête.
        ctx.health.spared();
        return Err(match refusal {
            Refusal::Forbidden => ApiError::Forbidden,
            Refusal::NotFound(p) => ApiError::NotFound(p),
        });
    }
    let (mut limited, mut transient) = (0u32, 0u32);
    loop {
        let slot = match ctx.limits.acquire(route, max_wait, cancel.as_mut()).await {
            Ok(s) => s,
            Err(Refused::Cancelled) => return Err(ApiError::Cancelled),
            Err(Refused::Blocked(until)) => return Err(ApiError::Blocked(until)),
            Err(Refused::TooLong | Refused::Full) => return Err(ApiError::Other(Problem::TooManyRequests)),
        };
        let req = build(ctx.identity.rest(ctx.http.request(method.clone(), url).header("Authorization", &ctx.token), &method).timeout(REQUEST_TIMEOUT));
        ctx.health.request();
        // En cas d'erreur réseau, `slot` est rendu sans rien apprendre (et
        // compte comme consommé).
        let answer = async {
            let mut resp = req.send().await?;
            let status = resp.status().as_u16();
            let headers = ratelimit::Headers::read(|n| resp.headers().get(n).and_then(|v| v.to_str().ok()));
            let server_ms = resp.headers().get(reqwest::header::DATE).and_then(|v| v.to_str().ok()).and_then(http_date_ms);
            let retry_header = resp
                .headers()
                .get(reqwest::header::RETRY_AFTER)
                .and_then(|v| v.to_str().ok())
                .and_then(|v| http_retry_after(v, server_ms.unwrap_or_else(|| chrono::Utc::now().timestamp_millis())));
            // Corps borné : une réponse sans fin ne remplit pas la mémoire.
            let mut body = Vec::new();
            let mut whole = true;
            while let Some(chunk) = resp.chunk().await? {
                if body.len() + chunk.len() > MAX_BODY {
                    whole = false;
                    break;
                }
                body.extend_from_slice(&chunk);
            }
            Ok::<_, reqwest::Error>((status, headers, server_ms, retry_header, whole.then_some(body)))
        }
        .await;
        let (status, headers, server_ms, retry_header, body) = match answer {
            Ok(a) => a,
            Err(e) => {
                drop(slot);
                // Lecture coupée (pas un délai dépassé : attendre 30 s de plus
                // n'aiderait personne) : nouvel essai, après un recul.
                if read && !e.is_timeout() && transient < TRANSIENT_RETRIES {
                    transient += 1;
                    ctx.health.retry();
                    if !pause(retry_wait(transient), cancel.as_mut()).await {
                        return Err(ApiError::Cancelled);
                    }
                    continue;
                }
                return Err(network_error(e));
            }
        };
        ctx.health.status(status);
        let Some(body) = body else {
            drop(slot);
            return Err(ApiError::Other(Problem::Http(status)));
        };
        let text = String::from_utf8_lossy(&body).into_owned();
        let mut outcome = Outcome { status, headers, ..Outcome::default() };
        if status == 429 {
            (outcome.retry_after, outcome.global_body, outcome.api_body) = parse_429(&text);
            outcome.http_retry_after = retry_header;
        }
        let (_stats, blocked) = slot.complete(&outcome);
        // L'heure de Discord sert à juger la fin des exclusions temporaires
        // même si l'horloge de la machine est décalée. Publiée après que le
        // limiteur a lu la réponse (les tests s'en servent comme repère).
        if let Some(ms) = server_ms {
            ctx.out.send(Event::ServerTime(ms));
        }
        match status {
            200..=299 => {
                if read {
                    ctx.refusals().forget(url);
                } else {
                    // Une écriture a pu changer ce qui était refusé (serveur
                    // rejoint, rôle donné, note créée…).
                    ctx.refusals().clear();
                }
                return Ok(text);
            }
            // Code 60003 : A2F exigée, soit par le serveur pour ses modérateurs
            // (403), soit comme vérification de cette action (401, le jeton reste
            // bon ; docs.discord.food, « Authentication › Multi-Factor
            // Authentication »). Un seul chemin : à faire dans l'application officielle.
            401 | 403 if mfa_required(&text) => return Err(ApiError::Other(Problem::MfaRequired)),
            // 401, 403 : jamais de nouvel essai automatique.
            401 => return Err(ApiError::Unauthorized),
            403 => {
                if read {
                    ctx.refusals().note(url, Refusal::Forbidden, Instant::now());
                }
                return Err(ApiError::Forbidden);
            }
            // Panne passagère de Discord (ou de son pare-feu) sur une lecture :
            // deux nouveaux essais au plus, avec recul. Une écriture n'est
            // jamais renvoyée d'elle-même (elle a pu être exécutée).
            500 | 502 | 503 | 504 if read && transient < TRANSIENT_RETRIES => {
                transient += 1;
                ctx.health.retry();
                if !pause(retry_wait(transient), cancel.as_mut()).await {
                    return Err(ApiError::Cancelled);
                }
            }
            429 => {
                // Ne devrait jamais arriver en usage normal : le limiteur attend
                // avant d'envoyer. Journalisé sans jeton ni identifiant.
                #[cfg(debug_assertions)]
                eprintln!(
                    "[rest] 429 n° {} sur {} (global : {}, portée : {}, attente : {:?}, blocage : {})",
                    _stats.limited,
                    route.template,
                    outcome.global_body || outcome.headers.global,
                    outcome.headers.scope.as_deref().unwrap_or("?"),
                    outcome.retry_after.or(outcome.http_retry_after),
                    blocked.is_some()
                );
                if let Some(until) = blocked {
                    // Pare-feu de Discord : tout est en pause, rien n'est retenté.
                    // L'heure du nouvel essai est annoncée par l'erreur de la requête.
                    return Err(ApiError::Blocked(until));
                }
                // Le nouvel essai attend dans le limiteur la fin indiquée par
                // Discord ; après le dernier refus, l'échec est annoncé tout de suite.
                limited += 1;
                if limited >= ATTEMPTS {
                    return Err(ApiError::Other(Problem::TooManyRequests));
                }
            }
            _ => {
                let v = serde_json::from_str::<serde_json::Value>(&text).ok();
                if relations::is_captcha(v.as_ref()) {
                    return Err(ApiError::Captcha);
                }
                // Refus d'une écriture du profil (limite de changements, image trop lourde, Nitro) : dit en clair.
                let msg = super::account::refusal(route.template, status, v.as_ref())
                    .unwrap_or_else(|| v.and_then(|v| v["message"].as_str().map(|m| Problem::Api(server_message(m, &ctx.token)))).unwrap_or(Problem::Http(status)));
                if status == 404 {
                    if read {
                        ctx.refusals().note(url, Refusal::NotFound(msg.clone()), Instant::now());
                    }
                    return Err(ApiError::NotFound(msg));
                }
                return Err(ApiError::Other(msg));
            }
        }
    }
}

/// Attente avant le `n`-ième nouvel essai d'une lecture (1, 2…) : 0,5 s puis
/// 1 s, avec gigue, 4 s au plus.
fn retry_wait(n: u32) -> Duration {
    super::backoff::retry_delay(RETRY_BASE, n.saturating_sub(1), RETRY_CAP, super::backoff::unit())
}

/// Attend `d` ; `false` si `cancel` se termine avant (salon quitté…).
async fn pause(d: Duration, cancel: impl Future<Output = ()>) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(d) => true,
        _ = cancel => false,
    }
}

/// Se termine quand l'utilisateur affiche un autre salon que `channel`.
/// L'interface ne demande d'historique que pour le salon qu'elle affiche :
/// la dernière demande reçue désigne donc le salon courant.
async fn left_channel(mut focus: watch::Receiver<Option<u64>>, channel: u64) {
    loop {
        if *focus.borrow_and_update() != Some(channel) {
            return;
        }
        if focus.changed().await.is_err() {
            std::future::pending::<()>().await;
        }
    }
}

/// Page d'historique demandée : avant `before` (aucun : la dernière page), ou
/// après `after` (fenêtre du cache qui redescend vers le présent).
#[derive(Clone, Copy)]
struct Page {
    before: Option<u64>,
    after: Option<u64>,
}

async fn history(ctx: Arc<Ctx>, channel_id: u64, page: Page, cancel: impl Future<Output = ()>) {
    let Page { before, after } = page;
    let mut url = format!("{}/channels/{channel_id}/messages?limit={HISTORY_PAGE}", ctx.api_root);
    if let Some(b) = before {
        url.push_str(&format!("&before={b}"));
    }
    if let Some(a) = after {
        url.push_str(&format!("&after={a}"));
    }
    let route = Route::channel_messages_get(channel_id);
    let res = api(&ctx, route, reqwest::Method::GET, &url, None, HISTORY_MAX_WAIT, cancel)
        .await
        .and_then(|body| serde_json::from_str::<Vec<RawMessage>>(&body).map_err(|e| ApiError::Other(Problem::Api(e.to_string()))));
    match res {
        // Documentation de Discord (Get Channel Messages,
        // https://discord.com/developers/docs/resources/message#get-channel-messages) :
        // « Returns an array of message objects from newest to oldest
        // on success », avec `before`, `after` ou `around` (« mutually
        // exclusive »). La page est donc retournée ici (du plus ancien
        // au plus récent) ; le magasin la trie de toute façon, sans
        // dépendre de cet ordre. La documentation ne dit pas lesquels
        // des messages postérieurs à `after` sont rendus quand il y en
        // a plus de `limit` : le magasin suppose les `limit` qui suivent
        // immédiatement (non vérifié sur le vrai service).
        Ok(raw) => {
            super::people::messages(&raw, &ctx.out.people);
            raw.iter().filter_map(RawMessage::thread_event).for_each(|e| ctx.out.send(e));
            let messages = raw.into_iter().rev().map(RawMessage::into_message).collect();
            ctx.out.send(match after {
                Some(after) => Event::HistoryAfter { channel_id, after, messages },
                None => Event::History { channel_id, before, messages },
            });
        }
        // Salon quitté : rien à afficher, il sera redemandé à la réouverture.
        Err(ApiError::Cancelled) => ctx.out.send(Event::HistoryCancelled { channel_id, before, after }),
        Err(e) => ctx.out.send(e.into_event(|error| Event::HistoryError { channel_id, before, after, error })),
    }
}

/// Corps de Create Message. `enforce_nonce` (documenté) : si un message du
/// même auteur avec ce nonce a été créé « dans les dernières minutes »,
/// Discord le renvoie au lieu d'en créer un second. Un nouvel essai avec le
/// même nonce ne fait donc pas de doublon tant qu'il suit de près.
///
/// Préfixes du texte (lot FF, `state::builtin`) : `/tts ` devient
/// `tts: true`, `@silent ` le drapeau `SUPPRESS_NOTIFICATIONS` (champs
/// `tts` et `flags` de Create Message, documentés).
fn send_body(content: &str, nonce: u64) -> serde_json::Value {
    let o = crate::state::builtin::send_opts(content);
    let mut body = json!({ "content": o.text, "nonce": nonce.to_string(), "tts": o.tts, "enforce_nonce": true });
    if o.silent {
        body["flags"] = json!(crate::state::builtin::SUPPRESS_NOTIFICATIONS);
    }
    body
}

async fn send(ctx: &Ctx, channel_id: u64, content: String, nonce: u64, kind: Option<SendKind>) {
    let url = format!("{}/channels/{channel_id}/messages", ctx.api_root);
    let route = Route::channel_messages_post(channel_id);
    let body = super::social::send_body(send_body(&content, nonce), kind.as_ref());
    let failed = |error: Problem, forbidden: bool, content: String| Event::SendError { channel_id, nonce, content, error, forbidden };
    match api(ctx, route, reqwest::Method::POST, &url, Some(body), SEND_MAX_WAIT, std::future::pending()).await {
        Ok(_) => ctx.out.send(Event::SendConfirmed { channel_id, nonce }),
        Err(ApiError::Unauthorized) => ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
        Err(ApiError::Forbidden) => ctx.out.send(failed(Problem::Forbidden, true, content)),
        Err(ApiError::Unknown(_)) => ctx.out.send(Event::SendUnconfirmed { channel_id, nonce, content }),
        Err(e @ (ApiError::Cancelled | ApiError::Blocked(_) | ApiError::Other(_) | ApiError::NotFound(_) | ApiError::Captcha)) => {
            let ev = e.into_event(|error| failed(error, false, content));
            ctx.out.send(ev);
        }
    }
}

/// « En train d'écrire » (Trigger Typing Indicator,
/// https://discord.com/developers/docs/resources/channel#trigger-typing-indicator,
/// réponse 204). Sans importance : s'il doit attendre, il est abandonné, et
/// un échec n'est pas signalé (sauf jeton refusé).
async fn typing(ctx: Arc<Ctx>, channel_id: u64) {
    let url = format!("{}/channels/{channel_id}/typing", ctx.api_root);
    let route = Route::channel_typing(channel_id);
    if let Err(ApiError::Unauthorized) = api(&ctx, route, reqwest::Method::POST, &url, None, TYPING_MAX_WAIT, std::future::pending()).await {
        ctx.out.send(ApiError::Unauthorized.into_event(Event::Error));
    }
}

/// File d'envoi d'un salon : ses messages partent un par un, dans l'ordre ;
/// les salons sont indépendants (un salon ralenti ne bloque pas les autres).
struct SendQueue {
    tx: tmpsc::UnboundedSender<Job>,
    /// Messages acceptés et pas encore traités.
    pending: Arc<AtomicUsize>,
}

/// Envoi en file : texte (avec réponse ou transfert), ou message avec
/// pièces jointes (lot T8).
enum Job {
    Text(String, u64, Option<SendKind>),
    Upload(upload::Job),
}

fn spawn_send_queue(ctx: Arc<Ctx>, channel_id: u64, cancels: upload::Cancels) -> SendQueue {
    let (tx, mut rx) = tmpsc::unbounded_channel::<Job>();
    let pending = Arc::new(AtomicUsize::new(0));
    let p = pending.clone();
    tokio::spawn(async move {
        while let Some(job) = rx.recv().await {
            match job {
                Job::Text(content, nonce, kind) => send(&ctx, channel_id, content, nonce, kind).await,
                Job::Upload(j) => upload::run(&ctx, j, &cancels).await,
            }
            p.fetch_sub(1, Ordering::SeqCst);
        }
    });
    SendQueue { tx, pending }
}

/// Met un envoi dans la file de son salon ; rendu s'il y a trop de files.
fn enqueue(ctx: &Arc<Ctx>, queues: &mut HashMap<u64, SendQueue>, cancels: &upload::Cancels, channel_id: u64, job: Job) -> Result<(), Job> {
    if !queues.contains_key(&channel_id) && queues.len() >= MAX_SEND_QUEUES {
        // Une file vide se termine dès que son émetteur disparaît.
        queues.retain(|_, q| q.pending.load(Ordering::SeqCst) > 0);
    }
    if !queues.contains_key(&channel_id) && queues.len() >= MAX_SEND_QUEUES {
        return Err(job);
    }
    let q = queues.entry(channel_id).or_insert_with(|| spawn_send_queue(ctx.clone(), channel_id, cancels.clone()));
    q.pending.fetch_add(1, Ordering::SeqCst);
    if let Err(e) = q.tx.send(job) {
        // La tâche de la file s'est arrêtée (ne devrait pas arriver) : nouvelle file.
        let fresh = spawn_send_queue(ctx.clone(), channel_id, cancels.clone());
        fresh.pending.fetch_add(1, Ordering::SeqCst);
        let _ = fresh.tx.send(e.0);
        queues.insert(channel_id, fresh);
    }
    Ok(())
}

pub(super) async fn command_task(
    http: reqwest::Client,
    token: String,
    api_root: String,
    mut rx: tmpsc::UnboundedReceiver<Cmd>,
    out: Out,
    identity: Arc<super::identity::Identity>,
    health: Arc<Health>,
) {
    let ctx = Arc::new(Ctx { http, token, api_root, out, limits: Arc::new(RateLimits::default()), identity, health, refusals: Default::default() });
    let (focus, _) = watch::channel(None::<u64>);
    let mut queues: HashMap<u64, SendQueue> = HashMap::new();
    let cancels = upload::Cancels::default();
    while let Some(cmd) = rx.recv().await {
        // Réponses et transferts passent par la même file d'envoi que les messages.
        let (cmd, kind) = match cmd {
            Cmd::Social(SocialCmd::Send { channel_id, content, nonce, kind }) => (Cmd::Send { channel_id, content, nonce }, Some(kind)),
            other => (other, None),
        };
        match cmd {
            Cmd::Social(c) => {
                tokio::spawn(super::social::run(ctx.clone(), c));
            }
            // Pages vers le passé (`before`) et vers le présent (`after`) : même
            // limiteur, même annulation quand le salon est quitté.
            Cmd::History { channel_id, before } => {
                focus.send_replace(Some(channel_id));
                let cancel = left_channel(focus.subscribe(), channel_id);
                tokio::spawn(history(ctx.clone(), channel_id, Page { before, after: None }, cancel));
            }
            Cmd::HistoryAfter { channel_id, after } => {
                focus.send_replace(Some(channel_id));
                let cancel = left_channel(focus.subscribe(), channel_id);
                tokio::spawn(history(ctx.clone(), channel_id, Page { before: None, after: Some(after) }, cancel));
            }
            Cmd::Typing { channel_id } => {
                tokio::spawn(typing(ctx.clone(), channel_id));
            }
            // Carte de profil : seulement sur un geste de l'utilisateur (le magasin dédoublonne).
            Cmd::OpenDm { user_id } => {
                tokio::spawn(super::people::open_dm(ctx.clone(), user_id));
            }
            Cmd::Profile(c) => {
                tokio::spawn(super::profile::run(ctx.clone(), c));
            }
            // Une écriture à la fois par réglage : le magasin y veille.
            Cmd::Account(c) => {
                tokio::spawn(super::account::run(ctx.clone(), c));
            }
            Cmd::Send { channel_id, content, nonce } => {
                if let Err(Job::Text(content, nonce, _)) = enqueue(&ctx, &mut queues, &cancels, channel_id, Job::Text(content, nonce, kind)) {
                    // Le texte revient à l'utilisateur.
                    ctx.out.send(Event::SendError { channel_id, nonce, content, error: Problem::TooManyQueues, forbidden: false });
                }
            }
            Cmd::Upload(UploadCmd::Send { channel_id, nonce, content, files, kind }) => {
                let cancel = cancels.register(nonce);
                let job = Job::Upload(upload::Job { channel_id, nonce, content, files, kind, cancel });
                if enqueue(&ctx, &mut queues, &cancels, channel_id, job).is_err() {
                    cancels.forget(nonce);
                    let failure = crate::state::upload::Failure::Problem(Problem::TooManyQueues);
                    ctx.out.send(Event::Upload(crate::state::upload::UploadEvent::Failed { channel_id, nonce, failure }));
                }
            }
            Cmd::Upload(UploadCmd::Cancel { nonce }) => cancels.cancel(nonce),
            // Passerelle, pas REST : aiguillée avant d'arriver ici (`net::connect`).
            Cmd::Subscribe(_) => {}
            Cmd::Relations(c) => {
                tokio::spawn(relations::run(ctx.clone(), c));
            }
            Cmd::Threads(c) => {
                tokio::spawn(super::threads::run(ctx.clone(), c));
            }
            Cmd::Servers(c) => {
                tokio::spawn(super::guilds::run(ctx.clone(), c));
            }
            // Recherche (après un geste) et page autour d'un message (saut).
            Cmd::Search(c) => {
                tokio::spawn(super::search::run(ctx.clone(), c));
            }
            Cmd::Inbox(c) => {
                tokio::spawn(super::inbox::run(ctx.clone(), c));
            }
            Cmd::Interactions(c) => {
                tokio::spawn(super::interactions::run(ctx.clone(), c));
            }
            Cmd::Polls(c) => {
                tokio::spawn(super::polls::run(ctx.clone(), c));
            }
            Cmd::Invites(c) => {
                tokio::spawn(super::announce::run_invite(ctx.clone(), c));
            }
            Cmd::Announce(c) => {
                tokio::spawn(super::announce::run(ctx.clone(), c));
            }
            Cmd::GuildAdmin(c) => {
                tokio::spawn(super::admin::guild::run(ctx.clone(), c));
            }
            Cmd::ChannelAdmin(c) => {
                tokio::spawn(super::admin::channels::run(ctx.clone(), c));
            }
            // Rôles et membres d'un serveur (lot A3), après un geste.
            Cmd::Roles(c) => {
                tokio::spawn(super::admin::roles::run(ctx.clone(), c));
            }
            Cmd::Moderation(c) => {
                tokio::spawn(super::admin::moderation::run(ctx.clone(), c));
            }
            // Événements programmés, accueil des nouveaux membres (lot A7).
            Cmd::GuildEvents(c) => {
                tokio::spawn(super::admin::events::run(ctx.clone(), c));
            }
            Cmd::Community(c) => {
                tokio::spawn(super::admin::community::run(ctx.clone(), c));
            }
            Cmd::Expressions(c) => {
                tokio::spawn(super::admin::expressions::run(ctx.clone(), c));
            }
            // Modération vocale (phase 4) ; l'op 4 part par la passerelle, le reste est local.
            Cmd::Voice(c @ crate::state::voice::VoiceCmd::Moderate { .. }) => {
                tokio::spawn(super::voicegw::run(ctx.clone(), c));
            }
            Cmd::Voice(_) => {}
            // Appels, conférences, tableau de sons (lot V6), après un geste.
            Cmd::Calls(c) => {
                tokio::spawn(super::calls::run_call(ctx.clone(), c));
            }
            Cmd::Stage(c) => {
                tokio::spawn(super::calls::run_stage(ctx.clone(), c));
            }
            // Vidéo (phase 5) : tout part par la passerelle ; tuiles locales.
            Cmd::Video(_) => {}
            // Activité (lot C3) : liste des jeux (à la connexion, une fois par
            // jour au plus) et adresse d'un bouton (après un clic).
            Cmd::Activity(ActivityCmd::LoadDetectable { cache, delay }) => {
                tokio::spawn(super::games::load(ctx.clone(), cache, delay));
            }
            Cmd::Activity(ActivityCmd::Button { user_id, session_id, app_id, index }) => {
                tokio::spawn(super::games::button(ctx.clone(), user_id, session_id, app_id, index));
            }
            Cmd::Activity(ActivityCmd::Assets { app_id }) => {
                tokio::spawn(super::games::assets(ctx.clone(), app_id));
            }
            // Passerelle et sondage : aiguillés avant d'arriver ici (`net::connect`).
            Cmd::Activity(ActivityCmd::Presence(_) | ActivityCmd::Scan(_) | ActivityCmd::Rpc(_)) => {}
            // GIF, favoris du compte, super réaction (lot G), après un geste.
            Cmd::Gifs(c) => {
                tokio::spawn(super::gifs::run(ctx.clone(), c));
            }
            // Effets de profil et quêtes (lot ND), après un geste.
            Cmd::Discovery(c) => {
                tokio::spawn(super::discovery::run(ctx.clone(), c));
            }
            Cmd::Summaries(c) => {
                tokio::spawn(super::summaries::run(ctx.clone(), c));
            }
            Cmd::Verification(c) => {
                tokio::spawn(super::verification::run(ctx.clone(), c));
            }
            Cmd::AppDir(c) => {
                tokio::spawn(super::appdir::run(ctx.clone(), c));
            }
            Cmd::SafetyHub(c) => {
                tokio::spawn(super::safety::run(ctx.clone(), c));
            }
            Cmd::Saved(c) => {
                tokio::spawn(super::saved::run(ctx.clone(), c));
            }
            Cmd::Suggestions(c) => {
                tokio::spawn(super::suggestions::run(ctx.clone(), c));
            }
            Cmd::Inventory(c) => {
                tokio::spawn(super::inventory::run(ctx.clone(), c));
            }
            Cmd::Shop(c) => {
                tokio::spawn(super::shop::run(ctx.clone(), c));
            }
            Cmd::Tags(c) => {
                tokio::spawn(super::tags::run(ctx.clone(), c));
            }
            Cmd::Widgets(c) => {
                tokio::spawn(super::widgets::run(ctx.clone(), c));
            }
            Cmd::Effects(c) => {
                tokio::spawn(super::effects::run(ctx.clone(), c));
            }
            Cmd::Quests(c) => {
                tokio::spawn(super::quests::run(ctx.clone(), c));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_date_header() {
        // Exemple de la RFC 9110.
        assert_eq!(http_date_ms("Sun, 06 Nov 1994 08:49:37 GMT"), Some(784_111_777_000));
        assert_eq!(http_date_ms("Thu, 01 Jan 1970 00:00:01 GMT"), Some(1000));
        for bad in ["", "hier", "1994-11-06T08:49:37Z"] {
            assert_eq!(http_date_ms(bad), None, "{bad}");
        }
    }

    #[test]
    fn body_of_429_is_bounded() {
        let ms = Duration::from_millis;
        assert_eq!(parse_429(r#"{"retry_after": 0.25, "global": false}"#), (Some(ms(250)), false, true));
        assert_eq!(parse_429(r#"{"retry_after": -5, "global": true}"#), (Some(Duration::ZERO), true, true));
        assert_eq!(parse_429(r#"{"retry_after": 1e300}"#), (Some(Duration::from_secs(3600)), false, true));
        assert_eq!(parse_429(r#"{"retry_after": "bientôt", "global": "oui"}"#), (None, false, true));
        // Page du pare-feu : ni objet JSON, ni valeurs.
        assert_eq!(parse_429("pas du json"), (None, false, false));
        assert_eq!(parse_429("<html>error code: 1015</html>"), (None, false, false));
        assert_eq!(parse_429(""), (None, false, false));
    }

    #[test]
    fn http_retry_after_header() {
        let s = Duration::from_secs;
        assert_eq!(http_retry_after("30", 0), Some(s(30)));
        assert_eq!(http_retry_after(" 2 ", 0), Some(s(2)));
        // Date HTTP : relative à l'heure du serveur.
        assert_eq!(http_retry_after("Thu, 01 Jan 1970 00:01:00 GMT", 30_000), Some(s(30)));
        assert_eq!(http_retry_after("Thu, 01 Jan 1970 00:00:00 GMT", 30_000), Some(Duration::ZERO));
        assert_eq!(http_retry_after("99999999999", 0), Some(s(3600)));
        assert_eq!(http_retry_after("demain", 0), None);
    }

    #[test]
    fn send_body_carries_the_nonce_and_enforces_it() {
        let b = send_body("salut", 1234);
        assert_eq!(b["nonce"], "1234");
        assert_eq!(b["enforce_nonce"], true);
        assert_eq!(b["content"], "salut");
        assert_eq!(b.as_object().map(|o| o.len()), Some(4));
        assert_eq!(b["tts"], false);
        // `@silent` et `/tts` en tête du texte deviennent des champs du corps.
        let s = send_body("@silent chut", 1);
        assert_eq!((s["content"].as_str(), s["flags"].as_u64(), s["tts"].as_bool()), (Some("chut"), Some(4096), Some(false)));
        let t = send_body("/tts bonjour", 1);
        assert_eq!((t["content"].as_str(), t.get("flags"), t["tts"].as_bool()), (Some("bonjour"), None, Some(true)));
    }
}

