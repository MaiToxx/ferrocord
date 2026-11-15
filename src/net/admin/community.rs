//! Fonctions du serveur, écran de bienvenue, intégration (lot A7).
//!
//! Formes vérifiées le 13 novembre 2026 :
//! - documentées par Discord (Guild, et docs.discord.food « Guild ») :
//!   champs `features`, `rules_channel_id`, `public_updates_channel_id`,
//!   `description` du serveur (sous `properties` pour un compte utilisateur),
//!   `guild_scheduled_events` de GUILD_CREATE ; Get et Modify Guild Welcome
//!   Screen (`GET` / `PATCH /guilds/{id}/welcome-screen` : `enabled`,
//!   `description`, `welcome_channels`), Get et Modify Guild Onboarding
//!   (`GET` / `PUT /guilds/{id}/onboarding` : `prompts`,
//!   `default_channel_ids`, `enabled`, `mode`) ;
//! - réservées aux comptes utilisateur, forme lue sur docs.discord.food
//!   (« Guild » : Create et Modify Guild Onboarding Responses) : `POST`
//!   (premières réponses) et `PUT /guilds/{id}/onboarding-responses`
//!   (`onboarding_responses`, `onboarding_prompts_seen`,
//!   `onboarding_responses_seen` ; réponse : l'objet, ou 204) ; le champ
//!   `responses` de Get Guild Onboarding (mes réponses).
//!
//! Les lectures suivent l'ouverture d'une fenêtre (ou l'arrivée sur un
//! serveur), les écritures un clic ; tout passe par le limiteur commun.

use super::events::{failed, list, RawEvent};
use crate::net::raw::{de_lenient, de_loose_id, Item};
use crate::net::ratelimit::Route;
use crate::net::rest::{api, Ctx};
use crate::net::Out;
use crate::state::admin::community::{CommunityAction, CommunityCmd, CommunityEvent, GuildInfo, Onboarding, Prompt, PromptOption, Venue, WelcomeChannel, WelcomeScreen};
use crate::state::admin::events::{EventEvent, Failure};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(15);

/// Types de salons gardés comme lieux d'événements : vocal (2) et conférence (13).
const VOICE: u64 = 2;
const STAGE: u64 = 13;

#[derive(Deserialize, Default)]
struct RawProps {
    #[serde(default, deserialize_with = "de_lenient")]
    features: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_loose_id")]
    rules_channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    public_updates_channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
}

#[derive(Deserialize)]
struct RawVenue {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    position: Option<i32>,
}

impl RawVenue {
    fn venue(&self) -> Option<Venue> {
        let kind = self.kind?;
        (kind == VOICE || kind == STAGE).then(|| Venue { id: self.id.unwrap_or(0), name: self.name.clone().unwrap_or_default(), stage: kind == STAGE, position: self.position.unwrap_or(0) }).filter(|v| v.id != 0)
    }
}

/// Ce que ce lot lit d'un serveur (READY, GUILD_CREATE, GUILD_UPDATE) ; le
/// reste du paquet est sauté sans être construit.
#[derive(Deserialize)]
struct RawGuildPart {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    unavailable: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    properties: Option<RawProps>,
    #[serde(default, deserialize_with = "de_lenient")]
    features: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_loose_id")]
    rules_channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    public_updates_channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_scheduled_events: Option<Vec<Item<RawEvent>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    channels: Option<Vec<Item<RawVenue>>>,
}

impl RawGuildPart {
    /// Fonctions et salons : sous `properties` (compte utilisateur) ou au premier niveau.
    fn props(&mut self) -> RawProps {
        match self.properties.take() {
            Some(p) => p,
            None => RawProps { features: self.features.take(), rules_channel_id: self.rules_channel_id, public_updates_channel_id: self.public_updates_channel_id, description: self.description.take() },
        }
    }

    /// Paquet complet (READY, GUILD_CREATE) : fonctions, lieux et événements.
    fn full(mut self, out: &mut Vec<Event>) {
        let Some(guild_id) = self.id.filter(|_| self.unavailable != Some(true)) else { return };
        let p = self.props();
        let venues = self.channels.take().unwrap_or_default().into_iter().filter_map(|c| c.0?.venue()).collect();
        let info = GuildInfo { features: p.features.unwrap_or_default(), rules_channel: p.rules_channel_id, updates_channel: p.public_updates_channel_id, description: p.description.unwrap_or_default(), venues };
        out.push(Event::Community(CommunityEvent::Info { guild_id, info }));
        if let Some(evs) = self.guild_scheduled_events.take() {
            out.push(Event::GuildEvents(EventEvent::Guild { guild_id, events: list(evs, Some(guild_id)) }));
        }
    }
}

#[derive(Deserialize)]
struct RawReady {
    #[serde(default)]
    guilds: Vec<Item<RawGuildPart>>,
}

/// Paquets de la passerelle qui intéressent ce lot.
pub(in crate::net) fn decode(kind: &str, d: &str) -> Vec<Event> {
    let mut out = Vec::new();
    match kind {
        "READY" => {
            if let Ok(r) = serde_json::from_str::<RawReady>(d) {
                r.guilds.into_iter().filter_map(|g| g.0).for_each(|g| g.full(&mut out));
            }
        }
        "GUILD_CREATE" => {
            if let Ok(g) = serde_json::from_str::<RawGuildPart>(d) {
                g.full(&mut out);
            }
        }
        "GUILD_UPDATE" => {
            if let Ok(mut g) = serde_json::from_str::<RawGuildPart>(d)
                && let Some(guild_id) = g.id
            {
                let p = g.props();
                out.push(Event::Community(CommunityEvent::InfoUpdate {
                    guild_id,
                    features: p.features.unwrap_or_default(),
                    rules_channel: p.rules_channel_id,
                    updates_channel: p.public_updates_channel_id,
                    description: p.description.unwrap_or_default(),
                }));
            }
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            if let Ok(c) = serde_json::from_str::<RawVenue>(d)
                && let (Some(guild_id), Some(venue)) = (c.guild_id, c.venue())
            {
                out.push(Event::Community(CommunityEvent::VenueUpsert { guild_id, venue }));
            }
        }
        "CHANNEL_DELETE" => {
            if let Ok(c) = serde_json::from_str::<RawVenue>(d)
                && let (Some(guild_id), Some(id), Some(VOICE | STAGE)) = (c.guild_id, c.id, c.kind)
            {
                out.push(Event::Community(CommunityEvent::VenueDelete { guild_id, id }));
            }
        }
        _ => {}
    }
    out
}

pub(in crate::net) fn dispatch(kind: &str, d: &str, out: &Out) {
    if matches!(kind, "READY" | "GUILD_CREATE" | "GUILD_UPDATE" | "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "CHANNEL_DELETE") {
        decode(kind, d).into_iter().for_each(|e| out.send(e));
    }
}

#[derive(Deserialize)]
struct RawWelcomeChannel {
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
}

#[derive(Deserialize)]
struct RawWelcome {
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    welcome_channels: Option<Vec<Item<RawWelcomeChannel>>>,
}

pub fn decode_welcome(body: &str) -> Option<WelcomeScreen> {
    let w = serde_json::from_str::<RawWelcome>(body).ok()?;
    let channels = w
        .welcome_channels
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| c.0)
        .filter_map(|c| Some(WelcomeChannel { channel_id: c.channel_id?, description: c.description.unwrap_or_default(), emoji_id: c.emoji_id, emoji_name: c.emoji_name }))
        .collect();
    Some(WelcomeScreen { description: w.description.unwrap_or_default(), channels })
}

#[derive(Deserialize, Default)]
struct RawEmoji {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    animated: Option<bool>,
}

fn ids(v: Option<Vec<Value>>) -> Vec<u64> {
    v.unwrap_or_default().iter().filter_map(|x| x.as_str().and_then(|s| s.parse().ok()).or_else(|| x.as_u64())).collect()
}

#[derive(Deserialize)]
struct RawOption {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    title: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    channel_ids: Option<Vec<Value>>,
    #[serde(default, deserialize_with = "de_lenient")]
    role_ids: Option<Vec<Value>>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji: Option<RawEmoji>,
}

#[derive(Deserialize)]
struct RawPrompt {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    title: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    single_select: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    required: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    in_onboarding: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    options: Option<Vec<Item<RawOption>>>,
}

#[derive(Deserialize)]
struct RawOnboarding {
    #[serde(default, deserialize_with = "de_lenient")]
    prompts: Option<Vec<Item<RawPrompt>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    default_channel_ids: Option<Vec<Value>>,
    #[serde(default, deserialize_with = "de_lenient")]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    mode: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    responses: Option<Vec<Value>>,
}

pub fn decode_onboarding(body: &str) -> Option<Onboarding> {
    let o = serde_json::from_str::<RawOnboarding>(body).ok()?;
    let option = |r: RawOption| {
        let e = r.emoji.unwrap_or_default();
        Some(PromptOption {
            id: r.id?,
            title: r.title.unwrap_or_default(),
            description: r.description.unwrap_or_default(),
            channel_ids: ids(r.channel_ids),
            role_ids: ids(r.role_ids),
            emoji_id: e.id,
            emoji_name: e.name.filter(|n| !n.is_empty()),
            emoji_animated: e.animated.unwrap_or(false),
        })
    };
    let prompts = o
        .prompts
        .unwrap_or_default()
        .into_iter()
        .filter_map(|p| p.0)
        .filter_map(|p| {
            Some(Prompt {
                id: p.id?,
                kind: p.kind.unwrap_or(0),
                title: p.title.unwrap_or_default(),
                single_select: p.single_select.unwrap_or(false),
                required: p.required.unwrap_or(false),
                in_onboarding: p.in_onboarding.unwrap_or(true),
                options: p.options.unwrap_or_default().into_iter().filter_map(|x| option(x.0?)).collect(),
            })
        })
        .collect();
    Some(Onboarding { prompts, default_channels: ids(o.default_channel_ids), enabled: o.enabled.unwrap_or(false), mode: o.mode.unwrap_or(0), responses: ids(o.responses) })
}

fn id_list(v: &[u64]) -> Value {
    v.iter().map(|i| Value::from(i.to_string())).collect()
}

fn onboarding_body(o: &Onboarding) -> Value {
    let prompts: Vec<Value> = o
        .prompts
        .iter()
        .map(|p| {
            let options: Vec<Value> = p
                .options
                .iter()
                .map(|x| {
                    json!({
                        "id": x.id.to_string(),
                        "title": x.title,
                        "description": if x.description.is_empty() { Value::Null } else { x.description.clone().into() },
                        "channel_ids": id_list(&x.channel_ids),
                        "role_ids": id_list(&x.role_ids),
                        "emoji_id": x.emoji_id.map(|i| i.to_string()),
                        "emoji_name": x.emoji_name,
                        "emoji_animated": x.emoji_animated,
                    })
                })
                .collect();
            json!({ "id": p.id.to_string(), "type": p.kind, "title": p.title, "single_select": p.single_select, "required": p.required, "in_onboarding": p.in_onboarding, "options": options })
        })
        .collect();
    json!({ "prompts": prompts, "default_channel_ids": id_list(&o.default_channels), "enabled": o.enabled, "mode": o.mode })
}

fn welcome_body(enabled: bool, s: &WelcomeScreen) -> Value {
    let channels: Vec<Value> = s
        .channels
        .iter()
        .map(|c| json!({ "channel_id": c.channel_id.to_string(), "description": c.description, "emoji_id": c.emoji_id.map(|i| i.to_string()), "emoji_name": c.emoji_name }))
        .collect();
    json!({ "enabled": enabled, "description": if s.description.is_empty() { Value::Null } else { s.description.clone().into() }, "welcome_channels": channels })
}

fn seen(ids: &[u64], at_ms: i64) -> Value {
    Value::Object(ids.iter().map(|i| (i.to_string(), Value::from(at_ms))).collect::<Map<_, _>>())
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn request(cmd: &CommunityCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        CommunityCmd::LoadWelcome { guild_id } => r("GET /guilds/{}/welcome-screen", *guild_id, Method::GET, format!("/guilds/{guild_id}/welcome-screen"), None),
        CommunityCmd::SaveWelcome { guild_id, enabled, screen } => {
            r("PATCH /guilds/{}/welcome-screen", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/welcome-screen"), Some(welcome_body(*enabled, screen)))
        }
        CommunityCmd::LoadOnboarding { guild_id } => r("GET /guilds/{}/onboarding", *guild_id, Method::GET, format!("/guilds/{guild_id}/onboarding"), None),
        CommunityCmd::SaveOnboarding { guild_id, onboarding } => r("PUT /guilds/{}/onboarding", *guild_id, Method::PUT, format!("/guilds/{guild_id}/onboarding"), Some(onboarding_body(onboarding))),
        CommunityCmd::Respond { guild_id, options, prompts_seen, options_seen, at_ms, first } => {
            let body = json!({ "onboarding_responses": id_list(options), "onboarding_prompts_seen": seen(prompts_seen, *at_ms), "onboarding_responses_seen": seen(options_seen, *at_ms) });
            let method = if *first { Method::POST } else { Method::PUT };
            r("POST /guilds/{}/onboarding-responses", *guild_id, method, format!("/guilds/{guild_id}/onboarding-responses"), Some(body))
        }
    }
}

/// Réponse reçue : l'événement à publier (`None` : réponse sans la forme attendue).
fn answer(cmd: &CommunityCmd, body: &str) -> Option<CommunityEvent> {
    let (action, guild_id) = cmd.action();
    Some(match cmd {
        CommunityCmd::LoadWelcome { .. } => CommunityEvent::Welcome { guild_id, screen: decode_welcome(body)? },
        CommunityCmd::LoadOnboarding { .. } => CommunityEvent::Onboarding { guild_id, onboarding: decode_onboarding(body)? },
        CommunityCmd::SaveWelcome { .. } | CommunityCmd::SaveOnboarding { .. } => CommunityEvent::Saved { action, guild_id },
        // 204 (aucune réponse) ou l'objet : ce qui a été envoyé est enregistré.
        CommunityCmd::Respond { options, .. } => CommunityEvent::Responded { guild_id, options: options.clone() },
    })
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: CommunityCmd) {
    let (action, guild_id) = cmd.action();
    let fail = |failure| Event::Community(CommunityEvent::Failed { action, guild_id, failure });
    if ctx.is_bot() && action == CommunityAction::Respond {
        ctx.out.send(fail(Failure::Problem(Problem::Forbidden)));
        return;
    }
    let req = request(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    let ev = match api(&ctx, req.route, req.method, &url, req.body, MAX_WAIT, std::future::pending()).await {
        Ok(body) => answer(&cmd, &body).map(Event::Community).unwrap_or_else(|| fail(Failure::Problem(Problem::Api(String::new())))),
        Err(e) => failed(e, fail),
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::events::Place;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn guild_create_brings_features_venues_and_events() {
        let v: Value = serde_json::from_str(&fixture("guild_create_community.json")).unwrap_or_default();
        let evs = decode("GUILD_CREATE", &v["d"].to_string());
        let [Event::Community(CommunityEvent::Info { guild_id, info }), Event::GuildEvents(EventEvent::Guild { events, .. })] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!(*guild_id, 2000000000000000501);
        assert!(info.has("COMMUNITY") && info.has("GUILD_ONBOARDING"));
        assert_eq!((info.rules_channel, info.updates_channel, info.description.as_str()), (Some(3000000000000000502), Some(3000000000000000503), "Un club fictif"));
        assert_eq!(info.venues.iter().map(|v| (v.name.as_str(), v.stage)).collect::<Vec<_>>(), vec![("Salon vocal", false), ("Scène", true)]);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].place, Place::Voice(3000000000000000504));
        // Compte bot : au premier niveau.
        let evs = decode("GUILD_UPDATE", r#"{ "id": "9", "features": ["NEWS"], "rules_channel_id": "4", "public_updates_channel_id": null }"#);
        assert!(matches!(evs.as_slice(), [Event::Community(CommunityEvent::InfoUpdate { guild_id: 9, rules_channel: Some(4), updates_channel: None, .. })]), "{evs:?}");
        assert!(decode("GUILD_CREATE", r#"{ "id": "9", "unavailable": true }"#).is_empty());
        let evs = decode("CHANNEL_CREATE", r#"{ "id": "8", "guild_id": "9", "type": 13, "name": "Scène 2", "position": 3 }"#);
        assert!(matches!(evs.as_slice(), [Event::Community(CommunityEvent::VenueUpsert { guild_id: 9, venue: Venue { id: 8, stage: true, .. } })]), "{evs:?}");
        assert!(decode("CHANNEL_CREATE", r#"{ "id": "8", "guild_id": "9", "type": 0, "name": "écrit" }"#).is_empty());
        assert!(matches!(decode("CHANNEL_DELETE", r#"{ "id": "8", "guild_id": "9", "type": 2 }"#).as_slice(), [Event::Community(CommunityEvent::VenueDelete { id: 8, .. })]));
    }

    #[test]
    fn ready_guilds_are_read_one_by_one() {
        let d = r#"{ "guilds": [ { "id": "1", "properties": { "features": ["COMMUNITY"] }, "guild_scheduled_events": [] }, 7, { "id": "2", "unavailable": true } ], "user": {} }"#;
        let evs = decode("READY", d);
        assert_eq!(evs.len(), 2, "{evs:?}");
    }

    #[test]
    fn welcome_and_onboarding() {
        let w = decode_welcome(&fixture("rest_welcome_screen.json"));
        let Some(w) = w else { panic!("écran") };
        assert_eq!(w.description, "Bienvenue sur le club fictif !");
        assert_eq!(w.channels.len(), 2);
        assert_eq!((w.channels[0].emoji_name.as_deref(), w.channels[1].emoji_id), (Some("📜"), Some(4000000000000000501)));
        let Some(o) = decode_onboarding(&fixture("rest_guild_onboarding.json")) else { panic!("intégration") };
        assert!(o.enabled);
        assert_eq!((o.mode, o.default_channels.len(), o.responses.clone()), (1, 2, vec![6000000000000000011]));
        assert_eq!(o.prompts.len(), 2);
        let p = &o.prompts[0];
        assert_eq!((p.title.as_str(), p.single_select, p.required, p.options.len()), ("Qu'est-ce qui vous amène ?", true, true, 2));
        assert_eq!(p.options[0].role_ids, vec![7000000000000000501]);
        assert_eq!(p.options[0].emoji_name.as_deref(), Some("🎲"));
        assert_eq!(o.prompts[1].options[0].emoji_id, Some(4000000000000000501));
    }

    #[test]
    fn request_shapes() {
        let s = WelcomeScreen { description: String::new(), channels: vec![WelcomeChannel { channel_id: 5, description: "Règles".into(), emoji_id: None, emoji_name: Some("📜".into()) }] };
        let r = request(&CommunityCmd::SaveWelcome { guild_id: 3, enabled: true, screen: s });
        assert_eq!((r.method, r.path.as_str()), (Method::PATCH, "/guilds/3/welcome-screen"));
        assert_eq!(r.body, Some(json!({ "enabled": true, "description": null, "welcome_channels": [{ "channel_id": "5", "description": "Règles", "emoji_id": null, "emoji_name": "📜" }] })));
        let Some(o) = decode_onboarding(&fixture("rest_guild_onboarding.json")) else { panic!("intégration") };
        let r = request(&CommunityCmd::SaveOnboarding { guild_id: 3, onboarding: o });
        let b = r.body.unwrap_or_default();
        assert_eq!((r.method, b["mode"].clone(), b["prompts"][0]["options"][0]["role_ids"].clone()), (Method::PUT, json!(1), json!(["7000000000000000501"])));
        assert!(b.get("responses").is_none(), "mes réponses ne partent pas avec l'édition");
        let r = request(&CommunityCmd::Respond { guild_id: 3, options: vec![11], prompts_seen: vec![1], options_seen: vec![11, 12], at_ms: 5, first: true });
        assert_eq!((r.method, r.path.as_str()), (Method::POST, "/guilds/3/onboarding-responses"));
        assert_eq!(r.body, Some(json!({ "onboarding_responses": ["11"], "onboarding_prompts_seen": { "1": 5 }, "onboarding_responses_seen": { "11": 5, "12": 5 } })));
        let cmd = CommunityCmd::Respond { guild_id: 3, options: vec![11], prompts_seen: vec![], options_seen: vec![], at_ms: 5, first: false };
        assert_eq!(request(&cmd).method, Method::PUT);
        assert!(matches!(answer(&cmd, ""), Some(CommunityEvent::Responded { guild_id: 3, .. })), "204");
    }
}
