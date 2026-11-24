//! Capture vidéo (lot M3) : caméras (`Windows.Media.Capture`), écrans et
//! fenêtres (Windows Graphics Capture, par la crate `windows-capture`), et le
//! son d'un partage (`sound`). Seul `process_sound` contient de l'`unsafe`
//! (boucle WASAPI par processus, que Windows n'offre pas autrement).
//!
//! Rien ne tourne hors d'une capture : chaque [`open`] crée son fil, arrêté
//! quand on lâche le [`CaptureHandle`]. Les vignettes ([`thumbnail`]) ouvrent
//! une capture le temps d'une image.
//!
//! Écrans et fenêtres sortent en BGRA, mis à la taille demandée ; les
//! caméras en I420, au format de la caméra le plus proche de la qualité
//! demandée (pas de mise à l'échelle : la caméra le fait mieux).

#[cfg(windows)]
mod camera;
pub mod pixels;
#[cfg(windows)]
mod process_sound;
#[cfg(windows)]
mod screen;
pub mod service;
pub mod sound;

use super::Frame;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceKind {
    Screen,
    Window,
    Camera,
}

/// Ce qu'on peut partager. `id` n'a de sens que pour cette session de Windows
/// (écran et fenêtre : poignée système ; caméra : identifiant du périphérique).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Source {
    pub id: String,
    pub name: String,
    pub kind: SourceKind,
}

pub use super::Quality;

impl Quality {
    pub const HD_30: Quality = Quality { width: 1280, height: 720, fps: 30 };
    pub const FHD_30: Quality = Quality { width: 1920, height: 1080, fps: 30 };
    pub const FHD_60: Quality = Quality { width: 1920, height: 1080, fps: 60 };
    pub const SOURCE: Quality = Quality { width: 0, height: 0, fps: 30 };
    /// Taille d'origine à 60 images par seconde (jeux, vidéo).
    pub const SOURCE_60: Quality = Quality { width: 0, height: 0, fps: 60 };
    /// Choix du sélecteur de partage, dans l'ordre affiché.
    pub const PRESETS: [Quality; 5] = [Quality::HD_30, Quality::FHD_30, Quality::FHD_60, Quality::SOURCE, Quality::SOURCE_60];

    pub fn is_source(&self) -> bool {
        self.width == 0 || self.height == 0
    }
}

impl Default for Quality {
    fn default() -> Self {
        Quality::HD_30
    }
}

/// Réglages d'une capture d'écran ou de fenêtre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Montrer le pointeur de la souris.
    pub cursor: bool,
}

impl Default for Options {
    fn default() -> Self {
        Options { cursor: true }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    /// La source n'existe plus (fenêtre fermée, écran débranché, caméra retirée).
    NotFound,
    /// Les réglages de confidentialité de Windows refusent la caméra.
    CameraDenied,
    /// Rien à capturer sur ce système (Windows trop ancien, autre système).
    Unsupported,
    /// Erreur de Windows, telle quelle.
    Backend(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CaptureError::NotFound => write!(f, "source introuvable"),
            CaptureError::CameraDenied => write!(f, "caméra refusée par Windows"),
            CaptureError::Unsupported => write!(f, "capture non prise en charge"),
            CaptureError::Backend(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// HRESULT E_ACCESSDENIED : refus de la confidentialité de Windows.
pub const E_ACCESSDENIED: i32 = 0x8007_0005_u32 as i32;

/// Capture en cours : elle s'arrête quand on la lâche.
pub struct CaptureHandle {
    stop: Option<Box<dyn FnOnce() + Send>>,
    ended: Arc<AtomicBool>,
}

impl CaptureHandle {
    // Hors Windows, aucune capture ne démarre (phase 7 : Android sans partage d'écran).
    #[cfg_attr(not(windows), allow(dead_code))]
    pub(crate) fn new(stop: Box<dyn FnOnce() + Send>, ended: Arc<AtomicBool>) -> Self {
        CaptureHandle { stop: Some(stop), ended }
    }

    /// La source a disparu (fenêtre fermée, caméra débranchée) : plus aucune image ne viendra.
    pub fn ended(&self) -> bool {
        self.ended.load(Ordering::Relaxed)
    }
}

impl Drop for CaptureHandle {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            stop();
        }
    }
}

/// Tampons de pixels rendus par l'encodeur, repris par la capture.
///
/// Une image d'écran en 4K pèse 33 Mo : en allouer une neuve soixante fois
/// par seconde, c'est autant de mémoire demandée à Windows, mise à zéro puis
/// rendue à chaque image. Deux tampons tournent à la place (celui que la
/// capture remplit, celui que l'encodeur tient). Rien ne survit au partage :
/// [`drop_spares`] les rend à sa fin.
static SPARES: std::sync::Mutex<Vec<Vec<u8>>> = std::sync::Mutex::new(Vec::new());
const MAX_SPARES: usize = 2;

/// Rend le tampon d'une image qui ne servira plus.
pub fn recycle(f: Frame) {
    if let crate::video::FrameData::Bgra(v) = f.data {
        let mut spares = SPARES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if spares.len() < MAX_SPARES {
            spares.push(v);
        }
    }
}

/// Tampon vide d'au moins `len` octets de capacité : un tampon rendu s'il
/// convient, un neuf sinon.
#[cfg_attr(not(windows), allow(dead_code))]
pub(crate) fn spare(len: usize) -> Vec<u8> {
    let mut spares = SPARES.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    match spares.iter().position(|v| v.capacity() >= len) {
        Some(i) => {
            let mut v = spares.swap_remove(i);
            v.clear();
            v
        }
        None => {
            // Trop petits (la source a grandi) : ils ne serviront plus.
            spares.clear();
            Vec::with_capacity(len)
        }
    }
}

/// Fin d'un partage : les tampons en attente sont libérés.
pub fn drop_spares() {
    SPARES.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
}

const SCREEN: &str = "screen:";
const WINDOW: &str = "window:";
const CAMERA: &str = "camera:";

pub(crate) fn source_id(kind: SourceKind, raw: &str) -> String {
    let prefix = match kind {
        SourceKind::Screen => SCREEN,
        SourceKind::Window => WINDOW,
        SourceKind::Camera => CAMERA,
    };
    format!("{prefix}{raw}")
}

/// Partie système d'un identifiant, si son préfixe correspond à `kind`.
pub(crate) fn raw_id(s: &Source) -> Option<&str> {
    let prefix = match s.kind {
        SourceKind::Screen => SCREEN,
        SourceKind::Window => WINDOW,
        SourceKind::Camera => CAMERA,
    };
    s.id.strip_prefix(prefix)
}

/// Une fenêtre de premier niveau telle que Windows la donne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WindowInfo {
    pub handle: usize,
    pub title: String,
    pub pid: u32,
    /// Nom de l'exécutable, s'il a pu être lu.
    pub exe: Option<String>,
    /// Rectangle de la fenêtre (gauche, haut, largeur, hauteur).
    pub rect: (i32, i32, i32, i32),
}

/// Programmes du système dont les fenêtres « visibles » ne se voient jamais
/// (claviers tactiles, menu Démarrer, recherche, écran de verrouillage).
const SHELL_EXES: [&str; 6] = ["textinputhost.exe", "shellexperiencehost.exe", "startmenuexperiencehost.exe", "searchhost.exe", "lockapp.exe", "shellhost.exe"];
/// Fenêtre du bureau d'`explorer.exe`.
const DESKTOP_TITLE: &str = "Program Manager";
/// Position où Windows range les fenêtres réduites.
const MINIMIZED_AT: i32 = -32000;

/// Fenêtres proposées au partage : titrées, ni réduites ni minuscules, ni
/// d'un programme du système, ni de Ferrocord (`own_pid`). Les fenêtres
/// cachées, d'outil et enfants sont déjà écartées par l'énumération.
pub fn filter_windows(list: Vec<WindowInfo>, own_pid: u32) -> Vec<Source> {
    list.into_iter()
        .filter(|w| {
            let title = w.title.trim();
            let exe = w.exe.as_deref().map(str::to_ascii_lowercase);
            let (x, y, width, height) = w.rect;
            !title.is_empty()
                && w.pid != own_pid
                && !(title == DESKTOP_TITLE && exe.as_deref().is_none_or(|e| e == "explorer.exe"))
                && !exe.as_deref().is_some_and(|e| SHELL_EXES.contains(&e))
                && x > MINIMIZED_AT
                && y > MINIMIZED_AT
                && width >= 32
                && height >= 32
        })
        .map(|w| Source { id: source_id(SourceKind::Window, &w.handle.to_string()), name: w.title.trim().to_string(), kind: SourceKind::Window })
        .collect()
}

/// Taille de sortie pour une source de `w` × `h` : dans la boîte de `want`
/// sans jamais agrandir, proportions gardées, côtés pairs (I420).
pub fn fit(w: u32, h: u32, want: Quality) -> (u32, u32) {
    let even = |n: u32| (n & !1).max(2);
    if w == 0 || h == 0 {
        return (2, 2);
    }
    if want.is_source() {
        return (even(w), even(h));
    }
    let scale = (f64::from(want.width) / f64::from(w)).min(f64::from(want.height) / f64::from(h)).min(1.0);
    let side = |n: u32| even((f64::from(n) * scale).round() as u32);
    (side(w), side(h))
}

/// Un format proposé par une caméra.
#[derive(Clone, Debug, PartialEq)]
pub struct CamFormat {
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    /// Sous-type Media Foundation (« NV12 », « YUY2 », « MJPG »…).
    pub subtype: String,
}

/// Format de caméra le plus proche de `want` : la définition d'abord (plus
/// petite que voulue : pénalisée), puis la fréquence (plus lente : pénalisée),
/// puis les formats bruts (NV12, YUY2) plutôt que compressés. « Source » :
/// la plus grande définition.
pub fn closest_format(formats: &[CamFormat], want: Quality) -> Option<usize> {
    let max_area = formats.iter().map(|f| u64::from(f.width) * u64::from(f.height)).max()?;
    let want_area = if want.is_source() { max_area as f64 } else { f64::from(want.width) * f64::from(want.height) };
    let want_fps = f64::from(want.fps.max(1));
    let cost = |f: &CamFormat| {
        let area = (f64::from(f.width) * f64::from(f.height)).max(1.0);
        let fps = f64::from(f.fps).max(0.1);
        let res = (area / want_area).ln().abs() + if area < want_area { 0.3 } else { 0.0 };
        let rate = 0.5 * (fps / want_fps).ln().abs() + if fps < want_fps * 0.9 { 0.2 } else { 0.0 };
        let kind = match f.subtype.to_ascii_uppercase().as_str() {
            "NV12" => 0.0,
            "YUY2" => 0.02,
            _ => 0.05,
        };
        res + rate + kind
    };
    formats.iter().enumerate().filter(|(_, f)| f.width > 0 && f.height > 0).min_by(|a, b| cost(a.1).total_cmp(&cost(b.1))).map(|(i, _)| i)
}

/// Capture (caméra, écrans, fenêtres) possible sur ce système : Windows
/// seulement pour l'instant (Android : boutons masqués, phase 7).
pub const AVAILABLE: bool = cfg!(windows);

/// Caméras branchées (vide si aucune, ou hors de Windows).
pub fn cameras() -> Vec<Source> {
    #[cfg(windows)]
    return camera::list();
    #[cfg(not(windows))]
    Vec::new()
}

/// Écrans, dans l'ordre de Windows.
pub fn screens() -> Vec<Source> {
    #[cfg(windows)]
    return screen::screens();
    #[cfg(not(windows))]
    Vec::new()
}

/// Fenêtres partageables (voir [`filter_windows`]).
pub fn windows() -> Vec<Source> {
    #[cfg(windows)]
    return filter_windows(screen::windows(), std::process::id());
    #[cfg(not(windows))]
    Vec::new()
}

/// Les vignettes peuvent être reprises sans rien montrer à l'écran : Windows
/// sait capturer sans son cadre jaune (Windows 11). Faux : chaque capture
/// ferait clignoter le cadre, les vignettes ne sont prises qu'une fois.
pub fn quiet_snapshots() -> bool {
    #[cfg(windows)]
    return screen::borderless();
    #[cfg(not(windows))]
    false
}

/// Une image de la source, en BGRA, large d'au plus `max_w` pixels.
pub fn thumbnail(s: &Source, max_w: u32) -> Option<Frame> {
    snapshot(s, max_w).ok()
}

/// Comme [`thumbnail`], avec la raison de l'échec (caméra refusée…).
pub fn snapshot(s: &Source, max_w: u32) -> Result<Frame, CaptureError> {
    let raw = raw_id(s).ok_or(CaptureError::NotFound)?;
    #[cfg(windows)]
    return match s.kind {
        SourceKind::Camera => camera::snapshot(raw, max_w),
        SourceKind::Screen | SourceKind::Window => screen::snapshot(s.kind, raw, max_w),
    };
    #[cfg(not(windows))]
    {
        let _ = (raw, max_w);
        Err(CaptureError::Unsupported)
    }
}

/// Lance la capture ; `on_frame` est appelé sur le fil de capture.
pub fn open(s: &Source, want: Quality, on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<CaptureHandle, CaptureError> {
    open_with(s, want, Options::default(), on_frame)
}

/// [`open`] avec les réglages (pointeur de la souris).
pub fn open_with(s: &Source, want: Quality, opts: Options, on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<CaptureHandle, CaptureError> {
    let raw = raw_id(s).ok_or(CaptureError::NotFound)?;
    #[cfg(windows)]
    return match s.kind {
        SourceKind::Camera => camera::open(raw, want, on_frame),
        SourceKind::Screen | SourceKind::Window => screen::open(s.kind, raw, want, opts, on_frame),
    };
    #[cfg(not(windows))]
    {
        let _ = (raw, want, opts, on_frame);
        Err(CaptureError::Unsupported)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(handle: usize, title: &str, pid: u32, exe: Option<&str>, rect: (i32, i32, i32, i32)) -> WindowInfo {
        WindowInfo { handle, title: title.into(), pid, exe: exe.map(str::to_string), rect }
    }

    #[test]
    fn window_filter() {
        let r = (10, 10, 800, 600);
        let list = vec![
            win(1, "Bloc-notes", 100, Some("notepad.exe"), r),
            win(2, "   ", 100, Some("notepad.exe"), r),
            win(3, "Ferrocord", 42, Some("ferrocord.exe"), r),
            win(4, "Program Manager", 7, Some("explorer.exe"), (0, 0, 2560, 1440)),
            win(5, "Expérience d'entrée Windows", 8, Some("TextInputHost.exe"), r),
            win(6, "Réduite", 9, None, (-32000, -32000, 160, 28)),
            win(7, "Minuscule", 9, None, (0, 0, 1, 1)),
            win(8, " Navigateur ", 11, None, r),
            win(9, "Explorateur", 7, Some("explorer.exe"), r),
        ];
        let got = filter_windows(list, 42);
        let names: Vec<&str> = got.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, ["Bloc-notes", "Navigateur", "Explorateur"]);
        assert_eq!(got[0].id, "window:1");
        assert!(got.iter().all(|s| s.kind == SourceKind::Window));
    }

    #[test]
    fn ids_round_trip() {
        let s = Source { id: source_id(SourceKind::Camera, r"\\?\usb#vid"), name: "Cam".into(), kind: SourceKind::Camera };
        assert_eq!(raw_id(&s), Some(r"\\?\usb#vid"));
        let wrong = Source { kind: SourceKind::Screen, ..s };
        assert_eq!(raw_id(&wrong), None, "préfixe d'un autre genre");
    }

    #[test]
    fn fit_keeps_ratio_and_never_upscales() {
        assert_eq!(fit(2560, 1440, Quality::HD_30), (1280, 720));
        assert_eq!(fit(3440, 1440, Quality::FHD_30), (1920, 804));
        assert_eq!(fit(800, 600, Quality::FHD_60), (800, 600));
        assert_eq!(fit(1001, 777, Quality::SOURCE), (1000, 776));
        assert_eq!(fit(600, 1200, Quality::HD_30), (360, 720), "fenêtre en hauteur");
        assert_eq!(fit(0, 10, Quality::HD_30), (2, 2));
    }

    fn fmt(width: u32, height: u32, fps: f32, subtype: &str) -> CamFormat {
        CamFormat { width, height, fps, subtype: subtype.into() }
    }

    #[test]
    fn closest_camera_format() {
        let list = [
            fmt(640, 480, 30.0, "YUY2"),
            fmt(1280, 720, 30.0, "MJPG"),
            fmt(1280, 720, 30.0, "NV12"),
            fmt(1920, 1080, 30.0, "MJPG"),
            fmt(1280, 720, 10.0, "YUY2"),
            fmt(1280, 720, 60.0, "MJPG"),
        ];
        assert_eq!(closest_format(&list, Quality::HD_30), Some(2), "même taille, même fréquence, brut");
        assert_eq!(closest_format(&list, Quality::FHD_30), Some(3));
        assert_eq!(closest_format(&list, Quality::FHD_60), Some(3), "la définition passe avant la fréquence");
        assert_eq!(closest_format(&list, Quality::SOURCE), Some(3));
        assert_eq!(closest_format(&list[..1], Quality::FHD_60), Some(0), "un seul format : celui-là");
        assert_eq!(closest_format(&[], Quality::HD_30), None);
        let small = [fmt(640, 480, 30.0, "NV12"), fmt(1280, 720, 15.0, "NV12")];
        assert_eq!(closest_format(&small, Quality::HD_30), Some(1), "720p à 15 i/s plutôt que 480p");
    }

    #[test]
    fn spare_buffers_are_reused_then_dropped() {
        drop_spares();
        let mut v = spare(1000);
        v.extend_from_slice(&[7; 1000]);
        let ptr = v.as_ptr();
        recycle(Frame { width: 1, height: 1, pts_us: 0, data: crate::video::FrameData::Bgra(v) });
        let again = spare(800);
        assert!(again.is_empty() && again.capacity() >= 1000);
        assert_eq!(again.as_ptr(), ptr, "le même tampon, sans nouvelle allocation");
        recycle(Frame { width: 1, height: 1, pts_us: 0, data: crate::video::FrameData::Bgra(again) });
        // Plus grand que ce qui attend : un neuf, et l'ancien est lâché.
        assert!(spare(5000).capacity() >= 5000);
        assert!(SPARES.lock().unwrap().is_empty());
        for _ in 0..5 {
            recycle(Frame { width: 1, height: 1, pts_us: 0, data: crate::video::FrameData::Bgra(vec![0; 16]) });
        }
        assert_eq!(SPARES.lock().unwrap().len(), MAX_SPARES);
        drop_spares();
        assert!(SPARES.lock().unwrap().is_empty());
    }

    #[test]
    fn presets() {
        assert!(Quality::SOURCE.is_source() && !Quality::HD_30.is_source());
        // « Source » existe aux deux cadences ; la taille ne dépend pas de la cadence.
        assert!(Quality::SOURCE_60.is_source() && Quality::SOURCE_60.fps == 60 && Quality::SOURCE_60 != Quality::SOURCE);
        assert_eq!(fit(1001, 777, Quality::SOURCE_60), (1000, 776));
        assert_eq!(Quality::PRESETS.len(), 5);
        assert_eq!(Quality::default(), Quality::HD_30);
    }
}

#[cfg(all(test, windows))]
mod live_tests {
    use super::*;

    /// Capture réelle du premier écran (à lancer à la main : `--ignored`).
    #[test]
    #[ignore]
    fn a_real_screen_is_captured_through_the_kept_texture() {
        let Some(s) = screens().into_iter().next() else { return };
        let snap = snapshot(&s, 640).expect("vignette");
        let crate::video::FrameData::Bgra(px) = &snap.data else { panic!("BGRA attendu") };
        assert_eq!(px.len(), 4 * snap.width as usize * snap.height as usize);
        assert!(px.iter().any(|b| *b != 0), "image toute noire");
        let seen = std::sync::Arc::new(std::sync::Mutex::new((0u32, 0usize, (0u32, 0u32))));
        let s2 = seen.clone();
        let h = open(
            &s,
            Quality::SOURCE_60,
            Box::new(move |f| {
                let mut g = s2.lock().unwrap();
                g.0 += 1;
                g.2 = (f.width, f.height);
                if let crate::video::FrameData::Bgra(v) = &f.data {
                    g.1 = v.len();
                }
                recycle(f);
            }),
        )
        .expect("capture");
        std::thread::sleep(std::time::Duration::from_secs(3));
        drop(h);
        let g = seen.lock().unwrap();
        eprintln!("{} images en 3 s, {}x{}, {} octets", g.0, g.2.0, g.2.1, g.1);
        assert!(g.0 >= 1);
        assert_eq!(g.1, 4 * g.2.0 as usize * g.2.1 as usize);
        drop_spares();
    }
}
