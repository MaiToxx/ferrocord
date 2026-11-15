//! Boutique en consultation (lot ND) : catégories et produits de la
//! boutique de Discord, à regarder seulement.
//!
//! Source : docs.discord.food, resources/collectibles, « Get Collectibles
//! Categories » (`GET /collectibles-categories`), « Collectible Category
//! Object », « Collectible Product Object ».
//!
//! **Aucun achat** : ni panier, ni paiement, ni dépense d'Orbes. « Voir dans
//! Discord » ouvre la boutique dans le navigateur. Le prix de base est
//! affiché à titre indicatif (resources/store, « Subscription Prices Structure »).
//! Une requête à l'ouverture de la page (ou sur « Actualiser »), une seule
//! à la fois ; la liste reçue sert pour la session.

use super::event::Cmd;
use super::usersettings::Remote;
use super::{Problem, Store};

/// Page de la boutique sur le site de Discord (aucun achat ici).
pub const SHOP_URL: &str = "https://discord.com/shop";
pub const MAX_CATEGORIES: usize = 40;
pub const MAX_PRODUCTS: usize = 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProductKind {
    Decoration,
    Effect,
    Nameplate,
    Frame,
    Bundle,
    Other,
}

impl ProductKind {
    /// « Collectible Product Type » de la source.
    pub fn from_code(code: u64) -> ProductKind {
        match code {
            0 => ProductKind::Decoration,
            1 => ProductKind::Effect,
            2 => ProductKind::Nameplate,
            3 => ProductKind::Frame,
            1000 => ProductKind::Bundle,
            _ => ProductKind::Other,
        }
    }
}

/// Prix unitaire (« Unit Price » de la source) : `amount` dans la plus petite
/// unité, `exponent` décimales ; `currency` en minuscules (`discord_orb` : Orbes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Price {
    pub amount: u64,
    pub exponent: u8,
    pub currency: String,
}

impl Price {
    pub fn orbs(&self) -> bool {
        self.currency == "discord_orb"
    }

    /// Montant en unités affichées (12,99 pour 1299 à deux décimales).
    pub fn value(&self) -> f64 {
        self.amount as f64 / 10f64.powi(i32::from(self.exponent))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Product {
    pub sku_id: u64,
    pub name: String,
    pub summary: String,
    pub kind: ProductKind,
    /// Prix de base (clé `0` de `prices`), au plus deux monnaies (argent, Orbes).
    pub prices: Vec<Price>,
    /// Image fixe d'aperçu (adresse complète du CDN), si le produit en donne une.
    pub image: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub sku_id: u64,
    pub name: String,
    pub summary: String,
    pub products: Vec<Product>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShopCmd {
    Load,
}

#[derive(Debug)]
pub enum ShopEvent {
    Loaded(Result<Vec<Category>, Problem>),
}

#[derive(Default)]
pub struct Shop {
    pub list: Remote<Vec<Category>>,
    loading: bool,
}

impl Shop {
    pub fn clear(&mut self) {
        *self = Shop::default();
    }

    pub fn loading(&self) -> bool {
        self.loading
    }
}

impl Store {
    /// La page de la boutique s'ouvre : les catégories sont demandées si
    /// elles ne sont pas connues.
    pub fn shop_open(&mut self) {
        if matches!(self.shop.list, Remote::Idle | Remote::Failed(_)) {
            self.shop_refresh();
        }
    }

    /// « Actualiser » : une demande à la fois ; la liste affichée reste en place.
    pub fn shop_refresh(&mut self) {
        if self.shop.loading {
            return;
        }
        self.shop.loading = true;
        if !matches!(self.shop.list, Remote::Ready(_)) {
            self.shop.list = Remote::Loading;
        }
        self.push_command(Cmd::Shop(ShopCmd::Load));
    }

    pub(super) fn apply_shop(&mut self, e: ShopEvent) {
        let ShopEvent::Loaded(result) = e;
        self.shop.loading = false;
        match result {
            Ok(list) => self.shop.list = Remote::Ready(list),
            // Un échec ne retire pas une liste déjà affichée.
            Err(p) if !matches!(self.shop.list, Remote::Ready(_)) => self.shop.list = Remote::Failed(p),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    #[test]
    fn one_load_at_a_time_and_a_failure_keeps_the_list() {
        let mut s = Store::new();
        s.shop_open();
        s.shop_open();
        s.shop_refresh();
        assert_eq!(s.take_commands(), vec![Cmd::Shop(ShopCmd::Load)]);
        s.apply(Event::Shop(ShopEvent::Loaded(Err(Problem::Forbidden))));
        assert_eq!(s.shop.list, Remote::Failed(Problem::Forbidden));
        s.shop_open();
        assert_eq!(s.take_commands(), vec![Cmd::Shop(ShopCmd::Load)], "après un échec, la page redemande");
        let cat = Category { sku_id: 1, name: "Essai".into(), summary: String::new(), products: Vec::new() };
        s.apply(Event::Shop(ShopEvent::Loaded(Ok(vec![cat.clone()]))));
        s.shop_open();
        assert!(s.take_commands().is_empty(), "liste connue : rien n'est redemandé");
        s.shop_refresh();
        assert!(s.shop.loading());
        s.apply(Event::Shop(ShopEvent::Loaded(Err(Problem::Forbidden))));
        assert_eq!(s.shop.list, Remote::Ready(vec![cat]));
    }
}
