//! Images animées (GIF, WebP, APNG ; WebM d'une plaque, voir `webm.rs`) :
//! toutes les images d'une animation,
//! décodées et ramenées à la taille d'affichage, toujours hors du fil
//! d'interface (fils de décodage de l'ordonnanceur). Mêmes règles que
//! `decode` : format reconnu au contenu, dimensions vérifiées avant toute
//! allocation, panique du décodeur changée en erreur.
//!
//! Bornes : au plus [`MAX_FRAMES`] images et [`MAX_ANIM_BYTES`] octets de
//! pixels par animation (au-delà, l'animation s'arrête à la dernière image
//! qui tient) ; le total est borné par le plafond commun du cache de
//! textures, où chaque animation compte pour tous ses octets.

use super::decode::{self, DecodeError, Fit};
use eframe::egui::ColorImage;
use image::{codecs, imageops::FilterType, AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat};
use std::io::Cursor;
use std::time::Duration;

/// Images gardées par animation, au plus.
pub const MAX_FRAMES: usize = 240;
/// Octets de pixels (RVBA) par animation, au plus.
pub const MAX_ANIM_BYTES: usize = 12 << 20;
/// Pixels par image d'une animation recadrée (bannière), au moins : 48 000
/// pixels font 192 Ko par image, soit une soixantaine d'images dans
/// [`MAX_ANIM_BYTES`]. Une boucle plus longue garde cette taille et saute des
/// images ; une plus courte est plus nette (voir `fill_plan`).
pub const FILL_FLOOR_PIXELS: u32 = 48_000;
/// Délai d'une image en dessous duquel les navigateurs (et Discord) jouent
/// l'image à 100 ms : un GIF à 0 ou 10 ms ne doit pas tourner à toute vitesse.
const MIN_DELAY: Duration = Duration::from_millis(20);
const SLOW_DELAY: Duration = Duration::from_millis(100);

/// Une animation prête : chaque image avec sa durée d'affichage.
pub type Frames = Vec<(ColorImage, Duration)>;

/// Les octets sont-ils ceux d'une image ou d'une animation lisible ici
/// (format reconnu au contenu) ?
pub fn readable(bytes: &[u8]) -> bool {
    decode::sniff(bytes).is_some() || super::webm::is_webm(bytes)
}

/// L'image contient-elle plus d'une image (GIF, WebP ou APNG) ? Lit au plus
/// deux images ; toute erreur vaut « non ».
pub fn is_animated(bytes: &[u8]) -> bool {
    crate::panics::contain(|| frames_iter(bytes, decode::MAX_SIDE, Fit::Cover(1), 2).map(|f| f.len() > 1).unwrap_or(false)).unwrap_or(false)
}

/// Toutes les images de l'animation, ajustées à `fit` ; une image fixe
/// donne une seule image. Aucun côté de la source ne dépasse `max_side`.
pub fn decode_frames(bytes: &[u8], max_side: u32, fit: Fit) -> Result<Frames, DecodeError> {
    #[cfg(test)]
    if bytes == decode::PANIC_TRIGGER {
        return crate::panics::contain(|| -> Result<Frames, DecodeError> { panic!("décodeur de test") }).unwrap_or_else(|| Err(DecodeError::Corrupt("panique du décodeur".into())));
    }
    crate::panics::contain(|| frames_iter(bytes, max_side, fit, MAX_FRAMES)).unwrap_or_else(|| Err(DecodeError::Corrupt("panique du décodeur".into())))
}

fn classify(e: &image::ImageError) -> DecodeError {
    match e {
        image::ImageError::Limits(_) => DecodeError::TooLarge { width: 0, height: 0 },
        image::ImageError::Unsupported(_) => DecodeError::Unsupported,
        other => DecodeError::Corrupt(other.to_string()),
    }
}

fn limits(max_side: u32) -> image::Limits {
    let mut l = image::Limits::default();
    l.max_image_width = Some(max_side);
    l.max_image_height = Some(max_side);
    l.max_alloc = Some(u64::from(max_side) * u64::from(max_side) * 4 + (8 << 20));
    l
}

fn check(d: &impl ImageDecoder, max_side: u32) -> Result<(), DecodeError> {
    let (width, height) = d.dimensions();
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return Err(DecodeError::TooLarge { width, height });
    }
    Ok(())
}

/// Côtés d'une image `w` × `h` ajustée à `fit` (comme `decode::fit`).
fn fitted_size(w: u32, h: u32, fit: Fit) -> (u32, u32) {
    match fit {
        Fit::Cover(s) => {
            let s = s.clamp(1, decode::MAX_SIDE);
            (s, s)
        }
        Fit::Fill(w, h) => (w.clamp(1, decode::MAX_SIDE), h.clamp(1, decode::MAX_SIDE)),
        Fit::Contain(mw, mh) => {
            let (mw, mh) = (mw.clamp(1, decode::MAX_SIDE), mh.clamp(1, decode::MAX_SIDE));
            if w <= mw && h <= mh {
                return (w, h);
            }
            let scale = (f64::from(mw) / f64::from(w)).min(f64::from(mh) / f64::from(h));
            (((f64::from(w) * scale).round() as u32).max(1), ((f64::from(h) * scale).round() as u32).max(1))
        }
    }
}

/// Ajustement d'une image de l'animation : filtre plus rapide que celui
/// des images fixes, une animation pouvant en compter des centaines.
pub(super) fn fit_frame(img: DynamicImage, fit: Fit) -> ColorImage {
    let rgba = fit_pixels(img, fit);
    ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
}

/// [`fit_frame`] pour des pixels dont la couleur est déjà multipliée par
/// l'alpha (image vidéo composée avec son alpha, voir `webm.rs`) : ajustée
/// telle quelle, pour que la couleur des zones transparentes ne bave pas sur
/// leurs voisines.
pub(super) fn fit_frame_premultiplied(img: DynamicImage, fit: Fit) -> ColorImage {
    let rgba = fit_pixels(img, fit);
    ColorImage::from_rgba_premultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
}

fn fit_pixels(img: DynamicImage, fit: Fit) -> image::RgbaImage {
    let (w, h) = fitted_size(img.width(), img.height(), fit);
    let out = if (img.width(), img.height()) == (w, h) {
        img
    } else if matches!(fit, Fit::Cover(_) | Fit::Fill(..)) {
        img.resize_to_fill(w, h, FilterType::Triangle)
    } else {
        img.resize_exact(w, h, FilterType::Triangle)
    };
    out.into_rgba8()
}

fn delay(d: image::Delay) -> Duration {
    let (n, den) = d.numer_denom_ms();
    let ms = if den == 0 { 0 } else { u64::from(n) / u64::from(den) };
    let d = Duration::from_millis(ms);
    if d < MIN_DELAY { SLOW_DELAY } else { d.min(Duration::from_secs(60)) }
}

/// Taille des images et pas de lecture d'une animation recadrée
/// (`Fit::Fill`, bannière de profil) de `count` images, pour qu'elle tienne
/// entière dans `budget` octets ([`MAX_ANIM_BYTES`] pour une image animée) :
/// à la taille demandée si elle y tient
/// (jamais plus grande que la source : agrandir ne rend rien plus net),
/// sinon réduite, sans descendre sous `floor_pixels` pixels
/// ([`FILL_FLOOR_PIXELS`] pour une bannière) ; en dessous,
/// une image sur `step` est gardée (sa durée couvre celles qui sont sautées)
/// plutôt que de couper la boucle avant sa fin.
pub(super) fn fill_plan(budget: usize, floor_pixels: u32, count: usize, (w, h): (u32, u32), (src_w, src_h): (u32, u32)) -> (Fit, usize) {
    let (w, h) = (f64::from(w.clamp(1, decode::MAX_SIDE)), f64::from(h.clamp(1, decode::MAX_SIDE)));
    let native = (f64::from(src_w.max(1)) / w).min(f64::from(src_h.max(1)) / h).min(1.0);
    let (w, h) = (w * native, h * native);
    let full = w * h;
    let floor = f64::from(floor_pixels).min(full);
    let count = count.max(1);
    let pixels = (budget as f64 / 4.0 / count as f64).clamp(floor, full);
    let scale = (pixels / full).sqrt();
    // Arrondi par défaut (à un epsilon près) : arrondie au-dessus, la taille
    // dépasserait le budget d'une image et la boucle en perdrait la moitié.
    let side = |v: f64| ((v * scale + 1e-6).floor() as u32).clamp(1, decode::MAX_SIDE);
    let (fw, fh) = (side(w), side(h));
    let kept = (budget / (fw as usize * fh as usize * 4)).max(1);
    (Fit::Fill(fw, fh), count.div_ceil(kept).max(1))
}

/// Images de l'animation (au plus `max`), en s'arrêtant à la dernière qui
/// tient dans [`MAX_ANIM_BYTES`]. `count` : nombre d'images de l'animation,
/// compté d'avance pour une animation recadrée (voir [`fill_plan`]).
fn collect<'a>(frames: image::Frames<'a>, fit: Fit, max: usize, count: Option<usize>) -> Result<Frames, DecodeError> {
    let mut out: Frames = Vec::new();
    let mut bytes = 0usize;
    let (mut fit, mut step) = (fit, 1);
    for (i, f) in frames.take(max).enumerate() {
        let f = match f {
            Ok(f) => f,
            // Fichier tronqué : les images déjà lues sont gardées.
            Err(_) if !out.is_empty() => break,
            Err(e) => return Err(classify(&e)),
        };
        let d = delay(f.delay());
        let buf = f.into_buffer();
        if i == 0
            && let (Fit::Fill(w, h), Some(n)) = (fit, count)
        {
            (fit, step) = fill_plan(MAX_ANIM_BYTES, FILL_FLOOR_PIXELS, n, (w, h), (buf.width(), buf.height()));
        }
        // Image sautée : celle d'avant reste affichée pendant sa durée aussi.
        if i % step != 0
            && let Some((_, last)) = out.last_mut()
        {
            *last = (*last + d).min(Duration::from_secs(60));
            continue;
        }
        let img = fit_frame(DynamicImage::ImageRgba8(buf), fit);
        let size = img.size[0] * img.size[1] * 4;
        if !out.is_empty() && bytes + size > MAX_ANIM_BYTES {
            break;
        }
        bytes += size;
        out.push((img, d));
    }
    if out.is_empty() {
        return Err(DecodeError::Corrupt("aucune image".into()));
    }
    Ok(out)
}

fn frames_iter(bytes: &[u8], max_side: u32, fit: Fit, max: usize) -> Result<Frames, DecodeError> {
    let max_side = max_side.clamp(1, decode::MAX_SIDE);
    // Plaque animée : un WebM, décodé par Windows (voir `webm.rs`).
    if super::webm::is_webm(bytes) {
        return super::webm::frames(bytes, max_side, fit, max);
    }
    let still = || -> Result<Frames, DecodeError> {
        let img = decode::decode(bytes, max_side)?;
        Ok(vec![(fit_frame(img, fit), SLOW_DELAY)])
    };
    let Some(frames) = open(bytes, max_side)? else { return still() };
    // Animation recadrée : ses images sont comptées d'abord (une passe de
    // décodage de plus, sans rien garder), pour choisir la taille qui la fait
    // tenir entière. Inutile si la taille demandée tient de toute façon.
    let count = match fit {
        Fit::Fill(w, h) if fitted_bytes(w, h).saturating_mul(max) > MAX_ANIM_BYTES => open(bytes, max_side)?.map(|f| f.take(max).take_while(Result::is_ok).count()),
        _ => None,
    };
    collect(frames, fit, max, count)
}

fn fitted_bytes(w: u32, h: u32) -> usize {
    w.clamp(1, decode::MAX_SIDE) as usize * h.clamp(1, decode::MAX_SIDE) as usize * 4
}

/// Images de l'animation, dimensions vérifiées ; `None` : image fixe (ou
/// format sans animation).
fn open(bytes: &[u8], max_side: u32) -> Result<Option<image::Frames<'_>>, DecodeError> {
    Ok(Some(match decode::sniff(bytes).ok_or(DecodeError::Unsupported)? {
        ImageFormat::Gif => {
            let mut d = codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| classify(&e))?;
            d.set_limits(limits(max_side)).map_err(|e| classify(&e))?;
            check(&d, max_side)?;
            d.into_frames()
        }
        ImageFormat::WebP => {
            let mut d = codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| classify(&e))?;
            if !d.has_animation() {
                return Ok(None);
            }
            d.set_limits(limits(max_side)).map_err(|e| classify(&e))?;
            check(&d, max_side)?;
            d.into_frames()
        }
        ImageFormat::Png => {
            let mut d = codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(|e| classify(&e))?;
            if !d.is_apng().map_err(|e| classify(&e))? {
                return Ok(None);
            }
            d.set_limits(limits(max_side)).map_err(|e| classify(&e))?;
            check(&d, max_side)?;
            d.apng().map_err(|e| classify(&e))?.into_frames()
        }
        _ => return Ok(None),
    }))
}

/// Décodage **en flux** (lot ND, effets de profil) : chaque image de
/// l'animation est décodée, ramenée à `size` pixels (étirée, sans recadrage)
/// et remise à `each` avec sa durée, sans qu'aucune autre ne soit gardée.
/// `each` rend `false` pour arrêter. Une image fixe donne une seule image.
/// Rend le nombre d'images remises. La mémoire tenue est celle du décodeur
/// (son canevas) et d'une image à `size`, quelle que soit la longueur de
/// l'animation.
pub fn stream_frames(bytes: &[u8], max_side: u32, size: (u32, u32), mut each: impl FnMut(ColorImage, Duration) -> bool) -> Result<usize, DecodeError> {
    let max_side = max_side.clamp(1, decode::MAX_SIDE);
    let fit = Fit::Fill(size.0.clamp(1, decode::MAX_SIDE), size.1.clamp(1, decode::MAX_SIDE));
    let scaled = |img: DynamicImage| {
        let Fit::Fill(w, h) = fit else { unreachable!() };
        let out = if (img.width(), img.height()) == (w, h) { img } else { img.resize_exact(w, h, FilterType::Triangle) };
        let rgba = out.into_rgba8();
        ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw())
    };
    let run = |frames: image::Frames<'_>, each: &mut dyn FnMut(ColorImage, Duration) -> bool| -> Result<usize, DecodeError> {
        let mut n = 0;
        for f in frames {
            let f = match f {
                Ok(f) => f,
                // Fichier tronqué : ce qui a été lu a été montré.
                Err(_) if n > 0 => break,
                Err(e) => return Err(classify(&e)),
            };
            let d = delay(f.delay());
            n += 1;
            if !each(scaled(DynamicImage::ImageRgba8(f.into_buffer())), d) {
                break;
            }
        }
        Ok(n)
    };
    crate::panics::contain(|| match decode::sniff(bytes).ok_or(DecodeError::Unsupported)? {
        ImageFormat::Png => {
            let mut d = codecs::png::PngDecoder::new(Cursor::new(bytes)).map_err(|e| classify(&e))?;
            d.set_limits(limits(max_side)).map_err(|e| classify(&e))?;
            check(&d, max_side)?;
            if d.is_apng().map_err(|e| classify(&e))? {
                return run(d.apng().map_err(|e| classify(&e))?.into_frames(), &mut each);
            }
            let img = DynamicImage::from_decoder(d).map_err(|e| classify(&e))?;
            each(scaled(img), SLOW_DELAY);
            Ok(1)
        }
        ImageFormat::WebP => {
            let mut d = codecs::webp::WebPDecoder::new(Cursor::new(bytes)).map_err(|e| classify(&e))?;
            d.set_limits(limits(max_side)).map_err(|e| classify(&e))?;
            check(&d, max_side)?;
            if d.has_animation() {
                return run(d.into_frames(), &mut each);
            }
            let img = DynamicImage::from_decoder(d).map_err(|e| classify(&e))?;
            each(scaled(img), SLOW_DELAY);
            Ok(1)
        }
        ImageFormat::Gif => {
            let mut d = codecs::gif::GifDecoder::new(Cursor::new(bytes)).map_err(|e| classify(&e))?;
            d.set_limits(limits(max_side)).map_err(|e| classify(&e))?;
            check(&d, max_side)?;
            run(d.into_frames(), &mut each)
        }
        _ => Err(DecodeError::Unsupported),
    })
    .unwrap_or_else(|| Err(DecodeError::Corrupt("panique du décodeur".into())))
}

/// Lecture d'une animation déjà décodée : image affichée et échéance de la
/// suivante. Calcul seul (l'instant est un paramètre).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Player {
    pub shown: usize,
    pub next_at: Option<std::time::Instant>,
}

/// Au-delà de ce retard, l'animation n'a pas été affichée entre-temps : elle
/// reprend où elle en était au lieu de sauter les images manquées.
const STALE: Duration = Duration::from_millis(500);

impl Player {
    pub fn new() -> Self {
        Player { shown: 0, next_at: None }
    }

    /// Avance jusqu'à l'image due à `now` ; `true` si l'image affichée change.
    /// Une animation d'une seule image n'a jamais d'échéance.
    pub fn advance(&mut self, delays: &[Duration], now: std::time::Instant) -> bool {
        let n = delays.len();
        if n < 2 {
            self.next_at = None;
            return false;
        }
        let Some(mut at) = self.next_at else {
            self.next_at = Some(now + delays[self.shown % n]);
            return false;
        };
        if now < at {
            return false;
        }
        if now.duration_since(at) > STALE {
            self.shown = (self.shown + 1) % n;
            self.next_at = Some(now + delays[self.shown]);
            return true;
        }
        let before = self.shown;
        // Au plus un tour : une image ne peut pas être sautée plus d'une fois par passage.
        for _ in 0..n {
            if now < at {
                break;
            }
            self.shown = (self.shown + 1) % n;
            at += delays[self.shown];
        }
        if now >= at {
            at = now + delays[self.shown];
        }
        self.next_at = Some(at);
        self.shown != before
    }

    /// Animation remise au début (affichée de nouveau après une pause).
    pub fn rewind(&mut self) {
        *self = Player::new();
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use image::{codecs::gif::GifEncoder, Delay, Frame, RgbaImage};
    use std::time::Instant;

    /// GIF animé de `n` images de `side` pixels, 50 ms chacune.
    pub(crate) fn gif(n: usize, side: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut out);
            let frames = (0..n).map(|i| Frame::from_parts(RgbaImage::from_pixel(side, side, image::Rgba([(i * 40) as u8, 100, 200, 255])), 0, 0, Delay::from_numer_denom_ms(50, 1)));
            enc.encode_frames(frames).unwrap_or_else(|e| panic!("{e}"));
        }
        out
    }

    /// GIF animé de `n` images de `w` × `h` pixels (une bannière), 50 ms chacune.
    pub(crate) fn gif_sized(n: usize, w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        {
            // Encodage rapide : seules les dimensions et le nombre d'images comptent ici.
            let mut enc = GifEncoder::new_with_speed(&mut out, 30);
            let frames = (0..n).map(|i| Frame::from_parts(RgbaImage::from_pixel(w, h, image::Rgba([(i * 7) as u8, 100, 200, 255])), 0, 0, Delay::from_numer_denom_ms(50, 1)));
            enc.encode_frames(frames).unwrap_or_else(|e| panic!("{e}"));
        }
        out
    }

    /// Bannière animée (profil complet) : une boucle courte est décodée à la
    /// taille demandée, donc nette ; une longue est réduite, puis allégée
    /// d'une image sur deux ou trois, mais jouée jusqu'au bout et à la même
    /// durée, toujours dans le plafond d'une animation.
    #[test]
    fn filled_animations_fit_whole_in_the_budget() {
        let bytes = |f: &Frames| f.iter().map(|(i, _)| i.size[0] * i.size[1] * 4).sum::<usize>();
        let total = |f: &Frames| f.iter().map(|(_, d)| *d).sum::<Duration>();
        let want = Fit::Fill(512, 179);
        // 12 images de 366 Ko : tout tient à la taille demandée.
        let short = decode_frames(&gif_sized(12, 512, 192), 1024, want).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(short.len(), 12);
        assert!(short.iter().all(|(i, _)| i.size == [512, 179]), "{:?}", short[0].0.size);
        // 60 images : réduites juste ce qu'il faut, aucune sautée.
        let medium = decode_frames(&gif_sized(60, 512, 192), 1024, want).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(medium.len(), 60);
        let [w, h] = medium[0].0.size;
        assert!(w < 512 && w * h >= FILL_FLOOR_PIXELS as usize, "{w} × {h}");
        assert!(bytes(&medium) <= MAX_ANIM_BYTES && bytes(&medium) > MAX_ANIM_BYTES * 9 / 10, "{}", bytes(&medium));
        // 150 images : au plancher de netteté, une image sur trois, la boucle entière.
        let long = decode_frames(&gif_sized(150, 512, 192), 1024, want).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(long.len(), 50);
        assert!(long.iter().all(|(i, _)| i.size == long[0].0.size && i.size[0] * i.size[1] >= FILL_FLOOR_PIXELS as usize * 99 / 100));
        assert_eq!(total(&long), Duration::from_millis(150 * 50), "durée de la boucle gardée");
        assert!(bytes(&long) <= MAX_ANIM_BYTES);
        // Source plus petite que la place : pas agrandie (la carte graphique s'en charge).
        let small = decode_frames(&gif_sized(4, 256, 96), 1024, want).unwrap_or_else(|e| panic!("{e}"));
        assert!(small.iter().all(|(i, _)| i.size[0] <= 256 && i.size[1] <= 96), "{:?}", small[0].0.size);
        // Les proportions demandées sont gardées (à l'arrondi près) : rien n'est déformé à l'affichage.
        for f in [&short, &medium, &long, &small] {
            let [w, h] = f[0].0.size;
            assert!((w as f32 / h as f32 - 512.0 / 179.0).abs() < 0.05, "{w} × {h}");
        }
    }

    #[test]
    fn gif_frames_are_decoded_and_fitted() {
        let g = gif(4, 32);
        assert!(is_animated(&g));
        let f = decode_frames(&g, 256, Fit::Cover(16)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.len(), 4);
        assert!(f.iter().all(|(img, d)| img.size == [16, 16] && *d == Duration::from_millis(50)));
        assert_ne!(f[0].0.pixels[0], f[1].0.pixels[0], "images différentes");
        // Une seule image : pas animée, mais décodable.
        assert!(!is_animated(&gif(1, 8)));
        assert_eq!(decode_frames(&gif(1, 8), 256, Fit::Contain(4, 4)).map(|f| f.len()), Ok(1));
    }

    #[test]
    fn still_images_and_limits() {
        let mut png = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::new(40, 20)).write_to(&mut png, ImageFormat::Png).unwrap_or_else(|e| panic!("{e}"));
        assert!(!is_animated(png.get_ref()));
        assert_eq!(decode_frames(png.get_ref(), 256, Fit::Contain(20, 20)).map(|f| f[0].0.size), Ok([20, 10]));
        assert!(matches!(decode_frames(&gif(2, 300), 256, Fit::Cover(8)), Err(DecodeError::TooLarge { .. })));
        assert_eq!(decode_frames(b"rien", 256, Fit::Cover(8)), Err(DecodeError::Unsupported));
        assert!(matches!(decode_frames(decode::PANIC_TRIGGER, 256, Fit::Cover(8)), Err(DecodeError::Corrupt(_))));
        let g = gif(3, 16);
        assert!(decode_frames(&g[..g.len() / 2], 256, Fit::Cover(8)).is_ok() || decode_frames(&g[..g.len() / 2], 256, Fit::Cover(8)).is_err(), "sans panique");
    }

    /// Flux : chaque image passe une fois, à la taille voulue, et l'appelant
    /// peut arrêter ; rien n'est gardé entre deux images.
    #[test]
    fn stream_hands_out_frames_one_at_a_time() {
        let g = gif(300, 64);
        let mut seen = Vec::new();
        let n = stream_frames(&g, 256, (20, 40), |img, d| {
            seen.push((img.size, d));
            true
        });
        assert_eq!(n, Ok(300), "aucun plafond de longueur : rien n'est accumulé");
        assert!(seen.iter().all(|(s, d)| *s == [20, 40] && *d == Duration::from_millis(50)));
        let mut count = 0;
        let n = stream_frames(&g, 256, (8, 8), |_, _| {
            count += 1;
            count < 3
        });
        assert_eq!((n, count), (Ok(3), 3), "arrêt demandé par l'appelant");
        // Image fixe : une seule image ; trop grande, illisible ou piégée : erreur sans panique.
        let mut png = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(RgbaImage::new(40, 20)).write_to(&mut png, ImageFormat::Png).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(stream_frames(png.get_ref(), 256, (10, 10), |img, _| img.size == [10, 10]), Ok(1));
        assert!(matches!(stream_frames(&gif(2, 300), 256, (8, 8), |_, _| true), Err(DecodeError::TooLarge { .. })));
        assert_eq!(stream_frames(b"rien", 256, (8, 8), |_, _| true), Err(DecodeError::Unsupported));
    }

    #[test]
    fn memory_per_animation_is_bounded() {
        // 300 images de 256 × 256 : 256 Kio chacune, 12 Mio en gardent 48.
        let g = gif(300, 256);
        let f = decode_frames(&g, 512, Fit::Cover(256)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f.len(), MAX_ANIM_BYTES / (256 * 256 * 4));
        let small = decode_frames(&gif(300, 4), 512, Fit::Cover(4)).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(small.len(), MAX_FRAMES);
    }

    #[test]
    fn player_follows_delays_and_resumes() {
        let d = [Duration::from_millis(100), Duration::from_millis(50), Duration::from_millis(200)];
        let t0 = Instant::now();
        let mut p = Player::new();
        assert!(!p.advance(&d, t0), "premier affichage : rien ne change");
        assert_eq!(p.next_at, Some(t0 + d[0]));
        assert!(!p.advance(&d, t0 + Duration::from_millis(99)));
        assert!(p.advance(&d, t0 + Duration::from_millis(100)));
        assert_eq!((p.shown, p.next_at), (1, Some(t0 + Duration::from_millis(150))));
        // Image en retard de peu : les images manquées sont rattrapées.
        assert!(p.advance(&d, t0 + Duration::from_millis(360)));
        assert_eq!((p.shown, p.next_at), (0, Some(t0 + Duration::from_millis(450))));
        // Longtemps non affichée : reprise à l'image suivante, sans saut.
        assert!(p.advance(&d, t0 + Duration::from_secs(10)));
        assert_eq!((p.shown, p.next_at), (1, Some(t0 + Duration::from_secs(10) + d[1])));
        let mut still = Player::new();
        assert!(!still.advance(&d[..1], t0 + Duration::from_secs(1)));
        assert_eq!(still.next_at, None, "une seule image : jamais de réveil");
        p.rewind();
        assert_eq!(p, Player::new());
    }

    #[test]
    fn tiny_delays_are_slowed() {
        assert_eq!(delay(image::Delay::from_numer_denom_ms(0, 1)), SLOW_DELAY);
        assert_eq!(delay(image::Delay::from_numer_denom_ms(10, 1)), SLOW_DELAY);
        assert_eq!(delay(image::Delay::from_numer_denom_ms(30, 1)), Duration::from_millis(30));
    }
}
