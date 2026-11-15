//! Journal des messages supprimés et modifiés (plugin, lot E1) : ce que le
//! client avait déjà reçu au moment où Discord a annoncé la suppression ou
//! la modification. Rien n'est demandé à Discord : on ne garde que ce qui
//! était en mémoire.
//!
//! Ici, seulement le modèle et sa forme texte (JSON) : aucun disque, aucun
//! chiffrement (`crate::offline::vault`, dans son fil). Tout est borné :
//! [`MAX_GHOSTS`] messages supprimés, [`MAX_EDITED`] messages modifiés,
//! [`MAX_VERSIONS`] versions par message, et une durée de conservation.

use super::model::Message;
use super::offline::SMsg;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Version du format : un autre numéro est ignoré.
pub const VERSION: u32 = 1;
/// Messages supprimés gardés au plus (les plus anciennement supprimés partent).
pub const MAX_GHOSTS: usize = 2000;
/// Messages modifiés suivis au plus.
pub const MAX_EDITED: usize = 2000;
/// Versions précédentes gardées par message (les plus anciennes partent).
pub const MAX_VERSIONS: usize = 10;
/// Longueur d'une version gardée (caractères) : celle d'un message de Discord.
pub const MAX_TEXT: usize = 4000;

/// Version précédente d'un message : son texte, et quand il a été remplacé.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct Version {
    /// Instant (ms Unix) où cette version a été remplacée.
    pub at_ms: i64,
    pub text: String,
}

/// Ce que le journal sait d'un message affiché (`MessageMeta::trace`).
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Trace {
    /// Supprimé chez Discord à cet instant (ms Unix) : gardé, atténué.
    pub gone_ms: Option<i64>,
    /// Versions précédentes, de la plus ancienne à la plus récente.
    pub versions: Vec<Version>,
    /// Versions dépliées sous le message.
    pub open: bool,
}

/// Message supprimé gardé.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ghost {
    pub chan: u64,
    /// Instant de la suppression (ms Unix).
    pub at: i64,
    pub msg: SMsg,
    pub versions: Vec<Version>,
}

impl Ghost {
    /// Message affichable, marqué supprimé.
    pub fn message(&self) -> Message {
        let mut m = self.msg.message(self.chan);
        m.meta.trace = Some(Box::new(Trace { gone_ms: Some(self.at), versions: self.versions.clone(), open: false }));
        m
    }
}

/// Versions précédentes d'un message encore présent.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Edited {
    pub chan: u64,
    pub id: u64,
    pub versions: Vec<Version>,
}

/// Forme enregistrée.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    version: u32,
    user: u64,
    ghosts: Vec<Ghost>,
    edits: Vec<Edited>,
}

/// Journal d'un compte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Journal {
    /// Messages supprimés par salon, triés par identifiant.
    ghosts: HashMap<u64, Vec<Ghost>>,
    ghost_count: usize,
    edits: HashMap<u64, Edited>,
    /// Quelque chose a changé depuis le dernier enregistrement.
    pub dirty: bool,
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(MAX_TEXT) {
        Some((i, _)) => text[..i].to_string(),
        None => text.to_string(),
    }
}

fn push_version(list: &mut Vec<Version>, v: Version) {
    list.push(v);
    if list.len() > MAX_VERSIONS {
        let over = list.len() - MAX_VERSIONS;
        list.drain(..over);
    }
}

impl Journal {
    pub fn is_empty(&self) -> bool {
        self.ghost_count == 0 && self.edits.is_empty()
    }

    /// (messages supprimés, messages modifiés) gardés.
    pub fn counts(&self) -> (usize, usize) {
        (self.ghost_count, self.edits.len())
    }

    /// Messages supprimés de ce salon, triés par identifiant.
    pub fn ghosts(&self, chan: u64) -> &[Ghost] {
        self.ghosts.get(&chan).map_or(&[], Vec::as_slice)
    }

    /// Le salon a-t-il quelque chose au journal ?
    pub fn touches(&self, chan: u64) -> bool {
        self.ghosts.contains_key(&chan) || self.edits.values().any(|e| e.chan == chan)
    }

    pub fn versions(&self, id: u64) -> Option<&[Version]> {
        self.edits.get(&id).map(|e| e.versions.as_slice())
    }

    pub fn is_ghost(&self, chan: u64, id: u64) -> bool {
        self.ghosts.get(&chan).is_some_and(|l| l.binary_search_by_key(&id, |g| g.msg.id).is_ok())
    }

    /// Le message `m` vient d'être supprimé chez Discord. `raw` : son texte
    /// d'origine si celui de `m` a été refait ; `keep_files` : garder la
    /// liste de ses pièces jointes. Les versions déjà suivies le rejoignent.
    pub fn record_delete(&mut self, m: &Message, raw: Option<&String>, now_ms: i64, keep_files: bool) {
        let mut msg = SMsg::of(m, raw);
        msg.content = clip(&msg.content);
        if !keep_files {
            msg.files.clear();
        }
        let versions = self.edits.remove(&m.id).map(|e| e.versions).unwrap_or_default();
        let list = self.ghosts.entry(m.channel_id).or_default();
        let at = list.partition_point(|g| g.msg.id < m.id);
        if list.get(at).is_some_and(|g| g.msg.id == m.id) {
            return;
        }
        list.insert(at, Ghost { chan: m.channel_id, at: now_ms, msg, versions });
        self.ghost_count += 1;
        self.dirty = true;
        if self.ghost_count > MAX_GHOSTS {
            self.evict_ghosts();
        }
    }

    /// Le texte du message `id` vient d'être remplacé : `old` est gardé.
    pub fn record_edit(&mut self, chan: u64, id: u64, old: &str, now_ms: i64) -> Version {
        let v = Version { at_ms: now_ms, text: clip(old) };
        let e = self.edits.entry(id).or_insert_with(|| Edited { chan, id, versions: Vec::new() });
        push_version(&mut e.versions, v.clone());
        self.dirty = true;
        if self.edits.len() > MAX_EDITED {
            self.evict_edits();
        }
        v
    }

    /// Un dixième des plus anciennement supprimés part d'un coup : une purge
    /// de modération ne trie pas le journal à chaque message.
    fn evict_ghosts(&mut self) {
        let mut ages: Vec<i64> = self.ghosts.values().flatten().map(|g| g.at).collect();
        let drop = (ages.len() - MAX_GHOSTS) + MAX_GHOSTS / 10;
        ages.sort_unstable();
        let Some(&limit) = ages.get(drop.saturating_sub(1)) else { return };
        let mut left = drop;
        self.ghosts.retain(|_, l| {
            l.retain(|g| {
                let go = left > 0 && g.at <= limit;
                left -= usize::from(go);
                !go
            });
            !l.is_empty()
        });
        self.recount();
    }

    fn evict_edits(&mut self) {
        let mut ages: Vec<i64> = self.edits.values().map(|e| e.versions.last().map_or(0, |v| v.at_ms)).collect();
        let drop = (ages.len() - MAX_EDITED) + MAX_EDITED / 10;
        ages.sort_unstable();
        let Some(&limit) = ages.get(drop.saturating_sub(1)) else { return };
        self.edits.retain(|_, e| e.versions.last().map_or(0, |v| v.at_ms) > limit);
    }

    fn recount(&mut self) {
        self.ghost_count = self.ghosts.values().map(Vec::len).sum();
    }

    /// Retire ce qui est plus vieux que `keep_days` jours ; vrai si quelque
    /// chose est parti.
    pub fn prune(&mut self, now_ms: i64, keep_days: i64) -> bool {
        let limit = now_ms - keep_days.max(1) * 86_400_000;
        let before = self.counts();
        self.ghosts.retain(|_, l| {
            l.retain(|g| g.at >= limit);
            !l.is_empty()
        });
        self.recount();
        self.edits.retain(|_, e| {
            e.versions.retain(|v| v.at_ms >= limit);
            !e.versions.is_empty()
        });
        let changed = before != self.counts();
        self.dirty |= changed;
        changed
    }

    /// Tout effacer (« Effacer le journal », plugin coupé, autre compte).
    pub fn clear(&mut self) {
        *self = Journal::default();
    }

    /// Ajoute ce que `other` (relu du disque) sait et que ce journal ignore.
    pub fn merge(&mut self, other: Journal) {
        for (chan, list) in other.ghosts {
            let mine = self.ghosts.entry(chan).or_default();
            for g in list {
                let at = mine.partition_point(|x| x.msg.id < g.msg.id);
                if mine.get(at).is_none_or(|x| x.msg.id != g.msg.id) {
                    mine.insert(at, g);
                }
            }
        }
        for (id, e) in other.edits {
            if self.is_ghost(e.chan, id) {
                continue;
            }
            match self.edits.get_mut(&id) {
                // Les versions du disque sont antérieures à celles de la session.
                Some(mine) => {
                    let mut all = e.versions;
                    all.append(&mut mine.versions);
                    all.dedup();
                    let over = all.len().saturating_sub(MAX_VERSIONS);
                    all.drain(..over);
                    mine.versions = all;
                }
                None => {
                    self.edits.insert(id, e);
                }
            }
        }
        self.recount();
        if self.ghost_count > MAX_GHOSTS {
            self.evict_ghosts();
        }
        if self.edits.len() > MAX_EDITED {
            self.evict_edits();
        }
    }

    /// Texte à enregistrer pour le compte `user`.
    pub fn encode(&self, user: u64) -> Vec<u8> {
        let mut ghosts: Vec<Ghost> = self.ghosts.values().flatten().cloned().collect();
        ghosts.sort_by_key(|g| (g.chan, g.msg.id));
        let mut edits: Vec<Edited> = self.edits.values().cloned().collect();
        edits.sort_by_key(|e| e.id);
        serde_json::to_vec(&Saved { version: VERSION, user, ghosts, edits }).unwrap_or_default()
    }

    /// Relit le journal du compte `user` ; abîmé, d'une autre version ou
    /// d'un autre compte : `None`.
    pub fn decode(bytes: &[u8], user: u64) -> Option<Journal> {
        let s: Saved = serde_json::from_slice(bytes).ok()?;
        if s.version != VERSION || s.user != user {
            return None;
        }
        let mut j = Journal::default();
        for mut g in s.ghosts.into_iter().take(MAX_GHOSTS) {
            g.versions.truncate(MAX_VERSIONS);
            let list = j.ghosts.entry(g.chan).or_default();
            let at = list.partition_point(|x| x.msg.id < g.msg.id);
            if list.get(at).is_none_or(|x| x.msg.id != g.msg.id) {
                list.insert(at, g);
            }
        }
        for mut e in s.edits.into_iter().take(MAX_EDITED) {
            e.versions.truncate(MAX_VERSIONS);
            if !e.versions.is_empty() {
                j.edits.insert(e.id, e);
            }
        }
        j.recount();
        Some(j)
    }
}

/// Texte simple d'un message tel qu'il est affiché (mentions en mots,
/// blocs sur leurs lignes) : sert aux versions gardées et à l'export.
pub fn plain_text(store: &super::store::Store, m: &Message, lang: crate::i18n::Lang, h24: bool) -> String {
    let doc = super::markdown::parse_message(store, m, lang, h24);
    let mut out = String::new();
    for b in &doc.blocks {
        if !out.is_empty() {
            out.push('\n');
        }
        for s in &b.spans {
            out.push_str(&s.text);
        }
    }
    out
}

/// Message d'essai (tests du journal, du coffre et de l'interface).
#[cfg(test)]
pub(crate) mod tests_support {
    use crate::state::model::{Avatar, Message};

    pub(crate) fn message(id: u64, chan: u64, text: &str) -> Message {
        Message {
            id,
            channel_id: chan,
            guild_id: None,
            author_id: 7,
            author: "Alice".into(),
            author_avatar: Avatar::default_for(7),
            content: text.into(),
            attachments: vec![crate::state::model::Attachment { id: 1, name: "plan.png".into(), ..Default::default() }],
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::tests_support::message as msg;
    use super::*;

    #[test]
    fn deletes_and_edits_are_kept_bounded_and_round_trip() {
        let mut j = Journal::default();
        assert!(j.is_empty() && !j.dirty);
        j.record_edit(9, 5, "premier jet", 1000);
        j.record_edit(9, 5, "deuxième jet", 2000);
        assert_eq!(j.versions(5).map(<[Version]>::len), Some(2));
        j.record_delete(&msg(5, 9, "troisième jet"), None, 3000, true);
        assert!(j.versions(5).is_none(), "les versions suivent le message supprimé");
        let g = &j.ghosts(9)[0];
        assert_eq!((g.at, g.msg.content.as_str(), g.versions.len(), g.msg.files.len()), (3000, "troisième jet", 2, 1));
        let shown = g.message();
        assert_eq!(shown.meta.trace.as_ref().and_then(|t| t.gone_ms), Some(3000));
        j.record_delete(&msg(5, 9, "doublon"), None, 4000, true);
        assert_eq!(j.counts(), (1, 0), "une suppression annoncée deux fois compte une fois");
        j.record_delete(&msg(6, 9, "sans fichier"), None, 4000, false);
        assert!(j.ghosts(9)[1].msg.files.is_empty());
        assert!(j.dirty && j.touches(9) && !j.touches(10));

        let back = Journal::decode(&j.encode(42), 42).unwrap();
        assert_eq!(back.counts(), (2, 0));
        assert_eq!(back.ghosts(9), j.ghosts(9));
        assert!(Journal::decode(&j.encode(42), 43).is_none(), "le journal d'un autre compte ne se lit pas");
        assert!(Journal::decode(b"{", 42).is_none());

        // Plus de MAX_VERSIONS : les plus anciennes partent.
        for i in 0..(MAX_VERSIONS as i64 + 3) {
            j.record_edit(9, 7, &format!("v{i}"), 5000 + i);
        }
        let v = j.versions(7).unwrap();
        assert_eq!((v.len(), v[0].text.as_str()), (MAX_VERSIONS, "v3"));
    }

    #[test]
    fn retention_and_capacity() {
        let mut j = Journal::default();
        let day = 86_400_000;
        j.record_delete(&msg(1, 9, "vieux"), None, 0, true);
        j.record_delete(&msg(2, 9, "récent"), None, 9 * day, true);
        j.record_edit(9, 3, "vieille version", 0);
        j.record_edit(9, 4, "version récente", 9 * day);
        assert!(j.prune(10 * day, 7));
        assert_eq!(j.counts(), (1, 1));
        assert_eq!(j.ghosts(9)[0].msg.id, 2);
        assert!(!j.prune(10 * day, 7));

        // Purge de modération : le journal reste borné.
        let mut j = Journal::default();
        for i in 0..(MAX_GHOSTS as u64 + 500) {
            j.record_delete(&msg(100 + i, 9, "x"), None, i as i64, false);
        }
        assert!(j.counts().0 <= MAX_GHOSTS);
        assert_eq!(j.ghosts(9).last().map(|g| g.msg.id), Some(100 + MAX_GHOSTS as u64 + 499), "les plus récents restent");
        for i in 0..(MAX_EDITED as u64 + 500) {
            j.record_edit(9, 10_000 + i, "x", i as i64);
        }
        assert!(j.counts().1 <= MAX_EDITED);
    }

    #[test]
    fn merge_keeps_both_sides() {
        let mut disk = Journal::default();
        disk.record_delete(&msg(1, 9, "du disque"), None, 10, true);
        disk.record_edit(9, 3, "v0", 10);
        let mut live = Journal::default();
        live.record_delete(&msg(2, 9, "de la session"), None, 20, true);
        live.record_edit(9, 3, "v1", 20);
        live.merge(disk);
        assert_eq!(live.counts(), (2, 1));
        assert_eq!(live.versions(3).unwrap().iter().map(|v| v.text.as_str()).collect::<Vec<_>>(), vec!["v0", "v1"]);
    }
}
