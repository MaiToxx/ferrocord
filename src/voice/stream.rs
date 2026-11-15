//! Tâche d'un direct (Go Live) : une connexion vocale à part
//! (`ConnKind::Stream`, DAVE sur le groupe du direct), ouverte après
//! STREAM_CREATE et STREAM_SERVER_UPDATE, la connexion de l'appel restant
//! ouverte à côté.
//! - Mon partage : capture de la source → encodeur → connexion, et le son
//!   de la source (si demandé) en Opus sur cette même connexion.
//! - Un direct regardé : réception seule ; images de son auteur → décodeur
//!   → tuile du direct ; son de l'auteur → fil de lecture de l'appel.

use super::call::{close_media, failure, SharedDave};
use super::media::{error_text, Media, PlayMsg, Shared};
use super::video::{voice_config, Receiving, Report, SendCfg, Sending};
use super::{Options, Player, Settings, Up, STREAM_AUDIO};
use crate::i18n::table::video::f as vf;
use crate::net::voice::dave::DaveSession;
use crate::net::voice::e2ee::{FrameCrypto, Passthrough};
use crate::net::voice::{self, CloseReason, VoiceCtl, VoiceEvent as NetEvent, VoiceParams};
use crate::state::audio_prefs::{AudioPrefs, InputMode};
use crate::state::stream::StreamEvent;
use crate::state::video::{Tile, VideoSink};
use crate::state::video_link::VideoUp;
use crate::video::Quality;
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use tokio::sync::mpsc as tmpsc;

/// Ce que je fais de ce direct.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Role {
    Share { source_id: String, quality: Quality, audio: bool },
    Watch { owner: u64 },
}

/// Interface -> tâche du direct.
pub(super) enum StreamMsg {
    /// Qualité voulue de l'image de l'auteur (op 15, 0 à 100).
    Quality(u8),
    Stop,
}

/// Vers l'interface : chaque envoi la réveille.
pub(super) struct StreamOut {
    pub generation: u64,
    pub key: String,
    pub tx: mpsc::Sender<(u64, Up)>,
    pub wake: super::Wake,
}

impl StreamOut {
    fn event(&self, event: StreamEvent) {
        if self.tx.send((self.generation, Up::Stream { key: self.key.clone(), event })).is_ok() {
            (self.wake)();
        }
    }

    fn ended(&self) {
        if self.tx.send((self.generation, Up::StreamEnded(self.key.clone()))).is_ok() {
            (self.wake)();
        }
    }
}

pub(super) struct StreamStart {
    pub params: VoiceParams,
    pub role: Role,
    pub opts: Options,
    pub sink: VideoSink,
    pub report: Report,
    pub player: Player,
    pub out: StreamOut,
}

/// Le son partagé part toujours : pas de porte, pas de traitement de la voix.
fn share_settings() -> Settings {
    let prefs = AudioPrefs { mode: InputMode::PushToTalk, noise_suppression: false, echo_cancellation: false, auto_gain: false, input_device: None, output_device: None, ..AudioPrefs::default() };
    Settings { muted: false, deafened: true, prefs, extras: Default::default() }
}

/// Ouvre le son de la source partagée (hors de la boucle : l'ouverture peut prendre une seconde).
async fn open_sound(s: &StreamStart, source_id: &str, net: &tmpsc::Sender<VoiceCtl>) -> Option<Media> {
    let source = super::video::source_of(source_id)?;
    let backend = s.opts.video.share_sound(&source)?;
    let (net, report) = (net.clone(), s.report.clone());
    let job = move || {
        let settings = share_settings();
        let shared = Arc::new(Shared::new(&settings, true));
        // Personne n'écoute « je parle » pour un partage.
        let (up, _) = tmpsc::unbounded_channel();
        let (m, err) = Media::open(backend.as_ref(), &settings.prefs, shared, net, up, &HashMap::new());
        if let Some(e) = err {
            report(VideoUp::Problem(vf::share_sound_failed(crate::i18n::language(), error_text(&e))));
        }
        m
    };
    tokio::task::spawn_blocking(job).await.ok()
}

pub(super) async fn run(s: StreamStart, mut rx: tmpsc::UnboundedReceiver<StreamMsg>) {
    let key = s.out.key.clone();
    let tile = Tile::Stream(key.clone());
    let dave = DaveSession::new(s.params.user_id, s.params.dave_group_id()).ok().map(|d| Arc::new(Mutex::new(d)));
    let crypto: Box<dyn FrameCrypto> = match &dave {
        Some(d) => Box::new(SharedDave(d.clone())),
        None => Box::new(Passthrough),
    };
    let (net, mut net_rx) = voice::spawn_with(&tokio::runtime::Handle::current(), s.params.clone(), crypto, voice_config(&s.opts));
    let mut send_codec = crate::video::Codec::H264;
    let mut sending: Option<Sending> = None;
    let mut sound: Option<Media> = None;
    let mut receiving = Receiving::new(s.opts.video.clone(), s.sink.clone(), net.clone(), s.report.clone());
    loop {
        tokio::select! {
            e = net_rx.recv() => match e {
                Some(NetEvent::Ready { .. }) => {
                    s.out.event(StreamEvent::Connected { key: key.clone() });
                    if let Role::Share { source_id, quality, audio } = &s.role
                        && sending.is_none()
                    {
                        sending = Some(Sending::start(SendCfg {
                            backend: s.opts.video.clone(),
                            source: Some(source_id.clone()),
                            quality: *quality,
                            screen: true,
                            codec: send_codec,
                            tile: tile.clone(),
                            sink: s.sink.clone(),
                            net: net.clone(),
                            report: s.report.clone(),
                            fail: VideoUp::ShareFailed,
                        }));
                        if *audio {
                            sound = open_sound(&s, source_id, &net).await;
                        }
                    }
                }
                Some(NetEvent::SendCodec(c)) => {
                    send_codec = c;
                    if let Some(v) = &sending {
                        v.set_codec(c);
                    }
                }
                Some(NetEvent::KeyframeRequested) => {
                    if let Some(v) = &sending {
                        v.keyframe();
                    }
                }
                Some(NetEvent::Video { user_id, frame }) => {
                    if s.role == (Role::Watch { owner: user_id }) {
                        receiving.frame(user_id, tile.clone(), frame);
                    }
                }
                Some(NetEvent::Audio { user_id, seq, opus, .. }) => {
                    if s.role == (Role::Watch { owner: user_id })
                        && let Some(p) = s.player.lock().unwrap_or_else(std::sync::PoisonError::into_inner).as_ref()
                    {
                        let _ = p.send(PlayMsg::Packet { user_id: user_id | STREAM_AUDIO, seq, opus });
                    }
                }
                Some(NetEvent::VideoState { user_id, active: false }) | Some(NetEvent::UserLeft(user_id)) => receiving.stop(user_id),
                Some(NetEvent::VideoState { .. } | NetEvent::Speaking { .. } | NetEvent::UserJoined(_) | NetEvent::Ping(_)) => {}
                Some(NetEvent::Closed(r)) => {
                    match r {
                        CloseReason::Requested => {}
                        CloseReason::Disconnected(_) | CloseReason::SessionExpired(_) => s.out.event(StreamEvent::Closed { key: key.clone() }),
                        r => {
                            if let Some(why) = failure(r) {
                                s.out.event(StreamEvent::Failed { key: key.clone(), why: vf::stream_failed(crate::i18n::language(), why) });
                            }
                        }
                    }
                    break;
                }
                None => break,
            },
            m = rx.recv() => match m {
                None | Some(StreamMsg::Stop) => {
                    drop(sending.take());
                    let _ = net.send(VoiceCtl::Disconnect).await;
                    break;
                }
                Some(StreamMsg::Quality(q)) => {
                    if let Role::Watch { owner } = s.role {
                        let _ = net.send(VoiceCtl::SinkWants(vec![(owner, q)])).await;
                    }
                }
            },
        }
    }
    drop(sending);
    drop(receiving);
    if let Some(m) = sound {
        close_media(m);
    }
    s.out.ended();
}
