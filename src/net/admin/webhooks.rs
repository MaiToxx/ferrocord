//! Objet Webhook (documentation : Webhook Resource). Le jeton n'est gardé que
//! dans l'adresse, enveloppée dans un `Secret`.

use crate::net::raw::{de_lenient, de_loose_id};
use crate::state::admin::webhooks::{HookKind, Secret, Webhook};
use serde::Deserialize;

#[derive(Deserialize, Default)]
struct RawName {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    global_name: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawWebhook {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, rename = "type", deserialize_with = "de_lenient")]
    kind: Option<u8>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    avatar: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawName>,
    #[serde(default, deserialize_with = "de_lenient")]
    token: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    url: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    source_guild: Option<RawName>,
    #[serde(default, deserialize_with = "de_lenient")]
    source_channel: Option<RawName>,
}

impl RawWebhook {
    /// `None` sans identifiant.
    pub(super) fn into_webhook(self) -> Option<Webhook> {
        let id = self.id?;
        let url = self.url.filter(|u| u.starts_with("https://")).or_else(|| self.token.filter(|t| !t.is_empty()).map(|t| format!("https://discord.com/api/webhooks/{id}/{t}")));
        let source = match (self.source_guild.and_then(|g| g.name), self.source_channel.and_then(|c| c.name)) {
            (Some(g), Some(c)) => Some(format!("{g} #{c}")),
            (g, c) => g.or(c),
        };
        Some(Webhook {
            id,
            kind: HookKind::from_code(self.kind.unwrap_or(0)),
            guild_id: self.guild_id,
            channel_id: self.channel_id,
            name: self.name.unwrap_or_default(),
            avatar: self.avatar,
            creator: self.user.and_then(|u| u.global_name.or(u.username)),
            url: url.map(Secret::new),
            source,
        })
    }
}

/// Liste : une entrée illisible est écartée sans gêner les autres.
pub(super) fn list(body: &str) -> Option<Vec<Webhook>> {
    let v = serde_json::from_str::<Vec<serde_json::Value>>(body).ok()?;
    Some(v.into_iter().filter_map(|w| serde_json::from_value::<RawWebhook>(w).ok()?.into_webhook()).collect())
}

pub(super) fn one(body: &str) -> Option<Webhook> {
    serde_json::from_str::<RawWebhook>(body).ok()?.into_webhook()
}
