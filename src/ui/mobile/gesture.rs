//! Gestes tactiles de la disposition mobile : glissement horizontal entre
//! écrans, « tirer pour charger » en haut de la discussion, et toucher
//! simulé à la souris (aperçu et fenêtre étroite sur Windows) pour que
//! l'appui long d'egui ouvre les menus comme sur un écran tactile.
//!
//! Le défilement par glissement avec inertie et l'appui long (clic
//! secondaire) sont ceux d'egui (`ScrollArea`, `Response::secondary_clicked`).

use super::nav::Swipe;
use eframe::egui::{self, Pos2};

/// Course minimale d'un glissement (points), et part de la largeur.
const SWIPE_MIN: f32 = 60.0;
const SWIPE_SHARE: f32 = 0.18;
/// Distance tirée vers le bas qui déclenche le chargement (points).
pub const PULL_DISTANCE: f32 = 80.0;

/// Suit l'appui principal : origine, dernière position.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Press {
    origin: Option<Pos2>,
    last: Option<Pos2>,
}

impl Press {
    /// Une image : bouton enfoncé ou non, position du pointeur. Rend
    /// `(origine, fin)` à l'image où l'appui se termine.
    pub fn step(&mut self, down: bool, pos: Option<Pos2>) -> Option<(Pos2, Pos2)> {
        if down {
            if self.origin.is_none() {
                self.origin = pos;
            }
            if pos.is_some() {
                self.last = pos;
            }
            return None;
        }
        let ended = self.origin.zip(pos.or(self.last));
        *self = Press::default();
        ended
    }

    /// Appui en cours : origine et déplacement depuis elle.
    pub fn current(&self) -> Option<(Pos2, egui::Vec2)> {
        let o = self.origin?;
        Some((o, self.last.unwrap_or(o) - o))
    }
}

/// Un appui terminé de `from` à `to` était-il un glissement horizontal ?
pub fn swipe(from: Pos2, to: Pos2, width: f32) -> Option<Swipe> {
    let d = to - from;
    let need = SWIPE_MIN.max(width * SWIPE_SHARE);
    if d.x.abs() < need || d.x.abs() < 2.0 * d.y.abs() {
        return None;
    }
    Some(if d.x > 0.0 { Swipe::Right } else { Swipe::Left })
}

/// « Tirer pour charger » : l'appui doit commencer liste en haut, puis
/// descendre de [`PULL_DISTANCE`].
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Pull {
    /// Appui commencé alors que la liste était tout en haut.
    armed: Option<bool>,
}

/// Ce que montre l'indicateur, et si le chargement part à cette image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PullView {
    Idle,
    /// Part de la distance parcourue (0 à 1).
    Pulling(f32),
    Fire,
}

impl Pull {
    /// `pulled` : déplacement vertical de l'appui en cours (`None` : aucun
    /// appui) ; `ended` : déplacement final, à l'image où il se termine.
    pub fn step(&mut self, at_top: bool, pulled: Option<f32>, ended: Option<f32>) -> PullView {
        if let Some(dy) = ended {
            let armed = self.armed.take() == Some(true);
            return if armed && dy >= PULL_DISTANCE { PullView::Fire } else { PullView::Idle };
        }
        let Some(dy) = pulled else {
            self.armed = None;
            return PullView::Idle;
        };
        let armed = *self.armed.get_or_insert(at_top);
        if armed && dy > 0.0 { PullView::Pulling((dy / PULL_DISTANCE).min(1.0)) } else { PullView::Idle }
    }
}

/// Doigt simulé à la souris : le bouton principal devient aussi un toucher
/// (ce que fait egui-winit pour un vrai écran tactile, dans l'autre sens).
#[derive(Clone, Copy, Debug, Default)]
pub struct TouchEmu {
    down: bool,
    /// Un vrai écran tactile a parlé : plus rien n'est simulé.
    real: bool,
}

const EMU_DEVICE: egui::TouchDeviceId = egui::TouchDeviceId(0xF0_C0);

impl TouchEmu {
    pub fn feed(&mut self, raw: &mut egui::RawInput) {
        if self.real || raw.events.iter().any(|e| matches!(e, egui::Event::Touch { device_id, .. } if *device_id != EMU_DEVICE)) {
            self.real = true;
            return;
        }
        let mut extra = Vec::new();
        for e in &raw.events {
            let (phase, pos) = match *e {
                egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, .. } => {
                    if pressed == self.down {
                        continue;
                    }
                    self.down = pressed;
                    (if pressed { egui::TouchPhase::Start } else { egui::TouchPhase::End }, pos)
                }
                egui::Event::PointerMoved(pos) if self.down => (egui::TouchPhase::Move, pos),
                egui::Event::PointerGone if self.down => {
                    self.down = false;
                    (egui::TouchPhase::Cancel, Pos2::ZERO)
                }
                _ => continue,
            };
            extra.push(egui::Event::Touch { device_id: EMU_DEVICE, id: egui::TouchId(0), phase, pos, force: None });
        }
        raw.events.extend(extra);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::pos2;

    #[test]
    fn press_reports_its_end_once() {
        let mut p = Press::default();
        assert_eq!(p.step(false, Some(pos2(1.0, 1.0))), None);
        assert_eq!(p.step(true, Some(pos2(10.0, 10.0))), None);
        assert_eq!(p.step(true, Some(pos2(50.0, 12.0))), None);
        assert_eq!(p.current(), Some((pos2(10.0, 10.0), egui::vec2(40.0, 2.0))));
        // Pointeur parti au relâchement : la dernière position connue.
        assert_eq!(p.step(false, None), Some((pos2(10.0, 10.0), pos2(50.0, 12.0))));
        assert_eq!(p.step(false, None), None);
    }

    #[test]
    fn swipe_needs_a_long_mostly_horizontal_move() {
        let o = pos2(100.0, 400.0);
        assert_eq!(swipe(o, pos2(300.0, 420.0), 390.0), Some(Swipe::Right));
        assert_eq!(swipe(o, pos2(10.0, 390.0), 390.0), Some(Swipe::Left));
        assert_eq!(swipe(o, pos2(140.0, 400.0), 390.0), None, "trop court");
        assert_eq!(swipe(o, pos2(250.0, 500.0), 390.0), None, "trop vertical");
        assert_eq!(swipe(o, pos2(170.0, 400.0), 1000.0), None, "fenêtre large : course plus longue");
    }

    #[test]
    fn pull_fires_only_from_the_top() {
        let mut p = Pull::default();
        assert_eq!(p.step(true, Some(0.0), None), PullView::Idle);
        assert_eq!(p.step(false, Some(40.0), None), PullView::Pulling(0.5), "armé au début de l'appui");
        assert_eq!(p.step(false, Some(120.0), None), PullView::Pulling(1.0));
        assert_eq!(p.step(false, None, Some(120.0)), PullView::Fire);
        // Appui commencé ailleurs qu'en haut : rien.
        assert_eq!(p.step(false, Some(0.0), None), PullView::Idle);
        assert_eq!(p.step(true, Some(200.0), None), PullView::Idle);
        assert_eq!(p.step(true, None, Some(200.0)), PullView::Idle);
        // Pas assez tiré.
        p.step(true, Some(0.0), None);
        assert_eq!(p.step(true, None, Some(30.0)), PullView::Idle);
    }

    #[test]
    fn mouse_becomes_a_touch_until_a_real_screen_speaks() {
        let mut emu = TouchEmu::default();
        let press = |pressed| egui::Event::PointerButton { pos: pos2(5.0, 5.0), button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let mut raw = egui::RawInput { events: vec![egui::Event::PointerMoved(pos2(5.0, 5.0)), press(true), egui::Event::PointerMoved(pos2(6.0, 5.0)), press(false)], ..Default::default() };
        emu.feed(&mut raw);
        let phases: Vec<_> = raw.events.iter().filter_map(|e| if let egui::Event::Touch { phase, .. } = e { Some(*phase) } else { None }).collect();
        assert_eq!(phases, [egui::TouchPhase::Start, egui::TouchPhase::Move, egui::TouchPhase::End]);
        let real = egui::Event::Touch { device_id: egui::TouchDeviceId(1), id: egui::TouchId(3), phase: egui::TouchPhase::Start, pos: pos2(1.0, 1.0), force: None };
        let mut raw = egui::RawInput { events: vec![real, press(true)], ..Default::default() };
        emu.feed(&mut raw);
        assert_eq!(raw.events.len(), 2, "vrai écran tactile : rien n'est ajouté");
    }
}
