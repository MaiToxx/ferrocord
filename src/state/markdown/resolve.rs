//! Mise en mots des mentions et des horodatages d'un message, à partir du
//! magasin (noms de salons, rôles du serveur) et du message lui-même
//! (utilisateurs mentionnés).

use super::{parse, parse_from, Doc, Resolve, TimeStyle};
use crate::i18n::{t, time, Lang};
use crate::state::model::Message;
use crate::state::store::{now_ms, Store};

/// Ce qu'il faut pour mettre en mots un message.
pub struct MessageContext<'a> {
    pub store: &'a Store,
    pub message: &'a Message,
    pub lang: Lang,
    /// Heures sur 24 h (sinon 12 h).
    pub h24: bool,
}

impl Resolve for MessageContext<'_> {
    fn user(&self, id: u64) -> String {
        match self.message.mentions.iter().find(|(uid, _)| *uid == id) {
            Some((_, name)) => name.clone(),
            None => t::UNKNOWN_USER.get(self.lang).to_string(),
        }
    }

    fn role(&self, id: u64) -> (String, u32) {
        let role = self.message.guild_id.and_then(|g| self.store.guilds.iter().find(|x| x.id == g)).and_then(|g| g.roles.iter().find(|r| r.id == id));
        match role {
            Some(r) => (r.name.clone(), r.color),
            None => (t::UNKNOWN_ROLE.get(self.lang).to_string(), 0),
        }
    }

    fn channel(&self, id: u64) -> String {
        self.store.channel_name(id).unwrap_or_else(|| t::UNKNOWN_CHANNEL.get(self.lang)).to_string()
    }

    fn time(&self, secs: i64, style: TimeStyle) -> Option<String> {
        let ms = secs.checked_mul(1000)?;
        let s = time::stamp(self.lang, ms, now_ms(), style.letter(), self.h24);
        (!s.is_empty()).then_some(s)
    }

    fn refresh_at(&self, secs: i64) -> Option<i64> {
        let now = now_ms();
        Some(now + time::relative_changes_in(secs.checked_mul(1000)?, now))
    }
}

/// Analyse le texte d'un message (à faire une fois, puis garder le résultat),
/// avec l'extrait de sa réponse, le message transféré et les textes de ses embeds.
pub fn parse_message(store: &Store, message: &Message, lang: Lang, h24: bool) -> Doc {
    let mut doc = parse(&message.content, &MessageContext { store, message, lang, h24 });
    // Mentions propres à l'extrait ou au transfert : résolues avec leur liste.
    let other = |content: &str, mentions: &[(u64, String)], first: u32| {
        let m = Message {
            id: message.id,
            channel_id: message.channel_id,
            guild_id: message.guild_id,
            author_id: 0,
            author: String::new(),
            author_avatar: message.author_avatar,
            content: String::new(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: mentions.to_vec(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        };
        parse_from(content, &MessageContext { store, message: &m, lang, h24 }, first)
    };
    if let Some((content, mentions)) = message.meta.reply_raw.as_deref() {
        let d = other(content, mentions, doc.spoilers);
        doc.spoilers = d.spoilers;
        doc.reply = Some(Box::new(d));
    }
    if let Some(f) = message.meta.forward.as_deref().filter(|f| !f.content.is_empty()) {
        let d = other(&f.content, &f.mentions, doc.spoilers);
        doc.spoilers = d.spoilers;
        doc.forward = Some(Box::new(d));
    }
    let ctx = MessageContext { store, message, lang, h24 };
    for (i, e) in message.embeds.iter().enumerate() {
        let texts = e.description.iter().map(|d| (0, d.as_str())).chain(e.fields.iter().enumerate().map(|(n, f)| (n + 1, f.value.as_str())));
        for (slot, text) in texts {
            let d = parse_from(text, &ctx, doc.spoilers);
            doc.spoilers = d.spoilers;
            doc.embeds.push((i, slot, d));
        }
    }
    if let Some(app) = message.meta.app.as_deref() {
        let mut texts = Vec::new();
        crate::state::interactions::Component::texts(&app.components, &mut texts);
        for text in texts {
            let d = parse_from(text, &ctx, doc.spoilers);
            doc.spoilers = d.spoilers;
            doc.components.push(d);
        }
    }
    doc
}
