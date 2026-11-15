//! Widgets de jeux du profil : noms des jeux, suggestions, écriture.
//!
//! Source (lue le 15 novembre 2026) : docs.discord.food,
//! - resources/widgets : « Game Widget Object », `PUT /users/@me/widgets`
//!   (corps `{ "widgets": [ { "id"?, "data": { "type", "games" |
//!   "application_id" } } ] }`, un widget par type, `id` facultatif,
//!   `updated_at` ignoré ; réponse : la liste des widgets gardés),
//!   `GET /users/@me/widgets/suggested-games` (`suggested_games`,
//!   `suggested_wishlist_games`) ;
//! - resources/game : « Get Games » (`GET /games?game_ids=…`, 1 à 25
//!   identifiants ; `id`, `name`, `icon_hash`, `cover_image_hash`).
//!
//! Tout passe par `rest::api` (limiteur commun). Captcha : l'écriture
//! s'arrête et le dit. Compte bot : rien n'est envoyé.

use super::ratelimit::Route;
use super::rest::{api, Ctx};
use super::Event;
use crate::state::model::ImageHash;
use crate::state::quests::Refused;
use crate::state::widgets::{Game, GameWidget, Suggested, WidgetCmd, WidgetEvent, WidgetGame, WidgetKind, GAMES_PER_REQUEST, MAX_COMMENT, MAX_TAGS};
use crate::state::Problem;
use reqwest::Method;
use serde_json::{json, Value};
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);
/// Nom d'un jeu : borne locale.
const MAX_NAME: usize = 100;
/// Jeux suggérés gardés par liste.
const MAX_SUGGESTED: usize = 50;

/// Identifiant en chaîne (forme documentée) ou en nombre.
fn id(v: &Value) -> Option<u64> {
    v.as_str().and_then(|s| s.parse().ok()).or(v.as_u64())
}

fn clip(s: &str, max: usize) -> String {
    s.trim().chars().take(max).collect()
}

fn widget(v: &Value) -> Option<GameWidget> {
    let data = v.get("data")?;
    let kind = WidgetKind::parse(data.get("type")?.as_str()?)?;
    let games = data.get("games").and_then(Value::as_array).map(Vec::as_slice).unwrap_or_default();
    let games: Vec<WidgetGame> = games
        .iter()
        .filter_map(|g| {
            let comment = g.get("comment").and_then(Value::as_str).map(|c| clip(c, MAX_COMMENT)).filter(|c| !c.is_empty());
            let tags = g.get("tags").and_then(Value::as_array).map(|t| t.iter().filter_map(Value::as_str).take(MAX_TAGS).map(|s| clip(s, 40)).collect()).unwrap_or_default();
            Some(WidgetGame { game_id: id(g.get("game_id")?)?, comment, tags })
        })
        .take(kind.max())
        .collect();
    let application_id = data.get("application_id").and_then(id);
    if kind == WidgetKind::Application && application_id.is_none() {
        return None;
    }
    Some(GameWidget { id: v.get("id").and_then(id), kind, games, application_id })
}

/// Widgets d'un profil ; vrai si l'un d'eux n'a pas été compris (ou fait
/// doublon) : les réécrire le perdrait.
pub(super) fn read_widgets(list: &[Value]) -> (Vec<GameWidget>, bool) {
    let mut out: Vec<GameWidget> = Vec::new();
    let mut opaque = false;
    for v in list {
        match widget(v) {
            Some(w) if !out.iter().any(|x| x.kind == w.kind) => out.push(w),
            _ => opaque = true,
        }
    }
    (out, opaque)
}

/// Réponse de `PUT /users/@me/widgets` : la liste des widgets gardés.
pub fn decode_widgets(body: &str) -> Option<Vec<GameWidget>> {
    let list: Vec<Value> = serde_json::from_str(body).ok()?;
    Some(read_widgets(&list).0)
}

/// Réponse de `GET /games` : nom et images de chaque jeu lisible.
pub fn decode_games(body: &str) -> Option<Vec<(u64, Game)>> {
    let list: Vec<Value> = serde_json::from_str(body).ok()?;
    let hash = |g: &Value, key: &str| g.get(key).and_then(Value::as_str).and_then(ImageHash::parse);
    Some(
        list.iter()
            .filter_map(|g| {
                let name = clip(g.get("name")?.as_str()?, MAX_NAME);
                (!name.is_empty()).then_some((id(g.get("id")?)?, Game { name, icon: hash(g, "icon_hash"), cover: hash(g, "cover_image_hash") }))
            })
            .take(GAMES_PER_REQUEST)
            .collect(),
    )
}

/// Réponse de `GET /users/@me/widgets/suggested-games`.
pub fn decode_suggested(body: &str) -> Option<Suggested> {
    let v: Value = serde_json::from_str(body).ok()?;
    let ids = |key: &str| v.get(key).and_then(Value::as_array).map(|l| l.iter().filter_map(id).take(MAX_SUGGESTED).collect()).unwrap_or_default();
    v.is_object().then(|| Suggested { games: ids("suggested_games"), wishlist: ids("suggested_wishlist_games") })
}

/// Corps de `PUT /users/@me/widgets`.
pub(super) fn body(widgets: &[GameWidget]) -> Value {
    let list: Vec<Value> = widgets
        .iter()
        .map(|w| {
            let data = match w.kind {
                WidgetKind::Application => json!({ "type": w.kind.code(), "application_id": w.application_id.map(|a| a.to_string()) }),
                _ => {
                    let games: Vec<Value> = w.games.iter().map(|g| json!({ "game_id": g.game_id.to_string(), "comment": g.comment, "tags": g.tags })).collect();
                    json!({ "type": w.kind.code(), "games": games })
                }
            };
            match w.id {
                Some(id) => json!({ "id": id.to_string(), "data": data }),
                None => json!({ "data": data }),
            }
        })
        .collect();
    json!({ "widgets": list })
}

const fn route(template: &'static str) -> Route {
    Route { template, major: 0 }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: WidgetCmd) {
    let w = Event::Widgets;
    let root = ctx.api_root.as_str();
    let unreadable = || Problem::Api(String::new());
    let ev = match cmd {
        WidgetCmd::Games { ids } if ctx.is_bot() => w(WidgetEvent::Games { ids, result: Err(Problem::Forbidden) }),
        WidgetCmd::Suggested if ctx.is_bot() => w(WidgetEvent::Suggested(Err(Problem::Forbidden))),
        WidgetCmd::Save { .. } if ctx.is_bot() => w(WidgetEvent::Saved(Err(Refused::Other(Problem::Forbidden)))),
        WidgetCmd::Games { ids } => {
            let query: Vec<String> = ids.iter().take(GAMES_PER_REQUEST).map(|i| format!("game_ids={i}")).collect();
            let url = format!("{root}/games?{}", query.join("&"));
            match api(&ctx, route("GET /games"), Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
                Ok(b) => w(WidgetEvent::Games { ids, result: decode_games(&b).ok_or_else(unreadable) }),
                Err(e) => e.into_event(|p| w(WidgetEvent::Games { ids, result: Err(p) })),
            }
        }
        WidgetCmd::Suggested => {
            let url = format!("{root}/users/@me/widgets/suggested-games");
            match api(&ctx, route("GET /users/@me/widgets/suggested-games"), Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
                Ok(b) => w(WidgetEvent::Suggested(decode_suggested(&b).ok_or_else(unreadable))),
                Err(e) => e.into_event(|p| w(WidgetEvent::Suggested(Err(p)))),
            }
        }
        WidgetCmd::Save { widgets } => {
            let url = format!("{root}/users/@me/widgets");
            match api(&ctx, route("PUT /users/@me/widgets"), Method::PUT, &url, Some(body(&widgets)), MAX_WAIT, std::future::pending()).await {
                // Réponse illisible : l'écriture est passée, ce qui est parti fait foi.
                Ok(b) => w(WidgetEvent::Saved(Ok(decode_widgets(&b).unwrap_or(widgets)))),
                Err(e) => match super::quests::refused(e) {
                    Ok(r) => w(WidgetEvent::Saved(Err(r))),
                    Err(fatal) => *fatal,
                },
            }
        }
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/tests/fixtures/nd/{name}.json", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path} : {e}"))
    }

    #[test]
    fn widgets_fixture_decodes_and_writes_back() {
        let list = decode_widgets(&fixture("rest_widgets")).unwrap_or_else(|| panic!("widgets"));
        let kinds: Vec<WidgetKind> = list.iter().map(|w| w.kind).collect();
        assert_eq!(kinds, vec![WidgetKind::Favorite, WidgetKind::Current, WidgetKind::Played, WidgetKind::Application]);
        let fav = &list[0];
        assert_eq!((fav.id, fav.games.len()), (Some(7_100_000_000_000_000_001), 1));
        assert_eq!((fav.games[0].game_id, fav.games[0].comment.as_deref(), fav.games[0].tags.as_slice()), (7_000_000_000_000_000_001, Some("Le meilleur jeu fictif."), ["expert".to_string(), "open_to_teach".to_string()].as_slice()));
        assert_eq!(list[1].games.len(), 2, "jeu sans identifiant écarté");
        assert_eq!(list[3].application_id, Some(7_000_000_000_000_000_009));
        // Un type inconnu rend la liste « opaque » : on ne la réécrira pas.
        let raw: Vec<Value> = serde_json::from_str(&fixture("rest_widgets")).unwrap_or_default();
        assert!(read_widgets(&raw).1);
        // Écriture : identifiants en chaînes, `id` absent pour un widget neuf.
        let mut new = GameWidget::new(WidgetKind::Wanted);
        new.games.push(WidgetGame::new(5));
        assert_eq!(
            body(&[fav.clone(), new, list[3].clone()]),
            json!({ "widgets": [
                { "id": "7100000000000000001", "data": { "type": "favorite_games", "games": [{ "game_id": "7000000000000000001", "comment": "Le meilleur jeu fictif.", "tags": ["expert", "open_to_teach"] }] } },
                { "data": { "type": "want_to_play_games", "games": [{ "game_id": "5", "comment": null, "tags": [] }] } },
                { "id": "7100000000000000004", "data": { "type": "application", "application_id": "7000000000000000009" } },
            ] })
        );
        assert!(decode_widgets("{}").is_none());
    }

    #[test]
    fn games_and_suggestions_fixtures_decode() {
        let games = decode_games(&fixture("rest_games")).unwrap_or_else(|| panic!("jeux"));
        assert_eq!(games.len(), 2, "jeu sans nom écarté");
        assert_eq!((games[0].0, games[0].1.name.as_str()), (7_000_000_000_000_000_001, "Course Fictive 6"));
        assert!(games[0].1.cover.is_some() && games[0].1.icon.is_some() && games[1].1.image().is_none());
        let s = decode_suggested(&fixture("rest_widgets_suggested")).unwrap_or_else(|| panic!("suggestions"));
        assert_eq!((s.games.as_slice(), s.wishlist.as_slice()), ([7_000_000_000_000_000_003, 7_000_000_000_000_000_004].as_slice(), [7_000_000_000_000_000_005].as_slice()));
        assert!(decode_suggested("[]").is_none() && decode_games("{}").is_none());
    }
}
