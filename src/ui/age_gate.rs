//! Écran d'entrée d'un salon à limite d'âge (F8) : à la place des messages
//! tant que « Continuer » n'a pas été choisi, comme le client officiel.
//! Le choix vaut pour la session ; rien n'est envoyé à Discord.

use super::widgets::{self, Kind};
use super::App;
use crate::i18n::table::ff::t as ft;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, RichText};

impl App {
    pub(super) fn age_gate(&mut self, ui: &mut egui::Ui, cid: u64, t: &Tokens) {
        let lang = self.lang;
        let (mut go, mut back) = (false, false);
        let width = (t.typography.body * 26.0).min(ui.available_width() - 2.0 * t.spacing.item_x).max(0.0);
        let title_h = t.typography.title * 1.4;
        // Hauteur approchée du bloc, pour le centrer sans mesure préalable.
        let block = Tokens::icon_side(t.typography.title) * 3.0 + title_h + t.typography.body * 4.0 + widgets::button_height(t) + 4.0 * t.spacing.item_y;
        ui.vertical_centered(|ui| {
            ui.add_space(((ui.available_height() - block) / 2.0).max(t.spacing.item_y));
            ui.set_max_width(width);
            let side = Tokens::icon_side(t.typography.title) * 3.0;
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
            icons::paint(Icon::Lock, ui.painter(), rect, t.colors.muted);
            ui.add_space(t.spacing.item_y);
            ui.label(RichText::new(ft::AGE_TITLE.get(lang)).size(t.typography.title).strong().color(t.colors.text));
            ui.add_space(t.spacing.item_y);
            ui.label(RichText::new(ft::AGE_BODY.get(lang)).color(t.colors.muted));
            ui.add_space(2.0 * t.spacing.item_y);
            // Les deux boutons côte à côte, centrés.
            let font = egui::TextStyle::Button.resolve(ui.style());
            let wide = |ui: &egui::Ui, s: &str| ui.painter().layout_no_wrap(s.to_string(), font.clone(), t.colors.text).size().x + 2.0 * ui.spacing().button_padding.x;
            let (a, b) = (ft::AGE_BACK.get(lang), ft::AGE_CONTINUE.get(lang));
            let each = wide(ui, a).max(wide(ui, b)).max(t.typography.body * 7.0);
            let total = 2.0 * each + t.spacing.item_x;
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = t.spacing.item_x;
                ui.add_space(((ui.available_width() - total) / 2.0).max(0.0));
                back = widgets::button_wide(ui, t, Kind::Secondary, a, each).clicked();
                go = widgets::button_wide(ui, t, Kind::Danger, b, each).clicked();
            });
        });
        if go {
            self.store.pass_age_gate(cid);
            self.scroll_to_bottom = true;
        } else if back {
            self.store.leave_age_gate(cid);
        }
    }
}
