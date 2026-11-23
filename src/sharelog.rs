//! Journal technique du partage d'écran et de la connexion vocale :
//! `%LOCALAPPDATA%\ferrocord\partage.log`.
//!
//! Un partage qui se fige chez ceux qui regardent ne laisse rien à voir du
//! côté de celui qui partage (constaté par l'utilisateur, 0.1.38 : deux
//! correctifs faits sur des suppositions n'ont rien changé). Ce journal dit,
//! avec l'heure, ce que l'encodeur a rendu, ce qui est parti, ce que la
//! connexion a jeté et ce que le serveur a demandé.
//!
//! **Rien ne part sur le réseau**, et rien de privé n'entre ici : des
//! nombres (tailles, débits, numéros d'opération) et des noms écrits dans le
//! code. Aucun identifiant, aucun texte reçu de Discord.

use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, OnceLock, PoisonError};

/// Au-delà, le journal repart de zéro à l'ouverture suivante.
const MAX_BYTES: u64 = 2 << 20;

/// Images clés demandées par le serveur (PLI, FIR) depuis la dernière mesure.
pub static PLI: AtomicU32 = AtomicU32::new(0);
/// Images arrivées à la connexion et jetées (pas de clé, attente d'une image clé).
pub static GAP: AtomicU32 = AtomicU32::new(0);
/// Images parties sur le réseau.
pub static SENT: AtomicU32 = AtomicU32::new(0);

static PATH: OnceLock<PathBuf> = OnceLock::new();
static FILE: Mutex<Option<File>> = Mutex::new(None);

/// Fichier du journal : `%LOCALAPPDATA%\ferrocord\partage.log`.
pub fn default_path() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    return None;
    #[cfg(not(target_os = "android"))]
    Some(dirs::data_local_dir()?.join("ferrocord").join("partage.log"))
}

/// À appeler une fois au lancement. Sans cet appel (tests), rien n'est écrit.
pub fn install(path: Option<PathBuf>) {
    if let Some(p) = path {
        let _ = PATH.set(p);
    }
}

/// Ajoute une ligne datée. Le fichier n'est créé qu'à la première ligne.
pub fn line(args: std::fmt::Arguments<'_>) {
    let Some(path) = PATH.get() else { return };
    let mut file = FILE.lock().unwrap_or_else(PoisonError::into_inner);
    if file.is_none() {
        let big = std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        *file = std::fs::OpenOptions::new().create(true).append(!big).write(true).truncate(big).open(path).ok();
        if let Some(f) = file.as_mut() {
            let _ = writeln!(f, "{} ---- Ferrocord {} ----", chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f"), env!("CARGO_PKG_VERSION"));
        }
    }
    if let Some(f) = file.as_mut() {
        let _ = writeln!(f, "{} {args}", chrono::Local::now().format("%H:%M:%S%.3f"));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn nothing_is_written_without_install() {
        // Les tests n'appellent jamais `install` : aucune ligne, aucun fichier.
        super::line(format_args!("essai"));
        assert!(super::FILE.lock().unwrap().is_none());
    }
}
