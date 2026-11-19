//! Réglages « Voix et vidéo » de cet ordinateur (section `[voix]` de
//! `local.toml`) et service audio de la page des réglages : liste des
//! périphériques et « Test du micro ». L'interface ne touche jamais aux
//! périphériques : elle passe par [`AudioService`], dont l'implémentation
//! réelle est `audio::io::service::DeviceService`.

use crate::audio::dsp::DspSettings;
use crate::audio::io::{AudioError, DeviceInfo};
use toml::de::{DeTable, DeValue};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputMode {
    /// Détection de la voix : on parle dès que le niveau passe le seuil.
    #[default]
    Voice,
    /// Appui pour parler : seulement touche enfoncée.
    PushToTalk,
}

pub const VOLUME_MAX: u8 = 200;
pub const THRESHOLD_MIN_DB: i8 = -100;
pub const RELEASE_MAX_MS: u16 = 2000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AudioPrefs {
    /// Identifiant du périphérique (`None` : celui de Windows, suivi s'il change).
    pub input_device: Option<String>,
    pub output_device: Option<String>,
    /// Volumes en pour cent, de 0 à [`VOLUME_MAX`].
    pub input_volume: u8,
    pub output_volume: u8,
    pub mode: InputMode,
    /// Seuil de la détection de la voix déduit du bruit de fond.
    pub auto_threshold: bool,
    /// Seuil réglé à la main, de [`THRESHOLD_MIN_DB`] à 0 dB.
    pub threshold_db: i8,
    /// Délai avant de couper le micro après le relâchement (la touche est
    /// dans `keys.toml`, action `vocal_appui_pour_parler`).
    pub ptt_release_ms: u16,
    pub noise_suppression: bool,
    pub echo_cancellation: bool,
    pub auto_gain: bool,
    /// « Couper le micro » et « Couper le casque » marchent hors de la fenêtre
    /// (raccourcis globaux de Windows, combinaisons de `keys.toml`).
    pub global_keys: bool,
    /// Annonce aux appels que cet ordinateur décode aussi VP9 et AV1 (quand
    /// Windows a les extensions). Coupé par défaut : pas encore éprouvé sur
    /// de vrais appels, et H.264 marche partout.
    pub extra_codecs: bool,
}

impl Default for AudioPrefs {
    fn default() -> Self {
        AudioPrefs {
            input_device: None,
            output_device: None,
            input_volume: 100,
            output_volume: 100,
            mode: InputMode::Voice,
            auto_threshold: true,
            threshold_db: -50,
            ptt_release_ms: 20,
            noise_suppression: true,
            echo_cancellation: true,
            auto_gain: true,
            global_keys: false,
            extra_codecs: false,
        }
    }
}

impl AudioPrefs {
    /// Étages du traitement de la voix ; le passe-haut est toujours là.
    pub fn dsp(&self) -> DspSettings {
        DspSettings { high_pass: true, noise_suppression: self.noise_suppression, echo_cancellation: self.echo_cancellation, auto_gain: self.auto_gain, rnnoise: 0 }
    }

    pub fn input_gain(&self) -> f32 {
        f32::from(self.input_volume.min(VOLUME_MAX)) / 100.0
    }

    pub fn output_gain(&self) -> f32 {
        f32::from(self.output_volume.min(VOLUME_MAX)) / 100.0
    }
}

pub const SECTION: &str = "voix";
const K_INPUT: &str = "entree";
const K_OUTPUT: &str = "sortie";
const K_IN_VOL: &str = "volume_entree";
const K_OUT_VOL: &str = "volume_sortie";
const K_MODE: &str = "mode";
const K_AUTO: &str = "seuil_automatique";
const K_THRESHOLD: &str = "seuil_db";
const K_RELEASE: &str = "delai_relache_ms";
const K_NOISE: &str = "suppression_bruit";
const K_ECHO: &str = "annulation_echo";
const K_GAIN: &str = "gain_automatique";
const K_GLOBAL: &str = "raccourcis_globaux";
const K_EXTRA_CODECS: &str = "codecs_vp9_av1";
const MODE_VOICE: &str = "voix";
const MODE_PTT: &str = "appui";

/// Chaîne TOML entre guillemets (les identifiants Windows contiennent `{}` et `\`).
pub(super) fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => out.push_str(&format!("\\u{:04X}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

impl AudioPrefs {
    /// Section `[voix]` de `local.toml` (un champ absent vaut sa valeur par défaut).
    pub fn to_toml(&self) -> String {
        let mut s = format!("[{SECTION}]\n");
        if let Some(d) = &self.input_device {
            s += &format!("{K_INPUT} = {}\n", quoted(d));
        }
        if let Some(d) = &self.output_device {
            s += &format!("{K_OUTPUT} = {}\n", quoted(d));
        }
        let mode = if self.mode == InputMode::PushToTalk { MODE_PTT } else { MODE_VOICE };
        s += &format!(
            "{K_IN_VOL} = {}\n{K_OUT_VOL} = {}\n{K_MODE} = \"{mode}\"\n{K_AUTO} = {}\n{K_THRESHOLD} = {}\n",
            self.input_volume, self.output_volume, self.auto_threshold, self.threshold_db
        );
        s += &format!("{K_RELEASE} = {}\n{K_NOISE} = {}\n{K_ECHO} = {}\n{K_GAIN} = {}\n", self.ptt_release_ms, self.noise_suppression, self.echo_cancellation, self.auto_gain);
        s += &format!("{K_GLOBAL} = {}\n{K_EXTRA_CODECS} = {}\n", self.global_keys, self.extra_codecs);
        s
    }

    /// Lit la section `[voix]`. Clé inconnue ou valeur illisible : valeur par défaut.
    pub fn from_table(t: &DeTable<'_>) -> AudioPrefs {
        let mut p = AudioPrefs::default();
        let int = |v: &DeValue| v.as_integer().and_then(|i| i64::from_str_radix(i.as_str(), i.radix()).ok());
        let text = |v: &DeValue| v.as_str().filter(|s| !s.is_empty()).map(str::to_string);
        for (k, v) in t.iter() {
            let v = v.get_ref();
            match k.get_ref().as_ref() {
                K_INPUT => p.input_device = text(v),
                K_OUTPUT => p.output_device = text(v),
                K_IN_VOL => p.input_volume = int(v).map_or(p.input_volume, |n| n.clamp(0, i64::from(VOLUME_MAX)) as u8),
                K_OUT_VOL => p.output_volume = int(v).map_or(p.output_volume, |n| n.clamp(0, i64::from(VOLUME_MAX)) as u8),
                K_MODE => {
                    p.mode = match v.as_str() {
                        Some(MODE_PTT) => InputMode::PushToTalk,
                        _ => InputMode::Voice,
                    }
                }
                K_AUTO => p.auto_threshold = v.as_bool().unwrap_or(p.auto_threshold),
                K_THRESHOLD => p.threshold_db = int(v).map_or(p.threshold_db, |n| n.clamp(i64::from(THRESHOLD_MIN_DB), 0) as i8),
                K_RELEASE => p.ptt_release_ms = int(v).map_or(p.ptt_release_ms, |n| n.clamp(0, i64::from(RELEASE_MAX_MS)) as u16),
                K_NOISE => p.noise_suppression = v.as_bool().unwrap_or(p.noise_suppression),
                K_ECHO => p.echo_cancellation = v.as_bool().unwrap_or(p.echo_cancellation),
                K_GAIN => p.auto_gain = v.as_bool().unwrap_or(p.auto_gain),
                K_GLOBAL => p.global_keys = v.as_bool().unwrap_or(p.global_keys),
                K_EXTRA_CODECS => p.extra_codecs = v.as_bool().unwrap_or(p.extra_codecs),
                _ => {}
            }
        }
        p
    }
}

/// Niveau du micro pendant le test.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Meter {
    /// Niveau de la dernière trame (après le volume d'entrée), en dB pleine échelle.
    pub level_db: f32,
    /// Seuil en vigueur (réglé, ou déduit du bruit de fond en automatique).
    pub threshold_db: f32,
    /// La détection de la voix laisse passer.
    pub open: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum AudioEvent {
    Devices { inputs: Vec<DeviceInfo>, outputs: Vec<DeviceInfo> },
    /// Test impossible à lancer, ou incident pendant le test (périphérique perdu…).
    TestError(AudioError),
}

/// Service audio de la page « Voix et vidéo ». Rien ne tourne hors du test :
/// aucun fil audio ouvert, aucun réveil.
pub trait AudioService {
    /// Demande la liste des périphériques (réponse : [`AudioEvent::Devices`]).
    fn list_devices(&mut self);
    /// Lance le test du micro : on s'entend, traitement compris.
    fn start_test(&mut self, prefs: &AudioPrefs);
    /// Volumes, seuil et traitement changés pendant le test (sans le relancer).
    fn update_test(&mut self, prefs: &AudioPrefs);
    fn stop_test(&mut self);
    fn testing(&self) -> bool;
    /// Niveau courant pendant le test.
    fn meter(&self) -> Option<Meter>;
    fn poll(&mut self) -> Vec<AudioEvent>;
}

/// Sans périphérique : tests, aperçu, avant `App::attach_audio`.
#[derive(Default)]
pub struct NoAudio;

impl AudioService for NoAudio {
    fn list_devices(&mut self) {}
    fn start_test(&mut self, _: &AudioPrefs) {}
    fn update_test(&mut self, _: &AudioPrefs) {}
    fn stop_test(&mut self) {}
    fn testing(&self) -> bool {
        false
    }
    fn meter(&self) -> Option<Meter> {
        None
    }
    fn poll(&mut self) -> Vec<AudioEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Par `local.toml` tout entier, comme au démarrage.
    fn parse(text: &str) -> AudioPrefs {
        crate::state::local::LocalPrefs::parse(text).unwrap().audio
    }

    #[test]
    fn section_round_trip() {
        let p = AudioPrefs {
            input_device: Some(r#"wasapi:{0.0.1.00000000}.{a"b\c}"#.into()),
            output_device: None,
            input_volume: 150,
            output_volume: 0,
            mode: InputMode::PushToTalk,
            auto_threshold: false,
            threshold_db: -37,
            ptt_release_ms: 400,
            noise_suppression: false,
            echo_cancellation: true,
            auto_gain: false,
            global_keys: true,
            extra_codecs: true,
        };
        assert_eq!(parse(&p.to_toml()), p);
        assert_eq!(parse(&AudioPrefs::default().to_toml()), AudioPrefs::default());
    }

    #[test]
    fn vocal_and_voix_sections_live_side_by_side() {
        let mut p = crate::state::local::LocalPrefs::default();
        p.voice.mute = true;
        p.voice.set_user(7, crate::state::voice::UserVoice { volume: 150, muted: true });
        p.audio.input_volume = 60;
        p.audio.mode = InputMode::PushToTalk;
        let text = p.to_toml();
        assert!(text.contains("[vocal]") && text.contains("[voix]"));
        assert_eq!(crate::state::local::LocalPrefs::parse(&text), Ok(p));
    }

    #[test]
    fn damaged_values_fall_back() {
        let got = parse("[voix]\nvolume_entree = 900\nseuil_db = -500\nmode = \"telepathie\"\ngain_automatique = 3\nentree = \"\"\ninconnu = 1\n");
        assert_eq!((got.input_volume, got.threshold_db, got.mode, got.auto_gain, got.input_device), (VOLUME_MAX, THRESHOLD_MIN_DB, InputMode::Voice, true, None));
    }

    #[test]
    fn gains_and_dsp() {
        let p = AudioPrefs { input_volume: 50, output_volume: 200, noise_suppression: false, ..Default::default() };
        assert_eq!((p.input_gain(), p.output_gain()), (0.5, 2.0));
        let d = p.dsp();
        assert!(d.high_pass && !d.noise_suppression && d.echo_cancellation && d.auto_gain);
    }
}
