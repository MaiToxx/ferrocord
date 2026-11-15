//! Fichiers des emojis, autocollants et sons d'un serveur (lot A6) : boîte
//! « Ouvrir » de Windows (comme `attach.rs`), lecture, vérification et mise
//! en forme dans un fil à part ; l'interface reprend le résultat par
//! [`ExprPicker::poll`] quand elle est réveillée.
//!
//! - Emoji : GIF animé gardé tel quel (256 Kio au plus), toute autre image
//!   ramenée dans 128 × 128 et envoyée en PNG.
//! - Autocollant : PNG, APNG ou GIF gardé tel quel (500 Kio au plus) ; une
//!   autre image fixe est ramenée dans 320 × 320 en PNG.
//! - Son : MP3 ou Ogg, 512 Kio et 5,2 s au plus (`state::admin::expr_files`).

use crate::attach::{encode_png, Wake};
use crate::images::decode::{self, Fit};
use crate::state::admin::expr_files::{self as files, SoundError, StickerKind};
use crate::state::admin::expressions::{name_from_file, Kind, StickerFile, EMOJI_NAME_MAX, SOUND_NAME_MAX, STICKER_NAME_MAX};
use eframe::egui::ColorImage;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// Fichiers plus gros : refusés sans être lus (octets).
pub const MAX_READ: u64 = 10 << 20;
/// Côté de l'aperçu d'un autocollant (pixels).
const PREVIEW_SIDE: u32 = 160;

/// Fichier prêt à envoyer.
#[derive(Clone, Debug)]
pub enum Picked {
    Emoji { name: String, data_uri: String, animated: bool, preview: ColorImage },
    Sticker { name: String, file: StickerFile, preview: ColorImage },
    /// `ms` : durée du son.
    Sound { name: String, data_uri: String, ms: u64 },
}

/// Pourquoi le fichier choisi ne convient pas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PickError {
    /// Pas une image (ou un son) lisible.
    Unreadable,
    /// Trop gros, même ramené à la bonne taille.
    TooBig,
    /// Son trop long (ms).
    TooLong(u64),
}

pub type Outcome = Result<Picked, PickError>;

#[cfg_attr(not(windows), allow(dead_code))]
fn file_name(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Image fixe ramenée dans `side` × `side` puis encodée en PNG.
fn static_png(bytes: &[u8], side: u32) -> Option<(Vec<u8>, ColorImage)> {
    let img = decode::decode(bytes, decode::MAX_SIDE).ok()?;
    let fitted = decode::fit(&img, Fit::Contain(side, side))?;
    Some((encode_png(&fitted)?, fitted))
}

fn preview(bytes: &[u8], side: u32) -> Option<ColorImage> {
    let img = decode::decode(bytes, decode::MAX_SIDE).ok()?;
    decode::fit(&img, Fit::Contain(side, side))
}

/// Emoji prêt à envoyer.
pub fn prepare_emoji(file: &str, bytes: &[u8]) -> Outcome {
    let name = name_from_file(file, EMOJI_NAME_MAX);
    let format = decode::sniff(bytes).ok_or(PickError::Unreadable)?;
    if format == image::ImageFormat::Gif && crate::images::anim::is_animated(bytes) {
        if bytes.len() > files::MAX_EMOJI_BYTES {
            return Err(PickError::TooBig);
        }
        let preview = preview(bytes, files::EMOJI_SIDE).ok_or(PickError::Unreadable)?;
        return Ok(Picked::Emoji { name, data_uri: files::data_uri("image/gif", bytes), animated: true, preview });
    }
    let (png, preview) = static_png(bytes, files::EMOJI_SIDE).ok_or(PickError::Unreadable)?;
    if png.len() > files::MAX_EMOJI_BYTES {
        return Err(PickError::TooBig);
    }
    Ok(Picked::Emoji { name, data_uri: files::data_uri("image/png", &png), animated: false, preview })
}

/// Autocollant prêt à envoyer.
pub fn prepare_sticker(file: &str, bytes: &[u8]) -> Outcome {
    let name = name_from_file(file, STICKER_NAME_MAX);
    let stem = file.rsplit_once('.').map_or(file, |(s, _)| s);
    let kept = files::sticker_kind(bytes).filter(|_| bytes.len() <= files::MAX_STICKER_BYTES);
    let (kind, data, preview) = match kept {
        Some(kind) => (kind, bytes.to_vec(), preview(bytes, PREVIEW_SIDE).ok_or(PickError::Unreadable)?),
        None => {
            decode::sniff(bytes).ok_or(PickError::Unreadable)?;
            // Une animation ne se réduit pas sans la perdre : refusée si trop grosse.
            if crate::images::anim::is_animated(bytes) {
                return Err(PickError::TooBig);
            }
            let (png, _) = static_png(bytes, files::STICKER_SIDE).ok_or(PickError::Unreadable)?;
            if png.len() > files::MAX_STICKER_BYTES {
                return Err(PickError::TooBig);
            }
            (StickerKind::Png, png, preview(bytes, PREVIEW_SIDE).ok_or(PickError::Unreadable)?)
        }
    };
    let file = StickerFile { name: format!("{}.{}", if stem.is_empty() { "sticker" } else { stem }, kind.ext()), mime: kind.mime(), bytes: Arc::from(data) };
    Ok(Picked::Sticker { name, file, preview })
}

/// Son prêt à envoyer.
pub fn prepare_sound(file: &str, bytes: &[u8]) -> Outcome {
    let (kind, ms) = files::check_sound(bytes).map_err(|e| match e {
        SoundError::Unsupported => PickError::Unreadable,
        SoundError::TooBig => PickError::TooBig,
        SoundError::TooLong(ms) => PickError::TooLong(ms),
    })?;
    Ok(Picked::Sound { name: name_from_file(file, SOUND_NAME_MAX), data_uri: files::data_uri(kind.mime(), bytes), ms })
}

/// Prépare `bytes` (fichier `file`) pour ce genre d'élément ; une panique
/// d'un décodeur devient un fichier illisible.
pub fn prepare(kind: Kind, file: &str, bytes: &[u8]) -> Outcome {
    let run = || match kind {
        Kind::Emoji => prepare_emoji(file, bytes),
        Kind::Sticker => prepare_sticker(file, bytes),
        Kind::Sound => prepare_sound(file, bytes),
    };
    crate::panics::contain(run).unwrap_or(Err(PickError::Unreadable))
}

/// Fil des choix de fichiers : un seul à la fois.
pub struct ExprPicker {
    tx: mpsc::Sender<(Kind, Outcome)>,
    rx: mpsc::Receiver<(Kind, Outcome)>,
    wake: Wake,
    busy: Arc<AtomicBool>,
}

impl ExprPicker {
    pub fn new(wake: Wake) -> Self {
        let (tx, rx) = mpsc::channel();
        ExprPicker { tx, rx, wake, busy: Arc::default() }
    }

    /// La boîte de dialogue existe sur cette plateforme.
    pub const CAN_PICK: bool = cfg!(windows);

    fn spawn(&self, kind: Kind, work: impl FnOnce() -> Option<Outcome> + Send + 'static) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let (tx, wake, busy) = (self.tx.clone(), self.wake.clone(), self.busy.clone());
        let spawned = std::thread::Builder::new().name("expressions".into()).spawn(move || {
            if let Some(out) = work() {
                let _ = tx.send((kind, out));
            }
            busy.store(false, Ordering::SeqCst);
            wake();
        });
        if spawned.is_err() {
            self.busy.store(false, Ordering::SeqCst);
        }
    }

    /// Boîte « Ouvrir » pour ce genre d'élément ; annulée : rien.
    #[cfg(windows)]
    pub fn pick(&self, kind: Kind, title: &str) {
        let title = title.to_string();
        self.spawn(kind, move || {
            let (label, exts): (&str, &[&str]) = match kind {
                Kind::Emoji => ("Images", &["png", "jpg", "jpeg", "webp", "gif"]),
                Kind::Sticker => ("Images", &["png", "apng", "gif", "jpg", "jpeg", "webp"]),
                Kind::Sound => ("Audio", &["mp3", "ogg", "opus"]),
            };
            let path = rfd::FileDialog::new().set_title(title).add_filter(label, exts).pick_file()?;
            let name = file_name(&path);
            let small = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_READ);
            if !small {
                return Some(Err(PickError::TooBig));
            }
            Some(std::fs::read(&path).map_or(Err(PickError::Unreadable), |b| prepare(kind, &name, &b)))
        });
    }

    /// Octets déjà en mémoire (aperçu, tests).
    pub fn from_bytes(&self, kind: Kind, name: String, bytes: Vec<u8>) {
        self.spawn(kind, move || Some(prepare(kind, &name, &bytes)));
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn poll(&self) -> Option<(Kind, Outcome)> {
        self.rx.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    fn png(w: usize, h: usize) -> Vec<u8> {
        encode_png(&ColorImage::new([w, h], vec![Color32::from_rgb(10, 200, 30); w * h])).unwrap_or_default()
    }

    #[test]
    fn emoji_is_reduced_to_128_png() {
        let Ok(Picked::Emoji { name, data_uri, animated, preview }) = prepare_emoji("mon chat.png", &png(300, 200)) else { panic!() };
        assert_eq!((name.as_str(), animated, preview.size), ("mon_chat", false, [128, 85]));
        assert!(data_uri.starts_with("data:image/png;base64,"));
        assert_eq!(prepare_emoji("x.txt", b"bonjour").err(), Some(PickError::Unreadable));
        let gif = crate::images::anim::tests::gif(3, 16);
        let Ok(Picked::Emoji { animated, data_uri, .. }) = prepare_emoji("danse.gif", &gif) else { panic!() };
        assert!(animated && data_uri.starts_with("data:image/gif;base64,"));
    }

    #[test]
    fn sticker_keeps_png_and_converts_others() {
        let Ok(Picked::Sticker { file, .. }) = prepare_sticker("salut.png", &png(320, 320)) else { panic!() };
        assert_eq!((file.name.as_str(), file.mime), ("salut.png", "image/png"));
        let mut jpeg = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image::RgbImage::new(640, 640)).write_to(&mut jpeg, image::ImageFormat::Jpeg).unwrap_or_else(|e| panic!("{e}"));
        let Ok(Picked::Sticker { file, preview, .. }) = prepare_sticker("photo.jpg", jpeg.get_ref()) else { panic!() };
        assert_eq!((file.name.as_str(), file.mime, preview.size), ("photo.png", "image/png", [160, 160]));
        assert_eq!(decode::sniff(&file.bytes), Some(image::ImageFormat::Png));
    }

    #[test]
    fn sound_is_checked() {
        let mp3 = crate::state::admin::expr_files::tests::mp3(100);
        let Ok(Picked::Sound { name, data_uri, ms }) = prepare_sound("ding dong.mp3", &mp3) else { panic!() };
        assert_eq!((name.as_str(), ms), ("ding_dong", 2612));
        assert!(data_uri.starts_with("data:audio/mpeg;base64,"));
        assert_eq!(prepare_sound("long.mp3", &crate::state::admin::expr_files::tests::mp3(300)).err(), Some(PickError::TooLong(7837)));
        assert_eq!(prepare(Kind::Sound, "x.wav", b"RIFF....WAVE").err(), Some(PickError::Unreadable));
    }
}
