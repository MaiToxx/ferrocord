//! Menu « + » de la zone de saisie (V1), comme dans le client officiel :
//! envoyer un fichier, créer un fil, créer un sondage, utiliser une
//! application. Le message vocal, propre à Ferrocord sur ordinateur, y est
//! rangé aussi : la barre ne garde, à droite, que GIF, autocollants, emojis.

use super::{menu, App};
use crate::i18n::table::ff::t as ft;
use crate::theme::icons::Icon;
use eframe::egui::{self, Id};

/// Choix fait dans le menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Plus {
    File,
    Thread,
    Poll,
    Apps,
    Voice,
    /// « Programmer… » (plugin des messages programmés, lot E3).
    Schedule,
    /// Liste des messages programmés (lot E3).
    Scheduled,
}

/// Le « + » de la saisie, comme dans le client : une croix au trait simple
/// (sans disque), de la taille des pictogrammes de droite (GIF, emojis), dans
/// la même cible que les boutons-icônes voisins.
fn plus_icon(ui: &mut egui::Ui, t: &crate::theme::Tokens, tip: &str) -> egui::Response {
    let side = super::emoji::row_height(ui, &egui::TextStyle::Body.resolve(ui.style()));
    let hit = crate::theme::Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y).max(side);
    let hit = if crate::ui::mobile::is_active(ui.ctx()) { hit.max(crate::ui::mobile::TOUCH) } else { hit };
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(hit), egui::Sense::click());
    let k = crate::ui::motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
    if k > 0.0 {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover.gamma_multiply(k));
    }
    let at = super::util::pixel_square(ui, rect.center() - egui::Vec2::splat(side / 2.0), side);
    crate::theme::icons::paint(Icon::Plus, ui.painter(), at, crate::ui::motion::mix(t.colors.muted, t.colors.text_strong, k));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    crate::ui::widgets::tip(resp, t, tip)
}

fn menu_id() -> Id {
    Id::unique("ferrocord.saisie.plus")
}

impl App {
    /// Entrées offertes dans ce salon, dans l'ordre de Discord.
    pub(super) fn plus_entries(&self, cid: u64) -> Vec<Plus> {
        let mut out = Vec::new();
        if crate::attach::Attach::CAN_PICK {
            out.push(Plus::File);
        }
        if self.store.can_create_thread(cid) {
            out.push(Plus::Thread);
        }
        if self.store.can_create_poll(cid) {
            out.push(Plus::Poll);
        }
        out.push(Plus::Apps);
        if self.can_start_voice_note(cid) {
            out.push(Plus::Voice);
        }
        // Messages programmés (lot E3) : plugin activé, et salon où l'on peut écrire.
        if crate::plugins::on(crate::plugins::Id::Scheduled) {
            out.push(Plus::Schedule);
            if !self.scheduled_items().is_empty() {
                out.push(Plus::Scheduled);
            }
        }
        out
    }

    /// Bouton « + » à gauche du champ et son menu.
    pub(super) fn plus_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let lang = self.lang;
        let entries = self.plus_entries(cid);
        let resp = plus_icon(ui, &t, ft::PLUS_TIP.get(lang));
        let mut picked = None;
        egui::Popup::menu(&resp).id(menu_id()).show(|ui| {
            for e in &entries {
                let (text, icon) = match e {
                    Plus::File => (ft::PLUS_FILE, Icon::File),
                    Plus::Thread => (ft::PLUS_THREAD, Icon::Thread),
                    Plus::Poll => (ft::PLUS_POLL, Icon::Poll),
                    Plus::Apps => (ft::PLUS_APPS, Icon::Apps),
                    Plus::Voice => (ft::PLUS_VOICE, Icon::Mic),
                    Plus::Schedule => (crate::i18n::table::e3::t::SCHED_MENU, Icon::Clock),
                    Plus::Scheduled => (crate::i18n::table::e3::t::SCHED_LIST, Icon::Calendar),
                };
                if menu::item(ui, &t, text.get(lang), Some(icon), false).clicked() {
                    picked = Some(*e);
                }
            }
        });
        if let Some(p) = picked {
            self.plus_pick(cid, p);
        }
    }

    /// Effet d'un choix du menu (aussi pour l'aperçu et les tests).
    pub(super) fn plus_pick(&mut self, cid: u64, p: Plus) {
        match p {
            Plus::File => self.pick_attachment(cid),
            Plus::Thread => self.open_thread_creator(cid),
            Plus::Poll => self.open_poll_creator(cid),
            // La liste des commandes s'ouvre dès que la saisie commence par « / ».
            Plus::Apps => self.set_composer_text("/"),
            Plus::Voice => self.start_voice_note(cid),
            Plus::Schedule => self.scheduled_open(cid),
            Plus::Scheduled => self.scheduled_open_list(),
        }
    }

    /// Ouvre le menu « + » (aperçu).
    pub fn open_plus_menu(&mut self) {
        egui::Popup::open_id(&self.ctx, menu_id());
    }
}
