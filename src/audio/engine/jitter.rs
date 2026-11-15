//! Tampon de gigue d'une personne : remet les paquets dans l'ordre du numéro
//! de séquence RTP (bouclage u16), retient un délai adaptatif borné et signale
//! les trous à combler (FEC si le paquet suivant est là, sinon PLC).
//!
//! Tout est préalloué : ni `insert` ni `pop` n'allouent.

/// Capacité en paquets (320 ms de trames de 20 ms) : au-delà, on rattrape.
pub const SLOTS: usize = 16;
/// Délai visé, en trames : 40 ms à 200 ms.
pub const MIN_DELAY: usize = 2;
pub const MAX_DELAY: usize = 10;
/// Plus long paquet accepté (une datagramme UDP ordinaire).
pub const MAX_PACKET_BYTES: usize = 1500;
/// Trames masquées par PLC quand le flux s'interrompt, avant de se taire.
const MAX_CONCEAL: u32 = 5;
/// Trames sans incident avant de raccourcir le délai d'une trame (10 s).
const STABLE_FRAMES: u32 = 500;
/// Excès de délai toléré avant de jeter des trames pour rattraper.
const CATCH_UP_MARGIN: usize = 3;
/// Saut arrière au-delà duquel on considère que le flux a recommencé.
const RESTART_BACKWARD: i32 = 1000;

struct Slot {
    full: bool,
    seq: u16,
    len: usize,
    data: [u8; MAX_PACKET_BYTES],
}

/// Ce que la lecture doit produire pour la trame suivante.
#[derive(Debug, PartialEq)]
pub enum Next<'a> {
    Packet(&'a [u8]),
    /// Paquet perdu ; voici le suivant, dont la FEC le reconstruit.
    Recover(&'a [u8]),
    /// Paquet perdu ou en retard, sans FEC : masquage.
    Conceal,
    /// Rien à jouer (personne silencieuse ou tampon en cours de remplissage).
    Silent,
}

pub struct Jitter {
    slots: Vec<Slot>,
    count: usize,
    next: u16,
    primed: bool,
    playing: bool,
    delay: usize,
    /// Trames écoulées tampon vide pendant la lecture (0 = pas de manque).
    starve: u32,
    /// Trames passées à attendre le remplissage avant de jouer.
    wait: usize,
    stable: u32,
    pub lost: u64,
    pub late: u64,
    pub dropped: u64,
}

impl Default for Jitter {
    fn default() -> Self {
        Self::new()
    }
}

impl Jitter {
    pub fn new() -> Self {
        let slots = (0..SLOTS).map(|_| Slot { full: false, seq: 0, len: 0, data: [0; MAX_PACKET_BYTES] }).collect();
        Self {
            slots,
            count: 0,
            next: 0,
            primed: false,
            playing: false,
            delay: MIN_DELAY + 1,
            starve: 0,
            wait: 0,
            stable: 0,
            lost: 0,
            late: 0,
            dropped: 0,
        }
    }

    #[cfg(test)]
    fn delay(&self) -> usize {
        self.delay
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.count
    }

    fn offset(&self, seq: u16) -> i32 {
        seq.wrapping_sub(self.next) as i16 as i32
    }

    fn grow(&mut self) {
        self.delay = (self.delay + 1).min(MAX_DELAY);
        self.stable = 0;
    }

    fn clear(&mut self) {
        for s in &mut self.slots {
            s.full = false;
        }
        self.count = 0;
    }

    /// Nombre de trames entre la prochaine à jouer et le plus récent paquet reçu.
    fn span(&self) -> usize {
        self.slots
            .iter()
            .filter(|s| s.full)
            .map(|s| self.offset(s.seq).max(0) as usize + 1)
            .max()
            .unwrap_or(0)
    }

    /// Jette la trame attendue (rattrapage).
    fn skip(&mut self) {
        let i = self.next as usize % SLOTS;
        if self.slots[i].full && self.slots[i].seq == self.next {
            self.slots[i].full = false;
            self.count -= 1;
        }
        self.dropped += 1;
        self.next = self.next.wrapping_add(1);
    }

    pub fn insert(&mut self, seq: u16, data: &[u8]) {
        if data.is_empty() || data.len() > MAX_PACKET_BYTES {
            return;
        }
        if !self.primed || (!self.playing && self.count == 0) {
            self.next = seq;
            self.primed = true;
        }
        let mut d = self.offset(seq);
        if d < -RESTART_BACKWARD {
            self.clear();
            self.playing = false;
            self.next = seq;
            d = 0;
        }
        // Pendant le remplissage, un paquet antérieur au premier reçu est encore jouable.
        if d < 0 && !self.playing && self.span() as i32 - d <= SLOTS as i32 {
            self.next = seq;
            d = 0;
        }
        if d < 0 {
            self.late += 1;
            self.grow();
            return;
        }
        if d as usize >= SLOTS {
            // Débordement : on garde les SLOTS dernières trames.
            while self.offset(seq) as usize >= SLOTS {
                self.skip();
            }
        }
        // Le flux reprend juste après un manque : c'était de la gigue, pas une pause.
        if self.starve > 0 {
            if self.starve <= MAX_CONCEAL {
                self.grow();
            }
            self.starve = 0;
        }
        let slot = &mut self.slots[seq as usize % SLOTS];
        if slot.full && slot.seq == seq {
            return;
        }
        if !slot.full {
            self.count += 1;
        }
        slot.full = true;
        slot.seq = seq;
        slot.len = data.len();
        slot.data[..data.len()].copy_from_slice(data);
    }

    pub fn pop(&mut self) -> Next<'_> {
        if !self.playing {
            if self.count == 0 {
                self.wait = 0;
                return Next::Silent;
            }
            // Un flux plus court que le délai (un mot bref) finit quand même par jouer.
            self.wait += 1;
            if self.span() < self.delay && self.wait < self.delay {
                return Next::Silent;
            }
            self.playing = true;
            self.wait = 0;
        }
        if self.count == 0 {
            // On ne fait pas avancer `next` : si le paquet n'était qu'en retard,
            // il sera joué à son arrivée et le délai aura grandi d'autant.
            self.starve += 1;
            if self.starve > MAX_CONCEAL {
                self.playing = false;
                self.starve = 0;
                return Next::Silent;
            }
            self.stable = 0;
            return Next::Conceal;
        }
        if self.span() > self.delay + CATCH_UP_MARGIN {
            self.skip();
        }
        self.stable += 1;
        if self.stable >= STABLE_FRAMES {
            self.stable = 0;
            self.delay = (self.delay - 1).max(MIN_DELAY);
        }
        let seq = self.next;
        self.next = seq.wrapping_add(1);
        let i = seq as usize % SLOTS;
        if self.slots[i].full && self.slots[i].seq == seq {
            self.count -= 1;
            let s = &mut self.slots[i];
            s.full = false;
            return Next::Packet(&s.data[..s.len]);
        }
        self.lost += 1;
        let j = self.next as usize % SLOTS;
        let s = &self.slots[j];
        if s.full && s.seq == self.next {
            return Next::Recover(&s.data[..s.len]);
        }
        Next::Conceal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pkt(seq: u16) -> [u8; 2] {
        seq.to_be_bytes()
    }

    fn seq_of(n: Next<'_>) -> Option<u16> {
        match n {
            Next::Packet(p) => Some(u16::from_be_bytes([p[0], p[1]])),
            _ => None,
        }
    }

    /// Joue jusqu'à obtenir des paquets ; rend la suite des numéros joués.
    fn drain(j: &mut Jitter, n: usize) -> Vec<Option<u16>> {
        let mut out = Vec::new();
        for _ in 0..n {
            match j.pop() {
                Next::Silent => out.push(None),
                Next::Conceal => out.push(Some(u16::MAX - 1)),
                Next::Recover(_) => out.push(Some(u16::MAX - 2)),
                p => out.push(seq_of(p)),
            }
        }
        out
    }

    #[test]
    fn reordonne_et_traverse_le_bouclage() {
        let mut j = Jitter::new();
        for s in [65534u16, 0, 65535, 2, 1] {
            j.insert(s, &pkt(s));
        }
        let played: Vec<u16> = drain(&mut j, 8).into_iter().flatten().take(5).collect();
        assert_eq!(played, vec![65534, 65535, 0, 1, 2]);
    }

    #[test]
    fn trou_comble_par_fec_ou_plc() {
        let mut j = Jitter::new();
        j.delay = 7;
        for s in [10u16, 12, 13, 16] {
            j.insert(s, &pkt(s));
        }
        assert_eq!(seq_of(j.pop()), Some(10));
        // 11 perdu, 12 présent : FEC.
        assert_eq!(j.pop(), Next::Recover(&pkt(12)));
        assert_eq!(seq_of(j.pop()), Some(12));
        assert_eq!(seq_of(j.pop()), Some(13));
        // 14 perdu, 15 absent aussi : PLC deux fois, puis 16.
        assert_eq!(j.pop(), Next::Conceal);
        assert_eq!(j.pop(), Next::Recover(&pkt(16)));
        assert_eq!(seq_of(j.pop()), Some(16));
        assert_eq!(j.lost, 3);
    }

    #[test]
    fn doublons_ignores_et_retard_rejete() {
        let mut j = Jitter::new();
        for s in [5u16, 5, 6, 7] {
            j.insert(s, &pkt(s));
        }
        assert_eq!(j.len(), 3);
        assert_eq!(seq_of(j.pop()), Some(5));
        let d = j.delay();
        j.insert(4, &pkt(4));
        assert_eq!(j.late, 1);
        assert_eq!(j.delay(), d + 1);
        assert_eq!(seq_of(j.pop()), Some(6));
    }

    #[test]
    fn debordement_rattrape_et_reste_borne() {
        let mut j = Jitter::new();
        for s in 0..100u16 {
            j.insert(s, &pkt(s));
            assert!(j.len() <= SLOTS);
        }
        // Les plus anciens ont été jetés ; la lecture rattrape vers le délai visé.
        let first = drain(&mut j, 1)[0].unwrap();
        assert!(first >= 100 - SLOTS as u16, "{first}");
        for _ in 0..SLOTS {
            j.pop();
        }
        assert!(j.dropped > 0);
    }

    #[test]
    fn attend_le_delai_puis_joue_un_flux_court() {
        let mut j = Jitter::new();
        j.insert(1, &pkt(1));
        let d = j.delay();
        let out = drain(&mut j, d);
        assert_eq!(out.iter().flatten().copied().collect::<Vec<_>>(), vec![1]);
    }

    #[test]
    fn manque_bref_allonge_le_delai_pause_longue_non() {
        let mut j = Jitter::new();
        let d0 = j.delay();
        for s in 0..4u16 {
            j.insert(s, &pkt(s));
        }
        drain(&mut j, 4);
        // Paquet 4 en retard de deux trames : PLC, puis il arrive et il est joué.
        assert_eq!(j.pop(), Next::Conceal);
        assert_eq!(j.pop(), Next::Conceal);
        j.insert(4, &pkt(4));
        assert_eq!(j.delay(), d0 + 1);
        assert_eq!(seq_of(j.pop()), Some(4));
        // Pause longue (fin de phrase) : on se tait sans allonger le délai.
        let out = drain(&mut j, 20);
        assert!(out[MAX_CONCEAL as usize..].iter().all(Option::is_none));
        j.insert(5, &pkt(5));
        assert_eq!(j.delay(), d0 + 1);
        let d = j.delay();
        assert!(drain(&mut j, d).contains(&Some(5)));
        // Le délai reste dans ses bornes même sous des retards répétés.
        for _ in 0..50 {
            j.insert(0, &pkt(0));
        }
        assert_eq!(j.delay(), MAX_DELAY);
    }

    #[test]
    fn delai_raccourci_quand_stable() {
        let mut j = Jitter::new();
        j.delay = MAX_DELAY;
        let mut s = 0u16;
        for _ in 0..(STABLE_FRAMES as usize * 3) {
            j.insert(s, &pkt(s));
            s = s.wrapping_add(1);
            j.pop();
        }
        assert!(j.delay() < MAX_DELAY);
        assert!(j.delay() >= MIN_DELAY);
    }
}
