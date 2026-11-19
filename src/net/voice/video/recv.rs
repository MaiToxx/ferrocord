//! Réception d'un flux vidéo : remise en ordre, NACK des trous, images
//! rendues complètes (du paquet suivant le dernier marqueur au marqueur
//! suivant), abandon d'une image trop en retard puis demande d'image clé
//! (PLI). Tout se fait à l'arrivée d'un paquet : aucune minuterie.

use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use super::{av1, h264, vp8, vp9};
use crate::video::Codec;

/// Trou plus long : le flux repart de zéro (et attend une image clé).
const MAX_GAP: u64 = 512;
/// Paquets en attente au plus.
const MAX_BUFFER: usize = 1024;
/// Une image incomplète plus vieille que la plus récente de 500 ms est abandonnée.
const GIVE_UP_90K: u32 = 45_000;
const NACK_TRIES: u8 = 3;
const NACK_RETRY: Duration = Duration::from_millis(100);
const PLI_EVERY: Duration = Duration::from_millis(500);

/// Paquet reçu (RTX déjà déballé).
#[derive(Clone, Debug)]
pub struct Incoming {
    pub seq: u16,
    pub ts: u32,
    pub marker: bool,
    pub payload: Vec<u8>,
}

/// Image complète, encore chiffrée de bout en bout le cas échéant.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Assembled {
    pub ts: u32,
    pub keyframe: bool,
    pub data: Vec<u8>,
}

#[derive(Default, Debug)]
pub struct Output {
    pub frames: Vec<Assembled>,
    /// Numéros à redemander.
    pub nack: Vec<u16>,
    /// Image clé à demander.
    pub pli: bool,
}

struct Missing {
    tries: u8,
    last: Option<Instant>,
}

pub struct Receiver {
    codec: Codec,
    buf: BTreeMap<u64, Incoming>,
    /// Premier paquet de la prochaine image ; `None` : en attente d'un marqueur.
    next: Option<u64>,
    highest: Option<u64>,
    missing: BTreeMap<u64, Missing>,
    need_key: bool,
    last_pli: Option<Instant>,
}

impl Receiver {
    pub fn new(codec: Codec) -> Self {
        Receiver { codec, buf: BTreeMap::new(), next: None, highest: None, missing: BTreeMap::new(), need_key: true, last_pli: None }
    }

    pub fn codec(&self) -> Codec {
        self.codec
    }

    /// Le décodeur a perdu le fil : on jette jusqu'à la prochaine image clé.
    pub fn want_keyframe(&mut self) {
        self.need_key = true;
    }

    fn extend(&self, seq: u16) -> u64 {
        match self.highest {
            // Base décalée : un paquet un peu plus ancien que le premier reste représentable.
            None => (1 << 32) + u64::from(seq),
            Some(h) => {
                let d = i64::from(seq.wrapping_sub(h as u16) as i16);
                (h as i64 + d).max(0) as u64
            }
        }
    }

    fn reset(&mut self) {
        self.buf.clear();
        self.missing.clear();
        self.next = None;
        self.need_key = true;
    }

    pub fn push(&mut self, p: Incoming, now: Instant, out: &mut Output) {
        let ext = self.extend(p.seq);
        if self.next.is_some_and(|n| ext < n) || self.buf.contains_key(&ext) {
            return;
        }
        match self.highest {
            Some(h) if ext > h + MAX_GAP => {
                self.reset();
                self.highest = Some(ext);
            }
            Some(h) if ext > h => {
                for s in h + 1..ext {
                    self.missing.insert(s, Missing { tries: 0, last: None });
                }
                self.highest = Some(ext);
            }
            Some(_) => {}
            None => self.highest = Some(ext),
        }
        self.missing.remove(&ext);
        let marker = p.marker;
        self.buf.insert(ext, p);
        if self.next.is_none() && marker {
            // Début de flux (ou reprise) : on se cale sur la fin d'une image.
            self.drop_through(ext);
        }
        self.assemble(out);
        self.give_up(out);
        if self.buf.len() > MAX_BUFFER {
            self.reset();
        }
        let missing_ok = |m: &Missing| m.tries < NACK_TRIES && m.last.is_none_or(|t| now.saturating_duration_since(t) >= NACK_RETRY);
        for (s, m) in self.missing.iter_mut() {
            if missing_ok(m) {
                m.tries += 1;
                m.last = Some(now);
                out.nack.push(*s as u16);
            }
        }
        if self.need_key && self.last_pli.is_none_or(|t| now.saturating_duration_since(t) >= PLI_EVERY) {
            self.last_pli = Some(now);
            out.pli = true;
        }
    }

    /// Oublie tout jusqu'au paquet `ext` compris ; l'image suivante commence après.
    fn drop_through(&mut self, ext: u64) {
        self.buf = self.buf.split_off(&(ext + 1));
        self.missing = self.missing.split_off(&(ext + 1));
        self.next = Some(ext + 1);
    }

    fn assemble(&mut self, out: &mut Output) {
        while let Some(n) = self.next {
            let mut s = n;
            let end = loop {
                match self.buf.get(&s) {
                    None => break None,
                    Some(p) if p.marker => break Some(s),
                    Some(_) => s += 1,
                }
            };
            let Some(end) = end else { return };
            let parts: Vec<Incoming> = (n..=end).filter_map(|k| self.buf.remove(&k)).collect();
            self.next = Some(end + 1);
            let ts = parts[0].ts;
            let data = if parts.iter().all(|p| p.ts == ts) { self.depacketize(&parts) } else { None };
            match data {
                Some((data, keyframe)) => {
                    if keyframe {
                        self.need_key = false;
                    }
                    if !self.need_key {
                        out.frames.push(Assembled { ts, keyframe, data });
                    }
                }
                None => self.need_key = true,
            }
        }
    }

    /// Image et « image clé ». H.264 et VP8 : lu dans l'image (ces octets
    /// restent en clair sous DAVE) ; VP9 et AV1 : dans les en-têtes des paquets.
    fn depacketize(&self, parts: &[Incoming]) -> Option<(Vec<u8>, bool)> {
        let it = parts.iter().map(|p| p.payload.as_slice());
        match self.codec {
            Codec::H264 => h264::depacketize(it).map(|d| {
                let key = h264::is_keyframe(&d);
                (d, key)
            }),
            Codec::Vp8 => vp8::depacketize(it).map(|d| {
                let key = vp8::is_keyframe(&d);
                (d, key)
            }),
            Codec::Vp9 => vp9::depacketize(it),
            Codec::Av1 => av1::depacketize(it),
        }
    }

    /// Image bloquée par un trou trop ancien ou des NACK épuisés : sautée.
    fn give_up(&mut self, out: &mut Output) {
        loop {
            let Some(n) = self.next else { return };
            let Some((_, first)) = self.buf.range(n..).next() else { return };
            let newest = self.highest.and_then(|h| self.buf.get(&h)).map_or(first.ts, |p| p.ts);
            let stale = newest.wrapping_sub(first.ts) as i32 > GIVE_UP_90K as i32;

            if !stale {
                return;
            }
            let gap = (n..).find(|s| !self.buf.contains_key(s)).unwrap_or(n);
            let marker = self.buf.range(gap..).find(|(_, p)| p.marker).map(|(k, _)| *k);
            self.need_key = true;
            match marker {
                Some(m) => {
                    self.drop_through(m);
                    self.assemble(out);
                }
                None => {
                    self.buf.clear();
                    self.missing.clear();
                    self.next = None;
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Paquets VP8 de `frames` images de `size` octets ; la première est une image clé.
    fn stream(frames: u32, size: usize) -> Vec<Incoming> {
        let mut out = Vec::new();
        let mut seq = 0u16;
        for f in 0..frames {
            let mut data = vec![(f as u8) | 1; size];
            if f == 0 {
                data[0] = 0;
            }
            let p = vp8::packetize(&data, 500);
            let n = p.len();
            for (k, payload) in p.into_iter().enumerate() {
                out.push(Incoming { seq, ts: f * 3000, marker: k + 1 == n, payload });
                seq = seq.wrapping_add(1);
            }
        }
        out
    }

    fn feed(r: &mut Receiver, pkts: impl IntoIterator<Item = Incoming>, now: Instant) -> Output {
        let mut out = Output::default();
        for p in pkts {
            r.push(p, now, &mut out);
        }
        out
    }

    /// Le premier paquet porte un marqueur factice (fin d'une image précédente).
    fn primed(r: &mut Receiver, first_seq: u16, now: Instant) {
        feed(r, [Incoming { seq: first_seq.wrapping_sub(1), ts: 0, marker: true, payload: vec![0x10] }], now);
    }

    #[test]
    fn ordre_et_desordre() {
        let now = Instant::now();
        let pkts = stream(4, 1800);
        let mut r = Receiver::new(Codec::Vp8);
        primed(&mut r, 0, now);
        let mut shuffled = pkts.clone();
        shuffled.swap(1, 3);
        shuffled.swap(5, 6);
        let out = feed(&mut r, shuffled, now);
        assert_eq!(out.frames.len(), 4);
        assert!(out.frames[0].keyframe);
        assert_eq!(out.frames.iter().map(|f| f.ts).collect::<Vec<_>>(), vec![0, 3000, 6000, 9000]);
        assert_eq!(out.frames[2].data, vec![3u8; 1800]);
    }

    #[test]
    fn perte_nack_puis_retransmission() {
        let now = Instant::now();
        let pkts = stream(3, 1800);
        let mut r = Receiver::new(Codec::Vp8);
        primed(&mut r, 0, now);
        let lost = pkts[5].clone();
        let mut out = feed(&mut r, pkts.iter().filter(|p| p.seq != 5).cloned(), now);
        assert_eq!(out.nack, vec![5]);
        assert_eq!(out.frames.len(), 1, "la deuxième image attend le paquet perdu");
        // Pas de nouvelle demande avant 100 ms.
        let again = feed(&mut r, [], now);
        assert!(again.nack.is_empty());
        r.push(lost, now, &mut out);
        assert_eq!(out.frames.len(), 3);
        assert!(r.missing.is_empty());
    }

    #[test]
    fn image_abandonnee_puis_image_cle() {
        let t0 = Instant::now();
        let mut r = Receiver::new(Codec::Vp8);
        primed(&mut r, 0, t0);
        let pkts = stream(30, 600);
        // Le paquet 3 (deuxième image) ne revient jamais.
        let t1 = t0 + Duration::from_secs(1);
        let out = feed(&mut r, pkts.iter().filter(|p| p.seq != 3).cloned(), t1);
        assert!(out.pli, "PLI demandé");
        // Seule la première image passe : les suivantes ne sont pas des images clés.
        assert_eq!(out.frames.len(), 1);
        // Image clé suivante : le flux reprend.
        let mut key = vec![0u8; 600];
        key[1] = 9;
        let seq = 60u16;
        let mut out = Output::default();
        for (k, payload) in vp8::packetize(&key, 500).into_iter().enumerate() {
            r.push(Incoming { seq: seq + k as u16, ts: 400_000, marker: k == 1, payload }, t1, &mut out);
        }
        assert_eq!(out.frames.len(), 1);
        assert!(out.frames[0].keyframe);
    }

    #[test]
    fn numeros_qui_rebouclent_et_doublons() {
        let now = Instant::now();
        let mut pkts = stream(3, 1200);
        for p in &mut pkts {
            p.seq = p.seq.wrapping_add(65530);
        }
        let mut r = Receiver::new(Codec::Vp8);
        primed(&mut r, 65530, now);
        let mut all = pkts.clone();
        all.insert(2, pkts[1].clone());
        let out = feed(&mut r, all, now);
        assert_eq!(out.frames.len(), 3);
        assert!(out.nack.is_empty());
    }

    #[test]
    fn grand_saut_repart_de_zero() {
        let now = Instant::now();
        let mut r = Receiver::new(Codec::Vp8);
        primed(&mut r, 0, now);
        let now = now + Duration::from_secs(1);
        let mut out = feed(&mut r, [Incoming { seq: 2000, ts: 1, marker: false, payload: vec![0x10, 0] }], now);
        assert!(out.nack.is_empty(), "pas de NACK sur 2000 paquets");
        assert!(out.pli);
        r.push(Incoming { seq: 2001, ts: 1, marker: true, payload: vec![0x10, 0] }, now, &mut out);
        assert!(out.frames.is_empty());
    }
}
