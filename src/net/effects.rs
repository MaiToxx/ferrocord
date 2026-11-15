//! Effets de profil (lot ND, chaîne corrigée par le lot profil) :
//! configuration d'un effet et effets possédés.
//!
//! Points d'API réservés aux comptes utilisateur, non documentés par
//! Discord. Formes vérifiées le 15 novembre 2026 sur docs.discord.food, page
//! « Collectibles » :
//! - « Get Collectibles Product » : `GET /collectibles-products/{sku.id}` ;
//!   réponse : un produit (`sku_id`, `type`, `name`, `items`, `variants?`,
//!   `bundled_products?`…) ;
//! - « Get Collectibles Categories » : `GET /collectibles-categories` ;
//!   réponse : liste de catégories, chacune avec ses `products` ;
//! - « List User Purchased Collectibles » : `GET
//!   /users/@me/collectibles-purchases` ; réponse : liste de produits ;
//! - type de produit 1 : `PROFILE_EFFECT` ; « Profile Effect Collectible
//!   Item Structure » : `type`, `sku_id`, `title`, `description`,
//!   `accessibilityLabel`, `animationType`, `thumbnailPreviewSrc`,
//!   `reducedMotionSrc`, `staticFrameSrc?`, `effects` ; « Profile Effect
//!   Animation Structure » : `src`, `loop`, `height`, `width`, `duration`,
//!   `start`, `loopDelay`, `position { x, y }`, `zIndex`,
//!   `randomizedSources?`.
//!
//! Page « Users », « Profile Effect Structure » : `profile_effect { id,
//! expires_at }` dans les métadonnées d'un profil. **Cet `id` n'est pas le
//! SKU du produit** : la liste publique des effets (paquet npm
//! `@discord-user-card/profile-effects`, relevée dans le client) donne pour
//! chaque effet `id` et `sku_id`, différents (« Boost Relic » : `id`
//! 1139323097930027068, celui de l'exemple de docs.discord.food ; `sku_id`
//! 1139323098370424932). Le lot ND demandait `GET
//! /collectibles-products/{id}` : 404, et rien n'était dessiné. La chaîne
//! suivie est décrite dans `state/effects.rs` : SKU du profil s'il le donne,
//! sinon le catalogue (qui relie `id` et `sku_id`), puis le produit.
//! L'ancienne liste `GET /user-profile-effects` n'est plus décrite par la
//! source : elle n'est pas appelée.
//!
//! Chaque requête suit un geste (profil ouvert, édition du profil ouverte),
//! une fois par effet (ou par session pour le catalogue), et passe par `api`
//! (limiteur commun). Un compte bot n'appelle rien. Jeux d'essai
//! synthétiques : `tests/fixtures/nd/rest_collectibles_*.json`,
//! `tests/fixtures/profil/*.json`.

use super::ratelimit::Route;
use super::raw::{de_lenient, de_loose_id, Item};
use super::rest::{api, ApiError, Ctx};
use crate::state::effects::{CatalogEntry, EffectCmd, EffectConfig, EffectEvent, Layer, OwnedLook, Wearable, CANVAS_HEIGHT, CANVAS_WIDTH, MAX_INDEX, MAX_LAYERS, MAX_OWNED, MAX_TEXT};
use crate::state::{Event, Problem};
use reqwest::Method;
use serde::Deserialize;
use std::{sync::Arc, time::Duration};

const MAX_WAIT: Duration = Duration::from_secs(10);
/// Type de produit « effet de profil » (docs.discord.food, Collectible Product Type).
const PROFILE_EFFECT: u64 = 1;
/// Durée, début ou pause acceptés au plus (ms) : dix minutes.
const MAX_MS: u64 = 600_000;

#[derive(Deserialize)]
struct RawPosition {
    #[serde(default, deserialize_with = "de_lenient")]
    x: Option<i64>,
    #[serde(default, deserialize_with = "de_lenient")]
    y: Option<i64>,
}

#[derive(Deserialize)]
struct RawLayer {
    #[serde(default, deserialize_with = "de_lenient")]
    src: Option<String>,
    #[serde(rename = "loop", default, deserialize_with = "de_lenient")]
    looped: Option<bool>,
    #[serde(default, deserialize_with = "de_lenient")]
    width: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    height: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    duration: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    start: Option<u64>,
    #[serde(rename = "loopDelay", default, deserialize_with = "de_lenient")]
    loop_delay: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    position: Option<RawPosition>,
    #[serde(rename = "zIndex", default, deserialize_with = "de_lenient")]
    z: Option<i64>,
}

#[derive(Deserialize)]
struct RawItem {
    #[serde(rename = "type", default, deserialize_with = "de_lenient")]
    kind: Option<u64>,
    /// Identifiant de l'effet (absent de la source, présent dans la liste publique des effets).
    #[serde(default, deserialize_with = "de_loose_id")]
    id: Option<u64>,
    #[serde(default, deserialize_with = "de_loose_id")]
    sku_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    title: Option<String>,
    #[serde(rename = "accessibilityLabel", default, deserialize_with = "de_lenient")]
    label: Option<String>,
    #[serde(rename = "thumbnailPreviewSrc", default, deserialize_with = "de_lenient")]
    thumbnail: Option<String>,
    #[serde(rename = "reducedMotionSrc", default, deserialize_with = "de_lenient")]
    reduced: Option<String>,
    #[serde(rename = "staticFrameSrc", default, deserialize_with = "de_lenient")]
    still: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    effects: Option<Vec<Item<RawLayer>>>,
    /// Décoration ou plaque : son fichier, sa palette, sa description.
    #[serde(default, deserialize_with = "de_lenient")]
    asset: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    palette: Option<String>,
    #[serde(rename = "label", default, deserialize_with = "de_lenient")]
    plain_label: Option<String>,
}

#[derive(Deserialize)]
struct RawProduct {
    #[serde(default, deserialize_with = "de_loose_id")]
    sku_id: Option<u64>,
    #[serde(default, deserialize_with = "de_lenient")]
    name: Option<String>,
    #[serde(default, deserialize_with = "de_lenient")]
    items: Option<Vec<Item<RawItem>>>,
    /// Groupe de variantes (type 2000) : `items` est vide, les variantes portent les objets.
    #[serde(default, deserialize_with = "de_lenient")]
    variants: Option<Vec<Item<RawProduct>>>,
    /// Lot (type 1000) : les produits qu'il contient.
    #[serde(default, deserialize_with = "de_lenient")]
    bundled_products: Option<Vec<Item<RawProduct>>>,
}

#[derive(Deserialize)]
struct RawCategory {
    #[serde(default, deserialize_with = "de_lenient")]
    products: Option<Vec<Item<RawProduct>>>,
}

/// « Get Collectibles Categories » rend une liste ; ses variantes (`/v2`,
/// `/collectibles-shop`) la rangent sous `categories`.
#[derive(Deserialize)]
struct RawWrapped {
    #[serde(default, deserialize_with = "de_lenient")]
    categories: Option<Vec<Item<RawCategory>>>,
}

/// Adresse d'image simple : `https://…` (ou le faux serveur local des
/// tests), bornée, sans espace ni caractère de contrôle. L'hôte est vérifié
/// au téléchargement (`CdnHosts::check`).
fn image_url(s: Option<&str>) -> Option<String> {
    let s = s?;
    let ok = s.len() <= MAX_TEXT && s.is_ascii() && (s.starts_with("https://") || s.starts_with("http://127.0.0.1")) && !s.chars().any(|c| c.is_whitespace() || c.is_control());
    ok.then(|| s.to_string())
}

fn text(s: Option<&str>) -> String {
    s.map(str::trim).unwrap_or_default().chars().take(MAX_TEXT).collect()
}

fn layer_of(l: &RawLayer) -> Option<Layer> {
    let ms = |v: Option<u64>| v.unwrap_or(0).min(MAX_MS) as u32;
    let side = |v: Option<u64>, default: u32| v.filter(|v| (1..=4096).contains(v)).map_or(default, |v| v as u32);
    let at = |v: Option<i64>| v.unwrap_or(0).clamp(-4096, 4096) as i32;
    Some(Layer {
        src: image_url(l.src.as_deref())?,
        looped: l.looped.unwrap_or(false),
        width: side(l.width, CANVAS_WIDTH),
        height: side(l.height, CANVAS_HEIGHT),
        duration_ms: ms(l.duration),
        start_ms: ms(l.start),
        loop_delay_ms: ms(l.loop_delay),
        x: at(l.position.as_ref().and_then(|p| p.x)),
        y: at(l.position.as_ref().and_then(|p| p.y)),
        z: at(l.z),
    })
}

/// Un objet « effet de profil » et le produit qui le porte.
struct Found<'a> {
    product: &'a RawProduct,
    item: &'a RawItem,
}

impl Found<'_> {
    /// Identifiant de l'effet : celui de l'objet, sinon son SKU (formes où les deux se confondent).
    fn id(&self) -> Option<u64> {
        self.item.id.or(self.sku())
    }

    fn sku(&self) -> Option<u64> {
        self.item.sku_id.or(self.product.sku_id)
    }

    /// Configuration lisible (au moins une couche), sous l'identifiant `id`.
    fn config(&self, id: u64) -> Option<EffectConfig> {
        let mut layers: Vec<Layer> = self.item.effects.iter().flatten().filter_map(|l| l.0.as_ref().and_then(layer_of)).take(MAX_LAYERS).collect();
        if layers.is_empty() {
            return None;
        }
        layers.sort_by_key(|l| l.z);
        Some(EffectConfig {
            id,
            sku_id: self.sku()?,
            title: text(self.item.title.as_deref().or(self.product.name.as_deref())),
            label: text(self.item.label.as_deref()),
            thumbnail: image_url(self.item.thumbnail.as_deref()),
            still: image_url(self.item.still.as_deref()).or_else(|| image_url(self.item.reduced.as_deref())),
            layers,
        })
    }
}

impl RawProduct {
    /// Objets « effet de profil » de ce produit, de ses variantes et des
    /// produits de son lot (un niveau), dans l'ordre reçu.
    fn effects<'a>(&'a self, out: &mut Vec<Found<'a>>, deep: bool) {
        for item in self.items.iter().flatten().filter_map(|i| i.0.as_ref()).filter(|i| i.kind == Some(PROFILE_EFFECT)) {
            if out.len() < MAX_INDEX {
                out.push(Found { product: self, item });
            }
        }
        if deep {
            for p in self.variants.iter().chain(&self.bundled_products).flatten().filter_map(|p| p.0.as_ref()) {
                p.effects(out, false);
            }
        }
    }

    fn found(&self) -> Vec<Found<'_>> {
        let mut out = Vec::new();
        self.effects(&mut out, true);
        out
    }

    /// Le premier effet de profil lisible de ce produit (achats : un produit, un effet).
    fn effect(&self) -> Option<EffectConfig> {
        self.found().iter().find_map(|f| f.config(f.id()?))
    }

    /// L'effet `id` dans ce produit, demandé par le SKU `sku` : l'objet de
    /// cet identifiant ; sinon, s'il ne dit pas le sien, celui de ce SKU.
    fn effect_for(&self, id: u64, sku: u64) -> Option<EffectConfig> {
        let found = self.found();
        let by_id = found.iter().find(|f| f.item.id == Some(id));
        let by_sku = || found.iter().find(|f| f.item.id.is_none() && [Some(id), Some(sku)].contains(&f.sku()));
        by_id.or_else(by_sku)?.config(id)
    }
}

/// Réponse de « Get Collectibles Product » pour le SKU `sku` : l'effet `id`,
/// ou `None` si le produit ne le porte pas ou si la réponse est illisible.
pub fn decode_effect_product(body: &str, id: u64, sku: u64) -> Option<EffectConfig> {
    serde_json::from_str::<RawProduct>(body).ok()?.effect_for(id, sku)
}

/// Réponse de « Get Collectibles Categories » : chaque effet de profil du
/// catalogue (identifiant, SKU, couches s'il les porte). `None` : illisible.
pub fn decode_effect_catalog(body: &str) -> Option<Vec<CatalogEntry>> {
    let cats = match serde_json::from_str::<Vec<Item<RawCategory>>>(body) {
        Ok(list) => list,
        Err(_) => serde_json::from_str::<RawWrapped>(body).ok()?.categories?,
    };
    let mut out: Vec<CatalogEntry> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for p in cats.iter().filter_map(|c| c.0.as_ref()).flat_map(|c| c.products.iter().flatten()).filter_map(|p| p.0.as_ref()) {
        for f in p.found() {
            let (Some(id), Some(sku_id)) = (f.id(), f.sku()) else { continue };
            if out.len() < MAX_INDEX && seen.insert(id) {
                out.push(CatalogEntry { id, sku_id, config: f.config(id) });
            }
        }
    }
    Some(out)
}

/// Réponse de « List User Purchased Collectibles » : les effets de profil
/// possédés (les autres produits sont ignorés). `None` : pas une liste.
pub fn decode_owned_effects(body: &str) -> Option<Vec<EffectConfig>> {
    let list: Vec<Item<RawProduct>> = serde_json::from_str(body).ok()?;
    let mut out: Vec<EffectConfig> = Vec::new();
    for c in list.iter().filter_map(|p| p.0.as_ref().and_then(RawProduct::effect)) {
        if out.len() < MAX_OWNED && !out.iter().any(|x| x.id == c.id) {
            out.push(c);
        }
    }
    Some(out)
}

/// Même réponse : les décorations d'avatar (type 0) et les plaques (type 2)
/// possédées, telles que l'élément du produit les décrit (`asset`, `label`,
/// `palette`). Illisible : liste vide.
pub fn decode_owned_looks(body: &str) -> Vec<OwnedLook> {
    use crate::state::cosmetics::{Asset, Nameplate, Palette};
    let list: Vec<Item<RawProduct>> = serde_json::from_str(body).unwrap_or_default();
    let mut out: Vec<OwnedLook> = Vec::new();
    for p in list.iter().filter_map(|p| p.0.as_ref()) {
        for i in p.items.iter().flatten().filter_map(|i| i.0.as_ref()) {
            let Some(sku_id) = i.sku_id.or(p.sku_id) else { continue };
            let asset = i.asset.as_deref();
            let look = match i.kind {
                Some(0) => asset.and_then(Asset::decoration).map(Wearable::Decoration),
                Some(2) => asset.and_then(Asset::path).map(|asset| Wearable::Nameplate(Nameplate { asset, palette: i.palette.as_deref().and_then(Palette::parse) })),
                _ => None,
            };
            let Some(look) = look else { continue };
            if out.len() < MAX_OWNED && !out.iter().any(|x| x.sku_id == sku_id) {
                out.push(OwnedLook { sku_id, label: text(i.plain_label.as_deref().or(p.name.as_deref())), look });
            }
        }
    }
    out
}

const fn route(template: &'static str) -> Route {
    Route { template, major: 0 }
}

pub(super) async fn run(ctx: Arc<Ctx>, cmd: EffectCmd) {
    let fx = Event::Effects;
    let ev = match cmd {
        EffectCmd::Config { id, .. } if ctx.is_bot() => fx(EffectEvent::Config { id, result: Ok(None) }),
        EffectCmd::Catalog if ctx.is_bot() => fx(EffectEvent::Catalog(Ok(Vec::new()))),
        EffectCmd::Owned if ctx.is_bot() => fx(EffectEvent::Owned(Err(Problem::Forbidden))),
        EffectCmd::Config { id, sku } => {
            let url = format!("{}/collectibles-products/{sku}", ctx.api_root);
            match api(&ctx, route("GET /collectibles-products/{}"), Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
                Ok(b) => fx(EffectEvent::Config { id, result: Ok(decode_effect_product(&b, id, sku)) }),
                // Produit retiré, inconnu ou refusé (captcha compris : on s'arrête là) : pas dans ce produit.
                Err(ApiError::NotFound(_) | ApiError::Forbidden | ApiError::Captcha) => fx(EffectEvent::Config { id, result: Ok(None) }),
                Err(e) => e.into_event(|p| fx(EffectEvent::Config { id, result: Err(p) })),
            }
        }
        EffectCmd::Catalog => {
            let url = format!("{}/collectibles-categories", ctx.api_root);
            match api(&ctx, route("GET /collectibles-categories"), Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
                // Illisible, refusé ou captcha : catalogue vide, les effets en attente sont tenus pour absents.
                Ok(b) => fx(EffectEvent::Catalog(Ok(decode_effect_catalog(&b).unwrap_or_default()))),
                Err(ApiError::NotFound(_) | ApiError::Forbidden | ApiError::Captcha) => fx(EffectEvent::Catalog(Ok(Vec::new()))),
                Err(e) => e.into_event(|p| fx(EffectEvent::Catalog(Err(p)))),
            }
        }
        EffectCmd::Owned => {
            let url = format!("{}/users/@me/collectibles-purchases", ctx.api_root);
            match api(&ctx, route("GET /users/@me/collectibles-purchases"), Method::GET, &url, None, MAX_WAIT, std::future::pending()).await {
                Ok(b) => {
                    // La même réponse porte les décorations et les plaques possédées.
                    ctx.out.send(fx(EffectEvent::OwnedLooks(decode_owned_looks(&b))));
                    match decode_owned_effects(&b) {
                        Some(list) => fx(EffectEvent::Owned(Ok(list))),
                        None => fx(EffectEvent::Owned(Err(Problem::Api(String::new())))),
                    }
                }
                Err(e) => e.into_event(|p| fx(EffectEvent::Owned(Err(p)))),
            }
        }
    };
    ctx.out.send(ev);
}

#[cfg(test)]
mod tests {
    use super::*;

    const RAIN: u64 = 6_000_000_000_000_000_001;
    /// Effet dont l'identifiant diffère du SKU (cas réel : tous les effets de la liste publique).
    const AURORA: u64 = 6_000_000_000_000_000_301;
    const AURORA_SKU: u64 = 6_000_000_000_000_000_302;

    #[test]
    fn product_fixture_gives_layers_in_stacking_order() {
        let c = decode_effect_product(include_str!("../../tests/fixtures/nd/rest_collectibles_product.json"), RAIN, RAIN).unwrap_or_else(|| panic!("effet attendu"));
        assert_eq!((c.id, c.sku_id, c.title.as_str(), c.label.as_str()), (RAIN, RAIN, "Pluie fictive", "Des gouttes synthétiques tombent sur le profil"));
        assert_eq!(c.layers.len(), 2, "couche sans adresse et couche hors https écartées");
        let (idle, intro) = (&c.layers[0], &c.layers[1]);
        assert_eq!((idle.z, idle.looped, idle.duration_ms, idle.start_ms, idle.loop_delay_ms), (1, true, 1200, 800, 300));
        assert_eq!((intro.z, intro.looped, intro.width, intro.height, intro.x, intro.y), (2, false, 450, 880, 0, 0));
        assert!(c.still.as_deref().is_some_and(|u| u.ends_with("/statique.png")));
        assert!(c.thumbnail.is_some());
    }

    /// Identifiant d'effet différent du SKU : trouvé par `id` dans le produit
    /// du SKU ; le même produit demandé pour un autre effet ne rend rien.
    #[test]
    fn effect_is_found_by_its_id_in_the_product_of_its_sku() {
        let body = include_str!("../../tests/fixtures/profil/rest_collectibles_product_effect.json");
        let c = decode_effect_product(body, AURORA, AURORA_SKU).unwrap_or_else(|| panic!("effet attendu"));
        assert_eq!((c.id, c.sku_id, c.title.as_str(), c.layers.len()), (AURORA, AURORA_SKU, "Aurore fictive", 2));
        assert_eq!(decode_effect_product(body, 42, AURORA_SKU), None, "autre effet : pas celui-ci");
        // Objet sans `id` (forme de docs.discord.food) : celui du SKU demandé, rangé sous l'identifiant voulu.
        let bare = r#"{ "sku_id": "70", "type": 1, "items": [{ "type": 1, "sku_id": "70", "effects": [{ "src": "https://cdn.discordapp.com/x.png" }] }] }"#;
        assert_eq!(decode_effect_product(bare, 7, 70).map(|c| (c.id, c.sku_id)), Some((7, 70)));
        assert_eq!(decode_effect_product(bare, 70, 70).map(|c| (c.id, c.sku_id)), Some((70, 70)));
        assert_eq!(decode_effect_product(bare, 7, 71), None);
    }

    /// Groupe de variantes et lot : l'effet est cherché dans les produits contenus.
    #[test]
    fn variants_and_bundles_are_searched() {
        let layer = r#"[{ "src": "https://cdn.discordapp.com/x.png", "loop": true }]"#;
        let group = format!(r#"{{ "sku_id": "90", "type": 2000, "items": [], "variants": [
            {{ "sku_id": "91", "type": 1, "name": "Rouge", "items": [{{ "type": 1, "id": "11", "sku_id": "91", "effects": {layer} }}] }},
            {{ "sku_id": "92", "type": 1, "name": "Bleu", "items": [{{ "type": 1, "id": "12", "sku_id": "92", "effects": {layer} }}] }} ] }}"#);
        assert_eq!(decode_effect_product(&group, 12, 90).map(|c| (c.id, c.sku_id, c.title)), Some((12, 92, "Bleu".into())));
        assert_eq!(decode_effect_product(&group, 13, 90), None);
        let bundle = format!(r#"{{ "sku_id": "80", "type": 1000, "items": [], "bundled_products": [
            {{ "sku_id": "81", "type": 0, "items": [{{ "type": 0, "asset": "a_x" }}] }},
            {{ "sku_id": "82", "type": 1, "items": [{{ "type": 1, "id": "21", "sku_id": "82", "effects": {layer} }}] }} ] }}"#);
        assert_eq!(decode_effect_product(&bundle, 21, 80).map(|c| c.sku_id), Some(82));
    }

    #[test]
    fn other_products_and_odd_shapes_give_no_effect() {
        assert_eq!(decode_effect_product(r#"{ "sku_id": "5", "type": 0, "items": [{ "type": 0, "asset": "a_x" }] }"#, 5, 5), None, "décoration d'avatar");
        assert_eq!(decode_effect_product(r#"{ "sku_id": "5", "type": 1, "items": [{ "type": 1, "sku_id": "5", "effects": [] }] }"#, 5, 5), None, "aucune couche");
        assert_eq!(decode_effect_product(r#"{ "sku_id": "5", "type": 1, "items": "x" }"#, 5, 5), None);
        assert_eq!(decode_effect_product("pas du json", 5, 5), None);
        // Valeurs hors bornes ramenées ; adresse piégée refusée.
        let odd = r#"{ "sku_id": "5", "type": 1, "items": [{ "type": 1, "sku_id": 5, "title": 7, "effects": [
            { "src": "https://cdn.discordapp.com/a b.png" },
            { "src": "javascript:alert(1)" },
            { "src": "https://cdn.discordapp.com/x.png", "loop": "oui", "width": 0, "height": 99999, "duration": 99999999999, "start": -4, "position": { "x": "g", "y": -99999 }, "zIndex": null } ] }] }"#;
        let c = decode_effect_product(odd, 5, 5).unwrap_or_else(|| panic!("effet attendu"));
        assert_eq!(c.layers.len(), 1);
        let l = &c.layers[0];
        assert_eq!((l.looped, l.width, l.height, l.duration_ms, l.start_ms, l.x, l.y), (false, CANVAS_WIDTH, CANVAS_HEIGHT, MAX_MS as u32, 0, 0, -4096));
        assert_eq!(c.title, "");
    }

    /// Catalogue : chaque effet avec son identifiant et son SKU ; couches si présentes.
    #[test]
    fn catalog_links_effect_ids_to_skus() {
        let list = decode_effect_catalog(include_str!("../../tests/fixtures/profil/rest_collectibles_categories_effects.json")).unwrap_or_default();
        let brief: Vec<(u64, u64, bool)> = list.iter().map(|e| (e.id, e.sku_id, e.config.is_some())).collect();
        assert_eq!(brief, [(AURORA, AURORA_SKU, false), (6_000_000_000_000_000_311, 6_000_000_000_000_000_312, true), (RAIN, RAIN, false)], "décoration, doublon et entrée illisible écartés");
        // Le catalogue de la boutique (lot ND) se lit de même ; forme rangée sous `categories` aussi.
        let shop = decode_effect_catalog(include_str!("../../tests/fixtures/nd/rest_collectibles_categories.json")).unwrap_or_default();
        assert!(shop.iter().any(|e| e.id == RAIN && e.sku_id == RAIN));
        let wrapped = r#"{ "categories": [{ "products": [{ "sku_id": "70", "type": 1, "items": [{ "type": 1, "id": "7", "sku_id": "70" }] }] }], "shop_blocks": [] }"#;
        assert_eq!(decode_effect_catalog(wrapped), Some(vec![CatalogEntry { id: 7, sku_id: 70, config: None }]));
        assert_eq!(decode_effect_catalog("[]"), Some(Vec::new()));
        assert!(decode_effect_catalog("7").is_none() && decode_effect_catalog(r#"{ "x": 1 }"#).is_none());
    }

    #[test]
    fn purchases_fixture_keeps_only_profile_effects() {
        let list = decode_owned_effects(include_str!("../../tests/fixtures/nd/rest_collectibles_purchases.json")).unwrap_or_default();
        assert_eq!(list.iter().map(|c| c.id).collect::<Vec<_>>(), [6_000_000_000_000_000_001, 6_000_000_000_000_000_002], "décoration, plaque, doublon et entrée illisible écartés");
        assert!(decode_owned_effects("{}").is_none());
        assert_eq!(decode_owned_effects("[]"), Some(Vec::new()));
    }
}
