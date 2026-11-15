//! Emojis Unicode (table complète de Discord, noms courts et catégories) et
//! autocollants. Sans egui ni réseau : le texte des messages est découpé une
//! fois à la réception (`markdown`), l'interface affiche l'image Twemoji de
//! chaque emoji reconnu (`ImageRef::Twemoji`), le réseau en construit
//! l'adresse avec [`Unicode::file`].

mod stickers;
mod table;
mod usable;

pub use stickers::{GuildSticker, StickerEvent, StickerFormat, StickerItem, Stickers};

use std::collections::HashMap;
use std::sync::OnceLock;

/// Une ligne de la table.
#[derive(Debug)]
pub struct Entry {
    /// Séquence telle que Discord l'envoie (sélecteurs de variante compris).
    pub emoji: &'static str,
    /// Noms courts, le premier est celui qu'on affiche.
    pub names: &'static [&'static str],
    /// Catégorie : indice dans [`CATEGORIES`].
    pub cat: u8,
    flags: u8,
}

/// Les cinq teintes de peau existent (règle « teinte après le premier caractère »).
const TONED: u8 = 1;
/// Variante (plusieurs teintes) : reconnue dans le texte, absente du sélecteur.
const HIDDEN: u8 = 2;

impl Entry {
    pub fn toned(&self) -> bool {
        self.flags & TONED != 0
    }

    pub fn hidden(&self) -> bool {
        self.flags & HIDDEN != 0
    }
}

/// Catégories de Discord, dans l'ordre du sélecteur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    People,
    Nature,
    Food,
    Activity,
    Travel,
    Objects,
    Symbols,
    Flags,
}

pub const CATEGORIES: [Category; 8] =
    [Category::People, Category::Nature, Category::Food, Category::Activity, Category::Travel, Category::Objects, Category::Symbols, Category::Flags];

/// Nombre de teintes de peau (1 à 5 ; 0 : jaune, sans teinte).
pub const TONES: u8 = 5;

/// Emoji Unicode connu : sa ligne et sa teinte (0 : aucune, 1 à 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Unicode {
    pub index: u16,
    pub tone: u8,
}

pub fn all() -> &'static [Entry] {
    table::EMOJIS
}

impl Unicode {
    pub fn entry(self) -> Option<&'static Entry> {
        table::EMOJIS.get(usize::from(self.index))
    }

    /// Même emoji avec la teinte `tone` s'il en a, sinon tel quel.
    pub fn with_tone(self, tone: u8) -> Unicode {
        let toned = self.entry().is_some_and(Entry::toned);
        Unicode { index: self.index, tone: if toned && tone <= TONES { tone } else { 0 } }
    }

    /// Séquence à envoyer (teinte comprise).
    pub fn text(self) -> String {
        let Some(e) = self.entry() else { return String::new() };
        if self.tone == 0 || !e.toned() {
            return e.emoji.to_string();
        }
        let mut chars = e.emoji.chars();
        let mut out = String::with_capacity(e.emoji.len() + 4);
        if let Some(first) = chars.next() {
            out.push(first);
        }
        if let Some(t) = char::from_u32(0x1F3FA + u32::from(self.tone)) {
            out.push(t);
        }
        let rest = chars.as_str();
        out.push_str(rest.strip_prefix('\u{FE0F}').unwrap_or(rest));
        out
    }

    /// Nom court affiché (`thumbsup`, `thumbsup_tone2`).
    pub fn name(self) -> String {
        let Some(n) = self.entry().and_then(|e| e.names.first()) else { return String::new() };
        if self.tone == 0 { (*n).to_string() } else { format!("{n}_tone{}", self.tone) }
    }

    /// Nom du fichier Twemoji (`1f44d-1f3fc`) : points de code en
    /// hexadécimal ; les sélecteurs de variante sont retirés sauf dans une
    /// séquence avec liaison (règle de Twemoji, vérifiée sur toute la table
    /// à la génération).
    pub fn file(self) -> String {
        let text = self.text();
        let zwj = text.contains('\u{200D}');
        let parts: Vec<String> = text.chars().filter(|c| zwj || *c != '\u{FE0F}').map(|c| format!("{:x}", u32::from(c))).collect();
        parts.join("-")
    }
}

impl Category {
    pub fn index(self) -> usize {
        self as usize
    }
}

/// Clé de recherche d'une séquence : sans sélecteur de variante.
fn key(s: &str) -> String {
    s.chars().filter(|c| *c != '\u{FE0F}').collect()
}

struct Index {
    by_text: HashMap<String, Unicode>,
    by_name: HashMap<&'static str, u16>,
    /// Longueur maximale d'une clé, en caractères.
    longest: usize,
}

fn index() -> &'static Index {
    static INDEX: OnceLock<Index> = OnceLock::new();
    INDEX.get_or_init(|| {
        let mut by_text = HashMap::with_capacity(table::EMOJIS.len() * 2);
        let mut by_name = HashMap::with_capacity(table::EMOJIS.len() * 2);
        for (i, e) in table::EMOJIS.iter().enumerate() {
            let Ok(index) = u16::try_from(i) else { break };
            let u = Unicode { index, tone: 0 };
            by_text.entry(key(e.emoji)).or_insert(u);
            if e.toned() {
                for tone in 1..=TONES {
                    by_text.entry(key(&Unicode { index, tone }.text())).or_insert(Unicode { index, tone });
                }
            }
            for n in e.names {
                by_name.entry(*n).or_insert(index);
            }
        }
        let longest = by_text.keys().map(|k| k.chars().count()).max().unwrap_or(1);
        Index { by_text, by_name, longest }
    })
}

/// Emoji de ce nom court (`thumbsup`, `thumbsup_tone3`).
pub fn by_name(name: &str) -> Option<Unicode> {
    let idx = index();
    if let Some(i) = idx.by_name.get(name) {
        return Some(Unicode { index: *i, tone: 0 });
    }
    let (base, tone) = name.rsplit_once("_tone")?;
    let tone: u8 = tone.parse().ok().filter(|t| (1..=TONES).contains(t))?;
    let i = idx.by_name.get(base)?;
    let u = Unicode { index: *i, tone: 0 };
    u.entry().filter(|e| e.toned()).map(|_| Unicode { index: *i, tone })
}

/// Emoji dont la séquence est exactement `text` (réaction).
pub fn find(text: &str) -> Option<Unicode> {
    index().by_text.get(&key(text)).copied()
}

/// Caractères BMP présentés en emoji par défaut (Emoji_Presentation=Yes
/// d'Unicode), plus ❤ que Discord montre en emoji. Les autres symboles BMP
/// (©, ™, ↔…) ne deviennent des emojis qu'avec le sélecteur U+FE0F.
fn emoji_by_default(c: char) -> bool {
    const RANGES: &[(u32, u32)] = &[
        (0x231A, 0x231B),
        (0x23E9, 0x23EC),
        (0x23F0, 0x23F0),
        (0x23F3, 0x23F3),
        (0x25FD, 0x25FE),
        (0x2614, 0x2615),
        (0x2648, 0x2653),
        (0x267F, 0x267F),
        (0x2693, 0x2693),
        (0x26A1, 0x26A1),
        (0x26AA, 0x26AB),
        (0x26BD, 0x26BE),
        (0x26C4, 0x26C5),
        (0x26CE, 0x26CE),
        (0x26D4, 0x26D4),
        (0x26EA, 0x26EA),
        (0x26F2, 0x26F3),
        (0x26F5, 0x26F5),
        (0x26FA, 0x26FA),
        (0x26FD, 0x26FD),
        (0x2705, 0x2705),
        (0x270A, 0x270B),
        (0x2728, 0x2728),
        (0x274C, 0x274C),
        (0x274E, 0x274E),
        (0x2753, 0x2755),
        (0x2757, 0x2757),
        (0x2764, 0x2764),
        (0x2795, 0x2797),
        (0x27B0, 0x27B0),
        (0x27BF, 0x27BF),
        (0x2B1B, 0x2B1C),
        (0x2B50, 0x2B50),
        (0x2B55, 0x2B55),
    ];
    let c = u32::from(c);
    c >= 0x1F000 || RANGES.iter().any(|(a, b)| (*a..=*b).contains(&c))
}

/// Caractère qu'Unicode présente en emoji par défaut (Emoji_Presentation=Yes) :
/// la liste de [`emoji_by_default`] sans « ❤ », que seul Discord traite en
/// emoji dans les messages. Les autres symboles se dessinent en texte tant
/// qu'aucun sélecteur U+FE0F ne les suit.
pub fn emoji_presentation(c: char) -> bool {
    c != '\u{2764}' && emoji_by_default(c)
}

/// Emoji qui commence `s` (la séquence la plus longue), et sa longueur en octets.
pub fn match_at(s: &str) -> Option<(usize, Unicode)> {
    let first = s.chars().next()?;
    // Filtre rapide : le texte ordinaire (lettres, ponctuation) ne passe pas.
    if u32::from(first) < 0xA9 && !matches!(first, '#' | '*' | '0'..='9') {
        return None;
    }
    let idx = index();
    let mut k = String::new();
    let mut best = None;
    for (i, c) in s.char_indices().take(idx.longest + 4) {
        let end = i + c.len_utf8();
        if c != '\u{FE0F}' {
            k.push(c);
        }
        if let Some(u) = idx.by_text.get(&k) {
            best = Some((end, *u));
        }
    }
    let (mut end, u) = best?;
    // Sélecteur de variante qui suit : il fait partie de l'emoji.
    if s[end..].starts_with('\u{FE0F}') {
        end += '\u{FE0F}'.len_utf8();
    }
    let matched = &s[..end];
    let single = matched.chars().filter(|c| *c != '\u{FE0F}').count() == 1;
    if single && !matched.contains('\u{FE0F}') && !emoji_by_default(first) {
        return None;
    }
    // Sélecteur de présentation texte (U+FE0E) : le caractère reste du texte.
    if single && s[end..].starts_with('\u{FE0E}') {
        return None;
    }
    Some((end, u))
}

/// Morceaux de `text` : texte ordinaire (`None`) et emojis reconnus, dans
/// l'ordre ; les plages sont en octets.
pub fn split(text: &str) -> Vec<(std::ops::Range<usize>, Option<Unicode>)> {
    let mut out = Vec::new();
    let mut plain = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some((len, u)) = match_at(rest) {
            if plain < i {
                out.push((plain..i, None));
            }
            out.push((i..i + len, Some(u)));
            i += len;
            plain = i;
            continue;
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    if plain < text.len() {
        out.push((plain..text.len(), None));
    }
    out
}

/// Rang d'un nom pour `needle` (en minuscules) : début du nom, début d'un
/// mot, n'importe où.
fn rank(name: &str, needle: &str) -> Option<u8> {
    if name.starts_with(needle) {
        Some(0)
    } else if name.split('_').any(|w| w.starts_with(needle)) {
        Some(1)
    } else if name.contains(needle) {
        Some(2)
    } else {
        None
    }
}

/// Emojis (hors variantes cachées) dont un nom contient `needle`, les
/// meilleurs d'abord (à rang égal, dans l'ordre de Discord), au plus `limit`.
pub fn search(needle: &str, limit: usize) -> Vec<Unicode> {
    let needle = needle.trim().trim_matches(':').to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u8, u16)> = table::EMOJIS
        .iter()
        .enumerate()
        .filter(|(_, e)| !e.hidden())
        .filter_map(|(i, e)| Some((e.names.iter().filter_map(|n| rank(n, &needle)).min()?, u16::try_from(i).ok()?)))
        .collect();
    hits.sort_by_key(|(r, i)| (*r, *i));
    hits.into_iter().take(limit).map(|(_, index)| Unicode { index, tone: 0 }).collect()
}

/// Indices (dans la table) des emojis d'une catégorie montrés par le sélecteur.
pub fn category(cat: Category) -> &'static [u16] {
    static CATS: OnceLock<Vec<Vec<u16>>> = OnceLock::new();
    let cats = CATS.get_or_init(|| {
        let mut v = vec![Vec::new(); CATEGORIES.len()];
        for (i, e) in table::EMOJIS.iter().enumerate() {
            if let (Some(list), Ok(i)) = (v.get_mut(usize::from(e.cat)), u16::try_from(i))
                && !e.hidden()
            {
                list.push(i);
            }
        }
        v
    });
    cats.get(cat.index()).map_or(&[], Vec::as_slice)
}

/// Nom court d'un emoji Unicode (`:fire:`), s'il est connu.
pub fn shortcode(text: &str) -> Option<String> {
    find(text).map(Unicode::name)
}

/// Texte d'un champ de saisie d'emoji : la police de l'interface n'a pas les
/// glyphes des emojis (carré vide), donc un emoji connu s'écrit `:nom:` ;
/// l'image est montrée à côté du champ.
pub fn field_text(stored: &str) -> String {
    shortcode(stored).map_or_else(|| stored.to_owned(), |n| format!(":{n}:"))
}

/// Valeur à enregistrer pour ce que l'utilisateur a tapé : `:nom:` connu
/// devient l'emoji lui-même (seulement fermé par `:`, pour ne pas convertir
/// `ok` pendant qu'on tape `ok_hand`) ; le reste est gardé tel quel (emoji
/// collé, ou nom en cours de frappe). `None` si le champ est vide.
pub fn from_field(input: &str) -> Option<String> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    let named = s.strip_prefix(':').and_then(|x| x.strip_suffix(':')).and_then(by_name);
    Some(named.map_or_else(|| input.to_owned(), Unicode::text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_field_shows_names_and_stores_the_emoji() {
        let up = by_name("thumbsup").map(Unicode::text).unwrap_or_default();
        assert_eq!(field_text(&up), ":thumbsup:");
        assert_eq!(from_field(":thumbsup:").as_deref(), Some(up.as_str()));
        // En cours de frappe, ou inconnu : gardé tel quel.
        assert_eq!(from_field(":thumbs").as_deref(), Some(":thumbs"));
        assert_eq!(from_field("thumbsup").as_deref(), Some("thumbsup"));
        assert_eq!(field_text("abc"), "abc");
        assert_eq!(from_field("  "), None);
    }

    #[test]
    fn table_is_complete_and_consistent() {
        assert!(all().len() > 2000, "{}", all().len());
        for c in CATEGORIES {
            assert!(!category(c).is_empty(), "{c:?}");
        }
        assert!(u16::try_from(all().len()).is_ok());
        // Chaque nom désigne un seul emoji, chaque séquence est retrouvée.
        for (i, e) in all().iter().enumerate() {
            assert!(!e.names.is_empty() && usize::from(e.cat) < CATEGORIES.len());
            assert_eq!(find(e.emoji).map(|u| usize::from(u.index)), Some(i), "{:?}", e.names);
        }
    }

    #[test]
    fn names_tones_and_files() {
        let up = by_name("thumbsup").unwrap_or_else(|| panic!("thumbsup"));
        assert_eq!(up.text(), "👍");
        assert_eq!(up.file(), "1f44d");
        let t2 = by_name("thumbsup_tone2").unwrap_or_else(|| panic!("teinte"));
        assert_eq!((t2.index, t2.tone), (up.index, 2));
        assert_eq!(t2.text(), "👍🏼");
        assert_eq!(t2.file(), "1f44d-1f3fc");
        assert_eq!(t2.name(), "thumbsup_tone2");
        assert_eq!(find("👍🏼"), Some(t2));
        // Sans teinte possible : ignorée.
        let fire = by_name("fire").unwrap_or_else(|| panic!("fire"));
        assert_eq!(fire.with_tone(3), fire);
        assert_eq!(by_name("fire_tone3"), None);
        assert_eq!(by_name("ce_nom_n_existe_pas"), None);
        // Sélecteur retiré hors liaison, gardé dans une séquence liée.
        assert_eq!(by_name("heart").map(Unicode::file).as_deref(), Some("2764"));
        assert_eq!(by_name("hash").map(Unicode::file).as_deref(), Some("23-20e3"));
        assert_eq!(by_name("rainbow_flag").map(Unicode::file).as_deref(), Some("1f3f3-fe0f-200d-1f308"));
        // Teinte après le premier caractère, sélecteur remplacé.
        let point = by_name("point_up").unwrap_or_else(|| panic!("point_up"));
        assert_eq!(point.with_tone(1).text(), "☝🏻");
        assert_eq!(shortcode("🔥").as_deref(), Some("fire"));
        assert_eq!(shortcode("❤\u{FE0F}").as_deref(), Some("heart"));
    }

    #[test]
    fn split_finds_sequences_and_leaves_text() {
        let s = "a👍🏽b 🏳️‍🌈 1 # © ©\u{FE0F} ❤ 1\u{FE0F}\u{20E3}";
        let parts = split(s);
        let emojis: Vec<String> = parts.iter().filter_map(|(r, u)| u.map(|u| (s[r.clone()].to_string(), u)).map(|(t, _)| t)).collect();
        assert_eq!(emojis, ["👍🏽", "🏳️‍🌈", "©\u{FE0F}", "❤", "1\u{FE0F}\u{20E3}"]);
        let back: String = parts.iter().map(|(r, _)| &s[r.clone()]).collect();
        assert_eq!(back, s);
        assert!(split("texte ordinaire, sans rien").iter().all(|(_, u)| u.is_none()));
        assert!(split("").is_empty());
        // Famille liée : une seule image.
        assert_eq!(split("👨‍👩‍👧‍👦").len(), 1);
    }

    /// Découpe des séquences : liaisons (familles, métiers), teintes, drapeaux,
    /// touches et sélecteurs de variante font chacun une seule image ; le
    /// sélecteur de présentation texte laisse le caractère en texte.
    #[test]
    fn split_keeps_whole_sequences() {
        let whole = [
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "👩\u{200D}💻",
            "🧑\u{200D}🚀",
            "🧑🏽\u{200D}🚀",
            "👨\u{200D}🍳",
            "🏳\u{FE0F}\u{200D}🌈",
            "🏴\u{200D}☠\u{FE0F}",
            "❤\u{FE0F}\u{200D}🔥",
            "🐻\u{200D}❄\u{FE0F}",
            "👩\u{200D}❤\u{FE0F}\u{200D}👨",
            "👍🏻",
            "👍🏿",
            "👋🏾",
            "🇫🇷",
            "🇯🇵",
            "🇺🇦",
            "🏴\u{E0067}\u{E0062}\u{E0073}\u{E0063}\u{E0074}\u{E007F}",
            "1\u{FE0F}\u{20E3}",
            "#\u{FE0F}\u{20E3}",
            "*\u{FE0F}\u{20E3}",
            "🔟",
            "☺\u{FE0F}",
            "❤\u{FE0F}",
            "❤",
            "⚠\u{FE0F}",
            "▶\u{FE0F}",
            "🫠",
            "🩷",
        ];
        for s in whole {
            let parts = split(s);
            assert!(parts.len() == 1 && parts[0].0 == (0..s.len()) && parts[0].1.is_some(), "{s:?} : {parts:?}");
            // Collée à du texte et à elle-même : toujours entière.
            let glued = format!("a{s}{s}b");
            let found: Vec<&str> = split(&glued).into_iter().filter(|(_, u)| u.is_some()).map(|(r, _)| &glued[r]).collect();
            assert_eq!(found, [s, s], "{glued:?}");
        }
        // Deux drapeaux à la suite ne se recoupent pas.
        let flags = "🇫🇷🇯🇵";
        assert_eq!(split(flags).iter().map(|(r, _)| &flags[r.clone()]).collect::<Vec<_>>(), ["🇫🇷", "🇯🇵"]);
        // Texte : symbole seul sans sélecteur, ou avec le sélecteur de présentation texte.
        for s in ["☺", "⚠", "™", "↔", "▶", "1", "#", "❤\u{FE0E}", "☺\u{FE0E}", "🔥\u{FE0E}", "★", "ツ", "𝓐", "🅲"] {
            assert!(split(s).iter().all(|(_, u)| u.is_none()), "{s:?}");
        }
    }

    #[test]
    fn search_ranks_prefix_first() {
        let r = search("thumbs", 5);
        assert_eq!(r.first().map(|u| u.name()).as_deref(), Some("thumbsup"));
        assert!(search(":fire:", 3).iter().any(|u| u.name() == "fire"));
        assert!(search("", 5).is_empty());
        assert!(search("zzzzzz", 5).is_empty());
        assert!(search("tone", 50).iter().all(|u| !u.entry().is_some_and(Entry::hidden)));
    }
}
