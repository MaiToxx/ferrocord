//! Réactions, réponses, transferts, épingles, modification et suppression
//! (lots T5/T6) : décodage des champs et événements de la passerelle, et
//! requêtes REST. Formes vérifiées dans la documentation publique :
//! - https://discord.com/developers/docs/resources/message (Reaction,
//!   Message Reference, Message Snapshot, Create/Delete Own Reaction, Get
//!   Reactions, Edit/Delete Message, Get Channel Pins, Pin/Unpin Message) ;
//! - https://discord.com/developers/docs/events/gateway-events
//!   (MESSAGE_REACTION_ADD/REMOVE/REMOVE_ALL/REMOVE_EMOJI, CHANNEL_PINS_UPDATE,
//!   GUILD_EMOJIS_UPDATE).

use super::ratelimit::Route;
use super::raw::{RawMessage, RawUser};
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::model::Message;
use crate::state::social::{
    Emoji, Forward, GuildEmoji, MessageMeta, Pin, Reaction, Reactor, ReplyRef, ReplyState, SendKind, SocialCmd, SocialEvent, REACTORS_PAGE,
};
use crate::state::{Event, Problem};
use serde::{de::DeserializeOwned, Deserialize, Deserializer};
use serde_json::{json, value::RawValue};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);
/// Épingles par page (maximum documenté de Get Channel Pins).
const PINS_PAGE: usize = 50;

// ====================================================================== décodage

/// Champ lu à part : `None` s'il est absent ou d'une forme inattendue (le
/// message reste lisible).
fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(d: D) -> Result<Option<T>, D::Error> {
    super::raw::with_raw(d, |raw| serde_json::from_str(raw).ok())
}

/// Identifiant obligatoire, en chaîne : sinon l'événement est illisible.
fn req_id<'de, D: Deserializer<'de>>(d: D) -> Result<u64, D::Error> {
    super::raw::de_id(d)
}

/// Identifiant en chaîne (forme documentée) ; `None` sinon.
fn loose_id<'de, D: Deserializer<'de>>(d: D) -> Result<Option<u64>, D::Error> {
    super::raw::with_raw(d, |raw| serde_json::from_str::<String>(raw).ok().and_then(|s| s.parse().ok()))
}

/// Champ présent, même `null` : `Some(brut)`. Absent : `None` (par défaut).
pub(super) fn present<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Box<RawValue>>, D::Error> {
    Box::<RawValue>::deserialize(d).map(Some)
}

/// Emoji partiel (`id` nul pour un emoji Unicode, `name` nul pour un
/// emoji personnalisé supprimé).
#[derive(Deserialize)]
pub(super) struct RawEmoji {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    animated: Option<bool>,
}

impl RawEmoji {
    fn emoji(self) -> Option<Emoji> {
        match (self.id, self.name) {
            (Some(id), name) => Some(Emoji::Custom { id, name: name.unwrap_or_default(), animated: self.animated.unwrap_or(false) }),
            (None, Some(n)) if !n.is_empty() && n.len() <= MAX_EMOJI_BYTES => Some(Emoji::Unicode(n)),
            _ => None,
        }
    }
}

/// Un emoji Unicode tient en quelques caractères ; au-delà, la valeur est refusée.
const MAX_EMOJI_BYTES: usize = 64;

#[derive(Deserialize, Default)]
struct RawCountDetails {
    #[serde(default)]
    normal: u32,
    #[serde(default)]
    burst: u32,
}

#[derive(Deserialize)]
pub(super) struct RawReaction {
    #[serde(default)]
    count: u32,
    #[serde(default, deserialize_with = "lenient")]
    count_details: Option<RawCountDetails>,
    #[serde(default)]
    me: bool,
    #[serde(default)]
    me_burst: bool,
    emoji: RawEmoji,
}

#[derive(Deserialize)]
pub(super) struct RawReference {
    #[serde(rename = "type", default)]
    kind: u8,
    #[serde(default, deserialize_with = "loose_id")]
    message_id: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    guild_id: Option<u64>,
}

impl RawReference {
    /// (type, serveur, salon, message) : référence d'un message système ou
    /// source d'une annonce publiée (lot T13, `net/announce.rs`).
    pub(super) fn ids(&self) -> (u8, Option<u64>, Option<u64>, Option<u64>) {
        (self.kind, self.guild_id, self.channel_id, self.message_id)
    }
}

/// Message de `referenced_message` : seuls l'auteur et le texte servent.
#[derive(Deserialize)]
struct RawRefMessage {
    author: RawUser,
    #[serde(default, deserialize_with = "lenient")]
    member: Option<RawNick>,
    #[serde(default)]
    content: String,
    #[serde(default)]
    mentions: Vec<RawUser>,
    #[serde(default, deserialize_with = "lenient")]
    attachments: Option<Vec<serde::de::IgnoredAny>>,
}

#[derive(Deserialize)]
struct RawNick {
    #[serde(default)]
    nick: Option<String>,
}

/// Message d'un instantané (documentation « Message Snapshot » : `content`,
/// `attachments`, `embeds`, `sticker_items`, `mentions`…). Pièces jointes,
/// embeds et autocollants sont lus comme ceux d'un message, chacun à part :
/// un élément illisible est ignoré, le reste de l'instantané s'affiche.
#[derive(Deserialize)]
struct RawSnapMessage {
    #[serde(default)]
    content: String,
    #[serde(default, deserialize_with = "super::media::de_items")]
    attachments: Vec<super::media::RawAttachment>,
    #[serde(default, deserialize_with = "super::media::de_items")]
    embeds: Vec<super::media::RawEmbed>,
    #[serde(default, deserialize_with = "super::raw::de_lenient")]
    sticker_items: Option<Vec<super::raw::Item<super::stickers::RawSticker>>>,
    #[serde(default, deserialize_with = "lenient")]
    mentions: Option<Vec<RawUser>>,
}

#[derive(Deserialize)]
pub(super) struct RawSnapshot {
    message: RawSnapMessage,
}

/// Champs de ce lot lus dans un message (voir `RawMessage`).
pub(super) struct RawMeta {
    pub reactions: Option<Vec<super::raw::Item<RawReaction>>>,
    pub reference: Option<RawReference>,
    pub referenced: Option<Box<RawValue>>,
    pub snapshots: Option<Vec<RawSnapshot>>,
    pub pinned: Option<bool>,
}

/// Réactions, réponse, transfert et épingle d'un message reçu.
pub(super) fn meta(raw: RawMeta) -> MessageMeta {
    let reactions = raw
        .reactions
        .unwrap_or_default()
        .into_iter()
        .filter_map(|i| i.0)
        .filter_map(|r| {
            let d = r.count_details.unwrap_or(RawCountDetails { normal: r.count, burst: 0 });
            let emoji = r.emoji.emoji()?;
            Some(Reaction { emoji, normal: d.normal, burst: d.burst, me: r.me, me_burst: r.me_burst })
        })
        .collect();
    let mut m = MessageMeta { reactions, pinned: raw.pinned.unwrap_or(false), ..MessageMeta::default() };
    let Some(reference) = raw.reference else { return m };
    match reference.kind {
        // DEFAULT : réponse. `referenced_message` absent = inconnu, nul = supprimé.
        0 => {
            let state = match raw.referenced.as_deref().map(RawValue::get) {
                None => ReplyState::Unknown,
                Some("null") => ReplyState::Deleted,
                Some(json) => match serde_json::from_str::<RawRefMessage>(json) {
                    Ok(r) => {
                        let author = match r.member.and_then(|m| m.nick) {
                            Some(n) if !n.is_empty() => n,
                            _ => r.author.display(),
                        };
                        let mentions = r.mentions.iter().map(|u| (u.id, u.display())).collect();
                        // Sans texte (pièce jointe seule) : l'extrait le dira.
                        let content = if r.content.is_empty() && r.attachments.is_some_and(|a| !a.is_empty()) { String::new() } else { r.content };
                        m.reply_raw = Some(Box::new((content, mentions)));
                        ReplyState::Loaded { author_id: r.author.id, author, excerpt: String::new() }
                    }
                    Err(_) => ReplyState::Unknown,
                },
            };
            m.reply = Some(Box::new(ReplyRef { message_id: reference.message_id, state }));
        }
        // FORWARD : un seul instantané pris en charge par Discord pour l'instant.
        1 => {
            if let Some(s) = raw.snapshots.and_then(|v| v.into_iter().next()) {
                let s = s.message;
                m.forward = Some(Box::new(Forward {
                    content: s.content,
                    attachments: super::media::attachments(s.attachments),
                    embeds: super::media::embeds(s.embeds),
                    stickers: super::stickers::items(s.sticker_items),
                    channel_id: reference.channel_id,
                    guild_id: reference.guild_id,
                    message_id: reference.message_id,
                    mentions: s.mentions.unwrap_or_default().iter().map(|u| (u.id, u.display())).collect(),
                }));
            }
        }
        _ => {}
    }
    m
}

#[derive(Deserialize)]
struct RawReactionEvent {
    #[serde(deserialize_with = "req_id")]
    channel_id: u64,
    #[serde(deserialize_with = "req_id")]
    message_id: u64,
    #[serde(default, deserialize_with = "loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    emoji: Option<RawEmoji>,
    #[serde(default, deserialize_with = "lenient")]
    burst: Option<bool>,
    #[serde(default, deserialize_with = "loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "loose_id")]
    message_author_id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    member: Option<RawReactor>,
}

/// `member` d'une réaction reçue sur un serveur : de quoi nommer la personne.
#[derive(Deserialize)]
struct RawReactor {
    #[serde(default, deserialize_with = "lenient")]
    nick: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    user: Option<RawReactorUser>,
}

#[derive(Deserialize)]
struct RawReactorUser {
    #[serde(default, deserialize_with = "lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    global_name: Option<String>,
}

impl RawReactor {
    fn name(self) -> Option<String> {
        let user = self.user;
        self.nick.or_else(|| user.and_then(|u| u.global_name.or(u.username))).filter(|n| !n.is_empty())
    }
}

/// Réaction ajoutée dont l'auteur du message est connu : l'événement de la
/// notification (F11), en plus de celui qui met le compteur à jour.
fn reacted_to(d: &str) -> Option<SocialEvent> {
    let r = serde_json::from_str::<RawReactionEvent>(d).ok()?;
    Some(SocialEvent::ReactedTo {
        guild_id: r.guild_id,
        channel_id: r.channel_id,
        message_id: r.message_id,
        user_id: r.user_id?,
        author_id: r.message_author_id?,
        emoji: r.emoji?.emoji()?,
        name: r.member.and_then(RawReactor::name),
    })
}

#[derive(Deserialize)]
struct RawPinned {
    #[serde(deserialize_with = "req_id")]
    id: u64,
    #[serde(deserialize_with = "req_id")]
    channel_id: u64,
    #[serde(default, deserialize_with = "lenient")]
    pinned: Option<bool>,
}

#[derive(Deserialize)]
struct RawChannelOnly {
    #[serde(deserialize_with = "req_id")]
    channel_id: u64,
}

#[derive(Deserialize)]
pub(super) struct RawGuildEmoji {
    #[serde(default, deserialize_with = "loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "lenient")]
    animated: Option<bool>,
    #[serde(default, deserialize_with = "lenient")]
    available: Option<bool>,
}

/// Emojis d'un serveur ; les entrées illisibles sont écartées.
pub(super) fn guild_emojis(raw: Vec<super::raw::Item<RawGuildEmoji>>) -> Vec<GuildEmoji> {
    raw.into_iter()
        .filter_map(|e| {
            let e = e.0?;
            Some(GuildEmoji { id: e.id?, name: e.name.filter(|n| !n.is_empty())?, animated: e.animated.unwrap_or(false), available: e.available.unwrap_or(true) })
        })
        .collect()
}

/// Relève la liste d'emojis d'un serveur reçu (READY, GUILD_CREATE), si le
/// paquet en porte une. Elle reste aussi dans le serveur (`Guild::emojis`,
/// autocomplétion du lot T7).
pub(super) fn take_emojis(g: &mut super::raw::RawGuild, into: &mut Vec<(u64, Vec<GuildEmoji>)>) {
    if !g.unavailable
        && let Some(list) = g.emoji_list()
    {
        let mut list: Vec<GuildEmoji> = list.into_iter().map(|e| GuildEmoji { id: e.id, name: e.name, animated: e.animated, available: e.available }).collect();
        list.shrink_to_fit();
        into.push((g.id, list));
    }
}

/// Publie les emojis retirés par `take_emojis`, après le serveur lui-même.
pub(super) fn send_emojis(list: Vec<(u64, Vec<GuildEmoji>)>, out: &Out) {
    for (guild_id, emojis) in list {
        out.send(Event::Social(SocialEvent::GuildEmojis { guild_id, emojis }));
    }
}

#[derive(Deserialize)]
struct RawEmojisUpdate {
    #[serde(deserialize_with = "req_id")]
    guild_id: u64,
    #[serde(default)]
    emojis: Vec<super::raw::Item<RawGuildEmoji>>,
}

/// Événements de passerelle de ce lot. MESSAGE_UPDATE est aussi lu par
/// `gateway::dispatch` (texte) : seul `pinned` est lu ici.
pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    let ev = match kind {
        "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE" => serde_json::from_str::<RawReactionEvent>(d).ok().and_then(|r| {
            Some(SocialEvent::Reaction {
                channel_id: r.channel_id,
                message_id: r.message_id,
                user_id: r.user_id?,
                emoji: r.emoji?.emoji()?,
                burst: r.burst.unwrap_or(false),
                add: kind == "MESSAGE_REACTION_ADD",
            })
        }),
        "MESSAGE_REACTION_REMOVE_ALL" => serde_json::from_str::<RawReactionEvent>(d)
            .ok()
            .map(|r| SocialEvent::ReactionRemoveAll { channel_id: r.channel_id, message_id: r.message_id }),
        "MESSAGE_REACTION_REMOVE_EMOJI" => serde_json::from_str::<RawReactionEvent>(d).ok().and_then(|r| {
            Some(SocialEvent::ReactionRemoveEmoji { channel_id: r.channel_id, message_id: r.message_id, emoji: r.emoji?.emoji()? })
        }),
        "MESSAGE_UPDATE" => serde_json::from_str::<RawPinned>(d)
            .ok()
            .and_then(|r| Some(SocialEvent::Pinned { channel_id: r.channel_id, message_id: r.id, pinned: r.pinned? })),
        "CHANNEL_PINS_UPDATE" => serde_json::from_str::<RawChannelOnly>(d).ok().map(|r| SocialEvent::PinsChanged { channel_id: r.channel_id }),
        "GUILD_EMOJIS_UPDATE" => {
            serde_json::from_str::<RawEmojisUpdate>(d).ok().map(|r| SocialEvent::GuildEmojis { guild_id: r.guild_id, emojis: guild_emojis(r.emojis) })
        }
        _ => None,
    };
    if let Some(ev) = ev {
        out.send(Event::Social(ev));
    }
    if kind == "MESSAGE_REACTION_ADD"
        && let Some(ev) = reacted_to(d)
    {
        out.send(Event::Social(ev));
    }
}

// ====================================================================== REST

/// Segment de chemin d'un emoji, encodé pour l'adresse (la documentation
/// exige l'encodage, sinon `10014: Unknown Emoji`) : le caractère Unicode,
/// ou `nom:id` pour un emoji personnalisé.
pub fn emoji_path(e: &Emoji) -> String {
    let raw = match e {
        Emoji::Unicode(s) => s.clone(),
        Emoji::Custom { id, name, .. } => format!("{name}:{id}"),
    };
    let mut out = String::with_capacity(raw.len() * 3);
    for b in raw.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~' | b':') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Corps de Create Message d'une réponse ou d'un transfert, à partir du
/// corps d'un message ordinaire.
pub(super) fn send_body(mut body: serde_json::Value, kind: Option<&SendKind>) -> serde_json::Value {
    match kind {
        None => {}
        Some(SendKind::Reply { message_id, mention }) => {
            body["message_reference"] = json!({ "type": 0, "message_id": message_id.to_string(), "fail_if_not_exists": false });
            // `allowed_mentions` présent remplace l'analyse par défaut : elle est
            // redemandée en entier pour que seules les mentions de la réponse changent.
            body["allowed_mentions"] = json!({ "parse": ["users", "roles", "everyone"], "replied_user": mention });
        }
        Some(SendKind::Forward { channel_id, message_id, guild_id }) => {
            let mut r = json!({ "type": 1, "message_id": message_id.to_string(), "channel_id": channel_id.to_string() });
            if let Some(g) = guild_id {
                r["guild_id"] = json!(g.to_string());
            }
            body["message_reference"] = r;
            // Un transfert n'a pas de texte à lui : `content` sert seulement à
            // l'affichage d'un envoi raté.
            if let Some(o) = body.as_object_mut() {
                o.remove("content");
            }
        }
        // Create Message, `sticker_ids` (documenté) : un autocollant du serveur,
        // sans texte (`content` ne sert qu'à l'affichage d'un échec).
        Some(SendKind::Sticker { sticker_id }) => {
            body["sticker_ids"] = json!([sticker_id.to_string()]);
            if let Some(o) = body.as_object_mut() {
                o.remove("content");
            }
        }
        // Message vocal (docs.discord.food, « Voice Messages ») : indicateur
        // IS_VOICE_MESSAGE, aucun texte ; durée et onde vont dans la pièce
        // jointe (`net::upload::payload`).
        Some(SendKind::Voice { .. }) => {
            body["flags"] = json!(crate::state::voice_msg::IS_VOICE_MESSAGE);
            if let Some(o) = body.as_object_mut() {
                o.remove("content");
            }
        }
    }
    body
}

fn route(template: &'static str, channel: u64) -> Route {
    Route { template, major: channel }
}

#[derive(Deserialize)]
struct RawContent {
    #[serde(default)]
    content: Option<String>,
}

#[derive(Deserialize)]
struct RawPinItem {
    #[serde(default)]
    pinned_at: String,
    message: Box<RawValue>,
}

#[derive(Deserialize)]
struct RawPins {
    // Obligatoire : sans lui, une réponse d'une autre forme passerait pour une page vide.
    items: Vec<super::raw::Item<RawPinItem>>,
    #[serde(default)]
    has_more: bool,
}

/// Page d'épingles : les messages illisibles sont écartés.
pub(super) fn decode_pins(body: &str) -> Option<(Vec<Pin>, bool)> {
    // Un objet est attendu : serde accepterait aussi un tableau pour une structure.
    if !body.trim_start().starts_with('{') {
        return None;
    }
    let p: RawPins = serde_json::from_str(body).ok()?;
    let pins = p
        .items
        .into_iter()
        .filter_map(|i| {
            let i = i.0?;
            let m: Message = serde_json::from_str::<RawMessage>(i.message.get()).ok()?.into_message();
            Some(Pin { pinned_at: i.pinned_at, message: m })
        })
        .collect();
    Some((pins, p.has_more))
}

/// Exécute une commande (sauf les envois, qui passent par la file du salon).
pub(super) async fn run(ctx: Arc<Ctx>, cmd: SocialCmd) {
    let root = ctx.api_root.clone();
    let call = |r: Route, m: reqwest::Method, url: String, body: Option<serde_json::Value>| {
        let ctx = ctx.clone();
        async move { api(&ctx, r, m, &url, body, ACTION_MAX_WAIT, std::future::pending()).await }
    };
    let send = |e: Event| ctx.out.send(e);
    let social = |e: SocialEvent| Event::Social(e);
    match cmd {
        SocialCmd::React { channel_id, message_id, emoji, add } => {
            let url = format!("{root}/channels/{channel_id}/messages/{message_id}/reactions/{}/@me", emoji_path(&emoji));
            let (method, template) = if add {
                (reqwest::Method::PUT, "PUT /channels/{}/messages/{}/reactions/{}/@me")
            } else {
                (reqwest::Method::DELETE, "DELETE /channels/{}/messages/{}/reactions/{}/@me")
            };
            let ev = match call(route(template, channel_id), method, url, None).await {
                Ok(_) => social(SocialEvent::ReactDone { channel_id, message_id, emoji, add, error: None }),
                Err(e) => e.into_event(|error| social(SocialEvent::ReactDone { channel_id, message_id, emoji, add, error: Some(error) })),
            };
            send(ev);
        }
        SocialCmd::Reactors { channel_id, message_id, emoji, burst } => {
            let url = format!(
                "{root}/channels/{channel_id}/messages/{message_id}/reactions/{}?limit={REACTORS_PAGE}&type={}",
                emoji_path(&emoji),
                u8::from(burst)
            );
            let res = call(route("GET /channels/{}/messages/{}/reactions/{}", channel_id), reqwest::Method::GET, url, None).await;
            let ev = match res {
                Ok(body) => {
                    let users = serde_json::from_str::<Vec<super::raw::Item<RawUser>>>(&body)
                        .map(|v| v.into_iter().filter_map(|u| u.0).take(REACTORS_PAGE).map(|u| Reactor { id: u.id, name: u.display() }).collect())
                        .map_err(|e| Problem::Api(e.to_string()));
                    social(SocialEvent::Reactors { message_id, emoji, burst, users })
                }
                Err(e) => e.into_event(|error| social(SocialEvent::Reactors { message_id, emoji, burst, users: Err(error) })),
            };
            send(ev);
        }
        SocialCmd::Edit { channel_id, message_id, content } => {
            let url = format!("{root}/channels/{channel_id}/messages/{message_id}");
            let body = json!({ "content": content });
            let ev = match call(route("PATCH /channels/{}/messages/{}", channel_id), reqwest::Method::PATCH, url, Some(body)).await {
                // Le texte renvoyé fait foi (MESSAGE_UPDATE arrivera aussi).
                Ok(body) => {
                    let content = serde_json::from_str::<RawContent>(&body).ok().and_then(|r| r.content).unwrap_or(content);
                    Event::MessageUpdate { channel_id, id: message_id, content }
                }
                Err(e) => e.into_event(|error| social(SocialEvent::EditFailed { channel_id, message_id, content, error })),
            };
            send(ev);
        }
        SocialCmd::Delete { channel_id, message_id } => {
            let url = format!("{root}/channels/{channel_id}/messages/{message_id}");
            let ev = match call(route("DELETE /channels/{}/messages/{}", channel_id), reqwest::Method::DELETE, url, None).await {
                Ok(_) => Event::MessageDelete { channel_id, id: message_id },
                Err(e) => e.into_event(|error| social(SocialEvent::DeleteFailed { channel_id, message_id, error })),
            };
            send(ev);
        }
        SocialCmd::Pin { channel_id, message_id, pin } => {
            let url = format!("{root}/channels/{channel_id}/messages/pins/{message_id}");
            let (method, template) = if pin {
                (reqwest::Method::PUT, "PUT /channels/{}/messages/pins/{}")
            } else {
                (reqwest::Method::DELETE, "DELETE /channels/{}/messages/pins/{}")
            };
            let ev = match call(route(template, channel_id), method, url, None).await {
                Ok(_) => social(SocialEvent::Pinned { channel_id, message_id, pinned: pin }),
                Err(e) => e.into_event(|error| social(SocialEvent::PinFailed { channel_id, message_id, error })),
            };
            send(ev);
        }
        // Documentation de Discord, « Edit Message » : un autre que l'auteur
        // ne peut changer que `flags`, et seul SUPPRESS_EMBEDS se pose.
        SocialCmd::SuppressEmbeds { channel_id, message_id, flags } => {
            let url = format!("{root}/channels/{channel_id}/messages/{message_id}");
            let ev = match call(route("PATCH /channels/{}/messages/{}", channel_id), reqwest::Method::PATCH, url, Some(json!({ "flags": flags }))).await {
                Ok(_) => social(SocialEvent::EmbedsSuppressed { channel_id, message_id }),
                Err(e) => e.into_event(|error| social(SocialEvent::PinFailed { channel_id, message_id, error })),
            };
            send(ev);
        }
        SocialCmd::Pins { channel_id, before } => {
            let mut url = format!("{root}/channels/{channel_id}/messages/pins?limit={PINS_PAGE}");
            if let Some(b) = &before {
                url.push_str("&before=");
                // Horodatage ISO 8601 reçu de Discord : `+` doit être encodé.
                url.push_str(&b.replace('+', "%2B").replace(':', "%3A"));
            }
            let pins = match call(route("GET /channels/{}/messages/pins", channel_id), reqwest::Method::GET, url, None).await {
                Ok(body) => decode_pins(&body).ok_or_else(|| Problem::Api(String::new())),
                Err(ApiError::Unauthorized) => return send(ApiError::Unauthorized.into_event(Event::Error)),
                Err(e) => Err(match e.into_event(Event::Error) {
                    Event::Error(p) => p,
                    _ => Problem::Api(String::new()),
                }),
            };
            send(social(SocialEvent::Pins { channel_id, before, pins }));
        }
        // Passé par la file d'envoi (voir `rest::command_task`).
        SocialCmd::Send { .. } => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta_of(json: &str) -> MessageMeta {
        let m: RawMessage = serde_json::from_str(json).unwrap_or_else(|e| panic!("{e}"));
        m.into_message().meta
    }

    #[test]
    fn emoji_paths_are_encoded() {
        assert_eq!(emoji_path(&Emoji::Unicode("👍".into())), "%F0%9F%91%8D");
        assert_eq!(emoji_path(&Emoji::Custom { id: 41771983429993937, name: "LUL".into(), animated: false }), "LUL:41771983429993937");
        assert_eq!(emoji_path(&Emoji::Unicode("a/b?c".into())), "a%2Fb%3Fc");
    }

    #[test]
    fn reactions_unicode_custom_and_super() {
        let m = meta_of(
            r##"{ "id": "1", "channel_id": "2", "author": { "id": "3" }, "pinned": true, "reactions": [
                { "count": 3, "count_details": { "normal": 2, "burst": 1 }, "me": true, "me_burst": false, "emoji": { "id": null, "name": "🔥" }, "burst_colors": ["#ff0000"] },
                { "count": 1, "me": false, "emoji": { "id": "77", "name": "chat", "animated": true } },
                { "count": 1, "me": false, "emoji": { "id": null, "name": null } },
                { "count": "x", "emoji": {} }
            ] }"##,
        );
        assert!(m.pinned);
        assert_eq!(
            m.reactions,
            vec![
                Reaction { emoji: Emoji::Unicode("🔥".into()), normal: 2, burst: 1, me: true, me_burst: false },
                Reaction { emoji: Emoji::Custom { id: 77, name: "chat".into(), animated: true }, normal: 1, burst: 0, me: false, me_burst: false },
            ]
        );
        // Réactions illisibles en bloc : message gardé, sans réactions.
        assert!(meta_of(r#"{ "id": "1", "channel_id": "2", "author": { "id": "3" }, "reactions": 5 }"#).reactions.is_empty());
    }

    #[test]
    fn reply_loaded_deleted_or_unknown() {
        let base = r#""id": "1", "channel_id": "2", "author": { "id": "3" }, "type": 19, "message_reference": { "message_id": "9", "channel_id": "2" }"#;
        let m = meta_of(&format!(
            r#"{{ {base}, "referenced_message": {{ "id": "9", "channel_id": "2", "author": {{ "id": "5", "username": "bob" }}, "content": "salut <@6>", "mentions": [{{ "id": "6", "username": "eve" }}] }} }}"#
        ));
        let r = m.reply.unwrap_or_else(|| panic!("réponse attendue"));
        assert_eq!(r.message_id, Some(9));
        assert_eq!(r.state, ReplyState::Loaded { author_id: 5, author: "bob".into(), excerpt: String::new() });
        assert_eq!(m.reply_raw.map(|b| *b), Some(("salut <@6>".to_string(), vec![(6, "eve".to_string())])));
        assert_eq!(meta_of(&format!(r#"{{ {base}, "referenced_message": null }}"#)).reply.map(|r| r.state), Some(ReplyState::Deleted));
        assert_eq!(meta_of(&format!("{{ {base} }}")).reply.map(|r| r.state), Some(ReplyState::Unknown));
        assert_eq!(meta_of(&format!(r#"{{ {base}, "referenced_message": 3 }}"#)).reply.map(|r| r.state), Some(ReplyState::Unknown));
    }

    #[test]
    fn forward_snapshot() {
        let m = meta_of(
            r#"{ "id": "1", "channel_id": "2", "author": { "id": "3" }, "flags": 16384,
                 "message_reference": { "type": 1, "message_id": "9", "channel_id": "8", "guild_id": "7" },
                 "message_snapshots": [ { "message": { "content": "transféré", "type": 0, "timestamp": "2025-01-01T00:00:00+00:00",
                   "attachments": [
                     { "id": "40", "filename": "paysage-fictif.png", "content_type": "image/png", "width": 1600, "height": 900, "size": 1234,
                       "url": "https://cdn.discordapp.com/attachments/8/40/paysage-fictif.png?ex=0a&is=0b&hm=0c",
                       "proxy_url": "https://media.discordapp.net/attachments/8/40/paysage-fictif.png?ex=0a&is=0b&hm=0c" },
                     { "filename": "sans-adresse.png" },
                     { "id": "41", "filename": "notes.txt", "size": 12, "url": "https://cdn.discordapp.com/attachments/8/41/notes.txt" } ],
                   "embeds": [ {}, { "type": "rich", "title": "Carte fictive", "description": "Texte de la carte" }, 7 ],
                   "sticker_items": [ { "id": "50", "name": "Salut", "format_type": 1 }, { "name": "sans identifiant" } ] } } ] }"#,
        );
        let f = m.forward.unwrap_or_else(|| panic!("transfert attendu"));
        assert_eq!(f.content, "transféré");
        // Pièces jointes lues comme celles d'un message ; l'illisible est ignorée.
        use crate::state::model::MediaKind;
        let files: Vec<(&str, MediaKind, u32, u32)> = f.attachments.iter().map(|a| (a.name.as_str(), a.kind, a.width, a.height)).collect();
        assert_eq!(files, [("paysage-fictif.png", MediaKind::Image, 1600, 900), ("notes.txt", MediaKind::File, 0, 0)]);
        assert!(f.attachments[0].fetch_url().starts_with("https://media.discordapp.net/"), "image demandée au proxy média");
        // Embed vide ou illisible : ignoré.
        assert_eq!(f.embeds.iter().map(|e| e.title.as_deref()).collect::<Vec<_>>(), [Some("Carte fictive")]);
        assert_eq!(f.stickers.iter().map(|s| (s.id, s.name.as_str())).collect::<Vec<_>>(), [(50, "Salut")]);
        assert_eq!((f.channel_id, f.guild_id, f.message_id), (Some(8), Some(7), Some(9)));
        assert!(m.reply.is_none());
        // Instantané réduit à son texte, ou aux listes de forme inattendue : toujours lisible.
        let m = meta_of(
            r#"{ "id": "1", "channel_id": "2", "author": { "id": "3" }, "message_reference": { "type": 1 },
                 "message_snapshots": [ { "message": { "content": "seul", "attachments": "x", "embeds": null, "sticker_items": 3 } } ] }"#,
        );
        let f = m.forward.unwrap_or_else(|| panic!("transfert attendu"));
        assert!(f.content == "seul" && f.attachments.is_empty() && f.embeds.is_empty() && f.stickers.is_empty());
    }

    #[test]
    fn reply_and_forward_bodies() {
        let base = || json!({ "content": "x", "nonce": "1", "tts": false, "enforce_nonce": true });
        let b = send_body(base(), Some(&SendKind::Reply { message_id: 9, mention: false }));
        assert_eq!(b["message_reference"]["message_id"], "9");
        assert_eq!(b["allowed_mentions"]["replied_user"], false);
        assert_eq!(b["allowed_mentions"]["parse"], json!(["users", "roles", "everyone"]));
        let b = send_body(base(), Some(&SendKind::Forward { channel_id: 8, message_id: 9, guild_id: Some(7) }));
        assert_eq!(b["message_reference"], json!({ "type": 1, "message_id": "9", "channel_id": "8", "guild_id": "7" }));
        assert!(b.get("content").is_none() && b["nonce"] == "1");
        assert_eq!(send_body(base(), None), base());
        let b = send_body(base(), Some(&SendKind::Sticker { sticker_id: 749054660769218631 }));
        assert_eq!(b["sticker_ids"], json!(["749054660769218631"]));
        assert!(b.get("content").is_none() && b["nonce"] == "1");
    }

    fn events(kind: &str, d: &str) -> Vec<Event> {
        let (tx, rx) = std::sync::mpsc::channel();
        let out = Out { tx, wake: Arc::new(|| {}), people: Default::default(), watch: Default::default(), interest: Default::default() };
        dispatch(kind, d, &out);
        drop(out);
        rx.try_iter().collect()
    }

    #[test]
    fn gateway_reaction_events() {
        let add = r#"{ "user_id": "5", "channel_id": "2", "message_id": "1", "guild_id": "7", "emoji": { "id": null, "name": "👍" }, "burst": false, "type": 0 }"#;
        let ev = events("MESSAGE_REACTION_ADD", add);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::Reaction { channel_id: 2, message_id: 1, user_id: 5, add: true, burst: false, emoji })] if *emoji == Emoji::Unicode("👍".into())));
        // Avec l'auteur du message : un second événement, pour la notification (F11).
        let add = r#"{ "user_id": "5", "channel_id": "2", "message_id": "1", "guild_id": "7", "message_author_id": "9", "member": { "nick": null, "user": { "id": "5", "username": "eve", "global_name": "Ève" } }, "emoji": { "id": "77", "name": "chat" }, "burst": false, "type": 0 }"#;
        let ev = events("MESSAGE_REACTION_ADD", add);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::Reaction { .. }), Event::Social(SocialEvent::ReactedTo { guild_id: Some(7), channel_id: 2, message_id: 1, user_id: 5, author_id: 9, name: Some(n), .. })] if n == "Ève"), "{ev:?}");
        assert_eq!(events("MESSAGE_REACTION_REMOVE", add).len(), 1);
        let ev = events("MESSAGE_REACTION_REMOVE", r#"{ "user_id": "5", "channel_id": "2", "message_id": "1", "emoji": { "id": "77", "name": "chat" }, "burst": true, "type": 1 }"#);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::Reaction { add: false, burst: true, .. })]));
        let ev = events("MESSAGE_REACTION_REMOVE_ALL", r#"{ "channel_id": "2", "message_id": "1" }"#);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::ReactionRemoveAll { channel_id: 2, message_id: 1 })]));
        let ev = events("MESSAGE_REACTION_REMOVE_EMOJI", r#"{ "channel_id": "2", "message_id": "1", "emoji": { "id": null, "name": "👍" } }"#);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::ReactionRemoveEmoji { .. })]));
        let ev = events("CHANNEL_PINS_UPDATE", r#"{ "channel_id": "2", "last_pin_timestamp": null }"#);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::PinsChanged { channel_id: 2 })]));
        // MESSAGE_UPDATE : seulement s'il porte `pinned`.
        assert!(events("MESSAGE_UPDATE", r#"{ "id": "1", "channel_id": "2", "content": "x" }"#).is_empty());
        let ev = events("MESSAGE_UPDATE", r#"{ "id": "1", "channel_id": "2", "pinned": true }"#);
        assert!(matches!(&ev[..], [Event::Social(SocialEvent::Pinned { pinned: true, .. })]));
        // Illisibles : rien.
        for (k, d) in [("MESSAGE_REACTION_ADD", r#"{ "channel_id": "2", "message_id": "1" }"#), ("MESSAGE_REACTION_ADD", "pas du json"), ("GUILD_EMOJIS_UPDATE", "{}")] {
            assert!(events(k, d).is_empty(), "{k} {d}");
        }
    }

    #[test]
    fn guild_emojis_update() {
        let ev = events(
            "GUILD_EMOJIS_UPDATE",
            r#"{ "guild_id": "7", "emojis": [ { "id": "1", "name": "chat", "animated": false, "available": true }, { "id": "2", "name": null }, { "id": "x" }, 4 ] }"#,
        );
        let [Event::Social(SocialEvent::GuildEmojis { guild_id: 7, emojis })] = &ev[..] else { panic!("{ev:?}") };
        assert_eq!(emojis, &vec![GuildEmoji { id: 1, name: "chat".into(), animated: false, available: true }]);
    }

    #[test]
    fn pins_page() {
        let body = r#"{ "items": [ { "pinned_at": "2025-01-01T00:00:00+00:00", "message": { "id": "1", "channel_id": "2", "author": { "id": "3", "username": "a" }, "content": "épinglé", "pinned": true } }, { "pinned_at": "x", "message": { "id": "bad" } } ], "has_more": true }"#;
        let (pins, more) = decode_pins(body).unwrap_or_else(|| panic!("page attendue"));
        assert!(more);
        assert_eq!(pins.len(), 1);
        assert_eq!((pins[0].message.content.as_str(), pins[0].pinned_at.as_str()), ("épinglé", "2025-01-01T00:00:00+00:00"));
        assert!(decode_pins("[]").is_none());
    }
}
