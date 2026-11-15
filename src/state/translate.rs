//! Traduction d'un message (plugin du lot E3) : ce qui ne touche pas au
//! réseau. Adresse du service, forme de la requête et de la réponse (API
//! publique de LibreTranslate : `POST /translate`, corps JSON `q`, `source`,
//! `target`, `format`, `api_key` facultative ; réponse `translatedText` et
//! `detectedLanguage.language`), cache borné, et ce qui s'affiche sous un
//! message (`MessageMeta::translated`).
//!
//! Le service est celui que l'utilisateur a saisi dans les réglages du
//! plugin ; aucun service n'est intégré d'office.

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};

/// Caractères traduits au plus par message (la limite d'un message avec Nitro).
pub const MAX_TEXT: usize = 4000;
/// Traductions gardées au plus, et leur taille totale.
pub const CACHE_ENTRIES: usize = 200;
pub const CACHE_BYTES: usize = 256 * 1024;

/// Pourquoi une traduction n'a pas abouti.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Failure {
    /// Pas de réponse dans le délai.
    Timeout,
    /// Service injoignable.
    Network,
    /// Clé refusée ou exigée (401, 403).
    Refused,
    /// Trop de requêtes (429).
    Busy,
    /// Autre réponse d'erreur du service.
    Service(u16),
    /// Réponse illisible, ou trop grosse.
    Answer,
}

/// Ce qui s'affiche sous un message.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Shown {
    Pending,
    Done {
        text: String,
        /// Langue reconnue par le service.
        from: Option<String>,
        to: String,
    },
    Failed(Failure),
}

/// Adresse refusée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UrlError {
    Empty,
    /// Ni `https://`, ni `http://` vers cet ordinateur ou le réseau local.
    Scheme,
    Malformed,
}

/// Hôte d'une adresse `http://` où le texte ne quitte pas le réseau local.
fn local_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    if host.eq_ignore_ascii_case("localhost") || host == "::1" {
        return true;
    }
    match host.parse::<std::net::Ipv4Addr>() {
        Ok(ip) => ip.is_loopback() || ip.is_private() || ip.is_link_local(),
        Err(_) => false,
    }
}

/// Adresse de la requête de traduction d'après l'adresse saisie (celle du
/// service, avec ou sans `/translate`). `https://` exigé, sauf vers cet
/// ordinateur ou le réseau local.
pub fn endpoint(base: &str) -> Result<String, UrlError> {
    let base = base.trim();
    if base.is_empty() {
        return Err(UrlError::Empty);
    }
    if base.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(UrlError::Malformed);
    }
    let (secure, rest) = match (base.strip_prefix("https://"), base.strip_prefix("http://")) {
        (Some(r), _) => (true, r),
        (_, Some(r)) => (false, r),
        _ => return Err(UrlError::Scheme),
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    // Pas d'identifiants dans l'adresse (ils finiraient dans des journaux).
    if authority.is_empty() || authority.contains('@') || rest.contains(['?', '#']) {
        return Err(UrlError::Malformed);
    }
    let host = match authority.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or_default(),
        None => authority.rsplit_once(':').map_or(authority, |(h, _)| h),
    };
    if host.is_empty() {
        return Err(UrlError::Malformed);
    }
    if !secure && !local_host(host) {
        return Err(UrlError::Scheme);
    }
    let trimmed = base.trim_end_matches('/');
    Ok(if trimmed.ends_with("/translate") { trimmed.to_string() } else { format!("{trimmed}/translate") })
}

/// Corps JSON de la requête.
pub fn body(text: &str, target: &str, key: Option<&str>) -> String {
    let mut v = serde_json::json!({ "q": text, "source": "auto", "target": target, "format": "text" });
    if let Some(k) = key.filter(|k| !k.is_empty()) {
        v["api_key"] = serde_json::Value::String(k.to_string());
    }
    v.to_string()
}

/// Réponse du service : (texte traduit, langue reconnue).
pub fn parse(bytes: &[u8]) -> Result<(String, Option<String>), Failure> {
    let v: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Failure::Answer)?;
    let text = v.get("translatedText").and_then(|t| t.as_str()).ok_or(Failure::Answer)?;
    if text.trim().is_empty() {
        return Err(Failure::Answer);
    }
    let from = v.get("detectedLanguage").and_then(|d| d.get("language")).and_then(|l| l.as_str()).filter(|l| !l.is_empty() && l.len() <= 12 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')).map(str::to_string);
    Ok((text.chars().take(MAX_TEXT * 2).collect(), from))
}

/// Échec d'après le code HTTP d'une réponse d'erreur.
pub fn failure_of(status: u16) -> Failure {
    match status {
        401 | 403 => Failure::Refused,
        429 => Failure::Busy,
        s => Failure::Service(s),
    }
}

/// Clé du cache : message, langue cible, et texte (un message modifié est retraduit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    pub message: u64,
    target: u64,
    text: u64,
}

fn hash_of(s: &str) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl Key {
    pub fn new(message: u64, target: &str, text: &str) -> Key {
        Key { message, target: hash_of(target), text: hash_of(text) }
    }
}

/// Traductions déjà obtenues, bornées en nombre et en taille : les plus
/// anciennes partent. En mémoire seulement.
#[derive(Default)]
pub struct Cache {
    map: HashMap<Key, (String, Option<String>)>,
    order: VecDeque<Key>,
    bytes: usize,
}

impl Cache {
    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn get(&self, key: &Key) -> Option<&(String, Option<String>)> {
        self.map.get(key)
    }

    pub fn put(&mut self, key: Key, text: String, from: Option<String>) {
        if text.len() > CACHE_BYTES {
            return;
        }
        if let Some((old, _)) = self.map.remove(&key) {
            self.bytes -= old.len();
            self.order.retain(|k| *k != key);
        }
        self.bytes += text.len();
        self.map.insert(key, (text, from));
        self.order.push_back(key);
        while self.map.len() > CACHE_ENTRIES || self.bytes > CACHE_BYTES {
            let Some(old) = self.order.pop_front() else { break };
            if let Some((t, _)) = self.map.remove(&old) {
                self.bytes -= t.len();
            }
        }
    }

    pub fn clear(&mut self) {
        *self = Cache::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_requires_https_except_locally() {
        assert_eq!(endpoint("https://traduction.exemple.org"), Ok("https://traduction.exemple.org/translate".into()));
        assert_eq!(endpoint(" https://t.exemple.org/api/ "), Ok("https://t.exemple.org/api/translate".into()));
        assert_eq!(endpoint("https://t.exemple.org/translate"), Ok("https://t.exemple.org/translate".into()));
        assert_eq!(endpoint("http://127.0.0.1:5000"), Ok("http://127.0.0.1:5000/translate".into()));
        assert_eq!(endpoint("http://localhost:5000/"), Ok("http://localhost:5000/translate".into()));
        assert_eq!(endpoint("http://192.168.1.20:5000"), Ok("http://192.168.1.20:5000/translate".into()));
        assert_eq!(endpoint("http://[::1]:5000"), Ok("http://[::1]:5000/translate".into()));
        assert_eq!(endpoint("http://traduction.exemple.org"), Err(UrlError::Scheme), "en clair hors du réseau local");
        assert_eq!(endpoint("http://8.8.8.8"), Err(UrlError::Scheme));
        assert_eq!(endpoint("ftp://x"), Err(UrlError::Scheme));
        assert_eq!(endpoint("traduction.exemple.org"), Err(UrlError::Scheme));
        assert_eq!(endpoint(""), Err(UrlError::Empty));
        assert_eq!(endpoint("https://"), Err(UrlError::Malformed));
        assert_eq!(endpoint("https://moi:secret@t.exemple.org"), Err(UrlError::Malformed));
        assert_eq!(endpoint("https://t.exemple.org/?cle=1"), Err(UrlError::Malformed));
        assert_eq!(endpoint("https://t.exemple .org"), Err(UrlError::Malformed));
    }

    #[test]
    fn request_and_answer_shapes() {
        let b: serde_json::Value = serde_json::from_str(&body("Bonjour \"vous\"", "en", None)).unwrap();
        assert_eq!((b["q"].as_str(), b["source"].as_str(), b["target"].as_str(), b["format"].as_str()), (Some("Bonjour \"vous\""), Some("auto"), Some("en"), Some("text")));
        assert!(b.get("api_key").is_none(), "sans clé : le champ n'existe pas");
        let b: serde_json::Value = serde_json::from_str(&body("x", "fr", Some("abc"))).unwrap();
        assert_eq!(b["api_key"].as_str(), Some("abc"));
        assert_eq!(parse(br#"{"translatedText":"Hello","detectedLanguage":{"confidence":90,"language":"fr"}}"#), Ok(("Hello".into(), Some("fr".into()))));
        assert_eq!(parse(br#"{"translatedText":"Hello"}"#), Ok(("Hello".into(), None)));
        assert_eq!(parse(br#"{"error":"Invalid API key"}"#), Err(Failure::Answer));
        assert_eq!(parse(b"<html>"), Err(Failure::Answer));
        assert_eq!(parse(br#"{"translatedText":"  "}"#), Err(Failure::Answer));
        assert_eq!((failure_of(403), failure_of(429), failure_of(500)), (Failure::Refused, Failure::Busy, Failure::Service(500)));
    }

    #[test]
    fn cache_is_bounded_and_keyed_by_text() {
        let mut c = Cache::default();
        let k = Key::new(1, "en", "bonjour");
        c.put(k, "hello".into(), Some("fr".into()));
        assert_eq!(c.get(&k), Some(&("hello".into(), Some("fr".into()))));
        assert!(c.get(&Key::new(1, "en", "bonjour à tous")).is_none(), "message modifié : autre clé");
        assert!(c.get(&Key::new(1, "de", "bonjour")).is_none());
        for n in 0..(CACHE_ENTRIES as u64 + 10) {
            c.put(Key::new(100 + n, "en", "x"), "y".into(), None);
        }
        assert_eq!(c.len(), CACHE_ENTRIES);
        assert!(c.get(&k).is_none(), "la plus ancienne est partie");
        let big = "z".repeat(CACHE_BYTES / 2 + 1);
        c.put(Key::new(1, "en", "a"), big.clone(), None);
        c.put(Key::new(2, "en", "a"), big, None);
        assert_eq!(c.len(), 1, "borné en taille aussi");
        c.put(Key::new(3, "en", "a"), "z".repeat(CACHE_BYTES + 1), None);
        assert!(c.get(&Key::new(3, "en", "a")).is_none());
        c.clear();
        assert!(c.is_empty());
    }
}
