//! Source WebM du lecteur (lot VID) : fichier démultiplexé par `video::mkv`,
//! images VP8 ou VP9 décodées par Windows (« Extensions vidéo VP9 », voir
//! `codec/vp8.rs`), son Opus par libopus ou Vorbis par `symphonia`. Rien
//! n'est ajouté à l'exécutable pour l'image ; seul le décodeur Vorbis l'est.

use super::{Chunk, Info, OpenError, Source};
use crate::audio::engine::{VoiceDecoder, MAX_DECODED};
use crate::video::codec::{self, CodecError, Decoder};
use crate::video::mkv::{self, CodecId};
use crate::video::{Codec, EncodedFrame, Frame};
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CodecParameters, Decoder as _, DecoderOptions, CODEC_TYPE_VORBIS};
use symphonia::core::formats::Packet;
use symphonia::default::codecs::VorbisDecoder;

/// Images illisibles d'affilée tolérées (on attend la prochaine image clé).
const MAX_BAD: usize = 300;
/// Son décodé avant la position visée après un saut (Opus converge en 80 ms).
const PRE_ROLL_US: u64 = 80_000;

enum Sound {
    Opus(VoiceDecoder),
    Vorbis(Box<VorbisDecoder>, Option<SampleBuffer<f32>>),
}

pub struct WebmSource {
    data: Vec<u8>,
    file: mkv::File,
    info: Info,
    picture: Option<(Codec, Box<dyn Decoder>)>,
    sound: Option<Sound>,
    video_at: usize,
    audio_at: usize,
    buf: Vec<f32>,
}

/// Ident et réglages Vorbis à la suite, tirés du `CodecPrivate` de Matroska
/// (trois en-têtes en laçage Xiph : ident, commentaires, réglages).
fn vorbis_headers(p: &[u8]) -> Option<Vec<u8>> {
    if *p.first()? != 2 {
        return None;
    }
    let mut at = 1;
    let mut sizes = [0usize; 2];
    for s in &mut sizes {
        loop {
            let b = *p.get(at)?;
            at += 1;
            *s += usize::from(b);
            if b != 255 {
                break;
            }
        }
    }
    let ident = p.get(at..at + sizes[0])?;
    let setup = p.get(at + sizes[0] + sizes[1]..)?;
    Some([ident, setup].concat())
}

fn sound(t: &mkv::Track) -> Option<Sound> {
    match t.codec {
        // Une ou deux voies (famille 0) : libopus rend toujours de la stéréo.
        CodecId::Opus if t.channels <= 2 && t.private.get(18).is_none_or(|family| *family == 0) => VoiceDecoder::new().map(Sound::Opus),
        CodecId::Vorbis => {
            let mut params = CodecParameters::new();
            params.for_codec(CODEC_TYPE_VORBIS).with_extra_data(vorbis_headers(&t.private)?.into_boxed_slice());
            VorbisDecoder::try_new(&params, &DecoderOptions::default()).ok().map(|d| Sound::Vorbis(Box::new(d), None))
        }
        _ => None,
    }
}

impl WebmSource {
    /// Ouvre un fichier WebM. Sur le fil du lecteur : le décodeur de
    /// Windows lui reste attaché.
    pub fn open(data: Vec<u8>) -> Result<Self, OpenError> {
        // Fichier venu du réseau : une panique de l'analyseur devient un simple refus.
        let mut file = crate::panics::contain(|| mkv::parse(&data)).flatten().ok_or(OpenError::Unsupported)?;
        let picture = match &file.video {
            Some(t) if !file.video_blocks.is_empty() => {
                let c = if t.codec == CodecId::Vp9 { Codec::Vp9 } else { Codec::Vp8 };
                Some((c, codec::file_decoder(c).ok_or(OpenError::NoDecoder)?))
            }
            _ => None,
        };
        let sound = file.audio.as_ref().filter(|_| !file.audio_blocks.is_empty()).and_then(sound);
        if picture.is_none() && sound.is_none() {
            return Err(OpenError::Unsupported);
        }
        if picture.is_none() {
            file.video = None;
        }
        let size = file.video.as_ref().map(|t| (t.width, t.height)).filter(|(w, h)| *w > 0 && *h > 0);
        let info = Info { duration_us: file.duration_us, video: picture.as_ref().map(|_| size.unwrap_or((16, 9))), rotation: 0, audio: sound.is_some() };
        Ok(WebmSource { data, file, info, picture, sound, video_at: 0, audio_at: 0, buf: vec![0.0; MAX_DECODED * 2] })
    }

}

impl Source for WebmSource {
    fn info(&self) -> Info {
        self.info
    }

    fn video(&mut self) -> Result<Option<Frame>, String> {
        let mut bad = 0;
        while let Some(&b) = self.file.video_blocks.get(self.video_at) {
            self.video_at += 1;
            let Some((c, dec)) = self.picture.as_mut() else { return Ok(None) };
            let data = self.data.get(b.start..b.start + b.len).unwrap_or(&[]).to_vec();
            let f = EncodedFrame { codec: *c, keyframe: b.key, ts90k: (b.pts_us * 9 / 100) as u32, data };
            match dec.decode(&f) {
                Ok(Some(mut frame)) => {
                    frame.pts_us = b.pts_us;
                    return Ok(Some(frame));
                }
                // Image cachée (référence), ou attente d'une image clé.
                Ok(None) => {}
                Err(CodecError::Corrupt(_)) if bad < MAX_BAD => bad += 1,
                Err(e) => return Err(e.to_string()),
            }
        }
        Ok(None)
    }

    fn audio(&mut self) -> Result<Option<Chunk>, String> {
        let delay = self.file.audio.as_ref().map_or(0, |t| t.delay_us);
        while let Some(&b) = self.file.audio_blocks.get(self.audio_at) {
            self.audio_at += 1;
            let (rate, channels, mut samples) = match self.sound.as_mut() {
                None => return Ok(None),
                Some(Sound::Opus(dec)) => {
                    let n = dec.decode(self.data.get(b.start..b.start + b.len).unwrap_or(&[]), &mut self.buf);
                    (super::RATE, 2, self.buf[..n * 2].to_vec())
                }
                Some(Sound::Vorbis(dec, buf)) => {
                    let packet = Packet::new_from_slice(0, 0, 0, self.data.get(b.start..b.start + b.len).unwrap_or(&[]));
                    let Ok(audio) = dec.decode(&packet) else { continue };
                    let spec = *audio.spec();
                    let ch = spec.channels.count();
                    let out = match buf {
                        Some(o) if o.capacity() >= audio.capacity() * ch => o,
                        _ => buf.insert(SampleBuffer::new(audio.capacity() as u64, spec)),
                    };
                    out.copy_interleaved_ref(audio);
                    (spec.rate, ch, out.samples().to_vec())
                }
            };
            if samples.is_empty() || channels == 0 {
                continue;
            }
            // Le début du flux Opus (`CodecDelay`) n'est pas à entendre.
            let mut pts_us = b.pts_us;
            if b.pts_us < delay {
                let cut = ((delay - b.pts_us) * u64::from(rate) / 1_000_000) as usize * channels;
                if cut >= samples.len() {
                    continue;
                }
                samples.drain(..cut);
                pts_us = 0;
            } else {
                pts_us -= delay;
            }
            return Ok(Some(Chunk { pts_us, rate, channels, samples }));
        }
        Ok(None)
    }

    fn seek(&mut self, us: u64) -> Result<(), String> {
        // Vidéo : l'image clé qui précède ; le lecteur décode jusqu'à la position.
        let v = &self.file.video_blocks;
        let upto = v.partition_point(|b| b.pts_us <= us);
        self.video_at = v[..upto].iter().rposition(|b| b.key).unwrap_or(0);
        let from = us.saturating_sub(PRE_ROLL_US);
        self.audio_at = self.file.audio_blocks.partition_point(|b| b.pts_us < from);
        match self.sound.as_mut() {
            Some(Sound::Opus(d)) => {
                if let Some(fresh) = VoiceDecoder::new() {
                    *d = fresh;
                }
            }
            Some(Sound::Vorbis(d, _)) => d.reset(),
            None => {}
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "webm_tests.rs"]
mod tests;
