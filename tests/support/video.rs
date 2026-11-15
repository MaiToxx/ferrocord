//! Fausse vidéo pour l'orchestrateur (lot M5) : une source qui donne une
//! mire toutes les 33 ms sur son fil, un faux encodeur H.264 (unités
//! d'accès Annex B qui portent la taille de l'image), un faux décodeur qui
//! la relit, et le faux périphérique audio pour le son d'un partage. Aucune
//! caméra, aucun écran, aucun codec réel n'est touché.

#![allow(dead_code)]

use super::audio::{Backend, FakeAudio};
use ferrocord::video::capture::{CaptureError, Quality, Source, SourceKind};
use ferrocord::video::codec::{CodecError, Decoder, Encoder};
use ferrocord::video::{us_to_90k, Codec, EncodedFrame, Frame, FrameData};
use ferrocord::voice::{AudioBackend, Capturing, VideoBackend};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering::SeqCst};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Octet qui rend une image illisible pour le faux décodeur.
pub const CORRUPT: u8 = 0xEE;

pub struct FakeVideo {
    /// Taille des images de la source.
    pub size: Mutex<(u32, u32)>,
    /// Plus grande définition acceptée par le faux encodeur (« carte graphique »).
    pub max_encode: Mutex<(u32, u32)>,
    pub open_now: AtomicUsize,
    pub opened: AtomicUsize,
    /// Décodeurs vivants.
    pub decoders_now: AtomicUsize,
    /// Images rendues par les décodeurs.
    pub decoded: AtomicUsize,
    /// Dernière source ouverte.
    pub last_source: Mutex<Option<String>>,
    pub audio: Arc<FakeAudio>,
}

impl FakeVideo {
    pub fn new(audio: Arc<FakeAudio>) -> Arc<Self> {
        Arc::new(FakeVideo {
            size: Mutex::new((320, 180)),
            max_encode: Mutex::new((4096, 4096)),
            open_now: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
            decoders_now: AtomicUsize::new(0),
            decoded: AtomicUsize::new(0),
            last_source: Mutex::new(None),
            audio,
        })
    }
}

/// Unité d'accès factice : SPS et PPS puis IDR pour une image clé, sinon une
/// tranche ; la taille de l'image (÷ 8) en tête de charge.
pub fn access_unit(key: bool, width: u32, height: u32, extra: u8) -> Vec<u8> {
    let mut d = Vec::new();
    if key {
        d.extend_from_slice(&[0, 0, 0, 1, 0x67, 0x42, 0xc0, 0x1f, 0, 0, 0, 1, 0x68, 0xce, 0x3c, 0x80]);
    }
    d.extend_from_slice(&[0, 0, 0, 1, if key { 0x65 } else { 0x41 }, (width / 8) as u8, (height / 8) as u8, extra]);
    d.extend(std::iter::repeat_n(0x55, 300));
    d
}

/// Image clé dans ces données (IDR).
pub fn is_key(data: &[u8]) -> bool {
    data.windows(4).any(|w| w[..3] == [0, 0, 1] && w[3] == 0x65)
}

struct Running {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    dev: Arc<FakeVideo>,
}

impl Capturing for Running {
    fn ended(&self) -> bool {
        false
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.store(true, SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        self.dev.open_now.fetch_sub(1, SeqCst);
    }
}

struct FakeEnc {
    first: bool,
}

impl Encoder for FakeEnc {
    fn encode(&mut self, f: &Frame, force_keyframe: bool) -> Result<Vec<EncodedFrame>, CodecError> {
        let key = std::mem::take(&mut self.first) || force_keyframe;
        Ok(vec![EncodedFrame { codec: Codec::H264, keyframe: key, ts90k: us_to_90k(f.pts_us), data: access_unit(key, f.width, f.height, 1) }])
    }
    fn set_bitrate(&mut self, _bps: u32) {}
}

struct FakeDec {
    dev: Arc<FakeVideo>,
}

impl Drop for FakeDec {
    fn drop(&mut self) {
        self.dev.decoders_now.fetch_sub(1, SeqCst);
    }
}

impl Decoder for FakeDec {
    fn decode(&mut self, f: &EncodedFrame) -> Result<Option<Frame>, CodecError> {
        // Dernière NAL : son en-tête, puis largeur et hauteur ÷ 8, puis l'octet témoin.
        let p = f.data.windows(3).rposition(|w| w == [0, 0, 1]).ok_or(CodecError::Corrupt("pas de NAL".into()))?;
        let body = f.data.get(p + 3..p + 7).ok_or(CodecError::Corrupt("court".into()))?;
        if body[3] == CORRUPT {
            return Err(CodecError::Corrupt("illisible".into()));
        }
        let (w, h) = (u32::from(body[1]) * 8, u32::from(body[2]) * 8);
        let (cw, ch) = (w.div_ceil(2) as usize, h.div_ceil(2) as usize);
        self.dev.decoded.fetch_add(1, SeqCst);
        Ok(Some(Frame { width: w, height: h, pts_us: 0, data: FrameData::I420 { y: vec![128; (w * h) as usize], u: vec![128; cw * ch], v: vec![128; cw * ch] } }))
    }
}

/// La fausse vidéo, partagée avec le test.
pub struct Video(pub Arc<FakeVideo>);

impl VideoBackend for Video {
    fn cameras(&self) -> Vec<Source> {
        vec![Source { id: "camera:fausse".into(), name: "Fausse caméra".into(), kind: SourceKind::Camera }]
    }

    fn open(&self, source: &Source, _want: Quality, mut on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<Box<dyn Capturing>, CaptureError> {
        if source.id.ends_with(":absente") {
            return Err(CaptureError::NotFound);
        }
        let dev = self.0.clone();
        *dev.last_source.lock().unwrap_or_else(|e| e.into_inner()) = Some(source.id.clone());
        dev.open_now.fetch_add(1, SeqCst);
        dev.opened.fetch_add(1, SeqCst);
        let (w, h) = *dev.size.lock().unwrap_or_else(|e| e.into_inner());
        let stop = Arc::new(AtomicBool::new(false));
        let s = stop.clone();
        let thread = std::thread::spawn(move || {
            let mut step = 0u32;
            while !s.load(SeqCst) {
                on_frame(ferrocord::state::video::test_pattern(w, h, step, 3));
                step += 1;
                std::thread::sleep(Duration::from_millis(33));
            }
        });
        Ok(Box::new(Running { stop, thread: Some(thread), dev }))
    }

    fn encoder(&self, codec: Codec, width: u32, height: u32, _fps: u32, _bps: u32) -> Option<Box<dyn Encoder>> {
        let (mw, mh) = *self.0.max_encode.lock().unwrap_or_else(|e| e.into_inner());
        (codec == Codec::H264 && width <= mw && height <= mh).then(|| Box::new(FakeEnc { first: true }) as Box<dyn Encoder>)
    }

    fn decoder(&self, codec: Codec) -> Option<Box<dyn Decoder>> {
        (codec == Codec::H264).then(|| {
            self.0.decoders_now.fetch_add(1, SeqCst);
            Box::new(FakeDec { dev: self.0.clone() }) as Box<dyn Decoder>
        })
    }

    fn share_sound(&self, source: &Source) -> Option<Arc<dyn AudioBackend>> {
        (source.kind != SourceKind::Camera).then(|| Arc::new(Backend(self.0.audio.clone())) as Arc<dyn AudioBackend>)
    }
}
