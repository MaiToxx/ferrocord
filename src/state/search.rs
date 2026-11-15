//! Recherche de messages (lot T12), sans egui ni réseau : texte tapé et
//! filtres (`from:`, `in:`, `has:`…), requête envoyée, page de résultats,
//! surlignage des mots cherchés.
//!
//! Forme des paramètres : docs.discord.food, « Messages › Search Messages »
//! (`GET /guilds/{id}/messages/search`, `GET /channels/{id}/messages/search`),
//! vérifiée le 13 novembre 2026. Une seule recherche est gardée à la fois :
//! sa page de résultats (25 messages au plus) est toute la mémoire du module.

use super::event::Event;
use super::markdown::{Doc, Span, SpanKind};
use super::model::{Message, DISCORD_EPOCH};
use super::notice::Problem;
use crate::i18n::Lang;
use chrono::{NaiveDate, TimeZone, Utc};

/// Résultats par page (documenté : 1 à 25, 25 par défaut).
pub const PAGE: usize = 25;
/// Décalage maximal accepté par Discord (documenté : 9975).
pub const MAX_OFFSET: usize = 9975;
/// Longueur maximale de `content` (documentée : 1024 caractères).
pub const MAX_CONTENT: usize = 1024;
/// Auteurs ou mentions par requête, au plus (documenté : 100).
pub const MAX_IDS: usize = 100;
/// Mots surlignés au plus.
const MAX_TERMS: usize = 16;

/// Où chercher : un serveur entier, ou un salon (MP, groupe).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Scope {
    Guild(u64),
    Channel(u64),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Sort {
    /// Plus récents d'abord (défaut de Discord).
    #[default]
    Newest,
    Oldest,
    /// Pertinence : `sort_order` n'est pas respecté par Discord.
    Relevance,
}

impl Sort {
    pub const ALL: [Sort; 3] = [Sort::Newest, Sort::Oldest, Sort::Relevance];

    /// (`sort_by`, `sort_order`).
    pub fn params(self) -> (&'static str, &'static str) {
        match self {
            Sort::Newest => ("timestamp", "desc"),
            Sort::Oldest => ("timestamp", "asc"),
            Sort::Relevance => ("relevance", "desc"),
        }
    }
}

/// Valeurs de `has` retenues (documentées : image, sound, video, file,
/// sticker, embed, link, poll, snapshot ; `snapshot` n'est pas proposé).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Has {
    Link,
    Embed,
    File,
    Image,
    Video,
    Sound,
    Sticker,
    Poll,
}

impl Has {
    pub const ALL: [Has; 8] = [Has::Link, Has::Embed, Has::File, Has::Image, Has::Video, Has::Sound, Has::Sticker, Has::Poll];

    /// Valeur envoyée à Discord.
    pub fn code(self) -> &'static str {
        match self {
            Has::Link => "link",
            Has::Embed => "embed",
            Has::File => "file",
            Has::Image => "image",
            Has::Video => "video",
            Has::Sound => "sound",
            Has::Sticker => "sticker",
            Has::Poll => "poll",
        }
    }

    /// Mot tapé après `has:`, en français ou en anglais.
    pub fn word(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (h, Lang::En) => h.code(),
            (Has::Link, _) => "lien",
            (Has::Embed, _) => "integration",
            (Has::File, _) => "fichier",
            (Has::Image, _) => "image",
            (Has::Video, _) => "video",
            (Has::Sound, _) => "son",
            (Has::Sticker, _) => "autocollant",
            (Has::Poll, _) => "sondage",
        }
    }

    fn from_word(w: &str) -> Option<Has> {
        let w = fold(w);
        Has::ALL.into_iter().find(|h| Lang::ALL.iter().any(|l| fold(h.word(*l)) == w))
    }
}

/// Valeurs de `author_type` (documentées : user, bot, webhook).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthorType {
    User,
    Bot,
    Webhook,
}

impl AuthorType {
    pub const ALL: [AuthorType; 3] = [AuthorType::User, AuthorType::Bot, AuthorType::Webhook];

    pub fn code(self) -> &'static str {
        match self {
            AuthorType::User => "user",
            AuthorType::Bot => "bot",
            AuthorType::Webhook => "webhook",
        }
    }

    pub fn word(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (AuthorType::User, Lang::Fr) => "utilisateur",
            (a, _) => a.code(),
        }
    }

    fn from_word(w: &str) -> Option<AuthorType> {
        let w = fold(w);
        AuthorType::ALL.into_iter().find(|a| Lang::ALL.iter().any(|l| fold(a.word(*l)) == w))
    }
}

/// Filtres du champ de recherche, dans l'ordre où l'aide les propose.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Filter {
    From,
    Mentions,
    Has,
    In,
    Before,
    After,
    During,
    Pinned,
    AuthorType,
}

impl Filter {
    pub const ALL: [Filter; 9] =
        [Filter::From, Filter::Mentions, Filter::Has, Filter::In, Filter::Before, Filter::After, Filter::During, Filter::Pinned, Filter::AuthorType];

    /// Mot-clé tapé avant `:`. Les deux langues sont toujours comprises ;
    /// l'aide et l'autocomplétion proposent celui de la langue affichée.
    pub fn key(self, lang: Lang) -> &'static str {
        match (self, lang) {
            (Filter::From, Lang::En) => "from",
            (Filter::Mentions, Lang::En) => "mentions",
            (Filter::Has, Lang::En) => "has",
            (Filter::In, Lang::En) => "in",
            (Filter::Before, Lang::En) => "before",
            (Filter::After, Lang::En) => "after",
            (Filter::During, Lang::En) => "during",
            (Filter::Pinned, Lang::En) => "pinned",
            (Filter::AuthorType, Lang::En) => "authorType",
            (Filter::From, _) => "de",
            (Filter::Mentions, _) => "mentionne",
            (Filter::Has, _) => "contient",
            (Filter::In, _) => "dans",
            (Filter::Before, _) => "avant",
            (Filter::After, _) => "apres",
            (Filter::During, _) => "pendant",
            (Filter::Pinned, _) => "epingle",
            (Filter::AuthorType, _) => "typeAuteur",
        }
    }

    fn from_key(k: &str) -> Option<Filter> {
        let k = fold(k);
        if k == "author_type" {
            return Some(Filter::AuthorType);
        }
        Filter::ALL.into_iter().find(|f| Lang::ALL.iter().any(|l| fold(f.key(*l)) == k))
    }

    /// Le filtre attend une date (`AAAA-MM-JJ`).
    pub fn is_date(self) -> bool {
        matches!(self, Filter::Before | Filter::After | Filter::During)
    }
}

/// Minuscules sans accents (comparaison des mots-clés et des valeurs).
pub(crate) fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Requête envoyée à Discord.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Query {
    pub content: String,
    pub authors: Vec<u64>,
    pub mentions: Vec<u64>,
    /// Salons du serveur (sans effet pour un MP : non envoyé).
    pub channels: Vec<u64>,
    pub has: Vec<Has>,
    pub author_types: Vec<AuthorType>,
    /// Messages postérieurs à cet identifiant (exclu).
    pub min_id: Option<u64>,
    /// Messages antérieurs à cet identifiant (exclu).
    pub max_id: Option<u64>,
    pub pinned: Option<bool>,
    pub sort: Sort,
}

/// Mot du texte tapé : un filtre (`clé:valeur`) ou un mot cherché.
/// `start`..`end` : position en octets dans le texte.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Token {
    pub filter: Option<Filter>,
    /// Valeur (guillemets retirés) ou mot.
    pub value: String,
    pub start: usize,
    pub end: usize,
}

/// Découpe le texte tapé. Un nom à espaces s'écrit entre guillemets
/// (`from:"Jean Dupont"`) ; un guillemet jamais fermé va jusqu'au bout.
pub fn tokens(text: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut it = text.char_indices().peekable();
    while let Some(&(start, c)) = it.peek() {
        if c.is_whitespace() {
            it.next();
            continue;
        }
        let mut raw = String::new();
        let mut quoted = false;
        let mut end = start;
        while let Some(&(i, c)) = it.peek() {
            if c == '"' {
                quoted = !quoted;
            } else if c.is_whitespace() && !quoted {
                break;
            }
            raw.push(c);
            end = i + c.len_utf8();
            it.next();
        }
        let (filter, value) = match raw.split_once(':') {
            Some((k, v)) if !k.contains('"') => match Filter::from_key(k) {
                Some(f) => (Some(f), v.trim_matches('"').to_string()),
                None => (None, raw.clone()),
            },
            _ => (None, raw.clone()),
        };
        out.push(Token { filter, value, start, end });
    }
    out
}

/// Résultat de la recherche d'un nom.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lookup {
    One(u64),
    Unknown,
    /// Plusieurs personnes ou salons de ce nom : à choisir dans la liste.
    Ambiguous,
}

/// Noms connus : membres et salons, et ceux choisis dans l'autocomplétion
/// (qui priment : deux personnes de même nom ne sont jamais devinées).
#[derive(Clone, Debug, Default)]
pub struct Names {
    pub people: Vec<(u64, String)>,
    pub channels: Vec<(u64, String)>,
    pub picked_people: Vec<(String, u64)>,
    pub picked_channels: Vec<(String, u64)>,
    /// Recherche dans un MP ou un groupe : `in:` n'y a pas de sens.
    pub private: bool,
}

impl Names {
    fn find(list: &[(u64, String)], picked: &[(String, u64)], name: &str) -> Lookup {
        if let Ok(id) = name.parse::<u64>() {
            return Lookup::One(id);
        }
        let name = name.trim_start_matches(['@', '#']);
        if let Some((_, id)) = picked.iter().rev().find(|(n, _)| n == name) {
            return Lookup::One(*id);
        }
        let want = fold(name);
        let mut found = list.iter().filter(|(_, n)| fold(n) == want).map(|(id, _)| *id);
        match (found.next(), found.next()) {
            (None, _) => Lookup::Unknown,
            (Some(id), None) => Lookup::One(id),
            (Some(a), Some(b)) if a == b => Lookup::One(a),
            _ => Lookup::Ambiguous,
        }
    }

    pub fn user(&self, name: &str) -> Lookup {
        Names::find(&self.people, &self.picked_people, name)
    }

    pub fn channel(&self, name: &str) -> Lookup {
        Names::find(&self.channels, &self.picked_channels, name)
    }
}

/// Pourquoi le texte tapé ne peut pas partir.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    /// Ni mot ni filtre.
    Empty,
    TooLong,
    UnknownUser(String),
    AmbiguousUser(String),
    UnknownChannel(String),
    AmbiguousChannel(String),
    /// `in:` dans un MP ou un groupe.
    NoChannels,
    BadDate(String),
    BadValue { filter: Filter, value: String },
}

/// Requête et mots à surligner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub query: Query,
    pub terms: Vec<String>,
}

/// Snowflake du premier instant `ms` (ms Unix) ; 0 avant l'époque de Discord.
pub(crate) fn snowflake_at(ms: i64) -> u64 {
    u64::try_from(ms).map_or(0, |ms| ms.saturating_sub(DISCORD_EPOCH)) << 22
}

/// Date tapée : `AAAA-MM-JJ` ou `JJ/MM/AAAA`.
pub(crate) fn date(v: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(v, "%Y-%m-%d").or_else(|_| NaiveDate::parse_from_str(v, "%d/%m/%Y")).ok()
}

/// Début (ms Unix) du jour `d` à l'heure locale, décalée de `utc_offset_s`
/// secondes sur UTC (donné par l'appelant : le module ne lit pas l'horloge).
pub(crate) fn day_start(d: NaiveDate, utc_offset_s: i32) -> i64 {
    let midnight = d.and_hms_opt(0, 0, 0).map_or(0, |t| Utc.from_utc_datetime(&t).timestamp_millis());
    midnight - i64::from(utc_offset_s) * 1000
}

/// Analyse le texte tapé. `utc_offset_s` : décalage de l'heure locale (les
/// dates sont des jours locaux, comme dans Discord).
pub fn parse(text: &str, names: &Names, utc_offset_s: i32, sort: Sort) -> Result<Parsed, ParseError> {
    let mut q = Query { sort, ..Query::default() };
    let mut words: Vec<&str> = Vec::new();
    let toks = tokens(text);
    for t in &toks {
        let Some(f) = t.filter else {
            words.push(&text[t.start..t.end]);
            continue;
        };
        let v = t.value.as_str();
        let bad = || ParseError::BadValue { filter: f, value: v.to_string() };
        match f {
            Filter::From | Filter::Mentions => {
                let id = match names.user(v) {
                    Lookup::One(id) => id,
                    Lookup::Unknown => return Err(ParseError::UnknownUser(v.to_string())),
                    Lookup::Ambiguous => return Err(ParseError::AmbiguousUser(v.to_string())),
                };
                let list = if f == Filter::From { &mut q.authors } else { &mut q.mentions };
                if !list.contains(&id) && list.len() < MAX_IDS {
                    list.push(id);
                }
            }
            Filter::In => {
                if names.private {
                    return Err(ParseError::NoChannels);
                }
                match names.channel(v) {
                    Lookup::One(id) if !q.channels.contains(&id) => q.channels.push(id),
                    Lookup::One(_) => {}
                    Lookup::Unknown => return Err(ParseError::UnknownChannel(v.to_string())),
                    Lookup::Ambiguous => return Err(ParseError::AmbiguousChannel(v.to_string())),
                }
            }
            Filter::Has => {
                let h = Has::from_word(v).ok_or_else(bad)?;
                if !q.has.contains(&h) {
                    q.has.push(h);
                }
            }
            Filter::AuthorType => {
                let a = AuthorType::from_word(v).ok_or_else(bad)?;
                if !q.author_types.contains(&a) {
                    q.author_types.push(a);
                }
            }
            Filter::Pinned => {
                q.pinned = Some(match fold(v).as_str() {
                    "true" | "yes" | "oui" | "vrai" => true,
                    "false" | "no" | "non" | "faux" => false,
                    _ => return Err(bad()),
                });
            }
            Filter::Before | Filter::After | Filter::During => {
                let d = date(v).ok_or_else(|| ParseError::BadDate(v.to_string()))?;
                let start = day_start(d, utc_offset_s);
                let next = d.succ_opt().map_or(i64::MAX, |n| day_start(n, utc_offset_s));
                // `min_id` et `max_id` sont exclus : « après » commence au
                // lendemain, « pendant » couvre le jour entier.
                let (lo, hi) = match f {
                    Filter::Before => (None, Some(snowflake_at(start))),
                    Filter::After => (Some(snowflake_at(next).saturating_sub(1)), None),
                    _ => (Some(snowflake_at(start).saturating_sub(1)), Some(snowflake_at(next))),
                };
                if let Some(lo) = lo {
                    q.min_id = Some(q.min_id.map_or(lo, |m| m.max(lo)));
                }
                if let Some(hi) = hi {
                    q.max_id = Some(q.max_id.map_or(hi, |m| m.min(hi)));
                }
            }
        }
    }
    q.content = words.join(" ");
    if q.content.chars().count() > MAX_CONTENT {
        return Err(ParseError::TooLong);
    }
    let filtered = !(q.authors.is_empty()
        && q.mentions.is_empty()
        && q.channels.is_empty()
        && q.has.is_empty()
        && q.author_types.is_empty()
        && q.pinned.is_none()
        && q.min_id.is_none()
        && q.max_id.is_none());
    if q.content.is_empty() && !filtered {
        return Err(ParseError::Empty);
    }
    let terms = terms(&q.content);
    Ok(Parsed { query: q, terms })
}

/// Mots cherchés, en minuscules, sans ponctuation ni doublon.
fn terms(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for w in content.split(|c: char| !c.is_alphanumeric()) {
        let w = w.to_lowercase();
        if !w.is_empty() && !out.contains(&w) && out.len() < MAX_TERMS {
            out.push(w);
        }
    }
    out
}

/// Surligne les mots cherchés dans le texte mis en forme : les passages
/// de texte sont coupés autour de chaque occurrence (début de mot, casse
/// ignorée), le reste du document est intact.
pub fn mark_terms(doc: &mut Doc, terms: &[String]) {
    if terms.is_empty() {
        return;
    }
    for b in &mut doc.blocks {
        let spans = std::mem::take(&mut b.spans);
        for s in spans {
            if s.kind != SpanKind::Text {
                b.spans.push(s);
                continue;
            }
            let mut rest = s.text.as_str();
            for (a, z) in matches(rest, terms) {
                // Positions relatives au passage d'origine : `rest` avance.
                let consumed = s.text.len() - rest.len();
                let (before, from) = rest.split_at(a - consumed);
                let (hit, after) = from.split_at(z - a);
                if !before.is_empty() {
                    b.spans.push(Span { text: before.to_string(), ..s.clone() });
                }
                let mut style = s.style;
                style.mark = true;
                b.spans.push(Span { text: hit.to_string(), style, kind: SpanKind::Text });
                rest = after;
            }
            if !rest.is_empty() {
                b.spans.push(Span { text: rest.to_string(), ..s });
            }
        }
    }
}

/// Occurrences (octets, sans chevauchement, dans l'ordre) des mots dans `text`,
/// chacune au début d'un mot.
fn matches(text: &str, terms: &[String]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    let mut skip_to = 0;
    for (i, c) in text.char_indices() {
        let starts_word = c.is_alphanumeric() && prev.is_none_or(|p| !p.is_alphanumeric());
        prev = Some(c);
        if i < skip_to || !starts_word {
            continue;
        }
        let rest = &text[i..];
        // Comparaison caractère par caractère : la longueur en octets des
        // minuscules peut différer de celle du texte.
        let hit = terms.iter().filter_map(|t| prefix_len(rest, t)).max();
        if let Some(n) = hit {
            out.push((i, i + n));
            skip_to = i + n;
        }
    }
    out
}

/// Longueur (octets de `text`) du préfixe de `text` égal à `term` à la casse près.
fn prefix_len(text: &str, term: &str) -> Option<usize> {
    let mut want = term.chars().peekable();
    let mut len = 0;
    for c in text.chars() {
        if want.peek().is_none() {
            break;
        }
        // Une lettre dont la minuscule fait deux caractères (« İ ») compte
        // en entier dès que son début correspond.
        for lc in c.to_lowercase() {
            match want.next() {
                Some(w) if w == lc => {}
                Some(_) => return None,
                None => break,
            }
        }
        len += c.len_utf8();
    }
    (want.peek().is_none() && len > 0).then_some(len)
}

/// Un résultat : le message (texte déjà rendu par le magasin) et, pour un
/// fil inconnu du magasin, le nom donné par la réponse.
#[derive(Clone, Debug)]
pub struct Hit {
    pub message: Message,
    pub thread_name: Option<String>,
}

/// Où en est la page affichée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    Loading,
    /// Discord indexe encore ce serveur ou ce salon : un seul nouvel essai,
    /// après le délai qu'il indique.
    Indexing,
    Done,
    /// Toujours pas indexé après le nouvel essai.
    NotIndexed,
    Failed(Problem),
}

/// Recherche affichée (panneau de droite).
#[derive(Clone, Debug)]
pub struct Active {
    pub scope: Scope,
    /// Texte tel que tapé (repris dans le champ).
    pub text: String,
    pub query: Query,
    pub terms: Vec<String>,
    /// Page affichée, à partir de 0.
    pub page: usize,
    /// Nombre de résultats annoncé par Discord (inconnu avant la première page).
    pub total: Option<u64>,
    pub hits: Vec<Hit>,
    pub phase: Phase,
    /// Numéro de la demande en cours : une réponse d'une autre est ignorée.
    pub seq: u64,
    /// Résultats de la recherche locale (plugin, lot E1) : rien n'est demandé à Discord.
    pub local: bool,
}

impl Active {
    /// Dernière page atteignable (bornée par `MAX_OFFSET`).
    pub fn last_page(&self) -> usize {
        let total = usize::try_from(self.total.unwrap_or(0)).unwrap_or(usize::MAX);
        let pages = total.div_ceil(PAGE).max(1);
        (pages - 1).min(MAX_OFFSET / PAGE)
    }
}

/// Interface -> réseau.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchCmd {
    Query { seq: u64, scope: Scope, query: Query, offset: usize },
    /// Historique autour d'un message (saut vers un résultat ou un lien),
    /// `GET /channels/{id}/messages?around=` (documenté).
    Around { channel_id: u64, around: u64 },
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum SearchEvent {
    /// `threads` : (identifiant, nom) des fils des résultats.
    Results { seq: u64, total: u64, messages: Vec<Message>, threads: Vec<(u64, String)> },
    /// Index en cours : nouvel essai dans `retry_ms`.
    Indexing { seq: u64, retry_ms: u64 },
    /// Toujours pas indexé après le nouvel essai.
    NotIndexed { seq: u64 },
    Failed { seq: u64, problem: Problem },
    Around { channel_id: u64, around: u64, messages: Vec<Message> },
    AroundFailed { channel_id: u64, around: u64, problem: Problem },
}

impl From<SearchEvent> for Event {
    fn from(e: SearchEvent) -> Event {
        Event::Search(e)
    }
}

/// Onglets du panneau de résultats (F16) : tout, ou seulement les messages
/// qui portent une image, une vidéo, un lien ou un fichier. Chaque onglet
/// ajoute un filtre `has` à la requête tapée.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Messages,
    Images,
    Videos,
    Links,
    Files,
}

impl Tab {
    pub const ALL: [Tab; 5] = [Tab::Messages, Tab::Images, Tab::Videos, Tab::Links, Tab::Files];

    fn has(self) -> Option<Has> {
        match self {
            Tab::Messages => None,
            Tab::Images => Some(Has::Image),
            Tab::Videos => Some(Has::Video),
            Tab::Links => Some(Has::Link),
            Tab::Files => Some(Has::File),
        }
    }
}

/// Recherche en cours et numéros des demandes.
#[derive(Debug, Default)]
pub struct Search {
    pub active: Option<Active>,
    seq: u64,
    /// Onglet choisi, et les filtres `has` de la requête telle que tapée.
    tab: Tab,
    typed_has: Vec<Has>,
}

impl Search {
    pub fn clear(&mut self) {
        self.active = None;
    }

    fn next_seq(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Nouvelle recherche, page 0.
    pub fn start(&mut self, scope: Scope, text: String, parsed: Parsed) -> SearchCmd {
        let seq = self.next_seq();
        let query = parsed.query.clone();
        self.tab = Tab::Messages;
        self.typed_has.clone_from(&query.has);
        self.active = Some(Active { scope, text, query: parsed.query, terms: parsed.terms, page: 0, total: None, hits: Vec::new(), phase: Phase::Loading, seq, local: false });
        SearchCmd::Query { seq, scope, query, offset: 0 }
    }

    /// Page `page` de la recherche affichée ; `None` hors bornes ou sans recherche.
    pub fn goto(&mut self, page: usize) -> Option<SearchCmd> {
        let seq = self.seq + 1;
        let a = self.active.as_mut().filter(|a| !a.local)?;
        if page > a.last_page() || (page == a.page && a.phase == Phase::Done) {
            return None;
        }
        a.page = page;
        a.phase = Phase::Loading;
        a.seq = seq;
        self.seq = seq;
        Some(SearchCmd::Query { seq, scope: a.scope, query: a.query.clone(), offset: page * PAGE })
    }

    /// Autre tri : la recherche repart de la première page.
    pub fn set_sort(&mut self, sort: Sort) -> Option<SearchCmd> {
        let seq = self.seq + 1;
        let a = self.active.as_mut().filter(|a| !a.local)?;
        if a.query.sort == sort {
            return None;
        }
        a.query.sort = sort;
        a.page = 0;
        a.total = None;
        a.phase = Phase::Loading;
        a.seq = seq;
        self.seq = seq;
        Some(SearchCmd::Query { seq, scope: a.scope, query: a.query.clone(), offset: 0 })
    }

    /// Onglet affiché.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// Autre onglet (F16) : la même recherche, avec le filtre `has` de
    /// l'onglet en plus de ceux tapés ; elle repart de la première page.
    pub fn set_tab(&mut self, tab: Tab) -> Option<SearchCmd> {
        let seq = self.seq + 1;
        if self.tab == tab {
            return None;
        }
        let a = self.active.as_mut().filter(|a| !a.local)?;
        let mut has = self.typed_has.clone();
        if let Some(h) = tab.has().filter(|h| !has.contains(h)) {
            has.push(h);
        }
        a.query.has = has;
        a.page = 0;
        a.total = None;
        a.phase = Phase::Loading;
        a.seq = seq;
        self.seq = seq;
        self.tab = tab;
        Some(SearchCmd::Query { seq, scope: a.scope, query: a.query.clone(), offset: 0 })
    }

    /// Résultats trouvés sur place (recherche locale) : affichés tels quels,
    /// aucune demande ne part ; une réponse de Discord encore en route est ignorée.
    pub fn show_local(&mut self, scope: Scope, text: String, parsed: Parsed, page: usize, total: u64, hits: Vec<Hit>) {
        let seq = self.next_seq();
        self.tab = Tab::Messages;
        self.active = Some(Active { scope, text, query: parsed.query, terms: parsed.terms, page, total: Some(total), hits, phase: Phase::Done, seq, local: true });
    }

    /// Recherche affichée dont la demande `seq` est la dernière.
    pub(super) fn current(&mut self, seq: u64) -> Option<&mut Active> {
        self.active.as_mut().filter(|a| a.seq == seq)
    }
}

/// Lien d'un message dans le client web (`https://discord.com/channels/…`) :
/// (serveur, salon, message), serveur `None` pour `@me`.
pub fn message_link(url: &str) -> Option<(Option<u64>, u64, u64)> {
    let rest = ["https://discord.com/channels/", "https://ptb.discord.com/channels/", "https://canary.discord.com/channels/", "https://discordapp.com/channels/"]
        .iter()
        .find_map(|p| url.strip_prefix(p))?;
    let mut parts = rest.trim_end_matches('/').split('/');
    let guild = match parts.next()? {
        "@me" => None,
        g => Some(g.parse().ok()?),
    };
    let channel = parts.next()?.parse().ok()?;
    let message = parts.next()?.parse().ok()?;
    parts.next().is_none().then_some((guild, channel, message))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::markdown::{parse as md, Resolve, TimeStyle};

    fn names() -> Names {
        Names {
            people: vec![(10, "Alice".into()), (11, "Bob".into()), (12, "Jean Dupont".into()), (13, "Bob".into())],
            channels: vec![(20, "général".into()), (21, "hors-sujet".into())],
            ..Names::default()
        }
    }

    fn ok(text: &str) -> Parsed {
        parse(text, &names(), 0, Sort::Newest).unwrap_or_else(|e| panic!("{text} : {e:?}"))
    }

    #[test]
    fn filters_in_both_languages() {
        let p = ok("from:alice in:général has:image has:lien rdv demain");
        assert_eq!(p.query.authors, vec![10]);
        assert_eq!(p.query.channels, vec![20]);
        assert_eq!(p.query.has, vec![Has::Image, Has::Link]);
        assert_eq!(p.query.content, "rdv demain");
        assert_eq!(p.terms, vec!["rdv", "demain"]);
        let p = ok(r#"de:"Jean Dupont" mentionne:Alice contient:fichier epingle:oui typeAuteur:bot authorType:webhook"#);
        assert_eq!((p.query.authors, p.query.mentions), (vec![12], vec![10]));
        assert_eq!((p.query.has, p.query.pinned), (vec![Has::File], Some(true)));
        assert_eq!(p.query.author_types, vec![AuthorType::Bot, AuthorType::Webhook]);
        assert!(p.query.content.is_empty());
        // Identifiant brut, accents et casse ignorés.
        assert_eq!(ok("FROM:1234 dans:GENERAL").query.authors, vec![1234]);
        assert_eq!(ok("dans:general").query.channels, vec![20]);
    }

    #[test]
    fn names_are_never_guessed() {
        assert_eq!(parse("from:bob", &names(), 0, Sort::Newest), Err(ParseError::AmbiguousUser("bob".into())));
        let mut n = names();
        n.picked_people.push(("Bob".into(), 13));
        assert_eq!(parse("from:Bob x", &n, 0, Sort::Newest).map(|p| p.query.authors), Ok(vec![13]));
        assert_eq!(parse("from:zoé", &names(), 0, Sort::Newest), Err(ParseError::UnknownUser("zoé".into())));
        assert_eq!(parse("in:inconnu", &names(), 0, Sort::Newest), Err(ParseError::UnknownChannel("inconnu".into())));
        let private = Names { private: true, ..names() };
        assert_eq!(parse("in:général", &private, 0, Sort::Newest), Err(ParseError::NoChannels));
        assert_eq!(parse("has:fromage", &names(), 0, Sort::Newest), Err(ParseError::BadValue { filter: Filter::Has, value: "fromage".into() }));
        assert_eq!(parse("   ", &names(), 0, Sort::Newest), Err(ParseError::Empty));
        assert_eq!(parse(&"a".repeat(MAX_CONTENT + 1), &names(), 0, Sort::Newest), Err(ParseError::TooLong));
        // Un mot avec deux-points qui n'est pas un filtre reste un mot.
        assert_eq!(ok("heure:12h").query.content, "heure:12h");
    }

    #[test]
    fn dates_are_local_days() {
        // 2024-01-01 00:00 UTC = 1704067200000 ms.
        let day = 1_704_067_200_000i64;
        let sf = |ms: i64| ((ms as u64) - DISCORD_EPOCH) << 22;
        let p = ok("before:2024-01-01");
        assert_eq!((p.query.min_id, p.query.max_id), (None, Some(sf(day))));
        let p = ok("after:01/01/2024");
        assert_eq!((p.query.min_id, p.query.max_id), (Some(sf(day + 86_400_000) - 1), None));
        let p = ok("during:2024-01-01");
        assert_eq!((p.query.min_id, p.query.max_id), (Some(sf(day) - 1), Some(sf(day + 86_400_000))));
        // Heure de Paris en hiver (UTC+1) : le jour commence une heure plus tôt.
        let p = parse("before:2024-01-01", &names(), 3600, Sort::Newest).unwrap();
        assert_eq!(p.query.max_id, Some(sf(day - 3_600_000)));
        // Deux bornes : la plus étroite l'emporte.
        let p = ok("after:2024-01-01 after:2024-01-05 before:2024-02-01 before:2024-01-20");
        assert_eq!(p.query.min_id, Some(sf(day + 5 * 86_400_000) - 1));
        assert_eq!(p.query.max_id, Some(sf(day + 19 * 86_400_000)));
        assert_eq!(parse("before:hier", &names(), 0, Sort::Newest), Err(ParseError::BadDate("hier".into())));
        // Avant l'époque de Discord : 0, sans débordement.
        assert_eq!(ok("before:2001-01-01").query.max_id, Some(0));
    }

    #[test]
    fn tokens_keep_their_positions() {
        let t = tokens(r#"salut from:"Jean D" in:gén"#);
        assert_eq!(t.len(), 3);
        assert_eq!((t[0].filter, t[0].value.as_str(), t[0].start, t[0].end), (None, "salut", 0, 5));
        assert_eq!((t[1].filter, t[1].value.as_str()), (Some(Filter::From), "Jean D"));
        assert_eq!((t[2].filter, t[2].value.as_str(), t[2].end), (Some(Filter::In), "gén", r#"salut from:"Jean D" in:gén"#.len()));
        // Guillemet jamais fermé : jusqu'au bout.
        assert_eq!(tokens(r#"from:"Jean"#)[0].value, "Jean");
    }

    struct NoNames;
    impl Resolve for NoNames {
        fn user(&self, id: u64) -> String {
            id.to_string()
        }
        fn role(&self, id: u64) -> (String, u32) {
            (id.to_string(), 0)
        }
        fn channel(&self, id: u64) -> String {
            id.to_string()
        }
        fn time(&self, _: i64, _: TimeStyle) -> Option<String> {
            None
        }
    }

    fn marked(src: &str, terms: &[&str]) -> Vec<(String, bool)> {
        let mut d = md(src, &NoNames);
        mark_terms(&mut d, &terms.iter().map(|t| t.to_string()).collect::<Vec<_>>());
        d.blocks.iter().flat_map(|b| b.spans.iter().map(|s| (s.text.clone(), s.style.mark))).collect()
    }

    #[test]
    fn terms_are_marked_at_word_starts() {
        let m = marked("Le Rendez-vous est **rdv** ici, rendez-vous !", &["rendez", "rdv"]);
        let hits: Vec<&str> = m.iter().filter(|(_, k)| *k).map(|(t, _)| t.as_str()).collect();
        assert_eq!(hits, vec!["Rendez", "rdv", "rendez"]);
        // Le texte entier est conservé, gras compris.
        assert_eq!(m.iter().map(|(t, _)| t.as_str()).collect::<String>(), "Le Rendez-vous est rdv ici, rendez-vous !");
        // Au milieu d'un mot : rien.
        assert!(marked("prendez", &["rendez"]).iter().all(|(_, k)| !k));
        // Lettres dont la minuscule change de longueur, et accents.
        let m = marked("ÉTÉ İstanbul été", &["été"]);
        assert_eq!(m.iter().filter(|(_, k)| *k).count(), 2, "{m:?}");
        assert_eq!(m.iter().map(|(t, _)| t.as_str()).collect::<String>(), "ÉTÉ İstanbul été");
        assert!(marked("rien", &[]).iter().all(|(_, k)| !k));
    }

    /// Onglets (F16) : le filtre `has` de l'onglet s'ajoute à la requête, qui
    /// repart de la première page ; « Messages » revient à la requête tapée.
    #[test]
    fn tabs_add_a_has_filter_and_restart_from_the_first_page() {
        let mut s = Search::default();
        s.start(Scope::Guild(1), "x".into(), ok("x"));
        assert_eq!(s.tab(), Tab::Messages);
        assert!(s.set_tab(Tab::Messages).is_none(), "onglet déjà affiché");
        let Some(SearchCmd::Query { seq: 2, offset: 0, query, .. }) = s.set_tab(Tab::Images) else { panic!() };
        assert_eq!(query.has, [Has::Image]);
        assert!(s.current(1).is_none(), "réponse périmée");
        let Some(SearchCmd::Query { seq: 3, query, .. }) = s.set_tab(Tab::Files) else { panic!() };
        assert_eq!(query.has, [Has::File], "un seul filtre d'onglet à la fois");
        let Some(SearchCmd::Query { query, .. }) = s.set_tab(Tab::Messages) else { panic!() };
        assert!(query.has.is_empty());
        s.set_tab(Tab::Links);
        s.start(Scope::Guild(1), "y".into(), ok("y"));
        assert_eq!(s.tab(), Tab::Messages, "nouvelle recherche : premier onglet");
    }

    #[test]
    fn pages_and_sort() {
        let mut s = Search::default();
        let cmd = s.start(Scope::Guild(1), "x".into(), ok("x"));
        assert_eq!(cmd, SearchCmd::Query { seq: 1, scope: Scope::Guild(1), query: ok("x").query, offset: 0 });
        assert!(s.goto(1).is_none(), "total inconnu : une seule page");
        let a = s.current(1).unwrap();
        a.total = Some(1001);
        a.phase = Phase::Done;
        assert_eq!(a.last_page(), 40);
        assert!(s.current(1).is_some() && s.goto(0).is_none(), "page déjà affichée");
        let Some(SearchCmd::Query { seq: 2, offset: 50, .. }) = s.goto(2) else { panic!() };
        assert!(s.current(1).is_none(), "réponse périmée");
        let Some(SearchCmd::Query { seq: 3, offset: 0, query, .. }) = s.set_sort(Sort::Relevance) else { panic!() };
        assert_eq!(query.sort.params(), ("relevance", "desc"));
        assert!(s.set_sort(Sort::Relevance).is_none());
        // Au-delà de 9975 : la dernière page atteignable.
        let a = s.current(3).unwrap();
        a.total = Some(100_000);
        assert_eq!(a.last_page(), MAX_OFFSET / PAGE);
        assert_eq!(MAX_OFFSET / PAGE * PAGE, 9975);
    }

    #[test]
    fn message_links() {
        assert_eq!(message_link("https://discord.com/channels/1/2/3"), Some((Some(1), 2, 3)));
        assert_eq!(message_link("https://discord.com/channels/@me/2/3/"), Some((None, 2, 3)));
        assert_eq!(message_link("https://canary.discord.com/channels/1/2/3"), Some((Some(1), 2, 3)));
        for bad in ["https://discord.com/channels/1/2", "https://discord.com/channels/1/2/3/4", "https://evil.tld/channels/1/2/3", "https://discord.com.evil.tld/channels/1/2/3", "https://discord.com/channels/x/2/3"] {
            assert_eq!(message_link(bad), None, "{bad}");
        }
    }
}
