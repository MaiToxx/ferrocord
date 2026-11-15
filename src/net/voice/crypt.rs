//! Chiffrement du transport vocal, modes « rtpsize » (documentation :
//! Voice Connections, Transport Encryption Modes) :
//! - données authentifiées : l'en-tête RTP non chiffré (12 octets, les CSRC
//!   et, s'il y a une extension, ses 4 premiers octets) ;
//! - nonce : un compteur de 32 bits ajouté en clair à la fin du paquet,
//!   gros-boutiste, complété par des zéros jusqu'à la taille du nonce ;
//! - paquet : en-tête ‖ chiffré ‖ étiquette (16 octets) ‖ compteur (4 octets).
//!
//! AES-256-GCM et ChaCha20-Poly1305 viennent d'aws-lc-rs, déjà dans
//! l'exécutable pour TLS (rustls). XChaCha20 = HChaCha20 (écrit ici, une
//! vingtaine de lignes) puis ChaCha20-Poly1305 : pas de nouvelle crate.

use aws_lc_rs::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, CHACHA20_POLY1305};

/// Étiquette d'authentification (les deux modes).
pub const TAG_LEN: usize = 16;
/// Compteur ajouté en fin de paquet.
pub const NONCE_LEN: usize = 4;

/// Modes de chiffrement pris en charge, du préféré au moins préféré.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Préféré par Discord quand le processeur a AES-NI.
    Aes256Gcm,
    /// Obligatoire : tous les serveurs vocaux le proposent.
    XChaCha20Poly1305,
}

impl Mode {
    pub const PREFERENCE: [Mode; 2] = [Mode::Aes256Gcm, Mode::XChaCha20Poly1305];

    pub fn name(self) -> &'static str {
        match self {
            Mode::Aes256Gcm => "aead_aes256_gcm_rtpsize",
            Mode::XChaCha20Poly1305 => "aead_xchacha20_poly1305_rtpsize",
        }
    }

    pub fn from_name(s: &str) -> Option<Mode> {
        Mode::PREFERENCE.into_iter().find(|m| m.name() == s)
    }

    /// Le mode préféré parmi ceux que propose le serveur (Ready, `modes`).
    pub fn choose<S: AsRef<str>>(offered: &[S]) -> Option<Mode> {
        Mode::PREFERENCE.into_iter().find(|m| offered.iter().any(|o| o.as_ref() == m.name()))
    }
}

/// Clé de session (Session Description, `secret_key`) et son mode.
pub struct Cipher {
    mode: Mode,
    key: [u8; 32],
    /// Clé AES préparée une fois ; XChaCha20 dérive une sous-clé par paquet.
    aes: Option<LessSafeKey>,
}

impl std::fmt::Debug for Cipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Cipher({:?}, clé masquée)", self.mode)
    }
}

impl Cipher {
    pub fn new(mode: Mode, key: [u8; 32]) -> Option<Cipher> {
        let aes = match mode {
            Mode::Aes256Gcm => Some(LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &key).ok()?)),
            Mode::XChaCha20Poly1305 => None,
        };
        Some(Cipher { mode, key, aes })
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Paquet complet : `header` en clair, `payload` chiffré, puis le compteur.
    pub fn seal(&self, header: &[u8], payload: &[u8], counter: u32) -> Option<Vec<u8>> {
        let mut data = Vec::with_capacity(header.len() + payload.len() + TAG_LEN + NONCE_LEN);
        data.extend_from_slice(payload);
        match self.mode {
            Mode::Aes256Gcm => {
                let key = self.aes.as_ref()?;
                key.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce12(counter)), Aad::from(header), &mut data).ok()?;
            }
            Mode::XChaCha20Poly1305 => xchacha_seal(&self.key, &nonce24(counter), header, &mut data)?,
        }
        let mut pkt = Vec::with_capacity(header.len() + data.len() + NONCE_LEN);
        pkt.extend_from_slice(header);
        pkt.extend_from_slice(&data);
        pkt.extend_from_slice(&counter.to_be_bytes());
        Some(pkt)
    }

    /// Clair de `body` (chiffré ‖ étiquette) ; `None` si l'authentification échoue.
    pub fn open(&self, aad: &[u8], body: &[u8], counter: [u8; 4]) -> Option<Vec<u8>> {
        let counter = u32::from_be_bytes(counter);
        let mut data = body.to_vec();
        let n = match self.mode {
            Mode::Aes256Gcm => {
                let key = self.aes.as_ref()?;
                key.open_in_place(Nonce::assume_unique_for_key(nonce12(counter)), Aad::from(aad), &mut data).ok()?.len()
            }
            Mode::XChaCha20Poly1305 => xchacha_open(&self.key, &nonce24(counter), aad, &mut data)?,
        };
        data.truncate(n);
        Some(data)
    }
}

fn nonce12(counter: u32) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[..4].copy_from_slice(&counter.to_be_bytes());
    n
}

fn nonce24(counter: u32) -> [u8; 24] {
    let mut n = [0u8; 24];
    n[..4].copy_from_slice(&counter.to_be_bytes());
    n
}

/// Clé et nonce ChaCha20-Poly1305 (IETF) d'un nonce XChaCha20 de 24 octets
/// (draft-irtf-cfrg-xchacha, section 2.3).
fn xchacha_key(key: &[u8; 32], nonce: &[u8; 24]) -> Option<(LessSafeKey, Nonce)> {
    let mut head = [0u8; 16];
    head.copy_from_slice(&nonce[..16]);
    let sub = hchacha20(key, &head);
    let mut n = [0u8; 12];
    n[4..].copy_from_slice(&nonce[16..]);
    let k = LessSafeKey::new(UnboundKey::new(&CHACHA20_POLY1305, &sub).ok()?);
    Some((k, Nonce::assume_unique_for_key(n)))
}

/// Chiffre `data` en place et lui ajoute l'étiquette.
pub fn xchacha_seal(key: &[u8; 32], nonce: &[u8; 24], aad: &[u8], data: &mut Vec<u8>) -> Option<()> {
    let (k, n) = xchacha_key(key, nonce)?;
    k.seal_in_place_append_tag(n, Aad::from(aad), data).ok()
}

/// Déchiffre `data` (chiffré ‖ étiquette) en place ; longueur du clair.
pub fn xchacha_open(key: &[u8; 32], nonce: &[u8; 24], aad: &[u8], data: &mut [u8]) -> Option<usize> {
    let (k, n) = xchacha_key(key, nonce)?;
    k.open_in_place(n, Aad::from(aad), data).ok().map(|p| p.len())
}

/// HChaCha20 (draft-irtf-cfrg-xchacha, section 2.2) : 20 tours ChaCha sans
/// l'addition finale ; la sous-clé est faite des mots 0 à 3 et 12 à 15.
pub fn hchacha20(key: &[u8; 32], nonce: &[u8; 16]) -> [u8; 32] {
    let mut s = [0u32; 16];
    s[..4].copy_from_slice(&[0x6170_7865, 0x3320_646e, 0x7962_2d32, 0x6b20_6574]);
    for (i, c) in key.as_chunks::<4>().0.iter().enumerate() {
        s[4 + i] = u32::from_le_bytes(*c);
    }
    for (i, c) in nonce.as_chunks::<4>().0.iter().enumerate() {
        s[12 + i] = u32::from_le_bytes(*c);
    }
    fn qr(s: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(16);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(12);
        s[a] = s[a].wrapping_add(s[b]);
        s[d] = (s[d] ^ s[a]).rotate_left(8);
        s[c] = s[c].wrapping_add(s[d]);
        s[b] = (s[b] ^ s[c]).rotate_left(7);
    }
    for _ in 0..10 {
        qr(&mut s, 0, 4, 8, 12);
        qr(&mut s, 1, 5, 9, 13);
        qr(&mut s, 2, 6, 10, 14);
        qr(&mut s, 3, 7, 11, 15);
        qr(&mut s, 0, 5, 10, 15);
        qr(&mut s, 1, 6, 11, 12);
        qr(&mut s, 2, 7, 8, 13);
        qr(&mut s, 3, 4, 9, 14);
    }
    let mut out = [0u8; 32];
    for (i, w) in s[..4].iter().chain(&s[12..]).enumerate() {
        out[4 * i..4 * i + 4].copy_from_slice(&w.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0)).collect()
    }

    #[test]
    fn hchacha20_matches_draft_vector() {
        // draft-irtf-cfrg-xchacha-03, 2.2.1.
        let key: [u8; 32] = std::array::from_fn(|i| i as u8);
        let nonce: [u8; 16] = hex("000000090000004a0000000031415927").try_into().unwrap_or([0; 16]);
        assert_eq!(hchacha20(&key, &nonce).to_vec(), hex("82413b4227b27bfed30e42508a877d73a0f9e4d58a74a853c12ec41326d3ecdc"));
    }

    #[test]
    fn xchacha_aead_matches_draft_vector() {
        // draft-irtf-cfrg-xchacha-03, A.3.1.
        let pt = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let aad = hex("50515253c0c1c2c3c4c5c6c7");
        let key: [u8; 32] = std::array::from_fn(|i| 0x80 + i as u8);
        let nonce: [u8; 24] = std::array::from_fn(|i| 0x40 + i as u8);
        let mut data = pt.to_vec();
        assert!(xchacha_seal(&key, &nonce, &aad, &mut data).is_some());
        let expected = hex(concat!(
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb",
            "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452",
            "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9",
            "21f9664c97637da9768812f615c68b13b52e",
            "c0875924c1c7987947deafd8780acf49",
        ));
        assert_eq!(data, expected);
        let n = xchacha_open(&key, &nonce, &aad, &mut data);
        assert_eq!(n.map(|n| &data[..n]), Some(&pt[..]));
    }

    #[test]
    fn both_modes_round_trip_and_reject_tampering() {
        let key = [7u8; 32];
        let header = [0x80, 0x78, 0, 1, 0, 0, 3, 0xC0, 0, 0, 0, 9];
        for mode in Mode::PREFERENCE {
            let c = Cipher::new(mode, key).unwrap_or_else(|| panic!("clé {mode:?}"));
            let pkt = c.seal(&header, b"opus", 0x0102_0304).unwrap_or_default();
            assert_eq!(pkt.len(), 12 + 4 + TAG_LEN + NONCE_LEN);
            assert_eq!(&pkt[..12], &header);
            assert_eq!(&pkt[pkt.len() - 4..], &[1, 2, 3, 4]);
            let body = &pkt[12..pkt.len() - 4];
            assert_eq!(c.open(&header, body, [1, 2, 3, 4]).as_deref(), Some(&b"opus"[..]));
            // Mauvais compteur, en-tête modifié : refusés.
            assert_eq!(c.open(&header, body, [1, 2, 3, 5]), None);
            let mut h2 = header;
            h2[3] = 2;
            assert_eq!(c.open(&h2, body, [1, 2, 3, 4]), None);
        }
    }

    #[test]
    fn mode_choice_prefers_aes() {
        assert_eq!(Mode::choose(&["aead_xchacha20_poly1305_rtpsize", "aead_aes256_gcm_rtpsize"]), Some(Mode::Aes256Gcm));
        assert_eq!(Mode::choose(&["xsalsa20_poly1305", "aead_xchacha20_poly1305_rtpsize"]), Some(Mode::XChaCha20Poly1305));
        assert_eq!(Mode::choose(&["xsalsa20_poly1305_lite"]), None);
        assert!(format!("{:?}", Cipher::new(Mode::Aes256Gcm, [1; 32])).contains("masquée"));
    }
}
