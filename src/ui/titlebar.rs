//! Barre de titre intégrée (lot V1, `docs/ecarts-visuels.md` M1 et M2) : même
//! fond que le rail, nom du serveur ou « Messages privés » au centre avec son
//! icône, puis à droite la boîte de réception, l'aide, réduire, agrandir,
//! fermer.
//!
//! La fenêtre est créée sans décoration (`lib.rs`), et Windows continue de
//! faire le déplacement, le double-clic, le redimensionnement, le menu
//! système et l'ancrage : `platform::caption` lui dit quelle zone de la barre
//! est quoi. Ici : le dessin, les boutons, et la bande réservée en haut de la
//! fenêtre (zone sûre d'egui : panneaux, fenêtres et voiles restent dessous).
//!
//! Hors Windows, ou avec le jeton `mise_en_page.titlebar = "windows"`, la barre du
//! système reste en place et rien n'est dessiné.

use super::{fonts, util, widgets, App};
use crate::i18n::t as tx;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, pos2, vec2, Color32, Id, Rect, Sense, Stroke, Vec2};

#[cfg(windows)]
use crate::platform::caption;

/// Hors Windows : pas de barre intégrée (les zones ne sont lues par personne).
#[cfg(not(windows))]
#[allow(dead_code)]
mod caption {
    pub type Px = [i32; 4];
    #[derive(Default)]
    pub struct Regions {
        pub height: i32,
        pub resize: i32,
        pub maximize: Px,
        pub holes: Vec<Px>,
    }
    #[derive(Default)]
    pub struct MaxButton {
        pub hovered: bool,
        pub pressed: bool,
    }
    pub fn active() -> bool {
        false
    }
    pub fn installed() -> bool {
        false
    }
    pub fn set_active(_: bool) {}
    pub fn set_regions(_: Regions) {}
    pub fn state() -> MaxButton {
        MaxButton::default()
    }
}

/// Largeur d'un bouton de fenêtre pour une barre haute de 32 (ceux de Windows 11 : 46 × 32).
const BUTTON_RATIO: f32 = 46.0 / 32.0;
/// Voile des fenêtres modales : celui d'egui (`Modal::backdrop_color` par défaut).
const MODAL_VEIL: Color32 = Color32::from_black_alpha(100);
/// Bande du haut de la barre qui redimensionne la fenêtre.
const RESIZE: f32 = 5.0;

/// Double la procédure de la fenêtre principale (Windows) ; sans effet ailleurs.
pub(super) fn attach(cc: &eframe::CreationContext<'_>) {
    #[cfg(windows)]
    {
        use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = cc.window_handle() else { return };
        let RawWindowHandle::Win32(h) = handle.as_raw() else { return };
        let ctx = cc.egui_ctx.clone();
        caption::install(h.hwnd.get(), std::sync::Arc::new(move || ctx.request_repaint()));
        // Fond de fenêtre du système (lot E5) : la même fenêtre.
        crate::platform::backdrop::set_window(h.hwnd.get());
    }
    #[cfg(not(windows))]
    let _ = cc;
}

/// Hauteur de la barre dessinée à cette image, en points ; 0 : aucune
/// (barre de Windows, plein écran, autre système).
fn height(t: &Tokens, fullscreen: bool) -> f32 {
    if caption::active() && !fullscreen { t.layout.titlebar_height } else { 0.0 }
}

/// La barre intégrée est dessinée à cette image : elle porte la boîte de
/// réception, que l'en-tête de la discussion n'a donc pas à répéter.
pub(super) fn shown(ctx: &egui::Context) -> bool {
    caption::active() && ctx.content_rect().top() > ctx.viewport_rect().top()
}

/// Réserve la bande de la barre en haut de la fenêtre : `content_rect` d'egui
/// commence dessous. À appeler dans `raw_input_hook`.
pub(super) fn reserve(t: &Tokens, raw: &mut egui::RawInput) {
    if !caption::installed() {
        return;
    }
    let fullscreen = raw.viewports.get(&raw.viewport_id).and_then(|v| v.fullscreen) == Some(true);
    let top = height(t, fullscreen);
    raw.safe_area_insets = Some(egui::SafeAreaInsets(egui::epaint::MarginF32 { top, ..Default::default() }));
}

/// Rectangle en points → pixels de la zone cliente.
fn px(r: Rect, ppp: f32) -> caption::Px {
    [(r.left() * ppp).round() as i32, (r.top() * ppp).round() as i32, (r.right() * ppp).round() as i32, (r.bottom() * ppp).round() as i32]
}

/// Dessin d'un bouton de fenêtre.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Fond et trait d'un bouton de fenêtre ; la croix survolée passe en rouge.
fn button_colors(t: &Tokens, glyph: Glyph, hot: bool, pressed: bool) -> (Color32, Color32) {
    match (hot || pressed, glyph) {
        (false, _) => (Color32::TRANSPARENT, t.colors.muted),
        (true, Glyph::Close) => (t.danger_fill(), t.colors.on_accent),
        (true, _) => (if pressed { t.colors.selected } else { t.colors.row_hover }, t.colors.text_strong),
    }
}

fn paint_glyph(p: &egui::Painter, rect: Rect, glyph: Glyph, color: Color32) {
    // Dessin de 10 points au centre, calé sur les pixels comme ceux de Windows.
    let c = rect.center().round();
    let s = Stroke::new(1.0, color);
    let h = 5.0;
    match glyph {
        Glyph::Minimize => {
            p.line_segment([pos2(c.x - h, c.y + 0.5), pos2(c.x + h, c.y + 0.5)], s);
        }
        Glyph::Maximize => {
            p.rect_stroke(Rect::from_center_size(c, Vec2::splat(2.0 * h)), 1.5, s, egui::StrokeKind::Inside);
        }
        Glyph::Restore => {
            let front = Rect::from_min_size(pos2(c.x - h, c.y - h + 2.0), Vec2::splat(2.0 * h - 2.0));
            p.rect_stroke(front, 1.5, s, egui::StrokeKind::Inside);
            // Fenêtre de derrière : seuls son haut et sa droite dépassent.
            let (l, t, r, b) = (c.x - h + 2.5, c.y - h + 0.5, c.x + h - 0.5, c.y + h - 2.5);
            p.line_segment([pos2(l, t), pos2(r - 1.0, t)], s);
            p.line_segment([pos2(r, t + 1.0), pos2(r, b)], s);
        }
        Glyph::Close => {
            p.line_segment([pos2(c.x - h, c.y - h), pos2(c.x + h, c.y + h)], s);
            p.line_segment([pos2(c.x - h, c.y + h), pos2(c.x + h, c.y - h)], s);
        }
    }
}

/// Bouton de fenêtre ; `forced` : survol et appui vus par Windows (agrandir).
/// `veiled` : une fenêtre modale est ouverte. egui ne laisse alors rien passer
/// sous elle : la souris est lue directement, pour que réduire et fermer
/// répondent toujours. Rend vrai au clic.
fn window_button(ui: &mut egui::Ui, t: &Tokens, rect: Rect, glyph: Glyph, name: &str, forced: Option<(bool, bool)>, veiled: bool) -> bool {
    let resp = ui.interact(rect, Id::unique(("barre_de_titre", name)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let raw = veiled.then(|| ui.input(|i| {
        let over = i.pointer.latest_pos().is_some_and(|p| rect.contains(p));
        (over, over && i.pointer.primary_down(), over && i.pointer.primary_clicked())
    }));
    let (hot, pressed) = forced.or(raw.map(|(over, down, _)| (over, down))).unwrap_or((resp.hovered(), resp.is_pointer_button_down_on()));
    let (fill, ink) = button_colors(t, glyph, hot || resp.has_focus(), pressed);
    if fill != Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    paint_glyph(ui.painter(), rect, glyph, ink);
    resp.clicked() || raw.is_some_and(|(_, _, clicked)| clicked)
}

/// Bouton à icône de la barre (boîte, aide).
fn icon_button(ui: &mut egui::Ui, t: &Tokens, rect: Rect, icon: Icon, name: &str, on: bool) -> egui::Response {
    let resp = ui.interact(rect, Id::unique(("barre_de_titre", name)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    let hot = resp.hovered() || resp.has_focus();
    let side = (rect.height() * 0.625).round().min(t.typography.icon).max(1.0);
    icons::paint(icon, ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(side)), if hot || on { t.colors.text_strong } else { t.colors.muted });
    widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, name)
}

impl App {
    /// Le réglage du thème (barre intégrée ou barre de Windows) suit la fenêtre.
    fn titlebar_sync(&self, ctx: &egui::Context) {
        let want = self.theme.titlebar_integrated();
        if caption::installed() && want != caption::active() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(!want));
            caption::set_active(want);
            ctx.request_repaint();
        }
    }

    /// Nom et icône de ce qui est ouvert : le serveur (son image), la page Amis
    /// (son icône dessinée, comme dans le client), sinon les messages privés.
    fn titlebar_title(&self) -> (String, Option<crate::state::model::ImageRef>, bool, Option<Icon>) {
        if !(self.backend.is_connected() && self.store.me.is_some()) {
            return ("Ferrocord".to_owned(), None, false, None);
        }
        match self.store.current_guild() {
            Some(g) if self.store.guild.is_some() => (super::words::guild_name(self.lang, &g.name).to_string(), g.icon, true, None),
            _ if self.friends_shown() => (crate::i18n::table::social::t::FRIENDS.get(self.lang).to_owned(), None, false, Some(Icon::Wave)),
            // Messages privés : notre icône d'accueil (la bulle du rail), comme le client met son logo.
            _ => (tx::DIRECT_MESSAGES.get(self.lang).to_owned(), None, false, Some(Icon::Chat)),
        }
    }

    /// Dessine la barre et traite ses boutons. Avant le reste de l'interface.
    pub(super) fn titlebar_ui(&mut self, root: &mut egui::Ui) {
        let ctx = &root.ctx().clone();
        self.titlebar_sync(ctx);
        let (fullscreen, maximized) = ctx.input(|i| (i.viewport().fullscreen == Some(true), i.viewport().maximized == Some(true)));
        let t = &self.theme;
        let h = height(t, fullscreen);
        // La bande réservée à l'image d'avant fait foi : c'est elle qui est libre.
        let full = ctx.viewport_rect();
        let reserved = ctx.content_rect().top() - full.top();
        if h <= 0.0 || reserved <= 0.0 {
            caption::set_regions(caption::Regions::default());
            return;
        }
        let bar = Rect::from_min_size(full.min, vec2(full.width(), reserved));

        let lang = self.lang;
        let (title, icon, guild, glyph) = self.titlebar_title();
        let logged = self.backend.is_connected() && self.store.me.is_some();
        let inbox_on = self.nav.inbox.open;
        let state = caption::state();
        let bw = (reserved * BUTTON_RATIO).round();
        let slot = |i: f32, w: f32, from: f32| Rect::from_min_size(pos2(from - (i + 1.0) * w, bar.top()), vec2(w, reserved));
        let (close_r, max_r, min_r) = (slot(0.0, bw, bar.right()), slot(1.0, bw, bar.right()), slot(2.0, bw, bar.right()));
        let (help_r, inbox_r) = (slot(0.0, reserved, min_r.left() - t.spacing.item_x), slot(1.0, reserved, min_r.left() - t.spacing.item_x));

        // Précédent / suivant, en haut à gauche comme dans le client : l'historique de
        // navigation (Alt+← / Alt+→, `state::nav::History`).
        let (back_r, fwd_r) = (Rect::from_min_size(pos2(bar.left() + t.spacing.item_x, bar.top()), vec2(reserved, reserved)), Rect::from_min_size(pos2(bar.left() + t.spacing.item_x + reserved, bar.top()), vec2(reserved, reserved)));
        let (can_back, can_fwd) = (self.nav.history.can_back(), self.nav.history.can_forward());
        let (mut back, mut forward) = (false, false);
        let (mut inbox, mut help, mut minimize, mut maximize, mut close) = (false, false, false, false, false);
        let (mut rec_r, mut rec_stop) = (None, false);
        // Panneau du haut de l'interface : tout le reste (panneaux, discussion) se range dessous.
        let panel = egui::Panel::top("barre_de_titre").exact_size(reserved).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(t.colors.bg0));
        panel.show(root, |ui| {
            ui.allocate_rect(bar, Sense::hover());
            let p = ui.painter().clone();
            // Fenêtre modale ouverte : son voile s'arrête sous la barre (zone sûre d'egui).
            let veiled = !ui.memory(|m| m.is_above_modal_layer(ui.layer_id()));

            // Titre centré dans la fenêtre, coupé avant les boutons.
            let font = fonts::medium_font(t.typography.button);
            let side = if icon.is_some() || guild || glyph.is_some() { (reserved - 2.0 * t.spacing.item_y).min(t.typography.icon).max(1.0) } else { 0.0 };
            let gap = if side > 0.0 { t.spacing.item_x } else { 0.0 };
            let room = (inbox_r.left() - bar.left() - 2.0 * t.spacing.control_x - side - gap).max(1.0);
            let mut job = egui::text::LayoutJob::simple_singleline(title.clone(), font, t.colors.text);
            job.wrap = egui::text::TextWrapping::truncate_at_width(room);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let width = side + gap + galley.size().x;
            // Enregistrement d'un appel en cours (plugin, lot E4) : pastille rouge à gauche, le titre lui laisse la place.
            let after_arrows = if logged { fwd_r.right() } else { bar.left() };
            if let Some((r, clicked)) = self.voice_rec_title_pill(ui, Rect::from_min_max(pos2(after_arrows, bar.top()), bar.max)) {
                (rec_r, rec_stop) = (Some(r), clicked);
            }
            let free_left = rec_r.map_or(after_arrows, |r| r.right());
            let left = (bar.center().x - width / 2.0).min(inbox_r.left() - t.spacing.control_x - width).max(free_left + t.spacing.control_x).round();
            if side > 0.0 {
                let r = Rect::from_min_size(pos2(left, (bar.center().y - side / 2.0).round()), Vec2::splat(side));
                let radius = egui::CornerRadius::same((side * 0.3) as u8);
                if let Some(glyph) = glyph {
                    icons::paint(glyph, &p, r, t.colors.muted);
                } else if !util::paint_image(ui, &self.images, r, icon, radius) {
                    util::paint_initials(ui, t, r, (t.colors.pill, t.colors.text_strong), &title);
                }
            }
            p.galley(pos2(left + side + gap, (bar.center().y - galley.size().y / 2.0).round()), galley, t.colors.text);

            if logged {
                // Sans pas à faire, la flèche reste dessinée, éteinte et inerte.
                let nt = crate::i18n::table::nav::t::ACT_BACK.get(lang);
                let arrow = |ui: &mut egui::Ui, rect: Rect, icon: Icon, name: &str, on: bool| {
                    if on {
                        return icon_button(ui, t, rect, icon, name, false).clicked();
                    }
                    let side = (rect.height() * 0.625).round().min(t.typography.icon).max(1.0);
                    icons::paint(icon, ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(side)), t.colors.faint.gamma_multiply(0.45));
                    false
                };
                back = arrow(ui, back_r, Icon::ArrowLeft, nt, can_back);
                forward = arrow(ui, fwd_r, Icon::ArrowRight, crate::i18n::table::nav::t::ACT_FORWARD.get(lang), can_fwd);
                inbox = icon_button(ui, t, inbox_r, Icon::Inbox, crate::i18n::table::nav::t::INBOX.get(lang), inbox_on).clicked();
                help = icon_button(ui, t, help_r, Icon::Help, tx::HELP.get(lang), false).clicked();
            }
            if veiled {
                // Le voile couvre toute la barre, fond des boutons de la fenêtre compris
                // (il s'arrêtait avant eux : un rectangle clair restait en haut à droite).
                // Leurs traits, dessinés ensuite, restent nets : ils répondent toujours.
                p.rect_filled(bar, 0.0, MODAL_VEIL);
            }
            minimize = window_button(ui, t, min_r, Glyph::Minimize, tx::WINDOW_MINIMIZE.get(lang), None, veiled);
            let (glyph, name) = if maximized { (Glyph::Restore, tx::WINDOW_RESTORE) } else { (Glyph::Maximize, tx::WINDOW_MAXIMIZE) };
            // À la souris, ce bouton est celui de Windows (volet d'ancrage) : le clic n'arrive ici qu'au clavier.
            maximize = window_button(ui, t, max_r, glyph, name.get(lang), Some((state.hovered, state.pressed)), false);
            close = window_button(ui, t, close_r, Glyph::Close, tx::WINDOW_CLOSE.get(lang), None, veiled);
            // Fenêtre sans bordure : un filet en haut la détache de ce qui est derrière.
            if !maximized {
                p.hline(bar.x_range(), bar.top() + 0.5, Stroke::new(1.0, t.colors.panel_border));
            }
        });

        let ppp = ctx.pixels_per_point();
        let mut holes = vec![px(min_r, ppp), px(close_r, ppp)];
        if logged {
            holes.extend([px(inbox_r, ppp), px(help_r, ppp)]);
            // Flèche éteinte : la barre s'y laisse saisir comme ailleurs.
            if can_back {
                holes.push(px(back_r, ppp));
            }
            if can_fwd {
                holes.push(px(fwd_r, ppp));
            }
        }
        if let Some(r) = rec_r {
            holes.push(px(r, ppp));
        }
        caption::set_regions(caption::Regions { height: (reserved * ppp).round() as i32, resize: (RESIZE * ppp).round() as i32, maximize: px(max_r, ppp), holes });

        if rec_stop {
            self.voice_rec_stop();
        }
        if inbox {
            self.toggle_inbox();
        }
        if back || forward {
            self.step_history(forward);
        }
        if help {
            self.nav.shortcuts_open = true;
        }
        if minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if maximize {
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        }
        if close {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, Preset, MIN_MUTED, MIN_TEXT};

    #[test]
    fn rect_to_client_pixels() {
        let r = Rect::from_min_max(pos2(100.0, 0.0), pos2(146.0, 32.0));
        assert_eq!(px(r, 1.0), [100, 0, 146, 32]);
        assert_eq!(px(r, 1.5), [150, 0, 219, 48]);
    }

    /// Traits des boutons lisibles sur le fond de la barre, au repos et survolés.
    #[test]
    fn buttons_are_readable_in_every_preset() {
        for p in Preset::ALL {
            let t = p.tokens();
            for glyph in [Glyph::Minimize, Glyph::Close] {
                let (_, ink) = button_colors(&t, glyph, false, false);
                assert!(contrast_ratio(ink, t.colors.bg0) >= MIN_MUTED, "{} repos", p.key());
                let (fill, ink) = button_colors(&t, glyph, true, false);
                assert!(contrast_ratio(ink, fill) >= MIN_TEXT, "{} survol : {:.2}", p.key(), contrast_ratio(ink, fill));
            }
        }
    }

    /// Sans fenêtre doublée, aucune barre : rien n'est réservé.
    #[test]
    fn nothing_reserved_without_a_window() {
        let t = Preset::Sombre.tokens();
        let mut raw = egui::RawInput::default();
        reserve(&t, &mut raw);
        assert_eq!(raw.safe_area_insets, None);
        assert_eq!(height(&t, false), 0.0);
    }
}
