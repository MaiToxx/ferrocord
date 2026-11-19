//! Charge utile RTP AV1 (« RTP Payload Format For AV1 », AOMedia, v1.0), à
//! la réception seulement.
//!
//! Chaque paquet commence par un octet d'agrégation (Z : le premier élément
//! continue une OBU du paquet précédent ; Y : le dernier continue dans le
//! suivant ; W : nombre d'éléments, 0 = chacun précédé de sa longueur ; N :
//! nouvelle séquence vidéo, donc image clé). Les OBU voyagent sans leur
//! champ de taille.
//!
//! L'image rendue a la forme qu'attend DAVE (protocol.md, « AV1 », et le
//! correctif de Discord à WebRTC en annexe A) : chaque OBU reprend sa
//! taille, sauf la dernière, qui s'étend jusqu'à la fin. L'émetteur a
//! chiffré l'image sous cette forme exacte, données de DAVE ajoutées au
//! bout ; `video::codec::av1` la remet ensuite au format des décodeurs.

const HAS_EXTENSION: u8 = 0x04;
const HAS_SIZE: u8 = 0x02;

/// Entier LEB128 (AV1, 4.10.5) : valeur et octets lus. Huit octets au plus.
pub fn leb128(d: &[u8]) -> Option<(usize, usize)> {
    let mut v = 0u64;
    for (i, &b) in d.iter().enumerate().take(8) {
        v |= u64::from(b & 0x7f) << (7 * i);
        if b & 0x80 == 0 {
            return usize::try_from(v).ok().filter(|v| *v <= u32::MAX as usize).map(|v| (v, i + 1));
        }
    }
    None
}

/// Écrit `v` en LEB128, au plus court.
pub fn write_leb128(mut v: usize, out: &mut Vec<u8>) {
    loop {
        let b = (v & 0x7f) as u8;
        v >>= 7;
        if v == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

/// Image AV1 à partir des charges utiles d'une image, dans l'ordre, et si
/// c'est une image clé (bit N du premier paquet).
pub fn depacketize<'a>(payloads: impl IntoIterator<Item = &'a [u8]>) -> Option<(Vec<u8>, bool)> {
    let mut obus: Vec<Vec<u8>> = Vec::new();
    let mut key = false;
    // Le dernier élément du paquet précédent attend sa suite.
    let mut open = false;
    for (k, p) in payloads.into_iter().enumerate() {
        let (&head, mut rest) = p.split_first()?;
        let (z, y, w) = (head & 0x80 != 0, head & 0x40 != 0, usize::from((head >> 4) & 0x03));
        if k == 0 {
            key = head & 0x08 != 0;
        }
        // Une suite sans début, ou un début resté sans suite : image abîmée.
        if z != open {
            return None;
        }
        let mut index = 0;
        while !rest.is_empty() || (w != 0 && index < w) {
            let last = w != 0 && index + 1 == w;
            let element = if last {
                std::mem::take(&mut rest)
            } else {
                let (len, n) = leb128(rest)?;
                let e = rest.get(n..n + len)?;
                rest = &rest[n + len..];
                e
            };
            if index == 0 && z {
                obus.last_mut()?.extend_from_slice(element);
            } else {
                obus.push(element.to_vec());
            }
            index += 1;
            if last {
                break;
            }
        }
        if index == 0 {
            return None;
        }
        open = y;
    }
    if open {
        return None;
    }
    let obus: Vec<&[u8]> = obus.iter().map(Vec::as_slice).filter(|o| !o.is_empty()).collect();
    let mut out = Vec::with_capacity(obus.iter().map(|o| o.len() + 2).sum());
    for (k, o) in obus.iter().enumerate() {
        let head = o[0];
        let header = if head & HAS_EXTENSION != 0 { 2 } else { 1 };
        let mut payload = o.get(header..)?;
        if head & HAS_SIZE != 0 {
            // Taille transmise malgré tout : elle doit couvrir le reste.
            let (len, n) = leb128(payload)?;
            payload = payload.get(n..).filter(|p| p.len() == len)?;
        }
        if k + 1 == obus.len() {
            out.push(head & !HAS_SIZE);
            out.extend_from_slice(&o[1..header]);
        } else {
            out.push(head | HAS_SIZE);
            out.extend_from_slice(&o[1..header]);
            write_leb128(payload.len(), &mut out);
        }
        out.extend_from_slice(payload);
    }
    (!out.is_empty()).then_some((out, key))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leb128_aller_retour() {
        for v in [0usize, 1, 127, 128, 300, 16_383, 16_384, 1_000_000] {
            let mut b = Vec::new();
            write_leb128(v, &mut b);
            assert_eq!(leb128(&b), Some((v, b.len())), "{v}");
        }
        // Forme rallongée (encodeurs AMD) : lue, mais jamais écrite ainsi.
        assert_eq!(leb128(&[0x85, 0x00]), Some((5, 2)));
        assert_eq!(leb128(&[0x80]), None);
        assert_eq!(leb128(&[]), None);
        assert_eq!(leb128(&[0xff; 9]), None);
    }

    #[test]
    fn un_paquet_plusieurs_obu() {
        // N, W = 2 : en-tête de séquence (type 1) avec sa longueur, puis image (type 6) jusqu'au bout.
        let p = [0x08 | 0x20, 3, 0x08, 0xaa, 0xbb, 0x30, 1, 2, 3];
        let (f, key) = depacketize([&p[..]]).unwrap_or_else(|| panic!("image valide"));
        assert!(key);
        // Première OBU avec sa taille, dernière sans.
        assert_eq!(f, vec![0x0a, 2, 0xaa, 0xbb, 0x30, 1, 2, 3]);
        // W = 0 : chaque élément précédé de sa longueur.
        let p = [0x00, 3, 0x08, 0xaa, 0xbb, 4, 0x30, 1, 2, 3];
        assert_eq!(depacketize([&p[..]]), Some((f, false)));
    }

    #[test]
    fn obu_coupee_entre_paquets() {
        // OBU d'image avec extension, coupée en trois ; la dernière OBU suit dans le dernier paquet.
        let a = [0x08 | 0x40 | 0x10, 0x34, 0x20, 1, 2];
        let b = [0x80 | 0x40 | 0x10, 3, 4];
        let c = [0x80 | 0x20, 1, 5, 0x30, 9];
        let (f, key) = depacketize([&a[..], &b[..], &c[..]]).unwrap_or_else(|| panic!("image valide"));
        assert!(key);
        assert_eq!(f, vec![0x36, 0x20, 5, 1, 2, 3, 4, 5, 0x30, 9]);
        // Paquet du milieu perdu : suite attendue absente, ou suite sans début.
        assert_eq!(depacketize([&a[..], &c[1..]]), None);
        assert_eq!(depacketize([&b[..], &c[..]]), None);
        // Image qui finit sur une OBU inachevée.
        assert_eq!(depacketize([&a[..], &b[..]]), None);
    }

    #[test]
    fn taille_transmise_et_entrees_abimees() {
        // OBU arrivée avec son champ de taille : il est relu puis réécrit ou retiré.
        let p = [0x20, 4, 0x0a, 2, 0xaa, 0xbb, 0x32, 2, 7, 8];
        assert_eq!(depacketize([&p[..]]), Some((vec![0x0a, 2, 0xaa, 0xbb, 0x30, 7, 8], false)));
        // Taille qui ment.
        assert_eq!(depacketize([&[0x10, 0x32, 5, 7, 8][..]]), None);
        // Longueur d'élément qui dépasse le paquet, paquet vide, octet d'agrégation seul.
        assert_eq!(depacketize([&[0x00, 9, 1][..]]), None);
        assert_eq!(depacketize([&[][..]]), None);
        assert_eq!(depacketize([&[0x00][..]]), None);
        assert_eq!(depacketize(std::iter::empty()), None);
    }
}
