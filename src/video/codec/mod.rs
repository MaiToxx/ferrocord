//! Décodeurs et encodeurs vidéo.
//!
//! H.264 partout ; VP8 quand Windows a les « Extensions vidéo VP9 » (voir
//! `vp8.rs`). VP9 et AV1 sont décodés seulement, par les mêmes extensions
//! et par l'« Extension vidéo AV1 » (voir `av1.rs`) : [`encoder`] rend
//! `None` pour eux. La négociation ne doit annoncer que
//! [`supported_decoders`] et [`supported_encoders`].
//!
//! VP8 (mire 720p30 à 1,4 Mb/s, `bench_vp8_720p`, même machine) : 6,7 à
//! 7,7 ms/image à l'encodage, 2,6 ms/image au décodage, une image rendue par
//! image d'entrée ; aucune dépendance, +26 Kio sur l'exécutable de release
//! (29 175 808 octets contre 29 149 184 à `711303e`).
//!
//! Choix, mesuré sur Ryzen 7 4800H + Radeon RX 5600M, mire 1080p30 à 4 Mb/s
//! (`bench_codecs_1080p`, mode optimisé) :
//!
//! | | encodage | décodage |
//! |---|---|---|
//! | Media Foundation | 8,4 ms/image, matériel (AMD) : processeur quasi libre | 6,0 ms/image (décodeur de Windows) |
//! | OpenH264 | 41,9 ms/image : ne tient pas 1080p30, un cœur saturé | 7,6 ms/image |
//!
//! D'où : encodeur matériel par Media Foundation ([`mf`]), sinon OpenH264 ;
//! décodeur de Windows, sinon OpenH264 (éditions N sans Media Feature Pack,
//! fil en STA). Les deux encodeurs rendent une image par image d'entrée
//! (aucune image B, aucun délai ajouté). Taille : OpenH264 + liaisons Media
//! Foundation + conversions = +752 Kio sur l'exécutable de release ; garder
//! OpenH264 comme repli assure la vidéo sur toute machine.

#[cfg(windows)]
mod av1;
mod h264;
#[cfg(windows)]
mod vp8;
#[cfg(windows)]
mod mf;
#[cfg(test)]
mod tests;
#[cfg(all(test, windows))]
mod tests_extra;
#[cfg(all(test, windows))]
mod tests_vp8;

use super::{Codec, EncodedFrame, Frame};

#[cfg(windows)]
pub use mf::reader::FileReader;

/// Erreur d'un codec. Après [`CodecError::Corrupt`], le décodeur reste
/// utilisable mais il faut une image clé pour repartir proprement : c'est
/// le moment d'en demander une à l'émetteur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    /// Flux illisible (paquet perdu, données abîmées).
    Corrupt(String),
    /// Image d'entrée incohérente (taille des plans, dimensions nulles ou trop grandes).
    BadFrame(&'static str),
    /// Image confiée au mauvais codec.
    WrongCodec(Codec),
    /// Le codec lui-même a échoué (initialisation, ressources).
    Internal(String),
}

impl std::fmt::Display for CodecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Corrupt(m) => write!(f, "flux vidéo illisible : {m}"),
            Self::BadFrame(m) => write!(f, "image invalide : {m}"),
            Self::WrongCodec(c) => write!(f, "codec inattendu : {c:?}"),
            Self::Internal(m) => write!(f, "erreur du codec : {m}"),
        }
    }
}

impl std::error::Error for CodecError {}

pub trait Decoder: Send {
    /// Décode une image complète (H.264 : une unité d'accès en Annex B).
    /// `Ok(None)` : rien à montrer pour l'instant (paramètres seuls, image
    /// sautée, attente d'une image clé).
    fn decode(&mut self, f: &EncodedFrame) -> Result<Option<Frame>, CodecError>;
}

pub trait Encoder: Send {
    /// Encode une image ; liste vide si l'encodeur la saute (débit dépassé).
    /// Une image de taille différente de la précédente reconfigure
    /// l'encodeur et donne une image clé.
    fn encode(&mut self, f: &Frame, force_keyframe: bool) -> Result<Vec<EncodedFrame>, CodecError>;
    /// Débit visé, en bits par seconde.
    fn set_bitrate(&mut self, bps: u32);
}

pub fn decoder(c: Codec) -> Option<Box<dyn Decoder>> {
    match c {
        Codec::H264 => system_decoder().or_else(software_decoder),
        Codec::Vp8 => vp8_decoder(),
        Codec::Vp9 | Codec::Av1 => extension_decoder(c),
    }
}

pub fn encoder(c: Codec, w: u32, h: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
    match c {
        Codec::H264 => hardware_encoder(w, h, fps, bps).or_else(|| software_encoder(w, h, fps, bps)),
        Codec::Vp8 => vp8_encoder(w, h, fps, bps),
        Codec::Vp9 | Codec::Av1 => None,
    }
}

/// Encodeur de repli quand celui de [`encoder`] a lâché en cours d'envoi :
/// H.264 logiciel (OpenH264), sans passer par la carte graphique. `None`
/// pour les autres codecs (ils n'ont qu'un encodeur).
pub fn fallback_encoder(c: Codec, w: u32, h: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
    if c == Codec::H264 { software_encoder(w, h, fps, bps) } else { None }
}

/// Encodeur matériel par Media Foundation, s'il y en a un.
fn hardware_encoder(w: u32, h: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
    #[cfg(windows)]
    return mf::MfEncoder::new(w, h, fps, bps).ok().map(|e| Box::new(e) as Box<dyn Encoder>);
    #[cfg(not(windows))]
    return {
        let _ = (w, h, fps, bps);
        None
    };
}

/// Décodeur H.264 de Windows (Media Foundation), s'il est installé (absent
/// des éditions N sans le Media Feature Pack).
fn system_decoder() -> Option<Box<dyn Decoder>> {
    #[cfg(windows)]
    return mf::MfDecoder::new().ok().map(|d| Box::new(d) as Box<dyn Decoder>);
    #[cfg(not(windows))]
    return None;
}

fn software_encoder(w: u32, h: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
    h264::H264Encoder::new(w, h, fps, bps).ok().map(|e| Box::new(e) as Box<dyn Encoder>)
}

fn software_decoder() -> Option<Box<dyn Decoder>> {
    h264::H264Decoder::new().ok().map(|d| Box::new(d) as Box<dyn Decoder>)
}

/// Décodeur VP8 de Windows (voir `vp8.rs`), s'il est installé.
fn vp8_decoder() -> Option<Box<dyn Decoder>> {
    #[cfg(windows)]
    return mf::MfDecoder::vp8().ok().map(|d| Box::new(d) as Box<dyn Decoder>);
    #[cfg(not(windows))]
    return None;
}

/// Décodeur VP9 ou AV1 de Windows, s'il est installé.
fn extension_decoder(c: Codec) -> Option<Box<dyn Decoder>> {
    #[cfg(windows)]
    return if c == Codec::Av1 { mf::MfDecoder::av1() } else { mf::MfDecoder::vp9() }.ok().map(|d| Box::new(d) as Box<dyn Decoder>);
    #[cfg(not(windows))]
    return {
        let _ = c;
        None
    };
}

/// Décodeur pour un fichier (lecteur du fil, lot VID). À appeler sur le fil
/// qui décodera.
pub fn file_decoder(c: Codec) -> Option<Box<dyn Decoder>> {
    decoder(c)
}

/// VP9 décodable ici (fil dans le MTA) : pour les essais.
#[cfg(all(test, windows))]
pub(crate) fn vp9_decodable() -> bool {
    mf::extra_support().0
}

/// VP9 et AV1 décodables sur cette machine. Relevé une fois, sur un fil à
/// part, comme [`vp8_support`].
fn extra_support() -> (bool, bool) {
    #[cfg(windows)]
    {
        static SUPPORT: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
        *SUPPORT.get_or_init(|| std::thread::Builder::new().name("ferrocord-codecs".into()).spawn(mf::extra_support).ok().and_then(|t| t.join().ok()).unwrap_or_default())
    }
    #[cfg(not(windows))]
    (false, false)
}

/// Encodeur VP8 de Windows (voir `vp8.rs`), s'il est installé.
fn vp8_encoder(w: u32, h: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
    #[cfg(windows)]
    return mf::MfVp8Encoder::new(w, h, fps, bps).ok().map(|e| Box::new(e) as Box<dyn Encoder>);
    #[cfg(not(windows))]
    return {
        let _ = (w, h, fps, bps);
        None
    };
}

/// VP8 sur cette machine : (encodeur, décodeur). Relevé une fois, sur un
/// fil à part : Media Foundation veut le MTA, et l'appelant peut être en STA.
fn vp8_support() -> (bool, bool) {
    #[cfg(windows)]
    {
        static SUPPORT: std::sync::OnceLock<(bool, bool)> = std::sync::OnceLock::new();
        *SUPPORT.get_or_init(|| std::thread::Builder::new().name("ferrocord-codecs".into()).spawn(mf::vp8_support).ok().and_then(|t| t.join().ok()).unwrap_or_default())
    }
    #[cfg(not(windows))]
    (false, false)
}

/// Codecs décodables, dans l'ordre de préférence.
pub fn supported_decoders() -> Vec<Codec> {
    let mut v = vec![Codec::H264];
    if vp8_support().1 {
        v.push(Codec::Vp8);
    }
    let (vp9, av1) = extra_support();
    if vp9 {
        v.push(Codec::Vp9);
    }
    if av1 {
        v.push(Codec::Av1);
    }
    v
}

/// Codecs encodables, dans l'ordre de préférence.
pub fn supported_encoders() -> Vec<Codec> {
    let mut v = vec![Codec::H264];
    if vp8_support().0 {
        v.push(Codec::Vp8);
    }
    v
}
