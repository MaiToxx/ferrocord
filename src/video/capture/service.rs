//! Service de capture réel du sélecteur de partage (trait
//! `state::share::CaptureService`) : listes, vignettes et aperçu dans des
//! fils éphémères, qui s'arrêtent d'eux-mêmes ; une demande plus récente ou
//! [`CaptureService::cancel`] fait abandonner les vignettes restantes.

use super::{cameras, screens, snapshot, windows, Source};
use crate::state::share::{wants_thumbnail, CaptureService, ShareEvent, SourceLists, PREVIEW_WIDTH, THUMB_WIDTH};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

/// Vignettes faites au plus par demande (une capture chacune).
const MAX_THUMBNAILS: usize = 48;

pub struct CaptureDevices {
    wake: Arc<dyn Fn() + Send + Sync>,
    tx: mpsc::Sender<ShareEvent>,
    rx: mpsc::Receiver<ShareEvent>,
    generation: Arc<AtomicU64>,
}

impl CaptureDevices {
    pub fn new(wake: Arc<dyn Fn() + Send + Sync>) -> Self {
        let (tx, rx) = mpsc::channel();
        CaptureDevices { wake, tx, rx, generation: Arc::default() }
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
    fn list(&mut self) {
        self.spawn("ferrocord-sources", |send| {
            send(ShareEvent::Sources(SourceLists { screens: screens(), windows: windows(), cameras: cameras() }));
        });
    }

    fn thumbnails(&mut self, sources: &[Source]) {
        let list: Vec<Source> = sources.iter().filter(|s| wants_thumbnail(s.kind)).take(MAX_THUMBNAILS).cloned().collect();
        self.spawn("ferrocord-vignettes", move |send| {
            for s in list {
                let frame = snapshot(&s, THUMB_WIDTH).ok();
                if !send(ShareEvent::Thumbnail { id: s.id, frame }) {
                    return;
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
        while self.rx.try_recv().is_ok() {}
    }

    fn poll(&mut self) -> Vec<ShareEvent> {
        self.rx.try_iter().collect()
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
