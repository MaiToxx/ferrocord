//! Effets de profil (lot ND) : la configuration d'un effet (ses couches
//! animées) et les effets que possède le compte.
//!
//! Requêtes non documentées par Discord ; formes vérifiées sur
//! docs.discord.food, page « Collectibles » (« Get Collectibles Product »,
//! « Get Collectibles Categories », « List User Purchased Collectibles »,
//! « Profile Effect Collectible Item Structure », « Profile Effect Animation
//! Structure ») et page « Users » (« Profile Effect Structure » :
//! `profile_effect.id`). Détail dans `net/effects.rs` et
//! `docs/passerelle-utilisateur.md`.
//!
//! **L'identifiant d'un effet n'est pas celui de son produit** (lot profil) :
//! `profile_effect.id` désigne l'effet, `GET /collectibles-products/{sku}`
//! attend le SKU du produit qui le vend, et les deux diffèrent (liste
//! publique `@discord-user-card/profile-effects` : chaque entrée porte `id`
//! et `sku_id`, différents). La chaîne suivie, dans l'ordre :
//! 1. le produit du SKU donné par le profil (`profile_effect.sku_id`) s'il y
//!    en a un, sinon celui dont le SKU serait l'identifiant lui-même ;
//! 2. sinon le catalogue (`GET /collectibles-categories`, une fois par
//!    session) : il donne, pour chaque effet, son identifiant, son SKU et,
//!    quand Discord les y met, ses couches ;
//! 3. sinon le produit du SKU trouvé dans le catalogue.
//!
//! Si rien n'aboutit, l'effet est tenu pour absent : la carte reste normale.
//!
//! Rien ne part sans un geste : la configuration d'un effet est demandée
//! quand un profil qui le porte est ouvert (une fois par effet et par
//! session), la liste des effets possédés à l'ouverture de l'édition du profil.

use super::event::Cmd;
use super::notice::Problem;
use super::store::Store;
use super::usersettings::Remote;
use std::collections::HashMap;
use std::sync::Arc;

/// Largeur du canevas d'un effet (les couches de Discord font 450 de large).
pub const CANVAS_WIDTH: u32 = 450;
/// Hauteur du canevas (880 pour toutes les couches connues).
pub const CANVAS_HEIGHT: u32 = 880;
/// Couches lues au plus par effet.
pub const MAX_LAYERS: usize = 4;
/// Configurations gardées au plus (les plus anciennes sont oubliées).
pub const MAX_CONFIGS: usize = 64;
/// Effets possédés gardés au plus.
pub const MAX_OWNED: usize = 200;
/// Longueur maximale d'un texte ou d'une adresse reçus.
pub const MAX_TEXT: usize = 512;
/// Correspondances effet → SKU gardées au plus (catalogue).
pub const MAX_INDEX: usize = 2048;
/// Demandes du catalogue au plus par session (les échecs réseau comptent).
pub const MAX_CATALOG_TRIES: u8 = 3;

/// Une couche animée (APNG) d'un effet, placée sur le canevas.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Layer {
    pub src: String,
    pub looped: bool,
    pub width: u32,
    pub height: u32,
    /// Durée d'un passage (ms) ; 0 : celle du fichier.
    pub duration_ms: u32,
    /// Début, depuis l'ouverture du profil (ms).
    pub start_ms: u32,
    /// Pause entre deux passages d'une couche en boucle (ms).
    pub loop_delay_ms: u32,
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

/// Où en est une couche à un instant donné.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Pas encore commencée.
    Waiting { in_ms: u64 },
    /// Passage numéro `pass` en cours ; `left_ms` : avant sa fin (`None` : jusqu'à la fin du fichier).
    Playing { pass: u64, left_ms: Option<u64> },
    /// Entre deux passages (pause de boucle) : rien n'est montré.
    Paused { in_ms: u64 },
    /// Terminée (couche sans boucle) : rien n'est montré.
    Over,
}

impl Layer {
    /// Phase de la couche `elapsed_ms` après l'ouverture. Une couche sans
    /// boucle joue une fois ; une couche en boucle reprend tous les
    /// `duration + loop_delay`. Sans durée, le fichier donne la sienne.
    pub fn phase(&self, elapsed_ms: u64) -> Phase {
        let start = u64::from(self.start_ms);
        if elapsed_ms < start {
            return Phase::Waiting { in_ms: start - elapsed_ms };
        }
        let t = elapsed_ms - start;
        let d = u64::from(self.duration_ms);
        if d == 0 {
            return Phase::Playing { pass: 0, left_ms: None };
        }
        if !self.looped {
            return if t < d { Phase::Playing { pass: 0, left_ms: Some(d - t) } } else { Phase::Over };
        }
        let cycle = d + u64::from(self.loop_delay_ms);
        let (pass, at) = (t / cycle, t % cycle);
        if at < d { Phase::Playing { pass, left_ms: Some(d - at) } } else { Phase::Paused { in_ms: cycle - at } }
    }
}

/// Configuration d'un effet de profil.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffectConfig {
    /// Identifiant de l'effet (`profile_effect.id`, `profile_effect_id`).
    pub id: u64,
    /// SKU du produit qui le vend (souvent différent de `id`).
    pub sku_id: u64,
    pub title: String,
    /// Description pour les lecteurs d'écran (`accessibilityLabel`).
    pub label: String,
    /// Aperçu (APNG) de la boutique.
    pub thumbnail: Option<String>,
    /// Image fixe, ou version à mouvement réduit dont la première image sert d'image fixe.
    pub still: Option<String>,
    /// Couches, dans l'ordre d'empilement (`zIndex` croissant).
    pub layers: Vec<Layer>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EffectCmd {
    /// Configuration de l'effet `id`, cherchée dans le produit `sku`
    /// (`GET /collectibles-products/{sku}`).
    Config { id: u64, sku: u64 },
    /// Catalogue des effets (`GET /collectibles-categories`) : identifiant, SKU, couches.
    Catalog,
    /// Effets possédés (`GET /users/@me/collectibles-purchases`).
    Owned,
}

/// Ce qu'un produit possédé fait porter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wearable {
    Decoration(super::cosmetics::Asset),
    Nameplate(super::cosmetics::Nameplate),
}

/// Décoration d'avatar ou plaque que le compte possède.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OwnedLook {
    pub sku_id: u64,
    pub label: String,
    pub look: Wearable,
}

/// Un effet du catalogue : son identifiant, son SKU, ses couches si le catalogue les donne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: u64,
    pub sku_id: u64,
    pub config: Option<EffectConfig>,
}

#[derive(Debug)]
pub enum EffectEvent {
    /// `Ok(None)` : le produit ne porte pas cet effet (ou n'existe pas).
    Config { id: u64, result: Result<Option<EffectConfig>, Problem> },
    Catalog(Result<Vec<CatalogEntry>, Problem>),
    Owned(Result<Vec<EffectConfig>, Problem>),
    /// Décorations et plaques possédées (même réponse que `Owned`).
    OwnedLooks(Vec<OwnedLook>),
}

/// Ce que l'on sait de la configuration d'un effet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Known {
    Loading,
    Ready(Arc<EffectConfig>),
    /// Introuvable ou illisible : rien n'est dessiné, rien n'est redemandé.
    Missing,
}

/// Où en est le catalogue des effets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Catalog {
    #[default]
    Idle,
    Loading,
    Ready,
}

#[derive(Default)]
pub struct Effects {
    configs: HashMap<u64, (Known, u64)>,
    /// Compteur d'usage, pour oublier les configurations les plus anciennes.
    tick: u64,
    /// SKU du dernier produit demandé pour chaque effet en cours de recherche.
    asked: HashMap<u64, u64>,
    catalog: Catalog,
    catalog_tries: u8,
    /// Effet → SKU, d'après le catalogue.
    index: HashMap<u64, u64>,
    /// Effets qui attendent le catalogue.
    waiting: Vec<u64>,
    /// Effets que possède le compte (identifiants, dans l'ordre reçu).
    pub owned: Remote<Vec<u64>>,
    /// Décorations et plaques possédées (choix dans « Modifier le profil »).
    pub looks: Vec<OwnedLook>,
}

impl Effects {
    pub fn clear(&mut self) {
        *self = Effects::default();
    }

    pub fn known(&self, id: u64) -> Option<&Known> {
        self.configs.get(&id).map(|(k, _)| k)
    }

    /// Configuration prête de l'effet `id`.
    pub fn config(&self, id: u64) -> Option<Arc<EffectConfig>> {
        match self.known(id)? {
            Known::Ready(c) => Some(c.clone()),
            _ => None,
        }
    }

    fn set(&mut self, id: u64, known: Known) {
        self.tick += 1;
        self.configs.insert(id, (known, self.tick));
        if self.configs.len() > MAX_CONFIGS
            && let Some(oldest) = self.configs.iter().filter(|(_, (k, _))| *k != Known::Loading).min_by_key(|(_, (_, t))| *t).map(|(id, _)| *id)
        {
            self.configs.remove(&oldest);
        }
    }
}

impl Store {
    /// Un profil ouvert porte l'effet `id` (`sku` : le SKU que le profil
    /// donne, s'il en donne un) : sa configuration est demandée une fois (un
    /// compte bot reçoit « introuvable » sans requête). Rend ce qui est déjà connu.
    pub fn want_effect(&mut self, id: u64, sku: Option<u64>) -> Option<Arc<EffectConfig>> {
        match self.effects.known(id) {
            Some(Known::Ready(c)) => return Some(c.clone()),
            Some(_) => return None,
            None => {}
        }
        if id == 0 {
            return None;
        }
        let sku = sku.filter(|s| *s != 0).or(self.effects.index.get(&id).copied()).unwrap_or(id);
        self.effects.set(id, Known::Loading);
        self.effects.asked.insert(id, sku);
        self.push_command(Cmd::Effects(EffectCmd::Config { id, sku }));
        None
    }

    /// Le produit demandé ne porte pas l'effet `id` : le catalogue dit quel
    /// produit le vend (demandé une fois par session) ; sinon l'effet est absent.
    fn effect_not_in_product(&mut self, id: u64) {
        match self.effects.catalog {
            Catalog::Ready => self.resolve_effect(id),
            Catalog::Loading => self.effects.waiting.push(id),
            Catalog::Idle if self.effects.catalog_tries < MAX_CATALOG_TRIES => {
                self.effects.catalog = Catalog::Loading;
                self.effects.catalog_tries += 1;
                self.effects.waiting.push(id);
                self.push_command(Cmd::Effects(EffectCmd::Catalog));
            }
            Catalog::Idle => self.effect_missing(id),
        }
    }

    fn effect_missing(&mut self, id: u64) {
        self.effects.asked.remove(&id);
        self.effects.set(id, Known::Missing);
    }

    /// Catalogue connu : le produit de l'effet `id` est demandé s'il ne l'a pas déjà été.
    fn resolve_effect(&mut self, id: u64) {
        match self.effects.index.get(&id).copied() {
            Some(sku) if self.effects.asked.get(&id) != Some(&sku) => {
                self.effects.asked.insert(id, sku);
                self.push_command(Cmd::Effects(EffectCmd::Config { id, sku }));
            }
            _ => self.effect_missing(id),
        }
    }

    /// Édition du profil ouverte : les effets possédés sont demandés (une
    /// fois, ou de nouveau après un échec).
    pub fn want_owned_effects(&mut self) {
        if matches!(self.effects.owned, Remote::Idle | Remote::Failed(_)) {
            self.effects.owned = Remote::Loading;
            self.push_command(Cmd::Effects(EffectCmd::Owned));
        }
    }

    pub(super) fn apply_effects(&mut self, e: EffectEvent) {
        match e {
            EffectEvent::Config { id, result: Ok(Some(c)) } => {
                self.effects.asked.remove(&id);
                self.effects.set(id, Known::Ready(Arc::new(c)));
            }
            // Pas dans ce produit : le catalogue, puis « absent », définitif pour la session.
            EffectEvent::Config { id, result: Ok(None) } => self.effect_not_in_product(id),
            // Échec réseau : oublié, redemandé au prochain profil ouvert.
            EffectEvent::Config { id, result: Err(_) } => {
                self.effects.asked.remove(&id);
                self.effects.configs.remove(&id);
            }
            EffectEvent::Catalog(Ok(list)) => {
                self.effects.catalog = Catalog::Ready;
                let waiting = std::mem::take(&mut self.effects.waiting);
                for e in list {
                    if self.effects.index.len() < MAX_INDEX {
                        self.effects.index.insert(e.id, e.sku_id);
                    }
                    // Seules les couches des effets attendus sont gardées.
                    if let Some(c) = e.config.filter(|_| waiting.contains(&e.id)) {
                        self.effects.asked.remove(&e.id);
                        self.effects.set(e.id, Known::Ready(Arc::new(c)));
                    }
                }
                for id in waiting {
                    if self.effects.known(id) == Some(&Known::Loading) {
                        self.resolve_effect(id);
                    }
                }
            }
            // Catalogue en échec : les effets en attente sont oubliés (redemandés au prochain profil ouvert).
            EffectEvent::Catalog(Err(_)) => {
                self.effects.catalog = Catalog::Idle;
                for id in std::mem::take(&mut self.effects.waiting) {
                    self.effects.asked.remove(&id);
                    self.effects.configs.remove(&id);
                }
            }
            EffectEvent::Owned(Ok(list)) => {
                let ids = list.iter().map(|c| c.id).take(MAX_OWNED).collect();
                for c in list.into_iter().take(MAX_CONFIGS / 2) {
                    self.effects.set(c.id, Known::Ready(Arc::new(c)));
                }
                self.effects.owned = Remote::Ready(ids);
            }
            EffectEvent::Owned(Err(p)) => self.effects.owned = Remote::Failed(p),
            EffectEvent::OwnedLooks(list) => self.effects.looks = list,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer(looped: bool, duration: u32, start: u32, delay: u32) -> Layer {
        Layer { src: String::new(), looped, width: 450, height: 880, duration_ms: duration, start_ms: start, loop_delay_ms: delay, x: 0, y: 0, z: 0 }
    }

    /// Une couche sans boucle joue une fois ; en boucle, elle reprend après sa pause.
    #[test]
    fn layer_phases_follow_start_duration_and_loop_delay() {
        let intro = layer(false, 2000, 500, 0);
        assert_eq!(intro.phase(0), Phase::Waiting { in_ms: 500 });
        assert_eq!(intro.phase(500), Phase::Playing { pass: 0, left_ms: Some(2000) });
        assert_eq!(intro.phase(2499), Phase::Playing { pass: 0, left_ms: Some(1) });
        assert_eq!(intro.phase(2500), Phase::Over);
        let idle = layer(true, 1000, 2000, 500);
        assert_eq!(idle.phase(1999), Phase::Waiting { in_ms: 1 });
        assert_eq!(idle.phase(2000), Phase::Playing { pass: 0, left_ms: Some(1000) });
        assert_eq!(idle.phase(3200), Phase::Paused { in_ms: 300 });
        assert_eq!(idle.phase(3500), Phase::Playing { pass: 1, left_ms: Some(1000) });
        assert_eq!(idle.phase(2000 + 1500 * 7 + 10), Phase::Playing { pass: 7, left_ms: Some(990) });
        // Sans durée : celle du fichier.
        assert_eq!(layer(true, 0, 0, 900).phase(123_456), Phase::Playing { pass: 0, left_ms: None });
    }

    fn config(id: u64) -> EffectConfig {
        EffectConfig { id, sku_id: id, title: format!("Effet {id}"), label: String::new(), thumbnail: None, still: None, layers: vec![layer(true, 1000, 0, 0)] }
    }

    fn ready(s: &mut Store) {
        s.apply(crate::state::Event::Ready { me: crate::state::model::User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        s.take_commands();
    }

    fn cmds(c: EffectCmd) -> Vec<Cmd> {
        vec![Cmd::Effects(c)]
    }

    /// Le profil donne le SKU : un seul produit demandé, une seule fois.
    #[test]
    fn sku_from_the_profile_is_asked_once_and_kept() {
        let mut s = Store::new();
        ready(&mut s);
        assert!(s.want_effect(7, Some(70)).is_none());
        assert!(s.want_effect(7, Some(70)).is_none());
        assert_eq!(s.take_commands(), cmds(EffectCmd::Config { id: 7, sku: 70 }));
        s.apply_effects(EffectEvent::Config { id: 7, result: Ok(Some(EffectConfig { sku_id: 70, ..config(7) })) });
        assert_eq!(s.want_effect(7, Some(70)).map(|c| (c.title.clone(), c.sku_id)), Some(("Effet 7".into(), 70)));
        assert!(s.take_commands().is_empty());
        // Échec réseau : redemandé au prochain profil ouvert.
        s.want_effect(9, None);
        s.apply_effects(EffectEvent::Config { id: 9, result: Err(Problem::Http(500)) });
        s.take_commands();
        s.want_effect(9, None);
        assert_eq!(s.take_commands(), cmds(EffectCmd::Config { id: 9, sku: 9 }), "redemandé après un échec");
        assert!(s.want_effect(0, None).is_none() && s.take_commands().is_empty());
    }

    /// Forme sans SKU (`profile_effect { id }`) : le produit de même
    /// identifiant, puis le catalogue (une fois), puis le produit qu'il désigne.
    #[test]
    fn bare_id_goes_through_the_catalog_once() {
        let mut s = Store::new();
        ready(&mut s);
        s.want_effect(7, None);
        assert_eq!(s.take_commands(), cmds(EffectCmd::Config { id: 7, sku: 7 }));
        s.apply_effects(EffectEvent::Config { id: 7, result: Ok(None) });
        assert_eq!(s.take_commands(), cmds(EffectCmd::Catalog));
        // D'autres effets pendant ce temps : ils attendent le même catalogue.
        s.want_effect(8, None);
        s.apply_effects(EffectEvent::Config { id: 8, result: Ok(None) });
        s.want_effect(5, None);
        s.apply_effects(EffectEvent::Config { id: 5, result: Ok(None) });
        assert_eq!(s.take_commands(), vec![Cmd::Effects(EffectCmd::Config { id: 8, sku: 8 }), Cmd::Effects(EffectCmd::Config { id: 5, sku: 5 })]);
        let entries = vec![
            CatalogEntry { id: 7, sku_id: 70, config: None },
            CatalogEntry { id: 8, sku_id: 80, config: Some(EffectConfig { sku_id: 80, ..config(8) }) },
            CatalogEntry { id: 6, sku_id: 60, config: Some(config(6)) },
        ];
        s.apply_effects(EffectEvent::Catalog(Ok(entries)));
        // 8 : couches dans le catalogue ; 7 : son produit ; 5 : inconnu du catalogue, absent.
        assert!(s.effects.config(8).is_some());
        assert_eq!(s.take_commands(), cmds(EffectCmd::Config { id: 7, sku: 70 }));
        assert_eq!(s.effects.known(5), Some(&Known::Missing));
        assert!(s.effects.config(6).is_none(), "couches d'un effet non attendu : pas gardées");
        s.apply_effects(EffectEvent::Config { id: 7, result: Ok(Some(EffectConfig { sku_id: 70, ..config(7) })) });
        assert!(s.want_effect(7, None).is_some());
        // Catalogue connu : un nouvel effet y trouve son SKU sans le redemander.
        s.want_effect(6, None);
        assert_eq!(s.take_commands(), cmds(EffectCmd::Config { id: 6, sku: 60 }));
        // Le produit du catalogue ne le porte pas non plus : absent, plus rien ne part.
        s.apply_effects(EffectEvent::Config { id: 6, result: Ok(None) });
        s.want_effect(6, None);
        s.want_effect(5, None);
        assert!(s.take_commands().is_empty());
        assert_eq!(s.effects.known(6), Some(&Known::Missing));
    }

    /// Catalogue en échec : rien n'est dessiné, et il n'est redemandé qu'un nombre borné de fois.
    #[test]
    fn failed_catalog_is_retried_a_bounded_number_of_times() {
        let mut s = Store::new();
        ready(&mut s);
        for round in 0..MAX_CATALOG_TRIES + 2 {
            s.want_effect(7, None);
            s.apply_effects(EffectEvent::Config { id: 7, result: Ok(None) });
            let asked = s.take_commands().contains(&Cmd::Effects(EffectCmd::Catalog));
            assert_eq!(asked, round < MAX_CATALOG_TRIES, "tour {round}");
            if asked {
                s.apply_effects(EffectEvent::Catalog(Err(Problem::Http(500))));
                assert!(s.effects.known(7).is_none(), "oublié : redemandé au prochain profil ouvert");
            }
        }
        assert_eq!(s.effects.known(7), Some(&Known::Missing));
    }

    #[test]
    fn owned_effects_are_listed_and_their_configs_kept() {
        let mut s = Store::new();
        s.want_owned_effects();
        s.want_owned_effects();
        assert_eq!(s.take_commands(), vec![Cmd::Effects(EffectCmd::Owned)]);
        s.apply_effects(EffectEvent::Owned(Ok(vec![config(3), config(4)])));
        assert_eq!(s.effects.owned, Remote::Ready(vec![3, 4]));
        assert!(s.effects.config(4).is_some());
        // Borne : les plus anciennes configurations sont oubliées.
        for id in 100..100 + MAX_CONFIGS as u64 + 10 {
            s.effects.set(id, Known::Missing);
        }
        assert!(s.effects.configs.len() <= MAX_CONFIGS);
    }
}
