//! Fil « ferrocord-installation » : registre, vérification et téléchargement
//! des mises à jour. Bloqué sur sa file au repos ; seule la vérification
//! quotidienne le réveille (attente bornée, aucun réveil de l'interface).
//!
//! Rythme des vérifications automatiques : une au lancement, puis une par
//! jour au plus ; jamais deux à moins de [`MIN_GAP`] d'écart, même d'un
//! lancement à l'autre (l'heure de la dernière est gardée, `memo`). Le
//! bouton « Rechercher maintenant » vérifie toujours.

use super::hive::Hive;
use super::memo::{self, Memo};
use super::update::{fetch, source, swap, Manifest, Status, Version};
use super::{system, Autostart, InstallEvent, Installer};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Une vérification par jour au plus, en plus de celle du démarrage.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// Écart minimal entre deux vérifications automatiques (lancements répétés).
pub const MIN_GAP: Duration = Duration::from_secs(60 * 60);

type Wake = Arc<dyn Fn() + Send + Sync>;

/// Ce dont le fil a besoin.
pub struct Config {
    pub hive: Box<dyn Hive>,
    /// L'exécutable lancé (`None` : rien n'est remplacé).
    pub exe: Option<PathBuf>,
    /// Clé publique des manifestes (`None` : mises à jour désactivées).
    pub key: Option<[u8; 32]>,
    /// Adresse du manifeste quand `local.toml` n'en donne pas (vide : aucune).
    pub default_url: String,
    /// Où retenir le réglage et l'heure de la dernière vérification.
    pub memo: Option<PathBuf>,
    /// Version de cet exécutable : rien de plus ancien ni d'égal n'est proposé.
    pub current: Version,
    pub min_gap: Duration,
    pub wake: Wake,
}

impl Config {
    /// Réglages de l'application : dépôt configuré à la compilation, fichier
    /// `installation.json` à côté de `local.toml`.
    pub fn for_app(hive: Box<dyn Hive>, exe: Option<PathBuf>, key: Option<[u8; 32]>, wake: Wake) -> Config {
        Config { hive, exe, key, default_url: source::default_manifest_url(), memo: memo::default_path(), current: Version::current(), min_gap: MIN_GAP, wake }
    }
}

enum Job {
    Load,
    Autostart(Autostart),
    Protocol(bool),
    Configure(String),
    /// Demande de l'utilisateur : toujours faite.
    Check,
    /// Échéance quotidienne.
    AutoCheck,
    SetAuto(bool),
    Download(Manifest),
    Confirm,
}

/// Réglage « Vérifier automatiquement », lu par l'interface sans attendre.
const AUTO_UNKNOWN: u8 = 0;
const AUTO_OFF: u8 = 1;
const AUTO_ON: u8 = 2;

pub struct InstallService {
    jobs: mpsc::Sender<Job>,
    events: mpsc::Receiver<InstallEvent>,
    auto: Arc<AtomicU8>,
}

struct Worker {
    hive: Box<dyn Hive>,
    exe: Option<PathBuf>,
    key: Option<[u8; 32]>,
    default_url: String,
    url: String,
    memo_path: Option<PathBuf>,
    memo: Memo,
    current: Version,
    min_gap: Duration,
    next_check: Option<Instant>,
    auto: Arc<AtomicU8>,
    tx: mpsc::Sender<InstallEvent>,
    wake: Wake,
    rt: Option<tokio::runtime::Runtime>,
    client: Option<reqwest::Client>,
}

impl InstallService {
    /// Sans dépôt par défaut ni fichier retenu (tests, outils).
    pub fn start(hive: Box<dyn Hive>, exe: Option<PathBuf>, key: Option<[u8; 32]>, wake: Wake) -> Self {
        Self::start_with(Config { hive, exe, key, default_url: String::new(), memo: None, current: Version::current(), min_gap: MIN_GAP, wake })
    }

    pub fn start_with(c: Config) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, events) = mpsc::channel();
        let auto = Arc::new(AtomicU8::new(AUTO_UNKNOWN));
        let mut w = Worker {
            hive: c.hive,
            exe: c.exe,
            key: c.key,
            default_url: c.default_url,
            url: String::new(),
            memo_path: c.memo,
            memo: Memo::default(),
            current: c.current,
            min_gap: c.min_gap,
            next_check: None,
            auto: auto.clone(),
            tx,
            wake: c.wake,
            rt: None,
            client: None,
        };
        let spawned = std::thread::Builder::new().name("ferrocord-installation".into()).spawn(move || {
            if let Some(p) = &w.memo_path {
                w.memo = memo::read(p);
            }
            w.publish_auto();
            loop {
                let job = match w.next_check {
                    Some(at) => match rx.recv_timeout(at.saturating_duration_since(Instant::now())) {
                        Ok(j) => j,
                        Err(mpsc::RecvTimeoutError::Timeout) => Job::AutoCheck,
                        Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    },
                    None => match rx.recv() {
                        Ok(j) => j,
                        Err(_) => return,
                    },
                };
                w.run(job);
            }
        });
        // Fil impossible à créer : rien ne se fait, l'interface reste sur « inconnu ».
        drop(spawned);
        InstallService { jobs, events, auto }
    }
}

impl Worker {
    fn send(&self, e: InstallEvent) {
        let _ = self.tx.send(e);
        (self.wake)();
    }

    fn registry(&self) {
        let (autostart, protocol) = match &self.exe {
            Some(exe) => (system::autostart(&*self.hive), system::protocol(&*self.hive, exe)),
            None => (Autostart::Off, super::Protocol::Free),
        };
        self.send(InstallEvent::Registry { autostart, protocol });
    }

    fn layout(&self) -> Option<swap::Layout> {
        self.exe.clone().map(|exe| swap::Layout { exe })
    }

    fn net(&mut self) -> Result<(), String> {
        if self.rt.is_none() {
            self.rt = Some(tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?);
        }
        if self.client.is_none() {
            self.client = Some(fetch::client()?);
        }
        Ok(())
    }

    fn auto_on(&self) -> bool {
        self.memo.verifier_auto.unwrap_or(true)
    }

    fn publish_auto(&self) {
        self.auto.store(if self.auto_on() { AUTO_ON } else { AUTO_OFF }, Ordering::SeqCst);
    }

    /// Écriture impossible : le réglage vaut pour ce lancement seulement.
    fn save_memo(&self) {
        if let Some(p) = &self.memo_path {
            // Le fichier est relu : la proposition d'installation y est écrite par ailleurs.
            let kept = memo::read(p);
            let _ = memo::write(p, &Memo { proposition_vue: kept.proposition_vue, proposition_chemin: kept.proposition_chemin, ..self.memo.clone() });
        }
    }

    fn run(&mut self, job: Job) {
        match job {
            Job::Load => self.registry(),
            Job::Autostart(want) => {
                if let Some(exe) = self.exe.clone()
                    && let Err(e) = system::set_autostart(&mut *self.hive, &exe, want)
                {
                    self.send(InstallEvent::Failed(e.to_string()));
                }
                self.registry();
            }
            Job::Protocol(on) => {
                if let Some(exe) = self.exe.clone() {
                    let r = if on { system::register_protocol(&mut *self.hive, &exe) } else { system::unregister_protocol(&mut *self.hive, &exe) };
                    if let Err(e) = r {
                        self.send(InstallEvent::Failed(e.to_string()));
                    }
                }
                self.registry();
            }
            Job::Configure(url) => {
                self.url = source::effective(&url, &self.default_url);
                self.auto_check(true);
            }
            Job::Check => self.check(),
            Job::AutoCheck => self.auto_check(false),
            Job::SetAuto(on) => {
                self.memo.verifier_auto = Some(on);
                self.save_memo();
                self.publish_auto();
                (self.wake)();
                if on && self.next_check.is_none() {
                    self.auto_check(false);
                } else if !on {
                    self.next_check = None;
                }
            }
            Job::Download(m) => self.download(m),
            Job::Confirm => {
                if let Some(l) = self.layout() {
                    let _ = swap::confirm(&l);
                }
            }
        }
    }

    /// Sans clé ou sans adresse, rien à vérifier : l'état le dit.
    fn unavailable(&self) -> Option<Status> {
        if self.key.is_none() {
            Some(Status::NoKey)
        } else if self.url.is_empty() {
            Some(Status::NoSource)
        } else {
            None
        }
    }

    /// Vérification automatique (lancement, échéance, réglage réactivé) :
    /// seulement si le réglage le permet et que la dernière date assez.
    /// `announce` : dire « rien vérifié » quand on ne vérifie pas (au
    /// lancement ; ensuite l'état affiché reste ce qu'il était).
    fn auto_check(&mut self, announce: bool) {
        self.next_check = None;
        if let Some(s) = self.unavailable() {
            self.send(InstallEvent::Update(s));
            return;
        }
        if !self.auto_on() {
            if announce {
                self.rest();
            }
            return;
        }
        let since = memo::now().checked_sub(self.memo.derniere_verification).map(Duration::from_secs);
        // Horloge reculée (`None`) : on vérifie plutôt que d'attendre sans fin.
        if let Some(since) = since
            && since < self.min_gap
        {
            self.next_check = Some(Instant::now() + (self.min_gap - since));
            if announce {
                self.rest();
            }
            return;
        }
        self.check();
    }

    /// Pas de vérification maintenant : une version déjà téléchargée reste
    /// annoncée (sans réseau), sinon « rien vérifié ».
    fn rest(&self) {
        let pending = self.layout().and_then(|l| l.read().pending).and_then(|p| Version::parse(&p.version));
        self.send(InstallEvent::Update(pending.map_or(Status::Idle, Status::Ready)));
    }

    fn check(&mut self) {
        if let Some(s) = self.unavailable() {
            self.send(InstallEvent::Update(s));
            return;
        }
        let Some(key) = self.key else { return };
        self.send(InstallEvent::Update(Status::Checking));
        let status = match self.net() {
            Err(e) => Status::Failed(e),
            Ok(()) => {
                let url = self.url.clone();
                let (Some(rt), Some(client)) = (&self.rt, &self.client) else { return };
                match rt.block_on(fetch::manifest(client, &url, &key)) {
                    Err(e) => Status::Failed(e),
                    Ok(m) => {
                        let layout = self.layout();
                        let state = layout.as_ref().map(swap::Layout::read).unwrap_or_default();
                        if state.pending.as_ref().is_some_and(|p| p.version == m.version.to_string()) {
                            Status::Ready(m.version)
                        } else if m.version > self.current && state.refused.as_deref() != Some(&m.version.to_string()) {
                            Status::Available(m)
                        } else {
                            // Version égale ou plus ancienne : jamais proposée (pas de retour en arrière).
                            let current = self.current;
                            Status::UpToDate { refused: state.refused.filter(|r| Version::parse(r).is_some_and(|v| v > current)) }
                        }
                    }
                }
            }
        };
        self.memo.derniere_verification = memo::now();
        self.save_memo();
        self.next_check = self.auto_on().then(|| Instant::now() + CHECK_EVERY);
        self.send(InstallEvent::Update(status));
    }

    fn download(&mut self, m: Manifest) {
        let Some(layout) = self.layout() else { return };
        if m.version <= self.current {
            self.send(InstallEvent::Update(Status::Failed("version plus ancienne que celle installée".into())));
            return;
        }
        if !swap::writable(&layout) {
            self.send(InstallEvent::Update(Status::Failed("dossier de l'exécutable en lecture seule".into())));
            return;
        }
        if let Err(e) = self.net() {
            self.send(InstallEvent::Update(Status::Failed(e)));
            return;
        }
        self.send(InstallEvent::Update(Status::Downloading { done: 0, total: m.size }));
        let part = layout.file("ferrocord.new.exe.part");
        let (tx, wake, total) = (self.tx.clone(), self.wake.clone(), m.size);
        let step = (total / 100).max(256 * 1024);
        let last = std::sync::atomic::AtomicU64::new(0);
        let progress = move |done: u64| {
            if done.saturating_sub(last.load(Ordering::Relaxed)) >= step {
                last.store(done, Ordering::Relaxed);
                let _ = tx.send(InstallEvent::Update(Status::Downloading { done, total }));
                wake();
            }
        };
        let (Some(rt), Some(client)) = (&self.rt, &self.client) else { return };
        let got = rt.block_on(fetch::binary(client, &m, &part, &progress));
        let status = match got.and_then(|()| std::fs::rename(&part, layout.file(swap::NEW)).map_err(|e| e.to_string())).and_then(|()| swap::stage(&layout, &m).map_err(|e| e.to_string())) {
            Ok(()) => Status::Ready(m.version),
            Err(e) => {
                let _ = std::fs::remove_file(&part);
                Status::Failed(e)
            }
        };
        self.send(InstallEvent::Update(status));
    }
}

impl Installer for InstallService {
    fn load(&mut self) {
        let _ = self.jobs.send(Job::Load);
    }
    fn set_autostart(&mut self, want: Autostart) {
        let _ = self.jobs.send(Job::Autostart(want));
    }
    fn set_protocol(&mut self, on: bool) {
        let _ = self.jobs.send(Job::Protocol(on));
    }
    fn configure_updates(&mut self, manifest_url: String) {
        let _ = self.jobs.send(Job::Configure(manifest_url));
    }
    fn check_updates(&mut self) {
        let _ = self.jobs.send(Job::Check);
    }
    fn set_auto_check(&mut self, on: bool) {
        let _ = self.jobs.send(Job::SetAuto(on));
    }
    fn auto_check(&self) -> Option<bool> {
        match self.auto.load(Ordering::SeqCst) {
            AUTO_ON => Some(true),
            AUTO_OFF => Some(false),
            _ => None,
        }
    }
    fn announces(&self) -> bool {
        true
    }
    fn download(&mut self, m: Manifest) {
        let _ = self.jobs.send(Job::Download(m));
    }
    fn confirm_started(&mut self) {
        let _ = self.jobs.send(Job::Confirm);
    }
    fn open_external(&mut self, url: &str) {
        super::browser::open(url);
    }
    fn poll(&mut self) -> Vec<InstallEvent> {
        self.events.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::hive::MemoryHive;
    use crate::install::Protocol;

    fn wait(s: &mut InstallService, n: usize) -> Vec<InstallEvent> {
        let start = Instant::now();
        let mut got = Vec::new();
        while got.len() < n && start.elapsed() < Duration::from_secs(10) {
            got.extend(s.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        got
    }

    /// Le fil sur un registre en mémoire : jamais le vrai.
    #[test]
    fn registry_jobs_run_in_the_thread() {
        let exe = PathBuf::from(r"C:\Programmes\Ferrocord\ferrocord.exe");
        let mut s = InstallService::start(Box::new(MemoryHive::default()), Some(exe), None, Arc::new(|| {}));
        s.set_autostart(Autostart::On { minimized: true });
        s.set_protocol(true);
        s.configure_updates(String::new());
        let got = wait(&mut s, 3);
        assert_eq!(
            got,
            vec![
                InstallEvent::Registry { autostart: Autostart::On { minimized: true }, protocol: Protocol::Free },
                InstallEvent::Registry { autostart: Autostart::On { minimized: true }, protocol: Protocol::Ours },
                InstallEvent::Update(Status::NoKey),
            ]
        );
    }

    /// Avec une clé mais ni dépôt ni adresse : aucune source, aucune requête.
    #[test]
    fn no_address_means_no_check() {
        let mut s = InstallService::start(Box::new(MemoryHive::default()), None, Some([1; 32]), Arc::new(|| {}));
        s.configure_updates("  ".into());
        s.check_updates();
        assert_eq!(wait(&mut s, 2), vec![InstallEvent::Update(Status::NoSource), InstallEvent::Update(Status::NoSource)]);
    }
}
