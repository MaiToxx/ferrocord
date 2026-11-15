//! Périphériques audio de Windows (WASAPI, mode partagé, par `cpal`) :
//! liste des entrées et sorties, ouverture d'une capture mono et d'une
//! lecture stéréo à 48 kHz (rééchantillonnées si le périphérique ne l'est
//! pas), sur des fils audio à part.
//!
//! Sans périphérique choisi, on suit celui par défaut de Windows : quand il
//! change, le flux est rouvert sur le nouveau. Un périphérique débranché ne
//! fait rien planter : on se replie sur le défaut, et l'erreur est remontée
//! par [`Streams::take_error`].

#[cfg(any(windows, target_os = "android"))]
mod backend;
pub mod resample;
pub mod ring;
pub mod service;

use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Identifiant stable (survit aux redémarrages) : c'est lui qu'on enregistre.
    pub id: String,
    pub name: String,
    pub is_default: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AudioError {
    /// Aucun périphérique de ce sens n'est disponible.
    NoDevice { input: bool },
    /// Le périphérique choisi a disparu (débranché) : remplacé par celui par défaut.
    DeviceLost { input: bool, name: String },
    /// Format du périphérique que l'on ne sait pas lire.
    Unsupported(String),
    /// Erreur du système audio (message de Windows / cpal).
    Backend(String),
}

impl fmt::Display for AudioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dir = |input: bool| if input { "entrée" } else { "sortie" };
        match self {
            AudioError::NoDevice { input } => write!(f, "aucun périphérique de {}", dir(*input)),
            AudioError::DeviceLost { input, name } => write!(f, "périphérique de {} perdu : {name}", dir(*input)),
            AudioError::Unsupported(m) => write!(f, "format non pris en charge : {m}"),
            AudioError::Backend(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for AudioError {}

/// Ce que donne la capture : du mono à 48 kHz, en morceaux de taille quelconque.
pub type CaptureFn = Box<dyn FnMut(&[f32]) + Send>;
/// Ce que demande la lecture : du stéréo entrelacé à 48 kHz, à remplir en entier.
pub type PlaybackFn = Box<dyn FnMut(&mut [f32]) + Send>;

/// Périphérique à ouvrir, d'après le choix enregistré et la liste actuelle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Celui de Windows, suivi s'il change.
    Default,
    /// Le périphérique choisi, présent.
    Chosen(String),
    /// Le périphérique choisi est absent : celui par défaut en attendant.
    Missing(String),
}

/// Choix du périphérique : `wanted` (identifiant enregistré, `None` :
/// celui de Windows) parmi `list`.
pub fn pick(wanted: Option<&str>, list: &[DeviceInfo]) -> Pick {
    match wanted {
        None => Pick::Default,
        Some(id) if list.iter().any(|d| d.id == id) => Pick::Chosen(id.to_string()),
        Some(id) => Pick::Missing(id.to_string()),
    }
}

/// Nom à afficher pour un identifiant enregistré (le nom brut s'il a disparu).
pub fn display_name<'a>(id: &'a str, list: &'a [DeviceInfo]) -> &'a str {
    list.iter().find(|d| d.id == id).map_or(id, |d| d.name.as_str())
}

/// Erreur E_ACCESSDENIED (0x80070005) de WASAPI : le plus souvent, les réglages
/// de confidentialité de Windows refusent le micro aux applications de bureau.
pub fn is_access_denied(msg: &str) -> bool {
    msg.contains("-2147024891") || msg.to_ascii_lowercase().contains("0x80070005")
}

/// Entrées (micros) disponibles.
pub fn inputs() -> Vec<DeviceInfo> {
    #[cfg(any(windows, target_os = "android"))]
    return backend::list(true);
    #[cfg(not(any(windows, target_os = "android")))]
    Vec::new()
}

/// Sorties (casques, haut-parleurs) disponibles.
pub fn outputs() -> Vec<DeviceInfo> {
    #[cfg(any(windows, target_os = "android"))]
    return backend::list(false);
    #[cfg(not(any(windows, target_os = "android")))]
    Vec::new()
}

/// Capture et lecture ouvertes ; tout s'arrête quand on la lâche.
pub struct Streams {
    #[cfg(any(windows, target_os = "android"))]
    inner: backend::Running,
}

impl Streams {
    /// Dernière erreur survenue depuis l'ouverture (périphérique perdu…), une fois.
    pub fn take_error(&self) -> Option<AudioError> {
        #[cfg(any(windows, target_os = "android"))]
        return self.inner.take_error();
        #[cfg(not(any(windows, target_os = "android")))]
        None
    }
}

/// Ouvre capture et lecture (`None` : périphérique par défaut de Windows).
/// Les rappels sont appelés sur les fils audio : ils ne doivent ni bloquer
/// ni allouer.
pub fn open(input: Option<&str>, output: Option<&str>, on_capture: CaptureFn, on_playback: PlaybackFn) -> Result<Streams, AudioError> {
    open_sides(input, output, (true, true), on_capture, on_playback)
}

/// Lecture seule (son d'un direct hors appel, messages vocaux) : le micro
/// n'est pas ouvert.
pub fn open_output(output: Option<&str>, on_playback: PlaybackFn) -> Result<Streams, AudioError> {
    open_sides(None, output, (false, true), Box::new(|_| {}), on_playback)
}

/// Capture seule (enregistrement d'un message vocal).
pub fn open_input(input: Option<&str>, on_capture: CaptureFn) -> Result<Streams, AudioError> {
    open_sides(input, None, (true, false), on_capture, Box::new(|o: &mut [f32]| o.fill(0.0)))
}

fn open_sides(input: Option<&str>, output: Option<&str>, sides: (bool, bool), on_capture: CaptureFn, on_playback: PlaybackFn) -> Result<Streams, AudioError> {
    // Android : le micro exige la permission RECORD_AUDIO, demandée au premier usage.
    #[cfg(target_os = "android")]
    if sides.0 {
        crate::platform::android::ask_microphone();
    }
    #[cfg(any(windows, target_os = "android"))]
    return backend::Running::start(input.map(str::to_string), output.map(str::to_string), sides, on_capture, on_playback).map(|inner| Streams { inner });
    #[cfg(not(any(windows, target_os = "android")))]
    {
        let _ = (input, output, sides, on_capture, on_playback);
        Err(AudioError::NoDevice { input: sides.0 })
    }
}

/// Moyenne des canaux d'une trame entrelacée (capture vers mono).
pub fn downmix(frame: &[f32]) -> f32 {
    if frame.is_empty() { 0.0 } else { frame.iter().sum::<f32>() / frame.len() as f32 }
}

/// Une trame stéréo vers les `out.len()` canaux du périphérique : mono par
/// moyenne, gauche et droite sur les deux premiers, silence sur les autres.
pub fn spread(lr: [f32; 2], out: &mut [f32]) {
    match out.len() {
        0 => {}
        1 => out[0] = (lr[0] + lr[1]) * 0.5,
        _ => {
            out[0] = lr[0];
            out[1] = lr[1];
            out[2..].iter_mut().for_each(|s| *s = 0.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, default: bool) -> DeviceInfo {
        DeviceInfo { id: id.into(), name: format!("Micro {id}"), is_default: default }
    }

    #[test]
    fn default_choice_and_fallback() {
        let list = [dev("a", true), dev("b", false)];
        assert_eq!(pick(None, &list), Pick::Default);
        assert_eq!(pick(Some("b"), &list), Pick::Chosen("b".into()));
        assert_eq!(pick(Some("c"), &list), Pick::Missing("c".into()), "débranché : repli sur le défaut");
        assert_eq!(pick(Some("a"), &[]), Pick::Missing("a".into()));
        assert_eq!(display_name("b", &list), "Micro b");
        assert_eq!(display_name("zz", &list), "zz");
    }

    #[test]
    fn channel_mapping() {
        assert_eq!(downmix(&[0.5, -0.5, 1.0, 0.0]), 0.25);
        assert_eq!(downmix(&[]), 0.0);
        let mut mono = [9.0];
        spread([0.2, 0.4], &mut mono);
        assert!((mono[0] - 0.3).abs() < 1e-6);
        let mut six = [9.0; 6];
        spread([0.2, 0.4], &mut six);
        assert_eq!(six, [0.2, 0.4, 0.0, 0.0, 0.0, 0.0]);
    }
}
