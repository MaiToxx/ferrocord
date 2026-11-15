//! Découverte de serveurs, en consultation (lot ND).
//!
//! Source : docs.discord.food, resources/discovery : « List Discoverable
//! Guilds » (`GET /discoverable-guilds`), « Search Discoverable Guilds »
//! (`GET /discoverable-guilds/search`), « List Discovery Categories »
//! (`GET /discovery/categories`), « Discoverable Guild Object ».
//!
//! Tout part d'un geste : ouvrir la page (catégories et première liste),
//! valider une recherche, choisir une catégorie. Aucune requête par frappe.
//! Rejoindre ne se fait pas ici : un serveur qui a une adresse
//! personnalisée ouvre la fenêtre « Rejoindre un serveur » habituelle,
//! les autres ouvrent leur page dans le navigateur.

use super::event::Cmd;
use super::usersettings::Remote;
use super::{Problem, Store};

/// Longueur maximale d'une recherche (source : 100 caractères).
pub const QUERY_MAX: usize = 100;
/// Serveurs demandés par liste (source : 48 au plus).
pub const PAGE: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub id: u32,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub guild_id: u64,
    pub name: String,
    pub description: String,
    /// Adresse de l'icône sur le CDN, si le serveur en a une.
    pub icon: Option<String>,
    pub members: u64,
    pub online: u64,
    /// Adresse personnalisée (`discord.gg/<code>`), si le serveur en a une.
    pub vanity: Option<String>,
}

impl Found {
    /// Page publique du serveur (`discord.com/servers/<nom>-<id>`).
    pub fn page_url(&self) -> String {
        let mut slug = String::new();
        for c in self.name.chars().flat_map(char::to_lowercase) {
            if c.is_ascii_alphanumeric() {
                slug.push(c);
            } else if !slug.is_empty() && !slug.ends_with('-') {
                slug.push('-');
            }
        }
        let slug = slug.trim_end_matches('-');
        if slug.is_empty() { format!("https://discord.com/servers/{}", self.guild_id) } else { format!("https://discord.com/servers/{slug}-{}", self.guild_id) }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryCmd {
    Categories { locale: &'static str },
    /// `query` vide : la liste de Discord (serveurs vérifiés ou partenaires).
    Search { seq: u32, query: String, category: Option<u32> },
}

#[derive(Debug)]
pub enum DiscoveryEvent {
    Categories(Result<Vec<Category>, Problem>),
    Results { seq: u32, result: Result<Vec<Found>, Problem> },
}

#[derive(Default)]
pub struct Discovery {
    pub categories: Remote<Vec<Category>>,
    pub results: Remote<Vec<Found>>,
    /// Ce que montre (ou attend) `results`.
    pub asked: Option<(String, Option<u32>)>,
    seq: u32,
}

impl Discovery {
    pub fn clear(&mut self) {
        let seq = self.seq;
        *self = Discovery { seq, ..Discovery::default() };
    }

    pub fn loading(&self) -> bool {
        matches!(self.results, Remote::Loading)
    }
}

impl Store {
    /// La page s'ouvre : catégories (une fois par session) et première liste.
    pub fn discovery_open(&mut self, locale: &'static str) {
        if matches!(self.discovery.categories, Remote::Idle) {
            self.discovery.categories = Remote::Loading;
            self.push_command(Cmd::Discovery(DiscoveryCmd::Categories { locale }));
        }
        if matches!(self.discovery.results, Remote::Idle) {
            self.discovery_search("", None);
        }
    }

    /// Recherche validée ou catégorie choisie. Une demande identique à celle
    /// qui est affichée ou en cours ne repart pas ; `true` si elle part.
    pub fn discovery_search(&mut self, query: &str, category: Option<u32>) -> bool {
        let query: String = query.trim().chars().filter(|c| !c.is_control()).take(QUERY_MAX).collect();
        let key = (query.clone(), category);
        if self.discovery.asked.as_ref() == Some(&key) && !matches!(self.discovery.results, Remote::Failed(_) | Remote::Idle) {
            return false;
        }
        self.discovery.seq = self.discovery.seq.wrapping_add(1);
        self.discovery.asked = Some(key);
        self.discovery.results = Remote::Loading;
        let seq = self.discovery.seq;
        self.push_command(Cmd::Discovery(DiscoveryCmd::Search { seq, query, category }));
        true
    }

    pub(super) fn apply_discovery(&mut self, e: DiscoveryEvent) {
        match e {
            DiscoveryEvent::Categories(r) => self.discovery.categories = r.map_or_else(Remote::Failed, Remote::Ready),
            // Une réponse dépassée par une demande plus récente est ignorée.
            DiscoveryEvent::Results { seq, result } if seq == self.discovery.seq => {
                // Les serveurs déjà rejoints restent listés : l'interface les marque.
                self.discovery.results = result.map_or_else(Remote::Failed, Remote::Ready);
            }
            DiscoveryEvent::Results { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    fn found(id: u64, name: &str) -> Found {
        Found { guild_id: id, name: name.into(), description: String::new(), icon: None, members: 300, online: 20, vanity: None }
    }

    #[test]
    fn opening_asks_once_and_a_stale_answer_is_ignored() {
        let mut s = Store::new();
        s.discovery_open("fr");
        s.discovery_open("fr");
        assert_eq!(s.take_commands(), vec![Cmd::Discovery(DiscoveryCmd::Categories { locale: "fr" }), Cmd::Discovery(DiscoveryCmd::Search { seq: 1, query: String::new(), category: None })]);
        assert!(s.discovery_search("  jardin\n ", Some(3)));
        assert!(!s.discovery_search("jardin", Some(3)), "même demande : rien ne repart");
        assert_eq!(s.take_commands(), vec![Cmd::Discovery(DiscoveryCmd::Search { seq: 2, query: "jardin".into(), category: Some(3) })]);
        s.apply(Event::Discovery(DiscoveryEvent::Results { seq: 1, result: Ok(vec![found(1, "Ancien")]) }));
        assert!(s.discovery.loading());
        s.apply(Event::Discovery(DiscoveryEvent::Results { seq: 2, result: Ok(vec![found(2, "Jardin")]) }));
        assert_eq!(s.discovery.results, Remote::Ready(vec![found(2, "Jardin")]));
        assert!(!s.discovery_search("jardin", Some(3)));
        // Après un échec, la même demande peut être refaite par un clic.
        s.apply(Event::Discovery(DiscoveryEvent::Results { seq: 2, result: Err(Problem::Forbidden) }));
        assert!(s.discovery_search("jardin", Some(3)));
        let long = "x".repeat(300);
        s.discovery_search(&long, None);
        assert_eq!(s.discovery.asked.as_ref().map(|a| a.0.len()), Some(QUERY_MAX));
    }

    #[test]
    fn page_address_is_built_from_name_and_id() {
        assert_eq!(found(42, "Le Jardin — Fictif !").page_url(), "https://discord.com/servers/le-jardin-fictif-42");
        assert_eq!(found(42, "日本").page_url(), "https://discord.com/servers/42");
    }
}
