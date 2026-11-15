//! Enregistrement local d'un appel (plugin, lot E4) : un fil, né au clic sur
//! « Démarrer l'enregistrement » et fini à l'arrêt, reçoit des deux fils
//! audio les voix décodées et le micro, les mixe et les écrit en Ogg/Opus,
//! page par page (rien ne s'accumule en mémoire).
//!
//! L'horloge est celle de la lecture : à chaque trame de 20 ms mixée par le
//! moteur ([`RecMsg::Tick`]), une trame part dans le fichier mixé (voix
//! reçues + micro) et, avec « une piste par personne », dans chaque piste.
//! Le micro arrive du fil de capture à son propre rythme : il passe par une
//! petite file (40 ms d'avance) qui absorbe l'écart.
//!
//! Hors enregistrement, rien de ceci n'existe : ni fil, ni file, ni tampon.

use crate::audio::clip::ogg::OpusFile;
use crate::audio::engine::{FRAME, MAX_PACKET, SAMPLE_RATE, SILENCE};
use crate::state::call_rec::{file_stem, CallRecFail, CallRecUp};
use opus::{Application, Bitrate, Channels, Encoder};
use std::collections::{HashMap, VecDeque};
use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

/// Retard de l'encodeur à 48 kHz (6,5 ms), à jeter à la lecture.
const PRE_SKIP: u16 = 312;
const MIX_BITRATE: i32 = 64_000;
const TRACK_BITRATE: i32 = 40_000;
/// Avance du micro avant d'en prendre (deux trames), et trop-plein au-delà
/// duquel le plus ancien est jeté (dix trames).
const MIC_LEAD: usize = 2 * FRAME;
const MIC_MAX: usize = 10 * FRAME;
/// Crête du mixage écrit.
const CEILING: f32 = 0.98;

/// Des fils audio (et de l'interface) vers le fil d'écriture.
pub(super) enum RecMsg {
    /// Une trame du micro telle qu'elle est émise (vide : silence, micro
    /// coupé ou porte fermée).
    Mic(Vec<f32>),
    /// Une trame mono décodée d'une personne.
    Peer(u64, Vec<f32>),
    /// Fin d'une trame de lecture : tout ce qui précède va dans la même trame.
    Tick,
    Names(Vec<(u64, String)>),
    Stop,
}

pub(super) type RecTx = mpsc::Sender<RecMsg>;

pub(super) struct Config {
    pub dir: PathBuf,
    pub base: String,
    pub tracks: bool,
    pub me: u64,
    pub names: Vec<(u64, String)>,
    /// Libellés des pistes sans nom connu : moi, son d'un direct.
    pub me_label: String,
    pub stream_label: String,
}

struct Track {
    file: OpusFile<BufWriter<File>>,
    enc: Encoder,
    /// Trame reçue pendant la trame de lecture en cours.
    fed: bool,
}

impl Track {
    fn create(path: &Path, bitrate: i32, serial: u32) -> Option<Track> {
        let mut enc = Encoder::new(SAMPLE_RATE, Channels::Mono, Application::Audio).ok()?;
        let _ = enc.set_bitrate(Bitrate::Bits(bitrate));
        let file = File::create(path).ok()?;
        let file = OpusFile::create(BufWriter::new(file), 1, PRE_SKIP, FRAME as u64, serial).ok()?;
        Some(Track { file, enc, fed: false })
    }

    fn encode(&mut self, pcm: &[f32], out: &mut [u8]) -> std::io::Result<()> {
        match self.enc.encode_float(pcm, out) {
            Ok(n) => self.file.push(&out[..n]),
            // Trame refusée par l'encodeur : un silence garde le fichier à l'heure.
            Err(_) => self.file.push(&SILENCE),
        }
    }

    fn silence(&mut self) -> std::io::Result<()> {
        self.file.push(&SILENCE)
    }
}

/// Chemin libre : `nom.ogg`, sinon `nom (2).ogg`, etc.
fn free_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.ogg"));
    if !first.exists() {
        return first;
    }
    (2..1000).map(|n| dir.join(format!("{stem} ({n}).ogg"))).find(|p| !p.exists()).unwrap_or(first)
}

struct Session {
    cfg: Config,
    mix_path: PathBuf,
    mix: Track,
    tracks: HashMap<u64, Track>,
    names: HashMap<u64, String>,
    mic: VecDeque<f32>,
    mic_flowing: bool,
    /// Trames des personnes pour la trame de lecture en cours.
    now: Vec<(u64, Vec<f32>)>,
    frame: Vec<f32>,
    mic_frame: Vec<f32>,
    out: Vec<u8>,
    ticks: u64,
    files: usize,
}

impl Session {
    fn start(cfg: Config) -> Option<Session> {
        std::fs::create_dir_all(&cfg.dir).ok()?;
        let mix_path = free_path(&cfg.dir, &file_stem(&cfg.base));
        let mix = Track::create(&mix_path, MIX_BITRATE, 1)?;
        let names = cfg.names.iter().cloned().collect();
        Some(Session {
            cfg,
            mix_path,
            mix,
            tracks: HashMap::new(),
            names,
            mic: VecDeque::new(),
            mic_flowing: false,
            now: Vec::new(),
            frame: vec![0.0; FRAME],
            mic_frame: vec![0.0; FRAME],
            out: vec![0; MAX_PACKET],
            ticks: 0,
            files: 1,
        })
    }

    fn mic(&mut self, pcm: &[f32]) {
        if pcm.is_empty() {
            self.mic.extend(std::iter::repeat_n(0.0, FRAME));
        } else {
            self.mic.extend(pcm.iter().copied());
        }
        if self.mic.len() > MIC_MAX {
            let extra = self.mic.len() - MIC_LEAD;
            self.mic.drain(..extra);
        }
    }

    /// Prochaine trame du micro dans `mic_frame` ; faux si rien n'est prêt.
    fn take_mic(&mut self) -> bool {
        if !self.mic_flowing && self.mic.len() >= MIC_LEAD {
            self.mic_flowing = true;
        }
        if self.mic_flowing && self.mic.len() >= FRAME {
            for (d, s) in self.mic_frame.iter_mut().zip(self.mic.drain(..FRAME)) {
                *d = s;
            }
            return true;
        }
        // File vide : on reprend de l'avance avant d'y puiser à nouveau.
        self.mic_flowing = false;
        false
    }

    /// Piste de `user`, créée à sa première trame et calée par des silences.
    fn track(&mut self, user: u64) -> Option<&mut Track> {
        if !self.tracks.contains_key(&user) {
            let who = match self.names.get(&user) {
                Some(n) => file_stem(n),
                None if user == self.cfg.me => self.cfg.me_label.clone(),
                None if user & super::STREAM_AUDIO != 0 => format!("{} {}", self.cfg.stream_label, user & !super::STREAM_AUDIO),
                None => user.to_string(),
            };
            let stem = format!("{} - {}", file_stem(&self.cfg.base), who);
            let path = free_path(&self.cfg.dir, &stem);
            let mut t = Track::create(&path, TRACK_BITRATE, self.files as u32 + 1)?;
            for _ in 0..self.ticks {
                t.silence().ok()?;
            }
            self.files += 1;
            self.tracks.insert(user, t);
        }
        self.tracks.get_mut(&user)
    }

    fn tick(&mut self) -> std::io::Result<()> {
        let has_mic = self.take_mic();
        self.frame.fill(0.0);
        if has_mic {
            self.frame.copy_from_slice(&self.mic_frame);
        }
        let now = std::mem::take(&mut self.now);
        for (_, pcm) in &now {
            for (m, s) in self.frame.iter_mut().zip(pcm) {
                *m += s;
            }
        }
        let peak = self.frame.iter().fold(0.0f32, |m, x| m.max(x.abs()));
        if peak > CEILING {
            let g = CEILING / peak;
            self.frame.iter_mut().for_each(|x| *x *= g);
        }
        let mut out = std::mem::take(&mut self.out);
        let frame = std::mem::take(&mut self.frame);
        let mut result = self.mix.encode(&frame, &mut out);
        if self.cfg.tracks && result.is_ok() {
            result = self.feed_tracks(has_mic, &now, &mut out);
        }
        self.frame = frame;
        self.out = out;
        self.now = now;
        self.now.clear();
        self.ticks += 1;
        result
    }

    fn feed_tracks(&mut self, has_mic: bool, now: &[(u64, Vec<f32>)], out: &mut [u8]) -> std::io::Result<()> {
        let failed = || std::io::Error::other("piste");
        let me = self.cfg.me;
        if has_mic && self.mic_frame.iter().any(|x| *x != 0.0) {
            let mic = std::mem::take(&mut self.mic_frame);
            let r = match self.track(me) {
                Some(t) => {
                    t.fed = true;
                    t.encode(&mic, out)
                }
                None => Err(failed()),
            };
            self.mic_frame = mic;
            r?;
        }
        for (user, pcm) in now {
            let t = self.track(*user).ok_or_else(failed)?;
            if !t.fed {
                t.fed = true;
                t.encode(pcm, out)?;
            }
        }
        // Qui n'a rien dit pendant cette trame : un silence, pour rester calé.
        for t in self.tracks.values_mut() {
            if !std::mem::take(&mut t.fed) {
                t.silence()?;
            }
        }
        Ok(())
    }

    fn finish(self) -> Result<CallRecUp, CallRecFail> {
        let ms = self.ticks * 20;
        let mut ok = self.mix.file.finish().and_then(|mut w| std::io::Write::flush(&mut w)).is_ok();
        for (_, t) in self.tracks {
            ok &= t.file.finish().and_then(|mut w| std::io::Write::flush(&mut w)).is_ok();
        }
        if ok { Ok(CallRecUp::Done { file: self.mix_path, files: self.files, ms }) } else { Err(CallRecFail::Write) }
    }
}

/// Corps du fil d'écriture : jusqu'à [`RecMsg::Stop`] ou la fin de l'appel
/// (tous les émetteurs lâchés). `done` reçoit l'issue, une fois.
pub(super) fn run(cfg: Config, rx: mpsc::Receiver<RecMsg>, done: impl FnOnce(CallRecUp)) {
    let Some(mut s) = Session::start(cfg) else {
        done(CallRecUp::Failed(CallRecFail::Create));
        return;
    };
    while let Ok(m) = rx.recv() {
        let r = match m {
            RecMsg::Mic(pcm) => {
                s.mic(&pcm);
                Ok(())
            }
            RecMsg::Peer(user, pcm) => {
                if pcm.len() == FRAME && !s.now.iter().any(|(u, _)| *u == user) {
                    s.now.push((user, pcm));
                }
                Ok(())
            }
            RecMsg::Tick => s.tick(),
            RecMsg::Names(names) => {
                s.names.extend(names);
                Ok(())
            }
            RecMsg::Stop => break,
        };
        if r.is_err() {
            // Ce qui est déjà écrit reste lisible : les fichiers sont fermés proprement.
            let _ = s.finish();
            done(CallRecUp::Failed(CallRecFail::Write));
            return;
        }
    }
    done(s.finish().unwrap_or_else(CallRecUp::Failed));
}

/// Lance le fil d'écriture ; `None` si le système refuse un fil de plus.
pub(super) fn spawn(cfg: Config, done: impl FnOnce(CallRecUp) + Send + 'static) -> Option<RecTx> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new().name("ferrocord-enregistrement".into()).spawn(move || run(cfg, rx, done)).ok()?;
    Some(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::clip;

    fn tone(hz: f32, amp: f32, start: usize, n: usize) -> Vec<f32> {
        (start..start + n).map(|i| amp * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin()).collect()
    }

    fn rms(x: &[f32]) -> f32 {
        (x.iter().map(|v| v * v).sum::<f32>() / x.len().max(1) as f32).sqrt()
    }

    fn read(path: &Path) -> Vec<f32> {
        let pcm = clip::decode(std::fs::read(path).unwrap()).unwrap();
        assert_eq!(pcm.channels, 1);
        pcm.data.iter().map(|s| f32::from(*s) / 32768.0).collect()
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrocord-test-enr-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn config(dir: &Path, tracks: bool) -> Config {
        Config { dir: dir.to_path_buf(), base: "Appel : essai".into(), tracks, me: 1, names: vec![(7, "Alice/Bob".into())], me_label: "moi".into(), stream_label: "direct".into() }
    }

    /// Une seconde d'appel synthétique : le micro tout du long, Alice (7) la
    /// première moitié, une inconnue (8) la seconde.
    fn feed(tx: &RecTx) {
        for f in 0..50 {
            let _ = tx.send(RecMsg::Mic(tone(300.0, 0.2, f * FRAME, FRAME)));
            if f < 25 {
                let _ = tx.send(RecMsg::Peer(7, tone(700.0, 0.2, f * FRAME, FRAME)));
            } else {
                let _ = tx.send(RecMsg::Peer(8, tone(1100.0, 0.2, f * FRAME, FRAME)));
            }
            let _ = tx.send(RecMsg::Tick);
        }
    }

    #[test]
    fn mixed_file_reads_back_with_every_voice() {
        let dir = temp_dir("mixe");
        let (tx, rx) = mpsc::channel();
        feed(&tx);
        let _ = tx.send(RecMsg::Stop);
        let mut up = None;
        run(config(&dir, false), rx, |u| up = Some(u));
        let Some(CallRecUp::Done { file, files, ms }) = up else { panic!("{up:?}") };
        assert_eq!((files, ms), (1, 1000));
        assert_eq!(file, dir.join("Appel _ essai.ogg"));
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1, "sans l'option : un seul fichier");
        let pcm = read(&file);
        assert!((pcm.len() as i64 - 48_000).abs() <= 960, "{} échantillons", pcm.len());
        // Le micro est là (avec 40 ms d'avance prise au début), et les voix aussi.
        assert!(rms(&pcm[9_600..43_200]) > 0.15, "mixage : {}", rms(&pcm[9_600..43_200]));
        assert!(pcm.iter().all(|x| x.abs() <= 1.0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn one_track_per_person_aligned_on_the_same_start() {
        let dir = temp_dir("pistes");
        let (tx, rx) = mpsc::channel();
        feed(&tx);
        drop(tx);
        // Émetteurs lâchés sans `Stop` (appel fini) : les fichiers sont fermés quand même.
        let mut up = None;
        run(config(&dir, true), rx, |u| up = Some(u));
        let Some(CallRecUp::Done { files, .. }) = up else { panic!("{up:?}") };
        assert_eq!(files, 4);
        let (alice, other, me) = (read(&dir.join("Appel _ essai - Alice_Bob.ogg")), read(&dir.join("Appel _ essai - 8.ogg")), read(&dir.join("Appel _ essai - moi.ogg")));
        for t in [&alice, &other, &me] {
            assert!((t.len() as i64 - 48_000).abs() <= 960, "piste calée : {}", t.len());
        }
        let half = 24_000;
        assert!(rms(&alice[4_800..half - 2_400]) > 0.1 && rms(&alice[half + 2_400..]) < 0.01, "Alice parle la première moitié");
        assert!(rms(&other[..half - 2_400]) < 0.01 && rms(&other[half + 2_400..43_200]) > 0.1, "l'autre la seconde");
        assert!(rms(&me[9_600..43_200]) > 0.1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unusable_folder_fails_cleanly() {
        let dir = temp_dir("refus");
        std::fs::create_dir_all(&dir).unwrap();
        // Un fichier à la place du dossier : la création échoue.
        let blocker = dir.join("pas-un-dossier");
        std::fs::write(&blocker, b"x").unwrap();
        let (_tx, rx) = mpsc::channel();
        let mut up = None;
        run(config(&blocker, false), rx, |u| up = Some(u));
        assert_eq!(up, Some(CallRecUp::Failed(CallRecFail::Create)));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mic_queue_absorbs_bursts_and_gaps() {
        let dir = temp_dir("micro");
        let mut s = Session::start(config(&dir, false)).unwrap();
        assert!(!s.take_mic(), "rien encore");
        s.mic(&[0.5; FRAME]);
        assert!(!s.take_mic(), "une trame : pas assez d'avance");
        s.mic(&[]);
        assert!(s.take_mic() && s.mic_frame[0] == 0.5);
        assert!(s.take_mic() && s.mic_frame[0] == 0.0, "trame vide : silence");
        assert!(!s.take_mic());
        for _ in 0..30 {
            s.mic(&[0.1; FRAME]);
        }
        assert!(s.mic.len() <= MIC_MAX, "trop-plein jeté");
        drop(s);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
