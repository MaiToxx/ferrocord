//! Activité de jeu détectée (lot C3) : liste des jeux que Discord sait
//! reconnaître, correspondance avec les programmes en cours, programmes
//! ajoutés à la main, et ma présence (« Joue à … ») envoyée par l'op 3.
//!
//! Partage des rôles : le réseau charge la liste (`GET /games/detectable`,
//! cache disque d'un jour), sonde les programmes en cours (toutes les
//! [`SCAN_EVERY`], seulement si le réglage est actif) et envoie l'op 3 ; le
//! magasin décide de ce qui est montré d'après les réglages synchronisés
//! (statut, `show_current_game`, statut personnalisé).
//!
//! Règles de correspondance des exécutables : la documentation
//! (docs.discord.food, « Game » › Application Executable Object) donne la
//! forme (`os`, `name`, `is_launcher`) sans la règle. Celle-ci suit le
//! client libre arRPC (OpenAsar, `src/process/index.js`) : nom comparé en
//! minuscules à la fin du chemin, composant par composant ; un `>` en tête
//! exige le nom de fichier exact ; les lanceurs sont ignorés ; un exécutable
//! qui exige des arguments est ignoré (on ne lit pas la ligne de commande
//! des autres programmes).

use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

/// Applications gardées dans la liste (le reste est ignoré).
pub const MAX_APPS: usize = 50_000;
/// Exécutables gardés par application.
pub const MAX_EXES: usize = 16;
/// Programmes ajoutés à la main.
pub const MAX_MANUAL: usize = 50;
/// Longueur gardée d'un nom ou d'un exécutable, en caractères.
pub const MAX_NAME: usize = 128;
/// Intervalle entre deux relevés des programmes en cours.
pub const SCAN_EVERY: Duration = Duration::from_secs(20);
/// Âge au-delà duquel la liste est redemandée à Discord.
pub const REFRESH_MS: i64 = 24 * 3600 * 1000;
/// Attente, après la connexion, avant de demander la liste à Discord (le
/// cache disque, lui, est lu tout de suite).
pub const LOAD_DELAY: Duration = Duration::from_secs(60);
/// Relevé des programmes possible sur ce système (Windows seulement) :
/// ailleurs, ni liste des jeux téléchargée ni sondage.
pub const DETECTION: bool = cfg!(windows);

/// Application que Discord sait détecter (champs utiles seulement).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DetectableApp {
    pub id: u64,
    pub name: String,
    /// Noms d'exécutables Windows, en minuscules, séparés par `/` ; `>` en
    /// tête : nom de fichier exact.
    pub exes: Vec<String>,
}

/// Chemin comparable : minuscules, séparateurs `/`.
pub fn normalize(path: &str) -> String {
    path.chars().map(|c| if c == '\\' { '/' } else { c }).collect::<String>().to_lowercase()
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Nom de fichier visé par un motif.
fn pattern_base(pattern: &str) -> &str {
    file_name(pattern.strip_prefix('>').unwrap_or(pattern))
}

/// `path` (normalisé) correspond-il au motif `pattern` (normalisé) ?
pub fn exe_matches(pattern: &str, path: &str) -> bool {
    if let Some(exact) = pattern.strip_prefix('>') {
        return !exact.is_empty() && file_name(path) == exact;
    }
    if pattern.is_empty() {
        return false;
    }
    if path == pattern {
        return true;
    }
    let Some(head) = path.strip_suffix(pattern) else { return false };
    head.ends_with('/')
}

/// Liste des jeux détectables, indexée par nom de fichier pour que chaque
/// programme en cours ne coûte qu'une recherche.
#[derive(Debug, Default)]
pub struct GameIndex {
    apps: Vec<DetectableApp>,
    by_file: HashMap<String, Vec<u32>>,
    /// Heure (ms Unix) à laquelle Discord a fourni la liste.
    pub fetched_ms: i64,
}

impl GameIndex {
    pub fn new(mut apps: Vec<DetectableApp>, fetched_ms: i64) -> Self {
        apps.truncate(MAX_APPS);
        let mut by_file: HashMap<String, Vec<u32>> = HashMap::new();
        for (i, a) in apps.iter().enumerate() {
            let mut seen = HashSet::new();
            for e in &a.exes {
                let base = pattern_base(e);
                if !base.is_empty() && seen.insert(base) {
                    by_file.entry(base.to_string()).or_default().push(i as u32);
                }
            }
        }
        GameIndex { apps, by_file, fetched_ms }
    }

    pub fn apps(&self) -> &[DetectableApp] {
        &self.apps
    }

    pub fn len(&self) -> usize {
        self.apps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.apps.is_empty()
    }

    /// Un programme de ce nom de fichier (minuscules) peut-il être un jeu ?
    /// Sinon, son chemin complet n'est même pas demandé au système.
    pub fn candidate(&self, file: &str) -> bool {
        self.by_file.contains_key(file)
    }

    /// Jeu correspondant au chemin (normalisé) d'un programme.
    pub fn find(&self, path: &str) -> Option<&DetectableApp> {
        let list = self.by_file.get(file_name(path))?;
        list.iter().filter_map(|i| self.apps.get(*i as usize)).find(|a| a.exes.iter().any(|e| exe_matches(e, path)))
    }
}

/// Programme ajouté à la main (page Activités) : reconnu à son chemin exact.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ManualGame {
    pub name: String,
    /// Chemin normalisé ([`normalize`]).
    pub path: String,
}

impl ManualGame {
    /// Nom tiré du fichier (`C:\Jeux\Mon Jeu.exe` → « Mon Jeu »).
    pub fn from_path(path: &str) -> Option<ManualGame> {
        let raw = path.trim();
        let file = raw.rsplit(['/', '\\']).next().unwrap_or(raw);
        if file.is_empty() || raw.chars().count() > 1024 {
            return None;
        }
        let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
        let name: String = if stem.is_empty() { file } else { stem }.chars().take(MAX_NAME).collect();
        Some(ManualGame { name, path: normalize(raw) })
    }

    pub fn file(&self) -> &str {
        file_name(&self.path)
    }
}

/// Jeu en cours sur cet ordinateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunningGame {
    /// Application Discord (absente pour un programme ajouté à la main).
    pub app_id: Option<u64>,
    pub name: String,
    /// Premier relevé où il tournait (ms Unix).
    pub start_ms: i64,
}

/// Ce que le sondage doit chercher. Comparé par adresse de la liste : une
/// liste rechargée est une nouvelle configuration.
#[derive(Clone, Debug, Default)]
pub struct ScanConfig {
    pub enabled: bool,
    pub index: Option<Arc<GameIndex>>,
    pub manual: Vec<ManualGame>,
}

impl ScanConfig {
    /// Rien à chercher : le sondage s'arrête (aucun relevé des programmes).
    pub fn idle(&self) -> bool {
        !self.enabled || (self.index.as_ref().is_none_or(|i| i.is_empty()) && self.manual.is_empty())
    }
}

impl PartialEq for ScanConfig {
    fn eq(&self, o: &Self) -> bool {
        let same_index = match (&self.index, &o.index) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        };
        self.enabled == o.enabled && same_index && self.manual == o.manual
    }
}

impl Eq for ScanConfig {}

/// Statut personnalisé à remettre dans l'op 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnCustom {
    pub text: String,
    pub emoji_name: String,
    pub emoji_id: u64,
}

/// Ma présence telle qu'elle doit partir (op 3). `status` : `online`,
/// `idle`, `dnd` ou `invisible`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnPresence {
    pub status: String,
    pub game: Option<RunningGame>,
    pub custom: Option<OwnCustom>,
    /// Activité d'une application branchée sur le tube local (lot R).
    pub rich: Option<Box<super::rpc::RichOut>>,
    /// Inactif (F2) : instant du dernier geste au clavier ou à la souris
    /// (ms Unix) ; `None` : présent.
    pub idle_since: Option<i64>,
}

impl OwnPresence {
    /// Rien d'autre à montrer que ce que Discord sait déjà : sans jeu, une
    /// première op 3 n'est pas envoyée (comportement d'avant ce lot).
    /// Une inactivité part toujours : Discord ne la devine pas.
    pub fn has_game(&self) -> bool {
        ((self.game.is_some() || self.rich.is_some()) && self.status != "invisible") || self.idle_since.is_some()
    }

    /// Activités montrées dans mon propre profil, comme les autres les voient.
    pub fn shown_activities(&self) -> Vec<super::people::Activity> {
        use super::people::{Activity, ActivityKind};
        let mut out: Vec<Activity> = self.rich.iter().map(|r| r.to_activity()).collect();
        if let Some(g) = &self.game {
            out.push(Activity { kind: ActivityKind::Playing, name: g.name.clone(), details: None, state: None, emoji: None, start_ms: Some(g.start_ms), rich: None });
        }
        out
    }
}

/// Magasin → réseau.
#[derive(Debug, PartialEq, Eq)]
pub enum ActivityCmd {
    /// Liste des jeux détectables : cache disque, sinon Discord.
    /// `delay` : attente avant de demander à Discord (pas au milieu de la connexion).
    LoadDetectable { cache: Option<PathBuf>, delay: Duration },
    /// Configuration du sondage des programmes.
    Scan(ScanConfig),
    /// Ma présence (op 3, passerelle).
    Presence(OwnPresence),
    /// Adresse d'un bouton d'activité d'un autre (après un clic) :
    /// `GET /users/{id}/sessions/{session}/activities/{app}/metadata`.
    Button { user_id: u64, session_id: String, app_id: u64, index: usize },
    /// Serveur Rich Presence locale (lot R) : `Some(préfixe des tubes)`
    /// l'ouvre, `None` le ferme.
    Rpc(Option<String>),
    /// Images d'une application Rich Presence, par nom (après « Charger
    /// les images ») : `GET /oauth2/applications/{app}/assets`.
    Assets { app_id: u64 },
}

/// Réseau → magasin.
#[derive(Debug)]
pub enum ActivityEvent {
    Detectable(Arc<GameIndex>),
    /// Liste indisponible (réseau, compte bot) : pas de détection automatique.
    DetectableFailed,
    /// Jeux en cours (envoyé seulement quand la liste change).
    Running(Vec<RunningGame>),
    /// Adresse du bouton, à ouvrir dans le navigateur.
    ButtonUrl(String),
    ButtonFailed,
    /// Tube local de la Rich Presence (lot R).
    Rpc(super::rpc::RpcEvent),
    /// Clavier et souris au repos depuis cet instant (ms Unix), ou de retour (F2).
    Idle(Option<i64>),
    /// Images d'une application : (nom, identifiant) ; `None` : échec.
    Assets { app_id: u64, list: Option<Vec<(String, String)>> },
}

/// État de l'activité dans le magasin.
#[derive(Debug, Default)]
pub struct Activities {
    /// Fichier de cache de la liste (`None` : pas de cache, tests).
    pub cache_path: Option<PathBuf>,
    /// Remplace [`LOAD_DELAY`] (tests).
    pub load_delay: Option<Duration>,
    pub index: Option<Arc<GameIndex>>,
    pub failed: bool,
    pub running: Vec<RunningGame>,
    /// Inactif depuis (ms Unix), d'après l'horloge de dernière entrée du système (F2).
    pub idle_since: Option<i64>,
    pub manual: Vec<ManualGame>,
    load_asked: bool,
    sent_scan: Option<ScanConfig>,
    sent_presence: Option<OwnPresence>,
    /// Adresse d'un bouton reçue, que l'interface ouvre puis retire.
    pub open_url: Option<String>,
    pub button_busy: bool,
    /// Rich Presence locale (lot R).
    pub rpc: super::rpc::RpcState,
    sent_rpc: Option<Option<String>>,
}

/// Statut synchronisé reconnu (vide : en ligne, valeur par défaut de Discord).
pub fn status_key(s: &str) -> Option<&'static str> {
    Some(match s {
        "" | "online" => "online",
        "idle" => "idle",
        "dnd" => "dnd",
        "invisible" => "invisible",
        _ => return None,
    })
}

impl Activities {
    /// Session neuve : tout est redemandé au réseau (le sondage et la
    /// passerelle d'une nouvelle connexion ne savent rien).
    pub fn session_reset(&mut self) {
        self.load_asked = false;
        self.sent_scan = None;
        self.sent_presence = None;
        self.running.clear();
        self.idle_since = None;
        self.button_busy = false;
        self.rpc.session_reset();
        self.sent_rpc = None;
    }

    /// Nom d'une application d'après la liste des jeux de Discord.
    fn listed_name(index: &Option<Arc<GameIndex>>) -> impl Fn(u64) -> Option<String> + '_ {
        move |id| index.as_ref()?.apps().iter().find(|a| a.id == id).map(|a| a.name.clone())
    }

    /// READY d'une même connexion : le sondage continue (ses jeux restent),
    /// la passerelle a oublié ma présence et la liste peut être redemandée.
    pub fn on_ready(&mut self) {
        self.load_asked = false;
        self.sent_presence = None;
        self.button_busy = false;
    }

    pub fn apply(&mut self, e: ActivityEvent) {
        match e {
            ActivityEvent::Detectable(i) => {
                self.index = Some(i);
                self.failed = false;
                self.rpc.rename(Self::listed_name(&self.index));
            }
            ActivityEvent::Rpc(e) => self.rpc.apply(e, Self::listed_name(&self.index)),
            ActivityEvent::Assets { app_id, list } => self.rpc.set_assets(app_id, list),
            ActivityEvent::DetectableFailed => self.failed = true,
            ActivityEvent::Running(r) => self.running = r,
            ActivityEvent::Idle(since) => self.idle_since = since,
            ActivityEvent::ButtonUrl(u) => {
                self.button_busy = false;
                if u.starts_with("https://") || u.starts_with("http://") {
                    self.open_url = Some(u);
                }
            }
            ActivityEvent::ButtonFailed => self.button_busy = false,
        }
    }

    pub fn set_manual(&mut self, mut list: Vec<ManualGame>) {
        list.truncate(MAX_MANUAL);
        self.manual = list;
    }

    /// Jeu montré : le dernier lancé.
    pub fn current(&self) -> Option<&RunningGame> {
        self.running.iter().max_by_key(|g| g.start_ms)
    }

    /// Présence voulue d'après les réglages ; `None` tant que le statut
    /// synchronisé est inconnu (rien n'est envoyé).
    pub fn wish(&self, status: Option<&str>, show_game: bool, custom: Option<OwnCustom>) -> Option<OwnPresence> {
        let status = status_key(status?)?;
        let invisible = status == "invisible";
        let shown = show_game && !invisible;
        // Rich Presence : même réglage que les jeux (« Afficher l'activité en cours »).
        let rich = (shown && self.rpc.enabled).then(|| self.rpc.current_out()).flatten().map(Box::new);
        // Le jeu détecté de la même application est déjà dit par sa Rich Presence.
        let game = shown.then(|| self.current().cloned()).flatten().filter(|g| rich.as_ref().is_none_or(|r| g.app_id != Some(r.app_id)));
        let custom = if invisible { None } else { custom };
        Some(OwnPresence { status: status.to_string(), game, custom, rich, idle_since: self.idle_since })
    }

    /// Commandes à envoyer quand quelque chose a changé (appelé à chaque
    /// passage du magasin : ne fait que comparer de petites valeurs).
    pub fn commands(&mut self, connected: bool, show_game: Option<bool>, presence: Option<OwnPresence>) -> Vec<ActivityCmd> {
        let mut out = Vec::new();
        if !connected {
            return out;
        }
        let enabled = DETECTION && show_game == Some(true);
        if enabled && self.index.is_none() && !self.load_asked {
            self.load_asked = true;
            out.push(ActivityCmd::LoadDetectable { cache: self.cache_path.clone(), delay: self.load_delay.unwrap_or(LOAD_DELAY) });
        }
        // Le sondage d'une connexion neuve est déjà à l'arrêt : une
        // configuration inactive ne part que pour en arrêter un en cours.
        let scan = ScanConfig { enabled, index: self.index.clone(), manual: self.manual.clone() };
        if self.sent_scan.as_ref() != Some(&scan) && (self.sent_scan.is_some() || !scan.idle()) {
            if scan.idle() {
                self.running.clear();
            }
            self.sent_scan = Some(scan.clone());
            out.push(ActivityCmd::Scan(scan));
        }
        // Tube local : ouvert tant que le réglage de cet ordinateur le veut ;
        // une connexion neuve part fermée, rien n'est envoyé pour le fermer.
        let rpc = self.rpc.enabled.then(|| self.rpc.pipe_prefix.clone().unwrap_or_else(|| crate::rpc::PIPE_PREFIX.to_string()));
        if self.sent_rpc.as_ref() != Some(&rpc) && (self.sent_rpc.is_some() || rpc.is_some()) {
            if rpc.is_none() {
                self.rpc.clients.clear();
            }
            self.sent_rpc = Some(rpc.clone());
            out.push(ActivityCmd::Rpc(rpc));
        }
        // Même règle que la passerelle : sans jeu, rien d'abord.
        if let Some(p) = presence
            && self.sent_presence.as_ref() != Some(&p)
            && (self.sent_presence.is_some() || p.has_game())
        {
            self.sent_presence = Some(p.clone());
            out.push(ActivityCmd::Presence(p));
        }
        out
    }

    /// « Charger les images » d'une application branchée (geste de
    /// l'utilisateur) ; `None` si la liste est connue ou en route.
    pub fn load_assets(&mut self, app_id: u64) -> Option<ActivityCmd> {
        (self.rpc.clients.iter().any(|c| c.app_id == app_id) && self.rpc.ask_assets(app_id)).then_some(ActivityCmd::Assets { app_id })
    }

    /// Clic sur un bouton d'activité ; `false` si une demande est en cours.
    pub fn button(&mut self, user_id: u64, session_id: &str, app_id: Option<u64>, index: usize) -> Option<ActivityCmd> {
        if self.button_busy || session_id.is_empty() {
            return None;
        }
        self.button_busy = true;
        Some(ActivityCmd::Button { user_id, session_id: session_id.to_string(), app_id: app_id.unwrap_or(0), index })
    }
}

/// Section de `local.toml` des programmes ajoutés (rien n'en part chez Discord).
pub const SECTION: &str = "jeux";
const K_LIST: &str = "programmes";
const K_NAME: &str = "nom";
const K_PATH: &str = "chemin";

/// Section `[jeux]` : `programmes = [{ nom = "…", chemin = "…" }, …]`.
pub fn games_toml(list: &[ManualGame]) -> String {
    let q = super::audio_prefs::quoted;
    let items: Vec<String> = list.iter().map(|g| format!("{{ {K_NAME} = {}, {K_PATH} = {} }}", q(&g.name), q(&g.path))).collect();
    format!("[{SECTION}]\n{K_LIST} = [{}]\n", items.join(", "))
}

/// Lit la section `[jeux]` ; entrée illisible : ignorée.
pub fn games_from_table(t: &toml::de::DeTable<'_>) -> Vec<ManualGame> {
    let mut out = Vec::new();
    for (k, v) in t.iter() {
        if k.get_ref().as_ref() != K_LIST {
            continue;
        }
        let Some(items) = v.get_ref().as_array() else { continue };
        for item in items.iter() {
            let Some(e) = item.get_ref().as_table() else { continue };
            let field = |key: &str| e.iter().find(|(k, _)| k.get_ref().as_ref() == key).and_then(|(_, v)| v.get_ref().as_str().map(str::to_string));
            let (Some(name), Some(path)) = (field(K_NAME), field(K_PATH)) else { continue };
            let name: String = name.trim().chars().take(MAX_NAME).collect();
            let path = normalize(path.trim());
            if !name.is_empty() && !file_name(&path).is_empty() && out.len() < MAX_MANUAL && !out.iter().any(|g: &ManualGame| g.path == path) {
                out.push(ManualGame { name, path });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn games_section_round_trip() {
        let list = vec![ManualGame { name: "Jeu \"guillemets\"".into(), path: "c:/jeux/a.exe".into() }, ManualGame { name: "B".into(), path: "d:/b.exe".into() }];
        let text = games_toml(&list);
        let root = toml::de::DeTable::parse(&text).unwrap_or_else(|e| panic!("{text} : {e:?}"));
        let t = root.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table().cloned());
        let t = t.unwrap_or_else(|| panic!("section"));
        assert_eq!(games_from_table(&t), list);
        let odd = toml::de::DeTable::parse("programmes = [5, { nom = \"x\" }, { nom = \"\", chemin = \"a.exe\" }, { nom = \"Ok\", chemin = 'C:\\J\\Ok.exe' }]").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(games_from_table(odd.get_ref()), [ManualGame { name: "Ok".into(), path: "c:/j/ok.exe".into() }]);
    }

    fn app(id: u64, name: &str, exes: &[&str]) -> DetectableApp {
        DetectableApp { id, name: name.into(), exes: exes.iter().map(|e| e.to_string()).collect() }
    }

    #[test]
    fn executables_match_by_path_suffix() {
        let p = normalize(r"C:\Program Files\Steam\steamapps\common\PUBG\TslGame\Binaries\Win64\TslGame.exe");
        assert!(exe_matches("win64/tslgame.exe", &p));
        assert!(exe_matches("tslgame.exe", &p));
        assert!(exe_matches(">tslgame.exe", &p));
        assert!(!exe_matches("in64/tslgame.exe", &p), "composant entier seulement");
        assert!(!exe_matches("win32/tslgame.exe", &p));
        assert!(!exe_matches(">win64/tslgame.exe", &p), "« > » : nom de fichier exact");
        assert!(!exe_matches("game.exe", &p));
        assert!(!exe_matches("", &p));
        assert!(!exe_matches(">", &p));
        assert!(exe_matches("tslgame.exe", "tslgame.exe"));
    }

    #[test]
    fn index_finds_the_right_game() {
        let idx = GameIndex::new(
            vec![app(1, "PUBG", &["win64/tslgame.exe", "tslgame_be.exe"]), app(2, "Autre", &["bin/game.exe"]), app(3, "Exact", &[">game.exe"])],
            0,
        );
        assert!(idx.candidate("tslgame.exe") && idx.candidate("game.exe"));
        assert!(!idx.candidate("explorer.exe"));
        assert_eq!(idx.find("d:/jeux/pubg/win64/tslgame.exe").map(|a| a.id), Some(1));
        assert_eq!(idx.find("d:/x/tslgame.exe"), None, "chemin trop court pour « win64/ »");
        assert_eq!(idx.find("d:/x/bin/game.exe").map(|a| a.id), Some(2));
        assert_eq!(idx.find("d:/x/game.exe").map(|a| a.id), Some(3));
    }

    #[test]
    fn manual_game_from_path() {
        let g = ManualGame::from_path(r"C:\Jeux\Mon Jeu.EXE").unwrap_or_else(|| panic!("chemin valide"));
        assert_eq!((g.name.as_str(), g.path.as_str(), g.file()), ("Mon Jeu", "c:/jeux/mon jeu.exe", "mon jeu.exe"));
        assert!(ManualGame::from_path("C:/Jeux/").is_none());
    }

    #[test]
    fn wish_follows_status_and_setting() {
        let running = vec![RunningGame { app_id: Some(1), name: "Vieux".into(), start_ms: 5 }, RunningGame { app_id: None, name: "Récent".into(), start_ms: 9 }];
        let a = Activities { running, ..Default::default() };
        let custom = Some(OwnCustom { text: "Code".into(), emoji_name: String::new(), emoji_id: 0 });
        let w = a.wish(Some("dnd"), true, custom.clone()).unwrap_or_else(|| panic!("statut connu"));
        assert_eq!((w.status.as_str(), w.game.as_ref().map(|g| g.name.as_str())), ("dnd", Some("Récent")));
        assert!(w.has_game());
        assert_eq!(a.wish(Some("online"), false, None).and_then(|w| w.game), None, "réglage coupé");
        let inv = a.wish(Some("invisible"), true, custom).unwrap_or_else(|| panic!("statut connu"));
        assert!(inv.game.is_none() && inv.custom.is_none() && !inv.has_game(), "invisible : rien n'est montré");
        assert_eq!(a.wish(None, true, None), None);
        assert_eq!(a.wish(Some("bizarre"), true, None), None);
        assert_eq!(a.wish(Some(""), true, None).map(|w| w.status), Some("online".into()));
    }

    #[test]
    fn commands_only_on_change() {
        let mut a = Activities { cache_path: Some("c.json".into()), ..Default::default() };
        assert!(a.commands(false, Some(true), None).is_empty(), "hors connexion : rien");
        let first = a.commands(true, Some(true), None);
        assert!(matches!(&first[..], [ActivityCmd::LoadDetectable { .. }]), "sondage déjà à l'arrêt : rien");
        assert!(a.commands(true, Some(true), None).is_empty(), "rien de neuf");
        a.apply(ActivityEvent::Detectable(Arc::new(GameIndex::new(vec![app(1, "J", &["j.exe"])], 0))));
        let c = a.commands(true, Some(true), None);
        assert!(matches!(&c[..], [ActivityCmd::Scan(c)] if !c.idle()));
        // Réglage coupé : le sondage s'arrête.
        a.running = vec![RunningGame { app_id: Some(1), name: "J".into(), start_ms: 1 }];
        let c = a.commands(true, Some(false), None);
        assert!(matches!(&c[..], [ActivityCmd::Scan(c)] if c.idle()));
        assert!(a.running.is_empty());
        let p = OwnPresence { status: "online".into(), game: None, custom: None, rich: None, idle_since: None };
        assert!(a.commands(true, Some(false), Some(p.clone())).is_empty(), "sans jeu, rien d'abord");
        let playing = OwnPresence { game: Some(RunningGame { app_id: Some(1), name: "J".into(), start_ms: 1 }), ..p.clone() };
        assert_eq!(a.commands(true, Some(false), Some(playing.clone())), vec![ActivityCmd::Presence(playing)]);
        assert_eq!(a.commands(true, Some(false), Some(p.clone())), vec![ActivityCmd::Presence(p.clone())], "fin du jeu");
        assert!(a.commands(true, Some(false), Some(p)).is_empty());
        a.session_reset();
        assert!(a.commands(true, Some(false), None).is_empty(), "session neuve, sondage inactif : rien");
        assert_eq!(a.commands(true, Some(true), None).len(), 1, "session neuve : configuration active renvoyée");
    }

    #[test]
    fn rich_presence_wish_and_server_commands() {
        use super::super::rpc::{RpcActivity, RpcEvent};
        let mut a = Activities { running: vec![RunningGame { app_id: Some(5), name: "Jeu".into(), start_ms: 1 }], ..Default::default() };
        let rpc_only = |c: Vec<ActivityCmd>| c.into_iter().filter(|c| matches!(c, ActivityCmd::Rpc(_))).collect::<Vec<_>>();
        assert!(rpc_only(a.commands(true, Some(true), None)).is_empty(), "réglage coupé : tube jamais ouvert");
        a.rpc.enabled = true;
        a.rpc.pipe_prefix = Some("essai-".into());
        assert_eq!(rpc_only(a.commands(true, Some(true), None)), [ActivityCmd::Rpc(Some("essai-".into()))]);
        assert!(rpc_only(a.commands(true, Some(true), None)).is_empty());
        a.apply(ActivityEvent::Rpc(RpcEvent::Connected { conn: 1, app_id: 5, since_ms: 0 }));
        let act = RpcActivity { name: "Donné".into(), details: Some("Menu".into()), ..Default::default() };
        a.apply(ActivityEvent::Rpc(RpcEvent::Activity { conn: 1, process: None, activity: Some(Box::new(act)) }));
        let w = a.wish(Some("online"), true, None).unwrap_or_else(|| panic!("statut connu"));
        assert_eq!(w.rich.as_ref().map(|r| (r.app_id, r.activity.name.as_str())), Some((5, "Donné")));
        assert!(w.game.is_none(), "même application : la Rich Presence suffit");
        assert_eq!(w.shown_activities().len(), 1);
        assert!(a.wish(Some("online"), false, None).is_some_and(|w| w.rich.is_none()), "partage coupé");
        assert!(a.wish(Some("invisible"), true, None).is_some_and(|w| w.rich.is_none()));
        a.apply(ActivityEvent::Detectable(Arc::new(GameIndex::new(vec![app(5, "Nom Officiel", &["j.exe"])], 0))));
        assert_eq!(a.rpc.current().map(|(_, x)| x.name.as_str()), Some("Nom Officiel"), "nom de la liste de Discord");
        a.rpc.enabled = false;
        let c = a.commands(true, Some(true), None);
        assert!(c.contains(&ActivityCmd::Rpc(None)), "{c:?}");
        assert!(a.rpc.clients.is_empty() && a.wish(Some("online"), true, None).is_some_and(|w| w.rich.is_none()));
        a.session_reset();
        assert!(!a.commands(true, Some(true), None).contains(&ActivityCmd::Rpc(None)), "connexion neuve : déjà fermé");
    }

    #[test]
    fn button_urls_are_checked() {
        let mut a = Activities::default();
        assert!(a.button(1, "", None, 0).is_none());
        assert_eq!(a.button(1, "s", None, 1), Some(ActivityCmd::Button { user_id: 1, session_id: "s".into(), app_id: 0, index: 1 }));
        assert!(a.button(1, "s", None, 1).is_none(), "une demande à la fois");
        a.apply(ActivityEvent::ButtonUrl("javascript:alert(1)".into()));
        assert_eq!(a.open_url, None);
        a.apply(ActivityEvent::ButtonUrl("https://exemple.test/x".into()));
        assert_eq!(a.open_url.as_deref(), Some("https://exemple.test/x"));
    }
}
