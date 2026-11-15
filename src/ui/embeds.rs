//! Embeds d'un message : barre de couleur, fournisseur, auteur, titre
//! cliquable, description, champs (en ligne par trois au plus), image,
//! vignette, pied et horodatage ; aperçus réduits à leur média (image, GIF,
//! vidéo d'un lien collé). Une vidéo dont Discord sert une copie (lien
//! direct vers un fichier, GIF de Tenor en MP4) se lit sur place, par le
//! lecteur du fil ; celle que seul son site sait jouer (YouTube, Twitch)
//! garde sa vignette et un bouton de lecture qui ouvre le navigateur.
//!
//! Comme pour les pièces jointes, chaque image a sa place avant d'être
//! chargée (dimensions reçues, ou boîte fixe), et seules les copies servies
//! par le proxy de Discord sont téléchargées : jamais le site d'origine.

use super::attachments::{fit_box, tile_ui, Action, Hot, Tile};
use super::player::{Clip, Hit};
use super::viewer::ViewSrc;
use crate::images::Images;
use crate::i18n::{time, Lang};
use super::markdown::Zones;
use crate::state::markdown::Doc;
use crate::state::model::{Embed, EmbedKind, EmbedMedia, EmbedSlot, Message};
use crate::theme::{Clock, Tokens};
use eframe::egui::{self, Color32, Rect, RichText, Sense, Vec2};

/// Où trouver le texte mis en forme d'un embed : document du message
/// (`Doc::embeds`, analysé avec lui) et ses zones cliquables.
pub(super) struct EmbedText<'d> {
    pub doc: &'d Doc,
    pub zones: &'d mut Zones,
}

/// Texte d'embed (description : `slot` 0, valeur du champ n : 1 + n), mis en
/// forme comme un message ; texte simple si le document n'a pas ce passage.
#[allow(clippy::too_many_arguments)]
fn embed_text(ui: &mut egui::Ui, t: &Tokens, lang: Lang, msg: u64, index: usize, slot: usize, text: &str, et: &mut EmbedText) {
    match et.doc.embeds.iter().find(|(i, s, _)| *i == index && *s == slot) {
        Some((_, _, d)) => super::markdown::doc_ui(ui, t, lang, msg, d, &[], false, et.zones),
        None => {
            ui.add(egui::Label::new(RichText::new(text).color(t.colors.text)).wrap());
        }
    }
}

/// Embeds du message `m` ; `images` absent : mesure seule.
#[allow(clippy::too_many_arguments)]
pub(super) fn embeds_ui<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, m: &'a Message, doc: &Doc, zones: &mut Zones, hot: &mut Hot<'a>) {
    // Réglages « Texte et images » : aperçus des liens, puis images et vidéos des liens.
    let shown = super::attachments::shown(ui.ctx());
    if !shown.embeds {
        return;
    }
    let mut et = EmbedText { doc, zones };
    for (i, e) in m.embeds.iter().enumerate() {
        if e.is_bare_media() && !shown.link_media {
            continue;
        }
        if e.is_bare_media() {
            bare_media(ui, t, lang, images, m.id, i, e, hot);
        } else {
            card(ui, t, lang, images, m.id, i, e, &mut et, hot);
        }
    }
}

/// Vidéo de l'embed que le lecteur intégré sait jouer : Discord en sert une
/// copie (`video.proxy_url` sur un de ses hôtes). Un GIF en vidéo est lu en
/// boucle et sans son.
pub(super) fn clip<'a>(msg: u64, index: usize, e: &'a Embed) -> Option<Clip<'a>> {
    if !matches!(e.kind, EmbedKind::Video | EmbedKind::Gifv) {
        return None;
    }
    let url = e.video.as_ref()?.proxy_url.as_deref()?;
    crate::net::cdn::served_by_discord(url).then(|| Clip::embed(msg, index, url, e.kind == EmbedKind::Gifv))
}

/// Clic sur l'image d'un aperçu : visionneuse pour une image ; pour une
/// vidéo ou un GIF, lecture sur place si Discord en sert le fichier, sinon
/// navigateur (lecture hors de l'application).
fn media_click<'a>(e: &'a Embed, msg: u64, index: usize, media: &'a EmbedMedia, image: crate::state::model::ImageRef) -> Option<Action<'a>> {
    match e.kind {
        EmbedKind::Video | EmbedKind::Gifv => match clip(msg, index, e) {
            Some(c) => Some(Action::Media(Hit::Start(c))),
            None => e.url.as_deref().or(e.video.as_ref().map(|v| v.url.as_str())).map(Action::OpenDirect),
        },
        _ => media.proxy_url.as_deref().map(|fetch| {
            Action::View(ViewSrc { image, fetch, original: &media.url, width: media.width, height: media.height, alt: None, name: e.title.as_deref().unwrap_or(&media.url) })
        }),
    }
}

fn tile<'a>(e: &'a Embed, msg: u64, index: usize, slot: EmbedSlot, media: &'a EmbedMedia, clickable: bool) -> Tile<'a> {
    let image = Embed::image_ref(msg, index, slot, media);
    let video = matches!(e.kind, EmbedKind::Video) && slot != EmbedSlot::Author && slot != EmbedSlot::Footer;
    Tile {
        image,
        fetch: media.proxy_url.as_deref(),
        width: media.width,
        height: media.height,
        spoiler: false,
        sensitive: false,
        gif: e.kind == EmbedKind::Gifv || media.animated,
        play: video,
        click: if clickable { media_click(e, msg, index, media, image) } else { None },
        alt: None,
    }
}

/// Grand média d'un embed dans `rect` : la vidéo en cours si le lecteur la
/// tient, sinon la vignette. `still` : faux quand `media` est la vidéo
/// elle-même (aucune vignette reçue) : rien n'est alors demandé comme image.
#[allow(clippy::too_many_arguments)]
fn media_tile<'a>(ui: &egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, rect: Rect, msg: u64, index: usize, slot: EmbedSlot, e: &'a Embed, media: &'a EmbedMedia, still: bool, hot: &mut Hot<'a>) {
    if images.is_some()
        && ui.is_rect_visible(rect)
        && let Some(c) = clip(msg, index, e)
        && super::player::live_tile(ui, t, lang, rect, c, hot)
    {
        return;
    }
    let mut tile = tile(e, msg, index, slot, media, true);
    if matches!(tile.click, Some(Action::OpenDirect(_))) {
        tile.alt = Some(crate::i18n::table::messages::t::WATCH_IN_BROWSER.get(lang));
    }
    if !still {
        tile.fetch = None;
        tile.play = true;
        tile.gif = false;
    }
    tile_ui(ui, t, lang, images, rect, false, tile, hot);
}

/// Aperçu fait d'un seul média : l'image, sans cadre.
#[allow(clippy::too_many_arguments)]
fn bare_media<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, msg: u64, index: usize, e: &'a Embed, hot: &mut Hot<'a>) {
    let playable = clip(msg, index, e).is_some();
    let (slot, media, still) = match (&e.thumbnail, &e.image, &e.video) {
        (Some(m), _, _) => (EmbedSlot::Thumbnail, m, true),
        (None, Some(m), _) => (EmbedSlot::Image, m, true),
        // Vidéo lisible ici sans vignette : une case sombre et son bouton de lecture.
        (None, None, Some(v)) if playable => (EmbedSlot::Image, v, false),
        _ => return,
    };
    // Sans copie du proxy, rien à montrer : le lien est déjà dans le texte.
    if media.proxy_url.is_none() && !playable {
        return;
    }
    ui.add_space(t.spacing.media_gap);
    let max = Vec2::new(ui.available_width().min(t.layout.media_width), t.layout.media_height);
    let size = fit_box(media.width, media.height, max, Tokens::icon_side(t.typography.body) * 2.0);
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    media_tile(ui, t, lang, images, rect, msg, index, slot, e, media, still, hot);
}

/// Petite icône (auteur, pied) : carrée, du côté d'une ligne de texte.
fn small_icon<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, tile: Tile<'a>, hot: &mut Hot<'a>) {
    let side = Tokens::icon_side(t.typography.body);
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    tile_ui(ui, t, lang, images, rect, true, tile, hot);
}

/// Texte cliquable s'il a une adresse.
fn link_label<'a>(ui: &mut egui::Ui, text: RichText, url: Option<&'a str>, live: bool, hot: &mut Hot<'a>) {
    let resp = ui.add(egui::Label::new(text).wrap());
    if let (Some(url), true) = (url, live) {
        hot.add(resp.rect, Action::Open(url), Some(url));
    }
}

/// Couleur de la barre : celle de l'embed (donnée du message), sinon la bordure du thème.
fn bar_color(t: &Tokens, e: &Embed) -> Color32 {
    match e.color {
        Some(c) if c != 0 => Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8),
        _ => t.colors.border,
    }
}

#[allow(clippy::too_many_arguments)]
fn card<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, msg: u64, index: usize, e: &'a Embed, et: &mut EmbedText, hot: &mut Hot<'a>) {
    ui.add_space(t.spacing.media_gap);
    let live = images.is_some();
    let width = ui.available_width().min(t.layout.embed_width);
    let bar = t.shapes.embed_bar;
    let mut margin = t.embed_card_margin();
    margin.left = margin.left.saturating_add(bar.round().clamp(0.0, f32::from(i8::MAX)) as i8);
    let inner_w = (width - margin.sum().x).max(1.0);
    let gap = t.spacing.item_x;
    // Vignette à droite (articles, liens, embeds riches) ; une vidéo la montre en grand.
    let side_thumb = e.thumbnail.as_ref().filter(|_| e.kind != EmbedKind::Video);
    let big = match (e.kind, &e.image, &e.thumbnail, &e.video) {
        (_, Some(img), _, _) => Some((EmbedSlot::Image, img, true)),
        (EmbedKind::Video, None, Some(th), _) => Some((EmbedSlot::Thumbnail, th, true)),
        (EmbedKind::Video, None, None, Some(v)) if clip(msg, index, e).is_some() => Some((EmbedSlot::Image, v, false)),
        _ => None,
    };
    let thumb = t.layout.embed_thumb.min(inner_w / 2.0).floor();
    let frame = egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_embed()).inner_margin(margin);
    let resp = frame.show(ui, |ui| {
        ui.set_width(inner_w);
        let text_w = if side_thumb.is_some() { (inner_w - thumb - gap).max(1.0) } else { inner_w };
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(text_w);
                if let Some(p) = &e.provider {
                    link_label(ui, RichText::new(&p.name).small().color(t.colors.muted), p.url.as_deref(), live, hot);
                }
                if let Some(a) = &e.author {
                    ui.horizontal(|ui| {
                        if let Some(icon) = &a.icon {
                            small_icon(ui, t, lang, images, tile(e, msg, index, EmbedSlot::Author, icon, false), hot);
                        }
                        link_label(ui, RichText::new(&a.name).size(t.typography.button).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong), a.url.as_deref(), live, hot);
                    });
                }
                if let Some(title) = &e.title {
                    let color = if e.url.is_some() { t.colors.link } else { t.colors.text_strong };
                    link_label(ui, RichText::new(title).family(crate::ui::fonts::demi()).strong().color(color), e.url.as_deref(), live, hot);
                }
                if let Some(d) = &e.description {
                    embed_text(ui, t, lang, msg, index, 0, d, et);
                }
                fields(ui, t, lang, msg, index, e, et);
            });
            if let Some(th) = side_thumb {
                let size = fit_box(th.width, th.height, Vec2::splat(thumb), 1.0);
                let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                tile_ui(ui, t, lang, images, rect, false, tile(e, msg, index, EmbedSlot::Thumbnail, th, true), hot);
            }
        });
        if let Some((slot, media, still)) = big {
            let max = Vec2::new(inner_w, t.layout.media_height);
            let size = fit_box(media.width, media.height, max, Tokens::icon_side(t.typography.body) * 2.0);
            let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
            media_tile(ui, t, lang, images, rect, msg, index, slot, e, media, still, hot);
        }
        footer(ui, t, lang, images, msg, index, e, hot);
    });
    // Barre de couleur à gauche, dans les coins arrondis du cadre.
    let r = t.r_embed();
    let rect = resp.response.rect;
    let bar_rect = Rect::from_min_size(rect.min, Vec2::new(bar.min(rect.width()), rect.height()));
    // La carte entière, découpée à la largeur de la barre : la barre épouse l'arrondi.
    ui.painter().with_clip_rect(bar_rect.intersect(ui.clip_rect())).rect_filled(rect, r, bar_color(t, e));
}

/// Champs : ceux « en ligne » côte à côte (trois au plus par rangée), les
/// autres sur toute la largeur.
#[allow(clippy::too_many_arguments)]
fn fields(ui: &mut egui::Ui, t: &Tokens, lang: Lang, msg: u64, index: usize, e: &Embed, et: &mut EmbedText) {
    let mut i = 0;
    while i < e.fields.len() {
        let run = if e.fields[i].inline { e.fields[i..].iter().take(3).take_while(|f| f.inline).count() } else { 1 };
        let row = &e.fields[i..i + run];
        ui.columns(run, |cols| {
            for (k, (col, f)) in cols.iter_mut().zip(row).enumerate() {
                // Colonnes d'egui en mise en page justifiée : le texte y serait étiré.
                col.vertical(|ui| {
                    ui.add(egui::Label::new(RichText::new(&f.name).size(t.typography.button).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).wrap());
                    embed_text(ui, t, lang, msg, index, i + k + 1, &f.value, et);
                });
            }
        });
        i += run;
    }
}

#[allow(clippy::too_many_arguments)]
fn footer<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, msg: u64, index: usize, e: &'a Embed, hot: &mut Hot<'a>) {
    let when = e.timestamp.map(|ms| time::clock(lang, ms, true, t.layout.clock == Clock::H24));
    let text = match (&e.footer, when) {
        (Some(f), Some(w)) => format!("{} · {w}", f.text),
        (Some(f), None) => f.text.clone(),
        (None, Some(w)) => w,
        (None, None) => return,
    };
    ui.horizontal(|ui| {
        if let Some(icon) = e.footer.as_ref().and_then(|f| f.icon.as_ref()) {
            small_icon(ui, t, lang, images, tile(e, msg, index, EmbedSlot::Footer, icon, false), hot);
        }
        ui.add(egui::Label::new(RichText::new(text).small().color(t.colors.faint)).wrap());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn media(url: &str, proxy: Option<&str>) -> EmbedMedia {
        EmbedMedia::new(url.into(), proxy.map(Into::into), 320, 180, 0)
    }

    /// Lecture sur place seulement quand Discord sert le fichier ; sinon le navigateur, sans confirmation.
    #[test]
    fn videos_des_embeds_lues_ici_ou_dans_le_navigateur() {
        let proxied = "https://images-ext-1.discordapp.net/external/sig/https/gif.exemple.invalid/chat.mp4";
        let gif = Embed { kind: EmbedKind::Gifv, url: Some("https://gif.exemple.invalid/vue".into()), thumbnail: Some(media("https://gif.exemple.invalid/chat.png", Some("https://images-ext-1.discordapp.net/external/sig/https/gif.exemple.invalid/chat.png"))), video: Some(media("https://gif.exemple.invalid/chat.mp4", Some(proxied))), ..Embed::default() };
        let c = clip(7, 0, &gif).unwrap_or_else(|| panic!("GIF en vidéo"));
        assert!(c.gif && c.video && c.url == proxied && c.mime == Some("video/mp4"));
        assert_ne!(c.id, clip(7, 1, &gif).map_or(0, |c| c.id), "une clé par embed");
        let image = Embed::image_ref(7, 0, EmbedSlot::Thumbnail, gif.thumbnail.as_ref().unwrap_or_else(|| panic!("vignette")));
        assert!(matches!(media_click(&gif, 7, 0, gif.thumbnail.as_ref().unwrap_or_else(|| panic!("vignette")), image), Some(Action::Media(Hit::Start(_)))));
        // Lien direct vers un fichier : mêmes règles, avec les commandes (pas un GIF) ; WebM reconnu.
        let direct = Embed { kind: EmbedKind::Video, video: Some(media("https://f.exemple.invalid/a.webm", Some("https://media.discordapp.net/external/sig/https/f.exemple.invalid/a.webm?x=1"))), ..Embed::default() };
        assert!(clip(7, 0, &direct).is_some_and(|c| !c.gif && c.mime == Some("video/webm")));
        // Site de vidéos : pas de copie chez Discord, ou copie annoncée sur un autre hôte.
        let site = Embed { kind: EmbedKind::Video, url: Some("https://video.exemple.invalid/regarder".into()), thumbnail: gif.thumbnail.clone(), video: Some(media("https://video.exemple.invalid/embed/1", None)), ..Embed::default() };
        assert!(clip(7, 0, &site).is_none());
        assert!(matches!(media_click(&site, 7, 0, site.thumbnail.as_ref().unwrap_or_else(|| panic!("vignette")), image), Some(Action::OpenDirect("https://video.exemple.invalid/regarder"))));
        let foreign = Embed { video: Some(media("https://x.invalid/a.mp4", Some("https://media.tenor.com/a.mp4"))), ..gif.clone() };
        assert!(clip(7, 0, &foreign).is_none(), "seuls les hôtes de Discord");
        let insecure = Embed { video: Some(media("https://x.invalid/a.mp4", Some("http://cdn.discordapp.com/a.mp4"))), ..gif.clone() };
        assert!(clip(7, 0, &insecure).is_none());
        // Une image n'est jamais une vidéo, même avec un champ `video`.
        let picture = Embed { kind: EmbedKind::Image, ..gif.clone() };
        assert!(clip(7, 0, &picture).is_none());
    }
}
