//! Messages des personnes bloquées ou ignorées (F1) : la ligne repliée
//! « N messages bloqués — Afficher » que la liste met à la place de leurs
//! messages, comme le client officiel.

use crate::i18n::table::ff::{p, t as ft};
use crate::i18n::Lang;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, RichText};

/// Ligne d'une suite de `n` messages masqués. `ignored` : personne ignorée
/// (sinon bloquée) ; `open` : la suite est dépliée. `live` faux : mesure
/// seule. Vrai si le lien « Afficher » ou « Masquer » vient d'être cliqué.
pub(super) fn bar(ui: &mut egui::Ui, t: &Tokens, lang: Lang, n: u16, ignored: bool, open: bool, live: bool) -> bool {
    let mut clicked = false;
    egui::Frame::new().inner_margin(t.message_margin(false)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
            let size = t.typography.small;
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(Tokens::icon_side(size)), egui::Sense::hover());
            icons::paint(Icon::Close, ui.painter(), rect, t.colors.faint);
            let count = if ignored { p::ignored_messages(lang, u64::from(n)) } else { p::blocked_messages(lang, u64::from(n)) };
            ui.label(RichText::new(count).size(size).color(t.colors.muted));
            let word = match (open, n > 1) {
                (true, true) => ft::HIDE_MESSAGES,
                (true, false) => ft::HIDE_MESSAGE,
                (false, true) => ft::SHOW_MESSAGES,
                (false, false) => ft::SHOW_MESSAGE,
            };
            let link = ui.add(egui::Label::new(RichText::new(word.get(lang)).size(size).color(t.colors.link)).sense(if live { egui::Sense::click() } else { egui::Sense::hover() }));
            if live {
                if link.hovered() {
                    ui.painter().hline(link.rect.x_range(), link.rect.bottom() - 1.0, egui::Stroke::new(1.0, t.colors.link));
                }
                clicked = link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            }
        });
    });
    clicked
}
