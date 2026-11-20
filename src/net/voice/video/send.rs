//! Émission d'un flux vidéo : paquets RTP d'une image, historique borné
//! pour répondre aux NACK par RTX (RFC 4588 : même horodatage, numéro de
//! séquence d'origine en tête de la charge, SSRC et type de charge RTX),
//! rapport d'émetteur toutes les 5 s (docs.discord.food, Voice Connections).

use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::super::rtp;
use super::{h264, rtcp, vp8};
use crate::video::Codec;

/// Charge utile RTP la plus grande : un paquet chiffré tient dans 1200
/// octets (12 d'en-tête, 16 d'étiquette, 4 de compteur).
pub const MAX_PAYLOAD: usize = 1100;
/// Paquets gardés pour les retransmissions : environ une seconde à 4 Mbit/s.
pub const HISTORY: usize = 512;
const SR_EVERY: Duration = Duration::from_secs(5);

/// Paquet à chiffrer puis envoyer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RtpOut {
    pub header: [u8; rtp::HEADER_LEN],
    pub payload: Vec<u8>,
}

struct Sent {
    seq: u16,
    ts: u32,
    marker: bool,
    payload: Vec<u8>,
}

pub struct Sender {
    pub ssrc: u32,
    pub rtx_ssrc: u32,
    pub payload_type: u8,
    pub rtx_payload_type: u8,
    pub codec: Codec,
    seq: u16,
    rtx_seq: u16,
    history: VecDeque<Sent>,
    packets: u32,
    octets: u32,
    last_ts: u32,
    last_sr: Option<Instant>,
}

impl Sender {
    pub fn new(codec: Codec, ssrc: u32, rtx_ssrc: u32, payload_type: u8, rtx_payload_type: u8) -> Self {
        Sender {
            ssrc,
            rtx_ssrc,
            payload_type,
            rtx_payload_type,
            codec,
            seq: 0,
            rtx_seq: 0,
            history: VecDeque::with_capacity(HISTORY),
            packets: 0,
            octets: 0,
            last_ts: 0,
            last_sr: None,
        }
    }

    /// Paquets RTP d'une image (déjà chiffrée de bout en bout le cas échéant) ;
    /// marqueur sur le dernier. `None` : codec sans paquétiseur.
    pub fn frame(&mut self, ts90k: u32, data: &[u8]) -> Option<Vec<RtpOut>> {
        let payloads = match self.codec {
            Codec::H264 => h264::packetize_plain(data, MAX_PAYLOAD),
            Codec::Vp8 => vp8::packetize(data, MAX_PAYLOAD),
            Codec::Vp9 | Codec::Av1 => return None,
        };
        let n = payloads.len();
        let mut out = Vec::with_capacity(n);
        for (k, payload) in payloads.into_iter().enumerate() {
            let marker = k + 1 == n;
            let seq = self.seq;
            self.seq = self.seq.wrapping_add(1);
            out.push(RtpOut { header: rtp::header_with(self.payload_type, marker, seq, ts90k, self.ssrc), payload: payload.clone() });
            self.packets = self.packets.wrapping_add(1);
            self.octets = self.octets.wrapping_add(payload.len() as u32);
            if self.history.len() >= HISTORY {
                self.history.pop_front();
            }
            self.history.push_back(Sent { seq, ts: ts90k, marker, payload });
        }
        self.last_ts = ts90k;
        Some(out)
    }

    /// Retransmissions RTX des paquets demandés encore dans l'historique.
    pub fn retransmit(&mut self, seqs: &[u16]) -> Vec<RtpOut> {
        let mut out = Vec::new();
        for &s in seqs {
            let Some(p) = self.history.iter().find(|p| p.seq == s) else { continue };
            let mut payload = Vec::with_capacity(2 + p.payload.len());
            payload.extend_from_slice(&p.seq.to_be_bytes());
            payload.extend_from_slice(&p.payload);
            let seq = self.rtx_seq;
            self.rtx_seq = self.rtx_seq.wrapping_add(1);
            out.push(RtpOut { header: rtp::header_with(self.rtx_payload_type, p.marker, seq, p.ts, self.rtx_ssrc), payload });
        }
        out
    }

    /// Rapport d'émetteur s'il est dû (envoyé avec les images, sans minuterie).
    pub fn sender_report(&mut self, now: Instant) -> Option<Vec<u8>> {
        if self.last_sr.is_some_and(|t| now.saturating_duration_since(t) < SR_EVERY) {
            return None;
        }
        self.last_sr = Some(now);
        Some(rtcp::sender_report(self.ssrc, ntp_now(), self.last_ts, self.packets, self.octets))
    }
}

/// Heure NTP (secondes depuis 1900 en virgule fixe 32.32).
fn ntp_now() -> u64 {
    let d = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = d.as_secs() + 2_208_988_800;
    let frac = (u64::from(d.subsec_nanos()) << 32) / 1_000_000_000;
    secs << 32 | frac
}

/// RTX reçu : numéro de séquence d'origine et charge d'origine.
pub fn unwrap_rtx(payload: &[u8]) -> Option<(u16, &[u8])> {
    let osn = u16::from_be_bytes([*payload.first()?, *payload.get(1)?]);
    Some((osn, &payload[2..]))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paquets_marqueur_et_rtx() {
        let mut s = Sender::new(Codec::Vp8, 10, 11, 105, 106);
        let frame = vec![0u8; 3000];
        let p = s.frame(90_000, &frame).unwrap_or_default();
        assert_eq!(p.len(), 3);
        assert!(p.iter().all(|p| p.header[8..12] == 10u32.to_be_bytes()));
        assert_eq!(p.iter().map(|p| p.header[1] & 0x80 != 0).collect::<Vec<_>>(), vec![false, false, true]);
        assert_eq!(p[0].header[1] & 0x7F, 105);
        let r = s.retransmit(&[2, 40]);
        assert_eq!(r.len(), 1, "40 n'a jamais été envoyé");
        assert_eq!(r[0].header[1], 0x80 | 106, "RTX garde le marqueur");
        assert_eq!(r[0].header[4..8], 90_000u32.to_be_bytes());
        assert_eq!(r[0].header[8..12], 11u32.to_be_bytes());
        assert_eq!(unwrap_rtx(&r[0].payload), Some((2, p[2].payload.as_slice())));
    }

    #[test]
    fn historique_borne() {
        let mut s = Sender::new(Codec::Vp8, 1, 2, 105, 106);
        for i in 0..HISTORY as u32 + 10 {
            s.frame(i * 3000, &[0x10]);
        }
        assert!(s.retransmit(&[0]).is_empty(), "premier paquet oublié");
        assert_eq!(s.retransmit(&[(HISTORY + 9) as u16]).len(), 1);
    }

    #[test]
    fn rapport_toutes_les_5_s() {
        let mut s = Sender::new(Codec::H264, 1, 2, 103, 104);
        let t = Instant::now();
        assert!(s.sender_report(t).is_some());
        assert!(s.sender_report(t + Duration::from_secs(1)).is_none());
        assert!(s.sender_report(t + Duration::from_secs(6)).is_some());
    }
}
