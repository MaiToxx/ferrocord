//! Caméras par `Windows.Media.Capture` (WinRT, sans `unsafe`) : un
//! `MediaCapture` en mémoire processeur, un `MediaFrameReader` qui convertit
//! en NV12 (partage) ou BGRA (vignettes), les pixels copiés par
//! `SoftwareBitmap::CopyToBuffer` puis lus par un `DataReader`.
//!
//! `MediaCapture` ne passe pas d'un fil à l'autre : chaque caméra ouverte a
//! son fil « ferrocord-camera », qui la ferme quand on lâche la capture.

use super::pixels::{nv12_to_i420, scale_bgra, Plane};
use super::{closest_format, fit, source_id, CamFormat, CaptureError, CaptureHandle, Quality, Source, SourceKind, E_ACCESSDENIED};
use crate::video::{Frame, FrameData};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use windows::core::{Ref, HSTRING};
use windows::Devices::Enumeration::{DeviceClass, DeviceInformation};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Imaging::{BitmapBufferAccessMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Capture::Frames::{MediaFrameArrivedEventArgs, MediaFrameReader, MediaFrameReaderAcquisitionMode, MediaFrameReaderStartStatus, MediaFrameSource, MediaFrameSourceKind};
use windows::Media::Capture::{MediaCapture, MediaCaptureFailedEventHandler, MediaCaptureInitializationSettings, MediaCaptureMemoryPreference, MediaCaptureSharingMode, MediaStreamType, StreamingCaptureMode};
use windows::Media::MediaProperties::MediaEncodingSubtypes;
use windows::Storage::Streams::{Buffer, DataReader};

/// Une vignette attend que la caméra ait réglé son exposition.
const WARM_UP: Duration = Duration::from_millis(400);
const SNAPSHOT_WAIT: Duration = Duration::from_secs(4);

fn backend(e: windows::core::Error) -> CaptureError {
    if e.code().0 == E_ACCESSDENIED { CaptureError::CameraDenied } else { CaptureError::Backend(e.message()) }
}

pub(super) fn list() -> Vec<Source> {
    let Ok(all) = DeviceInformation::FindAllAsyncDeviceClass(DeviceClass::VideoCapture).and_then(|op| op.join()) else { return Vec::new() };
    all.into_iter()
        .filter(|d| d.IsEnabled().unwrap_or(true))
        .filter_map(|d| Some(Source { id: source_id(SourceKind::Camera, &d.Id().ok()?.to_string_lossy()), name: d.Name().ok()?.to_string_lossy(), kind: SourceKind::Camera }))
        .collect()
}

enum Sink {
    Stream(Box<dyn FnMut(Frame) + Send>),
    Once(mpsc::Sender<Frame>),
}

#[derive(Clone, Copy)]
enum Out {
    I420,
    Bgra { max_w: u32 },
}

/// Ce que fait le rappel de chaque image (sur un fil de Windows).
struct Frames {
    sink: Option<Sink>,
    out: Out,
    start: Option<Instant>,
    buffer: Option<(Buffer, u32)>,
    bytes: Vec<u8>,
}

impl Frames {
    fn on_bitmap(&mut self, bmp: &SoftwareBitmap) -> windows::core::Result<()> {
        let now = Instant::now();
        let start = *self.start.get_or_insert(now);
        if matches!(self.sink, Some(Sink::Once(_))) && now.duration_since(start) < WARM_UP {
            return Ok(());
        }
        let (w, h) = (bmp.PixelWidth()?.max(0) as u32, bmp.PixelHeight()?.max(0) as u32);
        let format = bmp.BitmapPixelFormat()?;
        // Disposition des plans (début, pas, lignes) ; `CopyToBuffer` la garde.
        let lock = bmp.LockBuffer(BitmapBufferAccessMode::Read)?;
        let described = (0..lock.GetPlaneCount()?)
            .map(|i| lock.GetPlaneDescription(i).map(|p| (Plane { offset: p.StartIndex.max(0) as usize, stride: p.Stride.max(0) as usize }, p.Height.max(0) as usize)))
            .collect::<windows::core::Result<Vec<_>>>()?;
        lock.Close()?;
        let need = described.iter().map(|(p, rows)| p.offset + p.stride * rows).max().unwrap_or(0);
        let need = u32::try_from(need).unwrap_or(u32::MAX);
        let planes: Vec<Plane> = described.into_iter().map(|(p, _)| p).collect();
        if self.buffer.as_ref().is_none_or(|(_, cap)| *cap < need) {
            self.buffer = Some((Buffer::Create(need)?, need));
        }
        let Some((buffer, _)) = &self.buffer else { return Ok(()) };
        bmp.CopyToBuffer(buffer)?;
        let len = buffer.Length()? as usize;
        self.bytes.resize(len, 0);
        DataReader::FromBuffer(buffer)?.ReadBytes(&mut self.bytes)?;
        let pts_us = now.duration_since(start).as_micros() as u64;
        let image = match (self.out, format, planes.as_slice()) {
            (Out::I420, BitmapPixelFormat::Nv12, [y, uv, ..]) => nv12_to_i420(&self.bytes, w, h, *y, *uv).map(|(y, u, v)| Frame { width: w, height: h, pts_us, data: FrameData::I420 { y, u, v } }),
            (Out::Bgra { max_w }, BitmapPixelFormat::Bgra8, [p, ..]) => {
                let (dw, dh) = fit(w, h, Quality { width: max_w.max(2), height: u32::MAX / 2, fps: 30 });
                let mut px = Vec::with_capacity(4 * dw as usize * dh as usize);
                scale_bgra(&self.bytes[p.offset.min(self.bytes.len())..], w, h, p.stride, dw, dh, &mut px).then_some(Frame { width: dw, height: dh, pts_us, data: FrameData::Bgra(px) })
            }
            _ => None,
        };
        let Some(image) = image else { return Ok(()) };
        match &mut self.sink {
            Some(Sink::Stream(on_frame)) => on_frame(image),
            Some(Sink::Once(tx)) => {
                let _ = tx.send(image);
                self.sink = None;
            }
            None => {}
        }
        Ok(())
    }
}

/// Caméra ouverte : à fermer sur le fil qui l'a ouverte.
struct Session {
    capture: MediaCapture,
    reader: MediaFrameReader,
    token: i64,
}

impl Session {
    fn close(self) {
        let _ = self.reader.RemoveFrameArrived(self.token);
        let _ = self.reader.StopAsync().and_then(|op| op.join());
        let _ = self.reader.Close();
        let _ = self.capture.Close();
    }
}

fn init(device: &str, exclusive: bool) -> Result<MediaCapture, CaptureError> {
    let settings = MediaCaptureInitializationSettings::new().map_err(backend)?;
    settings.SetVideoDeviceId(&HSTRING::from(device)).map_err(backend)?;
    settings.SetStreamingCaptureMode(StreamingCaptureMode::Video).map_err(backend)?;
    settings.SetMemoryPreference(MediaCaptureMemoryPreference::Cpu).map_err(backend)?;
    settings.SetSharingMode(if exclusive { MediaCaptureSharingMode::ExclusiveControl } else { MediaCaptureSharingMode::SharedReadOnly }).map_err(backend)?;
    let capture = MediaCapture::new().map_err(backend)?;
    capture.InitializeWithSettingsAsync(&settings).and_then(|op| op.join()).map_err(backend)?;
    Ok(capture)
}

/// Source couleur de la caméra, celle d'enregistrement de préférence.
fn color_source(capture: &MediaCapture) -> Result<MediaFrameSource, CaptureError> {
    let mut best: Option<(bool, MediaFrameSource)> = None;
    for pair in capture.FrameSources().and_then(|m| m.First()).map_err(backend)? {
        let Ok(source) = pair.Value() else { continue };
        let Ok(info) = source.Info() else { continue };
        if info.SourceKind().ok() != Some(MediaFrameSourceKind::Color) {
            continue;
        }
        let record = info.MediaStreamType().ok() == Some(MediaStreamType::VideoRecord);
        if best.as_ref().is_none_or(|(r, _)| record && !r) {
            best = Some((record, source));
        }
    }
    best.map(|(_, s)| s).ok_or(CaptureError::NotFound)
}

/// Règle la caméra sur le format le plus proche de `want` (sans effet si
/// elle est partagée avec une autre application).
fn set_format(source: &MediaFrameSource, want: Quality) {
    let Ok(formats) = source.SupportedFormats() else { return };
    let all: Vec<_> = formats.into_iter().collect();
    let described: Vec<CamFormat> = all
        .iter()
        .map(|f| {
            let video = f.VideoFormat().ok();
            let rate = f.FrameRate().ok();
            let fps = rate.and_then(|r| Some(r.Numerator().ok()? as f32 / r.Denominator().ok()?.max(1) as f32)).unwrap_or(0.0);
            CamFormat {
                width: video.as_ref().and_then(|v| v.Width().ok()).unwrap_or(0),
                height: video.as_ref().and_then(|v| v.Height().ok()).unwrap_or(0),
                fps,
                subtype: f.Subtype().map(|s| s.to_string_lossy()).unwrap_or_default(),
            }
        })
        .collect();
    if let Some(f) = closest_format(&described, want).and_then(|i| all.get(i)) {
        let _ = source.SetFormatAsync(f).and_then(|op| op.join());
    }
}

fn start(device: &str, want: Option<Quality>, frames: Frames, ended: Arc<AtomicBool>) -> Result<Session, CaptureError> {
    // Partage : contrôle exclusif pour choisir le format ; caméra déjà prise
    // par une autre application : en lecture partagée, à son format.
    let (capture, exclusive) = match want {
        Some(_) => match init(device, true) {
            Ok(c) => (c, true),
            Err(CaptureError::CameraDenied) => return Err(CaptureError::CameraDenied),
            Err(_) => (init(device, false)?, false),
        },
        None => (init(device, false)?, false),
    };
    let source = color_source(&capture)?;
    if let (Some(want), true) = (want, exclusive) {
        set_format(&source, want);
    }
    let subtype = match frames.out {
        Out::I420 => MediaEncodingSubtypes::Nv12(),
        Out::Bgra { .. } => MediaEncodingSubtypes::Bgra8(),
    }
    .map_err(backend)?;
    let reader = capture.CreateFrameReaderWithSubtypeAsync(&source, &subtype).and_then(|op| op.join()).map_err(backend)?;
    reader.SetAcquisitionMode(MediaFrameReaderAcquisitionMode::Realtime).map_err(backend)?;
    let frames = Arc::new(Mutex::new(frames));
    let token = reader
        .FrameArrived(&TypedEventHandler::new(move |r: Ref<MediaFrameReader>, _: Ref<MediaFrameArrivedEventArgs>| {
            let Ok(reference) = r.ok()?.TryAcquireLatestFrame() else { return Ok(()) };
            let bitmap = reference.VideoMediaFrame().and_then(|v| v.SoftwareBitmap());
            if let (Ok(bmp), Ok(mut f)) = (&bitmap, frames.try_lock()) {
                let _ = f.on_bitmap(bmp);
            }
            // Rendre l'image tout de suite : en temps réel, le lecteur n'en garde que quelques-unes.
            if let Ok(b) = bitmap {
                let _ = b.Close();
            }
            let _ = reference.Close();
            Ok(())
        }))
        .map_err(backend)?;
    let _ = capture.Failed(&MediaCaptureFailedEventHandler::new(move |_, _| {
        ended.store(true, Ordering::Relaxed);
        Ok(())
    }));
    let session = Session { capture, reader, token };
    match session.reader.StartAsync().and_then(|op| op.join()) {
        Ok(MediaFrameReaderStartStatus::Success) => Ok(session),
        Ok(status) => {
            session.close();
            Err(CaptureError::Backend(format!("caméra indisponible ({})", status.0)))
        }
        Err(e) => {
            session.close();
            Err(backend(e))
        }
    }
}

pub(super) fn open(device: &str, want: Quality, on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<CaptureHandle, CaptureError> {
    let device = device.to_string();
    let ended = Arc::new(AtomicBool::new(false));
    let (ready_tx, ready) = mpsc::channel();
    let (stop_tx, stop_rx) = mpsc::channel::<()>();
    let ended_cam = ended.clone();
    let thread = std::thread::Builder::new()
        .name("ferrocord-camera".into())
        .spawn(move || {
            let frames = Frames { sink: Some(Sink::Stream(on_frame)), out: Out::I420, start: None, buffer: None, bytes: Vec::new() };
            match start(&device, Some(want), frames, ended_cam) {
                Ok(session) => {
                    let _ = ready_tx.send(Ok(()));
                    let _ = stop_rx.recv();
                    session.close();
                }
                Err(e) => {
                    let _ = ready_tx.send(Err(e));
                }
            }
        })
        .map_err(|e| CaptureError::Backend(e.to_string()))?;
    match ready.recv() {
        Ok(Ok(())) => Ok(CaptureHandle::new(
            Box::new(move || {
                let _ = stop_tx.send(());
                let _ = thread.join();
            }),
            ended,
        )),
        Ok(Err(e)) => {
            let _ = thread.join();
            Err(e)
        }
        Err(_) => {
            let _ = thread.join();
            Err(CaptureError::Backend("camera thread stopped".into()))
        }
    }
}

pub(super) fn snapshot(device: &str, max_w: u32) -> Result<Frame, CaptureError> {
    let (tx, rx) = mpsc::channel();
    let frames = Frames { sink: Some(Sink::Once(tx)), out: Out::Bgra { max_w }, start: None, buffer: None, bytes: Vec::new() };
    let session = start(device, None, frames, Arc::default())?;
    let got = rx.recv_timeout(SNAPSHOT_WAIT);
    session.close();
    got.map_err(|_| CaptureError::Backend("aucune image".into()))
}
