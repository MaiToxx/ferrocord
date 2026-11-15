//! Image d'avatar ou de bannière à envoyer : boîte de dialogue « Ouvrir »,
//! décodage, puis recadrage, redimensionnement et codage PNG. Tout se fait
//! dans un fil à part ; l'interface reprend les résultats par [`Avatar::poll`].

use crate::images::decode::{self, Fit};
use crate::state::crop::{self, Crop, ImageKind};
use eframe::egui::ColorImage;
use image::DynamicImage;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// Fichiers plus gros refusés sans être lus (l'application officielle
/// refuse aussi les gros fichiers d'avatar).
pub const MAX_BYTES: u64 = 10 << 20;
/// Côté de l'aperçu affiché pendant le recadrage.
pub const PREVIEW_SIDE: u32 = 512;

pub type Wake = Arc<dyn Fn() + Send + Sync>;

#[derive(Debug)]
pub enum Done {
    /// Image choisie et décodée : l'aperçu (réduit) et l'original.
    Picked { kind: ImageKind, image: Arc<DynamicImage>, preview: ColorImage },
    /// Fichier illisible, trop gros ou qui n'est pas une image.
    Unreadable { kind: ImageKind, name: String },
    /// Image prête à envoyer (`data:`).
    Encoded { kind: ImageKind, guild: Option<u64>, data: String },
}

pub struct Avatar {
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
    wake: Wake,
    picking: Arc<AtomicBool>,
}

/// Image décodée (premier cadre d'une animation), et son aperçu.
pub fn load(kind: ImageKind, name: String, bytes: &[u8]) -> Done {
    let Some(image) = decode::sniff(bytes).and_then(|_| decode::decode(bytes, decode::MAX_SIDE).ok()) else { return Done::Unreadable { kind, name } };
    match decode::fit(&image, Fit::Contain(PREVIEW_SIDE, PREVIEW_SIDE)) {
        Some(preview) => Done::Picked { kind, image: Arc::new(image), preview },
        None => Done::Unreadable { kind, name },
    }
}

/// PNG recadré aux proportions de `kind` et à sa taille d'envoi.
pub fn encode(image: &DynamicImage, c: Crop, kind: ImageKind) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let r = c.rect(image.width(), image.height(), kind);
    let (w, h) = kind.output();
    let out = image.crop_imm(r.x, r.y, r.w, r.h).resize_exact(w, h, image::imageops::FilterType::Triangle).to_rgba8();
    let mut png = Vec::new();
    image::codecs::png::PngEncoder::new(&mut png).write_image(out.as_raw(), w, h, image::ExtendedColorType::Rgba8).ok()?;
    Some(png)
}

impl Avatar {
    pub fn new(wake: Wake) -> Self {
        let (tx, rx) = mpsc::channel();
        Avatar { tx, rx, wake, picking: Arc::default() }
    }

    fn spawn(&self, work: impl FnOnce(&mpsc::Sender<Done>) + Send + 'static) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let spawned = std::thread::Builder::new().name("avatar".into()).spawn(move || {
            work(&tx);
            wake();
        });
        // Fil impossible à créer : le geste est perdu, rien d'autre.
        drop(spawned);
    }

    /// Boîte de dialogue « Ouvrir » (une image), dans un fil à part.
    #[cfg(windows)]
    pub fn pick(&self, kind: ImageKind, title: &str) {
        if self.picking.swap(true, Ordering::SeqCst) {
            return;
        }
        let (title, picking) = (title.to_string(), self.picking.clone());
        self.spawn(move |tx| {
            let chosen = rfd::FileDialog::new().set_title(title).add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"]).pick_file();
            picking.store(false, Ordering::SeqCst);
            let Some(path) = chosen else { return };
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let small = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
            let done = match small.then(|| std::fs::read(&path).ok()).flatten() {
                Some(bytes) => load(kind, name, &bytes),
                None => Done::Unreadable { kind, name },
            };
            let _ = tx.send(done);
        });
    }

    #[cfg(not(windows))]
    pub fn pick(&self, _: ImageKind, _: &str) {}

    pub const CAN_PICK: bool = cfg!(windows);

    pub fn picking(&self) -> bool {
        self.picking.load(Ordering::SeqCst)
    }

    /// Octets déjà en mémoire (aperçu, tests).
    pub fn add_bytes(&self, kind: ImageKind, name: String, bytes: Vec<u8>) {
        self.spawn(move |tx| {
            let _ = tx.send(load(kind, name, &bytes));
        });
    }

    /// Recadre et code l'image pour l'envoi.
    pub fn encode(&self, image: Arc<DynamicImage>, c: Crop, kind: ImageKind, guild: Option<u64>) {
        self.spawn(move |tx| {
            if let Some(png) = encode(&image, c, kind) {
                let _ = tx.send(Done::Encoded { kind, guild, data: crop::data_uri(&png) });
            }
        });
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, _| if x < w / 2 { image::Rgba([255, 0, 0, 255]) } else { image::Rgba([0, 0, 255, 255]) });
        let mut out = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(img).write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn banner_is_cropped_resized_and_encoded() {
        let Done::Picked { image, preview, .. } = load(ImageKind::Banner, "b.png".into(), &png(1000, 1000)) else { panic!() };
        assert_eq!(preview.size, [512, 512]);
        let out = encode(&image, Crop { zoom: 1.0, cx: 0.0, cy: 0.5 }, ImageKind::Banner).unwrap();
        let back = decode::decode(&out, 4096).unwrap();
        assert_eq!((back.width(), back.height()), (600, 240));
        let left = back.to_rgba8().get_pixel(10, 120).0;
        assert_eq!(left, [255, 0, 0, 255]);
    }

    #[test]
    fn not_an_image_is_refused() {
        assert!(matches!(load(ImageKind::Avatar, "x.txt".into(), b"bonjour"), Done::Unreadable { .. }));
    }

    #[test]
    fn thread_gives_a_png_data_uri() {
        let a = Avatar::new(Arc::new(|| {}));
        a.add_bytes(ImageKind::Avatar, "a.png".into(), png(300, 200));
        let start = std::time::Instant::now();
        let mut got = Vec::new();
        while got.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
            got = a.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let [Done::Picked { image, .. }] = got.as_slice() else { panic!("{got:?}") };
        a.encode(image.clone(), Crop::default(), ImageKind::Avatar, Some(9));
        let mut got = Vec::new();
        while got.is_empty() && start.elapsed() < std::time::Duration::from_secs(10) {
            got = a.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let [Done::Encoded { guild: Some(9), data, .. }] = got.as_slice() else { panic!("{got:?}") };
        assert!(data.starts_with("data:image/png;base64,iVBOR"));
    }
}
