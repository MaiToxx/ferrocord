//! Page « Webhooks » : liste du serveur (ou des salons où j'ai la
//! permission), création, nom, avatar et salon, adresse masquée qu'on copie
//! comme un secret, suppression confirmée (avec raison pour le journal
//! d'audit). [`show_for_channel`] l'ouvre filtrée sur un salon (point
//! d'appel « Intégrations » de la fenêtre d'un salon).

use super::super::appearance::row;
use super::super::settings::{note, section};
use super::{loading_or_failed, App, GuildSettings, Page};
use crate::avatar::{Avatar, Done};
use crate::i18n::table::admin::{f, t as tx};
use crate::state::admin::guild::Load;
use crate::state::admin::webhooks::{HookKind, HookScope, Webhook, NAME_MAX};
use crate::state::crop::{Crop, ImageKind};
use crate::state::model::ChannelKind;
use crate::state::perms::Permissions;
use crate::state::Store;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use std::sync::Arc;

/// Salons où je peux gérer les webhooks (inconnu : oui), dans l'ordre de la liste.
pub(super) fn channels(store: &Store, guild: u64) -> impl Iterator<Item = (u64, String)> + '_ {
    store.guilds.iter().filter(move |g| g.id == guild).flat_map(|g| g.channels.iter()).filter(move |c| c.kind != ChannelKind::Category && store.can_manage_webhooks(c.id)).map(|c| (c.id, c.name.clone()))
}

/// Ouvre la page Webhooks filtrée sur ce salon ; `false` si ce n'est pas permis.
pub(in crate::ui) fn show_for_channel(app: &mut App, channel_id: u64) -> bool {
    let Some(guild) = app.store.interaction_guild(channel_id) else { return false };
    if !app.store.can_manage_webhooks(channel_id) {
        return false;
    }
    app.open_guild_settings(guild, Some(Page::Webhooks));
    if app.guild_settings.win != Some(GuildSettings { guild_id: guild, page: Page::Webhooks }) {
        return false;
    }
    app.guild_settings.hooks.channel = Some(channel_id);
    true
}

/// Modification d'un webhook en cours.
struct HookDraft {
    id: u64,
    name: String,
    channel: Option<u64>,
    /// Nouvel avatar (`Some(None)` : retiré).
    avatar: Option<Option<String>>,
}

#[derive(Default)]
pub(super) struct HooksUi {
    /// Filtre : un salon, ou tous.
    channel: Option<u64>,
    new_name: String,
    new_channel: Option<u64>,
    editing: Option<HookDraft>,
    /// Webhook dont l'adresse est affichée (un seul à la fois).
    shown: Option<u64>,
    /// Suppression demandée : webhook et raison.
    confirm: Option<u64>,
    reason: String,
    worker: Option<Avatar>,
}

impl App {
    /// Ouvre la page filtrée sur un salon (fenêtre d'un salon, aperçu).
    pub fn show_channel_webhooks(&mut self, channel_id: u64) -> bool {
        let ok = show_for_channel(self, channel_id);
        self.flush_commands();
        ok
    }

    fn hooks_worker(&mut self) -> &Avatar {
        let ctx = self.ctx.clone();
        self.guild_settings.hooks.worker.get_or_insert_with(|| Avatar::new(Arc::new(move || ctx.request_repaint())))
    }

    /// Avatar choisi : recadré au centre et codé, puis gardé dans le brouillon.
    fn hooks_poll(&mut self) {
        let Some(w) = &self.guild_settings.hooks.worker else { return };
        let done = w.poll();
        for d in done {
            match d {
                Done::Picked { image, kind, .. } => {
                    let w = self.hooks_worker();
                    w.encode(image, Crop::default(), kind, None);
                }
                Done::Encoded { data, .. } => {
                    if let Some(e) = &mut self.guild_settings.hooks.editing {
                        e.avatar = Some(Some(data));
                    }
                }
                Done::Unreadable { .. } => self.notify(tx::IMAGE_UNREADABLE),
            }
        }
    }

    pub(super) fn page_guild_webhooks(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        let h24 = self.h24();
        self.hooks_poll();
        let allowed: Vec<(u64, String)> = channels(&self.store, guild).collect();
        let whole = self.store.may_in_guild(guild, Permissions::MANAGE_WEBHOOKS);
        let ui_state = &mut self.guild_settings.hooks;
        if ui_state.channel.is_some_and(|c| !allowed.iter().any(|a| a.0 == c)) {
            ui_state.channel = None;
        }
        // Sans la permission sur tout le serveur : un salon à la fois.
        if !whole && ui_state.channel.is_none() {
            ui_state.channel = allowed.first().map(|c| c.0);
        }
        let filter = ui_state.channel;
        let scope = match filter {
            Some(c) if !whole => HookScope::Channel(c),
            _ => HookScope::Guild(guild),
        };
        if self.store.webhooks_wanted(scope) {
            self.store.load_webhooks(scope);
        }
        note(ui, t, tx::WEBHOOKS_TIP.get(lang));

        // Filtre par salon.
        let name_of = |c: u64| allowed.iter().find(|a| a.0 == c).map(|a| a.1.clone()).unwrap_or_default();
        let mut refresh = false;
        ui.horizontal(|ui| {
            let shown = filter.map_or_else(|| tx::ALL_CHANNELS_HOOKS.get(lang).to_string(), |c| f::channel_hooks(lang, name_of(c)));
            let hooks = &mut self.guild_settings.hooks;
            egui::ComboBox::from_id_salt("webhooks_filtre").selected_text(shown).show_ui(ui, |ui| {
                if whole {
                    ui.selectable_value(&mut hooks.channel, None, tx::ALL_CHANNELS_HOOKS.get(lang));
                }
                for (id, name) in &allowed {
                    ui.selectable_value(&mut hooks.channel, Some(*id), f::channel(lang, name));
                }
            });
            refresh = ui.add(crate::ui::form::btn(t, tx::REFRESH.get(lang))).clicked();
        });
        if refresh {
            self.store.load_webhooks(scope);
        }

        self.new_webhook_form(ui, t, guild, &allowed);

        let list = self.store.guild_admin.hooks.get(&scope).cloned();
        if loading_or_failed(ui, t, lang, h24, list.as_ref()) {
            return;
        }
        let Some(Load::Ready(list)) = list else { return };
        let list: Vec<Webhook> = list.into_iter().filter(|h| filter.is_none_or(|c| h.channel_id == Some(c))).collect();
        section(ui, t, tx::PAGE_WEBHOOKS.get(lang));
        if list.is_empty() {
            note(ui, t, tx::NO_WEBHOOKS.get(lang));
        }
        for h in &list {
            ui.push_id(h.id, |ui| self.webhook_card(ui, t, guild, h, &allowed));
        }
    }

    fn new_webhook_form(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, allowed: &[(u64, String)]) {
        let lang = self.lang;
        section(ui, t, tx::NEW_WEBHOOK.get(lang));
        let busy = self.store.guild_admin.hook_busy(0);
        let h = &mut self.guild_settings.hooks;
        if h.new_channel.is_none_or(|c| !allowed.iter().any(|a| a.0 == c)) {
            h.new_channel = h.channel.or_else(|| allowed.first().map(|a| a.0));
        }
        row(ui, tx::WEBHOOK_NAME.get(lang), "", |ui| {
            crate::ui::form::field(ui, t, &mut h.new_name, "", NAME_MAX);
        });
        row(ui, tx::WEBHOOK_CHANNEL.get(lang), "", |ui| {
            let shown = h.new_channel.and_then(|c| allowed.iter().find(|a| a.0 == c)).map(|a| f::channel(lang, &a.1)).unwrap_or_default();
            egui::ComboBox::from_id_salt("webhook_nouveau_salon").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for (id, name) in allowed {
                    ui.selectable_value(&mut h.new_channel, Some(*id), f::channel(lang, name));
                }
            });
        });
        let ok = !h.new_name.trim().is_empty() && h.new_channel.is_some();
        let mut create = false;
        ui.horizontal(|ui| {
            create = ui.add_enabled(ok && !busy, crate::ui::form::primary(t, tx::NEW_WEBHOOK.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        });
        if create && let Some(c) = h.new_channel {
            let name = h.new_name.clone();
            if self.store.create_webhook(guild, c, &name, None) {
                self.guild_settings.hooks.new_name.clear();
            }
        }
    }

    fn webhook_card(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, h: &Webhook, allowed: &[(u64, String)]) {
        let lang = self.lang;
        let busy = self.store.guild_admin.hook_busy(h.id);
        let fresh = self.store.guild_admin.created_hook == Some(h.id);
        let channel_name = h.channel_id.and_then(|c| allowed.iter().find(|a| a.0 == c).map(|a| a.1.clone()).or_else(|| self.store.channel_name(c).map(str::to_string)));
        let (mut copy, mut toggle_url, mut edit, mut ask_delete) = (false, false, false, false);
        let stroke = if fresh { egui::Stroke::new(t.shapes.border, t.colors.accent) } else { t.border() };
        egui::Frame::new().fill(t.colors.bg1).stroke(stroke).corner_radius(t.r_card()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(&h.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                if let Some(c) = &channel_name {
                    ui.label(RichText::new(f::channel(lang, c)).color(t.colors.muted));
                }
            });
            let mut details = Vec::new();
            match h.kind {
                HookKind::Follower => details.push(h.source.as_ref().map_or_else(|| tx::FOLLOWER.get(lang).to_string(), |s| f::follows(lang, s))),
                HookKind::Application => details.push(tx::APP_WEBHOOK.get(lang).to_string()),
                HookKind::Incoming | HookKind::Other => {}
            }
            if let Some(c) = &h.creator {
                details.push(f::created_by(lang, c));
            }
            if !details.is_empty() {
                note(ui, t, &details.join(" · "));
            }
            if h.url.is_some() {
                let shown = self.guild_settings.hooks.shown == Some(h.id);
                match (&h.url, shown) {
                    (Some(url), true) => {
                        ui.add(egui::Label::new(RichText::new(url.expose()).monospace().small()).wrap());
                    }
                    _ => note(ui, t, tx::URL_HIDDEN.get(lang)),
                }
                ui.horizontal(|ui| {
                    copy = ui.add(crate::ui::form::btn(t, tx::COPY_URL.get(lang))).clicked();
                    toggle_url = ui.add(crate::ui::form::btn(t, if shown { tx::HIDE } else { tx::SHOW }.get(lang))).clicked();
                });
            }
            ui.horizontal(|ui| {
                edit = ui.add_enabled(!busy && h.kind != HookKind::Application, crate::ui::form::btn(t, tx::EDIT.get(lang))).clicked();
                ask_delete = ui.add_enabled(!busy, crate::ui::form::danger(t, tx::DELETE_WEBHOOK.get(lang))).clicked();
                if busy {
                    ui.spinner();
                }
            });
            self.webhook_editor(ui, t, guild, h, allowed);
            self.webhook_delete_confirm(ui, t, guild, h);
        });
        let hs = &mut self.guild_settings.hooks;
        if toggle_url {
            hs.shown = if hs.shown == Some(h.id) { None } else { Some(h.id) };
        }
        if edit {
            hs.editing = match &hs.editing {
                Some(e) if e.id == h.id => None,
                _ => Some(HookDraft { id: h.id, name: h.name.clone(), channel: h.channel_id, avatar: None }),
            };
        }
        if ask_delete {
            hs.confirm = Some(h.id);
            hs.reason.clear();
        }
        if copy && let Some(url) = &h.url {
            ui.ctx().copy_text(url.expose().to_string());
            self.notify(tx::URL_COPIED);
        }
        ui.add_space(t.spacing.item_y);
    }

    fn webhook_editor(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, h: &Webhook, allowed: &[(u64, String)]) {
        let lang = self.lang;
        let picking = self.guild_settings.hooks.worker.as_ref().is_some_and(Avatar::picking);
        let Some(e) = self.guild_settings.hooks.editing.as_mut().filter(|e| e.id == h.id) else { return };
        ui.separator();
        row(ui, tx::WEBHOOK_NAME.get(lang), "", |ui| {
            crate::ui::form::field(ui, t, &mut e.name, "", NAME_MAX);
        });
        row(ui, tx::WEBHOOK_CHANNEL.get(lang), "", |ui| {
            let shown = e.channel.and_then(|c| allowed.iter().find(|a| a.0 == c)).map(|a| f::channel(lang, &a.1)).unwrap_or_default();
            egui::ComboBox::from_id_salt("webhook_salon").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for (id, name) in allowed {
                    ui.selectable_value(&mut e.channel, Some(*id), f::channel(lang, name));
                }
            });
        });
        let (mut pick, mut save, mut cancel) = (false, false, false);
        row(ui, tx::WEBHOOK_AVATAR.get(lang), "", |ui| {
            let state = match &e.avatar {
                Some(Some(_)) => tx::IMAGE_READY,
                Some(None) => tx::IMAGE_WILL_GO,
                None if h.avatar.is_some() => tx::IMAGE_SET,
                None => tx::IMAGE_NONE,
            };
            ui.label(RichText::new(state.get(lang)).color(t.colors.muted));
            pick = ui.add_enabled(Avatar::CAN_PICK && !picking, crate::ui::form::btn(t, tx::CHANGE_IMAGE.get(lang))).clicked();
            if ui.add_enabled(h.avatar.is_some() || e.avatar.is_some(), crate::ui::form::btn(t, tx::REMOVE_IMAGE.get(lang))).clicked() {
                e.avatar = if h.avatar.is_some() { Some(None) } else { None };
            }
        });
        let name = e.name.trim().to_string();
        let changed_name = (name != h.name).then_some(name.clone());
        let changed_channel = e.channel.filter(|c| Some(*c) != h.channel_id);
        let any = changed_name.is_some() || changed_channel.is_some() || e.avatar.is_some();
        ui.horizontal(|ui| {
            save = ui.add_enabled(any && !name.is_empty(), crate::ui::form::primary(t, tx::SAVE_WEBHOOK.get(lang))).clicked();
            cancel = ui.add(crate::ui::form::btn(t, tx::CANCEL.get(lang))).clicked();
        });
        let avatar = e.avatar.clone();
        if pick {
            let title = tx::PICK_TITLE.get(lang);
            self.hooks_worker().pick(ImageKind::Avatar, title);
        }
        // Fermé sur « Annuler », ou quand l'écriture est partie.
        if cancel || (save && self.store.edit_webhook(guild, h.id, changed_name, avatar, changed_channel)) {
            self.guild_settings.hooks.editing = None;
        }
    }

    fn webhook_delete_confirm(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, h: &Webhook) {
        let lang = self.lang;
        let hs = &mut self.guild_settings.hooks;
        if hs.confirm != Some(h.id) {
            return;
        }
        ui.separator();
        ui.add(egui::Label::new(RichText::new(f::delete_webhook_question(lang, &h.name)).color(t.colors.text_strong)).wrap());
        crate::ui::form::field(ui, t, &mut hs.reason, tx::REASON.get(lang), 512);
        let (mut yes, mut no) = (false, false);
        ui.horizontal(|ui| {
            yes = ui.add(crate::ui::form::danger(t, tx::CONFIRM_DELETE.get(lang))).clicked();
            no = ui.add(crate::ui::form::btn(t, tx::CANCEL.get(lang))).clicked();
        });
        if no {
            hs.confirm = None;
        } else if yes {
            let reason = Some(std::mem::take(&mut hs.reason));
            hs.confirm = None;
            self.store.delete_webhook(guild, h.id, reason);
        }
    }
}
