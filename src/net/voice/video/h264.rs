//! Charge utile RTP H.264, RFC 6184 (mode de paquétisation 1, celui que
//! Discord annonce : `packetization-mode=1`, docs.discord.food, Voice
//! Connections) : NAL simple, agrégation STAP-A, fragmentation FU-A.
//! Les images se lisent et s'écrivent en Annex B ; à la sortie du
//! dépaquétiseur, chaque NAL est précédée d'un code de début de 4 octets,
//! comme le fait DAVE avant de chiffrer (libdave, `codec_utils.cpp`) : les
//! deux côtés voient ainsi exactement les mêmes octets.

/// Code de début long, ajouté avant chaque NAL.
pub const START_CODE: [u8; 4] = [0, 0, 0, 1];
const STAP_A: u8 = 24;
const FU_A: u8 = 28;
pub const NAL_SLICE: u8 = 1;
pub const NAL_IDR: u8 = 5;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

/// Position des codes de début : (début du code, début de la NAL). Un seul
/// zéro de tête est rattaché au code (règle de WebRTC) : une NAL chiffrée
/// par DAVE qui finit par des zéros les garde.
fn start_codes(b: &[u8]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 3 <= b.len() {
        if b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 1 {
            let sc = if i > 0 && b[i - 1] == 0 { i - 1 } else { i };
            out.push((sc, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    out
}

/// NAL d'une image Annex B (sans les codes de début). Sans aucun code de
/// début, l'image entière est prise pour une seule NAL.
pub fn nal_units(b: &[u8]) -> Vec<&[u8]> {
    let starts = start_codes(b);
    if starts.is_empty() {
        return if b.is_empty() { Vec::new() } else { vec![b] };
    }
    let mut out = Vec::with_capacity(starts.len());
    for (k, &(_, from)) in starts.iter().enumerate() {
        let to = starts.get(k + 1).map_or(b.len(), |s| s.0);
        if to > from {
            out.push(&b[from..to]);
        }
    }
    out
}

pub fn nal_type(nal: &[u8]) -> u8 {
    nal.first().map_or(0, |h| h & 0x1F)
}

/// Image clé : elle contient une NAL IDR.
pub fn is_keyframe(annexb: &[u8]) -> bool {
    nal_units(annexb).iter().any(|n| nal_type(n) == NAL_IDR)
}

/// Charges utiles RTP d'une image, `max` octets au plus chacune.
pub fn packetize(frame: &[u8], max: usize) -> Vec<Vec<u8>> {
    let max = max.max(3);
    let nals = nal_units(frame);
    let mut out = Vec::new();
    let mut i = 0;
    while i < nals.len() {
        let n = nals[i];
        if n.len() > max {
            fragment(n, max, &mut out);
            i += 1;
            continue;
        }
        // Petites NAL voisines (SPS, PPS, SEI…) réunies dans un STAP-A.
        let mut j = i;
        let mut size = 1;
        while j < nals.len() && size + 2 + nals[j].len() <= max {
            size += 2 + nals[j].len();
            j += 1;
        }
        if j - i >= 2 {
            let group = &nals[i..j];
            let f = group.iter().fold(0u8, |a, n| a | (n[0] & 0x80));
            let nri = group.iter().map(|n| n[0] & 0x60).max().unwrap_or(0);
            let mut p = Vec::with_capacity(size);
            p.push(f | nri | STAP_A);
            for n in group {
                p.extend_from_slice(&(n.len() as u16).to_be_bytes());
                p.extend_from_slice(n);
            }
            out.push(p);
            i = j;
        } else {
            out.push(n.to_vec());
            i += 1;
        }
    }
    out
}

fn fragment(n: &[u8], max: usize, out: &mut Vec<Vec<u8>>) {
    let h = n[0];
    let body = &n[1..];
    let chunk = max - 2;
    let count = body.len().div_ceil(chunk);
    for (k, part) in body.chunks(chunk).enumerate() {
        let mut fu = h & 0x1F;
        if k == 0 {
            fu |= 0x80;
        }
        if k + 1 == count {
            fu |= 0x40;
        }
        let mut p = Vec::with_capacity(2 + part.len());
        p.push((h & 0xE0) | FU_A);
        p.push(fu);
        p.extend_from_slice(part);
        out.push(p);
    }
}

/// Image Annex B à partir des charges utiles d'une image, dans l'ordre.
/// `None` : fragment manquant ou charge utile d'un type non pris en charge.
pub fn depacketize<'a>(payloads: impl IntoIterator<Item = &'a [u8]>) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut in_fu = false;
    for p in payloads {
        let h = *p.first()?;
        match h & 0x1F {
            1..=23 => {
                if in_fu {
                    return None;
                }
                out.extend_from_slice(&START_CODE);
                out.extend_from_slice(p);
            }
            STAP_A => {
                if in_fu {
                    return None;
                }
                let mut r = &p[1..];
                while !r.is_empty() {
                    let n = usize::from(u16::from_be_bytes([*r.first()?, *r.get(1)?]));
                    r = &r[2..];
                    if n == 0 || n > r.len() {
                        return None;
                    }
                    out.extend_from_slice(&START_CODE);
                    out.extend_from_slice(&r[..n]);
                    r = &r[n..];
                }
            }
            FU_A => {
                let fu = *p.get(1)?;
                if fu & 0x80 != 0 {
                    if in_fu {
                        return None;
                    }
                    out.extend_from_slice(&START_CODE);
                    out.push((h & 0xE0) | (fu & 0x1F));
                    in_fu = true;
                } else if !in_fu {
                    return None;
                }
                out.extend_from_slice(&p[2..]);
                if fu & 0x40 != 0 {
                    in_fu = false;
                }
            }
            _ => return None,
        }
    }
    (!in_fu && !out.is_empty()).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nal(t: u8, len: usize, seed: u8) -> Vec<u8> {
        // Corps sans 00 00 0x (aucun code de début émulé), comme après la prévention d'émulation.
        let mut n = vec![0x60 | t];
        n.extend((0..len - 1).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) | 1));
        n
    }

    fn annexb(nals: &[Vec<u8>]) -> Vec<u8> {
        nals.iter().flat_map(|n| START_CODE.iter().chain(n.iter()).copied()).collect()
    }

    #[test]
    fn decoupage_annex_b() {
        let mut f = vec![0, 0, 1, 0x67, 1, 2];
        f.extend_from_slice(&[0, 0, 0, 1, 0x68, 3]);
        f.extend_from_slice(&[0, 0, 1, 0x65, 4, 0]);
        f.extend_from_slice(&[0, 0, 0, 1, 0x41, 9]);
        let n = nal_units(&f);
        assert_eq!(n, vec![&[0x67, 1, 2][..], &[0x68, 3], &[0x65, 4, 0], &[0x41, 9]]);
        assert!(is_keyframe(&f));
        assert!(!is_keyframe(&[0, 0, 0, 1, 0x41, 2]));
        assert_eq!(nal_units(&[0x41, 7]), vec![&[0x41, 7][..]]);
    }

    #[test]
    fn grosse_image_cle_aller_retour() {
        let frame = annexb(&[nal(NAL_SPS, 12, 1), nal(NAL_PPS, 5, 2), nal(NAL_IDR, 150_000, 3), nal(NAL_SLICE, 900, 4)]);
        let p = packetize(&frame, 1100);
        assert!(p.iter().all(|p| p.len() <= 1100));
        assert_eq!(p[0][0] & 0x1F, STAP_A, "SPS et PPS réunis");
        assert_eq!(p[1][0] & 0x1F, FU_A);
        assert_eq!(p.last().map(|p| p[0] & 0x1F), Some(NAL_SLICE));
        let back = depacketize(p.iter().map(Vec::as_slice));
        assert_eq!(back.as_deref(), Some(frame.as_slice()));
    }

    #[test]
    fn codes_courts_rendus_longs() {
        let f = [0, 0, 1, 0x41, 5, 6];
        assert_eq!(depacketize(packetize(&f, 1200).iter().map(Vec::as_slice)), Some(vec![0, 0, 0, 1, 0x41, 5, 6]));
    }

    #[test]
    fn fragment_perdu_refuse() {
        let frame = annexb(&[nal(NAL_IDR, 5000, 7)]);
        // Un fragment du milieu perdu ne se voit qu'aux numéros de séquence
        // (le récepteur n'assemble que des suites contiguës) ; ici : le début.
        let mut p = packetize(&frame, 1000);
        p.remove(0);
        assert_eq!(depacketize(p.iter().map(Vec::as_slice)), None, "début manquant");
        let p = packetize(&frame, 1000);
        assert_eq!(depacketize(p[..p.len() - 1].iter().map(Vec::as_slice)), None, "fin manquante");
        assert_eq!(depacketize([&[0x7Du8, 1][..]]), None, "type 29 inconnu");
        assert_eq!(depacketize([&[0x78u8, 0, 9, 1][..]]), None, "STAP-A tronqué");
    }
}
