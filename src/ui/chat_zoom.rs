//! Zoom par salon (plugin du lot E2) : la taille du texte d'une discussion,
//! et d'elle seule, retenue pour chaque salon.
//!
//! - Ctrl+molette sur la liste des messages, Ctrl+= et Ctrl+- pour la
//!   discussion active, Ctrl+0 pour revenir à 100 %. Plugin activé, ces
//!   touches ne règlent plus le zoom de toute l'interface tant qu'une
//!   discussion est affichée (il reste dans Paramètres > Apparence) ; coupé,
//!   rien n'est pris et tout est comme avant.
//! - La liste reçoit des jetons et un style dont seules les tailles de texte
//!   sont multipliées ([`zoomed`], [`zoom_style`]) ; ses hauteurs sont
//!   refaites pour ce salon seulement. À 100 %, rien n'est copié.
//! - Enregistré dans `local.toml` (`[fenetres.zoom]`) à la première image qui
//!   suit d'une seconde le dernier changement, ou à la fermeture.

use super::palette::{ctrl, Command};
use super::App;
use crate::i18n::table::e2::{f as wf, t as wt};
use crate::plugins::{self, Id as Plugin};
use crate::state::windows::ZOOM_STEP;
use crate::theme::Tokens;
use eframe::egui::{self, Key, Modifiers};
use std::time::{Duration, Instant};

#[derive(Default)]
pub(super) struct ZoomUi {
    /// Fraction de cran de molette en attente (pavé tactile).
    acc: f32,
    /// Dernier changement pas encore enregistré.
    dirty: Option<Instant>,
}

/// Jetons de la liste pour un zoom `k` : seules les tailles de texte changent.
pub(super) fn zoomed(t: &Tokens, k: f32) -> Tokens {
    let mut z = t.clone();
    let ty = &mut z.typography;
    for v in [&mut ty.body, &mut ty.small, &mut ty.title, &mut ty.line_height] {
        *v = (*v * k * 2.0).round() / 2.0;
    }
    let md = &mut z.markdown;
    for v in [&mut md.h1, &mut md.h2, &mut md.h3, &mut md.emoji, &mut md.jumbo] {
        *v = (*v * k * 2.0).round() / 2.0;
    }
    z
}

/// Styles de texte d'egui multipliés de même, pour ce `Ui` et ses enfants.
pub(super) fn zoom_style(ui: &mut egui::Ui, k: f32) {
    for (style, font) in &mut ui.style_mut().text_styles {
        if matches!(style, egui::TextStyle::Body | egui::TextStyle::Small | egui::TextStyle::Monospace) {
            font.size = (font.size * k * 2.0).round() / 2.0;
        }
    }
}

impl App {
    fn zoom_on(&self) -> bool {
        plugins::on(Plugin::ChannelZoom)
    }

    /// Facteur du salon : 1 à 100 %, et toujours 1 plugin coupé.
    pub(super) fn chat_zoom(&self, cid: u64) -> f32 {
        if !self.zoom_on() {
            return 1.0;
        }
        f32::from(self.local.prefs.windows.zoom_of(cid)) / 100.0
    }

    /// Zoom du salon, en % (tests, aperçu).
    pub fn channel_zoom(&self, cid: u64) -> u16 {
        self.local.prefs.windows.zoom_of(cid)
    }

    /// Règle le zoom du salon (borné), le dit, et le fera enregistrer.
    pub fn set_channel_zoom(&mut self, cid: u64, percent: u16) {
        if !self.zoom_on() || !self.local.prefs.windows.set_zoom(cid, percent) {
            return;
        }
        self.zoom.dirty = Some(Instant::now());
        let now = self.local.prefs.windows.zoom_of(cid);
        self.notify(wf::zoom_now(self.lang, now));
        self.ctx.request_repaint();
    }

    fn zoom_step(&mut self, cid: u64, steps: i32) {
        let now = i32::from(self.channel_zoom(cid));
        let next = (now + steps * i32::from(ZOOM_STEP)).clamp(0, i32::from(u16::MAX));
        self.set_channel_zoom(cid, next as u16);
    }

    /// Ctrl+=, Ctrl+-, Ctrl+0 : la discussion active. Rien n'est pris sans
    /// discussion affichée, ni pendant que les Paramètres sont ouverts.
    pub(super) fn zoom_keys(&mut self, ui: &egui::Ui) {
        if !self.zoom_on() || self.params.open {
            return;
        }
        let Some(cid) = self.store.channel else { return };
        let (more, less, reset) = ui.input_mut(|i| {
            let more = i.consume_key(Modifiers::COMMAND, Key::Equals) || i.consume_key(Modifiers::COMMAND, Key::Plus);
            (more, i.consume_key(Modifiers::COMMAND, Key::Minus), i.consume_key(Modifiers::COMMAND, Key::Num0))
        });
        if reset {
            self.set_channel_zoom(cid, 100);
        } else if more || less {
            self.zoom_step(cid, if more { 1 } else { -1 });
        }
    }

    /// Ctrl+molette, pointeur sur la liste de `cid` : un cran, un pas.
    pub(super) fn zoom_wheel(&mut self, ui: &egui::Ui, cid: u64) {
        if !self.zoom_on() || !ui.rect_contains_pointer(ui.available_rect_before_wrap()) {
            return;
        }
        let turned: f32 = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    egui::Event::MouseWheel { unit, delta, modifiers, .. } if modifiers.command => Some(match unit {
                        egui::MouseWheelUnit::Line => delta.y,
                        egui::MouseWheelUnit::Page => delta.y * 3.0,
                        egui::MouseWheelUnit::Point => delta.y / 50.0,
                    }),
                    _ => None,
                })
                .sum()
        });
        if turned == 0.0 {
            return;
        }
        self.zoom.acc += turned;
        let steps = self.zoom.acc.trunc();
        self.zoom.acc -= steps;
        if steps != 0.0 {
            self.zoom_step(cid, steps as i32);
        }
    }

    /// Début de l'image : zoom changé il y a plus d'une seconde, enregistré.
    pub(super) fn zoom_tick(&mut self) {
        if self.zoom.dirty.is_some_and(|at| at.elapsed() >= Duration::from_secs(1)) {
            self.zoom_exit();
        }
    }

    /// Enregistre le zoom en attente (aussi à la fermeture).
    pub(super) fn zoom_exit(&mut self) {
        if self.zoom.dirty.take().is_some() && self.local.loaded {
            self.save_local_prefs();
        }
    }

    /// Actions du zoom dans la palette de commandes.
    pub(super) fn zoom_commands(&self, out: &mut Vec<Command>) {
        if !self.zoom_on() {
            return;
        }
        let Some(cid) = self.store.channel else { return };
        let lang = self.lang;
        out.push(Command::new(wt::G_VIEW, wt::CMD_ZOOM_IN.get(lang), move |app| app.zoom_step(cid, 1)).chord(ctrl(lang, "=")));
        out.push(Command::new(wt::G_VIEW, wt::CMD_ZOOM_OUT.get(lang), move |app| app.zoom_step(cid, -1)).chord(ctrl(lang, "-")));
        out.push(Command::new(wt::G_VIEW, wt::CMD_ZOOM_RESET.get(lang), move |app| app.set_channel_zoom(cid, 100)).chord(ctrl(lang, "0")));
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, frame, key};
    use super::*;

    const CTRL: Modifiers = Modifiers { ctrl: true, command: true, alt: false, shift: false, mac_cmd: false };

    #[test]
    fn only_text_sizes_change() {
        let t = Tokens::default();
        let z = zoomed(&t, 1.5);
        assert_eq!(z.typography.body, t.typography.body * 1.5);
        assert_eq!(z.markdown.h1, t.markdown.h1 * 1.5);
        assert_eq!((z.typography.button, z.typography.icon, z.layout.avatar_size), (t.typography.button, t.typography.icon, t.layout.avatar_size));
        assert_eq!(z.spacing, t.spacing);
        assert_eq!(zoomed(&t, 1.0), t);
    }

    #[test]
    fn keys_zoom_the_open_channel_only_and_it_is_remembered() {
        let mut a = app();
        frame(&mut a, Vec::new());
        let global = a.ctx.zoom_factor();
        frame(&mut a, vec![key(Key::Equals, CTRL)]);
        frame(&mut a, vec![key(Key::Equals, CTRL)]);
        assert_eq!((a.channel_zoom(10), a.channel_zoom(11)), (120, 100));
        assert_eq!(a.chat_zoom(10), 1.2);
        assert_eq!(a.ctx.zoom_factor(), global, "zoom de toute la fenêtre inchangé");
        assert!(a.notice().is_some_and(|n| n.contains("120")), "{:?}", a.notice());
        frame(&mut a, vec![key(Key::Minus, CTRL)]);
        assert_eq!(a.channel_zoom(10), 110);
        // L'autre salon garde sa taille ; revenu, le premier retrouve la sienne.
        a.open(Some(9), 11);
        frame(&mut a, Vec::new());
        assert_eq!(a.chat_zoom(11), 1.0);
        a.open(Some(9), 10);
        frame(&mut a, vec![key(Key::Num0, CTRL)]);
        assert_eq!(a.channel_zoom(10), 100);
        assert!(a.local_prefs().windows.zoom.is_empty(), "100 % : rien de retenu");
        // Bornes.
        a.set_channel_zoom(10, 9000);
        assert_eq!(a.channel_zoom(10), crate::state::windows::ZOOM_MAX);
        // Plugin coupé : la taille du thème, et les touches ne sont plus prises.
        a.set_plugin(Plugin::ChannelZoom, false);
        assert_eq!(a.chat_zoom(10), 1.0);
        frame(&mut a, vec![key(Key::Minus, CTRL)]);
        assert_eq!(a.channel_zoom(10), crate::state::windows::ZOOM_MAX, "réglage gardé pour plus tard, pas changé");
    }

    #[test]
    fn ctrl_wheel_over_the_list_steps_the_zoom() {
        let mut a = app();
        frame(&mut a, Vec::new());
        let over = egui::Event::PointerMoved(egui::pos2(600.0, 300.0));
        let wheel = |y: f32, modifiers| egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, y), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers };
        frame(&mut a, vec![over.clone(), wheel(1.0, CTRL)]);
        assert_eq!(a.channel_zoom(10), 110);
        frame(&mut a, vec![wheel(-2.0, CTRL)]);
        assert_eq!(a.channel_zoom(10), 90);
        frame(&mut a, vec![wheel(1.0, Modifiers::NONE)]);
        assert_eq!(a.channel_zoom(10), 90, "sans Ctrl : la molette fait défiler");
        // Hors de la liste (colonne des salons) : rien.
        frame(&mut a, vec![egui::Event::PointerMoved(egui::pos2(150.0, 300.0)), wheel(1.0, CTRL)]);
        assert_eq!(a.channel_zoom(10), 90);
    }
}
