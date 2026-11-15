//! Navigation : historique précédent / suivant, destinations récentes,
//! parcours des salons et des serveurs au clavier, et recherche du
//! sélecteur rapide (Ctrl+K). Tout est local : aucune requête.
//!
//! Le sélecteur fait sa liste de candidats une fois, à l'ouverture, et ne
//! refait la recherche que lorsque le texte saisi change.

use super::model::ChannelKind;
use super::store::Store;
use std::collections::{HashSet, VecDeque};

/// Pas d'historique gardés de chaque côté, au plus.
pub const MAX_HISTORY: usize = 50;
/// Destinations récentes gardées (en tête du sélecteur), au plus.
pub const MAX_RECENT: usize = 30;
/// Résultats du sélecteur, au plus.
pub const MAX_RESULTS: usize = 200;

/// Endroit affiché : serveur (`None` : messages privés) et salon.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Place {
    pub guild: Option<u64>,
    pub channel: Option<u64>,
}

/// Où mène une ligne du sélecteur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Target {
    /// Salon (serveur, fil ou MP) ; `guild` : `None` pour un MP.
    Channel { guild: Option<u64>, id: u64 },
    Guild(u64),
    /// Ami sans MP connu : le MP est ouvert (ou demandé) au choix.
    Friend(u64),
}

/// Précédent / suivant (Alt+← / Alt+→, boutons 4 et 5 de la souris).
#[derive(Default)]
pub struct History {
    current: Option<Place>,
    back: VecDeque<Place>,
    forward: Vec<Place>,
    recent: VecDeque<Target>,
}

impl History {
    /// Endroit affiché à cette image : un changement entre dans l'historique
    /// (et efface les pas « suivants »), et dans les destinations récentes.
    pub fn observe(&mut self, now: Place) {
        if self.current == Some(now) {
            return;
        }
        if let Some(old) = self.current.replace(now) {
            push_bounded(&mut self.back, old);
            self.forward.clear();
            if old.guild != now.guild
                && let Some(g) = now.guild
            {
                self.remember(Target::Guild(g));
            }
        }
        if let Some(id) = now.channel {
            self.remember(Target::Channel { guild: now.guild, id });
        }
    }

    fn remember(&mut self, t: Target) {
        self.recent.retain(|x| *x != t);
        self.recent.push_front(t);
        self.recent.truncate(MAX_RECENT);
    }

    /// Pas en arrière : l'endroit où aller (déjà tenu pour courant).
    pub fn back(&mut self) -> Option<Place> {
        let to = self.back.pop_back()?;
        if let Some(cur) = self.current.replace(to) {
            self.forward.push(cur);
            if self.forward.len() > MAX_HISTORY {
                self.forward.remove(0);
            }
        }
        Some(to)
    }

    pub fn forward(&mut self) -> Option<Place> {
        let to = self.forward.pop()?;
        if let Some(cur) = self.current.replace(to) {
            push_bounded(&mut self.back, cur);
        }
        Some(to)
    }

    pub fn can_back(&self) -> bool {
        !self.back.is_empty()
    }

    pub fn can_forward(&self) -> bool {
        !self.forward.is_empty()
    }

    /// Destinations récentes, la plus récente d'abord.
    pub fn recent(&self) -> impl Iterator<Item = Target> + '_ {
        self.recent.iter().copied()
    }

    /// Endroit disparu (salon supprimé, serveur quitté) : oublié partout.
    pub fn retain(&mut self, keep: impl Fn(&Place) -> bool) {
        self.back.retain(&keep);
        self.forward.retain(&keep);
    }

    pub fn clear(&mut self) {
        *self = History::default();
    }
}

fn push_bounded(q: &mut VecDeque<Place>, p: Place) {
    if q.back() != Some(&p) {
        q.push_back(p);
    }
    while q.len() > MAX_HISTORY {
        q.pop_front();
    }
}

/// Élément suivant (ou précédent) de `list` après `current`, en boucle.
/// Absent de la liste : le premier (ou le dernier).
pub fn cycle<T: Copy + PartialEq>(list: &[T], current: Option<T>, forward: bool) -> Option<T> {
    if list.is_empty() {
        return None;
    }
    let n = list.len();
    let next = match current.and_then(|c| list.iter().position(|x| *x == c)) {
        Some(i) if forward => (i + 1) % n,
        Some(i) => (i + n - 1) % n,
        None if forward => 0,
        None => n - 1,
    };
    list.get(next).copied()
}

/// Salons où mènent Alt+↑ / Alt+↓ dans la vue affichée : salons visibles du
/// serveur ouvert dans l'ordre de la liste, ou conversations privées.
pub fn channels_here(s: &Store) -> Vec<u64> {
    match s.current_guild() {
        Some(g) => g.channels.iter().filter(|c| c.kind != ChannelKind::Category && !s.is_hidden(c.id)).map(|c| c.id).collect(),
        None => s.dms.iter().map(|d| d.id).collect(),
    }
}

/// Tous les salons dans l'ordre du rail puis de la liste (conversations
/// privées d'abord), avec leur serveur : parcours des non-lus.
pub fn all_channels(s: &Store) -> Vec<Place> {
    let mut out: Vec<Place> = s.dms.iter().map(|d| Place { guild: None, channel: Some(d.id) }).collect();
    for g in &s.guilds {
        for c in g.channels.iter().filter(|c| c.kind != ChannelKind::Category && !s.is_hidden(c.id)) {
            out.push(Place { guild: Some(g.id), channel: Some(c.id) });
        }
    }
    out
}

/// Non-lu suivant (ou précédent) après le salon affiché, tous serveurs confondus.
pub fn unread_step(s: &Store, forward: bool) -> Option<Place> {
    let here = s.channel;
    let all = all_channels(s);
    let unread = s.unread();
    let n = all.len();
    let start = here.and_then(|c| all.iter().position(|p| p.channel == Some(c)));
    (1..=n).find_map(|k| {
        let i = match (start, forward) {
            (Some(i), true) => (i + k) % n,
            (Some(i), false) => (i + n * 2 - k) % n,
            (None, true) => k - 1,
            (None, false) => n - k,
        };
        all.get(i).filter(|p| p.channel.is_some_and(|c| unread.contains(&c) && Some(c) != here)).copied()
    })
}

/// Serveurs dans l'ordre du rail, messages privés (`None`) en tête.
pub fn guild_order(s: &Store) -> Vec<Option<u64>> {
    std::iter::once(None).chain(s.guilds.iter().map(|g| Some(g.id))).collect()
}

// ============================================================ sélecteur rapide

/// Genre d'une ligne (symbole affiché, filtre des préfixes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Text,
    Announcement,
    Forum,
    Thread,
    /// Salon vocal ou de conférence.
    Voice,
    Guild,
    Dm,
    Group,
    Friend,
}

/// Ce que limite un préfixe de la recherche, comme dans Discord.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    All,
    /// `#` : salons textuels, fils, forums.
    Text,
    /// `@` : conversations privées, groupes, amis.
    People,
    /// `*` : serveurs.
    Guilds,
    /// `!` : salons vocaux et de conférence.
    Voice,
}

impl Scope {
    fn admits(self, k: Kind) -> bool {
        match self {
            Scope::All => true,
            Scope::Text => matches!(k, Kind::Text | Kind::Announcement | Kind::Forum | Kind::Thread),
            Scope::People => matches!(k, Kind::Dm | Kind::Group | Kind::Friend),
            Scope::Guilds => k == Kind::Guild,
            Scope::Voice => k == Kind::Voice,
        }
    }
}

/// Préfixe et reste de la recherche.
pub fn parse_query(q: &str) -> (Scope, &str) {
    let q = q.trim_start();
    let scope = match q.chars().next() {
        Some('#') => Scope::Text,
        Some('@') => Scope::People,
        Some('*') => Scope::Guilds,
        Some('!') => Scope::Voice,
        _ => return (Scope::All, q.trim()),
    };
    (scope, q[1..].trim())
}

#[derive(Clone, Debug, PartialEq)]
pub struct Candidate {
    pub target: Target,
    pub kind: Kind,
    pub name: String,
    /// Serveur (ou salon parent d'un fil, nom d'utilisateur d'un ami).
    pub context: String,
    folded: String,
}

impl Candidate {
    fn new(target: Target, kind: Kind, name: &str, context: String) -> Self {
        Candidate { target, kind, name: name.to_string(), context, folded: fold(name) }
    }
}

/// Lignes possibles du sélecteur : salons visibles (vocaux compris), fils rejoints, serveurs,
/// conversations privées et groupes, amis sans conversation connue.
pub fn candidates(s: &Store) -> Vec<Candidate> {
    let mut out = Vec::new();
    for d in &s.dms {
        let group = s.relations.dm_info(d.id).map_or(d.recipients.len() > 1, |i| i.group);
        out.push(Candidate::new(Target::Channel { guild: None, id: d.id }, if group { Kind::Group } else { Kind::Dm }, &d.name, String::new()));
    }
    // Un ami qui a déjà une conversation est trouvé par elle.
    let dms: HashSet<u64> = s.dms.iter().map(|d| d.id).collect();
    for r in s.relations.friends() {
        if s.relations.dm_with(r.id).is_some_and(|c| dms.contains(&c)) {
            continue;
        }
        let name = s.relations.display_name(r.id);
        let user = s.relations.person(r.id).map(|p| p.username.clone()).unwrap_or_default();
        if !name.is_empty() {
            out.push(Candidate::new(Target::Friend(r.id), Kind::Friend, name, user));
        }
    }
    for g in &s.guilds {
        out.push(Candidate::new(Target::Guild(g.id), Kind::Guild, &g.name, String::new()));
        for c in g.channels.iter().filter(|c| c.kind != ChannelKind::Category && !s.is_hidden(c.id)) {
            let kind = match c.kind {
                ChannelKind::Announcement => Kind::Announcement,
                ChannelKind::Forum | ChannelKind::Media => Kind::Forum,
                _ => Kind::Text,
            };
            out.push(Candidate::new(Target::Channel { guild: Some(g.id), id: c.id }, kind, &c.name, g.name.clone()));
            for t in s.threads.children(c.id).iter().filter_map(|id| s.threads.get(*id)).filter(|t| t.joined && !s.is_hidden(t.id)) {
                out.push(Candidate::new(Target::Channel { guild: Some(g.id), id: t.id }, Kind::Thread, &t.name, format!("{} \u{b7} {}", c.name, g.name)));
            }
        }
        for c in s.voice.channels(g.id).iter().filter(|c| !s.voice.hidden(c.id)) {
            out.push(Candidate::new(Target::Channel { guild: Some(g.id), id: c.id }, Kind::Voice, &c.name, g.name.clone()));
        }
    }
    out
}

/// Minuscules, sans les accents courants : « Général » trouve « general ».
pub fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' | 'á' | 'ã' | 'å' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' | 'í' | 'ì' => 'i',
            'ô' | 'ö' | 'ó' | 'õ' | 'ò' | 'ø' => 'o',
            'ù' | 'û' | 'ü' | 'ú' => 'u',
            'ý' | 'ÿ' => 'y',
            'ç' => 'c',
            'ñ' => 'n',
            c => c,
        })
        .collect()
}

/// Clé de tri des noms : sans casse ni accents (« émile » entre « Denis » et
/// « Fanny »), puis le nom tel quel pour départager de façon stable.
pub fn sort_key(s: &str) -> (String, String) {
    (fold(s), s.to_owned())
}

/// Score d'un nom (déjà replié) pour une recherche (déjà repliée) : début du
/// nom, puis début d'un mot, puis morceau contigu, puis lettres dans
/// l'ordre ; `None` si les lettres n'y sont pas toutes, dans l'ordre.
pub fn score(query: &str, name: &str) -> Option<i32> {
    if query.is_empty() {
        return Some(0);
    }
    let len_penalty = i32::try_from(name.chars().count()).unwrap_or(i32::MAX).min(200);
    if name.starts_with(query) {
        return Some(4000 - len_penalty);
    }
    if let Some(pos) = name.find(query) {
        let word_start = name[..pos].chars().last().is_some_and(|c| !c.is_alphanumeric());
        let pos = i32::try_from(pos).unwrap_or(i32::MAX).min(200);
        return Some(if word_start { 3000 } else { 2000 } - pos - len_penalty);
    }
    let mut chars = name.chars().peekable();
    let (mut gaps, mut prev_matched) = (0i32, true);
    for q in query.chars() {
        loop {
            let c = chars.next()?;
            if c == q {
                prev_matched = true;
                break;
            }
            if prev_matched {
                gaps += 1;
            }
            prev_matched = false;
        }
    }
    Some(1000 - gaps * 50 - len_penalty)
}

/// Rangs des candidats à montrer pour `query`, meilleurs d'abord. Recherche
/// vide : destinations récentes, puis le reste dans l'ordre. Une destination
/// récente gagne un peu à score égal.
pub fn search(cands: &[Candidate], query: &str, recent: &[Target]) -> Vec<usize> {
    let (scope, text) = parse_query(query);
    let q = fold(text);
    let rank_of = |t: &Target| recent.iter().position(|r| r == t);
    let mut hits: Vec<(i32, usize)> = cands
        .iter()
        .enumerate()
        .filter(|(_, c)| scope.admits(c.kind))
        .filter_map(|(i, c)| {
            let s = score(&q, &c.folded)?;
            let bonus = rank_of(&c.target).map_or(0, |r| 500 - 10 * i32::try_from(r).unwrap_or(50));
            let base = if q.is_empty() && rank_of(&c.target).is_none() { -10_000 } else { s };
            Some((base + bonus, i))
        })
        .collect();
    hits.sort_by_key(|(s, i)| (std::cmp::Reverse(*s), *i));
    hits.truncate(MAX_RESULTS);
    hits.into_iter().map(|(_, i)| i).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_key_ignores_case_and_accents() {
        let mut names = vec!["Fanny", "émile", "Denis", "Élodie", "zoé", "Zoe"];
        names.sort_by_cached_key(|n| sort_key(n));
        assert_eq!(names, ["Denis", "Élodie", "émile", "Fanny", "Zoe", "zoé"]);
    }

    fn p(g: Option<u64>, c: u64) -> Place {
        Place { guild: g, channel: Some(c) }
    }

    #[test]
    fn history_goes_back_and_forth_and_forgets_forward_on_new_visit() {
        let mut h = History::default();
        h.observe(p(Some(1), 10));
        h.observe(p(Some(1), 11));
        h.observe(p(None, 50));
        assert_eq!(h.back(), Some(p(Some(1), 11)));
        h.observe(p(Some(1), 11));
        assert!(h.can_forward(), "revenir là où l'historique mène n'efface rien");
        assert_eq!(h.back(), Some(p(Some(1), 10)));
        assert_eq!(h.back(), None);
        assert_eq!(h.forward(), Some(p(Some(1), 11)));
        h.observe(p(Some(1), 11));
        h.observe(p(Some(2), 20));
        assert!(!h.can_forward(), "nouvelle visite : plus de « suivant »");
        let recent: Vec<Target> = h.recent().collect();
        assert_eq!(recent[0], Target::Channel { guild: Some(2), id: 20 });
        assert!(recent.contains(&Target::Guild(2)));
        for i in 0..200 {
            h.observe(p(Some(3), 100 + i));
        }
        assert_eq!(h.back.len(), MAX_HISTORY);
        assert_eq!(h.recent().count(), MAX_RECENT);
        h.retain(|pl| pl.guild != Some(3));
        assert!(h.back().is_none());
    }

    #[test]
    fn cycle_wraps() {
        assert_eq!(cycle(&[1, 2, 3], Some(3), true), Some(1));
        assert_eq!(cycle(&[1, 2, 3], Some(1), false), Some(3));
        assert_eq!(cycle(&[1, 2, 3], None, true), Some(1));
        assert_eq!(cycle(&[1, 2, 3], Some(9), false), Some(3));
        assert_eq!(cycle::<u8>(&[], None, true), None);
    }

    #[test]
    fn prefixes_and_fuzzy_scores() {
        assert_eq!(parse_query("#gen"), (Scope::Text, "gen"));
        assert_eq!(parse_query("  @ ali "), (Scope::People, "ali"));
        assert_eq!(parse_query("*rust"), (Scope::Guilds, "rust"));
        assert_eq!(parse_query("!salon"), (Scope::Voice, "salon"));
        assert_eq!(parse_query("gen"), (Scope::All, "gen"));
        let f = |q: &str, n: &str| score(&fold(q), &fold(n));
        assert!(f("gen", "Général") > f("gen", "hors-général"));
        assert!(f("hs", "hors-sujet").is_some());
        assert!(f("sujet", "hors-sujet") > f("hsj", "hors-sujet"));
        assert_eq!(f("xyz", "général"), None);
        assert_eq!(f("ga", "ag"), None, "lettres dans l'ordre");
    }

    #[test]
    fn search_filters_by_prefix_and_puts_recent_first() {
        let c = |t: Target, k: Kind, n: &str| Candidate::new(t, k, n, String::new());
        let cands = vec![
            c(Target::Channel { guild: Some(1), id: 10 }, Kind::Text, "général"),
            c(Target::Channel { guild: Some(1), id: 11 }, Kind::Text, "généalogie"),
            c(Target::Guild(1), Kind::Guild, "Gentils"),
            c(Target::Channel { guild: None, id: 50 }, Kind::Dm, "Genevieve"),
        ];
        let all = search(&cands, "gen", &[]);
        assert_eq!(all.len(), 4);
        assert_eq!(search(&cands, "#gen", &[]), vec![0, 1], "début du nom : le plus court d'abord");
        assert_eq!(search(&cands, "*", &[]).len(), 1);
        assert_eq!(search(&cands, "@gen", &[]), vec![3]);
        assert!(search(&cands, "!gen", &[]).is_empty());
        let recent = [Target::Channel { guild: Some(1), id: 10 }];
        assert_eq!(search(&cands, "", &recent)[0], 0, "récent en tête");
        let recent = [Target::Channel { guild: Some(1), id: 11 }];
        assert_eq!(search(&cands, "#gen", &recent), vec![1, 0], "à score proche, le récent gagne");
    }
}
