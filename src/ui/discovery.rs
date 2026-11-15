//! Page « Découvrir des serveurs » (lot ND), en consultation : la boussole
//! du rail l'ouvre à la place de la page Amis. Recherche validée par Entrée
//! ou par le bouton, catégories, liste des serveurs.
//!
//! Rejoindre ne se fait pas ici : « Rejoindre… » préremplit la fenêtre
//! « Rejoindre un serveur » avec l'adresse personnalisée du serveur (le
//! clic final reste celui de cette fenêtre) ; sans adresse personnalisée,
//! « Voir la page » ouvre la page publique dans le navigateur.

use super::friends::{empty_state, tab, PAGE_X, PAGE_Y};
use super::util::pixel_square;
use super::App;
use crate::i18n::table::nd::{p as np, t as nt};
use crate::i18n::Lang;
use crate::images::{self, Image};
use crate::state::discovery::{Found, QUERY_MAX};
use crate::state::model::ImageRef;
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense};

const ICON: f32 = 44.0;

#[derive(Default)]
pub(super) struct DiscoverPage {
    /// Texte du champ (envoyé seulement une fois validé).
    query: String,
    category: Option<u32>,
    /// Onglet « Applications » (`ui/app_directory.rs`) et sa recherche.
    pub(super) apps: bool,
    pub(super) app_query: String,
    pub(super) app_category: Option<u32>,
}

pub(super) fn locale(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "fr",
        Lang::En => "en-US",
    }
}

/// Ce qu'un clic dans une ligne demande.
enum Act {
    Join(String),
    Page(String),
    Open(u64),
}

impl App {
    /// Ouvre la page (boussole du rail, menu « + », aperçu, tests) : c'est ce
    /// geste qui demande les catégories et la première liste.
    pub fn show_discovery(&mut self) {
        self.store.select_dms();
        self.requests.open = false;
        self.quests = super::quests::QuestsPage { open: true, discover: true, ..Default::default() };
        self.store.discovery_open(locale(self.lang));
    }

    pub(super) fn discovery_page(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let mobile = self.mobile_active();
        let mut official = false;
        egui::Panel::top("discovery_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                self.mobile_back(ui);
                let side = super::chat_head::icon_side(&t);
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                icons::paint(Icon::Compass, ui.painter(), pixel_square(ui, slot.min, side), t.colors.muted);
                ui.label(RichText::new(nt::DISCOVER_TITLE.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                ui.separator();
                for (apps, text) in [(false, nt::DISCOVER_SERVERS), (true, nt::DISCOVER_APPS)] {
                    if tab(ui, &t, text.get(lang), self.discover.apps == apps).clicked() && self.discover.apps != apps {
                        self.discover.apps = apps;
                        if apps {
                            self.store.appdir_open(locale(lang));
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(super::form::btn(&t, nt::SHOP_OFFICIAL.get(lang))).on_hover_text(nt::DISCOVER_OFFICIAL_TIP.get(lang)).clicked() {
                        official = true;
                    }
                });
            });
        });
        let margin = if mobile { t.chat_head_margin() } else { egui::Margin { left: PAGE_X, right: PAGE_X, top: PAGE_Y, bottom: 0 } };
        let mut act = None;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(margin)).show(ui, |ui| {
            if self.discover.apps {
                act = self.app_directory_body(ui, &t).map(Act::Page);
                return;
            }
            let mut search = false;
            ui.horizontal(|ui| {
                let button = ui.spacing().interact_size.x.max(96.0);
                ui.allocate_ui_with_layout(egui::vec2((ui.available_width() - button - t.spacing.item_x).max(120.0), t.field_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                    let edit = crate::ui::widgets::field(ui, &t, &mut self.discover.query, nt::DISCOVER_HINT.get(lang));
                    search |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                });
                search |= ui.add_enabled(!self.store.discovery.loading(), super::form::btn(&t, nt::DISCOVER_SEARCH.get(lang))).clicked();
            });
            if self.discover.query.chars().count() > QUERY_MAX {
                self.discover.query = self.discover.query.chars().take(QUERY_MAX).collect();
            }
            ui.add_space(t.spacing.item_y);
            // Catégories : seulement si Discord en a renvoyé.
            if let Remote::Ready(cats) = &self.store.discovery.categories
                && !cats.is_empty()
            {
                let mut pick = None;
                ui.horizontal_wrapped(|ui| {
                    if tab(ui, &t, nt::DISCOVER_ALL.get(lang), self.discover.category.is_none()).clicked() {
                        pick = Some(None);
                    }
                    for c in cats {
                        if tab(ui, &t, &c.name, self.discover.category == Some(c.id)).clicked() {
                            pick = Some(Some(c.id));
                        }
                    }
                });
                if let Some(p) = pick.filter(|p| *p != self.discover.category) {
                    self.discover.category = p;
                    search = true;
                }
                ui.add_space(t.spacing.item_y);
            }
            if search {
                let query = self.discover.query.clone();
                self.store.discovery_search(&query, self.discover.category);
            }
            let list = match &self.store.discovery.results {
                Remote::Ready(l) => l,
                Remote::Idle | Remote::Loading => return empty_state(ui, &t, Icon::Compass, nt::DISCOVER_LOADING.get(lang)),
                Remote::Failed(problem) => {
                    let text = if *problem == Problem::Forbidden { nt::DISCOVER_REFUSED.get(lang).to_string() } else { format!("{} {}", nt::DISCOVER_FAILED.get(lang), super::words::problem(lang, problem, self.h24())) };
                    return empty_state(ui, &t, Icon::Compass, &text);
                }
            };
            if list.is_empty() {
                return empty_state(ui, &t, Icon::Compass, nt::DISCOVER_EMPTY.get(lang));
            }
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                for g in list {
                    if let Some(a) = self.discovery_row(ui, &t, g) {
                        act = Some(a);
                    }
                    ui.add_space(t.spacing.item_y / 2.0);
                }
                ui.add_space(t.spacing.item_x);
            });
        });
        match act {
            Some(Act::Join(code)) => self.open_join_with(&code),
            Some(Act::Page(url)) => self.open_link(&url),
            Some(Act::Open(id)) => {
                self.quests.open = false;
                if self.store.select_guild(id) {
                    self.scroll_to_bottom = true;
                }
            }
            None => {}
        }
        if official {
            self.open_link(if self.discover.apps { super::app_directory::DIRECTORY_URL } else { super::guild_create::DISCOVERY_URL });
        }
    }

    fn discovery_row(&self, ui: &mut egui::Ui, t: &Tokens, g: &Found) -> Option<Act> {
        let lang = self.lang;
        let c = &t.colors;
        let joined = self.store.guilds.iter().any(|x| x.id == g.guild_id);
        let mut act = None;
        egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_row()).inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(ICON), Sense::hover());
                if ui.is_rect_visible(slot) {
                    ui.painter().rect_filled(slot, t.r_row(), c.bg0);
                    match g.icon.as_ref().map(|url| self.images.get_asset(ImageRef::Media { owner: g.guild_id, slot: 3, rev: 1 }, url, slot.size())) {
                        Some(Image::Ready(tex)) => {
                            ui.painter().add(images::textured(slot, t.r_row(), tex));
                        }
                        _ => {
                            let initial: String = g.name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                            ui.painter().text(slot.center(), egui::Align2::CENTER_CENTER, initial, crate::ui::fonts::demi_font(t.typography.title), c.muted);
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if joined {
                        if ui.add(super::form::btn(t, nt::DISCOVER_OPEN.get(lang))).clicked() {
                            act = Some(Act::Open(g.guild_id));
                        }
                    } else if let Some(code) = &g.vanity {
                        if ui.add(super::form::btn(t, nt::DISCOVER_JOIN.get(lang))).on_hover_text(nt::DISCOVER_JOIN_TIP.get(lang)).clicked() {
                            act = Some(Act::Join(code.clone()));
                        }
                    } else if ui.add(super::form::btn(t, nt::DISCOVER_PAGE.get(lang))).on_hover_text(nt::DISCOVER_PAGE_TIP.get(lang)).clicked() {
                        act = Some(Act::Page(g.page_url()));
                    }
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(egui::Label::new(RichText::new(&g.name).family(crate::ui::fonts::demi()).color(c.text_strong)).truncate());
                        if !g.description.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&g.description).small().color(c.text)).truncate()).on_hover_text(&g.description);
                        }
                        let mut counts = np::server_members(lang, g.members);
                        if g.online > 0 {
                            counts = format!("{counts} · {}", np::online(lang, g.online));
                        }
                        if joined {
                            counts = format!("{counts} · {}", nt::DISCOVER_JOINED.get(lang));
                        }
                        ui.label(RichText::new(counts).small().color(c.muted));
                    });
                });
            });
        });
        act
    }
}
