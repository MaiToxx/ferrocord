//! Réglages du compte : requêtes REST, décodage des réponses et des
//! événements de la passerelle. Ces points d'API sont réservés aux comptes
//! utilisateur et ne figurent pas dans la documentation de Discord (sauf
//! mention contraire) ; leur forme vient de docs.discord.food et chacun est
//! inscrit dans `docs/passerelle-utilisateur.md`. Tous passent par le
//! limiteur de débit (`rest::api`) et ne partent que d'un geste.

use super::raw::Item;
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::usersettings::notif::{Change, ChannelNotif, GuildNotif, GuildPatch, Level, Mute};
use crate::state::usersettings::extra::{ConsentKind, Consents, ExtraCmd, ExtraEvent, ImageTarget};
use crate::state::usersettings::{b64, AccountCmd, AccountEvent, AuthSession, AuthorizedApp, GuildProfileEdit, LinkedAccount, Profile, ProfileEdit, ProtoReply};
use crate::state::{Event, Problem, ProfileRefusal};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale dans le limiteur avant d'abandonner (l'utilisateur peut refaire le geste).
const MAX_WAIT: Duration = Duration::from_secs(10);
/// Type « Preloaded » des réglages protobuf (docs.discord.food).
const PRELOADED: u8 = 1;

fn ms_of(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso).ok().map(|d| d.timestamp_millis())
}

fn de_flex_id<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    Ok(match Value::deserialize(d)? {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    })
}

#[derive(Deserialize, Default)]
pub(super) struct RawMuteConfig {
    #[serde(default)]
    end_time: Option<String>,
}

#[derive(Deserialize)]
struct RawOverride {
    #[serde(deserialize_with = "de_flex_id")]
    channel_id: Option<u64>,
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    mute_config: Option<RawMuteConfig>,
    #[serde(default = "inherit")]
    message_notifications: u8,
    #[serde(default)]
    flags: u64,
    #[serde(default, deserialize_with = "super::raw::de_lenient")]
    collapsed: Option<bool>,
}

fn inherit() -> u8 {
    3
}

fn mentions() -> u8 {
    1
}

fn mute(on: bool, cfg: Option<RawMuteConfig>) -> Mute {
    Mute { on, until_ms: cfg.and_then(|c| c.end_time).and_then(|t| ms_of(&t)) }
}

/// Objet « User Guild Settings » (docs.discord.food).
#[derive(Deserialize)]
pub(super) struct RawGuildSettings {
    #[serde(default, deserialize_with = "de_flex_id")]
    guild_id: Option<u64>,
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    mute_config: Option<RawMuteConfig>,
    #[serde(default = "mentions")]
    message_notifications: u8,
    #[serde(default)]
    suppress_everyone: bool,
    #[serde(default)]
    suppress_roles: bool,
    #[serde(default)]
    channel_overrides: Vec<Item<RawOverride>>,
    #[serde(default)]
    flags: u64,
    #[serde(default, deserialize_with = "super::raw::de_lenient")]
    hide_muted_channels: Option<bool>,
    #[serde(default, deserialize_with = "super::raw::de_lenient")]
    notify_highlights: Option<u8>,
    #[serde(default, deserialize_with = "super::raw::de_lenient")]
    mute_scheduled_events: Option<bool>,
    #[serde(default, deserialize_with = "super::raw::de_lenient")]
    mobile_push: Option<bool>,
}

impl RawGuildSettings {
    pub fn into_notif(self) -> GuildNotif {
        let overrides: Vec<RawOverride> = self.channel_overrides.into_iter().filter_map(|o| o.0).collect();
        let collapsed = overrides.iter().filter(|o| o.collapsed == Some(true)).filter_map(|o| o.channel_id).collect();
        let channels = overrides
            .into_iter()
            .filter_map(|o| Some((o.channel_id?, ChannelNotif { mute: mute(o.muted, o.mute_config), level: Level::from_code(o.message_notifications), flags: o.flags })))
            .collect();
        GuildNotif {
            guild: self.guild_id,
            mute: mute(self.muted, self.mute_config),
            level: Level::from_code(self.message_notifications),
            suppress_everyone: self.suppress_everyone,
            suppress_roles: self.suppress_roles,
            channels,
            flags: self.flags,
            hide_muted: self.hide_muted_channels.unwrap_or(false),
            collapsed,
            highlights_off: self.notify_highlights == Some(1),
            mute_events: self.mute_scheduled_events.unwrap_or(false),
            mobile_push: self.mobile_push.unwrap_or(true),
        }
    }
}

/// `user_guild_settings` du READY : objet versionné (`entries`, `partial`),
/// ou ancienne forme en tableau.
pub(super) struct RawGuildSettingsList {
    entries: Vec<Item<RawGuildSettings>>,
    partial: bool,
}

#[derive(Deserialize)]
struct RawVersioned {
    #[serde(default)]
    entries: Vec<Item<RawGuildSettings>>,
    #[serde(default)]
    partial: bool,
}

// Pas de `serde(untagged)` : il met le texte en tampon et `Item` (texte brut)
// ne peut plus s'y décoder.
impl<'de> Deserialize<'de> for RawGuildSettingsList {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Box::<serde_json::value::RawValue>::deserialize(d)?;
        if let Ok(v) = serde_json::from_str::<RawVersioned>(raw.get()) {
            return Ok(Self { entries: v.entries, partial: v.partial });
        }
        let entries = serde_json::from_str(raw.get()).map_err(serde::de::Error::custom)?;
        Ok(Self { entries, partial: false })
    }
}

/// Événements de réglages tirés du READY, à publier après `Event::Ready`.
pub(super) fn from_ready(proto: Option<String>, guilds: Option<RawGuildSettingsList>) -> Vec<Event> {
    let mut out = Vec::new();
    // Absents (bot, ou champ omis) : rien n'est supposé, la page proposera une lecture.
    if let Some(p) = proto {
        out.push(Event::Account(AccountEvent::Proto { bytes: b64::decode(&p), partial: false }));
    }
    if let Some(list) = guilds {
        let RawGuildSettingsList { entries, partial } = list;
        let all = entries.into_iter().filter_map(|e| e.0).map(RawGuildSettings::into_notif).collect();
        out.push(Event::Account(AccountEvent::GuildSettings { all, partial }));
    }
    out
}

#[derive(Deserialize)]
struct RawProtoUpdate {
    settings: RawProtoPayload,
    #[serde(default)]
    partial: bool,
}

#[derive(Deserialize)]
struct RawProtoPayload {
    #[serde(rename = "type")]
    kind: u8,
    proto: String,
}

/// Événements de passerelle propres à ce module ; `true` si `kind` en était un.
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) -> bool {
    match kind {
        "USER_SETTINGS_PROTO_UPDATE" => {
            if let Ok(u) = serde_json::from_str::<RawProtoUpdate>(d)
                && u.settings.kind == PRELOADED
            {
                out.send(Event::Account(AccountEvent::Proto { bytes: b64::decode(&u.settings.proto), partial: u.partial }));
            }
            true
        }
        "USER_GUILD_SETTINGS_UPDATE" => {
            if let Ok(g) = serde_json::from_str::<RawGuildSettings>(d) {
                out.send(Event::Account(AccountEvent::GuildSettingsUpdate(g.into_notif())));
            }
            true
        }
        _ => false,
    }
}

#[derive(Deserialize)]
struct RawProtoResponse {
    settings: Option<String>,
    #[serde(default)]
    out_of_date: bool,
}

#[derive(Deserialize)]
struct RawMe {
    #[serde(deserialize_with = "de_flex_id")]
    id: Option<u64>,
    #[serde(default)]
    username: String,
    #[serde(default)]
    global_name: Option<String>,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    phone: Option<String>,
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    mfa_enabled: bool,
    #[serde(default)]
    premium_type: Option<u8>,
    #[serde(default)]
    bio: Option<String>,
    #[serde(default)]
    pronouns: Option<String>,
    #[serde(default)]
    accent_color: Option<u32>,
}

#[derive(Deserialize)]
struct RawSessions {
    #[serde(default)]
    user_sessions: Vec<Item<RawSession>>,
}

#[derive(Deserialize)]
struct RawSession {
    id_hash: String,
    #[serde(default)]
    approx_last_used_time: Option<String>,
    #[serde(default)]
    client_info: RawClientInfo,
}

#[derive(Deserialize, Default)]
struct RawClientInfo {
    os: Option<String>,
    platform: Option<String>,
    location: Option<String>,
}

#[derive(Deserialize)]
struct RawConnection {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    verified: bool,
    #[serde(default)]
    visibility: u8,
    #[serde(default)]
    revoked: bool,
}

#[derive(Deserialize)]
struct RawToken {
    #[serde(deserialize_with = "de_flex_id")]
    id: Option<u64>,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    application: Option<RawApplication>,
}

#[derive(Deserialize)]
struct RawApplication {
    #[serde(default)]
    name: String,
}

pub(super) fn decode_profile(body: &str) -> Result<Profile, Problem> {
    let r: RawMe = serde_json::from_str(body).map_err(|e| Problem::Api(e.to_string()))?;
    Ok(Profile {
        id: r.id.unwrap_or(0),
        username: r.username,
        global_name: r.global_name.filter(|s| !s.is_empty()),
        email: r.email,
        phone: r.phone,
        verified: r.verified,
        mfa: r.mfa_enabled,
        premium: r.premium_type.unwrap_or(0),
        bio: r.bio.unwrap_or_default(),
        pronouns: r.pronouns.unwrap_or_default(),
        accent: r.accent_color,
    })
}

pub(super) fn decode_sessions(body: &str) -> Result<Vec<AuthSession>, Problem> {
    let r: RawSessions = serde_json::from_str(body).map_err(|e| Problem::Api(e.to_string()))?;
    Ok(r.user_sessions
        .into_iter()
        .filter_map(|s| s.0)
        .map(|s| AuthSession {
            id_hash: s.id_hash,
            last_used_ms: s.approx_last_used_time.as_deref().and_then(ms_of),
            os: s.client_info.os,
            platform: s.client_info.platform,
            location: s.client_info.location,
        })
        .collect())
}

pub(super) fn decode_connections(body: &str) -> Result<Vec<LinkedAccount>, Problem> {
    let r: Vec<Item<RawConnection>> = serde_json::from_str(body).map_err(|e| Problem::Api(e.to_string()))?;
    Ok(r.into_iter()
        .filter_map(|c| c.0)
        .map(|c| LinkedAccount { kind: c.kind, name: c.name, verified: c.verified, visible: c.visibility == 1, revoked: c.revoked })
        .collect())
}

pub(super) fn decode_apps(body: &str) -> Result<Vec<AuthorizedApp>, Problem> {
    let r: Vec<Item<RawToken>> = serde_json::from_str(body).map_err(|e| Problem::Api(e.to_string()))?;
    Ok(r.into_iter()
        .filter_map(|t| t.0)
        .filter_map(|t| Some(AuthorizedApp { id: t.id?, name: t.application.map(|a| a.name).unwrap_or_default(), scopes: t.scopes }))
        .collect())
}

fn decode_proto_reply(body: &str) -> Result<ProtoReply, Problem> {
    let r: RawProtoResponse = serde_json::from_str(body).map_err(|e| Problem::Api(e.to_string()))?;
    Ok(ProtoReply { bytes: r.settings.as_deref().and_then(b64::decode), out_of_date: r.out_of_date })
}

fn mute_json(m: &Mute, window_secs: i64) -> Value {
    let end = m.until_ms.and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));
    if m.on {
        json!({ "muted": true, "mute_config": { "end_time": end, "selected_time_window": window_secs } })
    } else {
        json!({ "muted": false, "mute_config": null })
    }
}

/// Corps « Partial User Guild Settings » (docs.discord.food) : seul le
/// réglage changé ; un salon passe par `channel_overrides` (table par identifiant).
pub(super) fn guild_patch_body(p: &GuildPatch) -> Value {
    let inner = match &p.change {
        Change::Mute(m) => mute_json(m, p.window_secs),
        Change::Level(l) => json!({ "message_notifications": l.code() }),
        Change::SuppressEveryone(b) => json!({ "suppress_everyone": b }),
        Change::SuppressRoles(b) => json!({ "suppress_roles": b }),
        Change::Flags(f) => json!({ "flags": f }),
        Change::HideMuted(b) => json!({ "hide_muted_channels": b }),
        // `notify_highlights` : 1 désactivées, 2 activées (docs.discord.food, « Highlight Level »).
        Change::HighlightsOff(b) => json!({ "notify_highlights": if *b { 1 } else { 2 } }),
        Change::MuteEvents(b) => json!({ "mute_scheduled_events": b }),
        Change::MobilePush(b) => json!({ "mobile_push": b }),
        Change::Reset => json!({ "message_notifications": Level::Inherit.code(), "muted": false, "mute_config": null }),
        Change::Collapsed(list) => {
            let overrides: Map<String, Value> = list.iter().map(|(id, on)| (id.to_string(), json!({ "collapsed": on }))).collect();
            json!({ "channel_overrides": overrides })
        }
    };
    match p.channel {
        None => inner,
        Some(c) => {
            let mut overrides = Map::new();
            overrides.insert(c.to_string(), inner);
            json!({ "channel_overrides": overrides })
        }
    }
}

/// Corps de `PATCH /users/@me` (nom affiché) et de `PATCH /users/@me/profile`
/// (bio, pronoms, couleur d'accent, couleurs du thème) ; `None` : rien à
/// envoyer à ce point d'API.
pub(super) fn profile_bodies(e: &ProfileEdit) -> (Option<Value>, Option<Value>) {
    let user = e.global_name.as_ref().map(|n| json!({ "global_name": n }));
    let mut profile = Map::new();
    if let Some(b) = &e.bio {
        profile.insert("bio".into(), json!(b));
    }
    if let Some(p) = &e.pronouns {
        profile.insert("pronouns".into(), json!(p));
    }
    if let Some(a) = e.accent {
        profile.insert("accent_color".into(), json!(a));
    }
    if let Some(t) = e.theme {
        profile.insert("theme_colors".into(), json!(t));
    }
    // docs.discord.food, « Modify User Profile » : `profile_effect_id` (identifiant, ou nul pour le retirer).
    if let Some(x) = e.effect {
        profile.insert("profile_effect_id".into(), json!(x.map(|id| id.to_string())));
    }
    (user, (!profile.is_empty()).then_some(Value::Object(profile)))
}

/// Corps de `PATCH /guilds/{id}/profile/@me` (docs.discord.food, « Modify
/// Guild Member Profile ») : seuls les champs modifiés.
pub(super) fn guild_profile_body(e: &GuildProfileEdit) -> Value {
    let mut body = Map::new();
    if let Some(b) = &e.bio {
        body.insert("bio".into(), json!(b));
    }
    if let Some(p) = &e.pronouns {
        body.insert("pronouns".into(), json!(p));
    }
    if let Some(t) = e.theme {
        body.insert("theme_colors".into(), json!(t));
    }
    Value::Object(body)
}

/// Routes dont les refus sont reconnus par [`refusal`].
const PROFILE_ROUTES: [&str; 4] = ["PATCH /users/@me", "PATCH /users/@me/profile", "PATCH /guilds/{}/members/@me", "PATCH /guilds/{}/profile/@me"];

/// Refus d'une écriture du profil, reconnu au corps d'erreur de Discord
/// (format documenté, « Reference › Error Messages » : `code`, `message`,
/// et `errors.<champ>._errors[] = { code, message }`). Reconnus : la limite
/// de changements (codes en `…RATE_LIMIT`), l'image trop lourde (HTTP 413,
/// codes 40005, 50045 et 50138, ou `…TOO_LARGE` / `BASE_TYPE_MAX_LENGTH` sur
/// le champ d'une image) et ce qui est réservé à Nitro (code en `…PREMIUM…`).
/// Sinon, le message du premier champ refusé, plus précis que « Invalid
/// Form Body » ; `None` : rien de plus à dire que le message général.
pub(super) fn refusal(template: &str, status: u16, body: Option<&Value>) -> Option<Problem> {
    if !PROFILE_ROUTES.contains(&template) {
        return None;
    }
    let top = body.and_then(|b| b["code"].as_u64());
    if status == 413 || matches!(top, Some(40005 | 50045 | 50138)) {
        return Some(Problem::Profile(ProfileRefusal::ImageTooLarge));
    }
    let fields = body?.get("errors")?.as_object()?;
    let mut first = None;
    for (field, v) in fields {
        let image = matches!(field.as_str(), "avatar" | "banner");
        for e in v["_errors"].as_array().into_iter().flatten() {
            let code = e["code"].as_str().unwrap_or_default();
            if code.contains("RATE_LIMIT") {
                return Some(Problem::Profile(if image { ProfileRefusal::ImageRateLimit } else { ProfileRefusal::NameRateLimit }));
            }
            if code.contains("PREMIUM") {
                return Some(Problem::Profile(ProfileRefusal::Nitro));
            }
            if image && (code.contains("TOO_LARGE") || code == "BASE_TYPE_MAX_LENGTH") {
                return Some(Problem::Profile(ProfileRefusal::ImageTooLarge));
            }
            if first.is_none() {
                first = e["message"].as_str().filter(|m| !m.is_empty()).map(|m| Problem::Api(m.to_string()));
            }
        }
    }
    first
}

const fn route(template: &'static str, major: u64) -> Route {
    Route { template, major }
}

async fn call(ctx: &Ctx, r: Route, method: Method, path: &str, body: Option<Value>) -> Result<String, ApiError> {
    let url = format!("{}{path}", ctx.api_root);
    api(ctx, r, method, &url, body, MAX_WAIT, std::future::pending()).await
}

/// Erreur d'une requête : 401 = jeton mort (retour à la connexion), sinon `make`.
fn failed(e: ApiError, make: impl FnOnce(Problem) -> AccountEvent) -> Event {
    e.into_event(|p| Event::Account(make(p)))
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: AccountCmd) {
    let out = &ctx.out;
    let ev = match cmd {
        AccountCmd::LoadProto => match call(&ctx, route("GET /users/@me/settings-proto/{}", 0), Method::GET, "/users/@me/settings-proto/1", None).await {
            // Réponse illisible : réglages inconnus, jamais réécrits.
            Ok(b) => Event::Account(AccountEvent::Proto { bytes: decode_proto_reply(&b).ok().and_then(|r| r.bytes), partial: false }),
            Err(e) => failed(e, AccountEvent::ProtoLoadFailed),
        },
        AccountCmd::PatchProto { id, body, required_version } => {
            let mut payload = json!({ "settings": b64::encode(&body) });
            if let Some(v) = required_version {
                payload["required_data_version"] = json!(v);
            }
            match call(&ctx, route("PATCH /users/@me/settings-proto/{}", 0), Method::PATCH, "/users/@me/settings-proto/1", Some(payload)).await {
                // Réponse illisible : l'écriture a eu lieu, les valeurs envoyées sont gardées.
                Ok(b) => Event::Account(AccountEvent::ProtoWritten { id, result: Ok(decode_proto_reply(&b).unwrap_or(ProtoReply { bytes: None, out_of_date: false })) }),
                Err(e) => failed(e, |p| AccountEvent::ProtoWritten { id, result: Err(p) }),
            }
        }
        AccountCmd::PatchGuild { id, guild, patch } => {
            let path = match guild {
                Some(g) => format!("/users/@me/guilds/{g}/settings"),
                None => "/users/@me/guilds/@me/settings".to_string(),
            };
            match call(&ctx, route("PATCH /users/@me/guilds/{}/settings", guild.unwrap_or(0)), Method::PATCH, &path, Some(guild_patch_body(&patch))).await {
                Ok(b) => {
                    let confirmed = serde_json::from_str::<RawGuildSettings>(&b).ok().map(RawGuildSettings::into_notif).map(|mut g| {
                        g.guild = guild;
                        g
                    });
                    Event::Account(AccountEvent::GuildWritten { id, guild, result: Ok(confirmed) })
                }
                Err(e) => failed(e, |p| AccountEvent::GuildWritten { id, guild, result: Err(p) }),
            }
        }
        AccountCmd::LoadProfile => match call(&ctx, route("GET /users/@me", 0), Method::GET, "/users/@me", None).await {
            Ok(b) => Event::Account(AccountEvent::Profile(decode_profile(&b))),
            Err(e) => failed(e, |p| AccountEvent::Profile(Err(p))),
        },
        AccountCmd::PatchProfile { id, edit } => {
            let (user, profile) = profile_bodies(&edit);
            let mut result = Ok(());
            if let Some(b) = user
                && let Err(e) = call(&ctx, route("PATCH /users/@me", 0), Method::PATCH, "/users/@me", Some(b)).await
            {
                result = Err(e);
            }
            if result.is_ok()
                && let Some(b) = profile
                && let Err(e) = call(&ctx, route("PATCH /users/@me/profile", 0), Method::PATCH, "/users/@me/profile", Some(b)).await
            {
                result = Err(e);
            }
            match result {
                Ok(()) => Event::Account(AccountEvent::ProfileWritten { id, result: Ok(()) }),
                Err(e) => failed(e, |p| AccountEvent::ProfileWritten { id, result: Err(p) }),
            }
        }
        AccountCmd::PatchGuildProfile { id, guild, edit } => {
            let path = format!("/guilds/{guild}/profile/@me");
            match call(&ctx, route("PATCH /guilds/{}/profile/@me", guild), Method::PATCH, &path, Some(guild_profile_body(&edit))).await {
                Ok(_) => Event::Account(AccountEvent::GuildProfileWritten { id, guild, result: Ok(()) }),
                Err(e) => failed(e, |p| AccountEvent::GuildProfileWritten { id, guild, result: Err(p) }),
            }
        }
        // Documenté : Modify Current Member (`nick`).
        AccountCmd::SetNick { guild, nick } => {
            let path = format!("/guilds/{guild}/members/@me");
            match call(&ctx, route("PATCH /guilds/{}/members/@me", guild), Method::PATCH, &path, Some(json!({ "nick": nick }))).await {
                Ok(_) => Event::Account(AccountEvent::NickWritten { guild, result: Ok(()) }),
                Err(e) => failed(e, |p| AccountEvent::NickWritten { guild, result: Err(p) }),
            }
        }
        AccountCmd::LoadSessions => match call(&ctx, route("GET /auth/sessions", 0), Method::GET, "/auth/sessions", None).await {
            Ok(b) => Event::Account(AccountEvent::Sessions(decode_sessions(&b))),
            Err(e) => failed(e, |p| AccountEvent::Sessions(Err(p))),
        },
        AccountCmd::LogoutSession(hash) => {
            let body = json!({ "session_id_hashes": [hash] });
            match call(&ctx, route("POST /auth/sessions/logout", 0), Method::POST, "/auth/sessions/logout", Some(body)).await {
                Ok(_) => Event::Account(AccountEvent::SessionLoggedOut { hash, result: Ok(()) }),
                Err(e) => failed(e, |p| AccountEvent::SessionLoggedOut { hash, result: Err(p) }),
            }
        }
        // Documenté : Get Current User Connections.
        AccountCmd::LoadConnections => match call(&ctx, route("GET /users/@me/connections", 0), Method::GET, "/users/@me/connections", None).await {
            Ok(b) => Event::Account(AccountEvent::Connections(decode_connections(&b))),
            Err(e) => failed(e, |p| AccountEvent::Connections(Err(p))),
        },
        AccountCmd::LoadApps => match call(&ctx, route("GET /oauth2/tokens", 0), Method::GET, "/oauth2/tokens", None).await {
            Ok(b) => Event::Account(AccountEvent::Apps(decode_apps(&b))),
            Err(e) => failed(e, |p| AccountEvent::Apps(Err(p))),
        },
        AccountCmd::RevokeApp(id) => {
            let path = format!("/oauth2/tokens/{id}");
            match call(&ctx, route("DELETE /oauth2/tokens/{}", 0), Method::DELETE, &path, None).await {
                Ok(_) => Event::Account(AccountEvent::AppRevoked { id, result: Ok(()) }),
                Err(e) => failed(e, |p| AccountEvent::AppRevoked { id, result: Err(p) }),
            }
        }
        AccountCmd::Extra(c) => run_extra(&ctx, c).await,
    };
    out.send(ev);
}

/// Corps de `GET`/`POST /users/@me/consent` : `{ "<type>": { "consented": bool } }`.
pub(super) fn decode_consents(body: &str) -> Result<Consents, Problem> {
    #[derive(Deserialize)]
    struct Status {
        #[serde(default)]
        consented: Option<bool>,
    }
    #[derive(Deserialize)]
    struct Raw {
        #[serde(default)]
        usage_statistics: Option<Status>,
        #[serde(default)]
        personalization: Option<Status>,
    }
    let r: Raw = serde_json::from_str(body).map_err(|e| Problem::Api(e.to_string()))?;
    Ok(Consents { usage_statistics: r.usage_statistics.and_then(|s| s.consented), personalization: r.personalization.and_then(|s| s.consented) })
}

/// Corps de Modify User Consents : le type accordé ou retiré, seul.
pub(super) fn consent_body(kind: ConsentKind, on: bool) -> Value {
    if on { json!({ "grant": [kind.code()], "revoke": [] }) } else { json!({ "grant": [], "revoke": [kind.code()] }) }
}

/// Chemin et corps d'un envoi d'avatar ou de bannière (`null` : retirée).
pub(super) fn image_request(target: ImageTarget, data: Option<&str>) -> (String, Value) {
    let mut body = Map::new();
    body.insert(target.kind.field().into(), data.map_or(Value::Null, |d| json!(d)));
    let path = match target.guild {
        Some(g) => format!("/guilds/{g}/members/@me"),
        None => "/users/@me".to_string(),
    };
    (path, Value::Object(body))
}

async fn run_extra(ctx: &Ctx, cmd: ExtraCmd) -> Event {
    let ev = |e: ExtraEvent| Event::Account(AccountEvent::Extra(e));
    match cmd {
        ExtraCmd::LoadConsents => match call(ctx, route("GET /users/@me/consent", 0), Method::GET, "/users/@me/consent", None).await {
            Ok(b) => ev(ExtraEvent::Consents(decode_consents(&b))),
            Err(e) => failed(e, |p| AccountEvent::Extra(ExtraEvent::Consents(Err(p)))),
        },
        ExtraCmd::SetConsent { kind, on } => match call(ctx, route("POST /users/@me/consent", 0), Method::POST, "/users/@me/consent", Some(consent_body(kind, on))).await {
            // Réponse illisible : l'écriture a eu lieu, la valeur envoyée est gardée.
            Ok(b) => ev(ExtraEvent::ConsentWritten { kind, result: Ok(decode_consents(&b).unwrap_or_default()) }),
            Err(e) => failed(e, |p| AccountEvent::Extra(ExtraEvent::ConsentWritten { kind, result: Err(p) })),
        },
        // La réponse (un utilisateur, avec parfois un champ `token`) n'est pas lue :
        // la passerelle annonce le nouvel avatar.
        ExtraCmd::SetImage { target, data } => {
            let removed = data.is_none();
            let (path, body) = image_request(target, data.as_deref());
            let r = match target.guild {
                Some(g) => route("PATCH /guilds/{}/members/@me", g),
                None => route("PATCH /users/@me", 0),
            };
            match call(ctx, r, Method::PATCH, &path, Some(body)).await {
                Ok(_) => ev(ExtraEvent::ImageWritten { target, removed, result: Ok(()) }),
                Err(e) => failed(e, |p| AccountEvent::Extra(ExtraEvent::ImageWritten { target, removed, result: Err(p) })),
            }
        }
    }
}

/// Réglages par serveur, indexés pour les tests.
#[cfg(test)]
fn by_guild(events: &[Event]) -> std::collections::HashMap<Option<u64>, GuildNotif> {
    let mut m = std::collections::HashMap::new();
    for e in events {
        if let Event::Account(AccountEvent::GuildSettings { all, .. }) = e {
            for g in all {
                m.insert(g.guild, g.clone());
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn guild_settings_of_the_ready() {
        let v: Value = serde_json::from_str(&fixture("user_guild_settings")).unwrap();
        let list: RawGuildSettingsList = serde_json::from_str(&v["d"]["user_guild_settings"].to_string()).unwrap();
        let events = from_ready(None, Some(list));
        let m = by_guild(&events);
        let g = &m[&Some(5000000000000000001)];
        assert!(g.mute.on && g.mute.until_ms == Some(1_893_456_000_000));
        assert_eq!(g.level, Level::All);
        assert!(g.suppress_everyone && !g.suppress_roles);
        let c = &g.channels[&5000000000000000011];
        assert!(c.mute.on && c.mute.until_ms.is_none());
        assert_eq!(c.level, Level::Nothing);
        assert!(!g.hide_muted);
        assert_eq!(g.collapsed, [5000000000000000012].into());
        // Messages privés (guild_id null), champs absents : valeurs par défaut.
        let dm = &m[&None];
        assert!(!dm.mute.on);
        assert_eq!(dm.level, Level::Mentions);
        // Entrée de forme inattendue : ignorée, les autres restent.
        assert_eq!(m.len(), 2);
        // Ancienne forme : tableau.
        let plain: RawGuildSettingsList = serde_json::from_str(r#"[{"guild_id":"7","muted":true}]"#).unwrap();
        assert!(by_guild(&from_ready(None, Some(plain)))[&Some(7)].mute.on);
    }

    #[test]
    fn settings_proto_update_event() {
        let (tx, rx) = std::sync::mpsc::channel();
        let out = Out { tx, wake: Arc::new(|| {}), people: Default::default(), watch: Default::default(), interest: Default::default() };
        assert!(dispatch("USER_SETTINGS_PROTO_UPDATE", &serde_json::from_str::<Value>(&fixture("user_settings_proto_update")).unwrap()["d"].to_string(), &out));
        // Autre type (frecency) : ignoré.
        assert!(dispatch("USER_SETTINGS_PROTO_UPDATE", r#"{"settings":{"type":2,"proto":"CAE="},"partial":false}"#, &out));
        assert!(dispatch("USER_GUILD_SETTINGS_UPDATE", r#"{"guild_id":"9","muted":false,"message_notifications":2}"#, &out));
        assert!(!dispatch("MESSAGE_CREATE", "{}", &out));
        drop(out);
        let events: Vec<Event> = rx.try_iter().collect();
        assert_eq!(events.len(), 2);
        let Event::Account(AccountEvent::Proto { bytes: Some(b), partial: true }) = &events[0] else { panic!("{events:?}") };
        assert!(crate::state::usersettings::proto::Msg::decode(b).is_ok());
        let Event::Account(AccountEvent::GuildSettingsUpdate(g)) = &events[1] else { panic!() };
        assert_eq!((g.guild, g.level), (Some(9), Level::Nothing));
    }

    #[test]
    fn rest_bodies() {
        let mute = GuildPatch { channel: None, change: Change::Mute(Mute { on: true, until_ms: Some(0) }), window_secs: 900 };
        assert_eq!(guild_patch_body(&mute), json!({ "muted": true, "mute_config": { "end_time": "1970-01-01T00:00:00.000Z", "selected_time_window": 900 } }));
        let forever = GuildPatch { channel: Some(42), change: Change::Mute(Mute { on: true, until_ms: None }), window_secs: -1 };
        assert_eq!(guild_patch_body(&forever), json!({ "channel_overrides": { "42": { "muted": true, "mute_config": { "end_time": null, "selected_time_window": -1 } } } }));
        let unmute = GuildPatch { channel: None, change: Change::Mute(Mute::default()), window_secs: 0 };
        assert_eq!(guild_patch_body(&unmute), json!({ "muted": false, "mute_config": null }));
        let level = GuildPatch { channel: None, change: Change::Level(Level::Nothing), window_secs: 0 };
        assert_eq!(guild_patch_body(&level), json!({ "message_notifications": 2 }));
        // « Parcourir les salons » : drapeaux du serveur, puis d'un salon suivi.
        let only_followed = GuildPatch { channel: None, change: Change::Flags(1 << 14), window_secs: 0 };
        assert_eq!(guild_patch_body(&only_followed), json!({ "flags": 16384 }));
        let follow = GuildPatch { channel: Some(42), change: Change::Flags(1 << 12), window_secs: 0 };
        assert_eq!(guild_patch_body(&follow), json!({ "channel_overrides": { "42": { "flags": 4096 } } }));
        let g: RawGuildSettings = serde_json::from_str(r#"{ "guild_id": "9", "flags": 16384, "channel_overrides": [ { "channel_id": "42", "flags": 4096 } ] }"#).unwrap_or_else(|e| panic!("{e}"));
        let g = g.into_notif();
        assert!(g.opt_in() && g.followed(42, None) && !g.followed(43, None));
        // Liste des salons : salons muets masqués, catégories repliées (une ou plusieurs par requête).
        let reset = GuildPatch { channel: Some(42), change: Change::Reset, window_secs: 0 };
        assert_eq!(guild_patch_body(&reset), json!({ "channel_overrides": { "42": { "message_notifications": 3, "muted": false, "mute_config": null } } }));
        let hide = GuildPatch { channel: None, change: Change::HideMuted(true), window_secs: 0 };
        assert_eq!(guild_patch_body(&hide), json!({ "hide_muted_channels": true }));
        // Les trois interrupteurs de la fenêtre de notifications (F12).
        let body = |c: Change| guild_patch_body(&GuildPatch { channel: None, change: c, window_secs: 0 });
        assert_eq!(body(Change::HighlightsOff(true)), json!({ "notify_highlights": 1 }));
        assert_eq!(body(Change::HighlightsOff(false)), json!({ "notify_highlights": 2 }));
        assert_eq!(body(Change::MuteEvents(true)), json!({ "mute_scheduled_events": true }));
        assert_eq!(body(Change::MobilePush(false)), json!({ "mobile_push": false }));
        let fold = GuildPatch { channel: None, change: Change::Collapsed(vec![(42, true), (43, false)]), window_secs: 0 };
        assert_eq!(guild_patch_body(&fold), json!({ "channel_overrides": { "42": { "collapsed": true }, "43": { "collapsed": false } } }));
        let g: RawGuildSettings = serde_json::from_str(r#"{ "guild_id": "9", "hide_muted_channels": true, "channel_overrides": [ { "channel_id": "42", "collapsed": true }, { "channel_id": "43", "collapsed": "oui" } ] }"#).unwrap_or_else(|e| panic!("{e}"));
        let g = g.into_notif();
        assert!(g.hide_muted);
        assert_eq!(g.collapsed, [42].into(), "une valeur illisible ne replie rien");

        let (u, p) = profile_bodies(&ProfileEdit { global_name: Some(None), bio: Some("b".into()), ..Default::default() });
        assert_eq!(u, Some(json!({ "global_name": null })));
        assert_eq!(p, Some(json!({ "bio": "b" })));
        assert_eq!(profile_bodies(&ProfileEdit { accent: Some(Some(0xff0000)), ..Default::default() }), (None, Some(json!({ "accent_color": 16711680 }))));
        // Couleurs du thème : deux entiers, ou `null` pour les retirer.
        assert_eq!(profile_bodies(&ProfileEdit { theme: Some(Some([0x112233, 0x445566])), ..Default::default() }).1, Some(json!({ "theme_colors": [1122867, 4478310] })));
        assert_eq!(profile_bodies(&ProfileEdit { theme: Some(None), ..Default::default() }).1, Some(json!({ "theme_colors": null })));
        let g = GuildProfileEdit { bio: Some("ici".into()), pronouns: Some(String::new()), theme: Some(None) };
        assert_eq!(guild_profile_body(&g), json!({ "bio": "ici", "pronouns": "", "theme_colors": null }));
        assert_eq!(guild_profile_body(&GuildProfileEdit { pronouns: Some("iel".into()), ..Default::default() }), json!({ "pronouns": "iel" }));
    }

    #[test]
    fn profile_refusals_are_recognised() {
        let all: Value = serde_json::from_str(&fixture("rest_profile_errors")).unwrap();
        let case = |key: &str| all.as_array().and_then(|l| l.iter().find(|c| c["name"] == key)).map(|c| c["response"].clone()).unwrap_or_else(|| panic!("cas {key} absent"));
        let of = |route: &str, status: u16, key: &str| refusal(route, status, Some(&case(key)));
        let p = |r| Some(Problem::Profile(r));
        assert_eq!(of("PATCH /users/@me", 400, "name_rate_limit"), p(ProfileRefusal::NameRateLimit));
        assert_eq!(of("PATCH /guilds/{}/members/@me", 400, "nick_rate_limit"), p(ProfileRefusal::NameRateLimit));
        assert_eq!(of("PATCH /users/@me", 400, "avatar_rate_limit"), p(ProfileRefusal::ImageRateLimit));
        assert_eq!(of("PATCH /users/@me", 400, "avatar_too_large"), p(ProfileRefusal::ImageTooLarge));
        assert_eq!(of("PATCH /users/@me", 400, "entity_too_large"), p(ProfileRefusal::ImageTooLarge));
        assert_eq!(of("PATCH /users/@me/profile", 400, "theme_premium"), p(ProfileRefusal::Nitro));
        assert_eq!(of("PATCH /guilds/{}/profile/@me", 400, "theme_premium"), p(ProfileRefusal::Nitro));
        // Autre refus d'un champ : son message, pas « Invalid Form Body ».
        assert_eq!(of("PATCH /users/@me/profile", 400, "bio_too_long"), Some(Problem::Api("Must be 190 or fewer in length.".into())));
        // Rien de reconnu, ou une autre route : le message général reste.
        assert_eq!(of("PATCH /users/@me", 400, "plain"), None);
        assert_eq!(of("PATCH /guilds/{}", 400, "name_rate_limit"), None);
        // 413 du pare-feu, sans corps JSON.
        assert_eq!(refusal("PATCH /users/@me", 413, None), p(ProfileRefusal::ImageTooLarge));
        assert_eq!(refusal("PATCH /users/@me", 500, None), None);
    }

    #[test]
    fn rest_responses() {
        let p = decode_profile(&fixture("rest_me")).unwrap();
        assert_eq!((p.username.as_str(), p.global_name.as_deref()), ("utilisateur.test", Some("Utilisateur Test")));
        assert_eq!(p.email.as_deref(), Some("test@example.invalid"));
        assert!(p.mfa && p.verified);
        assert_eq!((p.premium, p.accent), (2, Some(0x5865f2)));
        let s = decode_sessions(&fixture("rest_auth_sessions")).unwrap();
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].platform.as_deref(), Some("Chrome"));
        assert!(s[0].last_used_ms.is_some() && s[1].os.is_none());
        let c = decode_connections(&fixture("rest_connections")).unwrap();
        assert_eq!((c[0].kind.as_str(), c[0].visible, c[1].visible), ("github", true, false));
        let a = decode_apps(&fixture("rest_oauth2_tokens")).unwrap();
        assert_eq!((a[0].id, a[0].name.as_str(), a[0].scopes.len()), (5000000000000000701, "Appli Test", 2));
        let r = decode_proto_reply(r#"{"settings":"CAE=","out_of_date":true}"#).unwrap();
        assert_eq!(r, ProtoReply { bytes: Some(vec![8, 1]), out_of_date: true });
        assert!(decode_profile("pas du json").is_err());
    }

    #[test]
    fn consents_and_images() {
        use crate::state::crop::ImageKind;
        let c = decode_consents(&fixture("rest_consent")).unwrap();
        assert_eq!(c, Consents { usage_statistics: Some(false), personalization: Some(true) });
        assert_eq!(decode_consents(r#"{"type_futur":{"consented":true}}"#).unwrap(), Consents::default(), "type inconnu ignoré");
        assert_eq!(consent_body(ConsentKind::UsageStatistics, false), json!({ "grant": [], "revoke": ["usage_statistics"] }));
        assert_eq!(consent_body(ConsentKind::Personalization, true), json!({ "grant": ["personalization"], "revoke": [] }));
        let me = ImageTarget { kind: ImageKind::Avatar, guild: None };
        assert_eq!(image_request(me, Some("data:image/png;base64,AA==")), ("/users/@me".to_string(), json!({ "avatar": "data:image/png;base64,AA==" })));
        let banner = ImageTarget { kind: ImageKind::Banner, guild: Some(7) };
        assert_eq!(image_request(banner, None), ("/guilds/7/members/@me".to_string(), json!({ "banner": null })));
    }
}
