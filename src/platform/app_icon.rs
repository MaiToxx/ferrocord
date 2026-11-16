//! Icône de l'application (lot « icône ») : le logo de Ferrocord
//! ([`crate::desk::icon`]) partout, à la place du « e » d'eframe.
//!
//! - `assets/icone/ferrocord.ico` est PRODUIT par ce module ([`ico`], test
//!   ignoré `genere_le_fichier_ico`) à partir du dessin du logo, dans la
//!   couleur d'accent du thème par défaut ; un test le refait en mémoire et
//!   le compare octet à octet : il ne peut pas dériver du logo.
//! - `build.rs` le range dans les ressources de l'exécutable
//!   (`platform/winres.rs`) : Explorateur, raccourcis, barre des tâches,
//!   « Applications installées ».
//! - [`set`] pose l'icône de la fenêtre (barre de titre, barre des tâches,
//!   Alt+Tab) en la prenant dans ces ressources : les pixels ne sont pas
//!   embarqués une seconde fois. Sans ressource (exemples, tests) ou pour
//!   une taille absente du fichier, le logo est dessiné au vol (quelques
//!   microsecondes : rien à décoder ni à réduire).
//!
//! eframe, lui, décodait son icône de 512 × 512 avant la fenêtre puis la
//! réduisait deux fois dans la première image (une douzaine de
//! millisecondes, lot « cpu ») : [`skip_default`] l'en empêche toujours.
//!
//! L'icône de la zone de notification reste à part : elle suit la couleur
//! d'accent du thème courant (`desk::system`).

use eframe::egui;

/// Côtés des images du fichier `.ico` : petite et grande icône de Windows
/// de 100 à 300 %, vignettes de l'Explorateur jusqu'à 256.
pub const SIZES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

/// Identifiant du groupe d'icônes dans les ressources de l'exécutable
/// (`winres::ID`, que `build.rs` écrit ; un test vérifie l'égalité).
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
const GROUP: usize = 1;

/// Couleur du logo hors de l'interface : l'accent du thème par défaut.
pub fn accent() -> [u8; 3] {
    let c = crate::theme::Preset::DEFAULT.tokens().colors.accent;
    [c.r(), c.g(), c.b()]
}

/// eframe ne décode ni ne pose son icône : [`set`] s'en charge. Sans effet hors Windows.
pub fn skip_default(mut options: eframe::NativeOptions) -> eframe::NativeOptions {
    if cfg!(windows) && options.viewport.icon.is_none() {
        // Icône vide : eframe la tient pour « celle par défaut, rien à faire », et winit n'en reçoit aucune.
        options.viewport.icon = Some(std::sync::Arc::new(egui::IconData::default()));
    }
    options
}

/// Image d'icône au format de Windows (DIB) : en-tête `BITMAPINFOHEADER` à
/// la hauteur doublée, pixels BGRA de bas en haut (alpha non prémultiplié),
/// puis masque d'un bit par pixel (1 : transparent) pour les vieux lecteurs.
/// C'est le contenu d'une entrée de `.ico` comme d'une ressource `RT_ICON`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn dib(side: u32, rgba: &[u8]) -> Vec<u8> {
    let mask_row = side.div_ceil(32) as usize * 4;
    let pixels = (side * side * 4) as usize;
    let mut out = Vec::with_capacity(40 + pixels + mask_row * side as usize);
    out.extend_from_slice(&40u32.to_le_bytes());
    out.extend_from_slice(&side.to_le_bytes());
    out.extend_from_slice(&(side * 2).to_le_bytes());
    // Un plan, 32 bits par pixel, sans compression.
    out.extend_from_slice(&[1, 0, 32, 0, 0, 0, 0, 0]);
    out.extend_from_slice(&((pixels + mask_row * side as usize) as u32).to_le_bytes());
    out.extend_from_slice(&[0; 16]);
    let rows = || rgba.chunks_exact(side as usize * 4).rev();
    for row in rows() {
        for p in row.as_chunks::<4>().0 {
            out.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
        }
    }
    for row in rows() {
        let mut bits = vec![0u8; mask_row];
        for (x, p) in row.as_chunks::<4>().0.iter().enumerate() {
            if p[3] == 0 {
                bits[x / 8] |= 0x80 >> (x % 8);
            }
        }
        out.extend_from_slice(&bits);
    }
    out
}

/// Le logo au côté `side`, prêt pour `CreateIconFromResourceEx`.
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
fn drawn(side: u32) -> Vec<u8> {
    dib(side, &crate::desk::icon::rgba(side, accent(), None))
}

/// Pose la grande puis la petite icône de la fenêtre créée par eframe
/// (barre de titre, barre des tâches, Alt+Tab).
#[cfg(windows)]
pub fn set(cc: &eframe::CreationContext<'_>) {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SendMessageW, ICON_BIG, ICON_SMALL, SM_CXICON, SM_CXSMICON, WM_SETICON};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    let Ok(handle) = cc.window_handle() else { return };
    let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
    let hwnd = HWND(h.hwnd.get() as *mut core::ffi::c_void);
    // La grande d'abord, puis la petite (ordre d'eframe : l'inverse donne une petite icône floue).
    for (metric, which) in [(SM_CXICON, ICON_BIG), (SM_CXSMICON, ICON_SMALL)] {
        // SAFETY: lecture d'une mesure du système, sans effet.
        let size = unsafe { GetSystemMetrics(metric) };
        let Some(icon) = system::from_resource(size).or_else(|| system::from_drawing(size)) else { return };
        // SAFETY: `hwnd` est la fenêtre vivante de cette application (donnée
        // par eframe à sa création, sur ce fil) ; `icon` vient d'être créée
        // et vit autant que le processus (jamais détruite, comme le faisait eframe).
        unsafe {
            SendMessageW(hwnd, WM_SETICON, Some(WPARAM(which as usize)), Some(LPARAM(icon.0 as isize)));
        }
    }
}

#[cfg(not(windows))]
pub fn set(_cc: &eframe::CreationContext<'_>) {}

#[cfg(windows)]
mod system {
    use windows::core::PCWSTR;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{CreateIconFromResourceEx, LoadImageW, HICON, IMAGE_ICON, LR_DEFAULTCOLOR};

    /// L'image de côté `size` du groupe d'icônes de l'exécutable. Seulement
    /// pour une taille que le fichier contient : sinon Windows étirerait la
    /// plus proche, et le dessin au vol est plus net.
    pub(super) fn from_resource(size: i32) -> Option<HICON> {
        if !super::SIZES.iter().any(|s| i32::try_from(*s) == Ok(size)) {
            return None;
        }
        // SAFETY: sans nom, la fonction rend le module de l'exécutable, chargé pour toute la vie du processus.
        let module = unsafe { GetModuleHandleW(None) }.ok()?;
        // Nom numérique (MAKEINTRESOURCE) : l'identifiant du groupe, pas un pointeur déréférencé.
        let name = PCWSTR(super::GROUP as *const u16);
        // SAFETY: `module` est valide ; `name` est un identifiant de
        // ressource, que la fonction ne lit pas comme une chaîne ; sans
        // LR_SHARED, l'icône rendue nous appartient.
        let handle = unsafe { LoadImageW(Some(module.into()), name, IMAGE_ICON, size, size, LR_DEFAULTCOLOR) }.ok()?;
        Some(HICON(handle.0))
    }

    /// Le logo dessiné au vol.
    pub(super) fn from_drawing(size: i32) -> Option<HICON> {
        let bytes = super::drawn(u32::try_from(size).ok().filter(|s| (1..=256).contains(s))?);
        // SAFETY: `bytes` est une image d'icône entière (DIB), vivante pendant
        // l'appel, que la fonction lit sans la garder ; 0x00030000 est la
        // version attendue des icônes.
        unsafe { CreateIconFromResourceEx(&bytes, true, 0x0003_0000, size, size, LR_DEFAULTCOLOR) }.ok()
    }
}

/// Fichier `.ico` du logo : une image par côté de [`SIZES`], 32 bits avec
/// alpha ; DIB jusqu'à 128 (lu par tous les outils), PNG pour 256 (ce
/// qu'attend Windows à cette taille).
#[cfg(test)]
fn ico() -> Vec<u8> {
    use image::ImageEncoder;
    let images: Vec<Vec<u8>> = SIZES
        .iter()
        .map(|&side| {
            let rgba = crate::desk::icon::rgba(side, accent(), None);
            if side < 256 {
                return dib(side, &rgba);
            }
            let mut png = Vec::new();
            let encoder = image::codecs::png::PngEncoder::new_with_quality(&mut png, image::codecs::png::CompressionType::Best, image::codecs::png::FilterType::Adaptive);
            encoder.write_image(&rgba, side, side, image::ExtendedColorType::Rgba8).expect("codage PNG");
            png
        })
        .collect();
    // ICONDIR : réservé, genre (1 : icône), nombre d'images.
    let mut out: Vec<u8> = [0, 1, SIZES.len() as u16].into_iter().flat_map(u16::to_le_bytes).collect();
    let mut offset = 6 + 16 * SIZES.len();
    for (&side, image) in SIZES.iter().zip(&images) {
        // Côté (0 : 256) deux fois, palette, réservé, un plan, 32 bits.
        let byte = if side == 256 { 0 } else { side as u8 };
        out.extend_from_slice(&[byte, byte, 0, 0, 1, 0, 32, 0]);
        out.extend_from_slice(&(image.len() as u32).to_le_bytes());
        out.extend_from_slice(&(offset as u32).to_le_bytes());
        offset += image.len();
    }
    for image in &images {
        out.extend_from_slice(image);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::winres;

    const FILE: &[u8] = include_bytes!("../../assets/icone/ferrocord.ico");

    /// Écrit `assets/icone/ferrocord.ico`. À relancer quand le logo ou
    /// l'accent par défaut changent :
    /// `cargo test --lib genere_le_fichier_ico -- --ignored`.
    #[test]
    #[ignore = "générateur : récrit assets/icone/ferrocord.ico"]
    fn genere_le_fichier_ico() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/icone/ferrocord.ico");
        std::fs::write(path, ico()).expect("écriture du fichier .ico");
    }

    #[test]
    fn committed_ico_is_the_logo_byte_for_byte() {
        assert!(FILE == ico().as_slice(), "assets/icone/ferrocord.ico ne correspond plus au logo : cargo test --lib genere_le_fichier_ico -- --ignored");
    }

    /// Pixels RGBA d'une image DIB d'icône, de haut en bas, et son masque.
    fn undib(data: &[u8], side: u32) -> (Vec<u8>, Vec<bool>) {
        let word = |at: usize| u32::from_le_bytes(data[at..at + 4].try_into().unwrap());
        assert_eq!((word(0), word(4), word(8)), (40, side, side * 2), "en-tête : taille, largeur, hauteur doublée");
        assert_eq!(&data[12..20], &[1, 0, 32, 0, 0, 0, 0, 0], "un plan, 32 bits, sans compression");
        let (n, mask_row) = (side as usize, side.div_ceil(32) as usize * 4);
        assert_eq!(data.len(), 40 + n * n * 4 + mask_row * n);
        let (mut rgba, mut mask) = (Vec::new(), Vec::new());
        for y in (0..n).rev() {
            for x in 0..n {
                let p = &data[40 + (y * n + x) * 4..][..4];
                rgba.extend_from_slice(&[p[2], p[1], p[0], p[3]]);
                mask.push(data[40 + n * n * 4 + y * mask_row + x / 8] & (0x80 >> (x % 8)) != 0);
            }
        }
        (rgba, mask)
    }

    #[test]
    fn ico_holds_every_size_with_the_pixels_of_the_logo() {
        let images = winres::images(FILE).expect("fichier .ico");
        assert_eq!(images.iter().map(winres::Image::side).collect::<Vec<_>>(), SIZES);
        for image in &images {
            let side = image.side();
            let logo = crate::desk::icon::rgba(side, accent(), None);
            assert_eq!(&image.entry[2..8], &[0, 0, 1, 0, 32, 0], "entrée {side} : un plan, 32 bits");
            if side == 256 {
                assert!(image.data.starts_with(b"\x89PNG\r\n\x1a\n"), "256 px en PNG");
                let decoded = image::load_from_memory(image.data).expect("PNG").into_rgba8();
                assert_eq!((decoded.width(), decoded.height()), (256, 256));
                assert!(decoded.as_raw() == &logo, "pixels de l'image de 256 px");
            } else {
                let (rgba, mask) = undib(image.data, side);
                assert!(rgba == logo, "pixels de l'image de {side} px");
                // Le masque des vieux lecteurs : transparent là où l'alpha est nul, et seulement là.
                assert!(mask.iter().zip(logo.as_chunks::<4>().0).all(|(m, p)| *m == (p[3] == 0)), "masque de l'image de {side} px");
            }
        }
    }

    #[test]
    fn logo_uses_the_default_accent_and_no_badge() {
        assert_eq!(GROUP, usize::from(winres::ID));
        assert_eq!(accent(), [0x58, 0x65, 0xf2], "accent du thème sombre par défaut");
        let images = winres::images(FILE).expect("fichier .ico");
        let (rgba, _) = undib(images[5].data, 48);
        let at = |x: usize, y: usize| &rgba[(y * 48 + x) * 4..][..4];
        assert_eq!(at(24, 24), &[0x58, 0x65, 0xf2, 255], "centre : accent plein");
        assert_eq!(at(0, 0)[3], 0, "coin transparent");
        assert_eq!(at(36, 36), &[0x58, 0x65, 0xf2, 255], "pas de pastille dans le coin");
    }

    #[test]
    fn a_size_missing_from_the_file_is_drawn_on_the_fly() {
        // Petite icône à 175 % : 28 px, absente du fichier.
        assert!(!SIZES.contains(&28));
        let (rgba, _) = undib(&drawn(28), 28);
        assert!(rgba == crate::desk::icon::rgba(28, accent(), None));
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
