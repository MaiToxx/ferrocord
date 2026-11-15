//! Trames et commandes du tube local, sans entrée-sortie : chaque trame
//! reçue donne des réponses, un éventuel changement (poignée de main,
//! activité) et l'ordre de fermer. Tout ce qui vient de l'application est
//! borné et vérifié ; un champ inconnu est ignoré.

use super::MAX_FRAME;
use crate::state::rpc::{valid_asset_id, valid_asset_name, valid_mp, RpcActivity, RpcButton, MAX_LABEL, MAX_TEXT, MAX_URL};
use serde_json::{json, Map, Value};
use tokio::io::{AsyncRead, AsyncReadExt};

pub const OP_HANDSHAKE: u32 = 0;
pub const OP_FRAME: u32 = 1;
pub const OP_CLOSE: u32 = 2;
pub const OP_PING: u32 = 3;
pub const OP_PONG: u32 = 4;

/// Code d'erreur RPC : charge utile invalide.
const ERR_PAYLOAD: u64 = 4000;
/// Code d'erreur RPC : commande invalide (ici : refusée).
const ERR_COMMAND: u64 = 4002;
/// Fermeture : identifiant d'application invalide.
const CLOSE_CLIENT_ID: u64 = 4000;
/// Fermeture : version du protocole inconnue.
const CLOSE_VERSION: u64 = 4004;
/// Fermeture : trame inattendue (pas de numéro RPC pour cela ; celui de
/// WebSocket pour « données refusées »).
const CLOSE_PROTOCOL: u64 = 1003;

/// Trame reçue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub op: u32,
    pub body: Vec<u8>,
}

/// Trame à écrire : en-tête de deux u32 petit-boutistes (opcode, longueur), puis le JSON.
pub fn encode(op: u32, body: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(body.as_bytes());
    out
}

/// Lit une trame ; longueur au-delà de [`MAX_FRAME`] : erreur (la suite
/// du flux ne peut plus être lue).
pub async fn read_frame<R: AsyncRead + Unpin>(r: &mut R) -> std::io::Result<Frame> {
    let mut head = [0u8; 8];
    r.read_exact(&mut head).await?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "trame trop longue"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    Ok(Frame { op, body })
}

/// Ce qu'une trame reçue demande.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Trames à écrire, dans l'ordre.
    pub replies: Vec<Vec<u8>>,
    /// Poignée de main acceptée : application déclarée.
    pub ready: Option<u64>,
    /// `SET_ACTIVITY` accepté : `pid` donné et activité (`None` : effacée).
    pub activity: Option<(Option<u32>, Option<RpcActivity>)>,
    /// Fermer après avoir écrit les réponses.
    pub close: bool,
}

/// État d'une connexion : avant ou après la poignée de main.
#[derive(Debug, Default)]
pub struct Proto {
    app_id: Option<u64>,
}

fn close_frame(code: u64, message: &str) -> Vec<u8> {
    encode(OP_CLOSE, &json!({ "code": code, "message": message }).to_string())
}

fn closing(code: u64, message: &str) -> Outcome {
    Outcome { replies: vec![close_frame(code, message)], close: true, ..Default::default() }
}

fn error_frame(cmd: &Value, nonce: &Value, code: u64, message: &str) -> Vec<u8> {
    encode(OP_FRAME, &json!({ "cmd": cmd, "evt": "ERROR", "data": { "code": code, "message": message }, "nonce": nonce }).to_string())
}

/// Identifiant d'application : chaîne de chiffres (ou nombre), non nul.
fn snowflake(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) if !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit()) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
    .filter(|id| *id != 0)
}

/// READY, après la poignée de main. Aucune donnée du compte n'est donnée
/// aux programmes locaux : utilisateur neutre.
fn ready_frame() -> Vec<u8> {
    let v = json!({
        "cmd": "DISPATCH",
        "evt": "READY",
        "nonce": null,
        "data": {
            "v": 1,
            "config": { "cdn_host": "cdn.discordapp.com", "api_endpoint": "//discord.com/api", "environment": "production" },
            "user": { "id": "0", "username": "ferrocord", "discriminator": "0", "global_name": null, "avatar": null, "bot": false, "flags": 0, "premium_type": 0 }
        }
    });
    encode(OP_FRAME, &v.to_string())
}

impl Proto {
    pub fn ready(&self) -> bool {
        self.app_id.is_some()
    }

    pub fn handle(&mut self, f: &Frame) -> Outcome {
        match (f.op, self.app_id) {
            (OP_HANDSHAKE, None) => self.handshake(&f.body),
            (OP_HANDSHAKE, Some(_)) => closing(CLOSE_PROTOCOL, "Already connected"),
            (OP_PING, _) => Outcome { replies: vec![encode(OP_PONG, &String::from_utf8_lossy(&f.body))], ..Default::default() },
            (OP_PONG, _) => Outcome::default(),
            (OP_CLOSE, _) => Outcome { close: true, ..Default::default() },
            (OP_FRAME, Some(app_id)) => command(app_id, &f.body),
            _ => closing(CLOSE_PROTOCOL, "Handshake required"),
        }
    }

    fn handshake(&mut self, body: &[u8]) -> Outcome {
        let Ok(v) = serde_json::from_slice::<Value>(body) else { return closing(CLOSE_PROTOCOL, "Invalid handshake") };
        let version = match &v["v"] {
            Value::Number(n) => n.as_u64(),
            Value::String(s) => s.parse().ok(),
            _ => None,
        };
        if version != Some(1) {
            return closing(CLOSE_VERSION, "Invalid version");
        }
        let Some(app_id) = snowflake(&v["client_id"]) else { return closing(CLOSE_CLIENT_ID, "Invalid client ID") };
        self.app_id = Some(app_id);
        Outcome { replies: vec![ready_frame()], ready: Some(app_id), ..Default::default() }
    }
}

/// Une commande après la poignée de main : seule `SET_ACTIVITY` est acceptée.
fn command(app_id: u64, body: &[u8]) -> Outcome {
    let Ok(v) = serde_json::from_slice::<Value>(body) else {
        return Outcome { replies: vec![error_frame(&Value::Null, &Value::Null, ERR_PAYLOAD, "Invalid payload")], ..Default::default() };
    };
    let cmd = v.get("cmd").cloned().unwrap_or(Value::Null);
    let nonce = v.get("nonce").cloned().unwrap_or(Value::Null);
    if cmd.as_str() != Some("SET_ACTIVITY") {
        let msg = "Not supported by Ferrocord: only SET_ACTIVITY is accepted";
        return Outcome { replies: vec![error_frame(&cmd, &nonce, ERR_COMMAND, msg)], ..Default::default() };
    }
    let args = v.get("args").unwrap_or(&Value::Null);
    let pid = args.get("pid").and_then(Value::as_u64).and_then(|p| u32::try_from(p).ok()).filter(|p| *p != 0);
    let activity = match args.get("activity") {
        None | Some(Value::Null) => None,
        Some(Value::Object(a)) => Some(sanitize(a)),
        Some(_) => return Outcome { replies: vec![error_frame(&cmd, &nonce, ERR_PAYLOAD, "Invalid activity")], ..Default::default() },
    };
    let data = activity.as_ref().map_or(Value::Null, |a| activity_json(app_id, a));
    let reply = encode(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "evt": null, "data": data, "nonce": nonce }).to_string());
    Outcome { replies: vec![reply], activity: Some((pid, activity)), ..Default::default() }
}

/// Texte borné, sans caractères de contrôle ; moins de 2 caractères
/// (minimum de Discord) : absent.
fn text(v: Option<&Value>, max: usize) -> Option<String> {
    let s: String = v?.as_str()?.trim().chars().filter(|c| !c.is_control()).take(max).collect();
    let s = s.trim().to_string();
    (s.chars().count() >= 2).then_some(s)
}

/// Horodatage en ms Unix ; des secondes (le SDK) sont converties.
fn stamp(v: Option<&Value>) -> Option<i64> {
    let n = v?;
    let raw = n.as_i64().or_else(|| n.as_f64().filter(|f| f.is_finite() && f.abs() < 1e15).map(|f| f as i64))?;
    // En dessous de 10^11, ce ne peut pas être des ms récentes (1973).
    let ms = if raw < 100_000_000_000 { raw.checked_mul(1000)? } else { raw };
    // Après l'an 3000 : refusé.
    (ms > 0 && ms < 32_503_680_000_000).then_some(ms)
}

/// Clé d'image gardée : identifiant numérique, image du proxy (`mp:`), ou
/// nom d'une image de l'application (traduit plus tard par la liste de ses
/// images, `state::rpc`). Les adresses demanderaient `external-assets` : ignorées.
fn image_key(v: Option<&Value>) -> Option<String> {
    let k = v?.as_str()?.trim();
    let ok = valid_asset_id(k) || valid_asset_name(k) || k.strip_prefix("mp:").is_some_and(valid_mp);
    ok.then(|| k.to_string())
}

fn button(v: &Value) -> Option<RpcButton> {
    let label: String = v.get("label")?.as_str()?.trim().chars().filter(|c| !c.is_control()).take(MAX_LABEL).collect();
    let url = v.get("url")?.as_str()?.trim();
    let ok = (url.starts_with("https://") || url.starts_with("http://")) && url.len() <= MAX_URL && !url.chars().any(|c| c.is_control() || c.is_whitespace());
    (!label.trim().is_empty() && ok).then(|| RpcButton { label: label.trim().to_string(), url: url.to_string() })
}

/// Activité nettoyée ; ce qui ne passe pas est retiré, sans faire échouer le reste.
pub(super) fn sanitize(a: &Map<String, Value>) -> RpcActivity {
    let kind = a.get("type").and_then(Value::as_u64).filter(|k| matches!(k, 0 | 2 | 3 | 5)).unwrap_or(0) as u8;
    let ts = a.get("timestamps");
    let assets = a.get("assets");
    let party = a.get("party");
    let party_size = party.and_then(|p| p.get("size")).and_then(Value::as_array).and_then(|s| {
        let cur = u32::try_from(s.first()?.as_u64()?).ok()?;
        let max = u32::try_from(s.get(1)?.as_u64()?).ok()?;
        (cur > 0 && cur <= max).then_some((cur, max))
    });
    let party_id = party.and_then(|p| p.get("id")).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty() && s.len() <= MAX_TEXT && !s.chars().any(char::is_control)).map(str::to_string);
    let buttons = a.get("buttons").and_then(Value::as_array).map(|b| b.iter().filter_map(button).take(2).collect()).unwrap_or_default();
    let start_ms = stamp(ts.and_then(|t| t.get("start")));
    let end_ms = stamp(ts.and_then(|t| t.get("end"))).filter(|e| start_ms.is_none_or(|s| *e > s));
    RpcActivity {
        name: a.get("name").and_then(Value::as_str).map(|s| s.trim().chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()).unwrap_or_default(),
        kind,
        details: text(a.get("details"), MAX_TEXT),
        state: text(a.get("state"), MAX_TEXT),
        start_ms,
        end_ms,
        large_image: image_key(assets.and_then(|x| x.get("large_image"))),
        large_text: text(assets.and_then(|x| x.get("large_text")), MAX_TEXT),
        small_image: image_key(assets.and_then(|x| x.get("small_image"))),
        small_text: text(assets.and_then(|x| x.get("small_text")), MAX_TEXT),
        party_id,
        party_size,
        buttons,
    }
}

/// L'activité dans la forme de la passerelle (docs.discord.food,
/// « Presence » › Activity Object : `buttons` = libellés, adresses dans
/// `metadata.button_urls`). Sert à l'op 3 et à la réponse à `SET_ACTIVITY`.
pub fn activity_json(app_id: u64, a: &RpcActivity) -> Value {
    let mut v = json!({ "name": a.name, "type": a.kind, "application_id": app_id.to_string() });
    let mut put = |k: &str, x: &Option<String>| {
        if let Some(s) = x {
            v[k] = s.clone().into();
        }
    };
    put("details", &a.details);
    put("state", &a.state);
    let mut ts = Map::new();
    if let Some(s) = a.start_ms {
        ts.insert("start".into(), s.into());
    }
    if let Some(e) = a.end_ms {
        ts.insert("end".into(), e.into());
    }
    if !ts.is_empty() {
        v["timestamps"] = Value::Object(ts);
    }
    let mut assets = Map::new();
    for (k, x) in [("large_image", &a.large_image), ("large_text", &a.large_text), ("small_image", &a.small_image), ("small_text", &a.small_text)] {
        if let Some(s) = x {
            assets.insert(k.into(), s.clone().into());
        }
    }
    if !assets.is_empty() {
        v["assets"] = Value::Object(assets);
    }
    let mut party = Map::new();
    if let Some(id) = &a.party_id {
        party.insert("id".into(), id.clone().into());
    }
    if let Some((cur, max)) = a.party_size {
        party.insert("size".into(), json!([cur, max]));
    }
    if !party.is_empty() {
        v["party"] = Value::Object(party);
    }
    if !a.buttons.is_empty() {
        v["buttons"] = a.buttons.iter().map(|b| b.label.clone()).collect::<Vec<_>>().into();
        v["metadata"] = json!({ "button_urls": a.buttons.iter().map(|b| b.url.clone()).collect::<Vec<_>>() });
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    fn frame(op: u32, v: &Value) -> Frame {
        Frame { op, body: v.to_string().into_bytes() }
    }

    /// Trame écrite → (opcode, JSON).
    fn decoded(bytes: &[u8]) -> (u32, Value) {
        let op = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
        assert_eq!(bytes.len(), 8 + len);
        (op, serde_json::from_slice(&bytes[8..]).unwrap_or(Value::Null))
    }

    fn ready() -> Proto {
        let mut p = Proto::default();
        let o = p.handle(&frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "6000000000000000040" })));
        assert_eq!(o.ready, Some(6000000000000000040));
        p
    }

    #[tokio::test]
    async fn frames_round_trip_and_are_bounded() {
        let bytes = encode(OP_FRAME, "{\"a\":1}");
        assert_eq!(&bytes[..8], &[1, 0, 0, 0, 7, 0, 0, 0]);
        let f = read_frame(&mut &bytes[..]).await.unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(f, Frame { op: OP_FRAME, body: b"{\"a\":1}".to_vec() });
        let mut big = OP_FRAME.to_le_bytes().to_vec();
        big.extend_from_slice(&((MAX_FRAME + 1) as u32).to_le_bytes());
        assert!(read_frame(&mut &big[..]).await.is_err(), "trop longue");
        assert!(read_frame(&mut &bytes[..5]).await.is_err(), "coupée");
    }

    #[test]
    fn handshake_then_ready() {
        let mut p = Proto::default();
        let o = p.handle(&frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "6000000000000000040" })));
        let (op, v) = decoded(&o.replies[0]);
        assert_eq!((op, v["cmd"].as_str(), v["evt"].as_str(), v["data"]["v"].as_u64()), (OP_FRAME, Some("DISPATCH"), Some("READY"), Some(1)));
        assert_eq!(v["data"]["user"]["id"], "0", "aucune donnée du compte");
        assert!(p.ready() && !o.close);
        let again = p.handle(&frame(OP_HANDSHAKE, &json!({ "v": 1, "client_id": "1" })));
        assert!(again.close && again.ready.is_none(), "une seule poignée de main");
    }

    #[test]
    fn bad_handshakes_close() {
        for (body, code) in [
            (json!({ "v": 2, "client_id": "1" }), CLOSE_VERSION),
            (json!({ "v": 1 }), CLOSE_CLIENT_ID),
            (json!({ "v": 1, "client_id": "abc" }), CLOSE_CLIENT_ID),
            (json!({ "v": 1, "client_id": "0" }), CLOSE_CLIENT_ID),
        ] {
            let o = Proto::default().handle(&frame(OP_HANDSHAKE, &body));
            assert!(o.close && o.ready.is_none(), "{body}");
            let (op, v) = decoded(&o.replies[0]);
            assert_eq!((op, v["code"].as_u64()), (OP_CLOSE, Some(code)), "{body}");
        }
        let o = Proto::default().handle(&Frame { op: OP_HANDSHAKE, body: b"pas du json".to_vec() });
        assert!(o.close);
        let o = Proto::default().handle(&frame(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY" })));
        assert!(o.close && o.activity.is_none(), "commande avant la poignée de main");
        assert!(Proto::default().handle(&Frame { op: 99, body: Vec::new() }).close);
    }

    #[test]
    fn ping_pong_and_close() {
        let mut p = ready();
        let o = p.handle(&Frame { op: OP_PING, body: b"{\"x\":1}".to_vec() });
        assert_eq!(decoded(&o.replies[0]), (OP_PONG, json!({ "x": 1 })));
        assert_eq!(p.handle(&Frame { op: OP_PONG, body: Vec::new() }), Outcome::default());
        assert!(p.handle(&Frame { op: OP_CLOSE, body: Vec::new() }).close);
    }

    #[test]
    fn set_activity_fixture() {
        let mut p = ready();
        let v: Value = serde_json::from_str(&fixture("rpc/set_activity")).unwrap_or(Value::Null);
        let o = p.handle(&frame(OP_FRAME, &v));
        let (pid, a) = o.activity.unwrap_or_else(|| panic!("acceptée"));
        let a = a.unwrap_or_else(|| panic!("activité"));
        assert_eq!(pid, Some(4242));
        assert_eq!(a.details.as_deref(), Some("Donjon des Brumes"));
        assert_eq!(a.state.as_deref(), Some("En groupe"));
        assert_eq!((a.start_ms, a.end_ms), (Some(1_700_000_000_000), Some(1_700_000_900_000)), "secondes converties");
        assert_eq!(a.large_image.as_deref(), Some("6000000000000000041"));
        assert_eq!(a.large_text.as_deref(), Some("Carte du monde"));
        assert_eq!(a.small_image.as_deref(), Some("niveau"), "nom d'image gardé : traduit plus tard par la liste des images");
        assert_eq!(a.small_text.as_deref(), Some("Niveau 12"));
        assert_eq!((a.party_id.as_deref(), a.party_size), (Some("groupe-1"), Some((2, 4))));
        assert_eq!(a.buttons, [RpcButton { label: "Site du jeu".into(), url: "https://exemple.invalid/jeu".into() }], "bouton javascript: retiré");
        let (op, reply) = decoded(&o.replies[0]);
        assert_eq!((op, reply["cmd"].as_str(), &reply["evt"], reply["nonce"].as_str()), (OP_FRAME, Some("SET_ACTIVITY"), &Value::Null, Some("1")));
        assert_eq!(reply["data"]["application_id"], "6000000000000000040");
        let text = String::from_utf8_lossy(&o.replies[0]).to_string();
        assert!(!text.contains("secret"), "secrets jamais repris : {text}");
    }

    #[test]
    fn clear_and_refused_commands() {
        let mut p = ready();
        let o = p.handle(&frame(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "args": { "pid": 7 }, "nonce": "2" })));
        assert_eq!(o.activity, Some((Some(7), None)), "sans activité : effacée");
        for cmd in ["AUTHORIZE", "AUTHENTICATE", "SUBSCRIBE", "SEND_ACTIVITY_JOIN_INVITE", "GET_GUILDS"] {
            let o = p.handle(&frame(OP_FRAME, &json!({ "cmd": cmd, "args": {}, "nonce": "n" })));
            assert!(o.activity.is_none() && !o.close, "{cmd}");
            let (_, v) = decoded(&o.replies[0]);
            assert_eq!((v["cmd"].as_str(), v["evt"].as_str(), v["data"]["code"].as_u64(), v["nonce"].as_str()), (Some(cmd), Some("ERROR"), Some(ERR_COMMAND), Some("n")));
        }
        let o = p.handle(&frame(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY", "args": { "activity": "texte" } })));
        assert!(o.activity.is_none());
        assert_eq!(decoded(&o.replies[0]).1["data"]["code"].as_u64(), Some(ERR_PAYLOAD));
        let o = p.handle(&Frame { op: OP_FRAME, body: b"{ illisible".to_vec() });
        assert!(o.activity.is_none() && !o.close);
    }

    #[test]
    fn sanitize_bounds_everything() {
        let long = "x".repeat(500);
        let a = json!({
            "type": 4, "details": long, "state": "a",
            "timestamps": { "start": -5, "end": "demain" },
            "assets": { "large_image": "https://exemple.invalid/a.png", "small_image": "mp:../x" },
            "party": { "size": [5, 2] },
            "buttons": [{ "label": "A", "url": "https://a.invalid" }, { "label": "B", "url": "http://b.invalid" }, { "label": "C", "url": "https://c.invalid" }],
            "inconnu": true
        });
        let s = sanitize(a.as_object().unwrap_or(&Map::new()));
        assert_eq!(s.kind, 0, "statut personnalisé refusé");
        assert_eq!(s.details.map(|d| d.chars().count()), Some(MAX_TEXT));
        assert_eq!(s.state, None, "un caractère : trop court");
        assert_eq!((s.start_ms, s.end_ms), (None, None));
        assert_eq!((s.large_image, s.small_image), (None, None));
        assert_eq!(s.party_size, None);
        assert_eq!(s.buttons.len(), 2);
        let ms = sanitize(json!({ "timestamps": { "start": 1_700_000_000_000_i64, "end": 1_699_000_000_000_i64 } }).as_object().unwrap_or(&Map::new()));
        assert_eq!((ms.start_ms, ms.end_ms), (Some(1_700_000_000_000), None), "ms gardées ; fin avant le début : retirée");
    }

    #[test]
    fn gateway_shape() {
        let a = RpcActivity {
            name: "Jeu".into(),
            kind: 0,
            details: Some("D".repeat(3)),
            start_ms: Some(5),
            large_image: Some("1".into()),
            party_size: Some((1, 3)),
            buttons: vec![RpcButton { label: "Site".into(), url: "https://s.invalid".into() }],
            ..Default::default()
        };
        assert_eq!(
            activity_json(9, &a),
            json!({ "name": "Jeu", "type": 0, "application_id": "9", "details": "DDD", "timestamps": { "start": 5 },
                    "assets": { "large_image": "1" }, "party": { "size": [1, 3] },
                    "buttons": ["Site"], "metadata": { "button_urls": ["https://s.invalid"] } })
        );
    }
}
