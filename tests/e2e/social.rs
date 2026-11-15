//! Réactions, réponses, transferts, modification, suppression et épingles
//! (lots T5/T6) : paquets de la passerelle et requêtes REST, face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::social::{Emoji, ReplyState, SocialEvent};
use ferrocord::state::store::social::Reactors;
use ferrocord::state::{Effect, Event};
use serde_json::{json, Value};

const BOB_MSG: u64 = 5000000000000000201;
const MY_MSG: u64 = 5000000000000000202;
const REPLY: u64 = 5000000000000000320;
const FORWARD: u64 = 5000000000000000321;
const FERRIS: u64 = 6000000000000000501;

fn thumbs() -> Emoji {
    Emoji::Unicode("👍".into())
}

fn ferris() -> Emoji {
    Emoji::Custom { id: FERRIS, name: "ferris".into(), animated: false }
}

/// Réponse vide de Discord (204), comme pour PUT/DELETE d'une réaction.
fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

/// Connecté, #général ouvert avec la page `rest_history` (Bob, moi, Alice).
async fn general_open() -> Setup {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "GET");
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;
    s
}

fn path(rest: &str) -> String {
    format!("/api/v10/channels/{GENERAL}/messages/{rest}")
}

/// (normales, super, à moi) pour cet emoji, si présent.
fn counts(s: &Setup, id: u64, emoji: &Emoji) -> Option<(u32, u32, bool)> {
    let m = s.client.store.find_message(GENERAL, id)?;
    m.meta.reactions.iter().find(|r| r.emoji.same(emoji)).map(|r| (r.normal, r.burst, r.me))
}

fn is_reaction(e: &Event) -> bool {
    matches!(
        e,
        Event::Social(SocialEvent::Reaction { .. } | SocialEvent::ReactionRemoveAll { .. } | SocialEvent::ReactionRemoveEmoji { .. })
    )
}

#[tokio::test]
async fn gateway_reply_forward_and_reactions() {
    let mut s = general_open().await;
    s.conn.dispatch_fixture("message_create_reply", 30).await;
    s.client.until("réponse", |e| matches!(e, Event::MessageCreate(m) if m.id == REPLY)).await;
    let m = s.client.store.find_message(GENERAL, REPLY).cloned().unwrap_or_else(|| panic!("réponse absente"));
    let r = m.meta.reply.as_ref().unwrap_or_else(|| panic!("référence absente"));
    assert_eq!(r.message_id, Some(5000000000000000300));
    match &r.state {
        ReplyState::Loaded { author_id, author, excerpt } => {
            assert_eq!((*author_id, author.as_str(), excerpt.as_str()), (ALICE, "Alice", "Bonjour tout le monde"));
        }
        other => panic!("état inattendu : {other:?}"),
    }
    let party = Emoji::Unicode("🎉".into());
    assert_eq!(counts(&s, REPLY, &party), Some((2, 1, false)));
    assert_eq!(counts(&s, REPLY, &ferris()), Some((1, 0, true)));

    s.conn.dispatch_fixture("message_create_forward", 31).await;
    s.client.until("transfert", |e| matches!(e, Event::MessageCreate(m) if m.id == FORWARD)).await;
    let m = s.client.store.find_message(GENERAL, FORWARD).cloned().unwrap_or_else(|| panic!("transfert absent"));
    let f = m.meta.forward.as_ref().unwrap_or_else(|| panic!("instantané absent"));
    assert!(f.content.starts_with("Réunion demain"), "{}", f.content);
    assert_eq!(f.attachments, vec!["ordre-du-jour.txt".to_string()]);
    assert_eq!((f.channel_id, f.message_id), (Some(HORS_SUJET), Some(5000000000000000150)));
    assert!(m.meta.pinned);

    // Réaction d'Alice (nouvel emoji), puis super-réaction d'Ève sur ferris.
    s.conn.dispatch_fixture("message_reaction_add", 32).await;
    s.client.until("ajout", is_reaction).await;
    assert_eq!(counts(&s, REPLY, &thumbs()), Some((1, 0, false)));
    s.conn.dispatch_fixture("message_reaction_add_custom", 33).await;
    s.client.until("ajout personnalisé", is_reaction).await;
    assert_eq!(counts(&s, REPLY, &ferris()), Some((1, 1, true)));
    s.conn.dispatch_fixture("message_reaction_remove", 34).await;
    s.client.until("retrait", is_reaction).await;
    assert_eq!(counts(&s, REPLY, &thumbs()), None, "compte à zéro : la pastille disparaît");
    s.conn.dispatch_fixture("message_reaction_remove_emoji", 35).await;
    s.client.until("retrait d'un emoji", is_reaction).await;
    assert_eq!(counts(&s, REPLY, &party), None);
    assert!(counts(&s, REPLY, &ferris()).is_some());
    s.conn.dispatch_fixture("message_reaction_remove_all", 36).await;
    s.client.until("retrait de tout", is_reaction).await;
    assert!(s.client.store.find_message(GENERAL, REPLY).is_some_and(|m| m.meta.reactions.is_empty()));

    s.conn.dispatch_fixture("guild_emojis_update", 37).await;
    s.client.until("emojis", |e| matches!(e, Event::Social(SocialEvent::GuildEmojis { .. }))).await;
    let names: Vec<&str> = s.client.store.guild_emojis(ALPHA).iter().map(|e| e.name.as_str()).collect();
    assert!(names.contains(&"ferris") && names.contains(&"chat_qui_danse"), "{names:?}");

    s.conn.dispatch_fixture("channel_pins_update", 38).await;
    s.client.until("épingles", |e| matches!(e, Event::Social(SocialEvent::PinsChanged { channel_id } ) if *channel_id == GENERAL)).await;
}

#[tokio::test]
async fn reaction_toggle_paths_and_rollback() {
    let mut s = general_open().await;
    let thumbs_path = path(&format!("{BOB_MSG}/reactions/%F0%9F%91%8D/@me"));
    s.http.on("PUT", &thumbs_path, [no_content()]);
    assert!(s.client.store.toggle_reaction(GENERAL, BOB_MSG, thumbs()));
    // Mise à jour immédiate, avant la réponse ; un second clic attend.
    assert_eq!(counts(&s, BOB_MSG, &thumbs()), Some((1, 0, true)));
    assert!(!s.client.store.toggle_reaction(GENERAL, BOB_MSG, thumbs()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.body.as_str()), ("PUT", thumbs_path.as_str(), ""));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    s.client.until("ajout confirmé", |e| matches!(e, Event::Social(SocialEvent::ReactDone { error: None, .. }))).await;
    assert_eq!(counts(&s, BOB_MSG, &thumbs()), Some((1, 0, true)));

    // Retrait refusé : la réaction revient.
    s.http.on("DELETE", &thumbs_path, [Response::error(403, "Missing Permissions")]);
    assert!(s.client.store.toggle_reaction(GENERAL, BOB_MSG, thumbs()));
    assert_eq!(counts(&s, BOB_MSG, &thumbs()), None);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("DELETE", thumbs_path.as_str()));
    s.client.until("retrait refusé", |e| matches!(e, Event::Social(SocialEvent::ReactDone { error: Some(_), .. }))).await;
    assert_eq!(counts(&s, BOB_MSG, &thumbs()), Some((1, 0, true)), "rétabli après le refus");
    assert!(s.client.effects.iter().any(|e| matches!(e, Effect::Toast(_))), "erreur signalée");

    // Emoji du serveur : `nom:id`, sans encodage du deux-points.
    let custom = path(&format!("{MY_MSG}/reactions/ferris:{FERRIS}/@me"));
    s.http.on("PUT", &custom, [no_content()]);
    assert!(s.client.store.add_reaction(GENERAL, MY_MSG, ferris()));
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, custom);
    s.client.until("ajout personnalisé", |e| matches!(e, Event::Social(SocialEvent::ReactDone { .. }))).await;
    assert!(!s.client.store.add_reaction(GENERAL, MY_MSG, ferris()), "déjà mise : rien ne part");
}

#[tokio::test]
async fn reactors_loaded_once() {
    let mut s = general_open().await;
    let p = path(&format!("{REPLY}/reactions/ferris:{FERRIS}"));
    s.conn.dispatch_fixture("message_create_reply", 30).await;
    s.client.until("réponse", |e| matches!(e, Event::MessageCreate(_))).await;
    s.http.on("GET", &p, [Response::ok(fixture("rest_reactors"))]);
    assert!(s.client.store.request_reactors(GENERAL, REPLY, &ferris(), false));
    assert!(!s.client.store.request_reactors(GENERAL, REPLY, &ferris(), false), "déjà demandée");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "GET");
    assert_eq!(r.path, format!("{p}?limit=25&type=0"));
    s.client.until("liste", |e| matches!(e, Event::Social(SocialEvent::Reactors { .. }))).await;
    match s.client.store.reactors(REPLY, &ferris(), false) {
        Some(Reactors::Loaded(list)) => {
            let names: Vec<&str> = list.iter().map(|r| r.name.as_str()).collect();
            assert_eq!(names, ["Alice", "eve_fictive"]);
        }
        other => panic!("liste inattendue : {other:?}"),
    }
}

#[tokio::test]
async fn edit_and_delete() {
    let mut s = general_open().await;
    let mine = path(&MY_MSG.to_string());
    let mut edited = fixture("rest_history")[1].clone();
    edited["content"] = json!("Deuxième message corrigé");
    s.http.on("PATCH", &mine, [Response::ok(edited)]);
    assert!(!s.client.store.edit_message(GENERAL, BOB_MSG, "pas à moi".into()), "message d'un autre");
    assert!(!s.client.store.edit_message(GENERAL, MY_MSG, "  ".into()), "texte vide");
    assert!(s.client.store.edit_message(GENERAL, MY_MSG, "Deuxième message corrigé".into()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", mine.as_str()));
    assert_eq!(r.json(), json!({ "content": "Deuxième message corrigé" }));
    s.client.until("modifié", |e| matches!(e, Event::MessageUpdate { id, .. } if *id == MY_MSG)).await;
    assert_eq!(s.client.store.raw_content(GENERAL, MY_MSG).as_deref(), Some("Deuxième message corrigé"));

    // Suppression refusée, puis acceptée.
    s.http.on("DELETE", &mine, [Response::error(403, "Missing Permissions"), no_content()]);
    assert!(s.client.store.delete_message(GENERAL, MY_MSG));
    assert!(!s.client.store.delete_message(GENERAL, MY_MSG), "déjà en cours");
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "DELETE");
    s.client.until("refus", |e| matches!(e, Event::Social(SocialEvent::DeleteFailed { .. }))).await;
    assert!(s.client.store.find_message(GENERAL, MY_MSG).is_some());
    assert!(s.client.store.delete_message(GENERAL, MY_MSG));
    s.client.flush();
    assert_eq!(s.http.next_request().await.path, mine);
    s.client.until("supprimé", |e| matches!(e, Event::MessageDelete { id, .. } if *id == MY_MSG)).await;
    assert!(s.client.store.find_message(GENERAL, MY_MSG).is_none());
    assert!(!s.client.store.is_deleting(MY_MSG));
}

#[tokio::test]
async fn pins_put_delete_and_pages() {
    let mut s = general_open().await;
    let pin = format!("/api/v10/channels/{GENERAL}/messages/pins/{BOB_MSG}");
    s.http.on("PUT", &pin, [no_content()]);
    s.http.on("DELETE", &pin, [no_content()]);
    assert!(s.client.store.set_pinned(GENERAL, BOB_MSG, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PUT", pin.as_str()));
    s.client.until("épinglé", |e| matches!(e, Event::Social(SocialEvent::Pinned { pinned: true, .. }))).await;
    assert!(s.client.store.find_message(GENERAL, BOB_MSG).is_some_and(|m| m.meta.pinned));

    let list = format!("/api/v10/channels/{GENERAL}/messages/pins");
    let mut last = fixture("rest_pins");
    last["has_more"] = json!(false);
    last["items"] = json!([last["items"][1].clone()]);
    s.http.on("GET", &list, [Response::ok(fixture("rest_pins")), Response::ok(last)]);
    assert!(s.client.store.load_pins(GENERAL, false));
    assert!(!s.client.store.load_pins(GENERAL, false), "déjà en cours");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("{list}?limit=50"));
    s.client.until("page", |e| matches!(e, Event::Social(SocialEvent::Pins { .. }))).await;
    let l = s.client.store.pins(GENERAL).unwrap_or_else(|| panic!("liste absente"));
    assert_eq!((l.items.len(), l.more, l.loaded), (2, true, true));
    assert_eq!(l.items[1].message.content, "Règles du salon : soyez aimables");

    assert!(s.client.store.load_pins(GENERAL, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("{list}?limit=50&before=2025-01-10T08%3A00%3A00.000000%2B00%3A00"));
    s.client.until("page suivante", |e| matches!(e, Event::Social(SocialEvent::Pins { before: Some(_), .. }))).await;
    let l = s.client.store.pins(GENERAL).unwrap_or_else(|| panic!("liste absente"));
    assert_eq!((l.items.len(), l.more), (3, false));

    assert!(s.client.store.set_pinned(GENERAL, BOB_MSG, false));
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "DELETE");
    s.client.until("désépinglé", |e| matches!(e, Event::Social(SocialEvent::Pinned { pinned: false, .. }))).await;
    assert!(s.client.store.find_message(GENERAL, BOB_MSG).is_some_and(|m| !m.meta.pinned));
}

#[tokio::test]
async fn reply_and_forward_bodies() {
    let mut s = general_open().await;
    let created = fixture("message_create_simple")["d"].clone();
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(created.clone())]);
    s.http.on("POST", &messages_path(HORS_SUJET), [Response::ok(created)]);

    assert!(s.client.store.send_reply(GENERAL, "D'accord".into(), BOB_MSG, false));
    s.client.flush();
    let b = s.http.next_request().await.json();
    assert_eq!(b["content"], "D'accord");
    assert_eq!(b["message_reference"], json!({ "type": 0, "message_id": BOB_MSG.to_string(), "fail_if_not_exists": false }));
    assert_eq!(b["allowed_mentions"]["replied_user"], false);
    assert_eq!(b["allowed_mentions"]["parse"], json!(["users", "roles", "everyone"]));
    assert!(b["nonce"].is_string() && b["enforce_nonce"] == true);
    s.client.until("réponse partie", |e| matches!(e, Event::SendConfirmed { .. })).await;

    assert!(s.client.store.send_reply(GENERAL, "Oui".into(), BOB_MSG, true));
    s.client.flush();
    assert_eq!(s.http.next_request().await.json()["allowed_mentions"]["replied_user"], true);
    s.client.until("réponse partie", |e| matches!(e, Event::SendConfirmed { .. })).await;

    assert!(s.client.store.forward(HORS_SUJET, GENERAL, BOB_MSG, "transfert".into()));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, messages_path(HORS_SUJET));
    let b = r.json();
    assert!(b.get("content").is_none(), "un transfert n'a pas de texte : {b}");
    assert_eq!(
        b["message_reference"],
        json!({ "type": 1, "message_id": BOB_MSG.to_string(), "channel_id": GENERAL.to_string(), "guild_id": ALPHA.to_string() })
    );
}

/// « Supprimer les intégrations » (lot M) : un PATCH qui ne porte que `flags`.
#[tokio::test]
async fn suppress_embeds_patches_only_the_flags() {
    const EMBED: u64 = 5000000000000000302;
    let mut s = general_open().await;
    s.conn.dispatch_fixture("message_create_embed", 12).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    let p = path(&EMBED.to_string());
    s.http.on("PATCH", &p, [Response::ok(json!({ "id": EMBED.to_string(), "flags": 4 }))]);
    assert!(s.client.store.suppress_embeds(GENERAL, EMBED));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("PATCH", p.as_str()));
    assert_eq!(r.json(), json!({ "flags": 4 }));
    s.client.until("retirées", |e| matches!(e, Event::Social(SocialEvent::EmbedsSuppressed { .. }))).await;
    let m = s.client.store.find_message(GENERAL, EMBED).unwrap_or_else(|| panic!("message absent"));
    assert!(m.embeds.is_empty() && m.meta.flags & 4 != 0);
    assert!(!s.client.store.suppress_embeds(GENERAL, EMBED), "déjà retirées");
}
