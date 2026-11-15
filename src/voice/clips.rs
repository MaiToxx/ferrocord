//! Sons courts (phase 6) : sons du tableau et messages vocaux reçus,
//! téléchargés (CDN de Discord), décodés hors des fils audio, gardés dans un
//! cache borné, puis mixés au fil de lecture : celui de l'appel s'il y en a
//! un, sinon une sortie audio ouverte seule le temps de les jouer (elle sert
//! aussi au son d'un direct regardé hors appel). Enregistrement d'un message
//! vocal par une capture seule.
//!
//! La fin d'un son est datée par une minuterie (sa durée restante) : le fil
//! audio n'a rien à remonter.

use super::media::{error_text, AudioStreams, Media, PlayMsg, Shared};
use super::{Up, Voice};
use crate::audio::clip::record::Recorder;
use crate::audio::clip::{self, Cache, Pcm};
use crate::i18n::table::voice_msg::{f as mf, t as mt};
use crate::state::voice_msg::{ClipKey, ClipPlay, ClipUp, RecordCmd, Recorded};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// Sons décodés gardés : 48 Mio au plus (un son du tableau en fait environ 0,5).
const CACHE_BYTES: usize = 48 << 20;
/// Fichier téléchargé, au plus.
const MAX_DOWNLOAD: usize = 32 << 20;

/// Des tâches de sons vers l'orchestrateur.
pub(super) enum ClipMsg {
    Ready { key: ClipKey, id: u64, pcm: Pcm, from_ms: u32, volume: f32 },
    Failed { key: ClipKey, id: u64, why: String },
    Ended { key: ClipKey, id: u64 },
    OutputOpened { generation: u64, media: Media, err: Option<String> },
    RecOpened { generation: u64, res: Result<Box<dyn AudioStreams>, String> },
    Recorded { generation: u64, rec: Option<Recorded> },
}

/// Son prêt, en attente d'une sortie.
struct Queued {
    key: ClipKey,
    id: u64,
    msg: PlayMsg,
    duration_ms: u32,
    left_ms: u32,
}

struct Recording {
    generation: u64,
    rec: Arc<Mutex<Option<Recorder>>>,
    streams: Option<Box<dyn AudioStreams>>,
    /// Arrêt demandé avant l'ouverture du micro.
    then: Option<RecordCmd>,
}

pub(super) struct Clips {
    http: Option<reqwest::Client>,
    cache: Arc<Mutex<Cache>>,
    serial: u64,
    /// Son voulu (en chargement, en attente ou joué) par clé, et son numéro.
    wanted: HashMap<ClipKey, u64>,
    queue: Vec<Queued>,
    /// Sortie seule (hors appel), et réglages lus par son fil de lecture.
    out: Option<Media>,
    out_shared: Arc<Shared>,
    opening: Option<u64>,
    generation: u64,
    recording: Option<Recording>,
    ups: Vec<ClipUp>,
}

impl Clips {
    pub(super) fn new() -> Self {
        let settings = super::Settings { muted: true, deafened: false, prefs: Default::default(), extras: Default::default() };
        Clips {
            http: None,
            cache: Arc::new(Mutex::new(Cache::new(CACHE_BYTES))),
            serial: 0,
            wanted: HashMap::new(),
            queue: Vec::new(),
            out: None,
            out_shared: Arc::new(Shared::new(&settings, false)),
            opening: None,
            generation: 0,
            recording: None,
            ups: Vec::new(),
        }
    }

    pub(super) fn settings(&self, s: &super::Settings) {
        self.out_shared.set(s);
    }

    pub(super) fn take_ups(&mut self) -> Vec<ClipUp> {
        std::mem::take(&mut self.ups)
    }
}

/// Seules les adresses de Discord sont demandées.
pub(super) fn allowed(url: &str) -> bool {
    url.starts_with("https://cdn.discordapp.com/") || url.starts_with("https://media.discordapp.net/") || (cfg!(test) && url.starts_with("http://127.0.0.1:"))
}

pub(super) async fn download(client: reqwest::Client, url: &str, max: usize) -> Result<Vec<u8>, String> {
    let lang = crate::i18n::language();
    let mut r = client.get(url).send().await.map_err(|e| e.without_url().to_string())?;
    if !r.status().is_success() {
        return Err(format!("HTTP {}", r.status().as_u16()));
    }
    if r.content_length().is_some_and(|n| n > max as u64) {
        return Err(mt::TOO_BIG.get(lang).into());
    }
    let mut out = Vec::new();
    while let Some(c) = r.chunk().await.map_err(|e| e.without_url().to_string())? {
        out.extend_from_slice(&c);
        if out.len() > max {
            return Err(mt::TOO_BIG.get(lang).into());
        }
    }
    Ok(out)
}

fn locked<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Voice {
    fn clip_sender(&self) -> impl Fn(ClipMsg) + Send + 'static {
        let (tx, wake) = (self.up_tx.clone(), self.wake.clone());
        move |m| {
            if tx.send((0, Up::Clip(m))).is_ok() {
                wake();
            }
        }
    }

    /// Client HTTP des téléchargements (sons courts, fichiers joints).
    pub(super) fn http(&mut self) -> reqwest::Client {
        self.clips.http.get_or_insert_with(|| crate::platform::http_builder().build().unwrap_or_default()).clone()
    }

    pub(super) fn clip_play(&mut self, p: ClipPlay) {
        self.clip_stop(p.key);
        self.clips.serial += 1;
        let id = self.clips.serial;
        let key = p.key;
        if !allowed(&p.url) {
            self.clips.ups.push(ClipUp::Failed { key, why: mf::play_failed(crate::i18n::language(), mt::BAD_ADDRESS.get(crate::i18n::language())) });
            return;
        }
        self.clips.wanted.insert(key, id);
        let client = self.http();
        let cache = self.clips.cache.clone();
        let send = self.clip_sender();
        self.rt.spawn(async move {
            let cached = locked(&cache).get(&p.url);
            let pcm = match cached {
                Some(pcm) => Ok(pcm),
                None => match download(client, &p.url, MAX_DOWNLOAD).await {
                    Ok(bytes) => {
                        let lang = crate::i18n::language();
                        match tokio::task::spawn_blocking(move || clip::decode(bytes)).await {
                            Ok(Ok(pcm)) => {
                                locked(&cache).insert(p.url.clone(), pcm.clone());
                                Ok(pcm)
                            }
                            _ => Err(mt::UNREADABLE.get(lang).to_string()),
                        }
                    }
                    Err(e) => Err(e),
                },
            };
            send(match pcm {
                Ok(pcm) => ClipMsg::Ready { key, id, pcm, from_ms: p.from_ms, volume: p.volume.clamp(0.0, 1.0) },
                Err(e) => ClipMsg::Failed { key, id, why: mf::play_failed(crate::i18n::language(), &e) },
            });
        });
        self.sync_output();
    }

    pub(super) fn clip_stop(&mut self, key: ClipKey) {
        let Some(id) = self.clips.wanted.remove(&key) else { return };
        self.clips.queue.retain(|q| q.id != id);
        if let Some(p) = super::lock_player(&self.player).as_ref() {
            let _ = p.send(PlayMsg::StopClip(id));
        }
        self.sync_output();
    }

    /// Envoie au fil de lecture les sons prêts, s'il y en a un.
    fn clip_flush(&mut self) {
        if self.clips.queue.is_empty() {
            return;
        }
        let Some(player) = super::lock_player(&self.player).clone() else { return };
        for q in std::mem::take(&mut self.clips.queue) {
            let _ = player.send(q.msg);
            self.clips.ups.push(ClipUp::Started { key: q.key, duration_ms: q.duration_ms });
            let (send, key, id) = (self.clip_sender(), q.key, q.id);
            let left = Duration::from_millis(u64::from(q.left_ms));
            self.rt.spawn(async move {
                tokio::time::sleep(left).await;
                send(ClipMsg::Ended { key, id });
            });
        }
    }

    /// Ouvre la sortie seule quand il faut jouer quelque chose hors appel
    /// (son court, direct regardé), la ferme sinon.
    pub(super) fn sync_output(&mut self) {
        let watching = self.streams.values().any(|r| matches!(r.key.role, super::stream::Role::Watch { .. }));
        let need = self.running.is_none() && (watching || !self.clips.wanted.is_empty() || self.files.wants_output());
        if need {
            if self.clips.out.is_none() && self.clips.opening.is_none() {
                self.clips.generation += 1;
                let generation = self.clips.generation;
                self.clips.opening = Some(generation);
                let (backend, shared, send) = (self.opts.backend.clone(), self.clips.out_shared.clone(), self.clip_sender());
                let prefs = self.settings.as_ref().map(|s| s.prefs.clone()).unwrap_or_default();
                self.rt.spawn_blocking(move || {
                    let (media, err) = Media::open_output(backend.as_ref(), &prefs, shared);
                    send(ClipMsg::OutputOpened { generation, media, err: err.as_ref().map(error_text) });
                });
            }
            self.clip_flush();
            return;
        }
        self.clips.opening = None;
        if let Some(m) = self.clips.out.take() {
            // Un appel a pu prendre la place entre-temps : sa file reste.
            if self.running.is_none() {
                *super::lock_player(&self.player) = None;
            }
            super::call::close_media(m);
        }
        self.clip_flush();
    }

    pub(super) fn on_clip(&mut self, m: ClipMsg) {
        match m {
            ClipMsg::Ready { key, id, pcm, from_ms, volume } => {
                if self.clips.wanted.get(&key) != Some(&id) {
                    return;
                }
                let pos = (from_ms as usize * 48).min(pcm.frames());
                let duration_ms = pcm.duration_ms();
                let left_ms = duration_ms.saturating_sub(from_ms);
                let deafen = matches!(key, ClipKey::Sound(_));
                self.clips.queue.push(Queued { key, id, msg: PlayMsg::Clip { id, pcm, pos, volume, deafen }, duration_ms, left_ms });
                self.sync_output();
            }
            ClipMsg::Failed { key, id, why } => {
                if self.clips.wanted.get(&key) == Some(&id) {
                    self.clips.wanted.remove(&key);
                    self.clips.ups.push(ClipUp::Failed { key, why });
                    self.sync_output();
                }
            }
            ClipMsg::Ended { key, id } => {
                if self.clips.wanted.get(&key) == Some(&id) {
                    self.clips.wanted.remove(&key);
                    self.clips.ups.push(ClipUp::Ended(key));
                    self.sync_output();
                }
            }
            ClipMsg::OutputOpened { generation, media, err } => {
                if self.clips.opening != Some(generation) || self.running.is_some() {
                    super::call::close_media(media);
                    return;
                }
                self.clips.opening = None;
                if let Some(why) = err {
                    super::call::close_media(media);
                    // Sans sortie, les sons en attente échouent ; le direct reste muet.
                    for q in std::mem::take(&mut self.clips.queue) {
                        self.clips.wanted.remove(&q.key);
                        self.clips.ups.push(ClipUp::Failed { key: q.key, why: mf::play_failed(crate::i18n::language(), &why) });
                    }
                    return;
                }
                *super::lock_player(&self.player) = Some(media.player());
                self.clips.out = Some(media);
                self.sync_output();
            }
            ClipMsg::RecOpened { generation, res } => {
                let Some(r) = self.clips.recording.as_mut().filter(|r| r.generation == generation) else {
                    if let Ok(s) = res {
                        close_streams(s);
                    }
                    return;
                };
                match res {
                    Ok(s) => {
                        r.streams = Some(s);
                        if let Some(c) = r.then.take() {
                            self.record(c);
                        }
                    }
                    Err(e) => {
                        self.clips.recording = None;
                        self.clips.ups.push(ClipUp::RecordFailed(mf::record_failed(crate::i18n::language(), &e)));
                    }
                }
            }
            ClipMsg::Recorded { generation, rec } => {
                if self.clips.generation_rec_done(generation) {
                    self.clips.ups.push(match rec {
                        Some(r) => ClipUp::Recorded(r),
                        None => ClipUp::RecordFailed(mt::NOTHING_RECORDED.get(crate::i18n::language()).to_string()),
                    });
                }
            }
        }
    }

    pub(super) fn record(&mut self, c: RecordCmd) {
        match c {
            RecordCmd::Start => {
                if self.clips.recording.is_some() {
                    return;
                }
                let Some(recorder) = Recorder::new() else {
                    self.clips.ups.push(ClipUp::RecordFailed(mf::record_failed(crate::i18n::language(), "Opus")));
                    return;
                };
                self.clips.generation += 1;
                let generation = self.clips.generation;
                let rec = Arc::new(Mutex::new(Some(recorder)));
                self.clips.recording = Some(Recording { generation, rec: rec.clone(), streams: None, then: None });
                let prefs = self.settings.as_ref().map(|s| s.prefs.clone()).unwrap_or_default();
                let (backend, send) = (self.opts.backend.clone(), self.clip_sender());
                let gain = prefs.input_gain();
                let mut buf = vec![0.0f32; 4096];
                let capture: crate::audio::io::CaptureFn = Box::new(move |mono: &[f32]| {
                    // Jamais d'attente sur le fil temps réel.
                    let Ok(mut g) = rec.try_lock() else { return };
                    let Some(r) = g.as_mut() else { return };
                    for chunk in mono.chunks(buf.len()) {
                        let b = &mut buf[..chunk.len()];
                        for (d, x) in b.iter_mut().zip(chunk) {
                            *d = x * gain;
                        }
                        r.push(b);
                    }
                });
                self.rt.spawn_blocking(move || {
                    let res = backend.open_input(prefs.input_device.as_deref(), capture).map_err(|e| error_text(&e));
                    send(ClipMsg::RecOpened { generation, res });
                });
            }
            RecordCmd::Finish | RecordCmd::Cancel => {
                let Some(r) = self.clips.recording.as_mut() else { return };
                let Some(streams) = r.streams.take() else {
                    // Micro pas encore ouvert : l'arrêt attend l'ouverture.
                    r.then = Some(c);
                    return;
                };
                let (generation, rec) = (r.generation, r.rec.clone());
                if c == RecordCmd::Cancel {
                    self.clips.recording = None;
                    close_streams(streams);
                    return;
                }
                let send = self.clip_sender();
                let serial = generation as u32 ^ 0x4652_4F43;
                self.rt.spawn_blocking(move || {
                    // Capture arrêtée d'abord : plus rien n'arrive au codeur.
                    drop(streams);
                    let r = locked(&rec).take();
                    let rec = r.and_then(|r| r.finish(serial)).map(|r| Recorded { ogg: r.ogg.into(), duration_ms: r.duration_ms, waveform: r.waveform });
                    send(ClipMsg::Recorded { generation, rec });
                });
            }
        }
    }

    /// Ce qui est revenu des sons depuis le dernier relevé.
    pub(super) fn poll_clip_ups(&mut self) -> Vec<ClipUp> {
        self.drain();
        self.file_tick();
        self.clips.take_ups()
    }
}

impl Clips {
    /// Fin d'un enregistrement : `true` s'il est encore celui en cours.
    fn generation_rec_done(&mut self, generation: u64) -> bool {
        if self.recording.as_ref().is_some_and(|r| r.generation == generation) {
            self.recording = None;
            return true;
        }
        false
    }
}

/// Lâche une capture hors de la boucle (la fermeture attend le fil audio).
fn close_streams(s: Box<dyn AudioStreams>) {
    let _ = std::thread::Builder::new().name("ferrocord-voix-arret".into()).spawn(move || drop(s));
}
