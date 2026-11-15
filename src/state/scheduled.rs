//! Messages programmés (plugin du lot E3) : gardés sur cet ordinateur, et
//! **jamais envoyés par le programme**. Ce modèle ne sait que ranger des
//! messages et dire lesquels sont arrivés à échéance ; il ne contient aucun
//! envoi. À l'échéance, l'interface (`ui::scheduled`) montre un bandeau et
//! une notification, et l'envoi ne part qu'au clic de l'utilisateur.
//!
//! Fichier : un par compte, chiffré (`offline::vault`, dossier `programmes`).

use serde::{Deserialize, Serialize};

const VERSION: u32 = 1;
/// Messages programmés gardés au plus.
pub const MAX_ITEMS: usize = 100;
/// Caractères gardés au plus par message (la limite d'un message avec Nitro).
pub const MAX_TEXT: usize = 4000;

/// Un message programmé.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub id: u64,
    pub channel_id: u64,
    /// Nom lisible du salon au moment de la programmation (« #général », « Alice »).
    pub place: String,
    /// Texte tel qu'il serait envoyé.
    pub content: String,
    /// Échéance (ms, heure de l'ordinateur).
    pub due_ms: i64,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    user: u64,
    next_id: u64,
    items: Vec<Item>,
}

/// Messages programmés d'un compte, du plus proche au plus lointain.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scheduled {
    items: Vec<Item>,
    next_id: u64,
    /// Quelque chose a changé depuis la dernière écriture.
    pub dirty: bool,
}

impl Scheduled {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn items(&self) -> &[Item] {
        &self.items
    }

    pub fn get(&self, id: u64) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }

    fn sort(&mut self) {
        self.items.sort_by_key(|i| (i.due_ms, i.id));
        self.dirty = true;
    }

    /// Range un message ; `None` : texte vide ou liste pleine.
    pub fn add(&mut self, channel_id: u64, place: &str, content: &str, due_ms: i64) -> Option<u64> {
        let content = content.trim();
        if content.is_empty() || self.items.len() >= MAX_ITEMS {
            return None;
        }
        self.next_id += 1;
        let id = self.next_id;
        self.items.push(Item { id, channel_id, place: place.chars().filter(|c| !c.is_control()).take(120).collect(), content: content.chars().take(MAX_TEXT).collect(), due_ms });
        self.sort();
        Some(id)
    }

    /// Change le texte et l'échéance ; faux si le message n'existe plus ou si le texte est vide.
    pub fn edit(&mut self, id: u64, content: &str, due_ms: i64) -> bool {
        let content = content.trim();
        let Some(i) = self.items.iter_mut().find(|i| i.id == id).filter(|_| !content.is_empty()) else { return false };
        i.content = content.chars().take(MAX_TEXT).collect();
        i.due_ms = due_ms;
        self.sort();
        true
    }

    /// Reporte l'échéance.
    pub fn postpone(&mut self, id: u64, due_ms: i64) -> bool {
        let Some(i) = self.items.iter_mut().find(|i| i.id == id) else { return false };
        i.due_ms = due_ms;
        self.sort();
        true
    }

    /// Retire un message (supprimé, ou envoyé d'un clic) et le rend.
    pub fn remove(&mut self, id: u64) -> Option<Item> {
        let at = self.items.iter().position(|i| i.id == id)?;
        self.dirty = true;
        Some(self.items.remove(at))
    }

    /// Messages arrivés à échéance à `now`, du plus ancien au plus récent.
    pub fn due(&self, now: i64) -> impl Iterator<Item = &Item> {
        self.items.iter().take_while(move |i| i.due_ms <= now)
    }

    /// Prochaine échéance à venir après `now`.
    pub fn next_due(&self, now: i64) -> Option<i64> {
        self.items.iter().map(|i| i.due_ms).find(|d| *d > now)
    }

    pub fn encode(&self, user: u64) -> Vec<u8> {
        serde_json::to_vec(&Saved { version: VERSION, user, next_id: self.next_id, items: self.items.clone() }).unwrap_or_default()
    }

    /// Relit les messages programmés du compte `user` ; abîmés, d'une autre
    /// version ou d'un autre compte : `None`.
    pub fn decode(bytes: &[u8], user: u64) -> Option<Scheduled> {
        let s: Saved = serde_json::from_slice(bytes).ok()?;
        if s.version != VERSION || s.user != user {
            return None;
        }
        let mut items: Vec<Item> = s.items.into_iter().filter(|i| !i.content.trim().is_empty()).take(MAX_ITEMS).collect();
        for i in &mut items {
            if i.content.chars().count() > MAX_TEXT {
                i.content = i.content.chars().take(MAX_TEXT).collect();
            }
        }
        items.sort_by_key(|i| (i.due_ms, i.id));
        let next_id = items.iter().map(|i| i.id).max().unwrap_or(0).max(s.next_id);
        Some(Scheduled { items, next_id, dirty: false })
    }

    /// Ajoute ce que le disque gardait à ce qui a été programmé depuis le
    /// lancement (identifiants refaits pour ces derniers s'ils se croisent).
    pub fn merge(&mut self, loaded: Scheduled) {
        let mine = std::mem::take(&mut self.items);
        self.items = loaded.items;
        self.next_id = self.next_id.max(loaded.next_id);
        for mut i in mine {
            if self.items.len() >= MAX_ITEMS {
                break;
            }
            self.next_id += 1;
            i.id = self.next_id;
            self.items.push(i);
            self.dirty = true;
        }
        self.items.sort_by_key(|i| (i.due_ms, i.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_are_ordered_due_and_round_trip() {
        let mut s = Scheduled::default();
        assert_eq!(s.add(5, "#général", "   ", 1000), None, "texte vide");
        let late = s.add(5, "#général", "plus tard", 9000).unwrap();
        let soon = s.add(6, "Alice", " bientôt ", 2000).unwrap();
        assert_eq!(s.items().iter().map(|i| i.id).collect::<Vec<_>>(), vec![soon, late]);
        assert_eq!(s.get(soon).map(|i| i.content.as_str()), Some("bientôt"));
        assert_eq!(s.due(1999).count(), 0);
        assert_eq!(s.due(2000).map(|i| i.id).collect::<Vec<_>>(), vec![soon]);
        assert_eq!((s.next_due(0), s.next_due(2000), s.next_due(9000)), (Some(2000), Some(9000), None));
        assert!(s.dirty);
        let bytes = s.encode(42);
        let mut back = Scheduled::decode(&bytes, 42).unwrap();
        assert_eq!(back.items(), s.items());
        assert!(!back.dirty);
        assert_eq!(Scheduled::decode(&bytes, 43), None, "autre compte");
        assert_eq!(Scheduled::decode(b"[]", 42), None);
        assert!(back.postpone(soon, 20_000) && back.dirty);
        assert_eq!(back.items()[0].id, late);
        assert!(back.edit(late, "changé", 30_000) && !back.edit(late, "  ", 1) && !back.edit(999, "x", 1));
        assert_eq!(back.remove(late).map(|i| i.content), Some("changé".into()));
        assert_eq!(back.remove(late), None);
        assert_eq!(back.add(7, "x", "nouveau", 1), Some(late.max(soon) + 1), "un identifiant ne resert pas");
    }

    #[test]
    fn list_is_bounded_and_merge_keeps_both() {
        let mut s = Scheduled::default();
        for n in 0..MAX_ITEMS {
            assert!(s.add(1, "x", "m", n as i64).is_some());
        }
        assert_eq!(s.add(1, "x", "de trop", 0), None);
        let mut session = Scheduled::default();
        session.add(2, "y", "tapé avant la lecture", 500);
        let mut disk = Scheduled::default();
        disk.add(3, "z", "du disque", 100);
        disk.dirty = false;
        session.merge(disk);
        assert_eq!(session.items().iter().map(|i| (i.id, i.channel_id)).collect::<Vec<_>>(), vec![(1, 3), (2, 2)]);
        assert!(session.dirty);
    }
}
