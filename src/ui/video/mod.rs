//! Vue d'appel (phase 5, lot M4) : dans la zone centrale quand on ouvre un
//! salon vocal, ou l'appel d'un MP mis en plein cadre (sinon l'appel d'un MP
//! est un bandeau au-dessus de la conversation, `call_band.rs`, qui reprend
//! les tuiles d'ici dès qu'une vidéo est là). Grille de tuiles
//! (caméra ou avatar), directs avec « Regarder », tuile mise en avant avec
//! une bande des autres, plein écran (F11, double-clic), fenêtres détachées
//! (viewports immédiats, « toujours au-dessus »), barre de contrôle (micro,
//! sourdine, caméra, partage, quitter), bandeau du partage en cours, et
//! indicateur dans le panneau vocal tant que caméra ou partage sont actifs.
//!
//! Aucune capture ni réseau vidéo ici : les images arrivent par
//! `state::video::VideoSink` (lot M5 ; mires animées dans l'aperçu et les
//! tests). Rien ne se redessine au repos : seules les images reçues pour une
//! tuile affichée réveillent l'interface.

mod icons;
mod tiles;

use super::App;
use crate::i18n::table::video::{f as vf, p as vp, t as vt};
use crate::state::stream::{EndReason, StreamConnState};
use crate::state::video::{Tile, TileInfo, TileKind, VideoSink};
use crate::state::voice::MyVoice;
use crate::theme::Tokens;
use crate::ui::voice::icons::{self as vicons, VIcon};
use crate::video::Quality;
use eframe::egui::{self, pos2, vec2, Color32, Id, Rect, RichText, Sense, Vec2};
use icons::Glyph;
use std::collections::HashSet;
use tiles::{grid, tile_ui, Cx, Screens, TileAct, TileView, RATIO};

#[derive(Default)]
pub(super) struct VideoUi {
    /// Salons dont la discussion est affichée à la place de l'appel.
    pub(super) chat_open: HashSet<u64>,
    screens: Screens,
    /// Tuile en plein écran.
    fullscreen: Option<Tile>,
    /// Plein écran demandé à la fenêtre (pour ne l'envoyer qu'une fois).
    window_full: bool,
    /// Fenêtres détachées que l'utilisateur a sorties du « toujours au-dessus ».
    not_on_top: HashSet<Tile>,
    /// Sélecteur de partage relié (lot M3, par l'intégrateur) : sans lui,
    /// le bouton « Partager l'écran » n'est pas montré.
    picker_linked: bool,
    picker_requested: bool,
    /// Réveil donné à `VideoSink` (une fois).
    wake_set: bool,
}

/// Bouton rond de la barre de contrôle ; `fill` : fond, `color` : icône.
fn round_button(ui: &mut egui::Ui, t: &Tokens, side: f32, tip: &str, fill: Color32, color: Color32, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> bool {
    pill_button(ui, t, Vec2::splat(side), tip, fill, color, paint)
}

/// Hauteur de la zone des tuiles du bandeau d'un MP sous laquelle seules les vidéos sont montrées.
const LEAD_ROOMY: f32 = 240.0;

/// Largeur du bouton « Raccrocher » : une pilule, plus large que haute.
const HANGUP_WIDTH: f32 = 1.6;

/// Bouton en pilule (rond quand il est carré) ; l'icône garde la taille de celle d'un bouton rond.
fn pill_button(ui: &mut egui::Ui, t: &Tokens, size: Vec2, tip: &str, fill: Color32, color: Color32, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> bool {
    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
    let fill = if r.hovered() || r.has_focus() { t.colors.hover.lerp_to_gamma(fill, 0.5) } else { fill };
    let side = size.y;
    ui.painter().rect_filled(rect, side / 2.0, fill);
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(side)).shrink(side * 0.27), color);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    r.on_hover_text(tip).clicked()
}

impl App {
    /// Salon ouvert = salon vocal (pas une conférence), ou appel de MP où je
    /// suis, mis en plein cadre : sinon l'appel d'un MP est un bandeau
    /// au-dessus de la conversation (`call_band.rs`).
    fn is_call_place(&self, cid: u64) -> bool {
        let v = &self.store.voice;
        v.channel(cid).is_some_and(|c| !c.stage) || (v.me == Some(MyVoice { guild_id: None, channel_id: cid }) && self.calls_ui.band.wide == Some(cid))
    }

    /// Appelé au début de la zone du salon : dessine la vue d'appel et rend
    /// `true`, ou, discussion ouverte, une bande « Revenir à l'appel ».
    pub(super) fn call_view_hook(&mut self, ui: &mut egui::Ui, cid: u64) -> bool {
        if !self.is_call_place(cid) {
            return false;
        }
        if self.video_ui.chat_open.contains(&cid) {
            let t = self.theme.clone();
            let mut back = false;
            egui::Panel::top("call_back").frame(egui::Frame::new().fill(t.colors.bg1).inner_margin(crate::ui::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
                back = ui.button(vt::SHOW_CALL.get(self.lang)).clicked();
            });
            if back {
                self.video_ui.chat_open.remove(&cid);
            }
            return false;
        }
        self.call_view(ui, cid);
        true
    }

    /// Ce que montrent les tuiles d'un salon (lu avant de dessiner).
    fn tile_views(&self, cid: u64) -> Vec<TileView> {
        self.store
            .call_tiles(cid)
            .into_iter()
            .filter(|i| !self.store.video.popped.contains(&i.tile))
            .map(|i| self.tile_view(i))
            .collect()
    }

    fn tile_view(&self, i: TileInfo) -> TileView {
        let v = &self.store.voice;
        let st = v.state(i.user_id);
        let (name, avatar) = match st {
            Some(s) => self.voice_who(s),
            None => (crate::i18n::table::voice::t::UNKNOWN_USER.get(self.lang).to_string(), crate::state::model::Avatar::default_for(i.user_id)),
        };
        let conn = match &i.tile {
            Tile::Stream(k) => self.store.stream.get(k),
            Tile::Camera(_) => None,
        };
        let person = matches!(i.kind, TileKind::Person { .. });
        TileView {
            who: name.clone(),
            name: if person { name } else { vf::screen_of(self.lang, name) },
            avatar,
            speaking: person && v.speaking.contains(&i.user_id),
            muted: person && st.is_some_and(|s| s.muted()),
            deafened: person && st.is_some_and(|s| s.deafened()),
            viewers: conn.map_or(0, |c| c.viewers.len()),
            paused: conn.is_some_and(|c| c.paused),
            connecting: conn.is_some_and(|c| !matches!(c.state, StreamConnState::Connected)),
            tile: i.tile,
            kind: i.kind,
        }
    }

    fn call_view(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let lang = self.lang;
        let joined = self.store.voice.me.is_some_and(|m| m.channel_id == cid);
        let name = self.store.voice.channel(cid).map(|c| c.name.clone()).or_else(|| self.store.dms.iter().find(|d| d.id == cid).map(|d| crate::ui::words::dm_name(lang, &d.name).to_string())).unwrap_or_default();
        let count = self.store.voice.count(cid) as u64;
        // Appel d'un MP en plein cadre : la discussion est sous le bandeau, on y revient en réduisant l'appel.
        let dm = self.store.voice.channel(cid).is_none();
        let (back, back_tip) = if dm { (vt::SHRINK, vt::SHRINK_TIP) } else { (vt::SHOW_CHAT, vt::SHOW_CHAT_TIP) };
        let mut open_chat = false;
        egui::Panel::top("call_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(crate::ui::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(crate::ui::chat_head::head_row(&t));
                // Disposition mobile : retour à la liste (phase 7).
                self.mobile_back(ui);
                let (r, _) = ui.allocate_exact_size(Vec2::splat(Tokens::icon_side(t.typography.icon)), Sense::hover());
                vicons::paint(VIcon::Speaker, ui.painter(), r, t.colors.faint);
                let title = || egui::Label::new(RichText::new(&name).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong)).truncate();
                if crate::ui::mobile::is_active(ui.ctx()) {
                    // Écran étroit : le bouton d'abord, le nom tronqué dans la place restante,
                    // sans le nombre de personnes qui le recouvrait.
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        open_chat = ui.button(back.get(lang)).on_hover_text(back_tip.get(lang)).clicked();
                        let size = ui.available_size();
                        ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(title()));
                    });
                } else {
                    ui.add(title());
                    ui.label(RichText::new(vp::people(lang, count)).small().color(t.colors.faint));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        open_chat = ui.button(back.get(lang)).on_hover_text(back_tip.get(lang)).clicked();
                    });
                }
            });
        });
        if open_chat && dm {
            self.calls_ui.band.wide = None;
        } else if open_chat {
            self.video_ui.chat_open.insert(cid);
        }
        self.call_controls(ui, cid, joined);
        // Fond de la vue d'appel : noir, comme dans le client.
        ui.painter().rect_filled(ui.available_rect_before_wrap(), 0.0, t.call_bg());
        let area = ui.available_rect_before_wrap().shrink(t.spacing.chat_pad);
        ui.allocate_rect(area, Sense::hover());
        self.call_tiles_ui(ui, cid, area, false);
    }

    /// Une tuile au moins de cet appel montre une vidéo ou un direct (caméra
    /// allumée, partage d'écran, le mien compris) : le bandeau d'un MP s'agrandit.
    pub(in crate::ui) fn call_has_video(&self, cid: u64) -> bool {
        self.store.call_tiles(cid).iter().any(|i| !matches!(i.kind, TileKind::Person { camera: false }))
    }

    /// Tuiles d'un appel dans `area` (déjà réservée) : grille, ou tuile mise en
    /// avant et bande des autres ; F11 et double-clic passent en plein écran.
    pub(in crate::ui) fn call_tiles_ui(&mut self, ui: &mut egui::Ui, cid: u64, area: Rect, lead: bool) {
        let t = self.theme.clone();
        let lang = self.lang;
        let mut views = self.tile_views(cid);
        let has_video = |v: &TileView| !matches!(v.kind, TileKind::Person { camera: false });
        // Bandeau bas : seules les vidéos sont montrées, la place leur revient.
        if lead && area.height() < LEAD_ROOMY && views.iter().any(has_video) {
            views.retain(has_video);
        }
        if views.is_empty() {
            ui.painter().text(area.center(), egui::Align2::CENTER_CENTER, vt::EMPTY.get(lang), egui::FontId::proportional(t.typography.body), t.colors.muted);
            return;
        }
        // Plein écran : la grille n'est pas dessinée dessous.
        if self.video_ui.fullscreen.is_some() {
            return;
        }
        // `lead` (bandeau d'un MP) : sans tuile choisie, la vidéo passe devant (un direct d'abord).
        let auto = || views.iter().filter(|v| has_video(v)).max_by_key(|v| matches!(v.kind, TileKind::Stream { .. })).map(|v| v.tile.clone());
        let focus = self.store.video.focus.clone().filter(|f| views.iter().any(|v| &v.tile == f)).or_else(|| if lead { auto() } else { None });
        let gap = t.spacing.media_gap.max(2.0);
        let mut acts = Vec::new();
        let mut cx = Cx { t: &t, lang, images: &self.images, sink: &self.store.video.sink, screens: &mut self.video_ui.screens };
        match focus {
            None => {
                for (r, v) in grid(area, views.len(), gap).into_iter().zip(&views) {
                    acts.extend(tile_ui(ui, r, v, false, &mut cx));
                }
            }
            Some(f) => {
                let others: Vec<&TileView> = views.iter().filter(|v| v.tile != f).collect();
                let strip_h = if others.is_empty() { 0.0 } else { (area.height() * 0.2).max(t.typography.body * 4.0) };
                let main = Rect::from_min_max(area.min, pos2(area.right(), area.bottom() - if strip_h > 0.0 { strip_h + gap } else { 0.0 }));
                if let Some(v) = views.iter().find(|v| v.tile == f) {
                    let r = Rect::from_center_size(main.center(), {
                        let w = main.width().min(main.height() * RATIO);
                        vec2(w, w / RATIO)
                    });
                    acts.extend(tile_ui(ui, r, v, true, &mut cx));
                }
                if strip_h > 0.0 {
                    let strip = Rect::from_min_max(pos2(area.left(), area.bottom() - strip_h), area.max);
                    // Les vignettes rétrécissent pour tenir toutes dans la bande (aucune n'est
                    // tranchée par le bord) ; sous 60 % de leur taille, la bande défile.
                    let n = others.len().max(1) as f32;
                    let fit = (strip.width() - gap * (n - 1.0)) / n;
                    let w = (strip_h * RATIO).min(fit.floor()).max(strip_h * RATIO * 0.6);
                    let size = vec2(w, w / RATIO);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| {
                        egui::ScrollArea::horizontal().id_salt("bande").show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for v in others {
                                    let (r, _) = ui.allocate_exact_size(size, Sense::hover());
                                    acts.extend(tile_ui(ui, r, v, false, &mut cx));
                                }
                            });
                        });
                    });
                }
            }
        }
        // F11 : plein écran de la tuile en avant, sinon de la première vidéo.
        if ui.input(|i| i.key_pressed(egui::Key::F11)) {
            let pick = self.store.video.focus.clone().or_else(|| views.iter().find(|v| matches!(v.kind, TileKind::Person { camera: true } | TileKind::Stream { watching: true, .. })).map(|v| v.tile.clone()));
            if let Some(tile) = pick {
                acts.push(TileAct::Fullscreen(tile));
            }
        }
        self.tile_acts(acts);
    }

    fn tile_acts(&mut self, acts: Vec<TileAct>) {
        for a in acts {
            match a {
                TileAct::Focus(t) => self.store.video_focus(Some(t)),
                TileAct::Unfocus => self.store.video_focus(None),
                TileAct::Fullscreen(t) => self.video_ui.fullscreen = Some(t),
                TileAct::PopOut(t) => {
                    if self.video_ui.fullscreen.as_ref() == Some(&t) {
                        self.video_ui.fullscreen = None;
                    }
                    self.store.video_pop_out(t);
                }
                TileAct::Watch(k) => {
                    self.store.video_watch(&k);
                }
                TileAct::StopWatching(k) => self.store.video_stop_watching(&k),
            }
        }
    }

    /// Barre de contrôle en bas de la vue, et bandeau du partage en cours.
    fn call_controls(&mut self, ui: &mut egui::Ui, cid: u64, joined: bool) {
        let t = self.theme.clone();
        let lang = self.lang;
        let v = &self.store.voice;
        let (mute, deaf) = (v.self_mute, v.self_deaf);
        let camera = self.store.video.camera;
        let share = self.store.video.share.as_ref().map(|s| s.name.clone());
        let picker = self.video_ui.picker_linked;
        enum Act {
            Mute,
            Deaf,
            Camera,
            Share,
            StopShare,
            Leave,
            Join,
        }
        let mut act = None;
        egui::Panel::bottom("call_controls").frame(egui::Frame::new().fill(t.call_bg()).inner_margin(t.chat_head_margin())).show(ui, |ui| {
            if let Some(what) = &share {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(Tokens::icon_side(t.typography.body)), Sense::hover());
                    icons::paint(Glyph::Screen, ui.painter(), r, t.colors.danger);
                    let label = || egui::Label::new(RichText::new(vf::sharing(lang, what)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).truncate();
                    // Disposition mobile : « Arrêter » d'abord, le texte raccourci dans la place qui reste.
                    let mobile = crate::ui::mobile::is_active(ui.ctx());
                    if !mobile {
                        ui.add(label());
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.add(egui::Button::new(RichText::new(vt::STOP.get(lang)).color(t.colors.on_accent)).fill(t.danger_fill())).clicked() {
                            act = Some(Act::StopShare);
                        }
                        if mobile {
                            let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| ui.add(label()));
                        }
                    });
                });
                ui.add_space(t.spacing.item_y);
            }
            if !joined {
                ui.vertical_centered(|ui| {
                    if ui.add(egui::Button::new(RichText::new(vt::JOIN.get(lang)).color(t.colors.on_accent)).fill(t.colors.online)).clicked() {
                        act = Some(Act::Join);
                    }
                });
                return;
            }
            let pad = ui.spacing().button_padding.y;
            let side = Tokens::icon_button_side(t.typography.icon, pad) * 1.4;
            let gap = t.spacing.item_x.max(4.0);
            let n = if picker || share.is_some() { 5.0 } else { 4.0 } - if crate::video::capture::AVAILABLE { 0.0 } else { 1.0 };
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.add_space(((ui.available_width() - (n - 1.0 + HANGUP_WIDTH) * side - (n - 1.0) * gap) / 2.0).max(0.0));
                let idle = (t.colors.bg3, t.colors.text_strong);
                let (f, c) = if mute { (t.colors.bg3, t.colors.danger) } else { idle };
                if round_button(ui, &t, side, if mute { crate::i18n::table::voice::t::UNMUTE.get(lang) } else { crate::i18n::table::voice::t::MUTE.get(lang) }, f, c, |p, r, c| vicons::paint(if mute { VIcon::MicOff } else { VIcon::Mic }, p, r, c)) {
                    act = Some(Act::Mute);
                }
                let (f, c) = if deaf { (t.colors.bg3, t.colors.danger) } else { idle };
                if round_button(ui, &t, side, if deaf { crate::i18n::table::voice::t::UNDEAFEN.get(lang) } else { crate::i18n::table::voice::t::DEAFEN.get(lang) }, f, c, |p, r, c| vicons::paint(if deaf { VIcon::HeadphonesOff } else { VIcon::Headphones }, p, r, c)) {
                    act = Some(Act::Deaf);
                }
                let (f, c) = if camera { (t.colors.accent, t.colors.on_accent) } else { idle };
                // Sans capture sur ce système (Android), pas de bouton caméra.
                if crate::video::capture::AVAILABLE
                    && round_button(ui, &t, side, if camera { vt::CAMERA_OFF.get(lang) } else { vt::CAMERA_ON.get(lang) }, f, c, |p, r, c| {
                    if camera {
                        vicons::paint(VIcon::Camera, p, r, c)
                    } else {
                        icons::paint(Glyph::CameraOff, p, r, c)
                    }
                }) {
                    act = Some(Act::Camera);
                }
                if share.is_some() {
                    if round_button(ui, &t, side, vt::STOP_SHARE.get(lang), t.colors.accent, t.colors.on_accent, |p, r, c| icons::paint(Glyph::ScreenOff, p, r, c)) {
                        act = Some(Act::StopShare);
                    }
                } else if picker && round_button(ui, &t, side, vt::SHARE.get(lang), idle.0, idle.1, |p, r, c| icons::paint(Glyph::Screen, p, r, c)) {
                    act = Some(Act::Share);
                }
                // Raccrocher : pilule rouge, plus large que les boutons ronds.
                if pill_button(ui, &t, egui::vec2((side * HANGUP_WIDTH).round(), side), vt::LEAVE.get(lang), t.danger_fill(), t.colors.on_accent, |p, r, c| vicons::paint(VIcon::Hangup, p, r, c)) {
                    act = Some(Act::Leave);
                }
            });
        });
        match act {
            Some(Act::Mute) => self.voice_toggle_mute(),
            Some(Act::Deaf) => self.voice_toggle_deaf(),
            Some(Act::Camera) => {
                self.toggle_camera();
            }
            Some(Act::Share) => self.request_share_picker(),
            Some(Act::StopShare) => self.store.video_stop_share(),
            Some(Act::Leave) => self.store.voice_leave(),
            Some(Act::Join) => self.voice_join(cid),
            None => {}
        }
    }

    /// Indicateur du panneau vocal : visible tant que caméra ou partage sont actifs.
    pub(in crate::ui) fn video_indicator(&mut self, ui: &mut egui::Ui) {
        let v = &self.store.video;
        if !v.sending() {
            return;
        }
        let t = &self.theme;
        let lang = self.lang;
        let side = Tokens::icon_side(t.typography.small);
        if v.camera {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                vicons::paint(VIcon::Camera, ui.painter(), r, t.colors.danger);
                ui.add(egui::Label::new(RichText::new(vt::CAMERA_ACTIVE.get(lang)).small().color(t.colors.danger)).truncate().selectable(false));
            });
        }
        if let Some(s) = &v.share {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                icons::paint(Glyph::Screen, ui.painter(), r, t.colors.danger);
                ui.add(egui::Label::new(RichText::new(vf::sharing(lang, &s.name)).small().color(t.colors.danger)).truncate().selectable(false));
            });
        }
    }

    /// Une fois par image, après la zone centrale : fenêtres détachées, plein
    /// écran, fins de direct à annoncer, puis seules les tuiles dessinées
    /// gardent leur texture et reçoivent des images.
    pub(super) fn video_frame_end(&mut self, ui: &mut egui::Ui) {
        if !self.video_ui.wake_set {
            self.video_ui.wake_set = true;
            let ctx = ui.ctx().clone();
            self.store.video.sink.set_wake(std::sync::Arc::new(move || ctx.request_repaint()));
        }
        if let Some((_, reason)) = self.store.stream.take_ended() {
            let text = match reason {
                EndReason::Full => vt::END_FULL.get(self.lang).to_string(),
                EndReason::Unauthorized => vt::END_UNAUTHORIZED.get(self.lang).to_string(),
                EndReason::RateLimited => vt::END_RATE.get(self.lang).to_string(),
                EndReason::Unavailable => vt::END_UNAVAILABLE.get(self.lang).to_string(),
                EndReason::Other(r) => vf::ended(self.lang, r),
                EndReason::UserRequested | EndReason::StreamEnded => String::new(),
            };
            if !text.is_empty() {
                self.notify(text);
            }
        }
        self.video_popouts(ui);
        self.video_fullscreen(ui);
        self.video_ui.screens.end_frame(&self.store.video.sink);
    }

    /// Vue d'une seule tuile (plein écran, fenêtre détachée).
    fn solo_view(&mut self, tile: &Tile) -> Option<TileView> {
        let i = self.store.call_tiles(self.store.voice.me?.channel_id).into_iter().find(|i| &i.tile == tile)?;
        Some(self.tile_view(i))
    }

    fn video_fullscreen(&mut self, ui: &mut egui::Ui) {
        let view = self.video_ui.fullscreen.clone().and_then(|t| self.solo_view(&t));
        let want = view.is_some();
        if want != self.video_ui.window_full {
            self.video_ui.window_full = want;
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Fullscreen(want));
        }
        let Some(view) = view else {
            self.video_ui.fullscreen = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let screen = ui.ctx().content_rect();
        let mut acts = Vec::new();
        let mut exit = ui.input(|i| i.key_pressed(egui::Key::Escape) || i.key_pressed(egui::Key::F11));
        egui::Area::new(Id::unique("video_plein_ecran")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ui.ctx(), |ui| {
            ui.painter().rect_filled(screen, 0.0, t.colors.bg0);
            ui.allocate_rect(screen, Sense::hover());
            let mut cx = Cx { t: &t, lang, images: &self.images, sink: &self.store.video.sink, screens: &mut self.video_ui.screens };
            if let Some(a) = tile_ui(ui, screen, &view, false, &mut cx) {
                match a {
                    // Double-clic ou bouton « plein écran » : on en sort.
                    TileAct::Fullscreen(_) => exit = true,
                    TileAct::Focus(_) | TileAct::Unfocus => {}
                    other => acts.push(other),
                }
            }
            let side = t.typography.title * 1.6;
            let r = Rect::from_min_size(pos2(screen.right() - side - t.spacing.chat_pad, screen.top() + t.spacing.chat_pad), Vec2::splat(side));
            let cross = ui.put(r, egui::Button::new("").fill(t.media_veil())).on_hover_text(vt::EXIT_FULLSCREEN.get(lang));
            crate::theme::icons::paint(crate::theme::icons::Icon::Close, ui.painter(), egui::Rect::from_center_size(r.center(), Vec2::splat(r.height().min(r.width()) * 0.5)), t.on_media());
            if cross.clicked() {
                exit = true;
            }
        });
        if exit {
            self.video_ui.fullscreen = None;
        }
        self.tile_acts(acts);
    }

    /// Fenêtres détachées : une fenêtre système par tuile (viewport
    /// immédiat), « toujours au-dessus » par défaut ; la fermer rend la tuile
    /// à la vue d'appel.
    fn video_popouts(&mut self, ui: &mut egui::Ui) {
        let popped = self.store.video.popped.clone();
        for tile in popped {
            let Some(view) = self.solo_view(&tile) else {
                self.store.video_pop_in(&tile);
                continue;
            };
            let t = self.theme.clone();
            let lang = self.lang;
            let on_top = !self.video_ui.not_on_top.contains(&tile);
            let level = if on_top { egui::WindowLevel::AlwaysOnTop } else { egui::WindowLevel::Normal };
            let builder = egui::ViewportBuilder::default().with_title(view.name.clone()).with_inner_size([480.0, 270.0]).with_min_inner_size([160.0, 90.0]).with_window_level(level);
            let vid = egui::ViewportId::from_hash_of(("video_detachee", &tile));
            let images = &self.images;
            let sink = &self.store.video.sink;
            let screens = &mut self.video_ui.screens;
            let (closed, toggle_top, acts) = ui.ctx().show_viewport_immediate(vid, builder, |ui, _class| {
                let mut toggle = false;
                let mut acts = Vec::new();
                egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg0)).show(ui, |ui| {
                    let area = ui.max_rect();
                    let mut cx = Cx { t: &t, lang, images, sink, screens };
                    acts.extend(tile_ui(ui, area, &view, false, &mut cx));
                    let side = t.typography.body + t.spacing.media_gap * 2.0;
                    let r = Rect::from_min_size(pos2(area.right() - side - t.spacing.media_gap, area.bottom() - side - t.spacing.media_gap), Vec2::splat(side));
                    let resp = ui.interact(r, ui.scope_id().with("au_dessus"), Sense::click());
                    ui.painter().rect_filled(r, t.r_row(), if on_top { t.colors.accent } else { t.media_veil() });
                    icons::paint(Glyph::Pin, ui.painter(), r.shrink(side * 0.2), if on_top { t.colors.on_accent } else { t.on_media() });
                    toggle = resp.on_hover_text(vt::ON_TOP.get(lang)).clicked();
                });
                (ui.input(|i| i.viewport().close_requested()), toggle, acts)
            });
            if toggle_top && !self.video_ui.not_on_top.remove(&tile) {
                self.video_ui.not_on_top.insert(tile.clone());
            }
            let acts: Vec<TileAct> = acts.into_iter().filter(|a| matches!(a, TileAct::StopWatching(_))).collect();
            self.tile_acts(acts);
            if closed {
                self.video_ui.not_on_top.remove(&tile);
                self.store.video_pop_in(&tile);
            }
        }
    }

    /// « Partager l'écran » : ouvre le sélecteur de partage (lot M3) quand il
    /// est relié, sinon garde la demande.
    /// Le sélecteur de source est branché : « Partager » peut être proposé.
    pub(in crate::ui) fn share_picker_ready(&self) -> bool {
        self.video_ui.picker_linked
    }

    pub fn request_share_picker(&mut self) {
        if self.video_ui.picker_linked {
            self.open_share_picker(crate::state::share::ShareTab::default());
        } else {
            self.video_ui.picker_requested = true;
        }
    }

    /// Demande en attente, rendue une fois (lue par le sélecteur du lot M3).
    pub fn take_share_picker_request(&mut self) -> bool {
        std::mem::take(&mut self.video_ui.picker_requested)
    }

    /// Le sélecteur de partage est relié : « Partager l'écran » apparaît.
    pub fn enable_share_picker(&mut self) {
        self.video_ui.picker_linked = true;
    }

    /// Démarre un partage choisi dans le sélecteur (lot M3).
    pub fn start_share(&mut self, source_id: &str, name: &str, quality: Quality, audio: bool) -> bool {
        self.store.video_start_share(source_id, name, quality, audio)
    }

    /// File d'images des tuiles (câblage M5, aperçu).
    pub fn video_sink(&self) -> VideoSink {
        self.store.video.sink.clone()
    }

    /// Met une tuile en avant (aperçu, tests).
    pub fn focus_tile(&mut self, tile: Option<Tile>) {
        self.store.video_focus(tile);
    }

    /// Ouvre un salon vocal ou un appel avec la vue d'appel (aperçu, tests).
    pub fn show_call(&mut self, guild: Option<u64>, channel: u64) {
        self.store.guild = guild;
        self.open_channel(channel);
        self.video_ui.chat_open.remove(&channel);
        // Appel d'un MP : en plein cadre (sinon c'est un bandeau au-dessus de la conversation).
        if guild.is_none() {
            self.calls_ui.band.wide = Some(channel);
        }
    }

    /// Nombre de textures vidéo vivantes (tests).
    #[cfg(test)]
    pub(super) fn video_textures(&self) -> usize {
        self.video_ui.screens.textures()
    }
}
