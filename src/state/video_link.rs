//! Lien entre l'interface et la vidéo de l'orchestrateur (lot M5,
//! `crate::voice`). Comme pour le vocal (`voice_link`), l'interface décrit
//! à chaque passe ce qu'elle veut ([`VideoWant`], lu dans le magasin) et
//! relève ce qui s'est passé ([`VideoUp`]) : connexions des directs, et
//! pannes de capture ou de codec à afficher.

use super::stream::{parse_key, StreamConnState, StreamEvent};
use super::video::VideoSink;
use super::voice::Secret;
use super::Store;
use crate::video::Quality;

/// Ce qu'il faut pour ouvrir la connexion d'un direct (STREAM_CREATE et
/// STREAM_SERVER_UPDATE).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamTarget<'a> {
    /// `rtc_server_id` : tient lieu de serveur dans l'Identify.
    pub server_id: u64,
    /// `rtc_channel_id`, sinon le salon de la clé.
    pub channel_id: u64,
    pub endpoint: &'a str,
    pub token: &'a Secret,
    pub session_id: &'a str,
}

/// Que doit faire la connexion d'un direct.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamWant<'a> {
    /// Discord n'a pas (ou plus) donné de serveur, ou échec affiché : rien ne tourne.
    Wait,
    /// Ouvrir (ou rouvrir si autre chose).
    Open(StreamTarget<'a>),
    /// Connexion établie : la garder.
    Keep,
}

/// Mon partage d'écran.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareWant<'a> {
    pub key: &'a str,
    pub source_id: &'a str,
    pub quality: Quality,
    pub audio: bool,
    pub conn: StreamWant<'a>,
}

/// Un direct que je regarde.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WatchWant<'a> {
    pub key: &'a str,
    pub owner: u64,
    pub conn: StreamWant<'a>,
}

/// Ce que l'interface veut de la vidéo, à cette passe.
#[derive(Clone)]
pub struct VideoWant<'a> {
    /// Moi (`None` avant READY) : les directs se connectent en mon nom.
    pub user_id: Option<u64>,
    /// Ma caméra : `Some(None)` = la caméra par défaut.
    pub camera: Option<Option<&'a str>>,
    /// Personnes de mon salon (moi exclu) dont la caméra est allumée.
    pub cameras: Vec<u64>,
    pub share: Option<ShareWant<'a>>,
    pub watch: Vec<WatchWant<'a>>,
    /// Images vers les tuiles, et taille affichée de chacune.
    pub sink: &'a VideoSink,
}

/// Ce que mon partage envoie vraiment, mesuré sur les dernières secondes :
/// montré sous « Vous partagez » (un partage flou chez ceux qui regardent
/// sans rien qui dise pourquoi, constaté par l'utilisateur, 0.1.27).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SendStats {
    pub width: u32,
    pub height: u32,
    /// Images réellement encodées par seconde.
    pub fps: u32,
    /// Débit réellement produit, en kilobits par seconde.
    pub kbps: u32,
    /// Encodeur logiciel (celui de la carte graphique absent ou en panne).
    pub software: bool,
    /// Codec négocié avec Discord (« H264 », « VP8 »).
    pub codec: &'static str,
    /// Nom de l'encodeur (« NVIDIA H.264 Encoder MFT ») ; vide s'il n'en a pas.
    pub encoder: &'static str,
    /// Images perdues parce que la connexion n'a pas suivi.
    pub lost: u32,
}

/// Vidéo -> interface.
#[derive(Debug)]
pub enum VideoUp {
    /// Mesure périodique de mon partage.
    ShareStats(SendStats),
    /// Connexion d'un direct : `Connected`, `Failed`, `Closed`.
    Stream(StreamEvent),
    /// La caméra n'a pas pu tourner (message à afficher) : elle est éteinte.
    CameraFailed(String),
    /// Le partage n'a pas pu tourner (message à afficher) : il est arrêté.
    ShareFailed(String),
    /// À signaler sans rien arrêter (définition réduite, codec absent…).
    Problem(String),
}

/// Qualité à demander (op 15, 0 à 100) pour une tuile affichée à cette
/// définition : 0 hors champ, 100 au-delà de 720p, 50 en dessous.
pub fn sink_quality((w, h): (u32, u32)) -> u8 {
    match (w, h) {
        (0, _) | (_, 0) => 0,
        (w, h) if w >= 1280 || h >= 720 => 100,
        _ => 50,
    }
}

impl Store {
    fn stream_want(&self, key: &str) -> StreamWant<'_> {
        let Some(c) = self.stream.get(key) else { return StreamWant::Wait };
        match &c.state {
            StreamConnState::Connected => StreamWant::Keep,
            StreamConnState::Connecting { endpoint, token, rtc_server_id } => {
                let channel = c.rtc_channel_id.or_else(|| parse_key(key).map(|p| p.channel_id));
                match (channel, self.voice.session_id()) {
                    (Some(channel_id), Some(session_id)) => StreamWant::Open(StreamTarget { server_id: *rtc_server_id, channel_id, endpoint, token, session_id }),
                    _ => StreamWant::Wait,
                }
            }
            StreamConnState::Requested | StreamConnState::Failed(_) => StreamWant::Wait,
        }
    }

    /// Ce que la vidéo doit faire, d'après le magasin.
    pub fn video_want(&self) -> VideoWant<'_> {
        let v = &self.video;
        let me = self.me.as_ref().map(|u| u.id);
        let my = self.voice.me;
        let camera = (v.camera && my.is_some()).then_some(v.camera_source.as_deref());
        let cameras = match my {
            Some(m) => self.voice.members(m.channel_id).filter(|s| s.self_video && Some(s.user_id) != me).map(|s| s.user_id).collect(),
            None => Vec::new(),
        };
        let share = v.share.as_ref().map(|s| ShareWant { key: &s.key, source_id: &s.source_id, quality: s.quality, audio: s.audio, conn: self.stream_want(&s.key) });
        let watch = v.watched().iter().filter_map(|k| Some(WatchWant { key: k, owner: parse_key(k)?.owner, conn: self.stream_want(k) })).collect();
        VideoWant { user_id: me, camera, cameras, share, watch, sink: &v.sink }
    }

    /// Applique ce que la vidéo a donné ; rend le message à afficher.
    pub fn apply_video_up(&mut self, u: VideoUp) -> Option<String> {
        match u {
            VideoUp::Stream(e) => {
                self.apply_stream(e);
                None
            }
            VideoUp::CameraFailed(why) => {
                self.video_set_camera(false);
                Some(why)
            }
            VideoUp::ShareFailed(why) => {
                self.video_stop_share();
                Some(why)
            }
            VideoUp::Problem(why) => Some(why),
            VideoUp::ShareStats(st) => {
                if let Some(sh) = self.video.share.as_mut() {
                    sh.stats = Some(st);
                }
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Avatar, User};
    use crate::state::stream::stream_key;
    use crate::state::voice::{VoiceEvent, VoiceState};
    use crate::state::Event;

    const ME: u64 = 1;
    const BOB: u64 = 2;
    const G: u64 = 100;
    const VOC: u64 = 501;

    fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User { id: ME, name: "moi".into(), avatar: Avatar::default_for(ME) }, guilds: vec![], dms: vec![] });
        let st = |user_id, video, stream| VoiceState { user_id, guild_id: Some(G), channel_id: VOC, self_video: video, self_stream: stream, ..Default::default() };
        s.apply(Event::Voice(VoiceEvent::States(vec![st(BOB, true, true)])));
        s.voice_join_call(VOC);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(st(ME, false, false)), session_id: Some("sess".into()) }));
        s.take_commands();
        s
    }

    #[test]
    fn want_follows_the_store() {
        let mut s = store();
        let w = s.video_want();
        assert_eq!((w.camera, w.cameras.as_slice(), w.share.is_none(), w.watch.is_empty()), (None, &[BOB][..], true, true));
        s.video_set_camera(true);
        assert_eq!(s.video_want().camera, Some(None));
        let key = stream_key(Some(G), VOC, BOB);
        assert!(s.video_watch(&key));
        assert_eq!(s.video_want().watch, vec![WatchWant { key: &key, owner: BOB, conn: StreamWant::Wait }]);
        s.apply(Event::Stream(StreamEvent::Create { key: key.clone(), rtc_server_id: Some(77), rtc_channel_id: Some(78), region: None, viewers: vec![], paused: false }));
        let token = Secret::new("jeton-direct".into());
        s.apply(Event::Stream(StreamEvent::Server { key: key.clone(), endpoint: Some("d.example.invalid".into()), token: token.clone() }));
        let w = s.video_want();
        assert_eq!(w.watch[0].conn, StreamWant::Open(StreamTarget { server_id: 77, channel_id: 78, endpoint: "d.example.invalid", token: &token, session_id: "sess" }));
        assert_eq!(s.apply_video_up(VideoUp::Stream(StreamEvent::Connected { key: key.clone() })), None);
        assert_eq!(s.video_want().watch[0].conn, StreamWant::Keep);
        // Panne de caméra : éteinte, message rendu, op 4 repartie.
        assert_eq!(s.apply_video_up(VideoUp::CameraFailed("x".into())).as_deref(), Some("x"));
        // Une mesure sans partage en cours : rien à afficher, rien de retenu.
        let st = SendStats { width: 3840, height: 2160, fps: 60, kbps: 17_000, software: false, codec: "H264", encoder: "", lost: 0 };
        assert_eq!(s.apply_video_up(VideoUp::ShareStats(st)), None);
        assert!(s.video.share.is_none());
        assert!(!s.video.camera);
        assert_eq!(s.video_want().camera, None);
        assert_eq!(sink_quality((0, 0)), 0);
        assert_eq!(sink_quality((640, 360)), 50);
        assert_eq!(sink_quality((1920, 1080)), 100);
    }
}
