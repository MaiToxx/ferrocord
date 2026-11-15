//! Onglet GIF du sélecteur d'expressions (lot G) : tuiles (favoris,
//! tendances, catégories), résultats d'une recherche en deux colonnes,
//! favoris du compte, étoile sur chaque GIF.
//!
//! Les GIF ne s'animent que sous le pointeur (et jamais si les animations
//! sont réduites) : un sélecteur ouvert ne coûte rien tant qu'on n'y touche
//! pas. Seules les images visibles sont demandées.
//!
//! Rien n'est écrit ici dans le magasin : les demandes ([`Act`]) sont
//! appliquées par `App` après l'affichage (`composer_emoji.rs`).

use crate::i18n::{table::gifs::t as gt, Lang};
use crate::images::{Image, Images};
use crate::state::frecency::FavGif;
use crate::state::gifs::{Category, Gifs, View};
use crate::state::model::ImageRef;
use crate::state::usersettings::Remote;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Rect, Sense};

/// Colonnes de la grille.
const COLS: usize = 2;

/// Demande à appliquer au magasin après l'affichage.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Act {
    /// Le texte de la recherche a changé (la requête attend la fin de la frappe).
    Typed(String),
    /// Recherche immédiate (Entrée, clic sur une catégorie).
    Search(String),
    Trending,
    Favorites,
    Home,
    ToggleFav(FavGif),
    /// Catégories à redemander après un échec.
    Retry,
}

/// Clé d'image d'un média de GIF, tirée de son adresse.
fn image_ref(url: &str) -> ImageRef {
    // FNV-1a : stable d'une image à l'autre, sans état à garder.
    let h = url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    ImageRef::Media { owner: h, slot: u16::MAX, rev: (h >> 32) as u32 }
}

/// Place de chaque image dans une grille à colonnes de `colw` points :
/// chacune va dans la colonne la moins haute, à ses proportions (bornées).
/// Renvoie les rectangles (origine en haut à gauche) et la hauteur totale.
fn masonry(sizes: impl Iterator<Item = (u32, u32)>, colw: f32, gap: f32) -> (Vec<Rect>, f32) {
    let mut tops = [0.0f32; COLS];
    let mut out = Vec::new();
    for (w, h) in sizes {
        let ratio = if w > 0 && h > 0 { h as f32 / w as f32 } else { 9.0 / 16.0 };
        let height = (colw * ratio.clamp(0.4, 1.8)).round();
        let col = (0..COLS).min_by(|a, b| tops[*a].total_cmp(&tops[*b])).unwrap_or(0);
        out.push(Rect::from_min_size(egui::pos2(col as f32 * (colw + gap), tops[col]), egui::vec2(colw, height)));
        tops[col] += height + gap;
    }
    let total = tops.iter().copied().fold(0.0, f32::max);
    (out, (total - gap).max(0.0))
}

/// Dessine l'image `url` dans `rect`, recadrée au centre pour le remplir ;
/// animée si `play`. `false` : pas encore là (le fond reste).
fn paint_cover(ui: &egui::Ui, t: &Tokens, images: &Images, rect: Rect, url: &str, play: bool) -> bool {
    let image = image_ref(url);
    let got = if play { images.get_media_moving(image, url, rect.size(), true) } else { images.get_media(image, url, rect.size()) };
    let Image::Ready(tex) = got else { return false };
    let size = ui.ctx().tex_manager().read().meta(tex).map(|m| m.size).filter(|[w, h]| *w > 0 && *h > 0);
    let mut uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if let Some([w, h]) = size {
        let (have, want) = (w as f32 / h as f32, rect.width() / rect.height().max(1.0));
        if have > want {
            uv = uv.shrink2(egui::vec2((1.0 - want / have) / 2.0, 0.0));
        } else {
            uv = uv.shrink2(egui::vec2(0.0, (1.0 - have / want) / 2.0));
        }
    }
    ui.painter().add(egui::Shape::Rect(egui::epaint::RectShape::filled(rect, t.r_media(), egui::Color32::WHITE).with_texture(tex, uv)));
    true
}

/// Le GIF sous le pointeur joue-t-il ?
fn plays(ui: &egui::Ui, hovered: bool) -> bool {
    hovered && super::motion::get(ui.ctx()).gifs.now(true)
}

/// Contour d'un élément survolé ou qui a le focus.
fn outline(ui: &egui::Ui, t: &Tokens, rect: Rect) {
    ui.painter().rect_stroke(rect, t.r_media(), egui::Stroke::new(t.shapes.focus.max(1.0), t.colors.focus), egui::StrokeKind::Inside);
}

/// Couleur lisible sur le voile sombre posé sur une image.
fn on_scrim(t: &Tokens) -> egui::Color32 {
    t.readable(t.colors.on_accent, t.colors.scrim.to_opaque())
}

/// Tuile de l'accueil : image assombrie, libellé centré.
#[allow(clippy::too_many_arguments)]
fn tile(ui: &mut egui::Ui, t: &Tokens, images: &Images, rect: Rect, id: egui::Id, label: &str, src: Option<&str>, star: bool) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let hot = resp.hovered() || resp.has_focus();
    ui.painter().rect_filled(rect, t.r_media(), t.colors.bg3);
    let shown = src.is_some_and(|s| paint_cover(ui, t, images, rect, s, plays(ui, hot)));
    if shown {
        ui.painter().rect_filled(rect, t.r_media(), t.colors.scrim.linear_multiply(if hot { 0.6 } else { 0.85 }));
    }
    let color = if shown { on_scrim(t) } else { t.colors.text_strong };
    let font = super::fonts::demi_font(t.typography.body);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, color);
    let icon = if star { galley.size().y } else { 0.0 };
    let icon_gap = if star { t.spacing.item_x / 2.0 } else { 0.0 };
    let width = galley.size().x.min(rect.width() - 2.0 * t.spacing.item_x - icon - icon_gap);
    let left = rect.center().x - (width + icon + icon_gap) / 2.0;
    if star {
        let at = Rect::from_min_size(egui::pos2(left, rect.center().y - icon / 2.0), egui::Vec2::splat(icon)).shrink(icon * 0.1);
        super::emoji_picker::paint_star(ui.painter(), at, color, egui::Stroke::NONE);
    }
    let text_left = left + icon + icon_gap;
    let clip = Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2(text_left + width, rect.bottom()));
    ui.painter().with_clip_rect(clip.intersect(ui.clip_rect())).galley(egui::pos2(text_left, rect.center().y - galley.size().y / 2.0), galley, color);
    if hot {
        outline(ui, t, rect);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Un GIF de la grille. Renvoie (clic sur le GIF, clic sur l'étoile).
#[allow(clippy::too_many_arguments)]
fn gif_cell(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, rect: Rect, id: egui::Id, src: Option<&str>, favorite: bool, can_star: bool) -> (bool, bool) {
    let resp = ui.interact(rect, id, Sense::click());
    if !ui.is_rect_visible(rect) {
        return (false, false);
    }
    let hot = ui.rect_contains_pointer(rect) || resp.has_focus();
    ui.painter().rect_filled(rect, t.r_media(), t.colors.bg3);
    match src {
        Some(s) => {
            paint_cover(ui, t, images, rect, s, plays(ui, hot));
        }
        // Favori vidéo : pas d'aperçu, un triangle de lecture et l'explication au survol.
        None => {
            let side = (rect.height() * 0.3).min(Tokens::icon_side(t.typography.title));
            icons::paint(Icon::Play, ui.painter(), Rect::from_center_size(rect.center(), egui::Vec2::splat(side)), t.colors.muted);
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, gt::SEND_GIF.get(lang)));
    let mut starred = false;
    if hot {
        outline(ui, t, rect);
        if can_star {
            let side = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y);
            let at = Rect::from_min_size(egui::pos2(rect.right() - side - t.spacing.item_x / 2.0, rect.top() + t.spacing.item_x / 2.0), egui::Vec2::splat(side));
            let star = ui.interact(at, id.with("étoile"), Sense::click());
            ui.painter().rect_filled(at, t.r_row(), t.colors.scrim.to_opaque().linear_multiply(if star.hovered() { 0.95 } else { 0.75 }));
            let (fill, stroke) = if favorite { (t.colors.idle, egui::Stroke::NONE) } else { (egui::Color32::TRANSPARENT, egui::Stroke::new((side * 0.07).max(1.0), on_scrim(t))) };
            super::emoji_picker::paint_star(ui.painter(), at.shrink(side * 0.22), fill, stroke);
            let tip = if favorite { gt::REMOVE_FAVORITE } else { gt::ADD_FAVORITE };
            star.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip.get(lang)));
            starred = star.on_hover_text(tip.get(lang)).clicked();
        }
    }
    let resp = if src.is_none() { resp.on_hover_text(gt::VIDEO_FAVORITE.get(lang)) } else { resp };
    (resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !starred, starred)
}

/// Message centré dans la zone de la grille.
fn notice(ui: &mut egui::Ui, t: &Tokens, height: f32, text: &str, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
    let galley = ui.painter().layout(text.to_string(), egui::FontId::proportional(t.typography.body), color, rect.width() - 4.0 * t.spacing.item_x);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

/// Ligne « ‹ Titre » des vues Favoris et Tendances.
fn back_row(ui: &mut egui::Ui, t: &Tokens, lang: Lang, title: &str) -> bool {
    let mut back = false;
    ui.horizontal(|ui| {
        let side = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y);
        let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        if hot {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
        }
        let color = if hot { t.colors.text_strong } else { t.colors.muted };
        crate::theme::icons::paint(crate::theme::icons::Icon::ChevronLeft, ui.painter(), rect.shrink(side * 0.22), color);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, gt::BACK.get(lang)));
        back = resp.on_hover_text(gt::BACK.get(lang)).clicked();
        ui.label(egui::RichText::new(title).family(super::fonts::demi()).color(t.colors.text_strong));
    });
    back
}

/// Zone défilante de la grille, de hauteur fixe.
fn scroll(salt: impl egui::AsIdSalt, height: f32) -> egui::ScrollArea {
    egui::ScrollArea::vertical().id_salt(salt).max_height(height).min_scrolled_height(height).auto_shrink([false, false])
}

/// Tuiles de l'accueil : favoris, tendances, puis une par catégorie.
#[allow(clippy::too_many_arguments)]
fn home(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, gifs: &Gifs, query: &mut String, height: f32, colw: f32, acts: &mut Vec<Act>) {
    let gap = t.spacing.item_x;
    let h24 = t.layout.clock == crate::theme::Clock::H24;
    let cats: &[Category] = match &gifs.categories {
        Remote::Ready(c) => c,
        _ => &[],
    };
    let tile_h = (super::emoji_picker::cell(t) * 2.25).round();
    let count = 2 + cats.len();
    let tiles = count.div_ceil(COLS) as f32 * (tile_h + gap) - gap;
    let width = ui.available_width();
    scroll("gif-accueil", height).show_viewport(ui, |ui, view| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let (full, _) = ui.allocate_exact_size(egui::vec2(width, tiles), Sense::hover());
        for i in 0..count {
            let top = (i / COLS) as f32 * (tile_h + gap);
            if top + tile_h < view.min.y || top > view.max.y {
                continue;
            }
            let rect = Rect::from_min_size(full.min + egui::vec2((i % COLS) as f32 * (colw + gap), top), egui::vec2(colw, tile_h));
            let id = ui.scope_id().with(("tuile", i));
            match i {
                0 => {
                    let src = gifs.frecency.gifs.iter().find(|g| !g.video).map(|g| g.src.as_str());
                    if tile(ui, t, images, rect, id, gt::FAVORITES.get(lang), src, true).clicked() {
                        acts.push(Act::Favorites);
                    }
                }
                1 => {
                    if tile(ui, t, images, rect, id, gt::TRENDING.get(lang), gifs.trending_src.as_deref(), false).clicked() {
                        acts.push(Act::Trending);
                    }
                }
                _ => {
                    let Some(c) = cats.get(i - 2) else { continue };
                    if tile(ui, t, images, rect, id, &c.name, Some(&c.src), false).clicked() {
                        *query = c.name.clone();
                        acts.push(Act::Search(c.name.clone()));
                    }
                }
            }
        }
        // Sous les tuiles : l'état du chargement des catégories.
        match &gifs.categories {
            Remote::Loading => {
                ui.add_space(2.0 * gap);
                ui.vertical_centered(super::spinner::spinner);
            }
            Remote::Failed(p) => {
                ui.add_space(2.0 * gap);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new(super::words::problem(lang, p, h24)).color(t.colors.danger));
                    ui.add_space(gap);
                    if ui.button(gt::RETRY.get(lang)).clicked() {
                        acts.push(Act::Retry);
                    }
                });
            }
            _ => {}
        }
    });
}

/// Contenu de l'onglet GIF, sous le champ de recherche. `changed` et
/// `enter` : ce que le champ vient de recevoir. Renvoie le GIF à envoyer
/// (identifiant, adresse).
#[allow(clippy::too_many_arguments)]
pub(super) fn body(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, gifs: &Gifs, query: &mut String, changed: bool, enter: bool, height: f32, acts: &mut Vec<Act>) -> Option<(String, String)> {
    if enter && !query.trim().is_empty() {
        acts.push(Act::Search(query.clone()));
    } else if changed {
        acts.push(Act::Typed(query.clone()));
    }
    let h24 = t.layout.clock == crate::theme::Clock::H24;
    let gap = t.spacing.item_x;
    let mut height = height;
    // Favoris et tendances : une ligne de retour au-dessus de la grille.
    let title = match gifs.view {
        View::Favorites => Some(gt::FAVORITES),
        View::Trending => Some(gt::TRENDING),
        _ => None,
    };
    if let Some(title) = title {
        let before = ui.cursor().top();
        if back_row(ui, t, lang, title.get(lang)) {
            query.clear();
            acts.push(Act::Home);
        }
        height -= ui.cursor().top() - before;
    }
    let mut picked = None;
    let width = ui.available_width();
    // La barre de défilement garde sa place à droite.
    let colw = ((width - gap - t.spacing.item_x) / COLS as f32).floor();
    match &gifs.view {
        View::Home => home(ui, t, lang, images, gifs, query, height, colw, acts),
        View::Favorites => {
            let favs = &gifs.frecency.gifs;
            if favs.is_empty() {
                notice(ui, t, height, gt::NO_FAVORITES.get(lang), t.colors.muted);
                return None;
            }
            let (rects, total) = masonry(favs.iter().map(|g| (g.width, g.height)), colw, gap);
            scroll("gif-favoris", height).show_viewport(ui, |ui, view| {
                let (full, _) = ui.allocate_exact_size(egui::vec2(width, total.max(1.0)), Sense::hover());
                for (i, (g, r)) in favs.iter().zip(&rects).enumerate() {
                    if r.bottom() < view.min.y || r.top() > view.max.y {
                        continue;
                    }
                    let rect = r.translate(full.min.to_vec2());
                    let (send, star) = gif_cell(ui, t, lang, images, rect, ui.scope_id().with(("favori", i)), (!g.video).then_some(g.src.as_str()), true, true);
                    if star {
                        acts.push(Act::ToggleFav(g.clone()));
                    } else if send {
                        picked = Some((String::new(), g.url.clone()));
                    }
                }
            });
        }
        View::Trending | View::Search(_) => {
            if gifs.loading {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
                let at = Rect::from_center_size(rect.center(), egui::Vec2::splat(ui.spacing().interact_size.y));
                ui.scope_builder(egui::UiBuilder::new().max_rect(at), |ui| {
                    super::spinner::spinner(ui);
                });
                return None;
            }
            if let Some(p) = &gifs.error {
                notice(ui, t, height, &super::words::problem(lang, p, h24), t.colors.danger);
                return None;
            }
            if gifs.results.is_empty() {
                notice(ui, t, height, gt::NONE_FOUND.get(lang), t.colors.muted);
                return None;
            }
            let can_star = gifs.frecency.ready();
            let (rects, total) = masonry(gifs.results.iter().map(|g| (g.width, g.height)), colw, gap);
            // Une liste par recherche : chacune repart du haut.
            let salt = match &gifs.view {
                View::Search(q) => ("gif-résultats", q.as_str()),
                _ => ("gif-tendances", ""),
            };
            scroll(salt, height).show_viewport(ui, |ui, view| {
                let (full, _) = ui.allocate_exact_size(egui::vec2(width, total.max(1.0)), Sense::hover());
                for (i, (g, r)) in gifs.results.iter().zip(&rects).enumerate() {
                    if r.bottom() < view.min.y || r.top() > view.max.y {
                        continue;
                    }
                    let rect = r.translate(full.min.to_vec2());
                    let favorite = gifs.frecency.is_fav_gif(&g.url);
                    let (send, star) = gif_cell(ui, t, lang, images, rect, ui.scope_id().with(("gif", i)), Some(&g.src), favorite, can_star);
                    if star {
                        acts.push(Act::ToggleFav(g.favorite()));
                    } else if send {
                        picked = Some((g.id.clone(), g.url.clone()));
                    }
                }
            });
        }
    }
    picked
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masonry_fills_the_shortest_column_and_bounds_ratios() {
        let sizes = [(220, 110), (220, 220), (220, 110), (0, 0), (10, 1000)];
        let (r, total) = masonry(sizes.into_iter(), 200.0, 8.0);
        assert_eq!(r[0], Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(200.0, 100.0)));
        assert_eq!(r[1].min, egui::pos2(208.0, 0.0));
        // La troisième va sous la première (colonne la moins haute).
        assert_eq!(r[2].min, egui::pos2(0.0, 108.0));
        // Proportions inconnues : 16:9 ; image très haute : bornée.
        assert_eq!(r[3].height(), 113.0);
        assert_eq!(r[4].height(), 360.0);
        assert_eq!(total, r.iter().map(|x| x.bottom()).fold(0.0, f32::max));
        assert_eq!(masonry(std::iter::empty(), 200.0, 8.0), (Vec::new(), 0.0));
    }

    #[test]
    fn image_keys_differ_by_url() {
        let a = image_ref("https://media.tenor.com/aAAAAM/a.gif");
        assert_eq!(a, image_ref("https://media.tenor.com/aAAAAM/a.gif"));
        assert_ne!(a, image_ref("https://media.tenor.com/bAAAAM/b.gif"));
    }
}
