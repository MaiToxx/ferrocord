//! Ce que la zone de saisie lit du magasin : mode lent, personnes, rôles,
//! salons et emojis proposés par l'autocomplétion, noms de ceux qui écrivent.
//! Tout est lu à la demande (quand la saisie change), rien n'est recalculé
//! à chaque image.

use super::model::{snowflake_ms, Channel, ChannelKind, Emoji, Guild, Role};
use super::perms::{Permissions, Verdict};
use super::store::Store;

/// Salons dont l'instant du dernier envoi est retenu, au plus : au-delà,
/// ceux dont le mode lent est forcément fini (6 h, le maximum) sont oubliés,
/// puis le plus ancien.
const MAX_LAST_SENT: usize = 64;
/// Mode lent maximal documenté (6 h), en ms.
const SLOWMODE_MAX_MS: i64 = 21_600_000;
/// Messages relus (les plus récents) pour retrouver mon dernier envoi.
const OWN_LOOKBACK: usize = 100;

impl Store {
    /// Salon de serveur affiché ou connu, et son serveur.
    fn channel_in_guild(&self, channel: u64) -> Option<(&Guild, &Channel)> {
        let find = |g: &'_ Guild| g.channels.iter().find(|c| c.id == channel).map(|_| ());
        let g = match self.current_guild() {
            Some(g) if find(g).is_some() => g,
            _ => self.guilds.iter().find(|g| find(g).is_some())?,
        };
        Some((g, g.channels.iter().find(|c| c.id == channel)?))
    }

    /// Mode lent qui s'applique à moi dans ce salon (secondes, 0 sans).
    /// Gérer les messages ou le salon en dispense (documentation de
    /// `rate_limit_per_user`), comme la permission dédiée.
    pub fn slowmode(&self, channel: u64) -> u32 {
        let Some((_, c)) = self.channel_in_guild(channel) else { return 0 };
        if c.slowmode == 0 {
            return 0;
        }
        let exempt = [Permissions::MANAGE_MESSAGES, Permissions::MANAGE_CHANNELS, Permissions::BYPASS_SLOWMODE];
        if exempt.into_iter().any(|p| self.can(channel, p) == Verdict::Permis) {
            return 0;
        }
        c.slowmode
    }

    /// Attente restante avant de pouvoir écrire (ms), d'après mon dernier
    /// envoi de cette session ou mon dernier message reçu dans ce salon.
    pub fn slowmode_left(&self, channel: u64, now: i64) -> Option<i64> {
        let secs = self.slowmode(channel);
        if secs == 0 {
            return None;
        }
        let me = self.me.as_ref()?.id;
        let own = self.chans.get(&channel).and_then(|cs| cs.msgs.iter().rev().take(OWN_LOOKBACK).find(|m| m.author_id == me)).map(|m| snowflake_ms(m.id));
        let last = self.last_sent.get(&channel).copied().max(own)?;
        let left = last.saturating_add(i64::from(secs) * 1000).saturating_sub(now);
        // Jamais plus que le mode lent lui-même (horloges décalées).
        (left > 0).then_some(left.min(i64::from(secs) * 1000))
    }

    /// Un message vient de partir : début du mode lent.
    pub(super) fn note_sent(&mut self, channel: u64) {
        let now = self.now();
        if !self.last_sent.contains_key(&channel) && self.last_sent.len() >= MAX_LAST_SENT {
            self.last_sent.retain(|_, t| now.saturating_sub(*t) < SLOWMODE_MAX_MS);
            if self.last_sent.len() >= MAX_LAST_SENT
                && let Some(old) = self.last_sent.iter().min_by_key(|(_, t)| **t).map(|(c, _)| *c)
            {
                self.last_sent.remove(&old);
            }
        }
        self.last_sent.insert(channel, now);
    }

    /// L'envoi a échoué : le mode lent n'a pas commencé.
    pub(super) fn forget_sent(&mut self, channel: u64) {
        self.last_sent.remove(&channel);
    }

    /// Personnes connues pour mentionner dans ce salon, les plus récentes
    /// d'abord : auteurs et mentionnés des messages en mémoire (ce salon,
    /// puis les autres salons du même serveur), destinataires du MP, et
    /// ceux qui écrivent. Aucune requête réseau.
    pub fn known_people(&self, channel: u64) -> Vec<(u64, String)> {
        let mut out: Vec<(u64, String)> = Vec::new();
        let mut push = |id: u64, name: &str| {
            if !name.is_empty() && !out.iter().any(|(i, _)| *i == id) {
                out.push((id, name.to_string()));
            }
        };
        let now = self.now();
        for t in self.typing.active(channel, now) {
            if let Some(n) = &t.name {
                push(t.user_id, n);
            }
        }
        if let Some(cs) = self.chans.get(&channel) {
            for m in cs.msgs.iter().rev() {
                push(m.author_id, &m.author);
                for (id, name) in &m.mentions {
                    push(*id, name);
                }
            }
        }
        if let Some(d) = self.dms.iter().find(|d| d.id == channel) {
            for (id, name) in &d.recipients {
                push(*id, name);
            }
        }
        let guild = self.channel_in_guild(channel).map(|(g, _)| g.id);
        if let Some(gid) = guild {
            for (cid, cs) in &self.chans {
                if *cid == channel {
                    continue;
                }
                for m in cs.msgs.iter().rev().filter(|m| m.guild_id == Some(gid)) {
                    push(m.author_id, &m.author);
                }
            }
        }
        if let Some(me) = &self.me {
            push(me.id, &me.name);
        }
        out
    }

    /// Rôles que je peux mentionner ici (mentionnables, ou tous si j'ai
    /// « Mentionner @everyone »), sans @everyone lui-même.
    pub fn mentionable_roles(&self, channel: u64) -> Vec<&Role> {
        let Some((g, _)) = self.channel_in_guild(channel) else { return Vec::new() };
        let all = self.can(channel, Permissions::MENTION_EVERYONE) == Verdict::Permis;
        let mut roles: Vec<&Role> = g.roles.iter().filter(|r| r.id != g.id && (all || r.mentionable)).collect();
        roles.sort_by_key(|r| std::cmp::Reverse(r.position));
        roles
    }

    /// @everyone et @here ont un effet : ils me sont permis ici.
    pub fn can_mention_everyone(&self, channel: u64) -> bool {
        self.channel_in_guild(channel).is_some() && self.can(channel, Permissions::MENTION_EVERYONE) == Verdict::Permis
    }

    /// Salons textuels visibles du serveur de ce salon, dans l'ordre de la liste.
    pub fn visible_channels(&self, channel: u64) -> Vec<&Channel> {
        let Some((g, _)) = self.channel_in_guild(channel) else { return Vec::new() };
        g.channels.iter().filter(|c| c.kind != ChannelKind::Category && !self.is_hidden(c.id)).collect()
    }

    /// Emojis personnalisés du serveur de ce salon que je peux utiliser :
    /// disponibles, et réservés à aucun rôle ou à l'un des miens (mes rôles
    /// inconnus : proposés quand même, Discord tranchera).
    pub fn usable_emojis(&self, channel: u64) -> Vec<&Emoji> {
        let Some((g, _)) = self.channel_in_guild(channel) else { return Vec::new() };
        let mine = g.me.as_ref().map(|m| &m.roles);
        g.emojis.iter().filter(|e| e.available && (e.roles.is_empty() || mine.is_none_or(|r| e.roles.iter().any(|x| r.contains(x))))).collect()
    }

    /// Nom de quelqu'un qui écrit sans que le paquet le donne (MP) : retrouvé
    /// parmi les destinataires du MP ou les auteurs des messages du salon.
    pub(super) fn typer_name(&self, channel: u64, user: u64) -> Option<String> {
        if let Some(d) = self.dms.iter().find(|d| d.id == channel)
            && let Some((_, n)) = d.recipients.iter().find(|(id, _)| *id == user)
        {
            return Some(n.clone());
        }
        let cs = self.chans.get(&channel)?;
        cs.msgs.iter().rev().find(|m| m.author_id == user).map(|m| m.author.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{Dm, Guild, Member, Message, User};
    use super::super::{Event, Permissions as P};
    use super::*;

    const G: u64 = 9;
    const CHAN: u64 = 100;
    const ME: u64 = 1;

    fn chan(id: u64, kind: ChannelKind, slowmode: u32) -> Channel {
        Channel { id, name: format!("c{id}"), kind, position: id as i32, parent: None, overwrites: Some(Vec::new()), slowmode }
    }

    fn role(id: u64, p: P, mentionable: bool) -> Role {
        Role { id, name: format!("r{id}"), color: 0, position: id as i32, permissions: p, mentionable }
    }

    fn msg(id: u64, author: u64, name: &str) -> Message {
        Message {
            id,
            channel_id: CHAN,
            guild_id: Some(G),
            author_id: author,
            author: name.into(),
            author_avatar: super::super::model::Avatar::default_for(author),
            content: String::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    fn store(everyone: P, my_roles: Vec<u64>) -> Store {
        let mut s = Store::new();
        let mut g = Guild::new(G, "g".into(), vec![chan(CHAN, ChannelKind::Text, 10), chan(101, ChannelKind::Text, 0), chan(102, ChannelKind::Category, 0)]);
        g.roles = vec![role(G, everyone, false), role(5, P::NONE, true), role(6, P::MANAGE_MESSAGES, false)];
        g.me = Some(Member { roles: my_roles, nick: None, timeout_until: None });
        g.owner_id = Some(77);
        g.emojis = vec![
            Emoji { id: 1, name: "ok".into(), animated: false, available: true, roles: Vec::new() },
            Emoji { id: 2, name: "off".into(), animated: false, available: false, roles: Vec::new() },
            Emoji { id: 3, name: "vip".into(), animated: true, available: true, roles: vec![6] },
        ];
        s.apply(Event::Ready { me: User::new(ME, "moi".into()), guilds: vec![g], dms: Vec::new() });
        s.select_guild(G);
        s
    }

    #[test]
    fn slowmode_counts_down_from_my_last_message_unless_exempt() {
        let base = P::VIEW_CHANNEL | P::SEND_MESSAGES | P::READ_MESSAGE_HISTORY;
        let mut s = store(base, Vec::new());
        assert_eq!(s.slowmode(CHAN), 10);
        assert_eq!(s.slowmode(101), 0);
        let now = s.now();
        assert_eq!(s.slowmode_left(CHAN, now), None);
        assert!(s.send_message(CHAN, "a".into()));
        let left = s.slowmode_left(CHAN, s.now()).unwrap_or(0);
        assert!((9_000..=10_000).contains(&left), "{left}");
        assert_eq!(s.slowmode_left(CHAN, s.now() + 10_001), None);
        // Échec de l'envoi : pas de mode lent.
        s.apply(Event::SendError { channel_id: CHAN, nonce: 1, content: "a".into(), error: super::super::Problem::Forbidden, forbidden: false });
        assert_eq!(s.slowmode_left(CHAN, s.now()), None);
        // Mon dernier message reçu compte aussi (session précédente).
        let at = u64::try_from(s.now() - 4_000 - super::super::model::DISCORD_EPOCH as i64).unwrap_or(0) << 22;
        s.apply(Event::History { channel_id: CHAN, before: None, messages: vec![msg(at, ME, "moi")] });
        let left = s.slowmode_left(CHAN, s.now()).unwrap_or(0);
        assert!((5_000..=6_100).contains(&left), "{left}");
        // Gérer les messages : dispensé.
        let s = store(base, vec![6]);
        assert_eq!(s.slowmode(CHAN), 0);
    }

    #[test]
    fn roles_channels_and_emojis_offered() {
        let base = P::VIEW_CHANNEL | P::SEND_MESSAGES;
        let s = store(base, Vec::new());
        let ids: Vec<u64> = s.mentionable_roles(CHAN).iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![5]);
        assert!(!s.can_mention_everyone(CHAN));
        let s2 = store(base | P::MENTION_EVERYONE, Vec::new());
        let ids: Vec<u64> = s2.mentionable_roles(CHAN).iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![6, 5]);
        assert!(s2.can_mention_everyone(CHAN));
        let chans: Vec<u64> = s.visible_channels(CHAN).iter().map(|c| c.id).collect();
        assert_eq!(chans, vec![CHAN, 101]);
        let em: Vec<u64> = s.usable_emojis(CHAN).iter().map(|e| e.id).collect();
        assert_eq!(em, vec![1]);
        let s3 = store(base, vec![6]);
        let em: Vec<u64> = s3.usable_emojis(CHAN).iter().map(|e| e.id).collect();
        assert_eq!(em, vec![1, 3]);
        // MP : rien de tout cela.
        assert!(s.visible_channels(4242).is_empty() && s.mentionable_roles(4242).is_empty());
    }

    #[test]
    fn people_recent_first_with_dm_recipients_and_typing_names() {
        let base = P::VIEW_CHANNEL | P::SEND_MESSAGES | P::READ_MESSAGE_HISTORY;
        let mut s = store(base, Vec::new());
        s.select_channel(CHAN);
        s.take_commands();
        let mut m = msg(10, 2, "Bob");
        m.mentions = vec![(3, "Carol".into())];
        s.apply(Event::History { channel_id: CHAN, before: None, messages: vec![msg(9, 4, "Dan"), m] });
        let people: Vec<u64> = s.known_people(CHAN).iter().map(|p| p.0).collect();
        assert_eq!(people, vec![2, 3, 4, ME]);
        // MP : destinataires, nom retrouvé pour l'indicateur.
        let dm = Dm { recipients: vec![(8, "Eve".into())], ..Dm::new(50, "Eve".into(), 50) };
        s.apply(Event::DmUpsert { dm, known: super::super::DmKnown::ALL });
        assert_eq!(s.known_people(50), vec![(8, "Eve".into()), (ME, "moi".into())]);
        s.apply(Event::TypingStart { channel_id: 50, user_id: 8, name: None });
        let names: Vec<Option<String>> = s.typing.active(50, s.now()).map(|t| t.name.clone()).collect();
        assert_eq!(names, vec![Some("Eve".into())]);
        s.apply(Event::TypingStart { channel_id: CHAN, user_id: 2, name: None });
        s.apply(Event::TypingStart { channel_id: CHAN, user_id: 99, name: None });
        let names: Vec<Option<String>> = s.typing.active(CHAN, s.now()).map(|t| t.name.clone()).collect();
        assert_eq!(names, vec![Some("Bob".into()), None]);
        // Son message arrive : il n'écrit plus.
        s.apply(Event::MessageCreate(msg(11, 2, "Bob")));
        assert_eq!(s.typing.active(CHAN, s.now()).count(), 1);
    }
}
