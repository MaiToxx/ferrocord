//! Notifications de bureau de bout en bout : la vraie application (sans
//! fenêtre) et la vraie session face au faux serveur ; le bureau est un
//! `desk::Recorder` (aucun toast, son ni registre réels).

use crate::support::{ids::*, *};
use eframe::egui;
use ferrocord::account::{AccountLink, MemoryVault, Token};
use ferrocord::desk::{Call, DeskEvent, Recorder};
use ferrocord::images::Images;
use ferrocord::session::Session;
use ferrocord::settings::SettingsLink;
use ferrocord::state::notify::{DeskPrefs, Kind};
use ferrocord::state::Store;
use ferrocord::theme::{ThemeLink, WatchConfig};
use ferrocord::ui::{App, Boot};
use serde_json::json;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn until(app: &mut App, what: &str, pred: impl Fn(&App) -> bool) {
    let end = Instant::now() + WAIT;
    loop {
        app.step();
        if pred(app) {
            return;
        }
        assert!(Instant::now() < end, "délai dépassé : {what}");
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn toasts(r: &Recorder) -> Vec<ferrocord::state::notify::Alert> {
    r.calls().into_iter().filter_map(|c| if let Call::Toast(a) = c { Some(a) } else { None }).collect()
}

fn last_tray_mentions(r: &Recorder) -> Option<u32> {
    r.calls().into_iter().rev().find_map(|c| if let Call::Tray(t) = c { Some(t.mentions) } else { None })
}

#[test]
fn mention_notifies_once_per_channel_and_click_opens_it() {
    let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().unwrap_or_else(|e| panic!("{e}"));
    let (mut gw, http) = rt.block_on(async { (FakeGateway::start().await, FakeHttp::start().await) });
    http.on("GET", &messages_path(HORS_SUJET), [Response::ok(fixture("rest_history"))]);
    let ctx = egui::Context::default();
    let boot = Boot {
        backend: Box::new(Session::with_endpoints(endpoints(&gw, &http))),
        store: Store::new(),
        theme: ThemeLink::open(None, false, WatchConfig::default()),
        settings: SettingsLink::open(None, true, WatchConfig::default()),
        accounts: AccountLink::start(Arc::new(MemoryVault::new())),
        env_token: Some(Token::parse(USER_TOKEN)),
        legacy: None,
        images: Images::disabled(ctx.clone()),
    };
    let mut app = App::with_context(ctx, boot);
    let desk = Recorder::default();
    app.attach_desktop(Box::new(desk.clone()));
    assert_eq!(desk.calls().first(), Some(&Call::Load), "notify.toml demandé au branchement");
    desk.push(DeskEvent::Prefs(DeskPrefs { volume: 40, ..DeskPrefs::default() }));

    let end = Instant::now() + WAIT;
    let mut conn = loop {
        app.step();
        if let Ok(c) = rt.block_on(async { tokio::time::timeout(Duration::from_millis(10), gw.accept()).await }) {
            break c;
        }
        assert!(Instant::now() < end, "délai dépassé : connexion");
    };
    rt.block_on(conn.hello_identify(QUIET));
    rt.block_on(conn.send(ready_with_resume(&gw, "ready_user_classic")));
    until(&mut app, "READY", |a| a.store().me.is_some());
    assert_eq!(app.desk_prefs().volume, 40, "réglages lus appliqués");

    rt.block_on(conn.dispatch_fixture("message_create_mention", 2));
    until(&mut app, "toast", |_| !toasts(&desk).is_empty());
    let a = &toasts(&desk)[0];
    assert_eq!((a.kind, a.channel, a.guild, a.title.as_str()), (Kind::Mention, HORS_SUJET, Some(ALPHA), "bob_fictif"));
    assert!(a.place.as_deref().is_some_and(|p| p.contains(" › #")), "{:?}", a.place);
    assert!(a.body.starts_with("Salut @Moi Fictif"), "mention mise en mots : {}", a.body);
    assert!(desk.calls().contains(&Call::Sound(Kind::Mention, 40)));
    until(&mut app, "pastille", |_| last_tray_mentions(&desk) == Some(1));

    // Deuxième mention dans le même salon aussitôt : comptée, sans nouvelle notification.
    let mut second = fixture("message_create_mention")["d"].clone();
    second["id"] = json!("5000000000000000304");
    rt.block_on(conn.dispatch("MESSAGE_CREATE", 3, second));
    until(&mut app, "deuxième mention", |_| last_tray_mentions(&desk) == Some(2));
    assert_eq!(toasts(&desk).len(), 1, "regroupé");

    // Mon propre message : rien.
    let mut mine = fixture("message_create_mention")["d"].clone();
    mine["id"] = json!("5000000000000000305");
    mine["channel_id"] = json!(GENERAL.to_string());
    mine["author"]["id"] = json!(ME.to_string());
    rt.block_on(conn.dispatch("MESSAGE_CREATE", 4, mine));
    let end = Instant::now() + Duration::from_millis(300);
    while Instant::now() < end {
        app.step();
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(toasts(&desk).len(), 1);

    // Clic sur la notification : le salon s'ouvre, la pastille s'éteint.
    desk.push(DeskEvent::Open { guild: Some(ALPHA), channel: HORS_SUJET });
    until(&mut app, "salon ouvert", |a| a.store().channel == Some(HORS_SUJET));
    until(&mut app, "pastille éteinte", |_| last_tray_mentions(&desk) == Some(0));
}
