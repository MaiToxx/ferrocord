//! Paquets UDP de la connexion vocale : en-tête RTP (RFC 3550), paquets
//! RTCP à ignorer, découverte d'adresse (documentation : IP Discovery).

use super::crypt::{Cipher, NONCE_LEN, TAG_LEN};

pub const HEADER_LEN: usize = 12;
/// Type de charge utile de l'audio Opus chez Discord.
pub const PAYLOAD_TYPE: u8 = 0x78;
/// Une trame Opus de 20 ms à 48 kHz : pas de l'horodatage.
pub const FRAME_SAMPLES: u32 = 960;
/// Trame Opus de silence, envoyée cinq fois quand on cesse de parler (évite
/// l'interpolation du décodeur d'en face).
pub const SILENCE: [u8; 3] = [0xF8, 0xFF, 0xFE];

/// En-tête RTP minimal : version 2, sans CSRC ni extension.
pub fn header(seq: u16, timestamp: u32, ssrc: u32) -> [u8; HEADER_LEN] {
    let mut h = [0u8; HEADER_LEN];
    h[0] = 0x80;
    h[1] = PAYLOAD_TYPE;
    h[2..4].copy_from_slice(&seq.to_be_bytes());
    h[4..8].copy_from_slice(&timestamp.to_be_bytes());
    h[8..12].copy_from_slice(&ssrc.to_be_bytes());
    h
}

/// En-tête RTP d'un flux vidéo ou RTX : type de charge et marqueur au choix.
pub fn header_with(payload_type: u8, marker: bool, seq: u16, timestamp: u32, ssrc: u32) -> [u8; HEADER_LEN] {
    let mut h = header(seq, timestamp, ssrc);
    h[1] = payload_type & 0x7F | if marker { 0x80 } else { 0 };
    h
}

/// Les types RTCP (200 à 204…) occupent tout le deuxième octet, marqueur
/// compris (RFC 5761, démultiplexage RTP/RTCP).
pub fn is_rtcp(p: &[u8]) -> bool {
    p.len() >= 2 && (192..=223).contains(&p[1])
}

/// Paquet audio reçu, découpé mais encore chiffré.
#[derive(Debug, PartialEq, Eq)]
pub struct Packet<'a> {
    pub seq: u16,
    pub timestamp: u32,
    pub ssrc: u32,
    pub payload_type: u8,
    /// Dernier paquet d'une image (vidéo).
    pub marker: bool,
    /// En-tête non chiffré, données authentifiées.
    pub aad: &'a [u8],
    /// Chiffré ‖ étiquette.
    pub body: &'a [u8],
    pub nonce: [u8; NONCE_LEN],
    /// Corps de l'extension d'en-tête (chiffré avec la charge utile), à sauter après déchiffrement.
    ext_len: usize,
    padded: bool,
}

/// `None` : pas un paquet audio Opus RTP (RTCP, vidéo, trop court, autre version).
pub fn parse(p: &[u8]) -> Option<Packet<'_>> {
    parse_any(p).filter(|p| p.payload_type == PAYLOAD_TYPE)
}

/// Paquet RTP de n'importe quel type de charge (audio, vidéo, RTX).
pub fn parse_any(p: &[u8]) -> Option<Packet<'_>> {
    if p.len() < HEADER_LEN || p[0] >> 6 != 2 || is_rtcp(p) {
        return None;
    }
    let csrc = usize::from(p[0] & 0x0F);
    let ext = p[0] & 0x10 != 0;
    let mut aad_len = HEADER_LEN + 4 * csrc;
    let mut ext_len = 0;
    if ext {
        // Seul le préambule (profil, longueur en mots de 32 bits) est en clair.
        let pre = p.get(aad_len..aad_len + 4)?;
        ext_len = 4 * usize::from(u16::from_be_bytes([pre[2], pre[3]]));
        aad_len += 4;
    }
    if p.len() < aad_len + TAG_LEN + NONCE_LEN {
        return None;
    }
    let end = p.len() - NONCE_LEN;
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&p[end..]);
    Some(Packet {
        seq: u16::from_be_bytes([p[2], p[3]]),
        timestamp: u32::from_be_bytes([p[4], p[5], p[6], p[7]]),
        ssrc: u32::from_be_bytes([p[8], p[9], p[10], p[11]]),
        payload_type: p[1] & 0x7F,
        marker: p[1] & 0x80 != 0,
        aad: &p[..aad_len],
        body: &p[aad_len..end],
        nonce,
        ext_len,
        padded: p[0] & 0x20 != 0,
    })
}

impl Packet<'_> {
    /// Trame Opus : déchiffrée, sans le corps de l'extension ni le bourrage.
    pub fn open(&self, cipher: &Cipher) -> Option<Vec<u8>> {
        let mut plain = cipher.open(self.aad, self.body, self.nonce)?;
        if self.padded {
            let pad = usize::from(*plain.last()?);
            if pad == 0 || pad > plain.len() {
                return None;
            }
            plain.truncate(plain.len() - pad);
        }
        if self.ext_len > plain.len() {
            return None;
        }
        plain.drain(..self.ext_len);
        Some(plain)
    }
}

/// Longueur d'un paquet de découverte d'adresse (type, longueur, SSRC, adresse sur 64 octets, port).
pub const DISCOVERY_LEN: usize = 74;

pub fn discovery_request(ssrc: u32) -> [u8; DISCOVERY_LEN] {
    discovery(1, ssrc, "", 0)
}

/// Réponse du serveur (sert aussi au faux serveur des tests).
pub fn discovery_reply(ssrc: u32, address: &str, port: u16) -> [u8; DISCOVERY_LEN] {
    discovery(2, ssrc, address, port)
}

fn discovery(kind: u16, ssrc: u32, address: &str, port: u16) -> [u8; DISCOVERY_LEN] {
    let mut p = [0u8; DISCOVERY_LEN];
    p[0..2].copy_from_slice(&kind.to_be_bytes());
    p[2..4].copy_from_slice(&70u16.to_be_bytes());
    p[4..8].copy_from_slice(&ssrc.to_be_bytes());
    let a = address.as_bytes();
    let n = a.len().min(63);
    p[8..8 + n].copy_from_slice(&a[..n]);
    p[72..74].copy_from_slice(&port.to_be_bytes());
    p
}

/// Adresse et port publics lus dans une réponse de découverte.
pub fn discovery_response(p: &[u8]) -> Option<(String, u16)> {
    if p.len() != DISCOVERY_LEN || p[0..2] != [0, 2] {
        return None;
    }
    let raw = &p[8..72];
    let end = raw.iter().position(|&b| b == 0).unwrap_or(raw.len());
    let address = std::str::from_utf8(&raw[..end]).ok()?.to_string();
    if address.is_empty() {
        return None;
    }
    Some((address, u16::from_be_bytes([p[72], p[73]])))
}

/// Requête de découverte reçue par un serveur (tests) : son SSRC.
pub fn discovery_request_ssrc(p: &[u8]) -> Option<u32> {
    (p.len() == DISCOVERY_LEN && p[0..2] == [0, 1]).then(|| u32::from_be_bytes([p[4], p[5], p[6], p[7]]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::voice::crypt::Mode;

    #[test]
    fn header_layout() {
        assert_eq!(header(0x0102, 0x0304_0506, 0x0708_090A), [0x80, 0x78, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    #[test]
    fn discovery_round_trip() {
        let req = discovery_request(42);
        assert_eq!(req.len(), 74);
        assert_eq!(&req[..8], &[0, 1, 0, 70, 0, 0, 0, 42]);
        assert_eq!(discovery_request_ssrc(&req), Some(42));
        assert_eq!(discovery_response(&req), None);
        let rep = discovery_reply(42, "203.0.113.7", 50004);
        assert_eq!(discovery_response(&rep), Some(("203.0.113.7".to_string(), 50004)));
        assert_eq!(discovery_response(&rep[..73]), None);
    }

    #[test]
    fn rtcp_and_foreign_packets_are_skipped() {
        let mut rr = [0u8; 32];
        rr[0] = 0x81;
        rr[1] = 201;
        assert!(is_rtcp(&rr));
        assert_eq!(parse(&rr), None);
        let mut video = header(1, 2, 3).to_vec();
        video[1] = 0x65;
        video.extend_from_slice(&[0; 24]);
        assert_eq!(parse(&video), None);
        assert_eq!(parse(&[0x80, 0x78, 0]), None);
    }

    /// Paquet avec CSRC, extension et bourrage, comme en enverrait un autre client.
    #[test]
    fn extension_csrc_and_padding_are_removed() {
        let c = Cipher::new(Mode::Aes256Gcm, [3; 32]).unwrap_or_else(|| panic!("clé"));
        let mut h = header(9, 960, 77).to_vec();
        h[0] |= 0x10 | 0x20 | 1; // extension, bourrage, un CSRC
        h.extend_from_slice(&[0, 0, 0, 5]); // CSRC
        h.extend_from_slice(&[0xBE, 0xDE, 0, 2]); // préambule : 2 mots
        let mut payload = vec![0x11; 8]; // corps de l'extension
        payload.extend_from_slice(b"opus!");
        payload.extend_from_slice(&[0, 0, 3]); // bourrage de 3
        let pkt = c.seal(&h, &payload, 5).unwrap_or_default();
        let p = parse(&pkt).unwrap_or_else(|| panic!("paquet lisible"));
        assert_eq!((p.seq, p.timestamp, p.ssrc), (9, 960, 77));
        assert_eq!(p.aad.len(), 12 + 4 + 4);
        assert_eq!(p.open(&c).as_deref(), Some(&b"opus!"[..]));
    }
}
