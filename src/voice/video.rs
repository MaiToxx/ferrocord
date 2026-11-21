//! Vidéo d'un appel et des directs (lot M5) : capture → mise à la taille →
//! encodeur → connexion vocale, et images reçues → décodeur → tuile.
//!
//! Chaque encodeur et chaque décodeur tourne sur son propre fil (Media
//! Foundation ne doit jamais tourner sur le fil d'interface, et un codec
//! lent ne retarde ni l'audio ni la boucle de l'appel). Ces fils n'existent
//! que pendant la vidéo : caméra ou partage allumés, ou une personne qui
//! envoie de la vidéo. Hors vidéo, rien ne tourne.
//!
//! Capture et codecs passent par [`VideoBackend`] : Windows en vrai, une
//! fausse source dans les tests.

use super::AudioBackend;
use crate::i18n::table::share::{f as sf, t as st};
use crate::i18n::table::video::{f as vf, t as vt};
use crate::net::voice::{VoiceConfig, VoiceCtl};
use crate::state::video::{Tile, VideoSink};
use crate::state::video_link::{SendStats, VideoUp};
use crate::video::capture::{self, CaptureError, CaptureHandle, Quality, Source, SourceKind};
use crate::video::codec::{self, CodecError, Decoder, Encoder};
use crate::video::{Codec, EncodedFrame, Frame, FrameData};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{mpsc, Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};
use tokio::sync::mpsc as tmpsc;

/// Capture en cours ; elle s'arrête quand on la lâche.
pub trait Capturing: Send {
    /// La source a disparu : plus aucune image ne viendra.
    fn ended(&self) -> bool;
}

impl Capturing for CaptureHandle {
    fn ended(&self) -> bool {
        CaptureHandle::ended(self)
    }
}

/// Qui capture et qui code : Windows en vrai, des faux dans les tests.
pub trait VideoBackend: Send + Sync {
    fn cameras(&self) -> Vec<Source>;
    /// `on_frame` est appelé sur le fil de capture.
    fn open(&self, source: &Source, want: Quality, on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<Box<dyn Capturing>, CaptureError>;
    /// `None` : pas d'encodeur pour cette définition (carte graphique qui refuse).
    fn encoder(&self, codec: Codec, width: u32, height: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>>;
    /// Encodeur de repli quand celui d'[`encoder`](Self::encoder) s'ouvre
    /// mais échoue à l'envoi (encodeur de la carte graphique occupé ou en
    /// panne) ; `None` : pas de repli.
    fn fallback_encoder(&self, _codec: Codec, _width: u32, _height: u32, _fps: u32, _bps: u32) -> Option<Box<dyn Encoder>> {
        None
    }
    fn decoder(&self, codec: Codec) -> Option<Box<dyn Decoder>>;
    /// Codecs (encodables, décodables) à proposer à la passerelle vocale.
    fn codecs(&self) -> (Vec<Codec>, Vec<Codec>) {
        (vec![Codec::H264], vec![Codec::H264])
    }
    /// Son d'une source partagée, en lieu et place du micro.
    fn share_sound(&self, source: &Source) -> Option<Arc<dyn AudioBackend>>;
}

/// Caméras, écrans et fenêtres de Windows ; codecs de `video::codec`.
pub struct WindowsVideo;

impl VideoBackend for WindowsVideo {
    fn cameras(&self) -> Vec<Source> {
        capture::cameras()
    }
    fn open(&self, source: &Source, want: Quality, on_frame: Box<dyn FnMut(Frame) + Send>) -> Result<Box<dyn Capturing>, CaptureError> {
        capture::open(source, want, on_frame).map(|h| Box::new(h) as Box<dyn Capturing>)
    }
    fn encoder(&self, codec: Codec, width: u32, height: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
        codec::encoder(codec, width, height, fps, bps)
    }
    fn fallback_encoder(&self, codec: Codec, width: u32, height: u32, fps: u32, bps: u32) -> Option<Box<dyn Encoder>> {
        codec::fallback_encoder(codec, width, height, fps, bps)
    }
    fn decoder(&self, codec: Codec) -> Option<Box<dyn Decoder>> {
        codec::decoder(codec)
    }
    fn codecs(&self) -> (Vec<Codec>, Vec<Codec>) {
        (codec::supported_encoders(), codec::supported_decoders())
    }
    fn share_sound(&self, source: &Source) -> Option<Arc<dyn AudioBackend>> {
        capture::sound::ShareSound::for_source(source).map(|s| Arc::new(s) as Arc<dyn AudioBackend>)
    }
}

/// Réglage « Recevoir la vidéo en VP9 et AV1 » (`[voix] codecs_vp9_av1`),
/// lu à l'ouverture de chaque connexion vocale.
static EXTRA_CODECS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Windows décode VP9 ou AV1 sur cet ordinateur (premier appel : une
/// énumération des décodeurs, quelques millisecondes).
pub fn extra_codecs_available() -> bool {
    codec::supported_decoders().iter().any(|c| matches!(c, Codec::Vp9 | Codec::Av1))
}

/// Pose le réglage ; il vaut pour les appels et les directs ouverts ensuite.
pub fn set_extra_codecs(on: bool) {
    EXTRA_CODECS.store(on, Relaxed);
}

/// Réglages de la connexion vocale : les codecs vidéo proposés sont ceux du
/// moteur vidéo (VP8 quand Windows en a un ; VP9 et AV1 en réception si le
/// réglage le demande) ; une liste vide reste « pas de vidéo ».
pub(super) fn voice_config(opts: &super::Options) -> VoiceConfig {
    let mut c = opts.voice.clone();
    if !c.video_codecs.is_empty() {
        let (enc, mut dec) = opts.video.codecs();
        if !EXTRA_CODECS.load(Relaxed) {
            dec.retain(|c| !matches!(c, Codec::Vp9 | Codec::Av1));
        }
        c.video_codecs = crate::net::voice::video::offers(&enc, &dec);
    }
    c
}

/// Vers l'interface (et son réveil), depuis n'importe quel fil.
pub(super) type Report = Arc<dyn Fn(VideoUp) + Send + Sync>;

/// Source d'après son identifiant (`screen:`, `window:`, `camera:`).
pub fn source_of(id: &str) -> Option<Source> {
    let kind = [("screen:", SourceKind::Screen), ("window:", SourceKind::Window), ("camera:", SourceKind::Camera)].into_iter().find(|(p, _)| id.starts_with(p))?.1;
    Some(Source { id: id.to_string(), name: String::new(), kind })
}

/// Texte d'une erreur de capture, dans la langue de l'interface.
pub(super) fn capture_text(e: &CaptureError) -> String {
    let lang = crate::i18n::language();
    match e {
        CaptureError::CameraDenied => st::ERR_CAMERA_PRIVACY.get(lang).to_string(),
        CaptureError::NotFound => st::ERR_GONE.get(lang).to_string(),
        CaptureError::Unsupported => st::ERR_UNSUPPORTED.get(lang).to_string(),
        CaptureError::Backend(m) => sf::capture_error(lang, m),
    }
}

/// Définitions essayées, de la plus grande à la plus petite : quand la carte
/// graphique en refuse une, on passe à la suivante.
const LEVELS: [(u32, u32); 7] = [(3840, 2160), (2560, 1440), (1920, 1080), (1280, 720), (960, 540), (640, 360), (320, 180)];

/// Palier de départ pour la qualité demandée (« Source » : le plus grand).
fn first_level(q: Quality) -> usize {
    if q.is_source() {
        return 0;
    }
    LEVELS.iter().position(|(w, h)| *w <= q.width.max(320) && *h <= q.height.max(180)).unwrap_or(LEVELS.len() - 1)
}

/// Taille d'encodage d'une image `w`×`h` au palier `level` : jamais agrandie, côtés pairs.
fn level_size(w: u32, h: u32, level: usize) -> (u32, u32) {
    let (lw, lh) = LEVELS[level.min(LEVELS.len() - 1)];
    capture::fit(w, h, Quality { width: lw, height: lh, fps: 30 })
}

/// Débit visé : environ 0,08 bit par pixel pour un écran (texte net),
/// 0,05 pour une caméra, borné.
pub fn bitrate(width: u32, height: u32, fps: u32, screen: bool) -> u32 {
    let px = u64::from(width) * u64::from(height) * u64::from(fps.max(1));
    // Partage au-delà de 30 images/s : le plafond monte, sinon chaque image
    // aurait moitié moins de bits. Il monte aussi avec la définition : à
    // 8 Mb/s, un partage en 4K n'avait qu'un quart des bits par pixel d'un
    // partage en 1080p et sortait très pixélisé (constaté par l'utilisateur,
    // 0.1.25). Jamais au-delà de ce qui est annoncé à Discord (`max_bitrate`).
    let area = u64::from(width) * u64::from(height);
    let screen_max = match (area > 2560 * 1440, area > 1920 * 1080, fps > 30) {
        (true, _, true) => 18_000_000,
        (true, _, false) => 14_000_000,
        (false, true, true) => 12_000_000,
        (false, true, false) => 10_000_000,
        (false, false, true) => 8_000_000,
        (false, false, false) => 6_000_000,
    };
    let (num, max) = if screen { (8, screen_max) } else { (5, 2_500_000) };
    (px * num / 100).clamp(150_000, max) as u32
}

/// Débit à régler sur l'encodeur pour qu'il produise vraiment `base` quand
/// la chaîne ne tient que `real` images par seconde au lieu de `fps`.
/// L'encodeur répartit son débit sur `fps` images : à 30 images réelles
/// pour 60 annoncées, un partage en 4K ne sortait que la moitié du débit
/// visé, d'où une image pixélisée (constaté par l'utilisateur, 0.1.29).
/// Seulement quand la chaîne est en retard (`behind`) : un écran immobile
/// donne peu d'images sans manquer de débit. Jamais plus du double.
fn tuned_bitrate(base: u32, fps: u32, real: u32, behind: bool) -> u32 {
    if !behind || real == 0 || real >= fps {
        return base;
    }
    (u64::from(base) * u64::from(fps) / u64::from(real)).min(u64::from(base) * 2) as u32
}

/// Image mise à `w`×`h` (plus proche voisin pour l'I420, bilinéaire pour le BGRA).
fn scaled(f: Frame, w: u32, h: u32, scaler: &mut crate::video::convert::Scaler) -> Option<Frame> {
    if (f.width, f.height) == (w, h) {
        return Some(f);
    }
    let (sw, sh, dw, dh) = (f.width as usize, f.height as usize, w as usize, h as usize);
    let data = match &f.data {
        FrameData::Bgra(b) => {
            let mut out = Vec::new();
            scaler.scale_rgba(b, sw, sh, &mut out, dw, dh).ok()?;
            FrameData::Bgra(out)
        }
        FrameData::I420 { y, u, v } => {
            let plane = |src: &[u8], sw: usize, sh: usize, dw: usize, dh: usize| -> Option<Vec<u8>> {
                if src.len() < sw * sh {
                    return None;
                }
                Some((0..dh).flat_map(|oy| (0..dw).map(move |ox| (oy * sh / dh) * sw + ox * sw / dw)).map(|i| src[i]).collect())
            };
            let (scw, sch, dcw, dch) = (sw.div_ceil(2), sh.div_ceil(2), dw.div_ceil(2), dh.div_ceil(2));
            FrameData::I420 { y: plane(y, sw, sh, dw, dh)?, u: plane(u, scw, sch, dcw, dch)?, v: plane(v, scw, sch, dcw, dch)? }
        }
    };
    Some(Frame { width: w, height: h, pts_us: f.pts_us, data })
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Boîte aux lettres entre la capture et l'encodeur : seule la dernière
/// image compte (un encodeur en retard saute des images, il ne les empile pas).
#[derive(Default)]
struct Slot {
    frame: Option<Frame>,
    keyframe: bool,
    /// Nouveau codec d'envoi négocié, pas encore pris par l'encodeur.
    codec: Option<Codec>,
    stop: bool,
}

#[derive(Default)]
struct Mailbox {
    slot: Mutex<Slot>,
    ready: Condvar,
}

/// Ce que montre et envoie une vidéo à émettre.
pub(super) struct SendCfg {
    pub backend: Arc<dyn VideoBackend>,
    /// `None` : la première caméra.
    pub source: Option<String>,
    pub quality: Quality,
    /// Partage d'écran (débit plus élevé, op 12 de type `screen` côté connexion).
    pub screen: bool,
    /// Codec négocié (H.264 tant que la connexion n'a rien dit d'autre).
    pub codec: Codec,
    /// Tuile de l'aperçu local.
    pub tile: Tile,
    pub sink: VideoSink,
    pub net: tmpsc::Sender<VoiceCtl>,
    pub report: Report,
    /// Comment annoncer un échec qui arrête tout (caméra éteinte, partage arrêté).
    pub fail: fn(String) -> VideoUp,
}

/// Une vidéo émise (caméra ou partage) ; tout s'arrête quand on la lâche.
pub(super) struct Sending {
    mail: Arc<Mailbox>,
}

impl Sending {
    pub fn start(cfg: SendCfg) -> Sending {
        let mail = Arc::new(Mailbox { slot: Mutex::new(Slot { keyframe: true, ..Default::default() }), ready: Condvar::new() });
        let m = mail.clone();
        let report = cfg.report.clone();
        let fail = cfg.fail;
        let spawned = std::thread::Builder::new().name("ferrocord-video-envoi".into()).spawn(move || send_thread(cfg, m));
        if let Err(e) = spawned {
            report(fail(vf::video_error(crate::i18n::language(), e)));
        }
        Sending { mail }
    }

    /// Image clé demandée (PLI, FIR) : la prochaine image en sera une.
    pub fn keyframe(&self) {
        lock(&self.mail.slot).keyframe = true;
    }

    /// La connexion a négocié un autre codec : l'encodeur change à l'image suivante.
    pub fn set_codec(&self, codec: Codec) {
        lock(&self.mail.slot).codec = Some(codec);
    }
}

impl Drop for Sending {
    fn drop(&mut self) {
        lock(&self.mail.slot).stop = true;
        self.mail.ready.notify_all();
    }
}

/// Encodeur en service et le palier où il tient.
struct Enc {
    codec: Codec,
    level: usize,
    size: (u32, u32),
    enc: Option<Box<dyn Encoder>>,
    /// L'encodeur ordinaire a échoué à l'envoi : le repli logiciel sert
    /// jusqu'à la fin de cette vidéo.
    fallback: bool,
    /// Images données de suite à l'encodeur sans qu'il rende rien.
    dry: u32,
    /// Pannes de suite de l'encodeur ordinaire, avant le repli.
    errors: u32,
    /// Palier déjà annoncé à l'écran : une réduction après une panne à
    /// l'envoi se dit comme une définition refusée à l'ouverture.
    told: usize,
}

/// Pannes de suite de l'encodeur ordinaire avant le repli logiciel : à la
/// première, il est rouvert (une panne passagère de la carte graphique ne
/// condamne plus tout le partage à l'encodeur logiciel, cinq à dix fois
/// plus lent).
const HARD_ERRORS: u32 = 2;

/// Palier le plus grand tenu par l'encodeur logiciel : 1280×720. Mesuré
/// sur la machine de l'utilisateur (0.1.24) : 44 ms par image en 1080p,
/// 61 ms en 1440p, soit 16 à 22 images par seconde, contre 6 à 11 ms pour
/// l'encodeur de la carte graphique.
const SOFT_LEVEL: usize = 3;

/// Intervalle entre deux mesures de ce que le partage envoie.
const STATS_EVERY_MS: u64 = 2000;

/// Images sans sortie avant de tenir l'encodeur ordinaire pour muet : il
/// s'ouvre et accepte les images mais ne rend rien, et ceux qui regardent
/// voient le direct charger sans fin (constaté par l'utilisateur, 0.1.12).
/// Un encodeur qui saute des images pour tenir son débit n'en saute jamais
/// autant de suite.
const DRY_FRAMES: u32 = 90;

fn send_thread(cfg: SendCfg, mail: Arc<Mailbox>) {
    let lang = crate::i18n::language();
    let fail = |why: String| (cfg.report)((cfg.fail)(why));
    let source = match &cfg.source {
        // Caméra choisie débranchée : la première de Windows la remplace.
        Some(id) => source_of(id).map(|s| {
            let list = if s.kind == SourceKind::Camera { cfg.backend.cameras() } else { Vec::new() };
            if list.iter().any(|c| c.id == s.id) { s } else { list.into_iter().next().unwrap_or(s) }
        }),
        None => cfg.backend.cameras().into_iter().next(),
    };
    let Some(source) = source else {
        fail(vt::NO_CAMERA.get(lang).to_string());
        return;
    };
    let m = mail.clone();
    // Images de la capture remplacées avant d'avoir été encodées : la chaîne est en retard.
    let skipped = Arc::new(std::sync::atomic::AtomicU32::new(0));
    let sk = skipped.clone();
    let on_frame = Box::new(move |f: Frame| {
        let mut s = lock(&m.slot);
        if !s.stop {
            if s.frame.replace(f).is_some() {
                sk.fetch_add(1, Relaxed);
            }
            drop(s);
            m.ready.notify_one();
        }
    });
    if lock(&mail.slot).stop {
        return;
    }
    let capture = match cfg.backend.open(&source, cfg.quality, on_frame) {
        Ok(c) => c,
        Err(e) => {
            fail(capture_text(&e));
            return;
        }
    };
    let fps = cfg.quality.fps.clamp(1, 60);
    let mut enc = Enc { codec: cfg.codec, level: first_level(cfg.quality), size: (0, 0), enc: None, fallback: false, dry: 0, errors: 0, told: first_level(cfg.quality) };
    let mut scaler = crate::video::convert::Scaler::new();
    let mut announced = None;
    // Mesure de ce qui part vraiment : (début, images, octets, images perdues).
    let mut seen = (Instant::now(), 0u32, 0u64, 0u32);
    // Débit réglé sur l'encodeur en service (`None` : celui de son ouverture).
    let mut tuned: Option<u32> = None;
    loop {
        // Une image, ou l'arrêt ; sans image depuis une seconde, la source a peut-être disparu.
        let (frame, mut key) = {
            let mut s = lock(&mail.slot);
            loop {
                if s.stop {
                    return;
                }
                if let Some(f) = s.frame.take() {
                    if let Some(c) = s.codec.take().filter(|c| *c != enc.codec) {
                        enc = Enc { codec: c, level: first_level(cfg.quality), size: (0, 0), enc: None, fallback: false, dry: 0, errors: 0, told: first_level(cfg.quality) };
                    }
                    break (f, std::mem::take(&mut s.keyframe));
                }
                let (g, t) = mail.ready.wait_timeout(s, Duration::from_secs(1)).unwrap_or_else(PoisonError::into_inner);
                s = g;
                if t.timed_out() && !s.stop && capture.ended() {
                    drop(s);
                    fail(vt::SOURCE_ENDED.get(lang).to_string());
                    return;
                }
            }
        };
        let mut size = level_size(frame.width, frame.height, enc.level);
        if enc.enc.is_none() || size != enc.size {
            loop {
                let bps = bitrate(size.0, size.1, fps, cfg.screen);
                let made = if enc.fallback { cfg.backend.fallback_encoder(enc.codec, size.0, size.1, fps, bps) } else { None };
                if let Some(e) = made.or_else(|| cfg.backend.encoder(enc.codec, size.0, size.1, fps, bps)) {
                    enc.enc = Some(e);
                    enc.size = size;
                    tuned = None;
                    break;
                }
                // Refusé : palier suivant, s'il réduit vraiment l'image.
                let Some(next) = (enc.level + 1..LEVELS.len()).find(|l| level_size(frame.width, frame.height, *l) != size) else {
                    fail(vt::NO_ENCODER.get(lang).to_string());
                    return;
                };
                enc.level = next;
                size = level_size(frame.width, frame.height, next);
            }
            key = true;
        }
        let Some(img) = scaled(frame, size.0, size.1, &mut scaler) else { continue };
        if announced != Some(size) {
            announced = Some(size);
            if cfg.net.blocking_send(VoiceCtl::VideoState { active: true, width: size.0, height: size.1, fps }).is_err() {
                return;
            }
        }
        let Some(e) = enc.enc.as_mut() else { continue };
        let software = e.software();
        match e.encode(&img, key) {
            Ok(list) => {
                enc.dry = if list.is_empty() { enc.dry + 1 } else { 0 };
                if !list.is_empty() {
                    enc.errors = 0;
                    // Définition réduite (refusée à l'ouverture, ou en panne à
                    // l'envoi) : dite une fois que l'encodeur y tient.
                    if enc.level != enc.told {
                        enc.told = enc.level;
                        (cfg.report)(VideoUp::Problem(vf::lowered(lang, size.0, size.1)));
                    }
                }
                if enc.dry >= DRY_FRAMES && !enc.fallback && cfg.backend.fallback_encoder(enc.codec, size.0, size.1, fps, 1).is_some() {
                    enc.enc = None;
                    enc.fallback = true;
                    enc.level = enc.level.max(SOFT_LEVEL);
                    enc.dry = 0;
                    lock(&mail.slot).keyframe = true;
                    (cfg.report)(VideoUp::Problem(vf::soft_encoder(lang, vt::ENCODER_MUTE.get(lang))));
                    continue;
                }
                for f in list {
                    let bytes = f.data.len() as u64;
                    match cfg.net.try_send(VoiceCtl::Video(f)) {
                        Ok(()) => (seen.1, seen.2) = (seen.1 + 1, seen.2 + bytes),
                        // Connexion en retard : image perdue, la suivante sera une image clé.
                        Err(tmpsc::error::TrySendError::Full(_)) => {
                            seen.3 += 1;
                            lock(&mail.slot).keyframe = true;
                        }
                        Err(tmpsc::error::TrySendError::Closed(_)) => return,
                    }
                }
                let ms = seen.0.elapsed().as_millis() as u64;
                if ms >= STATS_EVERY_MS {
                    let real = ((u64::from(seen.1) * 1000 + ms / 2) / ms) as u32;
                    let behind = skipped.swap(0, Relaxed) > seen.1 / 8;
                    let base = bitrate(size.0, size.1, fps, cfg.screen);
                    let want = tuned_bitrate(base, fps, real, behind);
                    if tuned.unwrap_or(base).abs_diff(want) > base / 10 {
                        tuned = Some(want);
                        e.set_bitrate(want);
                    }
                    if cfg.screen {
                        let st = SendStats {
                            width: size.0,
                            height: size.1,
                            fps: ((u64::from(seen.1) * 1000 + ms / 2) / ms) as u32,
                            kbps: (seen.2 * 8 / ms) as u32,
                            software,
                            lost: seen.3,
                        };
                        (cfg.report)(VideoUp::ShareStats(st));
                    }
                    seen = (Instant::now(), 0, 0, 0);
                }
            }
            Err(CodecError::BadFrame(_)) => continue,
            Err(err) => {
                enc.enc = None;
                // Première panne à l'envoi (constaté : « Media Foundation :
                // 0x8000ffff » de l'encodeur de la carte graphique, qui
                // s'ouvrait pourtant) : même définition, encodeur logiciel.
                if !enc.fallback {
                    enc.errors += 1;
                    // Première panne : l'encodeur ordinaire est rouvert tel quel.
                    if enc.errors < HARD_ERRORS {
                        lock(&mail.slot).keyframe = true;
                        continue;
                    }
                    // Il lâche encore : la même carte graphique à la définition
                    // en dessous (1440p, 1080p) avant l'encodeur logiciel, qui
                    // ne tient que 720p (un partage en 4K tombait d'un coup à
                    // 720p, très flou chez ceux qui regardent).
                    let smaller = (enc.level + 1..SOFT_LEVEL).find(|l| level_size(img.width, img.height, *l) != size);
                    if let Some(next) = smaller {
                        enc.level = next;
                        enc.errors = 0;
                        lock(&mail.slot).keyframe = true;
                        continue;
                    }
                    if cfg.backend.fallback_encoder(enc.codec, size.0, size.1, fps, 1).is_some() {
                        enc.fallback = true;
                        // L'encodeur logiciel ne tient pas la cadence au-delà de 720p :
                        // image réduite (et annoncée) plutôt que saccadée.
                        enc.level = enc.level.max(SOFT_LEVEL);
                        lock(&mail.slot).keyframe = true;
                        (cfg.report)(VideoUp::Problem(vf::soft_encoder(lang, err)));
                        continue;
                    }
                }
                // L'encodeur lâche à cette définition : palier suivant au prochain tour.
                match (enc.level + 1..LEVELS.len()).find(|l| level_size(img.width, img.height, *l) != size) {
                    Some(next) => enc.level = next,
                    None => {
                        fail(vf::video_error(lang, err));
                        return;
                    }
                }
                continue;
            }
        }
        cfg.sink.push(cfg.tile.clone(), img);
    }
}

/// Un décodeur, sur son fil.
struct Dec {
    tx: mpsc::SyncSender<EncodedFrame>,
    /// Image jetée faute de place : le décodeur doit repartir d'une image clé.
    gap: Arc<AtomicBool>,
}

/// Images reçues : un décodeur par personne (ou par direct), sur son fil.
pub(super) struct Receiving {
    backend: Arc<dyn VideoBackend>,
    sink: VideoSink,
    net: tmpsc::Sender<VoiceCtl>,
    report: Report,
    users: HashMap<u64, Dec>,
}

/// Images en attente par décodeur : au-delà, elles sont jetées.
const DECODE_QUEUE: usize = 8;
/// Une demande d'image clé au plus par demi-seconde et par personne.
const KEYFRAME_EVERY: Duration = Duration::from_millis(500);

impl Receiving {
    pub fn new(backend: Arc<dyn VideoBackend>, sink: VideoSink, net: tmpsc::Sender<VoiceCtl>, report: Report) -> Self {
        Receiving { backend, sink, net, report, users: HashMap::new() }
    }

    /// Image de `user`, pour la tuile `tile`.
    pub fn frame(&mut self, user: u64, tile: Tile, f: EncodedFrame) {
        if !self.users.contains_key(&user) {
            let (tx, rx) = mpsc::sync_channel(DECODE_QUEUE);
            let gap = Arc::new(AtomicBool::new(false));
            let job = DecodeJob { user, tile, rx, gap: gap.clone(), backend: self.backend.clone(), sink: self.sink.clone(), net: self.net.clone(), report: self.report.clone() };
            if std::thread::Builder::new().name("ferrocord-video-decodage".into()).spawn(move || decode_thread(job)).is_err() {
                return;
            }
            self.users.insert(user, Dec { tx, gap });
        }
        let Some(d) = self.users.get(&user) else { return };
        match d.tx.try_send(f) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => d.gap.store(true, Relaxed),
            Err(mpsc::TrySendError::Disconnected(_)) => {
                self.users.remove(&user);
            }
        }
    }

    /// Vidéo coupée ou personne partie : son fil s'arrête.
    pub fn stop(&mut self, user: u64) {
        self.users.remove(&user);
    }
}

struct DecodeJob {
    user: u64,
    tile: Tile,
    rx: mpsc::Receiver<EncodedFrame>,
    gap: Arc<AtomicBool>,
    backend: Arc<dyn VideoBackend>,
    sink: VideoSink,
    net: tmpsc::Sender<VoiceCtl>,
    report: Report,
}

fn decode_thread(j: DecodeJob) {
    let mut dec: Option<(Codec, Box<dyn Decoder>)> = None;
    let mut need_key = true;
    let mut asked: Option<Instant> = None;
    let mut told = false;
    let ask = |asked: &mut Option<Instant>| {
        if asked.is_none_or(|t| t.elapsed() >= KEYFRAME_EVERY) {
            *asked = Some(Instant::now());
            let _ = j.net.try_send(VoiceCtl::RequestKeyframe(j.user));
        }
    };
    while let Ok(f) = j.rx.recv() {
        // Tuile hors champ : rien n'est décodé ; il faudra une image clé au retour.
        if !j.sink.is_visible(&j.tile) || j.gap.swap(false, Relaxed) {
            need_key = true;
            continue;
        }
        if need_key && !f.keyframe {
            ask(&mut asked);
            continue;
        }
        if dec.as_ref().is_none_or(|(c, _)| *c != f.codec) {
            dec = j.backend.decoder(f.codec).map(|d| (f.codec, d));
        }
        let Some((_, d)) = dec.as_mut() else {
            if !std::mem::replace(&mut told, true) {
                (j.report)(VideoUp::Problem(vt::NO_DECODER.get(crate::i18n::language()).to_string()));
            }
            continue;
        };
        match d.decode(&f) {
            Ok(Some(img)) => {
                need_key = false;
                j.sink.push(j.tile.clone(), img);
            }
            Ok(None) => need_key = false,
            Err(CodecError::Corrupt(_)) => {
                need_key = true;
                ask(&mut asked);
            }
            Err(_) => {
                // Décodeur en panne : un neuf repartira de la prochaine image clé.
                dec = None;
                need_key = true;
                ask(&mut asked);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Feed = Box<dyn FnMut(Frame) + Send>;

    /// Fausse carte graphique : son encodeur échoue au-delà de `hard_max`
    /// pixels de large ; chaque essai d'encodage est noté (largeur, logiciel).
    struct Fake {
        hard_max: u32,
        feed: Mutex<Option<Feed>>,
        tries: Arc<Mutex<Vec<(u32, bool)>>>,
    }

    struct Alive;

    impl Capturing for Alive {
        fn ended(&self) -> bool {
            false
        }
    }

    struct FakeEnc {
        fail: bool,
        soft: bool,
        tries: Arc<Mutex<Vec<(u32, bool)>>>,
    }

    impl Encoder for FakeEnc {
        fn encode(&mut self, f: &Frame, key: bool) -> Result<Vec<EncodedFrame>, CodecError> {
            lock(&self.tries).push((f.width, self.soft));
            if self.fail {
                return Err(CodecError::Internal("panne d'essai".into()));
            }
            Ok(vec![EncodedFrame { codec: Codec::H264, keyframe: key, ts90k: 0, data: vec![0; 100] }])
        }
        fn set_bitrate(&mut self, _: u32) {}
        fn software(&self) -> bool {
            self.soft
        }
    }

    impl VideoBackend for Fake {
        fn cameras(&self) -> Vec<Source> {
            Vec::new()
        }
        fn open(&self, _: &Source, _: Quality, on_frame: Feed) -> Result<Box<dyn Capturing>, CaptureError> {
            *lock(&self.feed) = Some(on_frame);
            Ok(Box::new(Alive))
        }
        fn encoder(&self, _: Codec, w: u32, _: u32, _: u32, _: u32) -> Option<Box<dyn Encoder>> {
            Some(Box::new(FakeEnc { fail: w > self.hard_max, soft: false, tries: self.tries.clone() }))
        }
        fn fallback_encoder(&self, _: Codec, _: u32, _: u32, _: u32, _: u32) -> Option<Box<dyn Encoder>> {
            Some(Box::new(FakeEnc { fail: false, soft: true, tries: self.tries.clone() }))
        }
        fn decoder(&self, _: Codec) -> Option<Box<dyn Decoder>> {
            None
        }
        fn share_sound(&self, _: &Source) -> Option<Arc<dyn AudioBackend>> {
            None
        }
    }

    /// Partage d'un écran 4K en « Source » avec une carte graphique qui ne
    /// tient que jusqu'à `hard_max` : essais d'encodage et messages affichés.
    fn share_4k(hard_max: u32, frames: usize) -> (Vec<(u32, bool)>, Vec<String>) {
        let tries = Arc::new(Mutex::new(Vec::new()));
        let backend = Arc::new(Fake { hard_max, feed: Mutex::new(None), tries: tries.clone() });
        let said = Arc::new(Mutex::new(Vec::new()));
        let s = said.clone();
        let report: Report = Arc::new(move |u| {
            if let VideoUp::Problem(t) = u {
                lock(&s).push(t);
            }
        });
        let (net, mut rx) = tmpsc::channel(256);
        let sending = Sending::start(SendCfg {
            backend: backend.clone(),
            source: Some("screen:1".into()),
            quality: Quality { width: 0, height: 0, fps: 60 },
            screen: true,
            codec: Codec::H264,
            tile: Tile::Stream("guild:1:2:3".into()),
            sink: VideoSink::default(),
            net,
            report,
            fail: VideoUp::ShareFailed,
        });
        let wait = |ok: &dyn Fn() -> bool| {
            let t0 = Instant::now();
            while !ok() {
                assert!(t0.elapsed() < Duration::from_secs(20), "le fil d'envoi ne suit pas");
                std::thread::sleep(Duration::from_millis(2));
            }
        };
        wait(&|| lock(&backend.feed).is_some());
        let (w, h) = (3840usize, 2160usize);
        let frame = Frame { width: w as u32, height: h as u32, pts_us: 0, data: FrameData::I420 { y: vec![0; w * h], u: vec![0; w * h / 4], v: vec![0; w * h / 4] } };
        for n in 1..=frames {
            if let Some(feed) = lock(&backend.feed).as_mut() {
                feed(frame.clone());
            }
            wait(&|| lock(&tries).len() >= n);
        }
        drop(sending);
        while rx.try_recv().is_ok() {}
        (lock(&tries).clone(), lock(&said).clone())
    }

    #[test]
    fn bitrate_follows_the_real_frame_rate_when_the_pipeline_is_behind() {
        let base = 18_000_000;
        assert_eq!(tuned_bitrate(base, 60, 60, true), base);
        assert_eq!(tuned_bitrate(base, 60, 45, true), 24_000_000);
        assert_eq!(tuned_bitrate(base, 60, 30, true), 36_000_000);
        assert_eq!(tuned_bitrate(base, 60, 10, true), 36_000_000, "jamais plus du double");
        // Écran immobile : peu d'images, mais la chaîne n'est pas en retard.
        assert_eq!(tuned_bitrate(base, 60, 5, false), base);
        assert_eq!(tuned_bitrate(base, 60, 0, true), base);
    }

    #[test]
    fn a_failing_graphics_encoder_steps_down_before_software() {
        assert!(Quality { width: 0, height: 0, fps: 60 }.is_source());
        // La carte graphique lâche en 4K et en 1440p mais tient en 1080p : elle
        // sert en 1080p, l'encodeur logiciel (720p) ne sert jamais.
        let (tries, said) = share_4k(1920, 6);
        assert_eq!(tries, vec![(3840, false), (3840, false), (2560, false), (2560, false), (1920, false), (1920, false)]);
        assert_eq!(said, vec![vf::lowered(crate::i18n::language(), 1920, 1080)]);
        // Elle lâche partout : encodeur logiciel en 720p, et le message dit pourquoi.
        let (tries, said) = share_4k(0, 8);
        assert_eq!(tries[..6], [(3840, false), (3840, false), (2560, false), (2560, false), (1920, false), (1920, false)]);
        assert_eq!(tries[6..], [(1280, true), (1280, true)]);
        assert!(said.iter().any(|t| t.contains("panne d'essai")), "{said:?}");
    }

    #[test]
    fn extra_codecs_follow_the_setting() {
        let opts = crate::voice::Options::real();
        let offer = |c: &VoiceConfig, k| c.video_codecs.iter().copied().find(|o| o.codec == k);
        set_extra_codecs(false);
        let off = voice_config(&opts);
        assert!(offer(&off, Codec::H264).is_some());
        assert!(offer(&off, Codec::Vp9).is_none() && offer(&off, Codec::Av1).is_none());
        set_extra_codecs(true);
        let on = voice_config(&opts);
        set_extra_codecs(false);
        let (_, dec) = opts.video.codecs();
        for k in [Codec::Vp9, Codec::Av1] {
            assert_eq!(offer(&on, k).is_some(), dec.contains(&k), "{k:?}");
            // Reçu seulement, jamais envoyé.
            assert!(offer(&on, k).is_none_or(|o| o.decode && !o.encode));
        }
        assert_eq!(extra_codecs_available(), dec.iter().any(|c| matches!(c, Codec::Vp9 | Codec::Av1)));
    }

    #[test]
    fn levels_and_sizes() {
        assert_eq!(LEVELS[first_level(Quality::HD_30)], (1280, 720));
        assert_eq!(LEVELS[first_level(Quality::FHD_60)], (1920, 1080));
        assert_eq!(first_level(Quality::SOURCE), 0);
        assert_eq!(LEVELS[first_level(Quality { width: 10, height: 10, fps: 30 })], (320, 180));
        assert_eq!(level_size(2560, 1440, first_level(Quality::HD_30)), (1280, 720));
        assert_eq!(level_size(640, 480, first_level(Quality::FHD_30)), (640, 480), "jamais agrandie");
        assert_eq!(bitrate(1920, 1080, 30, true), 4_976_640);
        assert_eq!(bitrate(1280, 720, 30, false), 1_382_400);
        // Le plafond d'un partage monte avec la définition et la cadence.
        assert_eq!(bitrate(1920, 1080, 60, true), 8_000_000);
        assert_eq!(bitrate(2560, 1440, 30, true), 8_847_360);
        assert_eq!(bitrate(2560, 1440, 60, true), 12_000_000);
        assert_eq!(bitrate(3840, 2160, 30, true), 14_000_000);
        assert_eq!(bitrate(3840, 2160, 60, true), 18_000_000);
        assert_eq!(bitrate(1920, 1080, 60, false), 2_500_000);
        assert_eq!(bitrate(16, 16, 1, false), 150_000);
        assert_eq!(source_of("window:12").map(|s| s.kind), Some(SourceKind::Window));
        assert_eq!(source_of("camera:\\\\?\\usb").map(|s| s.kind), Some(SourceKind::Camera));
        assert!(source_of("autre:1").is_none());
    }

    #[test]
    fn scaling_keeps_planes_consistent() {
        let mut s = crate::video::convert::Scaler::new();
        let i420 = Frame { width: 8, height: 4, pts_us: 7, data: FrameData::I420 { y: vec![9; 32], u: vec![1; 8], v: vec![2; 8] } };
        let out = scaled(i420, 4, 2, &mut s).unwrap_or_else(|| panic!("i420"));
        assert!(out.is_consistent() && out.pts_us == 7);
        let bgra = crate::state::video::test_pattern(64, 36, 0, 1);
        let out = scaled(bgra, 32, 18, &mut s).unwrap_or_else(|| panic!("bgra"));
        assert!(out.is_consistent());
        let short = Frame { width: 8, height: 4, pts_us: 0, data: FrameData::I420 { y: vec![], u: vec![], v: vec![] } };
        assert!(scaled(short, 4, 2, &mut s).is_none());
    }
}
