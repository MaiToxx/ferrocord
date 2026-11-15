//! Emojis et autocollants en image, partout où du texte en contient
//! (messages, réactions, saisie, statut, noms de salons).
//!
//! Mise en page : l'emoji reste dans le texte (copier un passage le
//! recopie), transparent, dans une police dont la taille est choisie pour
//! qu'il occupe exactement la place de l'image ; sa hauteur de ligne est
//! celle du texte (l'interligne ne change pas). Après la mise en page, la
//! place de chaque emoji est retrouvée par ses caractères (un glyphe par
//! caractère, garanti par egui).
//!
//! Dessin différé : qui dessine un emoji n'a pas besoin du cache d'images.
//! [`paint`] réserve une forme vide à sa place dans la couche courante et
//! note la demande ; [`flush`], appelé une fois en fin d'image par `App`,
//! demande les images (seulement celles qui sont visibles) et remplit les
//! formes réservées. Une image pas encore chargée laisse la place vide ;
//! son arrivée réveille l'interface (cache d'images).

use crate::images::{Image, Images};
use crate::state::emoji::Unicode;
use crate::state::model::ImageRef;
use crate::theme::Tokens;
use eframe::egui::{self, layers::ShapeIdx, text::LayoutJob, Color32, FontId, Id, LayerId, Rect};
use std::ops::Range;

/// Image d'emoji ou d'autocollant.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Img {
    Unicode(Unicode),
    /// Emoji personnalisé fixe.
    Custom(u64),
    /// Emoji personnalisé animé (joué selon `motion`).
    Animated(u64),
    /// `anim` : APNG ou GIF, joué selon `motion`.
    Sticker { id: u64, gif: bool, anim: bool },
}

impl Img {
    fn image(self) -> ImageRef {
        match self {
            Img::Unicode(u) => ImageRef::Twemoji(u),
            Img::Custom(id) | Img::Animated(id) => ImageRef::CustomEmoji { id },
            Img::Sticker { id, gif, .. } => ImageRef::Sticker { id, gif },
        }
    }

    pub(super) fn custom(id: u64, animated: bool) -> Img {
        if animated { Img::Animated(id) } else { Img::Custom(id) }
    }

    /// Image d'une réaction ou d'un emoji du modèle ; un emoji Unicode
    /// inconnu de la table n'en a pas.
    pub(super) fn of(e: &crate::state::social::Emoji) -> Option<Img> {
        match e {
            crate::state::social::Emoji::Unicode(s) => crate::state::emoji::find(s).map(Img::Unicode),
            crate::state::social::Emoji::Custom { id, animated, .. } => Some(Img::custom(*id, *animated)),
        }
    }
}

#[derive(Clone, Copy)]
struct Pending {
    layer: LayerId,
    clip: Rect,
    idx: ShapeIdx,
    rect: Rect,
    img: Img,
    /// Sous le pointeur (au moment du dessin).
    hover: bool,
    /// Opacité (1 : l'image telle quelle).
    alpha: f32,
}

/// Demandes de l'image en cours. Bornée : au-delà (ce qui n'arrive pas avec
/// des listes virtualisées), les suivantes ne sont pas dessinées.
#[derive(Clone, Default)]
struct Queue(Vec<Pending>);

const MAX_PENDING: usize = 4096;

fn queue_id() -> Id {
    Id::unique("ferrocord-emojis-a-dessiner")
}

/// Dessine `img` dans `rect` (en fin d'image), s'il est visible.
pub(super) fn paint(ui: &egui::Ui, rect: Rect, img: Img) {
    paint_faded(ui, rect, img, 1.0);
}

/// Comme [`paint`], à l'opacité `alpha` (emoji qui apparaît en fondu).
pub(super) fn paint_faded(ui: &egui::Ui, rect: Rect, img: Img, alpha: f32) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let painter = ui.painter();
    let idx = painter.add(egui::Shape::Noop);
    let hover = matches!(img, Img::Animated(_) | Img::Sticker { anim: true, .. }) && super::motion::hovered(ui, rect);
    let p = Pending { layer: painter.layer_id(), clip: painter.clip_rect(), idx, rect, img, hover, alpha };
    ui.ctx().data_mut(|d| {
        let q = d.get_temp_mut_or_default::<Queue>(queue_id());
        if q.0.len() < MAX_PENDING {
            q.0.push(p);
        }
    });
}

/// Remplit les places réservées par [`paint`] à cette image.
pub(super) fn flush(ctx: &egui::Context, images: &Images, t: &Tokens) {
    let Some(q) = ctx.data_mut(|d| d.remove_temp::<Queue>(queue_id())) else { return };
    let m = super::motion::get(ctx);
    for p in q.0 {
        let image = match p.img {
            Img::Unicode(_) => images.get(p.img.image(), p.rect.width().max(p.rect.height())),
            Img::Animated(_) => images.get_within_moving(p.img.image(), p.rect.width(), p.rect.height(), m.emoji.now(p.hover)),
            Img::Sticker { anim: true, .. } => images.get_within_moving(p.img.image(), p.rect.width(), p.rect.height(), m.stickers.now(p.hover)),
            _ => images.get_within(p.img.image(), p.rect.width(), p.rect.height()),
        };
        let shape = match image {
            Image::Ready(tex) if p.alpha < 1.0 => {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                egui::Shape::Rect(egui::epaint::RectShape::filled(fitted(ctx, tex, p.rect), egui::CornerRadius::ZERO, Color32::WHITE.gamma_multiply(p.alpha)).with_texture(tex, uv))
            }
            Image::Ready(tex) => crate::images::textured(fitted(ctx, tex, p.rect), egui::CornerRadius::ZERO, tex),
            Image::Pending => continue,
            // Indisponible : un cadre discret à sa place, le texte reste copiable.
            Image::Failed => egui::Shape::rect_stroke(p.rect.shrink(p.rect.width() * 0.15), t.r_row(), egui::Stroke::new(t.shapes.border.max(1.0), t.colors.faint), egui::StrokeKind::Inside),
        };
        ctx.layer_painter(p.layer).with_clip_rect(p.clip).set(p.idx, shape);
    }
}

/// Rectangle de la texture `tex` dans `rect`, proportions gardées, centré.
fn fitted(ctx: &egui::Context, tex: egui::TextureId, rect: Rect) -> Rect {
    let size = ctx.tex_manager().read().meta(tex).map(|m| m.size);
    let Some([w, h]) = size.filter(|[w, h]| *w > 0 && *h > 0) else { return rect };
    let (w, h) = (w as f32, h as f32);
    let scale = (rect.width() / w).min(rect.height() / h);
    Rect::from_center_size(rect.center(), egui::vec2(w * scale, h * scale))
}

/// Place d'un emoji dans une mise en page.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Spot {
    /// Caractères (pas octets) du texte qui le portent.
    pub chars: Range<usize>,
    pub img: Img,
    pub side: f32,
    /// Premier caractère : une mise en page coupée (« … ») ne le montre plus.
    first: char,
}

/// Taille de police (pour `base.family`) qui donne à `text` la largeur `side`.
fn sized(ui: &egui::Ui, text: &str, base: &FontId, side: f32) -> FontId {
    let probe = FontId::new(base.size.max(1.0), base.family.clone());
    // Largeur réelle (mise en page, liaisons et glyphes de remplacement compris).
    let width = ui.fonts_mut(|f| f.layout_no_wrap(text.to_string(), probe.clone(), Color32::WHITE).size().x);
    let size = if width > 0.01 { probe.size * side / width } else { side };
    // Tailles arrondies au quart de point : peu de tailles différentes dans l'atlas.
    let size = ((size * 4.0).round() / 4.0).clamp(0.25, side * 8.0);
    FontId::new(size, base.family.clone())
}

/// Ajoute à `job` un emoji (`text`, déjà dans le texte) occupant `side`
/// points de large, sur une ligne de `row` points ; `chars` : nombre de
/// caractères déjà dans `job`. Renvoie sa place.
#[allow(clippy::too_many_arguments)]
pub(super) fn append(ui: &egui::Ui, job: &mut LayoutJob, chars: usize, text: &str, base: &FontId, side: f32, row: f32, img: Img) -> Spot {
    let format = egui::TextFormat { font_id: sized(ui, text, base, side), color: Color32::TRANSPARENT, line_height: Some(row), valign: egui::Align::Center, ..Default::default() };
    job.append(text, 0.0, format);
    Spot { chars: chars..chars + text.chars().count(), img, side, first: text.chars().next().unwrap_or(' ') }
}

/// Hauteur d'une ligne de texte dans cette police.
pub(super) fn row_height(ui: &egui::Ui, font: &FontId) -> f32 {
    ui.fonts_mut(|f| f.row_height(font))
}

/// Dessine les emojis `spots` d'une mise en page `galley` posée en `pos`.
pub(super) fn paint_spots(ui: &egui::Ui, pos: egui::Pos2, galley: &egui::Galley, spots: &[Spot]) {
    if spots.is_empty() {
        return;
    }
    let mut c = 0;
    let mut k = 0;
    for row in &galley.rows {
        for g in &row.glyphs {
            while k < spots.len() && spots[k].chars.end <= c {
                k += 1;
            }
            if let Some(s) = spots.get(k)
                && s.chars.start == c
                && g.chr == s.first
            {
                let top = pos + row.pos.to_vec2();
                let center = egui::pos2(top.x + g.pos.x + s.side / 2.0, top.y + row.size.y / 2.0);
                paint(ui, Rect::from_center_size(center, egui::vec2(s.side, s.side)), s.img);
            }
            c += 1;
        }
        if row.ends_with_newline {
            c += 1;
        }
    }
}

/// Mise en page d'un texte d'affichage (statut, nom de salon) : emojis
/// Unicode reconnus en image de la hauteur du texte.
pub(super) fn job(ui: &egui::Ui, text: &str, font: &FontId, color: Color32) -> (LayoutJob, Vec<Spot>) {
    let mut job = LayoutJob::default();
    let mut spots = Vec::new();
    let row = row_height(ui, font);
    let side = row.min(font.size * 1.3);
    let mut chars = 0;
    for (r, u) in crate::state::emoji::split(text) {
        let piece = &text[r];
        match u {
            Some(u) => spots.push(append(ui, &mut job, chars, piece, font, side, row, Img::Unicode(u))),
            None => job.append(piece, 0.0, egui::TextFormat { font_id: font.clone(), color, ..Default::default() }),
        }
        chars += piece.chars().count();
    }
    super::fonts::route_marks(ui, &mut job);
    (job, spots)
}

/// Mise en page sur une ligne, coupée à `width` (« … »), emojis en image ;
/// à dessiner avec [paint_spots] puis le texte.
pub(super) fn truncated(ui: &egui::Ui, text: &str, font: &FontId, color: Color32, width: f32) -> (std::sync::Arc<egui::Galley>, Vec<Spot>) {
    let (mut job, spots) = job(ui, text, font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    (ui.fonts_mut(|f| f.layout_job(job)), spots)
}

/// Étiquette dont les emojis sont des images ; `truncate` : coupée sur une ligne.
pub(super) fn label(ui: &mut egui::Ui, text: &str, font: &FontId, color: Color32, truncate: bool) -> egui::Response {
    let (job, spots) = job(ui, text, font, color);
    let label = egui::Label::new(job);
    let label = if truncate { label.truncate() } else { label.wrap() };
    let (pos, galley, resp) = label.layout_in_ui(ui);
    // Coupée : les places au-delà n'existent plus dans la mise en page.
    paint_spots(ui, pos, &galley, &spots);
    ui.painter().galley(pos, galley, color);
    resp
}

/// Remplace, dans un texte déjà mis en forme (nom en couleur ou en dégradé),
/// les emojis Unicode reconnus par leur image Twemoji, comme dans les
/// messages. Sans emoji (le cas courant) : `job` est rendu tel quel.
pub(super) fn imaged(ui: &egui::Ui, job: LayoutJob) -> (LayoutJob, Vec<Spot>) {
    if job.text.is_ascii() {
        return (job, Vec::new());
    }
    let parts = crate::state::emoji::split(&job.text);
    if parts.iter().all(|(_, u)| u.is_none()) {
        return (job, Vec::new());
    }
    let mut out = LayoutJob { text: String::new(), sections: Vec::new(), ..job.clone() };
    let mut spots = Vec::new();
    let mut chars = 0;
    for (range, u) in parts {
        let piece = &job.text[range.clone()];
        match u {
            Some(u) => {
                let font = job.sections.iter().find(|s| (s.byte_range.start.0..s.byte_range.end.0).contains(&range.start)).map_or_else(FontId::default, |s| s.format.font_id.clone());
                let row = row_height(ui, &font);
                spots.push(append(ui, &mut out, chars, piece, &font, row.min(font.size * 1.3), row, Img::Unicode(u)));
            }
            None => {
                // Chaque morceau garde la mise en forme de sa section (couleur du dégradé).
                for s in job.sections.iter().filter(|s| s.byte_range.start.0 < range.end && s.byte_range.end.0 > range.start) {
                    let (from, to) = (s.byte_range.start.0.max(range.start), s.byte_range.end.0.min(range.end));
                    out.append(&job.text[from..to], 0.0, s.format.clone());
                }
            }
        }
        chars += piece.chars().count();
    }
    (out, spots)
}

/// Étiquette d'un nom déjà mis en forme, ses emojis en image.
pub(super) fn label_job(ui: &mut egui::Ui, job: LayoutJob, truncate: bool) -> egui::Response {
    let (job, spots) = imaged(ui, job);
    let label = egui::Label::new(job).selectable(false);
    let label = if truncate { label.truncate() } else { label };
    let (pos, galley, resp) = label.layout_in_ui(ui);
    if ui.is_rect_visible(resp.rect) {
        paint_spots(ui, pos, &galley, &spots);
        ui.painter().galley(pos, galley, ui.visuals().text_color());
    }
    resp
}

/// Mise en page sur une ligne d'un nom déjà mis en forme, coupée à `width`,
/// ses emojis en image ; à dessiner avec [paint_spots] puis le texte.
pub(super) fn name_galley(ui: &egui::Ui, text: egui::WidgetText, width: f32) -> (std::sync::Arc<egui::Galley>, Vec<Spot>) {
    let job = text.into_layout_job(ui.style(), egui::FontSelection::Default, egui::Align::Center);
    let (mut job, spots) = imaged(ui, (*job).clone());
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    (ui.fonts_mut(|f| f.layout_job(job)), spots)
}

/// Image seule d'un emoji, dans un carré de `side` points (réactions, sélecteur).
pub(super) fn icon(ui: &mut egui::Ui, img: Img, side: f32, sense: egui::Sense) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), sense);
    paint(ui, rect, img);
    resp
}

/// Images demandées depuis le dernier appel (tests).
#[cfg(test)]
pub(super) fn take_queued(ctx: &egui::Context) -> Vec<Img> {
    ctx.data_mut(|d| d.remove_temp::<Queue>(queue_id())).map_or_else(Vec::new, |q| q.0.into_iter().map(|p| p.img).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::emoji::by_name;

    fn run(mut f: impl FnMut(&mut egui::Ui)) -> (egui::Context, Vec<egui::epaint::ClippedShape>) {
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, &mut f);
            out.textures_delta.clear();
            shapes = std::mem::take(&mut out.shapes);
        }
        (ctx, shapes)
    }

    /// Chaque emoji prend la largeur demandée et sa place est retrouvée ;
    /// rien n'est demandé pour ce qui est invisible.
    #[test]
    fn spots_have_requested_width_and_are_queued() {
        let fire = by_name("fire").unwrap_or_else(|| panic!("fire"));
        let mut widths = Vec::new();
        let mut queued = 0;
        run(|ui| {
            let font = FontId::proportional(14.0);
            let (job, spots) = job(ui, "a 🔥 b :x: 👍🏽", &font, Color32::WHITE);
            assert_eq!(spots.len(), 2);
            assert_eq!(spots[0].img, Img::Unicode(fire));
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            widths = galley.rows[0].glyphs.iter().map(|g| g.advance_width).collect();
            paint_spots(ui, egui::Pos2::ZERO, &galley, &spots);
            queued = ui.ctx().data_mut(|d| d.remove_temp::<Queue>(queue_id())).map_or(0, |q| q.0.len());
            let mut child = ui.new_child(egui::UiBuilder::new().invisible());
            paint_spots(&child, egui::Pos2::ZERO, &galley, &spots);
            child.label("");
            assert!(ui.ctx().data_mut(|d| d.remove_temp::<Queue>(queue_id())).is_none());
            let side = spots[0].side;
            // « 🔥 » est le 3e glyphe : sa largeur est celle de l'image (à l'arrondi de taille près).
            assert!((widths[2] - side).abs() < side * 0.1, "{widths:?} {side}");
        });
        assert_eq!(queued, 2);
    }
}
