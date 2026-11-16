//! Liste des membres d'un compte utilisateur : souscription (op 37) et
//! décodage de GUILD_MEMBER_LIST_UPDATE. Rien de tout cela n'est documenté
//! par Discord ; les formes viennent de la documentation communautaire, voir
//! `docs/passerelle-utilisateur.md` (sources, rythme, ce qui est envoyé).
//!
//! L'op 14 (une souscription par serveur) est marquée « dépréciée » par cette
//! documentation : seule l'op 37 (souscriptions groupées) est envoyée.

use super::raw::RawUser;
use crate::state::people::{PeopleHub, Presence, UserInfo};
use crate::state::members::{pick_activity, Activity, ActivityKind, GroupId, ListItem, ListMember, ListOp, ListUpdate, MemberStatus, Subscription};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::{json, value::RawValue, Map, Value};
use std::{collections::HashSet, time::{Duration, Instant}};

/// Attente après une demande, pour regrouper les changements d'un défilement.
pub(super) const DEBOUNCE: Duration = Duration::from_millis(250);
/// Écart minimal entre deux souscriptions : 40 par minute au plus, trois
/// fois moins que ce que le limiteur de la passerelle laisse passer (120).
pub(super) const MIN_GAP: Duration = Duration::from_millis(1500);

// ====================================================================== envoi

/// Rythme et contenu des souscriptions : la dernière demande de l'interface
/// part au plus tôt `DEBOUNCE` après elle, et `MIN_GAP` après la précédente.
/// Ce qui a été envoyé est retenu pour n'envoyer que la différence.
#[derive(Debug, Default)]
pub(super) struct Pacer {
    desired: Option<Subscription>,
    sent: Option<Subscription>,
    due: Option<Instant>,
    last: Option<Instant>,
    /// Serveurs auxquels cette session est déjà abonnée (`typing`, `activities`).
    guilds: HashSet<u64>,
}

impl Pacer {
    pub fn want(&mut self, s: Subscription, now: Instant) {
        if self.desired.as_ref() == Some(&s) {
            return;
        }
        self.desired = Some(s);
        self.schedule(now);
    }

    fn schedule(&mut self, now: Instant) {
        if self.desired == self.sent {
            self.due = None;
            return;
        }
        // Une échéance déjà fixée ne recule pas : un défilement continu
        // envoie un paquet tous les `MIN_GAP`, pas seulement à l'arrêt.
        if self.due.is_none() {
            let earliest = self.last.map_or(now, |l| l + MIN_GAP);
            self.due = Some(earliest.max(now + DEBOUNCE));
        }
    }

    /// Nouvelle session (Identify) : le serveur a tout oublié.
    pub fn reset(&mut self, now: Instant) {
        self.sent = None;
        self.guilds.clear();
        self.schedule(now);
    }

    pub fn due(&self) -> Option<Instant> {
        self.due
    }

    /// Paquet à envoyer maintenant, s'il y en a un.
    pub fn payload(&self, now: Instant) -> Option<String> {
        if self.due.is_none_or(|d| d > now) {
            return None;
        }
        let want = self.desired.as_ref()?;
        let mut subs = Map::new();
        // Liste d'un autre serveur encore souscrite : quittée dans le même paquet.
        if let Some(prev) = &self.sent
            && prev.guild_id != want.guild_id
            && prev.channel.is_some()
        {
            subs.insert(prev.guild_id.to_string(), json!({ "channels": {} }));
        }
        let mut entry = Map::new();
        if !self.guilds.contains(&want.guild_id) {
            entry.insert("typing".into(), Value::Bool(true));
            entry.insert("activities".into(), Value::Bool(true));
        }
        let channels: Map<String, Value> = match &want.channel {
            Some((c, ranges)) => [(c.to_string(), json!(ranges.iter().map(|(a, b)| [a, b]).collect::<Vec<_>>()))].into_iter().collect(),
            None => Map::new(),
        };
        entry.insert("channels".into(), Value::Object(channels));
        subs.insert(want.guild_id.to_string(), Value::Object(entry));
        Some(json!({ "op": 37, "d": { "subscriptions": subs } }).to_string())
    }

    /// Le paquet de `payload` est parti.
    pub fn sent(&mut self, now: Instant) {
        if let Some(w) = &self.desired {
            self.guilds.insert(w.guild_id);
        }
        self.sent = self.desired.clone();
        self.last = Some(now);
        self.due = None;
    }

    /// Le paquet n'a pas pu être mis en file : nouvel essai plus tard.
    pub fn later(&mut self, now: Instant) {
        self.due = Some(now + MIN_GAP);
    }
}

// ====================================================================== réception

/// Élément lu à part : de forme inattendue, il vaut `None` au lieu de faire
/// échouer tout le paquet.
struct Lenient<T>(Option<T>);

impl<'de, T: DeserializeOwned> Deserialize<'de> for Lenient<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        super::raw::with_raw(d, |raw| Lenient(serde_json::from_str(raw).ok()))
    }
}

fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    Ok(Lenient::<T>::deserialize(d)?.0)
}

/// Identifiant en chaîne ou en nombre.
fn id_of(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

#[derive(Deserialize)]
struct RawListUpdate {
    #[serde(default)]
    guild_id: Value,
    #[serde(default)]
    id: Value,
    #[serde(default, deserialize_with = "lenient")]
    member_count: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    online_count: Option<u32>,
    #[serde(default, deserialize_with = "lenient")]
    groups: Option<Vec<Lenient<RawGroup>>>,
    #[serde(default, deserialize_with = "lenient")]
    ops: Option<Vec<Lenient<RawOp>>>,
}

#[derive(Deserialize)]
struct RawGroup {
    id: Value,
    #[serde(default)]
    count: u32,
}

#[derive(Deserialize)]
struct RawOp {
    op: String,
    #[serde(default)]
    range: Option<(u32, u32)>,
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    items: Option<Vec<Lenient<RawItem>>>,
    #[serde(default)]
    item: Option<Lenient<RawItem>>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(default, deserialize_with = "lenient")]
    group: Option<RawGroupRef>,
    #[serde(default, deserialize_with = "lenient")]
    member: Option<RawListMember>,
}

#[derive(Deserialize)]
struct RawGroupRef {
    id: Value,
}

#[derive(Deserialize)]
struct RawListMember {
    #[serde(default, deserialize_with = "lenient")]
    user: Option<RawUser>,
    #[serde(default, deserialize_with = "lenient")]
    roles: Option<Vec<String>>,
    #[serde(default, deserialize_with = "lenient")]
    nick: Option<String>,
    /// Gardé brut : lu ici pour la liste, et par `net::people` pour le
    /// magasin des présences (même objet que PRESENCE_UPDATE).
    #[serde(default)]
    presence: Option<Box<RawValue>>,
}

/// Ce que la liste apprend des utilisateurs et de leurs présences, versé
/// dans `state::people` après le décodage.
type Seen = Vec<(u64, Option<UserInfo>, Option<Presence>)>;

#[derive(Deserialize)]
struct RawPresence {
    #[serde(default, deserialize_with = "lenient")]
    status: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    activities: Option<Vec<Lenient<RawActivity>>>,
}

#[derive(Deserialize)]
struct RawActivity {
    #[serde(rename = "type")]
    kind: u8,
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    state: Option<String>,
}

fn group_id(v: &Value) -> Option<GroupId> {
    match v.as_str() {
        Some("online") => Some(GroupId::Online),
        Some("offline") => Some(GroupId::Offline),
        _ => id_of(v).map(GroupId::Role),
    }
}

impl RawListMember {
    fn into_member(self, seen: &mut Seen) -> Option<ListMember> {
        let user = self.user?;
        let raw = self.presence.as_deref().map(RawValue::get);
        seen.push((user.id, super::people::user_info(&user), raw.and_then(|r| super::people::list_presence(r, user.id))));
        let name = match self.nick.filter(|n| !n.is_empty()) {
            Some(n) => n,
            None => user.display(),
        };
        let presence = raw.and_then(|r| serde_json::from_str::<RawPresence>(r).ok());
        let status = presence.as_ref().and_then(|p| p.status.as_deref()).map_or(MemberStatus::Offline, MemberStatus::parse);
        let activities = presence
            .and_then(|p| p.activities)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|a| a.0)
            .filter_map(|a| Some(Activity { kind: ActivityKind::from_code(a.kind)?, name: a.name.unwrap_or_default(), state: a.state }))
            .collect();
        Some(ListMember {
            user_id: user.id,
            name,
            avatar: user.avatar(),
            roles: self.roles.unwrap_or_default().iter().filter_map(|r| r.parse().ok()).collect(),
            status,
            activity: pick_activity(activities),
            bot: user.bot.unwrap_or(false),
            color: 0,
            line: None,
        })
    }
}

impl RawItem {
    fn into_item(self, seen: &mut Seen) -> Option<ListItem> {
        if let Some(g) = self.group {
            return group_id(&g.id).map(ListItem::Group);
        }
        self.member?.into_member(seen).map(ListItem::Member)
    }
}

impl RawOp {
    fn into_op(self, seen: &mut Seen) -> Option<ListOp> {
        // Un élément illisible garde sa place (ligne en attente) : seul un
        // index ou un type d'opération illisible rend la liste inconnue.
        let mut item = |i: Option<Lenient<RawItem>>| -> ListItem { i.and_then(|x| x.0).and_then(|x| x.into_item(seen)).unwrap_or(ListItem::Unknown) };
        Some(match self.op.as_str() {
            "SYNC" => ListOp::Sync { range: self.range?, items: self.items.unwrap_or_default().into_iter().map(|x| item(Some(x))).collect() },
            "INSERT" => ListOp::Insert { index: self.index?, item: item(self.item) },
            "UPDATE" => ListOp::Update { index: self.index?, item: item(self.item) },
            "DELETE" => ListOp::Delete { index: self.index? },
            "INVALIDATE" => ListOp::Invalidate { range: self.range? },
            _ => return None,
        })
    }
}

/// `None` si le paquet n'a ni serveur ni liste lisibles.
#[cfg(test)]
pub(super) fn decode(d: &str) -> Option<ListUpdate> {
    decode_seen(d).map(|(u, _)| u)
}

/// Comme `decode` ; les utilisateurs et présences des membres reçus vont
/// aussi dans les tables partagées (carte de profil, pastilles). Pas de
/// réveil ici : la mise à jour de la liste s'en charge si elle est affichée.
pub(super) fn decode_into(d: &str, hub: &PeopleHub) -> Option<ListUpdate> {
    let (update, seen) = decode_seen(d)?;
    let mut p = hub.lock();
    for (id, user, presence) in seen {
        if let Some(u) = user {
            p.upsert_user(u);
        }
        if let Some(pr) = presence {
            p.set_presence(id, pr);
        }
    }
    // Rôles des membres listés (lot P) : couleur de leur nom dans la discussion.
    for op in &update.ops {
        let items: &[ListItem] = match op {
            ListOp::Sync { items, .. } => items,
            ListOp::Insert { item, .. } | ListOp::Update { item, .. } => std::slice::from_ref(item),
            _ => &[],
        };
        for m in items.iter().filter_map(|i| if let ListItem::Member(m) = i { Some(m) } else { None }) {
            p.note_roles(update.guild_id, m.user_id, &m.roles);
        }
    }
    Some(update)
}

fn decode_seen(d: &str) -> Option<(ListUpdate, Seen)> {
    let raw: RawListUpdate = serde_json::from_str(d).ok()?;
    let guild_id = id_of(&raw.guild_id)?;
    let id = match &raw.id {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        _ => return None,
    };
    let groups = raw.groups.unwrap_or_default().into_iter().filter_map(|g| g.0).filter_map(|g| Some((group_id(&g.id)?, g.count))).collect();
    let mut seen = Seen::new();
    let ops = raw.ops.unwrap_or_default().into_iter().map(|o| o.0.and_then(|o| o.into_op(&mut seen)).unwrap_or(ListOp::Unreadable)).collect();
    Some((ListUpdate { guild_id, id, member_count: raw.member_count.unwrap_or(0), online_count: raw.online_count.unwrap_or(0), groups, ops }, seen))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))).unwrap_or_else(|e| panic!("{e}"));
        v["d"].to_string()
    }

    fn sub(guild_id: u64, channel: Option<(u64, Vec<(u32, u32)>)>) -> Subscription {
        Subscription { guild_id, channel }
    }

    /// Les présences et utilisateurs de la liste vont aussi dans le magasin
    /// des présences (pastilles, carte de profil).
    #[test]
    fn list_presences_feed_the_people_store() {
        let hub = PeopleHub::default();
        let u = decode_into(&fixture("guild_member_list_update_sync"), &hub).unwrap_or_else(|| panic!("paquet illisible"));
        assert_eq!(u.id, "everyone");
        let mut p = hub.lock();
        assert_eq!(p.status(1000000000000000002), Some(crate::state::people::Status::Dnd));
        assert_eq!(p.presence(1000000000000000002).and_then(|x| x.custom()).and_then(|a| a.state.clone()).as_deref(), Some("En réunion"));
        assert_eq!(p.user(1000000000000000002).map(|u| u.display_name().to_string()).as_deref(), Some("Alice"));
        assert!(decode_into("{ illisible", &hub).is_none());
    }

    #[test]
    fn sync_fixture_decodes() {
        let u = decode(&fixture("guild_member_list_update_sync")).unwrap_or_else(|| panic!("paquet illisible"));
        assert_eq!(u.guild_id, 2000000000000000001);
        assert_eq!(u.id, "everyone");
        assert_eq!((u.member_count, u.online_count), (6, 4));
        assert_eq!(u.groups, vec![(GroupId::Role(2000000000000001010), 1), (GroupId::Online, 3), (GroupId::Offline, 2)]);
        let [ListOp::Sync { range, items }] = u.ops.as_slice() else { panic!("{:?}", u.ops) };
        assert_eq!(*range, (0, 99));
        assert_eq!(items.len(), 9);
        let ListItem::Member(alice) = &items[1] else { panic!("{:?}", items[1]) };
        // Pseudo du serveur d'abord ; présence et activité lues.
        assert_eq!(alice.name, "Alice (modération)");
        assert_eq!(alice.status, MemberStatus::Dnd);
        assert_eq!(alice.roles, vec![2000000000000001010]);
        assert_eq!(alice.activity.as_ref().map(|a| (a.kind, a.state.as_deref())), Some((ActivityKind::Custom, Some("En réunion"))));
        let ListItem::Member(bot) = &items[4] else { panic!("{:?}", items[4]) };
        assert!(bot.bot);
        assert_eq!(bot.activity.as_ref().map(|a| (a.kind, a.name.as_str())), Some((ActivityKind::Playing, "Échecs")));
        // Sans présence : hors ligne.
        let ListItem::Member(off) = &items[8] else { panic!("{:?}", items[8]) };
        assert_eq!(off.status, MemberStatus::Offline);
    }

    #[test]
    fn update_ops_fixture_decodes() {
        let u = decode(&fixture("guild_member_list_update_ops")).unwrap_or_else(|| panic!("paquet illisible"));
        let kinds: Vec<&str> = u
            .ops
            .iter()
            .map(|o| match o {
                ListOp::Sync { .. } => "SYNC",
                ListOp::Insert { .. } => "INSERT",
                ListOp::Update { .. } => "UPDATE",
                ListOp::Delete { .. } => "DELETE",
                ListOp::Invalidate { .. } => "INVALIDATE",
                ListOp::Unreadable => "?",
            })
            .collect();
        assert_eq!(kinds, ["DELETE", "INSERT", "UPDATE", "INVALIDATE", "?"]);
        assert!(matches!(&u.ops[1], ListOp::Insert { index: 5, item: ListItem::Member(m) } if m.status == MemberStatus::Idle));
    }

    #[test]
    fn odd_shapes_never_fail() {
        assert!(decode("{}").is_none());
        assert!(decode(r#"{"guild_id":"1"}"#).is_none(), "sans identifiant de liste");
        let u = decode(r#"{"guild_id":"1","id":"everyone","ops":"x","groups":[{"id":"online","count":"x"},{"id":"offline","count":2}],"extra":1}"#);
        let u = u.unwrap_or_else(|| panic!("toléré"));
        assert!(u.ops.is_empty());
        assert_eq!(u.groups, vec![(GroupId::Offline, 2)]);
        // Membre illisible dans une SYNC : sa ligne reste en attente, rien n'est décalé.
        let u = decode(r#"{"guild_id":"1","id":"e","ops":[{"op":"SYNC","range":[0,99],"items":[{"member":{"user":5}},{"group":{"id":"online"}}]}]}"#);
        assert_eq!(u.map(|u| u.ops), Some(vec![ListOp::Sync { range: (0, 99), items: vec![ListItem::Unknown, ListItem::Group(GroupId::Online)] }]));
    }

    #[test]
    fn first_payload_subscribes_to_the_guild_and_its_channel() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        p.want(sub(1, Some((9, vec![(0, 99)]))), t0);
        assert_eq!(p.due(), Some(t0 + DEBOUNCE));
        assert!(p.payload(t0).is_none(), "pas avant le délai");
        let text = p.payload(t0 + DEBOUNCE).unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or_default();
        assert_eq!(v, json!({ "op": 37, "d": { "subscriptions": { "1": { "typing": true, "activities": true, "channels": { "9": [[0, 99]] } } } } }));
        p.sent(t0 + DEBOUNCE);
        assert_eq!(p.due(), None);
        // Même demande : rien.
        p.want(sub(1, Some((9, vec![(0, 99)]))), t0 + DEBOUNCE);
        assert_eq!(p.due(), None);
    }

    #[test]
    fn later_payloads_send_only_the_difference_at_a_human_pace() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        p.want(sub(1, Some((9, vec![(0, 99)]))), t0);
        p.sent(t0);
        // Défilement : trois demandes rapprochées, un seul paquet, pas avant MIN_GAP.
        for (i, r) in [(100, 199), (200, 299), (300, 399)].into_iter().enumerate() {
            p.want(sub(1, Some((9, vec![(0, 99), r]))), t0 + Duration::from_millis(10 * i as u64));
        }
        assert_eq!(p.due(), Some(t0 + MIN_GAP));
        let v: Value = serde_json::from_str(&p.payload(t0 + MIN_GAP).unwrap_or_default()).unwrap_or_default();
        assert_eq!(v["d"]["subscriptions"], json!({ "1": { "channels": { "9": [[0, 99], [300, 399]] } } }));
        p.sent(t0 + MIN_GAP);
        // Autre serveur : l'ancien est quitté dans le même paquet.
        p.want(sub(2, Some((7, vec![(0, 99)]))), t0 + MIN_GAP * 5);
        let at = p.due().unwrap_or(t0);
        let v: Value = serde_json::from_str(&p.payload(at).unwrap_or_default()).unwrap_or_default();
        assert_eq!(
            v["d"]["subscriptions"],
            json!({ "1": { "channels": {} }, "2": { "typing": true, "activities": true, "channels": { "7": [[0, 99]] } } })
        );
        p.sent(at);
        // Liste masquée : seulement les salons du serveur, vidés.
        p.want(sub(2, None), at + MIN_GAP * 2);
        let at = p.due().unwrap_or(t0);
        let v: Value = serde_json::from_str(&p.payload(at).unwrap_or_default()).unwrap_or_default();
        assert_eq!(v["d"]["subscriptions"], json!({ "2": { "channels": {} } }));
        // Nouvelle session : tout est renvoyé.
        p.sent(at);
        p.reset(at + MIN_GAP * 2);
        let v: Value = serde_json::from_str(&p.payload(p.due().unwrap_or(t0)).unwrap_or_default()).unwrap_or_default();
        assert_eq!(v["d"]["subscriptions"], json!({ "2": { "typing": true, "activities": true, "channels": {} } }));
    }

    #[test]
    fn a_burst_of_requests_stays_far_below_the_gateway_limit() {
        let t0 = Instant::now();
        let mut p = Pacer::default();
        let mut sent = 0;
        // Une minute de demandes toutes les 10 ms (défilement frénétique).
        for ms in (0..60_000u64).step_by(10) {
            let now = t0 + Duration::from_millis(ms);
            p.want(sub(1, Some((9, vec![(0, 99), (((ms / 10) % 50) as u32 * 100 + 100, ((ms / 10) % 50) as u32 * 100 + 199)]))), now);
            if p.payload(now).is_some() {
                p.sent(now);
                sent += 1;
            }
        }
        assert!(sent <= 41, "{sent} souscriptions en une minute");
    }
}
