//! AV1 : lecture des OBU d'une image (spécification AV1, 5.3 et 5.5), sans
//! décodage. Le codec lui-même est celui de Windows (`mf.rs`, « Extension
//! vidéo AV1 » de Microsoft, à installer depuis le Microsoft Store quand
//! elle manque). Sans elle, AV1 n'est pas annoncé à la passerelle.

const HAS_EXTENSION: u8 = 0x04;
const HAS_SIZE: u8 = 0x02;
const SEQUENCE_HEADER: u8 = 1;
const TEMPORAL_DELIMITER: u8 = 2;

/// Entier LEB128 : valeur et octets lus.
fn leb128(d: &[u8]) -> Option<(usize, usize)> {
    let mut v = 0u64;
    for (i, &b) in d.iter().enumerate().take(8) {
        v |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return usize::try_from(v).ok().map(|v| (v, i + 1));
        }
    }
    None
}

/// Une OBU : (octet d'en-tête, extension, contenu).
pub(super) type Obu<'a> = (u8, Option<u8>, &'a [u8]);

/// OBU d'une image. Une OBU sans
/// champ de taille s'étend jusqu'à la fin. `None` : image abîmée.
pub(super) fn obus(d: &[u8]) -> Option<Vec<Obu<'_>>> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < d.len() {
        let head = d[at];
        // Bit interdit posé : ce n'est pas une OBU.
        if head & 0x80 != 0 {
            return None;
        }
        at += 1;
        let ext = if head & HAS_EXTENSION != 0 {
            at += 1;
            Some(*d.get(at - 1)?)
        } else {
            None
        };
        let len = if head & HAS_SIZE != 0 {
            let (len, n) = leb128(d.get(at..)?)?;
            at += n;
            len
        } else {
            d.len().checked_sub(at)?
        };
        out.push((head, ext, d.get(at..at.checked_add(len)?)?));
        at += len;
    }
    Some(out)
}

fn kind(head: u8) -> u8 {
    (head >> 3) & 0x0f
}

/// L'image porte un en-tête de séquence : un décodeur peut partir d'elle.
#[cfg(test)]
pub fn is_keyframe(d: &[u8]) -> bool {
    obus(d).is_some_and(|o| o.iter().any(|(h, _, _)| kind(*h) == SEQUENCE_HEADER))
}

/// Unité temporelle au format des décodeurs (« low overhead bitstream
/// format », 5.2) : un délimiteur temporel en tête, et chaque OBU avec son
/// champ de taille. L'image reçue n'a ni l'un ni, pour sa dernière OBU,
/// l'autre (voir `net::voice::video::av1`).
pub fn temporal_unit(d: &[u8]) -> Option<Vec<u8>> {
    let list = obus(d)?;
    if list.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(d.len() + 8);
    out.extend_from_slice(&[(TEMPORAL_DELIMITER << 3) | HAS_SIZE, 0]);
    for (head, ext, payload) in list {
        if kind(head) == TEMPORAL_DELIMITER {
            continue;
        }
        out.push(head | HAS_SIZE);
        out.extend(ext);
        let mut len = payload.len();
        loop {
            let b = (len & 0x7f) as u8;
            len >>= 7;
            if len == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out.extend_from_slice(payload);
    }
    Some(out)
}

/// Taille maximale des images d'une séquence (largeur, hauteur), lue dans
/// l'en-tête de séquence (5.5.1) ; `None` si l'image n'en porte pas.
pub fn sequence_size(d: &[u8]) -> Option<(u32, u32)> {
    let list = obus(d)?;
    let seq = list.iter().find(|(h, _, _)| kind(*h) == SEQUENCE_HEADER)?.2;
    let mut at = 0usize;
    let mut bits = |n: usize| -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = (v << 1) | u32::from((*seq.get(at / 8)? >> (7 - at % 8)) & 1);
            at += 1;
        }
        Some(v)
    };
    bits(3)?; // seq_profile
    bits(1)?; // still_picture
    if bits(1)? == 1 {
        // reduced_still_picture_header : un seul niveau.
        bits(5)?;
    } else {
        let mut decoder_model = None;
        if bits(1)? == 1 {
            // timing_info
            bits(32)?;
            bits(32)?;
            if bits(1)? == 1 {
                // num_ticks_per_picture_minus_1, en uvlc.
                let mut zeros = 0;
                while bits(1)? == 0 {
                    zeros += 1;
                    if zeros > 32 {
                        return None;
                    }
                }
                if zeros < 32 {
                    bits(zeros)?;
                }
            }
            if bits(1)? == 1 {
                // decoder_model_info
                let delay = bits(5)? as usize + 1;
                bits(32)?;
                bits(5)?;
                bits(5)?;
                decoder_model = Some(delay);
            }
        }
        let display_delay = bits(1)? == 1;
        for _ in 0..=bits(5)? {
            bits(12)?; // operating_point_idc
            if bits(5)? > 7 {
                bits(1)?; // seq_tier
            }
            if let Some(delay) = decoder_model
                && bits(1)? == 1
            {
                bits(delay)?;
                bits(delay)?;
                bits(1)?;
            }
            if display_delay && bits(1)? == 1 {
                bits(4)?;
            }
        }
    }
    let (wb, hb) = (bits(4)? as usize + 1, bits(4)? as usize + 1);
    let (w, h) = (bits(wb)? + 1, bits(hb)? + 1);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// En-tête de séquence minimal : profil 0, un point de fonctionnement
    /// de niveau 8 (donc avec `seq_tier`), tailles sur 9 et 8 bits, 320×240.
    fn sequence() -> Vec<u8> {
        let mut bits: Vec<u8> = Vec::new();
        let mut put = |v: u32, n: usize| bits.extend((0..n).rev().map(|i| ((v >> i) & 1) as u8));
        put(0, 3); // profil
        put(0, 1); // still_picture
        put(0, 1); // reduced
        put(0, 1); // timing_info
        put(0, 1); // initial_display_delay
        put(0, 5); // un point de fonctionnement
        put(0, 12);
        put(8, 5);
        put(0, 1); // seq_tier
        put(8, 4); // largeur sur 9 bits
        put(7, 4); // hauteur sur 8 bits
        put(319, 9);
        put(239, 8);
        put(0, 3);
        bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (i, b)| a | (b << (7 - i)))).collect()
    }

    #[test]
    fn taille_de_la_sequence() {
        let seq = sequence();
        // Image reçue : en-tête de séquence avec sa taille, puis une OBU d'image sans taille.
        let mut f = vec![0x0a, seq.len() as u8];
        f.extend_from_slice(&seq);
        f.extend_from_slice(&[0x30, 1, 2, 3]);
        assert_eq!(sequence_size(&f), Some((320, 240)));
        assert!(is_keyframe(&f));
        // Image intermédiaire : pas d'en-tête de séquence.
        assert_eq!(sequence_size(&[0x30, 1, 2, 3]), None);
        assert!(!is_keyframe(&[0x30, 1, 2, 3]));
        // En-tête coupé, taille d'OBU qui dépasse, bit interdit.
        assert_eq!(sequence_size(&[0x0a, 2, seq[0], seq[1]]), None);
        assert_eq!(sequence_size(&[0x0a, 9, 0]), None);
        assert!(!is_keyframe(&[0x8a, 0]));
        assert!(!is_keyframe(&[]));
    }

    #[test]
    fn modele_de_decodeur_et_image_fixe() {
        // reduced_still_picture_header : niveau seul avant les tailles (16×16 sur 4 bits).
        let mut bits: Vec<u8> = Vec::new();
        let mut put = |v: u32, n: usize| bits.extend((0..n).rev().map(|i| ((v >> i) & 1) as u8));
        put(0, 3);
        put(1, 1);
        put(1, 1);
        put(0, 5);
        put(3, 4);
        put(3, 4);
        put(15, 4);
        put(15, 4);
        put(0, 6);
        let seq: Vec<u8> = bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (i, b)| a | (b << (7 - i)))).collect();
        let mut f = vec![0x08];
        f.extend_from_slice(&seq);
        assert_eq!(sequence_size(&f), Some((16, 16)));

        // timing_info avec images à intervalle fixe (uvlc « 010 ») et modèle
        // de décodeur (délais sur 2 bits), présent pour le point de fonctionnement.
        let mut bits: Vec<u8> = Vec::new();
        let mut put = |v: u32, n: usize| bits.extend((0..n).rev().map(|i| ((v >> i) & 1) as u8));
        put(0, 5);
        put(1, 1); // timing_info
        put(1, 32);
        put(30, 32);
        put(1, 1); // equal_picture_interval
        put(0b010, 3);
        put(1, 1); // decoder_model_info
        put(1, 5);
        put(1, 32);
        put(0, 5);
        put(0, 5);
        put(1, 1); // initial_display_delay
        put(0, 5);
        put(0, 12);
        put(4, 5);
        put(1, 1); // modèle présent
        put(3, 2);
        put(3, 2);
        put(0, 1);
        put(1, 1); // délai d'affichage présent
        put(9, 4);
        put(10, 4);
        put(10, 4);
        put(1279, 11);
        put(719, 11);
        put(0, 7);
        let seq: Vec<u8> = bits.chunks(8).map(|c| c.iter().enumerate().fold(0u8, |a, (i, b)| a | (b << (7 - i)))).collect();
        let mut f = vec![0x08];
        f.extend_from_slice(&seq);
        assert_eq!(sequence_size(&f), Some((1280, 720)));
    }

    #[test]
    fn unite_temporelle() {
        // Deux OBU, la dernière sans taille : délimiteur ajouté, tailles partout.
        let f = [0x0a, 2, 0xaa, 0xbb, 0x34, 0x20, 1, 2, 3];
        assert_eq!(temporal_unit(&f), Some(vec![0x12, 0, 0x0a, 2, 0xaa, 0xbb, 0x36, 0x20, 3, 1, 2, 3]));
        // Déjà au format des décodeurs : inchangée (un seul délimiteur).
        let done = temporal_unit(&f).unwrap_or_default();
        assert_eq!(temporal_unit(&done), Some(done.clone()));
        // Contenu de 200 octets : taille sur deux octets.
        let mut big = vec![0x30];
        big.extend(std::iter::repeat_n(7u8, 200));
        let t = temporal_unit(&big).unwrap_or_default();
        assert_eq!(&t[..5], &[0x12, 0, 0x32, 0xc8, 0x01]);
        assert_eq!(t.len(), 205);
        assert_eq!(temporal_unit(&[]), None);
        assert_eq!(temporal_unit(&[0x0a, 5, 1]), None);
    }
}
