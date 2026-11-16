//! Santé d'une connexion (lot « fiable ») : compteurs de ce qu'un client
//! propre ne devrait presque jamais produire (4xx, 429, 5xx, nouveaux essais,
//! reconnexions), et mémoire négative des ressources refusées.
//!
//! Un client qui redemande en boucle une ressource que Discord lui refuse
//! (403, 404) se distingue d'un client normal, et s'approche du blocage
//! d'adresse (10 000 requêtes invalides par 10 minutes, documentation
//! « Rate Limits »). La mémoire négative répond à sa place, sans requête.
//!
//! Rien d'identifiant n'entre ici : des nombres, et des adresses d'API
//! gardées en mémoire seulement (jamais écrites ni journalisées).

use crate::state::Problem;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering::Relaxed},
        Arc, Mutex, Weak,
    },
    time::{Duration, Instant},
};

/// Compteurs d'une connexion, depuis son ouverture.
#[derive(Debug, Default)]
pub struct Health {
    client_errors: AtomicU64,
    rate_limited: AtomicU64,
    server_errors: AtomicU64,
    retries: AtomicU64,
    spared: AtomicU64,
    requests: AtomicU64,
    sessions: AtomicU64,
    identifies: AtomicU64,
}

/// Relevé des compteurs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// Requêtes REST parties.
    pub requests: u64,
    /// Réponses 4xx, hors 429.
    pub client_errors: u64,
    /// Réponses 429.
    pub rate_limited: u64,
    /// Réponses 5xx.
    pub server_errors: u64,
    /// Nouveaux essais automatiques (lectures seulement).
    pub retries: u64,
    /// Requêtes épargnées par la mémoire négative.
    pub spared: u64,
    /// Connexions ouvertes à la passerelle.
    pub sessions: u64,
    /// dont Identify (le reste : reprises).
    pub identifies: u64,
}

impl Snapshot {
    /// 4xx et 429 : ce qu'un usage normal ne doit pas produire.
    pub fn refused(&self) -> u64 {
        self.client_errors + self.rate_limited
    }
}

/// Connexion la plus récente (l'interface n'en a qu'une à la fois).
static CURRENT: Mutex<Weak<Health>> = Mutex::new(Weak::new());

impl Health {
    /// Compteurs neufs, déclarés comme ceux de la connexion en cours.
    pub fn start() -> Arc<Health> {
        let h = Arc::new(Health::default());
        *CURRENT.lock().unwrap_or_else(|e| e.into_inner()) = Arc::downgrade(&h);
        h
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            requests: self.requests.load(Relaxed),
            client_errors: self.client_errors.load(Relaxed),
            rate_limited: self.rate_limited.load(Relaxed),
            server_errors: self.server_errors.load(Relaxed),
            retries: self.retries.load(Relaxed),
            spared: self.spared.load(Relaxed),
            sessions: self.sessions.load(Relaxed),
            identifies: self.identifies.load(Relaxed),
        }
    }

    pub(super) fn request(&self) {
        self.requests.fetch_add(1, Relaxed);
    }

    /// Note le statut d'une réponse REST.
    pub(super) fn status(&self, status: u16) {
        match status {
            429 => self.rate_limited.fetch_add(1, Relaxed),
            400..=499 => self.client_errors.fetch_add(1, Relaxed),
            500..=599 => self.server_errors.fetch_add(1, Relaxed),
            _ => 0,
        };
    }

    pub(super) fn retry(&self) {
        self.retries.fetch_add(1, Relaxed);
    }

    pub(super) fn spared(&self) {
        self.spared.fetch_add(1, Relaxed);
    }

    pub(super) fn session(&self, identify: bool) {
        self.sessions.fetch_add(1, Relaxed);
        if identify {
            self.identifies.fetch_add(1, Relaxed);
        }
    }
}

/// Relevé de la connexion en cours (page Paramètres › Avancés) ; `None` hors connexion.
pub fn current() -> Option<Snapshot> {
    CURRENT.lock().unwrap_or_else(|e| e.into_inner()).upgrade().map(|h| h.snapshot())
}

// ====================================================================== mémoire négative

/// Première attente avant de redemander une ressource refusée.
pub const REFUSAL_FIRST: Duration = Duration::from_secs(10);
/// Attente la plus longue (elle double à chaque nouveau refus).
pub const REFUSAL_MAX: Duration = Duration::from_secs(300);
/// Sans nouveau refus pendant cette durée, la ressource est oubliée.
pub const REFUSAL_FORGET: Duration = Duration::from_secs(900);
/// Ressources retenues, au plus.
pub const REFUSAL_ENTRIES: usize = 128;

/// Refus retenu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    Forbidden,
    NotFound(Problem),
}

struct Entry {
    refusal: Refusal,
    until: Instant,
    last: Instant,
    strikes: u32,
}

/// Lectures (GET) refusées par un 403 ou un 404, par adresse : la même
/// lecture n'est pas renvoyée avant 10 s, puis 20, 40… jusqu'à 5 minutes.
/// L'utilisateur qui réessaie un peu plus tard obtient donc une vraie
/// réponse ; une boucle, elle, ne produit qu'une requête de loin en loin.
/// Toute écriture réussie vide la mémoire (elle a pu changer la réponse).
#[derive(Default)]
pub struct Refusals {
    map: HashMap<String, Entry>,
}

impl Refusals {
    /// Refus encore en vigueur pour cette adresse.
    pub fn lookup(&self, url: &str, now: Instant) -> Option<Refusal> {
        self.map.get(url).filter(|e| e.until > now).map(|e| e.refusal.clone())
    }

    /// Note un 403 ou un 404 reçu pour cette lecture.
    pub fn note(&mut self, url: &str, refusal: Refusal, now: Instant) {
        self.map.retain(|_, e| now.saturating_duration_since(e.last) < REFUSAL_FORGET);
        if !self.map.contains_key(url) && self.map.len() >= REFUSAL_ENTRIES {
            // Table pleine : la plus ancienne cède sa place.
            if let Some(oldest) = self.map.iter().min_by_key(|(_, e)| e.last).map(|(k, _)| k.clone()) {
                self.map.remove(&oldest);
            }
        }
        let strikes = self.map.get(url).map_or(0, |e| e.strikes.saturating_add(1));
        let wait = REFUSAL_FIRST.checked_mul(1 << strikes.min(8)).unwrap_or(REFUSAL_MAX).min(REFUSAL_MAX);
        self.map.insert(url.to_string(), Entry { refusal, until: now + wait, last: now, strikes });
    }

    /// La lecture a fini par aboutir : plus rien à retenir.
    pub fn forget(&mut self, url: &str) {
        self.map.remove(url);
    }

    /// Une écriture a abouti : les réponses retenues ne valent plus.
    pub fn clear(&mut self) {
        self.map.clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const URL: &str = "https://exemple.invalid/api/v10/channels/3000000000000000011/messages?limit=50";

    #[test]
    fn a_refused_read_is_answered_from_memory_with_growing_waits() {
        let t0 = Instant::now();
        let s = Duration::from_secs;
        let mut r = Refusals::default();
        assert_eq!(r.lookup(URL, t0), None);
        r.note(URL, Refusal::Forbidden, t0);
        assert_eq!(r.lookup(URL, t0 + s(9)), Some(Refusal::Forbidden));
        // Après 10 s : une vraie requête repart (réessai de l'utilisateur).
        assert_eq!(r.lookup(URL, t0 + s(10)), None);
        // Encore refusée : 20 s, puis 40 s… plafonnées à 5 minutes.
        r.note(URL, Refusal::Forbidden, t0 + s(10));
        assert_eq!(r.lookup(URL, t0 + s(29)), Some(Refusal::Forbidden));
        assert_eq!(r.lookup(URL, t0 + s(30)), None);
        let mut at = t0 + s(30);
        for _ in 0..12 {
            r.note(URL, Refusal::Forbidden, at);
            at += REFUSAL_MAX;
        }
        assert_eq!(r.lookup(URL, at - s(1)), Some(Refusal::Forbidden));
        assert_eq!(r.lookup(URL, at), None, "jamais plus de 5 minutes");
        // Une autre adresse n'est pas touchée.
        assert_eq!(r.lookup("https://exemple.invalid/autre", t0 + s(1)), None);
    }

    #[test]
    fn success_or_a_write_clears_what_was_remembered() {
        let t0 = Instant::now();
        let mut r = Refusals::default();
        let gone = Refusal::NotFound(Problem::Http(404));
        r.note(URL, gone.clone(), t0);
        assert_eq!(r.lookup(URL, t0), Some(gone.clone()));
        r.forget(URL);
        assert_eq!(r.lookup(URL, t0), None);
        r.note(URL, gone, t0);
        r.clear();
        assert_eq!(r.len(), 0);
    }

    #[test]
    fn the_table_is_bounded_and_forgets_old_refusals() {
        let t0 = Instant::now();
        let mut r = Refusals::default();
        for i in 0..REFUSAL_ENTRIES * 3 {
            r.note(&format!("u{i}"), Refusal::Forbidden, t0 + Duration::from_millis(i as u64));
        }
        assert_eq!(r.len(), REFUSAL_ENTRIES);
        assert_eq!(r.lookup("u0", t0), None, "les plus anciennes ont cédé leur place");
        // Un quart d'heure sans refus : tout est oublié au refus suivant, et le compteur repart.
        r.note("neuf", Refusal::Forbidden, t0 + REFUSAL_FORGET + Duration::from_secs(1));
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn statuses_are_counted_by_family() {
        let h = Health::default();
        for s in [200, 204, 304, 400, 403, 404, 429, 429, 500, 503] {
            h.request();
            h.status(s);
        }
        h.retry();
        h.spared();
        h.session(true);
        h.session(false);
        let s = h.snapshot();
        assert_eq!(s, Snapshot { requests: 10, client_errors: 3, rate_limited: 2, server_errors: 2, retries: 1, spared: 1, sessions: 2, identifies: 1 });
        assert_eq!(s.refused(), 5);
    }
}
