//! Sondages (lot T13) : décodage de l'objet Poll d'un message, des votes de
//! la passerelle et des drapeaux de MESSAGE_UPDATE ; requêtes REST.
//!
//! Formes : documentation officielle (Poll Resource : Poll Object, Poll
//! Media, Poll Answer, Poll Results, Poll Create Request, Get Answer Voters,
//! End Poll ; Gateway Events : MESSAGE_POLL_VOTE_ADD / _REMOVE). Le vote
//! (`PUT /channels/{id}/polls/{message}/answers/@me`, `answer_ids` :
//! tableau d'entiers, vide pour retirer, réponse 204) vient de
//! docs.discord.food (Message › Create Poll Vote) : non documenté
//! officiellement, il suit toujours un clic et passe par le limiteur commun.
//! Chaque champ est lu à part : une forme inattendue ne fait échouer que lui.

use super::raw::{de_lenient, de_loose_id, Item, RawUser};
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use super::Out;
use crate::state::announce::AnnounceEvent;
use crate::state::polls::{Poll, PollAnswer, PollCmd, PollDraft, PollEvent, VOTERS_PAGE};
use crate::state::social::{Emoji, Reactor};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const ACTION_MAX_WAIT: Duration = Duration::from_secs(15);
/// Longueur d'un emoji Unicode acceptée (octets).
const MAX_EMOJI_BYTES: usize = 64;

#[derive(Deserialize)]
struct RawEmoji {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    animated: Option<bool>,
}

impl RawEmoji {
    pub(super) fn emoji(&self) -> Option<Emoji> {
        match (self.id, self.name.as_deref().filter(|n| !n.is_empty())) {
            (Some(id), name) => Some(Emoji::Custom { id, name: name.unwrap_or_default().to_string(), animated: self.animated.unwrap_or(false) }),
            (None, Some(n)) if n.len() <= MAX_EMOJI_BYTES => Some(Emoji::Unicode(n.to_string())),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct RawMedia {
    #[serde(default, deserialize_with = "de_lenient")]
    text: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    emoji: Option<RawEmoji>,
}

#[derive(Deserialize)]
struct RawAnswer {
    #[serde(default, deserialize_with = "de_lenient")]
    answer_id: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    poll_media: Option<RawMedia>,
}

#[derive(Deserialize)]
struct RawCount {
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    count: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    me_voted: Option<bool>,
}

#[derive(Deserialize)]
struct RawResults {
    #[serde(default, deserialize_with = "de_lenient")]
    is_finalized: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    answer_counts: Option<Vec<Item<RawCount>>>,
}

/// Objet Poll d'un message.
#[derive(Deserialize)]
pub(super) struct RawPoll {
    #[serde(default, deserialize_with = "de_lenient")]
    question: Option<RawMedia>,
    #[serde(default, deserialize_with = "de_lenient")]
    answers: Option<Vec<Item<RawAnswer>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    expiry: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    allow_multiselect: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    results: Option<RawResults>,
}

impl RawPoll {
    /// `None` sans aucune réponse lisible.
    pub(super) fn to_poll(&self) -> Option<Poll> {
        let counts: Vec<&RawCount> = self.results.as_ref().and_then(|r| r.answer_counts.as_ref()).into_iter().flatten().filter_map(|c| c.0.as_ref()).collect();
        let answers: Vec<PollAnswer> = self
            .answers
            .iter()
            .flatten()
            .filter_map(|a| a.0.as_ref())
            .filter_map(|a| {
                let id = a.answer_id?;
                let media = a.poll_media.as_ref();
                let c = counts.iter().find(|c| c.id == Some(id));
                Some(PollAnswer {
                    id,
                    text: media.and_then(|m| m.text.clone()).unwrap_or_default(),
                    emoji: media.and_then(|m| m.emoji.as_ref()).and_then(RawEmoji::emoji),
                    count: c.and_then(|c| c.count).unwrap_or(0),
                    me: c.and_then(|c| c.me_voted).unwrap_or(false),
                })
            })
            .collect();
        if answers.is_empty() {
            return None;
        }
        Some(Poll {
            question: self.question.as_ref().and_then(|q| q.text.clone()).unwrap_or_default(),
            answers,
            expiry_ms: self.expiry.as_deref().and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok()).map(|d| d.timestamp_millis()),
            multi: self.allow_multiselect.unwrap_or(false),
            finalized: self.results.as_ref().and_then(|r| r.is_finalized).unwrap_or(false),
            counts_known: self.results.is_some(),
        })
    }
}

#[derive(Deserialize)]
struct RawVote {
    #[serde(default, deserialize_with = "de_loose_id")]
    user_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    message_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    answer_id: Option<u32>,
}

/// Ce que ce lot lit dans MESSAGE_UPDATE (le reste : `gateway.rs`).
#[derive(Deserialize)]
struct RawUpdate {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    channel_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    poll: Option<RawPoll>,
    #[serde(default, deserialize_with = "de_lenient")]
    flags: Option<u64>,
}

pub(super) fn dispatch(kind: &str, d: &str, out: &Out) {
    for e in decode(kind, d) {
        out.send(e);
    }
}

fn decode(kind: &str, d: &str) -> Vec<Event> {
    match kind {
        "MESSAGE_POLL_VOTE_ADD" | "MESSAGE_POLL_VOTE_REMOVE" => {
            let Ok(v) = serde_json::from_str::<RawVote>(d) else { return Vec::new() };
            let (Some(user_id), Some(channel_id), Some(message_id), Some(answer_id)) = (v.user_id, v.channel_id, v.message_id, v.answer_id) else { return Vec::new() };
            vec![Event::Polls(PollEvent::Vote { channel_id, message_id, answer_id, user_id, add: kind.ends_with("ADD") })]
        }
        "MESSAGE_UPDATE" => {
            let Ok(u) = serde_json::from_str::<RawUpdate>(d) else { return Vec::new() };
            let (Some(message_id), Some(channel_id)) = (u.id, u.channel_id) else { return Vec::new() };
            let mut out = Vec::new();
            if let Some(poll) = u.poll.as_ref().and_then(RawPoll::to_poll) {
                out.push(Event::Polls(PollEvent::Replace { channel_id, message_id, poll: Box::new(poll) }));
            }
            if let Some(flags) = u.flags {
                out.push(Event::Announce(AnnounceEvent::Flags { channel_id, message_id, crossposted: flags & crate::state::system_msgs::CROSSPOSTED != 0 }));
            }
            out
        }
        _ => Vec::new(),
    }
}

// ====================================================================== REST

/// Corps de Create Message avec un sondage (Poll Create Request Object).
fn create_body(draft: &PollDraft, nonce: u64) -> Value {
    let answers: Vec<Value> = draft.filled().into_iter().map(|a| json!({ "poll_media": { "text": a } })).collect();
    json!({
        "content": "",
        "nonce": nonce.to_string(),
        "enforce_nonce": true,
        "poll": { "question": { "text": draft.question.trim() }, "answers": answers, "duration": draft.hours, "allow_multiselect": draft.multi, "layout_type": 1 },
    })
}

#[derive(Deserialize)]
struct RawVoters {
    users: Vec<Item<RawUser>>,
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: PollCmd) {
    let root = ctx.api_root.clone();
    let send = |e: PollEvent| ctx.out.send(Event::Polls(e));
    match cmd {
        PollCmd::Vote { channel_id, message_id, answers, previous } => {
            let url = format!("{root}/channels/{channel_id}/polls/{message_id}/answers/@me");
            let route = Route { template: "PUT /channels/{}/polls/{}/answers/@me", major: channel_id };
            let res = api(&ctx, route, Method::PUT, &url, Some(json!({ "answer_ids": answers })), ACTION_MAX_WAIT, std::future::pending()).await;
            match res {
                Ok(_) => send(PollEvent::VoteDone { channel_id, message_id, previous, error: None }),
                Err(ApiError::Unauthorized) => ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
                Err(e) => ctx.out.send(e.into_event(|error| Event::Polls(PollEvent::VoteDone { channel_id, message_id, previous, error: Some(error) }))),
            }
        }
        PollCmd::Voters { channel_id, message_id, answer_id } => {
            let url = format!("{root}/channels/{channel_id}/polls/{message_id}/answers/{answer_id}?limit={VOTERS_PAGE}");
            let route = Route { template: "GET /channels/{}/polls/{}/answers/{}", major: channel_id };
            let res = api(&ctx, route, Method::GET, &url, None, ACTION_MAX_WAIT, std::future::pending()).await;
            let users = match res {
                Ok(body) => match serde_json::from_str::<RawVoters>(&body) {
                    Ok(v) => Ok(v.users.into_iter().filter_map(|u| u.0).take(VOTERS_PAGE).map(|u| Reactor { id: u.id, name: u.display() }).collect()),
                    Err(_) => Err(Problem::Api(String::new())),
                },
                Err(ApiError::Unauthorized) => return ctx.out.send(ApiError::Unauthorized.into_event(Event::Error)),
                Err(e) => match e.into_event(Event::Error) {
                    Event::Error(p) => Err(p),
                    other => return ctx.out.send(other),
                },
            };
            send(PollEvent::Voters { message_id, answer_id, users });
        }
        PollCmd::End { channel_id, message_id } => {
            let url = format!("{root}/channels/{channel_id}/polls/{message_id}/expire");
            let route = Route { template: "POST /channels/{}/polls/{}/expire", major: channel_id };
            let res = api(&ctx, route, Method::POST, &url, None, ACTION_MAX_WAIT, std::future::pending()).await;
            match res {
                // Le message renvoyé arrive aussi par MESSAGE_UPDATE (documenté).
                Ok(body) => {
                    if let Some(poll) = serde_json::from_str::<RawUpdate>(&body).ok().and_then(|u| u.poll).as_ref().and_then(RawPoll::to_poll) {
                        send(PollEvent::Replace { channel_id, message_id, poll: Box::new(poll) });
                    }
                    send(PollEvent::EndDone { message_id, error: None });
                }
                Err(e) => ctx.out.send(e.into_event(|error| Event::Polls(PollEvent::EndDone { message_id, error: Some(error) }))),
            }
        }
        PollCmd::Create { channel_id, draft, nonce } => {
            let url = format!("{root}/channels/{channel_id}/messages");
            let res = api(&ctx, Route::channel_messages_post(channel_id), Method::POST, &url, Some(create_body(&draft, nonce)), ACTION_MAX_WAIT, std::future::pending()).await;
            match res {
                Ok(_) => send(PollEvent::CreateDone { channel_id, error: None }),
                Err(e) => ctx.out.send(e.into_event(|error| Event::Polls(PollEvent::CreateDone { channel_id, error: Some(error) }))),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"));
        let v: Value = serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} : {e}"));
        if v.get("op").is_some() { v["d"].to_string() } else { text }
    }

    #[test]
    fn poll_of_a_message() {
        let v: Value = serde_json::from_str(&fixture("message_create_poll.json")).unwrap_or_default();
        let raw: RawPoll = serde_json::from_value(v["poll"].clone()).unwrap_or_else(|e| panic!("{e}"));
        let p = raw.to_poll().unwrap_or_else(|| panic!("sondage"));
        assert_eq!(p.question, "Où mange-t-on vendredi\u{a0}?");
        assert_eq!(p.answers.iter().map(|a| (a.id, a.text.as_str(), a.count, a.me)).collect::<Vec<_>>(), vec![(1, "Pizzeria", 3, true), (2, "Crêperie", 1, false), (3, "Chez moi", 0, false)]);
        assert_eq!(p.answers[0].emoji, Some(Emoji::Unicode("🍕".into())));
        assert_eq!(p.answers[1].emoji, Some(Emoji::Custom { id: 7000000000000000501, name: "ferris".into(), animated: false }));
        assert!(p.multi && !p.finalized && p.counts_known);
        assert_eq!(p.expiry_ms, Some(1_893_456_000_000));
        // Sans résultats : comptes inconnus ; réponses illisibles ignorées.
        let raw: RawPoll = serde_json::from_str(r#"{ "question": { "text": "Q" }, "answers": [{ "answer_id": 1, "poll_media": { "text": "a" } }, { "answer_id": "x" }, 5], "expiry": null }"#).unwrap_or_else(|e| panic!("{e}"));
        let p = raw.to_poll().unwrap_or_else(|| panic!("sondage"));
        assert!(!p.counts_known && p.answers.len() == 1 && p.expiry_ms.is_none());
        let empty: RawPoll = serde_json::from_str(r#"{ "answers": "beaucoup" }"#).unwrap_or_else(|e| panic!("{e}"));
        assert!(empty.to_poll().is_none());
    }

    #[test]
    fn vote_events_and_updates() {
        let ev = decode("MESSAGE_POLL_VOTE_ADD", &fixture("message_poll_vote_add.json"));
        assert!(matches!(ev.as_slice(), [Event::Polls(PollEvent::Vote { channel_id: 3000000000000000011, message_id: 5000000000000000400, answer_id: 2, user_id: 1000000000000000002, add: true })]), "{ev:?}");
        let ev = decode("MESSAGE_POLL_VOTE_REMOVE", r#"{ "user_id": "1", "channel_id": "2", "message_id": "3", "answer_id": 1 }"#);
        assert!(matches!(ev.as_slice(), [Event::Polls(PollEvent::Vote { add: false, .. })]));
        assert!(decode("MESSAGE_POLL_VOTE_ADD", r#"{ "user_id": "1" }"#).is_empty());
        let ev = decode("MESSAGE_UPDATE", r#"{ "id": "3", "channel_id": "2", "flags": 1 }"#);
        assert!(matches!(ev.as_slice(), [Event::Announce(AnnounceEvent::Flags { message_id: 3, crossposted: true, .. })]), "{ev:?}");
        assert!(decode("MESSAGE_UPDATE", r#"{ "id": "5", "channel_id": "6" }"#).is_empty());
    }

    #[test]
    fn creation_body_follows_the_documentation() {
        let d = PollDraft { question: " Q ".into(), answers: vec!["a".into(), " ".into(), "b".into()], hours: 4, multi: true };
        let b = create_body(&d, 42);
        assert_eq!(b["poll"]["question"]["text"], "Q");
        assert_eq!(b["poll"]["answers"], json!([{ "poll_media": { "text": "a" } }, { "poll_media": { "text": "b" } }]));
        assert_eq!((b["poll"]["duration"].as_u64(), b["poll"]["allow_multiselect"].as_bool(), b["poll"]["layout_type"].as_u64()), (Some(4), Some(true), Some(1)));
        assert_eq!(b["nonce"], "42");
    }
}
