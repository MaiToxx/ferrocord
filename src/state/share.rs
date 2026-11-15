//! Sélecteur de partage (lot M3) : ce que l'interface demande au service de
//! capture et ce qu'il répond. L'interface ne touche jamais aux écrans, aux
//! fenêtres ni aux caméras : elle passe par [`CaptureService`], dont
//! l'implémentation réelle est `video::capture::service::CaptureDevices`.

use crate::video::capture::{CaptureError, Quality, Source, SourceKind};
use crate::video::Frame;

/// Largeur des vignettes de la liste, et de l'aperçu de la source choisie.
pub const THUMB_WIDTH: u32 = 320;
pub const PREVIEW_WIDTH: u32 = 640;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShareTab {
    #[default]
    Screens,
    Windows,
    Cameras,
}

impl ShareTab {
    pub const ALL: [ShareTab; 3] = [ShareTab::Screens, ShareTab::Windows, ShareTab::Cameras];

    pub fn kind(self) -> SourceKind {
        match self {
            ShareTab::Screens => SourceKind::Screen,
            ShareTab::Windows => SourceKind::Window,
            ShareTab::Cameras => SourceKind::Camera,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceLists {
    pub screens: Vec<Source>,
    pub windows: Vec<Source>,
    pub cameras: Vec<Source>,
}

impl SourceLists {
    pub fn of(&self, tab: ShareTab) -> &[Source] {
        match tab {
            ShareTab::Screens => &self.screens,
            ShareTab::Windows => &self.windows,
            ShareTab::Cameras => &self.cameras,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ShareEvent {
    Sources(SourceLists),
    /// Vignette d'une source (`None` : pas d'image, la tuile garde son icône).
    Thumbnail { id: String, frame: Option<Frame> },
    /// Aperçu de la source choisie, ou pourquoi il manque (caméra refusée…).
    Preview { id: String, frame: Result<Frame, CaptureError> },
}

/// Ce que l'utilisateur a validé ; le câblage (lot M5) en fait un
/// `VideoCmd::StartShare` ou allume la caméra.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareChoice {
    pub source: Source,
    pub quality: Quality,
    /// Partager le son de l'ordinateur (écrans et fenêtres seulement).
    pub audio: bool,
}

/// Le son se partage avec un écran ou une fenêtre, pas avec une caméra.
pub fn sound_allowed(kind: SourceKind) -> bool {
    kind != SourceKind::Camera
}

/// Les caméras n'ont pas de vignette dans la liste : en faire une allumerait
/// chacune. Leur image n'est prise qu'une fois choisie (aperçu).
pub fn wants_thumbnail(kind: SourceKind) -> bool {
    kind != SourceKind::Camera
}

/// Service de capture du sélecteur. Rien ne tourne hors des demandes : pas
/// de fil ni de réveil quand le sélecteur est fermé.
pub trait CaptureService {
    /// Demande les écrans, fenêtres et caméras (réponse : [`ShareEvent::Sources`]).
    fn list(&mut self);
    /// Demande les vignettes de ces sources, une par une ([`ShareEvent::Thumbnail`]).
    fn thumbnails(&mut self, sources: &[Source]);
    /// Demande l'aperçu de la source choisie ([`ShareEvent::Preview`]).
    fn preview(&mut self, source: &Source);
    /// Abandonne les demandes en cours (sélecteur fermé, liste rafraîchie).
    fn cancel(&mut self);
    fn poll(&mut self) -> Vec<ShareEvent>;
}

/// Sans capture : tests, aperçu, avant `App::attach_capture`.
#[derive(Default)]
pub struct NoCapture;

impl CaptureService for NoCapture {
    fn list(&mut self) {}
    fn thumbnails(&mut self, _: &[Source]) {}
    fn preview(&mut self, _: &Source) {}
    fn cancel(&mut self) {}
    fn poll(&mut self) -> Vec<ShareEvent> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tabs_and_rules() {
        let s = |id: &str, kind| Source { id: id.into(), name: id.into(), kind };
        let lists = SourceLists { screens: vec![s("screen:1", SourceKind::Screen)], windows: vec![], cameras: vec![s("camera:a", SourceKind::Camera)] };
        assert_eq!(lists.of(ShareTab::Screens).len(), 1);
        assert!(lists.of(ShareTab::Windows).is_empty());
        assert_eq!(lists.of(ShareTab::Cameras)[0].id, "camera:a");
        assert!(ShareTab::ALL.iter().all(|t| lists.of(*t).iter().all(|s| s.kind == t.kind())));
        assert!(sound_allowed(SourceKind::Window) && sound_allowed(SourceKind::Screen) && !sound_allowed(SourceKind::Camera));
        assert!(!wants_thumbnail(SourceKind::Camera) && wants_thumbnail(SourceKind::Screen));
    }
}
