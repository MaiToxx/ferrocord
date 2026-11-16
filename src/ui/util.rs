//! Petites fonctions de présentation.

use crate::images::{self, Image, Images};
use crate::state::model::{Avatar, ImageRef};
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, emath::GuiRounding, CornerRadius, Rect, Sense, Vec2};

/// Jusqu'à trois initiales en majuscules, pour les icônes de serveur.
pub fn initials(name: &str) -> String {
    // Première lettre ou premier chiffre de chaque mot : « Bob (vocal) » donne « BV », pas « B( ».
    let s: String = name.split_whitespace().filter_map(|w| w.chars().find(|c| c.is_alphanumeric())).take(3).collect();
    if s.is_empty() { "?".into() } else { s.to_uppercase() }
}

/// Acronyme d'un serveur sans icône, comme dans le client : la première
/// lettre de chaque mot (casse gardée), la ponctuation gardée, les espaces
/// retirés ; le « 's » possessif est ignoré. « Projet.cool » donne « P.c »,
/// « Les amis d'Ada » donne « Lad'A ».
pub fn guild_acronym(name: &str) -> String {
    let name = name.replace("'s ", " ");
    let mut out = String::new();
    let mut in_word = false;
    for c in name.chars() {
        if c.is_alphanumeric() || c == '_' {
            if !in_word {
                out.push(c);
            }
            in_word = true;
        } else {
            in_word = false;
            if !c.is_whitespace() {
                out.push(c);
            }
        }
    }
    if out.is_empty() { "?".into() } else { out }
}

/// Initiales d'un avatar dessiné sans image (deux au plus).
pub fn avatar_initials(name: &str) -> String {
    initials(name).chars().take(2).collect()
}

/// Carré de `side` points dont le coin haut gauche est `min`, calé sur les
/// pixels de l'écran (une image nette ne doit pas être à cheval sur deux pixels).
pub fn pixel_square(ui: &egui::Ui, min: egui::Pos2, side: f32) -> Rect {
    Rect::from_min_size(min.round_to_pixels(ui.pixels_per_point()), Vec2::splat(side))
}

/// Image `image` dans `rect` si elle est prête. Ne demande rien pour ce qui
/// n'est pas visible ; renvoie `false` s'il faut dessiner le remplaçant.
pub fn paint_image(ui: &egui::Ui, images: &Images, rect: Rect, image: Option<ImageRef>, r: CornerRadius) -> bool {
    let Some(image) = image else { return false };
    if !ui.is_rect_visible(rect) {
        return false;
    }
    // Avatar ou icône animé (empreinte `a_`) : joué au survol.
    let play = super::motion::get(ui.ctx()).avatars.now(super::motion::hovered(ui, rect));
    match images.get_moving(image, rect.width(), play) {
        Image::Ready(tex) => {
            ui.painter().add(images::textured(rect, r, tex));
            true
        }
        Image::Pending | Image::Failed => false,
    }
}

/// Avatar dans `rect` : l'image si elle est prête, sinon (en attendant, en
/// cas d'échec ou sans image) la pastille aux initiales, de même taille.
pub fn paint_avatar(ui: &egui::Ui, images: &Images, t: &Tokens, rect: Rect, avatar: &Avatar, name: &str) {
    let r = t.r_avatar(rect.width());
    if !ui.is_rect_visible(rect) || paint_image(ui, images, rect, avatar.image, r) {
        return;
    }
    let (fill, text) = t.default_avatar(avatar.default);
    paint_initials(ui, t, rect, (fill, text), name);
}

/// Pastille aux initiales de `name`, couleurs `(fond, texte)`.
pub fn paint_initials(ui: &egui::Ui, t: &Tokens, rect: Rect, (fill, text): (egui::Color32, egui::Color32), name: &str) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let p = ui.painter();
    p.rect_filled(rect, t.r_avatar(rect.width()), fill);
    let font = egui::FontId::proportional(Tokens::initials_size(rect.width()));
    p.text(rect.center(), egui::Align2::CENTER_CENTER, avatar_initials(name), font, text);
}

/// Bouton discret fait d'une icône dessinée (taille du texte des boutons),
/// avec son explication en infobulle.
pub fn icon_button(ui: &mut egui::Ui, t: &Tokens, icon: Icon, tip: &str) -> egui::Response {
    let pad = ui.spacing().button_padding.y;
    let side = Tokens::icon_button_side(t.typography.button, pad);
    // Disposition mobile : la cible a la taille du doigt, l'icône garde la sienne.
    let hit = if crate::ui::mobile::is_active(ui.ctx()) { side.max(crate::ui::mobile::TOUCH) } else { side };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(hit), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let k = crate::ui::motion::hover(ui.ctx(), resp.id, hot);
    if k > 0.0 {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover.gamma_multiply(k));
    }
    let color = crate::ui::motion::mix(t.colors.muted, t.colors.text_strong, k);
    let slot = if hit > side { egui::Rect::from_center_size(rect.center(), Vec2::splat(side)) } else { rect }.shrink(pad);
    // Cran complet : l'icône grossit un peu sous le pointeur.
    let grow = if crate::ui::motion::level(ui.ctx()) == crate::ui::motion::Level::Full { egui::lerp(1.0..=crate::ui::motion::GROW, k) } else { 1.0 };
    icons::paint(icon, ui.painter(), egui::Rect::from_center_size(slot.center(), slot.size() * grow), color);
    // Nommé pour les lecteurs d'écran (l'icône seule ne dit rien).
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    // Infobulle à flèche, immédiate (lot V1).
    crate::ui::widgets::tip(resp, t, tip)
}

/// Icône dessinée alignée sur une ligne de texte de taille `size`.
pub fn inline_icon(ui: &mut egui::Ui, icon: Icon, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(Tokens::icon_side(size)), Sense::hover());
    icons::paint(icon, ui.painter(), rect, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_cases() {
        assert_eq!(initials("rust france"), "RF");
        assert_eq!(initials("Un deux trois quatre"), "UDT");
        assert_eq!(initials("   "), "?");
        assert_eq!(initials("été"), "É");
        assert_eq!(initials("Bob (vocal)"), "BV");
        assert_eq!(initials("-- !"), "?");
    }

    /// Même règle que le client : un mot = une lettre, ponctuation gardée, casse gardée.
    #[test]
    fn guild_acronyms() {
        assert_eq!(guild_acronym("Projet.cool"), "P.c");
        assert_eq!(guild_acronym("rust france"), "rf");
        assert_eq!(guild_acronym("Le Club des Cinq"), "LCdC");
        assert_eq!(guild_acronym("Ada's server"), "As");
        assert_eq!(guild_acronym("Les amis d'Ada"), "Lad'A");
        assert_eq!(guild_acronym("Bob (vocal)"), "B(v)");
        assert_eq!(guild_acronym("Été 2024"), "É2");
        assert_eq!(guild_acronym("   "), "?");
    }
}
