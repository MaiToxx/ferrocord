//! Page « Rôles » (lot A3) : liste ordonnée (glisser-déposer pour changer
//! l'ordre), recherche, création, copie, suppression confirmée ; éditeur
//! d'un rôle en trois onglets (affichage, permissions, membres). Ce qu'un
//! rôle au-dessus du mien interdit n'est pas proposé ; rien ne part sans geste.

use super::super::{App, ToastText};
use super::{rgb, role_perms};
use crate::guild_icon::{IconPicker, Outcome, Picked};
use crate::i18n::table::roles::{f, p, t as rt};
use crate::i18n::Lang;
use crate::state::admin::roles::{self as admin, RoleAction, RoleExtra, RoleFields, RolesNotice, NAME_MAX, PALETTE};
use crate::state::model::{ImageHash, ImageRef, Role};
use crate::state::{Permissions, Problem};
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText, Sense};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Display,
    Permissions,
    Members,
}

/// Valeurs modifiables d'un rôle.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Values {
    name: String,
    color: u32,
    hoist: bool,
    mentionable: bool,
    permissions: Permissions,
    emoji: String,
    icon: Option<ImageHash>,
}

impl Values {
    fn of(r: &Role, x: &RoleExtra) -> Self {
        Values { name: r.name.clone(), color: r.color, hoist: x.hoist, mentionable: r.mentionable, permissions: r.permissions, emoji: x.unicode_emoji.clone().unwrap_or_default(), icon: x.icon }
    }
}

enum IconChange {
    Keep,
    Remove,
    New { picked: Picked, tex: Option<egui::TextureHandle> },
}

/// Brouillon de l'éditeur : `base` est le rôle tel que Discord l'a donné.
struct Draft {
    role: u64,
    base: Values,
    cur: Values,
    icon: IconChange,
}

impl Draft {
    fn dirty(&self) -> bool {
        self.cur != self.base || !matches!(self.icon, IconChange::Keep)
    }

    /// Champs changés seulement.
    fn fields(&self) -> RoleFields {
        let (b, c) = (&self.base, &self.cur);
        let emoji = c.emoji.trim();
        RoleFields {
            name: (c.name.trim() != b.name).then(|| c.name.trim().to_string()),
            permissions: (c.permissions != b.permissions).then_some(c.permissions),
            color: (c.color != b.color).then_some(c.color),
            hoist: (c.hoist != b.hoist).then_some(c.hoist),
            mentionable: (c.mentionable != b.mentionable).then_some(c.mentionable),
            icon: match &self.icon {
                IconChange::Keep => None,
                IconChange::Remove => Some(None),
                IconChange::New { picked, .. } => Some(Some(picked.data_uri.clone())),
            },
            unicode_emoji: (emoji != b.emoji).then(|| (!emoji.is_empty()).then(|| emoji.to_string())),
        }
    }
}

#[derive(Default)]
pub(in crate::ui) struct RolesUi {
    search: String,
    /// Rôle ouvert dans l'éditeur.
    open: Option<u64>,
    tab: Tab,
    draft: Option<Draft>,
    perm_search: String,
    /// « Supprimer » cliqué : rôle à confirmer.
    confirm_delete: Option<u64>,
    /// « Retirer » cliqué dans l'onglet Membres : (membre, rôle) à confirmer.
    confirm_remove: Option<(u64, u64)>,
    add_query: String,
    picker: Option<IconPicker>,
    icon_failed: bool,
    pub(super) members: super::members::MembersPage,
    pub(super) dialog: Option<super::member_roles::MemberDialog>,
}

impl RolesUi {
    /// Changement de page ou de serveur : l'état de la page est oublié.
    pub(super) fn reset(&mut self) {
        let (picker, dialog) = (self.picker.take(), self.dialog.take());
        *self = RolesUi { picker, dialog, ..Default::default() };
    }

    /// Une confirmation est ouverte (elle prend Échap).
    pub(super) fn dialog_open(&self) -> bool {
        self.confirm_delete.is_some() || self.confirm_remove.is_some() || self.dialog.is_some()
    }
}

/// Correspondance d'une recherche : sans casse.
pub(super) fn matches(hay: &str, q: &str) -> bool {
    q.is_empty() || hay.to_lowercase().contains(q)
}

fn action_word(lang: Lang, a: RoleAction) -> &'static str {
    match a {
        RoleAction::Create => rt::ACTION_CREATE.get(lang),
        RoleAction::Edit => rt::ACTION_EDIT.get(lang),
        RoleAction::Delete => rt::ACTION_DELETE.get(lang),
        RoleAction::Reorder => rt::ACTION_REORDER.get(lang),
        RoleAction::MemberRole => rt::ACTION_MEMBER_ROLE.get(lang),
        RoleAction::Nick => rt::ACTION_NICK.get(lang),
        RoleAction::Search => rt::ACTION_SEARCH.get(lang),
    }
}

/// Fin d'une action, en mots.
pub(super) fn notice_text(lang: Lang, n: &RolesNotice, h24: bool) -> String {
    match n {
        RolesNotice::Saved(RoleAction::Create) => rt::SAVED_CREATE.get(lang).into(),
        RolesNotice::Saved(RoleAction::Delete) => rt::SAVED_DELETE.get(lang).into(),
        RolesNotice::Saved(RoleAction::Reorder) => rt::SAVED_REORDER.get(lang).into(),
        RolesNotice::Saved(RoleAction::Nick) => rt::SAVED_NICK.get(lang).into(),
        RolesNotice::Saved(_) => rt::SAVED_EDIT.get(lang).into(),
        // L'A2F se dit seule : elle dit déjà quoi faire.
        RolesNotice::Failed { problem: Problem::MfaRequired, .. } => crate::i18n::table::admin::t::MFA_REQUIRED.get(lang).into(),
        RolesNotice::Failed { action, problem: Problem::Forbidden } => f::failed(lang, action_word(lang, *action), rt::REFUSED.get(lang)),
        RolesNotice::Failed { action, problem } => f::failed(lang, action_word(lang, *action), super::super::words::problem(lang, problem, h24)),
    }
}

/// Nom affiché d'un membre (pseudo, nom global, nom d'utilisateur) et son nom d'utilisateur.
pub(super) fn member_names(app: &App, guild_id: u64, user_id: u64) -> (String, String) {
    let people = app.store.people.lock();
    let user = people.user(user_id);
    let handle = user.map(|u| u.handle()).unwrap_or_default();
    let nick = people.member(guild_id, user_id).and_then(|m| m.nick.clone());
    let me = app.store.me.as_ref().filter(|u| u.id == user_id).map(|u| u.name.clone());
    let name = nick.or_else(|| user.map(|u| u.display_name().to_string())).or(me).unwrap_or_else(|| user_id.to_string());
    (name, handle)
}

/// Rôles connus d'un membre (`None` : inconnus).
pub(super) fn member_roles(app: &App, guild_id: u64, user_id: u64) -> Option<Vec<u64>> {
    if app.store.me.as_ref().is_some_and(|u| u.id == user_id)
        && let Some(m) = app.store.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.me.as_ref())
    {
        return Some(m.roles.clone());
    }
    app.store.people.lock().member(guild_id, user_id)?.roles.clone()
}

/// Pastille de couleur d'un rôle (`0` : couleur par défaut).
pub(super) fn dot(ui: &mut egui::Ui, t: &Tokens, color: u32) {
    let side = t.shapes.mention_dot * 2.0;
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
    let c = if color == 0 { t.colors.faint } else { rgb(color) };
    ui.painter().circle_filled(rect.center(), t.shapes.mention_dot, c);
}

/// Dialogue de confirmation : `Some(true)` confirmé, `Some(false)` annulé.
pub(super) fn confirm(ui: &mut egui::Ui, t: &Tokens, lang: Lang, id: &str, text: &str, yes: &str) -> Option<bool> {
    let mut out = None;
    let width = crate::ui::mobile::card_width(ui.ctx(), t, 1.0);
    let modal = crate::ui::widgets::modal(ui.ctx(), t, Id::unique(("ferrocord.admin.confirmer", id)), width, text, crate::i18n::t::CLOSE_TIP.get(lang), |_| {}, |ui| {
        if ui.add(crate::ui::form::danger(t, yes)).clicked() {
            out = Some(true);
        }
        if ui.add(crate::ui::form::link(t, rt::CANCEL.get(lang))).clicked() {
            out = Some(false);
        }
    });
    if modal.closed {
        out = out.or(Some(false));
    }
    out
}

impl App {
    /// Fins d'actions, rôle créé à ouvrir, icône choisie (chaque image).
    pub(super) fn roles_tick(&mut self) {
        while let Some(n) = self.store.roles_admin.take_notice() {
            if n == RolesNotice::Saved(RoleAction::Edit) {
                // Le rôle relu devient la base du brouillon.
                self.roles_admin.draft = None;
            }
            let text = notice_text(self.lang, &n, self.h24());
            self.notify(ToastText::Text(text));
        }
        if let Some((guild, role)) = self.store.roles_admin.take_created()
            && self.guild_settings.win.is_some_and(|s| s.guild_id == guild && s.page == super::Page::Roles)
        {
            self.roles_admin.open = Some(role);
            self.roles_admin.tab = Tab::Display;
            self.roles_admin.draft = None;
        }
        let out = self.roles_admin.picker.as_ref().and_then(IconPicker::poll);
        match (out, self.roles_admin.draft.as_mut()) {
            (Some(Outcome::Ready(picked)), Some(d)) => {
                d.icon = IconChange::New { picked, tex: None };
                d.cur.emoji.clear();
                self.roles_admin.icon_failed = false;
            }
            (Some(Outcome::Unreadable), _) => self.roles_admin.icon_failed = true,
            _ => {}
        }
    }

    fn pick_role_icon(&mut self) {
        let ctx = self.ctx.clone();
        let picker = self.roles_admin.picker.get_or_insert_with(|| IconPicker::new(Arc::new(move || ctx.request_repaint())));
        #[cfg(windows)]
        picker.pick(rt::ICON_PICK.get(self.lang));
        #[cfg(not(windows))]
        let _ = picker;
    }

    /// Ouvre la page Rôles sur l'éditeur d'un rôle (aperçu : onglet
    /// « affichage », « permissions » ou « membres ») ; `false` si impossible.
    pub fn show_role_editor(&mut self, guild_id: u64, role_id: u64, tab: &str) -> bool {
        let tab = match tab {
            "affichage" => Tab::Display,
            "permissions" => Tab::Permissions,
            "membres" => Tab::Members,
            _ => return false,
        };
        self.open_guild_settings(guild_id, Some(super::Page::Roles));
        if self.guild_settings.win.is_none_or(|w| w.guild_id != guild_id || w.page != super::Page::Roles) || !self.store.guilds.iter().any(|g| g.id == guild_id && g.roles.iter().any(|r| r.id == role_id)) {
            return false;
        }
        self.roles_admin.open = Some(role_id);
        self.roles_admin.tab = tab;
        true
    }
}

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64) {
    let exists = |app: &App, r: u64| app.store.guilds.iter().any(|g| g.id == guild_id && g.roles.iter().any(|x| x.id == r));
    if let Some(r) = app.roles_admin.open
        && !exists(app, r)
    {
        app.roles_admin.open = None;
        app.roles_admin.draft = None;
    }
    match app.roles_admin.open {
        Some(r) => editor(app, ui, t, guild_id, r),
        None => list(app, ui, t, guild_id),
    }
    delete_dialog(app, ui, t, guild_id);
    remove_dialog(app, ui, t, guild_id);
}

fn list(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64) {
    let lang = app.lang;
    let Some(g) = app.store.guilds.iter().find(|g| g.id == guild_id) else { return };
    let roles: Vec<Role> = admin::sorted(&g.roles).into_iter().cloned().collect();
    let auth = app.store.role_authority(guild_id);
    let can = app.store.can_manage_roles(guild_id);
    let counts = app.store.known_role_counts(guild_id);
    super::super::settings::note(ui, t, rt::ROLES_INTRO.get(lang));
    ui.add_space(t.spacing.item_y);
    let mut create = false;
    // Le bouton au bord droit de la page, le champ dans la place qui reste.
    crate::ui::widgets::field_row(ui, t, |ui| {
        if can {
            let busy = app.store.roles_admin.pending(RoleAction::Create, guild_id);
            create = ui.add_enabled(!busy, crate::ui::form::primary(t, rt::CREATE_ROLE.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        }
        crate::ui::form::field(ui, t, &mut app.roles_admin.search, rt::SEARCH_ROLES.get(lang), 0);
    });
    if create {
        app.store.create_role(guild_id, rt::NEW_ROLE_NAME.get(lang));
    }
    ui.add_space(t.spacing.item_y);
    // @everyone : ses permissions, en tête comme dans Discord.
    let mut open = None;
    if let Some(everyone) = roles.iter().find(|r| r.id == guild_id) {
        let resp = egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_row()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(rt::DEFAULT_PERMISSIONS.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
            ui.label(RichText::new(rt::EVERYONE_DETAIL.get(lang)).small().color(t.colors.muted));
        });
        if resp.response.interact(Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            open = Some((everyone.id, Tab::Permissions));
        }
    }
    ui.add_space(t.spacing.item_y);
    let q = app.roles_admin.search.trim().to_lowercase();
    let shown: Vec<&Role> = roles.iter().filter(|r| r.id != guild_id && matches(&r.name, &q)).collect();
    // Colonne des boutons de chaque ligne, de largeur fixe : le nombre de
    // membres tombe au même endroit sur toutes les lignes, sous son titre.
    let button_w = |ui: &egui::Ui, text: &str| ui.painter().layout_no_wrap(text.to_string(), egui::TextStyle::Button.resolve(ui.style()), t.colors.text).size().x + 2.0 * ui.spacing().button_padding.x;
    let actions_w = button_w(ui, rt::EDIT.get(lang)) + button_w(ui, "…") + ui.spacing().item_spacing.x;
    ui.horizontal(|ui| {
        ui.label(RichText::new(format!("{} — {}", rt::PAGE_ROLES.get(lang).to_uppercase(), shown.len())).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(actions_w + ui.spacing().item_spacing.x);
            ui.label(RichText::new(rt::PAGE_MEMBERS.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted)).on_hover_text(rt::KNOWN_TIP.get(lang));
        });
    });
    ui.separator();
    if shown.is_empty() {
        ui.label(RichText::new(rt::NO_ROLE_FOUND.get(lang)).color(t.colors.muted));
    }
    // Glisser-déposer : sans recherche (l'ordre affiché est l'ordre complet).
    let reorder = can && q.is_empty() && !app.store.roles_admin.pending(RoleAction::Reorder, guild_id);
    let mut drop: Option<(u64, usize)> = None;
    let mut menu: Option<(u64, bool)> = None;
    egui::ScrollArea::vertical().id_salt(("roles-liste", guild_id)).auto_shrink([false, false]).show(ui, |ui| {
        for (i, r) in shown.iter().enumerate() {
            let editable = auth.can_edit_role(r);
            let managed = app.store.roles_admin.extra(guild_id, r.id).is_some_and(|x| x.managed);
            let row = ui.horizontal(|ui| {
                ui.set_min_height(t.spacing.channel_row);
                // Poignée : seulement pour les rôles qu'on peut déplacer.
                let side = ui.spacing().interact_size.y;
                let (grip, grip_resp) = ui.allocate_exact_size(egui::vec2(side * 0.6, side), if reorder && editable { Sense::drag() } else { Sense::hover() });
                if reorder && editable {
                    let color = if grip_resp.hovered() || grip_resp.dragged() { t.colors.text_strong } else { t.colors.faint };
                    let rad = t.shapes.mention_dot * 0.6;
                    for (dx, dy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 0.0), (1.0, 0.0), (-1.0, 1.0), (1.0, 1.0)] {
                        ui.painter().circle_filled(grip.center() + egui::vec2(dx * rad * 2.0, dy * rad * 3.0), rad, color);
                    }
                    grip_resp.dnd_set_drag_payload(r.id);
                    grip_resp.on_hover_text(rt::DRAG_TIP.get(lang)).on_hover_cursor(egui::CursorIcon::Grab);
                }
                dot(ui, t, r.color);
                let name_color = if r.color == 0 { t.colors.text } else { rgb(r.color) };
                ui.add(egui::Label::new(RichText::new(&r.name).color(name_color)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.allocate_ui_with_layout(egui::vec2(actions_w, ui.available_height()), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.set_min_width(actions_w);
                    if editable {
                        ui.menu_button("…", |ui| {
                            if crate::ui::menu::item(ui, t, rt::DUPLICATE.get(lang), None, false).clicked() {
                                menu = Some((r.id, false));
                            }
                            if !managed && crate::ui::menu::item(ui, t, rt::DELETE.get(lang), None, true).clicked() {
                                menu = Some((r.id, true));
                            }
                        });
                    }
                    if ui.add(crate::ui::form::btn(t, rt::EDIT.get(lang))).clicked() {
                        open = Some((r.id, Tab::Display));
                    }
                    });
                    let n = counts.get(&r.id).copied().unwrap_or(0);
                    ui.label(RichText::new(n.to_string()).color(t.colors.muted)).on_hover_text(p::known_members(lang, n as u64));
                });
            });
            let resp = row.response;
            if resp.interact(Sense::click()).double_clicked() {
                open = Some((r.id, Tab::Display));
            }
            // Ligne survolée par une poignée : trait au-dessus ou au-dessous.
            if reorder && editable && resp.dnd_hover_payload::<u64>().is_some() {
                let below = ui.ctx().pointer_hover_pos().is_some_and(|p| p.y > resp.rect.center().y);
                let y = if below { resp.rect.bottom() } else { resp.rect.top() };
                ui.painter().hline(resp.rect.x_range(), y, egui::Stroke::new(t.shapes.focus, t.colors.accent));
            }
            if reorder
                && editable
                && let Some(moved) = resp.dnd_release_payload::<u64>()
            {
                let below = ui.ctx().pointer_interact_pos().is_some_and(|p| p.y > resp.rect.center().y);
                drop = Some((*moved, i + usize::from(below)));
            }
        }
    });
    if let Some((moved, at)) = drop
        && let Some(from) = shown.iter().position(|r| r.id == moved)
    {
        let to = if from < at { at - 1 } else { at };
        app.store.move_role(guild_id, moved, to);
    }
    match menu {
        Some((id, false)) => {
            app.store.duplicate_role(guild_id, id);
        }
        Some((id, true)) => app.roles_admin.confirm_delete = Some(id),
        None => {}
    }
    if let Some((id, tab)) = open {
        app.roles_admin.open = Some(id);
        app.roles_admin.tab = tab;
        app.roles_admin.draft = None;
        app.roles_admin.perm_search.clear();
        app.roles_admin.add_query.clear();
    }
}

fn editor(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64, role_id: u64) {
    let lang = app.lang;
    let Some(role) = app.store.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.roles.iter().find(|r| r.id == role_id)).cloned() else { return };
    let extra = app.store.roles_admin.extra(guild_id, role_id).cloned().unwrap_or_default();
    let auth = app.store.role_authority(guild_id);
    let editable = auth.can_edit_role(&role);
    let everyone = role.id == guild_id;
    let base = Values::of(&role, &extra);
    // Rôle changé ailleurs (passerelle) : un brouillon intact suit la nouvelle version.
    let stale = app.roles_admin.draft.as_ref().is_none_or(|d| d.role != role_id || (!d.dirty() && d.base != base));
    if stale {
        app.roles_admin.draft = Some(Draft { role: role_id, base: base.clone(), cur: base, icon: IconChange::Keep });
    }
    if everyone && app.roles_admin.tab != Tab::Permissions {
        app.roles_admin.tab = Tab::Permissions;
    }
    let shown_name = if everyone { "@everyone".to_string() } else { role.name.clone() };
    let mut back = false;
    ui.horizontal(|ui| {
        back = ui.add(crate::ui::form::btn(t, rt::BACK.get(lang))).clicked();
        ui.add(egui::Label::new(RichText::new(f::edit_role(lang, &shown_name)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).truncate());
    });
    if back {
        app.roles_admin.open = None;
        app.roles_admin.draft = None;
        return;
    }
    if !editable {
        ui.add(egui::Label::new(RichText::new(rt::LOCKED.get(lang)).color(t.colors.danger)).wrap());
    }
    if extra.managed {
        let kind = if extra.tags.bot_id.is_some() {
            rt::MANAGED_BOT
        } else if extra.tags.premium_subscriber {
            rt::MANAGED_BOOSTER
        } else {
            rt::MANAGED_OTHER
        };
        ui.label(RichText::new(kind.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
        super::super::settings::note(ui, t, rt::MANAGED.get(lang));
    }
    // Barre « modifications non enregistrées ».
    let busy = app.store.roles_admin.pending(RoleAction::Edit, guild_id);
    let (mut reset, mut save) = (false, false);
    if app.roles_admin.draft.as_ref().is_some_and(Draft::dirty) {
        egui::Frame::new().fill(t.colors.bg0).corner_radius(t.r_row()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(rt::UNSAVED.get(lang)).color(t.colors.text_strong));
                reset = ui.add(crate::ui::form::btn(t, rt::RESET.get(lang))).clicked();
                save = ui.add_enabled(!busy, crate::ui::form::primary(t, rt::SAVE.get(lang))).clicked();
                if busy {
                    ui.spinner();
                }
            });
        });
    }
    if reset {
        app.roles_admin.draft = None;
        app.roles_admin.icon_failed = false;
        return;
    }
    if save && let Some(d) = app.roles_admin.draft.as_ref() {
        let fields = d.fields();
        if !fields.is_empty() && !app.store.edit_role(guild_id, role_id, fields) {
            app.notify(rt::NAME_INVALID);
        }
    }
    // Onglets.
    let members_n = app.store.known_role_counts(guild_id).get(&role_id).copied().unwrap_or(0);
    crate::ui::mobile::row(ui, |ui| {
        let tabs: Vec<(Tab, String)> = if everyone {
            vec![(Tab::Permissions, rt::TAB_PERMISSIONS.get(lang).to_string())]
        } else {
            vec![(Tab::Display, rt::TAB_DISPLAY.get(lang).to_string()), (Tab::Permissions, rt::TAB_PERMISSIONS.get(lang).to_string()), (Tab::Members, f::manage_members(lang, members_n))]
        };
        for (tab, label) in tabs {
            if ui.selectable_label(app.roles_admin.tab == tab, label).clicked() {
                app.roles_admin.tab = tab;
            }
        }
    });
    // Pas de filet ici : le contenu de l'onglet commence par le sien (il y en avait deux).
    ui.add_space(t.spacing.item_y);
    if everyone {
        super::super::settings::note(ui, t, rt::EVERYONE_NOTE.get(lang));
    }
    let tab = app.roles_admin.tab;
    egui::ScrollArea::vertical().id_salt(("role-editeur", role_id, tab as u8)).auto_shrink([false, false]).show(ui, |ui| match tab {
        Tab::Display => display_tab(app, ui, t, guild_id, &role, &extra, editable),
        Tab::Permissions => {
            let grant = auth.grantable();
            let App { roles_admin, .. } = app;
            if let Some(d) = roles_admin.draft.as_mut() {
                role_perms::show(ui, t, lang, &mut d.cur.permissions, grant, editable, &mut roles_admin.perm_search);
            }
        }
        Tab::Members => members_tab(app, ui, t, guild_id, &role, &extra),
    });
}

fn display_tab(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64, role: &Role, extra: &RoleExtra, editable: bool) {
    let lang = app.lang;
    let boost = app.store.roles_admin.boost(guild_id);
    // Niveau de boost : une seule source, le modèle `Guild` (lot A8 + A9).
    let tier = app.store.guilds.iter().find(|g| g.id == guild_id).map_or(0, |g| g.premium_tier);
    let can_pick = IconPicker::CAN_PICK && !app.roles_admin.picker.as_ref().is_some_and(IconPicker::busy);
    let mut pick = false;
    let App { roles_admin, images, .. } = app;
    let Some(d) = roles_admin.draft.as_mut() else { return };
    ui.add_enabled_ui(editable, |ui| {
        super::super::settings::section(ui, t, rt::ROLE_NAME.get(lang));
        ui.add_enabled(!extra.managed, egui::TextEdit::singleline(&mut d.cur.name).char_limit(NAME_MAX).desired_width(f32::INFINITY));

        super::super::settings::section(ui, t, rt::ROLE_COLOR.get(lang));
        super::super::settings::note(ui, t, rt::COLOR_DETAIL.get(lang));
        let side = ui.spacing().interact_size.y;
        // Disposition mobile : autant de pastilles par ligne que l'écran en tient.
        let gap = ui.spacing().item_spacing.x;
        let per_row = if crate::ui::mobile::is_active(ui.ctx()) { (((ui.available_width() - side * 2.6 - 3.0 * gap) / (side * 0.75 + gap)).floor() as usize).clamp(3, 10) } else { 10 };
        ui.horizontal(|ui| {
            if swatch(ui, t, side * 1.6, None, d.cur.color == 0).on_hover_text(rt::DEFAULT_COLOR.get(lang)).clicked() {
                d.cur.color = 0;
            }
            let mut rgb3 = [(d.cur.color >> 16) as u8, (d.cur.color >> 8) as u8, d.cur.color as u8];
            if egui::color_picker::color_edit_button_srgb(ui, &mut rgb3).on_hover_text(rt::CUSTOM_COLOR.get(lang)).changed() {
                d.cur.color = (u32::from(rgb3[0]) << 16) | (u32::from(rgb3[1]) << 8) | u32::from(rgb3[2]);
            }
            ui.vertical(|ui| {
                for row in PALETTE.chunks(per_row) {
                    ui.horizontal(|ui| {
                        for c in row {
                            if swatch(ui, t, side * 0.75, Some(*c), d.cur.color == *c).clicked() {
                                d.cur.color = *c;
                            }
                        }
                    });
                }
            });
        });
        if let Some(second) = extra.secondary_color {
            super::super::settings::section(ui, t, rt::GRADIENT.get(lang));
            ui.horizontal(|ui| {
                for c in [Some(role.color), Some(second), extra.tertiary_color].into_iter().flatten() {
                    swatch(ui, t, side * 0.75, Some(c), false);
                }
            });
            super::super::settings::note(ui, t, rt::GRADIENT_DETAIL.get(lang));
        }

        super::super::settings::section(ui, t, rt::ROLE_ICON.get(lang));
        super::super::settings::note(ui, t, rt::ROLE_ICON_DETAIL.get(lang));
        let has_icon = match &d.icon {
            IconChange::Keep => d.base.icon.is_some(),
            IconChange::Remove => false,
            IconChange::New { .. } => true,
        };
        if !boost.role_icons && !has_icon && d.base.emoji.is_empty() {
            super::super::settings::note(ui, t, rt::ROLE_ICON_LOCKED.get(lang));
            super::super::settings::note(ui, t, &f::tier(lang, tier));
        } else {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side * 1.6), Sense::hover());
                match &mut d.icon {
                    IconChange::New { picked, tex } => {
                        let tex = tex.get_or_insert_with(|| ui.ctx().load_texture("icone-role", picked.image.clone(), egui::TextureOptions::LINEAR));
                        ui.painter().add(crate::images::textured(rect, t.r_row(), tex.id()));
                    }
                    IconChange::Keep if d.base.icon.is_some() => {
                        let image = d.base.icon.map(|hash| ImageRef::RoleIcon { role_id: role.id, hash });
                        if !super::super::util::paint_image(ui, images, rect, image, t.r_row()) {
                            ui.painter().rect_filled(rect, t.r_row(), t.colors.bg3);
                        }
                    }
                    _ => {
                        ui.painter().rect_filled(rect, t.r_row(), t.colors.bg3);
                    }
                }
                ui.vertical(|ui| {
                    if boost.role_icons && IconPicker::CAN_PICK {
                        pick = ui.add_enabled(can_pick, crate::ui::form::btn(t, rt::ICON_PICK.get(lang))).clicked();
                    }
                    if has_icon && ui.add(crate::ui::form::btn(t, rt::ICON_REMOVE.get(lang))).clicked() {
                        d.icon = if d.base.icon.is_some() { IconChange::Remove } else { IconChange::Keep };
                    }
                });
            });
            if roles_admin.icon_failed {
                ui.label(RichText::new(rt::ICON_UNREADABLE.get(lang)).color(t.colors.danger));
            }
            if boost.role_icons || !d.base.emoji.is_empty() {
                ui.horizontal(|ui| {
                    ui.label(rt::EMOJI.get(lang));
                    ui.add(egui::TextEdit::singleline(&mut d.cur.emoji).hint_text(rt::EMOJI_HINT.get(lang)).char_limit(16).desired_width(side * 6.0));
                });
            }
        }

        ui.add_space(t.spacing.category_gap);
        crate::ui::settings::shell::switch_label(ui, &mut d.cur.hoist, rt::HOIST.get(lang));
        crate::ui::settings::shell::switch_label(ui, &mut d.cur.mentionable, rt::MENTIONABLE.get(lang));
        super::super::settings::note(ui, t, rt::MENTIONABLE_DETAIL.get(lang));
    });
    if pick {
        app.pick_role_icon();
    }
}

/// Case de couleur (`None` : couleur par défaut) ; cerclée si choisie.
fn swatch(ui: &mut egui::Ui, t: &Tokens, side: f32, color: Option<u32>, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
    let fill = color.map_or(t.colors.muted, rgb);
    ui.painter().rect_filled(rect, t.r_row(), fill);
    if selected || resp.hovered() {
        ui.painter().rect_stroke(rect, t.r_row(), egui::Stroke::new(t.shapes.focus, t.colors.text_strong), egui::StrokeKind::Outside);
    }
    resp
}

fn members_tab(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64, role: &Role, extra: &RoleExtra) {
    let lang = app.lang;
    let can = app.store.can_assign(guild_id, role);
    if extra.managed {
        super::super::settings::note(ui, t, rt::MANAGED.get(lang));
    }
    let known = app.store.known_members(guild_id);
    let mut with: Vec<(u64, String, String)> = Vec::new();
    let mut without: Vec<(u64, String, String)> = Vec::new();
    let q = app.roles_admin.add_query.trim().to_lowercase();
    for u in known {
        let Some(roles) = member_roles(app, guild_id, u) else { continue };
        let (name, handle) = member_names(app, guild_id, u);
        if roles.contains(&role.id) {
            with.push((u, name, handle));
        } else if can && !q.is_empty() && (matches(&name, &q) || matches(&handle, &q)) {
            without.push((u, name, handle));
        }
    }
    with.sort_by_key(|(_, n, _)| n.to_lowercase());
    without.sort_by_key(|(_, n, _)| n.to_lowercase());
    without.truncate(50);
    let (mut add, mut remove, mut search) = (None, None, false);
    if can {
        super::super::settings::section(ui, t, rt::ADD_MEMBERS.get(lang));
        let loading = app.store.roles_admin.search(guild_id).is_some_and(|s| s.loading);
        ui.horizontal(|ui| {
            let w = (ui.available_width() * 0.6).max(t.layout.button_height);
            ui.add(egui::TextEdit::singleline(&mut app.roles_admin.add_query).hint_text(rt::SEARCH_MEMBERS.get(lang)).desired_width(w));
            search = ui.add_enabled(!q.is_empty() && !loading, crate::ui::form::btn(t, rt::SEARCH_SERVER.get(lang))).clicked();
            if loading {
                ui.spinner();
            }
        });
        if !q.is_empty() && without.is_empty() {
            ui.label(RichText::new(rt::NO_MEMBER_FOUND.get(lang)).color(t.colors.muted));
        }
        for (u, name, handle) in &without {
            let pending = app.store.roles_admin.member_role_pending(guild_id, *u, role.id);
            ui.horizontal(|ui| {
                ui.label(RichText::new(name).color(t.colors.text_strong));
                ui.label(RichText::new(handle).small().color(t.colors.faint));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!pending, crate::ui::form::btn(t, rt::ADD.get(lang))).clicked() {
                        add = Some(*u);
                    }
                });
            });
        }
        ui.separator();
    }
    if with.is_empty() {
        super::super::settings::note(ui, t, rt::NO_ROLE_MEMBERS.get(lang));
    }
    for (u, name, handle) in &with {
        let pending = app.store.roles_admin.member_role_pending(guild_id, *u, role.id);
        ui.horizontal(|ui| {
            dot(ui, t, role.color);
            ui.label(RichText::new(name).color(t.colors.text_strong));
            ui.label(RichText::new(handle).small().color(t.colors.faint));
            if can {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled(!pending, crate::ui::form::danger(t, rt::REMOVE.get(lang))).clicked() {
                        remove = Some(*u);
                    }
                    if pending {
                        ui.spinner();
                    }
                });
            }
        });
    }
    if let Some(u) = add {
        app.store.set_member_role(guild_id, u, role.id, true);
    }
    if let Some(u) = remove {
        app.roles_admin.confirm_remove = Some((u, role.id));
    }
    if search {
        app.store.search_members(guild_id, &q);
    }
}

fn delete_dialog(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64) {
    let Some(id) = app.roles_admin.confirm_delete else { return };
    let Some(name) = app.store.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.roles.iter().find(|r| r.id == id)).map(|r| r.name.clone()) else {
        app.roles_admin.confirm_delete = None;
        return;
    };
    let lang = app.lang;
    match confirm(ui, t, lang, "supprimer-role", &f::delete_question(lang, &name), rt::DELETE.get(lang)) {
        Some(true) => {
            app.roles_admin.confirm_delete = None;
            app.store.delete_role(guild_id, id);
            if app.roles_admin.open == Some(id) {
                app.roles_admin.open = None;
                app.roles_admin.draft = None;
            }
        }
        Some(false) => app.roles_admin.confirm_delete = None,
        None => {}
    }
}

fn remove_dialog(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64) {
    let Some((user, role)) = app.roles_admin.confirm_remove else { return };
    let Some(role_name) = app.store.guilds.iter().find(|g| g.id == guild_id).and_then(|g| g.roles.iter().find(|r| r.id == role)).map(|r| r.name.clone()) else {
        app.roles_admin.confirm_remove = None;
        return;
    };
    let lang = app.lang;
    let (member, _) = member_names(app, guild_id, user);
    match confirm(ui, t, lang, "retirer-role", &f::remove_question(lang, &role_name, &member), rt::REMOVE.get(lang)) {
        Some(true) => {
            app.roles_admin.confirm_remove = None;
            app.store.set_member_role(guild_id, user, role, false);
        }
        Some(false) => app.roles_admin.confirm_remove = None,
        None => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_sends_only_changed_fields() {
        let role = Role { id: 5, name: "Lecteurs".into(), color: 0x3498db, position: 2, permissions: Permissions::VIEW_CHANNEL, mentionable: false };
        let extra = RoleExtra { unicode_emoji: Some("x".into()), ..Default::default() };
        let base = Values::of(&role, &extra);
        let mut d = Draft { role: 5, base: base.clone(), cur: base, icon: IconChange::Keep };
        assert!(!d.dirty());
        assert!(d.fields().is_empty());
        d.cur.name = " Lecteurs ".into();
        assert!(d.fields().is_empty(), "espaces autour du nom ignorés");
        d.cur.color = 0;
        d.cur.permissions = Permissions::NONE;
        d.cur.emoji = "  ".into();
        assert_eq!(d.fields(), RoleFields { color: Some(0), permissions: Some(Permissions::NONE), unicode_emoji: Some(None), ..Default::default() });
        d.icon = IconChange::Remove;
        assert_eq!(d.fields().icon, Some(None));
        assert!(d.dirty());
    }

    #[test]
    fn notices_in_words() {
        let fr = Lang::Fr;
        assert_eq!(notice_text(fr, &RolesNotice::Saved(RoleAction::Create), true), "Rôle créé.");
        let mfa = notice_text(fr, &RolesNotice::Failed { action: RoleAction::Edit, problem: Problem::MfaRequired }, true);
        assert!(mfa.starts_with("L'authentification à deux facteurs est requise"), "{mfa}");
        let refused = notice_text(Lang::En, &RolesNotice::Failed { action: RoleAction::Delete, problem: Problem::Forbidden }, true);
        assert_eq!(refused, "Deleting the role failed: Discord refused (missing permission or role too high)");
    }
}
