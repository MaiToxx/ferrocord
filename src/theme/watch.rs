//! E/S d'un fichier de configuration (`theme.toml`, `settings.toml`) dans
//! un fil à part : le fil d'interface ne touche jamais au disque (règle
//! 1.3.1). Seule la première lecture est faite par l'appelant de
//! [`FileLink::open`], avant l'ouverture de la fenêtre. Le mécanisme est
//! commun à tous ces fichiers ([`Document`]) ; seuls les messages changent.
//!
//! - Surveillance par simple relevé de la date de modification et de la
//!   taille, plusieurs fois par seconde : rien n'est lu tant que rien ne
//!   change, et l'interface n'est réveillée que pour un vrai changement.
//!   Sous Windows (lot « cpu »), le relevé n'a lieu que si le système a
//!   signalé un changement dans le dossier (`platform::dirwatch`), plus un
//!   relevé de sûreté de loin en loin : au repos, le disque n'est plus touché.
//! - Enregistrement différé : les réglages du panneau sont regroupés et
//!   écrits après un court silence, pas à chaque mouvement de curseur.
//! - Le fichier l'emporte : s'il a changé depuis le dernier relevé, une
//!   écriture en attente est abandonnée et le fichier est rechargé. Un
//!   fichier refusé (erreur) n'est jamais écrasé, sauf par « Réinitialiser
//!   le fichier », qui en garde une copie `.bak`.
//! - Pas de boucle ni d'écran désaccordé : le texte écrit par l'application
//!   est mémorisé, et chaque état du fichier porte un numéro de génération ;
//!   un enregistrement demandé avant que l'interface ait vu la dernière
//!   génération est ignoré (l'interface va de toute façon afficher le fichier).

use super::{export, parse, Tokens};
use crate::i18n::{
    language, t as tx,
    verbatim::{f as vf, t as vt},
    Text,
};
use std::marker::PhantomData;
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError, Sender},
        Arc, Mutex,
    },
    thread::JoinHandle,
    time::{Duration, Instant, SystemTime},
};

/// Emplacement du thème de l'utilisateur.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("theme.toml"))
}

/// Taille au-delà de laquelle un fichier est refusé sans être lu.
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;

/// Nombre d'échecs de lecture consécutifs avant de signaler l'erreur.
const READ_RETRIES: u32 = 4;

#[derive(Clone, Copy, Debug)]
pub struct WatchConfig {
    /// Intervalle entre deux relevés de la date de modification.
    pub poll: Duration,
    /// Silence attendu avant d'écrire les réglages du panneau.
    pub debounce: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        // 250 ms : une modification se voit en bien moins d'une seconde.
        Self { poll: Duration::from_millis(250), debounce: Duration::from_millis(600) }
    }
}

/// Contenu d'un fichier surveillé : texte <-> valeur, sans E/S.
pub trait Document: Clone + PartialEq + Send + 'static {
    /// Ce qu'il faut faire d'un fichier refusé (fin du message d'erreur).
    const FIX_HINT: Text;
    /// Début du message d'un enregistrement raté.
    const SAVE_FAILED: Text;
    /// Chaque écriture réussie est annoncée (`FileUpdate::Saved`).
    const ACK_SAVES: bool = false;
    /// Lit le texte ; en cas d'erreur : ligne en cause et explication.
    fn parse(text: &str) -> Result<Self, (Option<usize>, String)>;
    /// Texte complet, relu à l'identique par [`Document::parse`].
    fn export(&self) -> String;
}

impl Document for Tokens {
    const FIX_HINT: Text = vt::THEME_FIX_HINT;
    const SAVE_FAILED: Text = vt::THEME_SAVE_FAILED;
    fn parse(text: &str) -> Result<Self, (Option<usize>, String)> {
        parse(text).map_err(|e| (e.line, e.message))
    }
    fn export(&self) -> String {
        export(self)
    }
}

/// Lien vers `theme.toml`.
pub type ThemeLink = FileLink<Tokens>;
/// Nouvelle du fil de thème.
pub type Update = FileUpdate<Tokens>;

/// Nouvelle du fil d'un fichier pour l'interface.
#[derive(Debug, PartialEq)]
pub enum FileUpdate<D> {
    /// Le fichier est valide : c'est la valeur à afficher.
    Loaded(D),
    /// Le fichier est refusé : la valeur en place est gardée et rien n'est
    /// enregistré tant qu'il n'est pas corrigé ou réinitialisé.
    FileError(String),
    /// Valeur importée depuis un autre fichier : à appliquer puis enregistrer.
    Imported(D),
    /// Échec d'un import, d'un export ou d'une écriture.
    Error(String),
    /// Information à afficher (export réussi…).
    Info(String),
    /// Cette valeur vient d'être écrite sur le disque (`sync` fait).
    Saved(D),
}

enum Req<D> {
    /// Valeur à enregistrer, génération du fichier vue par l'interface, et
    /// « tout de suite » (sans attendre le silence habituel).
    Save(D, u64, bool),
    Export(D, PathBuf),
    Import(PathBuf),
    /// Copie le fichier en `.bak` puis le remplace par cette valeur.
    ResetFile(D),
    Shutdown,
}

/// Message du fil : la nouvelle et la génération du fichier qui l'accompagne.
type Msg<D> = (FileUpdate<D>, u64);

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Lien entre l'interface et le fil d'un fichier. L'interface n'appelle que
/// des méthodes non bloquantes (envoi et lecture de canaux).
pub struct FileLink<D: Document> {
    tx: Sender<Req<D>>,
    rx: Receiver<Msg<D>>,
    wake: Arc<Mutex<Option<Wake>>>,
    thread: Option<JoinHandle<()>>,
    path: Option<PathBuf>,
    writable: bool,
    /// Dernière génération du fichier reçue par l'interface.
    seen: u64,
}

impl<D: Document> FileLink<D> {
    /// Lit le fichier une première fois (sur le fil appelant, avant la
    /// fenêtre), puis lance le fil de surveillance. Sans `path`, aucun
    /// fichier n'est lu ni écrit ; sans `writable`, le fichier est seulement lu.
    pub fn open(path: Option<PathBuf>, writable: bool, cfg: WatchConfig) -> (Self, Option<FileUpdate<D>>) {
        let mut file = path.clone().map(Watched::<D>::new);
        let first = file.as_mut().and_then(Watched::check);
        let generation = u64::from(first.is_some());
        let (tx, req_rx) = mpsc::channel();
        let (up_tx, rx) = mpsc::channel();
        let wake: Arc<Mutex<Option<Wake>>> = Arc::default();
        let worker = Worker { file, writable, cfg, out: up_tx, wake: wake.clone(), pending: None, generation };
        let name = path.as_deref().and_then(Path::file_stem).map_or_else(|| "fichier".into(), |s| s.to_string_lossy().into_owned());
        let thread = std::thread::Builder::new().name(name).spawn(move || worker.run(req_rx)).ok();
        (Self { tx, rx, wake, thread, path, writable, seen: generation }, first)
    }

    /// Fonction appelée quand une nouvelle arrive (réveil de l'interface).
    pub fn set_wake(&self, f: impl Fn() + Send + Sync + 'static) {
        if let Ok(mut w) = self.wake.lock() {
            *w = Some(Arc::new(f));
        }
    }

    pub fn poll(&mut self) -> Vec<FileUpdate<D>> {
        let mut out = Vec::new();
        for (u, generation) in self.rx.try_iter() {
            self.seen = self.seen.max(generation);
            out.push(u);
        }
        out
    }

    /// Demande l'enregistrement (différé et regroupé) de la valeur.
    pub fn save(&self, t: &D) {
        let _ = self.tx.send(Req::Save(t.clone(), self.seen, false));
    }

    /// Enregistrement sans attendre le silence habituel (liste des comptes :
    /// une fenêtre fermée juste après ne doit pas la perdre).
    pub fn save_now(&self, t: &D) {
        let _ = self.tx.send(Req::Save(t.clone(), self.seen, true));
    }

    pub fn export(&self, t: &D, to: PathBuf) {
        let _ = self.tx.send(Req::Export(t.clone(), to));
    }

    pub fn import(&self, from: PathBuf) {
        let _ = self.tx.send(Req::Import(from));
    }

    /// Remplace un fichier refusé par `t`, après en avoir gardé une copie.
    pub fn reset_file(&self, t: &D) {
        let _ = self.tx.send(Req::ResetFile(t.clone()));
    }

    /// Fichier surveillé.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Les réglages du panneau sont-ils enregistrés ?
    pub fn writable(&self) -> bool {
        self.writable && self.path.is_some()
    }

    /// Fichier proposé pour importer ou exporter un thème.
    pub fn share_path(&self) -> PathBuf {
        match self.path.as_deref().and_then(Path::parent) {
            Some(dir) => dir.join("theme-partage.toml"),
            None => std::env::temp_dir().join("ferrocord-theme-partage.toml"),
        }
    }
}

impl<D: Document> Drop for FileLink<D> {
    /// Écrit ce qui reste en attente avant de quitter.
    fn drop(&mut self) {
        let _ = self.tx.send(Req::Shutdown);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// Avec le signal du dossier, le fichier n'est plus relevé qu'un appel sur
/// tant (relevé de sûreté : 5 s pour le thème, 20 s pour les réglages), au
/// cas où le système n'aurait pas prévenu (disque réseau).
const FULL_EVERY: u32 = 20;
/// Nouvel essai d'abonnement quand le dossier n'existait pas encore.
const SIGNAL_RETRY: Duration = Duration::from_secs(10);

/// Signal de changement du dossier du fichier surveillé (lot « cpu »).
struct Signal {
    state: Option<crate::platform::dirwatch::DirSignal>,
    /// Prochain relevé de sûreté.
    full_at: Instant,
    /// Prochain essai d'abonnement (sans signal).
    retry_at: Instant,
}

impl Signal {
    fn new(file: Option<&Path>, now: Instant) -> Signal {
        // Premier relevé sans attendre : un changement fait entre la lecture
        // initiale et l'abonnement ne lèverait aucun signal.
        let mut s = Signal { state: None, full_at: now, retry_at: now };
        s.open(file, now);
        s
    }

    fn open(&mut self, file: Option<&Path>, now: Instant) {
        self.state = file.and_then(Path::parent).and_then(crate::platform::dirwatch::DirSignal::open);
        self.retry_at = now + SIGNAL_RETRY;
    }
}

struct Worker<D: Document> {
    file: Option<Watched<D>>,
    writable: bool,
    cfg: WatchConfig,
    out: Sender<Msg<D>>,
    wake: Arc<Mutex<Option<Wake>>>,
    pending: Option<(D, Instant)>,
    /// Génération du fichier : augmente à chaque nouvel état lu sur disque.
    generation: u64,
}

impl<D: Document> Worker<D> {
    fn send(&self, u: FileUpdate<D>) {
        if self.out.send((u, self.generation)).is_ok()
            && let Ok(w) = self.wake.lock()
            && let Some(w) = w.as_ref()
        {
            w();
        }
    }

    /// Relève le fichier et transmet un changement éventuel.
    fn check(&mut self) {
        if let Some(u) = self.file.as_mut().and_then(Watched::check) {
            // Modifié au dehors : le fichier l'emporte sur les réglages en attente.
            self.pending = None;
            self.generation += 1;
            self.send(u);
        }
    }

    /// Relève le fichier si le dossier a signalé un changement, si une
    /// lecture est à refaire, ou si le relevé de sûreté est dû (lot « cpu ») :
    /// sans signal du système, à chaque appel, comme avant.
    fn poll(&mut self, signal: &mut Signal, now: Instant) {
        let due = match signal.state.as_mut().map(crate::platform::dirwatch::DirSignal::changed) {
            // Signal levé : quelque chose a changé dans le dossier.
            Some(Some(true)) => true,
            Some(Some(false)) => now >= signal.full_at || self.file.as_ref().is_some_and(|f| f.read_failures > 0),
            // Abonnement perdu : relevé simple, et nouvel essai plus tard.
            Some(None) => {
                signal.state = None;
                true
            }
            None => {
                if now >= signal.retry_at {
                    signal.open(self.file.as_ref().map(|f| f.path.as_path()), now);
                }
                true
            }
        };
        if due {
            self.check();
            signal.full_at = now + self.cfg.poll * FULL_EVERY;
        }
    }

    fn run(mut self, rx: Receiver<Req<D>>) {
        let mut next_poll = Instant::now() + self.cfg.poll;
        let mut signal = Signal::new(self.file.as_ref().map(|f| f.path.as_path()), Instant::now());
        loop {
            let now = Instant::now();
            if now >= next_poll {
                self.poll(&mut signal, now);
                next_poll = now + self.cfg.poll;
            }
            if self.pending.as_ref().is_some_and(|(_, at)| now >= *at) {
                self.flush(false);
            }
            let deadline = self.pending.as_ref().map_or(next_poll, |(_, at)| (*at).min(next_poll));
            match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(Req::Save(t, seen, now)) => {
                    let broken = self.file.as_ref().is_some_and(|f| f.broken);
                    // Ancienne génération : l'interface n'a pas encore vu le fichier
                    // actuel, qui va remplacer ce qu'elle affiche.
                    if self.writable && self.file.is_some() && !broken && seen == self.generation {
                        let at = if now { Instant::now() } else { Instant::now() + self.cfg.debounce };
                        self.pending = Some((t, at));
                    }
                }
                Ok(Req::Export(t, to)) => match write_atomic(&to, &t.export()) {
                    Ok(()) => self.send(FileUpdate::Info(vf::theme_exported(language(), to.display()))),
                    Err(e) => self.send(FileUpdate::Error(vf::export_failed(language(), to.display(), e))),
                },
                Ok(Req::Import(from)) => {
                    let u = match read_limited(&from) {
                        Ok(text) => match parse_guarded::<D>(&text) {
                            Ok(t) => FileUpdate::Imported(t),
                            Err((line, message)) => FileUpdate::Error(vf::import_refused(language(), file_name(&from), at_line(line), message)),
                        },
                        Err(e) => FileUpdate::Error(vf::import_unreadable(language(), from.display(), e)),
                    };
                    self.send(u);
                }
                Ok(Req::ResetFile(t)) => self.reset_file(t),
                Ok(Req::Shutdown) | Err(RecvTimeoutError::Disconnected) => {
                    self.flush(true);
                    return;
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }

    /// Écrit la valeur en attente, sauf si le fichier a changé entre-temps.
    fn flush(&mut self, closing: bool) {
        let Some((t, _)) = self.pending.take() else { return };
        let Some(f) = self.file.as_mut() else { return };
        match f.write(&t.export()) {
            Ok(true) => {
                if D::ACK_SAVES {
                    self.send(FileUpdate::Saved(t));
                }
            }
            // Le fichier a changé depuis le dernier relevé : on le recharge
            // (sauf à la fermeture, où il n'y a plus personne pour l'afficher).
            Ok(false) => {
                if !closing {
                    self.check();
                }
            }
            Err(e) => {
                let msg = vf::save_failed_in(language(), D::SAVE_FAILED.get(language()), f.path.display(), e);
                self.send(FileUpdate::Error(msg));
            }
        }
    }

    fn reset_file(&mut self, t: D) {
        let Some(f) = self.file.as_mut() else { return };
        let mut bak = f.path.as_os_str().to_owned();
        bak.push(".bak");
        let bak = PathBuf::from(bak);
        if f.path.exists()
            && let Err(e) = fs::copy(&f.path, &bak)
        {
            let msg = vf::backup_failed(language(), bak.display(), e);
            return self.send(FileUpdate::Error(msg));
        }
        let was_broken = f.broken;
        f.stamp = stamp(&f.path);
        f.broken = false;
        let result = f.write(&t.export());
        if result.is_err() {
            // Rien n'a été remplacé : le fichier reste refusé.
            f.broken = was_broken;
        }
        let path = f.path.display().to_string();
        match result {
            Ok(_) => {
                self.pending = None;
                self.generation += 1;
                self.send(FileUpdate::Loaded(t));
                self.send(FileUpdate::Info(vf::file_reset(language(), bak.display())));
            }
            Err(e) => self.send(FileUpdate::Error(vf::reset_failed(language(), path, e))),
        }
    }
}

/// « ligne N » précédé d'une espace, ou rien.
fn at_line(line: Option<usize>) -> String {
    line.map(|l| vf::at_line(language(), l)).unwrap_or_default()
}

/// Message affiché tant que le fichier est refusé (détail technique entre parenthèses).
fn file_error<D: Document>(path: &Path, line: Option<usize>, why: &str) -> String {
    vf::file_has_error(language(), file_name(path), at_line(line), D::FIX_HINT.get(language()), why)
}

/// [`Document::parse`] sans panique possible : une panique de l'analyseur
/// TOML (tiers) devient une erreur ordinaire.
fn parse_guarded<D: Document>(text: &str) -> Result<D, (Option<usize>, String)> {
    crate::panics::contain(|| D::parse(text)).unwrap_or_else(|| Err((None, tx::THEME_PARSER_PANIC.get(language()).into())))
}

fn file_name(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string())
}

/// Lit un fichier texte d'au plus [`MAX_FILE_SIZE`] octets.
pub(crate) fn read_limited(path: &Path) -> io::Result<String> {
    let mut f = fs::File::open(path)?;
    if f.metadata()?.len() > MAX_FILE_SIZE {
        return Err(io::Error::other(vf::file_over_kb(language(), MAX_FILE_SIZE / 1024)));
    }
    let mut bytes = Vec::new();
    Read::take(&mut f, MAX_FILE_SIZE + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_FILE_SIZE {
        return Err(io::Error::other(vf::file_over_kb(language(), MAX_FILE_SIZE / 1024)));
    }
    String::from_utf8(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidData, vt::NOT_UTF8.get(language())))
}

/// Écrit dans un fichier voisin, le force sur le disque, puis le renomme :
/// un éditeur ou le fil de surveillance ne voit jamais un fichier à moitié
/// écrit. Un texte vide n'est jamais écrit.
pub(crate) fn write_atomic(path: &Path, text: &str) -> io::Result<()> {
    if text.trim().is_empty() {
        return Err(io::Error::other(vt::EMPTY_TEXT.get(language())));
    }
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    let written = (|| {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()
    })();
    let result = written.and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    modified: Option<SystemTime>,
    len: u64,
}

fn stamp(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some(Stamp { modified: m.modified().ok(), len: m.len() })
}

/// Un fichier surveillé et ce qu'on en sait déjà.
struct Watched<D> {
    path: PathBuf,
    stamp: Option<Stamp>,
    /// Dernier contenu vu (lu, ou écrit par nous).
    text: Option<String>,
    /// Le dernier contenu lu a été refusé : on ne l'écrase pas.
    broken: bool,
    read_failures: u32,
    doc: PhantomData<fn() -> D>,
}

impl<D: Document> Watched<D> {
    fn new(path: PathBuf) -> Self {
        Self { path, stamp: None, text: None, broken: false, read_failures: 0, doc: PhantomData }
    }

    fn refuse(&mut self, why: String) -> Option<FileUpdate<D>> {
        self.broken = true;
        self.text = None;
        Some(FileUpdate::FileError(file_error::<D>(&self.path, None, &why)))
    }

    /// Nouvelle à transmettre si le fichier a vraiment changé.
    fn check(&mut self) -> Option<FileUpdate<D>> {
        let now = stamp(&self.path);
        if now == self.stamp {
            return None;
        }
        self.stamp = now;
        // Fichier supprimé : on garde le thème en place.
        let len = now?.len;
        if len > MAX_FILE_SIZE {
            return self.refuse(vf::over_kb_not_read(language(), MAX_FILE_SIZE / 1024));
        }
        let text = match read_limited(&self.path) {
            Ok(t) => t,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => return self.refuse(e.to_string()),
            Err(e) => {
                // Fichier verrouillé pendant son écriture : on réessaie un peu,
                // puis on le signale une seule fois (pas de relecture sans fin).
                self.read_failures += 1;
                if self.read_failures < READ_RETRIES {
                    self.stamp = None;
                    return None;
                }
                self.read_failures = 0;
                return self.refuse(vf::file_unreadable(language(), e));
            }
        };
        self.read_failures = 0;
        // Fichier vide : traité comme absent (l'application n'en écrit jamais).
        if text.trim().is_empty() {
            self.broken = false;
            self.text = Some(text);
            return None;
        }
        if self.text.as_ref() == Some(&text) {
            self.text = Some(text);
            return None;
        }
        let u = match parse_guarded::<D>(&text) {
            Ok(t) => {
                self.broken = false;
                FileUpdate::Loaded(t)
            }
            Err((line, message)) => {
                self.broken = true;
                FileUpdate::FileError(file_error::<D>(&self.path, line, &message))
            }
        };
        self.text = Some(text);
        Some(u)
    }

    /// Écrit `text`, sauf si le fichier a changé depuis le dernier relevé
    /// (renvoie alors `false`) ou s'il est refusé.
    fn write(&mut self, text: &str) -> io::Result<bool> {
        if self.broken || stamp(&self.path) != self.stamp {
            return Ok(false);
        }
        write_atomic(&self.path, text)?;
        self.stamp = stamp(&self.path);
        self.text = Some(text.to_string());
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::Preset;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Dossier temporaire propre à chaque test, supprimé à la fin.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ferrocord-theme-{}-{}-{name}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{dir:?} : {e}"));
            Self(dir)
        }
        fn file(&self) -> PathBuf {
            self.0.join("theme.toml")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, text: &str) {
        fs::write(path, text).unwrap_or_else(|e| panic!("{path:?} : {e}"));
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap_or_default()
    }

    #[test]
    fn watched_reports_only_real_changes() {
        let dir = TempDir::new("watched");
        let mut w = Watched::<Tokens>::new(dir.file());
        assert_eq!(w.check(), None, "fichier absent");

        write(&dir.file(), "base = \"clair\"\n");
        assert_eq!(w.check(), Some(Update::Loaded(Preset::Clair.tokens())));
        assert_eq!(w.check(), None, "rien n'a changé");

        // Même contenu réécrit (date changée) : pas de nouvelle.
        std::thread::sleep(Duration::from_millis(20));
        write(&dir.file(), "base = \"clair\"\n");
        assert_eq!(w.check(), None);

        // Écriture faite par l'application : pas de nouvelle non plus.
        w.check();
        assert_eq!(w.write(&export(&Preset::Minuit.tokens())).ok(), Some(true));
        assert_eq!(w.check(), None);

        // Fichier invalide : erreur avec la ligne, puis retour à la normale.
        write(&dir.file(), "base = \"minuit\"\n[couleurs]\nbg0 = \"#xyz\"\n");
        match w.check() {
            Some(Update::FileError(m)) => assert!(m.contains("ligne 3") && m.contains("theme.toml"), "{m}"),
            other => panic!("erreur attendue, reçu {other:?}"),
        }
        write(&dir.file(), "base = \"contraste\"\n");
        assert_eq!(w.check(), Some(Update::Loaded(Preset::Contraste.tokens())));

        // Fichier supprimé : rien (le thème en place est gardé).
        fs::remove_file(dir.file()).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(w.check(), None);
    }

    #[test]
    fn invalid_utf8_is_an_error() {
        let dir = TempDir::new("utf8");
        fs::write(dir.file(), [0xff, 0xfe, 0x00]).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(Watched::<Tokens>::new(dir.file()).check(), Some(Update::FileError(_))));
    }

    #[test]
    fn oversized_file_is_refused_unread() {
        let dir = TempDir::new("big");
        let big = format!("# {}\n", "x".repeat(MAX_FILE_SIZE as usize));
        write(&dir.file(), &big);
        let mut w = Watched::<Tokens>::new(dir.file());
        match w.check() {
            Some(Update::FileError(m)) => assert!(m.contains("Ko"), "{m}"),
            other => panic!("refus attendu, reçu {other:?}"),
        }
        assert!(w.text.is_none(), "le contenu ne doit pas être gardé");
        assert_eq!(w.check(), None, "un seul message tant qu'il ne change pas");
        assert_eq!(w.write("base = \"clair\"\n").ok(), Some(false), "jamais écrasé");
    }

    /// Fichier durablement illisible (ici : un dossier à sa place) : signalé
    /// une seule fois, en français, puis plus relu tant qu'il ne change pas.
    #[test]
    fn unreadable_file_is_reported_once() {
        let dir = TempDir::new("illisible");
        fs::create_dir_all(dir.file()).unwrap_or_else(|e| panic!("{e}"));
        let mut w = Watched::<Tokens>::new(dir.file());
        let reports: Vec<Update> = (0..READ_RETRIES * 5).filter_map(|_| w.check()).collect();
        assert_eq!(reports.len(), 1, "{reports:?}");
        assert!(matches!(&reports[0], Update::FileError(m) if m.contains("contient une erreur") && m.contains("fichier illisible")), "{reports:?}");
        assert!(w.broken);
    }

    /// Fichier vide : traité comme absent, et jamais écrit par l'application.
    #[test]
    fn empty_file_is_like_no_file() {
        let dir = TempDir::new("vide");
        write(&dir.file(), "  \n");
        let mut w = Watched::<Tokens>::new(dir.file());
        assert_eq!(w.check(), None);
        assert!(write_atomic(&dir.file(), "").is_err());
        assert!(!dir.0.join("theme.toml.tmp").exists());
    }

    #[test]
    fn rename_failure_cleans_temp_file() {
        let dir = TempDir::new("rename");
        // Destination impossible : c'est un dossier non vide.
        let target = dir.0.join("cible");
        fs::create_dir_all(target.join("dedans")).unwrap_or_else(|e| panic!("{e}"));
        assert!(write_atomic(&target, "base = \"clair\"\n").is_err());
        assert!(!dir.0.join("cible.tmp").exists());
    }

    fn fast() -> WatchConfig {
        WatchConfig { poll: Duration::from_millis(20), debounce: Duration::from_millis(150) }
    }

    /// Garde-fou contre un fil bloqué : jamais atteint quand tout va bien,
    /// il ne décide donc pas du résultat, même sous forte charge.
    const GUARD: Duration = Duration::from_secs(30);

    /// Réveils du lien : on attend un événement du fil plutôt qu'un délai.
    fn wakes(link: &ThemeLink) -> Receiver<()> {
        let (tx, rx) = mpsc::channel();
        link.set_wake(move || {
            let _ = tx.send(());
        });
        rx
    }

    fn wait_wake(rx: &Receiver<()>) {
        rx.recv_timeout(GUARD).unwrap_or_else(|e| panic!("le fil de surveillance ne répond pas : {e}"));
    }

    /// Le fil traite les demandes dans l'ordre : quand la réponse à cet import
    /// (fichier absent, donc une erreur) arrive, toutes les demandes envoyées
    /// avant ont été traitées, et une écriture immédiate a été faite.
    fn sync(link: &ThemeLink, rx: &Receiver<()>, dir: &TempDir) {
        link.import(dir.0.join("absent-synchro.toml"));
        wait_wake(rx);
    }

    /// Attend que le fichier contienne `want` (écriture différée par le fil).
    fn wait_content(path: &Path, want: &str) {
        let end = Instant::now() + GUARD;
        while read(path) != want {
            assert!(Instant::now() < end, "contenu attendu jamais écrit : {want:?}, lu {:?}", read(path));
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Attend une nouvelle (au plus `limit`).
    fn next(link: &mut ThemeLink, limit: Duration) -> Option<Update> {
        let end = Instant::now() + limit;
        while Instant::now() < end {
            if let Some(u) = link.poll().into_iter().next() {
                return Some(u);
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        None
    }

    #[test]
    fn first_read_happens_in_open() {
        let dir = TempDir::new("open");
        write(&dir.file(), "base = \"minuit\"\n");
        let (mut link, first) = ThemeLink::open(Some(dir.file()), true, fast());
        assert_eq!(first, Some(Update::Loaded(Preset::Minuit.tokens())));
        assert_eq!(next(&mut link, Duration::from_millis(150)), None, "pas de seconde lecture du même fichier");
    }

    #[test]
    fn hot_reload_in_temp_dir() {
        let dir = TempDir::new("hot");
        let (mut link, first) = ThemeLink::open(Some(dir.file()), true, fast());
        assert_eq!(first, None);
        let wakes = Arc::new(AtomicUsize::new(0));
        let w = wakes.clone();
        link.set_wake(move || {
            w.fetch_add(1, Ordering::SeqCst);
        });

        // Pas de fichier, pas de nouvelle, pas de réveil.
        assert_eq!(next(&mut link, Duration::from_millis(150)), None);
        assert_eq!(wakes.load(Ordering::SeqCst), 0);

        // Modification « dans un éditeur » : vue en moins d'une seconde.
        let t0 = Instant::now();
        write(&dir.file(), "base = \"aere\"\n[couleurs]\naccent = \"#336699\"\n");
        let mut want = Preset::Aere.tokens();
        want.colors.accent = eframe::egui::Color32::from_rgb(0x33, 0x66, 0x99);
        assert_eq!(next(&mut link, Duration::from_secs(1)), Some(Update::Loaded(want)));
        assert!(t0.elapsed() < Duration::from_secs(1));
        assert_eq!(wakes.load(Ordering::SeqCst), 1);

        // Rien ne change : aucune nouvelle, aucun réveil.
        assert_eq!(next(&mut link, Duration::from_millis(200)), None);
        assert_eq!(wakes.load(Ordering::SeqCst), 1);

        // Réglages du panneau : regroupés, écrits une fois, sans boucle.
        let mut t = Preset::Clair.tokens();
        for i in 0..10 {
            t.typography.body = 12.0 + i as f32;
            link.save(&t);
        }
        assert!(read(&dir.file()).contains("#336699"), "écrit trop tôt");
        wait_content(&dir.file(), &export(&t));
        assert_eq!(next(&mut link, Duration::from_millis(200)), None, "l'écriture de l'app ne doit pas revenir");
        assert_eq!(wakes.load(Ordering::SeqCst), 1);

        // Fichier cassé : erreur, puis réparation.
        write(&dir.file(), "[typographie]\nbody = 500\n");
        match next(&mut link, GUARD) {
            Some(Update::FileError(m)) => assert!(m.contains("ligne 2") && m.contains("hors bornes"), "{m}"),
            other => panic!("erreur attendue, reçu {other:?}"),
        }
        write(&dir.file(), "base = \"minuit\"\n");
        assert_eq!(next(&mut link, GUARD), Some(Update::Loaded(Preset::Minuit.tokens())));
    }

    /// Panique de l'analyseur sur le fichier : une erreur comme une autre,
    /// et le fil de surveillance continue (le fichier suivant est lu).
    #[test]
    fn parser_panic_keeps_the_watcher_alive() {
        let dir = TempDir::new("panique");
        let (mut link, _) = ThemeLink::open(Some(dir.file()), true, fast());
        write(&dir.file(), super::super::file::PANIC_TRIGGER);
        match next(&mut link, GUARD) {
            Some(Update::FileError(m)) => assert!(m.contains("illisible"), "{m}"),
            other => panic!("erreur attendue, reçu {other:?}"),
        }
        write(&dir.file(), "base = \"minuit\"
");
        assert_eq!(next(&mut link, GUARD), Some(Update::Loaded(Preset::Minuit.tokens())));
        // Le lien répond encore (aucun verrou figé) et se ferme proprement.
        link.set_wake(|| {});
        drop(link);
    }

    /// Modification externe pendant l'attente d'écriture : elle survit, y
    /// compris à l'écriture faite à la fermeture.
    #[test]
    fn external_edit_wins_over_pending_save() {
        let dir = TempDir::new("gagne");
        write(&dir.file(), "base = \"clair\"\n");
        let cfg = WatchConfig { poll: Duration::from_secs(60), debounce: Duration::from_secs(60) };
        {
            let (link, _) = ThemeLink::open(Some(dir.file()), true, cfg);
            link.save(&Preset::Minuit.tokens());
            std::thread::sleep(Duration::from_millis(50));
            write(&dir.file(), "base = \"contraste\"\n# modifié dans un éditeur\n");
        }
        assert_eq!(read(&dir.file()), "base = \"contraste\"\n# modifié dans un éditeur\n");
    }

    /// Même chose pendant le fonctionnement normal : l'écriture différée est
    /// abandonnée et le fichier est rechargé.
    #[test]
    fn external_edit_during_debounce_is_reloaded() {
        let dir = TempDir::new("gagne2");
        write(&dir.file(), "base = \"clair\"\n");
        let cfg = WatchConfig { poll: Duration::from_secs(60), debounce: Duration::from_millis(100) };
        let (mut link, _) = ThemeLink::open(Some(dir.file()), true, cfg);
        // Modifié avant la demande : le fil (sans relevé avant 60 s) ne le
        // découvre qu'à l'écriture, comme pendant le silence, mais sans
        // dépendre d'une écriture du test plus rapide que les 100 ms.
        write(&dir.file(), "base = \"contraste\"\n");
        link.save(&Preset::Minuit.tokens());
        assert_eq!(next(&mut link, GUARD), Some(Update::Loaded(Preset::Contraste.tokens())));
        assert_eq!(read(&dir.file()), "base = \"contraste\"\n");
    }

    /// Un `Loaded` encore en file quand l'interface enregistre : cet
    /// enregistrement, fait sans avoir vu le fichier, est ignoré, et l'écran
    /// (qui va afficher le `Loaded`) reste d'accord avec le fichier.
    #[test]
    fn stale_save_is_ignored() {
        let dir = TempDir::new("course");
        // Silence nul : un enregistrement accepté est écrit avant la demande
        // suivante, donc `sync` suffit à constater qu'il a été ignoré.
        let cfg = WatchConfig { poll: Duration::from_millis(20), debounce: Duration::ZERO };
        let (mut link, _) = ThemeLink::open(Some(dir.file()), true, cfg);
        let rx = wakes(&link);
        write(&dir.file(), "base = \"contraste\"\n");
        // Le fil a vu le fichier (réveil du `Loaded`)…
        wait_wake(&rx);
        // … mais l'interface n'a pas encore lu le `Loaded` qui l'attend.
        link.save(&Preset::Minuit.tokens());
        sync(&link, &rx, &dir);
        assert_eq!(read(&dir.file()), "base = \"contraste\"\n");
        let got = link.poll();
        assert_eq!(got.first(), Some(&Update::Loaded(Preset::Contraste.tokens())), "{got:?}");
        assert!(matches!(got.as_slice(), [_, Update::Error(_)]), "seule la réponse de synchro suit : {got:?}");
        // Après avoir vu le fichier, un enregistrement passe normalement.
        link.save(&Preset::Minuit.tokens());
        sync(&link, &rx, &dir);
        assert_eq!(read(&dir.file()), export(&Preset::Minuit.tokens()));
        assert!(matches!(link.poll().as_slice(), [Update::Error(_)]), "l'écriture de l'app ne doit pas revenir");
    }

    /// Fichier refusé : jamais écrasé par le panneau ; « Réinitialiser le
    /// fichier » en garde une copie `.bak` puis le remplace.
    #[test]
    fn broken_file_is_never_overwritten() {
        let dir = TempDir::new("casse");
        let broken = "base = \"clair\"\n[couleurs\n";
        write(&dir.file(), broken);
        let (mut link, first) = ThemeLink::open(Some(dir.file()), true, fast());
        assert!(matches!(first, Some(Update::FileError(ref m)) if m.contains("ligne 2")), "{first:?}");
        let rx = wakes(&link);
        link.save(&Preset::Minuit.tokens());
        sync(&link, &rx, &dir);
        assert_eq!(read(&dir.file()), broken);
        assert!(matches!(link.poll().as_slice(), [Update::Error(_)]));
        {
            let (link2, _) = ThemeLink::open(Some(dir.file()), true, fast());
            link2.save(&Preset::Minuit.tokens());
        }
        assert_eq!(read(&dir.file()), broken, "pas même à la fermeture");

        link.reset_file(&Preset::Aere.tokens());
        wait_wake(&rx);
        wait_wake(&rx);
        let got = link.poll();
        assert_eq!(got.first(), Some(&Update::Loaded(Preset::Aere.tokens())), "{got:?}");
        assert!(matches!(got.get(1), Some(Update::Info(m)) if m.contains("theme.toml.bak")), "{got:?}");
        assert_eq!(read(&dir.0.join("theme.toml.bak")), broken);
        assert_eq!(read(&dir.file()), export(&Preset::Aere.tokens()));
        // Le fichier est sain : le panneau enregistre de nouveau.
        link.save(&Preset::Minuit.tokens());
        wait_content(&dir.file(), &export(&Preset::Minuit.tokens()));
    }

    #[test]
    fn pending_save_is_flushed_on_drop() {
        let dir = TempDir::new("drop");
        let cfg = WatchConfig { poll: Duration::from_millis(20), debounce: Duration::from_secs(60) };
        let t = Preset::Compact.tokens();
        {
            let (link, _) = ThemeLink::open(Some(dir.file()), true, cfg);
            link.save(&t);
        }
        assert_eq!(fs::read_to_string(dir.file()).ok(), Some(export(&t)));
    }

    #[test]
    fn read_only_link_never_writes() {
        let dir = TempDir::new("ro");
        {
            let (link, _) = ThemeLink::open(Some(dir.file()), false, fast());
            link.save(&Preset::Clair.tokens());
            std::thread::sleep(Duration::from_millis(120));
        }
        assert!(!dir.file().exists());
    }

    #[test]
    fn export_then_import() {
        let dir = TempDir::new("io");
        let (mut link, _) = ThemeLink::open(None, true, fast());
        let to = dir.0.join("partage").join("mon-theme.toml");
        let t = Preset::Contraste.tokens();
        link.export(&t, to.clone());
        assert!(matches!(next(&mut link, GUARD), Some(Update::Info(_))));
        link.import(to);
        assert_eq!(next(&mut link, GUARD), Some(Update::Imported(t)));
        link.import(dir.0.join("absent.toml"));
        assert!(matches!(next(&mut link, GUARD), Some(Update::Error(_))));
    }
}
