//! Icônes de la vue d'appel : celles du jeu plein du thème (`theme::icons`),
//! plus « détacher », qui n'y est pas.

use eframe::egui::{pos2, Color32, Painter, Rect, Shape, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    /// Caméra barrée.
    CameraOff,
    /// Écran (partage).
    Screen,
    /// Écran barré (arrêter le partage).
    ScreenOff,
    /// Fenêtre et flèche vers l'extérieur (détacher).
    PopOut,
    /// Quatre coins (plein écran).
    Fullscreen,
    /// Œil (spectateurs).
    Eye,
    /// Punaise (toujours au-dessus).
    Pin,
}

pub(super) fn paint(g: Glyph, painter: &Painter, rect: Rect, color: Color32) {
    // Jeu plein du thème (lot V1) ; « détacher » n'y est pas.
    use crate::theme::icons::Icon;
    let full = match g {
        Glyph::CameraOff => Icon::CameraOff,
        Glyph::Screen => Icon::Screen,
        Glyph::ScreenOff => Icon::ScreenOff,
        Glyph::Fullscreen => Icon::Fullscreen,
        Glyph::Eye => Icon::Eye,
        Glyph::Pin => Icon::Pin,
        Glyph::PopOut => return paint_pop_out(painter, rect, color),
    };
    crate::theme::icons::paint(full, painter, rect, color);
}

fn paint_pop_out(painter: &Painter, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    let c = rect.center();
    let u = side * 0.5;
    let p = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    let line = |w: f32| Stroke::new((side * w).max(1.0), color);
    painter.add(Shape::line(vec![p(-0.1, -0.75), p(-0.75, -0.75), p(-0.75, 0.75), p(0.75, 0.75), p(0.75, 0.1)], line(0.1)));
    painter.line_segment([p(-0.05, 0.05), p(0.75, -0.75)], line(0.1));
    painter.add(Shape::line(vec![p(0.25, -0.75), p(0.75, -0.75), p(0.75, -0.25)], line(0.1)));
}
