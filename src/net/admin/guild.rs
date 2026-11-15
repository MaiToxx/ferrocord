//! Paramètres du serveur, webhooks, intégrations (lot A8 + A9) : requêtes et
//! réponses. Toutes les routes sont documentées par Discord (vérifiées le
//! 13 novembre 2026 sur discord.com/developers et docs.discord.food) :
//! Get Guild (`GET /guilds/{id}?with_counts=true`), Modify Guild
//! (`PATCH /guilds/{id}`, MANAGE_GUILD), Get Guild Vanity URL
//! (`GET /guilds/{id}/vanity-url`), Get Guild / Channel Webhooks, Create
//! Webhook (`POST /channels/{id}/webhooks` : `name`, `avatar`), Modify
//! Webhook (`PATCH /webhooks/{id}` : `name`, `avatar`, `channel_id`),
//! Delete Webhook, Get Guild Integrations, Delete Guild Integration, List
//! Guild Application Command Permissions
//! (`GET /applications/{app}/guilds/{guild}/commands/permissions`).
//!
//! Le champ facultatif `X-Audit-Log-Reason` (documenté, « URL-encoded ») part
//! avec les suppressions quand une raison est donnée. Aucune adresse de
//! webhook (elle contient son jeton) n'est journalisée.

use super::{integrations, webhooks};
use crate::net::raw::{de_lenient, de_loose_id};
use crate::net::ratelimit::Route;
use crate::net::rest::{api_req, ApiError, Ctx};
use crate::state::admin::guild::{AdminAction, AdminCmd, AdminEvent, Failure, GuildEdit, GuildPatch, Vanity};
use crate::state::admin::webhooks::HookScope;
use crate::state::Event;
use reqwest::Method;
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::{json, Map, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale avant de partir : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(15);
/// Raison du journal d'audit : 512 caractères au plus (documentation).
const REASON_MAX: usize = 512;

/// Champ présent (même `null`) : `Some`, avec sa valeur si elle est lisible.
fn present<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<Option<T>>, D::Error> {
    Ok(Some(de_lenient(d)?))
}

fn present_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Option<u64>>, D::Error> {
    Ok(Some(de_loose_id(d)?))
}

/// Objet Guild (réponse, GUILD_UPDATE) ; les comptes utilisateur reçoivent
/// parfois les champs dans `properties`.
#[derive(Deserialize, Default)]
struct RawSettings {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    owner_id: Option<u64>,
    #[serde(default, deserialize_with = "present")]
    icon: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    banner: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    splash: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    description: Option<Option<String>>,
    #[serde(default, deserialize_with = "present_id")]
    system_channel_id: Option<Option<u64>>,
    #[serde(default, deserialize_with = "de_lenient")]
    system_channel_flags: Option<u64>,
    #[serde(default, deserialize_with = "present_id")]
    afk_channel_id: Option<Option<u64>>,
    #[serde(default, deserialize_with = "de_lenient")]
    afk_timeout: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    default_message_notifications: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    preferred_locale: Option<String>,
    #[serde(default, deserialize_with = "present")]
    vanity_url_code: Option<Option<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_tier: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_subscription_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    premium_progress_bar_enabled: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    features: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    mfa_level: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    approximate_member_count: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    properties: Option<Box<RawSettings>>,
}

impl RawSettings {
    fn into_patch(self) -> GuildPatch {
        let p = self.properties.map(|b| b.into_patch()).unwrap_or_default();
        GuildPatch {
            name: self.name.or(p.name),
            owner_id: self.owner_id.or(p.owner_id),
            icon: self.icon.or(p.icon),
            banner: self.banner.or(p.banner),
            splash: self.splash.or(p.splash),
            description: self.description.or(p.description),
            system_channel_id: self.system_channel_id.or(p.system_channel_id),
            system_channel_flags: self.system_channel_flags.or(p.system_channel_flags),
            afk_channel_id: self.afk_channel_id.or(p.afk_channel_id),
            afk_timeout: self.afk_timeout.or(p.afk_timeout),
            default_message_notifications: self.default_message_notifications.or(p.default_message_notifications),
            preferred_locale: self.preferred_locale.or(p.preferred_locale),
            vanity_url_code: self.vanity_url_code.or(p.vanity_url_code),
            premium_tier: self.premium_tier.or(p.premium_tier),
            premium_subscription_count: self.premium_subscription_count.or(p.premium_subscription_count),
            premium_progress_bar_enabled: self.premium_progress_bar_enabled.or(p.premium_progress_bar_enabled),
            features: self.features.or(p.features),
            mfa_level: self.mfa_level.or(p.mfa_level),
            member_count: self.approximate_member_count.or(p.member_count),
        }
    }
}

/// Objet Guild lu : `None` s'il n'en a pas la forme.
fn patch(body: &str) -> Option<GuildPatch> {
    serde_json::from_str::<Value>(body).ok().filter(Value::is_object).and_then(|v| serde_json::from_value::<RawSettings>(v).ok()).map(RawSettings::into_patch)
}

#[derive(Deserialize)]
struct RawGuildRef {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
}

#[derive(Deserialize)]
struct RawVanity {
    #[serde(default, deserialize_with = "de_lenient")]
    code: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    uses: Option<u32>,
}

/// Événement de la passerelle qui concerne ce lot.
pub(super) fn gateway(kind: &str, d: &str) -> Option<AdminEvent> {
    match kind {
        "GUILD_UPDATE" => {
            let raw = serde_json::from_str::<RawSettings>(d).ok()?;
            let guild_id = raw.id?;
            Some(AdminEvent::Updated { guild_id, patch: raw.into_patch() })
        }
        "WEBHOOKS_UPDATE" => {
            let r = serde_json::from_str::<RawGuildRef>(d).ok()?;
            Some(AdminEvent::WebhooksChanged { guild_id: r.guild_id?, channel_id: r.channel_id })
        }
        "GUILD_INTEGRATIONS_UPDATE" => {
            let r = serde_json::from_str::<RawGuildRef>(d).ok()?;
            Some(AdminEvent::IntegrationsChanged { guild_id: r.guild_id? })
        }
        _ => None,
    }
}

/// Corps de Modify Guild : seuls les champs modifiés.
fn edit_body(e: &GuildEdit) -> Value {
    let mut m = Map::new();
    let id = |v: &Option<u64>| v.map_or(Value::Null, |i| Value::String(i.to_string()));
    let text = |v: &Option<String>| v.clone().map_or(Value::Null, Value::String);
    if let Some(n) = &e.name {
        m.insert("name".into(), n.clone().into());
    }
    for (k, v) in [("description", &e.description), ("icon", &e.icon), ("banner", &e.banner), ("splash", &e.splash)] {
        if let Some(v) = v {
            m.insert(k.into(), text(v));
        }
    }
    for (k, v) in [("system_channel_id", &e.system_channel_id), ("afk_channel_id", &e.afk_channel_id)] {
        if let Some(v) = v {
            m.insert(k.into(), id(v));
        }
    }
    if let Some(f) = e.system_channel_flags {
        m.insert("system_channel_flags".into(), f.into());
    }
    if let Some(t) = e.afk_timeout {
        m.insert("afk_timeout".into(), t.into());
    }
    if let Some(n) = e.default_message_notifications {
        m.insert("default_message_notifications".into(), n.into());
    }
    if let Some(l) = &e.preferred_locale {
        m.insert("preferred_locale".into(), l.clone().into());
    }
    if let Some(b) = e.premium_progress_bar_enabled {
        m.insert("premium_progress_bar_enabled".into(), b.into());
    }
    Value::Object(m)
}

/// Raison en « URL-encoded UTF-8 » (documentation, Audit Logs).
fn encode_reason(r: &str) -> String {
    let r: String = r.chars().take(REASON_MAX).collect();
    let mut out = String::with_capacity(r.len());
    for b in r.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub(super) struct Request {
    pub(super) route: Route,
    pub(super) method: Method,
    pub(super) path: String,
    pub(super) body: Option<Value>,
    pub(super) reason: Option<String>,
}

fn request(cmd: &AdminCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body, reason: None };
    match cmd {
        AdminCmd::Load { guild_id } => r("GET /guilds/{}", *guild_id, Method::GET, format!("/guilds/{guild_id}?with_counts=true"), None),
        AdminCmd::Modify { guild_id, edit } => r("PATCH /guilds/{}", *guild_id, Method::PATCH, format!("/guilds/{guild_id}"), Some(edit_body(edit))),
        AdminCmd::Vanity { guild_id } => r("GET /guilds/{}/vanity-url", *guild_id, Method::GET, format!("/guilds/{guild_id}/vanity-url"), None),
        AdminCmd::Webhooks { scope: HookScope::Guild(g), .. } => r("GET /guilds/{}/webhooks", *g, Method::GET, format!("/guilds/{g}/webhooks"), None),
        AdminCmd::Webhooks { scope: HookScope::Channel(c), .. } => r("GET /channels/{}/webhooks", *c, Method::GET, format!("/channels/{c}/webhooks"), None),
        AdminCmd::CreateWebhook { channel_id, name, avatar, .. } => {
            let mut body = json!({ "name": name });
            if let Some(a) = avatar {
                body["avatar"] = a.clone().into();
            }
            r("POST /channels/{}/webhooks", *channel_id, Method::POST, format!("/channels/{channel_id}/webhooks"), Some(body))
        }
        AdminCmd::EditWebhook { id, name, avatar, channel_id, .. } => {
            let mut body = Map::new();
            if let Some(n) = name {
                body.insert("name".into(), n.clone().into());
            }
            if let Some(a) = avatar {
                body.insert("avatar".into(), a.clone().map_or(Value::Null, Value::String));
            }
            if let Some(c) = channel_id {
                body.insert("channel_id".into(), c.to_string().into());
            }
            r("PATCH /webhooks/{}", *id, Method::PATCH, format!("/webhooks/{id}"), Some(Value::Object(body)))
        }
        AdminCmd::DeleteWebhook { id, reason, .. } => Request { reason: reason.clone(), ..r("DELETE /webhooks/{}", *id, Method::DELETE, format!("/webhooks/{id}"), None) },
        AdminCmd::Integrations { guild_id } => r("GET /guilds/{}/integrations", *guild_id, Method::GET, format!("/guilds/{guild_id}/integrations"), None),
        AdminCmd::RemoveIntegration { guild_id, id, reason } => Request {
            reason: reason.clone(),
            ..r("DELETE /guilds/{}/integrations/{}", *guild_id, Method::DELETE, format!("/guilds/{guild_id}/integrations/{id}"), None)
        },
        AdminCmd::CommandPerms { guild_id, app_id } => {
            r("GET /applications/{}/guilds/{}/commands/permissions", *guild_id, Method::GET, format!("/applications/{app_id}/guilds/{guild_id}/commands/permissions"), None)
        }
        AdminCmd::Extra(x) => super::extras::request(x),
    }
}

fn guild_of(cmd: &AdminCmd) -> u64 {
    match cmd {
        AdminCmd::Load { guild_id }
        | AdminCmd::Modify { guild_id, .. }
        | AdminCmd::Vanity { guild_id }
        | AdminCmd::CreateWebhook { guild_id, .. }
        | AdminCmd::EditWebhook { guild_id, .. }
        | AdminCmd::DeleteWebhook { guild_id, .. }
        | AdminCmd::Integrations { guild_id }
        | AdminCmd::RemoveIntegration { guild_id, .. }
        | AdminCmd::Webhooks { guild_id, .. }
        | AdminCmd::CommandPerms { guild_id, .. } => *guild_id,
        AdminCmd::Extra(x) => x.guild_id(),
    }
}

/// Sorte d'action et objet visé (webhook, intégration, application).
fn action(cmd: &AdminCmd) -> (AdminAction, Option<u64>) {
    match cmd {
        AdminCmd::Load { .. } => (AdminAction::Load, None),
        AdminCmd::Modify { .. } => (AdminAction::Save, None),
        AdminCmd::Vanity { .. } => (AdminAction::Vanity, None),
        AdminCmd::Webhooks { .. } => (AdminAction::Webhooks, None),
        AdminCmd::CreateWebhook { .. } => (AdminAction::SaveWebhook, Some(0)),
        AdminCmd::EditWebhook { id, .. } => (AdminAction::SaveWebhook, Some(*id)),
        AdminCmd::DeleteWebhook { id, .. } => (AdminAction::DeleteWebhook, Some(*id)),
        AdminCmd::Integrations { .. } => (AdminAction::Integrations, None),
        AdminCmd::RemoveIntegration { id, .. } => (AdminAction::RemoveIntegration, Some(*id)),
        AdminCmd::CommandPerms { app_id, .. } => (AdminAction::CommandPerms, Some(*app_id)),
        AdminCmd::Extra(x) => (AdminAction::Extra(x.action()), None),
    }
}

/// Réponse reçue : l'événement à publier (`Err` : réponse sans la forme attendue).
pub(in crate::net) fn answer(cmd: &AdminCmd, body: &str) -> Result<AdminEvent, Failure> {
    let bad = || Failure::Unreadable;
    Ok(match cmd {
        AdminCmd::Load { guild_id } => AdminEvent::Loaded { guild_id: *guild_id, patch: patch(body).ok_or_else(bad)?, saved: false },
        AdminCmd::Modify { guild_id, .. } => AdminEvent::Loaded { guild_id: *guild_id, patch: patch(body).ok_or_else(bad)?, saved: true },
        AdminCmd::Vanity { guild_id } => {
            let v = serde_json::from_str::<RawVanity>(body).map_err(|_| bad())?;
            AdminEvent::Vanity { guild_id: *guild_id, vanity: Vanity { code: v.code.filter(|c| !c.is_empty()), uses: v.uses.unwrap_or(0) } }
        }
        AdminCmd::Webhooks { scope, .. } => AdminEvent::Webhooks { scope: *scope, list: webhooks::list(body).ok_or_else(bad)? },
        AdminCmd::CreateWebhook { guild_id, .. } => AdminEvent::WebhookSaved { guild_id: *guild_id, hook: webhooks::one(body).ok_or_else(bad)?, created: true },
        AdminCmd::EditWebhook { guild_id, .. } => AdminEvent::WebhookSaved { guild_id: *guild_id, hook: webhooks::one(body).ok_or_else(bad)?, created: false },
        AdminCmd::DeleteWebhook { guild_id, id, .. } => AdminEvent::WebhookDeleted { guild_id: *guild_id, id: *id },
        AdminCmd::Integrations { guild_id } => AdminEvent::Integrations { guild_id: *guild_id, list: integrations::list(body).ok_or_else(bad)? },
        AdminCmd::RemoveIntegration { guild_id, id, .. } => AdminEvent::IntegrationRemoved { guild_id: *guild_id, id: *id },
        AdminCmd::CommandPerms { guild_id, app_id } => AdminEvent::CommandPerms { guild_id: *guild_id, app_id: *app_id, list: integrations::command_perms(body).ok_or_else(bad)? },
        AdminCmd::Extra(x) => AdminEvent::Extra(super::extras::answer(x, body).ok_or_else(bad)?),
    })
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: AdminCmd) {
    let (action, target) = action(&cmd);
    let guild_id = guild_of(&cmd);
    let failed = |failure| Event::GuildAdmin(AdminEvent::Failed { guild_id, action, failure, target });
    let req = request(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    let reason = req.reason.as_deref().map(encode_reason);
    let body = req.body;
    let build = |b: reqwest::RequestBuilder| {
        let b = match &body {
            Some(j) => b.json(j),
            None => b,
        };
        match &reason {
            Some(r) => b.header("X-Audit-Log-Reason", r.as_str()),
            None => b,
        }
    };
    let ev = match api_req(&ctx, req.route, req.method, &url, build, MAX_WAIT, std::future::pending()).await {
        Ok(text) => answer(&cmd, &text).map(Event::GuildAdmin).unwrap_or_else(failed),
        Err(ApiError::Forbidden) => failed(Failure::Forbidden),
        Err(ApiError::NotFound(_)) => failed(Failure::NotFound),
        Err(e) => e.into_event(|p| failed(Failure::Problem(p))),
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::integrations::PermTarget;
    use crate::state::admin::webhooks::HookKind;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn guild_settings_are_read_from_the_fixture() {
        let ev = answer(&AdminCmd::Load { guild_id: 1 }, &fixture("rest_guild_get.json"));
        let Ok(AdminEvent::Loaded { patch: p, saved: false, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(p.name.as_deref(), Some("Alpha"));
        assert_eq!((p.premium_tier, p.premium_subscription_count), (Some(2), Some(9)));
        assert_eq!(p.system_channel_id, Some(Some(3000000000000000011)));
        assert_eq!(p.afk_channel_id, Some(None));
        assert_eq!((p.afk_timeout, p.system_channel_flags, p.default_message_notifications), (Some(300), Some(5), Some(1)));
        assert_eq!(p.banner, Some(None));
        assert_eq!(p.vanity_url_code, Some(Some("alpha".into())));
        assert_eq!(p.member_count, Some(1234));
        assert!(p.features.as_ref().is_some_and(|f| f.iter().any(|x| x == "VANITY_URL")));
        assert!(answer(&AdminCmd::Load { guild_id: 1 }, "[]").is_err());
    }

    #[test]
    fn gateway_events() {
        let ev = gateway("GUILD_UPDATE", r#"{ "id": "9", "name": "Neuf", "premium_tier": 3, "banner": "abc", "description": null, "champ_inconnu": { "x": 1 }, "afk_timeout": "illisible" }"#);
        let Some(AdminEvent::Updated { guild_id: 9, patch: p }) = ev else { panic!("{ev:?}") };
        assert_eq!((p.name.as_deref(), p.premium_tier, p.banner, p.description, p.icon), (Some("Neuf"), Some(3), Some(Some("abc".into())), Some(None), None));
        // Comptes utilisateur : champs dans `properties`.
        let ev = gateway("GUILD_UPDATE", r#"{ "id": "9", "properties": { "name": "P", "afk_timeout": 60 }, "premium_subscription_count": 4 }"#);
        let Some(AdminEvent::Updated { patch: p, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!((p.name.as_deref(), p.afk_timeout, p.premium_subscription_count), (Some("P"), Some(60), Some(4)));
        assert!(matches!(gateway("WEBHOOKS_UPDATE", r#"{ "guild_id": "9", "channel_id": "5" }"#), Some(AdminEvent::WebhooksChanged { guild_id: 9, channel_id: Some(5) })));
        assert!(matches!(gateway("GUILD_INTEGRATIONS_UPDATE", r#"{ "guild_id": "9" }"#), Some(AdminEvent::IntegrationsChanged { guild_id: 9 })));
        assert!(gateway("GUILD_UPDATE", r#"{ "name": "sans id" }"#).is_none());
        assert!(gateway("MESSAGE_CREATE", "{}").is_none());
    }

    #[test]
    fn modify_sends_only_the_edited_fields() {
        let e = GuildEdit { name: Some("Club".into()), icon: Some(None), afk_channel_id: Some(Some(5)), system_channel_flags: Some(3), ..Default::default() };
        let r = request(&AdminCmd::Modify { guild_id: 9, edit: Box::new(e) });
        assert_eq!((r.method, r.path.as_str()), (Method::PATCH, "/guilds/9"));
        assert_eq!(r.body, Some(json!({ "name": "Club", "icon": null, "afk_channel_id": "5", "system_channel_flags": 3 })));
        assert_eq!(request(&AdminCmd::Load { guild_id: 9 }).path, "/guilds/9?with_counts=true");
    }

    #[test]
    fn webhook_requests_and_answers() {
        let c = request(&AdminCmd::CreateWebhook { guild_id: 9, channel_id: 5, name: "Annonces".into(), avatar: None });
        assert_eq!((c.path.as_str(), c.body), ("/channels/5/webhooks", Some(json!({ "name": "Annonces" }))));
        let e = request(&AdminCmd::EditWebhook { guild_id: 9, id: 7, name: None, avatar: Some(None), channel_id: Some(6) });
        assert_eq!((e.method, e.path.as_str(), e.body), (Method::PATCH, "/webhooks/7", Some(json!({ "avatar": null, "channel_id": "6" }))));
        let d = request(&AdminCmd::DeleteWebhook { guild_id: 9, id: 7, reason: Some("trop de bruit".into()) });
        assert_eq!((d.method, d.reason.as_deref()), (Method::DELETE, Some("trop de bruit")));
        let ev = answer(&AdminCmd::Webhooks { guild_id: 1, scope: HookScope::Guild(1) }, &fixture("rest_guild_webhooks.json"));
        let Ok(AdminEvent::Webhooks { list, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(list.len(), 2, "l'entrée sans identifiant est écartée");
        assert_eq!((list[0].kind, list[0].name.as_str(), list[0].creator.as_deref()), (HookKind::Incoming, "Annonces", Some("Alice")));
        assert!(list[0].url.as_ref().is_some_and(|u| u.expose().ends_with("/jetonfictif")));
        assert_eq!((list[1].kind, list[1].url.is_none(), list[1].source.as_deref()), (HookKind::Follower, true, Some("Club fictif #nouvelles")));
    }

    #[test]
    fn integrations_and_command_permissions() {
        let ev = answer(&AdminCmd::Integrations { guild_id: 1 }, &fixture("rest_guild_integrations.json"));
        let Ok(AdminEvent::Integrations { list, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(list.len(), 2);
        let bot = &list[0];
        assert_eq!((bot.kind.as_str(), bot.user.as_deref()), ("discord", Some("Alice")));
        assert_eq!(bot.application.as_ref().and_then(|a| a.bot.clone()), Some((4000000000000000002, "Robot fictif".into())));
        assert_eq!(list[1].account, "chaine-fictive");
        let ev = answer(&AdminCmd::CommandPerms { guild_id: 1, app_id: 2 }, &fixture("rest_command_permissions.json"));
        let Ok(AdminEvent::CommandPerms { list, .. }) = ev else { panic!("{ev:?}") };
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].perms, vec![crate::state::admin::integrations::CommandPerm { target: PermTarget::Role(2000000000000000001), allowed: false }]);
        assert_eq!(list[1].perms.len(), 2, "la permission de type inconnu est écartée");
        let r = request(&AdminCmd::CommandPerms { guild_id: 1, app_id: 2 });
        assert_eq!(r.path, "/applications/2/guilds/1/commands/permissions");
        let r = request(&AdminCmd::RemoveIntegration { guild_id: 1, id: 3, reason: None });
        assert_eq!((r.method, r.path.as_str(), r.reason), (Method::DELETE, "/guilds/1/integrations/3", None));
    }

    #[test]
    fn reason_is_url_encoded_and_bounded() {
        assert_eq!(encode_reason("Spam répété"), "Spam%20r%C3%A9p%C3%A9t%C3%A9");
        assert_eq!(encode_reason(&"a".repeat(600)).len(), REASON_MAX);
    }
}
