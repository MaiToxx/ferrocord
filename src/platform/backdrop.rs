//! Fond de fenêtre de Windows 11 : Mica ou acrylique (lot E5, plugin
//! `mica_windows`), posé par DWM derrière ce que l'interface laisse
//! transparent (le rail et la colonne des salons, sous un voile du thème).
//!
//! - La fenêtre doit avoir été créée transparente (`lib::with_backdrop`) :
//!   c'est un réglage de création, d'où le redémarrage que demande le plugin.
//! - `DwmSetWindowAttribute(DWMWA_SYSTEMBACKDROP_TYPE)` choisit la matière ;
//!   il n'existe qu'à partir de Windows 11 22H2 (compilation 22621) et échoue
//!   ailleurs : [`apply`] rend alors faux et rien ne change.
//! - `DwmExtendFrameIntoClientArea` étend le cadre à toute la zone cliente :
//!   sans lui, la matière ne se voit que dans une barre de titre que la
//!   fenêtre n'a pas.
//! - `DWMWA_USE_IMMERSIVE_DARK_MODE` donne à la matière sa teinte sombre ou claire.
//!
//! Les attributs sont posés une fois (et quand la matière ou la teinte
//! change) : rien n'est fait à chaque image.
//!
//! Tout l'`unsafe` du fond de fenêtre est dans ce fichier.

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMSBT_NONE, DWMSBT_TRANSIENTWINDOW, DWMWA_SYSTEMBACKDROP_TYPE, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::UI::Controls::MARGINS;

/// Première compilation de Windows qui connaît `DWMWA_SYSTEMBACKDROP_TYPE` (11 22H2).
pub const MIN_BUILD: u32 = 22621;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    Mica,
    Acrylic,
}

/// Fenêtre principale (0 : aucune).
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// La fenêtre a été créée transparente.
static TRANSPARENT: AtomicBool = AtomicBool::new(false);

/// Retient la fenêtre principale. À appeler sur son fil, une fois créée.
pub fn set_window(hwnd: isize) {
    WINDOW.store(hwnd, Ordering::SeqCst);
}

/// La fenêtre principale retenue ; 0 : pas encore créée.
pub fn window() -> isize {
    WINDOW.load(Ordering::SeqCst)
}

/// La fenêtre principale est créée transparente (dit avant sa création).
pub fn set_transparent(on: bool) {
    TRANSPARENT.store(on, Ordering::SeqCst);
}

pub fn transparent() -> bool {
    TRANSPARENT.load(Ordering::SeqCst)
}

/// Ce Windows connaît les fonds de fenêtre du système (lu une fois dans le
/// registre, sans `unsafe`).
pub fn supported() -> bool {
    static BUILD: OnceLock<Option<u32>> = OnceLock::new();
    let build = BUILD.get_or_init(|| windows_registry::LOCAL_MACHINE.open(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").and_then(|k| k.get_string("CurrentBuildNumber")).ok().and_then(|s| s.trim().parse().ok()));
    build.is_some_and(|b| b >= MIN_BUILD)
}

/// Pose la matière `material` derrière la fenêtre principale (`None` : la
/// retire), teintée sombre ou claire. Rend vrai si Windows l'a acceptée.
pub fn apply(material: Option<Material>, dark: bool) -> bool {
    let hwnd = WINDOW.load(Ordering::SeqCst);
    if hwnd == 0 {
        return false;
    }
    let window = HWND(hwnd as *mut core::ffi::c_void);
    let kind: i32 = match material {
        None => DWMSBT_NONE.0,
        Some(Material::Mica) => DWMSBT_MAINWINDOW.0,
        Some(Material::Acrylic) => DWMSBT_TRANSIENTWINDOW.0,
    };
    let dark = i32::from(dark);
    // -1 partout : tout le cadre ; 0 : le cadre d'origine.
    let edge = if material.is_some() { -1 } else { 0 };
    let margins = MARGINS { cxLeftWidth: edge, cxRightWidth: edge, cyTopHeight: edge, cyBottomHeight: edge };
    // SAFETY: `hwnd` est la fenêtre principale retenue par `set_window`,
    // vivante autant que l'application (si elle ne l'est plus, les appels
    // échouent sans effet). Chaque pointeur désigne une valeur de la pile,
    // vivante pendant l'appel, de la taille annoncée : deux entiers de 32 bits
    // (BOOL et DWM_SYSTEMBACKDROP_TYPE) et une structure MARGINS.
    unsafe {
        let size = size_of::<i32>() as u32;
        // Teinte : sans effet sur la suite si elle est refusée.
        let _ = DwmSetWindowAttribute(window, DWMWA_USE_IMMERSIVE_DARK_MODE, (&raw const dark).cast(), size);
        let ok = DwmSetWindowAttribute(window, DWMWA_SYSTEMBACKDROP_TYPE, (&raw const kind).cast(), size).is_ok();
        ok && DwmExtendFrameIntoClientArea(window, &margins).is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sans fenêtre retenue, rien n'est appelé et rien n'est en service.
    #[test]
    fn nothing_without_a_window() {
        assert!(!apply(Some(Material::Mica), true));
        assert!(!apply(None, false));
        assert!(!transparent());
    }
}
