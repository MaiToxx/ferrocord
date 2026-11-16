//! Bas de la colonne des salons : panneau de connexion vocale (état, ping,
//! salon et serveur, suppression du bruit, « Se déconnecter », puis la rangée
//! caméra, partage, tableau de sons), et boutons muet et sourdine du panneau
//! utilisateur. Le panneau est le haut de la carte du panneau utilisateur :
//! comme elle, il passe sous le rail.

use super::icons::{self, VIcon};
use super::App;
use crate::i18n::table::lotv::t as lt;
use crate::i18n::table::video::t as xt;
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::state::voice::VoiceConnState;
use crate::theme::Tokens;
use crate::theme::icons::Icon;
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

/// Bouton-icône du jeu plein ; `ink` : couleur forcée (état actif).
fn tool(ui: &mut egui::Ui, t: &Tokens, icon: Icon, tip: &str, ink: Option<egui::Color32>) -> egui::Response {
    let pad = ui.spacing().button_padding.y;
    let side = Tokens::icon_button_side(t.typography.button, pad);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    crate::theme::icons::paint(icon, ui.painter(), rect.shrink(pad), ink.unwrap_or(if hot { t.colors.text_strong } else { t.colors.muted }));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    crate::ui::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Bouton d'enregistrement : un disque, rouge plein pendant l'enregistrement.
fn record_tool(ui: &mut egui::Ui, t: &Tokens, tip: &str, recording: bool) -> egui::Response {
    let pad = ui.spacing().button_padding.y;
    let side = Tokens::icon_button_side(t.typography.button, pad);
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let r = (rect.shrink(pad).width() / 2.0).max(2.0);
    if recording {
        ui.painter().circle_filled(rect.center(), r * 0.8, t.colors.danger);
    } else {
        let ink = if hot { t.colors.text_strong } else { t.colors.muted };
        ui.painter().circle_stroke(rect.center(), r * 0.8, egui::Stroke::new(1.5, ink));
        ui.painter().circle_filled(rect.center(), r * 0.4, t.colors.danger);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    crate::ui::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Bouton large de la rangée du bas ; `on` : fonction active, en aplat d'accent.
fn wide(ui: &mut egui::Ui, t: &Tokens, width: f32, icon: Icon, tip: &str, on: bool) -> egui::Response {
    // Hauteur d'un bouton, pas celle des contrôles : à 24 de haut (Minuit), l'icône était minuscule.
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, crate::ui::widgets::button_height(t)), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let (fill, ink) = match (on, hot) {
        (true, _) => (t.colors.accent, t.colors.on_accent),
        (false, true) => (t.colors.control_bg, t.colors.text_strong),
        (false, false) => (t.colors.button_bg, t.colors.button_text),
    };
    ui.painter().rect_filled(rect, t.r_control(), fill);
    let side = Tokens::icon_side(t.typography.icon).min(rect.height() - 2.0 * t.spacing.item_y);
    crate::theme::icons::paint(icon, ui.painter(), egui::Rect::from_center_size(rect.center(), Vec2::splat(side)), ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, tip));
    crate::ui::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Ce que la rangée du bas propose, selon le système et le salon.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Tools {
    pub camera: bool,
    pub share: bool,
    pub sounds: bool,
}

impl Tools {
    pub(in crate::ui) fn count(self) -> usize {
        usize::from(self.camera) + usize::from(self.share) + usize::from(self.sounds)
    }
}

/// Largeur de chacun des `n` boutons d'une rangée large de `width`, séparés de `gap`.
pub(in crate::ui) fn tool_width(width: f32, n: usize, gap: f32) -> f32 {
    if n == 0 {
        return 0.0;
    }
    ((width - gap * (n as f32 - 1.0)) / n as f32).floor().max(1.0)
}

/// Bouton-icône du vocal ; `on` : état actif (micro coupé, sourdine), en rouge.
fn button(ui: &mut egui::Ui, t: &Tokens, icon: VIcon, tip: &str, on: bool) -> egui::Response {
    let pad = ui.spacing().button_padding.y;
    let side = Tokens::icon_button_side(t.typography.button, pad);
    // Disposition mobile : la cible a la taille du doigt, l'icône garde la sienne.
    let hit = if crate::ui::mobile::is_active(ui.ctx()) { side.max(crate::ui::mobile::TOUCH) } else { side };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(hit), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let color = if on { t.colors.danger } else if hot { t.colors.text_strong } else { t.colors.muted };
    icons::paint(icon, ui.painter(), if hit > side { egui::Rect::from_center_size(rect.center(), Vec2::splat(side)) } else { rect }.shrink(pad), color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip)
}

impl App {
    /// Muet et sourdine, dans le panneau utilisateur (de droite à gauche).
    pub(in crate::ui) fn voice_buttons(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let v = &self.store.voice;
        let (mute, deaf) = (v.self_mute, v.self_deaf);
        let deaf_tip = crate::ui::keys::with_chord(lang, &self.nav.bindings, crate::state::keys::Action::ToggleDeafen, if deaf { vt::UNDEAFEN.get(lang) } else { vt::DEAFEN.get(lang) });
        if button(ui, &t, if deaf { VIcon::HeadphonesOff } else { VIcon::Headphones }, &deaf_tip, deaf).clicked() {
            self.voice_toggle_deaf();
        }
        let mute_tip = crate::ui::keys::with_chord(lang, &self.nav.bindings, crate::state::keys::Action::ToggleMute, if mute { vt::UNMUTE.get(lang) } else { vt::MUTE.get(lang) });
        if button(ui, &t, if mute { VIcon::MicOff } else { VIcon::Mic }, &mute_tip, mute).clicked() {
            self.voice_toggle_mute();
        }
    }

    /// Ce que la rangée du bas du panneau propose.
    pub(in crate::ui) fn voice_tools(&self) -> Tools {
        let sharing = self.store.video.share.is_some();
        Tools { camera: crate::video::capture::AVAILABLE, share: sharing || self.share_picker_ready(), sounds: true }
    }

    /// Panneau de connexion, au-dessus du panneau utilisateur, quand je suis
    /// dans un salon vocal ; confirmation d'une modération. Rend le rectangle
    /// de sa carte, qui passe sous le rail (`reach` : largeur à gagner à gauche).
    pub(in crate::ui) fn voice_panel(&mut self, ui: &mut egui::Ui, reach: f32) -> Option<egui::Rect> {
        self.voice_confirm_ui(ui);
        self.voice_codes_window(ui);
        let my = self.store.voice.me?;
        let t = self.theme.clone();
        let lang = self.lang;
        let v = &self.store.voice;
        let (status, color, ping) = match &v.conn {
            VoiceConnState::Connected { ping_ms } => (vt::CONNECTED.get(lang).to_string(), t.colors.online, ping_ms.map(|p| vf::ping(lang, p))),
            VoiceConnState::Failed(why) => (vf::failed(lang, why), t.colors.danger, None),
            VoiceConnState::Idle | VoiceConnState::Joining | VoiceConnState::Connecting { .. } => (vt::JOINING.get(lang).to_string(), t.colors.idle, None),
        };
        let channel = v.channel(my.channel_id).map(|c| c.name.clone()).unwrap_or_default();
        let guild = my.guild_id.and_then(|g| self.store.guilds.iter().find(|x| x.id == g)).map(|g| crate::ui::words::guild_name(lang, &g.name).to_string()).unwrap_or_default();
        // Appel d'un MP : le nom de la conversation (ni salon ni serveur).
        let place = match my.guild_id {
            Some(_) => vf::place(lang, channel, guild),
            None => self.store.dms.iter().find(|d| d.id == my.channel_id).map(|d| crate::ui::words::dm_name(lang, &d.name).to_string()).unwrap_or_default(),
        };
        let mute_forced = !self.store.me.as_ref().is_some_and(|m| v.can_speak(m.id));
        let audio_problem = v.audio_problem.clone();
        let noise = self.local.prefs.audio.noise_suppression;
        let (camera, sharing) = (self.store.video.camera, self.store.video.share.is_some());
        let tools = self.voice_tools();
        // Plugin « Enregistrement local d'un appel » (lot E4).
        let (rec_shown, recording) = (self.voice_rec_shown(), self.voice_recording());
        #[derive(Clone, Copy)]
        enum Act {
            Leave,
            Noise,
            Record,
            Camera,
            Share,
            Sounds,
        }
        let mut act = None;
        let gap = t.spacing.item_x;
        let frame = egui::Frame::new().fill(t.colors.bg1).inner_margin(egui::Margin { left: gap as i8, right: gap as i8, top: gap as i8, bottom: 0 });
        let panel = egui::Panel::bottom("vocal")
            .frame(frame)
            // Haut de la carte du panneau utilisateur : pas de filet d'egui,
            // le contour de la carte suffit.
            .show_separator_line(false)
            .show(ui, |ui| {
                crate::ui::look::backdrop(ui, crate::ui::look::Zone::Side);
                let pad = t.user_bar_margin();
                // Le panneau des salons découpe à son bord : élargi jusque sous le rail.
                let clip = ui.clip_rect();
                let clip = egui::Rect::from_min_max(egui::pos2(clip.left() - reach, clip.top()), clip.max);
                // Découpe élargie posée telle quelle (`with_clip_rect` la croiserait avec celle du panneau).
                let mut painter = ui.painter().clone();
                painter.set_clip_rect(clip);
                // Fond posé avant le contenu, dimensionné après lui.
                let bg = painter.add(egui::Shape::Noop);
                let avail = ui.available_rect_before_wrap();
                let outer = egui::Rect::from_min_max(egui::pos2(avail.left() - reach, avail.top()), egui::pos2(avail.right(), avail.top()));
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_max(outer.min + pad.left_top(), egui::pos2(outer.right() - f32::from(pad.right), avail.bottom()))).layout(Layout::top_down(Align::Min)));
                inner.set_clip_rect(clip);
                {
                    let ui = &mut inner;
                    // Enregistrement en cours : bandeau rouge, toujours visible.
                    self.voice_rec_banner(ui);
                    ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = t.spacing.item_y / 2.0;
                        if button(ui, &t, VIcon::Hangup, vt::LEAVE.get(lang), false).clicked() {
                            act = Some(Act::Leave);
                        }
                        let tip = if noise { lt::NOISE_ON } else { lt::NOISE_OFF }.get(lang);
                        if tool(ui, &t, Icon::Signal, tip, noise.then_some(t.colors.online)).clicked() {
                            act = Some(Act::Noise);
                        }
                        if rec_shown {
                            let tip = if recording { crate::i18n::table::e4::t::REC_STOP } else { crate::i18n::table::e4::t::REC_BUTTON }.get(lang);
                            if record_tool(ui, &t, tip, recording).clicked() {
                                act = Some(Act::Record);
                            }
                        }
                        ui.with_layout(Layout::top_down(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
                            ui.horizontal(|ui| {
                                ui.add(egui::Label::new(RichText::new(status).family(crate::ui::fonts::demi()).strong().color(color)).truncate().selectable(false));
                                if let Some(p) = ping {
                                    ui.label(RichText::new(p).small().color(t.colors.muted));
                                }
                            });
                            ui.add(egui::Label::new(RichText::new(place).small().color(t.colors.muted)).truncate().selectable(false));
                            if mute_forced {
                                ui.add(egui::Label::new(RichText::new(vt::CANT_SPEAK.get(lang)).small().color(t.colors.danger)).truncate().selectable(false));
                            }
                            if let Some(p) = &audio_problem {
                                ui.add(egui::Label::new(RichText::new(p).small().color(t.colors.danger)).truncate().selectable(false));
                            }
                            self.voice_codes_link(ui);
                            // Caméra ou partage actifs (phase 5).
                            self.video_indicator(ui);
                        });
                    });
                    });
                    // Rangée du bas : caméra, partage, tableau de sons, à largeurs égales.
                    let n = tools.count();
                    if n > 0 {
                        ui.add_space(t.spacing.item_y);
                        let w = tool_width(ui.available_width(), n, gap);
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = gap;
                            if tools.camera && wide(ui, &t, w, if camera { Icon::Camera } else { Icon::CameraOff }, if camera { xt::CAMERA_OFF } else { xt::CAMERA_ON }.get(lang), camera).clicked() {
                                act = Some(Act::Camera);
                            }
                            if tools.share && wide(ui, &t, w, if sharing { Icon::ScreenOff } else { Icon::Screen }, if sharing { xt::STOP_SHARE } else { xt::SHARE }.get(lang), sharing).clicked() {
                                act = Some(Act::Share);
                            }
                            if tools.sounds && wide(ui, &t, w, Icon::Audio, crate::i18n::table::calls::t::SOUNDBOARD.get(lang), false).clicked() {
                                act = Some(Act::Sounds);
                            }
                        });
                    }
                }
                let bottom = inner.min_rect().bottom() + f32::from(pad.bottom);
                let card = egui::Rect::from_min_max(outer.min, egui::pos2(outer.right(), bottom));
                ui.allocate_rect(egui::Rect::from_min_max(avail.min, egui::pos2(avail.right(), bottom)), Sense::hover());
                // Coins du haut seulement : la carte du panneau utilisateur la prolonge.
                let r = t.r_card();
                let round = egui::CornerRadius { nw: r.nw, ne: r.ne, sw: 0, se: 0 };
                painter.set(bg, egui::epaint::RectShape::new(card, round, t.panel_bg(), t.panel_stroke(), egui::StrokeKind::Inside));
                card
            });
        match act {
            Some(Act::Leave) => self.store.voice_leave(),
            Some(Act::Record) => self.voice_rec_request(),
            Some(Act::Noise) => {
                let mut p = self.local.prefs.clone();
                p.audio.noise_suppression = !noise;
                self.set_local_prefs(p);
            }
            Some(Act::Camera) => {
                self.toggle_camera();
            }
            Some(Act::Share) if sharing => self.store.video_stop_share(),
            Some(Act::Share) => self.request_share_picker(),
            Some(Act::Sounds) => {
                if self.calls_ui.soundboard == Some(my.channel_id) {
                    self.calls_ui.soundboard = None;
                } else {
                    self.open_soundboard(my.channel_id);
                }
            }
            None => {}
        }
        Some(panel.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_rangee_partage_la_largeur() {
        assert_eq!(tool_width(200.0, 3, 8.0), 61.0);
        assert_eq!(tool_width(200.0, 1, 8.0), 200.0);
        assert_eq!(tool_width(200.0, 0, 8.0), 0.0);
        assert_eq!(Tools { camera: true, share: false, sounds: true }.count(), 2);
    }
}
