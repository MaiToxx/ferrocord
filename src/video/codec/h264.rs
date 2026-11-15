//! H.264 par OpenH264 (bibliothèque C++ de Cisco, BSD-2, compilée depuis
//! ses sources et liée statiquement, enveloppée par le crate sûr `openh264`) :
//! le repli logiciel quand Media Foundation n'offre rien (voir `mod.rs`).
//!
//! Profil produit : Constrained Baseline (celui des clients WebRTC et de
//! Discord), une tranche par image, sans images B : aucune latence ajoutée.

use std::time::{Duration, Instant};

use openh264::decoder::Decoder as OhDecoder;
use openh264::encoder::{BitRate, Encoder as OhEncoder, EncoderConfig, FrameRate, FrameType, RateControlMode, UsageType};
use openh264::formats::{YUVSlices, YUVSource};
use openh264::{OpenH264API, Timestamp};

use super::{CodecError, Decoder, Encoder};
use crate::video::{Codec, EncodedFrame, Frame, FrameData, convert, ts90k_to_us, us_to_90k};

/// Limite d'OpenH264 (niveau 5.2).
const MAX_LONG: u32 = 3840;
const MAX_SHORT: u32 = 2160;

/// Un nouveau débit n'est appliqué qu'en reconstruisant l'encodeur (le crate
/// sûr ne permet pas de le changer à chaud), ce qui coûte une image clé :
/// les petits écarts attendent la prochaine image clé, les grands sont
/// appliqués tout de suite mais pas plus d'une fois par intervalle.
const BITRATE_STEP: f64 = 0.25;
const REBUILD_INTERVAL: Duration = Duration::from_secs(2);

pub struct H264Encoder {
    enc: OhEncoder,
    fps: u32,
    bps: u32,
    pending_bps: Option<u32>,
    built_at: Instant,
    /// Plans I420 réutilisés quand l'image arrive en BGRA.
    y: Vec<u8>,
    u: Vec<u8>,
    v: Vec<u8>,
}

fn build(fps: u32, bps: u32) -> Result<OhEncoder, CodecError> {
    let config = EncoderConfig::new()
        .bitrate(BitRate::from_bps(bps))
        .max_frame_rate(FrameRate::from_hz(fps as f32))
        .rate_control_mode(RateControlMode::Bitrate)
        .usage_type(UsageType::CameraVideoRealTime)
        .skip_frames(true);
    OhEncoder::with_api_config(OpenH264API::from_source(), config).map_err(|e| CodecError::Internal(e.to_string()))
}

pub(super) fn check_size(w: u32, h: u32) -> Result<(), CodecError> {
    if w < 2 || h < 2 {
        return Err(CodecError::BadFrame("image trop petite"));
    }
    if w.max(h) > MAX_LONG || w.min(h) > MAX_SHORT {
        return Err(CodecError::BadFrame("image plus grande que 3840×2160"));
    }
    Ok(())
}

impl H264Encoder {
    pub fn new(w: u32, h: u32, fps: u32, bps: u32) -> Result<Self, CodecError> {
        check_size(w, h)?;
        let fps = fps.clamp(1, 240);
        let bps = bps.max(1);
        Ok(Self { enc: build(fps, bps)?, fps, bps, pending_bps: None, built_at: Instant::now(), y: Vec::new(), u: Vec::new(), v: Vec::new() })
    }

    /// Applique le débit en attente si c'est le moment (l'image suivante
    /// sera alors une image clé).
    fn apply_bitrate(&mut self, force_keyframe: bool) -> Result<(), CodecError> {
        let Some(target) = self.pending_bps else { return Ok(()) };
        let ratio = f64::from(target) / f64::from(self.bps);
        let big = (ratio - 1.0).abs() >= BITRATE_STEP;
        if force_keyframe || (big && self.built_at.elapsed() >= REBUILD_INTERVAL) {
            self.enc = build(self.fps, target)?;
            self.bps = target;
            self.pending_bps = None;
            self.built_at = Instant::now();
        }
        Ok(())
    }
}

impl Encoder for H264Encoder {
    fn encode(&mut self, f: &Frame, force_keyframe: bool) -> Result<Vec<EncodedFrame>, CodecError> {
        if !f.is_consistent() {
            return Err(CodecError::BadFrame("taille des plans incohérente"));
        }
        check_size(f.width, f.height)?;
        self.apply_bitrate(force_keyframe)?;
        let (w, h) = (f.width as usize, f.height as usize);
        let (y, u, v): (&[u8], &[u8], &[u8]) = match &f.data {
            FrameData::I420 { y, u, v } => (y, u, v),
            FrameData::Bgra(b) => {
                convert::bgra_to_i420(w, h, b, &mut self.y, &mut self.u, &mut self.v).map_err(|_| CodecError::BadFrame("BGRA incohérent"))?;
                (&self.y, &self.u, &self.v)
            }
        };
        // H.264 en 4:2:0 veut des dimensions paires : on rogne la dernière
        // colonne ou ligne impaire en gardant le pas d'origine.
        let (ew, eh, cw) = (w & !1, h & !1, w.div_ceil(2));
        let yuv = YUVSlices::new((&y[..eh * w], &u[..eh / 2 * cw], &v[..eh / 2 * cw]), (ew, eh), (w, cw, cw));
        if force_keyframe {
            self.enc.force_intra_frame();
        }
        let bs = self.enc.encode_at(&yuv, Timestamp::from_millis(f.pts_us / 1000)).map_err(|e| CodecError::Internal(e.to_string()))?;
        let keyframe = match bs.frame_type() {
            FrameType::Skip | FrameType::Invalid => return Ok(Vec::new()),
            FrameType::IDR => true,
            FrameType::I | FrameType::P | FrameType::IPMixed => false,
        };
        let data = bs.to_vec();
        if data.is_empty() {
            return Ok(Vec::new());
        }
        Ok(vec![EncodedFrame { codec: Codec::H264, keyframe, ts90k: us_to_90k(f.pts_us), data }])
    }

    fn set_bitrate(&mut self, bps: u32) {
        let bps = bps.max(1);
        self.pending_bps = (bps != self.bps).then_some(bps);
    }
}

pub struct H264Decoder {
    dec: OhDecoder,
}

impl H264Decoder {
    pub fn new() -> Result<Self, CodecError> {
        Ok(Self { dec: OhDecoder::new().map_err(|e| CodecError::Internal(e.to_string()))? })
    }
}

/// Recopie un plan à pas `stride` en plan contigu de `w`×`h`.
fn pack(src: &[u8], stride: usize, w: usize, h: usize) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(w * h);
    for row in 0..h {
        out.extend_from_slice(src.get(row * stride..row * stride + w)?);
    }
    Some(out)
}

impl Decoder for H264Decoder {
    fn decode(&mut self, f: &EncodedFrame) -> Result<Option<Frame>, CodecError> {
        if f.codec != Codec::H264 {
            return Err(CodecError::WrongCodec(f.codec));
        }
        if f.data.is_empty() {
            return Err(CodecError::Corrupt("image vide".into()));
        }
        let Some(img) = self.dec.decode(&f.data).map_err(|e| CodecError::Corrupt(e.to_string()))? else {
            return Ok(None);
        };
        let (w, h) = img.dimensions();
        let (sy, su, sv) = img.strides();
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let planes = (pack(img.y(), sy, w, h), pack(img.u(), su, cw, ch), pack(img.v(), sv, cw, ch));
        let (Some(y), Some(u), Some(v)) = planes else {
            return Err(CodecError::Internal("plans décodés plus courts qu'annoncé".into()));
        };
        let (Ok(width), Ok(height)) = (u32::try_from(w), u32::try_from(h)) else {
            return Err(CodecError::Internal("dimensions décodées hors limites".into()));
        };
        Ok(Some(Frame { width, height, pts_us: ts90k_to_us(f.ts90k), data: FrameData::I420 { y, u, v } }))
    }
}
