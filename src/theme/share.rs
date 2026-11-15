//! Thèmes partageables (lot E5, plugin `themes_partageables`) : un thème
//! complet dans un seul fichier `.ferrotheme`, lisible et modifiable à la main.
//!
//! C'est un fichier TOML : le thème tel que `theme.toml` (couleurs, rayons,
//! espacements, densité…), précédé de trois clés (`ferrotheme` : version du
//! format, `nom`, `auteur` facultatif) et suivi de deux sections facultatives :
//! `[apparence]` (dégradé, réglages de l'image, polices choisies) et `[image]`
//! (l'image de fond, en base64).
//!
//! Lecture **stricte**, pure, sans E/S, et qui ne panique sur aucune entrée :
//! - taille du fichier et de l'image bornées avant tout travail ;
//! - le thème est lu par l'analyseur de `theme.toml` (clé inconnue, valeur
//!   hors bornes, couleur illisible : fichier refusé en entier) ;
//! - `[apparence]` passe par `LookPrefs::from_table` (valeurs ramenées dans
//!   leurs bornes), et seul ce qui se partage en est gardé : **aucun chemin
//!   de fichier n'est repris ni suivi** ;
//! - l'image est reconnue à ses premiers octets et décodée par le décodeur
//!   des images du client, avec ses plafonds ; rien d'autre qu'une image PNG,
//!   JPEG ou WebP n'est accepté ;
//! - un fichier ne contient aucun code et n'en déclenche aucun.

use super::{export_in, parse_skipping, Tokens};
use crate::i18n::table::e5::{f as ef, t as et};
use crate::i18n::Lang;
use crate::images::decode;
use crate::state::look_prefs::{Backdrop, LookPrefs, SECTION as LOOK};
use std::sync::Arc;
use toml::de::{DeTable, DeValue};

/// Extension des fichiers de thème partageables.
pub const EXT: &str = "ferrotheme";
/// Version du format écrit ; un fichier d'une autre version est refusé.
pub const FORMAT: i64 = 1;
/// Fichier plus gros : refusé sans être analysé.
pub const FILE_MAX: usize = 8 << 20;
/// Image embarquée plus grosse (une fois décodée du base64) : refusée.
pub const IMAGE_MAX: usize = 4 << 20;
/// Longueurs maximales, en caractères.
pub const AUTHOR_MAX: usize = 60;
pub const FONT_MAX: usize = 64;

const K_FORMAT: &str = "ferrotheme";
const K_NAME: &str = "nom";
const K_AUTHOR: &str = "auteur";
const S_IMAGE: &str = "image";
const K_TYPE: &str = "type";
const K_DATA: &str = "donnees";
/// Ce que l'analyseur du thème laisse à ce module.
const OURS: [&str; 5] = [K_FORMAT, K_NAME, K_AUTHOR, LOOK, S_IMAGE];
const TYPES: [(&str, image::ImageFormat); 3] = [("png", image::ImageFormat::Png), ("jpeg", image::ImageFormat::Jpeg), ("webp", image::ImageFormat::WebP)];

/// Thèmes fournis avec Ferrocord (galerie « Thèmes fournis ») : (clé, fichier).
pub const BUNDLED: [(&str, &str); 4] = [
    ("braise", include_str!("../../assets/themes/braise.ferrotheme")),
    ("lagon", include_str!("../../assets/themes/lagon.ferrotheme")),
    ("aurore", include_str!("../../assets/themes/aurore.ferrotheme")),
    ("papier", include_str!("../../assets/themes/papier.ferrotheme")),
];

/// Image de fond embarquée : ses octets tels quels, déjà vérifiés.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub bytes: Arc<Vec<u8>>,
    /// `png`, `jpeg` ou `webp` (aussi l'extension du fichier une fois installée).
    pub kind: &'static str,
}

/// Contenu d'un fichier `.ferrotheme`.
#[derive(Clone, Debug, PartialEq)]
pub struct Package {
    /// Nom, déjà passé par [`crate::look::clean_name`].
    pub name: String,
    pub author: String,
    pub tokens: Tokens,
    /// Ce qui se partage de l'apparence ([`shared`]) : jamais de chemin.
    pub look: LookPrefs,
    pub image: Option<Image>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShareError {
    TooBig,
    NotText,
    /// Pas un fichier `.ferrotheme` (clé `ferrotheme` absente).
    NotATheme,
    /// Version de format inconnue.
    Format,
    Name,
    Author,
    Font,
    /// Section `[apparence]` ou `[image]` mal formée.
    Section(&'static str),
    /// Refus de l'analyseur du thème (ligne et raison, déjà dans la langue).
    Theme(String),
    ImageTooBig,
    ImageUnreadable,
    /// Le fichier n'a pas pu être lu sur le disque.
    Unreadable,
}

impl ShareError {
    pub fn message(&self, lang: Lang) -> String {
        match self {
            ShareError::TooBig => ef::share_too_big(lang, FILE_MAX >> 20),
            ShareError::NotText => et::SHARE_NOT_TEXT.get(lang).into(),
            ShareError::NotATheme => et::SHARE_NOT_A_THEME.get(lang).into(),
            ShareError::Format => ef::share_bad_format(lang, FORMAT),
            ShareError::Name => ef::share_bad_name(lang, crate::look::NAME_MAX),
            ShareError::Author => ef::share_bad_author(lang, AUTHOR_MAX),
            ShareError::Font => ef::share_bad_font(lang, FONT_MAX),
            ShareError::Section(s) => ef::share_bad_section(lang, s),
            ShareError::Theme(e) => e.clone(),
            ShareError::ImageTooBig => ef::share_image_too_big(lang, IMAGE_MAX >> 20),
            ShareError::ImageUnreadable => et::SHARE_IMAGE_UNREADABLE.get(lang).into(),
            ShareError::Unreadable => et::SHARE_UNREADABLE.get(lang).into(),
        }
    }
}

/// Ce qui se partage des réglages d'apparence : dégradé, réglages de l'image
/// (sans son chemin), polices choisies. Le reste (bascule jour et nuit,
/// détails d'affichage) reste propre à chacun.
pub fn shared(l: &LookPrefs) -> LookPrefs {
    LookPrefs { gradient: l.gradient.clone(), backdrop: Backdrop { path: String::new(), ..l.backdrop.clone() }, ui_font: l.ui_font.clone(), code_font: l.code_font.clone(), ..LookPrefs::default() }
}

/// Réglages d'apparence après l'application d'un thème : ceux de
/// l'utilisateur, où ce qui se partage vient du thème. `image` : chemin de
/// l'image installée (vide : pas d'image).
pub fn merged(current: &LookPrefs, theme: &LookPrefs, image: String) -> LookPrefs {
    let t = shared(theme);
    LookPrefs { gradient: t.gradient, backdrop: Backdrop { path: image, ..t.backdrop }, ui_font: t.ui_font, code_font: t.code_font, ..current.clone() }
}

/// Un thème importé ne change pas ce qui tient à l'écran et aux habitudes de
/// l'utilisateur : zoom, largeur des colonnes, barre de titre, format de l'heure.
pub fn keep_personal(tokens: &mut Tokens, current: &Tokens) {
    let (l, c) = (&mut tokens.layout, &current.layout);
    (l.zoom, l.channels_width, l.members_width, l.titlebar, l.titlebar_height, l.clock) = (c.zoom, c.channels_width, c.members_width, c.titlebar, c.titlebar_height, c.clock);
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars().filter(|c| !c.is_control()) {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// Texte du fichier, relu à l'identique par [`parse`]. Le chemin de l'image
/// de fond n'y figure jamais.
pub fn export(p: &Package, lang: Lang) -> String {
    let mut out = String::new();
    out.push_str(et::SHARE_FILE_HEADER.get(lang));
    out.push_str(&format!("{K_FORMAT} = {FORMAT}\n{K_NAME} = {}\n", quoted(&p.name)));
    if !p.author.is_empty() {
        out.push_str(&format!("{K_AUTHOR} = {}\n", quoted(&p.author)));
    }
    out.push('\n');
    out.push_str(&export_in(&p.tokens.clone().sanitized(), lang));
    out.push('\n');
    out.push_str(&shared(&p.look).to_toml());
    if let Some(image) = &p.image {
        out.push_str(&format!("\n[{S_IMAGE}]\n{K_TYPE} = \"{}\"\n{K_DATA} = \"\"\"\n", image.kind));
        let b64 = base64_encode(&image.bytes);
        for line in b64.as_bytes().chunks(76) {
            // Alphabet ASCII : chaque tranche est un texte valide.
            out.push_str(std::str::from_utf8(line).unwrap_or_default());
            out.push('\n');
        }
        out.push_str("\"\"\"\n");
    }
    out
}

/// Lit un fichier `.ferrotheme`. Ne panique sur aucune entrée.
pub fn parse(bytes: &[u8], lang: Lang) -> Result<Package, ShareError> {
    if bytes.len() > FILE_MAX {
        return Err(ShareError::TooBig);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| ShareError::NotText)?;
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    if too_deep(text) {
        return Err(ShareError::NotATheme);
    }
    let theme_error = |lang| ShareError::Theme(parse_skipping(text, lang, &OURS).err().map(|e| e.to_string()).unwrap_or_else(|| crate::i18n::t::THEME_SYNTAX.get(lang).into()));
    // Une panique de l'analyseur TOML (tiers) devient un refus ordinaire.
    let (name, author, look, image) = crate::panics::contain(|| read_ours(text)).unwrap_or(Err(Ours::Syntax)).map_err(|e| match e {
        Ours::Syntax => theme_error(lang),
        Ours::Refused(e) => e,
    })?;
    let tokens = parse_skipping(text, lang, &OURS).map_err(|e| ShareError::Theme(e.to_string()))?;
    Ok(Package { name, author, tokens, look, image })
}

/// Tableaux imbriqués sans fin (`[[[[…`) : refusés avant l'analyseur, dont
/// la descente récursive épuiserait la pile. Un thème n'imbrique rien.
fn too_deep(text: &str) -> bool {
    const MAX: usize = 16;
    let (mut depth, mut worst) = (0usize, 0usize);
    for b in text.bytes() {
        match b {
            b'[' | b'{' => {
                depth += 1;
                worst = worst.max(depth);
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            b'\n' => depth = 0,
            _ => {}
        }
    }
    worst > MAX
}

enum Ours {
    Syntax,
    Refused(ShareError),
}

type Head = (String, String, LookPrefs, Option<Image>);

/// Les clés et sections propres au format : nom, auteur, apparence, image.
fn read_ours(text: &str) -> Result<Head, Ours> {
    let root = DeTable::parse(text).map_err(|_| Ours::Syntax)?;
    let root = root.get_ref();
    let get = |key: &str| root.iter().find(|(k, _)| k.get_ref() == key).map(|(_, v)| v.get_ref());
    let refuse = |e| Ours::Refused(e);

    let format = get(K_FORMAT).ok_or(refuse(ShareError::NotATheme))?;
    let format = format.as_integer().and_then(|i| i64::from_str_radix(i.as_str(), i.radix()).ok());
    if format != Some(FORMAT) {
        return Err(refuse(ShareError::Format));
    }
    let raw = get(K_NAME).and_then(DeValue::as_str).ok_or(refuse(ShareError::Name))?;
    let name = crate::look::clean_name(raw).filter(|_| raw.chars().count() <= crate::look::NAME_MAX).ok_or(refuse(ShareError::Name))?;
    let author = match get(K_AUTHOR) {
        None => String::new(),
        Some(v) => {
            let a = v.as_str().filter(|a| a.chars().count() <= AUTHOR_MAX).ok_or(refuse(ShareError::Author))?;
            a.chars().filter(|c| !c.is_control()).collect::<String>().trim().to_string()
        }
    };
    let look = match get(LOOK) {
        None => LookPrefs::default(),
        Some(v) => shared(&LookPrefs::from_table(v.as_table().ok_or(refuse(ShareError::Section(LOOK)))?)),
    };
    if look.ui_font.chars().count() > FONT_MAX || look.code_font.chars().count() > FONT_MAX {
        return Err(refuse(ShareError::Font));
    }
    let image = match get(S_IMAGE) {
        None => None,
        Some(v) => Some(read_image(v.as_table().ok_or(refuse(ShareError::Section(S_IMAGE)))?).map_err(refuse)?),
    };
    Ok((name, author, look, image))
}

fn read_image(t: &DeTable<'_>) -> Result<Image, ShareError> {
    let (mut kind, mut data) = (None, None);
    for (k, v) in t.iter() {
        match k.get_ref().as_ref() {
            K_TYPE => kind = v.get_ref().as_str(),
            K_DATA => data = v.get_ref().as_str(),
            // Rien d'autre : ni chemin, ni adresse.
            _ => return Err(ShareError::Section(S_IMAGE)),
        }
    }
    let (Some(kind), Some(data)) = (kind, data) else { return Err(ShareError::Section(S_IMAGE)) };
    let (kind, format) = TYPES.iter().find(|(k, _)| *k == kind).copied().ok_or(ShareError::ImageUnreadable)?;
    // Bornée avant le décodage : quatre caractères pour trois octets, plus les retours à la ligne.
    if data.len() > IMAGE_MAX / 3 * 4 + IMAGE_MAX / 16 + 64 {
        return Err(ShareError::ImageTooBig);
    }
    let bytes = base64_decode(data).ok_or(ShareError::ImageUnreadable)?;
    if bytes.len() > IMAGE_MAX {
        return Err(ShareError::ImageTooBig);
    }
    // Reconnue à ses premiers octets, puis décodée en entier par le décodeur
    // du client (plafonds de côté et de mémoire, paniques contenues).
    if decode::sniff(&bytes) != Some(format) || decode::decode(&bytes, decode::MAX_SIDE).is_err() {
        return Err(ShareError::ImageUnreadable);
    }
    Ok(Image { bytes: Arc::new(bytes), kind })
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = u32::from(chunk[0]) << 16 | u32::from(*chunk.get(1).unwrap_or(&0)) << 8 | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(B64[(n >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Base64 standard ; les blancs sont ignorés, tout autre caractère refuse le tout.
pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut acc, mut bits, mut padded) = (0u32, 0u32, false);
    for b in text.bytes() {
        let v = match b {
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            b'=' => {
                padded = true;
                continue;
            }
            _ if padded => return None,
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        };
        acc = acc << 6 | u32::from(v);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    // Un seul caractère en trop ne code aucun octet : texte tronqué.
    (bits != 6).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::look_prefs::{Auto, Gradient, Scope};
    use crate::theme::Preset;
    use eframe::egui::Color32;

    const FR: Lang = Lang::Fr;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut out = Vec::new();
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([(x * 7) as u8, (y * 5) as u8, 90, 255]));
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png).unwrap();
        out
    }

    fn sample() -> Package {
        let mut tokens = Preset::Sombre.tokens();
        tokens.colors.accent = Color32::from_rgb(0xe0, 0x7a, 0x2c);
        tokens.shapes.roundness = 1.5;
        let look = LookPrefs {
            gradient: Gradient { on: true, colors: vec![[10, 20, 30], [200, 100, 0]], angle: 120, strength: 40 },
            backdrop: Backdrop { path: String::new(), scope: Scope::App, opacity: 50, blur: 12, dim: 20 },
            ui_font: "Bahnschrift".into(),
            code_font: "Cascadia Mono".into(),
            ..LookPrefs::default()
        };
        Package { name: "Braise d'hiver".replace('\'', " "), author: "Camille \"Cam\" Fictive".into(), tokens, look, image: Some(Image { bytes: Arc::new(png(32, 20)), kind: "png" }) }
    }

    #[test]
    fn base64_round_trip_and_refusals() {
        for len in 0..40usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 11) as u8).collect();
            let text = base64_encode(&bytes);
            assert_eq!(text.len(), len.div_ceil(3) * 4);
            assert_eq!(base64_decode(&text), Some(bytes.clone()), "{len}");
            // Avec des retours à la ligne, sans bourrage.
            let spaced: String = text.trim_end_matches('=').chars().flat_map(|c| [c, '\n']).collect();
            assert_eq!(base64_decode(&spaced), Some(bytes));
        }
        assert_eq!(base64_encode(b"Ferrocord"), "RmVycm9jb3Jk");
        assert_eq!(base64_decode("RmVy*m9j"), None);
        assert_eq!(base64_decode("Rm=Vy"), None);
        assert_eq!(base64_decode("R"), None);
    }

    #[test]
    fn export_import_round_trip() {
        let p = sample();
        for lang in Lang::ALL {
            let text = export(&p, lang);
            assert_eq!(parse(text.as_bytes(), lang), Ok(p.clone()));
        }
        // Sans image ni auteur, pour chaque préréglage.
        for preset in Preset::ALL {
            let p = Package { name: "Essai".into(), author: String::new(), tokens: preset.tokens(), look: LookPrefs::default(), image: None };
            let text = export(&p, FR);
            assert!(!text.contains("\nauteur = ") && !text.contains("[image]"));
            assert_eq!(parse(text.as_bytes(), FR), Ok(p), "{}", preset.key());
        }
    }

    /// Ce qui ne se partage pas ne sort pas du fichier, et n'y est pas lu.
    #[test]
    fn no_path_leaves_or_enters() {
        let mut p = sample();
        p.look.backdrop.path = r"C:\Users\Camille Fictive\Images\fond.png".into();
        p.look.auto = Auto::Clock;
        p.look.hide_rail = true;
        let text = export(&p, FR);
        assert!(!text.contains("Camille Fictive\\Images") && !text.contains("fond.png"), "le chemin de l'image n'est jamais écrit");
        let back = parse(text.as_bytes(), FR).unwrap();
        assert_eq!((back.look.backdrop.path.as_str(), back.look.auto, back.look.hide_rail), ("", Auto::Off, false));
        // Un fichier qui donne un chemin : ignoré, jamais suivi.
        let hostile = text.replace("fond_image = \"\"", "fond_image = \"C:\\\\Windows\\\\win.ini\"");
        assert_ne!(hostile, text);
        assert_eq!(parse(hostile.as_bytes(), FR).unwrap().look.backdrop.path, "");
        // Appliqué : les réglages personnels restent, l'image vient du dossier de Ferrocord.
        let mine = LookPrefs { auto: Auto::System, hide_rail: true, ui_font: "Arial".into(), ..LookPrefs::default() };
        let m = merged(&mine, &back.look, "fonds/x.png".into());
        assert_eq!((m.auto, m.hide_rail, m.ui_font.as_str(), m.backdrop.path.as_str(), m.gradient.on), (Auto::System, true, "Bahnschrift", "fonds/x.png", true));
        let mut tokens = back.tokens.clone();
        let mut current = Preset::Clair.tokens();
        (current.layout.zoom, current.layout.channels_width) = (1.25, 300.0);
        keep_personal(&mut tokens, &current);
        assert_eq!((tokens.layout.zoom, tokens.layout.channels_width, tokens.colors.accent), (1.25, 300.0, back.tokens.colors.accent));
    }

    /// Fichiers tronqués : jamais de panique, et jamais un thème à moitié lu
    /// pris pour le thème entier.
    #[test]
    fn truncated_files_never_panic() {
        let p = sample();
        let text = export(&p, FR);
        let bytes = text.as_bytes();
        let step = (bytes.len() / 400).max(1);
        for cut in (0..bytes.len()).step_by(step) {
            if let Ok(got) = parse(&bytes[..cut], FR) {
                // Coupé entre deux lignes après le nom : un thème partiel mais valide, sans image abîmée.
                assert_eq!(got.name, p.name);
                assert!(got.image.is_none() || got.image == p.image);
            }
        }
        // L'image coupée au milieu de ses données.
        let at = text.find("donnees").unwrap() + 60;
        assert!(parse(&bytes[..at], FR).is_err());
    }

    #[test]
    fn huge_files_and_images_are_refused_before_any_work() {
        let big = vec![b'#'; FILE_MAX + 1];
        assert_eq!(parse(&big, FR), Err(ShareError::TooBig));
        // Un commentaire énorme mais sous le plafond : lu, et refusé pour ce qu'il est.
        let mut filler = vec![b'#'; FILE_MAX - 64];
        filler.extend_from_slice(b"\nferrotheme = 1\n");
        assert_eq!(parse(&filler, FR), Err(ShareError::Name));
        // Image trop grosse : refusée avant le décodage du base64.
        let head = "ferrotheme = 1\nnom = \"Gros\"\n[image]\ntype = \"png\"\ndonnees = \"";
        let text = format!("{head}{}\"\n", "A".repeat(IMAGE_MAX / 3 * 4 + IMAGE_MAX / 8));
        assert_eq!(parse(text.as_bytes(), FR), Err(ShareError::ImageTooBig));
        // Image aux dimensions démesurées (en-tête PNG de 60 000 × 60 000) : le décodeur la refuse.
        let mut bomb = png(4, 4);
        bomb[16..24].copy_from_slice(&[0, 0, 0xea, 0x60, 0, 0, 0xea, 0x60]);
        let text = format!("{head}{}\"\n", base64_encode(&bomb));
        assert_eq!(parse(text.as_bytes(), FR), Err(ShareError::ImageUnreadable));
    }

    #[test]
    fn absurd_values_are_refused() {
        let ok = "ferrotheme = 1\nnom = \"Essai\"\nversion = 3\nbase = \"sombre\"\n";
        assert!(parse(ok.as_bytes(), FR).is_ok());
        let exe = base64_encode(b"MZ\x90\x00\x03\x00\x00\x00 ceci n'est pas une image");
        let gif = base64_encode(b"GIF89a\x01\x00\x01\x00\x00\x00\x00;");
        let png_as_jpeg = base64_encode(&png(2, 2));
        type Check = fn(&ShareError) -> bool;
        let cases: Vec<(String, Check)> = vec![
            ("nom = \"Essai\"\n".into(), |e| *e == ShareError::NotATheme),
            ("ferrotheme = 999\nnom = \"Essai\"\n".into(), |e| *e == ShareError::Format),
            ("ferrotheme = \"1\"\nnom = \"Essai\"\n".into(), |e| *e == ShareError::Format),
            ("ferrotheme = 1\n".into(), |e| *e == ShareError::Name),
            ("ferrotheme = 1\nnom = 12\n".into(), |e| *e == ShareError::Name),
            ("ferrotheme = 1\nnom = \"///\"\n".into(), |e| *e == ShareError::Name),
            (format!("ferrotheme = 1\nnom = \"{}\"\n", "a".repeat(500)), |e| *e == ShareError::Name),
            (format!("ferrotheme = 1\nnom = \"Essai\"\nauteur = \"{}\"\n", "a".repeat(AUTHOR_MAX + 1)), |e| *e == ShareError::Author),
            ("ferrotheme = 1\nnom = \"Essai\"\nauteur = [1]\n".into(), |e| *e == ShareError::Author),
            (format!("{ok}[apparence]\npolice_interface = \"{}\"\n", "a".repeat(FONT_MAX + 1)), |e| *e == ShareError::Font),
            ("ferrotheme = 1\nnom = \"Essai\"\napparence = 3\n".into(), |e| *e == ShareError::Section("apparence")),
            (format!("{ok}[image]\ntype = \"png\"\n"), |e| *e == ShareError::Section("image")),
            (format!("{ok}[image]\ntype = \"png\"\ndonnees = \"AAAA\"\nchemin = \"C:\\\\x.png\"\n"), |e| *e == ShareError::Section("image")),
            (format!("{ok}[image]\ntype = \"exe\"\ndonnees = \"{exe}\"\n"), |e| *e == ShareError::ImageUnreadable),
            (format!("{ok}[image]\ntype = \"png\"\ndonnees = \"{exe}\"\n"), |e| *e == ShareError::ImageUnreadable),
            (format!("{ok}[image]\ntype = \"png\"\ndonnees = \"{gif}\"\n"), |e| *e == ShareError::ImageUnreadable),
            (format!("{ok}[image]\ntype = \"jpeg\"\ndonnees = \"{png_as_jpeg}\"\n"), |e| *e == ShareError::ImageUnreadable),
            (format!("{ok}[image]\ntype = \"png\"\ndonnees = \"pas du base64 !\"\n"), |e| *e == ShareError::ImageUnreadable),
            // Le thème lui-même : aussi strict que theme.toml.
            (format!("{ok}[couleurs]\naccent = \"#zzzzzz\"\n"), |e| matches!(e, ShareError::Theme(m) if m.contains("ligne 6"))),
            (format!("{ok}[mise_en_page]\nzoom = 1e30\n"), |e| matches!(e, ShareError::Theme(_))),
            (format!("{ok}[formes]\nroundness = -4\n"), |e| matches!(e, ShareError::Theme(_))),
            (format!("{ok}[code]\nexecuter = \"calc.exe\"\n"), |e| matches!(e, ShareError::Theme(_))),
            (format!("{ok}script = \"calc.exe\"\n"), |e| matches!(e, ShareError::Theme(_))),
            ("ferrotheme = 1\nnom = \"Essai\"\nbase = \"inconnu\"\n".into(), |e| matches!(e, ShareError::Theme(_))),
            ("ferrotheme = 1\nnom = \"Essai\nversion = 3\n".into(), |e| matches!(e, ShareError::Theme(_))),
        ];
        for (text, expected) in cases {
            let got = parse(text.as_bytes(), FR);
            assert!(got.as_ref().is_err_and(expected), "{}\n→ {got:?}", text.chars().take(200).collect::<String>());
            assert!(!got.unwrap_err().message(FR).is_empty());
        }
        // `[apparence]` : valeurs ramenées dans leurs bornes, clé inconnue ignorée.
        let odd = format!("{ok}[apparence]\ndegrade_angle = 9000\nfond_flou = 999\nfond_opacite = -5\ndegrade_couleurs = [\"#000000\"]\ninconnu = 1\n");
        let got = parse(odd.as_bytes(), FR).unwrap();
        assert_eq!((got.look.gradient.angle, got.look.backdrop.blur, got.look.backdrop.opacity, got.look.gradient.colors.len()), (360, 40, 0, Gradient::default().colors.len()));
    }

    #[test]
    fn garbage_never_panics() {
        assert_eq!(parse(&[0xff, 0xfe, 0x00, 0x41], FR), Err(ShareError::NotText));
        assert!(parse(b"", FR).is_err());
        assert!(parse(&[0u8; 4096], FR).is_err());
        // Imbrication sans fin : refusée avant l'analyseur.
        let deep = format!("ferrotheme = 1\nnom = \"x\"\na = {}", "[".repeat(200_000));
        assert_eq!(parse(deep.as_bytes(), FR), Err(ShareError::NotATheme));
        let deep = format!("ferrotheme = 1\nnom = \"x\"\na = {}", "{a=".repeat(100_000));
        assert_eq!(parse(deep.as_bytes(), FR), Err(ShareError::NotATheme));
        // Octets pseudo-aléatoires, et un fichier valide abîmé octet par octet.
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..200 {
            let noise: Vec<u8> = (0..(next() % 600) as usize).map(|_| next() as u8).collect();
            let _ = parse(&noise, FR);
        }
        let good = export(&sample(), FR).into_bytes();
        for _ in 0..300 {
            let mut bad = good.clone();
            let at = (next() % bad.len() as u64) as usize;
            bad[at] = next() as u8;
            let _ = parse(&bad, FR);
        }
    }

    /// Les thèmes fournis se lisent, portent un nom et un auteur, et n'embarquent pas d'image.
    #[test]
    fn bundled_themes_are_valid() {
        let mut names = Vec::new();
        for (key, text) in BUNDLED {
            let p = parse(text.as_bytes(), FR).unwrap_or_else(|e| panic!("{key} : {}", e.message(FR)));
            assert!(!p.name.is_empty() && !p.author.is_empty() && p.image.is_none(), "{key}");
            assert!(!names.contains(&p.name), "{key}");
            names.push(p.name);
        }
    }
}
