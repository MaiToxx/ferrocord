//! Fenêtre « Parcourir les salons » (Browse Channels) d'un serveur avec
//! intégration : « Afficher tous les salons », et une case « Suivi » par
//! catégorie et par salon. Chaque case est un geste, une requête
//! (`state::browse`) ; une seule à la fois par serveur.

use super::App;
use crate::i18n::table::events::t as tx;
use crate::state::model::ChannelKind;
use eframe::egui::{self, Id, RichText};

fn open_id() -> Id {
    Id::unique("parcourir-salons")
}

/// Ouvre la fenêtre pour `guild`.
pub(super) fn open(ctx: &egui::Context, guild: u64) {
    ctx.data_mut(|d| d.insert_temp(open_id(), guild));
}

fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<u64>(open_id()));
}

enum Act {
    ShowAll(bool),
    Follow(u64, bool),
}

impl App {
    /// Ouvre la fenêtre pour `guild` (aperçu).
    pub fn show_browse(&self, guild: u64) {
        open(&self.ctx, guild);
    }

    /// Fenêtre ouverte (aperçu, tests).
    pub fn browse_open(&self) -> Option<u64> {
        self.ctx.data(|d| d.get_temp::<u64>(open_id()))
    }

    pub(super) fn browse_window(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(guild) = ctx.data(|d| d.get_temp::<u64>(open_id())) else { return };
        let Some(g) = self.store.guilds.iter().find(|g| g.id == guild) else { return close(&ctx) };
        let t = self.theme.clone();
        let lang = self.lang;
        let busy = self.store.account.guild_busy(Some(guild));
        let show_all = !self.store.browse_only_followed(guild);
        // Salons visibles (VIEW_CHANNEL), dans l'ordre de la liste.
        let rows: Vec<(u64, String, bool, bool)> = g
            .channels
            .iter()
            .filter(|c| !self.store.is_hidden(c.id))
            .map(|c| {
                let category = c.kind == ChannelKind::Category;
                let followed = self.store.browse_followed(guild, c.id, if category { None } else { c.parent });
                let name = if category { c.name.to_uppercase() } else { format!("#{}", c.name) };
                (c.id, name, category, followed)
            })
            .collect();
        let mut act = None;
        // Gabarit commun des fenêtres : titre et croix, contenu qui défile, pas de pied (la liste porte ses cases).
        let width = crate::ui::mobile::card_width(&ctx, &t, 1.0);
        let modal = crate::ui::widgets::sheet(&ctx, &t, Id::unique("parcourir-salons-fenetre"), width, tx::BROWSE_CHANNELS.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.add_enabled_ui(!busy, |ui| {
                let mut all = show_all;
                if ui.checkbox(&mut all, tx::BROWSE_SHOW_ALL.get(lang)).changed() {
                    act = Some(Act::ShowAll(all));
                }
                ui.label(RichText::new(tx::BROWSE_SHOW_ALL_TIP.get(lang)).small().color(t.colors.muted));
                ui.separator();
                egui::ScrollArea::vertical().max_height(t.layout.card_width).show(ui, |ui| {
                    for (id, name, category, followed) in &rows {
                        ui.horizontal(|ui| {
                            if !category {
                                ui.add_space(t.spacing.channel_indent);
                            }
                            let text = if *category { RichText::new(name).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted) } else { RichText::new(name).color(t.colors.text) };
                            ui.label(text);
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let mut on = *followed;
                                if ui.checkbox(&mut on, tx::BROWSE_FOLLOW.get(lang)).changed() {
                                    act = Some(Act::Follow(*id, on));
                                }
                            });
                        });
                    }
                });
            });
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tx::BROWSE_BUSY.get(lang));
                });
            }
        });
        if modal.closed {
            close(&ctx);
        }
        let done = match act {
            Some(Act::ShowAll(all)) => self.store.browse_set_only_followed(guild, !all),
            Some(Act::Follow(id, on)) => self.store.browse_follow(guild, id, on),
            None => return,
        };
        if done.is_err() {
            self.notify(tx::BROWSE_BUSY);
        }
        self.flush_commands();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_close() {
        let ctx = egui::Context::default();
        open(&ctx, 9);
        assert_eq!(ctx.data(|d| d.get_temp::<u64>(open_id())), Some(9));
        close(&ctx);
        assert_eq!(ctx.data(|d| d.get_temp::<u64>(open_id())), None);
    }
}
