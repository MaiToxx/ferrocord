//! Rôles et membres d'un serveur (lot A3).
//!
//! Formes vérifiées le 13 novembre 2026 dans la documentation de Discord
//! (ressources « Guild » et « Topics › Permissions › Role Object »), toutes
//! documentées :
//! - Create Guild Role `POST /guilds/{id}/roles`, Modify Guild Role
//!   `PATCH /guilds/{id}/roles/{role}` (`name`, `permissions` en chaîne,
//!   `color`, `hoist`, `icon` en `data:`, `unicode_emoji`, `mentionable` ;
//!   réponse : l'objet Role), Delete Guild Role (204), Modify Guild Role
//!   Positions `PATCH /guilds/{id}/roles` (liste `{ id, position }` ;
//!   réponse : tous les rôles) ;
//! - Add / Remove Guild Member Role `PUT` / `DELETE
//!   /guilds/{id}/members/{user}/roles/{role}` (204), Get Guild Member,
//!   Modify Guild Member (`nick`, `null` pour le retirer ; réponse : le
//!   membre), Search Guild Members `GET /guilds/{id}/members/search`
//!   (`query`, `limit` de 1 à 1 000).
//!
//! Le reste de l'objet Role (`hoist`, `icon`, `unicode_emoji`, `managed`,
//! `colors`, `tags`, `flags`) et les fonctions du serveur (`features`,
//! dont `ROLE_ICONS` ; le niveau de boost est dans `Guild`) sont lus depuis READY, GUILD_CREATE,
//! GUILD_UPDATE et GUILD_ROLE_*. Chaque écriture suit un geste et passe par
//! le limiteur commun (`api`).

use super::super::people::store_member;
use super::super::raw::{de_lenient, de_loose_id, Item, RawGuild, RawGuildMember, RawGuildProps, RawRole, RawRoleDelete, RawRoleEvent};
use super::super::ratelimit::Route;
use super::super::rest::{api, Ctx};
use super::super::Out;
use crate::state::admin::roles::{Boost, RoleAction, RoleExtra, RoleFields, RoleTags, RolesCmd, RolesEvent, SEARCH_LIMIT};
use crate::state::model::{ImageHash, Role};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);

/// Objet Role Colors (dégradé).
#[derive(Deserialize)]
pub(in crate::net) struct RawColors {
    #[serde(default, deserialize_with = "de_lenient")]
    secondary_color: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    tertiary_color: Option<u64>,
}

/// GUILD_UPDATE : seulement les fonctions du serveur.
#[derive(Deserialize)]
struct RawGuildBoost {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    features: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    properties: Option<RawGuildProps>,
}

/// GUILD_DELETE
#[derive(Deserialize)]
struct RawGone {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default)]
    unavailable: bool,
}

fn color(v: Option<u64>) -> Option<u32> {
    v.and_then(|c| u32::try_from(c).ok()).filter(|c| *c != 0)
}

/// Ce que `model::Role` ne garde pas de l'objet Role.
pub(in crate::net) fn extra_of(r: &RawRole) -> RoleExtra {
    let tags = r.tags.as_ref();
    let id = |k: &str| tags.and_then(|t| t.get(k)).and_then(|v| v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64()));
    // Étiquettes booléennes : présentes (valeur `null`) ou absentes.
    let present = |k: &str| tags.is_some_and(|t| t.contains_key(k));
    RoleExtra {
        hoist: r.hoist.unwrap_or(false),
        icon: r.icon.as_deref().and_then(ImageHash::parse),
        unicode_emoji: r.unicode_emoji.clone().filter(|e| !e.is_empty()),
        managed: r.managed.unwrap_or(false),
        secondary_color: color(r.colors.as_ref().and_then(|c| c.secondary_color)),
        tertiary_color: color(r.colors.as_ref().and_then(|c| c.tertiary_color)),
        tags: RoleTags {
            bot_id: id("bot_id"),
            integration_id: id("integration_id"),
            premium_subscriber: present("premium_subscriber"),
            subscription: present("subscription_listing_id"),
            guild_connections: present("guild_connections"),
        },
        flags: r.flags.unwrap_or(0),
    }
}

/// Ce que le niveau de boost ouvre pour les rôles ; le niveau lui-même est
/// lu dans le modèle `Guild` (`premium_tier`), seule source.
fn boost(features: Option<&Vec<String>>) -> Option<Boost> {
    let features = features?;
    Some(Boost::from_features(features.iter().map(String::as_str)))
}

/// READY et GUILD_CREATE : rôles complets et niveau de boost d'un serveur,
/// avant `into_guild` (qui consomme le serveur).
pub(in crate::net) fn take(g: &RawGuild, out: &mut Vec<Event>) {
    if g.unavailable {
        return;
    }
    if let Some(list) = &g.roles {
        let roles = list.iter().filter_map(|r| r.0.as_ref()).map(|r| (r.id, extra_of(r))).collect();
        out.push(Event::Roles(RolesEvent::Extras { guild_id: g.id, roles, replace: true }));
    }
    let props = g.properties.as_ref();
    if let Some(b) = boost(g.features.as_ref().or(props.and_then(|p| p.features.as_ref()))) {
        out.push(Event::Roles(RolesEvent::Boost { guild_id: g.id, boost: b }));
    }
}

/// Événements de la passerelle qui touchent aux rôles (décodés à part).
pub(in crate::net) fn dispatch(kind: &str, d: &str, out: &Out) {
    match kind {
        "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" => {
            if let Ok(r) = serde_json::from_str::<RawRoleEvent>(d)
                && let Some(raw) = r.role.0.as_ref()
            {
                out.send(Event::Roles(RolesEvent::Extras { guild_id: r.guild_id, roles: vec![(raw.id, extra_of(raw))], replace: false }));
            }
        }
        "GUILD_ROLE_DELETE" => {
            if let Ok(RawRoleDelete { guild_id, role_id: Some(role_id) }) = serde_json::from_str::<RawRoleDelete>(d) {
                out.send(Event::Roles(RolesEvent::Forget { guild_id, role_id: Some(role_id) }));
            }
        }
        "GUILD_UPDATE" => {
            if let Ok(g) = serde_json::from_str::<RawGuildBoost>(d)
                && let Some(guild_id) = g.id
            {
                let props = g.properties.as_ref();
                if let Some(b) = boost(g.features.as_ref().or(props.and_then(|p| p.features.as_ref()))) {
                    out.send(Event::Roles(RolesEvent::Boost { guild_id, boost: b }));
                }
            }
        }
        "GUILD_DELETE" => {
            if let Ok(RawGone { id: Some(guild_id), unavailable: false }) = serde_json::from_str::<RawGone>(d) {
                out.send(Event::Roles(RolesEvent::Forget { guild_id, role_id: None }));
            }
        }
        _ => {}
    }
}

fn fields_json(f: &RoleFields) -> Value {
    let mut m = Map::new();
    if let Some(n) = &f.name {
        m.insert("name".into(), n.clone().into());
    }
    if let Some(p) = f.permissions {
        m.insert("permissions".into(), p.0.to_string().into());
    }
    if let Some(c) = f.color {
        m.insert("color".into(), c.into());
    }
    if let Some(h) = f.hoist {
        m.insert("hoist".into(), h.into());
    }
    if let Some(x) = f.mentionable {
        m.insert("mentionable".into(), x.into());
    }
    if let Some(i) = &f.icon {
        m.insert("icon".into(), i.clone().map_or(Value::Null, Value::from));
    }
    if let Some(e) = &f.unicode_emoji {
        m.insert("unicode_emoji".into(), e.clone().map_or(Value::Null, Value::from));
    }
    Value::Object(m)
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn request(cmd: &RolesCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        RolesCmd::Create { guild_id, fields } => r("POST /guilds/{}/roles", *guild_id, Method::POST, format!("/guilds/{guild_id}/roles"), Some(fields_json(fields))),
        RolesCmd::Edit { guild_id, role_id, fields } => {
            r("PATCH /guilds/{}/roles/{}", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/roles/{role_id}"), Some(fields_json(fields)))
        }
        RolesCmd::Delete { guild_id, role_id } => r("DELETE /guilds/{}/roles/{}", *guild_id, Method::DELETE, format!("/guilds/{guild_id}/roles/{role_id}"), None),
        RolesCmd::Reorder { guild_id, positions } => {
            let body: Vec<Value> = positions.iter().map(|(id, p)| json!({ "id": id.to_string(), "position": p })).collect();
            r("PATCH /guilds/{}/roles", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/roles"), Some(Value::Array(body)))
        }
        RolesCmd::MemberRole { guild_id, user_id, role_id, add } => {
            let (template, method) = if *add { ("PUT /guilds/{}/members/{}/roles/{}", Method::PUT) } else { ("DELETE /guilds/{}/members/{}/roles/{}", Method::DELETE) };
            r(template, *guild_id, method, format!("/guilds/{guild_id}/members/{user_id}/roles/{role_id}"), None)
        }
        RolesCmd::Nick { guild_id, user_id, nick } => {
            let nick = if nick.is_empty() { Value::Null } else { nick.clone().into() };
            r("PATCH /guilds/{}/members/{}", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/members/{user_id}"), Some(json!({ "nick": nick })))
        }
        // La requête est ajoutée par `url` (encodée).
        RolesCmd::Search { guild_id, .. } => r("GET /guilds/{}/members/search", *guild_id, Method::GET, format!("/guilds/{guild_id}/members/search"), None),
    }
}

fn url(api_root: &str, cmd: &RolesCmd, path: &str) -> Option<String> {
    let mut u = reqwest::Url::parse(&format!("{api_root}{path}")).ok()?;
    if let RolesCmd::Search { query, .. } = cmd {
        u.query_pairs_mut().append_pair("query", query).append_pair("limit", &SEARCH_LIMIT.to_string());
    }
    Some(u.into())
}

fn action(cmd: &RolesCmd) -> RoleAction {
    match cmd {
        RolesCmd::Create { .. } => RoleAction::Create,
        RolesCmd::Edit { .. } => RoleAction::Edit,
        RolesCmd::Delete { .. } => RoleAction::Delete,
        RolesCmd::Reorder { .. } => RoleAction::Reorder,
        RolesCmd::MemberRole { .. } => RoleAction::MemberRole,
        RolesCmd::Nick { .. } => RoleAction::Nick,
        RolesCmd::Search { .. } => RoleAction::Search,
    }
}

fn failed(cmd: &RolesCmd, problem: Problem) -> RolesEvent {
    let (guild_id, user_id, role_id) = match cmd {
        RolesCmd::Create { guild_id, .. } | RolesCmd::Reorder { guild_id, .. } | RolesCmd::Search { guild_id, .. } => (*guild_id, None, None),
        RolesCmd::Edit { guild_id, role_id, .. } | RolesCmd::Delete { guild_id, role_id } => (*guild_id, None, Some(*role_id)),
        RolesCmd::MemberRole { guild_id, user_id, role_id, .. } => (*guild_id, Some(*user_id), Some(*role_id)),
        RolesCmd::Nick { guild_id, user_id, .. } => (*guild_id, Some(*user_id), None),
    };
    RolesEvent::Failed { action: action(cmd), guild_id, user_id, role_id, problem }
}

/// Rôle lisible : (rôle, champs supplémentaires).
fn role_of(raw: RawRole) -> Option<(Role, RoleExtra)> {
    let extra = extra_of(&raw);
    Some((raw.into_role()?, extra))
}

/// Réponse illisible : rien n'est supposé, l'échec est annoncé.
fn unreadable() -> Problem {
    Problem::Api(String::new())
}

/// Réponse reçue d'une écriture de rôle : l'événement à publier.
fn answer(cmd: &RolesCmd, body: &str) -> Result<RolesEvent, Problem> {
    let a = action(cmd);
    match cmd {
        RolesCmd::Create { guild_id, .. } | RolesCmd::Edit { guild_id, .. } => {
            let raw = serde_json::from_str::<RawRole>(body).map_err(|_| unreadable())?;
            let role = role_of(raw).ok_or_else(unreadable)?;
            Ok(RolesEvent::Saved { action: a, guild_id: *guild_id, roles: vec![role] })
        }
        RolesCmd::Reorder { guild_id, .. } => {
            // Chaque rôle est lu à part : un illisible ne fait pas échouer les autres.
            let list = serde_json::from_str::<Vec<Item<RawRole>>>(body).map_err(|_| unreadable())?;
            let roles = list.into_iter().filter_map(|r| r.0.and_then(role_of)).collect();
            Ok(RolesEvent::Saved { action: a, guild_id: *guild_id, roles })
        }
        RolesCmd::Delete { guild_id, role_id } => Ok(RolesEvent::Deleted { guild_id: *guild_id, role_id: *role_id }),
        // Membres : traités par `run` (ils écrivent dans les tables des membres).
        RolesCmd::MemberRole { .. } | RolesCmd::Nick { .. } | RolesCmd::Search { .. } => Err(unreadable()),
    }
}

/// Membres d'une réponse (un ou une liste), écrits dans les tables des
/// membres ; le mien va aussi au magasin (permissions). Renvoie leurs identifiants.
fn store_members(ctx: &Ctx, guild_id: u64, members: &[RawGuildMember]) -> Vec<u64> {
    let mut ids = Vec::new();
    {
        let mut people = ctx.out.people.lock();
        for m in members {
            if let Some(id) = m.user_id() {
                store_member(&mut people, guild_id, m);
                ids.push(id);
            }
        }
    }
    for m in members {
        if let Some(user_id) = m.user_id() {
            ctx.out.send(Event::MemberUpdate { guild_id, user_id, member: m.to_member() });
        }
    }
    ids
}

async fn call(ctx: &Ctx, req: &Request, url: &str) -> Result<String, super::super::rest::ApiError> {
    api(ctx, req.route, req.method.clone(), url, req.body.clone(), ACTION_MAX_WAIT, std::future::pending()).await
}

async fn exec(ctx: &Ctx, cmd: &RolesCmd) -> Result<RolesEvent, super::super::rest::ApiError> {
    use super::super::rest::ApiError;
    let req = request(cmd);
    let url = url(&ctx.api_root, cmd, &req.path).ok_or_else(|| ApiError::Other(unreadable()))?;
    let body = call(ctx, &req, &url).await?;
    match cmd {
        RolesCmd::MemberRole { guild_id, user_id, role_id, .. } => {
            // 204 sans corps : le membre est relu pour afficher ses vrais rôles
            // (GUILD_MEMBER_UPDATE n'arrive pas toujours à un compte utilisateur).
            let get = Request { route: Route { template: "GET /guilds/{}/members/{}", major: *guild_id }, method: Method::GET, path: format!("/guilds/{guild_id}/members/{user_id}"), body: None };
            let member_url = format!("{}{}", ctx.api_root, get.path);
            if let Ok(text) = call(ctx, &get, &member_url).await
                && let Ok(m) = serde_json::from_str::<RawGuildMember>(&text)
            {
                store_members(ctx, *guild_id, std::slice::from_ref(&m));
            }
            Ok(RolesEvent::MemberDone { action: RoleAction::MemberRole, guild_id: *guild_id, user_id: *user_id, role_id: Some(*role_id) })
        }
        RolesCmd::Nick { guild_id, user_id, .. } => {
            if let Ok(m) = serde_json::from_str::<RawGuildMember>(&body) {
                store_members(ctx, *guild_id, std::slice::from_ref(&m));
            }
            Ok(RolesEvent::MemberDone { action: RoleAction::Nick, guild_id: *guild_id, user_id: *user_id, role_id: None })
        }
        RolesCmd::Search { guild_id, query } => {
            let list = serde_json::from_str::<Vec<Item<RawGuildMember>>>(&body).map_err(|_| ApiError::Other(unreadable()))?;
            let members: Vec<RawGuildMember> = list.into_iter().filter_map(|m| m.0).collect();
            let users = store_members(ctx, *guild_id, &members);
            Ok(RolesEvent::Found { guild_id: *guild_id, query: query.clone(), users })
        }
        _ => answer(cmd, &body).map_err(ApiError::Other),
    }
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: RolesCmd) {
    let ev = match exec(&ctx, &cmd).await {
        Ok(e) => Event::Roles(e),
        Err(e) => e.into_event(|p| Event::Roles(failed(&cmd, p))),
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Permissions;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn request_shapes() {
        let fields = RoleFields {
            name: Some("Lecteurs".into()),
            permissions: Some(Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES),
            color: Some(0x3498db),
            hoist: Some(true),
            mentionable: Some(false),
            icon: Some(None),
            unicode_emoji: Some(Some("📚".into())),
        };
        let c = request(&RolesCmd::Create { guild_id: 5, fields: fields.clone() });
        assert_eq!((c.method, c.path.as_str(), c.route.major), (Method::POST, "/guilds/5/roles", 5));
        assert_eq!(c.body, Some(json!({ "name": "Lecteurs", "permissions": "3072", "color": 3447003, "hoist": true, "mentionable": false, "icon": null, "unicode_emoji": "📚" })));
        let e = request(&RolesCmd::Edit { guild_id: 5, role_id: 7, fields: RoleFields { color: Some(0), ..Default::default() } });
        assert_eq!((e.method, e.path.as_str(), e.body), (Method::PATCH, "/guilds/5/roles/7", Some(json!({ "color": 0 }))));
        let d = request(&RolesCmd::Delete { guild_id: 5, role_id: 7 });
        assert_eq!((d.method, d.path.as_str(), d.body), (Method::DELETE, "/guilds/5/roles/7", None));
        let o = request(&RolesCmd::Reorder { guild_id: 5, positions: vec![(7, 2), (8, 1)] });
        assert_eq!(o.body, Some(json!([{ "id": "7", "position": 2 }, { "id": "8", "position": 1 }])));
        let add = request(&RolesCmd::MemberRole { guild_id: 5, user_id: 9, role_id: 7, add: true });
        assert_eq!((add.method, add.path.as_str()), (Method::PUT, "/guilds/5/members/9/roles/7"));
        let rm = request(&RolesCmd::MemberRole { guild_id: 5, user_id: 9, role_id: 7, add: false });
        assert_eq!(rm.method, Method::DELETE);
        assert_eq!(request(&RolesCmd::Nick { guild_id: 5, user_id: 9, nick: String::new() }).body, Some(json!({ "nick": null })));
        assert_eq!(request(&RolesCmd::Nick { guild_id: 5, user_id: 9, nick: "Bob".into() }).body, Some(json!({ "nick": "Bob" })));
        let s = RolesCmd::Search { guild_id: 5, query: "al é&x".into() };
        let r = request(&s);
        assert_eq!(url("http://127.0.0.1:9/api/v10", &s, &r.path).as_deref(), Some("http://127.0.0.1:9/api/v10/guilds/5/members/search?query=al+%C3%A9%26x&limit=100"));
    }

    #[test]
    fn role_answer_keeps_every_field() {
        let ev = answer(&RolesCmd::Create { guild_id: 2, fields: RoleFields::default() }, &fixture("rest_guild_role.json"));
        let Ok(RolesEvent::Saved { action: RoleAction::Create, guild_id: 2, roles }) = ev else { panic!("{ev:?}") };
        let (role, x) = &roles[0];
        assert_eq!((role.id, role.name.as_str(), role.color, role.position, role.mentionable), (2000000000000001101, "Bibliothécaires", 0x3498db, 3, true));
        assert_eq!(role.permissions, Permissions::VIEW_CHANNEL | Permissions::SEND_MESSAGES | Permissions::MANAGE_ROLES);
        assert!(x.hoist && x.managed && x.icon.is_some() && x.unicode_emoji.is_none());
        assert_eq!((x.secondary_color, x.tertiary_color), (Some(15277667), None));
        assert_eq!(x.tags, RoleTags { bot_id: Some(1000000000000000099), premium_subscriber: true, ..Default::default() });
        // Liste : les rôles illisibles sont écartés, les autres gardés.
        let ev = answer(&RolesCmd::Reorder { guild_id: 2, positions: Vec::new() }, &fixture("rest_guild_roles.json"));
        let Ok(RolesEvent::Saved { roles, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(roles.len(), 3);
        assert_eq!(roles[1].1.unicode_emoji.as_deref(), Some("📚"));
        assert!(answer(&RolesCmd::Edit { guild_id: 2, role_id: 1, fields: RoleFields::default() }, "{}").is_err());
        assert!(matches!(answer(&RolesCmd::Delete { guild_id: 2, role_id: 1 }, ""), Ok(RolesEvent::Deleted { role_id: 1, .. })));
    }

    #[test]
    fn gateway_events_carry_extras_and_boost() {
        let (tx, rx) = std::sync::mpsc::channel();
        let out = Out { tx, wake: Arc::new(|| {}), people: Default::default(), watch: Default::default(), interest: Default::default() };
        let role_update = fixture("guild_role_update.json");
        let d = serde_json::from_str::<Value>(&role_update).map(|v| v["d"].to_string()).unwrap_or_default();
        dispatch("GUILD_ROLE_UPDATE", &d, &out);
        dispatch("GUILD_ROLE_DELETE", r#"{ "guild_id": "10", "role_id": "11" }"#, &out);
        dispatch("GUILD_UPDATE", r#"{ "id": "10", "premium_tier": 2, "features": ["ROLE_ICONS", "COMMUNITY"] }"#, &out);
        dispatch("GUILD_UPDATE", r#"{ "id": "10", "name": "sans boost" }"#, &out);
        dispatch("GUILD_DELETE", r#"{ "id": "10", "unavailable": true }"#, &out);
        dispatch("GUILD_DELETE", r#"{ "id": "10" }"#, &out);
        let events: Vec<Event> = rx.try_iter().collect();
        assert_eq!(events.len(), 4, "{events:?}");
        assert!(matches!(&events[0], Event::Roles(RolesEvent::Extras { replace: false, roles, .. }) if roles.len() == 1));
        assert!(matches!(&events[1], Event::Roles(RolesEvent::Forget { guild_id: 10, role_id: Some(11) })));
        assert!(matches!(&events[2], Event::Roles(RolesEvent::Boost { guild_id: 10, boost: Boost { role_icons: true, .. } })));
        assert!(matches!(&events[3], Event::Roles(RolesEvent::Forget { guild_id: 10, role_id: None })));
    }
}
