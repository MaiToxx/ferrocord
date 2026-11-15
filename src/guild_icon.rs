//! Icône d'un nouveau serveur (lot A1) : image choisie par la boîte
//! « Ouvrir » de Windows, lue, décodée et ramenée à 128 × 128 dans un fil à
//! part, puis envoyée en PNG dans une adresse `data:` (champ `icon` de
//! Create Guild, « image data » de la documentation). L'interface reprend
//! le résultat par [`IconPicker::poll`] quand elle est réveillée.

use crate::attach::{encode_png, Wake};
use crate::images::decode::{self, Fit};
use crate::state::usersettings::b64;
use eframe::egui::ColorImage;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc,
};

/// Côté de l'icône envoyée (documentation : 128 × 128).
pub const SIDE: u32 = 128;
/// Fichiers plus gros : refusés sans être lus (octets).
pub const MAX_BYTES: u64 = 10 << 20;

#[derive(Clone, Debug)]
pub struct Picked {
    /// `data:image/png;base64,…`
    pub data_uri: String,
    pub image: ColorImage,
}

#[derive(Debug)]
pub enum Outcome {
    Ready(Picked),
    Unreadable,
}

/// Image prête à envoyer, ou rien si ce n'en est pas une.
pub fn prepare(bytes: &[u8]) -> Option<Picked> {
    prepare_fit(bytes, Fit::Cover(SIDE))
}

/// Couverture d'un événement (lot fin-texte-restes) : image entière, au plus
/// 1000 × 400 (Discord la montre en 2,5:1 ; 800 × 320 au moins conseillé).
pub const COVER: Fit = Fit::Contain(1000, 400);

/// Image prête à envoyer, ajustée selon `fit`.
pub fn prepare_fit(bytes: &[u8], fit: Fit) -> Option<Picked> {
    decode::sniff(bytes)?;
    let img = decode::decode(bytes, decode::MAX_SIDE).ok()?;
    let image = decode::fit(&img, fit)?;
    let png = encode_png(&image)?;
    Some(Picked { data_uri: format!("data:image/png;base64,{}", b64::encode(&png)), image })
}

pub struct IconPicker {
    tx: mpsc::Sender<Outcome>,
    rx: mpsc::Receiver<Outcome>,
    wake: Wake,
    busy: Arc<AtomicBool>,
    fit: Fit,
}

impl IconPicker {
    pub fn new(wake: Wake) -> Self {
        let (tx, rx) = mpsc::channel();
        IconPicker { tx, rx, wake, busy: Arc::default(), fit: Fit::Cover(SIDE) }
    }

    /// Même boîte, image ajustée selon `fit` (couverture d'un événement).
    pub fn with_fit(wake: Wake, fit: Fit) -> Self {
        IconPicker { fit, ..IconPicker::new(wake) }
    }

    /// La boîte de dialogue existe sur cette plateforme.
    pub const CAN_PICK: bool = cfg!(windows);

    fn spawn(&self, work: impl FnOnce() -> Option<Outcome> + Send + 'static) {
        if self.busy.swap(true, Ordering::SeqCst) {
            return;
        }
        let (tx, wake, busy) = (self.tx.clone(), self.wake.clone(), self.busy.clone());
        let spawned = std::thread::Builder::new().name("icone-serveur".into()).spawn(move || {
            if let Some(out) = work() {
                let _ = tx.send(out);
            }
            busy.store(false, Ordering::SeqCst);
            wake();
        });
        if spawned.is_err() {
            self.busy.store(false, Ordering::SeqCst);
        }
    }

    /// Boîte « Ouvrir » (une image), dans un fil à part ; annulée : rien.
    #[cfg(windows)]
    pub fn pick(&self, title: &str) {
        let title = title.to_string();
        let fit = self.fit;
        self.spawn(move || {
            let path = rfd::FileDialog::new().set_title(title).add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif"]).pick_file()?;
            let small = std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() <= MAX_BYTES);
            let picked = if small { std::fs::read(&path).ok().and_then(|b| prepare_fit(&b, fit)) } else { None };
            Some(picked.map_or(Outcome::Unreadable, Outcome::Ready))
        });
    }

    /// Octets déjà en mémoire (aperçu, tests).
    pub fn from_bytes(&self, bytes: Vec<u8>) {
        let fit = self.fit;
        self.spawn(move || Some(prepare_fit(&bytes, fit).map_or(Outcome::Unreadable, Outcome::Ready)));
    }

    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::SeqCst)
    }

    pub fn poll(&self) -> Option<Outcome> {
        self.rx.try_iter().last()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Color32;

    #[test]
    fn image_becomes_a_128_png_data_uri() {
        let img = ColorImage::new([300, 200], vec![Color32::from_rgb(10, 200, 30); 300 * 200]);
        let png = encode_png(&img).unwrap();
        let p = prepare(&png).unwrap();
        assert_eq!(p.image.size, [SIDE as usize, SIDE as usize]);
        let b = p.data_uri.strip_prefix("data:image/png;base64,").unwrap();
        let bytes = b64::decode(b).unwrap();
        assert_eq!(decode::sniff(&bytes), Some(image::ImageFormat::Png));
        assert!(prepare(b"pas une image").is_none());
        // Couverture : image entière, ramenée sous 1000 × 400.
        let wide = encode_png(&ColorImage::new([2000, 500], vec![Color32::WHITE; 2000 * 500])).unwrap();
        assert_eq!(prepare_fit(&wide, COVER).map(|p| p.image.size), Some([1000, 250]));

        let picker = IconPicker::new(Arc::new(|| {}));
        picker.from_bytes(b"rien".to_vec());
        let start = std::time::Instant::now();
        let mut got = None;
        while got.is_none() && start.elapsed() < std::time::Duration::from_secs(10) {
            got = picker.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(matches!(got, Some(Outcome::Unreadable)));
    }
}
