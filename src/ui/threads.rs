//! Fils de discussion dans l'interface (lot T10) : lignes sous leur salon
//! parent, en-tête d'un fil ouvert, résumé « N messages » sous le message
//! de départ, fenêtre « Créer un fil ». Le fil lui-même s'affiche comme un
//! salon (`chat.rs`) ; la vue forum est dans `forum.rs`. Bouton « Fils » de
//! l'en-tête et son panneau (fils actifs, fils archivés chargés à la
//! demande), menu d'une ligne de fil (phase 10, lot S).

use super::channels::RowIcon;
use super::msg_actions::{Acts, Hit};
use super::util::inline_icon;
use super::App;
use crate::i18n::table::ff::{f as ff, t as ft};
use crate::i18n::table::threads::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::model::Message;
use crate::state::threads::{ThreadAction, ThreadKind, MAX_NAME};
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Id, Key, RichText};
use std::collections::HashSet;

/// Fils non rejoints montrés au plus sous un salon (les plus récemment
/// actifs) ; les fils rejoints et le fil ouvert le sont toujours.
const MAX_OTHER_ROWS: usize = 10;

/// État de l'interface des fils (appartient à `App`).
#[derive(Default)]
pub(super) struct ThreadsUi {
    /// Salons dont les fils sont repliés.
    collapsed: HashSet<u64>,
    /// « Créer un fil » ouvert : (salon, message, nom saisi).
    create: Option<(u64, u64, String)>,
    /// Vue forum (filtres, tri, publication en cours).
    pub forum: super::forum::ForumUi,
    /// Onglet « Archivés » du panneau de l'en-tête (sinon : « Actifs »).
    panel_archived: bool,
    /// Case « Fil privé » de la fenêtre de création (lot FF2).
    create_private: bool,
    /// Fenêtre « Membres du fil » ouverte pour ce fil (lot FF2).
    members_win: Option<u64>,
}

impl ThreadsUi {
    pub(super) fn toggle(&mut self, parent: u64) {
        if !self.collapsed.remove(&parent) {
            self.collapsed.insert(parent);
        }
    }

    pub(super) fn start_create(&mut self, channel_id: u64, message_id: u64, name: String) {
        self.create = Some((channel_id, message_id, name));
    }

    pub(super) fn set_panel_archived(&mut self, archived: bool) {
        self.panel_archived = archived;
    }
}

/// Ce que l'utilisateur a fait sur les lignes des fils d'un salon.
#[derive(Default)]
pub(super) struct Rows {
    pub clicked: Option<u64>,
    pub toggled: Option<u64>,
    pub pick: Option<ThreadPick>,
}

/// Choix du menu d'une ligne de fil.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ThreadPick {
    MarkRead(u64),
    Join(u64, bool),
    Archive(u64, bool),
    CopyLink { guild: u64, thread: u64 },
    /// Verrouiller (`true`) ou déverrouiller (lot FF2).
    Lock(u64, bool),
    /// Ouvrir la fenêtre « Membres du fil » (lot FF2).
    Members(u64),
}

/// Ce que l'utilisateur a fait dans le panneau « Fils ».
enum PanelAct {
    Open(u64),
    /// Page suivante (ou première page) des fils archivés.
    More,
}

fn action_word(lang: Lang, a: ThreadAction) -> &'static str {
    match a {
        ThreadAction::Join => tx::ACTION_JOIN.get(lang),
        ThreadAction::Leave => tx::ACTION_LEAVE.get(lang),
        ThreadAction::Archive => tx::ACTION_ARCHIVE.get(lang),
        ThreadAction::Create => tx::ACTION_CREATE.get(lang),
        ThreadAction::Post => tx::ACTION_POST.get(lang),
        ThreadAction::Archived | ThreadAction::Starter => tx::ACTION_ARCHIVED.get(lang),
        ThreadAction::Notify => crate::i18n::table::lotn::t::ACTION_THREAD_NOTIFY.get(lang),
        ThreadAction::Lock => ft::ACTION_LOCK.get(lang),
        ThreadAction::Members => ft::ACTION_THREAD_MEMBERS.get(lang),
    }
}

impl App {
    /// Fils de `parent` sous sa ligne. Salon ordinaire : fils actifs visibles
    /// (repliables) ; forum : seulement les publications rejointes (les autres
    /// sont dans la vue forum).
    pub(super) fn thread_rows(&self, ui: &mut egui::Ui, parent: u64, forum: bool) -> Rows {
        let mut out = Rows::default();
        let s = &self.store;
        let ids = s.threads.children(parent);
        if ids.is_empty() {
            return out;
        }
        let open = s.channel;
        let shown = |id: &u64| s.threads.get(*id).is_some_and(|t| !s.is_hidden(t.id) && (Some(t.id) == open || (!t.archived && (t.joined || !forum))));
        let visible: Vec<u64> = ids.iter().copied().filter(shown).collect();
        if visible.is_empty() {
            return out;
        }
        let collapsed = self.threads.collapsed.contains(&parent);
        let t = &self.theme;
        let lang = self.lang;
        // Lien discret pour replier ou déplier les fils de ce salon.
        let label = if collapsed { format!("{} ({})", tx::SHOW_THREADS.get(lang), visible.len()) } else { tx::HIDE_THREADS.get(lang).to_string() };
        let head = ui.horizontal(|ui| {
            ui.add_space(t.spacing.item_x * 2.0);
            ui.add(egui::Label::new(RichText::new(label).small().color(t.colors.faint)).sense(egui::Sense::click()).selectable(false))
        });
        if head.inner.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            out.toggled = Some(parent);
        }
        let mut others = 0;
        for id in visible {
            let Some(th) = s.threads.get(id) else { continue };
            let always = th.joined || Some(id) == open || s.unread().contains(&id);
            if collapsed && Some(id) != open {
                continue;
            }
            if !always {
                others += 1;
                if others > MAX_OTHER_ROWS {
                    continue;
                }
            }
            let row = self.channel_row_ex(ui, RowIcon::Drawn(Icon::Thread), &th.name, id, true, th.joined);
            if row.clicked() {
                out.clicked = Some(id);
            }
            row.context_menu(|ui| {
                if let Some(p) = self.thread_menu(ui, id) {
                    out.pick = Some(p);
                    ui.close();
                }
            });
        }
        out
    }

    /// Menu d'une ligne de fil : marquer comme lu, rejoindre ou quitter, archiver, copier le lien.
    fn thread_menu(&self, ui: &mut egui::Ui, id: u64) -> Option<ThreadPick> {
        let lang = self.lang;
        let th = self.store.threads.get(id)?;
        let mut pick = None;
        if ui.add_enabled_ui(self.store.unread().contains(&id), |ui| crate::ui::menu::item(ui, &self.theme, tx::MARK_READ.get(lang), Some(Icon::Check), false)).inner.clicked() {
            pick = Some(ThreadPick::MarkRead(id));
        }
        crate::ui::menu::separator(ui, &self.theme);
        let busy = |a| self.store.threads.pending(a, id);
        let (text, a) = if th.joined { (tx::LEAVE, ThreadAction::Leave) } else { (tx::JOIN, ThreadAction::Join) };
        if ui.add_enabled_ui(!busy(a), |ui| crate::ui::menu::item(ui, &self.theme, text.get(lang), Some(if th.joined { Icon::Logout } else { Icon::ArrowRight }), false)).inner.clicked() {
            pick = Some(ThreadPick::Join(id, !th.joined));
        }
        if self.store.can_archive_thread(id) {
            let text = if th.archived { tx::UNARCHIVE } else { tx::ARCHIVE };
            if ui.add_enabled_ui(!busy(ThreadAction::Archive), |ui| crate::ui::menu::item(ui, &self.theme, text.get(lang), Some(Icon::Folder), false)).inner.clicked() {
                pick = Some(ThreadPick::Archive(id, !th.archived));
            }
        }
        if self.store.can_lock_thread(id) {
            let text = if th.locked { ft::THREAD_UNLOCK } else { ft::THREAD_LOCK };
            if ui.add_enabled_ui(!busy(ThreadAction::Lock), |ui| crate::ui::menu::item(ui, &self.theme, text.get(lang), Some(Icon::Lock), false)).inner.clicked() {
                pick = Some(ThreadPick::Lock(id, !th.locked));
            }
        }
        if crate::ui::menu::item(ui, &self.theme, ft::THREAD_MEMBERS.get(lang), Some(Icon::Members), false).clicked() {
            pick = Some(ThreadPick::Members(id));
        }
        if crate::ui::menu::item(ui, &self.theme, tx::COPY_LINK.get(lang), Some(Icon::Link), false).clicked() {
            pick = Some(ThreadPick::CopyLink { guild: th.guild_id, thread: id });
        }
        pick
    }

    pub(super) fn apply_thread_pick(&mut self, pick: ThreadPick) {
        match pick {
            ThreadPick::MarkRead(id) => self.store.mark_read(id),
            ThreadPick::Join(id, join) => {
                self.store.set_thread_joined(id, join);
            }
            ThreadPick::Archive(id, archived) => {
                self.store.set_thread_archived(id, archived);
            }
            ThreadPick::CopyLink { guild, thread } => {
                self.ctx.copy_text(super::channel_settings::channel_link(guild, thread));
                self.notify(crate::i18n::table::chadmin::t::LINK_COPIED);
            }
            ThreadPick::Lock(id, locked) => {
                self.store.set_thread_locked(id, locked);
            }
            ThreadPick::Members(id) => {
                self.show_thread_members(id);
            }
        }
    }

    /// Ouvre la fenêtre « Membres du fil » : une lecture de la liste, par ouverture.
    pub fn show_thread_members(&mut self, thread: u64) -> bool {
        if self.store.threads.get(thread).is_none() {
            return false;
        }
        self.store.load_thread_members(thread);
        self.threads.members_win = Some(thread);
        self.ctx.request_repaint();
        true
    }

    /// Aperçu hors ligne : la fenêtre des membres d'un fil, avec une liste donnée.
    pub fn thread_members_preview(&mut self, thread: u64, members: Vec<(u64, String)>) {
        if self.show_thread_members(thread) {
            self.store.take_commands();
            self.store.apply(crate::state::Event::Threads(crate::state::threads::ThreadEvent::MemberList { thread_id: thread, members }));
        }
    }

    /// Fenêtre « Membres du fil » (lot FF2) : la première page de ses
    /// membres ; un clic ouvre la carte de profil.
    pub(super) fn thread_members_window(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.threads.members_win else { return };
        let Some(th) = self.store.threads.get(id).cloned() else {
            self.threads.members_win = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let list: Option<Option<Vec<(u64, String)>>> = self.store.threads.member_list(id).map(|l| l.map(<[_]>::to_vec));
        let mut clicked = None;
        let out = crate::ui::widgets::sheet(ui.ctx(), &t, Id::unique("membres-du-fil"), crate::ui::mobile::card_width(ui.ctx(), &t, 1.0), &ff::thread_members_title(lang, &th.name), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            match &list {
                // Lecture refusée : l'échec a été annoncé, il ne reste que le compte.
                None => {
                    ui.label(RichText::new(p::members(lang, u64::from(th.member_count))).color(t.colors.muted));
                }
                Some(None) => {
                    ui.spinner();
                }
                Some(Some(members)) => {
                    ui.label(RichText::new(p::members(lang, u64::from(th.member_count).max(members.len() as u64))).small().color(t.colors.muted));
                    egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.5).show(ui, |ui| {
                        for (user, name) in members {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), egui::Sense::click());
                            if resp.hovered() || resp.has_focus() {
                                ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
                            }
                            crate::ui::keys::focus_ring(ui, &t, &resp);
                            let font = egui::TextStyle::Body.resolve(ui.style());
                            let galley = ui.painter().layout_no_wrap(name.clone(), font, t.colors.text_strong);
                            let pos = egui::pos2(rect.left() + t.spacing.item_x, rect.top() + Tokens::centered_top(rect.height(), galley.size().y));
                            ui.painter().with_clip_rect(rect).galley(pos, galley, t.colors.text_strong);
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                clicked = Some((*user, rect.right_top()));
                            }
                        }
                    });
                }
            }
        });
        if let Some((user, at)) = clicked {
            self.open_profile(user, Some(th.guild_id), at);
        }
        if out.closed {
            self.threads.members_win = None;
            self.store.threads.forget_members();
        }
    }

    /// Bouton « Fils » de l'en-tête et son panneau : fils actifs et archivés
    /// du salon (dans un fil : ceux de son salon parent).
    pub(super) fn threads_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        let parent = self.store.threads.get(cid).map_or(cid, |th| th.parent_id);
        if !self.store.lists_threads(parent) {
            return;
        }
        let t = self.theme.clone();
        let id = super::chat_head::threads_popup_id();
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let resp = super::chat_head::head_icon_drawn(ui, &t, Icon::Thread, crate::i18n::table::chadmin::t::THREADS_TIP.get(self.lang), open);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.2);
        let mut act = None;
        egui::Popup::from_toggle_button_response(&resp).id(id).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(t.window_frame()).width(width).show(|ui| {
            ui.set_width(width);
            act = self.threads_panel(ui, &t, parent);
        });
        match act {
            Some(PanelAct::Open(thread)) => {
                egui::Popup::close_id(ui.ctx(), id);
                self.open_channel(thread);
            }
            Some(PanelAct::More) => {
                self.store.load_archived_threads(parent);
            }
            None => {}
        }
    }

    fn threads_panel(&mut self, ui: &mut egui::Ui, t: &Tokens, parent: u64) -> Option<PanelAct> {
        let lang = self.lang;
        let mut act = None;
        ui.horizontal(|ui| {
            inline_icon(ui, Icon::Thread, t.typography.title, t.colors.muted);
            ui.label(RichText::new(tx::PANEL_TITLE.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
            ui.add_space(t.spacing.item_x);
            for (archived, text) in [(false, tx::TAB_ACTIVE), (true, tx::TAB_ARCHIVED)] {
                if ui.selectable_label(self.threads.panel_archived == archived, text.get(lang)).clicked() {
                    self.threads.panel_archived = archived;
                }
            }
        });
        ui.separator();
        let archived = self.threads.panel_archived;
        let s = &self.store;
        let archive = s.threads.archive(parent).cloned();
        // Onglet « Archivés » ouvert pour la première fois : sa première page (le geste est le clic sur l'onglet).
        if archived && archive.is_none() {
            act = Some(PanelAct::More);
        }
        let ids: Vec<u64> = s.threads.children(parent).iter().copied().filter(|id| s.threads.get(*id).is_some_and(|th| th.archived == archived) && !s.is_hidden(*id)).collect();
        let loading = archived && archive.as_ref().is_none_or(|a| a.loading);
        // Hauteur minimale dès qu'il y a de quoi défiler : sans elle, la liste
        // resterait à la hauteur de l'image précédente (page reçue après l'ouverture).
        let max = ui.ctx().content_rect().height() * 0.5;
        let wanted = (ids.len() as f32 * (t.spacing.member_row + ui.spacing().item_spacing.y)).min(max);
        egui::ScrollArea::vertical().max_height(max).min_scrolled_height(wanted).show(ui, |ui| {
            if ids.is_empty() && !loading {
                let text = if archived { tx::NO_ARCHIVED } else { tx::NO_ACTIVE };
                ui.add_space(t.spacing.item_y);
                ui.label(RichText::new(text.get(lang)).color(t.colors.muted));
                ui.add_space(t.spacing.item_y);
            }
            for id in ids {
                let Some(th) = s.threads.get(id) else { continue };
                if thread_card(ui, t, lang, th, s.unread().contains(&id), s.channel == Some(id)).clicked() {
                    act = Some(PanelAct::Open(id));
                }
            }
            if !archived {
                return;
            }
            match archive {
                Some(a) if a.error.is_some() => {
                    ui.label(RichText::new(tx::LOAD_FAILED.get(lang)).small().color(t.colors.danger));
                    if ui.button(tx::RETRY.get(lang)).clicked() {
                        act = Some(PanelAct::More);
                    }
                }
                Some(a) if a.more && !a.loading => {
                    if ui.button(tx::LOAD_MORE.get(lang)).clicked() {
                        act = Some(PanelAct::More);
                    }
                }
                _ if loading => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(RichText::new(tx::LOADING_THREADS.get(lang)).small().color(t.colors.muted));
                    });
                }
                _ => {}
            }
        });
        act
    }

    /// En-tête d'un fil ouvert : parent (retour), état, rejoindre ou quitter, archiver.
    pub(super) fn thread_head(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let lang = self.lang;
        let Some(th) = self.store.threads.get(cid).cloned() else { return };
        let parent = self.store.channel_name(th.parent_id).unwrap_or_default().to_string();
        let mut go_parent = false;
        let (mut join, mut archive) = (None, None);
        let mut notify = None;
        let (mut lock, mut members) = (None, false);
        ui.horizontal(|ui| {
            let r = ui.add(egui::Label::new(RichText::new(f::in_parent(lang, &parent)).small().color(t.colors.link)).sense(egui::Sense::click()));
            go_parent = r.on_hover_text(tx::BACK_TO_PARENT.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            for (on, text) in [(th.kind == ThreadKind::Private, tx::PRIVATE), (th.archived, tx::ARCHIVED), (th.locked, tx::LOCKED)] {
                if on {
                    ui.label(RichText::new(text.get(lang)).small().color(t.colors.faint));
                }
            }
            // Le compte des membres ouvre leur liste (lot FF2).
            let count = ui.add(egui::Label::new(RichText::new(p::members(lang, u64::from(th.member_count))).small().color(t.colors.faint)).sense(egui::Sense::click()).selectable(false));
            members = count.on_hover_text(ft::THREAD_MEMBERS.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            let busy = |a| self.store.threads.pending(a, cid);
            let (text, a) = if th.joined { (tx::LEAVE, ThreadAction::Leave) } else { (tx::JOIN, ThreadAction::Join) };
            if ui.add_enabled(!busy(a), egui::Button::new(RichText::new(text.get(lang)).small())).clicked() {
                join = Some(!th.joined);
            }
            // Réglage de notification du fil (lot N) : réservé à ses membres.
            if th.joined {
                use crate::i18n::table::lotn::t as xt;
                use crate::i18n::table::s::t as st;
                use crate::state::usersettings::notif::Level;
                let now = self.store.threads.notif(cid);
                let resp = ui.add_enabled(!busy(ThreadAction::Notify), egui::Button::new(RichText::new(xt::THREAD_NOTIFY.get(lang)).small())).on_hover_text(xt::THREAD_NOTIFY_TIP.get(lang));
                egui::Popup::menu(&resp).id(thread_notif_popup_id()).show(|ui| {
                    for (l, text) in [(Level::All, st::LEVEL_ALL), (Level::Mentions, st::LEVEL_MENTIONS), (Level::Nothing, st::LEVEL_NOTHING)] {
                        if crate::ui::menu::radio(ui, &t, text.get(lang), now == Some(l)).clicked() {
                            notify = Some(l);
                            ui.close();
                        }
                    }
                    if now.is_none() {
                        crate::ui::menu::separator(ui, &t);
                        ui.label(RichText::new(xt::THREAD_DEFAULT.get(lang)).small().color(t.colors.faint));
                    }
                });
            }
            if self.store.can_archive_thread(cid) {
                let text = if th.archived { tx::UNARCHIVE } else { tx::ARCHIVE };
                if ui.add_enabled(!busy(ThreadAction::Archive), egui::Button::new(RichText::new(text.get(lang)).small())).clicked() {
                    archive = Some(!th.archived);
                }
            }
            if self.store.can_lock_thread(cid) {
                let text = if th.locked { ft::THREAD_UNLOCK } else { ft::THREAD_LOCK };
                if ui.add_enabled(!busy(ThreadAction::Lock), egui::Button::new(RichText::new(text.get(lang)).small())).clicked() {
                    lock = Some(!th.locked);
                }
            }
        });
        if go_parent {
            self.open_channel(th.parent_id);
        }
        if let Some(j) = join {
            self.store.set_thread_joined(cid, j);
        }
        if let Some(a) = archive {
            self.store.set_thread_archived(cid, a);
        }
        if let Some(l) = notify {
            self.store.set_thread_notif(cid, l);
        }
        if let Some(l) = lock {
            self.store.set_thread_locked(cid, l);
        }
        if members {
            self.show_thread_members(cid);
        }
    }

    /// Échec d'une action sur un fil : annoncé une fois.
    pub(super) fn thread_failures(&mut self) {
        if let Some((action, problem)) = self.store.take_thread_failure() {
            let reason = super::words::problem(self.lang, &problem, self.h24());
            self.notify(super::ToastText::Text(f::failed(self.lang, action_word(self.lang, action), reason)));
        }
    }

    /// « Créer un fil » du menu « + » de la saisie : un fil sans message
    /// d'origine (message 0), dans la même fenêtre.
    pub(super) fn open_thread_creator(&mut self, cid: u64) {
        if self.store.can_create_thread(cid) {
            self.threads.create = Some((cid, 0, String::new()));
            self.threads.create_private = false;
        }
    }

    /// Fenêtre « Créer un fil » depuis un message.
    pub(super) fn create_thread_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((cid, mid, name)) = self.threads.create.as_mut() else { return };
        let (cid, mid) = (*cid, *mid);
        let t = &self.theme;
        let lang = self.lang;
        let (mut yes, mut no, mut entered) = (false, false, false);
        let ok = !name.trim().is_empty();
        // Fil privé (lot FF2) : seulement sans message d'origine, et si j'en ai le droit.
        let may_private = mid == 0 && self.store.can_create_private_thread(cid);
        let private = &mut self.threads.create_private;
        *private &= may_private;
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            t,
            Id::unique("creer-fil"),
            crate::ui::mobile::card_width(ui.ctx(), t, 1.0),
            tx::CREATE_THREAD.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, t);
                let field = ui.add(egui::TextEdit::singleline(name).char_limit(MAX_NAME).hint_text(tx::THREAD_NAME_HINT.get(lang)).desired_width(f32::INFINITY));
                if !field.has_focus() && name.is_empty() {
                    field.request_focus();
                }
                entered = !name.trim().is_empty() && field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if may_private {
                    ui.add_space(t.spacing.item_y);
                    ui.checkbox(private, ft::THREAD_PRIVATE.get(lang));
                    ui.label(RichText::new(ft::THREAD_PRIVATE_NOTE.get(lang)).small().color(t.colors.muted));
                }
            },
            |ui| {
                use crate::ui::widgets::{button, Kind};
                yes = ui.add_enabled_ui(ok, |ui| button(ui, t, Kind::Primary, tx::CREATE.get(lang))).inner.clicked();
                no = button(ui, t, Kind::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        yes |= entered;
        if yes {
            let name = name.clone();
            let sent = if self.threads.create_private { self.store.create_private_thread(cid, &name) } else { self.store.create_thread(cid, mid, &name) };
            if sent {
                self.threads.create = None;
            }
        } else if no || modal.closed {
            self.threads.create = None;
        }
    }
}

/// Ligne d'un fil dans le panneau de l'en-tête : nom, puis nombre de
/// messages et état (rejoint, verrouillé) en petit.
fn thread_card(ui: &mut egui::Ui, t: &Tokens, lang: Lang, th: &crate::state::threads::Thread, unread: bool, open: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.member_row), egui::Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if open {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.selected);
    } else if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let inner = rect.shrink2(egui::vec2(t.spacing.item_x, 0.0));
    let name_color = if unread || open || hot { t.colors.text_strong } else { t.colors.text };
    let mut name_font = egui::TextStyle::Body.resolve(ui.style());
    name_font.family = crate::ui::fonts::demi();
    let (name, spots) = crate::ui::emoji::truncated(ui, &th.name, &name_font, name_color, inner.width());
    let mut sub = p::messages(lang, u64::from(th.message_count));
    for (on, text) in [(th.joined, tx::JOINED), (th.kind == ThreadKind::Private, tx::PRIVATE), (th.locked, tx::LOCKED)] {
        if on {
            sub.push_str(" · ");
            sub.push_str(text.get(lang));
        }
    }
    let sub = ui.painter().layout_no_wrap(sub, egui::FontId::proportional(t.typography.small), t.colors.muted);
    // Les deux lignes, centrées ensemble dans la hauteur de la ligne.
    let top = Tokens::centered_top(rect.height(), name.size().y + sub.size().y);
    let pos = egui::pos2(inner.left(), rect.top() + top);
    crate::ui::emoji::paint_spots(ui, pos, &name, &spots);
    let sub_pos = egui::pos2(inner.left(), pos.y + name.size().y);
    ui.painter().galley(pos, name, name_color);
    ui.painter().with_clip_rect(inner).galley(sub_pos, sub, t.colors.muted);
    crate::ui::keys::focus_ring(ui, t, &resp);
    let label = th.name.clone();
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Menu « Notifications » de l'en-tête d'un fil (aperçu, tests).
pub(super) fn thread_notif_popup_id() -> egui::Id {
    egui::Id::unique("fil_notifications")
}

/// Résumé du fil né de ce message : « Nom · N messages », cliquable.
pub(super) fn thread_link(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts) {
    let Some(th) = &m.meta.thread else { return };
    let row = egui::Frame::new()
        .fill(t.colors.bg1)
        .stroke(t.border())
        .corner_radius(t.r_embed())
        .inner_margin(t.embed_margin())
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                inline_icon(ui, Icon::Thread, t.typography.body, t.colors.faint);
                ui.add(egui::Label::new(RichText::new(&th.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).truncate());
                ui.label(RichText::new(p::messages(lang, u64::from(th.messages))).color(t.colors.link));
                if th.archived {
                    ui.label(RichText::new(tx::ARCHIVED.get(lang)).small().color(t.colors.faint));
                }
            });
        })
        .response
        .rect;
    acts.hits.push((row, Hit::Thread(th.id)));
}
