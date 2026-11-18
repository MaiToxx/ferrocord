//! Page « Plugins » des Paramètres (lot P0) : les fonctions propres à
//! Ferrocord, en cartes à interrupteur, avec recherche, filtre par catégorie,
//! compteur et « Tout désactiver ». Les réglages d'un plugin s'ouvrent dans
//! une fenêtre à pied, dessinée d'après leur déclaration (`crate::plugins`).
//!
//! C'est aussi ici que l'état des plugins est publié pour `plugins::on`
//! ([`App::plugins_sync`]) et qu'un plugin coupé rend ce qu'il tenait
//! ([`App::plugin_changed`]).

use super::settings::shell;
use super::widgets::{self, Kind as Button};
use super::{fonts, motion, App};
use crate::i18n::table::plug::{f as pf, t as pt};
use crate::i18n::Lang;
use crate::plugins::{self, Category, Id, Kind, Plugin, PluginPrefs, Setting, Value};
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, pos2, vec2, Align, Layout, Rect, RichText, Sense, Stroke};

/// Largeur de page à partir de laquelle les cartes vont par deux.
const TWO_COLUMNS: f32 = 520.0;
/// Lignes de description montrées sur une carte (le reste : au survol).
const ABOUT_ROWS: usize = 3;
/// Hauteur d'une pastille de catégorie.
const CHIP: f32 = 28.0;
/// Largeur du contenu de la fenêtre de réglages.
const MODAL_WIDTH: f32 = 440.0;

/// Fenêtre de réglages d'un plugin : brouillon, écrit au clic sur « Enregistrer ».
struct Editing {
    id: Id,
    title: String,
    settings: &'static [Setting],
    draft: PluginPrefs,
}

/// État de la page, et ce qui a été publié pour `plugins::on`.
pub(in crate::ui) struct PluginsUi {
    search: String,
    category: Option<Category>,
    editing: Option<Editing>,
    confirm_all: bool,
    /// États lus au lancement (`local.toml`) : ceux des plugins à redémarrage.
    pub(in crate::ui) boot: Option<u64>,
    published: (u64, PluginPrefs),
}

impl Default for PluginsUi {
    fn default() -> Self {
        // Une application neuve part des valeurs par défaut, quoi qu'ait
        // publié une autre avant elle sur ce fil (tests).
        let prefs = PluginPrefs::default();
        plugins::publish(prefs.bits(), &prefs);
        PluginsUi { search: String::new(), category: None, editing: None, confirm_all: false, boot: None, published: (prefs.bits(), prefs) }
    }
}

impl PluginsUi {
    /// Une fenêtre de la page est ouverte : Échap la ferme, pas les Paramètres.
    pub(in crate::ui) fn modal_open(&self) -> bool {
        self.editing.is_some() || self.confirm_all
    }
}

/// Le plugin répond à la recherche (`query` déjà pliée) et au filtre.
fn matches(p: &Plugin, lang: Lang, query: &str, category: Option<Category>) -> bool {
    category.is_none_or(|c| c == p.category) && (query.is_empty() || super::settings::fold(p.name.get(lang)).contains(query) || super::settings::fold(p.about.get(lang)).contains(query))
}

/// Catégories qui ont au moins un plugin, dans l'ordre de la page.
fn categories(all: &[Plugin]) -> impl Iterator<Item = Category> + '_ {
    Category::ALL.into_iter().filter(|c| all.iter().any(|p| p.category == *c))
}

/// Pastille de filtre ; `true` si elle est cliquée.
pub(super) fn chip(ui: &mut egui::Ui, t: &Tokens, text: &str, selected: bool) -> bool {
    let ink = if selected { t.colors.on_accent } else { t.colors.button_text };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), fonts::medium_font(t.typography.button), ink);
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 2.0 * t.spacing.control_x.min(14.0), CHIP), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, selected, text));
    if ui.is_rect_visible(rect) {
        let k = motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
        let fill = if selected { t.colors.accent } else { motion::mix(t.colors.button_bg, t.colors.control_bg, k) };
        ui.painter().rect_filled(rect, CHIP / 2.0, fill);
        ui.painter().galley(rect.center() - galley.size() / 2.0, galley, ink);
    }
    super::keys::focus_ring(ui, t, &resp);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Ce qu'un clic sur une carte demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CardAction {
    Toggle(bool),
    Settings,
}

/// Hauteur d'une carte : nom, description sur [`ABOUT_ROWS`] lignes, pied.
fn card_height(ui: &egui::Ui, t: &Tokens) -> f32 {
    let small = ui.text_style_height(&egui::TextStyle::Small);
    2.0 * card_pad(t) + shell_switch_height() + t.spacing.item_y + ABOUT_ROWS as f32 * small + t.spacing.item_x + small
}

fn card_pad(t: &Tokens) -> f32 {
    (2.0 * t.spacing.item_x).max(8.0)
}

/// Hauteur de la rangée du nom : celle de l'interrupteur.
fn shell_switch_height() -> f32 {
    24.0
}

/// Carte d'un plugin, large de `width`. `pending` : son changement attend un
/// redémarrage ; `unavailable` : ce système ne permet pas la fonction.
#[allow(clippy::too_many_arguments)]
fn card(ui: &mut egui::Ui, t: &Tokens, lang: Lang, p: &Plugin, on: bool, pending: bool, unavailable: bool, width: f32) -> Option<CardAction> {
    let (rect, hover) = ui.allocate_exact_size(vec2(width, card_height(ui, t)), Sense::hover());
    let about = p.about.get(lang);
    hover.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, about));
    if !ui.is_rect_visible(rect) {
        return None;
    }
    let name = p.name.get(lang);
    let pad = card_pad(t);
    let inner = rect.shrink(pad);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, t.r_card(), t.colors.bg1);
    painter.rect_stroke(rect, t.r_card(), if on { Stroke::new(t.shapes.border.max(1.0), motion::mix(t.colors.border, t.colors.online, 0.55)) } else { t.border() }, egui::StrokeKind::Inside);

    // Rangée du nom : interrupteur au bout, roue des réglages devant lui.
    let mut action = None;
    let head = Rect::from_min_max(inner.min, pos2(inner.right(), inner.top() + shell_switch_height()));
    let mut controls = ui.new_child(egui::UiBuilder::new().id_salt(p.key).max_rect(head).layout(Layout::right_to_left(Align::Center)));
    let mut value = on;
    let switch = shell::switch(&mut controls, &mut value);
    super::a11y::name(ui.ctx(), switch.id, name);
    if switch.changed() {
        action = Some(CardAction::Toggle(value));
    }
    if !p.settings.is_empty() {
        let tip = pf::settings_of(lang, name);
        let gear = controls.add_enabled_ui(on, |ui| super::util::icon_button(ui, t, Icon::Gear, &tip)).inner;
        if gear.clicked() {
            action = Some(CardAction::Settings);
        }
    }
    let name_w = (controls.min_rect().left() - inner.left() - t.spacing.item_x).max(1.0);
    let ink = if on { t.colors.text_strong } else { t.colors.text };
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_owned(), fonts::demi_font(t.typography.body), ink);
    job.wrap = egui::text::TextWrapping { max_width: name_w, max_rows: 1, break_anywhere: true, ..Default::default() };
    let galley = painter.layout_job(job);
    painter.galley(pos2(inner.left(), head.center().y - galley.size().y / 2.0), galley, ink);

    // Description, bornée ; entière au survol si elle est coupée.
    let small = egui::FontId::proportional(t.typography.small);
    let mut job = egui::text::LayoutJob::simple(about.to_owned(), small.clone(), t.colors.muted, inner.width());
    job.wrap.max_rows = ABOUT_ROWS;
    let galley = painter.layout_job(job);
    let cut = galley.elided;
    painter.galley(pos2(inner.left(), head.bottom() + t.spacing.item_y), galley, t.colors.muted);
    if cut {
        hover.on_hover_text(about);
    }

    // Pied : catégorie à gauche, « Redémarrage nécessaire » à droite.
    painter.text(inner.left_bottom(), egui::Align2::LEFT_BOTTOM, p.category.name().get(lang), small.clone(), t.colors.faint);
    if p.restart || unavailable {
        // « Non disponible » passe devant « Redémarrage nécessaire ».
        let (label, tip) = if unavailable { (pt::UNAVAILABLE, pt::UNAVAILABLE_TIP) } else { (pt::RESTART, pt::RESTART_TIP) };
        let ink = if pending || unavailable { t.colors.idle } else { t.colors.faint };
        let text = painter.layout_no_wrap(label.get(lang).to_owned(), small, ink);
        let side = text.size().y;
        let at = pos2(inner.right() - text.size().x, inner.bottom() - text.size().y);
        let dot = Rect::from_center_size(pos2(at.x - side * 0.6, at.y + side / 2.0), vec2(side * 0.45, side * 0.45));
        painter.circle_filled(dot.center(), dot.width() / 2.0, ink);
        let zone = Rect::from_min_max(pos2(dot.left(), at.y), inner.max);
        painter.galley(at, text, ink);
        ui.interact(zone, ui.scope_id().with(("redemarrage", p.key)), Sense::hover()).on_hover_text(tip.get(lang));
    }
    action
}

/// Contrôle d'un réglage déclaré, dans la fenêtre de réglages.
fn setting_row(ui: &mut egui::Ui, t: &Tokens, lang: Lang, id: Id, s: &Setting, draft: &mut PluginPrefs) {
    let (label, detail) = (s.label.get(lang), s.detail.get(lang));
    let current = draft.value(id, s);
    match (s.kind, current) {
        (Kind::Bool(_), Value::Bool(mut v)) => shell::settings_row(ui, label, detail, true, |ui| {
            let resp = shell::switch(ui, &mut v);
            super::a11y::name(ui.ctx(), resp.id, label);
            if resp.changed() {
                draft.put_declared(id, s, Value::Bool(v));
            }
        }),
        (Kind::Choice(options, _), Value::Str(key)) => shell::settings_row(ui, label, detail, false, |ui| {
            let shown = options.iter().find(|(k, _)| *k == key).map_or("", |(_, l)| l.get(lang));
            let combo = egui::ComboBox::from_id_salt(("plugin-choix", s.key)).selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for (k, l) in options {
                    if ui.selectable_label(*k == key, l.get(lang)).clicked() {
                        draft.put_declared(id, s, Value::Str((*k).to_string()));
                    }
                }
            });
            super::a11y::name(ui.ctx(), combo.response.id, label);
        }),
        (Kind::Number { min, max, .. }, Value::Int(mut n)) => shell::settings_row(ui, label, detail, false, |ui| {
            let resp = ui.add(egui::Slider::new(&mut n, min..=max));
            super::a11y::name(ui.ctx(), resp.id, label);
            if resp.changed() {
                draft.put_declared(id, s, Value::Int(n));
            }
        }),
        (Kind::Text { max, .. }, Value::Str(mut text)) => {
            ui.add_space(t.spacing.item_y);
            widgets::field_label(ui, t, label);
            let resp = super::form::field(ui, t, &mut text, "", max);
            super::a11y::name(ui.ctx(), resp.id, label);
            if resp.changed() {
                draft.put_declared(id, s, Value::Str(text));
            }
            if !detail.is_empty() {
                super::settings::note(ui, t, detail);
            }
        }
        // Valeur d'un autre type que la déclaration : `put_declared` ne la garde jamais.
        _ => {}
    }
}

impl App {
    /// Publie l'état des plugins s'il a changé (états, réglages), et prévient
    /// chaque plugin qui vient d'être activé ou coupé. Rien sinon.
    pub(in crate::ui) fn plugins_sync(&mut self) {
        let prefs: &PluginPrefs = &self.local.prefs.plugins;
        let bits = plugins::effective(plugins::ALL, prefs.bits(), self.local.plugins.boot);
        let (was, old) = &self.local.plugins.published;
        if bits == *was && prefs == old {
            return;
        }
        let was = *was;
        self.local.plugins.published = (bits, prefs.clone());
        plugins::publish(bits, &self.local.prefs.plugins);
        // Mode discret (lot E3) : ses réglages sont relus ici, pas à chaque image.
        self.stealth_apply();
        for p in plugins::ALL {
            let mask = 1u64 << p.id as u8;
            if (was ^ bits) & mask != 0 {
                self.plugin_changed(p.id, bits & mask != 0);
            }
        }
        self.ctx.request_repaint();
    }

    /// Un plugin vient d'être activé ou coupé : ce qu'il tient encore est
    /// rendu ici (lecture en cours, fil, fenêtre). Ce qui se lit à chaque
    /// image par `plugins::on` n'a rien à faire ici.
    fn plugin_changed(&mut self, id: Id, on: bool) {
        // L'historique est effacé par `offline_tick`, le tube fermé par `rpc_tick`,
        // l'image de fond et les polices rendues par `look_tick` : rien à faire ici pour eux.
        if (id, on) == (Id::ReadAloud, false) {
            crate::platform::speech::stop();
        }
        // L'enregistrement d'un appel s'arrête seul (`voice_rec_tick`), l'incrustation se ferme de même.
        // Journal technique du rapport de plantage : la clé du plugin, écrite dans le code.
        crate::crash::note_n(plugins::get(id).key, i64::from(on));
        if id == Id::CrashReport {
            self.crash_plugin_changed(on);
        }
    }

    /// Active ou coupe un plugin, et l'enregistre.
    pub fn set_plugin(&mut self, id: Id, on: bool) {
        let was = self.local.prefs.plugins.on(id);
        let mut p = self.local.prefs.clone();
        p.plugins.set(id, on);
        self.set_local_prefs(p);
        // Activé à la main : l'avertissement de l'enregistrement d'appel (lot E4).
        if id == Id::CallRecorder && on && !was {
            self.voice_rec_enabled();
        }
    }

    /// Aperçu hors ligne : fenêtre de réglages ouverte sur un exemple des
    /// quatre formes de réglage (rien n'en est lu ailleurs).
    #[doc(hidden)]
    pub fn plugins_preview_settings(&mut self) {
        const SIDES: &[(&str, crate::i18n::Text)] = &[("tous", pt::CAT_ALL), ("messages", pt::CAT_MESSAGES), ("voix", pt::CAT_VOICE)];
        const DEMO: &[Setting] = &[
            Setting { key: "annoncer_auteur", label: pt::SPEAK_AUTHOR, detail: pt::SPEAK_AUTHOR_TIP, kind: Kind::Bool(true) },
            Setting { key: "exemple_choix", label: pt::CAT_NAVIGATION, detail: pt::RESTART_TIP, kind: Kind::Choice(SIDES, "messages") },
            Setting { key: "exemple_nombre", label: pt::CAT_VOICE, detail: pt::SPEAK_AUTHOR_TIP, kind: Kind::Number { min: 50, max: 200, default: 100 } },
            Setting { key: "exemple_texte", label: pt::CAT_SYSTEM, detail: pt::SEARCH, kind: Kind::Text { default: "", max: 40 } },
        ];
        let title = pf::settings_of(self.lang, pt::SPEAK.get(self.lang));
        self.local.plugins.editing = Some(Editing { id: Id::ReadAloud, title, settings: DEMO, draft: (*self.local.prefs.plugins).clone() });
    }

    /// Ouvre la fenêtre de réglages d'un plugin (aperçu hors ligne).
    #[doc(hidden)]
    pub fn plugin_settings_open(&mut self, id: Id) {
        let p = plugins::get(id);
        let title = pf::settings_of(self.lang, p.name.get(self.lang));
        self.local.plugins.editing = Some(Editing { id, title, settings: p.settings, draft: (*self.local.prefs.plugins).clone() });
    }

    pub(in crate::ui) fn page_plugins(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let all = plugins::ALL;
        let prefs: PluginPrefs = (*self.local.prefs.plugins).clone();
        let boot = self.local.plugins.boot;
        let waits = |p: &Plugin| p.restart && boot.is_some_and(|b| (b & (1 << p.id as u8) != 0) != prefs.on(p.id));

        super::settings::note(ui, t, pt::INTRO.get(lang));
        ui.add_space(t.spacing.category_gap);
        let search = widgets::field(ui, t, &mut self.local.plugins.search, pt::SEARCH.get(lang));
        super::a11y::name(ui.ctx(), search.id, pt::SEARCH.get(lang));
        ui.add_space(t.spacing.item_x);

        // Catégories.
        let mut category = self.local.plugins.category;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(t.spacing.item_x, t.spacing.item_x);
            if chip(ui, t, pt::CAT_ALL.get(lang), category.is_none()) {
                category = None;
            }
            for c in categories(all) {
                if chip(ui, t, c.name().get(lang), category == Some(c)) {
                    category = Some(c);
                }
            }
        });
        self.local.plugins.category = category;
        ui.add_space(t.spacing.item_x);

        // Compteur et « Tout désactiver ».
        let count = prefs.count();
        ui.horizontal(|ui| {
            ui.set_min_height(widgets::button_height(t));
            ui.label(RichText::new(pf::enabled(lang, count, all.len())).family(fonts::medium()).color(t.colors.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled_ui(count > 0, |ui| widgets::button(ui, t, Button::Secondary, pt::DISABLE_ALL.get(lang))).inner.clicked() {
                    self.local.plugins.confirm_all = true;
                }
            });
        });
        if all.iter().any(waits) {
            ui.add_space(t.spacing.item_y);
            ui.label(RichText::new(pt::RESTART_PENDING.get(lang)).color(t.colors.idle));
        }
        ui.add_space(t.spacing.item_x);

        // Cartes.
        let query = super::settings::fold(self.local.plugins.search.trim());
        let shown: Vec<&Plugin> = all.iter().filter(|p| matches(p, lang, &query, category)).collect();
        if shown.is_empty() {
            ui.add_space(t.spacing.category_gap);
            ui.label(RichText::new(pt::NONE_FOUND.get(lang)).color(t.colors.muted));
        }
        let gap = card_pad(t) * 0.75;
        let full = ui.available_width();
        let columns = if full >= TWO_COLUMNS { 2 } else { 1 };
        let width = ((full - gap * (columns - 1) as f32) / columns as f32).floor();
        let mut action = None;
        let mica_unavailable = self.mica_unavailable();
        for row in shown.chunks(columns) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for p in row {
                    if let Some(a) = card(ui, t, lang, p, prefs.on(p.id), waits(p), p.id == Id::Mica && mica_unavailable, width) {
                        action = Some((*p, a));
                    }
                }
            });
            ui.add_space(gap);
        }
        match action {
            Some((p, CardAction::Toggle(on))) => self.set_plugin(p.id, on),
            Some((p, CardAction::Settings)) => {
                let title = pf::settings_of(lang, p.name.get(lang));
                self.local.plugins.editing = Some(Editing { id: p.id, title, settings: p.settings, draft: prefs.clone() });
            }
            None => {}
        }
        self.plugin_settings_modal(ui.ctx(), t);
        self.plugins_confirm_modal(ui.ctx(), t);
    }

    /// Fenêtre de réglages du plugin en cours d'édition.
    fn plugin_settings_modal(&mut self, ctx: &egui::Context, t: &Tokens) {
        let lang = self.lang;
        let Some(mut e) = self.local.plugins.editing.take() else { return };
        let (mut save, mut cancel, mut defaults) = (false, false, false);
        let out = widgets::modal(
            ctx,
            t,
            egui::Id::unique("plugin-reglages"),
            MODAL_WIDTH,
            &e.title,
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                shell::page_style(ui, t);
                for s in e.settings {
                    setting_row(ui, t, lang, e.id, s, &mut e.draft);
                }
                // Traduction (lot E3) : état de l'adresse saisie, clé d'API (jamais dans `local.toml`).
                if e.id == Id::Translate {
                    self.translate_settings_extra(ui, t, &e.draft);
                }
            },
            |ui| {
                save = widgets::button(ui, t, Button::Primary, crate::i18n::t::SAVE.get(lang)).clicked();
                cancel = widgets::button(ui, t, Button::Link, crate::i18n::t::CANCEL.get(lang)).clicked();
                defaults = widgets::button(ui, t, Button::Link, pt::DEFAULTS.get(lang)).clicked();
            },
        );
        if defaults {
            for s in e.settings {
                e.draft.clear(e.id, s.key);
            }
        }
        if save {
            let mut p = self.local.prefs.clone();
            p.plugins = Box::new(e.draft);
            self.set_local_prefs(p);
        } else if !(cancel || out.closed) {
            self.local.plugins.editing = Some(e);
        }
    }

    /// « Tout désactiver » : deuxième clic, dans une fenêtre (l'historique hors ligne est effacé).
    fn plugins_confirm_modal(&mut self, ctx: &egui::Context, t: &Tokens) {
        if !self.local.plugins.confirm_all {
            return;
        }
        let lang = self.lang;
        let (mut yes, mut no) = (false, false);
        let out = widgets::modal(
            ctx,
            t,
            egui::Id::unique("plugins-tout-desactiver"),
            MODAL_WIDTH,
            pt::DISABLE_ALL_TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.add(egui::Label::new(RichText::new(pt::DISABLE_ALL_BODY.get(lang)).color(t.colors.text)).wrap());
            },
            |ui| {
                yes = widgets::button(ui, t, Button::Danger, pt::DISABLE_ALL.get(lang)).clicked();
                no = widgets::button(ui, t, Button::Link, crate::i18n::t::CANCEL.get(lang)).clicked();
            },
        );
        if yes {
            let mut p = self.local.prefs.clone();
            p.plugins.disable_all();
            self.set_local_prefs(p);
        }
        if yes || no || out.closed {
            self.local.plugins.confirm_all = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::settings::Page;
    use super::super::tests::{find, offline_boot};
    use super::*;

    struct Offline;
    impl super::super::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            false
        }
        fn poll(&mut self) -> Vec<crate::state::Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    /// Assez haut pour que toute la colonne des paramètres soit à l'écran.
    const SCREEN: egui::Vec2 = vec2(1280.0, 1900.0);

    /// Un texte affiché (libellé d'egui : sa valeur ; élément nommé : son nom) contient `text`.
    fn shows(out: &egui::FullOutput, text: &str) -> bool {
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        update.nodes.iter().any(|(_, n)| n.label().is_some_and(|l| l.contains(text)) || n.value().is_some_and(|v| v.contains(text)))
    }

    /// Plugins activés tant que l'utilisateur n'a rien changé.
    fn on_by_default() -> usize {
        plugins::ALL.iter().filter(|p| p.default_on).count()
    }

    fn app() -> App {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Offline), crate::state::Store::new(), images));
        app.lang = Lang::Fr;
        app
    }

    fn run(app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, SCREEN)), events, ..Default::default() };
        let ctx = app.ctx.clone();
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
        out
    }

    fn settle(app: &mut App) -> egui::FullOutput {
        for _ in 0..4 {
            run(app, Vec::new());
        }
        run(app, Vec::new())
    }

    fn click_at(app: &mut App, at: egui::Pos2) -> egui::FullOutput {
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        run(app, vec![egui::Event::PointerMoved(at), button(true)]);
        run(app, vec![button(false)]);
        settle(app)
    }

    fn click(app: &mut App, label: &str) -> egui::FullOutput {
        let out = settle(app);
        let at = find(&out, label).unwrap_or_else(|| panic!("« {label} » introuvable")).center();
        click_at(app, at)
    }

    /// La page liste chaque plugin, la colonne des paramètres y mène, et
    /// l'interrupteur d'une carte coupe vraiment le plugin.
    #[test]
    fn page_lists_plugins_and_a_card_switch_turns_one_off() {
        let mut app = app();
        app.show_settings(Page::MyAccount);
        let out = click(&mut app, pt::PAGE.get(Lang::Fr));
        assert_eq!(app.params.page(), Page::Plugins);
        // Les premières cartes tiennent dans la fenêtre d'essai ; les suivantes
        // sont sous le pli (la liste s'allonge à chaque lot) et ne sont donc pas
        // cherchées à l'écran.
        for p in plugins::ALL.iter().take(8) {
            assert!(find(&out, p.name.get(Lang::Fr)).is_some(), "{}", p.key);
            assert!(shows(&out, p.about.get(Lang::Fr)), "{}", p.key);
        }
        for p in plugins::ALL {
            assert!(!p.name.get(Lang::Fr).is_empty() && !p.about.get(Lang::Fr).is_empty(), "{}", p.key);
        }
        assert!(shows(&out, &pf::enabled(Lang::Fr, on_by_default(), plugins::ALL.len())));
        assert!(plugins::on(Id::Backdrop));
        let out = click(&mut app, pt::BACKDROP.get(Lang::Fr));
        assert!(!plugins::on(Id::Backdrop) && !app.local_prefs().plugins.on(Id::Backdrop));
        assert!(plugins::on(Id::Animations), "les autres restent activés");
        assert!(shows(&out, &pf::enabled(Lang::Fr, on_by_default() - 1, plugins::ALL.len())));
    }

    #[test]
    fn search_and_category_narrow_the_list() {
        let fr = Lang::Fr;
        let hit = |q: &str, c| plugins::ALL.iter().filter(|p| matches(p, fr, &super::super::settings::fold(q), c)).map(|p| p.id).collect::<Vec<_>>();
        assert_eq!(hit("", None).len(), plugins::ALL.len());
        assert_eq!(hit("voix haute", None), vec![Id::ReadAloud]);
        assert_eq!(hit("degrade", None), vec![Id::Backdrop, Id::ShareThemes], "description, sans accent");
        assert!(hit("", Some(Category::Appearance)).contains(&Id::Animations) && !hit("", Some(Category::Appearance)).contains(&Id::ReadAloud));
        assert!(hit("zzz", None).is_empty());
        let cats: Vec<Category> = categories(plugins::ALL).collect();
        assert!(cats.contains(&Category::Privacy) && cats.contains(&Category::Voice));
        assert!(Category::ALL.iter().all(|c| cats.contains(c) == plugins::ALL.iter().any(|p| p.category == *c)), "seules les catégories peuplées sont proposées");

        let mut app = app();
        app.show_settings(Page::Plugins);
        app.local.plugins.search = "zzz".into();
        let out = settle(&mut app);
        assert!(shows(&out, pt::NONE_FOUND.get(fr)) && !shows(&out, pt::BACKDROP_ABOUT.get(fr)));
        app.local.plugins.search.clear();
        let out = click(&mut app, pt::CAT_MESSAGES.get(fr));
        assert!(shows(&out, pt::SPEAK_ABOUT.get(fr)) && !shows(&out, pt::BACKDROP_ABOUT.get(fr)));
    }

    /// « Tout désactiver » demande une confirmation, puis coupe tout ; Échap
    /// dans la fenêtre ne ferme pas les Paramètres.
    #[test]
    fn disable_all_asks_first() {
        let fr = Lang::Fr;
        let mut app = app();
        app.show_settings(Page::Plugins);
        let out = click(&mut app, pt::DISABLE_ALL.get(fr));
        assert!(shows(&out, pt::DISABLE_ALL_TITLE.get(fr)));
        assert_eq!(app.local_prefs().plugins.count(), on_by_default(), "rien avant la confirmation");
        run(&mut app, vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
        let out = settle(&mut app);
        assert!(app.params.open && !shows(&out, pt::DISABLE_ALL_TITLE.get(fr)));
        click(&mut app, pt::DISABLE_ALL.get(fr));
        let out = settle(&mut app);
        // Deux boutons du même nom : celui de la fenêtre est le plus bas.
        let update = out.platform_output.accesskit_update.as_ref().unwrap();
        let confirm = update.nodes.iter().filter(|(_, n)| n.label() == Some(pt::DISABLE_ALL.get(fr))).filter_map(|(_, n)| n.bounds()).map(|b| pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32)).fold(None, |best: Option<egui::Pos2>, p| Some(best.map_or(p, |b| if p.y > b.y { p } else { b }))).unwrap();
        click_at(&mut app, confirm);
        assert_eq!(app.local_prefs().plugins.count(), 0);
        assert!(plugins::ALL.iter().all(|p| !plugins::on(p.id)));
    }

    /// La roue ouvre les réglages déclarés ; « Enregistrer » écrit le brouillon.
    #[test]
    fn settings_modal_edits_declared_values() {
        let fr = Lang::Fr;
        let mut app = app();
        app.show_settings(Page::Plugins);
        let gear = pf::settings_of(fr, pt::SPEAK.get(fr));
        let out = click(&mut app, &gear);
        assert!(find(&out, pt::SPEAK_AUTHOR.get(fr)).is_some(), "réglage déclaré dessiné");
        assert!(plugins::flag(Id::ReadAloud, plugins::READ_ALOUD_AUTHOR));
        click(&mut app, pt::SPEAK_AUTHOR.get(fr));
        assert!(plugins::flag(Id::ReadAloud, plugins::READ_ALOUD_AUTHOR), "brouillon : rien d'écrit avant « Enregistrer »");
        click(&mut app, crate::i18n::t::SAVE.get(fr));
        assert!(!plugins::flag(Id::ReadAloud, plugins::READ_ALOUD_AUTHOR));
        assert!(!app.local_prefs().plugins.flag(Id::ReadAloud, plugins::READ_ALOUD_AUTHOR));
        assert!(app.local.plugins.editing.is_none());
    }

    /// Les quatre formes de réglage se dessinent d'après leur déclaration.
    #[test]
    fn every_kind_of_setting_is_drawn() {
        let fr = Lang::Fr;
        let mut app = app();
        app.show_settings(Page::Plugins);
        app.plugins_preview_settings();
        let out = settle(&mut app);
        for label in [pt::SPEAK_AUTHOR, pt::CAT_NAVIGATION, pt::CAT_VOICE, pt::CAT_SYSTEM] {
            assert!(find(&out, label.get(fr)).is_some(), "{}", label.get(fr));
        }
    }

    /// Plugins réellement coupés : plus de dégradé peint, plus d'animation, et tout revient en les rallumant.
    #[test]
    fn disabled_plugins_stop_their_feature() {
        let mut app = app();
        let mut p = app.local_prefs();
        p.look.gradient.on = true;
        app.set_local_prefs(p);
        settle(&mut app);
        assert!(!app.look_paints_nothing(), "dégradé peint, plugin activé");
        assert_eq!(app.local.motion_level(), motion::Level::Full);
        app.set_plugin(Id::Backdrop, false);
        app.set_plugin(Id::Animations, false);
        settle(&mut app);
        assert!(app.look_paints_nothing(), "plugin coupé : rien n'est peint");
        assert_eq!(app.local.motion_level(), motion::Level::None);
        assert!(app.local_prefs().look.gradient.on, "le réglage est gardé");
        app.set_plugin(Id::Backdrop, true);
        app.set_plugin(Id::Animations, true);
        settle(&mut app);
        assert!(!app.look_paints_nothing());
        assert_eq!(app.local.motion_level(), motion::Level::Full);
    }

    /// Un plugin à redémarrage garde l'état du lancement tant qu'on n'a pas relancé.
    #[test]
    fn boot_state_is_published_for_restart_plugins() {
        let mut app = app();
        app.set_plugin(Id::Animations, false);
        assert!(!plugins::on(Id::Animations), "plugin ordinaire : tout de suite");
        // Aucun plugin du registre ne demande de redémarrage aujourd'hui : la règle est dans `effective`.
        app.local.plugins.boot = Some(plugins::DEFAULT_BITS);
        app.set_plugin(Id::Animations, true);
        assert!(plugins::on(Id::Animations));
    }
}
