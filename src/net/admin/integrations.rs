//! Objets Integration et Guild Application Command Permissions
//! (documentation : Guild Resource, Application Commands).

use crate::net::raw::{de_lenient, de_loose_id};
use crate::state::admin::integrations::{CommandPerm, CommandPerms, Integration, IntegrationApp, PermTarget};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct RawName {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    global_name: Option<String>,
}

impl RawName {
    fn display(self) -> Option<String> {
        self.global_name.or(self.username).or(self.name)
    }
}

#[derive(Deserialize)]
struct RawApp {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    bot: Option<RawName>,
}

#[derive(Deserialize)]
struct RawIntegration {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, rename = "type", deserialize_with = "de_lenient")]
    kind: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    account: Option<RawName>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawName>,
    #[serde(default, deserialize_with = "de_lenient")]
    application: Option<RawApp>,
    #[serde(default, deserialize_with = "de_lenient")]
    scopes: Option<Vec<String>>,
}

impl RawIntegration {
    fn into_integration(self) -> Option<Integration> {
        let application = self.application.and_then(|a| {
            let bot = a.bot.and_then(|b| Some((b.id?, b.display().unwrap_or_default())));
            Some(IntegrationApp { id: a.id?, name: a.name.unwrap_or_default(), description: a.description.unwrap_or_default(), bot })
        });
        Some(Integration {
            id: self.id?,
            name: self.name.unwrap_or_default(),
            kind: self.kind.unwrap_or_default(),
            enabled: self.enabled.unwrap_or(true),
            account: self.account.and_then(|a| a.name).unwrap_or_default(),
            user: self.user.and_then(RawName::display),
            application,
            scopes: self.scopes.unwrap_or_default(),
        })
    }
}

pub(super) fn list(body: &str) -> Option<Vec<Integration>> {
    let v = serde_json::from_str::<Vec<serde_json::Value>>(body).ok()?;
    Some(v.into_iter().filter_map(|i| serde_json::from_value::<RawIntegration>(i).ok()?.into_integration()).collect())
}

#[derive(Deserialize)]
struct RawPerm {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, rename = "type", deserialize_with = "de_lenient")]
    kind: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    permission: Option<bool>,
}

#[derive(Deserialize)]
struct RawCommandPerms {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    permissions: Option<Vec<serde_json::Value>>,
}

/// List Guild Application Command Permissions ; une entrée illisible est écartée.
pub(super) fn command_perms(body: &str) -> Option<Vec<CommandPerms>> {
    let v = serde_json::from_str::<Vec<serde_json::Value>>(body).ok()?;
    let one = |raw: RawCommandPerms| {
        let perms = raw
            .permissions
            .unwrap_or_default()
            .into_iter()
            .filter_map(|p| {
                let p = serde_json::from_value::<RawPerm>(p).ok()?;
                let id = p.id?;
                let target = match p.kind? {
                    1 => PermTarget::Role(id),
                    2 => PermTarget::User(id),
                    3 => PermTarget::Channel(id),
                    _ => return None,
                };
                Some(CommandPerm { target, allowed: p.permission? })
            })
            .collect();
        Some(CommandPerms { command_id: raw.id?, perms })
    };
    Some(v.into_iter().filter_map(|c| one(serde_json::from_value::<RawCommandPerms>(c).ok()?)).collect())
}
