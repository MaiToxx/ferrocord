//! Icônes du vocal : celles du jeu plein du thème (`theme::icons`), plus la
//! main levée, qui n'y est pas.

use crate::theme::icons::Icon;
use eframe::egui::{pos2, Color32, Painter, Rect, Stroke};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum VIcon {
    /// Salon vocal (haut-parleur).
    Speaker,
    Mic,
    /// Micro barré.
    MicOff,
    Headphones,
    /// Casque barré.
    HeadphonesOff,
    /// Caméra allumée.
    Camera,
    /// Raccrocher (combiné).
    Hangup,
    /// Demande de parole (main levée : paume et doigts).
    Hand,
}

pub(in crate::ui) fn paint(icon: VIcon, painter: &Painter, rect: Rect, color: Color32) {
    // Jeu plein du thème (lot V1) ; la main levée n'y est pas.
    let full = match icon {
        VIcon::Speaker => Icon::Speaker,
        VIcon::Mic => Icon::Mic,
        VIcon::MicOff => Icon::MicOff,
        VIcon::Headphones => Icon::Headphones,
        VIcon::HeadphonesOff => Icon::HeadphonesOff,
        VIcon::Camera => Icon::Camera,
        VIcon::Hangup => Icon::HangUp,
        VIcon::Hand => return paint_hand(painter, rect, color),
    };
    crate::theme::icons::paint(full, painter, rect, color);
}

fn paint_hand(painter: &Painter, rect: Rect, color: Color32) {
    let side = rect.width().min(rect.height());
    let c = rect.center();
    let u = side * 0.5;
    let p = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    painter.rect_filled(Rect::from_min_max(p(-0.45, -0.1), p(0.45, 0.8)), side * 0.12, color);
    for (i, x) in [-0.36f32, -0.12, 0.12, 0.36].into_iter().enumerate() {
        let top = if i == 0 || i == 3 { -0.5 } else { -0.75 };
        painter.line_segment([p(x, top), p(x, 0.0)], Stroke::new((side * 0.13).max(1.0), color));
    }
}
