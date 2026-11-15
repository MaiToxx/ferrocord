//! Distance parcourue par un cran de molette.
//!
//! egui avance de 40 points par cran ; Discord (Chromium) de 100 pixels CSS
//! pour le réglage de Windows par défaut (3 lignes par cran), soit deux fois
//! et demie plus : la molette paraissait lente. Ici, un cran vaut
//! [`NOTCH_POINTS`] pour 3 lignes, suit le nombre de lignes réglé dans
//! Windows, et se règle (« Vitesse de défilement », `local.toml`).
//!
//! winit ne donne sous Windows que des crans (`MouseScrollDelta::LineDelta`,
//! 1.0 par cran, fractions pour un pavé tactile), jamais le nombre de lignes
//! de Windows : il est lu à part (`machine::windows_wheel_lines`).
//!
//! La distance est en points : avec le zoom de l'interface, elle grandit à
//! l'écran comme le texte, comme dans Chromium.

use eframe::egui;

/// Points parcourus par un cran, pour 3 lignes par cran et une vitesse de 100 %.
pub const NOTCH_POINTS: f32 = 100.0;
/// Lignes par cran de Windows par défaut (et quand le réglage est inconnu
/// ou vaut « un écran à la fois »).
pub const DEFAULT_LINES: u32 = 3;
/// Au-delà, un cran dépasserait la hauteur d'une petite fenêtre.
const MAX_LINES: u32 = 12;

/// Points par cran pour `lines` lignes par cran (Windows) et une vitesse de
/// `percent` % (réglage de l'utilisateur).
pub fn notch_points(lines: Option<u32>, percent: u16) -> f32 {
    let lines = lines.filter(|n| *n > 0).unwrap_or(DEFAULT_LINES).min(MAX_LINES);
    let percent = percent.clamp(crate::state::local::SCROLL_MIN, crate::state::local::SCROLL_MAX);
    NOTCH_POINTS * lines as f32 / DEFAULT_LINES as f32 * f32::from(percent) / 100.0
}

/// Règle la distance d'un cran pour toutes les listes (egui convertit lui-même
/// les crans en points, pour la liste des messages comme pour les autres).
pub fn apply(ctx: &egui::Context, lines: Option<u32>, percent: u16) {
    let points = notch_points(lines, percent);
    ctx.options_mut(|o| o.input_options.line_scroll_speed = points);
}

/// Distance de molette ramenée à l'échelle d'origine d'egui (40 points par
/// cran) : pour ce qui lit la molette autrement que pour défiler (zoom de la
/// visionneuse), et ne doit pas changer avec la vitesse de défilement.
pub fn in_egui_notches(ctx: &egui::Context, points: f32) -> f32 {
    const EGUI_NOTCH: f32 = 40.0;
    let speed = ctx.options(|o| o.input_options.line_scroll_speed);
    if speed > 0.0 { points * EGUI_NOTCH / speed } else { points }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_notch_is_a_hundred_points_by_default_and_follows_the_settings() {
        assert_eq!(notch_points(None, 100), 100.0);
        assert_eq!(notch_points(Some(3), 100), 100.0);
        assert_eq!(notch_points(Some(6), 100), 200.0);
        assert_eq!(notch_points(Some(3), 50), 50.0);
        assert_eq!(notch_points(Some(1), 300), 100.0);
        // Réglage absurde ou « un écran à la fois » : bornes.
        assert_eq!(notch_points(Some(0), 100), 100.0);
        assert_eq!(notch_points(Some(1000), 100), 400.0);
        assert_eq!(notch_points(Some(3), 0), notch_points(Some(3), crate::state::local::SCROLL_MIN));
    }

    /// Un cran de `lines` dans une `ScrollArea` ordinaire, images jouées tant
    /// qu'egui en redemande : distance parcourue et nombre d'images.
    fn notch(ctx: &egui::Context, time: &mut f64, dt: f64, lines: f32) -> (f32, usize) {
        let offset = std::cell::Cell::new(0.0f32);
        let mut frames = 0;
        let before = {
            run(ctx, time, dt, Vec::new(), &offset);
            offset.get()
        };
        let mut events = vec![
            egui::Event::PointerMoved(egui::pos2(100.0, 100.0)),
            egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, lines), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE },
        ];
        loop {
            let again = run(ctx, time, dt, std::mem::take(&mut events), &offset);
            frames += 1;
            if !again || frames > 300 {
                break;
            }
        }
        (offset.get() - before, frames)
    }

    /// Une image ; vrai si egui en redemande une tout de suite.
    fn run(ctx: &egui::Context, time: &mut f64, dt: f64, events: Vec<egui::Event>, offset: &std::cell::Cell<f32>) -> bool {
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), time: Some(*time), events, ..Default::default() };
        *time += dt;
        let mut out = ctx.run_ui(input, |ui| {
            let o = egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                ui.set_min_height(20_000.0);
            });
            offset.set(o.state.clamped_offset().y);
        });
        out.textures_delta.clear();
        out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay.is_zero())
    }

    #[test]
    fn one_notch_travels_its_full_distance_in_a_plain_scroll_area() {
        for dt in [1.0 / 30.0, 1.0 / 60.0, 1.0 / 144.0] {
            let ctx = egui::Context::default();
            apply(&ctx, Some(3), 100);
            let mut time = 0.0;
            // Vers le bas d'abord (la zone part du haut), puis retour.
            for lines in [-1.0f32, -3.0, 2.0] {
                let (moved, frames) = notch(&ctx, &mut time, dt, lines);
                assert!((moved + lines * NOTCH_POINTS).abs() < 0.01, "{lines} cran(s) à {dt:.4} s par image : {moved} points");
                // Lissage d'egui : 90 % en 100 ms, fini en moins d'une demi-seconde,
                // puis plus aucune image demandée.
                assert!((frames as f64) * dt < 0.5, "{frames} images à {dt:.4} s");
            }
            // Vitesse doublée : distance doublée.
            apply(&ctx, Some(3), 200);
            let (moved, _) = notch(&ctx, &mut time, dt, -1.0);
            assert!((moved - 2.0 * NOTCH_POINTS).abs() < 0.01, "{moved}");
        }
    }
}
