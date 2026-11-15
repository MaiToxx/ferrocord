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

fn paint_asset(ui: &egui::Ui, images: &Images, t: &Tokens, rect: egui::Rect, img: &ActivityImage) -> bool {
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
                });
            }
            if !a.running.is_empty() {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
            }
        }
        if crate::state::activity::DETECTION {
            muted(ui, at::SCAN_TIP.get(lang));
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
