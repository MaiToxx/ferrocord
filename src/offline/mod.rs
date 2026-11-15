//! Historique hors ligne sur le disque (lot H) : un fichier chiffré par
//! compte (`<compte>.bin`) et la clé commune (`cle.bin`), dans le dossier
//! du cache local (`%LOCALAPPDATA%\ferrocord\historique` sous Windows).
//!
//! Chiffrement au repos : le contenu est en AES-256-GCM (`crypt`), sa clé
//! aléatoire n'est écrite que protégée par DPAPI (`dpapi`), liée à
//! l'utilisateur Windows. Sans DPAPI (Android, autres systèmes), rien n'est
//! jamais écrit. Aucun jeton n'y entre : l'instantané (`state::offline`)
//! n'a ni jeton ni rien qui en contienne.
//!
//! Tout se fait dans un fil à part ; l'interface ne parle qu'au trait
//! [`OfflineDisk`] et reprend les réponses par [`OfflineDisk::poll`].

pub mod crypt;
#[cfg(windows)]
mod dpapi;
/// Fichiers des plugins locaux : journal, index de recherche (lot E1).
pub mod vault;

use crate::state::offline::{Prepared, Snapshot};
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
    time::{Duration, Instant},
};

const KEY_FILE: &str = "cle.bin";

#[derive(Debug)]
pub enum OfflineEvent {
    /// Instantané relu pour ce compte (`None` : aucun, abîmé ou illisible),
    /// et le temps de la lecture (disque, déchiffrement, décodage).
    /// Déjà converti en modèles dans ce fil (lot C9).
    Loaded { user: u64, snapshot: Option<Box<Prepared>>, took: Duration },
}

pub trait OfflineDisk {
    fn load(&mut self, user: u64);
    fn save(&mut self, user: u64, snapshot: Snapshot);
    /// Efface l'historique de ce compte (déconnexion, retrait).
    fn purge(&mut self, user: u64);
    /// Efface l'historique des comptes absents de `users`.
    fn keep_only(&mut self, users: Vec<u64>);
    /// Efface tout, clé comprise (option désactivée, « Effacer »).
    fn purge_all(&mut self);
    /// Attend, au plus `wait`, que ce qui a été demandé soit fait (fermeture).
    fn flush(&mut self, wait: Duration);
    fn poll(&mut self) -> Vec<OfflineEvent>;
}

/// Sans disque : tests, aperçu, avant `App::attach_offline`.
#[derive(Default)]
pub struct Quiet;

impl OfflineDisk for Quiet {
    fn load(&mut self, _: u64) {}
    fn save(&mut self, _: u64, _: Snapshot) {}
    fn purge(&mut self, _: u64) {}
    fn keep_only(&mut self, _: Vec<u64>) {}
    fn purge_all(&mut self) {}
    fn flush(&mut self, _: Duration) {}
    fn poll(&mut self) -> Vec<OfflineEvent> {
        Vec::new()
    }
}

/// Dossier de l'historique.
pub fn default_dir() -> Option<PathBuf> {
    Some(crate::platform::cache_dir()?.join("ferrocord").join("historique"))
}

#[cfg(windows)]
fn protect(data: &[u8]) -> Option<Vec<u8>> {
    dpapi::protect(data)
}

#[cfg(windows)]
fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    dpapi::unprotect(data)
}

// Pas de protection de la clé liée au compte du système : rien n'est écrit.
#[cfg(not(windows))]
fn protect(_: &[u8]) -> Option<Vec<u8>> {
    None
}

#[cfg(not(windows))]
fn unprotect(_: &[u8]) -> Option<Vec<u8>> {
    None
}

/// L'historique peut être chiffré sur ce système.
pub const AVAILABLE: bool = cfg!(windows);

fn user_file(dir: &Path, user: u64) -> PathBuf {
    dir.join(format!("{user}.bin"))
}

/// Comptes dont un fichier existe.
fn users_on_disk(dir: &Path) -> Vec<u64> {
    let Ok(list) = std::fs::read_dir(dir) else { return Vec::new() };
    list.filter_map(|e| e.ok()?.file_name().to_str()?.strip_suffix(".bin")?.parse().ok()).collect()
}

/// Écrit dans un fichier voisin puis le renomme : jamais lu à moitié écrit.
fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Relit la clé ; absente ou illisible et `create` : une nouvelle, et les
/// fichiers chiffrés par l'ancienne (illisibles) sont effacés.
fn key(dir: &Path, create: bool) -> Option<crypt::Key> {
    let path = dir.join(KEY_FILE);
    if let Some(bytes) = std::fs::read(&path).ok().and_then(|b| unprotect(&b))
        && let Ok(raw) = <[u8; crypt::KEY_LEN]>::try_from(bytes.as_slice())
        && let Some(k) = crypt::Key::new(&raw)
    {
        return Some(k);
    }
    if !create {
        return None;
    }
    let (raw, k) = crypt::Key::random()?;
    let sealed = protect(&raw)?;
    for u in users_on_disk(dir) {
        let _ = std::fs::remove_file(user_file(dir, u));
    }
    write_atomic(&path, &sealed).ok()?;
    Some(k)
}

/// Lit l'instantané d'un compte.
pub fn read(dir: &Path, user: u64) -> Option<Snapshot> {
    let sealed = std::fs::read(user_file(dir, user)).ok()?;
    let plain = crypt::open(&key(dir, false)?, user, &sealed)?;
    Snapshot::decode(&plain).filter(|s| s.me.id == user)
}

/// Enregistre l'instantané d'un compte (chiffré).
pub fn write(dir: &Path, user: u64, snapshot: Snapshot) -> Option<()> {
    let key = key(dir, true)?;
    let sealed = crypt::seal(&key, user, snapshot.encode())?;
    write_atomic(&user_file(dir, user), &sealed).ok()
}

/// Efface l'historique d'un compte ; plus aucun : la clé aussi.
pub fn purge(dir: &Path, user: u64) {
    let _ = std::fs::remove_file(user_file(dir, user));
    if users_on_disk(dir).is_empty() {
        purge_all(dir);
    }
}

pub fn keep_only(dir: &Path, users: &[u64]) {
    for u in users_on_disk(dir) {
        if !users.contains(&u) {
            purge(dir, u);
        }
    }
    if users_on_disk(dir).is_empty() && dir.join(KEY_FILE).exists() {
        purge_all(dir);
    }
}

pub fn purge_all(dir: &Path) {
    let _ = std::fs::remove_dir_all(dir);
}

enum Job {
    Load(u64),
    Save(u64, Box<Snapshot>),
    Purge(u64),
    KeepOnly(Vec<u64>),
    PurgeAll,
    Flush(mpsc::Sender<()>),
}

/// Fil des fichiers de l'historique.
pub struct OfflineFiles {
    jobs: mpsc::Sender<Job>,
    events: mpsc::Receiver<OfflineEvent>,
}

impl OfflineFiles {
    /// `dir` : dossier de l'historique (`None` : rien n'est lu ni écrit).
    pub fn start(dir: Option<PathBuf>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, events) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("ferrocord-historique".into()).spawn(move || {
            for job in rx {
                let Some(dir) = &dir else {
                    if let Job::Load(user) = job {
                        let _ = tx.send(OfflineEvent::Loaded { user, snapshot: None, took: Duration::ZERO });
                        wake();
                    }
                    continue;
                };
                match job {
                    Job::Load(user) => {
                        let at = Instant::now();
                        let snapshot = read(dir, user).map(|s| Box::new(s.prepare()));
                        let _ = tx.send(OfflineEvent::Loaded { user, snapshot, took: at.elapsed() });
                        wake();
                    }
                    Job::Save(user, s) => {
                        let _ = write(dir, user, *s);
                    }
                    Job::Purge(user) => purge(dir, user),
                    Job::KeepOnly(users) => keep_only(dir, &users),
                    Job::PurgeAll => purge_all(dir),
                    Job::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        // Fil impossible à lancer : les demandes restent sans effet.
        drop(spawned);
        OfflineFiles { jobs, events }
    }
}

impl OfflineDisk for OfflineFiles {
    fn load(&mut self, user: u64) {
        let _ = self.jobs.send(Job::Load(user));
    }
    fn save(&mut self, user: u64, snapshot: Snapshot) {
        let _ = self.jobs.send(Job::Save(user, Box::new(snapshot)));
    }
    fn purge(&mut self, user: u64) {
        let _ = self.jobs.send(Job::Purge(user));
    }
    fn keep_only(&mut self, users: Vec<u64>) {
        let _ = self.jobs.send(Job::KeepOnly(users));
    }
    fn purge_all(&mut self) {
        let _ = self.jobs.send(Job::PurgeAll);
    }
    fn flush(&mut self, wait: Duration) {
        let (done, rx) = mpsc::channel();
        if self.jobs.send(Job::Flush(done)).is_ok() {
            let _ = rx.recv_timeout(wait);
        }
    }
    fn poll(&mut self) -> Vec<OfflineEvent> {
        self.events.try_iter().collect()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::state::offline::{SChan, SMsg, SUser, VERSION};

    fn snap(user: u64, text: &str) -> Snapshot {
        Snapshot {
            version: VERSION,
            me: SUser { id: user, name: "Moi".into(), ..Default::default() },
            chans: vec![SChan { id: 9, more: false, msgs: vec![SMsg { id: 1, content: text.into(), author: "Alice".into(), ..Default::default() }] }],
            ..Default::default()
        }
    }

    fn temp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-test-historique-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn files_are_encrypted_per_account_and_purged() {
        let dir = temp_dir("disque");
        assert_eq!(read(&dir, 5), None, "rien encore");
        write(&dir, 5, snap(5, "secret du compte cinq")).unwrap();
        write(&dir, 6, snap(6, "autre")).unwrap();
        let raw = std::fs::read(user_file(&dir, 5)).unwrap();
        assert!(!raw.windows(6).any(|w| w == b"secret"), "chiffré au repos");
        let key_raw = std::fs::read(dir.join(KEY_FILE)).unwrap();
        assert!(key_raw.len() > crypt::KEY_LEN, "clé protégée par DPAPI, pas en clair");
        assert_eq!(read(&dir, 5).map(|s| s.chans[0].msgs[0].content.clone()).as_deref(), Some("secret du compte cinq"));
        // Le fichier d'un compte mis à la place d'un autre ne se lit pas.
        std::fs::copy(user_file(&dir, 6), user_file(&dir, 7)).unwrap();
        assert_eq!(read(&dir, 7), None);
        keep_only(&dir, &[5, 6]);
        assert!(!user_file(&dir, 7).exists());
        purge(&dir, 5);
        assert!(!user_file(&dir, 5).exists() && user_file(&dir, 6).exists());
        purge(&dir, 6);
        assert!(!dir.join(KEY_FILE).exists(), "plus aucun compte : la clé part aussi");
        // Clé perdue : les anciens fichiers, illisibles, sont effacés à la prochaine écriture.
        write(&dir, 5, snap(5, "a")).unwrap();
        std::fs::remove_file(dir.join(KEY_FILE)).unwrap();
        assert_eq!(read(&dir, 5), None);
        write(&dir, 6, snap(6, "b")).unwrap();
        assert!(!user_file(&dir, 5).exists());
        purge_all(&dir);
        assert!(!dir.exists());
    }

    /// Mesure (lot H) : instantané plein (20 salons de 50 messages de 300
    /// caractères), écrit puis relu par le vrai chemin (DPAPI, AES-GCM, JSON).
    #[test]
    fn full_snapshot_timing() {
        let dir = temp_dir("mesure");
        let msgs: Vec<SMsg> = (0..50).map(|i| SMsg { id: 1000 + i, content: "é".repeat(150) + &"x".repeat(150), author: "Alice".into(), mentions: vec![(5, "Moi".into())], ..Default::default() }).collect();
        let mut s = snap(5, "");
        s.chans = (0..20).map(|c| SChan { id: 100 + c, more: true, msgs: msgs.clone() }).collect();
        let at = Instant::now();
        write(&dir, 5, s.clone()).unwrap();
        let wrote = at.elapsed();
        let size = std::fs::metadata(user_file(&dir, 5)).unwrap().len();
        let at = Instant::now();
        let back = read(&dir, 5).unwrap();
        let read_ms = at.elapsed();
        assert_eq!(back, s);
        eprintln!("historique : {size} octets, écrit en {wrote:?}, relu en {read_ms:?}");
        assert!(read_ms < Duration::from_secs(2), "{read_ms:?}");
        purge_all(&dir);
    }

    #[test]
    fn worker_loads_saves_and_flushes() {
        let dir = temp_dir("fil");
        let mut files = OfflineFiles::start(Some(dir.clone()), Arc::new(|| {}));
        files.save(8, snap(8, "fil"));
        files.load(8);
        files.flush(Duration::from_secs(10));
        let got = files.poll();
        assert!(matches!(got.as_slice(), [OfflineEvent::Loaded { user: 8, snapshot: Some(s), .. }] if s.me.id == 8), "{got:?}");
        files.purge_all();
        files.flush(Duration::from_secs(10));
        assert!(!dir.exists());
    }
}
