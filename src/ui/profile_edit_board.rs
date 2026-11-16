//! Éditeur de profil : colonne de droite, les widgets de jeux du profil
//! (`state::widgets`). Onglet « Tableau » : « Jeux du moment », « Jeu
//! favori », « Jeux que j'aime » ; onglet « Liste de souhaits ».
//!
//! Tout se règle dans le brouillon : l'écriture (`PUT /users/@me/widgets`)
//! ne part qu'avec « Enregistrer ». La source publique ne décrit aucune
//! recherche de jeu par nom : le choix propose les suggestions de Discord
//! et filtre la liste des jeux détectables, si elle est chargée. L'onglet
//! « Activité » du client officiel n'est décrit nulle part : il n'est pas là.

use super::profile_edit::{BoardTab, Editor, Picker};
use super::profile_edit_left::r_cover;
use super::App;
use crate::i18n::table::quick::{f as qf, t as qt};
use crate::state::model::ImageRef;
use crate::state::usersettings::Remote;
use crate::state::widgets::{Game, GameWidget, WidgetGame, WidgetKind, MAX_COMMENT};
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Id, Rect, RichText, Sense};

/// Résultats montrés dans le choix d'un jeu.
const MAX_RESULTS: usize = 30;

fn title(kind: WidgetKind) -> crate::i18n::Text {
    match kind {
        WidgetKind::Current => qt::ED_W_CURRENT,
        WidgetKind::Favorite => qt::ED_W_FAVORITE,
        WidgetKind::Played => qt::ED_W_PLAYED,
        WidgetKind::Wanted | WidgetKind::Application => qt::ED_W_WANTED,
    }
}

/// Bouton carré « + » ou « × » d'une carte de widget.
fn square_button(ui: &mut egui::Ui, t: &Tokens, icon: Icon, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(t.spacing.control_height.max(28.0)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), tip));
    let hot = resp.hovered() || resp.has_focus();
    ui.painter().rect_filled(rect, t.r_row(), if hot { t.colors.row_hover } else { t.colors.bg3 });
    icons::paint(icon, ui.painter(), Rect::from_center_size(rect.center(), egui::Vec2::splat(14.0)), if hot { t.colors.text_strong } else { t.colors.text });
    super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Ce qu'un clic dans une carte de widget demande.
enum Act {
    Add,
    RemoveGame(usize),
    RemoveWidget,
    Comment(usize, String),
}

impl App {
    /// Nom d'un jeu : celui de Discord (`GET /games`), sinon celui de la liste des jeux détectables.
    fn game_name(&self, id: u64) -> Option<String> {
        self.store.widgets.game(id).map(|g| g.name.clone()).or_else(|| self.store.activity.index.as_ref()?.app(id).map(|a| a.name.to_string()))
    }

    /// Colonne des widgets (à droite du profil, ou dessous dans une fenêtre étroite).
    pub(super) fn editor_board(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor) {
        let lang = self.lang;
        let c = &t.colors;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.category_gap;
            for (tab, label) in [(BoardTab::Board, qt::ED_TAB_BOARD), (BoardTab::Wishlist, qt::ED_TAB_WISHLIST)] {
                if super::widgets::tab(ui, t, label.get(lang), e.tab == tab).clicked() {
                    e.tab = tab;
                }
            }
        });
        let y = ui.cursor().top() - ui.spacing().item_spacing.y;
        ui.painter().line_segment([egui::pos2(ui.max_rect().left(), y), egui::pos2(ui.max_rect().right(), y)], egui::Stroke::new(1.0, c.separator));
        ui.add_space(t.spacing.item_x);
        if e.worn.widgets.is_none() {
            ui.label(RichText::new(qt::ED_WIDGETS_LOADING.get(lang)).color(c.muted));
            return;
        }
        let locked = e.worn.widgets_locked;
        if locked {
            ui.add(egui::Label::new(RichText::new(qt::ED_WIDGETS_READONLY.get(lang)).small().color(c.muted)).wrap());
        }
        let has = |e: &Editor, kind: WidgetKind| e.widgets_now().iter().any(|w| w.kind == kind);
        match e.tab {
            BoardTab::Board => {
                let missing: Vec<WidgetKind> = WidgetKind::BOARD.into_iter().filter(|k| !has(e, *k)).collect();
                let mut add = None;
                ui.horizontal(|ui| {
                    ui.label(RichText::new(qt::ED_YOUR_WIDGETS.get(lang)).family(super::fonts::demi()).size(t.typography.title).color(c.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let r = ui.add_enabled_ui(!locked && !missing.is_empty(), |ui| super::widgets::button(ui, t, super::widgets::Kind::Secondary, &format!("+ {}", qt::ED_ADD_WIDGET.get(lang)))).inner;
                        egui::Popup::menu(&r).id(Id::unique("ferrocord.profil.edition.widgets")).show(|ui| {
                            for kind in &missing {
                                if super::menu::item(ui, t, title(*kind).get(lang), None, false).clicked() {
                                    add = Some(*kind);
                                }
                            }
                        });
                    });
                });
                // Un widget ajouté s'ouvre sur le choix de son premier jeu : vide, il ne serait pas gardé.
                if let Some(kind) = add
                    && let Some(list) = e.widgets_mut()
                {
                    list.push(GameWidget::new(kind));
                    e.picker = Some(Picker::Game(kind));
                    e.game_query.clear();
                }
                let shown: Vec<WidgetKind> = WidgetKind::BOARD.into_iter().filter(|k| has(e, *k)).collect();
                if shown.is_empty() {
                    ui.add_space(t.spacing.item_x);
                    ui.label(RichText::new(qt::ED_NO_WIDGETS.get(lang)).color(c.muted));
                }
                for kind in shown {
                    ui.add_space(t.spacing.item_x);
                    self.widget_card(ui, t, e, kind);
                }
            }
            BoardTab::Wishlist => self.widget_card(ui, t, e, WidgetKind::Wanted),
        }
    }

    /// Carte d'un widget : titre, consigne, « + », puis ses jeux.
    fn widget_card(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, kind: WidgetKind) {
        let lang = self.lang;
        let c = &t.colors;
        let locked = e.worn.widgets_locked;
        let widget = e.widgets_now().iter().find(|w| w.kind == kind).cloned();
        let games: &[WidgetGame] = widget.as_ref().map(|w| w.games.as_slice()).unwrap_or_default();
        let mut act = None;
        let pad = t.spacing.card_pad.round().clamp(0.0, 127.0) as i8;
        egui::Frame::new().fill(super::cosmetics::mix(c.bg1, c.text_strong, 0.05)).corner_radius(t.r_card()).inner_margin(egui::Margin::same(pad)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    ui.label(RichText::new(title(kind).get(lang)).family(super::fonts::demi()).size(t.typography.button).color(c.text_strong));
                    let hint = if kind.max() == 1 { qt::ED_PICK_ONE.get(lang).to_string() } else { qf::up_to(lang, kind.max()) };
                    ui.label(RichText::new(hint).small().color(c.muted));
                });
                if !locked {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        if widget.is_some() && kind != WidgetKind::Wanted && square_button(ui, t, Icon::Trash, qt::ED_REMOVE_WIDGET.get(lang)).clicked() {
                            act = Some(Act::RemoveWidget);
                        }
                        if games.len() < kind.max() && square_button(ui, t, Icon::Plus, qt::ED_ADD_GAME.get(lang)).clicked() {
                            act = Some(Act::Add);
                        }
                    });
                }
            });
            if games.is_empty() && kind == WidgetKind::Wanted {
                ui.label(RichText::new(qt::ED_WISHLIST_EMPTY.get(lang)).color(c.muted));
            }
            for (i, g) in games.iter().enumerate() {
                ui.add_space(t.spacing.item_y);
                // Jeu du moment et jeu favori : grande couverture ; les listes : une ligne par jeu.
                let detailed = matches!(kind, WidgetKind::Current | WidgetKind::Favorite);
                let cover = if detailed { egui::vec2(72.0, 96.0) } else { egui::vec2(42.0, 56.0) };
                let known = self.store.widgets.game(g.game_id);
                let name = self.game_name(g.game_id).unwrap_or_else(|| qt::ED_GAME_UNKNOWN.get(lang).to_string());
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(cover, Sense::hover());
                    self.game_cover(ui, t, rect, g.game_id, known, &name);
                    ui.add_space(t.spacing.item_x);
                    let room = ui.available_width() - if locked { 0.0 } else { t.spacing.control_height.max(28.0) + t.spacing.item_x };
                    ui.allocate_ui_with_layout(egui::vec2(room, cover.y), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.add_space(if detailed { t.spacing.item_x } else { t.spacing.item_y });
                        ui.add(egui::Label::new(RichText::new(&name).family(super::fonts::demi()).size(t.typography.button).color(c.text_strong)).truncate());
                        if kind == WidgetKind::Favorite {
                            let mut text = g.comment.clone().unwrap_or_default();
                            let r = ui.add_enabled(
                                !locked,
                                egui::TextEdit::multiline(&mut text)
                                    .frame(egui::Frame::NONE)
                                    .margin(egui::Margin::ZERO)
                                    .char_limit(MAX_COMMENT)
                                    .text_color(c.text)
                                    .hint_text(RichText::new(qt::ED_COMMENT_HINT.get(lang)).italics().color(c.muted))
                                    .desired_rows(2)
                                    .desired_width(f32::INFINITY),
                            );
                            super::a11y::name(ui.ctx(), r.id, qt::ED_COMMENT_HINT.get(lang));
                            if r.changed() {
                                act = Some(Act::Comment(i, text));
                            }
                        } else if let Some(comment) = &g.comment {
                            ui.add(egui::Label::new(RichText::new(comment).italics().color(c.text)).wrap());
                        }
                    });
                    if !locked {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            if square_button(ui, t, Icon::Close, qt::ED_REMOVE_GAME.get(lang)).clicked() {
                                act = Some(Act::RemoveGame(i));
                            }
                        });
                    }
                });
            }
        });
        let Some(act) = act else { return };
        if matches!(act, Act::Add) {
            e.picker = Some(Picker::Game(kind));
            e.game_query.clear();
            return;
        }
        let Some(list) = e.widgets_mut() else { return };
        let Some(at) = list.iter().position(|w| w.kind == kind) else { return };
        match act {
            Act::RemoveWidget => {
                list.remove(at);
            }
            Act::RemoveGame(i) if i < list[at].games.len() => {
                list[at].games.remove(i);
            }
            Act::Comment(i, text) => {
                if let Some(g) = list[at].games.get_mut(i) {
                    g.comment = (!text.trim().is_empty()).then_some(text);
                }
            }
            _ => {}
        }
        e.warned = false;
    }

    /// Couverture d'un jeu (`app-icons/{id}/{hash}`), sinon son initiale.
    fn game_cover(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, id: u64, known: Option<&Game>, name: &str) {
        let image = known.and_then(Game::image).map(|hash| ImageRef::AppIcon { app_id: id, hash });
        ui.painter().rect_filled(rect, r_cover(t), t.colors.bg3);
        if !super::util::paint_image(ui, &self.images, rect, image, r_cover(t)) {
            let initial: String = name.chars().next().map(|ch| ch.to_uppercase().collect()).unwrap_or_default();
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::new(rect.width() * 0.4, super::fonts::demi()), t.colors.muted);
        }
    }

    /// Choix d'un jeu à ajouter au widget `kind` : recherche dans la liste
    /// des jeux détectables (si elle est chargée), sinon suggestions de Discord.
    pub(super) fn pick_game(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, kind: WidgetKind) {
        let lang = self.lang;
        let c = &t.colors;
        let field = super::form::field(ui, t, &mut e.game_query, qt::ED_GAME_SEARCH.get(lang), 60);
        super::a11y::name(ui.ctx(), field.id, qt::ED_GAME_SEARCH.get(lang));
        let query = e.game_query.trim().to_lowercase();
        let have: Vec<u64> = e.widgets_now().iter().filter(|w| w.kind == kind).flat_map(|w| w.games.iter().map(|g| g.game_id)).collect();
        let suggested: Vec<u64> = match &self.store.widgets.suggested {
            // Liste de souhaits : ses suggestions d'abord.
            Remote::Ready(s) if kind == WidgetKind::Wanted => s.wishlist.iter().chain(&s.games).copied().collect(),
            Remote::Ready(s) => s.games.iter().chain(&s.wishlist).copied().collect(),
            _ => Vec::new(),
        };
        let searching = query.chars().count() >= 2;
        let mut results: Vec<(u64, String)> = Vec::new();
        if searching {
            if let Some(index) = &self.store.activity.index {
                results.extend(index.apps().filter(|a| a.name.to_lowercase().contains(&query)).map(|a| (a.id, a.name.to_string())));
            }
            results.extend(suggested.iter().filter_map(|id| Some((*id, self.game_name(*id)?))).filter(|(_, n)| n.to_lowercase().contains(&query)));
        } else {
            results.extend(suggested.iter().filter_map(|id| Some((*id, self.game_name(*id)?))));
        }
        let mut seen: Vec<u64> = have;
        results.retain(|(id, _)| {
            let new = !seen.contains(id);
            seen.push(*id);
            new
        });
        results.truncate(MAX_RESULTS);
        ui.add_space(t.spacing.item_y);
        if !searching && !results.is_empty() {
            super::widgets::field_label(ui, t, qt::ED_SUGGESTED.get(lang));
        }
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("profil-edition-jeux").max_height(8.0 * super::menu::row_height(t)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            for (id, name) in &results {
                if super::menu::line(ui, t, super::menu::Line { on_card: true, ..super::menu::Line::new(super::menu::Lead::None, name) }).clicked() {
                    pick = Some(*id);
                }
            }
        });
        if results.is_empty() {
            let text = if !searching && matches!(self.store.widgets.suggested, Remote::Loading) { crate::i18n::table::nd::t::BUSY } else { qt::ED_NO_GAME };
            ui.label(RichText::new(text.get(lang)).color(c.muted));
        }
        ui.add_space(t.spacing.item_y);
        ui.add(egui::Label::new(RichText::new(qt::ED_SEARCH_NOTE.get(lang)).small().color(c.muted)).wrap());
        let Some(id) = pick else { return };
        if let Some(list) = e.widgets_mut() {
            let at = match list.iter().position(|w| w.kind == kind) {
                Some(at) => at,
                None => {
                    list.push(GameWidget::new(kind));
                    list.len() - 1
                }
            };
            let w = &mut list[at];
            if w.games.len() < kind.max() && !w.games.iter().any(|g| g.game_id == id) {
                w.games.push(WidgetGame::new(id));
            }
        }
        e.picker = None;
        e.warned = false;
    }
}
