//! Sondages (lot T13) : objet Poll d'un message, votes reçus de la
//! passerelle (`MESSAGE_POLL_VOTE_ADD` / `_REMOVE`), votes, liste des votants,
//! fin d'un sondage et création depuis la saisie.
//!
//! Formes : documentation officielle (Poll Resource : Poll Object, Poll
//! Create Request Object, Poll Results Object, Get Answer Voters, End Poll) ;
//! le vote (`PUT /channels/{id}/polls/{message}/answers/@me`) vient de
//! docs.discord.food (Message › Create Poll Vote).

use super::notice::Problem;
use super::social::{Emoji, Reactor};
use std::collections::{HashMap, HashSet, VecDeque};

/// Réponses d'un sondage, au plus (documenté).
pub const MAX_ANSWERS: usize = 10;
/// Longueur maximale de la question (documentée).
pub const MAX_QUESTION: usize = 300;
/// Longueur maximale d'une réponse (documentée).
pub const MAX_ANSWER: usize = 55;
/// Durée maximale en heures : 32 jours (documentée).
pub const MAX_HOURS: u32 = 32 * 24;
/// Durées proposées à la création (heures), comme l'application officielle.
pub const DURATIONS: [u32; 7] = [1, 4, 8, 24, 72, 168, 336];
/// Votants demandés par réponse (une page, valeur par défaut documentée).
pub const VOTERS_PAGE: usize = 25;
/// Listes de votants gardées, au plus : les plus anciennes partent d'abord.
pub const MAX_VOTER_LISTS: usize = 32;

/// Réponse d'un sondage, avec son compte.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct PollAnswer {
    pub id: u32,
    pub text: String,
    pub emoji: Option<Emoji>,
    pub count: u32,
    /// J'ai voté pour elle.
    pub me: bool,
}

/// Sondage porté par un message.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Poll {
    pub question: String,
    pub answers: Vec<PollAnswer>,
    /// Fin (ms Unix) ; « nullable » d'après la documentation.
    pub expiry_ms: Option<i64>,
    pub multi: bool,
    /// Votes comptés précisément (sondage terminé et dépouillé).
    pub finalized: bool,
    /// `results` reçu : sans lui, les comptes sont inconnus (pas nuls).
    pub counts_known: bool,
}

impl Poll {
    /// Total des votes (en choix multiples : des votes, pas des votants).
    pub fn total(&self) -> u32 {
        self.answers.iter().fold(0u32, |n, a| n.saturating_add(a.count))
    }

    pub fn ended(&self, now_ms: i64) -> bool {
        self.finalized || self.expiry_ms.is_some_and(|e| e <= now_ms)
    }

    pub fn voted(&self) -> bool {
        self.answers.iter().any(|a| a.me)
    }

    pub fn mine(&self) -> Vec<u32> {
        self.answers.iter().filter(|a| a.me).map(|a| a.id).collect()
    }

    /// Plus grand compte (barre pleine, réponse en tête).
    pub fn top(&self) -> u32 {
        self.answers.iter().map(|a| a.count).max().unwrap_or(0)
    }

    /// Vote reçu de la passerelle ; `mine` : le mien (idempotent : l'écho
    /// d'un vote déjà compté ne compte pas deux fois). `true` si quelque chose change.
    pub fn apply_vote(&mut self, answer_id: u32, mine: bool, add: bool) -> bool {
        let Some(a) = self.answers.iter_mut().find(|a| a.id == answer_id) else { return false };
        if mine {
            if a.me == add {
                return false;
            }
            a.me = add;
        }
        let before = a.count;
        a.count = if add { a.count.saturating_add(1) } else { a.count.saturating_sub(1) };
        before != a.count || mine
    }

    /// Mes votes deviennent `ids` (mise à jour immédiate avant la réponse).
    pub fn set_mine(&mut self, ids: &[u32]) -> bool {
        let mut changed = false;
        for a in &mut self.answers {
            let want = ids.contains(&a.id);
            if a.me != want {
                a.me = want;
                a.count = if want { a.count.saturating_add(1) } else { a.count.saturating_sub(1) };
                changed = true;
            }
        }
        changed
    }
}

/// Sondage à créer (fenêtre de la saisie).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PollDraft {
    pub question: String,
    pub answers: Vec<String>,
    /// Heures.
    pub hours: u32,
    pub multi: bool,
}

impl Default for PollDraft {
    fn default() -> Self {
        PollDraft { question: String::new(), answers: vec![String::new(), String::new()], hours: 24, multi: false }
    }
}

impl PollDraft {
    /// Réponses non vides, coupées.
    pub fn filled(&self) -> Vec<String> {
        self.answers.iter().map(|a| a.trim().to_string()).filter(|a| !a.is_empty()).collect()
    }

    /// Prêt à partir : une question, au moins une réponse, limites documentées.
    pub fn valid(&self) -> bool {
        let q = self.question.trim();
        let answers = self.filled();
        !q.is_empty()
            && q.chars().count() <= MAX_QUESTION
            && !answers.is_empty()
            && answers.len() <= MAX_ANSWERS
            && answers.iter().all(|a| a.chars().count() <= MAX_ANSWER)
            && (1..=MAX_HOURS).contains(&self.hours)
    }
}

/// Action sur un sondage (pour annoncer un échec).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PollAction {
    Vote,
    End,
    Create,
}

/// Réseau -> magasin.
#[derive(Debug)]
pub enum PollEvent {
    /// MESSAGE_POLL_VOTE_ADD / _REMOVE.
    Vote { channel_id: u64, message_id: u64, answer_id: u32, user_id: u64, add: bool },
    /// MESSAGE_UPDATE qui porte le sondage (résultats, fin).
    Replace { channel_id: u64, message_id: u64, poll: Box<Poll> },
    /// Réponse au vote ; en échec, mes votes d'avant (`previous`) reviennent.
    VoteDone { channel_id: u64, message_id: u64, previous: Vec<u32>, error: Option<Problem> },
    Voters { message_id: u64, answer_id: u32, users: Result<Vec<Reactor>, Problem> },
    EndDone { message_id: u64, error: Option<Problem> },
    CreateDone { channel_id: u64, error: Option<Problem> },
}

/// Magasin -> réseau. Chaque commande suit un geste de l'utilisateur.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PollCmd {
    /// `answers` vide : retirer mon vote.
    Vote { channel_id: u64, message_id: u64, answers: Vec<u32>, previous: Vec<u32> },
    Voters { channel_id: u64, message_id: u64, answer_id: u32 },
    End { channel_id: u64, message_id: u64 },
    Create { channel_id: u64, draft: PollDraft, nonce: u64 },
}

/// Liste des votants d'une réponse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Voters {
    Loading,
    Ready(Vec<Reactor>),
    Failed(Problem),
}

/// Ce qui est en cours ou gardé pour les sondages.
#[derive(Default)]
pub struct Polls {
    /// Votes envoyés sans réponse (par message) : un nouveau clic attend.
    voting: HashSet<u64>,
    ending: HashSet<u64>,
    voters: HashMap<(u64, u32), Voters>,
    order: VecDeque<(u64, u32)>,
    /// Création envoyée, pas encore confirmée.
    pub creating: bool,
    /// Sondages créés (l'interface ferme sa fenêtre quand il change).
    pub created: u32,
    failure: Option<(PollAction, Problem)>,
}

impl Polls {
    pub fn clear(&mut self) {
        *self = Polls::default();
    }

    pub fn voting(&self, message_id: u64) -> bool {
        self.voting.contains(&message_id)
    }

    pub fn ending(&self, message_id: u64) -> bool {
        self.ending.contains(&message_id)
    }

    pub fn voters(&self, message_id: u64, answer_id: u32) -> Option<&Voters> {
        self.voters.get(&(message_id, answer_id))
    }

    pub(super) fn begin_vote(&mut self, message_id: u64) -> bool {
        self.voting.insert(message_id)
    }

    pub(super) fn end_vote(&mut self, message_id: u64) {
        self.voting.remove(&message_id);
    }

    pub(super) fn begin_end(&mut self, message_id: u64) -> bool {
        self.ending.insert(message_id)
    }

    pub(super) fn end_end(&mut self, message_id: u64) {
        self.ending.remove(&message_id);
    }

    /// `false` si la liste est déjà demandée ou gardée.
    pub(super) fn want_voters(&mut self, key: (u64, u32)) -> bool {
        if self.voters.contains_key(&key) {
            return false;
        }
        self.set_voters(key, Voters::Loading);
        true
    }

    pub(super) fn set_voters(&mut self, key: (u64, u32), v: Voters) {
        if self.voters.insert(key, v).is_none() {
            self.order.push_back(key);
            while self.order.len() > MAX_VOTER_LISTS {
                if let Some(old) = self.order.pop_front() {
                    self.voters.remove(&old);
                }
            }
        }
    }

    /// Un vote change les votants : les listes de ce message sont refaites à la demande.
    pub(super) fn forget_voters(&mut self, message_id: u64) {
        self.voters.retain(|(m, _), _| *m != message_id);
        self.order.retain(|(m, _)| *m != message_id);
    }

    pub(super) fn fail(&mut self, action: PollAction, problem: Problem) {
        self.failure = Some((action, problem));
    }

    pub fn take_failure(&mut self) -> Option<(PollAction, Problem)> {
        self.failure.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn poll() -> Poll {
        let a = |id, count| PollAnswer { id, text: format!("r{id}"), emoji: None, count, me: false };
        Poll { question: "Q".into(), answers: vec![a(1, 2), a(2, 0)], expiry_ms: Some(1000), multi: false, finalized: false, counts_known: true }
    }

    #[test]
    fn votes_count_once_and_mine_are_idempotent() {
        let mut p = poll();
        assert!(p.apply_vote(2, true, true));
        assert!(!p.apply_vote(2, true, true), "écho de mon vote");
        assert_eq!((p.answers[1].count, p.answers[1].me, p.total()), (1, true, 3));
        assert!(p.apply_vote(1, false, false));
        assert!(!p.apply_vote(9, false, true), "réponse inconnue");
        assert!(p.apply_vote(1, false, false) && !p.apply_vote(1, false, false), "jamais sous zéro");
        assert!(p.set_mine(&[1]));
        assert_eq!(p.mine(), vec![1]);
        assert_eq!((p.answers[0].count, p.answers[1].count), (1, 0));
        assert!(!p.ended(999) && p.ended(1000));
    }

    #[test]
    fn draft_limits() {
        let mut d = PollDraft { question: "Pizza ?".into(), ..PollDraft::default() };
        assert!(!d.valid(), "aucune réponse");
        d.answers[1] = " Oui ".into();
        assert!(d.valid());
        assert_eq!(d.filled(), vec!["Oui"]);
        d.answers[0] = "x".repeat(MAX_ANSWER + 1);
        assert!(!d.valid());
        d.answers[0] = "Non".into();
        d.hours = MAX_HOURS + 1;
        assert!(!d.valid());
    }

    #[test]
    fn voter_lists_are_bounded() {
        let mut p = Polls::default();
        for i in 0..(MAX_VOTER_LISTS as u64 + 3) {
            assert!(p.want_voters((i, 1)));
        }
        assert!(p.voters(0, 1).is_none() && p.voters(MAX_VOTER_LISTS as u64 + 2, 1).is_some());
        p.forget_voters(MAX_VOTER_LISTS as u64 + 2);
        assert!(p.want_voters((MAX_VOTER_LISTS as u64 + 2, 1)));
    }
}
