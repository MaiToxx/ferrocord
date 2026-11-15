//! Source WebM face à un vrai fichier, fabriqué ici : images VP8 encodées
//! par Windows (essai sauté sans les « Extensions vidéo VP9 »), son Opus
//! encodé par libopus, le tout écrit par `mkv::write`.

use super::super::*;
use super::WebmSource;

use crate::video::mkv::write::{webm, Packet};
use crate::video::{Codec, FrameData};

const W: u32 = 320;
const H: u32 = 240;

fn opus_head() -> Vec<u8> {
    let mut h = b"OpusHead".to_vec();
    h.extend_from_slice(&[1, 1, 0x38, 0x01, 0x80, 0xbb, 0, 0, 0, 0, 0]);
    h
}

/// Paquets Opus de 20 ms : un la à 440 Hz pendant `frames` trames.
fn opus_packets(frames: u64) -> Vec<Packet> {
    let mut enc = opus::Encoder::new(48_000, opus::Channels::Mono, opus::Application::Audio).unwrap_or_else(|e| panic!("encodeur Opus : {e}"));
    (0..frames)
        .map(|i| {
            let mono: Vec<f32> = (0..960).map(|k| ((i * 960 + k) as f32 * 440.0 * std::f32::consts::TAU / 48_000.0).sin() * 0.4).collect();
            (2, i * 20, true, enc.encode_vec_float(&mono, 4000).unwrap_or_else(|e| panic!("trame {i} : {e}")))
        })
        .collect()
}

/// Image grise unie dont la luminance suit `t`.
fn gray(t: u64) -> Frame {
    let (w, h) = (W as usize, H as usize);
    Frame { width: W, height: H, pts_us: t * 40_000, data: FrameData::I420 { y: vec![40 + (t * 8) as u8; w * h], u: vec![128; w * h / 4], v: vec![128; w * h / 4] } }
}

/// Images VP8 à 25 i/s, une image clé toutes les 10 ; `None` sans le codec de Windows.
fn vp8_packets(frames: u64) -> Option<Vec<Packet>> {
    let mut enc = crate::video::codec::encoder(Codec::Vp8, W, H, 25, 500_000)?;
    Some(
        (0..frames)
            .map(|t| {
                let mut out = enc.encode(&gray(t), t % 10 == 0).unwrap_or_else(|e| panic!("image {t} : {e}"));
                let f = out.remove(0);
                (1, t * 40, f.keyframe, f.data)
            })
            .collect(),
    )
}

fn merged(mut a: Vec<Packet>, b: Vec<Packet>) -> Vec<Packet> {
    a.extend(b);
    a.sort_by_key(|p| p.1);
    a
}

fn luma(f: &Frame) -> u8 {
    match &f.data {
        FrameData::I420 { y, .. } => y[y.len() / 2],
        FrameData::Bgra(_) => 0,
    }
}

#[test]
fn opus_seul_duree_et_decalage_initial() {
    let data = webm(None, Some(&opus_head()), &opus_packets(50), 1000.0, false);
    let mut s = WebmSource::open(data).unwrap_or_else(|e| panic!("ouverture : {e:?}"));
    assert_eq!(s.info(), Info { duration_us: 1_000_000, video: None, rotation: 0, audio: true });
    assert!(s.video().is_ok_and(|f| f.is_none()));
    let first = s.audio().ok().flatten().unwrap_or_else(|| panic!("son"));
    // Les 312 premiers échantillons (6,5 ms) ne sont pas à entendre.
    assert_eq!((first.pts_us, first.rate, first.channels, first.samples.len()), (0, 48_000, 2, (960 - 312) * 2));
    let second = s.audio().ok().flatten().unwrap_or_else(|| panic!("son"));
    assert_eq!((second.pts_us, second.samples.len()), (20_000 - 6500, 1920));
    let mut energy = second.samples.iter().map(|x| x * x).sum::<f32>();
    let mut chunks = 2;
    while let Some(c) = s.audio().unwrap_or_else(|e| panic!("{e}")) {
        energy += c.samples.iter().map(|x| x * x).sum::<f32>();
        chunks += 1;
    }
    assert_eq!(chunks, 50);
    assert!(energy > 100.0, "le la s'entend : {energy}");
    // Saut : reprise un peu avant la position (le lecteur jette l'avance).
    s.seek(500_000).unwrap_or_else(|e| panic!("{e}"));
    let c = s.audio().ok().flatten().unwrap_or_else(|| panic!("son après le saut"));
    assert!((400_000..=500_000).contains(&c.pts_us), "{}", c.pts_us);
}

#[test]
fn vp8_et_opus_images_dans_l_ordre_et_saut_a_l_image_cle() {
    let Some(video) = vp8_packets(25) else {
        eprintln!("VP8 de Windows absent : sauté");
        return;
    };
    for unknown in [false, true] {
        let data = webm(Some(("V_VP8", W, H)), Some(&opus_head()), &merged(video.clone(), opus_packets(50)), 1000.0, unknown);
        assert!(data.starts_with(&crate::video::mkv::MAGIC));
        // Par la même porte que le lecteur (signature du fichier, pas le type annoncé).
        let mut s = open(data, Some("video/mp4")).unwrap_or_else(|e| panic!("ouverture : {e:?}"));
        let info = s.info();
        assert_eq!((info.video, info.audio), (Some((W, H)), true));
        assert_eq!(info.duration_us, if unknown { 980_000 } else { 1_000_000 });
        let mut n = 0;
        while let Some(f) = s.video().unwrap_or_else(|e| panic!("image {n} : {e}")) {
            assert_eq!((f.width, f.height, f.pts_us), (W, H, n * 40_000));
            assert!(f.is_consistent());
            assert!(luma(&f).abs_diff(40 + (n * 8) as u8) < 12, "image {n} : {}", luma(&f));
            n += 1;
        }
        assert_eq!(n, 25);
        assert!(s.audio().is_ok_and(|c| c.is_some()));
        // Saut à 0,6 s : la vidéo repart de l'image clé qui précède (0,4 s).
        s.seek(600_000).unwrap_or_else(|e| panic!("{e}"));
        let f = s.video().ok().flatten().unwrap_or_else(|| panic!("image après le saut"));
        assert_eq!(f.pts_us, 400_000);
        assert!(luma(&f).abs_diff(40 + 80) < 12);
    }
}

#[test]
fn lecture_complete_par_le_lecteur() {
    let Some(video) = vp8_packets(10) else { return };
    let data = webm(Some(("V_VP8", W, H)), None, &video, 400.0, false);
    let h = Handle::loading(Arc::new(|| {}));
    h.start(move || open(data, None).map_err(|e| format!("{e:?}")));
    let t0 = Instant::now();
    let mut seen = 0;
    while h.snapshot().phase != Phase::Ended {
        if let Some((g, p)) = h.take_picture(seen) {
            seen = g;
            assert_eq!((p.width, p.height), (W, H));
        }
        assert!(!matches!(h.snapshot().phase, Phase::Failed(_)), "{:?}", h.snapshot());
        assert!(t0.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(seen >= 2, "{seen}");
    h.close();
}

#[test]
fn fichiers_refuses_sans_panique() {
    // Signature seule, pistes inconnues, données au hasard.
    assert_eq!(open(crate::video::mkv::MAGIC.to_vec(), None).err(), Some(OpenError::Unsupported));
    let other = webm(Some(("V_AV1", W, H)), None, &[(1, 0, true, vec![0; 8])], 40.0, false);
    assert_eq!(open(other, None).err(), Some(OpenError::Unsupported));
    // VP9 annoncé avec des données qui n'en sont pas : aucune image, aucune panique.
    let fake = webm(Some(("V_VP9", W, H)), None, &[(1, 0, true, vec![0x55; 64])], 40.0, false);
    match open(fake, None) {
        Ok(mut s) => assert!(s.video().is_ok_and(|f| f.is_none())),
        Err(e) => assert_eq!(e, OpenError::NoDecoder),
    }
    #[cfg(windows)]
    eprintln!("VP9 décodable ici : {}", std::thread::spawn(crate::video::codec::vp9_decodable).join().unwrap_or(false));
}
