//! Protection de la clé de l'historique par DPAPI (`CryptProtectData`) :
//! seul le même utilisateur Windows, sur cette machine, peut la relire.
//! Aucune interface sûre n'existe dans les crates déjà présentes : tout
//! l'`unsafe` du lot H est ici, limité à ces deux appels et à la libération
//! de leur tampon.

use windows::Win32::Foundation::{HLOCAL, LocalFree};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};

/// Copie le tampon rendu par DPAPI puis le libère.
fn take(out: CRYPT_INTEGER_BLOB) -> Vec<u8> {
    if out.pbData.is_null() {
        return Vec::new();
    }
    // SAFETY: en cas de succès, DPAPI rend dans `out` un tampon de
    // `cbData` octets alloué par LocalAlloc, lisible jusqu'à LocalFree.
    let bytes = unsafe { std::slice::from_raw_parts(out.pbData, out.cbData as usize) }.to_vec();
    // SAFETY: le tampon vient de DPAPI (LocalAlloc), fait `cbData` octets et
    // n'est plus lu après ; mis à zéro d'abord (il a pu porter la clé en clair).
    unsafe {
        std::ptr::write_bytes(out.pbData, 0, out.cbData as usize);
        LocalFree(Some(HLOCAL(out.pbData.cast())));
    }
    bytes
}

/// Chiffre `data` pour l'utilisateur Windows courant.
pub fn protect(data: &[u8]) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr().cast_mut() };
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: `input` pointe sur `data`, vivant pendant l'appel et seulement
    // lu par DPAPI ; `out` est un emplacement valide que DPAPI remplit.
    // Aucune fenêtre (UI_FORBIDDEN) : l'appel ne bloque jamais sur une invite.
    unsafe { CryptProtectData(&input, windows::core::w!("Ferrocord"), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.ok()?;
    Some(take(out))
}

/// Déchiffre ce que [`protect`] a rendu ; autre utilisateur ou abîmé : `None`.
pub fn unprotect(data: &[u8]) -> Option<Vec<u8>> {
    let input = CRYPT_INTEGER_BLOB { cbData: u32::try_from(data.len()).ok()?, pbData: data.as_ptr().cast_mut() };
    let mut out = CRYPT_INTEGER_BLOB::default();
    // SAFETY: comme pour `protect` ; la description n'est pas demandée.
    unsafe { CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out) }.ok()?;
    Some(take(out))
}

#[cfg(test)]
mod tests {
    #[test]
    fn protects_for_this_user_only() {
        let secret = [7u8; 32];
        let sealed = super::protect(&secret).unwrap();
        assert!(!sealed.windows(32).any(|w| w == secret));
        assert_eq!(super::unprotect(&sealed).as_deref(), Some(&secret[..]));
        let mut bad = sealed.clone();
        let mid = bad.len() / 2;
        bad[mid] ^= 0xff;
        assert_eq!(super::unprotect(&bad), None);
    }
}
