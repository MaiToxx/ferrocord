//! Pièces jointes d'un message, sous son texte : images en vignette (seule,
//! à sa proportion, ou en grille), spoilers masqués jusqu'au clic, cartes
//! pour les vidéos, sons et autres fichiers. Les embeds suivent
//! (`embeds.rs`).
//!
//! La taille de chaque vignette se déduit des dimensions reçues avec le
//! message, jamais de l'image chargée : la mise en page est la même avant et
//! après le chargement, et la mesure de la liste virtualisée (sans images)
//! donne la hauteur exacte du dessin.
//!
//! Les zones cliquables sont rendues à `message.rs` ([`Hot`]) et rendues
//! sensibles après le message entier : sinon la zone du message, ajoutée
//! après, recevrait leurs clics.

use super::viewer::{self, ViewSrc};
use crate::images::{Image, Images};
use crate::i18n::{decimal, f, t as tx, Lang};
use crate::state::model::{Attachment, ImageRef, MediaKind, Message};
use crate::state::store::Sensitive;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, emath::GuiRounding, Id, Rect, RichText, Sense, Vec2};
use std::collections::HashSet;

/// Ce que fait un clic sur une zone.
pub(super) enum Action<'a> {
    /// Ouvre une adresse dans l'application par défaut (navigateur).
    Open(&'a str),
    /// Comme `Open`, sans la confirmation des liens externes : bouton de
    /// lecture d'une vidéo que seul son site sait jouer (YouTube, Twitch).
    OpenDirect(&'a str),
    /// Ouvre la visionneuse.
    View(ViewSrc<'a>),
    /// Dévoile un spoiler ou charge une image « au clic ».
    Reveal(u64),
    /// Lecture ou pause d'un message vocal (phase 6, `voice_note.rs`).
    Note(&'a Attachment),
    /// Commande du lecteur vidéo et audio (lot M, `player.rs`).
    Media(super::player::Hit<'a>),
    /// « Afficher l'original » sous une traduction (lot E3) : (salon, message).
    Original(u64, u64),
}

/// Zones cliquables des pièces jointes et embeds d'un message.
#[derive(Default)]
pub(super) struct Hot<'a> {
    zones: Vec<(Rect, Action<'a>, Option<&'a str>)>,
}

impl<'a> Hot<'a> {
    pub(super) fn add(&mut self, rect: Rect, action: Action<'a>, tip: Option<&'a str>) {
        self.zones.push((rect, action, tip));
    }

    /// Rectangles des zones déclarées, dans l'ordre.
    #[cfg(test)]
    pub(super) fn rects(&self) -> Vec<Rect> {
        self.zones.iter().map(|z| z.0).collect()
    }

    /// Rend les zones sensibles (à appeler après la zone du message).
    pub(super) fn interact(self, ui: &mut egui::Ui, t: &Tokens, msg: u64) {
        for (i, (rect, action, tip)) in self.zones.into_iter().enumerate() {
            let sense = if matches!(&action, Action::Media(h) if h.drags()) { Sense::click_and_drag() } else { Sense::click() };
            let mut resp = ui.interact(rect, Id::unique(("média", msg, i)), sense);
            if let Action::Media(hit) = &action {
                // Le lecteur suit aussi les glissements, et dessine son propre survol.
                if resp.hovered() && !matches!(hit, super::player::Hit::Toggle) {
                    ui.painter().rect_filled(rect, t.r_control(), t.colors.hover);
                }
                resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if let Some(tip) = tip {
                    resp = resp.on_hover_text(tip);
                }
                super::player::on_zone(ui.ctx(), &resp, rect, *hit);
                continue;
            }
            if resp.hovered() {
                ui.painter().rect_filled(rect, t.r_media(), t.colors.hover);
            }
            resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            if let Some(tip) = tip {
                resp = resp.on_hover_text(tip);
            }
            // Clic droit sur une image : copier, enregistrer, lien (lot VID).
            if let Action::View(src) = &action {
                resp.context_menu(|ui| super::image_ops::menu(ui, t, src));
            }
            if !resp.clicked() {
                continue;
            }
            match action {
                Action::Open(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
                Action::OpenDirect(url) => super::links::open_direct(ui.ctx(), url),
                Action::View(src) => viewer::open(ui.ctx(), &src),
                Action::Reveal(key) => {
                    reveal(ui.ctx(), key);
                    ui.ctx().request_repaint();
                }
                Action::Note(a) => {
                    super::voice_note::clicked(ui.ctx(), a);
                    ui.ctx().request_repaint();
                }
                Action::Media(_) => {}
                Action::Original(channel, message) => super::translate::ask(ui.ctx(), channel, message, false),
            }
        }
    }
}

/// Spoilers dévoilés et images chargées au clic, pour la session. Borné :
/// au-delà, tout est oublié (les spoilers se recouvrent, rien d'autre).
#[derive(Clone, Default)]
struct Revealed(HashSet<u64>);

const MAX_REVEALED: usize = 4096;

fn revealed_id() -> Id {
    Id::unique("médias-dévoilés")
}

fn is_revealed(ctx: &egui::Context, key: u64) -> bool {
    ctx.data(|d| d.get_temp::<Revealed>(revealed_id()).is_some_and(|r| r.0.contains(&key)))
}

fn reveal(ctx: &egui::Context, key: u64) {
    ctx.data_mut(|d| {
        let r = d.get_temp_mut_or_default::<Revealed>(revealed_id());
        if r.0.len() >= MAX_REVEALED {
            r.0.clear();
        }
        r.0.insert(key);
    });
}

/// Sort des médias « contenu sensible » dans le salon affiché (F8) : posé
/// par la liste des messages avant de la dessiner.
pub(super) fn set_sensitive(ctx: &egui::Context, mode: Sensitive) {
    if sensitive(ctx) != mode {
        ctx.data_mut(|d| d.insert_temp(Id::unique("médias-sensibles"), mode));
    }
}

fn sensitive(ctx: &egui::Context) -> Sensitive {
    ctx.data(|d| d.get_temp::<Sensitive>(Id::unique("médias-sensibles"))).unwrap_or_default()
}

/// Réglages « Texte et images » du compte suivis par les messages ; posés
/// à chaque image, avant l'interface (`App::ui_pass`). Un réglage pas encore
/// reçu vaut celui de Discord : tout est montré.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Shown {
    /// Images et vidéos jointes montrées dans la discussion (sinon : la carte du fichier).
    pub attachments: bool,
    /// Images et vidéos des liens collés.
    pub link_media: bool,
    /// Embeds et aperçus des sites.
    pub embeds: bool,
    /// Réactions sous les messages.
    pub reactions: bool,
    /// Pastille « ALT » sur les images qui ont une description (lue au survol).
    pub descriptions: bool,
}

impl Default for Shown {
    fn default() -> Self {
        Shown { attachments: true, link_media: true, embeds: true, reactions: true, descriptions: false }
    }
}

impl Shown {
    pub(super) fn from_settings(s: &crate::state::usersettings::AccountState) -> Shown {
        use crate::state::usersettings::schema::{Key, SValue};
        let on = |k: Key| !matches!(s.value(k), Some(SValue::Bool(false)));
        Shown { attachments: on(Key::InlineAttachments), link_media: on(Key::InlineEmbeds), embeds: on(Key::RenderEmbeds), reactions: on(Key::RenderReactions), descriptions: matches!(s.value(Key::ImageDescriptions), Some(SValue::Bool(true))) }
    }
}

pub(super) fn set_shown(ctx: &egui::Context, v: Shown) {
    if shown(ctx) != v {
        ctx.data_mut(|d| d.insert_temp(Id::unique("médias-montrés"), v));
    }
}

pub(super) fn shown(ctx: &egui::Context) -> Shown {
    ctx.data(|d| d.get_temp::<Shown>(Id::unique("médias-montrés"))).unwrap_or_default()
}

/// Clé de dévoilement d'une image.
fn reveal_key(image: ImageRef) -> u64 {
    Id::unique(image).value()
}

/// Une image affichée dans le fil : vignette de pièce jointe ou d'embed.
pub(super) struct Tile<'a> {
    pub image: ImageRef,
    /// Adresse à télécharger (proxy de Discord) ; `None` : rien à charger.
    pub fetch: Option<&'a str>,
    /// Dimensions de la source ; 0 si inconnues.
    pub width: u32,
    pub height: u32,
    pub spoiler: bool,
    /// Marquée « contenu sensible » : masquée selon le réglage du compte.
    pub sensitive: bool,
    pub gif: bool,
    /// Vidéo : triangle de lecture par-dessus.
    pub play: bool,
    /// Clic : visionneuse ou ouverture externe.
    pub click: Option<Action<'a>>,
    /// Texte alternatif (infobulle).
    pub alt: Option<&'a str>,
}

/// Taille (points) d'une image de `w` × `h` pixels tenant dans `max`, sans
/// agrandissement ni côté plus petit que `min` ; dimensions inconnues : une
/// boîte de la hauteur maximale, aussi large qu'elle (bornée par `max`).
pub(super) fn fit_box(w: u32, h: u32, max: Vec2, min: f32) -> Vec2 {
    let max = Vec2::new(max.x.max(min), max.y.max(min));
    if w == 0 || h == 0 {
        let side = max.x.min(max.y);
        return Vec2::splat(side.floor());
    }
    let (wf, hf) = (w as f32, h as f32);
    let scale = (max.x / wf).min(max.y / hf).min(1.0);
    Vec2::new((wf * scale).floor().clamp(min, max.x), (hf * scale).floor().clamp(min, max.y))
}

/// Colonnes d'une grille de `n` images : deux pour 2 et 4, trois sinon.
pub(super) fn grid_columns(n: usize) -> usize {
    match n {
        0 | 1 => 1,
        2 | 4 => 2,
        _ => 3,
    }
}

/// Côté d'une case de grille sur `width` points, `cols` colonnes séparées de `gap`.
pub(super) fn grid_cell(width: f32, cols: usize, gap: f32) -> f32 {
    let cols = cols.max(1) as f32;
    ((width - gap * (cols - 1.0)) / cols).floor().max(1.0)
}

/// Plus petit côté d'une vignette : assez pour être cliquée.
fn min_side(t: &Tokens) -> f32 {
    Tokens::icon_side(t.typography.body) * 2.0
}

/// Pièces jointes du message `m`. `images` absent : mesure seule (même
/// place prise, rien demandé ni rendu cliquable).
pub(super) fn attachments_ui<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, m: &'a Message, hot: &mut Hot<'a>) {
    attachments_list(ui, t, lang, images, &m.attachments, hot);
}

/// Pièces jointes `list` (celles d'un message, ou de l'instantané d'un
/// message transféré), avec le même rendu : vignettes, lecteurs, cartes.
pub(super) fn attachments_list<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, list: &'a [Attachment], hot: &mut Hot<'a>) {
    if list.is_empty() {
        return;
    }
    let gap = t.spacing.media_gap;
    // « Afficher les images et vidéos jointes » décoché : chaque fichier garde sa carte.
    let inline = shown(ui.ctx()).attachments;
    let pics: Vec<&Attachment> = list.iter().filter(|a| inline && a.kind == MediaKind::Image).collect();
    if !pics.is_empty() {
        ui.add_space(gap);
    }
    if let [one] = pics[..] {
        let max = Vec2::new(ui.available_width().min(t.layout.media_width), t.layout.media_height);
        let size = fit_box(one.width, one.height, max, min_side(t));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        tile_ui(ui, t, lang, images, rect, false, attachment_tile(one), hot);
    } else if !pics.is_empty() {
        let cols = grid_columns(pics.len());
        let width = ui.available_width().min(t.layout.embed_width);
        let cell = grid_cell(width, cols, gap);
        for (n, row) in pics.chunks(cols).enumerate() {
            if n > 0 {
                ui.add_space(gap);
            }
            let (line, _) = ui.allocate_exact_size(Vec2::new(width, cell), Sense::hover());
            for (i, a) in row.iter().enumerate() {
                let rect = Rect::from_min_size(line.min + Vec2::new(i as f32 * (cell + gap), 0.0), Vec2::splat(cell));
                tile_ui(ui, t, lang, images, rect, true, attachment_tile(a), hot);
            }
        }
    }
    for a in list.iter().filter(|a| !inline || a.kind != MediaKind::Image) {
        ui.add_space(gap);
        match &a.voice {
            Some(note) => super::voice_note::voice_card(ui, t, lang, images.is_some(), a, note, hot),
            None if inline && super::player::playable(a) => super::player::card(ui, t, lang, images, a, hot),
            None => file_card(ui, t, lang, images.is_some(), a, hot),
        }
    }
}

pub(super) fn attachment_tile(a: &Attachment) -> Tile<'_> {
    let image = a.image_ref();
    let src = ViewSrc { image, fetch: a.fetch_url(), original: &a.url, width: a.width, height: a.height, alt: a.description.as_deref(), name: a.display_name() };
    Tile {
        image,
        fetch: Some(a.fetch_url()),
        width: a.width,
        height: a.height,
        spoiler: a.spoiler,
        sensitive: a.explicit,
        gif: a.animated,
        play: false,
        click: Some(Action::View(src)),
        alt: a.description.as_deref(),
    }
}

/// Dessine `tile` dans `rect` (place déjà prise). `cover` : l'image remplit
/// la case et en déborde, recadrée au centre (grille) ; sinon elle y tient
/// entière.
#[allow(clippy::too_many_arguments)]
pub(super) fn tile_ui<'a>(ui: &egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, rect: Rect, cover: bool, tile: Tile<'a>, hot: &mut Hot<'a>) {
    let Some(images) = images else { return };
    if !ui.is_rect_visible(rect) {
        return;
    }
    let r = t.r_media();
    let p = ui.painter();
    let key = reveal_key(tile.image);
    let sens = if tile.sensitive { sensitive(ui.ctx()) } else { Sensitive::Show };
    let open = sens != Sensitive::Block && ((t.media_auto() && !tile.spoiler && sens == Sensitive::Show) || is_revealed(ui.ctx(), key));
    if !open {
        p.rect_filled(rect, r, t.colors.bg3);
        let label = match sens {
            Sensitive::Block => crate::i18n::table::ff::t::SENSITIVE_BLOCKED.get(lang),
            Sensitive::Blur => crate::i18n::table::ff::t::SENSITIVE.get(lang),
            Sensitive::Show if tile.spoiler => tx::SPOILER.get(lang),
            Sensitive::Show => tx::CLICK_TO_LOAD.get(lang),
        };
        badge(ui, t, rect, rect.center(), egui::Align2::CENTER_CENTER, label);
        if sens != Sensitive::Block {
            hot.add(rect, Action::Reveal(key), tile.alt);
        }
        return;
    }
    // Mode discret (lot E3) : média flouté jusqu'au survol ou au clic.
    let veil_key = super::stealth::reveal_key(key);
    let veil = super::stealth::veiled(ui, rect, is_revealed(ui.ctx(), veil_key));
    let state = match tile.fetch {
        Some(url) if tile.gif && veil.is_none() => {
            let play = super::motion::get(ui.ctx()).gifs.now(super::motion::hovered(ui, rect));
            images.get_media_moving(tile.image, url, request_size(rect.size(), cover, tile.width, tile.height), play)
        }
        Some(url) => images.get_media(tile.image, url, request_size(rect.size(), cover, tile.width, tile.height)),
        None => Image::Failed,
    };
    match state {
        Image::Ready(tex) => {
            let uv = if cover { cover_uv(rect.size(), tile.width, tile.height) } else { Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)) };
            if veil.is_some() {
                super::stealth::mosaic(ui, t, rect, tex, uv);
            } else {
                p.add(egui::Shape::Rect(egui::epaint::RectShape::filled(rect, r, egui::Color32::WHITE).with_texture(tex, uv)));
            }
        }
        Image::Pending => {
            p.rect_filled(rect, r, t.card_fill());
        }
        Image::Failed => {
            p.rect_filled(rect, r, t.card_fill());
            p.rect_stroke(rect, r, t.border(), egui::StrokeKind::Inside);
            if let Some(url) = tile.fetch {
                // Adresse peut-être expirée : redemandée après la liste (lot T13).
                super::invites::note_failed_url(ui.ctx(), url);
                let font = egui::TextStyle::Small.resolve(ui.style());
                p.text(rect.center(), egui::Align2::CENTER_CENTER, tx::IMAGE_UNAVAILABLE.get(lang), font, t.colors.faint);
            }
        }
    }
    if let Some(mode) = veil {
        // Au clic : le premier clic dévoile, le suivant ouvre.
        if mode == super::stealth::Blur::Click {
            hot.add(rect, Action::Reveal(veil_key), tile.alt);
        } else if let Some(click) = tile.click {
            hot.add(rect, click, tile.alt);
        }
        return;
    }
    if tile.play {
        let side = (Tokens::icon_side(t.typography.title) * 2.0).min(rect.width().min(rect.height()));
        let disc = Rect::from_center_size(rect.center(), Vec2::splat(side));
        p.circle_filled(disc.center(), side / 2.0, t.media_veil());
        icons::paint(Icon::Play, p, disc.shrink(side / 4.0), t.on_media());
    }
    if tile.gif {
        let pad = t.spacing.media_gap.max(t.shapes.border);
        badge(ui, t, rect, rect.left_top() + Vec2::splat(pad), egui::Align2::LEFT_TOP, tx::GIF_BADGE.get(lang));
    }
    // « Afficher les descriptions d'images » (Texte et images) : pastille « ALT », texte au survol.
    if tile.alt.is_some_and(|a| !a.is_empty()) && shown(ui.ctx()).descriptions {
        let pad = t.spacing.media_gap.max(t.shapes.border);
        badge(ui, t, rect, rect.left_bottom() + Vec2::new(pad, -pad), egui::Align2::LEFT_BOTTOM, tx::ALT_BADGE.get(lang));
    }
    if let Some(click) = tile.click {
        hot.add(rect, click, tile.alt);
    }
}

/// Pastille de texte court (« GIF », « SPOILER ») posée sur l'image `within` ;
/// omise si elle n'y tient pas (petite icône).
fn badge(ui: &egui::Ui, t: &Tokens, within: Rect, at: egui::Pos2, align: egui::Align2, text: &str) {
    let font = egui::TextStyle::Small.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(text.to_string(), font, t.on_media());
    let pad = Vec2::new(t.spacing.item_x / 2.0, t.spacing.item_y);
    let rect = align.anchor_size(at, galley.size() + 2.0 * pad);
    if !within.contains_rect(rect) {
        return;
    }
    ui.painter().rect_filled(rect, t.r_toast(), t.media_veil());
    ui.painter().galley(rect.min + pad, galley, t.on_media());
}

/// Taille à demander (points) : celle de la case, ou, en mode recadré, celle
/// qui la couvre en gardant la proportion de la source.
fn request_size(cell: Vec2, cover: bool, w: u32, h: u32) -> Vec2 {
    if !cover || w == 0 || h == 0 {
        return cell;
    }
    let (wf, hf) = (w as f32, h as f32);
    let scale = (cell.x / wf).max(cell.y / hf);
    Vec2::new((wf * scale).ceil(), (hf * scale).ceil())
}

/// Partie de la texture visible dans une case de `cell` (recadrage centré).
fn cover_uv(cell: Vec2, w: u32, h: u32) -> Rect {
    let full = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    if w == 0 || h == 0 || cell.x <= 0.0 || cell.y <= 0.0 {
        return full;
    }
    let src = w as f32 / h as f32;
    let dst = cell.x / cell.y;
    if src > dst {
        let f = dst / src;
        Rect::from_min_max(egui::pos2((1.0 - f) / 2.0, 0.0), egui::pos2((1.0 + f) / 2.0, 1.0))
    } else {
        let f = src / dst;
        Rect::from_min_max(egui::pos2(0.0, (1.0 - f) / 2.0), egui::pos2(1.0, (1.0 + f) / 2.0))
    }
}

/// Taille d'un fichier : « 12,1 Ko », « 3.0 MB ».
pub(super) fn file_size(lang: Lang, bytes: u64) -> String {
    const K: f64 = 1024.0;
    let b = bytes as f64;
    if b < K {
        f::size_bytes(lang, bytes)
    } else if b < K * K {
        f::size_kb(lang, decimal(lang, b / K, 1))
    } else if b < K * K * K {
        f::size_mb(lang, decimal(lang, b / (K * K), 1))
    } else {
        f::size_gb(lang, decimal(lang, b / (K * K * K), 1))
    }
}

/// Carte d'un fichier qui n'est pas une image : icône, nom (lien), taille,
/// bouton « Ouvrir ». Une seule hauteur, quelle que soit la longueur du nom.
pub(super) fn file_card<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, live: bool, a: &'a Attachment, hot: &mut Hot<'a>) {
    let icon = match a.kind {
        MediaKind::Video => Icon::Play,
        MediaKind::Audio => Icon::Audio,
        MediaKind::Image | MediaKind::File => Icon::File,
    };
    let kind = match a.kind {
        MediaKind::Video => Some(tx::VIDEO.get(lang)),
        MediaKind::Audio => Some(tx::AUDIO.get(lang)),
        MediaKind::Image | MediaKind::File => None,
    };
    let width = ui.available_width().min(t.layout.embed_width);
    let frame = t.card_frame();
    frame.show(ui, |ui| {
        ui.set_width((width - t.embed_margin().sum().x).max(1.0));
        ui.horizontal(|ui| {
            let side = Tokens::icon_side(t.typography.body) * 2.0;
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            icons::paint(icon, ui.painter(), icon_rect, t.colors.muted);
            // Bouton à droite, mesuré d'abord pour laisser le reste au nom.
            let font = egui::TextStyle::Button.resolve(ui.style());
            let label = ui.painter().layout_no_wrap(tx::OPEN.get(lang).to_string(), font, t.colors.button_text);
            let button = label.size() + 2.0 * ui.spacing().button_padding;
            let text_w = (ui.available_width() - button.x - t.spacing.item_x).max(1.0);
            let names = ui.vertical(|ui| {
                ui.set_width(text_w);
                let name = ui.add(egui::Label::new(RichText::new(a.display_name()).color(t.colors.link)).truncate());
                let size = file_size(lang, a.size);
                let detail = match kind {
                    Some(k) => format!("{k} · {size}"),
                    None => size,
                };
                ui.label(RichText::new(detail).small().color(t.colors.faint));
                name.rect
            });
            let (brect, _) = ui.allocate_exact_size(button, Sense::hover());
            let brect = Rect::from_center_size(egui::pos2(brect.center().x, icon_rect.center().y), button).round_to_pixels(ui.pixels_per_point());
            ui.painter().rect_filled(brect, t.r_row(), t.colors.button_bg);
            ui.painter().galley(brect.center() - label.size() / 2.0, label, t.colors.button_text);
            if live {
                let tip = Some(tx::OPEN_EXTERNAL_TIP.get(lang));
                hot.add(names.inner, Action::Open(&a.url), tip);
                hot.add(brect, Action::Open(&a.url), tip);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_box_keeps_ratio_and_bounds() {
        let max = Vec2::new(400.0, 300.0);
        assert_eq!(fit_box(1600, 900, max, 20.0), Vec2::new(400.0, 225.0));
        assert_eq!(fit_box(900, 1600, max, 20.0), Vec2::new(168.0, 300.0));
        assert_eq!(fit_box(100, 50, max, 20.0), Vec2::new(100.0, 50.0), "jamais agrandie");
        assert_eq!(fit_box(100_000, 1, max, 20.0), Vec2::new(400.0, 20.0), "côté minimal");
        assert_eq!(fit_box(0, 0, max, 20.0), Vec2::new(300.0, 300.0), "inconnue : place fixe");
        assert_eq!(fit_box(u32::MAX, u32::MAX, Vec2::new(10.0, 10.0), 20.0), Vec2::new(20.0, 20.0));
    }

    #[test]
    fn grid_geometry() {
        assert_eq!((grid_columns(2), grid_columns(3), grid_columns(4), grid_columns(7)), (2, 3, 2, 3));
        assert_eq!(grid_cell(520.0, 3, 4.0), 170.0);
        assert_eq!(grid_cell(10.0, 3, 8.0), 1.0);
    }

    #[test]
    fn cover_crops_the_long_side() {
        let uv = cover_uv(Vec2::splat(100.0), 200, 100);
        assert_eq!((uv.min.x, uv.max.x, uv.min.y, uv.max.y), (0.25, 0.75, 0.0, 1.0));
        let uv = cover_uv(Vec2::splat(100.0), 100, 400);
        assert_eq!((uv.min.y, uv.max.y), (0.375, 0.625));
        assert_eq!(request_size(Vec2::splat(100.0), true, 200, 100), Vec2::new(200.0, 100.0));
    }

    /// Source qui note les adresses d'images jointes demandées et ne sert rien.
    struct Rec(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl crate::images::Source for Rec {
        fn url(&self, _: &ImageRef, _: u32) -> String {
            "avatar".into()
        }
        fn media_url(&self, url: &str, width: u32, height: u32) -> String {
            let u = crate::net::cdn::media_url(url, width, height);
            if let Ok(mut v) = self.0.lock() {
                v.push(u.clone());
            }
            u
        }
        fn fetch(&self, _: String) -> crate::images::FetchFuture {
            Box::pin(async { Err(crate::net::cdn::FetchError::NotFound) })
        }
    }

    fn media_message() -> Message {
        let path = format!("{}/tests/fixtures/message_create_media.json", env!("CARGO_MANIFEST_DIR"));
        let v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}"));
        let events = crate::net::decode_dispatch("MESSAGE_CREATE", &v["d"].to_string());
        events
            .into_iter()
            .find_map(|e| match e {
                crate::state::Event::MessageCreate(m) => Some(m),
                _ => None,
            })
            .unwrap_or_else(|| panic!("message"))
    }

    /// Deux passes sans fenêtre sur 700 points ; renvoie la hauteur prise.
    fn pass(ctx: &egui::Context, images: &Images, f: impl Fn(&mut egui::Ui, &Images)) -> f32 {
        let mut h = 0.0;
        for _ in 0..2 {
            images.begin_frame();
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, Vec2::new(700.0, 4000.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let r = ui.scope(|ui| f(ui, images)).response.rect;
                h = r.height();
            });
            out.textures_delta.clear();
            images.end_frame();
        }
        h
    }

    /// Hauteur mesurée (sans images) = hauteur dessinée ; seules les copies
    /// du proxy de Discord sont demandées, à leur taille d'affichage ; un
    /// spoiler n'est demandé qu'une fois dévoilé ; « au clic » : rien.
    #[test]
    fn media_layout_and_requests() {
        use super::super::message::{measure_ui, message_ui};
        let doc = crate::state::markdown::Doc::default();
        let m = media_message();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let urls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let ctx = egui::Context::default();
        let images = Images::start(ctx.clone(), rt.handle(), std::sync::Arc::new(Rec(urls.clone())), None, crate::images::Config::default(), std::sync::Arc::new(|| {}));
        let taken = |urls: &std::sync::Arc<std::sync::Mutex<Vec<String>>>| urls.lock().map(|v| v.clone()).unwrap_or_default();

        let mut t = Tokens::default();
        t.layout.media_load = crate::theme::MediaLoad::Click;
        let drawn = pass(&ctx, &images, |ui, im| message_ui(ui, &t, Lang::Fr, im, &m, &doc, false, 0));
        let measured = pass(&ctx, &images, |ui, _| measure_ui(ui, &t, Lang::Fr, &m, &doc, false, 0));
        assert_eq!(drawn, measured);
        assert!(taken(&urls).is_empty(), "au clic : rien de demandé {:?}", taken(&urls));

        let t = Tokens::default();
        let drawn = pass(&ctx, &images, |ui, im| message_ui(ui, &t, Lang::Fr, im, &m, &doc, false, 0));
        let measured = pass(&ctx, &images, |ui, _| measure_ui(ui, &t, Lang::Fr, &m, &doc, false, 0));
        assert_eq!(drawn, measured, "même place avant et après le chargement");
        let got = taken(&urls);
        assert!(!got.is_empty());
        for u in &got {
            assert!(crate::net::cdn::CdnHosts::Discord.check(u).is_some(), "hôte hors de Discord : {u}");
            assert!(u.contains("width=") && u.contains("height="), "{u}");
            assert!(!u.contains("SPOILER_"), "spoiler demandé avant le clic : {u}");
        }
        assert!(got.iter().any(|u| u.contains("paysage-fictif.jpg")) && got.iter().any(|u| u.contains("danse.gif")));
        assert!(!got.iter().any(|u| u.contains("archive.zip")), "fichiers : jamais demandés comme images");
        // Vidéo : seule son affiche est demandée (première image, par le proxy média), jamais le fichier.
        let clips: Vec<&String> = got.iter().filter(|u| u.contains("clip.mp4")).collect();
        assert!(!clips.is_empty() && clips.iter().all(|u| u.starts_with("https://media.discordapp.net/") && u.contains("format=jpeg")), "{clips:?}");

        let spoiler = m.attachments.iter().find(|a| a.spoiler).map(|a| a.image_ref()).unwrap_or_else(|| panic!("spoiler"));
        reveal(&ctx, reveal_key(spoiler));
        pass(&ctx, &images, |ui, im| message_ui(ui, &t, Lang::Fr, im, &m, &doc, false, 0));
        assert!(taken(&urls).iter().any(|u| u.contains("SPOILER_surprise.png")));
    }

    /// Message transféré (retour d'utilisation : l'image jointe n'était qu'un
    /// lien « image.png ») : l'instantané montre son image en vignette,
    /// demandée au proxy média, sa carte d'embed et son autocollant, à la
    /// même place à la mesure et au dessin.
    #[test]
    fn forwarded_snapshot_shows_its_media() {
        use super::super::message::{measure_ui, message_ui};
        use crate::state::emoji::{StickerFormat, StickerItem};
        use crate::state::model::Embed;
        use crate::state::social::Forward;
        let picture = Attachment {
            id: 91,
            name: "capture-fictive.png".into(),
            url: "https://cdn.discordapp.com/attachments/8/91/capture-fictive.png?ex=0a&is=0b&hm=0c".into(),
            proxy_url: "https://media.discordapp.net/attachments/8/91/capture-fictive.png?ex=0a&is=0b&hm=0c".into(),
            content_type: Some("image/png".into()),
            width: 1600,
            height: 900,
            ..Attachment::default()
        }
        .analysed(0);
        let file = Attachment { id: 92, size: 2048, ..Attachment::file("notes-fictives.txt", "https://cdn.discordapp.com/attachments/8/92/notes-fictives.txt") }.analysed(0);
        let snapshot = |attachments: Vec<Attachment>, embeds: Vec<Embed>, stickers: Vec<StickerItem>| {
            let mut m = media_message();
            m.content.clear();
            m.attachments.clear();
            m.embeds.clear();
            m.meta.forward = Some(Box::new(Forward { content: "Texte de l'instantané".into(), attachments, embeds, stickers, ..Forward::default() }));
            m
        };
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
        let urls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let ctx = egui::Context::default();
        let images = Images::start(ctx.clone(), rt.handle(), std::sync::Arc::new(Rec(urls.clone())), None, crate::images::Config::default(), std::sync::Arc::new(|| {}));
        let t = Tokens::default();
        let doc = crate::state::markdown::Doc::default();
        let heights = |m: &Message| {
            let drawn = pass(&ctx, &images, |ui, im| message_ui(ui, &t, Lang::Fr, im, m, &doc, false, 0));
            let measured = pass(&ctx, &images, |ui, _| measure_ui(ui, &t, Lang::Fr, m, &doc, false, 0));
            assert_eq!(drawn, measured, "même place à la mesure et au dessin");
            drawn
        };
        let text_only = heights(&snapshot(Vec::new(), Vec::new(), Vec::new()));
        assert!(urls.lock().map(|u| u.is_empty()).unwrap_or(false), "rien à charger sans média");
        // Image : une vignette à sa proportion (550 × 309 au plus), pas une ligne de texte.
        let with_picture = heights(&snapshot(vec![picture.clone()], Vec::new(), Vec::new()));
        assert!(with_picture - text_only > 200.0, "vignette absente : {text_only} -> {with_picture}");
        let got = urls.lock().map(|u| u.clone()).unwrap_or_default();
        assert!(got.iter().any(|u| u.starts_with("https://media.discordapp.net/attachments/8/91/capture-fictive.png?") && u.contains("width=") && u.contains("height=")), "{got:?}");
        // Fichier, embed, autocollant : chacun prend sa place sous le texte.
        let with_file = heights(&snapshot(vec![picture.clone(), file], Vec::new(), Vec::new()));
        let with_embed = heights(&snapshot(vec![picture.clone()], vec![Embed::text(Some("Carte fictive"), Some("Texte de la carte"))], Vec::new()));
        let with_sticker = heights(&snapshot(vec![picture], Vec::new(), vec![StickerItem { id: 93, name: "Salut".into(), format: StickerFormat::Png }]));
        assert!(with_file > with_picture && with_embed > with_picture, "{with_picture} {with_file} {with_embed}");
        assert!(with_sticker - with_picture >= t.markdown.sticker, "{with_picture} {with_sticker}");
        assert!(!urls.lock().map(|u| u.iter().any(|x| x.contains("notes-fictives.txt"))).unwrap_or(true), "un fichier n'est jamais demandé comme image");
    }

    /// Réglages « Texte et images » : chaque case décochée retire sa part du
    /// message (pièces jointes en cartes, images des liens, embeds, réactions),
    /// et la mesure suit le dessin.
    #[test]
    fn text_and_images_settings_are_followed() {
        use super::super::message::{measure_ui, message_ui};
        assert_eq!(Shown::from_settings(&Default::default()), Shown::default(), "réglages pas encore reçus : tout est montré");
        let doc = crate::state::markdown::Doc::default();
        let mut m = media_message();
        m.meta.reactions.push(crate::state::social::Reaction { emoji: crate::state::social::Emoji::Unicode("👍".into()), normal: 2, burst: 0, me: false, me_burst: false });
        assert!(m.embeds.iter().any(|e| e.is_bare_media()) && m.embeds.iter().any(|e| !e.is_bare_media()));
        let t = Tokens::default();
        let ctx = egui::Context::default();
        let images = Images::disabled(ctx.clone());
        let height = |v: Shown| {
            set_shown(&ctx, v);
            let drawn = pass(&ctx, &images, |ui, im| message_ui(ui, &t, Lang::Fr, im, &m, &doc, false, 0));
            let measured = pass(&ctx, &images, |ui, _| measure_ui(ui, &t, Lang::Fr, &m, &doc, false, 0));
            assert_eq!(drawn, measured, "{v:?}");
            drawn
        };
        let all = height(Shown::default());
        let no_media = height(Shown { link_media: false, ..Default::default() });
        let no_embeds = height(Shown { embeds: false, ..Default::default() });
        assert!(no_embeds < no_media && no_media < all, "{no_embeds} < {no_media} < {all}");
        assert!(height(Shown { attachments: false, ..Default::default() }) < all, "images jointes réduites à leur carte");
        assert!(height(Shown { reactions: false, ..Default::default() }) < all, "réactions masquées");
    }

    #[test]
    fn file_sizes_by_language() {
        assert_eq!(file_size(Lang::Fr, 42), "42\u{a0}o");
        assert_eq!(file_size(Lang::En, 42), "42 B");
        assert_eq!(file_size(Lang::Fr, 12345), "12,1\u{a0}Ko");
        assert_eq!(file_size(Lang::En, 3 << 20), "3.0 MB");
        assert_eq!(file_size(Lang::En, 5 << 30), "5.0 GB");
    }
}
