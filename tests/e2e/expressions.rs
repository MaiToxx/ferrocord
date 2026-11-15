//! Emojis, autocollants et sons d'un serveur (lot A6) face au faux serveur :
//! routes, corps (JSON et multipart), réponses, événements de la passerelle,
//! refus clairs. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::admin::expressions::{ExprEvent, ExprNotice, Failure, Kind, Op, SoundEmoji, StickerFile};
use ferrocord::state::{Event, Problem};
use serde_json::{json, Value};
use std::sync::Arc;

fn expr(e: &Event) -> Option<&ExprEvent> {
    match e {
        Event::Expressions(x) => Some(x),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

/// Serveur dont je suis propriétaire (toutes les permissions).
fn owned(s: &Setup) -> u64 {
    s.client.store.guilds.iter().find(|g| g.owner_id == Some(ME)).map(|g| g.id).unwrap_or_else(|| panic!("aucun serveur à moi"))
}

#[tokio::test]
async fn emojis_are_listed_created_and_deleted() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let g = owned(&s);
    let base = format!("/api/v10/guilds/{g}/emojis");
    s.http.on("GET", &base, [Response::ok(fixture("rest_guild_emojis"))]);
    assert!(s.client.store.load_expressions(g, Kind::Emoji));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", base.as_str()));
    s.client.until("liste", |e| matches!(expr(e), Some(ExprEvent::Emojis { .. }))).await;
    assert_eq!(s.client.store.emoji_counts(g), (1, 1));
    assert_eq!(s.client.store.expressions.emoji_author(g, 7000000000000000801).map(|a| a.name.as_str()), Some("Alice"));

    s.http.on("POST", &base, [Response::ok(json!({ "id": "7000000000000000810", "name": "chat", "animated": false, "user": { "id": ME.to_string(), "username": "moi" } }))]);
    assert_eq!(s.client.store.create_emoji(g, "chat", "data:image/png;base64,AA==".into(), false, vec![]), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "name": "chat", "image": "data:image/png;base64,AA==", "roles": [] }));
    s.client.until("créé", |e| matches!(expr(e), Some(ExprEvent::Done { .. }))).await;
    assert_eq!(s.client.store.expressions.take_notice(), Some(ExprNotice::Done(Kind::Emoji, Op::Create)));

    let one = format!("{base}/7000000000000000801");
    s.http.on("PATCH", &one, [Response::ok(json!({ "id": "7000000000000000801", "name": "crabe", "roles": [] }))]);
    assert_eq!(s.client.store.edit_emoji(g, 7000000000000000801, "crabe", vec![]), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("PATCH", json!({ "name": "crabe", "roles": [] })));
    s.client.until("modifié", |e| matches!(expr(e), Some(ExprEvent::Done { .. }))).await;

    s.http.on("DELETE", &one, [no_content()]);
    assert_eq!(s.client.store.delete_expression(g, Kind::Emoji, 7000000000000000801), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", one.as_str()));
    s.client.until("supprimé", |e| matches!(expr(e), Some(ExprEvent::Done { .. }))).await;
    // La liste suit la passerelle, pas la réponse.
    s.conn.dispatch("GUILD_EMOJIS_UPDATE", 40, json!({ "guild_id": g.to_string(), "emojis": [] })).await;
    s.client.until("liste à jour", |e| matches!(e, Event::GuildEmojis { .. })).await;
    assert_eq!(s.client.store.emoji_counts(g), (0, 0));
}

#[tokio::test]
async fn sticker_is_sent_as_multipart() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let g = owned(&s);
    let path = format!("/api/v10/guilds/{g}/stickers");
    s.http.on("POST", &path, [Response::ok(json!({ "id": "7000000000000000710", "name": "Salut", "description": "", "tags": "wave", "format_type": 1 }))]);
    let file = StickerFile { name: "salut.png".into(), mime: "image/png", bytes: Arc::from(&b"PNG-FICTIF"[..]) };
    assert_eq!(s.client.store.create_sticker(g, "Salut", "", "wave", file), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    let ct = r.headers.iter().find(|(n, _)| n == "content-type").map(|(_, v)| v.clone()).unwrap_or_default();
    assert!(ct.starts_with("multipart/form-data; boundary="), "{ct}");
    for part in ["name=\"name\"\r\n\r\nSalut\r\n", "name=\"tags\"\r\n\r\nwave\r\n", "name=\"file\"; filename=\"salut.png\"", "Content-Type: image/png", "PNG-FICTIF"] {
        assert!(r.body.contains(part), "{part:?} absent de {}", r.body);
    }
    s.client.until("créé", |e| matches!(expr(e), Some(ExprEvent::Done { .. }))).await;
    assert_eq!(s.client.store.expressions.sticker_info(g, 7000000000000000710).map(|i| i.tags.as_str()), Some("wave"));
}

#[tokio::test]
async fn sounds_follow_the_gateway_and_mfa_refusal_is_clear() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let g = owned(&s);
    let path = format!("/api/v10/guilds/{g}/soundboard-sounds");
    s.http.on("GET", &path, [Response::ok(fixture("rest_guild_soundboard_sounds"))]);
    assert!(s.client.store.load_expressions(g, Kind::Sound));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("sons", |e| matches!(expr(e), Some(ExprEvent::Sounds { .. }))).await;
    assert_eq!(s.client.store.expressions.sounds(g).map(<[_]>::len), Some(2));

    s.http.on("POST", &path, [Response::raw(401, json!({ "message": "Two factor is required for this operation", "code": 60003 }))]);
    let emoji = Some(SoundEmoji::Unicode("🔔".into()));
    assert_eq!(s.client.store.create_sound(g, "ding", "data:audio/mpeg;base64,AA==".into(), 50, emoji), Ok(()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.json(), json!({ "name": "ding", "sound": "data:audio/mpeg;base64,AA==", "volume": 0.5, "emoji_id": null, "emoji_name": "🔔" }));
    s.client.until("refusé", |e| matches!(expr(e), Some(ExprEvent::Failed { .. }))).await;
    assert_eq!(s.client.store.expressions.take_notice(), Some(ExprNotice::Failed(Kind::Sound, Op::Create, Failure::Problem(Problem::MfaRequired))));
    assert_eq!(s.http.requests().iter().filter(|r| r.method == "POST").count(), 1, "jamais retenté");

    // Ajouté ailleurs : la passerelle l'apporte ; supprimé : il part.
    let d = json!({ "guild_id": g.to_string(), "sound_id": "7000000000000000903", "name": "cloche", "volume": 1.0, "emoji_name": null, "available": true });
    s.conn.dispatch("GUILD_SOUNDBOARD_SOUND_CREATE", 41, d).await;
    s.client.until("ajouté", |e| matches!(expr(e), Some(ExprEvent::SoundUpsert { .. }))).await;
    assert_eq!(s.client.store.expressions.sounds(g).map(<[_]>::len), Some(3));
    s.conn.dispatch("GUILD_SOUNDBOARD_SOUND_DELETE", 42, json!({ "guild_id": g.to_string(), "sound_id": "7000000000000000901" })).await;
    s.client.until("retiré", |e| matches!(expr(e), Some(ExprEvent::SoundDelete { .. }))).await;
    assert_eq!(s.client.store.expressions.sounds(g).map(|l| l.iter().map(|s| s.id).collect::<Vec<_>>()), Some(vec![7000000000000000902, 7000000000000000903]));
}
