//! Commandes d'applications et composants (lot T11) : index des commandes,
//! `POST /interactions` et accusés de la passerelle, face au faux serveur.

use crate::support::{ids::*, *};
use ferrocord::state::interactions::{CommandKind, Failure, Filled, InteractionEvent as I, OptionKind, PendingState, Value as V};
use ferrocord::state::Event;
use serde_json::{json, Value};

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

#[tokio::test]
async fn command_index_send_and_gateway_answers() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    assert_eq!(s.client.store.interactions.session().map(|x| x.0.as_str()), Some("session-fictive-0001"));

    // Liste ouverte : index du serveur et du compte, une seule fois.
    let guild = format!("/api/v10/guilds/{ALPHA}/application-command-index");
    let user = "/api/v10/users/@me/application-command-index";
    s.http.on("GET", &guild, [Response::ok(fixture("rest_application_command_index"))]);
    s.http.on("GET", user, [Response::ok(json!({ "applications": [], "application_commands": [], "version": "1" }))]);
    s.client.store.want_commands(GENERAL);
    s.client.store.want_commands(GENERAL);
    s.client.flush();
    let mut paths = vec![s.http.next_request().await.path, s.http.next_request().await.path];
    paths.sort();
    assert_eq!(paths, vec![guild.clone(), user.to_string()]);
    s.client.until("index", |e| matches!(e, Event::Interactions(I::Index { .. }))).await;
    s.client.until("index", |e| matches!(e, Event::Interactions(I::Index { .. }))).await;
    let cmds = s.client.store.app_commands(GENERAL, CommandKind::ChatInput);
    let id = cmds.iter().find(|c| c.cmd.name == "sondage").map(|c| c.cmd.id).expect("sondage dans l'index");

    // Commande : type 2, nonce et session de la passerelle.
    s.http.on("POST", "/api/v10/interactions", [no_content()]);
    let question = Filled { name: "question".into(), kind: OptionKind::String, value: V::Str("Pizza ?".into()) };
    assert!(s.client.store.run_command(GENERAL, id, Vec::new(), vec![question], None));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("POST", "/api/v10/interactions"));
    let body = r.json();
    let nonce = s.client.store.interactions.pending()[0].nonce;
    assert_eq!(body["type"], 2);
    assert_eq!(body["session_id"], "session-fictive-0001");
    assert_eq!(body["nonce"], nonce.to_string());
    assert_eq!((body["guild_id"].as_str(), body["channel_id"].as_str()), (Some("2000000000000000001"), Some("3000000000000000011")));
    assert_eq!(body["data"]["name"], "sondage");
    assert_eq!(body["data"]["options"], json!([{ "type": 3, "name": "question", "value": "Pizza ?" }]));
    s.client.until("envoyé", |e| matches!(e, Event::Interactions(I::Sent { .. }))).await;

    // Accusés : créée, puis sans réponse de l'application.
    s.conn.dispatch("INTERACTION_CREATE", 20, json!({ "id": "1200000000000000009", "nonce": nonce.to_string() })).await;
    s.client.until("créée", |e| matches!(e, Event::Interactions(I::Created { .. }))).await;
    s.conn.dispatch("INTERACTION_FAILURE", 21, json!({ "id": "1200000000000000009", "nonce": nonce.to_string(), "reason_code": 2 })).await;
    s.client.until("échec", |e| matches!(e, Event::Interactions(I::Failed { .. }))).await;
    assert_eq!(s.client.store.interactions.pending()[0].state, PendingState::Failed(Failure::NoResponse));
    s.client.store.dismiss_interaction(nonce);
    assert!(s.client.store.interactions.pending().is_empty());

    // Fenêtre modale du robot, puis sa réponse (type 5).
    // Elle répond à la commande partie d'ici (même nonce) : une fenêtre
    // relayée pour un autre appareil du compte ne s'ouvrirait pas.
    let mut modal_d = fixture("interaction_modal_create")["d"].clone();
    modal_d["nonce"] = json!(nonce.to_string());
    s.conn.dispatch("INTERACTION_MODAL_CREATE", 22, modal_d).await;
    s.client.until("modale", |e| matches!(e, Event::Interactions(I::Modal(_)))).await;
    let modal = s.client.store.take_modal().expect("fenêtre reçue");
    assert_eq!(modal.title, "Votre avis");
    s.http.on("POST", "/api/v10/interactions", [no_content()]);
    let value = ferrocord::state::interactions::ModalValue { kind: 4, custom_id: "titre".into(), text: Some("Bien".into()), values: Vec::new(), wrapper: 1 };
    assert!(s.client.store.submit_modal(&modal, vec![value]));
    s.client.flush();
    let body = s.http.next_request().await.json();
    assert_eq!(body["type"], 5);
    assert_eq!(body["data"]["id"], "1200000000000000002");
    assert_eq!(body["data"]["custom_id"], "avis");
    assert_eq!(body["data"]["components"][0]["components"][0], json!({ "type": 4, "custom_id": "titre", "value": "Bien" }));
    s.conn.dispatch("INTERACTION_SUCCESS", 23, json!({ "id": "1200000000000000003", "nonce": body["nonce"] })).await;
    s.client.until("réussie", |e| matches!(e, Event::Interactions(I::Succeeded { .. }))).await;
    assert!(s.client.store.interactions.pending().is_empty());
}

#[tokio::test]
async fn button_click_sends_once() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    s.http.on("GET", &messages_path(GENERAL), [Response::ok(json!([]))]);
    s.client.store.select_channel(GENERAL);
    s.client.flush();
    assert_eq!(s.http.next_request().await.method, "GET");
    s.client.until("historique", |e| matches!(e, Event::History { .. })).await;
    s.conn.dispatch_fixture("message_create_components", 20).await;
    s.client.until("message", |e| matches!(e, Event::MessageCreate(_))).await;
    let msg = 5000000000000000900;
    s.http.on("POST", "/api/v10/interactions", [no_content()]);
    assert!(s.client.store.press_component(GENERAL, msg, 2, "oui", None));
    assert!(!s.client.store.press_component(GENERAL, msg, 2, "oui", None), "un seul envoi par geste");
    s.client.flush();
    let body = s.http.next_request().await.json();
    assert_eq!(body["type"], 3);
    assert_eq!(body["message_id"], msg.to_string());
    assert_eq!(body["message_flags"], 64);
    assert_eq!(body["application_id"], "4000000000000000001");
    assert_eq!(body["data"], json!({ "component_type": 2, "custom_id": "oui" }));
}
