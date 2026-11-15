//! Disposition mobile de bout en bout, sans fenêtre : choix du mode, bureau
//! inchangé, onglets, retour, fenêtres plein écran, appui long.

use super::super::App;
use super::{nav::Screen, nav::Tab, TOUCH};
use crate::state::mobile::Layout;
use crate::state::model::{Avatar, Channel, ChannelKind, Guild, Message, User, DISCORD_EPOCH};
use crate::state::{Cmd, Event, Store};
use eframe::egui;

const GUILD: u64 = 9;
const CHAN: u64 = 100;
const ME: u64 = 1;

struct Offline;

impl super::super::Backend for Offline {
    fn set_wake(&mut self, _: super::super::Wake) {}
    fn connect(&mut self, _: crate::account::Token) {}
    fn disconnect(&mut self) {}
    fn is_connected(&self) -> bool {
        true
    }
    fn poll(&mut self) -> Vec<Event> {
        Vec::new()
    }
    fn send(&mut self, _: Vec<Cmd>) {}
}

fn msg(i: u64) -> Message {
    let ms = 1_700_000_000_000 + i * 60_000;
    Message {
        id: ((ms - DISCORD_EPOCH) << 22) | i,
        channel_id: CHAN,
        guild_id: Some(GUILD),
        author_id: 10 + i % 3,
        author: format!("u{}", 10 + i % 3),
        author_avatar: Avatar::default_for(10 + i % 3),
        content: format!("message {i} ").repeat(4),
        attachments: Vec::new(),
        embeds: Vec::new(),
        mentions: Vec::new(),
        mention_everyone: false,
        edited: false,
        system: None,
        meta: Default::default(),
    }
}

struct Phone {
    app: App,
    time: f64,
    size: egui::Vec2,
    /// Délai de redessin demandé par la dernière image (`Duration::MAX` : aucun).
    wait: std::time::Duration,
}

impl Phone {
    fn new(layout: Layout, size: egui::Vec2) -> Self {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        let channels = (0..8u64).map(|c| Channel { id: CHAN + c, name: format!("s{c}"), kind: ChannelKind::Text, position: c as i32, parent: None, overwrites: Some(Vec::new()), slowmode: 0 }).collect();
        store.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![Guild::new(GUILD, "G".into(), channels)], dms: Vec::new() });
        store.select_guild(GUILD);
        store.select_channel(CHAN);
        store.apply(Event::History { channel_id: CHAN, before: None, messages: (0..30).map(msg).collect() });
        store.take_commands();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), store, images));
        let mut p = app.local_prefs();
        p.mobile.layout = layout;
        app.set_local_prefs(p);
        Phone { app, time: 0.0, size, wait: std::time::Duration::ZERO }
    }

    /// Une image de 1/60 s (ou `dt`), avec ces événements ; rend ce qui a été peint.
    fn frame_with(&mut self, events: Vec<egui::Event>, dt: f64) -> Vec<egui::epaint::ClippedShape> {
        let ctx = self.app.ctx.clone();
        let mut raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)), time: Some(self.time), events, ..Default::default() };
        self.time += dt;
        self.app.mobile_input(&mut raw);
        self.app.logic_pass();
        let mut out = ctx.run_ui(raw, |ui| self.app.ui_pass(ui));
        self.wait = out.viewport_output.get(&egui::ViewportId::ROOT).map_or(std::time::Duration::ZERO, |v| v.repaint_delay);
        out.textures_delta.clear();
        out.shapes
    }

    fn frames(&mut self, n: usize) -> Vec<egui::epaint::ClippedShape> {
        let mut last = Vec::new();
        for _ in 0..n {
            last = self.frame_with(Vec::new(), 1.0 / 60.0);
        }
        last
    }

    fn key(&mut self, key: egui::Key) {
        let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: egui::Modifiers::NONE };
        self.frame_with(vec![ev(true), ev(false)], 1.0 / 60.0);
        self.frames(1);
    }
}

/// Textes peints à cette image.
fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<String> {
    shapes
        .iter()
        .filter_map(|s| match &s.shape {
            egui::Shape::Text(t) => Some(t.galley.text().to_string()),
            _ => None,
        })
        .collect()
}

#[test]
fn wide_auto_window_draws_exactly_the_desktop_layout() {
    let size = egui::vec2(1000.0, 700.0);
    let mut auto = Phone::new(Layout::Auto, size);
    let mut desk = Phone::new(Layout::Desktop, size);
    let (a, d) = (auto.frames(4), desk.frames(4));
    assert!(!auto.app.is_mobile() && !desk.app.is_mobile());
    assert_eq!(a, d, "fenêtre large : la disposition automatique est celle du bureau, à l'identique");
    // La hauteur des contrôles est celle du thème (jeton `control_height`, lot V1).
    let default_interact = auto.app.theme.spacing.control_height;
    assert_eq!(auto.app.ctx.global_style().spacing.interact_size.y, default_interact, "style d'egui intact");
    assert!(!texts(&a).iter().any(|s| s == crate::i18n::table::mobile::t::TAB_HOME.get(auto.app.lang)), "pas de barre d'onglets");
}

#[test]
fn narrow_window_switches_to_mobile_and_back() {
    let mut p = Phone::new(Layout::Auto, egui::vec2(390.0, 844.0));
    let shapes = p.frames(3);
    assert!(p.app.is_mobile());
    assert!(p.app.ctx.global_style().spacing.interact_size.y >= TOUCH);
    assert!(p.app.theme.spacing.channel_row >= TOUCH, "lignes de salon à la taille du doigt");
    assert!(p.app.theme.typography.body >= 16.0);
    let lang = p.app.lang;
    let shown = texts(&shapes);
    for tab in super::TABS {
        assert!(shown.iter().any(|s| s == tab.1.get(lang)), "onglet {:?} absent", tab.0);
    }
    // Choix « Bureau » : même fenêtre étroite, disposition du bureau.
    let mut prefs = p.app.local_prefs();
    prefs.mobile.layout = Layout::Desktop;
    p.app.set_local_prefs(prefs);
    p.frames(3);
    assert!(!p.app.is_mobile());
    assert_eq!(p.app.ctx.global_style().spacing.interact_size.y, p.app.theme.spacing.control_height);
    assert_eq!(p.app.theme.spacing.channel_row, p.app.look.edited.scaled().spacing.channel_row);
    // Fenêtre large en automatique : bureau.
    let mut prefs = p.app.local_prefs();
    prefs.mobile.layout = Layout::Auto;
    p.app.set_local_prefs(prefs);
    p.size = egui::vec2(1200.0, 800.0);
    p.frames(2);
    assert!(!p.app.is_mobile());
}

#[test]
fn back_key_and_header_button_walk_the_stack() {
    let mut p = Phone::new(Layout::Mobile, egui::vec2(390.0, 844.0));
    p.frames(2);
    assert!(p.app.mobile_show("membres"));
    p.frames(2);
    assert_eq!(p.app.mobile.nav.screen(), Screen::Members);
    p.key(egui::Key::BrowserBack);
    assert_eq!(p.app.mobile.nav.screen(), Screen::Chat);
    // Bouton retour de l'en-tête (demande déposée comme le fait le bouton).
    p.app.ctx.data_mut(|d| d.insert_temp(super::back_id(), true));
    p.frames(1);
    assert_eq!(p.app.mobile.nav.screen(), Screen::List);
    p.app.mobile_show("vous");
    p.frames(1);
    assert_eq!(p.app.mobile.nav.tab(), Tab::You);
    p.key(egui::Key::BrowserBack);
    assert_eq!(p.app.mobile.nav.tab(), Tab::Home);
}

#[test]
fn settings_window_is_full_screen_with_back() {
    let mut p = Phone::new(Layout::Mobile, egui::vec2(390.0, 844.0));
    p.frames(2);
    assert!(p.app.open_settings("apparence"));
    p.frames(3);
    let rect = p.app.ctx.memory(|m| m.area_rect(egui::Id::unique("parametres")));
    let rect = rect.unwrap_or_else(|| panic!("fenêtre Paramètres absente"));
    assert!((rect.width() - 390.0).abs() < 1.0 && (rect.height() - 844.0).abs() < 1.0, "plein écran : {rect:?}");
    // Retour : la liste des pages, puis fermeture.
    p.key(egui::Key::BrowserBack);
    assert!(p.app.params.open);
    p.key(egui::Key::BrowserBack);
    assert!(!p.app.params.open);
}

#[test]
fn members_button_opens_the_drawer_without_touching_the_desktop_setting() {
    let mut p = Phone::new(Layout::Mobile, egui::vec2(390.0, 844.0));
    p.app.mobile_show("discussion");
    p.frames(2);
    let before = p.app.look.edited.layout.members;
    // Ce que fait le bouton « Membres » de l'en-tête.
    p.app.look.edited.layout.members = if before == crate::theme::MembersPanel::Shown { crate::theme::MembersPanel::Hidden } else { crate::theme::MembersPanel::Shown };
    // Le changement fait pendant l'image est repris après elle : simulé ici avant l'image.
    let screen = p.app.mobile.nav.screen();
    let place = (p.app.store.guild, p.app.store.channel);
    p.app.mobile_navigate(&p.app.ctx.clone(), screen, Tab::Home, place, false, before);
    assert_eq!(p.app.look.edited.layout.members, before);
    assert_eq!(p.app.mobile.nav.screen(), Screen::Members);
}

#[test]
fn long_press_with_the_mouse_opens_the_message_menu() {
    let mut p = Phone::new(Layout::Mobile, egui::vec2(390.0, 844.0));
    p.app.mobile_show("discussion");
    p.frames(4);
    // Même point que l'aperçu (`--mobile appui-long`) : sur un message, au-dessus de la saisie.
    let area = egui::Rect::from_min_size(egui::Pos2::ZERO, p.size);
    let at = egui::pos2(area.center().x, area.bottom() - 260.0);
    let press = egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE };
    p.frame_with(vec![egui::Event::PointerMoved(at), press], 1.0 / 60.0);
    assert!(!p.app.ctx.any_popup_open());
    for _ in 0..8 {
        p.frame_with(Vec::new(), 0.25);
    }
    assert!(p.app.ctx.any_popup_open(), "appui long : le menu du clic droit s'ouvre");
}

/// Au repos, aucun écran mobile ne redemande d'image (au plus un réveil
/// lointain : horloge, délai d'un message).
#[test]
fn mobile_screens_are_quiet_at_rest() {
    for (screen, size) in [("salons", (390.0, 844.0)), ("discussion", (390.0, 844.0)), ("membres", (360.0, 780.0)), ("messages", (360.0, 780.0)), ("notifications", (390.0, 844.0)), ("vous", (360.0, 780.0))] {
        let mut p = Phone::new(Layout::Mobile, egui::vec2(size.0, size.1));
        assert!(p.app.mobile_show(screen));
        p.frames(90);
        assert!(p.wait >= std::time::Duration::from_secs(1), "{screen} : redessin au repos dans {:?}", p.wait);
    }
}

/// Glissement lancé sur la discussion : le défilement continue sur sa
/// lancée, puis s'arrête, et les images s'arrêtent avec lui.
#[test]
fn fling_on_the_chat_comes_to_rest() {
    let mut p = Phone::new(Layout::Mobile, egui::vec2(390.0, 844.0));
    p.app.mobile_show("discussion");
    p.frames(4);
    let x = p.size.x / 2.0;
    let at = |y: f32| egui::pos2(x, y);
    p.frame_with(vec![egui::Event::PointerMoved(at(300.0)), egui::Event::PointerButton { pos: at(300.0), button: egui::PointerButton::Primary, pressed: true, modifiers: egui::Modifiers::NONE }], 1.0 / 60.0);
    for i in 1..=6 {
        p.frame_with(vec![egui::Event::PointerMoved(at(300.0 + 40.0 * i as f32))], 1.0 / 60.0);
    }
    p.frame_with(vec![egui::Event::PointerButton { pos: at(540.0), button: egui::PointerButton::Primary, pressed: false, modifiers: egui::Modifiers::NONE }, egui::Event::PointerGone], 1.0 / 60.0);
    let mut n = 0;
    while p.wait < std::time::Duration::from_secs(1) {
        n += 1;
        assert!(n < 600, "le défilement ne s'arrête pas (10 s d'images)");
        p.frame_with(Vec::new(), 1.0 / 60.0);
    }
}
