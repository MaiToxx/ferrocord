//! Visionneuse : une image agrandie par-dessus la fenêtre. Échap ou un clic
//! à côté de l'image la ferme ; la molette zoome autour du pointeur, un
//! glissé déplace l'image ; « Ouvrir l'original », « Copier le lien »,
//! « Copier l'image » et « Enregistrer l'image » (lot VID, `image_ops.rs`).
//!
//! L'image est demandée à la taille de la fenêtre (bornée pour
//! qu'une seule texture ne remplisse jamais le cache) ; zoomée, c'est la
//! source entière qui est demandée (même borne). Rien n'est animé : aucun redessin
//! tant que rien ne bouge.

use crate::images::{Image, Images};
use crate::i18n::{t as tx, Lang};
use crate::state::model::ImageRef;
use crate::theme::Tokens;
use eframe::egui::{self, Id, Rect, RichText, Sense, Vec2};

/// Ce qu'il faut pour ouvrir une image (emprunté au message affiché).
#[derive(Clone, Copy)]
pub(super) struct ViewSrc<'a> {
    pub image: ImageRef,
    /// Adresse téléchargée (proxy de Discord).
    pub fetch: &'a str,
    /// Adresse ouverte par « Ouvrir l'original » et copiée.
    pub original: &'a str,
    pub width: u32,
    pub height: u32,
    pub alt: Option<&'a str>,
    pub name: &'a str,
}

/// Image ouverte : gardée dans la mémoire d'egui jusqu'à la fermeture.
#[derive(Clone, Debug, PartialEq)]
struct Viewing {
    image: ImageRef,
    fetch: String,
    original: String,
    width: u32,
    height: u32,
    alt: Option<String>,
    name: String,
    zoom: f32,
    pan: Vec2,
}

const ZOOM_MIN: f32 = 1.0;
const ZOOM_MAX: f32 = 8.0;
/// Zoom par point de défilement de la molette.
const ZOOM_RATE: f32 = 0.002;

fn id() -> Id {
    Id::unique("visionneuse")
}

/// Ouvre la visionneuse sur `src` (à l'image suivante).
pub(super) fn open(ctx: &egui::Context, src: &ViewSrc<'_>) {
    let v = Viewing {
        image: src.image,
        fetch: src.fetch.to_string(),
        original: src.original.to_string(),
        width: src.width,
        height: src.height,
        alt: src.alt.map(str::to_string),
        name: src.name.to_string(),
        zoom: ZOOM_MIN,
        pan: Vec2::ZERO,
    };
    ctx.data_mut(|d| d.insert_temp(id(), v));
    ctx.request_repaint();
}

#[cfg(test)]
fn is_open(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<Viewing>(id()).is_some())
}

fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Viewing>(id()));
    ctx.request_repaint();
}

/// Taille (points) de l'image à zoom 1 : la source entière dans `area`,
/// jamais agrandie au-delà de ses pixels ; inconnue : toute la zone.
pub(super) fn fitted(width: u32, height: u32, area: Vec2, ppp: f32) -> Vec2 {
    if width == 0 || height == 0 {
        return area;
    }
    let src = Vec2::new(width as f32, height as f32) / ppp.max(f32::EPSILON);
    let scale = (area.x / src.x).min(area.y / src.y).min(1.0);
    src * scale
}

/// Taille demandée (points) : `fitted`, réduite pour que la texture tienne
/// dans `max_bytes` (RVBA).
pub(super) fn request(fitted: Vec2, ppp: f32, max_bytes: usize) -> Vec2 {
    let px = fitted * ppp;
    let bytes = px.x * px.y * 4.0;
    if !bytes.is_finite() || bytes <= max_bytes as f32 || bytes <= 0.0 {
        return fitted;
    }
    fitted * (max_bytes as f32 / bytes).sqrt()
}

/// Taille demandée (points) une fois l'image zoomée : la source entière,
/// réduite pour tenir dans `max_bytes` et dans le plus grand côté décodé.
/// `None` si elle n'apporte rien de plus que `wanted` (la copie à la taille
/// de la fenêtre) ou si la taille de la source est inconnue. Sans elle, le
/// zoom (jusqu'à 8 fois) étirait la copie réduite : une image floue.
pub(super) fn detail(width: u32, height: u32, wanted: Vec2, ppp: f32, max_bytes: usize) -> Option<Vec2> {
    if width == 0 || height == 0 {
        return None;
    }
    let ppp = ppp.max(f32::EPSILON);
    let px = Vec2::new(width as f32, height as f32);
    let side = crate::images::decode::MAX_SIDE as f32;
    let px = px * (side / px.max_elem()).min(1.0);
    let full = request(px / ppp, ppp, max_bytes);
    (full.x > wanted.x * 1.05 || full.y > wanted.y * 1.05).then_some(full)
}

/// Dessine la visionneuse si elle est ouverte. À appeler avant le reste de
/// l'interface : elle prend Échap et la molette pour elle.
pub(super) fn show(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images) {
    let ctx = ui.ctx().clone();
    let Some(mut v) = ctx.data(|d| d.get_temp::<Viewing>(id())) else { return };
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        close(&ctx);
        return;
    }
    let screen = ctx.content_rect();
    let ppp = ctx.pixels_per_point();
    let pad = t.spacing.chat_pad.max(t.spacing.item_x) * 2.0;
    let area = (screen.size() - Vec2::splat(2.0 * pad)).max(Vec2::splat(1.0));
    let base = fitted(v.width, v.height, area, ppp);
    let wanted = request(base, ppp, images.max_texture_bytes());
    let before = v.clone();
    let mut closing = false;
    egui::Area::new(id().with("zone")).order(egui::Order::Foreground).fixed_pos(screen.min).show(&ctx, |ui| {
        let (bg, back) = ui.allocate_exact_size(screen.size(), Sense::click_and_drag());
        ui.painter().rect_filled(bg, 0.0, t.colors.scrim);
        // Zoom autour du pointeur : le point visé reste sous le pointeur.
        // Zoom par cran inchangé, quelle que soit la vitesse de défilement réglée.
        let scroll = if back.hovered() { super::scroll::in_egui_notches(ui.ctx(), ui.input(|i| i.smooth_scroll_delta.y)) } else { 0.0 };
        if scroll != 0.0 {
            let z0 = v.zoom;
            let z1 = (z0 * (scroll * ZOOM_RATE).exp()).clamp(ZOOM_MIN, ZOOM_MAX);
            if let Some(ptr) = back.hover_pos() {
                let from_center = ptr - bg.center();
                v.pan = from_center - (from_center - v.pan) * (z1 / z0);
            }
            v.zoom = z1;
            ui.input_mut(|i| i.smooth_scroll_delta = Vec2::ZERO);
        }
        if back.dragged() {
            v.pan += back.drag_delta();
        }
        if v.zoom <= ZOOM_MIN {
            v.pan = Vec2::ZERO;
        }
        let rect = Rect::from_center_size(bg.center() + v.pan, base * v.zoom);
        // Zoomée : la source entière (bornée), dès qu'elle est prête ; d'ici là, et à zoom 1, la copie à la taille de la fenêtre.
        let close_up = (v.zoom > ZOOM_MIN).then(|| detail(v.width, v.height, wanted, ppp, images.max_texture_bytes())).flatten();
        let shown = match close_up.map(|size| images.get_media(v.image, &v.fetch, size)) {
            Some(Image::Ready(tex)) => Image::Ready(tex),
            _ => images.get_media(v.image, &v.fetch, wanted),
        };
        match shown {
            Image::Ready(tex) => {
                let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
                ui.painter().add(egui::Shape::Rect(egui::epaint::RectShape::filled(rect, t.r_media(), egui::Color32::WHITE).with_texture(tex, uv)));
            }
            state => {
                let text = if state == Image::Pending { tx::LOADING } else { tx::IMAGE_UNAVAILABLE };
                ui.painter().rect_filled(rect, t.r_media(), t.colors.bg1);
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text.get(lang), egui::TextStyle::Body.resolve(ui.style()), t.colors.muted);
            }
        }
        if back.clicked() && !back.interact_pointer_pos().is_some_and(|p| rect.contains(p)) {
            closing = true;
        }
        // Barre du haut : nom et actions.
        let bar = Rect::from_min_size(screen.min + Vec2::splat(pad / 2.0), Vec2::new(screen.width() - pad, 0.0));
        ui.scope_builder(egui::UiBuilder::new().max_rect(bar).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
            if ui.button(tx::CLOSE_TIP.get(lang)).clicked() {
                closing = true;
            }
            if ui.button(tx::COPY_IMAGE_LINK.get(lang)).clicked() {
                ui.ctx().copy_text(v.original.clone());
            }
            let file = super::image_ops::source(&v.fetch, &v.original);
            if super::image_ops::CAN_SAVE && ui.button(crate::i18n::table::messages::t::SAVE_IMAGE.get(lang)).clicked() {
                super::image_ops::ask(ui.ctx(), super::image_ops::Op::Save, file, &v.name);
            }
            if ui.button(crate::i18n::table::messages::t::COPY_IMAGE.get(lang)).clicked() {
                super::image_ops::ask(ui.ctx(), super::image_ops::Op::Copy, file, &v.name);
            }
            if ui.button(tx::OPEN_ORIGINAL.get(lang)).clicked() {
                ui.ctx().open_url(egui::OpenUrl::new_tab(v.original.clone()));
            }
            ui.add(egui::Label::new(RichText::new(&v.name).color(t.colors.text_strong)).truncate());
        });
        // Bas : texte alternatif, sinon l'aide des gestes.
        let (text, color) = match &v.alt {
            Some(alt) => (alt.as_str(), t.colors.text),
            None => (tx::VIEWER_HINT.get(lang), t.colors.muted),
        };
        let width = (screen.width() - 2.0 * pad).max(1.0);
        let galley = ui.painter().layout(text.to_string(), egui::TextStyle::Body.resolve(ui.style()), color, width);
        let margin = t.toast_margin().sum();
        let boxed = Rect::from_center_size(egui::pos2(screen.center().x, screen.max.y - pad / 2.0 - (galley.size().y + margin.y) / 2.0), galley.size() + margin);
        ui.painter().rect_filled(boxed, t.r_toast(), t.colors.bg3);
        ui.painter().galley(boxed.min + margin / 2.0, galley, color);
    });
    if closing {
        close(&ctx);
    } else if v != before {
        ctx.data_mut(|d| d.insert_temp(id(), v));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_never_enlarges() {
        let area = Vec2::new(1000.0, 800.0);
        assert_eq!(fitted(500, 400, area, 1.0), Vec2::new(500.0, 400.0));
        assert_eq!(fitted(4000, 1000, area, 1.0), Vec2::new(1000.0, 250.0));
        assert_eq!(fitted(500, 400, area, 2.0), Vec2::new(250.0, 200.0), "pixels de l'écran");
        assert_eq!(fitted(0, 10, area, 1.0), area);
    }

    #[test]
    fn request_fits_the_texture_budget() {
        let r = request(Vec2::new(2000.0, 2000.0), 1.0, 4_000_000);
        assert!(r.x * r.y * 4.0 <= 4_000_001.0, "{r:?}");
        assert_eq!(request(Vec2::new(100.0, 100.0), 2.0, 4_000_000), Vec2::new(100.0, 100.0));
    }

    /// Partie D : zoomée, la visionneuse demande la source (bornée), plus la
    /// copie réduite à la fenêtre. Coût en mémoire, avant et après.
    #[test]
    fn zoom_asks_for_the_source_within_the_budget() {
        const MAX: usize = crate::images::VIEWER_MAX_BYTES;
        let bytes = |points: Vec2, ppp: f32| (points.x * ppp).round() as usize * (points.y * ppp).round() as usize * 4;
        // Fenêtre de 1375 × 900 pixels à 125 % : environ 1000 × 650 points pour l'image.
        let (area, ppp) = (Vec2::new(1000.0, 650.0), 1.25);
        // Photo de 4000 × 3000 : avant, une copie de 1083 × 812 pixels (3,5 Mo), étirée jusqu'à 8 fois.
        let base = request(fitted(4000, 3000, area, ppp), ppp, MAX);
        assert_eq!(((base.x * ppp).round(), (base.y * ppp).round()), (1083.0, 813.0));
        let before = bytes(base, ppp);
        assert!((3_400_000..3_600_000).contains(&before), "{before}");
        // Après, zoomée : la source ramenée au budget d'une texture (12 Mio), 1,9 fois plus de pixels par côté.
        let full = detail(4000, 3000, base, ppp, MAX).unwrap_or_else(|| panic!("la source apporte du détail"));
        let after = bytes(full, ppp);
        assert!(after <= MAX + 16_000 && after > MAX * 9 / 10, "{after}");
        assert!(full.x / base.x > 1.85, "{full:?} {base:?}");
        assert!(before + after < 16 << 20, "les deux textures ensemble, le temps du zoom : moins de 16 Mio");
        // Capture d'écran de 1920 × 1080 : la source exacte (8,3 Mo), sous le budget.
        let base = request(fitted(1920, 1080, area, ppp), ppp, MAX);
        let full = detail(1920, 1080, base, ppp, MAX).unwrap_or_else(|| panic!("détail"));
        assert_eq!(((full.x * ppp).round(), (full.y * ppp).round()), (1920.0, 1080.0));
        assert_eq!(bytes(full, ppp), 1920 * 1080 * 4);
        // Rien de plus à montrer : aucune seconde demande.
        let small = request(fitted(500, 400, area, ppp), ppp, MAX);
        assert_eq!(detail(500, 400, small, ppp, MAX), None, "image déjà entière");
        assert_eq!(detail(0, 0, area, ppp, MAX), None, "taille inconnue");
        // Très longue : le plus grand côté décodé borne aussi.
        let long = detail(20_000, 200, Vec2::new(1000.0, 10.0), 1.0, MAX).unwrap_or_else(|| panic!("détail"));
        assert_eq!(long.x, crate::images::decode::MAX_SIDE as f32);
    }

    /// Ouverte puis fermée par Échap ; un clic sur le fond la ferme aussi.
    #[test]
    fn opens_and_closes() {
        let ctx = egui::Context::default();
        let images = Images::disabled(ctx.clone());
        let t = Tokens::default();
        let run = |events: Vec<egui::Event>| {
            let input = egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(800.0, 600.0))),
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, |ui| show(ui, &t, Lang::Fr, &images));
            out.textures_delta.clear();
        };
        let src = ViewSrc { image: ImageRef::Media { owner: 1, slot: 0, rev: 0 }, fetch: "u", original: "o", width: 10, height: 10, alt: Some("alt"), name: "n" };
        open(&ctx, &src);
        run(Vec::new());
        assert!(is_open(&ctx));
        let esc = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        run(vec![esc]);
        assert!(!is_open(&ctx));
        open(&ctx, &src);
        run(Vec::new());
        let at = egui::pos2(20.0, 300.0);
        let press = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        run(vec![egui::Event::PointerMoved(at), press(true)]);
        run(vec![press(false)]);
        run(Vec::new());
        assert!(!is_open(&ctx), "clic à côté de l'image");
    }
}
