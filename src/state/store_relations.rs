//! Raccord du magasin avec les relations (`social`) et les accusés de
//! lecture (`reads`). Module enfant de `store` (déclaré par `#[path]`) pour
//! tenir les non-lus par les mêmes fonctions privées que le reste du
//! magasin, sans les exposer ailleurs.

use super::Store;
use super::super::event::Cmd;
use super::super::model::ChannelKind;
use super::super::people::Status;
use super::super::relations::{FriendTab, RelationsCmd, RelationsEvent, RelationsNotice};
use super::Effect;
use std::{collections::HashMap, time::Instant};

/// Salons par requête de « marquer le serveur comme lu » (lots de 100 au plus).
pub const ACK_BULK_MAX: usize = 100;

impl Store {
    pub(super) fn apply_relations(&mut self, e: RelationsEvent) -> Option<Effect> {
        match e {
            RelationsEvent::Ready { relations, people, dms, reads, heads } => {
                self.relations.reset(relations, people, dms);
                self.keep_friend_presences();
                let mentions: Option<HashMap<u64, u32>> = reads.as_ref().map(|r| r.iter().map(|s| (s.channel_id, s.mentions)).collect());
                self.reads.reset(reads, heads);
                // Les non-lus deviennent ceux que Discord connaît (identiques
                // sur tous les appareils) ; le salon affiché reste lu ici et
                // sera accusé dès qu'il sera vraiment lu.
                if let Some(mentions) = mentions {
                    let old: Vec<u64> = self.unread.iter().chain(self.mentioned.iter()).copied().collect();
                    for c in old {
                        self.clear_unread(c);
                    }
                    let now_unread: Vec<u64> = self.reads.unread_channels().filter(|c| self.channel != Some(*c)).collect();
                    for c in now_unread {
                        let ping = mentions.get(&c).is_some_and(|n| *n > 0);
                        // Salon muet (lot C1) : pas de non-lu, sauf mention.
                        if ping || !self.channel_muted(c) {
                            self.mark_unread(c, ping);
                            self.set_mention_count(c, mentions.get(&c).copied().unwrap_or(0));
                        }
                    }
                }
            }
            RelationsEvent::RelationAdd(r) => {
                self.relations.add(r);
                self.keep_friend_presences();
            }
            RelationsEvent::RelationUpdate { id, nickname, ignored } => self.relations.update(id, nickname, ignored),
            RelationsEvent::RelationRemove { id } => {
                self.relations.remove(id);
                self.keep_friend_presences();
            }
            RelationsEvent::Dm { info, people, known_recipients } => {
                self.reads.track(info.id);
                self.relations.upsert_dm(info, people, known_recipients);
            }
            RelationsEvent::RecipientAdd { channel_id, user } => self.relations.recipient_add(channel_id, user),
            RelationsEvent::RecipientRemove { channel_id, user_id } => self.relations.recipient_remove(channel_id, user_id),
            RelationsEvent::Ack { channel_id, message_id, mentions } => {
                self.reads.on_ack(channel_id, message_id);
                let unread = self.reads.unread(channel_id);
                let ping = match mentions {
                    Some(n) => n > 0,
                    None => self.mentioned.contains(&channel_id),
                };
                if !unread {
                    self.clear_unread(channel_id);
                } else if self.channel != Some(channel_id) {
                    // Marqué non lu sur un autre appareil (muet : seulement s'il y a une mention).
                    self.clear_unread(channel_id);
                    if ping || !self.channel_muted(channel_id) {
                        self.mark_unread(channel_id, ping);
                    }
                }
            }
            RelationsEvent::DmOpened { channel_id, user_id } => {
                // Seul le MP que l'utilisateur vient de demander est ouvert.
                if user_id.is_none() || self.relations.opening == user_id {
                    self.relations.opening = None;
                    if self.dms.iter().any(|d| d.id == channel_id) {
                        self.select_dms();
                        self.select_channel(channel_id);
                        return Some(Effect::ScrollToBottom);
                    }
                }
            }
            RelationsEvent::RequestSent { username } => self.relations.notify(RelationsNotice::RequestSent { username }),
            RelationsEvent::Failed { action, problem } => {
                if action == super::super::relations::RelationsAction::OpenDm {
                    self.relations.opening = None;
                }
                self.relations.notify(RelationsNotice::Failed { action, problem });
            }
            RelationsEvent::Captcha { action } => self.relations.notify(RelationsNotice::Captcha { action }),
        }
        None
    }

    /// Les présences des amis restent dans le magasin des présences (page Amis).
    pub(super) fn keep_friend_presences(&self) {
        self.people.lock().keep_presences(self.relations.friends().map(|r| r.id));
    }

    /// Relations d'un onglet de la page Amis ; « En ligne » d'après le magasin des présences.
    pub fn friend_list(&self, tab: FriendTab, query: &str) -> Vec<u64> {
        let people = self.people.lock();
        self.relations.list(tab, query, |id| people.peek_status(id).is_some_and(|s| s != Status::Offline))
    }

    /// MESSAGE_CREATE : dernier message du salon (mon message vaut lecture).
    pub(super) fn reads_on_message(&mut self, channel: u64, id: u64, mine: bool) {
        self.reads.on_message(channel, id, mine);
    }

    /// Salon muet maintenant (lui, sa catégorie ou son serveur ; pour un MP,
    /// le réglage des messages privés), d'après les réglages du compte.
    pub(super) fn channel_muted(&self, channel: u64) -> bool {
        let guild = self.chan_guild.get(&channel).copied();
        let parent = guild
            .and_then(|g| self.guilds.iter().find(|x| x.id == g))
            .and_then(|g| g.channels.iter().find(|c| c.id == channel))
            .and_then(|c| c.parent);
        self.account.notifs.muted(guild, channel, parent, super::now_ms())
    }

    /// Salon ou MP supprimé : plus rien à accuser.
    pub(super) fn relations_forget(&mut self, channel: u64) {
        self.reads.forget(channel);
        self.relations.forget_dm(channel);
    }

    /// L'utilisateur lit vraiment ce salon (fenêtre active, bas de la liste
    /// visible) : l'accusé jusqu'au dernier message partira après
    /// `reads::ACK_DELAY`, regroupé avec ce qui arrive entre-temps.
    pub fn note_reading(&mut self, channel: u64, now: Instant) {
        let Some(cs) = self.chans.get(&channel) else { return };
        if !cs.loaded || cs.newer {
            return;
        }
        let last = cs.msgs.back().map_or(0, |m| m.id).max(self.reads.head(channel).unwrap_or(0));
        self.reads.want(channel, last, now);
    }

    /// Envoie les accusés arrivés à échéance ; renvoie la prochaine échéance
    /// (l'interface se réveille à ce moment-là, pas avant).
    pub fn flush_acks(&mut self, now: Instant) -> Option<Instant> {
        for (channel_id, message_id) in self.reads.take_due(now) {
            self.outbox.push(Cmd::Relations(RelationsCmd::Ack { channel_id, message_id }));
        }
        self.reads.next_due()
    }

    /// « Marquer comme lu » sur un salon ou un MP.
    pub fn mark_read(&mut self, channel: u64) {
        self.clear_unread(channel);
        if let Some(message_id) = self.reads.take_now(channel) {
            self.outbox.push(Cmd::Relations(RelationsCmd::Ack { channel_id: channel, message_id }));
        }
    }

    /// Le bandeau et la ligne des nouveaux messages de ce salon sont écartés.
    pub fn dismiss_new_marker(&mut self, channel: u64) {
        if let Some(cs) = self.chans.get_mut(&channel)
            && cs.new_marker.take().is_some()
        {
            cs.rev.bump();
        }
    }

    /// « Marquer comme non lu » n'a de sens qu'avec des états de lecture.
    pub fn can_mark_unread(&self) -> bool {
        self.reads.synced
    }

    /// « Marquer comme non lu » depuis le message `id` (lot M) : l'accusé
    /// recule juste avant lui, la ligne « Nouveau » se pose dessus, et le
    /// salon reste non lu tant qu'on ne l'a ni quitté ni rouvert. `false`
    /// si le compte n'a pas d'états de lecture.
    pub fn mark_message_unread(&mut self, channel: u64, id: u64) -> bool {
        if !self.reads.unack(channel, id) {
            return false;
        }
        let me = self.me.as_ref().map_or(0, |u| u.id);
        let mentions = self.chans.get(&channel).map_or(0, |cs| cs.msgs.iter().filter(|m| m.id >= id && m.author_id != me && (m.mention_everyone || m.mentions.iter().any(|(u, _)| *u == me))).count());
        let mentions = u32::try_from(mentions).unwrap_or(u32::MAX);
        self.clear_unread(channel);
        self.mark_unread(channel, mentions > 0);
        self.unread_from.insert(channel, id);
        if let Some(cs) = self.chans.get_mut(&channel) {
            cs.new_marker = Some(id);
            cs.rev.bump();
        }
        self.outbox.push(Cmd::Relations(RelationsCmd::Unack { channel_id: channel, message_id: id, mentions }));
        true
    }

    /// « Marquer le serveur comme lu » : un accusé groupé pour tous ses salons
    /// qui en ont besoin (par lots de `ACK_BULK_MAX`).
    pub fn mark_guild_read(&mut self, guild: u64) {
        let chans: Vec<u64> = self
            .guilds
            .iter()
            .find(|g| g.id == guild)
            .map(|g| g.channels.iter().filter(|c| c.kind != ChannelKind::Category).map(|c| c.id).collect())
            .unwrap_or_default();
        let mut acks = Vec::new();
        for c in chans {
            self.clear_unread(c);
            if let Some(m) = self.reads.take_now(c) {
                acks.push((c, m));
            }
        }
        for chunk in acks.chunks(ACK_BULK_MAX) {
            self.outbox.push(Cmd::Relations(RelationsCmd::AckBulk(chunk.to_vec())));
        }
    }

    /// « Message » sur un ami : ouvre le MP s'il existe, sinon le demande
    /// (il s'ouvre à son arrivée). Renvoie `true` s'il est ouvert tout de suite.
    pub fn open_dm_with(&mut self, user_id: u64) -> bool {
        if let Some(c) = self.relations.dm_with(user_id).filter(|c| self.dms.iter().any(|d| d.id == *c)) {
            self.select_dms();
            self.select_channel(c);
            return true;
        }
        self.relations.opening = Some(user_id);
        self.outbox.push(Cmd::Relations(RelationsCmd::OpenDm { user_id }));
        false
    }

    /// Écriture demandée par l'utilisateur (un clic, confirmé s'il le faut).
    pub fn relations_write(&mut self, cmd: RelationsCmd) {
        self.outbox.push(Cmd::Relations(cmd));
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::model::{Channel, Dm, Guild, Message, User};
    use super::super::super::relations::{DmInfo, ReadState, Relation, RelKind, RelationsAction};
    use super::super::super::{Event, Problem};
    use super::*;

    const G: u64 = 9;
    const A: u64 = 100;
    const B: u64 = 101;
    const DM: u64 = 200;
    const ME: u64 = 1;
    const FRIEND: u64 = 2;

    fn ready(reads: Option<Vec<ReadState>>) -> Store {
        let mut s = Store::new();
        let chans = [A, B].map(|id| Channel { id, name: id.to_string(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 });
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![Guild::new(G, "g".into(), chans.to_vec())], dms: vec![Dm::new(DM, "ami".into(), 50)] });
        s.apply(Event::Relations(RelationsEvent::Ready {
            relations: vec![Relation { id: FRIEND, kind: RelKind::Friend, nickname: None, ignored: false, user: None }],
            people: vec![],
            dms: vec![DmInfo { id: DM, group: false, owner: None, recipients: vec![FRIEND], ..Default::default() }],
            reads,
            heads: vec![(A, 20), (B, 30), (DM, 50)],
        }));
        s
    }

    fn msg(id: u64, channel_id: u64, author_id: u64) -> Message {
        Message {
            id,
            channel_id,
            guild_id: (channel_id != DM).then_some(G),
            author_id,
            author: String::new(),
            author_avatar: crate::state::model::Avatar::default_for(author_id),
            content: "x".into(),
            attachments: vec![],
            embeds: vec![],
            mentions: vec![],
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    fn acks(s: &mut Store) -> Vec<RelationsCmd> {
        s.take_commands().into_iter().filter_map(|c| if let Cmd::Relations(c) = c { Some(c) } else { None }).collect()
    }

    /// Serveur muet (lot C1) : les états de lecture de Discord ne rendent non
    /// lus que les salons où l'on est mentionné.
    #[test]
    fn muted_guild_read_states_keep_only_mentions() {
        use super::super::super::usersettings::notif::{Change, Mute};
        let mut s = Store::new();
        let chans = [A, B].map(|id| Channel { id, name: id.to_string(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 });
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![Guild::new(G, "g".into(), chans.to_vec())], dms: Vec::new() });
        s.account.change_notif(Some(G), None, Change::Mute(Mute { on: true, until_ms: None }), -1).unwrap_or_else(|e| panic!("{e:?}"));
        s.apply(Event::Relations(RelationsEvent::Ready {
            relations: vec![],
            people: vec![],
            dms: vec![],
            reads: Some(vec![ReadState { channel_id: A, last_acked: 10, mentions: 0 }, ReadState { channel_id: B, last_acked: 10, mentions: 2 }]),
            heads: vec![(A, 20), (B, 30)],
        }));
        assert!(!s.unread().contains(&A), "muet, sans mention");
        assert!(s.unread().contains(&B) && s.mentioned().contains(&B), "muet, mentionné");
        // Accusé d'un autre appareil qui laisse A non lu : toujours rien.
        s.apply(Event::Relations(RelationsEvent::Ack { channel_id: A, message_id: 15, mentions: Some(0) }));
        assert!(!s.unread().contains(&A));
    }

    #[test]
    fn ready_read_states_set_unread_and_mentions() {
        let s = ready(Some(vec![ReadState { channel_id: A, last_acked: 10, mentions: 2 }, ReadState { channel_id: B, last_acked: 30, mentions: 0 }, ReadState { channel_id: DM, last_acked: 40, mentions: 1 }]));
        // Pastille chiffrée (lot N) : le compte de Discord, 0 pour un salon sans mention.
        assert_eq!((s.mention_count(A), s.mention_count(B)), (if s.mentioned().contains(&A) { 2 } else { 0 }, 0));
        assert!(s.unread().contains(&A) && s.mentioned().contains(&A));
        assert!(!s.unread().contains(&B));
        assert!(s.unread().contains(&DM) && s.mentioned().contains(&DM));
        assert_eq!(s.guild_has_unread(&s.guilds[0]), (true, true));
    }

    #[test]
    fn ack_from_another_device_clears_or_restores() {
        let mut s = ready(Some(vec![ReadState { channel_id: A, last_acked: 10, mentions: 1 }]));
        s.apply(Event::Relations(RelationsEvent::Ack { channel_id: A, message_id: 20, mentions: Some(0) }));
        assert!(!s.unread().contains(&A) && !s.mentioned().contains(&A));
        assert_eq!(s.guild_has_unread(&s.guilds[0]), (false, false));
        // Marqué non lu ailleurs.
        s.apply(Event::Relations(RelationsEvent::Ack { channel_id: A, message_id: 15, mentions: Some(0) }));
        assert!(s.unread().contains(&A) && !s.mentioned().contains(&A));
    }

    #[test]
    fn reading_sends_one_grouped_ack() {
        let t0 = Instant::now();
        let mut s = ready(Some(vec![ReadState { channel_id: A, last_acked: 10, mentions: 0 }]));
        s.select_guild(G);
        s.apply(Event::History { channel_id: A, before: None, messages: vec![msg(20, A, FRIEND)] });
        s.take_commands();
        s.note_reading(A, t0);
        s.apply(Event::MessageCreate(msg(21, A, FRIEND)));
        s.note_reading(A, t0 + std::time::Duration::from_secs(1));
        assert_eq!(s.flush_acks(t0), Some(t0 + crate::state::reads::ACK_DELAY));
        assert!(acks(&mut s).is_empty());
        assert_eq!(s.flush_acks(t0 + crate::state::reads::ACK_DELAY), None);
        assert_eq!(acks(&mut s), vec![RelationsCmd::Ack { channel_id: A, message_id: 21 }]);
        // Rien de nouveau : rien n'est renvoyé.
        s.note_reading(A, t0 + crate::state::reads::ACK_DELAY * 2);
        assert_eq!(s.flush_acks(t0 + crate::state::reads::ACK_DELAY * 4), None);
        assert!(acks(&mut s).is_empty());
    }

    /// Mode discret (lot E3) : aucun accusé ne part tout seul, le salon
    /// ouvert reste non lu, « Marquer comme lu » accuse d'un clic ; coupé,
    /// le comportement d'origine revient.
    #[test]
    fn manual_reading_never_acks_by_itself() {
        let t0 = Instant::now();
        let late = crate::state::reads::ACK_DELAY * 4;
        let mut s = ready(Some(vec![ReadState { channel_id: A, last_acked: 10, mentions: 0 }]));
        s.reads.set_manual(true);
        s.select_guild(G);
        s.select_channel(A);
        s.apply(Event::History { channel_id: A, before: None, messages: vec![msg(20, A, FRIEND)] });
        s.take_commands();
        assert!(s.unread().contains(&A), "ouvert, toujours non lu");
        s.note_reading(A, t0);
        s.apply(Event::MessageCreate(msg(21, A, FRIEND)));
        s.note_reading(A, t0 + std::time::Duration::from_secs(1));
        assert_eq!(s.flush_acks(t0 + late), None);
        assert!(acks(&mut s).is_empty(), "aucun accusé automatique");
        assert!(s.unread().contains(&A) && s.guild_has_unread(&s.guilds[0]).0);
        // Le clic : un accusé, les pastilles s'éteignent.
        s.mark_read(A);
        assert_eq!(acks(&mut s), vec![RelationsCmd::Ack { channel_id: A, message_id: 21 }]);
        assert!(!s.unread().contains(&A) && !s.guild_has_unread(&s.guilds[0]).0);
        // Un message reçu salon ouvert le rend de nouveau non lu, sans accusé.
        s.apply(Event::MessageCreate(msg(22, A, FRIEND)));
        s.note_reading(A, t0 + late);
        assert_eq!(s.flush_acks(t0 + late * 2), None);
        assert!(acks(&mut s).is_empty() && s.unread().contains(&A));
        // Mon message vaut lecture (Discord accuse à l'envoi) : pastille éteinte, toujours aucune requête.
        s.apply(Event::MessageCreate(msg(23, A, ME)));
        assert!(acks(&mut s).is_empty() && !s.unread().contains(&A));
        // Un accusé en attente est abandonné quand le mode s'active.
        s.reads.set_manual(false);
        s.apply(Event::MessageCreate(msg(24, A, FRIEND)));
        assert!(!s.unread().contains(&A), "coupé : le salon ouvert est lu");
        s.note_reading(A, t0 + late * 2);
        s.reads.set_manual(true);
        assert_eq!(s.flush_acks(t0 + late * 4), None);
        assert!(acks(&mut s).is_empty());
        // Coupé : l'accusé groupé d'origine.
        s.reads.set_manual(false);
        s.note_reading(A, t0 + late * 4);
        s.flush_acks(t0 + late * 6);
        assert_eq!(acks(&mut s), vec![RelationsCmd::Ack { channel_id: A, message_id: 24 }]);
    }

    #[test]
    fn mark_unread_moves_the_ack_back_and_holds_it() {
        let t0 = Instant::now();
        let mut s = ready(Some(vec![ReadState { channel_id: A, last_acked: 20, mentions: 0 }]));
        s.select_guild(G);
        s.select_channel(A);
        let mut pinged = msg(21, A, FRIEND);
        pinged.mentions = vec![(ME, "moi".into())];
        s.apply(Event::History { channel_id: A, before: None, messages: vec![msg(19, A, FRIEND), msg(20, A, FRIEND), pinged] });
        s.take_commands();
        assert!(s.mark_message_unread(A, 20));
        assert_eq!(acks(&mut s), vec![RelationsCmd::Unack { channel_id: A, message_id: 20, mentions: 1 }]);
        assert!(s.unread().contains(&A) && s.mentioned().contains(&A));
        assert_eq!(s.chans[&A].new_marker, Some(20));
        // Le salon reste affiché : aucun accusé ne repart.
        s.note_reading(A, t0);
        assert_eq!(s.flush_acks(t0 + crate::state::reads::ACK_DELAY), None);
        assert!(acks(&mut s).is_empty());
        // Rouvert : il se relit.
        s.select_channel(A);
        s.note_reading(A, t0);
        s.flush_acks(t0 + crate::state::reads::ACK_DELAY);
        assert_eq!(acks(&mut s), vec![RelationsCmd::Ack { channel_id: A, message_id: 21 }]);
    }

    #[test]
    fn mark_read_channel_and_guild() {
        let mut s = ready(Some(vec![ReadState { channel_id: A, last_acked: 10, mentions: 0 }, ReadState { channel_id: B, last_acked: 10, mentions: 3 }]));
        s.mark_read(A);
        assert_eq!(acks(&mut s), vec![RelationsCmd::Ack { channel_id: A, message_id: 20 }]);
        assert!(!s.unread().contains(&A));
        s.mark_guild_read(G);
        assert_eq!(acks(&mut s), vec![RelationsCmd::AckBulk(vec![(B, 30)])]);
        assert_eq!(s.guild_has_unread(&s.guilds[0]), (false, false));
        s.mark_guild_read(G);
        assert!(acks(&mut s).is_empty(), "rien à accuser");
    }

    #[test]
    fn bot_ready_keeps_local_unread_and_sends_nothing() {
        let mut s = ready(None);
        s.apply(Event::MessageCreate(msg(25, A, FRIEND)));
        assert!(s.unread().contains(&A));
        s.mark_read(A);
        assert!(!s.unread().contains(&A));
        assert!(acks(&mut s).is_empty());
    }

    #[test]
    fn open_dm_existing_or_requested() {
        let mut s = ready(Some(vec![]));
        assert!(s.open_dm_with(FRIEND));
        assert_eq!(s.channel, Some(DM));
        s.take_commands();
        assert!(!s.open_dm_with(7));
        assert_eq!(acks(&mut s), vec![RelationsCmd::OpenDm { user_id: 7 }]);
        // Le MP arrive : il n'est ouvert que s'il répond à la demande.
        s.apply(Event::DmUpsert { dm: Dm::new(300, "x".into(), 300), known: crate::state::DmKnown::ALL });
        s.apply(Event::Relations(RelationsEvent::DmOpened { channel_id: 300, user_id: Some(8) }));
        assert_eq!(s.channel, Some(DM));
        assert_eq!(s.apply(Event::Relations(RelationsEvent::DmOpened { channel_id: 300, user_id: Some(7) })), Some(Effect::ScrollToBottom));
        assert_eq!(s.channel, Some(300));
        // Échec : une notification, plus d'ouverture en attente.
        s.open_dm_with(9);
        s.apply(Event::Relations(RelationsEvent::Failed { action: RelationsAction::OpenDm, problem: Problem::Forbidden }));
        assert!(s.relations.opening.is_none());
        assert!(matches!(s.relations.take_notice(), Some(RelationsNotice::Failed { action: RelationsAction::OpenDm, .. })));
    }

    #[test]
    fn deleted_channel_is_forgotten() {
        let mut s = ready(Some(vec![ReadState { channel_id: DM, last_acked: 10, mentions: 0 }]));
        s.apply(Event::ChannelDelete { guild_id: None, id: DM });
        assert!(s.relations.dm_info(DM).is_none());
        assert_eq!(s.reads.head(DM), None);
    }
}
