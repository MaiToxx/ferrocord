//! Vidéo et partage d'écran (phase 5) : types communs aux lots M1 à M4.
//!
//! - [`capture`] : caméras, écrans, fenêtres et son du partage (lot M3) ;
//! - [`codec`] : décodeurs et encodeurs (H.264, lot M2 ; VP8, lot V) ;
//! - [`convert`] : conversions de couleurs et mise à l'échelle (lot M2) ;
//! - [`mkv`] : démultiplexeur Matroska/WebM (lot VID) ;
//! - [`play`] : lecteur des fichiers joints aux messages (phase 10, lot M) ;
//! - transport (RTP, RTX, RTCP) : `net::voice::video` (lot M1).

pub mod capture;
pub mod codec;
pub mod convert;
pub mod mkv;
pub mod play;

/// Codecs vidéo connus de Discord (Select Protocol, `codecs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Codec {
    H264,
    Vp8,
    Vp9,
    Av1,
}

impl Codec {
    /// Nom dans `codecs` (Select Protocol) et `video_codec` (Session Description).
    pub fn name(self) -> &'static str {
        match self {
            Codec::H264 => "H264",
            Codec::Vp8 => "VP8",
            Codec::Vp9 => "VP9",
            Codec::Av1 => "AV1",
        }
    }

    pub fn from_name(s: &str) -> Option<Codec> {
        [Codec::H264, Codec::Vp8, Codec::Vp9, Codec::Av1].into_iter().find(|c| c.name().eq_ignore_ascii_case(s))
    }
}

/// Image décodée ou capturée.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    /// Instant de l'image, en microsecondes depuis le début du flux.
    pub pts_us: u64,
    pub data: FrameData,
}

/// Pixels d'une [`Frame`], plans contigus sans marge en fin de ligne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FrameData {
    /// Plans Y (`width * height`), U et V (`ceil(width / 2) * ceil(height / 2)`).
    I420 { y: Vec<u8>, u: Vec<u8>, v: Vec<u8> },
    /// Octets B, G, R, A, ligne après ligne, sans bourrage (`width * height * 4`).
    Bgra(Vec<u8>),
}

/// Image compressée, prête à partir ou tout juste reçue.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EncodedFrame {
    pub codec: Codec,
    pub keyframe: bool,
    /// Horodatage RTP (horloge à 90 kHz).
    pub ts90k: u32,
    /// H.264 : Annex B (codes de début avant chaque NAL).
    pub data: Vec<u8>,
}

/// Qualité demandée à une capture ou à un partage. `width` et `height` à 0 :
/// taille d'origine (« Source ») ; préréglages dans `capture`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Quality {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl Frame {
    /// Vrai si la taille des plans correspond aux dimensions annoncées.
    pub fn is_consistent(&self) -> bool {
        let (w, h) = (self.width as usize, self.height as usize);
        match &self.data {
            FrameData::I420 { y, u, v } => {
                let c = w.div_ceil(2) * h.div_ceil(2);
                y.len() == w * h && u.len() == c && v.len() == c
            }
            FrameData::Bgra(b) => b.len() == w * h * 4,
        }
    }
}

/// Microsecondes → horloge RTP à 90 kHz (repli modulo 2³², comme RTP).
pub fn us_to_90k(pts_us: u64) -> u32 {
    (pts_us.wrapping_mul(9) / 100) as u32
}

/// Horloge RTP à 90 kHz → microsecondes (sans tenir compte des replis).
pub fn ts90k_to_us(ts90k: u32) -> u64 {
    u64::from(ts90k) * 100 / 9
}
