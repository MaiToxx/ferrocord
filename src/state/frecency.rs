//! Favoris et fréquence d'usage du compte (lot G) : GIF favoris, emojis
//! favoris, emojis les plus utilisés. Ils vivent dans les réglages protobuf
//! de type 2 (`FrecencyUserSettings`), lus à la première ouverture du
//! sélecteur et jamais avant.
//!
//! Formes vérifiées sur des sources publiques :
//! - docs.discord.food, page « User Settings Proto » : type 2 (`FRECENCY`),
//!   `GET` et `PATCH /users/@me/settings-proto/2`, règle « the entire
//!   top-level field being modified must be sent » ;
//! - `discord-userdoccers/discord-protos`, `FrecencyUserSettings.proto` :
//!   numéros des champs repris ci-dessous.
//!
//! Mêmes règles que les réglages de type 1 (`state::usersettings`) : une
//! écriture ne part que d'un geste, une seule à la fois, optimiste et
//! rétablie octet pour octet si Discord refuse ; des réglages absents ou
//! illisibles ne sont jamais réécrits ; les champs inconnus sont gardés.
//! La fréquence d'usage du compte est lue, jamais écrite (voir
//! `docs/passerelle-utilisateur.md`) ; l'usage fait dans Ferrocord est gardé
//! sur cet ordinateur (`state::emoji_usage`) et passe devant elle.

use super::notice::{Notice, Problem};
use super::usersettings::proto::{Field, Msg, Value};
use super::usersettings::{ProtoReply, Refusal};

/// Champs de premier niveau de `FrecencyUserSettings`.
pub mod top {
    pub const VERSIONS: u32 = 1;
    pub const FAVORITE_GIFS: u32 = 2;
    pub const FAVORITE_EMOJIS: u32 = 5;
    pub const EMOJI_FRECENCY: u32 = 6;
}

/// `GIFType` : image (GIF, WebP) ou vidéo (MP4, WebM).
const GIF_TYPE_IMAGE: u64 = 1;
const GIF_TYPE_VIDEO: u64 = 2;
/// Favoris lus au plus (un compte en a rarement plus de quelques centaines).
pub const MAX_FAV_GIFS: usize = 2000;
pub const MAX_FAV_EMOJIS: usize = 500;
/// Emojis fréquents gardés, les mieux notés d'abord.
pub const MAX_FREQUENT: usize = 64;
/// Longueur maximale d'une adresse ou d'une clé lue.
const MAX_TEXT: usize = 1024;

/// GIF favori. `url` : la clé (adresse de la page du GIF, celle qu'on envoie) ;
/// `src` : le média.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FavGif {
    pub url: String,
    pub src: String,
    pub width: u32,
    pub height: u32,
    pub order: u32,
    /// Média vidéo (favori ajouté depuis le client officiel) : pas d'aperçu ici.
    pub video: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// Jamais demandés.
    #[default]
    Missing,
    Loading,
    /// Reçus mais illisibles : jamais réécrits.
    Unreadable,
    Ready(Msg),
}

struct Pending {
    id: u64,
    top: u32,
    before: Vec<Field>,
}

/// Écriture à envoyer : message ne contenant que le champ modifié, en entier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Write {
    pub id: u64,
    pub body: Vec<u8>,
    pub required_version: Option<u32>,
}

#[derive(Default)]
pub struct Frecency {
    state: State,
    pending: Option<Pending>,
    /// GIF favoris, les plus récents d'abord (lus une fois à chaque changement).
    pub gifs: Vec<FavGif>,
    /// Emojis favoris : identifiant d'un emoji personnalisé ou nom court.
    pub emojis: Vec<String>,
    /// Emojis les plus utilisés (mêmes clés), les mieux notés d'abord :
    /// l'usage local, puis celui du compte.
    pub frequent: Vec<String>,
    /// Ceux du compte seuls.
    account_frequent: Vec<String>,
    /// Usage fait dans Ferrocord, gardé sur cet ordinateur (F5).
    pub local: super::emoji_usage::LocalUse,
    next_id: u64,
}

fn text(m: &Msg, num: u32) -> Option<String> {
    m.string(num).filter(|s| !s.is_empty() && s.len() <= MAX_TEXT)
}

fn small(m: &Msg, num: u32) -> u32 {
    m.varint(num).and_then(|v| u32::try_from(v).ok()).unwrap_or(0)
}

/// Entrées d'une table protobuf (`map<string, V>` : champ 1 répété, clé en 1, valeur en 2).
fn entries(table: &Msg) -> impl Iterator<Item = (String, Msg)> + '_ {
    table.fields.iter().filter(|f| f.num == 1).filter_map(|f| {
        let Value::Len(b) = &f.value else { return None };
        let e = Msg::decode(b).ok()?;
        Some((text(&e, 1)?, e.message(2).ok().flatten().unwrap_or_default()))
    })
}

fn read_gifs(root: &Msg) -> Vec<FavGif> {
    let Ok(Some(table)) = root.message(top::FAVORITE_GIFS) else { return Vec::new() };
    let mut out: Vec<FavGif> = entries(&table)
        .filter_map(|(url, g)| Some(FavGif { url, src: text(&g, 2)?, width: small(&g, 3), height: small(&g, 4), order: small(&g, 5), video: g.varint(1) == Some(GIF_TYPE_VIDEO) }))
        .take(MAX_FAV_GIFS)
        .collect();
    out.sort_by_key(|g| std::cmp::Reverse(g.order));
    out
}

fn read_emojis(root: &Msg) -> Vec<String> {
    let Ok(Some(list)) = root.message(top::FAVORITE_EMOJIS) else { return Vec::new() };
    let mut out: Vec<String> = Vec::new();
    for f in list.fields.iter().filter(|f| f.num == 1) {
        if let Value::Len(b) = &f.value
            && let Ok(s) = std::str::from_utf8(b)
            && !s.is_empty()
            && s.len() <= MAX_TEXT
            && !out.iter().any(|x| x == s)
            && out.len() < MAX_FAV_EMOJIS
        {
            out.push(s.to_string());
        }
    }
    out
}

fn read_frequent(root: &Msg) -> Vec<String> {
    let Ok(Some(table)) = root.message(top::EMOJI_FRECENCY) else { return Vec::new() };
    // `score` est un int32 : écrit sur 64 bits quand il est négatif.
    let mut all: Vec<(i32, u32, String)> = entries(&table).map(|(k, v)| (v.varint(4).unwrap_or(0) as i32, small(&v, 1), k)).filter(|(score, uses, _)| *score > 0 || *uses > 0).collect();
    all.sort_by_key(|a| std::cmp::Reverse((a.0, a.1)));
    all.into_iter().take(MAX_FREQUENT).map(|(_, _, k)| k).collect()
}

impl Frecency {
    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn ready(&self) -> bool {
        matches!(self.state, State::Ready(_))
    }

    pub fn busy(&self) -> bool {
        self.pending.is_some()
    }

    /// Première ouverture du sélecteur : `true` s'il faut les demander.
    pub fn open(&mut self) -> bool {
        if self.state != State::Missing {
            return false;
        }
        self.state = State::Loading;
        true
    }

    pub fn is_fav_gif(&self, url: &str) -> bool {
        self.gifs.iter().any(|g| g.url == url)
    }

    pub fn is_fav_emoji(&self, key: &str) -> bool {
        self.emojis.iter().any(|k| k == key)
    }

    fn refresh(&mut self) {
        (self.gifs, self.emojis, self.account_frequent) = match &self.state {
            State::Ready(root) => (read_gifs(root), read_emojis(root), read_frequent(root)),
            _ => Default::default(),
        };
        self.merge_frequent();
    }

    /// « Fréquents » : l'usage local, puis celui du compte, sans doublon.
    fn merge_frequent(&mut self) {
        let mut out: Vec<String> = self.local.ranked().iter().take(MAX_FREQUENT).cloned().collect();
        for k in &self.account_frequent {
            if out.len() < MAX_FREQUENT && !out.contains(k) {
                out.push(k.clone());
            }
        }
        self.frequent = out;
    }

    /// Compte affiché (`None` : aucun) : son usage local est relu s'il a changé.
    pub fn bind_local(&mut self, me: Option<u64>, now_ms: i64) {
        if self.local.bind(me, now_ms) {
            self.merge_frequent();
        }
    }

    /// L'emoji `key` vient d'être choisi dans Ferrocord (F5).
    pub fn note_use(&mut self, key: &str, now_ms: i64) {
        self.local.note(key, now_ms);
        self.merge_frequent();
    }

    fn data_version(root: &Msg) -> Option<u32> {
        root.message(top::VERSIONS).ok().flatten()?.varint(3).and_then(|v| u32::try_from(v).ok())
    }

    /// Modifie le message du champ de premier niveau `top` ; rien n'est
    /// envoyé si les réglages sont inconnus, le champ illisible ou inchangé.
    fn edit_top(&mut self, top: u32, edit: impl FnOnce(&mut Msg)) -> Result<Write, Refusal> {
        if self.pending.is_some() {
            return Err(Refusal::Busy);
        }
        let State::Ready(root) = &mut self.state else { return Err(Refusal::Unknown) };
        let before = root.raw_field(top);
        let mut sub = root.message(top).map_err(|_| Refusal::Unknown)?.unwrap_or_default();
        let unchanged = sub.clone();
        edit(&mut sub);
        if sub == unchanged {
            return Err(Refusal::Unknown);
        }
        root.set(top, Value::Len(sub.encode()));
        let body = Msg { fields: root.raw_field(top) }.encode();
        let required_version = Self::data_version(root);
        self.next_id += 1;
        let id = self.next_id;
        self.pending = Some(Pending { id, top, before });
        self.refresh();
        Ok(Write { id, body, required_version })
    }

    /// Ajoute `gif` aux favoris (en tête), ou le retire s'il y est.
    pub fn toggle_gif(&mut self, gif: &FavGif) -> Result<Write, Refusal> {
        let present = self.is_fav_gif(&gif.url);
        let order = self.gifs.iter().map(|g| g.order).max().unwrap_or(0).saturating_add(1);
        self.edit_top(top::FAVORITE_GIFS, |table| {
            if present {
                table.fields.retain(|f| match &f.value {
                    Value::Len(b) if f.num == 1 => Msg::decode(b).ok().and_then(|e| e.string(1)).is_none_or(|k| k != gif.url),
                    _ => true,
                });
                return;
            }
            let mut value = Msg::default();
            value.set(1, Value::Varint(if gif.video { GIF_TYPE_VIDEO } else { GIF_TYPE_IMAGE }));
            value.set(2, Value::Len(gif.src.clone().into_bytes()));
            value.set(3, Value::Varint(u64::from(gif.width)));
            value.set(4, Value::Varint(u64::from(gif.height)));
            value.set(5, Value::Varint(u64::from(order)));
            let mut entry = Msg::default();
            entry.set(1, Value::Len(gif.url.clone().into_bytes()));
            entry.set(2, Value::Len(value.encode()));
            table.fields.push(Field { num: 1, value: Value::Len(entry.encode()) });
        })
    }

    /// Ajoute l'emoji `key` aux favoris (à la fin, comme Discord), ou le retire.
    pub fn toggle_emoji(&mut self, key: &str) -> Result<Write, Refusal> {
        if key.is_empty() || key.len() > MAX_TEXT {
            return Err(Refusal::Unknown);
        }
        let present = self.is_fav_emoji(key);
        self.edit_top(top::FAVORITE_EMOJIS, |list| {
            if present {
                list.fields.retain(|f| !(f.num == 1 && f.value == Value::Len(key.as_bytes().to_vec())));
            } else {
                list.fields.push(Field { num: 1, value: Value::Len(key.as_bytes().to_vec()) });
            }
        })
    }

    /// Réglages reçus (lecture, ou USER_SETTINGS_PROTO_UPDATE de type 2).
    pub fn received(&mut self, bytes: Option<Vec<u8>>, partial: bool) {
        let decoded = bytes.and_then(|b| Msg::decode(&b).ok());
        self.state = match (decoded, std::mem::take(&mut self.state)) {
            (Some(update), State::Ready(mut root)) if partial => {
                for top in update.top_numbers() {
                    root.restore_field(top, update.raw_field(top));
                }
                State::Ready(root)
            }
            // Mise à jour partielle sans base : l'état d'avant reste (ils
            // seront lus en entier à l'ouverture du sélecteur).
            (_, before @ (State::Missing | State::Loading)) if partial => before,
            (Some(m), _) if !partial => State::Ready(m),
            _ => State::Unreadable,
        };
        self.refresh();
    }

    pub fn load_failed(&mut self) {
        if self.state == State::Loading {
            self.state = State::Missing;
        }
    }

    fn revert(&mut self, p: Pending) {
        if let State::Ready(root) = &mut self.state {
            root.restore_field(p.top, p.before);
        }
    }

    /// Réponse à une écriture ; renvoie un message à afficher.
    pub fn written(&mut self, id: u64, result: Result<ProtoReply, Problem>) -> Option<Notice> {
        let pending = self.pending.take_if(|p| p.id == id)?;
        let notice = match result {
            Ok(ProtoReply { bytes, out_of_date: false }) => {
                if let Some(m) = bytes.and_then(|b| Msg::decode(&b).ok())
                    && let State::Ready(root) = &mut self.state
                {
                    for top in m.top_numbers() {
                        root.restore_field(top, m.raw_field(top));
                    }
                }
                None
            }
            Ok(ProtoReply { bytes, out_of_date: true }) => {
                self.revert(pending);
                if let Some(m) = bytes.and_then(|b| Msg::decode(&b).ok()) {
                    self.state = State::Ready(m);
                }
                Some(Notice::SettingOutdated)
            }
            Err(p) => {
                self.revert(pending);
                Some(Notice::SettingNotSaved(p))
            }
        };
        self.refresh();
        notice
    }
}

/// Réglages synthétiques (aperçu, tests) : favoris et fréquence donnés.
pub fn synthetic(gifs: &[FavGif], emojis: &[&str], frequent: &[(&str, u32)], data_version: u32) -> Vec<u8> {
    let mut root = Msg::default();
    let mut versions = Msg::default();
    versions.set(3, Value::Varint(u64::from(data_version)));
    root.set(top::VERSIONS, Value::Len(versions.encode()));
    let mut table = Msg::default();
    for g in gifs {
        let mut value = Msg::default();
        value.set(1, Value::Varint(if g.video { GIF_TYPE_VIDEO } else { GIF_TYPE_IMAGE }));
        value.set(2, Value::Len(g.src.clone().into_bytes()));
        value.set(3, Value::Varint(u64::from(g.width)));
        value.set(4, Value::Varint(u64::from(g.height)));
        value.set(5, Value::Varint(u64::from(g.order)));
        let mut entry = Msg::default();
        entry.set(1, Value::Len(g.url.clone().into_bytes()));
        entry.set(2, Value::Len(value.encode()));
        table.fields.push(Field { num: 1, value: Value::Len(entry.encode()) });
    }
    root.set(top::FAVORITE_GIFS, Value::Len(table.encode()));
    let list = Msg { fields: emojis.iter().map(|e| Field { num: 1, value: Value::Len(e.as_bytes().to_vec()) }).collect() };
    root.set(top::FAVORITE_EMOJIS, Value::Len(list.encode()));
    let mut freq = Msg::default();
    for (key, score) in frequent {
        let mut item = Msg::default();
        item.set(1, Value::Varint(u64::from(*score)));
        item.set(4, Value::Varint(u64::from(*score)));
        let mut entry = Msg::default();
        entry.set(1, Value::Len(key.as_bytes().to_vec()));
        entry.set(2, Value::Len(item.encode()));
        freq.fields.push(Field { num: 1, value: Value::Len(entry.encode()) });
    }
    root.set(top::EMOJI_FRECENCY, Value::Len(freq.encode()));
    root.encode()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gif(n: u32) -> FavGif {
        FavGif { url: format!("https://tenor.com/view/essai-{n}"), src: format!("https://media.tenor.com/essai{n}AAAAC/essai.gif"), width: 220, height: 124, order: n, video: false }
    }

    fn ready() -> Frecency {
        let mut f = Frecency::default();
        assert!(f.open());
        assert!(!f.open(), "une seule demande");
        f.received(Some(synthetic(&[gif(1), gif(2)], &["thumbsup", "7000000000000000501"], &[("fire", 90), ("joy", 300), ("jamais", 0)], 7)), false);
        f
    }

    #[test]
    fn reads_favorites_and_frequent_emojis() {
        let f = ready();
        assert!(f.ready());
        assert_eq!(f.gifs.iter().map(|g| g.order).collect::<Vec<_>>(), [2, 1], "les plus récents d'abord");
        assert_eq!(f.emojis, ["thumbsup", "7000000000000000501"]);
        assert_eq!(f.frequent, ["joy", "fire"], "par note, jamais utilisés écartés");
    }

    #[test]
    fn nothing_is_written_over_unknown_settings() {
        let mut f = Frecency::default();
        assert_eq!(f.toggle_emoji("fire"), Err(Refusal::Unknown));
        f.open();
        f.received(Some(vec![0xff]), false);
        assert_eq!(*f.state(), State::Unreadable);
        assert_eq!(f.toggle_gif(&gif(3)), Err(Refusal::Unknown));
        // Mise à jour partielle sans base : toujours à lire.
        let mut g = Frecency::default();
        g.received(Some(synthetic(&[], &["fire"], &[], 1)), true);
        assert_eq!(*g.state(), State::Missing);
    }

    #[test]
    fn toggle_sends_only_the_edited_field_and_keeps_unknown_fields() {
        let mut f = Frecency::default();
        f.open();
        // Un champ inconnu (9) dans la table des GIF, un autre (99) à la racine.
        let mut root = Msg::decode(&synthetic(&[gif(1)], &[], &[], 7)).unwrap_or_default();
        let mut table = root.message(top::FAVORITE_GIFS).ok().flatten().unwrap_or_default();
        table.set(2, Value::Varint(1));
        table.set(9, Value::Varint(5));
        root.set(top::FAVORITE_GIFS, Value::Len(table.encode()));
        root.set(99, Value::Varint(1));
        f.received(Some(root.encode()), false);

        let w = f.toggle_gif(&gif(8)).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(w.required_version, Some(7));
        let sent = Msg::decode(&w.body).unwrap_or_default();
        assert_eq!(sent.top_numbers(), [top::FAVORITE_GIFS]);
        let table = sent.message(top::FAVORITE_GIFS).ok().flatten().unwrap_or_default();
        assert_eq!((table.varint(2), table.varint(9)), (Some(1), Some(5)), "champs gardés");
        assert_eq!(f.gifs.iter().map(|g| (g.url.as_str(), g.order)).collect::<Vec<_>>(), [("https://tenor.com/view/essai-8", 2), ("https://tenor.com/view/essai-1", 1)]);
        assert_eq!(f.toggle_emoji("fire"), Err(Refusal::Busy), "une écriture à la fois");

        // Refus : rétabli à l'identique.
        assert_eq!(f.written(w.id, Err(Problem::Forbidden)), Some(Notice::SettingNotSaved(Problem::Forbidden)));
        assert_eq!(f.gifs.len(), 1);
        let State::Ready(now) = f.state() else { panic!("prêt") };
        assert_eq!(*now, root);

        // Retrait, accepté.
        let w = f.toggle_gif(&gif(1)).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(f.gifs.is_empty());
        assert_eq!(f.written(w.id, Ok(ProtoReply { bytes: None, out_of_date: false })), None);
        assert!(f.gifs.is_empty() && !f.busy());
    }

    #[test]
    fn emoji_favorites_toggle_and_outdated_write() {
        let mut f = ready();
        let w = f.toggle_emoji("fire").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(f.emojis, ["thumbsup", "7000000000000000501", "fire"]);
        let sent = Msg::decode(&w.body).unwrap_or_default();
        assert_eq!(sent.top_numbers(), [top::FAVORITE_EMOJIS]);
        // Modifiés ailleurs entre-temps : la réponse fait foi.
        let theirs = synthetic(&[], &["joy"], &[], 8);
        assert_eq!(f.written(w.id, Ok(ProtoReply { bytes: Some(theirs), out_of_date: true })), Some(Notice::SettingOutdated));
        assert_eq!(f.emojis, ["joy"]);
        let w = f.toggle_emoji("joy").unwrap_or_else(|e| panic!("{e:?}"));
        assert!(f.emojis.is_empty());
        assert_eq!(w.required_version, Some(8));
    }
}
