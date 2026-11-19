//! Dessin d'une image d'une animation Lottie : valeur de chaque propriété
//! à l'instant demandé, puis calques et formes rendus par `vello_cpu` (déjà
//! dans l'exécutable pour les glyphes en couleur).
//!
//! Rendu : calques de formes (tracés, ellipses, rectangles, étoiles,
//! remplissages, contours, dégradés, rognage des tracés), aplats,
//! précompositions, parenté, masques et caches (alpha et luminance).
//! Ignoré : images, texte, effets, modes de fusion, expressions,
//! répéteurs, coins arrondis et fusion de tracés.

use super::model::{Comp, Gradient, Item, Key, Kind, Layer, Line, Matte, PathData, PathProp, Position, Prop, Transform};
use vello_cpu::kurbo::{Affine, BezPath, Cap, Ellipse, Join, ParamCurve, ParamCurveArclen, PathEl, Point, Rect, RoundedRect, Shape, Stroke, Vec2};
use vello_cpu::peniko::{self, Color, Fill};
use vello_cpu::{Mask, Pixmap, RenderContext, Resources};

/// Précompositions imbriquées, au plus.
const MAX_COMP_DEPTH: usize = 8;
/// Chaîne de parents, au plus (une boucle s'arrête là).
const MAX_PARENTS: usize = 32;
/// Finesse des longueurs d'arc et des courbes approchées, en unités de la composition.
const ACCURACY: f64 = 0.05;

// ------------------------------------------------------------- propriétés

/// Ordonnée de la courbe d'accélération (0,0) (e0,e1) (e2,e3) (1,1) à l'abscisse `x`.
fn eased(e: [f64; 4], x: f64) -> f64 {
    let x = x.clamp(0.0, 1.0);
    if e == [0.0, 0.0, 1.0, 1.0] {
        return x;
    }
    let (x1, y1, x2, y2) = (e[0].clamp(0.0, 1.0), e[1], e[2].clamp(0.0, 1.0), e[3]);
    let bez = |a: f64, b: f64, t: f64| 3.0 * (1.0 - t) * (1.0 - t) * t * a + 3.0 * (1.0 - t) * t * t * b + t * t * t;
    // Abscisse monotone : dichotomie.
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        if bez(x1, x2, mid) < x {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    bez(y1, y2, (lo + hi) / 2.0)
}

/// Clé en cours à l'image `f` : (indice, avancement de 0 à 1), ou la valeur
/// fixe (avant la première clé, après la dernière, clé tenue).
fn segment<K>(keys: &[K], f: f64, t: impl Fn(&K) -> f64, hold: impl Fn(&K) -> bool, ease: impl Fn(&K) -> [f64; 4]) -> Result<(usize, f64), usize> {
    let n = keys.len();
    if n <= 1 || f <= t(&keys[0]) {
        return Err(0);
    }
    if f >= t(&keys[n - 1]) {
        return Err(n - 1);
    }
    let i = keys.partition_point(|k| t(k) <= f).saturating_sub(1).min(n - 2);
    let (a, b) = (t(&keys[i]), t(&keys[i + 1]));
    if hold(&keys[i]) || b <= a {
        return Err(i);
    }
    Ok((i, eased(ease(&keys[i]), (f - a) / (b - a))))
}

impl Prop {
    pub fn at(&self, f: f64) -> Vec<f64> {
        match segment(&self.keys, f, |k| k.t, |k| k.hold, |k| k.ease) {
            Err(i) => self.keys.get(i).map(|k| k.v.clone()).unwrap_or_default(),
            Ok((i, p)) => {
                let (k, next) = (&self.keys[i], &self.keys[i + 1]);
                let end = k.end.as_ref().unwrap_or(&next.v);
                match spatial(k, end, p) {
                    Some(v) => v,
                    None => k.v.iter().enumerate().map(|(j, a)| a + (end.get(j).copied().unwrap_or(*a) - a) * p).collect(),
                }
            }
        }
    }

    pub fn scalar(&self, f: f64) -> f64 {
        self.at(f).first().copied().unwrap_or(0.0)
    }

    pub fn point(&self, f: f64) -> Point {
        let v = self.at(f);
        Point::new(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0))
    }
}

/// Position le long de la trajectoire courbe d'une clé (tangentes `to` et
/// `ti`), à vitesse constante sur la courbe comme After Effects.
fn spatial(k: &Key, end: &[f64], p: f64) -> Option<Vec<f64>> {
    let (to, ti) = (k.to.as_ref()?, k.ti.as_ref()?);
    if k.v.len() < 2 || end.len() < 2 {
        return None;
    }
    let pt = |v: &[f64]| Point::new(v[0], v.get(1).copied().unwrap_or(0.0));
    let (p0, p3) = (pt(&k.v), pt(end));
    let tan = |v: &[f64]| Vec2::new(v.first().copied().unwrap_or(0.0), v.get(1).copied().unwrap_or(0.0));
    let c = vello_cpu::kurbo::CubicBez::new(p0, p0 + tan(to), p3 + tan(ti), p3);
    let len = c.arclen(ACCURACY);
    let at = if len > 0.0 { c.eval(c.inv_arclen(len * p.clamp(0.0, 1.0), ACCURACY)) } else { p0 };
    let mut out = k.v.clone();
    out[0] = at.x;
    out[1] = at.y;
    // Troisième composante (profondeur) : interpolée droit.
    for (j, v) in out.iter_mut().enumerate().skip(2) {
        *v += (end.get(j).copied().unwrap_or(*v) - *v) * p;
    }
    Some(out)
}

impl PathProp {
    pub fn at(&self, f: f64) -> PathData {
        match segment(&self.keys, f, |k| k.t, |k| k.hold, |k| k.ease) {
            Err(i) => self.keys.get(i).map(|k| k.shape.clone()).unwrap_or_default(),
            Ok((i, p)) => {
                let (k, next) = (&self.keys[i], &self.keys[i + 1]);
                let (a, b) = (&k.shape, k.end.as_ref().unwrap_or(&next.shape));
                if a.v.len() != b.v.len() {
                    return a.clone();
                }
                let mix = |x: &[(f64, f64)], y: &[(f64, f64)]| x.iter().zip(y).map(|(x, y)| (x.0 + (y.0 - x.0) * p, x.1 + (y.1 - x.1) * p)).collect();
                PathData { v: mix(&a.v, &b.v), i: mix(&a.i, &b.i), o: mix(&a.o, &b.o), closed: a.closed }
            }
        }
    }
}

fn bez(d: &PathData) -> BezPath {
    let mut p = BezPath::new();
    let n = d.v.len();
    if n == 0 {
        return p;
    }
    let pt = |(x, y): (f64, f64)| Point::new(x, y);
    p.move_to(pt(d.v[0]));
    let last = if d.closed { n } else { n - 1 };
    for j in 0..last {
        let k = (j + 1) % n;
        let (a, b) = (d.v[j], d.v[k]);
        p.curve_to(Point::new(a.0 + d.o[j].0, a.1 + d.o[j].1), Point::new(b.0 + d.i[k].0, b.1 + d.i[k].1), pt(b));
    }
    if d.closed {
        p.close_path();
    }
    p
}

/// Matrice et opacité (0 à 1) d'une transformation à l'image `f`.
fn matrix(t: &Transform, f: f64) -> (Affine, f64) {
    let pos = match &t.pos {
        Position::Joined(p) => p.point(f),
        Position::Split(x, y) => Point::new(x.scalar(f), y.scalar(f)),
    };
    let anchor = t.anchor.point(f);
    let scale = t.scale.at(f);
    let (sx, sy) = (scale.first().copied().unwrap_or(100.0) / 100.0, scale.get(1).copied().unwrap_or(100.0) / 100.0);
    let mut m = Affine::translate(pos.to_vec2()) * Affine::rotate(t.rot.scalar(f).to_radians());
    let skew = t.skew.scalar(f);
    if skew != 0.0 {
        let axis = t.skew_axis.scalar(f).to_radians();
        m = m * Affine::rotate(-axis) * Affine::skew((-skew).to_radians().tan(), 0.0) * Affine::rotate(axis);
    }
    m = m * Affine::scale_non_uniform(sx, sy) * Affine::translate(-anchor.to_vec2());
    (m, (t.opacity.scalar(f) / 100.0).clamp(0.0, 1.0))
}

// ------------------------------------------------------------------ formes

enum Paint {
    Solid(Color),
    Gradient(peniko::Gradient, Affine),
}

enum Op {
    Fill(BezPath, Paint, bool),
    Stroke(BezPath, Paint, Stroke),
}

fn color(v: &[f64], alpha: f64) -> Color {
    let c = |i: usize| v.get(i).copied().unwrap_or(0.0).clamp(0.0, 1.0) as f32;
    Color::new([c(0), c(1), c(2), alpha.clamp(0.0, 1.0) as f32])
}

fn gradient(g: &Gradient, f: f64, alpha: f64, tf: Affine) -> Option<Paint> {
    let v = g.stops.at(f);
    let colors = v.get(..g.count * 4)?;
    let alphas: Vec<(f64, f64)> = v[g.count * 4..].as_chunks::<2>().0.iter().map(|c| (c[0], c[1])).collect();
    // Alpha à la position d'un arrêt de couleur : entre les deux arrêts d'alpha voisins.
    let alpha_at = |x: f64| -> f64 {
        let Some(first) = alphas.first() else { return 1.0 };
        if x <= first.0 {
            return first.1;
        }
        for w in alphas.windows(2) {
            if x <= w[1].0 {
                let span = w[1].0 - w[0].0;
                return if span > 0.0 { w[0].1 + (w[1].1 - w[0].1) * (x - w[0].0) / span } else { w[1].1 };
            }
        }
        alphas.last().map_or(1.0, |a| a.1)
    };
    let stops: Vec<(f32, Color)> = colors.as_chunks::<4>().0.iter().map(|c| (c[0].clamp(0.0, 1.0) as f32, color(&c[1..], alpha * alpha_at(c[0])))).collect();
    let (s, e) = (g.start.point(f), g.end.point(f));
    let grad = if g.radial { peniko::Gradient::new_radial(s, (e - s).hypot() as f32) } else { peniko::Gradient::new_linear(s, e) };
    Some(Paint::Gradient(grad.with_stops(stops.as_slice()), tf))
}

fn stroke(line: &Line, f: f64, tf: Affine) -> Stroke {
    // Le tracé est déjà dans l'espace du calque : l'épaisseur suit l'échelle du groupe.
    let scale = tf.determinant().abs().sqrt();
    let mut s = Stroke::new(line.width.scalar(f).max(0.0) * scale)
        .with_caps(match line.cap {
            2 => Cap::Round,
            3 => Cap::Square,
            _ => Cap::Butt,
        })
        .with_join(match line.join {
            2 => Join::Round,
            3 => Join::Bevel,
            _ => Join::Miter,
        })
        .with_miter_limit(line.miter.max(1.0));
    let dashes: Vec<f64> = line.dashes.iter().map(|d| (d.scalar(f) * scale).max(0.0)).collect();
    if dashes.iter().any(|d| *d > 0.0) {
        s = s.with_dashes(line.dash_offset.as_ref().map_or(0.0, |o| o.scalar(f) * scale), dashes);
    }
    s
}

/// Garde de `path` la part qui va de `a` à `b` (fractions de sa longueur,
/// `a` dans [0, 1[, `b` jusqu'à `a + 1` : le tour peut passer par le début).
fn trimmed(path: &BezPath, a: f64, b: f64) -> BezPath {
    let segs: Vec<_> = path.segments().collect();
    let lens: Vec<f64> = segs.iter().map(|s| s.arclen(ACCURACY)).collect();
    let total: f64 = lens.iter().sum();
    let mut out = BezPath::new();
    if total <= 0.0 {
        return out;
    }
    let mut last: Option<Point> = None;
    let mut take = |from: f64, to: f64, out: &mut BezPath| {
        let mut acc = 0.0;
        for (s, len) in segs.iter().zip(&lens) {
            let (lo, hi) = (from.max(acc), to.min(acc + len));
            if hi > lo && *len > 0.0 {
                let t0 = if lo <= acc { 0.0 } else { s.inv_arclen(lo - acc, ACCURACY) };
                let t1 = if hi >= acc + len { 1.0 } else { s.inv_arclen(hi - acc, ACCURACY) };
                let part = s.subsegment(t0..t1);
                if last.is_none_or(|p| (p - part.start()).hypot() > 1e-6) {
                    out.push(PathEl::MoveTo(part.start()));
                }
                out.push(part.as_path_el());
                last = Some(part.end());
            }
            acc += len;
        }
    };
    let (from, to) = (a * total, b * total);
    if to <= total {
        take(from, to, &mut out);
    } else {
        take(from, total, &mut out);
        take(0.0, to - total, &mut out);
    }
    out
}

/// Bornes d'un rognage : (début dans [0, 1[, fin), ou `None` s'il garde tout.
fn trim_range(start: f64, end: f64, offset: f64) -> Option<(f64, f64)> {
    let (mut a, mut b) = (start / 100.0, end / 100.0);
    if a > b {
        std::mem::swap(&mut a, &mut b);
    }
    let (a, b) = (a.clamp(0.0, 1.0), b.clamp(0.0, 1.0));
    if b - a >= 1.0 {
        return None;
    }
    let shift = offset / 360.0;
    let from = (a + shift).rem_euclid(1.0);
    Some((from, from + (b - a)))
}

fn star(center: Point, points: f64, rot: f64, outer: f64, inner: f64, polygon: bool) -> BezPath {
    let n = points.round().clamp(3.0, 100.0) as usize;
    let count = if polygon { n } else { n * 2 };
    let mut p = BezPath::new();
    for k in 0..count {
        let r = if polygon || k % 2 == 0 { outer } else { inner };
        let a = (rot - 90.0).to_radians() + std::f64::consts::TAU * k as f64 / count as f64;
        let pt = center + Vec2::new(a.cos() * r, a.sin() * r);
        if k == 0 {
            p.move_to(pt);
        } else {
            p.line_to(pt);
        }
    }
    p.close_path();
    p
}

fn joined(geos: &[BezPath]) -> BezPath {
    let mut p = BezPath::new();
    for g in geos {
        p.extend(g.iter());
    }
    p
}

/// Parcourt les formes d'un groupe. Un style s'applique aux tracés qui le
/// précèdent dans son groupe (sous-groupes compris) ; les opérations sont
/// rendues dans l'ordre inverse, la première forme au-dessus.
fn collect(items: &[Item], f: f64, tf: Affine, alpha: f64, geos: &mut Vec<BezPath>, ops: &mut Vec<Op>) {
    for item in items {
        match item {
            Item::Group { items, tr } => {
                let (m, o) = tr.as_ref().map_or((Affine::IDENTITY, 1.0), |t| matrix(t, f));
                let mut mine = Vec::new();
                collect(items, f, tf * m, alpha * o, &mut mine, ops);
                geos.append(&mut mine);
            }
            Item::Path(p) => geos.push(tf * bez(&p.at(f))),
            Item::Ellipse { p, s } => {
                let size = s.point(f);
                geos.push(tf * Ellipse::new(p.point(f), (size.x / 2.0, size.y / 2.0), 0.0).to_path(ACCURACY));
            }
            Item::Rect { p, s, r } => {
                let (c, size) = (p.point(f), s.point(f));
                let (hw, hh) = (size.x.abs() / 2.0, size.y.abs() / 2.0);
                let rect = Rect::new(c.x - hw, c.y - hh, c.x + hw, c.y + hh);
                geos.push(tf * RoundedRect::from_rect(rect, r.scalar(f).clamp(0.0, hw.min(hh))).to_path(ACCURACY));
            }
            Item::Star { p, points, rot, outer, inner, polygon } => geos.push(tf * star(p.point(f), points.scalar(f), rot.scalar(f), outer.scalar(f), inner.scalar(f), *polygon)),
            Item::Fill { color: c, opacity, even_odd } if !geos.is_empty() => {
                let a = alpha * opacity.scalar(f) / 100.0;
                ops.push(Op::Fill(joined(geos), Paint::Solid(color(&c.at(f), a)), *even_odd));
            }
            Item::Stroke { color: c, opacity, line } if !geos.is_empty() => {
                let a = alpha * opacity.scalar(f) / 100.0;
                ops.push(Op::Stroke(joined(geos), Paint::Solid(color(&c.at(f), a)), stroke(line, f, tf)));
            }
            Item::GradientFill { grad, opacity, even_odd } if !geos.is_empty() => {
                if let Some(paint) = gradient(grad, f, alpha * opacity.scalar(f) / 100.0, tf) {
                    ops.push(Op::Fill(joined(geos), paint, *even_odd));
                }
            }
            Item::GradientStroke { grad, opacity, line } if !geos.is_empty() => {
                if let Some(paint) = gradient(grad, f, alpha * opacity.scalar(f) / 100.0, tf) {
                    ops.push(Op::Stroke(joined(geos), paint, stroke(line, f, tf)));
                }
            }
            Item::Trim { start, end, offset, sequential } => {
                let Some((a, b)) = trim_range(start.scalar(f), end.scalar(f), offset.scalar(f)) else { continue };
                if *sequential {
                    // Les tracés bout à bout forment une seule longueur.
                    let whole = trimmed(&joined(geos), a, b);
                    geos.clear();
                    geos.push(whole);
                } else {
                    for g in geos.iter_mut() {
                        *g = trimmed(g, a, b);
                    }
                }
            }
            _ => {}
        }
    }
}

// ----------------------------------------------------------------- calques

/// Où et quand un calque est dessiné : image de sa composition, matrice de
/// celle-ci vers les pixels, profondeur de précomposition.
#[derive(Clone, Copy)]
struct At {
    f: f64,
    base: Affine,
    depth: usize,
}

pub struct Renderer<'a> {
    comp: &'a Comp,
    width: u16,
    height: u16,
    /// De l'espace de la composition aux pixels.
    base: Affine,
    resources: Resources,
}

impl<'a> Renderer<'a> {
    /// Rend `comp` entière, centrée, dans `width` × `height` pixels.
    pub fn new(comp: &'a Comp, width: u16, height: u16) -> Self {
        let scale = (f64::from(width) / comp.w).min(f64::from(height) / comp.h);
        let offset = Vec2::new((f64::from(width) - comp.w * scale) / 2.0, (f64::from(height) - comp.h * scale) / 2.0);
        Renderer { comp, width, height, base: Affine::translate(offset) * Affine::scale(scale), resources: Resources::new() }
    }

    /// Pixels RVBA de l'image `f`, couleur multipliée par l'alpha.
    pub fn frame(&mut self, f: f64) -> Vec<u8> {
        let pixmap = self.rendered(|r, ctx| r.layers(ctx, &r.comp.layers, At { f, base: r.base, depth: 0 }));
        pixmap.data_as_u8_slice().to_vec()
    }

    fn rendered(&mut self, draw: impl FnOnce(&mut Self, &mut RenderContext)) -> Pixmap {
        let mut ctx = RenderContext::new(self.width, self.height);
        draw(self, &mut ctx);
        ctx.flush();
        let mut pixmap = Pixmap::new(self.width, self.height);
        ctx.render(&mut pixmap, &mut self.resources);
        pixmap
    }

    /// Calques dessinés du dernier au premier : le premier est au-dessus.
    fn layers(&mut self, ctx: &mut RenderContext, layers: &'a [Layer], at: At) {
        for i in (0..layers.len()).rev() {
            let l = &layers[i];
            if l.is_matte || l.hidden || at.f < l.ip || at.f >= l.op {
                continue;
            }
            let mask = match l.matte {
                Some(mode) if i > 0 => Some(self.matte(layers, i - 1, at, mode)),
                _ => None,
            };
            self.layer(ctx, layers, l, at, mask);
        }
    }

    /// Le calque `j`, rendu à part, comme cache du calque suivant.
    fn matte(&mut self, layers: &'a [Layer], j: usize, at: At, mode: Matte) -> Mask {
        let m = &layers[j];
        let pixmap = self.rendered(|r, ctx| {
            if !m.hidden && at.f >= m.ip && at.f < m.op {
                r.layer(ctx, layers, m, at, None);
            }
        });
        let data = pixmap
            .data_as_u8_slice()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| {
                // Couleur déjà multipliée par l'alpha : la luminance l'est aussi.
                let luma = ((u32::from(r) * 54 + u32::from(g) * 183 + u32::from(b) * 19) >> 8) as u8;
                match mode {
                    Matte::Alpha => a,
                    Matte::AlphaInverted => 255 - a,
                    Matte::Luma => luma,
                    Matte::LumaInverted => 255 - luma,
                }
            })
            .collect();
        Mask::from_parts(data, self.width, self.height)
    }

    /// Matrice du calque dans sa composition, parents compris.
    fn world(layers: &[Layer], l: &Layer, f: f64) -> (Affine, f64) {
        let (mut m, opacity) = matrix(&l.ks, f);
        let mut parent = l.parent;
        for _ in 0..MAX_PARENTS {
            let Some(p) = parent.and_then(|id| layers.iter().find(|x| x.ind == Some(id))) else { break };
            m = matrix(&p.ks, f).0 * m;
            parent = p.parent;
        }
        (m, opacity)
    }

    fn layer(&mut self, ctx: &mut RenderContext, layers: &'a [Layer], l: &'a Layer, at: At, mask: Option<Mask>) {
        let At { f, base, depth } = at;
        let (m, opacity) = Self::world(layers, l, f);
        if opacity <= 0.0 {
            return;
        }
        let tf = base * m;
        let clip = self.clip(l, f);
        let layered = opacity < 0.999 || clip.is_some() || mask.is_some();
        ctx.set_transform(tf);
        if layered {
            ctx.set_fill_rule(if clip.as_ref().is_some_and(|c| c.1) { Fill::EvenOdd } else { Fill::NonZero });
            ctx.push_layer(clip.as_ref().map(|c| &c.0), None, Some(opacity as f32), mask, None);
            ctx.set_fill_rule(Fill::NonZero);
        }
        match &l.kind {
            Kind::Shape(items) => {
                let (mut geos, mut ops) = (Vec::new(), Vec::new());
                collect(items, f, Affine::IDENTITY, 1.0, &mut geos, &mut ops);
                for op in ops.into_iter().rev() {
                    ctx.set_transform(tf);
                    let (path, paint) = match &op {
                        Op::Fill(path, paint, _) | Op::Stroke(path, paint, _) => (path, paint),
                    };
                    match paint {
                        Paint::Solid(c) => {
                            ctx.reset_paint_transform();
                            ctx.set_paint(*c);
                        }
                        Paint::Gradient(g, gtf) => {
                            ctx.set_paint_transform(*gtf);
                            ctx.set_paint(g.clone());
                        }
                    }
                    match &op {
                        Op::Fill(_, _, even_odd) => {
                            ctx.set_fill_rule(if *even_odd { Fill::EvenOdd } else { Fill::NonZero });
                            ctx.fill_path(path);
                        }
                        Op::Stroke(_, _, s) => {
                            ctx.set_stroke(s.clone());
                            ctx.stroke_path(path);
                        }
                    }
                }
                ctx.reset_paint_transform();
                ctx.set_fill_rule(Fill::NonZero);
            }
            Kind::Solid { color: c, w, h } => {
                ctx.set_paint(color(c, 1.0));
                ctx.fill_rect(&Rect::new(0.0, 0.0, *w, *h));
            }
            Kind::Precomp { id, w, h } => {
                if let (Some(inner), true) = (self.comp.assets.get(id), depth < MAX_COMP_DEPTH) {
                    let inner: &'a [Layer] = inner;
                    let local = match &l.time_remap {
                        Some(t) => t.scalar(f) * self.comp.fr,
                        None => (f - l.st) / l.sr,
                    };
                    let bounded = *w > 0.0 && *h > 0.0;
                    if bounded {
                        ctx.push_clip_layer(&Rect::new(0.0, 0.0, *w, *h).to_path(ACCURACY));
                    }
                    self.layers(ctx, inner, At { f: local, base: tf, depth: depth + 1 });
                    if bounded {
                        ctx.pop_layer();
                    }
                }
            }
            Kind::Null | Kind::Other => {}
        }
        if layered {
            ctx.pop_layer();
        }
    }

    /// Masques du calque réunis en un tracé de découpe, dans l'espace du
    /// calque, et s'il se remplit en pair-impair (masque inversé ou soustrait :
    /// un grand rectangle autour, le masque y fait un trou).
    fn clip(&self, l: &Layer, f: f64) -> Option<(BezPath, bool)> {
        if l.masks.is_empty() {
            return None;
        }
        let mut path = BezPath::new();
        let mut hole = false;
        for m in &l.masks {
            hole |= m.inverted != (m.mode == 's');
            path.extend(bez(&m.path.at(f)).iter());
        }
        if hole {
            let far = 4.0 * self.comp.w.max(self.comp.h) + 1e4;
            path.extend(Rect::new(-far, -far, far, far).to_path(ACCURACY).iter());
        }
        Some((path, hole))
    }
}
