//! Découverte de serveurs (lot ND), en lecture.
//!
//! Source : docs.discord.food, resources/discovery. « List Discoverable
//! Guilds » renvoie `{ guilds, total, limit, offset }` ; « Search
//! Discoverable Guilds » renvoie une liste d'objets de même forme (les deux
//! enveloppes sont lues) ; « List Discovery Categories » renvoie
//! `[{ id, name, is_primary }]`. Jeux d'essai synthétiques :
//! `tests/fixtures/nd/rest_discoverable_guilds.json`,
//! `tests/fixtures/nd/rest_discovery_categories.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::discovery::{Category, DiscoveryCmd, DiscoveryEvent, Found, PAGE};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);
const MAX_CATEGORIES: usize = 40;

#[derive(Deserialize)]
struct RawGuild {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    vanity_url_code: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    approximate_member_count: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    approximate_presence_count: Option<u64>,
}

#[derive(Deserialize)]
struct RawPage {
    #[serde(default, deserialize_with = "de_lenient")]
    guilds: Option<Vec<Item<RawGuild>>>,
}

#[derive(Deserialize)]
struct RawCategory {
    #[serde(default, deserialize_with = "de_lenient")]
    id: Option<u32>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    is_primary: Option<bool>,
}

fn text(s: Option<&str>, max: usize) -> String {
    s.map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(max).collect()
}

fn word(s: Option<&str>, max: usize) -> Option<&str> {
    s.filter(|s| !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'))
}

fn found(g: &RawGuild) -> Option<Found> {
    let guild_id = g.id?;
    let name = text(g.name.as_deref(), 100);
    let icon = word(g.icon.as_deref(), 64).map(|hash| format!("https://cdn.discordapp.com/icons/{guild_id}/{hash}.png?size=64"));
    (!name.is_empty()).then(|| Found {
        guild_id,
        name,
        description: text(g.description.as_deref(), 300),
        icon,
        members: g.approximate_member_count.unwrap_or(0),
        online: g.approximate_presence_count.unwrap_or(0),
        vanity: word(g.vanity_url_code.as_deref(), 32).map(str::to_owned),
    })
}

/// Liste de serveurs : l'enveloppe `{ guilds }` ou la liste nue. `None` : ni l'une ni l'autre.
pub fn decode_discoverable(body: &str) -> Option<Vec<Found>> {
    let list = if body.trim_start().starts_with('[') { serde_json::from_str::<Vec<Item<RawGuild>>>(body).ok()? } else { serde_json::from_str::<RawPage>(body).ok()?.guilds? };
    let mut out: Vec<Found> = Vec::new();
    for f in list.iter().filter_map(|g| g.0.as_ref().and_then(found)) {
        if out.len() < 2 * PAGE && !out.iter().any(|x| x.guild_id == f.guild_id) {
            out.push(f);
        }
    }
    Some(out)
}

/// Catégories principales (ou toutes si aucune n'est marquée principale).
pub fn decode_discovery_categories(body: &str) -> Option<Vec<Category>> {
    let list: Vec<Item<RawCategory>> = serde_json::from_str(body).ok()?;
    let all: Vec<(Category, bool)> = list
        .iter()
        .filter_map(|c| c.0.as_ref())
        .filter_map(|c| {
            let name = text(c.name.as_deref(), 40);
            (!name.is_empty()).then_some((Category { id: c.id?, name }, c.is_primary.unwrap_or(false)))
        })
        .collect();
    let primary = all.iter().any(|(_, p)| *p);
    let mut out: Vec<Category> = Vec::new();
    for (c, p) in all {
        if (p || !primary) && out.len() < MAX_CATEGORIES && !out.iter().any(|x| x.id == c.id) {
            out.push(c);
        }
    }
    Some(out)
}

/// Adresse et gabarit de limite d'une demande de serveurs.
fn search_url(root: &str, query: &str, category: Option<u32>) -> Option<(String, &'static str)> {
    let (path, template) = if query.is_empty() { ("discoverable-guilds", "GET /discoverable-guilds") } else { ("discoverable-guilds/search", "GET /discoverable-guilds/search") };
    let mut u = reqwest::Url::parse(&format!("{root}/{path}")).ok()?;
    {
        let mut q = u.query_pairs_mut();
        if !query.is_empty() {
            q.append_pair("query", query);
        }
        q.append_pair("limit", &PAGE.to_string());
        if let Some(c) = category {
            q.append_pair(if query.is_empty() { "categories" } else { "category_id" }, &c.to_string());
        }
    }
    Some((u.into(), template))
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: DiscoveryCmd) {
    let bot = ctx.is_bot();
    let ev = match cmd {
        DiscoveryCmd::Categories { locale } => {
            let done = |r| Event::Discovery(DiscoveryEvent::Categories(r));
            if bot {
                done(Err(Problem::Forbidden))
            } else {
                let url = format!("{}/discovery/categories?locale={locale}&primary_only=true", ctx.api_root);
                match api(&ctx, Route { template: "GET /discovery/categories", major: 0 }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_discovery_categories(&b)) {
                    Ok(Some(list)) => done(Ok(list)),
                    Ok(None) => done(Err(Problem::Api(String::new()))),
                    Err(ApiError::Captcha) => done(Err(Problem::Forbidden)),
                    Err(e) => e.into_event(|p| done(Err(p))),
                }
            }
        }
        DiscoveryCmd::Search { seq, query, category } => {
            let done = |result| Event::Discovery(DiscoveryEvent::Results { seq, result });
            match search_url(&ctx.api_root, &query, category) {
                _ if bot => done(Err(Problem::Forbidden)),
                None => done(Err(Problem::Api(String::new()))),
                Some((url, template)) => match api(&ctx, Route { template, major: 0 }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_discoverable(&b)) {
                    Ok(Some(list)) => done(Ok(list)),
                    Ok(None) => done(Err(Problem::Api(String::new()))),
                    // Captcha : on s'arrête là, comme pour un refus.
                    Err(ApiError::Captcha) => done(Err(Problem::Forbidden)),
                    Err(e) => e.into_event(|p| done(Err(p))),
                },
            }
        }
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guild_fixture_is_read_in_both_envelopes() {
        let body = include_str!("../../tests/fixtures/nd/rest_discoverable_guilds.json");
        let list = decode_discoverable(body).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|g| (g.guild_id, g.name.as_str(), g.members, g.online, g.vanity.as_deref())).collect::<Vec<_>>(), vec![(1000000000000000801, "Jardin Fictif", 12840, 931, Some("jardin-fictif")), (1000000000000000802, "Atelier Synthétique", 420, 12, None)]);
        assert_eq!(list[0].icon.as_deref(), Some("https://cdn.discordapp.com/icons/1000000000000000801/a11ce0000000000000000000000000b1.png?size=64"));
        assert_eq!((list[1].icon.as_deref(), list[1].description.as_str()), (None, ""));
        // Recherche : la liste nue.
        let v: serde_json::Value = serde_json::from_str(body).unwrap_or_default();
        assert_eq!(decode_discoverable(&v["guilds"].to_string()), Some(list));
        assert_eq!(decode_discoverable("{}"), None);
        assert_eq!(decode_discoverable("3"), None);
    }

    #[test]
    fn categories_keep_primary_ones_and_addresses_are_encoded() {
        let c = decode_discovery_categories(include_str!("../../tests/fixtures/nd/rest_discovery_categories.json")).unwrap_or_default();
        assert_eq!(c.iter().map(|c| (c.id, c.name.as_str())).collect::<Vec<_>>(), vec![(1, "Jeux"), (2, "Musique")]);
        assert_eq!(decode_discovery_categories("{}"), None);
        let root = "http://127.0.0.1:1/api/v10";
        assert_eq!(search_url(root, "", Some(2)), Some((format!("{root}/discoverable-guilds?limit=24&categories=2"), "GET /discoverable-guilds")));
        assert_eq!(search_url(root, "thé & café", None), Some((format!("{root}/discoverable-guilds/search?query=th%C3%A9+%26+caf%C3%A9&limit=24"), "GET /discoverable-guilds/search")));
        assert_eq!(search_url(root, "jeu", Some(1)).map(|u| u.0), Some(format!("{root}/discoverable-guilds/search?query=jeu&limit=24&category_id=1")));
    }
}
