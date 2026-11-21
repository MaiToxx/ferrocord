//! Orchestrateur du vocal (phase 4, lot V7) et de la vidéo (phase 5, lot
//! M5) : relie la connexion vocale (`net::voice`, chiffrement DAVE compris),
//! le moteur audio (`audio::engine`), les périphériques (`audio::io`), le
//! traitement de la voix (`audio::dsp`), la capture et les codecs vidéo
//! (`video`) à l'état du magasin (`state::voice`, `state::video`,
//! `state::stream`).
//!
//! L'interface ne le voit que par `state::voice_link::VoiceLink` : à chaque
//! passe elle dit ce qu'elle veut, il répond par des `state::voice::VoiceEvent`
//! et des `state::video_link::VideoUp`. Comme `session.rs`, ce module a le
//! droit au réseau, à l'audio et à la vidéo.
//!
//! Hors appel, rien ne tourne : ni tâche, ni fil audio ou vidéo, ni minuterie
//! (sauf les 15 s d'attente de la réponse de Discord à l'op 4, une seule
//! fois). Pendant un appel : une tâche tokio (celle-ci) et la connexion
//! vocale sur le runtime de la session, et les deux fils audio des
//! périphériques ; avec de la vidéo, un fil par encodeur et par décodeur ;
//! par direct, une tâche et sa connexion.

mod call;
mod clips;
mod media;
mod player;
mod ptt_hook;
pub(crate) mod ptt_keys;
mod recorder;
mod stream;
pub mod video;

pub use media::{AudioBackend, AudioStreams, Devices};
pub use video::{extra_codecs_available, set_extra_codecs, Capturing, VideoBackend, WindowsVideo};

use crate::i18n::table::voice::t as vt;
use crate::net::voice::{ConnKind, Secret, VoiceConfig, VoiceParams};
use crate::state::audio_prefs::AudioPrefs;
use crate::state::stream::StreamEvent;
use crate::state::video::{Tile, VideoSink};
use crate::state::video_link::{sink_quality, StreamTarget, StreamWant, VideoUp, VideoWant};
use crate::state::voice::{VoiceCmd, VoiceEvent};
use crate::state::voice_link::{CallTarget, CallWant, VoiceLink, VoiceWant};
use crate::state::voice_msg::{ClipKey, ClipPlay, ClipUp, PttKey, RecordCmd};
use std::collections::HashMap;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use stream::{Role, StreamMsg};
use tokio::sync::mpsc as tmpsc;

/// Attente de la réponse de Discord à l'op 4 avant d'annoncer l'échec.
pub const JOIN_TIMEOUT: Duration = Duration::from_secs(15);

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Le son d'un direct regardé passe par le fil de lecture de l'appel, sous
/// l'identifiant de son auteur marqué de ce bit (jamais pris par un
/// identifiant Discord, positif sur 63 bits) : il garde sa propre file de gigue.
pub(crate) const STREAM_AUDIO: u64 = 1 << 63;

/// Fil de lecture de l'appel en cours, s'il y en a un.
type Player = Arc<Mutex<Option<mpsc::Sender<media::PlayMsg>>>>;

fn lock_player(p: &Player) -> std::sync::MutexGuard<'_, Option<mpsc::Sender<media::PlayMsg>>> {
    p.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// La sortie de l'appel en cours, pour les sons de l'interface.
struct CallMixer {
    player: Player,
    in_call: Arc<std::sync::atomic::AtomicBool>,
}

impl crate::audio::clip::Mixer for CallMixer {
    fn play(&self, id: u64, pcm: crate::audio::clip::Pcm, volume: f32) -> bool {
        if !self.in_call.load(std::sync::atomic::Ordering::Relaxed) {
            return false;
        }
        let Some(p) = lock_player(&self.player).clone() else { return false };
        let _ = p.send(media::PlayMsg::StopClip(id));
        p.send(media::PlayMsg::Clip { id, pcm, pos: 0, volume, deafen: false }).is_ok()
    }

    fn stop(&self, id: u64) {
        if let Some(p) = lock_player(&self.player).as_ref() {
            let _ = p.send(media::PlayMsg::StopClip(id));
        }
    }
}

/// Réglages de l'orchestrateur ; l'application n'utilise que [`Options::real`].
#[derive(Clone)]
pub struct Options {
    pub voice: VoiceConfig,
    pub backend: Arc<dyn AudioBackend>,
    /// Capture et codecs vidéo.
    pub video: Arc<dyn VideoBackend>,
    pub join_timeout: Duration,
    /// Rythme du relevé des pannes de périphérique pendant un appel.
    pub device_check: Duration,
}

impl Options {
    /// Serveurs vocaux de Discord, périphériques de Windows.
    pub fn real() -> Self {
        Options { voice: VoiceConfig::default(), backend: Arc::new(Devices), video: Arc::new(WindowsVideo), join_timeout: JOIN_TIMEOUT, device_check: Duration::from_secs(2) }
    }
}

/// Ce que l'appel en cours doit appliquer (comparé à chaque passe).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Settings {
    pub muted: bool,
    pub deafened: bool,
    pub prefs: AudioPrefs,
    /// Plugins vocaux (lot E4), lus sur le fil de l'interface à chaque passe.
    pub extras: crate::audio::dsp::Extras,
}

/// Interface -> tâche de l'appel.
enum Msg {
    Settings(Settings),
    Ptt(bool),
    UserVolume(u64, f32),
    UserMuted(u64, bool),
    Codes,
    /// Ma caméra : `None` éteinte, `Some(None)` la caméra par défaut.
    Camera(Option<Option<String>>),
    /// Qualités voulues de la vidéo des autres (op 15).
    Wants(Vec<(u64, u8)>),
    /// Enregistrement local (plugin, lot E4) : son fil d'écriture, ou plus rien.
    Record(Option<recorder::RecTx>),
    Stop,
}

/// Tâches (appel, directs, fils vidéo, minuterie) -> interface.
enum Up {
    Event(VoiceEvent),
    /// La tâche de l'appel est finie.
    Ended,
    /// Fin de l'attente de la réponse à l'op 4 n° `.0`.
    Timeout(u64),
    Video(VideoUp),
    /// Connexion d'un direct (numéro de la tâche en tête du couple).
    Stream { key: String, event: StreamEvent },
    /// La tâche de ce direct est finie.
    StreamEnded(String),
    /// Sons courts, sortie seule, enregistrement (phase 6).
    Clip(clips::ClipMsg),
    /// Issue de l'enregistrement local d'un appel (lot E4).
    Rec(crate::state::call_rec::CallRecUp),
}

/// Ce qui identifie une connexion : autre chose, et on la refait.
#[derive(Clone, Debug)]
struct Key {
    guild_id: Option<u64>,
    channel_id: u64,
    user_id: u64,
    endpoint: String,
    session_id: String,
    token: Secret,
}

impl Key {
    fn of(t: &CallTarget<'_>) -> Self {
        Key { guild_id: t.guild_id, channel_id: t.channel_id, user_id: t.user_id, endpoint: t.endpoint.into(), session_id: t.session_id.into(), token: t.token.clone() }
    }

    fn is(&self, t: &CallTarget<'_>) -> bool {
        self.guild_id == t.guild_id
            && self.channel_id == t.channel_id
            && self.user_id == t.user_id
            && self.endpoint == t.endpoint
            && self.session_id == t.session_id
            && self.token.expose() == t.token.expose()
    }
}

struct Running {
    generation: u64,
    key: Key,
    tx: tmpsc::UnboundedSender<Msg>,
}

/// Ce qui identifie la connexion d'un direct.
#[derive(Clone, Debug, PartialEq)]
struct StreamKey {
    server_id: u64,
    channel_id: u64,
    endpoint: String,
    session_id: String,
    token: String,
    role: Role,
}

impl StreamKey {
    fn of(t: &StreamTarget<'_>, role: &Role) -> Self {
        StreamKey { server_id: t.server_id, channel_id: t.channel_id, endpoint: t.endpoint.into(), session_id: t.session_id.into(), token: t.token.expose().into(), role: role.clone() }
    }

    /// Même connexion : tout pareil, sauf ce que montre mon partage (source,
    /// qualité, son), qui change sans la rouvrir.
    fn same_connection(&self, o: &StreamKey) -> bool {
        let role = self.role == o.role || matches!((&self.role, &o.role), (Role::Share { .. }, Role::Share { .. }));
        role && (self.server_id, self.channel_id, &self.endpoint, &self.session_id, &self.token) == (o.server_id, o.channel_id, &o.endpoint, &o.session_id, &o.token)
    }
}

struct RunningStream {
    generation: u64,
    key: StreamKey,
    tx: tmpsc::UnboundedSender<StreamMsg>,
    /// Dernière qualité demandée (direct regardé).
    quality: Option<u8>,
}

/// L'orchestrateur, côté interface.
pub struct Voice {
    rt: tokio::runtime::Handle,
    opts: Options,
    wake: Wake,
    up_tx: mpsc::Sender<(u64, Up)>,
    up_rx: mpsc::Receiver<(u64, Up)>,
    generation: u64,
    running: Option<Running>,
    settings: Option<Settings>,
    /// Volume et muet réglés par personne (gardés hors appel, appliqués en entrant).
    users: HashMap<u64, (f32, bool)>,
    ptt: bool,
    /// Attente de la réponse à l'op 4 en cours (numéro).
    waiting: Option<u64>,
    waits: u64,
    /// Ma caméra voulue (gardée pour l'appel suivant).
    camera: Option<Option<String>>,
    /// Qualités voulues envoyées à l'appel en cours.
    wants: Vec<(u64, u8)>,
    sink: Option<VideoSink>,
    player: Player,
    in_call: Arc<std::sync::atomic::AtomicBool>,
    streams: HashMap<String, RunningStream>,
    stream_gens: u64,
    voice_out: Vec<VoiceEvent>,
    video_out: Vec<VideoUp>,
    clips: clips::Clips,
    /// Fichier joint en lecture (lot M).
    files: player::Files,
    /// Touche de l'appui-pour-parler et crochet global posé (pendant un appel).
    ptt_target: Option<ptt_keys::Target>,
    hook: Option<(ptt_keys::Target, Option<ptt_hook::Hook>)>,
    /// Enregistrement local en cours (plugin, lot E4) : son fil d'écriture.
    /// `None` hors enregistrement : ni fil, ni file.
    rec: Option<recorder::RecTx>,
    rec_out: Vec<crate::state::call_rec::CallRecUp>,
}

impl Voice {
    pub fn new(rt: tokio::runtime::Handle, opts: Options, wake: Wake) -> Self {
        let (up_tx, up_rx) = mpsc::channel();
        Voice {
            rt,
            opts,
            wake,
            up_tx,
            up_rx,
            generation: 0,
            running: None,
            settings: None,
            users: HashMap::new(),
            ptt: false,
            waiting: None,
            waits: 0,
            camera: None,
            wants: Vec::new(),
            sink: None,
            player: Arc::new(Mutex::new(None)),
            in_call: Arc::default(),
            streams: HashMap::new(),
            stream_gens: 0,
            voice_out: Vec::new(),
            video_out: Vec::new(),
            clips: clips::Clips::new(),
            files: player::Files::default(),
            ptt_target: None,
            hook: None,
            rec: None,
            rec_out: Vec::new(),
        }
    }

    /// Un enregistrement local tourne (son fil d'écriture existe).
    pub fn recording(&self) -> bool {
        self.rec.is_some()
    }

    /// Arrête l'enregistrement local : les fils audio rendent leur prise, le
    /// fil d'écriture ferme ses fichiers et s'arrête.
    fn rec_stop(&mut self) {
        if let Some(tx) = self.rec.take() {
            self.send(Msg::Record(None));
            let _ = tx.send(recorder::RecMsg::Stop);
        }
    }

    fn rec_start(&mut self, dir: std::path::PathBuf, base: String, tracks: bool, me: u64, names: Vec<(u64, String)>) {
        use crate::i18n::table::e4::t as xt;
        use crate::state::call_rec::{CallRecFail, CallRecUp};
        self.rec_stop();
        if self.running.is_none() {
            self.rec_out.push(CallRecUp::Failed(CallRecFail::NoCall));
            return;
        }
        let cfg = recorder::Config { dir, base, tracks, me, names, me_label: crate::i18n::tr(xt::REC_TRACK_ME).to_string(), stream_label: crate::i18n::tr(xt::REC_TRACK_STREAM).to_string() };
        let (up, wake) = (self.up_tx.clone(), self.wake.clone());
        let done = move |u| {
            if up.send((0, Up::Rec(u))).is_ok() {
                wake();
            }
        };
        match recorder::spawn(cfg, done) {
            Some(tx) => {
                self.send(Msg::Record(Some(tx.clone())));
                self.rec = Some(tx);
            }
            None => self.rec_out.push(CallRecUp::Failed(CallRecFail::Create)),
        }
    }

    /// Un appel tourne (tâche et périphériques).
    pub fn in_call(&self) -> bool {
        self.running.is_some()
    }

    /// Directs dont la tâche tourne (mon partage, ceux que je regarde).
    pub fn streams_running(&self) -> usize {
        self.streams.len()
    }

    fn stop(&mut self) {
        // L'enregistrement ne survit pas à l'appel.
        self.rec_stop();
        if let Some(r) = self.running.take() {
            let _ = r.tx.send(Msg::Stop);
        }
    }

    fn send(&self, m: Msg) {
        if let Some(r) = &self.running {
            let _ = r.tx.send(m);
        }
    }

    /// Vers l'interface, depuis les fils vidéo.
    fn report(&self) -> video::Report {
        let (tx, wake) = (self.up_tx.clone(), self.wake.clone());
        Arc::new(move |v| {
            if tx.send((0, Up::Video(v))).is_ok() {
                wake();
            }
        })
    }

    fn sink(&self) -> VideoSink {
        self.sink.clone().unwrap_or_default()
    }

    fn start(&mut self, t: &CallTarget<'_>, settings: Settings) {
        self.stop();
        self.generation += 1;
        self.wants.clear();
        let (tx, rx) = tmpsc::unbounded_channel();
        let params = VoiceParams {
            guild_id: t.guild_id,
            channel_id: t.channel_id,
            user_id: t.user_id,
            session_id: t.session_id.into(),
            endpoint: t.endpoint.into(),
            token: t.token.clone(),
            kind: ConnKind::Voice,
        };
        let out = call::Out { generation: self.generation, tx: self.up_tx.clone(), wake: self.wake.clone() };
        let video = call::VideoCx { sink: self.sink(), report: self.report(), player: self.player.clone(), in_call: self.in_call.clone() };
        let start = call::Start { params, settings, users: self.users.clone(), ptt: self.ptt, opts: self.opts.clone(), out, camera: self.camera.clone(), video };
        self.rt.spawn(call::run(start, rx));
        self.running = Some(Running { generation: self.generation, key: Key::of(t), tx });
        // La sortie seule laisse la place à celle de l'appel.
        self.sync_output();
    }

    fn start_wait(&mut self) {
        if self.waiting.is_some() {
            return;
        }
        self.waits += 1;
        let n = self.waits;
        self.waiting = Some(n);
        let (tx, wake, d) = (self.up_tx.clone(), self.wake.clone(), self.opts.join_timeout);
        self.rt.spawn(async move {
            tokio::time::sleep(d).await;
            if tx.send((0, Up::Timeout(n))).is_ok() {
                wake();
            }
        });
    }

    fn stop_stream(&mut self, key: &str) {
        if let Some(r) = self.streams.remove(key) {
            let _ = r.tx.send(StreamMsg::Stop);
        }
    }

    fn start_stream(&mut self, key: &str, me: u64, t: &StreamTarget<'_>, role: Role) {
        self.stop_stream(key);
        self.stream_gens += 1;
        let generation = self.stream_gens;
        let (tx, rx) = tmpsc::unbounded_channel();
        let params = VoiceParams {
            guild_id: Some(t.server_id),
            channel_id: t.channel_id,
            user_id: me,
            session_id: t.session_id.into(),
            endpoint: t.endpoint.into(),
            token: t.token.clone(),
            kind: ConnKind::Stream { stream_key: key.to_string() },
        };
        let out = stream::StreamOut { generation, key: key.to_string(), tx: self.up_tx.clone(), wake: self.wake.clone() };
        let s = stream::StreamStart { params, role: role.clone(), opts: self.opts.clone(), sink: self.sink(), report: self.report(), player: self.player.clone(), out };
        self.rt.spawn(stream::run(s, rx));
        self.streams.insert(key.to_string(), RunningStream { generation, key: StreamKey::of(t, &role), tx, quality: None });
    }

    /// Ouvre, garde ou laisse la connexion d'un direct ; rend vrai s'il doit tourner.
    fn sync_stream(&mut self, key: &str, me: u64, role: Role, conn: StreamWant<'_>, quality: Option<u8>) -> bool {
        match conn {
            StreamWant::Wait => return false,
            StreamWant::Keep => {
                if !self.streams.contains_key(key) {
                    return false;
                }
            }
            StreamWant::Open(t) => {
                let new = StreamKey::of(&t, &role);
                let same = self.streams.get(key).is_some_and(|r| r.key.same_connection(&new));
                if !same {
                    self.start_stream(key, me, &t, role.clone());
                }
            }
        }
        // Mon partage montre autre chose : la connexion reste, la capture change.
        if let (Role::Share { source_id, quality, audio }, Some(r)) = (&role, self.streams.get_mut(key))
            && r.key.role != role
        {
            r.key.role = role.clone();
            let _ = r.tx.send(StreamMsg::Source { source_id: source_id.clone(), quality: *quality, audio: *audio });
        }
        if let (Some(q), Some(r)) = (quality, self.streams.get_mut(key))
            && r.quality != Some(q)
        {
            r.quality = Some(q);
            let _ = r.tx.send(StreamMsg::Quality(q));
        }
        true
    }

    /// Relève tout ce qui est arrivé, rangé en vocal et vidéo.
    fn drain(&mut self) {
        while let Ok((generation, up)) = self.up_rx.try_recv() {
            let current = self.running.as_ref().is_some_and(|r| r.generation == generation);
            match up {
                Up::Timeout(n) => {
                    if self.waiting == Some(n) {
                        self.waiting = None;
                        self.voice_out.push(VoiceEvent::Failed(crate::i18n::tr(vt::NO_ANSWER).to_string()));
                    }
                }
                Up::Video(v) => self.video_out.push(v),
                // Événements d'un direct remplacé ou arrêté : sans objet.
                Up::Stream { key, event } => {
                    if self.streams.get(&key).is_some_and(|r| r.generation == generation) {
                        self.video_out.push(VideoUp::Stream(event));
                    }
                }
                Up::StreamEnded(key) => {
                    if self.streams.get(&key).is_some_and(|r| r.generation == generation) {
                        self.streams.remove(&key);
                    }
                }
                // Événements d'un appel remplacé ou arrêté : sans objet.
                Up::Event(e) if current => self.voice_out.push(e),
                Up::Ended if current => {
                    self.rec_stop();
                    self.running = None;
                }
                Up::Rec(u) => {
                    // Le fil d'écriture s'est arrêté seul (écriture impossible) : plus rien à lui envoyer.
                    if matches!(u, crate::state::call_rec::CallRecUp::Failed(_)) {
                        self.rec_stop();
                    }
                    self.rec_out.push(u);
                }
                Up::Clip(m) => self.on_clip(m),
                _ => {}
            }
        }
        self.sync_output();
    }

    /// Crochet clavier global : seulement pendant un appel en appui-pour-parler.
    fn sync_hook(&mut self) {
        let ptt = self.settings.as_ref().is_some_and(|s| s.prefs.mode == crate::state::audio_prefs::InputMode::PushToTalk);
        let want = self.ptt_target.filter(|_| ptt && self.running.is_some());
        ptt_hook::sync(&mut self.hook, want);
    }
}

impl Drop for Voice {
    fn drop(&mut self) {
        self.stop();
        for (_, r) in self.streams.drain() {
            let _ = r.tx.send(StreamMsg::Stop);
        }
    }
}

impl VoiceLink for Voice {
    fn sync(&mut self, want: &VoiceWant<'_>) {
        let extras = crate::audio::dsp::Extras::current();
        let same = self.settings.as_ref().is_some_and(|s| s.muted == want.muted && s.deafened == want.deafened && s.extras == extras && s.prefs == *want.audio);
        if !same {
            let s = Settings { muted: want.muted, deafened: want.deafened, prefs: want.audio.clone(), extras };
            self.send(Msg::Settings(s.clone()));
            self.clips.settings(&s);
            self.settings = Some(s);
        }
        if !matches!(want.call, CallWant::Wait) {
            self.waiting = None;
        }
        match want.call {
            CallWant::Stop => self.stop(),
            CallWant::Wait => {
                self.stop();
                self.start_wait();
            }
            CallWant::Open(t) => {
                if !self.running.as_ref().is_some_and(|r| r.key.is(&t)) {
                    let s = self.settings.clone().unwrap_or(Settings { muted: want.muted, deafened: want.deafened, prefs: want.audio.clone(), extras });
                    self.start(&t, s);
                }
            }
            CallWant::Keep => {}
        }
        self.sync_hook();
    }

    fn command(&mut self, c: &VoiceCmd) {
        match *c {
            VoiceCmd::SetUserVolume(u, v) => {
                let e = self.users.entry(u).or_insert((1.0, false));
                e.0 = v;
                self.send(Msg::UserVolume(u, v));
            }
            VoiceCmd::SetUserMuted(u, b) => {
                let e = self.users.entry(u).or_insert((1.0, false));
                e.1 = b;
                self.send(Msg::UserMuted(u, b));
            }
            VoiceCmd::SetPtt(b) => {
                self.ptt = b;
                self.send(Msg::Ptt(b));
            }
            _ => return,
        }
        // Personnes revenues aux réglages par défaut : rien à retenir.
        self.users.retain(|_, (v, m)| *v != 1.0 || *m);
    }

    fn request_codes(&mut self) {
        self.send(Msg::Codes);
    }

    fn poll(&mut self) -> Vec<VoiceEvent> {
        self.drain();
        std::mem::take(&mut self.voice_out)
    }

    fn sync_video(&mut self, want: &VideoWant<'_>) {
        if self.sink.is_none() {
            self.sink = Some(want.sink.clone());
        }
        let camera = want.camera.map(|c| c.map(str::to_string));
        if camera != self.camera {
            self.camera = camera.clone();
            self.send(Msg::Camera(camera));
        }
        if self.running.is_some() {
            let mut wants: Vec<(u64, u8)> = want.cameras.iter().map(|u| (*u, sink_quality(want.sink.wanted_quality(&Tile::Camera(*u))))).collect();
            wants.sort_unstable();
            if wants != self.wants {
                self.wants = wants.clone();
                self.send(Msg::Wants(wants));
            }
        }
        let mut keep: Vec<&str> = Vec::new();
        if let (Some(me), Some(s)) = (want.user_id, &want.share) {
            let role = Role::Share { source_id: s.source_id.to_string(), quality: s.quality, audio: s.audio };
            if self.sync_stream(s.key, me, role, s.conn, None) {
                keep.push(s.key);
            }
        }
        for w in &want.watch {
            let Some(me) = want.user_id else { break };
            let q = sink_quality(want.sink.wanted_quality(&Tile::Stream(w.key.to_string())));
            if self.sync_stream(w.key, me, Role::Watch { owner: w.owner }, w.conn, Some(q)) {
                keep.push(w.key);
            }
        }
        if self.streams.len() > keep.len() {
            let gone: Vec<String> = self.streams.keys().filter(|k| !keep.contains(&k.as_str())).cloned().collect();
            for k in gone {
                self.stop_stream(&k);
            }
        }
    }

    fn poll_video(&mut self) -> Vec<VideoUp> {
        self.drain();
        std::mem::take(&mut self.video_out)
    }

    fn call_mixer(&self) -> Option<Arc<dyn crate::audio::clip::Mixer>> {
        Some(Arc::new(CallMixer { player: self.player.clone(), in_call: self.in_call.clone() }))
    }

    fn play_clip(&mut self, p: ClipPlay) {
        self.clip_play(p);
    }

    fn stop_clip(&mut self, k: ClipKey) {
        self.clip_stop(k);
    }

    fn record(&mut self, c: RecordCmd) {
        Voice::record(self, c);
    }

    fn set_ptt_key(&mut self, k: Option<PttKey<'_>>) {
        let t = k.and_then(|k| ptt_keys::target(&k));
        if t != self.ptt_target {
            self.ptt_target = t;
            self.sync_hook();
        }
    }

    fn poll_clips(&mut self) -> Vec<ClipUp> {
        self.poll_clip_ups()
    }

    fn open_media(&mut self, url: &str, mime: Option<&str>, gif: bool) -> Option<crate::video::play::Handle> {
        Some(self.file_open(url, mime, gif))
    }

    fn close_media(&mut self) {
        self.file_close();
    }

    fn call_record(&mut self, c: crate::state::call_rec::CallRecCmd) {
        use crate::state::call_rec::CallRecCmd;
        match c {
            CallRecCmd::Start { dir, base, tracks, me, names } => self.rec_start(dir, base, tracks, me, names),
            CallRecCmd::Names(names) => {
                if let Some(tx) = &self.rec {
                    let _ = tx.send(recorder::RecMsg::Names(names));
                }
            }
            CallRecCmd::Stop => self.rec_stop(),
        }
    }

    fn poll_call_record(&mut self) -> Vec<crate::state::call_rec::CallRecUp> {
        self.drain();
        std::mem::take(&mut self.rec_out)
    }
}

#[cfg(test)]
mod mixer_tests {
    use super::*;
    use crate::audio::clip::{Mixer, Pcm};
    use std::sync::atomic::Ordering::Relaxed;

    /// Un son d'interface ne se mêle à la sortie que pendant un appel ; rejoué,
    /// il repart du début (l'ancien est arrêté d'abord).
    #[test]
    fn interface_sounds_mix_into_the_call_output_only_during_a_call() {
        let m = CallMixer { player: Arc::new(Mutex::new(None)), in_call: Arc::default() };
        let pcm = Pcm { channels: 2, data: vec![1i16; 8].into() };
        assert!(!m.play(7, pcm.clone(), 0.5), "hors appel");
        let (tx, rx) = mpsc::channel();
        *lock_player(&m.player) = Some(tx);
        // Sortie ouverte pour un message vocal, hors appel : toujours non.
        assert!(!m.play(7, pcm.clone(), 0.5));
        m.in_call.store(true, Relaxed);
        assert!(m.play(7, pcm.clone(), 0.5));
        assert!(matches!(rx.try_recv(), Ok(media::PlayMsg::StopClip(7))));
        assert!(matches!(rx.try_recv(), Ok(media::PlayMsg::Clip { id: 7, pos: 0, deafen: false, .. })));
        m.stop(7);
        assert!(matches!(rx.try_recv(), Ok(media::PlayMsg::StopClip(7))));
        // Appel fini (sortie fermée) : le lecteur de Windows reprend la main.
        drop(rx);
        assert!(!m.play(7, pcm, 0.5));
    }
}
