//! Notes de la version qui vient d'être installée.
//!
//! Au clic sur « Télécharger », les notes du manifeste signé sont gardées
//! dans `nouveautes.txt`, à côté de `local.toml` : première ligne, la
//! version ; la suite, les notes. Au lancement suivant, si l'exécutable est
//! bien cette version, un bandeau les propose une fois. Rien n'est demandé au
//! réseau pour cela.

use std::path::{Path, PathBuf};

const FILE: &str = "nouveautes.txt";
/// Borne de lecture : les notes d'un manifeste font 2 000 caractères au plus.
const MAX_BYTES: u64 = 16 * 1024;

fn file(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// Garde les notes de `version`. Un échec d'écriture est sans conséquence :
/// le bandeau ne s'affichera pas.
pub fn save(dir: &Path, version: &str, notes: &str) {
    let version: String = version.lines().next().unwrap_or_default().trim().to_string();
    if version.is_empty() {
        return;
    }
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(file(dir), format!("{version}\n{}", notes.trim()));
}

/// Notes gardées : (version, notes). `None` : aucun fichier, ou illisible.
pub fn load(dir: &Path) -> Option<(String, String)> {
    let path = file(dir);
    if std::fs::metadata(&path).ok()?.len() > MAX_BYTES {
        return None;
    }
    let text = std::fs::read_to_string(path).ok()?;
    let (version, notes) = text.split_once('\n').unwrap_or((&text, ""));
    let version = version.trim();
    (!version.is_empty()).then(|| (version.to_string(), notes.trim().to_string()))
}

/// Oublie les notes gardées.
pub fn clear(dir: &Path) {
    let _ = std::fs::remove_file(file(dir));
}

/// Notes à proposer à ce lancement : celles de `running`, la version de cet
/// exécutable. Des notes d'une autre version (téléchargement jamais installé,
/// retour à une version d'avant) sont oubliées.
pub fn pending(dir: &Path, running: &str) -> Option<String> {
    let (version, notes) = load(dir)?;
    if version == running {
        return Some(notes);
    }
    // Téléchargée mais pas encore installée : les notes attendent le redémarrage.
    let newer = match (super::Version::parse(&version), super::Version::parse(running)) {
        (Some(kept), Some(now)) => kept > now,
        _ => false,
    };
    if !newer {
        clear(dir);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrocord-nouveautes-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn notes_are_offered_to_the_version_they_describe() {
        let dir = temp("meme");
        assert_eq!(pending(&dir, "1.2.3"), None, "aucun fichier");
        save(&dir, "1.2.3", "- Une chose\n- Une autre\n");
        assert_eq!(load(&dir), Some(("1.2.3".into(), "- Une chose\n- Une autre".into())));
        assert_eq!(pending(&dir, "1.2.3").as_deref(), Some("- Une chose\n- Une autre"));
        clear(&dir);
        assert_eq!(pending(&dir, "1.2.3"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notes_wait_for_the_restart_and_old_ones_are_forgotten() {
        let dir = temp("autre");
        // Téléchargée, pas encore installée : gardées.
        save(&dir, "1.3.0", "Nouveau");
        assert_eq!(pending(&dir, "1.2.3"), None);
        assert!(load(&dir).is_some());
        // Version dépassée : oubliées.
        assert_eq!(pending(&dir, "1.4.0"), None);
        assert_eq!(load(&dir), None);
        // Sans notes : la version seule suffit.
        save(&dir, "2.0.0", "");
        assert_eq!(pending(&dir, "2.0.0").as_deref(), Some(""));
        // Version vide : rien n'est écrit.
        clear(&dir);
        save(&dir, "  ", "x");
        assert_eq!(load(&dir), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
