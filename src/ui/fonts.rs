//! Polices. Par défaut, Figtree (police libre, licence OFL 1.1,
//! `assets/fonts/`), embarquée : un seul fichier variable de 63 Ko, dont
//! l'axe de graisse donne la normale (400), la moyenne (500), la demi-grasse
//! (600) et la grasse (700). Elle n'a pas d'italique : egui penche la
//! normale. Segoe UI de Windows (lue au démarrage dans un fil à part,
//! projetée en mémoire : pas de copie privée) fournit les glyphes qui lui
//! manquent, et toute l'interface si le thème demande la police du système
//! (`typographie.family = "systeme"`).
//! Derrière elles, les polices de Windows des autres écritures, des symboles
//! et des emojis (voir « Repli » plus bas) : sans elles, tout caractère hors
//! de Segoe UI était un carré.
//!
//! La graisse moyenne (500 : boutons, noms, salons dans le client de bureau)
//! n'existe pas en Segoe UI : elle vient de Segoe UI Variable (Windows 11),
//! réglée sur son axe de graisse ; sans elle, la normale. Le code est en
//! Consolas, comme dans le client sous Windows.
//!
//! Les styles de nom de Discord ont leurs polices à eux : douze polices
//! libres embarquées (`assets/fonts/noms/`), chargées à la demande (voir
//! « Polices de nom » plus bas).
//!
//! Les familles [`BOLD`], [`ITALIC`] et [`BOLD_ITALIC`] existent dès le
//! départ (sur la police intégrée) : une mise en page qui les demande avant
//! la fin du chargement, ou sans Segoe UI, ne panique pas ; [`real`] dit si
//! la vraie graisse est là (sinon le gras reste marqué par sa couleur).

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId};
use std::path::{Path, PathBuf};
use std::sync::Arc;

const BOLD: &str = "ferrocord-gras";
const ITALIC: &str = "ferrocord-italique";
const BOLD_ITALIC: &str = "ferrocord-gras-italique";
/// Demi-grasse : noms, titres, lignes non lues (`docs/design.md`, typographie).
const DEMI: &str = "ferrocord-demi-gras";
/// Moyenne (500) : boutons, lignes de menu, infobulles ; noms et salons des thèmes calqués.
const MEDIUM: &str = "ferrocord-moyen";
/// Rang, dans [`FILES`], de la police variable de la graisse moyenne et de la police du code.
const VARIABLE: usize = 5;
const CODE: usize = 6;
/// Nombre de fichiers lus.
const N: usize = 7;

/// Fichiers de `Fonts` : Segoe UI normale, grasse, italique, grasse italique,
/// demi-grasse ; Segoe UI Variable (graisse moyenne) ; Consolas (code).
#[cfg(not(target_os = "android"))]
const FILES: [&str; N] = ["segoeui.ttf", "segoeuib.ttf", "segoeuii.ttf", "segoeuiz.ttf", "seguisb.ttf", "SegUIVar.ttf", "consola.ttf"];
/// Android : Roboto, police du système (`/system/fonts`). Depuis Android 12,
/// un seul fichier variable : la graisse normale seule, le gras par la couleur.
#[cfg(target_os = "android")]
const FILES: [&str; N] = ["Roboto-Regular.ttf", "Roboto-Bold.ttf", "Roboto-Italic.ttf", "Roboto-BoldItalic.ttf", "Roboto-Medium.ttf", "Roboto-Medium.ttf", "DroidSansMono.ttf"];
/// Un fichier de police plus gros est ignoré (Segoe UI fait moins de 1 Mo).
const MAX_FONT_BYTES: u64 = 8 << 20;

/// Figtree, fichier variable (axe `wght` de 300 à 900).
const FIGTREE: &[u8] = include_bytes!("../../assets/fonts/Figtree-wght.ttf");
/// Graisses de Figtree instanciées : normale, moyenne, demi-grasse, grasse.
const FIGTREE_WEIGHTS: [(&str, f32); 4] = [("figtree-400", 400.0), ("figtree-500", 500.0), ("figtree-600", 600.0), ("figtree-700", 700.0)];

/// Contenu d'un fichier de police, partagé sans copie (comme `Blob` d'epaint).
type Bytes = Arc<dyn AsRef<[u8]> + Send + Sync>;

/// Graisse et pente demandées.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Face {
    pub bold: bool,
    pub italic: bool,
}

/// Vraies polices installées (sinon : police intégrée, gras par la couleur,
/// italique penché par egui).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Real {
    pub bold: bool,
    pub italic: bool,
}

/// Polices installées depuis le lancement : les mises en page gardées
/// (`text_cache`) ne valent plus quand elles changent.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

pub(super) fn generation() -> u64 {
    GENERATION.load(std::sync::atomic::Ordering::Relaxed)
}

fn real_id() -> egui::Id {
    egui::Id::unique("ferrocord-polices-chargees")
}

/// Ce qui est vraiment installé à cette image.
pub(super) fn real(ctx: &egui::Context) -> Real {
    ctx.data(|d| d.get_temp::<Real>(real_id())).unwrap_or_default()
}

/// Police de `size` points pour cette graisse et cette pente (famille
/// proportionnelle seulement ; le code garde la chasse fixe).
pub(super) fn font(size: f32, face: Face) -> FontId {
    let family = match (face.bold, face.italic) {
        (false, false) => FontFamily::Proportional,
        (true, false) => FontFamily::Name(BOLD.into()),
        (false, true) => FontFamily::Name(ITALIC.into()),
        (true, true) => FontFamily::Name(BOLD_ITALIC.into()),
    };
    FontId::new(size, family)
}

/// Famille demi-grasse, pour un `RichText` (`.family(fonts::demi())`) : elle
/// existe toujours, sur la police intégrée tant que Segoe UI n'est pas lue.
pub(crate) fn demi() -> FontFamily {
    FontFamily::Name(DEMI.into())
}

/// Police demi-grasse de `size` points.
pub(crate) fn demi_font(size: f32) -> FontId {
    FontId::new(size, demi())
}

/// Famille moyenne (500), pour un `RichText` : elle existe toujours, sur la
/// normale tant que Segoe UI Variable n'est pas lue (ou absente).
pub(crate) fn medium() -> FontFamily {
    FontFamily::Name(MEDIUM.into())
}

/// Police moyenne de `size` points.
pub(crate) fn medium_font(size: f32) -> FontId {
    FontId::new(size, medium())
}

/// Jeu d'essai des glyphes (scène `glyphes` de l'aperçu et test de
/// couverture) : ce que les pseudos, noms de serveurs et messages de Discord
/// contiennent couramment hors du latin de base.
pub(super) const SAMPLES: &[(&str, &str)] = &[
    ("Latin étendu", "Ærøskøbing Łódź Ștefan Đặng ẞ ǆ ɐɔǝ ʃʒ ɸ θ ð þ ĳ œ ſ ﬁ ﬂ"),
    ("Diacritiques combinants", "e\u{301} a\u{308} n\u{303} o\u{302}\u{301} q\u{307}\u{323} x\u{35B} Z\u{351}\u{36B}\u{343}\u{36A}\u{302}\u{36B}\u{33D}\u{334}\u{319}\u{324}\u{31E}\u{349}\u{35A}\u{32F}\u{31E}\u{320}\u{34D}a\u{36B}\u{357}\u{334}\u{362}\u{335}\u{31C}\u{330}\u{354}l\u{368}\u{367}\u{369}\u{358}\u{320}g\u{311}\u{357}\u{30E}\u{305}\u{35B}\u{341}\u{334}\u{33B}\u{348}\u{34D}\u{354}\u{339}o\u{342}\u{30C}\u{30C}\u{358}\u{328}\u{335}\u{336}\u{319}\u{33A}"),
    ("Cyrillique, grec", "Привет, мир! Ёжик Ґанок Њ Ђ Ӝ Қазақ · Καλημέρα κόσμε ἀρχή ᾠδή"),
    ("Arabe, hébreu", "مرحبا بالعالم 123 abc · שלום עולם · فارسی گچپژ"),
    ("Chinois", "你好，世界！简体中文 繁體中文 龍 鬱 𠮷野家"),
    ("Japonais", "こんにちは世界 カタカナ ｶﾀｶﾅ 日本語「テスト」ー々〆"),
    ("Coréen", "안녕하세요 세계 한국어 ㄱㄴㄷ ㅋㅋㅋ"),
    ("Thaï, devanagari", "สวัสดีชาวโลก ภาษาไทย ก็ ที่ · नमस्ते दुनिया हिन्दी क्षत्रिय · বাংলা தமிழ்"),
    ("Autres écritures", "ქართული Հայերեն አማርኛ ᏣᎳᎩ ᐃᓄᒃᑎᑐᑦ မြန်မာ ខ្មែរ ລາວ བོད ᠮᠣᠩᠭᠣᠯ ᚠᚢᚦ"),
    ("Alphabets mathématiques", "𝓐𝓑𝓒 𝓪𝓫𝓬 𝕬𝕭𝕮 𝖆𝖇𝖈 𝔸𝔹ℂ 𝕒𝕓𝕔 𝐀𝐁𝐂 𝑨𝑩𝑪 𝘈𝘉𝘊 𝙰𝙱𝙲 𝟘𝟙𝟚 𝟎𝟏𝟐"),
    ("Lettres cerclées, pleine chasse", "🅰🅱🅲 🅐🅑🅒 🄰🄱🄲 ⒶⒷⒸ ⓐⓑⓒ ①②③ ❶❷❸ ⑴⑵ ＡＢＣ ａｂｃ １２３"),
    ("Petites capitales, exposants", "ᴀʙᴄᴅᴇꜰɢʜɪᴊᴋʟᴍɴᴏᴘǫʀꜱᴛᴜᴠᴡxʏᴢ x² x³ ⁰¹⁴⁵ⁿ ᵃᵇᶜᵈᵉ ₀₁₂₃ₐₑₒ ˢᵘᵖ"),
    ("Symboles", "★ ☆ ♡ ♥ ✦ ✧ ➤ ☾ ⚔ ✿ ❀ ツ ♪ ♫ ™ ® © ← → ↑ ↓ ↔ ⇒ ✓ ✗ ✔ ✘ • · ‣ ∞ ≠ ≈ ± ∑ √ ∫ ∀ ∃ ⨁ ⟨⟩ ⌘ ⏎ ⚠ ☠ ☯ ☮ ♔ ♞ † ‡ § ¶ ‽ № ℃ ℉ € £ ¥ ₿ ₹ ₽"),
    ("Blocs, boîtes, braille", "░ ▒ ▓ █ ▀ ▄ ▌ ▐ ■ □ ▪ ▫ ▲ △ ▼ ▽ ◆ ◇ ○ ● ◐ ◯ ┌─┬─┐ │ ├─┼─┤ └─┴─┘ ╔═╦═╗ ║ ╚═╩═╝ ╭─╮ ╰─╯ ⠓⠑⠇⠇⠕ ⣿⣷⣄⡀"),
    ("Kaomoji", "ʕ•ᴥ•ʔ (╯°□°)╯︵ ┻━┻ ¯\\_(ツ)_/¯ ಠ_ಠ (ノಠ益ಠ)ノ ( ͡° ͜ʖ ͡°) (づ｡◕‿‿◕｡)づ ٩(◕‿◕｡)۶ ◉_◉ ლ(ಠ益ಠლ) (≧▽≦) ꒰ᐢ. .ᐢ꒱ ♡⸜(˶˃ ᵕ ˂˶)⸝♡"),
    ("Emojis simples", "😀 😂 🔥 👍 ❤ ❤\u{FE0F} ✨ 🎉 🦀 🫠 🫨 🪿 🩷"),
    ("Emojis en séquence", "👨\u{200D}👩\u{200D}👧\u{200D}👦 👩\u{200D}💻 🧑\u{200D}🚀 👨\u{200D}🍳 🏳\u{FE0F}\u{200D}🌈 🏴\u{200D}☠\u{FE0F} ❤\u{FE0F}\u{200D}🔥 🐻\u{200D}❄\u{FE0F} 👩\u{200D}❤\u{FE0F}\u{200D}👨"),
    ("Teintes, drapeaux, touches", "👍🏻 👍🏽 👍🏿 👋🏾 🧑🏽\u{200D}🚀 🇫🇷 🇯🇵 🇧🇷 🇺🇦 🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F} 1\u{FE0F}\u{20E3} #\u{FE0F}\u{20E3} *\u{FE0F}\u{20E3} 🔟"),
    ("Sélecteurs de variante", "☺ ☺\u{FE0F} ☺\u{FE0E} · ❤ ❤\u{FE0F} ❤\u{FE0E} · ☎ ☎\u{FE0F} ☎\u{FE0E} · ⚠ ⚠\u{FE0F} · © ©\u{FE0F} · ™ ™\u{FE0F} · ↔ ↔\u{FE0F} · ▶ ▶\u{FE0F}"),
    ("Espaces et invisibles", "a\u{200B}b|a\u{2800}b|a\u{3164}b|a\u{2003}b|a\u{2009}b|a\u{202F}b|a\u{A0}b|a\u{AD}b|a\u{200D}b|a\u{2060}b|a\u{FEFF}b|a\u{180E}b|a\u{115F}b|a\u{FFA0}b|a\u{34F}b|a\u{61C}b|a\u{202E}b\u{202C}|a\u{E0001}b|a\u{7}b"),
    ("Sans aucune police", "a\u{E000}b a\u{F8FF}b a\u{10FFFD}b a\u{378}b a\u{FFFF}b a\u{FFFD}b"),
    ("Ligne mixte", "Alice ★ 你好 한글 สวัสดี 𝓐𝓑 नमस्ते ⚔ قلب 🔥 fin"),
];

/// Planche du jeu d'essai : chaque ligne en texte brut (pseudos, noms de
/// serveurs), puis avec les emojis en image (messages, salons, statuts).
pub(super) fn samples_ui(ui: &mut egui::Ui, t: &crate::theme::Tokens) {
    let body = FontId::proportional(t.typography.body);
    let small = FontId::proportional(t.typography.small);
    egui::ScrollArea::vertical().show(ui, |ui| {
        let third = (ui.available_width() - 150.0) / 2.0 - 2.0 * t.spacing.item_x;
        // Alignement vertical : une ligne mixte ne doit pas être plus haute qu'une ligne latine.
        for text in ["Ligne latine de référence, hauteur normale.", "Ligne mixte 你好 한글 สวัสดี 𝓐𝓑 नमस्ते ⚔ ░▒▓ de même hauteur."] {
            ui.label(egui::RichText::new(text).font(body.clone()).color(t.colors.text).background_color(t.colors.bg3));
        }
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("gras 你好 ★ 𝕬").font(font(t.typography.body, Face { bold: true, italic: false })).color(t.colors.text_strong));
            ui.label(egui::RichText::new("code 你好 ★ ░▒▓ ┌─┐").font(FontId::monospace(t.typography.body)).color(t.colors.text));
            ui.label(egui::RichText::new("Titre 你好 ツ ★ 𝓐").family(demi()).size(t.typography.title).color(t.colors.text_strong));
            ui.label(egui::RichText::new("petit 你好 ツ ★ 𝓐 😀").font(small.clone()).color(t.colors.muted));
        });
        egui::Grid::new("planche_glyphes").num_columns(3).min_row_height(0.0).spacing(egui::vec2(t.spacing.item_x, 0.0)).show(ui, |ui| {
            for (name, text) in SAMPLES {
                ui.label(egui::RichText::new(*name).font(small.clone()).color(t.colors.muted));
                ui.scope(|ui| {
                    ui.set_max_width(third);
                    ui.add(egui::Label::new(egui::RichText::new(*text).font(body.clone()).color(t.colors.text)).wrap());
                });
                ui.scope(|ui| {
                    ui.set_max_width(third);
                    super::emoji::label(ui, text, &body, t.colors.text, false);
                });
                ui.end_row();
            }
        });
    });
}

/// Définitions d'egui avec, en tête de chaque famille, les fichiers lus
/// (`data[i]` pour `FILES[i]`, absents : police intégrée) ; `figtree` : Figtree
/// devant, les fichiers du système en repli pour les glyphes qu'elle n'a pas.
fn definitions(data: [Option<Bytes>; N], figtree: bool) -> (FontDefinitions, Real) {
    let mut defs = FontDefinitions::default();
    let base = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let mut heads: [Option<String>; N] = Default::default();
    for (i, bytes) in data.into_iter().enumerate() {
        if let Some(bytes) = bytes {
            let name = format!("segoe-{i}");
            let mut font = FontData::from_blob(bytes, 0);
            if i == VARIABLE {
                // Graisse 500, dessin « Text » (corps de 10,5 points) ; une police statique ignore ces axes.
                font = font.tweak(egui::FontTweak { coords: egui::epaint::text::VariationCoords::new([(b"wght", 500.0), (b"opsz", 10.5)]), ..Default::default() });
            }
            if i == CODE && cfg!(not(target_os = "android")) {
                // Consolas garde son interligne sous la ligne de base : dans une ligne de texte,
                // le code montait de 3 points. Décalage qui pose sa ligne de base sur celle de la
                // police d'interface (ascendantes et hauteurs de ligne lues dans les fichiers).
                font = font.tweak(egui::FontTweak { y_offset_factor: if figtree { 0.19 } else { 0.25 }, ..Default::default() });
            }
            defs.font_data.insert(name.clone(), Arc::new(font));
            heads[i] = Some(name);
        }
    }
    // Sans la normale, les variantes gardent la police intégrée derrière.
    let chain = |head: Option<&String>, fallback: Option<&String>| -> Vec<String> {
        head.into_iter().chain(fallback).cloned().chain(base.iter().cloned()).collect()
    };
    defs.families.insert(FontFamily::Proportional, chain(heads[0].as_ref(), None));
    defs.families.insert(FontFamily::Name(BOLD.into()), chain(heads[1].as_ref(), heads[0].as_ref()));
    defs.families.insert(FontFamily::Name(ITALIC.into()), chain(heads[2].as_ref(), heads[0].as_ref()));
    defs.families.insert(FontFamily::Name(BOLD_ITALIC.into()), chain(heads[3].as_ref().or(heads[1].as_ref()), heads[0].as_ref()));
    // Demi-grasse absente : la grasse, sinon la normale (le texte fort garde sa couleur).
    defs.families.insert(FontFamily::Name(DEMI.into()), chain(heads[4].as_ref().or(heads[1].as_ref()), heads[0].as_ref()));
    // Moyenne absente (Windows 10) : la normale.
    defs.families.insert(FontFamily::Name(MEDIUM.into()), chain(heads[VARIABLE].as_ref(), heads[0].as_ref()));
    // Monospace : Consolas, puis la police intégrée, Segoe UI en secours pour les lettres qui leur manquent.
    if let Some(mono) = defs.families.get_mut(&FontFamily::Monospace) {
        if let Some(n) = &heads[0] {
            mono.insert(1, n.clone());
        }
        if let Some(code) = &heads[CODE] {
            mono.insert(0, code.clone());
        }
    }
    // Polices de nom : toujours là, sur la demi-grasse tant que leur fichier n'est pas
    // chargé (`lead_names`).
    let demi_chain = defs.families.get(&FontFamily::Name(DEMI.into())).cloned().unwrap_or_default();
    let named: Vec<FontFamily> = NAMES.iter().map(NameFont::family).filter(|f| *f != demi()).collect();
    for family in &named {
        defs.families.insert(family.clone(), demi_chain.clone());
    }
    // Jumelles « système d'abord » (diacritiques absents de Figtree, voir `route_marks`) et
    // famille témoin : toujours là, pour qu'une mise en page qui les demande ne panique pas.
    for family in [FontFamily::Proportional, FontFamily::Name(MEDIUM.into()), FontFamily::Name(DEMI.into()), FontFamily::Name(BOLD.into()), FontFamily::Name(ITALIC.into()), FontFamily::Name(BOLD_ITALIC.into())].into_iter().chain(named.iter().cloned()) {
        if let (Some(twin), Some(chain)) = (twin(&family), defs.families.get(&family).cloned()) {
            defs.families.insert(twin, chain);
        }
    }
    // Symboles « texte » : la chaîne ordinaire ; Segoe UI Symbol y passe en tête avec les replis (`with_fallbacks`).
    defs.families.insert(symbols(), defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
    defs.families.insert(solo(), if figtree { vec![FIGTREE_WEIGHTS[0].0.to_owned()] } else { defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default() });
    let real = Real { bold: heads[1].is_some(), italic: heads[2].is_some() };
    if !figtree {
        return (defs, real);
    }
    for (name, weight) in FIGTREE_WEIGHTS {
        let tweak = egui::FontTweak { coords: egui::epaint::text::VariationCoords::new([(b"wght", weight)]), ..Default::default() };
        defs.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(FIGTREE).tweak(tweak)));
    }
    // Figtree en tête de chaque famille ; l'italique est la normale, penchée par egui.
    let lead = [(FontFamily::Proportional, 0), (FontFamily::Name(MEDIUM.into()), 1), (FontFamily::Name(DEMI.into()), 2), (FontFamily::Name(BOLD.into()), 3), (FontFamily::Name(ITALIC.into()), 0), (FontFamily::Name(BOLD_ITALIC.into()), 3)];
    // Polices de nom : la demi-grasse derrière elles est celle de Figtree aussi.
    for (family, i) in lead.into_iter().chain(named.into_iter().map(|f| (f, 2))) {
        if let Some(chain) = defs.families.get_mut(&family) {
            chain.insert(0, FIGTREE_WEIGHTS[i].0.to_owned());
        }
    }
    (defs, Real { bold: true, italic: false })
}

/// Famille de Figtree seule : dit si un diacritique est dans la police d'interface.
const SOLO: &str = "ferrocord-interface-seule";
/// Suffixe des familles jumelles, où la police du système passe devant.
const TWIN: &str = "+systeme";

fn solo() -> FontFamily {
    FontFamily::Name(SOLO.into())
}

/// Jumelle « système d'abord » d'une famille de texte ; le code n'en a pas.
fn twin(family: &FontFamily) -> Option<FontFamily> {
    match family {
        FontFamily::Proportional => Some(FontFamily::Name(format!("ferrocord-normal{TWIN}").into())),
        FontFamily::Name(n) if !n.ends_with(TWIN) && n.as_ref() != SOLO => Some(FontFamily::Name(format!("{n}{TWIN}").into())),
        _ => None,
    }
}

fn within(c: u32, ranges: &[(u32, u32)]) -> bool {
    ranges.iter().any(|(a, b)| (*a..=*b).contains(&c))
}

/// Diacritique combinant qu'on empile sur une lettre latine (« zalgo », ( ͡° ͜ʖ ͡°)).
fn mark(c: char) -> bool {
    within(u32::from(c), &[(0x0300, 0x036F), (0x0483, 0x0489), (0x1AB0, 0x1AFF), (0x1DC0, 0x1DFF), (0x20D0, 0x20FF), (0xFE20, 0xFE2F)])
}

/// Passages de `text` (en octets) dont un diacritique manque à la police
/// d'interface : chaque lettre concernée, avec tous ses diacritiques.
fn marked(text: &str, mut has: impl FnMut(char) -> bool) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let (mut start, mut foreign) = (0, false);
    let mut flush = |start: usize, end: usize, foreign: bool| {
        if foreign {
            match out.last_mut() {
                Some(last) if last.end == start => last.end = end,
                _ => out.push(start..end),
            }
        }
    };
    for (i, c) in text.char_indices() {
        if mark(c) {
            foreign = foreign || !has(c);
        } else {
            flush(start, i, foreign);
            (start, foreign) = (i, false);
        }
    }
    flush(start, text.len(), foreign);
    out
}

/// egui choisit la police d'un groupe (lettre et diacritiques) d'après la
/// lettre seule, et omet les diacritiques que cette police n'a pas : avec
/// Figtree, le texte « zalgo » perdait presque tout. Ici, les groupes dont un
/// diacritique manque à Figtree passent dans la famille jumelle, où la police
/// du système (qui les a tous) vient en tête. Le texte n'est pas modifié :
/// seules les sections de `job` sont recoupées. Sans diacritique : rien.
pub(super) fn route_marks(ui: &egui::Ui, job: &mut egui::text::LayoutJob) {
    if !FIGTREE_ON.load(std::sync::atomic::Ordering::Relaxed) || !job.text.chars().any(mark) {
        return;
    }
    let probe = FontId::new(14.0, solo());
    // Polices d'egui par défaut (tests sans `install`) : pas de jumelles, rien à faire.
    if !ui.fonts(|f| f.definitions().families.contains_key(&probe.family)) {
        return;
    }
    reroute(job, |family, text| Some((twin(family)?, marked(text, |c| ui.fonts_mut(|f| f.has_glyph(&probe, c))))));
}

/// Recoupe les sections de `job` : dans chaque section de texte, les passages
/// que rend `plan` (famille de destination, plages en octets dans la section)
/// changent de famille. Le texte n'est pas modifié.
fn reroute(job: &mut egui::text::LayoutJob, mut plan: impl FnMut(&FontFamily, &str) -> Option<(FontFamily, Vec<std::ops::Range<usize>>)>) {
    use egui::text::ByteIndex;
    let mut out = Vec::with_capacity(job.sections.len() + 2);
    for section in std::mem::take(&mut job.sections) {
        let (a, b) = (section.byte_range.start.0, section.byte_range.end.0);
        // Un emoji en image garde sa police : sa taille y a été mesurée (`emoji::append`).
        let image = section.format.color == Color32::TRANSPARENT;
        let planned = if image { None } else { job.text.get(a..b).and_then(|text| plan(&section.format.font_id.family, text)).filter(|(_, spans)| !spans.is_empty()) };
        let Some((family, spans)) = planned else {
            out.push(section);
            continue;
        };
        let first = out.len();
        let piece = |from: usize, to: usize, routed: bool, out: &mut Vec<egui::text::LayoutSection>| {
            if from < to {
                let mut format = section.format.clone();
                if routed {
                    format.font_id.family = family.clone();
                }
                // Le retrait de première ligne ne vaut que pour le premier morceau.
                let leading_space = if out.len() == first { section.leading_space } else { 0.0 };
                out.push(egui::text::LayoutSection { leading_space, byte_range: ByteIndex(a + from)..ByteIndex(a + to), format });
            }
        };
        let mut at = 0;
        for span in spans {
            piece(at, span.start, false, &mut out);
            piece(span.start, span.end, true, &mut out);
            at = span.end;
        }
        piece(at, b - a, false, &mut out);
    }
    job.sections = out;
}

/// Famille des symboles « texte » : la chaîne ordinaire, Segoe UI Symbol
/// (dessins au trait) devant Segoe UI Emoji (voir [`route_symbols`]).
const SYMBOLS: &str = "ferrocord-symboles";

fn symbols() -> FontFamily {
    FontFamily::Name(SYMBOLS.into())
}

/// Symbole du plan de base (flèches, formes, cartes, dingbats : ❤ ♥ ☆ ✔ ☠)
/// qu'Unicode présente en texte tant qu'aucun sélecteur U+FE0F ne le suit.
fn text_symbol(c: char) -> bool {
    within(u32::from(c), &[(0x2190, 0x21FF), (0x2300, 0x23FF), (0x25A0, 0x27BF), (0x2900, 0x297F), (0x2B00, 0x2BFF), (0x3030, 0x3030), (0x303D, 0x303D), (0x3297, 0x3299)]) && !crate::state::emoji::emoji_presentation(c)
}

/// Passages de `text` (en octets) à dessiner en symboles « texte » : un
/// symbole du plan de base sans sélecteur d'emoji, ou tout caractère suivi du
/// sélecteur de texte U+FE0E (qui reste avec lui).
fn symbol_spans(text: &str) -> Vec<std::ops::Range<usize>> {
    let mut out: Vec<std::ops::Range<usize>> = Vec::new();
    let mut chars = text.char_indices().peekable();
    let mut previous = false;
    while let Some((i, c)) = chars.next() {
        let next = chars.peek().map(|(_, n)| *n);
        let routed = if c == '\u{FE0E}' { previous } else { next != Some('\u{FE0F}') && (next == Some('\u{FE0E}') || text_symbol(c)) };
        if routed {
            let end = i + c.len_utf8();
            match out.last_mut() {
                Some(last) if last.end == i => last.end = end,
                _ => out.push(i..end),
            }
        }
        previous = routed;
    }
    out
}

/// Nom d'une personne : le client le dessine en texte, sans image d'emoji. Un
/// symbole qu'Unicode présente en texte (« ❤ », « ♥ », « ✔ » sans sélecteur
/// U+FE0F) y prend donc la couleur du nom, alors que la chaîne de repli,
/// emojis en couleur d'abord, le donnait à Segoe UI Emoji (un cœur rouge à la
/// place du cœur gris). Ici, ces symboles passent dans la famille où Segoe UI
/// Symbol vient avant. Le texte n'est pas modifié : seules les sections de
/// `job` sont recoupées. Sans symbole : rien.
pub(super) fn route_symbols(ui: &egui::Ui, job: &mut egui::text::LayoutJob) {
    if !job.text.chars().any(|c| c == '\u{FE0E}' || text_symbol(c)) {
        return;
    }
    // Polices d'egui par défaut (tests sans `install`) : pas de famille de symboles.
    if !ui.fonts(|f| f.definitions().families.contains_key(&symbols())) {
        return;
    }
    reroute(job, |_, text| Some((symbols(), symbol_spans(text))));
}

// ---------------------------------------------------------------------------
// Repli : polices de Windows pour les caractères que la chaîne n'a pas.
//
// egui 0.37 n'embarque plus de police d'emojis ni de symboles, et son
// fournisseur de polices du système n'est pas compilé ici : sans ce repli,
// tout ce qui sort de Segoe UI (★, 你好, 한글, 𝓐𝓑𝓒, 😀…) s'affichait en carré.
//
// Les fichiers sont projetés en mémoire par le fil des polices (aucune copie
// privée : seules les pages lues comptent, et elles sont celles du cache du
// système) puis ajoutés en fin de chaque chaîne, dans un ordre fixe. Une
// police dont aucun glyphe n'est demandé ne coûte que l'analyse de ses
// en-têtes, une fois, à l'installation (24 polices : 83 ms dans un build de
// test non optimisé, sur le fil d'interface) : rien dans l'atlas, aucun
// contour lu.
//
// L'ordre est fixe exprès. Le chargement à la demande d'egui (`FontProvider`)
// a été essayé : il range les polices dans l'ordre où le texte les a
// réclamées, et le même caractère changeait de police selon ce qui avait été
// affiché avant (😀 au trait si ★ était passé d'abord, en couleur sinon).
//
// Limites d'egui 0.37, notées ici pour ne pas les redécouvrir :
// - pas d'algorithme bidirectionnel : un passage arabe ou hébreu est bien mis
//   en forme et écrit de droite à gauche (HarfBuzz), mais les passages d'une
//   ligne mixte restent rangés dans l'ordre logique, de gauche à droite ;
// - la police d'un groupe (lettre et diacritiques) est celle de sa lettre :
//   voir `route_marks` ;
// - drapeaux : Windows n'a pas de glyphes de drapeaux (lettres du pays à la
//   place) ; les images d'emojis, elles, les couvrent ;
// - une touche (1️⃣) hors d'une image d'emoji reste un chiffre.

/// Polices de repli (fichier de `Fonts`, face dans le fichier, décalage vertical
/// en fraction du corps), dans l'ordre des chaînes. Le décalage pose la ligne de
/// base de la police sur celle de Segoe UI : egui centre chaque police dans la
/// ligne d'après ses propres hauteurs, et celles-ci (lues dans les fichiers)
/// faisaient monter les emojis de 2 à 3 points, le birman et le tibétain de 4.
///
/// Ordre : les emojis en couleur d'abord (Segoe UI Symbol en a des dessins au
/// trait ; en contrepartie, les quelques symboles que Segoe UI Emoji dessine
/// aussi, comme △ ou ✓, viennent d'elle) ; les symboles et les alphabets
/// mathématiques avant les polices d'Asie, dont les symboles sont de pleine
/// chasse ; les polices d'Asie avant les petites polices d'écriture, qui ont
/// quelques signes de ponctuation chinoise à leur façon ; le chinois simplifié
/// avant les autres polices d'idéogrammes (choix de Windows hors d'une langue
/// d'Asie : un même texte ne mélange pas deux dessins) ; puis une police par
/// écriture. Cambria Math, MS Gothic et Sylfaen n'apportent aucun caractère de
/// plus : elles n'y sont pas.
#[cfg(windows)]
const FALLBACKS: &[(&str, u32, f32)] = &[
    ("seguiemj.ttf", 0, 0.22),
    ("seguisym.ttf", 0, 0.0),
    ("msyh.ttc", 0, 0.0),
    ("msjh.ttc", 0, 0.0),
    ("malgun.ttf", 0, 0.0),
    ("YuGothM.ttc", 1, 0.0),
    ("meiryo.ttc", 0, 0.0),
    ("LeelawUI.ttf", 0, 0.0),
    ("leelawad.ttf", 0, 0.0),
    ("Nirmala.ttc", 0, 0.0),
    ("Nirmala.ttf", 0, 0.0),
    ("ebrima.ttf", 0, 0.0),
    ("gadugi.ttf", 0, 0.0),
    ("mmrtext.ttf", 0, 0.31),
    ("himalaya.ttf", 0, 0.32),
    ("monbaiti.ttf", 0, 0.15),
    ("msyi.ttf", 0, 0.07),
    ("taile.ttf", 0, 0.12),
    ("ntailu.ttf", 0, 0.14),
    ("phagspa.ttf", 0, 0.0),
    ("javatext.ttf", 0, 0.3),
    ("mvboli.ttf", 0, 0.08),
    ("seguihis.ttf", 0, 0.0),
    ("simsunb.ttf", 0, 0.05),
    // Écritures rares et anciennes que rien d'autre ne couvre (linéaire A « 𐙚 », très
    // courant dans les pseudos décorés ; Windows 11), lettres modificatrices latines
    // récentes, idéogrammes de l'extension G.
    ("SansSerifCollection.ttf", 0, 0.0),
    ("arial.ttf", 0, 0.0),
    ("SimsunExtG.ttf", 0, 0.0),
];
/// Ailleurs (Android) : la police du système seule.
#[cfg(not(windows))]
const FALLBACKS: &[(&str, u32, f32)] = &[];

/// Un fichier de repli plus gros est ignoré (les polices chinoises font 20 à
/// 35 Mo ; projetées, elles ne coûtent que les pages lues).
const MAX_FALLBACK_BYTES: u64 = 64 << 20;

/// Polices de repli présentes sur cette machine : nom dans egui et contenu.
type Extra = Vec<(String, FontData)>;

fn read_fallbacks(dir: &Path) -> Extra {
    FALLBACKS
        .iter()
        .filter_map(|(file, index, y)| {
            let tweak = egui::FontTweak { y_offset_factor: *y, ..Default::default() };
            Some((format!("repli-{file}"), FontData::from_blob(read(dir, file, MAX_FALLBACK_BYTES)?, *index).tweak(tweak)))
        })
        .collect()
}

/// Ajoute `extra` à la fin de chaque chaîne (sauf la famille témoin).
fn with_fallbacks(defs: &mut FontDefinitions, extra: &Extra) {
    for (name, data) in extra {
        defs.font_data.insert(name.clone(), Arc::new(data.clone()));
    }
    for (family, chain) in &mut defs.families {
        if *family != solo() {
            chain.extend(extra.iter().map(|(name, _)| name.clone()));
        }
    }
    // Famille des symboles « texte » : Segoe UI Symbol devant tout (`route_symbols`).
    if let Some(chain) = defs.families.get_mut(&symbols())
        && let Some(i) = chain.iter().position(|name| name == SYMBOL_FONT)
    {
        let name = chain.remove(i);
        chain.insert(0, name);
    }
}

/// Nom dans egui de Segoe UI Symbol, lue comme police de repli.
const SYMBOL_FONT: &str = "repli-seguisym.ttf";

/// Caractère sans dessin : contrôle, format, « ignorable par défaut »
/// d'Unicode (liaisons, sélecteurs de variante, étiquettes, remplissages).
/// Jamais de carré pour eux.
pub(super) fn invisible(c: char) -> bool {
    const RANGES: &[(u32, u32)] = &[
        (0x00AD, 0x00AD),
        (0x034F, 0x034F),
        (0x061C, 0x061C),
        (0x115F, 0x1160),
        (0x17B4, 0x17B5),
        (0x180B, 0x180F),
        (0x200B, 0x200F),
        (0x2028, 0x202E),
        (0x2060, 0x206F),
        (0x3164, 0x3164),
        (0xFE00, 0xFE0F),
        (0xFEFF, 0xFEFF),
        (0xFFA0, 0xFFA0),
        (0xFFF0, 0xFFFB),
        (0x1BCA0, 0x1BCA3),
        (0x1D173, 0x1D17A),
        (0xE0000, 0xE0FFF),
    ];
    c.is_control() || within(u32::from(c), RANGES)
}

/// Remplace le carré d'egui pour ce qu'aucune police ne dessine : rien du
/// tout (largeur nulle) pour un caractère invisible, sinon un petit carré
/// pâle, plus bas et plus léger que le « tofu » d'une police. egui ne le
/// demande que pour un caractère absent de toute la chaîne.
fn placeholder(request: &egui::epaint::text::GlyphRasterizerRequest<'_>) -> Option<egui::epaint::text::RasterizedGlyph> {
    use egui::epaint::text::{GlyphBitmap, RasterizedGlyph};
    let c = request.cluster.chars().next()?;
    // Tabulation et espaces fines : egui les tire de l'espace de la police.
    if matches!(c, '\t' | '\u{2009}' | '\u{202F}') {
        return None;
    }
    if invisible(c) {
        let image = egui::ColorImage::filled([1, 1], Color32::TRANSPARENT);
        return Some(RasterizedGlyph { bitmap: GlyphBitmap { image, offset_px: egui::Vec2::ZERO, is_color: false }, advance_px: 0.0 });
    }
    let px = request.font_size_px;
    let side = (0.4 * px).round().max(3.0) as usize;
    let bearing = (0.08 * px).round().max(1.0);
    let mut image = egui::ColorImage::filled([side, side], Color32::TRANSPARENT);
    let ink = Color32::from_white_alpha(120);
    for i in 0..side {
        for (x, y) in [(i, 0), (i, side - 1), (0, i), (side - 1, i)] {
            image[(x, y)] = ink;
        }
    }
    // Posé un peu au-dessus de la ligne de base, à hauteur des minuscules.
    let offset_px = egui::vec2(bearing, -(side as f32) - (0.06 * px).round());
    Some(RasterizedGlyph { bitmap: GlyphBitmap { image, offset_px, is_color: false }, advance_px: side as f32 + 2.0 * bearing })
}

fn read(dir: &Path, name: &str, max: u64) -> Option<Bytes> {
    let mut file = std::fs::File::open(dir.join(name)).ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file() || meta.len() > max {
        return None;
    }
    let bytes = load(&mut file, usize::try_from(meta.len()).ok()?)?;
    // TrueType, OpenType ou collection, sinon ignoré (egui ne doit jamais lire autre chose).
    matches!((*bytes).as_ref().get(..4), Some([0, 1, 0, 0] | b"true" | b"OTTO" | b"ttcf")).then_some(bytes)
}

/// Windows : fichier projeté en mémoire, pas copié (lot C9 : environ 4,8 Mo
/// privés de moins, les pages restent celles du cache du système) ; copie si
/// la projection échoue.
#[cfg(windows)]
fn load(file: &mut std::fs::File, len: usize) -> Option<Bytes> {
    match crate::platform::mapped::Mapped::new(file, len) {
        Some(m) => Some(Arc::new(m)),
        None => copy(file),
    }
}

#[cfg(not(windows))]
fn load(file: &mut std::fs::File, _len: usize) -> Option<Bytes> {
    copy(file)
}

fn copy(file: &mut std::fs::File) -> Option<Bytes> {
    use std::io::Read;
    let mut v = Vec::new();
    file.read_to_end(&mut v).ok()?;
    Some(Arc::new(v))
}

/// Dossier des polices du système Android.
#[cfg(target_os = "android")]
fn fonts_dir() -> Option<PathBuf> {
    Some(PathBuf::from("/system/fonts"))
}

/// Dossier des polices de Windows.
#[cfg(not(target_os = "android"))]
fn fonts_dir() -> Option<PathBuf> {
    let root = std::env::var_os("SystemRoot").or_else(|| std::env::var_os("WINDIR"))?;
    Some(PathBuf::from(root).join("Fonts"))
}

// ---------------------------------------------------------------------------
// Polices de nom (lots profil et polices).
//
// Discord dessine un nom stylé dans une police à lui
// (`display_name_styles.font_id`, docs.discord.food, « Display Name Font »).
// Douze de ces polices sont libres (SIL Open Font License 1.1) : leurs
// fichiers sont embarqués (`assets/fonts/noms/<famille>/`, licence à côté).
// Les autres (Néo-Castel, Ribes, Sinistre) n'ont pas de fichier ici : elles
// prennent la police de Windows la plus proche. gg sans, propriétaire, est
// rendue par la demi-grasse de l'interface.
//
// Une police embarquée ne coûte rien tant que personne ne la porte : ses
// octets restent dans l'image de l'exécutable (pages jamais lues) et egui ne
// la connaît pas. Elle n'est ajoutée aux définitions que la première fois
// qu'un nom la demande (`styled`, puis `want`) : egui refait alors ses
// polices au début de l'image suivante, une seule fois par police, et une
// seule image est redemandée.

/// D'où vient le dessin d'une police de nom.
#[derive(Clone, Copy)]
enum NameSource {
    /// gg sans : la demi-grasse de l'interface.
    Interface,
    /// Fichier embarqué ; graisse réglée sur l'axe `wght` d'un fichier variable.
    Embedded(&'static [u8], Option<f32>),
    /// Pas de fichier : la police de Windows la plus proche (fichier de `Fonts`).
    Substitute(&'static str),
}

/// Une police de nom de Discord. Table unique : le rendu ([`styled`]) et le
/// sélecteur de l'éditeur de profil lisent la même.
pub(crate) struct NameFont {
    /// `display_name_styles.font_id`.
    pub id: u16,
    /// Nom du style dans Discord (nom propre, non traduit).
    pub label: &'static str,
    /// Police que Discord utilise pour ce style.
    pub font: &'static str,
    /// Proposée par le sélecteur (les autres sont retirées par Discord, mais encore lues).
    pub offered: bool,
    family: &'static str,
    source: NameSource,
}

macro_rules! embedded {
    ($path:literal) => {
        NameSource::Embedded(include_bytes!(concat!("../../assets/fonts/noms/", $path)), None)
    };
    ($path:literal, $weight:literal) => {
        NameSource::Embedded(include_bytes!(concat!("../../assets/fonts/noms/", $path)), Some($weight))
    };
}

const fn name(id: u16, label: &'static str, font: &'static str, offered: bool, family: &'static str, source: NameSource) -> NameFont {
    NameFont { id, label, font, offered, family, source }
}

/// Polices de nom, dans l'ordre du sélecteur (gg sans d'abord, puis par
/// identifiant). Seize entrées au plus : [`want`] les compte sur 16 bits.
/// Fichiers variables : graisse 700, comme les fichiers statiques retenus
/// (Kalam, Zilla Slab et Compagnon en « Bold »).
pub(crate) static NAMES: [NameFont; 16] = [
    name(11, "gg Sans", "gg sans", true, "ferrocord-nom-11", NameSource::Interface),
    name(1, "Bangers", "Bangers", false, "ferrocord-nom-1", embedded!("bangers/Bangers-Regular.ttf")),
    name(2, "BioRhyme", "BioRhyme", false, "ferrocord-nom-2", embedded!("biorhyme/BioRhyme[wdth,wght].ttf", 700.0)),
    name(3, "Sakura", "Cherry Bomb One", true, "ferrocord-nom-3", embedded!("cherrybombone/CherryBombOne-Regular.ttf")),
    name(4, "Jellybean", "Chicle", true, "ferrocord-nom-4", embedded!("chicle/Chicle-Regular.ttf")),
    name(5, "Compagnon", "Compagnon", false, "ferrocord-nom-5", embedded!("compagnon/Compagnon-Bold.otf")),
    name(6, "Modern", "MuseoModerno", true, "ferrocord-nom-6", embedded!("museomoderno/MuseoModerno[wght].ttf", 700.0)),
    name(7, "Medieval", "Néo-Castel", true, "ferrocord-nom-7", NameSource::Substitute("georgiab.ttf")),
    name(8, "8Bit", "Pixelify Sans", true, "ferrocord-nom-8", embedded!("pixelifysans/PixelifySans[wght].ttf", 700.0)),
    name(9, "Ribes", "Ribes", false, "ferrocord-nom-9", NameSource::Substitute("segoeprb.ttf")),
    name(10, "Vampyre", "Sinistre", true, "ferrocord-nom-10", NameSource::Substitute("georgiab.ttf")),
    name(12, "Tempo", "Zilla Slab", true, "ferrocord-nom-12", embedded!("zillaslab/ZillaSlab-Bold.ttf")),
    name(13, "Monkey Bars", "Playpen Sans", true, "ferrocord-nom-13", embedded!("playpensans/PlaypenSans[wght].ttf", 700.0)),
    name(14, "Mainframe", "Orbitron", true, "ferrocord-nom-14", embedded!("orbitron/Orbitron[wght].ttf", 700.0)),
    name(15, "Headbang", "New Rocker", true, "ferrocord-nom-15", embedded!("newrocker/NewRocker-Regular.ttf")),
    name(16, "Journal", "Kalam", true, "ferrocord-nom-16", embedded!("kalam/Kalam-Bold.ttf")),
];

/// Police d'un style de nom neuf (gg sans).
pub(crate) const DEFAULT_NAME_FONT: u16 = NAMES[0].id;

impl NameFont {
    /// Famille d'egui de cette police : elle existe toujours (sur la
    /// demi-grasse tant que le fichier n'est pas chargé, ou sans lui).
    fn family(&self) -> FontFamily {
        match self.source {
            NameSource::Interface => demi(),
            _ => FontFamily::Name(self.family.into()),
        }
    }

    /// Nom, dans egui, du fichier embarqué.
    fn data_name(&self) -> String {
        format!("nom-{}", self.id)
    }

    /// Le fichier de cette police est-il embarqué ?
    pub(crate) fn embedded(&self) -> bool {
        matches!(self.source, NameSource::Embedded(..))
    }
}

/// Fichiers de Windows des polices de nom sans fichier embarqué (nom du fichier, contenu projeté).
static NAME_DATA: std::sync::Mutex<Vec<(&'static str, Bytes)>> = std::sync::Mutex::new(Vec::new());

/// Famille du style de nom `font_id` ; identifiant inconnu : la demi-grasse.
/// Une police embarquée est chargée ici à sa première demande : le nom est
/// dessiné en demi-grasse pendant une image, puis dans sa police.
pub(crate) fn styled(ctx: &egui::Context, font_id: u16) -> FontFamily {
    let Some(i) = NAMES.iter().position(|n| n.id == font_id) else { return demi() };
    if NAMES[i].embedded() {
        want(ctx, i);
    }
    NAMES[i].family()
}

/// Ce que `ctx` a d'installé : de quoi refaire ses familles quand une police
/// de nom s'y ajoute.
#[derive(Clone, Default)]
struct Applied {
    data: [Option<Bytes>; N],
    extra: Extra,
    figtree: bool,
    /// Test de composant : le compteur global des polices n'est pas touché.
    quiet: bool,
    /// Polices de nom embarquées chargées (bit : rang dans [`NAMES`]).
    names: u16,
}

fn applied_id() -> egui::Id {
    egui::Id::unique("ferrocord-polices-installees")
}

fn applied(ctx: &egui::Context) -> Option<Arc<Applied>> {
    ctx.data(|d| d.get_temp::<Arc<Applied>>(applied_id()))
}

/// Un seul fil refait les familles à la fois (fil des polices, fil d'interface).
static APPLYING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Polices de nom embarquées chargées dans `ctx` (bit : rang dans [`NAMES`]).
#[cfg(test)]
pub(crate) fn loaded_names(ctx: &egui::Context) -> u16 {
    applied(ctx).map_or(0, |a| a.names)
}

/// Charge la police embarquée de rang `i` dans `ctx`, si elle ne l'est pas.
/// Chemin courant (déjà chargée) : une lecture de la mémoire d'egui. Sans
/// polices installées (contexte d'egui nu) : rien.
fn want(ctx: &egui::Context, i: usize) {
    let bit = 1u16 << i;
    if applied(ctx).is_none_or(|a| a.names & bit != 0) {
        return;
    }
    let _guard = APPLYING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // Relu sous le verrou : le fil des polices a pu passer entre-temps.
    let Some(a) = applied(ctx).filter(|a| a.names & bit == 0) else { return };
    commit(ctx, Applied { names: a.names | bit, ..(*a).clone() });
}

/// Lit (projette) les polices de Windows des styles sans fichier embarqué.
fn read_names(dir: &Path) {
    let mut found: Vec<(&'static str, Bytes)> = Vec::new();
    for n in &NAMES {
        if let NameSource::Substitute(file) = n.source
            && !found.iter().any(|(f, _)| *f == file)
            && let Some(bytes) = read(dir, file, MAX_FONT_BYTES)
        {
            found.push((file, bytes));
        }
    }
    if let Ok(mut kept) = NAME_DATA.lock() {
        *kept = found;
    }
}

/// Met en tête de sa famille (et de sa jumelle) chaque police de nom
/// disponible : fichier embarqué demandé (`names`), ou fichier de Windows lu.
fn lead_names(defs: &mut FontDefinitions, names: u16) {
    let kept = NAME_DATA.lock().map(|k| k.clone()).unwrap_or_default();
    for (i, n) in NAMES.iter().enumerate() {
        let (name, data) = match n.source {
            NameSource::Embedded(bytes, weight) if names & (1 << i) != 0 => {
                let coords = egui::epaint::text::VariationCoords::new(weight.map(|w| (b"wght", w)));
                (n.data_name(), FontData::from_static(bytes).tweak(egui::FontTweak { coords, ..Default::default() }))
            }
            NameSource::Substitute(file) => match kept.iter().find(|(f, _)| *f == file) {
                Some((_, bytes)) => (format!("nom-{file}"), FontData::from_blob(bytes.clone(), 0)),
                None => continue,
            },
            _ => continue,
        };
        defs.font_data.entry(name.clone()).or_insert_with(|| Arc::new(data));
        let family = n.family();
        for f in twin(&family).into_iter().chain([family]) {
            defs.families.entry(f).or_default().insert(0, name.clone());
        }
    }
}

/// Police choisie par le thème (Figtree, sinon celle du système) et fichiers
/// du système déjà lus : changer de police ne relit rien.
static FIGTREE_ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
static INSTALLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static SYSTEM: std::sync::Mutex<Option<([Option<Bytes>; N], Extra)>> = std::sync::Mutex::new(None);

/// Le thème change de police : les familles sont refaites à l'image suivante.
/// Sans effet dans les tests et l'aperçu sur police intégrée (`install` non appelé).
pub(super) fn set_family(ctx: &egui::Context, face: crate::theme::FontFace) {
    use std::sync::atomic::Ordering::Relaxed;
    let figtree = face == crate::theme::FontFace::Figtree;
    if FIGTREE_ON.swap(figtree, Relaxed) != figtree && INSTALLED.load(Relaxed) {
        let (data, extra) = SYSTEM.lock().ok().and_then(|d| d.clone()).unwrap_or_default();
        apply(ctx, data, &extra, figtree);
    }
}

/// Polices choisies par l'utilisateur parmi celles installées (lot PERSO) :
/// interface (normale, grasse, italique, grasse italique) et code. Elles
/// passent en tête de chaîne ; tout le reste sert de repli.
#[derive(Clone, Default)]
pub(super) struct Custom {
    pub ui: Option<[Option<Arc<Vec<u8>>>; 4]>,
    pub code: Option<Arc<Vec<u8>>>,
}

static CUSTOM: std::sync::Mutex<Option<Custom>> = std::sync::Mutex::new(None);

/// Change les polices choisies ; les familles sont refaites à l'image suivante.
pub(super) fn set_custom(ctx: &egui::Context, custom: Custom) {
    use std::sync::atomic::Ordering::Relaxed;
    if let Ok(mut c) = CUSTOM.lock() {
        *c = Some(custom);
    }
    if INSTALLED.load(Relaxed) {
        let (data, extra) = SYSTEM.lock().ok().and_then(|d| d.clone()).unwrap_or_default();
        apply(ctx, data, &extra, FIGTREE_ON.load(Relaxed));
    }
}

fn lead_custom(defs: &mut FontDefinitions, real: &mut Real) {
    let Some(c) = CUSTOM.lock().ok().and_then(|c| c.clone()) else { return };
    let mut lead = |family: FontFamily, name: &str, bytes: &Arc<Vec<u8>>| {
        defs.font_data.entry(name.to_owned()).or_insert_with(|| Arc::new(FontData::from_blob(bytes.clone() as Bytes, 0)));
        defs.families.entry(family).or_default().insert(0, name.to_owned());
    };
    if let Some([Some(regular), bold, italic, bold_italic]) = &c.ui {
        let bold_or = bold.as_ref().map_or(("choisie-0", regular), |b| ("choisie-1", b));
        lead(FontFamily::Proportional, "choisie-0", regular);
        lead(FontFamily::Name(MEDIUM.into()), "choisie-0", regular);
        lead(FontFamily::Name(DEMI.into()), bold_or.0, bold_or.1);
        lead(FontFamily::Name(BOLD.into()), bold_or.0, bold_or.1);
        let slanted = italic.as_ref().map_or(("choisie-0", regular), |i| ("choisie-2", i));
        lead(FontFamily::Name(ITALIC.into()), slanted.0, slanted.1);
        let both = bold_italic.as_ref().map_or(bold_or, |b| ("choisie-3", b));
        lead(FontFamily::Name(BOLD_ITALIC.into()), both.0, both.1);
        *real = Real { bold: bold.is_some(), italic: italic.is_some() };
    }
    if let Some(code) = &c.code {
        lead(FontFamily::Monospace, "choisie-code", code);
    }
}

/// Familles d'egui pour ce qui est installé.
fn build(a: &Applied) -> (FontDefinitions, Real) {
    let (mut defs, mut real) = definitions(a.data.clone(), a.figtree);
    with_fallbacks(&mut defs, &a.extra);
    // La police choisie passe devant tout, replis compris.
    lead_custom(&mut defs, &mut real);
    lead_names(&mut defs, a.names);
    (defs, real)
}

/// Installe `data` et `extra` ; les polices de nom déjà chargées dans `ctx` le restent.
fn apply(ctx: &egui::Context, data: [Option<Bytes>; N], extra: &Extra, figtree: bool) {
    let _guard = APPLYING.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let names = applied(ctx).map_or(0, |a| a.names);
    commit(ctx, Applied { data, extra: extra.clone(), figtree, quiet: false, names });
}

/// Donne ses polices à `ctx` : egui les refait au début de l'image suivante
/// (ou tout de suite si aucun texte n'a encore été mis en page dans celle-ci),
/// jamais au milieu d'une mise en page. Une seule image est demandée.
fn commit(ctx: &egui::Context, a: Applied) {
    let (defs, real) = build(&a);
    ctx.set_fonts(defs);
    // Boutons et lignes de menu en graisse moyenne (le thème ne règle que leur taille).
    ctx.all_styles_mut(|s| {
        if let Some(f) = s.text_styles.get_mut(&egui::TextStyle::Button) {
            f.family = medium();
        }
    });
    if !a.quiet {
        GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    ctx.data_mut(|d| {
        d.insert_temp(real_id(), real);
        d.insert_temp(applied_id(), Arc::new(a));
    });
    ctx.request_repaint();
}

/// Familles prêtes tout de suite (police intégrée), puis Segoe UI et les polices
/// de repli projetées dans un fil à part et installées à l'image suivante. Le fil d'interface ne lit
/// aucun fichier.
pub fn install(ctx: &egui::Context) {
    use std::sync::atomic::Ordering::Relaxed;
    INSTALLED.store(true, Relaxed);
    ctx.add_glyph_rasterizer(egui::epaint::text::GlyphRasterizer::new("ferrocord::sans-glyphe", placeholder));
    apply(ctx, Default::default(), &Extra::new(), FIGTREE_ON.load(Relaxed));
    let Some(dir) = fonts_dir() else { return };
    let ctx = ctx.clone();
    let spawned = std::thread::Builder::new().name("polices".into()).spawn(move || {
        let data = FILES.map(|f| read(&dir, f, MAX_FONT_BYTES));
        if data[0].is_some() {
            // Repli projeté ici aussi, puis une seule installation : une seule remise en page.
            let extra = read_fallbacks(&dir);
            read_names(&dir);
            if let Ok(mut kept) = SYSTEM.lock() {
                *kept = Some((data.clone(), extra.clone()));
            }
            apply(&ctx, data, &extra, FIGTREE_ON.load(Relaxed));
        }
    });
    // Sans fil, la police intégrée reste : rien d'autre à faire.
    drop(spawned);
}

/// Familles seules, sur la police intégrée (aperçu et tests : rendu stable).
pub fn install_builtin(ctx: &egui::Context) {
    apply(ctx, Default::default(), &Extra::new(), false);
}

/// Polices intégrées pour un test de composant, sans toucher au compteur
/// global des polices : les tests qui le surveillent tournent en parallèle.
#[cfg(test)]
pub(crate) fn install_quiet(ctx: &egui::Context) {
    let a = Applied { quiet: true, ..Default::default() };
    let (defs, real) = build(&a);
    ctx.set_fonts(defs);
    ctx.data_mut(|d| {
        d.insert_temp(real_id(), real);
        d.insert_temp(applied_id(), Arc::new(a));
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn families_exist_without_files_and_with_them() {
        let (defs, real) = definitions(Default::default(), false);
        assert_eq!(real, Real::default());
        for f in [BOLD, ITALIC, BOLD_ITALIC, DEMI, MEDIUM] {
            assert!(defs.families.get(&FontFamily::Name(f.into())).is_some_and(|v| !v.is_empty()), "{f}");
        }
        let fake = || -> Option<Bytes> { Some(Arc::new(b"pas une vraie police".to_vec())) };
        let (defs, real) = definitions([fake(), fake(), None, None, None, None, None], false);
        assert_eq!(real, Real { bold: true, italic: false });
        assert_eq!(defs.families[&FontFamily::Proportional][0], "segoe-0");
        assert_eq!(defs.families[&FontFamily::Name(BOLD.into())][..2], ["segoe-1".to_string(), "segoe-0".to_string()]);
        // Grasse italique absente : la grasse.
        assert_eq!(defs.families[&FontFamily::Name(BOLD_ITALIC.into())][0], "segoe-1");
        assert_eq!(defs.families[&FontFamily::Name(ITALIC.into())][0], "segoe-0");
        // Demi-grasse absente : la grasse ; présente : elle-même.
        assert_eq!(defs.families[&FontFamily::Name(DEMI.into())][0], "segoe-1");
        // Moyenne absente : la normale.
        assert_eq!(defs.families[&FontFamily::Name(MEDIUM.into())][0], "segoe-0");
        let (defs, _) = definitions([fake(), fake(), None, None, fake(), fake(), fake()], false);
        assert_eq!(defs.families[&FontFamily::Name(DEMI.into())][..2], ["segoe-4".to_string(), "segoe-0".to_string()]);
        // Moyenne : la police variable, réglée sur 500 ; code : Consolas en tête.
        assert_eq!(defs.families[&FontFamily::Name(MEDIUM.into())][..2], ["segoe-5".to_string(), "segoe-0".to_string()]);
        assert!(defs.font_data["segoe-5"].tweak.coords.as_ref().iter().any(|(_, v)| *v == 500.0));
        assert!(defs.font_data["segoe-0"].tweak.coords.as_ref().is_empty());
        assert_eq!(defs.families[&FontFamily::Monospace][0], "segoe-6");
    }

    /// Figtree : devant dans chaque famille de texte, à la bonne graisse ; le
    /// système derrière pour les glyphes absents ; le code reste en Consolas.
    #[test]
    fn figtree_leads_every_text_family() {
        let fake = || -> Option<Bytes> { Some(Arc::new(b"pas une vraie police".to_vec())) };
        let (defs, real) = definitions([fake(), fake(), fake(), None, None, None, fake()], true);
        assert_eq!(real, Real { bold: true, italic: false });
        let weight = |family: FontFamily| {
            let chain = &defs.families[&family];
            (chain[0].clone(), defs.font_data[&chain[0]].tweak.coords.as_ref().first().map(|(_, v)| *v), chain[1].clone())
        };
        assert_eq!(weight(FontFamily::Proportional), ("figtree-400".into(), Some(400.0), "segoe-0".into()));
        assert_eq!(weight(FontFamily::Name(MEDIUM.into())), ("figtree-500".into(), Some(500.0), "segoe-0".into()));
        assert_eq!(weight(FontFamily::Name(DEMI.into())), ("figtree-600".into(), Some(600.0), "segoe-1".into()));
        assert_eq!(weight(FontFamily::Name(BOLD.into())), ("figtree-700".into(), Some(700.0), "segoe-1".into()));
        assert_eq!(weight(FontFamily::Name(ITALIC.into())), ("figtree-400".into(), Some(400.0), "segoe-2".into()));
        assert_eq!(defs.families[&FontFamily::Monospace][0], "segoe-6");
        // Le fichier embarqué est bien une police TrueType.
        assert_eq!(FIGTREE.get(..4), Some(&[0u8, 1, 0, 0][..]));
    }

    #[test]
    fn missing_or_huge_files_are_skipped() {
        let dir = std::env::temp_dir().join(format!("ferrocord-polices-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let read = |d: &Path, n: &str| read(d, n, MAX_FONT_BYTES).map(|b| (*b).as_ref().to_vec());
        assert_eq!(read(&dir, "absente.ttf"), None);
        let _ = std::fs::write(dir.join("petite.ttf"), b"abc");
        assert_eq!(read(&dir, "petite.ttf"), None);
        let _ = std::fs::write(dir.join("vraie.ttf"), [0u8, 1, 0, 0, 9]);
        assert_eq!(read(&dir, "vraie.ttf").as_deref(), Some(&[0u8, 1, 0, 0, 9][..]));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Chaque caractère du jeu d'essai trouve un glyphe dans la chaîne, repli
    /// compris. Tolérant : s'il manque une police de repli sur cette machine,
    /// les absents sont seulement listés.
    #[cfg(windows)]
    #[test]
    fn every_sample_character_has_a_glyph() {
        let Some(dir) = fonts_dir() else { return };
        let data = FILES.map(|f| read(&dir, f, MAX_FONT_BYTES));
        if data[0].is_none() {
            return;
        }
        // Variantes d'anciens Windows : leur absence est normale.
        let optional = ["leelawad.ttf", "Nirmala.ttf", "meiryo.ttc"];
        let complete = FALLBACKS.iter().all(|(f, ..)| optional.contains(f) || dir.join(f).is_file());
        let started = std::time::Instant::now();
        let extra = read_fallbacks(&dir);
        let mapped = started.elapsed();
        let (mut defs, _) = definitions(data, true);
        with_fallbacks(&mut defs, &extra);
        // Le repli suit Figtree, Segoe UI et la police intégrée, dans l'ordre de la table.
        let chain = &defs.families[&FontFamily::Proportional];
        assert_eq!(chain[..2], ["figtree-400".to_string(), "segoe-0".to_string()]);
        assert_eq!(chain[chain.len() - extra.len()..], extra.iter().map(|(n, _)| n.clone()).collect::<Vec<_>>()[..]);
        assert_eq!(defs.families[&solo()], ["figtree-400".to_string()]);
        let mut fonts = egui::epaint::Fonts::new(Default::default(), defs);
        let id = FontId::proportional(14.0);
        let started = std::time::Instant::now();
        assert!(fonts.has_glyph(&id, 'a'));
        // Coût du repli tant que rien n'est demandé : projection, puis analyse des en-têtes.
        eprintln!("repli : {} polices, projection {mapped:?}, analyse {:?} (non optimisé)", extra.len(), started.elapsed());
        let mut missing = String::new();
        for (name, text) in SAMPLES.iter().filter(|(name, _)| *name != "Sans aucune police") {
            for c in text.chars().filter(|c| !c.is_whitespace() && !invisible(*c)) {
                if !fonts.has_glyph(&id, c) {
                    missing.push_str(&format!("{name} : {c:?} U+{:04X}\n", u32::from(c)));
                }
            }
        }
        if complete {
            assert!(missing.is_empty(), "sans glyphe :\n{missing}");
        } else if !missing.is_empty() {
            eprintln!("polices de repli absentes sur cette machine ; sans glyphe :\n{missing}");
        }
        // Rien dans l'atlas tant qu'aucun glyphe n'est dessiné.
        assert!(fonts.font_atlas_fill_ratio() < 0.01);
    }

    /// Le dessin de remplacement ne montre rien pour un caractère invisible,
    /// un petit carré pour un caractère inconnu, et laisse à egui la
    /// tabulation et les espaces fines.
    #[test]
    fn invisible_characters_draw_nothing_and_unknown_ones_a_small_box() {
        use egui::epaint::text::GlyphRasterizerRequest;
        let family = FontFamily::Proportional;
        let draw = |c: char| placeholder(&GlyphRasterizerRequest { cluster: c.encode_utf8(&mut [0; 4]), family: &family, font_size_px: 15.0, subpixel_offset_px: 0.0 });
        for c in ['\u{200B}', '\u{AD}', '\u{FE0E}', '\u{FE0F}', '\u{3164}', '\u{E0001}', '\u{E0067}', '\u{7}', '\n'] {
            assert!(invisible(c), "{c:?}");
            let g = draw(c).unwrap_or_else(|| panic!("{c:?}"));
            assert_eq!(g.advance_px, 0.0, "{c:?}");
            assert!(g.bitmap.image.pixels.iter().all(|p| p.a() == 0), "{c:?}");
        }
        for c in ['\u{E000}', '\u{378}', '\u{10FFFD}'] {
            let g = draw(c).unwrap_or_else(|| panic!("{c:?}"));
            // Plus petit et plus pâle que le carré d'une police : 6 px de côté à 15 px.
            assert_eq!(g.bitmap.image.size, [6, 6], "{c:?}");
            assert!(g.advance_px > 0.0 && g.bitmap.image.pixels.iter().all(|p| p.a() <= 120));
        }
        for c in ['\t', '\u{2009}', '\u{202F}'] {
            assert!(draw(c).is_none(), "{c:?}");
        }
        assert!(!invisible('a') && !invisible(' ') && !invisible('\u{2800}') && !invisible('你'));
    }

    /// Dans une vraie mise en page : aucun carré de police pour un caractère
    /// invisible ou inconnu, et la hauteur de ligne d'un texte mixte reste
    /// celle du texte latin.
    #[cfg(windows)]
    #[test]
    fn mixed_lines_keep_their_height_and_show_no_tofu() {
        let Some(dir) = fonts_dir() else { return };
        let data = FILES.map(|f| read(&dir, f, MAX_FONT_BYTES));
        if data[0].is_none() {
            return;
        }
        let (mut defs, _) = definitions(data, true);
        with_fallbacks(&mut defs, &read_fallbacks(&dir));
        let mut fonts = egui::epaint::Fonts::new(Default::default(), defs);
        fonts.add_glyph_rasterizer(egui::epaint::text::GlyphRasterizer::new("ferrocord::sans-glyphe", placeholder));
        let mut view = fonts.with_pixels_per_point(1.0);
        let mut layout = |text: &str| view.layout_no_wrap(text.to_string(), FontId::proportional(15.0), Color32::WHITE);
        let latin = layout("Ligne latine");
        for text in ["Alice ★ 你好 한글 สวัสดี 𝓐𝓑 नमस्ते ⚔ قلب 🔥", "░▒▓█ ┌─┐ ⣿ ʕ•ᴥ•ʔ ಠ_ಠ", "𝕬𝕭𝕮 ∑ ∫ ⨁ မြန်မာ བོད"] {
            assert_eq!(layout(text).size().y, latin.size().y, "{text}");
        }
        // Invisible et absent de toute police : largeur nulle (le carré d'une police en aurait une).
        let (plain, hidden) = (layout("ab"), layout("a\u{E0001}b"));
        assert_eq!(plain.size().x, hidden.size().x);
        // Inconnu : le petit carré de remplacement, 6 px de côté à 15 px.
        let unknown = layout("a\u{10FFFD}b");
        let g = unknown.rows[0].row.glyphs[1];
        assert_eq!(g.chr, '\u{10FFFD}');
        assert!((g.advance_width - 8.0).abs() < 0.01, "{}", g.advance_width);
    }

    /// Diacritiques : seuls les groupes dont un diacritique manque à la police
    /// d'interface sont repérés, lettre comprise, et les voisins sont réunis.
    #[test]
    fn marked_groups_are_found_with_their_letter() {
        let known = |c: char| c == '\u{301}';
        assert!(marked("texte sans rien", known).is_empty());
        assert!(marked("e\u{301}te\u{301}", known).is_empty());
        let s = "ae\u{301}Z\u{351}\u{301}a\u{36B}l o\u{35C}";
        let found: Vec<&str> = marked(s, known).into_iter().map(|r| &s[r]).collect();
        assert_eq!(found, ["Z\u{351}\u{301}a\u{36B}", "o\u{35C}"]);
        // Diacritique en tête de texte, sans lettre : repéré seul.
        assert_eq!(marked("\u{35C}a", known).first().map(|r| (r.start, r.end)), Some((0, 2)));
    }

    /// Les sections sont recoupées sans toucher au texte ; un emoji en image
    /// (texte transparent) et le code gardent leur police.
    #[test]
    fn foreign_marks_are_routed_to_the_system_first_twin() {
        let ctx = egui::Context::default();
        let (defs, _) = definitions(Default::default(), true);
        ctx.set_fonts(defs);
        let mut job = egui::text::LayoutJob::default();
        let format = |family: FontFamily, color| egui::TextFormat { font_id: FontId::new(14.0, family), color, ..Default::default() };
        job.append("( \u{361}° \u{35C}ʖ)", 3.0, format(FontFamily::Proportional, Color32::WHITE));
        job.append("o\u{35C}", 0.0, format(FontFamily::Name(BOLD.into()), Color32::WHITE));
        job.append("1\u{FE0F}\u{20E3}", 0.0, format(FontFamily::Proportional, Color32::TRANSPARENT));
        job.append("o\u{35C}", 0.0, format(FontFamily::Monospace, Color32::WHITE));
        let text = job.text.clone();
        let mut out = ctx.run_ui(Default::default(), |ui| {
            let mut plain = egui::text::LayoutJob::simple("rien à faire".into(), FontId::proportional(14.0), Color32::WHITE, 100.0);
            route_marks(ui, &mut plain);
            assert_eq!(plain.sections.len(), 1);
            route_marks(ui, &mut job);
            // Toutes les familles demandées existent : la mise en page ne panique pas.
            let _ = ui.fonts_mut(|f| f.layout_job(job.clone()));
        });
        out.textures_delta.clear();
        assert_eq!(job.text, text);
        let got: Vec<(&str, String, f32)> = job.sections.iter().map(|s| (&text[s.byte_range.start.0..s.byte_range.end.0], format!("{:?}", s.format.font_id.family), s.leading_space)).collect();
        let name = |n: &str| format!("{:?}", FontFamily::Name(n.into()));
        assert_eq!(
            got,
            [
                ("(", "Proportional".to_string(), 3.0),
                (" \u{361}", name("ferrocord-normal+systeme"), 0.0),
                ("°", "Proportional".to_string(), 0.0),
                (" \u{35C}", name("ferrocord-normal+systeme"), 0.0),
                ("ʖ)", "Proportional".to_string(), 0.0),
                ("o\u{35C}", name("ferrocord-gras+systeme"), 0.0),
                ("1\u{FE0F}\u{20E3}", "Proportional".to_string(), 0.0),
                ("o\u{35C}", "Monospace".to_string(), 0.0),
            ]
        );
    }
}

#[cfg(test)]
mod name_tests {
    use super::*;

    const MIXED: &str = "Zoé 🔥 Алиса ★ 你好";

    fn rank(id: u16) -> usize {
        NAMES.iter().position(|n| n.id == id).unwrap_or_else(|| panic!("police {id}"))
    }

    fn all_embedded() -> u16 {
        NAMES.iter().enumerate().filter(|(_, n)| n.embedded()).fold(0, |m, (i, _)| m | 1 << i)
    }

    fn at_rest(out: &egui::FullOutput) -> bool {
        out.viewport_output.values().all(|v| v.repaint_delay == std::time::Duration::MAX)
    }

    /// Identifiant de Discord → police (docs.discord.food, « Display Name
    /// Font ») : les douze polices libres sont embarquées, Néo-Castel, Ribes
    /// et Sinistre gardent un fichier de Windows, gg sans et tout identifiant
    /// inconnu la demi-grasse de l'interface.
    #[test]
    fn discord_font_ids_map_to_their_fonts() {
        let ctx = egui::Context::default();
        let expected: [(u16, &str, &str, bool); 16] = [
            (1, "Bangers", "Bangers-Regular.ttf", false),
            (2, "BioRhyme", "BioRhyme[wdth,wght].ttf", false),
            (3, "Cherry Bomb One", "CherryBombOne-Regular.ttf", true),
            (4, "Chicle", "Chicle-Regular.ttf", true),
            (5, "Compagnon", "Compagnon-Bold.otf", false),
            (6, "MuseoModerno", "MuseoModerno[wght].ttf", true),
            (7, "Néo-Castel", "", true),
            (8, "Pixelify Sans", "PixelifySans[wght].ttf", true),
            (9, "Ribes", "", false),
            (10, "Sinistre", "", true),
            (11, "gg sans", "", true),
            (12, "Zilla Slab", "ZillaSlab-Bold.ttf", true),
            (13, "Playpen Sans", "PlaypenSans[wght].ttf", true),
            (14, "Orbitron", "Orbitron[wght].ttf", true),
            (15, "New Rocker", "NewRocker-Regular.ttf", true),
            (16, "Kalam", "Kalam-Bold.ttf", true),
        ];
        let fonts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/fonts/noms");
        let folder = |font: &str| font.to_lowercase().replace(' ', "");
        for (id, font, file, offered) in expected {
            let n = &NAMES[rank(id)];
            assert_eq!((n.font, n.offered, n.embedded()), (font, offered, !file.is_empty()), "police {id}");
            match n.source {
                NameSource::Interface => assert_eq!((id, styled(&ctx, id)), (11, demi())),
                NameSource::Substitute(windows) => {
                    assert!([7, 9, 10].contains(&id) && windows.ends_with(".ttf"));
                    assert_eq!(styled(&ctx, id), FontFamily::Name(format!("ferrocord-nom-{id}").into()));
                }
                NameSource::Embedded(bytes, weight) => {
                    // Les octets embarqués sont ceux du fichier de la famille, licence à côté.
                    let dir = fonts.join(folder(font));
                    let on_disk = std::fs::read(dir.join(file)).unwrap_or_else(|e| panic!("{file} : {e}"));
                    assert!(bytes == on_disk.as_slice(), "{file}");
                    assert!(matches!(bytes.get(..4), Some([0, 1, 0, 0] | b"OTTO")), "{file}");
                    assert!(dir.join("OFL.txt").is_file() || dir.join("LICENSE.txt").is_file(), "licence de {font}");
                    assert_eq!(weight.is_some(), file.contains("wght"), "{file}");
                    assert_eq!(styled(&ctx, id), FontFamily::Name(format!("ferrocord-nom-{id}").into()));
                }
            }
        }
        assert_eq!(NAMES.iter().filter(|n| n.embedded()).count(), 12);
        for unknown in [0, 17, 999] {
            assert_eq!(styled(&ctx, unknown), demi(), "{unknown}");
        }
        // Le sélecteur : gg sans d'abord, puis les polices que Discord propose encore.
        let offered: Vec<u16> = NAMES.iter().filter(|n| n.offered).map(|n| n.id).collect();
        assert_eq!(offered, [11, 3, 4, 6, 7, 8, 10, 12, 13, 14, 15, 16]);
        assert_eq!(DEFAULT_NAME_FONT, 11);
    }

    /// Au démarrage, aucune police de nom embarquée n'est connue d'egui : les
    /// familles existent (sur la demi-grasse), sans aucun fichier derrière.
    #[test]
    fn startup_loads_no_name_font() {
        for figtree in [false, true] {
            let (defs, _) = build(&Applied { figtree, ..Default::default() });
            let demi_chain = &defs.families[&demi()];
            for n in NAMES.iter().filter(|n| n.embedded()) {
                assert!(!defs.font_data.contains_key(&n.data_name()), "{} chargée au démarrage", n.font);
                assert_eq!(&defs.families[&n.family()], demi_chain, "{}", n.font);
            }
        }
        // Dans un contexte : des images sans nom stylé ne chargent rien.
        let ctx = egui::Context::default();
        install_quiet(&ctx);
        for _ in 0..3 {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                ui.label(egui::RichText::new("Alice").family(demi()));
            });
            out.textures_delta.clear();
        }
        assert_eq!(loaded_names(&ctx), 0);
        assert!(ctx.fonts(|f| NAMES.iter().all(|n| !f.definitions().font_data.contains_key(&n.data_name()))));
    }

    /// Un nom stylé charge sa police à sa première image, elle seule, une
    /// seule fois : une image redemandée, puis plus aucune (repos).
    #[test]
    fn a_name_font_loads_once_when_first_drawn() {
        let ctx = egui::Context::default();
        install_quiet(&ctx);
        let frame = |ids: &[u16]| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                for id in ids {
                    ui.label(egui::RichText::new(MIXED).family(styled(ui.ctx(), *id)).size(20.0));
                }
            });
            out.textures_delta.clear();
            out
        };
        // gg sans, une police sans fichier, un identifiant inconnu : rien à charger.
        for _ in 0..3 {
            frame(&[11, 7, 999]);
        }
        assert_eq!(loaded_names(&ctx), 0);
        assert!(at_rest(&frame(&[11, 7, 999])), "{:?}", ctx.repaint_causes());
        // Sakura : chargée à la première image qui la demande, qui en redemande une.
        let first = frame(&[3]);
        assert_eq!(loaded_names(&ctx), 1 << rank(3));
        assert!(!at_rest(&first), "l'image suivante montre la police");
        frame(&[3]);
        let chain = ctx.fonts(|f| f.definitions().families[&NAMES[rank(3)].family()].clone());
        assert_eq!(chain[0], "nom-3");
        let count = |ctx: &egui::Context| ctx.fonts(|f| f.definitions().font_data.len());
        let loaded = count(&ctx);
        frame(&[3]);
        for i in 0..4 {
            assert!(at_rest(&frame(&[3])), "image {i} : réveil demandé par {:?}", ctx.repaint_causes());
        }
        assert_eq!((count(&ctx), loaded_names(&ctx)), (loaded, 1 << rank(3)), "rien de plus, rien de refait");
        // Deux polices de plus dans la même image (le sélecteur) : les deux, la première gardée.
        frame(&[3, 16, 14]);
        assert_eq!(loaded_names(&ctx), 1 << rank(3) | 1 << rank(16) | 1 << rank(14));
        frame(&[3, 16, 14]);
        frame(&[3, 16, 14]);
        assert_eq!(count(&ctx), loaded + 2);
        assert!(at_rest(&frame(&[3, 16, 14])), "{:?}", ctx.repaint_causes());
        // Les polices de l'interface refaites (thème) : celles déjà chargées restent.
        let (defs, _) = build(&applied(&ctx).unwrap_or_else(|| panic!("installé")));
        assert!(["nom-3", "nom-14", "nom-16"].iter().all(|n| defs.font_data.contains_key(*n)) && !defs.font_data.contains_key("nom-4"));
        // Contexte d'egui nu (aucune installation) : rien n'est tenté.
        let bare = egui::Context::default();
        assert_eq!(styled(&bare, 3), FontFamily::Name("ferrocord-nom-3".into()));
        assert_eq!(loaded_names(&bare), 0);
    }

    /// Derrière une police de nom : la chaîne de la demi-grasse, donc tous
    /// les replis ; sa jumelle la porte aussi en tête.
    #[test]
    fn name_fonts_fall_back_on_the_interface_chain() {
        let (defs, _) = build(&Applied { figtree: true, names: all_embedded(), ..Default::default() });
        let demi_chain = &defs.families[&demi()];
        for n in NAMES.iter().filter(|n| n.embedded()) {
            let chain = &defs.families[&n.family()];
            assert_eq!((chain[0].clone(), &chain[1..]), (n.data_name(), &demi_chain[..]), "{}", n.font);
            let twin = twin(&n.family()).unwrap_or_else(|| panic!("jumelle"));
            assert_eq!(defs.families[&twin][0], n.data_name());
        }
        // Chaque fichier est lisible par egui et dessine le latin lui-même ; le cyrillique
        // qu'il n'a pas vient de la chaîne de l'interface.
        let mut fonts = egui::epaint::Fonts::new(Default::default(), defs);
        for n in NAMES.iter().filter(|n| n.embedded()) {
            assert!(fonts.has_glyphs(&FontId::new(20.0, n.family()), "Alice Алиса"), "{}", n.font);
        }
        let mut view = fonts.with_pixels_per_point(1.0);
        let plain = view.layout_no_wrap("Alice".into(), demi_font(20.0), Color32::WHITE);
        for n in NAMES.iter().filter(|n| n.embedded()) {
            let own = view.layout_no_wrap("Alice".into(), FontId::new(20.0, n.family()), Color32::WHITE);
            assert!(own.size().x > 0.0 && own.size().x != plain.size().x, "{} : même largeur que la demi-grasse", n.font);
        }
    }

    /// Un nom mêlant latin, emoji, cyrillique et chinois : chaque caractère
    /// trouve un glyphe derrière chacune des polices de nom (pas de carré).
    #[cfg(windows)]
    #[test]
    fn mixed_names_show_no_tofu_in_any_name_font() {
        let Some(dir) = fonts_dir() else { return };
        let data = FILES.map(|f| read(&dir, f, MAX_FONT_BYTES));
        if data[0].is_none() {
            return;
        }
        let (defs, _) = build(&Applied { data, extra: read_fallbacks(&dir), figtree: true, quiet: true, names: all_embedded() });
        let mut fonts = egui::epaint::Fonts::new(Default::default(), defs);
        for n in &NAMES {
            let id = FontId::new(20.0, n.family());
            for c in MIXED.chars().filter(|c| !c.is_whitespace()) {
                assert!(fonts.has_glyph(&id, c), "{} : {c:?} sans glyphe", n.font);
            }
        }
    }

    /// Fichiers variables : l'axe de graisse est bien suivi par le moteur de
    /// texte (le même texte est plus large en 700 qu'en 400).
    #[test]
    fn variable_name_fonts_follow_their_weight_axis() {
        let mut seen = 0;
        for n in &NAMES {
            let NameSource::Embedded(bytes, Some(weight)) = n.source else { continue };
            assert_eq!(weight, 700.0, "{}", n.font);
            let mut defs = FontDefinitions::empty();
            for w in ["400", "700"] {
                let coords = egui::epaint::text::VariationCoords::new([(b"wght", if w == "400" { 400.0 } else { 700.0 })]);
                defs.font_data.insert(w.to_owned(), Arc::new(FontData::from_static(bytes).tweak(egui::FontTweak { coords, ..Default::default() })));
                defs.families.insert(FontFamily::Name(w.into()), vec![w.to_owned()]);
            }
            defs.families.insert(FontFamily::Proportional, vec!["400".into()]);
            defs.families.insert(FontFamily::Monospace, vec!["400".into()]);
            let mut fonts = egui::epaint::Fonts::new(Default::default(), defs);
            let mut view = fonts.with_pixels_per_point(1.0);
            let mut width = |w: &str| view.layout_no_wrap("Alice Martin".into(), FontId::new(40.0, FontFamily::Name(w.into())), Color32::WHITE).size().x;
            let (regular, bold) = (width("400"), width("700"));
            eprintln!("{} : {regular} en 400, {bold} en 700", n.font);
            assert!(bold > regular, "{} : {regular} en 400, {bold} en 700", n.font);
            seen += 1;
        }
        assert_eq!(seen, 5);
    }
}
