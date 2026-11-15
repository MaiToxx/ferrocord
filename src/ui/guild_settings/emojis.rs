//! Page Emojis : emplacements fixes et animés selon le niveau de boost,
//! liste (image, nom, auteur), envoi, renommage et rôles autorisés,
//! suppression confirmée. La liste est celle du serveur (GUILD_CREATE,
//! puis GUILD_EMOJIS_UPDATE) ; la liste REST n'apporte que les auteurs.

use super::expr_common::{self as c, Edit};
use super::super::emoji::{icon, Img};
use super::super::App;
use crate::expr_pick::Picked;
use crate::i18n::table::expressions::{f as xf, t as xt};
use crate::state::admin::expressions::{Kind, EMOJI_NAME_MAX};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    // Niveau de boost : `Guild::premium_tier`, seule source.
    let tier = app.store.guild_tier(g);
    let total = app.store.expr_slots(g).emoji;
    let (fixed, animated) = app.store.emoji_counts(g);
    ui.label(RichText::new(xf::tier(lang, tier)).small().color(t.colors.muted));
    c::slots(ui, t, lang, xt::STATIC_EMOJIS.get(lang), fixed, total);
    c::slots(ui, t, lang, xt::ANIMATED_EMOJIS.get(lang), animated, total);
    app.expr_header(ui, t, g, Kind::Emoji, fixed < total || animated < total);
    draft(app, ui, t, g);
    edit(app, ui, t, g);
    app.expr_confirm(ui, t, g);
    list(app, ui, t, g);
}

fn draft(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    let Some(mut d) = app.guild_settings.form.draft.take_if(|d| d.kind == Kind::Emoji) else { return };
    let mut keep = true;
    egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        ui.horizontal(|ui| {
            if let Some(tex) = &d.preview {
                let side = t.markdown.jumbo;
                ui.add(egui::Image::new((tex.id(), egui::vec2(side, side))).fit_to_exact_size(egui::vec2(side, side)));
            }
            ui.vertical(|ui| c::field(ui, t, xt::NAME.get(lang), &mut d.name, "", EMOJI_NAME_MAX));
        });
        ui.horizontal(|ui| {
            if ui.add(crate::ui::form::btn(t, xt::SEND.get(lang))).clicked()
                && let Picked::Emoji { data_uri, animated, .. } = &d.picked
            {
                match app.store.create_emoji(g, &d.name, data_uri.clone(), *animated, Vec::new()) {
                    Ok(()) => keep = false,
                    Err(e) => app.guild_settings.form.error = Some(c::invalid(lang, Kind::Emoji, e)),
                }
            }
            if ui.add(crate::ui::form::btn(t, xt::CANCEL.get(lang))).clicked() {
                keep = false;
            }
        });
    });
    ui.add_space(t.spacing.item_y);
    if keep {
        app.guild_settings.form.draft = Some(d);
    }
}

fn edit(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    let Some(mut e) = app.guild_settings.form.edit.take_if(|e| e.kind == Kind::Emoji) else { return };
    let roles: Vec<(u64, String)> = app.store.guilds.iter().find(|x| x.id == g).map(|x| x.roles.iter().filter(|r| r.id != g).map(|r| (r.id, r.name.clone())).collect()).unwrap_or_default();
    let mut keep = true;
    egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        c::field(ui, t, xt::NAME.get(lang), &mut e.name, "", EMOJI_NAME_MAX);
        ui.label(xt::ROLES.get(lang));
        if e.roles.is_empty() {
            ui.label(RichText::new(xt::ALL_MEMBERS.get(lang)).small().color(t.colors.muted));
        }
        egui::ScrollArea::vertical().id_salt("gs_emoji_roles").max_height(t.markdown.jumbo * 3.0).show(ui, |ui| {
            for (id, name) in &roles {
                let mut on = e.roles.contains(id);
                if crate::ui::settings::shell::switch_label(ui, &mut on, name.as_str()).changed() {
                    if on {
                        e.roles.push(*id);
                    } else {
                        e.roles.retain(|r| r != id);
                    }
                }
            }
        });
        ui.horizontal(|ui| {
            if ui.add(crate::ui::form::btn(t, xt::SAVE.get(lang))).clicked() {
                match app.store.edit_emoji(g, e.id, &e.name, e.roles.clone()) {
                    Ok(()) => keep = false,
                    Err(err) => app.guild_settings.form.error = Some(c::invalid(lang, Kind::Emoji, err)),
                }
            }
            if ui.add(crate::ui::form::btn(t, xt::CANCEL.get(lang))).clicked() {
                keep = false;
            }
        });
    });
    ui.add_space(t.spacing.item_y);
    if keep {
        app.guild_settings.form.edit = Some(e);
    }
}

fn list(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    let Some(guild) = app.store.guilds.iter().find(|x| x.id == g) else { return };
    // En-tête (fixes puis animés) suivi de ses emojis ; `None` : en-tête.
    let mut rows: Vec<Option<&crate::state::model::Emoji>> = Vec::with_capacity(guild.emojis.len() + 2);
    for animated in [false, true] {
        rows.push(None);
        rows.extend(guild.emojis.iter().filter(|e| e.animated == animated).map(Some));
    }
    let (height, side) = c::row_metrics(t);
    let mut action = None;
    egui::ScrollArea::vertical().id_salt("gs_emojis").auto_shrink([false, false]).show_rows(ui, height, rows.len(), |ui, range| {
        for i in range {
            let Some(e) = rows[i] else {
                // Un en-tête déjà passé : celui-ci est celui des animés.
                let animated = rows[..i].iter().any(Option::is_none);
                let (title, empty) = (if animated { xt::ANIMATED_EMOJIS } else { xt::STATIC_EMOJIS }, rows.get(i + 1).is_none_or(Option::is_none));
                c::row(ui, height, |ui| {
                    ui.label(RichText::new(title.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                    if empty {
                        ui.label(RichText::new(xt::NO_EMOJIS.get(lang)).small().color(t.colors.faint));
                    }
                });
                continue;
            };
            c::row(ui, height, |ui| {
                icon(ui, Img::custom(e.id, e.animated), side, egui::Sense::hover());
                ui.label(RichText::new(format!(":{}:", e.name)).color(t.colors.text_strong));
                c::author_label(ui, t, app.store.expressions.emoji_author(g, e.id));
                if !e.available {
                    ui.label(RichText::new(xt::UNAVAILABLE.get(lang)).small().color(t.colors.faint));
                }
                let busy = app.store.expressions.item_busy(g, Kind::Emoji, e.id);
                if let Some(a) = c::row_buttons(ui, t, lang, busy) {
                    let mut edit = Edit::new(Kind::Emoji, e.id, &e.name);
                    edit.roles = e.roles.clone();
                    action = Some((a, edit));
                }
            });
        }
    });
    if let Some((a, edit)) = action {
        app.expr_row_action(a, edit);
    }
}
