//! Directs (partage d'écran « en direct », phase 5, lot M4) : clés des
//! directs, état de chaque connexion de direct (le mien, ceux que je
//! regarde), spectateurs. Aucune connexion vidéo ici : le lot de câblage
//! (M5) lit [`StreamConnState::Connecting`] pour ouvrir la connexion vocale
//! du direct, et remplit `Connected`, `Failed`, `Closed`.
//!
//! Formes non documentées par Discord, vérifiées le 13 novembre 2026 sur
//! docs.discord.food (Topics › Gateway Events) : clé de direct
//! `guild:<serveur>:<salon>:<propriétaire>` ou `call:<salon>:<propriétaire>` ;
//! STREAM_CREATE et STREAM_UPDATE (`stream_key`, `rtc_server_id` et
//! `rtc_channel_id` à la création seulement, `region`, `viewer_ids`,
//! `paused`) ; STREAM_SERVER_UPDATE (`stream_key`, `endpoint` nul quand le
//! serveur est réattribué, `token`) ; STREAM_DELETE (`stream_key`,
//! `reason`, `unavailable`).

use super::voice::Secret;
use std::collections::HashMap;

/// Clé d'un direct, comme Discord la forme.
pub fn stream_key(guild_id: Option<u64>, channel_id: u64, owner: u64) -> String {
    match guild_id {
        Some(g) => format!("guild:{g}:{channel_id}:{owner}"),
        None => format!("call:{channel_id}:{owner}"),
    }
}

/// Parties d'une clé de direct ; `None` si elle est illisible.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyParts {
    pub guild_id: Option<u64>,
    pub channel_id: u64,
    pub owner: u64,
}

pub fn parse_key(key: &str) -> Option<KeyParts> {
    let mut it = key.split(':');
    let kind = it.next()?;
    let mut num = || it.next()?.parse::<u64>().ok();
    let parts = match kind {
        "guild" => KeyParts { guild_id: Some(num()?), channel_id: num()?, owner: num()? },
        "call" => KeyParts { guild_id: None, channel_id: num()?, owner: num()? },
        _ => return None,
    };
    it.next().is_none().then_some(parts)
}

/// Pourquoi Discord a fermé un direct (`reason` de STREAM_DELETE).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EndReason {
    /// Arrêté par son propriétaire (ou par moi).
    UserRequested,
    StreamEnded,
    Full,
    Unauthorized,
    RateLimited,
    /// Panne de Discord (`unavailable`).
    Unavailable,
    Other(String),
}

impl EndReason {
    pub fn from_wire(reason: Option<&str>, unavailable: bool) -> Self {
        if unavailable {
            return EndReason::Unavailable;
        }
        match reason.unwrap_or_default() {
            "user_requested" => EndReason::UserRequested,
            "stream_ended" | "" => EndReason::StreamEnded,
            "stream_full" => EndReason::Full,
            "unauthorized" => EndReason::Unauthorized,
            "safety_guild_rate_limited" => EndReason::RateLimited,
            other => EndReason::Other(other.to_string()),
        }
    }

    /// Fin normale : rien à annoncer.
    pub fn quiet(&self) -> bool {
        matches!(self, EndReason::UserRequested | EndReason::StreamEnded)
    }
}

/// Où en est la connexion à un direct.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum StreamConnState {
    /// Op 18 ou 20 envoyée ; on attend STREAM_CREATE et STREAM_SERVER_UPDATE.
    #[default]
    Requested,
    /// Tout est connu pour ouvrir la connexion du direct (lot M5).
    Connecting { endpoint: String, token: Secret, rtc_server_id: u64 },
    Connected,
    /// Message à afficher (venu de la connexion du direct).
    Failed(String),
}

/// Un direct que je diffuse ou que je regarde.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamConn {
    pub state: StreamConnState,
    /// Mon propre direct.
    pub mine: bool,
    pub rtc_server_id: Option<u64>,
    pub rtc_channel_id: Option<u64>,
    pub region: Option<String>,
    /// Spectateurs (STREAM_CREATE, STREAM_UPDATE).
    pub viewers: Vec<u64>,
    pub paused: bool,
    server: Option<(String, Secret)>,
}

impl StreamConn {
    fn try_connecting(&mut self) {
        if !matches!(self.state, StreamConnState::Requested) {
            return;
        }
        if let (Some(rtc_server_id), Some((endpoint, token))) = (self.rtc_server_id, &self.server) {
            self.state = StreamConnState::Connecting { endpoint: endpoint.clone(), token: token.clone(), rtc_server_id };
        }
    }
}

/// Réseau (et connexion du direct, lot M5) -> magasin.
#[derive(Debug)]
pub enum StreamEvent {
    /// Nouvelle session (READY) : aucun direct ne survit.
    Reset,
    Create { key: String, rtc_server_id: Option<u64>, rtc_channel_id: Option<u64>, region: Option<String>, viewers: Vec<u64>, paused: bool },
    /// STREAM_UPDATE : `None` = absent du paquet, rien ne change.
    Update { key: String, region: Option<String>, viewers: Option<Vec<u64>>, paused: Option<bool> },
    /// STREAM_SERVER_UPDATE ; `endpoint` : `None` quand le serveur est réattribué.
    Server { key: String, endpoint: Option<String>, token: Secret },
    Delete { key: String, reason: EndReason },
    // Remplis par la connexion du direct (lot M5).
    Connected { key: String },
    Failed { key: String, why: String },
    Closed { key: String },
}

/// Directs en cours de cette session, par clé.
#[derive(Default)]
pub struct Streams {
    conns: HashMap<String, StreamConn>,
    /// Dernière fin annoncée par Discord (lue une fois par l'interface).
    ended: Option<(String, EndReason)>,
}

impl Streams {
    pub fn clear(&mut self) {
        *self = Streams::default();
    }

    pub fn get(&self, key: &str) -> Option<&StreamConn> {
        self.conns.get(key)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.conns.keys().map(String::as_str)
    }

    /// Spectateurs d'un direct (vide si inconnu).
    pub fn viewers(&self, key: &str) -> &[u64] {
        self.conns.get(key).map(|c| c.viewers.as_slice()).unwrap_or(&[])
    }

    /// Dernière fin d'un direct, rendue une seule fois.
    pub fn take_ended(&mut self) -> Option<(String, EndReason)> {
        self.ended.take()
    }

    /// Op 18 ou 20 partie : le direct attend Discord.
    pub(super) fn request(&mut self, key: &str, mine: bool) {
        self.conns.insert(key.to_string(), StreamConn { mine, ..Default::default() });
    }

    pub(super) fn forget(&mut self, key: &str) -> Option<StreamConn> {
        self.conns.remove(key)
    }

    /// Applique un événement ; rend la clé d'un direct que Discord a fermé.
    pub(super) fn apply(&mut self, e: StreamEvent) -> Option<(String, bool)> {
        match e {
            StreamEvent::Reset => self.clear(),
            StreamEvent::Create { key, rtc_server_id, rtc_channel_id, region, viewers, paused } => {
                // Direct que je n'ai ni créé ni demandé : ignoré.
                let c = self.conns.get_mut(&key)?;
                c.rtc_server_id = rtc_server_id.or(c.rtc_server_id);
                c.rtc_channel_id = rtc_channel_id.or(c.rtc_channel_id);
                c.region = region;
                c.viewers = viewers;
                c.paused = paused;
                c.try_connecting();
            }
            StreamEvent::Update { key, region, viewers, paused } => {
                let c = self.conns.get_mut(&key)?;
                if region.is_some() {
                    c.region = region;
                }
                if let Some(v) = viewers {
                    c.viewers = v;
                }
                if let Some(p) = paused {
                    c.paused = p;
                }
            }
            StreamEvent::Server { key, endpoint, token } => {
                let c = self.conns.get_mut(&key)?;
                match endpoint {
                    Some(e) => {
                        c.server = Some((e, token));
                        // Nouveau serveur : la connexion est à refaire.
                        c.state = StreamConnState::Requested;
                        c.try_connecting();
                    }
                    None => {
                        c.server = None;
                        c.state = StreamConnState::Requested;
                    }
                }
            }
            StreamEvent::Delete { key, reason } => {
                let c = self.conns.remove(&key)?;
                if !reason.quiet() {
                    self.ended = Some((key.clone(), reason));
                }
                return Some((key, c.mine));
            }
            StreamEvent::Connected { key } => {
                let c = self.conns.get_mut(&key)?;
                c.state = StreamConnState::Connected;
            }
            StreamEvent::Failed { key, why } => {
                let c = self.conns.get_mut(&key)?;
                c.state = StreamConnState::Failed(why);
            }
            StreamEvent::Closed { key } => {
                let c = self.conns.get_mut(&key)?;
                if matches!(c.state, StreamConnState::Connected) {
                    c.state = StreamConnState::Requested;
                    c.try_connecting();
                }
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_round_trip_and_reject_garbage() {
        let k = stream_key(Some(5), 6, 7);
        assert_eq!(k, "guild:5:6:7");
        assert_eq!(parse_key(&k), Some(KeyParts { guild_id: Some(5), channel_id: 6, owner: 7 }));
        assert_eq!(stream_key(None, 6, 7), "call:6:7");
        assert_eq!(parse_key("call:1110739331624210483:852892297661906993"), Some(KeyParts { guild_id: None, channel_id: 1110739331624210483, owner: 852892297661906993 }));
        for bad in ["", "guild:1:2", "call:1:2:3", "test:1:2", "guild:a:2:3", "guild:1:2:3:4"] {
            assert_eq!(parse_key(bad), None, "{bad}");
        }
    }

    #[test]
    fn create_and_server_in_any_order_give_connecting() {
        let mut s = Streams::default();
        s.request("call:1:2", false);
        s.apply(StreamEvent::Server { key: "call:1:2".into(), endpoint: Some("e.example.invalid".into()), token: Secret::new("jeton-direct".into()) });
        assert_eq!(s.get("call:1:2").map(|c| &c.state), Some(&StreamConnState::Requested));
        s.apply(StreamEvent::Create { key: "call:1:2".into(), rtc_server_id: Some(9), rtc_channel_id: Some(8), region: Some("rotterdam".into()), viewers: vec![3], paused: false });
        let c = s.get("call:1:2").cloned().unwrap_or_default();
        let StreamConnState::Connecting { endpoint, token, rtc_server_id } = &c.state else { panic!("{c:?}") };
        assert_eq!((endpoint.as_str(), token.expose(), *rtc_server_id), ("e.example.invalid", "jeton-direct", 9));
        assert!(!format!("{c:?}").contains("jeton-direct"), "jeton masqué");
        s.apply(StreamEvent::Update { key: "call:1:2".into(), region: None, viewers: Some(vec![3, 4]), paused: Some(true) });
        assert_eq!(s.viewers("call:1:2"), &[3, 4]);
        // Serveur réattribué : on attend le suivant.
        s.apply(StreamEvent::Server { key: "call:1:2".into(), endpoint: None, token: Secret::new(String::new()) });
        assert_eq!(s.get("call:1:2").map(|c| &c.state), Some(&StreamConnState::Requested));
        // Un direct inconnu n'est pas créé par un événement.
        s.apply(StreamEvent::Create { key: "call:7:7".into(), rtc_server_id: Some(1), rtc_channel_id: None, region: None, viewers: vec![], paused: false });
        assert!(s.get("call:7:7").is_none());
    }

    #[test]
    fn delete_reports_only_unexpected_ends() {
        let mut s = Streams::default();
        s.request("call:1:2", true);
        assert_eq!(s.apply(StreamEvent::Delete { key: "call:1:2".into(), reason: EndReason::from_wire(Some("user_requested"), false) }), Some(("call:1:2".into(), true)));
        assert!(s.take_ended().is_none());
        s.request("call:1:3", false);
        s.apply(StreamEvent::Delete { key: "call:1:3".into(), reason: EndReason::from_wire(Some("stream_full"), false) });
        assert_eq!(s.take_ended(), Some(("call:1:3".into(), EndReason::Full)));
        assert_eq!(EndReason::from_wire(Some("x"), true), EndReason::Unavailable);
        assert_eq!(EndReason::from_wire(Some("nouveau"), false), EndReason::Other("nouveau".into()));
    }
}
