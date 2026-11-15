//! Décodage d'images reçues du réseau ou lues sur le disque, toujours hors
//! du fil d'interface. Les données sont hostiles par principe : format
//! vérifié d'après le contenu, dimensions vérifiées avant toute allocation
//! (bombe de décompression), et toute erreur devient un `Err`, jamais une
//! panique.

use eframe::egui::ColorImage;
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::io::Cursor;

/// Côté maximal absolu (largeur ou hauteur) d'une image source.
pub const MAX_SIDE: u32 = 4096;
/// Côté accepté au moins, quelle que soit la taille demandée (une petite
/// icône peut être servie un peu plus grande que demandé).
pub const MIN_ACCEPTED_SIDE: u32 = 256;

/// Octets qui font paniquer le décodeur dans les tests.
#[cfg(test)]
pub(crate) const PANIC_TRIGGER: &[u8] = b"PANIQUE-DE-TEST";

/// Côté le plus grand accepté pour une image demandée au CDN à la taille
/// `requested` : le double, au moins [`MIN_ACCEPTED_SIDE`], au plus
/// [`MAX_SIDE`]. Une image plus grande que ce qui a été demandé est refusée
/// avant que ses pixels ne soient alloués.
pub fn accepted_side(requested: u32) -> u32 {
    requested.saturating_mul(2).clamp(MIN_ACCEPTED_SIDE, MAX_SIDE)
}

/// Pourquoi une image n'a pas pu être décodée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecodeError {
    /// Format inconnu ou non pris en charge (seuls PNG, JPEG, WebP et GIF le sont).
    Unsupported,
    /// Dimensions annoncées trop grandes (ou nulles).
    TooLarge { width: u32, height: u32 },
    /// Fichier tronqué ou corrompu.
    Corrupt(String),
}

impl std::fmt::Display for DecodeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DecodeError::Unsupported => f.write_str("format d'image non pris en charge"),
            DecodeError::TooLarge { width, height } => write!(f, "image trop grande ({width} × {height})"),
            DecodeError::Corrupt(e) => write!(f, "image illisible ({e})"),
        }
    }
}

/// Formats acceptés, reconnus d'après les premiers octets (jamais d'après
/// l'adresse ou l'en-tête `Content-Type`).
pub fn sniff(bytes: &[u8]) -> Option<ImageFormat> {
    match image::guess_format(bytes).ok()? {
        f @ (ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Gif) => Some(f),
        _ => None,
    }
}

/// Image source décodée (première image seulement pour un GIF ou un WebP
/// animé), dont aucun côté ne dépasse `max_side` (ramené à [`MAX_SIDE`]).
/// Une panique du décodeur devient [`DecodeError::Corrupt`].
pub fn decode(bytes: &[u8], max_side: u32) -> Result<DynamicImage, DecodeError> {
    crate::panics::contain(|| decode_unguarded(bytes, max_side)).unwrap_or_else(|| Err(DecodeError::Corrupt("panique du décodeur".into())))
}

fn decode_unguarded(bytes: &[u8], max_side: u32) -> Result<DynamicImage, DecodeError> {
    // Déclencheur des tests de paniques (un vrai décodeur n'en offre pas à la demande).
    #[cfg(test)]
    if bytes == PANIC_TRIGGER {
        panic!("décodeur de test");
    }
    let max_side = max_side.clamp(1, MAX_SIDE);
    let format = sniff(bytes).ok_or(DecodeError::Unsupported)?;
    let mut reader = ImageReader::new(Cursor::new(bytes));
    reader.set_format(format);
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(u64::from(max_side) * u64::from(max_side) * 4 + (8 << 20));
    reader.limits(limits);
    let decoder = reader.into_decoder().map_err(|e| classify(&e))?;
    // Vérifié ici, avant que le décodeur ne réserve la place des pixels.
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 || width > max_side || height > max_side {
        return Err(DecodeError::TooLarge { width, height });
    }
    DynamicImage::from_decoder(decoder).map_err(|e| classify(&e))
}

fn classify(e: &image::ImageError) -> DecodeError {
    match e {
        image::ImageError::Limits(_) => DecodeError::TooLarge { width: 0, height: 0 },
        image::ImageError::Unsupported(_) => DecodeError::Unsupported,
        other => DecodeError::Corrupt(other.to_string()),
    }
}

/// Ajustement à la taille d'affichage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Fit {
    /// Carré de ce côté (en pixels), image recadrée au centre : avatars, icônes.
    Cover(u32),
    /// Image entière dans ce rectangle, sans agrandissement : pièces jointes (lots suivants).
    Contain(u32, u32),
    /// Rectangle rempli (en pixels), image recadrée au centre sans
    /// déformation : bannières de profil, plaques (lot P).
    Fill(u32, u32),
}

/// Image ramenée à sa taille utile, prête pour le processeur graphique ;
/// `None` si le redimensionnement a paniqué.
pub fn fit(img: &DynamicImage, fit: Fit) -> Option<ColorImage> {
    crate::panics::contain(|| fit_unguarded(img, fit))
}

fn fit_unguarded(img: &DynamicImage, fit: Fit) -> ColorImage {
    let out = match fit {
        Fit::Cover(side) => {
            let side = side.clamp(1, MAX_SIDE);
            if img.width() == side && img.height() == side {
                img.clone()
            } else {
                img.resize_to_fill(side, side, FilterType::Lanczos3)
            }
        }
        Fit::Contain(w, h) => {
            let (w, h) = (w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE));
            if img.width() <= w && img.height() <= h {
                img.clone()
            } else {
                img.resize(w, h, FilterType::Lanczos3)
            }
        }
        Fit::Fill(w, h) => {
            let (w, h) = (w.clamp(1, MAX_SIDE), h.clamp(1, MAX_SIDE));
            if img.width() == w && img.height() == h {
                img.clone()
            } else {
                img.resize_to_fill(w, h, FilterType::Lanczos3)
            }
        }
    };
    let rgba = out.into_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    ColorImage::from_rgba_unmultiplied(size, rgba.as_raw())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::RgbaImage;

    fn sample(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 7) as u8, (y * 5) as u8, 128, 255]))
    }

    fn encode(img: &RgbaImage, format: ImageFormat) -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        let rgb = DynamicImage::ImageRgba8(img.clone());
        let r = match format {
            ImageFormat::Jpeg => DynamicImage::ImageRgb8(rgb.to_rgb8()).write_to(&mut out, format),
            _ => rgb.write_to(&mut out, format),
        };
        r.unwrap_or_else(|e| panic!("encodage {format:?} : {e}"));
        out.into_inner()
    }

    #[test]
    fn decodes_supported_formats() {
        for f in [ImageFormat::Png, ImageFormat::Jpeg, ImageFormat::WebP, ImageFormat::Gif] {
            let bytes = encode(&sample(40, 30), f);
            assert_eq!(sniff(&bytes), Some(f));
            let img = decode(&bytes, MAX_SIDE).unwrap_or_else(|e| panic!("{f:?} : {e}"));
            assert_eq!((img.width(), img.height()), (40, 30), "{f:?}");
        }
    }

    #[test]
    fn fit_cover_and_contain() {
        let img = DynamicImage::ImageRgba8(sample(40, 30));
        assert_eq!(fit(&img, Fit::Cover(16)).map(|i| i.size), Some([16, 16]));
        assert_eq!(fit(&img, Fit::Cover(64)).map(|i| i.size), Some([64, 64]));
        assert_eq!(fit(&img, Fit::Cover(0)).map(|i| i.size), Some([1, 1]));
        assert_eq!(fit(&img, Fit::Contain(20, 20)).map(|i| i.size), Some([20, 15]));
        assert_eq!(fit(&img, Fit::Contain(400, 400)).map(|i| i.size), Some([40, 30]), "jamais agrandie");
    }

    /// Bannière : remplit exactement le cadre, recadrée au centre, agrandie s'il le faut.
    #[test]
    fn fit_fill_crops_to_the_frame() {
        let img = DynamicImage::ImageRgba8(sample(40, 30));
        assert_eq!(fit(&img, Fit::Fill(34, 12)).map(|i| i.size), Some([34, 12]));
        assert_eq!(fit(&img, Fit::Fill(10, 30)).map(|i| i.size), Some([10, 30]));
        assert_eq!(fit(&img, Fit::Fill(170, 60)).map(|i| i.size), Some([170, 60]), "agrandie pour couvrir");
        assert_eq!(fit(&img, Fit::Fill(0, 0)).map(|i| i.size), Some([1, 1]));
        assert!(fit(&img, Fit::Fill(u32::MAX, u32::MAX)).is_some_and(|i| i.size[0] <= MAX_SIDE as usize && i.size[1] <= MAX_SIDE as usize));
    }

    /// Fragment PNG : signature + IHDR (avec CRC correct) annonçant `w` × `h`.
    fn png_header(w: u32, h: u32) -> Vec<u8> {
        let mut ihdr = b"IHDR".to_vec();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
        out.extend_from_slice(&13u32.to_be_bytes());
        out.extend_from_slice(&ihdr);
        out.extend_from_slice(&crc32(&ihdr).to_be_bytes());
        // Un IDAT minuscule : le décodeur n'irait de toute façon pas plus loin.
        out.extend_from_slice(&[0, 0, 0, 0, b'I', b'D', b'A', b'T', 0x35, 0xAF, 0x06, 0x1E]);
        out
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for b in data {
            crc ^= u32::from(*b);
            for _ in 0..8 {
                crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
            }
        }
        !crc
    }

    /// La taille acceptée suit la taille demandée : une image de 1024 px
    /// servie pour un avatar de 64 est refusée avant l'allocation.
    #[test]
    fn accepted_side_follows_request() {
        assert_eq!(accepted_side(16), MIN_ACCEPTED_SIDE);
        assert_eq!(accepted_side(64), MIN_ACCEPTED_SIDE);
        assert_eq!(accepted_side(256), 512);
        assert_eq!(accepted_side(4096), MAX_SIDE);
        assert_eq!(accepted_side(u32::MAX), MAX_SIDE);
        let big = encode(&sample(1024, 8), ImageFormat::Png);
        assert!(matches!(decode(&big, accepted_side(64)), Err(DecodeError::TooLarge { .. })));
        assert!(decode(&big, accepted_side(512)).is_ok());
        // En-tête seul annonçant 257 × 1 : refusé pour une petite taille.
        assert!(matches!(decode(&png_header(257, 1), accepted_side(32)), Err(DecodeError::TooLarge { .. })));
    }

    #[test]
    fn hostile_dimensions_refused_before_allocation() {
        for (w, h) in [(100_000, 100_000), (u32::MAX, 1), (1, u32::MAX), (4097, 16), (0, 10)] {
            match decode(&png_header(w, h), MAX_SIDE) {
                Err(DecodeError::TooLarge { .. }) => {}
                // Certains décodeurs refusent la largeur nulle comme données corrompues.
                Err(DecodeError::Corrupt(_)) if w == 0 => {}
                other => panic!("{w} × {h} : {other:?}"),
            }
        }
        // JPEG : SOF0 annonçant 65500 × 65500.
        let mut jpeg = vec![0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0xFF, 0xDC, 0xFF, 0xDC, 0x03];
        jpeg.extend_from_slice(&[1, 0x11, 0, 2, 0x11, 0, 3, 0x11, 0, 0xFF, 0xD9]);
        assert!(matches!(decode(&jpeg, MAX_SIDE), Err(DecodeError::TooLarge { .. } | DecodeError::Corrupt(_))), "{:?}", decode(&jpeg, MAX_SIDE));
    }

    #[test]
    fn truncated_or_wrong_type_is_an_error() {
        let png = encode(&sample(64, 64), ImageFormat::Png);
        for cut in [0, 4, 8, 20, 33, png.len() / 2] {
            assert!(decode(&png[..cut], MAX_SIDE).is_err(), "PNG tronqué à {cut}");
        }
        for f in [ImageFormat::Jpeg, ImageFormat::WebP, ImageFormat::Gif] {
            let bytes = encode(&sample(64, 64), f);
            let _ = decode(&bytes[..bytes.len() / 2], MAX_SIDE); // erreur ou image partielle : sans panique
        }
        // Octets aléatoires derrière une vraie signature.
        let mut garbage = png[..8].to_vec();
        garbage.extend((0..500u32).map(|i| (i.wrapping_mul(2_654_435_761) >> 13) as u8));
        assert!(decode(&garbage, MAX_SIDE).is_err());
        assert_eq!(decode(b"<!doctype html><title>404</title>", MAX_SIDE), Err(DecodeError::Unsupported));
        assert_eq!(decode(b"", MAX_SIDE), Err(DecodeError::Unsupported));
        // Formats exclus (BMP, TIFF) : refusés même valides.
        assert_eq!(decode(b"BM\x3e\0\0\0\0\0\0\0\x36\0\0\0", MAX_SIDE), Err(DecodeError::Unsupported));
        assert_eq!(decode(b"II*\0\x08\0\0\0", MAX_SIDE), Err(DecodeError::Unsupported));
    }

    /// Panique du décodeur : un échec comme un autre, et le fil continue.
    #[test]
    fn decoder_panic_is_an_error() {
        assert!(matches!(decode(PANIC_TRIGGER, MAX_SIDE), Err(DecodeError::Corrupt(_))));
        let png = encode(&sample(8, 8), ImageFormat::Png);
        assert!(decode(&png, MAX_SIDE).is_ok(), "le fil décode encore après");
    }
}
