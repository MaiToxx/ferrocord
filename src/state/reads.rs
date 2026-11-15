//! Accusés de lecture synchronisés avec Discord.
//!
//! Un salon est non lu quand son dernier message (`heads`) est plus récent
//! que le dernier message lu (`acked`), comme le calcule le client officiel
//! (documentation communautaire « Read State » : « a resource is unread if
//! it has at least one entity with an ID greater than the last acknowledged
//! ID »). Les deux viennent du READY, puis de MESSAGE_CREATE et MESSAGE_ACK.
//!
//! Un salon réellement lu (fenêtre active, bas de la liste visible) est
//! accusé après `ACK_DELAY` : les messages qui arrivent pendant ce temps
//! partent dans le même accusé (jamais un envoi par message).

use super::relations::ReadState;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// Attente avant d'envoyer un accusé : regroupe les messages lus à la suite.
pub const ACK_DELAY: Duration = Duration::from_secs(3);

#[derive(Default)]
pub struct Reads {
    /// Le READY contenait des états de lecture (compte utilisateur) : sans
    /// eux, aucun accusé n'est envoyé et les non-lus restent locaux.
    pub(super) synced: bool,
    /// Dernier message lu, par salon.
    acked: HashMap<u64, u64>,
    /// Dernier message connu, par salon (READY puis MESSAGE_CREATE). Ne
    /// contient que les salons du READY ou créés depuis : borné par le compte.
    heads: HashMap<u64, u64>,
    /// Accusés à envoyer : salon -> message.
    pending: HashMap<u64, u64>,
    /// Échéance du prochain envoi groupé.
    due: Option<Instant>,
    /// Salon marqué non lu à la main (lot M) : plus aucun accusé automatique
    /// tant qu'il reste affiché, sinon il redeviendrait lu trois secondes après.
    held: Option<u64>,
    /// Mode discret (lot E3) : plus aucun accusé automatique ; un salon n'est
    /// accusé qu'au clic sur « Marquer comme lu ». Réglage de l'ordinateur :
    /// il survit au changement de session.
    manual: bool,
}

impl Reads {
    pub(super) fn reset(&mut self, states: Option<Vec<ReadState>>, heads: Vec<(u64, u64)>) {
        self.pending.clear();
        self.due = None;
        self.heads = heads.into_iter().collect();
        self.synced = states.is_some();
        self.acked = states.unwrap_or_default().into_iter().map(|r| (r.channel_id, r.last_acked)).collect();
    }

    pub(super) fn clear(&mut self) {
        *self = Reads { manual: self.manual, ..Reads::default() };
    }

    /// Mode discret : accusés automatiques coupés (ceux en attente sont abandonnés) ou rétablis.
    pub fn set_manual(&mut self, on: bool) {
        if on {
            self.pending.clear();
            self.due = None;
        }
        self.manual = on;
    }

    pub fn manual(&self) -> bool {
        self.manual
    }

    /// Salon non lu d'après Discord (les salons sans état de lecture sont lus).
    pub fn unread(&self, channel: u64) -> bool {
        self.acked.get(&channel).is_some_and(|a| self.heads.get(&channel).is_some_and(|h| h > a))
    }

    pub fn head(&self, channel: u64) -> Option<u64> {
        self.heads.get(&channel).copied()
    }

    pub fn acked(&self, channel: u64) -> Option<u64> {
        self.acked.get(&channel).copied()
    }

    /// Salons connus (READY ou depuis) qui ont un message plus récent que leur accusé.
    pub(super) fn unread_channels(&self) -> impl Iterator<Item = u64> + '_ {
        self.heads.keys().copied().filter(|c| self.unread(*c))
    }

    pub(super) fn track(&mut self, channel: u64) {
        self.heads.entry(channel).or_insert(0);
    }

    pub(super) fn forget(&mut self, channel: u64) {
        self.heads.remove(&channel);
        self.acked.remove(&channel);
        self.pending.remove(&channel);
    }

    /// Nouveau message ; le mien vaut lecture (Discord ne marque pas ses
    /// propres messages comme non lus). Salon inconnu : ignoré (borne).
    pub(super) fn on_message(&mut self, channel: u64, id: u64, mine: bool) {
        let Some(head) = self.heads.get_mut(&channel) else { return };
        *head = (*head).max(id);
        if mine {
            let a = self.acked.entry(channel).or_insert(0);
            *a = (*a).max(id);
            if self.pending.get(&channel).is_some_and(|p| *p <= id) {
                self.pending.remove(&channel);
            }
        }
    }

    /// MESSAGE_ACK : la valeur de Discord fait foi (un « marquer comme non
    /// lu » ailleurs peut la faire reculer).
    pub(super) fn on_ack(&mut self, channel: u64, id: u64) {
        self.acked.insert(channel, id);
        if self.pending.get(&channel).is_some_and(|p| *p <= id) {
            self.pending.remove(&channel);
        }
        if let Some(h) = self.heads.get_mut(&channel) {
            *h = (*h).max(id);
        }
    }

    /// Le salon est lu jusqu'à `id` : un accusé est prévu s'il fait avancer
    /// la lecture. Renvoie `true` si quelque chose a été ajouté.
    pub(super) fn want(&mut self, channel: u64, id: u64, now: Instant) -> bool {
        if !self.synced || self.manual || id == 0 || self.held == Some(channel) || self.acked.get(&channel).is_some_and(|a| *a >= id) || self.pending.get(&channel).is_some_and(|p| *p >= id) {
            return false;
        }
        self.pending.insert(channel, id);
        self.due.get_or_insert(now + ACK_DELAY);
        true
    }

    /// Accusés dus à `now`, retirés de l'attente et tenus pour faits.
    pub(super) fn take_due(&mut self, now: Instant) -> Vec<(u64, u64)> {
        if self.due.is_none_or(|d| d > now) {
            return Vec::new();
        }
        self.due = None;
        let mut out: Vec<(u64, u64)> = self.pending.drain().collect();
        out.sort_unstable();
        for (c, id) in &out {
            self.acked.insert(*c, *id);
        }
        out
    }

    /// « Marquer comme lu » : accusé immédiat jusqu'au dernier message
    /// connu ; `None` s'il n'y a rien à accuser.
    pub(super) fn take_now(&mut self, channel: u64) -> Option<u64> {
        if !self.synced {
            return None;
        }
        let head = self.heads.get(&channel).copied().filter(|h| *h > 0)?;
        self.pending.remove(&channel);
        if self.pending.is_empty() {
            self.due = None;
        }
        if self.acked.get(&channel).is_some_and(|a| *a >= head) {
            return None;
        }
        self.acked.insert(channel, head);
        Some(head)
    }

    /// « Marquer comme non lu » à partir de `id` : l'accusé recule juste
    /// avant lui, et rien ne repart pour ce salon tant qu'il est retenu.
    /// `false` sans états de lecture (compte de bot).
    pub(super) fn unack(&mut self, channel: u64, id: u64) -> bool {
        if !self.synced || id == 0 {
            return false;
        }
        self.pending.remove(&channel);
        if self.pending.is_empty() {
            self.due = None;
        }
        self.acked.insert(channel, id - 1);
        let head = self.heads.entry(channel).or_insert(0);
        *head = (*head).max(id);
        self.held = Some(channel);
        true
    }

    /// Le salon retenu est quitté ou rouvert : les accusés reprennent.
    pub(super) fn release(&mut self) {
        self.held = None;
    }

    pub fn next_due(&self) -> Option<Instant> {
        self.due
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rs(channel_id: u64, last_acked: u64) -> ReadState {
        ReadState { channel_id, last_acked, mentions: 0 }
    }

    #[test]
    fn unread_compares_head_and_ack() {
        let mut r = Reads::default();
        r.reset(Some(vec![rs(1, 10), rs(2, 20)]), vec![(1, 15), (2, 20), (3, 99)]);
        assert!(r.unread(1));
        assert!(!r.unread(2));
        assert!(!r.unread(3), "sans état de lecture : lu");
        assert_eq!(r.unread_channels().collect::<Vec<_>>(), vec![1]);
        r.on_message(2, 21, false);
        assert!(r.unread(2));
        r.on_message(2, 22, true);
        assert!(!r.unread(2), "mon message vaut lecture");
        r.on_ack(1, 15);
        assert!(!r.unread(1));
        // Un autre appareil marque comme non lu : l'accusé recule.
        r.on_ack(1, 12);
        assert!(r.unread(1));
        // Salon inconnu : rien n'est retenu.
        r.on_message(77, 5, false);
        assert_eq!(r.head(77), None);
    }

    #[test]
    fn acks_are_grouped_and_delayed() {
        let t0 = Instant::now();
        let mut r = Reads::default();
        r.reset(Some(vec![rs(1, 10)]), vec![(1, 10)]);
        r.on_message(1, 11, false);
        assert!(r.want(1, 11, t0));
        assert!(r.take_due(t0 + ACK_DELAY / 2).is_empty());
        // Messages lus pendant l'attente : même accusé, échéance inchangée.
        r.on_message(1, 12, false);
        assert!(r.want(1, 12, t0 + ACK_DELAY / 2));
        assert!(!r.want(1, 12, t0 + ACK_DELAY / 2), "déjà prévu");
        assert_eq!(r.next_due(), Some(t0 + ACK_DELAY));
        assert_eq!(r.take_due(t0 + ACK_DELAY), vec![(1, 12)]);
        assert_eq!(r.next_due(), None);
        assert!(!r.want(1, 12, t0 + ACK_DELAY), "déjà accusé");
        // L'écho de Discord n'en redemande pas.
        r.on_ack(1, 12);
        assert!(!r.want(1, 12, t0 + ACK_DELAY));
    }

    #[test]
    fn ack_from_elsewhere_cancels_pending() {
        let t0 = Instant::now();
        let mut r = Reads::default();
        r.reset(Some(vec![rs(1, 1)]), vec![(1, 5)]);
        assert!(r.want(1, 5, t0));
        r.on_ack(1, 5);
        assert!(r.take_due(t0 + ACK_DELAY).is_empty());
    }

    #[test]
    fn manual_unread_holds_acks_until_released() {
        let t0 = Instant::now();
        let mut r = Reads::default();
        r.reset(Some(vec![rs(1, 9)]), vec![(1, 9)]);
        assert!(r.unack(1, 7));
        assert_eq!(r.acked(1), Some(6));
        assert!(r.unread(1));
        assert!(!r.want(1, 9, t0), "retenu : pas d'accusé automatique");
        assert!(r.take_due(t0 + ACK_DELAY).is_empty());
        r.release();
        assert!(r.want(1, 9, t0));
        r.reset(None, vec![(1, 9)]);
        assert!(!r.unack(1, 7), "sans états de lecture");
    }

    #[test]
    fn mark_read_now_and_unsynced_accounts() {
        let mut r = Reads::default();
        r.reset(Some(vec![rs(1, 1)]), vec![(1, 5), (2, 0)]);
        assert_eq!(r.take_now(1), Some(5));
        assert_eq!(r.take_now(1), None);
        assert_eq!(r.take_now(2), None, "salon vide");
        // Compte de bot (pas de read_state) : jamais d'accusé.
        r.reset(None, vec![(1, 5)]);
        assert!(!r.want(1, 5, Instant::now()));
        assert_eq!(r.take_now(1), None);
    }
}
