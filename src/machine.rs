//! Réglages de cet ordinateur (`local.toml`), journal local de débogage
//! (`ferrocord.log`) et réglage « Effets d'animation » de Windows. Tout
//! accès au disque se fait dans un fil à part ; l'interface ne parle qu'au
//! trait [`Machine`] et reprend les réponses par [`Machine::poll`].

use crate::state::local::LocalPrefs;
use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::{mpsc, Arc},
};

/// Journal plus grand que ceci : l'ancien est gardé à côté et un nouveau commence.
pub const LOG_MAX_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
// `Loaded` n'arrive qu'une fois, au lancement : sa taille ne coûte rien.
#[allow(clippy::large_enum_variant)]
pub enum MachineEvent {
    /// `local.toml` lu (absent ou abîmé : valeurs par défaut), et ce que dit
    /// Windows des animations et du nombre de lignes par cran de molette
    /// (`None` : inconnu).
    Loaded { prefs: LocalPrefs, windows_reduced: Option<bool>, wheel_lines: Option<u32> },
    /// Écriture de `local.toml` ratée (raison).
    SaveFailed(String),
}

pub trait Machine {
    fn load(&mut self);
    fn save(&mut self, prefs: LocalPrefs);
    /// Ajoute une ligne (déjà horodatée) au journal local.
    fn log(&mut self, line: String);
    /// Dossier des réglages et du journal (affiché, ouvert sur demande).
    fn folder(&self) -> Option<PathBuf>;
    fn poll(&mut self) -> Vec<MachineEvent>;
}

/// Sans disque : tests, aperçu, avant `App::attach_machine`.
#[derive(Default)]
pub struct Quiet;

impl Machine for Quiet {
    fn load(&mut self) {}
    fn save(&mut self, _: LocalPrefs) {}
    fn log(&mut self, _: String) {}
    fn folder(&self) -> Option<PathBuf> {
        None
    }
    fn poll(&mut self) -> Vec<MachineEvent> {
        Vec::new()
    }
}

/// Lit `local.toml` (absent ou syntaxe fausse : valeurs par défaut).
pub fn read_prefs(path: &Path) -> LocalPrefs {
    std::fs::read_to_string(path).ok().and_then(|t| LocalPrefs::parse(&t).ok()).unwrap_or_default()
}

/// Écrit dans un fichier voisin puis le renomme : jamais lu à moitié écrit.
pub fn write_prefs(path: &Path, prefs: &LocalPrefs) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("toml.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(prefs.to_toml().as_bytes())?;
        f.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Ajoute `line` au journal ; au-delà de [`LOG_MAX_BYTES`], le journal
/// devient `ferrocord.old.log` (l'ancien est remplacé).
pub fn append_log(path: &Path, line: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|m| m.len() >= LOG_MAX_BYTES) {
        std::fs::rename(path, path.with_extension("old.log"))?;
    }
    std::fs::OpenOptions::new().create(true).append(true).open(path)?.write_all(line.as_bytes())
}

/// « Effets d'animation » coupés dans Windows ? Lu dans le registre par
/// `reg.exe` (sans `unsafe` ni dépendance) : `MinAnimate` vaut 0 quand le
/// réglage est désactivé.
#[cfg(windows)]
pub fn windows_reduced_motion() -> Option<bool> {
    use std::os::windows::process::CommandExt;
    // Pas de console qui clignote.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\Desktop\WindowMetrics", "/v", "MinAnimate"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_min_animate(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(windows))]
pub fn windows_reduced_motion() -> Option<bool> {
    None
}

/// Valeur de `MinAnimate` dans la sortie de `reg query`.
pub fn parse_min_animate(out: &str) -> Option<bool> {
    let line = out.lines().find(|l| l.trim_start().starts_with("MinAnimate"))?;
    let value = line.split_whitespace().last()?;
    Some(value == "0")
}

/// Lignes par cran de molette réglées dans Windows (Paramètres › Souris).
/// winit ne le donne pas : lu dans le registre comme `MinAnimate`. `None` :
/// inconnu, ou « un écran à la fois » (la distance par défaut est gardée).
#[cfg(windows)]
pub fn windows_wheel_lines() -> Option<u32> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("reg")
        .args(["query", r"HKCU\Control Panel\Desktop", "/v", "WheelScrollLines"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_wheel_lines(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(not(windows))]
pub fn windows_wheel_lines() -> Option<u32> {
    None
}

/// Valeur de `WheelScrollLines` dans la sortie de `reg query` (`-1` ou
/// `0xffffffff` : un écran à la fois ; 0 : molette coupée).
pub fn parse_wheel_lines(out: &str) -> Option<u32> {
    let line = out.lines().find(|l| l.trim_start().starts_with("WheelScrollLines"))?;
    let n: u32 = line.split_whitespace().last()?.parse().ok()?;
    (1..=100).contains(&n).then_some(n)
}

enum Job {
    Load,
    Save(Box<LocalPrefs>),
    Log(String),
}

/// Fil des fichiers de cet ordinateur.
pub struct MachineFiles {
    jobs: mpsc::Sender<Job>,
    events: mpsc::Receiver<MachineEvent>,
    dir: Option<PathBuf>,
}

impl MachineFiles {
    /// `dir` : dossier des réglages (`None` : rien n'est lu ni écrit).
    pub fn start(dir: Option<PathBuf>, wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (jobs, rx) = mpsc::channel::<Job>();
        let (tx, events) = mpsc::channel();
        let folder = dir.clone();
        let spawned = std::thread::Builder::new().name("ferrocord-local".into()).spawn(move || {
            for job in rx {
                let Some(dir) = &folder else { continue };
                let ev = match job {
                    Job::Load => Some(MachineEvent::Loaded { prefs: read_prefs(&dir.join("local.toml")), windows_reduced: windows_reduced_motion(), wheel_lines: windows_wheel_lines() }),
                    Job::Save(p) => write_prefs(&dir.join("local.toml"), &p).err().map(|e| MachineEvent::SaveFailed(e.to_string())),
                    // Journal impossible à écrire : rien d'autre à faire (il sert au débogage).
                    Job::Log(line) => {
                        let _ = append_log(&dir.join("ferrocord.log"), &line);
                        None
                    }
                };
                if let Some(ev) = ev {
                    let _ = tx.send(ev);
                    wake();
                }
            }
        });
        // Fil impossible à créer : réglages par défaut, rien d'enregistré.
        drop(spawned);
        MachineFiles { jobs, events, dir }
    }
}

/// Dossier des réglages de Ferrocord (celui de `theme.toml`).
pub fn default_dir() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord"))
}

impl Machine for MachineFiles {
    fn load(&mut self) {
        let _ = self.jobs.send(Job::Load);
    }
    fn save(&mut self, prefs: LocalPrefs) {
        let _ = self.jobs.send(Job::Save(Box::new(prefs)));
    }
    fn log(&mut self, line: String) {
        let _ = self.jobs.send(Job::Log(line));
    }
    fn folder(&self) -> Option<PathBuf> {
        self.dir.clone()
    }
    fn poll(&mut self) -> Vec<MachineEvent> {
        self.events.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-machine-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn prefs_written_then_read_back_through_the_thread() {
        let d = dir("prefs");
        let mut m = MachineFiles::start(Some(d.clone()), Arc::new(|| {}));
        let p = LocalPrefs { saturation: 30, streamer: crate::state::local::Streamer { on: true, ..Default::default() }, ..Default::default() };
        m.save(p.clone());
        m.load();
        let start = std::time::Instant::now();
        let mut got = Vec::new();
        while got.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
            got = m.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let [MachineEvent::Loaded { prefs, .. }] = got.as_slice() else { panic!("{got:?}") };
        assert_eq!(*prefs, p);
        std::fs::write(d.join("local.toml"), "saturation = ").unwrap();
        assert_eq!(read_prefs(&d.join("local.toml")), LocalPrefs::default());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn log_is_appended_and_rotated() {
        let d = dir("log");
        let path = d.join("ferrocord.log");
        append_log(&path, "un\n").unwrap();
        append_log(&path, "deux\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "un\ndeux\n");
        std::fs::write(&path, vec![b'x'; LOG_MAX_BYTES as usize]).unwrap();
        append_log(&path, "trois\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "trois\n");
        assert_eq!(std::fs::metadata(d.join("ferrocord.old.log")).unwrap().len(), LOG_MAX_BYTES);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn min_animate_from_reg_output() {
        let on = "\r\nHKEY_CURRENT_USER\\Control Panel\\Desktop\\WindowMetrics\r\n    MinAnimate    REG_SZ    1\r\n";
        assert_eq!(parse_min_animate(on), Some(false));
        assert_eq!(parse_min_animate(&on.replace("REG_SZ    1", "REG_SZ    0")), Some(true));
        assert_eq!(parse_min_animate("Erreur"), None);
    }

    #[test]
    fn wheel_lines_from_reg_output() {
        let three = "\r\nHKEY_CURRENT_USER\\Control Panel\\Desktop\r\n    WheelScrollLines    REG_SZ    3\r\n";
        assert_eq!(parse_wheel_lines(three), Some(3));
        assert_eq!(parse_wheel_lines(&three.replace("REG_SZ    3", "REG_SZ    7")), Some(7));
        // Un écran à la fois, molette coupée, sortie inattendue : inconnu.
        assert_eq!(parse_wheel_lines(&three.replace("REG_SZ    3", "REG_SZ    -1")), None);
        assert_eq!(parse_wheel_lines(&three.replace("REG_SZ    3", "REG_SZ    0")), None);
        assert_eq!(parse_wheel_lines("Erreur"), None);
    }
}
