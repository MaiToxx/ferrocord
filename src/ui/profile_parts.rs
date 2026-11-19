//! Pièces de la carte de profil (lot profil), partagées par la carte, le
//! panneau de profil d'un MP et le profil complet : rangée de badges et son
//! infobulle détaillée, bulle du statut personnalisé, boutons ronds de la
//! bannière, ligne des amis et serveurs en commun, comptes connectés en
//! pastilles rondes.
//!
//! Rien n'est inventé. L'infobulle d'un badge ne montre que ce que le profil
//! donne (description, dates de Nitro et de boost) ou ce qui s'en déduit
//! (ancienneté du compte, lue dans l'identifiant). Les paliers de Nitro
//! viennent de l'identifiant du badge (`premium_tenure_<mois>_month[_v2]`,
//! liste publique des badges, gist lié par docs.discord.food, « Profile Badge
//! Structure »). Le nom du palier d'ancienneté du compte (« Grand pin »…)
//! n'est décrit par aucune source publique : il n'est pas affiché.
//!
//! Rien ne bouge tout seul : une infobulle n'existe que sous le pointeur.

use super::cosmetics::mix;
use super::profile::Shown;
use super::profile_full::Tab;
use super::App;
use crate::i18n::table::profile::{f as pf, p as pp, service, t as pt};
use crate::i18n::{time, Lang, Text};
use crate::images::{self, Image, Images};
use crate::state::model::{snowflake_ms, Avatar, ImageRef};
use crate::state::people::{Activity, Presence, Status};
use crate::state::model::ContentHash;
use crate::state::profiles::{Connection, DirBadge, Profile, ProfileBadge};
use crate::state::relations::RelKind;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use chrono::{Datelike, TimeZone};
use eframe::egui::{self, Color32, Id, Pos2, Rect, RichText, Sense, Stroke};

/// Part de la couleur du texte dans le fond d'un bloc (pastille de badges,
/// cartes) : translucide, il suit le fond de la carte, thème du profil compris.
const BLOCK_TINT: f32 = 0.07;
/// Lignes du statut personnalisé dans sa bulle, au plus.
const BUBBLE_ROWS: usize = 3;
/// Avatars empilés devant « N amis en commun », au plus.
const STACKED: usize = 3;
/// Badges montrés dans la capsule d'une carte avant « +N ».
const CAPSULE: usize = 6;

/// Fond translucide d'un bloc posé sur la carte.
pub(super) fn tint(t: &Tokens) -> Color32 {
    t.colors.text_strong.gamma_multiply(BLOCK_TINT)
}

/// Petit titre d'une partie de la carte : demi-gras, sans capitales.
pub(super) fn heading(ui: &mut egui::Ui, t: &Tokens, title: &str) {
    ui.label(RichText::new(title).small().family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
}

/// Ancienneté entre `from_ms` et `now_ms` : « 8 ans, 8 mois », « 3 mois »,
/// « 12 jours » (années et mois pleins du calendrier, en UTC).
pub(super) fn tenure(lang: Lang, from_ms: i64, now_ms: i64) -> String {
    let day = |ms: i64| chrono::Utc.timestamp_millis_opt(ms).single().map(|d| d.date_naive());
    let (Some(a), Some(b)) = (day(from_ms), day(now_ms.max(from_ms))) else { return String::new() };
    let mut months = (b.year() - a.year()) * 12 + b.month() as i32 - a.month() as i32;
    if b.day() < a.day() {
        months -= 1;
    }
    let months = months.max(0) as u64;
    match (months / 12, months % 12) {
        (0, 0) => pp::days(lang, (b - a).num_days().max(0) as u64),
        (0, m) => pp::months(lang, m),
        (y, 0) => pp::years(lang, y),
        (y, m) => pf::tenure(lang, pp::years(lang, y), pp::months(lang, m)),
    }
}

/// Ce que montre l'infobulle d'un badge, sous sa grande image.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct BadgeTip {
    /// Ligne grise (« Ancienneté du compte », « Abonné depuis »…).
    pub(super) category: Option<String>,
    /// Nom du palier, affiché en gras et en capitales.
    pub(super) tier: Option<String>,
    /// Durée, date, ou description du badge.
    pub(super) detail: String,
    /// Lignes grises sous le détail : rang du palier, prochain palier et sa date.
    pub(super) more: Vec<String>,
}

/// Mois d'abonnement de chaque palier de Nitro (liste publique des badges).
const NITRO_LADDER: [u32; 8] = [1, 3, 6, 12, 24, 36, 60, 72];
/// Mois de boost de chaque niveau du badge de boost (`guild_booster_lvl1` à `9`).
const BOOST_LADDER: [u32; 9] = [1, 2, 3, 6, 9, 12, 15, 18, 24];

/// Rang du palier (`rank`, dès 1) dans son échelle, puis le palier suivant et
/// le jour où il sera atteint (`since` et les mois de l'échelle), s'il est à venir.
fn ladder_lines(lang: Lang, rank: usize, ladder: &[u32], since: Option<i64>, now_ms: i64, name: impl Fn(usize) -> Option<String>) -> Vec<String> {
    let mut out = vec![pf::tier_rank(lang, rank, ladder.len())];
    let at = since.zip(ladder.get(rank)).and_then(|(since, months)| chrono::Utc.timestamp_millis_opt(since).single()?.checked_add_months(chrono::Months::new(*months))).map(|d| d.timestamp_millis());
    if let Some(at) = at.filter(|at| *at > now_ms)
        && let Some(next) = name(rank)
    {
        out.push(pf::next_tier(lang, next, time::long_date(lang, at)));
    }
    out
}

/// Badge d'ancienneté du compte ? Son identifiant n'est dans aucune source
/// publique : reconnu à sa description (celle que Discord renvoie) ou à un
/// identifiant qui parle d'âge du compte.
pub(super) fn is_account_age(b: &ProfileBadge) -> bool {
    let id = b.id.to_ascii_lowercase();
    let d = b.description.trim().to_lowercase();
    (id.contains("account") && (id.contains("age") || id.contains("tenure") || id.contains("anniversary"))) || ["ancienneté du compte", "account age", "account tenure"].contains(&d.as_str())
}

/// Mois du palier d'un badge `premium_tenure_<mois>_month[_v2]`.
fn premium_months(id: &str) -> Option<u32> {
    let rest = id.strip_prefix("premium_tenure_")?;
    rest.split('_').next()?.parse().ok()
}

/// Nom du palier de Nitro (liste publique des badges : Bronze, Silver, Gold,
/// Platinum, Diamond, Emerald, Ruby, Opal).
fn premium_tier(months: u32) -> Option<Text> {
    Some(match months {
        1 => pt::TIER_BRONZE,
        3 => pt::TIER_SILVER,
        6 => pt::TIER_GOLD,
        12 => pt::TIER_PLATINUM,
        24 => pt::TIER_DIAMOND,
        36 => pt::TIER_EMERALD,
        60 => pt::TIER_RUBY,
        72 => pt::TIER_OPAL,
        _ => return None,
    })
}

/// Contenu de l'infobulle du badge `b` du profil `p` de `user_id`.
pub(super) fn badge_tip(lang: Lang, b: &ProfileBadge, user_id: u64, p: Option<&Profile>, now_ms: i64) -> BadgeTip {
    let date = |ms: Option<i64>| ms.map(|ms| time::long_date(lang, ms)).filter(|s| !s.is_empty());
    let plain = |tier: Option<String>| BadgeTip { category: None, tier, detail: b.description.clone(), more: Vec::new() };
    let id = b.id.as_str();
    if is_account_age(b) {
        return BadgeTip { category: Some(b.description.clone()), tier: None, detail: tenure(lang, snowflake_ms(user_id), now_ms), more: Vec::new() };
    }
    if let Some(months) = premium_months(id) {
        let tier = premium_tier(months).map(|t| t.get(lang).to_string());
        let since = p.and_then(|p| p.premium_since);
        let name = |i: usize| NITRO_LADDER.get(i).and_then(|m| premium_tier(*m)).map(|t| t.get(lang).to_string());
        let more = NITRO_LADDER.iter().position(|m| *m == months).map(|i| ladder_lines(lang, i + 1, &NITRO_LADDER, since, now_ms, name)).unwrap_or_default();
        return match date(since) {
            Some(detail) => BadgeTip { category: Some(pt::BADGE_SUBSCRIBER_SINCE.get(lang).to_string()), tier, detail, more },
            None => BadgeTip { more, ..plain(tier) },
        };
    }
    if id == "premium"
        && let Some(detail) = date(p.and_then(|p| p.premium_since))
    {
        return BadgeTip { category: Some(pt::BADGE_SUBSCRIBER_SINCE.get(lang).to_string()), tier: None, detail, more: Vec::new() };
    }
    if id.starts_with("guild_booster_lvl")
        && let Some(detail) = date(p.and_then(|p| p.premium_guild_since))
    {
        // Niveau du badge de boost : dans son identifiant.
        let level = id.strip_prefix("guild_booster_lvl").and_then(|n| n.parse::<usize>().ok()).filter(|n| (1..=BOOST_LADDER.len()).contains(n));
        let more = level.map(|n| ladder_lines(lang, n, &BOOST_LADDER, p.and_then(|p| p.premium_guild_since), now_ms, |i| Some(pf::boost_level(lang, i + 1)))).unwrap_or_default();
        return BadgeTip { category: Some(pt::BADGE_BOOST_SINCE.get(lang).to_string()), tier: level.map(|n| pf::boost_level(lang, n)), detail, more };
    }
    plain(None)
}

/// Badge du répertoire `list` qui correspond au badge `b` du profil. Les
/// deux listes n'ont pas de clé commune ; du plus sûr au moins sûr :
/// identifiants relevés dans le client (19/11/2026), puis la même petite
/// icône vectorielle, puis le même nom (mots dans n'importe quel ordre :
/// « HypeSquad Balance » dans le profil, « Balance HypeSquad » ici).
fn dir_find<'a>(list: &'a [DirBadge], b: &ProfileBadge) -> Option<&'a DirBadge> {
    let id = b.id.as_str();
    let known = if id.starts_with("premium") {
        Some(1)
    } else if id.starts_with("guild_booster") {
        Some(12)
    } else {
        match id {
            "hypesquad_house_3" => Some(7),
            "legacy_username" => Some(13),
            "account_age" => Some(18),
            "streaming" => Some(19),
            "game_time" => Some(20),
            "game_variety" => Some(21),
            _ => None,
        }
    };
    let words = |s: &str| {
        let mut w: Vec<String> = s.split_whitespace().map(str::to_lowercase).collect();
        w.sort();
        w
    };
    let mine = words(&b.description);
    known
        .and_then(|k| list.iter().find(|d| d.id == k))
        .or_else(|| b.simple.and_then(|h| list.iter().find(|d| d.has_simple(h))))
        .or_else(|| list.iter().find(|d| !mine.is_empty() && words(&d.name) == mine))
}

/// Fenêtre des badges ouverte : (utilisateur, badge choisi).
fn badge_window_id() -> Id {
    Id::unique("ferrocord.profils.fenêtre-badges")
}

/// Plus grand rectangle aux proportions de `size`, centré dans `rect`.
fn contained(rect: Rect, size: egui::Vec2) -> Rect {
    if size.x <= 0.0 || size.y <= 0.0 || !size.is_finite() {
        return rect;
    }
    let k = (rect.width() / size.x).min(rect.height() / size.y);
    Rect::from_center_size(rect.center(), size * k)
}

/// Illustration `art` d'un badge du répertoire, entière et centrée dans
/// `rect` (certaines sont plus larges que hautes : jamais recadrées ni
/// déformées) ; illisible : la petite icône `fallback` à sa place.
fn paint_dir_art(ui: &egui::Ui, images: &Images, rect: Rect, art: Option<(ContentHash, bool)>, fallback: Option<(ContentHash, bool)>) {
    let get = |(hash, svg): (ContentHash, bool)| images.get_within(ImageRef::BadgeArt { hash, svg }, rect.width(), rect.height());
    let shown = match art.map(get) {
        Some(Image::Failed) | None => fallback.map(get),
        other => other,
    };
    if let Some(Image::Ready(tex)) = shown {
        let size = ui.ctx().tex_manager().read().meta(tex).map(|m| egui::vec2(m.size[0] as f32, m.size[1] as f32));
        ui.painter().add(images::textured(contained(rect, size.unwrap_or(rect.size())), egui::CornerRadius::ZERO, tex));
    }
}

/// [`paint_dir_art`] dans un carré de côté `side` réservé ici.
fn dir_art(ui: &mut egui::Ui, images: &Images, art: Option<(ContentHash, bool)>, fallback: Option<(ContentHash, bool)>, side: f32) -> Rect {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
    paint_dir_art(ui, images, rect, art, fallback);
    rect
}

/// Nom du badge en gris quand le titre est celui du palier, puis le titre
/// en gras et en capitales.
fn dir_title(ui: &mut egui::Ui, t: &Tokens, d: &DirBadge) {
    let title = d.title();
    if title != d.name {
        ui.label(RichText::new(&d.name).color(t.colors.muted));
    }
    ui.add(egui::Label::new(RichText::new(title.to_uppercase()).size(t.typography.title).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).wrap());
}

/// Infobulle d'un badge du répertoire : image du palier atteint, nom du
/// badge, palier, puis ce que Discord en dit (« 5 jeux joués »).
fn dir_tip_ui(ui: &mut egui::Ui, t: &Tokens, images: &Images, d: &DirBadge) {
    let side = t.layout.profile_avatar;
    ui.set_max_width(side * 2.5);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = t.spacing.item_y / 2.0;
        dir_art(ui, images, d.best_art(), d.small_icon(), side);
        ui.add_space(t.spacing.item_y);
        dir_title(ui, t, d);
        if !d.info.is_empty() {
            ui.add(egui::Label::new(RichText::new(&d.info).color(t.colors.text)).wrap());
        }
    });
}

/// Infobulle d'un badge : grande image, catégorie en gris, palier en gras et
/// en capitales, détail.
fn badge_tip_ui(ui: &mut egui::Ui, t: &Tokens, images: &Images, b: &ProfileBadge, tip: &BadgeTip) {
    let side = t.layout.profile_avatar;
    ui.set_max_width(side * 2.5);
    ui.vertical_centered(|ui| {
        ui.spacing_mut().item_spacing.y = t.spacing.item_y / 2.0;
        if let Some(hash) = b.icon
            && let Image::Ready(tex) = images.get(ImageRef::BadgeIcon { hash }, side)
        {
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
            ui.painter().add(images::textured(rect, egui::CornerRadius::ZERO, tex));
            ui.add_space(t.spacing.item_y);
        }
        if let Some(c) = &tip.category {
            ui.label(RichText::new(c).color(t.colors.muted));
        }
        if let Some(tier) = &tip.tier {
            ui.label(RichText::new(tier.to_uppercase()).size(t.typography.title).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
        }
        ui.add(egui::Label::new(RichText::new(&tip.detail).color(t.colors.text)).wrap());
        for line in &tip.more {
            ui.add(egui::Label::new(RichText::new(line).small().color(t.colors.muted)).wrap());
        }
    });
}

/// Téléphone (présence sur mobile) dans `rect`, de couleur `color`. `hole` :
/// couleur de l'écran quand le fond est connu ; sinon le contour seul.
pub(super) fn paint_phone(p: &egui::Painter, rect: Rect, color: Color32, hole: Option<Color32>) {
    let h = rect.height();
    let body = Rect::from_center_size(rect.center(), egui::vec2((h * 0.62).min(rect.width()), h));
    let r = (h * 0.14).max(1.0);
    match hole {
        Some(hole) => {
            p.rect_filled(body, r, color);
            let screen = Rect::from_min_max(body.min + egui::vec2(h * 0.1, h * 0.14), body.max - egui::vec2(h * 0.1, h * 0.26));
            p.rect_filled(screen, r * 0.4, hole);
            p.circle_filled(egui::pos2(body.center().x, body.bottom() - h * 0.13), (h * 0.06).max(0.6), hole);
        }
        None => {
            let w = (h * 0.11).max(1.0);
            p.rect_stroke(body, r, Stroke::new(w, color), egui::StrokeKind::Inside);
            p.circle_filled(egui::pos2(body.center().x, body.bottom() - h * 0.2), (h * 0.07).max(0.7), color);
        }
    }
}

/// La personne est-elle en ligne depuis un mobile ?
pub(super) fn on_mobile(p: &Presence) -> bool {
    p.status != Status::Offline && p.client.mobile.is_some_and(|s| s != Status::Offline)
}

/// En ligne depuis un mobile seulement : la pastille de l'avatar devient un téléphone.
pub(super) fn mobile_only(p: &Presence) -> bool {
    let off = |s: Option<Status>| s.is_none_or(|s| s == Status::Offline);
    on_mobile(p) && off(p.client.desktop) && off(p.client.web)
}

/// Plateformes d'où la personne est connectée, dans l'ordre ordinateur,
/// navigateur, mobile (`client_status` ; aucune si elle est hors ligne).
pub(super) fn platforms(p: &Presence) -> [bool; 3] {
    let on = |s: Option<Status>| p.status != Status::Offline && s.is_some_and(|s| s != Status::Offline);
    [on(p.client.desktop), on(p.client.web), on(p.client.mobile)]
}

/// Largeur des icônes de [`paint_platforms`] (0 sans plateforme).
pub(super) fn platforms_width(t: &Tokens, on: [bool; 3], side: f32) -> f32 {
    let n = on.iter().filter(|b| **b).count() as f32;
    if n == 0.0 { 0.0 } else { n * side + (n - 1.0) * t.spacing.item_y }
}

/// Globe (présence depuis un navigateur) dans `rect`, de couleur `color`.
fn paint_globe(p: &egui::Painter, rect: Rect, color: Color32) {
    let r = rect.height() * 0.44;
    let stroke = Stroke::new((rect.height() * 0.09).max(1.0), color);
    let c = rect.center();
    p.circle_stroke(c, r, stroke);
    p.add(egui::Shape::ellipse_stroke(c, egui::vec2(r * 0.45, r), stroke));
    p.line_segment([egui::pos2(c.x - r, c.y), egui::pos2(c.x + r, c.y)], stroke);
}

/// Icônes des plateformes `on` (écran : ordinateur, globe : navigateur,
/// téléphone : mobile), de côté `side`, à partir de `left` et centrées sur
/// `y` ; leurs noms au survol.
#[allow(clippy::too_many_arguments)]
pub(super) fn paint_platforms(ui: &egui::Ui, t: &Tokens, lang: Lang, on: [bool; 3], left: f32, y: f32, side: f32, color: Color32, id: egui::Id) {
    let w = platforms_width(t, on, side);
    if w == 0.0 {
        return;
    }
    let all = Rect::from_min_size(egui::pos2(left, y - side / 2.0), egui::vec2(w, side));
    if !ui.is_rect_visible(all) {
        return;
    }
    let mut x = left;
    let mut names = Vec::new();
    let texts = [crate::i18n::t::PLATFORM_DESKTOP, crate::i18n::t::PLATFORM_WEB, crate::i18n::t::PLATFORM_MOBILE];
    for (i, text) in texts.into_iter().enumerate() {
        if !on[i] {
            continue;
        }
        let r = crate::ui::util::pixel_square(ui, egui::pos2(x, all.top()), side);
        match i {
            0 => icons::paint(Icon::Screen, ui.painter(), r, color),
            1 => paint_globe(ui.painter(), r, color),
            _ => paint_phone(ui.painter(), r, color, None),
        }
        names.push(text.get(lang));
        x += side + t.spacing.item_y;
    }
    ui.interact(all, id, Sense::hover()).on_hover_text(names.join(", "));
}

/// Pastille de statut « mobile » sur l'avatar `avatar` : un téléphone de la
/// couleur du statut, cerclé de `ring` (le fond à cet endroit).
pub(super) fn paint_mobile_status(p: &egui::Painter, t: &Tokens, avatar: Rect, status: Status, ring: Color32) {
    let (c, r, w) = t.status_dot(avatar);
    let glyph = Rect::from_center_size(c, egui::vec2(r * 1.5, r * 2.3));
    p.rect_filled(glyph.expand(w), (r * 0.5).max(1.0), ring);
    paint_phone(p, glyph, t.status_color(status), Some(ring));
}

/// Bulle de pensée du statut personnalisé `a`, son coin haut gauche en
/// `at`, large d'au plus `max_width` ; `base` : le fond de la carte à cet
/// endroit. Emoji en image, texte sur trois lignes au plus. Rend son
/// rectangle (rien si le statut est vide).
pub(super) fn status_bubble(ui: &egui::Ui, t: &Tokens, a: &Activity, at: Pos2, max_width: f32, base: Color32) -> Option<Rect> {
    let img = a.emoji.as_ref().and_then(|e| match (e.id, e.name.as_deref()) {
        (Some(id), _) => Some(super::emoji::Img::custom(id, e.animated)),
        (None, Some(n)) => crate::state::emoji::find(n).map(super::emoji::Img::Unicode),
        _ => None,
    });
    let text = a.state.as_deref().unwrap_or_default().trim();
    if img.is_none() && text.is_empty() {
        return None;
    }
    let font = egui::FontId::proportional(t.typography.body);
    let side = super::emoji::row_height(ui, &font);
    let pad = egui::vec2((t.spacing.item_x * 1.5).round(), t.spacing.item_x);
    let lead = if img.is_some() { side + t.spacing.item_y } else { 0.0 };
    let (mut job, spots) = super::emoji::job(ui, text, &font, t.colors.text);
    job.wrap = egui::text::TextWrapping { max_width: (max_width - 2.0 * pad.x).max(side), max_rows: BUBBLE_ROWS, break_anywhere: false, overflow_character: Some('…') };
    match job.sections.first_mut() {
        Some(first) => first.leading_space += lead,
        // Emoji seul : une ligne vide de sa hauteur.
        None => job.append("", lead, egui::TextFormat { font_id: font.clone(), ..Default::default() }),
    }
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let size = egui::vec2(galley.size().x.max(lead), galley.size().y.max(side)) + 2.0 * pad;
    let rect = Rect::from_min_size(at, size);
    if !ui.is_rect_visible(rect.expand(3.0 * t.spacing.item_x)) {
        return Some(rect);
    }
    // Plus sombre que la carte sur un thème sombre, plus claire sur un thème clair.
    let dark = super::cosmetics::legible(Color32::WHITE, base, 3.0) == Color32::WHITE;
    let fill = mix(base, if dark { Color32::BLACK } else { Color32::WHITE }, 0.45);
    let stroke = Stroke::new(t.shapes.border.max(1.0), t.hairline(true));
    let u = t.spacing.item_x;
    let p = ui.painter();
    // Les deux petits ronds qui relient la bulle à l'avatar.
    for (center, r) in [(at + egui::vec2(0.2 * u, -1.5 * u), 0.55 * u), (at + egui::vec2(2.2 * u, -0.1 * u), 1.05 * u)] {
        p.circle(center, r, fill, stroke);
    }
    p.rect(rect, t.r_card(), fill, stroke, egui::StrokeKind::Inside);
    // Le grand rond se fond dans la bulle : son bord intérieur est recouvert.
    p.circle_filled(at + egui::vec2(2.2 * u, 0.1 * u + stroke.width), 1.05 * u - stroke.width, fill);
    let pos = rect.min + pad;
    if let Some(img) = img {
        super::emoji::paint(ui, Rect::from_min_size(pos, egui::Vec2::splat(side)), img);
    }
    super::emoji::paint_spots(ui, pos, &galley, &spots);
    p.galley(pos, galley, t.colors.text);
    Some(rect)
}

/// Couleur de marque d'un service de comptes liés (fond de sa pastille).
fn service_color(kind: &str) -> Option<u32> {
    Some(match kind {
        "amazon-music" => 0x25d1da,
        "battlenet" => 0x148eff,
        "bluesky" => 0x0085ff,
        "bungie" => 0x0075bb,
        "crunchyroll" => 0xf47521,
        "ebay" => 0xe53238,
        "epicgames" => 0x2f2d2e,
        "facebook" => 0x1877f2,
        "github" => 0x24292f,
        "instagram" => 0xe4405f,
        "leagueoflegends" | "riotgames" => 0xd32936,
        "mastodon" => 0x6364ff,
        "paypal" => 0x003087,
        "playstation" | "playstation-stg" => 0x003791,
        "reddit" => 0xff4500,
        "roblox" => 0x393b3d,
        "samsung" => 0x1428a0,
        "skype" => 0x00aff0,
        "soundcloud" => 0xff5500,
        "spotify" => 0x1db954,
        "steam" => 0x1b2838,
        "tiktok" => 0x010101,
        "twitch" => 0x9146ff,
        "twitter" => 0x0f1419,
        "xbox" => 0x107c10,
        "youtube" => 0xff0000,
        _ => return None,
    })
}

/// Nom affiché du service d'un compte lié.
pub(super) fn service_name(lang: Lang, kind: &str) -> String {
    if kind == "domain" { pt::DOMAIN.get(lang).to_string() } else { service(kind).unwrap_or(kind).to_string() }
}

/// Pastille ronde d'un service dans `rect` : sa couleur de marque et son
/// initiale (aucun logo de marque n'est embarqué).
pub(super) fn paint_service(p: &egui::Painter, t: &Tokens, rect: Rect, lang: Lang, kind: &str) {
    let fill = service_color(kind).map_or(t.colors.bg3, super::cosmetics::rgb);
    let ink = if crate::theme::contrast_ratio(Color32::WHITE, fill) >= 3.0 { Color32::WHITE } else { Color32::from_rgb(6, 6, 7) };
    p.circle_filled(rect.center(), rect.width() / 2.0, fill);
    let letter: String = service_name(lang, kind).chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_default();
    p.text(rect.center(), egui::Align2::CENTER_CENTER, letter, crate::ui::fonts::demi_font((rect.height() * 0.5).round()), ink);
}

/// Comptes connectés en rangée de pastilles rondes : le service et le nom au
/// survol, la page publique au clic quand son adresse se déduit.
pub(super) fn connections_row(ui: &mut egui::Ui, t: &Tokens, lang: Lang, list: &[Connection]) {
    if list.is_empty() {
        return;
    }
    let side = t.layout.small_avatar;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_x, t.spacing.item_y);
        for c in list {
            let url = c.url();
            let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), if url.is_some() { Sense::click() } else { Sense::hover() });
            let label = pf::connection(lang, service_name(lang, &c.kind), &c.name);
            resp.widget_info(|| egui::WidgetInfo::labeled(if url.is_some() { egui::Role::Link } else { egui::Role::Label }, true, &label));
            if ui.is_rect_visible(rect) {
                paint_service(ui.painter(), t, rect, lang, &c.kind);
                if resp.hovered() && url.is_some() {
                    ui.painter().circle_stroke(rect.center(), side / 2.0, Stroke::new(t.border().width.max(1.0), t.colors.text_strong));
                }
            }
            let resp = resp.on_hover_text(label);
            if let Some(url) = url
                && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
            {
                ui.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
    });
}

/// Geste fait sur un bouton rond de la bannière.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum HeadPick {
    AddFriend,
    Accept,
    /// Annule ma demande, ou refuse celle reçue.
    Cancel,
    /// Retirer l'ami (demande « Oui »).
    Remove,
    /// « … » : le menu de l'utilisateur, au point donné.
    More(Pos2),
}

/// Bouton rond posé sur la bannière (disque sombre, lisible sur toute image).
fn round_button(ui: &mut egui::Ui, t: &Tokens, rect: Rect, id: Id, icon: Icon, tip: &str) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus();
        let veil = t.media_veil();
        let p = ui.painter();
        p.circle_filled(rect.center(), rect.width() / 2.0, if hot { mix(veil, Color32::WHITE, 0.18).gamma_multiply(1.0) } else { veil });
        let side = Tokens::icon_side(t.typography.body);
        icons::paint(icon, p, Rect::from_center_size(rect.center(), egui::Vec2::splat(side)), if hot { t.on_media() } else { t.on_media_dim() });
    }
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl App {
    /// Relation avec `user_id` telle que le bouton de la bannière la montre ;
    /// `None` : pas de bouton (moi, bot, bloqué, ancien nom à discriminant).
    fn friend_state(&self, user_id: u64) -> Option<Option<RelKind>> {
        if self.store.me.as_ref().is_some_and(|m| m.id == user_id) {
            return None;
        }
        match self.store.relations.relation(user_id).map(|r| r.kind) {
            Some(RelKind::Blocked) => None,
            Some(k @ (RelKind::Friend | RelKind::Incoming | RelKind::Outgoing)) => Some(Some(k)),
            // Demande d'ami par nom : seulement pour le nouveau système de noms, jamais un bot.
            _ => self.store.people.lock().user(user_id).is_some_and(|u| u.discriminator.is_none() && !u.bot && !u.system).then_some(None),
        }
    }

    /// Les deux boutons ronds de la bannière, alignés à droite sous
    /// `top_right` : état d'amitié (avec son menu) et « … » (menu de
    /// l'utilisateur). Hors du flux : ils ne déplacent rien.
    pub(super) fn header_buttons(&self, ui: &mut egui::Ui, t: &Tokens, top_right: Pos2, user_id: u64) -> Option<HeadPick> {
        let lang = self.lang;
        let side = t.spacing.control_height.max(t.typography.icon);
        let gap = t.spacing.item_x;
        let slot = |n: f32| Rect::from_min_size(egui::pos2(top_right.x - (n + 1.0) * side - n * gap, top_right.y), egui::Vec2::splat(side));
        let mut pick = None;
        let more = round_button(ui, t, slot(0.0), Id::unique(("ferrocord.profils.plus", user_id)), Icon::More, pt::MORE.get(lang));
        if more.clicked() {
            pick = Some(HeadPick::More(more.rect.left_bottom()));
        }
        if let Some(kind) = self.friend_state(user_id) {
            let (icon, tip) = match kind {
                Some(RelKind::Friend) => (Icon::Check, pt::FRIEND_YES),
                Some(RelKind::Outgoing) => (Icon::Clock, pt::FRIEND_SENT),
                Some(RelKind::Incoming) => (Icon::FriendAdd, pt::FRIEND_RECEIVED),
                _ => (Icon::FriendAdd, pt::FRIEND_ADD),
            };
            let resp = round_button(ui, t, slot(1.0), Id::unique(("ferrocord.profils.ami", user_id)), icon, tip.get(lang));
            egui::Popup::menu(&resp).id(Id::unique(("ferrocord.profils.ami.menu", user_id))).show(|ui| {
                let mut line = |ui: &mut egui::Ui, text: Text, icon: Icon, danger: bool, p: HeadPick| {
                    if super::menu::item(ui, t_menu(self), text.get(lang), Some(icon), danger).clicked() {
                        pick = Some(p);
                    }
                };
                match kind {
                    Some(RelKind::Friend) => line(ui, pt::MENU_REMOVE, Icon::Close, true, HeadPick::Remove),
                    Some(RelKind::Outgoing) => line(ui, pt::MENU_CANCEL, Icon::Close, false, HeadPick::Cancel),
                    Some(RelKind::Incoming) => {
                        line(ui, pt::MENU_ACCEPT, Icon::FriendAdd, false, HeadPick::Accept);
                        line(ui, crate::i18n::table::social::t::DECLINE, Icon::Close, false, HeadPick::Cancel);
                    }
                    _ => line(ui, pt::MENU_ADD_FRIEND, Icon::FriendAdd, false, HeadPick::AddFriend),
                }
            });
        }
        pick
    }

    /// Applique un geste des boutons de la bannière : chaque écriture suit
    /// le clic sur la ligne du menu ; retirer un ami demande « Oui ».
    pub(super) fn head_pick(&mut self, ctx: &egui::Context, user_id: u64, name: &str, pick: HeadPick) {
        use crate::state::relations::RelationsCmd;
        match pick {
            HeadPick::More(at) => super::profile_full::open_menu(ctx, user_id, name, at),
            HeadPick::Remove => super::profile_full::ask_relation(ctx, user_id, name, false),
            HeadPick::Accept => self.store.relations_write(RelationsCmd::Accept { user_id }),
            HeadPick::Cancel => self.store.relations_write(RelationsCmd::Remove { user_id }),
            HeadPick::AddFriend => {
                let username = self.store.people.lock().user(user_id).map(|u| u.username.clone());
                if let Some(username) = username {
                    self.store.relations_write(RelationsCmd::AddFriend { username, note: None });
                }
            }
        }
    }

    /// Rangée de badges dans sa pastille arrondie : le téléphone d'abord
    /// quand la personne est en ligne depuis un mobile, puis les badges du
    /// profil (icône ; leur description tant que l'icône n'est pas chargée).
    /// Au survol : l'infobulle détaillée ; au clic : le lien du badge.
    pub(super) fn badges_ui(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, s: &Shown) {
        let lang = self.lang;
        let badges = s.profile.as_ref().map(|p| p.badges.as_slice()).unwrap_or_default();
        let mobile = s.presence.as_ref().filter(|p| on_mobile(p));
        if badges.is_empty() && mobile.is_none() {
            return;
        }
        let side = t.profile_badge();
        let m = t.profile_chip_margin();
        // Fond réservé avant le contenu, dessiné à sa taille une fois celle-ci connue.
        let bg = ui.painter().add(egui::Shape::Noop);
        let mut used = Rect::NOTHING;
        egui::Frame::new().inner_margin(m).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_y, t.spacing.item_y);
                if let Some(p) = mobile {
                    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                    paint_phone(ui.painter(), rect.shrink(side * 0.08), t.status_color(p.status), None);
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, pt::ON_MOBILE.get(lang)));
                    used = used.union(resp.on_hover_text(pt::ON_MOBILE.get(lang)).rect);
                }
                for b in badges {
                    used = used.union(self.badge_ui(ui, t, user_id, s, b).rect);
                }
            });
        });
        if used.is_positive() {
            let rect = Rect::from_min_max(used.min - m.left_top(), used.max + m.right_bottom());
            ui.painter().set(bg, egui::Shape::Rect(egui::epaint::RectShape::new(rect, t.r_row(), tint(t), Stroke::new(t.shapes.border.max(1.0), t.hairline(false)), egui::StrokeKind::Inside)));
        }
    }

    /// La carte de `s` a-t-elle une capsule de badges à montrer ?
    pub(super) fn has_badges(&self, s: &Shown) -> bool {
        s.profile.as_ref().is_some_and(|p| !p.badges.is_empty()) || s.presence.as_ref().is_some_and(on_mobile)
    }

    /// Capsule des badges d'une carte, posée dans la ligne de l'identifiant
    /// comme dans le client : une seule rangée, [`CAPSULE`] badges au plus puis
    /// « +N », dont le clic les montre tous (sur plusieurs rangées au besoin).
    /// Elle passe à la ligne entière si elle ne tient pas au bout de celle-ci.
    pub(super) fn badges_capsule(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, s: &Shown) {
        let lang = self.lang;
        let badges = s.profile.as_ref().map(|p| p.badges.as_slice()).unwrap_or_default();
        let mobile = s.presence.as_ref().filter(|p| on_mobile(p));
        let lead = usize::from(mobile.is_some());
        if badges.is_empty() && lead == 0 {
            return;
        }
        let open_id = Id::unique(("ferrocord.profils.badges-dépliés", user_id));
        let open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false);
        let shown = if open || badges.len() <= CAPSULE { badges.len() } else { CAPSULE };
        let more = badges.len() - shown;
        let (side, m, gap) = (t.profile_badge(), t.profile_chip_margin(), t.spacing.item_y);
        let plus = (more > 0).then(|| ui.painter().layout_no_wrap(format!("+{more}"), crate::ui::fonts::demi_font(t.typography.small + 1.0), t.colors.text));
        let plus_w = plus.as_ref().map_or(0.0, |g| g.size().x + gap + t.spacing.item_y);
        let n = shown + lead;
        // Rangées : une seule tant que tout tient dans la carte.
        let room = (ui.max_rect().width() - m.sum().x - plus_w).max(side);
        let per_row = (((room + gap) / (side + gap)).floor() as usize).clamp(1, n.max(1));
        let rows = n.div_ceil(per_row).max(1);
        let cols = n.min(per_row);
        let size = egui::vec2(cols as f32 * side + cols.saturating_sub(1) as f32 * gap + plus_w + m.sum().x, rows as f32 * side + (rows - 1) as f32 * gap + m.sum().y);
        if size.x > ui.available_size_before_wrap().x {
            ui.end_row();
        }
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        if !ui.is_rect_visible(rect) {
            return;
        }
        ui.painter().rect(rect, t.r_row(), tint(t), Stroke::new(t.shapes.border.max(1.0), t.hairline(false)), egui::StrokeKind::Inside);
        let slot = |i: usize| Rect::from_min_size(rect.min + m.left_top() + egui::vec2((i % per_row) as f32 * (side + gap), (i / per_row) as f32 * (side + gap)), egui::Vec2::splat(side));
        if let Some(p) = mobile {
            let at = slot(0);
            paint_phone(ui.painter(), at.shrink(side * 0.08), t.status_color(p.status), None);
            let resp = ui.interact(at, ui.scope_id().with("badge-mobile"), Sense::hover());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, pt::ON_MOBILE.get(lang)));
            resp.on_hover_text(pt::ON_MOBILE.get(lang));
        }
        for (i, b) in badges.iter().take(shown).enumerate() {
            let at = slot(i + lead);
            let resp = ui.interact(at, ui.scope_id().with(("badge", i)), Sense::click());
            match b.icon.map(|hash| self.images.get(ImageRef::BadgeIcon { hash }, side)) {
                Some(Image::Ready(tex)) => {
                    ui.painter().add(images::textured(at, egui::CornerRadius::ZERO, tex));
                }
                // Icône pas encore là : sa place, la description reste dans l'infobulle.
                _ => {
                    ui.painter().rect_filled(at.shrink(side * 0.12), t.r_row().at_most(4), t.colors.bg3);
                }
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &b.description));
            self.badge_hover(ui, t, user_id, s, b, resp);
        }
        if let Some(g) = plus {
            // « +N » : au bout de la rangée, centré sur sa hauteur.
            let last = slot(n - 1);
            let at = Rect::from_min_size(egui::pos2(last.right() + gap, last.top()), egui::vec2(g.size().x + t.spacing.item_y, side));
            let resp = ui.interact(at, ui.scope_id().with("badges-plus"), Sense::click());
            let tip = pt::BADGES_MORE.get(lang);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
            let ink = if resp.hovered() || resp.has_focus() { t.colors.text_strong } else { t.colors.text };
            ui.painter().galley(egui::pos2(at.left(), at.center().y - g.size().y / 2.0), g, ink);
            if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ui.data_mut(|d| d.insert_temp(open_id, true));
            }
        }
    }

    /// Fenêtre des badges d'une personne (clic sur un badge de sa carte) :
    /// à gauche ses badges puis ceux qu'elle peut encore obtenir, à droite le
    /// badge choisi, son avancement et ses paliers.
    pub(super) fn badge_window(&self, ctx: &egui::Context) {
        let Some((user_id, picked)) = ctx.data(|d| d.get_temp::<(u64, u64)>(badge_window_id())) else { return };
        let list: Vec<DirBadge> = self.store.people.lock().profiles().directory(user_id).iter().filter(|d| !d.hidden).cloned().collect();
        if list.is_empty() {
            ctx.data_mut(|d| d.remove_temp::<(u64, u64)>(badge_window_id()));
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let mut pick = picked;
        let cell = (t.layout.profile_avatar * 0.5).round();
        let gap = t.spacing.item_y;
        let left = 4.0 * cell + 3.0 * gap;
        let width = (ctx.content_rect().width() - 4.0 * t.spacing.card_pad).clamp(left * 1.5, left * 3.6);
        let out = crate::ui::widgets::sheet(ctx, &t, badge_window_id().with("fenêtre"), width, pt::BADGES.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(left);
                    self.badge_grid(ui, &t, &list, true, cell, &mut pick);
                    if list.iter().any(|d| !d.owned) {
                        ui.add_space(t.spacing.item_x);
                        heading(ui, &t, pt::BADGES_UNLOCKABLE.get(lang));
                        ui.add_space(gap);
                        self.badge_grid(ui, &t, &list, false, cell, &mut pick);
                    }
                });
                ui.add_space(t.spacing.card_pad);
                ui.vertical(|ui| {
                    if let Some(d) = list.iter().find(|d| d.id == pick).or(list.first()) {
                        self.badge_detail(ui, &t, d);
                    }
                });
            });
        });
        if out.closed {
            ctx.data_mut(|d| d.remove_temp::<(u64, u64)>(badge_window_id()));
        } else if pick != picked {
            ctx.data_mut(|d| d.insert_temp(badge_window_id(), (user_id, pick)));
        }
    }

    /// Cases des badges obtenus (`owned`) ou à obtenir ; un clic choisit le badge.
    fn badge_grid(&self, ui: &mut egui::Ui, t: &Tokens, list: &[DirBadge], owned: bool, cell: f32, pick: &mut u64) {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::splat(t.spacing.item_y);
            for d in list.iter().filter(|d| d.owned == owned) {
                let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(cell), Sense::click());
                let on = d.id == *pick;
                let fill = if on || resp.hovered() || resp.has_focus() { t.colors.selected } else { tint(t) };
                ui.painter().rect(rect, t.r_row(), fill, Stroke::new(t.shapes.border.max(1.0), if on { t.colors.accent } else { t.hairline(false) }), egui::StrokeKind::Inside);
                let inner = rect.shrink((cell * 0.2).round());
                paint_dir_art(ui, &self.images, inner, d.small_icon(), None);
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, &d.name));
                if resp.on_hover_text(&d.name).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    *pick = d.id;
                }
            }
        });
    }

    /// Détail d'un badge : image, nom et palier, ce que Discord en dit et la
    /// date d'obtention, puis l'avancement, la description et les paliers.
    fn badge_detail(&self, ui: &mut egui::Ui, t: &Tokens, d: &DirBadge) {
        let lang = self.lang;
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = t.spacing.item_y / 2.0;
            dir_art(ui, &self.images, d.best_art(), d.small_icon(), (t.layout.profile_avatar * 1.4).round());
            ui.add_space(t.spacing.item_y);
            dir_title(ui, t, d);
            let mut line = d.info.clone();
            if d.owned
                && let Some(ms) = d.unlocked_at()
            {
                let unlocked = pf::unlocked(lang, time::long_date(lang, ms));
                line = if line.is_empty() { unlocked } else { format!("{line} • {unlocked}") };
            }
            if !line.is_empty() {
                ui.add(egui::Label::new(RichText::new(line).small().color(t.colors.muted)).wrap());
            }
        });
        if d.progress.is_none() && d.description.is_empty() && d.tiers.is_empty() {
            return;
        }
        ui.add_space(t.spacing.item_x);
        egui::Frame::new().fill(tint(t)).corner_radius(t.r_row()).inner_margin(t.spacing.item_x).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = t.spacing.item_y;
            if let Some(p) = d.progress.as_ref().filter(|_| d.next.is_some()) {
                heading(ui, t, pt::BADGE_PROGRESS.get(lang));
                if !p.helper.is_empty() {
                    ui.label(RichText::new(&p.helper).color(t.colors.text));
                }
                let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), (t.spacing.item_y * 0.75).max(4.0)), Sense::hover());
                let done = ((p.current - p.floor) as f32 / (p.threshold - p.floor).max(1) as f32).clamp(0.0, 1.0);
                let r = bar.height() / 2.0;
                ui.painter().rect_filled(bar, r, t.colors.bg3);
                ui.painter().rect_filled(Rect::from_min_size(bar.min, egui::vec2(bar.width() * done, bar.height())), r, t.colors.accent);
                ui.add_space(t.spacing.item_y);
            }
            if !d.description.is_empty() {
                ui.add(egui::Label::new(RichText::new(&d.description).color(t.colors.text)).wrap());
            }
            if d.tiers.is_empty() {
                return;
            }
            ui.add_space(t.spacing.item_y);
            let icon = (t.layout.profile_avatar * 0.45).round();
            let col = icon * 2.4;
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_y, t.spacing.item_x);
                for tier in &d.tiers {
                    ui.allocate_ui_with_layout(egui::vec2(col, 0.0), egui::Layout::top_down(egui::Align::Center), |ui| {
                        ui.set_width(col);
                        ui.spacing_mut().item_spacing.y = 0.0;
                        dir_art(ui, &self.images, tier.small(), None, icon);
                        // Palier pas encore atteint : textes estompés.
                        let (name, sub) = if tier.owned { (t.colors.text_strong, t.colors.muted) } else { (t.colors.muted, t.colors.faint) };
                        if !tier.name.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&tier.name).small().family(crate::ui::fonts::demi()).strong().color(name)).wrap());
                        }
                        if !tier.milestone.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&tier.milestone).small().color(sub)).wrap());
                        }
                    });
                }
            });
        });
    }

    /// Infobulle détaillée d'un badge, et son lien au clic.
    fn badge_hover(&self, ui: &egui::Ui, t: &Tokens, user_id: u64, s: &Shown, b: &ProfileBadge, resp: egui::Response) -> egui::Response {
        // Badge connu du répertoire : son palier dans l'infobulle, la fenêtre des badges au clic.
        let dir = if resp.hovered() || resp.clicked() { dir_find(self.store.people.lock().profiles().directory(user_id), b).cloned() } else { None };
        let resp = resp.on_hover_ui(|ui| match &dir {
            Some(d) => dir_tip_ui(ui, t, &self.images, d),
            None => {
                let tip = badge_tip(self.lang, b, user_id, s.profile.as_ref(), self.store.now());
                badge_tip_ui(ui, t, &self.images, b, &tip);
            }
        });
        if let Some(d) = &dir {
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            if resp.clicked() {
                ui.data_mut(|x| x.insert_temp(badge_window_id(), (user_id, d.id)));
            }
            return resp;
        }
        match &b.link {
            Some(link) => {
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                if resp.clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(link));
                }
                resp
            }
            None => resp,
        }
    }

    fn badge_ui(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, s: &Shown, b: &ProfileBadge) -> egui::Response {
        let side = t.profile_badge();
        let icon = b.icon.map(|hash| ImageRef::BadgeIcon { hash });
        let resp = match icon.map(|i| self.images.get(i, side)) {
            Some(Image::Ready(tex)) => {
                let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
                ui.painter().add(images::textured(rect, egui::CornerRadius::ZERO, tex));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &b.description));
                resp
            }
            _ => ui.add(egui::Label::new(RichText::new(&b.description).small().color(t.colors.text)).sense(Sense::click())),
        };
        self.badge_hover(ui, t, user_id, s, b, resp)
    }

    /// « avatars empilés + N amis en commun • N serveurs en commun », en
    /// gris : chaque partie ouvre l'onglet correspondant du profil complet.
    pub(super) fn mutuals_ui(&self, ui: &mut egui::Ui, t: &Tokens, s: &Shown) -> Option<Tab> {
        let lang = self.lang;
        let p = s.profile.as_ref()?;
        let servers = p.mutual_guilds.as_ref().map(Vec::len).filter(|n| *n > 0);
        let friends = p.friends_count().filter(|n| *n > 0);
        if servers.is_none() && friends.is_none() {
            return None;
        }
        let faces: Vec<(Avatar, String)> = {
            let people = self.store.people.lock();
            let ids = p.mutual_friends.as_deref().unwrap_or_default();
            ids.iter().filter_map(|id| people.user(*id).map(|u| (u.avatar, u.display_name().to_string()))).take(STACKED).collect()
        };
        let mut out = None;
        let mut link = |ui: &mut egui::Ui, text: String, tab: Tab| {
            let resp = ui.add(egui::Label::new(RichText::new(text).small().color(t.colors.muted)).extend().sense(Sense::click()));
            if resp.hovered() {
                let y = resp.rect.bottom() - 1.0;
                ui.painter().hline(resp.rect.x_range(), y, Stroke::new(1.0, t.colors.muted));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                out = Some(tab);
            }
        };
        // Les deux parties sur une ligne, séparées par un point ; si la carte est trop
        // étroite : deux lignes, le point au bout de la première.
        let gap = t.spacing.item_y * 1.5;
        let side = Tokens::icon_side(t.typography.body);
        let step = (side * 0.7).round();
        let stack = if friends.is_some() && t.avatars_shown() && !faces.is_empty() { side + step * (faces.len() - 1) as f32 + gap } else { 0.0 };
        let small = egui::TextStyle::Small.resolve(ui.style());
        let wide = |text: &str| ui.painter().layout_no_wrap(text.to_owned(), small.clone(), t.colors.muted).size().x;
        let one_line = match (friends, servers) {
            (Some(f), Some(s)) => stack + wide(&pp::friends(lang, f as u64)) + gap + wide("•") + gap + wide(&pp::servers(lang, s as u64)) <= ui.available_width(),
            _ => true,
        };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            if let Some(n) = friends {
                if stack > 0.0 {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(stack - gap, side), Sense::hover());
                    for (i, (avatar, name)) in faces.iter().enumerate() {
                        let slot = super::util::pixel_square(ui, rect.min + egui::vec2(step * i as f32, 0.0), side);
                        super::util::paint_avatar(ui, &self.images, t, slot, avatar, name);
                    }
                }
                link(ui, pp::friends(lang, n as u64), Tab::Friends);
            }
            if let Some(n) = servers {
                if friends.is_some() {
                    // Le point reste au bout de la première ligne, comme dans le client.
                    ui.label(RichText::new("•").small().color(t.colors.faint));
                    if !one_line {
                        ui.end_row();
                    }
                }
                link(ui, pp::servers(lang, n as u64), Tab::Servers);
            }
        });
        out
    }
}

/// Jetons des lignes d'un menu ouvert depuis une carte à thème : ceux de
/// l'application (le menu n'est pas posé sur le dégradé du profil).
fn t_menu(app: &App) -> &Tokens {
    &app.theme
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::ImageHash;

    fn ms(y: i32, m: u32, d: u32) -> i64 {
        chrono::Utc.with_ymd_and_hms(y, m, d, 12, 0, 0).single().map(|d| d.timestamp_millis()).unwrap_or_else(|| panic!("date"))
    }

    fn badge(id: &str, description: &str) -> ProfileBadge {
        ProfileBadge { id: id.into(), description: description.into(), icon: ImageHash::parse("0123456789abcdef0123456789abcdef"), simple: None, link: None }
    }

    #[test]
    fn profile_badges_find_their_directory_entry() {
        let hash = |c: char| ContentHash::parse(&c.to_string().repeat(64));
        let dir = |id: u64, name: &str, simple: Option<ContentHash>| DirBadge {
            id,
            name: name.into(),
            description: String::new(),
            info: String::new(),
            icon: None,
            simple,
            art: None,
            owned: true,
            hidden: false,
            obtained_at: None,
            current: None,
            next: None,
            tier_at: Vec::new(),
            tiers: Vec::new(),
            progress: None,
        };
        let list = [dir(21, "Variété de jeux", hash('a')), dir(7, "Balance HypeSquad", None), dir(30, "Inconnu", hash('b')), dir(5, "Bravoure HypeSquad", None)];
        let found = |b: &ProfileBadge| dir_find(&list, b).map(|d| d.id);
        // Identifiant relevé, puis la même icône vectorielle, puis les mêmes mots.
        assert_eq!(found(&badge("game_variety", "x")), Some(21));
        assert_eq!(found(&badge("hypesquad_house_3", "HypeSquad Balance")), Some(7));
        assert_eq!(found(&ProfileBadge { simple: hash('b'), ..badge("nouveau", "Autre nom") }), Some(30));
        assert_eq!(found(&badge("hypesquad_house_1", "hypesquad bravoure")), Some(5));
        assert_eq!(found(&badge("staff", "Équipe Discord")), None);
        assert_eq!(found(&badge("vide", "")), None);
        // Illustration plus large que haute : entière, centrée, jamais déformée.
        let square = Rect::from_min_size(egui::pos2(10.0, 10.0), egui::Vec2::splat(100.0));
        let wide = contained(square, egui::vec2(156.0, 100.0));
        assert!((wide.width() - 100.0).abs() < 0.01 && (wide.height() - 64.1).abs() < 0.1 && wide.center() == square.center());
        assert_eq!(contained(square, egui::vec2(72.0, 72.0)), square);
        assert_eq!(contained(square, egui::Vec2::ZERO), square);
    }

    /// Identifiant d'un compte créé à cet instant.
    fn born(at_ms: i64) -> u64 {
        ((at_ms - crate::state::model::DISCORD_EPOCH as i64) as u64) << 22
    }

    /// Texte de l'ancienneté : années et mois pleins, comme la capture
    /// (compte du 4 mars 2018 vu le 15 novembre 2026 : « 8 ans, 8 mois »).
    #[test]
    fn tenure_counts_full_years_and_months() {
        let from = ms(2018, 3, 4);
        assert_eq!(tenure(Lang::Fr, from, ms(2026, 11, 15)), "8 ans, 8 mois");
        assert_eq!(tenure(Lang::En, from, ms(2026, 11, 15)), "8 years, 8 months");
        assert_eq!(tenure(Lang::Fr, from, ms(2026, 11, 3)), "8 ans, 7 mois", "le mois n'est pas révolu");
        assert_eq!(tenure(Lang::Fr, from, ms(2019, 3, 4)), "1 an");
        assert_eq!(tenure(Lang::En, from, ms(2019, 4, 20)), "1 year, 1 month");
        assert_eq!(tenure(Lang::Fr, from, ms(2018, 6, 4)), "3 mois");
        assert_eq!(tenure(Lang::Fr, from, ms(2018, 3, 16)), "12 jours");
        assert_eq!(tenure(Lang::En, from, ms(2018, 3, 5)), "1 day");
        assert_eq!(tenure(Lang::Fr, from, from), "0 jour");
        // Horloge en retard sur l'identifiant : jamais négatif.
        assert_eq!(tenure(Lang::Fr, from, from - 86_400_000 * 40), "0 jour");
        assert_eq!(tenure(Lang::Fr, i64::MAX, 0), "");
    }

    /// Infobulle : catégorie, palier, détail, d'après le badge et le profil.
    #[test]
    fn badge_tip_follows_the_badge_kind() {
        let now = ms(2026, 11, 15);
        let user = born(ms(2018, 3, 4));
        let p = Profile { premium_since: Some(ms(2020, 2, 1)), premium_guild_since: Some(ms(2021, 6, 1)), ..Default::default() };
        let tip = |b: &ProfileBadge, p: Option<&Profile>| badge_tip(Lang::Fr, b, user, p, now);
        // Ancienneté du compte : la description de Discord en catégorie, la durée calculée.
        let age = tip(&badge("account_age_8", "Ancienneté du compte"), Some(&p));
        assert_eq!(age, BadgeTip { category: Some("Ancienneté du compte".into()), tier: None, detail: "8 ans, 8 mois".into(), more: Vec::new() });
        assert!(is_account_age(&badge("inconnu", " Account Age ")) && !is_account_age(&badge("premium_tenure_12_month_v2", "x")));
        // Nitro à paliers : le palier vient de l'identifiant, la date du profil.
        let nitro = tip(&badge("premium_tenure_24_month_v2", "Earned on 1 Feb 2022. 2 years: Diamond"), Some(&p));
        assert_eq!((nitro.category.as_deref(), nitro.tier.as_deref()), (Some("Abonné depuis"), Some("Diamant")));
        assert_eq!(nitro.detail, time::long_date(Lang::Fr, ms(2020, 2, 1)));
        assert_eq!(badge_tip(Lang::En, &badge("premium_tenure_72_month", "x"), user, Some(&p), now).tier.as_deref(), Some("Opal"));
        // Sans date dans le profil : la description de Discord, palier gardé.
        let bare = tip(&badge("premium_tenure_1_month_v2", "Gagné le 1 janv. 2024"), None);
        assert_eq!(bare, BadgeTip { category: None, tier: Some("Bronze".into()), detail: "Gagné le 1 janv. 2024".into(), more: vec!["Palier 1 sur 8".into()] });
        // Rang dans l'échelle ; le palier suivant seulement s'il est à venir.
        assert_eq!(nitro.more, ["Palier 5 sur 8"]);
        let recent = Profile { premium_since: Some(ms(2025, 1, 1)), premium_guild_since: Some(ms(2026, 9, 1)), ..Default::default() };
        let next = tip(&badge("premium_tenure_12_month_v2", "x"), Some(&recent));
        assert_eq!(next.more, ["Palier 4 sur 8".to_string(), format!("Prochain palier\u{a0}: Diamant, le {}", time::long_date(Lang::Fr, ms(2027, 1, 1)))]);
        let lvl = tip(&badge("guild_booster_lvl2", "x"), Some(&recent));
        assert_eq!(lvl.tier.as_deref(), Some("Niveau 2"));
        assert_eq!(lvl.more, ["Palier 2 sur 9".to_string(), format!("Prochain palier\u{a0}: Niveau 3, le {}", time::long_date(Lang::Fr, ms(2026, 12, 1)))]);
        assert_eq!(tip(&badge("guild_booster_lvl9", "x"), Some(&p)).more, ["Palier 9 sur 9"]);
        assert_eq!(tip(&badge("premium_tenure_7_month", "d"), None).tier, None, "palier inconnu : pas de nom inventé");
        // Nitro simple et boost : la catégorie et la date.
        let sub = tip(&badge("premium", "Abonné depuis le 1 févr. 2020"), Some(&p));
        assert_eq!((sub.category.as_deref(), sub.tier.as_deref()), (Some("Abonné depuis"), None));
        let boost = tip(&badge("guild_booster_lvl3", "Booste depuis…"), Some(&p));
        assert_eq!((boost.category.as_deref(), boost.detail.as_str()), (Some("Boost de serveur depuis"), time::long_date(Lang::Fr, ms(2021, 6, 1)).as_str()));
        assert_eq!(tip(&badge("guild_booster_lvl3", "Booste depuis…"), None), BadgeTip { category: None, tier: None, detail: "Booste depuis…".into(), more: Vec::new() });
        // Tout autre badge : sa description, telle que Discord la donne.
        assert_eq!(tip(&badge("hypesquad_house_2", "HypeSquad Brilliance"), Some(&p)), BadgeTip { category: None, tier: None, detail: "HypeSquad Brilliance".into(), more: Vec::new() });
    }

    /// L'infobulle peint la catégorie, le palier en capitales et le détail.
    #[test]
    fn badge_tooltip_paints_category_tier_and_detail() {
        let ctx = egui::Context::default();
        let images = Images::disabled(ctx.clone());
        let t = Tokens::default();
        let b = badge("premium_tenure_24_month_v2", "x");
        let tip = BadgeTip { category: Some("Abonné depuis".into()), tier: Some("Diamant".into()), detail: "1er février 2020".into(), more: Vec::new() };
        let mut out = ctx.run_ui(egui::RawInput::default(), |ui| badge_tip_ui(ui, &t, &images, &b, &tip));
        out.textures_delta.clear();
        let mut texts = Vec::new();
        for s in &out.shapes {
            if let egui::Shape::Text(t) = &s.shape {
                texts.push(t.galley.job.text.clone());
            }
        }
        assert_eq!(texts, ["Abonné depuis", "DIAMANT", "1er février 2020"]);
    }

    #[test]
    fn mobile_presence_is_read_from_the_client_status() {
        use crate::state::people::ClientStatus;
        let p = |status, client| Presence { status, client, activities: Vec::new() };
        let mobile = ClientStatus { mobile: Some(Status::Dnd), ..Default::default() };
        assert!(on_mobile(&p(Status::Dnd, mobile)) && mobile_only(&p(Status::Dnd, mobile)));
        let both = ClientStatus { mobile: Some(Status::Online), desktop: Some(Status::Online), ..Default::default() };
        assert!(on_mobile(&p(Status::Online, both)) && !mobile_only(&p(Status::Online, both)));
        assert!(!on_mobile(&p(Status::Offline, mobile)) && !on_mobile(&p(Status::Online, ClientStatus::default())));
        // Plateformes : ordinateur, navigateur, mobile ; aucune hors ligne.
        assert_eq!(platforms(&p(Status::Online, both)), [true, false, true]);
        assert_eq!(platforms(&p(Status::Dnd, mobile)), [false, false, true]);
        let web = ClientStatus { web: Some(Status::Idle), desktop: Some(Status::Offline), ..Default::default() };
        assert_eq!(platforms(&p(Status::Idle, web)), [false, true, false]);
        assert_eq!(platforms(&p(Status::Offline, both)), [false; 3]);
    }

    /// Bulle : rien pour un statut vide ; trois lignes au plus, dans la largeur donnée.
    #[test]
    fn bubble_is_clamped_to_three_rows() {
        use crate::state::people::ActivityKind;
        let ctx = egui::Context::default();
        let t = Tokens::default();
        let a = |state: Option<&str>| Activity { kind: ActivityKind::Custom, name: "Custom Status".into(), details: None, state: state.map(Into::into), emoji: None, start_ms: None, rich: None };
        let mut rects = Vec::new();
        ctx.run_ui(egui::RawInput::default(), |ui| {
            for text in [None, Some("Court"), Some("mot ".repeat(200).as_str())] {
                rects.push(status_bubble(ui, &t, &a(text), egui::pos2(100.0, 100.0), 200.0, t.colors.bg1));
            }
            let font = egui::FontId::proportional(t.typography.body);
            let row = super::super::emoji::row_height(ui, &font);
            let long = rects[2].unwrap_or_else(|| panic!("bulle"));
            assert!(long.width() <= 200.5 && long.height() <= 3.0 * row + 2.0 * t.spacing.item_x + 1.0, "{long:?} pour des lignes de {row}");
            assert!(long.height() > 2.0 * row);
        })
        .textures_delta
        .clear();
        assert!(rects[0].is_none());
        assert!(rects[1].is_some_and(|r| r.width() < 120.0), "la bulle suit un texte court");
    }

    /// Thèmes de profil clairs et sombres : le texte reste lisible sur les
    /// blocs translucides (pastille des badges, bulle de statut) posés sur
    /// le dégradé, à ses deux extrémités.
    #[test]
    fn text_stays_readable_on_tinted_blocks_of_any_profile_theme() {
        use crate::theme::{contrast_ratio, MIN_MUTED, MIN_TEXT};
        let base = Tokens::default();
        for colors in [[0x5b_6eef, 0x2b_2b2b], [0xff_ffff, 0xff_ffff], [0x00_0000, 0x00_0000], [0xff_ff00, 0x00_00ff], [0xff_c0cb, 0xf5_f5dc], [0x1a_1a2e, 0x16_213e], [0x9b_59b6, 0x3b_1d5e]] {
            let th = super::super::cosmetics::Themed::new(&base, colors);
            let t = &th.tokens;
            for y in [0.0, 80.0, 400.0] {
                let under = th.at(0.0, y);
                let block = mix(under, t.colors.text_strong, BLOCK_TINT);
                for (name, fg, min) in [("fort", t.colors.text_strong, MIN_TEXT), ("texte", t.colors.text, MIN_TEXT), ("atténué", t.colors.muted, MIN_MUTED)] {
                    let got = contrast_ratio(fg, block);
                    assert!(got >= min, "{colors:06x?} à {y} : {name} = {got:.2}");
                }
            }
        }
    }
}
