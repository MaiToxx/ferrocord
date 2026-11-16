//! Encart du jeu détecté (lot « activité »), au-dessus du panneau
//! utilisateur comme dans le client : icône du jeu (image de l'application
//! sur le CDN, sinon une manette), son nom, le temps écoulé (« depuis
//! 12 min » ; Ferrocord n'a pas de clips, donc pas de ligne de raccourci),
//! et, seulement dans un appel ou un salon vocal, le bouton « Partager
//! l'écran de ce jeu » qui ouvre le sélecteur de partage. Un clic ailleurs
//! sur l'encart ouvre la page Activités des Paramètres.
//!
//! L'encart est le haut de la carte du panneau utilisateur : pas d'air entre
//! les deux, coins du bas droits. Rien n'est animé ; le seul réveil demandé
//! est le prochain changement de minute du temps écoulé.

use super::settings::Page;
use super::util::pixel_square;
use super::App;
use crate::i18n::table::activity::{f as af, t as at};
use crate::i18n::Lang;
use crate::images::{self, Image};
use crate::theme::icons::{self, Icon};
use eframe::egui::{self, Align, Layout, RichText, Sense, Vec2};

/// Temps écoulé en mots, à la minute près.
pub(super) fn since(lang: Lang, ms: i64) -> String {
    let min = ms.max(0) / 60_000;
    match (min / 60, min % 60) {
        (0, 0) => at::JUST_STARTED.get(lang).to_string(),
        (0, m) => af::since_minutes(lang, m),
        (h, m) => af::since_hours(lang, h, format!("{m:02}")),
    }
}

/// Attente jusqu'au prochain changement du texte de [`since`].
pub(super) fn next_minute(ms: i64) -> std::time::Duration {
    std::time::Duration::from_millis((60_000 - ms.max(0) % 60_000) as u64)
}

impl App {
    /// Jeu annoncé aux autres en ce moment (manette verte sous mon nom) :
    /// celui de ma présence telle qu'elle part, ou une Rich Presence locale.
    pub(super) fn panel_activity(&self) -> bool {
        self.store.own_presence().is_some_and(|p| p.game.is_some() || p.rich.is_some())
    }

    /// Un jeu est détecté et pas masqué : l'encart se montre.
    pub(super) fn game_card_shown(&self) -> bool {
        self.store.activity.current().is_some()
    }

    /// Encart du jeu détecté ; rend le rectangle de sa carte, qui passe sous
    /// le rail (`reach` : largeur à gagner à gauche). À appeler après le
    /// panneau utilisateur et avant le panneau vocal : il se pose entre eux.
    pub(super) fn game_card(&mut self, ui: &mut egui::Ui, reach: f32) -> Option<egui::Rect> {
        let game = self.store.activity.current()?.clone();
        let icon = self.store.activity.icon(&game);
        let t = self.theme.clone();
        let lang = self.lang;
        let in_voice = self.store.voice.me.is_some();
        // Comme le client : le partage ne se propose que là où il a un public.
        let can_share = in_voice && self.share_picker_ready() && self.store.video.share.is_none();
        let elapsed = self.store.now() - game.start_ms;
        let gap = t.spacing.item_x as i8;
        let frame = egui::Frame::new().fill(self.mica_inner(t.colors.bg1)).inner_margin(egui::Margin { left: gap, right: gap, top: if in_voice { 0 } else { gap }, bottom: 0 });
        let (mut share, mut open) = (false, false);
        let panel = egui::Panel::bottom("jeu-detecte").frame(frame).show_separator_line(false).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Side);
            let pad = t.user_bar_margin();
            let side = t.layout.small_avatar;
            let (slot, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), side + pad.sum().y), Sense::hover());
            let card = egui::Rect::from_min_max(egui::pos2(slot.left() - reach, slot.top()), slot.max);
            // Le panneau des salons découpe à son bord : élargi jusque sous le rail.
            let clip = ui.clip_rect();
            let clip = egui::Rect::from_min_max(egui::pos2(clip.left() - reach, clip.top()), clip.max);
            let r = t.r_card();
            let round = if in_voice { egui::CornerRadius::ZERO } else { egui::CornerRadius { nw: r.nw, ne: r.ne, sw: 0, se: 0 } };
            let mut wide = ui.painter().clone();
            wide.set_clip_rect(clip);
            wide.rect(card, round, t.panel_bg(), t.panel_stroke(), egui::StrokeKind::Inside);
            // Toute la carte ouvre les réglages d'activité ; le bouton de partage passe devant.
            let whole = ui.interact(card.intersect(clip), ui.scope_id().with("carte"), Sense::click());
            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(card - pad).layout(Layout::right_to_left(Align::Center)));
            inner.set_clip_rect(clip);
            let ui = &mut inner;
            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
            if can_share {
                let pad_y = ui.spacing().button_padding.y;
                let button = t.panel_button(pad_y);
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(button), Sense::click());
                let hot = resp.hovered() || resp.has_focus();
                if hot {
                    ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
                }
                icons::paint(Icon::Screen, ui.painter(), egui::Rect::from_center_size(rect.center(), Vec2::splat(t.panel_icon())), if hot { t.colors.text_strong } else { t.colors.muted });
                let tip = at::SHARE_GAME.get(lang);
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                super::keys::focus_ring(ui, &t, &resp);
                share = super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), &t, tip).clicked();
            }
            ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                let rect = pixel_square(ui, slot.min, side);
                let drawn = match icon.map(|i| self.images.get(i, side)) {
                    Some(Image::Ready(tex)) => {
                        ui.painter().add(images::textured(rect, t.r_media(), tex));
                        true
                    }
                    _ => false,
                };
                if !drawn {
                    // Pas d'image (programme ajouté à la main, icône absente ou en route) : une manette.
                    ui.painter().rect_filled(rect, t.r_media(), t.colors.bg3);
                    super::user_panel::paint_gamepad(ui.painter(), rect.shrink((side * 0.22).round()), t.colors.text, t.colors.bg3);
                }
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    let text_h = ui.text_style_height(&egui::TextStyle::Body) + ui.text_style_height(&egui::TextStyle::Small);
                    ui.add_space(((side - text_h) / 2.0).max(0.0).floor());
                    ui.add(egui::Label::new(RichText::new(&game.name).family(super::fonts::demi()).strong().color(t.colors.text_strong)).truncate().selectable(false));
                    ui.add(egui::Label::new(RichText::new(since(lang, elapsed)).small().color(t.colors.muted)).truncate().selectable(false));
                });
            });
            let tip = at::CARD_TIP.get(lang);
            let name = game.name.clone();
            whole.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name} — {tip}")));
            open = whole.on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
            // Seul réveil : la minute suivante du temps écoulé.
            ui.ctx().request_repaint_after(next_minute(elapsed));
            card
        });
        if share {
            // Le sélecteur n'a pas (encore) d'entrée pour désigner une fenêtre
            // à l'avance : il s'ouvre sur l'onglet des fenêtres.
            self.open_share_picker(crate::state::share::ShareTab::Windows);
        } else if open {
            self.show_settings(Page::Activity);
        }
        Some(panel.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::activity::{ActivityEvent, RecentGame, RunningGame};
    use crate::state::{Event, Store};
    use crate::ui::tests::{find, frame, offline_boot, settle, Offline};
    use std::time::Duration;

    fn app() -> App {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        store.apply(Event::Ready { me: crate::state::model::User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        store.select_dms();
        let images = crate::images::Images::disabled(ctx.clone());
        App::with_context(ctx, offline_boot(Box::new(Offline), store, images))
    }

    fn running(app: &mut App, games: &[(&str, i64)]) {
        let now = app.store.now();
        let list = games.iter().map(|(name, ago)| RunningGame { app_id: None, name: (*name).into(), start_ms: now - ago }).collect();
        app.store.apply(Event::Activity(ActivityEvent::Running(list)));
    }

    #[test]
    fn elapsed_time_in_words() {
        assert_eq!(since(Lang::Fr, 0), "à l'instant");
        assert_eq!(since(Lang::Fr, 59_999), "à l'instant");
        assert_eq!(since(Lang::Fr, 12 * 60_000 + 5), "depuis 12\u{a0}min");
        assert_eq!(since(Lang::Fr, 125 * 60_000), "depuis 2\u{a0}h\u{a0}05");
        assert_eq!(since(Lang::En, 125 * 60_000), "for 2 h 05 min");
        assert_eq!(since(Lang::Fr, -5), "à l'instant");
        assert_eq!(next_minute(0), Duration::from_secs(60));
        assert_eq!(next_minute(61_000), Duration::from_secs(59));
    }

    /// Encart absent sans jeu, présent avec un jeu, absent quand il est
    /// masqué ou terminé ; le bouton de partage n'existe pas hors d'un appel.
    #[test]
    fn card_follows_the_detected_game() {
        let mut app = app();
        let tip = at::CARD_TIP.get(Lang::Fr);
        assert!(find(&settle(&mut app), tip).is_none(), "pas de jeu : pas d'encart");
        running(&mut app, &[("Astral Drift", 12 * 60_000 + 500)]);
        let out = settle(&mut app);
        let card = find(&out, tip).unwrap_or_else(|| panic!("encart du jeu"));
        let foot = app.place.card.unwrap_or_else(|| panic!("carte du pied"));
        assert!(card.bottom() <= foot.bottom() && card.height() > 20.0, "{card:?} dans {foot:?}");
        assert!(find(&out, at::SHARE_GAME.get(Lang::Fr)).is_none(), "hors appel : pas de bouton de partage");
        // Masqué : plus d'encart (et rien n'est annoncé, voir `state::activity`).
        app.store.activity.recent = vec![RecentGame { app_id: None, name: "Astral Drift".into(), seen_ms: 0, hidden: true }];
        // Les préférences locales font foi à chaque image : le masque y est posé aussi.
        let mut p = app.local.prefs.clone();
        p.recent_games = app.store.activity.recent.clone();
        app.set_local_prefs(p);
        assert!(find(&settle(&mut app), tip).is_none(), "jeu masqué : pas d'encart");
        let mut p = app.local.prefs.clone();
        p.recent_games.clear();
        app.set_local_prefs(p);
        assert!(find(&settle(&mut app), tip).is_some());
        running(&mut app, &[]);
        assert!(find(&settle(&mut app), tip).is_none(), "fin du jeu : l'encart disparaît");
    }

    /// Rien ne change : aucune image n'est redemandée. Avec un jeu, le seul
    /// réveil est la minute suivante du temps écoulé — jamais un redessin
    /// continu.
    #[test]
    fn no_redraw_while_nothing_changes() {
        let mut app = app();
        let out = settle(&mut app);
        assert!(out.viewport_output.values().all(|v| v.repaint_delay == Duration::MAX), "sans jeu : aucun réveil ({:?})", app.ctx.repaint_causes());
        running(&mut app, &[("Astral Drift", 30_000)]);
        let out = settle(&mut app);
        let delay = out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap_or_default();
        assert!(delay > Duration::from_secs(5) && delay <= Duration::from_secs(60), "{delay:?} ({:?})", app.ctx.repaint_causes());
        // Un relevé identique n'arrive même pas jusqu'ici : le sondage ne
        // signale que les changements (`net::games`).
        running(&mut app, &[]);
        let out = settle(&mut app);
        assert!(out.viewport_output.values().all(|v| v.repaint_delay == Duration::MAX), "{:?}", app.ctx.repaint_causes());
    }

    /// Les jeux vus se rangent dans les préférences une fois par partie.
    #[test]
    fn seen_games_are_remembered_once() {
        let mut app = app();
        running(&mut app, &[("Astral Drift", 1_000)]);
        let _ = frame(&mut app, Vec::new());
        assert_eq!(app.local.prefs.recent_games.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["Astral Drift"]);
        let before = app.local.prefs.recent_games.clone();
        for _ in 0..3 {
            let _ = frame(&mut app, Vec::new());
        }
        assert_eq!(app.local.prefs.recent_games, before);
    }
}
