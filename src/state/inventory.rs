//! Inventaire du compte, en lecture (lot ND) : droits possédés (achats,
//! cadeaux reçus, récompenses de quête, abonnements) et cadeaux encore à
//! offrir.
//!
//! Source : docs.discord.food, resources/entitlement, « List User
//! Entitlements » (`GET /users/@me/entitlements`, `with_sku`,
//! `exclude_ended`) et « List User Giftable Entitlements »
//! (`GET /users/@me/entitlements/gifts`), « Entitlement Object ».
//!
//! Lecture seule : rien n'est offert, échangé ni consommé par Ferrocord.
//! Deux requêtes à l'ouverture de l'onglet (ou sur « Actualiser »), une
//! demande à la fois.

use super::event::Cmd;
use super::usersettings::Remote;
use super::{Problem, Store};

pub const MAX_ENTITLEMENTS: usize = 300;

/// « Entitlement Type » de la source, regroupé pour la page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    Purchase,
    Subscription,
    Gift,
    Quest,
    Orbs,
    Other,
}

impl Origin {
    pub fn from_code(code: u64) -> Origin {
        match code {
            1 | 5 | 7 => Origin::Purchase,
            2 | 8 => Origin::Subscription,
            3 | 6 => Origin::Gift,
            10 => Origin::Quest,
            12 => Origin::Orbs,
            _ => Origin::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entitlement {
    pub id: u64,
    pub sku_id: u64,
    /// Nom du produit (`sku.name`) ; vide si Discord ne le joint pas.
    pub name: String,
    pub origin: Origin,
    pub starts_ms: Option<i64>,
    pub ends_ms: Option<i64>,
    pub consumed: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Owned {
    pub mine: Vec<Entitlement>,
    /// Cadeaux achetés, pas encore offerts.
    pub giftable: Vec<Entitlement>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InventoryCmd {
    Load,
}

#[derive(Debug)]
pub enum InventoryEvent {
    Loaded(Result<Owned, Problem>),
}

#[derive(Default)]
pub struct Inventory {
    pub list: Remote<Owned>,
    loading: bool,
}

impl Inventory {
    pub fn clear(&mut self) {
        *self = Inventory::default();
    }

    pub fn loading(&self) -> bool {
        self.loading
    }
}

impl Store {
    /// « Inventaire » ouvert : demandé tant que la liste manque.
    pub fn inventory_open(&mut self) {
        if matches!(self.inventory.list, Remote::Idle | Remote::Failed(_)) {
            self.inventory_refresh();
        }
    }

    pub fn inventory_refresh(&mut self) {
        if self.inventory.loading {
            return;
        }
        self.inventory.loading = true;
        if !matches!(self.inventory.list, Remote::Ready(_)) {
            self.inventory.list = Remote::Loading;
        }
        self.push_command(Cmd::Inventory(InventoryCmd::Load));
    }

    pub(super) fn apply_inventory(&mut self, e: InventoryEvent) {
        let InventoryEvent::Loaded(result) = e;
        self.inventory.loading = false;
        match result {
            Ok(owned) => self.inventory.list = Remote::Ready(owned),
            Err(p) if !matches!(self.inventory.list, Remote::Ready(_)) => self.inventory.list = Remote::Failed(p),
            Err(_) => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;

    #[test]
    fn one_load_at_a_time() {
        let mut s = Store::new();
        s.inventory_open();
        s.inventory_open();
        s.inventory_refresh();
        assert_eq!(s.take_commands(), vec![Cmd::Inventory(InventoryCmd::Load)]);
        s.apply(Event::Inventory(InventoryEvent::Loaded(Ok(Owned::default()))));
        s.inventory_open();
        assert!(s.take_commands().is_empty());
        s.inventory_refresh();
        s.apply(Event::Inventory(InventoryEvent::Loaded(Err(Problem::Forbidden))));
        assert_eq!(s.inventory.list, Remote::Ready(Owned::default()), "un échec garde la liste");
        assert_eq!((Origin::from_code(6), Origin::from_code(10), Origin::from_code(99)), (Origin::Gift, Origin::Quest, Origin::Other));
    }
}
