//! Modération et administration (lots A4 + A5) face au faux serveur :
//! raison dans `X-Audit-Log-Reason`, corps des écritures, listes lues par
//! les vrais décodeurs, suppression un par un pour un compte utilisateur.
//! Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::admin::audit::Filter;
use ferrocord::state::admin::automod::{Rule, Trigger};
use ferrocord::state::admin::moderation::{Load, ModCmd, ModEvent, ModNotice, SafetyChange};
use ferrocord::state::{Cmd, Event};
use serde_json::{json, Value};

fn moderation(e: &Event) -> Option<&ModEvent> {
    match e {
        Event::Moderation(m) => Some(m),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

fn header<'a>(r: &'a Request, name: &str) -> Option<&'a str> {
    r.headers.iter().find(|(n, _)| n == name).map(|(_, v)| v.as_str())
}

#[tokio::test]
async fn kick_and_ban_carry_the_reason_header() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let member = format!("/api/v10/guilds/{ALPHA}/members/{ALICE}");
    s.http.on("DELETE", &member, [no_content()]);
    assert!(s.client.store.kick_member(ALPHA, ALICE, " spam répété "));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", member.as_str()));
    assert_eq!(header(&r, "x-audit-log-reason"), Some("spam%20r%C3%A9p%C3%A9t%C3%A9"));
    s.client.until("expulsé", |e| matches!(moderation(e), Some(ModEvent::Kicked { .. }))).await;
    assert_eq!(s.client.store.moderation.take_notice(), Some(ModNotice::Done(ferrocord::state::admin::moderation::ModAction::Kick)));

    let ban = format!("/api/v10/guilds/{ALPHA}/bans/{ALICE}");
    s.http.on("PUT", &ban, [no_content()]);
    assert!(s.client.store.ban_member(ALPHA, ALICE, 86_400, ""));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "delete_message_seconds": 86_400 }));
    assert_eq!(header(&r, "x-audit-log-reason"), None, "raison vide : pas d'en-tête");
    s.client.until("banni", |e| matches!(moderation(e), Some(ModEvent::Banned { .. }))).await;
}

#[tokio::test]
async fn bans_audit_and_rules_are_listed() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}/bans"), [Response::ok(fixture("rest_guild_bans"))]);
    assert!(s.client.store.load_bans(ALPHA, false));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("/api/v10/guilds/{ALPHA}/bans?limit=1000"));
    s.client.until("bannis", |e| matches!(moderation(e), Some(ModEvent::Bans { .. }))).await;
    let Some(Load::Ready { items, more: false }) = s.client.store.moderation.bans.as_ref().map(|v| &v.list) else { panic!("liste des bannis") };
    assert_eq!(items.len(), 2);

    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}/audit-logs"), [Response::ok(fixture("rest_audit_log"))]);
    let filter = Filter { user_id: Some(ALICE), action: Some(22) };
    assert!(s.client.store.load_audit(ALPHA, filter, false));
    s.client.flush();
    let r = s.http.next_request().await;
    assert!(r.path.contains("limit=50") && r.path.contains(&format!("user_id={ALICE}")) && r.path.contains("action_type=22"), "{}", r.path);
    s.client.until("journal", |e| matches!(moderation(e), Some(ModEvent::Audit { .. }))).await;
    assert!(matches!(s.client.store.moderation.audit.as_ref().map(|v| &v.list), Some(Load::Ready { items, .. }) if items.len() == 3));

    let rules = format!("/api/v10/guilds/{ALPHA}/auto-moderation/rules");
    s.http.on("POST", &rules, [Response::ok(fixture("rest_automod_rules")[0].clone())]);
    let rule = Rule::new(Trigger::Keyword { keywords: vec!["crédits".into()], regex: Vec::new(), allow: Vec::new() }, "Arnaques".into());
    assert!(s.client.store.save_rule(ALPHA, rule));
    s.client.flush();
    let r = s.http.next_request().await;
    let body = r.json();
    assert_eq!((body["name"].clone(), body["trigger_type"].clone(), body["event_type"].clone()), (json!("Arnaques"), json!(1), json!(1)));
    assert_eq!(body["trigger_metadata"]["keyword_filter"], json!(["crédits"]));
    assert_eq!(body["actions"], json!([{ "type": 1, "metadata": {} }]));
    s.client.until("règle", |e| matches!(moderation(e), Some(ModEvent::RuleSaved { mine: true, .. }))).await;
    assert!(s.client.store.moderation.take_saved_rule().is_some());
}

#[tokio::test]
async fn safety_is_read_written_and_raids_paused() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let guild = format!("/api/v10/guilds/{ALPHA}");
    s.http.on("GET", &guild, [Response::ok(fixture("rest_guild_safety"))]);
    s.http.on("PATCH", &guild, [Response::ok(fixture("rest_guild_safety"))]);
    s.http.on("PUT", &format!("{guild}/incident-actions"), [Response::ok(json!({ "invites_disabled_until": null, "dms_disabled_until": null }))]);
    assert!(s.client.store.load_safety(ALPHA));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("sécurité", |e| matches!(moderation(e), Some(ModEvent::Safety { .. }))).await;
    assert!(matches!(s.client.store.moderation.safety.as_ref().map(|v| &v.state), Some(Load::Ready { items, .. }) if items.verification == Some(2) && items.mfa == Some(1)));

    assert!(s.client.store.set_safety(ALPHA, SafetyChange::Verification(3)));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("PATCH", json!({ "verification_level": 3 })));
    s.client.until("écrit", |e| matches!(moderation(e), Some(ModEvent::Safety { mine: true, .. }))).await;

    let until = s.client.store.now() + 3_600_000;
    assert!(s.client.store.set_incidents(ALPHA, Some(until), None));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "PUT");
    let body = r.json();
    assert!(body["invites_disabled_until"].as_str().is_some_and(|t| t.ends_with('Z')));
    assert_eq!(body["dms_disabled_until"], Value::Null);
    // Réponse partielle : le serveur est relu en entier.
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", guild.as_str()));
}

#[tokio::test]
async fn user_account_deletes_the_selection_one_by_one() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let now = s.client.store.now();
    // Deux identifiants récents (l'heure est dans les bits hauts).
    let id = |n: u64| ((u64::try_from(now - 1_420_070_400_000).unwrap_or(0) - 1000) << 22) + n;
    let ids = vec![id(1), id(2)];
    for m in &ids {
        s.http.on("DELETE", &format!("/api/v10/channels/{GENERAL}/messages/{m}"), [no_content()]);
    }
    s.client.send(Cmd::Moderation(ModCmd::BulkDelete { channel_id: GENERAL, ids: ids.clone(), now_ms: now }));
    let a = s.http.next_request().await;
    let b = s.http.next_request().await;
    assert_eq!((a.method.as_str(), b.method.as_str()), ("DELETE", "DELETE"));
    assert!(!a.path.contains("bulk-delete") && !b.path.contains("bulk-delete"), "route groupée réservée aux bots");
    s.client.until("supprimés", |e| matches!(moderation(e), Some(ModEvent::BulkDeleted { deleted: 2, failed: 0, .. }))).await;
}
