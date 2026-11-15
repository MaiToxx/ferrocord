//! Texte mis en forme : un message markdown reçu du faux serveur garde sa
//! source dans le magasin, et sa mise en forme résout mentions, rôle
//! (couleur comprise), salon et horodatages à partir du magasin.

use crate::support::{ids::*, *};
use ferrocord::i18n::Lang;
use ferrocord::state::markdown::{parse_message, BlockKind, Marker, Mention, SpanKind};
use ferrocord::state::Event;
use serde_json::json;

#[tokio::test]
async fn markdown_message_is_kept_as_source_and_formatted() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    s.client.until("historique de #général", |e| matches!(e, Event::History { .. })).await;

    // Rôle coloré (forme de GUILD_ROLE_CREATE, documentée), puis le message.
    let mut role = fixture("guild_role_create");
    role["s"] = json!(20);
    role["d"]["role"]["id"] = json!("2000000000000001009");
    role["d"]["role"]["name"] = json!("Modération");
    role["d"]["role"]["color"] = json!(0x3498db);
    s.conn.send(role).await;
    s.client.until("GUILD_ROLE_CREATE", |e| matches!(e, Event::RoleUpsert { .. })).await;
    s.conn.dispatch_fixture("message_create_markdown", 21).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(m) if m.id == 5000000000000000330)).await;

    let store = &s.client.store;
    let m = store.chans[&GENERAL].msgs.iter().find(|m| m.id == 5000000000000000330).unwrap_or_else(|| panic!("message absent"));
    let source = fixture("message_create_markdown")["d"]["content"].as_str().unwrap_or_default().to_string();
    assert_eq!(m.content, source, "la source est gardée telle quelle (copie, modification)");

    let doc = parse_message(store, m, Lang::Fr, true);
    let kinds: Vec<&BlockKind> = doc.blocks.iter().map(|b| &b.kind).collect();
    assert_eq!(kinds[0], &BlockKind::Heading(1));
    assert!(doc.blocks[2].quote, "{kinds:?}");
    assert!(kinds.contains(&&BlockKind::Item { depth: 1, marker: Marker::Bullet }), "{kinds:?}");
    assert!(kinds.contains(&&BlockKind::Item { depth: 0, marker: Marker::Number(2) }), "{kinds:?}");
    assert!(kinds.contains(&&BlockKind::Code { lang: Some("rust".into()), spoiler: None }), "{kinds:?}");
    assert_eq!(kinds.last(), Some(&&BlockKind::Subtext));

    let spans: Vec<(&str, &SpanKind)> = doc.blocks.iter().flat_map(|b| b.spans.iter().map(|s| (s.text.as_str(), &s.kind))).collect();
    let has = |text: &str, kind: SpanKind| spans.iter().any(|(t, k)| *t == text && **k == kind);
    assert!(has("@Moi Fictif", SpanKind::Mention(Mention::User(ME))), "{spans:?}");
    assert!(has("@Modération", SpanKind::Mention(Mention::Role { id: 2000000000000001009, color: 0x3498db })), "{spans:?}");
    assert!(has("#général", SpanKind::Mention(Mention::Channel(GENERAL))), "{spans:?}");
    assert!(has("@here", SpanKind::Mention(Mention::Here)), "{spans:?}");
    assert!(has(":ferris:", SpanKind::Emoji { id: 4000000000000000001, name: "ferris".into(), animated: false }), "{spans:?}");
    assert!(spans.iter().any(|(t, k)| matches!(k, SpanKind::Time(1736935200)) && t.contains("2025")), "{spans:?}");
    assert_eq!(doc.links, vec!["https://exemple.invalid/notes".to_string(), "https://exemple.invalid/sans-apercu".to_string()]);
    assert_eq!(doc.spoilers, 1);
    let last: String = doc.blocks.last().map(|b| b.spans.iter().map(|s| s.text.as_str()).collect()).unwrap_or_default();
    assert_eq!(last, "Petit texte en bas, *pas en italique*");
}
