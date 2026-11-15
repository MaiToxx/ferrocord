//! Tests du moteur entier (le codec, le tampon et le limiteur ont les leurs).

use super::*;
use std::sync::{Arc, Mutex};

pub(crate) fn sine(freq: f32, amp: f32, start: usize, n: usize) -> Vec<f32> {
    (start..start + n)
        .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SAMPLE_RATE as f32).sin())
        .collect()
}

fn corr_at(a: &[f32], b: &[f32], lag: usize) -> f32 {
    let n = a.len().min(b.len().saturating_sub(lag));
    let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
    for i in 0..n {
        let (x, y) = (a[i] as f64, b[i + lag] as f64);
        ab += x * y;
        aa += x * x;
        bb += y * y;
    }
    if aa > 0.0 && bb > 0.0 { (ab / (aa * bb).sqrt()) as f32 } else { 0.0 }
}

/// Meilleure corrélation normalisée de `b` décalé de 0..max_lag par rapport à
/// `a` (recherche grossière par pas de 8, puis fine autour du meilleur).
pub(crate) fn best_corr(a: &[f32], b: &[f32], max_lag: usize) -> f32 {
    let lags = (0..max_lag).filter(|l| b.len() >= l + a.len());
    let coarse = lags.step_by(8).map(|l| (l, corr_at(a, b, l))).fold((0, f32::MIN), |m, x| if x.1 > m.1 { x } else { m });
    (coarse.0.saturating_sub(8)..coarse.0 + 8)
        .filter(|l| b.len() >= l + a.len())
        .map(|l| corr_at(a, b, l))
        .fold(coarse.1, f32::max)
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
}

/// Paquets Opus d'un sinus, une trame chacun.
fn packets(freq: f32, amp: f32, frames: usize) -> Vec<Vec<u8>> {
    let mut enc = VoiceEncoder::new().unwrap();
    (0..frames).map(|i| enc.encode(&sine(freq, amp, i * FRAME, FRAME)).unwrap()).collect()
}

/// Reçoit un paquet par trame jouée (comme en vrai) et rend toute la sortie stéréo.
fn play(e: &mut Engine, streams: &[(u64, &[Vec<u8>])], frames: usize) -> Vec<f32> {
    let mut out = vec![0.0; frames * FRAME * 2];
    for (i, chunk) in out.chunks_mut(FRAME * 2).enumerate() {
        for (user, pk) in streams {
            if let Some(p) = pk.get(i) {
                e.receive(*user, i as u16, (i * FRAME) as u32, p);
            }
        }
        e.pull_playback(chunk);
    }
    out
}

fn left(stereo: &[f32]) -> Vec<f32> {
    stereo.chunks(2).map(|s| s[0]).collect()
}

#[test]
fn aller_retour_par_le_moteur() {
    let mut tx = Engine::new();
    tx.set_gate(Gate::Voice { auto: false, threshold_db: -60.0 });
    let src = sine(440.0, 0.5, 0, FRAME * 80);
    // Morceaux de taille quelconque, comme un périphérique.
    let frames: Vec<Vec<u8>> = src.chunks(441).flat_map(|c| tx.push_capture(c)).collect();
    assert_eq!(frames.len(), 80);
    assert!(tx.speaking_now());

    let mut rx = Engine::new();
    let out = left(&play(&mut rx, &[(7, &frames)], 80));
    let c = best_corr(&src[FRAME * 10..FRAME * 30], &out[FRAME * 10..], FRAME * 14);
    assert!(c > 0.95, "corrélation {c}");
}

#[test]
fn volume_muet_et_sourdine() {
    let pk = packets(300.0, 0.3, 60);
    let base = play(&mut Engine::new(), &[(1, &pk)], 60);
    assert!(rms(&base[FRAME * 2 * 20..]) > 0.1);

    for v in [0.0f32, 0.5, 2.0] {
        let mut e = Engine::new();
        e.set_user_volume(1, v);
        let out = play(&mut e, &[(1, &pk)], 60);
        let worst = out.iter().zip(&base).map(|(o, b)| (o - b * v).abs()).fold(0.0, f32::max);
        assert!(worst < 1e-4, "volume {v} : écart {worst}");
    }
    // Hors bornes : ramené à 0..=2.
    let mut e = Engine::new();
    e.set_user_volume(1, 9.0);
    let out = play(&mut e, &[(1, &pk)], 60);
    assert!((rms(&out) / rms(&base) - 2.0).abs() < 1e-3);

    let mut e = Engine::new();
    e.set_user_muted(1, true);
    assert!(play(&mut e, &[(1, &pk)], 60).iter().all(|x| *x == 0.0));

    let mut e = Engine::new();
    e.set_deafened(true);
    assert!(play(&mut e, &[(1, &pk)], 60).iter().all(|x| *x == 0.0));
}

/// Volume à 200 % et égaliseur poussé à fond : le limiteur, placé après
/// eux, garde la sortie sous la saturation.
#[test]
fn amplification_et_egaliseur_ne_saturent_pas() {
    let pk = packets(300.0, 0.8, 60);
    let mut e = Engine::new();
    e.set_user_volume(1, 2.0);
    e.play.set_eq([12, 12, 12]);
    let out = play(&mut e, &[(1, &pk)], 60);
    assert!(rms(&out[FRAME * 2 * 20..]) > 0.2);
    assert!(out.iter().all(|x| x.abs() < 1.0), "crête {}", out.iter().fold(0.0f32, |m, x| m.max(x.abs())));
    // À plat, l'égaliseur ne touche à rien.
    let base = play(&mut Engine::new(), &[(1, &pk)], 60);
    let mut flat = Engine::new();
    flat.play.set_eq([0; 3]);
    assert_eq!(play(&mut flat, &[(1, &pk)], 60), base);
}

#[test]
fn mixage_somme_les_voix_et_limite() {
    let a = packets(300.0, 0.2, 40);
    let b = packets(500.0, 0.2, 40);
    let oa = play(&mut Engine::new(), &[(1, &a)], 40);
    let ob = play(&mut Engine::new(), &[(2, &b)], 40);
    let both = play(&mut Engine::new(), &[(1, &a), (2, &b)], 40);
    let worst = both.iter().zip(oa.iter().zip(&ob)).map(|(m, (x, y))| (m - x - y).abs()).fold(0.0, f32::max);
    assert!(worst < 1e-4, "écart {worst}");

    // Quatre voix fortes à 200 % : jamais d'écrêtage.
    let loud = packets(300.0, 0.9, 40);
    let mut e = Engine::new();
    for u in 1..=4 {
        e.set_user_volume(u, 2.0);
    }
    let streams: Vec<(u64, &[Vec<u8>])> = (1..=4).map(|u| (u, loud.as_slice())).collect();
    let out = play(&mut e, &streams, 40);
    assert!(out.iter().all(|x| x.abs() < 1.0));
    assert!(rms(&out) > 0.3);
}

#[test]
fn pertes_et_desordre_sans_trou() {
    let pk = packets(440.0, 0.4, 60);
    let mut e = Engine::new();
    let mut out = vec![0.0; 70 * FRAME * 2];
    // Paquets échangés deux à deux, un sur dix perdu.
    let order: Vec<usize> = (0..60).map(|i| i ^ 1).collect();
    for (i, chunk) in out.chunks_mut(FRAME * 2).enumerate() {
        if let Some(&k) = order.get(i)
            && k % 10 != 5
        {
            e.receive(3, 1000u16.wrapping_add(k as u16), 0, &pk[k]);
        }
        e.pull_playback(chunk);
    }
    let frames: Vec<f32> = out.chunks(FRAME * 2).map(rms).collect();
    let first = frames.iter().position(|r| *r > 0.05).unwrap();
    assert!(first <= 6, "délai {first} trames");
    // Du premier au dernier paquet : chaque trame a du son (FEC ou PLC aux trous).
    assert!(frames[first..first + 55].iter().all(|r| *r > 0.05), "{frames:?}");
}

#[test]
fn porte_vocale_avec_maintien() {
    let mut e = Engine::new();
    e.set_gate(Gate::Voice { auto: false, threshold_db: -30.0 });
    let loud = sine(200.0, 0.5, 0, FRAME * 5);
    assert_eq!(e.push_capture(&loud).len(), 5);
    assert!(e.speaking_now());
    assert!((e.input_level_db() + 9.03).abs() < 0.1, "{}", e.input_level_db());

    let quiet = sine(200.0, 0.001, 0, FRAME * 30);
    let out = e.push_capture(&quiet);
    // 15 trames de maintien (300 ms ; la DTX peut en taire la fin),
    // puis 5 trames de silence.
    let held = out.iter().filter(|f| f.as_slice() != SILENCE).count();
    assert!((8..=15).contains(&held), "{held}");
    assert_eq!(out.len(), held + 5);
    assert!(out[held..].iter().all(|f| f.as_slice() == SILENCE));
    assert!(!e.speaking_now());
    assert!(e.input_level_db() < -50.0);
    assert!(e.push_capture(&quiet).is_empty());
    // Un bout de trame ne produit rien tant que la trame n'est pas complète.
    assert!(e.push_capture(&loud[..500]).is_empty());
    assert_eq!(e.push_capture(&loud[500..960]).len(), 1);
}

#[test]
fn appui_pour_parler() {
    let mut e = Engine::new();
    let loud = sine(200.0, 0.5, 0, FRAME * 3);
    let quiet = vec![0.0; FRAME * 3];
    e.set_gate(Gate::PushToTalk { pressed: false });
    assert!(e.push_capture(&loud).is_empty());
    assert!(!e.speaking_now());
    e.set_gate(Gate::PushToTalk { pressed: true });
    assert_eq!(e.push_capture(&quiet).len(), 3);
    assert!(e.speaking_now());
    e.set_gate(Gate::PushToTalk { pressed: false });
    let out = e.push_capture(&loud);
    assert_eq!(out.len(), 5);
    assert!(out.iter().all(|f| f.as_slice() == SILENCE));
}

#[test]
fn muet_ne_produit_rien() {
    let mut e = Engine::new();
    let loud = sine(200.0, 0.5, 0, FRAME * 10);
    e.push_capture(&loud[..FRAME]);
    e.set_muted(true);
    assert!(e.push_capture(&loud).is_empty());
    assert!(!e.speaking_now());
    // Le niveau reste mesuré (vumètre des paramètres).
    assert!(e.input_level_db() > -20.0);
    e.set_muted(false);
    assert_eq!(e.push_capture(&loud).len(), 10);
}

struct Probe {
    far_energy: Arc<Mutex<f32>>,
}

impl crate::audio::Processor for Probe {
    fn process(&mut self, mono: &mut [f32], far_end: &[f32]) {
        assert_eq!(mono.len(), FRAME);
        if let Ok(mut g) = self.far_energy.lock() {
            *g = rms(far_end);
        }
        mono.fill(0.0);
    }
}

#[test]
fn traitement_avant_la_porte_avec_reference_d_echo() {
    let far = Arc::new(Mutex::new(-1.0));
    let mut e = Engine::new();
    e.set_gate(Gate::Voice { auto: false, threshold_db: -60.0 });
    e.set_processor(Some(Box::new(Probe { far_energy: far.clone() })));
    let pk = packets(300.0, 0.3, 30);
    play(&mut e, &[(1, &pk)], 20);
    // Le traitement rend du silence : la porte reste fermée malgré le signal fort.
    assert!(e.push_capture(&sine(200.0, 0.5, 0, FRAME)).is_empty());
    assert_eq!(e.input_level_db(), -100.0);
    assert!(*far.lock().unwrap() > 0.1);
    e.set_processor(None);
    assert_eq!(e.push_capture(&sine(200.0, 0.5, 0, FRAME)).len(), 1);
}

#[test]
fn qui_parle_et_purge() {
    let pk = packets(300.0, 0.3, 5);
    let mut e = Engine::new();
    let t0 = Instant::now();
    e.receive_at(1, 0, 0, &pk[0], t0);
    e.receive_at(2, 0, 0, &SILENCE, t0);
    assert_eq!(e.speaking_users_at(t0 + Duration::from_millis(100)), vec![1]);
    assert!(e.speaking_users_at(t0 + Duration::from_millis(400)).is_empty());

    e.set_user_volume(1, 1.5);
    let mut buf = vec![0.0; FRAME * 2];
    for _ in 0..PURGE_FRAMES + 20 {
        e.pull_playback(&mut buf);
    }
    assert!(e.play.peers.is_empty());
    // Les réglages de la personne survivent à la purge.
    assert_eq!(e.play.prefs.get(&1).map(|p| p.volume), Some(1.5));
    e.set_user_volume(1, 1.0);
    assert!(e.play.prefs.is_empty());
}

#[test]
fn lecture_par_morceaux_quelconques() {
    let pk = packets(300.0, 0.3, 30);
    let whole = play(&mut Engine::new(), &[(1, &pk)], 30);
    let mut e = Engine::new();
    let mut out = vec![0.0; 30 * FRAME * 2];
    let mut pos = 0;
    let mut next_pkt = 0;
    while pos < out.len() {
        let n = 882.min(out.len() - pos);
        // On fournit le paquet d'une trame avant que la lecture n'en ait besoin.
        while next_pkt < pk.len() && next_pkt * FRAME * 2 < pos + n {
            e.receive(1, next_pkt as u16, 0, &pk[next_pkt]);
            next_pkt += 1;
        }
        e.pull_playback(&mut out[pos..pos + n]);
        pos += n;
    }
    assert_eq!(out, whole);
}

#[test]
fn moteur_envoyable_entre_fils() {
    fn send<T: Send>() {}
    send::<Engine>();
}

/// Petit banc : capture + 10 personnes reçues et mixées, par trame de 20 ms.
/// `cargo test --release bench_temps_par_trame -- --nocapture` pour le chiffre réel.
#[test]
fn bench_temps_par_trame() {
    const USERS: u64 = 10;
    const FRAMES: usize = 200;
    let pk = packets(300.0, 0.3, FRAMES);
    let mic = sine(200.0, 0.5, 0, FRAME * FRAMES);
    let mut e = Engine::new();
    let mut out = vec![0.0; FRAME * 2];
    let t = Instant::now();
    for i in 0..FRAMES {
        let sent = e.push_capture(&mic[i * FRAME..(i + 1) * FRAME]);
        assert_eq!(sent.len(), 1);
        for u in 0..USERS {
            e.receive(u, i as u16, 0, &pk[i]);
        }
        e.pull_playback(&mut out);
    }
    let per = t.elapsed() / FRAMES as u32;
    println!("moteur : {per:?} par trame de 20 ms (capture + {USERS} personnes)");
    assert!(per < Duration::from_millis(10), "{per:?}");
}
