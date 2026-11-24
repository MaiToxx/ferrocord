//! Index de la recherche locale (plugin, lot E1) : les messages que le
//! client a déjà reçus, cherchés sur place, sans rien demander à Discord.
//!
//! Borné : au plus [`MAX_ENTRIES`] messages et [`MAX_BYTES`] octets en
//! mémoire (les plus anciens partent d'abord), au plus [`MAX_FILE`] octets
//! une fois enregistré. Chercher, c'est parcourir ces entrées : quelques
//! millisecondes au plus, pendant la frappe. Ni egui ni réseau ici.

use super::model::Message;
use super::offline::SMsg;
use super::search::{fold, Has, Query, Scope, Sort};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const VERSION: u32 = 1;
/// Messages gardés au plus.
pub const MAX_ENTRIES: usize = 20_000;
/// Octets gardés au plus en mémoire (textes, noms, adresses des fichiers).
pub const MAX_BYTES: usize = 4 << 20;
/// Taille du fichier au plus, avant chiffrement.
pub const MAX_FILE: usize = 6 << 20;
/// Coût fixe compté par message (identifiants, structures).
const BASE_COST: usize = 96;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct Entry {
    chan: u64,
    msg: SMsg,
    /// Texte et noms des fichiers, en minuscules sans accents.
    #[serde(skip)]
    folded: String,
}

impl Entry {
    fn new(chan: u64, msg: SMsg) -> Entry {
        let mut folded = fold(&msg.content);
        for f in &msg.files {
            folded.push('\n');
            folded.push_str(&fold(&f.name));
        }
        Entry { chan, msg, folded }
    }

    fn cost(&self) -> usize {
        let files: usize = self.msg.files.iter().map(|f| f.name.len() + f.url.len() + f.proxy_url.len() + 48).sum();
        BASE_COST + self.msg.content.len() + self.folded.len() + self.msg.author.len() + files + self.msg.mentions.iter().map(|(_, n)| n.len() + 8).sum::<usize>()
    }

    fn has(&self, h: Has) -> bool {
        let kind = |prefix: &str, exts: &[&str]| {
            self.msg.files.iter().any(|f| {
                f.content_type.as_deref().is_some_and(|c| c.starts_with(prefix)) || f.name.rsplit_once('.').is_some_and(|(_, e)| exts.contains(&e.to_ascii_lowercase().as_str()))
            })
        };
        match h {
            Has::Link => self.msg.content.contains("https://") || self.msg.content.contains("http://"),
            Has::File => !self.msg.files.is_empty(),
            Has::Image => kind("image/", &["png", "jpg", "jpeg", "gif", "webp", "avif"]),
            Has::Video => kind("video/", &["mp4", "webm", "mov", "mkv"]),
            Has::Sound => kind("audio/", &["mp3", "ogg", "wav", "flac", "m4a", "opus"]),
            Has::Embed | Has::Sticker | Has::Poll => false,
        }
    }
}

#[derive(Serialize, Deserialize)]
struct Saved {
    version: u32,
    user: u64,
    entries: Vec<Entry>,
}

/// La requête demande ce que l'index ne sait pas (épinglé, type d'auteur,
/// intégration, autocollant, sondage) : la recherche locale la refuse
/// plutôt que de répondre à côté.
pub fn unsupported(q: &Query) -> bool {
    q.pinned.is_some() || !q.author_types.is_empty() || q.has.iter().any(|h| matches!(h, Has::Embed | Has::Sticker | Has::Poll))
}

/// Mots d'une requête, repliés comme l'index.
pub fn fold_terms(terms: &[String]) -> Vec<String> {
    terms.iter().map(|t| fold(t)).collect()
}

#[derive(Clone, Debug, Default)]
pub struct LocalIndex {
    entries: BTreeMap<u64, Entry>,
    bytes: usize,
    /// Changé depuis le dernier enregistrement.
    pub dirty: bool,
}

impl LocalIndex {
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn bytes(&self) -> usize {
        self.bytes
    }

    pub fn clear(&mut self) {
        self.dirty |= !self.entries.is_empty();
        self.entries.clear();
        self.bytes = 0;
    }

    fn put(&mut self, e: Entry) {
        self.bytes += e.cost();
        if let Some(old) = self.entries.insert(e.msg.id, e) {
            self.bytes -= old.cost();
        }
        while self.entries.len() > MAX_ENTRIES || self.bytes > MAX_BYTES {
            let Some((_, old)) = self.entries.pop_first() else { break };
            self.bytes -= old.cost();
        }
    }

    /// Range `m` (ou sa nouvelle version) ; faux s'il y était déjà tel quel
    /// ou s'il n'a rien à chercher (message système, supprimé, vide).
    pub fn upsert(&mut self, m: &Message, raw: Option<&String>) -> bool {
        if m.system.is_some() || m.meta.is_gone() || (m.content.is_empty() && m.attachments.is_empty()) {
            return false;
        }
        let content = raw.unwrap_or(&m.content);
        if self.entries.get(&m.id).is_some_and(|e| e.msg.content == *content && e.msg.edited == m.edited && e.msg.files.len() == m.attachments.len()) {
            return false;
        }
        self.put(Entry::new(m.channel_id, SMsg::of(m, raw)));
        self.dirty = true;
        true
    }

    pub fn remove(&mut self, id: u64) {
        if let Some(old) = self.entries.remove(&id) {
            self.bytes -= old.cost();
            self.dirty = true;
        }
    }

    /// Le message `id`, affichable.
    pub fn message(&self, id: u64) -> Option<Message> {
        self.entries.get(&id).map(|e| e.msg.message(e.chan))
    }

    /// Identifiants des messages qui répondent à la requête, dans l'ordre
    /// demandé (« pertinence » : les plus récents d'abord). Tous les mots
    /// doivent y être, n'importe où dans le texte, casse et accents ignorés.
    pub fn search(&self, scope: Scope, q: &Query, terms: &[String]) -> Vec<u64> {
        if unsupported(q) {
            return Vec::new();
        }
        let keep = |e: &Entry| {
            let here = match scope {
                Scope::Guild(g) => e.msg.guild == Some(g),
                Scope::Channel(c) => e.chan == c,
            };
            here && (q.channels.is_empty() || q.channels.contains(&e.chan))
                && (q.authors.is_empty() || q.authors.contains(&e.msg.author_id))
                && q.mentions.iter().all(|u| e.msg.mentions.iter().any(|(id, _)| id == u))
                && q.has.iter().all(|h| e.has(*h))
                && terms.iter().all(|t| e.folded.contains(t.as_str()))
        };
        let low = q.min_id.map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded);
        let high = q.max_id.map_or(std::ops::Bound::Unbounded, std::ops::Bound::Excluded);
        if matches!((q.min_id, q.max_id), (Some(a), Some(b)) if a >= b) {
            return Vec::new();
        }
        let found = self.entries.range((low, high)).filter(|(_, e)| keep(e)).map(|(id, _)| *id);
        if q.sort == Sort::Oldest { found.collect() } else { found.rev().collect() }
    }

    /// Copie à enregistrer : sans les textes repliés, que le fichier ne
    /// contient pas (la moitié de la copie, refaite à chaque enregistrement).
    /// Elle ne sert qu'à [`Self::encode`] : chercher dedans ne trouverait rien.
    pub fn for_saving(&self) -> LocalIndex {
        let entries = self.entries.iter().map(|(id, e)| (*id, Entry { chan: e.chan, msg: e.msg.clone(), folded: String::new() })).collect();
        LocalIndex { entries, bytes: self.bytes, dirty: false }
    }

    /// Ajoute ce qui a été relu du disque ; ce que la session a déjà rangé
    /// (plus frais) n'est pas remplacé.
    pub fn merge(&mut self, other: LocalIndex) {
        for (id, e) in other.entries {
            if !self.entries.contains_key(&id) {
                self.put(e);
            }
        }
    }

    /// Texte à enregistrer pour le compte `user` ; au-delà de [`MAX_FILE`],
    /// les plus anciens n'y sont pas.
    pub fn encode(&self, user: u64) -> Vec<u8> {
        let mut entries: Vec<&Entry> = self.entries.values().collect();
        loop {
            #[derive(Serialize)]
            struct Out<'a> {
                version: u32,
                user: u64,
                entries: &'a [&'a Entry],
            }
            let bytes = serde_json::to_vec(&Out { version: VERSION, user, entries: &entries }).unwrap_or_default();
            if bytes.len() <= MAX_FILE || entries.is_empty() {
                return bytes;
            }
            let cut = (entries.len() / 4).max(1);
            entries.drain(..cut);
        }
    }

    /// Relit l'index du compte `user` ; abîmé, d'une autre version ou d'un
    /// autre compte : `None`.
    pub fn decode(bytes: &[u8], user: u64) -> Option<LocalIndex> {
        let s: Saved = serde_json::from_slice(bytes).ok()?;
        if s.version != VERSION || s.user != user {
            return None;
        }
        let mut index = LocalIndex::default();
        for e in s.entries.into_iter().rev().take(MAX_ENTRIES) {
            index.put(Entry::new(e.chan, e.msg));
        }
        Some(index)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::journal::tests_support::message;

    fn index() -> LocalIndex {
        let mut ix = LocalIndex::default();
        let mut a = message(10, 100, "Réunion demain à 10 h");
        a.guild_id = Some(1);
        let mut b = message(11, 100, "voir https://exemple.invalid/plan");
        b.guild_id = Some(1);
        b.author_id = 8;
        b.attachments.clear();
        let mut c = message(12, 200, "la reunion est annulée");
        c.guild_id = Some(1);
        let d = message(13, 300, "réunion en privé");
        for m in [&a, &b, &c, &d] {
            assert!(ix.upsert(m, None));
        }
        ix
    }

    fn q() -> Query {
        Query::default()
    }

    fn words(s: &str) -> Vec<String> {
        fold_terms(&s.split(' ').map(str::to_string).collect::<Vec<_>>())
    }

    #[test]
    fn finds_words_without_case_or_accents_within_the_scope() {
        let ix = index();
        assert_eq!(ix.search(Scope::Guild(1), &q(), &words("REUNION")), vec![12, 10]);
        assert_eq!(ix.search(Scope::Guild(1), &q(), &words("réu")), vec![12, 10], "pendant la frappe : un début de mot suffit");
        assert_eq!(ix.search(Scope::Channel(300), &q(), &words("réunion")), vec![13]);
        assert_eq!(ix.search(Scope::Guild(1), &q(), &words("réunion annulée")), vec![12]);
        assert_eq!(ix.search(Scope::Guild(1), &Query { sort: Sort::Oldest, ..q() }, &words("reunion")), vec![10, 12]);
        assert!(ix.search(Scope::Guild(2), &q(), &words("reunion")).is_empty());
    }

    #[test]
    fn filters_author_channel_date_file_and_link() {
        let ix = index();
        let all = |query: Query| ix.search(Scope::Guild(1), &query, &[]);
        assert_eq!(all(Query { authors: vec![8], ..q() }), vec![11]);
        assert_eq!(all(Query { channels: vec![200], ..q() }), vec![12]);
        assert_eq!(all(Query { has: vec![Has::Link], ..q() }), vec![11]);
        assert_eq!(all(Query { has: vec![Has::File], ..q() }), vec![12, 10]);
        assert_eq!(all(Query { has: vec![Has::Image], ..q() }), vec![12, 10], "plan.png");
        assert_eq!(all(Query { min_id: Some(10), max_id: Some(12), ..q() }), vec![11]);
        assert!(all(Query { min_id: Some(12), max_id: Some(10), ..q() }).is_empty());
        // Ce que l'index ne sait pas : refusé, jamais une réponse à côté.
        assert!(unsupported(&Query { pinned: Some(true), ..q() }));
        assert!(all(Query { has: vec![Has::Poll], ..q() }).is_empty());
    }

    #[test]
    fn edits_replace_deletes_remove_and_nothing_empty_is_kept() {
        let mut ix = index();
        ix.dirty = false;
        let mut a = message(10, 100, "Réunion demain à 10 h");
        a.guild_id = Some(1);
        assert!(!ix.upsert(&a, None), "déjà rangé tel quel");
        assert!(!ix.dirty);
        a.content = "Atelier demain".into();
        a.edited = true;
        assert!(ix.upsert(&a, None));
        assert_eq!(ix.search(Scope::Guild(1), &q(), &words("reunion")), vec![12]);
        assert_eq!(ix.search(Scope::Guild(1), &q(), &words("atelier")), vec![10]);
        assert_eq!(ix.message(10).map(|m| (m.channel_id, m.content)), Some((100, "Atelier demain".into())));
        ix.remove(12);
        assert!(ix.search(Scope::Guild(1), &q(), &words("annulee")).is_empty());
        let mut empty = message(20, 100, "");
        empty.attachments.clear();
        assert!(!ix.upsert(&empty, None));
        let mut gone = message(21, 100, "supprimé");
        gone.meta.trace = Some(Box::new(crate::state::journal::Trace { gone_ms: Some(1), ..Default::default() }));
        assert!(!ix.upsert(&gone, None), "un message supprimé gardé par le journal n'est pas indexé");
        assert_eq!(ix.len(), 3);
    }

    #[test]
    fn stays_bounded_and_round_trips() {
        let mut ix = LocalIndex::default();
        for id in 1..=(MAX_ENTRIES as u64 + 50) {
            let mut m = message(id, 100, "mot");
            m.attachments.clear();
            ix.upsert(&m, None);
        }
        assert_eq!(ix.len(), MAX_ENTRIES);
        assert!(ix.message(50).is_none() && ix.message(51).is_some(), "les plus anciens partent d'abord");
        assert!(ix.bytes() <= MAX_BYTES);
        // Gros messages : c'est la taille qui borne.
        let mut big = LocalIndex::default();
        let text = "é".repeat(2000);
        for id in 1..=2000 {
            let mut m = message(id, 100, &text);
            m.attachments.clear();
            big.upsert(&m, None);
        }
        assert!(big.bytes() <= MAX_BYTES && big.len() < 2000 && big.message(2000).is_some());
        let bytes = big.encode(7);
        assert!(bytes.len() <= MAX_FILE);
        let back = LocalIndex::decode(&bytes, 7).expect("relu");
        assert_eq!(back.len(), big.len());
        assert_eq!(back.search(Scope::Channel(100), &q(), &words("ee")).len(), big.len(), "le texte replié est refait à la lecture");
        assert!(LocalIndex::decode(&bytes, 8).is_none(), "autre compte");
        assert!(LocalIndex::decode(b"{", 7).is_none());
        // Relu du disque : ne remplace pas ce que la session a déjà.
        let mut session = LocalIndex::default();
        let mut fresh = message(2000, 100, "version fraîche");
        fresh.attachments.clear();
        session.upsert(&fresh, None);
        session.merge(back);
        assert_eq!(session.message(2000).unwrap().content, "version fraîche");
        // La copie à enregistrer donne le même fichier que l'index lui-même.
        assert_eq!(session.for_saving().encode(7), session.encode(7));
        assert_eq!(session.len(), big.len());
    }
}
