//! Suggestions d'autocollants pendant la frappe (F17) : quand la saisie ne
//! contient qu'un mot et qu'il commence le nom (ou une étiquette) d'un
//! autocollant du serveur, ces autocollants sont proposés dans une bande
//! au-dessus du champ. Un clic envoie l'autocollant à la place du mot ; la
//! croix écarte la suggestion pour ce mot. Réglage : Accessibilité, « Suggérer
//! des autocollants pendant la frappe » (`LocalPrefs::sticker_hints`).
//!
//! Le choix des autocollants est dans `Store::sticker_hints`.

use super::App;
use crate::i18n::{t as tx, table::restes::t as ct};
use crate::state::emoji::StickerItem;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Id, Rect, Sense};

/// Côté d'un autocollant proposé.
const SIDE: f32 = 56.0;
/// Mot le plus court et le plus long qui déclenchent une suggestion.
const WORD: std::ops::RangeInclusive<usize> = 3..=30;

/// Ce qui est retenu d'une image à l'autre.
#[derive(Clone, Default)]
struct Kept {
    /// Mot pour lequel la suggestion a été écartée.
    dismissed: Option<String>,
    /// Place de la bande à l'image précédente : elle reste pendant un clic,
    /// qui retire le clavier au champ.
    rect: Option<Rect>,
}

fn kept_id() -> Id {
    Id::unique("ferrocord.saisie.autocollants")
}

/// Mot à comparer aux autocollants : le texte entier s'il ne fait qu'un mot
/// de lettres et de chiffres (ni mention, ni commande, ni lien, ni phrase).
pub(super) fn word(text: &str) -> Option<&str> {
    let w = text.trim();
    (WORD.contains(&w.chars().count()) && w.chars().all(char::is_alphanumeric)).then_some(w)
}

/// Place de la bande si elle est affichée (essais).
#[cfg(test)]
pub(super) fn shown(ctx: &egui::Context) -> Option<Rect> {
    ctx.data(|d| d.get_temp::<Kept>(kept_id())).and_then(|k| k.rect)
}

impl App {
    /// Bande des autocollants proposés pour `word`, au-dessus du champ
    /// `field` (qui occupe `anchor`).
    pub(super) fn sticker_hints(&mut self, ui: &egui::Ui, field: Id, anchor: Rect, cid: u64, word: Option<String>, t: &Tokens) {
        let ctx = ui.ctx().clone();
        let before: Option<Kept> = ctx.data(|d| d.get_temp(kept_id()));
        let Some(word) = word else {
            if before.is_some() {
                ctx.data_mut(|d| d.remove::<Kept>(kept_id()));
            }
            return;
        };
        let mut kept = before.unwrap_or_default();
        if kept.dismissed.as_deref().is_some_and(|d| d != word) {
            kept.dismissed = None;
        }
        let over = kept.rect.is_some_and(|r| ctx.input(|i| i.pointer.latest_pos()).is_some_and(|p| r.contains(p)));
        let wanted = self.local.prefs.sticker_hints && kept.dismissed.is_none() && (over || ctx.memory(|m| m.has_focus(field)));
        let list: Vec<StickerItem> = if wanted { self.store.sticker_hints(cid, &word).into_iter().map(|s| s.item()).collect() } else { Vec::new() };
        if list.is_empty() {
            kept.rect = None;
            ctx.data_mut(|d| d.insert_temp(kept_id(), kept));
            return;
        }
        let lang = self.lang;
        let (mut picked, mut close) = (None, false);
        let pos = anchor.left_top() - egui::vec2(0.0, 2.0 * t.spacing.item_y + 4.0);
        let area = egui::Area::new(kept_id()).order(egui::Order::Foreground).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(pos).constrain_to(ctx.content_rect()).show(&ctx, |ui| {
            egui::Frame::new().fill(t.colors.bg3).corner_radius(t.r_control()).stroke(egui::Stroke::new(t.shapes.border.max(1.0), t.colors.border)).inner_margin(egui::Margin::same(t.spacing.item_y as i8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.spacing.item_y;
                    for s in &list {
                        let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(SIDE), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, t.r_control(), t.colors.hover);
                        }
                        super::stickers::paint(ui, t, rect.shrink(3.0), s);
                        let label = ct::STICKER_HINT_SEND.get(lang).replacen("{}", &s.name, 1);
                        super::a11y::name(ui.ctx(), resp.id, &label);
                        if resp.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            picked = Some(s.clone());
                        }
                    }
                    close = super::util::icon_button(ui, t, Icon::Close, ct::STICKER_HINTS_CLOSE.get(lang)).clicked();
                });
            });
        });
        kept.rect = Some(area.response.rect);
        // Un choix (envoi ou fermeture) : le clavier revient au champ.
        let acted = picked.is_some() || close;
        if let Some(s) = picked {
            // Même envoi que depuis le sélecteur (`composer_emoji`).
            if self.store.send_sticker(cid, s.id, s.name) {
                self.clear_composer();
                self.scroll_to_bottom = true;
                kept.rect = None;
            } else {
                self.notify(tx::CANNOT_SEND);
            }
        }
        if close {
            kept.dismissed = Some(word);
            kept.rect = None;
        }
        if acted {
            ctx.memory_mut(|m| m.request_focus(field));
            ctx.request_repaint();
        }
        ctx.data_mut(|d| d.insert_temp(kept_id(), kept));
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_a_single_plain_word_asks_for_stickers() {
        use super::word;
        assert_eq!(word("  Coucou "), Some("Coucou"));
        assert_eq!(word("été"), Some("été"));
        for no in ["", "ab", "deux mots", "@moi", ":fire:", "/giphy", "https://exemple.invalid", "ligne\nautre", &"a".repeat(31)] {
            assert_eq!(word(no), None, "{no:?}");
        }
    }
}
