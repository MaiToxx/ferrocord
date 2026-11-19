//! Accusés de lecture côté interface : un salon n'est accusé que s'il est
//! vraiment lu (fenêtre active, bas de la liste visible) ; « marquer comme
//! lu » sur un salon et sur un serveur (clic droit) ; notifications sociales.

use super::{friends::notice_text, App};
use crate::i18n::table::social::t as st;
use eframe::egui;
use std::time::Instant;

impl App {
    /// À chaque passage : accusés arrivés à échéance et notifications
    /// sociales. Le seul réveil programmé est l'échéance du prochain accusé.
    pub(super) fn relations_tick(&mut self) {
        let now = Instant::now();
        if let Some(due) = self.store.flush_acks(now) {
            self.ctx.request_repaint_after(due.saturating_duration_since(now));
        }
        while let Some(n) = self.store.relations.take_notice() {
            // Onglet « Ajouter » ouvert : le résultat s'affiche sous la carte.
            if self.add_friend_result(&n) {
                continue;
            }
            let text = notice_text(self.lang, &n, self.h24());
            self.notify(text);
        }
    }

    /// Fin de l'image d'un salon : `at_bottom` si le bas de la liste est visible.
    pub(super) fn note_reading(&mut self, ui: &egui::Ui, channel: u64, at_bottom: bool) {
        if at_bottom && ui.input(|i| i.focused) {
            self.store.note_reading(channel, Instant::now());
        }
    }

    /// Entrée du menu contextuel d'un salon de serveur (le menu lui-même,
    /// avec les notifications, est ouvert par `channels.rs`) : `true` si
    /// « Marquer comme lu » est choisi.
    pub(super) fn channel_menu(&self, ui: &mut egui::Ui) -> bool {
        let chosen = crate::ui::menu::item(ui, &self.theme, st::MARK_READ.get(self.lang), Some(crate::theme::icons::Icon::Check), false).clicked();
        if chosen {
            ui.close();
        }
        chosen
    }

    /// Entrée « Tout marquer comme lu » des menus du rail (accueil, serveur,
    /// dossier) : tous les serveurs et les conversations, d'un seul geste.
    pub(super) fn mark_all_entry(&self, ui: &mut egui::Ui) -> bool {
        let chosen = crate::ui::menu::item(ui, &self.theme, crate::i18n::table::nav::t::MARK_ALL_READ.get(self.lang), Some(crate::theme::icons::Icon::CheckAll), false).clicked();
        if chosen {
            ui.close();
        }
        chosen
    }

    /// Entrée du menu contextuel d'un serveur du rail (menu ouvert par
    /// `rail.rs`) : `true` si « Marquer le serveur comme lu » est choisi.
    pub(super) fn guild_menu(&self, ui: &mut egui::Ui) -> bool {
        let chosen = crate::ui::menu::item(ui, &self.theme, st::MARK_GUILD_READ.get(self.lang), Some(crate::theme::icons::Icon::Check), false).clicked();
        if chosen {
            ui.close();
        }
        chosen
    }
}
