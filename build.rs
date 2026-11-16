//! Deux réglages de l'éditeur de liens de MSVC.
//!
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
//!
//! Icône et version de l'exécutable (lot « icône ») : un fichier `.res`
//! écrit ici même à partir de `assets/icone/ferrocord.ico` (sans `rc.exe`
//! ni dépendance, voir `src/platform/winres.rs`) et lié au seul binaire
//! `ferrocord`. Pas de manifeste d'application : il changerait le
//! comportement du processus (DPI avant winit, page de code, contrôles
//! communs), ce qui ne se vérifie pas sans lancer l'application.

#[path = "src/platform/winres.rs"]
mod winres;

/// DLL chargées au premier appel.
const DELAYED: &[&str] = &["d3d11.dll", "mfplat.dll", "uiautomationcore.dll", "mmdevapi.dll", "coremessaging.dll"];
/// Source de l'icône de l'exécutable, produite par `platform::app_icon`.
const ICO: &str = "assets/icone/ferrocord.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=src/platform/winres.rs");
    println!("cargo:rerun-if-changed={ICO}");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
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
    resources();
}

/// Écrit le fichier `.res` et le donne à l'éditeur de liens, qui l'accepte
/// tel quel, pour le binaire seulement (ni tests, ni exemples : ils dessinent
/// l'icône de leur fenêtre au vol).
fn resources() {
    let var = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name} absent"));
    // Fichier absent ou mal formé : un avertissement et pas de ressource, pour que le test qui le
    // régénère puisse encore être compilé (un autre test le compare au logo octet à octet).
    let ico = std::fs::read(std::path::Path::new(&var("CARGO_MANIFEST_DIR")).join(ICO)).unwrap_or_default();
    let Some(res) = winres::res(&ico, &var("CARGO_PKG_VERSION")) else {
        println!("cargo:warning={ICO} absent ou mal formé, exécutable sans icône : cargo test --lib genere_le_fichier_ico -- --ignored");
        return;
    };
    let path = std::path::Path::new(&var("OUT_DIR")).join("ferrocord.res");
    std::fs::write(&path, res).unwrap_or_else(|e| panic!("écriture de {} : {e}", path.display()));
    println!("cargo:rustc-link-arg-bins={}", path.display());
}
