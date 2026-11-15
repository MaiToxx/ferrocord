//! Zone de saisie : « en train d'écrire » reçu et envoyé, emojis du serveur,
//! mode lent.

use crate::support::{ids::*, *};
use ferrocord::state::{Cmd, Event};
use serde_json::json;

#[tokio::test]
async fn typing_start_is_shown_then_cleared_by_the_message() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Serveur : le surnom vient du membre du paquet.
    s.conn.dispatch_fixture("typing_start_guild", 2).await;
    s.client.until("TYPING_START (serveur)", |e| matches!(e, Event::TypingStart { .. })).await;
    let now = s.client.store.now();
    let names: Vec<Option<String>> = s.client.store.typing.active(GENERAL, now).map(|t| t.name.clone()).collect();
    assert_eq!(names, vec![Some("Alice du serveur".into())]);
    // MP : pas de membre, le nom vient des destinataires du MP.
    s.conn.dispatch_fixture("typing_start_dm", 3).await;
    s.client.until("TYPING_START (MP)", |e| matches!(e, Event::TypingStart { channel_id, .. } if *channel_id == DM_ALICE)).await;
    let names: Vec<Option<String>> = s.client.store.typing.active(DM_ALICE, now).map(|t| t.name.clone()).collect();
    assert_eq!(names, vec![Some("Alice".into())]);
    // Le mien n'est jamais transmis.
    let mut own = fixture("typing_start_guild");
    own["d"]["user_id"] = json!(ME.to_string());
    s.conn.dispatch("TYPING_START", 4, own["d"].clone()).await;
    // Son message arrive : elle n'écrit plus.
    s.conn.dispatch_fixture("message_create_simple", 5).await;
    let seen = s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_) | Event::TypingStart { .. })).await;
    assert!(seen.starts_with("MessageCreate"), "mon propre TYPING_START transmis : {seen}");
    assert_eq!(s.client.store.typing.active(GENERAL, now).count(), 0);
    // Paquet incomplet (sans user_id) : ignoré.
    s.conn.dispatch("TYPING_START", 6, json!({ "channel_id": GENERAL.to_string() })).await;
    s.conn.dispatch_fixture("message_delete", 7).await;
    let seen = s.client.until("MESSAGE_DELETE", |e| matches!(e, Event::MessageDelete { .. } | Event::TypingStart { .. })).await;
    assert!(seen.starts_with("MessageDelete"), "{seen}");
}

#[tokio::test]
async fn own_typing_indicator_is_posted() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/channels/{GENERAL}/typing");
    // Discord répond 204 ; le faux serveur ne sait pas envoyer de réponse sans corps.
    s.http.on("POST", &path, [Response::ok(json!({}))]);
    s.client.send(Cmd::Typing { channel_id: GENERAL });
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", path.as_str()));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    assert!(r.body.is_empty(), "corps inattendu : {}", r.body);
    // Refus : rien n'est signalé à l'utilisateur.
    s.http.on("POST", &path, [Response::error(403, "Missing Permissions")]);
    s.client.send(Cmd::Typing { channel_id: GENERAL });
    s.http.next_request().await;
    s.client.drain();
    assert!(s.client.effects.is_empty(), "{:?}", s.client.effects);
}

#[tokio::test]
async fn guild_emojis_and_slowmode_are_kept() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch_fixture("guild_emojis_update_roles", 2).await;
    s.client.until("GUILD_EMOJIS_UPDATE", |e| matches!(e, Event::GuildEmojis { .. })).await;
    let alpha = s.client.store.guilds.iter().find(|g| g.id == ALPHA).unwrap_or_else(|| panic!("Alpha absent"));
    let names: Vec<(&str, bool, bool)> = alpha.emojis.iter().map(|e| (e.name.as_str(), e.animated, e.available)).collect();
    // Sans identifiant ou au nom illisible : écartés ; les autres gardés tels quels.
    assert_eq!(names, vec![("ferris", false, true), ("danse", true, true), ("ancien", false, false), ("reserve", false, true)]);
    assert_eq!(alpha.emojis[3].roles, vec![2000000000000001999]);

    s.conn.dispatch_fixture("channel_update_slowmode", 3).await;
    s.client.until("CHANNEL_UPDATE", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    let alpha = s.client.store.guilds.iter().find(|g| g.id == ALPHA).unwrap_or_else(|| panic!("Alpha absent"));
    assert_eq!(alpha.channels.iter().find(|c| c.id == GENERAL).map(|c| c.slowmode), Some(30));
    assert_eq!(alpha.channels.iter().find(|c| c.id == HORS_SUJET).map(|c| c.slowmode), Some(0));
    // Permissions d'Alpha inconnues dans ce READY : le mode lent s'applique.
    assert_eq!(s.client.store.slowmode(GENERAL), 30);
}
