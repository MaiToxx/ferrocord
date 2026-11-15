//! Plages laissées en clair dans une image vidéo chiffrée par DAVE
//! (protocol.md, « Codec Handling » ; détail repris de libdave,
//! `cpp/src/codec_utils.cpp`) :
//! - H.264 : chaque NAL est précédée d'un code de début de 4 octets (en
//!   clair) ; une NAL non VCL reste entière en clair ; une tranche (types 1
//!   et 5) garde en clair son octet d'en-tête et les octets qui couvrent
//!   `first_mb_in_slice`, `slice_type` et `pic_parameter_set_id` ;
//! - VP8 : 10 octets en clair pour une image clé, 1 sinon ;
//! - VP9 : tout chiffré. AV1 n'est pas pris en charge (image non envoyée).

use crate::net::voice::video::h264;
use crate::video::Codec;

/// Image à chiffrer (normalisée pour H.264), plages en clair, contrôle des codes de début.
pub type Layout = (Vec<u8>, Vec<(usize, usize)>, bool);

pub fn layout(codec: Codec, frame: &[u8]) -> Option<Layout> {
    match codec {
        Codec::H264 => h264_layout(frame),
        Codec::Vp8 => {
            let n = if frame.first()? & 1 == 0 { 10 } else { 1 };
            (frame.len() >= n).then(|| (frame.to_vec(), vec![(0, n)], false))
        }
        Codec::Vp9 => Some((frame.to_vec(), Vec::new(), false)),
        Codec::Av1 => None,
    }
}

fn h264_layout(frame: &[u8]) -> Option<Layout> {
    // libdave refuse une image qui ne commence pas par un code de début.
    if frame.iter().position(|&b| b != 0).is_none_or(|i| frame.get(i) != Some(&1) || i < 2) {
        return None;
    }
    let mut out = Vec::with_capacity(frame.len() + 16);
    let mut clear: Vec<(usize, usize)> = Vec::new();
    for nal in h264::nal_units(frame) {
        let at = out.len();
        out.extend_from_slice(&h264::START_CODE);
        out.extend_from_slice(nal);
        let keep = match h264::nal_type(nal) {
            h264::NAL_SLICE | h264::NAL_IDR => 1 + bytes_covering_pps(&nal[1..])?,
            _ => nal.len(),
        };
        if keep > nal.len() {
            return None;
        }
        let range = (at, 4 + keep);
        match clear.last_mut() {
            Some(last) if last.0 + last.1 == at => last.1 += range.1,
            _ => clear.push(range),
        }
    }
    Some((out, clear, true))
}

/// Octets bruts (octets de prévention d'émulation compris) qui couvrent
/// les trois premiers Exp-Golomb d'un en-tête de tranche.
fn bytes_covering_pps(payload: &[u8]) -> Option<usize> {
    // RBSP des premiers octets et position brute de chacun.
    let mut rbsp = Vec::new();
    let mut raw_end = Vec::new();
    let mut zeros = 0;
    for (i, &b) in payload.iter().enumerate().take(64) {
        if zeros >= 2 && b == 3 {
            zeros = 0;
            continue;
        }
        zeros = if b == 0 { zeros + 1 } else { 0 };
        rbsp.push(b);
        raw_end.push(i + 1);
    }
    let mut bit = 0usize;
    let read_bit = |bit: &mut usize| -> Option<u8> {
        let b = rbsp.get(*bit / 8)? >> (7 - *bit % 8) & 1;
        *bit += 1;
        Some(b)
    };
    for _ in 0..3 {
        let mut lz = 0;
        while read_bit(&mut bit)? == 0 {
            lz += 1;
            if lz > 31 {
                return None;
            }
        }
        for _ in 0..lz {
            read_bit(&mut bit)?;
        }
    }
    raw_end.get((bit - 1) / 8).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exp_golomb_et_prevention_d_emulation() {
        // ue = 0 (1), ue = 0 (1), ue = 0 (1) : trois bits, un octet.
        assert_eq!(bytes_covering_pps(&[0b1110_0000, 0xFF]), Some(1));
        // first_mb = 3 (00100), slice_type = 7 (0001000), pps = 0 (1) : 13 bits, deux octets.
        assert_eq!(bytes_covering_pps(&[0b0010_0000, 0b1000_1000, 0xAA]), Some(2));
        // Octet de prévention d'émulation compté : 00 00 03 puis la suite.
        assert_eq!(bytes_covering_pps(&[0, 0, 3, 0b0000_0001, 0xFF, 0xFF, 0xFF, 0xFF]), Some(8));
        assert_eq!(bytes_covering_pps(&[0, 0, 0]), None);
    }

    #[test]
    fn plages_h264() {
        let mut f = vec![0, 0, 1, 0x67, 1, 2, 3];
        f.extend_from_slice(&[0, 0, 0, 1, 0x68, 4]);
        f.extend_from_slice(&[0, 0, 1, 0x65, 0b1110_0000, 9, 9, 9]);
        let (out, clear, h26x) = layout(Codec::H264, &f).unwrap_or_else(|| panic!("image valide"));
        assert!(h26x);
        let want = vec![0, 0, 0, 1, 0x67, 1, 2, 3, 0, 0, 0, 1, 0x68, 4, 0, 0, 0, 1, 0x65, 0b1110_0000, 9, 9, 9];
        assert_eq!(out, want);
        // SPS, PPS, code de début et en-tête de la tranche, premier octet de l'en-tête de tranche.
        assert_eq!(clear, vec![(0, 20)]);
        assert!(layout(Codec::H264, &[0x65, 1]).is_none(), "pas de code de début");
        assert_eq!(layout(Codec::Vp8, &[0x00; 12]).map(|l| l.1), Some(vec![(0, 10)]));
        assert_eq!(layout(Codec::Vp8, &[0x01; 3]).map(|l| l.1), Some(vec![(0, 1)]));
        assert!(layout(Codec::Vp8, &[0x00; 3]).is_none());
        assert!(layout(Codec::Av1, &[1]).is_none());
    }
}
