//! Bandeau d'un appel privé ou de groupe, au-dessus de la conversation du MP,
//! comme dans le client : la conversation reste affichée (en-tête, messages,
//! saisie) et l'appel occupe une bande noire sous l'en-tête.
//!
//! - Rangée d'avatars des participants : anneau vert pour qui parle, pastille
//!   « micro coupé » ou « casque coupé », avatar estompé et points d'attente
//!   pour qui n'a pas encore décroché.
//! - Commandes en blocs arrondis : [micro ⌄ | caméra ⌄], [partage, tableau de
//!   sons, « … »], puis « Raccrocher » en rouge. Les chevrons ouvrent les
//!   menus de périphériques du panneau utilisateur (`user_panel.rs`) ; la
//!   sourdine (casque) est dans « … », comme le reste quand la place manque.
//! - Appel entrant : « Accepter » (vert) et « Refuser » (rouge) à la place des
//!   commandes ; appel en cours sans moi : « Rejoindre l'appel ».
//! - Caméra ou partage d'écran dans l'appel : le bandeau s'agrandit et montre
//!   les tuiles vidéo (`video/`), sa bordure basse se tire à la souris ; le
//!   bouton « Agrandir l'appel » passe en plein cadre (la vue d'appel).
//!
//! Rien n'est animé hors des points d'attente d'une sonnerie : aucun redessin
//! au repos, ni hors appel ni pendant l'appel.

use super::call::clock;
use super::menu::{self, Lead, Line};
use super::util::paint_avatar;
use super::App;
use crate::i18n::table::audio::t as at;
use crate::i18n::table::calls::{f, t as tx};
use crate::i18n::table::profile::t as pt;
use crate::i18n::table::video::t as xt;
use crate::i18n::table::voice::t as vt;
use crate::state::calls::CallAction;
use crate::state::voice::MyVoice;
use crate::state::Store;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, pos2, vec2, Color32, Id, Rect, Sense, Stroke, Vec2};

/// Hauteurs possibles d'un bloc de commandes, de la plus aérée à la plus serrée.
const HEIGHTS: [f32; 3] = [60.0, 52.0, 44.0];
/// Marge intérieure d'un bloc de commandes.
const PAD: f32 = 4.0;
/// Marge du bandeau sur ses côtés.
const SIDE: f32 = 16.0;
/// Bandeau agrandi (vidéo) : hauteur minimale, et place laissée à la conversation.
const VIDEO_MIN: f32 = 260.0;
const CHAT_KEEP: f32 = 180.0;
/// Épaisseur de la poignée de la bordure basse.
const GRIP: f32 = 6.0;
/// Largeur minimale d'un menu du bandeau.
const MENU_WIDTH: f32 = 260.0;

/// Ce que le bandeau retient d'une image à l'autre.
#[derive(Default)]
pub(super) struct BandUi {
    /// Appel de MP montré en plein cadre (la vue d'appel) plutôt qu'en bandeau.
    pub(super) wide: Option<u64>,
    /// Hauteur choisie à la souris pour le bandeau agrandi.
    height: Option<f32>,
    /// Le bandeau a montré « Accepter / Refuser » pour ce salon à cette image :
    /// la fenêtre d'appel entrant ne les répète pas.
    answering: Option<(u64, u64)>,
}

impl BandUi {
    /// Le bandeau de ce salon propose déjà de décrocher (à cette image ou à la précédente).
    pub(super) fn answers(&self, channel: u64, frame: u64) -> bool {
        self.answering.is_some_and(|(c, at)| c == channel && frame <= at + 1)
    }
}

/// Un participant montré dans le bandeau.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Person {
    pub id: u64,
    /// On le fait sonner : il n'a pas encore décroché.
    pub ringing: bool,
    pub speaking: bool,
    pub muted: bool,
    pub deafened: bool,
}

/// Participants d'un appel : ceux qui y sont (moi dès que je rejoins, avant
/// l'écho de la passerelle), puis ceux qu'on fait sonner.
pub(super) fn band_people(store: &Store, cid: u64) -> Vec<Person> {
    let me = store.me.as_ref().map(|u| u.id);
    let v = &store.voice;
    let mut out: Vec<Person> = v
        .members(cid)
        .map(|s| {
            let mine = Some(s.user_id) == me;
            Person {
                id: s.user_id,
                ringing: false,
                speaking: v.speaking.contains(&s.user_id),
                muted: s.muted() || (mine && (v.self_mute || v.self_deaf)),
                deafened: s.deafened() || (mine && v.self_deaf),
            }
        })
        .collect();
    if let Some(me) = me
        && v.me == Some(MyVoice { guild_id: None, channel_id: cid })
        && !out.iter().any(|p| p.id == me)
    {
        out.insert(0, Person { id: me, ringing: false, speaking: v.speaking.contains(&me), muted: v.self_mute || v.self_deaf, deafened: v.self_deaf });
    }
    let waiting: Vec<u64> = match store.calls.call(cid) {
        Some(c) => c.ringing.clone(),
        // « Appeler » demandé, l'appel n'existe pas encore : les destinataires vont sonner.
        None if store.calls.calling(cid) => match store.relations.dm_info(cid) {
            Some(d) => d.recipients.clone(),
            None => store.dms.iter().find(|d| d.id == cid).map(|d| d.recipients.iter().map(|(id, _)| *id).collect()).unwrap_or_default(),
        },
        None => Vec::new(),
    };
    for id in waiting {
        if !out.iter().any(|p| p.id == id) {
            out.push(Person { id, ringing: true, speaking: false, muted: false, deafened: false });
        }
    }
    out
}

/// Ce que la rangée de commandes montre, selon la place : elle se resserre
/// d'abord, puis « … » absorbe le tableau de sons, le partage, les chevrons
/// et enfin la caméra. Micro, « … » et « Raccrocher » restent toujours.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Fit {
    /// Hauteur des blocs.
    pub h: f32,
    pub chevrons: bool,
    pub camera: bool,
    pub share: bool,
    pub sounds: bool,
}

impl Fit {
    /// Largeur d'une icône du premier bloc, et de son chevron.
    fn cell(self) -> f32 {
        (self.h * 0.87).round()
    }
    fn chevron(self) -> f32 {
        if self.chevrons { (self.h * 0.5).round() } else { 0.0 }
    }
    fn first(self) -> f32 {
        let n = if self.camera { 2.0 } else { 1.0 };
        n * (self.cell() + self.chevron()) + 2.0 * PAD
    }
    fn second(self) -> f32 {
        (1.0 + f32::from(u8::from(self.share)) + f32::from(u8::from(self.sounds))) * self.h
    }
    fn hangup(self) -> f32 {
        (self.h * 1.35).round()
    }
    fn gap(self) -> f32 {
        (self.h * 0.27).round()
    }
    /// Arrondi des blocs.
    fn radius(self) -> f32 {
        (self.h * 0.27).round()
    }
    pub(super) fn width(self) -> f32 {
        self.first() + self.second() + self.hangup() + 2.0 * self.gap()
    }
}

/// Commandes qui tiennent dans `width` ; `camera`, `share`, `sounds` : ce que
/// le système et le salon proposent.
/// Les blocs ne dépassent pas `max_h` de haut (bandeau agrandi dans une fenêtre basse).
pub(super) fn fit(width: f32, max_h: f32, camera: bool, share: bool, sounds: bool) -> Fit {
    let full = |h| Fit { h, chevrons: true, camera, share, sounds };
    if let Some(f) = HEIGHTS.into_iter().filter(|h| *h <= max_h.max(HEIGHTS[2])).map(full).find(|f| f.width() <= width) {
        return f;
    }
    let mut f = full(HEIGHTS[2]);
    for step in 0..4 {
        match step {
            0 => f.sounds = false,
            1 => f.share = false,
            2 => f.chevrons = false,
            _ => f.camera = false,
        }
        if f.width() <= width {
            break;
        }
    }
    f
}

/// Hauteurs du bandeau replié selon la place offerte à la conversation :
/// (marge haute, côté d'un avatar, écart, marge basse).
fn sizes(zone: f32) -> (f32, f32, f32, f32) {
    if zone >= 560.0 {
        (28.0, 100.0, 20.0, 24.0)
    } else if zone >= 400.0 {
        (16.0, 72.0, 14.0, 16.0)
    } else {
        (10.0, 48.0, 10.0, 10.0)
    }
}

/// Rangée d'avatars dans `width` : (côté, nombre montré, nombre replié dans « +N »).
/// Les avatars rétrécissent jusqu'à 60 % de `side`, puis les derniers se replient.
pub(super) fn avatar_row(width: f32, side: f32, n: usize) -> (f32, usize, usize) {
    if n == 0 {
        return (side, 0, 0);
    }
    let gap = |s: f32| (s * 0.24).round();
    let need = |s: f32, k: usize| k as f32 * s + (k as f32 - 1.0) * gap(s);
    if need(side, n) <= width {
        return (side, n, 0);
    }
    let small = (side * 0.6).round();
    let s = (width / (n as f32 + (n as f32 - 1.0) * 0.24) - 1.0).floor().clamp(small, side);
    if need(s, n) <= width {
        return (s, n, 0);
    }
    // Une place est gardée pour « +N ».
    let slots = (((width + gap(small)) / (small + gap(small))).floor() as usize).max(2);
    let shown = (slots - 1).min(n - 1);
    (small, shown, n - shown)
}

/// Hauteur du bandeau agrandi : celle choisie, sinon la moitié de la zone, en
/// laissant toujours de la place à la conversation.
pub(super) fn video_height(chosen: Option<f32>, zone: f32) -> f32 {
    let max = (zone - CHAT_KEEP).max(VIDEO_MIN.min(zone * 0.7));
    chosen.unwrap_or((zone * 0.5).round()).clamp(VIDEO_MIN.min(max), max)
}

fn mic_menu_id() -> Id {
    Id::unique("ferrocord.appel.menu-micro")
}
fn camera_menu_id() -> Id {
    Id::unique("ferrocord.appel.menu-camera")
}
fn more_menu_id() -> Id {
    Id::unique("ferrocord.appel.menu-plus")
}

/// Aspect d'une commande à bascule.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Look {
    Plain,
    /// Coupé : icône rouge sur fond rouge sombre (micro).
    Off,
    /// En marche : icône verte sur fond vert sombre (caméra, partage).
    On,
}

/// Zone cliquable d'une commande : fond au survol, contour du clavier, nom accessible.
fn hit(ui: &mut egui::Ui, t: &Tokens, rect: Rect, key: &str, tip: &str, radius: f32) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(("bandeau-appel", key)), Sense::click());
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_filled(rect, radius, t.colors.row_hover);
    }
    super::keys::focus_ring(ui, t, &resp);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn ink(t: &Tokens, look: Look, hot: bool) -> Color32 {
    match look {
        Look::Off => t.colors.danger,
        Look::On => t.colors.online,
        Look::Plain if hot => t.colors.text_strong,
        Look::Plain => t.colors.text,
    }
}

/// Bouton plein (raccrocher, accepter, refuser, rejoindre) : icône, puis texte s'il y en a un.
fn solid(ui: &mut egui::Ui, t: &Tokens, rect: Rect, key: &str, (icon, text, tip): (Icon, Option<&str>, &str), fill: Color32, radius: f32) -> egui::Response {
    let resp = ui.interact(rect, ui.scope_id().with(("bandeau-appel", key)), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let p = ui.painter();
    p.rect_filled(rect, radius, if hot { super::cosmetics::mix(fill, Color32::WHITE, 0.12) } else { fill });
    let side = (rect.height() * 0.45).round();
    match text {
        None => icons::paint(icon, p, Rect::from_center_size(rect.center(), Vec2::splat(side)), t.colors.on_accent),
        Some(text) => {
            let g = p.layout_no_wrap(text.to_owned(), super::fonts::medium_font(t.typography.button), t.colors.on_accent);
            let gap = t.spacing.item_x;
            let left = rect.center().x - (side + gap + g.size().x) / 2.0;
            icons::paint(icon, p, Rect::from_min_size(pos2(left, rect.center().y - side / 2.0), Vec2::splat(side)), t.colors.on_accent);
            p.galley(pos2(left + side + gap, rect.center().y - g.size().y / 2.0), g, t.colors.on_accent);
        }
    }
    super::keys::focus_ring(ui, t, &resp);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
    if text.is_some() { resp } else { super::widgets::tip(resp, t, tip) }
}

/// Avatar rond dans ce thème (sinon carré arrondi).
fn round_avatar(t: &Tokens, rect: Rect) -> bool {
    f32::from(t.r_avatar(rect.width()).nw) * 2.0 >= rect.width() - 1.5
}

/// Voile posé sur un avatar (personne qui n'a pas encore décroché).
fn veil(p: &egui::Painter, t: &Tokens, rect: Rect, color: Color32) {
    if round_avatar(t, rect) {
        p.circle_filled(rect.center(), rect.width() / 2.0 + 0.5, color);
    } else {
        p.rect_filled(rect, t.r_avatar(rect.width()), color);
    }
}

/// Anneau autour d'un avatar, détaché d'un point (personne qui parle).
fn ring_around(p: &egui::Painter, t: &Tokens, rect: Rect, stroke: Stroke) {
    let out = stroke.width / 2.0 + 1.0;
    if round_avatar(t, rect) {
        p.circle_stroke(rect.center(), rect.width() / 2.0 + out, stroke);
    } else {
        p.rect_stroke(rect.expand(out), t.r_avatar(rect.width()), stroke, egui::StrokeKind::Middle);
    }
}

/// Geste fait dans le bandeau, appliqué après le dessin.
#[derive(Clone, Copy)]
enum Act {
    Mute,
    Deaf,
    Camera,
    Share,
    StopShare,
    Sounds,
    Leave,
    Join,
    Accept,
    Decline,
    Ring,
    StopRinging,
    Wide,
}

/// Ce que les commandes ont besoin de savoir de l'appel.
#[derive(Clone, Copy)]
struct Ctl {
    /// Rangée des commandes.
    row: Rect,
    fit: Fit,
    /// Ce que le système propose (caméra, partage, tableau de sons).
    tools: (bool, bool, bool),
    /// D'autres que moi sonnent encore ; faire sonner est possible.
    others_ringing: bool,
    can_ring: bool,
    /// Durée de l'appel, pour l'infobulle de « Raccrocher ».
    since_ms: Option<i64>,
}

impl App {
    /// Bandeau de l'appel d'un MP, entre l'en-tête et la conversation. Rien
    /// quand aucun appel n'y est en cours.
    pub(super) fn call_band(&mut self, ui: &mut egui::Ui, cid: u64) {
        let joined = self.store.voice.me == Some(MyVoice { guild_id: None, channel_id: cid });
        if !joined && self.calls_ui.band.wide == Some(cid) {
            self.calls_ui.band.wide = None;
        }
        let calling = self.store.calls.calling(cid);
        let call = self.store.calls.call(cid).cloned();
        if call.is_none() && !calling && !joined {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let me = self.store.me.as_ref().map(|u| u.id);
        let people = band_people(&self.store, cid);
        let incoming = !joined && self.store.incoming_calls().contains(&cid);
        let video = joined && self.call_has_video(cid);
        let unavailable = call.as_ref().is_some_and(|c| c.unavailable);
        let others_ringing = call.as_ref().is_some_and(|c| c.ringing.iter().any(|u| Some(*u) != me));
        let can_ring = call.is_some() && !calling && !self.store.calls.is_busy(if others_ringing { CallAction::StopRinging } else { CallAction::Ring }, cid);
        let since_ms = call.as_ref().and_then(|c| c.started_ms()).map(|s| self.store.now() - s);
        // Place offerte à la conversation et à l'appel, sous l'en-tête.
        let zone = ui.available_height();
        let mut act = None;
        egui::Panel::top("call_band").frame(egui::Frame::new().fill(t.call_bg())).show_separator_line(false).show(ui, |ui| {
            let width = ui.available_width();
            let (top, avatar, gap, bottom) = sizes(zone);
            let tools = self.voice_tools();
            let tools = (tools.camera, tools.share, tools.sounds);
            // Bandeau agrandi : commandes plus basses, la place va à la vidéo.
            let max_h = if !video { HEIGHTS[0] } else if zone >= 560.0 { HEIGHTS[1] } else { HEIGHTS[2] };
            let fit = fit(width - 2.0 * SIDE, max_h, tools.0, tools.1, tools.2);
            let note = if unavailable { ui.text_style_height(&egui::TextStyle::Small) + gap / 2.0 } else { 0.0 };
            let height = if video { video_height(self.calls_ui.band.height, zone) } else { top + avatar + gap + note + fit.h + bottom };
            let (rect, _) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let bottom = if video { (bottom / 2.0).max(GRIP + 2.0) } else { bottom };
            let row = Rect::from_min_size(pos2(rect.left() + SIDE, rect.bottom() - bottom - fit.h), vec2(width - 2.0 * SIDE, fit.h));
            if video {
                let pad = t.spacing.chat_pad.min(SIDE);
                let area = Rect::from_min_max(rect.min + Vec2::splat(pad), pos2(rect.right() - pad, row.top() - pad));
                self.call_tiles_ui(ui, cid, area, true);
            } else {
                let avatars = Rect::from_min_size(pos2(rect.left() + SIDE, rect.top() + top), vec2(width - 2.0 * SIDE, avatar));
                self.band_avatars(ui, &t, avatars, &people);
                if unavailable {
                    ui.painter().text(pos2(rect.center().x, avatars.bottom() + gap / 2.0), egui::Align2::CENTER_TOP, tx::UNAVAILABLE.get(lang), egui::FontId::proportional(t.typography.small), t.colors.danger);
                }
            }
            let ctl = Ctl { row, fit, tools, others_ringing, can_ring, since_ms };
            act = if joined {
                self.band_controls(ui, &t, ctl)
            } else {
                if incoming {
                    self.calls_ui.band.answering = Some((cid, ui.ctx().cumulative_frame_nr()));
                }
                band_answer(ui, &t, lang, ctl, incoming)
            };
            // Filet sous le bandeau, comme dans le client.
            ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(t.shapes.border.max(1.0), t.colors.border));
            if video {
                // Bordure basse : se tire pour changer la hauteur.
                let grip = Rect::from_min_max(pos2(rect.left(), rect.bottom() - GRIP), rect.max);
                let resp = ui.interact(grip, ui.scope_id().with("bandeau-appel-poignee"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeVertical);
                if resp.dragged()
                    && let Some(at) = resp.interact_pointer_pos()
                {
                    self.calls_ui.band.height = Some(video_height(Some(at.y - rect.top()), zone));
                }
                if resp.hovered() || resp.dragged() {
                    ui.painter().hline(rect.x_range(), rect.bottom() - 1.0, Stroke::new(2.0, t.colors.accent));
                }
            }
        });
        let Some(act) = act else { return };
        match act {
            Act::Mute => self.voice_toggle_mute(),
            Act::Deaf => self.voice_toggle_deaf(),
            Act::Camera => self.toggle_camera(),
            Act::Share => self.request_share_picker(),
            Act::StopShare => self.store.video_stop_share(),
            Act::Sounds => {
                if self.calls_ui.soundboard == Some(cid) {
                    self.calls_ui.soundboard = None;
                } else {
                    self.open_soundboard(cid);
                }
            }
            Act::Leave => self.store.voice_leave(),
            Act::Join => self.store.voice_join_call(cid),
            Act::Accept => self.store.accept_call(cid),
            Act::Decline => {
                self.store.decline_call(cid);
            }
            Act::Ring => {
                self.store.ring(cid, None);
            }
            Act::StopRinging => {
                self.store.stop_ringing(cid);
            }
            Act::Wide => self.calls_ui.band.wide = Some(cid),
        }
        self.flush_commands();
    }

    /// Rangée d'avatars centrée dans `row`.
    fn band_avatars(&mut self, ui: &mut egui::Ui, t: &Tokens, row: Rect, people: &[Person]) {
        let lang = self.lang;
        let me = self.store.me.as_ref().map(|u| u.id);
        let (side, shown, extra) = avatar_row(row.width(), row.height(), people.len());
        let gap = (side * 0.24).round();
        let slots = shown + usize::from(extra > 0);
        let total = slots as f32 * side + (slots as f32 - 1.0).max(0.0) * gap;
        let mut x = (row.center().x - total / 2.0).round();
        let y = (row.center().y - side / 2.0).round();
        let bg = t.call_bg();
        for person in people.iter().take(shown) {
            let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(side));
            x += side + gap;
            let (name, avatar) = self.voice_person(person.id);
            let label = if person.ringing {
                f::person_ringing(lang, &name)
            } else if person.deafened {
                f::person_state(lang, &name, tx::DEAFENED.get(lang))
            } else if person.muted {
                f::person_state(lang, &name, tx::MUTED.get(lang))
            } else {
                name.clone()
            };
            let resp = ui.interact(rect, ui.scope_id().with(("bandeau-appel-personne", person.id)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &label));
            paint_avatar(ui, &self.images, t, rect, &avatar, &name);
            let p = ui.painter();
            if person.ringing {
                // Pas encore décroché : avatar estompé, points d'attente par-dessus.
                veil(p, t, rect, bg.gamma_multiply(0.72));
                super::motion::dots(ui, Rect::from_center_size(rect.center(), vec2(side * 0.44, side * 0.18)), t.on_media());
            } else if person.speaking {
                let ring = (side * 0.035).clamp(2.0, 4.0);
                ring_around(p, t, rect, Stroke::new(ring, t.colors.online));
            }
            if !person.ringing && (person.muted || person.deafened) {
                // Pastille en bas à droite : casque coupé, sinon micro coupé.
                let r = (side * 0.16).max(7.0);
                let c = pos2(rect.right() - r * 0.9, rect.bottom() - r * 0.9);
                p.circle_filled(c, r + 2.0, bg);
                p.circle_filled(c, r, t.colors.bg3);
                let icon = if person.deafened { Icon::HeadphonesOff } else { Icon::MicOff };
                icons::paint(icon, p, Rect::from_center_size(c, Vec2::splat(r * 1.25)), t.colors.danger);
            }
            let resp = super::widgets::tip(resp, t, &label);
            // Volume et « muet pour moi » d'une personne connectée, au clic droit.
            if !person.ringing && Some(person.id) != me {
                resp.context_menu(|ui| {
                    ui.set_min_width(MENU_WIDTH * 0.8);
                    ui.label(egui::RichText::new(&name).family(super::fonts::demi()).color(t.colors.text_strong));
                    self.user_menu_volume(ui, person.id);
                });
            }
        }
        if extra > 0 {
            let rect = Rect::from_min_size(pos2(x, y), Vec2::splat(side));
            let p = ui.painter();
            p.rect_filled(rect, t.r_avatar(side), t.colors.bg3);
            p.text(rect.center(), egui::Align2::CENTER_CENTER, f::others(lang, extra), super::fonts::medium_font(side * 0.3), t.colors.text);
        }
    }

    /// Commandes de l'appel où je suis : [micro ⌄ | caméra ⌄] [partage, sons, « … »] [raccrocher].
    fn band_controls(&mut self, ui: &mut egui::Ui, t: &Tokens, c: Ctl) -> Option<Act> {
        let lang = self.lang;
        let fit = c.fit;
        let (mute, deaf) = (self.store.voice.self_mute, self.store.voice.self_deaf);
        let camera = self.store.video.camera;
        let sharing = self.store.video.share.is_some();
        let keys = &self.nav.bindings;
        let mute_tip = super::keys::with_chord(lang, keys, crate::state::keys::Action::ToggleMute, if mute { vt::UNMUTE.get(lang) } else { vt::MUTE.get(lang) });
        let deaf_tip = super::keys::with_chord(lang, keys, crate::state::keys::Action::ToggleDeafen, if deaf { vt::UNDEAFEN.get(lang) } else { vt::DEAFEN.get(lang) });
        let mut act = None;
        let radius = fit.radius();
        let inner = (radius - PAD).max(2.0);
        let (fill, stroke) = (t.colors.bg2, Stroke::new(t.shapes.border.max(1.0), t.colors.border));
        let glyph = (fit.h * 0.4).round();
        let mut x = (c.row.center().x - fit.width() / 2.0).round().max(c.row.left());
        let y = c.row.top();

        // Premier bloc : micro et caméra, chacun suivi de son chevron.
        let block = Rect::from_min_size(pos2(x, y), vec2(fit.first(), fit.h));
        ui.painter().rect(block, radius, fill, stroke, egui::StrokeKind::Inside);
        let mut gx = block.left() + PAD;
        for input in [true, false] {
            if !input && !fit.camera {
                continue;
            }
            let group = Rect::from_min_size(pos2(gx, y + PAD), vec2(fit.cell() + fit.chevron(), fit.h - 2.0 * PAD));
            gx = group.right();
            let icon_rect = Rect::from_min_size(group.min, vec2(fit.cell(), group.height()));
            let chev_rect = Rect::from_min_max(pos2(icon_rect.right(), group.top()), group.max);
            let (icon, look, tip, key, menu_tip, menu) = if input {
                (if mute { Icon::MicOff } else { Icon::Mic }, if mute { Look::Off } else { Look::Plain }, mute_tip.as_str(), "micro", pt::PANEL_INPUT_MENU.get(lang), mic_menu_id())
            } else {
                (if camera { Icon::Camera } else { Icon::CameraOff }, if camera { Look::On } else { Look::Plain }, if camera { xt::CAMERA_OFF } else { xt::CAMERA_ON }.get(lang), "camera", tx::CAMERA_MENU.get(lang), camera_menu_id())
            };
            if look != Look::Plain {
                ui.painter().rect_filled(group, inner, super::cosmetics::mix(fill, ink(t, look, false), 0.3));
            }
            let resp = hit(ui, t, icon_rect, key, tip, inner);
            icons::paint(icon, ui.painter(), Rect::from_center_size(icon_rect.center(), Vec2::splat(glyph)), ink(t, look, resp.hovered()));
            let resp = super::widgets::tip(resp, t, tip);
            if resp.clicked() {
                act = Some(if input { Act::Mute } else { Act::Camera });
            }
            if fit.chevrons {
                let open = egui::Popup::is_id_open(ui.ctx(), menu);
                let chev = hit(ui, t, chev_rect, if input { "micro-menu" } else { "camera-menu" }, menu_tip, inner);
                let s = (fit.chevron() * 0.45).round().max(8.0);
                icons::paint(if open { Icon::ChevronUp } else { Icon::ChevronDown }, ui.painter(), Rect::from_center_size(chev_rect.center(), Vec2::splat(s)), ink(t, look, open || chev.hovered()));
                let chev = if open { chev } else { super::widgets::tip(chev, t, menu_tip) };
                egui::Popup::menu(&chev).id(menu).show(|ui| if input { self.audio_menu(ui, true) } else { self.camera_menu(ui) });
            } else {
                // Sans place pour le chevron : le menu au clic droit sur l'icône.
                resp.context_menu(|ui| if input { self.audio_menu(ui, true) } else { self.camera_menu(ui) });
            }
        }
        x = block.right() + fit.gap();

        // Second bloc : partage d'écran, tableau de sons, « … ».
        let block = Rect::from_min_size(pos2(x, y), vec2(fit.second(), fit.h));
        ui.painter().rect(block, radius, fill, stroke, egui::StrokeKind::Inside);
        let cell = |i: &mut f32| {
            let r = Rect::from_min_size(pos2(*i, y), Vec2::splat(fit.h)).shrink(PAD);
            *i += fit.h;
            r
        };
        let mut cx = block.left();
        if fit.share {
            let r = cell(&mut cx);
            let tip = if sharing { xt::STOP_SHARE } else { xt::SHARE }.get(lang);
            let look = if sharing { Look::On } else { Look::Plain };
            if sharing {
                ui.painter().rect_filled(r, inner, super::cosmetics::mix(fill, t.colors.online, 0.3));
            }
            let resp = hit(ui, t, r, "partage", tip, inner);
            icons::paint(if sharing { Icon::ScreenOff } else { Icon::Screen }, ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(glyph)), ink(t, look, resp.hovered()));
            if super::widgets::tip(resp, t, tip).clicked() {
                act = Some(if sharing { Act::StopShare } else { Act::Share });
            }
        }
        if fit.sounds {
            let r = cell(&mut cx);
            let tip = tx::SOUNDBOARD.get(lang);
            let resp = hit(ui, t, r, "sons", tip, inner);
            icons::paint(Icon::Audio, ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(glyph)), ink(t, Look::Plain, resp.hovered()));
            if super::widgets::tip(resp, t, tip).clicked() {
                act = Some(Act::Sounds);
            }
        }
        let r = cell(&mut cx);
        let tip = tx::MORE.get(lang);
        let open = egui::Popup::is_id_open(ui.ctx(), more_menu_id());
        let more = hit(ui, t, r, "plus", tip, inner);
        icons::paint(Icon::More, ui.painter(), Rect::from_center_size(r.center(), Vec2::splat(glyph)), ink(t, Look::Plain, open || more.hovered()));
        let more = if open { more } else { super::widgets::tip(more, t, tip) };
        egui::Popup::menu(&more).id(more_menu_id()).show(|ui| {
            ui.set_min_width(MENU_WIDTH);
            // Le casque n'est pas dans la rangée du client : la sourdine est ici.
            if menu::item(ui, t, &deaf_tip, Some(if deaf { Icon::HeadphonesOff } else { Icon::Headphones }), false).clicked() {
                act = Some(Act::Deaf);
            }
            // Ce qui ne tenait pas dans la rangée.
            if !fit.camera && c.tools.0 && menu::item(ui, t, if camera { xt::CAMERA_OFF } else { xt::CAMERA_ON }.get(lang), Some(if camera { Icon::Camera } else { Icon::CameraOff }), false).clicked() {
                act = Some(Act::Camera);
            }
            if !fit.share && c.tools.1 && menu::item(ui, t, if sharing { xt::STOP_SHARE } else { xt::SHARE }.get(lang), Some(if sharing { Icon::ScreenOff } else { Icon::Screen }), false).clicked() {
                act = Some(if sharing { Act::StopShare } else { Act::Share });
            }
            if !fit.sounds && c.tools.2 && menu::item(ui, t, tx::SOUNDBOARD.get(lang), Some(Icon::Audio), false).clicked() {
                act = Some(Act::Sounds);
            }
            menu::separator(ui, t);
            if !fit.chevrons {
                menu::submenu(ui, t, pt::PANEL_INPUT_MENU.get(lang), |ui| self.audio_menu(ui, true));
                if c.tools.0 {
                    menu::submenu(ui, t, tx::CAMERA_MENU.get(lang), |ui| self.camera_menu(ui));
                }
            }
            menu::submenu(ui, t, pt::PANEL_OUTPUT_MENU.get(lang), |ui| self.audio_menu(ui, false));
            menu::separator(ui, t);
            if c.can_ring && menu::item(ui, t, if c.others_ringing { tx::STOP_RINGING } else { tx::RING }.get(lang), Some(Icon::Phone), false).clicked() {
                act = Some(if c.others_ringing { Act::StopRinging } else { Act::Ring });
            }
            if menu::item(ui, t, xt::EXPAND.get(lang), Some(Icon::Fullscreen), false).clicked() {
                act = Some(Act::Wide);
            }
        });
        x = block.right() + fit.gap();

        // Raccrocher : rouge plein, plus large que haut.
        let hang = Rect::from_min_size(pos2(x, y), vec2(fit.hangup(), fit.h));
        let tip = match c.since_ms {
            Some(ms) => f::leave_after(lang, clock(ms)),
            None => tx::LEAVE.get(lang).to_string(),
        };
        if solid(ui, t, hang, "raccrocher", (Icon::HangUp, None, &tip), t.danger_fill(), radius).clicked() {
            act = Some(Act::Leave);
        }
        act
    }

    /// Menu du chevron de la caméra : caméra des appels, aperçu avant de
    /// l'allumer, « Paramètres vidéo ». Aucune caméra n'est allumée ici.
    fn camera_menu(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        ui.set_min_width(MENU_WIDTH);
        ui.set_max_width(MENU_WIDTH);
        let cameras = self.camera_devices();
        let base = (*self.local.prefs.video).clone();
        let mut v = base.clone();
        if menu::radio(ui, &t, at::DEFAULT_DEVICE.get(lang), v.camera.is_none()).clicked() {
            v.camera = None;
        }
        match &cameras {
            None => {
                ui.add_enabled_ui(false, |ui| menu::plain(ui, &t, at::LOADING_DEVICES.get(lang), None, false));
            }
            Some(list) => {
                for d in list {
                    if menu::radio(ui, &t, &d.name, v.camera.as_deref() == Some(d.id.as_str())).clicked() {
                        v.camera = Some(d.id.clone());
                    }
                }
            }
        }
        menu::separator(ui, &t);
        menu::check(ui, &t, at::CAMERA_PREVIEW.get(lang), &mut v.camera_preview);
        menu::separator(ui, &t);
        if menu::line(ui, &t, Line::new(Lead::Icon(Icon::Gear), tx::VIDEO_SETTINGS.get(lang))).clicked() {
            self.open_video_settings();
            ui.close();
        }
        if v != base {
            self.video_prefs_set(v);
        }
    }

    /// Hauteur choisie du bandeau agrandi (tests).
    #[cfg(test)]
    pub(super) fn call_band_height(&self) -> Option<f32> {
        self.calls_ui.band.height
    }
}

/// Appel où je ne suis pas : « Accepter » et « Refuser » s'il me fait sonner,
/// sinon « Rejoindre l'appel ».
fn band_answer(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, c: Ctl, incoming: bool) -> Option<Act> {
    let fit = c.fit;
    let (w, gap) = (fit.hangup(), fit.gap());
    let y = c.row.top();
    let mut act = None;
    if incoming {
        let x = (c.row.center().x - (2.0 * w + gap) / 2.0).round();
        let accept = Rect::from_min_size(pos2(x, y), vec2(w, fit.h));
        if solid(ui, t, accept, "accepter", (Icon::Phone, None, tx::ACCEPT.get(lang)), t.success_fill(), fit.radius()).clicked() {
            act = Some(Act::Accept);
        }
        let decline = Rect::from_min_size(pos2(accept.right() + gap, y), vec2(w, fit.h));
        if solid(ui, t, decline, "refuser", (Icon::HangUp, None, tx::DECLINE.get(lang)), t.danger_fill(), fit.radius()).clicked() {
            act = Some(Act::Decline);
        }
    } else {
        let text = tx::JOIN_CALL.get(lang);
        let label = ui.painter().layout_no_wrap(text.to_owned(), super::fonts::medium_font(t.typography.button), t.colors.on_accent).size().x;
        let w = (label + fit.h * 0.45 + t.spacing.item_x + 2.0 * fit.gap()).min(c.row.width());
        let rect = Rect::from_min_size(pos2((c.row.center().x - w / 2.0).round(), y), vec2(w, fit.h));
        if solid(ui, t, rect, "rejoindre", (Icon::Phone, Some(text), text), t.success_fill(), fit.radius()).clicked() {
            act = Some(Act::Join);
        }
    }
    act
}

#[cfg(test)]
mod tests {
    use super::*;

    /// La rangée se resserre d'abord ; ensuite « … » absorbe le tableau de
    /// sons, le partage, les chevrons puis la caméra. Elle ne déborde jamais
    /// d'une largeur raisonnable.
    #[test]
    fn controls_tighten_then_fold_into_more() {
        let all = |h| Fit { h, chevrons: true, camera: true, share: true, sounds: true };
        assert_eq!(fit(2000.0, 60.0, true, true, true), all(60.0));
        assert_eq!(fit(all(60.0).width(), 60.0, true, true, true), all(60.0));
        assert_eq!(fit(all(60.0).width() - 1.0, 60.0, true, true, true), all(52.0));
        assert_eq!(fit(all(52.0).width() - 1.0, 60.0, true, true, true), all(44.0));
        let f = fit(all(44.0).width() - 1.0, 60.0, true, true, true);
        assert_eq!(f, Fit { sounds: false, ..all(44.0) });
        let f = fit(f.width() - 1.0, 60.0, true, true, true);
        assert_eq!(f, Fit { sounds: false, share: false, ..all(44.0) });
        let f = fit(f.width() - 1.0, 60.0, true, true, true);
        assert_eq!(f, Fit { sounds: false, share: false, chevrons: false, ..all(44.0) });
        let f = fit(10.0, 60.0, true, true, true);
        assert_eq!(f, Fit { h: 44.0, sounds: false, share: false, chevrons: false, camera: false });
        // Hauteur plafonnée (bandeau agrandi) : jamais sous la plus serrée.
        assert_eq!(fit(2000.0, 52.0, true, true, true), all(52.0));
        assert_eq!(fit(2000.0, 10.0, true, true, true), all(44.0));
        // Ce que le système ne propose pas n'est jamais montré.
        assert_eq!(fit(2000.0, 60.0, false, false, true), Fit { h: 60.0, chevrons: true, camera: false, share: false, sounds: true });
        // 1280 × 720, colonne de profil ouverte : environ 600 de large, tout tient.
        assert!(all(60.0).width() <= 600.0 - 2.0 * SIDE, "{}", all(60.0).width());
        // Téléphone (360 de large) : micro, « … » et « Raccrocher » tiennent.
        assert!(fit(360.0 - 2.0 * SIDE, 60.0, true, true, true).width() <= 360.0 - 2.0 * SIDE);
    }

    /// Les avatars rétrécissent, puis les derniers se replient dans « +N ».
    #[test]
    fn avatars_shrink_then_fold() {
        assert_eq!(avatar_row(600.0, 100.0, 0), (100.0, 0, 0));
        assert_eq!(avatar_row(600.0, 100.0, 2), (100.0, 2, 0));
        let (side, shown, extra) = avatar_row(600.0, 100.0, 6);
        assert!((60.0..100.0).contains(&side) && shown == 6 && extra == 0, "{side} {shown} {extra}");
        assert!(6.0 * side + 5.0 * (side * 0.24).round() <= 600.0);
        let (side, shown, extra) = avatar_row(300.0, 100.0, 10);
        assert_eq!(side, 60.0);
        assert_eq!(shown + extra, 10);
        assert!(extra > 0 && (shown + 1) as f32 * 60.0 + shown as f32 * 14.0 <= 300.0, "{shown} {extra}");
    }

    /// Bandeau agrandi : la moitié de la zone par défaut, la hauteur choisie
    /// ensuite, toujours en laissant de la place à la conversation.
    #[test]
    fn video_band_height_is_bounded() {
        assert_eq!(video_height(None, 800.0), 400.0);
        assert_eq!(video_height(Some(100.0), 800.0), VIDEO_MIN);
        assert_eq!(video_height(Some(5000.0), 800.0), 800.0 - CHAT_KEEP);
        assert_eq!(video_height(Some(450.0), 800.0), 450.0);
        // Fenêtre très basse : jamais plus haut que la zone.
        assert!(video_height(None, 300.0) < 300.0);
    }

    #[test]
    fn the_band_answers_for_two_frames() {
        let b = BandUi { answering: Some((5, 10)), ..BandUi::default() };
        assert!(b.answers(5, 10) && b.answers(5, 11));
        assert!(!b.answers(5, 12) && !b.answers(6, 10));
    }
}

#[cfg(test)]
#[path = "call_band_tests.rs"]
mod band_tests;
