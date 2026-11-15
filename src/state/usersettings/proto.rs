//! Format binaire protobuf, réduit à ce qu'il faut pour lire et réécrire les
//! réglages du compte sans rien perdre.
//!
//! Pourquoi un décodeur maison plutôt que `prost` : `prost` jette les champs
//! qu'il ne connaît pas au décodage, donc les réécrire effacerait des réglages
//! ajoutés par Discord après l'écriture de ce code. Ici un message est une
//! simple liste de champs (numéro + valeur brute), dans l'ordre reçu : ce
//! qu'on ne modifie pas est réécrit tel quel. Aucune dépendance, aucune étape
//! de compilation.

/// Valeur brute d'un champ, selon son type de fil.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    /// Type de fil 0 (entiers, booléens, énumérations).
    Varint(u64),
    /// Type de fil 1 (`fixed64`, `double`).
    I64(u64),
    /// Type de fil 2 (chaînes, octets, messages imbriqués, listes compactées).
    Len(Vec<u8>),
    /// Type de fil 5 (`fixed32`, `float`).
    I32(u32),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub num: u32,
    pub value: Value,
}

/// Message décodé : ses champs dans l'ordre du fil, inconnus compris.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Msg {
    pub fields: Vec<Field>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    BadVarint,
    /// Groupes (types 3 et 4, abandonnés par protobuf) ou type inconnu.
    WireType(u8),
    FieldZero,
}

/// Profondeur maximale suivie par [`Msg::edit_path`] (les réglages en ont 3).
const MAX_DEPTH: usize = 8;

fn read_varint(b: &[u8], pos: &mut usize) -> Result<u64, DecodeError> {
    let mut v: u64 = 0;
    for shift in (0..64).step_by(7) {
        let Some(&byte) = b.get(*pos) else { return Err(DecodeError::Truncated) };
        *pos += 1;
        // Le dixième octet ne peut porter qu'un bit.
        if shift == 63 && byte > 1 {
            return Err(DecodeError::BadVarint);
        }
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(v);
        }
    }
    Err(DecodeError::BadVarint)
}

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn take<'a>(b: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], DecodeError> {
    let end = pos.checked_add(n).ok_or(DecodeError::Truncated)?;
    let s = b.get(*pos..end).ok_or(DecodeError::Truncated)?;
    *pos = end;
    Ok(s)
}

impl Msg {
    pub fn decode(b: &[u8]) -> Result<Msg, DecodeError> {
        let mut pos = 0;
        let mut fields = Vec::new();
        while pos < b.len() {
            let key = read_varint(b, &mut pos)?;
            let num = u32::try_from(key >> 3).map_err(|_| DecodeError::BadVarint)?;
            if num == 0 {
                return Err(DecodeError::FieldZero);
            }
            let value = match (key & 7) as u8 {
                0 => Value::Varint(read_varint(b, &mut pos)?),
                1 => {
                    let s = take(b, &mut pos, 8)?;
                    Value::I64(u64::from_le_bytes(s.try_into().map_err(|_| DecodeError::Truncated)?))
                }
                2 => {
                    let n = usize::try_from(read_varint(b, &mut pos)?).map_err(|_| DecodeError::Truncated)?;
                    Value::Len(take(b, &mut pos, n)?.to_vec())
                }
                5 => {
                    let s = take(b, &mut pos, 4)?;
                    Value::I32(u32::from_le_bytes(s.try_into().map_err(|_| DecodeError::Truncated)?))
                }
                w => return Err(DecodeError::WireType(w)),
            };
            fields.push(Field { num, value });
        }
        Ok(Msg { fields })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        for f in &self.fields {
            let num = u64::from(f.num) << 3;
            match &f.value {
                Value::Varint(v) => {
                    write_varint(&mut out, num);
                    write_varint(&mut out, *v);
                }
                Value::I64(v) => {
                    write_varint(&mut out, num | 1);
                    out.extend_from_slice(&v.to_le_bytes());
                }
                Value::Len(b) => {
                    write_varint(&mut out, num | 2);
                    write_varint(&mut out, b.len() as u64);
                    out.extend_from_slice(b);
                }
                Value::I32(v) => {
                    write_varint(&mut out, num | 5);
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        out
    }

    /// Dernière valeur du champ `num` (règle protobuf pour un champ simple).
    pub fn get(&self, num: u32) -> Option<&Value> {
        self.fields.iter().rev().find(|f| f.num == num).map(|f| &f.value)
    }

    pub fn has(&self, num: u32) -> bool {
        self.fields.iter().any(|f| f.num == num)
    }

    pub fn varint(&self, num: u32) -> Option<u64> {
        match self.get(num)? {
            Value::Varint(v) => Some(*v),
            _ => None,
        }
    }

    pub fn fixed64(&self, num: u32) -> Option<u64> {
        match self.get(num)? {
            Value::I64(v) => Some(*v),
            _ => None,
        }
    }

    pub fn bytes(&self, num: u32) -> Option<&[u8]> {
        match self.get(num)? {
            Value::Len(b) => Some(b),
            _ => None,
        }
    }

    pub fn string(&self, num: u32) -> Option<String> {
        self.bytes(num).map(|b| String::from_utf8_lossy(b).into_owned())
    }

    /// Message imbriqué `num`. Plusieurs occurrences se fusionnent (règle
    /// protobuf) : leurs octets mis bout à bout se décodent en un seul message.
    /// `Ok(None)` : absent ; `Err` : illisible.
    pub fn message(&self, num: u32) -> Result<Option<Msg>, DecodeError> {
        let mut joined: Option<Vec<u8>> = None;
        for f in self.fields.iter().filter(|f| f.num == num) {
            match &f.value {
                Value::Len(b) => joined.get_or_insert_with(Vec::new).extend_from_slice(b),
                _ => return Err(DecodeError::WireType(0)),
            }
        }
        joined.map(|b| Msg::decode(&b)).transpose()
    }

    /// Liste de `fixed64` (compactée ou non).
    pub fn repeated_fixed64(&self, num: u32) -> Vec<u64> {
        let mut out = Vec::new();
        for f in self.fields.iter().filter(|f| f.num == num) {
            match &f.value {
                Value::I64(v) => out.push(*v),
                Value::Len(b) => out.extend(b.as_chunks::<8>().0.iter().copied().map(u64::from_le_bytes)),
                _ => {}
            }
        }
        out
    }

    /// Remplace toutes les occurrences de `num` par une seule, à la place
    /// de la première (ajoutée à la fin si le champ était absent).
    pub fn set(&mut self, num: u32, value: Value) {
        match self.fields.iter().position(|f| f.num == num) {
            Some(i) => {
                self.fields[i].value = value;
                let mut seen = 0usize;
                self.fields.retain(|f| {
                    if f.num != num {
                        return true;
                    }
                    seen += 1;
                    seen == 1
                });
            }
            None => self.fields.push(Field { num, value }),
        }
    }

    pub fn remove(&mut self, num: u32) {
        self.fields.retain(|f| f.num != num);
    }

    /// Modifie le message imbriqué au bout de `path` (créé s'il manque) ;
    /// tout le reste, champs inconnus compris, est gardé tel quel.
    pub fn edit_path(&mut self, path: &[u32], f: impl FnOnce(&mut Msg)) -> Result<(), DecodeError> {
        fn walk(m: &mut Msg, path: &[u32], depth: usize, f: Box<dyn FnOnce(&mut Msg) + '_>) -> Result<(), DecodeError> {
            let Some((&head, rest)) = path.split_first() else {
                f(m);
                return Ok(());
            };
            if depth >= MAX_DEPTH {
                return Err(DecodeError::Truncated);
            }
            let mut sub = m.message(head)?.unwrap_or_default();
            walk(&mut sub, rest, depth + 1, f)?;
            m.set(head, Value::Len(sub.encode()));
            Ok(())
        }
        walk(self, path, 0, Box::new(f))
    }

    /// Message imbriqué au bout de `path` (`Ok(None)` si une étape manque).
    pub fn at_path(&self, path: &[u32]) -> Result<Option<Msg>, DecodeError> {
        let mut cur = self.clone();
        for &n in path {
            match cur.message(n)? {
                Some(m) => cur = m,
                None => return Ok(None),
            }
        }
        Ok(Some(cur))
    }

    /// Octets bruts de chaque occurrence du champ de premier niveau `num`
    /// (sert à comparer et à rétablir un champ entier).
    pub fn raw_field(&self, num: u32) -> Vec<Field> {
        self.fields.iter().filter(|f| f.num == num).cloned().collect()
    }

    /// Remet le champ `num` à `fields` (vide : retiré), à sa place d'origine si possible.
    pub fn restore_field(&mut self, num: u32, fields: Vec<Field>) {
        let at = self.fields.iter().position(|f| f.num == num).unwrap_or(self.fields.len());
        self.fields.retain(|f| f.num != num);
        let at = at.min(self.fields.len());
        self.fields.splice(at..at, fields);
    }

    /// Numéros des champs de premier niveau, sans doublon, dans l'ordre.
    pub fn top_numbers(&self) -> Vec<u32> {
        let mut out: Vec<u32> = Vec::new();
        for f in &self.fields {
            if !out.contains(&f.num) {
                out.push(f.num);
            }
        }
        out
    }
}

/// Message protobuf « enveloppe » (`google.protobuf.BoolValue`, etc.) : un seul
/// champ 1. Valeur par défaut omise, comme le fait l'encodeur officiel.
pub fn wrapper(v: Value) -> Msg {
    let mut m = Msg::default();
    match v {
        Value::Varint(0) => {}
        Value::Len(ref b) if b.is_empty() => {}
        v => m.set(1, v),
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Message de test : un booléen, un fixed64, une chaîne, un message
    /// imbriqué, un fixed32 et un champ inconnu de grand numéro.
    fn sample() -> Vec<u8> {
        let mut inner = Msg::default();
        inner.set(1, Value::Varint(1));
        inner.set(900, Value::Len(b"inconnu".to_vec()));
        let mut m = Msg::default();
        m.set(1, Value::Varint(300));
        m.set(2, Value::I64(0x0102_0304_0506_0708));
        m.set(3, Value::Len(b"texte".to_vec()));
        m.set(4, Value::Len(inner.encode()));
        m.set(5, Value::I32(7));
        m.set(536_870_911, Value::Varint(u64::MAX));
        m.encode()
    }

    #[test]
    fn round_trip_is_byte_exact() {
        let bytes = sample();
        let m = Msg::decode(&bytes).unwrap();
        assert_eq!(m.fields.len(), 6);
        assert_eq!(m.encode(), bytes);
        assert_eq!(m.varint(1), Some(300));
        assert_eq!(m.fixed64(2), Some(0x0102_0304_0506_0708));
        assert_eq!(m.string(3).as_deref(), Some("texte"));
        assert_eq!(m.varint(536_870_911), Some(u64::MAX));
        let inner = m.message(4).unwrap().unwrap();
        assert_eq!(inner.varint(1), Some(1));
        assert_eq!(inner.bytes(900), Some(&b"inconnu"[..]));
    }

    #[test]
    fn bad_input_is_an_error_never_a_panic() {
        assert_eq!(Msg::decode(&[0, 0, 0]), Err(DecodeError::FieldZero));
        assert_eq!(Msg::decode(&[0x08]), Err(DecodeError::Truncated));
        assert_eq!(Msg::decode(&[0x0a, 5, 1]), Err(DecodeError::Truncated));
        assert_eq!(Msg::decode(&[0x0b]), Err(DecodeError::WireType(3)));
        assert_eq!(Msg::decode(&[0x08, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x7f]), Err(DecodeError::BadVarint));
        assert_eq!(Msg::decode(&[0x11, 1, 2]), Err(DecodeError::Truncated));
        assert!(Msg::decode(&[]).unwrap().fields.is_empty());
        // Toutes les coupures possibles d'un message valide : erreur ou message, jamais de panique.
        let bytes = sample();
        for n in 0..bytes.len() {
            let _ = Msg::decode(&bytes[..n]);
        }
    }

    #[test]
    fn edit_keeps_unknown_fields_at_every_level() {
        let bytes = sample();
        let mut m = Msg::decode(&bytes).unwrap();
        m.edit_path(&[4], |inner| inner.set(1, Value::Varint(0))).unwrap();
        let inner = m.message(4).unwrap().unwrap();
        assert_eq!(inner.varint(1), Some(0));
        assert_eq!(inner.bytes(900), Some(&b"inconnu"[..]), "champ inconnu imbriqué gardé");
        assert_eq!(m.varint(536_870_911), Some(u64::MAX), "champ inconnu de premier niveau gardé");
        assert_eq!(m.top_numbers(), vec![1, 2, 3, 4, 5, 536_870_911], "ordre gardé");
        // Chemin absent : créé.
        m.edit_path(&[6, 2], |x| x.set(1, Value::Varint(9))).unwrap();
        assert_eq!(m.at_path(&[6, 2]).unwrap().unwrap().varint(1), Some(9));
        assert_eq!(m.at_path(&[7, 1]).unwrap(), None);
    }

    #[test]
    fn repeated_occurrences() {
        // Deux occurrences d'un message imbriqué se fusionnent ; d'un scalaire, la dernière gagne.
        let mut a = Msg::default();
        a.set(1, Value::Varint(1));
        let mut b = Msg::default();
        b.set(2, Value::Varint(2));
        let m = Msg {
            fields: vec![
                Field { num: 1, value: Value::Len(a.encode()) },
                Field { num: 2, value: Value::Varint(5) },
                Field { num: 1, value: Value::Len(b.encode()) },
                Field { num: 2, value: Value::Varint(6) },
            ],
        };
        let merged = m.message(1).unwrap().unwrap();
        assert_eq!((merged.varint(1), merged.varint(2)), (Some(1), Some(2)));
        assert_eq!(m.varint(2), Some(6));
        let mut m2 = m.clone();
        m2.set(2, Value::Varint(7));
        assert_eq!(m2.fields.iter().filter(|f| f.num == 2).count(), 1);
        assert_eq!(m2.fields[1].num, 2, "à la place de la première occurrence");
        // Liste de fixed64 compactée puis non compactée.
        let mut packed = Vec::new();
        packed.extend_from_slice(&10u64.to_le_bytes());
        packed.extend_from_slice(&11u64.to_le_bytes());
        let l = Msg { fields: vec![Field { num: 1, value: Value::Len(packed) }, Field { num: 1, value: Value::I64(12) }] };
        assert_eq!(l.repeated_fixed64(1), vec![10, 11, 12]);
    }

    #[test]
    fn restore_puts_a_field_back_in_place() {
        let mut m = Msg::decode(&sample()).unwrap();
        let before = m.raw_field(3);
        m.set(3, Value::Len(b"autre".to_vec()));
        m.restore_field(3, before);
        assert_eq!(m.encode(), sample());
        let before = m.raw_field(3);
        m.remove(3);
        m.restore_field(3, before);
        assert_eq!(m.string(3).as_deref(), Some("texte"));
    }

    #[test]
    fn wrappers_omit_the_default() {
        assert!(wrapper(Value::Varint(0)).fields.is_empty());
        assert_eq!(wrapper(Value::Varint(1)).encode(), vec![0x08, 0x01]);
        assert!(wrapper(Value::Len(Vec::new())).fields.is_empty());
    }
}
