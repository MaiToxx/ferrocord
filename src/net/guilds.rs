//! Serveurs (lot A1) : rejoindre, quitter, créer, modèles, invitations.
//!
//! Formes vérifiées le 13 novembre 2026 :
//! - documentées par Discord (et sur docs.discord.food) : Leave Guild
//!   (`DELETE /users/@me/guilds/{id}`, 204), Get Guild Template
//!   (`GET /guilds/templates/{code}`), Create Channel Invite
//!   (`POST /channels/{id}/invites` : `max_age`, `max_uses`, `temporary`),
//!   Get Guild Invites (`GET /guilds/{id}/invites`), Delete Invite
//!   (`DELETE /invites/{code}`) ;
//! - réservées aux comptes utilisateur, forme lue sur docs.discord.food
//!   (« Invites » : Accept Invite ; « Guilds » : Create Guild ;
//!   « Guild Templates » : Use Guild Template) : `POST /invites/{code}`
//!   (corps vide, `session_id` facultatif non envoyé ; réponse : l'invitation,
//!   avec `new_member` et `show_verification_form`), `POST /guilds`
//!   (`name`, `icon`), `POST /guilds/templates/{code}` (`name`, `icon`).
//!
//! Toutes suivent un geste et passent par le limiteur commun (`api`) ; un
//! captcha n'est jamais contourné : l'échec renvoie à l'application officielle.

use super::raw::{de_lenient, de_loose_id};
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use crate::state::servers::{Failure, Invite, ServerAction, ServerCmd, ServerEvent, TemplateInfo};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);

#[derive(Deserialize, Default)]
struct RawNamed {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawUser {
    #[serde(default, deserialize_with = "de_lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    global_name: Option<String>,
}

/// Objet Invite (avec ses métadonnées quand elles sont là).
#[derive(Deserialize)]
struct RawInvite {
    // Obligatoire : une réponse d'une autre forme n'est pas une invitation.
    code: String,
    #[serde(default, deserialize_with = "de_lenient")]
    guild: Option<RawNamed>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    channel: Option<RawNamed>,
    #[serde(default, deserialize_with = "de_lenient")]
    inviter: Option<RawUser>,
    #[serde(default, deserialize_with = "de_lenient")]
    uses: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    max_uses: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    max_age: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    temporary: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    expires_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    show_verification_form: Option<bool>,
}

impl RawInvite {
    fn guild_id(&self) -> Option<u64> {
        self.guild.as_ref().and_then(|g| g.id).or(self.guild_id)
    }

    fn into_invite(self) -> Invite {
        Invite {
            guild_id: self.guild_id(),
            channel_id: self.channel.as_ref().and_then(|c| c.id),
            channel_name: self.channel.and_then(|c| c.name).unwrap_or_default(),
            inviter: self.inviter.and_then(|u| u.global_name.or(u.username)),
            uses: self.uses.unwrap_or(0),
            max_uses: self.max_uses.unwrap_or(0),
            max_age: self.max_age.unwrap_or(0),
            temporary: self.temporary.unwrap_or(false),
            expires_at_ms: self.expires_at.as_deref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis()),
            code: self.code,
        }
    }
}

#[derive(Deserialize)]
struct RawGuildId {
    #[serde(deserialize_with = "de_loose_id")]
    id: Option<u64>,
}

#[derive(Deserialize, Default)]
struct RawSource {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    channels: Option<Vec<Value>>,
}

#[derive(Deserialize)]
struct RawTemplate {
    code: String,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    usage_count: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    serialized_source_guild: Option<RawSource>,
}

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn with_icon(mut body: Value, icon: &Option<String>) -> Value {
    if let Some(i) = icon {
        body["icon"] = i.clone().into();
    }
    body
}

fn request(cmd: &ServerCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        ServerCmd::Join { code } => r("POST /invites/{}", 0, Method::POST, format!("/invites/{code}"), Some(json!({}))),
        ServerCmd::Leave { guild_id } => r("DELETE /users/@me/guilds/{}", 0, Method::DELETE, format!("/users/@me/guilds/{guild_id}"), None),
        ServerCmd::Create { name, icon } => r("POST /guilds", 0, Method::POST, "/guilds".into(), Some(with_icon(json!({ "name": name }), icon))),
        ServerCmd::Template { code } => r("GET /guilds/templates/{}", 0, Method::GET, format!("/guilds/templates/{code}"), None),
        ServerCmd::CreateFromTemplate { code, name, icon } => {
            r("POST /guilds/templates/{}", 0, Method::POST, format!("/guilds/templates/{code}"), Some(with_icon(json!({ "name": name }), icon)))
        }
        ServerCmd::CreateInvite { channel_id, max_age, max_uses, temporary } => r(
            "POST /channels/{}/invites",
            *channel_id,
            Method::POST,
            format!("/channels/{channel_id}/invites"),
            Some(json!({ "max_age": max_age, "max_uses": max_uses, "temporary": temporary })),
        ),
        // MP de groupe : `max_age` est le seul paramètre admis (docs.discord.food,
        // « Create Channel Invite », note 2 : de 1 à 604800 secondes).
        ServerCmd::CreateGroupInvite { channel_id } => r("POST /channels/{}/invites", *channel_id, Method::POST, format!("/channels/{channel_id}/invites"), Some(json!({ "max_age": 86_400 }))),
        ServerCmd::Invites { guild_id } => r("GET /guilds/{}/invites", *guild_id, Method::GET, format!("/guilds/{guild_id}/invites"), None),
        ServerCmd::Revoke { code, .. } => r("DELETE /invites/{}", 0, Method::DELETE, format!("/invites/{code}"), None),
    }
}

fn action(cmd: &ServerCmd) -> ServerAction {
    match cmd {
        ServerCmd::Join { .. } => ServerAction::Join,
        ServerCmd::Leave { .. } => ServerAction::Leave,
        ServerCmd::Create { .. } | ServerCmd::CreateFromTemplate { .. } => ServerAction::Create,
        ServerCmd::Template { .. } => ServerAction::Template,
        ServerCmd::CreateInvite { .. } | ServerCmd::CreateGroupInvite { .. } => ServerAction::Invite,
        ServerCmd::Invites { .. } => ServerAction::Invites,
        ServerCmd::Revoke { .. } => ServerAction::Revoke,
    }
}

/// Invitation acceptée (`POST /invites/{code}`) : réutilisable par les cartes
/// d'invitation des messages, qui passent par `Store::join_invite`.
fn joined(body: &str) -> Option<ServerEvent> {
    let inv = serde_json::from_str::<RawInvite>(body).ok()?;
    let guild_id = inv.guild_id();
    let name = inv.guild.as_ref().and_then(|g| g.name.clone()).or_else(|| inv.channel.as_ref().and_then(|c| c.name.clone())).unwrap_or_default();
    Some(ServerEvent::Joined { guild_id, name, verify: inv.show_verification_form.unwrap_or(false) })
}

/// Réponse reçue : l'événement à publier (`Err` : réponse sans la forme attendue).
fn answer(cmd: &ServerCmd, body: &str) -> Result<ServerEvent, Failure> {
    let unreadable = || Failure::Problem(Problem::Api(String::new()));
    match cmd {
        ServerCmd::Join { .. } => joined(body).ok_or_else(unreadable),
        ServerCmd::Leave { guild_id } => Ok(ServerEvent::Left { guild_id: *guild_id }),
        ServerCmd::Create { .. } | ServerCmd::CreateFromTemplate { .. } => {
            serde_json::from_str::<RawGuildId>(body).ok().and_then(|g| g.id).map(|guild_id| ServerEvent::Created { guild_id }).ok_or_else(unreadable)
        }
        ServerCmd::Template { .. } => {
            let t = serde_json::from_str::<RawTemplate>(body).map_err(|_| unreadable())?;
            let src = t.serialized_source_guild.unwrap_or_default();
            let channels = src.channels.unwrap_or_default().iter().filter_map(|c| c.get("name")?.as_str().map(str::to_string)).take(50).collect();
            Ok(ServerEvent::Template(TemplateInfo {
                code: t.code,
                name: t.name.unwrap_or_default(),
                description: t.description.unwrap_or_default(),
                usage_count: t.usage_count.unwrap_or(0),
                guild_name: src.name.unwrap_or_default(),
                channels,
            }))
        }
        // Documenté : 204 sans corps si Discord a coupé les invitations du serveur.
        ServerCmd::CreateInvite { .. } | ServerCmd::CreateGroupInvite { .. } if body.trim().is_empty() => Err(Failure::InvitesDisabled),
        ServerCmd::CreateInvite { .. } | ServerCmd::CreateGroupInvite { .. } => serde_json::from_str::<RawInvite>(body).map(|i| ServerEvent::InviteCreated(i.into_invite())).map_err(|_| unreadable()),
        ServerCmd::Invites { guild_id } => {
            // Chaque invitation est lue à part : une illisible ne fait pas échouer la liste.
            let list = serde_json::from_str::<Vec<Value>>(body).map_err(|_| unreadable())?;
            let invites = list.into_iter().filter_map(|v| serde_json::from_value::<RawInvite>(v).ok()).map(RawInvite::into_invite).collect();
            Ok(ServerEvent::Invites { guild_id: *guild_id, invites })
        }
        ServerCmd::Revoke { guild_id, code } => Ok(ServerEvent::Revoked { guild_id: *guild_id, code: code.clone() }),
    }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: ServerCmd) {
    let action = action(&cmd);
    let failed = |failure| Event::Servers(ServerEvent::Failed { action, failure });
    // Réservé aux comptes utilisateur : un bot ne rejoint ni ne crée de serveur ainsi.
    if ctx.is_bot() && matches!(cmd, ServerCmd::Join { .. } | ServerCmd::Create { .. } | ServerCmd::CreateFromTemplate { .. }) {
        ctx.out.send(failed(Failure::Problem(Problem::Forbidden)));
        return;
    }
    let req = request(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    let ev = match api(&ctx, req.route, req.method, &url, req.body, ACTION_MAX_WAIT, std::future::pending()).await {
        Ok(body) => answer(&cmd, &body).map(Event::Servers).unwrap_or_else(failed),
        Err(ApiError::Captcha) => failed(Failure::Captcha),
        Err(ApiError::NotFound(_)) => failed(Failure::NotFound),
        Err(e) => e.into_event(|p| failed(Failure::Problem(p))),
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn request_shapes() {
        let j = request(&ServerCmd::Join { code: "abc".into() });
        assert_eq!((j.method, j.path.as_str(), j.body), (Method::POST, "/invites/abc", Some(json!({}))));
        let l = request(&ServerCmd::Leave { guild_id: 9 });
        assert_eq!((l.method, l.path.as_str(), l.body), (Method::DELETE, "/users/@me/guilds/9", None));
        let c = request(&ServerCmd::Create { name: "Club".into(), icon: Some("data:image/png;base64,AA==".into()) });
        assert_eq!((c.path.as_str(), c.body), ("/guilds", Some(json!({ "name": "Club", "icon": "data:image/png;base64,AA==" }))));
        let c = request(&ServerCmd::CreateFromTemplate { code: "t1".into(), name: "Club".into(), icon: None });
        assert_eq!((c.path.as_str(), c.body), ("/guilds/templates/t1", Some(json!({ "name": "Club" }))));
        assert_eq!(request(&ServerCmd::Template { code: "t1".into() }).method, Method::GET);
        let i = request(&ServerCmd::CreateInvite { channel_id: 4, max_age: 3600, max_uses: 5, temporary: true });
        assert_eq!((i.path.as_str(), i.route.major, i.body), ("/channels/4/invites", 4, Some(json!({ "max_age": 3600, "max_uses": 5, "temporary": true }))));
        assert_eq!(request(&ServerCmd::Invites { guild_id: 2 }).path, "/guilds/2/invites");
        let g = request(&ServerCmd::CreateGroupInvite { channel_id: 4 });
        assert_eq!((g.method, g.path.as_str(), g.body), (Method::POST, "/channels/4/invites", Some(json!({ "max_age": 86400 }))));
        let r = request(&ServerCmd::Revoke { guild_id: 2, code: "abc".into() });
        assert_eq!((r.method, r.path.as_str()), (Method::DELETE, "/invites/abc"));
    }

    #[test]
    fn accepted_invite() {
        let ev = answer(&ServerCmd::Join { code: "fictif".into() }, &fixture("rest_invite_accept.json"));
        let Ok(ServerEvent::Joined { guild_id, name, verify }) = ev else { panic!("{ev:?}") };
        assert_eq!((guild_id, name.as_str(), verify), (Some(2000000000000000301), "Club fictif", true));
        // MP de groupe : pas de serveur.
        let ev = answer(&ServerCmd::Join { code: "x".into() }, r#"{ "code": "x", "type": 1, "channel": { "id": "5", "name": "Groupe", "type": 3 } }"#);
        assert!(matches!(ev, Ok(ServerEvent::Joined { guild_id: None, .. })), "{ev:?}");
        assert!(answer(&ServerCmd::Join { code: "x".into() }, "[]").is_err());
    }

    #[test]
    fn created_guild_and_template() {
        let ev = answer(&ServerCmd::Create { name: "x".into(), icon: None }, &fixture("rest_guild_create.json"));
        assert!(matches!(ev, Ok(ServerEvent::Created { guild_id: 2000000000000000302 })), "{ev:?}");
        let ev = answer(&ServerCmd::Template { code: "x".into() }, &fixture("rest_guild_template.json"));
        let Ok(ServerEvent::Template(t)) = ev else { panic!("{ev:?}") };
        assert_eq!((t.code.as_str(), t.name.as_str(), t.guild_name.as_str(), t.usage_count), ("modelefictif", "Club de lecture", "Lecteurs", 42));
        assert_eq!(t.channels, vec!["Salons textuels", "général", "lectures"]);
    }

    #[test]
    fn invites_list_and_creation() {
        let ev = answer(&ServerCmd::Invites { guild_id: 2000000000000000001 }, &fixture("rest_guild_invites.json"));
        let Ok(ServerEvent::Invites { invites, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(invites.len(), 2, "l'entrée illisible est écartée");
        let a = &invites[0];
        assert_eq!((a.code.as_str(), a.channel_name.as_str(), a.inviter.as_deref(), a.uses, a.max_uses), ("invfictif1", "général", Some("Alice"), 3, 10));
        assert_eq!((a.max_age, a.temporary, a.expires_at_ms), (86400, false, Some(1_763_164_800_000)));
        assert_eq!((invites[1].inviter.as_deref(), invites[1].expires_at_ms), (Some("bob"), None));
        let ev = answer(&ServerCmd::CreateInvite { channel_id: 1, max_age: 0, max_uses: 0, temporary: false }, "");
        assert_eq!(ev.err(), Some(Failure::InvitesDisabled));
    }
}
