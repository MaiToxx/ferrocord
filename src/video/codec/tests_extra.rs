//! VP9 et AV1 reçus d'un appel : de vraies images, mises en paquets RTP
//! comme le fait libwebrtc, passées par la réception (`net::voice::video`)
//! puis par le décodeur de Windows. Sautés, avec un message, si
//! l'extension de Windows manque.
//!
//! VP9 : encodé ici par les « Extensions vidéo VP9 ». AV1 : Windows n'a
//! d'encodeur que matériel ; les images viennent de
//! `tests/fixtures/av1/frames.json`, écrit une fois par `ecrit_jeu_av1`
//! sur une machine qui en a un.

use std::time::Instant;

use super::tests::{pattern, psnr};
use super::*;
use crate::net::voice::video::av1 as rtp_av1;
use crate::net::voice::video::recv::{Assembled, Incoming, Output, Receiver};
use crate::video::{Codec, EncodedFrame};

/// Passe les paquets d'une image par la réception ; rend les images sorties.
fn receive(r: &mut Receiver, seq: &mut u16, ts: u32, payloads: Vec<Vec<u8>>) -> Vec<Assembled> {
    let mut out = Output::default();
    let n = payloads.len();
    for (k, payload) in payloads.into_iter().enumerate() {
        r.push(Incoming { seq: *seq, ts, marker: k + 1 == n, payload }, Instant::now(), &mut out);
        *seq = seq.wrapping_add(1);
    }
    out.frames
}

/// Récepteur calé : il attend la fin d'une image (marqueur) avant de rendre
/// la suivante ; un paquet de fin d'une image précédente le cale.
fn receiver(codec: Codec, seq: &mut u16) -> Receiver {
    let mut r = Receiver::new(codec);
    r.push(Incoming { seq: *seq, ts: 0, marker: true, payload: vec![0] }, Instant::now(), &mut Output::default());
    *seq = seq.wrapping_add(1);
    r
}

/// Paquets RTP VP9 d'une image (RFC 9628) : identifiant d'image sur 15
/// bits, P pour une image intermédiaire, B et E aux extrémités.
fn vp9_packets(frame: &[u8], key: bool, picture: u16, max: usize) -> Vec<Vec<u8>> {
    let chunks: Vec<&[u8]> = frame.chunks(max - 3).collect();
    chunks
        .iter()
        .enumerate()
        .map(|(k, c)| {
            let mut head = 0x80;
            if !key {
                head |= 0x40;
            }
            if k == 0 {
                head |= 0x08;
            }
            if k + 1 == chunks.len() {
                head |= 0x04;
            }
            let mut p = vec![head, 0x80 | (picture >> 8) as u8, picture as u8];
            p.extend_from_slice(c);
            p
        })
        .collect()
}

#[test]
fn vp9_recu_puis_decode() {
    let (Ok(mut enc), Some(mut dec)) = (mf::MfVp8Encoder::vp9(160, 120, 30, 400_000), decoder(Codec::Vp9)) else {
        eprintln!("VP9 de Windows absent : sauté");
        return;
    };
    assert!(supported_decoders().contains(&Codec::Vp9));
    let mut seq = 65_530u16;
    let mut rx = receiver(Codec::Vp9, &mut seq);
    let mut shown = 0;
    for t in 0..8u32 {
        let src = pattern(160, 120, t);
        // Image clé forcée au milieu : un spectateur tardif repart d'elle.
        let encoded = enc.encode(&src, t == 4).expect("encodage");
        for e in encoded {
            assert_eq!(e.codec, Codec::Vp9);
            if t == 0 || t == 4 {
                assert!(e.keyframe, "image clé à {t}");
            }
            let frames = receive(&mut rx, &mut seq, e.ts90k, vp9_packets(&e.data, e.keyframe, 0x1234 + t as u16, 300));
            assert_eq!(frames.len(), 1, "image {t} rendue entière");
            assert_eq!((frames[0].keyframe, &frames[0].data), (e.keyframe, &e.data));
            let got = dec.decode(&EncodedFrame { codec: Codec::Vp9, keyframe: frames[0].keyframe, ts90k: e.ts90k, data: frames[0].data.clone() });
            if let Some(d) = got.unwrap_or_else(|x| panic!("image {t} : {x:?}")) {
                assert_eq!((d.width, d.height), (160, 120));
                let q = psnr(&src, &d);
                assert!(q > 28.0, "PSNR {q:.1} dB à l'image {t}");
                shown += 1;
            }
        }
    }
    assert!(shown >= 7, "{shown} images montrées sur 8");
}

#[test]
fn vp9_flux_abime_sans_plantage() {
    let (Ok(mut enc), Some(mut dec)) = (mf::MfVp8Encoder::vp9(160, 120, 30, 500_000), decoder(Codec::Vp9)) else {
        eprintln!("VP9 de Windows absent : sauté");
        return;
    };
    let frames: Vec<EncodedFrame> = (0..4).flat_map(|t| enc.encode(&pattern(160, 120, t), false).expect("encodage")).collect();
    assert!(!frames.is_empty());
    // Image intermédiaire avant toute image clé : ignorée sans erreur.
    if let Some(inter) = frames.iter().find(|f| !f.keyframe) {
        assert!(matches!(dec.decode(inter), Ok(None)));
    }
    // Une seule image : chaque décodage raté attend sa sortie en tenant le
    // verrou commun des décodeurs VP8 et VP9, ce qui retarde les autres essais.
    for f in frames.iter().take(1) {
        for cut in [3, f.data.len() / 2] {
            let mut bad = f.clone();
            bad.data.truncate(cut);
            let _ = dec.decode(&bad);
            let mut noise = f.clone();
            for b in noise.data.iter_mut().skip(cut) {
                *b = b.wrapping_mul(31).wrapping_add(7);
            }
            let _ = dec.decode(&noise);
        }
    }
    assert!(dec.decode(&EncodedFrame { codec: Codec::Vp9, keyframe: false, ts90k: 0, data: Vec::new() }).is_err());
    assert!(matches!(dec.decode(&EncodedFrame { codec: Codec::Av1, keyframe: true, ts90k: 0, data: vec![1] }), Err(CodecError::WrongCodec(Codec::Av1))));
}

// ------------------------------------------------------------------- AV1

/// Image telle que l'émetteur la donne au paquétiseur après DAVE
/// (libdave, `ProcessFrameAv1`) : sans délimiteur temporel ni remplissage,
/// tailles d'OBU au plus court, dernière OBU sans taille.
fn av1_sender_form(unit: &[u8]) -> Vec<u8> {
    let list: Vec<_> = av1::obus(unit).expect("OBU").into_iter().filter(|(h, _, _)| !matches!((h >> 3) & 0x0f, 2 | 8 | 15)).collect();
    let mut out = Vec::new();
    for (k, (head, ext, payload)) in list.iter().enumerate() {
        if k + 1 == list.len() {
            out.push(head & !0x02);
            out.extend(*ext);
        } else {
            out.push(head | 0x02);
            out.extend(*ext);
            rtp_av1::write_leb128(payload.len(), &mut out);
        }
        out.extend_from_slice(payload);
    }
    out
}

/// Paquets RTP AV1 d'une image (forme de l'émetteur) : les OBU voyagent
/// sans champ de taille, coupées à `max` octets ; trois éléments au plus
/// par paquet, le dernier sans longueur (W), comme libwebrtc.
fn av1_packets(sender: &[u8], key: bool, max: usize) -> Vec<Vec<u8>> {
    let elements: Vec<Vec<u8>> = av1::obus(sender)
        .expect("OBU")
        .into_iter()
        .map(|(head, ext, payload)| {
            let mut e = vec![head & !0x02];
            e.extend(ext);
            e.extend_from_slice(payload);
            e
        })
        .collect();
    let mut packets: Vec<Vec<u8>> = Vec::new();
    let mut cur: Vec<Vec<u8>> = Vec::new();
    let mut z = false;
    let flush = |cur: &mut Vec<Vec<u8>>, z: bool, y: bool, packets: &mut Vec<Vec<u8>>| {
        let first = packets.is_empty();
        let mut p = vec![(u8::from(z) << 7) | (u8::from(y) << 6) | ((cur.len() as u8) << 4) | (u8::from(first && key) << 3)];
        let n = cur.len();
        for (k, e) in cur.drain(..).enumerate() {
            if k + 1 != n {
                rtp_av1::write_leb128(e.len(), &mut p);
            }
            p.extend_from_slice(&e);
        }
        packets.push(p);
    };
    for e in elements {
        let mut rest = e.as_slice();
        loop {
            let used: usize = 1 + cur.iter().map(|c| c.len() + 2).sum::<usize>();
            let room = max.saturating_sub(used + 2);
            if cur.len() == 3 || room == 0 {
                flush(&mut cur, z, false, &mut packets);
                z = false;
                continue;
            }
            if rest.len() <= room {
                cur.push(rest.to_vec());
                break;
            }
            cur.push(rest[..room].to_vec());
            rest = &rest[room..];
            flush(&mut cur, z, true, &mut packets);
            z = true;
        }
    }
    if !cur.is_empty() {
        flush(&mut cur, z, false, &mut packets);
    }
    packets
}

struct Av1Fixture {
    width: u32,
    height: u32,
    /// (horodatage, image clé, unité temporelle sortie de l'encodeur).
    frames: Vec<(u32, bool, Vec<u8>)>,
}

fn av1_fixture() -> Av1Fixture {
    let path = format!("{}/tests/fixtures/av1/frames.json", env!("CARGO_MANIFEST_DIR"));
    let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).expect("jeu d'essai")).expect("json");
    let unhex = |s: &serde_json::Value| -> Vec<u8> {
        let s = s.as_str().expect("hex");
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).expect("hex")).collect()
    };
    let num = |v: &serde_json::Value| v.as_u64().expect("nombre") as u32;
    let frames = v["frames"].as_array().expect("frames").iter().map(|f| (num(&f["ts"]), f["keyframe"] == true, unhex(&f["data"]))).collect();
    Av1Fixture { width: num(&v["width"]), height: num(&v["height"]), frames }
}

#[test]
fn av1_paquets_puis_forme_de_dave() {
    // Sans décodeur : la réception rend exactement ce que l'émetteur a chiffré.
    let fx = av1_fixture();
    assert!(fx.frames.len() >= 6 && fx.frames[0].1);
    for max in [1100, 300, 64] {
        let mut seq = 65_533u16;
        let mut rx = receiver(Codec::Av1, &mut seq);
        for (t, (ts, key, unit)) in fx.frames.iter().enumerate() {
            let sender = av1_sender_form(unit);
            assert_eq!(av1::is_keyframe(&sender), *key, "en-tête de séquence à l'image {t}");
            let packets = av1_packets(&sender, *key, max);
            assert!(packets.iter().all(|p| p.len() <= max), "paquets de {max} octets au plus");
            let frames = receive(&mut rx, &mut seq, *ts, packets);
            assert_eq!(frames.len(), 1, "image {t} rendue entière (paquets de {max})");
            assert_eq!(frames[0].keyframe, *key);
            assert!(frames[0].data == sender, "image {t} : octets identiques à ceux de l'émetteur (paquets de {max})");
            // Données ajoutées par DAVE au bout de la dernière OBU, retirées
            // au déchiffrement : la forme des décodeurs revient à l'identique.
            assert_eq!(av1::temporal_unit(&sender).as_deref(), av1::temporal_unit(unit).as_deref());
        }
        if max == 1100 {
            assert_eq!(av1::sequence_size(&av1_sender_form(&fx.frames[0].2)), Some((fx.width, fx.height)));
        }
    }
    // Une image intermédiaire seule, au début : rien ne sort avant une image clé.
    let mut seq = 0u16;
    let mut rx = receiver(Codec::Av1, &mut seq);
    let (ts, _, unit) = fx.frames.iter().find(|f| !f.1).expect("image intermédiaire");
    assert!(receive(&mut rx, &mut seq, *ts, av1_packets(&av1_sender_form(unit), false, 300)).is_empty());
}

#[test]
fn av1_recu_puis_decode() {
    let Some(mut dec) = decoder(Codec::Av1) else {
        eprintln!("AV1 de Windows absent : sauté");
        return;
    };
    assert!(supported_decoders().contains(&Codec::Av1));
    let fx = av1_fixture();
    let mut seq = 100u16;
    let mut rx = receiver(Codec::Av1, &mut seq);
    let mut shown = 0;
    for (t, (ts, key, unit)) in fx.frames.iter().enumerate() {
        let frames = receive(&mut rx, &mut seq, *ts, av1_packets(&av1_sender_form(unit), *key, 400));
        assert_eq!(frames.len(), 1);
        let got = dec.decode(&EncodedFrame { codec: Codec::Av1, keyframe: frames[0].keyframe, ts90k: *ts, data: frames[0].data.clone() });
        if let Some(d) = got.unwrap_or_else(|x| panic!("image {t} : {x:?}")) {
            assert_eq!((d.width, d.height), (fx.width, fx.height));
            assert!(d.is_consistent());
            let q = psnr(&pattern(fx.width, fx.height, t as u32), &d);
            assert!(q > 25.0, "PSNR {q:.1} dB à l'image {t}");
            shown += 1;
        }
    }
    assert!(shown + 1 >= fx.frames.len(), "{shown} images montrées sur {}", fx.frames.len());
    // Abîmé ou coupé : une erreur ou rien, jamais de plantage.
    for (_, _, unit) in &fx.frames {
        let sender = av1_sender_form(unit);
        for cut in [1, 2, sender.len() / 2] {
            let _ = dec.decode(&EncodedFrame { codec: Codec::Av1, keyframe: true, ts90k: 0, data: sender[..cut].to_vec() });
        }
        let noise: Vec<u8> = sender.iter().enumerate().map(|(i, b)| if i > 12 { b.wrapping_mul(29).wrapping_add(3) } else { *b }).collect();
        let _ = dec.decode(&EncodedFrame { codec: Codec::Av1, keyframe: true, ts90k: 0, data: noise });
    }
}

/// Écrit `tests/fixtures/av1/frames.json` (à relancer à la main, sur une
/// machine à encodeur AV1 matériel : `cargo test --lib ecrit_jeu_av1 --
/// --ignored`) : huit images 320×240 de la mire, clé forcée à la cinquième.
#[test]
#[ignore]
fn ecrit_jeu_av1() {
    let mut enc = mf::MfEncoder::av1(320, 240, 30, 400_000).expect("encodeur AV1 matériel");
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let mut frames = Vec::new();
    let mut t = 0u32;
    // L'encodeur matériel peut rendre une image avec celle d'après.
    while frames.len() < 8 && t < 40 {
        for f in enc.encode(&pattern(320, 240, t), t == 4).expect("encodage") {
            assert_eq!(f.codec, Codec::Av1);
            let key = av1::is_keyframe(&f.data);
            eprintln!("image {} : {} octets, clé {key} (attribut {})", frames.len(), f.data.len(), f.keyframe);
            frames.push(serde_json::json!({"ts": frames.len() as u32 * 3000, "keyframe": key, "data": hex(&f.data)}));
        }
        t += 1;
    }
    assert_eq!(frames.len(), 8);
    let doc = serde_json::json!({
        "source": "synthétique : mire 320x240 encodée par l'encodeur AV1 matériel de Windows (Media Foundation), unités temporelles telles que sorties",
        "width": 320, "height": 240, "frames": frames,
    });
    let path = format!("{}/tests/fixtures/av1/frames.json", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(std::path::Path::new(&path).parent().expect("dossier")).expect("dossier");
    std::fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("écriture");
}
