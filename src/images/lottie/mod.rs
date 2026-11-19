//! Autocollants Lottie (F17) : l'animation vectorielle d'un autocollant
//! (`stickers/{id}.json`, format de Bodymovin) changée en images, comme un
//! GIF. Lecture dans `model.rs`, dessin dans `draw.rs`, toujours sur un fil
//! de décodage de l'ordonnanceur.
//!
//! L'animation est dessinée directement à la taille d'affichage : nette à
//! toute échelle. Bornes : au plus [`anim::MAX_FRAMES`] images et
//! [`anim::MAX_ANIM_BYTES`] octets de pixels. Une animation trop longue
//! pour tenir est rendue à moins d'images par seconde (jusqu'à
//! [`MIN_FPS`]), puis en plus petit.

mod draw;
mod model;

use super::anim::{self, Frames};
use super::decode::{self, DecodeError, Fit};
use eframe::egui::ColorImage;
use std::time::Duration;

/// Taille d'un fichier Lottie, au plus (ceux de Discord font moins de 500 Ko).
const MAX_JSON: usize = 4 << 20;
/// Images par seconde rendues, au plus et au moins.
const MAX_FPS: f64 = 30.0;
const MIN_FPS: f64 = 12.0;

/// Les octets ont l'air d'une animation Lottie : un objet JSON qui a des
/// calques. Vérifié sur le début seulement ; `frames` fait la vraie lecture.
pub fn is_lottie(bytes: &[u8]) -> bool {
    let start = bytes.strip_prefix(b"\xef\xbb\xbf").unwrap_or(bytes);
    let start = &start[..start.len().min(64)];
    start.iter().find(|b| !b.is_ascii_whitespace()) == Some(&b'{') && bytes.len() <= MAX_JSON && bytes.windows(8).any(|w| w == b"\"layers\"")
}

/// Côtés rendus pour `fit` : l'animation garde ses proportions et remplit
/// la place demandée (un dessin vectoriel s'agrandit sans perte).
fn target(w: f64, h: f64, fit: Fit) -> (u32, u32) {
    let side = |v: u32| v.clamp(1, decode::MAX_SIDE);
    match fit {
        Fit::Cover(s) => (side(s), side(s)),
        Fit::Fill(fw, fh) => (side(fw), side(fh)),
        Fit::Contain(mw, mh) => {
            let scale = (f64::from(side(mw)) / w).min(f64::from(side(mh)) / h);
            (side((w * scale).round() as u32), side((h * scale).round() as u32))
        }
    }
}

/// Nombre d'images et taille qui tiennent dans le budget : d'abord moins
/// d'images par seconde, puis une image plus petite.
fn plan(seconds: f64, fps: f64, (w, h): (u32, u32), max: usize) -> (usize, (u32, u32)) {
    let max = max.clamp(1, anim::MAX_FRAMES);
    let wanted = ((seconds * fps.min(MAX_FPS)).round() as usize).clamp(1, max);
    let least = ((seconds * fps.min(MIN_FPS)).round() as usize).clamp(1, wanted);
    let bytes = |w: u32, h: u32| w as usize * h as usize * 4;
    let fits = (anim::MAX_ANIM_BYTES / bytes(w, h).max(1)).max(1);
    if wanted <= fits {
        return (wanted, (w, h));
    }
    if least <= fits {
        return (fits, (w, h));
    }
    // Encore trop : la surface est réduite pour que `least` images tiennent.
    let scale = (anim::MAX_ANIM_BYTES as f64 / (least * bytes(w, h)) as f64).sqrt();
    let shrink = |v: u32| ((f64::from(v) * scale).floor() as u32).max(1);
    (least, (shrink(w), shrink(h)))
}

/// Images de l'animation Lottie `bytes`, dessinées à la taille de `fit`.
pub fn frames(bytes: &[u8], fit: Fit, max: usize) -> Result<Frames, DecodeError> {
    if bytes.len() > MAX_JSON {
        return Err(DecodeError::TooLarge { width: 0, height: 0 });
    }
    let comp = model::parse(bytes).ok_or_else(|| DecodeError::Corrupt("animation Lottie illisible".into()))?;
    let seconds = (comp.op - comp.ip) / comp.fr;
    let (count, (w, h)) = plan(seconds, comp.fr, target(comp.w, comp.h, fit), max);
    let (w16, h16) = (u16::try_from(w).map_err(|_| DecodeError::TooLarge { width: w, height: h })?, u16::try_from(h).map_err(|_| DecodeError::TooLarge { width: w, height: h })?);
    let mut renderer = draw::Renderer::new(&comp, w16, h16);
    let delay = Duration::from_secs_f64((seconds / count as f64).clamp(0.008, 60.0));
    let step = (comp.op - comp.ip) / count as f64;
    let mut out: Frames = Vec::with_capacity(count);
    for k in 0..count {
        let rgba = renderer.frame(comp.ip + step * k as f64);
        out.push((ColorImage::from_rgba_premultiplied([w as usize, h as usize], &rgba), delay));
    }
    Ok(out)
}

/// Côté de l'image fixe, au plus : elle est ensuite ramenée à la taille d'affichage.
const STILL_SIDE: u32 = 512;

/// Première image de l'animation, pour l'affichage au repos.
pub fn still(bytes: &[u8], max_side: u32) -> Result<image::DynamicImage, DecodeError> {
    let side = max_side.clamp(1, STILL_SIDE);
    let mut f = frames(bytes, Fit::Contain(side, side), 1)?;
    let (img, _) = f.pop().ok_or_else(|| DecodeError::Corrupt("aucune image".into()))?;
    let [w, h] = img.size;
    // egui garde la couleur multipliée par l'alpha ; `image` l'attend droite.
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|p| p.to_srgba_unmultiplied()).collect();
    image::RgbaImage::from_raw(w as u32, h as u32, rgba).map(image::DynamicImage::ImageRgba8).ok_or_else(|| DecodeError::Corrupt("image incohérente".into()))
}

#[cfg(test)]
mod tests;
