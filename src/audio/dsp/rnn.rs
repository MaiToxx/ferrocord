//! Suppression de bruit par réseau de neurones (RNNoise, crate `nnnoiseless`,
//! Rust pur) : le plugin « Suppression de bruit RNNoise » (lot E4). Elle
//! remplace l'étage spectral de la chaîne quand le plugin est activé.
//!
//! RNNoise travaille par trames de 480 échantillons (10 ms à 48 kHz), sur
//! des valeurs à l'échelle d'un `i16` : une trame de la chaîne (960) en fait
//! deux, traitées sur place. RNNoise rend sa sortie une trame plus tard
//! (10 ms, mesuré) : c'est le retard qu'ajoute le plugin. L'intensité mélange
//! le signal d'origine et le signal débruité (100 : débruité seul) ;
//! l'origine est retardée d'autant, sinon la voix s'entendrait deux fois à
//! 10 ms d'écart (filtre en peigne).
//!
//! Rien n'existe tant que le plugin est coupé : l'état (quelques dizaines de
//! Kio) est créé par [`Rnn::new`] à l'activation et rendu à la coupure.

use nnnoiseless::DenoiseState;

/// Trame de RNNoise.
pub const RNN_FRAME: usize = DenoiseState::FRAME_SIZE;
const SCALE: f32 = 32768.0;

pub struct Rnn {
    state: Box<DenoiseState<'static>>,
    input: [f32; RNN_FRAME],
    output: [f32; RNN_FRAME],
    /// Trame d'entrée précédente : l'origine, en face de la sortie de RNNoise.
    dry: [f32; RNN_FRAME],
    /// La première trame de RNNoise est un fondu d'entrée : on garde l'origine.
    primed: bool,
}

impl Default for Rnn {
    fn default() -> Self {
        Self::new()
    }
}

impl Rnn {
    pub fn new() -> Self {
        Rnn { state: DenoiseState::new(), input: [0.0; RNN_FRAME], output: [0.0; RNN_FRAME], dry: [0.0; RNN_FRAME], primed: false }
    }

    /// Débruite `mono` sur place, par trames entières de 10 ms (un reste plus
    /// court, qui n'arrive pas dans la chaîne, passe tel quel). `strength` :
    /// 1 à 100.
    pub fn process(&mut self, mono: &mut [f32], strength: u8) {
        let k = f32::from(strength.min(100)) / 100.0;
        for frame in mono.as_chunks_mut::<RNN_FRAME>().0 {
            for (i, x) in self.input.iter_mut().zip(frame.iter()) {
                *i = x * SCALE;
            }
            self.state.process_frame(&mut self.output, &self.input);
            let k = if self.primed { k } else { 0.0 };
            self.primed = true;
            for ((x, o), dry) in frame.iter_mut().zip(&self.output).zip(self.dry.iter_mut()) {
                let clean = (o / SCALE).clamp(-1.0, 1.0);
                let before = std::mem::replace(dry, *x);
                *x = before + (clean - before) * k;
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::audio::dsp::level::rms_db;
    use std::f32::consts::TAU;

    /// Bruit blanc reproductible (générateur congruentiel).
    pub(crate) fn noise(seed: u32, amp: f32, n: usize) -> Vec<f32> {
        let mut s = seed;
        (0..n)
            .map(|_| {
                s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                amp * ((s >> 8) as f32 / (1u32 << 24) as f32 * 2.0 - 1.0)
            })
            .collect()
    }

    /// Bruit de ventilateur : bruit blanc passé par un passe-bas à 800 Hz,
    /// plus un ronflement à 100 Hz.
    pub(crate) fn fan(seed: u32, n: usize) -> Vec<f32> {
        let k = 1.0 - (-TAU * 800.0 / 48_000.0).exp();
        let mut low = 0.0f32;
        noise(seed, 0.25, n)
            .into_iter()
            .enumerate()
            .map(|(i, w)| {
                low += k * (w - low);
                low + 0.01 * (TAU * 100.0 * i as f32 / 48_000.0).sin()
            })
            .collect()
    }

    /// Voix synthétique : fondamentale à 130 Hz avec vibrato, trente
    /// harmoniques pesées par trois formants (voyelle « a »), syllabes à 3 Hz.
    pub(crate) fn voice(n: usize, amp: f32) -> Vec<f32> {
        let formants = [(700.0f32, 130.0f32, 1.0f32), (1220.0, 170.0, 0.5), (2600.0, 250.0, 0.25)];
        let mut phase = 0.0f32;
        (0..n)
            .map(|i| {
                let t = i as f32 / 48_000.0;
                let f0 = 130.0 * (1.0 + 0.03 * (TAU * 5.0 * t).sin()) * (1.0 + 0.15 * (TAU * 0.7 * t).sin());
                phase = (phase + TAU * f0 / 48_000.0) % TAU;
                let mut s = 0.0;
                for h in 1..=30 {
                    let f = f0 * h as f32;
                    let w: f32 = formants.iter().map(|(c, bw, g)| g / (1.0 + ((f - c) / bw).powi(2))).sum();
                    s += (w + 0.02) * (phase * h as f32).sin();
                }
                let syllable = (0.5 - 0.5 * (TAU * 3.0 * t).cos()).powf(0.6);
                amp * 0.5 * s * syllable
            })
            .collect()
    }

    fn run(rnn: &mut Rnn, x: &mut [f32], strength: u8) {
        for f in x.chunks_mut(960) {
            rnn.process(f, strength);
        }
    }

    /// Le bruit synthétique baisse d'un nombre de dB mesuré ; une voix
    /// synthétique dans ce bruit garde son niveau.
    #[test]
    fn noise_drops_and_voice_is_kept() {
        let drop_of = |mut x: Vec<f32>, what: &str| {
            let before = rms_db(&x[96_000..]);
            run(&mut Rnn::new(), &mut x, 100);
            let after = rms_db(&x[96_000..]);
            println!("RNNoise, {what} : {before:.1} dB -> {after:.1} dB ({:.1} dB de moins)", before - after);
            before - after
        };
        let white = drop_of(noise(7, 0.05, 48_000 * 4), "bruit blanc");
        let hum = drop_of(fan(7, 48_000 * 4), "ventilateur (bruit grave et ronflement)");
        assert!(white > 8.0, "bruit blanc : {white:.1} dB de moins");
        assert!(hum > 12.0, "ventilateur : {hum:.1} dB de moins");

        let clean = voice(48_000 * 4, 0.5);
        let n = fan(9, clean.len());
        let mut mix: Vec<f32> = clean.iter().zip(&n).map(|(a, b)| a + b).collect();
        let mut rnn = Rnn::new();
        run(&mut rnn, &mut mix, 100);
        let (kept, orig) = (rms_db(&mix[48_000..]), rms_db(&clean[48_000..]));
        // Ce qui reste d'étranger à la voix d'origine (bruit et déformation),
        // la sortie remise en face de l'entrée (RNNoise la retarde un peu).
        let residue_at = |delay: usize, from: usize, to: usize| {
            let err: Vec<f32> = (from..to).map(|i| mix[i + delay] - clean[i]).collect();
            rms_db(&err)
        };
        let delay = (0..=RNN_FRAME * 2).min_by(|a, b| residue_at(*a, 96_000, 108_000).total_cmp(&residue_at(*b, 96_000, 108_000))).unwrap_or(0);
        let residue = residue_at(delay, 48_000, clean.len() - RNN_FRAME * 2);
        println!("RNNoise, voix dans le bruit : voix {orig:.1} dB -> {kept:.1} dB ; bruit ajouté {:.1} dB -> résidu {residue:.1} dB (retard {delay})", rms_db(&n));
        assert!((kept - orig).abs() < 1.5, "voix : {orig:.1} dB → {kept:.1} dB");
        // La voix d'essai n'est pas une vraie voix : on exige seulement que l'écart à
        // l'original ne dépasse pas le bruit qu'on y avait mis (pas de déformation ajoutée).
        assert!(residue < rms_db(&n) + 1.0, "résidu {residue:.1} dB");
    }

    /// Intensité : à mi-course, le bruit baisse moins qu'à fond.
    #[test]
    fn strength_blends_dry_and_clean() {
        let level = |strength: u8| {
            let mut x = noise(7, 0.05, 48_000 * 2);
            run(&mut Rnn::new(), &mut x, strength);
            rms_db(&x[48_000..])
        };
        let (half, full) = (level(50), level(100));
        println!("RNNoise, intensité : 10 % {:.1} dB, 50 % {half:.1} dB, 100 % {full:.1} dB", level(10));
        assert!(half > full + 1.5, "50 % : {half:.1} dB, 100 % : {full:.1} dB");
        assert!(level(10) > half + 1.5);
    }

    /// Coût : une petite part du temps réel (budget : bien moins de 5 % d'un
    /// cœur). `nnnoiseless` et sa transformée de Fourier sont optimisées même
    /// en débogage (`Cargo.toml`, `profile.dev.package`) : la mesure vaut
    /// pour les deux profils.
    #[test]
    fn cpu_cost_is_a_small_share_of_real_time() {
        let seconds = 10;
        let mut x = noise(3, 0.05, 48_000 * seconds);
        let mut rnn = Rnn::new();
        let start = std::time::Instant::now();
        run(&mut rnn, &mut x, 100);
        let share = start.elapsed().as_secs_f64() / seconds as f64 * 100.0;
        println!("RNNoise : {share:.2} % d'un cœur ({})", if cfg!(debug_assertions) { "débogage" } else { "optimisé" });
        assert!(share < 5.0, "{share:.2} % d'un cœur");
    }

    #[test]
    fn output_stays_in_range_and_short_tails_pass() {
        let mut x = noise(5, 0.99, 960 * 5 + 100);
        let copy = x.clone();
        Rnn::new().process(&mut x, 100);
        assert!(x.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        assert_eq!(x[960 * 5..], copy[960 * 5..], "reste plus court qu'une trame : intact");
    }
}
