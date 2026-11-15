//! Colonne de navigation des Paramètres, à la façon de Discord : en-tête du
//! compte (avatar, nom, « Modifier les profils »), champ de recherche à
//! loupe, entrées précédées d'une icône et rangées par groupes, entrée
//! ouverte dépliée en sous-entrées le long d'un trait vertical, pied de
//! version.
//!
//! Une sous-entrée mène à une page (« Compte » réunit Mon compte, Profils,
//! Statut…) ou à une section de la page (repère de défilement, `shell`).
//! Rien n'est mémorisé sur le dépliage : l'entrée ouverte est celle de la
//! page affichée, d'où qu'on l'ait ouverte.

use super::{shell, Page};
use crate::i18n::table::audio::t as au;
use crate::i18n::table::install::t as it;
use crate::i18n::table::s::t as st;
use crate::i18n::table::s2::t as s2;
use crate::i18n::{Lang, Text};
use crate::theme::{icons, icons::Icon, Tokens};
use crate::ui::App;
use eframe::egui::{self, Color32, Rect, RichText, Sense};

/// Sous-entrée d'une entrée dépliée.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Sub {
    pub label: Text,
    pub page: Page,
    /// Titre de la section visée dans la page ; `None` : le haut de la page.
    pub anchor: Option<Text>,
}

/// Entrée de la colonne.
#[derive(Clone, Copy, Debug)]
pub(in crate::ui) struct Entry {
    pub icon: Icon,
    /// Libellé propre à la colonne ; `None` : le titre de la page.
    pub title: Option<Text>,
    /// Page ouverte par l'entrée.
    pub page: Page,
    pub subs: &'static [Sub],
    /// Pastille à droite du libellé (« NOUVEAU » dans Discord).
    pub badge: Option<Text>,
}

const fn entry(icon: Icon, page: Page, subs: &'static [Sub]) -> Entry {
    Entry { icon, title: None, page, subs, badge: None }
}

/// Sous-entrée qui ouvre une autre page.
const fn to(page: Page, label: Text) -> Sub {
    Sub { label, page, anchor: None }
}

/// Sous-entrée qui mène à la section `title` de `page`.
const fn at(page: Page, title: Text) -> Sub {
    Sub { label: title, page, anchor: Some(title) }
}

impl Entry {
    pub fn title(&self) -> Text {
        self.title.unwrap_or(self.page.title())
    }

    /// La page `page` est celle de l'entrée ou d'une de ses sous-entrées.
    pub fn holds(&self, page: Page) -> bool {
        self.page == page || self.subs.iter().any(|s| s.page == page)
    }
}


/// Groupes de la colonne, dans l'ordre de Discord ; `None` : groupe sans titre.
pub(in crate::ui) const GROUPS: &[(Option<Text>, &[Entry])] = &[
    (
        None,
        &[
            Entry {
                icon: Icon::User,
                title: Some(st::NAV_ACCOUNT),
                page: Page::MyAccount,
                subs: &[
                    to(Page::MyAccount, st::SUB_ACCOUNT_INFO),
                    to(Page::Profile, st::PAGE_PROFILE),
                    to(Page::Status, st::PAGE_STATUS),
                    Sub { label: st::SUB_PASSWORD, page: Page::MyAccount, anchor: Some(st::IN_DISCORD_ONLY) },
                    to(Page::Devices, st::PAGE_DEVICES),
                    to(Page::Connections, st::PAGE_CONNECTIONS),
                ],
                badge: None,
            },
            entry(Icon::ShieldLock, Page::DataPrivacy, &[at(Page::DataPrivacy, s2::SEC_DATA_USE), at(Page::DataPrivacy, crate::i18n::table::offline::t::SECTION)]),
            entry(
                Icon::ChatShield,
                Page::Privacy,
                &[at(Page::Privacy, st::SECTION_DMS), at(Page::Privacy, st::FRIEND_REQUESTS), at(Page::Privacy, st::EXPLICIT_FILTER), at(Page::Privacy, crate::i18n::table::nd::t::SAFETY)],
            ),
            entry(
                Icon::Bell,
                Page::Notifications,
                &[
                    at(Page::Notifications, crate::i18n::table::notify::t::SECTION_DESKTOP),
                    at(Page::Notifications, crate::i18n::table::journal::t::KW_SECTION),
                    at(Page::Notifications, st::SERVER_NOTIFICATIONS),
                    at(Page::Notifications, st::SECTION_ACCOUNT_NOTIFICATIONS),
                ],
            ),
        ],
    ),
    (Some(st::GROUP_BILLING), &[entry(Icon::Nitro, Page::Billing, &[])]),
    (
        Some(st::GROUP_EXPERIENCE),
        &[
            entry(
                Icon::Mic,
                Page::Voice,
                &[at(Page::Voice, au::SEC_VOICE), at(Page::Voice, au::SEC_TEST), at(Page::Voice, au::SEC_MODE), at(Page::Voice, au::SEC_PROCESSING), at(Page::Voice, au::SEC_VIDEO), at(Page::Voice, au::SEC_SHARE)],
            ),
            entry(Icon::Palette, Page::Appearance, &[]),
            entry(Icon::Accessibility, Page::Accessibility, &[at(Page::Accessibility, s2::SEC_MOTION), at(Page::Accessibility, s2::SEC_SCROLL), at(Page::Accessibility, s2::SEC_COLORS), at(Page::Accessibility, s2::SEC_TEXT)]),
            entry(
                Icon::Media,
                Page::TextImages,
                &[at(Page::TextImages, st::SECTION_MEDIA), at(Page::TextImages, st::SECTION_EMBEDS), at(Page::TextImages, st::SECTION_EMOJI), at(Page::TextImages, st::SECTION_COMMANDS)],
            ),
            entry(Icon::Keyboard, Page::Keybinds, &[at(Page::Keybinds, crate::i18n::table::nav::t::SHORTCUTS_EDITABLE), at(Page::Keybinds, crate::i18n::table::lotn::t::FIXED_TITLE)]),
            entry(Icon::Language, Page::Language, &[]),
            entry(Icon::Folder, Page::Folders, &[]),
            entry(Icon::Screen, Page::Streamer, &[]),
            entry(Icon::Sliders, Page::Advanced, &[at(Page::Advanced, s2::SEC_IDENTITY), at(Page::Advanced, s2::SEC_DEBUG), at(Page::Advanced, crate::i18n::table::ff::t::TRUSTED_DOMAINS)]),
        ],
    ),
    (
        Some(st::GROUP_GAMES),
        &[
            entry(
                Icon::Gamepad,
                Page::Activity,
                &[
                    at(Page::Activity, crate::i18n::table::activity::t::SEC_DETECTED),
                    at(Page::Activity, crate::i18n::table::activity::t::SEC_ADDED),
                    at(Page::Activity, crate::i18n::table::rpc::t::SECTION),
                ],
            ),
            entry(Icon::Apps, Page::Apps, &[]),
        ],
    ),
    (
        Some(crate::i18n::table::plug::t::GROUP),
        &[
            entry(Icon::Gear, Page::Ferrocord, &[at(Page::Ferrocord, st::SAVED_ACCOUNTS), at(Page::Ferrocord, it::SEC_STARTUP), at(Page::Ferrocord, it::SEC_LINKS), at(Page::Ferrocord, it::SEC_UPDATES)]),
            entry(Icon::Puzzle, Page::Plugins, &[]),
            entry(Icon::Info, Page::About, &[at(Page::About, st::UNOFFICIAL), at(Page::About, s2::SEC_LICENSES), at(Page::About, s2::SEC_ATTRIBUTIONS)]),
        ],
    ),
];

/// Toutes les entrées, dans l'ordre de la colonne.
pub(in crate::ui) fn entries() -> impl Iterator<Item = &'static Entry> {
    GROUPS.iter().flat_map(|(_, entries)| entries.iter())
}

/// Toutes les pages, dans l'ordre de la colonne : celle de l'entrée, puis
/// celles de ses sous-entrées.
pub(in crate::ui) fn pages() -> impl Iterator<Item = Page> {
    entries().flat_map(|e| std::iter::once(e.page).chain(e.subs.iter().filter(move |s| s.anchor.is_none() && s.page != e.page).map(|s| s.page)))
}

/// Entrée ou sous-entrée trouvée par la recherche.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) struct Hit {
    pub label: Text,
    pub page: Page,
    pub anchor: Option<Text>,
}

/// Recherche de la colonne : les entrées et les sous-entrées dont le libellé
/// contient `query`, puis les réglages de l'index ([`super::search`]).
pub(in crate::ui) fn search(lang: Lang, query: &str) -> Vec<Hit> {
    let q = super::fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    let hit = |label: Text| super::fold(label.get(lang)).contains(&q);
    let mut out: Vec<Hit> = Vec::new();
    for e in entries() {
        if hit(e.title()) {
            out.push(Hit { label: e.title(), page: e.page, anchor: None });
        }
        out.extend(e.subs.iter().filter(|s| hit(s.label)).map(|s| Hit { label: s.label, page: s.page, anchor: s.anchor }));
    }
    for (page, label) in super::search(lang, query) {
        if !out.iter().any(|h| h.page == page && h.label.get(lang) == label.get(lang)) {
            out.push(Hit { label, page, anchor: None });
        }
    }
    out
}

/// Géométrie des lignes, tirée des jetons du thème.
struct Metrics {
    /// Hauteur d'une entrée, d'une sous-entrée.
    row: f32,
    sub_row: f32,
    /// Marge à gauche et à droite du contenu d'une ligne.
    pad: f32,
    /// Côté de l'icône, et son écart au libellé.
    icon: f32,
    gap: f32,
}

fn metrics(t: &Tokens) -> Metrics {
    Metrics { row: t.field_height(), sub_row: t.spacing.channel_row, pad: t.spacing.item_x + t.spacing.item_y, icon: Tokens::icon_side(t.typography.body * 1.2), gap: t.spacing.item_x + t.spacing.item_y }
}

/// Ligne d'une entrée : icône, libellé (sur deux lignes s'il est long),
/// pastille éventuelle. `tint` : couleur imposée (« Déconnexion »). Sert aussi
/// aux colonnes des paramètres de serveur et de salon (mêmes lignes, sans
/// sous-entrées).
pub(in crate::ui) fn entry_row(ui: &mut egui::Ui, t: &Tokens, icon: Icon, label: &str, badge: Option<&str>, tint: Option<Color32>, selected: bool) -> egui::Response {
    let m = metrics(t);
    let width = ui.available_width();
    let chip = badge.map(|b| ui.painter().layout_no_wrap(b.to_uppercase(), crate::ui::fonts::demi_font((t.typography.small * 0.85).round()), t.colors.on_accent));
    let chip_w = chip.as_ref().map_or(0.0, |g| g.size().x + 2.0 * t.spacing.item_y + m.gap);
    let text_w = (width - 2.0 * m.pad - m.icon - m.gap - chip_w).max(1.0);
    let galley = ui.painter().layout(label.to_owned(), crate::ui::fonts::demi_font(t.typography.body), Color32::PLACEHOLDER, text_w);
    let height = m.row.max(galley.size().y + 2.0 * t.spacing.item_y);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus();
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, t.r_row(), t.colors.selected);
        } else if hot {
            p.rect_filled(rect, t.r_row(), t.colors.row_hover);
        }
        let color = tint.unwrap_or(if selected || hot { t.colors.text_strong } else { t.colors.muted });
        let at = crate::ui::util::pixel_square(ui, egui::pos2(rect.left() + m.pad, rect.center().y - m.icon / 2.0), m.icon);
        icons::paint(icon, p, at, color);
        p.galley(egui::pos2(at.right() + m.gap, rect.top() + Tokens::centered_top(height, galley.size().y)), galley, color);
        if let Some(g) = chip {
            let size = g.size() + egui::vec2(2.0 * t.spacing.item_y, t.spacing.item_y);
            let pill = Rect::from_center_size(egui::pos2(rect.right() - m.pad - size.x / 2.0, rect.center().y), size);
            p.rect_filled(pill, size.y / 2.0, t.colors.accent);
            p.galley(pill.center() - g.size() / 2.0, g, t.colors.on_accent);
        }
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, selected, label));
    crate::ui::keys::focus_ring(ui, t, &resp);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Sous-entrée : libellé aligné sur ceux des entrées, le long d'un trait
/// vertical passant sous les icônes ; `current` : en clair, avec sa barre.
fn sub_row(ui: &mut egui::Ui, t: &Tokens, label: &str, current: bool) -> egui::Response {
    let m = metrics(t);
    let width = ui.available_width();
    let left = m.pad + m.icon + m.gap;
    let galley = ui.painter().layout(label.to_owned(), crate::ui::fonts::demi_font(t.typography.body), Color32::PLACEHOLDER, (width - left - m.pad).max(1.0));
    let height = m.sub_row.max(galley.size().y + 2.0 * t.spacing.item_y);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus();
        let p = ui.painter();
        // Le trait court d'une sous-entrée à l'autre, par-dessus l'écart entre les lignes.
        let x = (rect.left() + m.pad + m.icon / 2.0).round();
        let w = t.shapes.border.max(1.0);
        let gap = ui.spacing().item_spacing.y;
        p.rect_filled(Rect::from_min_max(egui::pos2(x - w / 2.0, rect.top() - gap), egui::pos2(x + w / 2.0, rect.bottom())), 0.0, t.colors.border);
        if current {
            let bar = Rect::from_center_size(egui::pos2(x, rect.center().y), egui::vec2(w + 1.0, (height * 0.6).round()));
            p.rect_filled(bar, bar.width() / 2.0, t.colors.text_strong);
        }
        let color = if current || hot { t.colors.text_strong } else { t.colors.muted };
        p.galley(egui::pos2(rect.left() + left, rect.top() + Tokens::centered_top(height, galley.size().y)), galley, color);
    }
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, current, label));
    crate::ui::keys::focus_ring(ui, t, &resp);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Champ de recherche : haut, arrondi, loupe à gauche.
fn search_field(ui: &mut egui::Ui, t: &Tokens, text: &mut String, hint: &str) -> egui::Response {
    let m = metrics(t);
    let size = egui::vec2(ui.available_width(), t.field_height() + 2.0 * t.spacing.item_y);
    let margin = |v: f32| v.round().clamp(0.0, 127.0) as i8;
    let edit = egui::TextEdit::singleline(text)
        .hint_text(RichText::new(hint).color(t.colors.faint))
        .font(egui::FontId::proportional(t.typography.body))
        .margin(egui::Margin { left: margin(m.pad + m.icon + m.gap), right: margin(m.pad), top: 0, bottom: 0 })
        .align(egui::Align2::LEFT_CENTER);
    let resp = ui.add_sized(size, edit);
    let at = crate::ui::util::pixel_square(ui, egui::pos2(resp.rect.left() + m.pad, resp.rect.center().y - m.icon / 2.0), m.icon);
    icons::paint(Icon::Search, ui.painter(), at, t.colors.muted);
    resp
}

/// Ligne de version du pied : « Ferrocord 0.1.0 (commit) • windows x86_64 ».
fn version_line() -> &'static str {
    static LINE: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let commit = super::pages::COMMIT.map(|c| format!(" ({})", c.get(..7).unwrap_or(c))).unwrap_or_default();
        format!("Ferrocord {}{commit} \u{2022} {} {}", env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH)
    });
    &LINE
}

/// Lien du pied (petit, couleur des liens).
fn foot_link(ui: &mut egui::Ui, t: &Tokens, text: &str) -> bool {
    let resp = ui.add(egui::Label::new(RichText::new(text).size(t.typography.small).color(t.colors.link)).selectable(false).sense(Sense::click()));
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, text));
    if resp.hovered() || resp.has_focus() {
        let y = resp.rect.bottom() - 1.0;
        ui.painter().line_segment([egui::pos2(resp.rect.left(), y), egui::pos2(resp.rect.right(), y)], egui::Stroke::new(1.0, t.colors.link));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

impl App {
    /// En-tête du compte : avatar et sa décoration, nom (au style de nom du
    /// profil), « Modifier les profils » et son crayon. `true` : cliqué.
    fn nav_account(&mut self, ui: &mut egui::Ui, t: &Tokens) -> bool {
        let Some(me) = self.store.me.clone() else { return false };
        let lang = self.lang;
        let m = metrics(t);
        let shown = self.shown(me.id, None);
        let name = crate::ui::profile::name_of(&me.name, &shown);
        let worn = crate::ui::cosmetics::worn(ui.ctx(), None, me.id);
        let side = (t.layout.avatar_size * 1.4).round();
        let avatars = t.avatars_shown();
        // La décoration déborde de l'avatar : la marge la contient.
        let pad = (side * 0.1).ceil();
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), side + 2.0 * pad), Sense::click());
        let link = st::EDIT_PROFILES.get(lang);
        if ui.is_rect_visible(rect) {
            let hot = resp.hovered() || resp.has_focus();
            let mut left = rect.left() + pad;
            if avatars {
                let at = crate::ui::util::pixel_square(ui, egui::pos2(left, rect.top() + pad), side);
                crate::ui::util::paint_avatar(ui, &self.images, t, at, &me.avatar, &name);
                crate::ui::cosmetics::row_decoration(ui, &self.images, at, &worn);
                left = at.right() + m.gap;
            }
            let width = (rect.right() - left).max(1.0);
            let (title, glow) = crate::ui::cosmetics::styled_line(ui, &name, t.typography.title, worn.look.name_style(), t.colors.bg1, t.colors.text_strong, width);
            let pencil = Tokens::icon_side(t.typography.small);
            let color = if hot { t.colors.text_strong } else { t.colors.muted };
            let sub = ui.painter().layout_no_wrap(link.to_owned(), egui::FontId::proportional(t.typography.body - 1.0), color);
            let top = rect.top() + Tokens::centered_top(rect.height(), title.size().y + t.spacing.item_y / 2.0 + sub.size().y);
            let sub_at = egui::pos2(left, top + title.size().y + t.spacing.item_y / 2.0);
            crate::ui::cosmetics::paint_styled(ui.painter(), egui::pos2(left, top), title, glow, t.colors.text_strong);
            let at = crate::ui::util::pixel_square(ui, egui::pos2(sub_at.x + sub.size().x + t.spacing.item_y, sub_at.y + (sub.size().y - pencil) / 2.0), pencil);
            ui.painter().galley(sub_at, sub, color);
            if at.right() <= rect.right() {
                icons::paint(Icon::Pencil, ui.painter(), at, color);
            }
        }
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name}, {link}")));
        crate::ui::keys::focus_ring(ui, t, &resp);
        resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
    }

    /// Sous-entrée marquée dans l'entrée ouverte : la section où la page est
    /// rendue, sinon la sous-entrée de la page elle-même.
    fn current_sub(&self, e: &Entry) -> Option<usize> {
        let (page, lang) = (self.params.page(), self.lang);
        let spy = self.params.spy;
        let section = e.subs.iter().position(|s| s.page == page && s.anchor.is_some_and(|a| Some(shell::anchor_of(a.get(lang))) == spy));
        section.or_else(|| e.subs.iter().position(|s| s.page == page && s.anchor.is_none()))
    }

    pub(super) fn settings_nav(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let m = metrics(t);
        let mut go: Option<(Page, Option<Text>)> = None;
        if self.nav_account(ui, t) {
            go = Some((Page::Profile, None));
        }
        ui.add_space(t.spacing.item_y);
        let r = search_field(ui, t, &mut self.params.search, st::SEARCH_HINT.get(lang));
        crate::ui::a11y::name(ui.ctx(), r.id, st::SEARCH.get(lang));
        ui.add_space(t.spacing.item_x);
        egui::ScrollArea::vertical().id_salt("parametres_nav").auto_shrink([false, false]).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = (t.spacing.item_y / 2.0).round();
            if self.params.search.trim().is_empty() {
                let page = self.params.page();
                for (i, (group, entries)) in GROUPS.iter().enumerate() {
                    if i > 0 {
                        shell::nav_divider(ui, t);
                    }
                    if let Some(g) = group {
                        shell::nav_title(ui, t, g.get(lang));
                    }
                    for e in entries.iter() {
                        let open = e.holds(page);
                        let row = entry_row(ui, t, e.icon, e.title().get(lang), e.badge.map(|b| b.get(lang)), None, open);
                        if row.clicked() {
                            go = Some((e.page, None));
                        }
                        if !open {
                            continue;
                        }
                        let current = self.current_sub(e);
                        let mut area = row.rect;
                        for (k, s) in e.subs.iter().enumerate() {
                            let sub = sub_row(ui, t, s.label.get(lang), current == Some(k));
                            area = area.union(sub.rect);
                            if sub.clicked() {
                                go = Some((s.page, s.anchor));
                            }
                        }
                        if std::mem::take(&mut self.params.reveal) {
                            // Page ouverte d'ailleurs (palette, raccourci, lien) : son entrée
                            // et ses sous-entrées sont amenées en vue (rien ne bouge si elles y sont).
                            ui.scroll_to_rect(area, None);
                        }
                    }
                }
            } else {
                let found = search(lang, &self.params.search);
                if found.is_empty() {
                    ui.horizontal(|ui| {
                        ui.add_space(m.pad);
                        ui.label(RichText::new(st::NO_RESULT.get(lang)).color(t.colors.muted));
                    });
                }
                for h in found {
                    if super::nav_row(ui, t, h.label.get(lang), Some(h.page.title().get(lang)), self.params.page() == h.page).clicked() {
                        go = Some((h.page, h.anchor));
                    }
                }
            }
            // « Déconnexion », en rouge, dans le dernier groupe (sans titre).
            shell::nav_divider(ui, t);
            if entry_row(ui, t, Icon::Logout, st::LOG_OUT.get(lang), None, Some(t.colors.danger), false).on_hover_text(self.logout_wording().1).clicked() {
                self.params.logout_asked = true;
                self.params.open = false;
            }
            // Pied : la version, puis ce que la page « À propos » offre.
            shell::nav_divider(ui, t);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_y, 0.0);
                ui.add_space(m.pad);
                ui.add(egui::Label::new(RichText::new(version_line()).size(t.typography.small).color(t.colors.faint)).selectable(false));
                ui.end_row();
                ui.add_space(m.pad);
                if foot_link(ui, t, s2::PAGE_ABOUT.get(lang)) {
                    go = Some((Page::About, None));
                }
                ui.label(RichText::new("\u{2022}").size(t.typography.small).color(t.colors.faint));
                if foot_link(ui, t, st::FOOT_LICENSES.get(lang)) {
                    go = Some((Page::About, Some(s2::SEC_LICENSES)));
                }
            });
            ui.add_space(t.spacing.category_gap);
        });
        if let Some((page, anchor)) = go {
            self.params.show_at(page, anchor, lang);
        }
    }
}
