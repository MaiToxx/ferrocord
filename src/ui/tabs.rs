//! Onglets de salons (plugin du lot E2) : une barre au-dessus de la
//! discussion. Le modèle est dans `state::windows` ; ici, la barre, ses
//! gestes et ses raccourcis.
//!
//! - Un onglet n'est qu'un endroit (serveur, salon). L'onglet actif est
//!   toujours l'endroit affiché : la navigation ordinaire le déplace. Un
//!   onglet d'arrière-plan ne charge rien et ne dessine que sa pastille ; sa
//!   position de défilement et son brouillon sont ceux du salon (liste et
//!   saisie les gardent déjà par salon).
//! - La barre n'existe qu'à partir de deux onglets : avec un seul (ou le
//!   plugin coupé), la mise en page est celle d'avant, au pixel près.
//! - Clic molette ou Ctrl+clic sur un salon, « Ouvrir dans un onglet » :
//!   l'onglet s'ouvre à droite de l'actif, en arrière-plan, comme dans un
//!   navigateur. Ctrl+Tab, Ctrl+Maj+Tab, Ctrl+1 à 9, Ctrl+W.
//! - Enregistrés par compte dans `local.toml` : tout de suite pour un geste
//!   sur un onglet, à la prochaine image deux secondes plus tard (ou à la
//!   fermeture) pour un simple changement de salon. Aucune minuterie.

use super::{menu, words, App};
use crate::i18n::table::e2::{f as wf, t as wt};
use crate::plugins::{self, Id as Plugin};
use crate::state::nav::Place;
use crate::state::windows::Tabs;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, pos2, vec2, Key, Modifiers, Rect, Sense};
use std::time::{Duration, Instant};

/// Largeurs d'un onglet ordinaire (points) ; en deçà du minimum, la barre défile.
const TAB_MIN: f32 = 112.0;
const TAB_MAX: f32 = 220.0;
/// Délai avant d'enregistrer un simple changement de salon.
const SAVE_AFTER: Duration = Duration::from_secs(2);

#[derive(Default)]
pub(super) struct TabsUi {
    pub(super) model: Tabs,
    /// Compte dont `model` montre les onglets.
    owner: Option<u64>,
    /// `local.toml` était lu quand les onglets ont été repris.
    from_file: bool,
    /// Modèle changé par la navigation, pas encore enregistré.
    dirty: Option<Instant>,
    /// L'onglet actif est ramené dans la barre à la prochaine image.
    reveal: bool,
}

/// Geste fait sur la barre, appliqué après son dessin.
#[derive(Clone, Copy)]
enum Act {
    Select(usize),
    Close(usize),
    CloseOthers(usize),
    Pin(usize, bool),
    Move(usize, usize),
    Right(Place),
    Window(Place),
}

/// Ce que la barre montre d'un onglet.
struct Shown {
    place: Place,
    icon: Option<Icon>,
    title: String,
    /// Serveur du salon, pour l'infobulle.
    context: String,
    unread: bool,
    mentions: u32,
    pinned: bool,
}

/// Hauteur de la barre.
pub(super) fn bar_height(t: &Tokens) -> f32 {
    (menu::row_height(t) + t.spacing.item_y).round()
}

fn tab_id(p: Place) -> egui::Id {
    egui::Id::unique(("onglet", p.guild, p.channel))
}

impl App {
    /// Le plugin vaut ici (jamais dans la disposition mobile).
    fn tabs_on(&self) -> bool {
        plugins::on(Plugin::ChannelTabs) && !self.mobile_active()
    }

    /// La barre est affichée : au moins deux onglets.
    pub(super) fn tabs_shown(&self) -> bool {
        self.tabs_on() && self.tabs.model.list.len() >= 2
    }

    /// Clic qui ouvre un onglet au lieu d'aller au salon (Ctrl enfoncé).
    pub(super) fn tab_gesture(&self, ui: &egui::Ui) -> bool {
        self.tabs_on() && ui.input(|i| i.modifiers.command)
    }

    /// Début de l'image : onglets du compte repris, onglet actif mis d'accord
    /// avec l'endroit affiché. Plugin coupé : une comparaison.
    pub(super) fn tabs_sync(&mut self) {
        if !self.tabs_on() {
            if self.tabs.owner.is_some() {
                self.tabs_flush();
                self.tabs = TabsUi::default();
            }
            return;
        }
        let Some(me) = self.store.me.as_ref().map(|u| u.id) else { return };
        let loaded = self.local.loaded;
        if self.tabs.owner != Some(me) || (loaded && !self.tabs.from_file) {
            self.tabs.owner = Some(me);
            self.tabs.from_file = loaded;
            self.tabs.dirty = None;
            let mut model = self.local.prefs.windows.tabs.get(&me).cloned().unwrap_or_default();
            // Salons supprimés ou serveurs quittés depuis : leurs onglets partent.
            model.retain(|p| self.place_exists(p));
            self.tabs.model = model;
            self.tabs.reveal = true;
        }
        let now = Place { guild: self.store.guild, channel: self.store.channel };
        if self.tabs.model.observe(now) {
            self.tabs.reveal = true;
            self.tabs.dirty.get_or_insert_with(Instant::now);
        }
        if self.tabs.dirty.is_some_and(|since| since.elapsed() >= SAVE_AFTER) {
            self.tabs_flush();
        }
    }

    /// Enregistre les onglets du compte s'ils ont changé.
    fn tabs_flush(&mut self) {
        self.tabs.dirty = None;
        let Some(me) = self.tabs.owner else { return };
        let saved = &mut self.local.prefs.windows.tabs;
        if saved.get(&me) == Some(&self.tabs.model) {
            return;
        }
        saved.insert(me, self.tabs.model.clone());
        // Avant la lecture de `local.toml`, rien n'est écrit (le fichier l'emporterait).
        if self.local.loaded {
            self.save_local_prefs();
        }
    }

    /// Fermeture de l'application : dernier état des onglets.
    pub(super) fn tabs_exit(&mut self) {
        if self.tabs.dirty.is_some() {
            self.tabs_flush();
        }
    }

    /// Ouvre `place` dans un onglet d'arrière-plan (clic molette, Ctrl+clic, menu).
    pub fn tab_open(&mut self, place: Place) {
        if !self.tabs_on() {
            return;
        }
        // L'onglet de l'endroit affiché existe avant celui qu'on ouvre.
        self.tabs_sync();
        self.tabs.model.open(place);
        self.tabs.reveal = true;
        self.tabs_flush();
        self.ctx.request_repaint();
    }

    /// Endroits des onglets et rang de l'actif (tests, aperçu).
    pub fn tab_places(&self) -> (Vec<Place>, usize) {
        (self.tabs.model.list.iter().map(|t| t.place).collect(), self.tabs.model.active)
    }

    /// Épingle l'onglet `i` (aperçu, tests).
    pub fn tab_pin(&mut self, i: usize, pinned: bool) {
        self.tabs_apply(Act::Pin(i, pinned));
    }

    fn tab_select(&mut self, i: usize) {
        let Some(place) = self.tabs.model.list.get(i).map(|t| t.place) else { return };
        if !self.place_exists(&place) {
            // Salon disparu : l'onglet se ferme au lieu d'ouvrir un salon vide.
            self.tabs.model.close(i);
        } else if i != self.tabs.model.active {
            self.tabs.model.active = i;
            self.go(place);
        }
        self.tabs.reveal = true;
        self.tabs_flush();
    }

    fn tabs_apply(&mut self, act: Act) {
        match act {
            Act::Select(i) => return self.tab_select(i),
            Act::Close(i) => {
                if let Some(p) = self.tabs.model.close(i) {
                    self.go(p);
                }
            }
            Act::CloseOthers(i) => {
                if let Some(p) = self.tabs.model.close_others(i) {
                    self.go(p);
                }
            }
            Act::Pin(i, pinned) => self.tabs.model.pin(i, pinned),
            Act::Move(from, to) => {
                self.tabs.model.move_to(from, to);
            }
            Act::Right(p) => self.split_open(p),
            Act::Window(p) => self.detach_open(p),
        }
        self.tabs_flush();
        self.ctx.request_repaint();
    }

    /// Ctrl+Tab, Ctrl+Maj+Tab, Ctrl+W, Ctrl+1 à 9 : seulement quand la barre
    /// est affichée (sinon Ctrl+chiffre va aux serveurs, comme avant).
    pub(super) fn tabs_keys(&mut self, ui: &egui::Ui) {
        if !self.tabs_shown() {
            return;
        }
        const NUMS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        let (back, forward, close, number) = ui.input_mut(|i| {
            // Avec Maj d'abord : egui tient Ctrl+Tab pour vrai aussi quand Maj est enfoncée.
            let back = i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::Tab);
            let forward = !back && i.consume_key(Modifiers::COMMAND, Key::Tab);
            let close = i.consume_key(Modifiers::COMMAND, Key::W);
            let number = NUMS.iter().position(|k| i.consume_key(Modifiers::COMMAND, *k));
            (back, forward, close, number)
        });
        if back || forward {
            self.tab_cycle(forward);
        }
        if close {
            self.tabs_apply(Act::Close(self.tabs.model.active));
        }
        if let Some(n) = number.filter(|n| *n < self.tabs.model.list.len()) {
            self.tab_select(n);
        }
    }

    /// Ferme l'onglet actif (Ctrl+W, palette).
    pub(super) fn tab_close_active(&mut self) {
        self.tabs_apply(Act::Close(self.tabs.model.active));
    }

    /// « Nouvel onglet » : l'endroit affiché garde le sien, un onglet s'ouvre sur la page Amis.
    pub(super) fn tab_new(&mut self) {
        if !self.tabs_on() {
            return;
        }
        self.tabs_sync();
        let i = self.tabs.model.open(Place { guild: None, channel: None });
        self.tab_select(i);
    }

    /// Onglet suivant ou précédent (raccourci, palette).
    pub(super) fn tab_cycle(&mut self, forward: bool) {
        let n = self.tabs.model.list.len();
        if n >= 2 {
            let a = self.tabs.model.active;
            self.tab_select(if forward { (a + 1) % n } else { (a + n - 1) % n });
        }
    }

    /// Entrées « Ouvrir dans un onglet », « Ouvrir à droite », « Ouvrir dans
    /// une fenêtre » du menu d'un salon ou d'un message privé ; rien si les
    /// trois plugins sont coupés.
    pub(super) fn windows_menu(&mut self, ui: &mut egui::Ui, place: Place) {
        let t = self.theme.clone();
        let lang = self.lang;
        if self.tabs_on() && menu::item(ui, &t, wt::OPEN_IN_TAB.get(lang), Some(Icon::Plus), false).clicked() {
            self.tab_open(place);
        }
        if self.split_offered() && menu::item(ui, &t, wt::OPEN_RIGHT.get(lang), Some(Icon::ArrowRight), false).clicked() {
            self.split_open(place);
        }
        if self.detach_offered() && menu::item(ui, &t, wt::OPEN_WINDOW.get(lang), Some(Icon::Media), false).clicked() {
            self.detach_open(place);
        }
    }

    /// Icône et nom d'un endroit, et son serveur (onglet, en-tête d'une
    /// discussion secondaire, palette).
    pub(super) fn place_title(&self, p: Place) -> (Option<Icon>, String, String) {
        let s = &self.store;
        let lang = self.lang;
        let guild_name = |g: u64| s.guilds.iter().find(|x| x.id == g).map(|x| words::guild_name(lang, &x.name).to_string()).unwrap_or_default();
        match (p.guild, p.channel) {
            (None, None) => (Some(Icon::Members), wt::TAB_FRIENDS.get(lang).to_string(), String::new()),
            (Some(g), None) => (Some(Icon::Chat), guild_name(g), String::new()),
            (None, Some(c)) => match s.dms.iter().find(|d| d.id == c) {
                Some(d) => (None, words::dm_name(lang, &d.name).to_string(), String::new()),
                None => (None, wt::TAB_UNKNOWN.get(lang).to_string(), String::new()),
            },
            (Some(g), Some(c)) => {
                let icon = if s.is_thread(c) {
                    Icon::Thread
                } else if s.is_forum(c) {
                    Icon::Forum
                } else if s.is_announcement(c) {
                    Icon::Megaphone
                } else if s.voice.channel(c).is_some() {
                    Icon::Speaker
                } else {
                    Icon::Hash
                };
                let name = s.channel_name(c).map(str::to_string).or_else(|| self.voice_room_name(c)).unwrap_or_else(|| wt::TAB_UNKNOWN.get(lang).to_string());
                (Some(icon), name, guild_name(g))
            }
        }
    }

    fn tabs_view(&self) -> Vec<Shown> {
        let s = &self.store;
        self.tabs
            .model
            .list
            .iter()
            .map(|tab| {
                let (icon, title, context) = self.place_title(tab.place);
                let (unread, mentions) = match tab.place.channel {
                    Some(c) => (s.unread().contains(&c), s.mention_count(c)),
                    None => (false, 0),
                };
                Shown { place: tab.place, icon, title, context, unread, mentions, pinned: tab.pinned }
            })
            .collect()
    }

    /// La barre, en haut de la zone de discussion ; rien avec un seul onglet.
    pub(super) fn tabs_bar(&mut self, ui: &mut egui::Ui) {
        if !self.tabs_shown() {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let tabs = self.tabs_view();
        let active = self.tabs.model.active;
        let reveal = std::mem::take(&mut self.tabs.reveal);
        let (split, detach) = (self.split_offered(), self.detach_offered());
        let mut act = None;
        let height = bar_height(&t);
        egui::Panel::top("onglets").exact_size(height).resizable(false).show_separator_line(false).frame(egui::Frame::new().fill(t.colors.bg1)).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Side);
            let full = ui.max_rect();
            let gap = t.spacing.item_y;
            let pinned_w = (height + gap).round();
            let pins = tabs.iter().filter(|s| s.pinned).count();
            let others = tabs.len() - pins;
            let room = full.width() - pins as f32 * pinned_w;
            let fitted = if others == 0 { TAB_MAX } else { (room / others as f32).floor() };
            // Trop d'onglets pour la largeur : ils gardent leur minimum, la barre
            // défile et un bouton à droite en donne la liste.
            let overflow = fitted < TAB_MIN;
            let side = Tokens::icon_side(t.typography.body);
            let list_w = if overflow { (side + 2.0 * t.spacing.item_x).round() } else { 0.0 };
            let tab_w = if overflow { TAB_MIN } else { fitted.min(TAB_MAX) };
            let strip = Rect::from_min_max(full.min, pos2(full.right() - list_w, full.bottom()));
            // La molette sur la barre la fait défiler de côté.
            if overflow && ui.rect_contains_pointer(strip) {
                ui.input_mut(|i| {
                    if i.smooth_scroll_delta.x == 0.0 {
                        i.smooth_scroll_delta.x = std::mem::take(&mut i.smooth_scroll_delta.y);
                    }
                });
            }
            let mut rects: Vec<Rect> = Vec::with_capacity(tabs.len());
            let mut dragged = None;
            ui.scope_builder(egui::UiBuilder::new().max_rect(strip).layout(egui::Layout::left_to_right(egui::Align::Min)), |ui| {
                egui::ScrollArea::horizontal().id_salt("onglets").auto_shrink(false).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
                    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
                    for (i, s) in tabs.iter().enumerate() {
                        let w = if s.pinned { pinned_w } else { tab_w };
                        let (slot, _) = ui.allocate_exact_size(vec2(w, height), Sense::hover());
                        let rect = Rect::from_min_max(pos2(slot.left(), slot.top() + gap), pos2(slot.right() - 1.0, slot.bottom()));
                        rects.push(rect);
                        let resp = ui.interact(rect, tab_id(s.place), Sense::click_and_drag());
                        let is_active = i == active;
                        // Croix : sur l'onglet actif ou survolé, jamais sur un épinglé.
                        let cross_side = (side * 0.8).round();
                        let cross = Rect::from_center_size(pos2(rect.right() - t.spacing.item_x - cross_side / 2.0, rect.center().y), egui::Vec2::splat(cross_side));
                        let has_cross = !s.pinned && (is_active || resp.contains_pointer());
                        let closing = has_cross && {
                            let x = ui.interact(cross.expand(2.0), tab_id(s.place).with("x"), Sense::click());
                            x.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, wt::TAB_CLOSE.get(lang)));
                            x.on_hover_text(wt::TAB_CLOSE.get(lang)).clicked()
                        };
                        if ui.is_rect_visible(rect) {
                            paint_tab(ui, &t, rect, s, is_active, resp.hovered() || resp.has_focus(), has_cross.then_some(cross));
                        }
                        super::keys::focus_ring(ui, &t, &resp);
                        let name = if s.mentions > 0 {
                            wf::tab_mentions(lang, &s.title, s.mentions)
                        } else if s.unread {
                            wf::tab_unread(lang, &s.title)
                        } else {
                            s.title.clone()
                        };
                        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, is_active, &name));
                        if is_active && reveal {
                            resp.scroll_to_me(None);
                        }
                        if closing || resp.middle_clicked() {
                            act = Some(Act::Close(i));
                        } else if resp.clicked() {
                            act = Some(Act::Select(i));
                        }
                        if resp.dragged() {
                            dragged = Some(i);
                        }
                        let tip = if s.context.is_empty() { s.title.clone() } else { format!("{} \u{2013} {}", s.title, s.context) };
                        let resp = if resp.dragged() { resp } else { resp.on_hover_text(tip) };
                        resp.context_menu(|ui| {
                            if menu::item(ui, &t, wt::TAB_CLOSE.get(lang), Some(Icon::Close), false).clicked() {
                                act = Some(Act::Close(i));
                            }
                            if menu::item(ui, &t, wt::TAB_CLOSE_OTHERS.get(lang), None, false).clicked() {
                                act = Some(Act::CloseOthers(i));
                            }
                            let pin = if s.pinned { wt::TAB_UNPIN } else { wt::TAB_PIN };
                            if menu::item(ui, &t, pin.get(lang), Some(Icon::Pin), false).clicked() {
                                act = Some(Act::Pin(i, !s.pinned));
                            }
                            if s.place.channel.is_some() && (split || detach) {
                                menu::separator(ui, &t);
                                if split && menu::item(ui, &t, wt::OPEN_RIGHT.get(lang), Some(Icon::ArrowRight), false).clicked() {
                                    act = Some(Act::Right(s.place));
                                }
                                if detach && menu::item(ui, &t, wt::OPEN_WINDOW.get(lang), Some(Icon::Media), false).clicked() {
                                    act = Some(Act::Window(s.place));
                                }
                            }
                        });
                    }
                });
            });
            // Glisser : l'onglet prend la place de celui que le pointeur survole.
            if let (Some(from), Some(p)) = (dragged, ui.ctx().pointer_latest_pos())
                && let Some(to) = rects.iter().position(|r| r.x_range().contains(p.x))
                && to != from
            {
                act = Some(Act::Move(from, to));
            }
            if overflow {
                let slot = Rect::from_min_max(pos2(strip.right(), full.top() + gap), full.max);
                let resp = ui.interact(slot, egui::Id::unique("onglets-liste"), Sense::click());
                let hot = resp.hovered() || resp.has_focus() || egui::Popup::is_id_open(ui.ctx(), egui::Id::unique("onglets-liste-menu"));
                icons::paint(Icon::ChevronDown, ui.painter(), Rect::from_center_size(slot.center(), egui::Vec2::splat(side)), if hot { t.colors.text_strong } else { t.colors.muted });
                super::keys::focus_ring(ui, &t, &resp);
                let tip = wt::TAB_LIST.get(lang);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                let resp = resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
                egui::Popup::menu(&resp).id(egui::Id::unique("onglets-liste-menu")).show(|ui| {
                    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.6).show(ui, |ui| {
                        for (i, s) in tabs.iter().enumerate() {
                            let label = if s.context.is_empty() { s.title.clone() } else { format!("{} \u{2013} {}", s.title, s.context) };
                            if menu::radio(ui, &t, &label, i == active).clicked() {
                                act = Some(Act::Select(i));
                                ui.close();
                            }
                        }
                    });
                });
            }
        });
        if let Some(act) = act {
            self.tabs_apply(act);
        }
    }
}

/// Un onglet : fond, icône, nom, pastille, croix.
fn paint_tab(ui: &egui::Ui, t: &Tokens, rect: Rect, s: &Shown, active: bool, hot: bool, cross: Option<Rect>) {
    let p = ui.painter();
    let r = t.r_control();
    let corners = egui::CornerRadius { nw: r.nw, ne: r.ne, sw: 0, se: 0 };
    if active {
        // Même fond que la discussion : l'onglet s'y raccorde.
        p.rect_filled(rect, corners, t.colors.bg2);
        // Trait d'accent en haut : l'onglet actif se lit même quand les deux fonds sont proches.
        p.rect_filled(Rect::from_min_max(rect.min, pos2(rect.right(), rect.top() + 2.0)), corners, t.colors.accent);
    } else if hot {
        p.rect_filled(rect, corners, t.colors.row_hover);
    }
    let lit = active || hot || s.unread || s.mentions > 0;
    let ink = if lit { t.colors.text_strong } else { t.colors.muted };
    let side = Tokens::icon_side(t.typography.body);
    let pad = t.spacing.item_x;
    let icon = if s.pinned { Rect::from_center_size(rect.center(), egui::Vec2::splat(side)) } else { Rect::from_center_size(pos2(rect.left() + pad + side / 2.0, rect.center().y), egui::Vec2::splat(side)) };
    match s.icon {
        Some(i) => icons::paint(i, p, icon, if lit { t.colors.text } else { t.colors.faint }),
        None => {
            p.text(icon.center(), egui::Align2::CENTER_CENTER, "@", egui::FontId::proportional(t.typography.body), if lit { t.colors.text } else { t.colors.faint });
        }
    }
    // Pastille : compte de mentions, sinon point de non-lu.
    let mut right = cross.map_or(rect.right() - pad, |c| c.left() - pad / 2.0);
    if s.mentions > 0 {
        let text = if s.mentions > 99 { "99+".to_string() } else { s.mentions.to_string() };
        let galley = p.layout_no_wrap(text, super::fonts::demi_font(t.typography.small), t.colors.on_accent);
        let h = (t.typography.small * 4.0 / 3.0).round();
        let w = (galley.size().x + h * 0.5).round().max(h);
        let pill = if s.pinned { Rect::from_min_size(pos2(rect.right() - w - 2.0, rect.top() + 2.0), vec2(w, h)) } else { Rect::from_min_size(pos2(right - w, rect.center().y - h / 2.0), vec2(w, h)) };
        p.rect_filled(pill, egui::CornerRadius::same((h / 2.0).round() as u8), t.danger_fill());
        p.galley(pill.center() - galley.size() / 2.0, galley, t.colors.on_accent);
        right = pill.left() - pad / 2.0;
    } else if s.unread && !active {
        let d = t.shapes.mention_dot.max(3.0);
        let at = if s.pinned { pos2(rect.right() - d - 3.0, rect.top() + d + 3.0) } else { pos2(right - d, rect.center().y) };
        p.circle_filled(at, d, t.colors.text_strong);
        right = at.x - d - pad / 2.0;
    }
    if !s.pinned {
        let left = icon.right() + pad / 2.0;
        let font = if active { super::fonts::demi_font(t.typography.small) } else { egui::FontId::proportional(t.typography.small) };
        let mut job = egui::text::LayoutJob::simple_singleline(s.title.clone(), font, ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(1.0));
        let galley = p.layout_job(job);
        p.galley(pos2(left, rect.center().y - galley.size().y / 2.0), galley, ink);
    }
    if let Some(c) = cross {
        let over = ui.rect_contains_pointer(c.expand(2.0));
        if over {
            p.rect_filled(c.expand(2.0), t.r_row(), t.colors.hover);
        }
        icons::paint(Icon::Close, p, c.shrink(1.0), if over { t.colors.text_strong } else { t.colors.muted });
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, frame, key, test_message};
    use super::super::tests::{find, frame as wide_frame};
    use super::*;
    use crate::state::Event;

    const CTRL: Modifiers = Modifiers { ctrl: true, command: true, alt: false, shift: false, mac_cmd: false };

    fn at(guild: u64, channel: u64) -> Place {
        Place { guild: Some(guild), channel: Some(channel) }
    }

    /// Tout ce que l'écran nomme, avec sa place au demi-point près.
    fn layout(app: &mut App) -> Vec<(String, [i32; 4])> {
        let mut out = wide_frame(app, Vec::new());
        for _ in 0..3 {
            out = wide_frame(app, Vec::new());
        }
        let update = out.platform_output.accesskit_update.unwrap_or_else(|| panic!("arbre d'accessibilité"));
        let mut rows: Vec<(String, [i32; 4])> = update
            .nodes
            .iter()
            .filter_map(|(_, n)| {
                let b = n.bounds()?;
                Some((n.label()?.to_string(), [(b.x0 * 2.0) as i32, (b.y0 * 2.0) as i32, (b.x1 * 2.0) as i32, (b.y1 * 2.0) as i32]))
            })
            .collect();
        rows.sort();
        rows
    }

    /// Les cinq plugins du lot coupés, ou activés sans rien d'ouvert : la
    /// mise en page est la même, élément par élément.
    #[test]
    fn layout_is_unchanged_without_tabs() {
        let mut off = app();
        for id in [Plugin::ChannelTabs, Plugin::CommandPalette, Plugin::ChannelZoom, Plugin::SplitView, Plugin::DetachedWindows] {
            off.set_plugin(id, false);
        }
        let a = layout(&mut off);
        assert!(a.len() > 10, "{a:?}");
        off.tab_open(at(9, 11));
        assert_eq!(layout(&mut off), a, "plugin coupé : rien ne s'ouvre");
        // L'état des plugins est celui du fil : l'application suivante le remplace.
        let mut on = app();
        assert_eq!(layout(&mut on), a);
        assert!(!on.tabs_shown());
        // Un deuxième onglet : la barre apparaît.
        on.tab_open(at(9, 11));
        let with_bar = layout(&mut on);
        assert!(on.tabs_shown());
        assert!(with_bar.iter().any(|(l, _)| l == "Fermer l'onglet"), "{with_bar:?}");
        // Revenu à un onglet : la mise en page d'avant, au pixel près.
        on.tabs_apply(Act::Close(1));
        assert_eq!(layout(&mut on), a);
    }

    #[test]
    fn keys_walk_select_and_close_tabs_and_they_are_remembered() {
        let mut a = app();
        frame(&mut a, Vec::new());
        a.tab_open(at(9, 11));
        a.tab_open(Place { guild: None, channel: Some(50) });
        assert_eq!(a.tab_places(), (vec![at(9, 10), Place { guild: None, channel: Some(50) }, at(9, 11)], 0));
        assert_eq!(a.store.channel, Some(10), "ouverts en arrière-plan");
        frame(&mut a, vec![key(Key::Tab, CTRL)]);
        assert_eq!((a.store.guild, a.store.channel), (None, Some(50)));
        frame(&mut a, vec![key(Key::Tab, Modifiers { shift: true, ..CTRL })]);
        assert_eq!((a.store.guild, a.store.channel), (Some(9), Some(10)));
        frame(&mut a, vec![key(Key::Num3, CTRL)]);
        assert_eq!((a.store.guild, a.store.channel), (Some(9), Some(11)), "Ctrl+3 : le troisième onglet, pas un serveur");
        frame(&mut a, vec![key(Key::W, CTRL)]);
        assert_eq!((a.store.guild, a.store.channel), (None, Some(50)), "fermé : l'onglet voisin");
        frame(&mut a, Vec::new());
        assert_eq!(a.tab_places().0.len(), 2);
        // Navigation ordinaire : l'onglet actif suit ; vers un salon déjà ouvert, son onglet s'active.
        a.go(at(8, 20));
        frame(&mut a, Vec::new());
        assert_eq!(a.tab_places(), (vec![at(9, 10), at(8, 20)], 1));
        a.go(at(9, 10));
        frame(&mut a, Vec::new());
        assert_eq!(a.tab_places(), (vec![at(9, 10), at(8, 20)], 0));
        // Épinglé : il garde son salon.
        a.tab_pin(0, true);
        a.go(at(9, 11));
        frame(&mut a, Vec::new());
        assert_eq!(a.tab_places(), (vec![at(9, 10), at(9, 11), at(8, 20)], 1));
        // Retenus par compte, et repris par une autre session du même compte.
        a.tabs_exit();
        let saved = a.local_prefs();
        assert_eq!(saved.windows.tabs.get(&1).map(|t| t.list.len()), Some(3));
        let mut b = app();
        b.set_local_prefs(saved);
        frame(&mut b, Vec::new());
        assert_eq!(b.tab_places(), (vec![at(9, 10), at(9, 11), at(8, 20)], 0), "repris, sans changer de salon");
        assert_eq!(b.store.channel, Some(10));
        // Un seul onglet : Ctrl+chiffre va de nouveau aux serveurs.
        let mut c = app();
        frame(&mut c, Vec::new());
        frame(&mut c, vec![key(Key::Num3, CTRL)]);
        assert_eq!(c.store.guild, Some(8));
    }

    /// Un message dans un onglet d'arrière-plan ne change que sa pastille :
    /// ni liste mise en page, ni historique demandé.
    #[test]
    fn background_tab_only_gets_a_badge() {
        let mut a = app();
        frame(&mut a, Vec::new());
        a.tab_open(at(9, 11));
        let out = wide_frame(&mut a, Vec::new());
        assert!(find(&out, "hors-sujet, non lu").is_none());
        a.store.apply(Event::MessageCreate(crate::state::model::Message { channel_id: 11, guild_id: Some(9), ..test_message(7 << 22) }));
        let out = wide_frame(&mut a, Vec::new());
        assert!(find(&out, "hors-sujet, non lu").is_some(), "pastille de non-lu sur l'onglet");
        assert!(!a.list.has_view(11), "aucune liste pour l'onglet d'arrière-plan");
        assert!(a.store.chans.get(&11).is_none_or(|c| !c.loading && !c.loaded), "aucun historique demandé");
        assert_eq!(a.store.channel, Some(10));
        // Une mention : le compte.
        let me = (1, "Moi".to_string());
        a.store.apply(Event::MessageCreate(crate::state::model::Message { channel_id: 11, guild_id: Some(9), mentions: vec![me], ..test_message(8 << 22) }));
        let out = wide_frame(&mut a, Vec::new());
        assert!(find(&out, "hors-sujet, 1 mention").is_some());
    }
}
