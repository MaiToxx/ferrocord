//! Côté Windows de [`super`] : WASAPI par `cpal` (aucun `unsafe` ici).
//!
//! Un fil « ferrocord-audio » possède les deux flux ; `cpal` crée en plus
//! un fil temps réel par flux, où tournent les rappels. Quand un flux
//! signale une perte (périphérique débranché, défaut de Windows changé),
//! son rappel d'erreur prévient le fil, qui le rouvre.

use super::{downmix, resample::Resampler, spread, AudioError, CaptureFn, DeviceInfo, PlaybackFn};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{ErrorKind, FromSample, SampleFormat, SizedSample};
use std::str::FromStr;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Duration;

const RATE: u32 = 48_000;
/// Trames du périphérique traitées d'un coup par la capture.
const CHUNK: usize = 1024;
/// Trames à 48 kHz demandées d'un coup à la lecture (10 ms).
const PLAY_FRAMES: usize = 480;
/// Sans périphérique, on réessaie à ce rythme (seulement pendant un appel).
const RETRY: Duration = Duration::from_secs(2);
/// Canaux de périphérique pris en compte en lecture (au-delà : silence).
const MAX_DEVICE_CHANNELS: usize = 8;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

fn backend(e: impl std::fmt::Display) -> AudioError {
    AudioError::Backend(e.to_string())
}

fn name_of(d: &cpal::Device) -> Option<String> {
    d.description().ok().map(|x| x.name().to_string())
}

fn default_device(host: &cpal::Host, input: bool) -> Option<cpal::Device> {
    if input { host.default_input_device() } else { host.default_output_device() }
}

pub(super) fn list(input: bool) -> Vec<DeviceInfo> {
    let host = cpal::default_host();
    let default_id = default_device(&host, input).and_then(|d| d.id().ok());
    let devices: Vec<cpal::Device> = if input { host.input_devices().map(Iterator::collect) } else { host.output_devices().map(Iterator::collect) }.unwrap_or_default();
    devices
        .into_iter()
        .filter_map(|d| {
            let id = d.id().ok()?;
            let name = name_of(&d).unwrap_or_else(|| id.to_string());
            Some(DeviceInfo { is_default: Some(&id) == default_id.as_ref(), id: id.to_string(), name })
        })
        .collect()
}

/// Rappel de capture et ses tampons : du périphérique vers le mono à 48 kHz.
struct Capture {
    cb: CaptureFn,
    channels: usize,
    rs: Resampler,
    mono: Vec<f32>,
    out: Vec<f32>,
}

impl Capture {
    fn new(cb: CaptureFn) -> Self {
        Capture { cb, channels: 1, rs: Resampler::new(RATE, RATE, 1), mono: vec![0.0; CHUNK], out: Vec::new() }
    }

    /// Hors du fil temps réel : le flux n'est pas encore lancé.
    fn configure(&mut self, channels: usize, rate: u32) {
        self.channels = channels.max(1);
        self.rs = Resampler::new(rate, RATE, 1);
        self.out = vec![0.0; self.rs.max_output(CHUNK)];
    }

    fn feed<T: Copy>(&mut self, data: &[T])
    where
        f32: FromSample<T>,
    {
        let ch = self.channels;
        let mut n = 0;
        let mut frame = [0.0f32; MAX_DEVICE_CHANNELS];
        for f in data.chunks_exact(ch) {
            let k = ch.min(MAX_DEVICE_CHANNELS);
            for (d, s) in frame[..k].iter_mut().zip(f) {
                *d = f32::from_sample_(*s);
            }
            self.mono[n] = downmix(&frame[..k]);
            n += 1;
            if n == CHUNK {
                self.flush(n);
                n = 0;
            }
        }
        if n > 0 {
            self.flush(n);
        }
    }

    fn flush(&mut self, n: usize) {
        let Capture { cb, rs, mono, out, .. } = self;
        let (_, written) = rs.process(&mono[..n], out);
        if written > 0 {
            cb(&out[..written]);
        }
    }
}

/// Rappel de lecture et ses tampons : du stéréo à 48 kHz vers le périphérique.
struct Playback {
    cb: PlaybackFn,
    channels: usize,
    rs: Resampler,
    src: Vec<f32>,
    src_pos: usize,
    fifo: Vec<f32>,
    fifo_pos: usize,
    fifo_len: usize,
}

impl Playback {
    fn new(cb: PlaybackFn) -> Self {
        Playback { cb, channels: 2, rs: Resampler::new(RATE, RATE, 2), src: vec![0.0; 2 * PLAY_FRAMES], src_pos: PLAY_FRAMES, fifo: Vec::new(), fifo_pos: 0, fifo_len: 0 }
    }

    fn configure(&mut self, channels: usize, rate: u32) {
        self.channels = channels.max(1);
        self.rs = Resampler::new(RATE, rate, 2);
        self.fifo = vec![0.0; 2 * self.rs.max_output(PLAY_FRAMES)];
        (self.src_pos, self.fifo_pos, self.fifo_len) = (PLAY_FRAMES, 0, 0);
    }

    fn fill<T: FromSample<f32>>(&mut self, data: &mut [T]) {
        let ch = self.channels;
        let k = ch.min(MAX_DEVICE_CHANNELS);
        let mut frame = [0.0f32; MAX_DEVICE_CHANNELS];
        for out in data.chunks_exact_mut(ch) {
            if self.fifo_pos == self.fifo_len {
                self.refill();
            }
            let i = 2 * self.fifo_pos;
            let lr = [self.fifo.get(i).copied().unwrap_or(0.0), self.fifo.get(i + 1).copied().unwrap_or(0.0)];
            self.fifo_pos = (self.fifo_pos + 1).min(self.fifo_len);
            spread(lr, &mut frame[..k]);
            for (j, d) in out.iter_mut().enumerate() {
                *d = T::from_sample_(if j < k { frame[j] } else { 0.0 });
            }
        }
    }

    fn refill(&mut self) {
        (self.fifo_pos, self.fifo_len) = (0, 0);
        for _ in 0..4 {
            if self.src_pos == PLAY_FRAMES {
                (self.cb)(&mut self.src);
                self.src_pos = 0;
            }
            let (read, written) = self.rs.process(&self.src[2 * self.src_pos..], &mut self.fifo);
            self.src_pos += read;
            if written > 0 {
                self.fifo_len = written;
                return;
            }
        }
    }
}

enum Ctl {
    /// Le flux de capture (`true`) ou de lecture a cessé : à rouvrir.
    Lost(bool),
    Stop,
}

/// Un sens (capture ou lecture) : ce qui est voulu, ce qui est ouvert.
struct Side {
    input: bool,
    wanted: Option<String>,
    stream: Option<cpal::Stream>,
    /// Nom du périphérique ouvert (pour dire lequel a été perdu).
    name: Option<String>,
}

struct Shared {
    capture: Mutex<Capture>,
    playback: Mutex<Playback>,
    error: Mutex<Option<AudioError>>,
}

impl Side {
    /// Ouvre le périphérique voulu, ou celui par défaut s'il manque.
    fn open(&mut self, host: &cpal::Host, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<(), AudioError> {
        self.stream = None;
        let chosen = self.wanted.as_deref().and_then(|w| cpal::DeviceId::from_str(w).ok()).and_then(|id| host.device_by_id(&id));
        let lost = self.wanted.is_some() && chosen.is_none();
        let device = chosen.or_else(|| default_device(host, self.input)).ok_or(AudioError::NoDevice { input: self.input })?;
        let stream = if self.input { build_input(&device, shared, ctl)? } else { build_output(&device, shared, ctl)? };
        stream.play().map_err(backend)?;
        if lost {
            let name = self.name.clone().or_else(|| self.wanted.clone()).unwrap_or_default();
            *locked(&shared.error) = Some(AudioError::DeviceLost { input: self.input, name });
        }
        self.name = if lost { self.name.take() } else { name_of(&device) };
        self.stream = Some(stream);
        Ok(())
    }
}

fn on_error(input: bool, ctl: mpsc::Sender<Ctl>) -> impl FnMut(cpal::Error) + Send + 'static {
    move |e: cpal::Error| {
        if matches!(e.kind(), ErrorKind::DeviceNotAvailable | ErrorKind::StreamInvalidated | ErrorKind::DeviceChanged | ErrorKind::BackendError) {
            let _ = ctl.send(Ctl::Lost(input));
        }
    }
}

fn build_input(device: &cpal::Device, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, AudioError> {
    let supported = device.default_input_config().map_err(backend)?;
    let config = supported.config();
    locked(&shared.capture).configure(usize::from(config.channels), config.sample_rate);
    fn typed<T: SizedSample>(device: &cpal::Device, config: cpal::StreamConfig, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, AudioError>
    where
        f32: FromSample<T>,
    {
        let s = shared.clone();
        device
            .build_input_stream::<T, _, _>(
                config,
                move |data: &[T], _| {
                    // Jamais d'attente sur le fil temps réel : le verrou n'est
                    // pris ailleurs que pendant une réouverture.
                    if let Ok(mut c) = s.capture.try_lock() {
                        c.feed(data);
                    }
                },
                on_error(true, ctl.clone()),
                None,
            )
            .map_err(backend)
    }
    match supported.sample_format() {
        SampleFormat::F32 => typed::<f32>(device, config, shared, ctl),
        SampleFormat::I16 => typed::<i16>(device, config, shared, ctl),
        SampleFormat::I32 => typed::<i32>(device, config, shared, ctl),
        SampleFormat::U16 => typed::<u16>(device, config, shared, ctl),
        SampleFormat::U8 => typed::<u8>(device, config, shared, ctl),
        other => Err(AudioError::Unsupported(other.to_string())),
    }
}

fn build_output(device: &cpal::Device, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, AudioError> {
    let supported = device.default_output_config().map_err(backend)?;
    let config = supported.config();
    locked(&shared.playback).configure(usize::from(config.channels), config.sample_rate);
    fn typed<T: SizedSample + FromSample<f32>>(device: &cpal::Device, config: cpal::StreamConfig, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) -> Result<cpal::Stream, AudioError> {
        let s = shared.clone();
        device
            .build_output_stream::<T, _, _>(
                config,
                move |data: &mut [T], _| match s.playback.try_lock() {
                    Ok(mut p) => p.fill(data),
                    Err(_) => data.iter_mut().for_each(|d| *d = T::EQUILIBRIUM),
                },
                on_error(false, ctl.clone()),
                None,
            )
            .map_err(backend)
    }
    match supported.sample_format() {
        SampleFormat::F32 => typed::<f32>(device, config, shared, ctl),
        SampleFormat::I16 => typed::<i16>(device, config, shared, ctl),
        SampleFormat::I32 => typed::<i32>(device, config, shared, ctl),
        SampleFormat::U16 => typed::<u16>(device, config, shared, ctl),
        SampleFormat::U8 => typed::<u8>(device, config, shared, ctl),
        other => Err(AudioError::Unsupported(other.to_string())),
    }
}

/// Le fil « ferrocord-audio » et ce qu'il partage avec [`super::Streams`].
pub(super) struct Running {
    ctl: mpsc::Sender<Ctl>,
    thread: Option<JoinHandle<()>>,
    shared: Arc<Shared>,
}

impl Running {
    /// `open` : (capture, lecture) à ouvrir ; un sens fermé n'ouvre aucun périphérique.
    pub(super) fn start(input: Option<String>, output: Option<String>, open: (bool, bool), on_capture: CaptureFn, on_playback: PlaybackFn) -> Result<Self, AudioError> {
        let shared = Arc::new(Shared { capture: Mutex::new(Capture::new(on_capture)), playback: Mutex::new(Playback::new(on_playback)), error: Mutex::new(None) });
        let (ctl, rx) = mpsc::channel();
        let (ready_tx, ready) = mpsc::channel();
        let (s, tx) = (shared.clone(), ctl.clone());
        let thread = std::thread::Builder::new()
            .name("ferrocord-audio".into())
            .spawn(move || {
                let host = cpal::default_host();
                let mut sides: Vec<Side> = [Side { input: true, wanted: input, stream: None, name: None }, Side { input: false, wanted: output, stream: None, name: None }]
                    .into_iter()
                    .filter(|s| if s.input { open.0 } else { open.1 })
                    .collect();
                for side in &mut sides {
                    if let Err(e) = side.open(&host, &s, &tx) {
                        let _ = ready_tx.send(Err(e));
                        return;
                    }
                }
                let _ = ready_tx.send(Ok(()));
                drop(ready_tx);
                run(&host, &mut sides, &s, &tx, &rx);
            })
            .map_err(backend)?;
        match ready.recv() {
            Ok(Ok(())) => Ok(Running { ctl, thread: Some(thread), shared }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => {
                let _ = thread.join();
                Err(AudioError::Backend("audio thread stopped".into()))
            }
        }
    }

    pub(super) fn take_error(&self) -> Option<AudioError> {
        locked(&self.shared.error).take()
    }
}

/// Boucle du fil : attend les pertes et les rouvre. Pendant qu'un sens est
/// sans périphérique, on réessaie toutes les [`RETRY`] ; sinon aucun réveil.
fn run(host: &cpal::Host, sides: &mut [Side], shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>, rx: &mpsc::Receiver<Ctl>) {
    loop {
        let missing = sides.iter().any(|s| s.stream.is_none());
        let msg = if missing {
            match rx.recv_timeout(RETRY) {
                Ok(m) => Some(m),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return,
            }
        };
        match msg {
            Some(Ctl::Stop) => return,
            Some(Ctl::Lost(input)) => {
                for side in sides.iter_mut().filter(|s| s.input == input) {
                    reopen(host, side, shared, ctl);
                }
                // Une perte en signale souvent plusieurs : une seule réouverture suffit.
                while let Ok(m) = rx.try_recv() {
                    if matches!(m, Ctl::Stop) {
                        return;
                    }
                }
            }
            None => {
                for side in sides.iter_mut().filter(|s| s.stream.is_none()) {
                    reopen(host, side, shared, ctl);
                }
            }
        }
    }
}

fn reopen(host: &cpal::Host, side: &mut Side, shared: &Arc<Shared>, ctl: &mpsc::Sender<Ctl>) {
    if let Err(e) = side.open(host, shared, ctl) {
        let mut slot = locked(&shared.error);
        if slot.as_ref() != Some(&e) {
            *slot = Some(e);
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.ctl.send(Ctl::Stop);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
