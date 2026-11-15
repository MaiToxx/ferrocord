//! Raccord du magasin avec le journal des messages supprimés et modifiés
//! (plugin, lot E1). Module enfant de `store` (déclaré par `#[path]`).
//!
//! Le journal vit dans l'interface (`ui::journal`) ; le magasin ne fait que
//! porter ce qu'il faut afficher, dans `MessageMeta::trace` : un message
//! supprimé reste dans la fenêtre, marqué ; un message modifié porte ses
//! versions précédentes. Rien ici n'est appelé quand le plugin est coupé.

use super::super::journal::{Journal, Trace, Version, MAX_VERSIONS};
use super::super::model::Message;
use super::Store;

fn push(m: &mut Message, v: &Version) {
    let t = m.meta.trace.get_or_insert_with(Default::default);
    t.versions.push(v.clone());
    if t.versions.len() > MAX_VERSIONS {
        let over = t.versions.len() - MAX_VERSIONS;
        t.versions.drain(..over);
    }
}

impl Store {
    /// Page d'historique reçue pour un export (plugin, lot E1) : rendue
    /// comme une page du salon (balises remplacées, ordre croissant), sans y entrer.
    pub fn render_foreign(&self, messages: Vec<Message>) -> Vec<Message> {
        self.render_page(messages).0
    }

    /// La liste du salon attend elle-même la page antérieure à `before`.
    pub fn asked_older(&self, channel_id: u64, before: u64) -> bool {
        self.chans.get(&channel_id).is_some_and(|cs| cs.asked_older == Some(before))
    }

    /// Message en mémoire (fenêtre, ou file des messages arrivés pendant
    /// qu'elle est remontée), et son texte d'origine s'il a été refait.
    pub fn message_in_memory(&self, channel_id: u64, id: u64) -> Option<(&Message, Option<&String>)> {
        let cs = self.chans.get(&channel_id)?;
        let m = match cs.msgs.binary_search_by_key(&id, |m| m.id) {
            Ok(i) => cs.msgs.get(i),
            Err(_) => cs.live.iter().find(|m| m.id == id),
        }?;
        Some((m, cs.stamped.get(&id)))
    }

    /// Suppression gardée au journal : le message reste affiché, marqué
    /// supprimé à `at_ms`. Remplace `Event::MessageDelete` pour ce message.
    pub fn journal_mark_deleted(&mut self, channel_id: u64, id: u64, at_ms: i64) {
        self.social_deleted(id);
        let Some(cs) = self.chans.get_mut(&channel_id) else { return };
        cs.missed.remove(&id);
        if let Ok(i) = cs.msgs.binary_search_by_key(&id, |m| m.id)
            && let Some(m) = cs.msgs.get_mut(i)
        {
            let t = m.meta.trace.get_or_insert_with(Default::default);
            t.gone_ms = Some(at_ms);
            // Plus de réactions à cliquer ni de fil à ouvrir sur un message qui n'existe plus.
            m.meta.reactions.clear();
            cs.rev.bump();
        }
        if let Some(m) = cs.live.iter_mut().find(|m| m.id == id) {
            m.meta.trace.get_or_insert_with(Default::default).gone_ms = Some(at_ms);
        }
    }

    /// Version précédente d'un message qui va être modifié (avant
    /// `Event::MessageUpdate`, qui ne touche pas à `trace`).
    pub fn journal_push_version(&mut self, channel_id: u64, id: u64, v: &Version) {
        let Some(cs) = self.chans.get_mut(&channel_id) else { return };
        if let Ok(i) = cs.msgs.binary_search_by_key(&id, |m| m.id)
            && let Some(m) = cs.msgs.get_mut(i)
        {
            push(m, v);
            cs.rev.bump();
        }
        if let Some(m) = cs.live.iter_mut().find(|m| m.id == id) {
            push(m, v);
        }
    }

    /// Déplie ou replie les versions précédentes du message.
    pub fn journal_toggle(&mut self, channel_id: u64, id: u64) -> bool {
        let Some(cs) = self.chans.get_mut(&channel_id) else { return false };
        let Ok(i) = cs.msgs.binary_search_by_key(&id, |m| m.id) else { return false };
        let Some(t) = cs.msgs.get_mut(i).and_then(|m| m.meta.trace.as_mut()) else { return false };
        if t.versions.is_empty() {
            return false;
        }
        t.open = !t.open;
        cs.rev.bump();
        true
    }

    /// Après une page d'historique (ou la lecture du journal) : les messages
    /// modifiés de la fenêtre retrouvent leurs versions, et les messages
    /// supprimés qui tombent dans la fenêtre y reprennent leur place. Une
    /// fenêtre qui touche le présent (ou le début du salon) s'étend jusqu'à eux.
    pub fn journal_restore(&mut self, channel_id: u64, journal: &Journal) {
        let Some(cs) = self.chans.get_mut(&channel_id) else { return };
        if !cs.loaded {
            return;
        }
        let mut changed = false;
        for m in cs.msgs.iter_mut() {
            let Some(v) = journal.versions(m.id) else { continue };
            let t = m.meta.trace.get_or_insert_with(Default::default);
            if t.versions != v {
                t.versions = v.to_vec();
                changed = true;
            }
        }
        if changed {
            cs.rev.bump();
        }
        let ghosts = journal.ghosts(channel_id);
        if ghosts.is_empty() {
            return;
        }
        let lo = if cs.more { cs.msgs.front().map_or(u64::MAX, |m| m.id) } else { 0 };
        let hi = if cs.newer { cs.msgs.back().map_or(0, |m| m.id) } else { u64::MAX };
        for g in ghosts {
            if g.msg.id < lo || g.msg.id > hi {
                continue;
            }
            match cs.msgs.binary_search_by_key(&g.msg.id, |m| m.id) {
                // Encore dans la fenêtre (page du cache hors ligne) : marqué.
                Ok(i) => {
                    if let Some(m) = cs.msgs.get_mut(i)
                        && m.meta.trace.as_ref().is_none_or(|t| t.gone_ms.is_none())
                    {
                        m.meta.trace = Some(Box::new(Trace { gone_ms: Some(g.at), versions: g.versions.clone(), open: false }));
                        m.meta.reactions.clear();
                        cs.rev.bump();
                    }
                }
                Err(_) => cs.insert_sorted(g.message()),
            }
        }
    }

    /// Retire du magasin tout ce qui vient du journal (journal effacé,
    /// plugin coupé) : messages supprimés et versions précédentes.
    pub fn journal_strip(&mut self) {
        for cs in self.chans.values_mut() {
            let before = cs.msgs.len();
            cs.msgs.retain(|m| !m.meta.is_gone());
            cs.live.retain(|m| !m.meta.is_gone());
            let mut changed = cs.msgs.len() != before;
            for m in cs.msgs.iter_mut().chain(cs.live.iter_mut()) {
                changed |= m.meta.trace.take().is_some();
            }
            if changed {
                cs.rev.bump();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::event::Event;
    use crate::state::model::{Avatar, User};

    const C: u64 = 100;

    fn msg(id: u64, text: &str) -> Message {
        Message {
            id,
            channel_id: C,
            guild_id: None,
            author_id: 7,
            author: "Alice".into(),
            author_avatar: Avatar::default_for(7),
            content: text.into(),
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
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![crate::state::model::Dm::new(C, "Alice".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        s.apply(Event::History { channel_id: C, before: None, messages: vec![msg(10, "un"), msg(11, "deux"), msg(12, "trois")] });
        s
    }

    #[test]
    fn deleted_message_stays_marked_and_edit_keeps_the_old_text() {
        let mut s = store();
        let rev = s.chans[&C].rev;
        assert_eq!(s.message_in_memory(C, 11).map(|(m, _)| m.content.as_str()), Some("deux"));
        s.journal_mark_deleted(C, 11, 5000);
        let cs = &s.chans[&C];
        assert_eq!(cs.msgs.len(), 3, "le message reste dans la fenêtre");
        assert!(cs.msgs[1].meta.is_gone() && cs.rev != rev);

        s.journal_push_version(C, 12, &Version { at_ms: 6000, text: "trois".into() });
        s.apply(Event::MessageUpdate { channel_id: C, id: 12, content: "trois !".into() });
        let m = &s.chans[&C].msgs[2];
        assert_eq!(m.content, "trois !");
        assert_eq!(m.meta.trace.as_ref().map(|t| t.versions[0].text.as_str()), Some("trois"));
        assert!(s.journal_toggle(C, 12) && s.chans[&C].msgs[2].meta.trace.as_ref().unwrap().open);
        assert!(!s.journal_toggle(C, 10), "rien à déplier sans version");

        s.journal_strip();
        let cs = &s.chans[&C];
        assert_eq!(cs.msgs.iter().map(|m| m.id).collect::<Vec<_>>(), vec![10, 12]);
        assert!(cs.msgs.iter().all(|m| m.meta.trace.is_none()));
    }

    #[test]
    fn a_fresh_page_gets_its_ghosts_and_versions_back() {
        let mut s = store();
        let mut j = Journal::default();
        j.record_delete(&msg(11, "deux"), None, 5000, true);
        j.record_edit(C, 12, "trois", 6000);
        // La page fraîche de Discord n'a plus le message supprimé.
        s.apply(Event::History { channel_id: C, before: None, messages: vec![msg(10, "un"), msg(12, "trois !")] });
        s.journal_restore(C, &j);
        let cs = &s.chans[&C];
        assert_eq!(cs.msgs.iter().map(|m| (m.id, m.meta.is_gone())).collect::<Vec<_>>(), vec![(10, false), (11, true), (12, false)]);
        assert_eq!(cs.msgs[2].meta.trace.as_ref().map(|t| t.versions.len()), Some(1));
        // Deux fois : rien ne change.
        let rev = cs.rev;
        s.journal_restore(C, &j);
        assert_eq!(s.chans[&C].rev, rev);
    }
}
