//! Registre de l'utilisateur (HKCU) derrière une petite interface : le vrai
//! registre par `windows-registry` (sans `unsafe` de notre côté), et un
//! registre en mémoire pour les tests, qui n'écrivent jamais dans le vrai.

use std::collections::BTreeMap;
use std::io;

/// Clés sous `HKEY_CURRENT_USER` ; `name` vide : valeur par défaut de la clé.
pub trait Hive: Send {
    /// Valeur texte, `None` si la clé ou la valeur manque.
    fn get(&self, key: &str, name: &str) -> Option<String>;
    /// Crée la clé au besoin.
    fn set(&mut self, key: &str, name: &str, value: &str) -> io::Result<()>;
    /// Valeur entière (REG_DWORD) ; crée la clé au besoin.
    fn set_u32(&mut self, key: &str, name: &str, value: u32) -> io::Result<()>;
    /// Sans erreur si la valeur manquait déjà.
    fn remove_value(&mut self, key: &str, name: &str) -> io::Result<()>;
    /// La clé et tout ce qu'elle contient ; sans erreur si elle manquait.
    fn remove_tree(&mut self, key: &str) -> io::Result<()>;
}

/// Registre en mémoire (tests, aperçu). Comme Windows, sans casse.
#[derive(Clone, Debug, Default)]
pub struct MemoryHive {
    values: BTreeMap<(String, String), String>,
}

impl MemoryHive {
    fn k(key: &str, name: &str) -> (String, String) {
        (key.trim_matches('\\').to_ascii_lowercase(), name.to_ascii_lowercase())
    }

    /// Nombre de valeurs (tests : rien n'est laissé derrière).
    pub fn len(&self) -> usize {
        self.values.len()
    }

    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
}

impl Hive for MemoryHive {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        self.values.get(&Self::k(key, name)).cloned()
    }

    fn set(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        self.values.insert(Self::k(key, name), value.to_string());
        Ok(())
    }

    /// Gardée comme son écriture décimale (les tests relisent par `get`).
    fn set_u32(&mut self, key: &str, name: &str, value: u32) -> io::Result<()> {
        self.set(key, name, &value.to_string())
    }

    fn remove_value(&mut self, key: &str, name: &str) -> io::Result<()> {
        self.values.remove(&Self::k(key, name));
        Ok(())
    }

    fn remove_tree(&mut self, key: &str) -> io::Result<()> {
        let (root, _) = Self::k(key, "");
        let below = format!("{root}\\");
        self.values.retain(|(k, _), _| *k != root && !k.starts_with(&below));
        Ok(())
    }
}

/// Le vrai registre de l'utilisateur.
#[cfg(windows)]
#[derive(Clone, Copy, Debug, Default)]
pub struct UserHive;

#[cfg(windows)]
fn io_err(e: impl std::fmt::Display) -> io::Error {
    io::Error::other(e.to_string())
}

#[cfg(windows)]
impl Hive for UserHive {
    fn get(&self, key: &str, name: &str) -> Option<String> {
        windows_registry::CURRENT_USER.open(key).ok()?.get_string(name).ok()
    }

    fn set(&mut self, key: &str, name: &str, value: &str) -> io::Result<()> {
        windows_registry::CURRENT_USER.create(key).and_then(|k| k.set_string(name, value)).map_err(io_err)
    }

    fn set_u32(&mut self, key: &str, name: &str, value: u32) -> io::Result<()> {
        windows_registry::CURRENT_USER.create(key).and_then(|k| k.set_u32(name, value)).map_err(io_err)
    }

    fn remove_value(&mut self, key: &str, name: &str) -> io::Result<()> {
        let Ok(k) = windows_registry::CURRENT_USER.create(key) else { return Ok(()) };
        if k.get_type(name).is_err() {
            return Ok(());
        }
        k.remove_value(name).map_err(io_err)
    }

    fn remove_tree(&mut self, key: &str) -> io::Result<()> {
        if windows_registry::CURRENT_USER.open(key).is_err() {
            return Ok(());
        }
        windows_registry::CURRENT_USER.remove_tree(key).map_err(io_err)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_hive_behaves_like_the_registry() {
        let mut h = MemoryHive::default();
        h.set(r"Software\Classes\discord", "", "URL:Discord").unwrap();
        h.set(r"Software\Classes\discord\shell\open\command", "", "x").unwrap();
        h.set(r"Software\Classes\discordx", "", "garde").unwrap();
        assert_eq!(h.get(r"SOFTWARE\classes\DISCORD", "").as_deref(), Some("URL:Discord"));
        h.remove_tree(r"Software\Classes\discord").unwrap();
        assert_eq!(h.get(r"Software\Classes\discord\shell\open\command", ""), None);
        assert_eq!(h.get(r"Software\Classes\discordx", "").as_deref(), Some("garde"), "clé voisine intacte");
        h.remove_value("absente", "x").unwrap();
    }
}
