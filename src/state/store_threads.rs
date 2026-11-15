//! Raccord du magasin avec les fils et les forums (`state::threads`, lot
//! T10). Module enfant de `store` (déclaré par `#[path]`) : les permissions
//! d'un fil et ses non-lus passent par les mêmes fonctions privées que ceux
//! des salons.

use super::super::event::Cmd;
use super::super::model::{ChannelKind, Message};
use super::super::notice::Problem;
use super::super::perms::{Permissions, Verdict};
use super::super::threads::{ThreadAction, ThreadCmd, ThreadEvent, MAX_NAME};
use super::{Effect, Store};

/// Longueur maximale d'un message (documentée : 2000 caractères).
const MAX_POST: usize = 2000;

impl Store {
    pub(super) fn apply_threads(&mut self, e: ThreadEvent) -> Option<Effect> {
        let keep = self.channel;
        match e {
            ThreadEvent::Upsert { thread, joined } => {
                let id = thread.id;
                for action in [ThreadAction::Archive, ThreadAction::Join, ThreadAction::Leave] {
                    self.threads.end(action, id);
                }
                let g = self.threads.upsert(thread, joined, keep);
                self.patch_stub(id);
                return self.threads_changed(g);
            }
            ThreadEvent::Delete { id } => {
                let t = self.threads.remove(id)?;
                self.clear_unread(id);
                self.chans.remove(&id);
                if self.channel == Some(id) {
                    self.select_channel(t.parent_id);
                }
                self.recompute_guild(t.guild_id);
                return self.after_permissions_change();
            }
            ThreadEvent::Sync { guild_id, parents, threads, joined } => {
                self.threads.sync(guild_id, parents.as_deref(), threads, &joined, keep);
                self.patch_all_stubs();
                return self.threads_changed(guild_id);
            }
            ThreadEvent::Joined { thread_id, joined } => {
                self.threads.end(if joined { ThreadAction::Join } else { ThreadAction::Leave }, thread_id);
                if let Some(g) = self.threads.set_joined(thread_id, joined) {
                    if !joined {
                        self.clear_unread(thread_id);
                    }
                    return self.threads_changed(g);
                }
            }
            ThreadEvent::Members { thread_id, count } => self.threads.set_members(thread_id, count),
            ThreadEvent::Forums { guild_id, forums } => self.threads.set_forums(guild_id, forums),
            ThreadEvent::Forum(f) => self.threads.set_forum(f),
            ThreadEvent::Topics { guild_id, topics } => self.threads.set_topics(guild_id, topics),
            ThreadEvent::Topic { guild_id, channel_id, topic } => self.threads.set_topic(guild_id, channel_id, topic),
            ThreadEvent::AgeGates { guild_id, channels } => self.threads.set_age_gates(guild_id, channels),
            ThreadEvent::AgeGate { guild_id, channel_id, on } => self.threads.set_age_gate(guild_id, channel_id, on),
            ThreadEvent::Archived { parent_id, before, threads, joined, more } => {
                self.threads.archived_page(parent_id, before, threads, &joined, more, keep);
                self.patch_all_stubs();
                if let Some(g) = self.chan_guild.get(&parent_id).copied() {
                    return self.threads_changed(g);
                }
            }
            ThreadEvent::Starter { thread_id, starter } => self.threads.set_starter(thread_id, starter),
            ThreadEvent::Created { thread_id } => {
                self.threads.end(ThreadAction::Create, 0);
                self.threads.end(ThreadAction::Post, 0);
                self.threads.created += 1;
                if self.threads.get(thread_id).is_some() {
                    self.select_channel(thread_id);
                }
            }
            ThreadEvent::Notifs(list) => {
                for (id, flags) in list {
                    self.threads.end(ThreadAction::Notify, id);
                    self.threads.set_notif(id, flags);
                }
            }
            ThreadEvent::MemberList { thread_id, members } => {
                self.threads.end(ThreadAction::Members, thread_id);
                self.threads.set_member_list(thread_id, members);
            }
            ThreadEvent::Failed { action, thread_id, problem } => {
                let id = thread_id.unwrap_or(0);
                self.threads.end(action, id);
                if action == ThreadAction::Members {
                    self.threads.forget_members();
                }
                if action == ThreadAction::Archived {
                    self.threads.archived_failed(id, problem.clone());
                }
                self.thread_failure = Some((action, problem));
            }
        }
        None
    }

    /// Permissions des fils d'un serveur refaites (appartenance, verrou, parent).
    fn threads_changed(&mut self, guild: u64) -> Option<Effect> {
        self.recompute_guild(guild);
        self.after_permissions_change()
    }

    /// Fil né d'un message : son résumé sous le message, s'il est en mémoire.
    fn patch_stub(&mut self, thread_id: u64) {
        let Some(t) = self.threads.get(thread_id) else { return };
        let stub = t.stub();
        let Some(cs) = self.chans.get_mut(&t.parent_id) else { return };
        if let Ok(i) = cs.msgs.binary_search_by_key(&thread_id, |m| m.id)
            && let Some(m) = cs.msgs.get_mut(i)
            && m.meta.thread.as_deref() != Some(&stub)
        {
            m.meta.thread = Some(Box::new(stub));
            cs.rev.bump();
        }
    }

    /// Résumés de tous les messages en mémoire (après une resynchronisation) :
    /// parcourt les messages gardés (borné par le cache), pas les fils.
    fn patch_all_stubs(&mut self) {
        let threads = &self.threads;
        for (cid, cs) in self.chans.iter_mut() {
            let mut changed = false;
            for m in cs.msgs.iter_mut() {
                // Un fil inconnu ici garde le résumé que le message apportait.
                let Some(stub) = threads.get(m.id).filter(|t| t.parent_id == *cid).map(|t| Box::new(t.stub())) else { continue };
                if m.meta.thread.as_ref() != Some(&stub) {
                    m.meta.thread = Some(stub);
                    changed = true;
                }
            }
            if changed {
                cs.rev.bump();
            }
        }
    }

    /// Message reçu (MESSAGE_CREATE) : compte du fil, et résumé si c'est le
    /// message de départ d'un fil connu.
    pub(super) fn thread_on_message(&mut self, m: &mut Message) {
        if self.threads.note_message(m.channel_id, m.id, 1).is_some() {
            self.patch_stub(m.channel_id);
        }
        self.thread_stub(m);
    }

    /// Résumé du fil né de ce message, s'il est connu du magasin.
    pub(super) fn thread_stub(&self, m: &mut Message) {
        if let Some(t) = self.threads.get(m.id).filter(|t| t.parent_id == m.channel_id) {
            m.meta.thread = Some(Box::new(t.stub()));
        }
    }

    /// Un message dans ce salon le rend-il non lu ? Pas dans un fil que je
    /// n'ai pas rejoint (comme Discord).
    pub(super) fn thread_counts_unread(&self, channel: u64) -> bool {
        self.threads.get(channel).is_none_or(|t| t.joined)
    }

    /// Salon forum ou média.
    pub fn is_forum(&self, id: u64) -> bool {
        self.chan_guild
            .get(&id)
            .and_then(|g| self.guilds.iter().find(|x| x.id == *g))
            .and_then(|g| g.channels.iter().find(|c| c.id == id))
            .is_some_and(|c| c.kind.is_forum())
    }

    pub fn is_thread(&self, id: u64) -> bool {
        self.threads.get(id).is_some()
    }

    /// Salon dont l'en-tête propose le panneau « Fils » : textuel ou d'annonces
    /// (un forum liste déjà ses publications).
    pub fn lists_threads(&self, id: u64) -> bool {
        self.guilds.iter().flat_map(|g| g.channels.iter()).any(|c| c.id == id && matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement))
    }

    /// « Marquer comme lu » d'une catégorie : un accusé groupé pour ses
    /// salons et leurs fils qui en ont besoin (comme pour un serveur).
    pub fn mark_category_read(&mut self, guild: u64, category: u64) {
        let Some(g) = self.guilds.iter().find(|g| g.id == guild) else { return };
        let mut ids: Vec<u64> = g.channels.iter().filter(|c| c.parent == Some(category)).map(|c| c.id).collect();
        let threads: Vec<u64> = ids.iter().flat_map(|c| self.threads.children(*c).iter().copied()).collect();
        ids.extend(threads);
        let mut acks = Vec::new();
        for c in ids {
            self.clear_unread(c);
            if let Some(m) = self.reads.take_now(c) {
                acks.push((c, m));
            }
        }
        for chunk in acks.chunks(super::relations_glue::ACK_BULK_MAX) {
            self.outbox.push(Cmd::Relations(super::super::relations::RelationsCmd::AckBulk(chunk.to_vec())));
        }
    }

    /// Échec d'une action sur un fil, à annoncer une fois.
    pub fn take_thread_failure(&mut self) -> Option<(ThreadAction, Problem)> {
        self.thread_failure.take()
    }

    fn thread_cmd(&mut self, action: ThreadAction, id: u64, cmd: ThreadCmd) -> bool {
        if !self.threads.begin(action, id) {
            return false;
        }
        self.outbox.push(Cmd::Threads(cmd));
        true
    }

    /// Rejoindre (`true`) ou quitter un fil.
    pub fn set_thread_joined(&mut self, thread_id: u64, join: bool) -> bool {
        let Some(t) = self.threads.get(thread_id) else { return false };
        if t.joined == join {
            return false;
        }
        if join {
            self.thread_cmd(ThreadAction::Join, thread_id, ThreadCmd::Join { thread_id })
        } else {
            self.thread_cmd(ThreadAction::Leave, thread_id, ThreadCmd::Leave { thread_id })
        }
    }

    /// Règle les notifications d'un fil dont je suis membre (« tous les
    /// messages », « mentions », « rien »). `false` : rien n'est parti.
    pub fn set_thread_notif(&mut self, thread_id: u64, level: crate::state::usersettings::notif::Level) -> bool {
        let Some(flags) = super::super::threads::notif_flags(level) else { return false };
        if !self.threads.get(thread_id).is_some_and(|t| t.joined) || self.threads.notif(thread_id) == Some(level) {
            return false;
        }
        self.thread_cmd(ThreadAction::Notify, thread_id, ThreadCmd::Notify { thread_id, flags })
    }

    /// Puis-je archiver ce fil ? Son créateur, ou qui a MANAGE_THREADS ;
    /// verrouillé, seul ce dernier peut le désarchiver.
    pub fn can_archive_thread(&self, thread_id: u64) -> bool {
        let Some(t) = self.threads.get(thread_id) else { return false };
        let manage = self.can(t.parent_id, Permissions::MANAGE_THREADS) == Verdict::Permis;
        let owner = self.me.as_ref().is_some_and(|u| Some(u.id) == t.owner_id);
        manage || (owner && !t.locked)
    }

    pub fn set_thread_archived(&mut self, thread_id: u64, archived: bool) -> bool {
        if !self.can_archive_thread(thread_id) || self.threads.get(thread_id).is_none_or(|t| t.archived == archived) {
            return false;
        }
        self.thread_cmd(ThreadAction::Archive, thread_id, ThreadCmd::SetArchived { thread_id, archived })
    }

    /// Puis-je verrouiller ce fil ? Réservé à qui a MANAGE_THREADS.
    pub fn can_lock_thread(&self, thread_id: u64) -> bool {
        self.threads.get(thread_id).is_some_and(|t| self.can(t.parent_id, Permissions::MANAGE_THREADS) == Verdict::Permis)
    }

    /// Verrouille ou déverrouille un fil ; `false` si rien ne part.
    pub fn set_thread_locked(&mut self, thread_id: u64, locked: bool) -> bool {
        if !self.can_lock_thread(thread_id) || self.threads.get(thread_id).is_none_or(|t| t.locked == locked) {
            return false;
        }
        self.thread_cmd(ThreadAction::Lock, thread_id, ThreadCmd::SetLocked { thread_id, locked })
    }

    /// Ouverture de la fenêtre « Membres du fil » : une lecture, par geste.
    pub fn load_thread_members(&mut self, thread_id: u64) -> bool {
        if self.threads.get(thread_id).is_none() {
            return false;
        }
        self.threads.ask_members(thread_id);
        self.thread_cmd(ThreadAction::Members, thread_id, ThreadCmd::Members { thread_id })
    }

    /// Puis-je créer un fil privé dans ce salon ? (Pas dans un salon d'annonces.)
    pub fn can_create_private_thread(&self, channel_id: u64) -> bool {
        self.can_create_thread(channel_id) && !self.is_announcement(channel_id) && self.can(channel_id, Permissions::CREATE_PRIVATE_THREADS) != Verdict::Refuse
    }

    /// Crée un fil privé sans message d'origine ; `false` si rien ne part.
    pub fn create_private_thread(&mut self, channel_id: u64, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME || !self.can_create_private_thread(channel_id) {
            return false;
        }
        self.thread_cmd(ThreadAction::Create, 0, ThreadCmd::CreatePrivate { channel_id, name: name.to_string() })
    }

    /// Carte d'une publication affichée en galerie : son message de départ
    /// est demandé une fois (au plus quelques demandes à la fois), seulement
    /// si l'historique du forum m'est lisible. `true` si la demande part.
    pub fn want_starter(&mut self, thread_id: u64) -> bool {
        let Some(parent) = self.threads.get(thread_id).map(|t| t.parent_id) else { return false };
        if self.threads.forum(parent).is_none() || self.can(parent, Permissions::READ_MESSAGE_HISTORY) == Verdict::Refuse || !self.threads.ask_starter(thread_id) {
            return false;
        }
        self.outbox.push(Cmd::Threads(ThreadCmd::Starter { thread_id }));
        true
    }

    /// Page suivante des publications archivées d'un forum (ou des fils archivés d'un salon).
    pub fn load_archived_threads(&mut self, parent_id: u64) -> bool {
        if self.can(parent_id, Permissions::READ_MESSAGE_HISTORY) == Verdict::Refuse {
            return false;
        }
        let Some(before) = self.threads.ask_archived(parent_id) else { return false };
        self.outbox.push(Cmd::Threads(ThreadCmd::Archived { parent_id, before }));
        true
    }

    /// Puis-je créer un fil public depuis un message de ce salon ?
    pub fn can_create_thread(&self, channel_id: u64) -> bool {
        let kind = self.chan_guild.get(&channel_id).and_then(|g| self.guilds.iter().find(|x| x.id == *g)).and_then(|g| g.channels.iter().find(|c| c.id == channel_id)).map(|c| c.kind);
        matches!(kind, Some(ChannelKind::Text | ChannelKind::Announcement))
            && self.can(channel_id, Permissions::CREATE_PUBLIC_THREADS) != Verdict::Refuse
            && self.can(channel_id, Permissions::SEND_MESSAGES_IN_THREADS) != Verdict::Refuse
    }

    /// Crée un fil public à partir d'un message (ou sans message d'origine :
    /// `message_id` nul) ; `false` si rien ne part.
    pub fn create_thread(&mut self, channel_id: u64, message_id: u64, name: &str) -> bool {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > MAX_NAME || !self.can_create_thread(channel_id) || self.threads.get(message_id).is_some() {
            return false;
        }
        if message_id == 0 {
            let announcement = self.is_announcement(channel_id);
            return self.thread_cmd(ThreadAction::Create, 0, ThreadCmd::CreateStandalone { channel_id, name: name.to_string(), announcement });
        }
        self.thread_cmd(ThreadAction::Create, 0, ThreadCmd::CreateFromMessage { channel_id, message_id, name: name.to_string() })
    }

    /// Puis-je publier dans ce forum ? (SEND_MESSAGES y donne le droit de créer une publication.)
    pub fn can_post(&self, forum_id: u64) -> bool {
        self.is_forum(forum_id) && self.can(forum_id, Permissions::SEND_MESSAGES) != Verdict::Refuse
    }

    /// Crée une publication de forum ; `false` si rien ne part.
    pub fn create_post(&mut self, forum_id: u64, name: &str, content: &str, tags: Vec<u64>) -> bool {
        let (name, content) = (name.trim(), content.trim());
        if name.is_empty() || content.is_empty() || name.chars().count() > MAX_NAME || content.chars().count() > MAX_POST || !self.can_post(forum_id) {
            return false;
        }
        let known: Vec<u64> = self.threads.forum(forum_id).map(|f| f.tags.iter().map(|t| t.id).collect()).unwrap_or_default();
        // Documenté : cinq étiquettes au plus.
        let tags: Vec<u64> = tags.into_iter().filter(|t| known.contains(t)).take(5).collect();
        self.thread_cmd(ThreadAction::Post, 0, ThreadCmd::CreatePost { forum_id, name: name.to_string(), content: content.to_string(), tags })
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::model::{Channel, ChannelKind, Guild, Member, Role, User};
    use super::super::super::threads::{Thread, ThreadKind};
    use super::*;
    use crate::state::Event;

    const G: u64 = 1;
    const TEXT: u64 = 2;
    const FORUM: u64 = 3;
    const ME: u64 = 7;

    fn thread(id: u64, parent: u64) -> Thread {
        Thread {
            id,
            guild_id: G,
            parent_id: parent,
            name: format!("fil {id}"),
            kind: ThreadKind::Public,
            owner_id: Some(ME),
            archived: false,
            locked: false,
            auto_archive: 1440,
            archived_at: None,
            created_at: 0,
            message_count: 0,
            member_count: 1,
            applied_tags: Vec::new(),
            last_message_id: None,
            slowmode: 0,
            pinned: false,
            joined: false,
        }
    }

    fn store(everyone: Permissions) -> Store {
        let ch = |id, kind| Channel { id, name: format!("s{id}"), kind, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        let mut g = Guild::new(G, "g".into(), vec![ch(TEXT, ChannelKind::Text), ch(FORUM, ChannelKind::Forum)]);
        g.owner_id = Some(99);
        g.roles = vec![Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: everyone, mentionable: false }];
        g.me = Some(Member::default());
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![g], dms: Vec::new() });
        s.guild = Some(G);
        s
    }

    fn msg(id: u64, channel: u64) -> Message {
        Message {
            id,
            channel_id: channel,
            guild_id: Some(G),
            author_id: 8,
            author: "a".into(),
            author_avatar: crate::state::model::Avatar::default_for(8),
            content: "x".into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    use Permissions as P;
    const READ: P = P(P::VIEW_CHANNEL.0 | P::READ_MESSAGE_HISTORY.0 | P::SEND_MESSAGES.0);

    #[test]
    fn thread_is_a_channel_with_inherited_permissions() {
        let mut s = store(READ);
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10, TEXT)], joined: vec![] }));
        assert!(s.channel_exists(10));
        assert_eq!(s.channel_name(10), Some("fil 10"));
        // Pas de SEND_MESSAGES_IN_THREADS : lecture seule.
        assert_eq!(s.can(10, P::VIEW_CHANNEL), Verdict::Permis);
        assert!(s.send_block(10).is_some());
        // Rôle modifié : les fils suivent.
        s.apply(Event::RoleUpsert { guild_id: G, role: Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: P(READ.0 | P::SEND_MESSAGES_IN_THREADS.0), mentionable: false } });
        assert!(s.send_block(10).is_none());
        // Fil privé non rejoint : masqué ; rejoint : visible.
        let mut private = thread(11, TEXT);
        private.kind = ThreadKind::Private;
        s.apply(Event::Threads(ThreadEvent::Upsert { thread: private, joined: None }));
        assert!(s.is_hidden(11));
        s.apply(Event::Threads(ThreadEvent::Joined { thread_id: 11, joined: true }));
        assert!(!s.is_hidden(11));
    }

    #[test]
    fn thread_notification_level_is_asked_once_and_follows_discord() {
        use crate::state::usersettings::notif::Level;
        let mut s = store(READ);
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10, TEXT), thread(11, TEXT)], joined: vec![11] }));
        let _ = s.take_commands();
        assert!(!s.set_thread_notif(10, Level::All), "fil non rejoint");
        assert!(!s.set_thread_notif(11, Level::Inherit), "pas de niveau hérité pour un fil");
        assert_eq!(s.threads.notif(11), None);
        assert!(s.set_thread_notif(11, Level::Mentions));
        assert!(!s.set_thread_notif(11, Level::Nothing), "une écriture à la fois");
        assert!(s.take_commands().contains(&Cmd::Threads(ThreadCmd::Notify { thread_id: 11, flags: 4 })));
        assert_eq!(s.threads.notif(11), None, "pas avant la réponse");
        s.apply(Event::Threads(ThreadEvent::Notifs(vec![(11, 5)])));
        assert_eq!(s.threads.notif(11), Some(Level::Mentions));
        assert!(!s.set_thread_notif(11, Level::Mentions), "déjà ce niveau");
        assert!(s.set_thread_notif(11, Level::Nothing));
        s.apply(Event::Threads(ThreadEvent::Delete { id: 11 }));
        assert_eq!(s.threads.notif(11), None, "oublié avec le fil");
    }

    #[test]
    fn lock_private_thread_and_member_list() {
        let base = READ | Permissions::CREATE_PUBLIC_THREADS | Permissions::SEND_MESSAGES_IN_THREADS;
        let mut s = store(base);
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10, TEXT)], joined: vec![10] }));
        assert!(!s.can_lock_thread(10) && !s.set_thread_locked(10, true), "sans MANAGE_THREADS");
        assert!(!s.can_create_private_thread(TEXT) && !s.create_private_thread(TEXT, "Secret"));
        // La liste des membres : une lecture, oubliée à la fermeture ou au refus.
        assert!(s.load_thread_members(10) && !s.load_thread_members(10), "une lecture à la fois");
        assert_eq!(s.threads.member_list(10), Some(None));
        s.apply(Event::Threads(ThreadEvent::MemberList { thread_id: 10, members: vec![(5, "eve".into())] }));
        assert_eq!(s.threads.member_list(10).flatten().map(<[_]>::len), Some(1));
        assert!(s.load_thread_members(10));
        s.apply(Event::Threads(ThreadEvent::Failed { action: ThreadAction::Members, thread_id: Some(10), problem: Problem::Forbidden }));
        assert_eq!(s.threads.member_list(10), None);
        let _ = s.take_commands();

        let mut s = store(base | Permissions::MANAGE_THREADS | Permissions::CREATE_PRIVATE_THREADS);
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10, TEXT)], joined: vec![10] }));
        assert!(!s.set_thread_locked(10, false), "déjà déverrouillé");
        assert!(s.set_thread_locked(10, true) && !s.set_thread_locked(10, true), "une écriture à la fois");
        assert!(s.create_private_thread(TEXT, " Secret "));
        assert_eq!(s.take_commands(), vec![Cmd::Threads(ThreadCmd::SetLocked { thread_id: 10, locked: true }), Cmd::Threads(ThreadCmd::CreatePrivate { channel_id: TEXT, name: "Secret".into() })]);
    }

    #[test]
    fn messages_count_and_unread_only_when_joined() {
        let mut s = store(READ);
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10 << 22, TEXT), thread(11 << 22, TEXT)], joined: vec![11 << 22] }));
        s.apply(Event::MessageCreate(msg(20 << 22, 10 << 22)));
        s.apply(Event::MessageCreate(msg(21 << 22, 11 << 22)));
        assert!(!s.unread().contains(&(10 << 22)), "fil non rejoint : pas de non-lu");
        assert!(s.unread().contains(&(11 << 22)));
        assert_eq!(s.threads.get(10 << 22).map(|t| t.message_count), Some(1));
    }

    #[test]
    fn starter_message_shows_its_thread_and_follows_updates() {
        let mut s = store(READ);
        s.select_channel(TEXT);
        s.apply(Event::History { channel_id: TEXT, before: None, messages: vec![msg(5 << 22, TEXT)] });
        let rev = s.chans[&TEXT].rev;
        s.apply(Event::Threads(ThreadEvent::Upsert { thread: thread(5 << 22, TEXT), joined: Some(true) }));
        let stub = s.chans[&TEXT].msgs[0].meta.thread.clone();
        assert_eq!(stub.map(|t| (t.id, t.messages)), Some((5 << 22, 0)));
        assert_ne!(s.chans[&TEXT].rev, rev, "le message est remesuré");
        s.apply(Event::MessageCreate(msg(6 << 22, 5 << 22)));
        assert_eq!(s.chans[&TEXT].msgs[0].meta.thread.as_ref().map(|t| t.messages), Some(1));
    }

    #[test]
    fn forums_have_no_history_and_are_not_opened_first() {
        let mut s = store(READ);
        s.guild = None;
        s.select_guild(G);
        assert_eq!(s.channel, Some(TEXT));
        s.take_commands();
        s.select_channel(FORUM);
        assert!(s.take_commands().is_empty(), "un forum n'a pas d'historique");
        assert!(s.is_forum(FORUM) && !s.is_forum(TEXT));
    }

    #[test]
    fn writes_are_single_and_follow_permissions() {
        let mut s = store(P(READ.0 | P::CREATE_PUBLIC_THREADS.0 | P::SEND_MESSAGES_IN_THREADS.0));
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10, TEXT)], joined: vec![] }));
        s.take_commands();
        assert!(s.set_thread_joined(10, true));
        assert!(!s.set_thread_joined(10, true), "une seule demande à la fois");
        assert_eq!(s.take_commands(), vec![Cmd::Threads(ThreadCmd::Join { thread_id: 10 })]);
        s.apply(Event::Threads(ThreadEvent::Joined { thread_id: 10, joined: true }));
        assert!(s.set_thread_joined(10, false));
        // Mon fil : je peux l'archiver.
        assert!(s.set_thread_archived(10, true));
        assert!(s.create_thread(TEXT, 50, "  Sujet  "));
        assert!(!s.create_thread(TEXT, 51, "Autre"), "une création à la fois");
        assert!(!s.create_thread(FORUM, 52, "x"), "pas de fil depuis un message de forum");
        let cmds = s.take_commands();
        assert!(cmds.contains(&Cmd::Threads(ThreadCmd::CreateFromMessage { channel_id: TEXT, message_id: 50, name: "Sujet".into() })));
        s.apply(Event::Threads(ThreadEvent::Failed { action: ThreadAction::Create, thread_id: None, problem: Problem::Forbidden }));
        assert_eq!(s.take_thread_failure(), Some((ThreadAction::Create, Problem::Forbidden)));
        assert!(s.create_thread(TEXT, 51, "Autre"), "après l'échec, on peut recommencer");
        // Publication : titre et message obligatoires.
        assert!(!s.create_post(FORUM, "", "texte", vec![]));
        assert!(s.create_post(FORUM, "Titre", "texte", vec![99]));
        let cmds = s.take_commands();
        assert!(cmds.contains(&Cmd::Threads(ThreadCmd::CreatePost { forum_id: FORUM, name: "Titre".into(), content: "texte".into(), tags: vec![] })), "étiquette inconnue retirée");
    }

    #[test]
    fn deleted_open_thread_returns_to_parent() {
        let mut s = store(READ);
        s.apply(Event::Threads(ThreadEvent::Sync { guild_id: G, parents: None, threads: vec![thread(10, TEXT)], joined: vec![10] }));
        s.select_channel(10);
        s.apply(Event::Threads(ThreadEvent::Delete { id: 10 }));
        assert_eq!(s.channel, Some(TEXT));
        assert!(!s.channel_exists(10));
    }
}
