//! RTCP de la vidéo : NACK générique (RTPFB, FMT 1, RFC 4585 6.2.1), PLI
//! (PSFB, FMT 1, RFC 4585 6.3.1), FIR (PSFB, FMT 4, RFC 5104 4.3.1) et
//! rapport d'émetteur (SR, RFC 3550 6.4.1).
//!
//! Chiffrement de transport (docs.discord.food, Voice Connections : « RTCP
//! feedback packets keep the RTCP header clear as authenticated data and
//! encrypt the feedback body ») : l'en-tête en clair fait 8 octets (en-tête
//! commun et SSRC de l'expéditeur), le reste est chiffré, le compteur suit.

use super::super::crypt::{Cipher, NONCE_LEN, TAG_LEN};

/// En-tête RTCP laissé en clair (données authentifiées).
pub const AAD_LEN: usize = 8;
const SR: u8 = 200;
const RTPFB: u8 = 205;
const PSFB: u8 = 206;

/// Demande reçue d'un abonné.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Feedback {
    Nack { media: u32, seqs: Vec<u16> },
    Pli { media: u32 },
    /// SSRC dont une image complète est demandée.
    Fir { ssrcs: Vec<u32> },
}

fn packet(fmt: u8, pt: u8, sender: u32, body: &[u8]) -> Vec<u8> {
    let words = (4 + body.len()) / 4;
    let mut p = Vec::with_capacity(4 + 4 + body.len());
    p.push(0x80 | (fmt & 0x1F));
    p.push(pt);
    p.extend_from_slice(&(words as u16).to_be_bytes());
    p.extend_from_slice(&sender.to_be_bytes());
    p.extend_from_slice(body);
    p
}

/// NACK des numéros `seqs` (regroupés en PID + masque de 16 suivants).
pub fn nack(sender: u32, media: u32, seqs: &[u16]) -> Vec<u8> {
    let mut sorted = seqs.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let mut body = media.to_be_bytes().to_vec();
    let mut i = 0;
    while i < sorted.len() {
        let pid = sorted[i];
        let mut blp = 0u16;
        i += 1;
        while i < sorted.len() {
            let d = sorted[i].wrapping_sub(pid);
            if !(1..=16).contains(&d) {
                break;
            }
            blp |= 1 << (d - 1);
            i += 1;
        }
        body.extend_from_slice(&pid.to_be_bytes());
        body.extend_from_slice(&blp.to_be_bytes());
    }
    packet(1, RTPFB, sender, &body)
}

pub fn pli(sender: u32, media: u32) -> Vec<u8> {
    packet(1, PSFB, sender, &media.to_be_bytes())
}

/// FIR : la SSRC de média de l'en-tête vaut 0, la cible est dans le FCI.
pub fn fir(sender: u32, media: u32, seq_nr: u8) -> Vec<u8> {
    let mut body = vec![0u8; 4];
    body.extend_from_slice(&media.to_be_bytes());
    body.extend_from_slice(&[seq_nr, 0, 0, 0]);
    packet(4, PSFB, sender, &body)
}

/// Rapport d'émetteur sans bloc de réception.
pub fn sender_report(ssrc: u32, ntp: u64, rtp_ts: u32, packets: u32, octets: u32) -> Vec<u8> {
    let mut body = ntp.to_be_bytes().to_vec();
    body.extend_from_slice(&rtp_ts.to_be_bytes());
    body.extend_from_slice(&packets.to_be_bytes());
    body.extend_from_slice(&octets.to_be_bytes());
    packet(0, SR, ssrc, &body)
}

/// Demandes contenues dans un paquet RTCP composé (en clair). Les autres
/// types (rapports de réception…) sont ignorés.
pub fn parse(mut b: &[u8]) -> Vec<Feedback> {
    let mut out = Vec::new();
    while b.len() >= 4 {
        let fmt = b[0] & 0x1F;
        let pt = b[1];
        let len = 4 * (usize::from(u16::from_be_bytes([b[2], b[3]])) + 1);
        if b[0] >> 6 != 2 || len > b.len() {
            break;
        }
        let p = &b[..len];
        b = &b[len..];
        if p.len() < 12 {
            continue;
        }
        let media = u32::from_be_bytes([p[8], p[9], p[10], p[11]]);
        let fci = &p[12..];
        match (pt, fmt) {
            (RTPFB, 1) => {
                let mut seqs = Vec::new();
                for c in fci.as_chunks::<4>().0 {
                    let pid = u16::from_be_bytes([c[0], c[1]]);
                    let blp = u16::from_be_bytes([c[2], c[3]]);
                    seqs.push(pid);
                    seqs.extend((0..16).filter(|k| blp & (1 << k) != 0).map(|k| pid.wrapping_add(k + 1)));
                }
                out.push(Feedback::Nack { media, seqs });
            }
            (PSFB, 1) => out.push(Feedback::Pli { media }),
            (PSFB, 4) => out.push(Feedback::Fir { ssrcs: fci.as_chunks::<8>().0.iter().map(|c| u32::from_be_bytes([c[0], c[1], c[2], c[3]])).collect() }),
            _ => {}
        }
    }
    out
}

/// Paquet RTCP chiffré prêt à partir.
pub fn seal(cipher: &Cipher, plain: &[u8], counter: u32) -> Option<Vec<u8>> {
    if plain.len() < AAD_LEN {
        return None;
    }
    cipher.seal(&plain[..AAD_LEN], &plain[AAD_LEN..], counter)
}

/// RTCP reçu, en clair (en-tête compris). `None` : non authentifié.
pub fn open(cipher: &Cipher, pkt: &[u8]) -> Option<Vec<u8>> {
    if pkt.len() < AAD_LEN + TAG_LEN + NONCE_LEN {
        return None;
    }
    let end = pkt.len() - NONCE_LEN;
    let mut nonce = [0u8; NONCE_LEN];
    nonce.copy_from_slice(&pkt[end..]);
    let body = cipher.open(&pkt[..AAD_LEN], &pkt[AAD_LEN..end], nonce)?;
    let mut out = pkt[..AAD_LEN].to_vec();
    out.extend_from_slice(&body);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::voice::crypt::Mode;

    #[test]
    fn nack_regroupe_et_relu() {
        let p = nack(1, 2, &[100, 101, 116, 117, 65535, 3]);
        assert_eq!(p.len() % 4, 0);
        assert_eq!(p[1], RTPFB);
        let parsed = parse(&p);
        let [Feedback::Nack { media: 2, seqs }] = parsed.as_slice() else { panic!("NACK attendu") };
        let mut s = seqs.clone();
        s.sort_unstable();
        assert_eq!(s, vec![3, 100, 101, 116, 117, 65535]);
    }

    #[test]
    fn pli_fir_et_rapports_composes() {
        let mut compound = sender_report(9, 1 << 32, 90_000, 10, 1000);
        compound.extend(pli(1, 7));
        compound.extend(fir(1, 8, 3));
        // Rapport de réception (ignoré).
        compound.extend([0x81, 201, 0, 1, 0, 0, 0, 5]);
        assert_eq!(parse(&compound), vec![Feedback::Pli { media: 7 }, Feedback::Fir { ssrcs: vec![8] }]);
        assert!(parse(&[0x81, 205, 0, 50, 0]).is_empty(), "longueur mensongère");
    }

    #[test]
    fn chiffrement_aller_retour() {
        let c = Cipher::new(Mode::Aes256Gcm, [7; 32]).unwrap_or_else(|| panic!("clé"));
        let plain = pli(11, 22);
        let pkt = seal(&c, &plain, 5).unwrap_or_default();
        assert_eq!(&pkt[..8], &plain[..8], "en-tête en clair");
        assert_ne!(&pkt[8..12], &plain[8..12]);
        assert_eq!(open(&c, &pkt), Some(plain));
        let mut bad = pkt.clone();
        bad[1] ^= 1;
        assert_eq!(open(&c, &bad), None);
    }
}
