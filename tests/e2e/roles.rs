//! Rôles (lot A3) : créer, modifier, supprimer un rôle, donner et retirer
//! un rôle à un membre, pseudo, recherche de membres, refus A2F, face au
//! faux serveur. Le serveur Bêta du READY m'appartient. Rien ne quitte 127.0.0.1.

use crate::support::{ids::*, *};
use ferrocord::state::admin::roles::{RoleAction, RoleFields, RolesEvent, RolesNotice};
use ferrocord::state::{Event, Problem};
use serde_json::{json, Value};

const ROLE: u64 = 2000000000000001101;
const LIBRE: u64 = 2000000000000001102;

fn roles(e: &Event) -> Option<&RolesEvent> {
    match e {
        Event::Roles(r) => Some(r),
        _ => None,
    }
}

fn no_content() -> Response {
    Response { body: String::new(), ..Response::raw(204, Value::Null) }
}

#[tokio::test]
async fn create_edit_and_delete_a_role() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let path = format!("/api/v10/guilds/{BETA}/roles");
    s.http.on("POST", &path, [Response::ok(fixture("rest_guild_role"))]);
    assert!(s.client.store.create_role(BETA, "Bibliothécaires"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("POST", path.as_str(), json!({ "name": "Bibliothécaires" })));
    assert!(r.headers.iter().all(|(k, _)| k != "x-super-properties" && k != "x-audit-log-reason"));
    s.client.until("rôle créé", |e| matches!(roles(e), Some(RolesEvent::Saved { action: RoleAction::Create, .. }))).await;
    let g = s.client.store.guilds.iter().find(|g| g.id == BETA).expect("Bêta");
    assert!(g.roles.iter().any(|r| r.id == ROLE && r.name == "Bibliothécaires"));
    assert_eq!(s.client.store.roles_admin.take_created(), Some((BETA, ROLE)));
    assert_eq!(s.client.store.roles_admin.take_notice(), Some(RolesNotice::Saved(RoleAction::Create)));
    let extra = s.client.store.roles_admin.extra(BETA, ROLE).expect("compléments");
    assert!(extra.managed && extra.hoist);
    assert_eq!(extra.secondary_color, Some(15277667));

    // Modification : seuls les champs changés partent.
    let one = format!("/api/v10/guilds/{BETA}/roles/{ROLE}");
    // Réponse : le rôle n'est plus géré (il pourra être supprimé).
    let mut plain = fixture("rest_guild_role");
    plain["managed"] = false.into();
    plain["tags"] = Value::Null;
    s.http.on("PATCH", &one, [Response::ok(plain)]);
    assert!(s.client.store.edit_role(BETA, ROLE, RoleFields { color: Some(0), mentionable: Some(false), ..Default::default() }));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", one.as_str(), json!({ "color": 0, "mentionable": false })));
    s.client.until("rôle modifié", |e| matches!(roles(e), Some(RolesEvent::Saved { action: RoleAction::Edit, .. }))).await;
    assert!(!s.client.store.roles_admin.extra(BETA, ROLE).is_some_and(|x| x.managed));

    // Suppression refusée faute d'A2F : message clair, rôle gardé.
    let mfa = Response::raw(403, json!({ "message": "Two factor is required for this operation", "code": 60003 }));
    s.http.on("DELETE", &one, [mfa, no_content()]);
    assert!(s.client.store.delete_role(BETA, ROLE));
    s.client.flush();
    s.client.until("A2F", |e| matches!(roles(e), Some(RolesEvent::Failed { problem: Problem::MfaRequired, .. }))).await;
    assert!(s.client.store.guilds.iter().any(|g| g.id == BETA && g.roles.iter().any(|r| r.id == ROLE)));
    let _ = s.client.store.roles_admin.take_notice();
    assert_eq!(s.client.store.roles_admin.take_notice(), Some(RolesNotice::Failed { action: RoleAction::Delete, problem: Problem::MfaRequired }));
    assert_eq!(s.http.requests().iter().filter(|r| r.method == "DELETE").count(), 1, "jamais retenté");

    // Nouveau geste : supprimé.
    assert!(s.client.store.delete_role(BETA, ROLE));
    s.client.flush();
    s.client.until("rôle supprimé", |e| matches!(roles(e), Some(RolesEvent::Deleted { .. }))).await;
    assert!(!s.client.store.guilds.iter().any(|g| g.id == BETA && g.roles.iter().any(|r| r.id == ROLE)));
}

#[tokio::test]
async fn member_roles_nick_and_search() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Un rôle arrive par la passerelle (non géré : il peut être donné).
    let role = json!({ "id": LIBRE.to_string(), "name": "Lecteurs", "color": 0, "position": 1, "permissions": "0", "hoist": false, "managed": false, "mentionable": false });
    s.conn.dispatch("GUILD_ROLE_CREATE", 2, json!({ "guild_id": BETA.to_string(), "role": role })).await;
    s.client.until("rôle reçu", |e| matches!(e, Event::RoleUpsert { .. })).await;
    s.client.drain();
    assert!(s.client.store.guilds.iter().any(|g| g.id == BETA && g.roles.iter().any(|r| r.id == LIBRE)));

    // Recherche sur le serveur : les membres trouvés deviennent connus.
    s.http.on("GET", &format!("/api/v10/guilds/{BETA}/members/search"), [Response::ok(fixture("rest_guild_members_search"))]);
    assert!(s.client.store.search_members(BETA, "al"));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!(r.path, format!("/api/v10/guilds/{BETA}/members/search?query=al&limit=100"));
    s.client.until("membres trouvés", |e| matches!(roles(e), Some(RolesEvent::Found { .. }))).await;
    assert_eq!(s.client.store.roles_admin.search(BETA).map(|x| x.users.clone()), Some(vec![ALICE, 1000000000000000003]));
    assert!(s.client.store.known_members(BETA).contains(&ALICE));

    // Rôle donné : PUT puis relecture du membre.
    let put = format!("/api/v10/guilds/{BETA}/members/{ALICE}/roles/{LIBRE}");
    let member = format!("/api/v10/guilds/{BETA}/members/{ALICE}");
    let mut alice = fixture("rest_guild_member");
    alice["roles"] = json!([LIBRE.to_string()]);
    s.http.on("PUT", &put, [no_content()]);
    s.http.on("GET", &member, [Response::ok(alice)]);
    assert!(s.client.store.set_member_role(BETA, ALICE, LIBRE, true));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.body.as_str()), ("PUT", put.as_str(), ""));
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str()), ("GET", member.as_str()));
    s.client.until("rôle donné", |e| matches!(roles(e), Some(RolesEvent::MemberDone { .. }))).await;
    s.client.drain();
    let has = s.client.store.people.lock().member(BETA, ALICE).and_then(|m| m.roles.clone());
    assert_eq!(has, Some(vec![LIBRE]));

    // Pseudo : PATCH nick.
    let mut renamed = fixture("rest_guild_member");
    renamed["nick"] = "Al".into();
    s.http.on("PATCH", &member, [Response::ok(renamed)]);
    assert!(s.client.store.set_nick(BETA, ALICE, " Al "));
    s.client.flush();
    let r = s.http.next_request().await;
    assert_eq!((r.method.as_str(), r.path.as_str(), r.json()), ("PATCH", member.as_str(), json!({ "nick": "Al" })));
    s.client.until("pseudo changé", |e| matches!(roles(e), Some(RolesEvent::MemberDone { action: RoleAction::Nick, .. }))).await;
    s.client.drain();
    assert_eq!(s.client.store.people.lock().member(BETA, ALICE).and_then(|m| m.nick.clone()).as_deref(), Some("Al"));

    // Rôle supprimé par la passerelle : oublié partout.
    s.conn.dispatch("GUILD_ROLE_DELETE", 3, json!({ "guild_id": BETA.to_string(), "role_id": LIBRE.to_string() })).await;
    s.client.until("rôle oublié", |e| matches!(e, Event::RoleDelete { .. })).await;
    s.client.drain();
    assert!(!s.client.store.guilds.iter().any(|g| g.id == BETA && g.roles.iter().any(|r| r.id == LIBRE)));
}

#[tokio::test]
async fn nothing_is_offered_without_the_permission() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    // Alpha appartient à Alice et @everyone n'a que « Voir les salons ».
    assert!(!s.client.store.can_manage_roles(ALPHA));
    assert!(!s.client.store.create_role(ALPHA, "Essai"));
    assert!(!s.client.store.set_nick(ALPHA, ALICE, "x"));
    s.client.flush();
    assert!(s.http.requests().is_empty());
    s.client.drain();
}
