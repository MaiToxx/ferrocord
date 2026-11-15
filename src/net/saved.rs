//! Messages enregistrés et rappels (lot ND).
//!
//! Source : docs.discord.food, resources/user. « List Saved Messages »
//! renvoie `{ results: [{ message, save_data }] }` où `save_data` porte
//! `channel_id`, `message_id`, `guild_id?`, `saved_at`, `due_at` ; « Save
//! Message » prend `{ due_at? }` (date ISO 8601 ou nul) ; « Unsave
//! Message » répond 204. Jeu d'essai synthétique :
//! `tests/fixtures/nd/rest_saved_messages.json`.

use super::quests::iso_ms;
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use crate::state::quests::Refused;
use crate::state::saved::{Saved, SavedCmd, SavedEvent, MAX_SAVED};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

fn id(v: &Value) -> Option<u64> {
    v.as_str().and_then(|s| s.parse().ok()).or_else(|| v.as_u64())
}

fn text(v: &Value, max: usize) -> String {
    v.as_str().unwrap_or_default().split_whitespace().collect::<Vec<_>>().join(" ").chars().filter(|c| !c.is_control()).take(max).collect()
}

fn saved(v: &Value) -> Option<Saved> {
    let data = &v["save_data"];
    let (m, author) = (&v["message"], &v["message"]["author"]);
    let name = text(&author["global_name"], 64);
    Some(Saved {
        channel_id: id(&data["channel_id"])?,
        message_id: id(&data["message_id"])?,
        guild_id: id(&data["guild_id"]),
        saved_ms: iso_ms(data["saved_at"].as_str()),
        due_ms: iso_ms(data["due_at"].as_str()),
        author: if name.is_empty() { text(&author["username"], 64) } else { name },
        excerpt: text(&m["content"], 200),
    })
}

/// Réponse de « List Saved Messages » : rappels d'abord (par échéance),
/// puis les autres, du plus récent au plus ancien. `None` : forme inattendue.
pub fn decode_saved_messages(body: &str) -> Option<Vec<Saved>> {
    let v: Value = serde_json::from_str(body).ok()?;
    let list = v.as_object()?.get("results")?.as_array()?;
    let mut out: Vec<Saved> = Vec::new();
    for s in list.iter().filter_map(saved) {
        if out.len() < MAX_SAVED && !out.iter().any(|x| x.channel_id == s.channel_id && x.message_id == s.message_id) {
            out.push(s);
        }
    }
    out.sort_by_key(|s| (s.due_ms.is_none(), s.due_ms, std::cmp::Reverse(s.saved_ms)));
    Some(out)
}

fn save_body(due_ms: Option<i64>) -> Value {
    json!({ "due_at": due_ms.and_then(chrono::DateTime::from_timestamp_millis).map(|d| d.to_rfc3339_opts(chrono::SecondsFormat::Secs, true)) })
}

fn refused(e: ApiError, or: impl FnOnce(Refused) -> Event) -> Event {
    match super::quests::refused(e) {
        Ok(r) => or(r),
        Err(fatal) => *fatal,
    }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: SavedCmd) {
    let bot = ctx.is_bot();
    let root = ctx.api_root.as_str();
    let no = || Refused::Other(Problem::Forbidden);
    let ev = match cmd {
        SavedCmd::List => {
            let done = |r| Event::Saved(SavedEvent::List(r));
            if bot {
                done(Err(Problem::Forbidden))
            } else {
                match api(&ctx, Route { template: "GET /users/@me/saved-messages", major: 0 }, Method::GET, &format!("{root}/users/@me/saved-messages"), None, MAX_WAIT, std::future::pending()).await.map(|b| decode_saved_messages(&b)) {
                    Ok(Some(list)) => done(Ok(list)),
                    Ok(None) => done(Err(Problem::Api(String::new()))),
                    // Captcha : on s'arrête là, comme pour un refus.
                    Err(ApiError::Captcha) => done(Err(Problem::Forbidden)),
                    Err(e) => e.into_event(|p| done(Err(p))),
                }
            }
        }
        SavedCmd::Save { channel_id, message_id, due_ms } => {
            let done = |result| Event::Saved(SavedEvent::Saved { reminder: due_ms.is_some(), result });
            if bot {
                done(Err(no()))
            } else {
                let url = format!("{root}/users/@me/saved-messages/{channel_id}/{message_id}");
                match api(&ctx, Route { template: "PUT /users/@me/saved-messages/{}/{}", major: 0 }, Method::PUT, &url, Some(save_body(due_ms)), MAX_WAIT, std::future::pending()).await {
                    Ok(_) => done(Ok(())),
                    Err(e) => refused(e, |r| done(Err(r))),
                }
            }
        }
        SavedCmd::Remove { channel_id, message_id } => {
            let done = |result| Event::Saved(SavedEvent::Removed { channel_id, message_id, result });
            if bot {
                done(Err(no()))
            } else {
                let url = format!("{root}/users/@me/saved-messages/{channel_id}/{message_id}");
                match api(&ctx, Route { template: "DELETE /users/@me/saved-messages/{}/{}", major: 0 }, Method::DELETE, &url, None, MAX_WAIT, std::future::pending()).await {
                    // Déjà retiré ailleurs : c'est fait.
                    Ok(_) | Err(ApiError::NotFound(_)) => done(Ok(())),
                    Err(e) => refused(e, |r| done(Err(r))),
                }
            }
        }
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_fixture_puts_reminders_first() {
        let list = decode_saved_messages(include_str!("../../tests/fixtures/nd/rest_saved_messages.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|s| (s.message_id, s.due_ms.is_some(), s.author.as_str())).collect::<Vec<_>>(), vec![(1000000000000000982, true, "Alice Fictive"), (1000000000000000981, false, "bob_fictif"), (1000000000000000983, false, "")]);
        assert_eq!((list[0].guild_id, list[0].excerpt.as_str()), (Some(2000000000000000001), "Pense à arroser les semis fictifs demain."));
        assert_eq!(list[2].excerpt, "", "message supprimé : seule la référence reste");
        assert_eq!(decode_saved_messages("[]"), None);
        assert_eq!(decode_saved_messages("{}"), None);
        assert_eq!(save_body(None), json!({ "due_at": null }));
        assert_eq!(save_body(Some(1_767_225_600_000)), json!({ "due_at": "2026-01-01T00:00:00Z" }));
    }
}
