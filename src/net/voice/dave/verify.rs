//! Codes de vérification (protocol.md, « Displayable Codes » et
//! « Verification Fingerprint ») : empreinte de paire par scrypt et code
//! affichable par groupes de 5 chiffres.

use std::num::NonZeroU32;

use aws_lc_rs::pbkdf2;

const SALT: [u8; 16] = [0x24, 0xca, 0xb1, 0x7a, 0x7a, 0xf8, 0xec, 0x2b, 0x82, 0xb4, 0x12, 0xb9, 0x2d, 0xab, 0x19, 0x2e];

/// Code affichable : `len / group` groupes, chacun lu sur `group` octets
/// gros-boutistes, réduit modulo 10^group.
pub fn displayable_code(data: &[u8], len: usize, group: usize) -> Option<String> {
    if data.len() < len || group == 0 || group >= 8 || !len.is_multiple_of(group) {
        return None;
    }
    let modulo = 10u64.pow(group as u32);
    let mut out = String::with_capacity(len);
    for chunk in data[..len].chunks(group) {
        let v = chunk.iter().fold(0u64, |acc, &b| acc << 8 | u64::from(b));
        out.push_str(&format!("{:0width$}", v % modulo, width = group));
    }
    Some(out)
}

/// Empreinte d'une clé : version (u16) || clé publique || identifiant (u64), gros-boutistes.
fn key_fingerprint(key: &[u8], user_id: u64) -> Vec<u8> {
    let mut v = vec![0u8, 0u8];
    v.extend_from_slice(key);
    v.extend_from_slice(&user_id.to_be_bytes());
    v
}

/// Empreinte de paire (64 octets), identique des deux côtés.
pub fn pairwise_fingerprint(key_a: &[u8], user_a: u64, key_b: &[u8], user_b: u64) -> Option<Vec<u8>> {
    let mut parts = [key_fingerprint(key_a, user_a), key_fingerprint(key_b, user_b)];
    parts.sort();
    let pass = parts.concat();
    scrypt(&pass, &SALT, 16384, 8, 2, 64)
}

fn pbkdf2_1(pass: &[u8], salt: &[u8], out: &mut [u8]) {
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, NonZeroU32::MIN, salt, pass, out);
}

fn salsa20_8(b: &mut [u32; 16]) {
    let mut x = *b;
    for _ in 0..4 {
        macro_rules! qr {
            ($a:expr, $b:expr, $c:expr, $d:expr) => {
                x[$b] ^= x[$a].wrapping_add(x[$d]).rotate_left(7);
                x[$c] ^= x[$b].wrapping_add(x[$a]).rotate_left(9);
                x[$d] ^= x[$c].wrapping_add(x[$b]).rotate_left(13);
                x[$a] ^= x[$d].wrapping_add(x[$c]).rotate_left(18);
            };
        }
        qr!(0, 4, 8, 12);
        qr!(5, 9, 13, 1);
        qr!(10, 14, 2, 6);
        qr!(15, 3, 7, 11);
        qr!(0, 1, 2, 3);
        qr!(5, 6, 7, 4);
        qr!(10, 11, 8, 9);
        qr!(15, 12, 13, 14);
    }
    for (o, v) in b.iter_mut().zip(x) {
        *o = o.wrapping_add(v);
    }
}

/// scryptBlockMix : `b` fait 2r blocs de 16 mots ; `y` sert de tampon.
fn block_mix(b: &mut [u32], y: &mut [u32], r: usize) {
    let mut x = [0u32; 16];
    x.copy_from_slice(&b[(2 * r - 1) * 16..]);
    for i in 0..2 * r {
        for (xv, bv) in x.iter_mut().zip(&b[i * 16..i * 16 + 16]) {
            *xv ^= bv;
        }
        salsa20_8(&mut x);
        // Blocs pairs d'abord, impairs ensuite.
        let dst = if i % 2 == 0 { i / 2 } else { r + i / 2 };
        y[dst * 16..dst * 16 + 16].copy_from_slice(&x);
    }
    b.copy_from_slice(y);
}

/// scrypt (RFC 7914). Rend None si les paramètres sont absurdes.
pub fn scrypt(pass: &[u8], salt: &[u8], n: usize, r: usize, p: usize, dk_len: usize) -> Option<Vec<u8>> {
    if n < 2 || !n.is_power_of_two() || r == 0 || p == 0 {
        return None;
    }
    let block = 128 * r;
    let mut b = vec![0u8; p * block];
    pbkdf2_1(pass, salt, &mut b);
    let words = 32 * r;
    let mut v = vec![0u32; n * words];
    let mut x = vec![0u32; words];
    let mut y = vec![0u32; words];
    for chunk in b.chunks_mut(block) {
        for (w, c) in x.iter_mut().zip(chunk.chunks(4)) {
            *w = u32::from_le_bytes([c[0], c[1], c[2], c[3]]);
        }
        for i in 0..n {
            v[i * words..(i + 1) * words].copy_from_slice(&x);
            block_mix(&mut x, &mut y, r);
        }
        for _ in 0..n {
            let j = x[(2 * r - 1) * 16] as usize & (n - 1);
            for (xv, vv) in x.iter_mut().zip(&v[j * words..(j + 1) * words]) {
                *xv ^= vv;
            }
            block_mix(&mut x, &mut y, r);
        }
        for (c, w) in chunk.chunks_mut(4).zip(&x) {
            c.copy_from_slice(&w.to_le_bytes());
        }
    }
    let mut out = vec![0u8; dk_len];
    pbkdf2_1(pass, &b, &mut out);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    /// Vecteurs de libdave (js/__tests__/DisplayableCode-test.ts).
    #[test]
    fn code_affichable() {
        assert_eq!(displayable_code(&[0xaa, 0xbb, 0xcc, 0xdd, 0xee], 5, 5).as_deref(), Some("05870"));
        let long = hex("aabbccddeebbccddeeffccddeeffaaddeeffaabbeeffaabbccffaabbccdd");
        assert_eq!(displayable_code(&long, 30, 5).as_deref(), Some("058708105556138052119572494877"));
        assert!(displayable_code(&[0xaa, 0xbb, 0xcc, 0xdd], 5, 5).is_none());
        assert!(displayable_code(&[0xaa, 0xbb, 0xcc, 0xdd], 4, 3).is_none());
        assert!(displayable_code(&[0u8; 1024], 1024, 11).is_none());
    }

    /// RFC 7914, 12 (premier vecteur).
    #[test]
    fn scrypt_rfc7914() {
        assert_eq!(
            scrypt(b"", b"", 16, 1, 1, 64).unwrap(),
            hex("77d6576238657b203b19ca42c18a0497f16b4844e3074ae8dfdffa3fede21442fcd0069ded0948f8326a753a0fc81f17e8d3e0fb2e0d3628cf35e20c38d18906")
        );
    }

    /// Vecteurs de libdave (js/__tests__/PairwiseFingerprint-test.ts).
    #[test]
    fn empreinte_de_paire() {
        let fp = pairwise_fingerprint(&[0u8; 33], 1234, &[0u8; 65], 5678).unwrap();
        assert_eq!(
            fp,
            [
                133, 129, 241, 44, 36, 135, 79, 195, 27, 28, 151, 69, 124, 197, 189, 41, 192, 7, 16, 45, 79, 247, 138, 58, 126, 161, 178, 136, 12,
                109, 96, 164, 169, 92, 2, 232, 136, 174, 74, 156, 173, 144, 191, 184, 34, 45, 242, 136, 41, 133, 14, 158, 119, 79, 204, 48, 6, 220,
                121, 6, 242, 11, 164, 60,
            ]
        );
        let bad_sort = pairwise_fingerprint(&[0, 100], 1, &[0, 20], 2).unwrap();
        assert_eq!(&bad_sort[..4], &[141, 169, 194, 143]);
        // Symétrique.
        assert_eq!(pairwise_fingerprint(&[0u8; 65], 5678, &[0u8; 33], 1234).unwrap(), fp);
    }
}
