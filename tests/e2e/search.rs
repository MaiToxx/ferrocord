//! Recherche de messages et saut vers un message (lot T12), face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::search::{parse, Names, Phase, Scope, SearchEvent, Sort};
use ferrocord::state::store::Jump;
use ferrocord::state::Event;

fn start(s: &mut Setup, text: &str) {
    let parsed = parse(text, &Names::default(), 0, Sort::Newest).unwrap_or_else(|e| panic!("{e:?}"));
    s.client.store.start_search(Scope::Guild(ALPHA), text.into(), parsed);
    s.client.flush();
}

#[tokio::test]
async fn index_in_progress_is_retried_once_then_results_arrive() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{ALPHA}/messages/search");
    s.http.on("GET", &path, [Response::raw(202, fixture("rest_search_indexing")), Response::ok(fixture("rest_search_messages"))]);
    start(&mut s, "rendez-vous has:image");
    let want = format!("{path}?content=rendez-vous&has=image&sort_by=timestamp&sort_order=desc&limit=25");
    let first = s.http.next_request().await;
    assert_eq!((first.method.as_str(), first.path.as_str()), ("GET", want.as_str()));
    assert_eq!(first.authorization.as_deref(), Some(USER_TOKEN));
    s.client.until("index en cours", |e| matches!(e, Event::Search(SearchEvent::Indexing { retry_ms: 2000, .. }))).await;
    assert_eq!(s.client.store.search.active.as_ref().map(|a| a.phase.clone()), Some(Phase::Indexing));
    let second = s.http.next_request().await;
    assert_eq!(second.path, want);
    // Le nouvel essai attend le délai indiqué par Discord (2 s).
    assert!(second.received_at.duration_since(first.received_at) >= std::time::Duration::from_millis(1900));
    s.client.until("résultats", |e| matches!(e, Event::Search(SearchEvent::Results { .. }))).await;
    let a = s.client.store.search.active.as_ref().expect("recherche affichée");
    assert_eq!((a.phase.clone(), a.total, a.hits.len()), (Phase::Done, Some(57), 3));
    assert_eq!(a.hits[1].thread_name.as_deref(), Some("Sujet"));
    // Page suivante : `offset`.
    s.http.on("GET", &path, [Response::ok(fixture("rest_search_messages"))]);
    assert!(s.client.store.search_page(1));
    s.client.flush();
    assert!(s.http.next_request().await.path.contains("&offset=25&limit=25"));
}

#[tokio::test]
async fn still_not_indexed_after_one_retry_stops() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{ALPHA}/messages/search");
    let indexing = || Response::raw(202, serde_json::json!({ "message": "Index not yet available. Try again later", "code": 110000, "documents_indexed": 0, "retry_after": 0.2 }));
    s.http.on("GET", &path, [indexing(), indexing(), indexing()]);
    start(&mut s, "bonjour");
    s.client.until("pas indexé", |e| matches!(e, Event::Search(SearchEvent::NotIndexed { .. }))).await;
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    let sent = s.http.requests().iter().filter(|r| r.path.starts_with(&path)).count();
    assert_eq!(sent, 2, "un seul nouvel essai");
    assert_eq!(s.client.store.search.active.as_ref().map(|a| a.phase.clone()), Some(Phase::NotIndexed));
}

#[tokio::test]
async fn bot_accounts_never_search() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    start(&mut s, "bonjour");
    s.client.until("refus local", |e| matches!(e, Event::Search(SearchEvent::Failed { .. }))).await;
    assert!(!s.http.requests().iter().any(|r| r.path.contains("/search")));
}

#[tokio::test]
async fn jump_loads_the_history_around_the_message() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    let target = 5000000000000000202u64;
    assert_eq!(s.client.store.jump_to_message(GENERAL, target), Jump::Loading);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("{}?around={target}&limit=50", messages_path(GENERAL)));
    s.client.until("page autour", |e| matches!(e, Event::Search(SearchEvent::Around { .. }))).await;
    let store = &s.client.store;
    assert_eq!((store.guild, store.channel), (Some(ALPHA), Some(GENERAL)));
    let cs = &store.chans[&GENERAL];
    assert!(cs.contains(target) && cs.loaded && !cs.loading && cs.more && cs.newer);
    assert!(!store.jump_pending(GENERAL));
}
