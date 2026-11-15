//! Palette de commandes (plugin du lot E2) : Ctrl+Maj+P, ou « > » dans le
//! sélecteur rapide. Recherche floue dans toutes les actions de
//! l'application, avec leur raccourci à droite.
//!
//! Les actions viennent d'un registre unique, [`PROVIDERS`] : une fonction
//! par module, qui ajoute ses [`Command`] à la liste. Elles sont réunies à
//! l'ouverture de la palette (jamais à chaque image), la recherche n'est
//! refaite que lorsque le texte change, et seules les lignes visibles sont
//! dessinées. Une action passe toujours par le même chemin que son bouton :
//! ce qui demande une confirmation la demande ici aussi.
//!
//! Ajouter des actions (`docs/plugins.md`, « Palette de commandes ») :
//!
//! ```ignore
//! impl App {
//!     pub(super) fn mon_module_commands(&self, out: &mut Vec<Command>) {
//!         out.push(Command::new(wt::G_ACTION, "Faire ceci", |app| app.faire_ceci()));
//!     }
//! }
//! // puis une ligne dans `PROVIDERS` : `App::mon_module_commands`.
//! ```

use super::keys::{action_label, chord_label, focus_ring};
use super::App;
use crate::i18n::table::e2::{f as wf, t as wt};
use crate::i18n::Text;
use crate::plugins::{self, Id as Plugin};
use crate::state::keys::{Action, Chord};
use crate::state::nav::{self, Kind, Place, Target};
use crate::theme::{MessageMode, Preset, Tokens};
use eframe::egui::{self, Align2, FontId, Id, Key, Modifiers, RichText, Sense};

/// Lignes visibles à la fois, au plus.
const VISIBLE_ROWS: f32 = 10.0;
/// Résultats gardés, au plus.
const MAX_HITS: usize = 200;

/// Une action de la palette.
pub(super) struct Command {
    /// Ce que l'utilisateur lit et cherche.
    pub title: String,
    /// Famille, montrée en gris après le titre.
    pub group: Text,
    /// Raccourci, déjà écrit pour l'utilisateur (vide : aucun).
    pub chord: String,
    /// Titre sans casse ni accents, pour la recherche (fait à l'ouverture).
    folded: String,
    run: Box<dyn Fn(&mut App)>,
}

impl Command {
    pub(super) fn new(group: Text, title: impl Into<String>, run: impl Fn(&mut App) + 'static) -> Command {
        Command { title: title.into(), group, chord: String::new(), folded: String::new(), run: Box::new(run) }
    }

    pub(super) fn chord(mut self, chord: String) -> Command {
        self.chord = chord;
        self
    }
}

/// Ce qu'un module ajoute à la palette, d'après l'état du moment.
pub(super) type Provider = fn(&App, &mut Vec<Command>);

/// Le registre des actions : une ligne par module. L'ordre est celui de la
/// palette quand rien n'est tapé.
pub(super) const PROVIDERS: &[Provider] = &[
    App::palette_key_actions,
    App::palette_view,
    App::tabs_commands,
    App::panes_commands,
    App::zoom_commands,
    App::palette_settings,
    App::palette_themes,
    App::palette_status,
    App::palette_plugins,
    App::palette_accounts,
    App::palette_places,
];

#[derive(Default)]
pub(super) struct Palette {
    pub(super) open: bool,
    query: String,
    /// Texte pour lequel `hits` est à jour.
    searched: Option<String>,
    cmds: Vec<Command>,
    hits: Vec<usize>,
    selected: usize,
    reveal: bool,
    focus: bool,
}

impl Palette {
    fn refresh(&mut self) {
        if self.searched.as_deref() == Some(self.query.as_str()) {
            return;
        }
        let q = nav::fold(self.query.trim());
        self.hits = if q.is_empty() {
            (0..self.cmds.len().min(MAX_HITS)).collect()
        } else {
            let words: Vec<&str> = q.split_whitespace().collect();
            // Chaque mot doit se trouver dans le titre, dans n'importe quel ordre.
            let rank = |c: &Command| words.iter().try_fold(0i32, |sum, w| nav::score(w, &c.folded).map(|s| sum + s));
            let mut scored: Vec<(i32, usize)> = self.cmds.iter().enumerate().filter_map(|(i, c)| rank(c).map(|s| (s, i))).collect();
            // À score égal, l'ordre du registre.
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            scored.into_iter().take(MAX_HITS).map(|(_, i)| i).collect()
        };
        self.searched = Some(self.query.clone());
        self.selected = 0;
        self.reveal = true;
    }
}

/// « Ctrl+Maj+P », écrit dans la langue.
fn ctrl_shift(lang: crate::i18n::Lang, key: &str) -> String {
    chord_label(lang, &Chord { ctrl: true, alt: false, shift: true, key: key.into() })
}

pub(super) fn ctrl(lang: crate::i18n::Lang, key: &str) -> String {
    chord_label(lang, &Chord { ctrl: true, alt: false, shift: false, key: key.into() })
}

impl App {
    fn palette_on(&self) -> bool {
        plugins::on(Plugin::CommandPalette) && self.online()
    }

    /// Ouvre la palette : actions réunies une fois, recherche vide.
    pub fn open_palette(&mut self) {
        if !self.palette_on() {
            return;
        }
        let mut cmds = Vec::new();
        for provide in PROVIDERS {
            provide(self, &mut cmds);
        }
        for c in &mut cmds {
            c.folded = nav::fold(&c.title);
        }
        self.nav.switcher.open = false;
        self.nav.shortcuts_open = false;
        self.palette = Palette { open: true, cmds, focus: true, ..Palette::default() };
        self.palette.refresh();
        self.ctx.request_repaint();
    }

    fn close_palette(&mut self) {
        self.palette = Palette::default();
        self.ctx.request_repaint();
    }

    /// Texte de la palette, comme une saisie (tests, aperçu).
    pub fn palette_type(&mut self, text: &str) {
        self.palette.query = text.to_string();
        self.palette.refresh();
    }

    /// Titres des lignes affichées (tests).
    pub fn palette_rows(&self) -> Vec<&str> {
        self.palette.hits.iter().filter_map(|i| self.palette.cmds.get(*i)).map(|c| c.title.as_str()).collect()
    }

    /// Lance la ligne `row` des résultats, puis ferme la palette.
    fn palette_run(&mut self, row: usize) {
        let Some(i) = self.palette.hits.get(row).copied() else { return };
        let mut cmds = std::mem::take(&mut self.palette.cmds);
        self.close_palette();
        if i < cmds.len() {
            let c = cmds.swap_remove(i);
            (c.run)(self);
            self.flush_commands();
        }
    }

    /// Ctrl+Maj+P : ouvre ou ferme la palette. Plugin coupé : la touche n'est pas prise.
    pub(super) fn palette_keys(&mut self, ui: &egui::Ui) {
        if !self.palette_on() || !ui.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::P)) {
            return;
        }
        if self.palette.open {
            self.close_palette();
        } else {
            self.open_palette();
        }
    }

    /// « > » tapé dans le sélecteur rapide : la suite se cherche dans la palette.
    pub(super) fn palette_from_switcher(&mut self) -> bool {
        if !self.nav.switcher.open || !self.nav.switcher.query.starts_with('>') || !self.palette_on() {
            return false;
        }
        let rest = self.nav.switcher.query[1..].trim_start().to_string();
        self.open_palette();
        self.palette_type(&rest);
        true
    }

    pub(super) fn palette_ui(&mut self, ui: &mut egui::Ui) {
        if !self.palette.open {
            return;
        }
        let (esc, up, down, enter) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::Escape), i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Enter)));
        if esc {
            self.close_palette();
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        {
            let s = &mut self.palette;
            let n = s.hits.len();
            if n > 0 && (up || down) {
                s.selected = if down { (s.selected + 1) % n } else { (s.selected + n - 1) % n };
                s.reveal = true;
            }
        }
        if enter {
            self.palette_run(self.palette.selected);
            return;
        }
        let screen = ui.ctx().content_rect();
        let width = (screen.width() - 2.0 * t.spacing.card_pad).clamp(1.0, t.layout.card_width * 1.5);
        let row_h = t.spacing.channel_row + t.spacing.item_y;
        let frame = t.window_frame().inner_margin(t.toast_margin());
        let mut chosen = None;
        // Voile derrière la fenêtre : un clic dessus la ferme.
        let scrim = egui::Area::new(Id::unique("palette_voile")).order(egui::Order::Middle).fixed_pos(screen.min).show(ui.ctx(), |ui| {
            let resp = ui.allocate_rect(screen, Sense::click());
            ui.painter().rect_filled(screen, 0.0, t.colors.scrim.linear_multiply(0.5));
            resp.clicked()
        });
        let close = scrim.inner;
        egui::Area::new(Id::unique("palette")).order(egui::Order::Foreground).anchor(Align2::CENTER_TOP, egui::vec2(0.0, screen.height() * 0.15)).show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                let s = &mut self.palette;
                let edit = ui.add(egui::TextEdit::singleline(&mut s.query).id(Id::unique("palette_texte")).hint_text(wt::PALETTE_HINT.get(lang)).font(FontId::proportional(t.typography.title)).desired_width(f32::INFINITY));
                super::a11y::name(ui.ctx(), edit.id, wt::PALETTE_HINT.get(lang));
                if std::mem::take(&mut s.focus) {
                    edit.request_focus();
                }
                s.refresh();
                ui.add_space(t.spacing.item_y);
                if s.hits.is_empty() {
                    ui.label(RichText::new(wt::PALETTE_NONE.get(lang)).color(t.colors.muted));
                } else {
                    // Fenêtre basse : moins de lignes, la palette ne sort jamais de l'écran.
                    let rows = (screen.height() * 0.55 / row_h).floor().clamp(3.0, VISIBLE_ROWS);
                    let view_h = row_h * rows.min(s.hits.len() as f32);
                    let mut area = egui::ScrollArea::vertical().id_salt("palette_lignes").max_height(view_h).auto_shrink([false, true]);
                    if std::mem::take(&mut s.reveal) {
                        // Ligne choisie au clavier : gardée dans la vue.
                        let top = s.selected as f32 * row_h;
                        let offset = ui.data(|d| d.get_temp::<f32>(Id::unique("palette_decalage"))).unwrap_or(0.0);
                        let offset = if top < offset { top } else if top + row_h > offset + view_h { top + row_h - view_h } else { offset };
                        area = area.vertical_scroll_offset(offset.max(0.0));
                    }
                    let out = area.show_rows(ui, row_h, s.hits.len(), |ui, range| {
                        for i in range {
                            let Some(c) = s.hits.get(i).and_then(|k| s.cmds.get(*k)) else { continue };
                            if row(ui, &t, lang, c, i == s.selected, row_h).clicked() {
                                chosen = Some(i);
                            }
                        }
                    });
                    ui.data_mut(|d| d.insert_temp(Id::unique("palette_decalage"), out.state.clamped_offset().y));
                }
                ui.add_space(t.spacing.item_y);
                super::settings::note(ui, &t, wt::PALETTE_TIP.get(lang));
            });
        });
        if let Some(i) = chosen {
            self.palette_run(i);
        } else if close {
            self.close_palette();
        }
    }
}

/// Une ligne : titre, famille en gris, raccourci à droite.
fn row(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, c: &Command, selected: bool, h: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, t.r_row(), t.colors.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    focus_ring(ui, t, &resp);
    let pad = t.spacing.item_x;
    let mut right = rect.right() - pad;
    if !c.chord.is_empty() {
        let g = p.layout_no_wrap(c.chord.clone(), super::fonts::demi_font(t.typography.small), t.colors.muted);
        let at = egui::pos2(right - g.size().x, rect.center().y - g.size().y / 2.0);
        right = at.x - pad;
        p.galley(at, g, t.colors.muted);
    }
    let left = rect.left() + pad;
    let inner = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(right.max(left), rect.bottom()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::left_to_right(egui::Align::Center)));
    let strong = if selected { t.colors.text_strong } else { t.colors.text };
    child.add(egui::Label::new(RichText::new(&c.title).color(strong)).truncate().selectable(false));
    child.add(egui::Label::new(RichText::new(c.group.get(lang)).small().color(t.colors.faint)).truncate().selectable(false));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &c.title));
    resp
}

/// Ce que la palette propose d'elle-même. Les autres modules ajoutent les
/// leurs de la même façon (`tabs_commands`, `zoom_commands`…).
impl App {
    /// Les actions des raccourcis clavier, avec leur combinaison.
    fn palette_key_actions(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        let ringing = !self.ringing_calls().is_empty();
        for (a, c) in self.nav.bindings.iter() {
            // Gestes sur un message sélectionné, appui-pour-parler : sans objet ici.
            // « Paramètres » : avec les pages des paramètres, plus bas.
            if a.on_message() || a == Action::PushToTalk || a == Action::Settings || (a.on_ring() && !ringing) {
                continue;
            }
            let group = match a {
                Action::ToggleMute | Action::ToggleDeafen | Action::AnswerCall | Action::DeclineCall | Action::StartCall => wt::G_VOICE,
                Action::PrevChannel | Action::NextChannel | Action::PrevUnread | Action::NextUnread | Action::PrevGuild | Action::NextGuild | Action::Back | Action::Forward | Action::QuickSwitcher | Action::Inbox | Action::OldestUnread => wt::G_NAV,
                _ => wt::G_ACTION,
            };
            let run = move |app: &mut App| app.run_action(a);
            out.push(Command::new(group, action_label(a).get(lang), run).chord(c.map(|c| chord_label(lang, c)).unwrap_or_default()));
        }
    }

    /// Affichage et lecture : pages de l'accueil, mode compact, tout marquer comme lu.
    fn palette_view(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        out.push(Command::new(wt::G_ACTION, wt::CMD_MARK_ALL_READ.get(lang), |app| app.mark_everything_read()));
        out.push(Command::new(wt::G_NAV, wt::CMD_FRIENDS.get(lang), |app| app.go(Place { guild: None, channel: None })));
        out.push(Command::new(wt::G_NAV, wt::CMD_QUESTS.get(lang), |app| {
            app.go(Place { guild: None, channel: None });
            app.show_quests(false);
        }));
        out.push(Command::new(wt::G_VIEW, wt::CMD_COMPACT.get(lang), |app| {
            let m = &mut app.look.edited.layout.messages;
            *m = if *m == MessageMode::Compact { MessageMode::Grouped } else { MessageMode::Compact };
        }));
        out.push(Command::new(wt::G_ACCOUNT, wt::CMD_LOGOUT.get(lang), |app| app.request_logout()));
    }

    fn palette_settings(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        for page in super::settings::pages() {
            // Ctrl+, ouvre la page Apparence.
            let chord = if page == super::settings::Page::Appearance { self.nav.bindings.get(Action::Settings).map(|c| chord_label(lang, c)).unwrap_or_default() } else { String::new() };
            out.push(Command::new(wt::G_SETTINGS, wf::settings_page(lang, page.title().get(lang)), move |app| app.show_settings(page)).chord(chord));
        }
    }

    fn palette_themes(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        for p in Preset::ALL {
            out.push(Command::new(wt::G_THEME, wf::theme(lang, p.label(lang)), move |app| app.look.edited = p.tokens()));
        }
    }

    fn palette_status(&self, out: &mut Vec<Command>) {
        use crate::state::usersettings::schema::{Key as SKey, SValue};
        let lang = self.lang;
        for (code, label) in super::settings::synced::STATUSES {
            out.push(Command::new(wt::G_STATUS, wf::status(lang, label.get(lang)), move |app| {
                if let Err(r) = app.store.account.write(SKey::Status, SValue::Str(code.to_string())) {
                    app.refused(r);
                }
            }));
        }
    }

    fn palette_plugins(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        for p in plugins::ALL {
            let on = self.local.prefs.plugins.on(p.id);
            let title = if on { wf::plugin_off(lang, p.name.get(lang)) } else { wf::plugin_on(lang, p.name.get(lang)) };
            let id = p.id;
            out.push(Command::new(wt::G_PLUGIN, title, move |app| app.set_plugin(id, !on)));
        }
    }

    /// Les autres comptes enregistrés.
    fn palette_accounts(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        let current = self.current_account();
        for a in self.prefs.now.accounts.iter().filter(|a| Some(&a.key) != current) {
            let Some(name) = a.name.as_deref() else { continue };
            let key = a.key.clone();
            out.push(Command::new(wt::G_ACCOUNT, wf::account(lang, name), move |app| app.switch_account(key.clone())));
        }
    }

    /// Salons, fils, serveurs, conversations : les destinations du sélecteur rapide.
    fn palette_places(&self, out: &mut Vec<Command>) {
        let lang = self.lang;
        for c in nav::candidates(&self.store) {
            let name = match c.kind {
                Kind::Guild => super::words::guild_name(lang, &c.name).to_string(),
                Kind::Dm | Kind::Group => format!("@{}", super::words::dm_name(lang, &c.name)),
                Kind::Friend => format!("@{}", c.name),
                _ if c.context.is_empty() => format!("#{}", c.name),
                _ => format!("#{} ({})", c.name, c.context),
            };
            let target: Target = c.target;
            out.push(Command::new(wt::G_NAV, wf::go_to(lang, name), move |app| app.go_target(target)));
        }
    }

    /// « Marquer tout comme lu » : chaque serveur, puis les conversations non lues.
    fn mark_everything_read(&mut self) {
        let guilds: Vec<u64> = self.store.guilds.iter().map(|g| g.id).collect();
        for g in guilds {
            self.store.mark_guild_read(g);
        }
        let dms: Vec<u64> = self.store.dms.iter().map(|d| d.id).filter(|c| self.store.unread().contains(c)).collect();
        for c in dms {
            self.store.mark_read(c);
        }
    }
}

/// Actions des onglets (lot E2), ajoutées à la palette.
impl App {
    pub(super) fn tabs_commands(&self, out: &mut Vec<Command>) {
        if !plugins::on(Plugin::ChannelTabs) || self.mobile_active() {
            return;
        }
        let lang = self.lang;
        if self.tabs_shown() {
            out.push(Command::new(wt::G_NAV, wt::CMD_TAB_NEXT.get(lang), |app| app.tab_cycle(true)).chord(ctrl(lang, "Tab")));
            out.push(Command::new(wt::G_NAV, wt::CMD_TAB_PREV.get(lang), |app| app.tab_cycle(false)).chord(ctrl_shift(lang, "Tab")));
            out.push(Command::new(wt::G_NAV, wt::TAB_CLOSE.get(lang), |app| app.tab_close_active()).chord(ctrl(lang, "W")));
        }
        out.push(Command::new(wt::G_NAV, wt::CMD_TAB_NEW.get(lang), |app| app.tab_new()));
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, frame, key};
    use super::*;

    const CTRL_SHIFT: Modifiers = Modifiers { ctrl: true, command: true, shift: true, alt: false, mac_cmd: false };

    #[test]
    fn ctrl_shift_p_opens_searches_and_runs() {
        let mut a = app();
        frame(&mut a, Vec::new());
        frame(&mut a, vec![key(Key::P, CTRL_SHIFT)]);
        assert!(a.palette.open);
        assert!(!a.actions.pins_open, "Ctrl+Maj+P n'est pas Ctrl+P");
        // Rien de tapé : les actions, dans l'ordre du registre, raccourci compris.
        let mute = a.palette.cmds.iter().find(|c| c.title == "Couper ou rétablir le micro").or_else(|| a.palette.cmds.iter().find(|c| c.chord == "Ctrl+Maj+M")).unwrap_or_else(|| panic!("micro"));
        assert_eq!(mute.chord, "Ctrl+Maj+M");
        frame(&mut a, vec![egui::Event::Text("param apparence".into())]);
        assert_eq!(a.palette.query, "param apparence", "le champ a le clavier");
        assert_eq!(a.palette_rows().first().copied(), Some("Paramètres\u{a0}: Apparence"));
        frame(&mut a, vec![key(Key::Enter, Modifiers::NONE)]);
        assert!(!a.palette.open && a.palette.cmds.is_empty(), "fermée, actions rendues");
        assert!(a.params.open && a.params.page() == super::super::settings::Page::Appearance);
        // Une destination : même effet que le sélecteur rapide.
        a.params.open = false;
        a.open_palette();
        a.palette_type("aller accueil");
        assert_eq!(a.palette_rows().first().copied(), Some("Aller à #accueil (Beta)"));
        a.palette_run(0);
        assert_eq!((a.store.guild, a.store.channel), (Some(8), Some(20)));
        // Échap ferme sans rien lancer.
        a.open_palette();
        frame(&mut a, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!a.palette.open);
        assert_eq!(a.store.channel, Some(20));
    }

    #[test]
    fn switcher_prefix_and_plugin_toggle() {
        let mut a = app();
        frame(&mut a, Vec::new());
        a.open_switcher();
        frame(&mut a, Vec::new());
        frame(&mut a, vec![egui::Event::Text(">compact".into())]);
        assert!(a.palette.open && !a.nav.switcher.open, "« > » : la palette prend la suite");
        assert_eq!(a.palette.query, "compact");
        assert_eq!(a.theme.layout.messages, MessageMode::Grouped);
        a.palette_run(0);
        frame(&mut a, Vec::new());
        assert_eq!(a.theme.layout.messages, MessageMode::Compact, "le mode compact du thème, pas un second");
        // Un plugin se coupe depuis la palette ; coupée elle-même, la touche n'est plus prise.
        a.open_palette();
        a.palette_type("couper plugin palette");
        assert_eq!(a.palette_rows().first().copied(), Some("Couper le plugin «\u{a0}Palette de commandes\u{a0}»"));
        a.palette_run(0);
        assert!(!plugins::on(Plugin::CommandPalette));
        frame(&mut a, vec![key(Key::P, CTRL_SHIFT)]);
        assert!(!a.palette.open);
        a.open_switcher();
        frame(&mut a, Vec::new());
        frame(&mut a, vec![egui::Event::Text(">".into())]);
        assert!(a.nav.switcher.open && !a.palette.open);
    }

    /// Chaque action a un titre dans les deux langues et aucune n'est en double.
    #[test]
    fn registry_titles_are_unique_and_translated() {
        for lang in crate::i18n::Lang::ALL {
            let mut a = app();
            a.lang = lang;
            let mut cmds = Vec::new();
            for provide in PROVIDERS {
                provide(&a, &mut cmds);
            }
            assert!(cmds.len() > 60, "{}", cmds.len());
            let mut seen = std::collections::HashSet::new();
            for c in &cmds {
                assert!(!c.title.trim().is_empty() && !c.group.get(lang).is_empty());
                assert!(seen.insert(c.title.clone()), "en double : {}", c.title);
            }
        }
    }
}
