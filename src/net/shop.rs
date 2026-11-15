//! Boutique en consultation (lot ND) : `GET /collectibles-categories`.
//!
//! Source : docs.discord.food, resources/collectibles, « Get Collectibles
//! Categories » (réponse : liste de « Collectible Category Object », chacun
//! avec `products`). Aucun paramètre n'est envoyé. Lecture seule : aucune
//! requête d'achat n'existe dans Ferrocord. Jeu d'essai synthétique :
//! `tests/fixtures/nd/rest_collectibles_categories.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::shop::{Category, Price, Product, ProductKind, ShopCmd, ShopEvent, MAX_CATEGORIES, MAX_PRODUCTS};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);
const MAX_TEXT: usize = 300;

#[derive(Deserialize)]
struct RawAssets {
    #[serde(default, deserialize_with = "de_lenient")]
    static_image_url: Option<String>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(default, deserialize_with = "de_lenient")]
    assets: Option<RawAssets>,
    /// Effet de profil : sa vignette.
    #[serde(default, rename = "thumbnailPreviewSrc", deserialize_with = "de_lenient")]
    thumbnail: Option<String>,
}

/// « Unit Price » (resources/store, exemple : `currency`, `amount`, `exponent`).
#[derive(Deserialize)]
struct RawUnit {
    #[serde(default, deserialize_with = "de_lenient")]
    currency: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    amount: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    exponent: Option<u8>,
}

#[derive(Deserialize)]
struct RawCountry {
    #[serde(default, deserialize_with = "de_lenient")]
    prices: Option<Vec<Item<RawUnit>>>,
}

/// « Subscription Prices Structure » : seul `country_prices` est lu.
#[derive(Deserialize)]
struct RawPrices {
    #[serde(default, deserialize_with = "de_lenient")]
    country_prices: Option<RawCountry>,
}

#[derive(Deserialize)]
struct RawProduct {
    #[serde(default, deserialize_with = "de_loose_id")]
    sku_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    summary: Option<String>,
    #[serde(default, rename = "type", deserialize_with = "de_lenient")]
    kind: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    items: Option<Vec<Item<RawItem>>>,
    /// Par type d'achat ; la clé `0` est le prix de base.
    #[serde(default, deserialize_with = "de_lenient")]
    prices: Option<std::collections::HashMap<String, Item<RawPrices>>>,
}

#[derive(Deserialize)]
struct RawCategory {
    #[serde(default, deserialize_with = "de_loose_id")]
    sku_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    summary: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    products: Option<Vec<Item<RawProduct>>>,
}

fn text(s: Option<&str>) -> String {
    s.map(str::trim).unwrap_or_default().chars().filter(|c| !c.is_control()).take(MAX_TEXT).collect()
}

/// Adresse d'image : `https://…` (ou le faux serveur local des tests),
/// bornée et sans espace ; l'hôte est vérifié au téléchargement.
fn image_url(s: Option<&str>) -> Option<String> {
    let s = s?;
    let ok = s.len() <= 512 && s.is_ascii() && (s.starts_with("https://") || s.starts_with("http://127.0.0.1")) && !s.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| s.to_string())
}

fn product(p: &RawProduct) -> Option<Product> {
    let name = text(p.name.as_deref());
    if name.is_empty() {
        return None;
    }
    let image = p.items.iter().flatten().filter_map(|i| i.0.as_ref()).find_map(|i| image_url(i.thumbnail.as_deref()).or_else(|| image_url(i.assets.as_ref().and_then(|a| a.static_image_url.as_deref()))));
    let unit = |u: &RawUnit| {
        let currency = u.currency.as_deref().filter(|c| (3..=16).contains(&c.len()) && c.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))?;
        Some(Price { amount: u.amount?, exponent: u.exponent.unwrap_or(0).min(6), currency: currency.to_string() })
    };
    let base = p.prices.as_ref().and_then(|m| m.get("0")).and_then(|i| i.0.as_ref()).and_then(|p| p.country_prices.as_ref());
    let prices = base.iter().flat_map(|c| c.prices.iter().flatten()).filter_map(|u| u.0.as_ref().and_then(unit)).take(2).collect();
    Some(Product { sku_id: p.sku_id?, name, summary: text(p.summary.as_deref()), kind: ProductKind::from_code(p.kind.unwrap_or(u64::MAX)), image, prices })
}

/// Réponse de « Get Collectibles Categories ». `None` : pas une liste.
/// Les catégories sans nom ou sans produit lisible sont écartées.
pub fn decode_shop(body: &str) -> Option<Vec<Category>> {
    let list: Vec<Item<RawCategory>> = serde_json::from_str(body).ok()?;
    let cats = list.iter().filter_map(|c| c.0.as_ref()).filter_map(|c| {
        let products: Vec<Product> = c.products.iter().flatten().filter_map(|p| p.0.as_ref().and_then(product)).take(MAX_PRODUCTS).collect();
        let name = text(c.name.as_deref());
        (!name.is_empty() && !products.is_empty()).then_some(Category { sku_id: c.sku_id?, name, summary: text(c.summary.as_deref()), products })
    });
    Some(cats.take(MAX_CATEGORIES).collect())
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: ShopCmd) {
    let ShopCmd::Load = cmd;
    let done = |r| Event::Shop(ShopEvent::Loaded(r));
    if ctx.is_bot() {
        ctx.out.send(done(Err(Problem::Forbidden)));
        return;
    }
    let url = format!("{}/collectibles-categories", ctx.api_root);
    let route = Route { template: "GET /collectibles-categories", major: 0 };
    let ev = match api(&ctx, route, Method::GET, &url, None, MAX_WAIT, std::future::pending()).await.map(|b| decode_shop(&b)) {
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
    fn categories_fixture_keeps_readable_products_only() {
        let cats = decode_shop(include_str!("../../tests/fixtures/nd/rest_collectibles_categories.json")).unwrap_or_else(|| panic!("liste attendue"));
        assert_eq!(cats.iter().map(|c| (c.name.as_str(), c.products.len())).collect::<Vec<_>>(), vec![("Collection fictive", 3), ("Orbes fictives", 1)]);
        let p = &cats[0].products;
        assert_eq!(p.iter().map(|p| p.kind).collect::<Vec<_>>(), vec![ProductKind::Decoration, ProductKind::Effect, ProductKind::Nameplate]);
        assert_eq!(p[0].image.as_deref(), Some("https://cdn.discordapp.com/avatar-decoration-presets/a_dec0000000000000000000000000d1.png"));
        assert_eq!(p[1].image.as_deref(), Some("https://cdn.discordapp.com/assets/profile_effects/effects/fictif/pluie/apercu.png"));
        assert_eq!(p[2].image, None, "adresse en clair (http) refusée");
        let price = |amount, exponent, currency: &str| Price { amount, exponent, currency: currency.into() };
        assert_eq!(p[0].prices, vec![price(499, 2, "eur"), price(1400, 0, "discord_orb")], "prix de base, monnaie illisible écartée");
        assert!(p[1].prices.is_empty() && (p[0].prices[0].value() - 4.99).abs() < 1e-9 && p[0].prices[1].orbs());
        assert_eq!(decode_shop(r#"{ "categories": [] }"#), None);
        assert_eq!(decode_shop("[]"), Some(Vec::new()));
        assert_eq!(decode_shop(r#"[7, { "sku_id": "x" }, { "sku_id": "1", "name": "Vide", "products": [] }]"#), Some(Vec::new()));
    }
}
