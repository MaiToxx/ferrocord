//! Plugin « Messages programmés » (lot E3), à envoi **confirmé**.
//!
//! « Programmer… » (menu « + » de la saisie) range le texte avec une date
//! et une heure, sur cet ordinateur (`state::scheduled`, fichier chiffré du
//! dossier `programmes`). À l'heure dite, **rien n'est envoyé** : une
//! notification locale et un bandeau « Message programmé pour #salon »
//! apparaissent, et l'envoi ne part que du clic sur « Envoyer »
//! ([`App::scheduled_send`], seul endroit de ce fichier qui envoie, appelé
//! seulement par un bouton). Application fermée à l'heure dite : le bandeau
//! est là au lancement suivant. « Messages programmés » liste tout
//! (modifier, supprimer, envoyer maintenant).
//!
//! Réveil : à chaque passage de logique, un seul réveil est demandé, pour
//! la prochaine échéance à venir ; aucune minuterie ne tourne au repos.
//! Plugin coupé : une comparaison par passage, ni bandeau ni entrée de
//! menu ; les messages déjà programmés restent dans leur fichier.

use super::widgets::{self, Kind as Button};
use super::{menu, App};
use crate::i18n::table::e3::{f as xf, t as xt};
use crate::i18n::{t as tx, Lang, Text};
use crate::offline::vault::{Kind, Payload};
use crate::plugins::{self, Id as Plugin};
use crate::state::scheduled::{Scheduled, MAX_TEXT};
use crate::theme::{icons::Icon, Tokens};
use chrono::{Duration as Span, Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};
use eframe::egui::{self, RichText};
use std::collections::HashSet;
use std::time::Duration;

/// Largeur du contenu des fenêtres.
const WIDTH: f32 = 460.0;

/// Fenêtre « Programmer un message » ou « Modifier ».
struct Dialog {
    /// Message modifié ; `None` : nouveau.
    editing: Option<u64>,
    channel_id: u64,
    place: String,
    text: String,
    date: String,
    time: String,
    error: Option<Text>,
    /// Le texte vient de la saisie : elle est vidée quand il est programmé.
    from_composer: bool,
}

#[derive(Default)]
pub(in crate::ui) struct ScheduledUi {
    on: bool,
    user: Option<u64>,
    asked: Option<u64>,
    loaded: Option<u64>,
    known: Option<Vec<u64>>,
    data: Scheduled,
    /// Échéances déjà annoncées par une notification.
    alerted: HashSet<u64>,
    dialog: Option<Dialog>,
    /// La liste est ouverte.
    page: bool,
}

/// Date et heure saisies → instant local. Date : `AAAA-MM-JJ` ou `JJ/MM/AAAA` ;
/// heure : `HH:MM` (ou `HHhMM`).
pub(super) fn parse_when(date: &str, time: &str) -> Option<NaiveDateTime> {
    let date = date.trim();
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").or_else(|_| NaiveDate::parse_from_str(date, "%d/%m/%Y")).ok()?;
    let time = time.trim().replace(['h', 'H'], ":");
    let clock = NaiveTime::parse_from_str(&time, "%H:%M").ok()?;
    Some(day.and_time(clock))
}

/// Instant local → millisecondes (heure ambiguë d'un changement d'heure : la première).
fn local_ms(when: NaiveDateTime) -> Option<i64> {
    Local.from_local_datetime(&when).earliest().map(|d| d.timestamp_millis())
}

fn fields_of(ms: i64) -> (String, String) {
    let d = Local.timestamp_millis_opt(ms).earliest().map(|d| d.naive_local()).unwrap_or_default();
    (d.format("%Y-%m-%d").to_string(), d.format("%H:%M").to_string())
}

/// Première ligne d'un texte, pour le bandeau et la liste.
fn excerpt(text: &str, max: usize) -> String {
    let line = text.lines().find(|l| !l.trim().is_empty()).unwrap_or_default().trim();
    if line.chars().count() > max { format!("{}…", line.chars().take(max).collect::<String>().trim_end()) } else { line.to_string() }
}

/// Ce qu'un clic de la liste ou du bandeau demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Act {
    Send(u64),
    Edit(u64),
    Delete(u64),
    Postpone(u64, i64),
    OpenList,
}

impl App {
    /// Le plugin se sert du fil des fichiers.
    pub(in crate::ui) fn scheduled_uses_vault(&self) -> bool {
        self.scheduled.on
    }

    /// Messages programmés du compte (tests et aperçu).
    #[doc(hidden)]
    pub fn scheduled_items(&self) -> &[crate::state::scheduled::Item] {
        self.scheduled.data.items()
    }

    /// Nom lisible d'un salon : « #salon », ou le nom d'un message privé.
    fn scheduled_place(&self, cid: u64) -> String {
        match self.store.dms.iter().find(|d| d.id == cid) {
            Some(d) => d.name.clone(),
            None => self.store.channel_name(cid).map(|n| format!("#{n}")).unwrap_or_default(),
        }
    }

    /// « Programmer… » : la fenêtre s'ouvre sur le texte de la saisie.
    pub(super) fn scheduled_open(&mut self, cid: u64) {
        if !plugins::on(Plugin::Scheduled) {
            return;
        }
        let mut text = self.composer_encoded();
        match crate::state::builtin::apply(&text) {
            Some(crate::state::builtin::Builtin::Text(t)) => text = t,
            // `/nick` agit tout de suite : il ne se programme pas.
            Some(crate::state::builtin::Builtin::Nick(_)) => return self.notify(xt::SCHED_NO_COMMAND),
            None => {}
        }
        let (date, time) = fields_of(crate::state::store::now_ms() + 3_600_000);
        self.scheduled.dialog = Some(Dialog { editing: None, channel_id: cid, place: self.scheduled_place(cid), text, date, time, error: None, from_composer: true });
    }

    /// Ouvre la liste des messages programmés.
    pub fn scheduled_open_list(&mut self) {
        self.scheduled.page = plugins::on(Plugin::Scheduled);
    }

    /// Range un message pour `due_ms` (fenêtre « Programmer », tests, aperçu).
    /// Rien n'est envoyé ; `None` : texte vide ou liste pleine.
    pub fn schedule_message(&mut self, cid: u64, content: &str, due_ms: i64) -> Option<u64> {
        let place = self.scheduled_place(cid);
        let id = self.scheduled.data.add(cid, &place, content, due_ms);
        self.ctx.request_repaint();
        id
    }

    /// **Le seul envoi de ce plugin**, appelé par un clic (« Envoyer »,
    /// « Envoyer maintenant ») : la requête d'envoi ordinaire. Refusé (salon
    /// fermé, mode lent, message trop long) : le message reste programmé.
    pub fn scheduled_send(&mut self, id: u64) -> bool {
        let Some(item) = self.scheduled.data.get(id).cloned() else { return false };
        if item.content.chars().count() > super::complete::MESSAGE_MAX.max(crate::state::scheduled::MAX_TEXT) {
            self.notify(tx::MESSAGE_TOO_LONG);
            return false;
        }
        if self.store.slowmode_left(item.channel_id, self.store.now()).is_some() || !self.store.send_message(item.channel_id, item.content) {
            self.notify(tx::CANNOT_SEND);
            return false;
        }
        self.scheduled.data.remove(id);
        self.scheduled.alerted.remove(&id);
        self.ctx.request_repaint();
        true
    }

    pub(super) fn scheduled_act(&mut self, act: Act) {
        match act {
            Act::Send(id) => {
                self.scheduled_send(id);
            }
            Act::Delete(id) => {
                self.scheduled.data.remove(id);
                self.scheduled.alerted.remove(&id);
            }
            Act::Postpone(id, due) => {
                self.scheduled.data.postpone(id, due);
                self.scheduled.alerted.remove(&id);
            }
            Act::Edit(id) => {
                if let Some(i) = self.scheduled.data.get(id) {
                    let (date, time) = fields_of(i.due_ms.max(crate::state::store::now_ms() + 600_000));
                    self.scheduled.dialog = Some(Dialog { editing: Some(id), channel_id: i.channel_id, place: i.place.clone(), text: i.content.clone(), date, time, error: None, from_composer: false });
                }
            }
            Act::OpenList => self.scheduled.page = true,
        }
        self.ctx.request_repaint();
    }

    fn scheduled_save(&mut self) {
        let Some(user) = self.scheduled.user else { return };
        if self.scheduled.loaded != Some(user) || !std::mem::take(&mut self.scheduled.data.dirty) {
            return;
        }
        if !self.scheduled.known.as_ref().is_some_and(|k| k.contains(&user)) {
            return;
        }
        if let Some(files) = self.journal.vault.files() {
            if self.scheduled.data.is_empty() {
                files.purge(Kind::Scheduled, user);
            } else {
                files.save(user, Payload::Raw(Kind::Scheduled, self.scheduled.data.encode(user)));
            }
        }
    }

    /// Messages relus du disque : ils rejoignent ceux programmés depuis le lancement.
    pub(in crate::ui) fn scheduled_loaded(&mut self, user: u64, bytes: Option<Vec<u8>>) {
        if !self.scheduled.on || self.scheduled.user != Some(user) || self.scheduled.asked != Some(user) {
            return;
        }
        self.scheduled.loaded = Some(user);
        if let Some(loaded) = bytes.and_then(|b| Scheduled::decode(&b, user)) {
            self.scheduled.data.merge(loaded);
            self.ctx.request_repaint();
        }
    }

    /// À chaque passage de logique. Plugin coupé : une comparaison, rien d'autre.
    pub(in crate::ui) fn scheduled_tick(&mut self) {
        let on = plugins::on(Plugin::Scheduled);
        if !on && !self.scheduled.on {
            return;
        }
        // `local.toml` pas encore lu : l'état du plugin n'est pas encore le vrai.
        if !self.local.loaded {
            return;
        }
        if !on {
            // Coupé : plus rien n'est montré ni réveillé ; le fichier reste.
            self.scheduled_save();
            self.scheduled = ScheduledUi::default();
            self.vault_release_if_idle();
            return;
        }
        self.scheduled.on = true;
        let me = self.store.me.as_ref().map(|u| u.id);
        if me != self.scheduled.user {
            self.scheduled_save();
            self.scheduled = ScheduledUi { on: true, user: me, known: self.scheduled.known.take(), ..Default::default() };
        }
        if self.journal.vault.attached() {
            let mut known: Vec<u64> = self.prefs.now.accounts.iter().filter(|a| !self.forgetting.contains(&a.key)).filter_map(|a| a.user_id).collect();
            known.sort_unstable();
            known.dedup();
            if self.scheduled.known.as_ref() != Some(&known) {
                if let Some(files) = self.journal.vault.files() {
                    files.keep_only(Kind::Scheduled, known.clone());
                }
                self.scheduled.known = Some(known);
            }
            if let Some(u) = me
                && self.scheduled.asked != Some(u)
            {
                self.scheduled.asked = Some(u);
                if self.scheduled.known.as_ref().is_some_and(|k| k.contains(&u))
                    && let Some(files) = self.journal.vault.files()
                {
                    files.load(Kind::Scheduled, u);
                } else {
                    self.scheduled.loaded = Some(u);
                }
            }
            self.vault_receive();
        } else if self.scheduled.loaded != me {
            self.scheduled.asked = me;
            self.scheduled.loaded = me;
        }
        if self.scheduled.data.dirty {
            self.scheduled_save();
        }
        if self.scheduled.data.is_empty() {
            return;
        }
        // Échéances : une notification locale par message, et le bandeau.
        // Rien n'est envoyé ici.
        let now = crate::state::store::now_ms();
        let fresh: Vec<(u64, u64, String)> = self.scheduled.data.due(now).filter(|i| !self.scheduled.alerted.contains(&i.id)).map(|i| (i.id, i.channel_id, i.place.clone())).collect();
        for (id, channel, place) in fresh {
            self.scheduled.alerted.insert(id);
            let lang = self.lang;
            self.desk_alert(crate::state::notify::Alert { guild: None, channel, kind: crate::state::notify::Kind::Message, title: xt::SCHED_TOAST_TITLE.get(lang).to_string(), place: None, body: xf::sched_toast_body(lang, &place), avatar: None, header: None });
            self.ctx.request_repaint();
        }
        // Un seul réveil : la prochaine échéance à venir.
        if let Some(next) = self.scheduled.data.next_due(now) {
            self.ctx.request_repaint_after(Duration::from_millis(u64::try_from(next - now).unwrap_or(0)));
        }
    }

    /// Application quittée : ce qui restait à écrire l'est.
    pub(in crate::ui) fn scheduled_exit(&mut self) {
        if self.scheduled.on && self.local.loaded {
            self.scheduled_save();
        }
    }

    /// Bandeau en haut de la fenêtre tant qu'un message est arrivé à
    /// échéance : « Envoyer », « Reporter », « Supprimer ».
    pub(super) fn scheduled_banner(&mut self, ui: &mut egui::Ui) {
        if !self.scheduled.on || self.scheduled.data.is_empty() {
            return;
        }
        let now = crate::state::store::now_ms();
        let mut due = self.scheduled.data.due(now);
        let Some(first) = due.next().cloned() else { return };
        let more = due.count();
        let t = self.theme.clone();
        let lang = self.lang;
        let mut act = None;
        let frame = egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).inner_margin(egui::Margin::symmetric(t.spacing.chat_head_x as i8, t.spacing.item_y as i8));
        egui::Panel::top("message_programme").frame(frame).resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                super::util::inline_icon(ui, Icon::Clock, t.typography.body, t.colors.accent);
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if widgets::button(ui, &t, Button::Link, tx::DELETE.get(lang)).clicked() {
                        act = Some(Act::Delete(first.id));
                    }
                    let later = widgets::button(ui, &t, Button::Secondary, xt::SCHED_POSTPONE.get(lang));
                    egui::Popup::menu(&later).id(egui::Id::unique("ferrocord.programme.reporter")).show(|ui| {
                        for (label, by) in [(xt::SCHED_P10, 600_000), (xt::SCHED_P60, 3_600_000), (xt::SCHED_PDAY, 86_400_000)] {
                            if menu::item(ui, &t, label.get(lang), None, false).clicked() {
                                act = Some(Act::Postpone(first.id, now + by));
                            }
                        }
                    });
                    if widgets::button(ui, &t, Button::Primary, xt::SCHED_SEND.get(lang)).clicked() {
                        act = Some(Act::Send(first.id));
                    }
                    if more > 0 && widgets::button(ui, &t, Button::Link, &xf::sched_more(lang, more)).clicked() {
                        act = Some(Act::OpenList);
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let text = format!("{} — {}", xf::sched_banner(lang, &first.place), excerpt(&first.content, 120));
                        ui.add(egui::Label::new(RichText::new(text).color(t.colors.text)).truncate());
                    });
                });
            });
        });
        if let Some(a) = act {
            self.scheduled_act(a);
        }
    }

    /// Fenêtres du plugin : « Programmer » et la liste.
    pub(super) fn scheduled_ui(&mut self, ctx: &egui::Context) {
        if !self.scheduled.on {
            return;
        }
        let t = self.theme.clone();
        self.scheduled_list(ctx, &t);
        self.scheduled_dialog(ctx, &t);
    }

    fn scheduled_list(&mut self, ctx: &egui::Context, t: &Tokens) {
        if !self.scheduled.page {
            return;
        }
        let lang = self.lang;
        let h24 = self.h24();
        let now = crate::state::store::now_ms();
        let mut act = None;
        let items = self.scheduled.data.items();
        let out = widgets::sheet(ctx, t, egui::Id::unique("ferrocord.programmes"), WIDTH, xt::SCHED_LIST.get(lang), tx::CLOSE_TIP.get(lang), |ui| {
            super::settings::note(ui, t, xt::SCHED_CONFIRM_NOTE.get(lang));
            ui.add_space(t.spacing.item_x);
            if items.is_empty() {
                ui.label(RichText::new(xt::SCHED_NONE.get(lang)).color(t.colors.muted));
            }
            for i in items {
                let due = i.due_ms <= now;
                egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&i.place).family(super::fonts::demi()).color(t.colors.text_strong));
                        ui.label(RichText::new(crate::i18n::time::stamp(lang, i.due_ms, now, 'f', h24)).small().color(t.colors.muted));
                        if due {
                            ui.label(RichText::new(xt::SCHED_DUE.get(lang)).small().family(super::fonts::demi()).color(t.colors.accent));
                        }
                    });
                    ui.add(egui::Label::new(RichText::new(excerpt(&i.content, 240)).color(t.colors.text)).wrap());
                    ui.add_space(t.spacing.item_y);
                    ui.horizontal(|ui| {
                        ui.push_id(i.id, |ui| {
                            if widgets::button(ui, t, if due { Button::Primary } else { Button::Secondary }, xt::SCHED_SEND_NOW.get(lang)).clicked() {
                                act = Some(Act::Send(i.id));
                            }
                            if widgets::button(ui, t, Button::Secondary, xt::SCHED_EDIT.get(lang)).clicked() {
                                act = Some(Act::Edit(i.id));
                            }
                            if widgets::button(ui, t, Button::Link, tx::DELETE.get(lang)).clicked() {
                                act = Some(Act::Delete(i.id));
                            }
                        });
                    });
                });
                ui.add_space(t.spacing.item_y);
            }
        });
        if out.closed && self.scheduled.dialog.is_none() {
            self.scheduled.page = false;
        }
        if let Some(a) = act {
            self.scheduled_act(a);
        }
    }

    fn scheduled_dialog(&mut self, ctx: &egui::Context, t: &Tokens) {
        let Some(mut d) = self.scheduled.dialog.take() else { return };
        let lang = self.lang;
        let now = crate::state::store::now_ms();
        let title = if d.editing.is_some() { xt::SCHED_TITLE_EDIT } else { xt::SCHED_TITLE_NEW };
        let (mut ok, mut cancel) = (false, false);
        let out = widgets::modal(
            ctx,
            t,
            egui::Id::unique("ferrocord.programmer"),
            WIDTH,
            title.get(lang),
            tx::CLOSE_TIP.get(lang),
            |ui| {
                ui.label(RichText::new(xf::sched_for(lang, &d.place)).color(t.colors.muted));
                ui.add_space(t.spacing.item_y);
                widgets::field_label(ui, t, xt::SCHED_TEXT.get(lang));
                let edit = egui::TextEdit::multiline(&mut d.text).desired_rows(4).desired_width(f32::INFINITY).char_limit(MAX_TEXT);
                let resp = ui.add(edit);
                super::a11y::name(ui.ctx(), resp.id, xt::SCHED_TEXT.get(lang));
                ui.add_space(t.spacing.item_y);
                ui.columns(2, |cols| {
                    widgets::field_label(&mut cols[0], t, xt::SCHED_DATE.get(lang));
                    let r = super::form::field(&mut cols[0], t, &mut d.date, xt::SCHED_DATE_HINT.get(lang), 10);
                    super::a11y::name(cols[0].ctx(), r.id, xt::SCHED_DATE.get(lang));
                    widgets::field_label(&mut cols[1], t, xt::SCHED_TIME.get(lang));
                    let r = super::form::field(&mut cols[1], t, &mut d.time, xt::SCHED_TIME_HINT.get(lang), 5);
                    super::a11y::name(cols[1].ctx(), r.id, xt::SCHED_TIME.get(lang));
                });
                ui.add_space(t.spacing.item_y);
                ui.horizontal_wrapped(|ui| {
                    let today = Local::now().naive_local();
                    let quick = [
                        (xt::SCHED_IN_1H, today + Span::hours(1)),
                        (xt::SCHED_TONIGHT, today.date().and_hms_opt(20, 0, 0).unwrap_or(today)),
                        (xt::SCHED_TOMORROW, (today.date() + Span::days(1)).and_hms_opt(9, 0, 0).unwrap_or(today)),
                    ];
                    for (label, when) in quick {
                        if when > today && widgets::compact(ui, label.get(lang)).clicked() {
                            d.date = when.format("%Y-%m-%d").to_string();
                            d.time = when.format("%H:%M").to_string();
                            d.error = None;
                        }
                    }
                });
                ui.add_space(t.spacing.item_y);
                super::settings::note(ui, t, xt::SCHED_CONFIRM_NOTE.get(lang));
                if let Some(e) = d.error {
                    ui.add_space(t.spacing.item_y);
                    ui.label(RichText::new(e.get(lang)).color(t.colors.danger));
                }
            },
            |ui| {
                ok = widgets::button(ui, t, Button::Primary, if d.editing.is_some() { tx::SAVE.get(lang) } else { xt::SCHED_DO.get(lang) }).clicked();
                cancel = widgets::button(ui, t, Button::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        if cancel || (out.closed && !out.veil_clicked) {
            return;
        }
        if ok {
            let due = parse_when(&d.date, &d.time).and_then(local_ms);
            d.error = match due {
                None => Some(xt::SCHED_BAD_DATE),
                Some(ms) if ms <= now => Some(xt::SCHED_PAST),
                Some(_) if d.text.trim().is_empty() => Some(xt::SCHED_NO_TEXT),
                Some(ms) => {
                    let done = match d.editing {
                        Some(id) => self.scheduled.data.edit(id, &d.text, ms),
                        None => self.scheduled.data.add(d.channel_id, &d.place, &d.text, ms).is_some(),
                    };
                    if done {
                        if let Some(id) = d.editing {
                            self.scheduled.alerted.remove(&id);
                        }
                        if d.from_composer {
                            self.clear_composer();
                        }
                        let when = crate::i18n::time::stamp(lang, ms, now, 'f', self.h24());
                        self.notify(xf::sched_done(lang, &d.place, when));
                        self.ctx.request_repaint();
                        return;
                    }
                    Some(xt::SCHED_FULL)
                }
            };
        }
        self.scheduled.dialog = Some(d);
    }

    /// Aperçu hors ligne : plugin activé, un message échu (bandeau) et un à
    /// venir ; `list` : la liste ouverte ; `dialog` : la fenêtre « Programmer ».
    #[doc(hidden)]
    pub fn scheduled_preview(&mut self, list: bool, dialog: bool) {
        self.local.loaded = true;
        self.set_plugin(Plugin::Scheduled, true);
        self.scheduled_tick();
        let Some(cid) = self.store.channel else { return };
        let now = crate::state::store::now_ms();
        let texts = match self.lang {
            Lang::Fr => ["Bon anniversaire\u{a0}! 🎉", "Rappel\u{a0}: la réunion commence dans dix minutes."],
            _ => ["Happy birthday! 🎉", "Reminder: the meeting starts in ten minutes."],
        };
        self.schedule_message(cid, texts[0], now - 60_000);
        self.schedule_message(cid, texts[1], now + 26 * 3_600_000);
        self.scheduled.alerted.extend(self.scheduled.data.items().iter().map(|i| i.id));
        self.scheduled.page = list;
        if dialog {
            self.set_composer_text(texts[1]);
            self.scheduled_open(cid);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Dm, User};
    use crate::state::{Cmd, Event};
    use crate::ui::tests::{frame, offline_boot};
    use std::sync::{Arc, Mutex};

    const C: u64 = 100;

    /// Un texte affiché (libellé d'egui : sa valeur ; élément nommé : son nom) contient `text`.
    fn shows(out: &egui::FullOutput, text: &str) -> bool {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        update.nodes.iter().any(|(_, n)| n.label().is_some_and(|l| l.contains(text)) || n.value().is_some_and(|v| v.contains(text)))
    }

    /// Clic sur l'élément dont le nom est exactement `label`.
    fn click(app: &mut App, label: &str) {
        let mut out = frame(app, Vec::new());
        for _ in 0..3 {
            out = frame(app, Vec::new());
        }
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        let b = update.nodes.iter().find(|(_, n)| n.label() == Some(label)).and_then(|(_, n)| n.bounds()).unwrap_or_else(|| panic!("« {label} » introuvable"));
        let at = egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32);
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(app, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(app, vec![button(false)]);
        frame(app, Vec::new());
    }


    #[derive(Clone, Default)]
    struct Net {
        sent: Arc<Mutex<Vec<Cmd>>>,
    }

    impl crate::ui::Backend for Net {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            self.sent.lock().unwrap().extend(cmds);
        }
    }

    impl Net {
        fn sends(&self) -> Vec<(u64, String)> {
            self.sent.lock().unwrap().iter().filter_map(|c| if let Cmd::Send { channel_id, content, .. } = c { Some((*channel_id, content.clone())) } else { None }).collect()
        }
    }

    fn app(net: Net) -> App {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), s, images));
        app.lang = Lang::Fr;
        app.local.loaded = true;
        app
    }

    #[test]
    fn dates_are_parsed_in_both_forms() {
        let want = NaiveDate::from_ymd_opt(2026, 12, 24).unwrap().and_hms_opt(18, 30, 0).unwrap();
        assert_eq!(parse_when("2026-12-24", "18:30"), Some(want));
        assert_eq!(parse_when(" 24/12/2026 ", "18h30"), Some(want));
        assert_eq!(parse_when("2026-13-01", "18:30"), None);
        assert_eq!(parse_when("2026-12-24", "25:00"), None);
        assert_eq!(parse_when("demain", "midi"), None);
        assert_eq!(excerpt("\n  une ligne\nautre", 40), "une ligne");
        assert_eq!(excerpt("abcdef", 3), "abc…");
    }

    /// Le cœur du plugin : un message échu n'est JAMAIS envoyé sans clic.
    /// Des centaines de passages après l'échéance, fenêtre active ou non,
    /// liste ouverte ou non : aucune commande d'envoi. Le clic sur
    /// « Envoyer » en fait partir une, et une seule.
    #[test]
    fn a_due_message_is_never_sent_without_a_click() {
        let net = Net::default();
        let mut app = app(net.clone());
        assert!(plugins::on(Plugin::Scheduled));
        frame(&mut app, Vec::new());
        let now = crate::state::store::now_ms();
        let id = app.schedule_message(C, "bon anniversaire", now - 5_000).unwrap();
        app.schedule_message(C, "pour plus tard", now + 3_600_000).unwrap();
        for n in 0..300 {
            let events = if n % 2 == 0 { vec![egui::Event::WindowFocused(n % 4 == 0)] } else { Vec::new() };
            let out = frame(&mut app, events);
            if n == 150 {
                assert!(shows(&out, "Message programmé pour Alice"), "le bandeau est là");
                app.scheduled_open_list();
            }
        }
        std::thread::sleep(Duration::from_millis(50));
        frame(&mut app, Vec::new());
        assert!(net.sends().is_empty(), "rien n'est parti tout seul : {:?}", net.sends());
        assert!(net.sent.lock().unwrap().iter().all(|c| !matches!(c, Cmd::Upload(_) | Cmd::Typing { .. })));
        assert_eq!(app.scheduled_items().len(), 2, "le message échu attend toujours");
        assert!(app.scheduled.alerted.contains(&id), "notification locale donnée une fois");
        // Le clic : la requête d'envoi ordinaire, une fois.
        app.scheduled.page = false;
        frame(&mut app, Vec::new());
        click(&mut app, xt::SCHED_SEND.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert_eq!(net.sends(), vec![(C, "bon anniversaire".to_string())]);
        assert_eq!(app.scheduled_items().len(), 1);
        for _ in 0..50 {
            frame(&mut app, Vec::new());
        }
        assert_eq!(net.sends().len(), 1, "l'autre, pas encore échu, n'est pas parti non plus");
        let out = frame(&mut app, Vec::new());
        assert!(!shows(&out, "Message programmé pour Alice"), "plus de bandeau");
    }

    /// Onglet « Programmés » de la boîte de réception : les messages y sont
    /// listés avec leurs actions ; l'ouvrir n'envoie rien, « Supprimer » retire.
    #[test]
    fn inbox_tab_lists_scheduled_messages_and_sends_nothing() {
        let net = Net::default();
        let mut app = app(net.clone());
        frame(&mut app, Vec::new());
        let now = crate::state::store::now_ms();
        app.schedule_message(C, "**bon** anniversaire", now + 3_600_000).unwrap();
        app.show_inbox_tab("programmes");
        frame(&mut app, Vec::new());
        let out = frame(&mut app, Vec::new());
        assert!(shows(&out, "Programmés"), "onglet présent quand le plugin est activé");
        assert!(shows(&out, "bon anniversaire") && !shows(&out, "**"), "texte mis en forme");
        assert!(shows(&out, xt::SCHED_SEND_NOW.get(Lang::Fr)));
        click(&mut app, tx::DELETE.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert!(app.scheduled_items().is_empty());
        assert!(net.sends().is_empty(), "rien n'est parti");
        // Plugin coupé : plus d'onglet.
        app.set_plugin(Plugin::Scheduled, false);
        frame(&mut app, Vec::new());
        let out = frame(&mut app, Vec::new());
        assert!(!shows(&out, "Programmés"));
    }

    #[test]
    fn postpone_delete_and_disabled_plugin() {
        let net = Net::default();
        let mut app = app(net.clone());
        frame(&mut app, Vec::new());
        let now = crate::state::store::now_ms();
        let a = app.schedule_message(C, "un", now - 1000).unwrap();
        let b = app.schedule_message(C, "deux", now - 500).unwrap();
        app.scheduled_act(Act::Postpone(a, now + 600_000));
        frame(&mut app, Vec::new());
        assert_eq!(app.scheduled.data.due(crate::state::store::now_ms()).map(|i| i.id).collect::<Vec<_>>(), vec![b]);
        app.scheduled_act(Act::Delete(b));
        let out = frame(&mut app, Vec::new());
        assert!(!shows(&out, "Message programmé pour"));
        assert_eq!(app.scheduled_items().len(), 1);
        // Plugin coupé : ni bandeau, ni fenêtre, ni envoi, même échu.
        app.scheduled_act(Act::Postpone(a, now - 1));
        app.set_plugin(Plugin::Scheduled, false);
        for _ in 0..20 {
            let out = frame(&mut app, Vec::new());
            assert!(!shows(&out, "Message programmé pour"));
        }
        assert!(!app.scheduled.on && app.scheduled_items().is_empty());
        app.scheduled_open(C);
        app.scheduled_open_list();
        assert!(app.scheduled.dialog.is_none() && !app.scheduled.page);
        assert!(net.sends().is_empty());
    }

    /// « Programmer… » part du texte de la saisie, refuse une date passée,
    /// puis range le message et vide la saisie ; rien n'est envoyé.
    #[test]
    fn dialog_schedules_the_composer_text() {
        let net = Net::default();
        let mut app = app(net.clone());
        frame(&mut app, Vec::new());
        app.set_composer_text("à plus tard");
        frame(&mut app, Vec::new());
        app.scheduled_open(C);
        let d = app.scheduled.dialog.as_mut().unwrap();
        assert_eq!((d.text.as_str(), d.place.as_str()), ("à plus tard", "Alice"));
        d.date = "2001-01-01".into();
        click(&mut app, xt::SCHED_DO.get(Lang::Fr));
        assert_eq!(app.scheduled.dialog.as_ref().and_then(|d| d.error), Some(xt::SCHED_PAST));
        assert!(app.scheduled_items().is_empty());
        let d = app.scheduled.dialog.as_mut().unwrap();
        d.date = "2099-01-01".into();
        d.time = "08:15".into();
        click(&mut app, xt::SCHED_DO.get(Lang::Fr));
        assert!(app.scheduled.dialog.is_none());
        assert_eq!(app.scheduled_items().iter().map(|i| i.content.as_str()).collect::<Vec<_>>(), vec!["à plus tard"]);
        assert_eq!(app.composer_text(), "", "le texte a quitté la saisie");
        assert!(net.sends().is_empty());
    }
}
