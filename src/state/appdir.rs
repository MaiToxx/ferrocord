//! Répertoire d'applications, en consultation (lot ND).
//!
//! Source : docs.discord.food, resources/application-directory : « List
//! Application Directory Categories »
//! (`GET /application-directory-static/categories`), « Search Applications
//! Directory » (`GET /application-directory-static/search`, réponse
//! `results[{ type, data }]`, `num_pages`), « Application Directory Entry
//! Object » (`short_description`, `guild_count`).
//!
//! Tout part d'un geste : ouvrir l'onglet, valider une recherche, choisir
//! une catégorie. Ajouter une application ne se fait pas ici : « Voir la
//! fiche » ouvre sa page dans le navigateur.

use super::discovery::{Category, QUERY_MAX};
use super::event::Cmd;
use super::usersettings::Remote;
use super::{Problem, Store};

/// Applications demandées par recherche (source : 100 au plus).
pub const PAGE: usize = 24;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct App {
    pub id: u64,
    pub name: String,
    pub summary: String,
    /// Adresse de l'icône sur le CDN, si l'application en a une.
    pub icon: Option<String>,
    /// Nombre approximatif de serveurs, s'il est annoncé.
    pub guilds: Option<u64>,
}

impl App {
    /// Fiche de l'application dans le répertoire de Discord.
    pub fn page_url(&self) -> String {
        format!("https://discord.com/application-directory/{}", self.id)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AppDirCmd {
    Categories { locale: &'static str },
    Search { seq: u32, query: String, category: Option<u32>, locale: &'static str },
}

#[derive(Debug)]
pub enum AppDirEvent {
    Categories(Result<Vec<Category>, Problem>),
    Results { seq: u32, result: Result<Vec<App>, Problem> },
}

#[derive(Default)]
pub struct AppDir {
    pub categories: Remote<Vec<Category>>,
    pub results: Remote<Vec<App>>,
    /// Ce que montre (ou attend) `results`.
    pub asked: Option<(String, Option<u32>)>,
    seq: u32,
}

impl AppDir {
    pub fn clear(&mut self) {
        let seq = self.seq;
        *self = AppDir { seq, ..AppDir::default() };
    }

    pub fn loading(&self) -> bool {
        matches!(self.results, Remote::Loading)
    }
}

impl Store {
    /// L'onglet s'ouvre : catégories (une fois par session) et première liste.
    pub fn appdir_open(&mut self, locale: &'static str) {
        if matches!(self.appdir.categories, Remote::Idle) {
            self.appdir.categories = Remote::Loading;
            self.push_command(Cmd::AppDir(AppDirCmd::Categories { locale }));
        }
        if matches!(self.appdir.results, Remote::Idle) {
            self.appdir_search("", None, locale);
        }
    }

    /// Recherche validée ou catégorie choisie. Une demande identique à celle
    /// qui est affichée ou en cours ne repart pas ; `true` si elle part.
    pub fn appdir_search(&mut self, query: &str, category: Option<u32>, locale: &'static str) -> bool {
        let query: String = query.trim().chars().filter(|c| !c.is_control()).take(QUERY_MAX).collect();
        let key = (query.clone(), category);
        if self.appdir.asked.as_ref() == Some(&key) && !matches!(self.appdir.results, Remote::Failed(_) | Remote::Idle) {
            return false;
        }
        self.appdir.seq = self.appdir.seq.wrapping_add(1);
        self.appdir.asked = Some(key);
        self.appdir.results = Remote::Loading;
        let seq = self.appdir.seq;
        self.push_command(Cmd::AppDir(AppDirCmd::Search { seq, query, category, locale }));
        true
    }

    pub(super) fn apply_appdir(&mut self, e: AppDirEvent) {
        match e {
            AppDirEvent::Categories(r) => self.appdir.categories = r.map_or_else(Remote::Failed, Remote::Ready),
            // Une réponse dépassée par une demande plus récente est ignorée.
            AppDirEvent::Results { seq, result } if seq == self.appdir.seq => self.appdir.results = result.map_or_else(Remote::Failed, Remote::Ready),
            AppDirEvent::Results { .. } => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    #[test]
    fn opening_asks_once_and_a_stale_answer_is_ignored() {
        let mut s = Store::new();
        s.appdir_open("fr");
        s.appdir_open("fr");
        assert_eq!(s.take_commands(), vec![Cmd::AppDir(AppDirCmd::Categories { locale: "fr" }), Cmd::AppDir(AppDirCmd::Search { seq: 1, query: String::new(), category: None, locale: "fr" })]);
        assert!(s.appdir_search(" musique ", Some(4), "fr"));
        assert!(!s.appdir_search("musique", Some(4), "fr"));
        s.take_commands();
        let app = App { id: 5, name: "Essai".into(), summary: String::new(), icon: None, guilds: Some(10) };
        s.apply(Event::AppDir(AppDirEvent::Results { seq: 1, result: Ok(Vec::new()) }));
        assert!(s.appdir.loading());
        s.apply(Event::AppDir(AppDirEvent::Results { seq: 2, result: Ok(vec![app.clone()]) }));
        assert_eq!(s.appdir.results, Remote::Ready(vec![app.clone()]));
        assert_eq!(app.page_url(), "https://discord.com/application-directory/5");
        s.apply(Event::AppDir(AppDirEvent::Results { seq: 2, result: Err(Problem::Forbidden) }));
        assert!(s.appdir_search("musique", Some(4), "fr"), "après un échec, un clic redemande");
    }
}
