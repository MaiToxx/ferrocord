//! Commandes et composants des applications (lot T11) : index des
//! commandes, `POST /interactions`, événements de la passerelle, composants
//! des messages.
//!
//! Sources (vérifiées le 13 novembre 2026 sur docs.discord.food) :
//! - « Application Commands » : Get Guild / Channel / User Application
//!   Command Index (`GET /guilds/{id}/application-command-index`,
//!   `GET /channels/{id}/application-command-index`,
//!   `GET /users/@me/application-command-index`), objets Application Command,
//!   Option, Choice, Command Index Application ;
//! - « Receiving and Responding » : Create Interaction (`POST /interactions`,
//!   `type`, `application_id`, `guild_id`, `channel_id`, `message_id`,
//!   `message_flags`, `session_id`, `nonce`, `data`), Sendable Application
//!   Command Data, Sendable Message Component Data, Sendable Modal Submit
//!   Data, Modal Submit Component Data ; réponse 204 ;
//! - « Gateway Events » : INTERACTION_CREATE, INTERACTION_SUCCESS,
//!   INTERACTION_FAILURE (`reason_code`), INTERACTION_MODAL_CREATE,
//!   APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE ;
//! - « Message Components » : types 1 à 23, styles de boutons, champs ;
//! - « Message » : drapeaux, `interaction_metadata`, `application_id`.
//!
//! Requêtes réservées aux comptes utilisateur, autorisées par l'utilisateur
//! (PLAN.md 1.2.5) ; jamais envoyées par un compte bot. Chaque champ est lu
//! à part : une forme inattendue ne fait échouer que lui.

use super::raw::{de_lenient, de_loose_id, Item};
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::interactions::*;
use crate::state::model::Attachment;
use crate::state::social::Emoji;
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale avant de partir : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(10);
/// Profondeur maximale des composants et des options lus (les formes
/// documentées ne dépassent pas 3).
const MAX_DEPTH: u8 = 6;
/// Composants lus au plus à chaque niveau.
const MAX_CHILDREN: usize = 40;

fn id_of(v: &Value) -> Option<u64> {
    match v {
        Value::String(s) => s.parse().ok(),
        Value::Number(n) => n.as_u64(),
        _ => None,
    }
}

fn text(v: &Value) -> Option<String> {
    v.as_str().map(str::to_string)
}

fn small(v: &Value) -> Option<u8> {
    v.as_u64().and_then(|n| u8::try_from(n).ok())
}

// ====================================================================== index

#[derive(Deserialize)]
struct RawIndex {
    // Obligatoires : une réponse d'une autre forme ne passe pas pour un index vide.
    applications: Vec<Item<RawApp>>,
    application_commands: Vec<Item<Value>>,
}

#[derive(Deserialize)]
struct RawApp {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    bot_id: Option<u64>,
}

fn choice(v: &Value) -> Option<Choice> {
    let name = v.get("name_localized").and_then(text).or_else(|| v.get("name").and_then(text))?;
    let value = match v.get("value")? {
        Value::String(s) => ChoiceValue::Str(s.clone()),
        Value::Number(n) => match n.as_i64() {
            Some(i) => ChoiceValue::Int(i),
            None => ChoiceValue::Num(n.as_f64().filter(|x| x.is_finite())?),
        },
        _ => return None,
    };
    Some(Choice { name, value })
}

fn localized(v: &Value, key: &str) -> String {
    v.get(format!("{key}_localized")).and_then(text).or_else(|| v.get(key).and_then(text)).unwrap_or_default()
}

fn option(v: &Value, depth: u8) -> Option<CmdOption> {
    let kind = OptionKind::from_code(v.get("type").and_then(small)?);
    let name = v.get("name").and_then(text)?;
    let list = |key: &str| v.get(key).and_then(Value::as_array).map(|a| a.iter().take(MAX_CHILDREN).collect::<Vec<_>>()).unwrap_or_default();
    let options = if depth < MAX_DEPTH { list("options").into_iter().filter_map(|o| option(o, depth + 1)).collect() } else { Vec::new() };
    let num = |key: &str| v.get(key).and_then(Value::as_f64).filter(|x| x.is_finite());
    let len = |key: &str| v.get(key).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
    Some(CmdOption {
        kind,
        description: localized(v, "description"),
        name,
        required: v.get("required").and_then(Value::as_bool).unwrap_or(false),
        choices: list("choices").into_iter().filter_map(choice).collect(),
        options,
        channel_types: list("channel_types").into_iter().filter_map(small).collect(),
        min_value: num("min_value"),
        max_value: num("max_value"),
        min_length: len("min_length"),
        max_length: len("max_length"),
        autocomplete: v.get("autocomplete").and_then(Value::as_bool).unwrap_or(false),
    })
}

fn command(v: &Value) -> Option<Command> {
    Some(Command {
        id: v.get("id").and_then(id_of)?,
        app_id: v.get("application_id").and_then(id_of)?,
        version: v.get("version").and_then(id_of).unwrap_or(0),
        kind: CommandKind::from_code(v.get("type").and_then(small).unwrap_or(1)),
        name: v.get("name").and_then(text)?,
        description: localized(v, "description"),
        options: v.get("options").and_then(Value::as_array).map(|a| a.iter().take(MAX_CHILDREN).filter_map(|o| option(o, 0)).collect()).unwrap_or_default(),
    })
}

/// Réponse d'un `GET …/application-command-index` ; `None` si illisible.
pub(super) fn index_of(body: &str) -> Option<Index> {
    let raw: RawIndex = serde_json::from_str(body).ok()?;
    let apps = raw.applications.into_iter().filter_map(|a| a.0).filter_map(|a| Some(AppInfo { id: a.id?, name: a.name.unwrap_or_default(), bot_id: a.bot_id })).collect();
    let commands = raw.application_commands.iter().filter_map(|c| c.0.as_ref()).filter_map(command).collect();
    Some(Index { apps, commands })
}

// ====================================================================== composants

fn emoji(v: &Value) -> Option<Emoji> {
    match (v.get("id").and_then(id_of), v.get("name").and_then(Value::as_str).filter(|n| !n.is_empty())) {
        (Some(id), name) => Some(Emoji::Custom { id, name: name.unwrap_or_default().to_string(), animated: v.get("animated").and_then(Value::as_bool).unwrap_or(false) }),
        (None, Some(n)) => Some(Emoji::Unicode(n.to_string())),
        (None, None) => None,
    }
}

/// Clé d'image stable pour un média sans identifiant de pièce jointe (le
/// bit haut le sépare des vrais identifiants).
fn url_key(url: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in url.bytes() {
        h = (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
    }
    h | (1 << 63)
}

/// Média « déplié » d'un composant (`media`, `file`) : pièce jointe du
/// message (`attachment://nom`), ou adresse servie par le mandataire de Discord.
fn media(item: &Value, description: Option<String>, spoiler: bool, atts: &[Attachment]) -> Option<Attachment> {
    let url = item.get("url").and_then(Value::as_str)?;
    if let Some(name) = url.strip_prefix("attachment://") {
        let found = atts.iter().find(|a| a.name == name).or_else(|| item.get("attachment_id").and_then(id_of).and_then(|id| atts.iter().find(|a| a.id == id)));
        if let Some(a) = found {
            let mut a = a.clone();
            a.spoiler |= spoiler;
            if description.is_some() {
                a.description = description;
            }
            return Some(a);
        }
    }
    let dim = |k: &str| item.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok()).unwrap_or(0);
    let name = url.rsplit('/').next().and_then(|n| n.split('?').next()).unwrap_or_default().to_string();
    let a = Attachment {
        id: item.get("attachment_id").and_then(id_of).unwrap_or_else(|| url_key(url)),
        name,
        url: url.to_string(),
        proxy_url: item.get("proxy_url").and_then(text).unwrap_or_default(),
        content_type: item.get("content_type").and_then(text),
        width: dim("width"),
        height: dim("height"),
        description: description.filter(|d| !d.trim().is_empty()),
        ..Attachment::default()
    }
    .analysed(0);
    Some(Attachment { spoiler: a.spoiler || spoiler, ..a })
}

fn children(v: &Value, key: &str, depth: u8, atts: &[Attachment]) -> Vec<Component> {
    v.get(key).and_then(Value::as_array).map(|a| a.iter().take(MAX_CHILDREN).map(|c| component(c, depth + 1, atts)).filter(|c| *c != Component::Unknown).collect()).unwrap_or_default()
}

/// Un composant ; `Unknown` si son type est inconnu ou illisible.
pub(super) fn component(v: &Value, depth: u8, atts: &[Attachment]) -> Component {
    if depth > MAX_DEPTH {
        return Component::Unknown;
    }
    let s = |k: &str| v.get(k).and_then(text);
    let b = |k: &str, d: bool| v.get(k).and_then(Value::as_bool).unwrap_or(d);
    let count = |k: &str| v.get(k).and_then(small).unwrap_or(1);
    match v.get("type").and_then(small) {
        Some(1) => Component::Row(children(v, "components", depth, atts)),
        Some(2) => {
            let style = match v.get("style").and_then(small) {
                Some(1) => ButtonStyle::Primary,
                Some(3) => ButtonStyle::Success,
                Some(4) => ButtonStyle::Danger,
                Some(5) => ButtonStyle::Link,
                Some(6) => ButtonStyle::Premium,
                _ => ButtonStyle::Secondary,
            };
            Component::Button(Button { style, label: s("label").unwrap_or_default(), emoji: v.get("emoji").and_then(emoji), custom_id: s("custom_id"), url: s("url"), disabled: b("disabled", false) })
        }
        Some(t @ (3 | 5 | 6 | 7 | 8)) => {
            let kind = match t {
                3 => SelectKind::String,
                5 => SelectKind::User,
                6 => SelectKind::Role,
                7 => SelectKind::Mentionable,
                _ => SelectKind::Channel,
            };
            let Some(custom_id) = s("custom_id") else { return Component::Unknown };
            let options = v
                .get("options")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .take(25)
                        .filter_map(|o| {
                            Some(SelectOption {
                                label: o.get("label").and_then(text)?,
                                value: o.get("value").and_then(text)?,
                                description: o.get("description").and_then(text),
                                emoji: o.get("emoji").and_then(emoji),
                                default: o.get("default").and_then(Value::as_bool).unwrap_or(false),
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let channel_types = v.get("channel_types").and_then(Value::as_array).map(|a| a.iter().filter_map(small).collect()).unwrap_or_default();
            Component::Select(Select {
                kind,
                custom_id,
                placeholder: s("placeholder"),
                min_values: v.get("min_values").and_then(small).unwrap_or(1),
                max_values: count("max_values").max(1),
                options,
                disabled: b("disabled", false),
                required: b("required", true),
                channel_types,
            })
        }
        Some(4) => {
            let Some(custom_id) = s("custom_id") else { return Component::Unknown };
            let len = |k: &str| v.get(k).and_then(Value::as_u64).and_then(|n| u32::try_from(n).ok());
            Component::TextInput(TextInput {
                custom_id,
                paragraph: v.get("style").and_then(small) == Some(2),
                label: s("label").unwrap_or_default(),
                min_length: len("min_length"),
                max_length: len("max_length"),
                required: b("required", true),
                value: s("value").unwrap_or_default(),
                placeholder: s("placeholder"),
            })
        }
        Some(9) => {
            let accessory = v.get("accessory").map(|a| component(a, depth + 1, atts)).filter(|a| *a != Component::Unknown).map(Box::new);
            Component::Section { texts: children(v, "components", depth, atts), accessory }
        }
        Some(10) => Component::Text(s("content").unwrap_or_default()),
        Some(11) => match v.get("media").and_then(|m| media(m, s("description"), b("spoiler", false), atts)) {
            Some(a) => Component::Thumbnail(Box::new(a)),
            None => Component::Unknown,
        },
        Some(12) => {
            let items = v.get("items").and_then(Value::as_array).map(|a| {
                a.iter().take(10).filter_map(|i| media(i.get("media")?, i.get("description").and_then(text), i.get("spoiler").and_then(Value::as_bool).unwrap_or(false), atts)).collect::<Vec<_>>()
            });
            match items {
                Some(items) if !items.is_empty() => Component::Gallery(items),
                _ => Component::Unknown,
            }
        }
        Some(13) => match v.get("file").and_then(|f| media(f, None, b("spoiler", false), atts)) {
            Some(mut a) => {
                if let Some(n) = s("name") {
                    a.name = n;
                }
                if let Some(size) = v.get("size").and_then(Value::as_u64) {
                    a.size = size;
                }
                Component::File(Box::new(a))
            }
            None => Component::Unknown,
        },
        Some(14) => Component::Separator { divider: b("divider", true), large: v.get("spacing").and_then(small) == Some(2) },
        Some(17) => Component::Container {
            accent: v.get("accent_color").and_then(Value::as_u64).and_then(|c| u32::try_from(c & 0xFF_FFFF).ok()),
            spoiler: b("spoiler", false),
            children: children(v, "components", depth, atts),
        },
        Some(18) => match v.get("component").map(|c| component(c, depth + 1, atts)) {
            Some(child) if child != Component::Unknown => Component::Label { label: s("label").unwrap_or_default(), description: s("description"), child: Box::new(child) },
            _ => Component::Unknown,
        },
        _ => Component::Unknown,
    }
}

fn components(v: Option<&Value>, atts: &[Attachment]) -> Vec<Component> {
    v.and_then(Value::as_array).map(|a| a.iter().take(MAX_CHILDREN).map(|c| component(c, 0, atts)).filter(|c| *c != Component::Unknown).collect()).unwrap_or_default()
}

fn user_name(u: &Value) -> String {
    u.get("global_name").and_then(text).filter(|n| !n.is_empty()).or_else(|| u.get("username").and_then(text)).unwrap_or_default()
}

/// Interaction d'origine : `interaction_metadata`, ou l'ancien `interaction`.
fn interaction_of(meta: Option<&Value>, old: Option<&Value>) -> Option<InteractionMeta> {
    let v = meta.filter(|v| v.is_object()).or(old.filter(|v| v.is_object()))?;
    let user = v.get("user")?;
    // Seules les commandes ont un nom ; une réponse à un bouton ne montre pas d'en-tête.
    let command = (v.get("type").and_then(Value::as_u64) == Some(2)).then(|| v.get("name").and_then(text)).flatten();
    Some(InteractionMeta { user_id: user.get("id").and_then(id_of)?, user_name: user_name(user), command })
}

/// Ce qui fait d'un message une réponse d'application. Message à composants
/// v2 : ses pièces jointes montrées par les composants quittent `atts`.
pub(super) fn app_meta(flags: Option<u64>, comps: Option<&Value>, meta: Option<&Value>, old: Option<&Value>, application_id: Option<u64>, atts: &mut Vec<Attachment>) -> Option<Box<AppMeta>> {
    let app = AppMeta { flags: flags.unwrap_or(0), components: components(comps, atts), interaction: interaction_of(meta, old), application_id };
    if app.is_empty() {
        return None;
    }
    if app.v2() {
        let mut shown = Vec::new();
        Component::media_ids(&app.components, &mut shown);
        atts.retain(|a| !shown.contains(&a.id));
    }
    Some(Box::new(app))
}

// ====================================================================== passerelle

#[derive(Deserialize)]
struct RawAck {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    nonce: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    reason_code: Option<u32>,
}

#[derive(Deserialize)]
struct RawAppUpdate {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    flags: Option<u64>,
    #[serde(default)]
    components: Option<Value>,
    #[serde(default)]
    interaction_metadata: Option<Value>,
    #[serde(default)]
    interaction: Option<Value>,
    #[serde(default, deserialize_with = "de_loose_id")]
    application_id: Option<u64>,
    #[serde(default)]
    edited_timestamp: Option<Value>,
    #[serde(default, deserialize_with = "de_lenient")]
    attachments: Option<Vec<Item<super::media::RawAttachment>>>,
}

pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    for e in decode(kind, d) {
        out.send(e);
    }
}

fn decode(kind: &str, d: &str) -> Vec<Event> {
    let ev = |e: InteractionEvent| vec![Event::Interactions(e)];
    let ack = || serde_json::from_str::<RawAck>(d).ok();
    match kind {
        "INTERACTION_CREATE" => match ack() {
            Some(RawAck { id: Some(id), nonce: Some(nonce), .. }) => ev(InteractionEvent::Created { nonce, id }),
            _ => Vec::new(),
        },
        "INTERACTION_SUCCESS" => match ack() {
            Some(a) if a.id.is_some() || a.nonce.is_some() => ev(InteractionEvent::Succeeded { nonce: a.nonce.unwrap_or(0), id: a.id.unwrap_or(0) }),
            _ => Vec::new(),
        },
        "INTERACTION_FAILURE" => match ack() {
            Some(a) if a.id.is_some() || a.nonce.is_some() => ev(InteractionEvent::Failed { nonce: a.nonce.unwrap_or(0), id: a.id.unwrap_or(0), reason: a.reason_code.unwrap_or(1) }),
            _ => Vec::new(),
        },
        "INTERACTION_MODAL_CREATE" => {
            let Ok(v) = serde_json::from_str::<Value>(d) else { return Vec::new() };
            let (Some(id), Some(channel_id), Some(custom_id)) = (v.get("id").and_then(id_of), v.get("channel_id").and_then(id_of), v.get("custom_id").and_then(text)) else { return Vec::new() };
            let app = v.get("application");
            let Some(app_id) = app.and_then(|a| a.get("id")).and_then(id_of) else { return Vec::new() };
            let modal = Modal {
                id,
                channel_id,
                custom_id,
                title: v.get("title").and_then(text).unwrap_or_default(),
                app_id,
                app_name: app.and_then(|a| a.get("name")).and_then(text).unwrap_or_default(),
                components: components(v.get("components"), &[]),
                nonce: v.get("nonce").and_then(id_of),
            };
            ev(InteractionEvent::Modal(Box::new(modal)))
        }
        "APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE" => {
            let Ok(v) = serde_json::from_str::<Value>(d) else { return Vec::new() };
            let Some(nonce) = v.get("nonce").and_then(id_of) else { return Vec::new() };
            let choices = v.get("choices").and_then(Value::as_array).map(|a| a.iter().take(MAX_CHOICES).filter_map(choice).collect()).unwrap_or_default();
            ev(InteractionEvent::Autocomplete { nonce, choices })
        }
        "MESSAGE_UPDATE" => {
            let Ok(u) = serde_json::from_str::<RawAppUpdate>(d) else { return Vec::new() };
            let (Some(id), Some(channel_id)) = (u.id, u.channel_id) else { return Vec::new() };
            // Paquet partiel sans rien de ce qui nous concerne : rien ne change.
            if u.flags.is_none() && u.components.is_none() && u.interaction_metadata.is_none() {
                return Vec::new();
            }
            let mut atts: Vec<Attachment> = u.attachments.into_iter().flatten().filter_map(|a| a.0).map(super::media::RawAttachment::into_attachment).collect();
            let app = app_meta(u.flags, u.components.as_ref(), u.interaction_metadata.as_ref(), u.interaction.as_ref(), u.application_id, &mut atts);
            let edited = u.edited_timestamp.is_some_and(|t| !t.is_null());
            ev(InteractionEvent::MessageApp { channel_id, id, app, edited })
        }
        _ => Vec::new(),
    }
}

// ====================================================================== REST

fn index_request(scope: Scope) -> (Route, String) {
    match scope {
        Scope::Guild(g) => (Route { template: "GET /guilds/{}/application-command-index", major: g }, format!("/guilds/{g}/application-command-index")),
        Scope::Channel(c) => (Route { template: "GET /channels/{}/application-command-index", major: c }, format!("/channels/{c}/application-command-index")),
        Scope::User => (Route { template: "GET /users/@me/application-command-index", major: 0 }, "/users/@me/application-command-index".to_string()),
    }
}

fn value(v: &crate::state::interactions::Value) -> Value {
    use crate::state::interactions::Value as V;
    match v {
        V::Str(s) => json!(s),
        V::Int(n) => json!(n),
        V::Num(x) => json!(x),
        V::Bool(b) => json!(b),
        V::Id(id) => json!(id.to_string()),
        // Identifiant de la pièce jointe : son rang dans `attachments` (« Uploading Files »).
        V::Attachment(i) => json!(i.to_string()),
    }
}

/// Options envoyées, imbriquées dans la sous-commande et son groupe.
fn nested(cmd_options: Vec<Value>, path: &[String]) -> Vec<Value> {
    // Un chemin à deux noms : groupe (2) puis sous-commande (1).
    let mut inner = cmd_options;
    for (i, name) in path.iter().enumerate().rev() {
        let kind = if path.len() == 2 && i == 0 { 2 } else { 1 };
        inner = vec![json!({ "type": kind, "name": name, "options": inner })];
    }
    inner
}

fn filled(f: &Filled) -> Value {
    json!({ "type": f.kind.code(), "name": f.name, "value": value(&f.value) })
}

/// `attachments` de la commande : une entrée par fichier, `id` = rang de sa
/// partie `files[n]` (documentation de Discord, « Uploading Files »).
fn attachments(files: &[crate::state::upload::OutFile]) -> Value {
    Value::Array(files.iter().enumerate().map(|(i, f)| json!({ "id": i.to_string(), "filename": f.sent_name() })).collect())
}

/// Corps de `POST /interactions`.
pub(super) fn body(o: &Outgoing) -> Value {
    let data = match &o.kind {
        OutKind::Command { id, version, name, kind, path, options, target_id } => {
            let mut d = json!({ "id": id.to_string(), "name": name, "type": kind.code(), "version": version.to_string(), "attachments": attachments(&o.files) });
            if *kind == CommandKind::ChatInput {
                d["options"] = Value::Array(nested(options.iter().map(filled).collect(), path));
            }
            if let Some(t) = target_id {
                d["target_id"] = json!(t.to_string());
            }
            d
        }
        OutKind::Autocomplete { id, version, name, path, options, focused } => {
            let mut opts: Vec<Value> = options.iter().map(filled).collect();
            let mut f = filled(focused);
            f["focused"] = json!(true);
            opts.push(f);
            json!({ "id": id.to_string(), "name": name, "type": 1, "version": version.to_string(), "options": nested(opts, path), "attachments": [] })
        }
        OutKind::Component { component_type, custom_id, values, .. } => {
            let mut d = json!({ "component_type": component_type, "custom_id": custom_id });
            if let Some(v) = values {
                d["values"] = json!(v);
            }
            d
        }
        OutKind::ModalSubmit { interaction_id, custom_id, values } => {
            let comps: Vec<Value> = values
                .iter()
                .map(|m| {
                    let mut inner = json!({ "type": m.kind, "custom_id": m.custom_id });
                    match &m.text {
                        Some(t) => inner["value"] = json!(t),
                        None => inner["values"] = json!(m.values),
                    }
                    if m.wrapper == 18 { json!({ "type": 18, "component": inner }) } else { json!({ "type": 1, "components": [inner] }) }
                })
                .collect();
            json!({ "id": interaction_id.to_string(), "custom_id": custom_id, "components": comps })
        }
    };
    let mut b = json!({
        "type": o.kind.code(),
        "application_id": o.application_id.to_string(),
        "channel_id": o.channel_id.to_string(),
        "session_id": o.session.0,
        "nonce": o.nonce.to_string(),
        "data": data,
    });
    if let Some(g) = o.guild_id {
        b["guild_id"] = json!(g.to_string());
    }
    if let OutKind::Component { message_id, message_flags, .. } = &o.kind {
        b["message_id"] = json!(message_id.to_string());
        b["message_flags"] = json!(message_flags);
    }
    b
}

/// Fichiers des options pièce jointe, lus en entier (bloquant) : `None` si
/// l'un est illisible ou dépasse la limite d'envoi sans boost.
fn read_files(files: &[crate::state::upload::OutFile]) -> Option<Vec<(String, Vec<u8>)>> {
    use crate::state::upload::{Source, DEFAULT_LIMIT};
    files
        .iter()
        .map(|f| {
            let bytes = match &f.source {
                Source::Memory(b) => b.to_vec(),
                Source::Path(p) => {
                    let len = std::fs::metadata(p).ok().filter(|m| m.is_file())?.len();
                    if len > DEFAULT_LIMIT {
                        return None;
                    }
                    std::fs::read(p).ok()?
                }
            };
            (bytes.len() as u64 <= DEFAULT_LIMIT).then(|| (f.sent_name(), bytes))
        })
        .collect()
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: InteractionCmd) {
    let ev = |e: InteractionEvent| Event::Interactions(e);
    match cmd {
        InteractionCmd::Index { scope } => {
            // Routes réservées aux comptes utilisateur.
            if ctx.is_bot() {
                return ctx.out.send(ev(InteractionEvent::Index { scope, index: Index::default() }));
            }
            let (route, path) = index_request(scope);
            let url = format!("{}{path}", ctx.api_root);
            match api(&ctx, route, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
                Ok(body) => ctx.out.send(match index_of(&body) {
                    Some(index) => ev(InteractionEvent::Index { scope, index }),
                    None => ev(InteractionEvent::IndexFailed { scope }),
                }),
                Err(ApiError::Unauthorized) => ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
                // 403 (pas membre), 404 (MP sans bot) : pas de commandes, sans message.
                Err(_) => ctx.out.send(ev(InteractionEvent::IndexFailed { scope })),
            }
        }
        InteractionCmd::Send(o) => {
            let nonce = o.nonce;
            if ctx.is_bot() {
                return ctx.out.send(ev(InteractionEvent::SendFailed { nonce, problem: Problem::Forbidden }));
            }
            let url = format!("{}/interactions", ctx.api_root);
            let route = Route { template: "POST /interactions", major: 0 };
            let sent = if o.files.is_empty() {
                api(&ctx, route, Method::POST, &url, Some(body(&o)), MAX_WAIT, std::future::pending()).await
            } else {
                // Options pièce jointe : `multipart/form-data`, fichiers lus hors du fil d'interface.
                let files = o.files.clone();
                let read = tokio::task::spawn_blocking(move || read_files(&files)).await.ok().flatten();
                let Some(files) = read else {
                    return ctx.out.send(ev(InteractionEvent::SendFailed { nonce, problem: Problem::Api(String::new()) }));
                };
                let (kind, bytes) = super::rest::multipart_in_memory(nonce, &body(&o), &files);
                let build = move |b: reqwest::RequestBuilder| b.header(reqwest::header::CONTENT_TYPE, kind.clone()).body(bytes.clone());
                super::rest::api_req(&ctx, route, Method::POST, &url, build, MAX_WAIT, std::future::pending()).await
            };
            match sent {
                Ok(_) => ctx.out.send(ev(InteractionEvent::Sent { nonce })),
                // Captcha compris : jamais retenté, l'échec est montré.
                Err(e) => ctx.out.send(e.into_event(|problem| ev(InteractionEvent::SendFailed { nonce, problem }))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::interactions::Value as V;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"));
        if v.get("op").is_some() { v["d"].to_string() } else { text }
    }

    fn one(v: Vec<InteractionEvent>) -> InteractionEvent {
        let [e]: [InteractionEvent; 1] = v.try_into().unwrap_or_else(|v: Vec<InteractionEvent>| panic!("{v:?}"));
        e
    }

    fn events(kind: &str, d: &str) -> Vec<InteractionEvent> {
        decode(kind, d).into_iter().filter_map(|e| if let Event::Interactions(i) = e { Some(i) } else { None }).collect()
    }

    #[test]
    fn command_index_fixture() {
        let index = index_of(&fixture("rest_application_command_index.json")).unwrap_or_else(|| panic!("index illisible"));
        assert_eq!(index.apps.len(), 1);
        assert_eq!((index.apps[0].name.as_str(), index.apps[0].bot_id), ("Robot Fictif", Some(4000000000000000001)));
        let names: Vec<_> = index.commands.iter().map(|c| (c.name.as_str(), c.kind)).collect();
        assert_eq!(names, vec![("sondage", CommandKind::ChatInput), ("role", CommandKind::ChatInput), ("Profil rapide", CommandKind::User), ("Traduire", CommandKind::Message)]);
        let poll = &index.commands[0];
        assert_eq!((poll.version, poll.app_id), (4100000000000000901, 4000000000000000001));
        let kinds: Vec<_> = poll.options.iter().map(|o| (o.name.as_str(), o.kind, o.required)).collect();
        assert_eq!(
            kinds,
            vec![
                ("question", OptionKind::String, true),
                ("duree", OptionKind::Integer, false),
                ("anonyme", OptionKind::Boolean, false),
                ("salon", OptionKind::Channel, false),
                ("ville", OptionKind::String, false),
            ]
        );
        assert_eq!(poll.options[0].max_length, Some(300));
        assert_eq!(poll.options[1].choices.iter().map(|c| c.value.clone()).collect::<Vec<_>>(), vec![ChoiceValue::Int(60), ChoiceValue::Int(1440)]);
        assert_eq!(poll.options[3].channel_types, vec![0, 5]);
        assert!(poll.options[4].autocomplete);
        let leaves = index.commands[1].leaves();
        assert_eq!(leaves.iter().map(|(p, _, _)| p.join(" ")).collect::<Vec<_>>(), vec!["ajouter", "retirer"]);
        // Forme inattendue : échec, jamais un index vide.
        assert!(index_of("[]").is_none());
        assert!(index_of(r#"{ "applications": [], "application_commands": [ { "id": "x" }, 3 ] }"#).is_some_and(|i| i.commands.is_empty()));
    }

    #[test]
    fn gateway_acks_and_failure() {
        let created = fixture("interaction_create.json");
        assert_eq!(events("INTERACTION_CREATE", &created), vec![InteractionEvent::Created { nonce: 1300000000000000001, id: 1200000000000000001 }]);
        let ok = r#"{ "id": "1200000000000000001", "nonce": null }"#;
        assert_eq!(events("INTERACTION_SUCCESS", ok), vec![InteractionEvent::Succeeded { nonce: 0, id: 1200000000000000001 }]);
        let failed = fixture("interaction_failure.json");
        assert_eq!(events("INTERACTION_FAILURE", &failed), vec![InteractionEvent::Failed { nonce: 1300000000000000001, id: 1200000000000000001, reason: 2 }]);
        assert!(events("INTERACTION_FAILURE", "{}").is_empty());
        assert!(events("INTERACTION_CREATE", "pas du json").is_empty());
    }

    #[test]
    fn modal_and_autocomplete() {
        let InteractionEvent::Modal(m) = one(events("INTERACTION_MODAL_CREATE", &fixture("interaction_modal_create.json"))) else { panic!() };
        assert_eq!((m.id, m.title.as_str(), m.app_name.as_str(), m.custom_id.as_str()), (1200000000000000002, "Votre avis", "Robot Fictif", "avis"));
        assert_eq!(m.nonce, Some(1300000000000000002));
        assert_eq!(m.components.len(), 3);
        let Component::Row(row) = &m.components[0] else { panic!("{:?}", m.components[0]) };
        assert!(matches!(&row[0], Component::TextInput(t) if t.custom_id == "titre" && !t.paragraph && t.max_length == Some(80)));
        assert!(matches!(&m.components[1], Component::Label { label, child, .. } if label == "Détails" && matches!(**child, Component::TextInput(ref t) if t.paragraph && !t.required)));
        assert!(matches!(&m.components[2], Component::Label { child, .. } if matches!(**child, Component::Select(ref s) if s.options.len() == 2)));
        let ac = r#"{ "nonce": "1300000000000000005", "choices": [ { "name": "Paris", "value": "paris" }, { "name": "Lyon", "value": 2 }, { "nom": "?" } ] }"#;
        let InteractionEvent::Autocomplete { nonce, choices } = one(events("APPLICATION_COMMAND_AUTOCOMPLETE_RESPONSE", ac)) else { panic!() };
        assert_eq!(nonce, 1300000000000000005);
        assert_eq!(choices, vec![Choice { name: "Paris".into(), value: ChoiceValue::Str("paris".into()) }, Choice { name: "Lyon".into(), value: ChoiceValue::Int(2) }]);
    }

    fn message(name: &str) -> crate::state::model::Message {
        let raw: super::super::raw::RawMessage = serde_json::from_str(&fixture(name)).unwrap_or_else(|e| panic!("{e}"));
        raw.into_message()
    }

    #[test]
    fn message_components_v1() {
        let m = message("message_create_components.json");
        let app = m.meta.app.as_deref().unwrap_or_else(|| panic!("pas de composants"));
        assert!(app.ephemeral() && !app.loading() && !app.v2());
        assert_eq!(app.interaction, Some(InteractionMeta { user_id: 1000000000000000001, user_name: "Moi Fictif".into(), command: Some("sondage".into()) }));
        assert_eq!(app.application_id, Some(4000000000000000001));
        let [Component::Row(a), Component::Row(b)] = app.components.as_slice() else { panic!("{:?}", app.components) };
        assert_eq!(a.len(), 3);
        assert!(matches!(&a[0], Component::Button(x) if x.style == ButtonStyle::Success && x.custom_id.as_deref() == Some("oui") && x.emoji == Some(Emoji::Unicode("👍".into()))));
        assert!(matches!(&a[1], Component::Button(x) if x.style == ButtonStyle::Danger && x.disabled));
        assert!(matches!(&a[2], Component::Button(x) if x.style == ButtonStyle::Link && x.url.as_deref() == Some("https://sondage.invalid/resultats")));
        assert!(matches!(&b[0], Component::Select(s) if s.kind == SelectKind::String && s.max_values == 2 && s.options[1].default));
    }

    #[test]
    fn message_components_v2_take_their_attachments() {
        let m = message("message_create_components_v2.json");
        let app = m.meta.app.as_deref().unwrap_or_else(|| panic!("pas de composants"));
        assert!(app.v2());
        let [Component::Container { accent: Some(0x5865F2), children, .. }] = app.components.as_slice() else { panic!("{:?}", app.components) };
        assert!(matches!(&children[0], Component::Section { texts, accessory: Some(acc) } if texts.len() == 1 && matches!(**acc, Component::Thumbnail(ref a) if a.width == 128)));
        assert!(matches!(&children[1], Component::Separator { divider: true, large: true }));
        let Component::Gallery(items) = &children[2] else { panic!("{:?}", children[2]) };
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, 7000000000000000301, "pièce jointe du message retrouvée par son nom");
        assert!(items[1].id >> 63 == 1 && items[1].spoiler);
        assert!(matches!(&children[3], Component::Row(r) if matches!(&r[0], Component::Select(s) if s.kind == SelectKind::User)));
        // La pièce jointe montrée par la galerie n'est pas répétée sous le message.
        assert!(m.attachments.is_empty(), "{:?}", m.attachments);
        let mut texts = Vec::new();
        Component::texts(&app.components, &mut texts);
        assert_eq!(texts, vec!["**Météo** du jour : <@1000000000000000002> a gagné", "Fin"]);
        // Type inconnu ou illisible : écarté sans rendre le message illisible.
        assert_eq!(component(&json!({ "type": 99 }), 0, &[]), Component::Unknown);
        assert_eq!(component(&json!({ "type": "bouton" }), 0, &[]), Component::Unknown);
    }

    #[test]
    fn loading_update_resolves() {
        let up = r#"{ "id": "5000000000000000901", "channel_id": "3000000000000000011", "flags": 0, "edited_timestamp": null, "components": [], "content": "Fait" }"#;
        let InteractionEvent::MessageApp { app, edited: false, .. } = one(events("MESSAGE_UPDATE", up)) else { panic!() };
        assert!(app.is_none());
        // Mise à jour sans champ d'application : rien.
        assert!(events("MESSAGE_UPDATE", r#"{ "id": "1", "channel_id": "2", "content": "x" }"#).is_empty());
        let loading = r#"{ "id": "1", "channel_id": "2", "flags": 128, "interaction_metadata": { "id": "3", "type": 2, "name": "ping", "user": { "id": "4", "username": "moi" } } }"#;
        let InteractionEvent::MessageApp { app: Some(a), .. } = one(events("MESSAGE_UPDATE", loading)) else { panic!() };
        assert!(a.loading() && a.interaction.as_ref().is_some_and(|i| i.command.as_deref() == Some("ping")));
    }

    fn out(kind: OutKind) -> Outgoing {
        Outgoing { nonce: 42, session: SessionId("sess".into()), application_id: 7, guild_id: Some(9), channel_id: 8, kind, files: Vec::new() }
    }

    #[test]
    fn bodies() {
        let f = |name: &str, kind, value| Filled { name: name.into(), kind, value };
        let b = body(&out(OutKind::Command {
            id: 1,
            version: 2,
            name: "role".into(),
            kind: CommandKind::ChatInput,
            path: vec!["gerer".into(), "ajouter".into()],
            options: vec![f("qui", OptionKind::User, V::Id(5)), f("n", OptionKind::Integer, V::Int(3)), f("x", OptionKind::Number, V::Num(1.5)), f("ok", OptionKind::Boolean, V::Bool(true))],
            target_id: None,
        }));
        assert_eq!(
            b,
            json!({ "type": 2, "application_id": "7", "channel_id": "8", "guild_id": "9", "session_id": "sess", "nonce": "42",
                "data": { "id": "1", "name": "role", "type": 1, "version": "2", "attachments": [], "options": [
                    { "type": 2, "name": "gerer", "options": [ { "type": 1, "name": "ajouter", "options": [
                        { "type": 6, "name": "qui", "value": "5" }, { "type": 4, "name": "n", "value": 3 }, { "type": 10, "name": "x", "value": 1.5 }, { "type": 5, "name": "ok", "value": true } ] } ] } ] } })
        );
        let ctx = body(&out(OutKind::Command { id: 1, version: 2, name: "Traduire".into(), kind: CommandKind::Message, path: Vec::new(), options: Vec::new(), target_id: Some(55) }));
        assert_eq!(ctx["data"], json!({ "id": "1", "name": "Traduire", "type": 3, "version": "2", "attachments": [], "target_id": "55" }));
        let ac = body(&out(OutKind::Autocomplete { id: 1, version: 2, name: "sondage".into(), path: Vec::new(), options: vec![f("question", OptionKind::String, V::Str("Où ?".into()))], focused: f("ville", OptionKind::String, V::Str("Pa".into())) }));
        assert_eq!(ac["type"], 4);
        assert_eq!(ac["data"]["options"], json!([{ "type": 3, "name": "question", "value": "Où ?" }, { "type": 3, "name": "ville", "value": "Pa", "focused": true }]));
        let c = body(&out(OutKind::Component { message_id: 3, message_flags: 64, component_type: 3, custom_id: "choix".into(), values: Some(vec!["a".into()]) }));
        assert_eq!((c["type"].clone(), c["message_id"].clone(), c["message_flags"].clone()), (json!(3), json!("3"), json!(64)));
        assert_eq!(c["data"], json!({ "component_type": 3, "custom_id": "choix", "values": ["a"] }));
        let ms = body(&out(OutKind::ModalSubmit {
            interaction_id: 11,
            custom_id: "avis".into(),
            values: vec![
                ModalValue { kind: 4, custom_id: "titre".into(), text: Some("Bien".into()), values: Vec::new(), wrapper: 1 },
                ModalValue { kind: 3, custom_id: "note".into(), text: None, values: vec!["5".into()], wrapper: 18 },
            ],
        }));
        assert_eq!(
            ms["data"],
            json!({ "id": "11", "custom_id": "avis", "components": [
                { "type": 1, "components": [ { "type": 4, "custom_id": "titre", "value": "Bien" } ] },
                { "type": 18, "component": { "type": 3, "custom_id": "note", "values": ["5"] } } ] })
        );
        assert!(ms.get("message_id").is_none());
        let dm = Outgoing { guild_id: None, ..out(OutKind::Component { message_id: 3, message_flags: 0, component_type: 2, custom_id: "b".into(), values: None }) };
        assert!(body(&dm).get("guild_id").is_none());
        assert!(body(&dm)["data"].get("values").is_none());
    }

    /// Option pièce jointe : `attachments` relie `files[0]` à son nom, la
    /// valeur de l'option est son identifiant (« 0 ») ; corps en multipart.
    #[test]
    fn attachment_options_go_multipart() {
        use crate::state::upload::{OutFile, Source};
        let file = OutFile { source: Source::Memory(std::sync::Arc::from(b"PNG!".to_vec())), name: "carte.png".into(), size: 4, description: String::new(), spoiler: false };
        let o = Outgoing {
            files: vec![file],
            ..out(OutKind::Command {
                id: 1,
                version: 2,
                name: "scanner".into(),
                kind: CommandKind::ChatInput,
                path: Vec::new(),
                options: vec![Filled { name: "image".into(), kind: OptionKind::Attachment, value: V::Attachment(0) }],
                target_id: None,
            })
        };
        let b = body(&o);
        assert_eq!(b["data"]["attachments"], json!([{ "id": "0", "filename": "carte.png" }]));
        assert_eq!(b["data"]["options"], json!([{ "type": 11, "name": "image", "value": "0" }]));
        let files = read_files(&o.files).unwrap_or_else(|| panic!("fichier en mémoire"));
        let (kind, bytes) = super::super::rest::multipart_in_memory(42, &b, &files);
        let text = String::from_utf8_lossy(&bytes);
        let boundary = kind.strip_prefix("multipart/form-data; boundary=").unwrap_or_else(|| panic!("{kind}"));
        assert!(text.starts_with(&format!("--{boundary}\r\nContent-Disposition: form-data; name=\"payload_json\"")), "{text}");
        assert!(text.contains("name=\"files[0]\"; filename=\"carte.png\"\r\nContent-Type: image/png\r\n\r\nPNG!\r\n"), "{text}");
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
        let missing = OutFile { source: Source::Path("Z:/nulle-part/absent.png".into()), name: "absent.png".into(), size: 1, description: String::new(), spoiler: false };
        assert!(read_files(&[missing]).is_none());
    }

    #[test]
    fn index_paths() {
        assert_eq!(index_request(Scope::Guild(5)).1, "/guilds/5/application-command-index");
        assert_eq!(index_request(Scope::Channel(6)).1, "/channels/6/application-command-index");
        assert_eq!(index_request(Scope::User).1, "/users/@me/application-command-index");
        assert_eq!(index_request(Scope::Guild(5)).0.major, 5);
    }
}
