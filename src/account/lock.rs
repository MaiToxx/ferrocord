//! Verrou entre instances : deux Ferrocord ouverts en même temps ne migrent
//! pas l'ancien fichier et n'écrivent pas dans le coffre au même moment.
//!
//! C'est un fichier vide (`<config>/ferrocord/comptes.lock`) verrouillé de
//! façon exclusive par le système ([`std::fs::File::try_lock`], sans code
//! `unsafe`) le temps d'une opération. Le verrou est rendu à la fermeture du
//! fichier, y compris si le processus s'arrête brutalement : il ne reste
//! jamais « coincé ». Le contenu du fichier n'a aucune importance.

use crate::i18n::{
    language,
    verbatim::{f as vf, t as vt},
};
use std::{
    fmt, fs,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

/// Emplacement du fichier de verrou.
pub fn lock_path() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("comptes.lock"))
}

/// Attente maximale du verrou tenu par une autre instance.
pub const LOCK_WAIT: Duration = Duration::from_secs(5);

/// Condition qui garde le verrou après la fin d'une opération.
type Pin = Arc<dyn Fn() -> bool + Send + Sync>;

/// Verrou partagé par les instances qui ouvrent le même fichier.
#[derive(Clone)]
pub struct InstanceLock {
    /// `None` : pas de verrou (tests, aperçu : rien n'est partagé).
    path: Option<PathBuf>,
    wait: Duration,
    /// Tant que cette condition est vraie, le verrou rendu reste tenu
    /// (un appel au coffre abandonné n'est pas encore terminé).
    pin: Option<Pin>,
    /// Verrou gardé entre deux opérations à cause de `pin`.
    kept: Arc<Mutex<Option<fs::File>>>,
}

impl fmt::Debug for InstanceLock {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InstanceLock").field("path", &self.path).field("wait", &self.wait).finish_non_exhaustive()
    }
}

/// Verrou tenu ; rendu quand il est lâché (sauf si la condition de
/// [`InstanceLock::pinned_while`] est vraie à ce moment-là).
#[must_use]
pub struct Held {
    file: Option<fs::File>,
    pin: Option<Pin>,
    kept: Arc<Mutex<Option<fs::File>>>,
}

impl Drop for Held {
    fn drop(&mut self) {
        if let (Some(file), Some(pin)) = (self.file.take(), self.pin.as_ref())
            && pin()
            && let Ok(mut k) = self.kept.lock()
        {
            *k = Some(file);
        }
    }
}

impl InstanceLock {
    pub fn new(path: Option<PathBuf>, wait: Duration) -> Self {
        Self { path, wait, pin: None, kept: Arc::default() }
    }

    /// Aucun verrou (une seule instance possible : tests, aperçu).
    pub fn none() -> Self {
        Self::new(None, Duration::ZERO)
    }

    /// Le verrou reste tenu après une opération tant que `busy` est vrai
    /// (rendu à l'opération suivante où `busy` est devenu faux, ou à la fin
    /// du processus).
    pub fn pinned_while(mut self, busy: impl Fn() -> bool + Send + Sync + 'static) -> Self {
        self.pin = Some(Arc::new(busy));
        self.kept = Arc::default();
        self
    }

    /// Le verrou est gardé en ce moment (appel abandonné en cours).
    pub fn is_kept(&self) -> bool {
        self.kept.lock().map(|k| k.is_some()).unwrap_or(false)
    }

    fn held(&self, file: Option<fs::File>) -> Held {
        Held { file, pin: self.pin.clone(), kept: self.kept.clone() }
    }

    /// Prend le verrou, en attendant au plus le délai prévu.
    pub fn acquire(&self) -> Result<Held, String> {
        let Some(path) = &self.path else { return Ok(self.held(None)) };
        // Déjà tenu (gardé depuis l'opération d'avant) : repris tel quel, ou rendu.
        if let Ok(mut k) = self.kept.lock()
            && let Some(file) = k.take()
        {
            if self.pin.as_ref().is_some_and(|p| p()) {
                return Ok(self.held(Some(file)));
            }
            drop(file);
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir).map_err(|e| vf::lock_create(language(), e))?;
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|e| vf::lock_open(language(), e))?;
        let end = Instant::now() + self.wait;
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(self.held(Some(file))),
                Err(fs::TryLockError::WouldBlock) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
                Err(fs::TryLockError::WouldBlock) => return Err(vt::LOCK_BUSY.get(language()).into()),
                Err(fs::TryLockError::Error(e)) => return Err(vf::lock_failed(language(), e)),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_holder_waits_then_gives_up() {
        let dir = std::env::temp_dir().join(format!("ferrocord-verrou-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("sous-dossier").join("comptes.lock");
        let a = InstanceLock::new(Some(path.clone()), Duration::from_millis(50));
        let b = InstanceLock::new(Some(path.clone()), Duration::from_millis(50));
        let held = a.acquire().unwrap_or_else(|e| panic!("{e}"));
        let start = Instant::now();
        assert!(b.acquire().is_err(), "verrou déjà tenu");
        assert!(start.elapsed() >= Duration::from_millis(50), "attente avant d'abandonner");
        drop(held);
        assert!(b.acquire().is_ok(), "verrou rendu");
        assert!(InstanceLock::none().acquire().is_ok());
        let _ = fs::remove_dir_all(&dir);
    }

    /// Tant qu'un appel abandonné n'est pas fini, le verrou n'est pas rendu.
    #[test]
    fn pinned_lock_stays_held_until_released() {
        let dir = std::env::temp_dir().join(format!("ferrocord-verrou-garde-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let path = dir.join("comptes.lock");
        let busy = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let b2 = busy.clone();
        let a = InstanceLock::new(Some(path.clone()), Duration::from_millis(30)).pinned_while(move || b2.load(std::sync::atomic::Ordering::SeqCst));
        let other = InstanceLock::new(Some(path.clone()), Duration::from_millis(30));
        drop(a.acquire().unwrap_or_else(|e| panic!("{e}")));
        assert!(a.is_kept(), "gardé après l'opération");
        assert!(other.acquire().is_err(), "l'autre instance attend");
        drop(a.acquire().unwrap_or_else(|e| panic!("repris : {e}")));
        busy.store(false, std::sync::atomic::Ordering::SeqCst);
        drop(a.acquire().unwrap_or_else(|e| panic!("{e}")));
        assert!(!a.is_kept());
        assert!(other.acquire().is_ok(), "rendu une fois l'appel fini");
        let _ = fs::remove_dir_all(&dir);
    }
}
