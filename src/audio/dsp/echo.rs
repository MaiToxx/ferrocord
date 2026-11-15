//! Annulation d'écho, version légère : ce que le micro reprend du son joué.
//!
//! 1. Estimation du retard entre la trame lue et son écho (jusqu'à 250 ms),
//!    par corrélation des enveloppes par blocs de 1 ms : peu coûteux, et
//!    insensible à la coloration de la pièce.
//! 2. Filtre adaptatif NLMS de 512 coefficients (10,7 ms) placé à ce retard :
//!    il apprend le trajet haut-parleur → micro et retranche l'écho prévu.
//!    L'apprentissage est figé quand l'utilisateur parle en même temps
//!    (détecteur de Geigel), pour ne pas effacer sa voix.
//! 3. Atténuation du résidu (12 dB) quand seul l'écho est présent.
//!
//! Limites : un écho plus long que 10 ms après son arrivée (grande pièce
//! réverbérante) n'est que partiellement retranché ; le résidu est alors
//! surtout traité par l'atténuation et la suppression de bruit.
//! Tout est alloué à la création : rien n'alloue pendant le traitement.

/// Bloc d'analyse : 1 ms à 48 kHz.
pub const BLOCK: usize = 48;
const MAX_DELAY_BLOCKS: usize = 250;
const ENV_LEN: usize = MAX_DELAY_BLOCKS + 1;
const TAPS: usize = 512;
/// Le filtre commence un peu avant le retard estimé (incertitude d'un bloc).
const MARGIN: usize = 2 * BLOCK;
const HIST: usize = MAX_DELAY_BLOCKS * BLOCK + TAPS + BLOCK;
const MU: f32 = 0.5;
/// Lissage des enveloppes et de la corrélation (constante de temps 0,5 s).
const BETA: f32 = 0.002;
/// Blocs pendant lesquels le nouveau retard doit rester le meilleur avant d'être adopté.
const STABLE_BLOCKS: u32 = 100;
/// Son joué en dessous : rien à annuler.
const SILENT: f32 = 1e-4;
/// Voix locale plus forte que le son joué (crête) : double parole.
const GEIGEL: f32 = 1.0;
const TALK_HOLD_BLOCKS: u32 = 30;
const NLP_FLOOR: f32 = 0.25;

pub struct EchoCanceller {
    /// Son joué ; les `HIST` derniers échantillons se terminent à `end`.
    buf: Vec<f32>,
    end: usize,
    w: Vec<f32>,
    far_env: Vec<f32>,
    corr: Vec<f32>,
    block: usize,
    mean_near: f32,
    mean_far: f32,
    delay: usize,
    candidate: usize,
    stable: u32,
    talk_hold: u32,
    nlp_gain: f32,
}

impl Default for EchoCanceller {
    fn default() -> Self {
        Self::new()
    }
}

impl EchoCanceller {
    pub fn new() -> Self {
        EchoCanceller {
            buf: vec![0.0; 2 * HIST],
            end: HIST,
            w: vec![0.0; TAPS],
            far_env: vec![0.0; ENV_LEN],
            corr: vec![0.0; ENV_LEN],
            block: 0,
            mean_near: 0.0,
            mean_far: 0.0,
            delay: 0,
            candidate: 0,
            stable: 0,
            talk_hold: 0,
            nlp_gain: 1.0,
        }
    }

    /// Oublie le trajet appris (sans allouer).
    pub fn reset(&mut self) {
        self.buf.fill(0.0);
        self.end = HIST;
        self.w.fill(0.0);
        self.far_env.fill(0.0);
        self.corr.fill(0.0);
        (self.block, self.mean_near, self.mean_far, self.delay, self.candidate, self.stable, self.talk_hold, self.nlp_gain) = (0, 0.0, 0.0, 0, 0, 0, 0, 1.0);
    }

    /// Retard estimé de l'écho, en échantillons.
    pub fn delay_samples(&self) -> usize {
        self.delay * BLOCK
    }

    /// `near` : trame captée, traitée sur place par blocs de 1 ms (un reste
    /// plus court passe tel quel) ; `far` : son joué au même moment (plus
    /// court : complété par du silence).
    pub fn process(&mut self, near: &mut [f32], far: &[f32]) {
        for (b, chunk) in near.as_chunks_mut::<BLOCK>().0.iter_mut().enumerate() {
            let mut fb = [0.0f32; BLOCK];
            if let Some(src) = far.get(b * BLOCK..) {
                let n = src.len().min(BLOCK);
                fb[..n].copy_from_slice(&src[..n]);
            }
            self.push_far(&fb);
            self.track_delay(&chunk[..], &fb);
            self.cancel(&mut chunk[..]);
        }
    }

    fn push_far(&mut self, fb: &[f32; BLOCK]) {
        if self.end + BLOCK > self.buf.len() {
            self.buf.copy_within(self.end - HIST..self.end, 0);
            self.end = HIST;
        }
        self.buf[self.end..self.end + BLOCK].copy_from_slice(fb);
        self.end += BLOCK;
    }

    fn track_delay(&mut self, near: &[f32], far: &[f32; BLOCK]) {
        let env = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
        let (ne, fe) = (env(near), env(far));
        let k = self.block;
        self.block += 1;
        self.far_env[k % ENV_LEN] = fe;
        if fe < SILENT {
            return;
        }
        self.mean_near += BETA * (ne - self.mean_near);
        self.mean_far += BETA * (fe - self.mean_far);
        let n = ne - self.mean_near;
        for d in 0..ENV_LEN.min(k + 1) {
            let f = self.far_env[(k - d) % ENV_LEN] - self.mean_far;
            self.corr[d] += BETA * (n * f - self.corr[d]);
        }
        let Some((best, peak)) = self.corr.iter().copied().enumerate().max_by(|a, b| a.1.total_cmp(&b.1)) else { return };
        if peak <= 0.0 {
            return;
        }
        if best.abs_diff(self.candidate) <= 1 {
            self.stable = self.stable.saturating_add(1);
        } else {
            (self.candidate, self.stable) = (best, 0);
        }
        if self.stable >= STABLE_BLOCKS && self.candidate.abs_diff(self.delay) > 1 {
            self.delay = self.candidate;
            self.w.fill(0.0);
            self.talk_hold = 0;
        }
    }

    fn cancel(&mut self, chunk: &mut [f32]) {
        let d0 = (self.delay * BLOCK).saturating_sub(MARGIN);
        let first = self.end - BLOCK;
        let lo = first - d0 + 1 - TAPS;
        let span = &self.buf[lo..first + BLOCK - d0];
        let far_max = span.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if far_max < SILENT {
            self.nlp_gain += 0.3 * (1.0 - self.nlp_gain);
            return;
        }
        let near_max = chunk.iter().fold(0.0f32, |m, v| m.max(v.abs()));
        if near_max > GEIGEL * far_max {
            self.talk_hold = TALK_HOLD_BLOCKS;
        }
        let adapt = self.talk_hold == 0;
        self.talk_hold = self.talk_hold.saturating_sub(1);

        let buf = &self.buf;
        let w = &mut self.w;
        let mut energy: f32 = buf[lo..lo + TAPS].iter().map(|x| x * x).sum();
        let (mut ed, mut ee) = (0.0f32, 0.0f32);
        for (i, s) in chunk.iter_mut().enumerate() {
            let start = lo + i;
            let win = &buf[start..start + TAPS];
            if i > 0 {
                energy = (energy + win[TAPS - 1] * win[TAPS - 1] - buf[start - 1] * buf[start - 1]).max(0.0);
            }
            let y: f32 = w.iter().zip(win).map(|(a, b)| a * b).sum();
            let d = *s;
            let e = d - y;
            if adapt {
                let g = MU * e / (energy + 1e-6 * TAPS as f32);
                for (a, b) in w.iter_mut().zip(win) {
                    *a += g * b;
                }
            }
            ed += d * d;
            ee += e * e;
            *s = e;
        }
        // Filtre parti dans le décor (changement brutal du trajet) : on réapprend.
        if adapt && ed > 1e-8 && ee > 4.0 * ed {
            self.w.fill(0.0);
            chunk.iter_mut().for_each(|s| *s = 0.0);
        }
        let target = if adapt && ee < 0.1 * ed { NLP_FLOOR } else { 1.0 };
        self.nlp_gain += 0.3 * (target - self.nlp_gain);
        let g = self.nlp_gain;
        chunk.iter_mut().for_each(|s| *s *= g);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::dsp::level::rms_db;

    fn noise(seed: u32, amp: f32, n: usize) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amp * ((s >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    /// Écho d'une pièce simple : 80,6 ms de retard, deux réflexions.
    fn echo_of(far: &[f32]) -> Vec<f32> {
        const D: usize = 3870;
        (0..far.len()).map(|i| if i >= D + 7 { 0.6 * far[i - D] + 0.2 * far[i - D - 7] } else { 0.0 }).collect()
    }

    fn run(aec: &mut EchoCanceller, near: &mut [f32], far: &[f32]) {
        for (n, f) in near.chunks_mut(960).zip(far.chunks(960)) {
            aec.process(n, f);
        }
    }

    #[test]
    fn synthetic_echo_is_attenuated() {
        let far = noise(1, 0.3, 48_000 * 4);
        let mut near = echo_of(&far);
        for (s, n) in near.iter_mut().zip(noise(2, 1e-4, far.len())) {
            *s += n;
        }
        let before = rms_db(&near[48_000 * 3..]);
        let mut aec = EchoCanceller::new();
        run(&mut aec, &mut near, &far);
        let after = rms_db(&near[48_000 * 3..]);
        assert!(aec.delay_samples().abs_diff(3870) <= BLOCK, "retard estimé {}", aec.delay_samples());
        assert!(before - after > 20.0, "avant {before:.1} dB, après {after:.1} dB");
    }

    #[test]
    fn local_voice_survives_double_talk() {
        let n = 48_000 * 5;
        let far = noise(3, 0.3, n);
        let voice = noise(4, 0.3, n);
        let echo = echo_of(&far);
        // 3 s d'écho seul (apprentissage), puis la voix locale s'y ajoute.
        let mut near: Vec<f32> = (0..n).map(|i| echo[i] + if i >= 48_000 * 3 { voice[i] } else { 0.0 }).collect();
        let mut aec = EchoCanceller::new();
        run(&mut aec, &mut near, &far);
        let kept = rms_db(&near[48_000 * 4..]);
        let voice_alone = rms_db(&voice[48_000 * 4..]);
        assert!((kept - voice_alone).abs() < 3.0, "voix {voice_alone:.1} dB, sortie {kept:.1} dB");
    }

    #[test]
    fn nothing_played_means_nothing_changed() {
        let mut near = noise(5, 0.2, 9_600);
        let copy = near.clone();
        let mut aec = EchoCanceller::new();
        for frame in near.chunks_mut(960) {
            aec.process(frame, &[]);
        }
        assert_eq!(near, copy);
    }
}
