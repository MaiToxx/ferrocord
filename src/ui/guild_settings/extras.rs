//! Pages Widget, Modèle de serveur et Élagage (groupe 2, lot V). Le widget
//! s'enregistre à chaque changement ; le modèle se crée, se synchronise et se
//! supprime (confirmation) ; l'élagage montre d'abord combien de membres il
//! retirerait, puis demande confirmation.

use super::super::settings::{note, section, toggle};
use super::{loading_or_failed, App};
use crate::i18n::table::lotv::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::admin::extras::{ExtraAction, ExtraNotice, PRUNE_DAYS, TEMPLATE_DESCRIPTION_MAX, TEMPLATE_NAME_MAX};
use crate::state::admin::guild::Load;
use crate::state::model::ChannelKind;
use crate::theme::Tokens;
use crate::ui::form;
use crate::ui::widgets;
use eframe::egui::{self, RichText};

/// Brouillons de ces pages (refaits à chaque ouverture de la fenêtre).
#[derive(Default)]
pub(super) struct ExtrasUi {
    name: String,
    description: String,
    /// Code du modèle dont la suppression attend sa confirmation.
    delete: Option<String>,
    /// Indice dans [`PRUNE_DAYS`].
    days: usize,
    reason: String,
    confirm_prune: bool,
}

pub(super) fn action_word(lang: Lang, a: ExtraAction) -> &'static str {
    match a {
        ExtraAction::Widget | ExtraAction::Templates | ExtraAction::PruneCount => crate::i18n::table::admin::t::ACTION_LOAD,
        ExtraAction::SaveWidget => tx::ACT_WIDGET,
        ExtraAction::SaveTemplate => tx::ACT_TEMPLATE,
        ExtraAction::DeleteTemplate => tx::ACT_TEMPLATE_DELETE,
        ExtraAction::Prune => tx::ACT_PRUNE,
    }
    .get(lang)
}

pub(super) fn notice_text(lang: Lang, n: &ExtraNotice) -> String {
    match n {
        ExtraNotice::WidgetSaved => tx::WIDGET_SAVED.get(lang).into(),
        ExtraNotice::TemplateSaved => tx::TEMPLATE_SAVED.get(lang).into(),
        ExtraNotice::TemplateDeleted => tx::TEMPLATE_DELETED.get(lang).into(),
        ExtraNotice::Pruned(Some(n)) => p::pruned(lang, u64::from(*n)),
        ExtraNotice::Pruned(None) => tx::PRUNED.get(lang).into(),
    }
}

/// Ligne « valeur à copier » : champ en lecture, bouton « Copier » à droite.
fn copy_row(ui: &mut egui::Ui, t: &Tokens, lang: Lang, value: &str) -> bool {
    let mut copied = false;
    widgets::field_row(ui, t, |ui| {
        copied = ui.add(form::btn(t, tx::COPY.get(lang))).clicked();
        let mut shown = value.to_string();
        widgets::field(ui, t, &mut shown, "");
    });
    if copied {
        ui.ctx().copy_text(value.to_string());
    }
    copied
}

impl App {
    /// Donnée d'une page : `None` tant qu'elle n'est pas prête (l'état est
    /// alors dit, avec « Réessayer » après un échec).
    fn extra_ready<T: Clone>(&self, ui: &mut egui::Ui, t: &Tokens, l: Option<Load<T>>, retry: &mut bool) -> Option<T> {
        let lang = self.lang;
        if loading_or_failed(ui, t, lang, self.h24(), l.as_ref()) {
            if matches!(l, Some(Load::Failed(_))) {
                *retry = ui.add(form::btn(t, tx::RETRY.get(lang))).clicked();
            }
            return None;
        }
        match l {
            Some(Load::Ready(v)) => Some(v),
            _ => None,
        }
    }

    pub(super) fn page_guild_widget(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        if self.store.guild_admin.extras.widget.is_none() {
            self.store.load_widget(guild);
        }
        note(ui, t, tx::WIDGET_TIP.get(lang));
        let mut retry = false;
        let state = self.store.guild_admin.extras.widget.clone();
        let Some(w) = self.extra_ready(ui, t, state, &mut retry) else {
            if retry {
                self.store.guild_admin.extras.widget = None;
            }
            return;
        };
        let busy = self.store.guild_admin.extras.widget_busy;
        let mut next = w;
        if let Some(on) = toggle(ui, tx::WIDGET_ENABLE.get(lang), tx::WIDGET_ENABLE_TIP.get(lang), Some(w.enabled), !busy) {
            next.enabled = on;
        }
        section(ui, t, tx::WIDGET_CHANNEL.get(lang));
        note(ui, t, tx::WIDGET_CHANNEL_TIP.get(lang));
        let channels: Vec<(u64, String)> = self
            .store
            .guilds
            .iter()
            .find(|g| g.id == guild)
            .map(|g| g.channels.iter().filter(|c| matches!(c.kind, ChannelKind::Text | ChannelKind::Announcement)).map(|c| (c.id, format!("#{}", c.name))).collect())
            .unwrap_or_default();
        let none = tx::WIDGET_NO_INVITE.get(lang);
        let shown = w.channel_id.and_then(|id| channels.iter().find(|c| c.0 == id)).map_or(none, |c| c.1.as_str());
        ui.add_enabled_ui(!busy, |ui| {
            egui::ComboBox::from_id_salt("widget-salon").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                ui.selectable_value(&mut next.channel_id, None, none);
                for (id, name) in &channels {
                    ui.selectable_value(&mut next.channel_id, Some(*id), name);
                }
            });
        });
        if next != w {
            self.store.save_widget(guild, next);
        }
        section(ui, t, tx::WIDGET_ID.get(lang));
        if copy_row(ui, t, lang, &guild.to_string()) {
            self.notify(tx::COPIED);
        }
        section(ui, t, tx::WIDGET_JSON.get(lang));
        note(ui, t, tx::WIDGET_JSON_TIP.get(lang));
        if copy_row(ui, t, lang, &format!("https://discord.com/api/guilds/{guild}/widget.json")) {
            self.notify(tx::COPIED);
        }
    }

    pub(super) fn page_guild_template(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        if self.store.guild_admin.extras.templates.is_none() {
            self.store.load_templates(guild);
        }
        note(ui, t, tx::TEMPLATE_TIP.get(lang));
        let mut retry = false;
        let state = self.store.guild_admin.extras.templates.clone();
        let Some(list) = self.extra_ready(ui, t, state, &mut retry) else {
            if retry {
                self.store.guild_admin.extras.templates = None;
            }
            return;
        };
        let busy = self.store.guild_admin.extras.template_busy;
        // Discord ne garde qu'un modèle par serveur.
        let Some(tpl) = list.first() else {
            let x = &mut self.guild_settings.extras;
            widgets::field_label(ui, t, tx::TEMPLATE_NAME.get(lang));
            form::field(ui, t, &mut x.name, tx::TEMPLATE_NAME_HINT.get(lang), TEMPLATE_NAME_MAX);
            ui.add_space(t.spacing.item_y);
            widgets::field_label(ui, t, tx::TEMPLATE_DESCRIPTION.get(lang));
            form::field(ui, t, &mut x.description, tx::TEMPLATE_DESCRIPTION_HINT.get(lang), TEMPLATE_DESCRIPTION_MAX);
            ui.add_space(t.spacing.item_y);
            let ok = !x.name.trim().is_empty() && !busy;
            if ui.add_enabled(ok, form::primary(t, tx::TEMPLATE_CREATE.get(lang))).clicked() {
                let (name, description) = (x.name.clone(), x.description.clone());
                self.store.create_template(guild, &name, &description);
            }
            return;
        };
        ui.label(RichText::new(&tpl.name).family(crate::ui::fonts::demi()).size(t.typography.title).color(t.colors.text_strong));
        if !tpl.description.is_empty() {
            ui.add(egui::Label::new(RichText::new(&tpl.description).color(t.colors.text)).wrap());
        }
        ui.label(RichText::new(p::template_uses(lang, u64::from(tpl.usage_count))).small().color(t.colors.muted));
        section(ui, t, tx::TEMPLATE_LINK.get(lang));
        if copy_row(ui, t, lang, &tpl.url()) {
            self.notify(tx::COPIED);
        }
        ui.add_space(t.spacing.category_gap);
        if tpl.dirty {
            note(ui, t, tx::TEMPLATE_DIRTY.get(lang));
        }
        let (mut sync, mut ask) = (false, false);
        ui.horizontal(|ui| {
            sync = ui.add_enabled(tpl.dirty && !busy, form::primary(t, tx::TEMPLATE_SYNC.get(lang))).clicked();
            ask = ui.add_enabled(!busy, form::danger(t, tx::TEMPLATE_DELETE.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        });
        if sync {
            self.store.sync_template(guild, &tpl.code);
        }
        if ask {
            self.guild_settings.extras.delete = Some(tpl.code.clone());
        }
        if let Some(code) = self.guild_settings.extras.delete.clone() {
            match super::roles::confirm(ui, t, lang, "modele", &f::template_delete_question(lang, &tpl.name), tx::TEMPLATE_DELETE.get(lang)) {
                Some(true) => {
                    self.guild_settings.extras.delete = None;
                    self.store.delete_template(guild, &code);
                }
                Some(false) => self.guild_settings.extras.delete = None,
                None => {}
            }
        }
    }

    pub(super) fn page_guild_prune(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        note(ui, t, tx::PRUNE_TIP.get(lang));
        section(ui, t, tx::PRUNE_DAYS.get(lang));
        let x = &mut self.guild_settings.extras;
        x.days = x.days.min(PRUNE_DAYS.len() - 1);
        ui.horizontal(|ui| {
            // De l'air entre les deux choix (ils se touchaient presque).
            ui.spacing_mut().item_spacing.x = 2.0 * t.spacing.item_x;
            for (i, d) in PRUNE_DAYS.iter().enumerate() {
                ui.radio_value(&mut x.days, i, p::days(lang, u64::from(*d)));
            }
        });
        let days = PRUNE_DAYS[x.days];
        // Le compte suit la durée choisie ; une durée déjà comptée n'est pas redemandée.
        let known = self.store.guild_admin.extras.prune.clone().filter(|(d, _)| *d == days);
        if known.is_none() {
            self.store.load_prune_count(guild, days);
        }
        let mut retry = false;
        let count = self.extra_ready(ui, t, known.map(|(_, l)| l), &mut retry);
        if retry {
            self.store.guild_admin.extras.prune = None;
        }
        let Some(count) = count else { return };
        let text = if count == 0 { tx::PRUNE_NONE.get(lang).to_string() } else { p::prune_count(lang, u64::from(count)) };
        ui.label(RichText::new(text).color(t.colors.text_strong));
        ui.add_space(t.spacing.item_y);
        let busy = self.store.guild_admin.extras.pruning;
        let x = &mut self.guild_settings.extras;
        widgets::field_label(ui, t, tx::PRUNE_REASON.get(lang));
        form::field(ui, t, &mut x.reason, tx::PRUNE_REASON_HINT.get(lang), crate::state::admin::moderation::REASON_MAX);
        ui.add_space(t.spacing.item_y);
        ui.horizontal(|ui| {
            if ui.add_enabled(count > 0 && !busy, form::danger(t, tx::PRUNE.get(lang))).clicked() {
                x.confirm_prune = true;
            }
            if busy {
                ui.spinner();
            }
        });
        if x.confirm_prune {
            let question = p::prune_question(lang, u64::from(count));
            match super::roles::confirm(ui, t, lang, "elagage", &question, tx::PRUNE.get(lang)) {
                Some(true) => {
                    let reason = std::mem::take(&mut x.reason);
                    x.confirm_prune = false;
                    self.store.prune_members(guild, days, &reason);
                }
                Some(false) => x.confirm_prune = false,
                None => {}
            }
        }
    }
}
