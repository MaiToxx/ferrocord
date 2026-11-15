//! Ce que l'installation retient d'un lancement à l'autre, dans
//! `<réglages>/ferrocord/installation.json` : la proposition d'installation
//! déjà montrée, le réglage « Vérifier automatiquement » et l'heure de la
//! dernière vérification des mises à jour. Rien d'autre, rien de personnel.

use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

pub const FILE: &str = "installation.json";

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Memo {
    /// La fenêtre « Installer Ferrocord » a déjà été montrée : plus jamais.
    #[serde(default)]
    pub proposition_vue: bool,
    /// `None` : jamais réglé (activé dès qu'une adresse est configurée).
    #[serde(default)]
    pub verifier_auto: Option<bool>,
    /// Dernière vérification des mises à jour (secondes Unix), réussie ou non.
    #[serde(default)]
    pub derniere_verification: u64,
}

/// À côté de `local.toml`.
pub fn default_path() -> Option<PathBuf> {
    crate::machine::default_dir().map(|d| d.join(FILE))
}

/// Fichier absent ou illisible : tout par défaut.
pub fn read(path: &Path) -> Memo {
    std::fs::read(path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn write(path: &Path, memo: &Memo) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(memo).map_err(io::Error::other)?)?;
    std::fs::rename(&tmp, path)
}

/// Secondes Unix de maintenant.
pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_garbage() {
        let dir = std::env::temp_dir().join(format!("ferrocord-memo-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("sous").join(FILE);
        assert_eq!(read(&path), Memo::default());
        let m = Memo { proposition_vue: true, verifier_auto: Some(false), derniere_verification: 42 };
        write(&path, &m).unwrap();
        assert_eq!(read(&path), m);
        std::fs::write(&path, b"{").unwrap();
        assert_eq!(read(&path), Memo::default());
        // Fichier d'une version plus ancienne : les champs absents valent leur défaut.
        std::fs::write(&path, b"{\"proposition_vue\":true}").unwrap();
        assert_eq!(read(&path), Memo { proposition_vue: true, ..Memo::default() });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
