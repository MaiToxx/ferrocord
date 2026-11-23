//! Vidéo dans la connexion vocale (lot M1) face au faux serveur vocal :
//! négociation, op 12, image envoyée puis retransmise sur NACK, PLI reçu,
//! image d'un autre participant reçue malgré une perte, direct (Go Live).

use crate::support::voice::*;
use crate::support::within;
use ferrocord::net::voice::{
    self,
    e2ee::Passthrough,
    rtp,
    video::{h264, rtcp},
    ConnKind, VoiceCtl, VoiceEvent,
};
use ferrocord::video::{Codec, EncodedFrame};
use serde_json::json;
use tokio::sync::mpsc;

const AES: &str = "aead_aes256_gcm_rtpsize";
const OTHER: u64 = 41_771_983_423_143_937;
const OTHER_VIDEO: u32 = 900;
const OTHER_RTX: u32 = 901;

async fn next(rx: &mut mpsc::Receiver<VoiceEvent>) -> VoiceEvent {
    loop {
        match within("événement vocal", rx.recv()).await {
            Some(VoiceEvent::Ping(_)) => {}
            Some(e) => return e,
            None => panic!("connexion vocale arrêtée sans Closed"),
        }
    }
}

async fn ctl(tx: &mpsc::Sender<VoiceCtl>, c: VoiceCtl) {
    tx.send(c).await.unwrap_or_else(|_| panic!("connexion arrêtée"));
}

fn image(seed: u8, len: usize, key: bool) -> Vec<u8> {
    let mut f = Vec::new();
    if key {
        f.extend_from_slice(&[0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f, 0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    }
    f.extend_from_slice(&[0, 0, 0, 1, if key { 0x65 } else { 0x41 }]);
    f.extend((0..len).map(|i| (i as u8).wrapping_mul(17).wrapping_add(seed) | 1));
    f
}

/// Prochain paquet RTP du client (les RTCP, rapports d'émetteur compris, sont sautés).
async fn next_rtp(server: &FakeVoice, h: &Handshake) -> (u8, bool, u16, u32, u32, Vec<u8>) {
    loop {
        if let Media::Rtp { pt, marker, seq, ts, ssrc, payload } = server.recv_media(&h.cipher).await {
            return (pt, marker, seq, ts, ssrc, payload);
        }
    }
}

/// Prochain NACK envoyé par le client (PLI et autres RTCP sautés).
async fn next_nack(server: &FakeVoice, h: &Handshake) -> (u32, Vec<u16>) {
    loop {
        if let Media::Rtcp(plain) = server.recv_media(&h.cipher).await {
            for f in rtcp::parse(&plain) {
                if let rtcp::Feedback::Nack { media, seqs } = f {
                    return (media, seqs);
                }
            }
        }
    }
}

#[tokio::test]
async fn video_dans_les_deux_sens() {
    let mut server = FakeVoice::start().await;
    let (tx, mut rx) = voice::spawn_with(&tokio::runtime::Handle::current(), server.params(), Box::new(Passthrough), config());
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });

    // Négociation : vidéo proposée, H.264 avec son RTX.
    assert_eq!(h.identify["video"], true);
    assert_eq!(h.identify["streams"][0]["type"], "video");
    assert!(h.identify.get("channel_id").is_none(), "salon vocal : pas de channel_id");
    let codecs = h.select["codecs"].as_array().cloned().unwrap_or_default();
    assert_eq!(codecs[0]["name"], "opus");
    assert_eq!(codecs[1]["name"], "H264");
    assert_eq!(codecs[1]["payload_type"], 103);
    assert_eq!(codecs[1]["rtx_payload_type"], 104);

    // Ready sans streams : vidéo sur SSRC audio + 1, RTX sur + 2.
    let video_ssrc = MY_SSRC + 1;
    let rtx_ssrc = MY_SSRC + 2;
    ctl(&tx, VoiceCtl::VideoState { active: true, width: 1280, height: 720, fps: 30 }).await;
    let op = c.recv_op(12).await;
    assert_eq!(op["d"]["audio_ssrc"], MY_SSRC);
    assert_eq!(op["d"]["video_ssrc"], video_ssrc);
    assert_eq!(op["d"]["rtx_ssrc"], rtx_ssrc);
    assert_eq!(op["d"]["streams"][0]["max_framerate"], 30);

    // Envoi d'une grosse image clé : paquets H.264, marqueur sur le dernier.
    let key = image(3, 60_000, true);
    ctl(&tx, VoiceCtl::Video(EncodedFrame { codec: Codec::H264, keyframe: true, ts90k: 90_000, data: key.clone() })).await;
    let mut payloads = Vec::new();
    let mut seqs = Vec::new();
    loop {
        let (pt, marker, seq, ts, ssrc, payload) = next_rtp(&server, &h).await;
        assert_eq!((pt, ts, ssrc), (103, 90_000, video_ssrc));
        seqs.push(seq);
        payloads.push(payload);
        if marker {
            break;
        }
    }
    assert!(payloads.len() > 50);
    assert_eq!(h264::depacketize(payloads.iter().map(Vec::as_slice)).as_deref(), Some(key.as_slice()));

    // NACK du serveur : le paquet revient par RTX.
    server.send_rtcp_sealed(&h, &rtcp::nack(1, video_ssrc, &[seqs[7]]), 1).await;
    let (pt, _, _, ts, ssrc, payload) = next_rtp(&server, &h).await;
    assert_eq!((pt, ts, ssrc), (104, 90_000, rtx_ssrc));
    assert_eq!(&payload[..2], &seqs[7].to_be_bytes());
    assert_eq!(&payload[2..], payloads[7].as_slice());
    // … et la perte est dite à l'encodeur, qui règle son débit dessus.
    assert_eq!(next(&mut rx).await, VoiceEvent::VideoLoss(1));

    // PLI du serveur : le client demande une image clé à l'encodeur.
    server.send_rtcp_sealed(&h, &rtcp::pli(1, video_ssrc), 2).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::KeyframeRequested);

    // Un autre participant allume sa caméra.
    c.send(json!({"op": 11, "seq": 1, "d": {"user_ids": [OTHER.to_string()]}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::UserJoined(OTHER));
    c.send(json!({"op": 12, "seq": 2, "d": {
        "user_id": OTHER.to_string(), "audio_ssrc": 777, "video_ssrc": OTHER_VIDEO,
        "streams": [{"type": "video", "rid": "100", "ssrc": OTHER_VIDEO, "rtx_ssrc": OTHER_RTX, "active": true, "quality": 100}],
    }})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::VideoState { user_id: OTHER, active: true });

    // Ses paquets : une fin d'image (calage), puis une image clé dont un paquet se perd.
    let theirs = image(9, 30_000, true);
    let parts = h264::packetize(&theirs, 1100);
    let mut counter = 10u32;
    let prev = rtp::header_with(103, true, 499, 87_000, OTHER_VIDEO);
    server.send_sealed(&h, &prev, &[0x41, 0x9a], counter).await;
    let lost = 4usize;
    for (k, p) in parts.iter().enumerate() {
        counter += 1;
        if k == lost {
            continue;
        }
        let hdr = rtp::header_with(103, k + 1 == parts.len(), 500 + k as u16, 90_000, OTHER_VIDEO);
        server.send_sealed(&h, &hdr, p, counter).await;
    }
    let (media, nacked) = next_nack(&server, &h).await;
    assert_eq!((media, nacked), (OTHER_VIDEO, vec![500 + lost as u16]));
    // Retransmission RTX du serveur.
    let mut rtx = (500 + lost as u16).to_be_bytes().to_vec();
    rtx.extend_from_slice(&parts[lost]);
    let hdr = rtp::header_with(104, false, 1, 90_000, OTHER_RTX);
    server.send_sealed(&h, &hdr, &rtx, counter + 1).await;
    match next(&mut rx).await {
        VoiceEvent::Video { user_id, frame } => {
            assert_eq!(user_id, OTHER);
            assert_eq!(frame.codec, Codec::H264);
            assert!(frame.keyframe);
            assert_eq!(frame.ts90k, 90_000);
            assert_eq!(frame.data, theirs);
        }
        e => panic!("image attendue, reçu {e:?}"),
    }

    // Le décodeur demande une image clé : PLI puis FIR vers le serveur.
    ctl(&tx, VoiceCtl::RequestKeyframe(OTHER)).await;
    loop {
        if let Media::Rtcp(plain) = server.recv_media(&h.cipher).await {
            let f = rtcp::parse(&plain);
            if f.contains(&rtcp::Feedback::Pli { media: OTHER_VIDEO }) {
                assert!(f.contains(&rtcp::Feedback::Fir { ssrcs: vec![OTHER_VIDEO] }));
                break;
            }
        }
    }

    // Il coupe sa caméra ; nous aussi.
    c.send(json!({"op": 12, "seq": 3, "d": {"user_id": OTHER.to_string(), "audio_ssrc": 777, "video_ssrc": 0, "streams": []}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::VideoState { user_id: OTHER, active: false });
    ctl(&tx, VoiceCtl::VideoState { active: false, width: 0, height: 0, fps: 0 }).await;
    assert_eq!(c.recv_op(12).await["d"]["video_ssrc"], 0);
    ctl(&tx, VoiceCtl::Disconnect).await;
}

#[tokio::test]
async fn direct_go_live() {
    let mut server = FakeVoice::start().await;
    let mut p = server.params();
    p.kind = ConnKind::Stream { stream_key: format!("guild:{GUILD}:{CHANNEL}:{ME}") };
    assert_eq!(p.dave_group_id(), GUILD - 1);
    let (tx, mut rx) = voice::spawn_with(&tokio::runtime::Handle::current(), p, Box::new(Passthrough), config());
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });
    assert_eq!(h.identify["server_id"], GUILD.to_string());
    assert_eq!(h.identify["channel_id"], CHANNEL.to_string());
    assert_eq!(h.identify["streams"][0]["type"], "screen");
    ctl(&tx, VoiceCtl::VideoState { active: true, width: 1920, height: 1080, fps: 30 }).await;
    // Sur un direct, une op 5 (son du direct) précède toujours l'annonce de la vidéo.
    let speaking = c.recv_op(5).await;
    assert_eq!((speaking["d"]["speaking"].as_u64(), speaking["d"]["ssrc"].as_u64()), (Some(2), Some(u64::from(MY_SSRC))));
    assert_eq!(c.recv_op(12).await["d"]["streams"][0]["type"], "video", "« screen » ne sert que dans l'Identify");
    // Son de l'écran partagé : Speaking « son partagé » avec la première trame.
    ctl(&tx, VoiceCtl::Opus(b"son".to_vec())).await;
    assert_eq!(c.recv_op(5).await["d"]["speaking"], 2);
    assert_eq!(server.recv_audio(&h.cipher).await.3, b"son".to_vec());
    // Le son se tait : les silences partent, mais le Speaking n'est jamais
    // retiré (le direct chargeait sans fin chez ceux qui regardent). La
    // reprise du son et une nouvelle annonce de la vidéo ne le redisent pas.
    ctl(&tx, VoiceCtl::Speaking(false)).await;
    for _ in 0..5 {
        assert_eq!(server.recv_audio(&h.cipher).await.3, rtp::SILENCE.to_vec());
    }
    ctl(&tx, VoiceCtl::Opus(b"encore".to_vec())).await;
    assert_eq!(server.recv_audio(&h.cipher).await.3, b"encore".to_vec());
    ctl(&tx, VoiceCtl::VideoState { active: true, width: 1280, height: 720, fps: 30 }).await;
    loop {
        match c.recv().await {
            Msg::Json(v) if v["op"] == 5 => panic!("Speaking redit ou retiré sur un direct : {v}"),
            Msg::Json(v) if v["op"] == 12 => break,
            _ => {}
        }
    }
    ctl(&tx, VoiceCtl::Disconnect).await;
}

/// VP8 proposé (encodé et décodé) : la Session Update qui choisit VP8 est
/// remontée à l'appelant, puis les images VP8 partent en type de charge 105.
#[tokio::test]
async fn vp8_negocie_puis_envoye() {
    let mut server = FakeVoice::start().await;
    let both = [Codec::H264, Codec::Vp8];
    let cfg = voice::VoiceConfig { video_codecs: voice::video::offers(&both, &both), ..config() };
    let (tx, mut rx) = voice::spawn_with(&tokio::runtime::Handle::current(), server.params(), Box::new(Passthrough), cfg);
    let mut c = server.accept().await;
    let h = server.handshake(&mut c, &[AES], 0).await;
    // Session Description sans `video_codec` : rien à dire, H.264 par défaut.
    assert_eq!(next(&mut rx).await, VoiceEvent::Ready { ssrc: MY_SSRC });
    let codecs = h.select["codecs"].as_array().cloned().unwrap_or_default();
    assert_eq!(codecs.iter().map(|c| c["name"].as_str().unwrap_or("")).collect::<Vec<_>>(), ["opus", "H264", "VP8"]);
    assert_eq!(codecs[2]["payload_type"], 105);
    assert_eq!(codecs[2]["encode"], true);

    c.send(json!({"op": 14, "seq": 1, "d": {"video_codec": "VP8"}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::SendCodec(Codec::Vp8));
    ctl(&tx, VoiceCtl::VideoState { active: true, width: 640, height: 360, fps: 30 }).await;
    c.recv_op(12).await;
    let mut key = vec![0x50, 0x0c, 0x00, 0x9d, 0x01, 0x2a, 0x80, 0x02, 0x68, 0x01];
    key.extend((0..5000u32).map(|i| (i * 7) as u8));
    ctl(&tx, VoiceCtl::Video(EncodedFrame { codec: Codec::Vp8, keyframe: true, ts90k: 3000, data: key.clone() })).await;
    let mut parts = Vec::new();
    loop {
        let (pt, marker, _, ts, ssrc, payload) = next_rtp(&server, &h).await;
        if pt != 105 {
            continue;
        }
        assert_eq!((ts, ssrc), (3000, MY_SSRC + 1));
        parts.push(payload);
        if marker {
            break;
        }
    }
    assert!(parts.len() > 1);
    assert_eq!(voice::video::vp8::depacketize(parts.iter().map(Vec::as_slice)), Some(key));

    // Retour à H.264 : remonté aussi.
    c.send(json!({"op": 14, "seq": 2, "d": {"video_codec": "H264"}})).await;
    assert_eq!(next(&mut rx).await, VoiceEvent::SendCodec(Codec::H264));
    ctl(&tx, VoiceCtl::Disconnect).await;
}
