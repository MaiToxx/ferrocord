//! Profils et présence : PRESENCE_UPDATE, compléments de la carte de profil
//! (`GET /users/{id}`, `GET /guilds/{id}/members/{id}`) et « Envoyer un
//! message » (`POST /users/@me/channels`), face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::people::{ActivityKind, Status};
use ferrocord::state::profiles::ProfileState;
use ferrocord::state::{Effect, Event};
use serde_json::json;
use std::time::Duration;

const BOB: u64 = 1000000000000000003;
const DM_BOB: u64 = 4000000000000000003;

/// Attend qu'une condition sur le magasin devienne vraie (les écritures dans
/// les tables des profils ne passent pas par la file des événements).
async fn eventually(what: &str, s: &mut Setup, pred: impl Fn(&Setup) -> bool) {
    within(what, async {
        loop {
            s.client.drain();
            if pred(s) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
}

#[tokio::test]
async fn presence_update_is_stored_without_any_event() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.conn.dispatch_fixture("presence_update", 2).await;
    // Repère : un message envoyé après la présence ; la passerelle traite dans l'ordre.
    s.conn.dispatch_fixture("message_create_simple", 3).await;
    let mut seen = Vec::new();
    within("MESSAGE_CREATE", async {
        loop {
            let text = s.client.until("un événement", |_| true).await;
            let done = text.starts_with("MessageCreate");
            seen.push(text);
            if done {
                return;
            }
        }
    })
    .await;
    assert!(seen.iter().all(|e| !e.contains("Presence")), "aucun événement pour une présence : {seen:#?}");
    let mut p = s.client.store.people.lock();
    let presence = p.presence(ALICE).cloned().unwrap_or_else(|| panic!("présence d'Alice attendue"));
    assert_eq!(presence.status, Status::Online);
    assert_eq!(presence.activities.first().map(|a| a.kind), Some(ActivityKind::Custom));
    // Auteur du message et MP du READY retenus aussi.
    assert_eq!(p.user(ALICE).map(|u| u.username.clone()).as_deref(), Some("alice_fictive"));
    assert_eq!(p.dm_with(ALICE), Some(DM_ALICE));
}

/// Ouverture d'une carte : `GET /users/{id}/profile` (serveurs et amis en
/// commun, profil du serveur), une seule fois ; profil, membre, amis en
/// commun versés dans les tables.
#[tokio::test]
async fn full_profile_is_fetched_once_and_stored() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/users/{ALICE}/profile"), [Response::ok(fixture("rest_user_profile"))]);
    s.client.store.want_profile(ALICE, Some(ALPHA));
    s.client.store.want_profile(ALICE, Some(ALPHA));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.method, "GET");
    assert_eq!(
        r.path,
        format!("/api/v10/users/{ALICE}/profile?with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true&guild_id={ALPHA}")
    );
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    assert!(r.headers.iter().all(|(k, _)| !k.starts_with("x-super") && !k.starts_with("x-context")), "{:?}", r.headers);
    eventually("profil d'Alice", &mut s, |s| s.client.store.people.lock().profiles().profile(ALICE, Some(ALPHA)).and_then(ProfileState::profile).is_some()).await;
    {
        let p = s.client.store.people.lock();
        let prof = p.profiles().profile(ALICE, Some(ALPHA)).and_then(ProfileState::profile).cloned().unwrap_or_else(|| panic!("profil"));
        assert_eq!((prof.pronouns(), prof.friends_count()), (Some("elle"), Some(2)));
        assert_eq!(p.member(ALPHA, ALICE).and_then(|m| m.nick.clone()).as_deref(), Some("Ali"));
        assert_eq!(p.user(BOB).map(|u| u.display_name().to_string()).as_deref(), Some("Bob"));
    }
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(s.http.requests().len(), 1, "rien n'est redemandé");
}

/// Profil refusé (403 : aucun serveur en commun) : sans message, la carte se
/// rabat sur les compléments documentés.
#[tokio::test]
async fn refused_profile_falls_back_silently() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/users/{ALICE}/profile"), [Response::error(403, "Missing Access")]);
    s.http.on("GET", &format!("/api/v10/users/{ALICE}"), [Response::ok(fixture("rest_user"))]);
    s.http.on("GET", &format!("/api/v10/guilds/{ALPHA}/members/{ALICE}"), [Response::ok(fixture("rest_guild_member"))]);
    s.client.store.want_profile(ALICE, Some(ALPHA));
    s.client.flush();
    eventually("membre d'Alice", &mut s, |s| s.client.store.people.lock().member(ALPHA, ALICE).is_some()).await;
    let paths: Vec<String> = s.http.requests().iter().map(|r| r.path.split('?').next().unwrap_or_default().to_string()).collect();
    assert_eq!(paths, vec![format!("/api/v10/users/{ALICE}/profile"), format!("/api/v10/users/{ALICE}"), format!("/api/v10/guilds/{ALPHA}/members/{ALICE}")]);
    {
        let p = s.client.store.people.lock();
        assert_eq!(p.profiles().profile(ALICE, Some(ALPHA)), Some(&ProfileState::Unavailable));
        assert!(p.user(ALICE).is_some_and(|u| u.banner.is_some()));
    }
    s.client.drain();
    assert!(s.client.effects.iter().all(|e| !matches!(e, Effect::Toast(_))), "{:?}", s.client.effects);
}

/// Captcha demandé : jamais retenté, la carte renvoie à l'application officielle.
#[tokio::test]
async fn captcha_on_profile_is_not_retried() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &format!("/api/v10/users/{ALICE}/profile"), [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "x", "captcha_service": "hcaptcha" }))]);
    s.client.store.want_profile(ALICE, None);
    s.client.flush();
    eventually("captcha", &mut s, |s| s.client.store.people.lock().profiles().profile(ALICE, None) == Some(&ProfileState::Captcha)).await;
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert_eq!(s.http.requests().len(), 1);
}

/// Note : lue une fois (404 : pas de note), écrite à la demande (`PUT`, `note`).
#[tokio::test]
async fn note_is_read_then_written() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/users/@me/notes/{ALICE}");
    s.http.on("GET", &path, [Response::error(404, "Unknown User")]);
    s.http.on("PUT", &path, [Response::raw(204, serde_json::Value::Null)]);
    s.client.store.want_note(ALICE);
    s.client.store.want_note(ALICE);
    s.client.flush();
    eventually("note lue", &mut s, |s| s.client.store.people.lock().profiles().note(ALICE).is_some_and(|n| n.text.as_deref() == Some(""))).await;
    s.client.store.save_note(ALICE, "Rencontrée au club.");
    s.client.flush();
    eventually("note écrite", &mut s, |s| s.client.store.people.lock().profiles().note(ALICE).is_some_and(|n| n.writing.is_none() && n.text.as_deref() == Some("Rencontrée au club."))).await;
    let reqs = s.http.requests();
    assert_eq!(reqs.iter().map(|r| r.method.as_str()).collect::<Vec<_>>(), vec!["GET", "PUT"]);
    assert_eq!(reqs[1].json(), json!({ "note": "Rencontrée au club." }));
}

/// Compte bot : ni profil ni note (réservés aux comptes utilisateur), seulement
/// les points documentés.
#[tokio::test]
async fn bot_accounts_use_documented_endpoints_only() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    s.http.on("GET", &format!("/api/v10/users/{ALICE}"), [Response::ok(fixture("rest_user"))]);
    s.client.store.want_profile(ALICE, None);
    s.client.store.want_note(ALICE);
    s.client.flush();
    eventually("utilisateur", &mut s, |s| s.client.store.people.lock().user(ALICE).is_some_and(|u| u.banner.is_some())).await;
    eventually("note indisponible", &mut s, |s| s.client.store.people.lock().profiles().note(ALICE).is_some_and(|n| n.unavailable)).await;
    let paths: Vec<String> = s.http.requests().iter().map(|r| r.path.clone()).collect();
    assert_eq!(paths, vec![format!("/api/v10/users/{ALICE}")]);
}
/// « Envoyer un message » à quelqu'un sans MP connu : `POST /users/@me/channels`
/// avec `recipient_id` (documenté), puis le MP rendu est ajouté et ouvert.
#[tokio::test]
async fn send_message_creates_then_opens_the_dm() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", "/api/v10/users/@me/channels", [Response::ok(fixture("rest_dm_create"))]);
    s.http.on("GET", &messages_path(DM_BOB), [Response::ok(json!([]))]);
    s.client.store.message_user(BOB);
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/v10/users/@me/channels"));
    assert_eq!(r.json(), json!({ "recipient_id": BOB.to_string() }));
    assert_eq!(r.authorization.as_deref(), Some(USER_TOKEN));
    s.client.until("MP ouvert", |e| matches!(e, Event::DmOpened { .. })).await;
    assert_eq!((s.client.store.guild, s.client.store.channel), (None, Some(DM_BOB)));
    assert!(s.client.store.dms.iter().any(|d| d.id == DM_BOB && d.name == "Bob"));
    assert_eq!(s.client.store.people.lock().dm_with(BOB), Some(DM_BOB));
    // MP déjà connu (Alice, du READY) : ouvert sans requête.
    let before = s.http.requests().len();
    s.client.store.message_user(ALICE);
    s.client.flush();
    assert_eq!(s.client.store.channel, Some(DM_ALICE));
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(s.http.requests()[before..].iter().all(|r| r.method != "POST"));
}

/// Refus de Discord (par exemple, l'utilisateur n'accepte pas les MP) : notification.
#[tokio::test]
async fn refused_dm_creation_is_reported() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("POST", "/api/v10/users/@me/channels", [Response::error(400, "Cannot send messages to this user")]);
    s.client.store.message_user(BOB);
    s.client.flush();
    s.client.until("erreur", |e| matches!(e, Event::Error(_))).await;
    assert!(s.client.effects.iter().any(|e| matches!(e, Effect::Toast(_))));
    assert_eq!(s.client.store.channel, None);
}
