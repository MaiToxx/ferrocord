//! Aperçu de la mise en forme pendant la frappe (F4) : dans la zone de
//! saisie, le gras, l'italique, le souligné, le barré, le code, les
//! citations, les spoilers et les mentions prennent leur aspect, et les
//! marques (`**`, `` ` ``, `>`…) s'estompent, comme dans le client officiel.
//! Le texte du champ ne change pas : seule sa mise en page est colorée.
//!
//! L'analyse est volontairement plus simple que celle des messages reçus
//! (`state::markdown`) : une passe par sorte de marque, linéaire, sans
//! imbrication profonde ; ce n'est qu'un aperçu.

use crate::theme::Tokens;
use eframe::egui::{self, Color32, FontFamily, FontId};

/// Aspect d'un octet du texte saisi.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Fmt {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub spoiler: bool,
    pub mention: bool,
    /// Marque de mise en forme elle-même (estompée).
    pub mark: bool,
    /// Chevron d'une citation.
    pub quote: bool,
}

impl Fmt {
    fn plain(self) -> bool {
        self == Fmt::default()
    }
}

/// Aspect de chaque octet de `text`. `links` : mentions choisies dans
/// l'autocomplétion (texte lisible, syntaxe envoyée).
pub(super) fn scan(text: &str, links: &[(String, String)]) -> Vec<Fmt> {
    let b = text.as_bytes();
    let mut out = vec![Fmt::default(); b.len()];
    if !b.iter().any(|c| matches!(c, b'*' | b'_' | b'~' | b'`' | b'|' | b'>' | b'@' | b'#' | b'<')) {
        return out;
    }
    code(b, &mut out);
    for (mark, set) in [
        ("**", (|f: &mut Fmt| f.bold = true) as fn(&mut Fmt)),
        ("__", |f| f.underline = true),
        ("~~", |f| f.strike = true),
        ("||", |f| f.spoiler = true),
        ("*", |f| f.italic = true),
        ("_", |f| f.italic = true),
    ] {
        pairs(b, &mut out, mark.as_bytes(), set);
    }
    quotes(b, &mut out);
    mentions(text, links, &mut out);
    out
}

/// Blocs ``` et code en ligne : rien d'autre n'y est interprété.
fn code(b: &[u8], out: &mut [Fmt]) {
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'`' {
            i += 1;
            continue;
        }
        let n = if b[i..].starts_with(b"```") { 3 } else { 1 };
        let body = i + n;
        let close = (body..b.len()).find(|&j| b[j..].starts_with(&b[i..i + n]) && (n == 3 || b[j] != b'\n'));
        // Code en ligne : sur une seule ligne, et non vide.
        let ok = close.filter(|&j| j > body && (n == 3 || !b[body..j].contains(&b'\n')));
        let Some(j) = ok else {
            i += n;
            continue;
        };
        for (k, f) in out.iter_mut().enumerate().take(j + n).skip(i) {
            f.code = true;
            f.mark = k < body || k >= j;
        }
        i = j + n;
    }
}

fn word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c >= 0x80
}

/// Passages entourés de `mark` (hors code, hors marques déjà prises), sur
/// une même ligne et non vides.
fn pairs(b: &[u8], out: &mut [Fmt], mark: &[u8], set: fn(&mut Fmt)) {
    let n = mark.len();
    let free = |out: &[Fmt], i: usize| i + n <= b.len() && &b[i..i + n] == mark && out[i..i + n].iter().all(|f| !f.code && !f.mark);
    // « _ » au milieu d'un mot (nom_de_variable) n'est pas une marque.
    let under = mark == b"_";
    let mut i = 0;
    while i < b.len() {
        let opens = free(out, i) && b.get(i + n).is_some_and(|c| !c.is_ascii_whitespace()) && !(under && i > 0 && word(b[i - 1]));
        if !opens {
            i += 1;
            continue;
        }
        let mut j = i + n + 1;
        let mut close = None;
        while j < b.len() && b[j] != b'\n' {
            if free(out, j) && !b[j - 1].is_ascii_whitespace() && !(under && b.get(j + n).is_some_and(|c| word(*c))) {
                close = Some(j);
                break;
            }
            j += 1;
        }
        let Some(j) = close else {
            i += n;
            continue;
        };
        for f in &mut out[i..i + n] {
            f.mark = true;
        }
        for f in &mut out[j..j + n] {
            f.mark = true;
        }
        for f in &mut out[i + n..j] {
            if !f.code {
                set(f);
            }
        }
        i = j + n;
    }
}

/// « > » ou « >>> » suivi d'une espace en début de ligne.
fn quotes(b: &[u8], out: &mut [Fmt]) {
    let mut start = 0;
    while start < b.len() {
        let line = &b[start..];
        let n = if line.starts_with(b">>> ") {
            3
        } else if line.starts_with(b"> ") {
            1
        } else {
            0
        };
        if n > 0 && !out[start].code {
            for f in &mut out[start..start + n] {
                f.quote = true;
                f.mark = true;
            }
        }
        start += line.iter().position(|c| *c == b'\n').map_or(line.len(), |p| p + 1);
    }
}

/// Mentions : celles de l'autocomplétion (`@nom`, `#salon`), les syntaxes
/// brutes (`<@1>`, `<@&2>`, `<#3>`), `@everyone` et `@here`.
fn mentions(text: &str, links: &[(String, String)], out: &mut [Fmt]) {
    let b = text.as_bytes();
    let mark = |from: usize, len: usize, out: &mut [Fmt]| {
        if out[from..from + len].iter().all(|f| !f.code) {
            for f in &mut out[from..from + len] {
                *f = Fmt { mention: true, ..Fmt::default() };
            }
        }
    };
    let named = links.iter().map(|(shown, _)| shown.as_str()).filter(|s| s.starts_with('@') || s.starts_with('#')).chain(["@everyone", "@here"]);
    for shown in named {
        let mut from = 0;
        while let Some(p) = text[from..].find(shown) {
            let at = from + p;
            let end = at + shown.len();
            // Pas au milieu d'un mot plus long (« @alice2 » n'est pas « @alice »).
            if !b.get(end).is_some_and(|c| word(*c)) {
                mark(at, shown.len(), out);
            }
            from = end;
        }
    }
    let mut i = 0;
    while let Some(p) = text[i..].find('<') {
        let at = i + p;
        let rest = &b[at + 1..];
        let lead = if rest.starts_with(b"@&") || rest.starts_with(b"@!") {
            2
        } else if rest.starts_with(b"@") || rest.starts_with(b"#") {
            1
        } else {
            0
        };
        let digits = rest[lead.min(rest.len())..].iter().take_while(|c| c.is_ascii_digit()).count();
        if lead > 0 && (1..=20).contains(&digits) && rest.get(lead + digits) == Some(&b'>') {
            mark(at, lead + digits + 2, out);
            i = at + lead + digits + 2;
        } else {
            i = at + 1;
        }
    }
}

/// Mise en forme d'egui pour un aspect. `base` : celle du texte ordinaire
/// du champ (couleur réservée d'egui, remplacée par celle du champ).
pub(super) fn format(t: &Tokens, real: super::fonts::Real, font: &FontId, f: Fmt) -> egui::TextFormat {
    let mut out = egui::TextFormat { font_id: font.clone(), color: Color32::PLACEHOLDER, ..Default::default() };
    if f.plain() {
        return out;
    }
    let face = super::fonts::Face { bold: f.bold && real.bold, italic: f.italic && real.italic };
    if face.bold || face.italic {
        out.font_id = super::fonts::font(font.size, face);
    }
    if f.bold && !real.bold {
        out.color = t.colors.text_strong;
    }
    out.italics = f.italic && !face.italic;
    if f.code {
        out.font_id = FontId::new(font.size, FontFamily::Monospace);
        out.background = t.markdown.code_bg;
    }
    if f.spoiler {
        out.background = t.markdown.spoiler.gamma_multiply(0.45);
    }
    if f.mention {
        out.color = t.markdown.mention_text;
        out.background = t.markdown.mention_bg;
    }
    if f.mark {
        out.color = if f.quote { t.markdown.quote_bar } else { t.colors.faint };
        out.italics = false;
    }
    let line = |c: Color32| egui::Stroke::new(t.shapes.border.max(1.0), if c == Color32::PLACEHOLDER { t.colors.text } else { c });
    if f.underline && !f.mark {
        out.underline = line(out.color);
    }
    if f.strike && !f.mark {
        out.strikethrough = line(out.color);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Une lettre par octet : g(ras), i(talique), s(ouligné), b(arré),
    /// c(ode), p (spoiler), m(ention), q (citation), « . » marque, « - » rien.
    fn view(text: &str, links: &[(String, String)]) -> String {
        scan(text, links)
            .iter()
            .map(|f| match f {
                f if f.quote => 'q',
                f if f.mark => '.',
                f if f.mention => 'm',
                f if f.code => 'c',
                f if f.bold && f.italic => 'G',
                f if f.bold => 'g',
                f if f.italic => 'i',
                f if f.underline => 's',
                f if f.strike => 'b',
                f if f.spoiler => 'p',
                _ => '-',
            })
            .collect()
    }

    #[test]
    fn marks_fade_and_content_takes_its_style() {
        assert_eq!(view("a **b** c", &[]), "--..g..--");
        assert_eq!(view("*i* _j_ __s__ ~~b~~ ||p||", &[]), ".i.-.i.-..s..-..b..-..p..");
        assert_eq!(view("***x***", &[]), "...G...");
        assert_eq!(view("`a*b*` *c*", &[]), ".cccc.-.i.");
        assert_eq!(view("```\n**x**\n```", &[]), "...ccccccc...");
        assert_eq!(view("> cité\nnon > là", &[]), "q----------------");
    }

    #[test]
    fn unfinished_or_inner_marks_are_left_alone() {
        assert_eq!(view("**pas fini", &[]), "----------");
        assert_eq!(view("nom_de_variable", &[]), "---------------");
        assert_eq!(view("2 * 3 * 4", &[]), "---------");
        assert_eq!(view("* liste", &[]), "-------");
        assert_eq!(view("`` vide", &[]), "-------");
        assert_eq!(view("", &[]), "");
    }

    #[test]
    fn mentions_are_marked() {
        let links = vec![("@alice".to_string(), "<@5>".to_string()), (":blob:".to_string(), "<:blob:9>".to_string())];
        assert_eq!(view("@alice et @alice2", &links), "mmmmmm-----------");
        assert_eq!(view("<@12> <@&3> <#4> <x> <@>", &[]), "mmmmm-mmmmm-mmmm--------");
        assert_eq!(view("@everyone `@here`", &[]), "mmmmmmmmm-.ccccc.");
    }

    #[test]
    fn non_ascii_text_keeps_byte_alignment() {
        let text = "été **déjà** là";
        let v = scan(text, &[]);
        assert_eq!(v.len(), text.len());
        let bold: String = text.char_indices().filter(|(i, _)| v[*i].bold && !v[*i].mark).map(|(_, c)| c).collect();
        assert_eq!(bold, "déjà");
    }
}
