//! Fichiers de la personnalisation du client (lot PERSO) : image de fond
//! (décodée, réduite et floutée une fois), polices installées, bibliothèque
//! des thèmes et des profils d'apparence enregistrés. Tout se fait dans un
//! fil à part ; l'interface reprend les résultats par [`Look::poll`].
//!
//! Rien ne part sur le réseau : tout reste sur cet ordinateur.

use crate::images::decode;
use crate::theme::share::{self, Package, ShareError};
use eframe::egui::ColorImage;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Image de fond plus grosse : refusée sans être lue.
pub const MAX_IMAGE_BYTES: u64 = 32 << 20;
/// Côté maximal de la texture du fond (une seule, étirée sur la fenêtre).
pub const BACKDROP_SIDE: u32 = 1600;
/// Fichier de police plus gros : ignoré.
const MAX_FONT_BYTES: u64 = 16 << 20;
/// Longueur maximale du nom d'un thème ou d'un profil enregistré.
pub const NAME_MAX: usize = 40;
/// Sépare, dans un profil d'apparence, le thème de la section `[apparence]`.
pub const PROFILE_MARK: &str = "# ---- profil : reglages d'apparence ----\n";

/// Contenu d'un fichier de police.
pub type FontBytes = Arc<Vec<u8>>;

/// Famille de polices installée : normale, grasse, italique, grasse italique.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub name: String,
    /// Chasse fixe (proposée pour le code).
    pub mono: bool,
    pub faces: [Option<PathBuf>; 4],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Theme,
    Profile,
}

impl Kind {
    fn dir(self) -> &'static str {
        match self {
            Kind::Theme => "themes",
            Kind::Profile => "profils",
        }
    }
}

#[derive(Debug)]
pub enum Done {
    /// Image choisie dans la boîte « Ouvrir » : son chemin.
    Picked(String),
    /// Image de fond prête (`None` : illisible), pour ce chemin et ce flou.
    Backdrop { path: String, blur: u8, image: Option<ColorImage> },
    Fonts(Vec<Family>),
    /// Fichiers d'une famille ; `ui` : police de l'interface, sinon du code.
    Font { ui: bool, name: String, faces: [Option<FontBytes>; 4] },
    Library { themes: Vec<String>, profiles: Vec<String> },
    /// Texte d'un thème ou d'un profil enregistré.
    Entry { kind: Kind, name: String, text: String },
    /// Fichier `.ferrotheme` lu (ou refusé), et un aperçu réduit de son image (lot E5).
    Package { result: Result<Box<Package>, ShareError>, preview: Option<ColorImage> },
    /// Thème exporté : chemin du fichier, ou raison de l'échec.
    Exported(Result<String, String>),
    /// Thème importé, enregistré comme profil : son texte, à appliquer.
    Installed { name: String, text: String },
    Failed(String),
}

pub struct Look {
    tx: mpsc::Sender<Done>,
    rx: mpsc::Receiver<Done>,
    wake: Wake,
    picking: Arc<AtomicBool>,
    /// Dossier de configuration ; `None` : rien n'est lu ni écrit (aperçu, tests).
    folder: Option<PathBuf>,
    /// Dernière image décodée, réduite, pas encore floutée : régler le flou ne relit pas le fichier.
    source: Arc<Mutex<Option<(String, image::RgbaImage)>>>,
}

impl Look {
    pub fn new(wake: Wake, folder: Option<PathBuf>) -> Self {
        let (tx, rx) = mpsc::channel();
        Look { tx, rx, wake, picking: Arc::default(), folder, source: Arc::default() }
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }

    /// La bibliothèque peut être écrite (un dossier de configuration existe).
    pub fn has_folder(&self) -> bool {
        self.folder.is_some()
    }

    fn spawn(&self, work: impl FnOnce(&mpsc::Sender<Done>) + Send + 'static) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let spawned = std::thread::Builder::new().name("apparence".into()).spawn(move || {
            work(&tx);
            wake();
        });
        // Fil impossible à créer : le geste est perdu, rien d'autre.
        drop(spawned);
    }

    /// Boîte de dialogue « Ouvrir » (une image), dans un fil à part.
    #[cfg(windows)]
    pub fn pick_image(&self, title: &str) {
        if self.picking.swap(true, Ordering::SeqCst) {
            return;
        }
        let (title, picking) = (title.to_string(), self.picking.clone());
        self.spawn(move |tx| {
            let chosen = rfd::FileDialog::new().set_title(title).add_filter("Images", &["png", "jpg", "jpeg", "webp"]).pick_file();
            picking.store(false, Ordering::SeqCst);
            if let Some(path) = chosen {
                let _ = tx.send(Done::Picked(path.display().to_string()));
            }
        });
    }

    #[cfg(not(windows))]
    pub fn pick_image(&self, _: &str) {}

    pub const CAN_PICK: bool = cfg!(windows);

    pub fn picking(&self) -> bool {
        self.picking.load(Ordering::SeqCst)
    }

    /// Lit, réduit et floute l'image de fond.
    pub fn load_backdrop(&self, path: String, blur: u8) {
        let source = self.source.clone();
        self.spawn(move |tx| {
            let cached = source.lock().ok().and_then(|s| s.as_ref().filter(|(p, _)| *p == path).map(|(_, i)| i.clone()));
            let small = cached.or_else(|| {
                let p = Path::new(&path);
                let ok = std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() <= MAX_IMAGE_BYTES);
                let img = shrink(&ok.then(|| std::fs::read(p).ok()).flatten()?)?;
                if let Ok(mut s) = source.lock() {
                    *s = Some((path.clone(), img.clone()));
                }
                Some(img)
            });
            let _ = tx.send(Done::Backdrop { image: small.map(|i| blurred(&i, blur)), path, blur });
        });
    }

    /// Image de fond à partir d'octets en mémoire (aperçu, tests).
    pub fn backdrop_bytes(&self, path: String, bytes: Vec<u8>, blur: u8) {
        self.spawn(move |tx| {
            let _ = tx.send(Done::Backdrop { image: shrink(&bytes).map(|i| blurred(&i, blur)), path, blur });
        });
    }

    /// Liste les familles installées (Windows et utilisateur).
    pub fn list_fonts(&self) {
        self.spawn(|tx| {
            let _ = tx.send(Done::Fonts(families(&font_dirs())));
        });
    }

    /// Lit les fichiers de la famille `name`.
    pub fn load_font(&self, ui: bool, name: String) {
        self.spawn(move |tx| {
            let all = families(&font_dirs());
            let done = match all.into_iter().find(|f| f.name == name) {
                Some(f) => Done::Font { ui, name, faces: f.faces.map(|p| p.and_then(|p| read_capped(&p, MAX_FONT_BYTES)).map(Arc::new)) },
                None => Done::Failed(name),
            };
            let _ = tx.send(done);
        });
    }

    /// Relit la liste des thèmes et des profils enregistrés.
    pub fn list_library(&self) {
        let Some(folder) = self.folder.clone() else { return };
        self.spawn(move |tx| {
            let _ = tx.send(listing(&folder));
        });
    }

    /// Enregistre `text` sous `name` (déjà passé par [`clean_name`]), puis relit la liste.
    pub fn save_entry(&self, kind: Kind, name: String, text: String) {
        let Some(folder) = self.folder.clone() else { return };
        self.spawn(move |tx| {
            let dir = folder.join(kind.dir());
            let written = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(dir.join(format!("{name}.toml")), text));
            let _ = tx.send(match written {
                Ok(()) => listing(&folder),
                Err(e) => Done::Failed(e.to_string()),
            });
        });
    }

    /// Copie un thème ou un profil enregistré sous un autre nom, puis relit la liste.
    pub fn copy_entry(&self, kind: Kind, from: String, to: String) {
        let Some(folder) = self.folder.clone() else { return };
        self.spawn(move |tx| {
            let dir = folder.join(kind.dir());
            let copied = std::fs::copy(dir.join(format!("{from}.toml")), dir.join(format!("{to}.toml")));
            let _ = tx.send(match copied {
                Ok(_) => listing(&folder),
                Err(e) => Done::Failed(e.to_string()),
            });
        });
    }

    pub fn delete_entry(&self, kind: Kind, name: String) {
        let Some(folder) = self.folder.clone() else { return };
        self.spawn(move |tx| {
            let _ = std::fs::remove_file(folder.join(kind.dir()).join(format!("{name}.toml")));
            let _ = tx.send(listing(&folder));
        });
    }

    pub fn load_entry(&self, kind: Kind, name: String) {
        let Some(folder) = self.folder.clone() else { return };
        self.spawn(move |tx| {
            let path = folder.join(kind.dir()).join(format!("{name}.toml"));
            let done = match read_capped(&path, 1 << 20).and_then(|b| String::from_utf8(b).ok()) {
                Some(text) => Done::Entry { kind, name, text },
                None => Done::Failed(path.display().to_string()),
            };
            let _ = tx.send(done);
        });
    }
}

/// Nom d'un thème ou d'un profil : lettres, chiffres, espaces, `-` et `_`
/// (c'est aussi un nom de fichier) ; `None` s'il ne reste rien.
pub fn clean_name(name: &str) -> Option<String> {
    let kept: String = name.chars().filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_')).collect();
    let kept: String = kept.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(NAME_MAX).collect();
    let kept = kept.trim().to_string();
    (!kept.is_empty()).then_some(kept)
}

/// Texte d'un profil d'apparence : le thème, puis la section `[apparence]`.
pub fn profile_text(theme: &str, look: &str) -> String {
    format!("{theme}\n{PROFILE_MARK}{look}")
}

/// Sépare un profil en (thème, section `[apparence]`).
pub fn split_profile(text: &str) -> (&str, &str) {
    match text.split_once(PROFILE_MARK) {
        Some((theme, look)) => (theme, look),
        None => (text, ""),
    }
}

fn listing(folder: &Path) -> Done {
    let names = |kind: Kind| {
        let mut out: Vec<String> = std::fs::read_dir(folder.join(kind.dir()))
            .map(|d| d.flatten().filter_map(|e| e.path().file_name()?.to_str()?.strip_suffix(".toml").map(str::to_string)).filter(|n| clean_name(n).as_deref() == Some(n)).collect())
            .unwrap_or_default();
        out.sort_by_key(|n| n.to_lowercase());
        out
    };
    Done::Library { themes: names(Kind::Theme), profiles: names(Kind::Profile) }
}

fn read_capped(path: &Path, max: u64) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    (meta.is_file() && meta.len() <= max).then(|| std::fs::read(path).ok()).flatten()
}

/// Image décodée et réduite à [`BACKDROP_SIDE`] au plus.
fn shrink(bytes: &[u8]) -> Option<image::RgbaImage> {
    decode::sniff(bytes)?;
    let img = decode::decode(bytes, decode::MAX_SIDE).ok()?;
    let img = if img.width().max(img.height()) > BACKDROP_SIDE { img.resize(BACKDROP_SIDE, BACKDROP_SIDE, image::imageops::FilterType::Triangle) } else { img };
    Some(img.to_rgba8())
}

/// Flou précalculé une fois (rayon en pixels de la texture réduite).
fn blurred(img: &image::RgbaImage, blur: u8) -> ColorImage {
    let out = if blur == 0 { img.clone() } else { image::imageops::fast_blur(img, f32::from(blur)) };
    ColorImage::from_rgba_unmultiplied([out.width() as usize, out.height() as usize], out.as_raw())
}

/// Dossiers des polices : celui de Windows, puis celui de l'utilisateur.
fn font_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if cfg!(windows) {
        if let Some(root) = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("WINDIR")) {
            dirs.push(PathBuf::from(root).join("Fonts"));
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            dirs.push(PathBuf::from(local).join("Microsoft").join("Windows").join("Fonts"));
        }
    }
    dirs
}

/// Familles des fichiers TrueType et OpenType de `dirs`, triées par nom ;
/// seules celles qui ont une graisse normale sont gardées.
pub fn families(dirs: &[PathBuf]) -> Vec<Family> {
    let mut out: Vec<Family> = Vec::new();
    for dir in dirs {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for path in entries.flatten().map(|e| e.path()) {
            let ext = path.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase);
            if !matches!(ext.as_deref(), Some("ttf" | "otf")) {
                continue;
            }
            let Some((name, sub, mono)) = describe(&path) else { continue };
            let Some(slot) = face_slot(&sub) else { continue };
            let i = match out.iter().position(|f| f.name == name) {
                Some(i) => i,
                None => {
                    out.push(Family { name, mono, faces: Default::default() });
                    out.len() - 1
                }
            };
            out[i].faces[slot].get_or_insert(path);
        }
    }
    out.retain(|f| f.faces[0].is_some());
    out.sort_by_key(|f| f.name.to_lowercase());
    out
}

/// Rang d'un style : normale, grasse, italique, grasse italique.
fn face_slot(sub: &str) -> Option<usize> {
    match sub.trim().to_lowercase().as_str() {
        "regular" | "normal" | "roman" | "book" | "" => Some(0),
        "bold" => Some(1),
        "italic" | "oblique" => Some(2),
        "bold italic" | "bold oblique" => Some(3),
        _ => None,
    }
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(usize::from(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// (famille, style, chasse fixe) d'un fichier de police : seules les tables
/// `name` et `post` sont lues, pas le fichier entier.
fn describe(path: &Path) -> Option<(String, String, bool)> {
    let mut file = std::fs::File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_FONT_BYTES {
        return None;
    }
    let mut head = [0u8; 12];
    file.read_exact(&mut head).ok()?;
    if !matches!(&head[..4], [0, 1, 0, 0] | b"true" | b"OTTO") {
        return None;
    }
    let count = be16(&head, 4)?.min(64);
    let mut dir = vec![0u8; count * 16];
    file.read_exact(&mut dir).ok()?;
    let mut table = |tag: &[u8; 4], max: usize| -> Option<Vec<u8>> {
        let rec = dir.as_chunks::<16>().0.iter().find(|r| &r[..4] == tag)?;
        let (offset, len) = (be32(rec, 8)?, (be32(rec, 12)? as usize).min(max));
        let mut buf = vec![0u8; len];
        file.seek(SeekFrom::Start(u64::from(offset))).ok()?;
        file.read_exact(&mut buf).ok()?;
        Some(buf)
    };
    let (family, sub) = parse_names(&table(b"name", 256 << 10)?)?;
    // `post`, octets 12 à 15 : isFixedPitch.
    let mono = table(b"post", 16).and_then(|p| be32(&p, 12)).is_some_and(|v| v != 0);
    Some((family, sub, mono))
}

/// Famille et style lus dans une table `name` (identifiants 16 et 17 s'ils
/// existent, sinon 1 et 2 ; noms Windows en anglais d'abord).
pub fn parse_names(t: &[u8]) -> Option<(String, String)> {
    let (count, strings) = (be16(t, 2)?, be16(t, 4)?);
    // Meilleur rang trouvé pour chaque identifiant (1, 2, 16, 17).
    let mut best: [Option<(u8, String)>; 4] = Default::default();
    for i in 0..count {
        let r = 6 + i * 12;
        let (platform, lang, id, len, off) = (be16(t, r)?, be16(t, r + 4)?, be16(t, r + 6)?, be16(t, r + 8)?, be16(t, r + 10)?);
        let Some(slot) = [1, 2, 16, 17].iter().position(|n| *n == id) else { continue };
        let Some(raw) = t.get(strings + off..strings + off + len) else { continue };
        let (rank, text) = match platform {
            3 => (if lang == 0x409 { 3 } else { 2 }, String::from_utf16_lossy(&raw.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect::<Vec<_>>())),
            1 => (1, raw.iter().map(|b| char::from(*b)).collect()),
            _ => continue,
        };
        let text = text.trim().to_string();
        if !text.is_empty() && best[slot].as_ref().is_none_or(|(r, _)| *r < rank) {
            best[slot] = Some((rank, text));
        }
    }
    let [family, sub, pref_family, pref_sub] = best.map(|b| b.map(|(_, s)| s));
    Some((pref_family.or(family)?, pref_sub.or(sub).unwrap_or_default()))
}

// ---- Thèmes partageables (lot E5) : fichiers `.ferrotheme` ----

/// Côté maximal de l'aperçu de l'image d'un thème à importer.
const PREVIEW_SIDE: u32 = 480;

/// Fichier `.ferrotheme` lu et vérifié, avec un aperçu réduit de son image.
fn package_done(bytes: Option<Vec<u8>>) -> Done {
    let result = match bytes {
        Some(b) => share::parse(&b, crate::i18n::language()).map(Box::new),
        None => Err(ShareError::Unreadable),
    };
    let preview = result.as_ref().ok().and_then(|p| p.image.as_ref()).and_then(|i| shrink(&i.bytes)).map(|img| {
        let w = PREVIEW_SIDE.min(img.width()).max(1);
        let h = (u64::from(w) * u64::from(img.height()) / u64::from(img.width().max(1))).clamp(1, u64::from(decode::MAX_SIDE)) as u32;
        let small = image::imageops::thumbnail(&img, w, h);
        ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw())
    });
    Done::Package { result, preview }
}

/// Lit un fichier de thème : refusé sans être lu s'il dépasse le plafond.
fn read_package_file(path: &Path) -> Done {
    match std::fs::metadata(path) {
        Ok(m) if m.is_file() && m.len() > share::FILE_MAX as u64 => Done::Package { result: Err(ShareError::TooBig), preview: None },
        _ => package_done(read_capped(path, share::FILE_MAX as u64)),
    }
}

/// Image de fond à embarquer dans un thème exporté : réduite comme à
/// l'affichage, puis encodée en JPEG (un fond n'a pas de transparence).
fn embed_image(path: &Path) -> Option<share::Image> {
    let mut img = image::DynamicImage::ImageRgba8(shrink(&read_capped(path, MAX_IMAGE_BYTES)?)?).to_rgb8();
    for _ in 0..3 {
        let mut out = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 85).encode_image(&img).ok()?;
        if out.len() <= share::IMAGE_MAX {
            return Some(share::Image { bytes: Arc::new(out), kind: "jpeg" });
        }
        img = image::imageops::resize(&img, (img.width() / 2).max(1), (img.height() / 2).max(1), image::imageops::FilterType::Triangle);
    }
    None
}

/// Écrit le thème `pkg` dans `path`, avec l'image `image` si elle est demandée.
fn write_package_file(path: &Path, mut pkg: Package, image: Option<String>) -> Done {
    if let Some(src) = image {
        match embed_image(Path::new(&src)) {
            Some(i) => pkg.image = Some(i),
            None => return Done::Exported(Err(crate::i18n::table::e5::t::SHARE_IMAGE_EXPORT_FAILED.get(crate::i18n::language()).into())),
        }
    }
    let text = share::export(&pkg, crate::i18n::language());
    Done::Exported(std::fs::write(path, text).map(|()| path.display().to_string()).map_err(|e| e.to_string()))
}

impl Look {
    /// Lit et vérifie un fichier `.ferrotheme` (déposé sur la fenêtre).
    pub fn read_package(&self, path: PathBuf) {
        self.spawn(move |tx| {
            let _ = tx.send(read_package_file(&path));
        });
    }

    /// Vérifie un thème déjà en mémoire (aperçu, tests).
    pub fn parse_package(&self, bytes: Vec<u8>) {
        self.spawn(move |tx| {
            let _ = tx.send(package_done(Some(bytes)));
        });
    }

    /// Boîte « Ouvrir » (un fichier `.ferrotheme`), puis lecture et vérification.
    #[cfg(windows)]
    pub fn pick_package(&self, title: &str) {
        if self.picking.swap(true, Ordering::SeqCst) {
            return;
        }
        let (title, picking) = (title.to_string(), self.picking.clone());
        self.spawn(move |tx| {
            let chosen = rfd::FileDialog::new().set_title(title).add_filter("Ferrocord", &[share::EXT]).pick_file();
            picking.store(false, Ordering::SeqCst);
            if let Some(path) = chosen {
                let _ = tx.send(read_package_file(&path));
            }
        });
    }

    #[cfg(not(windows))]
    pub fn pick_package(&self, _: &str) {}

    /// Boîte « Enregistrer sous », puis écriture du thème ; `image` : chemin
    /// de l'image de fond à embarquer.
    #[cfg(windows)]
    pub fn export_package(&self, title: &str, pkg: Package, image: Option<String>) {
        if self.picking.swap(true, Ordering::SeqCst) {
            return;
        }
        let (title, picking) = (title.to_string(), self.picking.clone());
        self.spawn(move |tx| {
            let chosen = rfd::FileDialog::new().set_title(title).add_filter("Ferrocord", &[share::EXT]).set_file_name(format!("{}.{}", pkg.name, share::EXT)).save_file();
            picking.store(false, Ordering::SeqCst);
            if let Some(path) = chosen {
                let _ = tx.send(write_package_file(&path, pkg, image));
            }
        });
    }

    #[cfg(not(windows))]
    pub fn export_package(&self, _: &str, _: Package, _: Option<String>) {}

    /// Écrit le thème dans `path` sans boîte de dialogue (tests).
    pub fn write_package(&self, path: PathBuf, pkg: Package, image: Option<String>) {
        self.spawn(move |tx| {
            let _ = tx.send(write_package_file(&path, pkg, image));
        });
    }

    /// Installe un thème importé comme profil d'apparence `name` (déjà passé
    /// par [`clean_name`]) : son image, s'il en a une, est copiée dans le
    /// dossier `fonds` de Ferrocord, puis le profil est écrit et rendu pour
    /// être appliqué. Le chemin d'image de `look` est ignoré.
    pub fn install_package(&self, name: String, theme: String, mut look: crate::state::look_prefs::LookPrefs, image: Option<share::Image>) {
        let Some(folder) = self.folder.clone() else { return };
        let source = self.source.clone();
        self.spawn(move |tx| {
            let written = (|| {
                look.backdrop.path.clear();
                if let Some(img) = &image {
                    let dir = folder.join("fonds");
                    std::fs::create_dir_all(&dir)?;
                    let file = dir.join(format!("{name}.{}", if img.kind == "jpeg" { "jpg" } else { img.kind }));
                    std::fs::write(&file, img.bytes.as_slice())?;
                    look.backdrop.path = file.display().to_string();
                    // Une image d'avant, de même chemin, n'est plus la bonne.
                    if let Ok(mut s) = source.lock() {
                        *s = None;
                    }
                }
                let text = profile_text(&theme, &look.to_toml());
                let dir = folder.join(Kind::Profile.dir());
                std::fs::create_dir_all(&dir)?;
                std::fs::write(dir.join(format!("{name}.toml")), &text)?;
                Ok::<String, std::io::Error>(text)
            })();
            match written {
                Ok(text) => {
                    let _ = tx.send(listing(&folder));
                    let _ = tx.send(Done::Installed { name, text });
                }
                Err(e) => {
                    let _ = tx.send(Done::Failed(e.to_string()));
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fichier de thème écrit avec son image, relu, puis installé comme
    /// profil : l'image est copiée dans le dossier de Ferrocord, et le profil
    /// ne désigne qu'elle.
    #[test]
    fn package_export_import_install() {
        let dir = std::env::temp_dir().join(format!("ferrocord-partage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let look = Look::new(Arc::new(|| {}), Some(dir.join("config")));
        let wait = |look: &Look| look.rx.recv_timeout(std::time::Duration::from_secs(30)).ok();
        // Image de fond de l'utilisateur, hors du dossier de Ferrocord.
        let source = dir.join("ma photo.png");
        let src = image::RgbaImage::from_fn(2400, 600, |x, y| image::Rgba([(x / 10) as u8, (y / 3) as u8, 80, 255]));
        image::DynamicImage::ImageRgba8(src).save(&source).unwrap();
        let mut prefs = crate::state::look_prefs::LookPrefs::default();
        prefs.backdrop.path = source.display().to_string();
        prefs.gradient.on = true;
        let pkg = Package { name: "Essai".into(), author: "Camille".into(), tokens: crate::theme::Preset::Onyx.tokens(), look: share::shared(&prefs), image: None };
        let file = dir.join("Essai.ferrotheme");
        look.write_package(file.clone(), pkg.clone(), Some(prefs.backdrop.path.clone()));
        assert!(matches!(wait(&look), Some(Done::Exported(Ok(p))) if p == file.display().to_string()));
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(!text.contains("ma photo") && text.contains("[image]"), "le chemin de l'image reste sur cet ordinateur");

        look.read_package(file.clone());
        let Some(Done::Package { result: Ok(read), preview: Some(preview) }) = wait(&look) else { panic!("thème refusé") };
        assert_eq!((read.name.as_str(), read.author.as_str(), &read.tokens, &read.look), ("Essai", "Camille", &pkg.tokens, &pkg.look));
        let image = read.image.clone().unwrap();
        assert_eq!(image.kind, "jpeg");
        assert_eq!(preview.size, [480, 120], "aperçu réduit");
        assert_eq!(decode::decode(&image.bytes, decode::MAX_SIDE).map(|i| (i.width(), i.height())).ok(), Some((1600, 400)), "image réduite avant d'être embarquée");

        // Installé : image copiée dans `fonds`, profil ajouté et rendu pour être appliqué.
        let mut mine = share::merged(&crate::state::look_prefs::LookPrefs::default(), &read.look, r"C:\ailleurs\piege.png".into());
        mine.hide_rail = true;
        look.install_package("Essai".into(), crate::theme::export(&read.tokens), mine, read.image.clone());
        assert!(matches!(wait(&look), Some(Done::Library { profiles, .. }) if profiles == ["Essai"]));
        let Some(Done::Installed { name, text }) = wait(&look) else { panic!("pas installé") };
        let (theme, extra) = split_profile(&text);
        let back = crate::state::look_prefs::LookPrefs::parse(extra).unwrap();
        let copied = dir.join("config").join("fonds").join("Essai.jpg");
        assert_eq!((name.as_str(), back.backdrop.path.as_str(), back.hide_rail, back.gradient.on), ("Essai", copied.display().to_string().as_str(), true, true));
        assert_eq!(std::fs::read(&copied).unwrap(), *image.bytes);
        assert_eq!(crate::theme::parse(theme), Ok(read.tokens.clone()));

        // Fichier absent, trop gros, ou qui n'est pas un thème : refusé, sans panique.
        look.read_package(dir.join("absent.ferrotheme"));
        assert!(matches!(wait(&look), Some(Done::Package { result: Err(ShareError::Unreadable), .. })));
        let big = dir.join("gros.ferrotheme");
        std::fs::write(&big, vec![b'#'; share::FILE_MAX + 1]).unwrap();
        look.read_package(big);
        assert!(matches!(wait(&look), Some(Done::Package { result: Err(ShareError::TooBig), .. })));
        look.parse_package(b"MZ\x90\x00pas un theme".to_vec());
        assert!(matches!(wait(&look), Some(Done::Package { result: Err(_), preview: None })));
        // Image de fond disparue : rien n'est exporté.
        look.write_package(dir.join("sans.ferrotheme"), pkg, Some(dir.join("absente.png").display().to_string()));
        assert!(matches!(wait(&look), Some(Done::Exported(Err(_)))));
        assert!(!dir.join("sans.ferrotheme").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Table `name` synthétique : (plateforme, langue, identifiant, texte).
    fn name_table(records: &[(u16, u16, u16, &str)]) -> Vec<u8> {
        let mut strings = Vec::new();
        let mut recs = Vec::new();
        for (platform, lang, id, text) in records {
            let raw: Vec<u8> = if *platform == 3 { text.encode_utf16().flat_map(u16::to_be_bytes).collect() } else { text.bytes().collect() };
            for v in [*platform, 1, *lang, *id, raw.len() as u16, strings.len() as u16] {
                recs.extend_from_slice(&v.to_be_bytes());
            }
            strings.extend(raw);
        }
        let mut t = Vec::new();
        for v in [0u16, records.len() as u16, 6 + recs.len() as u16] {
            t.extend_from_slice(&v.to_be_bytes());
        }
        t.extend(recs);
        t.extend(strings);
        t
    }

    #[test]
    fn names_are_read_from_the_name_table() {
        let t = name_table(&[(1, 0, 1, "Mac Name"), (3, 0x40c, 1, "Nom français"), (3, 0x409, 1, "Cascadia Mono"), (3, 0x409, 2, "Bold Italic")]);
        assert_eq!(parse_names(&t), Some(("Cascadia Mono".into(), "Bold Italic".into())));
        // Les identifiants 16 et 17 passent devant 1 et 2.
        let t = name_table(&[(3, 0x409, 1, "Inter Light"), (3, 0x409, 2, "Regular"), (3, 0x409, 16, "Inter"), (3, 0x409, 17, "Light")]);
        assert_eq!(parse_names(&t), Some(("Inter".into(), "Light".into())));
        assert_eq!(parse_names(&name_table(&[(1, 0, 1, "Seule")])), Some(("Seule".into(), String::new())));
        assert_eq!(parse_names(&[0, 0, 0]), None);
        assert_eq!(parse_names(&name_table(&[(3, 0x409, 2, "Bold")])), None);
        // Table tronquée : pas de panique.
        let t = name_table(&[(3, 0x409, 1, "Cascadia Mono")]);
        for cut in 0..t.len() {
            let _ = parse_names(&t[..cut]);
        }
        assert_eq!([face_slot("Regular"), face_slot("Bold"), face_slot("Italic"), face_slot("Bold Italic"), face_slot("Light")], [Some(0), Some(1), Some(2), Some(3), None]);
    }

    #[test]
    fn names_of_saved_entries() {
        assert_eq!(clean_name("  Mon   thème d'été / 2 "), Some("Mon thème dété 2".into()));
        assert_eq!(clean_name("..\\..\\x"), Some("x".into()));
        assert_eq!(clean_name(" /\\: "), None);
        assert_eq!(clean_name(&"a".repeat(100)).map(|n| n.len()), Some(NAME_MAX));
        let text = profile_text("base = \"sombre\"\n", "[apparence]\ndegrade = true\n");
        assert_eq!(split_profile(&text), ("base = \"sombre\"\n\n", "[apparence]\ndegrade = true\n"));
        assert_eq!(split_profile("base = \"sombre\"\n"), ("base = \"sombre\"\n", ""));
    }

    #[test]
    fn library_round_trip_and_backdrop() {
        let dir = std::env::temp_dir().join(format!("ferrocord-apparence-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let look = Look::new(Arc::new(|| {}), Some(dir.clone()));
        let wait = |look: &Look| look.rx.recv_timeout(std::time::Duration::from_secs(20)).ok();
        look.save_entry(Kind::Theme, "Été".into(), "base = \"clair\"\n".into());
        assert!(matches!(wait(&look), Some(Done::Library { themes, profiles }) if themes == ["Été"] && profiles.is_empty()));
        look.load_entry(Kind::Theme, "Été".into());
        assert!(matches!(wait(&look), Some(Done::Entry { kind: Kind::Theme, text, .. }) if text == "base = \"clair\"\n"));
        look.delete_entry(Kind::Theme, "Été".into());
        assert!(matches!(wait(&look), Some(Done::Library { themes, .. }) if themes.is_empty()));
        look.load_entry(Kind::Profile, "absent".into());
        assert!(matches!(wait(&look), Some(Done::Failed(_))));

        // Image de fond : réduite au côté maximal, floutée, une seule image.
        let mut png = Vec::new();
        let src = image::RgbaImage::from_fn(2000, 500, |x, _| image::Rgba([if x < 1000 { 0 } else { 255 }, 0, 0, 255]));
        image::DynamicImage::ImageRgba8(src).write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png).unwrap();
        look.backdrop_bytes("mem".into(), png, 6);
        let Some(Done::Backdrop { image: Some(img), blur: 6, .. }) = wait(&look) else { panic!("pas d'image") };
        assert_eq!(img.size, [1600, 400]);
        // Le flou a mêlé les deux moitiés à la frontière.
        let mid = img.pixels[200 * 1600 + 800];
        assert!(mid.r() > 20 && mid.r() < 235, "{mid:?}");
        look.backdrop_bytes("mem".into(), b"pas une image".to_vec(), 0);
        assert!(matches!(wait(&look), Some(Done::Backdrop { image: None, .. })));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
