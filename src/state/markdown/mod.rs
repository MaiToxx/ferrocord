//! Mise en forme des messages (markdown de Discord), sans egui.
//!
//! Le texte source d'un message est analysé une seule fois en un [`Doc`] :
//! une liste plate de blocs (paragraphes, titres, éléments de liste, blocs
//! de code), chacun fait de passages ([`Span`]) dont le style est déjà
//! calculé. Les mentions et les horodatages sont mis en mots pendant
//! l'analyse, par un [`Resolve`] (noms, couleurs de rôle, langue, horloge).
//!
//! Coût : linéaire en la taille du texte, y compris sur une entrée hostile
//! (marqueurs jamais fermés, crochets ouverts par milliers…) : chaque
//! recherche vers l'avant s'arrête au premier caractère qui l'invalide, et
//! les marqueurs d'emphase sont appariés par une pile (voir `inline`).

mod inline;
mod resolve;
pub mod syntax;
#[cfg(test)]
mod tests;

pub use resolve::{parse_message, MessageContext};
pub use syntax::Class;

/// Mise en mots de ce qui dépend du magasin, de la langue ou de l'horloge.
pub trait Resolve {
    /// Nom affiché d'un utilisateur mentionné (sans `@`).
    fn user(&self, id: u64) -> String;
    /// Nom (sans `@`) et couleur (0xRRGGBB, 0 : aucune) d'un rôle.
    fn role(&self, id: u64) -> (String, u32);
    /// Nom d'un salon (sans `#`).
    fn channel(&self, id: u64) -> String;
    /// Horodatage `<t:secondes:style>` ; `None` : laissé tel quel (hors plage).
    fn time(&self, secs: i64, style: TimeStyle) -> Option<String>;
    /// Horodatage relatif `<t:…:R>` : instant (ms Unix) où son texte changera.
    fn refresh_at(&self, _secs: i64) -> Option<i64> {
        None
    }
}

/// Format d'un horodatage `<t:…:X>`, comme dans Discord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimeStyle {
    /// `t` : 16:20
    ShortTime,
    /// `T` : 16:20:30
    LongTime,
    /// `d` : 20/04/2021
    ShortDate,
    /// `D` : 20 avril 2021
    LongDate,
    /// `f` (par défaut) : 20 avril 2021 16:20
    ShortDateTime,
    /// `F` : mardi 20 avril 2021 16:20
    LongDateTime,
    /// `R` : il y a 2 mois
    Relative,
}

impl TimeStyle {
    pub fn from_letter(c: &str) -> Option<TimeStyle> {
        Some(match c {
            "t" => TimeStyle::ShortTime,
            "T" => TimeStyle::LongTime,
            "d" => TimeStyle::ShortDate,
            "D" => TimeStyle::LongDate,
            "f" => TimeStyle::ShortDateTime,
            "F" => TimeStyle::LongDateTime,
            "R" => TimeStyle::Relative,
            _ => return None,
        })
    }

    pub fn letter(self) -> char {
        match self {
            TimeStyle::ShortTime => 't',
            TimeStyle::LongTime => 'T',
            TimeStyle::ShortDate => 'd',
            TimeStyle::LongDate => 'D',
            TimeStyle::ShortDateTime => 'f',
            TimeStyle::LongDateTime => 'F',
            TimeStyle::Relative => 'R',
        }
    }
}

/// Message mis en forme.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Doc {
    pub blocks: Vec<Block>,
    /// Adresses des liens, désignées par `SpanKind::Link`.
    pub links: Vec<String>,
    /// Nombre de spoilers (numérotés de 0 à `spoilers - 1` dans l'ordre du texte).
    pub spoilers: u32,
    /// Premier instant (ms Unix) où un horodatage relatif change de texte :
    /// le message est alors remis en forme.
    pub refresh_at_ms: Option<i64>,
    /// Textes rattachés au message, mis en forme avec lui ; leurs spoilers
    /// sont numérotés à la suite de ceux du message (un seul jeu de
    /// spoilers révélés par message, sans collision) : extrait de la
    /// réponse, message transféré, et textes des embeds (embed,
    /// emplacement : 0 la description, 1 + n le champ n).
    pub reply: Option<Box<Doc>>,
    pub forward: Option<Box<Doc>>,
    pub embeds: Vec<(usize, usize, Doc)>,
    /// Textes des composants du message (lot T11), dans l'ordre de `Component::texts`.
    pub components: Vec<Doc>,
}

impl Doc {
    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// Texte sans mise en forme, un bloc par ligne : mentions et heures
    /// mises en mots, marqueurs retirés (extrait de réponse, transfert, épingles).
    pub fn plain(&self) -> String {
        let lines: Vec<String> = self.blocks.iter().map(|b| b.spans.iter().map(|s| s.text.as_str()).collect()).collect();
        lines.join("\n")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Block {
    pub kind: BlockKind,
    /// Dans une citation (`>` ou `>>>`) ; les citations ne s'imbriquent pas.
    pub quote: bool,
    pub spans: Vec<Span>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum BlockKind {
    Paragraph,
    /// Titre `#`, `##` ou `###` (1 à 3).
    Heading(u8),
    /// Petit texte `-#`.
    Subtext,
    /// Élément de liste ; `depth` : 0 pour le premier niveau.
    Item { depth: u8, marker: Marker },
    /// Bloc de code ; les passages portent leur classe de coloration.
    Code { lang: Option<String>, spoiler: Option<u32> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    Bullet,
    Number(u64),
}

/// Morceau de texte d'un seul style.
#[derive(Clone, Debug, PartialEq)]
pub struct Span {
    pub text: String,
    pub style: Style,
    pub kind: SpanKind,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    /// Code en ligne (police à chasse fixe).
    pub code: bool,
    /// Spoiler le plus extérieur qui contient ce passage.
    pub spoiler: Option<u32>,
    /// Mot cherché, surligné dans les résultats de recherche (lot T12).
    pub mark: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SpanKind {
    Text,
    /// Lien : indice dans `Doc::links`.
    Link(u32),
    Mention(Mention),
    /// Horodatage `<t:…>` mis en mots (secondes Unix).
    Time(i64),
    /// Emoji personnalisé `<:nom:id>` ; le texte est `:nom:` en attendant son image.
    Emoji { id: u64, name: String, animated: bool },
    /// Emoji Unicode reconnu (une séquence, le texte est son caractère) : affiché en image.
    Unicode(crate::state::emoji::Unicode),
    /// Morceau d'un bloc de code.
    Code(Class),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mention {
    User(u64),
    /// `color` : 0xRRGGBB, 0 si le rôle n'a pas de couleur.
    Role { id: u64, color: u32 },
    Channel(u64),
    Everyone,
    Here,
    /// Commande `</nom:id>`.
    Command(u64),
}

/// Profondeur maximale des listes imbriquées (au-delà : même niveau).
pub const MAX_LIST_DEPTH: usize = 8;

/// Analyse `src`. Ne panique sur aucune entrée.
pub fn parse(src: &str, r: &dyn Resolve) -> Doc {
    parse_from(src, r, 0)
}

/// Comme [`parse`], spoilers numérotés à partir de `first_spoiler` (`Doc::spoilers`
/// est alors le prochain numéro libre).
pub fn parse_from(src: &str, r: &dyn Resolve, first_spoiler: u32) -> Doc {
    let mut p = Parser { r, doc: Doc { spoilers: first_spoiler, ..Doc::default() } };
    p.blocks(src, false);
    split_emojis(&mut p.doc);
    p.doc
}

/// Emojis Unicode du texte mis à part (code en ligne compris, comme Discord ;
/// pas dans un bloc de code), pour être affichés en image ; une fois, à l'analyse.
pub(crate) fn split_emojis(doc: &mut Doc) {
    for b in &mut doc.blocks {
        if matches!(b.kind, BlockKind::Code { .. }) || !b.spans.iter().any(|s| s.kind == SpanKind::Text && !s.text.is_ascii()) {
            continue;
        }
        let mut out = Vec::with_capacity(b.spans.len());
        for s in b.spans.drain(..) {
            if s.kind != SpanKind::Text || s.text.is_ascii() {
                out.push(s);
                continue;
            }
            let parts = crate::state::emoji::split(&s.text);
            if parts.iter().all(|(_, u)| u.is_none()) {
                out.push(s);
                continue;
            }
            for (r, u) in parts {
                let kind = u.map_or(SpanKind::Text, SpanKind::Unicode);
                out.push(Span { text: s.text[r].to_string(), style: s.style, kind });
            }
        }
        b.spans = out;
    }
}

struct Parser<'r> {
    r: &'r dyn Resolve,
    doc: Doc,
}

/// Liste en cours : retraits des niveaux ouverts et dernier numéro de chacun.
#[derive(Default)]
struct ListState {
    indents: Vec<usize>,
    numbers: Vec<Option<u64>>,
}

impl ListState {
    fn clear(&mut self) {
        self.indents.clear();
        self.numbers.clear();
    }

    /// Niveau d'un élément au retrait `indent`, et son marqueur affiché.
    fn item(&mut self, indent: usize, number: Option<u64>) -> (u8, Marker) {
        while self.indents.last().is_some_and(|t| *t > indent) {
            self.indents.pop();
        }
        if self.indents.last() != Some(&indent) && self.indents.len() < MAX_LIST_DEPTH {
            self.indents.push(indent);
        }
        let depth = self.indents.len().saturating_sub(1);
        self.numbers.resize(depth + 1, None);
        let marker = match number {
            Some(n) => {
                let n = self.numbers[depth].map_or(n, |prev| prev.saturating_add(1));
                self.numbers[depth] = Some(n);
                Marker::Number(n)
            }
            None => {
                self.numbers[depth] = None;
                Marker::Bullet
            }
        };
        (depth as u8, marker)
    }
}

/// Paragraphe en cours : texte source accumulé.
#[derive(Default)]
struct Para {
    text: String,
    open: bool,
}

impl Para {
    fn line(&mut self, line: &str) {
        if self.open {
            self.text.push('\n');
        }
        self.text.push_str(line);
        self.open = true;
    }

    /// Suite de la ligne courante (après un bloc de code fermé au milieu).
    fn append(&mut self, s: &str) {
        self.text.push_str(s);
        self.open = true;
    }
}

impl Parser<'_> {
    fn flush(&mut self, para: &mut Para, quote: bool) {
        if !para.open {
            return;
        }
        let text = std::mem::take(&mut para.text);
        para.open = false;
        let spans = self.inline(&text);
        self.push(BlockKind::Paragraph, quote, spans);
    }

    fn push(&mut self, kind: BlockKind, quote: bool, spans: Vec<Span>) {
        self.doc.blocks.push(Block { kind, quote, spans });
    }

    fn inline(&mut self, text: &str) -> Vec<Span> {
        inline::parse(text, self.r, &mut self.doc)
    }

    /// Blocs de `src`. `quote` : contenu d'une citation (pas de citation dedans).
    fn blocks(&mut self, src: &str, quote: bool) {
        let mut para = Para::default();
        let mut list = ListState::default();
        let mut pos = 0;
        // Reste d'une ligne après un bloc de code : pas en début de ligne.
        let mut mid_line = false;
        while pos < src.len() {
            let line_end = src[pos..].find('\n').map_or(src.len(), |e| pos + e);
            let line = &src[pos..line_end];
            let next = if line_end < src.len() { line_end + 1 } else { src.len() };
            let at_start = !mid_line;
            mid_line = false;

            // Citations d'abord : un bloc de code peut être cité.
            if at_start && !quote {
                if let Some(rest) = line.strip_prefix(">>> ") {
                    self.flush(&mut para, quote);
                    let start = pos + (line.len() - rest.len());
                    self.blocks(&src[start..], true);
                    return;
                }
                if quote_line(line).is_some() {
                    self.flush(&mut para, quote);
                    list.clear();
                    let mut inner = String::new();
                    let mut p = pos;
                    while p < src.len() {
                        let e = src[p..].find('\n').map_or(src.len(), |e| p + e);
                        let Some(content) = quote_line(&src[p..e]) else { break };
                        if p != pos {
                            inner.push('\n');
                        }
                        inner.push_str(content);
                        p = if e < src.len() { e + 1 } else { src.len() };
                    }
                    self.blocks(&inner, true);
                    pos = p;
                    continue;
                }
            }

            // Bloc de code : peut s'ouvrir n'importe où dans la ligne.
            if let Some(f) = find_fence(line) {
                let open = pos + f;
                if let Some(fence) = code_fence(src, open) {
                    let before = &line[..f];
                    // `||` collé autour du bloc : bloc de code en spoiler.
                    let after = &src[fence.end..];
                    let spoiler_around = before.ends_with("||") && after.starts_with("||");
                    let before = if spoiler_around { &before[..before.len() - 2] } else { before };
                    if !before.is_empty() {
                        if at_start {
                            para.line(before);
                        } else {
                            para.append(before);
                        }
                    }
                    self.flush(&mut para, quote);
                    list.clear();
                    let spoiler = spoiler_around.then(|| self.next_spoiler());
                    let spans = syntax::highlight(fence.lang, fence.code);
                    self.push(BlockKind::Code { lang: fence.lang.map(str::to_ascii_lowercase), spoiler }, quote, spans);
                    pos = if spoiler_around { fence.end + 2 } else { fence.end };
                    // La suite de la ligne, s'il y en a, forme un nouveau paragraphe.
                    let rest_end = src[pos..].find('\n').map_or(src.len(), |e| pos + e);
                    if rest_end > pos {
                        mid_line = true;
                    } else if rest_end < src.len() {
                        pos = rest_end + 1;
                    } else {
                        pos = src.len();
                    }
                    continue;
                }
            }

            if !at_start {
                para.append(line);
                pos = next;
                continue;
            }

            if let Some((level, text)) = heading(line) {
                self.flush(&mut para, quote);
                list.clear();
                let spans = self.inline(text);
                self.push(BlockKind::Heading(level), quote, spans);
            } else if let Some(text) = line.strip_prefix("-# ").filter(|t| !t.trim().is_empty()) {
                self.flush(&mut para, quote);
                list.clear();
                let spans = self.inline(text);
                self.push(BlockKind::Subtext, quote, spans);
            } else if let Some((indent, number, text)) = list_item(line) {
                self.flush(&mut para, quote);
                let (depth, marker) = list.item(indent, number);
                let spans = self.inline(text);
                self.push(BlockKind::Item { depth, marker }, quote, spans);
            } else {
                list.clear();
                para.line(line);
            }
            pos = next;
        }
        self.flush(&mut para, quote);
    }

    fn next_spoiler(&mut self) -> u32 {
        let n = self.doc.spoilers;
        self.doc.spoilers = n.saturating_add(1);
        n
    }
}

/// Contenu d'une ligne de citation (`> texte`, ou `>` seul).
fn quote_line(line: &str) -> Option<&str> {
    if line == ">" {
        return Some("");
    }
    line.strip_prefix("> ")
}

/// Titre `# `, `## ` ou `### ` suivi d'un texte.
fn heading(line: &str) -> Option<(u8, &str)> {
    let hashes = line.bytes().take_while(|b| *b == b'#').count();
    if !(1..=3).contains(&hashes) {
        return None;
    }
    let text = line[hashes..].strip_prefix(' ')?;
    (!text.trim().is_empty()).then_some((hashes as u8, text))
}

/// Élément de liste : (retrait, numéro, texte). `- `, `* ` ou `12. `.
fn list_item(line: &str) -> Option<(usize, Option<u64>, &str)> {
    let indent = line.bytes().take_while(|b| *b == b' ').count();
    let rest = &line[indent..];
    let (number, text) = if let Some(t) = rest.strip_prefix("- ").or_else(|| rest.strip_prefix("* ")) {
        (None, t)
    } else {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        if digits == 0 || digits > 9 {
            return None;
        }
        let t = rest[digits..].strip_prefix(". ")?;
        (Some(rest[..digits].parse::<u64>().ok()?), t)
    };
    (!text.trim().is_empty()).then_some((indent, number, text))
}

/// Premier ```` ``` ```` non échappé de la ligne.
fn find_fence(line: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = line[from..].find("```") {
        let at = from + i;
        if at > 0 && line.as_bytes()[at - 1] == b'\\' {
            from = at + 1;
            continue;
        }
        return Some(at);
    }
    None
}

struct Fence<'a> {
    lang: Option<&'a str>,
    code: &'a str,
    /// Juste après le ```` ``` ```` fermant.
    end: usize,
}

/// Bloc de code qui s'ouvre à `open` (sur ```` ``` ````), s'il est fermé et non vide.
fn code_fence(src: &str, open: usize) -> Option<Fence<'_>> {
    let body_start = open + 3;
    let close = body_start + src.get(body_start..)?.find("```")?;
    let body = &src[body_start..close];
    // Langage : un mot sur la première ligne, suivi d'un retour à la ligne.
    let (lang, code) = match body.find('\n') {
        Some(nl) if nl > 0 && body[..nl].bytes().all(|b| b.is_ascii_alphanumeric() || b"_+-.#".contains(&b)) => (Some(&body[..nl]), &body[nl + 1..]),
        _ => (None, body),
    };
    let code = code.strip_prefix('\n').unwrap_or(code);
    let code = code.strip_suffix('\n').unwrap_or(code);
    if code.trim().is_empty() {
        return None;
    }
    Some(Fence { lang, code, end: close + 3 })
}
