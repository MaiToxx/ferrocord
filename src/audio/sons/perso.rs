//! Sons personnels : un fichier déposé dans `<réglages>/ferrocord/sons/`
//! (sous Windows : `%APPDATA%\ferrocord\sons\`) remplace le son intégré de
//! l'événement dont il porte le nom.
//!
//! - Noms : ceux de [`noms`], en français ou en anglais (`micro-coupe.wav`
//!   ou `mute.wav`), sans majuscules ni accents.
//! - Formats : WAV (PCM 8, 16, 24 ou 32 bits, ou flottant), MP3, Ogg/Opus
//!   (pas Ogg/Vorbis : le projet n'en a pas le décodeur). À nom égal, le WAV
//!   l'emporte, puis l'Ogg, puis le MP3 ; le nom français passe avant l'anglais.
//! - Limites : [`TAILLE_MAX`] octets, [`DUREE_MAX`] secondes (le reste est
//!   coupé, avec un fondu) ; tout est ramené à 48 kHz stéréo.
//! - Fichier illisible ou trop gros : le son intégré est joué, une ligne le
//!   dit dans le journal, rien ne plante.
//!
//! La lecture se fait dans le fil qui joue le son (ou dans celui de la page
//! des réglages), jamais dans le fil d'interface.

use super::synth;
use crate::state::notify::Kind;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

/// Taille d'un fichier personnel, au plus (8 Mio).
pub const TAILLE_MAX: u64 = 8 * 1024 * 1024;
/// Durée jouée d'un fichier personnel, au plus (secondes).
pub const DUREE_MAX: u32 = 10;
/// Extensions acceptées, par ordre de préférence.
pub const EXTENSIONS: [&str; 3] = ["wav", "ogg", "mp3"];
/// Sons gardés en mémoire, au plus (octets de WAV) : au-delà, les autres sont refaits à la demande.
const MEMOIRE_MAX: usize = 12 * 1024 * 1024;
/// Fichiers regardés dans le dossier, au plus (page des réglages).
const FICHIERS_MAX: usize = 512;
/// Nom de la notice écrite dans le dossier.
pub const NOTICE: &str = "LISEZMOI.txt";

/// Noms de fichier (sans extension) de l'événement : français, anglais.
pub fn noms(kind: Kind) -> [&'static str; 2] {
    match kind {
        Kind::Message => ["message", "message"],
        Kind::Mention => ["mention", "mention"],
        Kind::VoiceMute => ["micro-coupe", "mute"],
        Kind::VoiceUnmute => ["micro-retabli", "unmute"],
        Kind::VoiceDeafen => ["casque-coupe", "deafen"],
        Kind::VoiceUndeafen => ["casque-retabli", "undeafen"],
        Kind::VoiceJoin => ["arrivee", "user-join"],
        Kind::VoiceLeave => ["depart", "user-leave"],
        Kind::VoiceConnect => ["connexion", "connect"],
        Kind::VoiceDisconnect => ["deconnexion", "disconnect"],
        Kind::Ring => ["appel-entrant", "ring"],
        Kind::CallOut => ["appel-sortant", "call-out"],
        Kind::StreamStart => ["direct-commence", "stream-start"],
        Kind::StreamStop => ["direct-arrete", "stream-stop"],
        Kind::FriendRequest => ["demande-ami", "friend-request"],
        Kind::EventStart => ["evenement", "event"],
        Kind::Reaction => ["reaction", "reaction"],
        Kind::Reminder => ["rappel", "reminder"],
    }
}

/// Dossier des sons personnels (à côté de `notify.toml`).
pub fn dossier() -> Option<PathBuf> {
    Some(crate::platform::config_dir()?.join("ferrocord").join("sons"))
}

/// Noms de fichier essayés pour `kind`, du préféré au dernier.
fn candidats(kind: Kind) -> impl Iterator<Item = String> {
    EXTENSIONS.into_iter().flat_map(move |ext| noms(kind).into_iter().map(move |nom| format!("{nom}.{ext}")))
}

/// Ce qui distingue un fichier d'une version précédente du même fichier.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Empreinte {
    pub chemin: PathBuf,
    pub taille: u64,
    pub modifie: Option<SystemTime>,
}

fn empreinte(chemin: PathBuf) -> Option<Empreinte> {
    let m = std::fs::metadata(&chemin).ok().filter(|m| m.is_file())?;
    Some(Empreinte { chemin, taille: m.len(), modifie: m.modified().ok() })
}

/// Fichier personnel de `kind` dans `dossier`, s'il y en a un.
pub fn chercher(dossier: &Path, kind: Kind) -> Option<Empreinte> {
    candidats(kind).find_map(|nom| empreinte(dossier.join(nom)))
}

/// Pourquoi un fichier personnel n'est pas pris.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refus {
    /// Plus de [`TAILLE_MAX`] octets.
    TropGros,
    /// Illisible, vide, ou pas le format que dit son extension.
    Format,
    /// Format reconnu mais pas pris en charge (codage, fréquence).
    NonPrisEnCharge,
    /// Le disque a refusé la lecture.
    Lecture,
}

impl Refus {
    fn mots(self) -> &'static str {
        match self {
            Refus::TropGros => "fichier trop gros",
            Refus::Format => "format illisible",
            Refus::NonPrisEnCharge => "format non pris en charge",
            Refus::Lecture => "lecture impossible",
        }
    }
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Contenu d'un WAV : échantillons `f32` entrelacés (une ou deux voies),
/// fréquence, voies. Au plus [`DUREE_MAX`] secondes sont lues ; `true` si le
/// fichier en contenait davantage.
fn lire_wav(b: &[u8]) -> Result<(Vec<f32>, u32, usize, bool), Refus> {
    if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"WAVE" {
        return Err(Refus::Format);
    }
    // (codage, voies, fréquence, bits)
    let mut fmt: Option<(u16, usize, u32, usize)> = None;
    let mut at = 12;
    while let (Some(id), Some(size)) = (b.get(at..at + 4), u32_at(b, at + 4)) {
        let start = at + 8;
        // Bloc annoncé plus long que le fichier (écriture interrompue) : ce qui est là est pris.
        let end = start.saturating_add(size as usize).min(b.len());
        let body = &b[start..end];
        if id == b"fmt " {
            let (Some(mut tag), Some(ch), Some(rate), Some(bits)) = (u16_at(body, 0), u16_at(body, 2), u32_at(body, 4), u16_at(body, 14)) else { return Err(Refus::Format) };
            // WAVE_FORMAT_EXTENSIBLE : le vrai codage ouvre le sous-format.
            if tag == 0xFFFE {
                tag = u16_at(body, 24).ok_or(Refus::Format)?;
            }
            fmt = Some((tag, usize::from(ch), rate, usize::from(bits)));
        } else if id == b"data" {
            let (tag, ch, rate, bits) = fmt.ok_or(Refus::Format)?;
            let pcm = tag == 1 && matches!(bits, 8 | 16 | 24 | 32);
            let float = tag == 3 && matches!(bits, 32 | 64);
            if !(pcm || float) || !(1..=8).contains(&ch) || !(8_000..=192_000).contains(&rate) {
                return Err(Refus::NonPrisEnCharge);
            }
            let (octets, frame) = (bits / 8, ch * bits / 8);
            let all = body.len() / frame;
            let frames = all.min((rate * DUREE_MAX) as usize);
            let voies = ch.min(2);
            let mut out = Vec::with_capacity(frames * voies);
            for f in body.chunks_exact(frame).take(frames) {
                for s in f.chunks_exact(octets).take(voies) {
                    out.push(match (float, octets) {
                        (false, 1) => (f32::from(s[0]) - 128.0) / 128.0,
                        (false, 2) => f32::from(i16::from_le_bytes([s[0], s[1]])) / 32768.0,
                        (false, 3) => (i32::from_le_bytes([0, s[0], s[1], s[2]]) >> 8) as f32 / 8_388_608.0,
                        (false, _) => i32::from_le_bytes([s[0], s[1], s[2], s[3]]) as f32 / 2_147_483_648.0,
                        (true, 4) => f32::from_le_bytes([s[0], s[1], s[2], s[3]]),
                        (true, _) => f64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]) as f32,
                    });
                }
            }
            if out.is_empty() || out.iter().any(|x| !x.is_finite()) {
                return Err(Refus::Format);
            }
            return Ok((out, rate, voies, all > frames));
        }
        // Les blocs sont alignés sur deux octets.
        at = start.saturating_add(size as usize).saturating_add(size as usize & 1);
    }
    Err(Refus::Format)
}

/// Décode un fichier personnel d'après son extension : 48 kHz, stéréo
/// entrelacée, 16 bits, [`DUREE_MAX`] secondes au plus.
pub fn decoder(octets: Vec<u8>, extension: &str) -> Result<Vec<i16>, Refus> {
    if octets.len() as u64 > TAILLE_MAX {
        return Err(Refus::TropGros);
    }
    let (mono_ou_stereo, voies, mut coupe) = if extension.eq_ignore_ascii_case("wav") {
        let (samples, rate, voies, coupe) = lire_wav(&octets)?;
        (crate::audio::clip::to_48k(&samples, rate, voies), voies, coupe)
    } else {
        let pcm = crate::audio::clip::decode(octets).map_err(|e| match e {
            crate::audio::clip::ClipError::Format => Refus::Format,
            crate::audio::clip::ClipError::Unsupported => Refus::NonPrisEnCharge,
        })?;
        (pcm.data.to_vec(), pcm.channels.clamp(1, 2), false)
    };
    let max = (synth::FREQ * DUREE_MAX) as usize;
    let frames = mono_ou_stereo.len() / voies;
    coupe |= frames > max;
    let frames = frames.min(max);
    let mut out: Vec<i16> = Vec::with_capacity(frames * 2);
    for f in mono_ou_stereo.chunks_exact(voies).take(frames) {
        out.extend_from_slice(&[f[0], f[voies - 1]]);
    }
    if out.is_empty() {
        return Err(Refus::Format);
    }
    if coupe {
        // Coupé avant sa fin : fondu de 30 ms, pour ne pas finir sur un clic.
        let n = synth::trames(0.03).min(frames);
        for i in 0..n {
            let f = frames - 1 - i;
            for s in &mut out[2 * f..2 * f + 2] {
                *s = (i32::from(*s) * i as i32 / n as i32) as i16;
            }
        }
    }
    Ok(out)
}

/// Lit et décode le fichier `chemin` (taille vérifiée avant la lecture).
pub fn lire(chemin: &Path) -> Result<Vec<i16>, Refus> {
    use std::io::Read;
    let ext = chemin.extension().and_then(|e| e.to_str()).unwrap_or_default().to_string();
    let f = std::fs::File::open(chemin).map_err(|_| Refus::Lecture)?;
    if f.metadata().map_err(|_| Refus::Lecture)?.len() > TAILLE_MAX {
        return Err(Refus::TropGros);
    }
    // Borné même si le fichier grossit pendant la lecture.
    let mut octets = Vec::new();
    f.take(TAILLE_MAX + 1).read_to_end(&mut octets).map_err(|_| Refus::Lecture)?;
    decoder(octets, &ext)
}

/// Un son prêt à jouer.
#[derive(Clone, Debug)]
pub struct Son {
    /// Fichier WAV entier (48 kHz, stéréo, 16 bits).
    pub wav: Arc<[u8]>,
    /// Il vient du dossier de l'utilisateur.
    pub perso: bool,
}

struct Entree {
    /// Fichier personnel vu à la fabrication (même refusé), ou aucun.
    vu: Option<Empreinte>,
    son: Son,
}

/// Sons fabriqués à la demande et gardés en mémoire. Rien n'est fabriqué
/// tant qu'aucun son n'est demandé ; un fichier personnel ajouté, changé ou
/// retiré est pris en compte au son suivant.
pub struct Banque {
    dossier: Option<PathBuf>,
    sons: HashMap<Kind, Entree>,
}

impl Banque {
    /// `dossier` : celui des sons personnels (`None` : sons intégrés seulement).
    pub fn new(dossier: Option<PathBuf>) -> Banque {
        Banque { dossier, sons: HashMap::new() }
    }

    /// Le son de `kind` : fichier personnel s'il existe et se lit, sinon son intégré.
    pub fn son(&mut self, kind: Kind) -> Son {
        let vu = self.dossier.as_deref().and_then(|d| chercher(d, kind));
        if let Some(e) = self.sons.get(&kind)
            && e.vu == vu
        {
            return e.son.clone();
        }
        let perso = vu.as_ref().and_then(|e| match lire(&e.chemin) {
            Ok(pcm) => Some(pcm),
            Err(refus) => {
                let nom = e.chemin.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                eprintln!("[sons] {nom} ignoré ({}) : son intégré joué à la place", refus.mots());
                None
            }
        });
        let son = Son { perso: perso.is_some(), wav: synth::wav(&perso.unwrap_or_else(|| super::integre(kind))).into() };
        if self.octets() + son.wav.len() > MEMOIRE_MAX {
            self.sons.clear();
        }
        self.sons.insert(kind, Entree { vu, son: son.clone() });
        son
    }

    /// Octets de WAV gardés.
    pub fn octets(&self) -> usize {
        self.sons.values().map(|e| e.son.wav.len()).sum()
    }

    /// Sons gardés.
    pub fn len(&self) -> usize {
        self.sons.len()
    }

    pub fn is_empty(&self) -> bool {
        self.sons.is_empty()
    }
}

/// État des fichiers personnels, pour la page des réglages : quels sons sont
/// remplacés, quels fichiers sont refusés. Un fichier n'est relu que s'il a changé.
#[derive(Default)]
pub struct Releve {
    vus: HashMap<Kind, (Empreinte, bool)>,
}

impl Releve {
    /// Relit le dossier. Renvoie (sons remplacés, fichiers refusés), en bits de [`Kind::bit`].
    pub fn actualiser(&mut self, dossier: &Path) -> (u32, u32) {
        // Une seule lecture du dossier, bornée.
        // (nom en minuscules, nom tel qu'il est écrit sur le disque)
        let presents: HashMap<String, std::ffi::OsString> = std::fs::read_dir(dossier)
            .map(|d| d.flatten().take(FICHIERS_MAX).filter_map(|e| e.file_name().to_str().map(str::to_ascii_lowercase).map(|n| (n, e.file_name()))).collect())
            .unwrap_or_default();
        let (mut bons, mut refuses) = (0, 0);
        for kind in Kind::ALL {
            let Some(e) = candidats(kind).find_map(|nom| presents.get(&nom)).and_then(|nom| empreinte(dossier.join(nom))) else {
                self.vus.remove(&kind);
                continue;
            };
            let valide = match self.vus.get(&kind) {
                Some((old, v)) if *old == e => *v,
                _ => lire(&e.chemin).is_ok(),
            };
            self.vus.insert(kind, (e, valide));
            if valide {
                bons |= kind.bit();
            } else {
                refuses |= kind.bit();
            }
        }
        (bons, refuses)
    }
}

/// Texte de la notice du dossier : noms, formats, limites.
pub fn notice() -> String {
    let mut s = String::from(
        "Sons personnels de Ferrocord\r\n\
         ============================\r\n\r\n\
         Un fichier déposé ici remplace le son intégré de l'événement dont il\r\n\
         porte le nom (nom français ou anglais, au choix). Retirez-le pour\r\n\
         retrouver le son d'origine. Pas besoin de relancer Ferrocord.\r\n\r\n\
         Formats : .wav (PCM ou flottant), .mp3, .ogg (Opus seulement).\r\n",
    );
    s.push_str(&format!("Limites : {} Mo par fichier ; {DUREE_MAX} secondes jouées au plus.\r\n", TAILLE_MAX / (1024 * 1024)));
    s.push_str("Les sonneries sont relancées en boucle : visez 4 secondes (appel entrant) et 3 secondes (appel sortant).\r\n\r\nNoms (français / anglais) :\r\n");
    for kind in Kind::ALL {
        let [fr, en] = noms(kind);
        s.push_str(&if fr == en { format!("  {fr}.wav\r\n") } else { format!("  {fr}.wav  /  {en}.wav\r\n") });
    }
    s
}

/// Crée le dossier et sa notice s'ils manquent (avant de l'ouvrir dans l'Explorateur).
pub fn preparer(dossier: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dossier)?;
    let path = dossier.join(NOTICE);
    // Réécrite si la liste des sons a changé depuis (nouvelle version).
    let texte = notice();
    if std::fs::read_to_string(&path).ok().as_deref() != Some(texte.as_str()) {
        std::fs::write(path, texte)?;
    }
    Ok(())
}
