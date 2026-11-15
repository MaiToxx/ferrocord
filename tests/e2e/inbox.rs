//! Boîte de réception (lot navigation) : mentions récentes (`GET` et
//! `DELETE /users/@me/mentions`, non documentés) face au faux serveur.

use crate::support::*;
use ferrocord::state::inbox::{Filter, InboxEvent};
use ferrocord::state::Event;
use serde_json::json;

const LIST: &str = "/api/v10/users/@me/mentions";
const OLDEST: u64 = 5000000000000000401;
const NEWEST: u64 = 5000000000000000403;

#[tokio::test]
async fn mentions_listed_paged_filtered_and_removed() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", LIST, [Response::ok(fixture("rest_recent_mentions"))]);
    assert!(s.client.store.inbox_open());
    assert!(!s.client.store.inbox_open(), "une seule demande à la fois");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/users/@me/mentions?limit=25&roles=true&everyone=true"));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    assert!(!r.headers.iter().any(|(n, _)| n == "x-super-properties"), "aucun en-tête du client officiel");
    s.client.until("page", |e| matches!(e, Event::Inbox(InboxEvent::Page { .. }))).await;
    let ids: Vec<u64> = s.client.store.inbox.items.iter().map(|m| m.msg.id).collect();
    assert_eq!(ids, [NEWEST, OLDEST]);
    assert!(!s.client.store.inbox.more, "page incomplète : rien avant");
    assert!(!s.client.store.inbox_more());

    // Filtre « ce serveur », sans rôles : la liste repart, paramètres transmis.
    s.http.on("GET", LIST, [Response::ok(json!([]))]);
    let f = Filter { roles: false, everyone: true, guild: Some(2000000000000000001) };
    assert!(s.client.store.inbox_filter(f));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, "/api/v10/users/@me/mentions?limit=25&roles=false&everyone=true&guild_id=2000000000000000001");
    s.client.until("page filtrée", |e| matches!(e, Event::Inbox(InboxEvent::Page { .. }))).await;
    assert!(s.client.store.inbox.items.is_empty());

    // Retour au filtre par défaut puis retrait d'une mention.
    s.http.on("GET", LIST, [Response::ok(fixture("rest_recent_mentions"))]);
    assert!(s.client.store.inbox_filter(Filter::default()));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("page", |e| matches!(e, Event::Inbox(InboxEvent::Page { .. }))).await;
    let del = format!("{LIST}/{NEWEST}");
    // Discord répond 204 ; le faux serveur ne sait pas envoyer de réponse sans corps.
    s.http.on("DELETE", &del, [Response::ok(json!({}))]);
    assert!(s.client.store.inbox_delete(NEWEST));
    assert!(!s.client.store.inbox_delete(NEWEST), "déjà en cours");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", del.as_str()));
    s.client.until("retirée", |e| matches!(e, Event::Inbox(InboxEvent::Deleted { message_id }) if *message_id == NEWEST)).await;
    let ids: Vec<u64> = s.client.store.inbox.items.iter().map(|m| m.msg.id).collect();
    assert_eq!(ids, [OLDEST]);

    // Panne : l'erreur s'affiche, la liste reste.
    s.http.on("DELETE", &format!("{LIST}/{OLDEST}"), [Response::error(500, "panne")]);
    assert!(s.client.store.inbox_delete(OLDEST));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("échec", |e| matches!(e, Event::Inbox(InboxEvent::DeleteFailed { .. }))).await;
    assert_eq!(s.client.store.inbox.items.len(), 1);
}

#[tokio::test]
async fn bot_account_sends_nothing() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    assert!(s.client.store.inbox_open());
    s.client.flush();
    s.client.until("refus local", |e| matches!(e, Event::Inbox(InboxEvent::LoadFailed { .. }))).await;
    assert!(s.client.store.inbox.error.is_some());
    assert!(!s.http.requests().iter().any(|r| r.path.contains("/mentions")), "aucune requête");
}
