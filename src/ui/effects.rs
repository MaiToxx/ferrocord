//! Lecture des effets de profil (lot ND) par-dessus une carte de profil ou
//! un profil complet.
//!
//! Budget mémoire. Un effet est fait d'une à quatre couches APNG de
//! 450 × 880 ; décodée en entier, une couche d'une centaine d'images
//! pèserait plus de 150 Mo. Ici, rien n'est décodé d'avance :
//! - seul le profil **ouvert** a une scène ; elle est libérée à l'image où
//!   il n'est plus dessiné (fermeture) ;
//! - chaque couche est décodée **en flux** par un fil à elle, une image à la
//!   fois, à la taille affichée (au plus [`MAX_LAYER_PIXELS`] pixels) ; le fil
//!   attend que l'interface ait pris une image avant de décoder la suivante ;
//! - par couche : le fichier compressé, le canevas du décodeur, une texture
//!   et une image en attente. Au plus [`MAX_LAYERS`](crate::state::effects::MAX_LAYERS)
//!   couches et [`MAX_BYTES`] octets compressés par effet ;
//! - avec « réduire les animations » (ou « lire les GIF » coupé), une seule
//!   image fixe est décodée, et aucun fil ne reste.
//!
//! Redessins : un seul réveil est demandé, pour la prochaine image due parmi
//! les couches visibles. Profil fermé : aucune scène, aucun réveil, aucun fil.

use crate::images::{self, anim, Images};
use crate::state::effects::{EffectConfig, Layer, Phase, CANVAS_WIDTH};
use eframe::egui::{self, ColorImage, CornerRadius, Rect, TextureHandle, TextureOptions};
use std::cell::RefCell;
use std::sync::mpsc::{sync_channel, Receiver, TryRecvError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Pixels d'une couche décodée, au plus (800 Ko de texture) ; au-delà, elle
/// est décodée plus petite et étirée par le processeur graphique.
pub(super) const MAX_LAYER_PIXELS: u32 = 200_000;
/// Octets compressés d'un effet, toutes couches confondues, au plus.
pub(super) const MAX_BYTES: usize = 8 << 20;
/// Côté maximal d'une couche source.
const MAX_SIDE: u32 = 2048;
/// Scènes vivantes au plus (carte et profil complet au même instant).
const MAX_SCENES: usize = 2;
/// Attente avant de regarder de nouveau si l'image suivante est décodée.
const POLL: Duration = Duration::from_millis(4);

/// Fichier d'une couche, téléchargé hors du fil d'interface.
#[derive(Default)]
enum Fetch {
    #[default]
    Idle,
    Pending,
    Ready(Arc<[u8]>),
    Failed,
}

type Slot = Arc<Mutex<Fetch>>;

/// Flux d'images décodées d'une couche : le fil s'arrête quand le récepteur est lâché.
struct Stream {
    rx: Receiver<(ColorImage, Duration)>,
}

#[derive(Default)]
struct Run {
    file: Slot,
    stream: Option<Stream>,
    /// Passage en cours (voir [`Layer::phase`]).
    pass: Option<u64>,
    tex: Option<TextureHandle>,
    /// Échéance de l'image suivante.
    due: Option<Instant>,
    /// Le flux du passage en cours est terminé, à cet instant.
    ended: Option<Instant>,
    /// Passages rejoués d'une couche sans durée (celle du fichier).
    replays: u64,
}

struct Scene {
    effect: u64,
    /// Largeur du canevas en pixels (clé avec `effect`).
    width_px: u32,
    started: Option<Instant>,
    layers: Vec<Run>,
    still: Run,
    last_frame: u64,
}

#[derive(Default)]
struct Inner {
    scenes: Vec<Scene>,
    frame: u64,
}

/// Scènes des effets de profil affichés. Vit dans `App`.
#[derive(Default)]
pub(super) struct Stage {
    inner: RefCell<Inner>,
}

/// Chiffres pour les tests et les mesures.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Stats {
    pub scenes: usize,
    pub streams: usize,
    pub textures: usize,
    pub texture_bytes: usize,
    pub file_bytes: usize,
}

/// Taille de décodage d'une couche affichée sur `w` × `h` pixels.
pub(super) fn decode_size(w: f32, h: f32) -> (u32, u32) {
    let (w, h) = (w.max(1.0), h.max(1.0));
    let k = (MAX_LAYER_PIXELS as f32 / (w * h)).sqrt().min(1.0);
    (((w * k).floor() as u32).max(1), ((h * k).floor() as u32).max(1))
}

fn start_fetch(images: &Images, ctx: &egui::Context, slot: &Slot, url: &str) {
    let Ok(mut f) = slot.lock() else { return };
    if !matches!(*f, Fetch::Idle) {
        return;
    }
    *f = Fetch::Pending;
    drop(f);
    let (back, ctx) = (slot.clone(), ctx.clone());
    let sent = images.fetch_raw(url.to_string(), move |res| {
        if let Ok(mut f) = back.lock() {
            *f = match res {
                Ok(b) if b.len() <= MAX_BYTES => Fetch::Ready(b.into()),
                _ => Fetch::Failed,
            };
        }
        ctx.request_repaint();
    });
    if !sent
        && let Ok(mut f) = slot.lock()
    {
        *f = Fetch::Failed;
    }
}

fn ready(slot: &Slot) -> Option<Arc<[u8]>> {
    match &*slot.lock().ok()? {
        Fetch::Ready(b) => Some(b.clone()),
        _ => None,
    }
}

/// Lance le décodage en flux de `bytes` à `size` pixels ; `first_only` :
/// une seule image (image fixe). Le fil se bloque tant que l'interface n'a
/// pas pris l'image précédente, et s'arrête dès que le récepteur est lâché.
fn start_stream(ctx: &egui::Context, bytes: Arc<[u8]>, size: (u32, u32), first_only: bool) -> Option<Stream> {
    let (tx, rx) = sync_channel::<(ColorImage, Duration)>(1);
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name("effet-de-profil".into())
        .spawn(move || {
            let mut first = true;
            let _ = anim::stream_frames(&bytes, MAX_SIDE, size, |img, d| {
                if tx.send((img, d)).is_err() {
                    return false;
                }
                // La première image réveille l'interface ; les suivantes sont prises à leur échéance.
                if first {
                    first = false;
                    ctx.request_repaint();
                }
                !first_only
            });
            // Flux terminé : l'interface le constate à l'échéance en cours ; fin sans
            // aucune image (fichier illisible) : elle est réveillée pour le voir.
            if first {
                ctx.request_repaint();
            }
        })
        .ok()?;
    Some(Stream { rx })
}

impl Run {
    fn stop(&mut self) {
        self.stream = None;
        self.tex = None;
        self.pass = None;
        self.due = None;
        self.ended = None;
        self.replays = 0;
    }

    /// Avance le flux jusqu'à l'image due à `now` ; rend le délai avant le
    /// prochain changement, s'il y en a un.
    fn advance(&mut self, ctx: &egui::Context, now: Instant) -> Option<Duration> {
        let stream = self.stream.as_ref()?;
        if self.due.is_some_and(|d| now < d) {
            return self.due.map(|d| d - now);
        }
        match stream.rx.try_recv() {
            Ok((img, delay)) => {
                match &mut self.tex {
                    Some(t) if t.size() == img.size => t.set(img, TextureOptions::LINEAR),
                    slot => *slot = Some(ctx.load_texture("effet", img, TextureOptions::LINEAR)),
                }
                // Cadence tenue si l'image est à l'heure ; sinon elle repart d'ici.
                let at = self.due.filter(|d| now.duration_since(*d) < delay).unwrap_or(now) + delay;
                self.due = Some(at);
                Some(at.saturating_duration_since(now))
            }
            // Décodeur en retard (ou première image pas encore prête, annoncée par un réveil).
            Err(TryRecvError::Empty) => self.tex.is_some().then_some(POLL),
            Err(TryRecvError::Disconnected) => {
                self.stream = None;
                self.ended = Some(now);
                None
            }
        }
    }
}

impl Stage {
    /// Fin d'une image : les scènes qui n'y ont pas été dessinées sont
    /// libérées (textures, fichiers, fils de décodage).
    pub(super) fn end_frame(&self) {
        let mut inner = self.inner.borrow_mut();
        let frame = inner.frame;
        inner.scenes.retain(|s| s.last_frame == frame);
        inner.frame += 1;
    }

    #[cfg(test)]
    pub(super) fn stats(&self) -> Stats {
        let inner = self.inner.borrow();
        let mut st = Stats { scenes: inner.scenes.len(), ..Stats::default() };
        for run in inner.scenes.iter().flat_map(|s| s.layers.iter().chain(std::iter::once(&s.still))) {
            st.streams += usize::from(run.stream.is_some());
            if let Some(t) = &run.tex {
                st.textures += 1;
                st.texture_bytes += t.size()[0] * t.size()[1] * 4;
            }
            st.file_bytes += ready(&run.file).map_or(0, |b| b.len());
        }
        st
    }

    /// Dessine l'effet `cfg` par-dessus la carte `card` (coins `radius`).
    /// `play` : animé ; sinon son image fixe, s'il en a une. À appeler à
    /// chaque image où le profil est affiché : c'est ce qui garde la scène.
    pub(super) fn paint(&self, painter: &egui::Painter, images: &Images, cfg: &EffectConfig, card: Rect, radius: CornerRadius, play: bool) {
        if !painter.clip_rect().intersects(card) || card.width() < 1.0 {
            return;
        }
        let ctx = painter.ctx();
        let ppp = ctx.pixels_per_point();
        let scale = card.width() / CANVAS_WIDTH as f32;
        let width_px = (card.width() * ppp).round() as u32;
        let mut inner = self.inner.borrow_mut();
        let frame = inner.frame;
        let at = match inner.scenes.iter().position(|s| s.effect == cfg.id && s.width_px == width_px) {
            Some(i) => i,
            None => {
                if inner.scenes.len() >= MAX_SCENES {
                    inner.scenes.remove(0);
                }
                inner.scenes.push(Scene { effect: cfg.id, width_px, started: None, layers: cfg.layers.iter().map(|_| Run::default()).collect(), still: Run::default(), last_frame: frame });
                inner.scenes.len() - 1
            }
        };
        let scene = &mut inner.scenes[at];
        scene.last_frame = frame;
        let now = Instant::now();
        let painter = painter.with_clip_rect(card.intersect(painter.clip_rect()));
        let place = |l: &Layer| Rect::from_min_size(card.min + egui::vec2(l.x as f32, l.y as f32) * scale, egui::vec2(l.width as f32, l.height as f32) * scale);
        let size_of = |r: Rect| decode_size(r.width() * ppp, r.height() * ppp);
        let mut wake: Option<Duration> = None;
        let mut sooner = |d: Duration| wake = Some(wake.map_or(d, |w| w.min(d)));

        if !play {
            // Image fixe : les flux animés sont arrêtés, l'animation reprendra du début.
            scene.started = None;
            scene.layers.iter_mut().for_each(Run::stop);
            let (Some(url), Some(first)) = (&cfg.still, cfg.layers.first()) else { return };
            let rect = place(first);
            let run = &mut scene.still;
            start_fetch(images, ctx, &run.file, url);
            if run.tex.is_none()
                && run.stream.is_none()
                && run.ended.is_none()
                && let Some(bytes) = ready(&run.file)
            {
                run.stream = start_stream(ctx, bytes, size_of(rect), true);
            }
            run.advance(ctx, now);
            if let Some(t) = &run.tex {
                painter.add(textured(rect, card, radius, t));
            }
            return;
        }

        scene.still.stop();
        // Le compte à rebours part quand la première couche est téléchargée : l'intro n'est pas mangée par le réseau.
        let mut total = 0usize;
        for (l, run) in cfg.layers.iter().zip(&mut scene.layers) {
            start_fetch(images, ctx, &run.file, &l.src);
            total += ready(&run.file).map_or(0, |b| b.len());
            if total > MAX_BYTES
                && let Ok(mut f) = run.file.lock()
            {
                *f = Fetch::Failed;
            }
        }
        if scene.started.is_none() && scene.layers.iter().any(|r| ready(&r.file).is_some()) {
            scene.started = Some(now);
        }
        let Some(started) = scene.started else { return };
        let elapsed = now.duration_since(started).as_millis() as u64;
        for (l, run) in cfg.layers.iter().zip(&mut scene.layers) {
            let rect = place(l);
            match l.phase(elapsed) {
                Phase::Waiting { in_ms } => sooner(Duration::from_millis(in_ms)),
                Phase::Paused { in_ms } => {
                    run.stop();
                    sooner(Duration::from_millis(in_ms));
                }
                Phase::Over => run.stop(),
                Phase::Playing { pass, left_ms } => {
                    // Sans durée : le fichier donne la sienne, puis la pause de boucle.
                    let replay = left_ms.is_none() && l.looped && run.ended.is_some_and(|e| now.duration_since(e).as_millis() as u64 >= u64::from(l.loop_delay_ms));
                    if replay {
                        run.replays += 1;
                    }
                    let want = pass + run.replays;
                    if run.pass != Some(want)
                        && let Some(bytes) = ready(&run.file)
                    {
                        // La texture du passage précédent reste jusqu'à la première image du nouveau.
                        run.stream = start_stream(ctx, bytes, size_of(rect), false);
                        run.pass = Some(want);
                        run.due = None;
                        run.ended = None;
                    }
                    if let Some(d) = run.advance(ctx, now) {
                        sooner(d);
                    }
                    if let (Some(e), true, None) = (run.ended, l.looped, left_ms) {
                        sooner(Duration::from_millis(u64::from(l.loop_delay_ms)).saturating_sub(now.duration_since(e)).max(POLL));
                    }
                    if let Some(ms) = left_ms {
                        sooner(Duration::from_millis(ms.max(1)));
                    }
                    if let Some(t) = &run.tex {
                        painter.add(textured(rect, card, radius, t));
                    }
                }
            }
        }
        if let Some(d) = wake {
            ctx.request_repaint_after(d);
        }
    }
}

impl super::App {
    /// Un profil affiché porte un effet : sa configuration est demandée (une fois).
    pub(super) fn want_profile_effect(&mut self, s: &super::profile::Shown) {
        if let Some((id, sku)) = s.profile.as_ref().and_then(crate::state::profiles::Profile::effect_ref) {
            self.store.want_effect(id, sku);
        }
    }

    /// Effet du profil `s` par-dessus sa fenêtre `card`. Animé seulement si
    /// les GIF jouent d'eux-mêmes (« réduire les animations » : image fixe).
    pub(super) fn paint_profile_effect(&self, painter: &egui::Painter, s: &super::profile::Shown, card: Rect, radius: CornerRadius) {
        let Some(cfg) = s.profile.as_ref().and_then(crate::state::profiles::Profile::effect).and_then(|id| self.store.effects.config(id)) else { return };
        let play = super::motion::get(painter.ctx()).gifs == super::motion::Play::Always;
        self.profile_fx.paint(painter, &self.images, &cfg, card, radius, play);
    }
}

/// La partie de la couche `layer` qui tombe dans la carte `card`, avec les
/// coins de la carte là où leurs bords coïncident.
fn textured(layer: Rect, card: Rect, radius: CornerRadius, tex: &TextureHandle) -> egui::Shape {
    let (vis, uv) = clip_uv(layer, card);
    let near = |a: f32, b: f32| (a - b).abs() < 0.5;
    let (l, r, t, b) = (near(vis.left(), card.left()), near(vis.right(), card.right()), near(vis.top(), card.top()), near(vis.bottom(), card.bottom()));
    let pick = |on: bool, v: u8| if on { v } else { 0 };
    let radius = CornerRadius { nw: pick(l && t, radius.nw), ne: pick(r && t, radius.ne), sw: pick(l && b, radius.sw), se: pick(r && b, radius.se) };
    egui::Shape::Rect(egui::epaint::RectShape::filled(vis, radius, egui::Color32::WHITE).with_texture(tex.id(), uv))
}

/// Rectangle visible de `layer` dans `card`, et la part de la texture qui lui correspond.
fn clip_uv(layer: Rect, card: Rect) -> (Rect, Rect) {
    let vis = layer.intersect(card);
    let (w, h) = (layer.width().max(f32::EPSILON), layer.height().max(f32::EPSILON));
    let uv = Rect::from_min_max(egui::pos2((vis.left() - layer.left()) / w, (vis.top() - layer.top()) / h), egui::pos2((vis.right() - layer.left()) / w, (vis.bottom() - layer.top()) / h));
    (vis, uv)
}

/// Aperçu fixe d'un effet (vignette du choix dans l'édition du profil) :
/// passe par le cache d'images habituel, à la taille affichée.
pub(super) fn thumbnail(ui: &egui::Ui, images: &Images, cfg: &EffectConfig, rect: Rect, radius: CornerRadius) {
    let Some(url) = cfg.thumbnail.as_deref().or(cfg.still.as_deref()) else { return };
    let image = crate::state::model::ImageRef::Media { owner: cfg.id, slot: u16::MAX, rev: 1 };
    if let images::Image::Ready(tex) = images.get_asset(image, url, rect.size()) {
        ui.painter().add(images::textured(rect, radius, tex));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_size_is_capped_and_keeps_the_shape() {
        assert_eq!(decode_size(300.0, 587.0), (300, 587), "carte à l'échelle 1 : taille affichée");
        let (w, h) = decode_size(900.0, 1760.0);
        assert!(w * h <= MAX_LAYER_PIXELS, "{w} × {h}");
        assert!((w as f32 / h as f32 - 900.0 / 1760.0).abs() < 0.01);
        assert_eq!(decode_size(0.0, -3.0), (1, 1));
    }

    #[test]
    fn layer_is_clipped_to_the_card_with_matching_uv() {
        let card = Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(300.0, 400.0));
        let layer = Rect::from_min_size(card.min, egui::vec2(300.0, 587.0));
        let (vis, uv) = clip_uv(layer, card);
        assert_eq!(vis, card);
        assert_eq!(uv.min, egui::pos2(0.0, 0.0));
        assert!((uv.max.y - 400.0 / 587.0).abs() < 1e-5 && (uv.max.x - 1.0).abs() < 1e-5);
    }

    /// Une scène ne vit que tant qu'elle est dessinée : fermée, tout est libéré.
    #[test]
    fn scene_lives_only_while_painted() {
        use crate::state::effects::Layer;
        let ctx = egui::Context::default();
        let images = Images::disabled(ctx.clone());
        let stage = Stage::default();
        let layer = Layer { src: "https://cdn.discordapp.com/x.png".into(), looped: true, width: 450, height: 880, duration_ms: 1000, start_ms: 0, loop_delay_ms: 0, x: 0, y: 0, z: 0 };
        let cfg = EffectConfig { id: 5, sku_id: 5, title: String::new(), label: String::new(), thumbnail: None, still: None, layers: vec![layer] };
        let card = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(300.0, 400.0));
        let frame = |paint: bool, play: bool| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                if paint {
                    stage.paint(ui.painter(), &images, &cfg, card, CornerRadius::same(8), play);
                }
            });
            out.textures_delta.clear();
            // Les chiffres de l'image, avant le ménage de fin d'image.
            let st = stage.stats();
            stage.end_frame();
            st
        };
        assert_eq!(frame(true, true).scenes, 1);
        assert_eq!(frame(true, false).scenes, 1, "image fixe : même scène");
        // Images désactivées : aucun fichier, donc ni fil ni texture.
        assert_eq!(frame(true, true), Stats { scenes: 1, ..Stats::default() });
        frame(false, true);
        assert_eq!(stage.stats(), Stats::default(), "profil fermé : scène libérée");
    }
}
