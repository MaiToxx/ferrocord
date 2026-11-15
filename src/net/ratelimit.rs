//! Limites de débit REST par compartiment (documentation publique de
//! Discord, « Rate Limits »).
//!
//! Chaque réponse annonce son compartiment (`X-RateLimit-Bucket`), sa limite,
//! ce qu'il en reste et quand il se recharge. Une route (méthode + chemin,
//! paramètre majeur compris : salon ou serveur) est associée à son
//! compartiment au fil des réponses, et une requête attend AVANT de partir
//! si son compartiment est vide : le but est de ne jamais recevoir de 429.
//!
//! Le cœur ([`Limiter`]) est pur : l'heure est passée en paramètre. La
//! couche asynchrone ([`RateLimits`]) ne fait qu'attendre ce qu'il demande.

use std::{
    collections::{HashMap, VecDeque},
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::Notify;

/// Limite globale documentée : 50 requêtes par seconde.
pub const GLOBAL_PER_SECOND: usize = 50;
/// Bornes des tables (règle 1.3.3).
pub const MAX_ROUTES: usize = 64;
pub const MAX_BUCKETS: usize = 512;
/// Requêtes en attente ou en cours, au plus ; au-delà, refus immédiat.
pub const MAX_OUTSTANDING: usize = 256;
/// Attente la plus longue qu'on accepte de lire dans un en-tête ou un 429
/// (au-delà, la valeur est une anomalie ; la requête abandonne de toute façon
/// plus tôt, à son plafond d'attente).
const MAX_HEADER_WAIT: Duration = Duration::from_secs(3600);
/// Pause d'un compartiment dont on ne sait plus rien (réponses perdues).
pub const PRUDENT_PAUSE: Duration = Duration::from_secs(1);
/// Requêtes invalides (401, 403, 429) : Discord bloque l'adresse au-delà de
/// 10 000 par 10 minutes (documentation). On s'arrête bien avant.
pub const INVALID_WINDOW: Duration = Duration::from_secs(600);
pub const INVALID_PAUSE_AT: usize = 1000;
/// Blocage sans `Retry-After` lisible : attente par défaut.
pub const BLOCK_DEFAULT: Duration = Duration::from_secs(60);

/// Une route de l'API et son paramètre majeur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Route {
    /// Méthode et chemin sans identifiants (`GET /channels/{}/messages`).
    pub template: &'static str,
    /// Salon, serveur ou webhook : deux salons n'ont jamais le même compartiment.
    pub major: u64,
}

impl Route {
    pub const fn channel_messages_get(channel: u64) -> Self {
        Self { template: "GET /channels/{}/messages", major: channel }
    }

    /// `GET /users/{id}` : sans paramètre majeur.
    pub const fn user_get() -> Self {
        Self { template: "GET /users/{}", major: 0 }
    }

    pub const fn dm_create() -> Self {
        Self { template: "POST /users/@me/channels", major: 0 }
    }

    pub const fn guild_member_get(guild: u64) -> Self {
        Self { template: "GET /guilds/{}/members/{}", major: guild }
    }

    pub const fn channel_messages_post(channel: u64) -> Self {
        Self { template: "POST /channels/{}/messages", major: channel }
    }

    pub const fn channel_typing(channel: u64) -> Self {
        Self { template: "POST /channels/{}/typing", major: channel }
    }
}

/// En-têtes de limite de débit d'une réponse, et contenu d'un 429.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Headers {
    pub bucket: Option<String>,
    pub limit: Option<u32>,
    pub remaining: Option<u32>,
    pub reset_after: Option<Duration>,
    pub global: bool,
    pub scope: Option<String>,
}

impl Headers {
    /// Lit les en-têtes par `get` (nom en minuscules). Toute valeur
    /// illisible est traitée comme absente.
    pub fn read<'a>(get: impl Fn(&str) -> Option<&'a str>) -> Self {
        let num = |n: &str| get(n).and_then(|v| v.trim().parse::<u32>().ok());
        let bucket = get("x-ratelimit-bucket")
            .map(str::trim)
            .filter(|b| !b.is_empty() && b.len() <= 64 && b.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
            .map(String::from);
        Self {
            bucket,
            limit: num("x-ratelimit-limit"),
            remaining: num("x-ratelimit-remaining"),
            reset_after: get("x-ratelimit-reset-after").and_then(|v| seconds(v.trim().parse().ok())),
            global: get("x-ratelimit-global").is_some_and(|v| v.trim().eq_ignore_ascii_case("true")),
            scope: get("x-ratelimit-scope").map(str::trim).filter(|s| s.len() <= 16).map(String::from),
        }
    }

    /// Au moins un en-tête de limite de débit présent ?
    fn any(&self) -> bool {
        self.bucket.is_some() || self.limit.is_some() || self.remaining.is_some() || self.reset_after.is_some() || self.global || self.scope.is_some()
    }

    fn complete(&self) -> bool {
        self.bucket.is_some() && self.limit.is_some() && self.remaining.is_some() && self.reset_after.is_some()
    }
}

/// Durée lue sur le réseau : `Duration::from_secs_f64` paniquerait sur une
/// valeur négative ou non finie.
pub fn seconds(v: Option<f64>) -> Option<Duration> {
    match v {
        Some(s) if s.is_finite() => Some(Duration::from_secs_f64(s.clamp(0.0, MAX_HEADER_WAIT.as_secs_f64()))),
        _ => None,
    }
}

/// Réponse vue par le limiteur.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub status: u16,
    pub headers: Headers,
    /// 429 : `retry_after` (corps JSON), déjà borné.
    pub retry_after: Option<Duration>,
    /// 429 : champ `global` du corps.
    pub global_body: bool,
    /// 429 : en-tête HTTP standard `Retry-After` (secondes), en dernier recours.
    pub http_retry_after: Option<Duration>,
    /// 429 : le corps est un objet JSON de l'API (même sans `retry_after`
    /// lisible) ; sinon (page HTML, vide) la réponse vient du pare-feu.
    pub api_body: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct BucketKey {
    /// Empreinte annoncée par Discord, ou la route tant qu'elle est inconnue.
    id: BucketId,
    major: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum BucketId {
    Hash(String),
    Route(&'static str),
}

#[derive(Debug, Default)]
struct Bucket {
    /// Limite et restant connus (en-têtes complets reçus).
    known: bool,
    limit: u32,
    remaining: u32,
    reset_at: Option<Instant>,
    in_flight: u32,
    /// Durée de fenêtre la plus longue annoncée (`Reset-After` maximal).
    span: Duration,
    /// Dernière recharge locale : une réponse dont l'échéance la précède
    /// appartient à la fenêtre d'avant.
    last_reload: Option<Instant>,
}

/// Décision pour une requête en attente.
#[derive(Debug, PartialEq, Eq)]
pub enum Poll {
    /// Partir maintenant ; le jeton est compté.
    Go(Permit),
    /// Réessayer à cette heure.
    WaitUntil(Instant),
    /// Attendre la réponse d'une autre requête (compartiment inconnu, ou
    /// requête précédente de la même file).
    WaitResponse,
    /// Discord bloque la connexion jusqu'à cette heure : abandon immédiat,
    /// sans nouvel essai automatique.
    Blocked(Instant),
}

/// Place réservée dans un compartiment ; à rendre par [`Limiter::complete`]
/// ou [`Limiter::release`].
#[derive(Debug, PartialEq, Eq)]
pub struct Permit {
    route: Route,
    key: BucketKey,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Ticket {
    id: u64,
    route: Route,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Full;

/// Compteurs (journal de débogage et tests).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Stats {
    /// 429 reçus, toutes portées.
    pub limited: u64,
    /// dont limite globale.
    pub global: u64,
    /// dont portée `shared` (compartiment partagé, pas de notre fait).
    pub shared: u64,
    /// dont 429 du pare-feu (sans corps de l'API ni en-tête de limite).
    pub blocked: u64,
}

#[derive(Default)]
pub struct Limiter {
    routes: HashMap<&'static str, String>,
    buckets: HashMap<BucketKey, Bucket>,
    /// File d'attente par route et paramètre majeur : l'ordre d'arrivée est préservé.
    queues: HashMap<Route, VecDeque<u64>>,
    outstanding: usize,
    next_ticket: u64,
    global_until: Option<Instant>,
    recent: VecDeque<Instant>,
    blocked_until: Option<Instant>,
    /// Réponses 401, 403 et 429 récentes (au plus `INVALID_PAUSE_AT`).
    invalid: VecDeque<Instant>,
    pub stats: Stats,
}

impl Limiter {
    fn key(&self, route: &Route) -> BucketKey {
        let id = match self.routes.get(route.template) {
            Some(h) => BucketId::Hash(h.clone()),
            None => BucketId::Route(route.template),
        };
        BucketKey { id, major: route.major }
    }

    /// Prend une place dans la file de `route`.
    pub fn enqueue(&mut self, route: Route) -> Result<Ticket, Full> {
        if self.outstanding >= MAX_OUTSTANDING {
            return Err(Full);
        }
        self.outstanding += 1;
        self.next_ticket += 1;
        let id = self.next_ticket;
        self.queues.entry(route).or_default().push_back(id);
        Ok(Ticket { id, route })
    }

    /// Abandon d'une requête qui attendait encore (plafond, annulation).
    pub fn cancel(&mut self, t: &Ticket) {
        if let Some(q) = self.queues.get_mut(&t.route)
            && let Some(i) = q.iter().position(|&x| x == t.id)
        {
            q.remove(i);
            self.outstanding = self.outstanding.saturating_sub(1);
            if q.is_empty() {
                self.queues.remove(&t.route);
            }
        }
    }

    pub fn poll(&mut self, t: &Ticket, now: Instant) -> Poll {
        if self.queues.get(&t.route).and_then(|q| q.front()) != Some(&t.id) {
            return Poll::WaitResponse;
        }
        if let Some(b) = self.blocked_until {
            if b > now {
                return Poll::Blocked(b);
            }
            self.blocked_until = None;
        }
        while self.invalid.front().is_some_and(|&x| now.saturating_duration_since(x) >= INVALID_WINDOW) {
            self.invalid.pop_front();
        }
        if self.invalid.len() >= INVALID_PAUSE_AT
            && let Some(&first) = self.invalid.front()
        {
            // Bien avant la limite documentée (10 000 par 10 min) : pause
            // jusqu'à ce que la plus ancienne sorte de la fenêtre.
            let until = first + INVALID_WINDOW;
            self.blocked_until = Some(until);
            return Poll::Blocked(until);
        }
        if let Some(g) = self.global_until {
            if g > now {
                return Poll::WaitUntil(g);
            }
            self.global_until = None;
        }
        while self.recent.front().is_some_and(|&x| now.saturating_duration_since(x) >= Duration::from_secs(1)) {
            self.recent.pop_front();
        }
        if self.recent.len() >= GLOBAL_PER_SECOND
            && let Some(&first) = self.recent.front()
        {
            return Poll::WaitUntil(first + Duration::from_secs(1));
        }
        let key = self.key(&t.route);
        if matches!(key.id, BucketId::Route(_))
            && let Some(wait) = self.unknown_route_wait(key.major, now)
        {
            return wait;
        }
        if let Some(b) = self.buckets.get_mut(&key) {
            if b.reset_at.is_some_and(|r| r <= now) {
                // Échéance de la fenêtre : seule occasion de recharger le crédit.
                b.reset_at = None;
                if b.known {
                    b.remaining = b.limit.saturating_sub(b.in_flight);
                    b.last_reload = Some(now);
                }
            }
            if !b.known {
                // Compartiment inconnu : une seule requête à la fois, jusqu'à
                // ce qu'une réponse apprenne sa limite.
                if b.in_flight > 0 {
                    return Poll::WaitResponse;
                }
                if let Some(r) = b.reset_at {
                    return Poll::WaitUntil(r);
                }
            } else if b.remaining == 0 {
                return match b.reset_at {
                    Some(r) => Poll::WaitUntil(r),
                    // Vide sans heure de recharge : la réponse en cours la donnera.
                    None if b.in_flight > 0 => Poll::WaitResponse,
                    // Vide, sans heure de recharge ni réponse attendue (réponses
                    // perdues) : jamais de recharge supposée. Une requête à la
                    // fois, après une pause, jusqu'à ce qu'une réponse renseigne.
                    None => {
                        b.known = false;
                        let at = now + PRUDENT_PAUSE;
                        b.reset_at = Some(at);
                        Poll::WaitUntil(at)
                    }
                };
            }
        } else if self.buckets.len() >= MAX_BUCKETS && !self.make_room(now) {
            // Table pleine de compartiments épuisés : attendre la première recharge.
            let next = self.buckets.values().filter_map(|b| b.reset_at).min().unwrap_or(now + PRUDENT_PAUSE);
            return Poll::WaitUntil(next.max(now + Duration::from_millis(1)));
        }
        self.go(t, key, now)
    }

    /// Route dont le compartiment n'est pas encore connu : elle peut partager
    /// celui d'une autre route du même paramètre majeur. Elle attend donc
    /// qu'aucune requête de ce majeur ne soit en cours ni épuisée.
    fn unknown_route_wait(&self, major: u64, now: Instant) -> Option<Poll> {
        let mut busy = false;
        let mut until: Option<Instant> = None;
        for (k, b) in &self.buckets {
            if k.major != major || matches!(k.id, BucketId::Route(_)) {
                continue;
            }
            if b.in_flight > 0 {
                busy = true;
            }
            if (b.remaining == 0 || !b.known)
                && let Some(r) = b.reset_at.filter(|&r| r > now)
            {
                until = Some(until.map_or(r, |u| u.max(r)));
            }
        }
        match (until, busy) {
            (Some(u), _) => Some(Poll::WaitUntil(u)),
            (None, true) => Some(Poll::WaitResponse),
            (None, false) => None,
        }
    }

    fn go(&mut self, t: &Ticket, key: BucketKey, now: Instant) -> Poll {
        let b = self.buckets.entry(key.clone()).or_default();
        b.in_flight += 1;
        if b.known {
            b.remaining = b.remaining.saturating_sub(1);
        }
        if let Some(q) = self.queues.get_mut(&t.route) {
            q.pop_front();
            if q.is_empty() {
                self.queues.remove(&t.route);
            }
        }
        if self.recent.len() >= GLOBAL_PER_SECOND {
            self.recent.pop_front();
        }
        self.recent.push_back(now);
        Poll::Go(Permit { route: t.route, key })
    }

    /// Fait de la place sous `MAX_BUCKETS` ; `false` s'il n'y en a pas.
    /// Ne retire jamais un compartiment avec une requête en cours, ni un
    /// compartiment épuisé ou en pause dont l'échéance n'est pas passée :
    /// l'oublier ferait repartir des requêtes trop tôt.
    fn make_room(&mut self, now: Instant) -> bool {
        let pinned = |b: &Bucket| b.in_flight > 0 || ((b.remaining == 0 || !b.known) && b.reset_at.is_some_and(|r| r > now));
        if self.buckets.len() >= MAX_BUCKETS {
            // D'abord ce qui n'apprend plus rien : rechargé, sans requête en cours.
            self.buckets.retain(|_, b| pinned(b) || b.reset_at.is_some_and(|r| r > now));
        }
        while self.buckets.len() >= MAX_BUCKETS {
            // Puis un compartiment qui a encore du crédit : redevenu inconnu, il
            // repart à une requête à la fois, sans risque de dépassement.
            let Some(k) = self.buckets.iter().filter(|(_, b)| !pinned(b)).min_by_key(|(_, b)| b.reset_at).map(|(k, _)| k.clone()) else {
                return false;
            };
            self.buckets.remove(&k);
        }
        true
    }

    /// Requête terminée sans réponse exploitable (erreur réseau, délai
    /// dépassé). Elle a pu être comptée par Discord : sa place reste
    /// consommée, et un compartiment inconnu marque une pause prudente.
    pub fn release(&mut self, p: Permit, now: Instant) {
        if let Some(b) = self.buckets.get_mut(&p.key) {
            b.in_flight = b.in_flight.saturating_sub(1);
            if !b.known && b.reset_at.is_none_or(|r| r < now + PRUDENT_PAUSE) {
                b.reset_at = Some(now + PRUDENT_PAUSE);
            }
        }
        self.outstanding = self.outstanding.saturating_sub(1);
    }

    /// Apprend de la réponse : compartiment de la route, restant, recharge,
    /// limite globale. Renvoie l'heure de fin d'un blocage (429 du pare-feu),
    /// pendant lequel aucune requête ne part ni n'est retentée.
    pub fn complete(&mut self, p: Permit, o: &Outcome, now: Instant) -> Option<Instant> {
        let h = &o.headers;
        if let Some(b) = self.buckets.get_mut(&p.key) {
            b.in_flight = b.in_flight.saturating_sub(1);
        }
        self.outstanding = self.outstanding.saturating_sub(1);
        if matches!(o.status, 401 | 403 | 429) {
            if self.invalid.len() >= INVALID_PAUSE_AT {
                self.invalid.pop_front();
            }
            self.invalid.push_back(now);
        }
        if o.status == 429 && !h.any() && !o.api_body && o.retry_after.is_none() && !o.global_body {
            // 429 sans corps de l'API ni en-tête de limite : pare-feu de Discord
            // ou blocage d'adresse. Tout s'arrête jusqu'à `Retry-After`.
            self.stats.limited += 1;
            self.stats.blocked += 1;
            let until = now + o.http_retry_after.unwrap_or(BLOCK_DEFAULT);
            let until = self.blocked_until.map_or(until, |b| b.max(until));
            self.blocked_until = Some(until);
            return Some(until);
        }
        let mut key = p.key.clone();
        if let Some(hash) = &h.bucket {
            if self.routes.get(p.route.template) != Some(hash) {
                if self.routes.len() >= MAX_ROUTES {
                    self.routes.clear();
                }
                self.routes.insert(p.route.template, hash.clone());
            }
            key = BucketKey { id: BucketId::Hash(hash.clone()), major: p.route.major };
            if key != p.key && self.buckets.get(&p.key).is_some_and(|b| b.in_flight == 0) {
                self.buckets.remove(&p.key);
            }
        }
        if !self.buckets.contains_key(&key) && !self.make_room(now) {
            // Aucune place : rien n'est appris de cette réponse (la requête
            // suivante de la route repartira prudemment, une à la fois).
            return None;
        }
        let b = self.buckets.entry(key).or_default();
        if let (true, Some(limit), Some(remaining), Some(reset_after)) = (h.complete(), h.limit, h.remaining, h.reset_after) {
            // Les autres requêtes déjà parties ont consommé leur place.
            let told = remaining.saturating_sub(b.in_flight);
            let end = now + reset_after;
            b.span = b.span.max(reset_after);
            // Réponse d'une fenêtre déjà rechargée (arrivée en retard) : elle
            // ne dit rien de la fenêtre en cours.
            let stale = b.last_reload.is_some_and(|lr| end <= lr + b.span / 2);
            if !stale {
                if b.known {
                    // Une réponse ne redonne jamais de crédit : elle peut être plus
                    // ancienne que d'autres déjà lues (réponses dans le désordre).
                    // Le crédit ne revient qu'à l'échéance, dans `poll`.
                    b.remaining = b.remaining.min(told);
                    b.reset_at = Some(b.reset_at.map_or(end, |r| r.max(end)));
                } else {
                    b.remaining = told;
                    b.reset_at = Some(end);
                }
                b.known = true;
                b.limit = limit;
            }
        } else if o.status != 429 {
            // En-têtes absents ou partiels : prudence, une requête à la fois.
            b.known = false;
        }
        if o.status == 429 {
            self.stats.limited += 1;
            let wait = o.retry_after.or(h.reset_after).unwrap_or(Duration::from_secs(1));
            let global = h.global || o.global_body || h.scope.as_deref() == Some("global");
            if h.scope.as_deref() == Some("shared") {
                self.stats.shared += 1;
            }
            if global {
                self.stats.global += 1;
                let until = now + wait;
                self.global_until = Some(self.global_until.map_or(until, |g| g.max(until)));
            } else {
                b.remaining = 0;
                let until = now + wait;
                b.reset_at = Some(b.reset_at.map_or(until, |r| r.max(until)));
            }
        }
        None
    }

    #[cfg(test)]
    fn sizes(&self) -> (usize, usize, usize, usize) {
        (self.routes.len(), self.buckets.len(), self.queues.len(), self.outstanding)
    }
}

// ====================================================================== couche asynchrone

/// Pourquoi une requête n'est pas partie.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// L'attente dépasserait le plafond de cette requête.
    TooLong,
    /// Annulée pendant l'attente (salon quitté…).
    Cancelled,
    /// Trop de requêtes en attente.
    Full,
    /// Discord bloque la connexion jusqu'à cette heure.
    Blocked(Instant),
}

/// Limiteur partagé entre les tâches REST d'une connexion.
#[derive(Default)]
pub struct RateLimits {
    inner: Mutex<Limiter>,
    notify: Notify,
}

/// Place obtenue ; rendue automatiquement si la requête est abandonnée.
pub struct Slot {
    lim: Arc<RateLimits>,
    permit: Option<Permit>,
}

impl Slot {
    /// Rend la place avec ce que la réponse apprend ; renvoie les compteurs
    /// et, si Discord bloque la connexion, l'heure de fin du blocage.
    pub fn complete(mut self, o: &Outcome) -> (Stats, Option<Instant>) {
        let mut res = (Stats::default(), None);
        if let Some(p) = self.permit.take() {
            res = self.lim.with(|l| {
                let blocked = l.complete(p, o, Instant::now());
                (l.stats, blocked)
            });
            self.lim.notify.notify_waiters();
        }
        res
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        if let Some(p) = self.permit.take() {
            self.lim.with(|l| l.release(p, Instant::now()));
            self.lim.notify.notify_waiters();
        }
    }
}

/// Retire le ticket de sa file si l'attente est abandonnée.
struct Waiting<'a> {
    lim: &'a RateLimits,
    ticket: Option<Ticket>,
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        if let Some(t) = self.ticket.take() {
            self.lim.with(|l| l.cancel(&t));
            self.lim.notify.notify_waiters();
        }
    }
}

impl RateLimits {
    fn with<R>(&self, f: impl FnOnce(&mut Limiter) -> R) -> R {
        // Un verrou empoisonné (panique d'une autre tâche) garde un état cohérent :
        // chaque opération est faite d'un bloc.
        let mut g = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut g)
    }

    /// Attend une place pour `route`, au plus jusqu'à `deadline`, et tant que
    /// `cancel` ne s'est pas terminé.
    pub async fn acquire(self: &Arc<Self>, route: Route, max_wait: Duration, cancel: impl Future<Output = ()>) -> Result<Slot, Refused> {
        let deadline = Instant::now() + max_wait;
        let ticket = self.with(|l| l.enqueue(route)).map_err(|Full| Refused::Full)?;
        let mut waiting = Waiting { lim: self, ticket: Some(ticket) };
        tokio::pin!(cancel);
        loop {
            let notified = self.notify.notified();
            tokio::pin!(notified);
            // Inscrit avant de lire l'état : aucun réveil ne peut être manqué.
            notified.as_mut().enable();
            let Some(t) = waiting.ticket.as_ref() else { return Err(Refused::Cancelled) };
            let now = Instant::now();
            let poll = self.with(|l| l.poll(t, now));
            let until = match poll {
                Poll::Go(permit) => {
                    waiting.ticket = None;
                    // La requête suivante de la même file peut maintenant passer.
                    self.notify.notify_waiters();
                    return Ok(Slot { lim: self.clone(), permit: Some(permit) });
                }
                Poll::Blocked(at) => return Err(Refused::Blocked(at)),
                Poll::WaitUntil(at) if at > deadline => return Err(Refused::TooLong),
                Poll::WaitUntil(at) => at,
                // Réveillé par la réponse attendue ; au plafond, un dernier examen.
                Poll::WaitResponse if now >= deadline => return Err(Refused::TooLong),
                Poll::WaitResponse => deadline,
            };
            tokio::select! {
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(until)) => {}
                _ = &mut notified => {}
                _ = &mut cancel => return Err(Refused::Cancelled),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: u64 = 3000000000000000011;
    const B: u64 = 3000000000000000012;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    fn headers(bucket: &str, limit: u32, remaining: u32, reset_after_ms: u64) -> Headers {
        Headers {
            bucket: Some(bucket.into()),
            limit: Some(limit),
            remaining: Some(remaining),
            reset_after: Some(ms(reset_after_ms)),
            global: false,
            scope: None,
        }
    }

    fn ok(h: Headers) -> Outcome {
        Outcome { status: 200, headers: h, ..Outcome::default() }
    }

    fn go(l: &mut Limiter, route: Route, now: Instant) -> Permit {
        let t = l.enqueue(route).unwrap_or_else(|_| panic!("file pleine"));
        match l.poll(&t, now) {
            Poll::Go(p) => p,
            other => panic!("départ attendu, reçu {other:?}"),
        }
    }

    #[test]
    fn headers_are_read_and_sanitized() {
        let h = Headers::read(|n| match n {
            "x-ratelimit-bucket" => Some("abcd1234"),
            "x-ratelimit-limit" => Some("5"),
            "x-ratelimit-remaining" => Some("4"),
            "x-ratelimit-reset-after" => Some("1.25"),
            "x-ratelimit-scope" => Some("user"),
            _ => None,
        });
        assert_eq!(h, Headers { scope: Some("user".into()), ..headers("abcd1234", 5, 4, 1250) });
        let bad = Headers::read(|n| match n {
            "x-ratelimit-bucket" => Some("../../etc"),
            "x-ratelimit-limit" => Some("-1"),
            "x-ratelimit-remaining" => Some("beaucoup"),
            "x-ratelimit-reset-after" => Some("NaN"),
            "x-ratelimit-global" => Some("TRUE"),
            _ => None,
        });
        assert_eq!(bad, Headers { global: true, ..Headers::default() });
        assert_eq!(seconds(Some(-3.0)), Some(Duration::ZERO));
        assert_eq!(seconds(Some(1e12)), Some(MAX_HEADER_WAIT));
        assert_eq!(seconds(Some(f64::INFINITY)), None);
    }

    #[test]
    fn bucket_is_learned_then_exhausted_without_429() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_post(A);
        let p = go(&mut l, r, t0);
        l.complete(p, &ok(headers("h1", 2, 1, 1000)), t0 + ms(10));
        // Une place restante : part ; ensuite, vide jusqu'à la recharge.
        let p = go(&mut l, r, t0 + ms(20));
        l.complete(p, &ok(headers("h1", 2, 0, 990)), t0 + ms(30));
        let t = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(40)), Poll::WaitUntil(t0 + ms(1020)));
        // Recharge : la limite entière revient.
        assert!(matches!(l.poll(&t, t0 + ms(1020)), Poll::Go(_)));
        // Même empreinte, autre salon : compartiment distinct, libre.
        let other = l.enqueue(Route::channel_messages_post(B)).unwrap_or_else(|_| panic!());
        assert!(matches!(l.poll(&other, t0 + ms(1021)), Poll::Go(_)));
        assert_eq!(l.stats, Stats::default());
    }

    #[test]
    fn remaining_accounts_for_requests_in_flight() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_get(A);
        let p = go(&mut l, r, t0);
        l.complete(p, &ok(headers("g", 5, 4, 5000)), t0);
        let p1 = go(&mut l, r, t0);
        let p2 = go(&mut l, r, t0);
        let p3 = go(&mut l, r, t0);
        // La réponse à la première annonce 3 restants, mais deux autres sont parties.
        l.complete(p1, &ok(headers("g", 5, 3, 5000)), t0);
        let p4 = go(&mut l, r, t0);
        let t = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0), Poll::WaitUntil(t0 + ms(5000)));
        for p in [p2, p3, p4] {
            l.release(p, t0);
        }
    }

    #[test]
    fn unknown_bucket_or_missing_headers_send_one_at_a_time() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_get(A);
        let p = go(&mut l, r, t0);
        // Seconde requête de la même route avant toute réponse : elle attend.
        let t = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0), Poll::WaitResponse);
        // Réponse sans en-têtes : toujours une à la fois, sans attente fixe.
        l.complete(p, &ok(Headers::default()), t0 + ms(5));
        let p = match l.poll(&t, t0 + ms(5)) {
            Poll::Go(p) => p,
            other => panic!("{other:?}"),
        };
        let t2 = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t2, t0 + ms(6)), Poll::WaitResponse);
        // En-têtes partiels : même prudence.
        l.complete(p, &ok(Headers { remaining: Some(3), ..Headers::default() }), t0 + ms(7));
        let p = match l.poll(&t2, t0 + ms(7)) {
            Poll::Go(p) => p,
            other => panic!("{other:?}"),
        };
        let t3 = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t3, t0 + ms(8)), Poll::WaitResponse);
        // Réponse perdue : la place reste consommée, pause prudente avant la suivante.
        l.release(p, t0 + ms(8));
        assert_eq!(l.poll(&t3, t0 + ms(9)), Poll::WaitUntil(t0 + ms(8) + PRUDENT_PAUSE));
        assert!(matches!(l.poll(&t3, t0 + ms(8) + PRUDENT_PAUSE), Poll::Go(_)));
    }

    #[test]
    fn global_429_pauses_every_route() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let p = go(&mut l, Route::channel_messages_get(A), t0);
        let limited = Outcome { status: 429, retry_after: Some(ms(800)), global_body: true, ..Outcome::default() };
        l.complete(p, &limited, t0 + ms(10));
        assert_eq!(l.stats, Stats { limited: 1, global: 1, ..Stats::default() });
        let tickets: Vec<Ticket> =
            [Route::channel_messages_post(B), Route::channel_messages_get(B)].into_iter().map(|r| l.enqueue(r).unwrap_or_else(|_| panic!())).collect();
        for t in &tickets {
            assert_eq!(l.poll(t, t0 + ms(20)), Poll::WaitUntil(t0 + ms(810)));
        }
        for t in &tickets {
            assert!(matches!(l.poll(t, t0 + ms(810)), Poll::Go(_)));
        }
        // L'en-tête seul suffit aussi à reconnaître un 429 global.
        let p = go(&mut l, Route::channel_messages_get(A), t0 + ms(900));
        let h = Headers { global: true, ..Headers::default() };
        l.complete(p, &Outcome { status: 429, headers: h, retry_after: Some(ms(100)), ..Outcome::default() }, t0 + ms(900));
        let t = l.enqueue(Route::channel_messages_post(B)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(900)), Poll::WaitUntil(t0 + ms(1000)));
    }

    #[test]
    fn bucket_429_pauses_only_its_bucket_and_is_counted() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let p = go(&mut l, Route::channel_messages_post(A), t0);
        let h = Headers { scope: Some("shared".into()), ..headers("h", 5, 0, 300) };
        l.complete(p, &Outcome { status: 429, headers: h, retry_after: Some(ms(500)), ..Outcome::default() }, t0);
        assert_eq!(l.stats, Stats { limited: 1, shared: 1, ..Stats::default() });
        let t = l.enqueue(Route::channel_messages_post(A)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(1)), Poll::WaitUntil(t0 + ms(500)));
        let other = go(&mut l, Route::channel_messages_post(B), t0 + ms(1));
        l.complete(other, &ok(headers("h", 5, 4, 300)), t0 + ms(2));
        // 429 de l'API sans en-têtes ni retry_after lisible : une seconde par prudence.
        let p = go(&mut l, Route::channel_messages_get(B), t0);
        l.complete(p, &Outcome { status: 429, api_body: true, ..Outcome::default() }, t0);
        let t = l.enqueue(Route::channel_messages_get(B)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0), Poll::WaitUntil(t0 + Duration::from_secs(1)));
    }

    #[test]
    fn queue_preserves_order_within_a_route() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_post(A);
        let p = go(&mut l, r, t0);
        l.complete(p, &ok(headers("h", 5, 0, 100)), t0);
        let t1 = l.enqueue(r).unwrap_or_else(|_| panic!());
        let t2 = l.enqueue(r).unwrap_or_else(|_| panic!());
        // Le second ne passe jamais avant le premier, même interrogé d'abord.
        assert_eq!(l.poll(&t2, t0 + ms(200)), Poll::WaitResponse);
        assert!(matches!(l.poll(&t1, t0 + ms(200)), Poll::Go(_)));
        assert!(matches!(l.poll(&t2, t0 + ms(200)), Poll::Go(_)));
    }

    #[test]
    fn cancelled_ticket_leaves_the_queue() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_get(A);
        let p = go(&mut l, r, t0);
        l.complete(p, &ok(headers("h", 1, 0, 60_000)), t0);
        let t1 = l.enqueue(r).unwrap_or_else(|_| panic!());
        let t2 = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t1, t0), Poll::WaitUntil(t0 + ms(60_000)));
        l.cancel(&t1);
        // Le suivant prend la tête de file.
        assert_eq!(l.poll(&t2, t0), Poll::WaitUntil(t0 + ms(60_000)));
        l.cancel(&t2);
        l.cancel(&t2);
        assert_eq!(l.sizes().2, 0, "file vidée");
        assert_eq!(l.sizes().3, 0, "plus rien en attente");
    }

    #[test]
    fn global_rate_of_50_per_second() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        for i in 0..50u64 {
            let p = go(&mut l, Route::channel_messages_get(i + 1), t0 + ms(i));
            l.complete(p, &ok(headers("h", 5, 4, 1000)), t0 + ms(i));
        }
        let t = l.enqueue(Route::channel_messages_get(999)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(100)), Poll::WaitUntil(t0 + ms(1000)));
        assert!(matches!(l.poll(&t, t0 + ms(1000)), Poll::Go(_)));
    }

    #[test]
    fn tables_stay_bounded() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        // Des milliers de salons et d'empreintes : aucune table ne dépasse sa borne.
        for i in 0..5000u64 {
            let now = t0 + Duration::from_secs(i);
            let p = go(&mut l, Route::channel_messages_get(i), now);
            l.complete(p, &ok(headers(&format!("h{i}"), 5, 4, 500_000)), now);
        }
        let (routes, buckets, queues, outstanding) = l.sizes();
        assert!(routes <= MAX_ROUTES && buckets <= MAX_BUCKETS, "{routes} {buckets}");
        assert_eq!((queues, outstanding), (0, 0));
        // Trop de requêtes en attente : refus immédiat, pas de croissance.
        let mut tickets = Vec::new();
        for _ in 0..MAX_OUTSTANDING {
            tickets.push(l.enqueue(Route::channel_messages_post(A)).unwrap_or_else(|_| panic!()));
        }
        assert_eq!(l.enqueue(Route::channel_messages_post(B)), Err(Full));
        for t in &tickets {
            l.cancel(t);
        }
        assert!(l.enqueue(Route::channel_messages_post(B)).is_ok());
    }

    #[tokio::test]
    async fn async_acquire_cap_and_cancel_are_immediate() {
        let lim = Arc::new(RateLimits::default());
        let r = Route::channel_messages_get(A);
        let slot = lim.acquire(r, ms(10), std::future::pending()).await.unwrap_or_else(|e| panic!("{e:?}"));
        slot.complete(&ok(headers("h", 1, 0, 60_000)));
        // Recharge dans 60 s, plafond de 10 s : refus tout de suite, sans attendre.
        let t = std::time::Instant::now();
        assert_eq!(lim.acquire(r, Duration::from_secs(10), std::future::pending()).await.err(), Some(Refused::TooLong));
        // Plafond suffisant mais annulation : refus dès qu'elle survient.
        assert_eq!(lim.acquire(r, Duration::from_secs(120), std::future::ready(())).await.err(), Some(Refused::Cancelled));
        assert!(t.elapsed() < Duration::from_secs(1));
        assert_eq!(lim.with(|l| l.sizes().3), 0, "tickets rendus");
        // Une place abandonnée sans réponse (requête interrompue) est rendue,
        // mais comptée : la suivante attend la pause prudente (refus au plafond de 10 ms).
        let other = Route::channel_messages_get(B);
        drop(lim.acquire(other, ms(10), std::future::pending()).await);
        assert_eq!(lim.acquire(other, ms(10), std::future::pending()).await.err(), Some(Refused::TooLong));
        assert_eq!(lim.with(|l| l.sizes().3), 0);
    }

    #[tokio::test]
    async fn second_request_of_a_route_is_woken_when_the_first_leaves() {
        let lim = Arc::new(RateLimits::default());
        let r = Route::channel_messages_post(A);
        let slot = lim.acquire(r, ms(10), std::future::pending()).await.unwrap_or_else(|e| panic!("{e:?}"));
        slot.complete(&ok(headers("h", 5, 4, 60_000)));
        // Deux requêtes en file : la seconde part dès que la première est partie,
        // sans attendre sa réponse (le compartiment a du crédit).
        let a = lim.acquire(r, ms(500), std::future::pending());
        let b = lim.acquire(r, ms(500), std::future::pending());
        let (a, b) = tokio::join!(a, b);
        assert!(a.is_ok() && b.is_ok(), "{:?} {:?}", a.as_ref().err(), b.as_ref().err());
    }

    #[test]
    fn late_response_never_gives_credit_back() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_post(A);
        let p = go(&mut l, r, t0);
        l.complete(p, &ok(headers("h", 5, 4, 1000)), t0 + ms(10));
        let (p1, p2, p3) = (go(&mut l, r, t0 + ms(20)), go(&mut l, r, t0 + ms(21)), go(&mut l, r, t0 + ms(22)));
        // Réponses dans le désordre : la plus récente d'abord (1 restante)…
        l.complete(p3, &ok(headers("h", 5, 1, 970)), t0 + ms(40));
        // … puis les plus anciennes (3, 2 restantes) : le crédit ne remonte pas.
        l.complete(p1, &ok(headers("h", 5, 3, 960)), t0 + ms(60));
        l.complete(p2, &ok(headers("h", 5, 2, 950)), t0 + ms(70));
        let t = l.enqueue(r).unwrap_or_else(|_| panic!());
        // Échéance : la plus tardive annoncée (60 + 960, 70 + 950).
        assert_eq!(l.poll(&t, t0 + ms(80)), Poll::WaitUntil(t0 + ms(1020)), "crédit redonné par une réponse ancienne");
        // Après l'échéance, recharge ; une réponse qui décrit la fenêtre
        // d'avant (échéance déjà passée) ne retire rien et ne prolonge rien.
        let end = t0 + ms(1020);
        let p = match l.poll(&t, end) {
            Poll::Go(p) => p,
            other => panic!("{other:?}"),
        };
        let late = go(&mut l, r, end);
        l.complete(late, &ok(headers("h", 5, 0, 0)), end + ms(5));
        let next = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert!(matches!(l.poll(&next, end + ms(6)), Poll::Go(_)));
        l.release(p, end);
    }

    #[test]
    fn lost_responses_never_refill_an_exhausted_bucket() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let r = Route::channel_messages_post(A);
        let p = go(&mut l, r, t0);
        l.complete(p, &ok(headers("h", 5, 4, 1000)), t0);
        // Quatre envois dont les réponses sont perdues : comptés quand même.
        for i in 0..4 {
            let p = go(&mut l, r, t0 + ms(i));
            l.release(p, t0 + ms(i + 1));
        }
        let t = l.enqueue(r).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(10)), Poll::WaitUntil(t0 + ms(1000)));
        l.cancel(&t);
        // Puis plus aucune réponse : après l'échéance, le crédit de la fenêtre
        // suivante est consommé, et faute d'heure de recharge connue le client
        // passe en pause prudente, une requête à la fois, sans jamais supposer
        // une recharge.
        let mut sent = 0;
        let mut now = t0 + ms(1000);
        let end = now + Duration::from_secs(10);
        while now < end {
            let t = l.enqueue(r).unwrap_or_else(|_| panic!());
            match l.poll(&t, now) {
                Poll::Go(p) => {
                    sent += 1;
                    l.release(p, now);
                }
                _ => l.cancel(&t),
            }
            now += ms(10);
        }
        // Les 5 de la fenêtre rechargée, puis une par pause prudente au plus,
        // au lieu de 5 par seconde supposées (55 en 10 s).
        assert!(sent <= 5 + 11, "{sent} envois sans aucune réponse en 10 s");
    }

    #[test]
    fn firewall_429_blocks_everything_without_retry() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        let p = go(&mut l, Route::channel_messages_post(A), t0);
        let o = Outcome { status: 429, http_retry_after: Some(Duration::from_secs(120)), ..Outcome::default() };
        assert_eq!(l.complete(p, &o, t0), Some(t0 + Duration::from_secs(120)));
        assert_eq!(l.stats.blocked, 1);
        let t = l.enqueue(Route::channel_messages_get(B)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(1)), Poll::Blocked(t0 + Duration::from_secs(120)));
        assert!(matches!(l.poll(&t, t0 + Duration::from_secs(120)), Poll::Go(_)));
        // Sans `Retry-After` : 60 s par défaut.
        let p = go(&mut l, Route::channel_messages_post(B), t0 + Duration::from_secs(200));
        assert_eq!(l.complete(p, &Outcome { status: 429, ..Outcome::default() }, t0 + Duration::from_secs(200)), Some(t0 + Duration::from_secs(260)));
        // Un 429 de l'API (corps JSON) n'est pas un blocage.
        let now = t0 + Duration::from_secs(300);
        let p = go(&mut l, Route::channel_messages_post(A), now);
        assert_eq!(l.complete(p, &Outcome { status: 429, retry_after: Some(ms(100)), ..Outcome::default() }, now), None);
    }

    #[test]
    fn many_invalid_requests_pause_well_before_the_documented_limit() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        // Une toutes les 20 ms (sous les 50 par seconde).
        for i in 0..INVALID_PAUSE_AT as u64 {
            let p = go(&mut l, Route::channel_messages_get(i), t0 + ms(20 * i));
            l.complete(p, &Outcome { status: 403, ..Outcome::default() }, t0 + ms(20 * i));
        }
        let t = l.enqueue(Route::channel_messages_get(7)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(20 * INVALID_PAUSE_AT as u64)), Poll::Blocked(t0 + INVALID_WINDOW));
        const { assert!(INVALID_PAUSE_AT * 10 <= 10_000) };
        assert!(l.invalid.len() <= INVALID_PAUSE_AT);
    }

    #[test]
    fn eviction_spares_exhausted_buckets() {
        let t0 = Instant::now();
        let mut l = Limiter::default();
        // Le compartiment de A est vide pour une heure.
        let p = go(&mut l, Route::channel_messages_post(A), t0);
        l.complete(p, &ok(headers("hA", 5, 0, 3_600_000)), t0);
        // Des milliers d'autres salons remplissent la table.
        // Des milliers d'autres salons remplissent la table (une requête toutes
        // les 25 ms, sous les 50 par seconde).
        let n = 3 * MAX_BUCKETS as u64;
        for i in 0..n {
            let now = t0 + ms(25 * i);
            let p = go(&mut l, Route::channel_messages_get(10_000 + i), now);
            l.complete(p, &ok(headers("g", 5, 4, 1000)), now);
        }
        assert!(l.sizes().1 <= MAX_BUCKETS);
        let t = l.enqueue(Route::channel_messages_post(A)).unwrap_or_else(|_| panic!());
        assert_eq!(l.poll(&t, t0 + ms(25 * n)), Poll::WaitUntil(t0 + Duration::from_secs(3600)));
    }

    // ------------------------------------------------------------ simulation

    /// Générateur pseudo-aléatoire déterministe (xorshift).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
    }

    /// Compartiment côté serveur : fenêtres fixes de `WINDOW_MS`.
    struct ServerBucket {
        start: u64,
        count: u32,
    }

    const LIMIT: u32 = 3;
    const WINDOW_MS: u64 = 1000;
    const TEMPLATES: [&str; 5] = ["GET /a/{}", "POST /a/{}", "GET /b/{}", "PATCH /b/{}", "DELETE /c/{}"];

    enum Ev {
        /// Requête n° `id` reçue par le serveur.
        Arrive { id: usize },
        /// Réponse rendue au client (`None` : perdue, délai dépassé).
        Respond { id: usize, outcome: Option<Outcome> },
    }

    /// 10 000 requêtes sur 100 routes (5 gabarits × 20 salons) partageant
    /// 20 compartiments, 5 % de réponses perdues, latences aléatoires (donc
    /// réponses dans le désordre). Le serveur compte les dépassements.
    fn simulate(seed: u64) -> (u64, u64, Stats) {
        let mut rng = Rng(seed);
        let t0 = Instant::now();
        let at = |ms: u64| t0 + Duration::from_millis(ms);
        let mut l = Limiter::default();
        const N: usize = 10_000;
        let routes: Vec<Route> = (0..N)
            .map(|_| Route { template: TEMPLATES[rng.below(5) as usize], major: rng.below(20) })
            .collect();
        // Arrivées : environ 40 requêtes/s en moyenne (sous la limite globale
        // de 50/s), par rafales (écarts de 0 à 49 ms), pour une capacité de
        // 20 × 3 par seconde : les compartiments chauds s'épuisent souvent.
        let mut created: Vec<u64> = Vec::with_capacity(N);
        let mut t = 0;
        for _ in 0..N {
            t += rng.below(50);
            created.push(t);
        }
        let mut server: HashMap<u64, ServerBucket> = HashMap::new();
        let mut violations = 0;
        // Fenêtres du serveur vidées jusqu'au dernier crédit.
        let mut drained = 0;
        let mut tickets: Vec<Option<Ticket>> = (0..N).map(|_| None).collect();
        let mut permits: Vec<Option<Permit>> = (0..N).map(|_| None).collect();
        let mut sent_order: HashMap<Route, Vec<usize>> = HashMap::new();
        let mut events: std::collections::BTreeMap<(u64, u64), Ev> = std::collections::BTreeMap::new();
        let mut seq = 0u64;
        let mut push = |events: &mut std::collections::BTreeMap<(u64, u64), Ev>, when: u64, e: Ev| {
            seq += 1;
            events.insert((when, seq), e);
        };
        let mut next_new = 0;
        // Requêtes ayant un ticket, dans l'ordre de création.
        let mut waiting: Vec<usize> = Vec::new();
        let mut now = 0u64;
        let mut done = 0;
        let mut steps = 0;
        while done < N {
            steps += 1;
            assert!(steps < 2_000_000, "simulation bloquée ({done} terminées)");
            // Événements échus.
            while let Some((&(when, s), _)) = events.iter().next() {
                if when > now {
                    break;
                }
                let Some(ev) = events.remove(&(when, s)) else { break };
                match ev {
                    Ev::Arrive { id } => {
                        let b = server.entry(routes[id].major).or_insert(ServerBucket { start: now, count: 0 });
                        if now >= b.start + WINDOW_MS {
                            *b = ServerBucket { start: now, count: 0 };
                        }
                        let outcome = if b.count >= LIMIT {
                            violations += 1;
                            Outcome { status: 429, retry_after: Some(Duration::from_millis(b.start + WINDOW_MS - now)), ..Outcome::default() }
                        } else {
                            b.count += 1;
                            if b.count == LIMIT {
                                drained += 1;
                            }
                            ok(Headers {
                                bucket: Some("partage".into()),
                                limit: Some(LIMIT),
                                remaining: Some(LIMIT - b.count),
                                reset_after: Some(Duration::from_millis(b.start + WINDOW_MS - now)),
                                global: false,
                                scope: None,
                            })
                        };
                        let lost = rng.below(100) < 5;
                        let back = now + 5 + rng.below(150);
                        if lost {
                            push(&mut events, now + 2000, Ev::Respond { id, outcome: None });
                        } else {
                            push(&mut events, back, Ev::Respond { id, outcome: Some(outcome) });
                        }
                    }
                    Ev::Respond { id, outcome } => {
                        if let Some(p) = permits[id].take() {
                            match outcome {
                                Some(o) => {
                                    l.complete(p, &o, at(now));
                                }
                                None => l.release(p, at(now)),
                            }
                        }
                        done += 1;
                    }
                }
            }
            while next_new < N && created[next_new] <= now {
                match l.enqueue(routes[next_new]) {
                    Ok(t) => {
                        tickets[next_new] = Some(t);
                        waiting.push(next_new);
                        next_new += 1;
                    }
                    Err(Full) => break,
                }
            }
            // Chaque requête en attente, dans l'ordre de création.
            let mut wake: Option<u64> = None;
            for &id in &waiting {
                let Some(t) = tickets[id].as_ref() else { continue };
                match l.poll(t, at(now)) {
                    Poll::Go(p) => {
                        tickets[id] = None;
                        permits[id] = Some(p);
                        sent_order.entry(routes[id]).or_default().push(id);
                        let arrive = now + 5 + rng.below(150);
                        push(&mut events, arrive, Ev::Arrive { id });
                    }
                    Poll::WaitUntil(w) => {
                        let ms = u64::try_from(w.saturating_duration_since(t0).as_millis()).unwrap_or(u64::MAX);
                        wake = Some(wake.map_or(ms, |x: u64| x.min(ms)));
                    }
                    Poll::WaitResponse => {}
                    Poll::Blocked(_) => panic!("blocage inattendu"),
                }
            }
            waiting.retain(|&id| tickets[id].is_some());
            let (routes_n, buckets_n, queues_n, outstanding) = l.sizes();
            assert!(routes_n <= MAX_ROUTES && buckets_n <= MAX_BUCKETS && queues_n <= MAX_OUTSTANDING && outstanding <= MAX_OUTSTANDING);
            // Prochain instant intéressant.
            let mut next = u64::MAX;
            if let Some((&(when, _), _)) = events.iter().next() {
                next = next.min(when);
            }
            if next_new < N {
                next = next.min(created[next_new].max(now + 1));
            }
            if let Some(w) = wake {
                next = next.min(w.max(now + 1));
            }
            if next == u64::MAX {
                break;
            }
            now = next.max(now + 1);
        }
        // Ordre de départ = ordre de création, route par route.
        for ids in sent_order.values() {
            assert!(ids.windows(2).all(|w| w[0] < w[1]), "ordre d'une route non respecté");
        }
        assert_eq!(done, N);
        (violations, drained, l.stats)
    }

    #[test]
    fn simulation_never_sends_into_an_exhausted_bucket() {
        for seed in [0x5EED_0001, 0xC0FF_EE42, 0x0BAD_F00D, 0x1234_5678_9ABC] {
            let (violations, drained, stats) = simulate(seed);
            assert_eq!(violations, 0, "graine {seed:#x} : {violations} envois dans un compartiment épuisé ({stats:?})");
            assert_eq!(stats.limited, 0);
            // La simulation pousse vraiment les compartiments au bout (environ
            // 2 500 fenêtres vidées par graine).
            assert!(drained > 1000, "graine {seed:#x} : seulement {drained} fenêtres vidées");
        }
    }
}
