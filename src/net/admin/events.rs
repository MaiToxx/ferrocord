//! Événements programmés (lot A7).
//!
//! Formes vérifiées le 13 novembre 2026 :
//! - documentées par Discord (Guild Scheduled Event, et docs.discord.food
//!   « Guild Scheduled Event ») : `GET /guilds/{id}/scheduled-events?with_user_count=true`,
//!   `POST /guilds/{id}/scheduled-events`, `PATCH` et `DELETE
//!   /guilds/{id}/scheduled-events/{event}` ; objet Guild Scheduled Event
//!   (`entity_type` 1 conférence, 2 vocal, 3 externe ; `status` 1 à 4 ;
//!   `privacy_level` 2 ; `recurrence_rule`) ; passerelle
//!   `GUILD_SCHEDULED_EVENT_CREATE/UPDATE/DELETE` (l'objet) et
//!   `GUILD_SCHEDULED_EVENT_USER_ADD/REMOVE` (`guild_scheduled_event_id`,
//!   `user_id`, `guild_id`) ;
//! - réservées aux comptes utilisateur, forme lue sur docs.discord.food
//!   (« Guild Scheduled Event ») : `PUT` et `DELETE
//!   /guilds/{id}/scheduled-events/{event}/users/@me` (« Ça m'intéresse »,
//!   réponse : l'inscription, ou 204) et `GET
//!   /users/@me/scheduled-events?guild_ids={id}` (mes inscriptions).
//!
//! Tout part d'un geste (ouvrir la liste, cliquer) et passe par le limiteur commun.

use crate::net::raw::{de_lenient, de_loose_id, Item};
use crate::net::ratelimit::Route;
use crate::net::rest::{api, ApiError, Ctx};
use crate::net::Out;
use crate::state::admin::events::{EventAction, EventCmd, EventDraft, EventEvent, Failure, Frequency, Place, Recurrence, ScheduledEvent, Status};
use crate::state::model::ImageHash;
use crate::state::Event;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(15);

#[derive(Deserialize, Default)]
struct RawMeta {
    #[serde(default, deserialize_with = "de_lenient")]
    location: Option<String>,
}

#[derive(Deserialize)]
struct RawNth {
    #[serde(default, deserialize_with = "de_lenient")]
    n: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    day: Option<u8>,
}

#[derive(Deserialize)]
struct RawRule {
    #[serde(default, deserialize_with = "de_lenient")]
    frequency: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    interval: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    by_weekday: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "de_lenient")]
    by_n_weekday: Option<Vec<RawNth>>,
    #[serde(default, deserialize_with = "de_lenient")]
    by_month: Option<Vec<u8>>,
    #[serde(default, deserialize_with = "de_lenient")]
    by_month_day: Option<Vec<u8>>,
}

impl RawRule {
    fn into_rule(self) -> Option<Recurrence> {
        let by_n_weekday = self.by_n_weekday.unwrap_or_default().into_iter().find_map(|x| Some((x.n?, x.day?)));
        let month = self.by_month.and_then(|m| m.first().copied());
        let day = self.by_month_day.and_then(|d| d.first().copied());
        Some(Recurrence {
            frequency: Frequency::from_api(self.frequency?)?,
            interval: self.interval.unwrap_or(1).max(1),
            by_weekday: self.by_weekday.unwrap_or_default(),
            by_n_weekday,
            by_month_day: month.zip(day),
        })
    }
}

/// Objet Guild Scheduled Event. Les champs inconnus sont ignorés ; un
/// événement sans identifiant, serveur, début ou statut lisibles est écarté.
#[derive(Deserialize)]
pub(in crate::net) struct RawEvent {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    creator_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    scheduled_start_time: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    scheduled_end_time: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    status: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    entity_type: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    entity_metadata: Option<RawMeta>,
    #[serde(default, deserialize_with = "de_lenient")]
    user_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    image: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    recurrence_rule: Option<RawRule>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_scheduled_event_exceptions: Option<Vec<Item<RawException>>>,
}

/// Exception d'une occurrence (docs.discord.food, « Guild Scheduled Event
/// Exception Object ») : `event_exception_id` est le snowflake du début d'origine.
#[derive(Deserialize)]
struct RawException {
    #[serde(default, deserialize_with = "de_loose_id")]
    event_exception_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    is_canceled: Option<bool>,
}

/// Début d'origine (ms Unix) d'une exception, et l'inverse.
fn exception_ms(id: u64) -> i64 {
    i64::try_from((id >> 22).saturating_add(crate::state::model::DISCORD_EPOCH)).unwrap_or(i64::MAX)
}

fn exception_id(ms: i64) -> u64 {
    u64::try_from(ms).unwrap_or(0).saturating_sub(crate::state::model::DISCORD_EPOCH) << 22
}

fn time_ms(s: Option<&str>) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s?).ok().map(|d| d.timestamp_millis())
}

fn iso(ms: i64) -> Value {
    match chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms) {
        Some(d) => d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true).into(),
        None => Value::Null,
    }
}

impl RawEvent {
    /// `guild` : serveur du paquet englobant (GUILD_CREATE), si l'objet ne le dit pas.
    pub(in crate::net) fn into_event(self, guild: Option<u64>) -> Option<ScheduledEvent> {
        let place = match (self.entity_type?, self.channel_id) {
            (1, Some(c)) => Place::Stage(c),
            (2, Some(c)) => Place::Voice(c),
            (3, _) => Place::External(self.entity_metadata.and_then(|m| m.location).unwrap_or_default()),
            _ => Place::Other,
        };
        Some(ScheduledEvent {
            id: self.id?,
            guild_id: self.guild_id.or(guild)?,
            name: self.name.unwrap_or_default(),
            description: self.description.unwrap_or_default(),
            start_ms: time_ms(self.scheduled_start_time.as_deref())?,
            end_ms: time_ms(self.scheduled_end_time.as_deref()),
            status: Status::from_api(self.status?)?,
            place,
            creator_id: self.creator_id,
            user_count: self.user_count,
            image: self.image.as_deref().and_then(ImageHash::parse),
            recurrence: self.recurrence_rule.and_then(RawRule::into_rule),
            skipped: self
                .guild_scheduled_event_exceptions
                .into_iter()
                .flatten()
                .filter_map(|x| x.0)
                .filter(|x| x.is_canceled == Some(true))
                .filter_map(|x| x.event_exception_id.map(exception_ms))
                .collect(),
        })
    }
}

/// Liste d'événements : chacun lu à part (un illisible n'empêche pas les autres).
pub(in crate::net) fn list(items: Vec<Item<RawEvent>>, guild: Option<u64>) -> Vec<ScheduledEvent> {
    items.into_iter().filter_map(|i| i.0?.into_event(guild)).collect()
}

/// Décode une réponse de la liste (aperçu hors ligne, tests).
pub fn decode_list(body: &str) -> Vec<ScheduledEvent> {
    serde_json::from_str::<Vec<Item<RawEvent>>>(body).map(|l| list(l, None)).unwrap_or_default()
}

#[derive(Deserialize)]
struct RawSub {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_scheduled_event_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_scheduled_event_exception_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    response: Option<u64>,
}

/// Mes inscriptions : celles à l'événement entier (pas à une seule occurrence), intéressé.
fn subscriptions(body: &str) -> Option<Vec<u64>> {
    let l = serde_json::from_str::<Vec<Item<RawSub>>>(body).ok()?;
    Some(l.into_iter().filter_map(|i| i.0).filter(|s| s.guild_scheduled_event_exception_id.is_none() && s.response.unwrap_or(1) == 1).filter_map(|s| s.guild_scheduled_event_id).collect())
}

#[derive(Deserialize)]
struct RawUserEvent {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_scheduled_event_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_scheduled_event_exception_id: Option<u64>,
}

/// Paquets `GUILD_SCHEDULED_EVENT_*` de la passerelle.
pub(in crate::net) fn decode(kind: &str, d: &str) -> Option<EventEvent> {
    match kind {
        "GUILD_SCHEDULED_EVENT_CREATE" | "GUILD_SCHEDULED_EVENT_UPDATE" => serde_json::from_str::<RawEvent>(d).ok()?.into_event(None).map(EventEvent::Upsert),
        "GUILD_SCHEDULED_EVENT_DELETE" => {
            let e = serde_json::from_str::<RawEvent>(d).ok()?;
            Some(EventEvent::Deleted { guild_id: e.guild_id?, event_id: e.id? })
        }
        "GUILD_SCHEDULED_EVENT_USER_ADD" | "GUILD_SCHEDULED_EVENT_USER_REMOVE" => {
            let u = serde_json::from_str::<RawUserEvent>(d).ok()?;
            // Une occurrence seule ne change pas l'intérêt pour l'événement entier.
            if u.guild_scheduled_event_exception_id.is_some() {
                return None;
            }
            let (guild_id, event_id, user_id) = (u.guild_id?, u.guild_scheduled_event_id?, u.user_id?);
            Some(if kind.ends_with("ADD") { EventEvent::UserAdd { guild_id, event_id, user_id } } else { EventEvent::UserRemove { guild_id, event_id, user_id } })
        }
        _ => None,
    }
}

pub(in crate::net) fn dispatch(kind: &str, d: &str, out: &Out) {
    if kind.starts_with("GUILD_SCHEDULED_EVENT_")
        && let Some(e) = decode(kind, d)
    {
        out.send(Event::GuildEvents(e));
    }
}

fn rule_body(r: &Recurrence, start_ms: i64) -> Value {
    let opt = |v: Option<Value>| v.unwrap_or(Value::Null);
    json!({
        "start": iso(start_ms),
        "frequency": r.frequency.api(),
        "interval": r.interval,
        "by_weekday": if r.by_weekday.is_empty() { Value::Null } else { json!(r.by_weekday) },
        "by_n_weekday": opt(r.by_n_weekday.map(|(n, day)| json!([{ "n": n, "day": day }]))),
        "by_month": opt(r.by_month_day.map(|(m, _)| json!([m]))),
        "by_month_day": opt(r.by_month_day.map(|(_, d)| json!([d]))),
    })
}

/// Corps d'une création (`create`) ou d'une modification : une modification
/// envoie aussi les valeurs retirées (`null`), une création les omet.
pub(in crate::net) fn draft_body(d: &EventDraft, create: bool) -> Value {
    let mut b = json!({
        "name": d.name,
        "scheduled_start_time": iso(d.start_ms),
        "description": if d.description.is_empty() { Value::Null } else { d.description.clone().into() },
    });
    let (kind, channel, meta) = match &d.place {
        Place::Stage(c) => (1, Value::from(c.to_string()), Value::Null),
        Place::Voice(c) => (2, Value::from(c.to_string()), Value::Null),
        Place::External(l) => (3, Value::Null, json!({ "location": l })),
        // Écarté par `events::check` avant tout envoi.
        Place::Other => (3, Value::Null, json!({ "location": "" })),
    };
    b["entity_type"] = kind.into();
    b["channel_id"] = channel;
    b["entity_metadata"] = meta;
    if create {
        // Réservé aux membres du serveur : la seule valeur permise.
        b["privacy_level"] = 2.into();
    }
    match (d.end_ms, create) {
        (Some(e), _) => b["scheduled_end_time"] = iso(e),
        (None, false) => b["scheduled_end_time"] = Value::Null,
        (None, true) => {}
    }
    match (&d.recurrence, create) {
        (Some(r), _) => b["recurrence_rule"] = rule_body(r, d.start_ms),
        (None, false) => b["recurrence_rule"] = Value::Null,
        (None, true) => {}
    }
    // Image de couverture (« image data », Create/Modify Guild Scheduled Event) : seulement si changée.
    if let Some(img) = &d.image {
        b["image"] = img.as_str().into();
    }
    b
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn request(cmd: &EventCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        EventCmd::Load { guild_id } => r("GET /guilds/{}/scheduled-events", *guild_id, Method::GET, format!("/guilds/{guild_id}/scheduled-events?with_user_count=true"), None),
        EventCmd::Create { guild_id, draft } => r("POST /guilds/{}/scheduled-events", *guild_id, Method::POST, format!("/guilds/{guild_id}/scheduled-events"), Some(draft_body(draft, true))),
        EventCmd::Edit { guild_id, event_id, draft } => {
            r("PATCH /guilds/{}/scheduled-events/{}", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/scheduled-events/{event_id}"), Some(draft_body(draft, false)))
        }
        EventCmd::SetStatus { guild_id, event_id, status } => {
            r("PATCH /guilds/{}/scheduled-events/{}", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/scheduled-events/{event_id}"), Some(json!({ "status": status.api() })))
        }
        EventCmd::Delete { guild_id, event_id } => r("DELETE /guilds/{}/scheduled-events/{}", *guild_id, Method::DELETE, format!("/guilds/{guild_id}/scheduled-events/{event_id}"), None),
        EventCmd::Occurrence { guild_id, event_id, original_ms, skip: true } => r(
            "POST /guilds/{}/scheduled-events/{}/exceptions",
            *guild_id,
            Method::POST,
            format!("/guilds/{guild_id}/scheduled-events/{event_id}/exceptions"),
            Some(json!({ "original_scheduled_start_time": iso(*original_ms), "is_canceled": true })),
        ),
        EventCmd::Occurrence { guild_id, event_id, original_ms, skip: false } => r(
            "DELETE /guilds/{}/scheduled-events/{}/exceptions/{}",
            *guild_id,
            Method::DELETE,
            format!("/guilds/{guild_id}/scheduled-events/{event_id}/exceptions/{}", exception_id(*original_ms)),
            None,
        ),
        EventCmd::Interest { guild_id, event_id, on } => {
            let method = if *on { Method::PUT } else { Method::DELETE };
            r("PUT /guilds/{}/scheduled-events/{}/users/@me", *guild_id, method, format!("/guilds/{guild_id}/scheduled-events/{event_id}/users/@me"), None)
        }
    }
}

/// Échec d'une écriture d'administration : ressource inconnue, ou le problème
/// (A2F exigée : `Problem::MfaRequired`, chemin unique) ; un jeton mort renvoie à l'écran de connexion.
pub(in crate::net) fn failed(e: ApiError, make: impl FnOnce(Failure) -> Event) -> Event {
    match e {
        ApiError::NotFound(_) => make(Failure::NotFound),
        e => e.into_event(|p| make(Failure::Problem(p))),
    }
}

/// Réponse reçue : les événements à publier (`None` : réponse sans la forme attendue).
fn answer(cmd: &EventCmd, body: &str) -> Option<Vec<EventEvent>> {
    let (action, key) = cmd.action();
    let done = EventEvent::Done { action, key };
    Some(match cmd {
        EventCmd::Load { .. } => return None,
        EventCmd::Create { .. } | EventCmd::Edit { .. } | EventCmd::SetStatus { .. } => {
            let e = serde_json::from_str::<RawEvent>(body).ok()?.into_event(None)?;
            vec![EventEvent::Upsert(e), done]
        }
        EventCmd::Delete { guild_id, event_id } => vec![EventEvent::Deleted { guild_id: *guild_id, event_id: *event_id }, done],
        EventCmd::Interest { guild_id, event_id, on } => vec![EventEvent::Interest { guild_id: *guild_id, event_id: *event_id, on: *on }],
        EventCmd::Occurrence { guild_id, event_id, original_ms, skip } => vec![EventEvent::Skipped { guild_id: *guild_id, event_id: *event_id, original_ms: *original_ms, skip: *skip }, done],
    })
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: EventCmd) {
    let (action, key) = cmd.action();
    let fail = |failure| Event::GuildEvents(EventEvent::Failed { action, key, failure });
    // Réservé aux comptes utilisateur.
    if ctx.is_bot() && action == EventAction::Interest {
        ctx.out.send(fail(Failure::Problem(crate::state::Problem::Forbidden)));
        return;
    }
    let req = request(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    let body = match api(&ctx, req.route, req.method, &url, req.body, MAX_WAIT, std::future::pending()).await {
        Ok(b) => b,
        Err(e) => return ctx.out.send(failed(e, fail)),
    };
    if let EventCmd::Load { guild_id } = cmd {
        let Ok(items) = serde_json::from_str::<Vec<Item<RawEvent>>>(&body) else {
            return ctx.out.send(fail(Failure::Problem(crate::state::Problem::Api(String::new()))));
        };
        let events = list(items, Some(guild_id));
        // Mes inscriptions : une lecture de plus, seulement pour un compte utilisateur.
        let mine = if ctx.is_bot() {
            None
        } else {
            let url = format!("{}/users/@me/scheduled-events?guild_ids={guild_id}", ctx.api_root);
            let route = Route { template: "GET /users/@me/scheduled-events", major: 0 };
            api(&ctx, route, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.ok().and_then(|b| subscriptions(&b))
        };
        return ctx.out.send(Event::GuildEvents(EventEvent::Loaded { guild_id, events, mine }));
    }
    match answer(&cmd, &body) {
        Some(evs) => evs.into_iter().for_each(|e| ctx.out.send(Event::GuildEvents(e))),
        None => ctx.out.send(fail(Failure::Problem(crate::state::Problem::Api(String::new())))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    fn gateway(name: &str) -> (String, String) {
        let v: Value = serde_json::from_str(&fixture(name)).unwrap_or_default();
        (v["t"].as_str().unwrap_or_default().to_string(), v["d"].to_string())
    }

    #[test]
    fn list_with_counts_and_recurrence() {
        let l = decode_list(&fixture("rest_guild_scheduled_events.json"));
        assert_eq!(l.len(), 3, "l'entrée illisible est écartée");
        let a = &l[0];
        assert_eq!((a.name.as_str(), a.status, a.user_count, a.place.clone()), ("Soirée jeux", Status::Scheduled, Some(12), Place::Voice(3000000000000000901)));
        assert_eq!(a.start_ms, 1_795_284_000_000);
        assert_eq!(a.end_ms, None);
        assert!(a.image.is_some());
        let r = a.recurrence.as_ref().map(|r| (r.frequency, r.interval, r.by_weekday.clone()));
        assert_eq!(r, Some((Frequency::Weekly, 1, vec![3])));
        let b = &l[1];
        assert_eq!((b.place.clone(), b.status, b.end_ms.is_some()), (Place::External("Parc de la Tête d'Or".into()), Status::Active, true));
        assert_eq!(l[2].place, Place::Stage(3000000000000000902));
        assert_eq!(l[2].recurrence.as_ref().and_then(|r| r.by_n_weekday), Some((1, 4)));
        assert_eq!(a.share_url(), "https://discord.com/events/2000000000000000001/5000000000000000001");
    }

    #[test]
    fn gateway_packets() {
        let (k, d) = gateway("guild_scheduled_event_create.json");
        assert!(matches!(decode(&k, &d), Some(EventEvent::Upsert(ScheduledEvent { id: 5000000000000000004, user_count: None, .. }))));
        let (k, d) = gateway("guild_scheduled_event_user_add.json");
        assert!(matches!(decode(&k, &d), Some(EventEvent::UserAdd { event_id: 5000000000000000004, user_id: 1000000000000000003, .. })));
        assert!(decode("GUILD_SCHEDULED_EVENT_DELETE", &d).is_none(), "sans identifiant d'événement");
        let del = r#"{ "id": "5", "guild_id": "6", "status": 4 }"#;
        assert!(matches!(decode("GUILD_SCHEDULED_EVENT_DELETE", del), Some(EventEvent::Deleted { guild_id: 6, event_id: 5 })));
        let occ = r#"{ "guild_scheduled_event_id": "5", "user_id": "1", "guild_id": "6", "guild_scheduled_event_exception_id": "7" }"#;
        assert!(decode("GUILD_SCHEDULED_EVENT_USER_REMOVE", occ).is_none(), "une seule occurrence");
    }

    #[test]
    fn my_subscriptions() {
        let body = r#"[{ "guild_scheduled_event_id": "5", "user_id": "1", "response": 1 },
                       { "guild_scheduled_event_id": "6", "user_id": "1", "guild_scheduled_event_exception_id": "9", "response": 1 },
                       { "guild_scheduled_event_id": "7", "user_id": "1", "response": 0 }, 42]"#;
        assert_eq!(subscriptions(body), Some(vec![5]));
        assert_eq!(subscriptions("{}"), None);
    }

    #[test]
    fn request_shapes() {
        let d = EventDraft { name: "Pique-nique".into(), description: String::new(), start_ms: 1_795_284_000_000, end_ms: Some(1_795_291_200_000), place: Place::External("Parc".into()), recurrence: None, image: None };
        let c = request(&EventCmd::Create { guild_id: 4, draft: d.clone() });
        assert_eq!((c.method, c.path.as_str(), c.route.major), (Method::POST, "/guilds/4/scheduled-events", 4));
        assert_eq!(
            c.body,
            Some(json!({
                "name": "Pique-nique", "description": null, "scheduled_start_time": "2026-11-21T18:00:00.000Z", "scheduled_end_time": "2026-11-21T20:00:00.000Z",
                "entity_type": 3, "channel_id": null, "entity_metadata": { "location": "Parc" }, "privacy_level": 2
            }))
        );
        let rule = Recurrence { frequency: Frequency::Monthly, interval: 1, by_weekday: Vec::new(), by_n_weekday: Some((3, 0)), by_month_day: None };
        let e = request(&EventCmd::Edit { guild_id: 4, event_id: 9, draft: EventDraft { place: Place::Voice(77), end_ms: None, recurrence: Some(rule), image: Some("data:image/png;base64,AA==".into()), ..d } });
        let b = e.body.unwrap_or_default();
        assert_eq!((e.method, e.path.as_str()), (Method::PATCH, "/guilds/4/scheduled-events/9"));
        assert_eq!((&b["entity_type"], &b["channel_id"], &b["entity_metadata"], &b["scheduled_end_time"]), (&json!(2), &json!("77"), &Value::Null, &Value::Null));
        assert!(b.get("privacy_level").is_none());
        assert_eq!(b["image"], json!("data:image/png;base64,AA=="), "couverture envoyée");
        // Occurrence d'un événement récurrent : sautée, puis rétablie.
        let start = 1_795_284_000_000;
        let skip = request(&EventCmd::Occurrence { guild_id: 4, event_id: 9, original_ms: start, skip: true });
        assert_eq!((skip.method, skip.path.as_str()), (Method::POST, "/guilds/4/scheduled-events/9/exceptions"));
        assert_eq!(skip.body, Some(json!({ "original_scheduled_start_time": "2026-11-21T18:00:00.000Z", "is_canceled": true })));
        let back = request(&EventCmd::Occurrence { guild_id: 4, event_id: 9, original_ms: start, skip: false });
        assert_eq!(back.method, Method::DELETE);
        assert_eq!(back.path, format!("/guilds/4/scheduled-events/9/exceptions/{}", exception_id(start)));
        assert_eq!(exception_ms(exception_id(start)), start);
        let raw: RawEvent = serde_json::from_value(json!({
            "id": "9", "guild_id": "4", "scheduled_start_time": "2026-11-21T18:00:00.000Z", "status": 1, "entity_type": 3,
            "guild_scheduled_event_exceptions": [
                { "event_id": "9", "event_exception_id": exception_id(start).to_string(), "is_canceled": true },
                { "event_id": "9", "event_exception_id": "1", "is_canceled": false }, 7
            ]
        }))
        .unwrap_or_else(|e| panic!("{e}"));
        let e = raw.into_event(None).unwrap_or_else(|| panic!("événement"));
        assert_eq!(e.skipped, vec![start]);
        assert!(e.next_skipped());
        assert_eq!(b["recurrence_rule"], json!({ "start": "2026-11-21T18:00:00.000Z", "frequency": 1, "interval": 1, "by_weekday": null, "by_n_weekday": [{ "n": 3, "day": 0 }], "by_month": null, "by_month_day": null }));
        let s = request(&EventCmd::SetStatus { guild_id: 4, event_id: 9, status: Status::Canceled });
        assert_eq!(s.body, Some(json!({ "status": 4 })));
        let i = request(&EventCmd::Interest { guild_id: 4, event_id: 9, on: false });
        assert_eq!((i.method, i.path.as_str()), (Method::DELETE, "/guilds/4/scheduled-events/9/users/@me"));
        assert_eq!(request(&EventCmd::Load { guild_id: 4 }).path, "/guilds/4/scheduled-events?with_user_count=true");
    }
}
