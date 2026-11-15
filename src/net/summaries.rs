//! Résumés de salon (lot ND), en lecture.
//!
//! Source : docs.discord.food, resources/message, « Get Conversation
//! Summaries » : `GET /channels/{channel.id}/summaries` renvoie
//! `{ summaries: [conversation summary] }`. Jeu d'essai synthétique :
//! `tests/fixtures/nd/rest_channel_summaries.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::summaries::{Summary, SummaryCmd, SummaryEvent, MAX_SUMMARIES};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct RawSummary {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    topic: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    summ_short: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    people: Option<Vec<serde_json::Value>>,
    #[serde(default, rename = "unsafe", deserialize_with = "de_lenient")]
    not_safe: Option<bool>,
    #[serde(default, deserialize_with = "de_loose_id")]
    start_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    count: Option<u32>,
}

#[derive(Deserialize)]
struct RawList {
    #[serde(default, deserialize_with = "de_lenient")]
    summaries: Option<Vec<Item<RawSummary>>>,
}

fn text(s: Option<&str>, max: usize) -> String {
    s.map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Réponse de « Get Conversation Summaries », du plus récent au plus ancien.
/// `None` : pas la forme attendue.
pub fn decode_summaries(body: &str) -> Option<Vec<Summary>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    let list = serde_json::from_str::<RawList>(body).ok()?.summaries?;
    let mut out: Vec<Summary> = list
        .iter()
        .filter_map(|s| s.0.as_ref())
        .filter(|s| s.not_safe != Some(true))
        .filter_map(|s| {
            let (topic, text) = (text(s.topic.as_deref(), 120), text(s.summ_short.as_deref(), 600));
            (!topic.is_empty() || !text.is_empty()).then_some(Summary { id: s.id?, topic, text, start_id: s.start_id, messages: s.count.unwrap_or(0), people: s.people.as_ref().map_or(0, |p| p.len().min(999) as u32) })
        })
        .collect();
    out.sort_by_key(|s| std::cmp::Reverse(s.id));
    out.dedup_by_key(|s| s.id);
    out.truncate(MAX_SUMMARIES);
    Some(out)
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: SummaryCmd) {
    let SummaryCmd::Load { channel_id } = cmd;
    let done = |result| Event::Summaries(SummaryEvent::Loaded { channel_id, result });
    if ctx.is_bot() {
        ctx.out.send(done(Err(Problem::Forbidden)));
        return;
    }
    let url = format!("{}/channels/{channel_id}/summaries", ctx.api_root);
    let route = Route { template: "GET /channels/{}/summaries", major: channel_id };
    let ev = match api(&ctx, route, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_summaries(&b)) {
        Ok(Some(list)) => done(Ok(list)),
        Ok(None) => done(Err(Problem::Api(String::new()))),
        // Captcha : on s'arrête là, comme pour un refus.
        Err(ApiError::Captcha) => done(Err(Problem::Forbidden)),
        Err(e) => e.into_event(|p| done(Err(p))),
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summaries_fixture_skips_unsafe_and_sorts_newest_first() {
        let list = decode_summaries(include_str!("../../tests/fixtures/nd/rest_channel_summaries.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|s| (s.id, s.topic.as_str(), s.messages, s.people)).collect::<Vec<_>>(), vec![(1000000000000000912, "Choix du terreau", 14, 3), (1000000000000000911, "Arrosage des semis fictifs", 2, 2)]);
        assert_eq!(list[1].start_id, Some(1000000000000000901));
        assert!(list[0].text.starts_with("Discussion synthétique"));
        assert_eq!(decode_summaries("[]"), None);
        assert_eq!(decode_summaries("{}"), None);
        assert_eq!(decode_summaries(r#"{"summaries":[]}"#), Some(Vec::new()));
    }
}
