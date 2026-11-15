//! Liste des membres : souscription (op 37) et GUILD_MEMBER_LIST_UPDATE.

use crate::support::{ids::*, *};
use ferrocord::state::members::{GroupId, MemberStatus, Row};
use ferrocord::state::{Cmd, Event, Status};
use serde_json::json;
use std::time::{Duration, Instant};

/// Ouvre #général d'Alpha et demande sa liste, comme le ferait l'interface.
fn open_general(c: &mut Client) {
    c.store.guild = Some(ALPHA);
    c.store.channel = Some(GENERAL);
    c.store.want_members(true, Some(0..20));
    for cmd in c.store.take_commands() {
        if matches!(cmd, Cmd::Subscribe(_)) {
            c.send(cmd);
        }
    }
}

#[tokio::test]
async fn subscription_then_list_fills_the_panel() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    let asked = Instant::now();
    open_general(&mut s.client);
    let p = s.conn.recv_op(37).await;
    // Regroupement des demandes : rien ne part avant le délai.
    assert!(asked.elapsed() >= Duration::from_millis(200), "{:?}", asked.elapsed());
    assert_eq!(
        p["d"],
        json!({ "subscriptions": { ALPHA.to_string(): { "typing": true, "activities": true, "channels": { GENERAL.to_string(): [[0, 99]] } } } })
    );

    s.conn.dispatch_fixture("guild_member_list_update_sync", 2).await;
    s.client.until("liste des membres", |e| matches!(e, Event::MemberList(_))).await;
    let list = s.client.store.members.list_for(ALPHA, GENERAL).unwrap_or_else(|| panic!("liste attendue"));
    assert_eq!((list.len(), list.member_count, list.online_count), (9, 6, 4));
    let groups: Vec<GroupId> = list.headers().iter().map(|h| h.group).collect();
    assert_eq!(groups, [GroupId::Role(2000000000000001010), GroupId::Online, GroupId::Offline]);
    let Row::Member(alice) = list.row(1) else { panic!("{:?}", list.row(1)) };
    assert_eq!((alice.name.as_str(), alice.status, alice.line.as_deref()), ("Alice (modération)", MemberStatus::Dnd, Some("En réunion")));

    // Mises à jour : « moi » passe en inactif plus bas, Alice en ligne.
    let mut ops = fixture("guild_member_list_update_ops");
    let unreadable = ops["d"]["ops"].as_array_mut().and_then(Vec::pop);
    s.conn.dispatch("GUILD_MEMBER_LIST_UPDATE", 3, ops["d"].clone()).await;
    s.client.until("mise à jour de la liste", |e| matches!(e, Event::MemberList(_))).await;
    let list = s.client.store.members.list_for(ALPHA, GENERAL).unwrap_or_else(|| panic!("liste attendue"));
    let Row::Member(me) = list.row(5) else { panic!("{:?}", list.row(5)) };
    assert_eq!((me.user_id, me.status), (ME, MemberStatus::Idle));
    assert!(list.is_header(6));
    let Row::Member(alice) = list.row(1) else { panic!("{:?}", list.row(1)) };
    assert_eq!((alice.status, alice.line.as_deref()), (MemberStatus::Online, None));

    // Opération illisible : les lignes deviennent inconnues, rien de faux n'est affiché.
    s.conn.dispatch("GUILD_MEMBER_LIST_UPDATE", 4, json!({ "guild_id": ALPHA.to_string(), "id": "everyone", "ops": [unreadable] })).await;
    s.client.until("opération illisible", |e| matches!(e, Event::MemberList(_))).await;
    let list = s.client.store.members.list_for(ALPHA, GENERAL).unwrap_or_else(|| panic!("liste attendue"));
    assert_eq!(list.loaded(), 0);
}

#[tokio::test]
async fn new_session_subscribes_again() {
    let mut s = connected(USER_TOKEN, "ready_user_classic").await;
    open_general(&mut s.client);
    s.conn.recv_op(37).await;
    s.conn.invalid_session(false).await;
    let mut conn = s.gw.accept().await;
    conn.hello_identify(QUIET).await;
    conn.send(ready_with_resume(&s.gw, "ready_user_classic")).await;
    s.client.until("READY", |e| matches!(e, Event::Ready { .. })).await;
    s.client.until("statut Connecté", |e| matches!(e, Event::Status(Status::Connected))).await;
    // Le serveur a tout oublié : même souscription, serveur compris.
    let p = conn.recv_op(37).await;
    assert_eq!(p["d"]["subscriptions"][ALPHA.to_string()]["typing"], true);
    assert_eq!(p["d"]["subscriptions"][ALPHA.to_string()]["channels"][GENERAL.to_string()], json!([[0, 99]]));
}

#[tokio::test]
async fn bot_accounts_never_subscribe() {
    let mut s = connected(BOT_TOKEN, "ready_bot").await;
    open_general(&mut s.client);
    s.client.until("liste indisponible", |e| matches!(e, Event::MemberListUnavailable)).await;
    assert!(s.client.store.members.unavailable);
    // Rien n'est parti : le prochain paquet du client est un heartbeat demandé.
    s.conn.request_heartbeat().await;
    let p = s.conn.recv().await;
    assert_eq!(p["op"], 1, "{p}");
}

#[tokio::test]
async fn group_dm_recipients_come_from_ready() {
    let s = connected(USER_TOKEN, "ready_user_classic").await;
    let c = s.client;
    let names: Vec<&str> = c.store.members.recipients(DM_GROUP).unwrap_or_default().iter().map(|u| u.name.as_str()).collect();
    assert_eq!(names, ["bob_fictif", "Ève"]);
    assert!(c.store.members.recipients(DM_ALICE).is_none(), "MP à deux : pas de liste");
}
