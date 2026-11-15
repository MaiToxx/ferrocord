//! Requête de traduction d'un message (plugin du lot E3) vers le service
//! compatible LibreTranslate que l'utilisateur a saisi. Ce n'est pas une
//! requête à Discord : elle ne passe ni par son client HTTP ni par ses
//! en-têtes, et ne porte aucun jeton.
//!
//! - Une requête par clic sur « Traduire » ; jamais en masse, jamais seule.
//! - Délai d'expiration [`TIMEOUT`], réponse bornée à [`MAX_ANSWER`], aucune
//!   redirection suivie (le texte n'irait pas ailleurs que chez le service choisi).
//! - La clé éventuelle voyage dans le corps (`api_key`, comme le documente
//!   LibreTranslate) et n'apparaît dans aucune erreur ni aucun journal.
//! - Un fil par requête, qui s'arrête avec elle : rien ne tourne au repos.

use crate::state::translate::{self as model, Failure};
use std::sync::{mpsc, Arc};
use std::time::Duration;

/// Délai d'expiration d'une traduction, connexion comprise.
pub const TIMEOUT: Duration = Duration::from_secs(15);
/// Taille maximale d'une réponse.
pub const MAX_ANSWER: usize = 256 * 1024;

/// Une traduction demandée.
#[derive(Clone, PartialEq, Eq)]
pub struct Request {
    /// Adresse complète (`…/translate`), déjà vérifiée par `state::translate::endpoint`.
    pub endpoint: String,
    pub text: String,
    pub target: String,
    pub key: Option<String>,
}

// La clé ne s'affiche jamais, même dans un message de test.
impl std::fmt::Debug for Request {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Request").field("endpoint", &self.endpoint).field("target", &self.target).field("key", &self.key.as_ref().map(|_| "…")).finish_non_exhaustive()
    }
}

/// Sa réponse : (texte traduit, langue reconnue).
pub type Answer = Result<(String, Option<String>), Failure>;

/// Envoie la requête et lit la réponse.
pub async fn fetch(req: &Request, timeout: Duration) -> Answer {
    let client = crate::platform::http_builder()
        .user_agent(concat!("Ferrocord/", env!("CARGO_PKG_VERSION")))
        .redirect(reqwest::redirect::Policy::none())
        .timeout(timeout)
        .build()
        .map_err(|_| Failure::Network)?;
    let sent = client.post(&req.endpoint).header(reqwest::header::CONTENT_TYPE, "application/json").header(reqwest::header::ACCEPT, "application/json").body(model::body(&req.text, &req.target, req.key.as_deref())).send().await;
    let mut resp = sent.map_err(|e| if e.is_timeout() { Failure::Timeout } else { Failure::Network })?;
    let status = resp.status();
    if !status.is_success() {
        return Err(model::failure_of(status.as_u16()));
    }
    if resp.content_length().is_some_and(|n| n > MAX_ANSWER as u64) {
        return Err(Failure::Answer);
    }
    let mut bytes = Vec::new();
    loop {
        match resp.chunk().await {
            Ok(Some(c)) => {
                if bytes.len() + c.len() > MAX_ANSWER {
                    return Err(Failure::Answer);
                }
                bytes.extend_from_slice(&c);
            }
            Ok(None) => break,
            Err(e) => return Err(if e.is_timeout() { Failure::Timeout } else { Failure::Network }),
        }
    }
    model::parse(&bytes)
}

/// Lance la requête sur son propre fil (qui s'arrête avec elle) ; la réponse
/// arrive dans `out` avec `tag`, puis `wake` réveille l'interface. Faux si
/// le fil n'a pas pu naître.
pub fn spawn<T: Send + 'static>(req: Request, tag: T, out: mpsc::Sender<(T, Answer)>, wake: Arc<dyn Fn() + Send + Sync>) -> bool {
    std::thread::Builder::new()
        .name("ferrocord-traduction".into())
        .spawn(move || {
            let answer = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                Ok(rt) => rt.block_on(fetch(&req, TIMEOUT)),
                Err(_) => Err(Failure::Network),
            };
            let _ = out.send((tag, answer));
            wake();
        })
        .is_ok()
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// Faux service sur 127.0.0.1 : note chaque requête reçue (ligne de
    /// requête, en-têtes et corps) et répond `status` avec `body`, après `delay`.
    pub(crate) async fn serve(status: &'static str, body: &'static str, delay: Duration, seen: Arc<Mutex<Vec<String>>>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            while let Ok((mut s, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut got = Vec::new();
                    let mut buf = vec![0u8; 8192];
                    // En-têtes, puis le corps annoncé par Content-Length.
                    loop {
                        let n = s.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            break;
                        }
                        got.extend_from_slice(&buf[..n]);
                        let text = String::from_utf8_lossy(&got).to_string();
                        if let Some(end) = text.find("\r\n\r\n") {
                            let len = text[..end].lines().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse::<usize>().ok())).unwrap_or(0);
                            if got.len() >= end + 4 + len {
                                break;
                            }
                        }
                    }
                    seen.lock().unwrap().push(String::from_utf8_lossy(&got).to_string());
                    tokio::time::sleep(delay).await;
                    let head = format!("HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                    let _ = s.write_all(head.as_bytes()).await;
                    let _ = s.write_all(body.as_bytes()).await;
                });
            }
        });
        format!("http://127.0.0.1:{}", addr.port())
    }

    fn request(base: &str, key: Option<&str>) -> Request {
        Request { endpoint: model::endpoint(base).unwrap(), text: "Bonjour à tous".into(), target: "en".into(), key: key.map(str::to_string) }
    }

    #[tokio::test]
    async fn translation_goes_to_the_chosen_service_only() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = serve("200 OK", r#"{"translatedText":"Hello everyone","detectedLanguage":{"confidence":97,"language":"fr"}}"#, Duration::ZERO, seen.clone()).await;
        let got = fetch(&request(&base, Some("cle-secrete")), TIMEOUT).await;
        assert_eq!(got, Ok(("Hello everyone".into(), Some("fr".into()))));
        let reqs = seen.lock().unwrap().clone();
        assert_eq!(reqs.len(), 1, "une requête par traduction");
        let r = &reqs[0];
        assert!(r.starts_with("POST /translate HTTP/1.1"), "{r}");
        let low = r.to_ascii_lowercase();
        assert!(!low.contains("authorization:") && !low.contains("cookie:") && !low.contains("x-super-properties"), "aucun en-tête de Discord : {r}");
        assert!(low.contains("user-agent: ferrocord/"), "se présente comme lui-même : {r}");
        let body: serde_json::Value = serde_json::from_str(r.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!((body["q"].as_str(), body["target"].as_str(), body["api_key"].as_str()), (Some("Bonjour à tous"), Some("en"), Some("cle-secrete")));
        assert!(!format!("{:?}", request(&base, Some("cle-secrete"))).contains("cle-secrete"), "la clé ne s'affiche pas");
    }

    #[tokio::test]
    async fn errors_are_classified_and_never_followed_elsewhere() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let refused = serve("403 Forbidden", r#"{"error":"Invalid API key"}"#, Duration::ZERO, seen.clone()).await;
        assert_eq!(fetch(&request(&refused, None), TIMEOUT).await, Err(Failure::Refused));
        let busy = serve("429 Too Many Requests", "{}", Duration::ZERO, seen.clone()).await;
        assert_eq!(fetch(&request(&busy, None), TIMEOUT).await, Err(Failure::Busy));
        let broken = serve("200 OK", "<html>pas du JSON</html>", Duration::ZERO, seen.clone()).await;
        assert_eq!(fetch(&request(&broken, None), TIMEOUT).await, Err(Failure::Answer));
        // Redirection : pas suivie (le texte ne part pas vers une autre adresse).
        let moved = serve("307 Temporary Redirect", "", Duration::ZERO, seen.clone()).await;
        assert_eq!(fetch(&request(&moved, None), TIMEOUT).await, Err(Failure::Service(307)));
        // Service muet : le délai d'expiration rend la main.
        let slow = serve("200 OK", r#"{"translatedText":"trop tard"}"#, Duration::from_secs(5), seen.clone()).await;
        let started = std::time::Instant::now();
        assert_eq!(fetch(&request(&slow, None), Duration::from_millis(300)).await, Err(Failure::Timeout));
        assert!(started.elapsed() < Duration::from_secs(3));
        // Rien n'écoute : échec de réseau, pas de blocage.
        let dead = {
            let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            format!("http://127.0.0.1:{}", l.local_addr().unwrap().port())
        };
        assert_eq!(fetch(&request(&dead, None), Duration::from_secs(5)).await, Err(Failure::Network));
        assert_eq!(seen.lock().unwrap().len(), 5);
    }

    #[test]
    fn spawned_request_answers_through_the_channel() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = rt.block_on(serve("200 OK", r#"{"translatedText":"Hello"}"#, Duration::ZERO, seen.clone()));
        let (tx, rx) = mpsc::channel();
        let woke = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let w = woke.clone();
        assert!(spawn(request(&base, None), 7u64, tx, Arc::new(move || w.store(true, std::sync::atomic::Ordering::SeqCst))));
        let (tag, answer) = rx.recv_timeout(Duration::from_secs(20)).unwrap();
        assert_eq!((tag, answer), (7, Ok(("Hello".into(), None))));
        std::thread::sleep(Duration::from_millis(50));
        assert!(woke.load(std::sync::atomic::Ordering::SeqCst));
    }
}
