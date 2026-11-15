//! Caméra et directs sur la passerelle principale (phase 5, lot M4) :
//! `self_video` dans l'op 4, ops 18 (Create Stream), 19 (Delete Stream),
//! 20 (Watch Stream), et événements STREAM_CREATE, STREAM_UPDATE,
//! STREAM_SERVER_UPDATE, STREAM_DELETE.
//!
//! Non documentés par Discord. Formes vérifiées le 13 novembre 2026 sur
//! docs.discord.food (Topics › Gateway Events) :
//! - op 4 Update Voice State : `self_video?` (booléen) en plus de
//!   `guild_id`, `channel_id`, `self_mute`, `self_deaf` ;
//! - op 18 Create Stream : `type` (`guild` ou `call`), `guild_id?`,
//!   `channel_id`, `preferred_region?` ;
//! - op 19 Delete Stream : `stream_key` (« se déconnecter d'un direct ; le
//!   direct est supprimé si on en est le propriétaire ») ;
//! - op 20 Watch Stream : `stream_key` ;
//! - STREAM_CREATE / STREAM_UPDATE : objet direct (`stream_key`,
//!   `rtc_server_id` et `rtc_channel_id` à la création seulement, `region`,
//!   `viewer_ids`, `paused`) ; STREAM_SERVER_UPDATE (`token`, `stream_key`,
//!   `endpoint` nul pendant une réattribution) ; STREAM_DELETE
//!   (`stream_key`, `reason`, `unavailable?`).
//!
//! Tout part après un geste (caméra, partager, regarder, arrêter), par le
//! limiteur de la passerelle (`GwSink`). Le jeton du direct n'est ni
//! journalisé ni affiché (`state::voice::Secret`).

use super::raw::{de_lenient, de_loose_id, Item};
use super::Out;
use crate::state::stream::{EndReason, StreamEvent};
use crate::state::video::VideoCmd;
use crate::state::voice::{Op4, Secret, VoiceCmd};
use crate::state::Event;
use serde::Deserialize;
use serde_json::{json, Value};

/// Identifiant seul, en chaîne ou en nombre.
#[derive(Deserialize)]
#[serde(transparent)]
struct LooseId(#[serde(deserialize_with = "de_loose_id")] Option<u64>);

#[derive(Deserialize)]
struct RawStream {
    #[serde(default, deserialize_with = "de_lenient")]
    stream_key: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    rtc_server_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    rtc_channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    region: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    viewer_ids: Option<Vec<Item<LooseId>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    paused: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    endpoint: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    token: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    reason: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    unavailable: Option<bool>,
}

impl RawStream {
    fn viewers(&mut self) -> Option<Vec<u64>> {
        self.viewer_ids.take().map(|v| v.into_iter().filter_map(|i| i.0?.0).collect())
    }
}

pub(super) fn decode(kind: &str, d: &str) -> Vec<Event> {
    if kind == "READY" {
        return vec![Event::Stream(StreamEvent::Reset)];
    }
    let Ok(mut s) = serde_json::from_str::<RawStream>(d) else { return Vec::new() };
    let Some(key) = s.stream_key.take().filter(|k| !k.is_empty()) else { return Vec::new() };
    let e = match kind {
        "STREAM_CREATE" => {
            let viewers = s.viewers().unwrap_or_default();
            StreamEvent::Create { key, rtc_server_id: s.rtc_server_id, rtc_channel_id: s.rtc_channel_id, region: s.region, viewers, paused: s.paused.unwrap_or(false) }
        }
        "STREAM_UPDATE" => {
            let viewers = s.viewers();
            StreamEvent::Update { key, region: s.region, viewers, paused: s.paused }
        }
        "STREAM_SERVER_UPDATE" => {
            let Some(token) = s.token else { return Vec::new() };
            StreamEvent::Server { key, endpoint: s.endpoint.filter(|e| !e.is_empty()), token: Secret::new(token) }
        }
        "STREAM_DELETE" => StreamEvent::Delete { key, reason: EndReason::from_wire(s.reason.as_deref(), s.unavailable == Some(true)) },
        _ => return Vec::new(),
    };
    vec![Event::Stream(e)]
}

pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    if matches!(kind, "READY" | "STREAM_CREATE" | "STREAM_UPDATE" | "STREAM_SERVER_UPDATE" | "STREAM_DELETE") {
        decode(kind, d).into_iter().for_each(|e| out.send(e));
    }
}

/// Commandes vidéo qui donnent un paquet de passerelle ; les autres sont locales.
pub(super) fn for_gateway(c: &VideoCmd) -> bool {
    !matches!(c, VideoCmd::Focus(_) | VideoCmd::PopOut(_))
}

fn id_or_null(id: Option<u64>) -> Value {
    id.map_or(Value::Null, |i| Value::String(i.to_string()))
}

/// État vidéo tenu par la passerelle, à côté de l'op 4 du vocal.
#[derive(Default)]
pub(super) struct GwVideo {
    self_video: bool,
    /// Caméra touchée dans ce salon : `self_video` accompagne alors chaque op 4.
    touched: bool,
    /// Clé de mon direct (op 18 envoyée), pour l'op 19.
    own_key: Option<String>,
}

impl GwVideo {
    /// Commande vocale : quitter le salon éteint caméra et direct (Discord les
    /// ferme aussi) ; changer de salon les garde, comme dans le magasin.
    pub(super) fn on_voice(&mut self, c: &VoiceCmd) {
        if matches!(c, VoiceCmd::Leave) {
            *self = GwVideo::default();
        }
    }

    /// Nouvelle session : Discord a tout oublié.
    pub(super) fn forget(&mut self) {
        *self = GwVideo::default();
    }

    /// Paquet de l'op 4, avec `self_video` une fois la caméra touchée.
    pub(super) fn op4(&self, p: &Op4) -> String {
        if !self.touched {
            return super::voicegw::op4(p);
        }
        let d = json!({ "guild_id": id_or_null(p.guild_id), "channel_id": id_or_null(p.channel_id), "self_mute": p.self_mute, "self_deaf": p.self_deaf, "self_video": self.self_video });
        json!({ "op": 4, "d": d }).to_string()
    }

    /// Paquets d'une commande vidéo ; `current` : mon op 4 actuelle (hors salon : rien).
    pub(super) fn on_cmd(&mut self, c: &VideoCmd, current: Option<Op4>) -> Vec<String> {
        match c {
            VideoCmd::SetCamera(on) => {
                let Some(p) = current else { return Vec::new() };
                self.self_video = *on;
                self.touched = true;
                vec![self.op4(&p)]
            }
            VideoCmd::StartShare { stream_key, guild_id, channel_id, .. } => {
                if current.is_none_or(|p| p.channel_id != Some(*channel_id)) {
                    return Vec::new();
                }
                let mut out = Vec::new();
                if let Some(old) = self.own_key.take() {
                    out.push(op19(&old));
                }
                self.own_key = Some(stream_key.clone());
                let kind = if guild_id.is_some() { "guild" } else { "call" };
                out.push(json!({ "op": 18, "d": { "type": kind, "guild_id": id_or_null(*guild_id), "channel_id": channel_id.to_string(), "preferred_region": null } }).to_string());
                out
            }
            VideoCmd::StopShare => self.own_key.take().map(|k| op19(&k)).into_iter().collect(),
            VideoCmd::Watch { stream_key } => vec![json!({ "op": 20, "d": { "stream_key": stream_key } }).to_string()],
            VideoCmd::StopWatching(k) => vec![op19(k)],
            VideoCmd::Focus(_) | VideoCmd::PopOut(_) => Vec::new(),
        }
    }
}

fn op19(key: &str) -> String {
    json!({ "op": 19, "d": { "stream_key": key } }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::Quality;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))).unwrap_or_else(|e| panic!("{path}: {e}"));
        v["d"].to_string()
    }

    const KEY: &str = "guild:2000000000000000004:3000000000000000051:1000000000000000006";

    #[test]
    fn stream_events_decode() {
        let evs = decode("STREAM_CREATE", &fixture("stream_create"));
        let [Event::Stream(StreamEvent::Create { key, rtc_server_id, rtc_channel_id, region, viewers, paused })] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!((key.as_str(), *rtc_server_id, *rtc_channel_id, region.as_deref(), viewers.as_slice(), *paused), (KEY, Some(6000000000000000001), Some(6000000000000000002), Some("rotterdam"), &[1000000000000000001][..], false));
        let evs = decode("STREAM_UPDATE", &fixture("stream_update"));
        assert!(matches!(evs.as_slice(), [Event::Stream(StreamEvent::Update { viewers: Some(v), paused: Some(true), .. })] if v.len() == 2), "{evs:?}");
        // Aucun jeton dans les jeux de données : ajouté ici.
        let mut server: Value = serde_json::from_str(&fixture("stream_server_update")).unwrap_or_default();
        server["token"] = "jeton-direct-fictif".into();
        let evs = decode("STREAM_SERVER_UPDATE", &server.to_string());
        let [Event::Stream(StreamEvent::Server { key, endpoint: Some(e), token })] = evs.as_slice() else { panic!("{evs:?}") };
        assert_eq!((key.as_str(), e.as_str(), token.expose()), (KEY, "direct-fictif.example.invalid:443", "jeton-direct-fictif"));
        assert!(!format!("{evs:?}").contains("jeton-direct-fictif"));
        let evs = decode("STREAM_DELETE", &fixture("stream_delete"));
        assert!(matches!(evs.as_slice(), [Event::Stream(StreamEvent::Delete { reason: EndReason::Full, .. })]), "{evs:?}");
        // Paquets abîmés ou incomplets : rien, sans erreur.
        assert!(decode("STREAM_CREATE", r#"{ "region": 5 }"#).is_empty());
        assert!(decode("STREAM_SERVER_UPDATE", r#"{ "stream_key": "call:1:2", "endpoint": null }"#).is_empty(), "sans jeton");
        assert!(matches!(decode("STREAM_UPDATE", r#"{ "stream_key": "call:1:2", "viewer_ids": "?", "paused": 3 }"#).as_slice(), [Event::Stream(StreamEvent::Update { viewers: None, paused: None, .. })]));
        assert!(matches!(decode("READY", "{}").as_slice(), [Event::Stream(StreamEvent::Reset)]));
    }

    #[test]
    fn camera_rides_on_op4_and_stream_ops_have_their_shape() {
        let mut g = GwVideo::default();
        let here = Op4 { guild_id: Some(5), channel_id: Some(6), self_mute: false, self_deaf: false };
        let v = |s: &str| serde_json::from_str::<Value>(s).unwrap_or_default();
        assert_eq!(v(&g.op4(&here)), json!({ "op": 4, "d": { "guild_id": "5", "channel_id": "6", "self_mute": false, "self_deaf": false } }), "caméra jamais touchée : op 4 inchangée");
        assert!(g.on_cmd(&VideoCmd::SetCamera(true), None).is_empty(), "hors salon");
        let p = g.on_cmd(&VideoCmd::SetCamera(true), Some(here));
        assert_eq!(v(&p[0])["d"]["self_video"], true);
        let p = g.on_cmd(&VideoCmd::SetCamera(false), Some(here));
        assert_eq!(v(&p[0])["d"]["self_video"], false);
        assert_eq!(v(&g.op4(&here))["d"]["self_video"], false, "suit les op 4 suivantes");
        let q = Quality { width: 1920, height: 1080, fps: 30 };
        let start = VideoCmd::StartShare { source_id: "e".into(), quality: q, audio: false, stream_key: "guild:5:6:1".into(), guild_id: Some(5), channel_id: 6 };
        let p = g.on_cmd(&start, Some(here));
        assert_eq!(v(&p[0]), json!({ "op": 18, "d": { "type": "guild", "guild_id": "5", "channel_id": "6", "preferred_region": null } }));
        // Second partage : l'ancien direct est d'abord supprimé.
        let p = g.on_cmd(&start, Some(here));
        assert_eq!(p.iter().map(|x| v(x)["op"].clone()).collect::<Vec<_>>(), vec![json!(19), json!(18)]);
        assert_eq!(v(&g.on_cmd(&VideoCmd::StopShare, Some(here))[0]), json!({ "op": 19, "d": { "stream_key": "guild:5:6:1" } }));
        assert!(g.on_cmd(&VideoCmd::StopShare, Some(here)).is_empty(), "déjà arrêté");
        let call = VideoCmd::StartShare { source_id: "e".into(), quality: q, audio: false, stream_key: "call:6:1".into(), guild_id: None, channel_id: 6 };
        let dm = Op4 { guild_id: None, ..here };
        assert_eq!(v(&g.on_cmd(&call, Some(dm))[0])["d"], json!({ "type": "call", "guild_id": null, "channel_id": "6", "preferred_region": null }));
        let elsewhere = VideoCmd::StartShare { source_id: "e".into(), quality: q, audio: false, stream_key: "guild:5:7:1".into(), guild_id: Some(5), channel_id: 7 };
        assert!(g.on_cmd(&elsewhere, Some(here)).is_empty(), "autre salon");
        assert_eq!(v(&g.on_cmd(&VideoCmd::Watch { stream_key: "call:6:2".into() }, Some(here))[0]), json!({ "op": 20, "d": { "stream_key": "call:6:2" } }));
        assert_eq!(v(&g.on_cmd(&VideoCmd::StopWatching("call:6:2".into()), None)[0]), json!({ "op": 19, "d": { "stream_key": "call:6:2" } }));
        assert!(!for_gateway(&VideoCmd::Focus(None)) && for_gateway(&VideoCmd::StopShare));
        g.on_voice(&VoiceCmd::Leave);
        assert!(!v(&g.op4(&here))["d"].as_object().is_some_and(|d| d.contains_key("self_video")), "salon quitté : caméra oubliée");
    }
}
