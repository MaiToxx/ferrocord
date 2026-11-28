//! Lecteur d'un fichier vidéo ou audio joint à un message (phase 10, lot M).
//!
//! Un fil par lecture : il tire les images et le son d'une [`Source`] (le
//! lecteur de fichiers de Media Foundation pour MP4, MOV, M4A, WAV… ; un
//! son déjà décodé par `audio::clip`, donc `symphonia` ou libopus, pour MP3
//! et Ogg/Opus), les présente à l'heure et les publie :
//!
//! - l'image du moment, réduite à la taille affichée, dans [`Handle`] ;
//!   l'interface est réveillée à chaque nouvelle image et à chaque
//!   changement d'état, jamais autrement (0 redessin en pause ou à la fin) ;
//! - le son dans une file sans verrou ([`AudioOut`]) que le fil de lecture
//!   audio mixe (`voice::media`).
//!
//! L'horloge est celle de la machine. Le son est poussé avec [`LEAD_US`]
//! d'avance sur elle : la file en garde autant, et un échantillon sort donc
//! à l'instant de son image. En pause, le fil de lecture audio cesse de
//! puiser : la suite reprend à l'échantillon près.

mod webm;

use crate::audio::io::resample::Resampler;
use crate::video::{convert, Frame};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::sync::{mpsc, Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// Fichier joué, au plus (il est tenu en mémoire le temps de la lecture).
pub const MAX_BYTES: usize = 64 << 20;
/// Fréquence de la sortie audio.
pub const RATE: u32 = 48_000;
/// Une seconde de son stéréo en attente, au plus.
const FIFO_SAMPLES: usize = RATE as usize * 2;
/// Avance du son sur l'horloge (ce que la file garde en attente).
const LEAD_US: u64 = 150_000;
/// Sans demande du fil audio depuis ce délai, la sortie est tenue pour fermée.
const DETACHED: Duration = Duration::from_millis(300);
/// Attente de la sortie audio au départ, au plus.
const OUTPUT_WAIT: Duration = Duration::from_millis(400);
/// Sans taille affichée connue, l'image publiée tient dans ce cadre.
const DEFAULT_BOX: (u32, u32) = (1280, 720);
/// Images décodées sans être montrées, au plus, pour rejoindre une position.
const MAX_SKIP: usize = 600;

pub type Wake = Arc<dyn Fn() + Send + Sync>;

/// Ce qu'un fichier contient.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Info {
    /// 0 si inconnue.
    pub duration_us: u64,
    /// Dimensions affichées de la vidéo.
    pub video: Option<(u32, u32)>,
    /// Quart de tour à appliquer à l'affichage (0, 90, 180 ou 270 degrés).
    pub rotation: u16,
    pub audio: bool,
}

/// Son décodé : `samples` entrelacés, `channels` voies à `rate` Hz.
#[derive(Clone, Debug, PartialEq)]
pub struct Chunk {
    pub pts_us: u64,
    pub rate: u32,
    pub channels: usize,
    pub samples: Vec<f32>,
}

/// Démultiplexeur et décodeurs d'un fichier. Utilisé par un seul fil, celui
/// qui l'a ouvert.
pub trait Source {
    fn info(&self) -> Info;
    /// Image suivante ; `None` à la fin.
    fn video(&mut self) -> Result<Option<Frame>, String>;
    /// Son suivant ; `None` à la fin.
    fn audio(&mut self) -> Result<Option<Chunk>, String>;
    /// Reprend à cette position (ou à l'image clé qui la précède).
    fn seek(&mut self, us: u64) -> Result<(), String>;
}

// ---------------------------------------------------------------- son

/// File du son d'une lecture, entre le fil du lecteur (écrivain) et le fil
/// de lecture audio (lecteur) : sans verrou ni allocation, `f32` stéréo
/// entrelacés rangés en bits dans des atomiques (aucun `unsafe`).
pub struct AudioOut {
    buf: Box<[AtomicU32]>,
    head: AtomicUsize,
    tail: AtomicUsize,
    /// Tout ce qui a été écrit avant cette position est périmé (saut).
    drop_until: AtomicUsize,
    paused: AtomicBool,
    /// Volume (bits d'un `f32`, 0 à 1).
    volume: AtomicU32,
    /// Passages du fil audio : il avance tant que la sortie est ouverte.
    mixes: AtomicU64,
}

impl Default for AudioOut {
    fn default() -> Self {
        AudioOut {
            buf: (0..FIFO_SAMPLES).map(|_| AtomicU32::new(0)).collect(),
            head: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            drop_until: AtomicUsize::new(0),
            paused: AtomicBool::new(false),
            volume: AtomicU32::new(1.0f32.to_bits()),
            mixes: AtomicU64::new(0),
        }
    }
}

impl AudioOut {
    /// Fil audio : ajoute à `out` (stéréo entrelacé) ce qui est disponible.
    pub fn mix(&self, out: &mut [f32]) {
        self.mixes.fetch_add(1, Ordering::Relaxed);
        if self.paused.load(Ordering::Relaxed) {
            return;
        }
        let cap = self.buf.len();
        let head = self.head.load(Ordering::Acquire);
        let mut tail = self.tail.load(Ordering::Relaxed);
        // Saut : ce qui précède `drop_until` est jeté (s'il est bien devant).
        let skip = self.drop_until.load(Ordering::Acquire);
        if skip.wrapping_sub(tail) <= head.wrapping_sub(tail) {
            tail = skip;
        }
        let n = (out.len().min(head.wrapping_sub(tail))) & !1;
        let v = f32::from_bits(self.volume.load(Ordering::Relaxed));
        for (i, o) in out[..n].iter_mut().enumerate() {
            *o += f32::from_bits(self.buf[tail.wrapping_add(i) % cap].load(Ordering::Relaxed)) * v;
        }
        self.tail.store(tail.wrapping_add(n), Ordering::Release);
    }

    /// Lecteur : ajoute ce qui tient ; renvoie le nombre d'échantillons écrits.
    fn push(&self, data: &[f32]) -> usize {
        let cap = self.buf.len();
        let head = self.head.load(Ordering::Relaxed);
        let free = cap - head.wrapping_sub(self.tail.load(Ordering::Acquire)).min(cap);
        let n = data.len().min(free) & !1;
        for (i, s) in data[..n].iter().enumerate() {
            self.buf[head.wrapping_add(i) % cap].store(s.to_bits(), Ordering::Relaxed);
        }
        self.head.store(head.wrapping_add(n), Ordering::Release);
        n
    }

    /// Lecteur : tout ce qui attend est périmé.
    fn flush(&self) {
        self.drop_until.store(self.head.load(Ordering::Relaxed), Ordering::Release);
    }

    /// Échantillons en attente (périmés compris tant que le fil audio n'est pas passé).
    fn buffered(&self) -> usize {
        self.head.load(Ordering::Relaxed).wrapping_sub(self.tail.load(Ordering::Acquire))
    }

    fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn set_volume(&self, v: f32) {
        self.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn volume(&self) -> f32 {
        f32::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn mixes(&self) -> u64 {
        self.mixes.load(Ordering::Relaxed)
    }
}

// ---------------------------------------------------------------- état partagé

/// Où en est la lecture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Téléchargement ou ouverture.
    Loading,
    Playing,
    Paused,
    /// Fin du fichier : « lecture » repart du début.
    Ended,
    Failed(String),
}

/// Image publiée : RGBA, alpha plein.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    /// Un mot par pixel (rouge, vert, bleu, alpha dans l'ordre des octets) :
    /// l'interface en fait une image d'egui sans le recopier.
    pub pixels: Vec<u32>,
}

impl Picture {
    /// Les octets de l'image, quatre par pixel.
    pub fn bytes(&self) -> &[u8] {
        bytemuck::cast_slice(&self.pixels)
    }
}

/// Ce que l'interface lit à chaque image.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub phase: Phase,
    pub info: Info,
    pub pos_us: u64,
    /// Numéro de la dernière image publiée (0 : aucune).
    pub generation: u64,
}

struct State {
    phase: Phase,
    info: Info,
    picture: Option<Picture>,
    generation: u64,
    /// Commandes, prises par le fil du lecteur à son départ.
    rx: Option<mpsc::Receiver<Cmd>>,
}

struct Shared {
    state: Mutex<State>,
    pos_us: AtomicU64,
    /// Taille affichée, en pixels (`largeur << 32 | hauteur`) ; 0 : inconnue.
    shown: AtomicU64,
    closed: AtomicBool,
    /// GIF en vidéo : lu en boucle, sans son (lot VID).
    gif: AtomicBool,
    audio: Arc<AudioOut>,
    wake: Wake,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_phase(&self, phase: Phase) {
        let changed = {
            let mut g = self.lock();
            let changed = g.phase != phase;
            g.phase = phase;
            changed
        };
        if changed {
            (self.wake)();
        }
    }
}

enum Cmd {
    Play,
    Pause,
    Seek(u64),
    Close,
}

/// Une lecture, vue de l'interface et de l'orchestrateur. La fermer
/// ([`Handle::close`]) arrête son fil.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
    tx: mpsc::Sender<Cmd>,
}

impl Handle {
    /// Lecture en préparation (téléchargement) : rien ne tourne encore.
    pub fn loading(wake: Wake) -> Handle {
        let (tx, rx) = mpsc::channel();
        let state = State { phase: Phase::Loading, info: Info::default(), picture: None, generation: 0, rx: Some(rx) };
        let shared = Shared { state: Mutex::new(state), pos_us: AtomicU64::new(0), shown: AtomicU64::new(0), closed: AtomicBool::new(false), gif: AtomicBool::new(false), audio: Arc::new(AudioOut::default()), wake };
        Handle { shared: Arc::new(shared), tx }
    }

    /// Lecture figée dans cet état, sans fil (aperçu, tests de l'interface).
    pub fn fixed(phase: Phase, info: Info, pos_us: u64, picture: Option<Picture>) -> Handle {
        let h = Handle::loading(Arc::new(|| {}));
        {
            let mut g = h.shared.lock();
            g.phase = phase;
            g.info = info;
            g.generation = u64::from(picture.is_some());
            g.picture = picture;
        }
        h.shared.pos_us.store(pos_us, Ordering::Relaxed);
        h
    }

    /// Commandes envoyées et pas encore prises (lecture figée : jamais prises).
    #[cfg(test)]
    pub fn sent(&self) -> Vec<&'static str> {
        let g = self.shared.lock();
        let Some(rx) = g.rx.as_ref() else { return Vec::new() };
        rx.try_iter()
            .map(|c| match c {
                Cmd::Play => "lecture",
                Cmd::Pause => "pause",
                Cmd::Seek(_) => "saut",
                Cmd::Close => "fin",
            })
            .collect()
    }

    /// Démarre le fil du lecteur : `open` ouvre le fichier sur ce fil (les
    /// décodeurs de Windows lui restent attachés), puis la lecture commence.
    pub fn start(&self, open: impl FnOnce() -> Result<Box<dyn Source>, String> + Send + 'static) {
        let Some(rx) = self.shared.lock().rx.take() else { return };
        let shared = self.shared.clone();
        let spawned = std::thread::Builder::new().name("ferrocord-lecteur".into()).spawn(move || match open() {
            Ok(src) => run(src, &shared, &rx),
            Err(e) => shared.set_phase(Phase::Failed(e)),
        });
        if spawned.is_err() {
            self.fail("fil".into());
        }
    }

    /// À dire avant [`Handle::start`] : la vidéo est un GIF (Tenor…), lue en
    /// boucle et sans son, comme Discord.
    pub fn set_gif(&self) {
        self.shared.gif.store(true, Ordering::Relaxed);
    }

    pub fn is_gif(&self) -> bool {
        self.shared.gif.load(Ordering::Relaxed)
    }

    /// Téléchargement ou ouverture impossible.
    pub fn fail(&self, why: String) {
        self.shared.set_phase(Phase::Failed(why));
    }

    pub fn snapshot(&self) -> Snapshot {
        let g = self.shared.lock();
        Snapshot { phase: g.phase.clone(), info: g.info, pos_us: self.shared.pos_us.load(Ordering::Relaxed), generation: g.generation }
    }

    /// L'image publiée, si elle est plus récente que `seen` (elle est alors retirée).
    pub fn take_picture(&self, seen: u64) -> Option<(u64, Picture)> {
        let mut g = self.shared.lock();
        if g.generation <= seen {
            return None;
        }
        let generation = g.generation;
        g.picture.take().map(|p| (generation, p))
    }

    /// Taille affichée, en pixels : les images publiées y tiennent.
    pub fn set_shown(&self, width: u32, height: u32) {
        self.shared.shown.store((u64::from(width) << 32) | u64::from(height), Ordering::Relaxed);
    }

    pub fn play(&self) {
        let _ = self.tx.send(Cmd::Play);
    }

    pub fn pause(&self) {
        let _ = self.tx.send(Cmd::Pause);
    }

    pub fn seek(&self, us: u64) {
        let _ = self.tx.send(Cmd::Seek(us));
    }

    pub fn set_volume(&self, v: f32) {
        self.shared.audio.set_volume(v);
    }

    pub fn volume(&self) -> f32 {
        self.shared.audio.volume()
    }

    /// Le son de cette lecture, pour le fil de lecture audio.
    pub fn audio(&self) -> Arc<AudioOut> {
        self.shared.audio.clone()
    }

    pub fn is_closed(&self) -> bool {
        self.shared.closed.load(Ordering::Relaxed)
    }

    /// Arrête la lecture et son fil.
    pub fn close(&self) {
        self.shared.closed.store(true, Ordering::Relaxed);
        self.shared.audio.set_paused(true);
        let _ = self.tx.send(Cmd::Close);
    }
}

// ---------------------------------------------------------------- fil du lecteur

/// Horloge de la lecture : position de départ, et instant du départ (`None` en pause).
struct Clock {
    base_us: u64,
    since: Option<Instant>,
}

impl Clock {
    fn pos(&self) -> u64 {
        self.base_us + self.since.map_or(0, |s| u64::try_from(s.elapsed().as_micros()).unwrap_or(u64::MAX / 2))
    }

    fn set(&mut self, us: u64, running: bool) {
        self.base_us = us;
        self.since = running.then(Instant::now);
    }

    fn run(&mut self, running: bool) {
        let at = self.pos();
        self.set(at, running);
    }
}

/// Son prêt pour la sortie : stéréo à 48 kHz, pas encore poussé à partir de `at`.
struct Ready {
    pts_us: u64,
    samples: Vec<f32>,
    at: usize,
}

impl Ready {
    /// Instant du premier échantillon restant.
    fn pts(&self) -> u64 {
        self.pts_us + (self.at as u64 / 2) * 1_000_000 / u64::from(RATE)
    }
}

/// Voies quelconques vers stéréo : une voie doublée ; au-delà de deux, les
/// deux premières (gauche, droite), avec le centre s'il existe.
fn to_stereo(samples: &[f32], channels: usize) -> Vec<f32> {
    match channels {
        0 => Vec::new(),
        1 => samples.iter().flat_map(|s| [*s, *s]).collect(),
        2 => samples.to_vec(),
        n => samples
            .chunks_exact(n)
            .flat_map(|f| {
                let c = f[2] * std::f32::consts::FRAC_1_SQRT_2;
                [f[0] + c, f[1] + c]
            })
            .collect(),
    }
}

/// Rééchantillonneur gardé d'un morceau à l'autre (même fréquence d'entrée).
struct ToOutput {
    rs: Option<(u32, Resampler)>,
}

impl ToOutput {
    fn convert(&mut self, c: Chunk) -> Ready {
        let stereo = to_stereo(&c.samples, c.channels);
        if c.rate == RATE || c.rate == 0 {
            return Ready { pts_us: c.pts_us, samples: stereo, at: 0 };
        }
        if self.rs.as_ref().is_none_or(|(r, _)| *r != c.rate) {
            self.rs = Some((c.rate, Resampler::new(c.rate, RATE, 2)));
        }
        let Some((_, rs)) = self.rs.as_mut() else { return Ready { pts_us: c.pts_us, samples: stereo, at: 0 } };
        let mut out = Vec::with_capacity(rs.max_output(stereo.len() / 2) * 2);
        let mut buf = vec![0.0f32; rs.max_output(4096) * 2];
        for part in stereo.chunks(4096 * 2) {
            let mut at = 0;
            while at < part.len() {
                let (read, written) = rs.process(&part[at..], &mut buf);
                out.extend_from_slice(&buf[..written * 2]);
                if read == 0 && written == 0 {
                    break;
                }
                at += read * 2;
            }
        }
        Ready { pts_us: c.pts_us, samples: out, at: 0 }
    }
}

struct Player<'a> {
    src: Box<dyn Source>,
    shared: &'a Shared,
    info: Info,
    clock: Clock,
    playing: bool,
    /// Image décodée en attente de son heure.
    pending: Option<Frame>,
    video_done: bool,
    audio_done: bool,
    ready: Option<Ready>,
    to_output: ToOutput,
    /// Dernier passage vu du fil audio, et quand (`None` : jamais vu).
    mixes: (u64, Option<Instant>),
    scaler: convert::Scaler,
    full: Vec<u8>,
}

impl Player<'_> {
    fn fail(&self, e: String) {
        self.shared.set_phase(Phase::Failed(e));
    }

    /// La sortie audio puise-t-elle dans la file ?
    fn attached(&mut self) -> bool {
        let m = self.shared.audio.mixes();
        let was = self.listened();
        if m != self.mixes.0 {
            self.mixes = (m, Some(Instant::now()));
            if !was {
                // Sortie (r)ouverte : ce qui traînait dans la file est périmé.
                self.shared.audio.flush();
            }
        }
        self.listened()
    }

    fn listened(&self) -> bool {
        self.mixes.1.is_some_and(|t| t.elapsed() < DETACHED)
    }

    /// Publie l'image `f`, réduite à la taille affichée.
    fn publish(&mut self, f: &Frame) {
        let shown = self.shared.shown.load(Ordering::Relaxed);
        let (bw, bh) = if shown == 0 { DEFAULT_BOX } else { ((shown >> 32) as u32, shown as u32) };
        // Image tournée à l'affichage : le cadre l'est aussi.
        let (bw, bh) = if self.info.rotation % 180 == 90 { (bh, bw) } else { (bw, bh) };
        let (w, h) = convert::fit_within(f.width, f.height, bw.max(1), bh.max(1));
        let (sw, sh) = (f.width as usize, f.height as usize);
        // Le tampon part avec l'image : l'interface le passe à la carte
        // graphique sans le recopier.
        let mut pixels = Vec::new();
        if (w, h) == (f.width, f.height) {
            // À la taille de la source : convertie droit dans l'image qui part.
            if convert::frame_to_rgba_into(f, convert::pixel_bytes(&mut pixels, sw * sh)).is_err() {
                return;
            }
        } else if convert::frame_to_rgba(f, &mut self.full).is_err() || self.scaler.scale_rgba_into(&self.full, sw, sh, convert::pixel_bytes(&mut pixels, w as usize * h as usize), w as usize, h as usize).is_err() {
            return;
        }
        {
            let mut g = self.shared.lock();
            g.picture = Some(Picture { width: w, height: h, pixels });
            g.generation += 1;
        }
        (self.shared.wake)();
    }

    /// Décode jusqu'à la première image à `us` ou après, et la publie.
    fn show_at(&mut self, us: u64) -> Result<(), String> {
        self.pending = None;
        let mut last = None;
        for _ in 0..MAX_SKIP {
            match self.src.video()? {
                Some(f) if f.pts_us < us => last = Some(f),
                Some(f) => {
                    last = Some(f);
                    break;
                }
                None => {
                    self.video_done = true;
                    break;
                }
            }
        }
        if let Some(f) = last {
            self.publish(&f);
        }
        Ok(())
    }

    fn seek(&mut self, us: u64) -> Result<(), String> {
        let us = if self.info.duration_us > 0 { us.min(self.info.duration_us) } else { us };
        self.src.seek(us)?;
        self.shared.audio.flush();
        self.ready = None;
        self.video_done = self.info.video.is_none();
        self.audio_done = !self.info.audio;
        self.clock.set(us, self.playing);
        self.shared.pos_us.store(us, Ordering::Relaxed);
        if self.info.video.is_some() {
            self.show_at(us)?;
            // Le décodage a pris du temps : l'horloge repart de la position voulue.
            self.clock.set(us, self.playing);
        }
        Ok(())
    }

    fn set_playing(&mut self, playing: bool) {
        self.playing = playing;
        self.clock.run(playing);
        self.shared.audio.set_paused(!playing);
        self.shared.set_phase(if playing { Phase::Playing } else { Phase::Paused });
    }

    /// Pousse le son dû jusqu'à `pos` + avance ; sans sortie, il est lu et jeté.
    fn feed_audio(&mut self, pos: u64) -> Result<(), String> {
        let attached = self.attached();
        while !self.audio_done || self.ready.is_some() {
            let mut r = match self.ready.take() {
                Some(r) => r,
                None => match self.src.audio()? {
                    Some(c) => self.to_output.convert(c),
                    None => {
                        self.audio_done = true;
                        break;
                    }
                },
            };
            if r.pts() > pos + LEAD_US {
                self.ready = Some(r);
                break;
            }
            // Ce qui est déjà passé (reprise après un saut, sortie rouverte) est sauté.
            if r.pts() < pos {
                let late = ((pos - r.pts()) * u64::from(RATE) / 1_000_000) as usize * 2;
                r.at = (r.at + late).min(r.samples.len());
            }
            if attached {
                r.at += self.shared.audio.push(&r.samples[r.at..]);
                if r.at < r.samples.len() {
                    // File pleine : le reste attend le prochain passage.
                    self.ready = Some(r);
                    break;
                }
            }
        }
        Ok(())
    }

    /// Montre l'image due à `pos` (les images en retard sont sautées) et
    /// renvoie l'attente jusqu'à la suivante.
    fn feed_video(&mut self, pos: u64) -> Result<Option<Duration>, String> {
        let mut due = None;
        for _ in 0..MAX_SKIP {
            if self.pending.is_none() && !self.video_done {
                match self.src.video()? {
                    Some(f) => self.pending = Some(f),
                    None => self.video_done = true,
                }
            }
            match self.pending.take() {
                Some(f) if f.pts_us <= pos => due = Some(f),
                other => {
                    self.pending = other;
                    break;
                }
            }
        }
        if let Some(f) = due {
            self.publish(&f);
        }
        Ok(self.pending.as_ref().map(|f| Duration::from_micros(f.pts_us.saturating_sub(pos))))
    }

    /// Tout est lu et montré.
    fn finished(&self, pos: u64) -> bool {
        let video = self.video_done && self.pending.is_none();
        let audio = self.audio_done && self.ready.is_none();
        if !(video && audio) {
            return false;
        }
        // Le son poussé finit de sortir ; sans sortie, la durée fait foi.
        match self.info.duration_us {
            0 => self.shared.audio.buffered() == 0 || !self.listened(),
            d => pos >= d,
        }
    }
}

/// Pas de l'horloge quand rien d'autre ne presse (son seul, attente de la sortie).
const TICK: Duration = Duration::from_millis(10);

fn run(src: Box<dyn Source>, shared: &Shared, rx: &mpsc::Receiver<Cmd>) {
    let mut info = src.info();
    let gif = shared.gif.load(Ordering::Relaxed);
    if gif {
        // Sans son : la piste audio, s'il y en a une, n'est jamais lue.
        info.audio = false;
    }
    shared.lock().info = info;
    let mut p = Player {
        src,
        shared,
        info,
        clock: Clock { base_us: 0, since: None },
        playing: false,
        pending: None,
        video_done: info.video.is_none(),
        audio_done: !info.audio,
        ready: None,
        to_output: ToOutput { rs: None },
        mixes: (0, None),
        scaler: convert::Scaler::new(),
        full: Vec::new(),
    };
    // Le son part avec la sortie : on l'attend un instant (elle s'ouvre
    // pendant le téléchargement ; sans elle, la lecture se fait en silence).
    let t0 = Instant::now();
    while info.audio && shared.audio.mixes() == 0 && t0.elapsed() < OUTPUT_WAIT && !shared.closed.load(Ordering::Relaxed) {
        std::thread::sleep(Duration::from_millis(5));
    }
    p.set_playing(true);
    let mut wait = Duration::ZERO;
    loop {
        // En pause ou à la fin : le fil dort jusqu'à la prochaine commande.
        let cmd = if p.playing {
            match rx.recv_timeout(wait) {
                Ok(c) => Some(c),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => return,
            }
        } else {
            match rx.recv() {
                Ok(c) => Some(c),
                Err(_) => return,
            }
        };
        if shared.closed.load(Ordering::Relaxed) {
            return;
        }
        let ended = shared.lock().phase == Phase::Ended;
        let done = match cmd {
            Some(Cmd::Close) => return,
            Some(Cmd::Play) if ended => p.seek(0).map(|()| p.set_playing(true)),
            Some(Cmd::Play) => {
                p.set_playing(true);
                Ok(())
            }
            Some(Cmd::Pause) => {
                if !ended {
                    p.set_playing(false);
                }
                Ok(())
            }
            Some(Cmd::Seek(us)) => p.seek(us).map(|()| {
                if ended {
                    p.set_playing(false);
                }
            }),
            None => Ok(()),
        };
        if let Err(e) = done {
            return p.fail(e);
        }
        if !p.playing {
            continue;
        }
        let pos = p.clock.pos();
        shared.pos_us.store(if info.duration_us > 0 { pos.min(info.duration_us) } else { pos }, Ordering::Relaxed);
        let next = match p.feed_audio(pos).and_then(|()| p.feed_video(pos)) {
            Ok(n) => n,
            Err(e) => return p.fail(e),
        };
        if p.finished(pos) && gif {
            // En boucle : retour à la première image, l'horloge repart de zéro.
            if let Err(e) = p.seek(0) {
                return p.fail(e);
            }
            wait = Duration::ZERO;
            continue;
        }
        if p.finished(pos) {
            p.playing = false;
            p.clock.run(false);
            shared.audio.set_paused(true);
            shared.set_phase(Phase::Ended);
            continue;
        }
        wait = next.map_or(TICK, |d| d.min(TICK));
    }
}

// ---------------------------------------------------------------- sources

/// Son entièrement décodé (MP3 par `symphonia`, Ogg/Opus par libopus) : la
/// source d'un fichier audio que `audio::clip` sait lire.
pub struct PcmSource {
    pcm: crate::audio::clip::Pcm,
    /// Trame suivante.
    at: usize,
}

/// Trames rendues à chaque appel (100 ms).
const PCM_CHUNK: usize = RATE as usize / 10;

impl PcmSource {
    pub fn new(pcm: crate::audio::clip::Pcm) -> Self {
        PcmSource { pcm, at: 0 }
    }
}

impl Source for PcmSource {
    fn info(&self) -> Info {
        Info { duration_us: self.pcm.frames() as u64 * 1_000_000 / u64::from(RATE), audio: true, ..Info::default() }
    }

    fn video(&mut self) -> Result<Option<Frame>, String> {
        Ok(None)
    }

    fn audio(&mut self) -> Result<Option<Chunk>, String> {
        let end = (self.at + PCM_CHUNK).min(self.pcm.frames());
        if self.at >= end {
            return Ok(None);
        }
        let samples = (self.at..end).flat_map(|i| self.pcm.stereo(i)).collect();
        let pts_us = self.at as u64 * 1_000_000 / u64::from(RATE);
        self.at = end;
        Ok(Some(Chunk { pts_us, rate: RATE, channels: 2, samples }))
    }

    fn seek(&mut self, us: u64) -> Result<(), String> {
        self.at = ((us * u64::from(RATE) / 1_000_000) as usize).min(self.pcm.frames());
        Ok(())
    }
}

/// Ouvre un fichier téléchargé : Ogg/Opus et MP3 par `audio::clip`, le reste
/// par le lecteur de fichiers de Windows. `mime` : type annoncé par Discord.
pub fn open(data: Vec<u8>, mime: Option<&str>) -> Result<Box<dyn Source>, OpenError> {
    let mp3 = mime.is_some_and(|m| m.starts_with("audio/mpeg") || m.starts_with("audio/mp3")) || data.starts_with(b"ID3");
    if data.starts_with(b"OggS") || mp3 {
        // Fichier d'un tiers : une panique du décodeur est contenue, comme
        // pour le tableau de sons (`voice::clips`).
        return match crate::panics::contain(|| crate::audio::clip::decode(data)) {
            Some(Ok(pcm)) => Ok(Box::new(PcmSource::new(pcm))),
            Some(Err(_)) | None => Err(OpenError::Unsupported),
        };
    }
    if data.starts_with(&crate::video::mkv::MAGIC) {
        return webm::WebmSource::open(data).map(|s| Box::new(s) as Box<dyn Source>);
    }
    system_open(&data, mime)
}

/// Pourquoi un fichier ne s'ouvre pas.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenError {
    /// Format ou codec que rien ici ne lit.
    Unsupported,
    /// Aucun décodeur du système (hors Windows, ou Media Foundation absent).
    NoDecoder,
}

#[cfg(windows)]
fn system_open(data: &[u8], mime: Option<&str>) -> Result<Box<dyn Source>, OpenError> {
    match crate::video::codec::FileReader::open(data, mime) {
        Ok(r) => Ok(Box::new(r)),
        Err(crate::video::codec::CodecError::Internal(_)) if !crate::video::codec::FileReader::available() => Err(OpenError::NoDecoder),
        Err(_) => Err(OpenError::Unsupported),
    }
}

#[cfg(not(windows))]
fn system_open(_: &[u8], _: Option<&str>) -> Result<Box<dyn Source>, OpenError> {
    Err(OpenError::NoDecoder)
}

#[cfg(test)]
mod tests;
