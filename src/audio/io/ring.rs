//! File d'échantillons entre deux fils audio (un seul écrivain, un seul
//! lecteur), sans verrou ni allocation après la création : la capture y
//! dépose, la lecture y puise. Les `f32` sont rangés en bits dans des
//! atomiques (aucun `unsafe`).

use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

pub struct Ring {
    buf: Box<[AtomicU32]>,
    /// Échantillons écrits depuis la création (seul l'écrivain l'avance).
    head: AtomicUsize,
    /// Échantillons lus (seul le lecteur l'avance).
    tail: AtomicUsize,
}

impl Ring {
    pub fn new(capacity: usize) -> Self {
        Ring { buf: (0..capacity.max(1)).map(|_| AtomicU32::new(0)).collect(), head: AtomicUsize::new(0), tail: AtomicUsize::new(0) }
    }

    pub fn len(&self) -> usize {
        self.head.load(Ordering::Acquire).wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Écrivain : ajoute ce qui tient ; le reste est perdu. Renvoie le nombre écrit.
    pub fn push(&self, data: &[f32]) -> usize {
        let cap = self.buf.len();
        let head = self.head.load(Ordering::Relaxed);
        let free = cap - head.wrapping_sub(self.tail.load(Ordering::Acquire));
        let n = data.len().min(free);
        for (i, s) in data[..n].iter().enumerate() {
            self.buf[head.wrapping_add(i) % cap].store(s.to_bits(), Ordering::Relaxed);
        }
        self.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Lecteur : remplit `out` avec ce qui est disponible. Renvoie le nombre lu.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let cap = self.buf.len();
        let tail = self.tail.load(Ordering::Relaxed);
        let n = out.len().min(self.head.load(Ordering::Acquire).wrapping_sub(tail));
        for (i, d) in out[..n].iter_mut().enumerate() {
            *d = f32::from_bits(self.buf[tail.wrapping_add(i) % cap].load(Ordering::Relaxed));
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
        n
    }

    /// Lecteur : jette les plus anciens pour n'en garder que `keep` (retard borné).
    pub fn trim_to(&self, keep: usize) {
        let tail = self.tail.load(Ordering::Relaxed);
        let len = self.head.load(Ordering::Acquire).wrapping_sub(tail);
        if len > keep {
            self.tail.store(tail.wrapping_add(len - keep), Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_order_overflow_and_trim() {
        let r = Ring::new(4);
        assert_eq!(r.push(&[1.0, 2.0, 3.0]), 3);
        assert_eq!(r.push(&[4.0, 5.0]), 1, "plein : le reste est perdu");
        let mut out = [0.0; 3];
        assert_eq!(r.pop(&mut out), 3);
        assert_eq!(out, [1.0, 2.0, 3.0]);
        assert_eq!(r.push(&[6.0, 7.0, 8.0]), 3);
        r.trim_to(2);
        assert_eq!(r.pop(&mut out), 2);
        assert_eq!(&out[..2], &[7.0, 8.0]);
        assert!(r.is_empty());
    }

    #[test]
    fn two_threads() {
        let r = std::sync::Arc::new(Ring::new(64));
        let w = r.clone();
        let writer = std::thread::spawn(move || {
            let mut next = 0.0f32;
            while next < 10_000.0 {
                if w.push(&[next]) == 1 {
                    next += 1.0;
                }
            }
        });
        let mut expect = 0.0f32;
        let mut out = [0.0; 16];
        while expect < 10_000.0 {
            let n = r.pop(&mut out);
            for s in &out[..n] {
                assert_eq!(*s, expect);
                expect += 1.0;
            }
        }
        writer.join().unwrap();
    }
}
