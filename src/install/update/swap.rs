//! Remplacement de l'exécutable au lancement suivant, et retour arrière.
//!
//! Dans le dossier de l'exécutable :
//! - `ferrocord.new.exe` : version téléchargée et vérifiée, en attente ;
//! - `ferrocord.old.exe` : version précédente, gardée jusqu'à ce que la
//!   nouvelle ait affiché sa première image ;
//! - `ferrocord.echec.exe` : version qui n'a pas démarré (supprimée au
//!   lancement suivant) ;
//! - `ferrocord-maj.json` : où on en est.
//!
//! Au lancement ([`startup`]) : une version en attente remplace l'actuelle
//! (Windows permet de renommer un exécutable en cours) puis on relance ; une
//! version à l'essai qui n'a pas confirmé son démarrage la fois précédente
//! est remplacée par l'ancienne, et n'est plus proposée.

use super::fetch::file_sha256;
use super::manifest::{hex, unhex, Manifest};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const NEW: &str = "ferrocord.new.exe";
pub const OLD: &str = "ferrocord.old.exe";
pub const FAILED: &str = "ferrocord.echec.exe";
pub const STATE: &str = "ferrocord-maj.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub version: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Trial {
    pub version: String,
    /// Lancements de la nouvelle version sans confirmation.
    pub starts: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub pending: Option<Pending>,
    #[serde(default)]
    pub trial: Option<Trial>,
    /// Version revenue en arrière : plus proposée.
    #[serde(default)]
    pub refused: Option<String>,
}

/// L'exécutable et son dossier.
#[derive(Clone, Debug)]
pub struct Layout {
    pub exe: PathBuf,
}

impl Layout {
    pub fn current() -> Option<Layout> {
        std::env::current_exe().ok().map(|exe| Layout { exe })
    }

    fn dir(&self) -> &Path {
        self.exe.parent().unwrap_or(Path::new("."))
    }

    pub fn file(&self, name: &str) -> PathBuf {
        self.dir().join(name)
    }

    pub fn read(&self) -> State {
        std::fs::read(self.file(STATE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn write(&self, s: &State) -> io::Result<()> {
        let path = self.file(STATE);
        if *s == State::default() {
            return match std::fs::remove_file(&path) {
                Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
                _ => Ok(()),
            };
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(s).map_err(io::Error::other)?)?;
        std::fs::rename(&tmp, &path)
    }
}

/// Ce que doit faire le lancement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Startup {
    Continue,
    /// L'exécutable a changé : le relancer, puis quitter.
    Relaunch,
}

fn remove_if_present(p: &Path) -> io::Result<()> {
    match std::fs::remove_file(p) {
        Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// À appeler tout au début du lancement, avant la fenêtre.
pub fn startup(l: &Layout) -> io::Result<Startup> {
    let mut s = l.read();
    if let Some(trial) = s.trial.clone() {
        let old = l.file(OLD);
        if trial.starts == 0 || !old.exists() {
            s.trial = (trial.starts == 0).then_some(Trial { starts: 1, ..trial });
            l.write(&s)?;
            return Ok(Startup::Continue);
        }
        // Lancée une fois sans jamais afficher d'image : on revient en arrière.
        let failed = l.file(FAILED);
        remove_if_present(&failed)?;
        std::fs::rename(&l.exe, &failed)?;
        if let Err(e) = std::fs::rename(&old, &l.exe) {
            let _ = std::fs::rename(&failed, &l.exe);
            return Err(e);
        }
        s.trial = None;
        s.refused = Some(trial.version);
        l.write(&s)?;
        return Ok(Startup::Relaunch);
    }
    if let Some(p) = s.pending.take() {
        let new = l.file(NEW);
        let good = unhex::<32>(&p.sha256).is_some_and(|want| std::fs::metadata(&new).is_ok_and(|m| m.len() == p.size) && file_sha256(&new).is_ok_and(|h| h == want));
        if !good {
            // Fichier abîmé ou changé depuis le téléchargement : abandonné.
            remove_if_present(&new)?;
            l.write(&s)?;
            return Ok(Startup::Continue);
        }
        let old = l.file(OLD);
        remove_if_present(&old)?;
        std::fs::rename(&l.exe, &old)?;
        if let Err(e) = std::fs::rename(&new, &l.exe) {
            let _ = std::fs::rename(&old, &l.exe);
            return Err(e);
        }
        s.trial = Some(Trial { version: p.version, starts: 0 });
        l.write(&s)?;
        return Ok(Startup::Relaunch);
    }
    // Lancement ordinaire : restes d'un retour arrière.
    let _ = remove_if_present(&l.file(FAILED));
    Ok(Startup::Continue)
}

/// Première image affichée : la version à l'essai est gardée.
pub fn confirm(l: &Layout) -> io::Result<()> {
    let mut s = l.read();
    if s.trial.take().is_some() {
        remove_if_present(&l.file(OLD))?;
        l.write(&s)?;
    }
    Ok(())
}

/// Version téléchargée dans `ferrocord.new.exe` : en attente du lancement suivant.
pub fn stage(l: &Layout, m: &Manifest) -> io::Result<()> {
    let mut s = l.read();
    s.pending = Some(Pending { version: m.version.to_string(), sha256: hex(&m.sha256), size: m.size });
    l.write(&s)
}

/// Version retirée après un échec de démarrage.
pub fn refused(l: &Layout) -> Option<String> {
    l.read().refused
}

/// Le dossier de l'exécutable accepte-t-il l'écriture ? (installation dans
/// `Program Files` sans droits : pas de mise à jour automatique).
pub fn writable(l: &Layout) -> bool {
    let probe = l.file("ferrocord-maj.essai");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::install::update::manifest::Version;
    use aws_lc_rs::digest;

    fn layout(name: &str) -> Layout {
        let dir = std::env::temp_dir().join(format!("ferrocord-swap-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("ferrocord.exe");
        std::fs::write(&exe, b"version 1").unwrap();
        Layout { exe }
    }

    fn staged(l: &Layout, body: &[u8]) {
        std::fs::write(l.file(NEW), body).unwrap();
        let sha: [u8; 32] = digest::digest(&digest::SHA256, body).as_ref().try_into().unwrap();
        let m = Manifest { version: Version(2, 0, 0), url: String::new(), sha256: sha, size: body.len() as u64, notes: String::new() };
        stage(l, &m).unwrap();
    }

    fn read(p: PathBuf) -> String {
        String::from_utf8(std::fs::read(p).unwrap()).unwrap()
    }

    #[test]
    fn pending_version_replaces_then_confirms() {
        let l = layout("ok");
        assert_eq!(startup(&l).unwrap(), Startup::Continue);
        staged(&l, b"version 2");
        assert_eq!(startup(&l).unwrap(), Startup::Relaunch);
        assert_eq!(read(l.exe.clone()), "version 2");
        assert_eq!(read(l.file(OLD)), "version 1");
        // Le relancement : premier démarrage à l'essai.
        assert_eq!(startup(&l).unwrap(), Startup::Continue);
        assert_eq!(l.read().trial, Some(Trial { version: "2.0.0".into(), starts: 1 }));
        confirm(&l).unwrap();
        assert!(!l.file(OLD).exists());
        assert!(!l.file(STATE).exists(), "rien ne reste");
        assert_eq!(startup(&l).unwrap(), Startup::Continue);
        let _ = std::fs::remove_dir_all(l.exe.parent().unwrap());
    }

    #[test]
    fn version_that_never_shows_a_frame_is_rolled_back() {
        let l = layout("echec");
        staged(&l, b"version 2");
        assert_eq!(startup(&l).unwrap(), Startup::Relaunch);
        assert_eq!(startup(&l).unwrap(), Startup::Continue, "essai");
        // Plantage avant la première image, puis nouveau lancement.
        assert_eq!(startup(&l).unwrap(), Startup::Relaunch);
        assert_eq!(read(l.exe.clone()), "version 1");
        assert_eq!(refused(&l).as_deref(), Some("2.0.0"));
        assert_eq!(startup(&l).unwrap(), Startup::Continue);
        assert!(!l.file(FAILED).exists(), "version en échec supprimée");
        let _ = std::fs::remove_dir_all(l.exe.parent().unwrap());
    }

    #[test]
    fn tampered_download_is_dropped() {
        let l = layout("abime");
        staged(&l, b"version 2");
        std::fs::write(l.file(NEW), b"version X").unwrap();
        assert_eq!(startup(&l).unwrap(), Startup::Continue);
        assert_eq!(read(l.exe.clone()), "version 1");
        assert!(!l.file(NEW).exists());
        assert_eq!(l.read(), State::default());
        // État illisible : ignoré.
        std::fs::write(l.file(STATE), b"{").unwrap();
        assert_eq!(startup(&l).unwrap(), Startup::Continue);
        assert!(writable(&l));
        let _ = std::fs::remove_dir_all(l.exe.parent().unwrap());
    }
}
