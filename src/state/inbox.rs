//! Boîte de réception (Ctrl+I) : onglet « Mentions », chargé à la demande
//! par page, et onglet « Non-lus », tiré du magasin des non-lus.
//!
//! Mentions récentes : `GET /users/@me/mentions` et `DELETE
//! /users/@me/mentions/{message.id}`, points d'API réservés aux comptes
//! utilisateur et non documentés par Discord (forme vérifiée sur
//! docs.discord.food, page « User », sections « List Recent Mentions » et
//! « Delete Recent Mention »). Rien n'est demandé sans un geste : ouverture
//! de l'onglet, « Charger plus », filtre changé, croix d'une mention.

use super::event::Cmd;
use super::model::Message;
use super::notice::Problem;
use super::store::Store;
use std::collections::HashSet;

/// Mentions par page (1 à 100 d'après la source ; 25 par défaut).
pub const PAGE: u8 = 25;
/// Mentions gardées au plus : au-delà, « Charger plus » n'est plus proposé.
pub const MAX_KEPT: usize = 250;
/// Longueur d'un extrait (caractères).
const EXCERPT: usize = 300;

/// Paramètres `roles`, `everyone` et `guild_id` de la demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Filter {
    pub roles: bool,
    pub everyone: bool,
    pub guild: Option<u64>,
}

impl Default for Filter {
    fn default() -> Self {
        Filter { roles: true, everyone: true, guild: None }
    }
}

/// Une mention reçue, avec son extrait en texte simple (fait à la réception).
#[derive(Clone, Debug)]
pub struct Mention {
    pub msg: Message,
    pub guild: Option<u64>,
    pub excerpt: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InboxCmd {
    /// Page avant `before` (aucun : les plus récentes). `round` : numéro de la
    /// liste (une réponse d'une liste remplacée depuis est ignorée).
    Load { round: u64, before: Option<u64>, filter: Filter },
    Delete { message_id: u64 },
}

#[derive(Debug)]
pub enum InboxEvent {
    Page { round: u64, before: Option<u64>, messages: Vec<Message> },
    Deleted { message_id: u64 },
    LoadFailed { round: u64, problem: Problem },
    DeleteFailed { message_id: u64, problem: Problem },
}

#[derive(Default)]
pub struct Inbox {
    pub items: Vec<Mention>,
    pub filter: Filter,
    pub loading: bool,
    /// Au moins une page reçue pour ce filtre.
    pub loaded: bool,
    /// Il reste des pages (page pleine reçue).
    pub more: bool,
    pub error: Option<Problem>,
    /// Retraits en cours.
    pub deleting: HashSet<u64>,
    round: u64,
}

impl Inbox {
    pub fn clear(&mut self) {
        *self = Inbox { round: self.round + 1, ..Default::default() };
    }
}

impl Store {
    /// Ouverture de l'onglet « Mentions » : la première page, si elle n'est
    /// ni chargée ni en cours. `true` si la demande part.
    pub fn inbox_open(&mut self) -> bool {
        if self.inbox.loaded || self.inbox.loading {
            return false;
        }
        self.inbox_request(None)
    }

    /// « Charger plus » : la page qui précède la plus ancienne mention.
    pub fn inbox_more(&mut self) -> bool {
        let i = &self.inbox;
        if i.loading || !i.more || i.items.len() >= MAX_KEPT {
            return false;
        }
        let before = i.items.last().map(|m| m.msg.id);
        self.inbox_request(before)
    }

    /// Filtre changé : la liste repart de zéro.
    pub fn inbox_filter(&mut self, filter: Filter) -> bool {
        if self.inbox.filter == filter {
            return false;
        }
        let round = self.inbox.round + 1;
        self.inbox = Inbox { filter, round, ..Default::default() };
        self.inbox_request(None)
    }

    /// Rafraîchir (erreur, ou liste ancienne) : la liste repart de zéro.
    pub fn inbox_reload(&mut self) -> bool {
        if self.inbox.loading {
            return false;
        }
        let (filter, round) = (self.inbox.filter, self.inbox.round + 1);
        self.inbox = Inbox { filter, round, ..Default::default() };
        self.inbox_request(None)
    }

    fn inbox_request(&mut self, before: Option<u64>) -> bool {
        let i = &mut self.inbox;
        i.loading = true;
        i.error = None;
        let cmd = InboxCmd::Load { round: i.round, before, filter: i.filter };
        self.push_command(Cmd::Inbox(cmd));
        true
    }

    /// Croix d'une mention : retirée chez Discord, puis de la liste.
    pub fn inbox_delete(&mut self, message_id: u64) -> bool {
        if !self.inbox.items.iter().any(|m| m.msg.id == message_id) || !self.inbox.deleting.insert(message_id) {
            return false;
        }
        self.push_command(Cmd::Inbox(InboxCmd::Delete { message_id }));
        true
    }

    pub(super) fn apply_inbox(&mut self, e: InboxEvent) {
        match e {
            InboxEvent::Page { round, before, messages } => {
                if round != self.inbox.round {
                    return;
                }
                // Une page qui ne suit plus la liste (doublon de demande) est ignorée.
                if before != self.inbox.items.last().map(|m| m.msg.id) {
                    self.inbox.loading = false;
                    return;
                }
                let full = messages.len() >= usize::from(PAGE);
                let mut fresh: Vec<Mention> = messages.into_iter().filter(|m| !self.inbox.items.iter().any(|x| x.msg.id == m.id)).map(|m| self.mention(m)).collect();
                // Les plus récentes d'abord, quel que soit l'ordre reçu.
                fresh.sort_by_key(|m| std::cmp::Reverse(m.msg.id));
                let i = &mut self.inbox;
                i.items.extend(fresh);
                i.items.truncate(MAX_KEPT);
                i.loading = false;
                i.loaded = true;
                i.more = full && i.items.len() < MAX_KEPT;
            }
            InboxEvent::LoadFailed { round, problem } => {
                if round == self.inbox.round {
                    self.inbox.loading = false;
                    self.inbox.error = Some(problem);
                }
            }
            InboxEvent::Deleted { message_id } => {
                self.inbox.deleting.remove(&message_id);
                self.inbox.items.retain(|m| m.msg.id != message_id);
            }
            InboxEvent::DeleteFailed { message_id, problem } => {
                self.inbox.deleting.remove(&message_id);
                self.inbox.error = Some(problem);
            }
        }
    }

    fn mention(&self, msg: Message) -> Mention {
        let guild = msg.guild_id.or_else(|| self.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == msg.channel_id)).map(|g| g.id));
        let excerpt = self.mention_excerpt(&self.render_content(&msg), &msg, guild);
        Mention { msg, guild, excerpt }
    }

    /// Texte simple d'un message : mentions, salons, rôles et emojis
    /// personnalisés mis en mots, retours à la ligne aplatis, longueur bornée.
    fn mention_excerpt(&self, src: &str, m: &Message, guild: Option<u64>) -> String {
        let mut out = String::with_capacity(src.len().min(EXCERPT * 4));
        let mut rest = src;
        while let Some(start) = rest.find('<') {
            out.push_str(&rest[..start]);
            let tail = &rest[start..];
            let Some(end) = tail.find('>') else {
                out.push_str(tail);
                rest = "";
                break;
            };
            let inner = &tail[1..end];
            match self.tag_text(inner, m, guild) {
                Some(word) => out.push_str(&word),
                None => out.push_str(&tail[..=end]),
            }
            rest = &tail[end + 1..];
        }
        out.push_str(rest);
        let flat: String = out.split_whitespace().collect::<Vec<_>>().join(" ");
        match flat.char_indices().nth(EXCERPT) {
            Some((cut, _)) => format!("{}\u{2026}", &flat[..cut]),
            None => flat,
        }
    }

    fn tag_text(&self, inner: &str, m: &Message, guild: Option<u64>) -> Option<String> {
        let id = |s: &str| s.parse::<u64>().ok();
        if let Some(u) = inner.strip_prefix("@&").and_then(id) {
            let g = self.guilds.iter().find(|g| Some(g.id) == guild)?;
            return g.roles.iter().find(|r| r.id == u).map(|r| format!("@{}", r.name));
        }
        if let Some(u) = inner.strip_prefix("@!").or_else(|| inner.strip_prefix('@')).and_then(id) {
            return m.mentions.iter().find(|(x, _)| *x == u).map(|(_, n)| format!("@{n}"));
        }
        if let Some(c) = inner.strip_prefix('#').and_then(id) {
            return self.channel_name(c).map(|n| format!("#{n}"));
        }
        let emoji = inner.strip_prefix("a:").or_else(|| inner.strip_prefix(':'))?;
        let (name, rest) = emoji.split_once(':')?;
        id(rest).map(|_| format!(":{name}:"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Avatar, Channel, Guild, Message, Role};
    use crate::state::Permissions;

    fn msg(id: u64, channel: u64, content: &str) -> Message {
        Message {
            id,
            channel_id: channel,
            guild_id: None,
            author_id: 3,
            author: "Bob".into(),
            author_avatar: Avatar::default_for(3),
            content: content.into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: vec![(1, "Moi".into())],
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    fn store() -> Store {
        let mut s = Store::new();
        let chan = Channel { id: 10, name: "général".into(), kind: crate::state::model::ChannelKind::Text, position: 0, parent: None, overwrites: None, slowmode: 0 };
        let mut g = Guild::new(9, "Alpha".into(), vec![chan]);
        g.roles.push(Role { id: 77, name: "modos".into(), color: 0, position: 1, permissions: Permissions::NONE, mentionable: true });
        s.guilds.push(g);
        s
    }

    fn page(round: u64, before: Option<u64>, ids: std::ops::Range<u64>) -> InboxEvent {
        InboxEvent::Page { round, before, messages: ids.map(|i| msg(i, 10, "salut <@1>")).collect() }
    }

    #[test]
    fn pages_follow_each_other_and_stale_answers_are_ignored() {
        let mut s = store();
        assert!(s.inbox_open());
        assert!(!s.inbox_open(), "déjà en cours");
        let cmds = s.take_commands();
        assert_eq!(cmds, vec![Cmd::Inbox(InboxCmd::Load { round: 0, before: None, filter: Filter::default() })]);
        s.apply_inbox(page(0, None, 100..125));
        assert_eq!(s.inbox.items.len(), 25);
        assert_eq!(s.inbox.items[0].msg.id, 124, "plus récente d'abord");
        assert_eq!(s.inbox.items[0].guild, Some(9), "serveur retrouvé par le salon");
        assert_eq!(s.inbox.items[0].excerpt, "salut @Moi");
        assert!(s.inbox.more);
        assert!(s.inbox_more());
        assert_eq!(s.take_commands(), vec![Cmd::Inbox(InboxCmd::Load { round: 0, before: Some(100), filter: Filter::default() })]);
        // Filtre changé avant la réponse : elle ne vaut plus.
        assert!(s.inbox_filter(Filter { everyone: false, ..Filter::default() }));
        s.apply_inbox(page(0, Some(100), 80..90));
        assert!(s.inbox.items.is_empty() && s.inbox.loading);
        s.apply_inbox(page(1, None, 1..3));
        assert_eq!(s.inbox.items.len(), 2);
        assert!(!s.inbox.more, "page incomplète : c'est la fin");
        assert!(!s.inbox_more());
    }

    #[test]
    fn delete_removes_after_confirmation_only() {
        let mut s = store();
        s.inbox_open();
        s.apply_inbox(page(0, None, 1..4));
        let _ = s.take_commands();
        assert!(s.inbox_delete(2));
        assert!(!s.inbox_delete(2), "déjà en cours");
        assert!(!s.inbox_delete(99), "inconnue");
        assert_eq!(s.take_commands(), vec![Cmd::Inbox(InboxCmd::Delete { message_id: 2 })]);
        assert_eq!(s.inbox.items.len(), 3, "gardée jusqu'à la réponse");
        s.apply_inbox(InboxEvent::DeleteFailed { message_id: 2, problem: Problem::Forbidden });
        assert_eq!(s.inbox.items.len(), 3);
        assert!(s.inbox_delete(2));
        s.apply_inbox(InboxEvent::Deleted { message_id: 2 });
        assert_eq!(s.inbox.items.iter().map(|m| m.msg.id).collect::<Vec<_>>(), vec![3, 1]);
    }

    #[test]
    fn excerpt_words_tags_and_is_bounded() {
        let s = store();
        let m = msg(1, 10, "<@&77> voir <#10>\n\net <:ferris:123> <a:dance:9> <@5> <pas un tag");
        assert_eq!(s.mention_excerpt(&m.content, &m, Some(9)), "@modos voir #général et :ferris: :dance: <@5> <pas un tag");
        let long = "é".repeat(EXCERPT + 10);
        let m = msg(1, 10, &long);
        let e = s.mention_excerpt(&m.content, &m, None);
        assert_eq!(e.chars().count(), EXCERPT + 1);
        assert!(e.ends_with('\u{2026}'));
    }
}
