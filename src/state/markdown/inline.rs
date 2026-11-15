//! Mise en forme dans une ligne : emphase, code, liens, mentions, spoilers.
//!
//! Les éléments « atomiques » (code, balises `<…>`, liens, échappements)
//! sont reconnus de gauche à droite ; chaque recherche vers l'avant s'arrête
//! au premier caractère qui la rend impossible (`[`, `<`, blanc…), si bien
//! qu'aucun caractère n'est relu plus d'un nombre borné de fois.
//!
//! Les marqueurs d'emphase (`*`, `_`, `~~`, `||`) sont appariés avec une
//! pile d'ouvrants, comme dans CommonMark : un fermant cherche l'ouvrant du
//! même caractère le plus proche ; ce qui est entre les deux est enveloppé
//! et les ouvrants intermédiaires redeviennent du texte. Un plancher par
//! caractère (`bottom`) évite de reparcourir une pile où la recherche a déjà
//! échoué : le tout reste linéaire.

use super::{Doc, Mention, Resolve, Span, SpanKind, Style, TimeStyle};

/// Longueur maximale d'une balise `<…>` (mention, horodatage, adresse).
const MAX_TAG: usize = 2048;
/// Longueur maximale du texte d'un lien masqué.
const MAX_LABEL: usize = 512;
/// Imbrication maximale des styles : au-delà, le style le plus extérieur est
/// ignoré (récursion bornée à l'aplatissement, même sur une entrée hostile).
const MAX_NEST: u8 = 24;

fn depth(n: &Node) -> u8 {
    match n {
        Node::Styled(_, _, d) => *d,
        Node::Link { label, .. } => label.iter().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Bold,
    Italic,
    Underline,
    Strike,
    Spoiler,
}

#[derive(Debug)]
enum Node {
    Text(String),
    /// Marqueurs restants d'une suite (`ch` répété `n` fois) : du texte s'ils ne sont pas appariés.
    Delim { ch: u8, n: usize },
    /// Contenu stylé et sa profondeur d'imbrication (1 : aucun style dedans).
    Styled(Kind, Vec<Node>, u8),
    Code(String),
    Link { url: String, label: Vec<Node> },
    Mention(Mention, String),
    /// Secondes, texte, et pour `R` l'instant (ms) où le texte change.
    Time(i64, String, Option<i64>),
    Emoji { id: u64, name: String, animated: bool },
}

struct Opener {
    node: usize,
    ch: u8,
}

fn slot(ch: u8) -> usize {
    match ch {
        b'*' => 0,
        b'_' => 1,
        b'~' => 2,
        _ => 3,
    }
}

struct Inline<'a, 'r> {
    src: &'a str,
    r: &'r dyn Resolve,
    /// Dans le texte d'un lien masqué : ni lien ni adresse.
    in_link: bool,
    nodes: Vec<Node>,
    text: String,
    stack: Vec<Opener>,
    bottom: [usize; 4],
    /// Plus aucun fermant de code de 1 ou 2 accents graves plus loin.
    no_tick: [bool; 3],
}

/// Passages mis en forme de `text`.
pub(super) fn parse(text: &str, r: &dyn Resolve, doc: &mut Doc) -> Vec<Span> {
    let nodes = nodes(text, r, false);
    let mut out = Vec::new();
    flatten(nodes, Style::default(), None, doc, &mut out);
    out
}

fn nodes(src: &str, r: &dyn Resolve, in_link: bool) -> Vec<Node> {
    let mut p = Inline { src, r, in_link, nodes: Vec::new(), text: String::new(), stack: Vec::new(), bottom: [0; 4], no_tick: [false; 3] };
    p.run();
    p.flush_text();
    p.nodes
}

fn is_special(b: u8) -> bool {
    matches!(b, b'\\' | b'`' | b'<' | b'[' | b'h' | b'@' | b'*' | b'_' | b'~' | b'|')
}

impl Inline<'_, '_> {
    fn flush_text(&mut self) {
        if !self.text.is_empty() {
            self.nodes.push(Node::Text(std::mem::take(&mut self.text)));
        }
    }

    fn push(&mut self, n: Node) {
        self.flush_text();
        self.nodes.push(n);
    }

    fn prev_char(&self, i: usize) -> Option<char> {
        self.src[..i].chars().next_back()
    }

    fn run(&mut self) {
        let src = self.src;
        let b = src.as_bytes();
        let mut i = 0;
        while i < b.len() {
            let c = b[i];
            if !is_special(c) {
                let j = b[i..].iter().position(|x| is_special(*x)).map_or(b.len(), |k| i + k);
                self.text.push_str(&src[i..j]);
                i = j;
                continue;
            }
            let used = match c {
                b'\\' => self.escape(i),
                b'`' => self.code(i),
                b'<' => self.tag(i),
                b'[' if !self.in_link => self.link(i),
                b'h' if !self.in_link => self.url(i),
                b'@' => self.everyone(i),
                b'*' | b'_' | b'~' | b'|' => Some(self.delims(i)),
                _ => None,
            };
            match used {
                Some(end) => i = end,
                None => {
                    self.text.push(c as char);
                    i += 1;
                }
            }
        }
    }

    /// `\` suivi d'un caractère qui n'est ni lettre, ni chiffre, ni blanc : ce caractère, tel quel.
    fn escape(&mut self, i: usize) -> Option<usize> {
        let ch = self.src[i + 1..].chars().next()?;
        if ch.is_alphanumeric() || ch.is_whitespace() {
            return None;
        }
        self.text.push(ch);
        Some(i + 1 + ch.len_utf8())
    }

    /// Code en ligne : `` `x` `` ou ``` ``x`` ```.
    fn code(&mut self, i: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        let k = b[i..].iter().take_while(|x| **x == b'`').count();
        if k > 2 {
            // Suite de 3 ou plus non appariée en bloc : du texte.
            self.text.push_str(&self.src[i..i + k]);
            return Some(i + k);
        }
        let fence = &"``"[..k];
        let close = if self.no_tick[k] { None } else { self.src[i + k..].find(fence).map(|c| i + k + c) };
        let Some(close) = close.filter(|c| *c > i + k) else {
            if close.is_none() {
                self.no_tick[k] = true;
            }
            self.text.push_str(fence);
            return Some(i + k);
        };
        let mut inner = &self.src[i + k..close];
        if inner.len() > 2 && inner.starts_with(' ') && inner.ends_with(' ') && !inner.trim().is_empty() {
            inner = &inner[1..inner.len() - 1];
        }
        self.push(Node::Code(inner.to_string()));
        Some(close + k)
    }

    /// Balise `<…>` : mention, horodatage, emoji, commande, adresse sans aperçu.
    fn tag(&mut self, i: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        let command = b.get(i + 1) == Some(&b'/');
        let mut j = i + 1;
        while j < b.len() && j - i < MAX_TAG {
            match b[j] {
                b'>' => break,
                b'<' | b'\n' => return None,
                x if x.is_ascii_whitespace() && !command => return None,
                _ => j += 1,
            }
        }
        if b.get(j) != Some(&b'>') {
            return None;
        }
        let inner = &self.src[i + 1..j];
        let end = j + 1;
        let node = if inner.starts_with("https://") || inner.starts_with("http://") {
            if self.in_link || inner.len() <= "https://".len() {
                return None;
            }
            Node::Link { url: inner.to_string(), label: vec![Node::Text(inner.to_string())] }
        } else if let Some(id) = inner.strip_prefix("@&").and_then(id) {
            let (name, color) = self.r.role(id);
            Node::Mention(Mention::Role { id, color }, format!("@{name}"))
        } else if let Some(id) = inner.strip_prefix("@!").or_else(|| inner.strip_prefix('@')).and_then(id) {
            Node::Mention(Mention::User(id), format!("@{}", self.r.user(id)))
        } else if let Some(id) = inner.strip_prefix('#').and_then(id) {
            Node::Mention(Mention::Channel(id), format!("#{}", self.r.channel(id)))
        } else if let Some(rest) = inner.strip_prefix("t:") {
            let (secs, style) = match rest.split_once(':') {
                Some((s, st)) => (s, TimeStyle::from_letter(st)?),
                None => (rest, TimeStyle::ShortDateTime),
            };
            if secs.is_empty() || secs.len() > 20 {
                return None;
            }
            let secs: i64 = secs.parse().ok()?;
            let refresh = if style == TimeStyle::Relative { self.r.refresh_at(secs) } else { None };
            Node::Time(secs, self.r.time(secs, style)?, refresh)
        } else if let Some((animated, rest)) = inner.strip_prefix("a:").map(|r| (true, r)).or_else(|| inner.strip_prefix(':').map(|r| (false, r))) {
            let (name, id_text) = rest.split_once(':')?;
            if name.is_empty() || name.len() > 32 || !name.bytes().all(|x| x.is_ascii_alphanumeric() || x == b'_') {
                return None;
            }
            Node::Emoji { id: id(id_text)?, name: name.to_string(), animated }
        } else {
            // Commande `</nom:id>` ; toute autre balise reste du texte.
            let rest = inner.strip_prefix('/')?;
            let (name, id_text) = rest.rsplit_once(':')?;
            if name.trim().is_empty() || name.len() > 100 {
                return None;
            }
            Node::Mention(Mention::Command(id(id_text)?), format!("/{name}"))
        };
        self.push(node);
        Some(end)
    }

    /// Lien masqué `[texte](https://…)` ou `[texte](<https://…>)`.
    fn link(&mut self, i: usize) -> Option<usize> {
        let b = self.src.as_bytes();
        let mut j = i + 1;
        while j < b.len() && j - i <= MAX_LABEL {
            match b[j] {
                b']' => break,
                b'[' | b'\n' => return None,
                _ => j += 1,
            }
        }
        if b.get(j) != Some(&b']') || j == i + 1 || b.get(j + 1) != Some(&b'(') {
            return None;
        }
        let label = &self.src[i + 1..j];
        let mut k = j + 2;
        let bracketed = b.get(k) == Some(&b'<');
        if bracketed {
            k += 1;
        }
        let url_start = k;
        let mut depth = 0usize;
        loop {
            let x = *b.get(k)?;
            match x {
                b'(' => depth += 1,
                b')' if depth == 0 && !bracketed => break,
                b')' => depth = depth.saturating_sub(1),
                b'>' if bracketed => break,
                b'[' | b']' | b'<' | b'>' => return None,
                x if x.is_ascii_whitespace() => return None,
                _ => {}
            }
            k += 1;
        }
        let url = &self.src[url_start..k];
        let end = if bracketed {
            if b.get(k + 1) != Some(&b')') {
                return None;
            }
            k + 2
        } else {
            k + 1
        };
        if !(url.starts_with("https://") || url.starts_with("http://")) || url.len() <= "https://".len() || label.trim().is_empty() {
            return None;
        }
        let label = nodes(label, self.r, true);
        self.push(Node::Link { url: url.to_string(), label });
        Some(end)
    }

    /// Adresse nue `http(s)://…`, au début d'un mot.
    fn url(&mut self, i: usize) -> Option<usize> {
        let rest = &self.src[i..];
        let scheme = if rest.starts_with("https://") {
            8
        } else if rest.starts_with("http://") {
            7
        } else {
            return None;
        };
        if self.prev_char(i).is_some_and(char::is_alphanumeric) {
            return None;
        }
        let b = rest.as_bytes();
        let mut end = scheme;
        while end < b.len() {
            let x = b[end];
            if x.is_ascii_whitespace() || x == b'<' || (x == b'|' && b.get(end + 1) == Some(&b'|')) {
                break;
            }
            end += 1;
        }
        // Ponctuation finale : hors de l'adresse ; une parenthèse seulement si elle n'est pas appariée.
        while end > scheme {
            let last = b[end - 1];
            let trim = match last {
                b'.' | b',' | b':' | b';' | b'"' | b'\'' | b'!' | b'?' | b'*' | b'_' | b'~' | b']' => true,
                b')' => {
                    let u = &rest[..end];
                    u.matches('(').count() < u.matches(')').count()
                }
                _ => false,
            };
            if !trim {
                break;
            }
            end -= 1;
        }
        if end == scheme {
            return None;
        }
        let url = &rest[..end];
        self.push(Node::Link { url: url.to_string(), label: vec![Node::Text(url.to_string())] });
        Some(i + end)
    }

    fn everyone(&mut self, i: usize) -> Option<usize> {
        let rest = &self.src[i..];
        let (m, word) = if rest.starts_with("@everyone") {
            (Mention::Everyone, "@everyone")
        } else if rest.starts_with("@here") {
            (Mention::Here, "@here")
        } else {
            return None;
        };
        self.push(Node::Mention(m, word.to_string()));
        Some(i + word.len())
    }

    /// Suite de `*`, `_`, `~` ou `|` : ferme ce qu'elle peut, puis ouvre avec le reste.
    fn delims(&mut self, i: usize) -> usize {
        let b = self.src.as_bytes();
        let ch = b[i];
        let n = b[i..].iter().take_while(|x| **x == ch).count();
        let end = i + n;
        let pair_only = matches!(ch, b'~' | b'|');
        if pair_only && n < 2 {
            self.text.push_str(&self.src[i..end]);
            return end;
        }
        let prev = self.prev_char(i);
        let next = self.src[end..].chars().next();
        let ws = |c: Option<char>| c.is_none_or(char::is_whitespace);
        let word = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        let (can_open, can_close) = match ch {
            b'_' if n == 1 => (!ws(next) && !word(prev), !ws(prev) && !word(next)),
            b'*' | b'_' => (!ws(next), !ws(prev)),
            // Spoilers et barré : une phrase entière, espaces compris.
            _ => (next.is_some(), prev.is_some()),
        };
        let mut left = n;
        if can_close {
            left = self.close(ch, left);
        }
        if left > 0 {
            self.push(Node::Delim { ch, n: left });
            if can_open {
                self.stack.push(Opener { node: self.nodes.len() - 1, ch });
            }
        }
        end
    }

    /// Apparie un fermant de `n` marqueurs `ch` ; renvoie ce qu'il en reste.
    fn close(&mut self, ch: u8, mut n: usize) -> usize {
        let need = if matches!(ch, b'~' | b'|') { 2 } else { 1 };
        while n >= need {
            let s = slot(ch);
            let floor = self.bottom[s];
            let found = (floor..self.stack.len()).rev().find(|k| {
                let o = &self.stack[*k];
                o.ch == ch && matches!(self.nodes.get(o.node), Some(Node::Delim { n, .. }) if *n >= need)
            });
            let Some(k) = found else {
                self.bottom[s] = self.stack.len();
                break;
            };
            self.flush_text();
            let at = self.stack[k].node;
            let Some(Node::Delim { n: open_n, .. }) = self.nodes.get_mut(at) else { break };
            let used = if *open_n >= 2 && n >= 2 { 2 } else { 1 };
            *open_n -= used;
            let open_left = *open_n;
            n -= used;
            let kind = match (ch, used) {
                (b'*', 2) => Kind::Bold,
                (b'_', 2) => Kind::Underline,
                (b'~', _) => Kind::Strike,
                (b'|', _) => Kind::Spoiler,
                _ => Kind::Italic,
            };
            let inner = self.nodes.split_off(at + 1);
            let d = inner.iter().map(depth).max().unwrap_or(0).saturating_add(1);
            if d > MAX_NEST {
                self.nodes.extend(inner);
            } else {
                self.nodes.push(Node::Styled(kind, inner, d));
            }
            self.stack.truncate(k + 1);
            if open_left == 0 {
                self.stack.pop();
            }
            let len = self.stack.len();
            for b in &mut self.bottom {
                *b = (*b).min(len);
            }
        }
        n
    }
}

/// Identifiant (snowflake) en chiffres.
fn id(s: &str) -> Option<u64> {
    if s.is_empty() || s.len() > 20 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn flatten(nodes: Vec<Node>, style: Style, link: Option<u32>, doc: &mut Doc, out: &mut Vec<Span>) {
    let text_kind = || link.map_or(SpanKind::Text, SpanKind::Link);
    for node in nodes {
        match node {
            Node::Text(t) => push(out, t, style, text_kind()),
            Node::Delim { ch, n } => {
                if n > 0 {
                    push(out, (ch as char).to_string().repeat(n), style, text_kind());
                }
            }
            Node::Styled(kind, inner, _) => {
                let mut s = style;
                match kind {
                    Kind::Bold => s.bold = true,
                    Kind::Italic => s.italic = true,
                    Kind::Underline => s.underline = true,
                    Kind::Strike => s.strike = true,
                    Kind::Spoiler => {
                        if s.spoiler.is_none() {
                            s.spoiler = Some(doc.spoilers);
                            doc.spoilers = doc.spoilers.saturating_add(1);
                        }
                    }
                }
                flatten(inner, s, link, doc, out);
            }
            Node::Code(t) => push(out, t, Style { code: true, ..style }, text_kind()),
            Node::Link { url, label } => {
                let idx = doc.links.len() as u32;
                doc.links.push(url);
                flatten(label, style, Some(idx), doc, out);
            }
            Node::Mention(m, t) => push(out, t, style, SpanKind::Mention(m)),
            Node::Time(secs, t, refresh) => {
                if let Some(at) = refresh {
                    doc.refresh_at_ms = Some(doc.refresh_at_ms.map_or(at, |d| d.min(at)));
                }
                push(out, t, style, SpanKind::Time(secs))
            }
            Node::Emoji { id, name, animated } => {
                // Point d'extension des emojis personnalisés : le texte `:nom:` tient lieu d'image.
                push(out, format!(":{name}:"), style, SpanKind::Emoji { id, name, animated });
            }
        }
    }
}

/// Ajoute un passage, collé au précédent s'il a le même style (texte ou même lien).
fn push(out: &mut Vec<Span>, text: String, style: Style, kind: SpanKind) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = out.last_mut()
        && last.style == style
        && last.kind == kind
        && matches!(kind, SpanKind::Text | SpanKind::Link(_))
    {
        last.text.push_str(&text);
        return;
    }
    out.push(Span { text, style, kind });
}
