use super::record::Recorder;
use super::*;

/// MP3 fabriqué à la main : `n` trames MPEG-1 couche III, 128 kbit/s,
/// 44,1 kHz, mono, sans CRC. Informations annexes et données à zéro :
/// chaque trame est valide et donne 1 152 échantillons de silence.
fn silent_mp3(n: usize) -> Vec<u8> {
    let mut out = Vec::new();
    for _ in 0..n {
        let mut frame = vec![0u8; 417];
        frame[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0xC0]);
        out.extend_from_slice(&frame);
    }
    out
}

fn sine(n: usize) -> Vec<f32> {
    (0..n).map(|i| 0.5 * (i as f32 * 2.0 * std::f32::consts::PI * 440.0 / 48_000.0).sin()).collect()
}

#[test]
fn mp3_is_decoded_and_resampled_to_48k() {
    let pcm = decode(silent_mp3(40)).unwrap();
    assert_eq!(pcm.channels, 1);
    // 40 × 1 152 à 44,1 kHz ≈ 1 045 ms ; le décodeur peut retenir une trame.
    let ms = pcm.duration_ms();
    assert!((990..=1060).contains(&ms), "{ms} ms");
    assert!(pcm.data.iter().all(|s| *s == 0));
    assert_eq!(decode(b"rien de lisible".to_vec()), Err(ClipError::Format));
}

#[test]
fn recording_round_trip_through_ogg_opus() {
    let mut r = Recorder::new().unwrap();
    // 1,23 s par morceaux irréguliers, comme la capture.
    let src = sine(59_040);
    for chunk in src.chunks(441) {
        r.push(chunk);
    }
    assert_eq!(r.elapsed_ms(), 1220);
    let rec = r.finish(7).unwrap();
    assert_eq!(rec.duration_ms, 1240, "dernière trame complétée");
    assert!(rec.ogg.starts_with(b"OggS"));
    // Une crête par 100 ms : 13 points, tous forts (sinus d'amplitude 0,5).
    assert_eq!(rec.waveform.len(), 13);
    assert!(rec.waveform[..12].iter().all(|v| (120..=130).contains(v)), "{:?}", rec.waveform);
    let pcm = decode(rec.ogg.clone()).unwrap();
    assert_eq!(pcm.channels, 1);
    assert_eq!(pcm.frames(), 62 * 960 - 312, "pré-saut retiré");
    // Le son revient : énergie proche de celle du sinus (0,5² / 2).
    let mid = &pcm.data[9600..48_000];
    let energy = mid.iter().map(|s| (f32::from(*s) / 32768.0).powi(2)).sum::<f32>() / mid.len() as f32;
    assert!((0.09..0.16).contains(&energy), "{energy}");
    let w = waveform(&pcm);
    assert_eq!(w.len(), 13);
    assert!(w[2..10].iter().all(|v| *v > 100), "{w:?}");
    // Rien d'enregistré : rien à envoyer.
    assert!(Recorder::new().unwrap().finish(1).is_none());
}

#[test]
fn waveform_is_reduced_to_256_points() {
    let peaks: Vec<f32> = (0..1000).map(|i| if i % 4 == 3 { 1.0 } else { 0.1 }).collect();
    let w = waveform_from_peaks(&peaks);
    assert_eq!(w.len(), 250);
    assert!(w.iter().all(|v| *v == 255), "crête de chaque groupe");
    assert_eq!(waveform_from_peaks(&[0.0, 0.5, 2.0]), vec![0, 128, 255]);
    assert!(waveform_from_peaks(&[]).is_empty());
    let long = vec![0.2f32; 5000];
    assert!(waveform_from_peaks(&long).len() <= WAVEFORM_POINTS);
}

#[test]
fn cache_is_bounded_and_keeps_the_recent_ones() {
    let pcm = |n: usize| Pcm { channels: 1, data: vec![0i16; n].into() };
    let mut c = Cache::new(100);
    c.insert("a".into(), pcm(20));
    c.insert("b".into(), pcm(20));
    assert!(c.get("a").is_some());
    // 40 + 40 octets : « b », le moins récent, part.
    c.insert("c".into(), pcm(20));
    assert!(c.get("b").is_none());
    assert!(c.get("a").is_some() && c.get("c").is_some());
    // Trop gros pour la borne : pas gardé, rien d'autre ne part.
    c.insert("d".into(), pcm(60));
    assert_eq!(c.len(), 2);
    assert!(c.get("d").is_none());
    // Remplacement d'une clé : l'ancienne taille est rendue.
    c.insert("a".into(), pcm(10));
    c.insert("e".into(), pcm(20));
    assert_eq!(c.len(), 3);
}
