//! Fils de discussion et forums (lot T10) : décodage des objets et des
//! événements de la passerelle, requêtes REST.
//!
//! Formes tirées de la documentation officielle de Discord (Channels Resource :
//! objet Channel, Thread Metadata, Thread Member, Forum Tag, Default Reaction ;
//! Gateway Events : THREAD_CREATE, THREAD_UPDATE, THREAD_DELETE,
//! THREAD_LIST_SYNC, THREAD_MEMBER_UPDATE, THREAD_MEMBERS_UPDATE, champ
//! `threads` de GUILD_CREATE). Toutes les requêtes sont documentées ; elles
//! suivent un geste de l'utilisateur et passent par le limiteur commun (`api`).
//! Chaque champ est lu à part : une forme inattendue ne fait échouer que lui.

use super::raw::{de_lenient, de_loose_id, Item, RawChannel, RawGuild};
use super::ratelimit::Route;
use super::rest::{api, Ctx};
use super::Out;
use crate::state::model::snowflake_ms;
use crate::state::social::Emoji;
use crate::state::threads::{Forum, ForumTag, Thread, ThreadAction, ThreadCmd, ThreadEvent, ThreadKind, ThreadStub, ARCHIVED_PAGE};
use crate::state::Event;
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

/// Attente maximale d'une action avant de partir : au-delà, l'échec est annoncé.
const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);

fn iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

#[derive(Deserialize, Default)]
struct RawMeta {
    #[serde(default, deserialize_with = "de_lenient")]
    archived: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    locked: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    auto_archive_duration: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    archive_timestamp: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    create_timestamp: Option<String>,
}

/// Membre d'un fil (`member` d'un fil, `members` de THREAD_LIST_SYNC).
#[derive(Deserialize)]
pub(super) struct RawThreadMember {
    /// Identifiant du fil (absent sur certains points d'API).
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    /// Drapeaux de membre : réglage de notification du fil (lot N).
    #[serde(default, deserialize_with = "de_lenient")]
    flags: Option<u64>,
}

impl RawThreadMember {
    /// (fil, drapeaux) si un réglage de notification y est choisi.
    fn notif(&self, thread: Option<u64>) -> Option<(u64, u64)> {
        let flags = self.flags.filter(|f| crate::state::threads::notif_level(*f).is_some())?;
        Some((self.id.or(thread)?, flags))
    }
}

/// Événement des réglages de notification de fils, s'il y en a.
fn notifs(list: Vec<(u64, u64)>) -> Option<Event> {
    (!list.is_empty()).then(|| Event::Threads(ThreadEvent::Notifs(list)))
}

/// Fil : objet Channel de type 10, 11 ou 12.
#[derive(Deserialize)]
pub(super) struct RawThread {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u8>,
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    parent_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_loose_id")]
    owner_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    last_message_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    rate_limit_per_user: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    message_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    member_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    flags: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    applied_tags: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    thread_metadata: Option<RawMeta>,
    #[serde(default, deserialize_with = "de_lenient")]
    member: Option<RawThreadMember>,
}

impl RawThread {
    /// `guild` : serveur du paquet englobant (GUILD_CREATE ne le répète pas).
    /// `None` si ce n'est pas un fil lisible.
    pub(super) fn to_thread(&self, guild: Option<u64>) -> Option<Thread> {
        let kind = ThreadKind::from_code(self.kind?)?;
        let id = self.id?;
        let meta = self.thread_metadata.as_ref();
        Some(Thread {
            id,
            guild_id: self.guild_id.or(guild)?,
            parent_id: self.parent_id?,
            name: self.name.clone().unwrap_or_default(),
            kind,
            owner_id: self.owner_id,
            archived: meta.and_then(|m| m.archived).unwrap_or(false),
            locked: meta.and_then(|m| m.locked).unwrap_or(false),
            auto_archive: meta.and_then(|m| m.auto_archive_duration).unwrap_or(1440),
            archived_at: meta.and_then(|m| m.archive_timestamp.as_deref()).and_then(iso_ms),
            created_at: meta.and_then(|m| m.create_timestamp.as_deref()).and_then(iso_ms).unwrap_or_else(|| snowflake_ms(id)),
            message_count: self.message_count.unwrap_or(0),
            member_count: self.member_count.unwrap_or(0),
            applied_tags: self.applied_tags.iter().flatten().filter_map(|t| t.parse().ok()).collect(),
            last_message_id: self.last_message_id,
            slowmode: self.rate_limit_per_user.unwrap_or(0),
            pinned: self.flags.is_some_and(|f| f & 2 != 0),
            joined: self.member.is_some(),
        })
    }

    /// Résumé sous le message de départ (champ `thread` d'un message).
    pub(super) fn stub(&self) -> Option<ThreadStub> {
        self.to_thread(Some(0)).map(|t| t.stub())
    }

    fn joined(&self) -> Option<bool> {
        self.member.as_ref().map(|_| true)
    }
}

/// Étiquette disponible d'un forum.
#[derive(Deserialize)]
pub(super) struct RawTag {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    moderated: Option<bool>,
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
}

/// Réaction par défaut d'un forum (`emoji_id` ou `emoji_name`).
#[derive(Deserialize)]
pub(super) struct RawEmojiRef {
    #[serde(default, deserialize_with = "de_loose_id")]
    emoji_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji_name: Option<String>,
}

fn emoji(id: Option<u64>, name: Option<&str>) -> Option<Emoji> {
    match (id, name.filter(|n| !n.is_empty())) {
        (Some(id), name) => Some(Emoji::Custom { id, name: name.unwrap_or_default().to_string(), animated: false }),
        (None, Some(n)) => Some(Emoji::Unicode(n.to_string())),
        (None, None) => None,
    }
}

/// Champs d'un salon forum ou média (`RawChannel`).
pub(super) fn forum_of(c: &RawChannel, guild: Option<u64>) -> Option<Forum> {
    let (id, kind) = c.id_kind();
    if kind != 15 && kind != 16 {
        return None;
    }
    let tags = c.available_tags.iter().flatten().filter_map(|t| t.0.as_ref()).filter_map(|t| {
        Some(ForumTag { id: t.id?, name: t.name.clone().unwrap_or_default(), moderated: t.moderated.unwrap_or(false), emoji: emoji(t.emoji_id, t.emoji_name.as_deref()) })
    });
    Some(Forum {
        id,
        guild_id: c.guild_id.or(guild)?,
        tags: tags.collect(),
        default_reaction: c.default_reaction_emoji.as_ref().and_then(|e| emoji(e.emoji_id, e.emoji_name.as_deref())),
        media: kind == 16,
    })
}

/// Sujet d'un salon, s'il en a un.
fn topic_of(c: &RawChannel) -> Option<String> {
    c.topic.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string)
}

/// Fils et forums d'un serveur (READY, GUILD_CREATE), avant `into_guild`.
pub(super) fn take(g: &mut RawGuild, into: &mut Vec<Event>) {
    if g.unavailable {
        return;
    }
    let gid = g.id;
    let forums: Vec<Forum> = g.channels.iter().filter_map(|c| forum_of(c, Some(gid))).collect();
    into.push(Event::Threads(ThreadEvent::Forums { guild_id: gid, forums }));
    let topics = g.channels.iter().filter_map(|c| Some((c.id_kind().0, topic_of(c)?))).collect();
    into.push(Event::Threads(ThreadEvent::Topics { guild_id: gid, topics }));
    let channels = g.channels.iter().filter(|c| c.nsfw == Some(true)).map(|c| c.id_kind().0).collect();
    into.push(Event::Threads(ThreadEvent::AgeGates { guild_id: gid, channels }));
    let list = g.threads.take().unwrap_or_default();
    let raws: Vec<RawThread> = list.into_iter().filter_map(|t| t.0).collect();
    let joined = raws.iter().filter(|t| t.member.is_some()).filter_map(|t| t.id).collect();
    let threads = raws.iter().filter_map(|t| t.to_thread(Some(gid))).collect();
    let mine = raws.iter().filter_map(|t| t.member.as_ref()?.notif(t.id)).collect();
    into.push(Event::Threads(ThreadEvent::Sync { guild_id: gid, parents: None, threads, joined }));
    into.extend(notifs(mine));
}

/// Fil porté par un message (champ `thread`) : connu du magasin comme les autres.
pub(super) fn message_thread(t: &RawThread, guild: Option<u64>) -> Option<Event> {
    let thread = t.to_thread(guild)?;
    Some(Event::Threads(ThreadEvent::Upsert { thread, joined: t.joined() }))
}

#[derive(Deserialize)]
struct RawListSync {
    #[serde(default, deserialize_with = "de_loose_id")]
    guild_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    channel_ids: Option<Vec<String>>,
    #[serde(default)]
    threads: Vec<Item<RawThread>>,
    #[serde(default)]
    members: Vec<Item<RawThreadMember>>,
}

#[derive(Deserialize)]
struct RawDelete {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
}

#[derive(Deserialize)]
struct RawMembersUpdate {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    member_count: Option<u32>,
    #[serde(default)]
    added_members: Vec<Item<RawThreadMember>>,
    #[serde(default, deserialize_with = "de_lenient")]
    removed_member_ids: Option<Vec<String>>,
}

/// Événements de la passerelle ; `me` : mon identifiant (THREAD_MEMBERS_UPDATE).
pub(super) fn dispatch(kind: &str, d: &str, me: Option<u64>, out: &Out) {
    for e in decode(kind, d, me) {
        out.send(e);
    }
}

fn decode(kind: &str, d: &str, me: Option<u64>) -> Vec<Event> {
    let ev = |e: ThreadEvent| vec![Event::Threads(e)];
    match kind {
        "THREAD_CREATE" | "THREAD_UPDATE" => {
            let Ok(t) = serde_json::from_str::<RawThread>(d) else { return Vec::new() };
            match t.to_thread(None) {
                // THREAD_UPDATE ne porte pas `member` : l'appartenance connue reste.
                Some(thread) => {
                    let mine = t.member.as_ref().and_then(|m| m.notif(t.id));
                    let mut out = ev(ThreadEvent::Upsert { thread, joined: t.joined() });
                    out.extend(notifs(mine.into_iter().collect()));
                    out
                }
                None => Vec::new(),
            }
        }
        "THREAD_DELETE" => match serde_json::from_str::<RawDelete>(d).ok().and_then(|r| r.id) {
            Some(id) => ev(ThreadEvent::Delete { id }),
            None => Vec::new(),
        },
        "THREAD_LIST_SYNC" => {
            let Ok(s) = serde_json::from_str::<RawListSync>(d) else { return Vec::new() };
            let Some(guild_id) = s.guild_id else { return Vec::new() };
            let parents = s.channel_ids.map(|v| v.iter().filter_map(|c| c.parse().ok()).collect());
            let threads = s.threads.iter().filter_map(|t| t.0.as_ref()?.to_thread(Some(guild_id))).collect();
            let joined = s.members.iter().filter_map(|m| m.0.as_ref()?.id).collect();
            let mine = s.members.iter().filter_map(|m| m.0.as_ref()?.notif(None)).collect();
            let mut out = ev(ThreadEvent::Sync { guild_id, parents, threads, joined });
            out.extend(notifs(mine));
            out
        }
        // Documenté : envoyé quand le membre du fil de l'utilisateur courant change.
        "THREAD_MEMBER_UPDATE" => match serde_json::from_str::<RawThreadMember>(d).ok().filter(|m| m.user_id.is_none_or(|u| Some(u) == me)).and_then(|m| Some((m.id?, m.notif(None)))) {
            Some((thread_id, mine)) => {
                let mut out = ev(ThreadEvent::Joined { thread_id, joined: true });
                out.extend(notifs(mine.into_iter().collect()));
                out
            }
            None => Vec::new(),
        },
        "THREAD_MEMBERS_UPDATE" => {
            let Ok(u) = serde_json::from_str::<RawMembersUpdate>(d) else { return Vec::new() };
            let Some(thread_id) = u.id else { return Vec::new() };
            let mut out = Vec::new();
            if let Some(count) = u.member_count {
                out.push(Event::Threads(ThreadEvent::Members { thread_id, count }));
            }
            if let Some(me) = me {
                if u.added_members.iter().any(|m| m.0.as_ref().is_some_and(|m| m.user_id == Some(me))) {
                    out.push(Event::Threads(ThreadEvent::Joined { thread_id, joined: true }));
                }
                if u.removed_member_ids.iter().flatten().any(|id| id.parse() == Ok(me)) {
                    out.push(Event::Threads(ThreadEvent::Joined { thread_id, joined: false }));
                }
            }
            out
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" => {
            let Ok(c) = serde_json::from_str::<RawChannel>(d) else { return Vec::new() };
            let mut out = Vec::new();
            if let Some(guild_id) = c.guild_id {
                out.push(Event::Threads(ThreadEvent::Topic { guild_id, channel_id: c.id_kind().0, topic: topic_of(&c) }));
                if let Some(on) = c.nsfw {
                    out.push(Event::Threads(ThreadEvent::AgeGate { guild_id, channel_id: c.id_kind().0, on }));
                }
            }
            if let Some(f) = forum_of(&c, None) {
                out.push(Event::Threads(ThreadEvent::Forum(f)));
            }
            out
        }
        _ => Vec::new(),
    }
}

// ====================================================================== REST

struct Request {
    route: Route,
    method: Method,
    path: String,
    body: Option<Value>,
}

fn iso(ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(ms).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)).unwrap_or_default()
}

fn request(cmd: &ThreadCmd) -> Request {
    let r = |template: &'static str, major: u64, method: Method, path: String, body: Option<Value>| Request { route: Route { template, major }, method, path, body };
    match cmd {
        // Join Thread / Leave Thread (204).
        ThreadCmd::Join { thread_id } => r("PUT /channels/{}/thread-members/@me", *thread_id, Method::PUT, format!("/channels/{thread_id}/thread-members/@me"), None),
        ThreadCmd::Leave { thread_id } => r("DELETE /channels/{}/thread-members/@me", *thread_id, Method::DELETE, format!("/channels/{thread_id}/thread-members/@me"), None),
        // Modify Thread Settings (docs.discord.food, non documenté par Discord) : `flags`.
        ThreadCmd::Notify { thread_id, flags } => r(
            "PATCH /channels/{}/thread-members/@me/settings",
            *thread_id,
            Method::PATCH,
            format!("/channels/{thread_id}/thread-members/@me/settings"),
            Some(json!({ "flags": flags })),
        ),
        // Get Channel Messages : `after`, `limit`. Le message de départ d'une publication porte
        // l'identifiant de son fil : c'est le premier après l'identifiant précédent.
        ThreadCmd::Starter { thread_id } => r("GET /channels/{}/messages", *thread_id, Method::GET, format!("/channels/{thread_id}/messages?after={}&limit=1", thread_id.saturating_sub(1)), None),
        // Modify Channel (Thread) : `archived`.
        ThreadCmd::SetArchived { thread_id, archived } => r("PATCH /channels/{}", *thread_id, Method::PATCH, format!("/channels/{thread_id}"), Some(json!({ "archived": archived }))),
        // List Public Archived Threads : `before` (ISO 8601), `limit`.
        ThreadCmd::Archived { parent_id, before } => {
            let mut path = format!("/channels/{parent_id}/threads/archived/public?limit={ARCHIVED_PAGE}");
            if let Some(b) = before {
                path.push_str(&format!("&before={}", iso(*b)));
            }
            r("GET /channels/{}/threads/archived/public", *parent_id, Method::GET, path, None)
        }
        // Start Thread from Message : `name`.
        ThreadCmd::CreateFromMessage { channel_id, message_id, name } => r(
            "POST /channels/{}/messages/{}/threads",
            *channel_id,
            Method::POST,
            format!("/channels/{channel_id}/messages/{message_id}/threads"),
            Some(json!({ "name": name })),
        ),
        // Start Thread without Message (documentation officielle, « Channel ») :
        // `name`, `type` (11 fil public, 10 fil d'annonces).
        ThreadCmd::CreateStandalone { channel_id, name, announcement } => r(
            "POST /channels/{}/threads",
            *channel_id,
            Method::POST,
            format!("/channels/{channel_id}/threads"),
            Some(json!({ "name": name, "type": if *announcement { 10 } else { 11 } })),
        ),
        // Start Thread without Message, fil privé : `type` 12, `invitable`
        // (les membres peuvent en inviter d'autres, valeur par défaut de Discord).
        ThreadCmd::CreatePrivate { channel_id, name } => r(
            "POST /channels/{}/threads",
            *channel_id,
            Method::POST,
            format!("/channels/{channel_id}/threads"),
            Some(json!({ "name": name, "type": 12, "invitable": true })),
        ),
        // Modify Channel (Thread) : `locked`.
        ThreadCmd::SetLocked { thread_id, locked } => r("PATCH /channels/{}", *thread_id, Method::PATCH, format!("/channels/{thread_id}"), Some(json!({ "locked": locked }))),
        // List Thread Members : `with_member`, `limit`.
        ThreadCmd::Members { thread_id } => r(
            "GET /channels/{}/thread-members",
            *thread_id,
            Method::GET,
            format!("/channels/{thread_id}/thread-members?with_member=true&limit={}", crate::state::threads::MEMBERS_PAGE),
            None,
        ),
        // Start Thread in Forum or Media Channel : `name`, `message`, `applied_tags`.
        ThreadCmd::CreatePost { forum_id, name, content, tags } => r(
            "POST /channels/{}/threads",
            *forum_id,
            Method::POST,
            format!("/channels/{forum_id}/threads"),
            Some(json!({ "name": name, "message": { "content": content }, "applied_tags": tags.iter().map(u64::to_string).collect::<Vec<_>>() })),
        ),
    }
}

fn action(cmd: &ThreadCmd) -> (ThreadAction, Option<u64>) {
    match cmd {
        ThreadCmd::Join { thread_id } => (ThreadAction::Join, Some(*thread_id)),
        ThreadCmd::Leave { thread_id } => (ThreadAction::Leave, Some(*thread_id)),
        ThreadCmd::SetArchived { thread_id, .. } => (ThreadAction::Archive, Some(*thread_id)),
        ThreadCmd::Archived { parent_id, .. } => (ThreadAction::Archived, Some(*parent_id)),
        ThreadCmd::CreateFromMessage { .. } | ThreadCmd::CreateStandalone { .. } | ThreadCmd::CreatePrivate { .. } => (ThreadAction::Create, None),
        ThreadCmd::SetLocked { thread_id, .. } => (ThreadAction::Lock, Some(*thread_id)),
        ThreadCmd::Members { thread_id } => (ThreadAction::Members, Some(*thread_id)),
        ThreadCmd::CreatePost { .. } => (ThreadAction::Post, None),
        ThreadCmd::Notify { thread_id, .. } => (ThreadAction::Notify, Some(*thread_id)),
        ThreadCmd::Starter { thread_id } => (ThreadAction::Starter, Some(*thread_id)),
    }
}

/// Membre d'un fil rendu par « List Thread Members » avec `with_member` :
/// de quoi le nommer (pseudo du serveur, sinon nom affiché, sinon nom).
#[derive(Deserialize)]
struct RawListedMember {
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    member: Option<RawListedGuildMember>,
}

#[derive(Deserialize)]
struct RawListedGuildMember {
    #[serde(default, deserialize_with = "de_lenient")]
    nick: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawListedUser>,
}

#[derive(Deserialize)]
struct RawListedUser {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    username: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    global_name: Option<String>,
}

impl RawListedMember {
    fn named(self) -> Option<(u64, String)> {
        let (nick, user) = self.member.map_or((None, None), |m| (m.nick, m.user));
        let id = self.user_id.or(user.as_ref().and_then(|u| u.id))?;
        let name = nick.or_else(|| user.and_then(|u| u.global_name.or(u.username))).filter(|n| !n.is_empty()).unwrap_or_else(|| id.to_string());
        Some((id, name))
    }
}

/// Ce qu'une carte de galerie montre d'un message de départ : le début du
/// texte, et la première image (jointe, sinon intégrée).
#[derive(Deserialize)]
struct RawStarter {
    #[serde(default)]
    content: String,
    #[serde(default)]
    attachments: Vec<Item<RawStarterImage>>,
    #[serde(default)]
    embeds: Vec<Item<RawStarterEmbed>>,
}

#[derive(Deserialize)]
struct RawStarterImage {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    proxy_url: Option<String>,
    #[serde(default)]
    content_type: Option<String>,
    #[serde(default)]
    width: Option<u32>,
    #[serde(default)]
    height: Option<u32>,
}

#[derive(Deserialize)]
struct RawStarterEmbed {
    #[serde(default)]
    thumbnail: Option<RawStarterImage>,
    #[serde(default)]
    image: Option<RawStarterImage>,
}

impl RawStarterImage {
    /// Adresse du proxy média (qui redimensionne), sinon l'originale ; hôtes de Discord seulement.
    fn shown(&self, attachment: bool) -> Option<(String, u32, u32)> {
        if attachment && !self.content_type.as_deref().is_some_and(|c| c.starts_with("image/")) {
            return None;
        }
        let url = self.proxy_url.clone().or(self.url.clone())?;
        let host = url.strip_prefix("https://")?.split('/').next()?;
        let ours = host == "media.discordapp.net" || host == "cdn.discordapp.com" || host.ends_with(".discordapp.net");
        (ours && self.width? > 0 && self.height? > 0).then(|| (url, self.width.unwrap_or(0), self.height.unwrap_or(0)))
    }
}

fn starter(body: &str, thread_id: u64) -> Option<crate::state::threads::Starter> {
    #[derive(Deserialize)]
    struct Id {
        id: String,
    }
    let list = serde_json::from_str::<Vec<Box<serde_json::value::RawValue>>>(body).ok()?;
    // Message de départ supprimé : le premier message suivant n'est pas le sien.
    let Some(first) = list.first().filter(|m| serde_json::from_str::<Id>(m.get()).is_ok_and(|i| i.id.parse() == Ok(thread_id))) else { return Some(Default::default()) };
    let m = serde_json::from_str::<RawStarter>(first.get()).ok()?;
    let image = m.attachments.iter().filter_map(|a| a.0.as_ref()?.shown(true)).next().or_else(|| m.embeds.iter().filter_map(|e| e.0.as_ref()).filter_map(|e| e.image.as_ref().or(e.thumbnail.as_ref())?.shown(false)).next());
    Some(crate::state::threads::Starter { text: m.content.chars().take(crate::state::threads::STARTER_TEXT).collect(), image })
}

#[derive(Deserialize)]
struct RawArchived {
    // Obligatoire : une réponse d'une autre forme ne passe pas pour une page vide.
    threads: Vec<Item<RawThread>>,
    #[serde(default)]
    members: Vec<Item<RawThreadMember>>,
    #[serde(default)]
    has_more: bool,
}

/// Réponse reçue : événements à publier (`None` : réponse illisible).
fn answer(cmd: &ThreadCmd, body: &str) -> Option<Vec<Event>> {
    let th = |e: ThreadEvent| Event::Threads(e);
    Some(match cmd {
        ThreadCmd::Join { thread_id } => vec![th(ThreadEvent::Joined { thread_id: *thread_id, joined: true })],
        ThreadCmd::Leave { thread_id } => vec![th(ThreadEvent::Joined { thread_id: *thread_id, joined: false })],
        // Le membre du fil est rendu, ou 204 si rien n'a changé : ce qui a été
        // demandé vaut dans les deux cas.
        ThreadCmd::Notify { thread_id, flags } => {
            let got = serde_json::from_str::<RawThreadMember>(body).ok().and_then(|m| m.flags).unwrap_or(*flags);
            vec![th(ThreadEvent::Notifs(vec![(*thread_id, got)]))]
        }
        ThreadCmd::Members { thread_id } => {
            let list = serde_json::from_str::<Vec<Item<RawListedMember>>>(body).ok()?;
            let members = list.into_iter().filter_map(|m| m.0?.named()).collect();
            vec![th(ThreadEvent::MemberList { thread_id: *thread_id, members })]
        }
        ThreadCmd::SetArchived { .. } | ThreadCmd::SetLocked { .. } => {
            let t = serde_json::from_str::<RawThread>(body).ok()?;
            vec![th(ThreadEvent::Upsert { thread: t.to_thread(None)?, joined: t.joined() })]
        }
        ThreadCmd::Starter { thread_id } => vec![th(ThreadEvent::Starter { thread_id: *thread_id, starter: starter(body, *thread_id)? })],
        ThreadCmd::Archived { parent_id, before } => {
            let a = serde_json::from_str::<RawArchived>(body).ok()?;
            let threads = a.threads.iter().filter_map(|t| t.0.as_ref()?.to_thread(None)).filter(|t| t.parent_id == *parent_id).collect();
            let joined = a.members.iter().filter_map(|m| m.0.as_ref()?.id).collect();
            vec![th(ThreadEvent::Archived { parent_id: *parent_id, before: *before, threads, joined, more: a.has_more })]
        }
        ThreadCmd::CreateFromMessage { .. } | ThreadCmd::CreateStandalone { .. } | ThreadCmd::CreatePrivate { .. } | ThreadCmd::CreatePost { .. } => {
            let thread = serde_json::from_str::<RawThread>(body).ok()?.to_thread(None)?;
            let thread_id = thread.id;
            // Le créateur d'un fil en est membre.
            vec![th(ThreadEvent::Upsert { thread, joined: Some(true) }), th(ThreadEvent::Created { thread_id })]
        }
    })
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: ThreadCmd) {
    let req = request(&cmd);
    let (action, thread_id) = action(&cmd);
    let url = format!("{}{}", ctx.api_root, req.path);
    // Message de départ : un échec n'est pas annoncé (la carte reste sans vignette), ni redemandé.
    if let ThreadCmd::Starter { thread_id } = &cmd {
        let starter = match api(&ctx, req.route, req.method, &url, req.body, ACTION_MAX_WAIT, std::future::pending()).await {
            Ok(body) => starter(&body, *thread_id).unwrap_or_default(),
            Err(_) => Default::default(),
        };
        return ctx.out.send(Event::Threads(ThreadEvent::Starter { thread_id: *thread_id, starter }));
    }
    let failed = |problem| Event::Threads(ThreadEvent::Failed { action, thread_id, problem });
    match api(&ctx, req.route, req.method, &url, req.body, ACTION_MAX_WAIT, std::future::pending()).await {
        Ok(body) => match answer(&cmd, &body) {
            Some(events) => events.into_iter().for_each(|e| ctx.out.send(e)),
            None => ctx.out.send(failed(crate::state::Problem::Api(String::new()))),
        },
        // Captcha compris : jamais retenté ni contourné, l'échec est annoncé.
        Err(e) => ctx.out.send(e.into_event(failed)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: u64 = 2000000000000000001;
    const TEXT: u64 = 3000000000000000011;
    const FORUM: u64 = 3000000000000000050;
    const ME: u64 = 1000000000000000001;
    const T1: u64 = 5000000000000000700;
    const T2: u64 = 5000000000000000701;
    const POST: u64 = 5000000000000000710;
    const OLD: u64 = 5000000000000000720;
    const TAG: u64 = 6000000000000000900;

    /// `d` d'un paquet de passerelle, ou le corps d'une réponse REST.
    /// Message de départ d'une publication (carte de la galerie) : extrait,
    /// première image jointe servie par Discord, sinon image d'un embed.
    #[test]
    fn starter_fixture_decodes() {
        use crate::state::threads::Starter;
        let body = fixture("rest_thread_starter.json");
        let s = starter(&body, POST).unwrap_or_else(|| panic!("message de départ"));
        assert_eq!(s.text, "Message de départ fictif.\nDeuxième ligne de la publication.");
        assert_eq!(s.image, Some(("https://media.discordapp.net/attachments/3000000000000000050/7000000000000000002/carte.png".to_string(), 640, 360)), "le fichier texte n'est pas une vignette");
        // Message de départ supprimé : le premier message rendu est une réponse, rien n'est montré.
        assert_eq!(starter(&body, POST + 1), Some(Starter::default()));
        assert_eq!(starter("[]", POST), Some(Starter::default()));
        assert_eq!(starter("{\"message\": \"Missing Access\"}", POST), None);
        // Sans pièce jointe : image de l'embed, par le proxy de Discord seulement.
        let embed = |proxy: &str| format!(r#"[{{"id":"{POST}","content":"{}","embeds":[{{"image":{{"url":"https://exemple.invalid/a.png","proxy_url":"{proxy}","width":10,"height":20}}}}]}}]"#, "x".repeat(500));
        let s = starter(&embed("https://images-ext-1.discordapp.net/external/fictif/a.png"), POST).unwrap_or_else(|| panic!("embed"));
        assert_eq!(s.image, Some(("https://images-ext-1.discordapp.net/external/fictif/a.png".to_string(), 10, 20)));
        assert_eq!(s.text.chars().count(), crate::state::threads::STARTER_TEXT, "extrait borné");
        assert_eq!(starter(&embed("https://exemple.invalid/a.png"), POST).and_then(|s| s.image), None, "jamais un autre site");
    }

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"));
        if v.get("op").is_some() { v["d"].to_string() } else { text }
    }

    fn th_events(events: Vec<Event>) -> Vec<ThreadEvent> {
        events.into_iter().filter_map(|e| if let Event::Threads(t) = e { Some(t) } else { None }).collect()
    }

    #[test]
    fn thread_create_and_update() {
        let ev = th_events(decode("THREAD_CREATE", &fixture("thread_create.json"), Some(ME)));
        let [ThreadEvent::Upsert { thread, joined }] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!((thread.id, thread.guild_id, thread.parent_id, thread.kind), (T1, G, TEXT, ThreadKind::Public));
        assert_eq!((thread.name.as_str(), thread.owner_id, thread.message_count, thread.member_count), ("Sujet", Some(ME), 3, 2));
        assert!(!thread.archived && !thread.locked && thread.pinned);
        assert_eq!((thread.auto_archive, thread.applied_tags.clone()), (1440, vec![TAG]));
        assert_eq!(thread.created_at, 1_700_000_000_000);
        assert_eq!(*joined, Some(true));
        // THREAD_UPDATE sans `member` : appartenance inchangée ; champ illisible toléré.
        let upd = r#"{ "id": "5000000000000000700", "type": 11, "guild_id": "2000000000000000001", "parent_id": "3000000000000000011", "name": "Sujet", "message_count": "beaucoup",
            "thread_metadata": { "archived": true, "locked": true, "archive_timestamp": "2024-01-01T00:00:00.000+00:00", "auto_archive_duration": 60 } }"#;
        let ev = th_events(decode("THREAD_UPDATE", upd, Some(ME)));
        let [ThreadEvent::Upsert { thread, joined: None }] = ev.as_slice() else { panic!("{ev:?}") };
        assert!(thread.archived && thread.locked && thread.message_count == 0);
        assert_eq!(thread.archived_at, Some(1_704_067_200_000));
        // Un salon ordinaire n'est pas un fil.
        assert!(decode("THREAD_CREATE", r#"{ "id": "3", "type": 0, "guild_id": "1", "parent_id": "2" }"#, None).is_empty());
    }

    #[test]
    fn list_sync_members_and_delete() {
        let ev = th_events(decode("THREAD_LIST_SYNC", &fixture("thread_list_sync.json"), Some(ME)));
        let [ThreadEvent::Sync { guild_id, parents, threads, joined }] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!((*guild_id, parents.clone()), (G, Some(vec![TEXT, FORUM])));
        assert_eq!(threads.iter().map(|t| t.id).collect::<Vec<_>>(), vec![T1, T2]);
        assert_eq!(threads[1].kind, ThreadKind::Private);
        assert_eq!(joined, &vec![T2]);
        let ev = th_events(decode("THREAD_MEMBERS_UPDATE", &fixture("thread_members_update.json"), Some(ME)));
        assert!(matches!(ev.as_slice(), [ThreadEvent::Members { thread_id: T1, count: 4 }, ThreadEvent::Joined { thread_id: T1, joined: false }]), "{ev:?}");
        let ev = th_events(decode("THREAD_MEMBER_UPDATE", r#"{ "id": "5000000000000000700", "user_id": "1000000000000000001", "guild_id": "1", "join_timestamp": "2024-01-01T00:00:00Z", "flags": 1 }"#, Some(ME)));
        assert!(matches!(ev.as_slice(), [ThreadEvent::Joined { thread_id: T1, joined: true }]));
        let ev = th_events(decode("THREAD_DELETE", r#"{ "id": "5000000000000000700", "guild_id": "1", "parent_id": "2", "type": 11 }"#, None));
        assert!(matches!(ev.as_slice(), [ThreadEvent::Delete { id: T1 }]));
        for (k, d) in [("THREAD_LIST_SYNC", "{}"), ("THREAD_DELETE", "pas du json"), ("THREAD_MEMBERS_UPDATE", "{}")] {
            assert!(decode(k, d, Some(ME)).is_empty(), "{k}");
        }
    }

    #[test]
    fn guild_threads_and_forum_tags() {
        let mut g: RawGuild = serde_json::from_str(&fixture("guild_create_forum.json")).unwrap_or_else(|e| panic!("{e}"));
        let mut events = Vec::new();
        take(&mut g, &mut events);
        let ev = th_events(events);
        let [ThreadEvent::Forums { guild_id: G, forums }, ThreadEvent::Topics { guild_id: G, .. }, ThreadEvent::AgeGates { guild_id: G, .. }, ThreadEvent::Sync { parents: None, threads, joined, .. }] = ev.as_slice() else { panic!("{ev:?}") };
        let f = &forums[0];
        assert_eq!((f.id, f.media, f.tags.len()), (FORUM, false, 2));
        assert_eq!((f.tags[0].name.as_str(), f.tags[0].moderated, f.tags[0].emoji.clone()), ("Aide", false, Some(Emoji::Unicode("❓".into()))));
        assert!(f.tags[1].moderated);
        assert_eq!(f.tags[1].emoji, Some(Emoji::Custom { id: 7000000000000000077, name: "ok".into(), animated: false }));
        assert_eq!(f.default_reaction, Some(Emoji::Unicode("👍".into())));
        // Le serveur n'est pas répété dans ses fils : il vient du paquet.
        assert_eq!(threads.iter().map(|t| (t.id, t.guild_id, t.parent_id)).collect::<Vec<_>>(), vec![(POST, G, FORUM)]);
        assert_eq!(joined, &vec![POST]);
        // Le salon forum lui-même est gardé, de type Forum.
        let guild = g.into_guild(None, None).unwrap_or_else(|| panic!("serveur"));
        assert!(guild.channels.iter().any(|c| c.id == FORUM && c.kind == crate::state::model::ChannelKind::Forum));
    }

    #[test]
    fn topics_come_with_the_guild_and_follow_channel_updates() {
        // GUILD_CREATE : seuls les salons qui ont un sujet (aucun dans ce jeu, puis un).
        let mut g: RawGuild = serde_json::from_str(&fixture("guild_create_forum.json")).unwrap_or_else(|e| panic!("{e}"));
        let mut events = Vec::new();
        take(&mut g, &mut events);
        let topics = th_events(events).into_iter().find_map(|e| if let ThreadEvent::Topics { guild_id: G, topics } = e { Some(topics) } else { None });
        assert!(topics.is_some_and(|t| t.iter().all(|(_, text)| !text.trim().is_empty())));
        let mut v: Value = serde_json::from_str(&fixture("guild_create_forum.json")).unwrap_or_else(|e| panic!("{e}"));
        v["channels"][0]["topic"] = "  Bienvenue  ".into();
        let id: u64 = v["channels"][0]["id"].as_str().and_then(|s| s.parse().ok()).unwrap_or_else(|| panic!("identifiant"));
        let mut g: RawGuild = serde_json::from_value(v).unwrap_or_else(|e| panic!("{e}"));
        let mut events = Vec::new();
        take(&mut g, &mut events);
        assert!(th_events(events).iter().any(|e| matches!(e, ThreadEvent::Topics { topics, .. } if topics == &vec![(id, "Bienvenue".to_string())])));

        // CHANNEL_UPDATE : le sujet du salon, ou son retrait.
        let ev = th_events(decode("CHANNEL_UPDATE", &fixture("channel_update.json"), Some(ME)));
        let [ThreadEvent::Topic { guild_id: G, channel_id: 3000000000000000012, topic }, ThreadEvent::AgeGate { channel_id: 3000000000000000012, on: false, .. }] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!(topic.as_deref(), Some("Sujet fictif"));
        let ev = th_events(decode("CHANNEL_UPDATE", r#"{ "id": "7", "type": 0, "guild_id": "2000000000000000001", "topic": null }"#, Some(ME)));
        assert!(matches!(ev.as_slice(), [ThreadEvent::Topic { channel_id: 7, topic: None, .. }]), "{ev:?}");
        // Un MP n'a pas de sujet.
        assert!(decode("CHANNEL_UPDATE", r#"{ "id": "7", "type": 1 }"#, Some(ME)).is_empty());
    }

    #[test]
    fn message_carries_its_thread() {
        let mut m: Value = serde_json::from_str(&fixture("message_create_simple.json")).unwrap_or_else(|e| panic!("{e}"));
        m["thread"] = serde_json::from_str(&fixture("thread_create.json")).unwrap_or_else(|e| panic!("{e}"));
        let raw: super::super::raw::RawMessage = serde_json::from_value(m).unwrap_or_else(|e| panic!("{e}"));
        assert!(matches!(raw.thread_event(), Some(Event::Threads(ThreadEvent::Upsert { .. }))));
        let msg = raw.into_message();
        assert_eq!(msg.meta.thread.map(|t| (t.id, t.messages)), Some((T1, 3)));
    }

    #[test]
    fn request_shapes() {
        // Lot N : réglage de notification d'un fil.
        let notify = request(&ThreadCmd::Notify { thread_id: 9, flags: 4 });
        assert_eq!((notify.method, notify.path.as_str(), notify.route.major, notify.body), (Method::PATCH, "/channels/9/thread-members/@me/settings", 9, Some(json!({ "flags": 4 }))));
        let got = |body: &str| answer(&ThreadCmd::Notify { thread_id: 9, flags: 4 }, body).map(th_events);
        assert!(matches!(got("").as_deref(), Some([ThreadEvent::Notifs(l)]) if l == &vec![(9, 4)]), "204 : ce qui a été demandé");
        assert!(matches!(got(r#"{ "id": "9", "user_id": "1", "flags": 9 }"#).as_deref(), Some([ThreadEvent::Notifs(l)]) if l == &vec![(9, 9)]));
        let ev = th_events(decode("THREAD_MEMBER_UPDATE", r#"{ "id": "5000000000000000700", "user_id": "1000000000000000001", "flags": 5 }"#, Some(ME)));
        assert!(matches!(ev.as_slice(), [ThreadEvent::Joined { thread_id: T1, joined: true }, ThreadEvent::Notifs(l)] if l == &vec![(T1, 5)]), "{ev:?}");
        let join = request(&ThreadCmd::Join { thread_id: 9 });
        assert_eq!((join.method, join.path.as_str(), join.route.major, join.body), (Method::PUT, "/channels/9/thread-members/@me", 9, None));
        assert_eq!(request(&ThreadCmd::Leave { thread_id: 9 }).method, Method::DELETE);
        let arch = request(&ThreadCmd::SetArchived { thread_id: 9, archived: true });
        assert_eq!((arch.method, arch.path.as_str(), arch.body), (Method::PATCH, "/channels/9", Some(json!({ "archived": true }))));
        assert_eq!(request(&ThreadCmd::Archived { parent_id: 4, before: None }).path, "/channels/4/threads/archived/public?limit=25");
        assert_eq!(
            request(&ThreadCmd::Archived { parent_id: 4, before: Some(1_704_067_200_000) }).path,
            "/channels/4/threads/archived/public?limit=25&before=2024-01-01T00:00:00.000Z"
        );
        let c = request(&ThreadCmd::CreateFromMessage { channel_id: 2, message_id: 3, name: "Sujet".into() });
        assert_eq!((c.path.as_str(), c.body), ("/channels/2/messages/3/threads", Some(json!({ "name": "Sujet" }))));
        let s = request(&ThreadCmd::CreateStandalone { channel_id: 2, name: "Libre".into(), announcement: false });
        assert_eq!((s.path.as_str(), s.body), ("/channels/2/threads", Some(json!({ "name": "Libre", "type": 11 }))));
        let s = request(&ThreadCmd::CreateStandalone { channel_id: 2, name: "Annonce".into(), announcement: true });
        assert_eq!(s.body, Some(json!({ "name": "Annonce", "type": 10 })));
        // Lot FF2 : fil privé, verrou, liste des membres.
        let s = request(&ThreadCmd::CreatePrivate { channel_id: 2, name: "Entre nous".into() });
        assert_eq!((s.method, s.path.as_str(), s.body), (Method::POST, "/channels/2/threads", Some(json!({ "name": "Entre nous", "type": 12, "invitable": true }))));
        let l = request(&ThreadCmd::SetLocked { thread_id: 9, locked: true });
        assert_eq!((l.method, l.path.as_str(), l.body), (Method::PATCH, "/channels/9", Some(json!({ "locked": true }))));
        let m = request(&ThreadCmd::Members { thread_id: 9 });
        assert_eq!((m.method, m.path.as_str(), m.route.major, m.body), (Method::GET, "/channels/9/thread-members?with_member=true&limit=100", 9, None));
        let body = r#"[{ "id": "9", "user_id": "5", "flags": 1, "member": { "nick": "Capitaine", "user": { "id": "5", "username": "eve" } } },
            { "user_id": "6", "member": { "nick": null, "user": { "id": "6", "username": "bob", "global_name": "Bob" } } }, { "user_id": "7" }, "illisible"]"#;
        let ev = answer(&ThreadCmd::Members { thread_id: 9 }, body).map(th_events);
        assert!(matches!(ev.as_deref(), Some([ThreadEvent::MemberList { thread_id: 9, members }]) if members == &vec![(5, "Capitaine".to_string()), (6, "Bob".to_string()), (7, "7".to_string())]), "{ev:?}");
        assert!(answer(&ThreadCmd::Members { thread_id: 9 }, "{}").is_none());
        let p = request(&ThreadCmd::CreatePost { forum_id: 4, name: "T".into(), content: "M".into(), tags: vec![900] });
        assert_eq!((p.path.as_str(), p.body), ("/channels/4/threads", Some(json!({ "name": "T", "message": { "content": "M" }, "applied_tags": ["900"] }))));
    }

    #[test]
    fn archived_page_answer() {
        let cmd = ThreadCmd::Archived { parent_id: FORUM, before: None };
        let ev = th_events(answer(&cmd, &fixture("rest_threads_archived_public.json")).unwrap_or_default());
        let [ThreadEvent::Archived { parent_id: FORUM, threads, joined, more: true, .. }] = ev.as_slice() else { panic!("{ev:?}") };
        assert_eq!(threads.len(), 1, "le fil d'un autre salon est écarté");
        assert!(threads[0].archived);
        assert_eq!(joined, &vec![OLD]);
        assert!(answer(&cmd, "[]").is_none(), "forme inattendue : échec, pas page vide");
    }
}