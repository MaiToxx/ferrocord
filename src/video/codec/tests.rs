//! Encodage puis décodage de mires synthétiques, pour chaque paire
//! encodeur/décodeur présente sur la machine : OpenH264 (toujours là),
//! l'encodeur matériel et le décodeur de Windows (sautés, avec un message,
//! s'ils manquent). Les paires croisées vérifient que les flux s'échangent.

use std::time::Instant;

use super::*;
use crate::video::{Codec, EncodedFrame, Frame, FrameData};

type MakeEnc = fn(u32, u32, u32, u32) -> Option<Box<dyn Encoder>>;
type MakeDec = fn() -> Option<Box<dyn Decoder>>;

struct Pair {
    name: String,
    enc: MakeEnc,
    dec: MakeDec,
}

/// Paires disponibles ; les composants absents sont signalés puis sautés.
fn pairs() -> Vec<Pair> {
    let encs: [(&str, MakeEnc); 2] = [("OpenH264", software_encoder), ("matériel MF", hardware_encoder)];
    let decs: [(&str, MakeDec); 2] = [("OpenH264", software_decoder), ("Windows MF", system_decoder)];
    let mut out = Vec::new();
    for (en, e) in encs {
        if e(320, 240, 30, 1_000_000).is_none() {
            eprintln!("encodeur {en} absent : sauté");
            continue;
        }
        for (dn, d) in decs {
            if d().is_none() {
                eprintln!("décodeur {dn} absent : sauté");
                continue;
            }
            out.push(Pair { name: format!("{en} → {dn}"), enc: e, dec: d });
        }
    }
    out
}

impl Pair {
    fn make(&self) -> (Box<dyn Encoder>, Box<dyn Decoder>) {
        ((self.enc)(320, 240, 30, 1_500_000).expect("encodeur"), (self.dec)().expect("décodeur"))
    }
}

/// Mire mobile : dégradés et damier qui se déplacent avec `t`.
pub(super) fn pattern(w: u32, h: u32, t: u32) -> Frame {
    let (wu, hu) = (w as usize, h as usize);
    let (cw, ch) = (wu.div_ceil(2), hu.div_ceil(2));
    let mut y = vec![0u8; wu * hu];
    for (i, p) in y.iter_mut().enumerate() {
        let (x, r) = ((i % wu) as u32 + t * 3, (i / wu) as u32 + t);
        let check = if ((x / 16) + (r / 16)) % 2 == 0 { 40 } else { 0 };
        *p = (16 + (x * 2 + r) % 180 + check) as u8;
    }
    let u = (0..cw * ch).map(|i| (64 + (i % cw) * 128 / cw.max(1)) as u8).collect();
    let v = (0..cw * ch).map(|i| (64 + (i / cw) * 128 / ch.max(1)) as u8).collect();
    Frame { width: w, height: h, pts_us: u64::from(t) * 33_333, data: FrameData::I420 { y, u, v } }
}

pub(super) fn luma(f: &Frame) -> &[u8] {
    match &f.data {
        FrameData::I420 { y, .. } => y,
        FrameData::Bgra(_) => panic!("I420 attendu"),
    }
}

/// PSNR de la luminance, sur la zone commune (le décodé peut être rogné d'une colonne/ligne).
pub(super) fn psnr(a: &Frame, b: &Frame) -> f64 {
    let (w, h) = (a.width.min(b.width) as usize, a.height.min(b.height) as usize);
    let (ya, yb) = (luma(a), luma(b));
    let mut se = 0f64;
    for r in 0..h {
        for c in 0..w {
            let d = f64::from(ya[r * a.width as usize + c]) - f64::from(yb[r * b.width as usize + c]);
            se += d * d;
        }
    }
    let mse = se / (w * h) as f64;
    if mse == 0.0 { 99.0 } else { 10.0 * (255.0f64 * 255.0 / mse).log10() }
}

fn one(enc: &mut dyn Encoder, f: &Frame, key: bool) -> EncodedFrame {
    let mut out = enc.encode(f, key).expect("encodage");
    assert_eq!(out.len(), 1, "une image encodée attendue");
    out.remove(0)
}

#[test]
fn aller_retour_psnr() {
    for p in pairs() {
        let (mut enc, mut dec) = p.make();
        for t in 0..20 {
            let src = pattern(320, 240, t);
            let e = one(enc.as_mut(), &src, false);
            assert_eq!(e.codec, Codec::H264);
            assert!(e.data.starts_with(&[0, 0, 0, 1]) || e.data.starts_with(&[0, 0, 1]), "{} : Annex B attendu", p.name);
            // Ensuite, la détection de changement de scène peut aussi en placer.
            if t == 0 {
                assert!(e.keyframe, "{}", p.name);
            }
            assert_eq!(e.ts90k, (u64::from(t) * 33_333 * 9 / 100) as u32, "{}", p.name);
            let d = dec.decode(&e).unwrap_or_else(|x| panic!("{} image {t} : {x:?}", p.name)).unwrap_or_else(|| panic!("{} image {t} : rien décodé", p.name));
            assert_eq!((d.width, d.height), (320, 240), "{}", p.name);
            assert!(d.is_consistent());
            let q = psnr(&src, &d);
            assert!(q > 32.0, "{} : PSNR {q:.1} dB à l'image {t}", p.name);
        }
    }
}

/// Le SPS de chaque encodeur H.264 de cette machine se lit, sort conforme
/// de la mise en forme d'envoi (les clients officiels ne le réécriront pas,
/// DAVE reste valide) et les images mises en forme se décodent toujours.
#[test]
fn sps_des_encodeurs_conforme_apres_mise_en_forme() {
    use crate::net::voice::video::h264;
    let list = [("ordinaire", encoder(Codec::H264, 640, 360, 30, 1_000_000)), ("repli", fallback_encoder(Codec::H264, 640, 360, 30, 1_000_000))];
    for (name, enc) in list {
        let Some(mut enc) = enc else { continue };
        let Some(mut dec) = decoder(Codec::H264) else { continue };
        let mut norm = h264::Normalizer::default();
        let (mut seen, mut shown) = (false, 0);
        for t in 0..12 {
            for f in enc.encode(&pattern(640, 360, t), t == 0 || t == 6).unwrap_or_else(|e| panic!("{name} : {e:?}")) {
                let before = h264::nal_units(&f.data).into_iter().find(|n| h264::nal_type(n) == h264::NAL_SPS).map(h264::sps_conforming);
                let data = norm.frame(&f.data);
                for n in h264::nal_units(&data).into_iter().filter(|n| h264::nal_type(n) == h264::NAL_SPS) {
                    seen = true;
                    assert_eq!(h264::sps_conforming(n), Some(true), "{name} : SPS {n:02x?} (avant : {before:?})");
                }
                if f.keyframe {
                    assert!(h264::nal_units(&data).iter().any(|n| h264::nal_type(n) == h264::NAL_SPS), "{name} : image clé sans SPS");
                    eprintln!("{name} : SPS conforme avant mise en forme : {before:?}");
                }
                if dec.decode(&EncodedFrame { data, ..f }).unwrap_or_else(|e| panic!("{name} : {e:?}")).is_some() {
                    shown += 1;
                }
            }
        }
        assert!(seen && shown >= 6, "{name} : {shown} images décodées");
    }
}

#[test]
fn image_cle_forcee() {
    for p in pairs() {
        let (mut enc, mut dec) = p.make();
        let mut frames = Vec::new();
        for t in 0..6 {
            frames.push(one(enc.as_mut(), &pattern(320, 240, t), t == 4));
        }
        let keys: Vec<bool> = frames.iter().map(|f| f.keyframe).collect();
        assert_eq!(keys, [true, false, false, false, true, false], "{}", p.name);
        // Un spectateur qui arrive à l'image clé forcée décode sans l'historique.
        let mut late = (p.dec)().expect("décodeur");
        assert!(late.decode(&frames[4]).expect("décodage").is_some(), "{}", p.name);
        assert!(late.decode(&frames[5]).expect("décodage").is_some(), "{}", p.name);
        for f in &frames {
            assert!(dec.decode(f).expect("décodage").is_some(), "{}", p.name);
        }
    }
}

#[test]
fn changement_de_resolution() {
    for p in pairs() {
        let (mut enc, mut dec) = p.make();
        for (t, (w, h)) in [(320, 240), (320, 240), (640, 360), (640, 360), (176, 144)].into_iter().enumerate() {
            let src = pattern(w, h, t as u32);
            let e = one(enc.as_mut(), &src, false);
            assert_eq!(e.keyframe, t == 0 || t == 2 || t == 4, "{} : image {t}", p.name);
            let d = dec.decode(&e).expect("décodage").expect("image");
            assert_eq!((d.width, d.height), (w, h), "{}", p.name);
            assert!(psnr(&src, &d) > 30.0, "{}", p.name);
        }
    }
}

#[test]
fn dimensions_impaires_et_bgra() {
    for p in pairs() {
        let (mut enc, mut dec) = p.make();
        // 321×241 : rogné à 320×240.
        let src = pattern(321, 241, 0);
        let d = dec.decode(&one(enc.as_mut(), &src, false)).expect("décodage").expect("image");
        assert_eq!((d.width, d.height), (320, 240), "{}", p.name);
        assert!(psnr(&src, &d) > 30.0, "{}", p.name);
        // BGRA gris uni : la luminance décodée retombe sur celle de la conversion.
        let bgra = Frame { width: 320, height: 240, pts_us: 33_333, data: FrameData::Bgra([90, 90, 90, 255].repeat(320 * 240)) };
        let d = dec.decode(&one(enc.as_mut(), &bgra, false)).expect("décodage").expect("image");
        let mean = luma(&d).iter().map(|&p| u32::from(p)).sum::<u32>() / (320 * 240);
        assert!((92..=96).contains(&mean), "{} : luminance moyenne {mean}", p.name);
    }
}

#[test]
fn flux_corrompu_sans_plantage() {
    for p in pairs() {
        let (mut enc, mut dec) = p.make();
        let key = one(enc.as_mut(), &pattern(320, 240, 0), false);
        let next = one(enc.as_mut(), &pattern(320, 240, 1), false);
        let bad = |data: Vec<u8>| EncodedFrame { codec: Codec::H264, keyframe: false, ts90k: 0, data };

        assert!(matches!(dec.decode(&bad(Vec::new())), Err(CodecError::Corrupt(_))));
        let wrong = EncodedFrame { codec: Codec::Vp8, ..key.clone() };
        assert_eq!(dec.decode(&wrong), Err(CodecError::WrongCodec(Codec::Vp8)));

        // Octets au hasard (générateur déterministe), clé tronquée, clé abîmée, P sans référence.
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
            if rnd() % 2 == 0 {
                data.splice(0..0, [0, 0, 0, 1, 0x65]);
            }
            let _ = dec.decode(&bad(data));
        }
        let _ = dec.decode(&bad(key.data[..key.data.len() / 2].to_vec()));
        let mut flipped = key.data.clone();
        for i in (40..flipped.len()).step_by(17) {
            flipped[i] ^= 0x5a;
        }
        let _ = dec.decode(&bad(flipped));
        let mut fresh = (p.dec)().expect("décodeur");
        assert!(!matches!(fresh.decode(&next), Ok(Some(_))), "{} : une image P seule ne doit rien montrer", p.name);

        // Le même décodeur repart sur une image clé saine.
        let d = dec.decode(&key).expect("décodage de la clé").expect("image");
        assert!(psnr(&pattern(320, 240, 0), &d) > 30.0, "{}", p.name);
    }
}

#[test]
fn debit_reglable() {
    // Bruit qui change à chaque image : le débit fixe la taille.
    let noisy = |t: u32| {
        let mut f = pattern(320, 240, t);
        if let FrameData::I420 { y, .. } = &mut f.data {
            let mut s = 0x9e37_79b9_u32.wrapping_mul(t + 1);
            for p in y.iter_mut() {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                *p = 16 + (s % 200) as u8;
            }
        }
        f
    };
    let total = |enc: &mut dyn Encoder, from: u32| -> usize {
        (from..from + 30).flat_map(|t| enc.encode(&noisy(t), false).expect("encodage")).map(|e| e.data.len()).sum()
    };
    // Une fois par encodeur (le décodeur ne joue pas ici).
    for p in pairs().into_iter().filter(|p| p.name.ends_with("OpenH264")) {
        let mut enc = (p.enc)(320, 240, 30, 2_000_000).expect("encodeur");
        let high = total(enc.as_mut(), 0);
        enc.set_bitrate(150_000);
        // Le nouveau débit s'applique au plus tard à l'image clé suivante.
        let first = enc.encode(&noisy(30), true).expect("encodage");
        assert!(first.iter().any(|e| e.keyframe), "{}", p.name);
        let low = total(enc.as_mut(), 31);
        assert!(low * 3 < high, "{} : débit non appliqué : {high} octets à 2 Mb/s, {low} à 150 kb/s", p.name);
    }
}

#[test]
fn entrees_invalides_et_codecs_absents() {
    assert_eq!(decoder(Codec::Vp8).is_some(), supported_decoders().contains(&Codec::Vp8));
    // VP9 et AV1 : décodés quand Windows a l'extension, jamais encodés.
    assert_eq!(decoder(Codec::Vp9).is_some(), supported_decoders().contains(&Codec::Vp9));
    assert_eq!(decoder(Codec::Av1).is_some(), supported_decoders().contains(&Codec::Av1));
    assert!(encoder(Codec::Vp9, 320, 240, 30, 1_000_000).is_none());
    assert!(encoder(Codec::Av1, 320, 240, 30, 1_000_000).is_none());
    assert!(!supported_encoders().contains(&Codec::Vp9) && !supported_encoders().contains(&Codec::Av1));
    assert_eq!(supported_decoders()[0], Codec::H264);
    assert_eq!(supported_encoders()[0], Codec::H264);
    assert!(encoder(Codec::H264, 0, 240, 30, 1_000_000).is_none());
    assert!(encoder(Codec::H264, 4096, 2160, 30, 1_000_000).is_none());

    for p in pairs() {
        let (mut enc, _) = p.make();
        let short = Frame { width: 320, height: 240, pts_us: 0, data: FrameData::Bgra(vec![0; 10]) };
        assert!(matches!(enc.encode(&short, false), Err(CodecError::BadFrame(_))));
        let tiny = pattern(1, 1, 0);
        assert!(matches!(enc.encode(&tiny, false), Err(CodecError::BadFrame(_))));
    }
}

/// Mesure indicative en 1080p30 (affichée avec `--nocapture`) : temps par
/// image de chaque encodeur et décodeur, et taille du flux à 4 Mb/s.
#[test]
fn bench_codecs_1080p() {
    let (w, h, n) = (1920, 1080, 30u32);
    let src: Vec<Frame> = (0..n).map(|t| pattern(w, h, t)).collect();
    let encs: [(&str, MakeEnc); 2] = [("OpenH264", software_encoder), ("matériel MF", hardware_encoder)];
    let decs: [(&str, MakeDec); 2] = [("OpenH264", software_decoder), ("Windows MF", system_decoder)];
    let mut stream = Vec::new();
    for (name, make) in encs {
        let Some(mut e) = make(w, h, 30, 4_000_000) else { continue };
        let t = Instant::now();
        let out: Vec<EncodedFrame> = src.iter().flat_map(|f| e.encode(f, false).expect("encodage")).collect();
        let ms = t.elapsed().as_secs_f64() * 1e3 / f64::from(n);
        eprintln!("1080p encodage {name} : {ms:.1} ms/image, {} Kio pour {n} images", out.iter().map(|f| f.data.len()).sum::<usize>() / 1024);
        stream = out;
    }
    for (name, make) in decs {
        let Some(mut d) = make() else { continue };
        let t = Instant::now();
        let shown = stream.iter().filter(|f| matches!(d.decode(f), Ok(Some(_)))).count();
        let ms = t.elapsed().as_secs_f64() * 1e3 / stream.len().max(1) as f64;
        eprintln!("1080p décodage {name} : {ms:.1} ms/image, {shown}/{} images rendues", stream.len());
    }
}
