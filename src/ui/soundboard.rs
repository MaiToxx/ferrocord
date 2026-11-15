//! Tableau de sons (lot V6) : fenêtre ouverte depuis un salon vocal où l'on
//! est connecté ; sons du serveur puis sons par défaut, un clic joue le son
//! pour tout le salon (`POST /channels/{id}/send-soundboard-sound`). La
//! lecture locale des sons (les siens et ceux des autres) est faite par le
//! moteur audio (V7), à partir de `Store::take_sound_plays`.

use super::App;
use crate::i18n::table::calls::t as tx;
use crate::state::admin::expressions::{Sound, SoundEmoji};
use crate::state::calls::{CallAction, Loadable};
use crate::theme::Tokens;
use super::emoji::Img;
use eframe::egui::{self, FontId, Id, Rect, RichText, Sense, Vec2};

/// Ce qu'un clic demande : (son, serveur du son ; `None` pour un son par défaut).
type Pick = (u64, Option<u64>);

/// Une tuile par son : emoji (image), nom ; grisée quand on ne peut pas jouer.
fn sound_grid(ui: &mut egui::Ui, t: &Tokens, list: &[Sound], guild: Option<u64>, enabled: bool, pick: &mut Option<Pick>) {
    let font = FontId::proportional(t.typography.body);
    let side = t.typography.body * 1.4;
    let pad = t.spacing.item_x;
    ui.horizontal_wrapped(|ui| {
        for s in list.iter().filter(|s| s.available) {
            let img = match &s.emoji {
                Some(SoundEmoji::Unicode(u)) => crate::state::emoji::find(u).map(Img::Unicode),
                Some(SoundEmoji::Custom(id)) => Some(Img::Custom(*id)),
                None => None,
            };
            let color = if enabled { t.colors.text } else { t.colors.faint };
            let galley = ui.painter().layout_no_wrap(s.name.clone(), font.clone(), color);
            let icon_w = if img.is_some() { side + pad } else { 0.0 };
            let size = egui::vec2(icon_w + galley.size().x + 2.0 * pad, t.spacing.channel_row.max(side));
            let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
            let fill = if enabled && resp.hovered() { t.colors.row_hover } else { t.colors.bg3 };
            ui.painter().rect_filled(rect, t.r_row(), fill);
            if let Some(img) = img {
                let at = Rect::from_center_size(egui::pos2(rect.left() + pad + side / 2.0, rect.center().y), Vec2::splat(side));
                super::emoji::paint(ui, at, img);
            }
            let text_at = egui::pos2(rect.left() + pad + icon_w, rect.center().y - galley.size().y / 2.0);
            ui.painter().galley(text_at, galley, color);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, &s.name));
            if resp.clicked() {
                *pick = Some((s.id, guild));
            }
        }
    });
}

impl App {
    /// Ouvre le tableau de sons du salon (et demande les listes manquantes).
    pub(super) fn open_soundboard(&mut self, channel: u64) {
        let guild = self.store.voice_room(channel).map(|r| r.guild_id);
        self.store.load_soundboard(guild);
        self.calls_ui.soundboard = Some(channel);
        self.flush_commands();
    }

    /// Ouvre le tableau de sons (aperçu, tests).
    pub fn show_soundboard(&mut self, channel: u64) {
        self.open_soundboard(channel);
    }

    pub(super) fn soundboard_ui(&mut self, ui: &mut egui::Ui) {
        let Some(cid) = self.calls_ui.soundboard else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        // Ouvert depuis le panneau vocal aussi : le salon n'est pas forcément celui qui est affiché.
        let guild = self.store.voice_room(cid).map(|r| r.guild_id);
        let can = self.store.can_play_sounds(cid) && !self.store.calls.is_busy(CallAction::SendSound, cid);
        let mut open = true;
        let mut pick = None;
        let frame = t.window_frame().inner_margin(t.toast_margin());
        let screen = ui.ctx().content_rect();
        let area = self.chat_rect.unwrap_or(screen);
        egui::Window::new(tx::SOUNDBOARD.get(lang))
            .id(Id::unique("tableau_de_sons"))
            .collapsible(false)
            .resizable(false)
            // Barre de titre dessinée ici : celle d'egui prend un gris plus clair que toute
            // l'interface dans les thèmes Cendre et Clair.
            .title_bar(false)
            .frame(frame)
            // Dans le coin de la discussion, pas de la fenêtre : il se posait à cheval sur la liste des membres.
            .pivot(egui::Align2::RIGHT_TOP)
            .fixed_pos(egui::pos2(area.right() - t.spacing.card_pad, (area.top() + t.spacing.card_pad).max(screen.top() + t.spacing.card_pad * 4.0)))
            .max_height(screen.height() * 0.7)
            .show(ui.ctx(), |ui| {
                ui.set_max_width(crate::ui::mobile::card_width(ui.ctx(), &t, 1.0));
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tx::SOUNDBOARD.get(lang)).family(crate::ui::fonts::demi()).size(t.typography.title).color(t.colors.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if crate::ui::widgets::close_button(ui, &t, crate::i18n::t::CLOSE_TIP.get(lang)).clicked() {
                            open = false;
                        }
                    });
                });
                if !self.store.can_play_sounds(cid) {
                    ui.label(RichText::new(tx::SOUNDS_NEED_VOICE.get(lang)).small().color(t.colors.muted));
                }
                egui::ScrollArea::vertical().auto_shrink([false, true]).show(ui, |ui| {
                    if let Some(g) = guild {
                        ui.label(RichText::new(tx::SERVER_SOUNDS.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                        match self.store.expressions.sounds(g) {
                            Some([]) => {
                                ui.label(RichText::new(tx::NO_SOUNDS.get(lang)).small().color(t.colors.faint));
                            }
                            Some(list) => sound_grid(ui, &t, list, Some(g), can, &mut pick),
                            None if self.store.calls.is_busy(CallAction::GuildSounds, g) => {
                                ui.label(RichText::new(tx::LOADING.get(lang)).small().color(t.colors.faint));
                            }
                            None => {
                                ui.label(RichText::new(tx::LOAD_FAILED.get(lang)).small().color(t.colors.danger));
                            }
                        }
                        ui.add_space(t.spacing.item_y);
                    }
                    ui.label(RichText::new(tx::DEFAULT_SOUNDS.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                    match self.store.calls.default_sounds() {
                        Loadable::Ready(list) => sound_grid(ui, &t, list, None, can, &mut pick),
                        Loadable::Failed => {
                            ui.label(RichText::new(tx::LOAD_FAILED.get(lang)).small().color(t.colors.danger));
                        }
                        Loadable::Absent | Loadable::Loading => {
                            ui.label(RichText::new(tx::LOADING.get(lang)).small().color(t.colors.faint));
                        }
                    }
                });
            });
        if let Some((sound, from)) = pick {
            self.store.play_sound(cid, sound, from);
            self.flush_commands();
        }
        // Salon quitté ou fenêtre fermée (Échap compris, lot C5).
        let connected = self.store.voice.me.is_some_and(|m| m.channel_id == cid);
        if !open || (self.store.channel != Some(cid) && !connected) || super::a11y::escape(ui.ctx()) {
            self.calls_ui.soundboard = None;
        }
    }
}
