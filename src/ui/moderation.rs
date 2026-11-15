//! Actions de modération sur un membre (lot A4) : expulser, bannir, exclure
//! temporairement ou lever l'exclusion, depuis le menu d'un membre ou d'un
//! auteur. Chaque fenêtre est la confirmation : elle dit ce qui va se
//! passer, prend une raison facultative (`X-Audit-Log-Reason`) et n'envoie
//! rien sans le bouton. Aussi : sélection de messages pour la suppression
//! en masse (barre en haut du salon, confirmation avec le nombre), et mise
//! en mots des fins d'action.

use super::guild_settings::{audit_log::AuditUi, automod::AutoModUi, bans::BansUi, safety::SafetyUi};
use super::{App, ToastText};
use crate::i18n::table::guilds::f as gf;
use crate::i18n::table::moderation::{f, p, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::admin::moderation::{ModAction, ModNotice, BULK_DELETE_MAX};
use crate::state::Problem;
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText};

/// Durées d'exclusion proposées (secondes), comme l'application officielle.
pub(super) const TIMEOUTS: [u64; 7] = [60, 300, 600, 3600, 86_400, 604_800, 2_419_200];
/// Messages supprimés avec un bannissement (secondes ; 0 : aucun).
pub(super) const BAN_DELETE: [u32; 7] = [0, 3600, 21_600, 43_200, 86_400, 259_200, 604_800];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ModKind {
    Kick,
    Ban,
    Timeout,
    Untimeout,
}

impl ModKind {
    pub(super) fn label(self) -> Text {
        match self {
            ModKind::Kick => tx::MENU_KICK,
            ModKind::Ban => tx::MENU_BAN,
            ModKind::Timeout => tx::MENU_TIMEOUT,
            ModKind::Untimeout => tx::MENU_UNTIMEOUT,
        }
    }

    pub(super) fn danger(self) -> bool {
        !matches!(self, ModKind::Untimeout)
    }
}

/// Fenêtre d'action ouverte sur un membre.
struct MemberDialog {
    guild: u64,
    user: u64,
    name: String,
    kind: ModKind,
    reason: String,
    /// Indice dans [`TIMEOUTS`] ou [`BAN_DELETE`].
    choice: usize,
}

/// État de l'interface de modération (appartient à `App`).
#[derive(Default)]
pub(super) struct ModerationUi {
    member: Option<MemberDialog>,
    /// « Supprimer » de la sélection cliqué : confirmation affichée.
    confirm_delete: bool,
    pub(super) bans: BansUi,
    pub(super) audit: AuditUi,
    pub(super) automod: AutoModUi,
    pub(super) safety: SafetyUi,
}

impl ModerationUi {
    /// Une fenêtre de modération est ouverte (elle prend Échap).
    pub(super) fn dialog_open(&self) -> bool {
        self.member.is_some() || self.confirm_delete
    }
}

/// Durée en mots : la plus grande unité entière.
pub(super) fn span(lang: Lang, secs: u64) -> String {
    match secs {
        s if s >= 604_800 && s % 604_800 == 0 => p::weeks(lang, s / 604_800),
        s if s >= 86_400 && s % 86_400 == 0 => p::days(lang, s / 86_400),
        s if s >= 3600 && s % 3600 == 0 => p::hours(lang, s / 3600),
        s if s >= 60 && s % 60 == 0 => p::minutes(lang, s / 60),
        s => p::seconds(lang, s),
    }
}

fn ban_delete_label(lang: Lang, secs: u32) -> String {
    match secs {
        0 => tx::DONT_DELETE.get(lang).to_string(),
        3600 => tx::PREV_HOUR.get(lang).to_string(),
        // Comme Discord : « 24 heures » plutôt qu'« 1 jour ».
        86_400 => f::previous(lang, p::hours(lang, 24)),
        s => f::previous(lang, span(lang, u64::from(s))),
    }
}

fn action_word(a: ModAction) -> Text {
    match a {
        ModAction::Kick => tx::ACT_KICK,
        ModAction::Ban => tx::ACT_BAN,
        ModAction::Unban => tx::ACT_UNBAN,
        ModAction::Timeout => tx::ACT_TIMEOUT,
        ModAction::Bans | ModAction::Audit | ModAction::Rules | ModAction::Safety => tx::ACT_LOAD,
        ModAction::BulkBan => tx::ACT_BULK_BAN,
        ModAction::BulkDelete => tx::ACT_BULK_DELETE,
        ModAction::SaveRule => tx::ACT_RULE,
        ModAction::DeleteRule => tx::ACT_RULE_DELETE,
        ModAction::SetSafety => tx::ACT_SAFETY,
        ModAction::Incidents => tx::ACT_INCIDENTS,
    }
}

/// Raison d'un refus, en mots : un 403 dit permission ou rôle ; l'A2F exigée
/// (code 60003) arrive comme `Problem::MfaRequired`, chemin unique de `net::rest`.
pub(super) fn refusal(lang: Lang, problem: &Problem, h24: bool) -> String {
    match problem {
        Problem::Forbidden => tx::FORBIDDEN.get(lang).to_string(),
        p => super::words::problem(lang, p, h24),
    }
}

pub(super) fn notice_text(lang: Lang, n: &ModNotice, h24: bool) -> String {
    match n {
        ModNotice::Done(a) => match a {
            ModAction::Kick => tx::DONE_KICK,
            ModAction::Ban => tx::DONE_BAN,
            ModAction::Unban => tx::DONE_UNBAN,
            ModAction::Timeout => tx::DONE_TIMEOUT,
            ModAction::SaveRule => tx::DONE_RULE,
            ModAction::DeleteRule => tx::DONE_RULE_DELETE,
            ModAction::Incidents => tx::DONE_INCIDENTS,
            _ => tx::DONE_SAFETY,
        }
        .get(lang)
        .to_string(),
        ModNotice::BulkBanned { banned, failed } => f::bulk_banned(lang, banned, failed),
        ModNotice::BulkDeleted { deleted, failed: 0 } => p::bulk_deleted(lang, *deleted as u64),
        ModNotice::BulkDeleted { deleted, failed } => f::bulk_partial(lang, deleted, failed),
        ModNotice::Failed { action, problem } => {
            // L'A2F se dit seule (même message que les autres pages) : elle dit déjà quoi faire.
            if *problem == Problem::MfaRequired {
                return crate::i18n::table::admin::t::MFA_REQUIRED.get(lang).to_string();
            }
            gf::failed(lang, action_word(*action).get(lang), refusal(lang, problem, h24))
        }
    }
}

/// Bouton rouge d'une action destructive.
pub(super) fn danger_button<'a>(t: &'a Tokens, text: &'a str) -> super::form::Btn<'a> {
    super::form::danger(t, text)
}

/// Bouton principal.
pub(super) fn primary_button<'a>(t: &'a Tokens, text: &'a str) -> super::form::Btn<'a> {
    super::form::primary(t, text)
}


/// Actions de modération que mes droits et la hiérarchie permettent sur `user`.
pub(super) fn picks_for(store: &crate::state::Store, guild: u64, user: u64) -> Vec<ModKind> {
    let r = store.member_rights(guild, user);
    let mut out = Vec::new();
    if r.timeout {
        out.push(if r.timed_out_until.is_some() { ModKind::Untimeout } else { ModKind::Timeout });
    }
    if r.kick {
        out.push(ModKind::Kick);
    }
    if r.ban {
        out.push(ModKind::Ban);
    }
    out
}

impl App {
    /// Actions de modération proposées dans le menu de `user` (serveur ouvert).
    pub(super) fn moderation_picks(&self, user: u64) -> Vec<ModKind> {
        let Some(guild) = self.store.guild else { return Vec::new() };
        self.moderation_picks_in(guild, user)
    }

    /// Actions de modération proposées pour `user` dans `guild` (page Membres, lot A3).
    pub(super) fn moderation_picks_in(&self, guild: u64, user: u64) -> Vec<ModKind> {
        picks_for(&self.store, guild, user)
    }

    /// Ouvre la fenêtre d'une action sur un membre du serveur ouvert.
    pub(super) fn open_member_action(&mut self, user: u64, name: &str, kind: ModKind) {
        let Some(guild) = self.store.guild else { return };
        self.open_member_action_in(guild, user, name, kind);
    }

    /// Ouvre la fenêtre d'une action sur un membre de `guild` (page Membres, lot A3).
    pub(super) fn open_member_action_in(&mut self, guild: u64, user: u64, name: &str, kind: ModKind) {
        // « 1 heure » par défaut pour l'exclusion, rien de supprimé pour le bannissement.
        let choice = if kind == ModKind::Timeout { 3 } else { 0 };
        self.moderation.member = Some(MemberDialog { guild, user, name: name.to_string(), kind, reason: String::new(), choice });
    }

    /// Ouvre une action par son nom (aperçu, tests) : « expulser », « bannir », « exclure ».
    pub fn show_member_action(&mut self, user: u64, name: &str, kind: &str) -> bool {
        let k = match kind {
            "expulser" => ModKind::Kick,
            "bannir" => ModKind::Ban,
            "exclure" => ModKind::Timeout,
            _ => return false,
        };
        self.open_member_action(user, name, k);
        self.moderation.member.is_some()
    }

    /// Fenêtres, barre de sélection et messages de fin (chaque image).
    pub(super) fn moderation_ui(&mut self, ui: &mut egui::Ui) {
        while let Some(n) = self.store.moderation.take_notice() {
            let text = notice_text(self.lang, &n, self.h24());
            self.notify(ToastText::Text(text));
        }
        // Salon quitté : la sélection ne le suit pas.
        if self.store.moderation.selection.as_ref().is_some_and(|s| Some(s.channel_id) != self.store.channel) {
            self.store.moderation.selection = None;
            self.moderation.confirm_delete = false;
        }
        // Geste fait sur une alerte AutoMod du fil (lot V).
        match super::system_msgs::take_alert_pick(ui.ctx()) {
            Some(super::system_msgs::AlertPick::Jump { channel, message }) => self.jump_to_message(channel, message),
            Some(super::system_msgs::AlertPick::Member { guild, user, name, kind }) => self.open_member_action_in(guild, user, &name, kind),
            Some(super::system_msgs::AlertPick::Rules(guild)) => self.open_guild_settings(guild, Some(super::guild_settings::Page::AutoMod)),
            None => {}
        }
        self.member_dialog(ui);
        self.selection_bar(ui);
        self.bulk_delete_confirm(ui);
    }

    fn member_dialog(&mut self, ui: &mut egui::Ui) {
        if self.moderation.member.as_ref().is_some_and(|d| !self.store.guilds.iter().any(|g| g.id == d.guild)) {
            self.moderation.member = None;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = self.h24();
        let until = self.moderation.member.as_ref().and_then(|d| self.store.member_rights(d.guild, d.user).timed_out_until);
        let Some(d) = self.moderation.member.as_mut() else { return };
        let (mut yes, mut no) = (false, false);
        let (title, tip) = match d.kind {
            ModKind::Kick => (f::kick_title(lang, &d.name), tx::KICK_TIP),
            ModKind::Ban => (f::ban_title(lang, &d.name), tx::BAN_TIP),
            ModKind::Timeout => (f::timeout_title(lang, &d.name), tx::TIMEOUT_TIP),
            ModKind::Untimeout => (f::untimeout_title(lang, &d.name), tx::UNTIMEOUT_TIP),
        };
        let kind = d.kind;
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = super::widgets::modal(ui.ctx(), &t, Id::unique("moderation-membre"), width, &title, crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.add(egui::Label::new(RichText::new(tip.get(lang)).color(t.colors.muted)).wrap());
            if let (ModKind::Untimeout, Some(end)) = (d.kind, until) {
                ui.label(RichText::new(f::timed_out_until(lang, crate::i18n::time::until(lang, end, h24))).small().color(t.colors.faint));
            }
            ui.add_space(t.spacing.item_y);
            match d.kind {
                ModKind::Ban => {
                    super::widgets::field_label(ui, &t, tx::DELETE_HISTORY.get(lang));
                    let shown = ban_delete_label(lang, BAN_DELETE[d.choice.min(BAN_DELETE.len() - 1)]);
                    egui::ComboBox::from_id_salt("bannir-historique").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                        for (i, s) in BAN_DELETE.iter().enumerate() {
                            ui.selectable_value(&mut d.choice, i, ban_delete_label(lang, *s));
                        }
                    });
                }
                ModKind::Timeout => {
                    super::widgets::field_label(ui, &t, tx::DURATION.get(lang));
                    let shown = span(lang, TIMEOUTS[d.choice.min(TIMEOUTS.len() - 1)]);
                    egui::ComboBox::from_id_salt("exclure-duree").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                        for (i, s) in TIMEOUTS.iter().enumerate() {
                            ui.selectable_value(&mut d.choice, i, span(lang, *s));
                        }
                    });
                }
                ModKind::Kick | ModKind::Untimeout => {}
            }
            super::widgets::field_label(ui, &t, tx::REASON.get(lang));
            ui.add(egui::TextEdit::multiline(&mut d.reason).hint_text(tx::REASON_HINT.get(lang)).desired_rows(2).char_limit(crate::state::admin::moderation::REASON_MAX).desired_width(f32::INFINITY));
        }, |ui| {
            let label = kind.label().get(lang);
            yes = ui.add(if kind.danger() { danger_button(&t, label) } else { primary_button(&t, label) }).clicked();
            no = ui.add(super::form::link(&t, tx::CANCEL.get(lang))).clicked();
        });
        if yes && let Some(d) = self.moderation.member.take() {
            let (g, u, why) = (d.guild, d.user, d.reason.as_str());
            match d.kind {
                ModKind::Kick => self.store.kick_member(g, u, why),
                ModKind::Ban => self.store.ban_member(g, u, BAN_DELETE[d.choice.min(BAN_DELETE.len() - 1)], why),
                ModKind::Timeout => self.store.timeout_member(g, u, Some(TIMEOUTS[d.choice.min(TIMEOUTS.len() - 1)]), why),
                ModKind::Untimeout => self.store.timeout_member(g, u, None, why),
            };
        } else if no || modal.closed {
            self.moderation.member = None;
        }
    }

    /// Barre en haut du salon pendant la sélection : nombre, « Supprimer », « Annuler ».
    fn selection_bar(&mut self, ui: &mut egui::Ui) {
        let Some(n) = self.store.moderation.selection.as_ref().map(|s| s.ids.len()) else { return };
        let Some(chat) = self.chat_rect else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let (mut delete, mut cancel) = (false, false);
        egui::Area::new(Id::unique("selection-messages")).order(egui::Order::Foreground).pivot(egui::Align2::CENTER_TOP).fixed_pos(chat.center_top() + egui::vec2(0.0, t.spacing.chat_pad)).show(ui.ctx(), |ui| {
            egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(t.r_toast()).inner_margin(t.toast_margin()).show(ui, |ui| {
                ui.set_max_width(chat.width() * 0.9);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(p::selected(lang, n as u64)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                    delete = ui.add_enabled(n > 0, danger_button(&t, tx::DELETE.get(lang))).clicked();
                    cancel = ui.add(super::form::btn(&t, tx::CANCEL.get(lang))).clicked();
                });
                ui.label(RichText::new(tx::SELECTION_TIP.get(lang)).small().color(t.colors.faint));
            });
        });
        if delete {
            self.moderation.confirm_delete = true;
        }
        if cancel {
            self.store.moderation.selection = None;
            self.moderation.confirm_delete = false;
        }
    }

    fn bulk_delete_confirm(&mut self, ui: &mut egui::Ui) {
        if !self.moderation.confirm_delete {
            return;
        }
        let Some(ids) = self.store.moderation.selection.as_ref().map(|s| s.ids.clone()) else {
            self.moderation.confirm_delete = false;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let n = ids.len().min(BULK_DELETE_MAX);
        let (mut yes, mut no) = (false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = super::widgets::modal(ui.ctx(), &t, Id::unique("suppression-masse"), width, &p::confirm_bulk_delete(lang, n as u64), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.add(egui::Label::new(RichText::new(tx::BULK_NOTE.get(lang)).color(t.colors.muted)).wrap());
        }, |ui| {
            yes = ui.add(danger_button(&t, tx::DELETE.get(lang))).clicked();
            no = ui.add(super::form::link(&t, tx::CANCEL.get(lang))).clicked();
        });
        if yes {
            self.moderation.confirm_delete = false;
            self.store.delete_selection();
        } else if no || modal.closed {
            self.moderation.confirm_delete = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spans_and_labels() {
        assert_eq!(span(Lang::Fr, 60), "1 minute");
        assert_eq!(span(Lang::En, 2_419_200), "4 weeks");
        assert_eq!(span(Lang::Fr, 86_400), "1 jour");
        assert_eq!(span(Lang::Fr, 90), "90 secondes");
        assert_eq!(ban_delete_label(Lang::Fr, 86_400), "Des dernières 24 heures");
        assert_eq!(ban_delete_label(Lang::En, 0), "Don't delete any");
        let n = ModNotice::Failed { action: ModAction::Ban, problem: Problem::MfaRequired };
        assert_eq!(notice_text(Lang::Fr, &n, true), crate::i18n::table::admin::t::MFA_REQUIRED.get(Lang::Fr));
        let n = ModNotice::Failed { action: ModAction::Kick, problem: Problem::Forbidden };
        assert!(notice_text(Lang::En, &n, true).starts_with("Kick failed: Discord refused"));
        assert_eq!(notice_text(Lang::Fr, &ModNotice::BulkDeleted { deleted: 3, failed: 0 }, true), "3 messages supprimés.");
    }
}
