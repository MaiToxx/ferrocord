//! MP du compte système de Discord (« notifications officielles ») :
//! étiquette « ✓ OFFICIEL » (fil, profil, liste des MP) et bandeau qui prend
//! la place de la zone de saisie. Le salon est en lecture seule : les
//! écritures sont refusées par le magasin (`state::store::official`).
//!
//! Rien n'est animé : aucun redessin au repos.

use crate::i18n::table::messages::t as mt;
use crate::i18n::Lang;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, pos2, Color32, Id, Rect, RichText, Sense, Vec2};

/// Article d'aide public de Discord sur ses messages officiels (« How to
/// know a message is really from Discord ») : ouvert par « En savoir plus ».
pub(super) const HELP_URL: &str = "https://support.discord.com/hc/articles/360036118732";

fn peer_id() -> Id {
    Id::unique("ferrocord.officiel.correspondant")
}

/// Compte système du MP affiché (`None` ailleurs) : posé par la discussion
/// avant sa liste, pour que chaque message sache si son auteur est officiel
/// sans rien chercher.
pub(super) fn set_peer(ctx: &egui::Context, peer: Option<u64>) {
    if ctx.data(|d| d.get_temp::<Option<u64>>(peer_id())).flatten() != peer {
        ctx.data_mut(|d| d.insert_temp(peer_id(), peer));
    }
}

/// `user` est le compte système du MP affiché.
pub(super) fn is_peer(ctx: &egui::Context, user: u64) -> bool {
    ctx.data(|d| d.get_temp::<Option<u64>>(peer_id())).flatten() == Some(user)
}

/// Le salon affiché est un MP officiel (barre de réactions masquée).
pub(super) fn shown(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Option<u64>>(peer_id())).flatten().is_some()
}

/// Corps du texte de l'étiquette pour un texte voisin de `size`.
fn tag_font(size: f32) -> egui::FontId {
    crate::ui::fonts::demi_font((size * 0.75).round().max(9.0))
}

/// Taille de l'étiquette à côté d'un texte de `size`.
pub(super) fn tag_size(ui: &egui::Ui, lang: Lang, size: f32) -> Vec2 {
    let font = tag_font(size);
    let text = ui.painter().layout_no_wrap(mt::OFFICIAL_TAG.get(lang).to_string(), font.clone(), Color32::WHITE);
    let pad = (font.size * 0.35).round();
    let h = (font.size * 1.35).round();
    egui::vec2(pad + font.size + 2.0 + text.size().x + pad, h)
}

/// Peint l'étiquette « ✓ OFFICIEL » dans `rect` (de la taille de [`tag_size`]) :
/// coche puis texte, en clair sur l'aplat de la marque.
pub(super) fn paint_tag(ui: &egui::Ui, t: &Tokens, lang: Lang, rect: Rect, size: f32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let font = tag_font(size);
    let p = ui.painter();
    p.rect_filled(rect, (font.size * 0.3).round(), t.colors.accent);
    let pad = (font.size * 0.35).round();
    let check = Rect::from_center_size(pos2(rect.left() + pad + font.size / 2.0, rect.center().y), Vec2::splat(font.size));
    icons::paint(Icon::Check, p, check, t.colors.on_accent);
    let text = p.layout_no_wrap(mt::OFFICIAL_TAG.get(lang).to_string(), font, t.colors.on_accent);
    p.galley(pos2(check.right() + 2.0, (rect.center().y - text.size().y / 2.0).round()), text, t.colors.on_accent);
}

/// Étiquette posée dans la mise en page, à côté d'un nom de corps `size`.
pub(super) fn tag(ui: &mut egui::Ui, t: &Tokens, lang: Lang, size: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(tag_size(ui, lang, size), Sense::hover());
    paint_tag(ui, t, lang, rect, size);
    resp
}

/// Bandeau à la place de la zone de saisie : bouclier, les deux lignes de
/// Discord, « En savoir plus » à droite. `live` faux : mesure seule.
pub(super) fn banner(ui: &mut egui::Ui, t: &Tokens, lang: Lang) {
    let c = &t.colors;
    egui::Frame::new().fill(c.bg3).stroke(t.border()).corner_radius(t.r_composer()).inner_margin(egui::Margin::symmetric(12, 10)).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            let side = 40.0;
            let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            icons::paint(Icon::ShieldLock, ui.painter(), slot.shrink(2.0), c.muted);
            // Pastille cochée en bas à droite du bouclier, comme dans le client.
            let dot = Rect::from_center_size(slot.right_bottom() - Vec2::splat(7.0), Vec2::splat(14.0));
            ui.painter().circle_filled(dot.center(), 8.0, c.bg3);
            ui.painter().circle_filled(dot.center(), 6.5, c.accent);
            icons::paint(Icon::Check, ui.painter(), dot.shrink(3.0), c.on_accent);
            // Bouton mesuré d'abord : le texte prend ce qui reste et se replie.
            let label = crate::i18n::table::nd::t::LEARN_MORE.get(lang);
            let button_w = ui.painter().layout_no_wrap(label.to_string(), crate::ui::fonts::medium_font(t.typography.button), c.text).size().x + 2.0 * t.spacing.control_x;
            let text_w = (ui.available_width() - button_w - 12.0).max(1.0);
            ui.vertical(|ui| {
                ui.set_width(text_w);
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.add(egui::Label::new(RichText::new(mt::OFFICIAL_ONLY.get(lang)).family(crate::ui::fonts::demi()).size(t.typography.body).color(c.text_strong)).wrap());
                ui.add(egui::Label::new(RichText::new(mt::OFFICIAL_NEVER.get(lang)).size(t.typography.small).color(c.muted)).wrap());
            });
            if crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Secondary, label).clicked() {
                // Domaine de Discord : ouvert sans la confirmation des liens externes.
                ui.ctx().open_url(egui::OpenUrl::new_tab(HELP_URL));
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(ctx: &egui::Context, f: impl Fn(&mut egui::Ui)) -> String {
        let mut out = String::new();
        for frame in 0..3 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 300.0))), ..Default::default() };
            let mut o = ctx.run_ui(input, &f);
            o.textures_delta.clear();
            out.clear();
            fn walk(s: &egui::Shape, out: &mut String) {
                match s {
                    egui::Shape::Text(t) => {
                        out.push_str(t.galley.text());
                        out.push('\n');
                    }
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            o.shapes.iter().for_each(|s| walk(&s.shape, &mut out));
            // Au repos (troisième image) : aucun redessin demandé.
            let idle = o.viewport_output.values().all(|v| v.repaint_delay == std::time::Duration::MAX);
            assert!(frame < 2 || idle, "au repos : aucun redessin");
        }
        out
    }

    #[test]
    fn banner_says_what_discord_says_and_asks_for_no_repaint() {
        let ctx = egui::Context::default();
        let t = Tokens::default();
        let got = texts(&ctx, |ui| banner(ui, &t, Lang::Fr));
        for want in ["Ce salon est réservé aux notifications officielles de Discord.", "Discord ne te demandera jamais ton mot de passe ou ton token de compte.", "En savoir plus"] {
            assert!(got.contains(want), "« {want} » absent : {got}");
        }
        assert!(crate::ui::links::own_https(HELP_URL), "lien d'aide : un domaine de Discord, en https");
    }

    #[test]
    fn tag_and_peer_flag() {
        let ctx = egui::Context::default();
        let t = Tokens::default();
        assert!(texts(&ctx, |ui| drop(tag(ui, &t, Lang::Fr, 16.0))).contains("OFFICIEL"));
        assert!(texts(&ctx, |ui| drop(tag(ui, &t, Lang::En, 16.0))).contains("OFFICIAL"));
        assert!(!shown(&ctx) && !is_peer(&ctx, 7));
        set_peer(&ctx, Some(7));
        assert!(shown(&ctx) && is_peer(&ctx, 7) && !is_peer(&ctx, 8));
        set_peer(&ctx, None);
        assert!(!shown(&ctx));
    }
}
