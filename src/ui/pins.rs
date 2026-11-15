//! Panneau des messages épinglés du salon (lot T6), chargé à l'ouverture,
//! page par page (50 au plus par page, « Charger plus » pour la suite).

use super::msg_actions::{Action, Glyph};
use super::App;
use crate::i18n::t as tx;
use crate::theme::icons::Icon;
use eframe::egui::{self, RichText};

impl App {
    /// Bouton de l'en-tête du salon qui ouvre ou ferme le panneau.
    pub(super) fn pins_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let open = self.actions.pins_open;
        if super::chat_head::head_icon(ui, &t, tx::PINS_TIP.get(self.lang), open, |p, r, c| super::msg_actions::paint_glyph(Glyph::Pin, p, r, c)).clicked() {
            self.actions.pins_open = !self.actions.pins_open;
        }
        // Chargée à l'ouverture, et relue quand Discord signale un changement.
        if self.actions.pins_open && self.store.pins(cid).is_none_or(|l| !l.loaded && !l.loading) {
            self.store.load_pins(cid, false);
        }
    }

    /// Panneau à droite de la discussion, s'il est ouvert.
    pub(super) fn pins_panel(&mut self, ui: &mut egui::Ui, cid: u64) {
        if !self.actions.pins_open {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let rights = self.store.rights(cid);
        let mut close = false;
        let mut more = false;
        let mut retry = false;
        egui::Panel::right("pins")
            .frame(egui::Frame::new().fill(t.colors.bg1).inner_margin(t.channels_margin()))
            .default_size(t.layout.channels_width)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tx::PINS_TITLE.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = super::util::icon_button(ui, &t, Icon::Close, tx::CLOSE_TIP.get(lang)).clicked();
                    });
                });
                ui.separator();
                let Some(list) = self.store.pins(cid) else { return };
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if list.loaded && list.items.is_empty() && list.error.is_none() && !list.loading {
                        ui.label(RichText::new(tx::PINS_EMPTY.get(lang)).color(t.colors.muted));
                    }
                    for pin in &list.items {
                        let m = &pin.message;
                        egui::Frame::new().fill(t.colors.bg2).stroke(t.border()).corner_radius(t.r_embed()).inner_margin(t.embed_margin()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(RichText::new(&m.author).family(crate::ui::fonts::demi()).strong().color(t.author_color(m.author_id)));
                            if !m.content.is_empty() {
                                ui.add(egui::Label::new(RichText::new(&m.content).color(t.colors.text)).wrap().selectable(true));
                            }
                            for a in &m.attachments {
                                ui.hyperlink_to(&a.name, &a.url);
                            }
                            ui.horizontal(|ui| {
                                // Hors de la mémoire : chargé autour du message (lot T12).
                                if crate::ui::widgets::compact(ui, tx::JUMP.get(lang)).clicked() {
                                    self.actions.push(Action::Jump(m.id));
                                }
                                if rights.pin && crate::ui::widgets::compact(ui, tx::UNPIN_MESSAGE.get(lang)).clicked() {
                                    self.actions.push(Action::Pin(m.id, false));
                                }
                            });
                        });
                        ui.add_space(t.spacing.item_y);
                    }
                    if let Some(e) = &list.error {
                        ui.label(RichText::new(super::words::problem(lang, e, t.layout.clock == crate::theme::Clock::H24)).color(t.colors.danger));
                        retry = crate::ui::widgets::compact(ui, tx::RETRY.get(lang)).clicked();
                    }
                    if list.loading {
                        ui.vertical_centered(|ui| ui.spinner());
                    } else if list.more && list.error.is_none() {
                        more = crate::ui::widgets::compact(ui, tx::LOAD_MORE.get(lang)).clicked();
                    }
                });
            });
        // Échap ferme le panneau (lot C5).
        close |= super::a11y::escape(ui.ctx());
        if close {
            self.actions.pins_open = false;
        }
        if more || retry {
            let more = more || self.store.pins(cid).is_some_and(|l| !l.items.is_empty());
            self.store.load_pins(cid, more);
        }
    }
}
