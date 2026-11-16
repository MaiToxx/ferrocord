//! Décodage des données sociales des comptes utilisateur : relations,
//! états de lecture, membres des MP, présences des amis.
//!
//! Formes vérifiées sur la documentation communautaire de l'API utilisateur
//! (docs.discord.food, « Relationships », « Gateway Events » ; discord-userdoccers,
//! « Read State ») et, pour les présences et les MP, sur la documentation
//! officielle de Discord (Presence Update, Channel Object). Chaque élément est
//! décodé à part : une forme inattendue l'écarte seul, jamais le paquet.

use super::Out;
use crate::state::model::{Avatar, ImageHash};
use crate::state::relations::{DmInfo, Person, ReadState, RelKind, Relation, RelationsEvent};
use crate::state::Event;
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use std::collections::{HashMap, HashSet};

/// Élément lu à part : `None` s'il a une forme inattendue.
struct Lenient<T>(Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        super::raw::with_raw(d, |raw| Lenient(serde_json::from_str(raw).ok()))
    }
}

fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(Lenient::<T>::deserialize(d)?.0)
}

/// Identifiant en chaîne (forme documentée) ou en nombre ; `None` sinon.
fn loose_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    super::raw::de_loose_id(d)
}

/// Champ présent (même `null`) : `Some`, avec sa valeur si c'est un texte.
fn present_str<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    super::raw::with_raw(d, |raw| Some(serde_json::from_str::<String>(raw).ok()))
}

fn items<T>(v: Option<Vec<Lenient<T>>>) -> impl Iterator<Item = T> {
    v.into_iter().flatten().filter_map(|x| x.0)
}

#[derive(Deserialize)]
struct RawPerson {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    global_name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    avatar: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    discriminator: Option<String>,
}

impl RawPerson {
    fn person(&self) -> Option<Person> {
        let id = self.id?;
        let username = self.username.clone().unwrap_or_default();
        let name = self.global_name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| username.clone());
        Some(Person { id, name, username, avatar: Avatar::of_user(id, self.avatar.as_deref().and_then(ImageHash::parse), self.discriminator.as_deref()) })
    }
}

#[derive(Deserialize)]
struct RawRelation {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(rename = "type", default, deserialize_with = "lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "present_str")]
    nickname: Option<Option<String>>,
    #[serde(default, deserialize_with = "lenient")]
    user: Option<RawPerson>,
    #[serde(default, deserialize_with = "loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    user_ignored: Option<bool>,
}

impl RawRelation {
    fn id(&self) -> Option<u64> {
        self.id.or(self.user.as_ref().and_then(|u| u.id)).or(self.user_id)
    }

    fn relation(&self) -> Option<Relation> {
        Some(Relation {
            id: self.id()?,
            kind: RelKind::from_code(self.kind?),
            nickname: self.nickname.clone().flatten(),
            ignored: self.user_ignored.unwrap_or(false),
            user: self.user.as_ref().and_then(RawPerson::person),
        })
    }
}

#[derive(Deserialize)]
struct RawReadState {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    read_state_type: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    last_message_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    mention_count: Option<u64>,
}

/// `read_state` : tableau, ou structure versionnée (`entries`) avec la
/// capacité VERSIONED_READ_STATES.
enum RawReadStates {
    Versioned { entries: Vec<Lenient<RawReadState>> },
    List(Vec<Lenient<RawReadState>>),
}

/// Lu à la main : un `untagged` de serde passerait par une copie
/// intermédiaire où les éléments lus à part (`Lenient`) ne peuvent pas l'être.
impl<'de> Deserialize<'de> for RawReadStates {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Versioned {
            entries: Vec<Lenient<RawReadState>>,
        }
        super::raw::with_raw(d, |raw| {
            if let Ok(v) = serde_json::from_str::<Versioned>(raw) {
                return Ok(RawReadStates::Versioned { entries: v.entries });
            }
            serde_json::from_str(raw).map(RawReadStates::List)
        })?
        .map_err(serde::de::Error::custom)
    }
}

impl RawReadStates {
    fn states(self) -> Vec<ReadState> {
        let list = match self {
            RawReadStates::Versioned { entries } => entries,
            RawReadStates::List(l) => l,
        };
        items(Some(list))
            // Seuls les états de salon (type 0, absent par défaut) comptent ici.
            .filter(|r| r.read_state_type.unwrap_or(0) == 0)
            .filter_map(|r| {
                Some(ReadState {
                    channel_id: r.id?,
                    last_acked: r.last_message_id.unwrap_or(0),
                    mentions: r.mention_count.and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
                })
            })
            .collect()
    }
}

#[derive(Deserialize)]
struct RawPrivate {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(rename = "type", default, deserialize_with = "lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    owner_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    recipients: Option<Vec<Lenient<RawPerson>>>,
    #[serde(default, deserialize_with = "lenient")]
    recipient_ids: Option<Vec<Lenient<String>>>,
    #[serde(default, deserialize_with = "lenient")]
    is_message_request: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    is_spam: Option<bool>,
    #[serde(default, deserialize_with = "loose_id")]
    last_message_id: Option<u64>,
}

impl RawPrivate {
    /// MP (type 1) ou groupe (type 3), ses personnes, et si le paquet
    /// listait vraiment ses destinataires.
    fn info(self) -> Option<(DmInfo, Vec<Person>, bool)> {
        let group = match self.kind? {
            1 => false,
            3 => true,
            _ => return None,
        };
        if self.guild_id.is_some() {
            return None;
        }
        let known = self.recipients.is_some() || self.recipient_ids.is_some();
        let people: Vec<Person> = items(self.recipients).filter_map(|p| p.person()).collect();
        let mut recipients: Vec<u64> = people.iter().map(|p| p.id).collect();
        for id in items(self.recipient_ids).filter_map(|s| s.parse::<u64>().ok()) {
            if !recipients.contains(&id) {
                recipients.push(id);
            }
        }
        Some((DmInfo { id: self.id?, group, owner: self.owner_id, recipients, request: self.is_message_request.unwrap_or(false), spam: self.is_spam.unwrap_or(false) }, people, known))
    }
}

#[derive(Deserialize)]
struct RawHead {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    last_message_id: Option<u64>,
}

#[derive(Deserialize)]
struct RawGuildHeads {
    #[serde(default, deserialize_with = "lenient")]
    channels: Option<Vec<Lenient<RawHead>>>,
}

#[derive(Deserialize)]
struct RawSocialReady {
    #[serde(default, deserialize_with = "lenient")]
    relationships: Option<Vec<Lenient<RawRelation>>>,
    #[serde(default, deserialize_with = "lenient")]
    read_state: Option<RawReadStates>,
    #[serde(default, deserialize_with = "lenient")]
    private_channels: Option<Vec<Lenient<RawPrivate>>>,
    #[serde(default, deserialize_with = "lenient")]
    users: Option<Vec<Lenient<RawPerson>>>,
    #[serde(default, deserialize_with = "lenient")]
    guilds: Option<Vec<Lenient<RawGuildHeads>>>,
}

#[derive(Deserialize)]
struct RawAck {
    #[serde(default, deserialize_with = "loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    message_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    mention_count: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    ack_type: Option<u64>,
}

#[derive(Deserialize)]
struct RawRecipient {
    #[serde(default, deserialize_with = "loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    user: Option<RawPerson>,
}

/// Événements sociaux du READY (relu à part : seuls ces champs sont décodés,
/// le reste est sauté). Envoyé après `Event::Ready`.
pub(super) fn ready(d: &str, out: &Out) {
    if let Some(e) = decode_ready(d) {
        out.send(Event::Relations(e));
    }
}

fn decode_ready(d: &str) -> Option<RelationsEvent> {
    let r: RawSocialReady = serde_json::from_str(d).ok()?;
    // READY de bot : ni relations ni états de lecture, rien à transmettre.
    if r.relationships.is_none() && r.read_state.is_none() {
        return None;
    }
    let mut people: HashMap<u64, Person> = HashMap::new();
    let mut wanted: HashSet<u64> = HashSet::new();
    let relations: Vec<Relation> = items(r.relationships)
        .filter_map(|raw| {
            let rel = raw.relation()?;
            wanted.insert(rel.id);
            Some(rel)
        })
        .collect();
    let mut dms = Vec::new();
    let mut heads = Vec::new();
    for c in items(r.private_channels) {
        let last = c.last_message_id;
        if let Some((info, ps, _)) = c.info() {
            heads.push((info.id, last.unwrap_or(0)));
            wanted.extend(info.recipients.iter().copied());
            for p in ps {
                people.insert(p.id, p);
            }
            dms.push(info);
        }
    }
    // `users` (forme dédoublonnée) : seulement ceux que nomment relations et MP.
    for p in items(r.users).filter_map(|u| u.person()) {
        if wanted.contains(&p.id) {
            people.entry(p.id).or_insert(p);
        }
    }
    for g in items(r.guilds) {
        for h in items(g.channels) {
            if let Some(id) = h.id {
                heads.push((id, h.last_message_id.unwrap_or(0)));
            }
        }
    }
    Some(RelationsEvent::Ready { relations, people: people.into_values().collect(), dms, reads: r.read_state.map(RawReadStates::states), heads })
}

/// MP d'un CHANNEL_CREATE / CHANNEL_UPDATE (sans serveur), ou de la réponse
/// à Create DM : ses membres et son propriétaire.
pub(super) fn private_channel(d: &str) -> Option<RelationsEvent> {
    let (info, people, known_recipients) = serde_json::from_str::<RawPrivate>(d).ok()?.info()?;
    Some(RelationsEvent::Dm { info, people, known_recipients })
}

/// Événements sociaux de la passerelle (après READY).
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    if let Some(e) = decode(kind, d) {
        out.send(Event::Relations(e));
    }
}

fn decode(kind: &str, d: &str) -> Option<RelationsEvent> {
    match kind {
        "RELATIONSHIP_ADD" => Some(RelationsEvent::RelationAdd(serde_json::from_str::<RawRelation>(d).ok()?.relation()?)),
        "RELATIONSHIP_UPDATE" => {
            let r: RawRelation = serde_json::from_str(d).ok()?;
            Some(RelationsEvent::RelationUpdate { id: r.id()?, nickname: r.nickname, ignored: r.user_ignored })
        }
        "RELATIONSHIP_REMOVE" => Some(RelationsEvent::RelationRemove { id: serde_json::from_str::<RawRelation>(d).ok()?.id()? }),
        "MESSAGE_ACK" => {
            let a: RawAck = serde_json::from_str(d).ok()?;
            // Seuls les accusés de salon (type 0 ou absent) sont suivis.
            if a.ack_type.unwrap_or(0) != 0 {
                return None;
            }
            Some(RelationsEvent::Ack { channel_id: a.channel_id?, message_id: a.message_id?, mentions: a.mention_count.and_then(|n| u32::try_from(n).ok()) })
        }
        "CHANNEL_RECIPIENT_ADD" => {
            let r: RawRecipient = serde_json::from_str(d).ok()?;
            Some(RelationsEvent::RecipientAdd { channel_id: r.channel_id?, user: r.user?.person()? })
        }
        "CHANNEL_RECIPIENT_REMOVE" => {
            let r: RawRecipient = serde_json::from_str(d).ok()?;
            Some(RelationsEvent::RecipientRemove { channel_id: r.channel_id?, user_id: r.user?.id? })
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"));
        v["d"].to_string()
    }

    fn ev(kind: &str, name: &str) -> RelationsEvent {
        decode(kind, &fixture(name)).unwrap_or_else(|| panic!("{kind} attendu"))
    }

    const ME: u64 = 1_000_000_000_000_000_001;
    const ALICE: u64 = 1_000_000_000_000_000_002;
    const BOB: u64 = 1_000_000_000_000_000_003;
    const EVE: u64 = 1_000_000_000_000_000_004;
    const CAROL: u64 = 1_000_000_000_000_000_005;
    const DAN: u64 = 1_000_000_000_000_000_006;
    const TROLL: u64 = 1_000_000_000_000_000_007;
    const IGNORED: u64 = 1_000_000_000_000_000_008;
    const DM_GROUP: u64 = 4_000_000_000_000_000_002;

    #[test]
    fn ready_relations_reads_dms_and_heads() {
        let Some(RelationsEvent::Ready { relations, people, dms, reads, heads }) = decode_ready(&fixture("ready_user_social")) else { panic!("READY attendu") };
        let mut kinds: Vec<(u64, RelKind, bool)> = relations.iter().map(|r| (r.id, r.kind, r.ignored)).collect();
        kinds.sort_unstable_by_key(|k| k.0);
        assert_eq!(
            kinds,
            vec![
                (ALICE, RelKind::Friend, false),
                (BOB, RelKind::Friend, false),
                (CAROL, RelKind::Incoming, false),
                (DAN, RelKind::Outgoing, false),
                (TROLL, RelKind::Blocked, false),
                // Seulement ignoré (type 0).
                (IGNORED, RelKind::Other, true),
            ]
        );
        // Relation par `user_id` : la personne vient de `users`.
        assert!(people.iter().any(|p| p.id == DAN && p.name == "Dan"));
        // Personnes de `users` sans rapport : écartées.
        assert!(!people.iter().any(|p| p.id == 1_000_000_000_000_000_099));
        let alice = relations.iter().find(|r| r.id == ALICE).and_then(|r| r.user.clone());
        assert_eq!(alice.map(|p| (p.name, p.username)), Some(("Alice".into(), "alice_fictive".into())));
        assert_eq!(relations.iter().find(|r| r.id == BOB).and_then(|r| r.nickname.clone()), Some("Bobby".into()));
        let group = dms.iter().find(|d| d.id == DM_GROUP).cloned();
        assert_eq!(group, Some(DmInfo { id: DM_GROUP, group: true, owner: Some(BOB), recipients: vec![BOB, EVE], ..Default::default() }));
        let reads = reads.unwrap_or_else(|| panic!("états de lecture attendus"));
        // Le type GUILD_EVENT (1) est écarté ; un nombre accepté pour l'identifiant.
        assert_eq!(reads.len(), 3);
        assert!(reads.contains(&ReadState { channel_id: 3_000_000_000_000_000_011, last_acked: 5_000_000_000_000_000_200, mentions: 1 }));
        assert!(reads.contains(&ReadState { channel_id: DM_GROUP, last_acked: 0, mentions: 0 }));
        assert!(heads.contains(&(3_000_000_000_000_000_011, 5_000_000_000_000_000_203)));
        assert!(heads.contains(&(DM_GROUP, 5_000_000_000_000_000_200)));
        assert!(heads.contains(&(3_000_000_000_000_000_012, 0)));
    }

    #[test]
    fn ready_versioned_read_states_and_bot_ready() {
        let d = r#"{ "read_state": { "version": 3, "partial": false, "entries": [ { "id": "3000000000000000011", "last_message_id": "5", "mention_count": 0 } ] } }"#;
        let Some(RelationsEvent::Ready { reads, .. }) = decode_ready(d) else { panic!() };
        assert_eq!(reads, Some(vec![ReadState { channel_id: 3_000_000_000_000_000_011, last_acked: 5, mentions: 0 }]));
        // Bot : ni relations ni états de lecture, aucun événement.
        assert!(decode_ready(r#"{ "user": { "id": "1" }, "guilds": [] }"#).is_none());
        // Forme inattendue : écartée sans faire échouer le reste.
        let Some(RelationsEvent::Ready { relations, reads, .. }) = decode_ready(r#"{ "relationships": [ { "id": "x" }, { "id": "1000000000000000002", "type": 1 } ], "read_state": "?" }"#) else { panic!() };
        assert_eq!(relations.len(), 1);
        assert!(reads.is_none());
    }

    #[test]
    fn relationship_events() {
        let RelationsEvent::RelationAdd(r) = ev("RELATIONSHIP_ADD", "relationship_add") else { panic!() };
        assert_eq!((r.id, r.kind, r.user.map(|u| u.name)), (CAROL, RelKind::Friend, Some("Carol".into())));
        let RelationsEvent::RelationUpdate { id, nickname, ignored } = ev("RELATIONSHIP_UPDATE", "relationship_update") else { panic!() };
        assert_eq!((id, nickname, ignored), (ALICE, Some(Some("Ali".into())), Some(true)));
        let RelationsEvent::RelationRemove { id } = ev("RELATIONSHIP_REMOVE", "relationship_remove") else { panic!() };
        assert_eq!(id, DAN);
        // Surnom absent : inchangé ; nul : retiré.
        let Some(RelationsEvent::RelationUpdate { nickname, .. }) = decode("RELATIONSHIP_UPDATE", r#"{ "id": "1", "type": 1, "user_ignored": false }"#) else { panic!() };
        assert_eq!(nickname, None);
        let Some(RelationsEvent::RelationUpdate { nickname, .. }) = decode("RELATIONSHIP_UPDATE", r#"{ "id": "1", "type": 1, "nickname": null }"#) else { panic!() };
        assert_eq!(nickname, Some(None));
        assert!(decode("RELATIONSHIP_ADD", r#"{ "id": "1" }"#).is_none(), "sans type : ignoré");
    }

    /// Demande de message (lot U) : drapeaux lus, absents sur un MP ordinaire.
    #[test]
    fn message_request_flags() {
        let Some(RelationsEvent::Dm { info, people, .. }) = private_channel(&fixture("channel_create_message_request")) else { panic!() };
        assert!(info.request && info.spam && !info.group);
        assert_eq!(people.first().map(|p| p.name.as_str()), Some("Inconnu"));
        let Some(RelationsEvent::Dm { info, .. }) = private_channel(r#"{ "id": "9", "type": 1, "recipients": [] }"#) else { panic!() };
        assert!(!info.request && !info.spam);
    }

    #[test]
    fn ack_presences_and_recipients() {
        let RelationsEvent::Ack { channel_id, message_id, mentions } = ev("MESSAGE_ACK", "message_ack") else { panic!() };
        assert_eq!((channel_id, message_id, mentions), (3_000_000_000_000_000_011, 5_000_000_000_000_000_203, Some(0)));
        assert!(decode("MESSAGE_ACK", r#"{ "channel_id": "1", "message_id": "2", "ack_type": 3, "version": 1 }"#).is_none());
        // Présences des amis : dans le magasin des présences (`net::people`), plus ici.
        assert!(decode("READY_SUPPLEMENTAL", &fixture("ready_supplemental")).is_none());
        assert!(decode("PRESENCE_UPDATE", &fixture("presence_update_friend")).is_none());
        let RelationsEvent::RecipientAdd { channel_id, user } = ev("CHANNEL_RECIPIENT_ADD", "channel_recipient_add") else { panic!() };
        assert_eq!((channel_id, user.id, user.name.as_str()), (DM_GROUP, CAROL, "Carol"));
        let RelationsEvent::RecipientRemove { channel_id, user_id } = ev("CHANNEL_RECIPIENT_REMOVE", "channel_recipient_remove") else { panic!() };
        assert_eq!((channel_id, user_id), (DM_GROUP, EVE));
    }

    #[test]
    fn private_channel_updates() {
        let Some(RelationsEvent::Dm { info, known_recipients, .. }) = private_channel(&fixture("channel_create_dm")) else { panic!() };
        assert!(!info.group && known_recipients);
        let Some(RelationsEvent::Dm { known_recipients, .. }) = private_channel(r#"{ "id": "4000000000000000002", "type": 3, "name": "Nouveau nom" }"#) else { panic!() };
        assert!(!known_recipients, "CHANNEL_UPDATE partiel");
        assert!(private_channel(r#"{ "id": "1", "type": 0, "guild_id": "2" }"#).is_none());
        // Groupe créé avec moi en destinataire : la réponse ne liste que les autres.
        let Some(RelationsEvent::Dm { info, .. }) = private_channel(&format!(r#"{{ "id": "4000000000000000009", "type": 3, "owner_id": "{ME}", "recipients": [ {{ "id": "{ALICE}", "username": "alice_fictive" }} ] }}"#)) else { panic!() };
        assert_eq!((info.owner, info.recipients), (Some(ME), vec![ALICE]));
    }
}
