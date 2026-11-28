//! Une tuile de la vue d'appel (caméra ou avatar, nom, anneau de parole,
//! muet et sourdine, vignette « EN DIRECT ») et la disposition des tuiles.
//!
//! Textures : une par tuile affichée, mise à jour seulement quand
//! `VideoSink` a une image plus récente ; une tuile qui n'est plus dessinée
//! perd sa texture à la fin de l'image (`VideoUi::end_frame`).

use super::icons::{self, Glyph};
use crate::i18n::table::video::{p as vp, t as vt};
use crate::i18n::Lang;
use crate::images::Images;
use crate::state::model::Avatar;
use crate::state::video::{Tile, TileKind, VideoSink};
use crate::theme::Tokens;
use crate::ui::util::{image_pixels, paint_avatar, rgba_image};
use crate::ui::voice::icons::{self as vicons, VIcon};
use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect, Sense, Stroke, TextureHandle, TextureOptions, Vec2};
use std::collections::HashMap;
use std::sync::Arc;

/// Proportions d'une tuile.
pub(super) const RATIO: f32 = 16.0 / 9.0;

/// Ce qu'une tuile montre, lu du magasin avant le dessin.
#[derive(Clone, Debug)]
pub(super) struct TileView {
    pub tile: Tile,
    pub kind: TileKind,
    pub name: String,
    /// Nom de la personne, sans « Écran de » : ses initiales quand l'avatar n'a pas d'image.
    pub who: String,
    pub avatar: Avatar,
    pub speaking: bool,
    pub muted: bool,
    pub deafened: bool,
    pub viewers: usize,
    pub paused: bool,
    /// Direct regardé dont la connexion n'est pas encore établie.
    pub connecting: bool,
    /// Son d'un direct regardé (pas le mien) : sa clé dans les réglages par
    /// personne (identifiant de son auteur et bit `STREAM_AUDIO`), son volume
    /// et son muet. Réglé à part de la voix de la personne.
    pub sound: Option<(u64, crate::state::voice::UserVoice)>,
}

impl TileView {
    fn has_video(&self) -> bool {
        matches!(self.kind, TileKind::Person { camera: true } | TileKind::Stream { watching: true, .. })
    }
}

/// Geste fait sur une tuile, appliqué après le dessin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum TileAct {
    Focus(Tile),
    Unfocus,
    Fullscreen(Tile),
    PopOut(Tile),
    Watch(String),
    StopWatching(String),
    /// Volume du son d'un direct (pour cent) ; `done` : curseur relâché.
    Volume { user: u64, volume: u16, done: bool },
    /// Son d'un direct coupé pour moi.
    LocalMute(u64, bool),
}

/// Texture d'une tuile.
struct Shown {
    tex: TextureHandle,
    /// Génération de l'image téléversée.
    generation: u64,
    /// L'image téléversée, partagée avec egui jusqu'à ce qu'il l'ait passée à
    /// la carte graphique : à l'image suivante, son tampon retourne au
    /// convertisseur ([`VideoSink::give_back`]). Rien n'est alloué ni recopié
    /// par image sur le fil d'interface.
    last: Arc<egui::ColorImage>,
}

/// Textures des tuiles affichées et tuiles dessinées à cette image.
#[derive(Default)]
pub(super) struct Screens {
    textures: HashMap<Tile, Shown>,
    /// Tuiles à vidéo dessinées à cette image, taille en pixels physiques.
    pub drawn: Vec<(Tile, (u32, u32))>,
}

impl Screens {
    /// Texture de la tuile, mise à jour si une image plus récente attend.
    fn texture(&mut self, ctx: &egui::Context, sink: &VideoSink, tile: &Tile, rect: Rect) -> Option<(egui::TextureId, Vec2)> {
        let ppp = ctx.pixels_per_point();
        let px = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
        match self.drawn.iter_mut().find(|(t, _)| t == tile) {
            // Dessinée deux fois (grille sous le plein écran) : la plus grande taille compte.
            Some((_, s)) => *s = (s.0.max(px.0), s.1.max(px.1)),
            None => self.drawn.push((tile.clone(), px)),
        }
        let seen = self.textures.get(tile).map_or(0, |e| e.generation);
        if let Some((generation, img)) = sink.take(tile, seen) {
            let image = Arc::new(rgba_image(img.width, img.height, img.pixels));
            match self.textures.get_mut(tile) {
                Some(e) => {
                    e.tex.set(image.clone(), TextureOptions::LINEAR);
                    e.generation = generation;
                    // L'image d'avant est téléversée depuis au moins une image :
                    // si egui l'a lâchée, son tampon sert à la conversion suivante.
                    if let Ok(old) = Arc::try_unwrap(std::mem::replace(&mut e.last, image)) {
                        sink.give_back(tile, image_pixels(old));
                    }
                }
                None => {
                    let tex = ctx.load_texture("video", image.clone(), TextureOptions::LINEAR);
                    self.textures.insert(tile.clone(), Shown { tex, generation, last: image });
                }
            }
        }
        self.textures.get(tile).map(|e| (e.tex.id(), e.tex.size_vec2()))
    }

    /// Fin d'image : seules les tuiles dessinées gardent une texture et reçoivent des images.
    pub(super) fn end_frame(&mut self, sink: &VideoSink) {
        let drawn = std::mem::take(&mut self.drawn);
        self.textures.retain(|t, _| drawn.iter().any(|(d, _)| d == t));
        sink.show(&drawn);
    }

    #[cfg(test)]
    pub(super) fn textures(&self) -> usize {
        self.textures.len()
    }
}

/// Ce dont une tuile a besoin pour se dessiner.
pub(super) struct Cx<'a> {
    pub t: &'a Tokens,
    pub lang: Lang,
    pub images: &'a Images,
    pub sink: &'a VideoSink,
    pub screens: &'a mut Screens,
}

/// Rectangle de proportions `size` centré dans `area`, aussi grand que possible.
fn contain(area: Rect, size: Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return area;
    }
    let s = (area.width() / size.x).min(area.height() / size.y);
    Rect::from_center_size(area.center(), size * s)
}

/// Grille de `n` tuiles 16:9 dans `area` : le nombre de colonnes qui donne
/// les plus grandes tuiles, l'ensemble centré.
pub(super) fn grid(area: Rect, n: usize, gap: f32) -> Vec<Rect> {
    if n == 0 || area.width() <= 0.0 || area.height() <= 0.0 {
        return Vec::new();
    }
    let mut best = (0.0f32, 1usize);
    for cols in 1..=n {
        let rows = n.div_ceil(cols);
        let w = ((area.width() - gap * (cols - 1) as f32) / cols as f32).min((area.height() - gap * (rows - 1) as f32) / rows as f32 * RATIO);
        if w > best.0 {
            best = (w, cols);
        }
    }
    let (w, cols) = (best.0.max(1.0), best.1);
    let h = w / RATIO;
    let rows = n.div_ceil(cols);
    let total_h = h * rows as f32 + gap * (rows - 1) as f32;
    let top = area.top() + (area.height() - total_h) / 2.0;
    (0..n)
        .map(|i| {
            let (r, c) = (i / cols, i % cols);
            // Dernière rangée incomplète : centrée elle aussi.
            let in_row = if r == rows - 1 { n - r * cols } else { cols };
            let row_w = w * in_row as f32 + gap * (in_row - 1) as f32;
            let left = area.left() + (area.width() - row_w) / 2.0;
            Rect::from_min_size(pos2(left + c as f32 * (w + gap), top + r as f32 * (h + gap)), vec2(w, h))
        })
        .collect()
}

/// Petit bouton-icône posé sur une tuile.
fn overlay_button(ui: &mut egui::Ui, t: &Tokens, rect: Rect, id: egui::Id, tip: &str, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> bool {
    let r = ui.interact(rect, id, Sense::click());
    let fill = if r.hovered() { t.media_veil().to_opaque() } else { t.media_veil() };
    ui.painter().rect_filled(rect, t.r_row(), fill);
    paint(ui.painter(), rect.shrink(rect.width() * 0.2), t.on_media());
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    r.on_hover_text(tip).clicked()
}

/// Dessine une tuile dans `rect` ; rend le geste fait dessus.
pub(super) fn tile_ui(ui: &mut egui::Ui, rect: Rect, v: &TileView, focused: bool, cx: &mut Cx<'_>) -> Option<TileAct> {
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let t = cx.t;
    let lang = cx.lang;
    let id = ui.scope_id().with(("tuile", &v.tile));
    let resp = ui.interact(rect, id, Sense::click());
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    painter.rect_filled(rect, t.r_media(), t.colors.bg3);
    let tex = if v.has_video() { cx.screens.texture(ui.ctx(), cx.sink, &v.tile, rect) } else { None };
    let small = FontId::proportional(t.typography.small);
    match tex {
        Some((tid, size)) => {
            // L'image épouse les coins de la tuile au lieu de les recouvrir.
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            painter.add(egui::epaint::RectShape::filled(contain(rect, size), t.r_media(), Color32::WHITE).with_texture(tid, uv));
        }
        None => {
            let side = (rect.height() * 0.38).min(rect.width() * 0.3);
            let a = Rect::from_center_size(rect.center() - vec2(0.0, side * 0.15), Vec2::splat(side));
            paint_avatar(ui, cx.images, t, a, &v.avatar, &v.who);
            let note = match v.kind {
                TileKind::Stream { watching: true, .. } if v.paused => Some(vt::PAUSED),
                TileKind::Stream { watching: true, .. } if v.connecting => Some(vt::CONNECTING),
                TileKind::Stream { watching: true, .. } | TileKind::Person { camera: true } => Some(vt::WAITING),
                _ => None,
            };
            if let Some(n) = note {
                painter.text(pos2(rect.center().x, a.bottom() + t.spacing.media_gap), egui::Align2::CENTER_TOP, n.get(lang), small.clone(), t.colors.muted);
            }
        }
    }
    let pad = t.spacing.media_gap.max(2.0);
    // Anneau de parole.
    if v.speaking && matches!(v.kind, TileKind::Person { .. }) {
        let w = (t.shapes.unread_ring * 1.5).max(2.0);
        painter.rect_stroke(rect, t.r_media(), Stroke::new(w, t.colors.online), egui::StrokeKind::Inside);
    } else if focused {
        painter.rect_stroke(rect, t.r_media(), Stroke::new(t.shapes.focus.max(1.0), t.colors.accent), egui::StrokeKind::Inside);
    }
    // « Regarder » au centre d'un direct pas encore regardé : placé avant le nom, qui s'arrête à son bord.
    let watch = matches!(v.kind, TileKind::Stream { watching: false, .. }).then(|| {
        // Disposition mobile : tuiles étroites, le bouton prend plus de place pour que « Regarder » tienne sur une ligne.
        let share = if crate::ui::mobile::is_active(ui.ctx()) { 0.8 } else { 0.4 };
        // Jamais plus étroit que son libellé : il passait sur deux lignes dans une tuile moyenne.
        // Mesuré avec la police du bouton commun, qui le dessine : l'ancienne mesure
        // (autre graisse) le laissait se casser en « Regarde / r » dans une tuile étroite.
        let label = ui.painter().layout_no_wrap(vt::WATCH.get(lang).to_owned(), crate::ui::fonts::medium_font(t.typography.button), t.colors.on_accent).size().x + 2.0 * t.spacing.control_x;
        let size = vec2((rect.width() * share).max(label).min((rect.width() - 2.0 * pad).max(label)), crate::ui::widgets::button_height(t).min(rect.height() * 0.8));
        // Sous l'avatar, jamais dessus : une tuile basse rapproche les deux.
        let avatar_bottom = rect.center().y + (rect.height() * 0.38).min(rect.width() * 0.3) * 0.35;
        let y = (rect.bottom() - pad * 3.0 - size.y * 1.5).max(avatar_bottom + pad + size.y / 2.0).min(rect.bottom() - pad - size.y / 2.0);
        Rect::from_center_size(pos2(rect.center().x, y), size)
    });
    // Nom, muet et sourdine en bas à gauche.
    let icon = t.typography.small + 2.0;
    let icons_n = usize::from(v.muted) + usize::from(v.deafened);
    let extras = pad * 2.0 + icons_n as f32 * (icon + pad / 2.0);
    let name_h = ui.fonts_mut(|f| f.row_height(&small));
    // Le nom s'arrête avant le bouton quand ils partagent la même hauteur, avec des points de suspension.
    let room = match watch {
        Some(b) if b.bottom() > rect.bottom() - pad * 2.0 - name_h => b.left() - pad - (rect.left() + pad),
        _ => rect.width() - pad * 2.0,
    };
    let mut job = egui::text::LayoutJob::simple_singleline(v.name.clone(), small.clone(), t.on_media());
    job.wrap = egui::text::TextWrapping::truncate_at_width((room - extras).max(0.0));
    let name = painter.layout_job(job);
    let pill_w = (name.size().x + extras).min(room);
    let pill = Rect::from_min_size(pos2(rect.left() + pad, rect.bottom() - pad - name.size().y - pad), vec2(pill_w.max(0.0), name.size().y + pad));
    painter.rect_filled(pill, t.r_row(), t.media_veil());
    let mut x = pill.left() + pad;
    for (on, ic) in [(v.muted, VIcon::MicOff), (v.deafened, VIcon::HeadphonesOff)] {
        if on {
            vicons::paint(ic, &painter, Rect::from_center_size(pos2(x + icon / 2.0, pill.center().y), Vec2::splat(icon)), t.on_media_danger());
            x += icon + pad / 2.0;
        }
    }
    painter.with_clip_rect(pill.intersect(rect)).galley(pos2(x, pill.center().y - name.size().y / 2.0), name, t.on_media());
    // Direct : vignette « EN DIRECT » et spectateurs en haut à gauche.
    if let TileKind::Stream { .. } = v.kind {
        let live = painter.layout_no_wrap(vt::LIVE.get(lang).to_string(), FontId::proportional(t.typography.small), t.colors.on_accent);
        let badge = Rect::from_min_size(rect.min + Vec2::splat(pad), live.size() + vec2(pad * 2.0, pad / 2.0));
        painter.rect_filled(badge, t.r_row(), t.danger_fill());
        painter.galley(badge.min + vec2(pad, pad / 4.0), live, t.colors.on_accent);
        if v.viewers > 0 {
            let text = painter.layout_no_wrap(vp::viewers(lang, v.viewers as u64), small.clone(), t.on_media());
            let r = Rect::from_min_size(pos2(badge.right() + pad / 2.0, badge.top()), vec2(text.size().x + icon + pad * 2.0, badge.height()));
            painter.rect_filled(r, t.r_row(), t.media_veil());
            icons::paint(Glyph::Eye, &painter, Rect::from_center_size(pos2(r.left() + pad / 2.0 + icon / 2.0, r.center().y), Vec2::splat(icon)), t.on_media());
            painter.galley(pos2(r.left() + pad + icon, r.center().y - text.size().y / 2.0), text, t.on_media());
        }
    }
    let mut act = None;
    if let Some(b) = watch
        && let Tile::Stream(key) = &v.tile
    {
        let mut over = ui.new_child(egui::UiBuilder::new().max_rect(b).layout(egui::Layout::left_to_right(egui::Align::Center)));
        let r = crate::ui::widgets::button_wide(&mut over, t, crate::ui::widgets::Kind::Primary, vt::WATCH.get(lang), b.width());
        if r.clicked() {
            act = Some(TileAct::Watch(key.clone()));
        }
    }
    // Boutons au survol, en haut à droite.
    let hovered = ui.rect_contains_pointer(rect);
    if hovered && rect.width() > icon * 6.0 {
        let side = (t.typography.body + pad).min(rect.height() / 3.0);
        let mut right = rect.right() - pad;
        let mut slot = || {
            let r = Rect::from_min_size(pos2(right - side, rect.top() + pad), Vec2::splat(side));
            right -= side + pad / 2.0;
            r
        };
        if v.has_video() && overlay_button(ui, t, slot(), id.with("plein"), vt::FULLSCREEN.get(lang), |p, r, c| icons::paint(Glyph::Fullscreen, p, r, c)) {
            act = Some(TileAct::Fullscreen(v.tile.clone()));
        }
        if v.has_video() && overlay_button(ui, t, slot(), id.with("detacher"), vt::POP_OUT.get(lang), |p, r, c| icons::paint(Glyph::PopOut, p, r, c)) {
            act = Some(TileAct::PopOut(v.tile.clone()));
        }
        if let (TileKind::Stream { watching: true, mine: false }, Tile::Stream(key)) = (v.kind, &v.tile)
            && overlay_button(ui, t, slot(), id.with("arreter"), vt::STOP_WATCHING.get(lang), |p, r, c| icons::paint(Glyph::ScreenOff, p, r, c))
        {
            act = Some(TileAct::StopWatching(key.clone()));
        }
    }
    // Son du direct, au survol, en bas à droite : haut-parleur (couper) et curseur.
    if let Some((user, p)) = v.sound
        && hovered
        && rect.width() > icon * 16.0
    {
        let h = (t.typography.body + pad * 2.0).min(rect.height() / 3.0);
        let bar = Rect::from_min_max(pos2(rect.right() - pad - icon * 9.0, rect.bottom() - pad - h), pos2(rect.right() - pad, rect.bottom() - pad));
        ui.painter().rect_filled(bar, t.r_row(), t.media_veil());
        let speaker = Rect::from_min_size(bar.min, Vec2::splat(h));
        let tip = if p.muted { vt::STREAM_UNMUTE } else { vt::STREAM_MUTE }.get(lang);
        let r = ui.interact(speaker, id.with("son"), Sense::click()).on_hover_text(tip);
        vicons::paint(VIcon::Speaker, ui.painter(), speaker.shrink(h * 0.2), if p.muted { t.on_media_danger() } else { t.on_media() });
        if r.clicked() {
            act = Some(TileAct::LocalMute(user, !p.muted));
        }
        let mut over = ui.new_child(egui::UiBuilder::new().id_salt(id.with("volume")).max_rect(Rect::from_min_max(pos2(speaker.right() + pad / 2.0, bar.top()), bar.max - vec2(pad, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
        over.spacing_mut().slider_width = over.available_width();
        let mut volume = p.volume;
        let r = over.add(egui::Slider::new(&mut volume, 0..=200).show_value(false)).on_hover_text(format!("{} : {volume} %", vt::STREAM_VOLUME.get(lang)));
        if r.changed() || r.drag_stopped() {
            act = Some(TileAct::Volume { user, volume, done: !r.dragged() });
        }
    }
    // Clic droit : les mêmes gestes, nommés.
    if v.has_video() || v.sound.is_some() {
        let mut picked = None;
        resp.context_menu(|ui| {
            if let Some((user, p)) = v.sound {
                ui.label(egui::RichText::new(vt::STREAM_VOLUME.get(lang)).small().color(t.colors.muted));
                let mut volume = p.volume;
                let r = ui.add(egui::Slider::new(&mut volume, 0..=200).custom_formatter(|v, _| format!("{v:.0} %")));
                if r.changed() || r.drag_stopped() {
                    picked = Some(TileAct::Volume { user, volume, done: !r.dragged() });
                }
                let mut muted = p.muted;
                if crate::ui::menu::check(ui, t, vt::STREAM_MUTE.get(lang), &mut muted) {
                    picked = Some(TileAct::LocalMute(user, muted));
                }
                crate::ui::menu::separator(ui, t);
            }
            if v.has_video() {
                if crate::ui::menu::item(ui, t, vt::FULLSCREEN.get(lang), Some(crate::theme::icons::Icon::Fullscreen), false).clicked() {
                    picked = Some(TileAct::Fullscreen(v.tile.clone()));
                    ui.close();
                }
                if crate::ui::menu::item(ui, t, vt::POP_OUT.get(lang), None, false).clicked() {
                    picked = Some(TileAct::PopOut(v.tile.clone()));
                    ui.close();
                }
            }
            if let (TileKind::Stream { watching: true, mine: false }, Tile::Stream(key)) = (v.kind, &v.tile)
                && crate::ui::menu::item(ui, t, vt::STOP_WATCHING.get(lang), Some(crate::theme::icons::Icon::ScreenOff), true).clicked()
            {
                picked = Some(TileAct::StopWatching(key.clone()));
                ui.close();
            }
        });
        act = picked.or(act);
    }
    if act.is_none() {
        if resp.double_clicked() && v.has_video() {
            act = Some(TileAct::Fullscreen(v.tile.clone()));
        } else if resp.clicked() {
            act = Some(if focused { TileAct::Unfocus } else { TileAct::Focus(v.tile.clone()) });
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &v.name));
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_picks_the_biggest_tiles_and_centers_them() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
        assert!(grid(area, 0, 8.0).is_empty());
        let one = grid(area, 1, 8.0);
        assert_eq!(one.len(), 1);
        // 1000 × 600 : la largeur limite (1000 × 562,5), centrée en hauteur.
        assert!((one[0].width() - 1000.0).abs() < 0.01 && (one[0].center().y - 300.0).abs() < 0.01, "{one:?}");
        let four = grid(area, 4, 8.0);
        assert_eq!(four.len(), 4);
        assert!((four[0].top() - four[1].top()).abs() < 0.01 && four[2].top() > four[0].bottom(), "2 × 2");
        for r in &four {
            assert!(area.contains_rect(*r), "{r:?}");
            assert!((r.width() / r.height() - RATIO).abs() < 0.01);
        }
        // Trois tuiles sur une bande large : une rangée.
        let wide = grid(Rect::from_min_size(pos2(0.0, 0.0), vec2(2000.0, 300.0)), 3, 8.0);
        assert!(wide.iter().all(|r| (r.top() - wide[0].top()).abs() < 0.01));
        // Cinq tuiles en 3 + 2 : la dernière rangée est centrée.
        let five = grid(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 500.0)), 5, 8.0);
        let mid = (five[3].left() + five[4].right()) / 2.0;
        assert!((mid - 600.0).abs() < 0.5, "{five:?}");
        assert_eq!(contain(Rect::from_min_size(pos2(0.0, 0.0), vec2(200.0, 200.0)), vec2(16.0, 9.0)).width(), 200.0);
    }
}
