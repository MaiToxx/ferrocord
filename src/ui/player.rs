//! Lecteur vidéo et audio dans le fil (phase 10, lot M) : une vidéo jointe
//! se joue à sa place, un son dans sa carte. Rien n'est téléchargé avant le
//! clic, sauf l'affiche d'une vidéo (sa première image, servie par le proxy
//! média de Discord, comme une vignette) ; un seul fichier à la fois. Les
//! vidéos des embeds (lien direct, GIF en vidéo) passent par le même lecteur
//! quand Discord en sert une copie (lot VID).
//!
//! Dessiné comme les autres pièces jointes, sans accès à l'application :
//! l'état ([`Active`]) vit dans la mémoire d'egui, les demandes qui exigent
//! l'orchestrateur (ouvrir, fermer) y sont déposées et [`tick`] les exécute.
//! Le reste (pause, position, volume) va droit à la lecture.
//!
//! Redessins : en lecture vidéo, un par image décodée (le fil du lecteur
//! réveille l'interface) ; en lecture audio, quatre par seconde pour
//! l'heure ; en pause, à la fin ou sans lecteur, aucun. La mise en page ne
//! dépend pas de l'état : mêmes hauteurs avant, pendant et après la lecture.

use super::attachments::{fit_box, Action, Hot};
use super::voice_note::clock;
use crate::i18n::table::messages::{f as mf, t as mt};
use crate::i18n::{t as tx, Lang};
use crate::images::{Image, Images};
use crate::state::model::{Attachment, MediaKind};
use crate::state::voice_link::VoiceLink;
use crate::theme::{icons, icons::Icon, Tokens};
use crate::video::play::{Handle, Phase, Snapshot, MAX_BYTES};
use eframe::egui::{self, pos2, Color32, Id, Rect, Sense, Shape, Stroke, Vec2};
use std::time::Duration;

/// Rythme de l'heure affichée pendant une lecture sans image.
const AUDIO_TICK: Duration = Duration::from_millis(250);
/// Passes sans dessin de la vidéo avant sa mise en pause (salon quitté, message hors de vue).
const UNSEEN_PASSES: u64 = 3;
/// Proportion d'une vidéo dont Discord n'a pas donné les dimensions.
const DEFAULT_SIZE: (u32, u32) = (16, 9);

/// Types joués ici. Le WebM est démultiplexé par `video::mkv` (lot VID).
const PLAYABLE: &[&str] = &["video/webm", "audio/webm", "video/mp4", "video/quicktime", "audio/mpeg", "audio/mp3", "audio/ogg", "audio/wav", "audio/x-wav", "audio/wave", "audio/mp4", "audio/x-m4a", "audio/m4a", "audio/aac", "audio/flac", "audio/x-flac"];

/// Type du fichier : celui que Discord annonce, sinon d'après l'extension.
pub(super) fn mime_of(a: &Attachment) -> Option<&str> {
    if let Some(m) = a.content_type.as_deref().and_then(|c| c.split(';').next()).map(str::trim).filter(|m| !m.is_empty()) {
        return Some(m);
    }
    let ext = a.name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "mov" => "video/quicktime",
        "webm" => "video/webm",
        "weba" => "audio/webm",
        "mp3" => "audio/mpeg",
        "ogg" | "oga" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "aac" => "audio/aac",
        "flac" => "audio/flac",
        _ => return None,
    })
}

/// Vidéo ou son que le lecteur intégré sait jouer (sinon : carte « Ouvrir »).
pub(super) fn playable(a: &Attachment) -> bool {
    matches!(a.kind, MediaKind::Video | MediaKind::Audio) && a.voice.is_none() && a.size <= MAX_BYTES as u64 && mime_of(a).is_some_and(|m| PLAYABLE.iter().any(|p| p.eq_ignore_ascii_case(m)))
}

/// Ce que le lecteur joue : une pièce jointe, ou la vidéo d'un embed.
#[derive(Clone, Copy)]
pub(super) struct Clip<'a> {
    /// Clé de la lecture (identifiant de la pièce jointe, ou empreinte de l'embed).
    pub id: u64,
    /// Adresse téléchargée : toujours un hôte de Discord (vérifié au téléchargement).
    pub url: &'a str,
    pub mime: Option<&'a str>,
    pub video: bool,
    /// GIF en vidéo (Tenor…) : en boucle, sans son ni commandes.
    pub gif: bool,
}

impl<'a> Clip<'a> {
    pub(super) fn of(a: &'a Attachment) -> Self {
        Clip { id: a.id, url: &a.url, mime: mime_of(a), video: a.kind == MediaKind::Video, gif: false }
    }

    /// Vidéo du `index`-ième embed du message `msg`, servie par Discord à `url`.
    pub(super) fn embed(msg: u64, index: usize, url: &'a str, gif: bool) -> Self {
        let path = url.split(['?', '#']).next().unwrap_or(url).to_ascii_lowercase();
        let mime = if path.ends_with(".webm") {
            "video/webm"
        } else if path.ends_with(".mov") {
            "video/quicktime"
        } else {
            "video/mp4"
        };
        Clip { id: Id::unique(("vidéo d'embed", msg, index)).value(), url, mime: Some(mime), video: true, gif }
    }
}

/// Le fichier en cours (chargement, lecture, pause, fin ou échec).
#[derive(Clone)]
struct Active {
    /// Identifiant de la pièce jointe.
    id: u64,
    /// `None` : pas de lecteur (aperçu, tests).
    handle: Option<Handle>,
    tex: Option<egui::TextureHandle>,
    /// Numéro de la dernière image reçue.
    seen: u64,
    muted: bool,
    volume: f32,
    /// Vidéo agrandie à toute la fenêtre.
    full: bool,
    video: bool,
    /// GIF en vidéo : pas de commandes.
    gif: bool,
    /// Position visée pendant un glissement sur la barre (0 à 1).
    drag: Option<f32>,
    /// Dernière passe où la vidéo a été dessinée.
    drawn: u64,
}

/// Demande à l'orchestrateur.
#[derive(Clone)]
enum Cmd {
    Open { id: u64, url: String, mime: Option<String>, video: bool, gif: bool },
    Close,
}

/// Zone sensible du lecteur.
#[derive(Clone, Copy)]
pub(super) enum Hit<'a> {
    /// Charge et joue ce fichier.
    Start(Clip<'a>),
    /// Lecture ou pause.
    Toggle,
    Seek,
    Mute,
    Volume,
    Full,
    Close,
}

impl Hit<'_> {
    /// Zone qui se glisse (barres de position et de volume).
    pub(super) fn drags(&self) -> bool {
        matches!(self, Hit::Seek | Hit::Volume)
    }
}

fn id() -> Id {
    Id::unique("ferrocord-lecteur")
}

fn cmd_id() -> Id {
    Id::unique("ferrocord-lecteur-demandes")
}

fn active(ctx: &egui::Context) -> Option<Active> {
    ctx.data(|d| d.get_temp::<Active>(id()))
}

fn update(ctx: &egui::Context, f: impl FnOnce(&mut Active)) {
    if let Some(mut a) = active(ctx) {
        f(&mut a);
        ctx.data_mut(|d| d.insert_temp(id(), a));
    }
}

fn push(ctx: &egui::Context, c: Cmd) {
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<Cmd>>(cmd_id()).push(c));
    ctx.request_repaint();
}

/// Pose une lecture toute faite (aperçu, tests) : `handle` figé, sans orchestrateur.
pub fn preview(ctx: &egui::Context, attachment: u64, handle: Handle, video: bool, full: bool) {
    let a = Active { id: attachment, handle: Some(handle), tex: None, seen: 0, muted: false, volume: 1.0, full, video, gif: false, drag: None, drawn: ctx.cumulative_pass_nr() };
    ctx.data_mut(|d| d.insert_temp(id(), a));
}

/// Une fois par image, après le dessin : demandes à l'orchestrateur, image
/// décodée à montrer, réveil pour l'heure d'un son.
pub fn tick(ctx: &egui::Context, link: &mut dyn VoiceLink) {
    let cmds = ctx.data_mut(|d| d.remove_temp::<Vec<Cmd>>(cmd_id())).unwrap_or_default();
    for c in cmds {
        match c {
            Cmd::Open { id: attachment, url, mime, video, gif } => {
                let volume = active(ctx).map_or(1.0, |a| a.volume);
                let handle = link.open_media(&url, mime.as_deref(), gif);
                if let Some(h) = &handle {
                    h.set_volume(volume);
                }
                let a = Active { id: attachment, handle, tex: None, seen: 0, muted: false, volume, full: false, video, gif, drag: None, drawn: ctx.cumulative_pass_nr() };
                ctx.data_mut(|d| d.insert_temp(id(), a));
            }
            Cmd::Close => {
                link.close_media();
                ctx.data_mut(|d| d.remove::<Active>(id()));
            }
        }
    }
    let Some(mut a) = active(ctx) else { return };
    let Some(h) = a.handle.clone() else { return };
    if let Some((generation, pic)) = h.take_picture(a.seen) {
        // Alpha plein partout : les octets sont déjà « prémultipliés ».
        let image = egui::ColorImage::from_rgba_premultiplied([pic.width as usize, pic.height as usize], &pic.pixels);
        match a.tex.as_mut() {
            Some(t) => t.set(image, egui::TextureOptions::LINEAR),
            None => a.tex = Some(ctx.load_texture("ferrocord-lecteur", image, egui::TextureOptions::LINEAR)),
        }
        a.seen = generation;
        ctx.data_mut(|d| d.insert_temp(id(), a.clone()));
    }
    let snap = h.snapshot();
    if snap.phase == Phase::Playing {
        if a.video && !a.full && ctx.cumulative_pass_nr().saturating_sub(a.drawn) > UNSEEN_PASSES {
            h.pause();
        } else if snap.info.video.is_none() {
            ctx.request_repaint_after(AUDIO_TICK);
        }
    }
}

// ---------------------------------------------------------------- dessin

#[derive(Clone, Copy, PartialEq, Eq)]
enum Glyph {
    Play,
    Pause,
    Sound,
    Muted,
    Full,
    Close,
}

fn glyph(p: &egui::Painter, rect: Rect, g: Glyph, color: Color32) {
    let c = rect.center();
    let u = rect.width().min(rect.height()) * 0.5;
    let at = |x: f32, y: f32| pos2(c.x + x * u, c.y + y * u);
    let line = Stroke::new((u * 0.16).max(1.0), color);
    match g {
        Glyph::Play => icons::paint(Icon::Play, p, rect, color),
        Glyph::Pause => icons::paint(Icon::Pause, p, rect, color),
        Glyph::Sound => icons::paint(Icon::Speaker, p, rect, color),
        // Haut-parleur coupé : pas dans le jeu du thème.
        Glyph::Muted => {
            p.rect_filled(Rect::from_min_max(at(-0.75, -0.22), at(-0.4, 0.22)), 0.0, color);
            p.add(Shape::convex_polygon(vec![at(-0.45, -0.22), at(0.0, -0.62), at(0.0, 0.62), at(-0.45, 0.22)], color, Stroke::NONE));
                p.line_segment([at(0.28, -0.28), at(0.78, 0.28)], line);
                p.line_segment([at(0.28, 0.28), at(0.78, -0.28)], line);
        }
        Glyph::Full => icons::paint(Icon::Fullscreen, p, rect, color),
        Glyph::Close => icons::paint(Icon::Close, p, rect.shrink(u * 0.2), color),
    }
}

/// Hauteur de la barre de commandes (et côté de ses boutons).
fn bar_height(t: &Tokens) -> f32 {
    Tokens::icon_side(t.typography.body) + 2.0 * t.spacing.item_y
}

/// Piste (position ou volume) remplie jusqu'à `frac`, avec son repère.
fn track(p: &egui::Painter, zone: Rect, frac: f32, t: &Tokens, back: Color32) {
    let h = (zone.height() * 0.14).max(2.0);
    let line = Rect::from_center_size(zone.center(), Vec2::new(zone.width(), h));
    p.rect_filled(line, h / 2.0, back);
    let x = line.left() + line.width() * frac.clamp(0.0, 1.0);
    p.rect_filled(Rect::from_min_max(line.min, pos2(x, line.bottom())), h / 2.0, t.colors.accent);
    p.circle_filled(pos2(x, line.center().y), h * 1.6, t.colors.accent);
}

/// Position montrée (µs) et sa fraction de la durée.
fn position(a: &Active, s: &Snapshot) -> (u64, f32) {
    let d = s.info.duration_us;
    match a.drag {
        Some(f) if d > 0 => ((f64::from(f) * d as f64) as u64, f),
        _ if d > 0 => (s.pos_us.min(d), s.pos_us.min(d) as f32 / d as f32),
        _ => (s.pos_us, 0.0),
    }
}

fn time_text(pos_us: u64, duration_us: u64) -> String {
    let ms = |us: u64| u32::try_from(us / 1000).unwrap_or(u32::MAX);
    if duration_us == 0 {
        return clock(ms(pos_us));
    }
    format!("{} / {}", clock(ms(pos_us)), clock(ms(duration_us)))
}

/// Couleurs d'une barre de commandes : icônes et texte, fond des pistes.
#[derive(Clone, Copy)]
struct Ink {
    fore: Color32,
    track: Color32,
}

/// Dessine la barre de commandes dans `bar` et déclare ses zones. Les
/// éléments qui ne tiennent pas (heure, volume) sont omis.
#[allow(clippy::too_many_arguments)]
fn controls<'a>(p: &egui::Painter, t: &Tokens, lang: Lang, bar: Rect, a: &Active, s: &Snapshot, ink: Ink, add: &mut dyn FnMut(Rect, Hit<'a>, &'static str)) {
    let side = bar.height();
    let gap = t.spacing.item_x;
    let square = |x: f32| Rect::from_min_size(pos2(x, bar.top()), Vec2::splat(side));
    let icon = |r: Rect| r.shrink(side * 0.22);
    let playing = s.phase == Phase::Playing;
    // À gauche : lecture ou pause, puis l'heure.
    let toggle = square(bar.left());
    glyph(p, icon(toggle), if playing { Glyph::Pause } else { Glyph::Play }, ink.fore);
    add(toggle, Hit::Toggle, if playing { mt::PAUSE.get(lang) } else { mt::PLAY.get(lang) });
    let mut left = toggle.right() + gap;
    // À droite : fermer, agrandir, volume.
    let mut right = bar.right();
    let close = square(right - side);
    glyph(p, icon(close), Glyph::Close, ink.fore);
    add(close, Hit::Close, mt::CLOSE_PLAYER.get(lang));
    right = close.left();
    if a.video {
        let full = square(right - side);
        glyph(p, icon(full), Glyph::Full, ink.fore);
        add(full, Hit::Full, if a.full { mt::EXIT_FULLSCREEN.get(lang) } else { mt::FULLSCREEN.get(lang) });
        right = full.left();
    }
    let room = right - left;
    if s.info.audio || s.phase == Phase::Loading {
        if room > side * 9.0 {
            let vol = Rect::from_min_max(pos2(right - side * 2.5, bar.top()), pos2(right - gap, bar.bottom()));
            track(p, vol, if a.muted { 0.0 } else { a.volume }, t, ink.track);
            add(vol, Hit::Volume, mt::VOLUME.get(lang));
            right = vol.left() - gap;
        }
        let mute = square(right - side);
        glyph(p, icon(mute), if a.muted || a.volume == 0.0 { Glyph::Muted } else { Glyph::Sound }, ink.fore);
        add(mute, Hit::Mute, if a.muted { mt::UNMUTE.get(lang) } else { mt::MUTE.get(lang) });
        right = mute.left();
    }
    let (pos_us, frac) = position(a, s);
    if room > side * 6.0 {
        let font = egui::FontId::proportional(t.typography.small);
        let time = p.layout_no_wrap(time_text(pos_us, s.info.duration_us), font, ink.fore);
        p.galley(pos2(left, bar.center().y - time.size().y / 2.0), time.clone(), ink.fore);
        left += time.size().x + gap;
    }
    let seek = Rect::from_min_max(pos2(left + gap / 2.0, bar.top()), pos2(right - gap, bar.bottom()));
    if seek.width() > side {
        track(p, seek, frac, t, ink.track);
        if s.info.duration_us > 0 {
            add(seek, Hit::Seek, mt::SEEK.get(lang));
        }
    }
}

/// Texte d'état centré (chargement, échec), sur un fond lisible.
fn notice(ui: &egui::Ui, t: &Tokens, within: Rect, text: &str) {
    let font = egui::FontId::proportional(t.typography.small);
    let width = (within.width() - 4.0 * t.spacing.item_x).max(1.0);
    let galley = ui.painter().layout(text.to_string(), font, t.on_media(), width);
    let pad = Vec2::new(t.spacing.item_x, t.spacing.item_y);
    let rect = Rect::from_center_size(within.center(), galley.size() + 2.0 * pad);
    ui.painter().rect_filled(rect, t.r_toast(), t.media_veil());
    ui.painter().galley(rect.min + pad, galley, t.on_media());
}

/// L'image de la vidéo dans `rect`, tournée de `turn` degrés.
fn picture(p: &egui::Painter, rect: Rect, tex: &egui::TextureHandle, turn: u16, radius: egui::CornerRadius) {
    let full = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    if turn.is_multiple_of(360) {
        p.add(Shape::Rect(egui::epaint::RectShape::filled(rect, radius, Color32::WHITE).with_texture(tex.id(), full)));
        return;
    }
    // Coins de la texture vus aux coins haut-gauche, haut-droit, bas-droit, bas-gauche.
    let uv = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    let shift = match turn % 360 {
        90 => 3,
        180 => 2,
        _ => 1,
    };
    let mut mesh = egui::Mesh::with_texture(tex.id());
    for (i, corner) in [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()].into_iter().enumerate() {
        mesh.vertices.push(egui::epaint::Vertex { pos: corner, uv: uv[(i + shift) % 4], color: Color32::WHITE });
    }
    mesh.indices.extend_from_slice(&[0, 1, 2, 0, 2, 3]);
    p.add(Shape::mesh(mesh));
}

/// Taille d'une image de `size` pixels tournée de `turn` degrés.
fn turned(size: [usize; 2], turn: u16) -> Vec2 {
    let v = Vec2::new(size[0] as f32, size[1] as f32);
    if turn % 180 == 90 { Vec2::new(v.y, v.x) } else { v }
}

/// Dit au lecteur la taille affichée (pixels) : il y réduit ses images.
fn tell_shown(ctx: &egui::Context, h: &Handle, size: Vec2) {
    let px = size * ctx.pixels_per_point();
    h.set_shown(px.x.ceil().max(1.0) as u32, px.y.ceil().max(1.0) as u32);
}

/// Pièce jointe vidéo ou audio jouable. `images` absent : mesure (même
/// place prise, rien de demandé ni de cliquable).
pub(super) fn card<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, a: &'a Attachment, hot: &mut Hot<'a>) {
    if a.kind == MediaKind::Video {
        video_tile(ui, t, lang, images, a, hot);
    } else {
        audio_card(ui, t, lang, images.is_some(), a, hot);
    }
}

/// Affiche d'une vidéo jointe : sa première image, par le proxy média. Pas
/// d'affiche pour un spoiler, ni quand les médias ne se chargent qu'au clic.
fn poster(t: &Tokens, images: &Images, a: &Attachment, size: Vec2) -> Option<egui::TextureId> {
    if a.spoiler || !t.media_auto() {
        return None;
    }
    let url = crate::net::cdn::poster_url(&a.proxy_url)?;
    match images.get_media(a.image_ref(), &url, size) {
        Image::Ready(tex) => Some(tex),
        _ => None,
    }
}

fn video_tile<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, a: &'a Attachment, hot: &mut Hot<'a>) {
    let max = Vec2::new(ui.available_width().min(t.layout.media_width), t.layout.media_height);
    let min = bar_height(t) * 4.0;
    let (w, h) = if a.width == 0 || a.height == 0 { (max.x as u32 / DEFAULT_SIZE.0 * DEFAULT_SIZE.0, max.x as u32 / DEFAULT_SIZE.0 * DEFAULT_SIZE.1) } else { (a.width, a.height) };
    let (rect, _) = ui.allocate_exact_size(fit_box(w, h, max, min), Sense::hover());
    let Some(images) = images else { return };
    if !ui.is_rect_visible(rect) {
        return;
    }
    let clip = Clip::of(a);
    if live_tile(ui, t, lang, rect, clip, hot) {
        return;
    }
    // Au repos : l'affiche si elle est là, le disque de lecture, le nom et la taille.
    let p = ui.painter();
    let r = t.r_media();
    p.rect_filled(rect, r, t.colors.bg3);
    let shown = poster(t, images, a, rect.size());
    if let Some(tex) = shown {
        // Mode discret (lot E3) : affiche floutée ; la lecture, demandée d'un clic, ne l'est pas.
        if super::stealth::veiled(ui, rect, false).is_some() {
            super::stealth::mosaic(ui, t, rect, tex, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)));
        } else {
            p.add(crate::images::textured(rect, r, tex));
        }
    }
    let side = (Tokens::icon_side(t.typography.title) * 2.0).min(rect.width().min(rect.height()));
    let disc = Rect::from_center_size(rect.center(), Vec2::splat(side));
    p.circle_filled(disc.center(), side / 2.0, t.media_veil());
    glyph(p, disc.shrink(side / 4.0), Glyph::Play, t.on_media());
    let font = egui::FontId::proportional(t.typography.small);
    let pad = t.spacing.item_x;
    let ink = if shown.is_some() { t.on_media() } else { t.colors.muted };
    let mut job = egui::text::LayoutJob::simple_singleline(format!("{} · {}", a.display_name(), super::attachments::file_size(lang, a.size)), font, ink);
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 4.0 * pad).max(1.0));
    let label = p.layout_job(job);
    let at = pos2(rect.left() + 2.0 * pad, rect.bottom() - pad - label.size().y);
    if shown.is_some() {
        // Sur une image : un fond pour que le nom reste lisible.
        p.rect_filled(Rect::from_min_size(at, label.size()).expand2(Vec2::new(pad / 2.0, t.spacing.item_y / 2.0)), t.r_toast(), t.media_veil());
    }
    p.galley(at, label, ink);
    hot.add(rect, Action::Media(Hit::Start(clip)), Some(mt::PLAY.get(lang)));
}

/// Dessine dans `rect` la vidéo `clip` si c'est elle que le lecteur tient
/// (chargement, lecture, pause, fin, échec) et déclare ses zones ; sinon ne
/// fait rien et rend `false` : à l'appelant de dessiner le repos.
pub(super) fn live_tile<'a>(ui: &egui::Ui, t: &Tokens, lang: Lang, rect: Rect, clip: Clip<'a>, hot: &mut Hot<'a>) -> bool {
    let Some(act) = active(ui.ctx()).filter(|x| x.id == clip.id) else { return false };
    let p = ui.painter();
    let r = t.r_media();
    p.rect_filled(rect, r, t.colors.bg3);
    update(ui.ctx(), |x| x.drawn = ui.ctx().cumulative_pass_nr());
    let Some(handle) = act.handle.as_ref() else {
        notice(ui, t, rect, mt::PLAYER_NO_DECODER.get(lang));
        hot.add(rect, Action::Open(clip.url), Some(mt::OPEN_IN_BROWSER.get(lang)));
        return true;
    };
    let s = handle.snapshot();
    if !act.full {
        tell_shown(ui.ctx(), handle, rect.size());
        if let Some(tex) = &act.tex {
            picture(p, rect, tex, s.info.rotation, r);
        }
    }
    match &s.phase {
        Phase::Failed(why) => {
            notice(ui, t, rect, &mf::player_failed(lang, why));
            hot.add(rect, Action::Open(clip.url), Some(mt::OPEN_IN_BROWSER.get(lang)));
            return true;
        }
        Phase::Loading => notice(ui, t, rect, mt::PLAYER_LOADING.get(lang)),
        _ => {}
    }
    if act.gif {
        // GIF en vidéo : l'image seule ; un clic l'arrête ou la relance.
        hot.add(rect, Action::Media(Hit::Toggle), None);
        return true;
    }
    let bh = bar_height(t);
    let bar = Rect::from_min_max(pos2(rect.left(), rect.bottom() - bh), rect.max);
    let below = egui::CornerRadius { nw: 0, ne: 0, sw: r.sw, se: r.se };
    p.rect_filled(bar, below, t.media_veil());
    hot.add(Rect::from_min_max(rect.min, bar.right_top()), Action::Media(Hit::Toggle), None);
    let ink = Ink { fore: t.on_media(), track: t.on_media_dim() };
    controls(p, t, lang, bar, &act, &s, ink, &mut |zone, hit, tip| hot.add(zone, Action::Media(hit), Some(tip)));
    true
}

fn audio_card<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, live: bool, a: &'a Attachment, hot: &mut Hot<'a>) {
    let width = ui.available_width().min(t.layout.embed_width);
    let frame = t.card_frame();
    frame.show(ui, |ui| {
        let inner = (width - t.embed_margin().sum().x).max(1.0);
        let bh = bar_height(t);
        let small = egui::FontId::proportional(t.typography.small);
        let name_h = ui.fonts_mut(|f| f.row_height(&egui::TextStyle::Body.resolve(ui.style())));
        let (block, _) = ui.allocate_exact_size(Vec2::new(inner, name_h + t.spacing.item_y + bh), Sense::hover());
        if !live || !ui.is_rect_visible(block) {
            return;
        }
        let p = ui.painter();
        // Première ligne : nom du fichier (lien), taille à droite.
        let size = p.layout_no_wrap(super::attachments::file_size(lang, a.size), small, t.colors.faint);
        let mut job = egui::text::LayoutJob::simple_singleline(a.display_name().to_string(), egui::TextStyle::Body.resolve(ui.style()), t.colors.link);
        job.wrap = egui::text::TextWrapping::truncate_at_width((inner - size.size().x - t.spacing.item_x).max(1.0));
        let name = p.layout_job(job);
        let name_rect = Rect::from_min_size(block.min, name.size());
        p.galley(block.min, name, t.colors.link);
        p.galley(pos2(block.right() - size.size().x, block.top() + (name_h - size.size().y) / 2.0), size, t.colors.faint);
        hot.add(name_rect, Action::Open(&a.url), Some(tx::OPEN_EXTERNAL_TIP.get(lang)));
        // Seconde ligne : les commandes.
        let bar = Rect::from_min_max(pos2(block.left(), block.bottom() - bh), block.max);
        let ink = Ink { fore: t.colors.text, track: t.colors.bg3 };
        let act = active(ui.ctx()).filter(|x| x.id == a.id);
        let state = act.as_ref().and_then(|x| x.handle.as_ref().map(|h| (x, h.snapshot())));
        match (&act, state) {
            (Some(x), Some((_, s))) => match &s.phase {
                Phase::Failed(why) => {
                    let font = egui::FontId::proportional(t.typography.small);
                    let mut job = egui::text::LayoutJob::simple_singleline(mf::player_failed(lang, why), font, t.colors.danger);
                    job.wrap = egui::text::TextWrapping::truncate_at_width(bar.width());
                    let g = p.layout_job(job);
                    p.galley(pos2(bar.left(), bar.center().y - g.size().y / 2.0), g, t.colors.danger);
                }
                _ => controls(p, t, lang, bar, x, &s, ink, &mut |zone, hit, tip| hot.add(zone, Action::Media(hit), Some(tip))),
            },
            _ => {
                // Au repos (ou sans lecteur) : bouton de lecture et piste vide.
                let toggle = Rect::from_min_size(bar.min, Vec2::splat(bh));
                glyph(p, toggle.shrink(bh * 0.22), Glyph::Play, ink.fore);
                let line = Rect::from_min_max(pos2(toggle.right() + t.spacing.item_x, bar.top()), bar.max);
                let h = (bh * 0.14).max(2.0);
                p.rect_filled(Rect::from_center_size(line.center(), Vec2::new(line.width(), h)), h / 2.0, ink.track);
                if act.is_some() {
                    notice(ui, t, bar, mt::PLAYER_NO_DECODER.get(lang));
                } else {
                    hot.add(toggle, Action::Media(Hit::Start(Clip::of(a))), Some(mt::PLAY.get(lang)));
                }
            }
        }
    });
}

/// Fraction (0 à 1) du pointeur le long de `rect`.
fn along(resp: &egui::Response, rect: Rect) -> Option<f32> {
    let x = resp.interact_pointer_pos()?.x;
    Some(((x - rect.left()) / rect.width().max(1.0)).clamp(0.0, 1.0))
}

/// Effet d'une zone du lecteur (clic, glissement, clavier).
pub(super) fn on_zone(ctx: &egui::Context, resp: &egui::Response, rect: Rect, hit: Hit<'_>) {
    if let Hit::Start(c) = hit {
        if resp.clicked() {
            push(ctx, Cmd::Open { id: c.id, url: c.url.to_string(), mime: c.mime.map(str::to_string), video: c.video, gif: c.gif });
        }
        return;
    }
    let Some(act) = active(ctx) else { return };
    let handle = act.handle.as_ref();
    match hit {
        Hit::Start(_) => {}
        Hit::Toggle if resp.clicked() => {
            if let Some(h) = handle {
                if h.snapshot().phase == Phase::Playing { h.pause() } else { h.play() }
            }
        }
        Hit::Seek => {
            if resp.dragged() || resp.is_pointer_button_down_on() {
                let f = along(resp, rect);
                update(ctx, |x| x.drag = f.or(x.drag));
                ctx.request_repaint();
            } else if let Some(f) = act.drag {
                // Bouton relâché : le saut se fait une fois, à l'arrivée.
                if let Some(h) = handle {
                    h.seek((f64::from(f) * h.snapshot().info.duration_us as f64) as u64);
                }
                update(ctx, |x| x.drag = None);
                ctx.request_repaint();
            }
        }
        Hit::Volume => {
            if (resp.dragged() || resp.is_pointer_button_down_on())
                && let Some(f) = along(resp, rect)
            {
                if let Some(h) = handle {
                    h.set_volume(f);
                }
                update(ctx, |x| {
                    x.volume = f;
                    x.muted = false;
                });
                ctx.request_repaint();
            }
        }
        Hit::Mute if resp.clicked() => {
            let muted = !act.muted;
            if let Some(h) = handle {
                h.set_volume(if muted { 0.0 } else { act.volume });
            }
            update(ctx, |x| x.muted = muted);
            ctx.request_repaint();
        }
        Hit::Full if resp.clicked() => {
            update(ctx, |x| x.full = !x.full);
            ctx.request_repaint();
        }
        Hit::Close if resp.clicked() => push(ctx, Cmd::Close),
        _ => {}
    }
}

/// Vidéo agrandie à toute la fenêtre, si elle l'est. À appeler avant le
/// reste de l'interface : elle prend Échap.
pub(super) fn show_full(ui: &mut egui::Ui, t: &Tokens, lang: Lang) {
    let ctx = ui.ctx().clone();
    let Some(act) = active(&ctx).filter(|a| a.full) else { return };
    let Some(handle) = act.handle.clone() else { return };
    if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)) {
        update(&ctx, |x| x.full = false);
        return;
    }
    let screen = ctx.content_rect();
    let s = handle.snapshot();
    egui::Area::new(id().with("plein")).order(egui::Order::Foreground).fixed_pos(screen.min).show(&ctx, |ui| {
        let (bg, back) = ui.allocate_exact_size(screen.size(), Sense::click());
        let p = ui.painter().clone();
        p.rect_filled(bg, 0.0, t.media_veil());
        let bh = bar_height(t);
        let area = Rect::from_min_max(bg.min, pos2(bg.right(), bg.bottom() - bh));
        tell_shown(&ctx, &handle, area.size());
        if let Some(tex) = &act.tex {
            let src = turned(tex.size(), s.info.rotation);
            let scale = (area.width() / src.x.max(1.0)).min(area.height() / src.y.max(1.0));
            picture(&p, Rect::from_center_size(area.center(), src * scale), tex, s.info.rotation, egui::CornerRadius::ZERO);
        }
        match &s.phase {
            Phase::Failed(why) => notice(ui, t, area, &mf::player_failed(lang, why)),
            Phase::Loading => notice(ui, t, area, mt::PLAYER_LOADING.get(lang)),
            _ => {}
        }
        if back.clicked() {
            on_zone(&ctx, &back, area, Hit::Toggle);
        }
        let bar = Rect::from_min_max(pos2(bg.left(), bg.bottom() - bh), bg.max);
        p.rect_filled(bar, 0.0, t.media_veil());
        let ink = Ink { fore: t.on_media(), track: t.on_media_dim() };
        let mut zones: Vec<(Rect, Hit<'static>, &'static str)> = Vec::new();
        controls(&p, t, lang, bar, &act, &s, ink, &mut |zone, hit, tip| zones.push((zone, hit, tip)));
        for (i, (zone, hit, tip)) in zones.into_iter().enumerate() {
            let sense = if hit.drags() { Sense::click_and_drag() } else { Sense::click() };
            let resp = ui.interact(zone, id().with(("plein", i)), sense).on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.hovered() || resp.has_focus() {
                p.rect_filled(zone, t.r_control(), t.colors.hover);
            }
            on_zone(&ctx, &resp, zone, hit);
        }
    });
}

#[cfg(test)]
#[path = "player_tests.rs"]
mod tests;
