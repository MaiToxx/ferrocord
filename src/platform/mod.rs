//! Ce qui dépend du système, hors Windows (phase 7) : dossiers de
//! l'application, bureau (notifications), et le point d'entrée Android.
//!
//! Windows garde son code tel quel (`desk::system`, `install`, Gestionnaire
//! d'identifiants…) ; ce module n'y ajoute qu'un aiguillage. Sur Android,
//! [`android`] fournit le reste par JNI (Keystore, Intent, notifications,
//! permission du micro).

#[cfg(target_os = "android")]
pub mod android;
// Fichiers projetés en mémoire (polices du système, lot C9).
#[cfg(windows)]
pub mod mapped;
// Barre de titre intégrée : procédure de fenêtre doublée (lot V1).
#[cfg(windows)]
pub mod caption;
// Fond de fenêtre Mica ou acrylique de Windows 11 (lot E5).
#[cfg(windows)]
pub mod backdrop;
/// Hors Windows : pas de fond de fenêtre du système.
#[cfg(not(windows))]
pub mod backdrop {
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum Material {
        Mica,
        Acrylic,
    }
    pub fn set_window(_: isize) {}
    pub fn window() -> isize {
        0
    }
    pub fn set_transparent(_: bool) {}
    pub fn transparent() -> bool {
        false
    }
    pub fn supported() -> bool {
        false
    }
    pub fn apply(_: Option<Material>, _: bool) -> bool {
        false
    }
}
#[cfg(not(windows))]
mod prefs_desk;
// Lecture à voix haute d'un message (lot R du groupe 3).
pub mod speech;
// Horloge de dernière entrée, pour le statut inactif automatique (lot FF).
pub mod idle;

// Pointeur pendant un glisser-déposer de fichiers (dépôt sur un salon de la liste).
pub mod drop_point;

// Signal de changement d'un dossier, icône de la fenêtre (lot « cpu »).
pub mod dirwatch;
pub mod app_icon;
// Ressources de l'exécutable (lot « icône ») : le code est celui de `build.rs`, relu ici par ses tests.
#[cfg(test)]
mod winres;

use crate::desk::Desktop;
use crate::images::Source;
use crate::net::Wake;
use std::path::PathBuf;
use std::sync::Arc;

/// Dossier des réglages (`<dossier>/ferrocord/…`) : celui du système sur
/// ordinateur, le stockage privé de l'application sur Android.
pub fn config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    return android::files_dir();
    #[cfg(not(target_os = "android"))]
    dirs::config_dir()
}

/// Dossier du cache (images, mesures) : celui du système, ou le cache privé
/// de l'application sur Android (que le système peut vider).
pub fn cache_dir() -> Option<PathBuf> {
    #[cfg(target_os = "android")]
    return android::cache_dir();
    #[cfg(not(target_os = "android"))]
    dirs::cache_dir()
}

/// Client HTTP à compléter (agent, délais…). Sur Android, les certificats
/// sont vérifiés par les racines de Mozilla embarquées : le vérificateur du
/// système de reqwest y exige une classe Java absente de l'APK.
pub fn http_builder() -> reqwest::ClientBuilder {
    #[cfg(target_os = "android")]
    return reqwest::Client::builder().tls_certs_only(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().filter_map(|c| reqwest::Certificate::from_der(c).ok()));
    #[cfg(not(target_os = "android"))]
    reqwest::Client::builder()
}

/// Touche retour sans écran précédent : sur Android, l'application passe en
/// arrière-plan ; ailleurs, rien.
pub fn back_at_root() {
    #[cfg(target_os = "android")]
    android::keyboard::to_background();
}

/// Le bureau de ce système : toasts, sons et icône sous Windows ;
/// notifications (si permises) sur Android ; ailleurs, le seul fichier
/// `notify.toml`.
pub fn desk(wake: Wake, source: Arc<dyn Source>, images: Option<PathBuf>) -> Box<dyn Desktop> {
    #[cfg(windows)]
    return Box::new(crate::desk::Desk::start(wake, source, images));
    #[cfg(not(windows))]
    {
        let _ = (source, images);
        #[cfg(target_os = "android")]
        let notify: Option<prefs_desk::Notify> = Some(Box::new(android::notify::show));
        #[cfg(not(target_os = "android"))]
        let notify: Option<prefs_desk::Notify> = None;
        Box::new(prefs_desk::PrefsDesk::start(wake, notify))
    }
}
