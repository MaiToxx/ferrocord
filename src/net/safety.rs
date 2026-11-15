//! Centre de sécurité (lot ND), en lecture.
//!
//! Source : docs.discord.food, resources/safety-hub, « Get Safety Hub » :
//! `GET /safety-hub/@me` renvoie `{ classifications, guild_classifications,
//! account_standing: { state }, is_dsa_eligible, is_appeal_eligible,
//! username, appeal_eligibility }`. Jeu d'essai synthétique :
//! `tests/fixtures/nd/rest_safety_hub.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::safety::{SafetyCmd, SafetyEvent, SafetyHub, Standing, Violation, MAX_VIOLATIONS};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct RawAction {
    #[serde(default, deserialize_with = "de_lenient")]
    descriptions: Option<Vec<Item<String>>>,
}

#[derive(Deserialize)]
struct RawClassification {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    explainer_link: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    actions: Option<Vec<Item<RawAction>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    max_expiration_time: Option<String>,
}

#[derive(Deserialize)]
struct RawStanding {
    #[serde(default, deserialize_with = "de_lenient")]
    state: Option<u64>,
}

#[derive(Deserialize)]
struct RawHub {
    #[serde(default, deserialize_with = "de_lenient")]
    classifications: Option<Vec<Item<RawClassification>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_classifications: Option<Vec<serde_json::Value>>,
    #[serde(default, deserialize_with = "de_lenient")]
    account_standing: Option<RawStanding>,
    #[serde(default, deserialize_with = "de_lenient")]
    is_appeal_eligible: Option<bool>,
}

fn text(s: &str, max: usize) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(max).collect()
}

fn violation(c: &RawClassification) -> Option<Violation> {
    let description = text(c.description.as_deref()?, 300);
    let actions = c.actions.iter().flatten().filter_map(|a| a.0.as_ref()).flat_map(|a| a.descriptions.iter().flatten()).filter_map(|d| d.0.as_deref()).map(|d| text(d, 300)).filter(|d| !d.is_empty()).take(8).collect();
    // Lien d'explication : seulement une adresse https sans espace.
    let link = c.explainer_link.as_deref().filter(|u| u.len() <= 512 && u.starts_with("https://") && !u.chars().any(|ch| ch.is_whitespace() || ch.is_control())).map(str::to_owned);
    (!description.is_empty()).then(|| Violation { id: c.id.unwrap_or(0), description, actions, expires_ms: super::quests::iso_ms(c.max_expiration_time.as_deref()), link })
}

/// Réponse de « Get Safety Hub ». `None` : pas d'état du compte lisible.
pub fn decode_safety_hub(body: &str) -> Option<SafetyHub> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    let hub: RawHub = serde_json::from_str(body).ok()?;
    let standing = Standing::from_code(hub.account_standing?.state?);
    let violations = hub.classifications.iter().flatten().filter_map(|c| c.0.as_ref().and_then(violation)).take(MAX_VIOLATIONS).collect();
    Some(SafetyHub { standing, violations, guild_violations: hub.guild_classifications.map_or(0, |g| g.len()), appeal_eligible: hub.is_appeal_eligible.unwrap_or(false) })
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: SafetyCmd) {
    let SafetyCmd::Load = cmd;
    let done = |r| Event::SafetyHub(SafetyEvent::Loaded(r));
    if ctx.is_bot() {
        ctx.out.send(done(Err(Problem::Forbidden)));
        return;
    }
    let url = format!("{}/safety-hub/@me", ctx.api_root);
    let ev = match api(&ctx, Route { template: "GET /safety-hub/@me", major: 0 }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_safety_hub(&b)) {
        Ok(Some(hub)) => done(Ok(hub)),
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
    fn hub_fixture_gives_standing_and_readable_violations() {
        let hub = decode_safety_hub(include_str!("../../tests/fixtures/nd/rest_safety_hub.json")).unwrap_or_else(|| panic!("état attendu"));
        assert_eq!((hub.standing, hub.guild_violations, hub.appeal_eligible), (Standing::Limited, 1, true));
        assert_eq!(hub.violations.len(), 2);
        let v = &hub.violations[0];
        assert_eq!((v.id, v.description.as_str()), (1000000000000000971, "Message synthétique jugé contraire aux règles."));
        assert_eq!(v.actions, vec!["Envoi de messages privés limité.", "Avertissement."]);
        assert_eq!(v.link.as_deref(), Some("https://discord.com/safety/exemple-fictif"));
        assert!(v.expires_ms.is_some());
        assert_eq!((hub.violations[1].link.as_deref(), hub.violations[1].expires_ms), (None, None), "lien en clair refusé");
        assert_eq!(decode_safety_hub("{}"), None);
        assert_eq!(decode_safety_hub("[]"), None);
        assert_eq!(decode_safety_hub(r#"{"account_standing":{"state":100}}"#).map(|h| (h.standing, h.violations.len())), Some((Standing::AllGood, 0)));
    }
}
