//! Connexion vocale (lot V1) : passerelle vocale v8, découverte d'adresse,
//! transport RTP chiffré sur UDP. Documentation : Voice Connections.
//!
//! Une tâche par appel, lancée par [`spawn`] et arrêtée par
//! `VoiceCtl::Disconnect` ou l'abandon de l'émetteur : hors appel, rien ne
//! tourne. Le chiffrement de bout en bout (DAVE, lot V3) se branche par
//! [`e2ee::FrameCrypto`].

mod conn;
pub mod crypt;
pub mod dave;
pub mod e2ee;
pub mod rtp;
pub mod video;
#[cfg(test)]
mod robust;

pub use crate::state::admin::webhooks::Secret;

use crate::video::EncodedFrame;
use std::{net::IpAddr, time::Duration};
use tokio::sync::mpsc;

/// Ce que donnent VOICE_STATE_UPDATE (session) et VOICE_SERVER_UPDATE (point d'accès, jeton).
#[derive(Clone, Debug)]
pub struct VoiceParams {
    /// `None` pour un appel privé : le salon tient lieu de serveur. Pour un
    /// direct : `rtc_server_id` de STREAM_CREATE.
    pub guild_id: Option<u64>,
    /// Pour un direct : `rtc_channel_id` de STREAM_CREATE.
    pub channel_id: u64,
    pub user_id: u64,
    /// Session de la passerelle principale (la même pour un direct).
    pub session_id: String,
    /// `hôte[:port]`, tel que reçu dans VOICE_SERVER_UPDATE (STREAM_SERVER_UPDATE pour un direct).
    pub endpoint: String,
    /// Jamais affiché : `Debug` masqué.
    pub token: Secret,
    pub kind: ConnKind,
}

/// Connexion d'un salon vocal ou d'un direct (Go Live). Un direct est une
/// connexion à part, vers le serveur de STREAM_SERVER_UPDATE, la connexion
/// vocale restant ouverte (docs.discord.food, Voice Connections, « Go Live » ;
/// Gateway Events, « Stream Create » : `rtc_server_id` et `rtc_channel_id`
/// servent à l'Identify, que l'Identify d'un direct envoie avec
/// `channel_id` et un flux de type `screen`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum ConnKind {
    #[default]
    Voice,
    /// `guild:<serveur>:<salon>:<auteur>` ou `call:<salon>:<auteur>`.
    Stream { stream_key: String },
}

impl VoiceParams {
    /// Identifiant du groupe MLS de DAVE : le salon, ou pour un direct
    /// `rtc_server_id - 1` (docs.discord.food, Voice Connections, « Go Live »).
    pub fn dave_group_id(&self) -> u64 {
        match self.kind {
            ConnKind::Voice => self.channel_id,
            ConnKind::Stream { .. } => self.guild_id.unwrap_or(self.channel_id).wrapping_sub(1),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceCtl {
    /// Une trame Opus de 20 ms.
    Opus(Vec<u8>),
    Speaking(bool),
    Disconnect,
    /// Une image compressée à envoyer (ignorée tant que `VideoState` n'est pas actif).
    Video(EncodedFrame),
    /// Annonce (op 12) de notre vidéo : caméra ou partage allumé, ou coupé.
    VideoState { active: bool, width: u32, height: u32, fps: u32 },
    /// Demande une image clé au flux de cet utilisateur (décodeur perdu).
    RequestKeyframe(u64),
    /// Qualité voulue (0 à 100, 0 : rien) de la vidéo de ces personnes (op 15).
    SinkWants(Vec<(u64, u8)>),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VoiceEvent {
    /// Connexion prête à envoyer (clé de session reçue) ; mon SSRC.
    Ready { ssrc: u32 },
    Speaking { user_id: u64, speaking: bool },
    Audio { user_id: u64, seq: u16, timestamp: u32, opus: Vec<u8> },
    UserJoined(u64),
    UserLeft(u64),
    /// Aller-retour du dernier heartbeat, en ms.
    Ping(u32),
    /// Image complète d'un autre participant, déchiffrée.
    Video { user_id: u64, frame: EncodedFrame },
    /// Un participant allume ou coupe sa vidéo (op 12).
    VideoState { user_id: u64, active: bool },
    /// Un abonné demande une image clé (PLI ou FIR) : forcer la prochaine.
    KeyframeRequested,
    /// Paquets vidéo que les abonnés n'ont pas reçus et ont redemandés (NACK).
    VideoLoss(u32),
    /// Codec vidéo à envoyer (Session Description ou Update) : l'encodeur suit.
    SendCodec(crate::video::Codec),
    /// Dernier événement : la tâche s'arrête.
    Closed(CloseReason),
}

/// Pourquoi la connexion s'est arrêtée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseReason {
    /// `VoiceCtl::Disconnect`, ou plus personne pour écouter.
    Requested,
    /// Retiré du salon (4014) ou appel terminé (4022) : ne pas se reconnecter.
    Disconnected(u16),
    /// Session expirée ou invalide (4006, 4009) : il faut une nouvelle
    /// session (nouvel op 4 sur la passerelle principale).
    SessionExpired(u16),
    /// Refus du serveur (authentification, protocole, chiffrement, DAVE
    /// exigé, limite de débit…) : réessayer à l'identique ne servirait à rien.
    Refused(u16),
    /// Aucun mode de chiffrement commun.
    Unsupported,
    /// Point d'accès ou adresse UDP refusés par [`VoiceHosts`].
    BadEndpoint,
    /// Serveur injoignable : connexion, découverte d'adresse ou reprises épuisées.
    Unreachable,
}

/// Que faire d'un code de fermeture de la passerelle vocale (documentation :
/// Voice Close Event Codes). `None` : reprendre la session (op 7).
pub fn close_action(code: Option<u16>) -> Option<CloseReason> {
    match code {
        // Coupure sans code, fermeture normale par le serveur, serveur vocal tombé : reprise.
        None | Some(1000 | 1001 | 1006 | 1011 | 1012 | 4015) => None,
        Some(c @ (4014 | 4022)) => Some(CloseReason::Disconnected(c)),
        Some(c @ (4006 | 4009)) => Some(CloseReason::SessionExpired(c)),
        Some(c) => Some(CloseReason::Refused(c)),
    }
}

/// Règle des adresses où partent le jeton vocal et l'audio. Le point
/// d'accès vient du réseau (VOICE_SERVER_UPDATE), l'adresse UDP du serveur
/// vocal (Ready) : seuls ceux de Discord sont acceptés.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VoiceHosts {
    /// `wss://<sous-domaine>.discord.media` (ou `.discord.gg`), port quelconque ;
    /// adresse UDP publique.
    Discord,
    /// Tests seulement : `ws://127.0.0.1:<port>` et UDP sur 127.0.0.1.
    Loopback,
}

impl VoiceHosts {
    /// Adresse de la passerelle vocale v8 pour `endpoint`, si elle est acceptée.
    pub fn url(self, endpoint: &str) -> Option<String> {
        let endpoint = endpoint.strip_prefix("wss://").unwrap_or(endpoint).trim_end_matches('/');
        if !endpoint.is_ascii() || endpoint.contains(['/', '?', '#', '@', '\\']) {
            return None;
        }
        let scheme = match self {
            VoiceHosts::Discord => "wss",
            VoiceHosts::Loopback => "ws",
        };
        let u = reqwest::Url::parse(&format!("{scheme}://{endpoint}/?v=8")).ok()?;
        let ok = match self {
            VoiceHosts::Discord => {
                // Le port varie selon le serveur (443, 2087, 2096…) : seul l'hôte compte.
                u.domain().is_some_and(|h| h.ends_with(".discord.media") || h.ends_with(".discord.gg"))
            }
            VoiceHosts::Loopback => u.host_str() == Some("127.0.0.1") && u.port().is_some(),
        };
        ok.then(|| u.as_str().to_string())
    }

    /// Adresse UDP du serveur vocal (Ready) acceptée : jamais le réseau local en production.
    pub fn udp_ok(self, ip: IpAddr) -> bool {
        match self {
            VoiceHosts::Loopback => ip.is_loopback(),
            VoiceHosts::Discord => match ip {
                IpAddr::V4(v) => !(v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_multicast() || v.is_broadcast()),
                IpAddr::V6(v) => {
                    let s = v.segments()[0];
                    !(v.is_loopback() || v.is_unspecified() || v.is_multicast() || s & 0xFE00 == 0xFC00 || s & 0xFFC0 == 0xFE80)
                }
            },
        }
    }
}

/// Réglages de la connexion ; l'application n'utilise que `default()`.
#[derive(Clone, Debug)]
pub struct VoiceConfig {
    pub hosts: VoiceHosts,
    /// Attente avant une reprise ; doublée à chaque échec consécutif.
    pub reconnect_base: Duration,
    /// Reprises consécutives sans succès avant d'abandonner.
    pub max_resumes: u32,
    /// Intervalle de battement le plus court accepté du serveur.
    pub min_heartbeat: Duration,
    /// Attente d'une réponse de découverte d'adresse, par essai (5 essais).
    pub discovery_timeout: Duration,
    /// Codecs vidéo proposés (Select Protocol) ; vide : pas de vidéo.
    pub video_codecs: Vec<video::CodecOffer>,
    /// En-têtes d'ouverture du WebSocket vocal (`net::identity`) ; aucun : comme Ferrocord.
    pub ws_headers: Vec<(&'static str, String)>,
}

impl Default for VoiceConfig {
    fn default() -> Self {
        VoiceConfig {
            hosts: VoiceHosts::Discord,
            reconnect_base: Duration::from_millis(500),
            max_resumes: 5,
            min_heartbeat: Duration::from_secs(1),
            discovery_timeout: Duration::from_secs(1),
            video_codecs: video::default_offers(),
            ws_headers: Vec::new(),
        }
    }
}

/// Lance la connexion vocale sur `rt`.
pub fn spawn(rt: &tokio::runtime::Handle, p: VoiceParams, e2ee: Box<dyn e2ee::FrameCrypto>) -> (mpsc::Sender<VoiceCtl>, mpsc::Receiver<VoiceEvent>) {
    spawn_with(rt, p, e2ee, VoiceConfig::default())
}

/// Comme [`spawn`], avec d'autres réglages (tests : faux serveur local).
pub fn spawn_with(
    rt: &tokio::runtime::Handle,
    p: VoiceParams,
    e2ee: Box<dyn e2ee::FrameCrypto>,
    cfg: VoiceConfig,
) -> (mpsc::Sender<VoiceCtl>, mpsc::Receiver<VoiceEvent>) {
    // 64 trames = 1,3 s d'audio d'avance ; au-delà, l'envoyeur doit jeter.
    let (ctl_tx, ctl_rx) = mpsc::channel(64);
    let (ev_tx, ev_rx) = mpsc::channel(512);
    rt.spawn(conn::run(p, e2ee, cfg, ctl_rx, ev_tx));
    (ctl_tx, ev_rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_codes() {
        assert_eq!(close_action(None), None);
        assert_eq!(close_action(Some(4015)), None);
        assert_eq!(close_action(Some(1006)), None);
        assert_eq!(close_action(Some(4014)), Some(CloseReason::Disconnected(4014)));
        assert_eq!(close_action(Some(4022)), Some(CloseReason::Disconnected(4022)));
        assert_eq!(close_action(Some(4006)), Some(CloseReason::SessionExpired(4006)));
        assert_eq!(close_action(Some(4009)), Some(CloseReason::SessionExpired(4009)));
        for c in [4001, 4002, 4003, 4004, 4005, 4011, 4012, 4016, 4017, 4020, 4021, 4999] {
            assert_eq!(close_action(Some(c)), Some(CloseReason::Refused(c)));
        }
    }

    #[test]
    fn discord_endpoints() {
        let d = VoiceHosts::Discord;
        assert_eq!(d.url("c-cdg09-abcd.discord.media:443").as_deref(), Some("wss://c-cdg09-abcd.discord.media/?v=8"));
        assert_eq!(d.url("c-cdg09-abcd.discord.media").as_deref(), Some("wss://c-cdg09-abcd.discord.media/?v=8"));
        assert_eq!(d.url("wss://us-east1.discord.gg/").as_deref(), Some("wss://us-east1.discord.gg/?v=8"));
        assert_eq!(d.url("c-cdg08-1a2b3c4d.discord.media:2096").as_deref(), Some("wss://c-cdg08-1a2b3c4d.discord.media:2096/?v=8"));
        for bad in [
            "",
            "evil.tld",
            "discord.media.evil.tld",
            "c.discord.media/chemin",
            "c.discord.media?x",
            "u@c.discord.media",
            "evil.tld#.discord.media",
            "127.0.0.1:443",
            "évil.discord.media",
        ] {
            assert_eq!(d.url(bad), None, "{bad} devrait être refusé");
        }
        assert_eq!(VoiceHosts::Loopback.url("127.0.0.1:5000").as_deref(), Some("ws://127.0.0.1:5000/?v=8"));
        assert_eq!(VoiceHosts::Loopback.url("localhost:5000"), None);
    }

    #[test]
    fn udp_addresses() {
        let ok = |s: &str| VoiceHosts::Discord.udp_ok(s.parse().unwrap_or(IpAddr::from([0, 0, 0, 0])));
        assert!(ok("162.159.130.234"));
        assert!(ok("2606:4700::1"));
        for bad in ["127.0.0.1", "10.0.0.1", "192.168.1.1", "169.254.1.1", "0.0.0.0", "::1", "fd00::1", "fe80::1", "224.0.0.1"] {
            assert!(!ok(bad), "{bad}");
        }
        assert!(VoiceHosts::Loopback.udp_ok(IpAddr::from([127, 0, 0, 1])));
        assert!(!VoiceHosts::Loopback.udp_ok(IpAddr::from([8, 8, 8, 8])));
    }

    #[test]
    fn token_never_shown() {
        let p = VoiceParams {
            guild_id: Some(1),
            channel_id: 2,
            user_id: 3,
            session_id: "s".into(),
            endpoint: "e".into(),
            token: Secret::new("jeton-vocal-secret".into()),
            kind: ConnKind::Stream { stream_key: "guild:1:2:3".into() },
        };
        assert!(!format!("{p:?}").contains("jeton-vocal-secret"));
        // Groupe MLS d'un direct : rtc_server_id - 1 ; d'un salon : le salon.
        assert_eq!(p.dave_group_id(), 0);
        let v = VoiceParams { kind: ConnKind::Voice, ..p };
        assert_eq!(v.dave_group_id(), 2);
    }
}
