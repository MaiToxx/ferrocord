//! Sondages, invitations, annonces et adresses expirées (lot T13) : paquets
//! de la passerelle et requêtes REST, face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::announce::AnnounceEvent;
use ferrocord::state::invites::{InviteEvent, InviteState, JoinState};
use ferrocord::state::servers::{ServerAction, ServerEvent};
use ferrocord::state::polls::{PollEvent, Voters};
use ferrocord::state::Event;
use serde_json::{json, Value};

const POLL: u64 = 5000000000000000400;

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

/// Connecté, #général ouvert (page `rest_history`).
async fn general_open() -> Setup {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(fixture("rest_history"))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "GET");
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;
    s
}

fn counts(s: &Setup) -> Vec<(u32, bool)> {
    s.client.store.poll_of(GENERAL, POLL).map(|p| p.answers.iter().map(|a| (a.count, a.me)).collect()).unwrap_or_default()
}

#[tokio::test]
async fn poll_vote_voters_and_end() {
    let mut s = general_open().await;
    // Mon sondage : je peux le terminer.
    let mut poll = fixture("message_create_poll");
    poll["d"]["author"]["id"] = ME.to_string().into();
    s.conn.dispatch("MESSAGE_CREATE", 40, poll["d"].clone()).await;
    s.client.until("sondage", |e| matches!(e, Event::MessageCreate(m) if m.id == POLL)).await;
    assert_eq!(counts(&s), vec![(3, true), (1, false), (0, false)]);

    // Vote d'Alice reçu par la passerelle.
    s.conn.dispatch_fixture("message_poll_vote_add", 41).await;
    s.client.until("vote d'Alice", |e| matches!(e, Event::Polls(PollEvent::Vote { .. }))).await;
    assert_eq!(counts(&s), vec![(3, true), (2, false), (0, false)]);

    // Je change mon vote : PUT …/answers/@me, affiché tout de suite.
    let vote = format!("/api/v10/channels/{GENERAL}/polls/{POLL}/answers/@me");
    s.http.on("PUT", &vote, [no_content(), no_content()]);
    assert!(s.client.store.vote(GENERAL, POLL, vec![3, 1]));
    assert!(!s.client.store.vote(GENERAL, POLL, vec![2]), "un vote à la fois");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PUT", vote.as_str(), json!({ "answer_ids": [1, 3] })));
    assert_eq!(counts(&s), vec![(3, true), (2, false), (1, true)]);
    s.client.until("vote envoyé", |e| matches!(e, Event::Polls(PollEvent::VoteDone { error: None, .. }))).await;
    // Retrait : liste vide.
    assert!(s.client.store.vote(GENERAL, POLL, Vec::new()));
    s.client.flush();
    assert_eq!(s.http.next_request().await.json(), json!({ "answer_ids": [] }));
    s.client.until("vote retiré", |e| matches!(e, Event::Polls(PollEvent::VoteDone { .. }))).await;
    assert_eq!(counts(&s), vec![(2, false), (2, false), (0, false)]);

    // Votants d'une réponse, demandés une seule fois.
    let voters = format!("/api/v10/channels/{GENERAL}/polls/{POLL}/answers/2");
    s.http.on("GET", &voters, [Response::ok(json!({ "users": [fixture("rest_user")] }))]);
    assert!(s.client.store.request_voters(GENERAL, POLL, 2));
    assert!(!s.client.store.request_voters(GENERAL, POLL, 2));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("{voters}?limit=25"));
    s.client.until("votants", |e| matches!(e, Event::Polls(PollEvent::Voters { .. }))).await;
    assert!(matches!(s.client.store.polls.voters(POLL, 2), Some(Voters::Ready(v)) if v.len() == 1));

    // Fin du sondage.
    let expire = format!("/api/v10/channels/{GENERAL}/polls/{POLL}/expire");
    s.http.on("POST", &expire, [Response::ok(poll["d"].clone())]);
    assert!(s.client.store.end_poll(GENERAL, POLL));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", expire.as_str()));
    s.client.until("fin", |e| matches!(e, Event::Polls(PollEvent::EndDone { error: None, .. }))).await;

    // Création depuis la saisie : un message portant `poll`.
    let draft = ferrocord::state::polls::PollDraft { question: "Café ou thé\u{a0}?".into(), answers: vec!["Café".into(), "Thé".into(), " ".into()], hours: 4, multi: false };
    s.http.on("POST", &messages_path(GENERAL), [Response::ok(poll["d"].clone())]);
    assert!(s.client.store.create_poll(GENERAL, draft));
    s.client.flush();
    let body = s.http.next_request().await.json();
    assert_eq!(body["poll"]["question"], json!({ "text": "Café ou thé\u{a0}?" }));
    assert_eq!(body["poll"]["answers"], json!([{ "poll_media": { "text": "Café" } }, { "poll_media": { "text": "Thé" } }]));
    assert_eq!((body["poll"]["duration"].clone(), body["poll"]["allow_multiselect"].clone()), (json!(4), json!(false)));
    s.client.until("créé", |e| matches!(e, Event::Polls(PollEvent::CreateDone { error: None, .. }))).await;
}

#[tokio::test]
async fn invite_card_join_and_captcha() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = "/api/v10/invites/ferro";
    s.http.on("GET", path, [Response::ok(fixture("rest_invite"))]);
    assert!(s.client.store.want_invite("ferro"));
    assert!(!s.client.store.want_invite("ferro"), "une demande par code");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", "/api/v10/invites/ferro?with_counts=true"));
    s.client.until("invitation", |e| matches!(e, Event::Invites(InviteEvent::Loaded { .. }))).await;
    let Some(InviteState::Ready(inv)) = s.client.store.invites.get("ferro") else { panic!("invitation absente") };
    assert_eq!((inv.name.as_str(), inv.online, inv.members), ("Rustacés", Some(42), Some(1234)));

    // Captcha demandé : jamais contourné, la carte renvoie à l'app officielle.
    s.http.on("POST", path, [Response::raw(400, json!({ "captcha_key": ["captcha-required"], "captcha_sitekey": "fictif" }))]);
    assert!(s.client.store.join_invite_card("ferro"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("POST", json!({})));
    s.client.until("captcha", |e| matches!(e, Event::Servers(ServerEvent::Failed { action: ServerAction::Join, .. }))).await;
    assert_eq!(s.client.store.invites.join_state("ferro"), Some(JoinState::Captcha));
    assert!(!s.client.store.join_invite_card("ferro"), "pas de nouvel essai automatique");

    // Code inconnu : 404, carte « invalide ».
    s.http.on("GET", "/api/v10/invites/perdu", [Response::raw(404, json!({ "message": "Unknown Invite", "code": 10006 }))]);
    assert!(s.client.store.want_invite("perdu"));
    s.client.flush();
    s.http.next_request().await;
    s.client.until("invalide", |e| matches!(e, Event::Invites(InviteEvent::Invalid { .. }))).await;
    assert_eq!(s.client.store.invites.get("perdu"), Some(&InviteState::Invalid));
}

#[tokio::test]
async fn crosspost_mark_and_expired_urls() {
    let mut s = general_open().await;
    // Annonce venue d'un salon suivi : marquée, pas une réponse.
    s.conn.dispatch_fixture("message_create_crosspost", 42).await;
    s.client.until("annonce", |e| matches!(e, Event::MessageCreate(_))).await;
    let m = s.client.store.find_message(GENERAL, 5000000000000000402);
    let x = m.and_then(|m| m.meta.extra.as_deref()).expect("annonce marquée");
    assert!(x.source.is_some() && x.invites == vec!["ferro".to_string()]);

    // Pièce jointe dont l'adresse a expiré : une demande groupée, adresses remplacées.
    let old = "https://cdn.discordapp.com/attachments/fictif/a.png?ex=1&is=1&hm=1&";
    let mut att = fixture("message_create_simple");
    att["d"]["id"] = "5000000000000000410".into();
    att["d"]["attachments"] = json!([{ "id": "5000000000000000411", "filename": "a.png", "size": 10, "url": old, "proxy_url": "https://media.discordapp.net/attachments/fictif/a.png?ex=1&is=1&hm=1&", "content_type": "image/png", "width": 4, "height": 4 }]);
    s.conn.dispatch("MESSAGE_CREATE", 43, att["d"].clone()).await;
    s.client.until("pièce jointe", |e| matches!(e, Event::MessageCreate(m) if m.id == 5000000000000000410)).await;
    s.http.on("POST", "/api/v10/attachments/refresh-urls", [Response::ok(fixture("rest_refresh_urls"))]);
    let failed = vec!["https://media.discordapp.net/attachments/fictif/a.png?ex=1&is=1&hm=1&".to_string(), "https://cdn.discordapp.com/sans-signature.png".to_string()];
    assert_eq!(s.client.store.refresh_failed(&failed), 1);
    assert_eq!(s.client.store.refresh_failed(&failed), 0, "un seul essai");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.json()), ("POST", json!({ "attachment_urls": [old] })));
    s.client.until("rafraîchi", |e| matches!(e, Event::Announce(AnnounceEvent::Refreshed { .. }))).await;
    let a = s.client.store.find_message(GENERAL, 5000000000000000410).map(|m| m.attachments[0].clone());
    let a = a.expect("message gardé");
    assert!(a.url.contains("hm=cafe"), "{}", a.url);
    assert!(a.fetch_url().contains("hm=cafe") && a.fetch_url().starts_with("https://media.discordapp.net/"), "{}", a.fetch_url());
}

#[tokio::test]
async fn publish_in_announcement_channel() {
    let mut s = general_open().await;
    let mine = 5000000000000000202u64;
    let bob = 5000000000000000201u64;
    // #général devient un salon d'annonces où j'écris (sans gérer les messages).
    let mut update = fixture("channel_update_slowmode");
    update["d"]["id"] = GENERAL.to_string().into();
    update["d"]["type"] = 5.into();
    update["d"]["rate_limit_per_user"] = 0.into();
    s.conn.dispatch("CHANNEL_UPDATE", 50, update["d"].clone()).await;
    s.client.until("CHANNEL_UPDATE", |e| matches!(e, Event::ChannelUpsert { .. })).await;
    assert!(s.client.store.is_announcement(GENERAL));
    let can = |s: &Setup, id| s.client.store.find_message(GENERAL, id).is_some_and(|m| s.client.store.can_publish(m));
    assert!(can(&s, mine) && !can(&s, bob), "le mien seulement");
    let crosspost = format!("/api/v10/channels/{GENERAL}/messages/{mine}/crosspost");
    s.http.on("POST", &crosspost, [Response::ok(json!({ "id": mine.to_string(), "flags": 1 }))]);
    assert!(s.client.store.publish(GENERAL, mine));
    assert!(!s.client.store.publish(GENERAL, mine), "déjà en cours");
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", crosspost.as_str()));
    s.client.until("publié", |e| matches!(e, Event::Announce(AnnounceEvent::Flags { crossposted: true, .. }))).await;
    let m = s.client.store.find_message(GENERAL, mine).expect("message");
    assert!(m.meta.extra.as_ref().is_some_and(|x| x.crossposted));
    assert!(!s.client.store.can_publish(m), "publié une fois");
}
