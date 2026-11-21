//! Tâche d'un appel : connexion vocale (DAVE comme chiffrement de bout en
//! bout), périphériques, et ce qui remonte vers l'état vocal.

use super::media::{error_text, AudioUp, Media, PlayMsg, Shared};
use super::video::{voice_config, Receiving, Report, SendCfg, Sending};
use super::{Msg, Options, Player, Settings, Up, STREAM_AUDIO};
use crate::state::video::{Tile, VideoSink};
use crate::state::video_link::VideoUp;
use crate::video::{Codec, Quality};
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::net::voice::dave::DaveSession;
use crate::net::voice::e2ee::{FrameCrypto, Outgoing, Passthrough};
use crate::net::voice::{self, CloseReason, VoiceCtl, VoiceEvent as NetEvent, VoiceParams};
use crate::state::voice::{VoiceCodes, VoiceEvent};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex, MutexGuard};
use tokio::sync::mpsc as tmpsc;

/// Vers l'interface : chaque envoi la réveille.
pub(super) struct Out {
    pub generation: u64,
    pub tx: mpsc::Sender<(u64, Up)>,
    pub wake: super::Wake,
}

impl Out {
    fn up(&self, u: Up) {
        if self.tx.send((self.generation, u)).is_ok() {
            (self.wake)();
        }
    }

    fn event(&self, e: VoiceEvent) {
        self.up(Up::Event(e));
    }
}

pub(super) struct Start {
    pub params: VoiceParams,
    pub settings: Settings,
    pub users: HashMap<u64, (f32, bool)>,
    pub ptt: bool,
    pub opts: Options,
    pub out: Out,
    /// Ma caméra : `Some(None)` = la caméra par défaut.
    pub camera: Option<Option<String>>,
    pub video: VideoCx,
}

/// Ce qu'il faut à la vidéo de l'appel.
pub(super) struct VideoCx {
    pub sink: VideoSink,
    pub report: Report,
    pub player: Player,
    /// Vrai tant que la sortie audio de cet appel tourne.
    pub in_call: Arc<std::sync::atomic::AtomicBool>,
}

impl VideoCx {
    fn camera(&self, opts: &Options, source: Option<String>, net: &tmpsc::Sender<VoiceCtl>, me: u64, codec: Codec) -> Sending {
        Sending::start(SendCfg {
            backend: opts.video.clone(),
            source,
            quality: Quality::HD_30,
            screen: false,
            codec,
            tile: Tile::Camera(me),
            sink: self.sink.clone(),
            net: net.clone(),
            report: self.report.clone(),
            fail: VideoUp::CameraFailed,
        })
    }
}

/// Coupe ma caméra : plus rien n'est capturé ni annoncé.
fn camera_off(cam: &mut Option<(Option<String>, Sending)>, net: &tmpsc::Sender<VoiceCtl>) {
    if cam.take().is_some() {
        let _ = net.try_send(VoiceCtl::VideoState { active: false, width: 0, height: 0, fps: 0 });
    }
}

/// La session DAVE, partagée entre la connexion (qui chiffre et déchiffre)
/// et la demande des codes de vérification. Le verrou n'est tenu que le
/// temps d'une opération courte : les codes se calculent sur une copie.
pub(super) struct SharedDave(pub(super) Arc<Mutex<DaveSession>>);

fn locked(m: &Mutex<DaveSession>) -> MutexGuard<'_, DaveSession> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

impl FrameCrypto for SharedDave {
    fn max_protocol_version(&self) -> u16 {
        locked(&self.0).max_protocol_version()
    }
    fn on_op(&mut self, op: u8, json: Option<&serde_json::Value>, binary: Option<&[u8]>, out: &mut Vec<Outgoing>) {
        locked(&self.0).on_op(op, json, binary, out);
    }
    fn on_users(&mut self, connected: &[u64]) {
        locked(&self.0).on_users(connected);
    }
    fn encrypt(&mut self, opus: &[u8]) -> Option<Vec<u8>> {
        locked(&self.0).encrypt(opus)
    }
    fn decrypt(&mut self, user_id: u64, frame: &[u8]) -> Option<Vec<u8>> {
        locked(&self.0).decrypt(user_id, frame)
    }
    fn encrypt_video(&mut self, codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        locked(&self.0).encrypt_video(codec, frame)
    }
    fn decrypt_video(&mut self, user_id: u64, codec: crate::video::Codec, frame: &[u8]) -> Option<Vec<u8>> {
        locked(&self.0).decrypt_video(user_id, codec, frame)
    }
    fn verification_code(&self, user_id: u64) -> Option<String> {
        locked(&self.0).verification_code(user_id)
    }
}

/// Texte d'une fin de connexion à annoncer comme un échec.
pub(super) fn failure(r: CloseReason) -> Option<String> {
    let lang = crate::i18n::language();
    Some(match r {
        CloseReason::Requested | CloseReason::Disconnected(_) | CloseReason::SessionExpired(_) => return None,
        CloseReason::Refused(code) => vf::refused(lang, code),
        CloseReason::Unsupported => vt::UNSUPPORTED.get(lang).to_string(),
        CloseReason::BadEndpoint => vt::BAD_ENDPOINT.get(lang).to_string(),
        CloseReason::Unreachable => vt::UNREACHABLE.get(lang).to_string(),
    })
}

/// Ouvre les périphériques hors de la boucle (l'ouverture peut prendre une seconde).
async fn open_media(opts: &Options, prefs: &crate::state::audio_prefs::AudioPrefs, shared: &Arc<Shared>, net: &tmpsc::Sender<VoiceCtl>, aud: &tmpsc::UnboundedSender<AudioUp>, users: &HashMap<u64, (f32, bool)>) -> (Media, Option<String>) {
    let (backend, prefs, shared, net, aud, users) = (opts.backend.clone(), prefs.clone(), shared.clone(), net.clone(), aud.clone(), users.clone());
    match tokio::task::spawn_blocking(move || Media::open(backend.as_ref(), &prefs, shared, net, aud, &users)).await {
        Ok((m, err)) => (m, err.as_ref().map(error_text)),
        // Panique à l'ouverture (pilote) : l'appel continue, sans son.
        Err(_) => (Media::closed(), Some(crate::i18n::tr(vt::AUDIO_FAILED).to_string())),
    }
}

/// Lâche les périphériques hors de la boucle (la fermeture attend les fils audio).
pub(super) fn close_media(m: Media) {
    let _ = std::thread::Builder::new().name("ferrocord-voix-arret".into()).spawn(move || drop(m));
}

/// Codes de vérification : scrypt par personne, hors du fil d'interface et de la boucle.
async fn codes(dave: Option<Arc<Mutex<DaveSession>>>) -> VoiceCodes {
    let inputs = dave.map(|d| locked(&d).code_inputs()).unwrap_or_default();
    tokio::task::spawn_blocking(move || {
        let users = inputs.users().map(|u| (u, inputs.pairwise(u))).collect();
        VoiceCodes { call: inputs.call.clone(), users }
    })
    .await
    .unwrap_or_default()
}

pub(super) async fn run(s: Start, mut rx: tmpsc::UnboundedReceiver<Msg>) {
    let Start { params, mut settings, mut users, ptt, opts, out, camera, video } = s;
    let me = params.user_id;
    let dave = DaveSession::new(params.user_id, params.dave_group_id()).ok().map(|d| Arc::new(Mutex::new(d)));
    let crypto: Box<dyn FrameCrypto> = match &dave {
        Some(d) => Box::new(SharedDave(d.clone())),
        // Sans clé d'identité (cas qui ne devrait pas arriver) : chiffrement de transport seul.
        None => Box::new(Passthrough),
    };
    let (net, mut net_rx) = voice::spawn_with(&tokio::runtime::Handle::current(), params, crypto, voice_config(&opts));
    let mut send_codec = Codec::H264;
    let (aud_tx, mut aud_rx) = tmpsc::unbounded_channel();
    let shared = Arc::new(Shared::new(&settings, ptt));
    let mut cam = camera.map(|c| (c.clone(), video.camera(&opts, c, &net, me, send_codec)));
    let mut receiving = Receiving::new(opts.video.clone(), video.sink.clone(), net.clone(), video.report.clone());
    let (mut media, err) = open_media(&opts, &settings.prefs, &shared, &net, &aud_tx, &users).await;
    *lock_player(&video.player) = Some(media.player());
    video.in_call.store(true, std::sync::atomic::Ordering::Relaxed);
    out.event(VoiceEvent::AudioProblem(err));
    let mut check = tokio::time::interval(opts.device_check);
    check.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut others: Vec<u64> = Vec::new();
    let mut me_speaking = false;
    loop {
        tokio::select! {
            e = net_rx.recv() => match e {
                Some(NetEvent::Audio { user_id, seq, opus, .. }) => media.send(PlayMsg::Packet { user_id, seq, opus }),
                Some(NetEvent::Ready { .. }) => out.event(VoiceEvent::Connected),
                Some(NetEvent::Ping(ms)) => out.event(VoiceEvent::Ping(ms)),
                // « Je parle » de la passerelle sans son derrière n'allume rien :
                // l'audio reçu décide ; son arrêt, lui, éteint tout de suite.
                Some(NetEvent::Speaking { user_id, speaking: false }) | Some(NetEvent::UserLeft(user_id)) => {
                    if matches!(e, Some(NetEvent::UserLeft(_))) {
                        receiving.stop(user_id);
                    }
                    if let Some(i) = others.iter().position(|u| *u == user_id) {
                        others.remove(i);
                        out.event(VoiceEvent::Speaking { user_id, speaking: false });
                    }
                }
                Some(NetEvent::Speaking { .. } | NetEvent::UserJoined(_)) => {}
                Some(NetEvent::Video { user_id, frame }) => receiving.frame(user_id, Tile::Camera(user_id), frame),
                Some(NetEvent::VideoState { user_id, active: false }) => receiving.stop(user_id),
                Some(NetEvent::VideoState { .. }) => {}
                Some(NetEvent::SendCodec(c)) => {
                    send_codec = c;
                    if let Some((_, s)) = &cam {
                        s.set_codec(c);
                    }
                }
                Some(NetEvent::KeyframeRequested) => {
                    if let Some((_, c)) = &cam {
                        c.keyframe();
                    }
                }
                Some(NetEvent::Closed(r)) => {
                    match r {
                        CloseReason::SessionExpired(_) => out.event(VoiceEvent::Expired),
                        CloseReason::Disconnected(_) => out.event(VoiceEvent::Closed),
                        r => {
                            if let Some(why) = failure(r) {
                                out.event(VoiceEvent::Failed(why));
                            }
                        }
                    }
                    break;
                }
                None => break,
            },
            a = aud_rx.recv() => match a {
                Some(AudioUp::Me(b)) => {
                    if b != me_speaking {
                        me_speaking = b;
                        out.event(VoiceEvent::Speaking { user_id: me, speaking: b });
                    }
                }
                Some(AudioUp::Others(mut now)) => {
                    // Le son d'un direct regardé n'est pas une personne qui parle.
                    now.retain(|u| u & STREAM_AUDIO == 0);
                    for u in others.iter().filter(|u| !now.contains(u)) {
                        out.event(VoiceEvent::Speaking { user_id: *u, speaking: false });
                    }
                    for u in now.iter().filter(|u| !others.contains(u)) {
                        out.event(VoiceEvent::Speaking { user_id: *u, speaking: true });
                    }
                    others = now;
                }
                None => {}
            },
            m = rx.recv() => match m {
                None | Some(Msg::Stop) => {
                    cam = None;
                    let _ = net.send(VoiceCtl::Disconnect).await;
                    break;
                }
                Some(Msg::Camera(want)) => match want {
                    None => camera_off(&mut cam, &net),
                    Some(src) => {
                        if cam.as_ref().is_none_or(|(s, _)| *s != src) {
                            // Autre caméra : la précédente s'arrête d'abord.
                            drop(cam.take());
                            cam = Some((src.clone(), video.camera(&opts, src, &net, me, send_codec)));
                        }
                    }
                },
                Some(Msg::Wants(w)) => {
                    let _ = net.send(VoiceCtl::SinkWants(w)).await;
                }
                Some(Msg::Settings(n)) => {
                    shared.set(&n);
                    let devices = (n.prefs.input_device != media.input) || (n.prefs.output_device != media.output);
                    settings = n;
                    if devices {
                        close_media(media);
                        let (m, err) = open_media(&opts, &settings.prefs, &shared, &net, &aud_tx, &users).await;
                        media = m;
                        *lock_player(&video.player) = Some(media.player());
                        out.event(VoiceEvent::AudioProblem(err));
                    }
                }
                Some(Msg::Ptt(b)) => shared.set_ptt(b),
                // Enregistrement local : les fils audio prennent (ou rendent) leur prise au rappel suivant.
                Some(Msg::Record(tx)) => shared.set_rec(tx),
                Some(Msg::UserVolume(u, v)) => {
                    users.entry(u).or_insert((1.0, false)).0 = v;
                    media.send(PlayMsg::Volume(u, v));
                }
                Some(Msg::UserMuted(u, b)) => {
                    users.entry(u).or_insert((1.0, false)).1 = b;
                    media.send(PlayMsg::Muted(u, b));
                }
                Some(Msg::Codes) => out.event(VoiceEvent::Codes(codes(dave.clone()).await)),
            },
            _ = check.tick(), if media.is_open() => {
                if let Some(e) = media.take_error() {
                    out.event(VoiceEvent::AudioProblem(Some(error_text(&e))));
                }
            }
        }
    }
    drop(cam);
    drop(receiving);
    // Fin de l'appel : l'enregistrement en cours est fermé (émetteurs lâchés).
    shared.set_rec(None);
    video.in_call.store(false, std::sync::atomic::Ordering::Relaxed);
    *lock_player(&video.player) = None;
    close_media(media);
    out.up(Up::Ended);
}

fn lock_player(p: &Player) -> MutexGuard<'_, Option<mpsc::Sender<super::media::PlayMsg>>> {
    p.lock().unwrap_or_else(|e| e.into_inner())
}
