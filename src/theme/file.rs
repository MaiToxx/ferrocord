//! Lecture et écriture de `theme.toml`. Pur (aucune E/S) : `watch` lit et
//! écrit le fichier, ici on ne fait que convertir texte <-> jetons.
//!
//! Un fichier peut être partiel : les clés absentes viennent du préréglage
//! `base` (Rouille par défaut). Toute erreur (TOML cassé, clé inconnue,
//! couleur illisible, valeur hors bornes) refuse le fichier entier, avec la
//! ligne en cause : on ne veut pas d'un thème appliqué à moitié.
//!
//! Les messages d'erreur et les commentaires du fichier sont dans la langue
//! donnée (`parse_in`, `export_in`) ; `parse` et `export`, appelés par le
//! fil de thème, prennent la langue courante.

use super::{Field, FieldMut, Preset, Tokens, SECTIONS};
use crate::i18n::{self, f, t, Lang};
use eframe::egui::Color32;
use std::{fmt, ops::Range};
use toml::de::{DeTable, DeValue};
use toml::Spanned;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThemeError {
    /// Ligne en cause (à partir de 1), si on la connaît.
    pub line: Option<usize>,
    pub message: String,
    /// Langue de `message`.
    pub lang: Lang,
}

impl fmt::Display for ThemeError {
    fn fmt(&self, fmt: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(l) => fmt.write_str(&f::theme_line(self.lang, l, &self.message)),
            None => fmt.write_str(&self.message),
        }
    }
}

impl ThemeError {
    fn at(lang: Lang, text: &str, span: Option<Range<usize>>, message: impl Into<String>) -> Self {
        let line = span.map(|s| text.get(..s.start).unwrap_or(text).matches('\n').count() + 1);
        Self { line, message: message.into(), lang }
    }
}

/// Version du format écrit par [`export`]. 1 (ou absente) : avant la refonte
/// visuelle ; les valeurs de l'ancien préréglage sont mises à jour à la lecture.
/// 4 : icônes de serveur en carré arrondi dans les préréglages à la géométrie
/// de Rouille.
const VERSION: i64 = 5;

/// Texte qui fait paniquer l'analyseur dans les tests.
#[cfg(test)]
pub(crate) const PANIC_TRIGGER: &str = "# panique-de-test
";

/// Lit un thème, erreurs dans la langue courante.
pub fn parse(text: &str) -> Result<Tokens, ThemeError> {
    parse_in(text, i18n::language())
}

/// Lit un thème. Ne panique sur aucune entrée : une panique de l'analyseur
/// TOML (tiers) devient une erreur ordinaire.
pub fn parse_in(text: &str, lang: Lang) -> Result<Tokens, ThemeError> {
    parse_skipping(text, lang, &[])
}

/// Comme [`parse_in`], en laissant de côté les clés de premier niveau `skip`
/// (un fichier `.ferrotheme` porte, autour du thème, son nom et ses sections
/// propres : `theme::share`). Tout le reste est aussi strict.
pub fn parse_skipping(text: &str, lang: Lang, skip: &[&str]) -> Result<Tokens, ThemeError> {
    crate::panics::contain(|| parse_unguarded(text, lang, skip))
        .unwrap_or_else(|| Err(ThemeError { line: None, message: t::THEME_PARSER_PANIC.get(lang).into(), lang }))
}

fn parse_unguarded(text: &str, lang: Lang, skip: &[&str]) -> Result<Tokens, ThemeError> {
    #[cfg(test)]
    if text == PANIC_TRIGGER {
        panic!("analyseur de test");
    }
    let root = DeTable::parse(text).map_err(|e| ThemeError::at(lang, text, e.span(), syntax_message(lang, e.message())))?;
    let root = root.get_ref();
    let err = |span: Range<usize>, msg: String| ThemeError::at(lang, text, Some(span), msg);

    let base = match root.iter().find(|(k, _)| k.get_ref() == "base") {
        None => Preset::Rouille,
        Some((_, v)) => {
            let name = v.get_ref().as_str().ok_or_else(|| err(v.span(), t::THEME_BASE_NOT_STRING.get(lang).into()))?;
            Preset::from_name(name).ok_or_else(|| err(v.span(), f::theme_unknown_preset(lang, name, preset_list())))?
        }
    };
    // Fichier sans `version` : écrit avant la refonte visuelle (lot E).
    let version = match root.iter().find(|(k, _)| k.get_ref() == "version") {
        None => 1,
        Some((_, v)) => v.get_ref().as_integer().and_then(|i| i64::from_str_radix(i.as_str(), i.radix()).ok()).filter(|n| (1..=VERSION).contains(n)).ok_or_else(|| err(v.span(), f::theme_bad_version(lang, VERSION)))?,
    };
    // Les clés absentes d'un fichier ancien valent ce que valait alors son préréglage.
    let mut tokens = match version {
        ..4 => base.tokens_v3(),
        4 => base.tokens_v4(),
        _ => base.tokens(),
    };
    let mut fields = tokens.fields_mut();

    for (key, value) in root.iter() {
        let section = key.get_ref().as_ref();
        if section == "base" || section == "version" || skip.contains(&section) {
            continue;
        }
        if !SECTIONS.iter().any(|(s, _)| *s == section) {
            return Err(err(key.span(), f::theme_unknown_section(lang, section, section_list())));
        }
        let Some(table) = value.get_ref().as_table() else {
            return Err(err(key.span(), f::theme_not_section(lang, section)));
        };
        for (k, v) in table.iter() {
            let name = k.get_ref().as_ref();
            let Some(field) = fields.iter_mut().find(|f| f.section == section && f.key == name) else {
                return Err(err(k.span(), f::theme_unknown_key(lang, name, section)));
            };
            set(lang, field, v).map_err(|m| err(v.span(), f::theme_field(lang, section, name, m)))?;
        }
    }
    drop(fields);
    if version < 2 {
        let (old, new) = (super::legacy::tokens(tokens.base), tokens.base.tokens_v2());
        migrate(&mut tokens, old, new);
    }
    if version < 3 {
        // Rouille était le thème par défaut : qui ne l'a pas retouché suit le nouveau.
        let target = if tokens.base == Preset::Rouille { Preset::DEFAULT } else { tokens.base };
        let old = tokens.base.tokens_v2();
        migrate(&mut tokens, old, target.tokens());
        tokens.base = target;
    }
    if version < 4 {
        // Rail : qui avait gardé les pastilles rondes de son préréglage suit les carrés arrondis.
        let old = tokens.base.tokens_v3();
        let new = tokens.base.tokens();
        migrate(&mut tokens, old, new);
    }
    if version < 5 {
        // Pastille de statut sur les avatars des messages : qui l'avait laissée affichée (l'ancien
        // défaut) suit le client, qui n'en met pas ; qui l'avait masquée ne voit rien changer.
        let old = tokens.base.tokens_v4();
        let new = tokens.base.tokens();
        migrate(&mut tokens, old, new);
    }
    Ok(tokens)
}

/// Fichier d'une version précédente : il contient toutes les valeurs de son
/// préréglage d'alors (`old`). Chaque valeur restée égale à l'ancienne prend la
/// nouvelle (`new`) ; ce que l'utilisateur avait réglé lui-même est gardé.
/// Version 1 : avant la refonte visuelle (lot E). Version 2 : avant les
/// préréglages calqués sur le client de bureau (lot V1). Version 3 : avant les
/// icônes de serveur en carré arrondi (lot « amis »). Version 4 : avant le
/// fil des messages sans pastille de statut (lot « chatmp »).
fn migrate(tokens: &mut Tokens, mut old: Tokens, mut new: Tokens) {
    for ((cur, old), new) in tokens.fields_mut().into_iter().zip(old.fields_mut()).zip(new.fields_mut()) {
        match (cur.value, old.value, new.value) {
            (FieldMut::Color(c), FieldMut::Color(o), FieldMut::Color(n)) if *c == *o => *c = *n,
            (FieldMut::Palette(c), FieldMut::Palette(o), FieldMut::Palette(n)) if *c == *o => *c = *n,
            (FieldMut::Number(c, _), FieldMut::Number(o, _), FieldMut::Number(n, _)) if *c == *o => *c = *n,
            (FieldMut::Choice(c), FieldMut::Choice(o), FieldMut::Choice(n)) if c.index() == o.index() => c.set_index(n.index()),
            _ => {}
        }
    }
}

/// Les messages de `toml` sont en anglais : phrase dans la langue, détail entre parenthèses.
fn syntax_message(lang: Lang, detail: &str) -> String {
    let detail = detail.lines().next().unwrap_or_default().trim();
    if detail.is_empty() { t::THEME_SYNTAX.get(lang).into() } else { f::theme_syntax_detail(lang, detail) }
}

fn preset_list() -> String {
    Preset::ALL.iter().map(|p| p.key()).collect::<Vec<_>>().join(", ")
}

fn section_list() -> String {
    SECTIONS.iter().map(|(s, _)| *s).collect::<Vec<_>>().join(", ")
}

fn set(lang: Lang, field: &mut Field<'_>, v: &Spanned<DeValue<'_>>) -> Result<(), String> {
    match &mut field.value {
        FieldMut::Color(c) => **c = color(lang, v.get_ref())?,
        FieldMut::Palette(p) => {
            let arr = v.get_ref().as_array().ok_or(t::THEME_COLOR_LIST.get(lang))?;
            if arr.len() != p.len() {
                return Err(f::theme_color_count(lang, p.len(), arr.len()));
            }
            for (slot, item) in p.iter_mut().zip(arr.iter()) {
                *slot = color(lang, item.get_ref())?;
            }
        }
        FieldMut::Number(n, b) => {
            let x = number(lang, v.get_ref())?;
            if !(f64::from(b.min)..=f64::from(b.max)).contains(&x) {
                return Err(f::theme_out_of_range(lang, fmt_num(x as f32), fmt_num(b.min), fmt_num(b.max)));
            }
            **n = x as f32;
        }
        FieldMut::Choice(c) => {
            let s = v.get_ref().as_str().ok_or(t::THEME_QUOTED.get(lang))?;
            let i = c.options().iter().position(|(k, _)| *k == s).ok_or_else(|| f::theme_unknown_value(lang, s, choices(c.options())))?;
            c.set_index(i);
        }
    }
    Ok(())
}

fn kind(lang: Lang, v: &DeValue<'_>) -> &'static str {
    match v {
        DeValue::String(_) => t::KIND_STRING,
        DeValue::Integer(_) => t::KIND_INTEGER,
        DeValue::Float(_) => t::KIND_FLOAT,
        DeValue::Boolean(_) => t::KIND_BOOLEAN,
        DeValue::Datetime(_) => t::KIND_DATETIME,
        DeValue::Array(_) => t::KIND_ARRAY,
        DeValue::Table(_) => t::KIND_TABLE,
    }
    .get(lang)
}

fn choices<T>(opts: &[(&str, T)]) -> String {
    opts.iter().map(|(k, _)| format!("\"{k}\"")).collect::<Vec<_>>().join(", ")
}

fn number(lang: Lang, v: &DeValue<'_>) -> Result<f64, String> {
    let x = if let Some(i) = v.as_integer() {
        i64::from_str_radix(i.as_str(), i.radix()).map_err(|_| t::THEME_INT_TOO_BIG.get(lang).to_string())? as f64
    } else if let Some(fl) = v.as_float() {
        fl.as_str().parse::<f64>().map_err(|_| t::THEME_BAD_NUMBER.get(lang).to_string())?
    } else {
        return Err(f::theme_number_expected(lang, kind(lang, v)));
    };
    if x.is_finite() { Ok(x) } else { Err(t::THEME_FINITE.get(lang).into()) }
}

fn color(lang: Lang, v: &DeValue<'_>) -> Result<Color32, String> {
    let s = v.as_str().ok_or_else(|| f::theme_color_expected(lang, kind(lang, v)))?;
    parse_color(s).ok_or_else(|| f::theme_bad_color(lang, s))
}

/// `#rrggbb` ou `#rrggbbaa` (alpha non prémultiplié).
pub fn parse_color(s: &str) -> Option<Color32> {
    let hex = s.trim().strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color32::from_rgba_unmultiplied(byte(0)?, byte(2)?, byte(4)?, a))
}

pub fn fmt_color(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 { format!("#{r:02x}{g:02x}{b:02x}") } else { format!("#{r:02x}{g:02x}{b:02x}{a:02x}") }
}

/// Plus courte écriture qui se relit à l'identique.
fn fmt_num(x: f32) -> String {
    format!("{x}")
}

/// Fichier complet, commenté dans la langue courante.
pub fn export(tokens: &Tokens) -> String {
    export_in(tokens, i18n::language())
}

/// Fichier complet et commenté dans `lang`, relu à l'identique par [`parse`].
pub fn export_in(tokens: &Tokens, lang: Lang) -> String {
    let mut out = String::new();
    out.push_str(t::THEME_FILE_HEADER.get(lang));
    out.push_str(&f::theme_file_presets(lang, preset_list()));
    out.push_str(&format!("version = {VERSION}\nbase = \"{}\"\n", tokens.base.key()));
    let mut copy = tokens.clone();
    let mut current = "";
    for fl in copy.fields_mut() {
        if fl.section != current {
            current = fl.section;
            let title = SECTIONS.iter().find(|(s, _)| *s == current).map_or(current, |(_, title)| title.get(lang));
            out.push_str(&format!("\n# {title}\n[{current}]\n"));
        }
        let detail = fl.text.detail(lang);
        match fl.value {
            FieldMut::Color(c) => out.push_str(&format!("# {detail}\n{} = \"{}\"\n", fl.key, fmt_color(*c))),
            FieldMut::Palette(p) => {
                let list = p.iter().map(|c| format!("\"{}\"", fmt_color(*c))).collect::<Vec<_>>().join(", ");
                out.push_str(&format!("# {detail}\n{} = [{list}]\n", fl.key));
            }
            FieldMut::Number(n, b) => {
                out.push_str(&f::theme_file_range(lang, detail, fmt_num(b.min), fmt_num(b.max)));
                out.push_str(&format!("{} = {}\n", fl.key, fmt_num(*n)));
            }
            FieldMut::Choice(c) => {
                let key = c.options().get(c.index()).map(|(k, _)| *k).unwrap_or_default();
                out.push_str(&f::theme_file_choices(lang, detail, choices(c.options())));
                out.push_str(&format!("{} = \"{key}\"\n", fl.key));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Clock, Density, MessageMode, RailStyle};

    #[test]
    fn export_import_round_trip_for_every_preset() {
        for p in Preset::ALL {
            let t = p.tokens();
            let text = export(&t);
            assert_eq!(parse(&text), Ok(t.clone()), "{}", p.key());
            // Et un deuxième tour donne exactement le même texte.
            assert_eq!(export(&parse(&text).unwrap_or_default()), text);
        }
    }

    #[test]
    fn round_trip_with_odd_values() {
        let mut t = Preset::Clair.tokens();
        t.colors.hover = Color32::from_rgba_unmultiplied(10, 20, 30, 77);
        t.typography.body = 13.5;
        t.layout.zoom = 1.25;
        t.layout.density = Density::Compact;
        t.layout.messages = MessageMode::Compact;
        t.layout.clock = Clock::H12;
        t.layout.rail_style = RailStyle::List;
        let back = parse(&export(&t));
        assert_eq!(back, Ok(t));
    }

    /// Préréglage que devient `p` lu dans un fichier d'une version précédente.
    fn target(p: Preset) -> Preset {
        if p == Preset::Rouille { Preset::DEFAULT } else { p }
    }

    /// Fichier écrit avant la refonte (toutes les valeurs de l'ancien
    /// préréglage, pas de `version`) ou avant les préréglages calqués
    /// (`version = 2`) : il devient le préréglage d'aujourd'hui (Rouille, qui
    /// était le thème par défaut, devient Sombre), sauf ce que l'utilisateur
    /// avait réglé. Réécrit, il n'est plus touché.
    #[test]
    fn old_files_follow_the_new_presets() {
        let v1_text = |t: &Tokens| export(t).replace("version = 5\n", "");
        let v2_text = |t: &Tokens| export(t).replace("version = 5\n", "version = 2\n");
        let v3_text = |t: &Tokens| export(t).replace("version = 5\n", "version = 3\n");
        let v4_text = |t: &Tokens| export(t).replace("version = 5\n", "version = 4\n");
        for p in Preset::ALL {
            // Version 4 : seule la pastille de statut des messages restée affichée change.
            assert_eq!(parse(&v4_text(&p.tokens_v4())), Ok(p.tokens()), "{}", p.key());
            let mut kept = p.tokens_v4();
            kept.layout.channels_width = 301.0;
            let mut want = p.tokens();
            want.layout.channels_width = 301.0;
            assert_eq!(parse(&v4_text(&kept)), Ok(want), "{}", p.key());
            // Version 3 : seules les pastilles rondes du rail restées celles du préréglage changent.
            assert_eq!(parse(&v3_text(&p.tokens_v3())), Ok(p.tokens()), "{}", p.key());
            let mut round = p.tokens_v3();
            (round.shapes.rail, round.shapes.rail_selected, round.layout.channels_width) = (30.0, 9.0, 301.0);
            // Réglé à la main : gardé ; le reste suit les versions suivantes (pastille de statut, saisie).
            let mut want = round.clone();
            (want.layout.status_dots, want.spacing.composer_inner_y) = (p.tokens().layout.status_dots, p.tokens().spacing.composer_inner_y);
            assert_eq!(parse(&v3_text(&round)), Ok(want), "{}", p.key());
            type Text<'a> = &'a dyn Fn(&Tokens) -> String;
            let cases: [(Tokens, Text<'_>); 2] = [(crate::theme::legacy::tokens(p), &v1_text), (p.tokens_v2(), &v2_text)];
            for (old, text) in cases {
                assert_eq!(parse(&text(&old)), Ok(target(p).tokens()), "{}", p.key());
                // Réglages faits par l'utilisateur : gardés tels quels.
                let mut tuned = old.clone();
                tuned.colors.accent = Color32::from_rgb(1, 2, 3);
                tuned.layout.channels_width = 301.0;
                tuned.layout.clock = Clock::H12;
                let mut want = target(p).tokens();
                want.colors.accent = tuned.colors.accent;
                want.layout.channels_width = 301.0;
                want.layout.clock = Clock::H12;
                assert_eq!(parse(&text(&tuned)), Ok(want), "{}", p.key());
                // Version courante : les anciennes valeurs sont un choix, pas un reste.
                assert_eq!(parse(&export(&old)), Ok(old), "{}", p.key());
            }
        }
        for bad in ["version = 0\n", "version = 6\n","version = \"2\"\n"] {
            let e = parse(bad).expect_err(bad);
            assert_eq!(e.line, Some(1));
            assert!(e.message.contains("version"), "{e}");
        }
    }

    /// Sans `base` ni `version` : le préréglage par défaut.
    #[test]
    fn empty_file_is_the_default() {
        assert_eq!(parse(""), Ok(Preset::DEFAULT.tokens()));
        assert_eq!(parse("# rien\n"), Ok(Preset::DEFAULT.tokens()));
        assert_eq!(Tokens::default(), Preset::DEFAULT.tokens());
    }

    #[test]
    fn partial_file_takes_missing_keys_from_base() {
        let text = "base = \"clair\"\n[couleurs]\naccent = \"#123456\"\n[typographie]\nbody = 16\n";
        let mut want = Preset::Clair.tokens();
        want.colors.accent = Color32::from_rgb(0x12, 0x34, 0x56);
        want.typography.body = 16.0;
        assert_eq!(parse(text), Ok(want));

        // Sans `base` : le préréglage par défaut.
        let mut want = Preset::DEFAULT.tokens();
        want.layout.rail_style = RailStyle::List;
        assert_eq!(parse("[mise_en_page]\nrail_style = \"liste\"\n"), Ok(want));
    }

    fn line_of(text: &str) -> Option<usize> {
        parse(text).err().and_then(|e| e.line)
    }

    #[test]
    fn invalid_files_are_refused_with_line() {
        let cases: &[(&str, usize, &str)] = &[
            ("base = \"rouille\"\n[couleurs\nbg0 = \"#000000\"\n", 2, "syntaxe TOML invalide"),
            ("[couleurs]\nbg0 = \"#00000\"\n", 2, "couleur illisible"),
            ("[couleurs]\n\nbg0 = \"rouge\"\n", 3, "couleur illisible"),
            ("[couleurs]\nbg0 = 12\n", 2, "couleur"),
            ("[couleurs]\nauthors = [\"#000000\"]\n", 2, "couleurs attendu\u{a0}: 8, trouvé\u{a0}: 1"),
            ("[couleurs]\nauthors = [\"#000000\", 1, 2, 3, 4, 5, 6, 7]\n", 2, "couleur"),
            ("[typographie]\nbody = 900\n", 2, "hors bornes"),
            ("[typographie]\nbody = -1\n", 2, "hors bornes"),
            ("[typographie]\nbody = nan\n", 2, "fini"),
            ("[typographie]\nbody = inf\n", 2, "fini"),
            ("[typographie]\nbody = \"13\"\n", 2, "nombre attendu"),
            ("[typographie]\nbody = 99999999999999999999\n", 2, ""),
            ("[mise_en_page]\ndensity = \"serrée\"\n", 2, "inconnue"),
            ("[mise_en_page]\nzoom = 0\n", 2, "hors bornes"),
            ("[couleurs]\ninconnue = \"#000000\"\n", 2, "clé inconnue"),
            ("[autre]\nx = 1\n", 1, "section inconnue"),
            ("couleurs = 3\n", 1, "section"),
            ("base = \"violet\"\n", 1, "préréglage inconnu"),
            ("base = 3\n", 1, "préréglage"),
            ("[couleurs]\nbg0 = \"#000000\"\nbg0 = \"#ffffff\"\n", 3, "syntaxe TOML invalide"),
        ];
        for (text, line, needle) in cases {
            let e = parse(text).expect_err(text);
            assert_eq!(e.line, Some(*line), "{text:?} -> {e}");
            assert!(e.message.contains(needle), "{text:?} -> {e}");
        }
        assert_eq!(line_of("[couleurs]\nbg0 = \"#0000zz\"\n"), Some(2));
    }

    /// Aucune entrée ne fait paniquer le lecteur.
    #[test]
    fn garbage_never_panics() {
        let base = export(&Preset::Contraste.tokens());
        let mut inputs: Vec<String> = vec![
            "\u{0}".into(),
            "[[couleurs]]\n".into(),
            "[couleurs.x]\ny = 1\n".into(),
            "base = \"rouille\"\nbase = \"clair\"\n".into(),
            "[couleurs]\nbg0 = \"#ÿÿÿÿÿÿ\"\n".into(),
            "[couleurs]\nbg0 = \"#é1234\"\n".into(),
            "[typographie]\nbody = 0x10\n".into(),
            "[typographie]\nbody = 1e400\n".into(),
            "[typographie]\nbody = 2024-01-01\n".into(),
            "[mise_en_page]\ndensity = { a = 1 }\n".into(),
            "é".repeat(1000),
        ];
        // Le fichier valide tronqué à chaque position (frontières de caractères).
        for (i, _) in base.char_indices().step_by(7) {
            inputs.push(base[..i].to_string());
        }
        for s in &inputs {
            let _ = parse(s);
        }
    }

    /// L'exemple du README reste un fichier valide.
    #[test]
    fn readme_example_parses() {
        let readme = include_str!("../../README.md");
        let block = readme.split("```toml
").nth(1).and_then(|b| b.split("```").next()).unwrap_or_default();
        let t = parse(block).unwrap_or_else(|e| panic!("exemple du README : {e}"));
        assert_eq!(t.base, Preset::Minuit);
        assert_eq!(t.layout.rail_style, RailStyle::List);
        assert_eq!(t.layout.clock, Clock::H12);
    }

    #[test]
    fn bounds_accept_limits() {
        assert!(parse("[typographie]\nbody = 8\n").is_ok());
        assert!(parse("[typographie]\nbody = 32.0\n").is_ok());
        assert!(parse("[typographie]\nbody = 32.5\n").is_err());
        assert!(parse("[typographie]\nbody = 0x10\n").is_ok());
    }

    #[test]
    fn colors_parse_and_format() {
        assert_eq!(parse_color("#e2733a"), Some(Color32::from_rgb(0xe2, 0x73, 0x3a)));
        assert_eq!(parse_color("#E2733A"), Some(Color32::from_rgb(0xe2, 0x73, 0x3a)));
        assert_eq!(parse_color("#ffffff04"), Some(Color32::from_white_alpha(4)));
        for bad in ["", "#", "e2733a", "#e2733", "#e2733a0", "#gggggg", "#+12345", "#e2733a00ff"] {
            assert_eq!(parse_color(bad), None, "{bad}");
        }
        assert_eq!(fmt_color(Color32::from_white_alpha(4)), "#ffffff04");
        assert_eq!(fmt_color(Color32::from_rgb(1, 2, 3)), "#010203");
    }

    #[test]
    fn export_is_commented_and_readable() {
        let text = export(&Preset::Rouille.tokens());
        assert!(text.starts_with("# Thème Ferrocord."));
        assert!(text.contains("base = \"rouille\""));
        assert!(text.contains("[couleurs]\n# Fond 0"));
        assert!(text.contains("version = 5\nbase = \"rouille\"\n"));
        assert!(text.contains("bg2 = \"#2b2d31\""));
        assert!(text.contains("body = 15\n"));
        assert!(text.contains("density = \"normal\""));
        for (s, _) in SECTIONS {
            assert!(text.contains(&format!("\n[{s}]\n")), "{s}");
        }
    }
}

#[cfg(test)]
mod property {
    use super::*;
    use crate::theme::{FieldMut, CHANNELS_WIDTH, ZOOM};

    /// Petit générateur pseudo-aléatoire déterministe (xorshift).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn f32(&mut self, lo: f32, hi: f32) -> f32 {
            lo + (self.next() % 1_000_001) as f32 / 1_000_000.0 * (hi - lo)
        }
    }

    /// Valeurs extrêmes ou quelconques, dans et hors des bornes.
    fn wild(rng: &mut Rng, lo: f32, hi: f32) -> f32 {
        match rng.next() % 8 {
            0 => lo,
            1 => hi,
            2 => f32::NAN,
            3 => f32::INFINITY,
            4 => -1e9,
            5 => hi * 10.0 + 1.0,
            6 => lo - 0.001,
            _ => rng.f32(lo, hi),
        }
    }

    fn randomize(t: &mut Tokens, rng: &mut Rng) {
        for f in t.fields_mut() {
            match f.value {
                FieldMut::Number(v, b) => *v = wild(rng, b.min, b.max),
                FieldMut::Color(c) => {
                    let [r, g, b, a] = (rng.next() as u32).to_le_bytes();
                    *c = Color32::from_rgba_unmultiplied(r, g, b, a);
                }
                FieldMut::Palette(p) => {
                    for c in p.iter_mut() {
                        let [r, g, b, _] = (rng.next() as u32).to_le_bytes();
                        *c = Color32::from_rgb(r, g, b);
                    }
                }
                FieldMut::Choice(c) => {
                    let n = c.options().len();
                    c.set_index(rng.next() as usize % n);
                }
            }
        }
        t.base = Preset::ALL[rng.next() as usize % Preset::ALL.len()];
    }

    /// Tout thème que l'interface peut enregistrer (passé par `sanitized`,
    /// comme le fait le panneau) est relu à l'identique, bornes comprises.
    #[test]
    fn anything_the_ui_saves_is_read_back() {
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        for i in 0..2000 {
            let mut t = Preset::ALL[i % Preset::ALL.len()].tokens();
            randomize(&mut t, &mut rng);
            let t = t.sanitized();
            let text = export(&t);
            match parse(&text) {
                Ok(back) => assert_eq!(back, t, "tirage {i}"),
                Err(e) => panic!("tirage {i} : export refusé à l'import : {e}\n{text}"),
            }
        }
    }

    /// Valeurs adoptées depuis l'interface (souris, clavier) aux bornes et au-delà.
    #[test]
    fn adopted_values_at_limits_are_read_back() {
        for w in [CHANNELS_WIDTH.min, CHANNELS_WIDTH.max, 0.0, 99_999.0, -5.0, f32::NAN] {
            for z in [ZOOM.min, ZOOM.max, 0.2, 5.0, 1.1, f32::INFINITY] {
                let mut t = Tokens::default();
                t.layout.channels_width = w;
                t.layout.zoom = z;
                let t = t.sanitized();
                assert_eq!(parse(&export(&t)), Ok(t));
            }
        }
    }

    /// Un TOML très imbriqué est refusé sans déborder la pile.
    #[test]
    fn deep_nesting_does_not_overflow() {
        let n = 100_000;
        let arrays = format!("a = {}{}", "[".repeat(n), "]".repeat(n));
        let tables = format!("a = {}{}", "{ b = ".repeat(n), "}".repeat(n));
        let dotted = format!("{} = 1", vec!["a"; n].join("."));
        let header = format!("[{}]\nx = 1", vec!["a"; n].join("."));
        for text in [arrays, tables, dotted, header] {
            assert!(parse(&text).is_err());
        }
    }

    /// La phrase principale des erreurs est dans la langue demandée, avec la ligne.
    #[test]
    fn syntax_errors_are_in_the_requested_language() {
        let text = "base = \"rouille\"\n[couleurs\n";
        let e = parse_in(text, Lang::Fr).expect_err("cassé");
        assert_eq!(e.line, Some(2));
        assert!(e.to_string().starts_with("ligne 2\u{a0}: syntaxe TOML invalide"), "{e}");
        let e = parse_in(text, Lang::En).expect_err("cassé");
        assert!(e.to_string().starts_with("line 2: invalid TOML syntax"), "{e}");
        let e = parse_in("[typographie]\nbody = 900\n", Lang::En).expect_err("hors bornes");
        assert_eq!(e.to_string(), "line 2: typographie.body: 900 out of range (8 to 32)");
        let e = parse_in("[mise_en_page]\ndensity = \"serrée\"\n", Lang::Fr).expect_err("inconnue");
        assert_eq!(e.message, "mise_en_page.density\u{a0}: valeur «\u{a0}serrée\u{a0}» inconnue (attendu\u{a0}: \"aere\", \"normal\", \"compact\")");
    }

    /// Le fichier exporté en anglais est commenté en anglais et se relit à l'identique.
    #[test]
    fn english_export_round_trips() {
        let t = Preset::Minuit.tokens();
        let text = export_in(&t, Lang::En);
        assert!(text.starts_with("# Ferrocord theme."), "{text}");
        assert!(text.contains("\n# Colors\n[couleurs]\n# Background 0"));
        assert!(text.contains("# Vertical spacing: spacious, normal or compact: \"aere\", \"normal\", \"compact\"\n"));
        assert_eq!(parse_in(&text, Lang::En), Ok(t));
    }
}
