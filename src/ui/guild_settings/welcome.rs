//! Page « Écran de bienvenue » (lot A7) : activé ou non, description (140
//! caractères), jusqu'à 5 salons recommandés avec emoji et description.
//! Rien ne part avant « Enregistrer » ; ce que Discord a gardé est relu.

use crate::i18n::table::events::{f, t as tx};
use crate::i18n::Text;
use crate::state::admin::community::{CommunityAction, Loadable, WelcomeChannel, WelcomeScreen, COMMUNITY, WELCOME_CHANNELS_MAX, WELCOME_CHANNEL_DESCRIPTION_MAX, WELCOME_DESCRIPTION_MAX, WELCOME_SCREEN_ENABLED};
use crate::state::model::ChannelKind;
use crate::theme::Tokens;
use crate::ui::App;
use eframe::egui::{self, RichText};

pub(in crate::ui) struct Draft {
    guild: u64,
    enabled: bool,
    screen: WelcomeScreen,
    error: Option<Text>,
}

/// Salons écrits du serveur qu'on peut recommander (visibles), dans l'ordre.
pub(in crate::ui) fn text_channels(app: &App, guild: u64) -> Vec<(u64, String)> {
    let Some(g) = app.store.guilds.iter().find(|g| g.id == guild) else { return Vec::new() };
    g.channels.iter().filter(|c| c.kind != ChannelKind::Category && !app.store.is_hidden(c.id)).map(|c| (c.id, c.name.clone())).collect()
}

pub(in crate::ui) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
    let lang = app.lang;
    let h24 = app.h24();
    let info = app.store.guild_info(guild).cloned().unwrap_or_default();
    if !info.has(COMMUNITY) {
        ui.label(RichText::new(tx::WELCOME_COMMUNITY_ONLY.get(lang)).color(t.colors.muted));
        return;
    }
    match app.store.community.welcome.get(&guild) {
        None | Some(Loadable::Loading) if app.events_ui.welcome_draft.as_ref().is_none_or(|d| d.guild != guild) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tx::LOADING.get(lang));
            });
            return;
        }
        Some(Loadable::Failed(fl)) if app.events_ui.welcome_draft.as_ref().is_none_or(|d| d.guild != guild) => {
            ui.label(RichText::new(crate::ui::events::failure_text(lang, fl, h24)).color(t.colors.danger));
            return;
        }
        Some(Loadable::Ready(s)) if app.events_ui.welcome_draft.as_ref().is_none_or(|d| d.guild != guild) => {
            app.events_ui.welcome_draft = Some(Draft { guild, enabled: info.has(WELCOME_SCREEN_ENABLED), screen: s.clone(), error: None });
        }
        _ => {}
    }
    let channels = text_channels(app, guild);
    let busy = app.store.community.pending(CommunityAction::SaveWelcome, guild);
    let Some(d) = app.events_ui.welcome_draft.as_mut() else { return };
    let mut save = false;
    crate::ui::settings::shell::switch_label(ui, &mut d.enabled, tx::WELCOME_ENABLED.get(lang));
    ui.add_space(t.spacing.item_y);
    ui.label(RichText::new(tx::WELCOME_DESCRIPTION.get(lang)).family(crate::ui::fonts::demi()).strong());
    ui.add(egui::TextEdit::multiline(&mut d.screen.description).char_limit(WELCOME_DESCRIPTION_MAX).desired_rows(2).desired_width(f32::INFINITY));
    ui.label(RichText::new(f::options_of(lang, d.screen.description.chars().count(), WELCOME_DESCRIPTION_MAX)).small().color(t.colors.muted));
    ui.add_space(t.spacing.category_gap);
    ui.label(RichText::new(tx::WELCOME_CHANNELS.get(lang)).family(crate::ui::fonts::demi()).strong());
    let mut remove = None;
    for (i, c) in d.screen.channels.iter_mut().enumerate() {
        egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_row()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new(("bienvenue-salon-page", i)).num_columns(2).show(ui, |ui| {
                ui.label(tx::CHANNEL.get(lang));
                let current = channels.iter().find(|x| x.0 == c.channel_id).map(|x| format!("#{}", x.1)).unwrap_or_default();
                egui::ComboBox::from_id_salt(("bienvenue-salon-choix", i)).selected_text(current).show_ui(ui, |ui| {
                    for (id, name) in &channels {
                        ui.selectable_value(&mut c.channel_id, *id, format!("#{name}"));
                    }
                });
                ui.end_row();
                ui.label(tx::EMOJI.get(lang));
                ui.horizontal(|ui| {
                    crate::ui::welcome::emoji_icon(ui, c.emoji_id, c.emoji_name.as_deref(), t.typography.body * 1.3);
                    let mut name = crate::state::emoji::field_text(c.emoji_name.as_deref().unwrap_or_default());
                    if ui.add(egui::TextEdit::singleline(&mut name).desired_width(t.layout.card_width * 0.3)).changed() {
                        // Emoji retapé : un emoji Unicode, plus le personnalisé d'avant.
                        c.emoji_id = None;
                        c.emoji_name = crate::state::emoji::from_field(&name);
                    }
                });
                ui.end_row();
                ui.label(tx::CHANNEL_DESCRIPTION.get(lang));
                crate::ui::form::field(ui, t, &mut c.description, "", WELCOME_CHANNEL_DESCRIPTION_MAX);
                ui.end_row();
            });
            if ui.add(crate::ui::form::btn(t, tx::REMOVE.get(lang))).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        d.screen.channels.remove(i);
    }
    if d.screen.channels.len() < WELCOME_CHANNELS_MAX && ui.add(crate::ui::form::btn(t, tx::ADD_CHANNEL.get(lang))).clicked() {
        let used: Vec<u64> = d.screen.channels.iter().map(|c| c.channel_id).collect();
        let channel_id = channels.iter().find(|c| !used.contains(&c.0)).map_or(0, |c| c.0);
        d.screen.channels.push(WelcomeChannel { channel_id, ..Default::default() });
    }
    if let Some(e) = d.error {
        ui.label(RichText::new(e.get(lang)).color(t.colors.danger));
    }
    ui.add_space(t.spacing.category_gap);
    ui.horizontal(|ui| {
        let b = crate::ui::form::primary(t, tx::SAVE.get(lang));
        save = ui.add_enabled(!busy, b).clicked();
        if busy {
            ui.spinner();
        }
    });
    if save {
        let (enabled, screen) = (d.enabled, d.screen.clone());
        let err = app.store.save_welcome(guild, enabled, &screen).err().map(crate::ui::events::save_error);
        if let Some(d) = app.events_ui.welcome_draft.as_mut() {
            d.error = err;
        }
    }
}
