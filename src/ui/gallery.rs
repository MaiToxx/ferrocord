//! Planche des fondations visuelles (lot V1) : toutes les icônes, puis les
//! composants partagés (boutons, champ, réglages, menu, fenêtre modale).
//! Montrée par l'aperçu hors ligne à la place de l'interface, pour les
//! captures ; l'application ne l'ouvre jamais.

use super::settings::shell;
use super::{fonts, menu, widgets, widgets::Kind, App};
use crate::i18n::t as tx;
use crate::theme::{icons, icons::Icon};
use eframe::egui::{self, vec2, Id, RichText, Sense, Vec2};

/// Page de la planche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Icons,
    Components,
    Modal,
    /// Jeu d'essai des glyphes (`fonts::SAMPLES`).
    Glyphs,
    /// Polices de nom (`fonts::NAMES`).
    NameFonts,
}

/// État de la planche : ce que ses réglages d'essai valent.
#[derive(Clone, Debug)]
pub(super) struct Gallery {
    page: Page,
    text: String,
    on: bool,
    off: bool,
    check: bool,
    value: f32,
    choice: usize,
}

impl Gallery {
    fn new(page: Page) -> Self {
        Self { page, text: String::new(), on: true, off: false, check: true, value: 40.0, choice: 0 }
    }
}

impl App {
    /// Ouvre la planche `page` (`icones`, `composants`, `fenetre`, `glyphes`, `polices-nom`) à la place
    /// de l'interface. `false` : page inconnue.
    pub fn open_gallery(&mut self, page: &str) -> bool {
        let page = match page {
            "icones" => Page::Icons,
            "composants" => Page::Components,
            "fenetre" => Page::Modal,
            "glyphes" => Page::Glyphs,
            "polices-nom" => Page::NameFonts,
            _ => return false,
        };
        self.gallery = Some(Gallery::new(page));
        true
    }

    /// Dessine la planche ; `false` : aucune planche ouverte.
    pub(super) fn gallery_ui(&mut self, ui: &mut egui::Ui) -> bool {
        let Some(mut g) = self.gallery.take() else { return false };
        let t = &self.theme;
        let lang = self.lang;
        let frame = egui::Frame::new().fill(t.colors.bg2).inner_margin(t.spacing.card_pad);
        egui::CentralPanel::default().frame(frame).show(ui, |ui| {
            let title = |ui: &mut egui::Ui, text: &str| {
                ui.add_space(t.spacing.item_x);
                ui.label(RichText::new(text).family(fonts::demi()).size(t.typography.title).color(t.colors.text_strong));
            };
            match g.page {
                Page::Glyphs => fonts::samples_ui(ui, t),
                Page::NameFonts => super::cosmetics::fonts_board(ui, t),
                Page::Icons => {
                    // Chaque icône à la taille des jetons puis en grand, son nom de code dessous.
                    let big = (1.5 * t.typography.icon).round();
                    let cell = vec2(3.0 * big, big + t.typography.small + 3.0 * t.spacing.item_x);
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = Vec2::splat(t.spacing.item_y);
                        for icon in Icon::ALL {
                            let (rect, _) = ui.allocate_exact_size(cell, Sense::hover());
                            let p = ui.painter();
                            let small = egui::Rect::from_center_size(rect.center_top() + vec2(-big / 2.0 - t.spacing.item_y, t.spacing.item_x + big / 2.0), Vec2::splat(t.typography.icon));
                            let large = egui::Rect::from_center_size(rect.center_top() + vec2(big / 2.0, t.spacing.item_x + big / 2.0), Vec2::splat(big));
                            icons::paint(*icon, p, small, t.colors.muted);
                            icons::paint(*icon, p, large, t.colors.text_strong);
                            widgets::centered(p, egui::Rect::from_min_max(egui::pos2(rect.left(), large.bottom()), rect.max), &format!("{icon:?}"), egui::FontId::proportional(t.typography.small), t.colors.faint);
                        }
                    });
                }
                Page::Components | Page::Modal => {
                    ui.columns(2, |cols| {
                        let ui = &mut cols[0];
                        title(ui, tx::GALLERY_BUTTONS.get(lang));
                        for enabled in [true, false] {
                            ui.add_enabled_ui(enabled, |ui| {
                                ui.horizontal(|ui| {
                                    widgets::button(ui, t, Kind::Primary, tx::SAVE.get(lang));
                                    widgets::button(ui, t, Kind::Secondary, tx::CANCEL.get(lang));
                                    widgets::button(ui, t, Kind::Danger, tx::DELETE.get(lang));
                                    widgets::button(ui, t, Kind::Success, tx::REPLY.get(lang));
                                    widgets::button(ui, t, Kind::Link, tx::HELP.get(lang));
                                });
                            });
                        }
                        let wide = ui.available_width().min(t.layout.card_width);
                        widgets::button_wide(ui, t, Kind::Primary, tx::SAVE.get(lang), wide);
                        title(ui, tx::GALLERY_FIELD.get(lang));
                        widgets::field_label(ui, t, tx::GALLERY_FIELD.get(lang));
                        ui.allocate_ui(vec2(wide, t.field_height()), |ui| widgets::field(ui, t, &mut g.text, tx::GALLERY_FIELD_HINT.get(lang)));
                        title(ui, tx::GALLERY_CONTROLS.get(lang));
                        ui.horizontal(|ui| {
                            shell::switch(ui, &mut g.on);
                            shell::switch(ui, &mut g.off);
                            ui.checkbox(&mut g.check, tx::GALLERY_OPTION.get(lang));
                            ui.radio_value(&mut g.choice, 0, tx::GALLERY_OPTION.get(lang));
                            ui.radio_value(&mut g.choice, 1, tx::GALLERY_MORE.get(lang));
                        });
                        ui.add(egui::Slider::new(&mut g.value, 0.0..=100.0));
                        egui::ComboBox::from_id_salt("planche_liste").selected_text(tx::GALLERY_OPTION.get(lang)).show_ui(ui, |ui| {
                            ui.selectable_value(&mut g.choice, 0, tx::GALLERY_OPTION.get(lang));
                            ui.selectable_value(&mut g.choice, 1, tx::GALLERY_MORE.get(lang));
                        });
                        ui.horizontal(|ui| {
                            let r = ui.button(tx::HELP.get(lang));
                            widgets::tip(r, t, tx::CLOSE_TIP.get(lang));
                            let (rect, r) = ui.allocate_exact_size(Vec2::splat(widgets::button_height(t)), Sense::hover());
                            icons::paint(Icon::Inbox, ui.painter(), rect.shrink(t.spacing.item_y), t.colors.muted);
                            widgets::tip_at(r, t, tx::HELP.get(lang), widgets::Side::Right);
                        });

                        let ui = &mut cols[1];
                        title(ui, tx::GALLERY_MENU.get(lang));
                        ui.allocate_ui(vec2(menu::MIN_WIDTH + t.menu_margin().sum().x, 0.0), |ui| {
                            t.menu_frame().show(ui, |ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                ui.set_width(menu::MIN_WIDTH);
                                menu::item(ui, t, tx::REPLY.get(lang), Some(Icon::Reply), false);
                                menu::item(ui, t, tx::COPY_LINK.get(lang), Some(Icon::Link), false);
                                menu::separator(ui, t);
                                menu::check(ui, t, tx::GALLERY_OPTION.get(lang), &mut g.check);
                                for (i, text) in [tx::GALLERY_OPTION, tx::GALLERY_MORE].into_iter().enumerate() {
                                    if menu::radio(ui, t, text.get(lang), g.choice == i).clicked() {
                                        g.choice = i;
                                    }
                                }
                                menu::item(ui, t, tx::GALLERY_MORE.get(lang), Some(Icon::ChevronRight), false);
                                menu::separator(ui, t);
                                menu::item(ui, t, tx::DELETE.get(lang), Some(Icon::Trash), true);
                            });
                        });
                    });
                }
            }
        });
        if g.page == Page::Modal {
            let out = widgets::modal(
                ui.ctx(),
                t,
                Id::unique("planche_fenetre"),
                t.layout.card_width,
                tx::GALLERY_MODAL.get(lang),
                tx::CLOSE_TIP.get(lang),
                |ui| {
                    ui.label(RichText::new(tx::GALLERY_MODAL_BODY.get(lang)).color(t.colors.muted));
                    widgets::field_label(ui, t, tx::GALLERY_FIELD.get(lang));
                    widgets::field(ui, t, &mut g.text, tx::GALLERY_FIELD_HINT.get(lang));
                },
                |ui| {
                    widgets::button(ui, t, Kind::Primary, tx::SAVE.get(lang));
                    widgets::button(ui, t, Kind::Link, tx::CANCEL.get(lang));
                },
            );
            if out.closed {
                g.page = Page::Components;
            }
        }
        self.gallery = Some(g);
        true
    }
}
