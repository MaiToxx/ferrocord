//! Codec Opus (libopus liée statiquement, par la crate `opus`).

use opus::{Application, Bitrate, Channels, Decoder, Encoder, Signal};

pub const SAMPLE_RATE: u32 = 48_000;
/// Échantillons par voie dans une trame de 20 ms.
pub const FRAME: usize = 960;
/// Taille maximale d'une trame Opus unique (RFC 6716, 3.4).
pub const MAX_PACKET: usize = 1275;
/// Échantillons par voie d'un paquet de 120 ms, le plus long qu'Opus permet.
pub const MAX_DECODED: usize = 5760;
/// Trame de silence que Discord demande d'envoyer à chaque arrêt de l'émission,
/// pour que les autres clients n'interpolent pas la dernière trame.
pub const SILENCE: [u8; 3] = [0xF8, 0xFF, 0xFE];

pub struct VoiceEncoder {
    enc: Encoder,
    buf: [u8; MAX_PACKET],
}

impl VoiceEncoder {
    pub fn new() -> Option<Self> {
        let mut enc = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Voip).ok()?;
        // Un réglage refusé laisse la valeur par défaut de libopus : on encode quand même.
        let _ = enc.set_bitrate(Bitrate::Bits(64_000));
        let _ = enc.set_signal(Signal::Voice);
        let _ = enc.set_inband_fec(true);
        // libopus n'ajoute la FEC que si une perte est annoncée.
        let _ = enc.set_packet_loss_perc(10);
        Some(Self { enc, buf: [0; MAX_PACKET] })
    }

    pub fn set_dtx(&mut self, on: bool) {
        let _ = self.enc.set_dtx(on);
    }

    /// Encode une trame mono de 960 échantillons.
    pub fn encode(&mut self, mono: &[f32]) -> Option<Vec<u8>> {
        let n = self.enc.encode_float(mono, &mut self.buf).ok()?;
        Some(self.buf[..n].to_vec())
    }
}

/// Décodeur stéréo d'une personne. Chaque méthode écrit dans `out` (stéréo
/// entrelacé) et rend le nombre d'échantillons par voie, 0 en cas d'erreur.
pub struct VoiceDecoder {
    dec: Decoder,
}

impl VoiceDecoder {
    pub fn new() -> Option<Self> {
        Some(Self { dec: Decoder::new(SAMPLE_RATE, Channels::Stereo).ok()? })
    }

    pub fn decode(&mut self, packet: &[u8], out: &mut [f32]) -> usize {
        self.dec.decode_float(packet, out, false).unwrap_or(0)
    }

    /// Masquage d'une trame perdue (PLC).
    pub fn conceal(&mut self, out: &mut [f32]) -> usize {
        let n = out.len().min(FRAME * 2);
        self.dec.decode_float(&[], &mut out[..n], false).unwrap_or(0)
    }

    /// Reconstruit la trame perdue à partir de la FEC portée par la suivante.
    pub fn recover(&mut self, next: &[u8], out: &mut [f32]) -> usize {
        let n = out.len().min(FRAME * 2);
        match self.dec.decode_float(next, &mut out[..n], true) {
            Ok(n) if n > 0 => n,
            _ => self.conceal(out),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::engine::tests::{best_corr, sine};

    #[test]
    fn aller_retour_sinus_correle() {
        let mut enc = VoiceEncoder::new().unwrap();
        let mut dec = VoiceDecoder::new().unwrap();
        let src = sine(440.0, 0.5, 0, FRAME * 50);
        let mut left = Vec::new();
        let mut out = vec![0.0; MAX_DECODED * 2];
        for chunk in src.chunks(FRAME) {
            let p = enc.encode(chunk).unwrap();
            assert!(p.len() <= MAX_PACKET);
            let n = dec.decode(&p, &mut out);
            assert_eq!(n, FRAME);
            // Mono décodé en stéréo : les deux voies sont égales.
            left.extend(out[..n * 2].chunks(2).map(|s| {
                assert!((s[0] - s[1]).abs() < 1e-6);
                s[0]
            }));
        }
        // On ignore la mise en route de l'encodeur ; décalage du codec < 1 trame.
        let c = best_corr(&src[FRAME * 10..FRAME * 40], &left[FRAME * 10..], FRAME);
        assert!(c > 0.95, "corrélation {c}");
    }

    #[test]
    fn plc_et_fec_rendent_une_trame() {
        let mut enc = VoiceEncoder::new().unwrap();
        let mut dec = VoiceDecoder::new().unwrap();
        let mut out = vec![0.0; MAX_DECODED * 2];
        let frames: Vec<Vec<u8>> =
            (0..10).map(|i| enc.encode(&sine(300.0, 0.4, i * FRAME, FRAME)).unwrap()).collect();
        for p in &frames[..5] {
            dec.decode(p, &mut out);
        }
        assert_eq!(dec.recover(&frames[6], &mut out), FRAME);
        assert_eq!(dec.decode(&frames[6], &mut out), FRAME);
        assert_eq!(dec.conceal(&mut out), FRAME);
        // Paquet invalide : erreur absorbée.
        assert_eq!(dec.decode(&[0xFF; 2], &mut out), 0);
    }
}
