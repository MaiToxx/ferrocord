//! Moteur audio du vocal, sans périphérique : la capture y entre en
//! échantillons et en sort en trames Opus ; les trames reçues y entrent par
//! personne et la lecture en tire le mixage stéréo.
//!
//! Il se coupe en deux moitiés indépendantes, [`Capture`] et [`Playback`],
//! pour vivre sur les deux fils audio sans verrou : le son joué (référence
//! de l'annulation d'écho) passe de l'une à l'autre par une file sans
//! verrou (`audio::io::ring`). [`Engine`] les réunit (tests, banc).
//!
//! Côté lecture, tout est préalloué : `pull_playback` n'alloue qu'au moment
//! rare où une personne silencieuse est purgée (libération) ; `receive` alloue
//! seulement à l'arrivée d'une nouvelle personne.

mod codec;
mod jitter;
mod mix;
#[cfg(test)]
pub(crate) mod tests;

pub use codec::{FRAME, MAX_PACKET, SAMPLE_RATE, SILENCE};
/// Décodeur Opus des sons courts (`audio::clip`).
pub use codec::{MAX_DECODED, VoiceDecoder};

use crate::audio::dsp::level::{rms_db, VoiceGate, FLOOR_DB};
use codec::VoiceEncoder;
use jitter::{Jitter, Next};
use mix::Limiter;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Trames de silence envoyées à chaque arrêt de l'émission (demandé par Discord).
const TRAILING_SILENCE: usize = 5;
/// Personne purgée après 5 s sans rien à jouer.
const PURGE_FRAMES: u32 = 250;
/// Une personne « parle » si une trame non silencieuse est arrivée depuis moins que ça.
const SPEAKING_WINDOW: Duration = Duration::from_millis(250);
/// Borne du nombre de personnes suivies (un salon vocal en compte 99 au plus).
const MAX_PEERS: usize = 128;
const MAX_PREFS: usize = 1024;

/// Prise sur le micro (enregistrement local d'un appel, lot E4) : chaque
/// trame telle qu'elle est émise ; une tranche vide quand rien ne part
/// (micro coupé, porte fermée).
pub type MicTap = Box<dyn FnMut(&[f32]) + Send>;
/// Prise sur les voix reçues : `Some(personne)` et sa trame stéréo décodée
/// (avant volume, muet local et sourdine), puis `None` à la fin de chaque
/// trame mixée (l'horloge de l'enregistrement).
pub type PeerTap = Box<dyn FnMut(Option<u64>, &[f32]) + Send>;

/// Ouverture de la capture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gate {
    /// Détection de voix (la porte de `dsp::level`, la même que celle du test
    /// du micro) : ouverte au-dessus du seuil (dBFS), avec maintien ; `auto` :
    /// seuil déduit du bruit de fond.
    Voice { auto: bool, threshold_db: f32 },
    /// Appui-pour-parler : ouverte tant que la touche est enfoncée.
    PushToTalk { pressed: bool },
}

impl Default for Gate {
    fn default() -> Self {
        Gate::Voice { auto: false, threshold_db: -50.0 }
    }
}

#[derive(Clone, Copy)]
struct Prefs {
    volume: f32,
    muted: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self { volume: 1.0, muted: false }
    }
}

struct Peer {
    jitter: Jitter,
    dec: VoiceDecoder,
    /// Échantillons décodés pas encore mixés (paquets plus longs que 20 ms).
    pend: Vec<f32>,
    pend_pos: usize,
    pend_len: usize,
    idle: u32,
    last_voice: Option<Instant>,
}

impl Peer {
    fn new() -> Option<Self> {
        Some(Self {
            jitter: Jitter::new(),
            dec: VoiceDecoder::new()?,
            pend: vec![0.0; MAX_DECODED * 2],
            pend_pos: 0,
            pend_len: 0,
            idle: 0,
            last_voice: None,
        })
    }

    /// Remplit `out` (une trame stéréo) ; rend faux si la personne n'a rien à jouer.
    fn render(&mut self, out: &mut [f32]) -> bool {
        let mut filled = 0;
        // Borne : chaque tour consomme un paquet ou une trame de masquage.
        for _ in 0..8 {
            if self.pend_pos < self.pend_len {
                let n = (self.pend_len - self.pend_pos).min(out.len() - filled);
                out[filled..filled + n].copy_from_slice(&self.pend[self.pend_pos..self.pend_pos + n]);
                filled += n;
                self.pend_pos += n;
            }
            if filled == out.len() {
                break;
            }
            let n = match self.jitter.pop() {
                Next::Packet(p) => self.dec.decode(p, &mut self.pend),
                Next::Recover(p) => self.dec.recover(p, &mut self.pend),
                Next::Conceal => self.dec.conceal(&mut self.pend),
                Next::Silent => break,
            };
            self.pend_pos = 0;
            self.pend_len = n * 2;
        }
        out[filled..].fill(0.0);
        if filled == 0 {
            self.pend_len = 0;
            self.idle += 1;
            false
        } else {
            self.idle = 0;
            true
        }
    }
}

/// Moitié capture : traitement, porte, encodage.
pub struct Capture {
    enc: Option<VoiceEncoder>,
    gate: Gate,
    vad: VoiceGate,
    muted: bool,
    cap: Vec<f32>,
    cap_len: usize,
    level_db: f32,
    open: bool,
    processor: Option<Box<dyn crate::audio::Processor>>,
    far_end: Vec<f32>,
    tap: Option<MicTap>,
}

impl Default for Capture {
    fn default() -> Self {
        Self::new()
    }
}

impl Capture {
    pub fn new() -> Self {
        let mut c = Self {
            enc: VoiceEncoder::new(),
            gate: Gate::default(),
            vad: VoiceGate::new(false, -50.0),
            muted: false,
            cap: vec![0.0; FRAME],
            cap_len: 0,
            level_db: FLOOR_DB,
            open: false,
            processor: None,
            far_end: vec![0.0; FRAME],
            tap: None,
        };
        c.set_gate(Gate::default());
        c
    }

    /// Trames Opus prêtes à envoyer ; `far` remplit, avant chaque trame, la
    /// référence d'écho (dernier son joué, mono, 960 échantillons).
    pub fn push_with(&mut self, mono: &[f32], mut far: impl FnMut(&mut [f32])) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        let mut rest = mono;
        while !rest.is_empty() {
            let n = (FRAME - self.cap_len).min(rest.len());
            self.cap[self.cap_len..self.cap_len + n].copy_from_slice(&rest[..n]);
            self.cap_len += n;
            rest = &rest[n..];
            if self.cap_len == FRAME {
                self.cap_len = 0;
                far(&mut self.far_end);
                self.frame(&mut out);
            }
        }
        out
    }

    fn frame(&mut self, out: &mut Vec<Vec<u8>>) {
        if let Some(p) = &mut self.processor {
            p.process(&mut self.cap, &self.far_end);
        }
        self.level_db = rms_db(&self.cap);
        let want = match self.gate {
            Gate::Voice { .. } => self.vad.update(self.level_db),
            Gate::PushToTalk { pressed } => pressed,
        };
        if let Some(tap) = &mut self.tap {
            // Enregistrement local : ce qui est émis, sinon un silence.
            if want && !self.muted { tap(&self.cap) } else { tap(&[]) }
        }
        if self.muted {
            self.open = false;
            return;
        }
        if want {
            // libopus : un paquet de 2 octets ou moins est une trame DTX à ne pas envoyer.
            if let Some(f) = self.enc.as_mut().and_then(|e| e.encode(&self.cap))
                && f.len() > 2
            {
                out.push(f);
            }
        } else if self.open {
            out.extend((0..TRAILING_SILENCE).map(|_| SILENCE.to_vec()));
        }
        self.open = want;
    }

    pub fn set_gate(&mut self, g: Gate) {
        if let Gate::Voice { auto, threshold_db } = g {
            // Le bruit de fond appris est gardé : seul le seuil change.
            self.vad.auto = auto;
            self.vad.threshold_db = threshold_db;
        }
        self.gate = g;
        // En détection de voix, la DTX allège les fins de mots maintenues ;
        // en appui-pour-parler, on émet tel quel ce que la personne veut dire.
        if let Some(e) = &mut self.enc {
            e.set_dtx(matches!(g, Gate::Voice { .. }));
        }
    }

    pub fn set_muted(&mut self, b: bool) {
        self.muted = b;
    }

    /// Niveau de la dernière trame capturée (après traitement), en dBFS.
    pub fn input_level_db(&self) -> f32 {
        self.level_db
    }

    /// Seuil de détection de voix en vigueur (réglé ou automatique).
    pub fn threshold_db(&self) -> f32 {
        self.vad.threshold()
    }

    /// Vrai si la dernière trame capturée a été émise.
    pub fn speaking_now(&self) -> bool {
        self.open
    }

    pub fn set_processor(&mut self, p: Option<Box<dyn crate::audio::Processor>>) {
        self.processor = p;
    }

    /// Pose ou retire la prise de l'enregistrement local.
    pub fn set_tap(&mut self, tap: Option<MicTap>) {
        self.tap = tap;
    }
}

/// Moitié lecture : tampons de gigue, décodage, mixage.
pub struct Playback {
    deafened: bool,
    peers: HashMap<u64, Peer>,
    prefs: HashMap<u64, Prefs>,
    mix: Vec<f32>,
    mix_pos: usize,
    scratch: Vec<f32>,
    /// Égaliseur des voix (plugin, lot E4) ; à plat, il ne traite rien.
    eq: crate::audio::dsp::eq::Eq3,
    limiter: Limiter,
    far_end: Vec<f32>,
    tap: Option<PeerTap>,
}

impl Default for Playback {
    fn default() -> Self {
        Self::new()
    }
}

impl Playback {
    pub fn new() -> Self {
        Self {
            deafened: false,
            peers: HashMap::new(),
            prefs: HashMap::new(),
            mix: vec![0.0; FRAME * 2],
            mix_pos: FRAME * 2,
            scratch: vec![0.0; FRAME * 2],
            eq: crate::audio::dsp::eq::Eq3::new(),
            limiter: Limiter::default(),
            far_end: vec![0.0; FRAME],
            tap: None,
        }
    }

    pub fn receive(&mut self, user_id: u64, seq: u16, timestamp: u32, opus: &[u8]) {
        self.receive_at(user_id, seq, timestamp, opus, Instant::now());
    }

    // Le numéro de séquence suffit à ordonner ; l'horodatage saute pendant les
    // pauses (aucune trame envoyée) sans que ce soit une perte.
    fn receive_at(&mut self, user_id: u64, seq: u16, _timestamp: u32, opus: &[u8], now: Instant) {
        if !self.peers.contains_key(&user_id) {
            if self.peers.len() >= MAX_PEERS {
                return;
            }
            let Some(p) = Peer::new() else { return };
            self.peers.insert(user_id, p);
        }
        let Some(peer) = self.peers.get_mut(&user_id) else { return };
        peer.jitter.insert(seq, opus);
        peer.idle = 0;
        // Trames de 3 octets ou moins : silence ou DTX, pas de la parole.
        if opus.len() > SILENCE.len() {
            peer.last_voice = Some(now);
        }
    }

    /// Mixage de toutes les personnes, stéréo entrelacé, de longueur quelconque.
    pub fn pull_playback(&mut self, stereo: &mut [f32]) {
        let mut filled = 0;
        while filled < stereo.len() {
            if self.mix_pos == self.mix.len() {
                self.render_mix();
                self.mix_pos = 0;
            }
            let n = (self.mix.len() - self.mix_pos).min(stereo.len() - filled);
            stereo[filled..filled + n].copy_from_slice(&self.mix[self.mix_pos..self.mix_pos + n]);
            filled += n;
            self.mix_pos += n;
        }
    }

    fn render_mix(&mut self) {
        self.mix.fill(0.0);
        for (id, peer) in self.peers.iter_mut() {
            // On consomme même sans l'entendre : le tampon reste à l'heure.
            if !peer.render(&mut self.scratch) {
                continue;
            }
            if let Some(tap) = &mut self.tap {
                tap(Some(*id), &self.scratch);
            }
            if self.deafened {
                continue;
            }
            let p = self.prefs.get(id).copied().unwrap_or_default();
            if p.muted {
                continue;
            }
            for (m, s) in self.mix.iter_mut().zip(&self.scratch) {
                *m += s * p.volume;
            }
        }
        self.peers.retain(|_, p| p.idle < PURGE_FRAMES);
        // Avant le limiteur : une bande poussée ne sature pas la sortie.
        self.eq.process(&mut self.mix);
        self.limiter.apply(&mut self.mix);
        for (f, s) in self.far_end.iter_mut().zip(self.mix.as_chunks::<2>().0) {
            *f = (s[0] + s[1]) * 0.5;
        }
        if let Some(tap) = &mut self.tap {
            tap(None, &[]);
        }
    }

    /// Pose ou retire la prise de l'enregistrement local.
    pub fn set_tap(&mut self, tap: Option<PeerTap>) {
        self.tap = tap;
    }

    pub fn set_deafened(&mut self, b: bool) {
        self.deafened = b;
    }

    /// Égaliseur des voix : grave, médium, aigu, en dB (tout à 0 : aucun traitement).
    pub fn set_eq(&mut self, gains_db: [i8; 3]) {
        self.eq.set(gains_db);
    }

    pub fn set_user_volume(&mut self, user_id: u64, v: f32) {
        let v = if v.is_finite() { v.clamp(0.0, 2.0) } else { 1.0 };
        self.update_prefs(user_id, |p| p.volume = v);
    }

    pub fn set_user_muted(&mut self, user_id: u64, b: bool) {
        self.update_prefs(user_id, |p| p.muted = b);
    }

    fn update_prefs(&mut self, user_id: u64, f: impl FnOnce(&mut Prefs)) {
        let mut p = self.prefs.get(&user_id).copied().unwrap_or_default();
        f(&mut p);
        if p.volume == 1.0 && !p.muted {
            self.prefs.remove(&user_id);
        } else if self.prefs.len() < MAX_PREFS || self.prefs.contains_key(&user_id) {
            self.prefs.insert(user_id, p);
        }
    }

    pub fn speaking_users(&self) -> Vec<u64> {
        self.speaking_users_at(Instant::now())
    }

    fn speaking_users_at(&self, now: Instant) -> Vec<u64> {
        let mut v: Vec<u64> = self
            .peers
            .iter()
            .filter(|(_, p)| p.last_voice.is_some_and(|t| now.saturating_duration_since(t) < SPEAKING_WINDOW))
            .map(|(id, _)| *id)
            .collect();
        v.sort_unstable();
        v
    }
}

/// Les deux moitiés réunies, sur un seul fil.
#[derive(Default)]
pub struct Engine {
    cap: Capture,
    play: Playback,
}

impl Engine {
    pub fn new() -> Self {
        Self { cap: Capture::new(), play: Playback::new() }
    }

    /// Les deux moitiés, pour les fils de capture et de lecture.
    pub fn split(self) -> (Capture, Playback) {
        (self.cap, self.play)
    }

    /// Trames Opus prêtes à envoyer (après traitement et porte).
    pub fn push_capture(&mut self, mono: &[f32]) -> Vec<Vec<u8>> {
        let far = &self.play.far_end;
        self.cap.push_with(mono, |f| f.copy_from_slice(far))
    }

    pub fn receive(&mut self, user_id: u64, seq: u16, timestamp: u32, opus: &[u8]) {
        self.play.receive(user_id, seq, timestamp, opus);
    }

    #[cfg(test)]
    fn receive_at(&mut self, user_id: u64, seq: u16, timestamp: u32, opus: &[u8], now: Instant) {
        self.play.receive_at(user_id, seq, timestamp, opus, now);
    }

    /// Mixage de toutes les personnes, stéréo entrelacé, de longueur quelconque.
    pub fn pull_playback(&mut self, stereo: &mut [f32]) {
        self.play.pull_playback(stereo);
    }

    pub fn set_gate(&mut self, g: Gate) {
        self.cap.set_gate(g);
    }

    pub fn set_muted(&mut self, b: bool) {
        self.cap.set_muted(b);
    }

    pub fn set_deafened(&mut self, b: bool) {
        self.play.set_deafened(b);
    }

    pub fn set_user_volume(&mut self, user_id: u64, v: f32) {
        self.play.set_user_volume(user_id, v);
    }

    pub fn set_user_muted(&mut self, user_id: u64, b: bool) {
        self.play.set_user_muted(user_id, b);
    }

    /// Niveau de la dernière trame capturée (après traitement), en dBFS.
    pub fn input_level_db(&self) -> f32 {
        self.cap.input_level_db()
    }

    /// Vrai si la dernière trame capturée a été émise.
    pub fn speaking_now(&self) -> bool {
        self.cap.speaking_now()
    }

    pub fn speaking_users(&self) -> Vec<u64> {
        self.play.speaking_users()
    }

    #[cfg(test)]
    fn speaking_users_at(&self, now: Instant) -> Vec<u64> {
        self.play.speaking_users_at(now)
    }

    pub fn set_processor(&mut self, p: Option<Box<dyn crate::audio::Processor>>) {
        self.cap.set_processor(p);
    }
}
