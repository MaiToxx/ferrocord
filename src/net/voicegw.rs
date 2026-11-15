//! Vocal sur la passerelle principale et modération vocale (phase 4, lot
//! V4) : salons vocaux et états vocaux reçus (READY, GUILD_CREATE,
//! CHANNEL_*, VOICE_STATE_UPDATE, VOICE_SERVER_UPDATE), op 4 envoyée, et
//! `PATCH /guilds/{g}/members/{u}` (`mute`, `deaf`, `channel_id`).
//!
//! Tout est documenté par Discord (vérifié le 13 novembre 2026,
//! discord.com/developers/docs) : Topics › Gateway Events (Update Voice
//! State, op 4 : `guild_id`, `channel_id`, `self_mute`, `self_deaf` ;
//! Voice State Update ; Voice Server Update : `token`, `guild_id`,
//! `endpoint` nul), Resources › Voice (Voice State Object), Resources ›
//! Guild (Modify Guild Member : `mute` exige MUTE_MEMBERS, `deaf`
//! DEAFEN_MEMBERS, `channel_id` MOVE_MEMBERS et CONNECT dans la
//! destination, nul pour déconnecter ; en-tête `X-Audit-Log-Reason`),
//! Resources › Channel (`user_limit`, types 2 et 13). Le jeton du serveur
//! vocal n'est ni journalisé ni affiché (`state::voice::Secret`).

use super::raw::{de_lenient, de_loose_id, Item, RawOverwrite, RawUser};
use super::ratelimit::Route;
use super::rest::{api_req, ApiError, Ctx};
use super::Out;
use crate::state::voice::{Op4, Secret, VoiceChannel, VoiceCmd, VoiceEvent, VoiceMod, VoiceState};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::sync::Arc;
use std::time::Duration;

/// Attente maximale avant de partir : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(15);

const VOICE: u64 = 2;
const STAGE: u64 = 13;

#[derive(Deserialize)]
struct RawVoiceChannel {
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
    #[serde(default, deserialize_with = "de_loose_id")]
    parent_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    permission_overwrites: Option<Vec<Item<RawOverwrite>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    user_limit: Option<u32>,
}

impl RawVoiceChannel {
    fn voice(self, guild: Option<u64>) -> Option<VoiceChannel> {
        let kind = self.kind?;
        if kind != VOICE && kind != STAGE {
            return None;
        }
        // Une seule dérogation illisible : toutes inconnues (le salon reste visible).
        let overwrites = self.permission_overwrites.and_then(|v| v.into_iter().map(|o| o.0.and_then(RawOverwrite::into_overwrite)).collect());
        Some(VoiceChannel {
            id: self.id?,
            guild_id: self.guild_id.or(guild)?,
            name: self.name.unwrap_or_default(),
            stage: kind == STAGE,
            position: self.position.unwrap_or(0),
            parent: self.parent_id,
            overwrites,
            user_limit: self.user_limit.unwrap_or(0),
        })
    }
}

#[derive(Deserialize)]
struct RawVoiceMember {
    #[serde(default, deserialize_with = "de_lenient")]
    nick: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawUser>,
}

#[derive(Deserialize)]
pub(super) struct RawVoiceState {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    member: Option<RawVoiceMember>,
    #[serde(default, deserialize_with = "de_lenient")]
    session_id: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    deaf: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    mute: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    self_deaf: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    self_mute: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    self_stream: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    self_video: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    suppress: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    request_to_speak_timestamp: Option<String>,
}

impl RawVoiceState {
    /// État ; `None` si la personne n'est dans aucun salon (ou paquet sans utilisateur).
    pub(super) fn state(&self, guild: Option<u64>) -> Option<VoiceState> {
        let user = self.member.as_ref().and_then(|m| m.user.as_ref());
        let name = self.member.as_ref().and_then(|m| m.nick.clone().filter(|n| !n.is_empty())).or_else(|| user.map(RawUser::display));
        Some(VoiceState {
            user_id: self.user_id.or(user.map(|u| u.id))?,
            guild_id: self.guild_id.or(guild),
            channel_id: self.channel_id?,
            self_mute: self.self_mute.unwrap_or(false),
            self_deaf: self.self_deaf.unwrap_or(false),
            mute: self.mute.unwrap_or(false),
            deaf: self.deaf.unwrap_or(false),
            suppress: self.suppress.unwrap_or(false),
            self_video: self.self_video.unwrap_or(false),
            self_stream: self.self_stream.unwrap_or(false),
            request_to_speak: self.request_to_speak_timestamp.as_deref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis()),
            name,
            avatar: user.map(RawUser::avatar),
        })
    }
}

/// Ce que ce lot lit d'un serveur ; le reste du paquet est sauté.
#[derive(Deserialize)]
struct RawGuildPart {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    unavailable: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    channels: Option<Vec<Item<RawVoiceChannel>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    voice_states: Option<Vec<Item<RawVoiceState>>>,
}

impl RawGuildPart {
    fn event(self) -> Option<Event> {
        let guild_id = self.id.filter(|_| self.unavailable != Some(true))?;
        let channels = self.channels.unwrap_or_default().into_iter().filter_map(|c| c.0?.voice(Some(guild_id))).collect();
        let states = self.voice_states.unwrap_or_default().into_iter().filter_map(|s| s.0?.state(Some(guild_id))).collect();
        Some(Event::Voice(VoiceEvent::Guild { guild_id, channels, states }))
    }
}

#[derive(Deserialize)]
struct RawReady {
    #[serde(default)]
    guilds: Vec<Item<RawGuildPart>>,
}

#[derive(Deserialize)]
struct RawServer {
    #[serde(default, deserialize_with = "de_lenient")]
    token: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    endpoint: Option<String>,
}

/// Paquets de la passerelle qui intéressent ce lot.
pub(super) fn decode(kind: &str, d: &str) -> Vec<Event> {
    let mut out = Vec::new();
    match kind {
        "READY" => {
            out.push(Event::Voice(VoiceEvent::Reset));
            if let Ok(r) = serde_json::from_str::<RawReady>(d) {
                out.extend(r.guilds.into_iter().filter_map(|g| g.0?.event()));
            }
        }
        "GUILD_CREATE" => {
            if let Some(e) = serde_json::from_str::<RawGuildPart>(d).ok().and_then(RawGuildPart::event) {
                out.push(e);
            }
        }
        "GUILD_DELETE" => {
            // Panne passagère (`unavailable`) : le serveur reste, comme dans le magasin.
            if let Ok(g) = serde_json::from_str::<RawGuildPart>(d)
                && g.unavailable != Some(true)
                && let Some(id) = g.id
            {
                out.push(Event::Voice(VoiceEvent::GuildGone(id)));
            }
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            if let Some(c) = serde_json::from_str::<RawVoiceChannel>(d).ok().and_then(|c| c.voice(None)) {
                out.push(Event::Voice(VoiceEvent::ChannelUpsert(c)));
            }
        }
        "CHANNEL_DELETE" => {
            if let Ok(c) = serde_json::from_str::<RawVoiceChannel>(d)
                && let (Some(guild_id), Some(id), Some(VOICE | STAGE)) = (c.guild_id, c.id, c.kind)
            {
                out.push(Event::Voice(VoiceEvent::ChannelDelete { guild_id, id }));
            }
        }
        "VOICE_STATE_UPDATE" => {
            if let Ok(s) = serde_json::from_str::<RawVoiceState>(d)
                && let Some(user_id) = s.user_id.or(s.member.as_ref().and_then(|m| m.user.as_ref()).map(|u| u.id))
            {
                out.push(Event::Voice(VoiceEvent::StateUpdate { guild_id: s.guild_id, user_id, state: s.state(None), session_id: s.session_id.clone() }));
            }
        }
        "VOICE_SERVER_UPDATE" => {
            if let Ok(s) = serde_json::from_str::<RawServer>(d)
                && let Some(token) = s.token
            {
                out.push(Event::Voice(VoiceEvent::Server { guild_id: s.guild_id, endpoint: s.endpoint.filter(|e| !e.is_empty()), token: Secret::new(token) }));
            }
        }
        _ => {}
    }
    out
}

pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    if matches!(kind, "READY" | "GUILD_CREATE" | "GUILD_DELETE" | "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "CHANNEL_DELETE" | "VOICE_STATE_UPDATE" | "VOICE_SERVER_UPDATE") {
        decode(kind, d).into_iter().for_each(|e| out.send(e));
    }
}

fn id_or_null(id: Option<u64>) -> Value {
    id.map_or(Value::Null, |i| Value::String(i.to_string()))
}

/// Paquet de l'op 4 (Update Voice State).
pub(super) fn op4(p: &Op4) -> String {
    json!({ "op": 4, "d": { "guild_id": id_or_null(p.guild_id), "channel_id": id_or_null(p.channel_id), "self_mute": p.self_mute, "self_deaf": p.self_deaf } }).to_string()
}

/// Commandes qui changent mon état vocal sur la passerelle (op 4).
pub(super) fn for_gateway(c: &VoiceCmd) -> bool {
    matches!(c, VoiceCmd::Join { .. } | VoiceCmd::Leave | VoiceCmd::SetMute(_) | VoiceCmd::SetDeaf(_))
}

/// Corps de Modify Guild Member pour ce geste.
fn mod_body(action: VoiceMod) -> Value {
    let mut m = Map::new();
    match action {
        VoiceMod::Mute(b) => m.insert("mute".into(), Value::Bool(b)),
        VoiceMod::Deafen(b) => m.insert("deaf".into(), Value::Bool(b)),
        VoiceMod::Move(c) => m.insert("channel_id".into(), Value::String(c.to_string())),
        VoiceMod::Disconnect => m.insert("channel_id".into(), Value::Null),
    };
    Value::Object(m)
}

/// Modération vocale ; les autres commandes vocales ne passent pas par REST.
pub(super) async fn run(ctx: Arc<Ctx>, cmd: VoiceCmd) {
    let VoiceCmd::Moderate { guild_id, user_id, action, reason } = cmd else { return };
    let url = format!("{}/guilds/{guild_id}/members/{user_id}", ctx.api_root);
    let body = mod_body(action);
    let header = reason.as_deref().map(super::admin::moderation::encode_reason);
    let build = |b: reqwest::RequestBuilder| {
        let b = match &header {
            Some(h) => b.header("X-Audit-Log-Reason", h.as_str()),
            None => b,
        };
        b.json(&body)
    };
    let route = Route { template: "PATCH /guilds/{}/members/{}", major: guild_id };
    let failed = |problem: Problem| Event::Voice(VoiceEvent::ModFailed { guild_id, user_id, action, problem });
    let ev = match api_req(&ctx, route, Method::PATCH, &url, build, MAX_WAIT, std::future::pending()).await {
        Ok(_) => Event::Voice(VoiceEvent::Moderated { guild_id, user_id, action }),
        Err(ApiError::NotFound(p)) => failed(p),
        Err(e) => e.into_event(failed),
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap_or_else(|e| panic!("{path}: {e}"));
        v["d"].to_string()
    }

    #[test]
    fn guild_create_gives_voice_channels_and_states() {
        let evs = decode("GUILD_CREATE", &fixture("guild_create_voice"));
        let [Event::Voice(VoiceEvent::Guild { guild_id, channels, states })] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!(*guild_id, 2000000000000000004);
        let mut seen: Vec<_> = channels.iter().map(|c| (c.position, c.name.as_str(), c.stage, c.user_limit)).collect();
        seen.sort();
        assert_eq!(seen, vec![(1, "Salon vocal", false, 0), (2, "Petit salon", false, 2), (3, "Scène", true, 10000)]);
        assert!(channels.iter().all(|c| c.overwrites.is_some()));
        assert_eq!(states.len(), 4);
        let dana = states.iter().find(|s| s.user_id == 1000000000000000005).unwrap_or_else(|| panic!("Dana"));
        assert!(dana.self_mute && dana.self_video && !dana.deaf);
        assert_eq!(dana.name.as_deref(), Some("Dana"));
        let eve = states.iter().find(|s| s.user_id == 1000000000000000006).unwrap_or_else(|| panic!("Ève"));
        assert!(eve.mute && eve.self_stream);
        assert!(eve.name.is_none(), "état partiel : nom pris ailleurs");
    }

    #[test]
    fn voice_state_and_server_updates() {
        let evs = decode("VOICE_STATE_UPDATE", &fixture("voice_state_update"));
        let [Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(_), user_id, state: Some(s), session_id: Some(sess) })] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!((*user_id, s.channel_id, sess.as_str()), (1000000000000000001, 3000000000000000051, "session-vocale-fictive"));
        assert_eq!(s.name.as_deref(), Some("Moi Fictif"));
        assert!(s.request_to_speak.is_none());
        // Départ : salon nul.
        let left = r#"{ "guild_id": "2000000000000000004", "channel_id": null, "user_id": "1000000000000000005", "session_id": "x", "deaf": false, "mute": false, "self_deaf": false, "self_mute": false, "self_video": false, "suppress": false, "request_to_speak_timestamp": null }"#;
        assert!(matches!(decode("VOICE_STATE_UPDATE", left).as_slice(), [Event::Voice(VoiceEvent::StateUpdate { state: None, user_id: 1000000000000000005, .. })]));
        // Aucun jeton dans les jeux de données : ajouté ici.
        let mut server: Value = serde_json::from_str(&fixture("voice_server_update")).unwrap_or_default();
        server["token"] = "jeton-vocal-fictif".into();
        let evs = decode("VOICE_SERVER_UPDATE", &server.to_string());
        let [Event::Voice(VoiceEvent::Server { guild_id: Some(2000000000000000004), endpoint: Some(e), token })] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!(e, "voix-fictive.example.invalid:443");
        assert_eq!(token.expose(), "jeton-vocal-fictif");
        assert!(!format!("{evs:?}").contains("jeton-vocal-fictif"));
        // Serveur vocal tombé : `endpoint` nul.
        let down = r#"{ "token": "t", "guild_id": "1", "endpoint": null }"#;
        assert!(matches!(decode("VOICE_SERVER_UPDATE", down).as_slice(), [Event::Voice(VoiceEvent::Server { endpoint: None, .. })]));
    }

    #[test]
    fn ready_resets_then_lists_guilds_and_channels_follow_updates() {
        let mut ready: Value = serde_json::from_str(&fixture("ready_user_classic")).unwrap_or_default();
        if let Some(g) = ready["guilds"].as_array_mut() {
            g.push(serde_json::from_str(&fixture("guild_create_voice")).unwrap_or_default());
        }
        let evs = decode("READY", &ready.to_string());
        assert!(matches!(evs.first(), Some(Event::Voice(VoiceEvent::Reset))));
        let alpha = evs.iter().find_map(|e| match e {
            Event::Voice(VoiceEvent::Guild { guild_id: 2000000000000000001, channels, .. }) => Some(channels),
            _ => None,
        });
        assert_eq!(alpha.map(|c| c.iter().map(|c| c.id).collect::<Vec<_>>()), Some(vec![3000000000000000014]));
        let text = r#"{ "id": "9", "guild_id": "1", "type": 0, "name": "texte" }"#;
        assert!(decode("CHANNEL_CREATE", text).is_empty());
        let voice = r#"{ "id": "9", "guild_id": "1", "type": 13, "name": "Scène", "permission_overwrites": [{ "id": "1", "type": "étrange", "allow": "0", "deny": "0" }] }"#;
        let evs = decode("CHANNEL_UPDATE", voice);
        assert!(matches!(evs.as_slice(), [Event::Voice(VoiceEvent::ChannelUpsert(VoiceChannel { stage: true, overwrites: None, .. }))]), "{evs:?}");
        assert!(matches!(decode("CHANNEL_DELETE", r#"{ "id": "9", "guild_id": "1", "type": 2 }"#).as_slice(), [Event::Voice(VoiceEvent::ChannelDelete { guild_id: 1, id: 9 })]));
        assert!(matches!(decode("GUILD_DELETE", r#"{ "id": "1" }"#).as_slice(), [Event::Voice(VoiceEvent::GuildGone(1))]));
        assert!(decode("GUILD_DELETE", r#"{ "id": "1", "unavailable": true }"#).is_empty());
    }

    #[test]
    fn op4_and_moderation_bodies() {
        let p = Op4 { guild_id: Some(5), channel_id: None, self_mute: true, self_deaf: false };
        let v: Value = serde_json::from_str(&op4(&p)).unwrap_or_default();
        assert_eq!(v, json!({ "op": 4, "d": { "guild_id": "5", "channel_id": null, "self_mute": true, "self_deaf": false } }));
        assert_eq!(mod_body(VoiceMod::Mute(true)), json!({ "mute": true }));
        assert_eq!(mod_body(VoiceMod::Deafen(false)), json!({ "deaf": false }));
        assert_eq!(mod_body(VoiceMod::Move(7)), json!({ "channel_id": "7" }));
        assert_eq!(mod_body(VoiceMod::Disconnect), json!({ "channel_id": null }));
        assert!(for_gateway(&VoiceCmd::Leave) && !for_gateway(&VoiceCmd::SetPtt(true)));
    }
}
