//! Menu du clic droit sur un serveur, un salon ou les messages privés :
//! rendre muet (avec une durée), niveau de notification, @everyone et rôles.

use super::App;
use crate::i18n::table::s::{f as sf, t as st};
use crate::i18n::{time, Text};
use crate::state::usersettings::notif::{Change, Level, Mute};
use eframe::egui;

/// Sur quoi porte le menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum NotifTarget {
    /// Un serveur, ou les messages privés (`None`).
    Guild(Option<u64>),
    /// Un salon (ou une catégorie) d'un serveur.
    Channel { guild: Option<u64>, channel: u64 },
}

/// Durées du muet : secondes (-1 : jusqu'à ce qu'on le retire).
const DURATIONS: [(i64, Text); 6] = [
    (15 * 60, st::MUTE_15M),
    (60 * 60, st::MUTE_1H),
    (3 * 60 * 60, st::MUTE_3H),
    (8 * 60 * 60, st::MUTE_8H),
    (24 * 60 * 60, st::MUTE_24H),
    (-1, st::MUTE_FOREVER),
];

impl App {
    /// Contenu du menu ; chaque clic envoie une seule modification.
    pub(in crate::ui) fn notif_menu(&mut self, ui: &mut egui::Ui, target: NotifTarget) {
        let lang = self.lang;
        let (guild, channel) = match target {
            NotifTarget::Guild(g) => (g, None),
            NotifTarget::Channel { guild, channel } => (guild, Some(channel)),
        };
        let settings = self.store.account.notifs.get(guild);
        let (mute, level) = match channel {
            None => (settings.mute, settings.level),
            Some(c) => {
                let ch = settings.channel(c);
                (ch.mute, ch.level)
            }
        };
        let now = self.store.now();
        let busy = self.store.account.guild_busy(guild);
        let mut change = None;
        ui.add_enabled_ui(!busy, |ui| {
            if mute.active(now) {
                let label = match mute.until_ms {
                    Some(ms) => sf::unmute_until(lang, time::until(lang, ms, self.h24())),
                    None => st::UNMUTE.get(lang).to_string(),
                };
                if crate::ui::menu::item(ui, &self.theme, &label, Some(crate::theme::icons::Icon::Bell), false).clicked() {
                    change = Some((Change::Mute(Mute::default()), 0));
                    ui.close();
                }
            } else {
                let title = if channel.is_some() { st::MUTE_CHANNEL } else if guild.is_some() { st::MUTE_SERVER } else { st::MUTE_DMS };
                crate::ui::menu::submenu(ui, &self.theme, title.get(lang), |ui| {
                    for (secs, text) in DURATIONS {
                        if crate::ui::menu::item(ui, &self.theme, text.get(lang), None, false).clicked() {
                            let until_ms = (secs > 0).then(|| now.saturating_add(secs.saturating_mul(1000)));
                            change = Some((Change::Mute(Mute { on: true, until_ms }), secs));
                            ui.close();
                        }
                    }
                });
            }
            crate::ui::menu::separator(ui, &self.theme);
            ui.label(egui::RichText::new(st::NOTIFY_LEVEL.get(lang)).small().color(self.theme.colors.faint));
            let mut levels = vec![(Level::All, st::LEVEL_ALL), (Level::Mentions, st::LEVEL_MENTIONS), (Level::Nothing, st::LEVEL_NOTHING)];
            if channel.is_some() {
                levels.push((Level::Inherit, st::LEVEL_INHERIT));
            }
            for (l, text) in levels {
                if crate::ui::menu::radio(ui, &self.theme, text.get(lang), level == l).clicked() && level != l {
                    change = Some((Change::Level(l), 0));
                    ui.close();
                }
            }
            if channel.is_none() && guild.is_some() {
                crate::ui::menu::separator(ui, &self.theme);
                let mut everyone = settings.suppress_everyone;
                if crate::ui::settings::shell::switch_label(ui, &mut everyone, st::SUPPRESS_EVERYONE.get(lang)).changed() {
                    change = Some((Change::SuppressEveryone(everyone), 0));
                }
                let mut roles = settings.suppress_roles;
                if crate::ui::settings::shell::switch_label(ui, &mut roles, st::SUPPRESS_ROLES.get(lang)).changed() {
                    change = Some((Change::SuppressRoles(roles), 0));
                }
            }
            if busy {
                ui.label(st::SAVING.get(lang));
            }
        });
        // Serveur : la fenêtre complète, avec les exceptions par salon (lot N).
        if let NotifTarget::Guild(Some(g)) = target {
            crate::ui::menu::separator(ui, &self.theme);
            if crate::ui::menu::item(ui, &self.theme, crate::i18n::table::lotn::t::OPEN_WINDOW.get(lang), Some(crate::theme::icons::Icon::Gear), false).clicked() {
                self.open_notif_settings(g);
                ui.close();
            }
        }
        // Mode développeur : identifiant du salon, sinon du serveur.
        let id = channel.or(guild);
        if let Some(id) = id
            && super::local::copy_id_shown(ui.ctx(), &self.store)
        {
            crate::ui::menu::separator(ui, &self.theme);
            let label = if channel.is_some() { crate::i18n::table::s2::t::COPY_CHANNEL_ID } else { crate::i18n::table::s2::t::COPY_SERVER_ID };
            if crate::ui::menu::item(ui, &self.theme, label.get(lang), Some(crate::theme::icons::Icon::Id), false).clicked() {
                ui.ctx().copy_text(id.to_string());
                ui.close();
            }
        }
        if let Some((c, window)) = change
            && let Err(r) = self.store.account.change_notif(guild, channel, c, window)
        {
            self.refused(r);
        }
    }
}

impl App {
    /// Sourdine seule d'une conversation privée (menu d'un utilisateur, lot U) :
    /// durées en sous-menu sous `title`, ou la fin de la sourdine.
    pub(in crate::ui) fn mute_entry(&mut self, ui: &mut egui::Ui, channel: u64, title: &str) {
        let lang = self.lang;
        let t = self.theme.clone();
        let mute = self.store.account.notifs.get(None).channel(channel).mute;
        let now = self.store.now();
        let mut change = None;
        ui.add_enabled_ui(!self.store.account.guild_busy(None), |ui| {
            if mute.active(now) {
                let label = match mute.until_ms {
                    Some(ms) => sf::unmute_until(lang, time::until(lang, ms, self.h24())),
                    None => st::UNMUTE.get(lang).to_string(),
                };
                if crate::ui::menu::item(ui, &t, &label, Some(crate::theme::icons::Icon::Bell), false).clicked() {
                    change = Some((Change::Mute(Mute::default()), 0));
                }
            } else {
                crate::ui::menu::submenu(ui, &t, title, |ui| {
                    for (secs, text) in DURATIONS {
                        if crate::ui::menu::item(ui, &t, text.get(lang), None, false).clicked() {
                            let until_ms = (secs > 0).then(|| now.saturating_add(secs.saturating_mul(1000)));
                            change = Some((Change::Mute(Mute { on: true, until_ms }), secs));
                        }
                    }
                });
            }
        });
        if let Some((c, window)) = change
            && let Err(r) = self.store.account.change_notif(None, Some(channel), c, window)
        {
            self.refused(r);
        }
    }
}
