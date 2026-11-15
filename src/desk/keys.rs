//! Fichier `keys.toml` (raccourcis clavier réassignés), à côté de
//! `theme.toml`. Lu et écrit dans le fil « ferrocord-raccourcis », bloqué
//! sur sa file : aucun réveil au repos, rien sur le fil d'interface. Le
//! texte lu est analysé par `state::keys`.

use crate::state::keys::MAX_FILE;
use std::{
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

/// Ce que le fil renvoie.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KeysFileEvent {
    /// Texte du fichier (`None` : absent, valeurs par défaut).
    Loaded(Option<String>),
    /// Fichier illisible ou trop gros : valeurs par défaut, fichier gardé.
    ReadFailed(String),
    SaveFailed(String),
}

/// Le fichier des raccourcis, vu de l'interface.
pub trait KeysStore {
    fn load(&mut self);
    fn save(&mut self, text: String);
    fn poll(&mut self) -> Vec<KeysFileEvent>;
}

enum Job {
    Load,
    Save(String),
}

/// Fichier réel, servi par son fil.
pub struct KeysFile {
    tx: mpsc::Sender<Job>,
    rx: mpsc::Receiver<KeysFileEvent>,
}

/// Chemin de `keys.toml`, à côté de `theme.toml`.
pub fn default_path() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("keys.toml"))
}

impl KeysFile {
    /// Lance le fil ; il s'arrête quand `KeysFile` disparaît.
    pub fn start(path: PathBuf, wake: Arc<dyn Fn() + Send + Sync>) -> KeysFile {
        let (tx, jobs) = mpsc::channel::<Job>();
        let (out, rx) = mpsc::channel();
        let spawned = std::thread::Builder::new().name("ferrocord-raccourcis".into()).spawn(move || {
            while let Ok(job) = jobs.recv() {
                let ev = match job {
                    Job::Load => read(&path),
                    Job::Save(text) => match write(&path, &text) {
                        Ok(()) => continue,
                        Err(e) => KeysFileEvent::SaveFailed(e.to_string()),
                    },
                };
                if out.send(ev).is_err() {
                    return;
                }
                wake();
            }
        });
        if let Err(e) = spawned {
            // Sans fil, les raccourcis par défaut valent ; rien n'est écrit.
            #[cfg(debug_assertions)]
            eprintln!("[raccourcis] fil impossible : {e}");
            let _ = e;
        }
        KeysFile { tx, rx }
    }
}

impl KeysStore for KeysFile {
    fn load(&mut self) {
        let _ = self.tx.send(Job::Load);
    }

    fn save(&mut self, text: String) {
        let _ = self.tx.send(Job::Save(text));
    }

    fn poll(&mut self) -> Vec<KeysFileEvent> {
        self.rx.try_iter().collect()
    }
}

fn read(path: &Path) -> KeysFileEvent {
    match std::fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return KeysFileEvent::Loaded(None),
        Err(e) => return KeysFileEvent::ReadFailed(e.to_string()),
        Ok(m) if m.len() > MAX_FILE as u64 => return KeysFileEvent::ReadFailed(format!("{} octets", m.len())),
        Ok(_) => {}
    }
    match std::fs::read_to_string(path) {
        Ok(t) => KeysFileEvent::Loaded(Some(t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => KeysFileEvent::Loaded(None),
        Err(e) => KeysFileEvent::ReadFailed(e.to_string()),
    }
}

/// Écrit dans un fichier voisin puis le renomme : jamais lu à moitié écrit.
fn write(path: &Path, text: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Fichier en mémoire (tests, aperçu) : retient ce qui est écrit.
#[derive(Default, Clone)]
pub struct MemoryKeys {
    pub text: Arc<std::sync::Mutex<Option<String>>>,
    pending: Vec<KeysFileEvent>,
}

impl KeysStore for MemoryKeys {
    fn load(&mut self) {
        let t = self.text.lock().ok().and_then(|t| t.clone());
        self.pending.push(KeysFileEvent::Loaded(t));
    }

    fn save(&mut self, text: String) {
        if let Ok(mut t) = self.text.lock() {
            *t = Some(text);
        }
    }

    fn poll(&mut self) -> Vec<KeysFileEvent> {
        std::mem::take(&mut self.pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn wait(f: &mut KeysFile) -> KeysFileEvent {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(e) = f.poll().into_iter().next() {
                return e;
            }
            assert!(Instant::now() < deadline, "pas de réponse du fil");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn file_is_read_and_written_on_its_thread() {
        let dir = std::env::temp_dir().join(format!("ferrocord-keys-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("keys.toml");
        let mut f = KeysFile::start(path.clone(), Arc::new(|| {}));
        f.load();
        assert_eq!(wait(&mut f), KeysFileEvent::Loaded(None), "absent");
        f.save("[raccourcis]\nmembres = \"Ctrl+M\"\n".into());
        f.load();
        assert_eq!(wait(&mut f), KeysFileEvent::Loaded(Some("[raccourcis]\nmembres = \"Ctrl+M\"\n".into())));
        std::fs::write(&path, vec![b'#'; MAX_FILE + 1]).unwrap_or_default();
        f.load();
        assert!(matches!(wait(&mut f), KeysFileEvent::ReadFailed(_)), "trop gros : refusé sans être lu");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
