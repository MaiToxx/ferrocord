//! Cache disque des images téléchargées (fichiers d'origine, compressés).
//!
//! Politique d'éviction : un fichier plus vieux que `max_age` (date de
//! dernière utilisation) est supprimé ; au-delà de `max_bytes`, les fichiers
//! utilisés le moins récemment sont supprimés en premier, jusqu'à redescendre
//! à 90 % du plafond (un seul tri pour de nombreuses écritures). Toutes les
//! méthodes font des E/S bloquantes : elles ne sont appelées que depuis les
//! fils de travail (`spawn_blocking`), jamais depuis l'interface.
//!
//! L'ouverture ne parcourt pas le dossier : [`Disk::scan`] le fait ensuite,
//! pendant que les images sont déjà lues et écrites (une lecture va
//! directement au fichier ; l'index ne sert qu'aux plafonds).
//!
//! Chaque fichier porte un nom dérivé d'une empreinte de l'adresse (jamais
//! l'adresse elle-même) et commence par l'adresse complète : une collision
//! d'empreintes ou un fichier abîmé se lit comme une absence.

use std::{
    collections::HashMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Mutex, MutexGuard, PoisonError,
    },
    time::{Duration, SystemTime},
};

/// Début de chaque fichier du cache.
const MAGIC: &[u8; 4] = b"FCI1";
const EXT: &str = "img";
const TMP_EXT: &str = "tmp";
/// Nouveaux essais d'un renommage refusé parce qu'un autre programme
/// (antivirus, indexeur) tient le fichier ouvert.
const RENAME_TRIES: u32 = 5;

#[derive(Clone, Debug)]
pub struct DiskConfig {
    pub dir: PathBuf,
    pub max_bytes: u64,
    pub max_age: Duration,
}

impl DiskConfig {
    /// Plafonds par défaut : 256 Mo, 30 jours sans utilisation.
    pub fn new(dir: PathBuf) -> Self {
        DiskConfig { dir, max_bytes: 256 << 20, max_age: Duration::from_secs(30 * 24 * 3600) }
    }
}

struct Entry {
    used: SystemTime,
    size: u64,
}

pub struct Disk {
    cfg: DiskConfig,
    index: Mutex<Index>,
}

#[derive(Default)]
struct Index {
    files: HashMap<String, Entry>,
    total: u64,
}

/// Empreinte de 128 bits (deux FNV-1a 64 bits) de l'adresse, en hexadécimal.
/// Stable d'une version à l'autre (contrairement au hachage de `std`).
pub fn file_name(url: &str) -> String {
    let fnv = |seed: u64| url.bytes().fold(seed, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01B3));
    format!("{:016x}{:016x}.{EXT}", fnv(0xCBF2_9CE4_8422_2325), fnv(0x6C62_272E_07BB_0142))
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Renomme `from` en `to`. Refusé parce que l'un des deux est ouvert
/// ailleurs (Windows : partage ou accès refusé), réessaie au plus
/// [`RENAME_TRIES`] fois en appelant `wait(essai)` entre deux.
fn rename_retrying(from: &Path, to: &Path, mut wait: impl FnMut(u32)) -> std::io::Result<()> {
    let mut attempt = 0;
    loop {
        match fs::rename(from, to) {
            Err(e) if attempt < RENAME_TRIES && held_elsewhere(&e) => {
                attempt += 1;
                wait(attempt);
            }
            other => return other,
        }
    }
}

/// Fichier tenu ouvert par un autre programme (erreurs Windows 5, 32 et 33).
fn held_elsewhere(e: &std::io::Error) -> bool {
    cfg!(windows) && matches!(e.raw_os_error(), Some(5 | 32 | 33))
}

impl Disk {
    /// Ouvre (crée au besoin) le dossier, sans le parcourir : appeler
    /// [`Disk::scan`] ensuite, hors du chemin des premières images.
    pub fn open(cfg: DiskConfig) -> std::io::Result<Disk> {
        fs::create_dir_all(&cfg.dir)?;
        Ok(Disk { cfg, index: Mutex::new(Index::default()) })
    }

    /// Parcourt le dossier : supprime les fichiers temporaires abandonnés et
    /// les fichiers périmés, ajoute les autres à l'index (ce qui a été écrit
    /// entre-temps est gardé tel quel), puis ramène le total sous le plafond.
    pub fn scan(&self) {
        let Ok(dir) = fs::read_dir(&self.cfg.dir) else { return };
        let now = SystemTime::now();
        // Les fichiers temporaires de ce processus sont peut-être en cours d'écriture.
        let mine = format!(".{}.", std::process::id());
        let mut found = Vec::new();
        for entry in dir.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()).map(str::to_string) else { continue };
            let Ok(meta) = entry.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            let ext = path.extension().and_then(|e| e.to_str());
            let used = meta.modified().unwrap_or(now);
            let expired = now.duration_since(used).is_ok_and(|age| age > self.cfg.max_age);
            if (ext == Some(TMP_EXT) && !name.contains(&mine)) || (ext == Some(EXT) && expired) {
                let _ = fs::remove_file(&path);
                continue;
            }
            if ext == Some(EXT) {
                found.push((name, Entry { used, size: meta.len() }));
            }
        }
        {
            let mut idx = self.index();
            for (name, e) in found {
                if !idx.files.contains_key(&name) {
                    idx.total += e.size;
                    idx.files.insert(name, e);
                }
            }
        }
        self.shrink();
    }

    fn index(&self) -> MutexGuard<'_, Index> {
        // Rien ne panique sous ce verrou ; et même alors, l'index resterait utilisable.
        self.index.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.cfg.dir.join(name)
    }

    /// Octets occupés par le cache (fichiers connus de l'index).
    pub fn total(&self) -> u64 {
        self.index().total
    }

    /// Contenu enregistré pour `url`, s'il est intact et pas périmé.
    pub fn get(&self, url: &str) -> Option<Vec<u8>> {
        let name = file_name(url);
        let path = self.path(&name);
        let data = fs::read(&path).ok()?;
        let now = SystemTime::now();
        let fresh = fs::metadata(&path).and_then(|m| m.modified()).is_ok_and(|t| now.duration_since(t).map_or(true, |age| age <= self.cfg.max_age));
        let body = fresh.then(|| payload(&data, url)).flatten();
        let Some(body) = body else {
            self.remove(&name);
            return None;
        };
        // La date du fichier sert de date de dernière utilisation.
        if let Ok(f) = fs::File::options().write(true).open(&path) {
            let _ = f.set_modified(now);
        }
        if let Some(e) = self.index().files.get_mut(&name) {
            e.used = now;
        }
        Some(body.to_vec())
    }

    /// Enregistre `data` pour `url` (écriture dans un fichier temporaire
    /// puis renommage : un fichier n'est jamais lu à moitié écrit).
    pub fn put(&self, url: &str, data: &[u8]) {
        let size = (MAGIC.len() + 4 + url.len() + data.len()) as u64;
        if size > self.cfg.max_bytes || u32::try_from(url.len()).is_err() {
            return;
        }
        let name = file_name(url);
        let tmp = self.path(&format!("{name}.{}.{}.{TMP_EXT}", std::process::id(), TMP_COUNTER.fetch_add(1, Ordering::Relaxed)));
        let written = (|| -> std::io::Result<()> {
            let mut f = fs::File::create(&tmp)?;
            f.write_all(MAGIC)?;
            f.write_all(&(url.len() as u32).to_le_bytes())?;
            f.write_all(url.as_bytes())?;
            f.write_all(data)?;
            f.sync_all()?;
            drop(f);
            rename_retrying(&tmp, &self.path(&name), |n| std::thread::sleep(Duration::from_millis(10 * u64::from(n))))
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
            return;
        }
        {
            let mut idx = self.index();
            if let Some(old) = idx.files.insert(name, Entry { used: SystemTime::now(), size }) {
                idx.total -= old.size.min(idx.total);
            }
            idx.total += size;
        }
        self.shrink();
    }

    /// Oublie le fichier de `url` (image abîmée).
    pub fn forget(&self, url: &str) {
        self.remove(&file_name(url));
    }

    fn remove(&self, name: &str) {
        let _ = fs::remove_file(self.path(name));
        let mut idx = self.index();
        if let Some(old) = idx.files.remove(name) {
            idx.total -= old.size.min(idx.total);
        }
    }

    /// Au-delà du plafond, supprime les fichiers utilisés le moins récemment
    /// jusqu'à redescendre à 90 % du plafond.
    fn shrink(&self) {
        let mut idx = self.index();
        if idx.total <= self.cfg.max_bytes {
            return;
        }
        let target = self.cfg.max_bytes / 10 * 9;
        let mut order: Vec<(SystemTime, String)> = idx.files.iter().map(|(n, e)| (e.used, n.clone())).collect();
        order.sort();
        for (_, name) in order {
            if idx.total <= target {
                break;
            }
            let _ = fs::remove_file(self.cfg.dir.join(&name));
            if let Some(old) = idx.files.remove(&name) {
                idx.total -= old.size.min(idx.total);
            }
        }
    }

    pub fn dir(&self) -> &Path {
        &self.cfg.dir
    }
}

/// Contenu d'un fichier du cache s'il est bien celui de `url`.
fn payload<'a>(data: &'a [u8], url: &str) -> Option<&'a [u8]> {
    let rest = data.strip_prefix(MAGIC)?;
    let len = u32::from_le_bytes(rest.get(..4)?.try_into().ok()?) as usize;
    let stored = rest.get(4..4 + len)?;
    (stored == url.as_bytes()).then(|| rest.get(4 + len..)).flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-disk-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn file_names_are_hashes_not_urls() {
        let a = file_name("https://cdn.discordapp.com/avatars/1/x.webp?size=64");
        assert_eq!(a.len(), 32 + 1 + EXT.len());
        assert!(a.trim_end_matches(".img").bytes().all(|b| b.is_ascii_hexdigit()));
        assert_ne!(a, file_name("https://cdn.discordapp.com/avatars/1/x.webp?size=128"));
        assert_eq!(a, file_name("https://cdn.discordapp.com/avatars/1/x.webp?size=64"), "stable");
        // Valeur figée : un changement viderait le cache de tous les utilisateurs.
        assert_eq!(file_name(""), "cbf29ce4842223256c62272e07bb0142.img");
    }

    #[test]
    fn put_get_and_reopen() {
        let d = dir("roundtrip");
        let disk = Disk::open(DiskConfig::new(d.clone())).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(disk.get("u1"), None);
        disk.put("u1", b"abc");
        assert_eq!(disk.get("u1").as_deref(), Some(&b"abc"[..]));
        drop(disk);
        // Réouverture : le dossier n'est pas encore parcouru, la lecture fonctionne déjà.
        let disk = Disk::open(DiskConfig::new(d.clone())).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(disk.total(), 0);
        assert_eq!(disk.get("u1").as_deref(), Some(&b"abc"[..]));
        // Écrit pendant le parcours : gardé, et compté une seule fois.
        disk.put("u2", b"de");
        let after_put = disk.total();
        disk.scan();
        assert!(disk.total() > after_put);
        disk.scan();
        let twice = disk.total();
        disk.scan();
        assert_eq!(disk.total(), twice, "parcours répétés : rien de compté deux fois");
        assert_eq!(disk.get("u2").as_deref(), Some(&b"de"[..]));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn corrupt_or_foreign_files_are_misses() {
        let d = dir("corrupt");
        let disk = Disk::open(DiskConfig::new(d.clone())).unwrap_or_else(|e| panic!("{e}"));
        disk.put("u1", b"abc");
        let path = d.join(file_name("u1"));
        fs::write(&path, b"FCI").unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(disk.get("u1"), None);
        assert!(!path.exists(), "fichier abîmé supprimé");
        // Collision simulée : le fichier d'une autre adresse sous ce nom.
        disk.put("u2", b"xyz");
        fs::copy(d.join(file_name("u2")), &path).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(disk.get("u1"), None);
        // Longueur d'adresse absurde.
        let mut bad = MAGIC.to_vec();
        bad.extend_from_slice(&u32::MAX.to_le_bytes());
        fs::write(&path, bad).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(disk.get("u1"), None);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn cap_evicts_least_recently_used() {
        let d = dir("cap");
        let cfg = DiskConfig { dir: d.clone(), max_bytes: 10_000, max_age: Duration::from_secs(3600) };
        let disk = Disk::open(cfg.clone()).unwrap_or_else(|e| panic!("{e}"));
        let blob = vec![7u8; 1000];
        let base = SystemTime::now() - Duration::from_secs(600);
        for i in 0..30 {
            disk.put(&format!("u{i}"), &blob);
            // Dates de fichiers distinctes même sur un système de fichiers peu précis.
            if let Ok(f) = fs::File::options().write(true).open(d.join(file_name(&format!("u{i}")))) {
                let _ = f.set_modified(base + Duration::from_secs(i));
            }
            if let Ok(mut idx) = disk.index.lock()
                && let Some(e) = idx.files.get_mut(&file_name(&format!("u{i}")))
            {
                e.used = base + Duration::from_secs(i);
            }
            assert!(disk.total() <= cfg.max_bytes);
        }
        let on_disk: u64 = fs::read_dir(&d).into_iter().flatten().flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum();
        assert!(on_disk <= cfg.max_bytes, "{on_disk}");
        assert_eq!(disk.get("u0"), None, "la plus ancienne est partie");
        assert!(disk.get("u29").is_some(), "la plus récente reste");
        // Un fichier plus gros que le plafond n'est pas enregistré.
        disk.put("énorme", &vec![0u8; 20_000]);
        assert_eq!(disk.get("énorme"), None);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn open_removes_temporary_and_expired_files() {
        let d = dir("expire");
        fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
        let cfg = DiskConfig { dir: d.clone(), max_bytes: 1 << 20, max_age: Duration::from_secs(3600) };
        {
            let disk = Disk::open(cfg.clone()).unwrap_or_else(|e| panic!("{e}"));
            disk.put("vieux", b"1");
            disk.put("neuf", b"2");
        }
        let old = d.join(file_name("vieux"));
        if let Ok(f) = fs::File::options().write(true).open(&old) {
            f.set_modified(SystemTime::now() - Duration::from_secs(7200)).unwrap_or_else(|e| panic!("{e}"));
        }
        fs::write(d.join("abandon.1.2.tmp"), b"x").unwrap_or_else(|e| panic!("{e}"));
        let disk = Disk::open(cfg).unwrap_or_else(|e| panic!("{e}"));
        assert!(old.exists(), "rien n'est parcouru à l'ouverture");
        disk.scan();
        assert!(!old.exists());
        assert!(!d.join("abandon.1.2.tmp").exists());
        assert_eq!(disk.get("neuf").as_deref(), Some(&b"2"[..]));
        let _ = fs::remove_dir_all(&d);
    }

    /// Fichier temporaire tenu ouvert sans partage (comme le ferait un
    /// antivirus) : le renommage est réessayé, puis réussit dès qu'il est relâché.
    #[cfg(windows)]
    #[test]
    fn rename_waits_for_a_file_held_elsewhere() {
        use std::os::windows::fs::OpenOptionsExt;
        let d = dir("rename");
        fs::create_dir_all(&d).unwrap_or_else(|e| panic!("{e}"));
        let (from, to) = (d.join("a.tmp"), d.join("a.img"));
        fs::write(&from, b"x").unwrap_or_else(|e| panic!("{e}"));
        let hold = || fs::File::options().read(true).share_mode(0).open(&from).unwrap_or_else(|e| panic!("{e}"));
        // Relâché au deuxième essai : réussi.
        let mut held = Some(hold());
        let mut waits = Vec::new();
        rename_retrying(&from, &to, |n| {
            waits.push(n);
            if n == 2 {
                held = None;
            }
        })
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(waits, vec![1, 2]);
        assert!(to.exists() && !from.exists());
        // Jamais relâché : abandon après un nombre borné d'essais.
        fs::write(&from, b"y").unwrap_or_else(|e| panic!("{e}"));
        let _held = hold();
        let mut n = 0;
        assert!(rename_retrying(&from, &to, |_| n += 1).is_err());
        assert_eq!(n, RENAME_TRIES);
        drop(_held);
        let _ = fs::remove_dir_all(&d);
    }

    /// Une erreur qui n'est pas un partage refusé n'est pas réessayée.
    #[test]
    fn rename_does_not_retry_other_errors() {
        let d = dir("rename-absent");
        let mut n = 0;
        assert!(rename_retrying(&d.join("absent"), &d.join("b"), |_| n += 1).is_err());
        assert_eq!(n, 0);
    }
}
