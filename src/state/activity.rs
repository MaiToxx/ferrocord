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
//!
//! Lot « activité » : le relevé n'ouvre plus aucun processus (un anti-triche
//! ou un programme élevé le refusait, et le jeu n'était jamais reconnu). Il
//! ne connaît donc que le NOM DE FICHIER de chaque programme, pas son
//! chemin : voir [`GameIndex::find_file`] pour la règle qui en découle.
//! L'heure de début est celle du premier relevé où le jeu tournait.

use std::{
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
/// Jeux vus récemment gardés (page Activités).
pub const MAX_RECENT: usize = 20;
/// Intervalle entre deux relevés des programmes en cours (le client
/// officiel relève toutes les 5 à 15 s).
pub const SCAN_EVERY: Duration = Duration::from_secs(10);
/// Âge au-delà duquel la liste est redemandée à Discord.
pub const REFRESH_MS: i64 = 24 * 3600 * 1000;
/// Attente, après la connexion, avant de demander la liste à Discord (le
/// cache disque, lui, est lu tout de suite).
pub const LOAD_DELAY: Duration = Duration::from_secs(60);
/// Après un échec de la liste (ni réponse ni cache), un nouveau READY de la
/// même connexion ne la redemande pas avant ce délai.
pub const RETRY_AFTER_FAILURE: Duration = Duration::from_secs(3600);
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
    /// Icône (`icon_hash`), servie par le CDN sous `app-icons/{id}/{hash}`.
    pub icon: Option<super::image::ImageHash>,
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

/// Morceau du texte commun de l'index (octets).
#[derive(Clone, Copy, Debug, Default)]
struct Span {
    at: u32,
    len: u16,
}

/// Une application de l'index : ses textes sont des morceaux du texte commun.
#[derive(Debug)]
struct Slot {
    id: u64,
    icon: Option<super::image::ImageHash>,
    name: Span,
    /// Ses motifs dans `GameIndex::exes` : le premier, et leur nombre.
    first_exe: u32,
    exe_count: u8,
}

/// Application de la liste, lue dans l'index sans rien copier.
#[derive(Clone, Copy)]
pub struct AppRef<'a> {
    pub id: u64,
    pub name: &'a str,
    pub icon: Option<super::image::ImageHash>,
    text: &'a str,
    exes: &'a [Span],
}

impl<'a> AppRef<'a> {
    /// Motifs d'exécutables (voir [`DetectableApp::exes`]), dans l'ordre de Discord.
    pub fn exes(&self) -> impl Iterator<Item = &'a str> + 'a {
        let text = self.text;
        self.exes.iter().map(move |s| slice(text, *s))
    }

    /// Copie indépendante de l'index (cache disque, tests).
    pub fn to_app(&self) -> DetectableApp {
        DetectableApp { id: self.id, name: self.name.to_string(), exes: self.exes().map(str::to_string).collect(), icon: self.icon }
    }
}

impl std::fmt::Debug for AppRef<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AppRef").field("id", &self.id).field("name", &self.name).finish()
    }
}

impl PartialEq for AppRef<'_> {
    fn eq(&self, o: &Self) -> bool {
        self.id == o.id && self.name == o.name && self.icon == o.icon && self.exes().eq(o.exes())
    }
}

fn slice(text: &str, s: Span) -> &str {
    text.get(s.at as usize..s.at as usize + usize::from(s.len)).unwrap_or_default()
}

/// Liste des jeux détectables, indexée par nom de fichier pour que chaque
/// programme en cours ne coûte qu'une recherche.
///
/// Forme compacte (lot « ram ») : la liste de Discord compte une dizaine de
/// milliers d'applications ; une `String` par nom et par exécutable, plus deux
/// tables de hachage, tenaient 6,3 Mo en 60 000 blocs. Ici, tous les textes
/// sont bout à bout dans une seule chaîne, et les deux index (par nom de
/// fichier, par identifiant) sont des tableaux triés, lus par dichotomie :
/// cinq blocs en tout, quelle que soit la taille de la liste.
#[derive(Debug, Default)]
pub struct GameIndex {
    /// Noms et motifs d'exécutables, bout à bout.
    text: String,
    slots: Vec<Slot>,
    /// Motifs de toutes les applications, groupés par application.
    exes: Vec<Span>,
    /// (nom de fichier visé par un motif, numéro de l'application), trié par
    /// nom puis par numéro : les applications d'un même nom gardent l'ordre de Discord.
    by_file: Vec<(Span, u32)>,
    /// Numéros des applications triés par identifiant (le premier de la liste pour un identifiant).
    by_id: Vec<u32>,
    /// Heure (ms Unix) à laquelle Discord a fourni la liste.
    pub fetched_ms: i64,
}

/// Construction d'un [`GameIndex`] application par application : le décodage
/// de la liste n'a jamais à la tenir en entier sous une autre forme.
#[derive(Debug, Default)]
pub struct GameIndexBuilder(GameIndex);

impl GameIndexBuilder {
    /// Texte ajouté au texte commun ; `None` s'il n'y tient plus (jamais atteint
    /// en pratique : 4 Gio de noms).
    fn text(&mut self, s: &str) -> Option<Span> {
        let (at, len) = (u32::try_from(self.0.text.len()).ok()?, u16::try_from(s.len()).ok()?);
        at.checked_add(u32::from(len))?;
        self.0.text.push_str(s);
        Some(Span { at, len })
    }

    /// Ajoute une application (au-delà de [`MAX_APPS`] : ignorée).
    pub fn push(&mut self, app: &DetectableApp) {
        let n = self.0.slots.len();
        if n >= MAX_APPS {
            return;
        }
        let Some(name) = self.text(&app.name) else { return };
        let first_exe = self.0.exes.len();
        let first_file = self.0.by_file.len();
        for e in app.exes.iter().take(usize::from(u8::MAX)) {
            let Some(span) = self.text(e) else { break };
            self.0.exes.push(span);
            // Le nom de fichier visé est la fin du motif : un morceau du même texte.
            let base = pattern_base(e);
            let file = Span { at: span.at + (e.len() - base.len()) as u32, len: base.len() as u16 };
            let seen = self.0.by_file[first_file..].iter().any(|(s, _)| slice(&self.0.text, *s) == base);
            if !base.is_empty() && !seen {
                self.0.by_file.push((file, n as u32));
            }
        }
        self.0.slots.push(Slot { id: app.id, icon: app.icon, name, first_exe: first_exe as u32, exe_count: (self.0.exes.len() - first_exe) as u8 });
    }

    pub fn len(&self) -> usize {
        self.0.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.slots.is_empty()
    }

    /// Trie les index et rend les capacités en trop.
    pub fn finish(self, fetched_ms: i64) -> GameIndex {
        let mut g = self.0;
        g.fetched_ms = fetched_ms;
        let text = std::mem::take(&mut g.text);
        g.by_file.sort_unstable_by(|a, b| slice(&text, a.0).cmp(slice(&text, b.0)).then(a.1.cmp(&b.1)));
        g.text = text;
        let mut by_id: Vec<u32> = (0..g.slots.len() as u32).collect();
        by_id.sort_unstable_by_key(|i| (g.slots[*i as usize].id, *i));
        by_id.dedup_by_key(|i| g.slots[*i as usize].id);
        g.by_id = by_id;
        g.text.shrink_to_fit();
        g.slots.shrink_to_fit();
        g.exes.shrink_to_fit();
        g.by_file.shrink_to_fit();
        g.by_id.shrink_to_fit();
        g
    }
}

impl GameIndex {
    pub fn new(apps: Vec<DetectableApp>, fetched_ms: i64) -> Self {
        let mut b = GameIndexBuilder::default();
        for a in &apps {
            b.push(a);
        }
        b.finish(fetched_ms)
    }

    fn at(&self, n: u32) -> Option<AppRef<'_>> {
        let s = self.slots.get(n as usize)?;
        let exes = self.exes.get(s.first_exe as usize..s.first_exe as usize + usize::from(s.exe_count))?;
        Some(AppRef { id: s.id, name: slice(&self.text, s.name), icon: s.icon, text: &self.text, exes })
    }

    /// Application par identifiant (icône de l'encart, nom officiel).
    pub fn app(&self, id: u64) -> Option<AppRef<'_>> {
        let i = self.by_id.binary_search_by_key(&id, |n| self.slots[*n as usize].id).ok()?;
        self.at(self.by_id[i])
    }

    /// Toutes les applications, dans l'ordre de Discord.
    pub fn apps(&self) -> impl Iterator<Item = AppRef<'_>> + Clone {
        (0..self.slots.len() as u32).filter_map(|n| self.at(n))
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    /// Octets tenus par l'index (mesures et tests).
    pub fn bytes(&self) -> usize {
        use std::mem::size_of;
        self.text.capacity() + self.slots.capacity() * size_of::<Slot>() + self.exes.capacity() * size_of::<Span>() + self.by_file.capacity() * size_of::<(Span, u32)>() + self.by_id.capacity() * size_of::<u32>()
    }

    /// Applications qui revendiquent ce nom de fichier (minuscules), dans l'ordre de Discord.
    fn claims<'a>(&'a self, file: &'a str) -> impl Iterator<Item = AppRef<'a>> + Clone + 'a {
        let lo = self.by_file.partition_point(|(s, _)| slice(&self.text, *s) < file);
        self.by_file[lo..].iter().take_while(move |(s, _)| slice(&self.text, *s) == file).filter_map(|(_, n)| self.at(*n))
    }

    /// Un programme de ce nom de fichier (minuscules) peut-il être un jeu ?
    /// Sinon, son chemin complet n'est même pas demandé au système.
    pub fn candidate(&self, file: &str) -> bool {
        self.claims(file).next().is_some()
    }

    /// Jeu correspondant au chemin (normalisé) d'un programme.
    ///
    /// Plusieurs applications peuvent revendiquer le même fichier (`game.exe`,
    /// `launcher.exe`…). Dans l'ordre : celle dont un motif à dossier
    /// correspond au chemin (le plus précis) ; sinon, s'il n'y en a qu'une
    /// au motif sans dossier, celle-là ; à plusieurs, celle dont le nom se
    /// lit dans le chemin (`…/mon jeu/game.exe`), et aucune si rien ne les
    /// départage : un jeu non reconnu vaut mieux qu'un jeu mal attribué
    /// (0.1.13 ; avant, la première de la liste de Discord gagnait).
    pub fn find<'a>(&'a self, path: &'a str) -> Option<AppRef<'a>> {
        let hits = || self.claims(file_name(path)).filter(|a| a.exes().any(|e| exe_matches(e, path)));
        if let Some(a) = hits().find(|a| a.exes().any(|e| e.contains('/') && exe_matches(e, path))) {
            return Some(a);
        }
        let mut all = hits();
        let first = all.next()?;
        if all.next().is_none() {
            return Some(first);
        }
        let squash = |s: &str| s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect::<String>();
        let dirs = squash(path.rsplit_once('/').map_or("", |(d, _)| d));
        let mut named = hits().filter(|a| {
            let n = squash(a.name);
            n.len() >= 3 && dirs.contains(&n)
        });
        match (named.next(), named.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    }

    /// Jeu correspondant au seul nom de fichier (minuscules) d'un programme :
    /// tout ce que l'instantané des processus donne sans en ouvrir aucun.
    ///
    /// - un motif sans dossier (`jeu.exe`, `>jeu.exe`) correspond tel quel ;
    ///   à plusieurs, le premier de la liste de Discord ;
    /// - un motif à dossier (`win64/jeu.exe`) ne peut pas être vérifié sans
    ///   le chemin : il n'est accepté que si une seule application de la
    ///   liste revendique ce nom de fichier. Un nom partagé (`game.exe`
    ///   rangé sous des dossiers différents) reste donc non reconnu plutôt
    ///   que mal attribué.
    pub fn find_file<'a>(&'a self, file: &'a str) -> Option<AppRef<'a>> {
        let mut apps = self.claims(file);
        if let Some(a) = apps.clone().find(|a| a.exes().any(|e| e.strip_prefix('>').unwrap_or(e) == file)) {
            return Some(a);
        }
        match (apps.next(), apps.next()) {
            (Some(only), None) => Some(only),
            _ => None,
        }
    }
}

/// Jeu vu récemment sur cet ordinateur (page Activités), avec son
/// interrupteur : un jeu masqué n'est jamais annoncé.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct RecentGame {
    pub app_id: Option<u64>,
    pub name: String,
    /// Début de la dernière partie vue (ms Unix).
    pub seen_ms: i64,
    pub hidden: bool,
}

impl RecentGame {
    pub fn is(&self, g: &RunningGame) -> bool {
        self.app_id == g.app_id && self.name == g.name
    }
}

/// Range les jeux en cours en tête des jeux récents ; vrai si la liste a
/// changé (seulement quand une partie commence : rien n'est réécrit à
/// chaque relevé).
pub fn note_running(recent: &mut Vec<RecentGame>, running: &[RunningGame]) -> bool {
    let mut changed = false;
    // À rebours : le jeu montré (le premier du relevé) finit en tête.
    for g in running.iter().rev() {
        match recent.iter().position(|r| r.is(g)) {
            Some(i) if recent[i].seen_ms >= g.start_ms => {}
            Some(i) => {
                let mut r = recent.remove(i);
                r.seen_ms = g.start_ms;
                recent.insert(0, r);
                changed = true;
            }
            None => {
                recent.insert(0, RecentGame { app_id: g.app_id, name: g.name.clone(), seen_ms: g.start_ms, hidden: false });
                changed = true;
            }
        }
    }
    // Trop de jeux : le plus ancien non masqué part d'abord (un masque ne
    // tombe pas tout seul).
    while recent.len() > MAX_RECENT {
        let i = recent.iter().rposition(|r| !r.hidden).unwrap_or(recent.len() - 1);
        recent.remove(i);
    }
    changed
}

/// Programme ajouté à la main (page Activités) : reconnu à son nom de
/// fichier (le chemin enregistré sert d'infobulle et de clé).
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
    /// Jeux vus récemment et leurs masques (copie des préférences locales).
    pub recent: Vec<RecentGame>,
    /// Inactif depuis (ms Unix), d'après l'horloge de dernière entrée du système (F2).
    pub idle_since: Option<i64>,
    pub manual: Vec<ManualGame>,
    load_asked: bool,
    /// Dernier échec du chargement de la liste (ni réponse ni cache).
    failed_at: Option<std::time::Instant>,
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
        move |id| index.as_ref()?.app(id).map(|a| a.name.to_string())
    }

    /// READY d'une même connexion : le sondage continue (ses jeux restent),
    /// la passerelle a oublié ma présence et la liste peut être redemandée.
    pub fn on_ready(&mut self) {
        // Une liste refusée il y a peu n'est pas redemandée à chaque READY
        // (lot « fiable » : pas d'erreur répétée).
        if !self.failed_at.is_some_and(|t| t.elapsed() < RETRY_AFTER_FAILURE) {
            self.load_asked = false;
        }
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
            ActivityEvent::DetectableFailed => {
                self.failed = true;
                self.failed_at = Some(std::time::Instant::now());
            }
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

    /// Ce jeu est-il masqué (interrupteur de la page Activités) ?
    pub fn hidden(&self, g: &RunningGame) -> bool {
        self.recent.iter().any(|r| r.hidden && r.is(g))
    }

    /// Jeu détecté, masqué ou non (page Activités) : le premier du relevé,
    /// qui les range (premier plan d'abord, puis le dernier lancé).
    pub fn detected(&self) -> Option<&RunningGame> {
        self.running.first()
    }

    /// Jeu montré et annoncé : le premier du relevé qui n'est pas masqué.
    pub fn current(&self) -> Option<&RunningGame> {
        self.running.iter().find(|g| !self.hidden(g))
    }

    /// Icône de l'application `app_id`, si la liste des jeux déjà chargée la connaît.
    pub fn app_icon(&self, app_id: u64) -> Option<super::image::ImageRef> {
        let app = self.index.as_ref()?.app(app_id)?;
        Some(super::image::ImageRef::AppIcon { app_id: app.id, hash: app.icon? })
    }

    /// Icône du jeu d'après la liste de Discord.
    pub fn icon(&self, g: &RunningGame) -> Option<super::image::ImageRef> {
        let app = self.index.as_ref()?.app(g.app_id?)?;
        Some(super::image::ImageRef::AppIcon { app_id: app.id, hash: app.icon? })
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
const K_RECENT: &str = "recents";
const K_ID: &str = "id";
const K_SEEN: &str = "vu";
const K_HIDDEN: &str = "masque";

/// Section `[jeux]` : `programmes = [{ nom = "…", chemin = "…" }, …]`.
/// Puis `recents = [{ nom = "…", id = "…", vu = "…", masque = false }, …]`.
pub fn games_toml(list: &[ManualGame], recent: &[RecentGame]) -> String {
    let q = super::audio_prefs::quoted;
    let items: Vec<String> = list.iter().map(|g| format!("{{ {K_NAME} = {}, {K_PATH} = {} }}", q(&g.name), q(&g.path))).collect();
    let seen: Vec<String> = recent
        .iter()
        .map(|r| format!("{{ {K_NAME} = {}, {K_ID} = \"{}\", {K_SEEN} = \"{}\", {K_HIDDEN} = {} }}", q(&r.name), r.app_id.unwrap_or(0), r.seen_ms, r.hidden))
        .collect();
    format!("[{SECTION}]\n{K_LIST} = [{}]\n# Jeux vus récemment ; masque = true : jamais annoncé.\n{K_RECENT} = [{}]\n", items.join(", "), seen.join(", "))
}

/// Lit `recents` de la section `[jeux]` ; entrée illisible : ignorée.
pub fn recent_from_table(t: &toml::de::DeTable<'_>) -> Vec<RecentGame> {
    let mut out: Vec<RecentGame> = Vec::new();
    for (k, v) in t.iter() {
        if k.get_ref().as_ref() != K_RECENT {
            continue;
        }
        let Some(items) = v.get_ref().as_array() else { continue };
        for item in items.iter() {
            let Some(e) = item.get_ref().as_table() else { continue };
            let value = |key: &str| e.iter().find(|(k, _)| k.get_ref().as_ref() == key).map(|(_, v)| v.get_ref());
            let text = |key: &str| value(key).and_then(|v| v.as_str().map(str::to_string));
            let Some(name) = text(K_NAME) else { continue };
            let name: String = name.trim().chars().take(MAX_NAME).collect();
            let app_id = text(K_ID).and_then(|s| s.parse::<u64>().ok()).filter(|id| *id != 0);
            let seen_ms = text(K_SEEN).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
            let hidden = value(K_HIDDEN).and_then(|v| v.as_bool()).unwrap_or(false);
            if !name.is_empty() && out.len() < MAX_RECENT && !out.iter().any(|r| r.app_id == app_id && r.name == name) {
                out.push(RecentGame { app_id, name, seen_ms, hidden });
            }
        }
    }
    out
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
        let recent = vec![RecentGame { app_id: Some(6000000000000000020), name: "Astral \"Drift\"".into(), seen_ms: 1_700_000_000_000, hidden: true }, RecentGame { app_id: None, name: "Outil".into(), seen_ms: 5, hidden: false }];
        let text = games_toml(&list, &recent);
        let root = toml::de::DeTable::parse(&text).unwrap_or_else(|e| panic!("{text} : {e:?}"));
        let t = root.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table().cloned());
        let t = t.unwrap_or_else(|| panic!("section"));
        assert_eq!(games_from_table(&t), list);
        assert_eq!(recent_from_table(&t), recent);
        let odd = toml::de::DeTable::parse("recents = [5, { id = \"1\" }, { nom = \"Seul\" }, { nom = \"Seul\", masque = true }]").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(recent_from_table(odd.get_ref()), [RecentGame { app_id: None, name: "Seul".into(), seen_ms: 0, hidden: false }], "entrées mal formées et doublons écartés");
        let odd = toml::de::DeTable::parse("programmes = [5, { nom = \"x\" }, { nom = \"\", chemin = \"a.exe\" }, { nom = \"Ok\", chemin = 'C:\\J\\Ok.exe' }]").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(games_from_table(odd.get_ref()), [ManualGame { name: "Ok".into(), path: "c:/j/ok.exe".into() }]);
    }

    fn app(id: u64, name: &str, exes: &[&str]) -> DetectableApp {
        DetectableApp { id, name: name.into(), exes: exes.iter().map(|e| e.to_string()).collect(), icon: None }
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
        // Même fichier revendiqué sans dossier par plusieurs jeux : le nom lu
        // dans le chemin les départage ; sinon aucun (jamais « le premier »).
        let shared = GameIndex::new(vec![app(1, "Astral Run", &["game.exe"]), app(2, "Vallon", &["game.exe"]), app(3, "Fond", &["data/game.exe"])], 0);
        assert_eq!(shared.find("d:/jeux/vallon/game.exe").map(|a| a.id), Some(2));
        assert_eq!(shared.find("d:/steam/astral run/bin/game.exe").map(|a| a.id), Some(1));
        assert_eq!(shared.find("d:/outils/divers/game.exe"), None);
        assert_eq!(shared.find("d:/x/data/game.exe").map(|a| a.id), Some(3), "le motif à dossier l'emporte");
    }

    #[test]
    fn manual_game_from_path() {
        let g = ManualGame::from_path(r"C:\Jeux\Mon Jeu.EXE").unwrap_or_else(|| panic!("chemin valide"));
        assert_eq!((g.name.as_str(), g.path.as_str(), g.file()), ("Mon Jeu", "c:/jeux/mon jeu.exe", "mon jeu.exe"));
        assert!(ManualGame::from_path("C:/Jeux/").is_none());
    }

    #[test]
    fn wish_follows_status_and_setting() {
        // Le relevé range les jeux : le premier est celui qu'on montre.
        let running = vec![RunningGame { app_id: None, name: "Récent".into(), start_ms: 9 }, RunningGame { app_id: Some(1), name: "Vieux".into(), start_ms: 5 }];
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

    /// Lot « ram » : la liste entière tient dans quelques blocs. 10 000
    /// applications (la taille de la liste de Discord) pèsent moins d'un Mo et demi ;
    /// la forme d'avant (une `String` par texte, deux tables de hachage) en
    /// tenait plus de six.
    #[test]
    fn index_stays_compact_and_keeps_the_list() {
        let apps: Vec<DetectableApp> = (0..10_000u64)
            .map(|i| {
                let mut exes = vec![format!("jeu{i}.exe")];
                if i % 3 == 0 {
                    exes.push(format!("dossier{i}/bin/win64/jeu{i}-win64-shipping.exe"));
                }
                DetectableApp { id: 5_000 + i, name: format!("Jeu inventé numéro {i}"), exes, icon: None }
            })
            .collect();
        let idx = GameIndex::new(apps.clone(), 7);
        assert!(idx.bytes() < 3 << 19, "{} octets", idx.bytes());
        // Rien n'est perdu : mêmes applications, mêmes motifs, même ordre.
        assert!(idx.apps().map(|a| a.to_app()).eq(apps.iter().cloned()));
        assert_eq!((idx.len(), idx.fetched_ms), (10_000, 7));
        assert_eq!(idx.app(5_000 + 9_999).map(|a| a.name), Some("Jeu inventé numéro 9999"));
        assert_eq!(idx.find_file("jeu4242.exe").map(|a| a.id), Some(5_000 + 4242));
        assert_eq!(idx.find_file("jeu3-win64-shipping.exe").map(|a| a.id), Some(5_003));
        assert_eq!(idx.find("d:/jeux/dossier3/bin/win64/jeu3-win64-shipping.exe").map(|a| a.id), Some(5_003));
        assert!(!idx.candidate("jeu10000.exe") && !idx.candidate("") && idx.candidate("jeu0.exe"));
        // Identifiant en double : le premier de la liste répond ; un motif répété ne compte qu'une fois.
        let twice = GameIndex::new(vec![app(1, "Premier", &["a.exe", "x/a.exe"]), app(1, "Second", &["b.exe"])], 0);
        assert_eq!((twice.app(1).map(|a| a.name), twice.len()), (Some("Premier"), 2));
        assert_eq!(twice.find_file("a.exe").map(|a| a.name), Some("Premier"));
        assert!(GameIndex::default().app(1).is_none() && GameIndex::default().find_file("a.exe").is_none());
    }

    #[test]
    fn index_finds_by_file_name_alone() {
        let idx = GameIndex::new(
            vec![app(1, "PUBG", &["win64/tslgame.exe", "tslgame_be.exe"]), app(2, "Autre", &["bin/game.exe"]), app(3, "Exact", &[">game.exe"]), app(4, "Encore", &["x64/game.exe", "data/seul.exe"]), app(5, "Doublon", &["tslgame_be.exe"])],
            0,
        );
        assert_eq!(idx.find_file("tslgame.exe").map(|a| a.id), Some(1), "motif à dossier, une seule application");
        assert_eq!(idx.find_file("tslgame_be.exe").map(|a| a.id), Some(1), "deux motifs sans dossier : le premier de la liste");
        assert_eq!(idx.find_file("game.exe").map(|a| a.id), Some(3), "le motif sans dossier l'emporte");
        assert_eq!(idx.find_file("seul.exe").map(|a| a.id), Some(4));
        assert_eq!(idx.find_file("explorer.exe"), None);
        let shared = GameIndex::new(vec![app(2, "Autre", &["bin/game.exe"]), app(4, "Encore", &["x64/game.exe"])], 0);
        assert_eq!(shared.find_file("game.exe"), None, "nom partagé, dossiers invérifiables");
        assert_eq!((idx.app(4).map(|a| a.name), idx.app(9)), (Some("Encore"), None));
    }

    #[test]
    fn hidden_game_is_never_announced() {
        let astral = RunningGame { app_id: Some(1), name: "Astral Drift".into(), start_ms: 9 };
        let vallon = RunningGame { app_id: Some(2), name: "Vallon".into(), start_ms: 5 };
        let mut a = Activities { running: vec![astral.clone(), vallon.clone()], ..Default::default() };
        assert!(note_running(&mut a.recent, &[astral.clone(), vallon.clone()]));
        assert!(!note_running(&mut a.recent, &[astral.clone(), vallon.clone()]), "rien de neuf : rien à réécrire");
        assert_eq!(a.recent.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Astral Drift", "Vallon"]);
        assert_eq!(a.wish(Some("online"), true, None).and_then(|w| w.game), Some(astral.clone()));
        // Masqué : le suivant est annoncé ; tous masqués : rien, et aucune op 3.
        a.recent.iter_mut().filter(|r| r.name == "Astral Drift").for_each(|r| r.hidden = true);
        assert_eq!((a.detected(), a.current()), (Some(&astral), Some(&vallon)));
        assert_eq!(a.wish(Some("online"), true, None).and_then(|w| w.game), Some(vallon));
        a.recent.iter_mut().for_each(|r| r.hidden = true);
        let w = a.wish(Some("online"), true, None).unwrap_or_else(|| panic!("statut connu"));
        assert!(w.game.is_none() && !w.has_game());
        assert!(a.commands(true, Some(true), Some(w)).iter().all(|c| !matches!(c, ActivityCmd::Presence(_))), "jeu masqué : rien n'est envoyé");
        // Une nouvelle partie remonte le jeu sans lever son masque.
        assert!(note_running(&mut a.recent, &[RunningGame { start_ms: 50, ..astral }]));
        assert_eq!(a.recent.first().map(|r| (r.name.as_str(), r.seen_ms, r.hidden)), Some(("Astral Drift", 50, true)));
        // Liste pleine : le plus ancien non masqué part, les masques restent.
        let many: Vec<RunningGame> = (0..MAX_RECENT as i64 + 5).map(|i| RunningGame { app_id: None, name: format!("Jeu {i}"), start_ms: 100 + i }).collect();
        note_running(&mut a.recent, &many);
        assert_eq!(a.recent.len(), MAX_RECENT);
        assert_eq!(a.recent.iter().filter(|r| r.hidden).count(), 2);
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
    fn a_failed_list_is_not_asked_again_at_every_ready() {
        let mut a = Activities::default();
        assert_eq!(a.commands(true, Some(true), None).len(), 1, "première demande");
        a.apply(ActivityEvent::DetectableFailed);
        a.on_ready();
        assert!(a.commands(true, Some(true), None).is_empty(), "échec récent : pas redemandée au READY suivant");
        // Sans échec, un READY la redemande (comportement d'avant).
        let mut b = Activities::default();
        assert_eq!(b.commands(true, Some(true), None).len(), 1);
        b.on_ready();
        assert_eq!(b.commands(true, Some(true), None).iter().filter(|c| matches!(c, ActivityCmd::LoadDetectable { .. })).count(), 1);
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
