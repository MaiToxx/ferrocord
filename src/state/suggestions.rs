//! Suggestions d'amis (lot ND).
//!
//! Source : docs.discord.food, resources/relationships, « List Friend
//! Suggestions » (`GET /friend-suggestions`), « Remove Friend Suggestion »
//! (`DELETE /friend-suggestions/{user.id}`), « Friend Suggestion Object ».
//!
//! La liste est demandée une fois par session, quand l'onglet « En
//! attente » de la page Amis est ouvert. « Ajouter » envoie la demande
//! d'ami habituelle (`PUT /users/@me/relationships/{id}`) ; « Ignorer »
//! retire la suggestion. Rien n'est envoyé à propos des contacts du compte.

use super::event::Cmd;
use super::model::Avatar;
use super::relations::RelationsCmd;
use super::usersettings::Remote;
use super::{Problem, Store};

pub const MAX_SUGGESTIONS: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Suggestion {
    pub user_id: u64,
    /// Nom affiché (nom global, sinon nom d'utilisateur).
    pub name: String,
    pub username: String,
    pub avatar: Avatar,
    /// D'où vient la suggestion : plateforme et nom sur cette plateforme.
    pub reason: Option<(String, String)>,
    pub mutual_friends: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SuggestionCmd {
    List,
    Dismiss { user_id: u64 },
}

#[derive(Debug)]
pub enum SuggestionEvent {
    List(Result<Vec<Suggestion>, Problem>),
}

#[derive(Default)]
pub struct Suggestions {
    pub list: Remote<Vec<Suggestion>>,
}

impl Suggestions {
    pub fn clear(&mut self) {
        *self = Suggestions::default();
    }
}

impl Store {
    /// L'onglet « En attente » est ouvert : les suggestions sont demandées
    /// une fois par session (un échec n'est pas retenté tout seul).
    pub fn suggestions_open(&mut self) {
        if matches!(self.suggestions.list, Remote::Idle) {
            self.suggestions.list = Remote::Loading;
            self.push_command(Cmd::Suggestions(SuggestionCmd::List));
        }
    }

    fn take_suggestion(&mut self, user_id: u64) -> bool {
        let Remote::Ready(list) = &mut self.suggestions.list else { return false };
        let before = list.len();
        list.retain(|s| s.user_id != user_id);
        list.len() != before
    }

    /// « Ajouter » : la demande d'ami part, la suggestion quitte la liste.
    pub fn suggestion_add(&mut self, user_id: u64) -> bool {
        let known = self.take_suggestion(user_id);
        if known {
            self.push_command(Cmd::Relations(RelationsCmd::Accept { user_id }));
        }
        known
    }

    /// « Ignorer » : la suggestion est retirée, ici et chez Discord.
    pub fn suggestion_dismiss(&mut self, user_id: u64) -> bool {
        let known = self.take_suggestion(user_id);
        if known {
            self.push_command(Cmd::Suggestions(SuggestionCmd::Dismiss { user_id }));
        }
        known
    }

    pub(super) fn apply_suggestions(&mut self, e: SuggestionEvent) {
        let SuggestionEvent::List(result) = e;
        self.suggestions.list = match result {
            Ok(mut list) => {
                // Déjà amis, bloqués ou en attente : rien à suggérer.
                list.retain(|s| self.relations.relation(s.user_id).is_none());
                Remote::Ready(list)
            }
            Err(p) => Remote::Failed(p),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    fn suggestion(id: u64) -> Suggestion {
        Suggestion { user_id: id, name: format!("Personne {id}"), username: format!("personne{id}"), avatar: Avatar::default_for(id), reason: None, mutual_friends: None }
    }

    #[test]
    fn asked_once_then_add_and_dismiss_leave_the_list() {
        let mut s = Store::new();
        assert!(!s.suggestion_add(3), "rien de connu : rien ne part");
        s.suggestions_open();
        s.suggestions_open();
        assert_eq!(s.take_commands(), vec![Cmd::Suggestions(SuggestionCmd::List)]);
        s.apply(Event::Suggestions(SuggestionEvent::List(Ok(vec![suggestion(3), suggestion(4), suggestion(5)]))));
        s.suggestions_open();
        assert!(s.suggestion_add(3) && !s.suggestion_add(3));
        assert!(s.suggestion_dismiss(4) && !s.suggestion_dismiss(9));
        assert_eq!(s.take_commands(), vec![Cmd::Relations(RelationsCmd::Accept { user_id: 3 }), Cmd::Suggestions(SuggestionCmd::Dismiss { user_id: 4 })]);
        assert_eq!(s.suggestions.list, Remote::Ready(vec![suggestion(5)]));
        // Un échec n'est pas retenté tout seul.
        let mut s = Store::new();
        s.suggestions_open();
        s.apply(Event::Suggestions(SuggestionEvent::List(Err(Problem::Forbidden))));
        s.take_commands();
        s.suggestions_open();
        assert!(s.take_commands().is_empty());
    }
}
