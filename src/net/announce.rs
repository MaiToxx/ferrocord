//! Lot T13, hors sondages : ce qu'un message porte en plus (drapeaux
//! d'annonce, référence d'un message système, commande utilisée, appel,
//! résultat de sondage, codes d'invitation), et les requêtes REST des
//! annonces, des invitations et des adresses de pièces jointes expirées.
//!
//! Sources des formes :
//! - documentation officielle (Message Object : `flags`, `call`,
//!   `interaction_metadata`, `interaction`, types de messages, Poll Result
//!   Message ; Crosspost Message ; Follow Announcement Channel ; Get Invite) ;
//! - docs.discord.food, non documentés officiellement : Accept Invite
//!   (`POST /invites/{code}`, corps facultatif, sans `session_id` hors
//!   invitation d'invité) et Refresh Attachment URLs (`POST
//!   /attachments/refresh-urls`, `{"attachment_urls": [1 à 50]}`, réponse
//!   `{"refreshed_urls": [{"original", "refreshed"}]}`).
//!
//! Chaque requête suit un geste de l'utilisateur (clic, ou image en échec
//! affichée pour les adresses expirées : une demande par adresse et par
//! session) et passe par le limiteur commun.

use super::polls::RawPoll;
use super::raw::{de_lenient, de_loose_id, Item, RawUser};
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use crate::state::announce::{AnnounceCmd, AnnounceEvent};
use crate::state::invites::{invite_codes, Invite, InviteCmd, InviteEvent};
use crate::state::model::{EmbedKind, ImageHash, ImageRef, Message};
use crate::state::social::Emoji;
use crate::state::system_msgs::{Call, Command, Extra, PollResult, Ref, CROSSPOSTED, IS_CROSSPOST, MAX_INVITES};
use crate::state::{Event, Problem, SystemKind};
use reqwest::Method;
use serde::Deserialize;
use serde_json::json;
use std::{sync::Arc, time::Duration};

const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);
/// Une carte d'invitation n'attend pas longtemps : elle sera redemandée plus tard.
const INVITE_MAX_WAIT: Duration = Duration::from_secs(10);

/// Commande d'application (`interaction_metadata` ou `interaction`, déprécié).
#[derive(Deserialize)]
pub(super) struct RawInteraction {
    #[serde(default, deserialize_with = "de_lenient")]
    user: Option<RawUser>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct RawCall {
    #[serde(default, deserialize_with = "de_lenient")]
    participants: Option<Vec<String>>,
    #[serde(default, deserialize_with = "de_lenient")]
    ended_timestamp: Option<String>,
}

/// (type, serveur, salon, message) d'une `message_reference`.
pub(super) type RefIds = (u8, Option<u64>, Option<u64>, Option<u64>);

/// Champs du message lus par ce lot (`RawMessage::into_message`).
pub(super) struct Parts {
    pub code: u8,
    pub flags: Option<u64>,
    /// (type, serveur, salon, message) de `message_reference`.
    pub reference: Option<RefIds>,
    pub poll: Option<RawPoll>,
    pub metadata: Option<RawInteraction>,
    pub interaction: Option<RawInteraction>,
    pub call: Option<RawCall>,
}

fn iso_ms(s: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| d.timestamp_millis())
}

/// Résultat d'un sondage : champs documentés de l'embed `poll_result`.
fn poll_result(m: &Message) -> Option<PollResult> {
    let e = m.embeds.iter().find(|e| e.kind == EmbedKind::Other && e.fields.iter().any(|f| f.name == "poll_question_text"))?;
    let field = |name: &str| e.fields.iter().find(|f| f.name == name).map(|f| f.value.as_str());
    let num = |name: &str| field(name).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    let emoji = match (field("victor_answer_emoji_id").and_then(|v| v.parse::<u64>().ok()), field("victor_answer_emoji_name").filter(|n| !n.is_empty())) {
        (Some(id), name) => Some(Emoji::Custom { id, name: name.unwrap_or_default().to_string(), animated: field("victor_answer_emoji_animated") == Some("true") }),
        (None, Some(n)) => Some(Emoji::Unicode(n.to_string())),
        (None, None) => None,
    };
    let winner = field("victor_answer_text").map(|t| (t.to_string(), emoji)).filter(|_| field("victor_answer_id").is_some());
    Some(PollResult { question: field("poll_question_text").unwrap_or_default().to_string(), winner, winner_votes: num("victor_answer_votes"), total: num("total_votes") })
}

/// Complète un message reçu avec ce que le lot T13 y lit.
pub(super) fn finish(m: &mut Message, p: Parts) {
    let mut x = Extra { poll: p.poll.as_ref().and_then(RawPoll::to_poll), ..Extra::default() };
    let flags = p.flags.unwrap_or(0);
    x.crossposted = flags & CROSSPOSTED != 0;
    let reference = p.reference.map(|(_, guild_id, channel_id, message_id)| Ref { guild_id, channel_id, message_id });
    if flags & IS_CROSSPOST != 0 {
        x.source = reference;
    } else if m.system.is_some() {
        x.target = reference;
    }
    // Une référence n'est une réponse que pour un message ordinaire.
    if x.source.is_some() || m.system.is_some() || !matches!(p.code, 0 | 19) {
        m.meta.reply = None;
        m.meta.reply_raw = None;
    }
    if m.system.is_none() {
        x.invites = invite_codes(&m.content, MAX_INVITES);
    }
    if matches!(p.code, 20 | 23) {
        let i = p.metadata.as_ref().filter(|i| i.user.is_some()).or(p.interaction.as_ref());
        let name = p.interaction.as_ref().and_then(|i| i.name.clone()).or_else(|| p.metadata.as_ref().and_then(|i| i.name.clone())).unwrap_or_default();
        if let Some(user) = i.and_then(|i| i.user.as_ref()) {
            x.command = Some(Command { user: user.display(), name, slash: p.code == 20 });
        }
    }
    if let Some(c) = p.call {
        x.call = Some(Call { participants: c.participants.iter().flatten().filter_map(|s| s.parse().ok()).collect(), ended_ms: c.ended_timestamp.as_deref().and_then(iso_ms) });
    }
    if m.system == Some(SystemKind::PollResult) {
        x.result = poll_result(m);
        // Le résultat est dans la phrase : l'embed technique n'est pas montré.
        m.embeds.clear();
    }
    m.meta.extra = (!x.is_empty()).then(|| Box::new(x));
}

// ====================================================================== invitations

#[derive(Deserialize)]
struct RawInviteGuild {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon: Option<String>,
}

#[derive(Deserialize)]
struct RawInviteChannel {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawInvite {
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u8>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild: Option<RawInviteGuild>,
    #[serde(default, deserialize_with = "de_lenient")]
    channel: Option<RawInviteChannel>,
    #[serde(default, deserialize_with = "de_lenient")]
    inviter: Option<RawUser>,
    #[serde(default, deserialize_with = "de_lenient")]
    approximate_member_count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    approximate_presence_count: Option<u32>,
}

/// Corps de Get Invite ; `None` s'il est illisible.
pub(super) fn invite_of(code: &str, body: &str) -> Option<Invite> {
    let r: RawInvite = serde_json::from_str(body).ok()?;
    let kind = r.kind.unwrap_or(if r.guild.is_some() { 0 } else { 1 });
    let guild_id = r.guild.as_ref().and_then(|g| g.id);
    let icon = r.guild.as_ref().and_then(|g| Some(ImageRef::GuildIcon { guild_id: g.id?, hash: ImageHash::parse(g.icon.as_deref()?)? }));
    let name = match kind {
        0 => r.guild.as_ref().and_then(|g| g.name.clone()),
        1 => r.channel.as_ref().and_then(|c| c.name.clone()).filter(|n| !n.is_empty()).or_else(|| r.inviter.as_ref().map(RawUser::display)),
        _ => r.inviter.as_ref().map(RawUser::display),
    };
    Some(Invite { code: code.to_string(), kind, guild_id, name: name.unwrap_or_default(), icon, online: r.approximate_presence_count, members: r.approximate_member_count })
}

pub(super) async fn run_invite(ctx: Arc<Ctx>, cmd: InviteCmd) {
    let send = |e: InviteEvent| ctx.out.send(Event::Invites(e));
    match cmd {
        InviteCmd::Get { code } => {
            let url = format!("{}/invites/{code}?with_counts=true", ctx.api_root);
            let route = Route { template: "GET /invites/{}", major: 0 };
            match api(&ctx, route, Method::GET, &url, None, INVITE_MAX_WAIT, std::future::pending()).await {
                Ok(body) => {
                    let invite = invite_of(&code, &body).ok_or(Problem::Api(String::new()));
                    send(InviteEvent::Loaded { code, invite });
                }
                Err(ApiError::NotFound(_)) => send(InviteEvent::Invalid { code }),
                Err(ApiError::Unauthorized) => ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
                Err(e) => ctx.out.send(e.into_event(|p| Event::Invites(InviteEvent::Loaded { code, invite: Err(p) }))),
            }
        }
    }
}

// ====================================================================== annonces, adresses

#[derive(Deserialize)]
struct RawFlags {
    #[serde(default, deserialize_with = "de_lenient")]
    flags: Option<u64>,
}

#[derive(Deserialize)]
struct RawPair {
    #[serde(default, deserialize_with = "de_lenient")]
    original: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    refreshed: Option<String>,
}

#[derive(Deserialize)]
struct RawRefreshed {
    refreshed_urls: Vec<Item<RawPair>>,
}

/// Réponse de Refresh Attachment URLs : seules les adresses demandées, et
/// des adresses du CDN de Discord, sont reprises.
pub(super) fn refreshed_pairs(asked: &[String], body: &str) -> Vec<(String, String)> {
    let Ok(r) = serde_json::from_str::<RawRefreshed>(body) else { return Vec::new() };
    r.refreshed_urls
        .into_iter()
        .filter_map(|p| p.0)
        .filter_map(|p| Some((p.original?, p.refreshed?)))
        .filter(|(o, n)| asked.contains(o) && n.starts_with("https://cdn.discordapp.com/"))
        .collect()
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: AnnounceCmd) {
    let root = ctx.api_root.clone();
    let send = |e: AnnounceEvent| ctx.out.send(Event::Announce(e));
    match cmd {
        AnnounceCmd::Publish { channel_id, message_id } => {
            let url = format!("{root}/channels/{channel_id}/messages/{message_id}/crosspost");
            let route = Route { template: "POST /channels/{}/messages/{}/crosspost", major: channel_id };
            match api(&ctx, route, Method::POST, &url, None, ACTION_MAX_WAIT, std::future::pending()).await {
                Ok(body) => {
                    let flags = serde_json::from_str::<RawFlags>(&body).ok().and_then(|f| f.flags).unwrap_or(CROSSPOSTED);
                    // Réponse à « Publier » : affichée tout de suite, même si le message n'est plus à l'écran.
                    ctx.out.send_now(Event::Announce(AnnounceEvent::Flags { channel_id, message_id, crossposted: flags & CROSSPOSTED != 0 }));
                }
                Err(e) => ctx.out.send(e.into_event(|problem| Event::Announce(AnnounceEvent::PublishFailed { message_id, problem }))),
            }
        }
        AnnounceCmd::Follow { source, target } => {
            let url = format!("{root}/channels/{source}/followers");
            let route = Route { template: "POST /channels/{}/followers", major: source };
            let body = json!({ "webhook_channel_id": target.to_string() });
            match api(&ctx, route, Method::POST, &url, Some(body), ACTION_MAX_WAIT, std::future::pending()).await {
                Ok(_) => send(AnnounceEvent::Followed { source }),
                Err(e) => ctx.out.send(e.into_event(|problem| Event::Announce(AnnounceEvent::FollowFailed { source, problem }))),
            }
        }
        AnnounceCmd::Refresh { urls } => {
            if ctx.is_bot() || urls.is_empty() {
                return;
            }
            let url = format!("{root}/attachments/refresh-urls");
            let route = Route { template: "POST /attachments/refresh-urls", major: 0 };
            match api(&ctx, route, Method::POST, &url, Some(json!({ "attachment_urls": urls })), ACTION_MAX_WAIT, std::future::pending()).await {
                Ok(body) => send(AnnounceEvent::Refreshed { pairs: refreshed_pairs(&urls, &body) }),
                // Échec : les images restent indisponibles (aucun nouvel essai).
                Err(ApiError::Unauthorized) => ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
                Err(_) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"));
        if v.get("op").is_some() { v["d"].to_string() } else { text }
    }

    #[test]
    fn invite_with_counts() {
        let i = invite_of("ferro", &fixture("rest_invite.json")).unwrap_or_else(|| panic!("invitation"));
        assert_eq!((i.kind, i.guild_id, i.name.as_str(), i.online, i.members), (0, Some(2000000000000000009), "Rustacés", Some(42), Some(1234)));
        assert!(matches!(i.icon, Some(ImageRef::GuildIcon { guild_id: 2000000000000000009, .. })));
        let g = invite_of("g", r#"{ "type": 1, "channel": { "name": "" }, "inviter": { "id": "5", "username": "bob" } }"#).unwrap_or_else(|| panic!("groupe"));
        assert_eq!((g.kind, g.name.as_str(), g.guild_id), (1, "bob", None));
        assert!(invite_of("x", "pas du json").is_none());
    }

    #[test]
    fn only_asked_cdn_urls_are_taken() {
        let asked = vec!["https://cdn.discordapp.com/attachments/fictif/a.png?ex=1&is=1&hm=1&".to_string()];
        let pairs = refreshed_pairs(&asked, &fixture("rest_refresh_urls.json"));
        assert_eq!(pairs, vec![(asked[0].clone(), "https://cdn.discordapp.com/attachments/fictif/a.png?ex=6a000000&is=69f00000&hm=cafe&".to_string())]);
        assert!(refreshed_pairs(&asked, "{}").is_empty());
    }
}
