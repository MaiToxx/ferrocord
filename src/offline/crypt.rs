//! Chiffrement des fichiers de l'historique hors ligne : AES-256-GCM
//! (aws-lc-rs, déjà dans l'exécutable), un nonce aléatoire par écriture.
//! Le compte fait partie des données authentifiées : le fichier d'un compte
//! renommé à la place d'un autre ne se déchiffre pas.
//!
//! Format : `FCH1`, nonce (12 octets), texte chiffré suivi de l'étiquette.

use aws_lc_rs::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_256_GCM, NONCE_LEN};

const MAGIC: &[u8; 4] = b"FCH1";
pub const KEY_LEN: usize = 32;

/// Clé de l'historique, jamais écrite en clair (voir `offline::key`).
pub struct Key(LessSafeKey);

impl Key {
    pub fn new(bytes: &[u8; KEY_LEN]) -> Option<Key> {
        Some(Key(LessSafeKey::new(UnboundKey::new(&AES_256_GCM, bytes).ok()?)))
    }

    /// Nouvelle clé aléatoire (octets à protéger, et la clé).
    pub fn random() -> Option<([u8; KEY_LEN], Key)> {
        let mut bytes = [0u8; KEY_LEN];
        aws_lc_rs::rand::fill(&mut bytes).ok()?;
        let key = Key::new(&bytes)?;
        Some((bytes, key))
    }
}

fn aad(user: u64) -> [u8; 12] {
    let mut a = [0u8; 12];
    a[..4].copy_from_slice(MAGIC);
    a[4..].copy_from_slice(&user.to_le_bytes());
    a
}

/// Chiffre `plain` pour le compte `user`.
pub fn seal(key: &Key, user: u64, plain: Vec<u8>) -> Option<Vec<u8>> {
    let mut nonce = [0u8; NONCE_LEN];
    aws_lc_rs::rand::fill(&mut nonce).ok()?;
    let mut body = plain;
    key.0.seal_in_place_append_tag(Nonce::assume_unique_for_key(nonce), Aad::from(aad(user)), &mut body).ok()?;
    let mut out = Vec::with_capacity(MAGIC.len() + NONCE_LEN + body.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&body);
    Some(out)
}

/// Déchiffre ; fichier abîmé, autre clé ou autre compte : `None`.
pub fn open(key: &Key, user: u64, sealed: &[u8]) -> Option<Vec<u8>> {
    let rest = sealed.strip_prefix(&MAGIC[..])?;
    let (nonce, body) = rest.split_at_checked(NONCE_LEN)?;
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut body = body.to_vec();
    let len = key.0.open_in_place(nonce, Aad::from(aad(user)), &mut body).ok()?.len();
    body.truncate(len);
    Some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_tampering() {
        let (_, key) = Key::random().unwrap();
        let sealed = seal(&key, 42, b"bonjour".to_vec()).unwrap();
        assert!(!sealed.windows(7).any(|w| w == b"bonjour"), "rien en clair");
        assert_eq!(open(&key, 42, &sealed).as_deref(), Some(&b"bonjour"[..]));
        assert_eq!(open(&key, 43, &sealed), None, "autre compte");
        let (_, other) = Key::random().unwrap();
        assert_eq!(open(&other, 42, &sealed), None, "autre clé");
        let mut bad = sealed.clone();
        let last = bad.len() - 1;
        bad[last] ^= 1;
        assert_eq!(open(&key, 42, &bad), None, "modifié");
        assert_eq!(open(&key, 42, &sealed[..10]), None, "tronqué");
        assert_ne!(seal(&key, 42, b"bonjour".to_vec()), Some(sealed), "nonce neuf à chaque écriture");
    }
}
