//! Mots-clés surveillés (plugin, lot E1) : une liste de mots ou
//! d'expressions ; un message **reçu** qui en contient un donne une
//! notification locale, une ligne dans la boîte de réception, et le mot est
//! surligné dans le fil. Aucune requête : on ne regarde que ce que le
//! client reçoit déjà.
//!
//! Ici : les règles et leur forme dans `local.toml` (section `[mots_cles]`),
//! la recherche d'un mot dans un texte (casse ignorée, mot entier ou non),
//! le surlignage d'un texte mis en forme. Ni egui, ni réseau, ni disque.

use super::markdown::{Doc, Span, SpanKind};
use std::collections::HashSet;

pub const SECTION: &str = "mots_cles";
const K_RULES: &str = "regles";
const K_TEXT: &str = "texte";
const K_WHOLE: &str = "mot_entier";
const K_GUILD: &str = "serveur";
const K_EXCLUDED: &str = "salons_exclus";

/// Règles gardées au plus.
pub const MAX_RULES: usize = 50;
/// Longueur d'un mot ou d'une expression (caractères).
pub const MAX_LEN: usize = 60;
/// Salons exclus au plus.
pub const MAX_EXCLUDED: usize = 200;
/// Lignes gardées pour la boîte de réception (les plus anciennes partent).
pub const MAX_HITS: usize = 50;
/// Longueur de l'extrait d'une ligne (caractères).
pub const EXCERPT: usize = 200;

/// Un mot ou une expression à surveiller.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Rule {
    pub text: String,
    /// Mot entier seulement (« chat » ne répond pas à « chaton »).
    pub whole: bool,
    /// Dans ce serveur seulement ; `None` : partout, messages privés compris.
    pub guild: Option<u64>,
}

/// Réglages du plugin (cet ordinateur seulement).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KeywordPrefs {
    pub rules: Vec<Rule>,
    /// Salons où rien n'est surveillé.
    pub excluded: Vec<u64>,
}

/// Texte d'une règle tel qu'il est gardé : sans blancs autour ni caractères
/// de contrôle, borné.
pub fn clean(text: &str) -> String {
    text.trim().chars().filter(|c| !c.is_control()).take(MAX_LEN).collect::<String>().trim().to_string()
}

impl KeywordPrefs {
    /// Section `[mots_cles]` de `local.toml`.
    pub fn to_toml(&self) -> String {
        let q = super::audio_prefs::quoted;
        let rules: Vec<String> = self
            .rules
            .iter()
            .map(|r| {
                let guild = r.guild.map(|g| format!(", {K_GUILD} = {g}")).unwrap_or_default();
                format!("{{ {K_TEXT} = {}, {K_WHOLE} = {}{guild} }}", q(&r.text), r.whole)
            })
            .collect();
        let excluded: Vec<String> = self.excluded.iter().map(u64::to_string).collect();
        format!("[{SECTION}]\n# Mots-clés surveillés (Paramètres > Notifications).\n{K_RULES} = [{}]\n{K_EXCLUDED} = [{}]\n", rules.join(", "), excluded.join(", "))
    }

    /// Lit la section ; entrée illisible : ignorée.
    pub fn from_table(t: &toml::de::DeTable<'_>) -> KeywordPrefs {
        let id = |v: &toml::de::DeValue<'_>| v.as_integer().and_then(|i| u64::from_str_radix(i.as_str(), i.radix()).ok()).filter(|id| *id != 0);
        let mut p = KeywordPrefs::default();
        for (k, v) in t.iter() {
            let Some(items) = v.get_ref().as_array() else { continue };
            match k.get_ref().as_ref() {
                K_RULES => {
                    for item in items.iter() {
                        let Some(e) = item.get_ref().as_table() else { continue };
                        let field = |key: &str| e.iter().find(|(k, _)| k.get_ref().as_ref() == key).map(|(_, v)| v.get_ref());
                        let text = field(K_TEXT).and_then(|v| v.as_str()).map(clean).unwrap_or_default();
                        let rule = Rule { text, whole: field(K_WHOLE).and_then(|v| v.as_bool()).unwrap_or(false), guild: field(K_GUILD).and_then(id) };
                        p.add(rule);
                    }
                }
                K_EXCLUDED => {
                    for c in items.iter().filter_map(|v| id(v.get_ref())) {
                        if !p.excluded.contains(&c) && p.excluded.len() < MAX_EXCLUDED {
                            p.excluded.push(c);
                        }
                    }
                }
                _ => {}
            }
        }
        p
    }

    /// Ajoute une règle (texte nettoyé) ; faux si elle est vide, en double
    /// ou de trop.
    pub fn add(&mut self, mut rule: Rule) -> bool {
        rule.text = clean(&rule.text);
        if rule.text.is_empty() || self.rules.len() >= MAX_RULES || self.rules.iter().any(|r| r.text.to_lowercase() == rule.text.to_lowercase() && r.guild == rule.guild) {
            return false;
        }
        self.rules.push(rule);
        true
    }

    /// Exclut ou réintègre un salon.
    pub fn set_excluded(&mut self, channel: u64, excluded: bool) {
        self.excluded.retain(|c| *c != channel);
        if excluded && self.excluded.len() < MAX_EXCLUDED {
            self.excluded.push(channel);
        }
    }
}

/// Longueur (octets de `text`) du début de `text` égal à `needle` (déjà en
/// minuscules), à la casse près ; comparaison caractère par caractère : la
/// longueur en octets des minuscules peut différer de celle du texte.
fn prefix_len(text: &str, needle: &str) -> Option<usize> {
    let mut want = needle.chars().peekable();
    let mut len = 0;
    for c in text.chars() {
        if want.peek().is_none() {
            break;
        }
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

/// Les règles prêtes à servir : construit une fois quand elles changent,
/// consulté à chaque message reçu.
#[derive(Clone, Debug, Default)]
pub struct Matcher {
    /// (texte en minuscules, mot entier, serveur).
    rules: Vec<(String, bool, Option<u64>)>,
    excluded: HashSet<u64>,
}

impl Matcher {
    pub fn new(p: &KeywordPrefs) -> Matcher {
        Matcher { rules: p.rules.iter().filter(|r| !r.text.is_empty()).map(|r| (r.text.to_lowercase(), r.whole, r.guild)).collect(), excluded: p.excluded.iter().copied().collect() }
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Des règles s'appliquent-elles à ce salon ?
    pub fn watches(&self, guild: Option<u64>, channel: u64) -> bool {
        !self.excluded.contains(&channel) && self.rules.iter().any(|(_, _, g)| g.is_none() || *g == guild)
    }

    /// Occurrences (octets, sans chevauchement, dans l'ordre) des mots
    /// surveillés dans `text`, avec le rang de la règle.
    fn matches(&self, guild: Option<u64>, text: &str) -> Vec<(usize, usize, usize)> {
        let mut out = Vec::new();
        let mut prev: Option<char> = None;
        let mut skip_to = 0;
        for (i, c) in text.char_indices() {
            let before_ok = prev.is_none_or(|p| !p.is_alphanumeric());
            prev = Some(c);
            if i < skip_to {
                continue;
            }
            let rest = &text[i..];
            let hit = self
                .rules
                .iter()
                .enumerate()
                .filter(|(_, (_, _, g))| g.is_none() || *g == guild)
                .filter_map(|(k, (needle, whole, _))| {
                    let n = prefix_len(rest, needle)?;
                    let after_ok = rest[n..].chars().next().is_none_or(|a| !a.is_alphanumeric());
                    (!*whole || (before_ok && after_ok)).then_some((n, k))
                })
                .max();
            if let Some((n, k)) = hit {
                out.push((i, i + n, k));
                skip_to = i + n;
            }
        }
        out
    }

    /// Premier mot surveillé trouvé dans `text` (tel qu'il est écrit dans la
    /// règle, en minuscules) ; `None` dans un salon exclu.
    pub fn find(&self, guild: Option<u64>, channel: u64, text: &str) -> Option<&str> {
        if self.rules.is_empty() || !self.watches(guild, channel) {
            return None;
        }
        self.matches(guild, text).first().map(|(_, _, k)| self.rules[*k].0.as_str())
    }

    /// Surligne les mots surveillés dans le texte mis en forme d'un message
    /// de ce salon (passages de texte seulement : ni code, ni lien, ni mention).
    pub fn mark(&self, guild: Option<u64>, channel: u64, doc: &mut Doc) {
        if self.rules.is_empty() || !self.watches(guild, channel) {
            return;
        }
        for b in &mut doc.blocks {
            if !b.spans.iter().any(|s| s.kind == SpanKind::Text && !s.style.code && !self.matches(guild, &s.text).is_empty()) {
                continue;
            }
            let spans = std::mem::take(&mut b.spans);
            for s in spans {
                if s.kind != SpanKind::Text || s.style.code {
                    b.spans.push(s);
                    continue;
                }
                let mut at = 0;
                for (a, z, _) in self.matches(guild, &s.text) {
                    if a > at {
                        b.spans.push(Span { text: s.text[at..a].to_string(), ..s.clone() });
                    }
                    let mut style = s.style;
                    style.mark = true;
                    b.spans.push(Span { text: s.text[a..z].to_string(), style, kind: SpanKind::Text });
                    at = z;
                }
                if at < s.text.len() {
                    b.spans.push(Span { text: s.text[at..].to_string(), ..s });
                }
            }
        }
    }
}

/// Message reçu qui contient un mot surveillé : ligne de la boîte de réception.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hit {
    pub message: u64,
    pub channel: u64,
    pub guild: Option<u64>,
    pub author: String,
    /// Le mot trouvé.
    pub word: String,
    /// Début du message, en texte simple.
    pub excerpt: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::markdown::{parse, Resolve, TimeStyle};

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

    fn prefs() -> KeywordPrefs {
        let mut p = KeywordPrefs::default();
        assert!(p.add(Rule { text: "  Ferrocord ".into(), whole: false, guild: None }));
        assert!(p.add(Rule { text: "chat".into(), whole: true, guild: None }));
        assert!(p.add(Rule { text: "mise à jour".into(), whole: false, guild: Some(9) }));
        assert!(!p.add(Rule { text: "FERROCORD".into(), whole: true, guild: None }), "doublon à la casse près");
        assert!(!p.add(Rule { text: "   ".into(), whole: false, guild: None }));
        p.set_excluded(77, true);
        p
    }

    #[test]
    fn words_match_by_case_whole_word_scope_and_exclusion() {
        let m = Matcher::new(&prefs());
        assert_eq!(m.find(None, 1, "J'adore FERROCORDIEN !"), Some("ferrocord"), "casse ignorée, partie de mot");
        assert_eq!(m.find(None, 1, "un chaton"), None, "mot entier demandé");
        assert_eq!(m.find(None, 1, "le Chat, là"), Some("chat"));
        assert_eq!(m.find(Some(9), 1, "Grosse MISE À JOUR ce soir"), Some("mise à jour"), "expression, accents");
        assert_eq!(m.find(Some(8), 1, "mise à jour"), None, "autre serveur");
        assert_eq!(m.find(None, 1, "mise à jour"), None, "règle d'un serveur : pas en MP");
        assert_eq!(m.find(None, 77, "ferrocord"), None, "salon exclu");
        assert!(m.watches(None, 1) && !m.watches(None, 77));
        assert_eq!(m.find(None, 1, ""), None);
        assert!(Matcher::new(&KeywordPrefs::default()).is_empty());
    }

    #[test]
    fn words_are_marked_in_the_rendered_text() {
        let m = Matcher::new(&prefs());
        let mut doc = parse("Vive **Ferrocord** et le chat, `ferrocord` aussi", &NoNames);
        m.mark(None, 1, &mut doc);
        let marked: Vec<&str> = doc.blocks.iter().flat_map(|b| b.spans.iter()).filter(|s| s.style.mark).map(|s| s.text.as_str()).collect();
        assert_eq!(marked, vec!["Ferrocord", "chat"], "le code n'est pas surligné");
        let text: String = doc.blocks.iter().flat_map(|b| b.spans.iter()).map(|s| s.text.as_str()).collect();
        assert_eq!(text, "Vive Ferrocord et le chat, ferrocord aussi", "rien n'est perdu");
        let mut other = parse("ferrocord", &NoNames);
        m.mark(None, 77, &mut other);
        assert!(other.blocks[0].spans.iter().all(|s| !s.style.mark), "salon exclu");
    }

    #[test]
    fn prefs_round_trip_and_bad_entries_are_ignored() {
        let p = prefs();
        let text = p.to_toml();
        let root = toml::de::DeTable::parse(&text).unwrap();
        let table = root.get_ref().iter().find(|(k, _)| k.get_ref().as_ref() == SECTION).and_then(|(_, v)| v.get_ref().as_table()).unwrap();
        assert_eq!(KeywordPrefs::from_table(table), p, "{text}");
        let odd = "[mots_cles]\nregles = [{ texte = 3 }, { texte = \"ok\", serveur = \"x\" }, 7]\nsalons_exclus = [0, \"a\", 12, 12]\nautre = 1\n";
        let root = toml::de::DeTable::parse(odd).unwrap();
        let table = root.get_ref().iter().next().and_then(|(_, v)| v.get_ref().as_table()).unwrap();
        let got = KeywordPrefs::from_table(table);
        assert_eq!(got.rules, vec![Rule { text: "ok".into(), whole: false, guild: None }]);
        assert_eq!(got.excluded, vec![12]);
    }
}
