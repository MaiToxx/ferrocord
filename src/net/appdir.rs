//! Répertoire d'applications (lot ND), en lecture.
//!
//! Source : docs.discord.food, resources/application-directory. « Search
//! Applications Directory » renvoie `{ results: [{ type, data }],
//! num_pages, counts_by_category, load_id }` (`type` 1 : application ;
//! les liens et bannières sont ignorés) ; `data` est une application
//! (`id`, `name`, `description`, `icon`, `directory_entry` avec
//! `short_description` et `guild_count`). « List Application Directory
//! Categories » renvoie `[{ id, name }]`. Jeux d'essai synthétiques :
//! `tests/fixtures/nd/rest_app_directory_search.json`,
//! `tests/fixtures/nd/rest_app_directory_categories.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::appdir::{App, AppDirCmd, AppDirEvent, PAGE};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct RawEntry {
    #[serde(default, deserialize_with = "de_lenient")]
    short_description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    guild_count: Option<u64>,
}

#[derive(Deserialize)]
struct RawApp {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    description: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    icon: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    directory_entry: Option<RawEntry>,
}

#[derive(Deserialize)]
struct RawResult {
    #[serde(default, rename = "type", deserialize_with = "de_lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    data: Option<RawApp>,
}

#[derive(Deserialize)]
struct RawSearch {
    #[serde(default, deserialize_with = "de_lenient")]
    results: Option<Vec<Item<RawResult>>>,
}

fn text(s: Option<&str>, max: usize) -> String {
    s.map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(max).collect()
}

fn app(a: &RawApp) -> Option<App> {
    let id = a.id?;
    let name = text(a.name.as_deref(), 100);
    let short = a.directory_entry.as_ref().and_then(|e| e.short_description.as_deref()).filter(|s| !s.trim().is_empty());
    let icon = a.icon.as_deref().filter(|h| !h.is_empty() && h.len() <= 64 && h.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')).map(|hash| format!("https://cdn.discordapp.com/app-icons/{id}/{hash}.png?size=64"));
    (!name.is_empty()).then(|| App { id, name, summary: text(short.or(a.description.as_deref()), 300), icon, guilds: a.directory_entry.as_ref().and_then(|e| e.guild_count) })
}

/// Réponse de « Search Applications Directory ». `None` : forme inattendue.
pub fn decode_app_directory(body: &str) -> Option<Vec<App>> {
    if !body.trim_start().starts_with('{') {
        return None;
    }
    let list = serde_json::from_str::<RawSearch>(body).ok()?.results?;
    let mut out: Vec<App> = Vec::new();
    for a in list.iter().filter_map(|r| r.0.as_ref()).filter(|r| r.kind == Some(1)).filter_map(|r| r.data.as_ref().and_then(app)) {
        if out.len() < 2 * PAGE && !out.iter().any(|x| x.id == a.id) {
            out.push(a);
        }
    }
    Some(out)
}

fn search_url(root: &str, query: &str, category: Option<u32>, locale: &str) -> Option<String> {
    let mut u = reqwest::Url::parse(&format!("{root}/application-directory-static/search")).ok()?;
    {
        let mut q = u.query_pairs_mut();
        if !query.is_empty() {
            q.append_pair("query", query);
        }
        q.append_pair("page", "1").append_pair("page_size", &PAGE.to_string());
        if let Some(c) = category {
            q.append_pair("category_id", &c.to_string());
        }
        q.append_pair("locale", locale);
    }
    Some(u.into())
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: AppDirCmd) {
    let bot = ctx.is_bot();
    let get = |template: &'static str, url: String| {
        let ctx = ctx.clone();
        async move { api(&ctx, Route { template, major: 0 }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await }
    };
    // Captcha : on s'arrête là, comme pour un refus.
    let failed = |e: ApiError, done: &dyn Fn(Problem) -> Event| match e {
        ApiError::Captcha => done(Problem::Forbidden),
        e => e.into_event(done),
    };
    let ev = match cmd {
        AppDirCmd::Categories { locale } => {
            let done = |r| Event::AppDir(AppDirEvent::Categories(r));
            if bot {
                done(Err(Problem::Forbidden))
            } else {
                match get("GET /application-directory-static/categories", format!("{}/application-directory-static/categories?locale={locale}", ctx.api_root)).await.map(|b| super::discovery::decode_discovery_categories(&b)) {
                    Ok(Some(list)) => done(Ok(list)),
                    Ok(None) => done(Err(Problem::Api(String::new()))),
                    Err(e) => failed(e, &|p| done(Err(p))),
                }
            }
        }
        AppDirCmd::Search { seq, query, category, locale } => {
            let done = |result| Event::AppDir(AppDirEvent::Results { seq, result });
            match search_url(&ctx.api_root, &query, category, locale) {
                _ if bot => done(Err(Problem::Forbidden)),
                None => done(Err(Problem::Api(String::new()))),
                Some(url) => match get("GET /application-directory-static/search", url).await.map(|b| decode_app_directory(&b)) {
                    Ok(Some(list)) => done(Ok(list)),
                    Ok(None) => done(Err(Problem::Api(String::new()))),
                    Err(e) => failed(e, &|p| done(Err(p))),
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
    fn search_fixture_keeps_applications_only() {
        let list = decode_app_directory(include_str!("../../tests/fixtures/nd/rest_app_directory_search.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|a| (a.id, a.name.as_str(), a.guilds)).collect::<Vec<_>>(), vec![(1000000000000000961, "Arrosoir Fictif", Some(12500)), (1000000000000000962, "Semoir Synthétique", None)]);
        assert_eq!(list[0].summary, "Rappels d'arrosage pour plantes imaginaires.");
        assert_eq!(list[0].icon.as_deref(), Some("https://cdn.discordapp.com/app-icons/1000000000000000961/a11ce0000000000000000000000000c1.png?size=64"));
        assert_eq!((list[1].summary.as_str(), list[1].icon.as_deref()), ("Description longue reprise faute de résumé.", None));
        assert_eq!(decode_app_directory("[]"), None);
        assert_eq!(decode_app_directory("{}"), None);
        let root = "http://127.0.0.1:1/api/v10";
        assert_eq!(search_url(root, "", None, "fr"), Some(format!("{root}/application-directory-static/search?page=1&page_size=24&locale=fr")));
        assert_eq!(search_url(root, "thé", Some(4), "en-US"), Some(format!("{root}/application-directory-static/search?query=th%C3%A9&page=1&page_size=24&category_id=4&locale=en-US")));
        // Les catégories ont la forme de celles de la découverte, sans `is_primary`.
        let cats = super::super::discovery::decode_discovery_categories(include_str!("../../tests/fixtures/nd/rest_app_directory_categories.json")).unwrap_or_default();
        assert_eq!(cats.iter().map(|c| (c.id, c.name.as_str())).collect::<Vec<_>>(), vec![(4, "Divertissement"), (6, "Outils")]);
    }
}
