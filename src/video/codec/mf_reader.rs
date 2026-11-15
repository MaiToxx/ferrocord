//! Lecteur de fichiers de Media Foundation (`IMFSourceReader`) : démultiplexe
//! et décode un fichier tenu en mémoire (MP4, MOV, M4A, WAV, AAC…) avec les
//! décodeurs de Windows. Sert au lecteur du fil (`video::play`, lot M) ;
//! aucun octet de bibliothèque ajouté.
//!
//! Module enfant de `mf` : il en reprend l'initialisation du fil, la copie
//! des échantillons et la mise en plans I420. Les appels COM sont `unsafe`
//! (crate `windows`) ; chaque bloc dit ce qui le rend correct.
//!
//! Fil : l'objet est créé et utilisé par un seul fil (celui du lecteur), dans
//! le MTA. Il n'est pas `Send`.

use windows::Win32::Foundation::HGLOBAL;
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, PROPVARIANT};
use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET};
use windows::Win32::System::Variant::{VT_I8, VT_UI8};
use windows::core::{GUID, HSTRING, Interface};

use super::{HNS_PER_US, Shape, hr, i420_frame, ready, sample_bytes, shape_of, video_type};
use crate::video::Frame;
use crate::video::codec::CodecError;
use crate::video::play::{Chunk, Info, Source};

const VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
const AUDIO: u32 = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
const ALL: u32 = MF_SOURCE_READER_ALL_STREAMS.0 as u32;
const SOURCE: u32 = MF_SOURCE_READER_MEDIASOURCE.0 as u32;
/// Lectures sans échantillon tolérées d'affilée (trous du flux, changements de type).
const MAX_EMPTY: usize = 64;

/// Format du son rendu par le lecteur.
#[derive(Clone, Copy)]
struct AudioFmt {
    rate: u32,
    channels: usize,
    /// `f32` (sinon entiers de 16 bits).
    float: bool,
}

pub struct FileReader {
    reader: IMFSourceReader,
    info: Info,
    shape: Option<Shape>,
    audio: Option<AudioFmt>,
    bytes: Vec<u8>,
}

/// Flux mémoire COM contenant une copie de `data`, curseur au début.
fn memory_stream(data: &[u8]) -> windows::core::Result<IStream> {
    // SAFETY: le flux possède sa mémoire (poignée nulle : il l'alloue, et la
    // libère avec lui) ; chaque écriture lit `part.len()` octets de `part`,
    // vivant pendant l'appel.
    unsafe {
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        for part in data.chunks(1 << 30) {
            let mut done = 0u32;
            stream.Write(part.as_ptr().cast(), part.len() as u32, Some(&mut done)).ok()?;
            if done as usize != part.len() {
                return Err(windows::Win32::Foundation::E_OUTOFMEMORY.into());
            }
        }
        stream.Seek(0, STREAM_SEEK_SET, None)?;
        Ok(stream)
    }
}

/// Type audio non compressé demandé au lecteur.
fn audio_type(subtype: &GUID) -> windows::core::Result<IMFMediaType> {
    // SAFETY: objet neuf, clés et valeurs passées par référence valide.
    unsafe {
        let t = MFCreateMediaType()?;
        t.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
        t.SetGUID(&MF_MT_SUBTYPE, subtype)?;
        Ok(t)
    }
}

impl FileReader {
    /// Media Foundation utilisable sur ce fil.
    pub fn available() -> bool {
        ready()
    }

    /// Ouvre un fichier ; `mime` aide Windows à reconnaître le conteneur.
    /// [`CodecError::Internal`] : Media Foundation absent ; autre erreur :
    /// fichier que Windows ne sait pas lire.
    pub fn open(data: &[u8], mime: Option<&str>) -> Result<Self, CodecError> {
        if !ready() {
            return Err(CodecError::Internal("Media Foundation indisponible sur ce fil".into()));
        }
        let stream = memory_stream(data).map_err(hr)?;
        // SAFETY: objets neufs et possédés ; la chaîne du type vit pendant l'appel.
        let reader = unsafe {
            (|| -> windows::core::Result<IMFSourceReader> {
                let bytes = MFCreateMFByteStreamOnStream(&stream)?;
                if let (Some(m), Ok(attrs)) = (mime, bytes.cast::<IMFAttributes>()) {
                    let _ = attrs.SetString(&MF_BYTESTREAM_CONTENT_TYPE, &HSTRING::from(m));
                }
                let reader = MFCreateSourceReaderFromByteStream(&bytes, None::<&IMFAttributes>)?;
                reader.SetStreamSelection(ALL, false)?;
                Ok(reader)
            })()
        }
        .map_err(|e| CodecError::Corrupt(format!("ouverture {:#010x}", e.code().0)))?;
        let mut r = FileReader { reader, info: Info::default(), shape: None, audio: None, bytes: Vec::new() };
        r.select_video();
        r.select_audio();
        if r.shape.is_none() && r.audio.is_none() {
            return Err(CodecError::BadFrame("aucun flux lisible"));
        }
        r.info.audio = r.audio.is_some();
        r.info.duration_us = r.duration_us();
        Ok(r)
    }

    /// Première piste vidéo, décodée en NV12 ; sans elle (ou sans décodeur), aucune vidéo.
    fn select_video(&mut self) {
        // SAFETY: objets valides ; le type demandé vit pendant l'appel.
        let t = unsafe {
            (|| -> windows::core::Result<IMFMediaType> {
                self.reader.SetStreamSelection(VIDEO, true)?;
                self.reader.SetCurrentMediaType(VIDEO, None, &video_type(&MFVideoFormat_NV12)?)?;
                self.reader.GetCurrentMediaType(VIDEO)
            })()
        };
        self.shape = t.as_ref().ok().and_then(|t| shape_of(t, true));
        match (self.shape, t) {
            (Some(s), Ok(t)) => {
                // SAFETY: type valide.
                let turned = unsafe { t.GetUINT32(&MF_MT_VIDEO_ROTATION) }.unwrap_or(0) % 360;
                self.info.video = Some((s.width as u32, s.height as u32));
                self.info.rotation = turned as u16;
            }
            _ => {
                // SAFETY: objet valide.
                let _ = unsafe { self.reader.SetStreamSelection(VIDEO, false) };
            }
        }
    }

    /// Première piste audio, décodée en `f32` (à défaut en entiers de 16 bits).
    fn select_audio(&mut self) {
        for (subtype, float) in [(MFAudioFormat_Float, true), (MFAudioFormat_PCM, false)] {
            // SAFETY: objets valides ; le type demandé vit pendant l'appel.
            let fmt = unsafe {
                (|| -> windows::core::Result<AudioFmt> {
                    self.reader.SetStreamSelection(AUDIO, true)?;
                    self.reader.SetCurrentMediaType(AUDIO, None, &audio_type(&subtype)?)?;
                    let t = self.reader.GetCurrentMediaType(AUDIO)?;
                    let bits = t.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE)?;
                    if bits != if float { 32 } else { 16 } {
                        return Err(MF_E_INVALIDMEDIATYPE.into());
                    }
                    Ok(AudioFmt { rate: t.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND)?, channels: t.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS)? as usize, float })
                })()
            };
            if let Ok(f) = fmt
                && f.rate > 0
                && (1..=8).contains(&f.channels)
            {
                self.audio = Some(f);
                return;
            }
        }
        // SAFETY: objet valide.
        let _ = unsafe { self.reader.SetStreamSelection(AUDIO, false) };
    }

    fn duration_us(&self) -> u64 {
        // SAFETY: objet valide ; la valeur rendue est lue selon son type
        // annoncé (`VT_UI8` : entier de 64 bits, rien à libérer).
        unsafe {
            match self.reader.GetPresentationAttribute(SOURCE, &MF_PD_DURATION) {
                Ok(v) if v.Anonymous.Anonymous.vt == VT_UI8 => v.Anonymous.Anonymous.Anonymous.uhVal / HNS_PER_US as u64,
                _ => 0,
            }
        }
    }

    /// Échantillon suivant du flux `stream` et son instant (µs) ; `None` à la fin.
    fn read(&mut self, stream: u32) -> Result<Option<(IMFSample, u64)>, String> {
        for _ in 0..MAX_EMPTY {
            let (mut flags, mut ts, mut sample) = (0u32, 0i64, None);
            // SAFETY: les trois sorties désignent des variables locales vivantes.
            unsafe { self.reader.ReadSample(stream, 0, None, Some(&mut flags), Some(&mut ts), Some(&mut sample)) }.map_err(|e| format!("{:#010x}", e.code().0))?;
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err("flux".into());
            }
            if stream == VIDEO && flags & (MF_SOURCE_READERF_CURRENTMEDIATYPECHANGED.0 | MF_SOURCE_READERF_NATIVEMEDIATYPECHANGED.0) as u32 != 0 {
                // SAFETY: objet valide.
                if let Ok(t) = unsafe { self.reader.GetCurrentMediaType(VIDEO) } {
                    self.shape = shape_of(&t, true).or(self.shape);
                }
            }
            if let Some(s) = sample {
                return Ok(Some((s, u64::try_from(ts / HNS_PER_US).unwrap_or(0))));
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
        }
        Ok(None)
    }
}

impl Source for FileReader {
    fn info(&self) -> Info {
        self.info
    }

    fn video(&mut self) -> Result<Option<Frame>, String> {
        if self.shape.is_none() {
            return Ok(None);
        }
        let Some((s, pts_us)) = self.read(VIDEO)? else { return Ok(None) };
        sample_bytes(&s, &mut self.bytes).map_err(|e| format!("{:#010x}", e.code().0))?;
        let Some(shape) = self.shape else { return Ok(None) };
        i420_frame(&self.bytes, shape, pts_us).map(Some).ok_or_else(|| "image".to_string())
    }

    fn audio(&mut self) -> Result<Option<Chunk>, String> {
        let Some(fmt) = self.audio else { return Ok(None) };
        let Some((s, pts_us)) = self.read(AUDIO)? else { return Ok(None) };
        sample_bytes(&s, &mut self.bytes).map_err(|e| format!("{:#010x}", e.code().0))?;
        let samples = if fmt.float {
            self.bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
        } else {
            self.bytes.as_chunks::<2>().0.iter().map(|b| f32::from(i16::from_le_bytes(*b)) / 32768.0).collect()
        };
        Ok(Some(Chunk { pts_us, rate: fmt.rate, channels: fmt.channels, samples }))
    }

    fn seek(&mut self, us: u64) -> Result<(), String> {
        let mut at = PROPVARIANT::default();
        // SAFETY: la variante neuve reçoit un entier de 64 bits et son type
        // (`VT_I8`, rien à libérer) ; elle vit pendant l'appel, comme le
        // format de temps (GUID nul : unités de 100 ns).
        unsafe {
            let v = &mut *at.Anonymous.Anonymous;
            v.vt = VT_I8;
            v.Anonymous.hVal = i64::try_from(us).unwrap_or(i64::MAX / HNS_PER_US) * HNS_PER_US;
            self.reader.SetCurrentPosition(&GUID::zeroed(), &at)
        }
        .map_err(|e| format!("{:#010x}", e.code().0))
    }
}

#[cfg(test)]
#[path = "tests_reader.rs"]
mod tests;
