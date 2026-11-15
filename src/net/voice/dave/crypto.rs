//! Primitives de la suite MLS 2 (DHKEMP256_AES128GCM_SHA256_P256) sur aws-lc-rs,
//! déjà présent dans l'exécutable par rustls : SHA-256, HMAC, HKDF, HPKE
//! (RFC 9180, mode de base), ECDSA P-256 et AES-128-GCM.

use aws_lc_rs::{aead, agreement, cipher, digest, hmac, rand, signature};

use super::Error;
use super::codec::{R, build};

/// Taille du hachage (Nh) et des secrets.
pub const NH: usize = 32;
pub const KEY_LEN: usize = 16;
pub const NONCE_LEN: usize = 12;

pub fn hash(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA256, data).as_ref().to_vec()
}

pub fn mac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let k = hmac::Key::new(hmac::HMAC_SHA256, key);
    hmac::sign(&k, data).as_ref().to_vec()
}

/// HKDF-Extract : HMAC(sel, ikm) ; un sel vide équivaut à Nh zéros.
pub fn extract(salt: &[u8], ikm: &[u8]) -> Vec<u8> {
    mac(salt, ikm)
}

/// HKDF-Expand (RFC 5869) ; `len` <= 255 * Nh.
pub fn expand(prk: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let k = hmac::Key::new(hmac::HMAC_SHA256, prk);
    let mut out = Vec::with_capacity(len);
    let mut t: Vec<u8> = Vec::new();
    let mut i: u8 = 1;
    while out.len() < len {
        let mut ctx = hmac::Context::with_key(&k);
        ctx.update(&t);
        ctx.update(info);
        ctx.update(&[i]);
        t = ctx.sign().as_ref().to_vec();
        out.extend_from_slice(&t);
        i = i.wrapping_add(1);
    }
    out.truncate(len);
    out
}

fn mls_label(label: &str) -> Vec<u8> {
    mls_label_bytes(label.as_bytes())
}

fn mls_label_bytes(label: &[u8]) -> Vec<u8> {
    let mut l = b"MLS 1.0 ".to_vec();
    l.extend_from_slice(label);
    l
}

/// ExpandWithLabel de MLS (RFC 9420, 8).
pub fn expand_with_label(secret: &[u8], label: &str, context: &[u8], len: usize) -> Vec<u8> {
    expand_with_label_bytes(secret, label.as_bytes(), context, len)
}

pub fn expand_with_label_bytes(secret: &[u8], label: &[u8], context: &[u8], len: usize) -> Vec<u8> {
    let info = build(|w| {
        w.u16(len as u16).vec(&mls_label_bytes(label)).vec(context);
    });
    expand(secret, &info, len)
}

pub fn derive_secret(secret: &[u8], label: &str) -> Vec<u8> {
    expand_with_label(secret, label, &[], NH)
}

/// RefHash (RFC 9420, 5.2).
pub fn ref_hash(label: &str, value: &[u8]) -> Vec<u8> {
    hash(&build(|w| {
        w.vec(label.as_bytes()).vec(value);
    }))
}

pub fn random(n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    // L'échec du générateur du système est impossible en pratique ; à défaut
    // la clé serait nulle, donc on refuse plutôt de continuer sans aléa.
    if rand::fill(&mut v).is_err() {
        v.clear();
    }
    v
}

pub fn random_secret() -> R<Vec<u8>> {
    let v = random(NH);
    if v.len() == NH { Ok(v) } else { Err(Error("aléa indisponible")) }
}

/// Comparaison en temps constant.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

// ---------------------------------------------------------------- signature

/// Clé de signature ECDSA P-256 (identité de la session, jamais écrite sur disque).
pub struct SigKey {
    kp: signature::EcdsaKeyPair,
}

impl SigKey {
    pub fn generate() -> R<Self> {
        let kp = signature::EcdsaKeyPair::generate(&signature::ECDSA_P256_SHA256_ASN1_SIGNING)
            .map_err(|_| Error("génération de la clé de signature"))?;
        Ok(SigKey { kp })
    }

    #[cfg(test)]
    pub fn from_raw(sk: &[u8], pk: &[u8]) -> R<Self> {
        let kp = signature::EcdsaKeyPair::from_private_key_and_public_key(&signature::ECDSA_P256_SHA256_ASN1_SIGNING, sk, pk)
            .map_err(|_| Error("clé de signature"))?;
        Ok(SigKey { kp })
    }

    /// Clé publique au format X9.62 non compressé (65 octets).
    pub fn public(&self) -> &[u8] {
        use signature::KeyPair;
        self.kp.public_key().as_ref()
    }

    pub fn sign(&self, msg: &[u8]) -> R<Vec<u8>> {
        let rng = rand::SystemRandom::new();
        self.kp.sign(&rng, msg).map(|s| s.as_ref().to_vec()).map_err(|_| Error("signature"))
    }

    pub fn sign_with_label(&self, label: &str, content: &[u8]) -> R<Vec<u8>> {
        self.sign(&sign_content(label, content))
    }
}

fn sign_content(label: &str, content: &[u8]) -> Vec<u8> {
    build(|w| {
        w.vec(&mls_label(label)).vec(content);
    })
}

pub fn verify_with_label(pk: &[u8], label: &str, content: &[u8], sig: &[u8]) -> bool {
    signature::UnparsedPublicKey::new(&signature::ECDSA_P256_SHA256_ASN1, pk)
        .verify(&sign_content(label, content), sig)
        .is_ok()
}

// ---------------------------------------------------------------- HPKE

const KEM_SUITE: &[u8] = b"KEM\x00\x10";
const HPKE_SUITE: &[u8] = b"HPKE\x00\x10\x00\x01\x00\x01";

/// Ordre de la courbe P-256, pour DeriveKeyPair.
const P256_ORDER: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbc, 0xe6, 0xfa, 0xad,
    0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];

fn labeled_extract(suite: &[u8], salt: &[u8], label: &[u8], ikm: &[u8]) -> Vec<u8> {
    let mut m = b"HPKE-v1".to_vec();
    m.extend_from_slice(suite);
    m.extend_from_slice(label);
    m.extend_from_slice(ikm);
    extract(salt, &m)
}

fn labeled_expand(suite: &[u8], prk: &[u8], label: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let mut m = (len as u16).to_be_bytes().to_vec();
    m.extend_from_slice(b"HPKE-v1");
    m.extend_from_slice(suite);
    m.extend_from_slice(label);
    m.extend_from_slice(info);
    expand(prk, &m, len)
}

/// Paire de clés HPKE (DHKEM P-256).
pub struct HpkeKey {
    sk: agreement::PrivateKey,
    pk: Vec<u8>,
}

impl HpkeKey {
    fn from_sk(sk: agreement::PrivateKey) -> R<Self> {
        let pk = sk.compute_public_key().map_err(|_| Error("clé HPKE"))?.as_ref().to_vec();
        Ok(HpkeKey { sk, pk })
    }

    pub fn generate() -> R<Self> {
        Self::from_sk(agreement::PrivateKey::generate(&agreement::ECDH_P256).map_err(|_| Error("clé HPKE"))?)
    }

    /// DeriveKeyPair de DHKEM(P-256) (RFC 9180, 7.1.3).
    pub fn derive(ikm: &[u8]) -> R<Self> {
        let prk = labeled_extract(KEM_SUITE, &[], b"dkp_prk", ikm);
        for counter in 0u8..=255 {
            let cand = labeled_expand(KEM_SUITE, &prk, b"candidate", &[counter], 32);
            if cand.iter().any(|&b| b != 0) && cand.as_slice() < P256_ORDER.as_slice() {
                let sk = agreement::PrivateKey::from_private_key(&agreement::ECDH_P256, &cand)
                    .map_err(|_| Error("clé HPKE dérivée"))?;
                return Self::from_sk(sk);
            }
        }
        Err(Error("DeriveKeyPair"))
    }

    pub fn public(&self) -> &[u8] {
        &self.pk
    }

    #[cfg(test)]
    pub fn from_scalar(sk: &[u8]) -> R<Self> {
        Self::from_sk(agreement::PrivateKey::from_private_key(&agreement::ECDH_P256, sk).map_err(|_| Error("clé HPKE"))?)
    }

    fn dh(&self, peer: &[u8]) -> R<Vec<u8>> {
        agreement::agree(
            &self.sk,
            agreement::UnparsedPublicKey::new(&agreement::ECDH_P256, peer),
            Error("échange DH"),
            |s| Ok(s.to_vec()),
        )
    }
}

fn kem_shared(dh: &[u8], enc: &[u8], pk_r: &[u8]) -> Vec<u8> {
    let mut ctx = enc.to_vec();
    ctx.extend_from_slice(pk_r);
    let prk = labeled_extract(KEM_SUITE, &[], b"eae_prk", dh);
    labeled_expand(KEM_SUITE, &prk, b"shared_secret", &ctx, NH)
}

fn hpke_key_nonce(shared: &[u8], info: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let psk_id_hash = labeled_extract(HPKE_SUITE, &[], b"psk_id_hash", &[]);
    let info_hash = labeled_extract(HPKE_SUITE, &[], b"info_hash", info);
    let mut ksc = vec![0u8];
    ksc.extend_from_slice(&psk_id_hash);
    ksc.extend_from_slice(&info_hash);
    let secret = labeled_extract(HPKE_SUITE, shared, b"secret", &[]);
    let key = labeled_expand(HPKE_SUITE, &secret, b"key", &ksc, KEY_LEN);
    let nonce = labeled_expand(HPKE_SUITE, &secret, b"base_nonce", &ksc, NONCE_LEN);
    (key, nonce)
}

/// SealBase à un seul message : rend (enc, chiffré).
pub fn hpke_seal(pk_r: &[u8], info: &[u8], aad: &[u8], pt: &[u8]) -> R<(Vec<u8>, Vec<u8>)> {
    let eph = HpkeKey::generate()?;
    let dh = eph.dh(pk_r)?;
    let enc = eph.pk.clone();
    let shared = kem_shared(&dh, &enc, pk_r);
    let (key, nonce) = hpke_key_nonce(&shared, info);
    Ok((enc, aead_seal(&key, &nonce, aad, pt)?))
}

pub fn hpke_open(sk: &HpkeKey, enc: &[u8], info: &[u8], aad: &[u8], ct: &[u8]) -> R<Vec<u8>> {
    let dh = sk.dh(enc)?;
    let shared = kem_shared(&dh, enc, &sk.pk);
    let (key, nonce) = hpke_key_nonce(&shared, info);
    aead_open(&key, &nonce, aad, ct)
}

fn encrypt_context(label: &str, context: &[u8]) -> Vec<u8> {
    build(|w| {
        w.vec(&mls_label(label)).vec(context);
    })
}

/// EncryptWithLabel (RFC 9420, 5.1.3) : rend un HPKECiphertext encodé.
pub fn encrypt_with_label(pk: &[u8], label: &str, context: &[u8], pt: &[u8]) -> R<Vec<u8>> {
    let (enc, ct) = hpke_seal(pk, &encrypt_context(label, context), &[], pt)?;
    Ok(build(|w| {
        w.vec(&enc).vec(&ct);
    }))
}

pub fn decrypt_with_label(sk: &HpkeKey, label: &str, context: &[u8], kem_output: &[u8], ct: &[u8]) -> R<Vec<u8>> {
    hpke_open(sk, kem_output, &encrypt_context(label, context), &[], ct)
}

// ---------------------------------------------------------------- AES-GCM

fn gcm_key(key: &[u8]) -> R<aead::LessSafeKey> {
    let k = aead::UnboundKey::new(&aead::AES_128_GCM, key).map_err(|_| Error("clé AES"))?;
    Ok(aead::LessSafeKey::new(k))
}

fn gcm_nonce(nonce: &[u8]) -> R<aead::Nonce> {
    aead::Nonce::try_assume_unique_for_key(nonce).map_err(|_| Error("nonce"))
}

pub fn aead_seal(key: &[u8], nonce: &[u8], aad: &[u8], pt: &[u8]) -> R<Vec<u8>> {
    let k = gcm_key(key)?;
    let mut buf = pt.to_vec();
    k.seal_in_place_append_tag(gcm_nonce(nonce)?, aead::Aad::from(aad), &mut buf)
        .map_err(|_| Error("chiffrement"))?;
    Ok(buf)
}

pub fn aead_open(key: &[u8], nonce: &[u8], aad: &[u8], ct: &[u8]) -> R<Vec<u8>> {
    let k = gcm_key(key)?;
    let mut buf = ct.to_vec();
    let n = k
        .open_in_place(gcm_nonce(nonce)?, aead::Aad::from(aad), &mut buf)
        .map_err(|_| Error("déchiffrement"))?
        .len();
    buf.truncate(n);
    Ok(buf)
}

/// AES-128-GCM à étiquette tronquée (8 octets) des trames DAVE. L'API AEAD
/// d'aws-lc n'accepte que l'étiquette entière : à la réception, on déchiffre
/// en CTR (compteur initial 2, comme GCM) puis on recalcule l'étiquette en
/// rechiffrant le clair, et on compare ses 8 premiers octets.
pub struct FrameCipher {
    gcm: aead::LessSafeKey,
    ctr: cipher::DecryptingKey,
}

pub const TAG_LEN: usize = 8;

impl FrameCipher {
    pub fn new(key: &[u8]) -> R<Self> {
        let unbound = cipher::UnboundCipherKey::new(&cipher::AES_128, key).map_err(|_| Error("clé AES"))?;
        Ok(FrameCipher {
            gcm: gcm_key(key)?,
            ctr: cipher::DecryptingKey::ctr(unbound).map_err(|_| Error("clé AES"))?,
        })
    }

    /// Chiffre `buf` sur place et rend l'étiquette tronquée.
    pub fn seal(&self, nonce: &[u8; NONCE_LEN], aad: &[u8], buf: &mut [u8]) -> R<[u8; TAG_LEN]> {
        let tag = self
            .gcm
            .seal_in_place_separate_tag(aead::Nonce::assume_unique_for_key(*nonce), aead::Aad::from(aad), buf)
            .map_err(|_| Error("chiffrement"))?;
        let mut t = [0u8; TAG_LEN];
        t.copy_from_slice(&tag.as_ref()[..TAG_LEN]);
        Ok(t)
    }

    /// Déchiffre `buf` sur place si l'étiquette tronquée est bonne ; sinon
    /// `buf` est laissé indéfini et une erreur est rendue.
    pub fn open(&self, nonce: &[u8; NONCE_LEN], aad: &[u8], buf: &mut [u8], tag: &[u8]) -> R<()> {
        let original = buf.to_vec();
        let mut iv = [0u8; 16];
        iv[..NONCE_LEN].copy_from_slice(nonce);
        iv[15] = 2;
        self.ctr
            .decrypt(buf, cipher::DecryptionContext::Iv128(iv.into()))
            .map_err(|_| Error("déchiffrement"))?;
        let mut check = buf.to_vec();
        let full = self
            .gcm
            .seal_in_place_separate_tag(aead::Nonce::assume_unique_for_key(*nonce), aead::Aad::from(aad), &mut check)
            .map_err(|_| Error("déchiffrement"))?;
        if tag.len() != TAG_LEN || !ct_eq(&full.as_ref()[..TAG_LEN], tag) || check != original {
            return Err(Error("étiquette invalide"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn hkdf_rfc5869_cas_1() {
        let ikm = vec![0x0b; 22];
        let salt = hex("000102030405060708090a0b0c");
        let info = hex("f0f1f2f3f4f5f6f7f8f9");
        let prk = extract(&salt, &ikm);
        assert_eq!(prk, hex("077709362c2e32df0ddc3f0dc47bba6390b6c73bb50f9c3122ec844ad7c2b3e5"));
        let okm = expand(&prk, &info, 42);
        assert_eq!(
            okm,
            hex("3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf34007208d5b887185865")
        );
    }

    /// Vecteur A.3.1 de la RFC 9180 (DHKEM(P-256), HKDF-SHA256, AES-128-GCM, mode de base).
    #[test]
    fn hpke_rfc9180_a3() {
        let ikm_r = hex("668b37171f1072f3cf12ea8a236a45df23fc13b82af3609ad1e354f6ef817550");
        let sk = HpkeKey::derive(&ikm_r).unwrap();
        assert_eq!(
            sk.public(),
            hex("04fe8c19ce0905191ebc298a9245792531f26f0cece2460639e8bc39cb7f706a826a779b4cf969b8a0e539c7f62fb3d30ad6aa8f80e30f1d128aafd68a2ce72ea0")
                .as_slice()
        );
        let enc = hex("04a92719c6195d5085104f469a8b9814d5838ff72b60501e2c4466e5e67b325ac98536d7b61a1af4b78e5b7f951c0900be863c403ce65c9bfcb9382657222d18c4");
        let info = hex("4f6465206f6e2061204772656369616e2055726e");
        let aad = hex("436f756e742d30");
        let ct = hex("5ad590bb8baa577f8619db35a36311226a896e7342a6d836d8b7bcd2f20b6c7f9076ac232e3ab2523f39513434");
        let pt = hpke_open(&sk, &enc, &info, &aad, &ct).unwrap();
        assert_eq!(pt, hex("4265617574792069732074727574682c20747275746820626561757479"));
        // Aller-retour avec une clé éphémère.
        let (e, c) = hpke_seal(sk.public(), b"i", b"a", b"bonjour").unwrap();
        assert_eq!(hpke_open(&sk, &e, b"i", b"a", &c).unwrap(), b"bonjour");
    }

    #[test]
    fn gcm_tronque_equivaut_a_gcm() {
        let key = [7u8; 16];
        let nonce = [0u8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0];
        let c = FrameCipher::new(&key).unwrap();
        let mut buf = b"trame opus de test".to_vec();
        let tag = c.seal(&nonce, b"", &mut buf).unwrap();
        let full = aead_seal(&key, &nonce, b"", b"trame opus de test").unwrap();
        assert_eq!(&full[..buf.len()], buf.as_slice());
        assert_eq!(&full[buf.len()..buf.len() + 8], &tag);
        c.open(&nonce, b"", &mut buf, &tag).unwrap();
        assert_eq!(buf, b"trame opus de test");
        let mut bad = full[..18].to_vec();
        bad[3] ^= 1;
        assert!(c.open(&nonce, b"", &mut bad, &tag).is_err());
    }

    #[test]
    fn signature_avec_etiquette() {
        let k = SigKey::generate().unwrap();
        assert_eq!(k.public().len(), 65);
        let s = k.sign_with_label("LeafNodeTBS", b"abc").unwrap();
        assert!(verify_with_label(k.public(), "LeafNodeTBS", b"abc", &s));
        assert!(!verify_with_label(k.public(), "KeyPackageTBS", b"abc", &s));
    }
}
