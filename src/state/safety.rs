//! Centre de sécurité : état du compte, en lecture (lot ND).
//!
//! Source : docs.discord.food, resources/safety-hub : « Get Safety Hub »
//! (`GET /safety-hub/@me`, réponse `classifications`,
//! `guild_classifications`, `account_standing { state }`,
//! `is_appeal_eligible`), « Classification Object » (`description`,
//! `explainer_link`, `actions[].descriptions`, `max_expiration_time`).
//!
//! Lu seulement au clic sur « Vérifier auprès de Discord » (page
//! Confidentialité). Les recours, vérifications d'âge et autres démarches
//! ne sont pas faits ici : ils restent dans l'application officielle.

use super::event::Cmd;
use super::usersettings::Remote;
use super::{Problem, Store};

pub const MAX_VIOLATIONS: usize = 20;

/// « Account Standing State » (source : 100 à 500).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Standing {
    AllGood,
    Limited,
    VeryLimited,
    AtRisk,
    Suspended,
    Unknown,
}

impl Standing {
    pub fn from_code(code: u64) -> Standing {
        match code {
            100 => Standing::AllGood,
            200 => Standing::Limited,
            300 => Standing::VeryLimited,
            400 => Standing::AtRisk,
            500 => Standing::Suspended,
            _ => Standing::Unknown,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Violation {
    pub id: u64,
    /// Ce que Discord reproche, dans ses mots.
    pub description: String,
    /// Mesures prises, dans les mots de Discord.
    pub actions: Vec<String>,
    /// Fin des mesures (ms Unix), si elle est annoncée.
    pub expires_ms: Option<i64>,
    /// Page d'explication de Discord (`https://`), si elle est donnée.
    pub link: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafetyHub {
    pub standing: Standing,
    pub violations: Vec<Violation>,
    /// Infractions relevées sur des serveurs dont le compte est responsable.
    pub guild_violations: usize,
    pub appeal_eligible: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SafetyCmd {
    Load,
}

#[derive(Debug)]
pub enum SafetyEvent {
    Loaded(Result<SafetyHub, Problem>),
}

impl Store {
    /// « Vérifier auprès de Discord » : une demande à la fois.
    pub fn safety_hub_load(&mut self) -> bool {
        if matches!(self.safety_hub, Remote::Loading) {
            return false;
        }
        self.safety_hub = Remote::Loading;
        self.push_command(Cmd::SafetyHub(SafetyCmd::Load));
        true
    }

    pub(super) fn apply_safety_hub(&mut self, e: SafetyEvent) {
        let SafetyEvent::Loaded(result) = e;
        self.safety_hub = result.map_or_else(Remote::Failed, Remote::Ready);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    #[test]
    fn read_only_on_request_and_one_at_a_time() {
        let mut s = Store::new();
        assert!(s.take_commands().is_empty());
        assert!(s.safety_hub_load() && !s.safety_hub_load());
        assert_eq!(s.take_commands(), vec![Cmd::SafetyHub(SafetyCmd::Load)]);
        s.apply(Event::SafetyHub(SafetyEvent::Loaded(Err(Problem::Forbidden))));
        assert_eq!(s.safety_hub, Remote::Failed(Problem::Forbidden));
        assert!(s.take_commands().is_empty(), "un échec n'est pas retenté");
        assert!(s.safety_hub_load());
        assert_eq!((Standing::from_code(100), Standing::from_code(500), Standing::from_code(7)), (Standing::AllGood, Standing::Suspended, Standing::Unknown));
    }
}
