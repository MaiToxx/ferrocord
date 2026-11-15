//! Chiffrement des trames de média DAVE (protocol.md, « Encoded Frame
//! Transforms ») : cliquet de clés par expéditeur, nonce tronqué de 32 bits
//! dont l'octet de poids fort est la génération, étiquette AES-GCM de 8
//! octets, suffixe ULEB128 et marqueur 0xFAFA. L'émetteur ne connaît que
//! l'Opus (trame entièrement chiffrée) ; le récepteur lit aussi les plages
//! en clair des autres codecs.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::time::{Duration, Instant};

use super::codec::{uleb128_read, uleb128_write};
use super::crypto::{self, FrameCipher, KEY_LEN, NH, NONCE_LEN, TAG_LEN};
use super::mls::export_with_root;

/// Étiquette de l'exportateur MLS pour les clés de média.
pub const EXPORT_LABEL: &str = "Discord Secure Frames v0";
const GEN_SHIFT: u32 = 24;
const MAX_GENERATION_GAP: u32 = 250;
const MAX_MISSING_NONCES: usize = 1000;
const MAX_FRAMES_PER_SECOND: u64 = 50 + 2 * 60;
/// Durée de vie des clés d'une époque ou d'une génération dépassée.
pub const EXPIRY: Duration = Duration::from_secs(10);
/// Silence synthétisé par le SFU, toujours accepté en clair.
pub const SILENCE: [u8; 3] = [0xf8, 0xff, 0xfe];
const MARKER: [u8; 2] = [0xfa, 0xfa];
/// Étiquette + taille du suffixe + marqueur.
const MIN_SUPPLEMENTAL: usize = TAG_LEN + 1 + 2;

/// Clé de base d'un expéditeur pour une époque.
pub fn sender_base(root: &[u8], user_id: u64) -> Vec<u8> {
    export_with_root(root, &user_id.to_le_bytes(), KEY_LEN)
}

fn full_nonce(n: u32) -> [u8; NONCE_LEN] {
    let mut out = [0u8; NONCE_LEN];
    out[8..].copy_from_slice(&n.to_le_bytes());
    out
}

fn derive_tree_secret(secret: &[u8], label: &str, generation: u32, len: usize) -> Vec<u8> {
    crypto::expand_with_label(secret, label, &generation.to_be_bytes(), len)
}

/// Cliquet de hachage de MLS (RFC 9420, 9.1) partant de la clé de base.
pub struct HashRatchet {
    next_secret: Vec<u8>,
    next_gen: u32,
    keys: BTreeMap<u32, Vec<u8>>,
}

impl HashRatchet {
    pub fn new(base: Vec<u8>) -> Self {
        HashRatchet { next_secret: base, next_gen: 0, keys: BTreeMap::new() }
    }

    pub fn get(&mut self, generation: u32) -> Option<Vec<u8>> {
        while self.next_gen <= generation {
            let g = self.next_gen;
            self.keys.insert(g, derive_tree_secret(&self.next_secret, "key", g, KEY_LEN));
            self.next_secret = derive_tree_secret(&self.next_secret, "secret", g, NH);
            self.next_gen = g.checked_add(1)?;
        }
        self.keys.get(&generation).cloned()
    }

    pub fn erase(&mut self, generation: u32) {
        self.keys.remove(&generation);
    }
}

fn wrapped_generation(oldest: u32, generation: u32) -> u32 {
    let remainder = oldest % 256;
    let factor = oldest / 256 + u32::from(generation < remainder);
    factor.saturating_mul(256).saturating_add(generation)
}

fn big_nonce(generation: u32, nonce: u32) -> u64 {
    u64::from(generation) << GEN_SHIFT | u64::from(nonce & ((1 << GEN_SHIFT) - 1))
}

// ---------------------------------------------------------------- émission

/// Chiffreur de nos propres trames.
#[derive(Default)]
pub struct Encryptor {
    ratchet: Option<HashRatchet>,
    nonce: u32,
    generation: u32,
    cipher: Option<FrameCipher>,
}

impl Encryptor {
    /// Nouvelle clé de base (nouvelle époque) : le nonce repart de zéro.
    pub fn set_base(&mut self, base: Option<Vec<u8>>) {
        self.ratchet = base.map(HashRatchet::new);
        self.nonce = 0;
        self.generation = 0;
        self.cipher = None;
    }

    pub fn has_key(&self) -> bool {
        self.ratchet.is_some()
    }

    /// Chiffre une trame Opus entière.
    pub fn encrypt(&mut self, opus: &[u8]) -> Option<Vec<u8>> {
        self.encrypt_ranges(opus, &[], false)
    }

    /// Chiffre `frame` sauf les plages `clear` (décalage, longueur ; triées,
    /// disjointes), lues en clair et authentifiées. `h26x` : le chiffré et le
    /// suffixe ne doivent contenir aucun code de début 00 00 01 (sinon le
    /// paquétiseur couperait au mauvais endroit) ; on réessaie avec le
    /// nonce suivant, 10 fois au plus (protocol.md, « Codec Handling »).
    pub fn encrypt_ranges(&mut self, frame: &[u8], clear: &[(usize, usize)], h26x: bool) -> Option<Vec<u8>> {
        let mut aad = Vec::new();
        let mut plain = Vec::with_capacity(frame.len());
        let mut at = 0;
        for &(o, s) in clear {
            if o < at || o.checked_add(s)? > frame.len() {
                return None;
            }
            plain.extend_from_slice(&frame[at..o]);
            aad.extend_from_slice(&frame[o..o + s]);
            at = o + s;
        }
        plain.extend_from_slice(&frame[at..]);
        for _ in 0..if h26x { 10 } else { 1 } {
            let out = self.seal_once(frame, clear, &aad, &plain)?;
            if !h26x || !start_code_in_ciphertext(&out, clear, frame.len()) {
                return Some(out);
            }
        }
        None
    }

    fn seal_once(&mut self, frame: &[u8], clear: &[(usize, usize)], aad: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
        let ratchet = self.ratchet.as_mut()?;
        self.nonce = self.nonce.wrapping_add(1);
        let generation = wrapped_generation(self.generation, self.nonce >> GEN_SHIFT);
        if generation != self.generation || self.cipher.is_none() {
            let key = ratchet.get(generation)?;
            ratchet.erase(generation);
            self.cipher = Some(FrameCipher::new(&key).ok()?);
            self.generation = generation;
        }
        let cipher = self.cipher.as_ref()?;
        let mut ct = plain.to_vec();
        let tag = cipher.seal(&full_nonce(self.nonce), aad, &mut ct).ok()?;
        let mut out = Vec::with_capacity(frame.len() + MIN_SUPPLEMENTAL + 5 + 4 * clear.len());
        let mut at = 0;
        let mut c = 0;
        for &(o, s) in clear {
            out.extend_from_slice(&ct[c..c + (o - at)]);
            c += o - at;
            out.extend_from_slice(&frame[o..o + s]);
            at = o + s;
        }
        out.extend_from_slice(&ct[c..]);
        out.extend_from_slice(&tag);
        let before = out.len();
        uleb128_write(u64::from(self.nonce), &mut out);
        for &(o, s) in clear {
            uleb128_write(o as u64, &mut out);
            uleb128_write(s as u64, &mut out);
        }
        let supplemental = TAG_LEN + (out.len() - before) + 1 + 2;
        out.push(u8::try_from(supplemental).ok()?);
        out.extend_from_slice(&MARKER);
        Some(out)
    }
}

/// Code de début 00 00 01 dans une partie chiffrée (2 octets de marge de
/// chaque côté) ou dans le suffixe : même vérification que libdave
/// (`ValidateEncryptedFrame`).
fn start_code_in_ciphertext(out: &[u8], clear: &[(usize, usize)], body_len: usize) -> bool {
    let has = |a: usize, b: usize| out[a.saturating_sub(2)..(b + 2).min(out.len())].windows(3).any(|w| w == [0, 0, 1]);
    let mut at = 0;
    for &(o, s) in clear {
        if o > at && has(at, o) {
            return true;
        }
        at = o + s;
    }
    (body_len > at && has(at, body_len)) || has(body_len, out.len())
}

// ---------------------------------------------------------------- réception

/// Trame protocolaire découpée (protocol.md, « Protocol Frame Check »).
pub struct Parsed<'a> {
    frame: &'a [u8],
    body_len: usize,
    tag: &'a [u8],
    nonce: u32,
    ranges: Vec<(usize, usize)>,
}

pub fn parse(frame: &[u8]) -> Option<Parsed<'_>> {
    let n = frame.len();
    if n < MIN_SUPPLEMENTAL || frame[n - 2..] != MARKER {
        return None;
    }
    let supplemental = usize::from(frame[n - 3]);
    if supplemental < MIN_SUPPLEMENTAL || supplemental > n {
        return None;
    }
    let body_len = n - supplemental;
    let tag = &frame[body_len..body_len + TAG_LEN];
    let meta = &frame[body_len + TAG_LEN..n - 3];
    let mut pos = 0;
    let nonce = u32::try_from(uleb128_read(meta, &mut pos)?).ok()?;
    let mut ranges = Vec::new();
    let mut end = 0usize;
    while pos < meta.len() {
        let offset = usize::try_from(uleb128_read(meta, &mut pos)?).ok()?;
        let size = usize::try_from(uleb128_read(meta, &mut pos)?).ok()?;
        let stop = offset.checked_add(size)?;
        if offset < end || stop > body_len {
            return None;
        }
        end = stop;
        ranges.push((offset, size));
    }
    Some(Parsed { frame, body_len, tag, nonce, ranges })
}

impl Parsed<'_> {
    /// (données authentifiées en clair, chiffré) dans l'ordre de la trame.
    fn split(&self) -> (Vec<u8>, Vec<u8>) {
        let mut aad = Vec::new();
        let mut ct = Vec::with_capacity(self.body_len);
        let mut at = 0;
        for &(o, s) in &self.ranges {
            ct.extend_from_slice(&self.frame[at..o]);
            aad.extend_from_slice(&self.frame[o..o + s]);
            at = o + s;
        }
        ct.extend_from_slice(&self.frame[at..self.body_len]);
        (aad, ct)
    }

    fn rebuild(&self, plain: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.body_len);
        let mut at = 0;
        let mut p = 0;
        for &(o, s) in &self.ranges {
            let k = o - at;
            out.extend_from_slice(&plain[p..p + k]);
            p += k;
            out.extend_from_slice(&self.frame[o..o + s]);
            at = o + s;
        }
        out.extend_from_slice(&plain[p..]);
        out
    }
}

/// Clés d'un expéditeur dans une époque (CryptorManager de libdave).
pub struct SenderKeys {
    ratchet: HashRatchet,
    created: Instant,
    ciphers: BTreeMap<u32, (FrameCipher, Option<Instant>)>,
    oldest: u32,
    newest: u32,
    newest_nonce: Option<u64>,
    missing: VecDeque<u64>,
}

impl SenderKeys {
    pub fn new(base: Vec<u8>, now: Instant) -> Self {
        SenderKeys {
            ratchet: HashRatchet::new(base),
            created: now,
            ciphers: BTreeMap::new(),
            oldest: 0,
            newest: 0,
            newest_nonce: None,
            missing: VecDeque::new(),
        }
    }

    fn can_process(&self, generation: u32, nonce: u32) -> bool {
        let expected = self.newest_nonce.map_or(0, |n| n + 1);
        let big = big_nonce(generation, nonce);
        big >= expected || self.missing.contains(&big)
    }

    fn cleanup(&mut self, now: Instant) {
        self.ciphers.retain(|_, (_, exp)| exp.is_none_or(|e| e >= now));
        while self.oldest < self.newest && !self.ciphers.contains_key(&self.oldest) {
            self.ratchet.erase(self.oldest);
            self.oldest += 1;
        }
    }

    fn cipher(&mut self, generation: u32, now: Instant) -> Option<&FrameCipher> {
        self.cleanup(now);
        if generation < self.oldest || generation > self.newest.saturating_add(MAX_GENERATION_GAP) {
            return None;
        }
        // Une génération ne peut pas avancer plus vite que le débit maximal de trames.
        let lifetime = now.saturating_duration_since(self.created).as_secs();
        if u64::from(generation) > (MAX_FRAMES_PER_SECOND * lifetime) >> GEN_SHIFT {
            return None;
        }
        if !self.ciphers.contains_key(&generation) {
            let key = self.ratchet.get(generation)?;
            let expiry = (generation < self.newest).then(|| now + EXPIRY);
            self.ciphers.insert(generation, (FrameCipher::new(&key).ok()?, expiry));
        }
        self.ciphers.get(&generation).map(|(c, _)| c)
    }

    fn report_success(&mut self, generation: u32, nonce: u32, now: Instant) {
        let expected = self.newest_nonce.map_or(0, |n| n + 1);
        let big = big_nonce(generation, nonce);
        if big >= expected {
            let gap = (big - expected).min(MAX_MISSING_NONCES as u64);
            while !self.missing.is_empty() && self.missing.len() + gap as usize > MAX_MISSING_NONCES {
                self.missing.pop_front();
            }
            self.missing.extend(big - gap..big);
            self.newest_nonce = Some(big);
        } else if let Some(i) = self.missing.iter().rposition(|&m| m == big) {
            self.missing.remove(i);
        }
        if generation > self.newest && self.ciphers.contains_key(&generation) {
            self.newest = generation;
            let exp = now + EXPIRY;
            for (g, (_, e)) in self.ciphers.iter_mut() {
                if *g < generation {
                    *e = Some(e.map_or(exp, |x| x.min(exp)));
                }
            }
        }
    }

    /// Essaie de déchiffrer ; chaque (clé, nonce) n'est accepté qu'une fois.
    pub fn decrypt(&mut self, p: &Parsed, now: Instant) -> Option<Vec<u8>> {
        let generation = wrapped_generation(self.oldest, p.nonce >> GEN_SHIFT);
        if !self.can_process(generation, p.nonce) {
            return None;
        }
        let (aad, mut ct) = p.split();
        self.cipher(generation, now)?.open(&full_nonce(p.nonce), &aad, &mut ct, p.tag).ok()?;
        self.report_success(generation, p.nonce, now);
        Some(p.rebuild(&ct))
    }
}

/// Clés de réception d'une époque, pour tous ses membres.
pub struct EpochKeys {
    root: Vec<u8>,
    members: Vec<u64>,
    expiry: Option<Instant>,
    senders: HashMap<u64, SenderKeys>,
}

impl EpochKeys {
    pub fn new(root: Vec<u8>, members: Vec<u64>) -> Self {
        EpochKeys { root, members, expiry: None, senders: HashMap::new() }
    }
}

/// Déchiffreur : époques récentes d'abord, les anciennes gardées 10 s.
#[derive(Default)]
pub struct Decryptor {
    epochs: VecDeque<EpochKeys>,
}

impl Decryptor {
    pub fn install(&mut self, keys: EpochKeys, now: Instant) {
        self.cleanup(now);
        if self.epochs.iter().any(|e| e.root == keys.root) {
            return;
        }
        self.expire_all(now);
        self.epochs.push_back(keys);
    }

    /// Les époques en place n'ont plus que 10 s à vivre.
    pub fn expire_all(&mut self, now: Instant) {
        let exp = now + EXPIRY;
        for e in &mut self.epochs {
            e.expiry = Some(e.expiry.map_or(exp, |x| x.min(exp)));
        }
    }

    fn cleanup(&mut self, now: Instant) {
        self.epochs.retain(|e| e.expiry.is_none_or(|x| x >= now));
    }

    pub fn decrypt(&mut self, user_id: u64, p: &Parsed, now: Instant) -> Option<Vec<u8>> {
        self.cleanup(now);
        for e in self.epochs.iter_mut().rev() {
            if !e.members.contains(&user_id) {
                continue;
            }
            let root = &e.root;
            let keys = e.senders.entry(user_id).or_insert_with(|| SenderKeys::new(sender_base(root, user_id), now));
            if let Some(out) = keys.decrypt(p, now) {
                return Some(out);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pair() -> (Encryptor, Decryptor, Instant) {
        let root = vec![9u8; 32];
        let mut e = Encryptor::default();
        e.set_base(Some(sender_base(&root, 42)));
        let mut d = Decryptor::default();
        let now = Instant::now();
        d.install(EpochKeys::new(root, vec![42]), now);
        (e, d, now)
    }

    #[test]
    fn aller_retour_opus() {
        let (mut e, mut d, now) = pair();
        for i in 0..5u8 {
            let opus = vec![i; 60 + i as usize];
            let f = e.encrypt(&opus).unwrap();
            assert_eq!(&f[f.len() - 2..], &MARKER);
            assert_ne!(&f[..opus.len()], opus.as_slice());
            let p = parse(&f).unwrap();
            assert_eq!(d.decrypt(42, &p, now).unwrap(), opus);
        }
    }

    #[test]
    fn trame_alteree_rejouee_ou_d_un_autre_refusee() {
        let (mut e, mut d, now) = pair();
        let f = e.encrypt(b"bonjour").unwrap();
        let mut bad = f.clone();
        bad[2] ^= 0x40;
        assert!(d.decrypt(42, &parse(&bad).unwrap(), now).is_none());
        // Mauvais expéditeur : clé différente.
        assert!(d.decrypt(7, &parse(&f).unwrap(), now).is_none());
        assert!(d.decrypt(42, &parse(&f).unwrap(), now).is_some());
        // Rejeu du même nonce.
        assert!(d.decrypt(42, &parse(&f).unwrap(), now).is_none());
    }

    #[test]
    fn desordre_tolere() {
        let (mut e, mut d, now) = pair();
        let a = e.encrypt(b"a").unwrap();
        let b = e.encrypt(b"b").unwrap();
        let c = e.encrypt(b"c").unwrap();
        assert_eq!(d.decrypt(42, &parse(&c).unwrap(), now).unwrap(), b"c");
        assert_eq!(d.decrypt(42, &parse(&a).unwrap(), now).unwrap(), b"a");
        assert_eq!(d.decrypt(42, &parse(&b).unwrap(), now).unwrap(), b"b");
    }

    #[test]
    fn plages_en_clair_relues() {
        // Trame VP8 simulée : 1 octet en clair au début, le reste chiffré.
        let key = [5u8; 16];
        let c = FrameCipher::new(&key).unwrap();
        let mut ct = b"donnees video".to_vec();
        let tag = c.seal(&full_nonce(1), &[0x9d], &mut ct).unwrap();
        let mut f = vec![0x9d];
        f.extend_from_slice(&ct);
        f.extend_from_slice(&tag);
        let mut meta = Vec::new();
        uleb128_write(1, &mut meta);
        uleb128_write(0, &mut meta);
        uleb128_write(1, &mut meta);
        f.extend_from_slice(&meta);
        f.push((TAG_LEN + meta.len() + 3) as u8);
        f.extend_from_slice(&MARKER);
        let p = parse(&f).unwrap();
        assert_eq!(p.ranges, vec![(0, 1)]);
        let (aad, mut body) = p.split();
        c.open(&full_nonce(1), &aad, &mut body, p.tag).unwrap();
        assert_eq!(p.rebuild(&body), [&[0x9d][..], b"donnees video"].concat());
    }

    #[test]
    fn trames_non_protocolaires() {
        assert!(parse(&SILENCE).is_none());
        assert!(parse(b"opus en clair sans marqueur").is_none());
        // Marqueur présent mais taille de suffixe absurde.
        assert!(parse(&[1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 0xff, 0xfa, 0xfa]).is_none());
    }

    #[test]
    fn generation_suit_l_octet_de_poids_fort() {
        let mut r1 = HashRatchet::new(vec![1u8; 16]);
        let mut r2 = HashRatchet::new(vec![1u8; 16]);
        let k3 = r1.get(3).unwrap();
        assert_eq!(r2.get(3).unwrap(), k3);
        assert_ne!(r2.get(2).unwrap(), k3);
        r1.erase(3);
        assert!(r1.get(3).is_none());
        assert_eq!(wrapped_generation(0, 1), 1);
        assert_eq!(wrapped_generation(255, 0), 256);
        assert_eq!(wrapped_generation(256, 3), 259);
    }
}
