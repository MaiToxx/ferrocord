//! Activité (lot C3) côté interface : activités des autres sur la carte de
//! profil (images, durée, écoute Spotify avec sa progression, boutons),
//! section « Jeux détectés » et programmes ajoutés de la page Activités, et
//! raccord avec le magasin (programmes ajoutés, adresse d'un bouton).

use super::App;
use crate::game_pick::GamePicker;
use crate::i18n::table::activity::{f as af, p as ap, t as at};
use crate::i18n::Lang;
use crate::images::{self, Image, Images};
use crate::state::activity::ManualGame;
use crate::state::model::ImageRef;
use crate::state::people::{Activity, ActivityImage, ActivityKind, Rich};
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText, Sense};
use std::sync::{Arc, Mutex};

/// Clic sur un bouton d'activité : l'adresse se demande à Discord.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ButtonClick {
    pub(super) session_id: String,
    pub(super) app_id: Option<u64>,
    pub(super) index: usize,
}

/// Durée lisible : `m:ss`, ou `h:mm:ss` au-delà d'une heure.
pub(super) fn duration(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h}:{m:02}:{s:02}") } else { format!("{m}:{s:02}") }
}

/// Côté de la grande image d'activité, tiré du thème.
fn large_side(t: &Tokens) -> f32 {
    (t.layout.profile_avatar * 0.75).round()
}

/// Clé de cache d'une image d'activité (ses adresses viennent de `net`).
fn image_key(url: &str) -> ImageRef {
    let h = url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3));
    ImageRef::Media { owner: h, slot: 0xC3, rev: (h >> 32) as u32 }
}

pub(super) fn paint_asset(ui: &egui::Ui, images: &Images, t: &Tokens, rect: egui::Rect, img: &ActivityImage) -> bool {
    let Some(url) = img.url.as_deref() else { return false };
    if !ui.is_rect_visible(rect) {
        return false;
    }
    match images.get_media(image_key(url), url, rect.size()) {
        Image::Ready(tex) => {
            ui.painter().add(images::textured(rect, t.r_media(), tex));
            true
        }
        Image::Pending | Image::Failed => false,
    }
}

/// Grande image (et petite en coin) ; rien n'est réservé sans image.
fn images_ui(ui: &mut egui::Ui, images: &Images, t: &Tokens, r: &Rich) {
    let Some(large) = r.large.as_ref().filter(|i| i.url.is_some()) else { return };
    let side = large_side(t);
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
    if !paint_asset(ui, images, t, rect, large) {
        ui.painter().rect_filled(rect, t.r_media(), t.colors.bg3);
    }
    let mut tip = large.text.clone();
    if let Some(small) = r.small.as_ref().filter(|i| i.url.is_some()) {
        let s = (side * 0.36).round();
        let corner = egui::Rect::from_min_size(rect.max - egui::Vec2::splat(s * 0.8), egui::Vec2::splat(s));
        ui.painter().circle_filled(corner.center(), s / 2.0 + t.profile_ring() / 2.0, t.colors.bg1);
        paint_asset(ui, images, t, corner, small);
        if let Some(text) = &small.text {
            tip = Some(match tip {
                Some(l) => format!("{l}\n{text}"),
                None => text.clone(),
            });
        }
    }
    if let Some(tip) = tip {
        resp.on_hover_text(tip);
    }
}

/// Barre de progression d'une écoute (début et fin connus).
fn progress_ui(ui: &mut egui::Ui, t: &Tokens, start: i64, end: i64, now: i64) {
    let total = (end - start).max(1);
    let done = (now - start).clamp(0, total);
    let c = &t.colors;
    let h = (t.typography.small * 0.35).round().max(2.0);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
    let r = egui::CornerRadius::same((h / 2.0) as u8);
    ui.painter().rect_filled(rect, r, c.bg3);
    let mut fill = rect;
    fill.set_width(rect.width() * done as f32 / total as f32);
    ui.painter().rect_filled(fill, r, c.text);
    ui.horizontal(|ui| {
        ui.label(RichText::new(duration(done)).small().color(c.muted));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| ui.label(RichText::new(duration(total)).small().color(c.muted)));
    });
}

/// Une activité (hors statut personnalisé) : images, textes, durée ou
/// progression, boutons. Rend le bouton cliqué, s'il y en a un.
pub(super) fn activity_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, a: &Activity, now: i64) -> Option<ButtonClick> {
    let c = &t.colors;
    let rich = a.rich();
    let spotify = rich.is_some_and(|r| r.spotify);
    let mut click = None;
    ui.horizontal_top(|ui| {
        if let Some(r) = rich {
            images_ui(ui, images, t, r);
        }
        ui.vertical(|ui| {
            ui.label(RichText::new(super::profile::activity_line(lang, a)).family(crate::ui::fonts::demi()).strong().color(c.text));
            if spotify || a.kind == ActivityKind::Listening {
                // Écoute : titre, artiste, album.
                if let Some(d) = &a.details {
                    ui.label(RichText::new(d).color(c.text));
                }
                if let Some(s) = &a.state {
                    ui.label(RichText::new(af::by(lang, s.replace(';', ","))).small().color(c.muted));
                }
                if let Some(album) = rich.and_then(|r| r.large.as_ref()).and_then(|l| l.text.as_deref()) {
                    ui.label(RichText::new(af::on_album(lang, album)).small().color(c.muted));
                }
            } else {
                let detail = match a.kind {
                    ActivityKind::Streaming => a.state.clone(),
                    _ => match (&a.details, &a.state) {
                        (Some(d), Some(s)) => Some(format!("{d} \u{2014} {s}")),
                        (Some(d), None) => Some(d.clone()),
                        (None, s) => s.clone(),
                    },
                };
                if let Some(d) = detail {
                    ui.label(RichText::new(d).small().color(c.muted));
                }
            }
            match (a.start_ms, rich.and_then(|r| r.end_ms)) {
                (Some(start), Some(end)) if end > start => {
                    progress_ui(ui, t, start, end, now);
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                }
                (Some(start), _) if start <= now => {
                    ui.label(RichText::new(af::elapsed(lang, duration(now - start))).small().color(c.faint));
                    ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                }
                _ => {}
            }
        });
    });
    let r = rich?;
    ui.horizontal_wrapped(|ui| {
        if spotify
            && let Some(id) = &r.sync_id
            && ui.button(at::LISTEN_SPOTIFY.get(lang)).clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://open.spotify.com/track/{id}")));
        }
        if a.kind == ActivityKind::Streaming
            && let Some(url) = &r.url
            && ui.button(at::WATCH_STREAM.get(lang)).on_hover_text(url).clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        // Boutons de la présence : leur adresse n'est connue qu'après un clic.
        if let Some(session) = &r.session_id {
            for (i, label) in r.buttons.iter().enumerate() {
                if ui.button(label).clicked() {
                    click = Some(ButtonClick { session_id: session.clone(), app_id: r.app_id, index: i });
                }
            }
        }
    });
    click
}

/// Clic dans la carte d'activité d'un profil ([`activity_card`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum CardClick {
    Button(ButtonClick),
    /// « … » de ma propre activité : la page Activités des Paramètres.
    Settings,
    /// « … » de l'activité d'un autre : l'onglet « Activité » de son profil complet.
    Full,
}

/// Titre de la carte selon le genre : « Joue à », « Écoute Spotify »…
fn head_text(lang: Lang, a: &Activity) -> String {
    match a.kind {
        ActivityKind::Streaming => at::HEAD_STREAMING.get(lang).to_string(),
        ActivityKind::Listening if !a.name.is_empty() => format!("{} {}", at::HEAD_LISTENING.get(lang), a.name),
        ActivityKind::Listening => at::HEAD_LISTENING.get(lang).to_string(),
        ActivityKind::Watching => at::HEAD_WATCHING.get(lang).to_string(),
        ActivityKind::Competing => at::HEAD_COMPETING.get(lang).to_string(),
        ActivityKind::Playing | ActivityKind::Custom => at::HEAD_PLAYING.get(lang).to_string(),
    }
}

/// Côté de l'image d'une carte d'activité (56 pour un avatar de profil de 80).
fn card_image(t: &Tokens) -> f32 {
    (t.layout.profile_avatar * 0.7).round()
}

/// Ligne de texte de la carte, coupée par « … » à la largeur qui reste.
fn card_line(ui: &mut egui::Ui, text: RichText) {
    ui.add(egui::Label::new(text).truncate().selectable(false));
}

/// Bouton bordé sur toute la largeur d'une carte (boutons d'une activité,
/// « Voir le profil complet ») : texte centré, fond du bloc, filet autour.
pub(super) fn bordered_button(ui: &mut egui::Ui, t: &Tokens, text: &str, width: f32, height: f32) -> egui::Response {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
    let resp = super::widgets::button_in(ui, t, super::widgets::Kind::Secondary, text, None, rect);
    if ui.is_rect_visible(rect) {
        ui.painter().rect_stroke(rect, t.r_control(), egui::Stroke::new(t.shapes.border.max(1.0), t.hairline(true)), egui::StrokeKind::Inside);
    }
    resp
}

/// Carte d'activité d'un profil (carte, panneau de MP, carte de mon compte),
/// comme dans le client : cadre plus clair aux coins arrondis, titre (« Joue
/// à ») et menu « … », image et petite icône superposée, nom de l'application
/// seul en gras, deux lignes coupées, taille du groupe, manette et chrono
/// verts, boutons sur toute la largeur. `icon` : icône de l'application quand
/// l'activité n'a pas d'image ; `own` : c'est la mienne (le menu ouvre les
/// réglages). Le chrono ne redemande une image (une par seconde) que s'il est
/// à l'écran.
#[allow(clippy::too_many_arguments)]
pub(super) fn activity_card(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, a: &Activity, icon: Option<ImageRef>, now: i64, own: bool, copy_app: bool) -> Option<CardClick> {
    let c = &t.colors;
    let rich = a.rich();
    let listening = rich.is_some_and(|r| r.spotify) || a.kind == ActivityKind::Listening;
    let mut click = None;
    let pad = (t.spacing.item_x * 1.5).round();
    let gap = t.spacing.item_x;
    egui::Frame::new().fill(super::profile_parts::tint(t)).corner_radius(t.r_embed()).inner_margin(egui::Margin::same(pad as i8)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        let width = ui.available_width();
        // Titre et menu « … ».
        ui.horizontal(|ui| {
            super::profile_parts::heading(ui, t, &head_text(lang, a));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let side = Tokens::icon_side(t.typography.body);
                let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
                let tip = at::CARD_MENU.get(lang);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                let hot = resp.hovered() || resp.has_focus();
                crate::theme::icons::paint(crate::theme::icons::Icon::More, ui.painter(), rect.shrink(1.0), if hot { c.text_strong } else { c.muted });
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                egui::Popup::menu(&resp).id(Id::unique(("ferrocord.activite.menu", own, a.name.as_str()))).show(|ui| {
                    use crate::theme::icons::Icon;
                    if own {
                        if super::menu::item(ui, t, at::MENU_SETTINGS.get(lang), Some(Icon::Gear), false).clicked() {
                            click = Some(CardClick::Settings);
                        }
                    } else if super::menu::item(ui, t, at::MENU_FULL.get(lang), Some(Icon::Gamepad), false).clicked() {
                        click = Some(CardClick::Full);
                    }
                    if let Some(id) = rich.and_then(|r| r.app_id).filter(|_| copy_app)
                        && super::menu::item(ui, t, at::MENU_COPY_APP.get(lang), Some(Icon::Id), false).clicked()
                    {
                        ui.ctx().copy_text(id.to_string());
                    }
                });
            });
        });
        ui.add_space(gap);
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = pad;
            let side = card_image(t);
            let large = rich.and_then(|r| r.large.as_ref()).filter(|i| i.url.is_some());
            // Image de l'activité, sinon l'icône de l'application, sinon une manette.
            let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
            if ui.is_rect_visible(rect) {
                let drawn = match (large, icon) {
                    (Some(l), _) => paint_asset(ui, images, t, rect, l),
                    (None, Some(icon)) => match images.get(icon, side) {
                        Image::Ready(tex) => {
                            ui.painter().add(images::textured(rect, t.r_media(), tex));
                            true
                        }
                        _ => false,
                    },
                    (None, None) => false,
                };
                if !drawn {
                    ui.painter().rect_filled(rect, t.r_media(), c.bg3);
                    if large.is_none() {
                        super::user_panel::paint_gamepad(ui.painter(), rect.shrink((side * 0.26).round()), c.text, c.bg3);
                    }
                }
            }
            let mut tip = large.and_then(|l| l.text.clone());
            if let Some(small) = rich.and_then(|r| r.small.as_ref()).filter(|i| i.url.is_some() && large.is_some()) {
                // Petite icône ronde, à cheval sur le coin bas droit, détourée du fond.
                let s = (side * 0.36).round();
                let corner = egui::Rect::from_min_size(rect.max - egui::Vec2::splat(s * 0.72), egui::Vec2::splat(s));
                ui.painter().circle_filled(corner.center(), s / 2.0 + 2.0, t.colors.bg1);
                paint_asset(ui, images, t, corner, small);
                if let Some(text) = &small.text {
                    tip = Some(tip.map_or_else(|| text.clone(), |l| format!("{l}\n{text}")));
                }
            }
            if let Some(tip) = tip {
                resp.on_hover_text(tip);
            }
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 1.0;
                ui.set_width((width - side - pad).max(1.0));
                let small = t.typography.small + 1.0;
                // Écoute : le titre du morceau en gras, puis l'artiste ; sinon le nom de l'application seul.
                let (title, lines): (&str, [Option<String>; 2]) = if listening {
                    (a.details.as_deref().unwrap_or(&a.name), [a.state.as_ref().map(|s| af::by(lang, s.replace(';', ","))), rich.and_then(|r| r.large.as_ref()).and_then(|l| l.text.as_deref()).map(|al| af::on_album(lang, al))])
                } else if a.kind == ActivityKind::Streaming {
                    (a.details.as_deref().unwrap_or(&a.name), [a.state.clone(), None])
                } else {
                    (&a.name, [a.details.clone(), a.state.clone()])
                };
                card_line(ui, RichText::new(title).size(t.typography.button + 1.0).family(crate::ui::fonts::demi()).strong().color(c.text_strong));
                for line in lines.into_iter().flatten() {
                    card_line(ui, RichText::new(line).size(small).color(c.text));
                }
                let party = rich.and_then(|r| r.party);
                let since = a.start_ms.filter(|s| *s <= now && rich.and_then(|r| r.end_ms).is_none_or(|e| e <= *s));
                if party.is_some() || since.is_some() {
                    ui.add_space(2.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = t.spacing.item_y;
                        let icon_side = Tokens::icon_side(small);
                        if let Some((n, max)) = party {
                            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(icon_side), Sense::hover());
                            crate::theme::icons::paint(crate::theme::icons::Icon::Members, ui.painter(), r, c.muted);
                            ui.label(RichText::new(af::party(lang, n, max)).size(small).color(c.text));
                            ui.add_space(t.spacing.item_y);
                        }
                        if let Some(start) = since {
                            // Manette et chrono verts, chiffres à chasse fixe.
                            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(icon_side), Sense::hover());
                            super::user_panel::paint_gamepad(ui.painter(), r, c.online, super::cosmetics::mix(t.colors.bg1, c.text_strong, 0.07));
                            let clock = ui.label(RichText::new(duration(now - start)).size(small).family(egui::FontFamily::Monospace).color(c.online));
                            if ui.is_rect_visible(clock.rect) {
                                ui.ctx().request_repaint_after(std::time::Duration::from_millis((1000 - (now - start).rem_euclid(1000)) as u64));
                            }
                        }
                    });
                }
            });
        });
        if let (Some(start), Some(end)) = (a.start_ms, rich.and_then(|r| r.end_ms))
            && end > start
        {
            ui.add_space(gap);
            let bar = ui.scope(|ui| progress_ui(ui, t, start, end, now)).response;
            if ui.is_rect_visible(bar.rect) {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        let Some(r) = rich else { return };
        let button = |ui: &mut egui::Ui, label: &str| {
            ui.add_space(gap);
            bordered_button(ui, t, label, width, t.spacing.control_height)
        };
        if r.spotify
            && let Some(id) = &r.sync_id
            && button(ui, at::LISTEN_SPOTIFY.get(lang)).clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("https://open.spotify.com/track/{id}")));
        }
        if a.kind == ActivityKind::Streaming
            && let Some(url) = &r.url
            && button(ui, at::WATCH_STREAM.get(lang)).on_hover_text(url).clicked()
        {
            ui.ctx().open_url(egui::OpenUrl::new_tab(url));
        }
        // Boutons de la présence : leur adresse n'est connue qu'après un clic.
        if let Some(session) = &r.session_id {
            for (i, label) in r.buttons.iter().enumerate() {
                if button(ui, label).clicked() {
                    click = Some(CardClick::Button(ButtonClick { session_id: session.clone(), app_id: r.app_id, index: i }));
                }
            }
        }
    });
    click
}

fn picker_id() -> Id {
    Id::unique("ferrocord.activite.programme")
}

type SharedPicker = Arc<Mutex<GamePicker>>;

impl App {
    /// Une fois par image : programmes ajoutés vers le magasin, adresse
    /// d'un bouton à ouvrir, programme choisi dans la boîte « Ouvrir ».
    pub(super) fn activity_tick(&mut self) {
        if self.store.activity.manual != self.local.prefs.games {
            self.store.activity.set_manual(self.local.prefs.games.clone());
        }
        // Jeux vus récemment et leurs masques : les préférences font foi ; un
        // jeu qui démarre s'y range (une écriture par partie, pas par relevé).
        if self.store.activity.recent != self.local.prefs.recent_games {
            self.store.activity.recent = self.local.prefs.recent_games.clone();
        }
        let a = &self.store.activity;
        if a.running.iter().any(|g| !a.recent.iter().any(|r| r.is(g) && r.seen_ms >= g.start_ms)) {
            let mut p = self.local.prefs.clone();
            crate::state::activity::note_running(&mut p.recent_games, &self.store.activity.running);
            self.store.activity.recent = p.recent_games.clone();
            self.set_local_prefs(p);
        }
        self.rpc_tick();
        // Passage de logique, hors de l'image où `route_links` relève les
        // liens d'egui : ouvert directement par C6, sans `DISCORD_TOKEN`.
        if let Some(url) = self.store.activity.open_url.take() {
            self.open_link(&url);
        }
        let picked = self.ctx.data(|d| d.get_temp::<SharedPicker>(picker_id())).and_then(|p| p.lock().ok()?.poll());
        if let Some(path) = picked {
            self.add_game(&path);
        }
    }

    fn add_game(&mut self, path: &str) {
        let Some(g) = ManualGame::from_path(path) else { return };
        let mut p = self.local.prefs.clone();
        if p.games.iter().any(|x| x.path == g.path) {
            return self.notify(at::ALREADY.get(self.lang).to_string());
        }
        if p.games.len() >= crate::state::activity::MAX_MANUAL {
            return self.notify(at::TOO_MANY.get(self.lang).to_string());
        }
        p.games.push(g);
        self.set_local_prefs(p);
    }

    /// Jeux détectés et programmes ajoutés (page Activités des Paramètres).
    pub(super) fn activity_games_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let c = &t.colors;
        let now = crate::state::store::now_ms();
        super::settings::section(ui, t, at::SEC_DETECTED.get(lang));
        let on = self.store.account.bool(crate::state::usersettings::schema::Key::ShowCurrentGame) == Some(true);
        let a = &self.store.activity;
        let muted = |ui: &mut egui::Ui, s: &str| {
            // Texte d'état : petit, comme les notes des autres pages.
            ui.add(egui::Label::new(RichText::new(s).small().color(c.muted)).wrap());
        };
        if !crate::state::activity::DETECTION {
            muted(ui, at::UNSUPPORTED.get(lang));
        } else if !on {
            muted(ui, at::DETECTION_OFF.get(lang));
        } else {
            if self.store.own_presence().is_some_and(|p| p.status == "invisible") {
                muted(ui, at::INVISIBLE.get(lang));
            }
            match (&a.index, a.failed) {
                (Some(i), _) => muted(ui, &ap::known(lang, i.len() as u64)),
                (None, true) => muted(ui, at::LIST_FAILED.get(lang)),
                (None, false) => muted(ui, at::LIST_LOADING.get(lang)),
            }
            if a.running.is_empty() {
                muted(ui, at::NONE_RUNNING.get(lang));
            }
            for g in &a.running {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&g.name).family(crate::ui::fonts::demi()).strong().color(c.text));
                    ui.label(RichText::new(af::running_since(lang, duration(now - g.start_ms))).small().color(c.muted));
                    if g.app_id.is_none() {
                        ui.label(RichText::new(at::ADDED_TAG.get(lang)).small().color(c.faint));
                    }
                    if a.hidden(g) {
                        ui.label(RichText::new(at::HIDDEN_TAG.get(lang)).small().color(c.faint));
                    }
                });
            }
            if !a.running.is_empty() {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        if crate::state::activity::DETECTION {
            muted(ui, at::SCAN_TIP.get(lang));
        }

        // Jeux vus récemment : un interrupteur par jeu ; coupé, le jeu n'est
        // jamais annoncé (ni encart ni présence).
        super::settings::section(ui, t, at::SEC_RECENT.get(lang));
        if self.local.prefs.recent_games.is_empty() {
            muted(ui, at::NONE_RECENT.get(lang));
        }
        let mut flip = None;
        for (i, r) in self.local.prefs.recent_games.iter().enumerate() {
            let live = self.store.activity.running.iter().any(|g| r.is(g));
            let detail = if live { at::NOW_TAG.get(lang) } else { at::RECENT_TIP.get(lang) };
            ui.push_id(("jeu-recent", i), |ui| {
                if let Some(on) = super::settings::toggle(ui, &r.name, detail, Some(!r.hidden), true) {
                    flip = Some((i, !on));
                }
            });
        }
        if let Some((i, hidden)) = flip {
            let mut p = self.local.prefs.clone();
            if let Some(r) = p.recent_games.get_mut(i) {
                r.hidden = hidden;
            }
            self.set_local_prefs(p);
        }

        super::settings::section(ui, t, at::SEC_ADDED.get(lang));
        let mut remove = None;
        if self.local.prefs.games.is_empty() {
            muted(ui, at::NONE_ADDED.get(lang));
        }
        for (i, g) in self.local.prefs.games.iter().enumerate() {
            ui.horizontal(|ui| {
                if ui.button(at::REMOVE.get(lang)).clicked() {
                    remove = Some(i);
                }
                ui.label(RichText::new(&g.name).color(c.text)).on_hover_text(&g.path);
                ui.label(RichText::new(g.file()).small().color(c.faint));
            });
        }
        if let Some(i) = remove {
            let mut p = self.local.prefs.clone();
            p.games.remove(i);
            self.set_local_prefs(p);
        }
        if GamePicker::CAN_PICK {
            let ctx = ui.ctx().clone();
            let picker = ctx.data_mut(|d| {
                d.get_temp_mut_or_insert_with(picker_id(), || {
                    let wake = ctx.clone();
                    Arc::new(Mutex::new(GamePicker::new(Arc::new(move || wake.request_repaint()))))
                })
                .clone()
            });
            let busy = picker.lock().is_ok_and(|p| p.busy());
            if ui.add_enabled(!busy, egui::Button::new(at::ADD.get(lang))).on_hover_text(at::ADD_TIP.get(lang)).clicked()
                && let Ok(p) = picker.lock()
            {
                #[cfg(windows)]
                p.pick(at::PICK_TITLE.get(lang), at::PICK_FILTER.get(lang));
                #[cfg(not(windows))]
                drop(p);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(duration(0), "0:00");
        assert_eq!(duration(65_000), "1:05");
        assert_eq!(duration(3_725_000), "1:02:05");
        assert_eq!(duration(-5), "0:00");
    }

    #[test]
    fn image_keys_differ_by_url() {
        assert_ne!(image_key("https://a/1.png"), image_key("https://a/2.png"));
        assert_eq!(image_key("https://a/1.png"), image_key("https://a/1.png"));
    }
}
