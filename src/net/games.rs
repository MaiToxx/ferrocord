//! Activité de jeu (lot C3) côté réseau : liste des jeux détectables (cache
//! disque d'un jour), sondage des programmes en cours, op 3, adresses des
//! boutons d'activité.
//!
//! Requêtes réservées aux comptes utilisateur, non documentées par Discord ;
//! forme vérifiée sur docs.discord.food (13 novembre 2026) :
//! - page « Game », « List Detectable Games » : `GET /games/detectable`,
//!   liste d'objets « detectable application » (`id`, `name`,
//!   `executables[]` : `os`, `name`, `is_launcher`). L'ancien chemin
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
//! Le sondage ne fait aucune requête : il lit la liste des processus de cet
//! ordinateur (`crate::procs`).

use super::raw::{de_lenient, de_loose_id, Item};
use super::ratelimit::Route;
use super::rest::{api, Ctx};
use super::Out;
use crate::procs::{self, Details, Proc};
use crate::state::activity::{normalize, ActivityEvent, DetectableApp, GameIndex, OwnPresence, RunningGame, ScanConfig, MAX_EXES, MAX_NAME, REFRESH_MS};
use crate::state::Event;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
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
/// Fichier de cache plus gros : ignoré (octets).
const MAX_CACHE_BYTES: u64 = 32 << 20;
/// Attente maximale de la liste avant de renoncer.
const LOAD_MAX_WAIT: Duration = Duration::from_secs(30);
const BUTTON_MAX_WAIT: Duration = Duration::from_secs(10);
/// Écart minimal entre deux op 3 (documenté : 5 par 20 s).
pub(super) const PRESENCE_GAP: Duration = Duration::from_secs(4);

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
        (!name.is_empty() && !exes.is_empty()).then_some(DetectableApp { id: self.id?, name, exes })
    }
}

/// Réponse de `GET /games/detectable` : applications détectables sur ce
/// système ; `None` si ce n'est pas une liste.
pub(super) fn decode_detectable(body: &str) -> Option<Vec<DetectableApp>> {
    let raw: Vec<Item<RawApp>> = serde_json::from_str(body).ok()?;
    Some(raw.into_iter().filter_map(|a| a.0?.into_app()).collect())
}

/// Liste décodée (aperçu, tests).
pub fn decode_detectable_list(body: &str) -> Option<Vec<DetectableApp>> {
    decode_detectable(body)
}

// ====================================================================== cache

#[derive(Deserialize)]
struct CacheFile {
    fetched_ms: i64,
    apps: Vec<Item<CacheApp>>,
}

#[derive(Deserialize)]
struct CacheApp {
    #[serde(deserialize_with = "de_loose_id")]
    id: Option<u64>,
    name: String,
    exes: Vec<String>,
}

/// Cache : notre forme compacte (seulement les exécutables de ce système),
/// pas la réponse de Discord.
pub(super) fn cache_text(apps: &[DetectableApp], fetched_ms: i64) -> String {
    let apps: Vec<Value> = apps.iter().map(|a| json!({ "id": a.id.to_string(), "name": a.name, "exes": a.exes })).collect();
    json!({ "fetched_ms": fetched_ms, "apps": apps }).to_string()
}

pub(super) fn parse_cache(text: &str) -> Option<GameIndex> {
    let f: CacheFile = serde_json::from_str(text).ok()?;
    let apps = f
        .apps
        .into_iter()
        .filter_map(|a| {
            let a = a.0?;
            let exes: Vec<String> = a.exes.iter().map(|e| normalize(&short(e))).filter(|e| !e.is_empty()).take(MAX_EXES).collect();
            let name = short(&a.name);
            (!name.is_empty() && !exes.is_empty()).then_some(DetectableApp { id: a.id?, name, exes })
        })
        .collect();
    Some(GameIndex::new(apps, f.fetched_ms))
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
        other => other,
    };
    let fetched = if ctx.is_bot() {
        None
    } else {
        tokio::time::sleep(delay).await;
        let url = format!("{}/games/detectable", ctx.api_root);
        let route = Route { template: "GET /games/detectable", major: 0 };
        match api(&ctx, route, reqwest::Method::GET, &url, None, LOAD_MAX_WAIT, std::future::pending()).await {
            Ok(body) => tokio::task::spawn_blocking(move || decode_detectable(&body)).await.ok().flatten(),
            Err(_) => None,
        }
    };
    let ev = match (fetched, cached) {
        (Some(apps), _) => {
            if let Some(p) = cache {
                let text = cache_text(&apps, now);
                let _ = tokio::task::spawn_blocking(move || write_cache(&p, &text)).await;
            }
            ActivityEvent::Detectable(Arc::new(GameIndex::new(apps, now)))
        }
        // Discord injoignable : la liste d'avant, même vieille.
        (None, Some(c)) => ActivityEvent::Detectable(Arc::new(c)),
        (None, None) => ActivityEvent::DetectableFailed,
    };
    ctx.out.send(Event::Activity(ev));
}

// ====================================================================== sondage

/// Processus déjà examiné : ni rouvert ni recalculé aux relevés suivants.
#[derive(Clone, Debug)]
pub(super) struct Seen {
    file: String,
    hit: Option<(Option<u64>, String)>,
    start_ms: i64,
}

/// Un relevé : `procs` (instantané), `details` (chemin, lancement), les
/// processus déjà vus ; rend ceux à garder et les jeux en cours.
pub(super) fn scan_once(cfg: &ScanConfig, mut prev: HashMap<u32, Seen>, procs: &[Proc], details: impl Fn(u32) -> Option<Details>, now: i64) -> (HashMap<u32, Seen>, Vec<RunningGame>) {
    let manual_files: HashSet<&str> = cfg.manual.iter().map(|m| m.file()).collect();
    let mut seen = HashMap::new();
    for p in procs {
        let file = p.file.to_lowercase();
        if let Some(s) = prev.remove(&p.pid).filter(|s| s.file == file) {
            seen.insert(p.pid, s);
            continue;
        }
        let candidate = manual_files.contains(file.as_str()) || cfg.index.as_ref().is_some_and(|i| i.candidate(&file));
        if !candidate {
            continue;
        }
        let d = details(p.pid);
        let hit = d.as_ref().and_then(|d| {
            let path = normalize(&d.path);
            if let Some(m) = cfg.manual.iter().find(|m| m.path == path) {
                return Some((None, m.name.clone()));
            }
            cfg.index.as_ref()?.find(&path).map(|a| (Some(a.id), a.name.clone()))
        });
        let start_ms = d.and_then(|d| d.started_ms).filter(|ms| *ms <= now).unwrap_or(now);
        seen.insert(p.pid, Seen { file, hit, start_ms });
    }
    let mut running: Vec<RunningGame> = Vec::new();
    for s in seen.values() {
        let Some((app_id, name)) = &s.hit else { continue };
        match running.iter_mut().find(|g| g.app_id == *app_id && g.name == *name) {
            Some(g) => g.start_ms = g.start_ms.min(s.start_ms),
            None => running.push(RunningGame { app_id: *app_id, name: name.clone(), start_ms: s.start_ms }),
        }
    }
    running.sort_by(|a, b| a.start_ms.cmp(&b.start_ms).then_with(|| a.name.cmp(&b.name)));
    (seen, running)
}

/// Sondage des programmes : un instantané par [`SCAN_EVERY`] tant qu'il y a
/// quelque chose à chercher, rien sinon (attente du changement de réglage).
/// Windows n'offre pas d'avis de création de processus sans WMI ou ETW
/// (lourds ou réservés aux administrateurs) : l'instantané ToolHelp, qui
/// n'ouvre aucun processus, est le plus léger ; seuls les nouveaux
/// processus dont le nom de fichier peut être un jeu sont ouverts.
pub(super) async fn scan_task(mut rx: tokio::sync::watch::Receiver<ScanConfig>, out: Out, every: Duration) {
    let mut seen: HashMap<u32, Seen> = HashMap::new();
    let mut last: Vec<RunningGame> = Vec::new();
    loop {
        let cfg = rx.borrow_and_update().clone();
        seen.clear();
        if cfg.idle() {
            if !last.is_empty() {
                last.clear();
                out.send(Event::Activity(ActivityEvent::Running(Vec::new())));
            }
            if rx.changed().await.is_err() {
                return;
            }
            continue;
        }
        loop {
            let (cfg2, prev) = (cfg.clone(), std::mem::take(&mut seen));
            let Ok((kept, running)) = tokio::task::spawn_blocking(move || scan_once(&cfg2, prev, &procs::list(), procs::details, now_ms())).await else { return };
            seen = kept;
            if running != last {
                last = running.clone();
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
        self.next_at = Some(now + PRESENCE_GAP);
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
        let apps = vec![DetectableApp { id: 7, name: "Jeu".into(), exes: vec!["bin/jeu.exe".into()] }];
        let idx = parse_cache(&cache_text(&apps, 1_000)).unwrap_or_else(|| panic!("cache"));
        assert_eq!((idx.apps(), idx.fetched_ms), (&apps[..], 1_000));
        assert!(parse_cache("illisible").is_none());
        assert!(fresh(1_000, 1_000 + REFRESH_MS - 1));
        assert!(!fresh(1_000, 1_000 + REFRESH_MS));
        assert!(!fresh(1_000_000_000, 0), "daté du futur : redemandé");

        let dir = std::env::temp_dir().join(format!("ferrocord-detectable-{}", std::process::id()));
        let path = dir.join("detectable.json");
        write_cache(&path, &cache_text(&apps, 5));
        assert_eq!(read_cache(&path).map(|i| i.len()), Some(1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn proc(pid: u32, file: &str) -> Proc {
        Proc { pid, file: file.into() }
    }

    #[test]
    fn scan_matches_and_opens_each_process_once() {
        let index = Arc::new(GameIndex::new(vec![DetectableApp { id: 1, name: "PUBG".into(), exes: vec!["win64/tslgame.exe".into()] }], 0));
        let manual = vec![ManualGame::from_path(r"D:\Perso\Outil.exe").unwrap_or_else(|| panic!("chemin"))];
        let cfg = ScanConfig { enabled: true, index: Some(index), manual };
        let opened = std::cell::RefCell::new(Vec::new());
        let details = |pid: u32| {
            opened.borrow_mut().push(pid);
            Some(match pid {
                10 => Details { path: r"C:\Jeux\PUBG\Win64\TslGame.exe".into(), started_ms: Some(500) },
                11 => Details { path: r"C:\Ailleurs\TslGame.exe".into(), started_ms: Some(600) },
                12 => Details { path: r"D:\Perso\Outil.exe".into(), started_ms: None },
                _ => return None,
            })
        };
        let procs = [proc(1, "explorer.exe"), proc(10, "TslGame.exe"), proc(11, "tslgame.exe"), proc(12, "outil.exe")];
        let (seen, running) = scan_once(&cfg, HashMap::new(), &procs, details, 1_000);
        assert_eq!(*opened.borrow(), [10, 11, 12], "explorer.exe n'est pas ouvert");
        assert_eq!(
            running,
            [RunningGame { app_id: Some(1), name: "PUBG".into(), start_ms: 500 }, RunningGame { app_id: None, name: "Outil".into(), start_ms: 1_000 }]
        );
        opened.borrow_mut().clear();
        let (_, again) = scan_once(&cfg, seen, &procs[..3], details, 2_000);
        assert!(opened.borrow().is_empty(), "rien n'est rouvert");
        assert_eq!(again.len(), 1, "l'outil s'est arrêté");
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
        assert_eq!(g.due(), Some(t0 + PRESENCE_GAP));
        assert!(g.take(t0 + PRESENCE_GAP).is_some(), "fin du jeu envoyée");
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
