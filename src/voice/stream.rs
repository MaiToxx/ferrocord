//! Tâche d'un direct (Go Live) : une connexion vocale à part
//! (`ConnKind::Stream`, DAVE sur le groupe du direct), ouverte après
//! STREAM_CREATE et STREAM_SERVER_UPDATE, la connexion de l'appel restant
//! ouverte à côté.
//! - Mon partage : capture de la source → encodeur → connexion, et le son
//!   de la source (si demandé) en Opus sur cette même connexion.
//! - Un direct regardé : réception seule ; images de son auteur → décodeur
//!   → tuile du direct ; son de l'auteur → fil de lecture de l'appel.

use super::call::{close_media, failure, joined, SharedDave, Task};
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

/// Écart entre deux images clés forcées de mon partage.
const KEYFRAME_EVERY: std::time::Duration = std::time::Duration::from_secs(10);

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
    /// Mon partage change de source, de qualité ou de son : la connexion
    /// reste, seules la capture et l'encodage repartent (F23).
    Source { source_id: String, quality: Quality, audio: bool },
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

/// Ouvre le son de la source partagée sur un fil à part (l'ouverture peut
/// prendre une seconde) ; la boucle du direct n'attend jamais dessus.
fn start_sound(s: &StreamStart, source_id: &str, net: &tmpsc::Sender<VoiceCtl>) -> Option<Task<Media>> {
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
    Some(tokio::task::spawn_blocking(job))
}

/// Lance la capture et l'encodage de mon partage.
fn start_sending(s: &StreamStart, source_id: &str, quality: Quality, codec: crate::video::Codec, tile: &Tile, net: &tmpsc::Sender<VoiceCtl>) -> Sending {
    Sending::start(SendCfg {
        backend: s.opts.video.clone(),
        source: Some(source_id.to_string()),
        quality,
        screen: true,
        codec,
        tile: tile.clone(),
        sink: s.sink.clone(),
        net: net.clone(),
        report: s.report.clone(),
        fail: VideoUp::ShareFailed,
    })
}

pub(super) async fn run(s: StreamStart, mut rx: tmpsc::UnboundedReceiver<StreamMsg>) {
    let key = s.out.key.clone();
    let tile = Tile::Stream(key.clone());
    // Ce que je partage, si c'est mon direct ; change sans couper la connexion.
    let mut share = match &s.role {
        Role::Share { source_id, quality, audio } => Some((source_id.clone(), *quality, *audio)),
        Role::Watch { .. } => None,
    };
    let mut ready = false;
    let dave = DaveSession::new(s.params.user_id, s.params.dave_group_id()).ok().map(|d| Arc::new(Mutex::new(d)));
    let crypto: Box<dyn FrameCrypto> = match &dave {
        Some(d) => Box::new(SharedDave(d.clone())),
        None => Box::new(Passthrough),
    };
    let (net, mut net_rx) = voice::spawn_with(&tokio::runtime::Handle::current(), s.params.clone(), crypto, voice_config(&s.opts));
    let mut send_codec = crate::video::Codec::H264;
    let mut sending: Option<Sending> = None;
    let mut sound: Option<Media> = None;
    // Son en cours d'ouverture, et la source pour laquelle il a été demandé.
    let mut sound_opening: Option<Task<Media>> = None;
    let mut sound_src: Option<String> = None;
    // Qualité demandée pas encore partie.
    let mut wants: Option<Vec<(u64, u8)>> = None;
    let mut receiving = Receiving::new(s.opts.video.clone(), s.sink.clone(), net.clone(), s.report.clone());
    // Image clé régulière de mon partage : une personne dont la demande
    // d'image clé s'est perdue a une image au bout de ce délai au plus, au
    // lieu d'un chargement sans fin. Rare : une image clé pèse lourd en
    // taille source, et l'arrivée d'une personne en demande déjà une.
    let mut key_tick = tokio::time::interval(KEYFRAME_EVERY);
    key_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            m = joined(&mut sound_opening) => {
                // Le partage a pu changer pendant l'ouverture : son gardé
                // seulement s'il est encore voulu pour cette source.
                let want = share.as_ref().filter(|(_, _, audio)| *audio).map(|(src, _, _)| src.clone());
                if m.is_some() && want.is_some() && want == sound_src {
                    if let Some(old) = std::mem::replace(&mut sound, m) {
                        close_media(old);
                    }
                } else {
                    if let Some(m) = m {
                        close_media(m);
                    }
                    if let Some(src) = want.filter(|w| Some(w) != sound_src.as_ref()) {
                        sound_opening = start_sound(&s, &src, &net);
                        sound_src = Some(src);
                    }
                }
            }
            // Jamais d'attente sur la file de la connexion sans continuer à la lire.
            p = net.reserve(), if wants.is_some() => {
                if let (Ok(p), Some(w)) = (p, wants.take()) {
                    p.send(VoiceCtl::SinkWants(w));
                }
            }
            _ = key_tick.tick(), if sending.is_some() => {
                if let Some(v) = &sending {
                    v.keyframe();
                }
            }
            e = net_rx.recv() => match e {
                Some(NetEvent::Ready { .. }) => {
                    s.out.event(StreamEvent::Connected { key: key.clone() });
                    ready = true;
                    if let Some((source_id, quality, audio)) = &share
                        && sending.is_none()
                    {
                        sending = Some(start_sending(&s, source_id, *quality, send_codec, &tile, &net));
                        if *audio && sound_opening.is_none() {
                            sound_opening = start_sound(&s, source_id, &net);
                            sound_src = Some(source_id.clone());
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
                Some(NetEvent::VideoLoss(n)) => {
                    if let Some(v) = &sending {
                        v.lost(n);
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
                // Quelqu'un arrive sur mon direct : une image clé tout de suite.
                Some(NetEvent::UserJoined(_)) => {
                    if let Some(v) = &sending {
                        v.keyframe();
                    }
                }
                Some(NetEvent::VideoState { .. } | NetEvent::Speaking { .. } | NetEvent::Ping(_)) => {}
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
                    let _ = net.try_send(VoiceCtl::Disconnect);
                    break;
                }
                Some(StreamMsg::Source { source_id, quality, audio }) => {
                    let Some(old) = share.replace((source_id.clone(), quality, audio)) else { continue };
                    // Pas encore connecté : la nouvelle source partira à la connexion.
                    if !ready {
                        continue;
                    }
                    if old.0 != source_id || old.1 != quality {
                        // L'ancienne capture s'arrête avant que la nouvelle s'ouvre ;
                        // la première image de la nouvelle est une image clé.
                        drop(sending.take());
                        sending = Some(start_sending(&s, &source_id, quality, send_codec, &tile, &net));
                    }
                    if old.0 != source_id || old.2 != audio {
                        if let Some(m) = sound.take() {
                            close_media(m);
                        }
                        // Une ouverture en cours se termine d'abord, puis s'ajuste (voir plus haut).
                        if audio && sound_opening.is_none() {
                            sound_opening = start_sound(&s, &source_id, &net);
                            sound_src = Some(source_id.clone());
                        }
                    }
                }
                Some(StreamMsg::Quality(q)) => {
                    if let Role::Watch { owner } = s.role {
                        wants = Some(vec![(owner, q)]);
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
