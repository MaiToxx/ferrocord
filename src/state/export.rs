//! Export d'une conversation (plugin, lot E1) : période choisie, messages
//! rassemblés (ce qui est en mémoire, puis des pages d'historique au
//! besoin), fichier HTML lisible hors ligne ou texte brut.
//!
//! Ce module ne voit que des messages déjà mis en mots : ni jeton, ni
//! en-tête, ni réseau ne passent par ici. Le fichier ne contient que le
//! titre, la période, les auteurs, les heures, les textes et les fichiers.

use super::model::{snowflake_ms, Message};
use super::search::{date, day_start, snowflake_at};
use super::store::Store;
use crate::i18n::Lang;
use chrono::{FixedOffset, TimeZone};
use std::collections::BTreeMap;

/// Pages d'historique demandées au plus pour un export.
pub const MAX_PAGES: usize = 1000;
/// Messages exportés au plus.
pub const MAX_LINES: usize = 50_000;
/// Octets de pièces jointes téléchargés au plus pour un export.
pub const MAX_FILE_BYTES: u64 = 1 << 30;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Format {
    #[default]
    Html,
    Text,
}

impl Format {
    pub fn extension(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Text => "txt",
        }
    }
}

/// Période : du début du jour `from` à la fin du jour `to` (heure locale),
/// en millisecondes Unix ; `None` : pas de borne.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Range {
    pub from_ms: Option<i64>,
    /// Exclu.
    pub to_ms: Option<i64>,
}

impl Range {
    /// Dates tapées (`AAAA-MM-JJ` ou `JJ/MM/AAAA`, vide : pas de borne).
    /// `Err` : la date qui ne se lit pas, ou la fin si elle précède le début.
    pub fn parse(from: &str, to: &str, utc_offset_s: i32) -> Result<Range, String> {
        let day = |v: &str| -> Result<Option<chrono::NaiveDate>, String> {
            let v = v.trim();
            if v.is_empty() { Ok(None) } else { date(v).map(Some).ok_or_else(|| v.to_string()) }
        };
        let (a, b) = (day(from)?, day(to)?);
        if let (Some(a), Some(b)) = (a, b)
            && b < a
        {
            return Err(to.trim().to_string());
        }
        Ok(Range { from_ms: a.map(|d| day_start(d, utc_offset_s)), to_ms: b.and_then(|d| d.succ_opt()).map(|d| day_start(d, utc_offset_s)) })
    }

    pub fn contains(&self, id: u64) -> bool {
        let at = snowflake_ms(id);
        self.from_ms.is_none_or(|f| at >= f) && self.to_ms.is_none_or(|t| at < t)
    }

    /// Identifiant au-delà duquel rien n'est dans la période (borne haute
    /// d'une demande d'historique) ; `now_ms` sans date de fin.
    pub fn top(&self, now_ms: i64) -> u64 {
        snowflake_at(self.to_ms.map_or(now_ms.saturating_add(1), |t| t.min(now_ms.saturating_add(1))))
    }
}

/// Un fichier joint. `saved` : chemin relatif de sa copie à côté de l'export.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileRef {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub url: String,
    pub image: bool,
    pub saved: Option<String>,
}

/// Un message, mis en mots.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub id: u64,
    pub author: String,
    pub text: String,
    pub edited: bool,
    pub files: Vec<FileRef>,
}

impl Line {
    /// `m` tel qu'il est affiché (mentions en mots) ; `None` s'il n'a rien
    /// à exporter (message système sans texte, message supprimé gardé par le journal).
    pub fn of(store: &Store, m: &Message, lang: Lang, h24: bool) -> Option<Line> {
        if m.meta.is_gone() {
            return None;
        }
        let text = super::journal::plain_text(store, m, lang, h24);
        if text.trim().is_empty() && m.attachments.is_empty() {
            return None;
        }
        let files = m.attachments.iter().map(|a| FileRef { id: a.id, name: a.name.clone(), size: a.size, url: a.url.clone(), image: a.kind == super::model::MediaKind::Image, saved: None }).collect();
        Some(Line { id: m.id, author: m.author.clone(), text, edited: m.edited, files })
    }
}

/// Messages rassemblés pour un export, et où en est la remontée de l'historique.
#[derive(Clone, Debug)]
pub struct Collector {
    pub range: Range,
    lines: BTreeMap<u64, Line>,
    /// Prochaine demande : les messages antérieurs à celui-ci.
    pub cursor: u64,
    /// Borne haute de départ (progression).
    top: u64,
    pub pages: usize,
    /// Plus rien à demander (début du salon ou de la période, ou plafond).
    pub done: bool,
    /// Arrêté par [`MAX_PAGES`] ou [`MAX_LINES`].
    pub capped: bool,
}

impl Collector {
    pub fn new(range: Range, now_ms: i64) -> Collector {
        let top = range.top(now_ms);
        Collector { range, lines: BTreeMap::new(), cursor: top, top, pages: 0, done: false, capped: false }
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn lines(&self) -> impl Iterator<Item = &Line> {
        self.lines.values()
    }

    pub fn lines_mut(&mut self) -> impl Iterator<Item = &mut Line> {
        self.lines.values_mut()
    }

    /// Garde `line` s'il est dans la période.
    pub fn add(&mut self, line: Line) {
        if self.range.contains(line.id) && (self.lines.len() < MAX_LINES || self.lines.contains_key(&line.id)) {
            self.lines.insert(line.id, line);
        }
    }

    /// La mémoire du client couvre déjà tout jusqu'à `oldest` (fenêtre du
    /// salon qui touche le présent) : la remontée reprend de là. `more` :
    /// Discord a encore des messages plus anciens.
    pub fn covered_down_to(&mut self, oldest: u64, more: bool) {
        self.cursor = self.cursor.min(oldest);
        self.finish_if(!more);
    }

    /// Une page d'historique est arrivée : `ids` de ses messages, `full` si
    /// elle était pleine (il peut y en avoir d'autres).
    pub fn page(&mut self, ids: &[u64], full: bool) {
        self.pages += 1;
        if let Some(min) = ids.iter().copied().min() {
            self.cursor = self.cursor.min(min);
        }
        self.finish_if(!full || ids.is_empty());
        if !self.done && (self.pages >= MAX_PAGES || self.lines.len() >= MAX_LINES) {
            self.done = true;
            self.capped = true;
        }
    }

    fn finish_if(&mut self, end: bool) {
        // Passé le début de la période : rien de plus ancien n'est demandé.
        if end || self.range.from_ms.is_some_and(|f| snowflake_ms(self.cursor) <= f) {
            self.done = true;
        }
    }

    /// Part du chemin parcourue, de la fin de la période à son début (ou à
    /// la création du salon `channel_id`).
    pub fn progress(&self, channel_id: u64) -> f32 {
        if self.done {
            return 1.0;
        }
        let top = snowflake_ms(self.top);
        let bottom = self.range.from_ms.unwrap_or_else(|| snowflake_ms(channel_id)).min(top - 1);
        ((top - snowflake_ms(self.cursor)) as f32 / (top - bottom) as f32).clamp(0.0, 1.0)
    }

    /// Fichiers joints des messages gardés : (nombre, octets).
    pub fn files(&self) -> (usize, u64) {
        self.lines.values().flat_map(|l| &l.files).fold((0, 0), |(n, b), f| (n + 1, b + f.size))
    }
}

/// En-tête du fichier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Head {
    /// « #général », « Alice ».
    pub title: String,
    /// Serveur, s'il y en a un.
    pub place: Option<String>,
    pub exported_ms: i64,
    pub range: Range,
    pub lang: Lang,
    pub utc_offset_s: i32,
}

impl Head {
    fn zone(&self) -> FixedOffset {
        FixedOffset::east_opt(self.utc_offset_s).unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC"))
    }

    fn stamp(&self, ms: i64, fmt: &str) -> String {
        self.zone().timestamp_millis_opt(ms).single().map(|d| d.format(fmt).to_string()).unwrap_or_default()
    }

    fn day(&self, ms: i64) -> String {
        self.stamp(ms, if self.lang == Lang::Fr { "%d/%m/%Y" } else { "%Y-%m-%d" })
    }

    /// « Du 01/02/2026 au 03/02/2026 », « Tout l'historique disponible ».
    fn period(&self) -> String {
        use crate::i18n::table::journal::{f as jf, t as jt};
        // La fin est exclue : le dernier jour est la veille de la borne.
        let (a, b) = (self.range.from_ms.map(|f| self.day(f)), self.range.to_ms.map(|t| self.day(t - 1)));
        match (a, b) {
            (None, None) => jt::EX_PERIOD_ALL.get(self.lang).to_string(),
            (Some(a), None) => jf::ex_period_from(self.lang, a),
            (None, Some(b)) => jf::ex_period_to(self.lang, b),
            (Some(a), Some(b)) => jf::ex_period(self.lang, a, b),
        }
    }

    fn summary(&self, count: usize) -> String {
        crate::i18n::table::journal::f::ex_summary(self.lang, count, self.stamp(self.exported_ms, if self.lang == Lang::Fr { "%d/%m/%Y %H:%M" } else { "%Y-%m-%d %H:%M" }))
    }

    fn full_title(&self) -> String {
        match &self.place {
            Some(p) => format!("{} — {p}", self.title),
            None => self.title.clone(),
        }
    }
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
}

/// Adresse sûre dans un `href` : `http(s)` seulement.
fn web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Texte d'un message en HTML : échappé, retours à la ligne gardés, les
/// adresses `http(s)` deviennent des liens.
fn body_html(text: &str, out: &mut String) {
    for (i, line) in text.split('\n').enumerate() {
        if i > 0 {
            out.push_str("<br>\n");
        }
        for (j, word) in line.split(' ').enumerate() {
            if j > 0 {
                out.push(' ');
            }
            if web(word) && word.len() > 8 {
                out.push_str("<a href=\"");
                escape(word, out);
                out.push_str("\">");
                escape(word, out);
                out.push_str("</a>");
            } else {
                escape(word, out);
            }
        }
    }
}

const STYLE: &str = "body{font-family:system-ui,'Segoe UI',sans-serif;background:#1e1f22;color:#dbdee1;margin:0;padding:24px;line-height:1.4}\
main{max-width:860px;margin:0 auto}h1{font-size:1.4em;margin:0 0 4px;color:#f2f3f5}\
.meta{color:#949ba4;font-size:.9em;margin:0 0 16px}.day{color:#949ba4;font-size:.85em;text-align:center;border-bottom:1px solid #3f4147;margin:20px 0 8px;padding-bottom:4px}\
.m{margin:8px 0}.a{font-weight:600;color:#f2f3f5}.t{color:#949ba4;font-size:.8em;margin-left:6px}.e{color:#949ba4;font-size:.8em}\
.f{margin:4px 0}.f img{max-width:420px;max-height:320px;border-radius:6px;display:block;margin-top:4px}a{color:#00a8fc}\
@media print{body{background:#fff;color:#000}.a,h1{color:#000}}";

/// Fichier HTML autonome (style compris, aucun script, rien de chargé du
/// réseau : une image n'est montrée que si sa copie est à côté du fichier).
pub fn html<'a>(head: &Head, lines: impl Iterator<Item = &'a Line>) -> String {
    use crate::i18n::table::journal::t as jt;
    let lines: Vec<&Line> = lines.collect();
    let mut o = String::with_capacity(4096 + lines.len() * 160);
    o.push_str("<!DOCTYPE html>\n<html lang=\"");
    o.push_str(if head.lang == Lang::Fr { "fr" } else { "en" });
    o.push_str("\">\n<head>\n<meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n<title>");
    escape(&head.full_title(), &mut o);
    o.push_str("</title>\n<style>");
    o.push_str(STYLE);
    o.push_str("</style>\n</head>\n<body>\n<main>\n<h1>");
    escape(&head.full_title(), &mut o);
    o.push_str("</h1>\n<p class=\"meta\">");
    escape(&head.period(), &mut o);
    o.push_str(" · ");
    escape(&head.summary(lines.len()), &mut o);
    o.push_str("</p>\n");
    let mut day = String::new();
    for l in lines {
        let at = snowflake_ms(l.id);
        let d = head.day(at);
        if d != day {
            o.push_str("<div class=\"day\">");
            escape(&d, &mut o);
            o.push_str("</div>\n");
            day = d;
        }
        o.push_str("<div class=\"m\"><span class=\"a\">");
        escape(&l.author, &mut o);
        o.push_str("</span><span class=\"t\">");
        escape(&head.stamp(at, "%H:%M"), &mut o);
        o.push_str("</span>");
        if !l.text.is_empty() {
            o.push_str("<div>");
            body_html(&l.text, &mut o);
            if l.edited {
                o.push_str(" <span class=\"e\">");
                escape(jt::EX_EDITED.get(head.lang), &mut o);
                o.push_str("</span>");
            }
            o.push_str("</div>");
        }
        for f in &l.files {
            o.push_str("<div class=\"f\">");
            match &f.saved {
                Some(path) => {
                    o.push_str("<a href=\"");
                    escape(path, &mut o);
                    o.push_str("\">");
                    escape(&f.name, &mut o);
                    o.push_str("</a>");
                    if f.image {
                        o.push_str("<img src=\"");
                        escape(path, &mut o);
                        o.push_str("\" alt=\"");
                        escape(&f.name, &mut o);
                        o.push_str("\">");
                    }
                }
                None if web(&f.url) => {
                    o.push_str("<a href=\"");
                    escape(&f.url, &mut o);
                    o.push_str("\">");
                    escape(&f.name, &mut o);
                    o.push_str("</a> <span class=\"e\">");
                    escape(jt::EX_NOT_INCLUDED.get(head.lang), &mut o);
                    o.push_str("</span>");
                }
                None => escape(&f.name, &mut o),
            }
            o.push_str("</div>");
        }
        o.push_str("</div>\n");
    }
    o.push_str("</main>\n</body>\n</html>\n");
    o
}

/// Texte brut : une ligne par message, `[date heure] auteur : texte`, les
/// lignes suivantes d'un message en retrait, les fichiers nommés dessous.
pub fn text<'a>(head: &Head, lines: impl Iterator<Item = &'a Line>) -> String {
    use crate::i18n::table::journal::{f as jf, t as jt};
    let lines: Vec<&Line> = lines.collect();
    let mut o = String::with_capacity(1024 + lines.len() * 96);
    o.push_str(&head.full_title());
    o.push('\n');
    o.push_str(&head.period());
    o.push('\n');
    o.push_str(&head.summary(lines.len()));
    o.push_str("\n\n");
    for l in lines {
        let at = head.stamp(snowflake_ms(l.id), "%Y-%m-%d %H:%M");
        o.push_str(&jf::ex_text_line(head.lang, at, &l.author));
        let mut first = true;
        for part in l.text.split('\n') {
            if !first {
                o.push_str("\n    ");
            }
            o.push_str(part.trim_end_matches('\r'));
            first = false;
        }
        if l.edited {
            o.push(' ');
            o.push_str(jt::EX_EDITED.get(head.lang));
        }
        o.push('\n');
        for f in &l.files {
            o.push_str("    ");
            o.push_str(&jf::ex_text_file(head.lang, &f.name, f.saved.as_deref().unwrap_or(&f.url)));
            o.push('\n');
        }
    }
    o
}

/// Nom de fichier sans danger tiré de `name` : ni séparateur, ni caractère
/// réservé de Windows, ni point au bout ; borné.
pub fn safe_name(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') { '_' } else { c })
        .collect::<String>()
        .trim_matches(|c: char| c == '.' || c.is_whitespace())
        .chars()
        .take(80)
        .collect();
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    let reserved = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL") || (stem.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && stem.ends_with(|c: char| c.is_ascii_digit()));
    if out.is_empty() || reserved {
        out.insert(0, '_');
    }
    out
}

/// Nom proposé pour le fichier d'export.
pub fn file_name(title: &str, format: Format) -> String {
    let base = safe_name(title.trim_start_matches(['#', '@']));
    format!("{}.{}", if base == "_" { "export".to_string() } else { base }, format.extension())
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;
    /// 2026-02-01 00:00 UTC.
    const T0: i64 = 1_769_904_000_000;

    fn id(ms: i64) -> u64 {
        snowflake_at(ms)
    }

    fn line(ms: i64, author: &str, text: &str) -> Line {
        Line { id: id(ms), author: author.into(), text: text.into(), edited: false, files: Vec::new() }
    }

    fn head(range: Range) -> Head {
        Head { title: "#général".into(), place: Some("Serveur <Alpha>".into()), exported_ms: T0 + 10 * DAY, range, lang: Lang::Fr, utc_offset_s: 3600 }
    }

    #[test]
    fn range_is_whole_local_days_and_rejects_bad_dates() {
        let r = Range::parse("2026-02-01", "02/02/2026", 3600).unwrap();
        assert_eq!(r.from_ms, Some(T0 - 3_600_000));
        assert_eq!(r.to_ms, Some(T0 + 2 * DAY - 3_600_000));
        assert!(r.contains(id(T0)) && r.contains(id(T0 + 2 * DAY - 3_600_001)));
        assert!(!r.contains(id(T0 - 3_600_001)) && !r.contains(id(T0 + 2 * DAY)));
        assert_eq!(Range::parse("", " ", 0), Ok(Range::default()));
        assert_eq!(Range::parse("hier", "", 0), Err("hier".into()));
        assert_eq!(Range::parse("2026-02-03", "2026-02-01", 0), Err("2026-02-01".into()));
        // Sans date de fin : la borne haute est « maintenant ».
        assert_eq!(Range::default().top(T0), id(T0 + 1));
        assert_eq!(r.top(T0 + 30 * DAY), id(T0 + 2 * DAY - 3_600_000));
    }

    #[test]
    fn collector_keeps_the_period_and_stops_at_its_start() {
        let range = Range { from_ms: Some(T0), to_ms: Some(T0 + 3 * DAY) };
        let mut c = Collector::new(range, T0 + 30 * DAY);
        assert_eq!(c.cursor, id(T0 + 3 * DAY));
        assert_eq!(c.progress(1), 0.0);
        // Page pleine, encore dans la période : on continue.
        let ids: Vec<u64> = (0..50).map(|i| id(T0 + 2 * DAY + i * 1000)).collect();
        for i in &ids {
            c.add(Line { id: *i, ..line(0, "A", "x") });
        }
        c.page(&ids, true);
        assert!(!c.done && c.cursor == ids[0] && c.len() == 50);
        assert!(c.progress(1) > 0.3 && c.progress(1) < 0.4);
        // Page qui passe le début de la période : hors période ignoré, terminé.
        c.add(line(T0 + 1000, "A", "dedans"));
        c.add(line(T0 - 1000, "A", "avant"));
        c.page(&[id(T0 + 1000), id(T0 - 1000)], true);
        assert!(c.done && !c.capped && c.len() == 51);
        assert_eq!(c.progress(1), 1.0);
        // Sans début : la fin de l'historique (page incomplète) termine.
        let mut all = Collector::new(Range::default(), T0);
        all.page(&[id(T0 - DAY)], true);
        assert!(!all.done);
        all.page(&[], false);
        assert!(all.done);
        // La mémoire couvre déjà jusqu'au début du salon : rien à demander.
        let mut local = Collector::new(Range::default(), T0);
        local.covered_down_to(id(T0 - DAY), false);
        assert!(local.done && local.pages == 0);
        // Plafond de pages.
        let mut long = Collector::new(Range::default(), T0);
        for p in 0..MAX_PAGES as i64 {
            long.page(&[id(T0 - p * 1000)], true);
        }
        assert!(long.done && long.capped);
    }

    #[test]
    fn html_is_escaped_self_contained_and_ordered() {
        let mut a = line(T0 + 1000, "Alice <b>", "Bonjour & bienvenue\n<script>alert(1)</script> https://exemple.invalid/a?b=1&c=2");
        a.edited = true;
        a.files = vec![
            FileRef { id: 1, name: "plan \"A\".png".into(), size: 10, url: "https://cdn.exemple.invalid/plan.png?ex=1".into(), image: true, saved: Some("export_fichiers/1_plan.png".into()) },
            FileRef { id: 2, name: "notes.txt".into(), size: 5, url: "https://cdn.exemple.invalid/notes.txt".into(), image: false, saved: None },
            FileRef { id: 3, name: "piege".into(), size: 5, url: "javascript:alert(1)".into(), image: false, saved: None },
        ];
        let b = line(T0 + DAY, "Bob", "le lendemain");
        let out = html(&head(Range::parse("2026-02-01", "2026-02-02", 3600).unwrap()), [&a, &b].into_iter());
        assert!(out.starts_with("<!DOCTYPE html>") && out.contains("<meta charset=\"utf-8\">"));
        assert!(out.contains("#général — Serveur &lt;Alpha&gt;"));
        assert!(out.contains("Alice &lt;b&gt;") && !out.contains("<script>") && out.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        assert!(out.contains("Bonjour &amp; bienvenue<br>"));
        assert!(out.contains("<a href=\"https://exemple.invalid/a?b=1&amp;c=2\">"));
        assert!(out.contains("(modifié)"));
        // Fichier copié : lien et image relatifs ; non copié : lien seul, jamais d'image distante.
        assert!(out.contains("<img src=\"export_fichiers/1_plan.png\" alt=\"plan &quot;A&quot;.png\">"));
        assert!(out.contains("<a href=\"https://cdn.exemple.invalid/notes.txt\">notes.txt</a>"));
        assert!(!out.contains("javascript:") && out.contains("piege"));
        assert_eq!(out.matches("<img").count(), 1);
        assert!(!out.contains("<script") && !out.contains("src=\"http"), "rien n'est chargé du réseau");
        // Jours à l'heure locale (UTC+1), dans l'ordre.
        let (d1, d2) = (out.find("01/02/2026").unwrap(), out.find("02/02/2026</div>").unwrap());
        assert!(d1 < d2 && out.find("Alice").unwrap() < out.find("Bob").unwrap());
        assert!(out.contains("Du 01/02/2026 au 02/02/2026") && out.contains("2 messages"));
    }

    #[test]
    fn text_is_one_line_per_message_with_files() {
        let mut a = line(T0 + 1000, "Alice", "Bonjour\nà tous");
        a.files = vec![FileRef { id: 1, name: "plan.png".into(), size: 10, url: "https://cdn.exemple.invalid/plan.png".into(), image: true, saved: None }];
        let out = text(&Head { lang: Lang::En, ..head(Range::default()) }, [&a].into_iter());
        assert!(out.starts_with("#général — Serveur <Alpha>\nAll available history\n"));
        assert!(out.contains("[2026-02-01 01:00] Alice: Bonjour\n    à tous\n    File: plan.png (https://cdn.exemple.invalid/plan.png)\n"), "{out}");
    }

    #[test]
    fn names_are_safe_for_windows() {
        assert_eq!(safe_name("../../etc/pass:wd?.txt"), "_.._etc_pass_wd_.txt");
        assert_eq!(safe_name("con.txt"), "_con.txt");
        assert_eq!(safe_name("COM1"), "_COM1");
        assert_eq!(safe_name(" . "), "_");
        assert_eq!(safe_name(&"a".repeat(200)).len(), 80);
        assert_eq!(file_name("#général", Format::Html), "général.html");
        assert_eq!(file_name("@", Format::Text), "export.txt");
    }
}
