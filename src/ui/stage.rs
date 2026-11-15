//! Salons vocaux et de conférence (lot V6) : l'en-tête affiché au-dessus de
//! leur discussion textuelle (même identifiant de salon, routes des messages
//! existantes ; lignes de la liste : `ui::voice`, lot V4) : statut du salon,
//! conférence en cours (sujet, orateurs,
//! mains levées, auditeurs), gestes des auditeurs et des modérateurs,
//! rejoindre, tableau de sons, sons joués par les autres.

use super::call::{filled_button, paint_glyph, Glyph};
use super::App;
use crate::i18n::table::calls::{f, p, t as tx};
use crate::state::admin::expressions::SoundEmoji;
use crate::state::calls::EffectEmoji;
use crate::state::stage::{Invalid, Role, StageAction, STATUS_MAX, TOPIC_MAX};
use crate::theme::Tokens;
use eframe::egui::{self, FontId, RichText, Sense, Vec2};


/// Geste demandé dans l'en-tête (appliqué après le dessin).
enum RoomAct {
    Join,
    Leave,
    Hand(bool),
    Speaker(Option<u64>, bool),
    EditTopic,
    SaveTopic,
    CancelTopic,
    End,
    EditStatus,
    SaveStatus,
    CancelStatus,
    Soundboard,
}

/// Sous cette largeur, l'en-tête d'un salon vocal ne garde que l'icône et les boutons.
const NARROW_HEAD: f32 = 320.0;
/// Place de plus qu'il faut au nombre de personnes à côté des boutons.
const COUNT_ROOM: f32 = 120.0;

fn invalid_text(e: Invalid) -> Option<crate::i18n::Text> {
    match e {
        Invalid::Topic => Some(tx::TOPIC_INVALID),
        Invalid::Status => Some(tx::STATUS_INVALID),
        // Bouton absent ou action en cours : rien à dire.
        Invalid::Forbidden | Invalid::Busy => None,
    }
}

impl App {
    /// Nom d'un salon vocal (en-tête de sa discussion).
    pub(super) fn voice_room_name(&self, cid: u64) -> Option<String> {
        self.store.voice_room(cid).map(|r| r.venue.name)
    }

    /// En-tête d'un salon vocal ou de conférence, au-dessus de sa discussion.
    pub(super) fn room_head(&mut self, ui: &mut egui::Ui, cid: u64) {
        let Some(room) = self.store.guild.and_then(|g| self.store.voice_room_in(g, cid)) else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let g = room.guild_id;
        let joined = self.store.in_voice(cid);
        let moderator = self.store.stage_moderator(g);
        let instance = self.store.stage.instance(cid).cloned();
        let mut act = None;
        egui::Panel::top("room_head").frame(egui::Frame::new().fill(t.colors.bg1).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            super::settings::shell::page_style(ui, &t);
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                let (rect, _) = ui.allocate_exact_size(Vec2::splat(Tokens::icon_side(t.typography.body)), Sense::hover());
                let live = instance.is_some();
                paint_glyph(if room.venue.stage { Glyph::Stage } else { Glyph::Speaker }, ui.painter(), rect, if live { t.colors.online } else { t.colors.faint });
                let kind = if room.venue.stage { tx::STAGE } else { tx::VOICE_ROOM };
                // Disposition mobile ou colonne étroite : nature et nombre de personnes cèdent la
                // place aux boutons (l'icône dit la nature, la liste des personnes est juste en dessous).
                if !super::mobile::is_active(ui.ctx()) && ui.available_width() >= NARROW_HEAD {
                    ui.label(RichText::new(kind.get(lang)).color(t.colors.muted));
                    // Le nombre cède la place avant la nature : collé aux boutons, il ne se lisait plus.
                    if ui.available_width() >= NARROW_HEAD + COUNT_ROOM {
                        ui.label(RichText::new(p::in_room(lang, self.store.voice.count(cid) as u64)).small().color(t.colors.faint));
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if joined {
                        if filled_button(ui, &t, tx::LEAVE.get(lang), t.danger_fill()).clicked() {
                            act = Some(RoomAct::Leave);
                        }
                        if ui.button(tx::SOUNDBOARD.get(lang)).clicked() {
                            act = Some(RoomAct::Soundboard);
                        }
                    } else if filled_button(ui, &t, tx::JOIN.get(lang), t.colors.online).clicked() {
                        act = Some(RoomAct::Join);
                    }
                });
            });
            self.status_line(ui, &t, g, cid, &mut act);
            if room.venue.stage {
                self.stage_block(ui, &t, cid, instance.as_ref(), moderator, joined, &mut act);
            }
            // Sons joués par les autres, quelques secondes.
            let now = self.store.now();
            let lines: Vec<String> = self
                .store
                .calls
                .recent_effects(cid, now)
                .filter_map(|e| {
                    let name = self.voice_person(e.effect.user_id).0;
                    let sound = e.effect.sound_id.map(|id| self.sound_label(g, id)).or_else(|| match &e.effect.emoji {
                        Some(EffectEmoji::Unicode(s)) => Some(s.clone()),
                        Some(EffectEmoji::Custom { name, .. }) => Some(format!(":{name}:")),
                        None => None,
                    })?;
                    Some(f::played(lang, name, sound))
                })
                .collect();
            let font = FontId::proportional(t.typography.small);
            for l in lines {
                super::emoji::label(ui, &l, &font, t.colors.accent, true);
            }
            if let Some(until) = self.store.calls.effects_until(now) {
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(u64::try_from(until - now).unwrap_or(0)));
            }
        });
        self.apply_room_act(act, cid);
    }

    /// Nom d'un son (avec son emoji) : sons du serveur, puis sons par défaut.
    pub(super) fn sound_label(&self, guild: u64, id: u64) -> String {
        let defaults = match self.store.calls.default_sounds() {
            crate::state::calls::Loadable::Ready(l) => l.as_slice(),
            _ => &[],
        };
        let found = self.store.expressions.sounds(guild).unwrap_or(&[]).iter().chain(defaults).find(|s| s.id == id);
        match found {
            Some(s) => match &s.emoji {
                Some(SoundEmoji::Unicode(e)) => format!("{e} {}", s.name),
                _ => s.name.clone(),
            },
            None => format!("#{id}"),
        }
    }

    fn status_line(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, cid: u64, act: &mut Option<RoomAct>) {
        let lang = self.lang;
        let can = self.store.can_set_voice_status(guild, cid);
        let busy = self.store.stage.is_busy(StageAction::Status, cid, 0);
        match &mut self.calls_ui.status_edit {
            Some((c, text)) if *c == cid => {
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(text).hint_text(tx::STATUS_HINT.get(lang)).char_limit(STATUS_MAX).desired_width(t.layout.card_width));
                    if ui.add_enabled(!busy, egui::Button::new(tx::SAVE.get(lang))).clicked() {
                        *act = Some(RoomAct::SaveStatus);
                    }
                    if ui.button(tx::CANCEL.get(lang)).clicked() {
                        *act = Some(RoomAct::CancelStatus);
                    }
                });
            }
            _ => {
                let status = self.store.stage.status(cid);
                if status.is_some() || can {
                    ui.horizontal(|ui| {
                        if let Some(s) = status {
                            ui.label(RichText::new(s).color(t.colors.text));
                        }
                        if can && ui.button(tx::SET_STATUS.get(lang)).clicked() {
                            *act = Some(RoomAct::EditStatus);
                        }
                    });
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn stage_block(&mut self, ui: &mut egui::Ui, t: &Tokens, cid: u64, instance: Option<&crate::state::stage::StageInstance>, moderator: bool, joined: bool, act: &mut Option<RoomAct>) {
        let lang = self.lang;
        ui.add_space(t.spacing.item_y);
        match &mut self.calls_ui.topic_edit {
            Some((c, text, notify)) if *c == cid => {
                super::mobile::row(ui, |ui| {
                    ui.add(egui::TextEdit::singleline(text).hint_text(tx::TOPIC_HINT.get(lang)).char_limit(TOPIC_MAX).desired_width(t.layout.card_width));
                    if instance.is_none() {
                        ui.checkbox(notify, tx::NOTIFY_EVERYONE.get(lang));
                    }
                    let label = if instance.is_none() { tx::START_STAGE } else { tx::SAVE };
                    if filled_button(ui, t, label.get(lang), t.colors.accent).clicked() {
                        *act = Some(RoomAct::SaveTopic);
                    }
                    if ui.button(tx::CANCEL.get(lang)).clicked() {
                        *act = Some(RoomAct::CancelTopic);
                    }
                });
            }
            _ => {
                // Le sujet, puis ses boutons, renvoyés à la ligne plutôt que coupés au bord.
                ui.horizontal_wrapped(|ui| match instance {
                    Some(i) => {
                        ui.label(RichText::new(tx::LIVE.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.online));
                        ui.add(egui::Label::new(RichText::new(&i.topic).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong)).truncate());
                        if moderator {
                            if ui.button(tx::EDIT_TOPIC.get(lang)).clicked() {
                                *act = Some(RoomAct::EditTopic);
                            }
                            if ui.add_enabled(!self.store.stage.is_busy(StageAction::End, cid, 0), egui::Button::new(tx::END_STAGE.get(lang))).clicked() {
                                *act = Some(RoomAct::End);
                            }
                        }
                    }
                    None => {
                        ui.label(RichText::new(tx::NO_STAGE.get(lang)).color(t.colors.muted));
                        if moderator && ui.button(tx::START_STAGE.get(lang)).clicked() {
                            *act = Some(RoomAct::EditTopic);
                        }
                    }
                });
            }
        }
        // Mes gestes.
        let mine = self.store.my_stage_role(cid);
        if joined {
            let hand_busy = self.store.stage.is_busy(StageAction::Hand, cid, 0);
            let speak_busy = self.store.stage.is_busy(StageAction::Speaker, cid, 0);
            ui.horizontal(|ui| {
                let (hand, speak) = match mine {
                    Some(Role::Listener) => (Some(true), moderator.then_some(true)),
                    Some(Role::Hand) => (Some(false), None),
                    Some(Role::Speaker) => (None, Some(false)),
                    None => (None, None),
                };
                if let Some(up) = hand {
                    let label = if up { tx::RAISE_HAND } else { tx::LOWER_HAND };
                    if ui.add_enabled(!hand_busy, egui::Button::new(label.get(lang))).clicked() {
                        *act = Some(RoomAct::Hand(up));
                    }
                }
                if let Some(s) = speak {
                    let label = if s { tx::SPEAK } else { tx::BACK_TO_AUDIENCE };
                    if ui.add_enabled(!speak_busy, egui::Button::new(label.get(lang))).clicked() {
                        *act = Some(RoomAct::Speaker(None, s));
                    }
                }
            });
        }
        // Orateurs, mains levées, auditeurs.
        let people = self.store.stage_people(cid);
        if people.is_empty() {
            ui.label(RichText::new(tx::NOBODY.get(lang)).small().color(t.colors.faint));
            return;
        }
        let me = self.store.me.as_ref().map(|u| u.id);
        for (role, title) in [(Role::Speaker, tx::SPEAKERS), (Role::Hand, tx::HANDS), (Role::Listener, tx::LISTENERS)] {
            let group: Vec<u64> = people.iter().filter(|(_, r)| *r == role).map(|(u, _)| *u).collect();
            if group.is_empty() {
                continue;
            }
            ui.label(RichText::new(format!("{} — {}", title.get(lang), crate::i18n::number(lang, group.len() as u64))).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
            ui.horizontal_wrapped(|ui| {
                for u in group {
                    let row = self.person_row(ui, u, None);
                    if role == Role::Hand {
                        let (rect, _) = ui.allocate_exact_size(Vec2::splat(Tokens::icon_side(t.typography.small)), Sense::hover());
                        paint_glyph(Glyph::Hand, ui.painter(), rect, t.colors.accent);
                    }
                    if moderator && Some(u) != me {
                        let busy = self.store.stage.is_busy(StageAction::Speaker, cid, u);
                        row.context_menu(|ui| {
                            let (label, speak) = if role == Role::Speaker { (tx::TO_AUDIENCE, false) } else { (tx::INVITE_SPEAK, true) };
                            if ui.add_enabled_ui(!busy, |ui| crate::ui::menu::item(ui, t, label.get(lang), Some(crate::theme::icons::Icon::Mic), false)).inner.clicked() {
                                *act = Some(RoomAct::Speaker(Some(u), speak));
                                ui.close();
                            }
                        });
                        // Mains levées : l'invitation est aussi à portée de clic.
                        if role == Role::Hand && ui.add_enabled(!busy, egui::Button::new(tx::INVITE_SPEAK.get(lang))).clicked() {
                            *act = Some(RoomAct::Speaker(Some(u), true));
                        }
                    }
                    ui.add_space(t.spacing.item_x);
                }
            });
        }
    }

    fn apply_room_act(&mut self, act: Option<RoomAct>, cid: u64) {
        let Some(act) = act else { return };
        let result = match act {
            RoomAct::Join => {
                self.voice_join(cid);
                Ok(())
            }
            RoomAct::Leave => {
                self.store.voice_leave();
                Ok(())
            }
            RoomAct::Hand(up) => self.store.raise_hand(cid, up),
            RoomAct::Speaker(user, speak) => self.store.set_speaker(cid, user, speak),
            RoomAct::EditTopic => {
                let topic = self.store.stage.instance(cid).map(|i| i.topic.clone()).unwrap_or_default();
                self.calls_ui.topic_edit = Some((cid, topic, false));
                Ok(())
            }
            RoomAct::SaveTopic => match self.calls_ui.topic_edit.clone() {
                Some((c, text, notify)) if c == cid => {
                    let r = if self.store.stage.instance(cid).is_some() { self.store.set_stage_topic(cid, &text) } else { self.store.start_stage(cid, &text, notify) };
                    if r.is_ok() {
                        self.calls_ui.topic_edit = None;
                    }
                    r
                }
                _ => Ok(()),
            },
            RoomAct::CancelTopic => {
                self.calls_ui.topic_edit = None;
                Ok(())
            }
            RoomAct::End => self.store.end_stage(cid),
            RoomAct::EditStatus => {
                self.calls_ui.status_edit = Some((cid, self.store.stage.status(cid).unwrap_or_default().to_string()));
                Ok(())
            }
            RoomAct::SaveStatus => match self.calls_ui.status_edit.clone() {
                Some((c, text)) if c == cid => {
                    let r = self.store.set_voice_status(cid, &text);
                    if r.is_ok() {
                        self.calls_ui.status_edit = None;
                    }
                    r
                }
                _ => Ok(()),
            },
            RoomAct::CancelStatus => {
                self.calls_ui.status_edit = None;
                Ok(())
            }
            RoomAct::Soundboard => {
                self.open_soundboard(cid);
                Ok(())
            }
        };
        if let Err(e) = result
            && let Some(text) = invalid_text(e)
        {
            self.notify(text.get(self.lang).to_string());
        }
        self.flush_commands();
    }

    /// Ouvre la discussion d'un salon vocal (aperçu, tests).
    pub fn open_voice_room(&mut self, guild: u64, channel: u64) {
        self.store.guild = Some(guild);
        self.open_channel(channel);
    }
}
