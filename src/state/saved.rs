//! Messages enregistrés et rappels (lot ND).
//!
//! Source : docs.discord.food, resources/user : « List Saved Messages »
//! (`GET /users/@me/saved-messages`, réponse `{ results }` d'objets
//! `{ message, save_data }`), « Save Message »
//! (`PUT /users/@me/saved-messages/{channel.id}/{message.id}`, corps
//! `{ due_at? }`), « Unsave Message » (`DELETE`, 204).
//!
//! Tout part d'un clic : « Enregistrer le message » ou « Me le rappeler »
//! dans le menu d'un message, « Retirer » dans l'onglet « Enregistrés » de
//! la boîte de réception. La liste est lue à l'ouverture de cet onglet.
//! C'est Discord qui tient l'échéance : Ferrocord la montre, il ne
//! programme rien de son côté.

use super::event::Cmd;
use super::quests::Refused;
use super::usersettings::Remote;
use super::{Problem, Store};

pub const MAX_SAVED: usize = 200;
/// Échéances proposées dans le menu d'un message.
pub const HOUR_MS: i64 = 3_600_000;
pub const DAY_MS: i64 = 24 * HOUR_MS;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Saved {
    pub channel_id: u64,
    pub message_id: u64,
    pub guild_id: Option<u64>,
    pub saved_ms: Option<i64>,
    /// Échéance du rappel (ms Unix), s'il y en a une.
    pub due_ms: Option<i64>,
    /// Auteur et début du message, si Discord les a renvoyés.
    pub author: String,
    pub excerpt: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SavedCmd {
    List,
    Save { channel_id: u64, message_id: u64, due_ms: Option<i64> },
    Remove { channel_id: u64, message_id: u64 },
}

#[derive(Debug)]
pub enum SavedEvent {
    List(Result<Vec<Saved>, Problem>),
    Saved { reminder: bool, result: Result<(), Refused> },
    Removed { channel_id: u64, message_id: u64, result: Result<(), Refused> },
}

/// Ce que l'interface annonce après une écriture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SavedNotice {
    Saved,
    Reminder,
    Refused(Refused),
}

#[derive(Default)]
pub struct SavedMessages {
    pub list: Remote<Vec<Saved>>,
    /// Une écriture est en cours.
    pub busy: bool,
    notice: Option<SavedNotice>,
    /// Rappels à venir connus de cette session (posés ici, ou lus dans la
    /// liste) : de quoi notifier à l'échéance (lot FF2).
    reminders: Vec<Reminder>,
    /// Rappel de l'écriture en cours, retenu si Discord l'accepte.
    pending: Option<Reminder>,
}

/// Un rappel à notifier à son échéance (lot FF2).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub channel_id: u64,
    pub message_id: u64,
    pub guild_id: Option<u64>,
    pub due_ms: i64,
    pub author: String,
    pub excerpt: String,
}

impl SavedMessages {
    pub fn clear(&mut self) {
        *self = SavedMessages::default();
    }

    pub fn take_notice(&mut self) -> Option<SavedNotice> {
        self.notice.take()
    }

    fn remember(&mut self, r: Reminder) {
        self.reminders.retain(|x| !(x.channel_id == r.channel_id && x.message_id == r.message_id));
        if self.reminders.len() < MAX_SAVED {
            self.reminders.push(r);
        }
    }

    /// Échéance du prochain rappel connu : l'interface programme un seul
    /// réveil pour cette date, aucune minuterie périodique.
    pub fn next_reminder(&self) -> Option<i64> {
        self.reminders.iter().map(|r| r.due_ms).min()
    }

    /// Rappels arrivés à échéance à `now_ms`, retirés : chacun n'est notifié qu'une fois.
    pub fn take_due(&mut self, now_ms: i64) -> Vec<Reminder> {
        let (due, later): (Vec<Reminder>, Vec<Reminder>) = std::mem::take(&mut self.reminders).into_iter().partition(|r| r.due_ms <= now_ms);
        self.reminders = later;
        due
    }
}

impl Store {
    /// L'onglet « Enregistrés » s'ouvre : la liste est lue si elle n'est pas connue.
    pub fn saved_open(&mut self) {
        if matches!(self.saved.list, Remote::Idle) {
            self.saved_refresh();
        }
    }

    /// « Actualiser » : une lecture à la fois.
    pub fn saved_refresh(&mut self) {
        if matches!(self.saved.list, Remote::Loading) {
            return;
        }
        self.saved.list = Remote::Loading;
        self.push_command(Cmd::Saved(SavedCmd::List));
    }

    /// « Enregistrer le message » (`remind_in_ms` absent) ou « Me le
    /// rappeler » : une écriture à la fois.
    pub fn save_message(&mut self, channel_id: u64, message_id: u64, remind_in_ms: Option<i64>) -> bool {
        if self.saved.busy {
            return false;
        }
        self.saved.busy = true;
        let due_ms = remind_in_ms.map(|d| self.now().saturating_add(d.clamp(60_000, 366 * DAY_MS)));
        self.saved.pending = due_ms.map(|due_ms| {
            let m = self.find_message(channel_id, message_id);
            Reminder { channel_id, message_id, guild_id: m.and_then(|m| m.guild_id), due_ms, author: m.map(|m| m.author.clone()).unwrap_or_default(), excerpt: m.map(|m| super::store::social::excerpt(&m.content)).unwrap_or_default() }
        });
        self.push_command(Cmd::Saved(SavedCmd::Save { channel_id, message_id, due_ms }));
        true
    }

    /// « Retirer » un message listé.
    pub fn unsave_message(&mut self, channel_id: u64, message_id: u64) -> bool {
        let listed = matches!(&self.saved.list, Remote::Ready(l) if l.iter().any(|s| s.channel_id == channel_id && s.message_id == message_id));
        if self.saved.busy || !listed {
            return false;
        }
        self.saved.busy = true;
        self.push_command(Cmd::Saved(SavedCmd::Remove { channel_id, message_id }));
        true
    }

    pub(super) fn apply_saved(&mut self, e: SavedEvent) {
        let now = self.now();
        let s = &mut self.saved;
        match e {
            SavedEvent::List(r) => {
                // Les rappels à venir de la liste sont retenus pour être notifiés à l'échéance.
                for x in r.iter().flatten() {
                    if let Some(due_ms) = x.due_ms.filter(|d| *d > now) {
                        s.remember(Reminder { channel_id: x.channel_id, message_id: x.message_id, guild_id: x.guild_id, due_ms, author: x.author.clone(), excerpt: x.excerpt.clone() });
                    }
                }
                s.list = r.map_or_else(Remote::Failed, Remote::Ready);
            }
            SavedEvent::Saved { reminder, result } => {
                s.busy = false;
                match s.pending.take() {
                    Some(p) if result.is_ok() => s.remember(p),
                    _ => {}
                }
                s.notice = Some(match result {
                    Ok(()) => {
                        // La liste connue est dépassée : elle sera relue à la prochaine ouverture de l'onglet.
                        if matches!(s.list, Remote::Ready(_) | Remote::Failed(_)) {
                            s.list = Remote::Idle;
                        }
                        if reminder { SavedNotice::Reminder } else { SavedNotice::Saved }
                    }
                    Err(r) => SavedNotice::Refused(r),
                });
            }
            SavedEvent::Removed { channel_id, message_id, result } => {
                s.busy = false;
                match result {
                    Ok(()) => {
                        s.reminders.retain(|x| !(x.channel_id == channel_id && x.message_id == message_id));
                        if let Remote::Ready(l) = &mut s.list {
                            l.retain(|x| !(x.channel_id == channel_id && x.message_id == message_id));
                        }
                    }
                    Err(r) => s.notice = Some(SavedNotice::Refused(r)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    fn saved(message_id: u64) -> Saved {
        Saved { channel_id: 5, message_id, guild_id: None, saved_ms: None, due_ms: None, author: String::new(), excerpt: String::new() }
    }

    #[test]
    fn one_write_at_a_time_and_the_list_is_reread_only_on_open() {
        let mut s = Store::new();
        assert!(s.save_message(5, 6, Some(HOUR_MS)) && !s.save_message(5, 7, None));
        let now = s.now();
        let cmds = s.take_commands();
        let [Cmd::Saved(SavedCmd::Save { channel_id: 5, message_id: 6, due_ms: Some(due) })] = cmds.as_slice() else { panic!("écriture attendue : {cmds:?}") };
        assert!((now + HOUR_MS - 5_000..=now + HOUR_MS + 5_000).contains(due));
        s.apply(Event::Saved(SavedEvent::Saved { reminder: true, result: Ok(()) }));
        assert_eq!(s.saved.take_notice(), Some(SavedNotice::Reminder));
        assert!(s.take_commands().is_empty(), "rien n'est relu tant que l'onglet n'est pas ouvert");
        s.saved_open();
        s.saved_open();
        assert_eq!(s.take_commands(), vec![Cmd::Saved(SavedCmd::List)]);
        s.apply(Event::Saved(SavedEvent::List(Ok(vec![saved(6), saved(7)]))));
        assert!(!s.unsave_message(5, 9), "message non listé");
        assert!(s.unsave_message(5, 6));
        s.apply(Event::Saved(SavedEvent::Removed { channel_id: 5, message_id: 6, result: Ok(()) }));
        assert_eq!(s.saved.list, Remote::Ready(vec![saved(7)]));
        assert!(s.save_message(5, 8, None));
        s.apply(Event::Saved(SavedEvent::Saved { reminder: false, result: Err(Refused::Captcha) }));
        assert_eq!(s.saved.take_notice(), Some(SavedNotice::Refused(Refused::Captcha)));
        assert_eq!(s.saved.list, Remote::Ready(vec![saved(7)]), "un refus garde la liste");
    }

    #[test]
    fn a_reminder_is_notified_once_when_due() {
        let mut s = Store::new();
        assert_eq!(s.saved.next_reminder(), None);
        // Posé ici : retenu seulement si Discord l'accepte.
        assert!(s.save_message(5, 6, Some(HOUR_MS)));
        assert_eq!(s.saved.next_reminder(), None);
        s.apply(Event::Saved(SavedEvent::Saved { reminder: true, result: Ok(()) }));
        let due = s.saved.next_reminder().unwrap();
        assert!(s.save_message(5, 7, Some(DAY_MS)));
        s.apply(Event::Saved(SavedEvent::Saved { reminder: true, result: Err(Refused::Captcha) }));
        assert_eq!(s.saved.next_reminder(), Some(due), "refusé : pas retenu");
        // Lu dans la liste : les rappels à venir seulement, sans doublon.
        let now = s.now();
        let listed = |id, due_ms| Saved { due_ms, ..saved(id) };
        s.apply(Event::Saved(SavedEvent::List(Ok(vec![listed(6, Some(due)), listed(8, Some(now - 1)), listed(9, Some(now + 1000)), listed(10, None)]))));
        assert_eq!(s.saved.next_reminder(), Some(now + 1000));
        assert!(s.saved.take_due(now).is_empty());
        let got = s.saved.take_due(now + 1000);
        assert_eq!(got.iter().map(|r| r.message_id).collect::<Vec<_>>(), vec![9]);
        assert!(s.saved.take_due(now + 1000).is_empty(), "une seule fois");
        assert_eq!(s.saved.next_reminder(), Some(due));
        // Retiré : plus de rappel.
        assert!(s.unsave_message(5, 6));
        s.apply(Event::Saved(SavedEvent::Removed { channel_id: 5, message_id: 6, result: Ok(()) }));
        assert_eq!(s.saved.next_reminder(), None);
    }
}
