//! Activité de jeu (lot C3) côté réseau : liste des jeux détectables (cache
//! disque d'un jour), sondage des programmes en cours, op 3, adresses des
//! boutons d'activité.
//!
//! Requêtes réservées aux comptes utilisateur, non documentées par Discord ;
//! forme vérifiée sur docs.discord.food (13 novembre 2026) :
//! - page « Game », « List Detectable Games » : `GET /games/detectable`,
//!   liste d'objets « detectable application » (`id`, `name`, `icon_hash`,
//!   `executables[]` : `os`, `name`, `is_launcher`) ; revérifiée le
//!   15 novembre 2026. L'icône est servie par le CDN (« CDN Endpoints » :
//!   `app-icons/{application_id}/{application_icon}.png`). L'ancien chemin
//!   `/applications/detectable` n'y figure plus : on suit la page. Demandée
//!   au plus une fois par jour (cache), à la connexion, si le réglage
//!   « Afficher l'activité en cours » est actif ;
//! - page « Presence », « Get Activity Metadata » :
//!   `GET /users/{user.id}/sessions/{session_id}/activities/{application.id}/metadata`
//!   (`0` pour une écoute sans application, Spotify), réponse
//!   `{ "button_urls": [..] }` ou 204 ; seulement après un clic sur un bouton.
//! - page « Gateway Events », « Update Presence » (op 3) : `since`,
//!   `activities`, `status`, `afk` ; 5 mises à jour par 20 s au plus.
//!
//! Le sondage ne fait aucune requête : il lit les noms des programmes de cet
//! ordinateur (`crate::procs`, un instantané, aucun processus ouvert). Rien
//! d'autre que l'op 3 ne part : ni télémétrie ni événement « science ».

use super::raw::{de_lenient, de_loose_id, Item};
use super::ratelimit::Route;
use super::rest::{api, Ctx};
use super::Out;
use crate::procs;
use crate::state::activity::{normalize, ActivityEvent, DetectableApp, GameIndex, GameIndexBuilder, OwnPresence, RunningGame, ScanConfig, MAX_EXES, MAX_NAME, REFRESH_MS};
use crate::state::model::ImageHash;
use crate::state::Event;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

/// Système dont les exécutables sont gardés (champ `os`).
const OS: &str = if cfg!(windows) {
    "win32"
} else if cfg!(target_os = "macos") {
    "darwin"
} else {
    "linux"
};
/// Forme du fichier de cache (2 : avec l'icône de chaque jeu).
const CACHE_VERSION: u32 = 2;
/// Fichier de cache plus gros : ignoré (octets).
const MAX_CACHE_BYTES: u64 = 32 << 20;
/// Attente maximale de la liste avant de renoncer.
const LOAD_MAX_WAIT: Duration = Duration::from_secs(30);
const BUTTON_MAX_WAIT: Duration = Duration::from_secs(10);
/// Écart minimal entre deux op 3 (documenté : 5 par 20 s).
pub(super) const PRESENCE_GAP: Duration = Duration::from_secs(4);
/// L'écart réel est tiré entre [`PRESENCE_GAP`] et cette borne (lot
/// « fiable ») : une application qui change son activité sans arrêt ne fait
/// plus partir une op 3 toutes les 4 s pile.
pub(super) const PRESENCE_GAP_MAX: Duration = Duration::from_secs(7);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

// ====================================================================== liste

#[derive(Deserialize)]
struct RawApp {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    executables: Option<Vec<Item<RawExe>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon_hash: Option<String>,
}

#[derive(Deserialize)]
struct RawExe {
    #[serde(default, deserialize_with = "de_lenient")]
    os: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    is_launcher: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    arguments: Option<String>,
}

fn short(s: &str) -> String {
    s.trim().chars().take(MAX_NAME).collect()
}

impl RawApp {
    fn into_app(self) -> Option<DetectableApp> {
        let name = short(self.name.as_deref()?);
        let exes: Vec<String> = self
            .executables?
            .into_iter()
            .filter_map(|e| e.0)
            .filter(|e| e.os.as_deref() == Some(OS) && e.is_launcher != Some(true) && e.arguments.as_deref().is_none_or(|a| a.trim().is_empty()))
            .filter_map(|e| Some(normalize(&short(e.name.as_deref()?))))
            .filter(|n| !n.is_empty() && n != ">")
            .take(MAX_EXES)
            .collect();
        (!name.is_empty() && !exes.is_empty()).then_some(DetectableApp { id: self.id?, name, exes, icon: self.icon_hash.as_deref().and_then(ImageHash::parse) })
    }
}

/// Tableau JSON lu élément par élément (lot « ram ») : chaque élément lisible
/// est donné à `each` puis oublié ; un élément illisible est sauté, comme avec
/// [`Item`]. La liste des jeux (une dizaine de milliers d'objets) n'existe
/// ainsi jamais en entier sous sa forme brute.
struct Stream<T, F>(F, std::marker::PhantomData<T>);

impl<'de, T: serde::de::DeserializeOwned, F: FnMut(T)> serde::de::Visitor<'de> for Stream<T, F> {
    type Value = ();

    fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("un tableau")
    }

    fn visit_seq<A: serde::de::SeqAccess<'de>>(mut self, mut seq: A) -> Result<(), A::Error> {
        while let Some(Item(item)) = seq.next_element::<Item<T>>()? {
            if let Some(item) = item {
                (self.0)(item);
            }
        }
        Ok(())
    }
}

fn each_item<'de, D: serde::Deserializer<'de>, T: serde::de::DeserializeOwned>(d: D, each: impl FnMut(T)) -> Result<(), D::Error> {
    d.deserialize_seq(Stream(each, std::marker::PhantomData))
}

/// Réponse de `GET /games/detectable` : chaque application détectable sur ce
/// système est donnée à `each` ; `None` si ce n'est pas une liste.
fn detectable_each(body: &str, mut each: impl FnMut(DetectableApp)) -> Option<()> {
    let mut de = serde_json::Deserializer::from_str(body);
    each_item(&mut de, |a: RawApp| {
        if let Some(app) = a.into_app() {
            each(app);
        }
    })
    .ok()?;
    de.end().ok()
}

/// Les applications de la réponse, dans un tableau (tests, aperçu).
pub(super) fn decode_detectable(body: &str) -> Option<Vec<DetectableApp>> {
    let mut apps = Vec::new();
    detectable_each(body, |a| apps.push(a))?;
    Some(apps)
}

/// La réponse, versée directement dans l'index compact.
pub(super) fn decode_detectable_index(body: &str, fetched_ms: i64) -> Option<GameIndex> {
    let mut b = GameIndexBuilder::default();
    detectable_each(body, |a| b.push(&a))?;
    Some(b.finish(fetched_ms))
}

/// Index décodé (aperçu, banc).
pub fn decode_detectable_to_index(body: &str, fetched_ms: i64) -> Option<GameIndex> {
    decode_detectable_index(body, fetched_ms)
}

/// Liste décodée (aperçu, tests).
pub fn decode_detectable_list(body: &str) -> Option<Vec<DetectableApp>> {
    decode_detectable(body)
}

// ====================================================================== cache

#[derive(Deserialize)]
struct CacheFile {
    /// Forme du cache ; absente ou plus ancienne : relu mais redemandé.
    #[serde(default)]
    v: u32,
    fetched_ms: i64,
    #[serde(deserialize_with = "cache_apps")]
    apps: GameIndexBuilder,
}

/// Applications du cache, versées une à une dans l'index.
fn cache_apps<'de, D: serde::Deserializer<'de>>(d: D) -> Result<GameIndexBuilder, D::Error> {
    let mut b = GameIndexBuilder::default();
    each_item(d, |a: CacheApp| {
        let exes: Vec<String> = a.exes.iter().map(|e| normalize(&short(e))).filter(|e| !e.is_empty()).take(MAX_EXES).collect();
        let name = short(&a.name);
        if let Some(id) = a.id
            && !name.is_empty()
            && !exes.is_empty()
        {
            b.push(&DetectableApp { id, name, exes, icon: a.icon.as_deref().and_then(ImageHash::parse) });
        }
    })?;
    Ok(b)
}

#[derive(Deserialize)]
struct CacheApp {
    #[serde(deserialize_with = "de_loose_id")]
    id: Option<u64>,
    name: String,
    exes: Vec<String>,
    #[serde(default)]
    icon: Option<String>,
}

/// Cache : notre forme compacte (seulement les exécutables de ce système),
/// pas la réponse de Discord.
pub(super) fn cache_text(index: &GameIndex, fetched_ms: i64) -> String {
    // Écrit application par application : un arbre JSON de toute la liste
    // coûterait plusieurs Mo le temps de l'écriture.
    let quoted = |s: &str| Value::from(s).to_string();
    let mut out = String::with_capacity(index.bytes() * 2 + 64);
    out.push_str(&format!(r#"{{"v":{CACHE_VERSION},"fetched_ms":{fetched_ms},"apps":["#));
    for (i, a) in index.apps().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&format!(r#"{{"id":"{}","name":{},"exes":["#, a.id, quoted(a.name)));
        for (k, e) in a.exes().enumerate() {
            if k > 0 {
                out.push(',');
            }
            out.push_str(&quoted(e));
        }
        out.push_str(r#"],"icon":"#);
        match a.icon {
            Some(h) => out.push_str(&quoted(&h.to_string())),
            None => out.push_str("null"),
        }
        out.push('}');
    }
    out.push_str("]}");
    out
}

pub(super) fn parse_cache(text: &str) -> Option<GameIndex> {
    let f: CacheFile = serde_json::from_str(text).ok()?;
    // Cache d'avant les icônes : gardé (la détection marche tout de suite),
    // mais daté de zéro pour être redemandé une fois, par le limiteur.
    Some(f.apps.finish(if f.v == CACHE_VERSION { f.fetched_ms } else { 0 }))
}

fn read_cache(path: &Path) -> Option<GameIndex> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_CACHE_BYTES {
        return None;
    }
    parse_cache(&std::fs::read_to_string(path).ok()?)
}

/// Écrit à côté puis remplace : un cache à moitié écrit n'est jamais relu.
fn write_cache(path: &Path, text: &str) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, text).is_ok() && std::fs::rename(&tmp, path).is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
}

/// Liste encore bonne (moins d'un jour, pas datée du futur).
fn fresh(fetched_ms: i64, now: i64) -> bool {
    fetched_ms <= now + 60_000 && now - fetched_ms < REFRESH_MS
}

pub(super) async fn load(ctx: Arc<Ctx>, cache: Option<PathBuf>, delay: Duration) {
    let now = now_ms();
    let cached = match cache.clone() {
        Some(p) => tokio::task::spawn_blocking(move || read_cache(&p)).await.ok().flatten(),
        None => None,
    };
    let cached = match cached {
        Some(c) if fresh(c.fetched_ms, now) && !c.is_empty() => {
            return ctx.out.send(Event::Activity(ActivityEvent::Detectable(Arc::new(c))));
        }
        other => other.filter(|c| !c.is_empty()).map(Arc::new),
    };
    // Liste périmée (ou d'une forme plus ancienne) : elle sert tout de
    // suite, la neuve la remplacera ; la détection n'attend pas le réseau.
    if let Some(c) = &cached {
        ctx.out.send(Event::Activity(ActivityEvent::Detectable(c.clone())));
    }
    let fetched = if ctx.is_bot() {
        None
    } else {
        // Jamais à la même seconde après chaque connexion (lot « fiable »).
        tokio::time::sleep(super::backoff::between(delay, delay + delay / 2, super::backoff::unit())).await;
        let url = format!("{}/games/detectable", ctx.api_root);
        let route = Route { template: "GET /games/detectable", major: 0 };
        match api(&ctx, route, reqwest::Method::GET, &url, None, LOAD_MAX_WAIT, std::future::pending()).await {
            Ok(body) => tokio::task::spawn_blocking(move || decode_detectable_index(&body, now)).await.ok().flatten(),
            Err(_) => None,
        }
    };
    let ev = match (fetched, cached) {
        (Some(index), _) => {
            let index = Arc::new(index);
            if let Some(p) = cache {
                let written = index.clone();
                let _ = tokio::task::spawn_blocking(move || write_cache(&p, &cache_text(&written, now))).await;
            }
            ActivityEvent::Detectable(index)
        }
        // Discord injoignable : la liste d'avant, déjà envoyée, reste.
        (None, Some(_)) => return,
        (None, None) => ActivityEvent::DetectableFailed,
    };
    ctx.out.send(Event::Activity(ev));
}

// ====================================================================== sondage

/// Programme déjà examiné : son nom n'est ni décodé ni recherché aux
/// relevés suivants.
struct Seen {
    /// Empreinte du nom reçu : un identifiant repris par un autre programme
    /// est réexaminé.
    name: u64,
    /// Jeu reconnu (numéro dans [`Scanner::games`]).
    game: Option<u32>,
    round: u32,
}

/// Jeu en cours suivi d'un relevé à l'autre.
struct Tracked {
    uid: u32,
    game: RunningGame,
    /// Rang de son dernier passage au premier plan (0 : jamais).
    focus: u64,
    round: u32,
}

/// Empreinte FNV-1a d'un nom en UTF-16, sans le décoder.
fn name_hash(name: &[u16]) -> u64 {
    name.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, c| (h ^ u64::from(*c)).wrapping_mul(0x100_0000_01b3))
}

/// Jeu (application de Discord, nom) d'un programme d'après son nom de
/// fichier — ou son chemin, s'il est connu (tests, autres systèmes).
fn identify(cfg: &ScanConfig, raw: &str) -> Option<(Option<u64>, String)> {
    let path = normalize(raw.trim());
    let file = path.rsplit('/').next().unwrap_or(&path);
    let whole = file.len() != path.len();
    if let Some(m) = cfg.manual.iter().find(|m| if whole { m.path == path } else { m.file() == file }) {
        return Some((None, m.name.clone()));
    }
    let index = cfg.index.as_ref()?;
    if whole { index.find(&path) } else { index.find_file(file) }.map(|a| (Some(a.id), a.name.to_string()))
}

/// Le nom de fichier `raw` est-il celui d'un jeu connu ou ajouté à la main ?
fn candidate(cfg: &ScanConfig, raw: &str) -> bool {
    let file = normalize(raw.trim());
    cfg.manual.iter().any(|m| m.file() == file) || cfg.index.as_ref().is_some_and(|i| i.candidate(&file))
}

/// Relevés successifs des programmes en cours. Seuls les programmes dont le
/// nom de fichier est celui d'un jeu connu sont ouverts, en lecture limitée,
/// pour lire leur chemin ; les autres ne le sont jamais. Au repos (mêmes programmes qu'au relevé d'avant), un relevé
/// n'alloue rien : chaque nom n'est décodé et recherché qu'une fois.
#[derive(Default)]
pub(super) struct Scanner {
    seen: HashMap<u32, Seen>,
    games: Vec<Tracked>,
    round: u32,
    next_uid: u32,
    focus: u64,
    now: i64,
    changed: bool,
    /// Demander au système le chemin d'un programme dont le nom de fichier
    /// est celui d'un jeu connu (le vrai relevé ; les essais donnent leurs
    /// propres noms). Sans chemin (processus protégé), le nom seul sert.
    paths: bool,
}

impl Scanner {
    /// Début d'un relevé.
    pub(super) fn begin(&mut self, now: i64) {
        self.round = self.round.wrapping_add(1);
        self.now = now;
    }

    /// Un programme de l'instantané.
    pub(super) fn see(&mut self, cfg: &ScanConfig, pid: u32, name: &[u16]) {
        let hash = name_hash(name);
        let round = self.round;
        if let Some(s) = self.seen.get_mut(&pid)
            && s.name == hash
        {
            s.round = round;
            if let Some(uid) = s.game
                && let Some(t) = self.games.iter_mut().find(|t| t.uid == uid)
            {
                t.round = round;
            }
            return;
        }
        let file = String::from_utf16_lossy(name);
        // Le chemin n'est demandé que pour un nom que la liste connaît : il
        // vérifie les motifs à dossier et départage les noms partagés
        // (`game.exe`), cause de jeux reconnus à tort (0.1.13).
        let path = (self.paths && candidate(cfg, &file)).then(|| procs::details(pid)).flatten().map(|d| d.path);
        let game = identify(cfg, path.as_deref().unwrap_or(&file)).map(|(app_id, name)| self.track(app_id, name));
        self.seen.insert(pid, Seen { name: hash, game, round });
    }

    fn track(&mut self, app_id: Option<u64>, name: String) -> u32 {
        if let Some(t) = self.games.iter_mut().find(|t| t.game.app_id == app_id && t.game.name == name) {
            t.round = self.round;
            return t.uid;
        }
        self.next_uid = self.next_uid.wrapping_add(1);
        self.games.push(Tracked { uid: self.next_uid, game: RunningGame { app_id, name, start_ms: self.now }, focus: 0, round: self.round });
        self.changed = true;
        self.next_uid
    }

    /// Fin d'un relevé ; `foreground` : processus de la fenêtre de premier
    /// plan. Rend les jeux en cours seulement s'ils ont changé — rangés :
    /// celui qui a eu le premier plan le plus récemment d'abord (comme le
    /// client officiel), puis le dernier lancé.
    pub(super) fn end(&mut self, foreground: Option<u32>) -> Option<Vec<RunningGame>> {
        let round = self.round;
        self.seen.retain(|_, s| s.round == round);
        let before = self.games.len();
        self.games.retain(|t| t.round == round);
        self.changed |= self.games.len() != before;
        let top = self.focus;
        if let Some(uid) = foreground.and_then(|pid| self.seen.get(&pid)).and_then(|s| s.game)
            && let Some(t) = self.games.iter_mut().find(|t| t.uid == uid)
            && (t.focus == 0 || t.focus != top)
        {
            self.focus += 1;
            t.focus = self.focus;
        }
        let rank = |t: &Tracked| (std::cmp::Reverse(t.focus), std::cmp::Reverse(t.game.start_ms));
        let order = |a: &Tracked, b: &Tracked| rank(a).cmp(&rank(b)).then_with(|| a.game.name.cmp(&b.game.name));
        if !self.games.is_sorted_by(|a, b| order(a, b).is_le()) {
            self.games.sort_by(order);
            self.changed = true;
        }
        std::mem::take(&mut self.changed).then(|| self.games.iter().map(|t| t.game.clone()).collect())
    }

    /// Un relevé complet des programmes de cet ordinateur.
    pub(super) fn scan(&mut self, cfg: &ScanConfig, now: i64) -> Option<Vec<RunningGame>> {
        self.begin(now);
        procs::for_each(|pid, name| self.see(cfg, pid, name));
        self.end(procs::foreground_pid())
    }

    /// Nouvelle configuration (liste rechargée, programme ajouté) : chaque
    /// programme est réexaminé ; un jeu toujours là garde son heure de début.
    pub(super) fn forget(&mut self) {
        self.seen.clear();
    }

    /// Sondage arrêté : plus aucun jeu ; vrai s'il y en avait.
    pub(super) fn clear(&mut self) -> bool {
        self.seen.clear();
        self.changed = false;
        let had = !self.games.is_empty();
        self.games.clear();
        had
    }
}

/// Sondage des programmes : un instantané par [`SCAN_EVERY`] tant qu'il y a
/// quelque chose à chercher, rien sinon (attente du changement de réglage).
/// Windows n'offre pas d'avis de création de processus sans WMI ou ETW
/// (lourds ou réservés aux administrateurs) : l'instantané ToolHelp, qui
/// n'ouvre aucun processus, est le plus léger. Chaque relevé tourne sur un
/// fil d'arrière-plan ; un événement ne part que si les jeux ont changé
/// (aucun réveil de l'interface sinon).
///
/// [`SCAN_EVERY`]: crate::state::activity::SCAN_EVERY
pub(super) async fn scan_task(mut rx: tokio::sync::watch::Receiver<ScanConfig>, out: Out, every: Duration) {
    let mut scanner = Scanner { paths: true, ..Default::default() };
    loop {
        let cfg = Arc::new(rx.borrow_and_update().clone());
        if cfg.idle() {
            if scanner.clear() {
                out.send(Event::Activity(ActivityEvent::Running(Vec::new())));
            }
            if rx.changed().await.is_err() {
                return;
            }
            continue;
        }
        scanner.forget();
        loop {
            let (cfg2, mut moved) = (cfg.clone(), std::mem::take(&mut scanner));
            let Ok((back, running)) = tokio::task::spawn_blocking(move || {
                let running = moved.scan(&cfg2, now_ms());
                (moved, running)
            })
            .await
            else {
                return;
            };
            scanner = back;
            if let Some(running) = running {
                out.send(Event::Activity(ActivityEvent::Running(running)));
            }
            tokio::select! {
                _ = tokio::time::sleep(every) => {}
                r = rx.changed() => {
                    if r.is_err() {
                        return;
                    }
                    break;
                }
            }
        }
    }
}

// ====================================================================== op 3

/// Op 3 (Update Presence) de ma présence.
pub(super) fn op3(p: &OwnPresence) -> String {
    let mut activities = Vec::new();
    if p.status != "invisible" {
        if let Some(c) = &p.custom {
            let mut a = json!({ "name": "Custom Status", "type": 4 });
            if !c.text.is_empty() {
                a["state"] = c.text.clone().into();
            }
            if !c.emoji_name.is_empty() {
                let mut e = json!({ "name": c.emoji_name });
                if c.emoji_id != 0 {
                    e["id"] = c.emoji_id.to_string().into();
                }
                a["emoji"] = e;
            }
            activities.push(a);
        }
        if let Some(g) = &p.game {
            let mut a = json!({ "name": g.name, "type": 0, "timestamps": { "start": g.start_ms } });
            if let Some(id) = g.app_id {
                a["application_id"] = id.to_string().into();
            }
            activities.push(a);
        }
        // Rich Presence locale (lot R) : déjà nettoyée par `crate::rpc`.
        if let Some(r) = &p.rich {
            activities.push(crate::rpc::activity_json(r.app_id, &r.activity));
        }
    }
    // Inactif (F2) : `since` date le dernier geste, `afk` laisse les
    // notifications aller au téléphone ; « en ligne » devient « inactif »,
    // les autres statuts choisis restent ce qu'ils sont.
    let status = if p.idle_since.is_some() && p.status == "online" { "idle" } else { p.status.as_str() };
    json!({ "op": 3, "d": { "since": p.idle_since.unwrap_or(0), "activities": activities, "status": status, "afk": p.idle_since.is_some() } }).to_string()
}

/// Ma présence côté passerelle : la dernière voulue, la dernière envoyée,
/// et l'écart minimal entre deux envois. Une session neuve (Identify) a
/// tout oublié ; une reprise garde la présence.
#[derive(Default)]
pub(super) struct PresenceGate {
    wish: Option<OwnPresence>,
    sent: Option<OwnPresence>,
    next_at: Option<Instant>,
}

impl PresenceGate {
    pub(super) fn want(&mut self, p: OwnPresence) {
        self.wish = Some(p);
    }

    pub(super) fn new_session(&mut self) {
        self.sent = None;
        self.next_at = None;
    }

    /// Sans jeu, une première op 3 ne part pas : Discord garde le statut
    /// qu'il connaît (comportement d'avant ce lot).
    fn pending(&self) -> bool {
        match &self.wish {
            Some(w) => self.sent.as_ref() != Some(w) && (self.sent.is_some() || w.has_game()),
            None => false,
        }
    }

    /// Quand envoyer, s'il y a quelque chose à envoyer.
    pub(super) fn due(&self) -> Option<Instant> {
        self.pending().then(|| self.next_at.unwrap_or_else(Instant::now))
    }

    pub(super) fn take(&mut self, now: Instant) -> Option<String> {
        if !self.pending() || self.next_at.is_some_and(|t| t > now) {
            return None;
        }
        let w = self.wish.clone()?;
        let packet = op3(&w);
        self.sent = Some(w);
        self.next_at = Some(now + super::backoff::between(PRESENCE_GAP, PRESENCE_GAP_MAX, super::backoff::unit()));
        Some(packet)
    }
}

// ====================================================================== boutons

#[derive(Deserialize)]
struct RawMetadata {
    #[serde(default, deserialize_with = "de_lenient")]
    button_urls: Option<Vec<Item<String>>>,
}

pub(super) fn button_url(body: &str, index: usize) -> Option<String> {
    let m: RawMetadata = serde_json::from_str(body).ok()?;
    let url = m.button_urls?.into_iter().nth(index)?.0?;
    let ok = (url.starts_with("https://") || url.starts_with("http://")) && url.len() <= 512 && !url.chars().any(char::is_control);
    ok.then_some(url)
}

// ====================================================================== images nommées

/// Image d'une application (docs.discord.food, « Application » ›
/// Application OAuth2 Asset Object : `id`, `type` (déprécié), `name`).
#[derive(Deserialize)]
struct RawAsset {
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

/// Réponse de `GET /oauth2/applications/{app}/assets` : (nom, identifiant)
/// des images bien formées, bornées.
pub(super) fn decode_assets(body: &str) -> Option<Vec<(String, String)>> {
    use crate::state::rpc::{valid_asset_id, valid_asset_name, MAX_ASSETS};
    let list: Vec<Item<RawAsset>> = serde_json::from_str(body).ok()?;
    Some(list.into_iter().filter_map(|a| a.0).filter_map(|a| Some((a.name?, a.id?))).filter(|(n, id)| valid_asset_name(n) && valid_asset_id(id)).take(MAX_ASSETS).collect())
}

/// Liste des images d'une application Rich Presence, demandée par
/// « Charger les images » : une requête, par le limiteur.
pub(super) async fn assets(ctx: Arc<Ctx>, app_id: u64) {
    let url = format!("{}/oauth2/applications/{app_id}/assets", ctx.api_root);
    let route = Route { template: "GET /oauth2/applications/{}/assets", major: 0 };
    let list = match api(&ctx, route, reqwest::Method::GET, &url, None, BUTTON_MAX_WAIT, std::future::pending()).await {
        Ok(body) => decode_assets(&body),
        Err(_) => None,
    };
    ctx.out.send(Event::Activity(ActivityEvent::Assets { app_id, list }));
}

pub(super) async fn button(ctx: Arc<Ctx>, user_id: u64, session_id: String, app_id: u64, index: usize) {
    // L'identifiant de session vient d'un autre client : il n'entre dans le
    // chemin que s'il est fait de lettres et de chiffres.
    if ctx.is_bot() || session_id.is_empty() || session_id.len() > 64 || !session_id.bytes().all(|b| b.is_ascii_alphanumeric()) {
        ctx.out.send(Event::Activity(ActivityEvent::ButtonFailed));
        return;
    }
    let url = format!("{}/users/{user_id}/sessions/{session_id}/activities/{app_id}/metadata", ctx.api_root);
    let route = Route { template: "GET /users/{}/sessions/{}/activities/{}/metadata", major: 0 };
    let ev = match api(&ctx, route, reqwest::Method::GET, &url, None, BUTTON_MAX_WAIT, std::future::pending()).await {
        Ok(body) => button_url(&body, index).map_or(ActivityEvent::ButtonFailed, ActivityEvent::ButtonUrl),
        Err(_) => ActivityEvent::ButtonFailed,
    };
    ctx.out.send(Event::Activity(ev));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::activity::{ManualGame, OwnCustom};

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn detectable_fixture_decodes() {
        let apps = decode_detectable(&fixture("rest_games_detectable")).unwrap_or_else(|| panic!("liste"));
        if cfg!(windows) {
            let pubg = apps.iter().find(|a| a.id == 6000000000000000020).unwrap_or_else(|| panic!("jeu Windows gardé"));
            assert_eq!(pubg.exes, ["win64/tslgame_be.exe", "win64/tslgame.exe", "tslgame.exe"]);
            let launcher = apps.iter().find(|a| a.name == "Lanceur Fictif");
            assert!(launcher.is_none(), "lanceurs seuls : application ignorée");
            let args = apps.iter().find(|a| a.name == "Jeu Java Fictif").unwrap_or_else(|| panic!("jeu Java"));
            assert_eq!(args.exes, [">jeujava.exe"], "exécutable à arguments ignoré");
        }
        assert!(apps.iter().all(|a| !a.name.is_empty() && !a.exes.is_empty()));
        assert!(decode_detectable("{ \"pas\": \"une liste\" }").is_none());
        assert_eq!(decode_detectable("[1, null, {\"id\": \"x\"}, {\"name\": 5}]").map(|v| v.len()), Some(0));
    }

    #[test]
    fn cache_round_trip_and_freshness() {
        let apps = vec![DetectableApp { id: 7, name: "Jeu".into(), exes: vec!["bin/jeu.exe".into()], icon: ImageHash::parse("0123456789abcdef0123456789abcdef") }];
        let idx = parse_cache(&cache_text(&GameIndex::new(apps.clone(), 0), 1_000)).unwrap_or_else(|| panic!("cache"));
        assert_eq!((idx.apps().map(|a| a.to_app()).collect::<Vec<_>>(), idx.fetched_ms), (apps.clone(), 1_000));
        assert!(parse_cache("illisible").is_none());
        // Cache d'avant les icônes : lu, mais daté de zéro (redemandé une fois).
        let old = parse_cache(r#"{ "fetched_ms": 1000, "apps": [{ "id": "7", "name": "Jeu", "exes": ["bin/jeu.exe"] }] }"#).unwrap_or_else(|| panic!("ancien cache"));
        assert_eq!((old.len(), old.fetched_ms, old.apps().next().and_then(|a| a.icon)), (1, 0, None));
        assert!(fresh(1_000, 1_000 + REFRESH_MS - 1));
        assert!(!fresh(1_000, 1_000 + REFRESH_MS));
        assert!(!fresh(1_000_000_000, 0), "daté du futur : redemandé");

        let dir = std::env::temp_dir().join(format!("ferrocord-detectable-{}", std::process::id()));
        let path = dir.join("detectable.json");
        write_cache(&path, &cache_text(&GameIndex::new(apps.clone(), 0), 5));
        assert_eq!(read_cache(&path).map(|i| i.len()), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn units(s: &str) -> Vec<u16> {
        s.encode_utf16().collect()
    }

    /// Un relevé simulé : (identifiant, nom) de chaque programme, tels que
    /// l'instantané les donne.
    fn round(sc: &mut Scanner, cfg: &ScanConfig, procs: &[(u32, &str)], foreground: Option<u32>, now: i64) -> Option<Vec<RunningGame>> {
        sc.begin(now);
        for (pid, name) in procs {
            sc.see(cfg, *pid, &units(name));
        }
        sc.end(foreground)
    }

    fn app(id: u64, name: &str, exes: &[&str]) -> DetectableApp {
        DetectableApp { id, name: name.into(), exes: exes.iter().map(|e| e.to_string()).collect(), icon: None }
    }

    fn cfg_of(apps: Vec<DetectableApp>) -> ScanConfig {
        ScanConfig { enabled: true, index: Some(Arc::new(GameIndex::new(apps, 0))), manual: Vec::new() }
    }

    /// Entrée d'essai de la même forme que celle de la vraie liste (valeurs
    /// inventées). Elle n'est pas dans le jeu d'essai partagé : les tests de
    /// bout en bout font tourner le vrai sondage, qui ne doit rien
    /// reconnaître sur l'ordinateur de qui les lance.
    const LOL_ENTRY: &str = r#"{
        "aliases": ["LoL"],
        "executables": [
            { "is_launcher": false, "name": "league of legends.exe", "os": "win32" },
            { "is_launcher": true, "name": "leagueclient.exe", "os": "win32" },
            { "is_launcher": true, "name": "leagueclientux.exe", "os": "win32" },
            { "is_launcher": false, "name": "leagueoflegends.app", "os": "darwin" }
        ],
        "hook": true,
        "icon_hash": "0a1b2c3d4e5f60718293a4b5c6d7e8f9",
        "id": "6000000000000000030",
        "name": "League of Legends",
        "overlay": true,
        "overlay_compatibility_hook": false,
        "overlay_methods": null,
        "overlay_warn": false,
        "themes": ["Strategy"]
    }"#;

    /// Liste d'essai, de la même forme que celle de Discord.
    fn fixture_cfg() -> ScanConfig {
        let mut list: Value = serde_json::from_str(&fixture("rest_games_detectable")).unwrap_or_default();
        if let Some(l) = list.as_array_mut() {
            l.push(serde_json::from_str(LOL_ENTRY).unwrap_or_default());
        }
        let apps = decode_detectable(&list.to_string()).unwrap_or_default();
        assert_eq!(apps.iter().find(|a| a.id == LOL).and_then(|a| a.icon).map(|h| h.to_string()).as_deref(), Some("0a1b2c3d4e5f60718293a4b5c6d7e8f9"));
        cfg_of(apps)
    }

    const LOL: u64 = 6000000000000000030;

    /// Cause du défaut signalé (« Ferrocord ne détecte pas l'activité ») :
    /// le relevé ouvrait chaque candidat (`OpenProcess`) pour lire son
    /// chemin, et classait « pas un jeu », pour toujours, celui qui refusait
    /// — ce que fait un jeu protégé par un anti-triche ou lancé en
    /// administrateur. L'instantané ne donne que le nom de fichier : il doit
    /// suffire.
    #[cfg(windows)]
    #[test]
    fn protected_game_is_detected_from_its_file_name_alone() {
        let cfg = fixture_cfg();
        let mut sc = Scanner::default();
        let procs = [(4, "System"), (812, "explorer.exe"), (9001, "League of Legends.exe")];
        let got = round(&mut sc, &cfg, &procs, None, 1_000).unwrap_or_else(|| panic!("jeu annoncé"));
        assert_eq!(got, [RunningGame { app_id: Some(LOL), name: "League of Legends".into(), start_ms: 1_000 }]);
        // Activité telle qu'elle part : type 0, application, début.
        let p = OwnPresence { status: "online".into(), game: got.first().cloned(), custom: None, rich: None, idle_since: None };
        let v: Value = serde_json::from_str(&op3(&p)).unwrap_or_default();
        assert_eq!(v["d"]["activities"], json!([{ "name": "League of Legends", "type": 0, "application_id": LOL.to_string(), "timestamps": { "start": 1_000 } }]));
        // Même résultat quand le chemin entier est connu.
        let mut sc = Scanner::default();
        let got = round(&mut sc, &cfg, &[(7, r"C:\Riot Games\League of Legends\Game\League of Legends.exe")], None, 5);
        assert_eq!(got.map(|g| g[0].app_id), Some(Some(LOL)));
    }

    #[cfg(windows)]
    #[test]
    fn executables_match_by_name_case_launcher_and_os() {
        let cfg = fixture_cfg();
        let one = |name: &str| round(&mut Scanner::default(), &cfg, &[(1, name)], None, 0).and_then(|g| g.first().map(|g| g.name.clone()));
        assert_eq!(one("LEAGUE OF LEGENDS.EXE").as_deref(), Some("League of Legends"), "casse ignorée");
        assert_eq!(one("LeagueClient.exe"), None, "un lanceur seul n'est pas un jeu");
        assert_eq!(one("LeagueClientUx.exe"), None);
        assert_eq!(one("lanceur.exe"), None);
        assert_eq!(one("leagueoflegends.app"), None, "exécutable d'un autre système");
        assert_eq!(one("jeu.app"), None);
        assert_eq!(one("javaw.exe"), None, "exécutable à arguments");
        assert_eq!(one("League of Legends.exe.bak"), None);
        assert_eq!(one("of Legends.exe"), None, "nom entier seulement");
        // Motif à dossier (`win64/tslgame_be.exe`) : accepté au nom seul
        // parce qu'une seule application le revendique.
        assert_eq!(one("TslGame_BE.exe").as_deref(), Some("PLAYERUNKNOWN'S BATTLEGROUNDS"));
        // Nom partagé par deux applications sous des dossiers différents :
        // non reconnu au nom seul, reconnu avec le chemin.
        let shared = cfg_of(vec![app(1, "Un", &["bin/game.exe"]), app(2, "Deux", &["x64/game.exe"]), app(3, "Trois", &["trois.exe", "x64/trois.exe"])]);
        let name = |n: &str| round(&mut Scanner::default(), &shared, &[(1, n)], None, 0).map(|g| g[0].name.clone());
        assert_eq!(name("game.exe"), None);
        assert_eq!(name(r"D:\Jeux\Deux\x64\game.exe").as_deref(), Some("Deux"));
        assert_eq!(name(r"D:\Jeux\Ailleurs\game.exe"), None);
        assert_eq!(name("Trois.exe").as_deref(), Some("Trois"), "motif sans dossier");
    }

    #[test]
    fn scan_reports_only_changes_foreground_first_and_the_end() {
        let cfg = cfg_of(vec![app(1, "Astral Drift", &["astraldrift.exe"]), app(2, "Vallon", &["vallon.exe"])]);
        let mut sc = Scanner::default();
        let both = [(10, "astraldrift.exe"), (11, "astraldrift.exe"), (20, "vallon.exe"), (30, "notepad.exe")];
        let names = |r: Option<Vec<RunningGame>>| r.map(|v| v.into_iter().map(|g| g.name).collect::<Vec<_>>());
        assert_eq!(names(round(&mut sc, &cfg, &both[..2], None, 100)), Some(vec!["Astral Drift".to_string()]), "deux processus, un jeu");
        assert_eq!(round(&mut sc, &cfg, &both[..2], None, 200), None, "rien de neuf : aucun événement");
        // Second jeu : sans premier plan, le dernier lancé d'abord.
        assert_eq!(names(round(&mut sc, &cfg, &both, None, 300)), Some(vec!["Vallon".to_string(), "Astral Drift".to_string()]));
        // Le premier plan l'emporte, et reste acquis quand on regarde ailleurs.
        assert_eq!(names(round(&mut sc, &cfg, &both, Some(11), 400)), Some(vec!["Astral Drift".to_string(), "Vallon".to_string()]));
        assert_eq!(round(&mut sc, &cfg, &both, Some(11), 500), None);
        assert_eq!(round(&mut sc, &cfg, &both, Some(30), 600), None, "une autre fenêtre ne change rien");
        assert_eq!(names(round(&mut sc, &cfg, &both, Some(20), 700)), Some(vec!["Vallon".to_string(), "Astral Drift".to_string()]));
        // Fin d'un jeu, puis de l'autre : la liste se vide.
        let left = round(&mut sc, &cfg, &both[..2], None, 800).unwrap_or_default();
        assert_eq!(left, [RunningGame { app_id: Some(1), name: "Astral Drift".into(), start_ms: 100 }], "l'heure de début ne bouge pas");
        assert_eq!(round(&mut sc, &cfg, &both[3..], None, 900), Some(Vec::new()));
        assert_eq!(round(&mut sc, &cfg, &both[3..], None, 1_000), None);
        // Identifiant repris par un autre programme : réexaminé.
        assert_eq!(names(round(&mut sc, &cfg, &[(30, "vallon.exe")], None, 1_100)), Some(vec!["Vallon".to_string()]));
        // Liste rechargée : le jeu en cours garde son heure de début.
        sc.forget();
        assert_eq!(round(&mut sc, &cfg, &[(30, "vallon.exe")], None, 1_200), None);
        assert!(sc.clear());
        assert!(!sc.clear());
    }

    #[test]
    fn manual_program_matches_by_file_name() {
        let manual = vec![ManualGame::from_path(r"D:\Perso\Outil.exe").unwrap_or_else(|| panic!("chemin"))];
        let cfg = ScanConfig { enabled: true, index: None, manual };
        let got = round(&mut Scanner::default(), &cfg, &[(1, "explorer.exe"), (2, "OUTIL.EXE")], None, 7);
        assert_eq!(got, Some(vec![RunningGame { app_id: None, name: "Outil".into(), start_ms: 7 }]));
    }

    /// Coût d'un relevé au repos : 400 programmes déjà vus, liste de
    /// 20 000 jeux. Le chiffre s'affiche avec `--nocapture`.
    #[test]
    fn idle_scan_is_cheap() {
        let apps: Vec<DetectableApp> = (0..20_000u64).map(|i| app(i + 1, &format!("Jeu {i}"), &[&format!("jeu{i}.exe")])).collect();
        let cfg = cfg_of(apps);
        let names: Vec<(u32, Vec<u16>)> = (0..400u32).map(|i| (i * 4 + 4, units(&if i == 7 { "jeu7.exe".to_string() } else { format!("programme{i}.exe") }))).collect();
        let mut sc = Scanner::default();
        let pass = |sc: &mut Scanner, now: i64| {
            sc.begin(now);
            for (pid, name) in &names {
                sc.see(&cfg, *pid, name);
            }
            sc.end(Some(32))
        };
        assert_eq!(pass(&mut sc, 1).map(|g| g.len()), Some(1));
        let rounds = 2_000;
        let t = Instant::now();
        for i in 0..rounds {
            assert!(pass(&mut sc, 2 + i).is_none(), "au repos : aucun événement, donc aucun redessin");
        }
        let per = t.elapsed() / rounds as u32;
        eprintln!("relevé au repos (400 programmes simulés) : {per:?}");
        assert!(per < Duration::from_millis(2), "{per:?}");
    }

    /// Le vrai relevé de cet ordinateur : il ne plante pas et reste léger ;
    /// rien n'est affirmé sur ce qu'il trouve. Chiffre avec `--nocapture`.
    #[test]
    fn real_scan_does_not_crash_and_is_timed() {
        let cfg = cfg_of(decode_detectable(&fixture("rest_games_detectable")).unwrap_or_default());
        let mut sc = Scanner::default();
        let t = Instant::now();
        let _ = sc.scan(&cfg, 1);
        let first = t.elapsed();
        assert!(!sc.seen.is_empty(), "au moins ce programme-ci");
        let rounds = 20;
        let t = Instant::now();
        for i in 0..rounds {
            let _ = sc.scan(&cfg, 2 + i);
        }
        let per = t.elapsed() / rounds as u32;
        eprintln!("vrai relevé : premier {first:?}, suivants {per:?} ({} programmes)", sc.seen.len());
        assert!(per < Duration::from_millis(250), "{per:?}");
    }

    #[test]
    fn op3_shapes() {
        let game = RunningGame { app_id: Some(356873622985506820), name: "PUBG".into(), start_ms: 1_700_000_000_000 };
        let custom = OwnCustom { text: "Code".into(), emoji_name: "pain".into(), emoji_id: 7 };
        let p = OwnPresence { status: "dnd".into(), game: Some(game.clone()), custom: Some(custom), rich: None, idle_since: None };
        let v: Value = serde_json::from_str(&op3(&p)).unwrap_or_default();
        assert_eq!(
            v,
            json!({ "op": 3, "d": { "since": 0, "status": "dnd", "afk": false, "activities": [
                { "name": "Custom Status", "type": 4, "state": "Code", "emoji": { "name": "pain", "id": "7" } },
                { "name": "PUBG", "type": 0, "application_id": "356873622985506820", "timestamps": { "start": 1_700_000_000_000_i64 } }
            ] } })
        );
        let manual = OwnPresence { status: "online".into(), game: Some(RunningGame { app_id: None, ..game.clone() }), custom: None, rich: None, idle_since: None };
        let v: Value = serde_json::from_str(&op3(&manual)).unwrap_or_default();
        assert_eq!(v["d"]["activities"], json!([{ "name": "PUBG", "type": 0, "timestamps": { "start": 1_700_000_000_000_i64 } }]));
        let rich = crate::state::rpc::RichOut { app_id: 6000000000000000040, activity: crate::state::rpc::RpcActivity { name: "Donjon".into(), details: Some("Menu".into()), ..Default::default() } };
        let with_rich = OwnPresence { status: "online".into(), game: None, custom: None, rich: Some(Box::new(rich)), idle_since: None };
        // Inactif : « en ligne » devient « inactif », `afk` et `since` suivent ; un autre statut reste.
        let away = OwnPresence { status: "online".into(), game: None, custom: None, rich: None, idle_since: Some(1_700_000_000_000) };
        assert!(away.has_game(), "part même sans jeu");
        let v: Value = serde_json::from_str(&op3(&away)).unwrap_or_default();
        assert_eq!(v["d"], json!({ "since": 1_700_000_000_000_i64, "status": "idle", "afk": true, "activities": [] }));
        let v: Value = serde_json::from_str(&op3(&OwnPresence { status: "dnd".into(), ..away })).unwrap_or_default();
        assert_eq!((v["d"]["status"].as_str(), v["d"]["afk"].as_bool()), (Some("dnd"), Some(true)));
        let v: Value = serde_json::from_str(&op3(&with_rich)).unwrap_or_default();
        assert_eq!(v["d"]["activities"], json!([{ "name": "Donjon", "type": 0, "application_id": "6000000000000000040", "details": "Menu" }]));
        let inv = OwnPresence { status: "invisible".into(), game: Some(game), custom: None, rich: None, idle_since: None };
        let v: Value = serde_json::from_str(&op3(&inv)).unwrap_or_default();
        assert_eq!((v["d"]["status"].as_str(), v["d"]["activities"].as_array().map(Vec::len)), (Some("invisible"), Some(0)));
    }

    #[test]
    fn gate_sends_on_change_with_a_gap() {
        let t0 = Instant::now();
        let mut g = PresenceGate::default();
        let idle = OwnPresence { status: "online".into(), game: None, custom: None, rich: None, idle_since: None };
        g.want(idle.clone());
        assert_eq!(g.due(), None, "sans jeu, rien n'est envoyé d'abord");
        let playing = OwnPresence { game: Some(RunningGame { app_id: None, name: "J".into(), start_ms: 1 }), ..idle.clone() };
        g.want(playing.clone());
        assert!(g.take(t0).is_some_and(|p| p.contains("\"J\"")));
        assert_eq!(g.take(t0), None, "déjà envoyée");
        g.want(idle.clone());
        assert_eq!(g.take(t0 + Duration::from_secs(1)), None, "écart minimal");
        let due = g.due().expect("une op 3 en attente");
        assert!(due >= t0 + PRESENCE_GAP && due <= t0 + PRESENCE_GAP_MAX, "écart tiré entre les deux bornes");
        assert_eq!(g.take(due - Duration::from_millis(1)), None, "pas avant l'échéance");
        assert!(g.take(due).is_some(), "fin du jeu envoyée");
        g.new_session();
        assert_eq!(g.due(), None, "session neuve, sans jeu : rien");
        g.want(playing);
        assert!(g.due().is_some());
    }

    #[test]
    fn asset_list_fixture_decodes() {
        let got = decode_assets(&fixture("rest_application_assets")).unwrap_or_else(|| panic!("liste"));
        assert_eq!(got, [("logo".to_string(), "6000000000000000051".to_string()), ("carte_2".to_string(), "6000000000000000052".to_string())], "entrées mal formées écartées");
        assert_eq!(decode_assets("{}"), None);
        assert_eq!(decode_assets("[]"), Some(Vec::new()));
    }

    #[test]
    fn button_urls() {
        let body = fixture("rest_activity_metadata");
        assert_eq!(button_url(&body, 0).as_deref(), Some("https://exemple.invalid/rejoindre"));
        assert_eq!(button_url(&body, 1), None, "pas http(s)");
        assert_eq!(button_url(&body, 5), None);
        assert_eq!(button_url("", 0), None);
    }
}
