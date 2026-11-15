//! Panneau « Résumés du salon » (lot ND), ouvert par le menu d'un salon de
//! serveur : les résumés que Discord a déjà produits, du plus récent au
//! plus ancien. Lecture seule ; « Aller au message » ouvre le premier
//! message résumé.

use super::msg_actions::Action;
use super::App;
use crate::i18n::t as tx;
use crate::i18n::table::nd::{p as np, t as nt};
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::icons::Icon;
use eframe::egui::{self, RichText};

impl App {
    /// Entrée du menu contextuel d'un salon ; `true` si elle est choisie.
    pub(super) fn summaries_entry(&self, ui: &mut egui::Ui) -> bool {
        let chosen = crate::ui::menu::item(ui, &self.theme, nt::SUMMARIES.get(self.lang), Some(Icon::Inbox), false).clicked();
        if chosen {
            ui.close();
        }
        chosen
    }

    /// Ouvre le salon et son panneau de résumés (menu du salon, aperçu,
    /// tests) : c'est ce geste qui demande les résumés.
    pub fn show_summaries(&mut self, channel_id: u64) {
        self.open_channel(channel_id);
        self.actions.pins_open = false;
        self.summaries_open = Some(channel_id);
        self.store.summaries_open(channel_id);
    }

    /// Panneau à droite de la discussion, s'il est ouvert pour ce salon.
    pub(super) fn summaries_panel(&mut self, ui: &mut egui::Ui, cid: u64) {
        if self.summaries_open != Some(cid) {
            // Changer de salon referme le panneau.
            self.summaries_open = None;
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let (mut close, mut refresh, mut jump) = (false, false, None);
        let h24 = self.h24();
        egui::Panel::right("summaries").frame(egui::Frame::new().fill(t.colors.bg1).inner_margin(t.channels_margin())).default_size(t.layout.channels_width).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(nt::SUMMARIES.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = super::util::icon_button(ui, &t, Icon::Close, tx::CLOSE_TIP.get(lang)).clicked();
                });
            });
            ui.separator();
            let state = self.store.summaries.get(cid);
            let note = |ui: &mut egui::Ui, text: &str| {
                ui.add(egui::Label::new(RichText::new(text).color(t.colors.muted)).wrap());
            };
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                match state {
                    None | Some(Remote::Idle | Remote::Loading) => {
                        ui.vertical_centered(|ui| ui.spinner());
                        return;
                    }
                    Some(Remote::Failed(Problem::Forbidden)) => note(ui, nt::SUMMARIES_REFUSED.get(lang)),
                    Some(Remote::Failed(p)) => {
                        ui.add(egui::Label::new(RichText::new(super::words::problem(lang, p, h24)).color(t.colors.danger)).wrap());
                    }
                    Some(Remote::Ready(list)) if list.is_empty() => note(ui, nt::SUMMARIES_EMPTY.get(lang)),
                    Some(Remote::Ready(list)) => {
                        note(ui, nt::SUMMARIES_TIP.get(lang));
                        ui.add_space(t.spacing.item_y);
                        for s in list {
                            egui::Frame::new().fill(t.colors.bg2).stroke(t.border()).corner_radius(t.r_embed()).inner_margin(t.embed_margin()).show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                if !s.topic.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(&s.topic).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).wrap());
                                }
                                if !s.text.is_empty() {
                                    ui.add(egui::Label::new(RichText::new(&s.text).color(t.colors.text)).wrap().selectable(true));
                                }
                                let mut counts = Vec::new();
                                if s.messages > 0 {
                                    counts.push(np::messages(lang, u64::from(s.messages)));
                                }
                                if s.people > 0 {
                                    counts.push(np::people(lang, u64::from(s.people)));
                                }
                                if !counts.is_empty() {
                                    ui.label(RichText::new(counts.join(" · ")).small().color(t.colors.muted));
                                }
                                if let Some(id) = s.start_id
                                    && ui.small_button(tx::JUMP.get(lang)).clicked()
                                {
                                    jump = Some(id);
                                }
                            });
                            ui.add_space(t.spacing.item_y);
                        }
                    }
                }
                refresh = ui.small_button(nt::REFRESH.get(lang)).clicked();
            });
        });
        close |= super::a11y::escape(ui.ctx());
        if close {
            self.summaries_open = None;
        }
        if let Some(id) = jump {
            self.actions.push(Action::Jump(id));
        }
        if refresh {
            self.store.summaries_refresh(cid);
        }
    }
}
