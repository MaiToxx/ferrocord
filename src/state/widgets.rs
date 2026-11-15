//! Widgets de jeux du profil (« Tableau » et « Liste de souhaits » de
//! l'éditeur de profil).
//!
//! Source : docs.discord.food, resources/widgets (lu le 15 novembre 2026) :
//! « Game Widget Object » (`id`, `updated_at`, `data` : `type`, `games`,
//! `application_id`), « Game Widget Types » (`favorite_games`, 1 jeu ;
//! `played_games`, 20 ; `current_games`, 5 ; `want_to_play_games`, 20 ;
//! `application`), « Game Widget Game Structure » (`game_id`, `comment`,
//! `tags`) ; `PUT /users/@me/widgets` (remplace les widgets, un par type) ;
//! `GET /users/@me/widgets/suggested-games`. Lecture : champ `widgets` de
//! `GET /users/{id}/profile` (resources/user). Noms et images des jeux :
//! resources/game, « Get Games » (`GET /games?game_ids=`, 25 au plus).
//!
//! La source ne décrit aucune recherche de jeu par nom : l'ajout propose
//! les suggestions de Discord et, si elle est chargée, la liste des jeux
//! détectables (`state::activity`), filtrée sur place.
//!
//! Rien ne part sans geste : ouverture de l'éditeur (noms des jeux),
//! ouverture du choix d'un jeu (suggestions), « Enregistrer » (écriture).

use super::event::Cmd;
use super::model::ImageHash;
use super::quests::Refused;
use super::usersettings::Remote;
use super::{Problem, Store};
use std::collections::HashMap;

/// Jeux demandés par requête (`game_ids`, source : 1 à 25).
pub const GAMES_PER_REQUEST: usize = 25;
/// Jeux dont le nom est gardé pour la session.
pub const MAX_GAMES: usize = 512;
/// Commentaire d'un jeu : borne locale (la source n'en donne pas).
pub const MAX_COMMENT: usize = 140;
/// Étiquettes gardées par jeu (renvoyées telles quelles).
pub const MAX_TAGS: usize = 16;

/// Type d'un widget (« Game Widget Types »).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum WidgetKind {
    /// « Jeu favori » : un jeu, avec un commentaire.
    Favorite,
    /// « Jeux que j'aime ».
    Played,
    /// « Jeux du moment ».
    Current,
    /// Liste de souhaits.
    Wanted,
    /// Détails d'un jeu précis : gardé et renvoyé tel quel, pas affiché.
    Application,
}

impl WidgetKind {
    pub const BOARD: [WidgetKind; 3] = [WidgetKind::Current, WidgetKind::Favorite, WidgetKind::Played];

    pub fn code(self) -> &'static str {
        match self {
            WidgetKind::Favorite => "favorite_games",
            WidgetKind::Played => "played_games",
            WidgetKind::Current => "current_games",
            WidgetKind::Wanted => "want_to_play_games",
            WidgetKind::Application => "application",
        }
    }

    pub fn parse(s: &str) -> Option<WidgetKind> {
        [WidgetKind::Favorite, WidgetKind::Played, WidgetKind::Current, WidgetKind::Wanted, WidgetKind::Application].into_iter().find(|k| k.code() == s)
    }

    /// Nombre de jeux accepté par la source.
    pub fn max(self) -> usize {
        match self {
            WidgetKind::Favorite => 1,
            WidgetKind::Current => 5,
            WidgetKind::Played | WidgetKind::Wanted => 20,
            WidgetKind::Application => 0,
        }
    }
}

/// Jeu d'un widget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WidgetGame {
    pub game_id: u64,
    pub comment: Option<String>,
    /// Étiquettes (« Game Widget Tags ») : gardées et renvoyées, pas modifiées ici.
    pub tags: Vec<String>,
}

impl WidgetGame {
    pub fn new(game_id: u64) -> WidgetGame {
        WidgetGame { game_id, comment: None, tags: Vec::new() }
    }
}

/// Widget de jeux d'un profil.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameWidget {
    /// Absent pour un widget créé ici (la source : facultatif à l'écriture).
    pub id: Option<u64>,
    pub kind: WidgetKind,
    pub games: Vec<WidgetGame>,
    /// Widget `application` seulement.
    pub application_id: Option<u64>,
}

impl GameWidget {
    pub fn new(kind: WidgetKind) -> GameWidget {
        GameWidget { id: None, kind, games: Vec::new(), application_id: None }
    }
}

/// Ce que l'on sait d'un jeu (« Game Object » : `name`, `icon_hash`, `cover_image_hash`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    pub name: String,
    pub icon: Option<ImageHash>,
    pub cover: Option<ImageHash>,
}

impl Game {
    /// Image à montrer : la couverture, sinon l'icône.
    pub fn image(&self) -> Option<ImageHash> {
        self.cover.or(self.icon)
    }
}

/// Jeux suggérés par Discord pour mes widgets.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Suggested {
    pub games: Vec<u64>,
    pub wishlist: Vec<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WidgetCmd {
    /// `GET /games?game_ids=…` (25 au plus).
    Games { ids: Vec<u64> },
    /// `GET /users/@me/widgets/suggested-games`.
    Suggested,
    /// `PUT /users/@me/widgets`.
    Save { widgets: Vec<GameWidget> },
}

#[derive(Debug)]
pub enum WidgetEvent {
    Games { ids: Vec<u64>, result: Result<Vec<(u64, Game)>, Problem> },
    Suggested(Result<Suggested, Problem>),
    /// Widgets tels que Discord les a gardés.
    Saved(Result<Vec<GameWidget>, Refused>),
}

#[derive(Default)]
pub struct Widgets {
    /// `None` : demandé, pas (encore) connu.
    games: HashMap<u64, Option<Game>>,
    pub suggested: Remote<Suggested>,
    saving: bool,
    refused: Option<Refused>,
}

impl Widgets {
    pub fn clear(&mut self) {
        *self = Widgets::default();
    }

    pub fn game(&self, id: u64) -> Option<&Game> {
        self.games.get(&id)?.as_ref()
    }

    pub fn saving(&self) -> bool {
        self.saving
    }

    /// Refus de la dernière écriture ; lu une fois.
    pub fn take_refusal(&mut self) -> Option<Refused> {
        self.refused.take()
    }
}

impl Store {
    /// Noms et images de ces jeux, demandés une fois par session chacun.
    pub fn want_games(&mut self, ids: &[u64]) {
        let mut missing: Vec<u64> = Vec::new();
        for id in ids {
            if self.widgets.games.len() + missing.len() < MAX_GAMES && !self.widgets.games.contains_key(id) && !missing.contains(id) {
                missing.push(*id);
            }
        }
        for chunk in missing.chunks(GAMES_PER_REQUEST) {
            for id in chunk {
                self.widgets.games.insert(*id, None);
            }
            self.push_command(Cmd::Widgets(WidgetCmd::Games { ids: chunk.to_vec() }));
        }
    }

    /// Ouverture de l'éditeur : les jeux restés sans réponse pourront être redemandés.
    pub fn retry_games(&mut self) {
        self.widgets.games.retain(|_, g| g.is_some());
    }

    /// Suggestions de Discord, demandées une fois (de nouveau après un échec).
    pub fn want_suggested_games(&mut self) {
        if matches!(self.widgets.suggested, Remote::Idle | Remote::Failed(_)) {
            self.widgets.suggested = Remote::Loading;
            self.push_command(Cmd::Widgets(WidgetCmd::Suggested));
        }
    }

    /// « Enregistrer » : mes widgets remplacés par `widgets` (un par type,
    /// bornés comme le veut la source). Faux si une écriture est en cours.
    pub fn save_widgets(&mut self, mut widgets: Vec<GameWidget>) -> bool {
        if self.widgets.saving {
            return false;
        }
        let mut seen: Vec<WidgetKind> = Vec::new();
        widgets.retain(|w| {
            let first = !seen.contains(&w.kind);
            seen.push(w.kind);
            first
        });
        for w in &mut widgets {
            w.games.truncate(w.kind.max());
        }
        self.widgets.saving = true;
        self.widgets.refused = None;
        self.push_command(Cmd::Widgets(WidgetCmd::Save { widgets }));
        true
    }

    pub(super) fn apply_widgets(&mut self, e: WidgetEvent) {
        match e {
            WidgetEvent::Games { result: Ok(list), .. } => {
                // Un jeu absent de la réponse reste « demandé » : il n'est pas redemandé.
                for (id, game) in list {
                    if let Some(slot) = self.widgets.games.get_mut(&id) {
                        *slot = Some(game);
                    }
                }
            }
            // Échec : ils restent « demandés » jusqu'à la prochaine ouverture de l'éditeur (`retry_games`).
            WidgetEvent::Games { result: Err(_), .. } => {}
            WidgetEvent::Suggested(Ok(s)) => self.widgets.suggested = Remote::Ready(s),
            WidgetEvent::Suggested(Err(p)) => self.widgets.suggested = Remote::Failed(p),
            WidgetEvent::Saved(result) => {
                self.widgets.saving = false;
                match result {
                    Ok(list) => {
                        let Some(me) = self.me.as_ref().map(|m| m.id) else { return };
                        let shown = self.guild;
                        let mut people = self.people.lock();
                        let tables = people.profiles_mut();
                        tables.set_widgets(me, None, &list);
                        if shown.is_some() {
                            tables.set_widgets(me, shown, &list);
                        }
                    }
                    Err(r) => self.widgets.refused = Some(r),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::User;
    use crate::state::profiles::{Profile, ProfileState};
    use crate::state::Event;

    fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "moi".into()), guilds: Vec::new(), dms: Vec::new() });
        s.people.lock().profiles_mut().set(1, None, ProfileState::Ready(Box::default()));
        s.take_commands();
        s
    }

    fn game(name: &str) -> Game {
        Game { name: name.into(), icon: None, cover: None }
    }

    /// Un nom n'est demandé qu'une fois ; 25 jeux par requête ; un échec est oublié.
    #[test]
    fn games_are_asked_once_in_batches() {
        let mut s = store();
        let ids: Vec<u64> = (1..=30).collect();
        s.want_games(&ids);
        s.want_games(&ids);
        let cmds = s.take_commands();
        assert_eq!(cmds, vec![Cmd::Widgets(WidgetCmd::Games { ids: (1..=25).collect() }), Cmd::Widgets(WidgetCmd::Games { ids: (26..=30).collect() })]);
        s.apply(Event::Widgets(WidgetEvent::Games { ids: (1..=25).collect(), result: Ok(vec![(1, game("Un"))]) }));
        assert_eq!(s.widgets.game(1).map(|g| g.name.as_str()), Some("Un"));
        s.want_games(&[1, 2]);
        assert!(s.take_commands().is_empty(), "jeu absent de la réponse : pas redemandé");
        s.apply(Event::Widgets(WidgetEvent::Games { ids: (26..=30).collect(), result: Err(Problem::Api(String::new())) }));
        s.want_games(&[26]);
        assert!(s.take_commands().is_empty(), "échec : pas de nouvel essai tant que l'éditeur reste ouvert");
        s.retry_games();
        s.want_games(&[1, 26]);
        assert_eq!(s.take_commands(), vec![Cmd::Widgets(WidgetCmd::Games { ids: vec![26] })]);
    }

    /// Une écriture à la fois, un widget par type, bornes de la source ; le
    /// profil gardé suit la réponse, un refus est rendu une fois.
    #[test]
    fn saving_is_bounded_and_updates_my_profile() {
        let mut s = store();
        let mut current = GameWidget::new(WidgetKind::Current);
        current.games = (1..=9).map(WidgetGame::new).collect();
        let twice = GameWidget::new(WidgetKind::Current);
        assert!(s.save_widgets(vec![current.clone(), twice]));
        assert!(!s.save_widgets(Vec::new()), "une écriture à la fois");
        let cmds = s.take_commands();
        let [Cmd::Widgets(WidgetCmd::Save { widgets })] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!((widgets.len(), widgets[0].games.len()), (1, 5));
        let kept = widgets.clone();
        s.apply(Event::Widgets(WidgetEvent::Saved(Ok(kept.clone()))));
        assert!(!s.widgets.saving());
        let got = s.people.lock().profiles().profile(1, None).and_then(ProfileState::profile).map(|p: &Profile| p.widgets.clone());
        assert_eq!(got, Some(kept));
        assert!(s.save_widgets(Vec::new()));
        s.apply(Event::Widgets(WidgetEvent::Saved(Err(Refused::Captcha))));
        assert_eq!((s.widgets.take_refusal(), s.widgets.take_refusal()), (Some(Refused::Captcha), None));
    }

    #[test]
    fn kinds_round_trip_and_suggestions_are_asked_once() {
        for k in [WidgetKind::Favorite, WidgetKind::Played, WidgetKind::Current, WidgetKind::Wanted, WidgetKind::Application] {
            assert_eq!(WidgetKind::parse(k.code()), Some(k));
        }
        assert_eq!(WidgetKind::parse("inconnu"), None);
        let mut s = store();
        s.want_suggested_games();
        s.want_suggested_games();
        assert_eq!(s.take_commands(), vec![Cmd::Widgets(WidgetCmd::Suggested)]);
        s.apply(Event::Widgets(WidgetEvent::Suggested(Err(Problem::Forbidden))));
        s.want_suggested_games();
        assert_eq!(s.take_commands(), vec![Cmd::Widgets(WidgetCmd::Suggested)], "redemandé après un échec");
    }
}
