//! Tests du bandeau d'appel d'un MP (`call_band.rs`) sur une application sans
//! fenêtre ni réseau : la conversation reste à l'écran pendant l'appel, la
//! sonnerie montre ses points d'attente et ses boutons, la vidéo agrandit le
//! bandeau.

use super::super::tests::{click, find, frame, offline_boot};
use super::*;
use crate::i18n::Lang;
use crate::state::calls::{Call, CallEvent};
use crate::state::model::{Avatar, Dm, Message, User};
use crate::state::voice::{VoiceEvent, VoiceState};
use crate::state::Event;

const MP: u64 = 50;
const ALICE: u64 = 7;

struct Quiet;

impl super::super::Backend for Quiet {
    fn set_wake(&mut self, _: crate::net::Wake) {}
    fn connect(&mut self, _: crate::account::Token) {}
    fn disconnect(&mut self) {}
    fn is_connected(&self) -> bool {
        true
    }
    fn poll(&mut self) -> Vec<Event> {
        Vec::new()
    }
    fn send(&mut self, _: Vec<crate::state::Cmd>) {}
}

/// MP avec Alice ouvert, un message dans l'historique.
fn app_in_dm() -> App {
    let ctx = egui::Context::default();
    let mut store = Store::new();
    let dm = Dm { id: MP, name: "Alice".into(), last_message: 0, avatar: Avatar::default_for(ALICE), recipients: vec![(ALICE, "Alice".into())] };
    store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![dm] });
    store.select_dms();
    store.select_channel(MP);
    let said = Message { channel_id: MP, author_id: ALICE, author: "Alice".into(), content: "Bonjour du MP".into(), ..crate::ui::keys::tests::test_message(1 << 22) };
    store.apply(Event::History { channel_id: MP, before: None, messages: vec![said] });
    let images = crate::images::Images::disabled(ctx.clone());
    let mut app = App::with_context(ctx, offline_boot(Box::new(Quiet), store, images));
    app.lang = Lang::Fr;
    app
}

fn state(user_id: u64) -> VoiceState {
    VoiceState { user_id, channel_id: MP, ..Default::default() }
}

fn call(ringing: Vec<u64>, states: Vec<VoiceState>) -> Event {
    Event::Calls(CallEvent::Upsert { call: Call { channel_id: MP, message_id: Some(1 << 30), ringing, ..Default::default() }, voice_states: Some(states) })
}

fn settle(app: &mut App) -> egui::FullOutput {
    for _ in 0..3 {
        frame(app, Vec::new());
    }
    frame(app, Vec::new())
}

fn count(out: &egui::FullOutput, label: &str) -> usize {
    out.platform_output.accesskit_update.iter().flat_map(|u| u.nodes.iter()).filter(|(_, n)| n.label().is_some_and(|l| l.contains(label))).count()
}

/// Délai avant le prochain redessin demandé.
fn idle(out: &egui::FullOutput) -> std::time::Duration {
    out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap_or(std::time::Duration::MAX)
}

fn must(out: &egui::FullOutput, label: &str) -> Rect {
    find(out, label).unwrap_or_else(|| panic!("« {label} » introuvable"))
}

/// Appel en cours dans un MP : la conversation reste là (en-tête, message,
/// saisie), l'appel est un bandeau au-dessus avec ses commandes.
#[test]
fn a_dm_call_keeps_the_conversation_on_screen() {
    let fr = Lang::Fr;
    let mut app = app_in_dm();
    let out = settle(&mut app);
    assert!(find(&out, "Raccrocher").is_none() && find(&out, tx::MORE.get(fr)).is_none(), "pas d'appel : pas de bandeau");
    app.store.voice_join_call(MP);
    app.store.apply(call(Vec::new(), vec![state(1), state(ALICE)]));
    let out = settle(&mut app);
    let hang = must(&out, "Raccrocher");
    let said = must(&out, "Bonjour du MP");
    let composer = must(&out, &crate::i18n::f::message_dm(fr, "Alice"));
    assert!(hang.bottom() < said.top() && said.bottom() <= composer.top() + 1.0, "bandeau, messages, saisie : {hang:?} {said:?} {composer:?}");
    // Personne ne sonne, personne ne change d'état : rien ne se redessine.
    // (La barre de défilement finit d'abord d'apparaître : une fraction de seconde.)
    let mut out = frame(&mut app, Vec::new());
    for _ in 0..100 {
        if idle(&out) >= std::time::Duration::from_secs(1) {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
        out = frame(&mut app, Vec::new());
    }
    assert!(idle(&out) >= std::time::Duration::from_secs(1), "redessin au repos pendant l'appel : {:?} {:?}", idle(&out), app.ctx.repaint_causes());
    // En-tête normal du MP : pas de bouton « Discussion », pas de « Revenir à l'appel ».
    assert!(find(&out, xt::SHOW_CHAT.get(fr)).is_none() && find(&out, xt::SHOW_CALL.get(fr)).is_none());
    // Commandes : options de la caméra, « … », sur la ligne de « Raccrocher », dans la fenêtre.
    for want in [tx::CAMERA_MENU.get(fr), tx::MORE.get(fr)] {
        let r = must(&out, want);
        assert!(r.left() >= 0.0 && r.right() <= hang.left() && (r.center().y - hang.center().y).abs() < 1.0, "{want} : {r:?}");
    }
    // Plein cadre et retour.
    app.calls_ui.band.wide = Some(MP);
    let out = settle(&mut app);
    assert!(find(&out, "Bonjour du MP").is_none(), "plein cadre : la vue d'appel remplace la conversation");
    click(&mut app, xt::SHRINK.get(fr));
    let out = settle(&mut app);
    assert!(app.calls_ui.band.wide.is_none() && find(&out, "Bonjour du MP").is_some());
    click(&mut app, "Raccrocher");
    assert!(app.store.voice.me.is_none(), "« Raccrocher » quitte l'appel");
}

/// Sonnerie : celui qui n'a pas décroché a ses points d'attente ; un appel
/// entrant se décroche ou se refuse dans le bandeau, sans doublon de la
/// fenêtre d'appel entrant, qui reste quand le MP n'est pas affiché.
#[test]
fn ringing_shows_waiting_dots_and_answer_buttons() {
    let fr = Lang::Fr;
    let mut app = app_in_dm();
    app.store.apply(call(vec![1], vec![state(ALICE)]));
    let out = settle(&mut app);
    let (accept, decline) = (tx::ACCEPT.get(fr), tx::DECLINE.get(fr));
    assert_eq!((count(&out, accept), count(&out, decline)), (1, 1), "un seul jeu de boutons, dans le bandeau");
    must(&out, "Moi, sonnerie…");
    must(&out, "Bonjour du MP");
    let people = band_people(&app.store, MP);
    assert_eq!(people.iter().map(|p| (p.id, p.ringing)).collect::<Vec<_>>(), [(ALICE, false), (1, true)]);
    // MP fermé : la fenêtre d'appel entrant prend le relais.
    app.store.channel = None;
    let out = settle(&mut app);
    assert_eq!(count(&out, accept), 1, "fenêtre d'appel entrant hors du MP");
    app.store.select_channel(MP);
    settle(&mut app);
    click(&mut app, accept);
    assert_eq!(app.store.voice.me, Some(MyVoice { guild_id: None, channel_id: MP }), "« Accepter » rejoint l'appel");
    let out = settle(&mut app);
    assert!(find(&out, "Raccrocher").is_some() && count(&out, accept) == 0);
    // Appel sortant : le correspondant sonne.
    app.store.apply(call(vec![ALICE], vec![state(1)]));
    let out = settle(&mut app);
    assert_eq!(count(&out, "sonnerie…"), 1, "Alice n'a pas encore décroché");
    assert!(idle(&out) < std::time::Duration::from_secs(1), "les points d'attente battent pendant la sonnerie");
    // Les points ne battent que pendant la sonnerie.
    app.store.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: None, user_id: ALICE, state: Some(state(ALICE)), session_id: None }));
    app.store.apply(call(Vec::new(), vec![state(1), state(ALICE)]));
    let out = settle(&mut app);
    assert_eq!(count(&out, "sonnerie…"), 0);
}

/// Caméra d'un participant : le bandeau s'agrandit, la conversation reste dessous.
#[test]
fn video_makes_the_band_taller() {
    let mut app = app_in_dm();
    app.store.voice_join_call(MP);
    app.store.apply(call(Vec::new(), vec![state(1), state(ALICE)]));
    let out = settle(&mut app);
    let before = must(&out, "Raccrocher");
    app.store.apply(call(Vec::new(), vec![state(1), VoiceState { self_video: true, ..state(ALICE) }]));
    assert!(app.call_has_video(MP));
    let out = settle(&mut app);
    let after = must(&out, "Raccrocher");
    assert!(after.top() > before.top() + 60.0, "bandeau agrandi : {before:?} puis {after:?}");
    let said = must(&out, "Bonjour du MP");
    assert!(said.top() > after.bottom());
    assert!(app.store.video.sink.is_visible(&crate::state::video::Tile::Camera(ALICE)), "la tuile vidéo d'Alice est montrée");
    assert_eq!(app.call_band_height(), None, "hauteur par défaut tant qu'on ne tire pas la bordure");
}
