//! Indicateur de chargement sobre (lot C9) : le même dessin que
//! `egui::Spinner`, mais redessiné au plus [`FPS`] fois par seconde. Celui
//! d'egui demande une image à chaque image, soit 120 à 144 images par
//! seconde sur un écran rapide, pendant tout le chargement d'un salon (au
//! démarrage : la moitié des images avant le calme du processeur).

use eframe::egui::{self, emath::lerp, vec2, Pos2, Sense, Shape, Stroke};
use std::time::Duration;

/// Images par seconde de l'animation : assez pour qu'elle reste fluide à l'œil.
pub const FPS: u64 = 30;

/// Dessine l'indicateur (taille : `interact_size` du style, comme egui).
pub(super) fn spinner(ui: &mut egui::Ui) -> egui::Response {
    let size = ui.style().spacing.interact_size.y;
    let (rect, response) = ui.allocate_exact_size(vec2(size, size), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::new(egui::Role::ProgressIndicator));
    if ui.is_rect_visible(rect) {
        ui.ctx().request_repaint_after(Duration::from_millis(1000 / FPS));
        let color = ui.visuals().strong_text_color();
        let radius = (rect.height().min(rect.width()) * 0.5) - 2.0;
        let n_points = (radius.round() as u32).clamp(8, 128);
        // Temps arrondi au pas de l'animation : une image due à autre chose
        // (souris) ne la fait pas avancer plus vite.
        let step = 1.0 / FPS as f64;
        let time = (ui.input(|i| i.time) / step).floor() * step;
        let start_angle = time * std::f64::consts::TAU;
        let end_angle = start_angle + 240f64.to_radians() * time.sin();
        let points: Vec<Pos2> = (0..n_points)
            .map(|i| {
                let angle = lerp(start_angle..=end_angle, i as f64 / n_points as f64);
                let (sin, cos) = angle.sin_cos();
                rect.center() + radius * vec2(cos as f32, sin as f32)
            })
            .collect();
        ui.painter().add(Shape::line(points, Stroke::new(3.0, color)));
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    /// L'indicateur ne demande pas d'image immédiate, seulement après le pas de l'animation.
    #[test]
    fn asks_for_a_frame_per_step_not_every_frame() {
        let ctx = egui::Context::default();
        let mut delays = Vec::new();
        for i in 0..6 {
            let input = egui::RawInput { time: Some(f64::from(i) * 0.01), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                spinner(ui);
            });
            out.textures_delta.clear();
            delays.push(out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay));
        }
        // Les premières images, egui en demande lui-même (atlas, mise en page).
        for d in delays.into_iter().skip(3) {
            let d = d.expect("fenêtre principale");
            assert!(d >= Duration::from_millis(1000 / FPS - 2), "{d:?}");
            assert!(d <= Duration::from_millis(1000 / FPS), "{d:?}");
        }
    }
}
