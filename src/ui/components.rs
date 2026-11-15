//! Composants des messages d'applications (lot T11) : boutons, menus de
//! sélection, composants v2 (conteneur, section, texte, galerie, fichier,
//! séparateur), « X a utilisé /commande », « X réfléchit… », réponse
//! éphémère et son « Ignorer le message ».
//!
//! Dessiné par `message.rs` dans la zone du message : la même mise en page
//! sert à la mesure (rien n'y est cliquable). Un clic devient une
//! [`AppAction`], appliquée au magasin après la liste (`msg_actions.rs`).
//! La hauteur ne dépend d'aucun état passager (un bouton qui attend sa
//! réponse est seulement grisé).

use super::attachments::{self, fit_box, grid_cell, grid_columns, tile_ui, Hot};
use super::markdown::{self, Zones};
use super::msg_actions::{Action, Acts};
use super::App;
use crate::i18n::table::interactions::{f, t as tx};
use crate::i18n::Lang;
use crate::images::Images;
use crate::state::interactions::{Button, ButtonStyle, CommandKind, Component, Select, SelectKind};
use crate::state::markdown::Doc;
use crate::state::model::{ChannelKind, Message};
use crate::state::Store;
use crate::theme::Tokens;
use eframe::egui::{self, pos2, Rect, RichText, Sense, Vec2};
use std::collections::HashMap;

/// Menus à choix multiples ou à recherche en cours, au plus (vidés au-delà).
const MAX_DRAFTS: usize = 16;
/// Entrées montrées au plus dans un menu d'utilisateurs, de rôles ou de salons.
const MAX_ENTRIES: usize = 25;

/// Geste sur un message d'application, appliqué après la liste.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum AppAction {
    /// Bouton ou menu de sélection : interaction de composant (type 3).
    Press { message_id: u64, component_type: u8, custom_id: String, values: Option<Vec<String>> },
    /// « Ignorer le message » d'une réponse éphémère.
    Dismiss(u64),
    /// Commande de menu contextuel d'un message (type 2, cible).
    MessageCommand { message_id: u64, command_id: u64 },
    /// Menu « Applications » ouvert : index des commandes à demander s'il manque.
    WantCommands,
}

/// Choix en cours dans un menu (cases cochées, recherche).
#[derive(Default)]
struct Draft {
    picked: Vec<String>,
    query: String,
}

/// État de l'interface des composants (dans `ActionsUi`).
#[derive(Default)]
pub(super) struct ComponentsUi {
    drafts: HashMap<(u64, String), Draft>,
}

impl ComponentsUi {
    fn draft(&mut self, message_id: u64, custom_id: &str) -> &mut Draft {
        let key = (message_id, custom_id.to_string());
        if !self.drafts.contains_key(&key) && self.drafts.len() >= MAX_DRAFTS {
            self.drafts.clear();
        }
        self.drafts.entry(key).or_default()
    }
}

/// Ce que reçoit le dessin des composants d'un message.
struct Cx<'b> {
    t: &'b Tokens,
    lang: Lang,
    images: Option<&'b Images>,
    doc: &'b Doc,
    /// Rang du prochain texte (dans `Doc::components`).
    text: usize,
}

/// « Moi a utilisé /sondage » au-dessus d'une réponse à une commande.
pub(super) fn header(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message) {
    let Some(i) = m.meta.app.as_deref().and_then(|a| a.interaction.as_ref()) else { return };
    let Some(cmd) = &i.command else { return };
    ui.add(egui::Label::new(RichText::new(f::used(lang, &i.user_name, cmd)).small().color(t.colors.faint)).truncate());
}

/// Composants, attente et mention « éphémère » sous le texte du message.
#[allow(clippy::too_many_arguments)]
pub(super) fn body<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, m: &'a Message, doc: &Doc, links: &mut Zones, hot: &mut Hot<'a>, acts: &mut Acts) {
    let Some(app) = m.meta.app.as_deref() else { return };
    if app.loading() {
        ui.label(RichText::new(f::thinking(lang, &m.author)).italics().color(t.colors.muted));
    }
    let mut cx = Cx { t, lang, images, doc, text: 0 };
    list(ui, &mut cx, m, &app.components, links, hot, acts);
    if app.ephemeral() {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tx::ONLY_YOU.get(lang)).small().color(t.colors.muted));
            let r = ui.add(egui::Label::new(RichText::new(tx::DISMISS.get(lang)).small().color(t.colors.link)).sense(Sense::click()).selectable(false));
            if images.is_some() && r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                acts.st.push(Action::App(AppAction::Dismiss(m.id)));
            }
        });
    }
}

fn list<'a>(ui: &mut egui::Ui, cx: &mut Cx, m: &'a Message, comps: &'a [Component], links: &mut Zones, hot: &mut Hot<'a>, acts: &mut Acts) {
    for c in comps {
        one(ui, cx, m, c, links, hot, acts);
    }
}

fn one<'a>(ui: &mut egui::Ui, cx: &mut Cx, m: &'a Message, c: &'a Component, links: &mut Zones, hot: &mut Hot<'a>, acts: &mut Acts) {
    let t = cx.t;
    match c {
        Component::Row(items) => {
            ui.add_space(t.spacing.media_gap);
            ui.horizontal_wrapped(|ui| {
                for it in items {
                    match it {
                        Component::Button(b) => button(ui, cx, m, b, acts),
                        Component::Select(s) => select(ui, cx, m, s, acts),
                        _ => {}
                    }
                }
            });
        }
        Component::Button(b) => button(ui, cx, m, b, acts),
        Component::Select(s) => select(ui, cx, m, s, acts),
        Component::Text(_) => {
            if let Some(d) = cx.doc.components.get(cx.text) {
                markdown::doc_ui(ui, t, cx.lang, m.id, d, &[], false, links);
            }
            cx.text += 1;
        }
        Component::Section { texts, accessory } => {
            let Some(acc) = accessory else {
                return list(ui, cx, m, texts, links, hot, acts);
            };
            let side = t.layout.embed_thumb.min(ui.available_width() / 3.0).floor();
            let gap = t.spacing.item_x;
            ui.horizontal_top(|ui| {
                let w = (ui.available_width() - side - gap).max(1.0);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    list(ui, cx, m, texts, links, hot, acts);
                });
                match &**acc {
                    Component::Thumbnail(a) => {
                        let size = fit_box(a.width, a.height, Vec2::splat(side), 1.0);
                        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                        tile_ui(ui, t, cx.lang, cx.images, rect, false, attachments::attachment_tile(a), hot);
                    }
                    Component::Button(b) => button(ui, cx, m, b, acts),
                    _ => {}
                }
            });
        }
        Component::Thumbnail(a) => gallery(ui, cx, std::slice::from_ref(&**a), hot),
        Component::Gallery(items) => gallery(ui, cx, items, hot),
        Component::File(a) => {
            ui.add_space(t.spacing.media_gap);
            attachments::file_card(ui, t, cx.lang, cx.images.is_some(), a, hot);
        }
        Component::Separator { divider, large } => {
            let space = t.spacing.media_gap * if *large { 2.0 } else { 1.0 };
            ui.add_space(space);
            if *divider {
                ui.separator();
            }
        }
        Component::Container { accent, children, .. } => {
            ui.add_space(t.spacing.media_gap);
            let width = ui.available_width().min(t.layout.embed_width);
            let bar = t.shapes.embed_bar;
            let mut margin = t.embed_margin();
            margin.left = margin.left.saturating_add(bar.round().clamp(0.0, f32::from(i8::MAX)) as i8);
            let inner_w = (width - margin.sum().x).max(1.0);
            let resp = egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_embed()).inner_margin(margin).show(ui, |ui| {
                ui.set_width(inner_w);
                list(ui, cx, m, children, links, hot, acts);
            });
            let color = accent.map_or(t.colors.border, |c| egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8));
            let r = resp.response.rect;
            let bar_rect = Rect::from_min_max(r.min, pos2(r.left() + bar, r.bottom()));
            ui.painter().with_clip_rect(bar_rect.intersect(ui.clip_rect())).rect_filled(r, t.r_embed(), color);
        }
        // Champs de fenêtre modale : jamais dans un message.
        Component::TextInput(_) | Component::Label { .. } | Component::Unknown => {}
    }
}

/// Images d'une galerie (ou une vignette seule), comme des pièces jointes.
fn gallery<'a>(ui: &mut egui::Ui, cx: &Cx, items: &'a [crate::state::model::Attachment], hot: &mut Hot<'a>) {
    let t = cx.t;
    let gap = t.spacing.media_gap;
    ui.add_space(gap);
    if let [one] = items {
        let max = Vec2::new(ui.available_width().min(t.layout.media_width), t.layout.media_height);
        let size = fit_box(one.width, one.height, max, Tokens::icon_side(t.typography.body) * 2.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        tile_ui(ui, t, cx.lang, cx.images, rect, false, attachments::attachment_tile(one), hot);
        return;
    }
    let cols = grid_columns(items.len());
    let width = ui.available_width().min(t.layout.embed_width);
    let cell = grid_cell(width, cols, gap);
    for (n, row) in items.chunks(cols).enumerate() {
        if n > 0 {
            ui.add_space(gap);
        }
        let (line, _) = ui.allocate_exact_size(Vec2::new(width, cell), Sense::hover());
        for (i, a) in row.iter().enumerate() {
            let rect = Rect::from_min_size(line.min + Vec2::new(i as f32 * (cell + gap), 0.0), Vec2::splat(cell));
            tile_ui(ui, t, cx.lang, cx.images, rect, true, attachments::attachment_tile(a), hot);
        }
    }
}

fn button(ui: &mut egui::Ui, cx: &Cx, m: &Message, b: &Button, acts: &mut Acts) {
    let t = cx.t;
    let (fill, color) = match b.style {
        ButtonStyle::Primary => (t.colors.accent, t.colors.on_accent),
        ButtonStyle::Success => (t.colors.online, t.colors.on_accent),
        ButtonStyle::Danger => (t.danger_fill(), t.colors.on_accent),
        ButtonStyle::Secondary | ButtonStyle::Link | ButtonStyle::Premium => (t.colors.button_bg, t.colors.button_text),
    };
    // Emoji en image (Twemoji ou emoji du serveur), comme ailleurs : son
    // texte, transparent, lui réserve la place ; sinon le glyphe.
    let img = b.emoji.as_ref().and_then(super::emoji::Img::of);
    let mut label = String::new();
    if img.is_none()
        && let Some(e) = &b.emoji
    {
        label = e.display();
    }
    if !b.label.is_empty() {
        if !label.is_empty() || img.is_some() {
            label.push(' ');
        }
        label.push_str(&b.label);
    }
    if b.style == ButtonStyle::Link {
        label.push_str(" ↗");
    }
    let busy = b.custom_id.as_deref().is_some_and(|c| acts.store.interactions.component_busy(m.id, c));
    let enabled = !b.disabled && !busy && b.style != ButtonStyle::Premium && (b.style != ButtonStyle::Link || b.url.is_some());
    let text: egui::WidgetText = match (img, &b.emoji) {
        (Some(img), Some(e)) => {
            let font = egui::TextStyle::Button.resolve(ui.style());
            let row = super::emoji::row_height(ui, &font);
            let side = row.min(font.size * 1.3);
            let mut job = egui::text::LayoutJob::default();
            super::emoji::append(ui, &mut job, 0, &e.display(), &font, side, row, img);
            job.append(&label, 0.0, egui::TextFormat { font_id: font, color, ..Default::default() });
            job.into()
        }
        _ => RichText::new(label).color(color).into(),
    };
    let r = ui.add_enabled(enabled, egui::Button::new(text).fill(fill).corner_radius(t.r_row()));
    if let Some(img) = img {
        let font = egui::TextStyle::Button.resolve(ui.style());
        let side = super::emoji::row_height(ui, &font).min(font.size * 1.3);
        let left = r.rect.left() + ui.spacing().button_padding.x;
        super::emoji::paint(ui, Rect::from_center_size(pos2(left + side / 2.0, r.rect.center().y), Vec2::splat(side)), img);
    }
    if cx.images.is_none() {
        return;
    }
    match b.style {
        ButtonStyle::Link => {
            if let Some(url) = &b.url {
                // L'adresse réelle, visible avant de cliquer.
                if r.on_hover_text(url).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(url));
                }
            }
        }
        // Jamais d'achat depuis Ferrocord.
        ButtonStyle::Premium => {
            r.on_disabled_hover_text(tx::PREMIUM.get(cx.lang));
        }
        _ => {
            if r.clicked()
                && let Some(id) = &b.custom_id
            {
                acts.st.push(Action::App(AppAction::Press { message_id: m.id, component_type: 2, custom_id: id.clone(), values: None }));
            }
        }
    }
}

/// Entrées proposées par un menu : (valeur envoyée, texte montré).
pub(super) fn select_entries(store: &Store, channel_id: u64, s: &Select, query: &str) -> Vec<(String, String)> {
    let q = query.to_lowercase();
    let keep = |name: &str| q.is_empty() || name.to_lowercase().contains(&q);
    let roles = || -> Vec<(String, String)> {
        let g = store.guild_of(channel_id).and_then(|g| store.guilds.iter().find(|x| x.id == g));
        g.map(|g| g.roles.iter().filter(|r| r.id != g.id && keep(&r.name)).map(|r| (r.id.to_string(), format!("@{}", r.name))).collect()).unwrap_or_default()
    };
    let users = || -> Vec<(String, String)> { store.known_people(channel_id).into_iter().filter(|(_, n)| keep(n)).map(|(id, n)| (id.to_string(), n)).collect() };
    let mut out = match s.kind {
        SelectKind::String => s.options.iter().filter(|o| keep(&o.label)).map(|o| (o.value.clone(), option_text(o))).collect(),
        SelectKind::User => users(),
        SelectKind::Role => roles(),
        SelectKind::Mentionable => {
            let mut v = users();
            v.extend(roles());
            v
        }
        SelectKind::Channel => store
            .visible_channels(channel_id)
            .into_iter()
            .filter(|c| s.channel_types.is_empty() || s.channel_types.contains(&channel_code(c.kind)))
            .filter(|c| keep(&c.name))
            .map(|c| (c.id.to_string(), format!("#{}", c.name)))
            .collect(),
    };
    out.truncate(MAX_ENTRIES);
    out
}

fn option_text(o: &crate::state::interactions::SelectOption) -> String {
    match &o.emoji {
        Some(e) => format!("{} {}", e.display(), o.label),
        None => o.label.clone(),
    }
}

/// Type de salon de Discord (pour `channel_types`).
pub(super) fn channel_code(k: ChannelKind) -> u8 {
    match k {
        ChannelKind::Text => 0,
        ChannelKind::Category => 4,
        ChannelKind::Announcement => 5,
        ChannelKind::Forum => 15,
        ChannelKind::Media => 16,
    }
}

fn select(ui: &mut egui::Ui, cx: &Cx, m: &Message, s: &Select, acts: &mut Acts) {
    let t = cx.t;
    let lang = cx.lang;
    let store = acts.store;
    let live = cx.images.is_some();
    let busy = store.interactions.component_busy(m.id, &s.custom_id);
    let defaults: Vec<&str> = s.options.iter().filter(|o| o.default).map(|o| o.label.as_str()).collect();
    let shown = if defaults.is_empty() { s.placeholder.clone().unwrap_or_else(|| tx::CHOOSE.get(lang).to_string()) } else { defaults.join(", ") };
    let width = ui.available_width().min(t.layout.embed_width);
    let multi = s.max_values > 1;
    let mut sent = None;
    ui.add_enabled_ui(live && !s.disabled && !busy, |ui| {
        egui::ComboBox::from_id_salt(("app-select", m.id, &s.custom_id)).width(width).selected_text(RichText::new(shown).color(t.colors.muted)).show_ui(ui, |ui| {
            let draft = acts.st.app.draft(m.id, &s.custom_id);
            if s.kind != SelectKind::String {
                ui.add(egui::TextEdit::singleline(&mut draft.query).hint_text(tx::SEARCH.get(lang)).desired_width(f32::INFINITY));
            }
            for (value, label) in select_entries(store, m.channel_id, s, &draft.query) {
                if multi {
                    let mut on = draft.picked.contains(&value);
                    if ui.checkbox(&mut on, label).changed() {
                        if on {
                            draft.picked.push(value);
                        } else {
                            draft.picked.retain(|v| *v != value);
                        }
                    }
                } else if ui.selectable_label(false, label).clicked() {
                    sent = Some(vec![value]);
                    ui.close();
                }
            }
            if multi {
                let n = draft.picked.len();
                let ok = n >= usize::from(s.min_values) && n <= usize::from(s.max_values);
                if !ok {
                    ui.label(RichText::new(f::pick_between(lang, s.min_values.to_string(), s.max_values.to_string())).small().color(t.colors.faint));
                }
                if ui.add_enabled(ok, egui::Button::new(tx::CONFIRM.get(lang))).clicked() {
                    sent = Some(std::mem::take(&mut draft.picked));
                    ui.close();
                }
            }
        });
    });
    if let Some(values) = sent {
        acts.st.push(Action::App(AppAction::Press { message_id: m.id, component_type: s.kind.code(), custom_id: s.custom_id.clone(), values: Some(values) }));
    }
}

/// Entrée « Applications » du menu d'un message : ses commandes de menu contextuel.
pub(super) fn apps_menu(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts) {
    if m.system.is_some() {
        return;
    }
    let store = acts.store;
    acts.st.push(Action::App(AppAction::WantCommands));
    let cmds = store.app_commands(m.channel_id, CommandKind::Message);
    let loading = store.commands_loading(m.channel_id);
    if cmds.is_empty() && !loading {
        return;
    }
    let mut chosen = None;
    crate::ui::menu::submenu(ui, t, tx::APPS.get(lang), |ui| {
        if cmds.is_empty() {
            ui.label(tx::LOADING_COMMANDS.get(lang));
        }
        for c in &cmds {
            if crate::ui::menu::item(ui, t, &c.cmd.name, None, false).on_hover_text(c.app_name()).clicked() {
                chosen = Some(c.cmd.id);
                ui.close();
            }
        }
    });
    if let Some(command_id) = chosen {
        acts.st.push(Action::App(AppAction::MessageCommand { message_id: m.id, command_id }));
    }
}

impl App {
    pub(super) fn apply_app_action(&mut self, cid: u64, a: AppAction) {
        let no_session = self.store.interactions.session().is_none();
        match a {
            AppAction::Press { message_id, component_type, custom_id, values } => {
                if !self.store.press_component(cid, message_id, component_type, &custom_id, values) && no_session {
                    self.notify(tx::NOT_CONNECTED);
                }
            }
            AppAction::Dismiss(id) => self.store.dismiss_ephemeral(cid, id),
            AppAction::MessageCommand { message_id, command_id } => {
                if !self.store.run_command(cid, command_id, Vec::new(), Vec::new(), Some(message_id)) {
                    self.notify(if no_session { tx::NOT_CONNECTED } else { tx::COMMAND_GONE });
                }
            }
            AppAction::WantCommands => self.store.want_commands(cid),
        }
    }
}
