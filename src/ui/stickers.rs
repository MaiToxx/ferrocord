//! Autocollants d'un message, sous son texte (lot T4) : PNG, APNG (première
//! image) et GIF (première image) en image ; Lottie (animation vectorielle,
//! sans décodeur) : son nom dans un cadre. Même place à la mesure et au dessin.

use super::emoji;
use crate::i18n::{table::emoji::t as et, Lang};
use crate::state::emoji::StickerItem;
use crate::state::model::Message;
use crate::theme::Tokens;
use eframe::egui::{self, Rect, Sense};

/// Dessine l'autocollant `s` dans `rect` (image, ou nom pour un Lottie).
pub(super) fn paint(ui: &egui::Ui, t: &Tokens, rect: Rect, s: &StickerItem) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    match super::emoji_picker::sticker_img(s) {
        Some(img) => emoji::paint(ui, rect, img),
        None => {
            let p = ui.painter();
            p.rect(rect, t.r_card(), t.colors.bg3, t.border(), egui::StrokeKind::Inside);
            let font = egui::TextStyle::Small.resolve(ui.style());
            let galley = p.layout(s.name.clone(), font, t.colors.muted, rect.width() - 2.0 * t.spacing.item_x);
            let pos = rect.center() - galley.size() / 2.0;
            p.galley(pos, galley, t.colors.muted);
        }
    }
}

/// Autocollants du message `m`, chacun dans un carré de `t.markdown.sticker`.
pub(super) fn stickers_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message) {
    stickers_list(ui, t, lang, &m.meta.stickers);
}

/// Autocollants `list` (ceux d'un message, ou de l'instantané d'un message transféré).
pub(super) fn stickers_list(ui: &mut egui::Ui, t: &Tokens, lang: Lang, list: &[StickerItem]) {
    for s in list {
        let side = t.markdown.sticker;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), Sense::hover());
        paint(ui, t, rect, s);
        let tip = if s.format.has_image() { s.name.clone() } else { format!("{} — {}", s.name, et::ANIMATED_STICKER.get(lang)) };
        resp.on_hover_text(tip);
    }
}
