//! Manifeste de mise à jour signé (Ed25519, aws-lc-rs).
//!
//! Format (JSON) : une enveloppe dont `manifeste` est le texte exact signé,
//! et `signature` la signature Ed25519 de ces octets, en hexadécimal. Pas de
//! mise en forme canonique à reproduire : on vérifie les octets reçus, puis
//! on les lit.
//!
//! ```json
//! {"manifeste": "{\"produit\":\"ferrocord\",\"canal\":\"stable\",\"version\":\"0.2.0\",\"url\":\"https://…/ferrocord.exe\",\"sha256\":\"…\",\"taille\":123,\"notes\":\"…\"}",
//!  "signature": "…128 chiffres hexadécimaux…"}
//! ```
//!
//! `canal` absent vaut « stable » (manifestes d'avant ce champ) ; un
//! manifeste d'un autre canal que celui de l'exécutable est refusé.

use aws_lc_rs::signature::{UnparsedPublicKey, ED25519};
use serde::Deserialize;

pub use super::source::CHANNEL;

pub const PRODUCT: &str = "ferrocord";
/// Manifeste plus gros : refusé avant toute lecture.
pub const MAX_MANIFEST: usize = 64 * 1024;
/// Exécutable plus gros : refusé (l'actuel fait une vingtaine de Mo).
pub const MAX_BINARY: u64 = 256 * 1024 * 1024;

/// Version `majeur.mineur.correctif`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    pub fn parse(s: &str) -> Option<Version> {
        let mut it = s.trim().trim_start_matches('v').split('.');
        let mut n = || it.next().and_then(|p| if !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()) { p.parse().ok() } else { None });
        let v = Version(n()?, n()?, n()?);
        it.next().is_none().then_some(v)
    }

    /// Version de cet exécutable.
    pub fn current() -> Version {
        Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// Manifeste vérifié.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Manifest {
    pub version: Version,
    pub url: String,
    pub sha256: [u8; 32],
    pub size: u64,
    pub notes: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// Trop gros, pas du JSON, champ manquant ou illisible.
    Format(&'static str),
    /// Signature fausse : manifeste altéré ou signé par une autre clé.
    Signature,
    /// Signé, mais pour un autre produit.
    Product,
    /// Signé, mais pour un autre canal que celui de cet exécutable.
    Channel,
}

impl std::fmt::Display for ManifestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ManifestError::Format(what) => write!(f, "manifeste illisible ({what})"),
            ManifestError::Signature => f.write_str("signature invalide"),
            ManifestError::Product => f.write_str("manifeste d'un autre produit"),
            ManifestError::Channel => f.write_str("manifeste d'un autre canal"),
        }
    }
}

#[derive(Deserialize)]
struct Envelope {
    manifeste: String,
    signature: String,
}

#[derive(Deserialize)]
struct Payload {
    produit: String,
    #[serde(default = "stable")]
    canal: String,
    version: String,
    url: String,
    sha256: String,
    taille: u64,
    #[serde(default)]
    notes: String,
}

fn stable() -> String {
    CHANNEL.to_string()
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.trim();
    if s.len() != 2 * N || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; N];
    for (i, slot) in out.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

/// Clé publique intégrée au binaire (`cle_publique.txt`, 64 chiffres
/// hexadécimaux, lignes `#` ignorées) ; `None` : mises à jour désactivées.
pub fn embedded_key() -> Option<[u8; 32]> {
    parse_key(include_str!("cle_publique.txt"))
}

pub fn parse_key(text: &str) -> Option<[u8; 32]> {
    let line = text.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    unhex(line)
}

/// Vérifie la signature de `bytes` avec `key`, puis lit le manifeste.
pub fn verify(bytes: &[u8], key: &[u8; 32]) -> Result<Manifest, ManifestError> {
    if bytes.len() > MAX_MANIFEST {
        return Err(ManifestError::Format("trop gros"));
    }
    let env: Envelope = serde_json::from_slice(bytes).map_err(|_| ManifestError::Format("enveloppe"))?;
    let sig: [u8; 64] = unhex(&env.signature).ok_or(ManifestError::Format("signature"))?;
    UnparsedPublicKey::new(&ED25519, key).verify(env.manifeste.as_bytes(), &sig).map_err(|_| ManifestError::Signature)?;
    let p: Payload = serde_json::from_str(&env.manifeste).map_err(|_| ManifestError::Format("contenu"))?;
    if p.produit != PRODUCT {
        return Err(ManifestError::Product);
    }
    if p.canal != CHANNEL {
        return Err(ManifestError::Channel);
    }
    let version = Version::parse(&p.version).ok_or(ManifestError::Format("version"))?;
    let sha256 = unhex(&p.sha256).ok_or(ManifestError::Format("sha256"))?;
    if !(p.url.starts_with("https://") || super::fetch::loopback(&p.url)) {
        return Err(ManifestError::Format("adresse"));
    }
    if p.taille == 0 || p.taille > MAX_BINARY {
        return Err(ManifestError::Format("taille"));
    }
    Ok(Manifest { version, url: p.url, sha256, size: p.taille, notes: p.notes.chars().take(2000).collect() })
}

/// Texte du manifeste à signer (utilisé par `tools/signer` et les tests).
pub fn payload_json(version: &str, url: &str, sha256: &[u8; 32], size: u64, notes: &str) -> String {
    payload_json_on(CHANNEL, version, url, sha256, size, notes)
}

/// Comme [`payload_json`], pour le canal `channel`.
pub fn payload_json_on(channel: &str, version: &str, url: &str, sha256: &[u8; 32], size: u64, notes: &str) -> String {
    serde_json::json!({ "produit": PRODUCT, "canal": channel, "version": version, "url": url, "sha256": hex(sha256), "taille": size, "notes": notes }).to_string()
}

/// Enveloppe finale autour d'un manifeste signé.
pub fn envelope_json(payload: &str, signature: &[u8]) -> String {
    serde_json::json!({ "manifeste": payload, "signature": hex(signature) }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_lc_rs::rand::SystemRandom;
    use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair};

    fn keypair() -> Ed25519KeyPair {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()
    }

    fn signed(k: &Ed25519KeyPair, payload: &str) -> Vec<u8> {
        envelope_json(payload, k.sign(payload.as_bytes()).as_ref()).into_bytes()
    }

    /// Vecteur 1 de la RFC 8032 (section 7.1) : message vide.
    #[test]
    fn rfc8032_vector() {
        let pk: [u8; 32] = unhex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a").unwrap();
        let sig: [u8; 64] = unhex("e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b").unwrap();
        assert!(UnparsedPublicKey::new(&ED25519, &pk).verify(b"", &sig).is_ok());
        let mut bad = sig;
        bad[0] ^= 1;
        assert!(UnparsedPublicKey::new(&ED25519, &pk).verify(b"", &bad).is_err());
    }

    /// Manifeste écrit par `tools/signer` (lancé par `tools/publier.ps1`) avec
    /// une clé d'essai aussitôt détruite : l'outil et l'application
    /// s'entendent sur le format, canal compris.
    #[test]
    fn manifest_from_the_signing_tool_is_accepted() {
        let pk: [u8; 32] = unhex("edcb5832ae2a96c7863fc18fbfb0ec1b863dbb1fd9c59613e8f91520044fb97c").unwrap();
        let file = r#"{"manifeste":"{\"canal\":\"stable\",\"notes\":\"Corrections\",\"produit\":\"ferrocord\",\"sha256\":\"faedd276256c1cd1e84a9c1c62bf659f934f19b581f772bbee429e8c4631ac46\",\"taille\":43,\"url\":\"https://github.com/essai/ferrocord/releases/download/v9.9.9/Ferrocord-9.9.9-x64.exe\",\"version\":\"9.9.9\"}","signature":"790b03457774ee6910511cc92a4ff0bddaf58521a8caf39fafdd3428cf593f59e8fb01ccf443f0cbf0e3f0b648a71b2b5bc845a025404fb103387d4e7ee0da07"}"#;
        let m = verify(file.as_bytes(), &pk).unwrap();
        assert_eq!((m.version, m.size, m.notes.as_str()), (Version(9, 9, 9), 43, "Corrections"));
        assert_eq!(m.url, super::super::source::binary_url("essai/ferrocord", "9.9.9"));
        assert_eq!(hex(&m.sha256), "faedd276256c1cd1e84a9c1c62bf659f934f19b581f772bbee429e8c4631ac46");
        assert_eq!(verify(file.replace("9.9.9", "9.9.8").as_bytes(), &pk), Err(ManifestError::Signature));
    }

    #[test]
    fn good_manifest_is_read() {
        let k = keypair();
        let pk: [u8; 32] = k.public_key().as_ref().try_into().unwrap();
        let payload = payload_json("0.2.0", "https://exemple.invalid/ferrocord.exe", &[7; 32], 1234, "Corrections");
        let m = verify(&signed(&k, &payload), &pk).unwrap();
        assert_eq!(m, Manifest { version: Version(0, 2, 0), url: "https://exemple.invalid/ferrocord.exe".into(), sha256: [7; 32], size: 1234, notes: "Corrections".into() });
    }

    #[test]
    fn altered_or_foreign_manifests_are_refused() {
        let k = keypair();
        let pk: [u8; 32] = k.public_key().as_ref().try_into().unwrap();
        let payload = payload_json("0.2.0", "https://exemple.invalid/ferrocord.exe", &[7; 32], 1234, "");
        // Manifeste altéré après signature (autre adresse).
        let text = String::from_utf8(signed(&k, &payload)).unwrap().replace("exemple.invalid", "pirate.invalid");
        assert_eq!(verify(text.as_bytes(), &pk), Err(ManifestError::Signature));
        // Signé par une autre clé.
        assert_eq!(verify(&signed(&keypair(), &payload), &pk), Err(ManifestError::Signature));
        // Bien signé, mais pas pour Ferrocord, ou adresse en clair.
        let other = payload.replace("\"ferrocord\"", "\"autre\"");
        assert_eq!(verify(&signed(&k, &other), &pk), Err(ManifestError::Product));
        // Bien signé, mais pour un autre canal ; sans canal : « stable ».
        let beta = payload_json_on("beta", "0.2.0", "https://exemple.invalid/ferrocord.exe", &[7; 32], 1234, "");
        assert_eq!(verify(&signed(&k, &beta), &pk), Err(ManifestError::Channel));
        let legacy = payload.replace("\"canal\":\"stable\",", "");
        assert!(!legacy.contains("canal"));
        assert!(verify(&signed(&k, &legacy), &pk).is_ok());
        let http = payload_json("0.2.0", "http://exemple.invalid/f.exe", &[7; 32], 1234, "");
        assert_eq!(verify(&signed(&k, &http), &pk), Err(ManifestError::Format("adresse")));
        for junk in [&b"pas du json"[..], b"{}", b"{\"manifeste\":\"x\",\"signature\":\"zz\"}"] {
            assert!(matches!(verify(junk, &pk), Err(ManifestError::Format(_))));
        }
        assert_eq!(verify(&vec![b' '; MAX_MANIFEST + 1], &pk), Err(ManifestError::Format("trop gros")));
    }

    #[test]
    fn versions_and_keys() {
        assert!(Version::parse("0.10.0") > Version::parse("0.9.9"));
        assert_eq!(Version::parse("v1.2.3"), Some(Version(1, 2, 3)));
        for bad in ["1.2", "1.2.3.4", "1.x.3", "", "1.2.-3"] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert_eq!(parse_key("# commentaire\n\n"), None);
        assert_eq!(parse_key(&format!("# clé\n{}\n", "ab".repeat(32))), Some([0xab; 32]));
        assert_eq!(parse_key("abc"), None);
    }
}
