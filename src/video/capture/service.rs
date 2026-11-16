//! Service de capture réel du sélecteur de partage (trait
//! `state::share::CaptureService`) : un fil de suivi tant que le sélecteur
//! est ouvert (listes, puis vignettes de l'onglet montré, reprises à un
//! rythme modéré), un fil éphémère pour l'aperçu de la caméra. Tout s'arrête
//! à [`CaptureService::cancel`] ; rien ne tourne quand le sélecteur est fermé.
//!
//! Coût d'un tour de vignettes : chaque vignette ouvre une capture de Windows
//! le temps d'une image (un fil, un périphérique D3D11, une copie réduite à
//! [`THUMB_WIDTH`]). Le suivi ne prend donc que l'onglet montré, attend entre
//! deux tours [`REFRESH_EVERY`] et au moins trois fois la durée du tour
//! (un quart de fil au plus), et ne réveille l'interface que pour une image
//! qui a changé. Sans le réglage « sans bordure » de Windows (avant
//! Windows 11), chaque capture ferait clignoter le cadre jaune : les
//! vignettes ne sont alors prises qu'une fois par source.

use super::{cameras, screens, snapshot, windows, Source};
use crate::state::share::{wants_thumbnail, CaptureService, ShareEvent, ShareTab, SourceLists, PREVIEW_WIDTH, THUMB_WIDTH};
use crate::video::{Frame, FrameData};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

/// Vignettes faites au plus par tour (une capture chacune).
const MAX_THUMBNAILS: usize = 48;
/// Pause entre deux tours de vignettes, au moins.
const REFRESH_EVERY: Duration = Duration::from_secs(2);
/// Pause au plus (bureau très chargé : le tour lui-même est long).
const REFRESH_SLOWEST: Duration = Duration::from_secs(10);
/// Pas de la pause : délai au plus entre la fermeture du sélecteur et la fin du fil.
const STEP: Duration = Duration::from_millis(100);

pub struct CaptureDevices {
    wake: Arc<dyn Fn() + Send + Sync>,
    tx: mpsc::Sender<ShareEvent>,
    rx: mpsc::Receiver<ShareEvent>,
    generation: Arc<AtomicU64>,
    /// Numéro du suivi en cours : un suivi plus récent arrête le précédent
    /// sans jeter l'aperçu demandé entre-temps.
    watching: Arc<AtomicU64>,
}

/// Empreinte d'une image : deux vignettes égales ne réveillent pas l'interface.
fn mark(f: &Frame) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (f.width, f.height).hash(&mut h);
    if let FrameData::Bgra(px) = &f.data {
        px.hash(&mut h);
    }
    h.finish()
}

/// Ce que le suivi a déjà annoncé à l'interface.
#[derive(Default)]
struct Told {
    lists: Option<SourceLists>,
    /// Empreinte de la vignette annoncée par source (`None` : pas d'image).
    marks: HashMap<String, Option<u64>>,
}

impl Told {
    /// Un tour : annonce `lists` si elles ont changé, puis les vignettes de
    /// `tab` qui manquent ou qui ont changé (`refresh` faux : seulement
    /// celles qui manquent). Une source déjà montrée garde son image quand
    /// la capture échoue (fenêtre réduite entre-temps). Faux : suivi arrêté.
    fn round(&mut self, lists: SourceLists, tab: ShareTab, refresh: bool, shoot: &dyn Fn(&Source) -> Option<Frame>, send: &dyn Fn(ShareEvent) -> bool) -> bool {
        if self.lists.as_ref() != Some(&lists) {
            self.marks.retain(|id, _| ShareTab::ALL.iter().any(|t| lists.of(*t).iter().any(|s| &s.id == id)));
            self.lists = Some(lists.clone());
            if !send(ShareEvent::Sources(lists.clone())) {
                return false;
            }
        }
        for s in lists.of(tab).iter().filter(|s| wants_thumbnail(s.kind)).take(MAX_THUMBNAILS) {
            let before = self.marks.get(&s.id).copied();
            if before.is_some() && !refresh {
                continue;
            }
            let frame = shoot(s);
            let now = frame.as_ref().map(mark);
            let news = match before {
                None => true,
                Some(old) => now.is_some() && now != old,
            };
            if news {
                self.marks.insert(s.id.clone(), now);
                if !send(ShareEvent::Thumbnail { id: s.id.clone(), frame }) {
                    return false;
                }
            }
        }
        true
    }
}

impl CaptureDevices {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        CaptureDevices { wake, tx, rx, generation: Arc::default(), watching: Arc::default() }
    }

    fn spawn(&self, name: &str, job: impl FnOnce(&dyn Fn(ShareEvent) -> bool) + Send + 'static) {
        let (tx, wake, generation) = (self.tx.clone(), self.wake.clone(), self.generation.clone());
        let mine = generation.load(Ordering::Relaxed);
        let _ = std::thread::Builder::new().name(name.into()).spawn(move || {
            // Faux quand la demande est périmée : le travail s'arrête.
            let send = |e: ShareEvent| {
                if generation.load(Ordering::Relaxed) != mine {
                    return false;
                }
                let ok = tx.send(e).is_ok();
                wake();
                ok
            };
            job(&send);
        });
    }
}

impl CaptureService for CaptureDevices {
    fn watch(&mut self, tab: ShareTab) {
        let turn = self.watching.fetch_add(1, Ordering::Relaxed) + 1;
        let watching = self.watching.clone();
        self.spawn("ferrocord-sources", move |send| {
            let live = || watching.load(Ordering::Relaxed) == turn;
            let send = |e: ShareEvent| live() && send(e);
            let refresh = super::quiet_snapshots();
            let mut told = Told::default();
            let mut cams = cameras();
            loop {
                let started = Instant::now();
                // Les caméras ne sont relevées de nouveau que sur leur onglet (énumération plus lente).
                if tab == ShareTab::Cameras && told.lists.is_some() {
                    cams = cameras();
                }
                let lists = SourceLists { screens: screens(), windows: windows(), cameras: cams.clone() };
                let shoot = |s: &Source| if live() { snapshot(s, THUMB_WIDTH).ok() } else { None };
                if !told.round(lists, tab, refresh, &shoot, &send) {
                    return;
                }
                let pause = (started.elapsed() * 3).clamp(REFRESH_EVERY, REFRESH_SLOWEST);
                let until = Instant::now() + pause;
                while Instant::now() < until {
                    if !live() {
                        return;
                    }
                    std::thread::sleep(STEP);
                }
            }
        });
    }

    fn preview(&mut self, source: &Source) {
        let s = source.clone();
        self.spawn("ferrocord-apercu", move |send| {
            send(ShareEvent::Preview { frame: snapshot(&s, PREVIEW_WIDTH), id: s.id });
        });
    }

    fn cancel(&mut self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
        self.watching.fetch_add(1, Ordering::Relaxed);
        while self.rx.try_recv().is_ok() {}
    }

    fn poll(&mut self) -> Vec<ShareEvent> {
        self.rx.try_iter().collect()
    }
}

impl Drop for CaptureDevices {
    fn drop(&mut self) {
        // Le fil de suivi s'arrête même si personne n'a fermé le sélecteur.
        self.watching.fetch_add(1, Ordering::Relaxed);
    }
}

/// Définition demandée à la caméra pour « Tester la vidéo » : assez pour
/// l'aperçu de la page, légère à convertir.
const TEST_QUALITY: super::Quality = super::Quality { width: 640, height: 360, fps: 30 };

/// Test de caméra en cours : le fil garde la capture ouverte jusqu'à `stop`.
struct LiveTest {
    stop: Arc<std::sync::atomic::AtomicBool>,
    slot: Arc<std::sync::Mutex<Option<crate::video::Frame>>>,
    thread: std::thread::Thread,
}

/// Service réel de la caméra de la page « Voix et vidéo » (trait
/// `state::video_prefs::CameraService`). La caméra n'est ouverte que
/// pendant le test ; lâcher le service l'arrête.
pub struct CameraTest {
    wake: Arc<dyn Fn() + Send + Sync>,
    tx: mpsc::Sender<crate::state::video_prefs::CameraEvent>,
    rx: mpsc::Receiver<crate::state::video_prefs::CameraEvent>,
    live: Option<LiveTest>,
}

impl CameraTest {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        CameraTest { wake, tx, rx, live: None }
    }
}

impl crate::state::video_prefs::CameraService for CameraTest {
    fn list(&mut self) {
        let (tx, wake) = (self.tx.clone(), self.wake.clone());
        let _ = std::thread::Builder::new().name("ferrocord-cameras".into()).spawn(move || {
            let _ = tx.send(crate::state::video_prefs::CameraEvent::Cameras(cameras()));
            wake();
        });
    }

    fn start(&mut self, camera: Option<&str>) {
        use crate::state::video_prefs::CameraEvent;
        use std::sync::atomic::AtomicBool;
        self.stop();
        let stop = Arc::new(AtomicBool::new(false));
        let slot: Arc<std::sync::Mutex<Option<crate::video::Frame>>> = Arc::default();
        let (tx, wake, wanted) = (self.tx.clone(), self.wake.clone(), camera.map(str::to_string));
        let (stopped, images) = (stop.clone(), slot.clone());
        let spawned = std::thread::Builder::new().name("ferrocord-test-camera".into()).spawn(move || {
            // Faux une fois le test arrêté : plus rien n'est annoncé.
            let fail = |e| {
                if !stopped.load(Ordering::Relaxed) {
                    let _ = tx.send(CameraEvent::Failed(e));
                    wake();
                }
            };
            // Caméra choisie débranchée : la première de Windows, comme dans un appel.
            let list = cameras();
            let source = wanted.and_then(|id| list.iter().find(|s| s.id == id).cloned()).or_else(|| list.into_iter().next());
            let Some(source) = source else { return fail(super::CaptureError::NotFound) };
            let (on, wake_frame) = (stopped.clone(), wake.clone());
            let on_frame = Box::new(move |f: crate::video::Frame| {
                if !on.load(Ordering::Relaxed) {
                    *images.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(f);
                    wake_frame();
                }
            });
            match super::open(&source, TEST_QUALITY, on_frame) {
                Ok(capture) => {
                    while !stopped.load(Ordering::Relaxed) {
                        if capture.ended() {
                            return fail(super::CaptureError::NotFound);
                        }
                        std::thread::park_timeout(std::time::Duration::from_millis(250));
                    }
                }
                Err(e) => fail(e),
            }
        });
        match spawned {
            Ok(handle) => self.live = Some(LiveTest { stop, slot, thread: handle.thread().clone() }),
            Err(e) => {
                let _ = self.tx.send(CameraEvent::Failed(super::CaptureError::Backend(e.to_string())));
            }
        }
    }

    fn stop(&mut self) {
        if let Some(live) = self.live.take() {
            live.stop.store(true, Ordering::Relaxed);
            live.thread.unpark();
        }
    }

    fn running(&self) -> bool {
        self.live.is_some()
    }

    fn frame(&mut self) -> Option<crate::video::Frame> {
        self.live.as_ref()?.slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take()
    }

    fn poll(&mut self) -> Vec<crate::state::video_prefs::CameraEvent> {
        self.rx.try_iter().collect()
    }
}

impl Drop for CameraTest {
    fn drop(&mut self) {
        crate::state::video_prefs::CameraService::stop(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::video::capture::SourceKind;
    use std::cell::RefCell;

    fn src(id: &str, kind: SourceKind) -> Source {
        Source { id: id.into(), name: id.into(), kind }
    }

    fn image(shade: u8) -> Frame {
        Frame { width: 2, height: 2, pts_us: 0, data: FrameData::Bgra(vec![shade; 16]) }
    }

    /// Le suivi n'annonce que ce qui change : listes, puis vignettes de
    /// l'onglet montré ; une caméra n'est jamais prise en vignette.
    #[test]
    fn a_round_only_tells_what_changed() {
        let lists = SourceLists {
            screens: vec![src("screen:1", SourceKind::Screen)],
            windows: vec![src("window:1", SourceKind::Window), src("window:2", SourceKind::Window)],
            cameras: vec![src("camera:a", SourceKind::Camera)],
        };
        let sent = RefCell::new(Vec::new());
        let shots = RefCell::new(Vec::new());
        let shade = RefCell::new(HashMap::from([("window:1".to_string(), Some(1u8)), ("window:2".to_string(), None), ("screen:1".to_string(), Some(9))]));
        let send = |e: ShareEvent| {
            sent.borrow_mut().push(e);
            true
        };
        let shoot = |s: &Source| {
            shots.borrow_mut().push(s.id.clone());
            shade.borrow().get(&s.id).copied().flatten().map(image)
        };
        let names = |v: &RefCell<Vec<ShareEvent>>| -> Vec<String> {
            v.borrow_mut()
                .drain(..)
                .map(|e| match e {
                    ShareEvent::Sources(_) => "listes".to_string(),
                    ShareEvent::Thumbnail { id, frame } => format!("{id}={}", frame.is_some()),
                    ShareEvent::Preview { .. } => "aperçu".to_string(),
                })
                .collect()
        };
        let mut told = Told::default();
        assert!(told.round(lists.clone(), ShareTab::Windows, true, &shoot, &send));
        assert_eq!(names(&sent), ["listes", "window:1=true", "window:2=false"]);
        // Rien n'a bougé : deux captures, aucune annonce (l'interface dort).
        shots.borrow_mut().clear();
        assert!(told.round(lists.clone(), ShareTab::Windows, true, &shoot, &send));
        assert_eq!((names(&sent).len(), shots.borrow().len()), (0, 2));
        // Une image change, l'autre apparaît ; une capture ratée ne retire pas l'image montrée.
        shade.borrow_mut().insert("window:1".into(), Some(2));
        shade.borrow_mut().insert("window:2".into(), Some(3));
        assert!(told.round(lists.clone(), ShareTab::Windows, true, &shoot, &send));
        assert_eq!(names(&sent), ["window:1=true", "window:2=true"]);
        shade.borrow_mut().insert("window:1".into(), None);
        assert!(told.round(lists.clone(), ShareTab::Windows, true, &shoot, &send));
        assert!(names(&sent).is_empty());
        // Sans rafraîchissement (cadre jaune) : seules les sources jamais prises le sont.
        shots.borrow_mut().clear();
        let mut more = lists.clone();
        more.windows.push(src("window:3", SourceKind::Window));
        assert!(told.round(more.clone(), ShareTab::Windows, false, &shoot, &send));
        assert_eq!((names(&sent), shots.borrow().clone()), (vec!["listes".to_string(), "window:3=false".to_string()], vec!["window:3".to_string()]));
        // Onglet des caméras : aucune capture.
        shots.borrow_mut().clear();
        assert!(told.round(more.clone(), ShareTab::Cameras, true, &shoot, &send));
        assert!(names(&sent).is_empty() && shots.borrow().is_empty());
        // Suivi arrêté : le tour s'interrompt à la première annonce refusée.
        let refuse = |_: ShareEvent| false;
        assert!(!told.round(more, ShareTab::Screens, true, &shoot, &refuse));
    }
}
