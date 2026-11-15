//! Fenêtre « Règles et vérification » d'un serveur (lot ND), ouverte par le
//! menu du serveur : le formulaire de vérification des membres, l'état de
//! la demande du compte, et, pour qui peut expulser, les demandes
//! d'adhésion en attente (accepter ou refuser, une par clic).

use super::App;
use crate::i18n::table::nd::{f as nf, t as nt};
use crate::state::quests::Refused;
use crate::state::usersettings::Remote;
use crate::state::verification::{complete, Answer, Field, FieldKind, Gate, JoinRequest, Status, PARAGRAPH_MAX, REASON_MAX, TEXT_MAX};
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText};

pub(super) struct VerifyWindow {
    guild: u64,
    /// Réponses saisies, dans l'ordre du formulaire (vide tant qu'il n'est pas lu).
    answers: Vec<Answer>,
    /// Demande en cours de refus, et le motif saisi.
    rejecting: Option<u64>,
    reason: String,
}

/// Ce qu'un clic dans la fenêtre demande.
enum Act {
    Submit,
    Requests,
    Decide(u64, bool),
}

fn cap(s: &mut String, max: usize) {
    if s.chars().count() > max {
        *s = s.chars().take(max).collect();
    }
}

fn field_ui(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, index: usize, f: &Field, a: &mut Answer, enabled: bool) {
    let c = &t.colors;
    let label = if f.required { format!("{} *", f.label) } else { f.label.clone() };
    ui.add(egui::Label::new(RichText::new(label).family(crate::ui::fonts::demi()).color(c.text_strong)).wrap());
    if !f.description.is_empty() {
        ui.add(egui::Label::new(RichText::new(&f.description).small().color(c.muted)).wrap());
    }
    ui.add_enabled_ui(enabled, |ui| match (f.kind, a) {
        (FieldKind::Terms, Answer::Agreed(ok)) => {
            for (i, rule) in f.rules.iter().enumerate() {
                ui.add(egui::Label::new(RichText::new(format!("{}. {rule}", i + 1)).color(c.text)).wrap());
            }
            ui.checkbox(ok, nt::VERIFY_AGREE.get(lang));
        }
        (FieldKind::Text, Answer::Text(s)) => {
            crate::ui::widgets::field(ui, t, s, &f.placeholder);
            cap(s, TEXT_MAX);
        }
        (FieldKind::Paragraph, Answer::Text(s)) => {
            ui.add(egui::TextEdit::multiline(s).id_salt(("verification-paragraphe", index)).hint_text(RichText::new(&f.placeholder).color(c.faint)).desired_rows(3).desired_width(f32::INFINITY));
            cap(s, PARAGRAPH_MAX);
        }
        (FieldKind::Choice, Answer::Choice(pick)) => {
            for (i, choice) in f.choices.iter().enumerate() {
                if ui.radio(*pick == Some(i), choice).clicked() {
                    *pick = Some(i);
                }
            }
        }
        _ => {
            ui.add(egui::Label::new(RichText::new(nt::VERIFY_UNSUPPORTED.get(lang)).small().color(c.muted)).wrap());
        }
    });
    ui.add_space(t.spacing.item_x);
}

impl App {
    /// Entrée du menu d'un serveur ; `true` si elle est choisie.
    pub(super) fn verification_entry(&self, ui: &mut egui::Ui) -> bool {
        crate::ui::menu::item(ui, &self.theme, nt::VERIFY.get(self.lang), Some(crate::theme::icons::Icon::Check), false).clicked()
    }

    /// Ouvre la fenêtre (menu du serveur, aperçu, tests) : c'est ce geste qui
    /// lit le formulaire et l'état de la demande.
    pub fn show_verification(&mut self, guild: u64) {
        self.verify = Some(VerifyWindow { guild, answers: Vec::new(), rejecting: None, reason: String::new() });
        self.store.verification_open(guild);
    }

    /// « Voir les demandes » dans la fenêtre ouverte (aperçu, tests).
    pub fn show_join_requests(&mut self) -> bool {
        self.store.join_requests_load()
    }

    pub(super) fn verification_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(guild) = self.verify.as_ref().map(|w| w.guild) else { return };
        let Some(gname) = self.store.guilds.iter().find(|g| g.id == guild).map(|g| super::words::guild_name(self.lang, &g.name).to_string()) else {
            self.verify = None;
            self.store.verification_close();
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let c = &t.colors;
        let h24 = self.h24();
        let v = &self.store.verification;
        let can_submit = self.store.verification_can_submit();
        let reviewer = self.store.can_review_requests(guild);
        let Some(w) = self.verify.as_mut() else { return };
        // Formulaire lu : une réponse vide par champ.
        if let Remote::Ready(Gate { form: Some(form), .. }) = &v.gate
            && w.answers.len() != form.fields.len()
        {
            w.answers = form.fields.iter().map(|f| Answer::empty(f.kind)).collect();
        }
        let ready = matches!(&v.gate, Remote::Ready(Gate { form: Some(form), .. }) if complete(form, &w.answers));
        let mut act = None;
        let (mut close, mut submit) = (false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.2);
        let modal = super::widgets::modal(
            ui.ctx(),
            &t,
            Id::unique("verification"),
            width,
            &nf::verify_title(lang, &gname),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, &t);
                let note = |ui: &mut egui::Ui, text: &str, color| {
                    ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                };
                match &v.gate {
                    Remote::Idle | Remote::Loading => {
                        ui.spinner();
                    }
                    Remote::Failed(p) => note(ui, &super::words::problem(lang, p, h24), c.danger),
                    Remote::Ready(Gate { form: None, .. }) => note(ui, nt::VERIFY_NONE.get(lang), c.muted),
                    Remote::Ready(Gate { form: Some(form), status }) => {
                        if !form.description.is_empty() {
                            note(ui, &form.description, c.text);
                            ui.add_space(t.spacing.item_y);
                        }
                        match status {
                            Status::Submitted => note(ui, nt::VERIFY_SUBMITTED.get(lang), c.online),
                            Status::Approved => note(ui, nt::VERIFY_APPROVED.get(lang), c.online),
                            Status::Rejected(reason) if reason.is_empty() => note(ui, nt::VERIFY_REJECTED.get(lang), c.danger),
                            Status::Rejected(reason) => note(ui, &nf::verify_rejected(lang, reason), c.danger),
                            Status::Unknown | Status::Started => {}
                        }
                        ui.add_space(t.spacing.item_x);
                        for (i, (f, a)) in form.fields.iter().zip(w.answers.iter_mut()).enumerate() {
                            field_ui(ui, &t, lang, i, f, a, can_submit && !v.sending);
                        }
                    }
                }
                match &v.refused {
                    Some(Refused::Captcha) => note(ui, nt::CAPTCHA.get(lang), c.danger),
                    Some(Refused::Other(p)) => note(ui, &format!("{} {}", nt::VERIFY_REFUSED.get(lang), super::words::problem(lang, p, h24)), c.danger),
                    None => {}
                }
                if reviewer {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(nt::JOIN_REQUESTS.get(lang)).family(crate::ui::fonts::demi()).strong().color(c.text_strong));
                        let label = if matches!(v.requests, Remote::Idle) { nt::JOIN_REQUESTS_SHOW } else { nt::REFRESH };
                        if ui.add_enabled(!matches!(v.requests, Remote::Loading), super::form::btn(&t, label.get(lang))).clicked() {
                            act = Some(Act::Requests);
                        }
                    });
                    match &v.requests {
                        Remote::Idle => {}
                        Remote::Loading => {
                            ui.spinner();
                        }
                        Remote::Failed(p) => note(ui, &super::words::problem(lang, p, h24), c.danger),
                        Remote::Ready(list) if list.is_empty() => note(ui, nt::JOIN_REQUESTS_EMPTY.get(lang), c.muted),
                        Remote::Ready(list) => {
                            for r in list {
                                if let Some(a) = request_ui(ui, &t, lang, r, v.deciding, &mut w.rejecting, &mut w.reason) {
                                    act = Some(a);
                                }
                                ui.add_space(t.spacing.item_y);
                            }
                        }
                    }
                }
            },
            |ui| {
                if can_submit && ui.add_enabled(ready && !v.sending, crate::ui::form::primary(&t, nt::VERIFY_SEND.get(lang))).clicked() {
                    submit = true;
                }
                close = ui.add(crate::ui::form::link(&t, crate::i18n::table::guilds::t::CLOSE.get(lang))).clicked();
                if v.sending {
                    ui.spinner();
                }
            },
        );
        match act.or(submit.then_some(Act::Submit)) {
            Some(Act::Submit) => {
                let answers = w.answers.clone();
                self.store.verification_submit(&answers);
            }
            Some(Act::Requests) => {
                self.store.join_requests_load();
            }
            Some(Act::Decide(id, approve)) => {
                let reason = std::mem::take(&mut w.reason);
                w.rejecting = None;
                self.store.join_request_decide(id, approve, &reason);
            }
            None => {}
        }
        if close || modal.closed {
            self.verify = None;
            self.store.verification_close();
        }
    }
}

fn request_ui(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, r: &JoinRequest, deciding: Option<u64>, rejecting: &mut Option<u64>, reason: &mut String) -> Option<Act> {
    let c = &t.colors;
    let mut act = None;
    egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_row()).inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut who = if r.username.is_empty() || r.username == r.name { r.name.clone() } else { format!("{} ({})", r.name, r.username) };
        if who.is_empty() {
            who = r.user_id.to_string();
        }
        ui.add(egui::Label::new(RichText::new(who).family(crate::ui::fonts::demi()).color(c.text_strong)).truncate());
        if let Some(at) = r.created_ms {
            ui.label(RichText::new(crate::i18n::time::long_date(lang, at)).small().color(c.muted));
        }
        for (question, reply) in &r.answers {
            ui.add(egui::Label::new(RichText::new(question).small().color(c.muted)).wrap());
            ui.add(egui::Label::new(RichText::new(reply).color(c.text)).wrap());
        }
        let free = deciding.is_none();
        if *rejecting == Some(r.id) {
            crate::ui::widgets::field(ui, t, reason, nt::JOIN_REASON_HINT.get(lang));
            cap(reason, REASON_MAX);
            ui.horizontal(|ui| {
                if ui.add_enabled(free, crate::ui::form::danger(t, nt::JOIN_REJECT_CONFIRM.get(lang))).clicked() {
                    act = Some(Act::Decide(r.id, false));
                }
                if ui.add(crate::ui::form::link(t, crate::i18n::table::guilds::t::CANCEL.get(lang))).clicked() {
                    *rejecting = None;
                    reason.clear();
                }
            });
        } else {
            ui.horizontal(|ui| {
                if ui.add_enabled(free, crate::ui::form::btn(t, nt::JOIN_APPROVE.get(lang))).clicked() {
                    act = Some(Act::Decide(r.id, true));
                }
                if ui.add_enabled(free, crate::ui::form::btn(t, nt::JOIN_REJECT.get(lang))).clicked() {
                    *rejecting = Some(r.id);
                    reason.clear();
                }
                if deciding == Some(r.id) {
                    ui.spinner();
                }
            });
        }
    });
    act
}
