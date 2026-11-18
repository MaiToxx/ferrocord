//! Menu contextuel complet d'un utilisateur (lot U), dans l'ordre du client :
//! profil, mentionner, message, appeler, note, pseudo d'ami ; inviter sur un
//! serveur, ami, ignorer, bloquer ; pseudo et rôles du membre, exclure,
//! expulser, bannir ; sourdine de la conversation ; applications ; identifiant.
//!
//! Les lignes se posent dans n'importe quel menu d'egui (`user_menu_items` :
//! ligne d'un MP, ligne d'un ami) ; ouvert par un clic droit ailleurs (membre,
//! auteur, mention), le menu est un volet d'egui, donc gardé dans la fenêtre,
//! sous-menus compris. Chaque écriture suit le clic ; retirer un ami et
//! bloquer demandent « Oui ».

use super::guild_settings::member_roles::MenuPick;
use super::{menu, widgets, App};
use crate::i18n::table::people::t as ut;
use crate::i18n::table::profile::t as pt;
use crate::i18n::table::social::t as st;
use crate::state::model::Avatar;
use crate::state::relations::{RelKind, RelationsCmd};
use crate::theme::icons::Icon;
use eframe::egui::{self, Id, RichText};

/// Longueur maximale d'un pseudo d'ami (docs.discord.food, Modify Relationship).
const NICK_MAX: usize = 32;
/// Images pendant lesquelles l'ouverture est redemandée : le clic droit qui
/// déplace un menu déjà ouvert est aussi, pour lui, un clic en dehors.
const OPENING: u8 = 2;

#[derive(Clone)]
struct Open {
    user_id: u64,
    name: String,
    at: egui::Pos2,
    fresh: u8,
}

/// Pseudo d'ami en cours de saisie.
#[derive(Clone)]
struct NickEdit {
    user_id: u64,
    text: String,
    fresh: bool,
}

/// Invitation demandée depuis le menu d'une personne : son lien, une fois
/// créé, part dans le MP avec elle (comme dans Discord).
#[derive(Clone)]
struct InviteTo {
    guild: u64,
    user_id: u64,
    name: String,
    /// Lien créé, en attente du MP.
    url: Option<String>,
    /// Instant (horloge d'egui) où le MP a été demandé.
    dm_asked: Option<f64>,
}

/// Validité d'une invitation envoyée à une personne : 7 jours, sans limite d'usages (défaut de Discord).
const INVITE_AGE: u32 = 7 * 24 * 3600;
/// Attente du MP demandé avant d'abandonner (secondes).
const DM_WAIT: f64 = 15.0;

fn invite_id() -> Id {
    Id::unique("ferrocord.personnes.invitation")
}

fn open_id() -> Id {
    Id::unique("ferrocord.personnes.menu")
}

fn popup_id() -> Id {
    Id::unique("ferrocord.personnes.menu.volet")
}

fn nick_id() -> Id {
    Id::unique("ferrocord.personnes.pseudo")
}

/// Ouvre le menu de `user_id` au point `at`.
pub(super) fn open(ctx: &egui::Context, user_id: u64, name: &str, at: egui::Pos2) {
    ctx.data_mut(|d| d.insert_temp(open_id(), Open { user_id, name: name.to_string(), at, fresh: OPENING }));
}

/// Liste longue d'un sous-menu (serveurs, rôles) : elle défile au lieu de sortir de la fenêtre.
fn long_list(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let max = ui.ctx().content_rect().height() * 0.6;
    egui::ScrollArea::vertical().max_height(max).show(ui, add);
}

impl App {
    /// Lignes du menu de `user_id` ; à poser dans un menu d'egui.
    pub(super) fn user_menu_items(&mut self, ui: &mut egui::Ui, user_id: u64, name: &str) {
        let t = self.theme.clone();
        let lang = self.lang;
        let ctx = ui.ctx().clone();
        let me = self.store.me.as_ref().is_some_and(|u| u.id == user_id);
        let copy_id = super::settings::local::copy_id_shown(&ctx, &self.store);
        let here = egui::Rect::from_min_size(ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default(), egui::Vec2::ZERO);

        if menu::item(ui, &t, pt::MENU_PROFILE.get(lang), Some(Icon::Members), false).clicked() {
            super::profile::open_here(&ctx, user_id, name, here);
        }
        if self.store.channel.is_some() && menu::item(ui, &t, pt::MENU_MENTION.get(lang), Some(Icon::Reply), false).clicked() {
            self.insert_mention(user_id, name);
        }
        if me {
            if copy_id {
                menu::separator(ui, &t);
                if menu::item(ui, &t, pt::MENU_COPY_ID.get(lang), Some(Icon::Id), false).clicked() {
                    ctx.copy_text(user_id.to_string());
                }
            }
            return;
        }
        if menu::item(ui, &t, pt::MENU_MESSAGE.get(lang), Some(Icon::Chat), false).clicked() {
            self.store.message_user(user_id);
        }
        // « Appeler » : seulement quand la conversation existe déjà (l'appel se pose sur elle).
        let dm = self.store.relations.dm_with(user_id).filter(|c| self.store.dms.iter().any(|d| d.id == *c));
        if let Some(c) = dm
            && self.store.calls.call(c).is_none()
            && !self.store.calls.calling(c)
            && menu::item(ui, &t, ut::CALL.get(lang), Some(Icon::Phone), false).clicked()
        {
            self.store.open_dm_with(user_id);
            self.store.start_call(c);
            self.flush_commands();
        }
        let known = self.store.people.lock().user(user_id).map(|u| (u.username.clone(), u.avatar, u.discriminator.is_none() && !u.bot, u.bot));
        let (username, avatar, can_request, bot) = known.unwrap_or_else(|| (String::new(), Avatar::default_for(user_id), false, false));
        if menu::item(ui, &t, ut::ADD_NOTE.get(lang), Some(Icon::Pencil), false).clicked() {
            super::profile_full::open_note(&ctx, user_id, self.store.guild, name, avatar);
        }
        self.user_menu_volume(ui, user_id);
        let rel = self.store.relations.relation(user_id).map(|r| (r.kind, r.ignored, r.nickname.clone().unwrap_or_default()));
        let kind = rel.as_ref().map(|r| r.0);
        let ignored = rel.as_ref().is_some_and(|r| r.1);
        if kind == Some(RelKind::Friend) {
            let nick = rel.map(|r| r.2).unwrap_or_default();
            let label = if nick.is_empty() { ut::ADD_FRIEND_NICK } else { ut::EDIT_FRIEND_NICK };
            if menu::item(ui, &t, label.get(lang), Some(Icon::Pencil), false).clicked() {
                ctx.data_mut(|d| d.insert_temp(nick_id(), NickEdit { user_id, text: nick, fresh: true }));
            }
        }

        menu::separator(ui, &t);
        if !bot && !self.store.guilds.is_empty() {
            let mut invite = None;
            menu::submenu(ui, &t, ut::INVITE_TO_SERVER.get(lang), |ui| {
                long_list(ui, |ui| {
                    // Serveurs où je peux créer une invitation ; calculé seulement quand le sous-menu est ouvert.
                    for g in self.store.guilds.iter().filter(|g| !self.store.invite_channels(g.id).is_empty()) {
                        if menu::item(ui, &t, &g.name, None, false).clicked() {
                            invite = Some(g.id);
                        }
                    }
                });
            });
            if let Some(g) = invite {
                self.invite_user_to(g, user_id, name);
            }
        }
        // Catégories de contacts (locales) : une case par catégorie. Pour n'importe qui
        // sauf une personne bloquée : un MP se range aussi sans être ami.
        if kind != Some(RelKind::Blocked) && self.store.me.as_ref().is_none_or(|me| me.id != user_id) {
            let groups = self.local.prefs.friend_groups.clone();
            let mut change = None;
            menu::submenu(ui, &t, st::CATEGORIES.get(lang), |ui| {
                if groups.groups.is_empty() {
                    ui.label(egui::RichText::new(st::CAT_NONE_YET.get(lang)).color(t.colors.muted));
                }
                long_list(ui, |ui| {
                    for g in &groups.groups {
                        let mut on = g.members.contains(&user_id);
                        if menu::check(ui, &t, &g.name, &mut on) {
                            change = Some((g.name.clone(), on));
                        }
                    }
                });
            });
            if let Some((name, on)) = change {
                self.friend_groups_edit(|g| g.set(&name, user_id, on));
            }
        }
        match kind {
            Some(RelKind::Friend) => {
                if menu::item(ui, &t, pt::MENU_REMOVE.get(lang), Some(Icon::Close), true).clicked() {
                    super::profile_full::ask_relation(&ctx, user_id, name, false);
                }
            }
            Some(RelKind::Incoming) => {
                if menu::item(ui, &t, pt::MENU_ACCEPT.get(lang), Some(Icon::FriendAdd), false).clicked() {
                    self.store.relations_write(RelationsCmd::Accept { user_id });
                }
            }
            Some(RelKind::Outgoing) => {
                if menu::item(ui, &t, pt::MENU_CANCEL.get(lang), Some(Icon::Close), false).clicked() {
                    self.store.relations_write(RelationsCmd::Remove { user_id });
                }
            }
            Some(RelKind::Blocked) => {}
            // Demande d'ami par nom : seulement pour le nouveau système de noms.
            _ if can_request && menu::item(ui, &t, pt::MENU_ADD_FRIEND.get(lang), Some(Icon::FriendAdd), false).clicked() => {
                self.store.relations_write(RelationsCmd::AddFriend { username, note: None });
            }
            _ => {}
        }
        if ignored {
            if menu::item(ui, &t, st::UNIGNORE.get(lang), Some(Icon::Eye), false).clicked() {
                self.store.relations_write(RelationsCmd::Unignore { user_id });
            }
        } else if kind != Some(RelKind::Blocked) && menu::item(ui, &t, ut::IGNORE.get(lang), Some(Icon::BellOff), false).clicked() {
            self.store.relations_write(RelationsCmd::Ignore { user_id });
        }
        if kind == Some(RelKind::Blocked) {
            if menu::item(ui, &t, pt::MENU_UNBLOCK.get(lang), Some(Icon::Close), false).clicked() {
                self.store.relations_write(RelationsCmd::Remove { user_id });
            }
        } else if menu::item(ui, &t, pt::MENU_BLOCK.get(lang), Some(Icon::Lock), true).clicked() {
            super::profile_full::ask_relation(&ctx, user_id, name, true);
        }

        // Membre du serveur affiché : pseudo, rôles à cocher, modération (selon mes permissions).
        let admin = self.member_menu_picks(user_id);
        let mods = self.moderation_picks(user_id);
        if let Some(guild) = self.store.guild.filter(|_| !admin.is_empty() || !mods.is_empty()) {
            menu::separator(ui, &t);
            if admin.contains(&MenuPick::Nick) && menu::item(ui, &t, ut::CHANGE_NICK.get(lang), Some(Icon::Pencil), false).clicked() {
                self.open_member_dialog(guild, user_id);
            }
            if admin.contains(&MenuPick::Roles) {
                let mut toggle = None;
                menu::submenu(ui, &t, ut::ROLES.get(lang), |ui| long_list(ui, |ui| toggle = self.role_checks(ui, guild, user_id)));
                if let Some((role, on)) = toggle {
                    self.store.set_member_role(guild, user_id, role, on);
                }
            }
            for k in mods {
                if menu::item(ui, &t, k.label().get(lang), None, k.danger()).clicked() {
                    self.open_member_action(user_id, name, k);
                }
            }
        }

        let apps = self.user_app_commands();
        if dm.is_some() || !apps.is_empty() {
            menu::separator(ui, &t);
        }
        if let Some(c) = dm {
            self.mute_entry(ui, c, ut::MUTE_DM.get(lang));
        }
        // Commandes de menu contextuel des applications (lot T11).
        if !apps.is_empty() {
            let mut run = None;
            menu::submenu(ui, &t, crate::i18n::table::interactions::t::APPS.get(lang), |ui| {
                long_list(ui, |ui| {
                    for (id, name) in &apps {
                        if menu::item(ui, &t, name, Some(Icon::Apps), false).clicked() {
                            run = Some(*id);
                        }
                    }
                });
            });
            if let Some(id) = run {
                self.run_user_command(id, user_id);
            }
        }
        if copy_id {
            menu::separator(ui, &t);
            if menu::item(ui, &t, pt::MENU_COPY_ID.get(lang), Some(Icon::Id), false).clicked() {
                ctx.copy_text(user_id.to_string());
            }
        }
    }

    /// Rôles du serveur en cases à cocher ; renvoie (rôle, nouvel état) au clic.
    /// Ce que je ne peux pas changer n'est montré que s'il est porté.
    fn role_checks(&self, ui: &mut egui::Ui, guild: u64, user_id: u64) -> Option<(u64, bool)> {
        let t = &self.theme;
        let g = self.store.guilds.iter().find(|g| g.id == guild)?;
        let has = self.store.people.lock().member(guild, user_id).and_then(|m| m.roles.clone()).unwrap_or_default();
        let mut out = None;
        for r in crate::state::admin::roles::sorted(&g.roles).into_iter().filter(|r| r.id != guild) {
            let mut on = has.contains(&r.id);
            let can = self.store.can_assign(guild, r);
            if !can && !on {
                continue;
            }
            let busy = self.store.roles_admin.member_role_pending(guild, user_id, r.id);
            ui.add_enabled_ui(can && !busy, |ui| {
                if menu::check(ui, t, &r.name, &mut on) {
                    out = Some((r.id, on));
                }
            });
        }
        out
    }

    /// Menu d'un utilisateur ouvert par un clic droit, et fenêtre du pseudo d'ami.
    pub(super) fn user_menu_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        if let Some(m) = ctx.data(|d| d.get_temp::<Open>(open_id())) {
            let id = popup_id();
            egui::Popup::new(id, ctx.clone(), egui::PopupAnchor::Position(m.at), ui.layer_id())
                .kind(egui::PopupKind::Menu)
                .layout(egui::Layout::top_down_justified(egui::Align::Min))
                .style(egui::containers::menu::menu_style)
                .open_memory((m.fresh > 0).then_some(egui::SetOpenCommand::Bool(true)))
                .show(|ui| self.user_menu_items(ui, m.user_id, &m.name));
            // Un autre menu a pu être demandé pendant le dessin : il reste.
            let same = ctx.data(|d| d.get_temp::<Open>(open_id())).is_some_and(|o| o.user_id == m.user_id && o.at == m.at && o.fresh == m.fresh);
            if same {
                if m.fresh > 0 {
                    ctx.data_mut(|d| d.insert_temp(open_id(), Open { fresh: m.fresh - 1, ..m }));
                    ctx.request_repaint();
                } else if !egui::Popup::is_id_open(&ctx, id) {
                    ctx.data_mut(|d| d.remove::<Open>(open_id()));
                }
            }
        }
        self.friend_nick_ui(&ctx);
        self.invite_tick(&ctx);
    }

    /// « Inviter sur le serveur » : crée une invitation vers le premier salon
    /// qui le permet ; son lien part dans le MP dès qu'il existe.
    pub fn invite_user_to(&mut self, guild: u64, user_id: u64, name: &str) {
        let Some(channel) = self.store.invite_channels(guild).first().map(|c| c.0) else {
            self.notify(crate::i18n::table::restes::t::INVITE_NO_CHANNEL);
            return;
        };
        if self.store.create_invite(channel, INVITE_AGE, 0, false) {
            self.ctx.data_mut(|d| d.insert_temp(invite_id(), InviteTo { guild, user_id, name: name.to_string(), url: None, dm_asked: None }));
            self.flush_commands();
        }
    }

    /// Fenêtre « Inviter des amis » (F14) : envoie à un ami un lien déjà
    /// créé, par le même chemin (MP existant, sinon ouvert, puis un message).
    pub(super) fn send_invite_link(&mut self, guild: u64, user_id: u64, name: &str, url: String) {
        self.ctx.data_mut(|d| d.insert_temp(invite_id(), InviteTo { guild, user_id, name: name.to_string(), url: Some(url), dm_asked: None }));
        self.ctx.request_repaint();
    }

    /// Un envoi d'invitation attend encore son MP : le suivant patiente.
    pub(super) fn invite_sending(&self) -> bool {
        self.ctx.data(|d| d.get_temp::<InviteTo>(invite_id())).is_some()
    }

    /// Suite de [`App::invite_user_to`] : attend le lien, puis le MP, puis envoie.
    fn invite_tick(&mut self, ctx: &egui::Context) {
        let Some(mut inv) = ctx.data(|d| d.get_temp::<InviteTo>(invite_id())) else { return };
        let drop = |ctx: &egui::Context| {
            ctx.data_mut(|d| d.remove::<InviteTo>(invite_id()));
        };
        if inv.url.is_none() {
            match &self.store.servers.created {
                Some(i) if i.guild_id == Some(inv.guild) => {
                    inv.url = Some(i.url());
                    self.store.servers.created = None;
                }
                // Création refusée : le magasin l'a déjà dit.
                _ if !self.store.servers.pending(crate::state::servers::ServerAction::Invite) => return drop(ctx),
                _ => return,
            }
        }
        let Some(url) = inv.url.clone() else { return };
        let dm = self.store.relations.dm_with(inv.user_id).filter(|c| self.store.dms.iter().any(|d| d.id == *c));
        let now = ctx.input(|i| i.time);
        match (dm, inv.dm_asked) {
            (Some(c), _) => {
                drop(ctx);
                if self.store.send_message(c, url) {
                    let guild = self.store.guilds.iter().find(|g| g.id == inv.guild).map(|g| g.name.clone()).unwrap_or_default();
                    self.notify(crate::i18n::table::restes::f::invite_sent(self.lang, guild, &inv.name));
                }
                self.flush_commands();
            }
            (None, None) => {
                inv.dm_asked = Some(now);
                self.store.open_dm_with(inv.user_id);
                self.flush_commands();
                ctx.data_mut(|d| d.insert_temp(invite_id(), inv));
            }
            // MP jamais arrivé (refusé) : abandon, rien n'est envoyé.
            (None, Some(at)) if now - at > DM_WAIT => drop(ctx),
            (None, Some(_)) => {
                ctx.data_mut(|d| d.insert_temp(invite_id(), inv));
            }
        }
    }

    /// « Pseudo d'ami » : écrit à « Enregistrer » (ou Entrée), retiré par « Retirer le pseudo ».
    fn friend_nick_ui(&mut self, ctx: &egui::Context) {
        let Some(mut n) = ctx.data(|d| d.get_temp::<NickEdit>(nick_id())) else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let current = self.store.relations.relation(n.user_id).and_then(|r| r.nickname.clone()).unwrap_or_default();
        let hint = self.store.relations.person(n.user_id).map(|p| p.name.clone()).unwrap_or_default();
        let (mut save, mut enter, mut reset, mut cancel) = (false, false, false, false);
        let modal = widgets::modal(
            ctx,
            &t,
            nick_id(),
            super::mobile::card_width(ctx, &t, 1.0),
            ut::NICK_TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.add(egui::Label::new(RichText::new(ut::NICK_DETAIL.get(lang)).color(t.colors.muted)).wrap());
                ui.add_space(t.spacing.item_y);
                let field = widgets::field(ui, &t, &mut n.text, &hint);
                if n.fresh {
                    field.request_focus();
                    n.fresh = false;
                }
                if n.text.chars().count() > NICK_MAX {
                    n.text = n.text.chars().take(NICK_MAX).collect();
                }
                enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            },
            |ui| {
                save |= widgets::button(ui, &t, widgets::Kind::Primary, ut::SAVE.get(lang)).clicked();
                cancel = widgets::button(ui, &t, widgets::Kind::Link, ut::CANCEL.get(lang)).clicked();
                reset = !current.is_empty() && widgets::button(ui, &t, widgets::Kind::Link, ut::NICK_RESET.get(lang)).clicked();
            },
        );
        save |= enter;
        let typed = n.text.trim().to_string();
        if reset || (save && typed.is_empty() && !current.is_empty()) {
            self.store.relations_write(RelationsCmd::Nickname { user_id: n.user_id, nickname: None });
        } else if save && !typed.is_empty() && typed != current {
            self.store.relations_write(RelationsCmd::Nickname { user_id: n.user_id, nickname: Some(typed) });
        }
        if save || reset || cancel || modal.closed {
            ctx.data_mut(|d| d.remove::<NickEdit>(nick_id()));
        } else {
            ctx.data_mut(|d| d.insert_temp(nick_id(), n));
        }
    }

    /// Ouvre la fenêtre du pseudo d'ami (aperçu, tests).
    pub fn show_friend_nick(&mut self, user_id: u64) {
        let text = self.store.relations.relation(user_id).and_then(|r| r.nickname.clone()).unwrap_or_default();
        self.ctx.data_mut(|d| d.insert_temp(nick_id(), NickEdit { user_id, text, fresh: true }));
    }

    /// Ouvre le menu d'un utilisateur (aperçu, tests).
    pub fn show_user_menu(&mut self, user_id: u64, name: &str, at: egui::Pos2) {
        open(&self.ctx, user_id, name, at);
    }
}
