//! Inventaire du compte, en lecture (lot ND).
//!
//! Source : docs.discord.food, resources/entitlement : « List User
//! Entitlements » (`with_sku=true`, `exclude_ended=true`) et « List User
//! Giftable Entitlements ». Les deux réponses sont des listes d'objets
//! « Entitlement ». Aucune écriture. Jeu d'essai synthétique :
//! `tests/fixtures/nd/rest_entitlements.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::inventory::{Entitlement, InventoryCmd, InventoryEvent, Origin, Owned, MAX_ENTITLEMENTS};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct RawSku {
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
}

#[derive(Deserialize)]
struct RawEntitlement {
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    sku_id: Option<u64>,
    #[serde(default, rename = "type", deserialize_with = "de_lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    deleted: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    consumed: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    starts_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    ends_at: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    sku: Option<RawSku>,
}

/// Liste d'objets « Entitlement » ; les droits supprimés sont écartés.
/// `None` : pas une liste.
pub fn decode_entitlements(body: &str) -> Option<Vec<Entitlement>> {
    let list: Vec<Item<RawEntitlement>> = serde_json::from_str(body).ok()?;
    let name = |e: &RawEntitlement| e.sku.as_ref().and_then(|s| s.name.as_deref()).map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(200).collect();
    let one = |e: &RawEntitlement| {
        (e.deleted != Some(true)).then_some(())?;
        Some(Entitlement {
            id: e.id?,
            sku_id: e.sku_id?,
            name: name(e),
            origin: Origin::from_code(e.kind.unwrap_or(u64::MAX)),
            starts_ms: super::quests::iso_ms(e.starts_at.as_deref()),
            ends_ms: super::quests::iso_ms(e.ends_at.as_deref()),
            consumed: e.consumed == Some(true),
        })
    };
    Some(list.iter().filter_map(|e| e.0.as_ref().and_then(one)).take(MAX_ENTITLEMENTS).collect())
}

async fn list(ctx: &Ctx, template: &'static str, path: &str) -> Result<Vec<Entitlement>, ApiError> {
    let url = format!("{}{path}", ctx.api_root);
    let body = api(ctx, Route { template, major: 0 }, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await?;
    decode_entitlements(&body).ok_or_else(|| ApiError::Other(Problem::Api(String::new())))
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: InventoryCmd) {
    let InventoryCmd::Load = cmd;
    let done = |r| Event::Inventory(InventoryEvent::Loaded(r));
    if ctx.is_bot() {
        ctx.out.send(done(Err(Problem::Forbidden)));
        return;
    }
    let mine = list(&ctx, "GET /users/@me/entitlements", "/users/@me/entitlements?with_sku=true&exclude_ended=true").await;
    let both = match mine {
        Ok(mine) => list(&ctx, "GET /users/@me/entitlements/gifts", "/users/@me/entitlements/gifts").await.map(|giftable| Owned { mine, giftable }),
        Err(e) => Err(e),
    };
    let ev = match both {
        Ok(owned) => done(Ok(owned)),
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
    fn entitlements_fixture_drops_deleted_and_unreadable_entries() {
        let list = decode_entitlements(include_str!("../../tests/fixtures/nd/rest_entitlements.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(list.iter().map(|e| (e.name.as_str(), e.origin)).collect::<Vec<_>>(), vec![("Pluie fictive", Origin::Purchase), ("Décoration fictive", Origin::Gift), ("", Origin::Quest)]);
        assert!(list[0].starts_ms.is_some() && list[0].ends_ms.is_none() && list[2].consumed);
        assert_eq!(decode_entitlements("{}"), None);
    }
}
