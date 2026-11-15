//! Gabarit plein écran des fenêtres de réglages (utilisateur, serveur,
//! salon) : colonne des catégories à gauche, alignée contre la page ; page
//! centrée, de largeur bornée ; bouton rond de fermeture à sa droite
//! (`docs/design.md`, « Paramètres »). Les composants des pages (ligne de
//! réglage, interrupteur, section) sont ici aussi, pour que toutes les pages
//! se ressemblent.

use crate::i18n::{table::s::t as st, Lang};
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Align, Color32, Layout, Rect, RichText, Sense, Stroke};

/// Largeur de la colonne des catégories.
pub(in crate::ui) const NAV_WIDTH: f32 = 218.0;
/// Colonne des paramètres utilisateur : plus large (une icône devant chaque
/// entrée, des sous-entrées indentées).
pub(in crate::ui) const USER_NAV_WIDTH: f32 = 264.0;
/// Largeur maximale d'une page.
pub(in crate::ui) const PAGE_WIDTH: f32 = 740.0;
/// Marge autour de la page, et au-dessus des deux colonnes.
const GUTTER: f32 = 40.0;
const TOP: f32 = 56.0;
/// Côté du bouton de fermeture.
const CLOSE: f32 = 36.0;
/// Interrupteur : largeur, hauteur.
const SWITCH: egui::Vec2 = egui::vec2(40.0, 24.0);

/// Zones du gabarit, dans la fenêtre.
pub(in crate::ui) struct Shell {
    /// Fond sombre de gauche, jusqu'au bord de la fenêtre.
    pub left: Rect,
    pub nav: Rect,
    pub page: Rect,
    pub close: Rect,
}

/// Assez de place pour deux colonnes (sinon : la liste des pages ou la page).
pub(in crate::ui) fn wide(ctx: &egui::Context) -> bool {
    wide_for(ctx, NAV_WIDTH)
}

/// [`wide`], pour une colonne de largeur `nav`.
pub(in crate::ui) fn wide_for(ctx: &egui::Context, nav: f32) -> bool {
    !crate::ui::mobile::is_active(ctx) && ctx.content_rect().width() >= NAV_WIDTH + nav + 2.0 * GUTTER + CLOSE
}

/// Découpe `full` : la colonne et la page sont centrées ensemble.
pub(in crate::ui) fn layout(full: Rect) -> Shell {
    layout_for(full, NAV_WIDTH)
}

/// [`layout`], pour une colonne de largeur `nav_width`.
pub(in crate::ui) fn layout_for(full: Rect, nav_width: f32) -> Shell {
    let close_col = CLOSE + GUTTER / 2.0;
    let page_w = (full.width() - nav_width - 2.0 * GUTTER - close_col).clamp(1.0, PAGE_WIDTH);
    let side = ((full.width() - nav_width - 2.0 * GUTTER - page_w - close_col) / 2.0).max(0.0).floor();
    let nav_left = full.left() + side + GUTTER / 2.0;
    let nav_right = full.left() + side + nav_width + GUTTER / 2.0;
    let left = Rect::from_min_max(full.min, egui::pos2(nav_right + GUTTER / 4.0, full.bottom()));
    let nav = Rect::from_min_max(egui::pos2(nav_left, full.top() + TOP), egui::pos2(nav_right, full.bottom() - GUTTER / 2.0));
    let page_left = left.right() + GUTTER;
    let page = Rect::from_min_max(egui::pos2(page_left, full.top() + TOP), egui::pos2(page_left + page_w, full.bottom()));
    let close = Rect::from_min_size(egui::pos2(page.right() + GUTTER / 2.0, full.top() + TOP), egui::Vec2::splat(CLOSE));
    Shell { left, nav, page, close }
}

/// Fonds du gabarit : la page est sur le fond principal, la colonne sur celui des listes.
pub(in crate::ui) fn backdrop(ui: &egui::Ui, t: &Tokens, full: Rect, s: &Shell) {
    ui.painter().rect_filled(full, 0.0, t.colors.bg2);
    ui.painter().rect_filled(s.left, 0.0, t.colors.bg1);
}

/// Bouton rond « X », le nom de la touche dessous ; `true` s'il est cliqué.
pub(in crate::ui) fn close_button(ui: &mut egui::Ui, t: &Tokens, lang: Lang, rect: Rect) -> bool {
    let tip = crate::i18n::t::CLOSE_TIP.get(lang);
    let resp = ui.interact(rect, ui.scope_id().with("fermer_reglages"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    let hot = resp.hovered() || resp.has_focus();
    let color = if hot { t.colors.text_strong } else { t.colors.muted };
    let p = ui.painter();
    if hot {
        p.circle_filled(rect.center(), rect.width() / 2.0, t.colors.row_hover);
    }
    p.circle_stroke(rect.center(), rect.width() / 2.0 - 1.0, Stroke::new(2.0, color));
    icons::paint(Icon::Close, p, rect.shrink(rect.width() * 0.3), color);
    p.text(egui::pos2(rect.center().x, rect.bottom() + t.spacing.item_y), egui::Align2::CENTER_TOP, st::ESC_KEY.get(lang), crate::ui::fonts::demi_font(t.typography.small), t.colors.muted);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    crate::ui::keys::focus_ring(ui, t, &resp);
    resp.on_hover_text(tip).clicked()
}

/// Style des pages en deux colonnes : une ligne a d'emblée la hauteur d'un
/// bouton, donc un libellé posé devant un bouton est centré sur lui.
pub(in crate::ui) fn page_style(ui: &mut egui::Ui, t: &Tokens) {
    ui.spacing_mut().interact_size.y = t.head_row();
}

/// En-tête d'une page. Deux colonnes : le titre, en grand (la fermeture est
/// le bouton rond). Une colonne : titre, « X » et filet ; `true` si « X » est cliqué.
pub(in crate::ui) fn header(ui: &mut egui::Ui, t: &Tokens, lang: Lang, title: &str) -> bool {
    if wide(ui.ctx()) {
        ui.add(egui::Label::new(RichText::new(title).family(crate::ui::fonts::demi()).size(t.typography.title * 1.25).color(t.colors.text_strong)).truncate());
        ui.add_space(t.spacing.category_gap);
        return false;
    }
    let mut close = false;
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            close = crate::ui::util::icon_button(ui, t, Icon::Close, crate::i18n::t::CLOSE_TIP.get(lang)).clicked();
        });
    });
    ui.separator();
    close
}

/// Filet entre deux groupes de la colonne des catégories.
pub(in crate::ui) fn nav_divider(ui: &mut egui::Ui, t: &Tokens) {
    ui.add_space(t.spacing.item_x);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.shapes.border.max(1.0)), Sense::hover());
    ui.painter().rect_filled(rect.shrink2(egui::vec2(t.spacing.item_x, 0.0)), 0.0, t.colors.border);
    ui.add_space(t.spacing.item_y);
}

/// Titre de groupe en casse normale, à la façon des paramètres de Discord
/// (utilisateur, serveur et salon).
pub(in crate::ui) fn nav_title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(t.spacing.item_x + t.spacing.item_y);
        ui.add(egui::Label::new(RichText::new(text).size(t.typography.small).family(crate::ui::fonts::demi()).color(t.colors.muted)).truncate().selectable(false));
    });
    ui.add_space(t.spacing.item_y);
}

/// Repères des sections de la page affichée : la colonne des paramètres
/// utilisateur y lit la section courante (sous-entrée marquée) et y dépose
/// celle à amener en haut de la page.
#[derive(Clone, Default)]
struct Anchors {
    /// Une page est en cours de dessin entre [`anchors_begin`] et [`anchors_end`].
    active: bool,
    goto: Option<egui::Id>,
    /// Sections dessinées, dans l'ordre : identifiant et haut à l'écran.
    seen: Vec<(egui::Id, f32)>,
}

fn anchors_id() -> egui::Id {
    egui::Id::unique("ferrocord-reperes-de-sections")
}

/// Identifiant d'une section d'après son titre affiché.
pub(in crate::ui) fn anchor_of(title: &str) -> egui::Id {
    egui::Id::unique(("section-de-reglages", title))
}

/// Repère du haut de la page (aucune section ne le porte).
pub(in crate::ui) fn anchor_top() -> egui::Id {
    egui::Id::unique("ferrocord-haut-de-page")
}

/// Début du dessin d'une page ; `goto` : section à amener en haut.
pub(in crate::ui) fn anchors_begin(ctx: &egui::Context, goto: Option<egui::Id>) {
    ctx.data_mut(|d| {
        let a = d.get_temp_mut_or_default::<Anchors>(anchors_id());
        a.active = true;
        a.goto = goto;
        a.seen.clear();
    });
}

/// Fin du dessin d'une page : les sections qu'elle a dessinées.
pub(in crate::ui) fn anchors_end(ctx: &egui::Context) -> Vec<(egui::Id, f32)> {
    ctx.data_mut(|d| {
        let a = d.get_temp_mut_or_default::<Anchors>(anchors_id());
        a.active = false;
        a.goto = None;
        std::mem::take(&mut a.seen)
    })
}

/// Titre de section d'une page, sous un filet.
pub(in crate::ui) fn section(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    // Première chose de la page : pas de filet (il restait nu sous le titre de la page).
    let first = ui.min_rect().height() < 1.0;
    let rect = if first {
        egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), 0.0))
    } else {
        ui.add_space(t.spacing.category_gap);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.shapes.border.max(1.0)), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, t.colors.border);
        ui.add_space(t.spacing.category_gap);
        rect
    };
    let title = ui.add(egui::Label::new(RichText::new(text).family(crate::ui::fonts::demi()).size(t.typography.title).color(t.colors.text_strong)).wrap());
    ui.add_space(t.spacing.item_y);
    let key = anchor_of(text);
    let goto = ui.data_mut(|d| {
        let a = d.get_temp_mut_or_default::<Anchors>(anchors_id());
        if !a.active {
            return false;
        }
        a.seen.push((key, rect.top()));
        a.goto.take_if(|g| *g == key).is_some()
    });
    if goto {
        ui.scroll_to_rect(rect.union(title.rect), Some(Align::TOP));
    }
}

/// Couleurs de l'interrupteur, posées dans le contexte par le thème : les
/// lignes de réglage sont dessinées par des fonctions qui n'ont pas les jetons.
#[derive(Clone, Copy)]
struct SwitchColors {
    on: Color32,
    off: Color32,
    knob: Color32,
}

fn switch_id() -> egui::Id {
    egui::Id::unique("ferrocord-interrupteur")
}

/// À appeler quand le thème change (et au premier passage).
pub(in crate::ui) fn remember(ctx: &egui::Context, t: &Tokens) {
    let c = SwitchColors { on: t.colors.online, off: t.colors.offline, knob: Color32::WHITE };
    ctx.data_mut(|d| d.insert_temp(switch_id(), c));
}

/// Interrupteur en pilule, vert quand il est activé.
pub(in crate::ui) fn switch(ui: &mut egui::Ui, on: &mut bool) -> egui::Response {
    let (rect, mut resp) = ui.allocate_exact_size(SWITCH, Sense::click());
    if resp.clicked() {
        *on = !*on;
        resp.mark_changed();
    }
    let checked = *on;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, ui.is_enabled(), checked, ""));
    if ui.is_rect_visible(rect) {
        let v = ui.visuals();
        let c = ui.ctx().data(|d| d.get_temp::<SwitchColors>(switch_id())).unwrap_or(SwitchColors { on: v.selection.bg_fill, off: v.widgets.inactive.bg_fill, knob: Color32::WHITE });
        let how = crate::ui::motion::toward(ui.ctx(), resp.id, checked, crate::ui::motion::dur::MORPH);
        let fade = if ui.is_enabled() { 1.0 } else { 0.45 };
        let mix = |a: Color32, b: Color32| Color32::from_rgb(egui::lerp(f32::from(a.r())..=f32::from(b.r()), how) as u8, egui::lerp(f32::from(a.g())..=f32::from(b.g()), how) as u8, egui::lerp(f32::from(a.b())..=f32::from(b.b()), how) as u8);
        let r = rect.height() / 2.0;
        ui.painter().rect_filled(rect, r, mix(c.off, c.on).gamma_multiply(fade));
        let x = egui::lerp((rect.left() + r)..=(rect.right() - r), how);
        ui.painter().circle_filled(egui::pos2(x, rect.center().y), r - 3.0, c.knob.gamma_multiply(fade));
        // Dans le bouton : coche (activé) ou croix (désactivé), de la couleur du fond (lot V1).
        let (k, o, ink) = (r - 3.0, egui::pos2(x, rect.center().y), Stroke::new(1.6, mix(c.off, c.on).gamma_multiply(fade)));
        if how > 0.5 {
            ui.painter().line(vec![o + egui::vec2(-0.42 * k, 0.02 * k), o + egui::vec2(-0.12 * k, 0.32 * k), o + egui::vec2(0.42 * k, -0.28 * k)], ink);
        } else {
            let d = 0.3 * k;
            ui.painter().line_segment([o + egui::vec2(-d, -d), o + egui::vec2(d, d)], ink);
            ui.painter().line_segment([o + egui::vec2(-d, d), o + egui::vec2(d, -d)], ink);
        }
        if resp.has_focus() {
            ui.painter().rect_stroke(rect.expand(2.0), r + 2.0, Stroke::new(2.0, v.selection.stroke.color), egui::StrokeKind::Outside);
        }
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Interrupteur suivi de son libellé, là où une page posait une case à cocher.
pub(in crate::ui) fn switch_label(ui: &mut egui::Ui, on: &mut bool, text: impl Into<egui::WidgetText>) -> egui::Response {
    let text = text.into();
    let name = text.text().to_string();
    let inner = ui.horizontal(|ui| {
        let mut resp = switch(ui, on);
        let label = ui.add(egui::Label::new(text).wrap().selectable(false).sense(Sense::click()));
        if label.clicked() {
            *on = !*on;
            resp.mark_changed();
        }
        resp
    });
    let checked = *on;
    inner.inner.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, checked, &name));
    inner.inner
}

/// Ligne de réglage : libellé et description grise à gauche, réglage à
/// droite. `end` : le réglage est collé au bord droit (interrupteur, bouton).
///
/// La ligne occupe exactement la largeur disponible : son contenu ne peut
/// pas élargir la page.
pub(in crate::ui) fn settings_row(ui: &mut egui::Ui, label: &str, detail: &str, end: bool, add: impl FnOnce(&mut egui::Ui)) {
    let pad = ui.spacing().item_spacing.y + 2.0;
    let gap = 2.0 * ui.spacing().item_spacing.x;
    let full = ui.available_width();
    let control_w = if end { (full * 0.4).min(280.0) } else { (full * 0.45).min(320.0) };
    // Réglage plus large que sa colonne (curseur, valeur et bouton dans une page
    // étroite) : la colonne prend la largeur mesurée à l'image précédente, au lieu
    // de couper le dernier contrôle (« Ess »). Le libellé garde au moins un tiers.
    let need_id = ui.scope_id().with(("largeur-de-reglage", label));
    let need = ui.data(|d| d.get_temp::<(f32, f32)>(need_id)).filter(|(w, _)| (*w - full).abs() < 0.5).map_or(0.0, |(_, n)| n);
    let control_w = control_w.max(need).min((full * 0.67).max(control_w));
    let text_w = (full - control_w - gap).max(1.0);
    let control_h = ui.spacing().interact_size.y.max(ui.text_style_height(&egui::TextStyle::Button).max(ui.text_style_height(&egui::TextStyle::Body)) + 2.0 * ui.spacing().button_padding.y);
    let name = egui::WidgetText::from(RichText::new(label).color(ui.visuals().strong_text_color())).into_galley(ui, Some(egui::TextWrapMode::Wrap), text_w, egui::TextStyle::Body);
    let shown = !detail.is_empty() && detail != label;
    let more = shown.then(|| egui::WidgetText::from(RichText::new(detail).small().color(ui.visuals().weak_text_color())).into_galley(ui, Some(egui::TextWrapMode::Wrap), text_w, egui::TextStyle::Small));
    let text_h = name.size().y + more.as_ref().map_or(0.0, |m| m.size().y + 2.0);
    let height = control_h.max(text_h) + 2.0 * pad;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(full, height), Sense::hover());
    if ui.is_rect_visible(rect) {
        let top = rect.top() + (height - text_h) / 2.0;
        let at = egui::pos2(rect.left(), top);
        let below = at + egui::vec2(0.0, name.size().y + 2.0);
        ui.painter().galley(at, name, ui.visuals().strong_text_color());
        if let Some(m) = more {
            ui.painter().galley(below, m, ui.visuals().weak_text_color());
        }
    }
    let rest = Rect::from_min_max(egui::pos2(rect.right() - control_w, rect.top()), rect.max);
    let layout = if end { Layout::right_to_left(Align::Center) } else { Layout::left_to_right(Align::Center) };
    // Un réglage posé de gauche à droite (curseur et valeur, pastille de
    // couleur, bouton et texte) et plus étroit que sa colonne est décalé de
    // la place qu'il a laissée à l'image précédente : tous les réglages
    // d'une page finissent ainsi sur le même bord droit. La largeur offerte
    // ne change pas, donc un réglage qui la remplit ne bouge pas.
    let slack_id = ui.scope_id().with(("ligne-de-reglage", label));
    let slack = if end { 0.0 } else { ui.data(|d| d.get_temp::<f32>(slack_id)).unwrap_or(0.0).clamp(0.0, control_w) };
    let mut value = ui.new_child(egui::UiBuilder::new().max_rect(rest.translate(egui::vec2(slack, 0.0))).layout(layout));
    // Curseur et case de valeur sur la même ligne médiane.
    value.spacing_mut().interact_size.y = control_h;
    value.set_clip_rect(rest.with_max_y(ui.clip_rect().bottom()).intersect(ui.clip_rect()));
    add(&mut value);
    let used = value.min_rect().width().ceil();
    if used > control_w + 0.5 && used > need + 0.5 {
        ui.data_mut(|d| d.insert_temp(need_id, (full, used)));
        ui.ctx().request_discard("largeur d'un réglage");
    }
    if !end {
        let left = (rest.right() - value.min_rect().right() + slack).clamp(0.0, control_w);
        let left = (left * ui.pixels_per_point()).floor() / ui.pixels_per_point();
        if (left - slack).abs() > 0.01 {
            ui.data_mut(|d| d.insert_temp(slack_id, left));
            // Première image de la page : refaite avant d'être montrée.
            ui.ctx().request_discard("alignement d'un réglage");
        }
    }
    // Une palette peut passer à la ligne : la place en plus est réservée.
    let extra = value.min_rect().bottom() - rect.bottom();
    if extra > 0.0 {
        ui.allocate_space(egui::vec2(0.0, extra));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La colonne, la page et le bouton tiennent dans la fenêtre sans se
    /// chevaucher, et la page ne dépasse jamais sa largeur maximale.
    #[test]
    fn zones_fit_and_do_not_overlap() {
        for w in [520.0, 800.0, 1280.0, 1920.0, 3840.0] {
            let full = Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, 800.0));
            let s = layout(full);
            assert!(s.nav.left() >= full.left() && s.close.right() <= full.right() + 0.5, "largeur {w} : {:?} {:?}", s.nav, s.close);
            assert!(s.nav.right() <= s.left.right() && s.left.right() < s.page.left() && s.page.right() < s.close.left(), "largeur {w}");
            assert!(s.page.width() <= PAGE_WIDTH && s.page.width() >= 1.0, "largeur {w}");
            assert_eq!(s.nav.width(), NAV_WIDTH);
            // Colonne plus large des paramètres utilisateur : mêmes garanties.
            let s = layout_for(full, USER_NAV_WIDTH);
            assert!(s.nav.left() >= full.left() && s.nav.right() <= s.left.right() && s.left.right() < s.page.left() && s.page.right() < s.close.left(), "largeur {w}");
            assert_eq!(s.nav.width(), USER_NAV_WIDTH);
        }
    }
}
