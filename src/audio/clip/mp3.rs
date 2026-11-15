//! MP3 (format des sons du tableau de Discord) par `symphonia`, réduit au
//! seul décodeur MP3.

use super::ClipError;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::DecoderOptions;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

/// Échantillons entrelacés, fréquence, voies (1 ou 2 ; au-delà, les deux premières).
pub struct Decoded {
    pub samples: Vec<f32>,
    pub rate: u32,
    pub channels: usize,
}

pub fn decode(data: Vec<u8>, max_frames: usize) -> Result<Decoded, ClipError> {
    let mss = MediaSourceStream::new(Box::new(std::io::Cursor::new(data)), Default::default());
    let mut hint = Hint::new();
    hint.with_extension("mp3");
    let probed = symphonia::default::get_probe().format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default()).map_err(|_| ClipError::Format)?;
    let mut format = probed.format;
    let track = format.default_track().ok_or(ClipError::Format)?;
    let track_id = track.id;
    let mut decoder = symphonia::default::get_codecs().make(&track.codec_params, &DecoderOptions::default()).map_err(|_| ClipError::Unsupported)?;
    let mut out = Decoded { samples: Vec::new(), rate: 0, channels: 0 };
    let mut buf: Option<SampleBuffer<f32>> = None;
    // Fin du fichier ou paquet illisible : on garde ce qui est décodé.
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() != track_id {
            continue;
        }
        let Ok(audio) = decoder.decode(&packet) else { continue };
        let spec = *audio.spec();
        let ch = spec.channels.count();
        if ch == 0 {
            continue;
        }
        if out.rate == 0 {
            out.rate = spec.rate;
            out.channels = ch.min(2);
        }
        // Format changé en cours de fichier : ces trames sont sautées.
        if ch < out.channels || spec.rate != out.rate {
            continue;
        }
        let b = match &mut buf {
            Some(b) if b.capacity() >= audio.capacity() * ch => b,
            _ => buf.insert(SampleBuffer::new(audio.capacity() as u64, spec)),
        };
        b.copy_interleaved_ref(audio);
        for f in b.samples().chunks_exact(ch) {
            out.samples.extend_from_slice(&f[..out.channels]);
        }
        if out.samples.len() / out.channels.max(1) >= max_frames {
            out.samples.truncate(max_frames * out.channels);
            break;
        }
    }
    if out.samples.is_empty() || out.rate == 0 {
        return Err(ClipError::Format);
    }
    Ok(out)
}
