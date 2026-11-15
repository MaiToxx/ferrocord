//! Comptes : jeton enveloppé ([`Token`]), coffre à jetons ([`Vault`]),
//! migration de l'ancien fichier en clair ([`migrate`]) et fil des comptes
//! ([`AccountLink`]) qui fait tous les appels au coffre et au disque hors du
//! fil d'interface.
//!
//! La liste des comptes connus (identifiant, nom, empreinte d'avatar, jamais
//! le jeton) vit dans `settings.toml` (voir `crate::settings`).

mod link;
pub mod lock;
pub mod migrate;
pub mod token;
pub mod vault;

pub use link::{AccountLink, Act, Expect, LinkOptions, Reply, Started, SHUTDOWN_WAIT};
pub use lock::{lock_path, InstanceLock};
pub use migrate::{legacy_path, CopyLeft, FileMark, FileState, Finish, Migration, Survey, WipeCheck, WipeFailure};
pub use token::{Token, TokenError};
pub use vault::{Faults, MemoryVault, SystemVault, TimedVault, Vault, VaultError};

use std::fmt;

/// Nom d'un compte dans le coffre et dans `settings.toml` : l'identifiant
/// Discord du compte, ou `importe` pour le jeton venu de l'ancien fichier
/// (dont l'identité n'est connue qu'après READY).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct AccountKey(String);

impl AccountKey {
    const IMPORTED: &'static str = "importe";

    /// Clé du jeton migré depuis l'ancien fichier.
    pub fn imported() -> Self {
        AccountKey(Self::IMPORTED.into())
    }

    /// Clé d'un compte validé par Discord.
    pub fn of_user(id: u64) -> Self {
        AccountKey(id.to_string())
    }

    /// Clé lue dans un fichier : `importe` ou un identifiant décimal.
    pub fn parse(s: &str) -> Option<Self> {
        let ok = s == Self::IMPORTED || (!s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()) && s.parse::<u64>().is_ok());
        ok.then(|| AccountKey(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn is_imported(&self) -> bool {
        self.0 == Self::IMPORTED
    }
}

impl fmt::Display for AccountKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_ids_or_imported() {
        assert_eq!(AccountKey::parse("importe"), Some(AccountKey::imported()));
        assert_eq!(AccountKey::parse("1000000000000000001"), Some(AccountKey::of_user(1000000000000000001)));
        for bad in ["", "abc", "-1", "1.5", "99999999999999999999", "importé", "12 ", "../x"] {
            assert_eq!(AccountKey::parse(bad), None, "{bad:?}");
        }
    }
}
