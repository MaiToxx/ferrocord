//! Installation et intégration à Windows (lot C6) : démarrage avec Windows,
//! protocole `discord://`, instance unique, ouverture des liens externes
//! sans le jeton, mises à jour signées, licences des dépendances.
//!
//! L'interface ne parle qu'au trait [`Installer`] ; registre, disque et
//! réseau se font dans le fil « ferrocord-installation » ([`service`]).

pub mod browser;
pub mod hive;
pub mod licenses;
pub mod links;
// Réglage et dates retenus d'un lancement à l'autre (lot distrib).
pub mod memo;
pub mod service;
// Installation et désinstallation par l'exécutable lui-même (lot distrib).
pub mod setup;
pub mod shortcut;
pub mod single;
pub mod system;
pub mod update;

use std::sync::atomic::{AtomicBool, Ordering};

pub use system::{Autostart, Protocol};

/// Ce que le fil d'installation renvoie à l'interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallEvent {
    /// État du registre (démarrage avec Windows, protocole).
    Registry { autostart: Autostart, protocol: Protocol },
    Update(update::Status),
    /// Écriture refusée (registre, dossier de l'exécutable) : raison.
    Failed(String),
}

/// Le système, vu de l'interface.
pub trait Installer {
    /// Relit l'état du registre ; réponse par [`InstallEvent::Registry`].
    fn load(&mut self);
    fn set_autostart(&mut self, want: Autostart);
    fn set_protocol(&mut self, on: bool);
    /// Adresse du manifeste (`local.toml`) ; vide : celle du dépôt configuré à
    /// la compilation, et sans elle les mises à jour sont désactivées. Sinon
    /// vérification au lancement puis chaque jour au plus.
    fn configure_updates(&mut self, manifest_url: String);
    fn check_updates(&mut self);
    /// Réglage « Vérifier automatiquement » (retenu d'un lancement à l'autre).
    fn set_auto_check(&mut self, _on: bool) {}
    /// Ce réglage ; `None` : pas encore lu, ou système sans mises à jour.
    fn auto_check(&self) -> Option<bool> {
        None
    }
    /// Une version disponible est-elle annoncée par une notification ?
    /// (le vrai système seulement : ni les tests ni l'aperçu).
    fn announces(&self) -> bool {
        false
    }
    /// Télécharge et vérifie la version proposée (clic de l'utilisateur).
    fn download(&mut self, m: update::Manifest);
    /// Première image affichée : la version à l'essai est gardée.
    fn confirm_started(&mut self);
    /// Ouvre un lien hors de Ferrocord (navigateur, Explorateur), sans le jeton.
    fn open_external(&mut self, url: &str);
    fn poll(&mut self) -> Vec<InstallEvent>;
}

/// Sans système : tests, aperçu, avant `App::attach_install`. Les liens
/// demandés sont retenus, jamais ouverts.
#[derive(Default)]
pub struct Quiet {
    pub opened: Vec<String>,
}

impl Installer for Quiet {
    fn load(&mut self) {}
    fn set_autostart(&mut self, _: Autostart) {}
    fn set_protocol(&mut self, _: bool) {}
    fn configure_updates(&mut self, _: String) {}
    fn check_updates(&mut self) {}
    fn download(&mut self, _: update::Manifest) {}
    fn confirm_started(&mut self) {}
    fn open_external(&mut self, url: &str) {
        self.opened.push(url.to_string());
    }
    fn poll(&mut self) -> Vec<InstallEvent> {
        Vec::new()
    }
}

/// « Redémarrer » demandé (mise à jour prête) : `run` relance l'exécutable
/// une fois la fenêtre fermée.
static RESTART: AtomicBool = AtomicBool::new(false);

pub fn request_restart() {
    RESTART.store(true, Ordering::SeqCst);
}

/// Exécutable à lancer à la place de celui-ci au redémarrage demandé
/// (celui qui vient d'être installé depuis les paramètres).
static RESTART_AS: std::sync::Mutex<Option<std::path::PathBuf>> = std::sync::Mutex::new(None);

/// Redémarrage demandé vers un autre exécutable.
pub fn request_restart_as(exe: std::path::PathBuf) {
    if let Ok(mut to) = RESTART_AS.lock() {
        *to = Some(exe);
    }
    request_restart();
}

/// Exécutable à relancer, s'il n'est pas celui en cours.
pub fn restart_target() -> Option<std::path::PathBuf> {
    RESTART_AS.lock().ok().and_then(|to| to.clone())
}

pub fn restart_requested() -> bool {
    RESTART.load(Ordering::SeqCst)
}
