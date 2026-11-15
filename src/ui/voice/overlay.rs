//! Incrustation « qui parle » (plugin, lot E4) : pendant un appel, une petite
//! fenêtre sans bordure, transparente, toujours au premier plan, qui liste
//! les participants (avatar, nom) et entoure celui qui parle.
//!
//! C'est une fenêtre ordinaire du système (un « viewport » différé d'egui) :
//! rien n'est injecté dans un autre processus. Elle ne s'affiche donc pas
//! par-dessus un jeu en plein écran exclusif.
//!
//! - Les clics la traversent et elle ne prend pas le focus (`winit` :
//!   `set_cursor_hittest(false)`, fenêtre créée inactive) : aucun `unsafe`.
//!   En « mode déplacer », elle reçoit la souris : glisser la déplace, son
//!   coin la redimensionne, « Terminé » retient position et taille.
//! - Elle a son propre rythme de dessin : la fenêtre principale lui donne un
//!   modèle ([`Model`]) et ne lui demande une image que quand ce modèle
//!   change (quelqu'un commence ou cesse de parler, entre, sort, réglage).
//!   Elle-même ne demande jamais d'image.
//! - Hors appel ou plugin coupé : pas de fenêtre, pas de modèle.

use super::icons::{self, VIcon};
use super::App;
use crate::i18n::table::e4::t as xt;
use crate::plugins::{self, Id, Value};
use crate::state::model::Avatar;
use eframe::egui::{self, pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind};
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::{Arc, Mutex, PoisonError};

/// La fenêtre de l'incrustation existe : la couleur d'effacement des fenêtres
/// doit alors être transparente (`App::clear_color`).
static OPEN: AtomicBool = AtomicBool::new(false);

/// Couleur d'effacement voulue par l'incrustation : transparente tant qu'elle
/// est ouverte (la fenêtre principale, opaque, peint tout son fond elle-même).
pub(in crate::ui) fn clear_color() -> Option<[f32; 4]> {
    OPEN.load(Relaxed).then_some([0.0; 4])
}

const AVATAR: f32 = 28.0;
const ROW: f32 = 36.0;
const PAD: f32 = 6.0;
const NAME: f32 = 14.0;
/// Côté de la poignée de redimensionnement (mode déplacer).
const GRIP: f32 = 18.0;
const MIN_SIZE: [f32; 2] = [140.0, 60.0];

fn viewport_id() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("ferrocord-incrustation-qui-parle")
}

/// Une personne du salon, telle que l'incrustation la dessine.
#[derive(Clone, Debug, PartialEq)]
struct Row {
    name: String,
    speaking: bool,
    /// Micro coupé ou sourdine (par la personne ou le serveur).
    muted: bool,
    deaf: bool,
    /// Avatar déjà chargé ; sinon la pastille aux initiales.
    texture: Option<egui::TextureId>,
    initials: String,
    fill: Color32,
    ink: Color32,
}

/// Aspect, repris du thème à chaque changement.
#[derive(Clone, Debug, PartialEq)]
struct Look {
    opacity: f32,
    move_mode: bool,
    back: Color32,
    text: Color32,
    muted: Color32,
    speaking: Color32,
    danger: Color32,
    accent: Color32,
    on_accent: Color32,
    avatar_radius: CornerRadius,
    row_radius: CornerRadius,
    done: &'static str,
    hint: &'static str,
}

/// Ce que la fenêtre principale donne à l'incrustation, et ce qui en revient.
struct Model {
    rows: Vec<Row>,
    look: Look,
    /// Mode déplacer : dernière position (coin extérieur) et taille (intérieure) vues.
    placed: Option<(egui::Pos2, egui::Vec2)>,
    /// « Terminé » cliqué.
    done: bool,
    /// Images dessinées par l'incrustation (mesure et tests).
    frames: u64,
}

fn lock(m: &Mutex<Model>) -> std::sync::MutexGuard<'_, Model> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Ce que l'interface retient de l'incrustation.
#[derive(Default)]
pub(super) struct OverlayUi {
    model: Option<Arc<Mutex<Model>>>,
    /// Images demandées à l'incrustation (une par changement du modèle).
    asked: u64,
}

impl Drop for OverlayUi {
    fn drop(&mut self) {
        if self.model.is_some() {
            OPEN.store(false, Relaxed);
        }
    }
}

/// Placement retenu (points) : position du coin extérieur, taille intérieure.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placement {
    pos: [f32; 2],
    size: [f32; 2],
}

fn with_alpha(c: Color32, k: f32) -> Color32 {
    c.gamma_multiply(k.clamp(0.0, 1.0))
}

/// Dessin de l'incrustation, dans sa fenêtre.
fn draw(ui: &mut egui::Ui, model: &Mutex<Model>) {
    let ctx = ui.ctx().clone();
    let full = ui.max_rect();
    let (rows, look) = {
        let mut m = lock(model);
        m.frames += 1;
        if m.look.move_mode {
            let (outer, inner) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect));
            if let (Some(o), Some(i)) = (outer, inner) {
                m.placed = Some((o.min, i.size()));
            }
        }
        (m.rows.clone(), m.look.clone())
    };
    let k = look.opacity;
    let p = ui.painter().clone();
    if look.move_mode {
        // La fenêtre se voit et se saisit : fond et contour.
        p.rect_filled(full, look.row_radius, with_alpha(look.back, 0.85));
        p.rect_stroke(full.shrink(1.0), look.row_radius, Stroke::new(2.0, look.accent), StrokeKind::Inside);
        let drag = ui.interact(full, egui::Id::unique("incrustation-deplacer"), Sense::drag());
        if drag.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        drag.on_hover_cursor(egui::CursorIcon::Grab);
    }
    let mut y = full.top() + PAD;
    for row in &rows {
        let rect = Rect::from_min_size(pos2(full.left() + PAD, y), vec2(full.width() - 2.0 * PAD, ROW - 4.0));
        if rect.bottom() > full.bottom() {
            break;
        }
        y += ROW;
        let dim = if row.speaking { 1.0 } else { 0.7 };
        p.rect_filled(rect, look.row_radius, with_alpha(look.back, k * if row.speaking { 0.82 } else { 0.55 }));
        let slot = Rect::from_min_size(pos2(rect.left() + 4.0, rect.center().y - AVATAR / 2.0), egui::Vec2::splat(AVATAR));
        match row.texture {
            Some(tex) => {
                let tint = with_alpha(Color32::WHITE, k * dim);
                p.add(egui::epaint::RectShape::filled(slot, look.avatar_radius, tint).with_texture(tex, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0))));
            }
            None => {
                p.rect_filled(slot, look.avatar_radius, with_alpha(row.fill, k * dim));
                p.text(slot.center(), Align2::CENTER_CENTER, &row.initials, FontId::proportional(AVATAR * 0.42), with_alpha(row.ink, k * dim));
            }
        }
        if row.speaking {
            // Halo : un contour net, doublé d'un contour large et doux.
            p.rect_stroke(slot.expand(1.0), look.avatar_radius, Stroke::new(2.0, with_alpha(look.speaking, k)), StrokeKind::Outside);
            p.rect_stroke(slot.expand(3.0), look.avatar_radius, Stroke::new(3.0, with_alpha(look.speaking, k * 0.35)), StrokeKind::Outside);
        }
        let mut right = rect.right() - 6.0;
        let mark = if row.deaf { Some(VIcon::HeadphonesOff) } else if row.muted { Some(VIcon::MicOff) } else { None };
        if let Some(icon) = mark {
            let r = Rect::from_center_size(pos2(right - 7.0, rect.center().y), egui::Vec2::splat(14.0));
            icons::paint(icon, &p, r, with_alpha(look.danger, k));
            right = r.left() - 4.0;
        }
        let left = slot.right() + 8.0;
        let ink = with_alpha(if row.speaking { look.text } else { look.muted }, k);
        let mut job = egui::text::LayoutJob::simple_singleline(row.name.clone(), FontId::proportional(NAME), ink);
        job.wrap = egui::text::TextWrapping::truncate_at_width((right - left).max(1.0));
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        p.galley(pos2(left, rect.center().y - galley.size().y / 2.0), galley, ink);
    }
    if look.move_mode {
        // Poignée de redimensionnement, puis « Terminé ».
        let grip = Rect::from_min_max(full.max - egui::Vec2::splat(GRIP), full.max);
        let g = ui.interact(grip, egui::Id::unique("incrustation-taille"), Sense::drag());
        if g.drag_started() {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(egui::ResizeDirection::SouthEast));
        }
        g.on_hover_cursor(egui::CursorIcon::ResizeNwSe);
        for i in 1..=3 {
            let d = i as f32 * 4.5;
            p.line_segment([pos2(full.right() - d, full.bottom() - 2.0), pos2(full.right() - 2.0, full.bottom() - d)], Stroke::new(1.5, look.accent));
        }
        let text = p.layout_no_wrap(look.done.to_owned(), FontId::proportional(NAME), look.on_accent);
        let button = Rect::from_min_size(pos2(full.left() + PAD, full.bottom() - PAD - 24.0), vec2(text.size().x + 20.0, 24.0));
        let b = ui.interact(button, egui::Id::unique("incrustation-termine"), Sense::click());
        p.rect_filled(button, look.row_radius, look.accent);
        p.galley(button.center() - text.size() / 2.0, text, look.on_accent);
        p.text(pos2(button.right() + 8.0, button.center().y), Align2::LEFT_CENTER, look.hint, FontId::proportional(NAME - 2.0), look.muted);
        b.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, look.done));
        if b.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            lock(model).done = true;
            // La fenêtre principale enregistre et quitte le mode déplacer.
            ctx.request_repaint_of(egui::ViewportId::ROOT);
        }
    }
}

impl App {
    /// L'incrustation doit exister : plugin activé et dans un salon vocal.
    fn voice_overlay_wanted(&self) -> bool {
        cfg!(not(target_os = "android")) && plugins::on(Id::SpeakerOverlay) && self.store.voice.me.is_some()
    }

    /// La fenêtre de l'incrustation est ouverte.
    pub fn voice_overlay_open(&self) -> bool {
        self.voice.overlay.model.is_some()
    }

    /// Images demandées à l'incrustation depuis son ouverture, et images
    /// qu'elle a dessinées (mesure et tests).
    #[doc(hidden)]
    pub fn voice_overlay_counts(&self) -> (u64, u64) {
        (self.voice.overlay.asked, self.voice.overlay.model.as_ref().map_or(0, |m| lock(m).frames))
    }

    fn voice_overlay_rows(&self, channel: u64) -> Vec<Row> {
        let t = &self.theme;
        let v = &self.store.voice;
        v.members(channel)
            .map(|s| {
                let (name, avatar): (String, Avatar) = self.voice_who(s);
                let texture = avatar.image.and_then(|image| match self.images.get(image, AVATAR) {
                    crate::images::Image::Ready(tex) => Some(tex),
                    _ => None,
                });
                let (fill, ink) = t.default_avatar(avatar.default);
                Row { initials: crate::ui::util::avatar_initials(&name), name, speaking: v.speaking.contains(&s.user_id), muted: s.mute || s.self_mute, deaf: s.deaf || s.self_deaf, texture, fill, ink }
            })
            .collect()
    }

    fn voice_overlay_look(&self) -> Look {
        let t = &self.theme;
        let c = &t.colors;
        Look {
            opacity: plugins::number(Id::SpeakerOverlay, plugins::OVERLAY_OPACITY) as f32 / 100.0,
            move_mode: plugins::flag(Id::SpeakerOverlay, plugins::OVERLAY_MOVE),
            back: c.bg0,
            text: c.text_strong,
            muted: c.text,
            speaking: c.online,
            danger: c.danger,
            accent: c.accent,
            on_accent: c.on_accent,
            avatar_radius: t.r_avatar(AVATAR),
            row_radius: t.r_control(),
            done: xt::OVERLAY_DONE.get(self.lang),
            hint: xt::OVERLAY_HINT.get(self.lang),
        }
    }

    fn voice_overlay_placement(&self) -> Placement {
        let n = |key| plugins::number(Id::SpeakerOverlay, key) as f32;
        Placement { pos: [n(plugins::OVERLAY_X), n(plugins::OVERLAY_Y)], size: [n(plugins::OVERLAY_W).max(MIN_SIZE[0]), n(plugins::OVERLAY_H).max(MIN_SIZE[1])] }
    }

    /// Une fois par image de la fenêtre principale : ouvre, tient à jour ou
    /// ferme l'incrustation. Plugin coupé ou hors appel : une comparaison.
    pub(in crate::ui) fn voice_overlay_tick(&mut self, ctx: &egui::Context) {
        if !self.voice_overlay_wanted() {
            if self.voice.overlay.model.take().is_some() {
                // Plus déclarée : egui ferme la fenêtre à la fin de cette image.
                OPEN.store(false, Relaxed);
                self.voice.overlay.asked = 0;
            }
            return;
        }
        let Some(my) = self.store.voice.me else { return };
        let rows = self.voice_overlay_rows(my.channel_id);
        let look = self.voice_overlay_look();
        let place = self.voice_overlay_placement();
        let opened = self.voice.overlay.model.is_none();
        let model = self.voice.overlay.model.get_or_insert_with(|| Arc::new(Mutex::new(Model { rows: Vec::new(), look: look.clone(), placed: None, done: false, frames: 0 }))).clone();
        OPEN.store(true, Relaxed);
        // Le verrou est rendu avant de déclarer la fenêtre (egui peut la dessiner tout de suite).
        let (changed, done, placed) = {
            let mut m = lock(&model);
            let changed = opened || m.rows != rows || m.look != look;
            if changed {
                m.rows = rows;
                m.look = look.clone();
            }
            let done = std::mem::take(&mut m.done);
            // Fin du mode déplacer (« Terminé », ou réglage coupé) : le placement vu est retenu.
            let placed = if done || !look.move_mode { m.placed.take() } else { None };
            (changed, done, placed)
        };
        if done || placed.is_some() {
            self.voice_overlay_keep(done, placed);
        }
        let title = xt::OVERLAY_TITLE.get(self.lang);
        let builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_taskbar(false)
            .with_active(false)
            .with_resizable(look.move_mode)
            .with_mouse_passthrough(!look.move_mode)
            .with_min_inner_size(MIN_SIZE)
            .with_position(place.pos)
            .with_inner_size(place.size);
        let shared = model.clone();
        ctx.show_viewport_deferred(viewport_id(), builder, move |ui, _class| draw(ui, &shared));
        if changed {
            self.voice.overlay.asked += 1;
            ctx.request_repaint_of(viewport_id());
        }
    }

    /// Retient le placement vu pendant le mode déplacer, et quitte ce mode si
    /// « Terminé » a été cliqué.
    fn voice_overlay_keep(&mut self, done: bool, placed: Option<(egui::Pos2, egui::Vec2)>) {
        let mut p = self.local.prefs.clone();
        if let Some((pos, size)) = placed {
            for (key, v) in [(plugins::OVERLAY_X, pos.x), (plugins::OVERLAY_Y, pos.y), (plugins::OVERLAY_W, size.x), (plugins::OVERLAY_H, size.y)] {
                p.plugins.put(Id::SpeakerOverlay, key, Value::Int(v.round() as i64));
            }
        }
        if done {
            p.plugins.put(Id::SpeakerOverlay, plugins::OVERLAY_MOVE, Value::Bool(false));
        }
        self.set_local_prefs(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::voice::{VoiceCmd, VoiceEvent, VoiceState};
    use crate::state::voice_link::{NoVoice, VoiceLink, VoiceWant};
    use crate::state::{Event, Store};
    use crate::ui::tests::offline_boot;

    const ME: u64 = 1;
    const BOB: u64 = 2;
    const EVE: u64 = 3;
    const G: u64 = 10;
    const VOC: u64 = 20;

    struct Offline;
    impl crate::ui::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            false
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    /// Orchestrateur muet : l'état vocal du magasin n'est pas touché.
    struct Quiet(NoVoice);
    impl VoiceLink for Quiet {
        fn sync(&mut self, w: &VoiceWant<'_>) {
            self.0.sync(w);
        }
        fn command(&mut self, c: &VoiceCmd) {
            self.0.command(c);
        }
        fn request_codes(&mut self) {}
        fn poll(&mut self) -> Vec<VoiceEvent> {
            Vec::new()
        }
    }

    fn state(user_id: u64) -> VoiceState {
        VoiceState { user_id, guild_id: Some(G), channel_id: VOC, name: Some(format!("personne {user_id}")), ..Default::default() }
    }

    fn in_call() -> App {
        let mut s = Store::new();
        s.apply(Event::Ready { me: crate::state::model::User { id: ME, name: "moi".into(), avatar: Avatar::default_for(ME) }, guilds: vec![], dms: vec![] });
        s.apply(Event::Voice(VoiceEvent::States(vec![state(BOB)])));
        s.voice_join_call(VOC);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(state(ME)), session_id: Some("sess".into()) }));
        s.apply(Event::Voice(VoiceEvent::Connected));
        s.take_commands();
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Offline), s, images));
        app.attach_voice(Box::new(Quiet(NoVoice)));
        app
    }

    fn frame(app: &mut App) {
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))), ..Default::default() };
        let ctx = app.ctx.clone();
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
    }

    /// Plugin coupé (état par défaut) : ni fenêtre, ni modèle, même en appel.
    #[test]
    fn no_window_while_the_plugin_is_off() {
        let mut app = in_call();
        assert!(!plugins::on(Id::SpeakerOverlay));
        frame(&mut app);
        frame(&mut app);
        assert!(!app.voice_overlay_open());
        assert_eq!(app.voice_overlay_counts(), (0, 0));
    }

    /// Une image n'est demandée à l'incrustation que quand quelqu'un commence
    /// ou cesse de parler, entre ou sort ; elle se ferme hors appel.
    #[test]
    fn redraws_only_when_speakers_or_people_change() {
        let mut app = in_call();
        app.set_plugin(Id::SpeakerOverlay, true);
        frame(&mut app);
        assert!(app.voice_overlay_open());
        assert_eq!(app.voice_overlay_counts().0, 1, "à l'ouverture");
        for _ in 0..5 {
            frame(&mut app);
        }
        assert_eq!(app.voice_overlay_counts().0, 1, "rien ne bouge : aucune image demandée");

        let rows = |app: &App| lock(app.voice.overlay.model.as_ref().unwrap()).rows.clone();
        assert_eq!(rows(&app).iter().map(|r| (r.name.as_str(), r.speaking)).collect::<Vec<_>>(), [("personne 2", false), ("personne 1", false)]);
        app.store.apply(Event::Voice(VoiceEvent::Speaking { user_id: BOB, speaking: true }));
        frame(&mut app);
        frame(&mut app);
        assert_eq!(app.voice_overlay_counts().0, 2, "Bob commence à parler");
        assert!(rows(&app).iter().any(|r| r.name == "personne 2" && r.speaking));
        app.store.apply(Event::Voice(VoiceEvent::Speaking { user_id: BOB, speaking: false }));
        frame(&mut app);
        assert_eq!(app.voice_overlay_counts().0, 3, "Bob se tait");
        app.store.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: EVE, state: Some(state(EVE)), session_id: None }));
        frame(&mut app);
        frame(&mut app);
        assert_eq!(app.voice_overlay_counts().0, 4, "Ève entre");
        assert_eq!(rows(&app).len(), 3);
        app.store.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: EVE, state: None, session_id: None }));
        frame(&mut app);
        assert_eq!(app.voice_overlay_counts().0, 5, "Ève sort");

        // Hors appel : fermée, modèle rendu.
        app.store.voice_leave();
        app.store.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: None, session_id: None }));
        frame(&mut app);
        assert!(!app.voice_overlay_open());
        assert!(clear_color().is_none() || app.store.voice.me.is_some());
    }

    /// « Terminé » retient la position et la taille vues, et quitte le mode déplacer.
    #[test]
    fn move_mode_keeps_the_placement() {
        let mut app = in_call();
        let mut p = app.local_prefs();
        p.plugins.set(Id::SpeakerOverlay, true);
        p.plugins.put(Id::SpeakerOverlay, plugins::OVERLAY_MOVE, Value::Bool(true));
        p.plugins.put(Id::SpeakerOverlay, plugins::OVERLAY_OPACITY, Value::Int(60));
        app.set_local_prefs(p);
        frame(&mut app);
        {
            let mut m = lock(app.voice.overlay.model.as_ref().unwrap());
            assert!(m.look.move_mode && (m.look.opacity - 0.6).abs() < 1e-6);
            m.placed = Some((pos2(300.4, 80.0), vec2(260.0, 410.6)));
            m.done = true;
        }
        frame(&mut app);
        let prefs = app.local_prefs().plugins;
        let n = |key| prefs.number(Id::SpeakerOverlay, key);
        assert_eq!((n(plugins::OVERLAY_X), n(plugins::OVERLAY_Y), n(plugins::OVERLAY_W), n(plugins::OVERLAY_H)), (300, 80, 260, 411));
        assert!(!prefs.flag(Id::SpeakerOverlay, plugins::OVERLAY_MOVE));
        assert_eq!(app.voice_overlay_placement(), Placement { pos: [300.0, 80.0], size: [260.0, 411.0] });
        frame(&mut app);
        assert!(!lock(app.voice.overlay.model.as_ref().unwrap()).look.move_mode);
    }
}
