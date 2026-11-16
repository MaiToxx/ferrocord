//! Tests des sons d'interface : purs, sans sortie audio.

use super::perso::{self, Banque, Refus, Releve};
use super::synth::{self, FREQ};
use super::*;
use std::path::PathBuf;

/// −6 dBFS en 16 bits.
const MOINS_6_DB: i32 = 16_422;

fn secondes(pcm: &[i16]) -> f32 {
    (pcm.len() / 2) as f32 / FREQ as f32
}

/// Moyenne des deux voies, de `de` à `a` secondes.
fn mono(pcm: &[i16], de: f32, a: f32) -> Vec<f32> {
    let (i, j) = (synth::trames(de), synth::trames(a).min(pcm.len() / 2));
    (i..j).map(|f| (f32::from(pcm[2 * f]) + f32::from(pcm[2 * f + 1])) / 65536.0).collect()
}

/// Fréquence fondamentale d'un extrait (autocorrélation normalisée ; le plus
/// petit retard presque aussi bon que le meilleur, pour ne pas tomber une octave trop bas).
fn hauteur(x: &[f32]) -> f32 {
    let lags = (FREQ / 1200) as usize..(FREQ / 150) as usize;
    let score = |lag: usize| {
        let n = x.len() - lag;
        let (mut ab, mut aa, mut bb) = (0.0f64, 0.0f64, 0.0f64);
        for i in 0..n {
            let (a, b) = (f64::from(x[i]), f64::from(x[i + lag]));
            ab += a * b;
            aa += a * a;
            bb += b * b;
        }
        ab / (aa * bb).sqrt().max(1e-12)
    };
    let scores: Vec<(usize, f64)> = lags.map(|l| (l, score(l))).collect();
    let best = scores.iter().fold(0.0f64, |m, s| m.max(s.1));
    // Puis le sommet de cette bosse.
    let mut i = scores.iter().position(|s| s.1 >= 0.97 * best).unwrap_or(0);
    while i + 1 < scores.len() && scores[i + 1].1 > scores[i].1 {
        i += 1;
    }
    let lag = scores[i].0;
    FREQ as f32 / lag as f32
}

/// Durée attendue de chaque son, en secondes.
fn plage(kind: Kind) -> (f32, f32) {
    match kind {
        Kind::VoiceMute | Kind::VoiceUnmute => (0.12, 0.26),
        Kind::VoiceDeafen | Kind::VoiceUndeafen => (0.2, 0.4),
        Kind::Reaction => (0.1, 0.3),
        Kind::Ring => (4.0, 4.0),
        Kind::CallOut => (3.0, 3.0),
        _ => (0.3, 1.5),
    }
}

#[test]
fn chaque_son_est_propre() {
    for kind in Kind::ALL {
        let r = recette(kind);
        let pcm = integre(kind);
        let nom = perso::noms(kind)[0];
        assert_eq!(pcm.len() % 2, 0, "{nom} : stéréo");
        // Ni clic au début, ni à la fin.
        assert_eq!(pcm[..2], [0, 0], "{nom} : commence à zéro");
        assert_eq!(pcm[pcm.len() - 2..], [0, 0], "{nom} : finit à zéro");
        // Les premières et dernières millisecondes restent tout près de zéro.
        let ms = synth::trames(0.001) * 2;
        assert!(pcm[..ms / 2].iter().all(|s| s.abs() < 1500), "{nom} : attaque progressive");
        assert!(pcm[pcm.len() - ms..].iter().all(|s| s.abs() < 300), "{nom} : fin éteinte");
        let crete = pcm.iter().map(|s| i32::from(*s).abs()).max().unwrap();
        assert!(crete <= MOINS_6_DB, "{nom} : crête {crete}");
        assert!(crete > 2_000, "{nom} : audible ({crete})");
        let (min, max) = plage(kind);
        let d = secondes(&pcm);
        assert!((min - 1e-3..=max + 1e-3).contains(&d), "{nom} : durée {d} s");
        for c in 0..2 {
            let moyenne = pcm.iter().skip(c).step_by(2).map(|s| f64::from(*s)).sum::<f64>() / (pcm.len() / 2) as f64;
            assert!(moyenne.abs() < 8.0, "{nom} : composante continue {moyenne}");
        }
        // Aucun saut d'un échantillon au suivant (clic au milieu du son).
        let saut = pcm.as_chunks::<2>().0.windows(2).map(|w| (i32::from(w[1][0]) - i32::from(w[0][0])).abs()).max().unwrap();
        assert!(saut < 3_000, "{nom} : saut de {saut}");
        // Chaque note finit avant la fin du cycle : rien n'est coupé par le fondu.
        for note in r.notes {
            assert!(note.debut + note.tenue + 0.045 <= r.duree - r.fondu / 2.0, "{nom} : note à {} s trop longue", note.debut);
            assert!((0.003..=0.010).contains(&r.timbre.attaque), "{nom} : attaque");
        }
    }
}

#[test]
fn les_sons_ont_le_meme_niveau_percu() {
    for kind in Kind::ALL {
        let r = recette(kind);
        let l = synth::sonie(&rendre(&r));
        let voulu = CIBLE + r.niveau;
        // Jamais au-dessus du niveau voulu ; au plus 5 dB dessous (sons très
        // brefs, retenus par la crête à −6 dBFS).
        assert!(l <= voulu + 0.1 && l >= voulu - 5.0, "{} : {l:.1} dB pour {voulu:.1} dB", perso::noms(kind)[0]);
    }
}

#[test]
fn le_micro_descend_puis_remonte_et_le_casque_est_plus_grave() {
    let notes = |kind: Kind, coupe: f32| {
        let pcm = integre(kind);
        (hauteur(&mono(&pcm, 0.010, coupe - 0.008)), hauteur(&mono(&pcm, coupe + 0.010, coupe + 0.075)))
    };
    let (a, b) = notes(Kind::VoiceMute, 0.07);
    assert!(a > b * 1.2, "micro coupé : {a} Hz puis {b} Hz");
    let (c, d) = notes(Kind::VoiceUnmute, 0.07);
    assert!(d > c * 1.2, "micro rétabli : {c} Hz puis {d} Hz");
    // Les deux mêmes notes, dans l'autre sens.
    assert!((a - d).abs() < a * 0.03 && (b - c).abs() < b * 0.03, "{a} {b} {c} {d}");
    let (e, f) = notes(Kind::VoiceDeafen, 0.095);
    assert!(e > f * 1.2, "casque coupé : {e} Hz puis {f} Hz");
    let (g, h) = notes(Kind::VoiceUndeafen, 0.095);
    assert!(h > g * 1.2, "casque rétabli : {g} Hz puis {h} Hz");
    assert!(e < b && f < b, "le casque est plus grave que le micro");
}

#[test]
fn arrivee_monte_et_depart_descend() {
    let glisse = |kind: Kind| {
        let pcm = integre(kind);
        (hauteur(&mono(&pcm, 0.002, 0.022)), hauteur(&mono(&pcm, 0.12, 0.20)))
    };
    let (a, b) = glisse(Kind::VoiceJoin);
    assert!(b > a * 1.1, "arrivée : {a} Hz puis {b} Hz");
    let (c, d) = glisse(Kind::VoiceLeave);
    assert!(d < c / 1.1, "départ : {c} Hz puis {d} Hz");
    // Ma connexion est plus longue et aussi forte, au moins, que l'arrivée d'un autre.
    assert!(recette(Kind::VoiceConnect).duree > recette(Kind::VoiceJoin).duree);
    assert!(recette(Kind::VoiceConnect).niveau >= recette(Kind::VoiceJoin).niveau);
}

#[test]
fn la_sonnerie_boucle_sans_discontinuite() {
    for kind in [Kind::Ring, Kind::CallOut] {
        let pcm = integre(kind);
        // Du dernier échantillon au premier : aucun saut.
        assert_eq!(pcm[pcm.len() - 2..], pcm[..2]);
        // Un vrai silence avant de recommencer.
        let fin = mono(&pcm, secondes(&pcm) - 0.05, secondes(&pcm));
        assert!(fin.iter().all(|x| x.abs() < 0.01), "silence de fin");
    }
    // Appel entrant : deux cycles identiques ; la relance toutes les quatre
    // secondes tombe donc en mesure.
    let ring = integre(Kind::Ring);
    let (a, b) = ring.split_at(ring.len() / 2);
    assert_eq!(a, b);
    let r = recette(Kind::Ring);
    assert!((2.0..=3.0).contains(&r.duree) && r.cycles == 2);
    let melodie = r.notes.iter().filter(|n| n.gain == 1.0).count();
    assert!((4..=6).contains(&melodie), "{melodie} notes");
    // Appel sortant : plus discret.
    assert!(recette(Kind::CallOut).niveau < r.niveau);
}

#[test]
fn rendu_identique_d_une_fois_a_l_autre() {
    assert_eq!(integre(Kind::Mention), integre(Kind::Mention));
}

#[test]
fn noms_de_fichier_simples_et_distincts() {
    let mut vus = std::collections::HashSet::new();
    for kind in Kind::ALL {
        let [fr, en] = perso::noms(kind);
        for nom in [fr, en] {
            assert!(nom.chars().all(|c| c.is_ascii_lowercase() || c == '-'), "{nom}");
        }
        assert!(vus.insert(fr), "{fr} en double");
        assert!(fr == en || vus.insert(en), "{en} en double");
    }
    let notice = perso::notice();
    assert!(notice.contains("micro-coupe.wav  /  mute.wav") && notice.contains("  message.wav\r\n"), "{notice}");
}

/// Dossier temporaire du test, vidé à la fin.
struct Dossier(PathBuf);

impl Dossier {
    fn new(nom: &str) -> Dossier {
        let d = std::env::temp_dir().join(format!("ferrocord-sons-{}-{nom}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dossier(d)
    }
}

impl Drop for Dossier {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// WAV PCM 16 bits d'un sinus de 440 Hz.
fn wav_test(rate: u32, voies: u16, secondes: f32) -> Vec<u8> {
    let frames = (rate as f32 * secondes) as usize;
    let data = frames * usize::from(voies) * 2;
    let mut out = Vec::new();
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data as u32).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&voies.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(voies) * 2).to_le_bytes());
    out.extend_from_slice(&(voies * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    // Un bloc inconnu avant les données : il doit être sauté.
    out.extend_from_slice(b"LIST\x03\x00\x00\x00abc\x00");
    out.extend_from_slice(b"data");
    out.extend_from_slice(&(data as u32).to_le_bytes());
    for i in 0..frames {
        let s = ((i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 12_000.0) as i16;
        for _ in 0..voies {
            out.extend_from_slice(&s.to_le_bytes());
        }
    }
    out
}

#[test]
fn wav_personnel_ramene_a_48_khz_stereo() {
    let pcm = perso::decoder(wav_test(44_100, 1, 0.5), "wav").unwrap();
    assert!((secondes(&pcm) - 0.5).abs() < 0.01, "{}", secondes(&pcm));
    assert!((hauteur(&mono(&pcm, 0.1, 0.2)) - 440.0).abs() < 8.0);
    assert!(pcm.as_chunks::<2>().0.iter().all(|f| f[0] == f[1]), "mono recopié sur les deux voies");
    let pcm = perso::decoder(wav_test(48_000, 2, 0.25), "WAV").unwrap();
    assert_eq!(pcm.len(), 2 * 12_000);
    // 8 bits, 24 bits et flottant : même sinus à mi-échelle.
    let autre = |tag: u16, bits: u16, echantillon: &dyn Fn(f32) -> Vec<u8>| {
        let mut w = wav_test(48_000, 1, 0.0);
        w[20..22].copy_from_slice(&tag.to_le_bytes());
        w[34..36].copy_from_slice(&bits.to_le_bytes());
        let n = w.len();
        let mut data = Vec::new();
        for i in 0..4800 {
            data.extend(echantillon((i as f32 * 0.05).sin() * 0.5));
        }
        w[n - 4..].copy_from_slice(&(data.len() as u32).to_le_bytes());
        w.extend(data);
        let pcm = perso::decoder(w, "wav").unwrap();
        let crete = pcm.iter().map(|s| s.abs()).max().unwrap();
        assert!((15_800..=16_500).contains(&crete), "{bits} bits : crête {crete}");
    };
    autre(1, 8, &|x| vec![(x * 127.0 + 128.0) as u8]);
    autre(1, 24, &|x| ((x * 8_388_607.0) as i32).to_le_bytes()[..3].to_vec());
    autre(1, 32, &|x| ((x * 2_147_483_647.0) as i32).to_le_bytes().to_vec());
    autre(3, 32, &|x| x.to_le_bytes().to_vec());
    autre(3, 64, &|x| f64::from(x).to_le_bytes().to_vec());
}

#[test]
fn fichier_trop_long_coupe_et_fondu() {
    let pcm = perso::decoder(wav_test(8_000, 1, 12.0), "wav").unwrap();
    assert_eq!(pcm.len(), 2 * (FREQ * perso::DUREE_MAX) as usize);
    assert_eq!(pcm[pcm.len() - 2..], [0, 0], "fondu jusqu'à zéro");
}

#[test]
fn fichiers_invalides_refuses_sans_planter() {
    assert_eq!(perso::decoder(Vec::new(), "wav"), Err(Refus::Format));
    assert_eq!(perso::decoder(b"RIFF\x04\x00\x00\x00WAVE".to_vec(), "wav"), Err(Refus::Format));
    assert_eq!(perso::decoder(b"pas un son du tout".to_vec(), "wav"), Err(Refus::Format));
    assert!(perso::decoder(b"pas un son du tout".to_vec(), "mp3").is_err());
    assert!(perso::decoder(b"OggS pas vraiment".to_vec(), "ogg").is_err());
    // Codage inconnu (ADPCM), zéro voie, fréquence absurde.
    for (at, valeur) in [(20, 2u16), (22, 0), (22, 40)] {
        let mut w = wav_test(48_000, 1, 0.1);
        w[at..at + 2].copy_from_slice(&valeur.to_le_bytes());
        assert_eq!(perso::decoder(w, "wav"), Err(Refus::NonPrisEnCharge));
    }
    let mut w = wav_test(48_000, 1, 0.1);
    w[24..28].copy_from_slice(&1u32.to_le_bytes());
    assert_eq!(perso::decoder(w, "wav"), Err(Refus::NonPrisEnCharge));
    // Données annoncées mais absentes ; fichier tronqué n'importe où.
    let w = wav_test(48_000, 2, 0.1);
    for coupe in [13, 30, 44, 57, 60, 61] {
        let _ = perso::decoder(w[..coupe].to_vec(), "wav");
    }
    assert_eq!(perso::decoder(w[..56].to_vec(), "wav"), Err(Refus::Format));
    // Tronqué au milieu des données : ce qui est là est joué.
    assert!(perso::decoder(w[..w.len() - 1001].to_vec(), "wav").is_ok());
    assert_eq!(perso::decoder(vec![0; perso::TAILLE_MAX as usize + 1], "wav"), Err(Refus::TropGros));
}

#[test]
fn la_banque_prefere_le_fichier_personnel_valide() {
    let d = Dossier::new("banque");
    let mut b = Banque::new(Some(d.0.clone()));
    assert!(b.is_empty(), "rien n'est fabriqué avant la première demande");
    let integre = b.son(Kind::VoiceMute);
    assert!(!integre.perso);
    assert_eq!(&integre.wav[..4], b"RIFF");
    assert_eq!(integre.wav.len(), 44 + super::integre(Kind::VoiceMute).len() * 2);
    assert!(std::sync::Arc::ptr_eq(&integre.wav, &b.son(Kind::VoiceMute).wav), "gardé en mémoire");
    assert_eq!(b.len(), 1);

    // Fichier valide, sous son nom anglais : pris au son suivant, sans relancer.
    std::fs::write(d.0.join("mute.wav"), wav_test(22_050, 1, 0.2)).unwrap();
    let perso = b.son(Kind::VoiceMute);
    assert!(perso.perso);
    assert_ne!(perso.wav, integre.wav);
    assert!(std::sync::Arc::ptr_eq(&perso.wav, &b.son(Kind::VoiceMute).wav), "relu seulement s'il change");
    assert!(!b.son(Kind::VoiceUnmute).perso, "les autres sons ne changent pas");

    // Le nom français passe avant ; invalide : son intégré.
    std::fs::write(d.0.join("micro-coupe.wav"), b"n'importe quoi").unwrap();
    let invalide = b.son(Kind::VoiceMute);
    assert!(!invalide.perso);
    assert_eq!(invalide.wav, integre.wav);
    std::fs::remove_file(d.0.join("micro-coupe.wav")).unwrap();
    assert!(b.son(Kind::VoiceMute).perso, "fichier invalide retiré : l'autre est repris");

    // Trop gros : refusé avant d'être lu.
    let gros = d.0.join("ring.wav");
    std::fs::File::create(&gros).unwrap().set_len(perso::TAILLE_MAX + 1).unwrap();
    assert_eq!(perso::lire(&gros), Err(Refus::TropGros));
    assert!(!b.son(Kind::Ring).perso);

    // Retiré : retour au son intégré.
    std::fs::remove_file(d.0.join("mute.wav")).unwrap();
    assert_eq!(b.son(Kind::VoiceMute).wav, integre.wav);

    // Page des réglages : sons remplacés et fichiers refusés.
    std::fs::write(d.0.join("Mention.WAV"), wav_test(48_000, 2, 0.1)).unwrap();
    let mut r = Releve::default();
    assert_eq!(r.actualiser(&d.0), (Kind::Mention.bit(), Kind::Ring.bit()));
    assert_eq!(r.actualiser(&d.0), (Kind::Mention.bit(), Kind::Ring.bit()));
    std::fs::remove_file(&gros).unwrap();
    assert_eq!(r.actualiser(&d.0), (Kind::Mention.bit(), 0));
    assert_eq!(Releve::default().actualiser(&d.0.join("absent")), (0, 0));

    // Sans dossier : sons intégrés.
    assert!(!Banque::new(None).son(Kind::Mention).perso);
}

#[test]
fn la_notice_est_ecrite_une_fois() {
    let d = Dossier::new("notice");
    let sons = d.0.join("sons");
    perso::preparer(&sons).unwrap();
    let path = sons.join(perso::NOTICE);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), perso::notice());
    let avant = std::fs::metadata(&path).unwrap().modified().unwrap();
    perso::preparer(&sons).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), avant);
    // La notice n'est le son d'aucun événement.
    assert_eq!(Releve::default().actualiser(&sons), (0, 0));
}
