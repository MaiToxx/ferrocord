//! Charge utile RTP VP9, RFC 9628, à la réception seulement : le
//! descripteur de chaque paquet est sauté, les charges mises bout à bout.
//!
//! L'image clé se lit dans le descripteur (bit P à 0 sur le premier paquet
//! de la couche spatiale 0), pas dans l'image : DAVE chiffre l'image VP9
//! entière, en-tête compris.

/// Descripteur d'un paquet : longueur, début d'image (B), image prédite (P),
/// couche spatiale.
struct Descriptor {
    len: usize,
    start: bool,
    predicted: bool,
    spatial: u8,
}

/// RFC 9628, 4.2. `None` : descripteur coupé.
fn descriptor(p: &[u8]) -> Option<Descriptor> {
    let b0 = *p.first()?;
    let (i, pred, l, f, b, v) = (b0 & 0x80 != 0, b0 & 0x40 != 0, b0 & 0x20 != 0, b0 & 0x10 != 0, b0 & 0x08 != 0, b0 & 0x02 != 0);
    let mut n = 1;
    if i {
        // Identifiant d'image : 7 bits, ou 15 si le bit M est posé.
        n += if *p.get(n)? & 0x80 != 0 { 2 } else { 1 };
    }
    let mut spatial = 0;
    if l {
        spatial = (*p.get(n)? >> 1) & 0x07;
        // Mode non flexible : TL0PICIDX suit.
        n += if f { 1 } else { 2 };
    }
    if f && pred {
        // Jusqu'à trois écarts de référence, chacun annonçant le suivant (bit N).
        for k in 0..3 {
            let more = *p.get(n)? & 0x01 != 0;
            n += 1;
            if !more {
                break;
            }
            if k == 2 {
                return None;
            }
        }
    }
    if v {
        // Structure d'échelonnage : tailles des couches, puis groupe d'images.
        let ss = *p.get(n)?;
        n += 1;
        if ss & 0x10 != 0 {
            n += 4 * (usize::from(ss >> 5) + 1);
        }
        if ss & 0x08 != 0 {
            let pictures = usize::from(*p.get(n)?);
            n += 1;
            for _ in 0..pictures {
                n += 1 + usize::from((*p.get(n)? >> 2) & 0x03);
            }
        }
    }
    (n <= p.len()).then_some(Descriptor { len: n, start: b, predicted: pred, spatial })
}

/// Image VP9 à partir des charges utiles d'une image, dans l'ordre, et si
/// c'est une image clé.
pub fn depacketize<'a>(payloads: impl IntoIterator<Item = &'a [u8]>) -> Option<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    let mut key = false;
    for (k, p) in payloads.into_iter().enumerate() {
        let d = descriptor(p)?;
        if k == 0 {
            if !d.start {
                return None;
            }
            key = !d.predicted && d.spatial == 0;
        }
        out.extend_from_slice(&p[d.len..]);
    }
    (!out.is_empty()).then_some((out, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descripteur_minimal_et_image_cle() {
        // B seul : image clé, un octet de descripteur.
        let a = [0x08, 1, 2, 3];
        // Suite, fin (E).
        let b = [0x04, 4, 5];
        assert_eq!(depacketize([&a[..], &b[..]]), Some((vec![1, 2, 3, 4, 5], true)));
        // P posé : image intermédiaire.
        assert_eq!(depacketize([&[0x48, 9][..]]), Some((vec![9], false)));
        // Premier paquet sans B : image incomplète.
        assert_eq!(depacketize([&b[..]]), None);
        assert_eq!(depacketize([&[0x08][..]]), None);
        assert_eq!(depacketize(std::iter::empty()), None);
    }

    #[test]
    fn champs_facultatifs_sautes() {
        // I avec identifiant sur 15 bits, L en mode non flexible (TID, SID 0, TL0PICIDX).
        let p = [0x80 | 0x20 | 0x08, 0x81, 0x23, 0x00, 0x07, 0xaa, 0xbb];
        assert_eq!(depacketize([&p[..]]), Some((vec![0xaa, 0xbb], true)));
        // Identifiant sur 7 bits, mode flexible, P avec deux écarts de référence.
        let p = [0x80 | 0x40 | 0x10 | 0x08, 0x12, 0x03, 0x04, 0xcc];
        assert_eq!(depacketize([&p[..]]), Some((vec![0xcc], false)));
        // Couche spatiale 1 : pas une image clé même sans P.
        let p = [0x20 | 0x10 | 0x08, 0x02, 0xdd];
        assert_eq!(depacketize([&p[..]]), Some((vec![0xdd], false)));
        // Structure d'échelonnage : une couche avec sa taille, groupe de deux
        // images dont la seconde a une référence.
        let p = [0x08 | 0x02, 0x18, 1, 64, 0, 240, 2, 0x00, 0x04, 1, 0xee];
        assert_eq!(depacketize([&p[..]]), Some((vec![0xee], true)));
        // Descripteur coupé avant la fin.
        assert_eq!(depacketize([&p[..6]]), None);
        // Trois écarts qui en annoncent un quatrième : refusé.
        assert_eq!(depacketize([&[0x40 | 0x10 | 0x08, 3, 3, 3, 3, 0][..]]), None);
    }
}
