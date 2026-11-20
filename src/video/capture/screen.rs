//! Écrans et fenêtres par Windows Graphics Capture (crate `windows-capture`,
//! qui porte l'interop COM et D3D11). Chaque capture a son fil, créé par la
//! crate ; les pixels sont lus par une texture intermédiaire D3D11 (copie
//! GPU puis lecture mappée), puis réduits à la taille voulue.
//!
//! Bordure jaune de Windows : comportement standard pendant un partage (on
//! ne la retire pas, c'est l'indicateur de Windows). Seules les vignettes la
//! retirent quand Windows le permet, pour ne pas faire clignoter chaque fenêtre.

use super::pixels::scale_bgra;
use super::{fit, source_id, CaptureError, CaptureHandle, Options, Quality, Source, SourceKind, WindowInfo};
use crate::video::{Frame, FrameData};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};
use windows_capture::capture::{Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame as WgcFrame;
use windows_capture::graphics_capture_api::{GraphicsCaptureApi, InternalCaptureControl};
use windows_capture::monitor::Monitor;
use windows_capture::settings::{ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, GraphicsCaptureItemType, MinimumUpdateIntervalSettings, SecondaryWindowSettings, Settings};
use windows_capture::window::Window;

/// Attente de la première image d'une vignette.
const SNAPSHOT_WAIT: Duration = Duration::from_millis(1500);

fn backend(e: impl std::fmt::Display) -> CaptureError {
    CaptureError::Backend(e.to_string())
}

pub(super) fn screens() -> Vec<Source> {
    let Ok(list) = Monitor::enumerate() else { return Vec::new() };
    list.iter()
        .enumerate()
        .map(|(i, m)| {
            let name = m.name().ok().filter(|n| !n.trim().is_empty()).or_else(|| m.device_string().ok()).unwrap_or_else(|| (i + 1).to_string());
            Source { id: source_id(SourceKind::Screen, &m.as_raw_hmonitor().expose_provenance().to_string()), name, kind: SourceKind::Screen }
        })
        .collect()
}

pub(super) fn windows() -> Vec<WindowInfo> {
    let Ok(list) = Window::enumerate() else { return Vec::new() };
    list.iter()
        .filter_map(|w| {
            let r = w.rect().ok()?;
            Some(WindowInfo {
                handle: w.as_raw_hwnd().expose_provenance(),
                title: w.title().unwrap_or_default(),
                pid: w.process_id().unwrap_or(0),
                exe: w.process_name().ok(),
                rect: (r.left, r.top, r.right - r.left, r.bottom - r.top),
            })
        })
        .collect()
}

/// Processus d'une fenêtre (pour capter le son de son application).
pub(super) fn window_pid(raw: &str) -> Option<u32> {
    let addr: usize = raw.parse().ok()?;
    let w = Window::from_raw_hwnd(std::ptr::with_exposed_provenance_mut(addr));
    w.is_valid().then(|| w.process_id().ok()).flatten()
}

/// Ce que la capture vise, retrouvé d'après l'identifiant.
enum Target {
    Screen(Monitor),
    Window(Window),
}

impl TryFrom<Target> for GraphicsCaptureItemType {
    type Error = windows::core::Error;

    fn try_from(t: Target) -> Result<Self, Self::Error> {
        match t {
            Target::Screen(m) => m.try_into(),
            Target::Window(w) => w.try_into(),
        }
    }
}

fn target(kind: SourceKind, raw: &str) -> Result<Target, CaptureError> {
    let addr: usize = raw.parse().map_err(|_| CaptureError::NotFound)?;
    let ptr = std::ptr::with_exposed_provenance_mut(addr);
    match kind {
        SourceKind::Screen => {
            let m = Monitor::from_raw_hmonitor(ptr);
            // Une poignée d'écran périmée (écran débranché) n'est plus dans la liste.
            Monitor::enumerate().map_err(backend)?.contains(&m).then_some(Target::Screen(m)).ok_or(CaptureError::NotFound)
        }
        SourceKind::Window => {
            let w = Window::from_raw_hwnd(ptr);
            w.is_valid().then_some(Target::Window(w)).ok_or(CaptureError::NotFound)
        }
        SourceKind::Camera => Err(CaptureError::NotFound),
    }
}

enum Sink {
    Stream(Box<dyn FnMut(Frame) + Send>),
    Once(mpsc::Sender<Frame>),
}

struct Flags {
    want: Quality,
    sink: Sink,
    ended: Arc<AtomicBool>,
}

struct Handler {
    want: Quality,
    sink: Sink,
    ended: Arc<AtomicBool>,
    period: Duration,
    start: Instant,
    /// Instant où la prochaine image est due.
    due: Option<Instant>,
}

/// Faut-il garder l'image arrivée à `now` ? Rend alors la nouvelle échéance.
/// L'échéance avance d'une période exacte à chaque image gardée : sur un
/// écran plus rapide que la cadence voulue (144 Hz pour 60 images/s), la
/// moyenne tombe juste, là où « assez loin de la précédente » n'en gardait
/// qu'une sur trois (48 images/s, irrégulières).
fn keep(due: Option<Instant>, now: Instant, period: Duration) -> Option<Instant> {
    match due {
        None => Some(now + period),
        Some(d) if now + period / 8 < d => None,
        // Très en retard (écran resté immobile) : on repart de maintenant.
        Some(d) if now > d + period => Some(now + period),
        Some(d) => Some(d + period),
    }
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = String;

    fn new(ctx: Context<Flags>) -> Result<Self, String> {
        let Flags { want, sink, ended } = ctx.flags;
        let period = Duration::from_micros(1_000_000 / u64::from(want.fps.max(1)));
        Ok(Handler { want, sink, ended, period, start: Instant::now(), due: None })
    }

    fn on_frame_arrived(&mut self, frame: &mut WgcFrame, control: InternalCaptureControl) -> Result<(), String> {
        let now = Instant::now();
        // Windows livre au rythme de l'écran : on ne garde que la fréquence voulue.
        let Some(due) = keep(self.due, now, self.period) else { return Ok(()) };
        self.due = Some(due);
        let (w, h) = (frame.width(), frame.height());
        let (dw, dh) = fit(w, h, self.want);
        let mut fb = frame.buffer().map_err(|e| e.to_string())?;
        let stride = fb.row_pitch() as usize;
        let mut out = Vec::with_capacity(4 * dw as usize * dh as usize);
        if !scale_bgra(fb.as_raw_buffer(), w, h, stride, dw, dh, &mut out) {
            return Ok(());
        }
        let image = Frame { width: dw, height: dh, pts_us: now.duration_since(self.start).as_micros() as u64, data: FrameData::Bgra(out) };
        match &mut self.sink {
            Sink::Stream(on_frame) => on_frame(image),
            Sink::Once(tx) => {
                let _ = tx.send(image);
                control.stop();
            }
        }
        Ok(())
    }

    fn on_closed(&mut self) -> Result<(), String> {
        self.ended.store(true, Ordering::Relaxed);
        Ok(())
    }
}

type Control = windows_capture::capture::CaptureControl<Handler, String>;

fn start(kind: SourceKind, raw: &str, want: Quality, cursor: bool, border: DrawBorderSettings, sink: Sink, ended: Arc<AtomicBool>) -> Result<Control, CaptureError> {
    let item = target(kind, raw)?;
    let cursor = if cursor { CursorCaptureSettings::Default } else { CursorCaptureSettings::WithoutCursor };
    let settings = Settings::new(
        item,
        cursor,
        border,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Bgra8,
        Flags { want, sink, ended },
    );
    Handler::start_free_threaded(settings).map_err(backend)
}

pub(super) fn open(kind: SourceKind, raw: &str, want: Quality, opts: Options, on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<CaptureHandle, CaptureError> {
    let ended = Arc::new(AtomicBool::new(false));
    // Sans le cadre jaune que Windows dessine autour de ce qui est capturé
    // (gênant pendant tout un partage, signalé par l'utilisateur, 0.1.15),
    // là où Windows le permet (Windows 11) ; refusé : avec, comme avant.
    // Le rappel est partagé pour pouvoir servir au second essai.
    let shared = Arc::new(std::sync::Mutex::new(on_frame));
    let run = |border| {
        let s = shared.clone();
        let sink: Box<dyn FnMut(Frame) + Send> = Box::new(move |f| (s.lock().unwrap_or_else(std::sync::PoisonError::into_inner))(f));
        start(kind, raw, want, opts.cursor, border, Sink::Stream(sink), ended.clone())
    };
    let control = if borderless() { run(DrawBorderSettings::WithoutBorder).or_else(|_| run(DrawBorderSettings::Default))? } else { run(DrawBorderSettings::Default)? };
    Ok(CaptureHandle::new(
        Box::new(move || {
            let _ = control.stop();
        }),
        ended,
    ))
}

/// Windows sait capturer sans dessiner son cadre jaune.
pub(super) fn borderless() -> bool {
    GraphicsCaptureApi::is_border_settings_supported().unwrap_or(false)
}

pub(super) fn snapshot(kind: SourceKind, raw: &str, max_w: u32) -> Result<Frame, CaptureError> {
    let want = Quality { width: max_w.max(2), height: u32::MAX / 2, fps: 30 };
    let run = |border| {
        let (tx, rx) = mpsc::channel();
        start(kind, raw, want, true, border, Sink::Once(tx), Arc::default()).map(|c| (c, rx))
    };
    let (control, rx) = if borderless() {
        // Sans bordure refusé par Windows : avec, comme un partage.
        run(DrawBorderSettings::WithoutBorder).or_else(|_| run(DrawBorderSettings::Default))?
    } else {
        run(DrawBorderSettings::Default)?
    };
    let got = rx.recv_timeout(SNAPSHOT_WAIT);
    let _ = control.stop();
    got.map_err(|_| CaptureError::Backend("aucune image".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Images gardées en une seconde pour un écran à `hz` et une cadence voulue `fps`.
    fn kept(hz: u64, fps: u64) -> usize {
        let (t0, period) = (Instant::now(), Duration::from_micros(1_000_000 / fps));
        let mut due = None;
        (0..hz)
            .filter(|k| {
                let next = keep(due, t0 + Duration::from_micros(k * 1_000_000 / hz), period);
                if next.is_some() {
                    due = next;
                }
                next.is_some()
            })
            .count()
    }

    #[test]
    fn la_cadence_voulue_est_tenue_quel_que_soit_l_ecran() {
        assert_eq!(kept(60, 60), 60);
        assert_eq!(kept(60, 30), 30);
        assert!((59..=61).contains(&kept(144, 60)), "{}", kept(144, 60));
        assert!((29..=31).contains(&kept(144, 30)), "{}", kept(144, 30));
        assert!((59..=61).contains(&kept(240, 60)), "{}", kept(240, 60));
        assert_eq!(kept(30, 60), 30, "écran plus lent : toutes ses images");
    }
}
