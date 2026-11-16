//! Icône de la fenêtre, sans la recalculer à chaque lancement (lot « cpu »).
//!
//! Sans icône fournie, eframe décode la sienne (PNG de 512 × 512) avant la
//! fenêtre, en donne une copie d'un Mo à winit, puis, à la première image,
//! la réduit deux fois (grande et petite icône, filtre de Lanczos) et
//! recode chaque réduction en PNG pour Windows : une douzaine de
//! millisecondes du fil d'interface, dans la première image.
//!
//! Ici la même icône est posée par le même appel
//! (`CreateIconFromResourceEx`, `WM_SETICON`), mais les réductions sont
//! faites d'avance : `assets/icone/egui-<taille>.png` sort de la chaîne
//! d'eframe (décodage, `imageops::resize` Lanczos3, codage PNG) appliquée à
//! `assets/icone/egui-512.png`, copie de l'icône d'eframe. Un test refait
//! chaque réduction et compare les pixels : l'icône affichée est la même.
//! Pour une taille non prévue (échelle d'écran inhabituelle), la réduction
//! est faite au lancement, comme le faisait eframe.

use eframe::egui;

/// Icône d'eframe, telle qu'il l'embarque (source des réductions).
#[cfg_attr(not(windows), allow(dead_code))]
const SOURCE: &[u8] = include_bytes!("../../assets/icone/egui-512.png");

macro_rules! sizes {
    ($($s:literal),*) => {
        /// Réductions faites d'avance : côté en pixels, PNG.
        #[cfg_attr(not(windows), allow(dead_code))]
        const SCALED: &[(i32, &[u8])] = &[$(($s, include_bytes!(concat!("../../assets/icone/egui-", $s, ".png")))),*];
    };
}
// Grande et petite icône de Windows aux échelles de 100 à 300 %.
sizes!(16, 20, 24, 28, 32, 40, 48, 56, 64, 80, 96);

/// eframe ne décode ni ne pose son icône : [`set`] s'en charge. Sans effet hors Windows.
pub fn skip_default(mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    if cfg!(windows) && options.viewport.icon.is_none() {
        // Icône vide : eframe la tient pour « celle par défaut, rien à faire », et winit n'en reçoit aucune.
        options.viewport.icon = Some(std::sync::Arc::new(egui::IconData::default()));
    }
    options
}

/// PNG de l'icône au côté `size` : la réduction faite d'avance, sinon la
/// même chaîne qu'eframe, au vol.
#[cfg_attr(not(windows), allow(dead_code))]
fn png(size: i32) -> Option<std::borrow::Cow<'static, [u8]>> {
    if let Some((_, bytes)) = SCALED.iter().find(|(s, _)| *s == size) {
        return Some(std::borrow::Cow::Borrowed(bytes));
    }
    scale(u32::try_from(size).ok().filter(|s| *s > 0)?).map(std::borrow::Cow::Owned)
}

/// Chaîne d'eframe (`native/app_icon.rs`) : décodage, Lanczos3, codage PNG.
#[cfg_attr(not(windows), allow(dead_code))]
fn scale(size: u32) -> Option<Vec<u8>> {
    let source = image::load_from_memory(SOURCE).ok()?.into_rgba8();
    let scaled = image::imageops::resize(&source, size, size, image::imageops::Lanczos3);
    let mut bytes = Vec::new();
    scaled.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).ok()?;
    Some(bytes)
}

/// Pose la grande puis la petite icône de la fenêtre créée par eframe
/// (barre de titre, barre des tâches, Alt+Tab), comme il le faisait.
#[cfg(windows)]
pub fn set(cc: &eframe::CreationContext<'_>) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL, LR_DEFAULTCOLOR, SM_CXICON, SM_CXSMICON, WM_SETICON};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
    // La grande d'abord, puis la petite (ordre d'eframe : l'inverse donne une petite icône floue).
    for (metric, which) in [(SM_CXICON, ICON_BIG), (SM_CXSMICON, ICON_SMALL)] {
        // SAFETY: lecture d'une mesure du système, sans effet.
        let size = unsafe { GetSystemMetrics(metric) };
        let Some(bytes) = png(size) else { return };
        // SAFETY: `bytes` est un PNG entier, vivant pendant l'appel, que la
        // fonction lit sans le garder ; 0x00030000 est la version attendue
        // des icônes ; l'icône créée est donnée à la fenêtre pour toute la
        // vie du processus (jamais détruite, comme le faisait eframe).
        let Ok(icon) = (unsafe { CreateIconFromResourceEx(&bytes, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }) else { return };
        // SAFETY: `hwnd` est la fenêtre vivante de cette application (donnée
        // par eframe à sa création, sur ce fil) ; `icon` vient d'être créée.
        unsafe {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize)));
        }
    }
}

#[cfg(not(windows))]
pub fn set(_cc: &eframe::CreationContext<'_>) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// Chaque réduction faite d'avance a les pixels que la chaîne d'eframe
    /// produit aujourd'hui (même version de `image`) : l'icône n'a pas changé.
    #[test]
    fn stored_sizes_match_the_runtime_scaling_pixel_for_pixel() {
        let source = image::load_from_memory(SOURCE).expect("icône source").into_rgba8();
        assert_eq!((source.width(), source.height()), (512, 512));
        for (size, bytes) in SCALED {
            let stored = image::load_from_memory(bytes).expect("réduction").into_rgba8();
            let fresh = image::imageops::resize(&source, *size as u32, *size as u32, image::imageops::Lanczos3);
            assert_eq!((stored.width(), stored.height()), (*size as u32, *size as u32));
            assert!(stored.as_raw() == fresh.as_raw(), "réduction {size} px différente de celle d'eframe");
        }
    }

    #[test]
    fn an_unusual_size_is_scaled_on_the_fly() {
        assert!(SCALED.iter().all(|(s, _)| *s != 36));
        let bytes = png(36).expect("réduction au vol");
        let img = image::load_from_memory(&bytes).expect("PNG").into_rgba8();
        assert_eq!((img.width(), img.height()), (36, 36));
        assert!(png(0).is_none() && png(-4).is_none());
    }

    #[test]
    fn eframe_is_told_to_leave_the_icon_alone() {
        let o = skip_default(eframe::NativeOptions::default());
        if cfg!(windows) {
            assert!(o.viewport.icon.as_deref().is_some_and(|i| *i == egui::IconData::default()), "icône vide attendue");
        } else {
            assert!(o.viewport.icon.is_none());
        }
    }
}
