//! Fils et forums (lot T10) : paquets de la passerelle et requêtes REST,
//! face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::threads::ThreadEvent;
use ferrocord::state::Event;
use serde_json::{json, Value};

const THREAD: u64 = 5000000000000000777;

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

fn thread_json(archived: bool) -> Value {
    json!({ "id": THREAD.to_string(), "type": 11, "guild_id": ALPHA.to_string(), "parent_id": GENERAL.to_string(), "name": "Sujet",
        "owner_id": ME.to_string(), "message_count": 2, "member_count": 1,
        "thread_metadata": { "archived": archived, "auto_archive_duration": 1440, "archive_timestamp": "2026-11-10T10:00:00+00:00", "locked": false } })
}

#[tokio::test]
async fn thread_events_join_leave_and_archive() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch("THREAD_CREATE", 20, thread_json(false)).await;
    s.client.until("fil créé", |e| matches!(e, Event::Threads(ThreadEvent::Upsert { .. }))).await;
    assert!(s.client.store.channel_exists(THREAD));
    assert_eq!(s.client.store.threads.children(GENERAL), &[THREAD]);

    let member = format!("/api/v10/channels/{THREAD}/thread-members/@me");
    s.http.on("PUT", &member, [no_content()]);
    s.http.on("DELETE", &member, [no_content()]);
    assert!(s.client.store.set_thread_joined(THREAD, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", member.as_str()));
    s.client.until("rejoint", |e| matches!(e, Event::Threads(ThreadEvent::Joined { joined: true, .. }))).await;
    assert!(s.client.store.threads.get(THREAD).is_some_and(|t| t.joined));
    assert!(s.client.store.set_thread_joined(THREAD, false));
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "DELETE");
    s.client.until("quitté", |e| matches!(e, Event::Threads(ThreadEvent::Joined { joined: false, .. }))).await;

    // Archivage (je suis le créateur) : PATCH, le fil renvoyé remplace l'ancien.
    let chan = format!("/api/v10/channels/{THREAD}");
    s.http.on("PATCH", &chan, [Response::ok(thread_json(true))]);
    assert!(s.client.store.set_thread_archived(THREAD, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", chan.as_str(), json!({ "archived": true })));
    s.client.until("archivé", |e| matches!(e, Event::Threads(ThreadEvent::Upsert { thread, .. }) if thread.archived)).await;
    assert!(s.client.store.threads.get(THREAD).is_some_and(|t| t.archived));

    s.conn.dispatch("THREAD_DELETE", 21, json!({ "id": THREAD.to_string(), "guild_id": ALPHA.to_string(), "parent_id": GENERAL.to_string(), "type": 11 })).await;
    s.client.until("supprimé", |e| matches!(e, Event::Threads(ThreadEvent::Delete { .. }))).await;
    assert!(!s.client.store.channel_exists(THREAD));
}

#[tokio::test]
async fn create_thread_from_message_and_archived_page() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let msg = 5000000000000000201u64;
    let path = format!("/api/v10/channels/{GENERAL}/messages/{msg}/threads");
    let mut created = thread_json(false);
    created["id"] = msg.to_string().into();
    s.http.on("POST", &path, [Response::ok(created)]);
    assert!(s.client.store.create_thread(GENERAL, msg, "Sujet"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("POST", path.as_str(), json!({ "name": "Sujet" })));
    s.client.until("ouvert", |e| matches!(e, Event::Threads(ThreadEvent::Created { .. }))).await;
    assert_eq!(s.client.store.channel, Some(msg), "le fil créé est ouvert");
    assert!(s.client.store.threads.get(msg).is_some_and(|t| t.joined));

    let archived = format!("/api/v10/channels/{GENERAL}/threads/archived/public");
    let mut page = fixture("rest_threads_archived_public");
    page["threads"][0]["parent_id"] = GENERAL.to_string().into();
    s.http.on("GET", &archived, [Response::ok(page)]);
    assert!(s.client.store.load_archived_threads(GENERAL));
    assert!(!s.client.store.load_archived_threads(GENERAL), "déjà en cours");
    s.client.flush();
    // Le premier GET est celui de l'historique du fil ouvert ; on cherche celui des archives.
    let mut seen = Vec::new();
    loop {
        let r = s.http.next_request().await;
        if r.path.starts_with(&archived) {
            assert_eq!(r.path, format!("{archived}?limit=25"));
            break;
        }
        seen.push(r.path);
        assert!(seen.len() < 4, "{seen:?}");
    }
    s.client.until("archives", |e| matches!(e, Event::Threads(ThreadEvent::Archived { .. }))).await;
    assert!(s.client.store.threads.get(5000000000000000720).is_some_and(|t| t.archived && t.joined));
}
