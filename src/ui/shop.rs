//! Page de la boutique, en consultation (lot ND) : une ligne « Boutique »
//! dans la colonne des MP, et la page à la place de la page Amis.
//! Catégories et produits tels que Discord les renvoie, image fixe et nom.
//!
//! Rien ne s'achète ici : aucun panier, aucun prix, aucune requête de
//! paiement. « Voir dans Discord » ouvre la boutique dans le navigateur.

use super::friends::{empty_state, PAGE_X, PAGE_Y};
use super::util::pixel_square;
use super::App;
use crate::i18n::table::nd::{f as nf, p as np, t as nt};
use crate::state::inventory::Origin;
use crate::images::{self, Image};
use crate::state::model::ImageRef;
use crate::state::shop::{Category, Product, ProductKind, SHOP_URL};
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense};

/// Largeur d'une vignette de produit, et hauteur de son image.
const TILE: f32 = 168.0;
const PICTURE: f32 = 120.0;

fn kind_text(k: ProductKind) -> crate::i18n::Text {
    match k {
        ProductKind::Decoration => nt::SHOP_DECORATION,
        ProductKind::Effect => nt::EFFECT,
        ProductKind::Nameplate => nt::SHOP_NAMEPLATE,
        ProductKind::Frame => nt::SHOP_FRAME,
        ProductKind::Bundle => nt::SHOP_BUNDLE,
        ProductKind::Other => nt::SHOP_OTHER,
    }
}

impl App {
    /// Ligne « Boutique » de la colonne des MP ; `true` si elle vient d'être cliquée.
    pub(super) fn shop_entry(&self, ui: &mut egui::Ui) -> bool {
        self.dm_entry(ui, Icon::Shop, nt::SHOP.get(self.lang), self.shop_shown() && self.store.channel.is_none(), false).clicked()
    }

    pub(super) fn shop_shown(&self) -> bool {
        self.quests.open && self.quests.shop
    }

    /// Ouvre la boutique (clic sur sa ligne, aperçu, tests) : c'est ce geste
    /// qui demande les catégories.
    pub fn show_shop(&mut self) {
        self.store.select_dms();
        self.requests.open = false;
        self.quests = super::quests::QuestsPage { open: true, shop: true, ..Default::default() };
        self.store.shop_open();
    }

    /// Ouvre la boutique sur son onglet « Inventaire » (aperçu, tests).
    pub fn show_inventory(&mut self) {
        self.show_shop();
        self.quests.inventory = true;
        self.store.inventory_open();
    }

    pub(super) fn shop_page(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let mobile = self.mobile_active();
        let (mut refresh, mut official) = (false, false);
        egui::Panel::top("shop_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                self.mobile_back(ui);
                let side = super::chat_head::icon_side(&t);
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                icons::paint(Icon::Boost, ui.painter(), pixel_square(ui, slot.min, side), t.colors.muted);
                ui.label(RichText::new(nt::SHOP.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                ui.separator();
                for (inventory, text) in [(false, nt::SHOP_TAB), (true, nt::INVENTORY)] {
                    if super::friends::tab(ui, &t, text.get(lang), self.quests.inventory == inventory).clicked() && self.quests.inventory != inventory {
                        self.quests.inventory = inventory;
                        if inventory {
                            self.store.inventory_open();
                        }
                    }
                }
                let busy = if self.quests.inventory { self.store.inventory.loading() } else { self.store.shop.loading() };
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(super::form::btn(&t, nt::SHOP_OFFICIAL.get(lang))).on_hover_text(nt::SHOP_OFFICIAL_TIP.get(lang)).clicked() {
                        official = true;
                    }
                    if ui.add_enabled(!busy, super::form::btn(&t, nt::REFRESH.get(lang))).clicked() {
                        refresh = true;
                    }
                    // Solde d'Orbes : seulement si Discord l'a annoncé.
                    if let Some(n) = self.store.quests.orbs {
                        let label = ui.label(RichText::new(np::orbs(lang, n)).family(crate::ui::fonts::demi()).color(t.colors.text));
                        super::widgets::tip(label, &t, nt::ORBS_TIP.get(lang));
                    }
                });
            });
        });
        if refresh && self.quests.inventory {
            self.store.inventory_refresh();
        } else if refresh {
            self.store.shop_refresh();
        }
        let margin = if mobile { t.chat_head_margin() } else { egui::Margin { left: PAGE_X, right: PAGE_X, top: PAGE_Y, bottom: 0 } };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(margin)).show(ui, |ui| {
            if self.quests.inventory {
                return self.inventory_body(ui, &t);
            }
            let list = match &self.store.shop.list {
                Remote::Ready(l) => l,
                Remote::Idle | Remote::Loading => return empty_state(ui, &t, Icon::Boost, nt::SHOP_LOADING.get(lang)),
                Remote::Failed(problem) => {
                    let text = if *problem == Problem::Forbidden { nt::SHOP_REFUSED.get(lang).to_string() } else { format!("{} {}", nt::SHOP_FAILED.get(lang), super::words::problem(lang, problem, self.h24())) };
                    return empty_state(ui, &t, Icon::Boost, &text);
                }
            };
            ui.add(egui::Label::new(RichText::new(nt::SHOP_TIP.get(lang)).small().color(t.colors.muted)).wrap());
            ui.add_space(t.spacing.item_y);
            if list.is_empty() {
                return empty_state(ui, &t, Icon::Boost, nt::SHOP_EMPTY.get(lang));
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for cat in list {
                    self.shop_category(ui, &t, cat);
                    ui.add_space(t.spacing.item_x * 2.0);
                }
            });
        });
        if official {
            self.open_link(SHOP_URL);
        }
    }

    /// Onglet « Inventaire » : ce que le compte possède, puis les cadeaux
    /// encore à offrir. Lecture seule.
    fn inventory_body(&self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let c = &t.colors;
        let owned = match &self.store.inventory.list {
            Remote::Ready(o) => o,
            Remote::Idle | Remote::Loading => return empty_state(ui, t, Icon::Boost, nt::INVENTORY_LOADING.get(lang)),
            Remote::Failed(problem) => {
                let text = if *problem == Problem::Forbidden { nt::INVENTORY_REFUSED.get(lang).to_string() } else { format!("{} {}", nt::INVENTORY_FAILED.get(lang), super::words::problem(lang, problem, self.h24())) };
                return empty_state(ui, t, Icon::Boost, &text);
            }
        };
        ui.add(egui::Label::new(RichText::new(nt::INVENTORY_TIP.get(lang)).small().color(c.muted)).wrap());
        ui.add_space(t.spacing.item_y);
        if owned.mine.is_empty() && owned.giftable.is_empty() {
            return empty_state(ui, t, Icon::Boost, nt::INVENTORY_EMPTY.get(lang));
        }
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for (title, list) in [(nt::INVENTORY_MINE, &owned.mine), (nt::INVENTORY_GIFTABLE, &owned.giftable)] {
                if list.is_empty() {
                    continue;
                }
                super::profile::section(ui, t, title.get(lang));
                for e in list {
                    let origin = match e.origin {
                        Origin::Purchase => nt::ORIGIN_PURCHASE,
                        Origin::Subscription => nt::ORIGIN_SUBSCRIPTION,
                        Origin::Gift => nt::ORIGIN_GIFT,
                        Origin::Quest => nt::ORIGIN_QUEST,
                        Origin::Orbs => nt::ORIGIN_ORBS,
                        Origin::Other => nt::SHOP_OTHER,
                    };
                    let name = if e.name.is_empty() { nf::unnamed_item(lang, e.sku_id) } else { e.name.clone() };
                    let mut detail = origin.get(lang).to_string();
                    if let Some(at) = e.starts_ms {
                        detail = nf::since(lang, &detail, crate::i18n::time::long_date(lang, at));
                    }
                    if let Some(end) = e.ends_ms {
                        detail = nf::until(lang, &detail, crate::i18n::time::long_date(lang, end));
                    }
                    if e.consumed {
                        detail = nf::consumed(lang, &detail);
                    }
                    egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_row()).inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
                        ui.set_width(ui.available_width().min(560.0));
                        ui.label(RichText::new(name).family(crate::ui::fonts::demi()).color(c.text_strong));
                        ui.label(RichText::new(detail).small().color(c.muted));
                    });
                    ui.add_space(t.spacing.item_y / 2.0);
                }
                ui.add_space(t.spacing.item_x);
            }
        });
    }

    fn shop_category(&self, ui: &mut egui::Ui, t: &Tokens, cat: &Category) {
        let c = &t.colors;
        ui.label(RichText::new(&cat.name).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(c.text_strong));
        if !cat.summary.is_empty() {
            ui.add(egui::Label::new(RichText::new(&cat.summary).small().color(c.muted)).wrap());
        }
        ui.add_space(t.spacing.item_y);
        let gap = t.spacing.item_x;
        let cols = (((ui.available_width() + gap) / (TILE + gap)).floor() as usize).max(1);
        for row in cat.products.chunks(cols) {
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for product in row {
                    ui.allocate_ui_with_layout(egui::vec2(TILE, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| self.shop_tile(ui, t, product));
                }
            });
            ui.add_space(gap);
        }
    }

    fn shop_tile(&self, ui: &mut egui::Ui, t: &Tokens, product: &Product) {
        let lang = self.lang;
        let c = &t.colors;
        let radius = t.r_card();
        egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(radius).show(ui, |ui| {
            ui.set_width(TILE);
            ui.spacing_mut().item_spacing.y = t.spacing.item_y / 2.0;
            let (picture, _) = ui.allocate_exact_size(egui::vec2(TILE, PICTURE), Sense::hover());
            if ui.is_rect_visible(picture) {
                let top = egui::CornerRadius { sw: 0, se: 0, ..radius };
                ui.painter().rect_filled(picture, top, c.bg0);
                // Image entière, centrée dans le cadre.
                let side = PICTURE - 2.0 * t.spacing.item_y;
                let inner = egui::Rect::from_center_size(picture.center(), egui::Vec2::splat(side));
                if let Some(url) = &product.image
                    && let Image::Ready(tex) = self.images.get_asset(ImageRef::Media { owner: product.sku_id, slot: 2, rev: 1 }, url, inner.size())
                {
                    ui.painter().add(images::textured(inner, t.r_row(), tex));
                }
            }
            egui::Frame::new().inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
                ui.set_width(TILE - 2.0 * t.spacing.item_x);
                let name = ui.add(egui::Label::new(RichText::new(&product.name).family(crate::ui::fonts::demi()).color(c.text_strong)).truncate());
                if !product.summary.is_empty() {
                    name.on_hover_text(&product.summary);
                }
                ui.label(RichText::new(kind_text(product.kind).get(lang)).small().color(c.muted));
                // Prix de base, à titre indicatif : l'achat se fait dans Discord.
                let prices: Vec<String> = product.prices.iter().map(|p| if p.orbs() { np::orbs(lang, p.amount) } else { format!("{} {}", crate::i18n::decimal(lang, p.value(), usize::from(p.exponent)), p.currency.to_uppercase()) }).collect();
                if !prices.is_empty() {
                    ui.label(RichText::new(prices.join(" · ")).small().color(c.text)).on_hover_text(nt::SHOP_PRICE_TIP.get(lang));
                }
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, find, frame, offline_boot};
    use super::*;
    use crate::i18n::Lang;
    use crate::state::model::User;
    use crate::state::shop::{ShopCmd, ShopEvent};
    use crate::state::{Cmd, Event, Store};
    use std::sync::{Arc, Mutex};

    struct Recorder(Arc<Mutex<Vec<Cmd>>>);

    impl super::super::Backend for Recorder {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            if let Ok(mut log) = self.0.lock() {
                log.extend(cmds);
            }
        }
    }

    /// Les catégories partent à l'ouverture de la page, une fois ; la page
    /// n'envoie rien d'autre (aucun achat).
    #[test]
    fn shop_asks_once_and_never_buys() {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        store.select_dms();
        let log = Arc::new(Mutex::new(Vec::new()));
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = super::super::App::with_context(ctx, offline_boot(Box::new(Recorder(log.clone())), store, images));
        let sent = |log: &Arc<Mutex<Vec<Cmd>>>| log.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();

        frame(&mut app, Vec::new());
        assert!(!sent(&log).iter().any(|c| matches!(c, Cmd::Shop(_))), "rien ne part tant que la page n'est pas ouverte");
        click(&mut app, nt::SHOP.get(Lang::Fr));
        assert!(app.shop_shown() && !sent(&log).iter().any(|c| matches!(c, Cmd::Quests(_))), "la boutique ne demande pas les quêtes");
        let cats = crate::net::decode_shop(include_str!("../../tests/fixtures/nd/rest_collectibles_categories.json")).unwrap_or_default();
        app.store.apply(Event::Shop(ShopEvent::Loaded(Ok(cats))));
        let out = frame(&mut app, Vec::new());
        let mut drawn = String::new();
        fn walk(s: &egui::Shape, out: &mut String) {
            match s {
                egui::Shape::Text(t) => {
                    out.push_str(&t.galley.job.text);
                    out.push(' ');
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut drawn));
        assert!(drawn.contains("4,99 EUR") && drawn.contains("400 Orbes"), "prix de base absent de {drawn}");
        for text in ["Collection fictive", "Décoration fictive", "Pluie fictive", "Plaque fictive", "Orbes fictives", "Lot fictif"] {
            assert!(drawn.contains(text), "{text} absent de {drawn}");
        }
        assert!(find(&out, nt::SHOP_OFFICIAL.get(Lang::Fr)).is_some());
        click(&mut app, nt::REFRESH.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert_eq!(sent(&log), vec![Cmd::Shop(ShopCmd::Load)], "« Actualiser » : une demande");
        // La ligne « Quêtes » reprend la page.
        click(&mut app, nt::QUESTS.get(Lang::Fr));
        assert!(app.quests_shown() && !app.shop_shown());
    }
}
