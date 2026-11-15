//! Choix de l'effet de profil dans l'édition du profil (lot ND) : les effets
//! que le compte possède, en vignettes fixes, et « Aucun ». Rien ne s'achète
//! ici : la boutique reste celle de l'application officielle.

use super::effects::thumbnail;
use crate::i18n::table::nd::{f as nf, t as nt};
use crate::state::usersettings::Remote;
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense, Stroke, StrokeKind};

/// Taille d'une vignette (même forme que le canevas d'un effet, 450 × 880).
const TILE: egui::Vec2 = egui::vec2(46.0, 90.0);

impl super::App {
    /// Rangée du choix. Rend le nouveau choix au clic (`Some(None)` : aucun effet).
    pub(super) fn effect_picker(&self, ui: &mut egui::Ui, t: &Tokens, worn: Option<u64>) -> Option<Option<u64>> {
        let lang = self.lang;
        let c = &t.colors;
        super::profile::section(ui, t, nt::EFFECT.get(lang));
        let owned = match &self.store.effects.owned {
            Remote::Ready(ids) => ids,
            Remote::Failed(_) => {
                ui.label(RichText::new(nt::EFFECT_FAILED.get(lang)).small().color(c.faint));
                return None;
            }
            _ => {
                ui.label(RichText::new(nt::EFFECT_LOADING.get(lang)).small().color(c.faint));
                return None;
            }
        };
        if owned.is_empty() && worn.is_none() {
            ui.label(RichText::new(nt::EFFECT_EMPTY.get(lang)).small().color(c.faint)).on_hover_text(nt::EFFECT_TIP.get(lang));
            return None;
        }
        let mut picked = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_y, t.spacing.item_y);
            // « Aucun », puis chaque effet possédé.
            for id in std::iter::once(None).chain(owned.iter().copied().map(Some)) {
                let cfg = id.and_then(|id| self.store.effects.config(id));
                let name = match (&cfg, id) {
                    (Some(cfg), _) if !cfg.title.is_empty() => nf::effect_named(lang, &cfg.title),
                    (_, None) => nt::EFFECT_NONE.get(lang).to_owned(),
                    _ => nt::EFFECT.get(lang).to_owned(),
                };
                let (rect, r) = ui.allocate_exact_size(TILE, Sense::click());
                super::a11y::name(ui.ctx(), r.id, &name);
                let on = worn == id;
                ui.painter().rect_filled(rect, t.r_control(), c.bg0);
                match &cfg {
                    Some(cfg) => thumbnail(ui, &self.images, cfg, rect, t.r_control()),
                    None if id.is_none() => {
                        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, nt::EFFECT_NONE.get(lang), egui::FontId::proportional(11.0), c.muted);
                    }
                    None => {}
                }
                let stroke = if on { Stroke::new(2.0, c.accent) } else if r.hovered() { Stroke::new(1.0, c.muted) } else { Stroke::new(1.0, c.faint) };
                ui.painter().rect_stroke(rect, t.r_control(), stroke, StrokeKind::Inside);
                if r.on_hover_text(name).clicked() && !on {
                    picked = Some(id);
                }
            }
        });
        picked
    }
}
