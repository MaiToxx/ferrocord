//! Événements programmés côté membre (lot A7) : bouton « Événements » en
//! haut de la liste des salons, fenêtre de la liste (couverture, date, lieu,
//! description mise en forme, intéressés, statut), « Ça m'intéresse », lien
//! de partage, et pour qui en a le droit : créer, modifier, commencer,
//! terminer, annuler, supprimer (ces deux-là après confirmation). Les
//! entrées du menu d'un serveur (événements, accueil, paramètres) et les
//! messages de fin des actions du lot sont ici aussi.

use super::util::paint_image;
use super::App;
use crate::i18n::table::events::{f, p, t as tx};
use crate::i18n::{time, Lang, Text};
use crate::state::admin::community::{self, Arrival, CommunityAction, Onboarding};
use crate::state::admin::events::{local_fields, local_ms, DraftError, EventAction, EventDraft, EventNotice, Failure, Frequency, Place, Recurrence, Repeat, ScheduledEvent, Status};
use crate::state::model::ImageRef;
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText, Sense};

/// Action à confirmer d'un second clic.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Ask {
    Delete,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PlaceKind {
    Stage,
    Voice,
    External,
}

/// Fenêtre de création ou de modification.
pub(super) struct Editor {
    guild: u64,
    event: Option<u64>,
    name: String,
    description: String,
    date: String,
    time: String,
    has_end: bool,
    end_date: String,
    end_time: String,
    kind: PlaceKind,
    channel: u64,
    location: String,
    repeat: Repeat,
    /// Règle reçue (gardée telle quelle si la répétition n'est pas changée).
    rule: Option<Recurrence>,
    error: Option<Text>,
    /// Nouvelle couverture choisie (`data:` PNG) ; `None` : inchangée.
    cover: Option<String>,
    /// Boîte « Ouvrir » de la couverture (fil à part), créée au premier clic.
    cover_picker: Option<crate::guild_icon::IconPicker>,
    cover_unreadable: bool,
}

/// Ce qu'un clic sur une carte demande (appliqué après le dessin).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CardAct {
    Interest(bool),
    Copy(String),
    Edit,
    /// Sauter (`true`) ou rétablir la prochaine occurrence d'un événement récurrent.
    Skip(bool),
    Status(Status),
    Ask(Ask),
    Confirm,
    Dismiss,
}

/// Entrées du menu d'un serveur (lot A7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum AdminPick {
    Events,
    Arrival(Arrival),
    /// « Parcourir les salons ».
    Browse,
}

#[derive(Default)]
pub(super) struct EventsUi {
    /// Liste des événements ouverte pour ce serveur.
    pub(super) list: Option<u64>,
    pub(super) editor: Option<Editor>,
    /// Événement dont l'action attend son second clic.
    pub(super) confirm: Option<(u64, Ask)>,
    /// Réponses cochées dans la fenêtre d'intégration : (serveur, options).
    pub(super) answers: Option<(u64, Vec<u64>)>,
    /// Brouillons des pages des paramètres du serveur.
    pub(super) welcome_draft: Option<super::guild_settings::welcome::Draft>,
    pub(super) onboarding_draft: Option<(u64, Onboarding)>,
}

pub(super) fn failure_text(lang: Lang, f: &Failure, h24: bool) -> String {
    match f {
        Failure::Problem(p) => super::words::problem(lang, p, h24),
        Failure::NotFound => tx::NOT_FOUND.get(lang).to_string(),
    }
}

/// Message d'un échec : l'A2F se dit seule (même message que les autres pages).
fn failed_text(lang: Lang, action: &str, fl: &Failure, h24: bool) -> String {
    match fl {
        Failure::Problem(crate::state::Problem::MfaRequired) => crate::i18n::table::admin::t::MFA_REQUIRED.get(lang).to_string(),
        other => f::failed(lang, action, failure_text(lang, other, h24)),
    }
}

fn event_action(a: EventAction) -> Text {
    match a {
        EventAction::Load => tx::ACT_LOAD,
        EventAction::Create => tx::ACT_CREATE,
        EventAction::Edit => tx::ACT_EDIT,
        EventAction::Status => tx::ACT_STATUS,
        EventAction::Delete => tx::ACT_DELETE,
        EventAction::Interest => tx::ACT_INTEREST,
        EventAction::Occurrence => tx::ACT_OCCURRENCE,
    }
}

fn community_action(a: CommunityAction) -> Text {
    match a {
        CommunityAction::Welcome => tx::ACT_WELCOME,
        CommunityAction::SaveWelcome => tx::ACT_SAVE_WELCOME,
        CommunityAction::Onboarding => tx::ACT_ONBOARDING,
        CommunityAction::SaveOnboarding => tx::ACT_SAVE_ONBOARDING,
        CommunityAction::Respond => tx::ACT_RESPOND,
    }
}

pub(super) fn draft_error(e: DraftError) -> Text {
    match e {
        DraftError::Name => tx::ERR_NAME,
        DraftError::Description => tx::ERR_DESCRIPTION,
        DraftError::Location => tx::ERR_LOCATION,
        DraftError::EndRequired => tx::ERR_END_REQUIRED,
        DraftError::EndBeforeStart => tx::ERR_END_BEFORE,
        DraftError::StartPast => tx::ERR_START_PAST,
        DraftError::Channel => tx::ERR_CHANNEL,
        DraftError::Busy => tx::BUSY,
    }
}

pub(super) fn save_error(e: community::SaveError) -> Text {
    use community::SaveError as E;
    match e {
        E::TooLong => tx::ERR_TOO_LONG,
        E::TooMany => tx::ERR_TOO_MANY,
        E::Empty => tx::ERR_EMPTY,
        E::NoOption => tx::ERR_NO_OPTION,
        E::Answer => tx::ERR_ANSWER,
        E::Busy => tx::BUSY,
    }
}

fn repeat_label(r: Repeat) -> Text {
    match r {
        Repeat::None => tx::REPEAT_NONE,
        Repeat::Daily => tx::REPEAT_DAILY,
        Repeat::Weekdays => tx::REPEAT_WEEKDAYS,
        Repeat::Weekly => tx::REPEAT_WEEKLY,
        Repeat::Biweekly => tx::REPEAT_BIWEEKLY,
        Repeat::Monthly => tx::REPEAT_MONTHLY,
        Repeat::Yearly => tx::REPEAT_YEARLY,
    }
}

/// Règle de récurrence en mots.
pub(super) fn repeat_text(lang: Lang, r: &Recurrence) -> String {
    let day = |d: u8| time::weekday(lang, usize::from(d));
    let nth = [tx::NTH_1, tx::NTH_2, tx::NTH_3, tx::NTH_4, tx::NTH_5];
    match r.frequency {
        Frequency::Daily if r.by_weekday == [0, 1, 2, 3, 4] => tx::REPEAT_WEEKDAYS.get(lang).to_string(),
        Frequency::Daily => tx::REPEAT_DAILY.get(lang).to_string(),
        Frequency::Weekly => match r.by_weekday.first() {
            Some(d) if r.interval == 2 => f::repeat_biweekly(lang, day(*d)),
            Some(d) => f::repeat_weekly(lang, day(*d)),
            None => tx::REPEAT_WEEKLY.get(lang).to_string(),
        },
        Frequency::Monthly => match r.by_n_weekday {
            Some((n, d)) if (1..=5).contains(&n) => f::repeat_monthly(lang, nth[usize::from(n - 1)].get(lang), day(d)),
            _ => tx::REPEAT_MONTHLY.get(lang).to_string(),
        },
        Frequency::Yearly => match r.by_month_day {
            Some((m, d)) if (1..=12).contains(&m) => f::repeat_yearly(lang, d, time::month(lang, usize::from(m - 1))),
            _ => tx::REPEAT_YEARLY.get(lang).to_string(),
        },
    }
}

/// Début de l'heure suivante, au moins une heure plus tard (proposition de création).
fn next_hour(now_ms: i64) -> i64 {
    (now_ms / 3_600_000 + 2) * 3_600_000
}

fn editor_draft(ed: &Editor) -> Result<EventDraft, Text> {
    let start_ms = local_ms(&ed.date, &ed.time).ok_or(tx::ERR_DATE)?;
    let end_ms = if ed.has_end || ed.kind == PlaceKind::External { Some(local_ms(&ed.end_date, &ed.end_time).ok_or(tx::ERR_DATE)?) } else { None };
    let place = match ed.kind {
        PlaceKind::Stage => Place::Stage(ed.channel),
        PlaceKind::Voice => Place::Voice(ed.channel),
        PlaceKind::External => Place::External(ed.location.clone()),
    };
    let recurrence = if ed.event.is_some() && Repeat::of(ed.rule.as_ref()) == ed.repeat { ed.rule.clone() } else { ed.repeat.rule(start_ms) };
    Ok(EventDraft { name: ed.name.clone(), description: ed.description.clone(), start_ms, end_ms, place, recurrence, image: ed.cover.clone() })
}

fn primary<'a>(t: &'a Tokens, text: &'a str) -> crate::ui::form::Btn<'a> {
    crate::ui::form::primary(t, text)
}

fn danger<'a>(t: &'a Tokens, text: &'a str) -> crate::ui::form::Btn<'a> {
    crate::ui::form::danger(t, text)
}

impl App {
    fn guild_name_of(&self, guild: u64) -> Option<String> {
        self.store.guilds.iter().find(|g| g.id == guild).map(|g| super::words::guild_name(self.lang, &g.name).to_string())
    }

    /// « Événements » en haut de la liste des salons, avec le nombre à venir.
    pub(super) fn events_entry(&self, ui: &mut egui::Ui, guild: u64) -> egui::Response {
        let t = &self.theme;
        let n = self.store.events.upcoming(guild);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), Sense::click());
        if resp.hovered() {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
        }
        let (icon, left) = t.channel_row_parts(rect, Default::default());
        let label = tx::EVENTS.get(self.lang);
        let color = if n > 0 { t.colors.text_strong } else if resp.hovered() { t.colors.text } else { t.colors.muted };
        crate::theme::icons::paint(crate::theme::icons::Icon::Calendar, ui.painter(), icon, if n > 0 { t.colors.text_strong } else { t.colors.faint });
        let font = egui::FontId::proportional(t.typography.body);
        ui.painter().text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER, label, font.clone(), color);
        if n > 0 {
            let count = crate::i18n::number(self.lang, n as u64);
            ui.painter().text(egui::pos2(rect.right() - t.spacing.item_x, rect.center().y), egui::Align2::RIGHT_CENTER, count, font, t.colors.accent);
        }
        resp
    }

    /// Ouvre la liste des événements d'un serveur (et la redemande à Discord).
    pub(super) fn open_events(&mut self, guild: u64) {
        self.events_ui.list = Some(guild);
        self.events_ui.confirm = None;
        self.store.load_events(guild);
    }

    /// Entrées du menu d'un serveur ; `None` si rien n'a été choisi.
    pub(super) fn admin_menu(&self, ui: &mut egui::Ui, guild: u64) -> Option<AdminPick> {
        let lang = self.lang;
        let mut pick = None;
        if crate::ui::menu::item(ui, &self.theme, tx::EVENTS.get(lang), Some(crate::theme::icons::Icon::Calendar), false).clicked() {
            pick = Some(AdminPick::Events);
        }
        let info = self.store.guild_info(guild);
        if info.is_some_and(|i| i.has(community::GUILD_ONBOARDING)) && crate::ui::menu::item(ui, &self.theme, tx::CHANNELS_ROLES.get(lang), Some(crate::theme::icons::Icon::Apps), false).clicked() {
            pick = Some(AdminPick::Arrival(Arrival::Onboarding(guild)));
        }
        let browse = info.is_some_and(|i| i.has(community::GUILD_ONBOARDING)) || self.store.browse_only_followed(guild);
        if browse && crate::ui::menu::item(ui, &self.theme, tx::BROWSE_CHANNELS.get(lang), Some(crate::theme::icons::Icon::Hash), false).clicked() {
            pick = Some(AdminPick::Browse);
        }
        if info.is_some_and(|i| i.has(community::WELCOME_SCREEN_ENABLED)) && crate::ui::menu::item(ui, &self.theme, tx::PAGE_WELCOME.get(lang), Some(crate::theme::icons::Icon::Eye), false).clicked() {
            pick = Some(AdminPick::Arrival(Arrival::Welcome(guild)));
        }
        crate::ui::menu::separator(ui, &self.theme);
        if pick.is_some() {
            ui.close();
        }
        pick
    }

    pub(super) fn apply_events_pick(&mut self, guild: u64, pick: AdminPick) {
        match pick {
            AdminPick::Events => self.open_events(guild),
            AdminPick::Arrival(a) => self.store.open_arrival(a),
            AdminPick::Browse => super::browse::open(&self.ctx, guild),
        }
    }

    /// Ouvre la fenêtre de création (`event` : `None`) ou de modification.
    pub(super) fn open_editor(&mut self, guild: u64, event: Option<u64>) {
        let venues = self.store.community.venues(guild);
        let first = |stage: bool| venues.iter().find(|v| v.stage == stage).map_or(0, |v| v.id);
        let ed = match event.and_then(|id| self.store.find_event(guild, id)) {
            Some(e) => {
                let (date, time) = local_fields(e.start_ms);
                let (end_date, end_time) = local_fields(e.end_ms.unwrap_or(e.start_ms + 3_600_000));
                let (kind, channel, location) = match &e.place {
                    Place::Stage(c) => (PlaceKind::Stage, *c, String::new()),
                    Place::Voice(c) => (PlaceKind::Voice, *c, String::new()),
                    Place::External(l) => (PlaceKind::External, 0, l.clone()),
                    Place::Other => (PlaceKind::External, 0, String::new()),
                };
                Editor {
                    guild,
                    event: Some(e.id),
                    name: e.name.clone(),
                    description: e.description.clone(),
                    date,
                    time,
                    has_end: e.end_ms.is_some(),
                    end_date,
                    end_time,
                    kind,
                    channel,
                    location,
                    repeat: Repeat::of(e.recurrence.as_ref()),
                    rule: e.recurrence.clone(),
                    error: None,
                    cover: None,
                    cover_picker: None,
                    cover_unreadable: false,
                }
            }
            None => {
                let start = next_hour(self.store.now());
                let (date, time) = local_fields(start);
                let (end_date, end_time) = local_fields(start + 3_600_000);
                let (kind, channel) = match (first(false), first(true)) {
                    (v, _) if v != 0 => (PlaceKind::Voice, v),
                    (_, s) if s != 0 => (PlaceKind::Stage, s),
                    _ => (PlaceKind::External, 0),
                };
                Editor {
                    guild,
                    event: None,
                    name: String::new(),
                    description: String::new(),
                    date,
                    time,
                    has_end: false,
                    end_date,
                    end_time,
                    kind,
                    channel,
                    location: String::new(),
                    repeat: Repeat::None,
                    rule: None,
                    error: None,
                    cover: None,
                    cover_picker: None,
                    cover_unreadable: false,
                }
            }
        };
        self.events_ui.editor = Some(ed);
    }

    /// Carte d'un événement ; `manage` : contrôles de gestion affichés ;
    /// `confirm` : action attendant son second clic sur cette carte.
    pub(super) fn event_card(&self, ui: &mut egui::Ui, t: &Tokens, e: &ScheduledEvent, manage: bool, confirm: Option<Ask>) -> Option<CardAct> {
        let lang = self.lang;
        let h24 = self.h24();
        let now = self.store.now();
        let mut act = None;
        egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_embed()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(hash) = e.image {
                // 16:9, pas plus haute que la moitié de la largeur d'une carte.
                let h = (ui.available_width() * 9.0 / 16.0).min(t.layout.card_width * 0.5);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(h * 16.0 / 9.0, h), Sense::hover());
                if !paint_image(ui, &self.images, rect, Some(ImageRef::EventCover { event_id: e.id, hash }), t.r_embed()) {
                    ui.painter().rect_filled(rect, t.r_embed(), t.colors.bg3);
                }
            }
            let mut when = time::stamp(lang, e.start_ms, now, 'F', h24);
            if let Some(end) = e.end_ms {
                when = format!("{when} {}", f::until(lang, time::stamp(lang, end, now, 'F', h24)));
            }
            ui.horizontal_wrapped(|ui| {
                let status = match e.status {
                    Status::Active => Some((tx::LIVE, t.colors.accent)),
                    Status::Completed => Some((tx::COMPLETED, t.colors.muted)),
                    Status::Canceled => Some((tx::CANCELED, t.colors.danger)),
                    Status::Scheduled => None,
                };
                if let Some((s, c)) = status {
                    ui.label(RichText::new(s.get(lang)).family(crate::ui::fonts::demi()).strong().color(c));
                }
                ui.label(RichText::new(when).small().color(t.colors.muted));
            });
            if let Some(r) = &e.recurrence {
                ui.label(RichText::new(repeat_text(lang, r)).small().color(t.colors.muted));
                if e.next_skipped() {
                    ui.label(RichText::new(tx::NEXT_SKIPPED.get(lang)).small().color(t.colors.danger));
                }
            }
            ui.label(RichText::new(&e.name).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
            let place = self.place_text(e.guild_id, &e.place);
            if !place.is_empty() {
                ui.label(RichText::new(place).color(t.colors.text));
            }
            if !e.description.is_empty() {
                self.bio_ui(ui, t, e.id, Some(e.guild_id), &e.description);
            }
            if let Some(n) = e.user_count {
                ui.label(RichText::new(p::interested(lang, u64::from(n))).small().color(t.colors.muted));
            }
            ui.horizontal_wrapped(|ui| {
                if e.upcoming() {
                    let on = self.store.event_interested(e.guild_id, e.id);
                    let busy = self.store.events.pending(EventAction::Interest, e.id);
                    let b = if on { crate::ui::form::btn(t, tx::NOT_INTERESTED.get(lang)) } else { primary(t, tx::INTERESTED.get(lang)) };
                    if ui.add_enabled(!busy, b).clicked() {
                        act = Some(CardAct::Interest(!on));
                    }
                }
                if ui.add(crate::ui::form::btn(t, tx::COPY_LINK.get(lang))).clicked() {
                    act = Some(CardAct::Copy(e.share_url()));
                }
                if manage {
                    let busy = self.store.events.pending(EventAction::Status, e.id) || self.store.events.pending(EventAction::Delete, e.id);
                    if e.upcoming() && ui.add_enabled(!busy, crate::ui::form::btn(t, tx::EDIT.get(lang))).clicked() {
                        act = Some(CardAct::Edit);
                    }
                    if e.status.can_become(Status::Active) && ui.add_enabled(!busy, crate::ui::form::btn(t, tx::START.get(lang))).clicked() {
                        act = Some(CardAct::Status(Status::Active));
                    }
                    if e.status.can_become(Status::Completed) && ui.add_enabled(!busy, crate::ui::form::btn(t, tx::END.get(lang))).clicked() {
                        act = Some(CardAct::Status(Status::Completed));
                    }
                    if e.recurrence.is_some() && e.status == Status::Scheduled {
                        let skipped = e.next_skipped();
                        let label = if skipped { tx::UNSKIP_NEXT } else { tx::SKIP_NEXT };
                        let busy = busy || self.store.events.pending(EventAction::Occurrence, e.id);
                        if ui.add_enabled(!busy, crate::ui::form::btn(t, label.get(lang))).clicked() {
                            act = Some(CardAct::Skip(!skipped));
                        }
                    }
                    if e.status.can_become(Status::Canceled) && ui.add_enabled(!busy, crate::ui::form::btn(t, tx::CANCEL_EVENT.get(lang))).clicked() {
                        act = Some(CardAct::Ask(Ask::Cancel));
                    }
                    if ui.add_enabled(!busy, danger(t, tx::DELETE.get(lang))).clicked() {
                        act = Some(CardAct::Ask(Ask::Delete));
                    }
                    if busy {
                        ui.spinner();
                    }
                }
            });
            if let Some(ask) = confirm {
                let q = match ask {
                    Ask::Delete => f::confirm_delete(lang, &e.name),
                    Ask::Cancel => f::confirm_cancel(lang, &e.name),
                };
                ui.label(RichText::new(q).color(t.colors.danger));
                ui.horizontal(|ui| {
                    if ui.add(danger(t, tx::CONFIRM.get(lang))).clicked() {
                        act = Some(CardAct::Confirm);
                    }
                    if ui.add(crate::ui::form::btn(t, tx::CANCEL.get(lang))).clicked() {
                        act = Some(CardAct::Dismiss);
                    }
                });
            }
        });
        act
    }

    pub(super) fn place_text(&self, guild: u64, place: &Place) -> String {
        let lang = self.lang;
        let name = |id: u64| {
            self.store.community.venues(guild).iter().find(|v| v.id == id).map(|v| v.name.clone()).unwrap_or_else(|| tx::UNKNOWN_CHANNEL.get(lang).to_string())
        };
        match place {
            Place::Stage(c) => f::stage_at(lang, name(*c)),
            Place::Voice(c) => f::voice_at(lang, name(*c)),
            Place::External(l) => l.clone(),
            Place::Other => String::new(),
        }
    }

    /// Cartes des événements à venir d'un serveur, avec l'action demandée
    /// (identifiant de l'événement) ; partagé avec la page des paramètres.
    pub(super) fn events_list_body(&self, ui: &mut egui::Ui, t: &Tokens, guild: u64) -> Option<(u64, CardAct)> {
        let lang = self.lang;
        let events: Vec<&ScheduledEvent> = self.store.events.guild(guild).map(|g| g.list.iter().filter(|e| e.upcoming()).collect()).unwrap_or_default();
        let loading = self.store.events.pending(EventAction::Load, guild);
        if events.is_empty() {
            ui.horizontal(|ui| {
                if loading {
                    ui.spinner();
                    ui.label(tx::LOADING.get(lang));
                } else {
                    ui.label(RichText::new(tx::NO_EVENTS.get(lang)).color(t.colors.muted));
                }
            });
            return None;
        }
        let mut out = None;
        for e in events {
            let manage = self.store.can_manage_event(e);
            let confirm = self.events_ui.confirm.filter(|(id, _)| *id == e.id).map(|(_, a)| a);
            if let Some(a) = self.event_card(ui, t, e, manage, confirm) {
                out = Some((e.id, a));
            }
            ui.add_space(t.spacing.item_y);
        }
        out
    }

    pub(super) fn apply_card(&mut self, ctx: &egui::Context, guild: u64, id: u64, act: CardAct) {
        match act {
            CardAct::Interest(on) => {
                self.store.set_event_interest(guild, id, on);
            }
            CardAct::Copy(url) => {
                ctx.copy_text(url);
                self.notify(tx::LINK_COPIED);
            }
            CardAct::Edit => self.open_editor(guild, Some(id)),
            CardAct::Skip(skip) => {
                self.store.skip_occurrence(guild, id, skip);
            }
            CardAct::Status(s) => {
                self.store.set_event_status(guild, id, s);
            }
            CardAct::Ask(a) => self.events_ui.confirm = Some((id, a)),
            CardAct::Dismiss => self.events_ui.confirm = None,
            CardAct::Confirm => {
                match self.events_ui.confirm.take() {
                    Some((_, Ask::Delete)) => self.store.delete_event(guild, id),
                    Some((_, Ask::Cancel)) => self.store.set_event_status(guild, id, Status::Canceled),
                    None => false,
                };
            }
        }
    }

    /// Fenêtres et messages du lot A7 (chaque image).
    pub(super) fn admin_events_ui(&mut self, ui: &mut egui::Ui) {
        let (lang, h24) = (self.lang, self.h24());
        while let Some(EventNotice::Failed { action, failure }) = self.store.events.take_notice() {
            let text = failed_text(lang, event_action(action).get(lang), &failure, h24);
            self.notify(super::ToastText::Text(text));
        }
        while let Some((action, failure)) = self.store.community.take_notice() {
            let text = failed_text(lang, community_action(action).get(lang), &failure, h24);
            self.notify(super::ToastText::Text(text));
        }
        if let Some((action, _)) = self.store.events.take_done()
            && matches!(action, EventAction::Create | EventAction::Edit)
        {
            self.events_ui.editor = None;
        }
        if let Some((action, guild)) = self.store.community.take_saved() {
            match action {
                CommunityAction::Respond => {
                    self.notify(tx::ANSWERS_SAVED);
                    if self.store.community.show == Some(Arrival::Onboarding(guild)) {
                        self.store.community.show = None;
                        self.events_ui.answers = None;
                    }
                }
                CommunityAction::SaveWelcome => {
                    self.notify(tx::SAVED);
                    self.events_ui.welcome_draft = None;
                }
                CommunityAction::SaveOnboarding => {
                    self.notify(tx::SAVED);
                    self.events_ui.onboarding_draft = None;
                }
                CommunityAction::Welcome | CommunityAction::Onboarding => {}
            }
        }
        self.events_window(ui);
        self.editor_window(ui);
        self.arrival_windows(ui);
    }

    fn events_window(&mut self, ui: &mut egui::Ui) {
        let Some(guild) = self.events_ui.list else { return };
        let Some(gname) = self.guild_name_of(guild) else {
            self.events_ui.list = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let can_create = self.store.can_create_events(guild);
        let (mut act, mut create, mut close) = (None, false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.6);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("evenements"), width, &f::events_of(lang, &gname), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            act = self.events_list_body(ui, &t, guild);
        }, |ui| {
            if can_create {
                create = ui.add(primary(&t, tx::CREATE_EVENT.get(lang))).clicked();
            }
            close = ui.add(crate::ui::form::link(&t, tx::CLOSE.get(lang))).clicked();
        });
        if let Some((id, a)) = act {
            self.apply_card(ui.ctx(), guild, id, a);
        }
        if create {
            self.open_editor(guild, None);
        }
        if close || (modal.closed && self.events_ui.editor.is_none()) {
            self.events_ui.list = None;
            self.events_ui.confirm = None;
        }
    }

    fn editor_window(&mut self, ui: &mut egui::Ui) {
        let Some(guild) = self.events_ui.editor.as_ref().map(|e| e.guild) else { return };
        if self.guild_name_of(guild).is_none() {
            self.events_ui.editor = None;
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let venues = self.store.community.venues(guild).to_vec();
        let busy = self.store.events.pending(EventAction::Create, guild) || self.events_ui.editor.as_ref().and_then(|e| e.event).is_some_and(|id| self.store.events.pending(EventAction::Edit, id));
        let (mut save, mut close) = (false, false);
        let Some(ed) = self.events_ui.editor.as_mut() else { return };
        let editing = ed.event.is_some();
        let title = if editing { tx::EDIT_EVENT } else { tx::CREATE_EVENT };
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.4);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("evenement-edition"), width, title.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            {
                crate::ui::settings::shell::page_style(ui, &t);
                ui.label(RichText::new(tx::PLACE.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                ui.horizontal_wrapped(|ui| {
                    for (k, label) in [(PlaceKind::Stage, tx::STAGE), (PlaceKind::Voice, tx::VOICE), (PlaceKind::External, tx::EXTERNAL)] {
                        if ui.radio_value(&mut ed.kind, k, label.get(lang)).changed() {
                            let stage = k == PlaceKind::Stage;
                            ed.channel = venues.iter().find(|v| v.stage == stage).map_or(0, |v| v.id);
                        }
                    }
                });
                match ed.kind {
                    PlaceKind::External => {
                        crate::ui::form::field(ui, &t, &mut ed.location, tx::LOCATION_HINT.get(lang), crate::state::admin::events::LOCATION_MAX);
                    }
                    k => {
                        let stage = k == PlaceKind::Stage;
                        let choices: Vec<_> = venues.iter().filter(|v| v.stage == stage).collect();
                        if choices.is_empty() {
                            ui.label(RichText::new(tx::NO_VENUE.get(lang)).color(t.colors.muted));
                        } else {
                            let current = choices.iter().find(|v| v.id == ed.channel).map(|v| v.name.clone()).unwrap_or_default();
                            egui::ComboBox::from_id_salt("evenement-salon").selected_text(current).show_ui(ui, |ui| {
                                for v in &choices {
                                    ui.selectable_value(&mut ed.channel, v.id, &v.name);
                                }
                            });
                        }
                    }
                }
                ui.label(RichText::new(tx::NAME.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                crate::ui::form::field(ui, &t, &mut ed.name, "", crate::state::admin::events::NAME_MAX);
                // Largeurs à la mesure du texte attendu (« 2026-11-21 », « 20:30 »).
                // « 07:00 » en entier : le champ de l'heure coupait son dernier chiffre.
                let (date_w, time_w) = (t.typography.body * 7.5, t.typography.body * 5.5);
                ui.horizontal(|ui| {
                    ui.label(tx::START_DATE.get(lang));
                    ui.add(egui::TextEdit::singleline(&mut ed.date).hint_text(tx::DATE_HINT.get(lang)).desired_width(date_w));
                    ui.add(egui::TextEdit::singleline(&mut ed.time).hint_text(tx::TIME_HINT.get(lang)).desired_width(time_w));
                });
                ui.horizontal(|ui| {
                    let external = ed.kind == PlaceKind::External;
                    if external {
                        ui.label(tx::END_DATE.get(lang));
                    } else {
                        ui.checkbox(&mut ed.has_end, tx::HAS_END.get(lang));
                    }
                    if external || ed.has_end {
                        ui.add(egui::TextEdit::singleline(&mut ed.end_date).hint_text(tx::DATE_HINT.get(lang)).desired_width(date_w));
                        ui.add(egui::TextEdit::singleline(&mut ed.end_time).hint_text(tx::TIME_HINT.get(lang)).desired_width(time_w));
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(tx::REPEAT.get(lang));
                    egui::ComboBox::from_id_salt("evenement-repetition").selected_text(repeat_label(ed.repeat).get(lang)).show_ui(ui, |ui| {
                        for r in Repeat::ALL {
                            ui.selectable_value(&mut ed.repeat, r, repeat_label(r).get(lang));
                        }
                    });
                });
                // Couverture (boîte « Ouvrir », lue et réduite dans un fil à part).
                if let Some(out) = ed.cover_picker.as_ref().and_then(crate::guild_icon::IconPicker::poll) {
                    match out {
                        crate::guild_icon::Outcome::Ready(p) => (ed.cover, ed.cover_unreadable) = (Some(p.data_uri), false),
                        crate::guild_icon::Outcome::Unreadable => ed.cover_unreadable = true,
                    }
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(tx::COVER.get(lang));
                    let picking = ed.cover_picker.as_ref().is_some_and(crate::guild_icon::IconPicker::busy);
                    if crate::guild_icon::IconPicker::CAN_PICK && ui.add_enabled(!picking, crate::ui::form::btn(&t, tx::COVER_PICK.get(lang))).clicked() {
                        let ctx = ui.ctx().clone();
                        let picker = ed.cover_picker.get_or_insert_with(|| crate::guild_icon::IconPicker::with_fit(std::sync::Arc::new(move || ctx.request_repaint()), crate::guild_icon::COVER));
                        #[cfg(windows)]
                        picker.pick(tx::COVER.get(lang));
                        #[cfg(not(windows))]
                        let _ = picker;
                    }
                    if picking {
                        ui.spinner();
                    }
                    if ed.cover.is_some() {
                        ui.label(RichText::new(tx::COVER_CHOSEN.get(lang)).color(t.colors.muted));
                        if ui.add(crate::ui::form::btn(&t, tx::COVER_KEEP.get(lang))).clicked() {
                            ed.cover = None;
                        }
                    }
                });
                if ed.cover_unreadable {
                    ui.label(RichText::new(tx::COVER_UNREADABLE.get(lang)).color(t.colors.danger));
                }
                ui.label(RichText::new(tx::DESCRIPTION.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                ui.add(egui::TextEdit::multiline(&mut ed.description).char_limit(crate::state::admin::events::DESCRIPTION_MAX).desired_rows(4).desired_width(f32::INFINITY));
                if let Some(e) = ed.error {
                    ui.label(RichText::new(e.get(lang)).color(t.colors.danger));
                }
            }
        }, |ui| {
            let label = if editing { tx::SAVE } else { tx::CREATE };
            save = ui.add_enabled(!busy, primary(&t, label.get(lang))).clicked();
            close = ui.add(crate::ui::form::link(&t, tx::CANCEL.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        });
        if save && let Some(ed) = self.events_ui.editor.as_ref() {
            let (g, id) = (ed.guild, ed.event);
            let res = editor_draft(ed).and_then(|d| match id {
                Some(id) => self.store.edit_event(g, id, &d).map_err(draft_error),
                None => self.store.create_event(g, &d).map_err(draft_error),
            });
            if let Some(ed) = self.events_ui.editor.as_mut() {
                ed.error = res.err();
            }
        }
        if close || modal.closed {
            self.events_ui.editor = None;
        }
    }

    /// Ouvre la liste des événements (aperçu, tests).
    pub fn show_events(&mut self, guild: u64) {
        self.open_events(guild);
        self.flush_commands();
    }

    /// Ouvre la fenêtre de création d'un événement (aperçu, tests).
    pub fn show_event_editor(&mut self, guild: u64, name: &str) {
        self.open_editor(guild, None);
        if let Some(ed) = self.events_ui.editor.as_mut() {
            ed.name = name.to_string();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rules_in_words() {
        let r = |frequency, interval, by_weekday: Vec<u8>, by_n_weekday, by_month_day| Recurrence { frequency, interval, by_weekday, by_n_weekday, by_month_day };
        assert_eq!(repeat_text(Lang::Fr, &r(Frequency::Weekly, 1, vec![3], None, None)), "Chaque semaine, le jeudi");
        assert_eq!(repeat_text(Lang::En, &r(Frequency::Weekly, 2, vec![0], None, None)), "Every other week on Monday");
        assert_eq!(repeat_text(Lang::Fr, &r(Frequency::Monthly, 1, vec![], Some((1, 4)), None)), "Chaque mois, le premier vendredi");
        assert_eq!(repeat_text(Lang::En, &r(Frequency::Yearly, 1, vec![], None, Some((11, 21)))), "Annually on November 21");
        assert_eq!(repeat_text(Lang::Fr, &r(Frequency::Daily, 1, vec![0, 1, 2, 3, 4], None, None)), "Du lundi au vendredi");
        assert_eq!(repeat_text(Lang::Fr, &r(Frequency::Monthly, 1, vec![], Some((9, 4)), None)), "Chaque mois");
    }

    #[test]
    fn editor_turns_into_a_draft() {
        let ed = Editor {
            guild: 1,
            event: None,
            name: "Soirée".into(),
            description: String::new(),
            date: "2026-11-21".into(),
            time: "20:30".into(),
            has_end: false,
            end_date: "2026-11-21".into(),
            end_time: "22:00".into(),
            kind: PlaceKind::Voice,
            channel: 9,
            location: String::new(),
            repeat: Repeat::Weekly,
            rule: None,
            error: None,
            cover: None,
            cover_picker: None,
            cover_unreadable: false,
        };
        let d = editor_draft(&ed).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!((d.place, d.end_ms, d.recurrence.map(|r| r.frequency)), (Place::Voice(9), None, Some(Frequency::Weekly)));
        let ext = Editor { kind: PlaceKind::External, location: "Parc".into(), ..ed };
        let d = editor_draft(&ext).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(d.end_ms.map(|e| e - d.start_ms), Some(90 * 60_000), "fin obligatoire ailleurs");
        let bad = Editor { time: "20h30".into(), ..ext };
        assert_eq!(editor_draft(&bad).err(), Some(tx::ERR_DATE));
        // Couverture choisie : envoyée avec le brouillon ; sinon inchangée.
        assert_eq!(d.image, None);
        let covered = Editor { time: "20:30".into(), cover: Some("data:image/png;base64,AA==".into()), ..bad };
        assert_eq!(editor_draft(&covered).ok().and_then(|d| d.image).as_deref(), Some("data:image/png;base64,AA=="));
        assert!(next_hour(3_600_000 * 5 + 10) == 3_600_000 * 7);
    }
}
