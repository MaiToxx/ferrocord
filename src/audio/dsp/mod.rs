//! Traitement de la voix captée, dans l'ordre habituel des chaînes vocales :
//! passe-haut → annulation d'écho → suppression de bruit → gain automatique.
//! Chaque étage se coupe ; rien n'alloue après [`VoiceChain::new`].

pub mod agc;
pub mod echo;
pub mod eq;
pub mod highpass;
pub mod level;
pub mod rnn;

use super::Processor;

/// Étages actifs de la chaîne.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DspSettings {
    pub high_pass: bool,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    /// Plugin « Suppression de bruit RNNoise » : 0 coupé, sinon son intensité
    /// (1 à 100). Activé, RNNoise tient la place de l'étage spectral.
    pub rnnoise: u8,
}

impl Default for DspSettings {
    fn default() -> Self {
        DspSettings { high_pass: true, noise_suppression: true, echo_cancellation: true, auto_gain: true, rnnoise: 0 }
    }
}

impl DspSettings {
    /// Rangés dans un mot (réglage partagé avec le fil audio par un atomique).
    pub fn bits(self) -> u16 {
        u16::from(self.high_pass) | u16::from(self.noise_suppression) << 1 | u16::from(self.echo_cancellation) << 2 | u16::from(self.auto_gain) << 3 | u16::from(self.rnnoise.min(100)) << 8
    }

    pub fn from_bits(b: u16) -> Self {
        DspSettings { high_pass: b & 1 != 0, noise_suppression: b & 2 != 0, echo_cancellation: b & 4 != 0, auto_gain: b & 8 != 0, rnnoise: ((b >> 8) as u8).min(100) }
    }

    /// Avec les réglages des plugins vocaux.
    pub fn with(self, extras: Extras) -> Self {
        DspSettings { rnnoise: extras.rnnoise, ..self }
    }
}

/// Ce que les plugins vocaux (lot E4) ajoutent aux réglages audio. Lu sur le
/// fil de l'interface ([`Extras::current`]) et porté aux fils audio avec le
/// reste des réglages : eux ne consultent jamais le registre.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Extras {
    /// Intensité de RNNoise (0 : plugin coupé).
    pub rnnoise: u8,
    /// Égaliseur des voix entendues : grave, médium, aigu, en dB (tout à 0 :
    /// plugin coupé ou préréglage « Plat »).
    pub eq: [i8; 3],
}

impl Extras {
    /// État des plugins vocaux pour le fil qui appelle.
    pub fn current() -> Self {
        use crate::plugins::{self, Id};
        let rnnoise = if plugins::on(Id::NoiseRnn) { plugins::number(Id::NoiseRnn, plugins::RNN_STRENGTH).clamp(1, 100) as u8 } else { 0 };
        let eq = if plugins::on(Id::VoiceEq) {
            let preset = plugins::choice(Id::VoiceEq, plugins::EQ_PRESET);
            let custom = if preset == eq::PRESET_CUSTOM { [plugins::EQ_LOW, plugins::EQ_MID, plugins::EQ_HIGH].map(|k| plugins::number(Id::VoiceEq, k) as i8) } else { [0; 3] };
            eq::preset_gains(preset, custom)
        } else {
            [0; 3]
        };
        Extras { rnnoise, eq }
    }
}

pub struct VoiceChain {
    settings: DspSettings,
    high_pass: highpass::HighPass,
    echo: echo::EchoCanceller,
    /// RNNoise : n'existe que plugin activé.
    rnn: Option<rnn::Rnn>,
    agc: agc::Agc,
}

impl VoiceChain {
    pub fn new(settings: DspSettings) -> Self {
        VoiceChain { settings, high_pass: highpass::HighPass::voice(), echo: echo::EchoCanceller::new(), rnn: (settings.rnnoise > 0).then(rnn::Rnn::new), agc: agc::Agc::new() }
    }

    pub fn settings(&self) -> DspSettings {
        self.settings
    }

    /// RNNoise est en place (plugin activé).
    pub fn has_rnn(&self) -> bool {
        self.rnn.is_some()
    }

    /// Change les étages ; un étage rallumé repart de zéro (sans allouer,
    /// sauf RNNoise, créé à l'activation du plugin et rendu à sa coupure).
    pub fn set_settings(&mut self, s: DspSettings) {
        let was = self.settings;
        if (s.rnnoise > 0) != self.rnn.is_some() {
            self.rnn = (s.rnnoise > 0).then(rnn::Rnn::new);
        }
        if s.high_pass && !was.high_pass {
            self.high_pass.reset();
        }
        if s.echo_cancellation && !was.echo_cancellation {
            self.echo.reset();
        }
        if s.auto_gain && !was.auto_gain {
            self.agc.reset();
        }
        self.settings = s;
    }
}

impl Processor for VoiceChain {
    fn process(&mut self, mono: &mut [f32], far_end: &[f32]) {
        let s = self.settings;
        if s.high_pass {
            self.high_pass.process(mono);
        }
        if s.echo_cancellation {
            self.echo.process(mono, far_end);
        }
        // Suppression de bruit : RNNoise seulement (plugin). L'étage spectral
        // d'origine a été retiré à la demande de l'utilisateur (0.1.13) : il
        // abîmait la voix.
        if s.noise_suppression
            && let Some(rnn) = &mut self.rnn
        {
            rnn.process(mono, s.rnnoise);
        }
        if s.auto_gain {
            self.agc.process(mono);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_bits_round_trip() {
        for b in (0..16u16).chain([0x6400 | 2, 0x0100 | 15]) {
            assert_eq!(DspSettings::from_bits(b).bits(), b);
        }
        assert_eq!(DspSettings::from_bits(DspSettings::default().bits()), DspSettings::default());
    }

    #[test]
    fn everything_off_is_transparent() {
        let off = DspSettings { high_pass: false, noise_suppression: false, echo_cancellation: false, auto_gain: false, rnnoise: 0 };
        let mut chain = VoiceChain::new(off);
        let mut x: Vec<f32> = (0..960).map(|i| (i as f32 * 0.01).sin() * 0.3).collect();
        let copy = x.clone();
        chain.process(&mut x, &copy);
        assert_eq!(x, copy);
    }

    #[test]
    fn full_chain_keeps_samples_in_range() {
        let mut chain = VoiceChain::new(DspSettings::default());
        let far = vec![0.0f32; 960];
        for f in 0..50 {
            let mut x: Vec<f32> = (0..960).map(|i| ((f * 960 + i) as f32 * 0.05).sin() * 0.99).collect();
            chain.process(&mut x, &far);
            assert!(x.iter().all(|s| s.is_finite() && s.abs() <= 1.0));
        }
    }

    /// Plugin coupé : RNNoise n'existe pas et rien ne change ; activé, il
    /// remplace l'étage spectral, et la coupure le rend.
    #[test]
    fn rnnoise_exists_only_while_the_plugin_is_on() {
        let only_noise = |rnnoise| DspSettings { high_pass: false, noise_suppression: true, echo_cancellation: false, auto_gain: false, rnnoise };
        let x = rnn::tests::noise(11, 0.05, 960 * 100);
        let run = |chain: &mut VoiceChain| {
            let mut y = x.clone();
            let far = vec![0.0f32; 960];
            for f in y.chunks_mut(960) {
                chain.process(f, &far);
            }
            y
        };
        let mut off = VoiceChain::new(only_noise(0));
        assert!(!off.has_rnn());
        let spectral = run(&mut off);
        let mut on = VoiceChain::new(only_noise(100));
        assert!(on.has_rnn());
        let neural = run(&mut on);
        assert_ne!(spectral, neural);
        assert!(level::rms_db(&neural[48_000..]) < level::rms_db(&x[48_000..]) - 6.0);
        on.set_settings(only_noise(0));
        assert!(!on.has_rnn(), "plugin coupé : l'état de RNNoise est rendu");
        off.set_settings(only_noise(60));
        assert!(off.has_rnn());
        // Sans plugin publié sur ce fil, rien n'est demandé.
        assert_eq!(Extras::current(), Extras::default());
    }
}
