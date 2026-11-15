//! Audio du vocal (phase 4). Format commun : 48 kHz, trames de 20 ms
//! (960 échantillons), `f32` dans [-1, 1] ; capture en mono, lecture en
//! stéréo entrelacée.
//!
//! - [`engine`] : codec Opus, tampon de gigue, mixage, porte (lot V2) ;
//! - [`io`] : périphériques Windows (capture, lecture, rééchantillonnage) ;
//! - [`dsp`] : traitement de la voix captée (passe-haut, écho, bruit, gain) ;
//! - [`clip`] : sons courts (tableau de sons, messages vocaux) : décodage
//!   MP3 et Ogg/Opus, enregistrement en Ogg/Opus, onde (phase 6).

pub mod clip;
pub mod dsp;
pub mod engine;
pub mod io;

/// Traitement de la capture (suppression d'écho, de bruit, gain…) appliqué
/// avant la porte et l'encodage.
pub trait Processor: Send {
    fn process(&mut self, mono: &mut [f32] /* 960 */, far_end: &[f32] /* dernière trame lue, mono, pour l'écho */);
}
