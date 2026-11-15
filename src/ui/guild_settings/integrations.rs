//! Page « Intégrations » : robots et applications, comptes liés (Twitch,
//! YouTube…), retrait confirmé, et permissions des commandes d'une
//! application en lecture seule (les modifier demande une autorisation
//! OAuth2 de l'application, que Ferrocord n'a pas).

use super::super::settings::{note, section};
use super::{loading_or_failed, App};
use crate::i18n::table::admin::{f, t as tx};
use crate::i18n::Lang;
use crate::state::admin::guild::Load;
use crate::state::admin::integrations::{CommandPerms, Integration, PermTarget};
use crate::state::interactions::Scope;
use crate::state::Store;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

#[derive(Default)]
pub(super) struct IntegrationsUi {
    /// Retrait demandé : intégration à confirmer, et raison.
    confirm: Option<u64>,
    reason: String,
    /// Application dont les permissions de commandes sont affichées.
    perms_of: Option<u64>,
}

/// Cible d'une permission en mots (constantes documentées : `@everyone` =
/// identifiant du serveur, « tous les salons » = identifiant du serveur - 1).
fn target_text(store: &Store, lang: Lang, guild: u64, target: PermTarget) -> String {
    let g = store.guilds.iter().find(|g| g.id == guild);
    match target {
        PermTarget::Role(id) if id == guild => tx::EVERYONE.get(lang).to_string(),
        PermTarget::Role(id) => g.and_then(|g| g.roles.iter().find(|r| r.id == id)).map_or_else(|| f::role(lang, id), |r| f::role(lang, &r.name)),
        PermTarget::Channel(id) if id == guild.wrapping_sub(1) => tx::ALL_CHANNELS.get(lang).to_string(),
        PermTarget::Channel(id) => store.channel_name(id).map_or_else(|| f::channel(lang, id), |n| f::channel(lang, n)),
        PermTarget::User(id) => store.people.lock().user(id).map_or_else(|| f::user(lang, id), |u| u.display_name().to_string()),
    }
}

impl App {
    pub(super) fn page_guild_integrations(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        let h24 = self.h24();
        if self.store.integrations_wanted() {
            self.store.load_integrations(guild);
        }
        note(ui, t, tx::INTEGRATIONS_TIP.get(lang));
        if ui.add(crate::ui::form::btn(t, tx::REFRESH.get(lang))).clicked() {
            self.store.load_integrations(guild);
        }
        let list = self.store.guild_admin.integrations.clone();
        if loading_or_failed(ui, t, lang, h24, list.as_ref()) {
            return;
        }
        let Some(Load::Ready(list)) = list else { return };
        if list.is_empty() {
            note(ui, t, tx::NO_INTEGRATIONS.get(lang));
            return;
        }
        let (apps, linked): (Vec<&Integration>, Vec<&Integration>) = list.iter().partition(|i| i.application.is_some());
        for (title, group) in [(tx::BOTS, apps), (tx::LINKED, linked)] {
            if group.is_empty() {
                continue;
            }
            section(ui, t, title.get(lang));
            for i in group {
                ui.push_id(i.id, |ui| self.integration_card(ui, t, guild, i));
            }
        }
    }

    fn integration_card(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, i: &Integration) {
        let lang = self.lang;
        let removing = self.store.guild_admin.removing(i.id);
        let (mut ask, mut perms) = (false, false);
        egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(t.r_card()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&i.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                ui.label(RichText::new(&i.kind).small().color(t.colors.muted));
                if !i.enabled {
                    ui.label(RichText::new(tx::DISABLED.get(lang)).small().color(t.colors.danger));
                }
            });
            let mut details = Vec::new();
            if !i.account.is_empty() && i.application.is_none() {
                details.push(i.account.clone());
            }
            if let Some(u) = &i.user {
                details.push(f::added_by(lang, u));
            }
            if !details.is_empty() {
                note(ui, t, &details.join(" · "));
            }
            if let Some(app) = &i.application
                && !app.description.is_empty()
            {
                note(ui, t, &app.description);
            }
            crate::ui::mobile::row(ui, |ui| {
                if i.application.is_some() {
                    perms = ui.add(crate::ui::form::btn(t, tx::SHOW_PERMS.get(lang))).clicked();
                }
                ask = ui.add_enabled(!removing, crate::ui::form::danger(t, tx::REMOVE.get(lang))).clicked();
                if removing {
                    ui.spinner();
                }
            });
            self.remove_confirm(ui, t, guild, i);
            if let Some(app) = &i.application
                && self.guild_settings.integrations.perms_of == Some(app.id)
            {
                self.command_perms_view(ui, t, guild, app.id);
            }
        });
        let s = &mut self.guild_settings.integrations;
        if ask {
            s.confirm = Some(i.id);
            s.reason.clear();
        }
        if perms && let Some(app) = &i.application {
            if s.perms_of == Some(app.id) {
                s.perms_of = None;
            } else {
                s.perms_of = Some(app.id);
                self.store.load_command_perms(guild, app.id);
                // Noms des commandes : index des commandes du serveur (lot T11).
                if let Some(c) = self.store.guilds.iter().find(|g| g.id == guild).and_then(|g| g.channels.first()).map(|c| c.id) {
                    self.store.want_commands(c);
                }
            }
        }
        ui.add_space(t.spacing.item_y);
    }

    fn remove_confirm(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, i: &Integration) {
        let lang = self.lang;
        let s = &mut self.guild_settings.integrations;
        if s.confirm != Some(i.id) {
            return;
        }
        ui.separator();
        ui.add(egui::Label::new(RichText::new(f::remove_question(lang, &i.name)).color(t.colors.text_strong)).wrap());
        crate::ui::form::field(ui, t, &mut s.reason, tx::REASON.get(lang), 512);
        let (mut yes, mut no) = (false, false);
        ui.horizontal(|ui| {
            yes = ui.add(crate::ui::form::danger(t, tx::CONFIRM_REMOVE.get(lang))).clicked();
            no = ui.add(crate::ui::form::btn(t, tx::CANCEL.get(lang))).clicked();
        });
        if no {
            s.confirm = None;
        } else if yes {
            let reason = Some(std::mem::take(&mut s.reason));
            s.confirm = None;
            self.store.remove_integration(guild, i.id, reason);
        }
    }

    fn command_perms_view(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, app: u64) {
        let lang = self.lang;
        let h24 = self.h24();
        ui.separator();
        ui.label(RichText::new(tx::COMMAND_PERMS.get(lang)).family(crate::ui::fonts::demi()).strong());
        note(ui, t, tx::PERMS_READ_ONLY.get(lang));
        let l = self.store.guild_admin.command_perms.get(&app).cloned();
        if loading_or_failed(ui, t, lang, h24, l.as_ref()) {
            return;
        }
        let Some(Load::Ready(list)) = l else { return };
        if list.iter().all(|c| c.perms.is_empty()) {
            note(ui, t, tx::NO_OVERRIDES.get(lang));
            return;
        }
        let index = self.store.interactions.index(Scope::Guild(guild));
        let name = |c: &CommandPerms| {
            if c.command_id == app {
                return tx::ALL_COMMANDS.get(lang).to_string();
            }
            index.and_then(|i| i.commands.iter().find(|x| x.id == c.command_id)).map_or_else(|| f::command(lang, c.command_id), |x| f::command(lang, &x.name))
        };
        for c in list.iter().filter(|c| !c.perms.is_empty()) {
            ui.label(RichText::new(name(c)).color(t.colors.text_strong));
            for p in &c.perms {
                let state = if p.allowed { (tx::ALLOWED, t.colors.muted) } else { (tx::DENIED, t.colors.danger) };
                ui.horizontal(|ui| {
                    ui.label(target_text(&self.store, lang, guild, p.target));
                    ui.label(RichText::new(state.0.get(lang)).color(state.1));
                });
            }
        }
    }
}
