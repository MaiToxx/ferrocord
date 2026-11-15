use super::*;
use crate::video::FrameData;
use std::sync::atomic::AtomicUsize;

/// Source factice : `frames` images grises à 50 i/s, et autant de son à 48 kHz.
struct Fake {
    frames: u64,
    next_video: u64,
    next_audio: u64,
    audio: bool,
    video: bool,
}

const STEP_US: u64 = 20_000;

impl Fake {
    fn new(frames: u64, video: bool, audio: bool) -> Self {
        Fake { frames, next_video: 0, next_audio: 0, audio, video }
    }
}

impl Source for Fake {
    fn info(&self) -> Info {
        Info { duration_us: self.frames * STEP_US, video: self.video.then_some((4, 2)), rotation: 0, audio: self.audio }
    }

    fn video(&mut self) -> Result<Option<Frame>, String> {
        if !self.video || self.next_video >= self.frames {
            return Ok(None);
        }
        let i = self.next_video;
        self.next_video += 1;
        // Le gris suit le numéro de l'image (modulo 200, de 16 à 215).
        Ok(Some(Frame { width: 4, height: 2, pts_us: i * STEP_US, data: FrameData::I420 { y: vec![16 + (i % 200) as u8; 8], u: vec![128; 2], v: vec![128; 2] } }))
    }

    fn audio(&mut self) -> Result<Option<Chunk>, String> {
        if !self.audio || self.next_audio >= self.frames {
            return Ok(None);
        }
        let i = self.next_audio;
        self.next_audio += 1;
        // 20 ms dont chaque échantillon vaut le numéro du morceau (+ 1) / 1000.
        Ok(Some(Chunk { pts_us: i * STEP_US, rate: RATE, channels: 1, samples: vec![(i + 1) as f32 / 1000.0; 960] }))
    }

    fn seek(&mut self, us: u64) -> Result<(), String> {
        // Comme un vrai fichier : la vidéo repart d'une image clé (toutes les 10 images).
        self.next_video = us / STEP_US / 10 * 10;
        self.next_audio = us / STEP_US;
        Ok(())
    }
}

fn counted() -> (Wake, Arc<AtomicUsize>) {
    let n = Arc::new(AtomicUsize::new(0));
    let c = n.clone();
    let wake: Wake = Arc::new(move || {
        c.fetch_add(1, Ordering::Relaxed);
    });
    (wake, n)
}

fn wait_for(h: &Handle, what: &str, ok: impl Fn(&Snapshot) -> bool) -> Snapshot {
    let t0 = Instant::now();
    loop {
        let s = h.snapshot();
        if ok(&s) {
            return s;
        }
        assert!(t0.elapsed() < Duration::from_secs(10), "{what} : {s:?}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn fifo_mixes_in_order_and_drops_what_a_seek_made_stale() {
    let a = AudioOut::default();
    assert_eq!(a.push(&[0.1, 0.2, 0.3, 0.4]), 4);
    let mut out = [1.0f32; 2];
    a.mix(&mut out);
    assert_eq!(out, [1.1, 1.2]);
    // Saut : le reste est périmé, seule la suite sort.
    a.flush();
    assert_eq!(a.push(&[0.5, 0.6]), 2);
    let mut out = [0.0f32; 4];
    a.mix(&mut out);
    assert_eq!(out, [0.5, 0.6, 0.0, 0.0]);
    // Volume, puis pause : rien ne sort et rien n'est perdu.
    a.set_volume(0.5);
    a.push(&[1.0, 1.0]);
    a.set_paused(true);
    let mut out = [0.0f32; 2];
    a.mix(&mut out);
    assert_eq!(out, [0.0, 0.0]);
    a.set_paused(false);
    a.mix(&mut out);
    assert_eq!(out, [0.5, 0.5]);
    // Pleine : l'excédent est refusé, jamais écrasé.
    let big = vec![0.25f32; FIFO_SAMPLES + 10];
    assert_eq!(a.push(&big), FIFO_SAMPLES);
    assert_eq!(a.push(&big), 0);
    assert_eq!(a.buffered(), FIFO_SAMPLES);
}

#[test]
fn stereo_downmix_keeps_left_and_right() {
    assert_eq!(to_stereo(&[0.5, -0.5], 1), vec![0.5, 0.5, -0.5, -0.5]);
    assert_eq!(to_stereo(&[0.1, 0.2], 2), vec![0.1, 0.2]);
    assert_eq!(to_stereo(&[0.1, 0.2, 0.0, 9.0, 9.0, 9.0], 6), vec![0.1, 0.2]);
    assert!(to_stereo(&[1.0], 0).is_empty());
}

#[test]
fn video_plays_in_order_then_sleeps_at_the_end() {
    let (wake, wakes) = counted();
    let h = Handle::loading(wake);
    assert_eq!(h.snapshot().phase, Phase::Loading);
    h.start(|| Ok(Box::new(Fake::new(15, true, false))));
    let mut seen = 0;
    let mut last = None;
    let t0 = Instant::now();
    while h.snapshot().phase != Phase::Ended {
        if let Some((g, p)) = h.take_picture(seen) {
            assert!(g > seen);
            seen = g;
            assert_eq!((p.width, p.height, p.pixels.len()), (4, 2, 32));
            // Jamais de retour en arrière (le gris suit le numéro de l'image).
            assert!(last.is_none_or(|l| p.pixels[0] >= l));
            last = Some(p.pixels[0]);
        }
        assert!(t0.elapsed() < Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(1));
    }
    let s = h.snapshot();
    assert_eq!(s.info.video, Some((4, 2)));
    assert!((2..=15).contains(&s.generation), "{s:?}");
    assert_eq!(s.pos_us, 15 * STEP_US);
    // À la fin, plus aucun réveil : le fil dort (0 redessin au repos).
    let n = wakes.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(wakes.load(Ordering::Relaxed), n);
    // « Lecture » après la fin : tout repart du début.
    h.play();
    wait_for(&h, "reprise", |s| s.phase == Phase::Playing);
    wait_for(&h, "seconde fin", |s| s.phase == Phase::Ended);
    h.close();
}

#[test]
fn pause_freezes_position_and_wakes_nobody() {
    let (wake, wakes) = counted();
    let h = Handle::loading(wake);
    h.start(|| Ok(Box::new(Fake::new(500, true, false))));
    wait_for(&h, "première image", |s| s.generation > 0);
    h.pause();
    wait_for(&h, "pause", |s| s.phase == Phase::Paused);
    std::thread::sleep(Duration::from_millis(30));
    let (n, at) = (wakes.load(Ordering::Relaxed), h.snapshot());
    std::thread::sleep(Duration::from_millis(80));
    assert_eq!(wakes.load(Ordering::Relaxed), n, "réveil en pause");
    assert_eq!(h.snapshot(), at);
    assert!(at.pos_us < 2_000_000);
    // Saut en pause : l'image de la position voulue (pas l'image clé qui la
    // précède) est montrée, et la lecture ne repart pas.
    let before = at.generation;
    h.seek(1_100_000);
    let s = wait_for(&h, "saut", |s| s.generation > before);
    assert_eq!((s.phase.clone(), s.pos_us), (Phase::Paused, 1_100_000));
    let (_, p) = h.take_picture(before).unwrap_or_else(|| panic!("image absente"));
    let grey = |i: u8| {
        let mut px = Vec::new();
        let f = Frame { width: 4, height: 2, pts_us: 0, data: FrameData::I420 { y: vec![16 + i; 8], u: vec![128; 2], v: vec![128; 2] } };
        assert!(convert::frame_to_rgba(&f, &mut px).is_ok());
        px[0]
    };
    assert_eq!(p.pixels[0], grey(55), "image 55 attendue (1,1 s à 50 i/s)");
    h.play();
    wait_for(&h, "reprise", |s| s.phase == Phase::Playing && s.pos_us > 1_100_000);
    h.close();
    assert!(h.is_closed());
}

#[test]
fn audio_reaches_the_output_in_time_and_only_when_it_listens() {
    let h = Handle::loading(Arc::new(|| {}));
    let out = h.audio();
    // Fil audio factice : puise 10 ms toutes les 10 ms environ.
    let stop = Arc::new(AtomicBool::new(false));
    let got = Arc::new(Mutex::new(Vec::<f32>::new()));
    let (stop2, got2, out2) = (stop.clone(), got.clone(), out.clone());
    let mixer = std::thread::spawn(move || {
        while !stop2.load(Ordering::Relaxed) {
            let mut buf = [0.0f32; 960];
            out2.mix(&mut buf);
            got2.lock().unwrap_or_else(PoisonError::into_inner).extend(buf.iter().step_by(2));
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    std::thread::sleep(Duration::from_millis(30));
    h.start(|| Ok(Box::new(Fake::new(25, false, true))));
    wait_for(&h, "fin", |s| s.phase == Phase::Ended);
    std::thread::sleep(Duration::from_millis(50));
    stop.store(true, Ordering::Relaxed);
    let _ = mixer.join();
    let got = got.lock().unwrap_or_else(PoisonError::into_inner);
    // Les morceaux sortent dans l'ordre, sans retour en arrière ni trou au début.
    let heard: Vec<f32> = got.iter().copied().filter(|s| *s != 0.0).collect();
    assert!(heard.len() > 48 * 200, "{} échantillons entendus", heard.len());
    assert!(heard.windows(2).all(|p| p[0] <= p[1]));
    assert!((heard[0] - 0.001).abs() < 1e-6, "début manqué : {}", heard[0]);
}

#[test]
fn audio_without_output_still_ends_on_the_clock() {
    let h = Handle::loading(Arc::new(|| {}));
    h.start(|| Ok(Box::new(Fake::new(10, false, true))));
    let s = wait_for(&h, "fin", |s| s.phase == Phase::Ended);
    assert_eq!(s.pos_us, 10 * STEP_US);
    assert_eq!(h.audio().buffered(), 0, "son poussé sans sortie");
}

#[test]
fn open_failure_is_reported() {
    let h = Handle::loading(Arc::new(|| {}));
    h.start(|| Err("illisible".into()));
    let s = wait_for(&h, "échec", |s| matches!(s.phase, Phase::Failed(_)));
    assert_eq!(s.phase, Phase::Failed("illisible".into()));
    assert!(matches!(open(b"ni son ni image".to_vec(), None).map(|_| ()), Err(OpenError::Unsupported | OpenError::NoDecoder)));
    assert!(matches!(open(b"OggS n'importe quoi".to_vec(), None).map(|_| ()), Err(OpenError::Unsupported)));
}

#[test]
fn pcm_source_cuts_and_seeks() {
    let pcm = crate::audio::clip::Pcm { channels: 1, data: (0..9600).map(|i| i as i16).collect::<Vec<_>>().into() };
    let mut s = PcmSource::new(pcm);
    assert_eq!(s.info(), Info { duration_us: 200_000, audio: true, ..Info::default() });
    let c = s.audio().ok().flatten().unwrap_or_else(|| panic!("vide"));
    assert_eq!((c.pts_us, c.channels, c.samples.len()), (0, 2, 9600));
    assert!(s.seek(150_000).is_ok());
    let c = s.audio().ok().flatten().unwrap_or_else(|| panic!("vide"));
    assert_eq!((c.pts_us, c.samples.len()), (150_000, 4800));
    assert_eq!(c.samples[0], 7200.0 / 32768.0);
    assert!(s.audio().is_ok_and(|c| c.is_none()));
}
