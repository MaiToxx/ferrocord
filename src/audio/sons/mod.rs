//! Sons d'interface de Ferrocord : sonneries, micro et casque, arrivées et
//! départs du vocal, notifications. Ce sont des sons D'ORIGINE, fabriqués par
//! le petit synthétiseur de [`synth`] : rien n'est embarqué, téléchargé, ni
//! repris d'un autre logiciel.
//!
//! Une seule famille : la gamme pentatonique de ré majeur (ré mi fa♯ la si),
//! le même timbre de base plus ou moins feutré, la même petite réverbération,
//! le même niveau perçu. Chaque son est décrit par une [`Recette`] (quelques
//! notes) et fabriqué à la demande ([`rendre`]) ; [`perso::Banque`] le garde
//! ensuite en mémoire, ou lui préfère le fichier que l'utilisateur a déposé
//! dans son dossier de sons.
//!
//! Format commun : 48 kHz, stéréo, 16 bits ; début et fin à zéro (aucun
//! clic), crête à −6 dBFS au plus.

pub mod perso;
pub mod synth;

use crate::state::notify::Kind;
use synth::{Note, Timbre, CLAIR, DOUX, FEUTRE};

// Gamme commune (hertz) : pentatonique de ré majeur, du si 2 au la 4 (la 3 = 440 Hz).
const SI2: f32 = 246.94;
const RE3: f32 = 293.66;
const MI3: f32 = 329.63;
const LA3: f32 = 440.0;
const SI3: f32 = 493.88;
const RE4: f32 = 587.33;
const MI4: f32 = 659.26;
const FA4: f32 = 739.99;
const LA4: f32 = 880.0;

/// Timbre du casque : celui du micro en plus sourd, avec assez d'harmoniques
/// pour rester audible sur de petits haut-parleurs malgré le registre grave.
const SOURD: Timbre = Timbre { indice: 0.55, indice_fond: 0.2, indice_tau: 0.06, octave: 0.12, frappe: 0.0, attaque: 0.008 };

/// Puissance perçue visée par tous les sons (dB, voir [`synth::sonie`]).
pub const CIBLE: f32 = -21.0;

/// Comment fabriquer un son.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Recette {
    pub notes: &'static [Note],
    pub timbre: Timbre,
    /// Durée d'un cycle, en secondes (queue de réverbération comprise).
    pub duree: f32,
    /// Cycles mis bout à bout (2 pour la sonnerie : elle boucle sur elle-même).
    pub cycles: usize,
    /// Coupure du passe-bas, en hertz.
    pub passe_bas: f32,
    /// Réverbération : dose (0 à 1) et temps d'extinction (secondes).
    pub reverb: (f32, f32),
    /// Fondu de fin, en secondes.
    pub fondu: f32,
    /// Écart voulu par rapport à [`CIBLE`], en décibels (sons discrets : négatif).
    pub niveau: f32,
}

impl Recette {
    /// Durée totale du son, en secondes.
    pub fn duree_totale(&self) -> f32 {
        self.duree * self.cycles as f32
    }
}

const fn n(debut: f32, hz: f32, tau: f32, tenue: f32) -> Note {
    Note::new(debut, hz, tau, tenue)
}

const COURT: Recette = Recette { notes: const { &[] }, timbre: DOUX, duree: 0.5, cycles: 1, passe_bas: 4500.0, reverb: (0.15, 0.35), fondu: 0.08, niveau: 0.0 };

/// Recette du son intégré de `kind`.
pub fn recette(kind: Kind) -> Recette {
    match kind {
        // Micro : deux notes brèves et feutrées, une quarte ; elles descendent
        // quand il se coupe, remontent quand il revient.
        Kind::VoiceMute => Recette { notes: const { &[n(0.0, RE4, 0.045, 0.065), n(0.07, LA3, 0.05, 0.08)] }, timbre: FEUTRE, duree: 0.24, passe_bas: 1800.0, reverb: (0.10, 0.12), fondu: 0.04, niveau: -2.0, ..COURT },
        Kind::VoiceUnmute => Recette { notes: const { &[n(0.0, LA3, 0.045, 0.065), n(0.07, RE4, 0.05, 0.08)] }, timbre: FEUTRE, duree: 0.24, passe_bas: 1800.0, reverb: (0.10, 0.12), fondu: 0.04, niveau: -2.0, ..COURT },
        // Casque : la même quarte, près d'une octave plus bas, plus lente et plus étouffée.
        Kind::VoiceDeafen => Recette { notes: const { &[n(0.0, MI3, 0.06, 0.09), n(0.095, SI2, 0.08, 0.13)] }, timbre: SOURD, duree: 0.34, passe_bas: 1100.0, reverb: (0.10, 0.15), fondu: 0.05, niveau: -2.0, ..COURT },
        Kind::VoiceUndeafen => Recette { notes: const { &[n(0.0, SI2, 0.06, 0.09), n(0.095, MI3, 0.08, 0.13)] }, timbre: SOURD, duree: 0.34, passe_bas: 1100.0, reverb: (0.10, 0.15), fondu: 0.05, niveau: -2.0, ..COURT },
        // Quelqu'un arrive : une seule note qui glisse vers le haut ; il part : vers le bas.
        Kind::VoiceJoin => Recette { notes: const { &[n(0.0, MI4, 0.13, 0.30).glisse(-5.0)] }, duree: 0.42, niveau: -2.0, ..COURT },
        Kind::VoiceLeave => Recette { notes: const { &[n(0.0, SI3, 0.13, 0.30).glisse(5.0)] }, duree: 0.42, niveau: -2.0, ..COURT },
        // Moi : une quinte franche, plus claire et plus longue, sur une basse discrète.
        Kind::VoiceConnect => Recette { notes: const { &[n(0.0, RE4, 0.12, 0.16), n(0.10, LA4, 0.22, 0.42), n(0.0, LA3, 0.2, 0.3).gain(0.35)] }, timbre: CLAIR, duree: 0.66, passe_bas: 5000.0, reverb: (0.16, 0.4), ..COURT },
        Kind::VoiceDisconnect => Recette { notes: const { &[n(0.0, LA4, 0.12, 0.16), n(0.10, RE4, 0.22, 0.42), n(0.10, RE3, 0.2, 0.3).gain(0.35)] }, timbre: CLAIR, duree: 0.66, passe_bas: 5000.0, reverb: (0.16, 0.4), ..COURT },
        // Direct : trois notes rapides, vers le haut puis vers le bas.
        Kind::StreamStart => Recette { notes: const { &[n(0.0, RE4, 0.08, 0.10), n(0.07, MI4, 0.08, 0.10), n(0.14, LA4, 0.22, 0.38)] }, timbre: CLAIR, duree: 0.62, passe_bas: 5000.0, ..COURT },
        Kind::StreamStop => Recette { notes: const { &[n(0.0, LA4, 0.08, 0.10), n(0.07, MI4, 0.08, 0.10), n(0.14, RE4, 0.22, 0.38)] }, timbre: CLAIR, duree: 0.62, passe_bas: 5000.0, ..COURT },
        // Message : un appui bref puis la tierce, qui sonne ; mention : plus claire, plus haute, à deux voix.
        Kind::Message => Recette { notes: const { &[n(0.0, RE4, 0.06, 0.08).gain(0.6), n(0.045, FA4, 0.16, 0.34)] }, niveau: -1.0, ..COURT },
        Kind::Mention => Recette { notes: const { &[n(0.0, FA4, 0.12, 0.16), n(0.11, LA4, 0.25, 0.5), n(0.11, RE4, 0.25, 0.5).gain(0.4)] }, timbre: CLAIR, duree: 0.75, passe_bas: 5000.0, ..COURT },
        Kind::FriendRequest => Recette { notes: const { &[n(0.0, RE4, 0.10, 0.14), n(0.12, SI3, 0.10, 0.14), n(0.24, MI4, 0.25, 0.42)] }, duree: 0.78, ..COURT },
        // Événement : l'accord de ré égrené vers le haut.
        Kind::EventStart => Recette { notes: const { &[n(0.0, LA3, 0.12, 0.16), n(0.11, RE4, 0.12, 0.16), n(0.22, FA4, 0.12, 0.16), n(0.33, LA4, 0.3, 0.55)] }, timbre: CLAIR, duree: 1.0, passe_bas: 5000.0, reverb: (0.16, 0.45), fondu: 0.1, ..COURT },
        // Réaction : une goutte, très brève et discrète.
        Kind::Reaction => Recette { notes: const { &[n(0.0, LA4, 0.06, 0.12).glisse(-2.0)] }, duree: 0.24, reverb: (0.12, 0.2), fondu: 0.05, niveau: -5.0, ..COURT },
        // Rappel : deux fois la même quarte montante.
        Kind::Reminder => Recette { notes: const { &[n(0.0, SI3, 0.2, 0.3), n(0.14, MI4, 0.2, 0.3), n(0.5, SI3, 0.2, 0.3), n(0.64, MI4, 0.25, 0.5)] }, duree: 1.3, fondu: 0.1, ..COURT },
        // Appel entrant : petite phrase de six notes sur l'accord de ré (elle
        // monte, redescend, se pose), puis un silence ; deux cycles de deux secondes.
        Kind::Ring => Recette {
            notes: const {
                &[
                n(0.0, LA3, 0.22, 0.30).pan(-0.2),
                n(0.15, RE4, 0.22, 0.30).pan(0.2),
                n(0.30, FA4, 0.25, 0.40).pan(-0.1),
                n(0.60, MI4, 0.22, 0.30).pan(0.2),
                n(0.75, RE4, 0.22, 0.30).pan(-0.2),
                n(0.90, LA3, 0.40, 0.80),
                n(0.0, RE3, 0.40, 0.60).gain(0.4),
                n(0.90, RE3, 0.45, 0.80).gain(0.4),
                ]
            },
            timbre: CLAIR,
            duree: 2.0,
            cycles: 2,
            passe_bas: 5000.0,
            reverb: (0.18, 0.5),
            fondu: 0.25,
            niveau: 0.0,
        },
        // Appel sortant : deux notes lentes, deux fois, puis un long silence.
        Kind::CallOut => Recette { notes: const { &[n(0.0, LA3, 0.30, 0.50), n(0.32, RE4, 0.35, 0.60), n(1.10, LA3, 0.30, 0.50), n(1.42, RE4, 0.35, 0.60)] }, duree: 3.0, passe_bas: 3000.0, reverb: (0.18, 0.5), fondu: 0.3, niveau: -5.0, ..COURT },
    }
}

/// Fabrique le son décrit par `r` : `f32`, 48 kHz, stéréo entrelacée.
pub fn rendre(r: &Recette) -> Vec<f32> {
    let mut buf = vec![0.0f32; 2 * synth::trames(r.duree)];
    for note in r.notes {
        synth::jouer(&mut buf, note, &r.timbre);
    }
    synth::passe_bas(&mut buf, r.passe_bas);
    synth::reverberer(&mut buf, r.reverb.0, r.reverb.1);
    synth::sans_continu(&mut buf);
    synth::fondre(&mut buf, r.fondu);
    synth::niveler(&mut buf, CIBLE + r.niveau);
    let cycle = buf.len();
    for _ in 1..r.cycles {
        buf.extend_from_within(..cycle);
    }
    buf
}

/// Son intégré de `kind`, en 16 bits (48 kHz, stéréo entrelacée).
pub fn integre(kind: Kind) -> Vec<i16> {
    synth::en_i16(&rendre(&recette(kind)))
}

#[cfg(test)]
mod tests;
