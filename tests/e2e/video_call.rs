//! Vidéo de bout en bout (lot M5) : orchestrateur réel, faux serveur vocal
//! (127.0.0.1), fausse source vidéo, faux codecs, faux périphérique audio.
//! Caméra aller et retour, image clé sur PLI, image reçue décodée vers sa
//! tuile, image clé redemandée après une image illisible, partage d'écran
//! (avec son, définition réduite quand la « carte graphique » refuse),
//! direct regardé (image et son), et plus rien ne tourne à la fin.

use crate::support::audio::{Backend, FakeAudio};
use crate::support::video::{access_unit, is_key, FakeVideo, Video, CORRUPT};
use crate::support::voice::*;
use crate::support::within;
use ferrocord::audio::engine::{Engine, Gate, FRAME};
use ferrocord::net::voice::{
    rtp,
    video::{h264, rtcp},
    Secret,
};
use ferrocord::state::audio_prefs::AudioPrefs;
use ferrocord::state::stream::StreamEvent;
use ferrocord::state::video::{Tile, VideoSink};
use ferrocord::state::video_link::{ShareWant, StreamTarget, StreamWant, VideoUp, VideoWant, WatchWant};
use ferrocord::state::voice::VoiceEvent;
use ferrocord::state::voice_link::{CallTarget, CallWant, VoiceLink, VoiceWant};
use ferrocord::video::Quality;
use ferrocord::voice::{Options, Voice};
use serde_json::json;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::Duration;

const AES: &str = "aead_aes256_gcm_rtpsize";
const OTHER: u64 = 41_771_983_423_143_937;
const OTHER_AUDIO: u32 = 777;
const OTHER_VIDEO: u32 = 900;
const H264_PT: u8 = 103;
const RTC_SERVER: u64 = 99_000_000_000_000_001;

struct Rig {
    dev: Arc<FakeAudio>,
    vid: Arc<FakeVideo>,
    v: Voice,
    sink: VideoSink,
    prefs: AudioPrefs,
}

fn rig() -> Rig {
    let dev = FakeAudio::new();
    let vid = FakeVideo::new(dev.clone());
    let opts = Options { voice: config(), backend: Arc::new(Backend(dev.clone())), video: Arc::new(Video(vid.clone())), join_timeout: Duration::from_millis(300), device_check: Duration::from_millis(200) };
    let v = Voice::new(tokio::runtime::Handle::current(), opts, Arc::new(|| {}));
    let prefs = AudioPrefs { noise_suppression: false, echo_cancellation: false, auto_gain: false, auto_threshold: false, threshold_db: -40, ..Default::default() };
    Rig { dev, vid, v, sink: VideoSink::default(), prefs }
}

impl Rig {
    fn call(&mut self, endpoint: Option<&str>, token: &Secret) {
        let call = match endpoint {
            Some(endpoint) => CallWant::Open(CallTarget { guild_id: Some(GUILD), channel_id: CHANNEL, user_id: ME, endpoint, session_id: SESSION, token }),
            None => CallWant::Stop,
        };
        self.v.sync(&VoiceWant { call, muted: false, deafened: false, audio: &self.prefs });
    }

    async fn until_up(&mut self, what: &str, mut pred: impl FnMut(&VideoUp) -> bool) -> VideoUp {
        within(what, async {
            loop {
                let _ = self.v.poll();
                if let Some(u) = self.v.poll_video().into_iter().find(|u| pred(u)) {
                    return u;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    }

    async fn until_voice(&mut self, what: &str, mut pred: impl FnMut(&VoiceEvent) -> bool) {
        within(what, async {
            loop {
                if self.v.poll().iter().any(&mut pred) {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
    }
}

/// Ce que l'interface veut de la vidéo, réglé par `f`.
fn video<'a>(v: &mut Voice, sink: &'a VideoSink, f: impl FnOnce(&mut VideoWant<'a>)) {
    let mut w = VideoWant { user_id: Some(ME), camera: None, cameras: Vec::new(), share: None, watch: Vec::new(), sink };
    f(&mut w);
    v.sync_video(&w);
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    within(what, async {
        while !f() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

/// Prochaine image vidéo complète du client : (SSRC, données réassemblées).
async fn next_image(server: &FakeVoice, h: &Handshake) -> (u32, Vec<u8>) {
    let mut parts = Vec::new();
    loop {
        if let Media::Rtp { pt, marker, ssrc, payload, .. } = server.recv_media(&h.cipher).await {
            if pt != H264_PT {
                continue;
            }
            parts.push(payload);
            if marker {
                let data = h264::depacketize(parts.iter().map(Vec::as_slice)).unwrap_or_else(|| panic!("image illisible"));
                return (ssrc, data);
            }
        }
    }
}

/// Prochain op JSON `op` du client (les autres sont sautés).
async fn next_op(c: &mut VoiceConn, op: u64) -> serde_json::Value {
    within("op du client", async {
        loop {
            if let Msg::Json(v) = c.recv().await
                && v["op"] == op
            {
                return v;
            }
        }
    })
    .await
}

/// Un autre participant envoie une image (paquets H.264 de 1100 octets au plus).
async fn send_image(server: &FakeVoice, h: &Handshake, data: &[u8], seq: &mut u16, ts: u32, counter: &mut u32) {
    let parts = h264::packetize(data, 1100);
    for (k, p) in parts.iter().enumerate() {
        let hdr = rtp::header_with(H264_PT, k + 1 == parts.len(), *seq, ts, OTHER_VIDEO);
        server.send_sealed(h, &hdr, p, *counter).await;
        *seq = seq.wrapping_add(1);
        *counter += 1;
    }
}

/// Fin d'une image précédente : le récepteur se cale sur un marqueur avant
/// de réassembler (comme en cours de flux).
async fn prime(server: &FakeVoice, h: &Handshake, seq: &mut u16, counter: &mut u32) {
    let hdr = rtp::header_with(H264_PT, true, *seq, 87_000, OTHER_VIDEO);
    server.send_sealed(h, &hdr, &[0x41, 0x9a], *counter).await;
    *seq = seq.wrapping_add(1);
    *counter += 1;
}

/// L'autre participant annonce sa caméra (op 11 puis op 12).
async fn other_camera(c: &mut VoiceConn) {
    c.send(json!({"op": 11, "seq": 1, "d": {"user_ids": [OTHER.to_string()]}})).await;
    c.send(json!({"op": 12, "seq": 2, "d": {
        "user_id": OTHER.to_string(), "audio_ssrc": OTHER_AUDIO, "video_ssrc": OTHER_VIDEO,
        "streams": [{"type": "video", "rid": "100", "ssrc": OTHER_VIDEO, "rtx_ssrc": OTHER_VIDEO + 1, "active": true, "quality": 100}],
    }}))
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn camera_aller_retour_et_image_cle_sur_pli() {
    let mut r = rig();
    let mut server = FakeVoice::start().await;
    let token = Secret::new(VOICE_TOKEN.into());
    // Aperçu local affiché.
    r.sink.show(&[(Tile::Camera(ME), (320, 180))]);
    video(&mut r.v, &r.sink, |w| w.camera = Some(None));
    assert_eq!(r.vid.opened.load(SeqCst), 0, "rien avant l'appel");
    r.call(Some(&server.endpoint()), &token);
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    assert_eq!(h.identify["video"], true);

    // La caméra s'annonce (op 12) à sa définition, puis la première image est une image clé.
    let op = next_op(&mut c, 12).await;
    assert_eq!((op["d"]["video_ssrc"].as_u64(), op["d"]["streams"][0]["max_resolution"]["width"].as_u64()), (Some(u64::from(MY_SSRC) + 1), Some(320)));
    let (ssrc, first) = next_image(&server, &h).await;
    assert_eq!(ssrc, MY_SSRC + 1);
    assert!(is_key(&first), "image clé au démarrage");
    assert_eq!(r.vid.last_source.lock().map(|s| s.clone()).unwrap_or_default().as_deref(), Some("camera:fausse"), "caméra par défaut");
    wait_for("aperçu local", || r.sink.take(&Tile::Camera(ME), 0).is_some()).await;

    // Régime établi : plus d'images clés ; un PLI en fait venir une.
    loop {
        if !is_key(&next_image(&server, &h).await.1) {
            break;
        }
    }
    server.send_rtcp_sealed(&h, &rtcp::pli(1, MY_SSRC + 1), 1).await;
    within("image clé après le PLI", async {
        while !is_key(&next_image(&server, &h).await.1) {}
    })
    .await;

    // Caméra éteinte : op 12 sans vidéo, capture fermée.
    video(&mut r.v, &r.sink, |w| w.camera = None);
    assert_eq!(next_op(&mut c, 12).await["d"]["video_ssrc"], 0);
    wait_for("capture fermée", || r.vid.open_now.load(SeqCst) == 0).await;

    // Rallumée : nouvelle capture, image clé.
    video(&mut r.v, &r.sink, |w| w.camera = Some(None));
    assert_ne!(next_op(&mut c, 12).await["d"]["video_ssrc"], 0);
    assert!(is_key(&next_image(&server, &h).await.1));

    // Départ : tout s'arrête.
    r.call(None, &token);
    c.expect_closed().await;
    wait_for("capture fermée au départ", || r.vid.open_now.load(SeqCst) == 0 && r.dev.open_now.load(SeqCst) == 0).await;
    assert_eq!(r.vid.opened.load(SeqCst), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn video_recue_decodee_vers_sa_tuile() {
    let mut r = rig();
    let mut server = FakeVoice::start().await;
    let token = Secret::new(VOICE_TOKEN.into());
    // Comme l'interface : la vidéo d'abord (file d'images), puis l'appel.
    video(&mut r.v, &r.sink, |_| {});
    r.call(Some(&server.endpoint()), &token);
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    r.until_voice("Connected", |e| matches!(e, VoiceEvent::Connected)).await;
    other_camera(&mut c).await;
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Tuile affichée petite : qualité 50 demandée pour sa SSRC (op 15).
    let other = Tile::Camera(OTHER);
    r.sink.show(&[(other.clone(), (320, 180))]);
    video(&mut r.v, &r.sink, |w| w.cameras = vec![OTHER]);
    assert_eq!(next_op(&mut c, 15).await["d"], json!({ OTHER_VIDEO.to_string(): 50 }));
    // Agrandie : qualité 100.
    r.sink.show(&[(other.clone(), (1280, 720))]);
    video(&mut r.v, &r.sink, |w| w.cameras = vec![OTHER]);
    assert_eq!(next_op(&mut c, 15).await["d"], json!({ OTHER_VIDEO.to_string(): 100 }));

    // Une image clé arrive : décodée, puis posée sur sa tuile.
    let (mut seq, mut counter) = (500u16, 10u32);
    prime(&server, &h, &mut seq, &mut counter).await;
    send_image(&server, &h, &access_unit(true, 640, 360, 1), &mut seq, 90_000, &mut counter).await;
    wait_for("image sur la tuile", || r.sink.take(&other, 0).is_some_and(|(_, img)| (img.width, img.height) == (640, 360))).await;
    assert_eq!(r.vid.decoders_now.load(SeqCst), 1, "un décodeur pour cette personne");

    // Image illisible pour le décodeur : il redemande une image clé (PLI).
    send_image(&server, &h, &access_unit(false, 640, 360, CORRUPT), &mut seq, 93_000, &mut counter).await;
    within("PLI après l'image illisible", async {
        loop {
            if let Media::Rtcp(plain) = server.recv_media(&h.cipher).await
                && rtcp::parse(&plain).contains(&rtcp::Feedback::Pli { media: OTHER_VIDEO })
            {
                return;
            }
        }
    })
    .await;

    // Il coupe sa caméra : son décodeur s'arrête.
    c.send(json!({"op": 12, "seq": 3, "d": {"user_id": OTHER.to_string(), "audio_ssrc": OTHER_AUDIO, "video_ssrc": 0, "streams": []}})).await;
    wait_for("décodeur arrêté", || r.vid.decoders_now.load(SeqCst) == 0).await;
    r.call(None, &token);
    c.expect_closed().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn partage_d_ecran_avec_son_puis_arret() {
    let mut r = rig();
    // Source 1280×720 demandée en 720p, mais la « carte graphique » plafonne à 640×360.
    *r.vid.size.lock().unwrap_or_else(|e| e.into_inner()) = (1280, 720);
    *r.vid.max_encode.lock().unwrap_or_else(|e| e.into_inner()) = (640, 360);
    let mut server = FakeVoice::start().await;
    let key = format!("guild:{GUILD}:{CHANNEL}:{ME}");
    let token = Secret::new("jeton-direct".into());
    let endpoint = server.endpoint();
    let target = StreamTarget { server_id: RTC_SERVER, channel_id: CHANNEL, endpoint: &endpoint, token: &token, session_id: SESSION };
    let share = |conn| ShareWant { key: &key, source_id: "screen:1", quality: Quality::HD_30, audio: true, conn };
    video(&mut r.v, &r.sink, |w| w.share = Some(share(StreamWant::Wait)));
    assert_eq!(r.v.streams_running(), 0, "pas de serveur : rien ne tourne");

    // STREAM_SERVER_UPDATE reçu : connexion du direct.
    video(&mut r.v, &r.sink, |w| w.share = Some(share(StreamWant::Open(target))));
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    assert_eq!(h.identify["server_id"], RTC_SERVER.to_string());
    assert_eq!(h.identify["channel_id"], CHANNEL.to_string());
    assert_eq!(h.identify["token"], "jeton-direct");
    assert_eq!(h.identify["streams"][0]["type"], "screen");
    r.until_up("direct connecté", |u| matches!(u, VideoUp::Stream(StreamEvent::Connected { key: k }) if *k == key)).await;
    video(&mut r.v, &r.sink, |w| w.share = Some(share(StreamWant::Keep)));

    // Définition refusée : repli annoncé, op 12 et images en 640×360.
    let op = next_op(&mut c, 12).await;
    assert_eq!(op["d"]["streams"][0]["type"], "video", "« screen » ne sert que dans l'Identify");
    assert_eq!(op["d"]["streams"][0]["max_resolution"]["width"], 640);
    r.until_up("repli annoncé", |u| matches!(u, VideoUp::Problem(_))).await;
    assert!(is_key(&next_image(&server, &h).await.1));
    assert_eq!(r.vid.last_source.lock().map(|s| s.clone()).unwrap_or_default().as_deref(), Some("screen:1"));

    // Le son de la source part en Opus sur la connexion du direct.
    r.dev.set_mic(0.5);
    within("son du partage", async {
        loop {
            if let Media::Rtp { pt: rtp::PAYLOAD_TYPE, payload, .. } = server.recv_media(&h.cipher).await
                && payload.len() > 3
            {
                return;
            }
        }
    })
    .await;

    // Arrêt : connexion fermée, capture et son lâchés.
    video(&mut r.v, &r.sink, |_| {});
    c.expect_closed().await;
    wait_for("capture et son fermés", || r.vid.open_now.load(SeqCst) == 0 && r.dev.open_now.load(SeqCst) == 0).await;
    wait_for("tâche du direct finie", || {
        let _ = r.v.poll_video();
        r.v.streams_running() == 0
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn regarder_un_direct_image_et_son() {
    let mut r = rig();
    video(&mut r.v, &r.sink, |_| {});
    // L'appel (pour jouer le son du direct).
    let mut voice_server = FakeVoice::start().await;
    let voice_token = Secret::new(VOICE_TOKEN.into());
    r.call(Some(&voice_server.endpoint()), &voice_token);
    let mut vc = voice_server.accept().await;
    voice_server.handshake(&mut vc, &[AES], 0).await;
    r.until_voice("Connected", |e| matches!(e, VoiceEvent::Connected)).await;
    wait_for("périphériques ouverts", || r.dev.open_now.load(SeqCst) == 1).await;

    let mut server = FakeVoice::start().await;
    let key = format!("guild:{GUILD}:{CHANNEL}:{OTHER}");
    let tile = Tile::Stream(key.clone());
    r.sink.show(&[(tile.clone(), (1280, 720))]);
    let token = Secret::new("jeton-direct".into());
    let endpoint = server.endpoint();
    let target = StreamTarget { server_id: RTC_SERVER, channel_id: CHANNEL, endpoint: &endpoint, token: &token, session_id: SESSION };
    video(&mut r.v, &r.sink, |w| w.watch = vec![WatchWant { key: &key, owner: OTHER, conn: StreamWant::Open(target) }]);
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    r.until_up("direct connecté", |u| matches!(u, VideoUp::Stream(StreamEvent::Connected { .. }))).await;

    // L'auteur annonce son image : qualité 100 demandée (op 15, tuile en 720p).
    other_camera(&mut c).await;
    assert_eq!(next_op(&mut c, 15).await["d"], json!({ OTHER_VIDEO.to_string(): 100 }));
    let (mut seq, mut counter) = (1u16, 10u32);
    prime(&server, &h, &mut seq, &mut counter).await;
    send_image(&server, &h, &access_unit(true, 1280, 720, 1), &mut seq, 90_000, &mut counter).await;
    wait_for("image du direct", || r.sink.take(&tile, 0).is_some_and(|(_, img)| img.width == 1280)).await;

    // Son de l'auteur : joué par le casque de l'appel.
    c.send(json!({"op": 5, "seq": 3, "d": {"user_id": OTHER.to_string(), "ssrc": OTHER_AUDIO, "speaking": 1}})).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    r.dev.take_peak();
    let mut enc = Engine::new();
    enc.set_gate(Gate::PushToTalk { pressed: true });
    let tone: Vec<f32> = (0..FRAME * 30).map(|i| 0.5 * (i as f32 * 0.04).sin()).collect();
    for (i, pk) in enc.push_capture(&tone).into_iter().enumerate() {
        server.send_audio(&h, OTHER_AUDIO, i as u16, i as u32 * FRAME as u32, 100 + i as u32, &pk).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(r.dev.take_peak() > 0.05, "son du direct joué");

    // Arrêter de regarder : connexion fermée, décodeur arrêté ; l'appel continue.
    video(&mut r.v, &r.sink, |_| {});
    c.expect_closed().await;
    wait_for("décodeur arrêté", || r.vid.decoders_now.load(SeqCst) == 0).await;
    assert!(r.v.in_call());
    r.call(None, &voice_token);
    vc.expect_closed().await;
    wait_for("plus rien ne tourne", || {
        let _ = r.v.poll_video();
        r.v.streams_running() == 0 && r.dev.open_now.load(SeqCst) == 0 && r.vid.open_now.load(SeqCst) == 0
    })
    .await;
}
