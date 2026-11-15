//! Page Autocollants : emplacements selon le niveau de boost, liste (image,
//! nom, description, emoji associé, auteur), envoi (PNG, APNG ou GIF),
//! modification, suppression confirmée. La liste est celle du serveur
//! (GUILD_CREATE, puis GUILD_STICKERS_UPDATE) ; la liste REST apporte
//! descriptions, emojis associés et auteurs.

use super::expr_common::{self as c, Edit};
use super::super::emoji::icon;
use super::super::emoji_picker::sticker_img;
use super::super::App;
use crate::expr_pick::Picked;
use crate::i18n::table::expressions::{f as xf, t as xt};
use crate::state::admin::expressions::{Kind, STICKER_DESC_MAX, STICKER_NAME_MAX};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Longueur du champ « tags » chez Discord.
const TAGS_MAX: usize = 200;

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    // Niveau de boost : `Guild::premium_tier`, seule source.
    let tier = app.store.guild_tier(g);
    let total = app.store.expr_slots(g).stickers;
    let used = app.store.stickers.guild(g).len();
    ui.label(RichText::new(xf::tier(lang, tier)).small().color(t.colors.muted));
    c::slots(ui, t, lang, xt::PAGE_STICKERS.get(lang), used, total);
    app.expr_header(ui, t, g, Kind::Sticker, used < total);
    draft(app, ui, t, g);
    edit(app, ui, t, g);
    app.expr_confirm(ui, t, g);
    list(app, ui, t, g);
}

fn fields(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, name: &mut String, description: &mut String, tags: &mut String) {
    c::field(ui, t, xt::NAME.get(lang), name, "", STICKER_NAME_MAX);
    c::field(ui, t, xt::DESCRIPTION.get(lang), description, "", STICKER_DESC_MAX);
    c::field(ui, t, xt::TAGS.get(lang), tags, xt::TAGS_HINT.get(lang), TAGS_MAX);
}

fn draft(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    let Some(mut d) = app.guild_settings.form.draft.take_if(|d| d.kind == Kind::Sticker) else { return };
    let mut keep = true;
    egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        ui.horizontal_top(|ui| {
            if let Some(tex) = &d.preview {
                let side = t.markdown.sticker;
                ui.add(egui::Image::new((tex.id(), egui::vec2(side, side))).fit_to_exact_size(egui::vec2(side, side)));
            }
            ui.vertical(|ui| fields(ui, t, lang, &mut d.name, &mut d.description, &mut d.tags));
        });
        ui.horizontal(|ui| {
            if ui.add(crate::ui::form::btn(t, xt::SEND.get(lang))).clicked()
                && let Picked::Sticker { file, .. } = &d.picked
            {
                match app.store.create_sticker(g, &d.name, &d.description, &d.tags, file.clone()) {
                    Ok(()) => keep = false,
                    Err(e) => app.guild_settings.form.error = Some(c::invalid(lang, Kind::Sticker, e)),
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
    let Some(mut e) = app.guild_settings.form.edit.take_if(|e| e.kind == Kind::Sticker) else { return };
    let mut keep = true;
    egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        fields(ui, t, lang, &mut e.name, &mut e.description, &mut e.tags);
        ui.horizontal(|ui| {
            if ui.add(crate::ui::form::btn(t, xt::SAVE.get(lang))).clicked() {
                match app.store.edit_sticker(g, e.id, &e.name, &e.description, &e.tags) {
                    Ok(()) => keep = false,
                    Err(err) => app.guild_settings.form.error = Some(c::invalid(lang, Kind::Sticker, err)),
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
    let stickers = app.store.stickers.guild(g);
    if stickers.is_empty() {
        ui.label(RichText::new(xt::NO_STICKERS.get(lang)).color(t.colors.faint));
        return;
    }
    let (height, side) = c::row_metrics(t);
    let mut action = None;
    egui::ScrollArea::vertical().id_salt("gs_stickers").auto_shrink([false, false]).show_rows(ui, height, stickers.len(), |ui, range| {
        for s in &stickers[range] {
            let info = app.store.expressions.sticker_info(g, s.id);
            c::row(ui, height, |ui| {
                let item = s.item();
                match sticker_img(&item) {
                    Some(img) => drop(icon(ui, img, side, egui::Sense::hover())),
                    None => drop(ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover())),
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new(s.name.as_str()).color(t.colors.text_strong));
                    // L'auteur finit la ligne de détail (à part, il flottait trois pixels plus haut qu'elle).
                    let author = info.and_then(|i| i.author.as_ref()).map_or("", |a| a.name.as_str());
                    let detail = info.map(|i| [i.tags.as_str(), i.description.as_str(), author].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ")).unwrap_or_default();
                    if !detail.is_empty() {
                        ui.add(egui::Label::new(RichText::new(detail).small().color(t.colors.muted)).truncate());
                    }
                });
                if !s.available {
                    ui.label(RichText::new(xt::UNAVAILABLE.get(lang)).small().color(t.colors.faint));
                }
                let busy = app.store.expressions.item_busy(g, Kind::Sticker, s.id);
                if let Some(a) = c::row_buttons(ui, t, lang, busy) {
                    let mut edit = Edit::new(Kind::Sticker, s.id, &s.name);
                    if let Some(i) = info {
                        edit.description = i.description.clone();
                        edit.tags = i.tags.clone();
                    }
                    action = Some((a, edit));
                }
            });
        }
    });
    if let Some((a, edit)) = action {
        app.expr_row_action(a, edit);
    }
}
