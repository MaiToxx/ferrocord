//! Rôles et pseudo d'un membre (lot A3) : dialogue ouvert depuis la carte de
//! profil, le clic droit sur un membre ou la page Membres. Chaque case cochée
//! part tout de suite (geste) ; retirer un rôle demande confirmation.

use super::super::App;
use super::roles::{confirm, member_names, member_roles};
use super::rgb;
use crate::i18n::table::roles::{f, t as rt};
use crate::state::admin::roles::{self as admin, NICK_MAX};
use crate::state::model::Role;
use crate::state::Permissions;
use eframe::egui::{self, Id, RichText};

pub(in crate::ui) struct MemberDialog {
    guild_id: u64,
    user_id: u64,
    nick: String,
    /// Case décochée : (rôle) à confirmer.
    confirm_remove: Option<u64>,
}

/// Entrées d'administration du clic droit sur un membre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum MenuPick {
    Roles,
    Nick,
}

impl App {
    /// Je peux donner ou retirer au moins un rôle à ce membre.
    pub(in crate::ui) fn can_change_roles(&self, guild_id: u64) -> bool {
        let Some(g) = self.store.guilds.iter().find(|g| g.id == guild_id) else { return false };
        self.store.guild_has(guild_id, Permissions::MANAGE_ROLES) && g.roles.iter().any(|r| self.store.can_assign(guild_id, r))
    }

    /// Ce que le clic droit sur `user_id` propose dans le serveur affiché.
    pub(in crate::ui) fn member_menu_picks(&self, user_id: u64) -> Vec<MenuPick> {
        let Some(guild_id) = self.store.guild else { return Vec::new() };
        let mut out = Vec::new();
        if self.can_change_roles(guild_id) && self.store.known_members(guild_id).contains(&user_id) {
            out.push(MenuPick::Roles);
        }
        if self.store.can_set_nick(guild_id, user_id) {
            out.push(MenuPick::Nick);
        }
        out
    }

    /// Bouton « Gérer les rôles » de la carte de profil.
    pub(in crate::ui) fn card_offers_roles(&self, guild_id: Option<u64>, user_id: u64) -> bool {
        guild_id.is_some_and(|g| (self.can_change_roles(g) && member_roles(self, g, user_id).is_some()) || self.store.can_set_nick(g, user_id))
    }

    pub(in crate::ui) fn open_member_dialog(&mut self, guild_id: u64, user_id: u64) {
        let nick = self.store.people.lock().member(guild_id, user_id).and_then(|m| m.nick.clone()).unwrap_or_default();
        self.roles_admin.dialog = Some(MemberDialog { guild_id, user_id, nick, confirm_remove: None });
    }

    /// Ouvre le dialogue des rôles d'un membre (aperçu) ; `false` si rien n'y est permis.
    pub fn show_member_roles(&mut self, guild_id: u64, user_id: u64) -> bool {
        if !self.card_offers_roles(Some(guild_id), user_id) {
            return false;
        }
        self.open_member_dialog(guild_id, user_id);
        true
    }

    pub(super) fn member_dialog_ui(&mut self, ui: &mut egui::Ui) {
        let Some(d) = self.roles_admin.dialog.as_ref() else { return };
        let (guild_id, user_id, confirm_remove) = (d.guild_id, d.user_id, d.confirm_remove);
        let Some(g) = self.store.guilds.iter().find(|g| g.id == guild_id) else {
            self.roles_admin.dialog = None;
            return;
        };
        let roles: Vec<Role> = admin::sorted(&g.roles).into_iter().filter(|r| r.id != guild_id).cloned().collect();
        let lang = self.lang;
        let t = self.theme.clone();
        let (name, handle) = member_names(self, guild_id, user_id);
        let has = member_roles(self, guild_id, user_id);
        let can_roles = self.store.guild_has(guild_id, Permissions::MANAGE_ROLES);
        let can_nick = self.store.can_set_nick(guild_id, user_id);
        let current_nick = self.store.people.lock().member(guild_id, user_id).and_then(|m| m.nick.clone()).unwrap_or_default();
        let nick_busy = self.store.roles_admin.nick_pending(guild_id, user_id);
        let assignable: Vec<bool> = roles.iter().map(|r| can_roles && self.store.can_assign(guild_id, r)).collect();
        let pending: Vec<bool> = roles.iter().map(|r| self.store.roles_admin.member_role_pending(guild_id, user_id, r.id)).collect();
        let (mut toggle, mut save_nick, mut close) = (None, None, false);
        let Some(d) = self.roles_admin.dialog.as_mut() else { return };
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("ferrocord.admin.membre"), width, &f::roles_of(lang, &name), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            {
                crate::ui::settings::shell::page_style(ui, &t);
                ui.label(RichText::new(&handle).small().color(t.colors.faint));
                ui.separator();
                match &has {
                    None => {
                        ui.add(egui::Label::new(RichText::new(rt::MEMBER_ROLES_UNKNOWN.get(lang)).color(t.colors.muted)).wrap());
                    }
                    Some(has) => {
                        let max = ui.ctx().content_rect().height() * 0.5;
                        egui::ScrollArea::vertical().id_salt("membre-roles").max_height(max).show(ui, |ui| {
                            for (i, r) in roles.iter().enumerate() {
                                let on = has.contains(&r.id);
                                // Ce que je ne peux pas changer n'est montré que s'il est porté.
                                if !assignable[i] && !on {
                                    continue;
                                }
                                ui.horizontal(|ui| {
                                    let mut v = on;
                                    let label = RichText::new(&r.name).color(if r.color == 0 { t.colors.text } else { rgb(r.color) });
                                    if ui.add_enabled(assignable[i] && !pending[i], egui::Checkbox::new(&mut v, label)).changed() {
                                        toggle = Some((r.id, v));
                                    }
                                    if pending[i] {
                                        ui.spinner();
                                    }
                                });
                            }
                        });
                    }
                }
                if can_nick {
                    ui.separator();
                    ui.label(RichText::new(f::nick_of(lang, &name)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                    // Le bouton d'abord, calé à droite ; le champ prend ce qui reste : la ligne
                    // ne dépasse plus la largeur de la fenêtre.
                    ui.horizontal(|ui| {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let changed = d.nick.trim() != current_nick;
                            if ui.add_enabled(changed && !nick_busy, crate::ui::form::btn(&t, rt::SAVE.get(lang))).clicked() {
                                save_nick = Some(d.nick.clone());
                            }
                            if nick_busy {
                                ui.spinner();
                            }
                            let room = ui.available_width();
                            ui.add_sized([room, ui.spacing().interact_size.y], egui::TextEdit::singleline(&mut d.nick).hint_text(rt::NICK_HINT.get(lang)).char_limit(NICK_MAX));
                        });
                    });
                    if !current_nick.is_empty() && ui.add_enabled(!nick_busy, crate::ui::form::btn(&t, rt::NICK_RESET.get(lang))).clicked() {
                        d.nick.clear();
                        save_nick = Some(String::new());
                    }
                }
            }
        }, |ui| close = ui.add(crate::ui::form::btn(&t, rt::CLOSE.get(lang))).clicked());
        // Échap et clic à côté ferment, sauf pendant une confirmation.
        if modal.closed && confirm_remove.is_none() {
            close = true;
        }
        match toggle {
            Some((role, true)) => {
                self.store.set_member_role(guild_id, user_id, role, true);
            }
            Some((role, false)) => {
                if let Some(d) = self.roles_admin.dialog.as_mut() {
                    d.confirm_remove = Some(role);
                }
            }
            None => {}
        }
        if let Some(nick) = save_nick {
            self.store.set_nick(guild_id, user_id, &nick);
        }
        if let Some(role) = confirm_remove {
            let role_name = roles.iter().find(|r| r.id == role).map(|r| r.name.clone()).unwrap_or_default();
            if let Some(yes) = confirm(ui, &t, lang, "membre-retirer", &f::remove_question(lang, &role_name, &name), rt::REMOVE.get(lang)) {
                if yes {
                    self.store.set_member_role(guild_id, user_id, role, false);
                }
                if let Some(d) = self.roles_admin.dialog.as_mut() {
                    d.confirm_remove = None;
                }
            }
        }
        if close {
            self.roles_admin.dialog = None;
        }
    }
}
