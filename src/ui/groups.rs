//! MP de groupe : panneau des membres (ajouter, retirer, renommer, quitter)
//! et menu contextuel des conversations privées (marquer comme lu, fermer,
//! quitter). Quitter un groupe et retirer un membre demandent une confirmation.

use super::{
    friends::{paint_status, presence},
    util::{paint_avatar, pixel_square},
    words, App,
};
use crate::i18n::table::people::t as ut;
use crate::i18n::table::social::{f, t as st};
use crate::theme::icons::Icon;
use crate::state::relations::{RelKind, RelationsCmd, MAX_GROUP};
use crate::i18n::table::ff::t as ft;
use crate::theme::MembersPanel;
use eframe::egui::{self, Align, Layout, RichText, Sense};

/// Longueur maximale d'un nom de groupe (documentation : 1 à 100 caractères).
const GROUP_NAME_MAX: usize = 100;
/// Amis proposés à l'ajout, au plus (la recherche affine).
const ADD_SUGGESTIONS: usize = 8;

#[derive(Default)]
pub(super) struct GroupPanel {
    /// Groupe dont le panneau est ouvert.
    open: Option<u64>,
    rename: String,
    query: String,
    ask_leave: bool,
    ask_remove: Option<u64>,
    /// « Démarrer un appel vidéo » (lot U) : MP où ma caméra s'allume dès que j'y suis en appel.
    camera_on_join: Option<u64>,
    /// Transfert de propriété à confirmer (lot FF2, F20).
    ask_transfer: Option<u64>,
    /// Choix d'une icône en cours.
    icon_picker: Option<crate::guild_icon::IconPicker>,
    /// Un lien d'invitation a été demandé pour ce groupe : copié dès qu'il existe.
    invite_wanted: Option<u64>,
}

/// Choix du menu contextuel d'une conversation privée.
pub(super) enum DmMenu {
    MarkRead(u64),
    Close(u64),
    Leave(u64),
}

impl App {
    pub(super) fn is_group(&self, channel: u64) -> bool {
        self.store.relations.dm_info(channel).is_some_and(|d| d.group)
    }

    /// Ouvre le panneau des membres d'un groupe (aperçu, tests).
    pub fn show_group_members(&mut self, channel: u64) {
        self.open_group_panel(channel);
        self.look.edited.layout.members = MembersPanel::Shown;
    }

    fn open_group_panel(&mut self, channel: u64) {
        let name = self.store.dms.iter().find(|d| d.id == channel).map(|d| d.name.clone()).unwrap_or_default();
        self.groups = GroupPanel { open: Some(channel), rename: name, camera_on_join: self.groups.camera_on_join, invite_wanted: self.groups.invite_wanted, ..GroupPanel::default() };
    }

    /// Le panneau du groupe affiché est le seul panneau de membres d'un MP
    /// de groupe : il suit le bouton « Membres » de l'en-tête (même réglage
    /// que pour un serveur), et s'impose le temps de confirmer « Quitter ».
    pub(super) fn group_panel_shown(&self) -> bool {
        self.store.channel.is_some_and(|c| self.store.guild.is_none() && self.is_group(c) && (self.theme.members_shown() || (self.groups.ask_leave && self.groups.open == Some(c))))
    }

    pub(super) fn group_panel(&mut self, ui: &mut egui::Ui) {
        let Some(channel) = self.store.channel else { return };
        if self.groups.open != Some(channel) {
            self.open_group_panel(channel);
        }
        let Some(info) = self.store.relations.dm_info(channel).cloned() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let me = self.store.me.as_ref().map(|u| u.id);
        let owner = info.owner.is_some() && info.owner == me;
        let current = self.store.dms.iter().find(|d| d.id == channel).map(|d| d.name.clone()).unwrap_or_default();
        let mut cmd = None;
        let mut close = false;
        let mut hide = false;
        let mut profile = None;

        // En-tête de la hauteur de celui de la discussion : les deux titres sont
        // sur la même ligne, et les deux filets se rejoignent.
        super::settings::shell::page_style(ui, &t);
        let pad = t.spacing.channels_pad;
        ui.add_space((t.spacing.chat_head_y - pad).max(0.0));
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), t.head_row()), Layout::left_to_right(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(st::GROUP_SETTINGS.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong)).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                hide = super::util::icon_button(ui, &t, crate::theme::icons::Icon::Close, st::CLOSE.get(lang)).clicked();
            });
        });
        ui.add_space((t.spacing.chat_head_y - ui.spacing().item_spacing.y).max(0.0));
        let line = ui.cursor().top();
        ui.painter().hline(ui.max_rect().expand2(egui::vec2(t.spacing.chat_head_x, 0.0)).x_range(), line, egui::Stroke::new(t.shapes.border.max(1.0), t.colors.separator));
        ui.add_space(pad);

        // Renommer : le champ prend la place que laisse le bouton.
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), t.head_row()), Layout::right_to_left(Align::Center), |ui| {
            let name = self.groups.rename.trim().to_string();
            if ui.add_enabled(!name.is_empty() && name != current, egui::Button::new(st::RENAME.get(lang))).clicked() {
                cmd = Some(RelationsCmd::Rename { channel_id: channel, name });
            }
            ui.add(egui::TextEdit::singleline(&mut self.groups.rename).hint_text(st::GROUP_NAME_HINT.get(lang)).char_limit(GROUP_NAME_MAX).desired_width(f32::INFINITY));
        });
        // Icône et lien d'invitation du groupe (lot FF2, F20).
        if let Some(out) = self.groups.icon_picker.as_ref().and_then(crate::guild_icon::IconPicker::poll) {
            match out {
                crate::guild_icon::Outcome::Ready(p) => cmd = Some(RelationsCmd::SetIcon { channel_id: channel, icon: p.data_uri }),
                crate::guild_icon::Outcome::Unreadable => self.notify(ft::GROUP_ICON_UNREADABLE),
            }
        }
        let mut invite = false;
        // L'un sous l'autre : côte à côte, le second se repliait lettre par lettre dans le panneau.
        ui.vertical(|ui| {
            let picking = self.groups.icon_picker.as_ref().is_some_and(crate::guild_icon::IconPicker::busy);
            if crate::guild_icon::IconPicker::CAN_PICK && ui.add_enabled_ui(!picking, |ui| crate::ui::widgets::compact(ui, ft::GROUP_ICON.get(lang))).inner.clicked() {
                let ctx = ui.ctx().clone();
                let picker = self.groups.icon_picker.get_or_insert_with(|| crate::guild_icon::IconPicker::new(std::sync::Arc::new(move || ctx.request_repaint())));
                #[cfg(windows)]
                picker.pick(ft::GROUP_ICON.get(lang));
                #[cfg(not(windows))]
                let _ = picker;
            }
            let waiting = self.groups.invite_wanted == Some(channel);
            invite = ui.add_enabled_ui(!waiting, |ui| crate::ui::widgets::compact(ui, ft::GROUP_INVITE.get(lang))).inner.clicked();
        });
        ui.separator();

        // Membres : moi, puis les autres.
        let row_h = t.layout.small_avatar + 2.0 * t.spacing.item_y;
        let mut members: Vec<u64> = me.into_iter().collect();
        members.extend(info.recipients.iter().copied());
        egui::ScrollArea::vertical().max_height(ui.available_height() / 2.0).auto_shrink([false, true]).show(ui, |ui| {
            for id in members {
                // Clic sur la ligne : carte de profil (comme la liste des membres d'un serveur).
                let (rect, row) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                if row.clicked() {
                    profile = Some(id);
                }
                // Propriétaire : clic droit sur un membre pour lui transférer le groupe (F20).
                if owner && Some(id) != me {
                    row.context_menu(|ui| {
                        if crate::ui::menu::item(ui, &t, ft::GROUP_TRANSFER.get(lang), Some(crate::theme::icons::Icon::Crown), true).clicked() {
                            self.groups.ask_transfer = Some(id);
                            ui.close();
                        }
                    });
                }
                if row.hovered() {
                    ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
                }
                let side = t.layout.small_avatar;
                let slot = pixel_square(ui, egui::pos2(rect.left(), rect.center().y - side / 2.0), side);
                let mine = Some(id) == me;
                let (name, avatar) = if mine {
                    (st::YOU.get(lang).to_string(), self.store.me.as_ref().map(|u| u.avatar))
                } else {
                    let name = self.store.relations.display_name(id);
                    let name = if name.is_empty() { crate::i18n::t::UNKNOWN_USER.get(lang) } else { name };
                    (name.to_string(), self.store.relations.person(id).map(|p| p.avatar))
                };
                row.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if t.avatars_shown()
                    && let Some(a) = avatar
                {
                    paint_avatar(ui, &self.images, &t, slot, &a, &name);
                    if !mine && self.store.relations.relation(id).is_some_and(|r| r.kind == RelKind::Friend) {
                        paint_status(ui, &t, slot, presence(&self.store, id).status, t.colors.bg1);
                    }
                }
                let text = egui::Rect::from_min_max(egui::pos2(slot.right() + t.spacing.avatar_gap, rect.top()), rect.max);
                ui.scope_builder(egui::UiBuilder::new().max_rect(text).layout(Layout::left_to_right(Align::Center)), |ui| {
                    ui.add(egui::Label::new(RichText::new(&name).color(t.colors.text_strong)).truncate().selectable(false));
                    if info.owner == Some(id) {
                        ui.label(RichText::new(st::OWNER.get(lang)).small().color(t.colors.muted));
                    }
                    if owner && !mine {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if self.groups.ask_remove == Some(id) {
                                if ui.button(st::NO.get(lang)).clicked() {
                                    self.groups.ask_remove = None;
                                }
                                if ui.button(RichText::new(st::YES.get(lang)).color(t.colors.danger)).clicked() {
                                    cmd = Some(RelationsCmd::RemoveRecipient { channel_id: channel, user_id: id });
                                    self.groups.ask_remove = None;
                                }
                            } else if ui.button(RichText::new(st::REMOVE_MEMBER.get(lang)).color(t.colors.danger)).clicked() {
                                self.groups.ask_remove = Some(id);
                            }
                        });
                    }
                });
            }
        });
        ui.separator();

        // Ajouter un ami (10 membres au plus, moi compris).
        if info.recipients.len() + 1 < MAX_GROUP {
            ui.add(egui::TextEdit::singleline(&mut self.groups.query).hint_text(st::ADD_MEMBER_HINT.get(lang)).desired_width(f32::INFINITY));
            if !self.groups.query.trim().is_empty() {
                let found: Vec<u64> = self
                    .store
                    .friend_list(crate::state::relations::FriendTab::All, &self.groups.query)
                    .into_iter()
                    .filter(|id| !info.recipients.contains(id))
                    .take(ADD_SUGGESTIONS)
                    .collect();
                for id in found {
                    ui.horizontal(|ui| {
                        ui.add(egui::Label::new(self.store.relations.display_name(id)).truncate().selectable(false));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button(st::ADD_MEMBER.get(lang)).clicked() {
                                cmd = Some(RelationsCmd::AddRecipient { channel_id: channel, user_id: id });
                            }
                        });
                    });
                }
            }
        } else {
            ui.label(RichText::new(st::GROUP_FULL.get(lang)).small().color(t.colors.muted));
        }
        ui.separator();

        // Transfert de propriété (confirmé).
        if let Some(to) = self.groups.ask_transfer.filter(|u| owner && info.recipients.contains(u)) {
            let name = self.store.relations.display_name(to).to_string();
            ui.add(egui::Label::new(RichText::new(crate::i18n::table::ff::f::group_transfer_confirm(lang, name)).color(t.colors.text_strong)).wrap());
            ui.horizontal(|ui| {
                if ui.button(RichText::new(st::YES.get(lang)).color(t.colors.danger)).clicked() {
                    cmd = Some(RelationsCmd::Transfer { channel_id: channel, user_id: to });
                    self.groups.ask_transfer = None;
                }
                if ui.button(st::NO.get(lang)).clicked() {
                    self.groups.ask_transfer = None;
                }
            });
            ui.separator();
        }
        // Quitter (confirmé).
        if self.groups.ask_leave {
            ui.label(RichText::new(f::confirm_leave(lang, words::dm_name(lang, &current))).color(t.colors.text_strong));
            ui.horizontal(|ui| {
                if ui.button(RichText::new(st::YES.get(lang)).color(t.colors.danger)).clicked() {
                    cmd = Some(RelationsCmd::Close { channel_id: channel });
                    close = true;
                }
                if ui.button(st::NO.get(lang)).clicked() {
                    self.groups.ask_leave = false;
                }
            });
        } else if ui.button(RichText::new(st::LEAVE_GROUP.get(lang)).color(t.colors.danger)).clicked() {
            self.groups.ask_leave = true;
        }

        if let Some(c) = cmd {
            if matches!(c, RelationsCmd::AddRecipient { .. }) {
                self.groups.query.clear();
            }
            self.store.relations_write(c);
        }
        if invite && self.store.create_group_invite(channel) {
            self.groups.invite_wanted = Some(channel);
        }
        // Lien demandé : copié dès qu'il existe ; création refusée : le magasin l'a dit.
        if let Some(wanted) = self.groups.invite_wanted {
            match &self.store.servers.created {
                Some(i) if i.channel_id == Some(wanted) || (i.guild_id.is_none() && i.channel_id.is_none()) => {
                    ui.ctx().copy_text(i.url());
                    self.store.servers.created = None;
                    self.groups.invite_wanted = None;
                    self.notify(ft::GROUP_INVITE_COPIED);
                }
                _ if !self.store.servers.pending(crate::state::servers::ServerAction::Invite) => self.groups.invite_wanted = None,
                _ => {}
            }
        }
        if close {
            self.groups = GroupPanel::default();
        }
        if hide {
            self.groups.ask_leave = false;
            self.look.edited.layout.members = MembersPanel::Hidden;
        }
        if let Some(id) = profile {
            self.open_member_profile(id, None);
        }
    }

    /// Entrées du menu contextuel d'une conversation privée (menu ouvert
    /// par `channels.rs`, avec les notifications).
    pub(super) fn dm_menu(&self, ui: &mut egui::Ui, channel: u64) -> Option<DmMenu> {
        let lang = self.lang;
        let group = self.is_group(channel);
        let mut out = None;
        if crate::ui::menu::item(ui, &self.theme, st::MARK_READ.get(lang), Some(crate::theme::icons::Icon::Check), false).clicked() {
            out = Some(DmMenu::MarkRead(channel));
            ui.close();
        }
        let (label, choice) = if group { (st::LEAVE_GROUP, DmMenu::Leave(channel)) } else { (st::CLOSE_DM, DmMenu::Close(channel)) };
        if crate::ui::menu::item(ui, &self.theme, label.get(lang), Some(if group { crate::theme::icons::Icon::Logout } else { crate::theme::icons::Icon::Close }), group).clicked() {
            out = Some(choice);
            ui.close();
        }
        out
    }

    pub(super) fn apply_dm_menu(&mut self, choice: DmMenu) {
        match choice {
            DmMenu::MarkRead(c) => self.store.mark_read(c),
            // Fermer un MP à deux se défait en le rouvrant : pas de confirmation.
            DmMenu::Close(c) => self.store.relations_write(RelationsCmd::Close { channel_id: c }),
            DmMenu::Leave(c) => {
                self.open_channel(c);
                self.open_group_panel(c);
                self.groups.ask_leave = true;
            }
        }
    }
}

/// Colonne et en-tête des messages privés (lot U).
impl App {
    /// Tête de la colonne des MP : « Rechercher ou lancer une conversation »,
    /// qui ouvre le sélecteur rapide ; `true` au clic. Comme dans le client :
    /// haut de 40 (il déborde dans la marge de l'en-tête, dont la hauteur ne
    /// change pas), texte centré en demi-gras, bordure fine.
    pub(super) fn dm_find_button(&self, ui: &mut egui::Ui) -> bool {
        let t = &self.theme;
        let label = ut::FIND.get(self.lang);
        let (slot, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ui.available_height().min(t.find_button_height())), Sense::hover());
        let rect = egui::Rect::from_center_size(slot.center(), egui::vec2(slot.width(), t.find_button_height()));
        let resp = ui.interact(rect, ui.scope_id().with("chercher-mp"), Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        let mut p = ui.painter().clone();
        p.set_clip_rect(ui.clip_rect().expand2(egui::vec2(0.0, ((rect.height() - slot.height()) / 2.0).max(0.0))));
        p.rect(rect, t.r_control(), if hot { t.colors.bg3 } else { t.colors.bg0 }, t.border(), egui::StrokeKind::Inside);
        let inner = rect.shrink2(egui::vec2(t.spacing.item_x, 0.0));
        let text = RichText::new(label).size(t.typography.button).family(crate::ui::fonts::demi()).color(t.colors.text);
        let galley = egui::WidgetText::from(text).into_galley(ui, Some(egui::TextWrapMode::Truncate), inner.width(), egui::TextStyle::Body);
        let pos = egui::Align2::CENTER_CENTER.align_size_within_rect(galley.size(), inner).min;
        p.galley(pos, galley, t.colors.text);
        super::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    /// En-tête « Messages privés » de la liste (casse normale, comme dans le
    /// client), avec son « + » (créer un MP ou un groupe) ; `true` si le « + »
    /// vient d'être cliqué.
    pub(super) fn dm_list_head(&self, ui: &mut egui::Ui) -> bool {
        let t = &self.theme;
        let lang = self.lang;
        ui.add_space(t.spacing.category_gap);
        let head = ui.horizontal(|ui| {
            ui.add_space(t.spacing.item_x);
            let text = RichText::new(ut::DMS.get(lang)).size(t.typography.small + 2.0).family(crate::ui::fonts::medium()).color(t.colors.muted);
            ui.add(egui::Label::new(text).truncate().selectable(false));
        });
        let row = egui::Rect::from_min_max(head.response.rect.min, egui::pos2(ui.max_rect().right(), head.response.rect.max.y));
        let tip = ut::NEW_DM.get(lang);
        let icon = crate::theme::Tokens::icon_side(t.typography.button);
        let center = egui::pos2(row.right() - t.spacing.mention_inset, row.center().y);
        let hit = egui::Rect::from_center_size(center, egui::Vec2::splat(row.height() + t.spacing.item_y));
        let resp = ui.interact(hit, ui.scope_id().with("plus-mp"), Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        if hot {
            ui.painter().rect_filled(hit, t.r_row(), t.colors.row_hover);
        }
        let at = pixel_square(ui, center - egui::Vec2::splat(icon / 2.0), icon);
        crate::theme::icons::paint(Icon::Plus, ui.painter(), at, if hot { t.colors.text_strong } else { t.colors.muted });
        super::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        ui.add_space(t.spacing.item_y);
        resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    /// Croix de fermeture d'une ligne de MP, montrée au survol de la ligne ; `true` au clic.
    pub(super) fn dm_close_cross(&self, ui: &mut egui::Ui, row: &egui::Response, channel: u64) -> bool {
        if !row.contains_pointer() || super::mobile::is_active(ui.ctx()) {
            return false;
        }
        let t = &self.theme;
        let tip = if self.is_group(channel) { st::LEAVE_GROUP } else { st::CLOSE_DM }.get(self.lang);
        let icon = crate::theme::Tokens::icon_side(t.typography.small);
        let center = egui::pos2(row.rect.right() - t.spacing.item_x - icon / 2.0, row.rect.center().y);
        let hit = egui::Rect::from_center_size(center, egui::Vec2::splat(icon + 2.0 * t.spacing.item_y));
        let resp = ui.interact(hit, ui.scope_id().with(("fermer-mp", channel)), Sense::click());
        // Fond de la ligne sous la croix : la fin d'un nom long ne passe pas dessous.
        let under = if self.store.channel == Some(channel) { t.colors.selected } else { t.colors.row_hover };
        ui.painter().rect_filled(hit.expand2(egui::vec2(t.spacing.item_y, 0.0)), 0.0, under);
        let at = pixel_square(ui, center - egui::Vec2::splat(icon / 2.0), icon);
        crate::theme::icons::paint(Icon::Close, ui.painter(), at, if resp.hovered() { t.colors.text_strong } else { t.colors.muted });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    /// Correspondant d'un MP à deux (identifiant, nom affiché) ; rien pour un groupe.
    pub(super) fn dm_person(&self, channel: u64) -> Option<(u64, String)> {
        let info = self.store.relations.dm_info(channel).filter(|d| !d.group)?;
        let user = *info.recipients.first()?;
        let name = self.store.relations.display_name(user);
        let name = if name.is_empty() { self.store.dms.iter().find(|d| d.id == channel).map(|d| d.name.clone()).unwrap_or_default() } else { name.to_string() };
        Some((user, name))
    }

    /// En-tête d'un MP : profil du correspondant (MP à deux) et « Ajouter des
    /// amis au MP ». Posés de droite à gauche.
    pub(super) fn dm_head_buttons(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let lang = self.lang;
        let person = self.dm_person(cid);
        if let Some((user, _)) = person {
            // Bureau : le bouton affiche ou masque le panneau de profil (même réglage
            // que la liste des membres) ; disposition mobile : le profil complet.
            if self.mobile_active() {
                if super::chat_head::head_icon_drawn(ui, &t, Icon::UserCircle, ut::SHOW_PROFILE.get(lang), false).clicked() {
                    self.open_full_profile(user, None);
                }
            } else {
                let shown = t.members_shown();
                let tip = if shown { crate::i18n::table::restes::t::PROFILE_HIDE } else { crate::i18n::table::restes::t::PROFILE_SHOW }.get(lang);
                if super::chat_head::head_icon_drawn(ui, &t, Icon::UserCircle, tip, shown).clicked() {
                    self.look.edited.layout.members = if shown { crate::theme::MembersPanel::Hidden } else { crate::theme::MembersPanel::Shown };
                }
            }
        }
        // MP officiel de Discord : on n'y ajoute personne.
        if super::official::shown(ui.ctx()) {
            return;
        }
        if super::chat_head::head_icon_drawn(ui, &t, Icon::GroupAdd, ut::ADD_TO_DM.get(lang), false).clicked() {
            match person {
                // MP à deux : un nouveau groupe avec ce correspondant déjà coché.
                Some((user, _)) => self.pick_friends(vec![user]),
                // Groupe : son panneau, où se trouve « Ajouter ».
                None => self.show_group_members(cid),
            }
        }
    }

    /// En-tête d'un MP : « Démarrer un appel vidéo » — l'appel, puis ma caméra
    /// dès que j'y suis. Rien pendant un appel (la caméra est dans sa vue).
    pub(super) fn dm_video_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        if self.groups.camera_on_join == Some(cid) && self.store.in_voice(cid) {
            self.groups.camera_on_join = None;
            self.store.video_set_camera(true);
            self.flush_commands();
        }
        if self.store.calls.call(cid).is_some() || self.store.calls.calling(cid) {
            return;
        }
        let t = self.theme.clone();
        if super::chat_head::head_icon_drawn(ui, &t, Icon::Camera, ut::VIDEO_CALL.get(self.lang), false).clicked() {
            self.groups.camera_on_join = Some(cid);
            self.store.start_call(cid);
            self.flush_commands();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, find, frame, offline_boot};
    use crate::i18n::{t as tx, Lang};
    use crate::state::model::{Avatar, Dm, User};
    use crate::state::relations::{DmInfo, Person, RelationsEvent};
    use crate::state::{Cmd, Event, Store};
    use eframe::egui;
    use std::sync::{Arc, Mutex};

    const GROUP: u64 = 50;

    struct Recorder(Arc<Mutex<Vec<Cmd>>>);

    impl super::super::Backend for Recorder {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            if let Ok(mut v) = self.0.lock() {
                v.extend(cmds);
            }
        }
    }

    /// MP de groupe connu des deux côtés (liste des membres et relations) :
    /// un seul panneau, celui du groupe, et un seul bouton qui le montre ou
    /// le masque ; un clic sur un membre ouvre sa carte.
    #[test]
    fn group_dm_has_one_panel_and_one_button() {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        let dm = Dm { id: GROUP, name: "Groupe".into(), last_message: 0, avatar: Avatar::default_for(GROUP), recipients: vec![(2, "Alice".into())] };
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![dm] });
        store.apply(Event::GroupRecipients { channel_id: GROUP, users: vec![User::new(2, "Alice".into())] });
        let alice = Person { id: 2, name: "Alice".into(), username: "alice".into(), avatar: Avatar::default_for(2) };
        let info = DmInfo { id: GROUP, group: true, owner: Some(1), recipients: vec![2], ..Default::default() };
        store.apply(Event::Relations(RelationsEvent::Ready { relations: vec![], people: vec![alice], dms: vec![info], reads: Some(vec![]), heads: vec![] }));
        store.select_dms();
        store.select_channel(GROUP);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = super::super::App::with_context(ctx, offline_boot(Box::new(Recorder(sent.clone())), store, images));
        app.theme = crate::theme::Tokens::default();
        let fr = Lang::Fr;
        let leave = crate::i18n::table::social::t::LEAVE_GROUP.get(fr);
        let labels = |out: &egui::FullOutput| -> Vec<String> { out.platform_output.accesskit_update.iter().flat_map(|u| u.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string))).collect() };
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        let out = frame(&mut app, Vec::new());
        let all = labels(&out);
        assert!(find(&out, leave).is_some(), "panneau du groupe affiché : {all:?}");
        assert_eq!(all.iter().filter(|l| l.contains(tx::MEMBERS_HIDE_TIP.get(fr))).count(), 1, "un seul bouton : {all:?}");
        assert!(!all.iter().any(|l| l.contains(&tx::MEMBERS.get(fr).to_uppercase())), "pas de seconde liste des membres : {all:?}");
        // Le bouton masque le panneau, puis le remontre.
        click(&mut app, tx::MEMBERS_HIDE_TIP.get(fr));
        assert!(find(&frame(&mut app, Vec::new()), leave).is_none());
        click(&mut app, tx::MEMBERS_SHOW_TIP.get(fr));
        let out = frame(&mut app, Vec::new());
        assert!(find(&out, leave).is_some());
        // Clic sur un membre (à gauche de la ligne, hors du bouton « Retirer ») : sa carte de profil.
        let row = find(&out, "Alice").unwrap_or_else(|| panic!("ligne d'Alice introuvable"));
        let at = egui::pos2(row.left() + 4.0, row.center().y);
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&mut app, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(&mut app, vec![button(false)]);
        frame(&mut app, Vec::new());
        let asked = sent.lock().map(|v| v.iter().any(|c| matches!(c, Cmd::Profile(crate::state::profiles::ProfileCmd::Fetch { user_id: 2, .. })))).unwrap_or(false);
        assert!(asked, "carte de profil d'Alice ouverte (profil demandé)");
    }
}
