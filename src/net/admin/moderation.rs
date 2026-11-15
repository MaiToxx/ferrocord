//! Modération, journal d'audit, AutoMod et sécurité (lots A4 et A5).
//!
//! Toutes ces routes sont documentées par Discord (ressources Guild, Audit
//! Log, Auto Moderation, Message) ; formes relues sur docs.discord.food le
//! 13 novembre 2026 :
//! - Remove Guild Member `DELETE /guilds/{g}/members/{u}` (204) ;
//! - Create Guild Ban `PUT /guilds/{g}/bans/{u}` (`delete_message_seconds`
//!   0 à 604800, 204) ; Remove Guild Ban `DELETE /guilds/{g}/bans/{u}` (204) ;
//! - Get Guild Bans `GET /guilds/{g}/bans` (`limit` 1 à 1000, obligatoire
//!   pour un compte utilisateur ; `after`) ;
//! - Bulk Guild Ban `POST /guilds/{g}/bulk-ban` (`user_ids` ≤ 200 ; réponse
//!   `banned_users`, `failed_users`) ;
//! - Modify Guild Member `PATCH /guilds/{g}/members/{u}`
//!   (`communication_disabled_until`, 28 jours au plus, `null` : levée) ;
//! - Bulk Delete Messages `POST /channels/{c}/messages/bulk-delete` (2 à 100
//!   messages de moins de 14 jours ; « not usable by user accounts » : un
//!   compte utilisateur supprime un par un, Delete Message, 204) ;
//! - Get Guild Audit Log `GET /guilds/{g}/audit-logs` (`limit` 1 à 100,
//!   `before`, `user_id`, `action_type`) ;
//! - Auto Moderation `GET|POST /guilds/{g}/auto-moderation/rules`,
//!   `PATCH|DELETE …/rules/{id}` ;
//! - Get Guild / Modify Guild `GET|PATCH /guilds/{g}` (`verification_level`,
//!   `explicit_content_filter`, `default_message_notifications`) ;
//! - Modify Guild Incident Actions `PUT /guilds/{g}/incident-actions`
//!   (`invites_disabled_until`, `dms_disabled_until`, 24 h au plus).
//!
//! L'en-tête facultatif `X-Audit-Log-Reason` (documenté) porte la raison,
//! encodée en pour-cent (UTF-8). Chaque requête suit un geste et passe par
//! le limiteur commun ; la suppression un par un aussi, à son rythme.

use super::super::ratelimit::Route;
use super::super::rest::{api_req, ApiError, Ctx};
use super::super::Out;
use crate::state::admin::audit::{self, Change, Entry};
use crate::state::admin::automod::{Execution, Rule, RuleAction, Trigger};
use crate::state::admin::moderation::{snowflake_ms, Ban, ModAction, ModCmd, ModEvent, Safety, SafetyChange, BANS_PAGE, BULK_DELETE_MAX, BULK_DELETE_MAX_AGE_MS, BULK_DELETE_MIN};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde_json::{json, Map, Value};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Attente maximale avant de partir : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(15);
/// Suppression un par un : chaque message attend son tour plus longtemps.
const DELETE_WAIT: Duration = Duration::from_secs(60);
/// Marge sous les 14 jours de la suppression groupée (horloges décalées).
const AGE_MARGIN_MS: i64 = 60_000;

/// Raison encodée pour l'en-tête : caractères non réservés gardés, le reste en `%XX`.
pub(crate) fn encode_reason(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn iso(ms: i64) -> Value {
    chrono::DateTime::from_timestamp_millis(ms).map_or(Value::Null, |d| Value::String(d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)))
}

fn time_ms(v: &Value) -> Option<i64> {
    v.as_str().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis())
}

fn id(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

fn ids(v: &Value) -> Vec<u64> {
    v.as_array().map(|a| a.iter().filter_map(id).collect()).unwrap_or_default()
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().filter_map(|s| s.as_str().map(str::to_string)).collect()).unwrap_or_default()
}

fn small(v: &Value) -> Option<u8> {
    v.as_u64().and_then(|n| u8::try_from(n).ok())
}

/// Requête prête : route du limiteur, méthode, chemin (avec requête), corps, raison.
struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
    reason: Option<String>,
}

fn rule_body(r: &Rule) -> Value {
    let mut meta = Map::new();
    match &r.trigger {
        Trigger::Keyword { keywords, regex, allow } | Trigger::Profile { keywords, regex, allow } => {
            meta.insert("keyword_filter".into(), json!(keywords));
            meta.insert("regex_patterns".into(), json!(regex));
            meta.insert("allow_list".into(), json!(allow));
        }
        Trigger::Preset { presets, allow } => {
            meta.insert("presets".into(), json!(presets));
            meta.insert("allow_list".into(), json!(allow));
        }
        Trigger::MentionSpam { limit, raid } => {
            meta.insert("mention_total_limit".into(), json!(limit));
            meta.insert("mention_raid_protection_enabled".into(), json!(raid));
        }
        Trigger::Spam | Trigger::Other(_) => {}
    }
    let actions: Vec<Value> = r
        .actions
        .iter()
        .map(|a| {
            let metadata = match a {
                RuleAction::Block { message: Some(m) } if !m.trim().is_empty() => json!({ "custom_message": m.trim() }),
                RuleAction::Alert { channel_id } => json!({ "channel_id": channel_id.to_string() }),
                RuleAction::Timeout { seconds } => json!({ "duration_seconds": seconds }),
                _ => json!({}),
            };
            json!({ "type": a.code(), "metadata": metadata })
        })
        .collect();
    let mut body = json!({
        "name": r.name.trim(),
        "event_type": r.trigger.event_type(),
        "trigger_metadata": Value::Object(meta),
        "actions": actions,
        "enabled": r.enabled,
        "exempt_roles": r.exempt_roles.iter().map(u64::to_string).collect::<Vec<_>>(),
        "exempt_channels": r.exempt_channels.iter().map(u64::to_string).collect::<Vec<_>>(),
    });
    // Le type de déclencheur ne se change pas après la création.
    if r.id == 0 {
        body["trigger_type"] = json!(r.trigger.code());
    }
    body
}

fn request(cmd: &ModCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>, reason: &Option<String>| Request {
        route: Route { template, major },
        method,
        path,
        body,
        reason: reason.clone(),
    };
    let none = &None;
    match cmd {
        ModCmd::Kick { guild_id: g, user_id: u, reason } => r("DELETE /guilds/{}/members/{}", *g, Method::DELETE, format!("/guilds/{g}/members/{u}"), None, reason),
        ModCmd::Ban { guild_id: g, user_id: u, delete_seconds, reason } => {
            r("PUT /guilds/{}/bans/{}", *g, Method::PUT, format!("/guilds/{g}/bans/{u}"), Some(json!({ "delete_message_seconds": delete_seconds })), reason)
        }
        ModCmd::Unban { guild_id: g, user_id: u, reason } => r("DELETE /guilds/{}/bans/{}", *g, Method::DELETE, format!("/guilds/{g}/bans/{u}"), None, reason),
        ModCmd::Timeout { guild_id: g, user_id: u, until_ms, reason } => {
            let until = until_ms.map_or(Value::Null, iso);
            r("PATCH /guilds/{}/members/{}", *g, Method::PATCH, format!("/guilds/{g}/members/{u}"), Some(json!({ "communication_disabled_until": until })), reason)
        }
        ModCmd::Bans { guild_id: g, after } => {
            let mut path = format!("/guilds/{g}/bans?limit={BANS_PAGE}");
            if let Some(a) = after {
                path.push_str(&format!("&after={a}"));
            }
            r("GET /guilds/{}/bans", *g, Method::GET, path, None, none)
        }
        ModCmd::BulkBan { guild_id: g, user_ids, delete_seconds, reason } => {
            let body = json!({ "user_ids": user_ids.iter().map(u64::to_string).collect::<Vec<_>>(), "delete_message_seconds": delete_seconds });
            r("POST /guilds/{}/bulk-ban", *g, Method::POST, format!("/guilds/{g}/bulk-ban"), Some(body), reason)
        }
        ModCmd::BulkDelete { channel_id: c, .. } => r("POST /channels/{}/messages/bulk-delete", *c, Method::POST, format!("/channels/{c}/messages/bulk-delete"), None, none),
        ModCmd::Audit { guild_id: g, filter, before } => {
            let mut path = format!("/guilds/{g}/audit-logs?limit={}", audit::PAGE);
            if let Some(b) = before {
                path.push_str(&format!("&before={b}"));
            }
            if let Some(u) = filter.user_id {
                path.push_str(&format!("&user_id={u}"));
            }
            if let Some(a) = filter.action {
                path.push_str(&format!("&action_type={a}"));
            }
            r("GET /guilds/{}/audit-logs", *g, Method::GET, path, None, none)
        }
        ModCmd::Rules { guild_id: g } => r("GET /guilds/{}/auto-moderation/rules", *g, Method::GET, format!("/guilds/{g}/auto-moderation/rules"), None, none),
        ModCmd::SaveRule { guild_id: g, rule } if rule.id == 0 => {
            r("POST /guilds/{}/auto-moderation/rules", *g, Method::POST, format!("/guilds/{g}/auto-moderation/rules"), Some(rule_body(rule)), none)
        }
        ModCmd::SaveRule { guild_id: g, rule } => r(
            "PATCH /guilds/{}/auto-moderation/rules/{}",
            *g,
            Method::PATCH,
            format!("/guilds/{g}/auto-moderation/rules/{}", rule.id),
            Some(rule_body(rule)),
            none,
        ),
        ModCmd::DeleteRule { guild_id: g, rule_id } => {
            r("DELETE /guilds/{}/auto-moderation/rules/{}", *g, Method::DELETE, format!("/guilds/{g}/auto-moderation/rules/{rule_id}"), None, none)
        }
        ModCmd::Safety { guild_id: g } => r("GET /guilds/{}", *g, Method::GET, format!("/guilds/{g}"), None, none),
        ModCmd::SetSafety { guild_id: g, change } => {
            let body = match change {
                SafetyChange::Verification(v) => json!({ "verification_level": v }),
                SafetyChange::ExplicitFilter(v) => json!({ "explicit_content_filter": v }),
                SafetyChange::Notifications(v) => json!({ "default_message_notifications": v }),
            };
            r("PATCH /guilds/{}", *g, Method::PATCH, format!("/guilds/{g}"), Some(body), none)
        }
        ModCmd::Incidents { guild_id: g, invites_until_ms, dms_until_ms } => {
            let body = json!({ "invites_disabled_until": invites_until_ms.map_or(Value::Null, iso), "dms_disabled_until": dms_until_ms.map_or(Value::Null, iso) });
            r("PUT /guilds/{}/incident-actions", *g, Method::PUT, format!("/guilds/{g}/incident-actions"), Some(body), none)
        }
    }
}

// ====================================================================== lecture des réponses

/// Nom affiché et nom d'utilisateur d'un objet User.
fn user_names(u: &Value) -> (String, String) {
    let username = u["username"].as_str().unwrap_or_default().to_string();
    let handle = match u["discriminator"].as_str() {
        Some(d) if d != "0" && !d.is_empty() => format!("{username}#{d}"),
        _ => username.clone(),
    };
    let name = u["global_name"].as_str().filter(|n| !n.is_empty()).map_or_else(|| username.clone(), str::to_string);
    (name, handle)
}

fn ban_of(v: &Value) -> Option<Ban> {
    let user = &v["user"];
    let (name, handle) = user_names(user);
    Some(Ban { user_id: id(&user["id"])?, name, handle, reason: v["reason"].as_str().map(str::to_string).filter(|r| !r.is_empty()) })
}

pub fn decode_bans(body: &str) -> Option<Vec<Ban>> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(v.as_array()?.iter().filter_map(ban_of).collect())
}

/// Valeur d'un changement en texte : texte tel quel, nombre et booléen écrits,
/// rôles ajoutés ou retirés par leur nom, le reste en JSON compact.
fn change_text(v: &Value) -> Option<String> {
    let s = match v {
        Value::Null => return None,
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::Array(a) if a.iter().all(|x| x.get("name").is_some()) => a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", "),
        other => other.to_string(),
    };
    Some(audit::clip(&s))
}

fn entry_of(v: &Value, names: &HashMap<u64, String>) -> Option<Entry> {
    let opt = &v["options"];
    let changes = v["changes"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|c| Some(Change { key: c["key"].as_str()?.to_string(), old: change_text(&c["old_value"]), new: change_text(&c["new_value"]) }))
                .take(audit::MAX_CHANGES)
                .collect()
        })
        .unwrap_or_default();
    let user_id = id(&v["user_id"]);
    let target_id = id(&v["target_id"]);
    let count = opt["count"].as_str().and_then(|s| s.parse().ok()).or_else(|| opt["count"].as_u64()).or_else(|| opt["members_removed"].as_str().and_then(|s| s.parse().ok()));
    let extra = opt["role_name"].as_str().or_else(|| opt["auto_moderation_rule_name"].as_str()).map(str::to_string);
    Some(Entry {
        id: id(&v["id"])?,
        action: u32::try_from(v["action_type"].as_u64()?).ok()?,
        user_name: user_id.and_then(|u| names.get(&u).cloned()),
        user_id,
        target_name: target_id.and_then(|t| names.get(&t).cloned()),
        target_id,
        reason: v["reason"].as_str().map(str::to_string).filter(|r| !r.is_empty()),
        changes,
        count,
        channel_id: id(&opt["channel_id"]),
        extra,
    })
}

/// Réponse de Get Guild Audit Log : noms tirés des tableaux joints.
pub fn decode_audit(body: &str) -> Option<Vec<Entry>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let mut names = HashMap::new();
    for u in v["users"].as_array().into_iter().flatten() {
        if let Some(i) = id(&u["id"]) {
            names.insert(i, user_names(u).0);
        }
    }
    for list in ["threads", "webhooks", "integrations", "auto_moderation_rules", "guild_scheduled_events"] {
        for x in v[list].as_array().into_iter().flatten() {
            if let (Some(i), Some(n)) = (id(&x["id"]), x["name"].as_str()) {
                names.entry(i).or_insert_with(|| n.to_string());
            }
        }
    }
    Some(v["audit_log_entries"].as_array()?.iter().filter_map(|e| entry_of(e, &names)).collect())
}

fn rule_of(v: &Value) -> Option<Rule> {
    let meta = &v["trigger_metadata"];
    let code = small(&v["trigger_type"])?;
    let trigger = match code {
        1 => Trigger::Keyword { keywords: strings(&meta["keyword_filter"]), regex: strings(&meta["regex_patterns"]), allow: strings(&meta["allow_list"]) },
        3 => Trigger::Spam,
        4 => Trigger::Preset { presets: meta["presets"].as_array().map(|a| a.iter().filter_map(small).collect()).unwrap_or_default(), allow: strings(&meta["allow_list"]) },
        5 => Trigger::MentionSpam {
            limit: meta["mention_total_limit"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
            raid: meta["mention_raid_protection_enabled"].as_bool().unwrap_or(false),
        },
        6 => Trigger::Profile { keywords: strings(&meta["keyword_filter"]), regex: strings(&meta["regex_patterns"]), allow: strings(&meta["allow_list"]) },
        c => Trigger::Other(c),
    };
    let actions = v["actions"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let m = &x["metadata"];
                    Some(match small(&x["type"])? {
                        1 => RuleAction::Block { message: m["custom_message"].as_str().map(str::to_string).filter(|s| !s.is_empty()) },
                        2 => RuleAction::Alert { channel_id: id(&m["channel_id"]).unwrap_or(0) },
                        3 => RuleAction::Timeout { seconds: m["duration_seconds"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0) },
                        4 => RuleAction::Quarantine,
                        c => RuleAction::Other(c),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Some(Rule {
        id: id(&v["id"])?,
        name: v["name"].as_str().unwrap_or_default().to_string(),
        enabled: v["enabled"].as_bool().unwrap_or(false),
        trigger,
        actions,
        exempt_roles: ids(&v["exempt_roles"]),
        exempt_channels: ids(&v["exempt_channels"]),
    })
}

pub fn decode_rules(body: &str) -> Option<Vec<Rule>> {
    let v: Value = serde_json::from_str(body).ok()?;
    Some(v.as_array()?.iter().filter_map(rule_of).collect())
}

/// Réglages de sécurité d'un objet Guild (forme REST, ou `properties` d'un
/// paquet de la passerelle pour un compte utilisateur) ; `None` s'il n'en porte aucun.
fn safety_of(v: &Value) -> Option<Safety> {
    let field = |k: &str| if v[k].is_null() { &v["properties"][k] } else { &v[k] };
    if field("verification_level").is_null() && field("explicit_content_filter").is_null() {
        return None;
    }
    let inc = field("incidents_data");
    Some(Safety {
        verification: small(field("verification_level")),
        explicit_filter: small(field("explicit_content_filter")),
        notifications: small(field("default_message_notifications")),
        mfa: small(field("mfa_level")),
        invites_disabled_until: time_ms(&inc["invites_disabled_until"]),
        dms_disabled_until: time_ms(&inc["dms_disabled_until"]),
        raid_detected_at: time_ms(&inc["raid_detected_at"]),
    })
}

pub fn decode_safety(body: &str) -> Option<Safety> {
    safety_of(&serde_json::from_str::<Value>(body).ok()?)
}

/// Réponse reçue : l'événement à publier (`None` : réponse sans la forme attendue).
fn answer(cmd: &ModCmd, body: &str) -> Option<ModEvent> {
    Some(match cmd {
        ModCmd::Kick { guild_id, user_id, .. } => ModEvent::Kicked { guild_id: *guild_id, user_id: *user_id },
        ModCmd::Ban { guild_id, user_id, .. } => ModEvent::Banned { guild_id: *guild_id, user_id: *user_id },
        ModCmd::Unban { guild_id, user_id, .. } => ModEvent::Unbanned { guild_id: *guild_id, user_id: *user_id },
        ModCmd::Timeout { guild_id, user_id, until_ms, .. } => {
            // Le membre renvoyé fait foi ; sans corps lisible, la demande.
            let until = serde_json::from_str::<Value>(body).ok().map_or(*until_ms, |v| time_ms(&v["communication_disabled_until"]));
            ModEvent::TimedOut { guild_id: *guild_id, user_id: *user_id, until_ms: until }
        }
        ModCmd::Bans { guild_id, after } => ModEvent::Bans { guild_id: *guild_id, after: *after, bans: decode_bans(body)? },
        ModCmd::BulkBan { guild_id, .. } => {
            let v: Value = serde_json::from_str(body).ok()?;
            ModEvent::BulkBanned { guild_id: *guild_id, banned: ids(&v["banned_users"]), failed: ids(&v["failed_users"]) }
        }
        ModCmd::BulkDelete { .. } => return None,
        ModCmd::Audit { guild_id, filter, before } => ModEvent::Audit { guild_id: *guild_id, filter: *filter, before: *before, entries: decode_audit(body)? },
        ModCmd::Rules { guild_id } => ModEvent::Rules { guild_id: *guild_id, rules: decode_rules(body)? },
        ModCmd::SaveRule { guild_id, .. } => ModEvent::RuleSaved { guild_id: *guild_id, rule: rule_of(&serde_json::from_str(body).ok()?)?, mine: true },
        ModCmd::DeleteRule { guild_id, rule_id } => ModEvent::RuleDeleted { guild_id: *guild_id, rule_id: *rule_id },
        ModCmd::Safety { guild_id } | ModCmd::SetSafety { guild_id, .. } => ModEvent::Safety { guild_id: *guild_id, safety: decode_safety(body)?, mine: true },
        // Réponse partielle : le serveur est relu ensuite (voir `run`).
        ModCmd::Incidents { .. } => return None,
    })
}

async fn call(ctx: &Ctx, req: &Request, max_wait: Duration) -> Result<String, ApiError> {
    let url = format!("{}{}", ctx.api_root, req.path);
    let header = req.reason.as_deref().map(encode_reason);
    let build = |b: reqwest::RequestBuilder| {
        let b = match &header {
            Some(h) => b.header("X-Audit-Log-Reason", h.as_str()),
            None => b,
        };
        match &req.body {
            Some(body) => b.json(body),
            None => b,
        }
    };
    api_req(ctx, req.route, req.method.clone(), &url, build, max_wait, std::future::pending()).await
}

pub(in crate::net) async fn run(ctx: Arc<Ctx>, cmd: ModCmd) {
    let (action, guild_id, key) = (cmd.action(), cmd.guild(), cmd.key());
    let failed = |problem| Event::Moderation(ModEvent::Failed { action, guild_id, key, problem });
    if let ModCmd::BulkDelete { channel_id, ids, now_ms } = &cmd {
        return bulk_delete(&ctx, *channel_id, ids, *now_ms).await;
    }
    let req = request(&cmd);
    let ev = match call(&ctx, &req, MAX_WAIT).await {
        Ok(_) if matches!(cmd, ModCmd::Incidents { .. }) => {
            // Relu en entier : la page montre l'état que Discord a retenu.
            let get = request(&ModCmd::Safety { guild_id: key });
            match call(&ctx, &get, MAX_WAIT).await {
                Ok(body) => decode_safety(&body).map(|safety| Event::Moderation(ModEvent::Safety { guild_id: key, safety, mine: true })).unwrap_or_else(|| failed(Problem::Api(String::new()))),
                Err(e) => e.into_event(failed),
            }
        }
        Ok(body) => answer(&cmd, &body).map(Event::Moderation).unwrap_or_else(|| failed(Problem::Api(String::new()))),
        Err(ApiError::NotFound(p)) => failed(p),
        Err(e) => e.into_event(failed),
    };
    ctx.out.send(ev);
}

/// Messages choisis : groupés quand c'est permis (bot, 2 à 100 de moins de
/// 14 jours), un par un sinon ; chaque message supprimé est retiré tout de suite.
async fn bulk_delete(ctx: &Ctx, channel_id: u64, ids: &[u64], now_ms: i64) {
    let ids: Vec<u64> = ids.iter().copied().take(BULK_DELETE_MAX).collect();
    let fresh = |id: &u64| now_ms - snowflake_ms(*id) < BULK_DELETE_MAX_AGE_MS - AGE_MARGIN_MS;
    let (grouped, singles): (Vec<u64>, Vec<u64>) = if ctx.is_bot() && ids.iter().filter(|i| fresh(i)).count() >= BULK_DELETE_MIN {
        ids.iter().partition(|i| fresh(i))
    } else {
        (Vec::new(), ids.clone())
    };
    let (mut deleted, mut failed, mut problem) = (0usize, 0usize, None);
    let gone = |id: u64| ctx.out.send(Event::MessageDelete { channel_id, id });
    if !grouped.is_empty() {
        let req = Request {
            route: Route { template: "POST /channels/{}/messages/bulk-delete", major: channel_id },
            method: Method::POST,
            path: format!("/channels/{channel_id}/messages/bulk-delete"),
            body: Some(json!({ "messages": grouped.iter().map(u64::to_string).collect::<Vec<_>>() })),
            reason: None,
        };
        match call(ctx, &req, MAX_WAIT).await {
            Ok(_) => {
                deleted += grouped.len();
                grouped.iter().for_each(|i| gone(*i));
            }
            Err(ApiError::Unauthorized) => return ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
            Err(e) => {
                failed += grouped.len();
                problem = Some(e.into_event(Event::Error));
            }
        }
    }
    for (n, id) in singles.iter().enumerate() {
        let req = Request {
            route: Route { template: "DELETE /channels/{}/messages/{}", major: channel_id },
            method: Method::DELETE,
            path: format!("/channels/{channel_id}/messages/{id}"),
            body: None,
            reason: None,
        };
        match call(ctx, &req, DELETE_WAIT).await {
            Ok(_) => {
                deleted += 1;
                gone(*id);
            }
            // Déjà supprimé ailleurs : rien à faire.
            Err(ApiError::NotFound(_)) => gone(*id),
            Err(ApiError::Unauthorized) => return ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
            // Refus ou limite : les suivants subiraient le même sort.
            Err(e) => {
                failed += singles.len() - n;
                problem = Some(e.into_event(Event::Error));
                break;
            }
        }
    }
    let ev = match problem {
        Some(Event::Error(p)) if deleted == 0 => ModEvent::Failed { action: ModAction::BulkDelete, guild_id: None, key: channel_id, problem: p },
        _ => ModEvent::BulkDeleted { channel_id, deleted, failed },
    };
    ctx.out.send(Event::Moderation(ev));
}

// ====================================================================== passerelle

/// Événements de la passerelle utiles à la modération ; les autres ne sont pas décodés.
pub(in crate::net) fn dispatch(kind: &str, d: &str, out: &Out) {
    if !matches!(
        kind,
        "GUILD_AUDIT_LOG_ENTRY_CREATE"
            | "AUTO_MODERATION_RULE_CREATE"
            | "AUTO_MODERATION_RULE_UPDATE"
            | "AUTO_MODERATION_RULE_DELETE"
            | "AUTO_MODERATION_ACTION_EXECUTION"
            | "GUILD_BAN_ADD"
            | "GUILD_BAN_REMOVE"
            | "GUILD_UPDATE"
            | "MESSAGE_DELETE_BULK"
    ) {
        return;
    }
    for e in decode_dispatch(kind, d) {
        out.send(e);
    }
}

pub(crate) fn decode_dispatch(kind: &str, d: &str) -> Vec<Event> {
    let Ok(v) = serde_json::from_str::<Value>(d) else { return Vec::new() };
    let guild = id(&v["guild_id"]);
    let one = |e: Option<ModEvent>| e.map(Event::Moderation).into_iter().collect();
    match kind {
        "GUILD_AUDIT_LOG_ENTRY_CREATE" => one(guild.zip(entry_of(&v, &HashMap::new())).map(|(guild_id, entry)| ModEvent::AuditEntry { guild_id, entry })),
        "AUTO_MODERATION_RULE_CREATE" | "AUTO_MODERATION_RULE_UPDATE" => one(guild.zip(rule_of(&v)).map(|(guild_id, rule)| ModEvent::RuleSaved { guild_id, rule, mine: false })),
        "AUTO_MODERATION_RULE_DELETE" => one(guild.zip(id(&v["id"])).map(|(guild_id, rule_id)| ModEvent::RuleDeleted { guild_id, rule_id })),
        "AUTO_MODERATION_ACTION_EXECUTION" => one(execution_of(&v, guild).map(ModEvent::Execution)),
        "GUILD_BAN_ADD" => one(guild.zip(ban_of(&v)).map(|(guild_id, ban)| ModEvent::BanAdd { guild_id, ban })),
        "GUILD_BAN_REMOVE" => one(guild.zip(id(&v["user"]["id"])).map(|(guild_id, user_id)| ModEvent::BanRemove { guild_id, user_id })),
        "GUILD_UPDATE" => one(id(&v["id"]).zip(safety_of(&v)).map(|(guild_id, safety)| ModEvent::Safety { guild_id, safety, mine: false })),
        "MESSAGE_DELETE_BULK" => match id(&v["channel_id"]) {
            Some(channel_id) => ids(&v["ids"]).into_iter().map(|id| Event::MessageDelete { channel_id, id }).collect(),
            None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn execution_of(v: &Value, guild: Option<u64>) -> Option<Execution> {
    Some(Execution {
        guild_id: guild?,
        rule_id: id(&v["rule_id"])?,
        trigger: small(&v["rule_trigger_type"]).unwrap_or(0),
        action: small(&v["action"]["type"]).unwrap_or(0),
        user_id: id(&v["user_id"])?,
        channel_id: id(&v["channel_id"]),
        content: audit::clip(v["content"].as_str().unwrap_or_default()),
        matched: audit::clip(v["matched_keyword"].as_str().or_else(|| v["matched_content"].as_str()).unwrap_or_default()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::audit::Filter;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn reason_header_is_percent_encoded() {
        assert_eq!(encode_reason("Spam répété !"), "Spam%20r%C3%A9p%C3%A9t%C3%A9%20%21");
        assert_eq!(encode_reason("a-b_c.d~e"), "a-b_c.d~e");
    }

    #[test]
    fn request_shapes() {
        let k = request(&ModCmd::Kick { guild_id: 1, user_id: 2, reason: Some("x".into()) });
        assert_eq!((k.method, k.path.as_str(), k.body, k.reason.as_deref()), (Method::DELETE, "/guilds/1/members/2", None, Some("x")));
        let b = request(&ModCmd::Ban { guild_id: 1, user_id: 2, delete_seconds: 3600, reason: None });
        assert_eq!((b.method, b.path.as_str(), b.body), (Method::PUT, "/guilds/1/bans/2", Some(json!({ "delete_message_seconds": 3600 }))));
        let t = request(&ModCmd::Timeout { guild_id: 1, user_id: 2, until_ms: Some(1_763_164_800_000), reason: None });
        assert_eq!(t.body, Some(json!({ "communication_disabled_until": "2025-11-15T00:00:00.000Z" })));
        let t = request(&ModCmd::Timeout { guild_id: 1, user_id: 2, until_ms: None, reason: None });
        assert_eq!(t.body, Some(json!({ "communication_disabled_until": null })));
        assert_eq!(request(&ModCmd::Bans { guild_id: 1, after: Some(9) }).path, "/guilds/1/bans?limit=1000&after=9");
        let a = request(&ModCmd::Audit { guild_id: 1, filter: Filter { user_id: Some(5), action: Some(22) }, before: Some(7) });
        assert_eq!(a.path, "/guilds/1/audit-logs?limit=50&before=7&user_id=5&action_type=22");
        let bb = request(&ModCmd::BulkBan { guild_id: 1, user_ids: vec![3, 4], delete_seconds: 0, reason: None });
        assert_eq!(bb.body, Some(json!({ "user_ids": ["3", "4"], "delete_message_seconds": 0 })));
        let i = request(&ModCmd::Incidents { guild_id: 1, invites_until_ms: Some(1_763_164_800_000), dms_until_ms: None });
        assert_eq!((i.method, i.body), (Method::PUT, Some(json!({ "invites_disabled_until": "2025-11-15T00:00:00.000Z", "dms_disabled_until": null }))));
        let s = request(&ModCmd::SetSafety { guild_id: 1, change: SafetyChange::ExplicitFilter(2) });
        assert_eq!((s.method, s.body), (Method::PATCH, Some(json!({ "explicit_content_filter": 2 }))));
    }

    #[test]
    fn rule_round_trip_and_body() {
        let rules = decode_rules(&fixture("rest_automod_rules.json")).unwrap_or_default();
        assert_eq!(rules.len(), 3, "la règle illisible est écartée");
        let kw = &rules[0];
        assert_eq!(kw.trigger, Trigger::Keyword { keywords: vec!["arnaque*".into()], regex: vec!["fr[e3]e n[i1]tro".into()], allow: vec!["arnaque-test".into()] });
        assert_eq!(kw.actions, vec![RuleAction::Block { message: Some("Pas de ça ici.".into()) }, RuleAction::Alert { channel_id: 3000000000000000016 }, RuleAction::Timeout { seconds: 60 }]);
        assert_eq!(kw.exempt_roles, vec![2000000000000000501]);
        assert_eq!(rules[1].trigger, Trigger::MentionSpam { limit: 10, raid: true });
        assert_eq!(rules[2].trigger, Trigger::Other(7), "type inconnu gardé tel quel");
        let body = rule_body(kw);
        assert!(body.get("trigger_type").is_none(), "pas de type pour une modification");
        assert_eq!(body["actions"][1], json!({ "type": 2, "metadata": { "channel_id": "3000000000000000016" } }));
        let mut fresh = kw.clone();
        fresh.id = 0;
        assert_eq!(rule_body(&fresh)["trigger_type"], 1);
        assert_eq!(rule_body(&fresh)["event_type"], 1);
    }

    #[test]
    fn audit_entries_with_names_and_changes() {
        let entries = decode_audit(&fixture("rest_audit_log.json")).unwrap_or_default();
        assert_eq!(entries.len(), 3);
        let ban = &entries[0];
        assert_eq!((ban.action, ban.user_name.as_deref(), ban.target_name.as_deref(), ban.reason.as_deref()), (22, Some("Alice"), Some("bob"), Some("spam")));
        let roles = &entries[1];
        assert_eq!(roles.changes[0], Change { key: "$add".into(), old: None, new: Some("Modérateur".into()) });
        let upd = &entries[2];
        assert_eq!(upd.changes[0], Change { key: "name".into(), old: Some("Club".into()), new: Some("Club fictif".into()) });
        assert_eq!(upd.changes[1], Change { key: "verification_level".into(), old: Some("1".into()), new: Some("2".into()) });
    }

    #[test]
    fn bans_and_safety() {
        let bans = decode_bans(&fixture("rest_guild_bans.json")).unwrap_or_default();
        assert_eq!(bans.len(), 2);
        assert_eq!((bans[0].name.as_str(), bans[0].handle.as_str(), bans[0].reason.as_deref()), ("Bob", "bob", Some("spam")));
        assert_eq!((bans[1].handle.as_str(), bans[1].reason.as_deref()), ("vieux#1234", None));
        let s = decode_safety(&fixture("rest_guild_safety.json")).unwrap_or_default();
        assert_eq!((s.verification, s.explicit_filter, s.notifications, s.mfa), (Some(2), Some(1), Some(1), Some(1)));
        assert_eq!(s.invites_disabled_until, Some(1_763_164_800_000));
        assert_eq!(s.dms_disabled_until, None);
    }

    #[test]
    fn gateway_events() {
        let ev = decode_dispatch("MESSAGE_DELETE_BULK", r#"{ "ids": ["5", "6"], "channel_id": "9", "guild_id": "1" }"#);
        assert!(matches!(ev.as_slice(), [Event::MessageDelete { channel_id: 9, id: 5 }, Event::MessageDelete { id: 6, .. }]), "{ev:?}");
        let ev = decode_dispatch("GUILD_BAN_ADD", r#"{ "guild_id": "1", "user": { "id": "2", "username": "bob" } }"#);
        assert!(matches!(ev.as_slice(), [Event::Moderation(ModEvent::BanAdd { guild_id: 1, .. })]), "{ev:?}");
        let packet: Value = serde_json::from_str(&fixture("gateway_automod_execution.json")).unwrap_or_default();
        let ev = decode_dispatch("AUTO_MODERATION_ACTION_EXECUTION", &packet["d"].to_string());
        let [Event::Moderation(ModEvent::Execution(x))] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!((x.action, x.trigger, x.matched.as_str()), (1, 1, "arnaque"));
        // Compte utilisateur : réglages sous `properties`.
        let ev = decode_dispatch("GUILD_UPDATE", r#"{ "id": "1", "properties": { "verification_level": 3, "explicit_content_filter": 0, "mfa_level": 0 } }"#);
        assert!(matches!(ev.as_slice(), [Event::Moderation(ModEvent::Safety { safety: Safety { verification: Some(3), .. }, mine: false, .. })]), "{ev:?}");
        // Sans réglage de sécurité, rien ; paquet illisible, rien.
        assert!(decode_dispatch("GUILD_UPDATE", r#"{ "id": "1", "name": "x" }"#).is_empty());
        assert!(decode_dispatch("GUILD_BAN_ADD", "{ pas du json").is_empty());
        let ev = decode_dispatch("GUILD_AUDIT_LOG_ENTRY_CREATE", r#"{ "guild_id": "1", "id": "8", "action_type": 20, "user_id": "2", "target_id": "3" }"#);
        assert!(matches!(ev.as_slice(), [Event::Moderation(ModEvent::AuditEntry { guild_id: 1, .. })]), "{ev:?}");
    }
}
