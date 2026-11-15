use super::super::keys::tests::{frame, key};
use super::*;
use crate::i18n::table::expressions::t as xt;
use crate::state::admin::expressions::{Author, ExprEvent, ExprNotice, Failure, Kind, Loaded, Op, Sound, SoundEmoji};
use crate::state::model::{Emoji, Guild};
use crate::state::{Event, Problem};
use std::time::{Duration, Instant};

const G: u64 = 9;

fn emoji(id: u64, animated: bool) -> Emoji {
    Emoji { id, name: format!("e{id}"), animated, available: true, roles: Vec::new() }
}

/// Application d'essai où « Gérer les expressions » est connue sur G (droits
/// inconnus : rien n'est proposé, `Authority::Unknown`).
fn app() -> App {
    let mut app = super::super::keys::tests::app();
    let mut g: Guild = app.store.guilds.iter().find(|g| g.id == G).cloned().unwrap_or_else(|| panic!("serveur"));
    g.owner_id = Some(2);
    g.roles = vec![crate::state::model::Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: crate::state::perms::Permissions::MANAGE_GUILD_EXPRESSIONS, mentionable: false }];
    g.me = Some(crate::state::model::Member { roles: Vec::new(), ..Default::default() });
    app.store.apply(Event::GuildCreate(g));
    app
}

/// Les trois pages se dessinent avec leurs données ; chacune demande sa
/// liste REST une fois ; Échap ferme.
#[test]
fn pages_load_once_and_escape_closes() {
    let mut app = app();
    app.open_guild_settings(G, Some(Page::Emojis));
    frame(&mut app, Vec::new());
    assert_eq!(app.store.expressions.loaded(G, Kind::Emoji), Some(&Loaded::Loading));
    let author = Author { id: 3, name: "Alice".into() };
    app.store.apply(Event::Expressions(ExprEvent::Emojis { guild_id: G, emojis: vec![emoji(1, false), emoji(2, true)], authors: vec![(1, author.clone())] }));
    frame(&mut app, Vec::new());
    assert_eq!(app.store.emoji_counts(G), (1, 1));
    for page in [Page::Stickers, Page::Sounds] {
        app.open_guild_settings(G, Some(page));
        frame(&mut app, Vec::new());
        assert_eq!(app.store.expressions.loaded(G, page.kind().unwrap_or(Kind::Emoji)), Some(&Loaded::Loading));
    }
    let sound = Sound { id: 5, name: "ding".into(), volume: 60, emoji: Some(SoundEmoji::Unicode("🔔".into())), available: true, author: Some(author) };
    app.store.apply(Event::Expressions(ExprEvent::Sounds { guild_id: G, list: vec![sound] }));
    frame(&mut app, Vec::new());
    frame(&mut app, Vec::new());
    assert_eq!(app.store.expressions.loaded(G, Kind::Sound), Some(&Loaded::Ready), "pas redemandée");
    frame(&mut app, vec![key(egui::Key::Escape, egui::Modifiers::NONE)]);
    assert_eq!(app.guild_settings.win, None);
}

/// Sans la permission, la fenêtre ne s'ouvre pas ; retirée, elle se ferme.
#[test]
fn permission_is_required() {
    let mut app = app();
    let lose = |app: &mut App| {
        let mut g: Guild = app.store.guilds.iter().find(|g| g.id == G).cloned().unwrap_or_else(|| panic!("serveur"));
        // Propriétaire connu (un autre), rôle @everyone sans permission : plus rien.
        g.owner_id = Some(2);
        g.roles = vec![crate::state::model::Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: crate::state::perms::Permissions(0), mentionable: false }];
        g.me = Some(crate::state::model::Member { roles: Vec::new(), ..Default::default() });
        app.store.apply(Event::GuildCreate(g));
    };
    app.open_guild_settings(G, Some(Page::Sounds));
    frame(&mut app, Vec::new());
    assert!(app.guild_settings.win.is_some());
    lose(&mut app);
    frame(&mut app, Vec::new());
    assert_eq!(app.guild_settings.win, None);
    app.open_guild_settings(G, Some(Page::Sounds));
    assert_eq!(app.guild_settings.win, None);
}

/// Un fichier choisi ouvre le formulaire d'envoi ; un refus de Discord
/// (double authentification) devient un message clair.
#[test]
fn picked_file_opens_the_form_and_failures_are_told() {
    let mut app = app();
    app.open_guild_settings(G, Some(Page::Sounds));
    frame(&mut app, Vec::new());
    app.pick_expression_bytes(Page::Sounds, "ding dong.mp3", crate::state::admin::expr_files::tests::mp3(100));
    let deadline = Instant::now() + Duration::from_secs(30);
    while app.guild_settings.form.draft.is_none() {
        assert!(Instant::now() < deadline, "formulaire jamais ouvert");
        std::thread::sleep(Duration::from_millis(5));
        frame(&mut app, Vec::new());
    }
    let d = app.guild_settings.form.draft.as_ref().unwrap_or_else(|| panic!("brouillon"));
    assert_eq!((d.kind, d.name.as_str()), (Kind::Sound, "ding_dong"));
    // Fichier refusé : message sous l'en-tête, pas de formulaire.
    app.pick_expression_bytes(Page::Sounds, "x.wav", b"RIFF....WAVE".to_vec());
    while app.guild_settings.form.error.is_none() {
        assert!(Instant::now() < deadline, "refus jamais dit");
        std::thread::sleep(Duration::from_millis(5));
        frame(&mut app, Vec::new());
    }
    assert_eq!(app.guild_settings.form.error.as_deref(), Some(xt::UNREADABLE.get(app.lang)));
    let n = ExprNotice::Failed(Kind::Sound, Op::Create, Failure::Problem(Problem::MfaRequired));
    let text = app.expr_notice(&n).unwrap_or_default();
    assert!(text.contains("deux facteurs"), "{text}");
}
