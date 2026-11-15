//! Disposition mobile (phase 7, lot P2) : sur Android, ou dans une fenêtre
//! de moins de 600 points de large (réglage « Disposition »), l'interface
//! passe d'écran en écran comme l'application mobile de Discord : rail et
//! salons, puis la discussion, la liste des membres glissée depuis la
//! droite, une barre d'onglets en bas (Accueil, Messages, Notifications,
//! Vous), un bouton retour et des glissements horizontaux.
//!
//! Rien n'est redessiné ici : chaque écran reprend les zones du bureau
//! (`guild_rail`, `channel_list`, `chat`, `members_ui`, la boîte de
//! réception…), et les fenêtres de bureau (Paramètres, Paramètres du
//! serveur, profil complet) passent en plein écran avec un bouton retour.
//! En disposition bureau, rien de ce module n'est dessiné.
//!
//! Clavier virtuel : ni egui ni winit 0.30 ne donnent sa hauteur sur
//! Android ; la plateforme peut la passer par [`App::set_keyboard_inset`],
//! sinon la marge du réglage (`local.toml`, `[mobile] marge_clavier`) est
//! gardée sous la saisie tant qu'un champ a le clavier.

mod gesture;
mod nav;

use super::App;
use crate::i18n::table::mobile::t as mt;
use crate::i18n::{Lang, Text};
use crate::state::mobile::{Layout, MAX_KEYBOARD_MARGIN};
use crate::theme::icons::{self, Icon};
use crate::theme::{MembersPanel, Tokens};
use eframe::egui::{self, pos2, vec2, Id, Key, Modifiers, Rect, RichText, Sense, Stroke, Vec2};
use gesture::PullView;
use nav::{Nav, Screen, Tab};

/// Côté minimal d'une cible tactile (points).
pub(super) const TOUCH: f32 = 44.0;
/// Hauteur de la barre d'onglets.
const TAB_BAR: f32 = 56.0;
/// Largeur au-dessous de laquelle Android réduit le zoom (points à zoom 1).
const MIN_WIDTH: f32 = 360.0;

#[derive(Default)]
pub(super) struct MobileUi {
    active: bool,
    nav: Nav,
    press: gesture::Press,
    pull: gesture::Pull,
    emu: gesture::TouchEmu,
    /// Hauteur du clavier virtuel donnée par la plateforme (points).
    inset: f32,
    /// Hauteur des cibles d'egui avant le passage au tactile (rendue au retour au bureau).
    interact_before: Option<f32>,
    /// Dernier serveur de l'onglet Accueil (retour depuis les Messages).
    home_guild: Option<u64>,
    #[cfg(target_os = "android")]
    zoom_fitted: bool,
}

fn active_id() -> Id {
    Id::unique("ferrocord.mobile.actif")
}

fn back_id() -> Id {
    Id::unique("ferrocord.mobile.retour")
}

/// Disposition mobile active à cette image.
pub(super) fn is_active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(active_id())).unwrap_or(false)
}

/// Jetons du thème en disposition mobile : cibles d'au moins [`TOUCH`],
/// textes lisibles sur un téléphone.
pub(super) fn touch_tokens(mut t: Tokens) -> Tokens {
    let ty = &mut t.typography;
    ty.body = ty.body.max(16.0);
    ty.small = ty.small.max(13.0);
    ty.button = ty.button.max(16.0);
    ty.title = ty.title.max(18.0);
    ty.icon = ty.icon.max(20.0);
    let s = &mut t.spacing;
    s.channel_row = s.channel_row.max(TOUCH);
    s.member_row = s.member_row.max(48.0);
    t.layout.rail_button = t.layout.rail_button.max(48.0);
    t
}

/// Marge gardée de chaque côté d'une fenêtre (points).
pub(super) const GUTTER: f32 = 12.0;

/// Largeur du contenu d'une fenêtre modale de `factor` × `card_width` :
/// telle quelle sur le bureau ; en disposition mobile, jamais plus large
/// que l'écran moins les marges (cadre de carte compris).
pub(super) fn card_width(ctx: &egui::Context, t: &Tokens, factor: f32) -> f32 {
    let w = t.layout.card_width * factor;
    if !is_active(ctx) {
        return w;
    }
    w.min(fit_width(ctx, t.card_margin().sum().x))
}

/// Disposition mobile : place horizontale d'une fenêtre dont le cadre a
/// `frame` points de marges intérieures (les deux côtés).
pub(super) fn fit_width(ctx: &egui::Context, frame: f32) -> f32 {
    (ctx.content_rect().width() - 2.0 * GUTTER - frame).max(120.0)
}

/// Rangée de boutons ou de libellés : sur le bureau, une ligne
/// (`horizontal`) ; en disposition mobile, elle passe à la ligne au lieu de
/// sortir de l'écran.
pub(super) fn row<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> egui::InnerResponse<R> {
    if is_active(ui.ctx()) { ui.horizontal_wrapped(add) } else { ui.horizontal(add) }
}

/// Bouton « … » à la taille du doigt, qui ouvre un menu d'actions.
pub(super) fn more_menu(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let button = egui::Button::new("…").min_size(Vec2::splat(TOUCH));
    egui::containers::menu::MenuButton::from_button(button).ui(ui, add);
}

/// Contenu d'une fenêtre plein écran : dessiné dans la place de `ui`,
/// coupé à ses bords. Une ligne trop large ne peut plus élargir la fenêtre
/// (qui, centrée, sortirait alors de l'écran par la gauche).
pub(super) fn clipped(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let rect = ui.max_rect();
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(*ui.layout()));
    child.set_clip_rect(rect.intersect(ui.clip_rect()));
    add(&mut child);
    ui.allocate_rect(rect, Sense::hover());
}

/// Zoom qui garde au moins [`MIN_WIDTH`] points de large (`width` : largeur
/// en points à zoom 1, donc déjà divisée par la densité de l'écran).
#[cfg_attr(not(target_os = "android"), allow(dead_code))]
fn fit_zoom(width: f32) -> f32 {
    if width > 0.0 && width < MIN_WIDTH { width / MIN_WIDTH } else { 1.0 }
}

/// Flèche de retour, cible tactile entière.
fn arrow_button(ui: &mut egui::Ui, t: &Tokens, tip: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(TOUCH), Sense::click());
    let hot = resp.hovered() || resp.has_focus() || resp.is_pointer_button_down_on();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let c = rect.center();
    let u = TOUCH * 0.16;
    let stroke = Stroke::new(2.0, if hot { t.colors.text_strong } else { t.colors.text });
    ui.painter().line(vec![pos2(c.x + u * 0.6, c.y - u), pos2(c.x - u * 0.6, c.y), pos2(c.x + u * 0.6, c.y + u)], stroke);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip).clicked()
}

/// Bouton retour d'un en-tête (discussion, appel, Amis, forum) : rien en
/// disposition bureau ; touché, le retour est fait en fin d'image.
pub(super) fn back_button(ui: &mut egui::Ui, t: &Tokens, lang: Lang) {
    if is_active(ui.ctx()) && arrow_button(ui, t, mt::BACK.get(lang)) {
        ui.ctx().data_mut(|d| d.insert_temp(back_id(), true));
    }
}

/// Ligne « retour » en haut d'une fenêtre passée en plein écran ; `true` si touchée.
pub(super) fn back_row(ui: &mut egui::Ui, t: &Tokens, lang: Lang, title: &str) -> bool {
    let mut back = false;
    ui.horizontal(|ui| {
        back = arrow_button(ui, t, mt::BACK.get(lang));
        ui.add(egui::Label::new(RichText::new(title).strong().size(t.typography.title).color(t.colors.text_strong)).truncate());
    });
    back
}

/// Bouton pleine largeur, à la taille du doigt.
fn wide_button(ui: &mut egui::Ui, t: &Tokens, text: &str) -> bool {
    let b = egui::Button::new(RichText::new(text).size(t.typography.button).color(t.colors.text_strong)).min_size(vec2(ui.available_width(), TOUCH)).fill(t.colors.bg2);
    ui.add(b).clicked()
}

/// Maison de l'onglet Accueil.
fn paint_home(painter: &egui::Painter, rect: Rect, color: egui::Color32) {
    let c = rect.center();
    let u = rect.width() * 0.5;
    let p = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    let s = Stroke::new((rect.width() * 0.09).max(1.5), color);
    painter.line(vec![p(-0.85, -0.05), p(0.0, -0.8), p(0.85, -0.05)], s);
    painter.line(vec![p(-0.6, -0.25), p(-0.6, 0.75), p(0.6, 0.75), p(0.6, -0.25)], s);
}

const TABS: [(Tab, Text); 4] = [(Tab::Home, mt::TAB_HOME), (Tab::Dms, mt::TAB_DMS), (Tab::Inbox, mt::TAB_INBOX), (Tab::You, mt::TAB_YOU)];

const LAYOUTS: [(Layout, Text); 3] = [(Layout::Auto, mt::LAYOUT_AUTO), (Layout::Desktop, mt::LAYOUT_DESKTOP), (Layout::Mobile, mt::LAYOUT_MOBILE)];

impl App {
    pub(super) fn mobile_active(&self) -> bool {
        self.mobile.active
    }

    /// Bouton retour des en-têtes (disposition mobile seulement).
    pub(super) fn mobile_back(&self, ui: &mut egui::Ui) {
        back_button(ui, &self.theme, self.lang);
    }

    /// Hauteur du clavier virtuel en points (0 : fermé), donnée par la
    /// plateforme quand elle la connaît (Android, lot P1).
    pub fn set_keyboard_inset(&mut self, points: f32) {
        let v = if points.is_finite() { points.max(0.0) } else { 0.0 };
        if v != self.mobile.inset {
            self.mobile.inset = v;
            self.ctx.request_repaint();
        }
    }

    /// Disposition mobile active (aperçu, tests).
    pub fn is_mobile(&self) -> bool {
        self.mobile.active
    }

    /// Écran mobile montré (aperçu, tests) : `salons`, `discussion`,
    /// `membres`, `messages`, `notifications`, `vous` ; `false` si inconnu.
    pub fn mobile_show(&mut self, what: &str) -> bool {
        match what {
            "salons" => self.mobile.nav.set_tab(Tab::Home),
            "discussion" => self.mobile.nav.open_chat(),
            "membres" => {
                self.mobile.nav.open_chat();
                self.mobile.nav.open_members();
            }
            "messages" => self.mobile_set_tab(Tab::Dms),
            "notifications" => self.mobile_set_tab(Tab::Inbox),
            "vous" => self.mobile_set_tab(Tab::You),
            _ => return false,
        }
        true
    }

    /// Mode du clavier simulé : la souris touche comme un doigt en
    /// disposition mobile (hors Android, où les vrais touchers arrivent).
    pub(super) fn mobile_input(&mut self, raw: &mut egui::RawInput) {
        if self.mobile.active && !cfg!(target_os = "android") {
            self.mobile.emu.feed(raw);
        }
    }

    /// Début d'image : disposition choisie d'après le réglage et la largeur.
    pub(super) fn mobile_decide(&mut self, ctx: &egui::Context) {
        let width = ctx.content_rect().width();
        let active = nav::decide(self.local.prefs.mobile.layout, width, cfg!(target_os = "android"));
        if active != self.mobile.active {
            self.mobile.active = active;
            ctx.data_mut(|d| d.insert_temp(active_id(), active));
            // Jetons tactiles, ou ceux du bureau, dès cette image.
            self.sync_theme();
            ctx.request_repaint();
        }
        match (active, self.mobile.interact_before) {
            (true, None) => {
                self.mobile.interact_before = Some(ctx.global_style().spacing.interact_size.y);
                ctx.all_styles_mut(|s| s.spacing.interact_size.y = s.spacing.interact_size.y.max(TOUCH));
            }
            (false, Some(before)) => {
                self.mobile.interact_before = None;
                ctx.all_styles_mut(|s| s.spacing.interact_size.y = before);
            }
            _ => {}
        }
    }

    /// Espace gardé sous la saisie : clavier virtuel ouvert.
    fn keyboard_inset(&self, ctx: &egui::Context) -> f32 {
        let margin = if ctx.egui_wants_keyboard_input() { f32::from(self.local.prefs.mobile.keyboard_margin) } else { 0.0 };
        self.mobile.inset.max(margin)
    }

    /// Android : zoom réduit une fois si l'écran fait moins de 360 points
    /// de large (la densité, elle, vient de winit : `pixels_per_point`).
    #[cfg(target_os = "android")]
    fn fit_density(&mut self, ctx: &egui::Context) {
        if self.mobile.zoom_fitted {
            return;
        }
        self.mobile.zoom_fitted = true;
        let width = ctx.content_rect().width() * ctx.zoom_factor();
        let z = fit_zoom(width);
        if z < 1.0 && (self.look.edited.layout.zoom - 1.0).abs() < f32::EPSILON {
            self.look.edited.layout.zoom = crate::theme::ZOOM.clamp(z);
        }
    }

    fn mobile_set_tab(&mut self, tab: Tab) {
        let from = self.mobile.nav.tab();
        if from == Tab::Inbox && tab != Tab::Inbox {
            self.nav.inbox.open = false;
        }
        if from == Tab::Home && self.store.guild.is_some() {
            self.mobile.home_guild = self.store.guild;
        }
        match tab {
            Tab::Dms if self.store.guild.is_some() => {
                self.mobile.home_guild = self.store.guild;
                self.store.select_dms();
            }
            Tab::Home if self.store.guild.is_none() => {
                if let Some(g) = self.mobile.home_guild
                    && self.store.select_guild(g)
                {
                    self.scroll_to_bottom = true;
                }
            }
            Tab::Inbox if !self.nav.inbox.open => self.toggle_inbox(),
            _ => {}
        }
        self.mobile.nav.set_tab(tab);
    }

    /// Retour (bouton, touche retour d'Android) : fenêtres plein écran
    /// d'abord, puis écran précédent. `false` à la racine de l'Accueil.
    fn mobile_go_back(&mut self) -> bool {
        if self.params.open {
            self.params.back();
            return true;
        }
        if self.guild_settings_back() || self.close_full_profile() {
            return true;
        }
        if self.mobile.nav.tab() == Tab::Inbox && self.mobile.nav.depth() == 1 {
            self.nav.inbox.open = false;
        }
        self.mobile.nav.back()
    }

    /// Interface principale en disposition mobile (à la place de `main_ui`).
    pub(super) fn mobile_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        super::profile::install(&ctx, &self.store.people);
        super::cosmetics::install(&ctx, &self.store);
        self.search_tick(&ctx);
        self.streamer_banner(ui);
        #[cfg(target_os = "android")]
        self.fit_density(&ctx);
        let t = self.theme.clone();
        let before = (self.store.guild, self.store.channel);
        let scroll_before = self.scroll_to_bottom;
        let members_before = self.look.edited.layout.members;

        let inset = self.keyboard_inset(&ctx);
        if inset > 0.0 {
            egui::Panel::bottom("mobile_clavier").exact_size(inset).resizable(false).frame(egui::Frame::new().fill(t.colors.bg2)).show(ui, |_| {});
        }
        if self.mobile.nav.shows_tab_bar() {
            egui::Panel::bottom("mobile_onglets").exact_size(TAB_BAR).resizable(false).frame(egui::Frame::new().fill(t.colors.bg0)).show(ui, |ui| self.tab_bar(ui, &t));
        }
        let screen = self.mobile.nav.screen();
        let tab = self.mobile.nav.tab();
        match screen {
            Screen::List => self.mobile_list(ui, tab, &t),
            Screen::Chat | Screen::Members => self.mobile_chat(ui, &t),
        }
        if screen == Screen::Members {
            self.members_drawer(ui, &t);
        } else {
            // Aucune liste des membres affichée : rien n'est souscrit.
            self.store.members.watch().show(None);
            self.store.want_members(false, None);
        }

        // Par-dessus le reste, comme sur le bureau.
        self.profile_ui(ui);
        self.servers_ui(ui);
        self.guild_settings_ui(ui);
        self.channel_admin_ui(ui);
        self.moderation_ui(ui);
        self.admin_events_ui(ui);
        self.nav_ui(ui);
        self.calls_overlay(ui);
        self.video_frame_end(ui);
        self.share_picker_ui(ui);

        self.mobile_navigate(&ctx, screen, tab, before, scroll_before, members_before);
    }

    /// Après l'image : salon ouvert, bouton des membres, gestes, retour.
    fn mobile_navigate(&mut self, ctx: &egui::Context, screen: Screen, tab: Tab, before: nav::Place, scroll_before: bool, members_before: MembersPanel) {
        let after = (self.store.guild, self.store.channel);
        let reopened = !scroll_before && self.scroll_to_bottom;
        let opened = match (screen, tab) {
            (Screen::List, Tab::Home | Tab::Dms) => nav::opened_from_list(before, after, reopened),
            // Boîte de réception, profil (« Envoyer un message », serveur en commun).
            (Screen::List, _) => after != before && (after.1.is_some() || after.0.is_none()),
            _ => false,
        };
        if opened {
            self.nav.inbox.open = false;
            self.mobile.nav.open_chat();
            ctx.request_repaint();
        } else if screen == Screen::List && tab == Tab::Inbox && !self.nav.inbox.open {
            // Boîte fermée par sa croix.
            self.mobile.nav.set_tab(Tab::Home);
            ctx.request_repaint();
        }
        // Bouton « Membres » de l'en-tête : le réglage du bureau reste tel quel,
        // la liste glisse depuis la droite.
        if self.look.edited.layout.members != members_before {
            self.look.edited.layout.members = members_before;
            if self.mobile.nav.open_members() {
                ctx.request_repaint();
            }
        }
        if self.mobile.nav.screen() == Screen::Members && !self.has_member_list() {
            self.mobile.nav.back();
        }

        // Gestes : glissement horizontal entre écrans, « tirer pour charger ».
        let (down, pos) = ctx.input(|i| (i.pointer.primary_down(), i.pointer.latest_pos()));
        let ended = self.mobile.press.step(down, pos);
        let blocked = self.params.open || self.guild_settings.win.is_some() || ctx.any_popup_open();
        if !blocked
            && let Some((from, to)) = ended
            && let Some(dir) = gesture::swipe(from, to, ctx.content_rect().width())
        {
            let has_chat = self.store.channel.is_some() || self.store.guild.is_none();
            if self.mobile.nav.swipe(dir, has_chat, self.has_member_list()) {
                ctx.request_repaint();
            }
        }
        if self.mobile.nav.screen() == Screen::Chat
            && let (Some(cid), Some(area)) = (self.store.channel, self.chat_rect)
        {
            let at_top = self.list.at_top(cid);
            let pulled = self.mobile.press.current().filter(|(o, _)| area.contains(*o) && !blocked).map(|(_, d)| d.y);
            let end = ended.filter(|(o, _)| area.contains(*o) && !blocked).map(|(o, e)| e.y - o.y);
            match self.mobile.pull.step(at_top, pulled, end) {
                PullView::Fire => {
                    self.store.load_older(cid);
                }
                PullView::Pulling(p) => self.pull_hint(ctx, area, p),
                PullView::Idle => {}
            }
        }

        let back_key = ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::BrowserBack));
        let back_button = ctx.data_mut(|d| d.remove_temp::<bool>(back_id())).unwrap_or(false);
        if back_key || back_button {
            if self.mobile_go_back() {
                ctx.request_repaint();
            } else if back_key {
                // Racine : Android met l'application en arrière-plan (lot P1).
                crate::platform::back_at_root();
            }
        }
    }

    /// Indicateur « Tirez / Relâchez pour charger ».
    fn pull_hint(&self, ctx: &egui::Context, area: Rect, progress: f32) {
        let t = &self.theme;
        let text = if progress >= 1.0 { mt::RELEASE } else { mt::PULL }.get(self.lang);
        let at = pos2(area.center().x, area.top() + TOUCH * 1.5 + progress * TOUCH);
        egui::Area::new(Id::unique("mobile_tirer")).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(at).interactable(false).show(ctx, |ui| {
            egui::Frame::new().fill(t.colors.bg3).corner_radius(t.r_toast()).inner_margin(t.toast_margin()).show(ui, |ui| {
                ui.label(RichText::new(text).small().color(t.colors.text_strong));
            });
        });
    }

    /// Racine de l'onglet.
    fn mobile_list(&mut self, ui: &mut egui::Ui, tab: Tab, t: &Tokens) {
        match tab {
            Tab::Home => {
                let rail = egui::Frame::new().fill(t.colors.bg0).inner_margin(t.rail_margin());
                egui::Panel::left("rail").exact_size(t.rail_panel_width()).resizable(false).frame(rail).show(ui, |ui| self.guild_rail(ui));
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg1)).show(ui, |ui| self.channel_list(ui));
            }
            Tab::Dms => {
                if self.store.guild.is_some() {
                    self.store.select_dms();
                }
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg1)).show(ui, |ui| self.channel_list(ui));
            }
            Tab::Inbox => {
                self.voice_tick(ui);
                if !self.nav.inbox.open {
                    self.toggle_inbox();
                }
                // La boîte de réception se place dans cette zone (dessinée avec les fenêtres).
                let r = egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg1)).show(ui, |_| {});
                self.chat_rect = Some(r.response.rect.with_min_y(r.response.rect.top() - TOUCH));
            }
            Tab::You => {
                self.voice_tick(ui);
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg1).inner_margin(t.channels_margin())).show(ui, |ui| self.you_page(ui, t));
            }
        }
    }

    /// Discussion, vue d'appel, forum ou page Amis : l'écran entier.
    fn mobile_chat(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        self.voice_tick(ui);
        let home = self.store.guild.is_none() && self.store.channel.is_none();
        let chat = egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2)).show(ui, |ui| if home { self.friends_page(ui) } else { self.chat(ui) });
        self.chat_rect = Some(chat.response.rect);
    }

    /// Liste des membres glissée depuis la droite, voile sur la discussion.
    fn members_drawer(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let ctx = ui.ctx().clone();
        let area = self.chat_rect.unwrap_or_else(|| ctx.content_rect());
        let width = (area.width() * 0.85).min(360.0);
        let drawer = Rect::from_min_max(pos2(area.right() - width, area.top()), area.max);
        let mut close = false;
        egui::Area::new(Id::unique("mobile_membres")).order(egui::Order::Foreground).fixed_pos(area.min).show(&ctx, |ui| {
            ui.painter().rect_filled(area, 0.0, t.colors.scrim);
            close = ui.allocate_rect(area, Sense::click()).clicked();
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(drawer));
            child.set_clip_rect(drawer);
            child.painter().rect_filled(drawer, 0.0, t.colors.bg1);
            let group = self.store.guild.is_none() && self.store.channel.is_some_and(|c| self.is_group(c));
            if group {
                let frame = egui::Frame::new().fill(t.colors.bg1).inner_margin(t.channels_margin());
                egui::CentralPanel::default().frame(frame).show(&mut child, |ui| self.group_panel(ui));
            } else {
                // Panneau du bureau, affiché quel que soit son réglage, à la largeur du tiroir.
                let (shown, w) = (self.theme.layout.members, self.theme.layout.members_width);
                self.theme.layout.members = MembersPanel::Shown;
                self.theme.layout.members_width = width;
                self.members_ui(&mut child);
                self.theme.layout.members = shown;
                self.theme.layout.members_width = w;
            }
        });
        if close {
            self.mobile.nav.back();
            ctx.request_repaint();
        }
    }

    /// Barre d'onglets du bas.
    fn tab_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let rect = ui.max_rect();
        let w = rect.width() / TABS.len() as f32;
        let side = t.typography.icon.max(22.0);
        let mut pick = None;
        for (i, (tab, label)) in TABS.iter().enumerate() {
            let r = Rect::from_min_size(pos2(rect.left() + w * i as f32, rect.top()), vec2(w, rect.height()));
            let resp = ui.interact(r, ui.scope_id().with(("onglet", i)), Sense::click());
            let on = self.mobile.nav.tab() == *tab;
            let color = if on { t.colors.text_strong } else { t.colors.muted };
            let icon = Rect::from_center_size(pos2(r.center().x, r.top() + 6.0 + side / 2.0), Vec2::splat(side));
            let painter = ui.painter();
            match tab {
                Tab::Home => paint_home(painter, icon, color),
                Tab::Dms => icons::paint(Icon::Forum, painter, icon, color),
                Tab::Inbox => icons::paint(Icon::Inbox, painter, icon, color),
                Tab::You => match self.store.me.as_ref().filter(|_| t.avatars_shown()) {
                    Some(me) => super::util::paint_avatar(ui, &self.images, t, icon, &me.avatar, &me.name),
                    None => {
                        painter.circle_stroke(icon.center(), side * 0.42, Stroke::new(2.0, color));
                    }
                },
            }
            if on {
                ui.painter().rect_filled(Rect::from_center_size(pos2(r.center().x, r.top() + 1.5), vec2(w * 0.4, 3.0)), 1.5, t.colors.accent);
            }
            ui.painter().text(pos2(r.center().x, r.bottom() - 6.0), egui::Align2::CENTER_BOTTOM, label.get(lang), egui::FontId::proportional(t.typography.small), color);
            resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, label.get(lang)));
            if resp.clicked() {
                pick = Some(*tab);
            }
        }
        ui.allocate_rect(rect, Sense::hover());
        if let Some(tab) = pick {
            self.mobile_set_tab(tab);
        }
    }

    /// Onglet « Vous » : profil, Paramètres, disposition.
    fn you_page(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let me = self.store.me.as_ref().map(|u| (u.id, u.name.clone(), u.avatar));
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.add_space(t.spacing.category_gap);
            if let Some((id, name, avatar)) = &me {
                ui.horizontal(|ui| {
                    if t.avatars_shown() {
                        let side = t.layout.avatar_size * 1.6;
                        let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                        let rect = super::util::pixel_square(ui, slot.min, side);
                        super::util::paint_avatar(ui, &self.images, t, rect, avatar, name);
                        super::profile::status_dot(ui, t, rect, *id, t.colors.bg1);
                    }
                    ui.add(egui::Label::new(RichText::new(name).strong().size(t.typography.title).color(t.colors.text_strong)).truncate());
                });
                ui.add_space(t.spacing.category_gap);
                if wide_button(ui, t, mt::MY_PROFILE.get(lang)) {
                    self.open_full_profile(*id, None);
                }
            }
            ui.add_space(t.spacing.item_y);
            if wide_button(ui, t, mt::SETTINGS.get(lang)) {
                self.params.show_menu();
            }
            ui.add_space(t.spacing.item_y);
            ui.horizontal(|ui| self.voice_buttons(ui));
            self.mobile_settings(ui, t);
        });
    }

    /// Réglage « Disposition » et marge du clavier (page Apparence, onglet Vous).
    pub(super) fn mobile_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let mut p = self.local.prefs.mobile;
        ui.add_space(t.spacing.category_gap);
        ui.label(RichText::new(mt::LAYOUT.get(lang)).strong().color(t.colors.text_strong));
        ui.horizontal_wrapped(|ui| {
            for (l, text) in LAYOUTS {
                ui.selectable_value(&mut p.layout, l, text.get(lang));
            }
        });
        ui.add(egui::Label::new(RichText::new(mt::LAYOUT_TIP.get(lang)).small().color(t.colors.muted)).wrap());
        ui.horizontal(|ui| {
            ui.label(mt::KEYBOARD_MARGIN.get(lang));
            ui.add(egui::DragValue::new(&mut p.keyboard_margin).range(0..=MAX_KEYBOARD_MARGIN).suffix(" pt"));
        });
        ui.add(egui::Label::new(RichText::new(mt::KEYBOARD_TIP.get(lang)).small().color(t.colors.muted)).wrap());
        if p != self.local.prefs.mobile {
            let mut all = self.local.prefs.clone();
            all.mobile = p;
            self.set_local_prefs(all);
        }
    }
}

#[cfg(test)]
mod tests;
