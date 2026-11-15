//! Appels privés et de groupe (lot V6) : bouton « Démarrer un appel » dans
//! l'en-tête d'un MP, bandeau de l'appel en haut du MP (durée, participants,
//! sonnerie, rejoindre, raccrocher), fenêtre d'appel entrant (accepter,
//! refuser) avec sonnerie et notification de bureau, messages d'échec du lot.
//!
//! La connexion vocale n'est pas ici : « Rejoindre », « Accepter » et
//! « Raccrocher » déposent une demande dans `state::calls` (câblée par V7).
//! Les icônes du lot sont dessinées ici pour ne pas toucher au thème.
//!
//! Réveils : la sonnerie (toutes les quatre secondes, une minute au plus)
//! et la durée de l'appel affiché (chaque seconde), seulement pendant un appel.

use super::util::{paint_avatar, pixel_square};
use super::App;
use crate::i18n::table::calls::{f, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::calls::CallAction;
use crate::state::model::Avatar;
use crate::state::notify::{Alert, Kind};
use crate::state::stage::StageAction;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, pos2, Color32, Id, Painter, Rect, RichText, Sense, Shape, Stroke, Vec2};
use std::time::{Duration, Instant};

/// Écart entre deux sonneries, et durée maximale de la sonnerie locale.
const RING_EVERY: Duration = Duration::from_secs(4);
const RING_MAX: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(super) struct CallsUi {
    /// Appels qui sonnaient à l'image précédente.
    ringing: Vec<u64>,
    ring_started: Option<Instant>,
    last_ring: Option<Instant>,
    /// Tableau de sons ouvert pour ce salon (`soundboard.rs`).
    pub(super) soundboard: Option<u64>,
    /// Statut du salon vocal en cours de saisie : (salon, texte).
    pub(super) status_edit: Option<(u64, String)>,
    /// Sujet de conférence en cours de saisie : (salon, texte, prévenir @everyone).
    pub(super) topic_edit: Option<(u64, String, bool)>,
}

/// Icônes du lot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    Phone,
    Speaker,
    Stage,
    Hand,
}

/// Dessine `g` dans `rect` (carré conseillé).
pub(super) fn paint_glyph(g: Glyph, painter: &Painter, rect: Rect, color: Color32) {
    // Jeu plein du thème ; la main levée n'y est pas.
    let full = match g {
        Glyph::Phone => Some(Icon::Phone),
        Glyph::Speaker => Some(Icon::Speaker),
        Glyph::Stage => Some(Icon::Stage),
        Glyph::Hand => None,
    };
    if let Some(icon) = full {
        return crate::theme::icons::paint(icon, painter, rect, color);
    }
    let side = rect.width().min(rect.height());
    let c = rect.center();
    let u = side * 0.5;
    let p = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    painter.add(Shape::convex_polygon(vec![p(-0.55, -0.05), p(0.55, -0.05), p(0.5, 0.55), p(0.2, 0.85), p(-0.3, 0.85), p(-0.55, 0.45)], color, Stroke::NONE));
    for (i, x) in [-0.45_f32, -0.15, 0.15, 0.45].into_iter().enumerate() {
        let top = if i == 1 || i == 2 { -0.85 } else { -0.65 };
        painter.add(Shape::convex_polygon(vec![p(x - 0.11, top), p(x + 0.11, top), p(x + 0.11, 0.0), p(x - 0.11, 0.0)], color, Stroke::NONE));
    }
}

/// Bouton plein (accent ou danger).
pub(super) fn filled_button(ui: &mut egui::Ui, t: &Tokens, text: &str, fill: Color32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).color(t.colors.on_accent)).fill(fill))
}

/// Durée d'un appel, « 3:07 » ou « 1:02:09 ».
pub(super) fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    match s / 3600 {
        0 => format!("{}:{:02}", s / 60, s % 60),
        h => format!("{h}:{:02}:{:02}", s / 60 % 60, s % 60),
    }
}

fn call_action(a: CallAction) -> Text {
    match a {
        CallAction::Ring => tx::A_RING,
        CallAction::StopRinging => tx::A_STOP_RINGING,
        CallAction::DefaultSounds | CallAction::GuildSounds => tx::A_SOUNDS,
        CallAction::SendSound => tx::A_PLAY,
    }
}

fn stage_action(a: StageAction) -> Text {
    match a {
        StageAction::Start => tx::A_START,
        StageAction::Topic => tx::A_TOPIC,
        StageAction::End => tx::A_END,
        StageAction::Hand => tx::A_HAND,
        StageAction::Speaker => tx::A_SPEAKER,
        StageAction::Status => tx::A_STATUS,
    }
}

fn failure(lang: Lang, action: Text, problem: Option<&crate::state::Problem>, h24: bool) -> String {
    match problem {
        None => tx::CAPTCHA.get(lang).to_string(),
        Some(crate::state::Problem::MfaRequired) => crate::i18n::table::admin::t::MFA_REQUIRED.get(lang).to_string(),
        Some(p) => f::failed(lang, action.get(lang), super::words::problem(lang, p, h24)),
    }
}

impl App {
    /// Nom affiché et avatar d'une personne (moi compris).
    pub(super) fn voice_person(&self, id: u64) -> (String, Avatar) {
        if let Some(me) = self.store.me.as_ref().filter(|u| u.id == id) {
            return (me.name.clone(), me.avatar);
        }
        match self.store.people.lock().user(id) {
            Some(u) => (u.display_name().to_string(), u.avatar),
            None => (crate::i18n::t::UNKNOWN_USER.get(self.lang).to_string(), Avatar::of_user(id, None, None)),
        }
    }

    /// Une personne : avatar et nom, puis une précision discrète.
    pub(super) fn person_row(&self, ui: &mut egui::Ui, id: u64, note: Option<&str>) -> egui::Response {
        let t = &self.theme;
        let (name, avatar) = self.voice_person(id);
        ui.horizontal(|ui| {
            // La ligne a d'emblée la hauteur de l'avatar : le nom se centre sur lui.
            ui.set_min_height(t.layout.small_avatar.max(ui.spacing().interact_size.y));
            if t.avatars_shown() {
                let side = t.layout.small_avatar;
                let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                paint_avatar(ui, &self.images, t, pixel_square(ui, slot.min, side), &avatar, &name);
            }
            ui.add(egui::Label::new(RichText::new(&name).color(t.colors.text)).truncate());
            if let Some(n) = note {
                ui.label(RichText::new(n).small().color(t.colors.faint));
            }
        })
        .response
    }

    /// À chaque image : sonnerie des appels entrants, messages d'échec du lot.
    pub(super) fn calls_tick(&mut self) {
        let now = Instant::now();
        let rings = self.store.incoming_calls();
        let fresh = rings.iter().copied().find(|c| !self.calls_ui.ringing.contains(c));
        if rings.is_empty() {
            self.calls_ui.ring_started = None;
        } else if let Some(c) = fresh {
            self.calls_ui.ring_started = Some(now);
            self.calls_ui.last_ring = Some(now);
            let alert = self.ring_alert(c);
            self.desk_ring(Some(&alert));
        } else if let (Some(start), Some(last)) = (self.calls_ui.ring_started, self.calls_ui.last_ring)
            && now.duration_since(start) < RING_MAX
            && now.duration_since(last) >= RING_EVERY
        {
            self.calls_ui.last_ring = Some(now);
            self.desk_ring(None);
        }
        if let (Some(start), Some(last)) = (self.calls_ui.ring_started, self.calls_ui.last_ring)
            && now.duration_since(start) < RING_MAX
        {
            self.ctx.request_repaint_after(RING_EVERY.saturating_sub(now.duration_since(last)));
        }
        self.calls_ui.ringing = rings;
        let h24 = self.h24();
        while let Some(n) = self.store.calls.take_notice() {
            let text = failure(self.lang, call_action(n.action), n.problem.as_ref(), h24);
            self.notify(text);
        }
        while let Some(n) = self.store.stage.take_notice() {
            let text = failure(self.lang, stage_action(n.action), n.problem.as_ref(), h24);
            self.notify(text);
        }
    }

    /// Qui appelle : la première personne déjà dans l'appel, sinon le nom du MP.
    fn caller(&self, channel: u64) -> (String, Avatar) {
        let me = self.store.me.as_ref().map(|u| u.id);
        match self.store.voice.members(channel).map(|s| s.user_id).find(|u| Some(*u) != me) {
            Some(u) => self.voice_person(u),
            None => {
                let dm = self.store.dms.iter().find(|d| d.id == channel);
                (dm.map(|d| super::words::dm_name(self.lang, &d.name).to_string()).unwrap_or_default(), dm.map_or(Avatar::of_user(channel, None, None), |d| d.avatar))
            }
        }
    }

    fn ring_alert(&self, channel: u64) -> Alert {
        let (title, avatar) = self.caller(channel);
        let group = self.is_group(channel);
        let place = group.then(|| self.store.dms.iter().find(|d| d.id == channel).map(|d| super::words::dm_name(self.lang, &d.name).to_string())).flatten();
        Alert { guild: None, channel, kind: Kind::Ring, title, place, body: tx::INCOMING.get(self.lang).to_string(), avatar: avatar.image, header: crate::state::notify::header(&self.store, None, self.lang) }
    }

    /// Fenêtres d'appel entrant, en haut au centre, par-dessus le reste.
    pub(super) fn incoming_calls_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let mut act = None;
        let step = t.layout.avatar_size + 3.0 * t.spacing.card_pad;
        let area = self.chat_rect.unwrap_or_else(|| ui.ctx().content_rect());
        for (i, c) in self.calls_ui.ringing.clone().into_iter().enumerate() {
            let (name, avatar) = self.caller(c);
            let at = incoming_slot(area, i, step);
            egui::Area::new(Id::unique(("appel_entrant", c))).pivot(egui::Align2::CENTER_CENTER).fixed_pos(at).order(egui::Order::Foreground).show(ui.ctx(), |ui| {
                egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(t.r_card()).inner_margin(t.toast_margin()).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        if t.avatars_shown() {
                            let side = t.layout.avatar_size;
                            let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                            paint_avatar(ui, &self.images, &t, pixel_square(ui, slot.min, side), &avatar, &name);
                        }
                        ui.vertical(|ui| {
                            ui.label(RichText::new(tx::INCOMING.get(lang)).small().color(t.colors.muted));
                            ui.label(RichText::new(f::calling_you(lang, &name)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                        });
                        ui.add_space(t.spacing.card_pad);
                        if filled_button(ui, &t, tx::ACCEPT.get(lang), t.colors.online).clicked() {
                            act = Some((c, true));
                        }
                        if filled_button(ui, &t, tx::DECLINE.get(lang), t.danger_fill()).clicked() {
                            act = Some((c, false));
                        }
                    });
                });
            });
        }
        match act {
            Some((c, true)) => {
                self.store.accept_call(c);
                self.store.select_dms();
                self.open_channel(c);
            }
            Some((c, false)) => {
                self.store.decline_call(c);
            }
            None => {}
        }
        if act.is_some() {
            self.flush_commands();
        }
    }

    /// En-tête d'un MP : « Démarrer un appel » tant qu'aucun appel n'y est en cours.
    pub(super) fn call_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        if self.store.calls.call(cid).is_some() || self.store.calls.calling(cid) {
            return;
        }
        let t = self.theme.clone();
        if super::chat_head::head_icon(ui, &t, tx::START_CALL.get(self.lang), false, |p, r, c| paint_glyph(Glyph::Phone, p, r, c)).clicked() {
            self.store.start_call(cid);
            self.flush_commands();
        }
    }

    /// Bandeau de l'appel en haut d'un MP.
    pub(super) fn call_banner(&mut self, ui: &mut egui::Ui, cid: u64) {
        let calling = self.store.calls.calling(cid);
        let Some(call) = self.store.calls.call(cid).cloned().or_else(|| calling.then(Default::default)) else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let me = self.store.me.as_ref().map(|u| u.id);
        let in_call = self.store.in_voice(cid);
        let people: Vec<(u64, bool)> = self.store.voice.members(cid).map(|s| (s.user_id, s.muted())).collect();
        let mut act = None;
        egui::Panel::top("call_banner").frame(egui::Frame::new().fill(t.colors.bg1).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(Tokens::icon_side(t.typography.body)), Sense::hover());
                paint_glyph(Glyph::Phone, ui.painter(), rect, t.colors.online);
                let title = if calling { tx::CALLING } else { tx::IN_CALL };
                ui.label(RichText::new(title.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                if let Some(start) = call.started_ms() {
                    let now = self.store.now();
                    ui.label(RichText::new(clock(now - start)).color(t.colors.muted));
                    // La seconde suivante, seulement tant que le bandeau est affiché.
                    let wait = 1000 - (now - start).rem_euclid(1000);
                    ui.ctx().request_repaint_after(Duration::from_millis(u64::try_from(wait).unwrap_or(1000)));
                }
                if call.unavailable {
                    ui.label(RichText::new(tx::UNAVAILABLE.get(lang)).color(t.colors.danger));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if in_call {
                        if filled_button(ui, &t, tx::LEAVE.get(lang), t.danger_fill()).clicked() {
                            act = Some(BannerAct::Leave);
                        }
                    } else if !calling && filled_button(ui, &t, tx::JOIN.get(lang), t.colors.online).clicked() {
                        act = Some(BannerAct::Join);
                    }
                    let others_ringing = call.ringing.iter().any(|u| Some(*u) != me);
                    if in_call && others_ringing && !self.store.calls.is_busy(CallAction::StopRinging, cid) && ui.button(tx::STOP_RINGING.get(lang)).clicked() {
                        act = Some(BannerAct::StopRinging);
                    }
                    if in_call && !others_ringing && !calling && !self.store.calls.is_busy(CallAction::Ring, cid) && ui.button(tx::RING.get(lang)).clicked() {
                        act = Some(BannerAct::Ring);
                    }
                });
            });
            ui.horizontal_wrapped(|ui| {
                for (u, muted) in &people {
                    self.person_row(ui, *u, muted.then(|| tx::MUTED.get(lang)));
                    ui.add_space(t.spacing.item_x);
                }
                for u in call.ringing.iter().filter(|u| !people.iter().any(|(p, _)| p == *u)) {
                    self.person_row(ui, *u, Some(tx::RINGING.get(lang)));
                    ui.add_space(t.spacing.item_x);
                }
            });
        });
        match act {
            Some(BannerAct::Join) => self.store.voice_join_call(cid),
            Some(BannerAct::Leave) => self.store.voice_leave(),
            Some(BannerAct::StopRinging) => {
                self.store.stop_ringing(cid);
            }
            Some(BannerAct::Ring) => {
                self.store.ring(cid, None);
            }
            None => {}
        }
        if act.is_some() {
            self.flush_commands();
        }
    }

    /// Fenêtres du lot, par-dessus le reste (appel entrant, tableau de sons).
    pub(super) fn calls_overlay(&mut self, ui: &mut egui::Ui) {
        self.calls_tick();
        self.incoming_calls_ui(ui);
        self.soundboard_ui(ui);
    }

    /// Appel entrant affiché (aperçu, tests).
    pub fn ringing_calls(&self) -> &[u64] {
        &self.calls_ui.ringing
    }
}

/// Centre de la `i`-ième fenêtre d'appel entrant : au milieu de la zone de
/// discussion, pas en haut, où elle cachait l'en-tête du salon et le bandeau
/// de l'appel (« Rejoindre », « Raccrocher »). Les suivantes s'empilent
/// dessous, sans sortir de la zone.
fn incoming_slot(area: Rect, i: usize, step: f32) -> egui::Pos2 {
    let y = area.center().y + i as f32 * step;
    pos2(area.center().x, y.min(area.bottom() - step / 2.0).max(area.top() + step / 2.0))
}

#[derive(Clone, Copy)]
enum BannerAct {
    Join,
    Leave,
    StopRinging,
    Ring,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_clock() {
        assert_eq!(clock(0), "0:00");
        assert_eq!(clock(187_000), "3:07");
        assert_eq!(clock(3_729_000), "1:02:09");
        assert_eq!(clock(-5), "0:00");
    }

    #[test]
    fn incoming_call_leaves_the_header_and_banner_visible() {
        let chat = Rect::from_min_max(pos2(300.0, 0.0), pos2(1100.0, 800.0));
        let step = 70.0;
        let first = incoming_slot(chat, 0, step);
        assert_eq!(first, pos2(700.0, 400.0));
        // Bien en dessous de l'en-tête et du bandeau (moins de 2 hauteurs de carte).
        assert!(first.y - step / 2.0 > chat.top() + 2.0 * step);
        assert!(incoming_slot(chat, 1, step).y > first.y);
        assert!(incoming_slot(chat, 50, step).y <= chat.bottom() - step / 2.0);
    }
}
