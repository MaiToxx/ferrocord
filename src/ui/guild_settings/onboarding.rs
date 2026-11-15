//! Page « Intégration » (lot A7) : activée ou non, critères, salons par
//! défaut, questions (titre, choix unique, obligatoire, posée à l'arrivée,
//! liste déroulante) et leurs options (titre, description, emoji, rôles
//! donnés, salons ajoutés). Rien ne part avant « Enregistrer » ; ce que
//! Discord a gardé est relu.

use super::welcome::text_channels;
use crate::i18n::table::events::t as tx;
use crate::state::admin::community::{new_id, CommunityAction, Loadable, Prompt, PromptOption, OPTIONS_MAX, PROMPTS_MAX, TITLE_MAX};
use crate::theme::Tokens;
use crate::ui::App;
use eframe::egui::{self, RichText};

/// Liste à cocher dans un menu (rôles, salons).
fn pick_many(ui: &mut egui::Ui, t: &Tokens, label: &str, items: &[(u64, String)], chosen: &mut Vec<u64>) {
    let names: Vec<&str> = items.iter().filter(|(id, _)| chosen.contains(id)).map(|(_, n)| n.as_str()).collect();
    let text = if names.is_empty() { label.to_string() } else { format!("{label} ({})", names.join(", ")) };
    ui.menu_button(text, |ui| {
        egui::ScrollArea::vertical().max_height(ui.ctx().content_rect().height() * 0.5).show(ui, |ui| {
            for (id, name) in items {
                let mut on = chosen.contains(id);
                if crate::ui::menu::check(ui, t, name, &mut on) {
                    if on {
                        chosen.push(*id);
                    } else {
                        chosen.retain(|x| x != id);
                    }
                }
            }
        });
    });
}

pub(in crate::ui) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
    let lang = app.lang;
    let h24 = app.h24();
    let drafted = app.events_ui.onboarding_draft.as_ref().is_some_and(|(g, _)| *g == guild);
    if !drafted {
        match app.store.community.onboarding.get(&guild) {
            None | Some(Loadable::Loading) => {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tx::LOADING.get(lang));
                });
                return;
            }
            Some(Loadable::Failed(fl)) => {
                ui.label(RichText::new(crate::ui::events::failure_text(lang, fl, h24)).color(t.colors.danger));
                return;
            }
            Some(Loadable::Ready(o)) => app.events_ui.onboarding_draft = Some((guild, o.clone())),
        }
    }
    let channels = text_channels(app, guild);
    let roles: Vec<(u64, String)> = app.store.guilds.iter().find(|g| g.id == guild).map(|g| g.roles.iter().filter(|r| r.id != guild).map(|r| (r.id, r.name.clone())).collect()).unwrap_or_default();
    let busy = app.store.community.pending(CommunityAction::SaveOnboarding, guild);
    let now = app.store.now();
    let frame_nr = ui.ctx().cumulative_frame_nr();
    let Some((_, o)) = app.events_ui.onboarding_draft.as_mut() else { return };
    let mut save = false;

    crate::ui::settings::shell::switch_label(ui, &mut o.enabled, tx::ONB_ENABLED.get(lang));
    ui.label(RichText::new(tx::ONB_TIP.get(lang)).small().color(t.colors.muted));
    ui.horizontal(|ui| {
        ui.label(tx::ONB_MODE.get(lang));
        let label = |m: u8| if m == 1 { tx::MODE_ADVANCED.get(lang) } else { tx::MODE_DEFAULT.get(lang) };
        egui::ComboBox::from_id_salt("integration-mode").selected_text(label(o.mode)).show_ui(ui, |ui| {
            for m in [0u8, 1] {
                ui.selectable_value(&mut o.mode, m, label(m));
            }
        });
    });
    pick_many(ui, t, tx::DEFAULT_CHANNELS.get(lang), &channels, &mut o.default_channels);
    ui.add_space(t.spacing.category_gap);
    ui.label(RichText::new(tx::QUESTIONS.get(lang)).family(crate::ui::fonts::demi()).strong());

    let (mut remove, mut up, mut down) = (None, None, None);
    let count = o.prompts.len();
    for (i, p) in o.prompts.iter_mut().enumerate() {
        egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_row()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tx::QUESTION_TITLE.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
            crate::ui::form::field(ui, t, &mut p.title, "", TITLE_MAX);
            ui.horizontal_wrapped(|ui| {
                crate::ui::settings::shell::switch_label(ui, &mut p.single_select, tx::SINGLE_SELECT.get(lang));
                crate::ui::settings::shell::switch_label(ui, &mut p.required, tx::REQUIRED_Q.get(lang));
                crate::ui::settings::shell::switch_label(ui, &mut p.in_onboarding, tx::IN_ONBOARDING.get(lang));
                let mut dropdown = p.kind == 1;
                if crate::ui::settings::shell::switch_label(ui, &mut dropdown, tx::DROPDOWN.get(lang)).changed() {
                    p.kind = u8::from(dropdown);
                }
            });
            let mut drop_opt = None;
            for (j, opt) in p.options.iter_mut().enumerate() {
                ui.separator();
                egui::Grid::new(("integration-option-page", i, j)).num_columns(2).show(ui, |ui| {
                    ui.label(tx::OPTION_TITLE.get(lang));
                    crate::ui::form::field(ui, t, &mut opt.title, "", TITLE_MAX);
                    ui.end_row();
                    ui.label(tx::OPTION_DESCRIPTION.get(lang));
                    crate::ui::form::field(ui, t, &mut opt.description, "", TITLE_MAX);
                    ui.end_row();
                    ui.label(tx::EMOJI.get(lang));
                    ui.horizontal(|ui| {
                        crate::ui::welcome::emoji_icon(ui, opt.emoji_id, opt.emoji_name.as_deref(), t.typography.body * 1.3);
                        let mut name = crate::state::emoji::field_text(opt.emoji_name.as_deref().unwrap_or_default());
                        if ui.add(egui::TextEdit::singleline(&mut name).desired_width(t.layout.card_width * 0.3)).changed() {
                            opt.emoji_id = None;
                            opt.emoji_animated = false;
                            opt.emoji_name = crate::state::emoji::from_field(&name);
                        }
                    });
                    ui.end_row();
                });
                ui.horizontal_wrapped(|ui| {
                    pick_many(ui, t, tx::ROLES.get(lang), &roles, &mut opt.role_ids);
                    pick_many(ui, t, tx::CHANNELS_GIVEN.get(lang), &channels, &mut opt.channel_ids);
                    if ui.add(crate::ui::form::btn(t, tx::REMOVE.get(lang))).clicked() {
                        drop_opt = Some(j);
                    }
                });
            }
            if let Some(j) = drop_opt {
                p.options.remove(j);
            }
            ui.horizontal_wrapped(|ui| {
                if p.options.len() < OPTIONS_MAX && ui.add(crate::ui::form::btn(t, tx::ADD_OPTION.get(lang))).clicked() {
                    let salt = (i as u64) << 12 | p.options.len() as u64 | frame_nr << 18;
                    p.options.push(PromptOption { id: new_id(now, salt), ..Default::default() });
                }
                if i > 0 && ui.add(crate::ui::form::btn(t, tx::MOVE_UP.get(lang))).clicked() {
                    up = Some(i);
                }
                if i + 1 < count && ui.add(crate::ui::form::btn(t, tx::MOVE_DOWN.get(lang))).clicked() {
                    down = Some(i);
                }
                if ui.add(crate::ui::form::danger(t, tx::REMOVE.get(lang))).clicked() {
                    remove = Some(i);
                }
            });
        });
        ui.add_space(t.spacing.item_y);
    }
    if let Some(i) = up {
        o.prompts.swap(i, i - 1);
    }
    if let Some(i) = down {
        o.prompts.swap(i, i + 1);
    }
    if let Some(i) = remove {
        o.prompts.remove(i);
    }
    if o.prompts.len() < PROMPTS_MAX && ui.add(crate::ui::form::btn(t, tx::ADD_QUESTION.get(lang))).clicked() {
        let salt = (o.prompts.len() as u64) << 12 | 0xfff | frame_nr << 18;
        o.prompts.push(Prompt { id: new_id(now, salt), in_onboarding: true, ..Default::default() });
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
        let o = o.clone();
        if let Err(e) = app.store.save_onboarding(guild, &o) {
            app.notify(crate::ui::events::save_error(e));
        }
    }
}
