//! Mises à jour automatiques signées.
//!
//! Source : les « Releases » du dépôt GitHub de `depot.txt` ([`source`]), ou
//! une adresse de manifeste écrite dans `local.toml` (section
//! `[mises_a_jour]`, clé `manifeste`) ; sans l'une ni l'autre, désactivées. Le
//! manifeste est signé (Ed25519) par la clé privée de l'auteur, gardée hors
//! du dépôt ; la clé publique est intégrée au binaire (`cle_publique.txt`).
//! Vérification au démarrage puis une fois par jour au plus ; rien n'est
//! téléchargé ni installé sans un clic.

pub mod fetch;
pub mod manifest;
pub mod news;
pub mod source;
pub mod swap;

#[cfg(test)]
mod bout_en_bout;

pub use manifest::{Manifest, Version};

/// Où en est la mise à jour, pour l'interface.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Status {
    /// Ni dépôt configuré à la compilation, ni adresse dans `local.toml`.
    #[default]
    NoSource,
    /// Rien vérifié depuis le lancement : vérification automatique coupée,
    /// ou dernière vérification trop récente pour en refaire une.
    Idle,
    /// Aucune clé publique intégrée à cet exécutable.
    NoKey,
    Checking,
    /// À jour (`refused` : version plus récente retirée après un échec).
    UpToDate { refused: Option<String> },
    Available(Manifest),
    Downloading { done: u64, total: u64 },
    /// Téléchargée et vérifiée : installée au prochain lancement.
    Ready(Version),
    Failed(String),
}
