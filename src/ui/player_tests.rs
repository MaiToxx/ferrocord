use super::*;
use crate::video::play::{Info, Picture};
use std::cell::RefCell;

fn file(id: u64, name: &str, mime: Option<&str>, size: u64, dims: (u32, u32)) -> Attachment {
    let mut a = Attachment::file(name, format!("https://cdn.discordapp.com/attachments/1/{id}/{name}"));
    a.id = id;
    a.content_type = mime.map(str::to_string);
    a.size = size;
    (a.width, a.height) = dims;
    a.analysed(0)
}

#[test]
fn only_known_formats_under_the_size_limit_play_here() {
    assert!(playable(&file(1, "clip.mp4", Some("video/mp4"), 1000, (640, 360))));
    assert!(playable(&file(1, "clip.MOV", None, 1000, (640, 360))), "d'après l'extension");
    assert!(playable(&file(1, "air.mp3", Some("audio/mpeg; charset=binary"), 1000, (0, 0))));
    assert!(playable(&file(1, "air.ogg", Some("audio/ogg"), 1000, (0, 0))));
    assert!(playable(&file(1, "clip.webm", Some("video/webm"), 1000, (640, 360))), "WebM : démultiplexeur maison");
    assert!(playable(&file(1, "clip.WEBM", None, 1000, (640, 360))));
    assert!(!playable(&file(1, "film.mkv", Some("video/x-matroska"), 1000, (640, 360))));
    assert!(!playable(&file(1, "clip.mp4", Some("video/mp4"), MAX_BYTES as u64 + 1, (640, 360))), "trop gros");
    assert!(!playable(&file(1, "notes.txt", Some("text/plain"), 10, (0, 0))));
    assert!(!playable(&file(1, "photo.png", Some("image/png"), 10, (10, 10))));
    assert_eq!(mime_of(&file(1, "a.m4a", None, 1, (0, 0))), Some("audio/mp4"));
    assert_eq!(time_text(65_000_000, 0), "1:05");
    assert_eq!(time_text(5_000_000, 125_000_000), "0:05 / 2:05");
}

/// Orchestrateur factice : rend une lecture figée et compte les appels.
struct Link {
    handle: Handle,
    opened: Vec<(String, Option<String>)>,
    gifs: usize,
    closed: usize,
}

impl VoiceLink for Link {
    fn sync(&mut self, _: &crate::state::voice_link::VoiceWant<'_>) {}
    fn command(&mut self, _: &crate::state::voice::VoiceCmd) {}
    fn request_codes(&mut self) {}
    fn poll(&mut self) -> Vec<crate::state::voice::VoiceEvent> {
        Vec::new()
    }
    fn open_media(&mut self, url: &str, mime: Option<&str>, gif: bool) -> Option<Handle> {
        self.opened.push((url.to_string(), mime.map(str::to_string)));
        self.gifs += usize::from(gif);
        Some(self.handle.clone())
    }
    fn close_media(&mut self) {
        self.closed += 1;
    }
}

fn paused(video: bool) -> Handle {
    let info = Info { duration_us: 90_000_000, video: video.then_some((640, 360)), rotation: 0, audio: true };
    let picture = video.then(|| Picture { width: 4, height: 2, pixels: vec![u32::MAX; 8] });
    Handle::fixed(Phase::Paused, info, 30_000_000, picture)
}

/// Une passe sur 700 × 900 points ; rend la hauteur prise, les zones
/// sensibles (rectangle, infobulle) et le délai de redessin demandé.
fn pass(ctx: &egui::Context, a: &Attachment, live: bool, events: Vec<egui::Event>, link: Option<&mut Link>) -> (f32, Vec<Rect>, Duration) {
    let t = Tokens::default();
    let height = RefCell::new(0.0);
    let zones = RefCell::new(Vec::new());
    let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(700.0, 900.0))), events, ..Default::default() };
    let mut out = ctx.run_ui(input, |ui| {
        show_full(ui, &t, Lang::Fr);
        let mut hot = Hot::default();
        let images = Images::disabled(ui.ctx().clone());
        let r = ui.scope(|ui| card(ui, &t, Lang::Fr, live.then_some(&images), a, &mut hot)).response.rect;
        *height.borrow_mut() = r.height();
        *zones.borrow_mut() = hot.rects();
        hot.interact(ui, &t, 1);
    });
    out.textures_delta.clear();
    if let Some(l) = link {
        tick(ctx, l);
    }
    let delay = out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap_or(Duration::MAX);
    (height.into_inner(), zones.into_inner(), delay)
}

fn click(at: egui::Pos2) -> Vec<egui::Event> {
    let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    vec![egui::Event::PointerMoved(at), button(true), button(false)]
}

#[test]
fn layout_is_the_same_measured_idle_and_playing() {
    for (a, video) in [(file(7, "clip.mp4", Some("video/mp4"), 5 << 20, (1280, 720)), true), (file(8, "air.mp3", Some("audio/mpeg"), 3 << 20, (0, 0)), false)] {
        let ctx = egui::Context::default();
        let measured = pass(&ctx, &a, false, vec![], None).0;
        let (idle, zones, _) = pass(&ctx, &a, true, vec![], None);
        assert_eq!(measured, idle);
        assert!(!zones.is_empty(), "au repos : au moins le bouton de lecture");
        preview(&ctx, a.id, paused(video), video, false);
        let mut link = Link { handle: paused(video), opened: vec![], gifs: 0, closed: 0 };
        pass(&ctx, &a, true, vec![], Some(&mut link));
        let (playing, zones, delay) = pass(&ctx, &a, true, vec![], Some(&mut link));
        assert_eq!(playing, idle, "même hauteur pendant la lecture");
        // Lecture/pause, position, son, (agrandir), fermer.
        assert!(zones.len() >= if video { 6 } else { 5 }, "{}", zones.len());
        assert_eq!(delay, Duration::MAX, "en pause : aucun redessin demandé");
    }
}

#[test]
fn nothing_is_fetched_before_the_click_and_close_stops_it() {
    let a = file(7, "clip.mp4", Some("video/mp4"), 5 << 20, (1280, 720));
    let ctx = egui::Context::default();
    let mut link = Link { handle: paused(true), opened: vec![], gifs: 0, closed: 0 };
    // Les deux premières passes d'un contexte neuf se redemandent d'elles-mêmes.
    pass(&ctx, &a, true, vec![], Some(&mut link));
    pass(&ctx, &a, true, vec![], Some(&mut link));
    let (_, zones, delay) = pass(&ctx, &a, true, vec![], Some(&mut link));
    assert!(link.opened.is_empty(), "rien avant le clic");
    assert_eq!(delay, Duration::MAX, "au repos : aucun redessin demandé");
    // Clic sur la vidéo : une seule ouverture, avec l'adresse et le type du fichier.
    pass(&ctx, &a, true, click(zones[0].center()), Some(&mut link));
    pass(&ctx, &a, true, vec![], Some(&mut link));
    assert_eq!(link.opened, vec![(a.url.clone(), Some("video/mp4".to_string()))]);
    assert!(active(&ctx).is_some_and(|x| x.id == 7 && x.video && x.tex.is_some()), "image reçue et montrée");
    // Agrandir, puis Échap : retour au fil, la lecture reste.
    update(&ctx, |x| x.full = true);
    pass(&ctx, &a, true, vec![], Some(&mut link));
    let esc = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
    pass(&ctx, &a, true, vec![esc], Some(&mut link));
    assert!(active(&ctx).is_some_and(|x| !x.full));
    // Fermer (dernière zone de la barre… la première à droite) : tout est lâché.
    let (_, zones, _) = pass(&ctx, &a, true, vec![], Some(&mut link));
    let close = zones.iter().copied().max_by(|l, r| l.center().x.total_cmp(&r.center().x)).unwrap_or(Rect::NOTHING);
    pass(&ctx, &a, true, click(close.center()), Some(&mut link));
    pass(&ctx, &a, true, vec![], Some(&mut link));
    assert_eq!(link.closed, 1);
    assert!(active(&ctx).is_none());
}

#[test]
fn a_video_left_unseen_is_paused() {
    let ctx = egui::Context::default();
    let a = file(7, "clip.mp4", Some("video/mp4"), 5 << 20, (1280, 720));
    // Lecture « en cours » qui n'est plus dessinée : `tick` demande la pause.
    let info = Info { duration_us: 1_000_000, video: Some((4, 2)), rotation: 0, audio: false };
    let h = Handle::fixed(Phase::Playing, info, 0, None);
    preview(&ctx, a.id, h.clone(), true, false);
    let mut link = Link { handle: h.clone(), opened: vec![], gifs: 0, closed: 0 };
    for _ in 0..6 {
        let t = Tokens::default();
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| show_full(ui, &t, Lang::Fr));
        out.textures_delta.clear();
        tick(&ctx, &mut link);
    }
    assert!(h.sent().contains(&"pause"));
}
