//! Vue scindée et fenêtres détachées (plugins du lot E2) : d'autres
//! discussions à l'écran en même temps que le salon ouvert.
//!
//! - **Vue scindée** (« Ouvrir à droite ») : deux discussions côte à côte
//!   dans la zone centrale. Le salon ouvert reste la discussion complète
//!   (`chat`), avec sa saisie ; l'autre est dessinée par [`App::with_place`]
//!   (en-tête, liste, pied « Cliquez pour écrire ici »). Un clic dans l'autre
//!   échange les rôles sans rien déplacer à l'écran : le brouillon de chacune
//!   reste dans la saisie par salon, qui n'est pas dessinée deux fois. La
//!   séparation se tire à la souris (`[fenetres] scinde`, en %). Sous
//!   `2 × PANE_MIN` de large, une seule colonne, et la vue revient avec la place.
//! - **Fenêtres détachées** (« Ouvrir dans une fenêtre ») : un viewport
//!   immédiat d'egui par salon, avec en-tête, liste et une saisie simple
//!   (Entrée envoie, réponse comprise ; ni complétion ni pièces jointes).
//!   Place et taille dans `[fenetres.cadres]`. Le viewport immédiat emprunte
//!   l'application : il est dessiné avec la fenêtre principale, et seulement
//!   quand elle l'est (rien au repos).
//! - Une discussion secondaire a ses propres gestes sur les messages
//!   ([`ActionsUi`]) : réponse, modification, barre d'actions.
//! - Les salons ainsi affichés sont dits au filtre d'intérêt de la passerelle
//!   ([`App::panes_publish`]) : leurs événements réveillent, ceux d'un onglet
//!   d'arrière-plan non. Plugins coupés : rien n'est dessiné ni publié.

use super::chat_head::{head_icon_drawn, head_margin, head_row};
use super::msg_actions::ActionsUi;
use super::palette::Command;
use super::util::inline_icon;
use super::App;
use crate::i18n::table::e2::{f as wf, t as wt};
use crate::i18n::{f, t as tx};
use crate::plugins::{self, Id as Plugin};
use crate::state::interest::ALSO_MAX;
use crate::state::nav::Place;
use crate::state::windows::{Frame, SPLIT_MAX, SPLIT_MIN};
use crate::theme::icons::Icon;
use eframe::egui::{self, Id, Key, Rect, RichText, Sense};
use std::time::{Duration, Instant};

/// Largeur minimale d'une discussion de la vue scindée (points).
pub(super) const PANE_MIN: f32 = 300.0;
/// Fenêtres détachées ouvertes à la fois, au plus.
const WINDOWS_MAX: usize = 4;
/// Taille d'une fenêtre détachée jamais placée.
const WINDOW_SIZE: [f32; 2] = [520.0, 640.0];

struct Split {
    other: Place,
    /// L'autre discussion est à droite du salon ouvert.
    right: bool,
    actions: ActionsUi,
}

struct Win {
    place: Place,
    actions: ActionsUi,
    draft: String,
    /// Place lue au moment de l'ouverture : donnée une fois à la fenêtre.
    initial: Option<Frame>,
    /// Place vue à la dernière image.
    frame: Option<Frame>,
    to_bottom: bool,
    focus: bool,
    close: bool,
}

#[derive(Default)]
pub(super) struct PanesUi {
    split: Option<Split>,
    wins: Vec<Win>,
    /// Compte auquel ces discussions appartiennent.
    owner: Option<u64>,
    /// Salons secondaires dessinés à cette image, et ceux déjà publiés.
    visible: Vec<u64>,
    published: Vec<u64>,
    /// Place d'une fenêtre changée, pas encore enregistrée.
    dirty: Option<Instant>,
}

/// Entrée sans modificateur, retirée des événements (Maj+Entrée reste au champ).
fn take_enter(i: &mut egui::InputState) -> bool {
    let before = i.events.len();
    i.events.retain(|e| !matches!(e, egui::Event::Key { key: Key::Enter, pressed: true, modifiers, .. } if modifiers.is_none()));
    i.events.len() != before
}

impl App {
    fn split_on(&self) -> bool {
        plugins::on(Plugin::SplitView)
    }

    fn detach_on(&self) -> bool {
        plugins::on(Plugin::DetachedWindows)
    }

    /// « Ouvrir à droite » est proposé.
    pub(super) fn split_offered(&self) -> bool {
        self.split_on() && !self.mobile_active()
    }

    /// « Ouvrir dans une fenêtre » est proposé.
    pub(super) fn detach_offered(&self) -> bool {
        self.detach_on() && !self.mobile_active()
    }

    /// Salon d'une discussion secondaire possible : ni page, ni forum.
    fn pane_channel(&mut self, place: Place) -> Option<u64> {
        let cid = place.channel?;
        if self.store.is_forum(cid) || !self.place_exists(&place) {
            self.notify(wt::PANE_FORUM);
            return None;
        }
        Some(cid)
    }

    /// Autre discussion de la vue scindée, s'il y en a une.
    pub fn split_other(&self) -> Option<Place> {
        self.panes.split.as_ref().map(|s| s.other)
    }

    /// Ouvre `place` à côté du salon ouvert. Le salon ouvert lui-même : rien.
    pub fn split_open(&mut self, place: Place) {
        if !self.split_offered() || place.channel == self.store.channel {
            return;
        }
        let Some(cid) = self.pane_channel(place) else { return };
        self.store.ensure_history(cid);
        self.panes.owner = self.store.me.as_ref().map(|u| u.id);
        self.panes.split = Some(Split { other: place, right: true, actions: ActionsUi::default() });
        self.ctx.request_repaint();
    }

    pub fn split_close(&mut self) {
        self.panes.split = None;
        self.ctx.request_repaint();
    }

    /// Salons des fenêtres détachées ouvertes.
    pub fn detached(&self) -> Vec<u64> {
        self.panes.wins.iter().filter_map(|w| w.place.channel).collect()
    }

    /// Ouvre `place` dans sa fenêtre ; déjà ouverte, elle passe devant.
    pub fn detach_open(&mut self, place: Place) {
        if !self.detach_offered() {
            return;
        }
        let Some(cid) = self.pane_channel(place) else { return };
        if self.panes.wins.iter().any(|w| w.place.channel == Some(cid)) {
            self.ctx.send_viewport_cmd_to(window_id(cid), egui::ViewportCommand::Focus);
            return;
        }
        if self.panes.wins.len() >= WINDOWS_MAX {
            self.notify(wt::PANE_TOO_MANY);
            return;
        }
        self.store.ensure_history(cid);
        self.panes.owner = self.store.me.as_ref().map(|u| u.id);
        let initial = self.local.prefs.windows.frames.get(&cid).copied();
        self.panes.wins.push(Win { place, actions: ActionsUi::default(), draft: String::new(), initial, frame: None, to_bottom: true, focus: true, close: false });
        self.ctx.request_repaint();
    }

    /// Début de l'image : autre compte, plus aucune discussion secondaire ;
    /// place des fenêtres enregistrée une seconde après le dernier déplacement.
    pub(super) fn panes_begin(&mut self) {
        let p = &mut self.panes;
        p.visible.clear();
        if p.split.is_none() && p.wins.is_empty() && p.dirty.is_none() {
            return;
        }
        let me = self.store.me.as_ref().map(|u| u.id);
        if p.owner != me {
            p.owner = me;
            p.split = None;
            p.wins.clear();
        }
        if p.dirty.is_some_and(|at| at.elapsed() >= Duration::from_secs(1)) {
            self.panes_exit();
        }
    }

    /// Enregistre la place des fenêtres (aussi à la fermeture).
    pub(super) fn panes_exit(&mut self) {
        if self.panes.dirty.take().is_some() && self.local.loaded {
            self.save_local_prefs();
        }
    }

    /// Salons secondaires dessinés à la dernière image.
    pub fn panes_visible(&self) -> &[u64] {
        &self.panes.visible
    }

    /// Dit à la passerelle les salons secondaires à l'écran, quand ils changent.
    pub(super) fn panes_publish(&mut self) {
        let p = &mut self.panes;
        p.visible.truncate(ALSO_MAX);
        if p.visible != p.published {
            p.published.clone_from(&p.visible);
            self.interest.shared().show_also(&p.published);
        }
    }

    /// Dessine une discussion autre que le salon ouvert : le magasin, la
    /// liste, les gestes et la notification sont ceux de `place` le temps de
    /// `body`, puis ceux du salon ouvert reviennent. Si `body` a navigué
    /// (lien vers un salon), la navigation est gardée.
    fn with_place<R>(&mut self, place: Place, actions: &mut ActionsUi, to_bottom: &mut bool, body: impl FnOnce(&mut App) -> R) -> R {
        let Some(cid) = place.channel else { return body(self) };
        // Encore affiché : pas le premier à sortir du cache des salons.
        match self.store.chans.get_mut(&cid) {
            Some(cs) => cs.last_view = Some(Instant::now()),
            None => self.store.ensure_history(cid),
        }
        self.panes.visible.push(cid);
        let guild = std::mem::replace(&mut self.store.guild, place.guild);
        let channel = self.store.channel.replace(cid);
        let current = self.list.swap_current(Some(cid));
        let bottom = std::mem::replace(&mut self.scroll_to_bottom, std::mem::take(to_bottom));
        let toast = self.toast.take();
        let slot = self.list.toast_slot.take();
        std::mem::swap(&mut self.actions, actions);
        super::attachments::set_sensitive(&self.ctx, self.store.sensitive_media(cid));
        let out = body(self);
        std::mem::swap(&mut self.actions, actions);
        *to_bottom = std::mem::replace(&mut self.scroll_to_bottom, bottom);
        if self.toast.is_none() {
            self.toast = toast;
        }
        self.list.toast_slot = slot;
        if self.store.channel == Some(cid) && self.store.guild == place.guild {
            self.store.guild = guild;
            self.store.channel = channel;
            self.list.swap_current(current);
            if let Some(c) = channel {
                super::attachments::set_sensitive(&self.ctx, self.store.sensitive_media(c));
            }
        }
        out
    }

    /// En-tête d'une discussion secondaire ; `true` si sa croix est cliquée.
    fn pane_head(&mut self, ui: &mut egui::Ui, place: Place, cross: bool) -> bool {
        let t = self.theme.clone();
        let (icon, name, guild) = self.place_title(place);
        let mut closed = false;
        egui::Panel::top(Id::unique(("e2-tete", place.channel)))
            .frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(head_margin(&t, t.spacing.chat_head_x)))
            .show_separator_line(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.set_height(head_row(&t));
                    if cross {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            closed = head_icon_drawn(ui, &t, Icon::Close, wt::PANE_CLOSE.get(self.lang), false).clicked();
                            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| pane_title(ui, &t, icon, &name, &guild));
                        });
                    } else {
                        pane_title(ui, &t, icon, &name, &guild);
                    }
                });
            });
        closed
    }

    /// Liste d'une discussion secondaire, ou ce qui en tient lieu.
    fn pane_list(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2)).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Chat);
            if self.store.age_gated(cid) {
                pane_note(ui, &t, wt::PANE_FORUM.get(self.lang));
            } else {
                self.message_list(ui, cid, &t);
            }
        });
        self.message_actions(ui, cid);
    }

    /// Zone centrale : le salon ouvert seul, ou la vue scindée.
    pub(super) fn chat_area(&mut self, ui: &mut egui::Ui) {
        let Some(other) = self.split_shown(ui.available_width()) else { return self.chat(ui) };
        let Some(other_cid) = other.channel else { return self.chat(ui) };
        let t = self.theme.clone();
        let full = ui.available_rect_before_wrap();
        let margin = (PANE_MIN / full.width()).min(0.5);
        let ratio = (f32::from(self.local.prefs.windows.split) / 100.0).clamp(margin, 1.0 - margin);
        let x = (full.left() + full.width() * ratio).round();
        let right = self.panes.split.as_ref().is_some_and(|s| s.right);
        let (left_rect, right_rect) = (full.with_max_x(x), full.with_min_x(x + 1.0));
        let (main_rect, other_rect) = if right { (left_rect, right_rect) } else { (right_rect, left_rect) };

        let mut main = ui.new_child(egui::UiBuilder::new().max_rect(main_rect).id_salt("e2-principale"));
        main.set_clip_rect(main_rect.intersect(ui.clip_rect()));
        self.chat(&mut main);

        let mut pane = ui.new_child(egui::UiBuilder::new().max_rect(other_rect).id_salt("e2-secondaire"));
        pane.set_clip_rect(other_rect.intersect(ui.clip_rect()));
        let mut actions = self.panes.split.as_mut().map(|s| std::mem::take(&mut s.actions)).unwrap_or_default();
        let closed = self.pane_head(&mut pane, other, true);
        // Pied : la saisie est celle du salon ouvert ; ici, de quoi y venir.
        let draft = self.input.has_draft(other_cid);
        let lang = self.lang;
        let foot = egui::Panel::bottom(Id::unique("e2-pied"))
            .frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(t.composer_margin()))
            .show_separator_line(false)
            .show(&mut pane, |ui| {
                let card = egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(t.r_composer()).inner_margin(t.composer_inner()).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    let text = if draft { wt::PANE_DRAFT } else { wt::PANE_WRITE_HERE }.get(lang);
                    ui.add(egui::Label::new(RichText::new(text).color(t.colors.muted)).selectable(false).truncate());
                });
                let resp = ui.interact(card.response.rect, Id::unique("e2-pied-clic"), Sense::click()).on_hover_cursor(egui::CursorIcon::Text);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, wt::PANE_WRITE_HERE.get(lang)));
                resp.clicked()
            })
            .inner;
        let mut to_bottom = false;
        self.with_place(other, &mut actions, &mut to_bottom, |app| app.pane_list(&mut pane, other_cid));
        // Un geste commencé ici (réponse, modification, suppression…) se
        // poursuit dans la discussion complète : celle-ci le devient.
        let began = actions.reply.is_some() || actions.edit.is_some() || actions.confirm_delete.is_some() || actions.picker.is_some() || actions.forward.is_some();
        let clicked = ui.rect_contains_pointer(other_rect) && ui.input(|i| i.pointer.primary_released());

        // Séparation : un filet, une poignée par-dessus les deux discussions.
        ui.painter().vline(x + 0.5, full.y_range(), t.border());
        let top = main_rect.with_max_y(main_rect.top() + 2.0);
        ui.painter().rect_filled(top, 0.0, t.colors.accent);
        let handle = Rect::from_x_y_ranges((x - 3.0)..=(x + 4.0), full.y_range());
        let grip = ui.interact(handle, ui.scope_id().with("e2-separation"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
        if let Some(p) = grip.interact_pointer_pos().filter(|_| grip.dragged()) {
            let pct = ((p.x - full.left()) / full.width() * 100.0).round().clamp(f32::from(SPLIT_MIN), f32::from(SPLIT_MAX)) as u8;
            self.local.prefs.windows.split = pct;
        }
        if grip.drag_stopped() && self.local.loaded {
            self.save_local_prefs();
        }

        if let Some(s) = self.panes.split.as_mut() {
            s.actions = actions;
        }
        if closed {
            self.split_close();
        } else if (foot || began || clicked) && !grip.dragged() && !grip.drag_stopped() {
            self.split_swap();
            if foot {
                ui.memory_mut(|m| m.request_focus(Id::unique("composer")));
            }
        }
    }

    /// L'autre discussion devient le salon ouvert, sans changer de côté à l'écran.
    pub fn split_swap(&mut self) {
        let here = Place { guild: self.store.guild, channel: self.store.channel };
        let Some(s) = self.panes.split.as_mut() else { return };
        let other = std::mem::replace(&mut s.other, here);
        s.right = !s.right;
        std::mem::swap(&mut self.actions, &mut s.actions);
        self.go(other);
        // Chacune garde l'endroit où elle en était.
        self.scroll_to_bottom = false;
        self.list.swap_current(other.channel);
    }

    /// Autre discussion à dessiner à cette image, si la vue scindée tient
    /// dans `width`. Devenue le salon ouvert, ou disparue : la vue se ferme.
    fn split_shown(&mut self, width: f32) -> Option<Place> {
        let other = self.panes.split.as_ref()?.other;
        if !self.split_on() || other.channel == self.store.channel || !self.place_exists(&other) {
            self.panes.split = None;
            return None;
        }
        (width >= 2.0 * PANE_MIN && !self.mobile_active()).then_some(other)
    }

    /// Fin de l'image : chaque discussion détachée dans sa fenêtre.
    pub(super) fn detached_ui(&mut self) {
        if self.panes.wins.is_empty() {
            return;
        }
        if !self.detach_on() {
            self.panes.wins.clear();
            return;
        }
        let mut wins = std::mem::take(&mut self.panes.wins);
        for w in &mut wins {
            self.detached_window(w);
        }
        wins.retain(|w| !w.close);
        // Fenêtres ouvertes pendant cette image.
        wins.append(&mut self.panes.wins);
        self.panes.wins = wins;
    }

    fn detached_window(&mut self, w: &mut Win) {
        let Some(cid) = w.place.channel.filter(|_| self.place_exists(&w.place)) else {
            w.close = true;
            return;
        };
        let (_, name, _) = self.place_title(w.place);
        let mut builder = egui::ViewportBuilder::default().with_title(wf::window_title(self.lang, &name)).with_min_inner_size([340.0, 260.0]);
        builder = match w.initial {
            Some(f) => builder.with_position(f.pos).with_inner_size(f.size),
            None => builder.with_inner_size(WINDOW_SIZE),
        };
        let ctx = self.ctx.clone();
        ctx.show_viewport_immediate(window_id(cid), builder, |ui, class| self.detached_body(ui, class, w, cid, &name));
    }

    fn detached_body(&mut self, ui: &mut egui::Ui, class: egui::ViewportClass, w: &mut Win, cid: u64, name: &str) {
        let t = self.theme.clone();
        if class != egui::ViewportClass::EmbeddedWindow {
            let (close, outer, inner) = ui.input(|i| {
                let v = i.viewport();
                (v.close_requested(), v.outer_rect, v.inner_rect)
            });
            if close || ui.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, Key::W)) {
                w.close = true;
            }
            // Place et taille : retenues dès qu'elles changent, enregistrées plus tard.
            if let (Some(o), Some(i)) = (outer, inner)
                && i.width() >= 100.0
                && i.height() >= 100.0
            {
                let now = Frame { pos: [o.min.x.round(), o.min.y.round()], size: [i.width().round(), i.height().round()] };
                if w.frame != Some(now) {
                    if w.frame.is_some() {
                        self.local.prefs.windows.set_frame(cid, now);
                        self.panes.dirty = Some(Instant::now());
                    }
                    w.frame = Some(now);
                }
            }
            ui.painter().rect_filled(ui.max_rect(), 0.0, t.colors.bg2);
        }
        self.pane_head(ui, w.place, false);
        // Même salon dans la fenêtre principale : une seule liste le dessine.
        if self.store.channel == Some(cid) || self.panes.visible.contains(&cid) {
            egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2)).show(ui, |ui| pane_note(ui, &t, wt::PANE_ELSEWHERE.get(self.lang)));
            return;
        }
        let hint = if w.place.guild.is_none() { f::message_dm(self.lang, name) } else { f::message_channel(self.lang, name) };
        let Win { place, actions, draft, to_bottom, focus, .. } = w;
        self.with_place(*place, actions, to_bottom, |app| {
            app.window_composer(ui, cid, draft, hint, focus);
            app.pane_list(ui, cid);
        });
    }

    /// Saisie simple d'une fenêtre détachée : Entrée envoie, Maj+Entrée va à la ligne.
    fn window_composer(&mut self, ui: &mut egui::Ui, cid: u64, draft: &mut String, hint: String, focus: &mut bool) {
        let t = self.theme.clone();
        let lang = self.lang;
        let id = Id::unique(("e2-saisie", cid));
        let blocked = self.store.send_block(cid).is_some();
        let enter = !blocked && ui.memory(|m| m.has_focus(id)) && ui.input_mut(take_enter);
        egui::Panel::bottom(Id::unique(("e2-saisie-pied", cid)))
            .frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(t.composer_margin()))
            .show_separator_line(false)
            .show(ui, |ui| {
                self.sending_list(ui, cid);
                self.unsent_list(ui, cid, !blocked);
                let replying = self.reply_bar(ui, cid, id);
                let mut corners = t.r_composer();
                if replying {
                    (corners.nw, corners.ne) = (0, 0);
                }
                egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(corners).inner_margin(t.composer_inner()).show(ui, |ui| {
                    if blocked {
                        ui.set_min_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(tx::NO_PERMISSION_TO_SEND.get(lang)).color(t.colors.muted)).wrap());
                        return;
                    }
                    egui::ScrollArea::vertical().id_salt(("e2-saisie-defilement", cid)).max_height(ui.text_style_height(&egui::TextStyle::Body) * 8.0).show(ui, |ui| {
                        let edit = egui::TextEdit::multiline(draft).id(id).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY).hint_text(RichText::new(hint));
                        let resp = ui.add(edit).on_hover_text(wt::PANE_SEND_TIP.get(lang));
                        if std::mem::take(focus) {
                            resp.request_focus();
                        }
                    });
                });
            });
        if enter {
            self.window_send(cid, draft);
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// Envoi demandé par Entrée dans une fenêtre détachée (mêmes refus que la saisie).
    fn window_send(&mut self, cid: u64, draft: &mut String) {
        let text = draft.trim().to_string();
        if text.is_empty() {
            return;
        }
        if text.chars().count() > super::complete::MESSAGE_MAX {
            self.notify(tx::MESSAGE_TOO_LONG);
        } else if self.store.slowmode_left(cid, self.store.now()).is_some() || !self.send_text(cid, text) {
            self.notify(tx::CANNOT_SEND);
        } else {
            draft.clear();
            self.scroll_to_bottom = true;
        }
    }

    /// Actions des volets dans la palette de commandes.
    pub(super) fn panes_commands(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        let here = Place { guild: self.store.guild, channel: self.store.channel };
        if self.panes.split.is_some() {
            out.push(Command::new(wt::G_VIEW, wt::CMD_SPLIT_CLOSE.get(lang), |app| app.split_close()));
        }
        if here.channel.is_some() && self.detach_offered() {
            out.push(Command::new(wt::G_VIEW, wt::OPEN_WINDOW.get(lang), move |app| app.detach_open(here)));
        }
    }
}

fn window_id(cid: u64) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(("ferrocord-salon", cid))
}

fn pane_title(ui: &mut egui::Ui, t: &crate::theme::Tokens, icon: Option<Icon>, name: &str, guild: &str) {
    if let Some(icon) = icon {
        inline_icon(ui, icon, t.typography.body, t.colors.muted);
    }
    ui.add(egui::Label::new(RichText::new(name).color(t.colors.text_strong).strong()).selectable(false).truncate());
    if !guild.is_empty() && ui.available_width() > 80.0 {
        ui.add(egui::Label::new(RichText::new(guild).color(t.colors.muted).size(t.typography.small)).selectable(false).truncate());
    }
}

fn pane_note(ui: &mut egui::Ui, t: &crate::theme::Tokens, text: &str) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(text).color(t.colors.muted));
    });
}

impl App {
    /// Aperçu hors ligne des plugins « fenêtres » (`--scene onglets`, …).
    #[doc(hidden)]
    pub fn windows_preview(&mut self, which: &str, places: &[(Option<u64>, u64)]) {
        let places: Vec<Place> = places.iter().map(|(guild, c)| Place { guild: *guild, channel: Some(*c) }).collect();
        match which {
            "onglets" => {
                for p in &places {
                    self.tab_open(*p);
                }
                let last = self.tab_places().0.len().saturating_sub(1);
                self.tab_pin(last, true);
            }
            "zoom" => {
                if let Some(c) = self.store.channel {
                    self.set_channel_zoom(c, 150);
                }
            }
            "palette" => {
                self.open_palette();
                self.palette_type("th");
            }
            "scinde" => {
                if let Some(p) = places.first() {
                    self.split_open(*p);
                }
            }
            "detache" => {
                // Dans la capture : la fenêtre détachée est incrustée dans la principale.
                self.ctx.set_embed_viewports(true);
                if let Some(p) = places.first() {
                    self.detach_open(*p);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, test_message};
    use super::super::tests::find;
    use super::*;
    use crate::state::interest::Verdict;
    use crate::state::Event;

    const AT11: Place = Place { guild: Some(9), channel: Some(11) };

    fn sized(a: &mut App, width: f32, events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = a.ctx.clone();
        ctx.enable_accesskit();
        a.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 760.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| a.ui_pass(ui));
        out.textures_delta.clear();
        out
    }

    fn message(id: u64, channel_id: u64) -> Event {
        Event::MessageCreate(crate::state::model::Message { channel_id, guild_id: Some(9), ..test_message(id << 22) })
    }

    fn click_at(a: &mut App, width: f32, at: egui::Pos2) {
        let b = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        sized(a, width, vec![egui::Event::PointerMoved(at)]);
        sized(a, width, vec![b(true)]);
        sized(a, width, vec![b(false)]);
    }

    #[test]
    fn split_shows_two_lists_and_a_click_swaps_them() {
        let mut a = app();
        sized(&mut a, 1500.0, Vec::new());
        assert!(a.panes_visible().is_empty() && !a.list.has_view(11));
        a.split_open(AT11);
        a.store.apply(message(5, 11));
        let out = sized(&mut a, 1500.0, Vec::new());
        assert!(a.list.has_view(10) && a.list.has_view(11), "les deux listes sont dessinées : {:?} {:?} {:?} {:?}", a.list.has_view(10), a.list.has_view(11), a.split_other(), a.panes_visible());
        assert_eq!(a.panes_visible(), [11]);
        assert_eq!(a.store.channel, Some(10), "le salon ouvert ne change pas");
        let foot = find(&out, "Cliquez pour écrire ici").expect("pied de la discussion secondaire");
        let head = find(&out, "Fermer cette discussion").expect("croix de la discussion secondaire");
        assert!(foot.center().x > 750.0, "à droite : {foot:?}");
        // Le filtre de la passerelle : le salon d'à côté réveille, un autre non.
        sized(&mut a, 1500.0, Vec::new());
        let interest = a.interest.shared();
        assert_eq!(interest.verdict(&Event::TypingStart { channel_id: 11, user_id: 3, name: None }), Verdict::Wake);
        assert_ne!(interest.verdict(&Event::TypingStart { channel_id: 20, user_id: 3, name: None }), Verdict::Wake);
        // Clic sur le pied : la discussion de droite devient le salon ouvert, sans changer de côté.
        click_at(&mut a, 1500.0, foot.center());
        assert_eq!(a.store.channel, Some(11));
        assert_eq!(a.split_other().and_then(|p| p.channel), Some(10));
        let out = sized(&mut a, 1500.0, Vec::new());
        let foot = find(&out, "Cliquez pour écrire ici").expect("pied de l'autre discussion");
        assert!(foot.center().x < 750.0, "l'ancienne discussion active reste à gauche : {foot:?}");
        assert_eq!(a.panes_visible(), [10]);
        // Fenêtre étroite : une seule colonne, la vue revient avec la place.
        let out = sized(&mut a, 900.0, Vec::new());
        assert!(find(&out, "Cliquez pour écrire ici").is_none() && a.panes_visible().is_empty());
        assert!(a.split_other().is_some());
        let out = sized(&mut a, 1500.0, Vec::new());
        assert!(find(&out, "Cliquez pour écrire ici").is_some());
        // La croix ferme la vue ; plus rien n'est publié.
        let _ = head;
        let cross = find(&out, "Fermer cette discussion").expect("croix");
        click_at(&mut a, 1500.0, cross.center());
        assert!(a.split_other().is_none());
        sized(&mut a, 1500.0, Vec::new());
        sized(&mut a, 1500.0, Vec::new());
        assert_ne!(a.interest.shared().verdict(&Event::TypingStart { channel_id: 10, user_id: 3, name: None }), Verdict::Wake);
    }

    #[test]
    fn layout_is_the_same_with_the_plugins_off_or_unused() {
        let texts = |out: &egui::FullOutput| -> Vec<(String, [i32; 4])> {
            let update = out.platform_output.accesskit_update.as_ref().expect("accesskit");
            let mut v: Vec<_> = update
                .nodes
                .iter()
                .filter_map(|(_, n)| {
                    let b = n.bounds()?;
                    Some((n.label()?.to_string(), [b.x0 as i32, b.y0 as i32, b.x1 as i32, b.y1 as i32]))
                })
                .collect();
            v.sort();
            v
        };
        let mut off = app();
        off.set_plugin(Plugin::SplitView, false);
        off.set_plugin(Plugin::DetachedWindows, false);
        off.split_open(AT11);
        off.detach_open(AT11);
        assert!(off.split_other().is_none() && off.detached().is_empty(), "plugins coupés : rien ne s'ouvre");
        sized(&mut off, 1500.0, Vec::new());
        let before = texts(&sized(&mut off, 1500.0, Vec::new()));
        assert!(off.panes_visible().is_empty());
        let mut on = app();
        sized(&mut on, 1500.0, Vec::new());
        let after = texts(&sized(&mut on, 1500.0, Vec::new()));
        assert_eq!(before, after);
    }

    #[test]
    fn two_visible_conversations_ask_for_no_frame_at_rest() {
        let mut a = app();
        // Historiques arrivés (messages anciens : aucune heure relative à tenir à jour).
        for c in [10, 11, 20] {
            let m = crate::state::model::Message { channel_id: c, guild_id: Some(9), ..test_message(c) };
            a.store.apply(Event::History { channel_id: c, before: None, messages: vec![m] });
        }
        a.split_open(AT11);
        a.detach_open(Place { guild: Some(8), channel: Some(20) });
        let ctx = a.ctx.clone();
        let mut time = 0.0;
        for i in 0..300 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1500.0, 760.0))), time: Some(time), ..Default::default() };
            time += 1.0 / 60.0;
            // Le champ de la fenêtre détachée a le clavier à l'ouverture : son curseur
            // clignote, comme celui de toute saisie. Le repos se juge sans lui.
            if i == 10 {
                ctx.memory_mut(|m| {
                    if let Some(id) = m.focused() {
                        m.surrender_focus(id);
                    }
                });
            }
            let mut out = ctx.run_ui(input, |ui| {
                a.logic_pass();
                a.ui_pass(ui);
            });
            out.textures_delta.clear();
            if i >= 240 {
                for (id, v) in &out.viewport_output {
                    assert_eq!(v.repaint_delay, Duration::MAX, "image {i}, fenêtre {id:?} : réveil demandé par {:?}", ctx.repaint_causes());
                }
            }
        }
        assert_eq!(a.panes_visible(), [11, 20], "vue scindée et fenêtre détachée dessinées");
    }

    #[test]
    fn detached_window_has_its_list_and_sends_on_enter() {
        let mut a = app();
        sized(&mut a, 1500.0, Vec::new());
        a.detach_open(AT11);
        a.detach_open(AT11);
        assert_eq!(a.detached(), [11], "une fenêtre par salon");
        a.store.apply(message(5, 11));
        let out = sized(&mut a, 1500.0, Vec::new());
        assert!(a.list.has_view(11) && a.panes_visible() == [11]);
        assert_eq!(a.store.channel, Some(10));
        assert!(find(&out, "Envoyer un message dans #hors-sujet").is_some() || find(&out, "hors-sujet").is_some());
        // Le champ a le clavier à l'ouverture : le texte tapé y va, Entrée l'envoie.
        sized(&mut a, 1500.0, vec![egui::Event::Text("bonjour".into())]);
        let sent = |a: &App| a.store.sending.iter().filter(|s| s.0 == 11).count();
        assert_eq!(sent(&a), 0, "rien n'est envoyé sans Entrée");
        sized(&mut a, 1500.0, vec![super::super::keys::tests::key(Key::Enter, egui::Modifiers::NONE)]);
        assert_eq!(sent(&a), 1, "Entrée envoie dans le salon de la fenêtre");
        assert_eq!(a.store.sending.iter().filter(|s| s.0 == 10).count(), 0);
        // Le salon ouvert dans la fenêtre principale : une seule liste le dessine.
        a.open(Some(9), 11);
        sized(&mut a, 1500.0, Vec::new());
        assert!(a.panes_visible().is_empty());
        // Autre compte ou plugin coupé : les fenêtres se ferment.
        a.set_plugin(Plugin::DetachedWindows, false);
        sized(&mut a, 1500.0, Vec::new());
        assert!(a.detached().is_empty());
    }
}
