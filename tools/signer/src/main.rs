//! Clé et signature des manifestes de mise à jour de Ferrocord.
//!
//! ```text
//! ferrocord-signer cle --privee <fichier hors du dépôt> --publique <src/install/update/cle_publique.txt>
//! ferrocord-signer publique --privee <fichier>
//! ferrocord-signer signe --privee <fichier> --exe <ferrocord.exe> --version 0.2.0 --url https://…/ferrocord.exe [--notes "…" | --notes-fichier <fichier>] [--canal stable] --sortie manifeste.json
//! ```
//!
//! `publique` n'affiche que la clé publique (64 chiffres hexadécimaux) : pour
//! vérifier qu'une clé privée est bien celle dont l'exécutable intègre la clé
//! publique. `tools/publier.ps1` enchaîne tout cela.
//!
//! La clé privée (PKCS#8) ne doit jamais entrer dans un dépôt : `cle` refuse
//! de l'écrire dans un dossier suivi par git, et de remplacer une clé
//! existante. Format du manifeste : `src/install/update/manifest.rs`.

use aws_lc_rs::digest;
use aws_lc_rs::rand::SystemRandom;
use aws_lc_rs::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn options(args: &[String]) -> Result<HashMap<String, String>, String> {
    let mut out = HashMap::new();
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let key = k.strip_prefix("--").ok_or_else(|| format!("option attendue : {k}"))?;
        let v = it.next().ok_or_else(|| format!("valeur manquante après {k}"))?;
        out.insert(key.to_string(), v.clone());
    }
    Ok(out)
}

fn need<'a>(o: &'a HashMap<String, String>, k: &str) -> Result<&'a str, String> {
    o.get(k).map(String::as_str).ok_or_else(|| format!("--{k} manquant"))
}

/// Dossier suivi par git (le fichier ou l'un de ses parents contient `.git`).
fn in_repository(path: &Path) -> bool {
    let abs = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    abs.ancestors().skip(1).any(|d| d.join(".git").exists())
}

fn key(o: &HashMap<String, String>) -> Result<(), String> {
    let private = PathBuf::from(need(o, "privee")?);
    let public = PathBuf::from(need(o, "publique")?);
    if in_repository(&private) {
        return Err(format!("{} est dans un dépôt git : la clé privée doit rester ailleurs (voir README)", private.display()));
    }
    if private.exists() {
        return Err(format!("{} existe déjà : rien n'est remplacé", private.display()));
    }
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).map_err(|_| "génération impossible")?;
    let pair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).map_err(|_| "clé générée illisible")?;
    std::fs::write(&private, pkcs8.as_ref()).map_err(|e| format!("{} : {e}", private.display()))?;
    let text = format!(
        "# Clé publique Ed25519 des mises à jour de Ferrocord (64 chiffres hexadécimaux\n# sur une ligne). Écrite ici par `tools/signer cle` ; la clé privée, elle, ne\n# doit jamais entrer dans le dépôt (voir docs/distribution.md).\n# Sans clé, les mises à jour automatiques sont désactivées.\n{}\n",
        hex(pair.public_key().as_ref())
    );
    std::fs::write(&public, text).map_err(|e| format!("{} : {e}", public.display()))?;
    println!("Clé privée écrite dans : {}", private.display());
    println!("Clé publique : {} ({})", public.display(), hex(pair.public_key().as_ref()));
    Ok(())
}

/// Clé publique d'une clé privée (la clé privée n'est jamais affichée).
fn public(o: &HashMap<String, String>) -> Result<(), String> {
    let pkcs8 = std::fs::read(need(o, "privee")?).map_err(|e| format!("clé privée : {e}"))?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| "clé privée illisible (PKCS#8 Ed25519 attendu)")?;
    println!("{}", hex(pair.public_key().as_ref()));
    Ok(())
}

fn sign(o: &HashMap<String, String>) -> Result<(), String> {
    let pkcs8 = std::fs::read(need(o, "privee")?).map_err(|e| format!("clé privée : {e}"))?;
    let pair = Ed25519KeyPair::from_pkcs8(&pkcs8).map_err(|_| "clé privée illisible (PKCS#8 Ed25519 attendu)")?;
    let exe = std::fs::read(need(o, "exe")?).map_err(|e| format!("exécutable : {e}"))?;
    let version = need(o, "version")?;
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit())) {
        return Err("version attendue : majeur.mineur.correctif".into());
    }
    let url = need(o, "url")?;
    if !url.starts_with("https://") {
        return Err("l'adresse doit commencer par https://".into());
    }
    let channel = o.get("canal").map_or("stable", String::as_str);
    if channel.is_empty() || !channel.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') {
        return Err("canal attendu : lettres minuscules, chiffres, tirets".into());
    }
    let notes = match o.get("notes-fichier") {
        Some(path) => std::fs::read_to_string(path).map_err(|e| format!("{path} : {e}"))?.trim_start_matches('\u{feff}').trim().to_string(),
        None => o.get("notes").cloned().unwrap_or_default(),
    };
    let sha = digest::digest(&digest::SHA256, &exe);
    let payload = serde_json::json!({
        "produit": "ferrocord",
        "canal": channel,
        "version": version,
        "url": url,
        "sha256": hex(sha.as_ref()),
        "taille": exe.len(),
        "notes": notes,
    })
    .to_string();
    let signature = pair.sign(payload.as_bytes());
    UnparsedPublicKey::new(&ED25519, pair.public_key().as_ref()).verify(payload.as_bytes(), signature.as_ref()).map_err(|_| "signature non vérifiable")?;
    let envelope = serde_json::json!({ "manifeste": payload, "signature": hex(signature.as_ref()) }).to_string();
    let out = need(o, "sortie")?;
    std::fs::write(out, envelope).map_err(|e| format!("{out} : {e}"))?;
    println!("Manifeste signé : {out} (version {version}, {} octets, SHA-256 {})", exe.len(), hex(sha.as_ref()));
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.split_first() {
        Some((cmd, rest)) if cmd == "cle" => options(rest).and_then(|o| key(&o)),
        Some((cmd, rest)) if cmd == "publique" => options(rest).and_then(|o| public(&o)),
        Some((cmd, rest)) if cmd == "signe" => options(rest).and_then(|o| sign(&o)),
        _ => Err("usage : ferrocord-signer cle --privee <fichier> --publique <cle_publique.txt>\n        ferrocord-signer signe --privee <fichier> --exe <ferrocord.exe> --version x.y.z --url https://… [--notes … | --notes-fichier <fichier>] [--canal stable] --sortie <manifeste.json>\n        ferrocord-signer publique --privee <fichier>".into()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
