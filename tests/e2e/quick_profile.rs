//! Édition rapide du profil : couleurs du thème, profil propre à un serveur,
//! refus de Discord reconnus (jeu `rest_profile_errors`). Faux serveur local
//! seulement.

use crate::support::{ids::*, *};
use ferrocord::state::usersettings::{AccountEvent, GuildProfileEdit, ProfileEdit};
use ferrocord::state::{Effect, Event, Notice, Problem, ProfileRefusal};
use serde_json::json;

fn account(e: &Event) -> Option<&AccountEvent> {
    match e {
        Event::Account(a) => Some(a),
        _ => None,
    }
}

/// Session ouverte, profil du compte lu (`GET /users/@me`).
async fn with_profile() -> Setup {
    let mut s = connected_with(USER_TOKEN, "ready_user_classic", |_| {}).await;
    s.http.on("GET", "/api/v10/users/@me", [Response::ok(fixture("rest_me"))]);
    s.client.store.account.load_profile();
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, "/api/v10/users/@me");
    s.client.until("profil", |e| matches!(account(e), Some(AccountEvent::Profile(_)))).await;
    s
}

#[tokio::test]
async fn theme_colors_and_guild_profile_are_written() {
    let mut s = with_profile().await;
    let guild_path = format!("/api/v10/guilds/{ALPHA}/profile/@me");
    s.http.on("PATCH", "/api/v10/users/@me/profile", [Response::ok(json!({ "theme_colors": [1122867, 4478310] }))]);
    s.http.on("PATCH", &guild_path, [Response::ok(json!({ "guild_id": ALPHA.to_string(), "bio": "Ici.", "pronouns": "" }))]);

    // Couleurs du thème : seules, dans le profil (pas de PATCH /users/@me).
    let edit = ProfileEdit { theme: Some(Some([0x112233, 0x445566])), ..Default::default() };
    s.client.store.account.save_profile(edit).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", "/api/v10/users/@me/profile", json!({ "theme_colors": [1122867, 4478310] })));
    s.client.until("thème enregistré", |e| matches!(account(e), Some(AccountEvent::ProfileWritten { result: Ok(()), .. }))).await;

    // Profil propre au serveur : sa route, seulement ce qui change.
    let edit = GuildProfileEdit { bio: Some("Ici.".into()), pronouns: Some(String::new()), theme: None };
    s.client.store.account.save_guild_profile(ALPHA, edit).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(s.client.store.account.guild_profile_busy());
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", guild_path.as_str(), json!({ "bio": "Ici.", "pronouns": "" })));
    s.client.until("profil du serveur enregistré", |e| matches!(account(e), Some(AccountEvent::GuildProfileWritten { guild: ALPHA, result: Ok(()), .. }))).await;
    assert!(!s.client.store.account.guild_profile_busy());
    assert_eq!(s.client.store.account.take_profile_failure(), None);
    assert_eq!(s.http.requests().len(), 3, "une requête par geste, rien d'autre");
}

#[tokio::test]
async fn refusals_are_recognised_and_the_profile_is_restored() {
    let mut s = with_profile().await;
    let all = fixture("rest_profile_errors");
    let case = |key: &str| all.as_array().and_then(|l| l.iter().find(|c| c["name"] == key)).map(|c| c["response"].clone()).unwrap_or_else(|| panic!("cas {key} absent"));
    let guild_path = format!("/api/v10/guilds/{ALPHA}/profile/@me");
    s.http.on("PATCH", "/api/v10/users/@me", [Response::raw(400, case("name_rate_limit"))]);
    s.http.on("PATCH", &guild_path, [Response::raw(400, case("theme_premium"))]);
    let before = s.client.store.account.profile.ready().cloned();

    // Nom changé trop souvent : dit tel quel, le nom d'avant revient.
    let edit = ProfileEdit { global_name: Some(Some("Autre Nom".into())), ..Default::default() };
    s.client.store.account.save_profile(edit).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    assert_eq!(s.http.next_request().await.json(), json!({ "global_name": "Autre Nom" }));
    s.client.until("nom refusé", |e| matches!(account(e), Some(AccountEvent::ProfileWritten { result: Err(Problem::Profile(ProfileRefusal::NameRateLimit)), .. }))).await;
    assert_eq!(s.client.store.account.profile.ready().cloned(), before);
    assert_eq!(s.client.store.account.take_profile_failure(), Some(Problem::Profile(ProfileRefusal::NameRateLimit)));
    assert_eq!(s.client.store.account.take_profile_failure(), None, "rendu une fois");
    assert!(s.client.effects.contains(&Effect::Toast(Notice::SettingNotSaved(Problem::Profile(ProfileRefusal::NameRateLimit)))));

    // Couleurs de thème d'un profil de serveur sans Nitro.
    let edit = GuildProfileEdit { theme: Some(Some([1, 2])), ..Default::default() };
    s.client.store.account.save_guild_profile(ALPHA, edit).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.flush();
    assert_eq!(s.http.next_request().await.json(), json!({ "theme_colors": [1, 2] }));
    s.client.until("thème refusé", |e| matches!(account(e), Some(AccountEvent::GuildProfileWritten { result: Err(Problem::Profile(ProfileRefusal::Nitro)), .. }))).await;
    assert_eq!(s.client.store.account.take_profile_failure(), Some(Problem::Profile(ProfileRefusal::Nitro)));
    // Un refus n'est pas une session morte.
    assert!(s.client.effects.iter().all(|e| !matches!(e, Effect::Fatal { .. })), "{:?}", s.client.effects);
}
