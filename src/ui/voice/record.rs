//! Enregistrement local d'un appel (plugin, lot E4), côté interface.
//!
//! - Jamais de démarrage sans un clic : le bouton du panneau vocal ouvre une
//!   fenêtre qui rappelle, à chaque fois, que Discord ne prévient pas les
//!   autres et que la loi peut exiger leur accord ; l'enregistrement ne part
//!   qu'au clic sur « Démarrer l'enregistrement ».
//! - Le même avertissement s'affiche quand le plugin est activé.
//! - Pendant l'enregistrement : bandeau rouge dans le panneau vocal et
//!   pastille rouge dans la barre de titre, avec la durée ; un clic arrête.
//! - Quitter le salon, changer de salon ou couper le plugin arrête
//!   l'enregistrement.
//!
//! Plugin coupé : aucun bouton, aucune fenêtre, rien n'est demandé à
//! l'orchestrateur ([`App::voice_rec_request`] rend la main).

use super::App;
use crate::i18n::table::e4::{f as xf, t as xt};
use crate::plugins::{self, Id, Value};
use crate::state::call_rec::{clock, CallRecCmd, CallRecFail, CallRecUp};
use crate::state::voice::VoiceConnState;
use crate::theme::Tokens;
use crate::ui::widgets::{self, Kind as Button};
use eframe::egui::{self, pos2, vec2, Align2, Rect, RichText, Sense};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Largeur du contenu des fenêtres d'avertissement.
const MODAL_WIDTH: f32 = 440.0;

/// Fenêtre ouverte.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    /// Le plugin vient d'être activé.
    Enabled,
    /// Avant chaque enregistrement.
    Start,
}

struct Active {
    channel: u64,
    since: Instant,
    /// Arrêt demandé : on attend la fermeture des fichiers.
    stopping: bool,
    /// Participants dont les noms ont été donnés au fil d'écriture.
    people: Vec<u64>,
}

/// Ce que l'interface retient de l'enregistrement.
#[derive(Default)]
pub(super) struct RecUi {
    ask: Option<Ask>,
    active: Option<Active>,
    /// Boîte « Choisir le dossier » ouverte.
    picking: Option<mpsc::Receiver<Option<PathBuf>>>,
}

#[cfg(windows)]
fn pick_folder(ctx: egui::Context, title: String, start: String) -> Option<mpsc::Receiver<Option<PathBuf>>> {
    let (tx, rx) = mpsc::channel();
    // La boîte de dialogue bloque : dans son propre fil.
    std::thread::Builder::new()
        .name("ferrocord-dossier".into())
        .spawn(move || {
            let mut dialog = rfd::FileDialog::new().set_title(title);
            if !start.is_empty() {
                dialog = dialog.set_directory(start);
            }
            let _ = tx.send(dialog.pick_folder());
            ctx.request_repaint();
        })
        .ok()?;
    Some(rx)
}

#[cfg(not(windows))]
fn pick_folder(_: egui::Context, _: String, _: String) -> Option<mpsc::Receiver<Option<PathBuf>>> {
    None
}

/// Les trois lignes de l'avertissement.
fn warning(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang) {
    ui.label(RichText::new(xt::REC_WARN_1.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.danger));
    ui.add_space(t.spacing.item_y);
    ui.label(RichText::new(xt::REC_WARN_2.get(lang)).color(t.colors.text));
    ui.add_space(t.spacing.item_y);
    ui.label(RichText::new(xt::REC_WARN_3.get(lang)).small().color(t.colors.muted));
}

/// Pastille rouge « ● ENR 1:23 » dans `rect` ; rend sa réponse (un clic arrête).
fn pill(ui: &mut egui::Ui, t: &Tokens, rect: Rect, text: &str, tip: &str, id: &str) -> egui::Response {
    let resp = ui.interact(rect, egui::Id::unique(id), Sense::click());
    let p = ui.painter();
    let fill = if resp.hovered() || resp.has_focus() { t.colors.danger } else { t.danger_fill() };
    p.rect_filled(rect, t.r_control(), fill);
    let ink = t.colors.on_accent;
    let dot = (rect.height() * 0.22).clamp(2.5, 5.0);
    let pad = t.spacing.item_x;
    p.circle_filled(pos2(rect.left() + pad + dot, rect.center().y), dot, ink);
    p.text(pos2(rect.left() + 2.0 * pad + 2.0 * dot, rect.center().y), Align2::LEFT_CENTER, text, crate::ui::fonts::demi_font(t.typography.small), ink);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Largeur d'une pastille pour ce texte.
fn pill_width(ui: &egui::Ui, t: &Tokens, text: &str, height: f32) -> f32 {
    let g = ui.painter().layout_no_wrap(text.to_owned(), crate::ui::fonts::demi_font(t.typography.small), t.colors.on_accent);
    let dot = (height * 0.22).clamp(2.5, 5.0);
    g.size().x + 3.0 * t.spacing.item_x + 2.0 * dot
}

impl App {
    /// Le bouton d'enregistrement a sa place : plugin activé, dans un salon vocal.
    pub(in crate::ui) fn voice_rec_shown(&self) -> bool {
        plugins::on(Id::CallRecorder) && self.store.voice.me.is_some()
    }

    /// Un enregistrement est en cours (jusqu'à la fermeture des fichiers).
    pub fn voice_recording(&self) -> bool {
        self.voice.rec.active.is_some()
    }

    /// Durée de l'enregistrement en cours.
    fn voice_rec_clock(&self) -> Option<String> {
        self.voice.rec.active.as_ref().map(|a| clock(a.since.elapsed().as_secs()))
    }

    /// Le plugin vient d'être activé à la main : l'avertissement.
    pub(in crate::ui) fn voice_rec_enabled(&mut self) {
        self.voice.rec.ask = Some(Ask::Enabled);
    }

    /// Clic sur le bouton du panneau : arrête, ou ouvre la fenêtre de
    /// démarrage. Plugin coupé : rien.
    pub fn voice_rec_request(&mut self) {
        if !plugins::on(Id::CallRecorder) {
            return;
        }
        if self.voice.rec.active.is_some() {
            self.voice_rec_stop();
        } else if self.store.voice.me.is_some() {
            self.voice.rec.ask = Some(Ask::Start);
        }
    }

    /// Dossier des enregistrements choisi (vide : aucun).
    fn voice_rec_folder(&self) -> String {
        self.local.prefs.plugins.text(Id::CallRecorder, plugins::REC_FOLDER).trim().to_string()
    }

    /// Démarre (après la fenêtre d'avertissement) ; faux si rien n'a été lancé.
    pub fn voice_rec_start(&mut self) -> bool {
        if !plugins::on(Id::CallRecorder) || self.voice.rec.active.is_some() {
            return false;
        }
        let (Some(my), Some(me)) = (self.store.voice.me, self.store.me.as_ref().map(|m| m.id)) else { return false };
        if !matches!(self.store.voice.conn, VoiceConnState::Connected { .. }) {
            self.notify(xt::REC_NOT_CONNECTED);
            return false;
        }
        let folder = self.voice_rec_folder();
        if folder.is_empty() {
            return false;
        }
        let channel = self.store.voice.channel(my.channel_id).map(|c| c.name.clone()).unwrap_or_default();
        let stamp = chrono::Local::now().format("%Y-%m-%d %Hh%Mm%S");
        let base = if channel.is_empty() { format!("Ferrocord - {stamp}") } else { format!("Ferrocord - {channel} - {stamp}") };
        let (people, names) = self.voice_rec_names(my.channel_id);
        let tracks = self.local.prefs.plugins.flag(Id::CallRecorder, plugins::REC_TRACKS);
        self.voice.link.call_record(CallRecCmd::Start { dir: PathBuf::from(folder), base, tracks, me, names });
        self.voice.rec.active = Some(Active { channel: my.channel_id, since: Instant::now(), stopping: false, people });
        true
    }

    /// Participants du salon et leurs noms, pour les pistes séparées.
    fn voice_rec_names(&self, channel: u64) -> (Vec<u64>, Vec<(u64, String)>) {
        let names: Vec<(u64, String)> = self.store.voice.members(channel).map(|s| (s.user_id, self.voice_who(s).0)).collect();
        (names.iter().map(|(u, _)| *u).collect(), names)
    }

    pub fn voice_rec_stop(&mut self) {
        if let Some(a) = self.voice.rec.active.as_mut()
            && !a.stopping
        {
            a.stopping = true;
            self.voice.link.call_record(CallRecCmd::Stop);
        }
    }

    /// Une fois par image : dossier choisi, issue d'un enregistrement, arrêt
    /// quand on quitte le salon, fenêtres. Plugin coupé et rien en cours :
    /// deux comparaisons.
    pub(in crate::ui) fn voice_rec_tick(&mut self, ctx: &egui::Context) {
        let on = plugins::on(Id::CallRecorder);
        if !on && self.voice.rec.active.is_none() && self.voice.rec.ask.is_none() && self.voice.rec.picking.is_none() {
            return;
        }
        if let Some(rx) = &self.voice.rec.picking
            && let Ok(chosen) = rx.try_recv()
        {
            self.voice.rec.picking = None;
            if let Some(dir) = chosen {
                let mut p = self.local.prefs.clone();
                p.plugins.put(Id::CallRecorder, plugins::REC_FOLDER, Value::Str(dir.to_string_lossy().into_owned()));
                self.set_local_prefs(p);
            }
        }
        if self.voice.rec.active.is_some() {
            for up in self.voice.link.poll_call_record() {
                self.voice.rec.active = None;
                match up {
                    CallRecUp::Done { file, .. } => {
                        let text = xf::rec_saved(self.lang, file.display());
                        self.notify(text);
                    }
                    CallRecUp::Failed(CallRecFail::Create) => self.notify(xt::REC_FAILED_OPEN),
                    CallRecUp::Failed(CallRecFail::Write) => self.notify(xt::REC_FAILED_WRITE),
                    CallRecUp::Failed(CallRecFail::NoCall) => self.notify(xt::REC_NOT_CONNECTED),
                }
            }
        }
        let here = self.store.voice.me.map(|m| m.channel_id);
        if let Some(a) = &self.voice.rec.active {
            if !on || here != Some(a.channel) {
                // Salon quitté ou changé, plugin coupé : l'enregistrement s'arrête seul.
                self.voice_rec_stop();
            } else if !a.stopping {
                let (people, names) = self.voice_rec_names(a.channel);
                if people != a.people {
                    self.voice.link.call_record(CallRecCmd::Names(names));
                    if let Some(a) = self.voice.rec.active.as_mut() {
                        a.people = people;
                    }
                }
                // La durée affichée avance : une image par seconde, tant que ça enregistre.
                ctx.request_repaint_after(Duration::from_secs(1));
            }
        }
        if !on || (here.is_none() && self.voice.rec.ask == Some(Ask::Start)) {
            self.voice.rec.ask = None;
        }
        self.voice_rec_modals(ctx);
    }

    fn voice_rec_modals(&mut self, ctx: &egui::Context) {
        let Some(ask) = self.voice.rec.ask else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let close_tip = crate::i18n::t::CLOSE_TIP.get(lang);
        match ask {
            Ask::Enabled => {
                let (mut ok, mut off) = (false, false);
                let out = widgets::modal(ctx, &t, egui::Id::unique("enregistrement-avertissement"), MODAL_WIDTH, xt::REC_WARN_TITLE.get(lang), close_tip, |ui| warning(ui, &t, lang), |ui| {
                    ok = widgets::button(ui, &t, Button::Primary, xt::REC_UNDERSTOOD.get(lang)).clicked();
                    off = widgets::button(ui, &t, Button::Link, xt::REC_TURN_OFF.get(lang)).clicked();
                });
                if off {
                    self.voice.rec.ask = None;
                    self.set_plugin(Id::CallRecorder, false);
                } else if ok || out.closed {
                    self.voice.rec.ask = None;
                }
            }
            Ask::Start => {
                let folder = self.voice_rec_folder();
                let connected = matches!(self.store.voice.conn, VoiceConnState::Connected { .. });
                let mut tracks = self.local.prefs.plugins.flag(Id::CallRecorder, plugins::REC_TRACKS);
                let was_tracks = tracks;
                let picking = self.voice.rec.picking.is_some();
                let (mut start, mut cancel, mut pick) = (false, false, false);
                let out = widgets::modal(
                    ctx,
                    &t,
                    egui::Id::unique("enregistrement-demarrer"),
                    MODAL_WIDTH,
                    xt::REC_START_TITLE.get(lang),
                    close_tip,
                    |ui| {
                        warning(ui, &t, lang);
                        ui.add_space(t.spacing.category_gap);
                        widgets::field_label(ui, &t, xt::REC_FOLDER.get(lang));
                        let shown = if folder.is_empty() { xt::REC_NO_FOLDER.get(lang).to_string() } else { folder.clone() };
                        ui.add(egui::Label::new(RichText::new(shown).color(if folder.is_empty() { t.colors.danger } else { t.colors.text })).wrap());
                        ui.add_space(t.spacing.item_y);
                        pick = ui.add_enabled_ui(!picking, |ui| widgets::button(ui, &t, Button::Secondary, xt::REC_PICK.get(lang))).inner.clicked();
                        ui.add_space(t.spacing.item_x);
                        ui.checkbox(&mut tracks, xt::REC_TRACKS.get(lang)).on_hover_text(xt::REC_TRACKS_TIP.get(lang));
                        if !connected {
                            ui.add_space(t.spacing.item_y);
                            ui.label(RichText::new(xt::REC_NOT_CONNECTED.get(lang)).small().color(t.colors.muted));
                        }
                    },
                    |ui| {
                        start = ui.add_enabled_ui(connected && !folder.is_empty(), |ui| widgets::button(ui, &t, Button::Danger, xt::REC_START.get(lang))).inner.clicked();
                        cancel = widgets::button(ui, &t, Button::Link, crate::i18n::t::CANCEL.get(lang)).clicked();
                    },
                );
                if tracks != was_tracks {
                    let mut p = self.local.prefs.clone();
                    p.plugins.put(Id::CallRecorder, plugins::REC_TRACKS, Value::Bool(tracks));
                    self.set_local_prefs(p);
                }
                if pick {
                    self.voice.rec.picking = pick_folder(ctx.clone(), xt::REC_PICK_TITLE.get(lang).to_string(), folder);
                }
                if start {
                    self.voice.rec.ask = None;
                    self.voice_rec_start();
                } else if cancel || out.closed {
                    self.voice.rec.ask = None;
                }
            }
        }
    }

    /// Bandeau rouge du panneau vocal pendant l'enregistrement : durée, arrêt d'un clic.
    pub(super) fn voice_rec_banner(&mut self, ui: &mut egui::Ui) {
        let Some(time) = self.voice_rec_clock() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.spacing.control_height), Sense::hover());
        let text = format!("{}  ·  {}", xf::rec_time(lang, &time), xt::REC_STOP.get(lang));
        if pill(ui, &t, rect, &text, &xf::rec_tip(lang, &time), "enregistrement-bandeau").clicked() {
            self.voice_rec_stop();
        }
        ui.add_space(t.spacing.item_y);
    }

    /// Pastille rouge de la barre de titre, à gauche : rend sa zone (elle
    /// reçoit les clics : un trou dans la zone de déplacement de la fenêtre)
    /// et vrai si elle est cliquée (arrêter).
    pub(in crate::ui) fn voice_rec_title_pill(&self, ui: &mut egui::Ui, bar: Rect) -> Option<(Rect, bool)> {
        let time = self.voice_rec_clock()?;
        let t = &self.theme;
        let lang = self.lang;
        let text = xf::rec_time(lang, &time);
        let h = (bar.height() - 2.0 * t.spacing.item_y).max(12.0);
        let w = pill_width(ui, t, &text, h);
        let rect = Rect::from_min_size(pos2(bar.left() + t.spacing.control_x, bar.center().y - h / 2.0), vec2(w, h));
        let clicked = pill(ui, t, rect, &text, &xf::rec_tip(lang, &time), "enregistrement-titre").clicked();
        Some((rect, clicked))
    }

    /// Aperçu hors ligne : fenêtre de démarrage ouverte.
    #[doc(hidden)]
    pub fn voice_rec_preview_ask(&mut self) {
        self.voice.rec.ask = Some(Ask::Start);
    }

    /// Aperçu hors ligne : un enregistrement « en cours » depuis `seconds`
    /// (rien n'est écrit : aucun orchestrateur n'est branché).
    #[doc(hidden)]
    pub fn voice_rec_preview_active(&mut self, seconds: u64) {
        if let Some(my) = self.store.voice.me {
            let since = Instant::now().checked_sub(Duration::from_secs(seconds)).unwrap_or_else(Instant::now);
            self.voice.rec.active = Some(Active { channel: my.channel_id, since, stopping: false, people: self.voice_rec_names(my.channel_id).0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::voice::{VoiceCmd, VoiceEvent, VoiceState};
    use crate::state::voice_link::{VoiceLink, VoiceWant};
    use crate::state::{Event, Store};
    use crate::ui::tests::offline_boot;
    use std::sync::{Arc, Mutex};

    const ME: u64 = 1;
    const BOB: u64 = 2;
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

    /// Orchestrateur factice : retient les demandes, rend les issues qu'on lui donne.
    #[derive(Clone, Default)]
    struct Fake {
        asked: Arc<Mutex<Vec<CallRecCmd>>>,
        ups: Arc<Mutex<Vec<CallRecUp>>>,
    }

    impl VoiceLink for Fake {
        fn sync(&mut self, _: &VoiceWant<'_>) {}
        fn command(&mut self, _: &VoiceCmd) {}
        fn request_codes(&mut self) {}
        fn poll(&mut self) -> Vec<VoiceEvent> {
            Vec::new()
        }
        fn call_record(&mut self, c: CallRecCmd) {
            self.asked.lock().unwrap().push(c);
        }
        fn poll_call_record(&mut self) -> Vec<CallRecUp> {
            std::mem::take(&mut self.ups.lock().unwrap())
        }
    }

    /// Moi et Bob dans un salon vocal, connexion établie.
    fn in_call() -> (App, Fake) {
        let mut s = Store::new();
        s.apply(Event::Ready { me: crate::state::model::User { id: ME, name: "moi".into(), avatar: crate::state::model::Avatar::default_for(ME) }, guilds: vec![], dms: vec![] });
        let st = |user_id| VoiceState { user_id, guild_id: Some(G), channel_id: VOC, name: Some(format!("personne {user_id}")), ..Default::default() };
        s.apply(Event::Voice(VoiceEvent::States(vec![st(BOB)])));
        s.voice_join_call(VOC);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(st(ME)), session_id: Some("sess".into()) }));
        s.apply(Event::Voice(VoiceEvent::Connected));
        s.take_commands();
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Offline), s, images));
        let fake = Fake::default();
        app.attach_voice(Box::new(fake.clone()));
        (app, fake)
    }

    fn frame(app: &mut App) {
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(1280.0, 800.0))), ..Default::default() };
        let ctx = app.ctx.clone();
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
    }

    fn set_folder(app: &mut App, dir: &str) {
        let mut p = app.local_prefs();
        p.plugins.put(Id::CallRecorder, plugins::REC_FOLDER, Value::Str(dir.into()));
        app.set_local_prefs(p);
    }

    /// Plugin coupé (état par défaut) : ni bouton, ni fenêtre, et rien n'est
    /// demandé à l'orchestrateur, quoi qu'on appelle.
    #[test]
    fn disabled_plugin_never_records() {
        let (mut app, fake) = in_call();
        assert!(!plugins::on(Id::CallRecorder), "coupé par défaut");
        assert!(matches!(app.store.voice.conn, VoiceConnState::Connected { .. }));
        set_folder(&mut app, "C:/enregistrements");
        assert!(!app.voice_rec_shown());
        app.voice_rec_request();
        assert!(app.voice.rec.ask.is_none());
        assert!(!app.voice_rec_start());
        frame(&mut app);
        assert!(!app.voice_recording() && fake.asked.lock().unwrap().is_empty());
    }

    /// L'avertissement s'affiche quand on active le plugin à la main, pas
    /// quand `local.toml` est relu avec le plugin déjà activé.
    #[test]
    fn enabling_warns_once() {
        let (mut app, fake) = in_call();
        let mut p = app.local_prefs();
        p.plugins.set(Id::CallRecorder, true);
        app.set_local_prefs(p);
        assert!(plugins::on(Id::CallRecorder) && app.voice.rec.ask.is_none(), "réglage relu : pas de fenêtre");
        app.set_plugin(Id::CallRecorder, false);
        app.set_plugin(Id::CallRecorder, true);
        assert_eq!(app.voice.rec.ask, Some(Ask::Enabled));
        frame(&mut app);
        assert!(fake.asked.lock().unwrap().is_empty(), "activer n'enregistre rien");
    }

    /// Le bouton ouvre l'avertissement ; seul « Démarrer » enregistre ;
    /// quitter le salon arrête ; l'issue remet l'interface au repos.
    #[test]
    fn records_only_after_the_warning_and_stops_on_leaving() {
        let (mut app, fake) = in_call();
        app.set_plugin(Id::CallRecorder, true);
        app.voice.rec.ask = None;
        assert!(app.voice_rec_shown());

        // Sans dossier choisi : rien ne part.
        app.voice_rec_request();
        assert_eq!(app.voice.rec.ask, Some(Ask::Start));
        frame(&mut app);
        assert!(!app.voice_rec_start() && fake.asked.lock().unwrap().is_empty(), "le clic sur le bouton ne démarre rien");

        set_folder(&mut app, "C:/enregistrements");
        let mut p = app.local_prefs();
        p.plugins.put(Id::CallRecorder, plugins::REC_TRACKS, Value::Bool(true));
        app.set_local_prefs(p);
        assert!(app.voice_rec_start() && app.voice_recording());
        {
            let asked = fake.asked.lock().unwrap();
            let [CallRecCmd::Start { dir, base, tracks, me, names }] = asked.as_slice() else { panic!("{asked:?}") };
            assert_eq!((dir.as_path(), *tracks, *me), (std::path::Path::new("C:/enregistrements"), true, ME));
            assert!(base.starts_with("Ferrocord - "));
            assert!(names.contains(&(BOB, "personne 2".to_string())) && names.contains(&(ME, "personne 1".to_string())));
        }
        assert!(app.voice_rec_clock().is_some());
        frame(&mut app);
        assert_eq!(fake.asked.lock().unwrap().len(), 1, "rien de plus tant que personne ne bouge");

        // Je quitte le salon : arrêt demandé seul, une fois.
        app.store.voice_leave();
        frame(&mut app);
        frame(&mut app);
        assert_eq!(fake.asked.lock().unwrap().last(), Some(&CallRecCmd::Stop));
        assert_eq!(fake.asked.lock().unwrap().iter().filter(|c| **c == CallRecCmd::Stop).count(), 1);
        assert!(app.voice_recording(), "jusqu'à la fermeture des fichiers");
        fake.ups.lock().unwrap().push(CallRecUp::Done { file: "C:/enregistrements/a.ogg".into(), files: 3, ms: 1000 });
        frame(&mut app);
        assert!(!app.voice_recording());
        assert!(app.toast.as_ref().is_some_and(|t| t.text.contains("a.ogg")));
    }

    /// Couper le plugin pendant un enregistrement l'arrête.
    #[test]
    fn disabling_the_plugin_stops_the_recording() {
        let (mut app, fake) = in_call();
        app.set_plugin(Id::CallRecorder, true);
        set_folder(&mut app, "C:/enregistrements");
        assert!(app.voice_rec_start());
        app.set_plugin(Id::CallRecorder, false);
        frame(&mut app);
        assert_eq!(fake.asked.lock().unwrap().last(), Some(&CallRecCmd::Stop));
        fake.ups.lock().unwrap().push(CallRecUp::Failed(CallRecFail::Write));
        frame(&mut app);
        assert!(!app.voice_recording());
    }
}
