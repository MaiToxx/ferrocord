//! Écritures sociales et accusés de lecture (points d'API réservés aux
//! comptes utilisateur, autorisés par l'utilisateur le 13 novembre 2026).
//! Chacune suit un geste de l'utilisateur et passe par `api`, donc par le
//! limiteur de débit commun. Liste et sources : `docs/passerelle-utilisateur.md`.
//!
//! Aucun en-tête propre au client officiel n'est ajouté (ni
//! `X-Super-Properties` ni `X-Context-Properties`) : si Discord exige une
//! vérification (captcha), l'action est refusée et l'utilisateur est invité à
//! la faire dans l'application officielle.

use super::super::ratelimit::Route;
use super::super::raw::RawChannel;
use super::super::relations as parse;
use super::{api, ApiError, Ctx};
use crate::state::model::DISCORD_EPOCH;
use crate::state::relations::{Done, RelationsAction, RelationsCmd, RelationsEvent};
use crate::state::Event;
use reqwest::Method;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une écriture avant de partir : au-delà, l'échec est
/// annoncé (l'utilisateur peut recommencer).
const SOCIAL_MAX_WAIT: Duration = Duration::from_secs(10);

/// Réponse d'erreur qui demande un captcha (`captcha_key`, documentation
/// communautaire « CAPTCHA Handling »).
pub(super) fn is_captcha(v: Option<&Value>) -> bool {
    v.is_some_and(|v| v.get("captcha_key").is_some())
}

struct Request {
    route: Route,
    method: Method,
    /// Chemin sous la racine de l'API.
    path: String,
    body: Option<Value>,
}

/// Jours écoulés depuis l'époque de Discord (`last_viewed` d'un accusé).
fn days_since_epoch(now_ms: i64) -> i64 {
    (now_ms - DISCORD_EPOCH as i64).max(0) / 86_400_000
}

fn request(cmd: &RelationsCmd, now_ms: i64) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        RelationsCmd::Ack { channel_id, message_id } => r(
            "POST /channels/{}/messages/{}/ack",
            *channel_id,
            Method::POST,
            format!("/channels/{channel_id}/messages/{message_id}/ack"),
            Some(json!({ "token": null, "last_viewed": days_since_epoch(now_ms) })),
        ),
        // Accusé posé à la main juste avant le message (docs.discord.food,
        // « Read State », Acknowledge Message : `manual`, `mention_count`).
        RelationsCmd::Unack { channel_id, message_id, mentions } => r(
            "POST /channels/{}/messages/{}/ack",
            *channel_id,
            Method::POST,
            format!("/channels/{channel_id}/messages/{}/ack", message_id.saturating_sub(1)),
            Some(json!({ "manual": true, "mention_count": mentions })),
        ),
        RelationsCmd::AckBulk(list) => r(
            "POST /read-states/ack-bulk",
            0,
            Method::POST,
            "/read-states/ack-bulk".into(),
            Some(json!({ "read_states": list.iter().map(|(c, m)| json!({ "channel_id": c.to_string(), "message_id": m.to_string(), "read_state_type": 0 })).collect::<Vec<_>>() })),
        ),
        // `note` (non documenté officiellement ; docs.discord.food, « Send Friend
        // Request » : 120 caractères au plus) n'est envoyé que s'il y a un texte.
        RelationsCmd::AddFriend { username, note } => {
            let mut body = json!({ "username": username });
            if let Some(n) = note.as_deref().map(str::trim).filter(|n| !n.is_empty()) {
                body["note"] = json!(n.chars().take(crate::state::relations::NOTE_MAX).collect::<String>());
            }
            r("POST /users/@me/relationships", 0, Method::POST, "/users/@me/relationships".into(), Some(body))
        }
        // `confirm_stranger_request` (docs.discord.food, « Create Relationship ») : sans lui, Discord
        // refuse la demande d'une personne sans ami ni serveur en commun (« Tu dois confirmer la
        // demande d'ami »). Le clic sur « Accepter » est ce consentement.
        RelationsCmd::Accept { user_id } => r("PUT /users/@me/relationships/{}", 0, Method::PUT, format!("/users/@me/relationships/{user_id}"), Some(json!({ "confirm_stranger_request": true }))),
        RelationsCmd::Block { user_id } => r("PUT /users/@me/relationships/{}", 0, Method::PUT, format!("/users/@me/relationships/{user_id}"), Some(json!({ "type": 2 }))),
        RelationsCmd::Remove { user_id } => r("DELETE /users/@me/relationships/{}", 0, Method::DELETE, format!("/users/@me/relationships/{user_id}"), None),
        RelationsCmd::Unignore { user_id } => r("DELETE /users/@me/relationships/{}/ignore", 0, Method::DELETE, format!("/users/@me/relationships/{user_id}/ignore"), None),
        RelationsCmd::OpenDm { user_id } => r("POST /users/@me/channels", 0, Method::POST, "/users/@me/channels".into(), Some(json!({ "recipients": [user_id.to_string()] }))),
        RelationsCmd::CreateGroup { recipients } => r(
            "POST /users/@me/channels",
            0,
            Method::POST,
            "/users/@me/channels".into(),
            Some(json!({ "recipients": recipients.iter().map(u64::to_string).collect::<Vec<_>>() })),
        ),
        RelationsCmd::AddRecipient { channel_id, user_id } => r("PUT /channels/{}/recipients/{}", *channel_id, Method::PUT, format!("/channels/{channel_id}/recipients/{user_id}"), None),
        RelationsCmd::RemoveRecipient { channel_id, user_id } => r("DELETE /channels/{}/recipients/{}", *channel_id, Method::DELETE, format!("/channels/{channel_id}/recipients/{user_id}"), None),
        RelationsCmd::Rename { channel_id, name } => r("PATCH /channels/{}", *channel_id, Method::PATCH, format!("/channels/{channel_id}"), Some(json!({ "name": name }))),
        // Modify Channel d'un MP de groupe : `icon` (image `data:`) est documenté ;
        // `owner` l'est sur docs.discord.food (« Modify Channel », paramètres d'un Group DM).
        RelationsCmd::SetIcon { channel_id, icon } => r("PATCH /channels/{}", *channel_id, Method::PATCH, format!("/channels/{channel_id}"), Some(json!({ "icon": icon }))),
        RelationsCmd::Transfer { channel_id, user_id } => r("PATCH /channels/{}", *channel_id, Method::PATCH, format!("/channels/{channel_id}"), Some(json!({ "owner": user_id.to_string() }))),
        RelationsCmd::Close { channel_id } => r("DELETE /channels/{}", *channel_id, Method::DELETE, format!("/channels/{channel_id}"), None),
        // docs.discord.food, « Relationships » : Ignore User, Modify Relationship (`nickname`, amis seulement).
        RelationsCmd::Ignore { user_id } => r("PUT /users/@me/relationships/{}/ignore", 0, Method::PUT, format!("/users/@me/relationships/{user_id}/ignore"), None),
        RelationsCmd::Nickname { user_id, nickname } => r("PATCH /users/@me/relationships/{}", 0, Method::PATCH, format!("/users/@me/relationships/{user_id}"), Some(json!({ "nickname": nickname }))),
        // docs.discord.food, « Channels » : Update Message Request (`consent_status` 2 = ACCEPTED), Reject Message Request.
        RelationsCmd::AcceptRequest { channel_id } => r("PUT /channels/{}/recipients/@me", *channel_id, Method::PUT, format!("/channels/{channel_id}/recipients/@me"), Some(json!({ "consent_status": 2 }))),
        RelationsCmd::RejectRequest { channel_id } => r("DELETE /channels/{}/recipients/@me", *channel_id, Method::DELETE, format!("/channels/{channel_id}/recipients/@me"), None),
    }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: RelationsCmd) {
    let action = cmd.action();
    let req = request(&cmd, chrono::Utc::now().timestamp_millis());
    let url = format!("{}{}", ctx.api_root, req.path);
    match api(&ctx, req.route, req.method, &url, req.body, SOCIAL_MAX_WAIT, std::future::pending()).await {
        Ok(body) => match cmd {
            RelationsCmd::AddFriend { username, .. } => ctx.out.send(Event::Relations(RelationsEvent::RequestSent { username })),
            RelationsCmd::OpenDm { user_id } => opened(&ctx, &body, Some(user_id)),
            RelationsCmd::CreateGroup { .. } => opened(&ctx, &body, None),
            // Demande acceptée : la réponse est le MP, sans son drapeau de demande.
            RelationsCmd::AcceptRequest { .. } => {
                if let Some(e) = parse::private_channel(&body) {
                    ctx.out.send(Event::Relations(e));
                }
            }
            // Retrait accepté par Discord (ami retiré, demande annulée, blocage levé) :
            // appliqué tout de suite, sans dépendre du RELATIONSHIP_REMOVE de la
            // passerelle, qui le répétera sans effet. Sans cela, un blocage levé dont
            // l'événement ne venait pas laissait les messages masqués.
            RelationsCmd::Remove { user_id } => ctx.out.send(Event::Relations(RelationsEvent::RelationRemove { id: user_id })),
            RelationsCmd::Block { user_id } => done(&ctx, user_id, Done::Blocked),
            RelationsCmd::Accept { user_id } => done(&ctx, user_id, Done::Accepted),
            RelationsCmd::Ignore { user_id } => done(&ctx, user_id, Done::Ignored),
            RelationsCmd::Nickname { user_id, nickname } => ctx.out.send(Event::Relations(RelationsEvent::RelationUpdate { id: user_id, kind: None, nickname: Some(nickname), ignored: None })),
            RelationsCmd::Unignore { user_id } => ctx.out.send(Event::Relations(RelationsEvent::RelationUpdate { id: user_id, kind: None, nickname: None, ignored: Some(false) })),
            // MP et groupes : même principe, la réponse suffit. MP fermé, groupe quitté
            // ou demande refusée : la conversation sort de la liste.
            RelationsCmd::Close { channel_id } | RelationsCmd::RejectRequest { channel_id } => ctx.out.send(Event::ChannelDelete { guild_id: None, id: channel_id }),
            // Groupe renommé, icône ou propriétaire changés : la réponse est le groupe.
            // Ajout à un MP à deux : la réponse est le groupe créé (sinon elle est vide,
            // et le nouveau membre vient par CHANNEL_RECIPIENT_ADD).
            RelationsCmd::Rename { .. } | RelationsCmd::SetIcon { .. } | RelationsCmd::Transfer { .. } | RelationsCmd::AddRecipient { .. } => updated(&ctx, &body),
            RelationsCmd::RemoveRecipient { channel_id, user_id } => ctx.out.send(Event::Relations(RelationsEvent::RecipientRemove { channel_id, user_id })),
            // Le reste revient par la passerelle (MESSAGE_ACK).
            _ => {}
        },
        Err(ApiError::Captcha) => ctx.out.send(Event::Relations(RelationsEvent::Captcha { action })),
        // Accusé refusé : rien à montrer (il sera refait à la prochaine lecture).
        Err(ApiError::Cancelled | ApiError::Blocked(_) | ApiError::Unknown(_) | ApiError::Other(_) | ApiError::NotFound(_) | ApiError::Forbidden) if action == RelationsAction::Ack => {}
        Err(e) => ctx.out.send(e.into_event(|problem| Event::Relations(RelationsEvent::Failed { action, problem }))),
    }
}

fn done(ctx: &Ctx, id: u64, what: Done) {
    ctx.out.send(Event::Relations(RelationsEvent::Done { id, what }));
}

/// MP ou groupe rendu par une écriture : publié comme le ferait CHANNEL_UPDATE.
/// Réponse vide ou qui n'est pas un MP : rien.
fn updated(ctx: &Ctx, body: &str) {
    for e in updated_events(body) {
        ctx.out.send(e);
    }
}

fn updated_events(body: &str) -> Vec<Event> {
    let Ok(c) = serde_json::from_str::<RawChannel>(body) else { return Vec::new() };
    if c.guild_id.is_some() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let group = c.group_recipients(&std::collections::HashMap::new());
    if let Some((dm, known)) = c.into_dm_update() {
        out.push(Event::DmUpsert { dm, known });
    }
    if let Some((channel_id, users)) = group {
        out.push(Event::GroupRecipients { channel_id, users });
    }
    if let Some(e) = parse::private_channel(body) {
        out.push(Event::Relations(e));
    }
    out
}

/// MP créé ou retrouvé (réponse de Create DM) : publié comme par la
/// passerelle, puis ouvert.
fn opened(ctx: &Ctx, body: &str, user_id: Option<u64>) {
    let Ok(c) = serde_json::from_str::<RawChannel>(body) else { return };
    let Some((dm, known)) = c.into_dm_update() else { return };
    let channel_id = dm.id;
    ctx.out.send(Event::DmUpsert { dm, known });
    if let Some(e) = parse::private_channel(body) {
        ctx.out.send(Event::Relations(e));
    }
    ctx.out.send(Event::Relations(RelationsEvent::DmOpened { channel_id, user_id }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_shapes() {
        // 2026-11-13 00:00 UTC : 4334 jours après le 1er janvier 2015.
        let now = 1_794_528_000_000;
        let ack = request(&RelationsCmd::Ack { channel_id: 3, message_id: 5 }, now);
        assert_eq!((ack.method, ack.path.as_str(), ack.route.template, ack.route.major), (Method::POST, "/channels/3/messages/5/ack", "POST /channels/{}/messages/{}/ack", 3));
        assert_eq!(ack.body, Some(json!({ "token": null, "last_viewed": 4334 })));
        let unack = request(&RelationsCmd::Unack { channel_id: 3, message_id: 5, mentions: 2 }, now);
        assert_eq!((unack.method, unack.path.as_str(), unack.route.template), (Method::POST, "/channels/3/messages/4/ack", "POST /channels/{}/messages/{}/ack"));
        assert_eq!(unack.body, Some(json!({ "manual": true, "mention_count": 2 })));
        let bulk = request(&RelationsCmd::AckBulk(vec![(3, 5), (4, 6)]), now);
        assert_eq!(bulk.body, Some(json!({ "read_states": [ { "channel_id": "3", "message_id": "5", "read_state_type": 0 }, { "channel_id": "4", "message_id": "6", "read_state_type": 0 } ] })));
        let add = request(&RelationsCmd::AddFriend { username: "alice".into(), note: None }, now);
        assert_eq!((add.method, add.path.as_str(), add.body), (Method::POST, "/users/@me/relationships", Some(json!({ "username": "alice" }))));
        // Message joint : seulement s'il y a un texte, coupé à 120 caractères.
        let note = |n: &str| request(&RelationsCmd::AddFriend { username: "alice".into(), note: Some(n.into()) }, now).body;
        assert_eq!(note("  "), Some(json!({ "username": "alice" })));
        assert_eq!(note(" Salut, c'est Bob "), Some(json!({ "username": "alice", "note": "Salut, c'est Bob" })));
        assert_eq!(note(&"é".repeat(200)), Some(json!({ "username": "alice", "note": "é".repeat(120) })));
        let block = request(&RelationsCmd::Block { user_id: 7 }, now);
        assert_eq!((block.method, block.path.as_str(), block.body), (Method::PUT, "/users/@me/relationships/7", Some(json!({ "type": 2 }))));
        assert_eq!(request(&RelationsCmd::Accept { user_id: 7 }, now).body, Some(json!({ "confirm_stranger_request": true })));
        let rm = request(&RelationsCmd::Remove { user_id: 7 }, now);
        assert_eq!((rm.method, rm.path.as_str(), rm.body), (Method::DELETE, "/users/@me/relationships/7", None));
        assert_eq!(request(&RelationsCmd::Unignore { user_id: 7 }, now).path, "/users/@me/relationships/7/ignore");
        // Groupes (lot FF2) : icône et transfert de propriété, par Modify Channel.
        let icon = request(&RelationsCmd::SetIcon { channel_id: 9, icon: "data:image/png;base64,AAAA".into() }, now);
        assert_eq!((icon.method, icon.path.as_str(), icon.route.major, icon.body), (Method::PATCH, "/channels/9", 9, Some(json!({ "icon": "data:image/png;base64,AAAA" }))));
        let give = request(&RelationsCmd::Transfer { channel_id: 9, user_id: 7 }, now);
        assert_eq!((give.method, give.path.as_str(), give.body), (Method::PATCH, "/channels/9", Some(json!({ "owner": "7" }))));
        assert_eq!(request(&RelationsCmd::OpenDm { user_id: 7 }, now).body, Some(json!({ "recipients": ["7"] })));
        assert_eq!(request(&RelationsCmd::CreateGroup { recipients: vec![7, 8] }, now).body, Some(json!({ "recipients": ["7", "8"] })));
        let add = request(&RelationsCmd::AddRecipient { channel_id: 9, user_id: 7 }, now);
        assert_eq!((add.method, add.path.as_str(), add.route.major), (Method::PUT, "/channels/9/recipients/7", 9));
        assert_eq!(request(&RelationsCmd::RemoveRecipient { channel_id: 9, user_id: 7 }, now).method, Method::DELETE);
        let ren = request(&RelationsCmd::Rename { channel_id: 9, name: "Les amis".into() }, now);
        assert_eq!((ren.method, ren.body), (Method::PATCH, Some(json!({ "name": "Les amis" }))));
        let close = request(&RelationsCmd::Close { channel_id: 9 }, now);
        assert_eq!((close.method, close.path.as_str()), (Method::DELETE, "/channels/9"));
        let ign = request(&RelationsCmd::Ignore { user_id: 7 }, now);
        assert_eq!((ign.method, ign.path.as_str(), ign.body), (Method::PUT, "/users/@me/relationships/7/ignore", None));
        let nick = request(&RelationsCmd::Nickname { user_id: 7, nickname: Some("Ali".into()) }, now);
        assert_eq!((nick.method, nick.path.as_str(), nick.body), (Method::PATCH, "/users/@me/relationships/7", Some(json!({ "nickname": "Ali" }))));
        assert_eq!(request(&RelationsCmd::Nickname { user_id: 7, nickname: None }, now).body, Some(json!({ "nickname": null })));
        let acc = request(&RelationsCmd::AcceptRequest { channel_id: 9 }, now);
        assert_eq!((acc.method, acc.path.as_str(), acc.route.major, acc.body), (Method::PUT, "/channels/9/recipients/@me", 9, Some(json!({ "consent_status": 2 }))));
        let rej = request(&RelationsCmd::RejectRequest { channel_id: 9 }, now);
        assert_eq!((rej.method, rej.path.as_str(), rej.body), (Method::DELETE, "/channels/9/recipients/@me", None));
    }

    #[test]
    fn a_group_returned_by_a_write_is_applied_like_a_gateway_update() {
        let body = r#"{ "id": "9", "type": 3, "name": "Les amis", "icon": null, "owner_id": "1", "recipients": [{ "id": "7", "username": "eve", "global_name": "Eve" }] }"#;
        let ev = updated_events(body);
        assert!(ev.iter().any(|e| matches!(e, Event::DmUpsert { dm, .. } if dm.id == 9 && dm.name == "Les amis")), "{ev:?}");
        assert!(ev.iter().any(|e| matches!(e, Event::Relations(RelationsEvent::Dm { info, .. }) if info.id == 9)), "{ev:?}");
        // Réponse vide (204), illisible, ou salon de serveur : rien.
        assert!(updated_events("").is_empty() && updated_events("{}").is_empty());
        assert!(updated_events(r#"{ "id": "9", "type": 0, "guild_id": "4", "name": "général" }"#).is_empty());
    }

    #[test]
    fn captcha_is_recognised() {
        let v: Value = serde_json::from_str(r#"{ "captcha_key": ["captcha-required"], "captcha_sitekey": "x", "captcha_service": "hcaptcha" }"#).unwrap_or_default();
        assert!(is_captcha(Some(&v)));
        assert!(!is_captcha(Some(&json!({ "message": "Unknown User", "code": 10013 }))));
        assert!(!is_captcha(None));
    }
}
