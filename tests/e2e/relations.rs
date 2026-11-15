//! Amis, MP de groupe et accusés de lecture : READY utilisateur, événements
//! de la passerelle, requêtes REST (formes, limiteur, captcha).

use crate::support::{ids::*, *};
use ferrocord::state::relations::{RelKind, RelationsAction, RelationsCmd, RelationsEvent, RelationsNotice};
use ferrocord::state::{Cmd, Event};
use serde_json::{json, Value};

const BOB: u64 = 1000000000000000003;
const CAROL: u64 = 1000000000000000005;
const DAN: u64 = 1000000000000000006;

/// Réponse vide (204), comme Discord pour la plupart des écritures.
fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

fn social(e: &Event) -> Option<&RelationsEvent> {
    if let Event::Relations(s) = e { Some(s) } else { None }
}

#[tokio::test]
async fn ready_relations_and_read_states() {
    let mut s = connected(USER_TOKEN, "ready_user_social").await;
    let store = &s.client.store;
    assert_eq!(store.relations.relation(ALICE).map(|r| r.kind), Some(RelKind::Friend));
    assert_eq!(store.relations.display_name(BOB), "Bobby");
    assert_eq!(store.relations.display_name(DAN), "Dan", "personne tirée de `users`");
    assert_eq!(store.relations.incoming(), 1);
    assert_eq!(store.relations.dm_with(ALICE), Some(DM_ALICE));
    // Non-lus de Discord : #général (mention), pas le MP d'Alice (déjà lu).
    assert!(store.unread().contains(&GENERAL) && store.mentioned().contains(&GENERAL));
    assert!(!store.unread().contains(&DM_ALICE));
    assert!(store.unread().contains(&DM_GROUP), "groupe jamais lu");

    // Présences des amis, puis une demande acceptée ailleurs.
    s.conn.dispatch_fixture("ready_supplemental", 2).await;
    s.conn.dispatch_fixture("relationship_add", 3).await;
    s.client.until("RELATIONSHIP_ADD", |e| matches!(social(e), Some(RelationsEvent::RelationAdd(_)))).await;
    // Présences des amis (traitées avant RELATIONSHIP_ADD) : un seul magasin, `state::people`, que lit aussi la page Amis.
    assert_eq!(s.client.store.people.lock().peek_status(ALICE), Some(ferrocord::state::people::Status::Online));
    assert!(s.client.store.friend_list(ferrocord::state::relations::FriendTab::Online, "").contains(&ALICE));
    assert_eq!(s.client.store.relations.relation(CAROL).map(|r| r.kind), Some(RelKind::Friend));
    s.conn.dispatch_fixture("relationship_remove", 4).await;
    s.client.until("RELATIONSHIP_REMOVE", |e| matches!(social(e), Some(RelationsEvent::RelationRemove { .. }))).await;
    assert!(s.client.store.relations.relation(DAN).is_none());

    // Lu sur un autre appareil : le non-lu disparaît ici aussi.
    s.conn.dispatch_fixture("message_ack", 5).await;
    s.client.until("MESSAGE_ACK", |e| matches!(social(e), Some(RelationsEvent::Ack { .. }))).await;
    assert!(!s.client.store.unread().contains(&GENERAL) && !s.client.store.mentioned().contains(&GENERAL));

    // Membres du groupe.
    s.conn.dispatch_fixture("channel_recipient_add", 6).await;
    s.client.until("CHANNEL_RECIPIENT_ADD", |e| matches!(social(e), Some(RelationsEvent::RecipientAdd { .. }))).await;
    s.conn.dispatch_fixture("channel_recipient_remove", 7).await;
    s.client.until("CHANNEL_RECIPIENT_REMOVE", |e| matches!(social(e), Some(RelationsEvent::RecipientRemove { .. }))).await;
    assert_eq!(s.client.store.relations.dm_info(DM_GROUP).map(|d| d.recipients.clone()), Some(vec![BOB, CAROL]));
}

#[tokio::test]
async fn mark_read_sends_one_ack_through_the_limiter() {
    let mut s = connected(USER_TOKEN, "ready_user_social").await;
    let path = format!("/api/v10/channels/{GENERAL}/messages/5000000000000000203/ack");
    s.http.on("POST", &path, [Response::ok(json!({ "token": null }))]);
    s.client.store.mark_read(GENERAL);
    s.client.flush();
    let req = s.http.next_request().await;
    assert_eq!((req.method.as_str(), req.path.as_str()), ("POST", path.as_str()));
    assert_eq!(req.authorization.as_deref(), Some(USER_TOKEN));
    let body = req.json();
    assert_eq!(body["token"], Value::Null);
    assert!(body["last_viewed"].as_u64().is_some_and(|d| d > 4000), "{body}");
    // Aucun en-tête propre au client officiel.
    assert!(!req.headers.iter().any(|(n, _)| n.starts_with("x-super-properties") || n.starts_with("x-context-properties")));
    assert!(!s.client.store.unread().contains(&GENERAL));
    // Déjà accusé : rien ne repart.
    s.client.store.mark_read(GENERAL);
    assert!(s.client.store.take_commands().is_empty());

    // Serveur entier : un seul accusé groupé (le MP n'en fait pas partie).
    s.http.on("POST", "/api/v10/read-states/ack-bulk", [no_content()]);
    s.conn.dispatch_fixture("message_create_simple", 2).await;
    s.client.until("MESSAGE_CREATE", |e| matches!(e, Event::MessageCreate(_))).await;
    assert!(s.client.store.unread().contains(&GENERAL));
    s.client.store.mark_guild_read(ALPHA);
    s.client.flush();
    let req = s.http.next_request().await;
    assert_eq!(req.path, "/api/v10/read-states/ack-bulk");
    assert_eq!(req.json(), json!({ "read_states": [ { "channel_id": GENERAL.to_string(), "message_id": "5000000000000000300", "read_state_type": 0 } ] }));
}

#[tokio::test]
async fn friend_request_with_captcha_is_not_bypassed() {
    let mut s = connected(USER_TOKEN, "ready_user_social").await;
    s.http.on("POST", "/api/v10/users/@me/relationships", [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "cle-fictive", "captcha_service": "hcaptcha" }))]);
    s.client.send(Cmd::Relations(RelationsCmd::AddFriend { username: "carol_fictive".into(), note: None }));
    s.client.until("captcha", |e| matches!(social(e), Some(RelationsEvent::Captcha { .. }))).await;
    assert_eq!(s.client.store.relations.take_notice(), Some(RelationsNotice::Captcha { action: RelationsAction::AddFriend }));
    // Une seule requête : aucun nouvel essai, aucun contournement.
    let sent: Vec<_> = s.http.requests().into_iter().filter(|r| r.path.ends_with("/relationships")).collect();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].json(), json!({ "username": "carol_fictive" }));

    // Sans captcha : 204, la demande est annoncée.
    s.http.on("POST", "/api/v10/users/@me/relationships", [no_content()]);
    s.client.send(Cmd::Relations(RelationsCmd::AddFriend { username: "carol_fictive".into(), note: None }));
    s.client.until("demande envoyée", |e| matches!(social(e), Some(RelationsEvent::RequestSent { .. }))).await;

    // Refus de Discord : message rendu à l'utilisateur.
    let block = format!("/api/v10/users/@me/relationships/{CAROL}");
    s.http.on("PUT", &block, [Response::error(403, "Missing Access")]);
    s.client.send(Cmd::Relations(RelationsCmd::Block { user_id: CAROL }));
    s.client.until("refus", |e| matches!(social(e), Some(RelationsEvent::Failed { .. }))).await;
    let req = s.http.requests().into_iter().find(|r| r.path == block).unwrap_or_else(|| panic!("PUT attendu"));
    assert_eq!((req.method.as_str(), req.json()), ("PUT", json!({ "type": 2 })));
}

#[tokio::test]
async fn message_button_opens_or_creates_the_dm() {
    let mut s = connected(USER_TOKEN, "ready_user_social").await;
    // MP existant : ouvert sans requête.
    assert!(s.client.store.open_dm_with(ALICE));
    assert_eq!(s.client.store.channel, Some(DM_ALICE));
    s.client.store.take_commands();

    // Pas de MP avec Carol : Create DM, puis ouverture.
    let created = json!({ "id": "4000000000000000007", "type": 1, "last_message_id": null, "recipients": [ { "id": CAROL.to_string(), "username": "carol_fictive", "global_name": "Carol" } ] });
    s.http.on("POST", "/api/v10/users/@me/channels", [Response::ok(created)]);
    assert!(!s.client.store.open_dm_with(CAROL));
    s.client.flush();
    s.client.until("MP ouvert", |e| matches!(social(e), Some(RelationsEvent::DmOpened { .. }))).await;
    assert_eq!(s.client.store.channel, Some(4000000000000000007));
    assert!(s.client.store.dms.iter().any(|d| d.id == 4000000000000000007 && d.name == "Carol"));
    assert_eq!(s.client.store.relations.dm_with(CAROL), Some(4000000000000000007));
    let req = s.http.requests().into_iter().find(|r| r.path == "/api/v10/users/@me/channels").unwrap_or_else(|| panic!("POST attendu"));
    assert_eq!(req.json(), json!({ "recipients": [CAROL.to_string()] }));

    // Quitter le groupe : DELETE du salon ; la passerelle confirme.
    let close = format!("/api/v10/channels/{DM_GROUP}");
    s.http.on("DELETE", &close, [Response::ok(json!({ "id": DM_GROUP.to_string(), "type": 3 }))]);
    s.client.send(Cmd::Relations(RelationsCmd::Close { channel_id: DM_GROUP }));
    // File d'attente : les requêtes déjà vues plus haut passent d'abord.
    let req = loop {
        let r = s.http.next_request().await;
        if r.method == "DELETE" {
            break r;
        }
    };
    assert_eq!(req.path, close);
}

/// « Marquer comme non lu » (lot M) : l'accusé recule juste avant le
/// message, à la main, et le salon redevient non lu.
#[tokio::test]
async fn mark_unread_sends_one_manual_ack() {
    let mut s = connected(USER_TOKEN, "ready_user_social").await;
    let path = format!("/api/v10/channels/{GENERAL}/messages/5000000000000000202/ack");
    s.http.on("POST", &path, [Response::ok(json!({ "token": null }))]);
    assert!(s.client.store.mark_message_unread(GENERAL, 5000000000000000203));
    s.client.flush();
    let req = s.http.next_request().await;
    assert_eq!((req.method.as_str(), req.path.as_str()), ("POST", path.as_str()));
    assert_eq!(req.json(), json!({ "manual": true, "mention_count": 0 }));
    assert!(!req.headers.iter().any(|(n, _)| n.starts_with("x-super-properties")));
    assert!(s.client.store.unread().contains(&GENERAL));
    // Retenu : « lu » automatique sans effet tant que le salon n'est pas rouvert.
    assert!(s.client.store.take_commands().is_empty());
}

/// Lot U : « Ignorer », pseudo d'ami et demande de message acceptée — une
/// requête par geste, aux formes documentées ; la réponse à l'acceptation (le
/// MP sans son drapeau) sort la conversation du dossier des demandes.
#[tokio::test]
async fn ignore_nickname_and_message_request() {
    const ASK: u64 = 4000000000000000009;
    let mut s = connected(USER_TOKEN, "ready_user_social").await;
    let rel = format!("/api/v10/users/@me/relationships/{BOB}");
    let ignore = format!("{rel}/ignore");
    let consent = format!("/api/v10/channels/{ASK}/recipients/@me");
    s.http.on("PUT", &ignore, [no_content()]);
    s.http.on("PATCH", &rel, [no_content()]);
    let stranger = json!({ "id": "1000000000000000009", "username": "inconnu_fictif", "global_name": "Inconnu" });
    s.http.on("PUT", &consent, [Response::ok(json!({ "id": ASK.to_string(), "type": 1, "last_message_id": null, "recipients": [stranger.clone()] }))]);
    s.http.on("DELETE", &consent, [Response::ok(json!({ "id": ASK.to_string(), "type": 1, "recipients": [stranger] }))]);

    // La demande arrive par la passerelle : elle est rangée à part.
    s.conn.dispatch_fixture("channel_create_message_request", 21).await;
    s.client.until("demande reçue", |e| matches!(social(e), Some(RelationsEvent::Dm { .. }))).await;
    assert_eq!(s.client.store.relations.requests(true), vec![ASK]);
    assert!(s.client.store.relations.is_request(ASK));

    s.client.send(Cmd::Relations(RelationsCmd::Ignore { user_id: BOB }));
    s.client.send(Cmd::Relations(RelationsCmd::Nickname { user_id: BOB, nickname: Some("Bobby".into()) }));
    s.client.send(Cmd::Relations(RelationsCmd::AcceptRequest { channel_id: ASK }));
    s.client.until("demande acceptée", |e| matches!(social(e), Some(RelationsEvent::Dm { info, .. }) if !info.request)).await;
    assert!(!s.client.store.relations.is_request(ASK));
    assert_eq!(s.client.store.relations.request_count(), 0);

    s.client.send(Cmd::Relations(RelationsCmd::RejectRequest { channel_id: ASK }));
    let mut seen = Vec::new();
    while seen.len() < 4 {
        let r = s.http.next_request().await;
        if r.path == ignore || r.path == rel || r.path == consent {
            seen.push((r.method.clone(), r.path.clone(), if r.method == "DELETE" || r.path == ignore { Value::Null } else { r.json() }));
        }
    }
    assert!(seen.contains(&("PUT".into(), ignore.clone(), Value::Null)));
    assert!(seen.contains(&("PATCH".into(), rel.clone(), json!({ "nickname": "Bobby" }))));
    assert!(seen.contains(&("PUT".into(), consent.clone(), json!({ "consent_status": 2 }))));
    assert!(seen.contains(&("DELETE".into(), consent.clone(), Value::Null)));
}
