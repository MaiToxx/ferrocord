//! Appels privés, conférences, statut des salons vocaux et tableau de sons
//! (lot V6), côté réseau. Formes vérifiées le 13 novembre 2026 :
//! - non documentées (docs.discord.food) : CALL_CREATE (`channel_id`,
//!   `message_id`, `region`, `ringing`, `voice_states`), CALL_UPDATE (sans
//!   `voice_states`), CALL_DELETE (`channel_id`, `unavailable`) ;
//!   `POST /channels/{id}/call/ring` et `…/call/stop-ringing` (`recipients`,
//!   204) ; `PUT /channels/{id}/voice-status` (`status`, 204) et
//!   VOICE_CHANNEL_STATUS_UPDATE (`id`, `guild_id`, `status`) ; `voice_states`
//!   des serveurs de READY_SUPPLEMENTAL ;
//! - documentées par Discord : VOICE_STATE_UPDATE (objet état vocal),
//!   STAGE_INSTANCE_CREATE / UPDATE / DELETE, `POST /stage-instances`,
//!   `PATCH` et `DELETE /stage-instances/{channel}`,
//!   `PATCH /guilds/{id}/voice-states/@me` (`channel_id`, `suppress`,
//!   `request_to_speak_timestamp`) et `…/voice-states/{user}` (`channel_id`,
//!   `suppress`), `GET /soundboard-default-sounds`,
//!   `POST /channels/{id}/send-soundboard-sound` (`sound_id`,
//!   `source_guild_id`, 204), VOICE_CHANNEL_EFFECT_SEND.
//!
//! Tout part sur un geste, par le limiteur commun ; aucun en-tête ni champ
//! propre au client officiel.

use super::raw::{de_lenient, de_loose_id, Item, Parts, Shared};
use super::ratelimit::Route;
use super::rest::{api_req, ApiError, Ctx};
use super::Out;
use crate::state::admin::expressions::{ExprCmd, Kind, Sound, SoundEmoji};
use super::voicegw::RawVoiceState;
use crate::state::calls::{Call, CallCmd, CallEvent, EffectEmoji, EffectSend};
use crate::state::voice::{VoiceEvent, VoiceState};
use crate::state::stage::{StageCmd, StageEvent, StageInstance};
use crate::state::Event;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);


fn iso_of_ms(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
}

/// États vocaux d'un appel ou d'un serveur : même lecture que `net::voicegw`
/// (les états vivent dans `state::voice`).
fn states(list: Option<Vec<Item<RawVoiceState>>>, guild: Option<u64>) -> Vec<VoiceState> {
    list.unwrap_or_default().into_iter().filter_map(|s| s.0?.state(guild)).collect()
}

#[derive(Deserialize)]
struct RawCall {
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    message_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    region: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    ringing: Option<Vec<Item<LooseId>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    voice_states: Option<Vec<Item<RawVoiceState>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    unavailable: Option<bool>,
}

/// Identifiant seul, en chaîne ou en nombre.
#[derive(Deserialize)]
#[serde(transparent)]
struct LooseId(#[serde(deserialize_with = "de_loose_id")] Option<u64>);

#[derive(Deserialize)]
struct RawStage {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    topic: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_scheduled_event_id: Option<u64>,
}

impl RawStage {
    fn into_instance(self, guild: Option<u64>) -> Option<StageInstance> {
        Some(StageInstance { id: self.id?, guild_id: self.guild_id.or(guild)?, channel_id: self.channel_id?, topic: self.topic.unwrap_or_default(), event_id: self.guild_scheduled_event_id })
    }
}

#[derive(Deserialize)]
struct RawStatusChannel {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    status: Option<String>,
}

/// Ce que ce lot lit d'un serveur (READY, READY_SUPPLEMENTAL, GUILD_CREATE) ;
/// le reste du paquet est sauté sans être construit.
#[derive(Deserialize)]
struct RawGuildVoice {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    unavailable: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    voice_states: Option<Vec<Item<RawVoiceState>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    stage_instances: Option<Vec<Item<RawStage>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    channels: Option<Vec<Item<RawStatusChannel>>>,
}

impl RawGuildVoice {
    /// Les états vocaux de READY et GUILD_CREATE sont lus par `net::voicegw` ;
    /// ceux de READY_SUPPLEMENTAL (`supplemental`), ici.
    fn events(self, supplemental: bool, out: &mut Vec<Event>) {
        let Some(guild_id) = self.id.filter(|_| self.unavailable != Some(true)) else { return };
        // Paquet sans conférences ni salons (READY_SUPPLEMENTAL) : seulement les états vocaux.
        if self.stage_instances.is_some() || self.channels.is_some() {
            let instances = self.stage_instances.unwrap_or_default().into_iter().filter_map(|i| i.0?.into_instance(Some(guild_id))).collect();
            let statuses = self.channels.unwrap_or_default().into_iter().filter_map(|c| c.0).filter_map(|c| Some((c.id?, c.status?))).collect();
            out.push(StageEvent::Guild { guild_id, instances, statuses }.into());
        }
        if supplemental && self.voice_states.is_some() {
            out.push(Event::Voice(VoiceEvent::States(states(self.voice_states, Some(guild_id)))));
        }
    }
}

#[derive(Deserialize)]
struct RawStatusUpdate {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    status: Option<String>,
}

#[derive(Deserialize)]
struct RawEmoji {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawEffect {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    sound_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    sound_volume: Option<f32>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji: Option<RawEmoji>,
}

fn emoji_of(e: Option<RawEmoji>) -> Option<EffectEmoji> {
    let e = e?;
    let name = e.name.filter(|n| !n.is_empty());
    match (e.id, name) {
        (Some(id), name) => Some(EffectEmoji::Custom { id, name: name.unwrap_or_default() }),
        (None, Some(n)) => Some(EffectEmoji::Unicode(n)),
        (None, None) => None,
    }
}

/// Paquets de la passerelle qui intéressent ce lot.
#[cfg(test)]
pub(super) fn decode(kind: &str, d: &str) -> Vec<Event> {
    decode_shared(kind, d, &Shared::of(kind, d))
}

/// Comme [`decode`], avec le découpage du READY ou du GUILD_CREATE déjà fait
/// pour tous les décodeurs du paquet.
pub(super) fn decode_shared(kind: &str, d: &str, shared: &Shared) -> Vec<Event> {
    let mut out = Vec::new();
    match kind {
        "CALL_CREATE" | "CALL_UPDATE" => {
            if let Ok(raw) = serde_json::from_str::<RawCall>(d)
                && let Some(channel_id) = raw.channel_id
            {
                let ringing = raw.ringing.unwrap_or_default().into_iter().filter_map(|i| i.0?.0).collect();
                let call = Call { channel_id, message_id: raw.message_id, region: raw.region, ringing, unavailable: raw.unavailable.unwrap_or(false) };
                // CALL_UPDATE ne porte pas les états : ceux connus sont gardés.
                let voice_states = (kind == "CALL_CREATE").then(|| states(raw.voice_states, None));
                out.push(CallEvent::Upsert { call, voice_states }.into());
            }
        }
        "CALL_DELETE" => {
            if let Ok(raw) = serde_json::from_str::<RawCall>(d)
                && let Some(channel_id) = raw.channel_id
            {
                out.push(CallEvent::Delete { channel_id, unavailable: raw.unavailable.unwrap_or(false) }.into());
            }
        }
        "READY" | "READY_SUPPLEMENTAL" => {
            shared.guilds().iter().filter_map(|g| g.read::<RawGuildVoice>().ok()).for_each(|g| g.events(kind == "READY_SUPPLEMENTAL", &mut out));
        }
        "GUILD_CREATE" => {
            if let Some(Ok(g)) = shared.guild().map(Parts::read::<RawGuildVoice>) {
                g.events(false, &mut out);
            }
        }
        "STAGE_INSTANCE_CREATE" | "STAGE_INSTANCE_UPDATE" => {
            if let Some(i) = serde_json::from_str::<RawStage>(d).ok().and_then(|r| r.into_instance(None)) {
                out.push(StageEvent::Upsert(i).into());
            }
        }
        "STAGE_INSTANCE_DELETE" => {
            if let Some(channel_id) = serde_json::from_str::<RawStage>(d).ok().and_then(|r| r.channel_id) {
                out.push(StageEvent::Delete { channel_id }.into());
            }
        }
        "VOICE_CHANNEL_STATUS_UPDATE" => {
            if let Ok(raw) = serde_json::from_str::<RawStatusUpdate>(d)
                && let Some(channel_id) = raw.id
            {
                out.push(StageEvent::Status { channel_id, status: raw.status }.into());
            }
        }
        "VOICE_CHANNEL_EFFECT_SEND" => {
            if let Ok(raw) = serde_json::from_str::<RawEffect>(d)
                && let (Some(channel_id), Some(user_id)) = (raw.channel_id, raw.user_id)
            {
                let effect = EffectSend { guild_id: raw.guild_id, channel_id, user_id, sound_id: raw.sound_id, sound_volume: raw.sound_volume.filter(|v| v.is_finite()), emoji: emoji_of(raw.emoji) };
                out.push(CallEvent::Effect(effect).into());
            }
        }
        _ => {}
    }
    out
}

/// Appelé pour chaque paquet, après le traitement principal (READY doit
/// arriver au magasin avant les états vocaux qu'il porte).
pub(super) fn dispatch(kind: &str, d: &str, shared: &Shared, out: &Out) {
    if matches!(
        kind,
        "CALL_CREATE"
            | "CALL_UPDATE"
            | "CALL_DELETE"
            | "READY"
            | "READY_SUPPLEMENTAL"
            | "GUILD_CREATE"
            | "STAGE_INSTANCE_CREATE"
            | "STAGE_INSTANCE_UPDATE"
            | "STAGE_INSTANCE_DELETE"
            | "VOICE_CHANNEL_STATUS_UPDATE"
            | "VOICE_CHANNEL_EFFECT_SEND"
    ) {
        decode_shared(kind, d, shared).into_iter().for_each(|e| out.send(e));
    }
}

#[derive(Deserialize)]
struct RawSound {
    #[serde(default, deserialize_with = "de_loose_id")]
    sound_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    volume: Option<f64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    available: Option<bool>,
}

/// Réponse de `GET /soundboard-default-sounds` (tableau de sons) ; `None` si illisible.
pub fn decode_default_sounds(body: &str) -> Option<Vec<Sound>> {
    let list: Vec<Item<RawSound>> = serde_json::from_str(body).ok()?;
    Some(
        list.into_iter()
            .filter_map(|s| s.0)
            .filter_map(|s| {
                let emoji = match (s.emoji_id, s.emoji_name.filter(|n| !n.is_empty())) {
                    (Some(id), _) => Some(SoundEmoji::Custom(id)),
                    (None, Some(n)) => Some(SoundEmoji::Unicode(n)),
                    (None, None) => None,
                };
                let volume = s.volume.filter(|v| v.is_finite()).map_or(100, |v| (v.clamp(0.0, 1.0) * 100.0).round() as u8);
                Some(Sound { id: s.sound_id?, name: s.name.unwrap_or_default(), volume, emoji, available: s.available.unwrap_or(true), author: None })
            })
            .collect(),
    )
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn ids(list: &[u64]) -> Vec<String> {
    list.iter().map(u64::to_string).collect()
}

fn req(template: &'static str, major: u64, method: Method, path: String, body: Option<Value>) -> Request {
    Request { route: Route { template, major }, method, path, body }
}

fn call_request(cmd: &CallCmd) -> Request {
    match cmd {
        CallCmd::Ring { channel_id: c, recipients } => {
            let r = recipients.as_deref().map_or(Value::Null, |v| json!(ids(v)));
            req("POST /channels/{}/call/ring", *c, Method::POST, format!("/channels/{c}/call/ring"), Some(json!({ "recipients": r })))
        }
        CallCmd::StopRinging { channel_id: c, recipients } => {
            req("POST /channels/{}/call/stop-ringing", *c, Method::POST, format!("/channels/{c}/call/stop-ringing"), Some(json!({ "recipients": ids(recipients) })))
        }
        CallCmd::DefaultSounds => req("GET /soundboard-default-sounds", 0, Method::GET, "/soundboard-default-sounds".into(), None),
        CallCmd::GuildSounds { guild_id: g } => req("GET /guilds/{}/soundboard-sounds", *g, Method::GET, format!("/guilds/{g}/soundboard-sounds"), None),
        CallCmd::SendSound { channel_id: c, sound_id, source_guild_id } => {
            let mut b = json!({ "sound_id": sound_id.to_string() });
            if let Some(g) = source_guild_id {
                b["source_guild_id"] = g.to_string().into();
            }
            req("POST /channels/{}/send-soundboard-sound", *c, Method::POST, format!("/channels/{c}/send-soundboard-sound"), Some(b))
        }
    }
}

fn stage_request(cmd: &StageCmd) -> Request {
    match cmd {
        StageCmd::Start { channel_id: c, topic, notify, .. } => req(
            "POST /stage-instances",
            *c,
            Method::POST,
            "/stage-instances".into(),
            // Réservé aux membres du serveur (2) : la seule valeur non dépréciée.
            Some(json!({ "channel_id": c.to_string(), "topic": topic, "privacy_level": 2, "send_start_notification": notify })),
        ),
        StageCmd::Topic { channel_id: c, topic } => req("PATCH /stage-instances/{}", *c, Method::PATCH, format!("/stage-instances/{c}"), Some(json!({ "topic": topic }))),
        StageCmd::End { channel_id: c } => req("DELETE /stage-instances/{}", *c, Method::DELETE, format!("/stage-instances/{c}"), None),
        StageCmd::Hand { guild_id: g, channel_id: c, at_ms } => {
            let at = at_ms.and_then(iso_of_ms).map_or(Value::Null, Value::String);
            req("PATCH /guilds/{}/voice-states/@me", *g, Method::PATCH, format!("/guilds/{g}/voice-states/@me"), Some(json!({ "channel_id": c.to_string(), "request_to_speak_timestamp": at })))
        }
        StageCmd::Speaker { guild_id: g, channel_id: c, user_id, speak } => {
            let body = json!({ "channel_id": c.to_string(), "suppress": !speak });
            match user_id {
                None => req("PATCH /guilds/{}/voice-states/@me", *g, Method::PATCH, format!("/guilds/{g}/voice-states/@me"), Some(body)),
                Some(u) => req("PATCH /guilds/{}/voice-states/{}", *g, Method::PATCH, format!("/guilds/{g}/voice-states/{u}"), Some(body)),
            }
        }
        StageCmd::Status { channel_id: c, status } => req("PUT /channels/{}/voice-status", *c, Method::PUT, format!("/channels/{c}/voice-status"), Some(json!({ "status": status }))),
    }
}

async fn send(ctx: &Ctx, r: &Request) -> Result<String, ApiError> {
    let url = format!("{}{}", ctx.api_root, r.path);
    let build = |rb: reqwest::RequestBuilder| match &r.body {
        Some(v) => rb.json(v),
        None => rb,
    };
    api_req(ctx, r.route, r.method.clone(), &url, build, ACTION_MAX_WAIT, std::future::pending()).await
}

/// Réponse reçue à une commande d'appel : événements à publier.
fn call_answer(cmd: &CallCmd, body: &str) -> Vec<Event> {
    let (action, key) = cmd.action();
    match cmd {
        CallCmd::DefaultSounds => match decode_default_sounds(body) {
            Some(list) => vec![CallEvent::DefaultSounds(list).into()],
            None => vec![CallEvent::Failed { action, key, problem: Some(crate::state::Problem::Api(String::new())) }.into()],
        },
        // Même décodeur que la page Sons des paramètres du serveur (lot A6).
        CallCmd::GuildSounds { guild_id } => {
            let list = super::admin::expressions::answer(&ExprCmd::List { guild_id: *guild_id, kind: Kind::Sound }, body);
            let mut v: Vec<Event> = list.map(Event::Expressions).into_iter().collect();
            v.push(CallEvent::Done { action, key }.into());
            v
        }
        _ => vec![CallEvent::Done { action, key }.into()],
    }
}

pub(super) async fn run_call(ctx: Arc<Ctx>, cmd: CallCmd) {
    let (action, key) = cmd.action();
    let events = match send(&ctx, &call_request(&cmd)).await {
        Ok(body) => call_answer(&cmd, &body),
        Err(ApiError::Captcha) => vec![CallEvent::Failed { action, key, problem: None }.into()],
        Err(e) => vec![e.into_event(|p| CallEvent::Failed { action, key, problem: Some(p) }.into())],
    };
    events.into_iter().for_each(|e| ctx.out.send(e));
}

fn stage_answer(cmd: &StageCmd, body: &str) -> Vec<Event> {
    let busy = cmd.busy();
    let mut v = Vec::new();
    match cmd {
        // La passerelle confirmera aussi : la réponse est seulement plus rapide.
        StageCmd::Start { .. } | StageCmd::Topic { .. } => {
            if let Some(i) = serde_json::from_str::<RawStage>(body).ok().and_then(|r| r.into_instance(None)) {
                v.push(StageEvent::Upsert(i).into());
            }
        }
        StageCmd::End { channel_id } => v.push(StageEvent::Delete { channel_id: *channel_id }.into()),
        _ => {}
    }
    v.push(StageEvent::Done { busy }.into());
    v
}

pub(super) async fn run_stage(ctx: Arc<Ctx>, cmd: StageCmd) {
    let busy = cmd.busy();
    let events = match send(&ctx, &stage_request(&cmd)).await {
        Ok(body) => stage_answer(&cmd, &body),
        Err(ApiError::Captcha) => vec![StageEvent::Failed { busy, problem: None }.into()],
        Err(e) => vec![e.into_event(|p| StageEvent::Failed { busy, problem: Some(p) }.into())],
    };
    events.into_iter().for_each(|e| ctx.out.send(e));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::stage::StageAction;

    /// Le découpage partagé rend exactement ce que rendait la lecture du
    /// texte entier (ancienne écriture, gardée ici comme référence).
    #[test]
    fn shared_split_reads_like_the_whole_text() {
        fn whole(kind: &str, d: &str) -> Vec<Event> {
            #[derive(Deserialize)]
            struct RawGuilds {
                #[serde(default)]
                guilds: Vec<Item<RawGuildVoice>>,
            }
            let mut out = Vec::new();
            match kind {
                "READY" | "READY_SUPPLEMENTAL" => {
                    if let Ok(r) = serde_json::from_str::<RawGuilds>(d) {
                        r.guilds.into_iter().filter_map(|g| g.0).for_each(|g| g.events(kind == "READY_SUPPLEMENTAL", &mut out));
                    }
                }
                "GUILD_CREATE" => {
                    if let Ok(g) = serde_json::from_str::<RawGuildVoice>(d) {
                        g.events(false, &mut out);
                    }
                }
                _ => {}
            }
            out
        }
        let mut events = 0;
        for (kind, d) in super::super::raw::guild_texts() {
            let got = decode(kind, &d);
            events += got.len();
            assert_eq!(format!("{got:?}"), format!("{:?}", whole(kind, &d)), "{kind} {d}");
        }
        assert!(events > 5, "{events} événements comparés");
    }

    fn fixture(name: &str) -> Value {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let s = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        serde_json::from_str(&s).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    fn packet(name: &str) -> Vec<Event> {
        let v = fixture(name);
        decode(v["t"].as_str().unwrap_or_default(), &v["d"].to_string())
    }

    #[test]
    fn call_create_update_delete() {
        let ev = packet("call_create");
        let [Event::Calls(CallEvent::Upsert { call, voice_states: Some(states) })] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!(call.channel_id, 4000000000000000001);
        assert_eq!(call.ringing, vec![1000000000000000001]);
        assert_eq!(call.region.as_deref(), Some("rotterdam"));
        assert!(call.started_ms().is_some());
        assert_eq!(states.len(), 1, "l'état illisible est ignoré");
        assert_eq!((states[0].user_id, states[0].channel_id, states[0].guild_id), (1000000000000000002, 4000000000000000001, None));
        let ev = packet("call_update");
        let [Event::Calls(CallEvent::Upsert { call, voice_states: None })] = ev.as_slice() else { panic!("{ev:?}") };
        assert!(call.ringing.is_empty());
        let ev = packet("call_delete");
        assert!(matches!(ev.as_slice(), [Event::Calls(CallEvent::Delete { channel_id: 4000000000000000001, unavailable: false })]));
        assert!(decode("CALL_CREATE", "{}").is_empty());
        assert!(decode("CALL_CREATE", "pas du json").is_empty());
    }

    #[test]
    fn voice_state_reads_stage_fields() {
        // Les états vocaux sont lus par `net::voicegw` (lot V4) ; ce lot lit les champs de conférence.
        let v = fixture("voice_state_update_stage");
        let ev = super::super::voicegw::decode("VOICE_STATE_UPDATE", &v["d"].to_string());
        let [Event::Voice(VoiceEvent::StateUpdate { state: Some(s), .. })] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!(s.guild_id, Some(2000000000000000601));
        assert!(s.suppress);
        assert_eq!(s.request_to_speak, Some(1_763_049_600_000));
        assert!(!s.muted() && !s.deafened());
        assert!(decode("VOICE_STATE_UPDATE", &v["d"].to_string()).is_empty(), "lu une seule fois");
    }

    #[test]
    fn guild_create_brings_stages_statuses_and_voice_states() {
        let ev = packet("guild_create_stage");
        let [Event::Stage(StageEvent::Guild { guild_id, instances, statuses })] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!(*guild_id, 2000000000000000601);
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].topic, "Questions du jeudi");
        assert_eq!(statuses, &vec![(3000000000000000602, "On joue aux échecs".to_string())]);
        // READY_SUPPLEMENTAL : seulement les états, ajoutés à ceux de `state::voice`.
        let d = r#"{ "guilds": [ { "id": "7", "voice_states": [ { "user_id": "3", "channel_id": "8" } ] }, 5 ] }"#;
        let ev = decode("READY_SUPPLEMENTAL", d);
        let [Event::Voice(VoiceEvent::States(states))] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!(states.iter().map(|s| (s.guild_id, s.channel_id, s.user_id)).collect::<Vec<_>>(), vec![(Some(7), 8, 3)]);
    }

    #[test]
    fn stage_instances_status_and_effects() {
        let ev = packet("stage_instance_create");
        let [Event::Stage(StageEvent::Upsert(i))] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!((i.channel_id, i.topic.as_str()), (3000000000000000601, "Questions du jeudi"));
        let ev = decode("STAGE_INSTANCE_DELETE", &fixture("stage_instance_create")["d"].to_string());
        assert!(matches!(ev.as_slice(), [Event::Stage(StageEvent::Delete { channel_id: 3000000000000000601 })]));
        let ev = packet("voice_channel_status_update");
        assert!(matches!(ev.as_slice(), [Event::Stage(StageEvent::Status { channel_id: 3000000000000000602, status: Some(_) })]), "{ev:?}");
        let ev = packet("voice_channel_effect_send");
        let [Event::Calls(CallEvent::Effect(e))] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!((e.sound_id, e.sound_volume), (Some(7000000000000000003), Some(0.5)));
        assert_eq!(e.emoji, Some(EffectEmoji::Unicode("🦆".into())));
    }

    #[test]
    fn default_sounds() {
        let list = decode_default_sounds(&fixture("rest_soundboard_default_sounds").to_string()).unwrap_or_default();
        assert_eq!(list.iter().map(|s| (s.id, s.name.as_str())).collect::<Vec<_>>(), vec![(7000000000000000001, "quack"), (7000000000000000002, "airhorn"), (7000000000000000003, "cricket")]);
        assert_eq!(list[0].emoji, Some(SoundEmoji::Unicode("🦆".into())));
        assert_eq!(list[1].volume, 100);
        assert!(decode_default_sounds("{}").is_none());
    }

    #[test]
    fn requests_have_the_verified_shape() {
        let r = call_request(&CallCmd::Ring { channel_id: 4, recipients: None });
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::POST, "/channels/4/call/ring", Some(json!({ "recipients": null }))));
        let r = call_request(&CallCmd::StopRinging { channel_id: 4, recipients: vec![1] });
        assert_eq!((r.path.as_str(), r.body), ("/channels/4/call/stop-ringing", Some(json!({ "recipients": ["1"] }))));
        let r = call_request(&CallCmd::SendSound { channel_id: 4, sound_id: 9, source_guild_id: Some(5) });
        assert_eq!((r.path.as_str(), r.body), ("/channels/4/send-soundboard-sound", Some(json!({ "sound_id": "9", "source_guild_id": "5" }))));
        let r = stage_request(&StageCmd::Start { guild_id: 5, channel_id: 6, topic: "Sujet".into(), notify: true });
        assert_eq!((r.path.as_str(), r.body), ("/stage-instances", Some(json!({ "channel_id": "6", "topic": "Sujet", "privacy_level": 2, "send_start_notification": true }))));
        let r = stage_request(&StageCmd::Hand { guild_id: 5, channel_id: 6, at_ms: Some(1_763_049_600_000) });
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::PATCH, "/guilds/5/voice-states/@me", Some(json!({ "channel_id": "6", "request_to_speak_timestamp": "2025-11-13T16:00:00.000Z" }))));
        let r = stage_request(&StageCmd::Hand { guild_id: 5, channel_id: 6, at_ms: None });
        assert_eq!(r.body, Some(json!({ "channel_id": "6", "request_to_speak_timestamp": null })));
        let r = stage_request(&StageCmd::Speaker { guild_id: 5, channel_id: 6, user_id: Some(7), speak: true });
        assert_eq!((r.path.as_str(), r.body), ("/guilds/5/voice-states/7", Some(json!({ "channel_id": "6", "suppress": false }))));
        let r = stage_request(&StageCmd::Status { channel_id: 6, status: None });
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::PUT, "/channels/6/voice-status", Some(json!({ "status": null }))));
        assert_eq!(stage_request(&StageCmd::End { channel_id: 6 }).method, Method::DELETE);
    }

    #[test]
    fn answers_release_the_action() {
        let ev = stage_answer(&StageCmd::End { channel_id: 6 }, "");
        assert!(matches!(ev.as_slice(), [Event::Stage(StageEvent::Delete { channel_id: 6 }), Event::Stage(StageEvent::Done { busy: (StageAction::End, 6, 0) })]));
        let ev = call_answer(&CallCmd::DefaultSounds, "pas du json");
        assert!(matches!(ev.as_slice(), [Event::Calls(CallEvent::Failed { .. })]));
    }
}
