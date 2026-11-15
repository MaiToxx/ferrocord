//! Sélecteur rapide (Ctrl+K) : fenêtre centrée, recherche floue locale sur
//! les salons visibles, fils rejoints, serveurs, conversations privées,
//! groupes et amis ; préfixes `#`, `@`, `*`, `!` comme Discord ; destinations
//! récentes en tête ; flèches et Entrée.
//!
//! Les candidats sont réunis à l'ouverture ; la recherche n'est refaite que
//! lorsque le texte change ; seules les lignes visibles sont dessinées.

use super::keys::focus_ring;
use super::util::initials;
use super::{words, App};
use crate::i18n::table::nav::t as nt;
use crate::state::nav::{self, Candidate, Kind, Target};
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Align2, FontId, Id, Key, Modifiers, RichText, Sense};

/// Lignes visibles à la fois, au plus.
const VISIBLE_ROWS: f32 = 9.0;

#[derive(Default)]
pub(super) struct Switcher {
    pub(super) open: bool,
    pub(super) query: String,
    /// Texte pour lequel `hits` est à jour.
    searched: Option<String>,
    cands: Vec<Candidate>,
    hits: Vec<usize>,
    selected: usize,
    /// La sélection a bougé au clavier : la liste la ramène en vue.
    reveal: bool,
    /// Le champ prend le clavier à la prochaine image.
    focus: bool,
}

impl Switcher {
    /// Destination de la ligne choisie.
    pub(super) fn selected_target(&self) -> Option<Target> {
        self.hits.get(self.selected).and_then(|i| self.cands.get(*i)).map(|c| c.target)
    }

    /// Lignes affichées : (genre, nom).
    #[cfg(test)]
    pub(super) fn rows(&self) -> Vec<(Kind, &str)> {
        self.hits.iter().filter_map(|i| self.cands.get(*i)).map(|c| (c.kind, c.name.as_str())).collect()
    }

    /// Refait la recherche si le texte a changé depuis la dernière.
    pub(super) fn refresh(&mut self, recent: &[Target]) {
        if self.searched.as_deref() == Some(self.query.as_str()) {
            return;
        }
        self.hits = nav::search(&self.cands, &self.query, recent);
        self.searched = Some(self.query.clone());
        self.selected = 0;
        self.reveal = true;
    }
}

impl App {
    /// Ouvre le sélecteur : candidats réunis une fois, recherche vide.
    pub fn open_switcher(&mut self) {
        let s = &mut self.nav.switcher;
        s.open = true;
        s.query.clear();
        s.searched = None;
        s.cands = nav::candidates(&self.store);
        s.focus = true;
        let recent: Vec<Target> = self.nav.history.recent().collect();
        self.nav.switcher.refresh(&recent);
    }

    /// Texte du sélecteur, comme une saisie (tests, aperçu).
    pub fn switcher_type(&mut self, text: &str) {
        self.nav.switcher.query = text.to_string();
        let recent: Vec<Target> = self.nav.history.recent().collect();
        self.nav.switcher.refresh(&recent);
    }

    fn choose(&mut self, t: Target) {
        self.nav.switcher.open = false;
        self.nav.switcher.cands = Vec::new();
        self.nav.switcher.hits = Vec::new();
        self.go_target(t);
    }

    pub(super) fn switcher_ui(&mut self, ui: &mut egui::Ui) {
        if !self.nav.switcher.open {
            return;
        }
        let (esc, up, down, enter) = ui.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Escape),
                i.consume_key(Modifiers::NONE, Key::ArrowUp),
                i.consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Enter),
            )
        });
        if esc {
            self.nav.switcher.open = false;
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        {
            let s = &mut self.nav.switcher;
            let n = s.hits.len();
            if n > 0 && (up || down) {
                s.selected = if down { (s.selected + 1) % n } else { (s.selected + n - 1) % n };
                s.reveal = true;
            }
        }
        if enter && let Some(target) = self.nav.switcher.selected_target() {
            self.choose(target);
            return;
        }
        let screen = ui.ctx().content_rect();
        let width = (screen.width() - 2.0 * t.spacing.card_pad).clamp(1.0, t.layout.card_width * 1.5);
        let row_h = t.spacing.channel_row + t.spacing.item_y;
        let frame = t.window_frame().inner_margin(t.toast_margin());
        let mut chosen = None;
        let mut close = false;
        // Voile derrière la fenêtre : un clic dessus la ferme.
        let scrim = egui::Area::new(Id::unique("selecteur_voile")).order(egui::Order::Middle).fixed_pos(screen.min).show(ui.ctx(), |ui| {
            let resp = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0.0, t.colors.scrim.linear_multiply(0.5));
            resp.clicked()
        });
        close |= scrim.inner;
        let recent: Vec<Target> = self.nav.history.recent().collect();
        egui::Area::new(Id::unique("selecteur_rapide")).order(egui::Order::Foreground).anchor(Align2::CENTER_TOP, egui::vec2(0.0, screen.height() * 0.15)).show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                let s = &mut self.nav.switcher;
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut s.query)
                        .id(Id::unique("selecteur_rapide_texte"))
                        .hint_text(nt::SWITCHER_HINT.get(lang))
                        .font(FontId::proportional(t.typography.title))
                        .desired_width(f32::INFINITY),
                );
                super::a11y::name(ui.ctx(), edit.id, nt::SWITCHER_HINT.get(lang));
                if std::mem::take(&mut s.focus) {
                    edit.request_focus();
                }
                s.refresh(&recent);
                ui.add_space(t.spacing.item_y);
                if s.hits.is_empty() {
                    ui.label(RichText::new(nt::NO_MATCH.get(lang)).color(t.colors.muted));
                } else {
                    let view_h = row_h * VISIBLE_ROWS.min(s.hits.len() as f32);
                    let mut area = egui::ScrollArea::vertical().id_salt("selecteur_lignes").max_height(view_h).auto_shrink([false, true]);
                    if std::mem::take(&mut s.reveal) {
                        // Ligne choisie au clavier : gardée dans la vue.
                        let top = s.selected as f32 * row_h;
                        let offset = ui.data(|d| d.get_temp::<f32>(Id::unique("selecteur_decalage"))).unwrap_or(0.0);
                        let offset = if top < offset { top } else if top + row_h > offset + view_h { top + row_h - view_h } else { offset };
                        area = area.vertical_scroll_offset(offset.max(0.0));
                    }
                    let out = area.show_rows(ui, row_h, s.hits.len(), |ui, range| {
                        for i in range {
                            let Some(c) = s.hits.get(i).and_then(|k| s.cands.get(*k)) else { continue };
                            if row(ui, &t, lang, c, i == s.selected, row_h).clicked() {
                                chosen = Some(c.target);
                            }
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(Id::unique("selecteur_decalage"), out.state.clamped_offset().y));
                }
                ui.add_space(t.spacing.item_y);
                super::settings::note(ui, &t, nt::SWITCHER_TIP.get(lang));
            });
        });
        // « > » : la suite se cherche dans la palette de commandes (plugin, lot E2).
        if self.palette_from_switcher() {
            return;
        }
        if let Some(target) = chosen {
            self.choose(target);
        } else if close {
            self.nav.switcher.open = false;
        }
    }
}

/// Une ligne : symbole, nom, contexte (serveur, salon parent…).
fn row(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, c: &Candidate, selected: bool, h: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, t.r_row(), t.colors.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    focus_ring(ui, t, &resp);
    let side = Tokens::icon_side(t.typography.body);
    let icon_rect = egui::Rect::from_center_size(egui::pos2(rect.left() + t.spacing.item_x + side / 2.0, rect.center().y), egui::Vec2::splat(side));
    let color = if selected { t.colors.text_strong } else { t.colors.faint };
    let drawn = match c.kind {
        Kind::Text => Some(Icon::Hash),
        Kind::Announcement => Some(Icon::Megaphone),
        Kind::Forum => Some(Icon::Forum),
        Kind::Thread => Some(Icon::Thread),
        Kind::Voice => Some(Icon::Speaker),
        _ => None,
    };
    let name = match c.kind {
        Kind::Guild => words::guild_name(lang, &c.name),
        Kind::Dm | Kind::Group => words::dm_name(lang, &c.name),
        _ => c.name.as_str(),
    };
    match drawn {
        Some(i) => icons::paint(i, p, icon_rect, color),
        None => {
            let glyph = if c.kind == Kind::Guild { initials(name) } else { "@".to_string() };
            let size = if c.kind == Kind::Guild { t.typography.small } else { t.typography.body };
            p.text(icon_rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(size), color);
        }
    }
    let context = match c.kind {
        Kind::Guild => nt::KIND_GUILD.get(lang),
        Kind::Dm => nt::KIND_DM.get(lang),
        Kind::Group => nt::KIND_GROUP.get(lang),
        Kind::Friend if c.context.is_empty() => nt::KIND_FRIEND.get(lang),
        _ => c.context.as_str(),
    };
    let left = icon_rect.right() + t.spacing.item_x;
    let inner = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2((rect.right() - t.spacing.item_x).max(left), rect.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let strong = if selected { t.colors.text_strong } else { t.colors.text };
    child.add(egui::Label::new(RichText::new(name).color(strong)).truncate().selectable(false));
    if !context.is_empty() {
        child.add(egui::Label::new(RichText::new(context).small().color(t.colors.faint)).truncate().selectable(false));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    resp
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, frame, key};
    use super::*;

    #[test]
    fn ctrl_k_opens_types_and_enter_goes_there() {
        let mut a = app();
        frame(&mut a, Vec::new());
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        frame(&mut a, vec![key(Key::K, ctrl)]);
        assert!(a.nav.switcher.open);
        // Vide : le salon ouvert est la destination récente, en tête.
        assert_eq!(a.nav.switcher.selected_target(), Some(Target::Channel { guild: Some(9), id: 10 }));
        frame(&mut a, vec![egui::Event::Text("acc".into())]);
        assert_eq!(a.nav.switcher.query, "acc", "le champ a le clavier");
        assert_eq!(a.nav.switcher.rows(), vec![(Kind::Text, "accueil")]);
        frame(&mut a, vec![key(Key::Enter, Modifiers::NONE)]);
        assert!(!a.nav.switcher.open);
        assert_eq!((a.store.guild, a.store.channel), (Some(8), Some(20)));
    }

    #[test]
    fn prefixes_arrows_and_escape() {
        let mut a = app();
        frame(&mut a, Vec::new());
        a.open_switcher();
        a.switcher_type("*");
        assert_eq!(a.nav.switcher.rows().iter().map(|r| r.0).collect::<Vec<_>>(), vec![Kind::Guild, Kind::Guild]);
        a.switcher_type("@");
        assert_eq!(a.nav.switcher.rows(), vec![(Kind::Dm, "Alice")]);
        a.switcher_type("!");
        assert!(a.nav.switcher.rows().is_empty());
        // Un salon vocal du serveur : « ! » le trouve, et la recherche sans préfixe aussi.
        let room = crate::state::voice::VoiceChannel { id: 30, guild_id: 9, name: "Salon vocal".into(), stage: false, position: 0, parent: None, overwrites: Some(Vec::new()), user_limit: 0 };
        a.store.apply(crate::state::Event::Voice(crate::state::voice::VoiceEvent::Guild { guild_id: 9, channels: vec![room], states: Vec::new() }));
        a.open_switcher();
        a.switcher_type("!");
        assert_eq!(a.nav.switcher.rows(), vec![(Kind::Voice, "Salon vocal")]);
        a.switcher_type("!voc");
        assert_eq!(a.nav.switcher.selected_target(), Some(Target::Channel { guild: Some(9), id: 30 }));
        a.switcher_type("salon v");
        assert_eq!(a.nav.switcher.rows(), vec![(Kind::Voice, "Salon vocal")]);
        a.switcher_type("#salon v");
        assert!(a.nav.switcher.rows().is_empty(), "« # » ne montre pas les salons vocaux");
        a.switcher_type("!voc");
        frame(&mut a, vec![key(Key::Enter, Modifiers::NONE)]);
        assert_eq!((a.store.guild, a.store.channel), (Some(9), Some(30)), "Entrée ouvre le salon vocal");
        a.open(Some(9), 10);
        a.open_switcher();
        a.switcher_type("#");
        frame(&mut a, Vec::new());
        let first = a.nav.switcher.selected_target();
        frame(&mut a, vec![key(Key::ArrowDown, Modifiers::NONE)]);
        assert_ne!(a.nav.switcher.selected_target(), first);
        frame(&mut a, vec![key(Key::ArrowUp, Modifiers::NONE)]);
        assert_eq!(a.nav.switcher.selected_target(), first);
        frame(&mut a, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!a.nav.switcher.open);
        assert_eq!(a.store.channel, Some(10), "rien d'ouvert");
    }
}
