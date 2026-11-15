//! Ordonnanceur des chargements, sur le runtime tokio : file avec priorité
//! à ce qui est visible, nombre de téléchargements simultanés borné,
//! dédoublonnage par adresse, annulation de ce qui n'est plus affiché,
//! mémoire des échecs (404 pour la session, erreurs réseau avec attente
//! croissante), pause complète sur un 429 du CDN.
//!
//! Mémoire bornée : décodages simultanés limités (sémaphore, permis gardé
//! jusqu'à la fin du travail même après une annulation), image source
//! refusée au-delà du double de la taille demandée, et octets des images
//! prêtes mais pas encore reprises par l'interface plafonnés (le permis
//! voyage avec l'image jusqu'à sa transformation en texture).

use super::decode;
use super::disk::{Disk, DiskConfig};
use super::{Key, Source};
use crate::net::cdn::FetchError;
use eframe::egui::ColorImage;
use image::DynamicImage;
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex, MutexGuard, PoisonError,
    },
    time::Duration,
};
use tokio::{
    sync::{mpsc as tmpsc, OwnedSemaphorePermit, Semaphore},
    task::AbortHandle,
    time::Instant,
};

/// Réglages de l'ordonnanceur.
#[derive(Clone, Copy, Debug)]
pub struct WorkerConfig {
    /// Chargements (disque ou réseau) simultanés au plus.
    pub concurrency: usize,
    /// Décodages et redimensionnements simultanés au plus.
    pub decoders: usize,
    /// Octets d'images prêtes que l'interface n'a pas encore reprises, au plus.
    pub queued_bytes: usize,
    /// Attente après une première erreur réseau ; doublée à chaque nouvelle erreur.
    pub retry_base: Duration,
    pub retry_max: Duration,
}

impl Default for WorkerConfig {
    fn default() -> Self {
        WorkerConfig { concurrency: 6, decoders: 2, queued_bytes: 16 << 20, retry_base: Duration::from_secs(5), retry_max: Duration::from_secs(600) }
    }
}

/// Adresses en échec gardées en mémoire au plus (404 d'un côté, attentes de l'autre).
const MAX_MEMO: usize = 10_000;

/// Unité des permis d'octets en attente (le sémaphore compte des permis `u32`).
const QUEUE_UNIT: usize = 1024;

pub(super) struct Job {
    pub key: Key,
    pub url: String,
    /// Côté accepté au plus pour l'image source (voir [`decode::accepted_side`]).
    pub max_side: u32,
}

pub(super) enum ToWorker {
    Want(Job),
    /// Clés en attente encore affichées : le reste de la file est abandonné.
    Visible(HashSet<Key>),
}

/// Échec rapporté à l'interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Inutile de redemander pendant la session (404, image illisible…).
    Permanent,
    /// Redemander après ce délai.
    Retry(Duration),
}

/// Octets d'une image prête comptés dans le plafond tant qu'elle n'est pas
/// devenue une texture (relâchés quand ce jeton est détruit).
pub(super) struct Ticket(#[allow(dead_code)] OwnedSemaphorePermit);

pub(super) enum FromWorker {
    Ready(Key, ColorImage, Ticket),
    /// Toutes les images d'une animation (clé `anim`).
    Anim(Key, super::anim::Frames, Ticket),
    Failed(Key, Failure),
    /// Abandonné parce qu'il n'est plus affiché : à redemander s'il revient.
    Cancelled(Key),
}

/// Chiffres de l'ordonnanceur, pour les tests et les mesures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WorkerStats {
    /// Décodages ou redimensionnements en cours.
    pub decoding: usize,
    /// Le plus grand nombre de décodages simultanés observé.
    pub peak_decoding: usize,
    /// Décodages commencés depuis le début.
    pub decodes: usize,
    /// Octets d'images prêtes pas encore reprises par l'interface.
    pub queued_bytes: usize,
    pub peak_queued_bytes: usize,
    /// Places de la file rendues sans avoir servi (ajustement annulé ou
    /// arrêté en cours, échec) : chacune permet un ajustement de plus.
    pub discarded: usize,
    /// Messages envoyés à l'interface.
    pub replies: usize,
    /// Tâches qui attendent une place dans la file vers l'interface.
    pub waiting_for_room: usize,
    /// Téléchargements suspendus après un 429.
    pub paused: bool,
}

#[derive(Default)]
pub(super) struct Probe {
    decoding: AtomicUsize,
    peak_decoding: AtomicUsize,
    decodes: AtomicUsize,
    peak_queued: AtomicUsize,
    replies: AtomicUsize,
    waiting_for_room: AtomicUsize,
    discarded: AtomicUsize,
    paused: AtomicBool,
}

/// Compte une tâche qui attend une place dans la file.
struct Waiting<'a>(&'a Probe);

impl<'a> Waiting<'a> {
    fn enter(p: &'a Probe) -> Self {
        p.waiting_for_room.fetch_add(1, Ordering::AcqRel);
        Waiting(p)
    }
}

impl Drop for Waiting<'_> {
    fn drop(&mut self) {
        self.0.waiting_for_room.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Compte un décodage en cours (décompté à la fin, même en cas de panique).
struct Busy<'a>(&'a Probe);

impl<'a> Busy<'a> {
    fn enter(p: &'a Probe) -> Self {
        p.decodes.fetch_add(1, Ordering::Relaxed);
        let now = p.decoding.fetch_add(1, Ordering::AcqRel) + 1;
        p.peak_decoding.fetch_max(now, Ordering::AcqRel);
        Busy(p)
    }
}

impl Drop for Busy<'_> {
    fn drop(&mut self) {
        self.0.decoding.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Relais vers l'interface : un seul réveil tant qu'elle n'a pas lu.
#[derive(Clone)]
pub(super) struct Reply {
    pub tx: mpsc::Sender<FromWorker>,
    pub woken: Arc<AtomicBool>,
    pub wake: crate::net::Wake,
    pub probe: Arc<Probe>,
}

impl Reply {
    fn send(&self, m: FromWorker) {
        let _ = self.tx.send(m);
        self.probe.replies.fetch_add(1, Ordering::AcqRel);
        if !self.woken.swap(true, Ordering::AcqRel) {
            (self.wake)();
        }
    }
}

/// Ce que l'ordonnanceur partage avec l'interface (chiffres) et ses tâches.
pub(super) struct Shared {
    pub probe: Arc<Probe>,
    decoders: Arc<Semaphore>,
    queue: Arc<Semaphore>,
    queue_units: usize,
}

impl Shared {
    pub fn new(cfg: &WorkerConfig, probe: Arc<Probe>) -> Arc<Self> {
        let queue_units = (cfg.queued_bytes / QUEUE_UNIT).clamp(1, Semaphore::MAX_PERMITS);
        Arc::new(Shared { probe, decoders: Arc::new(Semaphore::new(cfg.decoders.max(1))), queue: Arc::new(Semaphore::new(queue_units)), queue_units })
    }

    pub fn stats(&self) -> WorkerStats {
        let p = &self.probe;
        let queued = (self.queue_units - self.queue.available_permits().min(self.queue_units)) * QUEUE_UNIT;
        WorkerStats {
            decoding: p.decoding.load(Ordering::Acquire),
            peak_decoding: p.peak_decoding.load(Ordering::Acquire),
            decodes: p.decodes.load(Ordering::Acquire),
            queued_bytes: queued,
            peak_queued_bytes: p.peak_queued.load(Ordering::Acquire) * QUEUE_UNIT,
            replies: p.replies.load(Ordering::Acquire),
            waiting_for_room: p.waiting_for_room.load(Ordering::Acquire),
            discarded: p.discarded.load(Ordering::Acquire),
            paused: p.paused.load(Ordering::Acquire),
        }
    }

    /// Réserve la place d'une image de `bytes` octets dans la file vers l'interface.
    async fn reserve(&self, bytes: usize) -> Option<Ticket> {
        let units = bytes.div_ceil(QUEUE_UNIT).clamp(1, self.queue_units);
        // Compté pendant l'attente, décompté même si la tâche est arrêtée.
        let _waiting = Waiting::enter(&self.probe);
        let permit = self.queue.clone().acquire_many_owned(u32::try_from(units).ok()?).await.ok()?;
        let used = self.queue_units - self.queue.available_permits().min(self.queue_units);
        self.probe.peak_queued.fetch_max(used, Ordering::AcqRel);
        Some(Ticket(permit))
    }

    /// `f` sur un fil bloquant, une fois un permis de décodage obtenu. Une
    /// tâche annulée pendant l'attente du permis ne commence rien ; une fois
    /// commencé, le travail garde son permis jusqu'au bout, même si la tâche
    /// qui l'attend est annulée. `None` : le travail a paniqué.
    async fn decode<T: Send + 'static>(&self, f: impl FnOnce(&Probe) -> T + Send + 'static) -> Option<T> {
        let permit = self.decoders.clone().acquire_owned().await.ok()?;
        let probe = self.probe.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            f(&probe)
        })
        .await
        .ok()
    }
}

/// Demandeurs d'une adresse en cours de chargement, partagés entre
/// l'ordonnanceur (qui en ajoute et en retire) et la tâche (qui les sert).
#[derive(Default)]
struct Waiters {
    keys: Vec<Key>,
    /// La tâche a servi tout le monde : plus aucun ajout possible.
    finished: bool,
}

type SharedWaiters = Arc<Mutex<Waiters>>;

fn lock(w: &Mutex<Waiters>) -> MutexGuard<'_, Waiters> {
    // Rien ne panique sous ce verrou ; la liste resterait cohérente de toute façon.
    w.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Fin d'un chargement (une adresse, toutes tailles confondues).
enum Done {
    Ok(String, u64),
    Err(String, u64, LoadError),
}

enum LoadError {
    Fetch(FetchError),
    Decode,
}

struct Running {
    id: u64,
    max_side: u32,
    abort: AbortHandle,
    waiters: SharedWaiters,
}

struct Backoff {
    failures: u32,
    until: Instant,
}

/// Mémoire des échecs, bornée : au plafond, les attentes échues depuis plus
/// de `linger` sont oubliées d'abord ; s'il en reste trop, tout est vidé
/// (comme les 404). Le pire effet d'un oubli est une requête de plus.
pub(super) struct Memo {
    gone: HashSet<String>,
    backoff: HashMap<String, Backoff>,
    cap: usize,
    linger: Duration,
}

impl Memo {
    pub fn new(cap: usize, linger: Duration) -> Self {
        Memo { gone: HashSet::new(), backoff: HashMap::new(), cap: cap.max(1), linger }
    }

    pub(super) fn is_gone(&self, url: &str) -> bool {
        self.gone.contains(url)
    }

    pub(super) fn mark_gone(&mut self, url: String) {
        if self.gone.len() >= self.cap {
            self.gone.clear();
        }
        self.gone.insert(url);
    }

    /// Attente restante pour `url`, s'il y en a une.
    pub(super) fn waiting(&mut self, url: &str, now: Instant) -> Option<Duration> {
        let b = self.backoff.get(url)?;
        if b.until > now {
            return Some(b.until - now);
        }
        if now.duration_since(b.until) > self.linger {
            self.backoff.remove(url);
        }
        None
    }

    /// Note une erreur réseau de plus pour `url` ; renvoie l'attente imposée.
    pub(super) fn failed(&mut self, url: String, now: Instant, base: Duration, max: Duration) -> Duration {
        if !self.backoff.contains_key(&url) && self.backoff.len() >= self.cap {
            let linger = self.linger;
            self.backoff.retain(|_, b| b.until > now || now.duration_since(b.until) <= linger);
            if self.backoff.len() >= self.cap {
                self.backoff.clear();
            }
        }
        let b = self.backoff.entry(url).or_insert(Backoff { failures: 0, until: now });
        let wait = base.checked_mul(1 << b.failures.min(16)).unwrap_or(max).min(max);
        b.failures = b.failures.saturating_add(1);
        b.until = now + wait;
        wait
    }

    pub(super) fn succeeded(&mut self, url: &str) {
        self.backoff.remove(url);
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> (usize, usize) {
        (self.gone.len(), self.backoff.len())
    }
}

/// Pause après un 429 : rien ne part avant `until`, puis une requête à la
/// fois, une de plus à chaque réussite jusqu'au nombre habituel.
struct Throttle {
    until: Option<Instant>,
    allowed: usize,
    max: usize,
}

impl Throttle {
    fn limit(&self) -> usize {
        if self.until.is_some() { 0 } else { self.allowed }
    }
}

pub(super) async fn run(
    cfg: WorkerConfig,
    source: Arc<dyn Source>,
    disk: Option<DiskConfig>,
    shared: Arc<Shared>,
    mut rx: tmpsc::UnboundedReceiver<ToWorker>,
    reply: Reply,
) {
    // Ouverture sans parcours : le parcours du dossier se fait à côté, sans
    // retarder les premières images.
    let disk = match disk {
        Some(c) => tokio::task::spawn_blocking(move || Disk::open(c).ok()).await.ok().flatten().map(Arc::new),
        None => None,
    };
    if let Some(d) = disk.clone() {
        tokio::task::spawn_blocking(move || d.scan());
    }
    let (done_tx, mut done_rx) = tmpsc::unbounded_channel::<Done>();
    let mut queue: Vec<Job> = Vec::new();
    let mut running: HashMap<String, Running> = HashMap::new();
    let mut memo = Memo::new(MAX_MEMO, cfg.retry_max);
    let concurrency = cfg.concurrency.max(1);
    let mut throttle = Throttle { until: None, allowed: concurrency, max: concurrency };
    let mut next_id = 0u64;

    loop {
        let pause = throttle.until;
        tokio::select! {
            // Demandes de l'interface d'abord : une adresse demandée deux
            // fois dans la même image rejoint le chargement en cours.
            biased;
            msg = rx.recv() => {
                let Some(msg) = msg else { break };
                match msg {
                    ToWorker::Want(job) => {
                        let slot = slot(&job.url, job.key.anim);
                        if memo.is_gone(&slot) {
                            reply.send(FromWorker::Failed(job.key, Failure::Permanent));
                        } else if let Some(wait) = memo.waiting(&slot, Instant::now()) {
                            reply.send(FromWorker::Failed(job.key, Failure::Retry(wait)));
                        } else if !running.get(&slot).is_some_and(|r| join(&r.waiters, job.key)) && !queue.iter().any(|j| j.key == job.key) {
                            queue.push(job);
                        }
                    }
                    ToWorker::Visible(visible) => {
                        queue.retain(|j| {
                            let keep = visible.contains(&j.key);
                            if !keep {
                                reply.send(FromWorker::Cancelled(j.key));
                            }
                            keep
                        });
                        running.retain(|_, r| {
                            let mut w = lock(&r.waiters);
                            w.keys.retain(|k| {
                                let keep = visible.contains(k);
                                if !keep {
                                    reply.send(FromWorker::Cancelled(*k));
                                }
                                keep
                            });
                            let idle = w.keys.is_empty();
                            if idle {
                                // Plus personne n'attend : la tâche s'arrête à
                                // sa prochaine attente (rien de nouveau ne commence).
                                w.finished = true;
                                r.abort.abort();
                            }
                            !idle
                        });
                    }
                }
            }
            Some(done) = done_rx.recv() => {
                let (url, id, result) = match done {
                    Done::Ok(url, id) => (url, id, Ok(())),
                    Done::Err(url, id, e) => (url, id, Err(e)),
                };
                // Fin d'une tâche déjà abandonnée (et peut-être remplacée) : ignorée.
                if running.get(&url).is_none_or(|r| r.id != id) {
                    continue;
                }
                let Some(r) = running.remove(&url) else { continue };
                let waiters = std::mem::take(&mut lock(&r.waiters).keys);
                match result {
                    Ok(()) => {
                        memo.succeeded(&url);
                        throttle.allowed = (throttle.allowed + 1).min(throttle.max);
                        // Arrivés après la fin de la tâche : rechargés (depuis le disque, en général).
                        for key in waiters {
                            queue.push(Job { key, url: url_of(&url).to_string(), max_side: r.max_side });
                        }
                    }
                    Err(LoadError::Fetch(FetchError::RateLimited(wait))) => {
                        // Tout s'arrête ; les demandeurs restent en attente et repartent après la pause.
                        throttle.until = Some(Instant::now() + wait);
                        throttle.allowed = 1;
                        shared.probe.paused.store(true, Ordering::Release);
                        for key in waiters {
                            queue.push(Job { key, url: url_of(&url).to_string(), max_side: r.max_side });
                        }
                    }
                    Err(LoadError::Fetch(FetchError::Network(_))) => {
                        let wait = memo.failed(url, Instant::now(), cfg.retry_base, cfg.retry_max);
                        for key in waiters {
                            reply.send(FromWorker::Failed(key, Failure::Retry(wait)));
                        }
                    }
                    Err(_) => {
                        memo.mark_gone(url);
                        for key in waiters {
                            reply.send(FromWorker::Failed(key, Failure::Permanent));
                        }
                    }
                }
            }
            () = sleep_until(pause), if pause.is_some() => {
                throttle.until = None;
                shared.probe.paused.store(false, Ordering::Release);
            }
        }
        // Le plus récemment demandé d'abord : c'est ce qui vient d'apparaître à l'écran.
        while running.len() < throttle.limit()
            && let Some(job) = queue.pop()
        {
            let job_slot = slot(&job.url, job.key.anim);
            if running.get(&job_slot).is_some_and(|r| join(&r.waiters, job.key)) {
                continue;
            }
            let mut keys = vec![job.key];
            queue.retain(|j| {
                let same = j.url == job.url && j.key.anim == job.key.anim;
                if same && !keys.contains(&j.key) {
                    keys.push(j.key);
                }
                !same
            });
            next_id += 1;
            let waiters: SharedWaiters = Arc::new(Mutex::new(Waiters { keys, finished: false }));
            let task = Task {
                url: job.url.clone(),
                slot: job_slot.clone(),
                anim: job.key.anim,
                id: next_id,
                max_side: job.max_side,
                waiters: waiters.clone(),
                source: source.clone(),
                disk: disk.clone(),
                shared: shared.clone(),
                reply: reply.clone(),
                done: done_tx.clone(),
            };
            let handle = tokio::spawn(task.run());
            running.insert(job_slot, Running { id: next_id, max_side: job.max_side, abort: handle.abort_handle(), waiters });
        }
    }
    for r in running.values() {
        r.abort.abort();
    }
}

async fn sleep_until(at: Option<Instant>) {
    match at {
        Some(at) => tokio::time::sleep_until(at).await,
        None => std::future::pending().await,
    }
}

/// Marque des chargements d'animations : la même adresse (pièce jointe GIF)
/// peut être chargée en image fixe et en animation sans que l'un attende
/// l'autre ni partage ses échecs.
const ANIM_SLOT: &str = "\n#anim";

fn slot(url: &str, anim: bool) -> String {
    if anim { format!("{url}{ANIM_SLOT}") } else { url.to_string() }
}

fn url_of(slot: &str) -> &str {
    slot.strip_suffix(ANIM_SLOT).unwrap_or(slot)
}

/// Ajoute `key` aux demandeurs d'un chargement en cours ; `false` si la
/// tâche a déjà fini de servir (il faut alors un nouveau chargement).
fn join(w: &Mutex<Waiters>, key: Key) -> bool {
    let mut w = lock(w);
    if w.finished {
        return false;
    }
    if !w.keys.contains(&key) {
        w.keys.push(key);
    }
    true
}

/// Octets d'une image ajustée à `fit` (RVBA).
fn fit_bytes(fit: decode::Fit) -> usize {
    let (w, h) = match fit {
        decode::Fit::Cover(s) => (s, s),
        decode::Fit::Contain(w, h) | decode::Fit::Fill(w, h) => (w, h),
    };
    let side = |v: u32| v.clamp(1, decode::MAX_SIDE) as usize;
    side(w) * side(h) * 4
}

/// Place réservée dans la file vers l'interface ; rendue sans avoir servi
/// (annulation, tâche arrêtée, échec), elle est comptée comme jetée.
struct Held<'a> {
    ticket: Option<Ticket>,
    probe: &'a Probe,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        if self.ticket.is_some() {
            self.probe.discarded.fetch_add(1, Ordering::AcqRel);
        }
    }
}

/// Chargement d'une adresse : image source (disque, sinon réseau), puis une
/// image ajustée par taille demandée, tant que quelqu'un l'attend.
struct Task {
    url: String,
    /// Clé du chargement (adresse, marquée pour une animation).
    slot: String,
    anim: bool,
    id: u64,
    max_side: u32,
    waiters: SharedWaiters,
    source: Arc<dyn Source>,
    disk: Option<Arc<Disk>>,
    shared: Arc<Shared>,
    reply: Reply,
    done: tmpsc::UnboundedSender<Done>,
}

impl Task {
    async fn run(self) {
        if self.anim {
            return self.run_anim().await;
        }
        let img = match self.source_image().await {
            Ok(img) => Arc::new(img),
            Err(e) => {
                let _ = self.done.send(Done::Err(self.slot, self.id, e));
                return;
            }
        };
        loop {
            let key = {
                let mut w = lock(&self.waiters);
                match w.keys.first() {
                    Some(k) => *k,
                    None => {
                        w.finished = true;
                        break;
                    }
                }
            };
            // Place réservée avant le travail : rien n'est produit que l'interface ne pourrait pas reprendre.
            let mut ticket = Held { ticket: self.shared.reserve(fit_bytes(key.fit)).await, probe: &self.shared.probe };
            let src = img.clone();
            let fitted = self.shared.decode(move |probe| {
                let _busy = Busy::enter(probe);
                decode::fit(&src, key.fit)
            });
            let fitted = fitted.await.flatten();
            let still_wanted = {
                let mut w = lock(&self.waiters);
                let at = w.keys.iter().position(|k| *k == key);
                if let Some(i) = at {
                    w.keys.remove(i);
                }
                at.is_some()
            };
            // Retiré entre-temps : l'annulation a déjà été annoncée.
            if !still_wanted {
                continue;
            }
            match (fitted, ticket.ticket.take()) {
                (Some(image), Some(ticket)) => self.reply.send(FromWorker::Ready(key, image, ticket)),
                _ => self.reply.send(FromWorker::Failed(key, Failure::Permanent)),
            }
        }
        let _ = self.done.send(Done::Ok(self.slot, self.id));
    }

    /// Animation : octets source (disque, sinon réseau), puis toutes les
    /// images ajustées, par taille demandée. La place dans la file vers
    /// l'interface n'est réservée qu'après le décodage (le nombre d'images
    /// n'est connu qu'alors) : au pire `decoders` animations de
    /// [`super::anim::MAX_ANIM_BYTES`] attendent hors plafond.
    async fn run_anim(self) {
        let bytes = match self.source_bytes().await {
            Ok(b) => Arc::new(b),
            Err(e) => {
                let _ = self.done.send(Done::Err(self.slot, self.id, e));
                return;
            }
        };
        loop {
            let key = {
                let mut w = lock(&self.waiters);
                match w.keys.first() {
                    Some(k) => *k,
                    None => {
                        w.finished = true;
                        break;
                    }
                }
            };
            let (src, max) = (bytes.clone(), self.max_side);
            let frames = self.shared.decode(move |probe| {
                let _busy = Busy::enter(probe);
                super::anim::decode_frames(&src, max, key.fit).ok()
            });
            let frames = frames.await.flatten();
            let size = frames.as_ref().map_or(0, |f| f.iter().map(|(i, _)| i.size[0] * i.size[1] * 4).sum());
            let mut ticket = Held { ticket: if frames.is_some() { self.shared.reserve(size).await } else { None }, probe: &self.shared.probe };
            let still_wanted = {
                let mut w = lock(&self.waiters);
                let at = w.keys.iter().position(|k| *k == key);
                if let Some(i) = at {
                    w.keys.remove(i);
                }
                at.is_some()
            };
            if !still_wanted {
                continue;
            }
            match (frames, ticket.ticket.take()) {
                (Some(frames), Some(ticket)) => self.reply.send(FromWorker::Anim(key, frames, ticket)),
                _ => self.reply.send(FromWorker::Failed(key, Failure::Permanent)),
            }
        }
        let _ = self.done.send(Done::Ok(self.slot, self.id));
    }

    /// Octets de l'image : disque, sinon réseau (gardés sur disque s'ils se
    /// lisent comme une image, ou comme le WebM d'une plaque animée).
    async fn source_bytes(&self) -> Result<Vec<u8>, LoadError> {
        if let Some(d) = self.disk.clone() {
            let u = self.url.clone();
            let cached = self.shared.decode(move |_| d.get(&u).filter(|b| super::anim::readable(b)));
            if let Some(b) = cached.await.flatten() {
                return Ok(b);
            }
        }
        let bytes = self.source.fetch(self.url.clone()).await.map_err(LoadError::Fetch)?;
        if !super::anim::readable(&bytes) {
            return Err(LoadError::Decode);
        }
        let Some(d) = self.disk.clone() else { return Ok(bytes) };
        let u = self.url.clone();
        let kept = self.shared.decode(move |_| {
            d.put(&u, &bytes);
            bytes
        });
        kept.await.ok_or(LoadError::Decode)
    }

    async fn source_image(&self) -> Result<DynamicImage, LoadError> {
        let max = self.max_side;
        if let Some(d) = self.disk.clone() {
            let u = self.url.clone();
            let cached = self.shared.decode(move |probe| {
                let bytes = d.get(&u)?;
                let _busy = Busy::enter(probe);
                match decode::decode(&bytes, max) {
                    Ok(img) => Some(img),
                    Err(_) => {
                        // Fichier lisible mais image abîmée : oubliée, puis retéléchargée.
                        d.forget(&u);
                        None
                    }
                }
            });
            if let Some(img) = cached.await.flatten() {
                return Ok(img);
            }
        }
        let bytes = self.source.fetch(self.url.clone()).await.map_err(LoadError::Fetch)?;
        let (d, u) = (self.disk.clone(), self.url.clone());
        let decoded = self.shared.decode(move |probe| {
            let _busy = Busy::enter(probe);
            let img = decode::decode(&bytes, max).ok()?;
            if let Some(d) = d {
                d.put(&u, &bytes);
            }
            Some(img)
        });
        decoded.await.flatten().ok_or(LoadError::Decode)
    }
}
