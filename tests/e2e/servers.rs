//! Serveurs (lot S7 + A1) : rejoindre, créer, quitter, invitations, et
//! écriture des dossiers du rail dans les réglages synchronisés, face au
//! faux serveur. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::guild_folders::{Edit, Folders, Target};
use ferrocord::state::servers::{Failure, InviteList, ServerEvent, ServerNotice};
use ferrocord::state::usersettings::proto::Msg;
use ferrocord::state::usersettings::{b64, AccountEvent};
use ferrocord::state::Event;
use serde_json::{json, Value};

fn servers(e: &Event) -> Option<&ServerEvent> {
    match e {
        Event::Servers(s) => Some(s),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

#[tokio::test]
async fn join_by_invite_then_the_guild_opens() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", "/api/v10/invites/fictif", [Response::ok(fixture("rest_invite_accept"))]);
    assert!(s.client.store.join_invite("https://discord.gg/fictif"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("POST", "/api/v10/invites/fictif", json!({})));
    s.client.until("rejoint", |e| matches!(servers(e), Some(ServerEvent::Joined { .. }))).await;
    assert_eq!(s.client.store.servers.take_notice(), Some(ServerNotice::Joined { name: "Club fictif".into(), verify: true }));
    // Le serveur arrive par la passerelle : il s'ouvre.
    let mut g = fixture("guild_create_beta");
    g["d"]["id"] = "2000000000000000301".into();
    g["d"]["channels"][0]["id"] = "3000000000000000301".into();
    s.conn.dispatch("GUILD_CREATE", 30, g["d"].clone()).await;
    s.client.until("serveur reçu", |e| matches!(e, Event::GuildCreate(_))).await;
    assert_eq!(s.client.store.guild, Some(2000000000000000301));
}

#[tokio::test]
async fn captcha_is_never_bypassed_and_unknown_invite_is_reported() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let captcha = Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "x", "captcha_service": "hcaptcha" }));
    s.http.on("POST", "/api/v10/invites/abc", [captcha, Response::error(404, "Unknown Invite")]);
    assert!(s.client.store.join_invite("abc"));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("captcha", |e| matches!(servers(e), Some(ServerEvent::Failed { failure: Failure::Captcha, .. }))).await;
    assert_eq!(s.http.requests().len(), 1, "jamais retenté");
    assert!(s.client.store.join_invite("abc"));
    s.client.flush();
    s.client.until("inconnue", |e| matches!(servers(e), Some(ServerEvent::Failed { failure: Failure::NotFound, .. }))).await;
}

#[tokio::test]
async fn create_guild_and_leave_one() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", "/api/v10/guilds", [Response::ok(fixture("rest_guild_create"))]);
    assert!(s.client.store.create_guild("Mon serveur", Some("data:image/png;base64,AA==".into())));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "name": "Mon serveur", "icon": "data:image/png;base64,AA==" }));
    s.client.until("créé", |e| matches!(servers(e), Some(ServerEvent::Created { .. }))).await;

    let leave = format!("/api/v10/users/@me/guilds/{ALPHA}");
    s.http.on("DELETE", &leave, [no_content()]);
    assert!(s.client.store.leave_guild(ALPHA));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", leave.as_str()));
    s.client.until("quitté", |e| matches!(servers(e), Some(ServerEvent::Left { .. }))).await;
    assert!(!s.client.store.guilds.iter().any(|g| g.id == ALPHA));
}

#[tokio::test]
async fn invites_are_created_listed_and_revoked() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let mut created = fixture("rest_guild_invites")[0].clone();
    created["code"] = "neuf".into();
    s.http.on("POST", &format!("/api/v10/channels/{GENERAL}/invites"), [Response::ok(created)]);
    assert!(s.client.store.create_invite(GENERAL, 3600, 5, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "max_age": 3600, "max_uses": 5, "temporary": true }));
    s.client.until("invitation", |e| matches!(servers(e), Some(ServerEvent::InviteCreated(_)))).await;
    assert_eq!(s.client.store.servers.created.as_ref().map(|i| i.url()).as_deref(), Some("https://discord.gg/neuf"));

    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}/invites"), [Response::ok(fixture("rest_guild_invites"))]);
    s.http.on("DELETE", "/api/v10/invites/invfictif1", [Response::ok(fixture("rest_guild_invites")[0].clone())]);
    assert!(s.client.store.load_invites(ALPHA));
    s.client.flush();
    s.client.until("liste", |e| matches!(servers(e), Some(ServerEvent::Invites { .. }))).await;
    assert!(s.client.store.revoke_invite(ALPHA, "invfictif1"));
    s.client.flush();
    s.client.until("révoquée", |e| matches!(servers(e), Some(ServerEvent::Revoked { .. }))).await;
    let Some(InviteList::Ready(list)) = s.client.store.servers.invites.get(&ALPHA) else { panic!("liste") };
    assert_eq!(list.iter().map(|i| i.code.as_str()).collect::<Vec<_>>(), vec!["invfictif2"]);
}

#[tokio::test]
async fn folder_written_by_drop_sends_only_the_folders_field() {
    let mut s = connected_with(USER_TOKEN, "ready_user_classic", |p| {
        p["d"]["user_settings_proto"] = fixture("rest_settings_proto")["settings"].clone();
    })
    .await;
    assert!(s.client.store.account.proto_ready());
    let known: Vec<u64> = s.client.store.guilds.iter().map(|g| g.id).collect();
    assert!(known.len() >= 2);
    let path = "/api/v10/users/@me/settings-proto/1";
    s.http.on("PATCH", path, [Response::ok(json!({ "settings": "" }))]);
    // Le deuxième serveur déposé sur le premier : un dossier.
    let edit = Edit::MoveGuild { guild: known[1], to: Target::Combine(0) };
    assert_eq!(ferrocord::state::guild_folders::write(&mut s.client.store.account, &known, &edit, 99), Ok(true));
    s.client.flush();
    let r = s.http.next_request().await;
    let v: Value = serde_json::from_str(&r.body).unwrap_or_default();
    let bytes = b64::decode(v["settings"].as_str().unwrap_or_default()).unwrap_or_default();
    let sent = Msg::decode(&bytes).unwrap_or_default();
    assert_eq!(sent.top_numbers(), vec![14], "seul guild_folders part");
    let f = Folders::read(&sent).unwrap_or_default();
    assert_eq!(f.entries[0].folder.guilds, vec![known[0], known[1]]);
    assert!(f.entries[0].folder.id.is_some());
    s.client.until("écrit", |e| matches!(e, Event::Account(AccountEvent::ProtoWritten { .. }))).await;
    assert!(s.client.store.account.snapshot().folders.iter().any(|f| f.guilds == vec![known[0], known[1]]));
}
