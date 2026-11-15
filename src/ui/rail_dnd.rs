//! Glisser-déposer du rail (lot S7) : un serveur ou un dossier saisi suit la
//! souris, un repère montre où il tombera, et rien n'est écrit avant que le
//! bouton soit relâché (Échap annule). Le dépôt devient une modification
//! des dossiers (`state::guild_folders::Edit`), écrite par le magasin.

use crate::state::guild_folders::{Edit, Target};
use crate::theme::Tokens;
use eframe::egui::{self, Pos2, Rect, Stroke};

/// Ce qu'on fait glisser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Dragged {
    Guild(u64),
    /// Entrée (dossier) de la liste complète.
    Folder(usize),
}

/// Place d'un élément affiché, dans la liste complète des dossiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Spot {
    /// Au premier niveau : un serveur seul (`guild`), ou un dossier (replié,
    /// ou l'en-tête d'un dossier déplié : `open`).
    Top { entry: usize, guild: Option<u64>, open: bool },
    /// Serveur `guild` à la place `at` du dossier `entry` déplié.
    Inside { entry: usize, at: usize, guild: u64 },
}

/// Où tomberait l'objet, et comment le montrer.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Mark {
    /// Trait horizontal à cette hauteur.
    Line { y: f32, x: (f32, f32) },
    /// Contour de cet élément (regroupement).
    Ring(Rect),
}

/// Fraction de la hauteur, en haut et en bas d'un serveur, qui insère
/// avant ou après ; le milieu regroupe.
const EDGE: f32 = 0.25;

/// Dépôt de `dragged` au point `pos`, parmi les éléments `hits` (dans
/// l'ordre d'affichage) ; `entries` : taille de la liste complète.
pub(super) fn resolve(dragged: Dragged, pos: Pos2, hits: &[(Rect, Spot)], entries: usize) -> Option<(Edit, Mark)> {
    let line = |r: &Rect, top: bool| Mark::Line { y: if top { r.top() } else { r.bottom() }, x: (r.left(), r.right()) };
    let Some((rect, spot)) = hits.iter().find(|(r, _)| r.y_range().contains(pos.y)) else {
        // Sous le dernier élément : à la fin du premier niveau.
        let last = hits.last()?;
        if pos.y < last.0.bottom() {
            return None;
        }
        let edit = match dragged {
            Dragged::Guild(guild) => Edit::MoveGuild { guild, to: Target::Before(entries) },
            Dragged::Folder(entry) => Edit::MoveFolder { entry, before: entries },
        };
        return Some((edit, line(&last.0, false)));
    };
    let f = ((pos.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
    match (dragged, *spot) {
        (Dragged::Folder(from), Spot::Top { entry, .. } | Spot::Inside { entry, .. }) => {
            let before = if f < 0.5 { entry } else { entry + 1 };
            let r = if f < 0.5 { line(rect, true) } else { line(rect, false) };
            (from != entry).then_some((Edit::MoveFolder { entry: from, before }, r))
        }
        (Dragged::Guild(guild), Spot::Top { entry, guild: other, open }) => {
            if f < EDGE {
                Some((Edit::MoveGuild { guild, to: Target::Before(entry) }, line(rect, true)))
            } else if f > 1.0 - EDGE {
                // Sous l'en-tête d'un dossier déplié : en tête du dossier.
                let to = if open { Target::InFolder { entry, at: 0 } } else { Target::Before(entry + 1) };
                Some((Edit::MoveGuild { guild, to }, line(rect, false)))
            } else if other == Some(guild) {
                None
            } else {
                Some((Edit::MoveGuild { guild, to: Target::Combine(entry) }, Mark::Ring(*rect)))
            }
        }
        (Dragged::Guild(guild), Spot::Inside { entry, at, .. }) => {
            let (at, mark) = if f < 0.5 { (at, line(rect, true)) } else { (at + 1, line(rect, false)) };
            Some((Edit::MoveGuild { guild, to: Target::InFolder { entry, at } }, mark))
        }
    }
}

/// Glisser-déposer en cours.
#[derive(Default)]
pub(super) struct RailDnd {
    dragging: Option<Dragged>,
    /// Dépôt prévu à la dernière image.
    target: Option<(Edit, Mark)>,
}

impl RailDnd {
    /// Saisie d'un élément (le glissement a commencé sur lui).
    pub(super) fn start(&mut self, d: Dragged) {
        self.dragging = Some(d);
        self.target = None;
    }

    /// À la fin de l'image du rail : repère dessiné, et la modification si
    /// le bouton vient d'être relâché au-dessus d'une place valable.
    pub(super) fn finish(&mut self, ui: &egui::Ui, t: &Tokens, hits: &[(Rect, Spot)], entries: usize) -> Option<Edit> {
        let dragged = self.dragging?;
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            *self = RailDnd::default();
            return None;
        }
        let (pos, down) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.any_down()));
        if let Some(p) = pos {
            self.target = resolve(dragged, p, hits, entries);
        }
        if !down {
            let edit = self.target.take().map(|(e, _)| e);
            self.dragging = None;
            return edit;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let stroke = Stroke::new(t.shapes.unread_ring, t.colors.accent);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("depot")));
        match self.target {
            Some((_, Mark::Line { y, x })) => {
                painter.line_segment([egui::pos2(x.0, y), egui::pos2(x.1, y)], stroke);
            }
            Some((_, Mark::Ring(r))) => {
                painter.rect_stroke(r, t.r_rail(true), stroke, egui::StrokeKind::Outside);
            }
            None => {}
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    fn rect(i: usize) -> Rect {
        Rect::from_min_size(pos2(0.0, i as f32 * 50.0), vec2(48.0, 48.0))
    }

    /// Rail : serveur 4 seul, dossier 1 déplié (1, 2), serveur 3 seul.
    fn hits() -> Vec<(Rect, Spot)> {
        vec![
            (rect(0), Spot::Top { entry: 0, guild: Some(4), open: false }),
            (rect(1), Spot::Top { entry: 1, guild: None, open: true }),
            (rect(2), Spot::Inside { entry: 1, at: 0, guild: 1 }),
            (rect(3), Spot::Inside { entry: 1, at: 1, guild: 2 }),
            (rect(4), Spot::Top { entry: 2, guild: Some(3), open: false }),
        ]
    }

    fn edit(d: Dragged, y: f32) -> Option<Edit> {
        resolve(d, pos2(10.0, y), &hits(), 3).map(|(e, _)| e)
    }

    #[test]
    fn guild_drops() {
        let g = Dragged::Guild(4);
        assert_eq!(edit(g, 205.0), Some(Edit::MoveGuild { guild: 4, to: Target::Before(2) }), "haut du serveur 3");
        assert_eq!(edit(g, 224.0), Some(Edit::MoveGuild { guild: 4, to: Target::Combine(2) }), "milieu : dossier");
        assert_eq!(edit(g, 246.0), Some(Edit::MoveGuild { guild: 4, to: Target::Before(3) }));
        assert_eq!(edit(g, 20.0), None, "sur lui-même");
        assert_eq!(edit(g, 70.0), Some(Edit::MoveGuild { guild: 4, to: Target::Combine(1) }), "sur l'en-tête : dans le dossier");
        assert_eq!(edit(g, 95.0), Some(Edit::MoveGuild { guild: 4, to: Target::InFolder { entry: 1, at: 0 } }));
        assert_eq!(edit(g, 160.0), Some(Edit::MoveGuild { guild: 4, to: Target::InFolder { entry: 1, at: 1 } }));
        assert_eq!(edit(g, 190.0), Some(Edit::MoveGuild { guild: 4, to: Target::InFolder { entry: 1, at: 2 } }));
        assert_eq!(edit(g, 400.0), Some(Edit::MoveGuild { guild: 4, to: Target::Before(3) }), "sous tout : à la fin");
        assert_eq!(edit(g, 49.0), None, "entre deux éléments");
        assert!(matches!(resolve(g, pos2(10.0, 224.0), &hits(), 3), Some((_, Mark::Ring(_)))));
    }

    #[test]
    fn folder_drops() {
        let f = Dragged::Folder(1);
        assert_eq!(edit(f, 5.0), Some(Edit::MoveFolder { entry: 1, before: 0 }));
        assert_eq!(edit(f, 240.0), Some(Edit::MoveFolder { entry: 1, before: 3 }));
        assert_eq!(edit(f, 120.0), None, "sur lui-même");
    }
}
