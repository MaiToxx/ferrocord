//! Composants partagés (lot V1, `docs/ecarts-visuels.md` § 2.15 et 2.16) :
//! bouton (primaire, secondaire, danger, succès, lien), champ de formulaire,
//! infobulle à flèche, fenêtre modale à en-tête et pied. Tout vient des
//! jetons : aucun écran n'a de couleur ni de taille à écrire.
//!
//! Les menus sont dans `menu.rs`, l'interrupteur dans `settings/shell.rs`.

use super::{fonts, motion};
use crate::theme::{icons, icons::Icon, Elevation, Tokens};
use eframe::egui::{self, pos2, vec2, Align, Align2, Color32, CornerRadius, Id, Layout, Margin, Rect, Response, RichText, Sense, Shape, Stroke, Ui, Vec2};

/// Rôle d'un bouton : il règle son aplat et son texte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Action principale d'une fenêtre : aplat d'accent.
    Primary,
    /// Action ordinaire (« Annuler ») : aplat neutre.
    Secondary,
    /// Action destructrice : aplat rouge.
    Danger,
    /// Action positive (« Rejoindre », « Enregistrer ») : aplat vert.
    Success,
    /// Action discrète : texte seul, souligné au survol.
    Link,
}

/// Fonce (thème sombre) ou éclaircit (aplat sombre sur thème clair) un aplat survolé.
fn shade(c: Color32, by: f32) -> Color32 {
    let f = |v: u8| (f32::from(v) * (1.0 - by)).round() as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// Aplat au repos, aplat survolé, texte.
fn colors(t: &Tokens, kind: Kind) -> (Color32, Color32, Color32) {
    let c = &t.colors;
    match kind {
        Kind::Primary => (c.accent, shade(c.accent, 0.14), c.on_accent),
        Kind::Secondary => (c.button_bg, c.control_bg, c.button_text),
        Kind::Danger => (t.danger_fill(), shade(t.danger_fill(), 0.14), c.on_accent),
        Kind::Success => (t.success_fill(), shade(t.success_fill(), 0.14), c.on_accent),
        Kind::Link => (Color32::TRANSPARENT, Color32::TRANSPARENT, c.text),
    }
}

/// Aplat et texte d'un bouton désactivé : l'aplat fondu dans le fond de la
/// fenêtre, et celui du texte du bouton ou du texte du thème qui s'y lit le
/// mieux (blanc sur lavande pâle était invisible en thème clair), adouci tant
/// qu'il garde 3:1.
pub(crate) fn disabled(t: &Tokens, kind: Kind) -> (Color32, Color32) {
    use crate::theme::{contrast_ratio, MIN_MUTED};
    let (fill, _, ink) = colors(t, kind);
    let surface = t.colors.bg2;
    let fill = if kind == Kind::Link { Color32::TRANSPARENT } else { motion::mix(surface, fill, 0.45) };
    let under = if kind == Kind::Link { surface } else { fill };
    let best = if contrast_ratio(ink, under) >= contrast_ratio(t.colors.text_strong, under) { ink } else { t.colors.text_strong };
    let soft = motion::mix(under, best, 0.6);
    (fill, if contrast_ratio(soft, under) >= MIN_MUTED { soft } else { best })
}

/// Hauteur d'un bouton : celle des contrôles du thème, jamais moins que son texte et ses marges.
pub(crate) fn button_height(t: &Tokens) -> f32 {
    t.spacing.control_height.max((t.typography.button * 1.35).ceil() + 2.0 * t.spacing.control_y)
}

/// Bouton plein (ou lien), en graisse moyenne. Désactivé si `ui` l'est.
pub(crate) fn button(ui: &mut Ui, t: &Tokens, kind: Kind, text: &str) -> Response {
    button_wide(ui, t, kind, text, 0.0)
}

/// Comme [`button`], large de `min_width` au moins (bouton pleine largeur d'un formulaire).
pub(crate) fn button_wide(ui: &mut Ui, t: &Tokens, kind: Kind, text: &str, min_width: f32) -> Response {
    let (mut fill, mut hover, mut ink) = colors(t, kind);
    if !ui.is_enabled() {
        (fill, ink) = disabled(t, kind);
        hover = fill;
    }
    let galley = ui.painter().layout_no_wrap(text.to_owned(), fonts::medium_font(t.typography.button), ink);
    let pad = if kind == Kind::Link { t.spacing.item_x } else { t.spacing.control_x };
    let size = vec2((galley.size().x + 2.0 * pad).max(min_width), button_height(t));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), text));
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus();
        let p = ui.painter();
        if kind != Kind::Link {
            let k = motion::hover(ui.ctx(), resp.id, hot);
            let fill = if resp.is_pointer_button_down_on() { shade(hover, 0.1) } else { motion::mix(fill, hover, k) };
            p.rect_filled(rect, t.r_control(), fill);
        }
        let at = rect.center() - galley.size() / 2.0;
        if kind == Kind::Link && hot {
            let y = (at.y + galley.size().y).round() - 1.0;
            p.line_segment([pos2(at.x, y), pos2(at.x + galley.size().x, y)], Stroke::new(1.0, ink));
        }
        p.galley(at, galley, ink);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Comme [`button`], dans un rectangle imposé (boutons de même largeur au pied
/// d'une carte) : un texte trop long finit par « … » ; `icon` le suit à droite
/// (chevron d'un menu). Le nom accessible reste le texte entier.
pub(crate) fn button_in(ui: &mut Ui, t: &Tokens, kind: Kind, text: &str, icon: Option<Icon>, rect: Rect) -> Response {
    let (mut fill, mut hover, mut ink) = colors(t, kind);
    if !ui.is_enabled() {
        (fill, ink) = disabled(t, kind);
        hover = fill;
    }
    let resp = ui.allocate_rect(rect, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), text));
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus();
        let p = ui.painter();
        let k = motion::hover(ui.ctx(), resp.id, hot);
        let fill = if resp.is_pointer_button_down_on() { shade(hover, 0.1) } else { motion::mix(fill, hover, k) };
        p.rect_filled(rect, t.r_control(), fill);
        let side = Tokens::icon_side(t.typography.button);
        let extra = if icon.is_some() { side + t.spacing.item_x / 2.0 } else { 0.0 };
        let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), fonts::medium_font(t.typography.button), ink);
        job.wrap = egui::text::TextWrapping { max_width: (rect.width() - 2.0 * t.spacing.item_x - extra).max(0.0), max_rows: 1, break_anywhere: true, ..Default::default() };
        let galley = p.layout_job(job);
        let width = galley.size().x + extra;
        let at = pos2(rect.center().x - width / 2.0, rect.center().y - galley.size().y / 2.0);
        if let Some(icon) = icon {
            let slot = Rect::from_center_size(pos2(at.x + width - side / 2.0, rect.center().y), Vec2::splat(side));
            icons::paint(icon, p, slot, ink);
        }
        p.galley(at, galley, ink);
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Hauteur d'un bouton compact : un cran sous celle des contrôles, jamais
/// moins que 26 (le texte de 14 garde de l'air au-dessus et au-dessous).
pub(crate) const COMPACT_TRIM: f32 = 4.0;

/// Bouton neutre compact, pour une action posée dans une ligne (« Réessayer »,
/// « Actualiser », « Aller au message ») : texte de bouton, hauteur des
/// contrôles moins [`COMPACT_TRIM`]. Remplace `small_button` d'egui, dont la
/// hauteur (24) écrasait le texte à côté de contrôles de 32 à 40.
pub(crate) fn compact(ui: &mut Ui, text: impl Into<egui::WidgetText>) -> Response {
    let h = (ui.spacing().interact_size.y - COMPACT_TRIM).max(26.0);
    let pad = ui.spacing().button_padding.x.min(10.0);
    ui.scope(|ui| {
        ui.spacing_mut().button_padding = vec2(pad, 0.0);
        ui.spacing_mut().interact_size.y = h;
        ui.add(egui::Button::new(text).min_size(vec2(0.0, h)))
    })
    .inner
}

/// Champ de formulaire sur une ligne, haut de [`Tokens::field_height`] (40),
/// sur toute la largeur offerte.
pub(crate) fn field(ui: &mut Ui, t: &Tokens, text: &mut String, hint: &str) -> Response {
    let size = vec2(ui.available_width(), t.field_height());
    let edit = egui::TextEdit::singleline(text)
        .hint_text(RichText::new(hint).color(t.colors.faint))
        .font(egui::FontId::proportional(t.typography.body))
        .margin(Margin::symmetric(px(t.spacing.item_x + t.spacing.item_y), 0))
        .align(egui::Align2::LEFT_CENTER);
    ui.add_sized(size, edit)
}

/// Rangée « champ et bouton » : haute comme le champ, le bouton centré sur lui
/// (dans une rangée ordinaire, posé le premier, il restait calé sur le haut du
/// champ, plus haut que lui). `add` pose de droite à gauche : le bouton
/// d'abord, le champ ensuite, qui prend la place restante.
pub(crate) fn field_row<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.field_height()), Layout::right_to_left(Align::Center), add).inner
}

/// Libellé d'un champ de formulaire : petit, gras, au-dessus du champ.
pub(crate) fn field_label(ui: &mut Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).size(t.typography.small).family(fonts::demi()).color(t.colors.muted));
}

/// Côté d'où part une infobulle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Side {
    /// Au-dessus de l'élément (en dessous si la place manque).
    Above,
    /// À sa droite (rail des serveurs).
    Right,
}

/// Infobulle à flèche, immédiate, au-dessus de `resp` quand il est survolé.
pub(crate) fn tip(resp: Response, t: &Tokens, text: &str) -> Response {
    tip_at(resp, t, text, Side::Above)
}

/// Écart entre l'élément et la pointe de la flèche, et hauteur de la flèche.
const TIP_GAP: f32 = 4.0;
const TIP_ARROW: f32 = 5.0;
/// Largeur maximale du texte d'une infobulle.
const TIP_WIDTH: f32 = 190.0;

pub(crate) fn tip_at(resp: Response, t: &Tokens, text: &str, side: Side) -> Response {
    if text.is_empty() || !resp.hovered() || resp.ctx.input(|i| i.pointer.any_down()) {
        return resp;
    }
    let ctx = &resp.ctx;
    let screen = ctx.content_rect();
    let font = fonts::medium_font(t.typography.button);
    let galley = ctx.fonts_mut(|f| f.layout(text.to_owned(), font, t.colors.text, TIP_WIDTH));
    let pad = vec2(t.spacing.control_x.min(12.0), t.spacing.item_y * 2.0);
    let size = galley.size() + 2.0 * pad;
    let target = resp.rect;
    let reach = TIP_GAP + TIP_ARROW;
    // Rectangle de la bulle et pointe de la flèche ; `dir` : de la bulle vers l'élément.
    let (rect, tip, dir) = match side {
        Side::Right => {
            let rect = Rect::from_min_size(pos2(target.right() + reach, target.center().y - size.y / 2.0), size);
            (rect, pos2(target.right() + TIP_GAP, target.center().y), vec2(-1.0, 0.0))
        }
        Side::Above => {
            let x = (target.center().x - size.x / 2.0).clamp(screen.left() + 4.0, (screen.right() - 4.0 - size.x).max(screen.left() + 4.0));
            if target.top() - reach - size.y >= screen.top() {
                (Rect::from_min_size(pos2(x, target.top() - reach - size.y), size), pos2(target.center().x, target.top() - TIP_GAP), vec2(0.0, 1.0))
            } else {
                (Rect::from_min_size(pos2(x, target.bottom() + reach), size), pos2(target.center().x, target.bottom() + TIP_GAP), vec2(0.0, -1.0))
            }
        }
    };
    let layer = egui::LayerId::new(egui::Order::Tooltip, Id::unique("ferrocord-infobulle"));
    // Une clé par élément visé : passer d'un bouton au voisin rejoue l'apparition.
    motion::pop(ctx, layer, Id::unique(("infobulle", resp.id)), tip, 0, motion::dur::POP);
    let p = ctx.layer_painter(layer);
    let r = t.r_embed();
    p.add(t.shadow(Elevation::Menu).as_shape(rect, r));
    p.rect_filled(rect, r, t.colors.menu_bg);
    p.rect_stroke(rect, r, t.border(), egui::StrokeKind::Inside);
    // Flèche : triangle dont la base est sur le bord de la bulle.
    let across = vec2(dir.y, dir.x) * TIP_ARROW;
    let base = tip - dir * TIP_ARROW;
    p.add(Shape::convex_polygon(vec![base - across, base + across, tip], t.colors.menu_bg, Stroke::NONE));
    p.galley(rect.min + pad, galley, t.colors.text);
    resp
}

/// Ce que rend [`modal`].
pub(crate) struct ModalOut<R> {
    /// Ce que le contenu a rendu.
    #[allow(dead_code)]
    pub inner: R,
    /// Croix, Échap ou clic hors de la fenêtre.
    pub closed: bool,
    /// Clic sur le voile (une fenêtre qui garde un brouillon peut alors rester ouverte).
    pub veil_clicked: bool,
}

/// Fenêtre modale au gabarit du client : titre et croix en haut, contenu,
/// puis pied en bande plus sombre où les boutons sont alignés à droite, le
/// principal au bout (`footer` les pose de droite à gauche : le principal
/// d'abord, « Annuler » ensuite). `width` : largeur du contenu.
#[allow(clippy::too_many_arguments)]
pub(crate) fn modal<R>(ctx: &egui::Context, t: &Tokens, id: Id, width: f32, title: &str, close_tip: &str, body: impl FnOnce(&mut Ui) -> R, footer: impl FnOnce(&mut Ui)) -> ModalOut<R> {
    modal_impl(ctx, t, id, width, title, close_tip, body, Some(footer))
}

/// Fenêtre du même gabarit sans pied : titre et croix, contenu qui défile quand
/// la fenêtre est basse. Pour un contenu qui porte ses propres actions (assistant
/// à plusieurs pages, liste à choisir).
pub(crate) fn sheet<R>(ctx: &egui::Context, t: &Tokens, id: Id, width: f32, title: &str, close_tip: &str, body: impl FnOnce(&mut Ui) -> R) -> ModalOut<R> {
    modal_impl(ctx, t, id, width, title, close_tip, body, None::<fn(&mut Ui)>)
}

#[allow(clippy::too_many_arguments)]
fn modal_impl<R, F: FnOnce(&mut Ui)>(ctx: &egui::Context, t: &Tokens, id: Id, width: f32, title: &str, close_tip: &str, body: impl FnOnce(&mut Ui) -> R, footer: Option<F>) -> ModalOut<R> {
    let pad = t.spacing.card_pad;
    let r = t.r_card();
    let frame = egui::Frame::new().fill(t.colors.bg2).stroke(t.border()).corner_radius(r).shadow(t.shadow(Elevation::Window));
    // Ce que la fenêtre laisse au contenu : sa hauteur, moins le titre, le pied et de l'air autour.
    let foot = if footer.is_some() { 4.0 * t.spacing.item_x + button_height(t) } else { 0.0 };
    let chrome = pad / 2.0 + close_side(t) + t.spacing.item_x + pad + foot + 2.0 * pad;
    let room = (ctx.content_rect().height() - chrome).max(4.0 * button_height(t));
    let mut cross = false;
    let shown = egui::Modal::new(id).frame(frame).show(ctx, |ui| {
        ui.set_width(width + 2.0 * pad);
        ui.spacing_mut().item_spacing.y = 0.0;
        let head = Margin { left: px(pad), right: px(pad / 2.0), top: px(pad / 2.0), bottom: px(t.spacing.item_x) };
        egui::Frame::new().inner_margin(head).show(ui, |ui| {
            ui.horizontal(|ui| {
                let room = ui.available_width() - close_side(t) - t.spacing.item_x;
                ui.allocate_ui_with_layout(vec2(room, 0.0), Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(title).family(fonts::demi()).size(t.typography.title * 1.25).color(t.colors.text_strong)).wrap());
                });
                ui.with_layout(Layout::right_to_left(Align::Min), |ui| cross = close_button(ui, t, close_tip).clicked());
            });
        });
        let inner = egui::Frame::new()
            .inner_margin(Margin { left: px(pad), right: px(pad), top: 0, bottom: px(pad) })
            .show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = t.spacing.item_y;
                // Fenêtre basse : le contenu défile, le titre et le pied restent à l'écran.
                // Seulement quand il dépasse (hauteur de l'image d'avant) : une zone de
                // défilement autour d'un contenu qui tient déplace la fenêtre d'une image à l'autre.
                let tall_id = id.with("haut");
                let tall = ui.data(|d| d.get_temp::<f32>(tall_id)).is_some_and(|h| h > room);
                if tall {
                    egui::ScrollArea::vertical()
                        .id_salt(id)
                        .max_height(room)
                        .show(ui, |ui| {
                            ui.set_width(width);
                            let top = ui.cursor().top();
                            let inner = body(ui);
                            let h = ui.min_rect().bottom() - top;
                            ui.data_mut(|d| d.insert_temp(tall_id, h));
                            inner
                        })
                        .inner
                } else {
                    let top = ui.cursor().top();
                    let inner = body(ui);
                    let h = ui.min_rect().bottom() - top;
                    ui.data_mut(|d| d.insert_temp(tall_id, h));
                    inner
                }
            })
            .inner;
        // Pied : bande plus sombre sur toute la largeur, coins du bas arrondis avec la fenêtre.
        if let Some(footer) = footer {
            egui::Frame::new().fill(t.colors.bg1).corner_radius(CornerRadius { nw: 0, ne: 0, sw: r.sw, se: r.se }).inner_margin(Margin::symmetric(px(pad), px(t.spacing.item_x * 2.0))).show(ui, |ui| {
            // Mêmes marges de côté que le contenu : le bouton principal finit au bord droit des champs.
            let wide = width;
            ui.set_width(wide);
            ui.spacing_mut().item_spacing.y = t.spacing.item_y;
            // Hauteur donnée : centrés dans toute la place restante, les boutons étiraient le pied.
            ui.allocate_ui_with_layout(vec2(wide, button_height(t)), Layout::right_to_left(Align::Center), footer);
            });
        }
        inner
    });
    ModalOut { closed: cross || shown.should_close(), veil_clicked: shown.backdrop_response.clicked(), inner: shown.inner }
}

fn px(v: f32) -> i8 {
    v.round().clamp(0.0, f32::from(i8::MAX)) as i8
}

pub(crate) fn close_side(t: &Tokens) -> f32 {
    t.spacing.control_height.max(28.0)
}

/// Croix de fermeture d'une fenêtre (coin haut droit).
pub(crate) fn close_button(ui: &mut Ui, t: &Tokens, tip_text: &str) -> Response {
    close_button_inked(ui, t, tip_text, (t.colors.muted, t.colors.text_strong), t.colors.row_hover)
}

/// Croix posée sur une image (bannière d'un profil), sur le disque sombre
/// [`Tokens::media_veil`] : traits clairs dans tous les thèmes.
pub(crate) fn close_button_on_media(ui: &mut Ui, t: &Tokens, tip_text: &str) -> Response {
    close_button_inked(ui, t, tip_text, (t.on_media_dim(), t.on_media()), Color32::TRANSPARENT)
}

fn close_button_inked(ui: &mut Ui, t: &Tokens, tip_text: &str, ink: (Color32, Color32), hover: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(close_side(t)), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let k = motion::hover(ui.ctx(), resp.id, hot);
    if k > 0.0 && hover != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, t.r_row(), hover.gamma_multiply(k));
    }
    icons::paint(Icon::Close, ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(16.0_f32.min(rect.width()))), motion::mix(ink.0, ink.1, k));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip_text));
    tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip_text)
}

/// Onglet souligné (profil complet) : texte en graisse moyenne, trait de 2
/// sous l'onglet actif, fondu au survol. À poser dans une rangée horizontale.
pub(crate) fn tab(ui: &mut Ui, t: &Tokens, text: &str, active: bool) -> Response {
    let ink = if active { t.colors.text_strong } else { t.colors.muted };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), fonts::medium_font(t.typography.button), ink);
    let line = 2.0 * t.shapes.border.max(1.0);
    let size = vec2(galley.size().x, button_height(t) + line);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, active, text));
    if ui.is_rect_visible(rect) {
        let k = if active { 1.0 } else { motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus()) * 0.5 };
        let p = ui.painter();
        p.galley(pos2(rect.left(), rect.top() + (button_height(t) - galley.size().y) / 2.0), galley, motion::mix(ink, t.colors.text_strong, k));
        if k > 0.0 {
            let under = Rect::from_min_max(pos2(rect.left(), rect.bottom() - line), rect.max);
            p.rect_filled(under, line / 2.0, t.colors.text_strong.gamma_multiply(k));
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Ancre d'un texte centré dans un rectangle (raccourci des pastilles et étiquettes).
pub(crate) fn centered(p: &egui::Painter, rect: Rect, text: &str, font: egui::FontId, color: Color32) {
    p.text(rect.center(), Align2::CENTER_CENTER, text, font, color);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, Preset, MIN_TEXT};

    fn pass(ctx: &egui::Context, f: impl FnMut(&mut Ui)) {
        let mut out = ctx.run_ui(egui::RawInput::default(), f);
        out.textures_delta.clear();
    }

    /// Chaque bouton plein porte un texte lisible, au repos comme survolé, dans tous les préréglages.
    #[test]
    fn button_text_is_readable_on_every_fill() {
        for p in Preset::ALL {
            let t = p.tokens();
            for kind in [Kind::Primary, Kind::Secondary, Kind::Danger, Kind::Success] {
                let (fill, hover, ink) = colors(&t, kind);
                assert!(contrast_ratio(ink, fill) >= MIN_TEXT, "{} {kind:?} : {:.2}", p.key(), contrast_ratio(ink, fill));
                assert!(contrast_ratio(ink, hover) >= MIN_TEXT, "{} {kind:?} survolé : {:.2}", p.key(), contrast_ratio(ink, hover));
            }
        }
    }

    /// Un bouton désactivé reste lisible (3:1) dans tous les préréglages : en
    /// thème clair, « Envoyer » désactivé était blanc sur lavande pâle.
    #[test]
    fn disabled_button_text_is_readable_in_every_preset() {
        let mut fails = Vec::new();
        for p in Preset::ALL {
            let t = p.tokens();
            for kind in [Kind::Primary, Kind::Secondary, Kind::Danger, Kind::Success, Kind::Link] {
                let (fill, ink) = disabled(&t, kind);
                let under = if kind == Kind::Link { t.colors.bg2 } else { fill };
                let r = contrast_ratio(ink, under);
                if r < crate::theme::MIN_MUTED {
                    fails.push(format!("{} {kind:?} désactivé : {r:.2}", p.key()));
                }
                // Désactivé se voit : l'aplat n'est plus celui du bouton actif.
                if kind != Kind::Link && fill == colors(&t, kind).0 {
                    fails.push(format!("{} {kind:?} : aplat désactivé identique", p.key()));
                }
            }
        }
        assert!(fails.is_empty(), "{fails:#?}");
    }

    /// Ce qui se pose sur une image, une vidéo ou une tuile d'appel (nom,
    /// durée, bouton de lecture) se lit dans tous les préréglages, que la
    /// pastille soit sur du blanc, du noir ou le fond des tuiles.
    #[test]
    fn media_labels_are_readable_in_every_preset() {
        let over = |fg: Color32, bg: Color32| {
            let a = f32::from(fg.a()) / 255.0;
            let [r, g, b, _] = fg.to_srgba_unmultiplied();
            let mix = |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
            Color32::from_rgb(mix(r, bg.r()), mix(g, bg.g()), mix(b, bg.b()))
        };
        let mut fails = Vec::new();
        for p in Preset::ALL {
            let t = p.tokens();
            for (name, bg) in [("blanc", Color32::WHITE), ("noir", Color32::BLACK), ("bg3", t.colors.bg3), ("bg2", t.colors.bg2)] {
                let pill = over(t.media_veil(), bg);
                for (what, ink, min) in [("on_media", t.on_media(), MIN_TEXT), ("on_media_dim", t.on_media_dim(), crate::theme::MIN_MUTED), ("on_media_danger", t.on_media_danger(), crate::theme::MIN_MUTED)] {
                    let r = contrast_ratio(ink, pill);
                    if r < min {
                        fails.push(format!("{} {what} sur {name} : {r:.2} < {min}", p.key()));
                    }
                }
            }
        }
        assert!(fails.is_empty(), "{fails:#?}");
    }

    /// Une case ou un bouton radio non cochés se détachent de la page dans tous
    /// les préréglages (dans Onyx, ils se fondaient dans le noir).
    #[test]
    fn unchecked_boxes_stand_out_in_every_preset() {
        for p in Preset::ALL {
            let t = p.tokens();
            let fill = t.visuals().widgets.inactive.bg_fill;
            for (name, bg) in [("bg2", t.colors.bg2), ("bg1", t.colors.bg1)] {
                let r = contrast_ratio(fill, bg);
                assert!(r >= crate::theme::BOX_CONTRAST - 0.25, "{} case sur {name} : {r:.2}", p.key());
            }
        }
    }

    /// Hauteurs du client dans les thèmes calqués : bouton de 32, champ de 40.
    #[test]
    fn sizes_follow_the_client() {
        let ctx = egui::Context::default();
        fonts::install_quiet(&ctx);
        let t = Preset::Sombre.tokens().scaled();
        t.apply(&ctx);
        let (mut b, mut f, mut wide) = (Vec2::ZERO, Vec2::ZERO, 0.0);
        for _ in 0..2 {
            pass(&ctx, |ui| {
                b = button(ui, &t, Kind::Primary, "Enregistrer").rect.size();
                wide = button_wide(ui, &t, Kind::Secondary, "Ok", 200.0).rect.width();
                ui.allocate_ui(vec2(300.0, 100.0), |ui| f = field(ui, &t, &mut String::new(), "Nom").rect.size());
            });
        }
        assert_eq!(b.y, 32.0);
        assert!(b.x > 2.0 * t.spacing.control_x);
        assert_eq!(wide, 200.0);
        assert_eq!(f, vec2(300.0, 40.0));
        // Rouille garde sa hauteur d'avant : le texte et ses marges.
        assert_eq!(button_height(&Preset::Rouille.tokens()), 31.0);
    }

    /// La fenêtre modale rend la valeur du contenu ; ni croix ni Échap : elle reste ouverte.
    #[test]
    fn modal_returns_its_body_and_stays_open() {
        let ctx = egui::Context::default();
        fonts::install_quiet(&ctx);
        let t = Preset::Sombre.tokens().scaled();
        t.apply(&ctx);
        let mut seen = None;
        for _ in 0..2 {
            pass(&ctx, |ui| {
                let out = modal(
                    ui.ctx(),
                    &t,
                    Id::unique("essai"),
                    400.0,
                    "Titre",
                    "Fermer",
                    |ui| {
                        ui.label("corps");
                        7
                    },
                    |ui| {
                        button(ui, &t, Kind::Primary, "Oui");
                        button(ui, &t, Kind::Link, "Annuler");
                    },
                );
                seen = Some((out.inner, out.closed));
            });
        }
        assert_eq!(seen, Some((7, false)));
    }
}
