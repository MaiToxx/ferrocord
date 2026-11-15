//! Lignes de menu contextuel au gabarit du client (lot V1,
//! `docs/ecarts-visuels.md` § 2.16) : texte à gauche en graisse moyenne,
//! icône, case ou rond à droite, ligne survolée en aplat, action
//! destructrice en rouge. À poser dans un menu d'egui (`context_menu`,
//! `menu_button`) : son cadre (fond `menu_bg`, rayon, marge, ombre) vient du
//! thème, pour tous les menus, ceux qui posent encore de simples boutons compris.

use super::fonts;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, pos2, vec2, Color32, Rect, Response, Sense, Stroke, Ui, Vec2};

/// Largeur minimale d'un menu.
pub(crate) const MIN_WIDTH: f32 = 188.0;
/// Côté de l'icône, de la case ou du rond posés à droite d'une ligne.
const MARK: f32 = 18.0;

/// Ce qu'une ligne porte à droite de son texte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Mark {
    None,
    Icon(Icon),
    /// Case à cocher.
    Check(bool),
    /// Choix unique.
    Radio(bool),
    /// Ouvre un sous-menu.
    Submenu,
    /// Place gardée à droite : l'appelant y pose son propre bouton.
    Slot,
}

/// Ce qu'une ligne de carte porte à gauche de son texte.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Lead {
    None,
    Icon(Icon),
    /// Pastille de couleur (statut).
    Dot(Color32),
}

/// Ligne de menu d'une carte (carte de mon compte) ou de son sous-menu :
/// icône ou pastille à gauche, texte, description facultative dessous, marque
/// à droite.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Line<'a> {
    pub lead: Lead,
    pub text: &'a str,
    /// Courte description, sous le texte.
    pub hint: Option<&'a str>,
    pub mark: Mark,
    /// Son sous-menu est ouvert : elle reste en aplat.
    pub open: bool,
    /// Posée sur le fond d'une carte (aplat de survol discret, texte de la
    /// carte) et non dans un menu (aplat d'accent).
    pub on_card: bool,
}

impl<'a> Line<'a> {
    pub(crate) fn new(lead: Lead, text: &'a str) -> Self {
        Line { lead, text, hint: None, mark: Mark::None, open: false, on_card: false }
    }
}

/// Couleurs d'une ligne de carte : fond (survolée), texte, description.
pub(crate) fn line_colors(t: &Tokens, hot: bool, on_card: bool) -> (Color32, Color32, Color32) {
    match (on_card, hot) {
        (true, false) => (Color32::TRANSPARENT, t.colors.text, t.colors.muted),
        (true, true) => (t.colors.row_hover, t.colors.text_strong, t.colors.text),
        (false, false) => (Color32::TRANSPARENT, t.colors.text, t.colors.muted),
        (false, true) => {
            let (fill, ink) = t.menu_hover();
            (fill, ink, ink)
        }
    }
}

/// Ligne à icône de tête (voir [`Line`]) ; large de toute la place offerte.
/// Le texte trop long est coupé, la description passe à la ligne.
pub(crate) fn line(ui: &mut Ui, t: &Tokens, l: Line<'_>) -> Response {
    let pad = t.spacing.item_x;
    let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
    let lead_w = if l.lead == Lead::None { 0.0 } else { MARK + pad };
    let mark_w = if l.mark == Mark::None { 0.0 } else { MARK + pad };
    let width = offered(ui).max(MIN_WIDTH);
    let room = (width - 2.0 * pad - lead_w - mark_w).max(1.0);
    let mut job = egui::text::LayoutJob::simple_singleline(l.text.to_owned(), fonts::medium_font(t.typography.button), Color32::PLACEHOLDER);
    job.wrap = egui::text::TextWrapping::truncate_at_width(room);
    let text = ui.painter().layout_job(job);
    let hint = l.hint.map(|h| ui.painter().layout(h.to_owned(), egui::FontId::proportional(t.typography.small), Color32::PLACEHOLDER, room));
    let hint_h = hint.as_ref().map_or(0.0, |g| g.size().y);
    let height = row_height(t).max(text.size().y + hint_h + 2.0 * t.spacing.item_y).ceil();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), l.text));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hot = ui.is_enabled() && (resp.hovered() || resp.has_focus() || l.open);
    let (fill, ink, soft) = line_colors(t, hot, l.on_card);
    let (ink, soft) = (ink.gamma_multiply(fade), soft.gamma_multiply(fade));
    let p = ui.painter();
    if hot {
        let k = super::motion::hover(ui.ctx(), resp.id, true);
        p.rect_filled(rect, t.r_row().at_most(4), fill.gamma_multiply(k.max(0.35)));
    } else {
        super::motion::hover(ui.ctx(), resp.id, false);
    }
    // Icône et marque à la hauteur de la première ligne de texte.
    let top = rect.top() + (height - text.size().y - hint_h) / 2.0;
    let mid = top + text.size().y / 2.0;
    let slot = Rect::from_center_size(pos2(rect.left() + pad + MARK / 2.0, mid), Vec2::splat(MARK));
    match l.lead {
        Lead::None => {}
        Lead::Icon(icon) => icons::paint(icon, p, slot, ink),
        Lead::Dot(color) => {
            p.circle_filled(slot.center(), t.shapes.mention_dot, color.gamma_multiply(fade));
        }
    }
    let x = rect.left() + pad + lead_w;
    let text_h = text.size().y;
    p.galley(pos2(x, top), text, ink);
    if let Some(g) = hint {
        p.galley(pos2(x, top + text_h), g, soft);
    }
    let slot = Rect::from_center_size(pos2(rect.right() - pad - MARK / 2.0, mid), Vec2::splat(MARK));
    match l.mark {
        Mark::Icon(icon) => icons::paint(icon, p, slot, ink),
        Mark::Submenu => icons::paint(Icon::ChevronRight, p, slot.shrink(2.0), ink),
        _ => {}
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Emplacement de la marque de droite d'une ligne `rect` posée par [`line`]
/// sans description ([`Mark::Slot`]).
pub(crate) fn mark_slot(t: &Tokens, rect: Rect) -> Rect {
    Rect::from_center_size(pos2(rect.right() - t.spacing.item_x - MARK / 2.0, rect.center().y), Vec2::splat(MARK))
}

/// Hauteur d'une ligne : celle des contrôles du thème (32).
pub(crate) fn row_height(t: &Tokens) -> f32 {
    t.spacing.control_height.max((t.typography.button * 1.35).ceil() + 2.0 * t.spacing.item_y)
}

/// Ligne de menu ; `danger` : action destructrice. Le menu se ferme au clic
/// comme pour un bouton d'egui (sauf case et rond, qui restent ouverts : à
/// l'appelant d'appeler `ui.close()` s'il le veut).
pub(crate) fn item(ui: &mut Ui, t: &Tokens, text: &str, icon: Option<Icon>, danger: bool) -> Response {
    let resp = row(ui, t, text, icon.map_or(Mark::None, Mark::Icon), danger);
    if resp.clicked() {
        ui.close();
    }
    resp
}

/// Ligne sans fermeture : pour un menu qui n'est pas un menu d'egui (menu d'un
/// utilisateur, posé dans une zone flottante) et qui se ferme lui-même.
#[allow(dead_code)]
pub(crate) fn plain(ui: &mut Ui, t: &Tokens, text: &str, icon: Option<Icon>, danger: bool) -> Response {
    row(ui, t, text, icon.map_or(Mark::None, Mark::Icon), danger)
}

/// Case à cocher dans un menu (à droite du texte) ; `true` si elle vient de changer.
pub(crate) fn check(ui: &mut Ui, t: &Tokens, text: &str, on: &mut bool) -> bool {
    let clicked = row(ui, t, text, Mark::Check(*on), false).clicked();
    if clicked {
        *on = !*on;
    }
    clicked
}

/// Choix unique dans un menu (rond à droite du texte).
pub(crate) fn radio(ui: &mut Ui, t: &Tokens, text: &str, on: bool) -> Response {
    row(ui, t, text, Mark::Radio(on), false)
}

/// Ligne qui ouvre un sous-menu (chevron à droite).
pub(crate) fn submenu(ui: &mut Ui, t: &Tokens, text: &str, add: impl FnOnce(&mut Ui)) {
    let resp = row(ui, t, text, Mark::Submenu, false);
    egui::containers::menu::SubMenu::default().show(ui, &resp, add);
}

/// Filet entre deux groupes de lignes.
pub(crate) fn separator(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(offered(ui), 2.0 * t.spacing.item_y + 1.0), Sense::hover());
    let y = rect.center().y.round();
    ui.painter().line_segment([pos2(rect.left() + t.spacing.item_y, y), pos2(rect.right() - t.spacing.item_y, y)], Stroke::new(1.0, t.colors.separator));
}

/// Largeur offerte à une ligne. À la passe de mesure d'un menu d'egui, la
/// largeur disponible est celle de l'écran : la prendre étirerait le menu ;
/// chaque ligne ne compte alors que pour son contenu.
fn offered(ui: &Ui) -> f32 {
    if ui.is_sizing_pass() { 0.0 } else { ui.available_width() }
}

/// Couleurs d'une ligne : fond (survolée), texte.
pub(crate) fn row_colors(t: &Tokens, hot: bool, danger: bool) -> (Color32, Color32) {
    match (hot, danger) {
        (false, false) => (Color32::TRANSPARENT, t.colors.muted),
        (false, true) => (Color32::TRANSPARENT, t.colors.danger),
        (true, false) => t.menu_hover(),
        (true, true) => (t.danger_fill(), t.colors.on_accent),
    }
}

fn row(ui: &mut Ui, t: &Tokens, text: &str, mark: Mark, danger: bool) -> Response {
    let pad = t.spacing.item_x;
    let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), fonts::medium_font(t.typography.button), Color32::PLACEHOLDER);
    let mark_w = if mark == Mark::None { 0.0 } else { MARK + pad };
    let width = offered(ui).max(MIN_WIDTH).max(galley.size().x + 2.0 * pad + mark_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(width, row_height(t)), Sense::click());
    let (role, selected) = match mark {
        Mark::Check(on) => (egui::Role::CheckBox, Some(on)),
        Mark::Radio(on) => (egui::Role::RadioButton, Some(on)),
        _ => (egui::Role::Button, None),
    };
    resp.widget_info(|| match selected {
        Some(on) => egui::WidgetInfo::selected(role, ui.is_enabled(), on, text),
        None => egui::WidgetInfo::labeled(role, ui.is_enabled(), text),
    });
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hot = ui.is_enabled() && (resp.hovered() || resp.has_focus());
    let (fill, ink) = row_colors(t, hot, danger);
    let ink = ink.gamma_multiply(fade);
    let p = ui.painter();
    if hot {
        // Le texte change tout de suite (il doit rester lisible), l'aplat arrive en fondu.
        let k = super::motion::hover(ui.ctx(), resp.id, true);
        p.rect_filled(rect, t.r_row().at_most(4), fill.gamma_multiply(k.max(0.35)));
    } else {
        super::motion::hover(ui.ctx(), resp.id, false);
    }
    p.galley(pos2(rect.left() + pad, rect.center().y - galley.size().y / 2.0), galley, ink);
    let slot = Rect::from_center_size(pos2(rect.right() - pad - MARK / 2.0, rect.center().y), Vec2::splat(MARK));
    match mark {
        Mark::None | Mark::Slot => {}
        Mark::Icon(icon) => icons::paint(icon, p, slot, ink),
        Mark::Submenu => icons::paint(Icon::ChevronRight, p, slot.shrink(2.0), ink),
        Mark::Check(on) => {
            let r = (slot.width() * 0.22).round();
            if on {
                // Coché : aplat d'accent (blanc sur une ligne survolée en accent), coche du ton opposé.
                let (bg, fg) = if hot && fill == t.colors.accent { (t.colors.on_accent, t.colors.accent) } else { (t.colors.accent, t.colors.on_accent) };
                p.rect_filled(slot, r, bg.gamma_multiply(fade));
                icons::paint(Icon::Check, p, slot.shrink(3.0), fg);
            } else {
                p.rect_stroke(slot, r, Stroke::new(1.5, ink), egui::StrokeKind::Inside);
            }
        }
        Mark::Radio(on) => {
            p.circle_stroke(slot.center(), slot.width() / 2.0 - 1.0, Stroke::new(1.5, ink));
            if on {
                p.circle_filled(slot.center(), slot.width() / 2.0 - 4.5, ink);
            }
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, Preset, MIN_MUTED, MIN_TEXT};

    /// Texte d'une ligne lisible sur le fond des menus, au repos, survolée et en rouge.
    #[test]
    fn rows_are_readable_in_every_preset() {
        for p in Preset::ALL {
            let t = p.tokens();
            let bg = t.colors.menu_bg;
            for danger in [false, true] {
                let (_, ink) = row_colors(&t, false, danger);
                // Le rouge de danger est un texte court et gras : 3:1 suffit, comme sur les autres fonds sombres.
                let min = if danger { MIN_MUTED } else { MIN_TEXT };
                assert!(contrast_ratio(ink, bg) >= min, "{} danger={danger} : {:.2}", p.key(), contrast_ratio(ink, bg));
                let (fill, ink) = row_colors(&t, true, danger);
                assert!(contrast_ratio(ink, fill) >= MIN_TEXT, "{} survol danger={danger} : {:.2}", p.key(), contrast_ratio(ink, fill));
            }
        }
    }

    /// Ligne de carte : lisible au repos et survolée, sur le fond de la carte
    /// (`bg1`) comme dans un sous-menu (`menu_bg`), description comprise.
    #[test]
    fn card_lines_are_readable_in_every_preset() {
        for p in Preset::ALL {
            let t = p.tokens();
            for (on_card, bg) in [(true, t.colors.bg1), (false, t.colors.menu_bg)] {
                let (_, ink, soft) = line_colors(&t, false, on_card);
                assert!(contrast_ratio(ink, bg) >= MIN_TEXT, "{} carte={on_card} : {:.2}", p.key(), contrast_ratio(ink, bg));
                assert!(contrast_ratio(soft, bg) >= MIN_MUTED, "{} carte={on_card} description : {:.2}", p.key(), contrast_ratio(soft, bg));
                let (fill, ink, soft) = line_colors(&t, true, on_card);
                assert!(contrast_ratio(ink, fill) >= MIN_TEXT, "{} carte={on_card} survol : {:.2}", p.key(), contrast_ratio(ink, fill));
                assert!(contrast_ratio(soft, fill) >= MIN_TEXT, "{} carte={on_card} survol, description : {:.2}", p.key(), contrast_ratio(soft, fill));
            }
        }
    }

    /// Une ligne fait la hauteur des contrôles et au moins la largeur minimale ; la case bascule au clic.
    #[test]
    fn row_geometry_and_check() {
        let ctx = egui::Context::default();
        fonts::install_quiet(&ctx);
        let t = Preset::Sombre.tokens().scaled();
        t.apply(&ctx);
        let mut size = Vec2::ZERO;
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
            ui.allocate_ui(vec2(40.0, 200.0), |ui| size = row(ui, &t, "Copier le lien", Mark::Icon(Icon::Link), false).rect.size());
        });
        out.textures_delta.clear();
        assert_eq!(size.y, 32.0);
        assert!(size.x >= MIN_WIDTH);
    }
}
