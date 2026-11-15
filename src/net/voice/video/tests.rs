use super::*;
use std::time::Duration;

const A: u64 = 1001;
const B: u64 = 2002;

fn h264_image(seed: u8, len: usize, key: bool) -> Vec<u8> {
    let mut f = Vec::new();
    if key {
        f.extend_from_slice(&[0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f, 0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    }
    f.extend_from_slice(&[0, 0, 0, 1, if key { 0x65 } else { 0x41 }]);
    f.extend((0..len).map(|i| (i as u8).wrapping_mul(13).wrapping_add(seed) | 1));
    f
}

/// A annonce sa vidéo, B l'apprend par l'op 12 (relayé par le serveur avec `user_id`).
fn pair() -> (Link, Link) {
    let mut a = Link::new(default_offers(), false);
    a.on_ready(&json!({"ssrc": 10, "streams": [{"type": "video", "ssrc": 100, "rtx_ssrc": 101, "rid": "100", "quality": 100, "active": false}]}), 10);
    a.on_session(&json!({"audio_codec": "opus", "video_codec": "H264"}));
    let mut op = a.set_state(true, 1280, 720, 30);
    assert_eq!(op["op"], 12);
    assert_eq!(op["d"]["video_ssrc"], 100);
    assert_eq!(op["d"]["rtx_ssrc"], 101);
    assert_eq!(op["d"]["streams"][0]["max_resolution"], json!({"type": "fixed", "width": 1280, "height": 720}));
    op["d"]["user_id"] = json!(A.to_string());
    let mut b = Link::new(default_offers(), false);
    b.on_ready(&json!({"ssrc": 20}), 20);
    assert_eq!(b.video_ssrc(), 21, "sans streams : SSRC audio + 1");
    assert_eq!(b.on_video_op(&op["d"], B), Some((A, true)));
    assert!(b.owns(100) && b.owns(101) && !b.owns(10));
    (a, b)
}

fn deliver(b: &mut Link, p: &RtpOut, now: Instant) -> Option<Received> {
    let h = &p.header;
    let seq = u16::from_be_bytes([h[2], h[3]]);
    let ts = u32::from_be_bytes([h[4], h[5], h[6], h[7]]);
    let ssrc = u32::from_be_bytes([h[8], h[9], h[10], h[11]]);
    b.on_rtp(RtpMeta { ssrc, payload_type: h[1] & 0x7F, seq, ts, marker: h[1] & 0x80 != 0 }, &p.payload, now)
}

#[test]
fn codecs_proposes() {
    let offers = vec![CodecOffer::new(Codec::Av1, false, true), CodecOffer::new(Codec::H264, true, true), CodecOffer::new(Codec::Vp8, false, true)];
    let c = select_codecs(&offers);
    assert_eq!(c[0], json!({"name": "opus", "type": "audio", "priority": 1000, "payload_type": 120}));
    assert_eq!(c[1], json!({"name": "H264", "type": "video", "priority": 1000, "payload_type": 103, "rtx_payload_type": 104, "encode": true, "decode": true}));
    assert_eq!(c[2]["name"], "VP8");
    assert_eq!(c[2]["priority"], 2000);
    assert_eq!(c.as_array().map(Vec::len), Some(3), "AV1 sans paquétiseur : non proposé");
    assert!(c.as_array().into_iter().flatten().all(|c| c["payload_type"] != 96 && c["rtx_payload_type"] != 96));
    assert_eq!(identify_streams(true)[0]["type"], "screen");
    assert!(!Link::new(Vec::new(), false).enabled());
}

#[test]
fn coupure_et_depart() {
    let (mut a, mut b) = pair();
    let off = a.set_state(false, 0, 0, 0);
    assert_eq!(off["d"]["video_ssrc"], 0);
    assert_eq!(off["d"]["streams"], json!([]));
    assert_eq!(a.send_frame(Codec::H264, 0, &h264_image(1, 10, true), Instant::now()).0.len(), 0, "vidéo coupée : rien ne part");
    assert_eq!(b.on_video_op(&json!({"user_id": A.to_string(), "audio_ssrc": 10, "video_ssrc": 0, "streams": []}), B), Some((A, false)));
    assert!(!b.owns(100));
    let (_, mut b) = pair();
    b.user_left(A);
    assert!(!b.owns(100) && !b.owns(101));
}

#[test]
fn grosses_images_pertes_et_retransmissions() {
    let (mut a, mut b) = pair();
    let t = Instant::now();
    let images: Vec<Vec<u8>> = (0..6).map(|i| h264_image(i, if i == 0 { 120_000 } else { 9_000 }, i == 0)).collect();
    let mut pkts = Vec::new();
    for (i, img) in images.iter().enumerate() {
        let (p, sr) = a.send_frame(Codec::H264, 3000 * i as u32, img, t);
        assert_eq!(sr.is_some(), i == 0, "un seul rapport d'émetteur en 5 s");
        pkts.extend(p);
    }
    // Calage du récepteur : un paquet marqué juste avant le flux (fin d'une image précédente).
    let first = u16::from_be_bytes([pkts[0].header[2], pkts[0].header[3]]);
    let r = b.on_rtp(RtpMeta { ssrc: 100, payload_type: 103, seq: first.wrapping_sub(1), ts: 0, marker: true }, &[0x41, 1], t).unwrap_or_default();
    assert!(r.frames.is_empty());
    // Deux paquets perdus : un dans l'image clé, un dans la troisième image.
    let lost = [5usize, pkts.len() - 12];
    let t1 = t + Duration::from_secs(1);
    let mut frames = Vec::new();
    let mut feedback = Vec::new();
    for (k, p) in pkts.iter().enumerate() {
        if lost.contains(&k) {
            continue;
        }
        let r = deliver(&mut b, p, t1).unwrap_or_default();
        assert_eq!(r.user_id, A);
        frames.extend(r.frames);
        feedback.extend(r.rtcp);
    }
    assert!(frames.is_empty(), "tout attend l'image clé incomplète");
    assert!(!feedback.is_empty());
    // A reçoit les NACK et retransmet par RTX.
    let mut rtx = Vec::new();
    for f in &feedback {
        let fb = a.on_rtcp(f);
        rtx.extend(fb.retransmit);
    }
    assert_eq!(rtx.len(), 2);
    assert!(rtx.iter().all(|p| p.header[1] & 0x7F == 104 && p.header[8..12] == 101u32.to_be_bytes()));
    for p in &rtx {
        frames.extend(deliver(&mut b, p, t1).unwrap_or_default().frames);
    }
    assert_eq!(frames.len(), images.len());
    assert!(frames[0].keyframe && !frames[1].keyframe);
    for (f, img) in frames.iter().zip(&images) {
        assert_eq!(&f.data, img);
    }
    // Retransmission en double : ignorée.
    assert!(deliver(&mut b, &rtx[0], t1).unwrap_or_default().frames.is_empty());
}

#[test]
fn demandes_d_image_cle_dans_les_deux_sens() {
    let (mut a, mut b) = pair();
    let req = b.request_keyframe(A).unwrap_or_default();
    let parsed = rtcp::parse(&req);
    assert_eq!(parsed.len(), 2, "PLI et FIR");
    assert!(a.on_rtcp(&req).keyframe);
    // Un PLI pour une autre SSRC ne concerne pas A.
    assert!(!a.on_rtcp(&rtcp::pli(5, 999)).keyframe);
    assert!(a.on_rtcp(&rtcp::fir(5, 100, 1)).keyframe);
    a.set_state(false, 0, 0, 0);
    assert!(!a.on_rtcp(&rtcp::pli(5, 100)).keyframe, "vidéo coupée : pas d'image clé à forcer");
    assert!(b.request_keyframe(9999).is_none());
}

#[test]
fn codec_non_negocie_ou_inconnu() {
    let (mut a, mut b) = pair();
    assert!(a.send_frame(Codec::Vp8, 0, &[0x10; 30], Instant::now()).0.is_empty(), "VP8 non négocié");
    assert!(b.on_rtp(RtpMeta { ssrc: 100, payload_type: 77, seq: 1, ts: 0, marker: true }, &[1], Instant::now()).is_none(), "type de charge inconnu");
}

#[test]
fn media_sink_wants() {
    let (_, mut b) = pair();
    // Op 15 : clé = SSRC vidéo principale de A (pas RTX), valeur bornée à 100.
    assert_eq!(b.set_wants(&[(A, 250)]), Some(json!({"op": 15, "d": {"100": 100}})));
    assert_eq!(b.set_wants(&[(A, 100)]), None, "rien de changé");
    assert_eq!(b.set_wants(&[(A, 0)]), Some(json!({"op": 15, "d": {"100": 0}})));
    // Personne sans SSRC connue : retenue, envoyée après son op 12.
    assert_eq!(b.set_wants(&[(3003, 50)]), Some(json!({"op": 15, "d": {"100": 0}})));
    assert!(b.wanted(3003) && !b.wanted(4004));
    b.on_video_op(&json!({"user_id": "3003", "audio_ssrc": 30, "video_ssrc": 300}), B);
    assert_eq!(b.wants_op(), Some(json!({"op": 15, "d": {"100": 0, "300": 50}})));
}

#[test]
fn offre_vp8_et_bascule_du_codec_d_envoi() {
    let both = [Codec::H264, Codec::Vp8];
    let all = offers(&both, &both);
    let c = select_codecs(&all);
    assert_eq!(c[1]["name"], "H264", "H.264 d'abord");
    assert_eq!(c[2], json!({"name": "VP8", "type": "video", "priority": 2000, "payload_type": 105, "rtx_payload_type": 106, "encode": true, "decode": true}));
    // VP8 décodé seulement ; AV1 jamais proposé (pas de paquétiseur).
    assert_eq!(offers(&[Codec::H264], &[Codec::H264, Codec::Vp8, Codec::Av1])[1], CodecOffer::new(Codec::Vp8, false, true));
    assert_eq!(offers(&[Codec::H264], &[Codec::H264]), default_offers());

    let mut a = Link::new(all.clone(), false);
    a.on_ready(&json!({"ssrc": 10, "streams": [{"ssrc": 100, "rtx_ssrc": 101}]}), 10);
    assert_eq!(a.send_codec(), Some(Codec::H264), "avant la Session Description : le premier encodable");
    a.on_session(&json!({"video_codec": "VP8"}));
    assert_eq!(a.send_codec(), Some(Codec::Vp8));
    let mut op = a.set_state(true, 640, 360, 30);
    op["d"]["user_id"] = json!(A.to_string());
    let mut b = Link::new(all, false);
    b.on_ready(&json!({"ssrc": 20}), 20);
    b.on_video_op(&op["d"], B);
    let now = Instant::now();
    assert!(a.send_frame(Codec::H264, 0, &h264_image(1, 50, true), now).0.is_empty(), "H.264 n'est plus le codec négocié");
    let mut key = vec![0x50, 0x0c, 0x00, 0x9d, 0x01, 0x2a, 0x80, 0x02, 0x68, 0x01];
    key.extend((0..3000u32).map(|i| i as u8));
    let (pkts, _) = a.send_frame(Codec::Vp8, 0, &key, now);
    assert!(pkts.len() > 2 && pkts.iter().all(|p| p.header[1] & 0x7f == 105));
    // Le premier paquet ferme une image factice : B se cale sur la suivante.
    let mut frames = Vec::new();
    for p in &pkts {
        let r = deliver(&mut b, p, now).expect("paquet VP8 accepté");
        assert_eq!(r.codec, Some(Codec::Vp8));
        frames.extend(r.frames);
    }
    let (pkts, _) = a.send_frame(Codec::Vp8, 3000, &key, now);
    for p in &pkts {
        frames.extend(deliver(&mut b, p, now).expect("paquet").frames);
    }
    assert_eq!(frames.last().map(|f| (f.keyframe, f.data.clone())), Some((true, key)));
    // Session Update : retour à H.264.
    a.on_session(&json!({"video_codec": "H264"}));
    assert_eq!(a.send_codec(), Some(Codec::H264));
}
