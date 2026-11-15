//! Sélecteur de GIF (lot G) : catégories en tendance, GIF en tendance,
//! recherche, favoris, et l'envoi d'une super réaction.
//!
//! Points d'API `/gifs/*` : réservés aux comptes utilisateur et non
//! documentés par Discord ; forme vérifiée sur docs.discord.food, page
//! « Integration », sections « List Trending GIF Categories », « List
//! Trending GIFs », « Search GIFs » et « Track Selected GIF » (détail dans
//! `net/gifs.rs` et `docs/passerelle-utilisateur.md`).
//!
//! Rien ne part sans un geste : ouverture de l'onglet GIF, clic sur une
//! tuile, envoi d'un GIF, et la frappe dans la recherche. La recherche à la
//! frappe attend [`SEARCH_DELAY`] sans nouvelle touche avant de partir : une
//! requête par mot tapé, pas par lettre.

use super::event::Cmd;
use super::frecency::{FavGif, Frecency, Write};
use super::notice::{Notice, Problem};
use super::social::Emoji;
use super::store::{Effect, Store};
use super::usersettings::{ProtoReply, Refusal, Remote};
use std::collections::HashSet;
use std::time::{Duration, Instant};

/// Silence du clavier attendu avant qu'une recherche parte.
pub const SEARCH_DELAY: Duration = Duration::from_millis(400);
/// Format demandé : GIF d'environ 220 px de large, lu par le décodeur d'images
/// (les formats vidéo du client officiel demanderaient un lecteur).
pub const MEDIA_FORMAT: &str = "tinygif";
/// Résultats demandés (20 à 500 d'après la source).
pub const LIMIT: u16 = 50;
/// Résultats gardés au plus.
pub const MAX_KEPT: usize = 200;
/// Longueur maximale d'une recherche (caractères).
pub const MAX_QUERY: usize = 100;

/// Un GIF de la réponse. `url` : la page du GIF, envoyée telle quelle dans
/// le message (Discord en fait l'aperçu) ; `src` : le média au format demandé.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gif {
    pub id: String,
    pub url: String,
    pub src: String,
    pub gif_src: String,
    pub width: u32,
    pub height: u32,
}

impl Gif {
    /// Ce GIF en favori (média au format GIF, lisible par tous les clients).
    pub fn favorite(&self) -> FavGif {
        let src = if self.gif_src.is_empty() { self.src.clone() } else { self.gif_src.clone() };
        FavGif { url: self.url.clone(), src, width: self.width, height: self.height, order: 0, video: false }
    }
}

/// Catégorie en tendance : un mot à chercher et son image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub name: String,
    pub src: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GifCmd {
    /// Catégories en tendance (ouverture de l'onglet).
    Categories { locale: &'static str },
    /// GIF en tendance (tuile « Tendances »).
    Trending { round: u64, locale: &'static str },
    Search { round: u64, query: String, locale: &'static str },
    /// GIF envoyé : signalé à Discord, avec la recherche qui l'a trouvé.
    Select { id: String, query: String },
    LoadFrecency,
    PatchFrecency { id: u64, body: Vec<u8>, required_version: Option<u32> },
    /// Super réaction : ajoutée (`add`) ou retirée.
    SuperReact { channel_id: u64, message_id: u64, emoji: Emoji, add: bool },
}

#[derive(Debug)]
pub enum GifEvent {
    /// Catégories, et le média d'un GIF du moment (image de la tuile « Tendances »).
    Categories(Result<(Vec<Category>, Option<String>), Problem>),
    Results { round: u64, gifs: Result<Vec<Gif>, Problem> },
    Frecency { bytes: Option<Vec<u8>>, partial: bool },
    FrecencyLoadFailed(Problem),
    FrecencyWritten { id: u64, result: Result<ProtoReply, Problem> },
    SuperReactDone { message_id: u64, emoji: Emoji, error: Option<Problem> },
}

/// Ce que montre l'onglet GIF.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum View {
    /// Tuiles : favoris, tendances, catégories.
    #[default]
    Home,
    Favorites,
    Trending,
    /// Résultats de cette recherche.
    Search(String),
}

#[derive(Default)]
pub struct Gifs {
    pub categories: Remote<Vec<Category>>,
    /// Média d'un GIF du moment, pour la tuile « Tendances ».
    pub trending_src: Option<String>,
    pub view: View,
    pub results: Vec<Gif>,
    pub loading: bool,
    pub error: Option<Problem>,
    /// Numéro de la liste demandée : une réponse d'une liste remplacée est ignorée.
    round: u64,
    /// Recherche tapée, pas encore partie, et l'heure de la dernière touche.
    typed: Option<(String, Instant)>,
    pub frecency: Frecency,
    /// Super réactions en cours (une à la fois par emoji d'un message).
    bursts: HashSet<(u64, Emoji)>,
}

impl Gifs {
    pub fn clear(&mut self) {
        // Le dossier de l'usage local des emojis vaut pour tous les comptes.
        let dir = self.frecency.local.dir.take();
        *self = Gifs { round: self.round + 1, ..Default::default() };
        self.frecency.local.dir = dir;
    }

    fn list(&mut self, view: View) -> u64 {
        self.round += 1;
        self.view = view;
        self.results.clear();
        self.error = None;
        self.typed = None;
        self.round
    }
}

/// Recherche nettoyée : espaces du bord retirés, longueur bornée.
fn cleaned(query: &str) -> String {
    query.trim().chars().take(MAX_QUERY).collect()
}

impl Store {
    /// Ouverture d'un sélecteur d'expressions : favoris et fréquence du
    /// compte, lus une seule fois.
    pub fn expressions_open(&mut self) {
        if self.gifs.frecency.open() {
            self.push_command(Cmd::Gifs(GifCmd::LoadFrecency));
        }
    }

    /// Ouverture de l'onglet GIF : les catégories, si elles ne sont pas là.
    pub fn gifs_open(&mut self, locale: &'static str) {
        self.expressions_open();
        if matches!(self.gifs.categories, Remote::Idle | Remote::Failed(_)) {
            self.gifs.categories = Remote::Loading;
            self.push_command(Cmd::Gifs(GifCmd::Categories { locale }));
        }
    }

    /// Retour aux tuiles.
    pub fn gifs_home(&mut self) {
        self.gifs.list(View::Home);
        self.gifs.loading = false;
    }

    pub fn gifs_favorites(&mut self) {
        self.gifs.list(View::Favorites);
        self.gifs.loading = false;
    }

    /// Tuile « Tendances ».
    pub fn gifs_trending(&mut self, locale: &'static str) {
        let round = self.gifs.list(View::Trending);
        self.gifs.loading = true;
        self.push_command(Cmd::Gifs(GifCmd::Trending { round, locale }));
    }

    /// Recherche immédiate (clic sur une catégorie, Entrée) ; vide : les tuiles.
    pub fn gifs_search(&mut self, query: &str, locale: &'static str) {
        let query = cleaned(query);
        if query.is_empty() {
            return self.gifs_home();
        }
        if self.gifs.view == View::Search(query.clone()) && (self.gifs.loading || self.gifs.error.is_none()) {
            self.gifs.typed = None;
            return;
        }
        let round = self.gifs.list(View::Search(query.clone()));
        self.gifs.loading = true;
        self.push_command(Cmd::Gifs(GifCmd::Search { round, query, locale }));
    }

    /// Le texte de la recherche a changé à `now` : elle partira après
    /// [`SEARCH_DELAY`] sans autre touche ([`Store::gifs_tick`]). Vidée : les
    /// tuiles reviennent tout de suite, sans requête.
    pub fn gifs_typed(&mut self, query: &str, now: Instant) {
        let query = cleaned(query);
        if query.is_empty() {
            if matches!(self.gifs.view, View::Search(_)) || self.gifs.typed.is_some() {
                self.gifs_home();
            }
            return;
        }
        self.gifs.typed = Some((query, now));
    }

    /// À chaque image tant que l'onglet GIF est ouvert : envoie la recherche
    /// tapée si le délai est passé ; sinon renvoie le temps qu'il reste.
    pub fn gifs_tick(&mut self, now: Instant, locale: &'static str) -> Option<Duration> {
        let (query, at) = self.gifs.typed.as_ref()?;
        let waited = now.saturating_duration_since(*at);
        if waited < SEARCH_DELAY {
            return Some(SEARCH_DELAY - waited);
        }
        let query = query.clone();
        self.gifs_search(&query, locale);
        None
    }

    /// Un GIF de la liste vient d'être envoyé : le choix est signalé à
    /// Discord avec la recherche qui l'a trouvé. Un favori n'a pas
    /// d'identifiant : rien à signaler.
    pub fn gifs_selected(&mut self, id: &str) {
        if id.is_empty() {
            return;
        }
        let query = match &self.gifs.view {
            View::Search(q) => q.clone(),
            _ => String::new(),
        };
        self.push_command(Cmd::Gifs(GifCmd::Select { id: id.to_string(), query }));
    }

    fn frecency_write(&mut self, w: Result<Write, Refusal>) -> Result<(), Refusal> {
        let Write { id, body, required_version } = w?;
        self.push_command(Cmd::Gifs(GifCmd::PatchFrecency { id, body, required_version }));
        Ok(())
    }

    /// Étoile d'un GIF : ajouté aux favoris du compte, ou retiré.
    pub fn toggle_fav_gif(&mut self, gif: &FavGif) -> Result<(), Refusal> {
        let w = self.gifs.frecency.toggle_gif(gif);
        self.frecency_write(w)
    }

    /// Emoji ajouté aux favoris du compte, ou retiré (`key` : [`emoji_key`]).
    pub fn toggle_fav_emoji(&mut self, key: &str) -> Result<(), Refusal> {
        let w = self.gifs.frecency.toggle_emoji(key);
        self.frecency_write(w)
    }

    /// Super réaction ajoutée à un message, ou la mienne retirée. Le compte
    /// suit l'écho de la passerelle (MESSAGE_REACTION_ADD, `burst`).
    pub fn super_react(&mut self, channel_id: u64, message_id: u64, emoji: Emoji) -> bool {
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        let existing = m.meta.reactions.iter().find(|r| r.emoji.same(&emoji));
        let add = !existing.is_some_and(|r| r.me_burst);
        let rights = self.rights(channel_id);
        let allowed = if existing.is_some() { rights.react_existing } else { rights.react };
        if !allowed || !self.gifs.bursts.insert((message_id, emoji.clone())) {
            return false;
        }
        self.push_command(Cmd::Gifs(GifCmd::SuperReact { channel_id, message_id, emoji, add }));
        true
    }

    pub(super) fn apply_gifs(&mut self, e: GifEvent) -> Option<Effect> {
        let g = &mut self.gifs;
        match e {
            GifEvent::Categories(Ok((list, preview))) => {
                g.categories = Remote::Ready(list);
                g.trending_src = preview;
            }
            GifEvent::Categories(Err(p)) => g.categories = Remote::Failed(p),
            GifEvent::Results { round, gifs } => {
                if round != g.round {
                    return None;
                }
                g.loading = false;
                match gifs {
                    Ok(mut list) => {
                        list.truncate(MAX_KEPT);
                        g.results = list;
                    }
                    Err(p) => g.error = Some(p),
                }
            }
            GifEvent::Frecency { bytes, partial } => g.frecency.received(bytes, partial),
            GifEvent::FrecencyLoadFailed(_) => g.frecency.load_failed(),
            GifEvent::FrecencyWritten { id, result } => return g.frecency.written(id, result).map(Effect::Toast),
            GifEvent::SuperReactDone { message_id, emoji, error } => {
                g.bursts.retain(|(m, e)| !(*m == message_id && e.same(&emoji)));
                return error.map(|p| Effect::Toast(Notice::Error(p)));
            }
        }
        None
    }
}

/// Clé d'un emoji dans les favoris et la fréquence du compte : identifiant
/// d'un emoji personnalisé, nom court d'un emoji Unicode.
pub fn emoji_key(e: &Emoji) -> Option<String> {
    match e {
        Emoji::Custom { id, .. } => Some(id.to_string()),
        Emoji::Unicode(s) => super::emoji::shortcode(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    fn gif(n: u32) -> Gif {
        Gif { id: n.to_string(), url: format!("https://tenor.com/view/essai-{n}"), src: format!("https://media.tenor.com/e{n}AAAAM/essai.gif"), gif_src: format!("https://media.tenor.com/e{n}AAAAC/essai.gif"), width: 220, height: 124 }
    }

    fn cmds(s: &mut Store) -> Vec<GifCmd> {
        s.take_commands().into_iter().filter_map(|c| if let Cmd::Gifs(g) = c { Some(g) } else { None }).collect()
    }

    #[test]
    fn opening_loads_categories_and_frecency_once() {
        let mut s = Store::default();
        s.gifs_open("fr");
        s.gifs_open("fr");
        assert_eq!(cmds(&mut s), [GifCmd::LoadFrecency, GifCmd::Categories { locale: "fr" }]);
        s.apply(Event::Gifs(GifEvent::Categories(Err(Problem::Forbidden))));
        s.gifs_open("fr");
        assert_eq!(cmds(&mut s), [GifCmd::Categories { locale: "fr" }], "après un échec, redemandées à la réouverture");
    }

    #[test]
    fn typing_waits_for_the_delay_and_sends_one_search() {
        let mut s = Store::default();
        let t0 = Instant::now();
        s.gifs_typed("c", t0);
        s.gifs_typed("ch", t0 + Duration::from_millis(100));
        s.gifs_typed("  chat  ", t0 + Duration::from_millis(200));
        assert_eq!(s.gifs_tick(t0 + Duration::from_millis(300), "fr"), Some(SEARCH_DELAY - Duration::from_millis(100)));
        assert!(cmds(&mut s).is_empty(), "rien pendant la frappe");
        assert_eq!(s.gifs_tick(t0 + Duration::from_millis(200) + SEARCH_DELAY, "fr"), None);
        assert_eq!(s.gifs_tick(t0 + Duration::from_secs(5), "fr"), None);
        assert_eq!(cmds(&mut s), [GifCmd::Search { round: 1, query: "chat".into(), locale: "fr" }]);
        assert!(s.gifs.loading);
        // Même mot retapé pendant le chargement : pas de deuxième requête.
        s.gifs_typed("chat", t0 + Duration::from_secs(6));
        assert_eq!(s.gifs_tick(t0 + Duration::from_secs(7), "fr"), None);
        assert!(cmds(&mut s).is_empty());
        // Champ vidé : les tuiles reviennent sans requête, la réponse en route est ignorée.
        s.gifs_typed("", t0 + Duration::from_secs(8));
        assert_eq!(s.gifs.view, View::Home);
        s.apply(Event::Gifs(GifEvent::Results { round: 1, gifs: Ok(vec![gif(1)]) }));
        assert!(s.gifs.results.is_empty() && cmds(&mut s).is_empty());
    }

    #[test]
    fn results_follow_the_latest_list_only() {
        let mut s = Store::default();
        s.gifs_search("chat", "en-US");
        s.gifs_trending("en-US");
        assert_eq!(cmds(&mut s), [GifCmd::Search { round: 1, query: "chat".into(), locale: "en-US" }, GifCmd::Trending { round: 2, locale: "en-US" }]);
        s.apply(Event::Gifs(GifEvent::Results { round: 1, gifs: Ok(vec![gif(1)]) }));
        assert!(s.gifs.results.is_empty() && s.gifs.loading);
        s.apply(Event::Gifs(GifEvent::Results { round: 2, gifs: Ok((0..300).map(gif).collect()) }));
        assert_eq!(s.gifs.results.len(), MAX_KEPT);
        assert!(!s.gifs.loading);
        s.gifs_search("chien", "en-US");
        s.apply(Event::Gifs(GifEvent::Results { round: 3, gifs: Err(Problem::TooManyRequests) }));
        assert_eq!(s.gifs.error, Some(Problem::TooManyRequests));
        // Après une erreur, la même recherche peut repartir.
        s.take_commands();
        s.gifs_search("chien", "en-US");
        assert_eq!(cmds(&mut s).len(), 1);
    }

    #[test]
    fn favorites_are_written_through_the_account_settings() {
        let mut s = Store::default();
        assert_eq!(s.toggle_fav_gif(&gif(1).favorite()), Err(Refusal::Unknown), "réglages pas encore lus");
        s.expressions_open();
        s.apply(Event::Gifs(GifEvent::Frecency { bytes: Some(crate::state::frecency::synthetic(&[], &[], &[], 3)), partial: false }));
        s.take_commands();
        assert_eq!(s.toggle_fav_gif(&gif(1).favorite()), Ok(()));
        let sent = cmds(&mut s);
        let [GifCmd::PatchFrecency { id, required_version: Some(3), .. }] = &sent[..] else { panic!("{sent:?}") };
        assert!(s.gifs.frecency.is_fav_gif("https://tenor.com/view/essai-1"));
        assert_eq!(s.gifs.frecency.gifs[0].src, "https://media.tenor.com/e1AAAAC/essai.gif", "média au format GIF");
        let effect = s.apply(Event::Gifs(GifEvent::FrecencyWritten { id: *id, result: Err(Problem::Forbidden) }));
        assert!(matches!(effect, Some(Effect::Toast(Notice::SettingNotSaved(_)))));
        assert!(s.gifs.frecency.gifs.is_empty(), "refusé : rétabli");
        assert_eq!(emoji_key(&Emoji::Unicode("🔥".into())).as_deref(), Some("fire"));
        assert_eq!(emoji_key(&Emoji::Custom { id: 7, name: "x".into(), animated: false }).as_deref(), Some("7"));
    }
}
