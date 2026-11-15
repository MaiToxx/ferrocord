//! Page Sons : emplacements selon le niveau de boost, liste (emoji, nom,
//! volume, auteur), envoi (MP3 ou Ogg, durée et taille vérifiées),
//! modification (nom, volume, emoji), suppression confirmée. Pas de bouton
//! « écouter » : ferrocord ne joue pas de son. La liste vient de la liste
//! REST, tenue à jour par les événements GUILD_SOUNDBOARD_SOUND_*.

use super::expr_common::{self as c, Edit};
use super::super::emoji::{icon, Img};
use super::super::App;
use crate::expr_pick::Picked;
use crate::i18n::table::expressions::{f as xf, t as xt};
use crate::state::admin::expressions::{Kind, SoundEmoji, SOUND_NAME_MAX};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Champ emoji : un emoji, ou `:nom:` d'un emoji du serveur.
const EMOJI_MAX: usize = 40;

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    // Niveau de boost : `Guild::premium_tier`, seule source.
    let tier = app.store.guild_tier(g);
    let total = app.store.expr_slots(g).sounds;
    let used = app.store.expressions.sounds(g).map_or(0, <[_]>::len);
    ui.label(RichText::new(xf::tier(lang, tier)).small().color(t.colors.muted));
    c::slots(ui, t, lang, xt::PAGE_SOUNDS.get(lang), used, total);
    app.expr_header(ui, t, g, Kind::Sound, used < total);
    draft(app, ui, t, g);
    edit(app, ui, t, g);
    app.expr_confirm(ui, t, g);
    list(app, ui, t, g);
}

fn fields(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, name: &mut String, volume: &mut u8, emoji: &mut String) {
    c::field(ui, t, xt::NAME.get(lang), name, "", SOUND_NAME_MAX);
    ui.label(xt::VOLUME.get(lang));
    ui.add(egui::Slider::new(volume, 0..=100).suffix(xf::percent(lang, "").as_str()));
    c::field(ui, t, xt::SOUND_EMOJI.get(lang), emoji, xt::SOUND_EMOJI_HINT.get(lang), EMOJI_MAX);
}

fn draft(app: &mut App, ui: &mut egui::Ui, t: &Tokens, g: u64) {
    let lang = app.lang;
    let Some(mut d) = app.guild_settings.form.draft.take_if(|d| d.kind == Kind::Sound) else { return };
    let mut keep = true;
    egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        if let Picked::Sound { ms, .. } = &d.picked {
            ui.label(RichText::new(xf::duration(lang, c::seconds(lang, *ms))).small().color(t.colors.muted));
        }
        fields(ui, t, lang, &mut d.name, &mut d.volume, &mut d.emoji);
        ui.horizontal(|ui| {
            if ui.add(crate::ui::form::btn(t, xt::SEND.get(lang))).clicked()
                && let Picked::Sound { data_uri, .. } = &d.picked
            {
                match c::sound_emoji(&app.store, g, &d.emoji) {
                    Err(()) => app.guild_settings.form.error = Some(xt::BAD_SOUND_EMOJI.get(lang).into()),
                    Ok(emoji) => match app.store.create_sound(g, &d.name, data_uri.clone(), d.volume, emoji) {
                        Ok(()) => keep = false,
                        Err(e) => app.guild_settings.form.error = Some(c::invalid(lang, Kind::Sound, e)),
                    },
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
    let Some(mut e) = app.guild_settings.form.edit.take_if(|e| e.kind == Kind::Sound) else { return };
    let mut keep = true;
    egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
        fields(ui, t, lang, &mut e.name, &mut e.volume, &mut e.emoji);
        ui.horizontal(|ui| {
            if ui.add(crate::ui::form::btn(t, xt::SAVE.get(lang))).clicked() {
                match c::sound_emoji(&app.store, g, &e.emoji) {
                    Err(()) => app.guild_settings.form.error = Some(xt::BAD_SOUND_EMOJI.get(lang).into()),
                    Ok(emoji) => match app.store.edit_sound(g, e.id, &e.name, e.volume, emoji) {
                        Ok(()) => keep = false,
                        Err(err) => app.guild_settings.form.error = Some(c::invalid(lang, Kind::Sound, err)),
                    },
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
    let sounds = app.store.expressions.sounds(g).unwrap_or_default();
    if sounds.is_empty() {
        ui.label(RichText::new(xt::NO_SOUNDS.get(lang)).color(t.colors.faint));
        return;
    }
    let (height, side) = c::row_metrics(t);
    let side = side / 2.0;
    let mut action = None;
    egui::ScrollArea::vertical().id_salt("gs_sounds").auto_shrink([false, false]).show_rows(ui, height, sounds.len(), |ui, range| {
        for s in &sounds[range] {
            c::row(ui, height, |ui| {
                let img = match &s.emoji {
                    Some(SoundEmoji::Unicode(u)) => crate::state::emoji::find(u).map(Img::Unicode),
                    Some(SoundEmoji::Custom(id)) => Some(Img::Custom(*id)),
                    None => None,
                };
                match img {
                    Some(img) => drop(icon(ui, img, side, egui::Sense::hover())),
                    None => drop(ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover())),
                }
                ui.label(RichText::new(s.name.as_str()).color(t.colors.text_strong));
                ui.label(RichText::new(xf::percent(lang, s.volume)).small().color(t.colors.muted));
                c::author_label(ui, t, s.author.as_ref());
                if !s.available {
                    ui.label(RichText::new(xt::UNAVAILABLE.get(lang)).small().color(t.colors.faint));
                }
                let busy = app.store.expressions.item_busy(g, Kind::Sound, s.id);
                if let Some(a) = c::row_buttons(ui, t, lang, busy) {
                    let mut edit = Edit::new(Kind::Sound, s.id, &s.name);
                    edit.volume = s.volume;
                    edit.emoji = c::sound_emoji_text(&app.store, g, s.emoji.as_ref());
                    action = Some((a, edit));
                }
            });
        }
    });
    if let Some((a, edit)) = action {
        app.expr_row_action(a, edit);
    }
}
