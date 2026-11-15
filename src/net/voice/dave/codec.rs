//! Lecture et écriture du langage de présentation TLS tel que MLS l'emploie
//! (RFC 9420, 2.1) : entiers gros-boutistes, vecteurs préfixés par un
//! entier variable (1, 2 ou 4 octets), optionnels sur un octet. Et ULEB128
//! pour les trames DAVE.

use super::Error;

pub type R<T> = Result<T, Error>;

/// Plus grand vecteur accepté : bien au-delà de ce qu'envoie la passerelle,
/// mais évite d'allouer sur une longueur forgée.
const MAX_VEC: usize = 1 << 20;

pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf, pos: 0 }
    }

    pub fn pos(&self) -> usize {
        self.pos
    }

    /// Octets lus entre `start` et la position courante (pour garder la
    /// forme exacte d'une structure signée ou hachée).
    pub fn since(&self, start: usize) -> &'a [u8] {
        &self.buf[start..self.pos]
    }

    pub fn is_empty(&self) -> bool {
        self.pos >= self.buf.len()
    }

    pub fn rest(&mut self) -> &'a [u8] {
        let r = &self.buf[self.pos..];
        self.pos = self.buf.len();
        r
    }

    pub fn bytes(&mut self, n: usize) -> R<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error("longueur"))?;
        if end > self.buf.len() {
            return Err(Error("message tronqué"));
        }
        let r = &self.buf[self.pos..end];
        self.pos = end;
        Ok(r)
    }

    pub fn u8(&mut self) -> R<u8> {
        Ok(self.bytes(1)?[0])
    }

    pub fn u16(&mut self) -> R<u16> {
        let b = self.bytes(2)?;
        Ok(u16::from_be_bytes([b[0], b[1]]))
    }

    pub fn u32(&mut self) -> R<u32> {
        let b = self.bytes(4)?;
        Ok(u32::from_be_bytes([b[0], b[1], b[2], b[3]]))
    }

    pub fn u64(&mut self) -> R<u64> {
        let b = self.bytes(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_be_bytes(a))
    }

    /// Entier variable de MLS ; l'encodage minimal est exigé (RFC 9420, 2.1.2).
    pub fn varint(&mut self) -> R<usize> {
        let first = self.u8()?;
        let (len, v) = match first >> 6 {
            0 => (1, u32::from(first & 0x3f)),
            1 => (2, u32::from(first & 0x3f) << 8 | u32::from(self.u8()?)),
            2 => {
                let b = self.bytes(3)?;
                (4, u32::from(first & 0x3f) << 24 | u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]))
            }
            _ => return Err(Error("entier variable invalide")),
        };
        let minimal = match len {
            1 => true,
            2 => v >= 64,
            _ => v >= 16384,
        };
        if !minimal {
            return Err(Error("entier variable non minimal"));
        }
        Ok(v as usize)
    }

    /// Vecteur `opaque<V>`.
    pub fn vec(&mut self) -> R<&'a [u8]> {
        let n = self.varint()?;
        if n > MAX_VEC {
            return Err(Error("vecteur trop long"));
        }
        self.bytes(n)
    }

    /// Vecteur d'éléments : rend un lecteur borné à son contenu.
    pub fn sub(&mut self) -> R<Reader<'a>> {
        Ok(Reader::new(self.vec()?))
    }

    /// `optional<T>` : vrai si présent.
    pub fn present(&mut self) -> R<bool> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error("optionnel invalide")),
        }
    }

    pub fn end(&self) -> R<()> {
        if self.is_empty() { Ok(()) } else { Err(Error("octets en trop")) }
    }
}

#[derive(Default)]
pub struct Writer {
    pub buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Writer { buf: Vec::new() }
    }

    pub fn u8(&mut self, v: u8) -> &mut Self {
        self.buf.push(v);
        self
    }

    pub fn u16(&mut self, v: u16) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u32(&mut self, v: u32) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn u64(&mut self, v: u64) -> &mut Self {
        self.buf.extend_from_slice(&v.to_be_bytes());
        self
    }

    pub fn raw(&mut self, b: &[u8]) -> &mut Self {
        self.buf.extend_from_slice(b);
        self
    }

    pub fn varint(&mut self, n: usize) -> &mut Self {
        if n < 64 {
            self.buf.push(n as u8);
        } else if n < 16384 {
            self.buf.extend_from_slice(&((n as u16) | 0x4000).to_be_bytes());
        } else {
            // Au-delà de 2^30 : impossible ici (nos messages font quelques Kio).
            self.buf.extend_from_slice(&((n as u32 & 0x3fff_ffff) | 0x8000_0000).to_be_bytes());
        }
        self
    }

    pub fn vec(&mut self, b: &[u8]) -> &mut Self {
        self.varint(b.len());
        self.raw(b)
    }

    /// Écrit un vecteur dont le contenu est produit par `f`.
    pub fn nested(&mut self, f: impl FnOnce(&mut Writer)) -> &mut Self {
        let mut w = Writer::new();
        f(&mut w);
        self.vec(&w.buf)
    }

}

/// Raccourci : construit un tampon avec `f`.
pub fn build(f: impl FnOnce(&mut Writer)) -> Vec<u8> {
    let mut w = Writer::new();
    f(&mut w);
    w.buf
}

pub fn uleb128_write(mut v: u64, out: &mut Vec<u8>) {
    while v >= 0x80 {
        out.push(0x80 | (v & 0x7f) as u8);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Lit un ULEB128 dans `buf` à partir de `*pos` ; refuse un débordement de 64 bits.
pub fn uleb128_read(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut v: u64 = 0;
    let mut shift = 0u32;
    loop {
        let b = *buf.get(*pos)?;
        *pos += 1;
        if shift >= 64 || (shift == 63 && b & 0x7e != 0) {
            return None;
        }
        v |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_aller_retour_et_minimal() {
        for n in [0usize, 1, 63, 64, 100, 16383, 16384, 70000] {
            let b = build(|w| {
                w.varint(n);
            });
            let mut r = Reader::new(&b);
            assert_eq!(r.varint().unwrap(), n);
            assert!(r.is_empty());
        }
        // 5 codé sur deux octets : refusé.
        assert!(Reader::new(&[0x40, 0x05]).varint().is_err());
        assert!(Reader::new(&[0xc0]).varint().is_err());
    }

    #[test]
    fn uleb128() {
        for v in [0u64, 1, 127, 128, 300, u32::MAX as u64, u64::MAX] {
            let mut b = Vec::new();
            uleb128_write(v, &mut b);
            let mut p = 0;
            assert_eq!(uleb128_read(&b, &mut p), Some(v));
            assert_eq!(p, b.len());
        }
        let mut b = Vec::new();
        uleb128_write(300, &mut b);
        assert_eq!(b, [0xac, 0x02]);
        let mut p = 0;
        assert_eq!(uleb128_read(&[0x80, 0x80], &mut p), None);
    }
}
