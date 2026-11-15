//! Orchestrateur du vocal (lot V7) de bout en bout : faux serveur vocal
//! (127.0.0.1) et faux périphérique audio. Rejoindre, audio dans les deux
//! sens, muet, sourdine, déplacement (4014), session expirée (4006),
//! départ ; et l'échec annoncé quand Discord ne répond pas à l'op 4.

use crate::support::audio::{Backend, FakeAudio};
use crate::support::voice::*;
use crate::support::within;
use ferrocord::audio::engine::{Engine, Gate, FRAME};
use ferrocord::net::voice::Secret;
use ferrocord::state::audio_prefs::AudioPrefs;
use ferrocord::state::voice::VoiceEvent;
use ferrocord::state::voice_link::{CallTarget, CallWant, VoiceLink, VoiceWant};
use ferrocord::voice::{Options, Voice};
use serde_json::json;
use std::sync::atomic::Ordering::SeqCst;
use std::sync::Arc;
use std::time::Duration;

const AES: &str = "aead_aes256_gcm_rtpsize";
const OTHER: u64 = 41_771_983_423_143_937;
const OTHER_SSRC: u32 = 777;
const CHANNEL_2: u64 = CHANNEL + 1;

fn prefs() -> AudioPrefs {
    AudioPrefs { noise_suppression: false, echo_cancellation: false, auto_gain: false, auto_threshold: false, threshold_db: -40, ..Default::default() }
}

fn opts(dev: &Arc<FakeAudio>) -> Options {
    Options { voice: config(), backend: Arc::new(Backend(dev.clone())), video: Arc::new(crate::support::video::Video(crate::support::video::FakeVideo::new(dev.clone()))), join_timeout: Duration::from_millis(300), device_check: Duration::from_millis(200) }
}

struct Want {
    channel: u64,
    endpoint: String,
    token: Secret,
    call: u8,
    muted: bool,
    deafened: bool,
    prefs: AudioPrefs,
}

const STOP: u8 = 0;
const WAIT_OP4: u8 = 1;
const OPEN: u8 = 2;
const KEEP: u8 = 3;

impl Want {
    fn new(server: &FakeVoice) -> Self {
        Want { channel: CHANNEL, endpoint: server.endpoint(), token: Secret::new(VOICE_TOKEN.into()), call: OPEN, muted: false, deafened: false, prefs: prefs() }
    }

    fn sync(&self, v: &mut Voice) {
        let target = CallTarget { guild_id: Some(GUILD), channel_id: self.channel, user_id: ME, endpoint: &self.endpoint, session_id: SESSION, token: &self.token };
        let call = match self.call {
            STOP => CallWant::Stop,
            WAIT_OP4 => CallWant::Wait,
            OPEN => CallWant::Open(target),
            _ => CallWant::Keep,
        };
        v.sync(&VoiceWant { call, muted: self.muted, deafened: self.deafened, audio: &self.prefs });
    }
}

/// Relève les événements jusqu'à celui que `pred` reconnaît.
async fn until(v: &mut Voice, what: &str, mut pred: impl FnMut(&VoiceEvent) -> bool) -> VoiceEvent {
    within(what, async {
        loop {
            if let Some(e) = v.poll().into_iter().find(|e| pred(e)) {
                return e;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

/// Op 5 du client avec ce drapeau `speaking`.
async fn speaking_op(c: &mut VoiceConn, on: u64) {
    within("op 5 du client", async {
        loop {
            let v = c.recv_op(5).await;
            if v["d"]["speaking"] == on {
                return;
            }
        }
    })
    .await
}

/// L'autre participant parle : `n` trames Opus d'un sinus, au rythme réel.
async fn other_speaks(server: &FakeVoice, h: &Handshake, first_seq: u16, n: usize) {
    let mut enc = Engine::new();
    enc.set_gate(Gate::PushToTalk { pressed: true });
    let tone: Vec<f32> = (0..FRAME * n).map(|i| 0.5 * (i as f32 * 0.04).sin()).collect();
    for (i, pk) in enc.push_capture(&tone).into_iter().enumerate() {
        let seq = first_seq.wrapping_add(i as u16);
        server.send_audio(h, OTHER_SSRC, seq, u32::from(seq) * FRAME as u32, u32::from(seq), &pk).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_for(what: &str, mut f: impl FnMut() -> bool) {
    within(what, async {
        while !f() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn appel_complet_de_bout_en_bout() {
    let dev = FakeAudio::new();
    let mut server = FakeVoice::start().await;
    let mut v = Voice::new(tokio::runtime::Handle::current(), opts(&dev), Arc::new(|| {}));
    let mut want = Want::new(&server);
    assert_eq!(dev.open_now.load(SeqCst), 0, "rien d'ouvert avant l'appel");

    // Rejoindre : connexion, puis périphériques ouverts.
    want.sync(&mut v);
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    assert_eq!(h.identify["max_dave_protocol_version"], 1, "DAVE proposé");
    until(&mut v, "Connected", |e| matches!(e, VoiceEvent::Connected)).await;
    wait_for("périphériques ouverts", || dev.open_now.load(SeqCst) == 1).await;
    want.call = KEEP;
    want.sync(&mut v);

    // J'émets : Speaking puis de l'Opus chiffré.
    dev.set_mic(0.5);
    speaking_op(&mut c, 1).await;
    let (_, _, ssrc, opus) = server.recv_audio(&h.cipher).await;
    assert_eq!(ssrc, MY_SSRC);
    assert!(opus.len() > 3, "vraie trame Opus");
    until(&mut v, "je parle", |e| matches!(e, VoiceEvent::Speaking { user_id: ME, speaking: true })).await;

    // J'entends l'autre : il parle et sa voix sort du faux casque.
    c.send(json!({"op": 11, "seq": 1, "d": {"user_ids": [OTHER.to_string()]}})).await;
    c.send(json!({"op": 5, "seq": 2, "d": {"user_id": OTHER.to_string(), "ssrc": OTHER_SSRC, "speaking": 1}})).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    dev.take_peak();
    other_speaks(&server, &h, 0, 40).await;
    assert!(dev.take_peak() > 0.05, "voix de l'autre jouée");
    until(&mut v, "l'autre parle", |e| matches!(e, VoiceEvent::Speaking { user_id: OTHER, speaking: true })).await;

    // Muet : arrêt de l'émission malgré le micro.
    want.muted = true;
    want.sync(&mut v);
    speaking_op(&mut c, 0).await;
    until(&mut v, "je ne parle plus", |e| matches!(e, VoiceEvent::Speaking { user_id: ME, speaking: false })).await;
    want.muted = false;
    want.sync(&mut v);
    speaking_op(&mut c, 1).await;

    // Sourdine : plus rien n'est joué.
    want.deafened = true;
    want.sync(&mut v);
    tokio::time::sleep(Duration::from_millis(100)).await;
    dev.take_peak();
    other_speaks(&server, &h, 100, 30).await;
    assert_eq!(dev.take_peak(), 0.0, "sourdine");
    want.deafened = false;
    want.sync(&mut v);

    // Déplacé par un modérateur : 4014, puis Discord donne le nouveau salon.
    c.close(4014).await;
    until(&mut v, "Closed après 4014", |e| matches!(e, VoiceEvent::Closed)).await;
    wait_for("fin de l'appel", || !v.in_call()).await;
    let mut server2 = FakeVoice::start().await;
    want = Want { channel: CHANNEL_2, endpoint: server2.endpoint(), token: Secret::new("jeton-2".into()), ..want };
    want.call = OPEN;
    want.sync(&mut v);
    let mut c2 = server2.accept().await;
    let h2 = server2.handshake(&mut c2, &[AES], 0).await;
    assert_eq!(h2.identify["token"], "jeton-2");
    until(&mut v, "Connected dans le nouveau salon", |e| matches!(e, VoiceEvent::Connected)).await;
    want.call = KEEP;
    want.sync(&mut v);

    // Session expirée : l'interface doit renvoyer l'op 4.
    c2.close(4006).await;
    until(&mut v, "Expired", |e| matches!(e, VoiceEvent::Expired)).await;
    wait_for("fin de l'appel", || !v.in_call()).await;
    // L'audio de l'ancienne session, encore en file, n'intéresse plus le serveur.
    tokio::time::sleep(Duration::from_millis(100)).await;
    let mut buf = [0u8; 2048];
    while server2.udp.try_recv_from(&mut buf).is_ok() {}
    want.token = Secret::new("jeton-3".into());
    want.call = OPEN;
    want.sync(&mut v);
    let mut c3 = server2.accept().await;
    server2.handshake(&mut c3, &[AES], 0).await;
    until(&mut v, "Connected après la nouvelle session", |e| matches!(e, VoiceEvent::Connected)).await;

    // Départ : connexion fermée, périphériques lâchés, plus rien ne tourne.
    want.call = STOP;
    want.sync(&mut v);
    assert!(!v.in_call());
    c3.expect_closed().await;
    wait_for("périphériques fermés", || dev.open_now.load(SeqCst) == 0).await;
    let opened = dev.opened.load(SeqCst);
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(dev.opened.load(SeqCst), opened, "rien ne se rouvre au repos");
    assert!(v.poll().iter().all(|e| !matches!(e, VoiceEvent::Failed(_))));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn op4_sans_reponse_annonce_l_echec() {
    let dev = FakeAudio::new();
    let server = FakeVoice::start().await;
    let mut v = Voice::new(tokio::runtime::Handle::current(), opts(&dev), Arc::new(|| {}));
    let mut want = Want::new(&server);
    want.call = WAIT_OP4;
    want.sync(&mut v);
    let e = until(&mut v, "échec après l'attente", |e| matches!(e, VoiceEvent::Failed(_))).await;
    let VoiceEvent::Failed(why) = e else { unreachable!() };
    assert!(!why.is_empty());
    assert_eq!(dev.opened.load(SeqCst), 0);

    // Réponse arrivée à temps : pas d'échec.
    want.call = STOP;
    want.sync(&mut v);
    want.call = WAIT_OP4;
    want.sync(&mut v);
    want.call = STOP;
    want.sync(&mut v);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(v.poll().iter().all(|e| !matches!(e, VoiceEvent::Failed(_))));
}
