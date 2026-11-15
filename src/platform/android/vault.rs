//! Coffre à jetons d'Android : une clé AES-256 créée dans le Keystore
//! d'Android (elle ne quitte jamais le système, matériel sécurisé s'il y en
//! a), qui chiffre chaque jeton en AES-GCM. Le résultat est écrit dans le
//! stockage privé de l'application, un fichier par compte :
//! `files/ferrocord/coffre/<compte>.bin` = `FCV1`, longueur du vecteur
//! d'initialisation, vecteur, texte chiffré. Rien n'y est en clair.
//!
//! Les appels bloquent (Keystore, disque) : ils ne viennent que du fil des
//! comptes. Aucune erreur ne contient de donnée lue.

use super::jvm::with_activity;
use jni::objects::{JByteArray, JObject, JValue};
use jni::{jni_sig, jni_str, Env};
use std::path::PathBuf;

/// Nom de la clé dans le Keystore.
const ALIAS: &str = "ferrocord-coffre";
const MAGIC: &[u8; 4] = b"FCV1";
/// Taille de l'étiquette d'authentification GCM, en bits.
const TAG_BITS: i32 = 128;
/// Un fichier plus gros n'est pas un jeton chiffré : refusé sans être lu.
const MAX_FILE: u64 = 64 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Keystore absent, clé impossible à créer ou à utiliser.
    Keystore,
    /// Lecture ou écriture du fichier ratée.
    File,
    /// Fichier abîmé, ou chiffré par une clé disparue (application réinstallée).
    Unreadable,
}

fn dir() -> Option<PathBuf> {
    Some(super::files_dir()?.join("ferrocord").join("coffre"))
}

fn path(account: &str) -> Result<PathBuf, Error> {
    // Les clés de compte sont `importe` ou des chiffres (AccountKey::parse) :
    // jamais de séparateur, mais on le vérifie pour ne sortir du dossier en aucun cas.
    if account.is_empty() || !account.bytes().all(|b| b.is_ascii_alphanumeric()) {
        return Err(Error::File);
    }
    Ok(dir().ok_or(Error::File)?.join(format!("{account}.bin")))
}

pub fn get(account: &str) -> Result<Option<String>, Error> {
    let path = path(account)?;
    let meta = match std::fs::metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::File),
    };
    if !meta.is_file() || meta.len() > MAX_FILE {
        return Err(Error::Unreadable);
    }
    let bytes = std::fs::read(&path).map_err(|_| Error::File)?;
    let (iv, sealed) = split(&bytes).ok_or(Error::Unreadable)?;
    let plain = with_activity(|env, _| decrypt(env, iv, sealed)).ok_or(Error::Keystore)?.ok_or(Error::Unreadable)?;
    String::from_utf8(plain).map(Some).map_err(|_| Error::Unreadable)
}

pub fn set(account: &str, token: &str) -> Result<(), Error> {
    let path = path(account)?;
    let (iv, sealed) = with_activity(|env, _| encrypt(env, token.as_bytes())).ok_or(Error::Keystore)?;
    let iv_len = u8::try_from(iv.len()).map_err(|_| Error::Keystore)?;
    let mut out = Vec::with_capacity(5 + iv.len() + sealed.len());
    out.extend_from_slice(MAGIC);
    out.push(iv_len);
    out.extend_from_slice(&iv);
    out.extend_from_slice(&sealed);
    write_atomic(&path, &out).map_err(|_| Error::File)
}

pub fn delete(account: &str) -> Result<(), Error> {
    match std::fs::remove_file(path(account)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::File),
    }
}

/// `FCV1`, longueur, vecteur, chiffré : `(vecteur, chiffré)`.
fn split(bytes: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = bytes.strip_prefix(MAGIC)?;
    let (&len, rest) = rest.split_first()?;
    let len = usize::from(len);
    (len > 0 && rest.len() > len).then(|| rest.split_at(len))
}

fn write_atomic(path: &std::path::Path, data: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d)?;
    }
    let tmp = path.with_extension("bin.tmp");
    let written = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()
    })();
    let result = written.and_then(|()| std::fs::rename(&tmp, path));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// La clé du Keystore, créée au premier besoin.
fn key<'l>(env: &mut Env<'l>) -> jni::errors::Result<JObject<'l>> {
    let provider = env.new_string("AndroidKeyStore")?;
    let ks = env
        .call_static_method(jni_str!("java/security/KeyStore"), jni_str!("getInstance"), jni_sig!("(Ljava/lang/String;)Ljava/security/KeyStore;"), &[JValue::Object(&provider)])?
        .l()?;
    let none = JObject::null();
    env.call_method(&ks, jni_str!("load"), jni_sig!("(Ljava/security/KeyStore$LoadStoreParameter;)V"), &[JValue::Object(&none)])?;
    let alias = env.new_string(ALIAS)?;
    let found = env
        .call_method(&ks, jni_str!("getKey"), jni_sig!("(Ljava/lang/String;[C)Ljava/security/Key;"), &[JValue::Object(&alias), JValue::Object(&none)])?
        .l()?;
    if !found.is_null() {
        return Ok(found);
    }
    // KeyProperties.PURPOSE_ENCRYPT | PURPOSE_DECRYPT.
    let builder = env.new_object(
        jni_str!("android/security/keystore/KeyGenParameterSpec$Builder"),
        jni_sig!("(Ljava/lang/String;I)V"),
        &[JValue::Object(&alias), JValue::Int(1 | 2)],
    )?;
    let string_class = env.find_class(jni_str!("java/lang/String"))?;
    let gcm = env.new_string("GCM")?;
    let modes = env.new_object_array(1, &string_class, &gcm)?;
    env.call_method(&builder, jni_str!("setBlockModes"), jni_sig!("([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;"), &[JValue::Object(&modes)])?;
    let no_padding = env.new_string("NoPadding")?;
    let paddings = env.new_object_array(1, &string_class, &no_padding)?;
    env.call_method(
        &builder,
        jni_str!("setEncryptionPaddings"),
        jni_sig!("([Ljava/lang/String;)Landroid/security/keystore/KeyGenParameterSpec$Builder;"),
        &[JValue::Object(&paddings)],
    )?;
    env.call_method(&builder, jni_str!("setKeySize"), jni_sig!("(I)Landroid/security/keystore/KeyGenParameterSpec$Builder;"), &[JValue::Int(256)])?;
    let spec = env.call_method(&builder, jni_str!("build"), jni_sig!("()Landroid/security/keystore/KeyGenParameterSpec;"), &[])?.l()?;
    let aes = env.new_string("AES")?;
    let generator = env
        .call_static_method(
            jni_str!("javax/crypto/KeyGenerator"),
            jni_str!("getInstance"),
            jni_sig!("(Ljava/lang/String;Ljava/lang/String;)Ljavax/crypto/KeyGenerator;"),
            &[JValue::Object(&aes), JValue::Object(&provider)],
        )?
        .l()?;
    env.call_method(&generator, jni_str!("init"), jni_sig!("(Ljava/security/spec/AlgorithmParameterSpec;)V"), &[JValue::Object(&spec)])?;
    env.call_method(&generator, jni_str!("generateKey"), jni_sig!("()Ljavax/crypto/SecretKey;"), &[])?.l()
}

fn cipher<'l>(env: &mut Env<'l>) -> jni::errors::Result<JObject<'l>> {
    let name = env.new_string("AES/GCM/NoPadding")?;
    env.call_static_method(jni_str!("javax/crypto/Cipher"), jni_str!("getInstance"), jni_sig!("(Ljava/lang/String;)Ljavax/crypto/Cipher;"), &[JValue::Object(&name)])?.l()
}

fn bytes_of(env: &mut Env, obj: JObject) -> jni::errors::Result<Vec<u8>> {
    let array = env.cast_local::<JByteArray>(obj)?;
    env.convert_byte_array(&array)
}

/// `(vecteur, chiffré)` ; le vecteur est tiré par le Keystore (jamais réutilisé).
fn encrypt(env: &mut Env, plain: &[u8]) -> jni::errors::Result<(Vec<u8>, Vec<u8>)> {
    let key = key(env)?;
    let c = cipher(env)?;
    // Cipher.ENCRYPT_MODE.
    env.call_method(&c, jni_str!("init"), jni_sig!("(ILjava/security/Key;)V"), &[JValue::Int(1), JValue::Object(&key)])?;
    let input = env.byte_array_from_slice(plain)?;
    let sealed = env.call_method(&c, jni_str!("doFinal"), jni_sig!("([B)[B"), &[JValue::Object(&input)])?.l()?;
    let sealed = bytes_of(env, sealed)?;
    let iv = env.call_method(&c, jni_str!("getIV"), jni_sig!("()[B"), &[])?.l()?;
    Ok((bytes_of(env, iv)?, sealed))
}

/// `Ok(None)` : déchiffrement refusé (fichier abîmé, ou autre clé).
fn decrypt(env: &mut Env, iv: &[u8], sealed: &[u8]) -> jni::errors::Result<Option<Vec<u8>>> {
    let key = key(env)?;
    let c = cipher(env)?;
    let iv = env.byte_array_from_slice(iv)?;
    let spec = env.new_object(jni_str!("javax/crypto/spec/GCMParameterSpec"), jni_sig!("(I[B)V"), &[JValue::Int(TAG_BITS), JValue::Object(&iv)])?;
    // Cipher.DECRYPT_MODE.
    env.call_method(&c, jni_str!("init"), jni_sig!("(ILjava/security/Key;Ljava/security/spec/AlgorithmParameterSpec;)V"), &[JValue::Int(2), JValue::Object(&key), JValue::Object(&spec)])?;
    let input = env.byte_array_from_slice(sealed)?;
    match env.call_method(&c, jni_str!("doFinal"), jni_sig!("([B)[B"), &[JValue::Object(&input)]) {
        Ok(v) => Ok(Some(bytes_of(env, v.l()?)?)),
        // AEADBadTagException : effacée ici, jamais rendue à Java.
        Err(jni::errors::Error::JavaException) => {
            env.exception_clear();
            Ok(None)
        }
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_layout_is_checked() {
        assert_eq!(split(b"FCV1\x02abcd"), Some((&b"ab"[..], &b"cd"[..])));
        assert_eq!(split(b"FCV1\x02ab"), None, "aucun chiffré");
        assert_eq!(split(b"FCV1\x00abcd"), None);
        assert_eq!(split(b"XXXX\x02abcd"), None);
        assert!(path("../x").is_err());
        assert!(path("").is_err());
    }
}
