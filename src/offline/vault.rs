//! Fichiers des plugins locaux (lot E1) : journal des messages supprimés et
//! modifiés, index de la recherche locale. Mêmes garanties que l'historique
//! hors ligne, avec les mêmes fonctions : un fichier par compte, chiffré en
//! AES-256-GCM, la clé protégée par DPAPI ; sans DPAPI, rien n'est écrit.
//! Chaque sorte a son dossier, voisin de `historique` dans le cache local
//! (`%LOCALAPPDATA%\ferrocord\journal`, `…\recherche`), et sa propre clé :
//! effacer l'une ne touche pas aux autres.
//!
//! Le fil ne naît qu'à la première demande ([`Vault::files`]) et s'arrête
//! quand l'interface le rend ([`Vault::release`]) : aucun plugin activé,
//! aucun fil.

use super::{crypt, key, keep_only, purge, purge_all, user_file, write_atomic};
use crate::state::journal::Journal;
use crate::state::local_index::LocalIndex;
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::Duration,
};

/// Ce qui est gardé, et dans quel dossier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Journal,
    Index,
    /// Brouillons persistants (lot E3).
    Drafts,
    /// Messages programmés (lot E3).
    Scheduled,
    /// Clé du service de traduction (lot E3) : un seul fichier, compte 0.
    Secrets,
}

impl Kind {
    fn dir(self, root: &Path) -> PathBuf {
        root.join(match self {
            Kind::Journal => "journal",
            Kind::Index => "recherche",
            Kind::Drafts => "brouillons",
            Kind::Scheduled => "programmes",
            Kind::Secrets => "secrets",
        })
    }
}

/// Contenu à enregistrer (encodé dans le fil).
pub enum Payload {
    Journal(Box<Journal>),
    Index(Box<LocalIndex>),
    /// Petit contenu déjà encodé par son plugin (lot E3).
    Raw(Kind, Vec<u8>),
}

impl Payload {
    fn kind(&self) -> Kind {
        match self {
            Payload::Journal(_) => Kind::Journal,
            Payload::Index(_) => Kind::Index,
            Payload::Raw(kind, _) => *kind,
        }
    }

    fn encode(self, user: u64) -> Vec<u8> {
        match self {
            Payload::Journal(j) => j.encode(user),
            Payload::Index(i) => i.encode(user),
            Payload::Raw(_, bytes) => bytes,
        }
    }
}

/// Contenu relu (`None` : aucun, abîmé ou illisible), décodé dans le fil.
#[derive(Debug)]
pub enum Loaded {
    Journal(Option<Box<Journal>>),
    Index(Option<Box<LocalIndex>>),
    /// Contenu déchiffré, décodé par son plugin (lot E3).
    Raw(Kind, Option<Vec<u8>>),
}

#[derive(Debug)]
pub struct VaultEvent {
    pub user: u64,
    pub data: Loaded,
}

/// Dossier parent des dossiers des plugins : celui de `historique`.
pub fn default_root() -> Option<PathBuf> {
    Some(super::default_dir()?.parent()?.to_path_buf())
}

fn read_blob(dir: &Path, user: u64) -> Option<Vec<u8>> {
    let sealed = std::fs::read(user_file(dir, user)).ok()?;
    crypt::open(&key(dir, false)?, user, &sealed)
}

fn write_blob(dir: &Path, user: u64, plain: Vec<u8>) -> Option<()> {
    let key = key(dir, true)?;
    let sealed = crypt::seal(&key, user, plain)?;
    write_atomic(&user_file(dir, user), &sealed).ok()
}

fn load(root: &Path, kind: Kind, user: u64) -> Loaded {
    let bytes = read_blob(&kind.dir(root), user);
    match kind {
        Kind::Journal => Loaded::Journal(bytes.and_then(|b| Journal::decode(&b, user)).map(Box::new)),
        Kind::Index => Loaded::Index(bytes.and_then(|b| LocalIndex::decode(&b, user)).map(Box::new)),
        Kind::Drafts | Kind::Scheduled | Kind::Secrets => Loaded::Raw(kind, bytes),
    }
}

enum Job {
    Load(Kind, u64),
    Save(u64, Payload),
    Purge(Kind, u64),
    KeepOnly(Kind, Vec<u64>),
    PurgeAll(Kind),
    Flush(mpsc::Sender<()>),
}

/// Fil des fichiers des plugins locaux.
pub struct VaultFiles {
    jobs: mpsc::Sender<Job>,
    events: mpsc::Receiver<VaultEvent>,
}

impl VaultFiles {
    fn start(root: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, events) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("ferrocord-journal".into()).spawn(move || {
            // Le fil s'arrête quand l'interface rend `VaultFiles` (file fermée).
            for job in rx {
                match job {
                    Job::Load(kind, user) => {
                        let _ = tx.send(VaultEvent { user, data: load(&root, kind, user) });
                        wake();
                    }
                    Job::Save(user, payload) => {
                        let dir = payload.kind().dir(&root);
                        let _ = write_blob(&dir, user, payload.encode(user));
                    }
                    Job::Purge(kind, user) => purge(&kind.dir(&root), user),
                    Job::KeepOnly(kind, users) => keep_only(&kind.dir(&root), &users),
                    Job::PurgeAll(kind) => purge_all(&kind.dir(&root)),
                    Job::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        // Fil impossible à lancer : les demandes restent sans effet.
        drop(spawned);
        VaultFiles { jobs, events }
    }

    pub fn load(&self, kind: Kind, user: u64) {
        let _ = self.jobs.send(Job::Load(kind, user));
    }

    pub fn save(&self, user: u64, payload: Payload) {
        let _ = self.jobs.send(Job::Save(user, payload));
    }

    pub fn purge(&self, kind: Kind, user: u64) {
        let _ = self.jobs.send(Job::Purge(kind, user));
    }

    pub fn keep_only(&self, kind: Kind, users: Vec<u64>) {
        let _ = self.jobs.send(Job::KeepOnly(kind, users));
    }

    pub fn purge_all(&self, kind: Kind) {
        let _ = self.jobs.send(Job::PurgeAll(kind));
    }

    /// Attend, au plus `wait`, que ce qui a été demandé soit fait.
    pub fn flush(&self, wait: Duration) {
        let (done, rx) = mpsc::channel();
        if self.jobs.send(Job::Flush(done)).is_ok() {
            let _ = rx.recv_timeout(wait);
        }
    }
}

/// Accès aux fichiers des plugins : le dossier et, tant qu'un plugin s'en
/// sert, le fil.
#[derive(Default)]
pub struct Vault {
    /// `None` : rien n'est lu ni écrit (tests, aperçu, système sans DPAPI).
    root: Option<PathBuf>,
    wake: Option<Arc<dyn Fn() + Send + Sync>>,
    files: Option<VaultFiles>,
}

impl Vault {
    pub fn new(root: Option<PathBuf>, wake: Arc<dyn Fn() + Send + Sync>) -> Vault {
        Vault { root: root.filter(|_| super::AVAILABLE), wake: Some(wake), files: None }
    }

    /// Un disque est branché (sinon tout reste en mémoire).
    pub fn attached(&self) -> bool {
        self.root.is_some()
    }

    /// Le fil tourne.
    pub fn running(&self) -> bool {
        self.files.is_some()
    }

    /// Le fil, lancé à la première demande ; `None` sans disque.
    pub fn files(&mut self) -> Option<&VaultFiles> {
        if self.files.is_none() {
            let (root, wake) = (self.root.clone()?, self.wake.clone()?);
            self.files = Some(VaultFiles::start(root, wake));
        }
        self.files.as_ref()
    }

    /// Réponses du fil (rien s'il ne tourne pas).
    pub fn poll(&mut self) -> Vec<VaultEvent> {
        self.files.as_ref().map(|f| f.events.try_iter().collect()).unwrap_or_default()
    }

    /// Plus aucun plugin ne s'en sert : ce qui est demandé se termine, puis
    /// le fil s'arrête.
    pub fn release(&mut self) {
        self.files = None;
    }

    pub fn flush(&self, wait: Duration) {
        if let Some(f) = &self.files {
            f.flush(wait);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::state::journal::tests_support::message;

    fn temp_root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-test-coffre-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn journal_is_encrypted_loaded_back_and_purged_and_the_thread_is_lazy() {
        let root = temp_root("journal");
        let mut vault = Vault::new(Some(root.clone()), Arc::new(|| {}));
        assert!(vault.attached() && !vault.running(), "aucun fil avant la première demande");
        assert!(vault.poll().is_empty());
        let mut j = Journal::default();
        j.record_delete(&message(5, 9, "secret du journal"), None, 1000, true);
        let files = vault.files().unwrap();
        files.save(42, Payload::Journal(Box::new(j.clone())));
        files.load(Kind::Journal, 42);
        files.load(Kind::Journal, 43);
        vault.flush(Duration::from_secs(10));
        let raw = std::fs::read(user_file(&root.join("journal"), 42)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"), "chiffré au repos");
        let got = vault.poll();
        assert!(matches!(&got[0], VaultEvent { user: 42, data: Loaded::Journal(Some(back)) } if back.ghosts(9) == j.ghosts(9)), "{got:?}");
        assert!(matches!(&got[1], VaultEvent { user: 43, data: Loaded::Journal(None) }), "{got:?}");
        vault.files().unwrap().purge_all(Kind::Journal);
        vault.flush(Duration::from_secs(10));
        assert!(!root.join("journal").exists());
        vault.release();
        assert!(!vault.running());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_a_folder_nothing_runs() {
        let mut vault = Vault::default();
        assert!(!vault.attached() && vault.files().is_none() && !vault.running());
    }
}
