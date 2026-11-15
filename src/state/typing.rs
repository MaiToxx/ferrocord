//! Qui écrit en ce moment, par salon (TYPING_START). Rien n'expire tout
//! seul : on ne lit que les entrées encore valables, et l'interface ne se
//! réveille qu'une fois, à la prochaine échéance, tant qu'un indicateur est
//! affiché (aucun minuteur au repos).
//!
//! Borné : au plus `MAX_PER_CHANNEL` personnes par salon et `MAX_CHANNELS`
//! salons. Les entrées échues sont retirées à chaque nouvel événement ; au-delà
//! des bornes, l'entrée qui échoit la première part.

use std::collections::HashMap;

/// Durée d'un indicateur sans nouvel événement (documentation de Discord :
/// TYPING_START vaut pour 10 secondes).
pub const TYPING_MS: i64 = 10_000;
const MAX_PER_CHANNEL: usize = 10;
const MAX_CHANNELS: usize = 50;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typer {
    pub user_id: u64,
    /// `None` : nom inconnu (l'interface dit « quelqu'un »).
    pub name: Option<String>,
    /// Échéance (ms Unix, horloge du magasin).
    pub until: i64,
}

#[derive(Default)]
pub struct Typing {
    by_channel: HashMap<u64, Vec<Typer>>,
}

impl Typing {
    pub fn start(&mut self, channel: u64, user_id: u64, name: Option<String>, now: i64) {
        self.prune(now);
        if !self.by_channel.contains_key(&channel)
            && self.by_channel.len() >= MAX_CHANNELS
            && let Some(first) = self.by_channel.iter().min_by_key(|(_, l)| l.iter().map(|t| t.until).max()).map(|(c, _)| *c)
        {
            self.by_channel.remove(&first);
        }
        let list = self.by_channel.entry(channel).or_default();
        let until = now.saturating_add(TYPING_MS);
        match list.iter_mut().find(|t| t.user_id == user_id) {
            Some(t) => {
                t.until = until;
                if name.is_some() {
                    t.name = name;
                }
            }
            None => {
                if list.len() >= MAX_PER_CHANNEL
                    && let Some(i) = list.iter().enumerate().min_by_key(|(_, t)| t.until).map(|(i, _)| i)
                {
                    list.remove(i);
                }
                list.push(Typer { user_id, name, until });
            }
        }
    }

    /// Son message est arrivé : il n'écrit plus.
    pub fn stop(&mut self, channel: u64, user_id: u64) {
        if let Some(list) = self.by_channel.get_mut(&channel) {
            list.retain(|t| t.user_id != user_id);
            if list.is_empty() {
                self.by_channel.remove(&channel);
            }
        }
    }

    /// Personnes qui écrivent encore à `now`, dans l'ordre d'arrivée.
    pub fn active(&self, channel: u64, now: i64) -> impl Iterator<Item = &Typer> {
        self.by_channel.get(&channel).into_iter().flatten().filter(move |t| t.until > now)
    }

    /// Prochaine échéance dans ce salon (le seul réveil à programmer).
    pub fn next_end(&self, channel: u64, now: i64) -> Option<i64> {
        self.active(channel, now).map(|t| t.until).min()
    }

    /// Tous ceux qui écrivent encore à `now` : (salon, personne).
    pub fn all(&self, now: i64) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.by_channel.iter().flat_map(move |(c, l)| l.iter().filter(move |t| t.until > now).map(move |t| (*c, t.user_id)))
    }

    pub fn clear(&mut self) {
        self.by_channel.clear();
    }

    fn prune(&mut self, now: i64) {
        self.by_channel.retain(|_, l| {
            l.retain(|t| t.until > now);
            !l.is_empty()
        });
    }

    #[cfg(test)]
    fn len(&self) -> (usize, usize) {
        (self.by_channel.len(), self.by_channel.values().map(Vec::len).sum())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(t: &Typing, c: u64, now: i64) -> Vec<u64> {
        t.active(c, now).map(|t| t.user_id).collect()
    }

    #[test]
    fn expires_after_ten_seconds_and_refreshes() {
        let mut t = Typing::default();
        t.start(1, 7, Some("Alice".into()), 0);
        t.start(1, 8, None, 3_000);
        assert_eq!(names(&t, 1, 5_000), vec![7, 8]);
        assert_eq!(t.next_end(1, 5_000), Some(10_000));
        assert_eq!(names(&t, 1, 10_000), vec![8]);
        // Un nouvel événement repousse l'échéance et garde le nom connu.
        t.start(1, 8, None, 12_000);
        t.start(1, 7, None, 12_000);
        assert_eq!(t.next_end(1, 12_500), Some(22_000));
        assert_eq!(t.active(1, 12_500).find(|x| x.user_id == 7).and_then(|x| x.name.clone()), None);
        t.start(1, 7, Some("Alice".into()), 13_000);
        t.start(1, 7, None, 13_500);
        assert_eq!(t.active(1, 14_000).find(|x| x.user_id == 7).and_then(|x| x.name.as_deref()), Some("Alice"));
        assert_eq!(t.next_end(2, 0), None);
    }

    #[test]
    fn message_stops_typing() {
        let mut t = Typing::default();
        t.start(1, 7, None, 0);
        t.stop(1, 7);
        t.stop(2, 7);
        assert_eq!(t.len(), (0, 0));
    }

    #[test]
    fn bounded() {
        let mut t = Typing::default();
        for u in 0..30 {
            t.start(1, u, None, u as i64);
        }
        assert_eq!(t.len(), (1, MAX_PER_CHANNEL));
        // Les plus récents restent.
        assert!(names(&t, 1, 0).contains(&29) && !names(&t, 1, 0).contains(&0));
        for c in 0..200 {
            t.start(100 + c, 1, None, 1_000 + c as i64);
        }
        assert!(t.len().0 <= MAX_CHANNELS);
        // Les échus partent au prochain événement.
        t.start(5, 1, None, 1_000_000);
        assert_eq!(t.len(), (1, 1));
    }
}
