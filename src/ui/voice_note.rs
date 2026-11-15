//! Carte d'un message vocal reçu (phase 6) : bouton lecture ou pause, onde
//! (`waveform`) qui se colore à mesure de la lecture, durée. Dessinée comme
//! les autres pièces jointes, sans accès à l'application : l'état de la
//! lecture arrive par la mémoire d'egui ([`NoteView`], posé à chaque image
//! par `ui::voice::notes`), le clic repart de même ([`NoteClick`]).

use super::attachments::{Action, Hot};
use crate::i18n::{table::voice_msg::t as mt, Lang};
use crate::state::model::Attachment;
use crate::state::voice_msg::VoiceNote;
use crate::theme::Tokens;
use eframe::egui::{self, pos2, Id, Rect, Sense, Shape, Stroke, Vec2};

/// Message vocal en cours (lecture ou pause), vu par les cartes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoteView {
    pub id: u64,
    pub pos_ms: u32,
    pub playing: bool,
}

/// Clic sur le bouton d'un message vocal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NoteClick {
    pub id: u64,
    pub url: String,
    pub duration_ms: u32,
}

fn view_id() -> Id {
    Id::unique("ferrocord-message-vocal-vue")
}

fn click_id() -> Id {
    Id::unique("ferrocord-message-vocal-clic")
}

pub fn set_view(ctx: &egui::Context, v: Option<NoteView>) {
    ctx.data_mut(|d| match v {
        Some(v) => {
            d.insert_temp(view_id(), v);
        }
        None => {
            d.remove::<NoteView>(view_id());
        }
    });
}

fn view(ctx: &egui::Context) -> Option<NoteView> {
    ctx.data(|d| d.get_temp::<NoteView>(view_id()))
}

pub(super) fn clicked(ctx: &egui::Context, a: &Attachment) {
    let duration_ms = a.voice.as_ref().map_or(0, |v| v.duration_ms);
    ctx.data_mut(|d| d.insert_temp(click_id(), NoteClick { id: a.id, url: a.url.clone(), duration_ms }));
}

pub fn take_click(ctx: &egui::Context) -> Option<NoteClick> {
    ctx.data_mut(|d| {
        let c = d.get_temp::<NoteClick>(click_id());
        d.remove::<NoteClick>(click_id());
        c
    })
}

/// « 1:05 ».
pub fn clock(ms: u32) -> String {
    let s = ms / 1000;
    format!("{}:{:02}", s / 60, s % 60)
}

/// Hauteur de chaque barre (0 à 1) pour `bars` barres : le point le plus
/// fort de chaque tranche ; une onde vide donne des barres basses.
pub fn bars(wave: &[u8], bars: usize) -> Vec<f32> {
    (0..bars)
        .map(|i| {
            if wave.is_empty() {
                return 0.0;
            }
            let a = i * wave.len() / bars;
            let b = ((i + 1) * wave.len() / bars).max(a + 1).min(wave.len());
            f32::from(wave[a..b].iter().copied().max().unwrap_or(0)) / 255.0
        })
        .collect()
}

/// Bouton rond : triangle (lecture) ou deux barres (pause).
fn paint_button(p: &egui::Painter, rect: Rect, playing: bool, t: &Tokens) {
    let c = rect.center();
    let r = rect.width() / 2.0;
    p.circle_filled(c, r, t.colors.button_bg);
    let u = r * 0.4;
    if playing {
        let w = u * 0.55;
        for dx in [-u * 0.6, u * 0.6] {
            p.rect_filled(Rect::from_center_size(pos2(c.x + dx, c.y), Vec2::new(w, 2.0 * u)), 0.0, t.colors.button_text);
        }
    } else {
        let pts = vec![pos2(c.x - u * 0.7, c.y - u), pos2(c.x + u, c.y), pos2(c.x - u * 0.7, c.y + u)];
        p.add(Shape::convex_polygon(pts, t.colors.button_text, Stroke::NONE));
    }
}

/// Carte d'un message vocal (même hauteur avant et après chargement).
/// `live` : faux pendant la mesure (rien de cliquable).
pub(super) fn voice_card<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, live: bool, a: &'a Attachment, note: &VoiceNote, hot: &mut Hot<'a>) {
    let current = view(ui.ctx()).filter(|v| v.id == a.id);
    let playing = current.is_some_and(|v| v.playing);
    let width = ui.available_width().min(t.layout.embed_width);
    let frame = t.card_frame();
    frame.show(ui, |ui| {
        let inner = (width - t.embed_margin().sum().x).max(1.0);
        let side = Tokens::icon_side(t.typography.body) * 2.0;
        let (row, _) = ui.allocate_exact_size(Vec2::new(inner, side), Sense::hover());
        let button = Rect::from_min_size(row.min, Vec2::splat(side));
        paint_button(ui.painter(), button, playing, t);
        let shown = current.map_or(note.duration_ms, |v| v.pos_ms);
        let font = egui::FontId::proportional(t.typography.small);
        let time = ui.painter().layout_no_wrap(clock(shown), font, t.colors.muted);
        let time_at = pos2(row.right() - time.size().x, row.center().y - time.size().y / 2.0);
        let wave = Rect::from_min_max(pos2(button.right() + t.spacing.item_x, row.top()), pos2(time_at.x - t.spacing.item_x, row.bottom()));
        // Barres à l'échelle du texte : une barre et un vide de même largeur.
        let bar_w = (t.typography.small / 6.0).max(1.0);
        let n = ((wave.width() / (2.0 * bar_w)).floor() as usize).max(1);
        let done = match current {
            Some(v) if note.duration_ms > 0 => (v.pos_ms as f32 / note.duration_ms as f32).clamp(0.0, 1.0),
            _ => 0.0,
        };
        for (i, h) in bars(&note.waveform, n).into_iter().enumerate() {
            let x = wave.left() + i as f32 * 2.0 * bar_w;
            if x + bar_w > wave.right() {
                break;
            }
            let height = (h * wave.height() * 0.8).max(bar_w);
            let color = if (i as f32 + 0.5) / n as f32 <= done { t.colors.link } else { t.colors.faint };
            let bar = Rect::from_center_size(pos2(x + bar_w / 2.0, wave.center().y), Vec2::new(bar_w, height));
            ui.painter().rect_filled(bar, bar_w / 2.0, color);
        }
        ui.painter().galley(time_at, time, t.colors.muted);
        if live {
            let tip = if playing { mt::PAUSE.get(lang) } else { mt::PLAY.get(lang) };
            hot.add(button, Action::Note(a), Some(tip));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_and_bars() {
        assert_eq!(clock(65_400), "1:05");
        assert_eq!(clock(0), "0:00");
        assert_eq!(bars(&[0, 255, 51, 102], 2), vec![1.0, 0.4]);
        assert_eq!(bars(&[255], 3), vec![1.0, 1.0, 1.0]);
        assert_eq!(bars(&[], 2), vec![0.0, 0.0]);
    }
}
