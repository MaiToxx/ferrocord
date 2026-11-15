//! Widget, modèle de serveur et élagage (groupe 2, lot V) : requêtes et
//! réponses. Routes documentées par Discord (discord.com/developers, Guild
//! et Guild Template, relues le 15 novembre 2026) :
//! Get / Modify Guild Widget Settings (`GET`, `PATCH /guilds/{id}/widget` :
//! `enabled`, `channel_id` ; MANAGE_GUILD), Get Guild Templates
//! (`GET /guilds/{id}/templates`), Create Guild Template (`POST`, `name`,
//! `description`), Sync Guild Template (`PUT /guilds/{id}/templates/{code}`),
//! Delete Guild Template (`DELETE`), Get Guild Prune Count
//! (`GET /guilds/{id}/prune?days=N`) et Begin Guild Prune
//! (`POST /guilds/{id}/prune` : `days`, `compute_prune_count` ;
//! KICK_MEMBERS et MANAGE_GUILD).

use super::guild::Request;
use crate::net::ratelimit::Route;
use crate::net::raw::{de_lenient, de_loose_id};
use crate::state::admin::extras::{ExtraCmd, ExtraEvent, Template, Widget};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Deserialize)]
struct RawWidget {
    #[serde(default, deserialize_with = "de_lenient")]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
}

#[derive(Deserialize)]
struct RawTemplate {
    #[serde(default, deserialize_with = "de_lenient")]
    code: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    usage_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    is_dirty: Option<bool>,
}

impl RawTemplate {
    fn into_template(self) -> Option<Template> {
        Some(Template {
            code: self.code.filter(|c| !c.is_empty() && c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))?,
            name: self.name.unwrap_or_default(),
            description: self.description.unwrap_or_default(),
            usage_count: self.usage_count.unwrap_or(0),
            dirty: self.is_dirty.unwrap_or(false),
        })
    }
}

#[derive(Deserialize)]
struct RawPruned {
    #[serde(default, deserialize_with = "de_lenient")]
    pruned: Option<u32>,
}

pub(super) fn request(cmd: &ExtraCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body, reason: None };
    match cmd {
        ExtraCmd::Widget { guild_id } => r("GET /guilds/{}/widget", *guild_id, Method::GET, format!("/guilds/{guild_id}/widget"), None),
        ExtraCmd::SetWidget { guild_id, widget } => {
            let channel = widget.channel_id.map_or(Value::Null, |c| c.to_string().into());
            r("PATCH /guilds/{}/widget", *guild_id, Method::PATCH, format!("/guilds/{guild_id}/widget"), Some(json!({ "enabled": widget.enabled, "channel_id": channel })))
        }
        ExtraCmd::Templates { guild_id } => r("GET /guilds/{}/templates", *guild_id, Method::GET, format!("/guilds/{guild_id}/templates"), None),
        ExtraCmd::CreateTemplate { guild_id, name, description } => {
            r("POST /guilds/{}/templates", *guild_id, Method::POST, format!("/guilds/{guild_id}/templates"), Some(json!({ "name": name, "description": description })))
        }
        ExtraCmd::SyncTemplate { guild_id, code } => r("PUT /guilds/{}/templates/{}", *guild_id, Method::PUT, format!("/guilds/{guild_id}/templates/{code}"), None),
        ExtraCmd::DeleteTemplate { guild_id, code } => r("DELETE /guilds/{}/templates/{}", *guild_id, Method::DELETE, format!("/guilds/{guild_id}/templates/{code}"), None),
        ExtraCmd::PruneCount { guild_id, days } => r("GET /guilds/{}/prune", *guild_id, Method::GET, format!("/guilds/{guild_id}/prune?days={days}"), None),
        ExtraCmd::Prune { guild_id, days, reason } => Request {
            reason: reason.clone(),
            ..r("POST /guilds/{}/prune", *guild_id, Method::POST, format!("/guilds/{guild_id}/prune"), Some(json!({ "days": days, "compute_prune_count": true })))
        },
    }
}

/// Réponse reçue ; `None` : elle n'a pas la forme attendue.
pub(super) fn answer(cmd: &ExtraCmd, body: &str) -> Option<ExtraEvent> {
    let widget = |saved| {
        let w = serde_json::from_str::<RawWidget>(body).ok()?;
        Some(ExtraEvent::Widget { guild_id: cmd.guild_id(), widget: Widget { enabled: w.enabled?, channel_id: w.channel_id }, saved })
    };
    let template = || serde_json::from_str::<RawTemplate>(body).ok()?.into_template();
    let pruned = || serde_json::from_str::<RawPruned>(body).ok();
    match cmd {
        ExtraCmd::Widget { .. } => widget(false),
        ExtraCmd::SetWidget { .. } => widget(true),
        ExtraCmd::Templates { guild_id } => {
            let v = serde_json::from_str::<Vec<Value>>(body).ok()?;
            let list = v.into_iter().filter_map(|t| serde_json::from_value::<RawTemplate>(t).ok()?.into_template()).collect();
            Some(ExtraEvent::Templates { guild_id: *guild_id, list })
        }
        ExtraCmd::CreateTemplate { guild_id, .. } | ExtraCmd::SyncTemplate { guild_id, .. } => Some(ExtraEvent::TemplateSaved { guild_id: *guild_id, template: template()? }),
        // La réponse est le modèle supprimé : seul son code compte, et on le connaît.
        ExtraCmd::DeleteTemplate { guild_id, code } => Some(ExtraEvent::TemplateDeleted { guild_id: *guild_id, code: code.clone() }),
        ExtraCmd::PruneCount { guild_id, days } => Some(ExtraEvent::PruneCount { guild_id: *guild_id, days: *days, count: pruned()?.pruned? }),
        ExtraCmd::Prune { guild_id, .. } => Some(ExtraEvent::Pruned { guild_id: *guild_id, count: pruned().and_then(|p| p.pruned) }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn widget_requests_and_answers() {
        let r = request(&ExtraCmd::SetWidget { guild_id: 9, widget: Widget { enabled: true, channel_id: Some(5) } });
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::PATCH, "/guilds/9/widget", Some(json!({ "enabled": true, "channel_id": "5" }))));
        let r = request(&ExtraCmd::SetWidget { guild_id: 9, widget: Widget::default() });
        assert_eq!(r.body, Some(json!({ "enabled": false, "channel_id": null })));
        let ev = answer(&ExtraCmd::Widget { guild_id: 9 }, &fixture("rest_guild_widget.json"));
        let Some(ExtraEvent::Widget { widget, saved: false, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(widget, Widget { enabled: true, channel_id: Some(3000000000000000011) });
        assert!(answer(&ExtraCmd::Widget { guild_id: 9 }, "{}").is_none(), "sans `enabled`");
    }

    #[test]
    fn template_requests_and_answers() {
        let ev = answer(&ExtraCmd::Templates { guild_id: 9 }, &fixture("rest_guild_templates.json"));
        let Some(ExtraEvent::Templates { list, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(list.len(), 1, "l'entrée sans code est écartée");
        assert_eq!((list[0].code.as_str(), list[0].name.as_str(), list[0].usage_count, list[0].dirty), ("hgM48av5Q69A", "Club fictif", 3, true));
        let r = request(&ExtraCmd::CreateTemplate { guild_id: 9, name: "Club".into(), description: String::new() });
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::POST, "/guilds/9/templates", Some(json!({ "name": "Club", "description": "" }))));
        let r = request(&ExtraCmd::SyncTemplate { guild_id: 9, code: "abc".into() });
        assert_eq!((r.method, r.path.as_str(), r.body), (Method::PUT, "/guilds/9/templates/abc", None));
        assert_eq!(request(&ExtraCmd::DeleteTemplate { guild_id: 9, code: "abc".into() }).method, Method::DELETE);
        // Un code qui sortirait du chemin n'est jamais gardé.
        assert!(answer(&ExtraCmd::SyncTemplate { guild_id: 9, code: "abc".into() }, r#"{ "code": "../x" }"#).is_none());
    }

    #[test]
    fn prune_requests_and_answers() {
        let r = request(&ExtraCmd::PruneCount { guild_id: 9, days: 30 });
        assert_eq!((r.method, r.path.as_str()), (Method::GET, "/guilds/9/prune?days=30"));
        let r = request(&ExtraCmd::Prune { guild_id: 9, days: 7, reason: Some("ménage".into()) });
        assert_eq!((r.method, r.body, r.reason.as_deref()), (Method::POST, Some(json!({ "days": 7, "compute_prune_count": true })), Some("ménage")));
        assert!(matches!(answer(&ExtraCmd::PruneCount { guild_id: 9, days: 30 }, r#"{ "pruned": 12 }"#), Some(ExtraEvent::PruneCount { days: 30, count: 12, .. })));
        assert!(answer(&ExtraCmd::PruneCount { guild_id: 9, days: 30 }, r#"{ "pruned": null }"#).is_none());
        assert!(matches!(answer(&ExtraCmd::Prune { guild_id: 9, days: 7, reason: None }, r#"{ "pruned": null }"#), Some(ExtraEvent::Pruned { count: None, .. })));
    }
}
