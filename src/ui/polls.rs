//! Lot T13 dans l'interface : carte d'un sondage dans son message (vote,
//! retrait, votants, fin), fenêtre « Créer un sondage » de la saisie,
//! « Publier » et « Suivre » des salons d'annonces, et l'aiguillage des
//! clics du lot (`ExtraHit` → `ExtraAction`). Les messages système sont dans
//! `system_msgs.rs`, les cartes d'invitation dans `invites.rs`.
//!
//! Tout ce qui se clique dans un message est rangé dans `Acts::hits` puis
//! rendu interactif après le message (comme les réactions) : la carte ne
//! fait que dessiner pendant la liste, et sa hauteur ne dépend que du
//! sondage (pas du survol ni de la sélection).

use super::msg_actions::{Action, Acts, Hit};
use super::App;
use crate::i18n::table::polls::{f, p, t as tx};
use crate::i18n::Lang;
use crate::images::Images;
use crate::state::announce::AnnounceNotice;
use crate::state::model::Message;
use crate::state::polls::{Poll, PollAction, PollDraft, Voters, DURATIONS, MAX_ANSWER, MAX_ANSWERS, MAX_QUESTION};
use crate::state::system_msgs::Ref;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Id, Rect, RichText, Sense, Vec2};

/// Zone cliquable d'un message, propre au lot T13.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ExtraHit {
    /// Ligne d'une réponse : voter (choix unique), cocher (choix multiples)
    /// ou, résultats affichés, voir les votants.
    Answer(u32),
    /// « Voter » avec les réponses cochées.
    Submit,
    Remove,
    End,
    /// Message visé par un message système, ou source d'une annonce publiée.
    Target(Ref),
    /// « Rejoindre » d'une carte d'invitation.
    Join(String),
}

/// Geste à appliquer après la liste.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum ExtraAction {
    Vote(u64, Vec<u32>),
    Toggle(u64, u32),
    Voters(u64, u32, egui::Pos2),
    End(u64),
    Target(Ref),
    Join(String),
    Publish(u64),
}

/// État de l'interface du lot (appartient à `App`).
#[derive(Default)]
pub(super) struct PollsUi {
    /// Fenêtre « Créer un sondage » : (salon, brouillon).
    create: Option<(u64, PollDraft)>,
    /// Votants affichés : (salon, message, réponse, position).
    voters: Option<(u64, u64, u32, egui::Pos2)>,
    /// Fenêtre « Suivre » ouverte pour ce salon d'annonces.
    follow: Option<u64>,
}

fn selection_id(message: u64) -> Id {
    Id::unique(("selection-sondage", message))
}

fn selection(ctx: &egui::Context, message: u64) -> Vec<u32> {
    ctx.data(|d| d.get_temp::<Vec<u32>>(selection_id(message))).unwrap_or_default()
}

/// Bouton dessiné (sa zone est rendue cliquable après le message).
fn drawn_button(ui: &mut egui::Ui, t: &Tokens, text: &str, primary: bool) -> Rect {
    let font = egui::TextStyle::Button.resolve(ui.style());
    let color = if primary { t.colors.on_accent } else { t.colors.button_text };
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, color);
    let pad = ui.spacing().button_padding;
    let (rect, _) = ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, t.r_row(), if primary { t.colors.accent } else { t.colors.button_bg });
    p.galley(rect.min + pad, galley, color);
    rect
}

/// Temps restant et instant du prochain changement de ce texte (ms).
fn remaining(lang: Lang, poll: &Poll, now: i64) -> (String, Option<i64>) {
    let Some(end) = poll.expiry_ms.filter(|_| !poll.ended(now)) else { return (tx::POLL_ENDED.get(lang).to_string(), None) };
    let left = end - now;
    const MIN: i64 = 60_000;
    const HOUR: i64 = 60 * MIN;
    const DAY: i64 = 24 * HOUR;
    let (text, unit) = if left >= DAY {
        (p::days_left(lang, (left / DAY) as u64), DAY)
    } else if left >= HOUR {
        (p::hours_left(lang, (left / HOUR) as u64), HOUR)
    } else {
        (p::minutes_left(lang, ((left + MIN - 1) / MIN).max(1) as u64), MIN)
    };
    (text, Some(left % unit + 1))
}

/// Carte du sondage d'un message. `live` : dessin réel (sinon mesure).
pub(super) fn poll_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts, live: bool) {
    let Some(poll) = m.meta.extra.as_ref().and_then(|e| e.poll.as_ref()) else { return };
    let now = acts.store.now();
    let ended = poll.ended(now);
    let results = ended || poll.voted();
    let can_vote = !ended && !poll.voted() && acts.rights.react_existing;
    let picked = if live { selection(ui.ctx(), m.id) } else { Vec::new() };
    let total = poll.total();
    ui.add_space(t.spacing.media_gap);
    let width = ui.available_width().min(t.layout.embed_width);
    t.card_frame().show(ui, |ui| {
        ui.set_width(width - t.embed_margin().sum().x);
        ui.horizontal(|ui| {
            super::util::inline_icon(ui, Icon::Poll, t.typography.body, t.colors.faint);
            ui.add(egui::Label::new(RichText::new(&poll.question).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).wrap());
        });
        let hint = if !poll.counts_known && results {
            tx::COUNTS_UNKNOWN
        } else if poll.multi {
            tx::SELECT_MANY
        } else {
            tx::SELECT_ONE
        };
        ui.label(RichText::new(hint.get(lang)).size(t.typography.small).color(t.colors.faint));
        // Réponses hautes (≈ 50 dans le client) : un champ et de l'air.
        let row_h = t.field_height() + 2.0 * t.spacing.item_y;
        let mark_side = Tokens::icon_side(t.typography.title);
        let font = egui::TextStyle::Body.resolve(ui.style());
        for a in &poll.answers {
            ui.add_space(t.spacing.item_y / 2.0);
            let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), row_h), Sense::hover());
            let p = ui.painter();
            let r = t.r_row();
            p.rect_filled(rect, r, t.colors.bg2);
            if results && poll.counts_known && total > 0 {
                let frac = a.count as f32 / total as f32;
                let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width() * frac, rect.height()));
                p.rect_filled(bar, r, t.colors.selected);
            }
            let marked = a.me || picked.contains(&a.id);
            let stroke = if marked { egui::Stroke::new(t.shapes.border.max(1.0), t.colors.accent) } else { t.border() };
            p.rect_stroke(rect, r, stroke, egui::StrokeKind::Inside);
            let pad = t.spacing.item_x * 1.5;
            // Emoji en image (comme les réactions), sinon son texte.
            let img = a.emoji.as_ref().and_then(super::emoji::Img::of);
            let label = match &a.emoji {
                Some(e) if img.is_none() => format!("{} {}", acts.st.emoji_text(&acts.ctx, e), a.text),
                _ => a.text.clone(),
            };
            let mut left = rect.left() + pad;
            if let Some(img) = img {
                let side = super::emoji::row_height(ui, &font);
                if live {
                    super::emoji::paint(ui, Rect::from_min_size(egui::pos2(left, rect.center().y - side / 2.0), Vec2::splat(side)), img);
                }
                left += side + pad;
            }
            let right = if results && poll.counts_known {
                let pct = if total == 0 { 0 } else { (u64::from(a.count) * 100 + u64::from(total) / 2) / u64::from(total) };
                format!("{} · {}", f::percent(lang, pct), p::votes(lang, u64::from(a.count)))
            } else {
                String::new()
            };
            let p = ui.painter();
            let right_g = p.layout_no_wrap(right, font.clone(), t.colors.muted);
            // Avant le vote : case ronde à droite, pleine (avec sa coche) pour une réponse choisie.
            let mut mark_w = 0.0;
            if can_vote {
                let c = egui::pos2(rect.right() - pad - mark_side / 2.0, rect.center().y);
                if marked {
                    p.circle_filled(c, mark_side / 2.0, t.colors.accent);
                    crate::theme::icons::paint(Icon::Check, p, Rect::from_center_size(c, Vec2::splat(mark_side * 0.7)), t.colors.on_accent);
                } else {
                    p.circle_stroke(c, mark_side / 2.0 - 1.0, egui::Stroke::new(2.0 * t.shapes.border.max(1.0), t.colors.faint));
                }
                mark_w = mark_side + pad;
            }
            let text_w = (rect.right() - left - 2.0 * pad - right_g.size().x - mark_w).max(0.0);
            let left_g = p.layout(label, font.clone(), if a.me { t.colors.text_strong } else { t.colors.text }, text_w);
            let y = rect.center().y;
            p.galley(egui::pos2(left, y - left_g.size().y.min(rect.height()) / 2.0), left_g, t.colors.text);
            p.galley(egui::pos2(rect.right() - pad - right_g.size().x, y - right_g.size().y / 2.0), right_g, t.colors.muted);
            if can_vote || results {
                acts.hits.push((rect, Hit::Extra(ExtraHit::Answer(a.id))));
            }
        }
        ui.add_space(t.spacing.item_y);
        ui.horizontal(|ui| {
            let (left, next) = remaining(lang, poll, now);
            ui.label(RichText::new(format!("{} · {left}", p::votes(lang, u64::from(total)))).size(t.typography.small).color(t.colors.faint));
            if live && let Some(ms) = next {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(u64::try_from(ms).unwrap_or(0)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if acts.store.can_end_poll(m) {
                    let r = drawn_button(ui, t, tx::END_POLL.get(lang), false);
                    acts.hits.push((r, Hit::Extra(ExtraHit::End)));
                }
                if !ended && poll.voted() {
                    let r = drawn_button(ui, t, tx::REMOVE_VOTE.get(lang), false);
                    acts.hits.push((r, Hit::Extra(ExtraHit::Remove)));
                } else if can_vote && poll.multi {
                    let r = drawn_button(ui, t, tx::VOTE.get(lang), !picked.is_empty());
                    acts.hits.push((r, Hit::Extra(ExtraHit::Submit)));
                }
            });
        });
    });
}

/// Clic sur une zone du lot, après le message.
pub(super) fn interact(lang: Lang, m: &Message, acts: &mut Acts, r: egui::Response, hit: ExtraHit) {
    let poll = m.meta.extra.as_ref().and_then(|e| e.poll.as_ref());
    let busy = acts.store.polls.voting(m.id);
    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
    let action = match hit {
        ExtraHit::Answer(id) => {
            let Some(poll) = poll else { return };
            let results = poll.ended(acts.store.now()) || poll.voted();
            let r = if results { r.on_hover_text(tx::SHOW_VOTERS.get(lang)) } else { r };
            if !r.clicked() {
                return;
            }
            if results {
                ExtraAction::Voters(m.id, id, r.rect.left_bottom())
            } else if poll.multi {
                ExtraAction::Toggle(m.id, id)
            } else if busy {
                return;
            } else {
                ExtraAction::Vote(m.id, vec![id])
            }
        }
        ExtraHit::Submit => {
            let picked = selection(&acts.ctx, m.id);
            if !r.clicked() || picked.is_empty() || busy {
                return;
            }
            ExtraAction::Vote(m.id, picked)
        }
        ExtraHit::Remove => {
            if !r.clicked() || busy {
                return;
            }
            ExtraAction::Vote(m.id, Vec::new())
        }
        ExtraHit::End => {
            if !r.clicked() || acts.store.polls.ending(m.id) {
                return;
            }
            ExtraAction::End(m.id)
        }
        ExtraHit::Target(t) => {
            if !r.clicked() {
                return;
            }
            ExtraAction::Target(t)
        }
        ExtraHit::Join(code) => {
            if !r.clicked() {
                return;
            }
            ExtraAction::Join(code)
        }
    };
    acts.st.push(Action::Extra(action));
}

/// Ce que le lot ajoute sous le texte d'un message : sondage, cartes
/// d'invitation, mention d'une annonce publiée.
pub(super) fn extras(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, m: &Message, acts: &mut Acts) {
    let Some(x) = m.meta.extra.as_deref() else { return };
    let live = images.is_some();
    poll_ui(ui, t, lang, m, acts, live);
    for code in &x.invites {
        super::invites::invite_card(ui, t, lang, images, code, acts);
    }
    if x.crossposted {
        ui.label(RichText::new(tx::PUBLISHED.get(lang)).size(t.typography.small).color(t.colors.faint));
    }
    if let Some(src) = x.source {
        let row = ui
            .horizontal(|ui| {
                super::util::inline_icon(ui, Icon::Megaphone, t.typography.small, t.colors.faint);
                ui.label(RichText::new(tx::SERVER_TAG.get(lang)).size(t.typography.small).family(crate::ui::fonts::demi()).strong().color(t.colors.accent));
                ui.label(RichText::new(tx::FROM_FOLLOWED.get(lang)).size(t.typography.small).color(t.colors.link));
            })
            .response
            .rect;
        if src.channel_id.is_some_and(|c| acts.store.channel_exists(c)) {
            acts.hits.push((row, Hit::Extra(ExtraHit::Target(src))));
        }
    }
}

impl App {
    pub(super) fn apply_extra(&mut self, cid: u64, a: ExtraAction) {
        match a {
            ExtraAction::Vote(mid, answers) => {
                self.store.vote(cid, mid, answers);
                self.ctx.data_mut(|d| d.remove::<Vec<u32>>(selection_id(mid)));
            }
            ExtraAction::Toggle(mid, answer) => {
                let mut sel = selection(&self.ctx, mid);
                if let Some(i) = sel.iter().position(|x| *x == answer) {
                    sel.remove(i);
                } else {
                    sel.push(answer);
                }
                self.ctx.data_mut(|d| d.insert_temp(selection_id(mid), sel));
            }
            ExtraAction::Voters(mid, answer, pos) => {
                self.store.request_voters(cid, mid, answer);
                self.polls_ui.voters = Some((cid, mid, answer, pos));
            }
            ExtraAction::End(mid) => {
                self.store.end_poll(cid, mid);
            }
            ExtraAction::Target(r) => {
                let channel = r.channel_id.unwrap_or(cid);
                match r.message_id {
                    // Message du salon ouvert, déjà en mémoire : on y va.
                    Some(mid) if channel == cid && self.store.find_message(cid, mid).is_some() => {
                        self.list.jump_to(cid, mid);
                        self.ctx.request_repaint();
                    }
                    _ if channel != cid && self.store.channel_exists(channel) => self.open_channel(channel),
                    // Message épinglé plus ancien : le panneau des épingles.
                    Some(_) if channel == cid => self.actions.pins_open = true,
                    _ => {}
                }
            }
            ExtraAction::Join(code) => {
                self.store.join_invite_card(&code);
            }
            ExtraAction::Publish(mid) => {
                self.store.publish(cid, mid);
            }
        }
    }

    /// Fenêtres et fenêtres surgissantes du lot, après la liste.
    pub(super) fn lot13_frame(&mut self, ui: &mut egui::Ui, cid: u64) {
        let (codes, urls) = super::invites::drain(ui.ctx());
        for code in codes {
            self.store.want_invite(&code);
        }
        if !urls.is_empty() {
            self.store.refresh_failed(&urls);
        }
        self.voters_popup(ui);
        self.poll_dialog(ui);
        self.follow_dialog(ui, cid);
    }

    /// Échecs et confirmations du lot, annoncés une fois.
    pub(super) fn lot13_notices(&mut self) {
        let lang = self.lang;
        let h24 = self.h24();
        let failed = |action: &str, p: &crate::state::Problem| super::ToastText::Text(f::failed(lang, action, super::words::problem(lang, p, h24)));
        if let Some((action, problem)) = self.store.take_poll_failure() {
            let word = match action {
                PollAction::Vote => tx::ACTION_VOTE,
                PollAction::End => tx::ACTION_END,
                PollAction::Create => tx::ACTION_CREATE,
            };
            self.notify(failed(word.get(lang), &problem));
        }
        match self.store.take_announce_notice() {
            Some(AnnounceNotice::Published) => self.notify(tx::PUBLISHED_OK),
            Some(AnnounceNotice::Followed) => self.notify(tx::FOLLOWED),
            Some(AnnounceNotice::Failed { follow, problem }) => {
                let word = if follow { tx::ACTION_FOLLOW } else { tx::ACTION_PUBLISH };
                self.notify(failed(word.get(lang), &problem));
            }
            None => {}
        }
    }

    /// « Créer un sondage » du menu « + » de la zone de saisie.
    pub(super) fn open_poll_creator(&mut self, cid: u64) {
        if self.store.can_create_poll(cid) {
            self.polls_ui.create = Some((cid, PollDraft::default()));
        }
    }

    /// Bouton « Suivre » de l'en-tête d'un salon d'annonces.
    pub(super) fn follow_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        if !self.store.is_announcement(cid) || self.store.follow_targets(cid).is_empty() {
            return;
        }
        let busy = self.store.announce.following(cid);
        if ui.add_enabled(!busy, egui::Button::new(RichText::new(tx::FOLLOW.get(self.lang)).small())).clicked() {
            self.polls_ui.follow = Some(cid);
        }
    }

    fn voters_popup(&mut self, ui: &mut egui::Ui) {
        let Some((cid, mid, answer, pos)) = self.polls_ui.voters else { return };
        let Some(poll) = self.store.poll_of(cid, mid) else {
            self.polls_ui.voters = None;
            return;
        };
        let t = &self.theme;
        let lang = self.lang;
        let title = poll.answers.iter().find(|a| a.id == answer).map(|a| a.text.clone()).unwrap_or_default();
        let area = egui::Area::new(Id::unique("votants")).order(egui::Order::Foreground).fixed_pos(pos).constrain(true);
        let resp = area.show(ui.ctx(), |ui| {
            egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(t.r_card()).inner_margin(t.card_margin()).show(ui, |ui| {
                ui.set_max_width(crate::ui::mobile::card_width(ui.ctx(), t, 1.0));
                crate::ui::settings::shell::page_style(ui, t);
                ui.label(RichText::new(f::voters_of(lang, &title)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                match self.store.polls.voters(mid, answer) {
                    None | Some(Voters::Loading) => {
                        ui.spinner();
                    }
                    Some(Voters::Failed(p)) => {
                        ui.label(RichText::new(super::words::problem(lang, p, self.theme.layout.clock == crate::theme::Clock::H24)).color(t.colors.danger));
                    }
                    Some(Voters::Ready(list)) if list.is_empty() => {
                        ui.label(RichText::new(tx::NO_VOTERS.get(lang)).color(t.colors.muted));
                    }
                    Some(Voters::Ready(list)) => {
                        egui::ScrollArea::vertical().max_height(t.layout.card_width).show(ui, |ui| {
                            for v in list {
                                ui.label(RichText::new(&v.name).color(t.colors.text));
                            }
                        });
                    }
                }
            });
        });
        let close = resp.response.clicked_elsewhere() || ui.input(|i| i.key_pressed(egui::Key::Escape));
        if close {
            self.polls_ui.voters = None;
        }
    }

    fn poll_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((cid, draft)) = self.polls_ui.create.as_mut() else { return };
        let cid = *cid;
        let t = &self.theme;
        let lang = self.lang;
        let (mut post, mut cancel) = (false, false);
        // Lu avant le contenu, qui emprunte le brouillon : l'état du bouton suit d'une image.
        let valid = draft.valid();
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            t,
            Id::unique("creer-sondage"),
            crate::ui::mobile::card_width(ui.ctx(), t, 1.0),
            tx::CREATE_POLL.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, t);
                ui.label(RichText::new(tx::QUESTION.get(lang)).small().color(t.colors.muted));
                let q = ui.add(egui::TextEdit::singleline(&mut draft.question).char_limit(MAX_QUESTION).hint_text(tx::QUESTION_HINT.get(lang)).desired_width(f32::INFINITY));
                if draft.question.is_empty() && !q.has_focus() && !ui.memory(|m| m.focused().is_some()) {
                    q.request_focus();
                }
                ui.label(RichText::new(tx::ANSWERS.get(lang)).small().color(t.colors.muted));
                let mut remove = None;
                let n = draft.answers.len();
                for (i, a) in draft.answers.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if n > 1 && super::util::icon_button(ui, t, Icon::Close, tx::REMOVE_ANSWER.get(lang)).clicked() {
                            remove = Some(i);
                        }
                        ui.add(egui::TextEdit::singleline(a).char_limit(MAX_ANSWER).hint_text(tx::ANSWER_HINT.get(lang)).desired_width(f32::INFINITY));
                    });
                }
                if let Some(i) = remove {
                    draft.answers.remove(i);
                }
                if draft.answers.len() < MAX_ANSWERS && ui.button(tx::ADD_ANSWER.get(lang)).clicked() {
                    draft.answers.push(String::new());
                }
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tx::DURATION.get(lang)).color(t.colors.muted));
                    let label = |h: u32| if h.is_multiple_of(24) { p::days(lang, u64::from(h / 24)) } else { p::hours(lang, u64::from(h)) };
                    egui::ComboBox::from_id_salt("duree-sondage").selected_text(label(draft.hours)).show_ui(ui, |ui| {
                        for h in DURATIONS {
                            ui.selectable_value(&mut draft.hours, h, label(h));
                        }
                    });
                });
                ui.checkbox(&mut draft.multi, tx::MULTI.get(lang));
            },
            |ui| {
                use crate::ui::widgets::{button, Kind};
                post = ui.add_enabled_ui(valid, |ui| button(ui, t, Kind::Primary, tx::POST.get(lang))).inner.clicked();
                cancel = button(ui, t, Kind::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        if post {
            let d = draft.clone();
            if self.store.create_poll(cid, d) {
                self.polls_ui.create = None;
            }
        } else if cancel || modal.closed {
            self.polls_ui.create = None;
        }
    }

    fn follow_dialog(&mut self, ui: &mut egui::Ui, cid: u64) {
        let Some(source) = self.polls_ui.follow else { return };
        if source != cid {
            self.polls_ui.follow = None;
            return;
        }
        let targets = self.store.follow_targets(source);
        let t = &self.theme;
        let lang = self.lang;
        let mut chosen = None;
        let mut cancel = false;
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            t,
            Id::unique("suivre-annonces"),
            crate::ui::mobile::card_width(ui.ctx(), t, 1.0),
            tx::FOLLOW_TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, t);
                ui.label(RichText::new(tx::FOLLOW_TIP.get(lang)).color(t.colors.muted));
                if targets.is_empty() {
                    ui.label(RichText::new(tx::NO_FOLLOW_TARGET.get(lang)).color(t.colors.faint));
                }
                egui::ScrollArea::vertical().max_height(t.layout.card_width).show(ui, |ui| {
                    for (id, name, guild) in &targets {
                        if ui.add(egui::Button::new(format!("#{name} ({guild})")).frame(false)).clicked() {
                            chosen = Some(*id);
                        }
                    }
                });
            },
            |ui| cancel = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked(),
        );
        if let Some(target) = chosen {
            self.store.follow(source, target);
            self.polls_ui.follow = None;
        } else if cancel || modal.closed {
            self.polls_ui.follow = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::polls::PollAnswer;

    #[test]
    fn remaining_time_and_next_change() {
        let mut p = Poll { answers: vec![PollAnswer::default()], expiry_ms: Some(10 * 86_400_000), ..Poll::default() };
        let (text, next) = remaining(Lang::Fr, &p, 0);
        assert_eq!((text.as_str(), next), ("10 jours restants", Some(1)));
        let (text, next) = remaining(Lang::En, &p, 10 * 86_400_000 - 90_000);
        assert_eq!((text.as_str(), next), ("2 minutes left", Some(30_001)));
        assert_eq!(remaining(Lang::En, &p, 10 * 86_400_000).0, "Poll closed");
        p.finalized = true;
        assert_eq!(remaining(Lang::Fr, &p, 0), ("Sondage terminé".to_string(), None));
    }
}
