//! VP8 : lecture de l'en-tête d'image (RFC 6386, 9.1), sans décodage.
//!
//! Le codec lui-même est celui de Windows (`mf.rs`) : les « Extensions
//! vidéo VP9 » (Microsoft, installées d'office sur Windows 10 et 11, et
//! réinstallables depuis le Microsoft Store) contiennent un décodeur et un
//! encodeur VP8 pour Media Foundation. Aucun octet ajouté à l'exécutable,
//! contre environ 1 Mio pour libvpx (qu'il faudrait aussi compiler avec
//! yasm et le script `configure`, absents ici). Sans ces extensions, VP8
//! n'est pas annoncé à la passerelle et l'appel reste en H.264.

/// Taille d'une image clé VP8 (largeur, hauteur), `None` pour une image
/// intermédiaire ou un en-tête illisible. Image clé : bit P à 0, code de
/// départ `9d 01 2a`, puis largeur et hauteur sur 14 bits (les 2 bits hauts
/// sont l'agrandissement, ignoré ici).
pub fn keyframe_size(d: &[u8]) -> Option<(u32, u32)> {
    if d.len() < 10 || d[0] & 1 != 0 || d[3..6] != [0x9d, 0x01, 0x2a] {
        return None;
    }
    let w = u32::from(u16::from_le_bytes([d[6], d[7]]) & 0x3fff);
    let h = u32::from(u16::from_le_bytes([d[8], d[9]]) & 0x3fff);
    (w > 0 && h > 0).then_some((w, h))
}

/// L'image est à montrer (bit `show_frame`) ; une image cachée (référence
/// de rechange) ne donne aucune sortie.
pub fn is_shown(d: &[u8]) -> bool {
    d.first().is_some_and(|b| b & 0x10 != 0)
}

/// Taille d'une image clé VP9 (largeur, hauteur), lue dans son en-tête non
/// compressé (spécification VP9, 6.2) ; `None` pour une image intermédiaire
/// ou un en-tête illisible.
pub fn vp9_keyframe_size(d: &[u8]) -> Option<(u32, u32)> {
    let mut at = 0usize;
    let mut bits = |n: usize| -> Option<u32> {
        let mut v = 0;
        for _ in 0..n {
            v = (v << 1) | u32::from((*d.get(at / 8)? >> (7 - at % 8)) & 1);
            at += 1;
        }
        Some(v)
    };
    if bits(2)? != 2 {
        return None;
    }
    let profile = bits(1)? | (bits(1)? << 1);
    if profile == 3 {
        bits(1)?;
    }
    // Image déjà décodée remontrée, ou image intermédiaire : pas une image clé.
    if bits(1)? != 0 || bits(1)? != 0 {
        return None;
    }
    bits(2)?;
    if bits(24)? != 0x49_8342 {
        return None;
    }
    if profile >= 2 {
        bits(1)?;
    }
    // Espace de couleurs 7 (sRGB) : pas d'étendue ; profils 1 et 3 : sous-échantillonnage.
    let srgb = bits(3)? == 7;
    if !srgb {
        bits(1)?;
    }
    if profile & 1 == 1 {
        bits(if srgb { 1 } else { 3 })?;
    }
    let (w, h) = (bits(16)? + 1, bits(16)? + 1);
    Some((w, h))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn en_tete_image_cle() {
        // Image clé 320×240, montrée, partition de 100 octets.
        let key = [0x50, 0x0c, 0x00, 0x9d, 0x01, 0x2a, 0x40, 0x01, 0xf0, 0x00, 0xaa];
        assert_eq!(keyframe_size(&key), Some((320, 240)));
        assert!(is_shown(&key));
        // Bits d'agrandissement ignorés.
        let mut scaled = key;
        scaled[7] |= 0xc0;
        assert_eq!(keyframe_size(&scaled), Some((320, 240)));
        // Image intermédiaire, code de départ faux, trop court, taille nulle.
        let mut inter = key;
        inter[0] |= 1;
        assert_eq!(keyframe_size(&inter), None);
        let mut bad = key;
        bad[4] = 0;
        assert_eq!(keyframe_size(&bad), None);
        assert_eq!(keyframe_size(&key[..9]), None);
        let mut zero = key;
        zero[6] = 0;
        zero[7] = 0;
        assert_eq!(keyframe_size(&zero), None);
        assert!(!is_shown(&[0x00]));
        assert!(!is_shown(&[]));
    }

    #[test]
    fn en_tete_image_cle_vp9() {
        // Profil 0, image clé montrée, code de départ, couleurs BT.601, 320×240.
        let key = [0x82, 0x49, 0x83, 0x42, 0x20, 0x13, 0xf0, 0x0e, 0xf0];
        assert_eq!(vp9_keyframe_size(&key), Some((320, 240)));
        // Image intermédiaire (bit de type à 1), marqueur faux, coupée.
        let mut inter = key;
        inter[0] |= 0x04;
        assert_eq!(vp9_keyframe_size(&inter), None);
        assert_eq!(vp9_keyframe_size(&[0x42, 0x49, 0x83, 0x42]), None);
        assert_eq!(vp9_keyframe_size(&key[..6]), None);
        assert_eq!(vp9_keyframe_size(&[]), None);
    }
}
