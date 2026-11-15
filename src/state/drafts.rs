//! Brouillons persistants (plugin du lot E3) : le texte non envoyé de chaque
//! salon, et la réponse en préparation, tels qu'ils sont écrits sur le
//! disque (chiffrés, `offline::vault`, dossier `brouillons`). Les pièces
//! jointes en attente ne sont pas gardées.
//!
//! Ici, seulement le contenu du fichier et ses bornes ; quand lire et
//! écrire est décidé par `ui::drafts`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const VERSION: u32 = 1;
/// Brouillons gardés au plus (les plus anciens partent).
pub const MAX_SAVED: usize = 200;
/// Caractères gardés au plus par brouillon (deux fois la limite d'un message Nitro).
pub const MAX_TEXT: usize = 8000;
/// Liens « texte lisible → syntaxe » gardés au plus par brouillon.
const MAX_LINKS: usize = 64;
const DAY_MS: i64 = 86_400_000;

/// Réponse en préparation dans un salon.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedReply {
    pub message_id: u64,
    pub author: String,
    pub mention: bool,
}

/// Brouillon d'un salon.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDraft {
    pub text: String,
    /// (texte lisible, syntaxe) des mentions et emojis choisis dans les listes.
    #[serde(default)]
    pub links: Vec<(String, String)>,
    #[serde(default)]
    pub reply: Option<SavedReply>,
    /// Dernière modification (ms).
    pub at: i64,
}

impl SavedDraft {
    fn is_empty(&self) -> bool {
        self.text.trim().is_empty() && self.reply.is_none()
    }

    /// Même contenu (la date ne compte pas).
    fn same(&self, other: &SavedDraft) -> bool {
        self.text == other.text && self.links == other.links && self.reply == other.reply
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    user: u64,
    drafts: Vec<(u64, SavedDraft)>,
}

/// Brouillons d'un compte, par salon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SavedDrafts {
    map: BTreeMap<u64, SavedDraft>,
}

impl SavedDrafts {
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn get(&self, channel: u64) -> Option<&SavedDraft> {
        self.map.get(&channel)
    }

    pub fn iter(&self) -> impl Iterator<Item = (u64, &SavedDraft)> {
        self.map.iter().map(|(c, d)| (*c, d))
    }

    /// Pose le brouillon de `channel` tel qu'il est à `now` ; inchangé, il
    /// garde sa date. Vide : retiré. Texte et liens bornés.
    pub fn put(&mut self, channel: u64, mut draft: SavedDraft, now: i64) {
        if draft.is_empty() {
            self.map.remove(&channel);
            return;
        }
        if draft.text.chars().count() > MAX_TEXT {
            draft.text = draft.text.chars().take(MAX_TEXT).collect();
        }
        draft.links.truncate(MAX_LINKS);
        draft.at = match self.map.get(&channel) {
            Some(old) if old.same(&draft) => old.at,
            _ => now,
        };
        self.map.insert(channel, draft);
    }

    /// Retire les brouillons plus vieux que `days` jours, puis les plus
    /// anciens au-delà de [`MAX_SAVED`]. Rend vrai si quelque chose est parti.
    pub fn prune(&mut self, now: i64, days: i64) -> bool {
        let before = self.map.len();
        let limit = now - days.max(1) * DAY_MS;
        self.map.retain(|_, d| d.at >= limit);
        while self.map.len() > MAX_SAVED {
            let Some(oldest) = self.map.iter().min_by_key(|(_, d)| d.at).map(|(c, _)| *c) else { break };
            self.map.remove(&oldest);
        }
        self.map.len() != before
    }

    pub fn encode(&self, user: u64) -> Vec<u8> {
        let drafts = self.map.iter().map(|(c, d)| (*c, d.clone())).collect();
        serde_json::to_vec(&Saved { version: VERSION, user, drafts }).unwrap_or_default()
    }

    /// Relit les brouillons du compte `user` ; abîmés, d'une autre version
    /// ou d'un autre compte : `None`.
    pub fn decode(bytes: &[u8], user: u64) -> Option<SavedDrafts> {
        let s: Saved = serde_json::from_slice(bytes).ok()?;
        if s.version != VERSION || s.user != user {
            return None;
        }
        let mut out = SavedDrafts::default();
        for (c, mut d) in s.drafts.into_iter().take(MAX_SAVED) {
            if d.is_empty() {
                continue;
            }
            if d.text.chars().count() > MAX_TEXT {
                d.text = d.text.chars().take(MAX_TEXT).collect();
            }
            d.links.truncate(MAX_LINKS);
            out.map.insert(c, d);
        }
        Some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(text: &str) -> SavedDraft {
        SavedDraft { text: text.into(), ..Default::default() }
    }

    #[test]
    fn round_trip_dates_and_bounds() {
        let mut d = SavedDrafts::default();
        d.put(5, draft("à finir"), 1000);
        d.put(6, SavedDraft { reply: Some(SavedReply { message_id: 9, author: "Alice".into(), mention: true }), ..Default::default() }, 1000);
        d.put(7, draft("   "), 1000);
        assert_eq!(d.len(), 2, "un brouillon vide n'est pas gardé, une réponse seule l'est");
        d.put(5, draft("à finir"), 5000);
        assert_eq!(d.get(5).map(|x| x.at), Some(1000), "inchangé : la date reste");
        d.put(5, draft("à finir demain"), 6000);
        assert_eq!(d.get(5).map(|x| x.at), Some(6000));
        d.put(8, draft(&"é".repeat(MAX_TEXT + 10)), 6000);
        assert_eq!(d.get(8).map(|x| x.text.chars().count()), Some(MAX_TEXT));
        let bytes = d.encode(42);
        assert_eq!(SavedDrafts::decode(&bytes, 42), Some(d.clone()));
        assert_eq!(SavedDrafts::decode(&bytes, 43), None, "autre compte");
        assert_eq!(SavedDrafts::decode(b"{", 42), None);
        d.put(5, draft(""), 7000);
        assert!(d.get(5).is_none());
    }

    #[test]
    fn old_drafts_are_pruned() {
        let mut d = SavedDrafts::default();
        d.put(1, draft("vieux"), 0);
        d.put(2, draft("récent"), 9 * DAY_MS);
        assert!(!d.prune(9 * DAY_MS, 30));
        assert!(d.prune(10 * DAY_MS, 7));
        assert!(d.get(1).is_none() && d.get(2).is_some());
        for c in 0..(MAX_SAVED as u64 + 5) {
            d.put(100 + c, draft("x"), 10 * DAY_MS + c as i64);
        }
        d.prune(10 * DAY_MS + 1000, 30);
        assert_eq!(d.len(), MAX_SAVED);
        assert!(d.get(2).is_none() && d.get(100).is_none(), "les plus anciens partent");
    }
}
