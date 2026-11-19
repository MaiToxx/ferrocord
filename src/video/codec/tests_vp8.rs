//! VP8 par les « Extensions vidéo VP9 » de Windows : encodage puis décodage
//! de mires. Sautés, avec un message, si les extensions manquent.

use std::time::Instant;

use super::tests::{luma, pattern, psnr};
use super::*;
use crate::video::{Codec, EncodedFrame, Frame, FrameData};

fn codecs(w: u32, h: u32) -> Option<(Box<dyn Encoder>, Box<dyn Decoder>)> {
    match (vp8_encoder(w, h, 30, 1_500_000), vp8_decoder()) {
        (Some(e), Some(d)) => Some((e, d)),
        _ => {
            eprintln!("VP8 de Windows absent : sauté");
            assert!(!supported_decoders().contains(&Codec::Vp8) || vp8_decoder().is_some());
            None
        }
    }
}

fn one(enc: &mut dyn Encoder, f: &Frame, key: bool) -> EncodedFrame {
    let mut out = enc.encode(f, key).expect("encodage");
    assert_eq!(out.len(), 1, "une image encodée par image d'entrée (aucun retard)");
    out.remove(0)
}

#[test]
fn vp8_aller_retour_psnr() {
    let Some((mut enc, mut dec)) = codecs(320, 240) else { return };
    assert_eq!(supported_decoders(), [Codec::H264, Codec::Vp8]);
    for t in 0..20 {
        let src = pattern(320, 240, t);
        let e = one(enc.as_mut(), &src, false);
        assert_eq!(e.codec, Codec::Vp8);
        if t == 0 {
            assert!(e.keyframe);
            assert_eq!(vp8::keyframe_size(&e.data), Some((320, 240)));
        }
        assert_eq!(e.keyframe, e.data[0] & 1 == 0);
        assert_eq!(e.ts90k, (u64::from(t) * 33_333 * 9 / 100) as u32);
        let got = dec.decode(&e).unwrap_or_else(|x| panic!("image {t} : {x:?}"));
        if got.is_none() {
            eprintln!("DIAG vp8 image {t} : rien ; {} octets, tete {:02x?}, cle {}", e.data.len(), &e.data[..e.data.len().min(10)], e.keyframe);
            let mut suite = Vec::new();
            for u in t + 1..t + 6 {
                let e2 = one(enc.as_mut(), &pattern(320, 240, u), false);
                let r = dec.decode(&e2);
                suite.push(format!("{u}:{}o:{}", e2.data.len(), match &r { Ok(Some(f)) => format!("image pts {}", f.pts_us), Ok(None) => "rien".into(), Err(x) => format!("{x:?}") }));
            }
            let mut neuf = vp8_decoder().expect("décodeur");
            panic!("image {t} : rien décodé ; suite {suite:?} ; décodeurs {:?} ; décodeur neuf sur cette image : {:?}", supported_decoders(), neuf.decode(&e).map(|f| f.is_some()));
        }
        let d = got.expect("image");
        assert_eq!((d.width, d.height), (320, 240));
        assert!(d.is_consistent());
        let q = psnr(&src, &d);
        assert!(q > 30.0, "PSNR {q:.1} dB à l'image {t}");
    }
}

#[test]
fn vp8_image_cle_forcee_et_spectateur_tardif() {
    let Some((mut enc, mut dec)) = codecs(320, 240) else { return };
    let frames: Vec<EncodedFrame> = (0..6).map(|t| one(enc.as_mut(), &pattern(320, 240, t), t == 4)).collect();
    let keys: Vec<bool> = frames.iter().map(|f| f.keyframe).collect();
    assert_eq!(keys, [true, false, false, false, true, false]);
    let mut late = vp8_decoder().expect("décodeur");
    // Une image intermédiaire seule ne montre rien ; l'image clé, si.
    assert!(late.decode(&frames[3]).expect("décodage").is_none());
    assert!(late.decode(&frames[4]).expect("décodage").is_some());
    assert!(late.decode(&frames[5]).expect("décodage").is_some());
    for f in &frames {
        assert!(dec.decode(f).expect("décodage").is_some());
    }
}

#[test]
fn vp8_changement_de_resolution_et_bgra() {
    let Some((mut enc, mut dec)) = codecs(320, 240) else { return };
    for (t, (w, h)) in [(320, 240), (640, 360), (640, 360), (176, 144)].into_iter().enumerate() {
        let src = pattern(w, h, t as u32);
        let e = one(enc.as_mut(), &src, false);
        assert_eq!(e.keyframe, t != 2, "image {t}");
        let d = dec.decode(&e).expect("décodage").expect("image");
        assert_eq!((d.width, d.height), (w, h));
        assert!(psnr(&src, &d) > 30.0);
    }
    // Impaire : rognée ; BGRA gris : la luminance retombe sur celle de la conversion.
    let d = dec.decode(&one(enc.as_mut(), &pattern(321, 241, 0), false)).expect("décodage").expect("image");
    assert_eq!((d.width, d.height), (320, 240));
    let bgra = Frame { width: 320, height: 240, pts_us: 33_333, data: FrameData::Bgra([90, 90, 90, 255].repeat(320 * 240)) };
    let d = dec.decode(&one(enc.as_mut(), &bgra, false)).expect("décodage").expect("image");
    let mean = luma(&d).iter().map(|&p| u32::from(p)).sum::<u32>() / (320 * 240);
    assert!((92..=96).contains(&mean), "luminance moyenne {mean}");
}

#[test]
fn vp8_flux_corrompu_sans_plantage() {
    let Some((mut enc, mut dec)) = codecs(320, 240) else { return };
    let key = one(enc.as_mut(), &pattern(320, 240, 0), false);
    let next = one(enc.as_mut(), &pattern(320, 240, 1), false);
    let bad = |data: Vec<u8>| EncodedFrame { codec: Codec::Vp8, keyframe: false, ts90k: 0, data };
    assert!(matches!(dec.decode(&bad(Vec::new())), Err(CodecError::Corrupt(_))));
    assert_eq!(dec.decode(&EncodedFrame { codec: Codec::H264, ..key.clone() }), Err(CodecError::WrongCodec(Codec::H264)));
    let mut seed = 0x2545_f491_u32;
    let mut rnd = || {
        seed ^= seed << 13;
        seed ^= seed >> 17;
        seed ^= seed << 5;
        seed
    };
    for _ in 0..50 {
        let n = (rnd() % 2000) as usize + 1;
        let mut data: Vec<u8> = (0..n).map(|_| rnd() as u8).collect();
        if rnd() % 2 == 0 && n > 10 {
            // En-tête d'image clé valable, contenu au hasard.
            data[..10].copy_from_slice(&key.data[..10]);
        }
        let _ = dec.decode(&bad(data));
    }
    let _ = dec.decode(&bad(key.data[..key.data.len() / 2].to_vec()));
    let mut flipped = key.data.clone();
    for i in (20..flipped.len()).step_by(13) {
        flipped[i] ^= 0x5a;
    }
    let _ = dec.decode(&bad(flipped));
    // Le même décodeur repart sur une image clé saine.
    let d = dec.decode(&key).expect("décodage de la clé").expect("image");
    assert!(psnr(&pattern(320, 240, 0), &d) > 30.0);
    assert!(dec.decode(&next).expect("décodage").is_some());
}

#[test]
fn vp8_debit_reglable() {
    // Mire mobile (le bruit pur bute sur le quantificateur maximal de libvpx).
    let Some(mut enc) = vp8_encoder(320, 240, 30, 2_000_000) else { return };
    let total = |enc: &mut dyn Encoder, from: u32| -> usize { (from..from + 30).flat_map(|t| enc.encode(&pattern(320, 240, t), false).expect("encodage")).map(|e| e.data.len()).sum() };
    let high = total(enc.as_mut(), 0);
    enc.set_bitrate(150_000);
    let first = enc.encode(&pattern(320, 240, 30), true).expect("encodage");
    assert!(first.iter().any(|e| e.keyframe));
    let low = total(enc.as_mut(), 31);
    // Marge large : sous charge (suite complète en parallèle), le mode temps
    // réel de l'encodeur baisse la qualité à 2 Mb/s ; rapport observé 2,8.
    assert!(low * 2 < high,"débit non appliqué : {high} octets à 2 Mb/s, {low} à 150 kb/s");
}

/// Mesure indicative (affichée avec `--nocapture`) : temps par image en 720p30.
#[test]
fn bench_vp8_720p() {
    let (w, h, n) = (1280, 720, 30u32);
    let Some((mut enc, mut dec)) = codecs(w, h) else { return };
    enc.set_bitrate(1_400_000);
    let src: Vec<Frame> = (0..n).map(|t| pattern(w, h, t)).collect();
    let t = Instant::now();
    let out: Vec<EncodedFrame> = src.iter().flat_map(|f| enc.encode(f, false).expect("encodage")).collect();
    let ms = t.elapsed().as_secs_f64() * 1e3 / f64::from(n);
    eprintln!("720p encodage VP8 : {ms:.1} ms/image, {} Kio pour {n} images", out.iter().map(|f| f.data.len()).sum::<usize>() / 1024);
    let t = Instant::now();
    let shown = out.iter().filter(|f| matches!(dec.decode(f), Ok(Some(_)))).count();
    let ms = t.elapsed().as_secs_f64() * 1e3 / out.len().max(1) as f64;
    eprintln!("720p décodage VP8 : {ms:.1} ms/image, {shown}/{} images rendues", out.len());
}

/// Écrit `tests/fixtures/vp8/rtp_stream.json` (à relancer à la main :
/// `cargo test --lib ecrit_jeu_rtp_vp8 -- --ignored`) : six images 96×64
/// encodées ici (clé forcée à la quatrième), puis mises en paquets RTP comme
/// le fait libwebrtc (descripteur étendu : X, I sur 15 bits, L, T ; 300
/// octets de charge au plus), numéros de séquence qui rebouclent.
#[test]
#[ignore]
fn ecrit_jeu_rtp_vp8() {
    let Some((mut enc, _)) = codecs(96, 64) else { return };
    let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    let (mut frames, mut packets) = (Vec::new(), Vec::new());
    let mut seq = 65_530u16;
    for t in 0..6u32 {
        let f = one(enc.as_mut(), &pattern(96, 64, t), t == 3);
        let picture_id = 0x1234 + t as u16;
        let chunks: Vec<&[u8]> = f.data.chunks(300 - 6).collect();
        for (k, c) in chunks.iter().enumerate() {
            let s = if k == 0 { 0x10 } else { 0 };
            let mut p = vec![0x80 | s, 0xe0, 0x80 | (picture_id >> 8) as u8, picture_id as u8, t as u8, 0x20];
            p.extend_from_slice(c);
            packets.push(serde_json::json!({"seq": seq, "ts": f.ts90k, "marker": k + 1 == chunks.len(), "payload": hex(&p)}));
            seq = seq.wrapping_add(1);
        }
        frames.push(serde_json::json!({"ts": f.ts90k, "keyframe": f.keyframe, "data": hex(&f.data)}));
    }
    let doc = serde_json::json!({
        "source": "synthétique : mire 96x64 encodée par l'encodeur VP8 de Windows, paquets RTP selon la RFC 7741",
        "width": 96, "height": 64, "payload_type": 105, "frames": frames, "packets": packets,
    });
    let path = format!("{}/tests/fixtures/vp8/rtp_stream.json", env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(std::path::Path::new(&path).parent().expect("dossier")).expect("dossier");
    std::fs::write(&path, serde_json::to_string_pretty(&doc).expect("json")).expect("écriture");
}
