//! Quand jouer les animations (lot A6) : emojis, autocollants, GIF joints,
//! avatars et icônes. Choisi une fois par image par `App` d'après les
//! réglages synchronisés (« Lire les emojis animés », « Lire les GIF
//! automatiquement », « Animer les autocollants ») et « Réduire les
//! animations » ; lu là où les images sont dessinées.
//!
//! Animations de l'interface (lot ANIM) : durées et courbes en jetons,
//! primitives qui ne demandent une image que pendant leur durée. Aucune ne
//! tourne au repos : `toward`, `appear`, `watch` et `played` s'arrêtent
//! d'elles-mêmes, `dots` ne bat que tant qu'il est dessiné (quelqu'un écrit).
//! Le cran « aucune » (ou « Réduire les animations ») rend tout immédiat.

use crate::state::usersettings::{
    schema::{Key, SValue},
    AccountState,
};
use crate::state::local::Intensity;
use eframe::egui::{self, emath::TSTransform, layers::ShapeIdx, Color32, CornerRadius, Id, LayerId, Pos2, Rect, Shape, Ui};
use std::collections::HashMap;
use std::time::Duration;

/// Intensité effective des animations de l'interface.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Level {
    /// Rien ne bouge : chaque état est atteint tout de suite.
    #[default]
    None,
    /// Fondus seulement : ni échelle, ni glissement, ni rebond.
    Subtle,
    Full,
}

impl Level {
    /// « Réduire les animations » l'emporte sur le cran choisi.
    pub(crate) fn of(intensity: Intensity, reduced: bool) -> Level {
        match (reduced, intensity) {
            (true, _) | (_, Intensity::None) => Level::None,
            (_, Intensity::Subtle) => Level::Subtle,
            (_, Intensity::Full) => Level::Full,
        }
    }
}

/// Durées (secondes) : les seules de l'interface.
pub(crate) mod dur {
    /// Fondu de survol (lignes, boutons, icônes).
    pub(crate) const HOVER: f32 = 0.12;
    /// Menus, infobulles, cartes, popovers, pastilles.
    pub(crate) const POP: f32 = 0.14;
    /// Fenêtres modales et pages plein écran.
    pub(crate) const WINDOW: f32 = 0.16;
    /// Changement de forme (rail, pilule qui écrit, interrupteur).
    pub(crate) const MORPH: f32 = 0.18;
    /// Arrivée d'un message.
    pub(crate) const MESSAGE: f32 = 0.2;
    /// Pulsation d'un message qui mentionne.
    pub(crate) const PULSE: f32 = 0.9;
    /// Fin du surlignage d'un message visé.
    pub(crate) const FLASH_FADE: f64 = 0.5;
    /// Vague des trois points d'écriture.
    pub(crate) const DOTS_PERIOD: f64 = 1.2;
    /// Cadence des animations continues (points, squelettes) : 30 images par seconde.
    pub(crate) const FRAME_MS: u64 = 33;
}

/// Échelle de départ d'un élément qui apparaît (menu, fenêtre, pastille).
pub(crate) const POP_SCALE: f32 = 0.95;
/// « Pop » d'une pastille : échelle de départ et dépassement.
pub(crate) const POP_START: f32 = 0.6;
pub(crate) const POP_OVER: f32 = 0.12;
/// Glissement de départ d'un message ou d'un bouton qui arrive (points).
pub(crate) const SLIDE: f32 = 8.0;
/// Grossissement d'un bouton à icône survolé (envoi, emoji).
pub(crate) const GROW: f32 = 1.12;
/// Opacité d'un message en cours d'envoi.
pub(crate) const SENDING: f32 = 0.5;

/// Courbe de toutes les transitions : départ vif, arrivée douce.
pub(crate) fn ease_out(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    1.0 - (1.0 - x).powi(3)
}

pub(crate) fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    if k <= 0.0 {
        return a;
    }
    if k >= 1.0 {
        return b;
    }
    let f = |x: u8, y: u8| egui::lerp(f32::from(x)..=f32::from(y), k).round() as u8;
    Color32::from_rgba_premultiplied(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()), f(a.a(), b.a()))
}

pub(crate) fn mix_radius(a: CornerRadius, b: CornerRadius, k: f32) -> CornerRadius {
    let f = |x: u8, y: u8| egui::lerp(f32::from(x)..=f32::from(y), k.clamp(0.0, 1.0)).round() as u8;
    CornerRadius { nw: f(a.nw, b.nw), ne: f(a.ne, b.ne), sw: f(a.sw, b.sw), se: f(a.se, b.se) }
}

/// Mise à l'échelle `s` autour de `origin`.
pub(crate) fn scale_about(origin: Pos2, s: f32) -> TSTransform {
    TSTransform::new(origin.to_vec2() * (1.0 - s), s)
}

/// Échelle d'un « pop » (pastille, réaction) à l'avancement `k` : part de
/// `POP_START`, dépasse un peu, revient à 1. Cran discret : pas d'échelle.
pub(crate) fn pop_scale(ctx: &egui::Context, center: Pos2, k: f32) -> TSTransform {
    if k >= 1.0 || level(ctx) != Level::Full {
        return TSTransform::IDENTITY;
    }
    let s = POP_START + (1.0 - POP_START) * k + POP_OVER * (std::f32::consts::PI * k).sin();
    scale_about(center, s)
}

pub(crate) fn level(ctx: &egui::Context) -> Level {
    get(ctx).level
}

/// De 0 à 1 vers `on`, en `secs` : survol, sélection, interrupteur. S'arrête seule.
pub(crate) fn toward(ctx: &egui::Context, id: Id, on: bool, secs: f32) -> f32 {
    if level(ctx) == Level::None {
        return if on { 1.0 } else { 0.0 };
    }
    ctx.animate_bool_with_time_and_easing(id.with("anim"), on, secs, ease_out)
}

/// Fondu de survol.
pub(crate) fn hover(ctx: &egui::Context, id: Id, hot: bool) -> f32 {
    toward(ctx, id, hot, dur::HOVER)
}

/// Valeur qui rejoint `target` en `secs` (hauteur d'une marque du rail).
pub(crate) fn value(ctx: &egui::Context, id: Id, target: f32, secs: f32) -> f32 {
    if level(ctx) == Level::None {
        return target;
    }
    ctx.animate_value_with_time(id.with("valeur"), target, secs)
}

#[derive(Clone, Copy)]
struct Seen {
    start: f64,
    pass: u64,
    value: u64,
    /// Valeur d'avant le dernier changement (compteur qui glisse).
    prev: u64,
    /// Pas encore montré : l'horloge part à l'image suivante.
    fresh: bool,
}

/// Dernier dessin d'une zone flottante ouverte (menu, fenêtre, infobulle),
/// rejoué en fondu quand elle se ferme.
#[derive(Clone)]
struct Ghost {
    shapes: Vec<(Rect, Shape)>,
    origin: Pos2,
    /// Formes du début qui ne changent pas d'échelle (voile).
    skip: usize,
    pass: u64,
    /// Instant de la fermeture.
    closed: Option<f64>,
}

/// Survol d'une liste : la ligne survolée (clé, départ) et celles qu'on
/// vient de quitter (clé, instant, force). Borné : `MAX_LEFT` lignes.
#[derive(Clone, Default)]
struct Trail {
    hot: Option<(u64, f64)>,
    left: Vec<(u64, f64, f32)>,
}

const MAX_LEFT: usize = 4;
/// Au-delà, une zone se ferme sans animation (rien n'est copié).
const MAX_GHOST_SHAPES: usize = 3000;

/// Pilule d'écriture ouverte ou en train de se refermer.
#[derive(Clone, Copy)]
struct Pill {
    on: bool,
    at: f64,
    from: f32,
    pass: u64,
}

#[derive(Clone, Copy)]
struct Pop {
    layer: LayerId,
    skip: usize,
    alpha: f32,
    tf: TSTransform,
}

/// Animations en cours. Borné : `seen` ne garde que ce qui a été dessiné à
/// la passe d'avant, `shots` au plus `MAX_SHOTS` départs.
#[derive(Clone, Default)]
struct Clock {
    seen: HashMap<Id, Seen>,
    shots: Vec<(Id, f64)>,
    pops: Vec<Pop>,
    pills: HashMap<Id, Pill>,
    newest: Option<(u64, u64)>,
    hooked: bool,
    ghosts: HashMap<LayerId, Ghost>,
    trails: HashMap<Id, Trail>,
}

const MAX_SHOTS: usize = 32;
/// Avance donnée à une apparition à sa deuxième image (une image à 60 Hz).
const FIRST_FRAME: f64 = 1.0 / 60.0;

fn clock_id() -> Id {
    Id::unique("ferrocord.motion.horloge")
}

fn with_clock<R>(ctx: &egui::Context, f: impl FnOnce(&mut Clock) -> R) -> R {
    ctx.data_mut(|d| f(d.get_temp_mut_or_default::<Clock>(clock_id())))
}

/// Avancement de 0 à 1 ; tant qu'il n'est pas fini, demande l'image suivante.
fn progress(ctx: &egui::Context, now: f64, start: f64, secs: f32) -> f32 {
    let p = ((now - start) / f64::from(secs.max(0.001))) as f32;
    if p < 1.0 {
        ctx.request_repaint();
    }
    p.clamp(0.0, 1.0)
}

/// Ménage de fin de passe : oublie ce qui n'est plus dessiné, applique les apparitions de couche.
fn hook(ctx: &egui::Context) {
    if with_clock(ctx, |c| !std::mem::replace(&mut c.hooked, true)) {
        ctx.on_end_pass("ferrocord-animations", std::sync::Arc::new(|ui: &mut Ui| finish(ui.ctx())));
    }
}

/// De 0 à 1 depuis la première image d'une suite d'images où l'élément est
/// dessiné : repart si l'élément a cessé d'être dessiné (menu rouvert).
pub(crate) fn appear(ctx: &egui::Context, id: Id, secs: f32) -> f32 {
    if level(ctx) == Level::None {
        return 1.0;
    }
    hook(ctx);
    let (now, pass) = (ctx.input(|i| i.time), ctx.cumulative_pass_nr());
    let start = with_clock(ctx, |c| {
        let s = c.seen.entry(id).or_insert(Seen { start: now, pass, value: 0, prev: 0, fresh: true });
        if s.pass + 1 < pass {
            (s.start, s.fresh) = (now, true);
        } else if s.fresh && now > s.start {
            // La première image d'un écran lourd (fenêtre, menu) peut durer plus que
            // l'animation : l'horloge part quand cette image est montrée, pas avant.
            (s.start, s.fresh) = (now - FIRST_FRAME, false);
        }
        s.pass = pass;
        s.start
    });
    ease_out(progress(ctx, now, start, secs))
}

/// De 0 à 1 depuis le dernier changement de `value`, 1 au repos. Un élément
/// qui entre dans la vue ne compte pas comme un changement.
pub(crate) fn watch(ctx: &egui::Context, id: Id, value: u64, secs: f32) -> f32 {
    changed(ctx, id, value, secs).0
}

/// Comme [`watch`], avec la valeur d'avant le changement (compteur qui glisse).
pub(crate) fn changed(ctx: &egui::Context, id: Id, value: u64, secs: f32) -> (f32, u64) {
    if level(ctx) == Level::None {
        return (1.0, value);
    }
    hook(ctx);
    let (now, pass) = (ctx.input(|i| i.time), ctx.cumulative_pass_nr());
    let (start, prev) = with_clock(ctx, |c| {
        let s = c.seen.entry(id).or_insert(Seen { start: f64::NEG_INFINITY, pass, value, prev: value, fresh: false });
        if s.pass + 1 < pass {
            (s.start, s.value, s.prev) = (f64::NEG_INFINITY, value, value);
        } else if s.value != value {
            (s.start, s.prev, s.value) = (now, s.value, value);
        }
        s.pass = pass;
        (s.start, s.prev)
    });
    if start == f64::NEG_INFINITY {
        return (1.0, value);
    }
    (ease_out(progress(ctx, now, start, secs)), prev)
}

/// Fondu de survol d'une ligne `key` d'une liste `group` dont les lignes
/// sont trop nombreuses pour garder un état chacune (messages) : seules la
/// ligne survolée et les dernières quittées sont suivies.
pub(crate) fn trail(ctx: &egui::Context, group: Id, key: u64, hot: bool) -> f32 {
    if level(ctx) == Level::None {
        return if hot { 1.0 } else { 0.0 };
    }
    let now = ctx.input(|i| i.time);
    let span = f64::from(dur::HOVER);
    let fading = |at: f64, from: f32| from * (1.0 - ((now - at) / span).clamp(0.0, 1.0) as f32);
    let (k, moving) = with_clock(ctx, |c| {
        let t = c.trails.entry(group).or_default();
        if hot {
            if t.hot.is_none_or(|(k, _)| k != key) {
                if let Some((old, start)) = t.hot.take() {
                    if t.left.len() >= MAX_LEFT {
                        t.left.remove(0);
                    }
                    t.left.push((old, now, ((now - start) / span).clamp(0.0, 1.0) as f32));
                }
                // Ligne retrouvée avant la fin de son fondu : elle repart d'où elle en était.
                let from = t.left.iter().position(|l| l.0 == key).map_or(0.0, |i| {
                    let (_, at, from) = t.left.remove(i);
                    fading(at, from)
                });
                t.hot = Some((key, now - f64::from(from) * span));
            }
            let k = t.hot.map_or(1.0, |(_, start)| ((now - start) / span).clamp(0.0, 1.0) as f32);
            return (k, k < 1.0);
        }
        if let Some((_, start)) = t.hot.filter(|(k, _)| *k == key) {
            t.hot = None;
            if t.left.len() >= MAX_LEFT {
                t.left.remove(0);
            }
            t.left.push((key, now, ((now - start) / span).clamp(0.0, 1.0) as f32));
        }
        let Some(i) = t.left.iter().position(|l| l.0 == key) else { return (0.0, false) };
        let k = fading(t.left[i].1, t.left[i].2);
        if k <= 0.0 {
            t.left.remove(i);
        }
        (k, k > 0.0)
    });
    if moving {
        ctx.request_repaint();
    }
    k
}

/// Lance une animation jouée une fois (message arrivé, mention).
pub(crate) fn kick(ctx: &egui::Context, id: Id) {
    if level(ctx) == Level::None {
        return;
    }
    let now = ctx.input(|i| i.time);
    with_clock(ctx, |c| {
        c.shots.retain(|(i, _)| *i != id);
        if c.shots.len() >= MAX_SHOTS {
            c.shots.remove(0);
        }
        c.shots.push((id, now));
    });
    ctx.request_repaint();
}

/// Avancement (linéaire, de 0 à 1) d'une animation lancée par [`kick`] ; 1 si
/// elle est finie ou n'a jamais été lancée.
pub(crate) fn played(ctx: &egui::Context, id: Id, secs: f32) -> f32 {
    let now = ctx.input(|i| i.time);
    let start = with_clock(ctx, |c| {
        let at = c.shots.iter().position(|(i, _)| *i == id)?;
        let start = c.shots[at].1;
        if now - start >= f64::from(secs) {
            c.shots.remove(at);
            return None;
        }
        Some(start)
    });
    start.map_or(1.0, |s| progress(ctx, now, s, secs))
}

/// Dernier message du salon `channel` : s'il vient de changer pendant qu'on
/// regarde le bas du fil, son arrivée est animée.
pub(crate) fn newest(ctx: &egui::Context, channel: u64, message: u64, pings: bool) {
    let fresh = with_clock(ctx, |c| {
        let fresh = c.newest.is_some_and(|(ch, m)| ch == channel && message > m);
        c.newest = Some((channel, message));
        fresh
    });
    if fresh {
        kick(ctx, message_id(message));
        if pings {
            kick(ctx, pulse_id(message));
        }
    }
}

fn pulse_id(message: u64) -> Id {
    Id::unique(("ferrocord.motion.mention", message))
}

/// Force (de 0 à 1) du fond d'un message qui vient d'arriver et nous
/// mentionne : monte puis retombe, une seule fois.
pub(crate) fn pulse(ctx: &egui::Context, message: u64) -> f32 {
    let p = played(ctx, pulse_id(message), dur::PULSE);
    if p >= 1.0 { 0.0 } else { (std::f32::consts::PI * p).sin() }
}

/// Entrée d'un élément à l'avancement linéaire `p` : opacité, et glissement
/// vers le haut au cran complet.
pub(crate) fn slide_in(ctx: &egui::Context, p: f32) -> (f32, TSTransform) {
    let k = ease_out(p);
    let tf = if k < 1.0 && level(ctx) == Level::Full { TSTransform::from_translation(egui::vec2(0.0, (1.0 - k) * SLIDE)) } else { TSTransform::IDENTITY };
    (k, tf)
}

pub(crate) fn message_id(message: u64) -> Id {
    Id::unique(("ferrocord.motion.message", message))
}

/// Suite de formes d'une couche, à fondre ou déplacer après coup : ce qui
/// est dessiné entre [`Span::begin`] et [`Span::end`].
pub(crate) struct Span {
    layer: LayerId,
    from: ShapeIdx,
}

impl Span {
    pub(crate) fn begin(ui: &Ui) -> Span {
        Span { layer: ui.layer_id(), from: ui.painter().add(Shape::Noop) }
    }

    pub(crate) fn end(self, ui: &Ui, alpha: f32, tf: TSTransform) {
        if alpha >= 1.0 && tf == TSTransform::IDENTITY {
            return;
        }
        let to = ui.painter().add(Shape::Noop);
        apply(ui.ctx(), self.layer, self.from.0, Some(to.0), alpha, tf);
    }
}

fn apply(ctx: &egui::Context, layer: LayerId, from: usize, to: Option<usize>, alpha: f32, tf: TSTransform) {
    ctx.graphics_mut(|g| {
        let Some(list) = g.get_mut(layer) else { return };
        let to = to.unwrap_or(list.next_idx().0).min(list.next_idx().0);
        if from >= to {
            return;
        }
        if alpha < 1.0 {
            for i in from..to {
                list.mutate_shape(ShapeIdx(i), |s| {
                    egui::epaint::shape_transform::adjust_colors(&mut s.shape, move |c| {
                        if *c != Color32::PLACEHOLDER {
                            *c = c.gamma_multiply(alpha);
                        }
                    });
                });
            }
        }
        if tf != TSTransform::IDENTITY {
            list.transform_range(ShapeIdx(from), ShapeIdx(to), tf);
        }
    });
}

/// Apparition d'une couche entière (menu, infobulle, carte, fenêtre) : fondu
/// et, au cran complet, échelle depuis `origin`. `skip` : formes du début de
/// la couche qui ne changent pas d'échelle (voile d'une fenêtre modale).
/// Appliqué en fin de passe, quand la couche est complète.
pub(crate) fn pop(ctx: &egui::Context, layer: LayerId, key: Id, origin: Pos2, skip: usize, secs: f32) {
    let p = appear(ctx, key, secs);
    if p >= 1.0 {
        return;
    }
    let tf = if level(ctx) == Level::Full { scale_about(origin, egui::lerp(POP_SCALE..=1.0, p)) } else { TSTransform::IDENTITY };
    with_clock(ctx, |c| {
        if !c.pops.iter().any(|x| x.layer == layer) {
            c.pops.push(Pop { layer, skip, alpha: p, tf });
        }
    });
}

/// Apparition de la fenêtre plein écran qui contient `ui` (paramètres) : zoom depuis le centre.
pub(crate) fn pop_window(ui: &Ui) {
    let layer = ui.layer_id();
    pop(ui.ctx(), layer, layer_key(layer), ui.ctx().content_rect().center(), 0, dur::WINDOW);
}

fn layer_key(layer: LayerId) -> Id {
    Id::unique(("ferrocord.motion.couche", layer))
}

/// Toute zone flottante qui vient d'apparaître (menu contextuel, popover,
/// carte de profil, fenêtre modale, infobulle d'egui) : fondu et échelle,
/// sans rien demander aux écrans. Le voile d'une fenêtre modale (première
/// forme, grande comme l'écran) se fond sans changer d'échelle.
fn pop_new_layers(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let layers: Vec<(LayerId, Option<Rect>)> = ctx.memory(|m| {
        let floating = |l: &LayerId| matches!(l.order, egui::Order::Foreground | egui::Order::Tooltip);
        m.areas().visible_layer_ids().into_iter().filter(floating).map(|l| (l, m.area_rect(l.id))).collect()
    });
    for (layer, rect) in layers {
        let veil = ctx.graphics(|g| {
            let list = g.get(layer).filter(|l| !l.is_empty())?;
            Some(list.all_entries().next().is_some_and(|s| matches!(&s.shape, Shape::Rect(r) if r.rect.contains_rect(screen.shrink(1.0)))))
        });
        // Rien de dessiné à cette passe : la zone est fermée.
        let Some(veil) = veil else { continue };
        let origin = if veil { screen.center() } else { rect.map_or(screen.center(), |r| r.center()) };
        remember(ctx, layer, origin, usize::from(veil));
        pop(ctx, layer, layer_key(layer), origin, usize::from(veil), if veil { dur::WINDOW } else { dur::POP });
    }
}

/// Garde le dessin de la zone `layer` de cette passe, pour sa fermeture.
fn remember(ctx: &egui::Context, layer: LayerId, origin: Pos2, skip: usize) {
    let shapes = ctx.graphics(|g| {
        let list = g.get(layer)?;
        let all = list.all_entries();
        (all.len() <= MAX_GHOST_SHAPES).then(|| all.map(|s| (s.clip_rect, s.shape.clone())).collect::<Vec<_>>())
    });
    let pass = ctx.cumulative_pass_nr();
    with_clock(ctx, |c| match shapes {
        Some(shapes) => {
            c.ghosts.insert(layer, Ghost { shapes, origin, skip, pass, closed: None });
        }
        None => {
            c.ghosts.remove(&layer);
        }
    });
}

/// Zones flottantes fermées depuis peu : leur dernier dessin s'efface en
/// fondu (et rétrécit un peu au cran complet), puis est oublié.
fn fade_closed(ctx: &egui::Context) {
    let (now, pass) = (ctx.input(|i| i.time), ctx.cumulative_pass_nr());
    let full = level(ctx) == Level::Full;
    let closing: Vec<(LayerId, Ghost, f32)> = with_clock(ctx, |c| {
        let mut out = Vec::new();
        c.ghosts.retain(|layer, g| {
            if g.pass == pass {
                return true;
            }
            let at = *g.closed.get_or_insert(now);
            let secs = if g.skip > 0 { dur::WINDOW } else { dur::POP };
            let p = ((now - at) / f64::from(secs)) as f32;
            if p < 1.0 {
                out.push((*layer, g.clone(), ease_out(p)));
            }
            p < 1.0
        });
        out
    });
    for (layer, g, k) in closing {
        // La zone a été redessinée par quelqu'un d'autre : rien à rejouer.
        if ctx.graphics(|gr| gr.get(layer).is_some_and(|l| !l.is_empty())) {
            continue;
        }
        let painter = ctx.layer_painter(layer);
        for (clip, shape) in g.shapes {
            painter.with_clip_rect(clip).add(shape);
        }
        let tf = if full { scale_about(g.origin, egui::lerp(1.0..=POP_SCALE, k)) } else { TSTransform::IDENTITY };
        apply(ctx, layer, 0, Some(g.skip), 1.0 - k, TSTransform::IDENTITY);
        apply(ctx, layer, g.skip, None, 1.0 - k, tf);
        ctx.request_repaint();
    }
}

fn finish(ctx: &egui::Context) {
    if level(ctx) != Level::None {
        pop_new_layers(ctx);
        fade_closed(ctx);
    } else {
        with_clock(ctx, |c| c.ghosts.clear());
    }
    let pass = ctx.cumulative_pass_nr();
    let pops = with_clock(ctx, |c| {
        c.seen.retain(|_, s| s.pass + 1 >= pass);
        c.pills.retain(|_, p| p.pass + 1 >= pass);
        std::mem::take(&mut c.pops)
    });
    for p in pops {
        // Le voile garde sa place : seul le fondu s'y applique.
        apply(ctx, p.layer, 0, Some(p.skip), p.alpha, TSTransform::IDENTITY);
        apply(ctx, p.layer, p.skip, None, p.alpha, p.tf);
    }
}

/// Trois points qui rebondissent en vague dans `rect` (indicateur
/// d'écriture). Ne bat que tant qu'il est dessiné, à 30 images par seconde.
pub(crate) fn dots(ui: &Ui, rect: Rect, color: Color32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let lv = level(ui.ctx());
    let d = (rect.height() * 0.42).min(rect.width() / 4.0);
    let step = d * 1.5;
    let now = ui.input(|i| i.time);
    let p = ui.painter();
    for i in 0..3u8 {
        let wave = if lv == Level::None { 0.6 } else { dot_wave(now, i) };
        let lift = if lv == Level::Full { wave * d * 0.5 } else { 0.0 };
        let grow = if lv == Level::Full { 0.85 + 0.15 * wave } else { 1.0 };
        let c = egui::pos2(rect.center().x + (f32::from(i) - 1.0) * step, rect.center().y + d * 0.2 - lift);
        p.circle_filled(c, d / 2.0 * grow, color.gamma_multiply(0.45 + 0.55 * wave));
    }
    if lv != Level::None {
        ui.ctx().request_repaint_after(Duration::from_millis(dur::FRAME_MS));
    }
}

/// Hauteur (de 0 à 1) du point `i` à l'instant `now` : une bosse par période, décalée d'un point à l'autre.
fn dot_wave(now: f64, i: u8) -> f32 {
    let phase = ((now / dur::DOTS_PERIOD) - f64::from(i) * 0.16).rem_euclid(1.0) as f32;
    if phase < 0.6 { (std::f32::consts::PI * phase / 0.6).sin() } else { 0.0 }
}

/// Ouverture (de 0 à 1) d'une pilule d'écriture. Rien n'est gardé pour qui
/// n'écrit pas : la liste des membres peut être longue.
fn morph(ctx: &egui::Context, key: Id, on: bool) -> f32 {
    if level(ctx) == Level::None {
        return if on { 1.0 } else { 0.0 };
    }
    let (now, pass) = (ctx.input(|i| i.time), ctx.cumulative_pass_nr());
    let (m, moving) = with_clock(ctx, |c| {
        let Some(p) = c.pills.get_mut(&key) else {
            if on {
                c.pills.insert(key, Pill { on, at: now, from: 0.0, pass });
            }
            return (0.0, on);
        };
        p.pass = pass;
        let k = ((now - p.at) / f64::from(dur::MORPH)).clamp(0.0, 1.0) as f32;
        let m = egui::lerp(p.from..=if p.on { 1.0 } else { 0.0 }, ease_out(k));
        if p.on != on {
            (p.on, p.at, p.from) = (on, now, m);
            return (m, true);
        }
        if !on && k >= 1.0 {
            c.pills.remove(&key);
            return (0.0, false);
        }
        (m, k < 1.0)
    });
    if moving {
        hook(ctx);
        ctx.request_repaint();
    }
    m
}

/// Largeur de la pilule d'écriture ouverte et son débord à droite de la pastille, en rayons.
const PILL_WIDTH: f32 = 4.4;
const PILL_OUT: f32 = 1.0;

/// Pastille de statut qui s'élargit en pilule à trois points tant que la
/// personne écrit, puis revient. `dot` : centre, rayon et anneau de la pastille.
pub(crate) fn typing_pill(ui: &Ui, key: Id, on: bool, dot: (Pos2, f32, f32), fill: Color32, ring: Color32) {
    let m = morph(ui.ctx(), key, on);
    if m <= 0.0 {
        return;
    }
    let (c, r, w) = dot;
    // La pilule déborde un peu à droite de l'avatar et reste courte : plus large et
    // toute vers la gauche, elle rognait le bas des initiales (« E » lu « F »).
    let wide = egui::lerp((2.0 * r)..=(PILL_WIDTH * r), m);
    let right = c.x + r + PILL_OUT * r * m;
    let rect = Rect::from_min_max(egui::pos2(right - wide, c.y - r), egui::pos2(right, c.y + r));
    let p = ui.painter();
    p.rect_filled(rect.expand(w), r + w, ring);
    p.rect_filled(rect, r, fill);
    if on && m > 0.5 {
        dots(ui, rect.shrink2(egui::vec2(r * 0.5, r * 0.25)), ring.gamma_multiply(m));
    }
}

/// Qui écrit en ce moment : (salon, personne). Posé par `App` quand la liste
/// change ; lu par les pastilles de statut.
#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct Typists {
    pub(crate) open_channel: u64,
    pub(crate) list: std::sync::Arc<[(u64, u64)]>,
}

impl Typists {
    /// Écrit dans le salon ouvert.
    pub(crate) fn here(&self, user: u64) -> bool {
        self.in_channel(self.open_channel, user)
    }

    /// Écrit dans le salon `channel` (ligne d'un MP).
    pub(crate) fn in_channel(&self, channel: u64, user: u64) -> bool {
        self.list.iter().any(|(c, u)| *c == channel && *u == user)
    }
}

fn typists_id() -> Id {
    Id::unique("ferrocord.motion.écrivent")
}

pub(crate) fn set_typists(ctx: &egui::Context, open_channel: u64, list: impl Iterator<Item = (u64, u64)>) {
    let old = typists(ctx);
    let list: Vec<(u64, u64)> = list.collect();
    if old.open_channel != open_channel || *old.list != *list {
        ctx.data_mut(|d| d.insert_temp(typists_id(), Typists { open_channel, list: list.into() }));
    }
}

pub(crate) fn typists(ctx: &egui::Context) -> Typists {
    ctx.data(|d| d.get_temp::<Typists>(typists_id())).unwrap_or_default()
}

/// Squelette d'un historique en cours de chargement : `rows` messages en
/// blocs gris, qui respirent tant qu'ils sont visibles (cran complet).
pub(crate) fn skeleton(ui: &mut Ui, block: Color32, avatar: f32, rows: usize) {
    let width = ui.available_width();
    let line = (avatar * 0.3).round().max(6.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, rows as f32 * (avatar + line)), egui::Sense::hover());
    if !ui.is_rect_visible(rect) {
        return;
    }
    let breath = if level(ui.ctx()) == Level::Full {
        ui.ctx().request_repaint_after(Duration::from_millis(dur::FRAME_MS));
        let t = (ui.input(|i| i.time) / 1.6).rem_euclid(1.0) as f32;
        0.7 + 0.3 * (std::f32::consts::TAU * t).sin().abs()
    } else {
        1.0
    };
    let color = block.gamma_multiply(breath);
    let p = ui.painter();
    for i in 0..rows {
        let top = rect.top() + i as f32 * (avatar + line);
        let left = rect.left() + line;
        p.circle_filled(egui::pos2(left + avatar / 2.0, top + avatar / 2.0), avatar / 2.0, color);
        let x = left + avatar + line;
        let room = (rect.right() - x - line).max(0.0);
        // Largeurs variées, mais fixes : le squelette ne scintille pas.
        let (name, text) = ([0.22, 0.3, 0.18][i % 3], [0.7, 0.45, 0.85][i % 3]);
        p.rect_filled(Rect::from_min_size(egui::pos2(x, top + line * 0.3), egui::vec2(room * name, line)), line / 2.0, color);
        p.rect_filled(Rect::from_min_size(egui::pos2(x, top + line * 1.9), egui::vec2(room * text, line)), line / 2.0, color);
    }
}

/// Quand une image animée bouge.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Play {
    /// Jamais : première image.
    #[default]
    Never,
    /// Sous le pointeur seulement.
    Hover,
    Always,
}

impl Play {
    pub(super) fn now(self, hovered: bool) -> bool {
        match self {
            Play::Never => false,
            Play::Hover => hovered,
            Play::Always => true,
        }
    }

    /// « Réduire les animations » : rien ne bouge sans le pointeur dessus.
    fn reduced(self, reduced: bool) -> Play {
        if reduced && self == Play::Always { Play::Hover } else { self }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Motion {
    pub emoji: Play,
    pub stickers: Play,
    pub gifs: Play,
    /// Avatars et icônes animés : au survol, comme l'application officielle.
    pub avatars: Play,
    /// Intensité des animations de l'interface (lot ANIM).
    pub level: Level,
}

impl Motion {
    /// Réglages pas encore reçus : valeurs par défaut de Discord (tout
    /// joue), sauf animations réduites.
    pub(super) fn from_settings(s: &AccountState, reduced: bool) -> Motion {
        let on = |k: Key| !matches!(s.value(k), Some(SValue::Bool(false)));
        let stickers = match s.value(Key::AnimateStickers) {
            Some(SValue::U32(1)) => Play::Hover,
            Some(SValue::U32(2)) => Play::Never,
            _ => Play::Always,
        };
        let pick = |b: bool| if b { Play::Always } else { Play::Hover };
        Motion {
            emoji: pick(on(Key::AnimateEmoji)).reduced(reduced),
            stickers: stickers.reduced(reduced),
            gifs: pick(on(Key::GifAutoPlay)).reduced(reduced),
            avatars: Play::Hover,
            level: Level::None,
        }
    }
}

impl Motion {
    pub(crate) fn with_level(mut self, level: Level) -> Motion {
        self.level = level;
        self
    }
}

fn id() -> Id {
    Id::unique("ferrocord.motion")
}

pub(super) fn set(ctx: &egui::Context, m: Motion) {
    ctx.data_mut(|d| d.insert_temp(id(), m));
    if m.level != Level::None {
        hook(ctx);
    }
}

/// Réglage de l'image en cours ; jamais posé (tests) : rien ne bouge.
pub(super) fn get(ctx: &egui::Context) -> Motion {
    ctx.data(|d| d.get_temp::<Motion>(id())).unwrap_or_default()
}

/// Le pointeur est-il sur `rect` (dans la couche de `ui`) ?
pub(super) fn hovered(ui: &egui::Ui, rect: egui::Rect) -> bool {
    ui.rect_contains_pointer(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx_at(level: Level) -> egui::Context {
        let ctx = egui::Context::default();
        set(&ctx, Motion::default().with_level(level));
        // Les premières images d'un contexte neuf en demandent une autre (polices, mise en place).
        for _ in 0..3 {
            let _ = frame(&ctx, 0.0, |_| {});
        }
        ctx
    }

    /// Une image à l'instant `time` ; rend le délai avant la prochaine image demandée.
    fn frame(ctx: &egui::Context, time: f64, f: impl FnMut(&mut Ui)) -> Duration {
        let mut out = ctx.run_ui(egui::RawInput { time: Some(time), ..Default::default() }, f);
        out.textures_delta.clear();
        out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap_or(Duration::MAX)
    }

    const REST: Duration = Duration::from_secs(3600);

    #[test]
    fn level_follows_the_setting_and_reduced_motion() {
        assert_eq!(Level::of(Intensity::Full, false), Level::Full);
        assert_eq!(Level::of(Intensity::Subtle, false), Level::Subtle);
        assert_eq!(Level::of(Intensity::None, false), Level::None);
        assert_eq!(Level::of(Intensity::Full, true), Level::None, "réduites : tout est coupé");
        assert_eq!((ease_out(0.0), ease_out(1.0)), (0.0, 1.0));
        assert!(ease_out(0.5) > 0.5, "départ vif");
    }

    /// Chaque animation finie ne demande plus d'image : survol, apparition, valeur surveillée, départ unique.
    #[test]
    fn every_animation_stops() {
        let ctx = ctx_at(Level::Full);
        let id = Id::unique("essai");
        // Délai avant la prochaine image, puis (survol, apparition, valeur surveillée, départ unique).
        let run = |time: f64, hot: bool, value: u64| {
            let mut got = (0.0, 0.0, 0.0, 0.0);
            let d = frame(&ctx, time, |ui| {
                got = (hover(ui.ctx(), id, hot), appear(ui.ctx(), id.with(1), dur::POP), watch(ui.ctx(), id.with(2), value, dur::POP), played(ui.ctx(), id.with(3), dur::MESSAGE));
            });
            (d, got)
        };
        // Première image : seul ce qui apparaît bouge.
        assert!(run(0.0, false, 1).0 < REST, "apparition en cours");
        // Première image longue : l'apparition n'est pas sautée, elle part à l'image suivante.
        let (d, seen) = run(0.5, false, 1);
        assert!(d < REST && seen.1 > 0.0 && seen.1 < 1.0, "pas sautée : {}", seen.1);
        // egui redessine une fois de plus après chaque image demandée : le repos se lit à la suivante.
        let _ = run(0.8, false, 1);
        let (d, seen) = run(0.9, false, 1);
        assert!(d >= REST, "apparition finie : repos");
        assert_eq!(seen, (0.0, 1.0, 1.0, 1.0));
        // Survol : bouge, puis s'arrête.
        assert!(run(1.0, true, 1).0 < REST);
        let seen = run(1.05, true, 1).1;
        assert!(seen.0 > 0.0 && seen.0 < 1.0, "fondu en cours : {}", seen.0);
        let _ = run(1.5, true, 1);
        let (d, seen) = run(1.6, true, 1);
        assert!(d >= REST, "survol atteint : repos");
        assert_eq!(seen.0, 1.0);
        // Valeur qui change : pop, puis repos.
        let (d, seen) = run(2.0, true, 2);
        assert!(d < REST && seen.2 < 1.0);
        let _ = run(2.4, true, 2);
        let (d, seen) = run(2.5, true, 2);
        assert!(d >= REST);
        assert_eq!(seen.2, 1.0);
        // Départ unique.
        let _ = run(3.0, true, 2);
        kick(&ctx, id.with(3));
        let (d, seen) = run(3.05, true, 2);
        assert!(d < REST && seen.3 > 0.0 && seen.3 < 1.0, "{}", seen.3);
        let seen = run(3.0 + f64::from(dur::MESSAGE) + 0.01, true, 2).1;
        assert_eq!(seen.3, 1.0);
        assert!(run(4.0, true, 2).0 >= REST, "tout est fini : aucune image demandée");
    }

    /// Cran « aucune » : état final tout de suite, jamais d'image demandée, points d'écriture figés.
    #[test]
    fn level_none_never_asks_for_a_frame() {
        let ctx = ctx_at(Level::None);
        let id = Id::unique("essai");
        kick(&ctx, id);
        for time in [0.0, 0.01] {
            let d = frame(&ctx, time, |ui| {
                assert_eq!(hover(ui.ctx(), id, true), 1.0);
                assert_eq!(appear(ui.ctx(), id, dur::POP), 1.0);
                assert_eq!(watch(ui.ctx(), id, time as u64 + 7, dur::POP), 1.0);
                assert_eq!(played(ui.ctx(), id, dur::MESSAGE), 1.0);
                assert_eq!(value(ui.ctx(), id, 3.0, dur::MORPH), 3.0);
                dots(ui, Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(24.0, 12.0)), Color32::WHITE);
            });
            assert!(d >= REST);
        }
    }

    /// Les points battent à 30 images par seconde tant qu'ils sont dessinés, et plus du tout ensuite.
    #[test]
    fn dots_tick_only_while_drawn() {
        let ctx = ctx_at(Level::Full);
        let rect = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(24.0, 12.0));
        let d = frame(&ctx, 0.0, |ui| dots(ui, rect, Color32::WHITE));
        assert_eq!(d, Duration::from_millis(dur::FRAME_MS));
        assert!(frame(&ctx, 0.1, |_| {}) >= REST, "personne n'écrit : aucune image");
        // Hors de la vue : rien non plus.
        let away = rect.translate(egui::vec2(0.0, 1.0e6));
        assert!(frame(&ctx, 0.2, |ui| dots(ui, away, Color32::WHITE)) >= REST);
        // La vague passe d'un point à l'autre.
        assert!(dot_wave(0.3, 0) > dot_wave(0.3, 2));
        assert_eq!(dot_wave(1.1, 0), 0.0);
    }

    /// La pilule s'ouvre quand la personne écrit, bat, se referme, puis ne demande plus rien.
    #[test]
    fn typing_pill_opens_ticks_and_closes() {
        let ctx = ctx_at(Level::Full);
        let dot = (egui::pos2(60.0, 60.0), 5.0, 2.0);
        let key = Id::unique(42u64);
        let run = |time: f64, on: bool| frame(&ctx, time, |ui| typing_pill(ui, key, on, dot, Color32::GREEN, Color32::BLACK));
        assert!(run(0.0, false) >= REST, "n'écrit pas : rien");
        assert!(run(1.0, true) < REST, "s'ouvre");
        let _ = run(1.1, true);
        let _ = run(1.5, true);
        assert_eq!(run(1.6, true), Duration::from_millis(dur::FRAME_MS), "ouverte : seuls les points battent");
        assert!(run(2.0, false) < REST, "se referme");
        let _ = run(2.1, false);
        let _ = run(2.6, false);
        assert!(run(2.7, false) >= REST, "refermée : repos");
        assert!(with_clock(&ctx, |c| c.pills.is_empty()), "rien de gardé pour qui n'écrit plus");
    }

    /// Un message plus récent dans le même salon est animé ; un changement de salon, non.
    #[test]
    fn newest_message_is_kicked_once() {
        let ctx = ctx_at(Level::Full);
        let p = |m: u64| {
            let mut got = 1.0;
            let _ = frame(&ctx, 1.0, |ui| got = played(ui.ctx(), message_id(m), dur::MESSAGE));
            got
        };
        let _ = frame(&ctx, 1.0, |_| {});
        newest(&ctx, 1, 10, false);
        assert_eq!(p(10), 1.0, "premier affichage : pas d'animation");
        newest(&ctx, 1, 11, true);
        assert!(p(11) < 1.0);
        let mut strength = 0.0;
        let _ = frame(&ctx, 1.0 + f64::from(dur::PULSE) / 2.0, |ui| strength = pulse(ui.ctx(), 11));
        assert!(strength > 0.9, "mention : le fond pulse ({strength})");
        let _ = frame(&ctx, 1.0 + f64::from(dur::PULSE) + 0.05, |ui| strength = pulse(ui.ctx(), 11));
        let d = frame(&ctx, 1.0 + f64::from(dur::PULSE) + 0.1, |ui| strength = pulse(ui.ctx(), 11) + played(ui.ctx(), message_id(11), dur::MESSAGE) - 1.0);
        assert_eq!(strength, 0.0);
        assert!(d >= REST, "pulsation finie : repos");
        newest(&ctx, 2, 50, false);
        assert_eq!(p(50), 1.0, "autre salon");
        newest(&ctx, 2, 50, false);
        assert_eq!(p(50), 1.0, "même message");
    }

    /// Une zone flottante d'egui (menu, carte, fenêtre) apparaît en fondu sans que l'écran le demande, puis se tait.
    #[test]
    fn floating_areas_pop_on_their_own() {
        let show = |ctx: &egui::Context, time: f64| {
            frame(ctx, time, |ui| {
                egui::Area::new(Id::unique("carte")).order(egui::Order::Foreground).fixed_pos(egui::pos2(20.0, 20.0)).show(ui.ctx(), |ui| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(80.0, 40.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                });
            })
        };
        // Cran « aucune » : ce qu'egui demande de lui-même pour mettre la zone en place, rien de plus.
        let still = ctx_at(Level::None);
        let plain: Vec<bool> = [0.0, 0.05, 0.6, 0.7].iter().map(|t| show(&still, *t) < REST).collect();
        assert!(!plain[3], "zone en place : repos");
        let ctx = ctx_at(Level::Full);
        assert!(show(&ctx, 0.0) < REST, "apparaît");
        assert!(show(&ctx, 0.05) < REST, "fondu en cours");
        let _ = show(&ctx, 0.6);
        assert!(show(&ctx, 0.7) >= REST, "apparue : repos");
        assert!(with_clock(&ctx, |c| c.seen.len() == 1 && c.pops.is_empty()), "une seule zone suivie");
    }

    /// Survol d'une liste : la ligne quittée s'éteint pendant que la nouvelle
    /// s'allume, puis plus aucune image n'est demandée ; mémoire bornée.
    #[test]
    fn trail_fades_rows_then_rests() {
        let ctx = ctx_at(Level::Full);
        let group = Id::unique("liste");
        let half = f64::from(dur::HOVER) / 2.0;
        let run = |time: f64, hot: Option<u64>| {
            let mut got = [0.0_f32; 3];
            let d = frame(&ctx, time, |ui| {
                for (key, k) in got.iter_mut().enumerate() {
                    *k = trail(ui.ctx(), group, key as u64, hot == Some(key as u64));
                }
            });
            (d, got)
        };
        let _ = run(0.5, None);
        assert!(run(0.6, None).0 >= REST, "rien de survolé : repos");
        let (d, k) = run(1.0, Some(0));
        assert!(d < REST && k[0] == 0.0, "le fondu part de zéro : {k:?}");
        let k = run(1.0 + half, Some(0)).1;
        assert!(k[0] > 0.0 && k[0] < 1.0, "{k:?}");
        let _ = run(1.5, Some(0));
        let (d, k) = run(1.6, Some(0));
        assert!(d >= REST && k == [1.0, 0.0, 0.0], "survol atteint : repos ({k:?})");
        // La souris passe à la ligne suivante : deux fondus croisés.
        let (d, k) = run(2.0, Some(1));
        assert!(d < REST && k[0] == 1.0 && k[1] == 0.0, "{k:?}");
        let k = run(2.0 + half, Some(1)).1;
        assert!(k[0] > 0.0 && k[0] < 1.0 && k[1] > 0.0 && k[1] < 1.0, "{k:?}");
        let _ = run(2.5, Some(1));
        let (d, k) = run(2.6, Some(1));
        assert!(d >= REST && k == [0.0, 1.0, 0.0], "{k:?}");
        // Plus rien de survolé : la dernière s'éteint, puis tout se tait et s'oublie.
        assert!(run(3.0, None).0 < REST);
        let _ = run(3.5, None);
        let (d, k) = run(3.6, None);
        assert!(d >= REST && k == [0.0; 3], "{k:?}");
        assert!(with_clock(&ctx, |c| c.trails.get(&group).is_some_and(|t| t.hot.is_none() && t.left.is_empty())));
        // Beaucoup de lignes traversées vite : jamais plus de `MAX_LEFT` suivies.
        for key in 10..40_u64 {
            let _ = frame(&ctx, 4.0, |ui| {
                trail(ui.ctx(), group, key, true);
            });
        }
        assert!(with_clock(&ctx, |c| c.trails.get(&group).is_some_and(|t| t.left.len() <= MAX_LEFT)));
        // Cran « aucune » : tout de suite à l'état final, rien demandé.
        let still = ctx_at(Level::None);
        let d = frame(&still, 1.0, |ui| {
            assert_eq!((trail(ui.ctx(), group, 1, true), trail(ui.ctx(), group, 2, false)), (1.0, 0.0));
        });
        assert!(d >= REST);
    }

    /// Une zone flottante qui se ferme s'efface en fondu (son dernier dessin
    /// est rejoué), puis plus rien n'est demandé ni gardé.
    #[test]
    fn closed_areas_fade_out_then_rest() {
        let show = |ctx: &egui::Context, time: f64, open: bool| {
            frame(ctx, time, |ui| {
                if open {
                    egui::Area::new(Id::unique("carte")).order(egui::Order::Foreground).fixed_pos(egui::pos2(20.0, 20.0)).show(ui.ctx(), |ui| {
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(80.0, 40.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, Color32::WHITE);
                    });
                }
            })
        };
        let ghosts = |ctx: &egui::Context| with_clock(ctx, |c| (c.ghosts.len(), c.ghosts.values().filter(|g| g.closed.is_some()).count()));
        let ctx = ctx_at(Level::Full);
        for t in [0.0, 0.05, 0.6] {
            let _ = show(&ctx, t, true);
        }
        assert!(show(&ctx, 0.7, true) >= REST, "ouverte : repos");
        assert_eq!(ghosts(&ctx), (1, 0), "dernier dessin gardé");
        // Fermée : le fondu demande des images, le temps de `dur::POP`.
        assert!(show(&ctx, 1.0, false) < REST, "fermeture en cours");
        assert_eq!(ghosts(&ctx), (1, 1));
        assert!(show(&ctx, 1.0 + f64::from(dur::POP) / 2.0, false) < REST);
        let _ = show(&ctx, 2.0, false);
        assert!(show(&ctx, 2.1, false) >= REST, "effacée : repos");
        assert_eq!(ghosts(&ctx), (0, 0), "plus rien en mémoire");
        // Rouverte pendant sa fermeture : le fondu de fermeture est abandonné.
        for t in [3.0, 3.05, 3.6, 3.7] {
            let _ = show(&ctx, t, true);
        }
        let _ = show(&ctx, 4.0, false);
        let _ = show(&ctx, 4.02, true);
        assert_eq!(ghosts(&ctx), (1, 0));
        // Cran « aucune » : rien n'est gardé, la fermeture est immédiate.
        let still = ctx_at(Level::None);
        for t in [0.0, 0.05, 0.6, 0.7] {
            let _ = show(&still, t, true);
        }
        assert_eq!(ghosts(&still), (0, 0));
        let _ = show(&still, 1.0, false);
        assert!(show(&still, 1.1, false) >= REST);
    }

    /// Fondu et échelle d'une suite de formes : couleurs atténuées, formes déplacées, le reste intact.
    #[test]
    fn span_fades_and_moves_only_its_shapes() {
        let ctx = ctx_at(Level::Full);
        let r = Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(10.0, 10.0));
        let mut colors = Vec::new();
        let _ = frame(&ctx, 0.0, |ui| {
            ui.painter().rect_filled(r, 0.0, Color32::WHITE);
            let span = Span::begin(ui);
            ui.painter().rect_filled(r, 0.0, Color32::WHITE);
            span.end(ui, 0.5, TSTransform::from_translation(egui::vec2(0.0, SLIDE)));
            let layer = ui.layer_id();
            ui.ctx().graphics_mut(|g| {
                for s in g.entry(layer).all_entries() {
                    if let Shape::Rect(x) = &s.shape {
                        colors.push((x.fill, x.rect.top()));
                    }
                }
            });
        });
        assert_eq!(colors.len(), 2);
        assert_eq!(colors[0], (Color32::WHITE, 100.0));
        assert!(colors[1].0.a() < 200 && colors[1].1 == 100.0 + SLIDE, "{:?}", colors[1]);
        assert_eq!(scale_about(egui::pos2(10.0, 10.0), 0.5) * egui::pos2(10.0, 10.0), egui::pos2(10.0, 10.0));
    }

    /// Une couche qui apparaît se fond puis se tait ; la mémoire des éléments disparus est rendue.
    #[test]
    fn layer_pop_ends_and_forgets() {
        let ctx = ctx_at(Level::Full);
        let layer = LayerId::new(egui::Order::Foreground, Id::unique("menu"));
        let show = |time: f64, open: bool| {
            frame(&ctx, time, |ui| {
                if open {
                    ui.ctx().layer_painter(layer).rect_filled(Rect::from_min_size(egui::pos2(5.0, 5.0), egui::vec2(50.0, 50.0)), 0.0, Color32::WHITE);
                    pop(ui.ctx(), layer, Id::unique("menu"), egui::pos2(5.0, 5.0), 0, dur::POP);
                }
            })
        };
        assert!(show(0.0, true) < REST);
        assert!(show(0.05, true) < REST);
        let _ = show(0.5, true);
        assert!(show(0.6, true) >= REST, "apparu : repos");
        assert!(show(1.0, false) >= REST);
        assert!(show(1.1, false) >= REST);
        assert!(with_clock(&ctx, |c| c.seen.is_empty() && c.pops.is_empty()), "plus rien en mémoire");
        assert!(show(2.0, true) < REST, "rouvert : rejoué");
    }

    #[test]
    fn settings_and_reduced_motion() {
        let s = AccountState::default();
        let m = Motion::from_settings(&s, false);
        assert_eq!((m.emoji, m.stickers, m.gifs, m.avatars), (Play::Always, Play::Always, Play::Always, Play::Hover));
        let m = Motion::from_settings(&s, true);
        assert_eq!((m.emoji, m.stickers, m.gifs), (Play::Hover, Play::Hover, Play::Hover), "réduites : au survol seulement");
        assert!(!Play::Never.now(true) && Play::Hover.now(true) && !Play::Hover.now(false) && Play::Always.now(false));
        assert_eq!(get(&egui::Context::default()), Motion::default(), "jamais posé : rien ne bouge");
    }
}
