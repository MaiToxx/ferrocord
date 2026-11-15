//! Raccord du magasin avec la traduction d'un message (plugin, lot E3).
//! Module enfant de `store` (déclaré par `#[path]`).
//!
//! La traduction vit dans l'interface (`ui::translate`) ; le magasin ne
//! fait que porter ce qui s'affiche sous le message, dans
//! `MessageMeta::translated`. Rien ici n'est appelé quand le plugin est coupé.

use super::super::translate::Shown;
use super::Store;

impl Store {
    /// Pose (ou retire, `None`) ce qui s'affiche sous le message. Faux si
    /// le message n'est plus en mémoire ou si rien ne change.
    pub fn translation_set(&mut self, channel_id: u64, id: u64, shown: Option<Shown>) -> bool {
        let Some(cs) = self.chans.get_mut(&channel_id) else { return false };
        let Ok(i) = cs.msgs.binary_search_by_key(&id, |m| m.id) else { return false };
        let Some(m) = cs.msgs.get_mut(i) else { return false };
        let next = shown.map(Box::new);
        if m.meta.translated == next {
            return false;
        }
        m.meta.translated = next;
        cs.rev.bump();
        true
    }

    /// Retire toutes les traductions affichées (plugin coupé).
    pub fn translations_strip(&mut self) {
        for cs in self.chans.values_mut() {
            let mut changed = false;
            for m in cs.msgs.iter_mut().filter(|m| m.meta.translated.is_some()) {
                m.meta.translated = None;
                changed = true;
            }
            if changed {
                cs.rev.bump();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::journal::tests_support::message;
    use super::super::super::Event;
    use super::*;

    #[test]
    fn translation_is_carried_by_the_message_and_stripped() {
        const C: u64 = 9;
        let mut s = Store::new();
        s.select_channel(C);
        s.apply(Event::History { channel_id: C, before: None, messages: vec![message(10, C, "un"), message(11, C, "deux")] });
        let rev = s.chans[&C].rev;
        assert!(s.translation_set(C, 11, Some(Shown::Pending)));
        assert!(!s.translation_set(C, 11, Some(Shown::Pending)), "rien ne change");
        assert!(!s.translation_set(C, 99, Some(Shown::Pending)) && !s.translation_set(8, 11, Some(Shown::Pending)));
        assert!(s.chans[&C].rev != rev, "la liste se remet en page");
        assert_eq!(s.message_in_memory(C, 11).and_then(|(m, _)| m.meta.translated.as_deref().cloned()), Some(Shown::Pending));
        assert!(s.translation_set(C, 10, Some(Shown::Done { text: "one".into(), from: None, to: "en".into() })));
        s.translations_strip();
        assert!(s.chans[&C].msgs.iter().all(|m| m.meta.translated.is_none()));
        assert!(s.translation_set(C, 11, Some(Shown::Pending)) && s.translation_set(C, 11, None));
    }
}
