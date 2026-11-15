//! Réglages « Vidéo » de cet ordinateur (section `[video]` de `local.toml`,
//! page « Voix et vidéo ») et service de la caméra de cette page : liste
//! des caméras et « Tester la vidéo ». L'interface ne touche jamais à la
//! caméra : elle passe par [`CameraService`], dont l'implémentation réelle
//! est `video::capture::service::CameraTest`.

use super::audio_prefs::quoted;
use crate::video::capture::{CaptureError, Source};
use crate::video::{Frame, Quality};
use toml::de::DeTable;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoPrefs {
    /// Identifiant de la caméra (`None` : la première de Windows).
    pub camera: Option<String>,
    /// Le bouton de la caméra passe par l'aperçu du sélecteur avant de l'allumer.
    pub camera_preview: bool,
    /// Qualité proposée d'abord dans le sélecteur de partage.
    pub share_quality: Quality,
    /// « Partager le son » coché d'avance dans le sélecteur de partage.
    pub share_sound: bool,
}

impl Default for VideoPrefs {
    fn default() -> Self {
        VideoPrefs { camera: None, camera_preview: true, share_quality: Quality::default(), share_sound: false }
    }
}

pub const SECTION: &str = "video";
const K_CAMERA: &str = "camera";
const K_PREVIEW: &str = "apercu_avant_camera";
const K_QUALITY: &str = "qualite_partage";
const K_SOUND: &str = "son_partage";

/// Nom d'une qualité dans le fichier, dans l'ordre de [`Quality::PRESETS`].
const QUALITY_NAMES: [&str; 4] = ["720p30", "1080p30", "1080p60", "source"];

fn quality_name(q: Quality) -> &'static str {
    Quality::PRESETS.iter().position(|p| *p == q).map_or(QUALITY_NAMES[0], |i| QUALITY_NAMES[i])
}

impl VideoPrefs {
    /// Section `[video]` de `local.toml` (un champ absent vaut sa valeur par défaut).
    pub fn to_toml(&self) -> String {
        let mut s = format!("[{SECTION}]\n");
        if let Some(c) = &self.camera {
            s += &format!("{K_CAMERA} = {}\n", quoted(c));
        }
        s += &format!("{K_PREVIEW} = {}\n{K_QUALITY} = \"{}\"\n{K_SOUND} = {}\n", self.camera_preview, quality_name(self.share_quality), self.share_sound);
        s
    }

    /// Lit la section `[video]`. Clé inconnue ou valeur illisible : valeur par défaut.
    pub fn from_table(t: &DeTable<'_>) -> VideoPrefs {
        let mut p = VideoPrefs::default();
        for (k, v) in t.iter() {
            let v = v.get_ref();
            match k.get_ref().as_ref() {
                K_CAMERA => p.camera = v.as_str().filter(|s| !s.is_empty()).map(str::to_string),
                K_PREVIEW => p.camera_preview = v.as_bool().unwrap_or(p.camera_preview),
                K_QUALITY => p.share_quality = v.as_str().and_then(|s| QUALITY_NAMES.iter().position(|n| *n == s)).map_or(p.share_quality, |i| Quality::PRESETS[i]),
                K_SOUND => p.share_sound = v.as_bool().unwrap_or(p.share_sound),
                _ => {}
            }
        }
        p
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CameraEvent {
    Cameras(Vec<Source>),
    /// Test impossible à lancer, ou caméra perdue pendant le test : il est arrêté.
    Failed(CaptureError),
}

/// Caméra de la page « Voix et vidéo ». Rien ne tourne hors du test : la
/// caméra n'est ouverte qu'entre [`CameraService::start`] et
/// [`CameraService::stop`], sur un geste de l'utilisateur.
pub trait CameraService {
    /// Demande la liste des caméras (réponse : [`CameraEvent::Cameras`]) ; n'en allume aucune.
    fn list(&mut self);
    /// Lance le test (`None` : la première caméra de Windows).
    fn start(&mut self, camera: Option<&str>);
    fn stop(&mut self);
    fn running(&self) -> bool;
    /// Dernière image du test, si elle est nouvelle depuis le dernier appel.
    fn frame(&mut self) -> Option<Frame>;
    fn poll(&mut self) -> Vec<CameraEvent>;
}

/// Sans caméra : tests, aperçu, avant `App::attach_camera`.
#[derive(Default)]
pub struct NoCamera;

impl CameraService for NoCamera {
    fn list(&mut self) {}
    fn start(&mut self, _: Option<&str>) {}
    fn stop(&mut self) {}
    fn running(&self) -> bool {
        false
    }
    fn frame(&mut self) -> Option<Frame> {
        None
    }
    fn poll(&mut self) -> Vec<CameraEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Par `local.toml` tout entier, comme au démarrage.
    fn parse(text: &str) -> VideoPrefs {
        *crate::state::local::LocalPrefs::parse(text).unwrap().video
    }

    #[test]
    fn section_round_trip() {
        for q in Quality::PRESETS {
            let p = VideoPrefs { camera: Some(r#"camera:\\?\usb#vid_046d&pid{a"b}"#.into()), camera_preview: false, share_quality: q, share_sound: true };
            assert_eq!(parse(&p.to_toml()), p);
        }
        assert_eq!(parse(&VideoPrefs::default().to_toml()), VideoPrefs::default());
        // Avec le reste du fichier : la section vit à côté de `[voix]`.
        let mut all = crate::state::local::LocalPrefs::default();
        all.video.share_quality = Quality::FHD_60;
        all.audio.input_volume = 60;
        assert_eq!(crate::state::local::LocalPrefs::parse(&all.to_toml()), Ok(all));
    }

    #[test]
    fn damaged_values_fall_back() {
        let got = parse("[video]\ncamera = \"\"\napercu_avant_camera = 3\nqualite_partage = \"8k\"\nson_partage = \"oui\"\ninconnu = 1\n");
        assert_eq!(got, VideoPrefs::default());
        assert_eq!(parse("").share_quality, Quality::HD_30);
    }
}
