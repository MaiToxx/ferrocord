//! Page « AutoMod » (lot A5) : règles du serveur (activer, modifier,
//! supprimer après confirmation, créer), éditeur validé avant l'envoi
//! (limites documentées), et alertes reçues par la passerelle.

use super::{note, section};
use crate::i18n::table::moderation::{f, p, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::admin::automod::{split_list, Rule, RuleAction, RuleError, Trigger, PRESET_PROFANITY, PRESET_SEXUAL, PRESET_SLURS};
use crate::state::admin::moderation::{Load, ModAction, RuleView};
use crate::state::model::ChannelKind;
use crate::theme::Tokens;
use crate::ui::moderation::{danger_button, primary_button, span, TIMEOUTS};
use crate::ui::App;
use eframe::egui::{self, Id, RichText};

/// Types proposés à la création, dans l'ordre de l'application officielle.
const NEW_TYPES: [u8; 5] = [1, 3, 5, 4, 6];

/// Règle en cours d'écriture : les listes restent du texte jusqu'à l'envoi.
struct Draft {
    rule: Rule,
    keywords: String,
    regex: String,
    allow: String,
    block: bool,
    message: String,
    alert: Option<u64>,
    timeout: Option<usize>,
    quarantine: bool,
}

impl Draft {
    fn of(rule: Rule) -> Draft {
        let (keywords, regex, allow) = match &rule.trigger {
            Trigger::Keyword { keywords, regex, allow } | Trigger::Profile { keywords, regex, allow } => (keywords.join("\n"), regex.join("\n"), allow.join("\n")),
            Trigger::Preset { allow, .. } => (String::new(), String::new(), allow.join("\n")),
            _ => Default::default(),
        };
        let mut d = Draft { keywords, regex, allow, block: false, message: String::new(), alert: None, timeout: None, quarantine: false, rule };
        for a in &d.rule.actions {
            match a {
                RuleAction::Block { message } => {
                    d.block = true;
                    d.message = message.clone().unwrap_or_default();
                }
                RuleAction::Alert { channel_id } => d.alert = Some(*channel_id),
                RuleAction::Timeout { seconds } => d.timeout = Some(TIMEOUTS.iter().position(|s| *s >= u64::from(*seconds)).unwrap_or(TIMEOUTS.len() - 1)),
                RuleAction::Quarantine => d.quarantine = true,
                RuleAction::Other(_) => {}
            }
        }
        d
    }

    /// Règle à envoyer, telle que saisie.
    fn build(&self) -> Rule {
        let mut r = self.rule.clone();
        r.name = r.name.trim().to_string();
        match &mut r.trigger {
            Trigger::Keyword { keywords, regex, allow } | Trigger::Profile { keywords, regex, allow } => {
                *keywords = split_list(&self.keywords);
                *regex = self.regex.lines().map(str::trim).filter(|s| !s.is_empty()).map(str::to_string).collect();
                *allow = split_list(&self.allow);
            }
            Trigger::Preset { allow, .. } => *allow = split_list(&self.allow),
            _ => {}
        }
        let mut actions = Vec::new();
        if self.block {
            let m = self.message.trim();
            actions.push(RuleAction::Block { message: (!m.is_empty()).then(|| m.to_string()) });
        }
        if let Some(c) = self.alert {
            actions.push(RuleAction::Alert { channel_id: c });
        }
        if let Some(i) = self.timeout {
            let secs = TIMEOUTS[i.min(TIMEOUTS.len() - 1)];
            actions.push(RuleAction::Timeout { seconds: u32::try_from(secs).unwrap_or(u32::MAX) });
        }
        if self.quarantine {
            actions.push(RuleAction::Quarantine);
        }
        r.actions = actions;
        r
    }
}

#[derive(Default)]
pub(in crate::ui) struct AutoModUi {
    draft: Option<Draft>,
    /// Règle dont la suppression attend confirmation.
    confirm_delete: Option<(u64, String)>,
}

pub(super) fn open(app: &mut App, guild: u64) {
    app.moderation.automod = AutoModUi::default();
    app.store.load_rules(guild);
}

fn trigger_label(code: u8) -> Text {
    match code {
        1 => tx::TR_KEYWORD,
        3 => tx::TR_SPAM,
        4 => tx::TR_PRESET,
        5 => tx::TR_MENTION,
        6 => tx::TR_PROFILE,
        _ => tx::TR_OTHER,
    }
}

fn error_text(e: RuleError) -> Text {
    match e {
        RuleError::Name => tx::E_NAME,
        RuleError::Empty => tx::E_EMPTY,
        RuleError::Keywords => tx::E_KEYWORDS,
        RuleError::Regex => tx::E_REGEX,
        RuleError::Allow => tx::E_ALLOW,
        RuleError::Mentions => tx::E_MENTIONS,
        RuleError::NoAction => tx::E_NO_ACTION,
        RuleError::Action => tx::E_ACTION,
        RuleError::AlertChannel => tx::E_ALERT,
        RuleError::Exempt => tx::E_EXEMPT,
        RuleError::Unsupported => tx::E_UNSUPPORTED,
    }
}

/// Résumé d'une règle sous son nom.
fn summary(lang: Lang, r: &Rule) -> String {
    let what = trigger_label(r.trigger.code()).get(lang);
    match &r.trigger {
        Trigger::Keyword { keywords, .. } | Trigger::Profile { keywords, .. } => format!("{what} · {}", p::keywords(lang, keywords.len() as u64)),
        Trigger::MentionSpam { limit, .. } => format!("{what} · {}", p::mentions(lang, u64::from(*limit))),
        _ => what.to_string(),
    }
}

pub(super) fn show(ui: &mut egui::Ui, app: &mut App, guild: u64) {
    if app.store.moderation.take_saved_rule().is_some() {
        app.moderation.automod.draft = None;
    }
    if app.moderation.automod.draft.is_some() {
        editor(ui, app, guild);
        return;
    }
    let t = app.theme.clone();
    let lang = app.lang;
    let h24 = app.h24();
    let view: Option<RuleView> = app.store.moderation.rules.clone().filter(|v| v.guild_id == guild);
    let (mut toggle, mut edit, mut create, mut refresh) = (None, None, None, false);
    let st = &mut app.moderation.automod;
    ui.horizontal(|ui| {
        egui::ComboBox::from_id_salt("automod-nouvelle").selected_text(tx::NEW_RULE.get(lang)).show_ui(ui, |ui| {
            for c in NEW_TYPES {
                if ui.selectable_label(false, trigger_label(c).get(lang)).clicked() {
                    create = Some(c);
                }
            }
        });
        refresh = ui.add(crate::ui::form::btn(&t, tx::REFRESH.get(lang))).clicked();
    });
    section(ui, &t, tx::RULES.get(lang));
    match view.map(|v| v.list) {
        None | Some(Load::Loading) => {
            ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
        }
        Some(Load::Failed(problem)) => {
            ui.label(RichText::new(crate::ui::moderation::refusal(lang, &problem, h24)).color(t.colors.danger));
        }
        Some(Load::Ready { items, .. }) => {
            if items.is_empty() {
                ui.label(RichText::new(tx::NO_RULES.get(lang)).color(t.colors.muted));
            }
            // Quelques règles par serveur (limites de Discord) : pas de virtualisation.
            for r in &items {
                egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_row()).inner_margin(t.card_margin()).show(ui, |ui| {
                    // Disposition mobile : nom et résumé en entier, puis les commandes en dessous.
                    if crate::ui::mobile::is_active(ui.ctx()) {
                        ui.label(RichText::new(&r.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                        ui.add(egui::Label::new(RichText::new(summary(lang, r)).small().color(t.colors.muted)).wrap());
                        crate::ui::mobile::row(ui, |ui| {
                            let editable = !matches!(r.trigger, Trigger::Other(_));
                            let busy = app.store.moderation.busy(ModAction::SaveRule, guild);
                            let mut on = r.enabled;
                            let label = if on { tx::ENABLED } else { tx::DISABLED };
                            if ui.add_enabled(editable && !busy, egui::Checkbox::new(&mut on, label.get(lang))).changed() {
                                let mut flipped = r.clone();
                                flipped.enabled = on;
                                toggle = Some(flipped);
                            }
                            if editable && ui.add(crate::ui::form::btn(&t, tx::EDIT.get(lang))).clicked() {
                                edit = Some(r.clone());
                            }
                            if editable && ui.add(danger_button(&t, tx::DELETE.get(lang))).clicked() {
                                st.confirm_delete = Some((r.id, r.name.clone()));
                            }
                        });
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&r.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                            ui.label(RichText::new(summary(lang, r)).small().color(t.colors.muted));
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let editable = !matches!(r.trigger, Trigger::Other(_));
                            if editable && ui.add(danger_button(&t, tx::DELETE.get(lang))).clicked() {
                                st.confirm_delete = Some((r.id, r.name.clone()));
                            }
                            if editable && ui.add(crate::ui::form::btn(&t, tx::EDIT.get(lang))).clicked() {
                                edit = Some(r.clone());
                            }
                            let busy = app.store.moderation.busy(ModAction::SaveRule, guild);
                            let mut on = r.enabled;
                            let label = if on { tx::ENABLED } else { tx::DISABLED };
                            if ui.add_enabled(editable && !busy, egui::Checkbox::new(&mut on, label.get(lang))).changed() {
                                let mut flipped = r.clone();
                                flipped.enabled = on;
                                toggle = Some(flipped);
                            }
                        });
                    });
                });
            }
        }
    }
    let executions: Vec<_> = app.store.moderation.executions.iter().filter(|e| e.guild_id == guild).cloned().collect();
    if !executions.is_empty() {
        section(ui, &t, tx::EXECUTIONS.get(lang));
        for e in executions.iter().rev().take(10) {
            let shown = if e.matched.is_empty() { &e.content } else { &e.matched };
            let who = app.store.people.lock().user(e.user_id).map_or_else(|| e.user_id.to_string(), |u| u.display_name().to_string());
            ui.add(egui::Label::new(RichText::new(f::execution(lang, who, shown)).small().color(t.colors.muted)).truncate());
        }
    }
    if let Some((id, name)) = st.confirm_delete.clone() {
        let (mut yes, mut no) = (false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("automod-supprimer"), width, &f::confirm_delete_rule(lang, &name), crate::i18n::t::CLOSE_TIP.get(lang), |_| {}, |ui| {
            yes = ui.add(danger_button(&t, tx::DELETE.get(lang))).clicked();
            no = ui.add(crate::ui::form::link(&t, tx::CANCEL.get(lang))).clicked();
        });
        if yes {
            st.confirm_delete = None;
            app.store.delete_rule(guild, id);
        } else if no || modal.closed {
            st.confirm_delete = None;
        }
    }
    if let Some(r) = edit {
        app.moderation.automod.draft = Some(Draft::of(r));
    }
    if let Some(c) = create {
        app.moderation.automod.draft = Some(Draft::of(Rule::new(Trigger::empty(c), trigger_label(c).get(lang).to_string())));
    }
    if let Some(r) = toggle {
        app.store.save_rule(guild, r);
    }
    if refresh {
        app.store.load_rules(guild);
    }
}

/// Salons ou rôles : (identifiant, nom affiché).
type Named = Vec<(u64, String)>;

/// Liste à cocher (rôles ou salons exemptés).
fn pick_many(ui: &mut egui::Ui, t: &Tokens, label: &str, items: &[(u64, String)], chosen: &mut Vec<u64>) {
    let text = format!("{label} ({})", chosen.len());
    ui.menu_button(text, |ui| {
        egui::ScrollArea::vertical().max_height(crate::ui::menu::row_height(t) * 10.0).show(ui, |ui| {
            for (id, name) in items {
                let mut on = chosen.contains(id);
                if crate::ui::menu::check(ui, t, name, &mut on) {
                    if on {
                        chosen.push(*id);
                    } else {
                        chosen.retain(|c| c != id);
                    }
                }
            }
        });
    });
}

fn editor(ui: &mut egui::Ui, app: &mut App, guild: u64) {
    let t = app.theme.clone();
    let lang = app.lang;
    let (channels, roles): (Named, Named) = app
        .store
        .guilds
        .iter()
        .find(|g| g.id == guild)
        .map(|g| {
            let c = g.channels.iter().filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement)).map(|c| (c.id, format!("#{}", c.name))).collect();
            let r = g.roles.iter().filter(|r| r.id != guild).map(|r| (r.id, format!("@{}", r.name))).collect();
            (c, r)
        })
        .unwrap_or_default();
    let busy = app.store.moderation.busy(ModAction::SaveRule, guild);
    let Some(d) = app.moderation.automod.draft.as_mut() else { return };
    let (mut save, mut cancel) = (false, false);
    egui::ScrollArea::vertical().id_salt("automod-editeur").auto_shrink([false, true]).show(ui, |ui| {
        ui.label(RichText::new(trigger_label(d.rule.trigger.code()).get(lang)).small().color(t.colors.muted));
        ui.label(RichText::new(tx::RULE_NAME.get(lang)).small().color(t.colors.muted));
        crate::ui::form::field(ui, &t, &mut d.rule.name, "", crate::state::admin::automod::NAME_MAX);
        match &mut d.rule.trigger {
            Trigger::Keyword { .. } | Trigger::Profile { .. } => {
                ui.label(RichText::new(tx::KEYWORDS.get(lang)).small().color(t.colors.muted));
                ui.add(egui::TextEdit::multiline(&mut d.keywords).desired_rows(3).desired_width(f32::INFINITY));
                ui.label(RichText::new(tx::REGEX.get(lang)).small().color(t.colors.muted));
                ui.add(egui::TextEdit::multiline(&mut d.regex).desired_rows(2).code_editor().desired_width(f32::INFINITY));
                ui.label(RichText::new(tx::ALLOW.get(lang)).small().color(t.colors.muted));
                ui.add(egui::TextEdit::multiline(&mut d.allow).desired_rows(2).desired_width(f32::INFINITY));
            }
            Trigger::Preset { presets, .. } => {
                for (code, label) in [(PRESET_PROFANITY, tx::PRESET_PROFANITY), (PRESET_SEXUAL, tx::PRESET_SEXUAL), (PRESET_SLURS, tx::PRESET_SLURS)] {
                    let mut on = presets.contains(&code);
                    if crate::ui::settings::shell::switch_label(ui, &mut on, label.get(lang)).changed() {
                        if on {
                            presets.push(code);
                            presets.sort_unstable();
                        } else {
                            presets.retain(|p| *p != code);
                        }
                    }
                }
                ui.label(RichText::new(tx::ALLOW.get(lang)).small().color(t.colors.muted));
                ui.add(egui::TextEdit::multiline(&mut d.allow).desired_rows(2).desired_width(f32::INFINITY));
            }
            Trigger::MentionSpam { limit, raid } => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tx::MENTION_LIMIT.get(lang)).small().color(t.colors.muted));
                    ui.add(egui::DragValue::new(limit).range(1..=crate::state::admin::automod::MENTION_LIMIT_MAX));
                });
                crate::ui::settings::shell::switch_label(ui, raid, tx::RAID_PROTECTION.get(lang));
            }
            Trigger::Spam | Trigger::Other(_) => {}
        }
        section(ui, &t, tx::RESPONSES.get(lang));
        let rule = &d.rule;
        if rule.allows(1) {
            crate::ui::settings::shell::switch_label(ui, &mut d.block, tx::A_BLOCK.get(lang));
            if d.block {
                crate::ui::form::field(ui, &t, &mut d.message, tx::A_CUSTOM.get(lang), crate::state::admin::automod::CUSTOM_MESSAGE_MAX);
            }
        }
        if rule.allows(4) {
            crate::ui::settings::shell::switch_label(ui, &mut d.quarantine, tx::A_QUARANTINE.get(lang));
        }
        if rule.allows(2) {
            ui.horizontal(|ui| {
                let mut on = d.alert.is_some();
                if crate::ui::settings::shell::switch_label(ui, &mut on, tx::A_ALERT.get(lang)).changed() {
                    d.alert = on.then_some(0);
                }
                if let Some(c) = d.alert.as_mut() {
                    let shown = channels.iter().find(|(id, _)| id == c).map_or_else(|| tx::PICK_CHANNEL.get(lang).to_string(), |(_, n)| n.clone());
                    egui::ComboBox::from_id_salt("automod-alerte").selected_text(shown).show_ui(ui, |ui| {
                        for (id, name) in &channels {
                            ui.selectable_value(c, *id, name);
                        }
                    });
                }
            });
        }
        if rule.allows(3) {
            ui.horizontal(|ui| {
                let mut on = d.timeout.is_some();
                if crate::ui::settings::shell::switch_label(ui, &mut on, tx::A_TIMEOUT.get(lang)).changed() {
                    d.timeout = on.then_some(0);
                }
                if let Some(i) = d.timeout.as_mut() {
                    egui::ComboBox::from_id_salt("automod-exclusion").selected_text(span(lang, TIMEOUTS[(*i).min(TIMEOUTS.len() - 1)])).show_ui(ui, |ui| {
                        for (n, s) in TIMEOUTS.iter().enumerate() {
                            ui.selectable_value(i, n, span(lang, *s));
                        }
                    });
                }
            });
        }
        section(ui, &t, tx::EXEMPT_ROLES.get(lang));
        ui.horizontal_wrapped(|ui| {
            pick_many(ui, &t, tx::EXEMPT_ROLES.get(lang), &roles, &mut d.rule.exempt_roles);
            if d.rule.trigger.event_type() == 1 {
                pick_many(ui, &t, tx::EXEMPT_CHANNELS.get(lang), &channels, &mut d.rule.exempt_channels);
            }
        });
        let check = d.build().check();
        if let Err(e) = check {
            note(ui, &t, error_text(e).get(lang));
        }
        ui.add_space(t.spacing.item_y);
        ui.horizontal(|ui| {
            save = ui.add_enabled(check.is_ok() && !busy, primary_button(&t, tx::SAVE.get(lang))).clicked();
            cancel = ui.add(crate::ui::form::btn(&t, tx::CANCEL.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        });
    });
    if save {
        let rule = d.build();
        app.store.save_rule(guild, rule);
    }
    if cancel {
        app.moderation.automod.draft = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_round_trips_a_rule() {
        let mut r = Rule::new(Trigger::empty(1), "Mots".into());
        r.actions = vec![RuleAction::Block { message: Some("Non".into()) }, RuleAction::Alert { channel_id: 7 }, RuleAction::Timeout { seconds: 600 }];
        if let Trigger::Keyword { keywords, .. } = &mut r.trigger {
            *keywords = vec!["chat*".into(), "chien".into()];
        }
        let d = Draft::of(r.clone());
        assert_eq!(d.keywords, "chat*\nchien");
        assert_eq!(d.build(), r);
        assert_eq!(d.build().check(), Ok(()));
        let mut d = Draft::of(Rule::new(Trigger::empty(1), "Vide".into()));
        assert_eq!(d.build().check(), Err(RuleError::Empty));
        d.keywords = "a, b".into();
        d.block = false;
        assert_eq!(d.build().check(), Err(RuleError::NoAction));
        assert_eq!(summary(Lang::Fr, &r), "Mots-clés personnalisés · 2 mots-clés");
    }
}
