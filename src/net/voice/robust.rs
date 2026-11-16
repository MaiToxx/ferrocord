//! Lot « fiable » : tout ce qui arrive du réseau en binaire (UDP du serveur
//! vocal, trames DAVE) passe par des analyseurs qui découpent des tranches.
//! Aucun ne doit paniquer, quoi qu'il reçoive : paquets vides, tronqués à
//! chaque longueur, bruit, paquets valides abîmés octet par octet.
//!
//! Générateur déterministe (xorshift) : un échec se rejoue à l'identique.

use super::{rtp, video::{h264, rtcp, vp8}};

pub(super) struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    pub fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| (self.next() >> 24) as u8).collect()
    }
}

/// Paquets hostiles dérivés de `seeds` : chaque préfixe, chaque octet
/// remplacé par 0x00 et 0xFF, puis du bruit de toutes les petites longueurs.
pub(super) fn hostile(seeds: &[Vec<u8>]) -> Vec<Vec<u8>> {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut out = vec![Vec::new()];
    for s in seeds {
        out.extend((0..=s.len()).map(|n| s[..n].to_vec()));
        for i in 0..s.len() {
            for v in [0x00, 0xFF, s[i] ^ 0x80] {
                let mut m = s.clone();
                m[i] = v;
                out.push(m);
            }
        }
    }
    for len in 0..96 {
        for _ in 0..24 {
            out.push(rng.bytes(len));
        }
    }
    // Longueurs annoncées énormes, en-têtes qui se disent plus longs que le paquet.
    for len in [4usize, 12, 13, 20, 74, 200, 1500] {
        out.push(vec![0xFF; len]);
        out.push(vec![0x00; len]);
        let mut p = vec![0x9F; len];
        p[0] = 0x9F;
        out.push(p);
    }
    out
}

#[test]
fn les_analyseurs_udp_ne_paniquent_jamais() {
    let mut rtp_ok = rtp::header(7, 960, 42).to_vec();
    rtp_ok.extend_from_slice(&[0xAB; 40]);
    // Avec extension et CSRC : les longueurs lues dans le paquet.
    let mut rtp_ext = rtp_ok.clone();
    rtp_ext[0] = 0x80 | 0x10 | 0x03;
    let rtcp_nack = rtcp::nack(1, 2, &[10, 11, 12, 40]);
    let rtcp_fir = rtcp::fir(1, 2, 3);
    let discovery = rtp::discovery_reply(42, "203.0.113.7", 50_000).to_vec();
    let h264_frame = [&[0u8, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 1, 0x65][..], &[0x11; 3000][..]].concat();
    let mut seeds = vec![rtp_ok, rtp_ext, rtcp_nack, rtcp_fir, discovery];
    seeds.extend(h264::packetize(&h264_frame, 1100));
    seeds.extend(vp8::packetize(&[0x10; 2500], 1100));
    let cases = hostile(&seeds);
    assert!(cases.len() > 5000, "{} cas seulement", cases.len());
    for p in &cases {
        let _ = rtp::is_rtcp(p);
        let _ = rtp::parse(p);
        if let Some(pkt) = rtp::parse_any(p) {
            // Les tranches rendues tiennent dans le paquet.
            assert!(pkt.aad.len() + pkt.body.len() <= p.len());
        }
        let _ = rtp::discovery_response(p);
        let _ = rtp::discovery_request_ssrc(p);
        let _ = rtcp::parse(p);
        let _ = h264::nal_units(p);
        let _ = h264::is_keyframe(p);
        let _ = h264::depacketize([p.as_slice()]);
        let _ = vp8::depacketize([p.as_slice()]);
        let _ = vp8::is_keyframe(p);
    }
    // Suites de charges utiles : fragments dans le désordre, mélangés.
    for w in cases.windows(3).step_by(7) {
        let _ = h264::depacketize(w.iter().map(Vec::as_slice));
        let _ = vp8::depacketize(w.iter().map(Vec::as_slice));
    }
}
