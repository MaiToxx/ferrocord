//! Lecteur de fichiers de Media Foundation face à de vrais fichiers, fabriqués
//! ici (aucun fichier d'essai dans le dépôt) : un WAV écrit à la main, et un
//! MP4 H.264 écrit par l'enregistreur de Windows (`IMFSinkWriter`).

use super::super::{new_sample, pair};
use super::*;

/// WAV PCM 16 bits : `frames` trames d'une rampe, `channels` voies à `rate` Hz.
fn wav(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    let data = frames * u32::from(channels) * 2;
    let mut v = Vec::new();
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes());
    v.extend_from_slice(&channels.to_le_bytes());
    v.extend_from_slice(&rate.to_le_bytes());
    v.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    v.extend_from_slice(&(channels * 2).to_le_bytes());
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for i in 0..frames {
        for _ in 0..channels {
            v.extend_from_slice(&((i % 1000) as i16 * 16).to_le_bytes());
        }
    }
    v
}

#[test]
fn wav_is_read_with_its_duration_and_seeks() {
    let Ok(mut r) = FileReader::open(&wav(44_100, 2, 44_100), Some("audio/wav")) else {
        assert!(!FileReader::available(), "WAV refusé alors que Media Foundation est là");
        return;
    };
    let info = r.info();
    assert!(info.audio && info.video.is_none());
    assert!((990_000..=1_010_000).contains(&info.duration_us), "{info:?}");
    assert!(r.video().is_ok_and(|f| f.is_none()));
    let (mut frames, mut first) = (0usize, None);
    while let Some(c) = r.audio().unwrap_or_else(|e| panic!("lecture : {e}")) {
        assert_eq!((c.rate, c.channels), (44_100, 2));
        first.get_or_insert(c.samples[..4].to_vec());
        frames += c.samples.len() / 2;
    }
    assert_eq!(frames, 44_100);
    // Trames 0 et 1 de la rampe, sur les deux voies.
    let k = 16.0 / 32768.0;
    assert_eq!(first, Some(vec![0.0, 0.0, k, k]));
    // Saut au milieu : la suite reprend là, pas au début.
    r.seek(500_000).unwrap_or_else(|e| panic!("saut : {e}"));
    let c = r.audio().ok().flatten().unwrap_or_else(|| panic!("rien après le saut"));
    assert!((400_000..=500_000).contains(&c.pts_us), "{}", c.pts_us);
}

#[test]
fn garbage_is_refused_without_panic() {
    assert!(FileReader::open(b"ceci n'est pas un film", Some("video/mp4")).is_err());
    assert!(FileReader::open(&[], None).is_err());
}

/// MP4 H.264 de `n` images `w` × `h` à 30 i/s ; l'image `i` est d'un gris `16 + 4 i`.
fn mp4(path: &std::path::Path, w: u32, h: u32, n: u32) -> windows::core::Result<()> {
    let describe = |t: &IMFMediaType| -> windows::core::Result<()> {
        // SAFETY: type valide, valeurs simples.
        unsafe {
            t.SetUINT64(&MF_MT_FRAME_SIZE, pair(w, h))?;
            t.SetUINT64(&MF_MT_FRAME_RATE, pair(30, 1))?;
            t.SetUINT64(&MF_MT_PIXEL_ASPECT_RATIO, pair(1, 1))?;
            t.SetUINT32(&MF_MT_INTERLACE_MODE, MFVideoInterlace_Progressive.0 as u32)
        }
    };
    // SAFETY: objets neufs et possédés ; le chemin vit pendant l'appel.
    unsafe {
        let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path.as_os_str()), None::<&IMFByteStream>, None::<&IMFAttributes>)?;
        let out = video_type(&MFVideoFormat_H264)?;
        out.SetUINT32(&MF_MT_AVG_BITRATE, 600_000)?;
        describe(&out)?;
        let stream = writer.AddStream(&out)?;
        let input = video_type(&MFVideoFormat_NV12)?;
        describe(&input)?;
        writer.SetInputMediaType(stream, &input, None::<&IMFAttributes>)?;
        writer.BeginWriting()?;
        let luma = (w * h) as usize;
        for i in 0..n {
            let s = new_sample(luma * 3 / 2, i64::from(i) * 10_000_000 / 30, |b| {
                b[..luma].fill(16 + 4 * i as u8);
                b[luma..].fill(128);
            })?;
            s.SetSampleDuration(10_000_000 / 30)?;
            writer.WriteSample(stream, &s)?;
        }
        writer.Finalize()
    }
}

#[test]
fn mp4_frames_come_in_order_and_seek_lands_before_the_target() {
    if !FileReader::available() {
        return;
    }
    let path = std::env::temp_dir().join(format!("ferrocord-essai-lecteur-{}.mp4", std::process::id()));
    if let Err(e) = mp4(&path, 320, 240, 30) {
        // Pas d'encodeur H.264 dans ce Windows (édition N) : rien à vérifier.
        eprintln!("MP4 d'essai non écrit : {e}");
        let _ = std::fs::remove_file(&path);
        return;
    }
    let data = std::fs::read(&path).unwrap_or_default();
    let _ = std::fs::remove_file(&path);
    let mut r = FileReader::open(&data, Some("video/mp4")).unwrap_or_else(|e| panic!("ouverture : {e}"));
    let info = r.info();
    assert_eq!((info.video, info.audio, info.rotation), (Some((320, 240)), false, 0));
    assert!((900_000..=1_100_000).contains(&info.duration_us), "{info:?}");
    let mut seen = Vec::new();
    while let Some(f) = r.video().unwrap_or_else(|e| panic!("lecture : {e}")) {
        assert!((f.width, f.height) == (320, 240) && f.is_consistent());
        let crate::video::FrameData::I420 { y, .. } = &f.data else { panic!("pas en I420") };
        seen.push((f.pts_us, y[y.len() / 2]));
    }
    assert_eq!(seen.len(), 30);
    assert!(seen.windows(2).all(|p| p[0].0 < p[1].0), "{seen:?}");
    // Le gris monte d'image en image (compression : à quelques niveaux près).
    assert!(seen[0].1 < 30 && seen[29].1 > 120, "{seen:?}");
    r.seek(500_000).unwrap_or_else(|e| panic!("saut : {e}"));
    let f = r.video().ok().flatten().unwrap_or_else(|| panic!("rien après le saut"));
    assert!(f.pts_us <= 500_000, "{}", f.pts_us);
}

/// Plans décodés plus larges et plus hauts que l'image (pas de ligne et
/// lignes de bourrage), côtés impairs : chaque plan est relevé à sa place,
/// sans cisaillement, en I420 comme en NV12.
#[test]
fn plans_avec_bourrage_et_cotes_impairs() {
    let (w, h, st, rows) = (5usize, 3usize, 9usize, 5usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let (cst, crows) = (st.div_ceil(2), rows.div_ceil(2));
    // Valeur d'un échantillon : plan (centaines), ligne (dizaines), colonne ; bourrage à 255.
    let fill = |b: &mut [u8], off: usize, stride: usize, pw: usize, ph: usize, step: usize, base: u8| {
        for r in 0..ph {
            for c in 0..pw {
                b[off + r * stride + c * step] = base + (r * 10 + c) as u8;
            }
        }
    };
    let want_y: Vec<u8> = (0..h).flat_map(|r| (0..w).map(move |c| (r * 10 + c) as u8)).collect();
    let want_c = |base: u8| -> Vec<u8> { (0..ch).flat_map(|r| (0..cw).map(move |c| base + (r * 10 + c) as u8)).collect() };
    let mut i420 = vec![255u8; st * rows + 2 * cst * crows];
    fill(&mut i420, 0, st, w, h, 1, 0);
    fill(&mut i420, st * rows, cst, cw, ch, 1, 100);
    fill(&mut i420, st * rows + cst * crows, cst, cw, ch, 1, 200);
    let mut nv12 = vec![255u8; st * rows + st * crows + 1];
    fill(&mut nv12, 0, st, w, h, 1, 0);
    fill(&mut nv12, st * rows, st, cw, ch, 2, 100);
    fill(&mut nv12, st * rows + 1, st, cw, ch, 2, 200);
    for (bytes, is_nv12) in [(&i420, false), (&nv12, true)] {
        // Dans des plans déjà servis, plus grands que l'image : rien n'en reste.
        let f = i420_frame(bytes, Shape { width: w, height: h, stride: st, rows, nv12: is_nv12 }, 0, [vec![9; 400], vec![9; 400], vec![9; 400]]).unwrap_or_else(|| panic!("image (nv12 : {is_nv12})"));
        assert_eq!((f.width, f.height), (5, 3));
        let crate::video::FrameData::I420 { y, u, v } = f.data else { panic!("format") };
        assert_eq!((y, u, v), (want_y.clone(), want_c(100), want_c(200)), "nv12 : {is_nv12}");
    }
    // Tampon plus court qu'annoncé : refusé, jamais lu au-delà.
    assert!(i420_frame(&i420[..st * rows + cst * crows + cst], Shape { width: w, height: h, stride: st, rows, nv12: false }, 0, Default::default()).is_none());
}
