//! Onglet « Applications » de la page « Découvrir » (lot ND) : le
//! répertoire d'applications de Discord, en consultation. Recherche
//! validée par Entrée ou par le bouton, catégories, liste.
//!
//! Ajouter une application ne se fait pas ici : « Voir la fiche » ouvre sa
//! page du répertoire dans le navigateur.

use super::discovery::locale;
use super::friends::{empty_state, tab};
use super::App;
use crate::i18n::table::nd::{p as np, t as nt};
use crate::images::{self, Image};
use crate::state::appdir::App as Listed;
use crate::state::discovery::QUERY_MAX;
use crate::state::model::ImageRef;
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::icons::Icon;
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense};

pub(super) const DIRECTORY_URL: &str = "https://discord.com/application-directory";
const ICON: f32 = 44.0;

impl App {
    /// Ouvre la page « Découvrir » sur l'onglet « Applications » (aperçu,
    /// tests) : c'est ce geste qui demande les catégories et la liste.
    pub fn show_app_directory(&mut self) {
        self.show_discovery();
        self.discover.apps = true;
        self.store.appdir_open(locale(self.lang));
    }

    /// Corps de l'onglet ; rend l'adresse de la fiche à ouvrir, s'il y en a une.
    pub(super) fn app_directory_body(&mut self, ui: &mut egui::Ui, t: &Tokens) -> Option<String> {
        let lang = self.lang;
        let mut search = false;
        ui.horizontal(|ui| {
            let button = ui.spacing().interact_size.x.max(96.0);
            ui.allocate_ui_with_layout(egui::vec2((ui.available_width() - button - t.spacing.item_x).max(120.0), t.field_height()), egui::Layout::top_down(egui::Align::Min), |ui| {
                let edit = crate::ui::widgets::field(ui, t, &mut self.discover.app_query, nt::APPS_HINT.get(lang));
                search |= edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            });
            search |= ui.add_enabled(!self.store.appdir.loading(), super::form::btn(t, nt::DISCOVER_SEARCH.get(lang))).clicked();
        });
        if self.discover.app_query.chars().count() > QUERY_MAX {
            self.discover.app_query = self.discover.app_query.chars().take(QUERY_MAX).collect();
        }
        ui.add_space(t.spacing.item_y);
        if let Remote::Ready(cats) = &self.store.appdir.categories
            && !cats.is_empty()
        {
            let mut pick = None;
            ui.horizontal_wrapped(|ui| {
                if tab(ui, t, nt::DISCOVER_ALL.get(lang), self.discover.app_category.is_none()).clicked() {
                    pick = Some(None);
                }
                for c in cats {
                    if tab(ui, t, &c.name, self.discover.app_category == Some(c.id)).clicked() {
                        pick = Some(Some(c.id));
                    }
                }
            });
            if let Some(p) = pick.filter(|p| *p != self.discover.app_category) {
                self.discover.app_category = p;
                search = true;
            }
            ui.add_space(t.spacing.item_y);
        }
        if search {
            let query = self.discover.app_query.clone();
            self.store.appdir_search(&query, self.discover.app_category, locale(lang));
        }
        let list = match &self.store.appdir.results {
            Remote::Ready(l) => l,
            Remote::Idle | Remote::Loading => {
                empty_state(ui, t, Icon::Compass, nt::APPS_LOADING.get(lang));
                return None;
            }
            Remote::Failed(problem) => {
                let text = if *problem == Problem::Forbidden { nt::APPS_REFUSED.get(lang).to_string() } else { format!("{} {}", nt::APPS_FAILED.get(lang), super::words::problem(lang, problem, self.h24())) };
                empty_state(ui, t, Icon::Compass, &text);
                return None;
            }
        };
        if list.is_empty() {
            empty_state(ui, t, Icon::Compass, nt::APPS_EMPTY.get(lang));
            return None;
        }
        let mut open = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            for a in list {
                if self.app_row(ui, t, a) {
                    open = Some(a.page_url());
                }
                ui.add_space(t.spacing.item_y / 2.0);
            }
            ui.add_space(t.spacing.item_x);
        });
        open
    }

    /// Une application ; `true` si « Voir la fiche » est cliqué.
    fn app_row(&self, ui: &mut egui::Ui, t: &Tokens, a: &Listed) -> bool {
        let lang = self.lang;
        let c = &t.colors;
        let mut open = false;
        egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_row()).inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(ICON), Sense::hover());
                if ui.is_rect_visible(slot) {
                    ui.painter().rect_filled(slot, t.r_row(), c.bg0);
                    match a.icon.as_ref().map(|url| self.images.get_asset(ImageRef::Media { owner: a.id, slot: 4, rev: 1 }, url, slot.size())) {
                        Some(Image::Ready(tex)) => {
                            ui.painter().add(images::textured(slot, t.r_row(), tex));
                        }
                        _ => {
                            let initial: String = a.name.chars().find(|ch| ch.is_alphanumeric()).map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
                            ui.painter().text(slot.center(), egui::Align2::CENTER_CENTER, initial, crate::ui::fonts::demi_font(t.typography.title), c.muted);
                        }
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    open = ui.add(super::form::btn(t, nt::APPS_PAGE.get(lang))).on_hover_text(nt::APPS_PAGE_TIP.get(lang)).clicked();
                    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.add(egui::Label::new(RichText::new(&a.name).family(crate::ui::fonts::demi()).color(c.text_strong)).truncate());
                        if !a.summary.is_empty() {
                            ui.add(egui::Label::new(RichText::new(&a.summary).small().color(c.text)).truncate()).on_hover_text(&a.summary);
                        }
                        if let Some(n) = a.guilds.filter(|n| *n > 0) {
                            ui.label(RichText::new(np::servers(lang, n)).small().color(c.muted));
                        }
                    });
                });
            });
        });
        open
    }
}
