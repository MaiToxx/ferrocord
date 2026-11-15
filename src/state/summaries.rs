//! Résumés de salon (lot ND), en lecture.
//!
//! Source : docs.discord.food, resources/message : « Get Conversation
//! Summaries » (`GET /channels/{channel.id}/summaries`, réponse
//! `{ summaries }`, 50 au plus) et « Conversation Summary Object »
//! (`id`, `topic`, `summ_short`, `people`, `unsafe`, `start_id`, `count`).
//!
//! Demandés au clic sur « Résumés du salon » (menu du salon), et de nouveau
//! seulement par « Actualiser ». Les résumés marqués `unsafe` ne sont pas
//! montrés. Ferrocord ne demande jamais à Discord de produire un résumé.

use super::event::Cmd;
use super::usersettings::Remote;
use super::{Problem, Store};
use std::collections::HashMap;

pub const MAX_SUMMARIES: usize = 50;
/// Salons dont les résumés sont gardés en mémoire.
const KEPT: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Summary {
    pub id: u64,
    pub topic: String,
    pub text: String,
    /// Premier message résumé : cible de « Aller au message ».
    pub start_id: Option<u64>,
    pub messages: u32,
    pub people: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SummaryCmd {
    Load { channel_id: u64 },
}

#[derive(Debug)]
pub enum SummaryEvent {
    Loaded { channel_id: u64, result: Result<Vec<Summary>, Problem> },
}

#[derive(Default)]
pub struct Summaries {
    by_channel: HashMap<u64, Remote<Vec<Summary>>>,
}

impl Summaries {
    pub fn clear(&mut self) {
        self.by_channel.clear();
    }

    pub fn get(&self, channel_id: u64) -> Option<&Remote<Vec<Summary>>> {
        self.by_channel.get(&channel_id)
    }
}

impl Store {
    /// Le panneau est ouvert : les résumés sont demandés une fois ; après un
    /// échec, seul « Actualiser » redemande.
    pub fn summaries_open(&mut self, channel_id: u64) {
        if self.summaries.get(channel_id).is_none() {
            self.summaries_refresh(channel_id);
        }
    }

    /// « Actualiser » : une demande à la fois par salon.
    pub fn summaries_refresh(&mut self, channel_id: u64) {
        if matches!(self.summaries.get(channel_id), Some(Remote::Loading)) {
            return;
        }
        if self.summaries.by_channel.len() >= KEPT && !self.summaries.by_channel.contains_key(&channel_id) {
            self.summaries.by_channel.retain(|_, r| matches!(r, Remote::Loading));
        }
        self.summaries.by_channel.insert(channel_id, Remote::Loading);
        self.push_command(Cmd::Summaries(SummaryCmd::Load { channel_id }));
    }

    pub(super) fn apply_summaries(&mut self, e: SummaryEvent) {
        let SummaryEvent::Loaded { channel_id, result } = e;
        // Réponse pour un salon oublié entre-temps : ignorée.
        if let Some(slot) = self.summaries.by_channel.get_mut(&channel_id) {
            *slot = result.map_or_else(Remote::Failed, Remote::Ready);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    #[test]
    fn asked_on_open_once_and_again_only_on_refresh() {
        let mut s = Store::new();
        s.summaries_open(7);
        s.summaries_open(7);
        s.summaries_refresh(7);
        assert_eq!(s.take_commands(), vec![Cmd::Summaries(SummaryCmd::Load { channel_id: 7 })]);
        s.apply(Event::Summaries(SummaryEvent::Loaded { channel_id: 7, result: Ok(Vec::new()) }));
        s.apply(Event::Summaries(SummaryEvent::Loaded { channel_id: 8, result: Ok(Vec::new()) }));
        assert!(s.summaries.get(8).is_none());
        s.summaries_open(7);
        assert!(s.take_commands().is_empty());
        s.summaries_refresh(7);
        assert_eq!(s.take_commands().len(), 1);
        s.apply(Event::Summaries(SummaryEvent::Loaded { channel_id: 7, result: Err(Problem::Forbidden) }));
        s.summaries_open(7);
        assert!(s.take_commands().is_empty(), "un échec ne repart pas tout seul");
        for c in 100..120 {
            s.summaries_open(c);
            s.apply(Event::Summaries(SummaryEvent::Loaded { channel_id: c, result: Ok(Vec::new()) }));
        }
        assert!(s.summaries.by_channel.len() <= KEPT);
    }
}
