//! Logique de la zone de saisie, sans egui : mot en cours d'autocomplétion,
//! propositions, texte lisible et syntaxe envoyée, brouillons par salon.
//!
//! Le champ montre un texte lisible (`@Alice`, `#général`, `:blob:`) ; chaque
//! proposition choisie retient le lien entre ce texte et sa syntaxe Discord
//! (`<@id>`, `<#id>`, `<:blob:id>`). À l'envoi, chaque occurrence de ce texte
//! (hors code) devient la syntaxe. Deux personnes de même nom : la seconde
//! est insérée en syntaxe brute, pour ne jamais deviner.

use crate::state::Store;
use std::collections::HashMap;

/// Propositions affichées au plus.
pub(super) const MAX_SUGGESTIONS: usize = 8;
/// Brouillons gardés au plus (les plus anciens partent).
pub(super) const MAX_DRAFTS: usize = 100;
/// Liens gardés par brouillon au plus (les plus anciens partent).
const MAX_LINKS: usize = 50;
/// Lettres tapées après `:` avant de proposer des emojis (comme Discord).
const EMOJI_MIN: usize = 2;

/// Limite de Discord pour un message (caractères).
pub(super) const MESSAGE_MAX: usize = 2000;
/// Le compteur apparaît à partir de ce nombre de caractères.
pub(super) const COUNTER_FROM: usize = 1800;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Trigger {
    /// `@` : personnes, rôles, @everyone et @here.
    At,
    /// `#` : salons du serveur.
    Hash,
    /// `:` : emojis.
    Colon,
}

/// Mot en cours de frappe qui appelle des propositions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Query {
    pub trigger: Trigger,
    /// Position (en caractères) du symbole déclencheur.
    pub start: usize,
    /// Texte tapé après le symbole.
    pub needle: String,
}

/// Mot à compléter qui se termine au curseur (`cursor` en caractères), s'il
/// y en a un : un symbole en début de texte ou après un blanc, suivi de
/// lettres sans blanc.
pub(super) fn query_at(text: &str, cursor: usize) -> Option<Query> {
    let before: Vec<char> = text.chars().take(cursor).collect();
    if before.len() != cursor {
        return None;
    }
    let mut i = before.len();
    while i > 0 {
        let c = before[i - 1];
        if c.is_whitespace() {
            return None;
        }
        let trigger = match c {
            '@' => Some(Trigger::At),
            '#' => Some(Trigger::Hash),
            ':' => Some(Trigger::Colon),
            _ => None,
        };
        if let Some(trigger) = trigger {
            if i >= 2 && !before[i - 2].is_whitespace() {
                return None;
            }
            let needle: String = before[i..].iter().collect();
            if needle.chars().any(|c| matches!(c, '@' | '#' | ':' | '<' | '>' | '`')) {
                return None;
            }
            if trigger == Trigger::Colon && needle.chars().count() < EMOJI_MIN {
                return None;
            }
            return Some(Query { trigger, start: i - 1, needle });
        }
        i -= 1;
    }
    None
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Member,
    Role,
    Everyone,
    Here,
    Channel,
    CustomEmoji,
    Emoji,
}

/// Une proposition : ce qui est montré, ce qui est inséré, et la syntaxe
/// envoyée à la place du texte inséré (`None` : envoyé tel quel).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Suggestion {
    pub kind: Kind,
    pub label: String,
    pub insert: String,
    pub syntax: Option<String>,
}

/// Rang d'un nom pour `needle` (déjà en minuscules) : début du nom, début
/// d'un mot, puis n'importe où ; `None` sinon.
fn rank(name: &str, needle: &str) -> Option<u8> {
    if needle.is_empty() {
        return Some(0);
    }
    let name = name.to_lowercase();
    if name.starts_with(needle) {
        Some(0)
    } else if name.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(needle)) {
        Some(1)
    } else if name.contains(needle) {
        Some(2)
    } else {
        None
    }
}

/// Garde les `MAX_SUGGESTIONS` meilleures, à rang égal dans l'ordre d'origine.
fn best(mut items: Vec<(u8, Suggestion)>) -> Vec<Suggestion> {
    items.sort_by_key(|(r, _)| *r);
    items.into_iter().take(MAX_SUGGESTIONS).map(|(_, s)| s).collect()
}

/// Propositions pour le mot `q` dans `channel`, lues dans le magasin.
pub(super) fn suggest(store: &Store, channel: u64, q: &Query) -> Vec<Suggestion> {
    let needle = q.needle.to_lowercase();
    let mut out: Vec<(u8, Suggestion)> = Vec::new();
    match q.trigger {
        Trigger::At => {
            for (id, name) in store.known_people(channel) {
                if let Some(r) = rank(&name, &needle) {
                    out.push((r, Suggestion { kind: Kind::Member, insert: format!("@{name}"), syntax: Some(format!("<@{id}>")), label: name }));
                }
            }
            for role in store.mentionable_roles(channel) {
                if let Some(r) = rank(&role.name, &needle) {
                    let name = role.name.clone();
                    out.push((r, Suggestion { kind: Kind::Role, insert: format!("@{name}"), syntax: Some(format!("<@&{}>", role.id)), label: name }));
                }
            }
            if store.can_mention_everyone(channel) {
                for (kind, word) in [(Kind::Everyone, "everyone"), (Kind::Here, "here")] {
                    if let Some(r) = rank(word, &needle) {
                        out.push((r, Suggestion { kind, label: format!("@{word}"), insert: format!("@{word}"), syntax: None }));
                    }
                }
            }
        }
        Trigger::Hash => {
            for c in store.visible_channels(channel) {
                if let Some(r) = rank(&c.name, &needle) {
                    let name = c.name.clone();
                    out.push((r, Suggestion { kind: Kind::Channel, insert: format!("#{name}"), syntax: Some(format!("<#{}>", c.id)), label: name }));
                }
            }
        }
        Trigger::Colon => {
            for e in store.usable_emojis(channel) {
                if let Some(r) = rank(&e.name, &needle) {
                    let a = if e.animated { "a" } else { "" };
                    let syntax = format!("<{a}:{}:{}>", e.name, e.id);
                    out.push((r, Suggestion { kind: Kind::CustomEmoji, label: format!(":{}:", e.name), insert: format!(":{}:", e.name), syntax: Some(syntax) }));
                }
            }
            // Inséré sous son nom court, remplacé par le caractère à l'envoi.
            // Table complète (`state::emoji`) : le premier nom qui convient de chaque emoji.
            for e in crate::state::emoji::all().iter().filter(|e| !e.hidden()) {
                if let Some((r, name)) = e.names.iter().filter_map(|n| rank(n, &needle).map(|r| (r, n))).min_by_key(|(r, _)| *r) {
                    out.push((r, Suggestion { kind: Kind::Emoji, label: format!(":{name}:"), insert: format!(":{name}:"), syntax: None }));
                }
            }
        }
    }
    best(out)
}

/// Texte en cours dans un salon et liens « texte lisible → syntaxe ».
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Draft {
    pub text: String,
    /// (texte lisible, syntaxe) des propositions choisies.
    pub links: Vec<(String, String)>,
}

impl Draft {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }

    pub fn clear(&mut self) {
        self.text.clear();
        self.links.clear();
    }

    /// La proposition `s` a été insérée sous la forme `inserted` (voir
    /// [`inserts`]) : un texte lisible est lié à sa syntaxe.
    pub fn accept(&mut self, s: &Suggestion, inserted: &str) {
        let Some(syntax) = &s.syntax else { return };
        if inserted != s.insert || self.links.iter().any(|(d, _)| *d == s.insert) {
            return;
        }
        if self.links.len() >= MAX_LINKS {
            self.links.remove(0);
        }
        self.links.push((s.insert.clone(), syntax.clone()));
    }

    /// Lie `shown` à `syntax` (emoji personnalisé choisi dans le sélecteur) ;
    /// `false` : ce texte est déjà lié à autre chose (homonyme) et le reste :
    /// insérer alors la syntaxe brute, pour ne jamais deviner à l'envoi.
    pub fn link(&mut self, shown: String, syntax: String) -> bool {
        if let Some((_, x)) = self.links.iter().find(|(d, _)| *d == shown) {
            return *x == syntax;
        }
        if self.links.len() >= MAX_LINKS {
            self.links.remove(0);
        }
        self.links.push((shown, syntax));
        true
    }

    /// Texte envoyé : chaque texte lisible lié, hors code, devient sa syntaxe.
    pub fn encoded(&self) -> String {
        encode(&self.text, &self.links)
    }
}

/// Texte à insérer pour chaque proposition : lisible, sauf s'il désigne déjà
/// autre chose (lien existant, ou homonyme dans la même liste) : syntaxe
/// brute, pour qu'aucun texte lisible ne soit ambigu à l'envoi.
pub(super) fn inserts(links: &[(String, String)], items: &[Suggestion]) -> Vec<String> {
    items
        .iter()
        .map(|s| match &s.syntax {
            Some(syntax)
                if links.iter().any(|(d, x)| *d == s.insert && x != syntax)
                    || items.iter().any(|o| o.insert == s.insert && o.syntax.as_ref().is_some_and(|x| x != syntax)) =>
            {
                syntax.clone()
            }
            _ => s.insert.clone(),
        })
        .collect()
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Emoji Unicode du nom court `:nom:` qui commence `rest`, et sa longueur.
fn shortcode(rest: &str) -> Option<(String, usize)> {
    let inner = rest.strip_prefix(':')?;
    let end = inner.char_indices().take(40).find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_')).map(|(i, _)| i)?;
    if !inner[end..].starts_with(':') {
        return None;
    }
    let name = &inner[..end];
    crate::state::emoji::by_name(name).map(|u| (u.text(), end + 2))
}

/// Texte envoyé : liens remplacés par leur syntaxe, et noms courts d'emojis
/// connus (`:smile:`) par le caractère, comme le fait le client officiel ;
/// rien n'est touché dans le code.
fn encode(text: &str, links: &[(String, String)]) -> String {
    if links.is_empty() && !text.contains(':') {
        return text.to_string();
    }
    // Les plus longs d'abord : « @Ana Lise » avant « @Ana ».
    let mut links: Vec<&(String, String)> = links.iter().collect();
    links.sort_by_key(|(d, _)| std::cmp::Reverse(d.len()));
    let mut out = String::with_capacity(text.len());
    let mut code: Option<usize> = None;
    let mut prev: Option<char> = None;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        // Code en ligne ou bloc : ouvert et fermé par la même suite d'accents graves.
        if rest.starts_with('`') {
            let run = rest.len() - rest.trim_start_matches('`').len();
            code = match code {
                None => Some(run),
                Some(n) if n == run => None,
                c => c,
            };
            out.push_str(&rest[..run]);
            prev = Some('`');
            i += run;
            continue;
        }
        if code.is_none() && !prev.is_some_and(word_char) {
            let hit = links.iter().find(|(d, _)| rest.starts_with(d.as_str()) && !rest[d.len()..].chars().next().is_some_and(word_char));
            if let Some((d, syntax)) = hit {
                out.push_str(syntax);
                prev = d.chars().last();
                i += d.len();
                continue;
            }
            if let Some((glyph, len)) = shortcode(rest)
                && !rest[len..].chars().next().is_some_and(word_char)
            {
                out.push_str(&glyph);
                prev = Some(':');
                i += len;
                continue;
            }
        }
        let Some(c) = rest.chars().next() else { break };
        out.push(c);
        prev = Some(c);
        i += c.len_utf8();
    }
    out
}

/// Brouillons des autres salons, bornés : au-delà de `MAX_DRAFTS`, le
/// moins récemment rangé part. Rien n'est écrit sur le disque.
#[derive(Default)]
pub(super) struct Drafts {
    map: HashMap<u64, (Draft, u64)>,
    tick: u64,
}

impl Drafts {
    /// Range le brouillon d'un salon (un texte vide efface le brouillon).
    pub fn put(&mut self, channel: u64, draft: Draft) {
        if draft.is_empty() {
            self.map.remove(&channel);
            return;
        }
        if !self.map.contains_key(&channel)
            && self.map.len() >= MAX_DRAFTS
            && let Some(old) = self.map.iter().min_by_key(|(_, (_, t))| *t).map(|(c, _)| *c)
        {
            self.map.remove(&old);
        }
        self.tick += 1;
        self.map.insert(channel, (draft, self.tick));
    }

    pub fn take(&mut self, channel: u64) -> Draft {
        self.map.remove(&channel).map(|(d, _)| d).unwrap_or_default()
    }

    pub fn has(&self, channel: u64) -> bool {
        self.map.contains_key(&channel)
    }

    /// Tous les brouillons rangés (brouillons persistants, lot E3).
    pub fn iter(&self) -> impl Iterator<Item = (u64, &Draft)> {
        self.map.iter().map(|(c, (d, _))| (*c, d))
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Channel, ChannelKind, Emoji, Guild, Member, Role, User};
    use crate::state::{Event, Permissions as P};

    fn q(text: &str) -> Option<Query> {
        query_at(text, text.chars().count())
    }

    #[test]
    fn query_needs_a_symbol_at_word_start() {
        assert_eq!(q("salut @al"), Some(Query { trigger: Trigger::At, start: 6, needle: "al".into() }));
        assert_eq!(q("@").map(|x| x.needle), Some(String::new()));
        assert_eq!(q("voir #gé").map(|x| (x.trigger, x.needle)), Some((Trigger::Hash, "gé".into())));
        assert_eq!(q(":sm").map(|x| x.trigger), Some(Trigger::Colon));
        // Une seule lettre après « : » ne suffit pas (« 10:3 », « :) »).
        assert_eq!(q("a :s"), None);
        assert_eq!(q("12:30"), None);
        assert_eq!(q("mail@exemple"), None);
        assert_eq!(q("@al "), None);
        assert_eq!(q("rien"), None);
        // Curseur au milieu : seul ce qui le précède compte.
        assert_eq!(query_at("@alice bob", 3).map(|x| x.needle), Some("al".into()));
        assert_eq!(query_at("@a", 5), None);
    }

    fn store() -> Store {
        let mut s = Store::new();
        let chan = |id: u64, name: &str| Channel { id, name: name.into(), kind: ChannelKind::Text, position: id as i32, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        let mut g = Guild::new(9, "g".into(), vec![chan(100, "général"), chan(101, "jeux-vidéo"), chan(102, "secret")]);
        g.roles = vec![
            Role { id: 9, name: "@everyone".into(), color: 0, position: 0, permissions: P::VIEW_CHANNEL | P::SEND_MESSAGES | P::READ_MESSAGE_HISTORY, mentionable: false },
            Role { id: 5, name: "Admins".into(), color: 0, position: 2, permissions: P::NONE, mentionable: true },
        ];
        g.channels[2].overwrites = Some(vec![crate::state::model::Overwrite { id: 9, kind: crate::state::model::OverwriteKind::Role, allow: P::NONE, deny: P::VIEW_CHANNEL }]);
        g.me = Some(Member::default());
        g.owner_id = Some(77);
        g.emojis = vec![Emoji { id: 42, name: "smirk_cat".into(), animated: true, available: true, roles: Vec::new() }];
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![g], dms: Vec::new() });
        s.select_guild(9);
        s
    }

    fn labels(v: &[Suggestion]) -> Vec<&str> {
        v.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn suggestions_by_kind() {
        let s = store();
        let at = suggest(&s, 100, &q("@").unwrap_or_else(|| panic!()));
        assert_eq!(labels(&at), vec!["Moi", "Admins"]);
        assert_eq!(at[1].syntax.as_deref(), Some("<@&5>"));
        let hash = suggest(&s, 100, &q("#j").unwrap_or_else(|| panic!()));
        assert_eq!(labels(&hash), vec!["jeux-vidéo"]);
        assert_eq!(hash[0].syntax.as_deref(), Some("<#101>"));
        // Salon masqué : jamais proposé.
        assert!(suggest(&s, 100, &q("#sec").unwrap_or_else(|| panic!())).is_empty());
        let em = suggest(&s, 100, &q(":smi").unwrap_or_else(|| panic!()));
        assert_eq!(em[0].label, ":smirk_cat:");
        assert_eq!(em[0].syntax.as_deref(), Some("<a:smirk_cat:42>"));
        assert!(em.iter().any(|e| e.label == ":smile:" && e.insert == ":smile:" && e.syntax.is_none()));
        assert!(em.len() <= MAX_SUGGESTIONS);
        // Début du nom avant le milieu : « joy » avant « … joy … ».
        let joy = suggest(&s, 100, &q(":jo").unwrap_or_else(|| panic!()));
        assert_eq!(joy[0].label, ":joy:");
    }

    fn member(id: u64, name: &str) -> Suggestion {
        Suggestion { kind: Kind::Member, label: name.into(), insert: format!("@{name}"), syntax: Some(format!("<@{id}>")) }
    }

    /// Insère `s` comme la liste de propositions le ferait.
    fn pick(d: &mut Draft, items: &[Suggestion], i: usize) {
        let ins = inserts(&d.links, items);
        d.text.push_str(&ins[i]);
        d.text.push(' ');
        d.accept(&items[i], &ins[i]);
    }

    #[test]
    fn readable_text_is_sent_as_syntax() {
        let mut d = Draft { text: "salut ".into(), ..Draft::default() };
        pick(&mut d, &[member(7, "Alice")], 0);
        assert_eq!(d.text, "salut @Alice ");
        d.text.push_str("et @Alice, `@Alice` @Alicette mail@Alice");
        assert_eq!(d.encoded(), "salut <@7> et <@7>, `@Alice` @Alicette mail@Alice");
        // Homonyme d'un nom déjà lié : syntaxe brute.
        d.text.push(' ');
        pick(&mut d, &[member(8, "Alice")], 0);
        assert!(d.text.ends_with(" <@8> "), "{}", d.text);
        // Homonymes dans la même liste : syntaxe brute pour chacun.
        assert_eq!(inserts(&[], &[member(1, "Léo"), member(2, "Léo"), member(3, "Zoé")]), vec!["<@1>", "<@2>", "@Zoé"]);
        // Bloc de code : rien n'est remplacé dedans.
        let mut d = Draft::default();
        pick(&mut d, &[member(7, "Alice")], 0);
        d.text.push_str("```\n@Alice\n``` @Alice");
        assert_eq!(d.encoded(), "<@7> ```\n@Alice\n``` <@7>");
        // Emoji Unicode : inséré tel quel, aucun lien.
        let mut d = Draft { text: "ok ".into(), ..Draft::default() };
        let e = Suggestion { kind: Kind::Emoji, label: ":smile:".into(), insert: ":smile:".into(), syntax: None };
        pick(&mut d, &[e], 0);
        assert_eq!(d.text, "ok :smile: ");
        assert!(d.links.is_empty());
        // Nom court tapé à la main : converti aussi, sauf dans le code ou s'il est inconnu.
        d.text.push_str(":fire::x: `:fire:` :inconnu: a:fire: :fire:s");
        assert_eq!(d.encoded(), "ok 😄 🔥❌ `:fire:` :inconnu: a:fire: :fire:s");
    }

    #[test]
    fn drafts_are_bounded() {
        let mut ds = Drafts::default();
        for c in 0..(MAX_DRAFTS as u64 + 10) {
            ds.put(c, Draft { text: format!("d{c}"), ..Draft::default() });
        }
        assert!(!ds.has(0) && ds.has(MAX_DRAFTS as u64 + 9));
        assert_eq!(ds.take(50).text, "d50");
        assert!(!ds.has(50));
        ds.put(1, Draft { text: "  ".into(), ..Draft::default() });
        assert!(!ds.has(1));
        assert_eq!(ds.take(1), Draft::default());
    }
}
