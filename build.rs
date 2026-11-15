//! Démarrage plus rapide sous Windows (lot C8) : les DLL des fonctions qui ne
//! servent qu'après l'ouverture de la fenêtre sont chargées à leur premier
//! appel (`/DELAYLOAD` de l'éditeur de liens de MSVC) au lieu de l'être par
//! Windows avant `main` : Direct3D 11 et la file de messages de WinRT
//! (partage d'écran), Media Foundation (codecs vidéo), l'énumération des
//! périphériques audio (vocal) et UI Automation (lecteurs d'écran, chargée
//! seulement quand un outil d'accessibilité interroge la fenêtre).
//! Aucune n'exporte de données (seulement des fonctions) : le chargement
//! différé est sûr. Une DLL absente donnerait une erreur à l'appel, pas au
//! lancement.

/// DLL chargées au premier appel.
const DELAYED: &[&str] = &["d3d11.dll", "mfplat.dll", "uiautomationcore.dll", "mmdevapi.dll", "coremessaging.dll"];

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let target = std::env::var("TARGET").unwrap_or_default();
    if !target.ends_with("windows-msvc") {
        return;
    }
    for dll in DELAYED {
        println!("cargo:rustc-link-arg=/DELAYLOAD:{dll}");
    }
    // Aide de l'éditeur de liens qui charge les DLL différées.
    println!("cargo:rustc-link-arg=delayimp.lib");
    // Exécutables qui n'appellent pas toutes ces DLL (tests) : l'avertissement
    // « /DELAYLOAD ignoré » n'apprend rien.
    println!("cargo:rustc-link-arg=/IGNORE:4199");
}
