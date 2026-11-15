//! Glyphes courants préparés avant le premier salon (lot C9).
//!
//! Au démarrage, l'image qui montre le premier salon (historique hors ligne
//! ou page reçue) rastérisait d'un coup tous les glyphes de l'écran, dans
//! chaque police et chaque taille : 14 ms sur le vrai compte, pour une image
//! qui en vaut 1 à 2 ensuite. Les glyphes des lettres courantes sont donc
//! préparés pendant l'attente, une police par image (moins de 2 ms chacune),
//! tant qu'aucun salon n'est affiché. Les polices changent (Segoe UI arrive
//! après la police intégrée, zoom) : tout est refait pour les nouvelles.

use super::fonts;
use crate::theme::Tokens;
use eframe::egui::{self, Color32, FontId};

/// Lettres, chiffres et ponctuation courants (français et anglais).
const SAMPLE: &str = "abcdefghijklmnopqrstuvwxyz ABCDEFGHIJKLMNOPQRSTUVWXYZ 0123456789 .,;:!?'\"()[]{}<>-_/\\@#&%*+=~|$€ éèêëàâäçùûüôöîïœÉÈÀÇ’«»…–—•·";

#[derive(Default)]
pub(super) struct Warm {
    /// Polices déjà préparées pour la génération `generation`.
    done: usize,
    /// Génération des polices et échelle pour lesquelles `done` vaut.
    generation: Option<(u64, u32)>,
}

/// Polices du premier écran : texte des messages, noms, heures, salons, en-têtes.
fn faces(t: &Tokens) -> [FontId; 7] {
    let (body, small, title) = (t.typography.body, t.typography.small, t.typography.title);
    let bold = fonts::Face { bold: true, italic: false };
    [
        FontId::proportional(body),
        fonts::demi_font(body),
        fonts::font(body, bold),
        FontId::proportional(small),
        fonts::demi_font(small),
        fonts::demi_font(title),
        FontId::monospace(body),
    ]
}

impl Warm {
    /// Une police préparée à cette image si aucun salon n'est encore
    /// affiché ; demande l'image suivante tant qu'il en reste.
    pub(super) fn step(&mut self, ctx: &egui::Context, t: &Tokens, channel_shown: bool) {
        let now = (fonts::generation(), ctx.pixels_per_point().to_bits());
        if self.generation != Some(now) {
            self.generation = Some(now);
            self.done = 0;
        }
        let faces = faces(t);
        if channel_shown || self.done >= faces.len() {
            return;
        }
        let font = faces[self.done].clone();
        ctx.fonts_mut(|f| {
            let _ = f.layout_no_wrap(SAMPLE.to_owned(), font, Color32::WHITE);
        });
        self.done += 1;
        if self.done < faces.len() {
            ctx.request_repaint();
        }
    }

    /// Toutes les polices du premier écran sont prêtes.
    #[cfg(test)]
    fn finished(&self, t: &Tokens) -> bool {
        self.done >= faces(t).len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_font_per_frame_then_nothing_and_again_when_fonts_change() {
        let ctx = egui::Context::default();
        fonts::install_builtin(&ctx);
        let t = Tokens::default();
        let mut w = Warm::default();
        let frames = |w: &mut Warm, shown: bool, n: usize| {
            let mut asked = 0;
            for _ in 0..n {
                let mut out = ctx.run_ui(egui::RawInput::default(), |ui| w.step(ui.ctx(), &t, shown));
                out.textures_delta.clear();
                if out.viewport_output.values().any(|v| v.repaint_delay == std::time::Duration::ZERO) {
                    asked += 1;
                }
            }
            asked
        };
        // Salon déjà affiché : rien n'est préparé.
        frames(&mut w, true, 3);
        assert_eq!(w.done, 0);
        let n = faces(&t).len();
        // Une police par image, l'image suivante demandée jusqu'à la dernière (egui peut
        // en demander une de plus pour l'atlas des glyphes) ; puis plus rien.
        // Le compteur de génération est commun au processus : un autre test qui installe
        // des polices au même moment relance la préparation ; on recommence alors.
        for attempt in 0.. {
            w = Warm::default();
            let before = fonts::generation();
            let asked = frames(&mut w, false, n);
            let done = w.done;
            frames(&mut w, false, 2);
            let after = frames(&mut w, false, 3);
            let finished = w.finished(&t);
            if fonts::generation() != before && attempt < 50 {
                continue;
            }
            assert!(asked >= n - 1, "{asked}");
            assert_eq!(done, n);
            assert_eq!(after, 0, "plus aucune image demandée");
            assert!(finished);
            break;
        }
        // Nouvelles polices : refait.
        fonts::install_builtin(&ctx);
        frames(&mut w, false, 1);
        assert_eq!(w.done, 1);
    }
}
