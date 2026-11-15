//! Page « Découverte et communauté » (lot A7) : fonctions du serveur
//! (`features`), salon des règles, salon des mises à jour, description,
//! présence dans la découverte. Activer ou couper la communauté touche à la
//! vérification et aux salons : renvoyé vers l'application officielle.

use crate::i18n::table::events::t as tx;
use crate::state::admin::community::{COMMUNITY, DISCOVERABLE};
use crate::theme::Tokens;
use crate::ui::App;
use eframe::egui::{self, RichText};

pub(in crate::ui) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
    let lang = app.lang;
    let Some(info) = app.store.guild_info(guild).cloned() else {
        ui.label(RichText::new(tx::NOT_LOADED.get(lang)).color(t.colors.muted));
        return;
    };
    let note = |ui: &mut egui::Ui, text: &str| {
        ui.label(RichText::new(text).color(t.colors.text));
    };
    note(ui, if info.has(COMMUNITY) { tx::COMMUNITY_ON.get(lang) } else { tx::COMMUNITY_OFF.get(lang) });
    note(ui, if info.has(DISCOVERABLE) { tx::DISCOVERY_ON.get(lang) } else { tx::DISCOVERY_OFF.get(lang) });
    ui.add_space(t.spacing.category_gap);
    let mut open = None;
    egui::Grid::new("communaute").num_columns(3).show(ui, |ui| {
        for (label, channel) in [(tx::RULES_CHANNEL, info.rules_channel), (tx::UPDATES_CHANNEL, info.updates_channel)] {
            ui.label(RichText::new(label.get(lang)).family(crate::ui::fonts::demi()).strong());
            match channel.and_then(|c| app.store.channel_name(c).map(|n| (c, n.to_string()))) {
                Some((c, name)) => {
                    ui.label(format!("#{name}"));
                    if ui.add(crate::ui::form::btn(t, tx::OPEN.get(lang))).clicked() {
                        open = Some(c);
                    }
                }
                None => {
                    ui.label(RichText::new(tx::NONE.get(lang)).color(t.colors.muted));
                    ui.label("");
                }
            }
            ui.end_row();
        }
    });
    if !info.description.is_empty() {
        ui.add_space(t.spacing.item_y);
        ui.label(RichText::new(tx::SERVER_DESCRIPTION.get(lang)).family(crate::ui::fonts::demi()).strong());
        note(ui, &info.description);
    }
    ui.add_space(t.spacing.category_gap);
    ui.label(RichText::new(tx::FEATURES.get(lang)).family(crate::ui::fonts::demi()).strong());
    ui.horizontal_wrapped(|ui| {
        for f in &info.features {
            ui.label(RichText::new(f).monospace().small().color(t.colors.muted));
        }
    });
    ui.add_space(t.spacing.category_gap);
    ui.label(RichText::new(tx::ENABLE_COMMUNITY_TIP.get(lang)).small().color(t.colors.muted));
    if ui.add(crate::ui::form::btn(t, tx::ENABLE_COMMUNITY.get(lang))).clicked() {
        ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://discord.com/channels/{guild}")));
    }
    if let Some(c) = open {
        app.close_guild_settings();
        if app.store.guild != Some(guild) {
            app.store.select_guild(guild);
        }
        app.open_channel(c);
    }
}
