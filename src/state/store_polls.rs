//! Raccord du magasin avec le lot T13 : sondages, phrases des messages
//! système, salons d'annonces, cartes d'invitation, adresses de pièces
//! jointes rafraîchies. Module enfant de `store` (déclaré par `#[path]`) :
//! les messages en mémoire et les permissions passent par ses fonctions.

use super::super::announce::{is_expired, AnnounceCmd, AnnounceEvent, AnnounceNotice, REFRESH_BATCH};
use super::super::event::Cmd;
use super::super::invites::{Invite, InviteCmd, InviteEvent, InviteState, JoinState};
use super::super::model::{ChannelKind, Message};
use super::super::notice::{Problem, SystemKind};
use super::super::perms::{Permissions, Verdict};
use super::super::polls::{Poll, PollAction, PollCmd, PollDraft, PollEvent, Voters};
use super::super::system_msgs::{self, Names};
use super::{now_ms, Store};

fn poll_mut(m: &mut Message) -> Option<&mut Poll> {
    m.meta.extra.as_mut()?.poll.as_mut()
}

impl Store {
    /// Phrase d'un message système dans la langue du magasin.
    pub(super) fn system_text(&self, kind: SystemKind, m: &Message) -> String {
        let me = self.me.as_ref().map(|u| u.id);
        let guild = m.guild_id.and_then(|g| self.guilds.iter().find(|x| x.id == g)).map(|g| g.name.as_str());
        system_msgs::text(self.lang, kind, m, Names { me, guild })
    }

    // ------------------------------------------------------------------ sondages

    pub(super) fn apply_polls(&mut self, e: PollEvent) {
        let me = self.me.as_ref().map(|u| u.id);
        match e {
            PollEvent::Vote { channel_id, message_id, answer_id, user_id, add } => {
                let mine = Some(user_id) == me;
                self.change_message(channel_id, message_id, |m| poll_mut(m).is_some_and(|p| p.apply_vote(answer_id, mine, add)));
                self.polls.forget_voters(message_id);
            }
            PollEvent::Replace { channel_id, message_id, poll } => {
                self.change_message(channel_id, message_id, |m| {
                    let Some(extra) = m.meta.extra.as_mut() else { return false };
                    let mut poll = *poll;
                    // Sans résultats, les comptes connus restent (la documentation
                    // dit de ne pas les lire comme des zéros).
                    if !poll.counts_known
                        && let Some(old) = &extra.poll
                    {
                        for a in &mut poll.answers {
                            if let Some(o) = old.answers.iter().find(|o| o.id == a.id) {
                                (a.count, a.me) = (o.count, o.me);
                            }
                        }
                        poll.counts_known = old.counts_known;
                    }
                    let changed = extra.poll.as_ref() != Some(&poll);
                    extra.poll = Some(poll);
                    changed
                });
            }
            PollEvent::VoteDone { channel_id, message_id, previous, error } => {
                self.polls.end_vote(message_id);
                if let Some(problem) = error {
                    self.change_message(channel_id, message_id, |m| poll_mut(m).is_some_and(|p| p.set_mine(&previous)));
                    self.polls.fail(PollAction::Vote, problem);
                }
            }
            PollEvent::Voters { message_id, answer_id, users } => {
                let v = match users {
                    Ok(u) => Voters::Ready(u),
                    Err(p) => Voters::Failed(p),
                };
                self.polls.set_voters((message_id, answer_id), v);
            }
            PollEvent::EndDone { message_id, error } => {
                self.polls.end_end(message_id);
                if let Some(p) = error {
                    self.polls.fail(PollAction::End, p);
                }
            }
            PollEvent::CreateDone { error, .. } => {
                self.polls.creating = false;
                match error {
                    Some(p) => self.polls.fail(PollAction::Create, p),
                    None => self.polls.created += 1,
                }
            }
        }
    }

    pub fn poll_of(&self, channel_id: u64, message_id: u64) -> Option<&Poll> {
        self.find_message(channel_id, message_id)?.meta.extra.as_ref()?.poll.as_ref()
    }

    /// Vote (ou retrait si `answers` est vide), affiché tout de suite et
    /// défait si Discord refuse. `false` si rien ne part.
    pub fn vote(&mut self, channel_id: u64, message_id: u64, mut answers: Vec<u32>) -> bool {
        let now = self.now();
        let Some(p) = self.poll_of(channel_id, message_id) else { return false };
        answers.retain(|a| p.answers.iter().any(|x| x.id == *a));
        answers.sort_unstable();
        answers.dedup();
        if p.ended(now) || (!p.multi && answers.len() > 1) {
            return false;
        }
        let previous = p.mine();
        if previous == answers || !self.polls.begin_vote(message_id) {
            return false;
        }
        self.change_message(channel_id, message_id, |m| poll_mut(m).is_some_and(|p| p.set_mine(&answers)));
        self.polls.forget_voters(message_id);
        self.outbox.push(Cmd::Polls(PollCmd::Vote { channel_id, message_id, answers, previous }));
        true
    }

    /// Votants d'une réponse, demandés une fois (gardés ensuite).
    pub fn request_voters(&mut self, channel_id: u64, message_id: u64, answer_id: u32) -> bool {
        if self.poll_of(channel_id, message_id).is_none() || !self.polls.want_voters((message_id, answer_id)) {
            return false;
        }
        self.outbox.push(Cmd::Polls(PollCmd::Voters { channel_id, message_id, answer_id }));
        true
    }

    /// Seul l'auteur d'un sondage peut le terminer (documenté).
    pub fn can_end_poll(&self, m: &Message) -> bool {
        let mine = self.me.as_ref().is_some_and(|u| u.id == m.author_id);
        mine && m.meta.extra.as_ref().and_then(|e| e.poll.as_ref()).is_some_and(|p| !p.ended(self.now()))
    }

    pub fn end_poll(&mut self, channel_id: u64, message_id: u64) -> bool {
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        if !self.can_end_poll(m) || !self.polls.begin_end(message_id) {
            return false;
        }
        self.outbox.push(Cmd::Polls(PollCmd::End { channel_id, message_id }));
        true
    }

    /// Puis-je créer un sondage ici ? (SEND_POLLS, et l'envoi permis.)
    pub fn can_create_poll(&self, channel_id: u64) -> bool {
        self.send_block(channel_id).is_none() && self.can(channel_id, Permissions::SEND_POLLS) != Verdict::Refuse && !self.is_forum(channel_id)
    }

    pub fn create_poll(&mut self, channel_id: u64, draft: PollDraft) -> bool {
        if !draft.valid() || !self.can_create_poll(channel_id) || self.polls.creating {
            return false;
        }
        self.polls.creating = true;
        let nonce = self.next_nonce();
        self.outbox.push(Cmd::Polls(PollCmd::Create { channel_id, draft, nonce }));
        true
    }

    pub fn take_poll_failure(&mut self) -> Option<(PollAction, Problem)> {
        self.polls.take_failure()
    }

    // ------------------------------------------------------------------ invitations

    pub(super) fn apply_invites(&mut self, e: InviteEvent) {
        match e {
            InviteEvent::Loaded { code, invite } => {
                let v = match invite {
                    Ok(i) => InviteState::Ready(i),
                    Err(p) => InviteState::Failed(p),
                };
                self.invites.set(&code, v);
            }
            InviteEvent::Invalid { code } => self.invites.set(&code, InviteState::Invalid),
        }
    }

    /// Invitation à demander (carte visible), une fois par code.
    pub fn want_invite(&mut self, code: &str) -> bool {
        if !self.invites.want(code) {
            return false;
        }
        self.outbox.push(Cmd::Invites(InviteCmd::Get { code: code.to_string() }));
        true
    }

    /// Suis-je déjà membre du serveur de cette invitation ?
    pub fn invite_joined(&self, inv: &Invite) -> bool {
        inv.guild_id.is_some_and(|g| self.guilds.iter().any(|x| x.id == g))
    }

    /// « Rejoindre » d'une carte (après un clic) : la demande passe par
    /// [`Store::join_invite`] (lot serveurs), qui l'annonce et l'ouvre.
    pub fn join_invite_card(&mut self, code: &str) -> bool {
        let Some(InviteState::Ready(inv)) = self.invites.get(code) else { return false };
        if inv.kind != 0 || self.invite_joined(inv) || self.invites.join_state(code).is_some() {
            return false;
        }
        if !self.join_invite(code) {
            return false;
        }
        self.invites.set_join(code, Some(JoinState::Joining));
        true
    }

    // ------------------------------------------------------------------ annonces

    pub(super) fn apply_announce(&mut self, e: AnnounceEvent) {
        match e {
            AnnounceEvent::Flags { channel_id, message_id, crossposted } => {
                if crossposted && self.announce.publishing(message_id) {
                    self.announce.notice = Some(AnnounceNotice::Published);
                }
                self.announce.end_publish(message_id);
                self.change_message(channel_id, message_id, |m| {
                    let extra = m.meta.extra.get_or_insert_with(Default::default);
                    let changed = extra.crossposted != crossposted;
                    extra.crossposted = crossposted;
                    if extra.is_empty() {
                        m.meta.extra = None;
                    }
                    changed
                });
            }
            AnnounceEvent::PublishFailed { message_id, problem } => {
                self.announce.end_publish(message_id);
                self.announce.notice = Some(AnnounceNotice::Failed { follow: false, problem });
            }
            AnnounceEvent::Followed { source } => {
                self.announce.end_follow(source);
                self.announce.notice = Some(AnnounceNotice::Followed);
            }
            AnnounceEvent::FollowFailed { source, problem } => {
                self.announce.end_follow(source);
                self.announce.notice = Some(AnnounceNotice::Failed { follow: true, problem });
            }
            AnnounceEvent::Refreshed { pairs } => {
                for (_, new) in &pairs {
                    self.announce.note_refreshed(new);
                }
                for cs in self.chans.values_mut() {
                    let mut changed = false;
                    for m in cs.msgs.iter_mut().chain(cs.live.iter_mut()) {
                        for a in &mut m.attachments {
                            if let Some((_, new)) = pairs.iter().find(|(old, _)| *old == a.url) {
                                a.refresh(new);
                                changed = true;
                            }
                        }
                    }
                    if changed {
                        cs.rev.bump();
                    }
                }
            }
        }
    }

    /// « Publier » : dans un salon d'annonces, un message ordinaire pas
    /// encore publié ; le mien (SEND_MESSAGES) ou celui d'un autre (MANAGE_MESSAGES).
    pub fn can_publish(&self, m: &Message) -> bool {
        if !self.is_announcement(m.channel_id) || m.system.is_some() || self.announce.publishing(m.id) {
            return false;
        }
        if m.meta.extra.as_ref().is_some_and(|e| e.crossposted || e.source.is_some()) {
            return false;
        }
        let mine = self.me.as_ref().is_some_and(|u| u.id == m.author_id);
        if mine { self.send_block(m.channel_id).is_none() } else { self.can(m.channel_id, Permissions::MANAGE_MESSAGES) == Verdict::Permis }
    }

    pub fn publish(&mut self, channel_id: u64, message_id: u64) -> bool {
        let Some(m) = self.find_message(channel_id, message_id) else { return false };
        if !self.can_publish(m) || !self.announce.begin_publish(message_id) {
            return false;
        }
        self.outbox.push(Cmd::Announce(AnnounceCmd::Publish { channel_id, message_id }));
        true
    }

    /// Salons de mes serveurs où les publications d'un salon d'annonces
    /// peuvent arriver : salons textuels visibles où j'ai MANAGE_WEBHOOKS
    /// (exigé par la documentation de Follow Announcement Channel).
    /// (salon, nom du salon, nom du serveur).
    pub fn follow_targets(&self, source: u64) -> Vec<(u64, String, String)> {
        self.guilds
            .iter()
            .flat_map(|g| g.channels.iter().map(move |c| (g, c)))
            .filter(|(_, c)| c.kind == ChannelKind::Text && c.id != source && !self.is_hidden(c.id))
            .filter(|(_, c)| self.can(c.id, Permissions::MANAGE_WEBHOOKS) == Verdict::Permis)
            .map(|(g, c)| (c.id, c.name.clone(), g.name.clone()))
            .collect()
    }

    pub fn follow(&mut self, source: u64, target: u64) -> bool {
        if !self.is_announcement(source) || !self.follow_targets(source).iter().any(|(id, _, _)| *id == target) || !self.announce.begin_follow(source) {
            return false;
        }
        self.outbox.push(Cmd::Announce(AnnounceCmd::Follow { source, target }));
        true
    }

    pub fn take_announce_notice(&mut self) -> Option<AnnounceNotice> {
        self.announce.notice.take()
    }

    // ------------------------------------------------------------------ adresses expirées

    /// Images de pièces jointes en échec sous ces adresses (adresse servie,
    /// proxy ou non) : celles dont la signature est passée sont redemandées
    /// à Discord, en une requête par lot de `REFRESH_BATCH`, une seule fois
    /// chacune. Renvoie le nombre d'adresses demandées.
    pub fn refresh_failed(&mut self, fetched: &[String]) -> usize {
        let now = now_ms();
        let mut originals: Vec<String> = Vec::new();
        for url in fetched {
            if !is_expired(url, now) {
                continue;
            }
            let found = self.chans.values().flat_map(|cs| cs.msgs.iter().chain(cs.live.iter())).flat_map(|m| m.attachments.iter()).find(|a| a.fetch_url() == url || a.url == *url);
            if let Some(a) = found
                && !originals.contains(&a.url)
            {
                originals.push(a.url.clone());
            }
        }
        originals.retain(|u| self.announce.want_refresh(u));
        let n = originals.len();
        for chunk in originals.chunks(REFRESH_BATCH) {
            self.outbox.push(Cmd::Announce(AnnounceCmd::Refresh { urls: chunk.to_vec() }));
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::event::Event;
    use super::super::super::model::{Attachment, Avatar, Channel, Guild, User};
    use super::super::super::polls::PollAnswer;
    use super::super::super::system_msgs::Extra;
    use super::*;

    const G: u64 = 1;
    const ANN: u64 = 10;
    const TEXT: u64 = 11;
    const ME: u64 = 100;

    fn msg(id: u64, channel_id: u64, author_id: u64) -> Message {
        Message {
            id,
            channel_id,
            guild_id: Some(G),
            author_id,
            author: "A".into(),
            author_avatar: Avatar::default_for(author_id),
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

    fn store() -> Store {
        let mut s = Store::new();
        let ch = |id, kind| Channel { id, name: format!("c{id}"), kind, position: 0, parent: None, overwrites: None, slowmode: 0 };
        let g = Guild::new(G, "Alpha".into(), vec![ch(ANN, ChannelKind::Announcement), ch(TEXT, ChannelKind::Text)]);
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![g], dms: Vec::new() });
        s
    }

    fn with_poll(s: &mut Store, author: u64, multi: bool) -> u64 {
        let a = |id, count| PollAnswer { id, text: format!("r{id}"), emoji: None, count, me: false };
        let poll = Poll { question: "Q".into(), answers: vec![a(1, 1), a(2, 0)], expiry_ms: Some(i64::MAX), multi, finalized: false, counts_known: true };
        let mut m = msg(50, TEXT, author);
        m.meta.extra = Some(Box::new(Extra { poll: Some(poll), ..Extra::default() }));
        s.select_channel(TEXT);
        s.apply(Event::History { channel_id: TEXT, before: None, messages: vec![m] });
        s.take_commands();
        50
    }

    #[test]
    fn vote_is_shown_at_once_and_undone_on_refusal() {
        let mut s = store();
        let id = with_poll(&mut s, 7, false);
        assert!(!s.vote(TEXT, id, vec![1, 2]), "un seul choix");
        assert!(s.vote(TEXT, id, vec![2]));
        assert!(!s.vote(TEXT, id, vec![1]), "un vote à la fois");
        assert_eq!(s.take_commands(), vec![Cmd::Polls(PollCmd::Vote { channel_id: TEXT, message_id: id, answers: vec![2], previous: vec![] })]);
        assert_eq!(s.poll_of(TEXT, id).map(|p| (p.answers[1].count, p.answers[1].me)), Some((1, true)));
        // Écho de mon vote par la passerelle : compté une fois.
        s.apply(Event::Polls(PollEvent::Vote { channel_id: TEXT, message_id: id, answer_id: 2, user_id: ME, add: true }));
        assert_eq!(s.poll_of(TEXT, id).map(|p| p.total()), Some(2));
        s.apply(Event::Polls(PollEvent::VoteDone { channel_id: TEXT, message_id: id, previous: vec![], error: Some(Problem::Forbidden) }));
        assert_eq!(s.poll_of(TEXT, id).map(|p| (p.answers[1].count, p.voted())), Some((0, false)));
        assert_eq!(s.take_poll_failure().map(|f| f.0), Some(PollAction::Vote));
        // Vote d'un autre.
        s.apply(Event::Polls(PollEvent::Vote { channel_id: TEXT, message_id: id, answer_id: 1, user_id: 8, add: true }));
        assert_eq!(s.poll_of(TEXT, id).map(|p| p.answers[0].count), Some(2));
    }

    #[test]
    fn end_only_my_poll_and_voters_once() {
        let mut s = store();
        let id = with_poll(&mut s, 7, true);
        assert!(!s.end_poll(TEXT, id), "sondage d'un autre");
        assert!(s.request_voters(TEXT, id, 1) && !s.request_voters(TEXT, id, 1));
        let mut s = store();
        let id = with_poll(&mut s, ME, true);
        assert!(s.end_poll(TEXT, id) && !s.end_poll(TEXT, id));
        assert_eq!(s.take_commands(), vec![Cmd::Polls(PollCmd::End { channel_id: TEXT, message_id: id })]);
        // Résultats définitifs par MESSAGE_UPDATE ; un sondage sans résultats garde les comptes.
        let mut p = s.poll_of(TEXT, id).cloned().unwrap_or_default();
        p.counts_known = false;
        p.answers[0].count = 0;
        p.finalized = true;
        s.apply(Event::Polls(PollEvent::Replace { channel_id: TEXT, message_id: id, poll: Box::new(p) }));
        assert_eq!(s.poll_of(TEXT, id).map(|p| (p.answers[0].count, p.finalized)), Some((1, true)));
    }

    #[test]
    fn poll_creation_checks_the_draft() {
        let mut s = store();
        s.select_channel(TEXT);
        s.take_commands();
        assert!(!s.create_poll(TEXT, PollDraft::default()));
        let d = PollDraft { question: "Q".into(), answers: vec!["a".into(), "b".into()], hours: 24, multi: false };
        assert!(s.create_poll(TEXT, d.clone()) && !s.create_poll(TEXT, d), "une création à la fois");
        assert!(matches!(s.take_commands().as_slice(), [Cmd::Polls(PollCmd::Create { channel_id: TEXT, .. })]));
        s.apply(Event::Polls(PollEvent::CreateDone { channel_id: TEXT, error: None }));
        assert_eq!((s.polls.creating, s.polls.created), (false, 1));
    }

    #[test]
    fn publish_only_in_announcement_channels() {
        let mut s = store();
        s.select_channel(ANN);
        s.apply(Event::History { channel_id: ANN, before: None, messages: vec![msg(60, ANN, ME), msg(61, ANN, 7)] });
        s.take_commands();
        assert!(s.publish(ANN, 60) && !s.publish(ANN, 60));
        // Message d'un autre : MANAGE_MESSAGES inconnu ici, pas proposé.
        assert!(!s.publish(ANN, 61));
        assert_eq!(s.take_commands(), vec![Cmd::Announce(AnnounceCmd::Publish { channel_id: ANN, message_id: 60 })]);
        s.apply(Event::Announce(AnnounceEvent::Flags { channel_id: ANN, message_id: 60, crossposted: true }));
        assert_eq!(s.take_announce_notice(), Some(AnnounceNotice::Published));
        assert!(s.find_message(ANN, 60).is_some_and(|m| !s.can_publish(m)));
        // Permissions inconnues : aucun salon proposé pour suivre (MANAGE_WEBHOOKS exigé).
        assert!(s.follow_targets(ANN).is_empty() && !s.follow(ANN, TEXT));
    }

    #[test]
    fn expired_attachments_are_refreshed_once() {
        let mut s = store();
        let old = "https://cdn.discordapp.com/attachments/1/2/a.png?ex=10&is=1&hm=aa&";
        let mut m = msg(70, TEXT, 7);
        m.attachments.push(Attachment { proxy_url: "https://media.discordapp.net/attachments/1/2/a.png?ex=10&is=1&hm=aa&".into(), ..Attachment::file("a.png", old) });
        s.select_channel(TEXT);
        s.apply(Event::History { channel_id: TEXT, before: None, messages: vec![m] });
        s.take_commands();
        let fetched = vec!["https://media.discordapp.net/attachments/1/2/a.png?ex=10&is=1&hm=aa&".to_string(), "https://cdn.discordapp.com/b.png?ex=ffffffffff".into()];
        assert_eq!(s.refresh_failed(&fetched), 1);
        assert_eq!(s.refresh_failed(&fetched), 0, "une seule demande");
        assert_eq!(s.take_commands(), vec![Cmd::Announce(AnnounceCmd::Refresh { urls: vec![old.to_string()] })]);
        let new = "https://cdn.discordapp.com/attachments/1/2/a.png?ex=ffffffff&is=2&hm=bb&";
        let before = s.chans[&TEXT].rev;
        s.apply(Event::Announce(AnnounceEvent::Refreshed { pairs: vec![(old.into(), new.into())] }));
        let a = &s.chans[&TEXT].msgs[0].attachments[0];
        assert_eq!(a.url, new);
        assert_eq!(a.proxy_url, "https://media.discordapp.net/attachments/1/2/a.png?ex=ffffffff&is=2&hm=bb");
        assert_ne!(a.image_ref(), Attachment::file("a.png", old).image_ref(), "nouvelle clé d'image");
        assert_ne!(s.chans[&TEXT].rev, before);
    }

    #[test]
    fn invites_are_asked_once_and_joined_on_click() {
        let mut s = store();
        assert!(s.want_invite("abc") && !s.want_invite("abc"));
        assert_eq!(s.take_commands(), vec![Cmd::Invites(InviteCmd::Get { code: "abc".into() })]);
        assert!(!s.join_invite_card("abc"), "pas encore chargée");
        let inv = Invite { code: "abc".into(), kind: 0, guild_id: Some(G), name: "Alpha".into(), ..Invite::default() };
        s.apply(Event::Invites(InviteEvent::Loaded { code: "abc".into(), invite: Ok(inv.clone()) }));
        assert!(s.invite_joined(&inv) && !s.join_invite_card("abc"), "déjà membre");
        let other = Invite { guild_id: Some(99), ..inv };
        s.apply(Event::Invites(InviteEvent::Loaded { code: "abc".into(), invite: Ok(other) }));
        assert!(s.join_invite_card("abc") && !s.join_invite_card("abc"));
        assert_eq!(s.take_commands(), vec![Cmd::Servers(crate::state::servers::ServerCmd::Join { code: "abc".into() })], "par le lot serveurs");
        assert_eq!(s.invites.join_state("abc"), Some(JoinState::Joining));
        let failed = |failure| Event::Servers(crate::state::servers::ServerEvent::Failed { action: crate::state::servers::ServerAction::Join, failure });
        s.apply(failed(crate::state::servers::Failure::Captcha));
        assert_eq!(s.invites.join_state("abc"), Some(JoinState::Captcha));
        assert!(!s.join_invite_card("abc"), "pas de nouvel essai automatique");
    }
}
