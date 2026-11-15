//! Suggestions d'amis (lot ND).
//!
//! Source : docs.discord.food, resources/relationships : « List Friend
//! Suggestions » (liste d'objets `suggested_user`, `reasons`,
//! `mutual_friends_count`) et « Remove Friend Suggestion » (204). La raison
//! porte `platform` dans le tableau de la source et `platform_type` dans
//! son exemple : les deux sont lus. Jeu d'essai synthétique :
//! `tests/fixtures/nd/rest_friend_suggestions.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, Item, RawUser};
use super::rest::{api, ApiError, Ctx};
use crate::state::suggestions::{Suggestion, SuggestionCmd, SuggestionEvent, MAX_SUGGESTIONS};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct RawReason {
    #[serde(default, deserialize_with = "de_lenient")]
    platform: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    platform_type: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawSuggestion {
    #[serde(default, deserialize_with = "de_lenient")]
    suggested_user: Option<RawUser>,
    #[serde(default, deserialize_with = "de_lenient")]
    reasons: Option<Vec<Item<RawReason>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    mutual_friends_count: Option<u32>,
}

fn text(s: Option<&str>, max: usize) -> String {
    s.map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(max).collect()
}

/// Réponse de « List Friend Suggestions ». `None` : pas une liste.
pub fn decode_suggestions(body: &str) -> Option<Vec<Suggestion>> {
    let list: Vec<Item<RawSuggestion>> = serde_json::from_str(body).ok()?;
    let one = |s: &RawSuggestion| {
        let u = s.suggested_user.as_ref()?;
        let username = text(Some(u.username()), 64);
        let name = text(u.global_name().or(Some(u.username())), 64);
        let reason = s.reasons.iter().flatten().filter_map(|r| r.0.as_ref()).find_map(|r| {
            let platform = text(r.platform.as_deref().or(r.platform_type.as_deref()), 32);
            let name = text(r.name.as_deref(), 64);
            (!platform.is_empty() && !name.is_empty()).then_some((platform, name))
        });
        (!name.is_empty()).then(|| Suggestion { user_id: u.id, name, username, avatar: u.avatar(), reason, mutual_friends: s.mutual_friends_count })
    };
    let mut out: Vec<Suggestion> = Vec::new();
    for s in list.iter().filter_map(|s| s.0.as_ref().and_then(one)) {
        if out.len() < MAX_SUGGESTIONS && !out.iter().any(|x| x.user_id == s.user_id) {
            out.push(s);
        }
    }
    Some(out)
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: SuggestionCmd) {
    if ctx.is_bot() {
        if cmd == SuggestionCmd::List {
            ctx.out.send(Event::Suggestions(SuggestionEvent::List(Err(Problem::Forbidden))));
        }
        return;
    }
    match cmd {
        SuggestionCmd::List => {
            let url = format!("{}/friend-suggestions", ctx.api_root);
            let done = |r| Event::Suggestions(SuggestionEvent::List(r));
            let ev = match api(&ctx, Route { template: "GET /friend-suggestions", major: 0 }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_suggestions(&b)) {
                Ok(Some(list)) => done(Ok(list)),
                Ok(None) => done(Err(Problem::Api(String::new()))),
                Err(ApiError::Captcha) => done(Err(Problem::Forbidden)),
                Err(e) => e.into_event(|p| done(Err(p))),
            };
            ctx.out.send(ev);
        }
        SuggestionCmd::Dismiss { user_id } => {
            // La suggestion a déjà quitté la liste ; un échec ne change rien d'autre (401 excepté).
            let url = format!("{}/friend-suggestions/{user_id}", ctx.api_root);
            if let Err(ApiError::Unauthorized) = api(&ctx, Route { template: "DELETE /friend-suggestions/{}", major: 0 }, Method::DELETE, &url, None, MAX_WAIT, std::future::pending()).await {
                ctx.out.send(ApiError::Unauthorized.into_event(Event::Error));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggestions_fixture_keeps_readable_users_once() {
        let list = decode_suggestions(include_str!("../../tests/fixtures/nd/rest_friend_suggestions.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|s| (s.user_id, s.name.as_str(), s.username.as_str())).collect::<Vec<_>>(), vec![(1000000000000000701, "Camille Fictive", "camille_fictive"), (1000000000000000702, "denis_fictif", "denis_fictif")]);
        assert_eq!(list[0].reason, Some(("steam".to_owned(), "CamilleSynthétique".to_owned())));
        assert_eq!((list[0].mutual_friends, list[1].reason.clone(), list[1].mutual_friends), (Some(2), Some(("xbox".to_owned(), "Denis#0000".to_owned())), None));
        assert_eq!(decode_suggestions("{}"), None);
    }
}
