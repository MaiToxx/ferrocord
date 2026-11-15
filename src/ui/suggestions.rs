//! Suggestions d'amis (lot ND) : un bloc en tête de l'onglet « En attente »
//! de la page Amis. Chaque ligne : avatar, nom, d'où vient la suggestion,
//! « Ajouter » (demande d'ami) et « Ignorer ».

use crate::i18n::table::nd::{f as nf, p as np, t as nt};
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense};

/// Suggestions montrées au plus (les suivantes arrivent quand celles-ci partent).
const SHOWN: usize = 4;

impl super::App {
    /// Page Amis sur « En attente », là où paraissent les suggestions (aperçu, tests).
    pub fn show_friend_suggestions(&mut self) {
        self.store.select_dms();
        self.requests.open = false;
        self.quests.open = false;
        self.friends.tab = crate::state::relations::FriendTab::Pending;
    }

    /// L'onglet « En attente » est affiché : c'est ce geste qui demande la
    /// liste (une fois par session). Rien n'est dessiné tant qu'elle est vide.
    pub(super) fn suggestions_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        self.store.suggestions_open();
        let lang = self.lang;
        let c = &t.colors;
        let list: Vec<_> = match &self.store.suggestions.list {
            crate::state::usersettings::Remote::Ready(l) if !l.is_empty() => l.iter().take(SHOWN).cloned().collect(),
            _ => return,
        };
        super::profile::section(ui, t, nt::SUGGESTIONS.get(lang));
        let (mut add, mut dismiss) = (None, None);
        for s in &list {
            egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_row()).inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let side = t.layout.small_avatar;
                    let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                    super::util::paint_avatar(ui, &self.images, t, slot, &s.avatar, &s.name);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new(&s.name).family(crate::ui::fonts::demi()).color(c.text_strong));
                        let mut detail = s.username.clone();
                        if let Some((platform, name)) = &s.reason {
                            detail = nf::suggested_from(lang, &detail, name, platform);
                        }
                        if let Some(n) = s.mutual_friends.filter(|n| *n > 0) {
                            detail = format!("{detail} · {}", np::mutual_friends(lang, u64::from(n)));
                        }
                        ui.add(egui::Label::new(RichText::new(detail).small().color(c.muted)).truncate());
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(super::form::btn(t, nt::SUGGESTION_DISMISS.get(lang))).clicked() {
                            dismiss = Some(s.user_id);
                        }
                        if ui.add(super::form::btn(t, nt::SUGGESTION_ADD.get(lang))).clicked() {
                            add = Some(s.user_id);
                        }
                    });
                });
            });
            ui.add_space(t.spacing.item_y / 2.0);
        }
        ui.add_space(t.spacing.item_y);
        if let Some(id) = add {
            self.store.suggestion_add(id);
        }
        if let Some(id) = dismiss {
            self.store.suggestion_dismiss(id);
        }
    }
}
