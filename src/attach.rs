//! Fichiers locaux à joindre à un message (lot T8) : boîte de dialogue
//! « Ouvrir » de Windows, fichiers déposés, image collée. Tout ce qui touche
//! au disque ou décode une image se fait dans un fil à part ; l'interface
//! reprend les résultats par [`Attach::poll`] quand elle est réveillée.
//!
//! Rien n'est lu en entier : la taille vient des métadonnées, et seule une
//! image de moins de [`THUMB_MAX_BYTES`] est lue pour sa vignette. Le
//! contenu part plus tard, en flux, au moment de l'envoi (`net::upload`).

use crate::images::decode::{self, Fit};
use crate::state::upload::{OutFile, Source};
use eframe::egui::ColorImage;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc,
    },
};

/// Images lues pour leur vignette, au plus (octets).
pub const THUMB_MAX_BYTES: u64 = 8 << 20;
/// Côté de la vignette décodée (pixels).
pub const THUMB_SIDE: u32 = 192;
/// Nom d'une image collée (celui que donne l'application officielle).
pub const PASTED_NAME: &str = "image.png";

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Résultat d'un ajout.
#[derive(Debug)]
pub enum Staged {
    /// Fichier prêt à joindre, avec sa vignette si c'est une image.
    File { channel: u64, file: OutFile, thumb: Option<ColorImage> },
    /// Fichier illisible ou dossier : seulement son nom.
    Unreadable { channel: u64, name: String },
}

/// Fil des ajouts.
pub struct Attach {
    tx: mpsc::Sender<Staged>,
    rx: mpsc::Receiver<Staged>,
    wake: Wake,
    /// Une boîte de dialogue est ouverte (une seule à la fois).
    picking: Arc<AtomicBool>,
}

fn name_of(path: &std::path::Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Vignette d'une image (premier cadre), ou rien si ce n'en est pas une.
fn thumbnail(bytes: &[u8]) -> Option<ColorImage> {
    decode::sniff(bytes)?;
    let img = decode::decode(bytes, decode::MAX_SIDE).ok()?;
    decode::fit(&img, Fit::Cover(THUMB_SIDE))
}

/// Taille et vignette d'un fichier du disque (bloquant).
fn stage_path(channel: u64, path: PathBuf) -> Staged {
    let name = name_of(&path);
    let Ok(meta) = std::fs::metadata(&path) else { return Staged::Unreadable { channel, name } };
    if !meta.is_file() {
        return Staged::Unreadable { channel, name };
    }
    let size = meta.len();
    let thumb = if size <= THUMB_MAX_BYTES { std::fs::read(&path).ok().and_then(|b| thumbnail(&b)) } else { None };
    let file = OutFile { source: Source::Path(path), name, size, description: String::new(), spoiler: false };
    Staged::File { channel, file, thumb }
}

/// Octets en mémoire (image collée encodée, ou test).
fn stage_bytes(channel: u64, name: String, bytes: Vec<u8>) -> Staged {
    let thumb = thumbnail(&bytes);
    let size = bytes.len() as u64;
    let file = OutFile { source: Source::Memory(Arc::from(bytes)), name, size, description: String::new(), spoiler: false };
    Staged::File { channel, file, thumb }
}

/// Image du presse-papiers en PNG (couleurs prémultipliées d'egui remises à plat).
pub fn encode_png(img: &ColorImage) -> Option<Vec<u8>> {
    use image::ImageEncoder;
    let [w, h] = img.size;
    let rgba: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out).write_image(&rgba, u32::try_from(w).ok()?, u32::try_from(h).ok()?, image::ExtendedColorType::Rgba8).ok()?;
    Some(out)
}

impl Attach {
    pub fn new(wake: Wake) -> Self {
        let (tx, rx) = mpsc::channel();
        Attach { tx, rx, wake, picking: Arc::default() }
    }

    fn spawn(&self, work: impl FnOnce(&mpsc::Sender<Staged>) + Send + 'static) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let spawned = std::thread::Builder::new().name("pieces-jointes".into()).spawn(move || {
            work(&tx);
            wake();
        });
        // Fil impossible à créer (ressources épuisées) : l'ajout est perdu, rien d'autre.
        drop(spawned);
    }

    /// Fichiers choisis par l'utilisateur (déposés sur la fenêtre, par exemple).
    pub fn add_paths(&self, channel: u64, paths: Vec<PathBuf>) {
        if paths.is_empty() {
            return;
        }
        self.spawn(move |tx| {
            for p in paths {
                let _ = tx.send(stage_path(channel, p));
            }
        });
    }

    /// Octets déjà en mémoire, sous ce nom.
    pub fn add_bytes(&self, channel: u64, name: String, bytes: Vec<u8>) {
        self.spawn(move |tx| {
            let _ = tx.send(stage_bytes(channel, name, bytes));
        });
    }

    /// Image collée : encodée en PNG hors du fil d'interface.
    pub fn add_image(&self, channel: u64, img: Arc<ColorImage>) {
        self.spawn(move |tx| {
            if let Some(png) = encode_png(&img) {
                let _ = tx.send(stage_bytes(channel, PASTED_NAME.into(), png));
            }
        });
    }

    /// Boîte de dialogue « Ouvrir » (plusieurs fichiers), dans un fil à part.
    /// Sans effet si une autre est déjà ouverte.
    #[cfg(windows)]
    pub fn pick(&self, channel: u64, title: &str) {
        if self.picking.swap(true, Ordering::SeqCst) {
            return;
        }
        let (title, picking) = (title.to_string(), self.picking.clone());
        self.spawn(move |tx| {
            let chosen = rfd::FileDialog::new().set_title(title).pick_files().unwrap_or_default();
            picking.store(false, Ordering::SeqCst);
            for p in chosen {
                let _ = tx.send(stage_path(channel, p));
            }
        });
    }

    /// La boîte de dialogue existe sur cette plateforme.
    pub const CAN_PICK: bool = cfg!(windows);

    pub fn picking(&self) -> bool {
        self.picking.load(Ordering::SeqCst)
    }

    /// Ajouts terminés depuis le dernier appel.
    pub fn poll(&self) -> Vec<Staged> {
        self.rx.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    fn wait(a: &Attach, n: usize) -> Vec<Staged> {
        let mut out = Vec::new();
        let start = std::time::Instant::now();
        while out.len() < n && start.elapsed() < std::time::Duration::from_secs(10) {
            out.extend(a.poll());
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        out
    }

    #[test]
    fn pasted_image_becomes_a_png_with_a_thumbnail() {
        let a = Attach::new(Arc::new(|| {}));
        let img = ColorImage::new([300, 200], vec![Color32::from_rgba_unmultiplied(200, 10, 10, 128); 300 * 200]);
        a.add_image(7, Arc::new(img));
        let got = wait(&a, 1);
        let [Staged::File { channel: 7, file, thumb: Some(thumb) }] = got.as_slice() else { panic!("{got:?}") };
        assert_eq!(file.name, "image.png");
        let Source::Memory(bytes) = &file.source else { panic!() };
        assert_eq!(decode::sniff(bytes), Some(image::ImageFormat::Png));
        assert_eq!(file.size, bytes.len() as u64);
        assert_eq!(thumb.size, [THUMB_SIDE as usize, THUMB_SIDE as usize]);
        // Couleur gardée à peu près (prémultiplication défaite puis refaite).
        let back = decode::decode(bytes, 4096).unwrap().to_rgba8();
        let p = back.get_pixel(0, 0).0;
        assert!(p[0] > 190 && p[1] < 20 && (p[3] as i32 - 128).abs() <= 1, "{p:?}");
    }

    #[test]
    fn files_on_disk_give_size_and_folders_are_refused() {
        let dir = std::env::temp_dir().join(format!("ferrocord-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("notes.txt");
        std::fs::write(&path, b"bonjour").unwrap();
        let a = Attach::new(Arc::new(|| {}));
        a.add_paths(3, vec![path.clone(), dir.clone(), dir.join("absent.bin")]);
        let got = wait(&a, 3);
        assert!(matches!(&got[0], Staged::File { channel: 3, file, thumb: None } if file.size == 7 && file.name == "notes.txt" && file.source == Source::Path(path.clone())), "{got:?}");
        assert!(matches!(&got[1], Staged::Unreadable { .. }), "{got:?}");
        assert!(matches!(&got[2], Staged::Unreadable { name, .. } if name == "absent.bin"), "{got:?}");
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir(&dir);
    }
}
