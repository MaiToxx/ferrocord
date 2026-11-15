//! Recherche de messages et historique autour d'un message (lot T12).
//!
//! Recherche : `GET /guilds/{id}/messages/search` (serveur) et
//! `GET /channels/{id}/messages/search` (salon, MP, groupe), réservées aux
//! comptes utilisateur et **non documentées** par Discord. Forme vérifiée le
//! 13 novembre 2026 sur docs.discord.food (« Messages › Search Messages ») :
//! paramètres `content`, `author_id`, `author_type`, `mentions`, `has`,
//! `channel_id`, `pinned`, `min_id`, `max_id`, `sort_by`, `sort_order`,
//! `offset`, `limit` (les tableaux en paramètres répétés) ; réponse
//! `total_results`, `messages` (tableau de tableaux, le message trouvé
//! seul ou marqué `hit`), `threads` ; index pas encore prêt : **202** avec
//! `code` 110000 et `retry_after` (secondes, 0 : « un peu plus tard »).
//!
//! Chaque recherche suit un geste (Entrée, page, tri), passe par le limiteur
//! commun (`api`), et l'index en cours n'est retenté **qu'une fois**, après le
//! délai indiqué (borné). Comptes bot : jamais envoyée.
//!
//! Saut : `GET /channels/{id}/messages?around={id}&limit=50`, documenté
//! (Get Channel Messages).

use super::raw::{de_lenient, Item, RawMessage};
use super::ratelimit::Route;
use super::rest::{api, ApiError, Ctx};
use super::threads::RawThread;
use crate::state::search::{Query, Scope, SearchCmd, SearchEvent, PAGE};
use crate::state::{Event, Problem, HISTORY_PAGE};
use serde::Deserialize;
use serde_json::value::RawValue;
use std::{sync::Arc, time::Duration};

/// Attente maximale avant de partir (limiteur) : au-delà, l'échec est annoncé.
const MAX_WAIT: Duration = Duration::from_secs(10);
/// Code de l'erreur « index pas encore disponible » (docs.discord.food).
const NOT_INDEXED: u64 = 110_000;
/// Délai du nouvel essai quand Discord indique 0 ou rien.
const RETRY_DEFAULT: Duration = Duration::from_secs(2);
/// Délai du nouvel essai, au plus.
const RETRY_MAX: Duration = Duration::from_secs(15);

fn route(scope: Scope) -> Route {
    match scope {
        Scope::Guild(g) => Route { template: "GET /guilds/{}/messages/search", major: g },
        Scope::Channel(c) => Route { template: "GET /channels/{}/messages/search", major: c },
    }
}

/// Adresse de la recherche : seuls les filtres utilisés sont envoyés.
pub(super) fn url(api_root: &str, scope: Scope, q: &Query, offset: usize) -> String {
    let path = match scope {
        Scope::Guild(g) => format!("{api_root}/guilds/{g}/messages/search"),
        Scope::Channel(c) => format!("{api_root}/channels/{c}/messages/search"),
    };
    let mut pairs: Vec<(&str, String)> = Vec::new();
    if !q.content.is_empty() {
        pairs.push(("content", q.content.clone()));
    }
    pairs.extend(q.authors.iter().map(|id| ("author_id", id.to_string())));
    pairs.extend(q.author_types.iter().map(|a| ("author_type", a.code().to_string())));
    pairs.extend(q.mentions.iter().map(|id| ("mentions", id.to_string())));
    pairs.extend(q.has.iter().map(|h| ("has", h.code().to_string())));
    // `channel_id` ne s'applique pas aux salons privés (documenté).
    if let Scope::Guild(_) = scope {
        pairs.extend(q.channels.iter().map(|id| ("channel_id", id.to_string())));
    }
    if let Some(p) = q.pinned {
        pairs.push(("pinned", p.to_string()));
    }
    if let Some(id) = q.min_id {
        pairs.push(("min_id", id.to_string()));
    }
    if let Some(id) = q.max_id {
        pairs.push(("max_id", id.to_string()));
    }
    let (by, order) = q.sort.params();
    pairs.push(("sort_by", by.to_string()));
    pairs.push(("sort_order", order.to_string()));
    if offset > 0 {
        pairs.push(("offset", offset.to_string()));
    }
    pairs.push(("limit", PAGE.to_string()));
    match reqwest::Url::parse_with_params(&path, &pairs) {
        Ok(u) => u.to_string(),
        // Racine de l'API mal formée (ne vient jamais du réseau) : sans paramètres.
        Err(_) => path,
    }
}

#[derive(Deserialize)]
struct RawResults {
    #[serde(default, deserialize_with = "de_lenient")]
    total_results: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    messages: Option<Vec<Vec<Box<RawValue>>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    threads: Option<Vec<Item<RawThread>>>,
    #[serde(default, deserialize_with = "de_lenient")]
    code: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    retry_after: Option<f64>,
}

#[derive(Deserialize)]
struct HitMark {
    #[serde(default, deserialize_with = "de_lenient")]
    hit: Option<bool>,
}

/// Réponse lue.
pub(super) enum Answer {
    Results { total: u64, messages: Vec<RawMessage>, threads: Vec<(u64, String)> },
    /// Index en cours : nouvel essai après ce délai.
    Indexing(Duration),
}

/// Lit une réponse de recherche ; `None` si elle est illisible. Un message
/// illisible est sauté sans faire échouer la page.
pub(super) fn read(body: &str) -> Option<Answer> {
    let r: RawResults = serde_json::from_str(body).ok()?;
    if r.code == Some(NOT_INDEXED) || (r.messages.is_none() && r.retry_after.is_some()) {
        let wait = r.retry_after.filter(|s| s.is_finite() && *s > 0.0).map_or(RETRY_DEFAULT, |s| Duration::from_secs_f64(s.min(RETRY_MAX.as_secs_f64())));
        return Some(Answer::Indexing(wait));
    }
    let groups = r.messages?;
    let messages = groups
        .iter()
        .filter_map(|group| {
            // Autrefois le message trouvé était entouré de son contexte et marqué `hit`.
            let pick = group.iter().find(|m| serde_json::from_str::<HitMark>(m.get()).ok().and_then(|h| h.hit) == Some(true)).or(group.first())?;
            serde_json::from_str::<RawMessage>(pick.get()).ok()
        })
        .collect::<Vec<_>>();
    let threads = r.threads.into_iter().flatten().filter_map(|t| t.0?.to_thread(Some(0)).map(|t| (t.id, t.name))).collect();
    Some(Answer::Results { total: r.total_results.unwrap_or(messages.len() as u64), messages, threads })
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: SearchCmd) {
    match cmd {
        SearchCmd::Query { seq, scope, query, offset } => search(&ctx, seq, scope, &query, offset).await,
        SearchCmd::Around { channel_id, around } => around_page(&ctx, channel_id, around).await,
    }
}

async fn search(ctx: &Ctx, seq: u64, scope: Scope, query: &Query, offset: usize) {
    let failed = |problem| Event::Search(SearchEvent::Failed { seq, problem });
    if ctx.is_bot() {
        // Route réservée aux comptes utilisateur.
        ctx.out.send(failed(Problem::Forbidden));
        return;
    }
    let url = url(&ctx.api_root, scope, query, offset);
    for attempt in 0..2 {
        let body = match api(ctx, route(scope), reqwest::Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
            Ok(b) => b,
            // Captcha compris : jamais retenté, l'échec est annoncé.
            Err(e) => return ctx.out.send(e.into_event(failed)),
        };
        match read(&body) {
            Some(Answer::Results { total, messages, threads }) => {
                super::people::messages(&messages, &ctx.out.people);
                let messages = messages.into_iter().map(RawMessage::into_message).collect();
                return ctx.out.send(Event::Search(SearchEvent::Results { seq, total, messages, threads }));
            }
            Some(Answer::Indexing(wait)) if attempt == 0 => {
                ctx.out.send(Event::Search(SearchEvent::Indexing { seq, retry_ms: u64::try_from(wait.as_millis()).unwrap_or(u64::MAX) }));
                tokio::time::sleep(wait).await;
            }
            Some(Answer::Indexing(_)) => return ctx.out.send(Event::Search(SearchEvent::NotIndexed { seq })),
            None => return ctx.out.send(failed(Problem::Api(String::new()))),
        }
    }
}

async fn around_page(ctx: &Ctx, channel_id: u64, around: u64) {
    let url = format!("{}/channels/{channel_id}/messages?around={around}&limit={HISTORY_PAGE}", ctx.api_root);
    let failed = |problem| Event::Search(SearchEvent::AroundFailed { channel_id, around, problem });
    let res = api(ctx, Route::channel_messages_get(channel_id), reqwest::Method::GET, &url, None, MAX_WAIT, std::future::pending())
        .await
        .and_then(|body| serde_json::from_str::<Vec<Item<RawMessage>>>(&body).map_err(|e| ApiError::Other(Problem::Api(e.to_string()))));
    match res {
        Ok(raw) => {
            let raw: Vec<RawMessage> = raw.into_iter().filter_map(|m| m.0).collect();
            super::people::messages(&raw, &ctx.out.people);
            raw.iter().filter_map(RawMessage::thread_event).for_each(|e| ctx.out.send(e));
            let messages = raw.into_iter().rev().map(RawMessage::into_message).collect();
            ctx.out.send(Event::Search(SearchEvent::Around { channel_id, around, messages }));
        }
        Err(e) => ctx.out.send(e.into_event(failed)),
    }
}

/// Réponse de recherche (jeu d'essai ou aperçu) décodée comme par le réseau.
pub(super) fn decode(body: &str, seq: u64) -> Option<Event> {
    match read(body)? {
        Answer::Results { total, messages, threads } => {
            Some(Event::Search(SearchEvent::Results { seq, total, messages: messages.into_iter().map(RawMessage::into_message).collect(), threads }))
        }
        Answer::Indexing(d) => Some(Event::Search(SearchEvent::Indexing { seq, retry_ms: u64::try_from(d.as_millis()).unwrap_or(u64::MAX) })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::search::{parse, AuthorType, Has, Names, Sort};

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn url_carries_only_the_used_filters() {
        let q = Query { content: "rdv & café".into(), ..Query::default() };
        assert_eq!(
            url("https://x/api/v10", Scope::Guild(7), &q, 0),
            "https://x/api/v10/guilds/7/messages/search?content=rdv+%26+caf%C3%A9&sort_by=timestamp&sort_order=desc&limit=25"
        );
        let q = Query {
            content: String::new(),
            authors: vec![1, 2],
            mentions: vec![3],
            channels: vec![4],
            has: vec![Has::Image, Has::Link],
            author_types: vec![AuthorType::Bot],
            min_id: Some(5),
            max_id: Some(6),
            pinned: Some(false),
            sort: Sort::Oldest,
        };
        assert_eq!(
            url("https://x/api/v10", Scope::Guild(7), &q, 50),
            "https://x/api/v10/guilds/7/messages/search?author_id=1&author_id=2&author_type=bot&mentions=3&has=image&has=link&channel_id=4&pinned=false&min_id=5&max_id=6&sort_by=timestamp&sort_order=asc&offset=50&limit=25"
        );
        // MP : pas de `channel_id`.
        let u = url("https://x/api/v10", Scope::Channel(9), &q, 0);
        assert!(u.starts_with("https://x/api/v10/channels/9/messages/search?") && !u.contains("channel_id"), "{u}");
        let names = Names::default();
        let p = parse("bonjour", &names, 0, Sort::Relevance).unwrap();
        assert!(url("https://x/api/v10", Scope::Channel(1), &p.query, 0).contains("sort_by=relevance&sort_order=desc"));
    }

    #[test]
    fn results_fixture() {
        let Some(Answer::Results { total, messages, threads }) = read(&fixture("rest_search_messages.json")) else { panic!() };
        assert_eq!(total, 57);
        // Le message illisible est sauté ; l'ancien format (contexte + `hit`) donne le message trouvé.
        let ids: Vec<u64> = messages.into_iter().map(|m| m.into_message().id).collect();
        assert_eq!(ids, vec![5000000000000000301, 5000000000000000302, 5000000000000000304]);
        assert_eq!(threads, vec![(5000000000000000777, "Sujet".to_string())]);
    }

    #[test]
    fn index_not_ready() {
        let Some(Answer::Indexing(d)) = read(&fixture("rest_search_indexing.json")) else { panic!() };
        assert_eq!(d, Duration::from_secs(2));
        let Some(Answer::Indexing(d)) = read(r#"{"message":"Index not yet available. Try again later","code":110000,"documents_indexed":0,"retry_after":0}"#) else { panic!() };
        assert_eq!(d, RETRY_DEFAULT, "0 : un peu plus tard");
        let Some(Answer::Indexing(d)) = read(r#"{"code":110000,"retry_after":1e9}"#) else { panic!() };
        assert_eq!(d, RETRY_MAX);
        assert!(read("[]").is_none() && read("pas du json").is_none() && read("{}").is_none());
        // Champs inconnus ou de forme inattendue : pas d'échec.
        let Some(Answer::Results { total: 0, messages, .. }) = read(r#"{"total_results":"?","messages":[],"analytics_id":3,"threads":"x"}"#) else { panic!() };
        assert!(messages.is_empty());
    }
}
