//! Sélecteur de partage (lot M3) : ce que l'interface demande au service de
//! capture et ce qu'il répond. L'interface ne touche jamais aux écrans, aux
//! fenêtres ni aux caméras : elle passe par [`CaptureService`], dont
//! l'implémentation réelle est `video::capture::service::CaptureDevices`.

use crate::video::capture::{CaptureError, Quality, Source, SourceKind};
use crate::video::Frame;

/// Largeur des vignettes de la grille (tuiles d'une demi-fenêtre), et de
/// l'aperçu de la caméra choisie.
pub const THUMB_WIDTH: u32 = 480;
pub const PREVIEW_WIDTH: u32 = 960;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShareTab {
    /// « Applications » : les fenêtres. Onglet d'ouverture, comme dans le client.
    #[default]
    Windows,
    /// « Écran entier ».
    Screens,
    /// « Appareils » : les caméras.
    Cameras,
}

impl ShareTab {
    /// Dans l'ordre de la barre d'onglets.
    pub const ALL: [ShareTab; 3] = [ShareTab::Windows, ShareTab::Screens, ShareTab::Cameras];

    pub fn kind(self) -> SourceKind {
        match self {
            ShareTab::Screens => SourceKind::Screen,
            ShareTab::Windows => SourceKind::Window,
            ShareTab::Cameras => SourceKind::Camera,
        }
    }

    /// Onglet où se range une source de ce genre.
    pub fn of(kind: SourceKind) -> ShareTab {
        match kind {
            SourceKind::Screen => ShareTab::Screens,
            SourceKind::Window => ShareTab::Windows,
            SourceKind::Camera => ShareTab::Cameras,
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
    /// Aperçu de la caméra choisie, ou pourquoi il manque (caméra refusée…).
    Preview { id: String, frame: Result<Frame, CaptureError> },
}

/// Ce que l'utilisateur a choisi ; le câblage (lot M5) en fait un
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

/// Les caméras n'ont pas de vignette dans la grille : en faire une allumerait
/// chacune. Leur image n'est prise qu'une fois choisie (aperçu).
pub fn wants_thumbnail(kind: SourceKind) -> bool {
    kind != SourceKind::Camera
}

/// Service de capture du sélecteur. Rien ne tourne hors des demandes : pas
/// de fil ni de réveil quand le sélecteur est fermé.
pub trait CaptureService {
    /// Suit ce qui peut être partagé tant que le sélecteur est ouvert : les
    /// écrans, fenêtres et caméras ([`ShareEvent::Sources`], d'abord puis à
    /// chaque changement) et les vignettes de l'onglet `tab`
    /// ([`ShareEvent::Thumbnail`], d'abord puis quand l'image a changé), à un
    /// rythme modéré. Remplace le suivi précédent ; [`CaptureService::cancel`]
    /// l'arrête.
    fn watch(&mut self, tab: ShareTab);
    /// Demande l'aperçu de la caméra choisie ([`ShareEvent::Preview`]).
    fn preview(&mut self, source: &Source);
    /// Abandonne le suivi et les demandes en cours (sélecteur fermé).
    fn cancel(&mut self);
    fn poll(&mut self) -> Vec<ShareEvent>;
}

/// Sans capture : tests, aperçu, avant `App::attach_capture`.
#[derive(Default)]
pub struct NoCapture;

impl CaptureService for NoCapture {
    fn watch(&mut self, _: ShareTab) {}
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
        assert!(ShareTab::ALL.iter().all(|t| ShareTab::of(t.kind()) == *t));
        assert_eq!((ShareTab::default(), ShareTab::ALL[0]), (ShareTab::Windows, ShareTab::Windows), "ouverture sur « Applications », comme le client");
        assert!(sound_allowed(SourceKind::Window) && sound_allowed(SourceKind::Screen) && !sound_allowed(SourceKind::Camera));
        assert!(!wants_thumbnail(SourceKind::Camera) && wants_thumbnail(SourceKind::Screen));
    }
}
