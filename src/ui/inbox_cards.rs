//! Pièces de la boîte de réception : cartes (cadre, en-tête, boutons
//! carrés), onglets soulignés, liste virtualisée, et le message d'une carte.
//!
//! Le message est dessiné par le code de la discussion (`message.rs`), en
//! mode « parcours » (`Acts::browse`, celui des résultats de recherche) : ni
//! barre d'actions ni menu, un clic va au message. Il n'y a pas de second
//! moteur de rendu.

use super::keys::focus_ring;
use super::message::message_ui_acts;
use super::msg_actions::{Action, ActionsUi, Acts};
use super::util::{inline_icon, paint_avatar, paint_image, paint_initials};
use super::vlist::Geometry;
use crate::i18n::Lang;
use crate::images::Images;
use crate::state::markdown::{parse_message, Doc};
use crate::state::model::{Avatar, ImageRef, Message};
use crate::state::Store;
use crate::theme::{
    icons::{self, Icon},
    Tokens,
};
use eframe::egui::{self, pos2, vec2, Align, Layout, Rect, Response, RichText, Sense, Ui, UiBuilder, Vec2};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Bits ajoutés à l'identifiant d'un message dessiné dans la boîte : ses
/// zones cliquables ne se confondent ni avec celles du même message dans la
/// discussion, ni avec celles d'un résultat de recherche (bit 63 seul). Les
/// snowflakes n'atteignent pas le bit 62 avant 2049.
const SALT: u64 = 0b11 << 62;
/// Marge intérieure d'une carte et de son en-tête.
pub(super) const PAD: f32 = 12.0;
/// Côté de l'icône de serveur d'un en-tête de carte.
const HEAD_ICON: f32 = 32.0;
/// Hauteur d'un en-tête de carte : deux lignes de texte et la marge.
const HEAD_H: f32 = 40.0 + 2.0 * PAD;
/// Côté d'un bouton carré (en-tête du panneau et des cartes).
pub(super) const SQUARE: f32 = 32.0;
/// Rayon des cartes et des boutons carrés.
const CARD_RADIUS: u8 = 12;
const SQUARE_RADIUS: u8 = 8;
/// Documents gardés au plus (au-delà, le cache repart de zéro).
const MAX_DOCS: usize = 512;

/// Empreinte d'une valeur (clé de cache).
pub(super) fn stamp(v: impl Hash) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    v.hash(&mut h);
    h.finish()
}

/// Message prêt à être dessiné : copie à l'identifiant salé, texte mis en forme.
pub(super) struct Prep {
    stamp: u64,
    pub(super) msg: Message,
    pub(super) doc: Arc<Doc>,
}

/// Messages mis en forme, à la demande : seuls ceux des cartes visibles le sont.
#[derive(Default)]
pub(super) struct Docs(HashMap<u64, Prep>);

impl Docs {
    /// Texte mis en forme du message `id`, s'il a été dessiné (tests).
    #[cfg(test)]
    pub(super) fn doc(&self, id: u64) -> Option<&Doc> {
        self.0.get(&id).map(|p| &*p.doc)
    }

    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    /// Le message `m` prêt à dessiner ; refait si `stamp` a changé.
    /// `guild` : serveur du salon, pour un message qui ne porte pas le sien (historique).
    pub(super) fn get(&mut self, store: &Store, m: &Message, guild: Option<u64>, stamp: u64, lang: Lang, h24: bool) -> &Prep {
        if self.0.len() >= MAX_DOCS && !self.0.contains_key(&m.id) {
            self.0.clear();
        }
        let fresh = |m: &Message| {
            let mut msg = m.clone();
            msg.id |= SALT;
            msg.guild_id = msg.guild_id.or(guild);
            Prep { stamp, doc: Arc::new(parse_message(store, m, lang, h24)), msg }
        };
        let p = self.0.entry(m.id).or_insert_with(|| fresh(m));
        if p.stamp != stamp {
            *p = fresh(m);
        }
        p
    }
}

/// Jetons d'un message dessiné dans une carte : marges de la carte.
pub(super) fn card_tokens(t: &Tokens) -> Tokens {
    let mut c = t.clone();
    c.spacing.message_x = c.spacing.message_x.min(PAD);
    c.spacing.message_right = PAD;
    c
}

/// Un message, par le rendu de la discussion en mode « parcours ». Renvoie
/// l'identifiant (réel) du message visé par un clic.
/// `ts` : heure montrée (celle du message, ou l'échéance d'un message programmé).
#[allow(clippy::too_many_arguments)]
pub(super) fn message(ui: &mut Ui, t: &Tokens, lang: Lang, store: &Store, images: &Images, p: &Prep, grouped: bool, ts: i64) -> Option<u64> {
    let mut scratch = ActionsUi::default();
    let mut acts = Acts::new(&mut scratch, store, p.msg.channel_id, ui.ctx().clone());
    acts.browse = true;
    acts.home = true;
    message_ui_acts(ui, t, lang, images, &p.msg, &p.doc, grouped, ts, &mut acts);
    scratch.take_out().into_iter().find_map(|a| match a {
        Action::Jump(id) => Some(id & !SALT),
        _ => None,
    })
}

/// Cadre d'une carte : fond un peu plus clair que le panneau, contour fin, grand rayon.
pub(super) fn card<R>(ui: &mut Ui, t: &Tokens, add: impl FnOnce(&mut Ui) -> R) -> R {
    egui::Frame::new()
        .fill(t.colors.bg2)
        .stroke(t.border())
        .corner_radius(CARD_RADIUS)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = 0.0;
            add(ui)
        })
        .inner
}

/// Filet sur toute la largeur.
pub(super) fn rule(ui: &mut Ui, t: &Tokens) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, t.colors.separator);
}

/// Bouton carré arrondi (fond sombre, contour fin) fait d'une icône ; `on` : enfoncé.
pub(super) fn square_button(ui: &mut Ui, t: &Tokens, icon: Icon, tip: &str, on: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(SQUARE), Sense::click());
    if ui.is_rect_visible(rect) {
        let k = if on { 1.0 } else { super::motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus()) };
        let fill = super::motion::mix(t.colors.bg1, t.colors.selected, k);
        ui.painter().rect(rect, SQUARE_RADIUS, fill, t.border(), egui::StrokeKind::Inside);
        let side = Tokens::icon_side(t.typography.body);
        icons::paint(icon, ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(side)), super::motion::mix(t.colors.muted, t.colors.text_strong, k));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    focus_ring(ui, t, &resp);
    super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Image à gauche d'un en-tête de carte.
pub(super) enum HeadIcon<'a> {
    /// Icône du serveur (initiales de son nom à défaut).
    Guild(Option<ImageRef>, &'a str),
    /// Avatar d'une conversation privée.
    Person(&'a Avatar, &'a str),
    /// Icône dessinée (carte sans salon).
    Glyph(Icon),
}

/// En-tête d'une carte.
pub(super) struct Head<'a> {
    pub(super) icon: HeadIcon<'a>,
    /// Icône du genre de salon devant le titre (dièse, fil…).
    pub(super) kind: Option<Icon>,
    pub(super) title: &'a str,
    /// « Serveur › Catégorie », en petit sous le titre.
    pub(super) sub: &'a str,
}

/// En-tête d'une carte : image, titre en gras, sous-titre en petit ; `tail`
/// pose les boutons de droite (de droite à gauche). Les boutons sont placés
/// d'abord et les noms raccourcis par « … » dans la place qui reste : rien
/// ne se chevauche, quelle que soit la longueur des noms. Renvoie la réponse
/// de l'en-tête entier (clic : ouvrir le salon).
pub(super) fn head(ui: &mut Ui, t: &Tokens, images: &Images, h: &Head<'_>, tail: impl FnOnce(&mut Ui)) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), HEAD_H), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, h.title));
    focus_ring(ui, t, &resp);
    let mut right = ui.new_child(UiBuilder::new().max_rect(rect.shrink(PAD)).layout(Layout::right_to_left(Align::Center)));
    right.spacing_mut().item_spacing.x = t.spacing.item_x;
    tail(&mut right);
    let rest = right.available_rect_before_wrap();
    let mut left = right.new_child(UiBuilder::new().max_rect(rest).layout(Layout::left_to_right(Align::Center)));
    let slot = Rect::from_min_size(pos2(rest.left(), rest.center().y - HEAD_ICON / 2.0), Vec2::splat(HEAD_ICON));
    left.advance_cursor_after_rect(slot);
    match &h.icon {
        HeadIcon::Guild(icon, name) => {
            if !paint_image(&left, images, slot, *icon, egui::CornerRadius::same(SQUARE_RADIUS)) {
                left.painter().rect_filled(slot, SQUARE_RADIUS, t.colors.selected);
                left.painter().text(slot.center(), egui::Align2::CENTER_CENTER, super::util::initials(name), egui::FontId::proportional(Tokens::initials_size(HEAD_ICON)), t.colors.text_strong);
            }
        }
        HeadIcon::Person(avatar, name) => {
            if t.avatars_shown() {
                paint_avatar(&left, images, t, slot, avatar, name);
            } else {
                paint_initials(&left, t, slot, (t.colors.selected, t.colors.text_strong), name);
            }
        }
        HeadIcon::Glyph(icon) => icons::paint(*icon, left.painter(), slot.shrink(HEAD_ICON * 0.15), t.colors.muted),
    }
    // Titre et sous-titre : deux lignes placées à la main, centrées ensemble sur l'icône.
    let text_left = slot.right() + t.spacing.item_x + 2.0;
    let (title_h, sub_h) = (left.text_style_height(&egui::TextStyle::Body), if h.sub.is_empty() { 0.0 } else { left.text_style_height(&egui::TextStyle::Small) + 2.0 });
    let top = (rest.center().y - (title_h + sub_h) / 2.0).round();
    let line = |y: f32, height: f32| Rect::from_min_max(pos2(text_left, y), pos2(rest.right().max(text_left), y + height));
    let mut title = left.new_child(UiBuilder::new().max_rect(line(top, title_h)).layout(Layout::left_to_right(Align::Center)));
    title.spacing_mut().item_spacing.x = 4.0;
    if let Some(kind) = h.kind {
        inline_icon(&mut title, kind, t.typography.body, t.colors.muted);
    }
    title.add(egui::Label::new(RichText::new(h.title).family(super::fonts::demi()).strong().color(t.colors.text_strong)).truncate().selectable(false));
    if !h.sub.is_empty() {
        let mut sub = left.new_child(UiBuilder::new().max_rect(line(top + title_h + 2.0, sub_h - 2.0)).layout(Layout::left_to_right(Align::Center)));
        sub.add(egui::Label::new(RichText::new(h.sub).small().color(t.colors.faint)).truncate().selectable(false));
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Rangée d'onglets soulignés : l'actif à la couleur d'accent avec un trait
/// de 2 dessous, les autres gris ; les libellés trop longs sont raccourcis
/// par « … » (le plus large d'abord) pour que la rangée tienne dans
/// `width` ; un filet court sous la rangée. Renvoie l'onglet cliqué.
pub(super) fn tab_strip(ui: &mut Ui, t: &Tokens, labels: &[&str], active: usize, width: f32) -> Option<usize> {
    const GAP: f32 = 20.0;
    let font = super::fonts::medium_font(t.typography.button);
    let natural: Vec<f32> = labels.iter().map(|l| ui.painter().layout_no_wrap((*l).to_owned(), font.clone(), t.colors.muted).size().x.ceil()).collect();
    let room = (width - GAP * labels.len().saturating_sub(1) as f32).max(labels.len() as f32);
    // Largeur plafond : la plus grande pour laquelle tous les onglets tiennent.
    let mut cap = natural.iter().copied().fold(0.0, f32::max);
    if natural.iter().sum::<f32>() > room {
        let (mut lo, mut hi) = (0.0, cap);
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if natural.iter().map(|w| w.min(mid)).sum::<f32>() <= room { lo = mid } else { hi = mid }
        }
        cap = lo.floor().max(1.0);
    }
    let height = super::widgets::button_height(t) + 2.0;
    let accent = super::cosmetics::legible(t.colors.accent, t.colors.bg1, 4.5);
    let mut picked = None;
    let row = ui
        .horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            for (i, label) in labels.iter().enumerate() {
                let on = i == active;
                let w = natural[i].min(cap);
                let (rect, resp) = ui.allocate_exact_size(vec2(w, height), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, *label));
                focus_ring(ui, t, &resp);
                if ui.is_rect_visible(rect) {
                    let k = if on { 1.0 } else { super::motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus()) };
                    let ink = if on { accent } else { super::motion::mix(t.colors.muted, t.colors.text_strong, k) };
                    let mut job = egui::text::LayoutJob::simple_singleline((*label).to_owned(), font.clone(), ink);
                    job.wrap = egui::text::TextWrapping { max_width: w, max_rows: 1, break_anywhere: true, overflow_character: Some('\u{2026}') };
                    let galley = ui.painter().layout_job(job);
                    ui.painter().galley(pos2(rect.left(), rect.top() + (height - 2.0 - galley.size().y) / 2.0), galley, ink);
                    if on {
                        ui.painter().rect_filled(Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max), 1.0, accent);
                    }
                }
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                let resp = if natural[i] > w { super::widgets::tip(resp, t, label) } else { resp };
                if resp.clicked() {
                    picked = Some(i);
                }
            }
        })
        .response
        .rect;
    // Le filet passe sous les onglets ; le trait de l'onglet actif le recouvre.
    let line = Rect::from_min_max(pos2(ui.max_rect().left(), row.bottom() - 1.0), pos2(ui.max_rect().right(), row.bottom()));
    ui.painter().rect_filled(line, 0.0, t.colors.separator.gamma_multiply(0.6));
    picked
}

/// Liste de cartes virtualisée : seules celles qui coupent la vue sont mises
/// en page. Hauteurs estimées, puis corrigées au premier dessin (une image de
/// plus, une fois) et gardées tant que la carte reste dans la liste.
#[derive(Default)]
pub(super) struct CardList {
    geo: Geometry,
    key: Option<u64>,
    to_top: bool,
}

impl CardList {
    /// Liste remplacée (autre onglet) : hauteurs oubliées, retour en haut.
    pub(super) fn reset(&mut self) {
        *self = CardList { to_top: true, ..Default::default() };
    }

    /// Dessine les cartes `ids` visibles ; `draw` reçoit le rang de la carte.
    /// Renvoie les rangs dessinés (tests, mesures).
    pub(super) fn show(&mut self, ui: &mut Ui, salt: &str, ids: &[u64], estimate: f32, gap: f32, mut draw: impl FnMut(&mut Ui, usize)) -> std::ops::Range<usize> {
        let key = stamp((salt, ids));
        if self.key != Some(key) {
            let old: HashMap<u64, f32> = self.geo.ids().iter().enumerate().map(|(i, id)| (*id, self.geo.height(i))).collect();
            let heights = ids.iter().map(|id| old.get(id).copied().unwrap_or(estimate + gap)).collect();
            self.geo.set(ids.to_vec(), heights);
            self.key = Some(key);
        }
        let mut area = egui::ScrollArea::vertical().id_salt(salt).auto_shrink(false);
        if std::mem::take(&mut self.to_top) {
            area = area.vertical_scroll_offset(0.0);
        }
        let geo = &mut self.geo;
        let mut changed = false;
        let mut drawn = 0..0;
        area.show_viewport(ui, |ui, viewport| {
            let origin = ui.max_rect().min;
            let width = ui.available_width();
            ui.set_min_size(vec2(width, geo.total()));
            drawn = geo.range(viewport.min.y, viewport.max.y);
            for i in drawn.clone() {
                let rect = Rect::from_min_size(pos2(origin.x, origin.y + geo.top(i)), vec2(width, f32::INFINITY));
                let mut child = ui.new_child(UiBuilder::new().max_rect(rect).layout(Layout::top_down(Align::Min)).id_salt((salt, ids[i])));
                draw(&mut child, i);
                let h = child.min_rect().height() + gap;
                changed |= geo.set_height(i, (h * ui.pixels_per_point()).round() / ui.pixels_per_point());
            }
        });
        if changed {
            ui.ctx().request_repaint();
        }
        drawn
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pass(width: f32, mut f: impl FnMut(&mut Ui)) {
        let ctx = egui::Context::default();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width, 600.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| f(ui));
        // Les textures produites sont jetées (aucun moteur de rendu).
        out.textures_delta.clear();
    }

    /// Une liste de 200 cartes ne met en page que celles de la vue.
    #[test]
    fn only_visible_cards_are_laid_out() {
        let ids: Vec<u64> = (10..210).collect();
        let mut list = CardList::default();
        let mut calls = 0;
        pass(600.0, |ui| {
            let drawn = list.show(ui, "essai", &ids, 100.0, 12.0, |ui, _| {
                calls += 1;
                ui.allocate_space(vec2(ui.available_width(), 100.0));
            });
            assert!(drawn.start == 0 && drawn.end < 12, "{drawn:?}");
        });
        assert!((1..12).contains(&calls), "{calls} cartes mises en page");
        // Une carte qui reste garde sa hauteur mesurée quand la liste change (ici, hors de la vue).
        list.geo.set_height(150, 40.0);
        pass(600.0, |ui| {
            list.show(ui, "essai", &ids[2..], 100.0, 12.0, |_, _| {});
        });
        assert_eq!(list.geo.height(148), 40.0);
    }

    /// Les onglets tiennent dans la largeur offerte, quitte à être raccourcis.
    #[test]
    fn tabs_fit_the_offered_width() {
        let t = Tokens::default();
        let labels = ["Messages non lus", "Mentions", "Messages marqués", "Rappels", "Programmés"];
        for width in [560.0, 300.0, 180.0] {
            pass(900.0, |ui| {
                ui.allocate_ui(vec2(width, 60.0), |ui| {
                    let left = ui.cursor().left();
                    assert_eq!(tab_strip(ui, &t, &labels, 0, width), None);
                    assert!(ui.min_rect().right() <= left + width + 1.0, "{width} : {:?}", ui.min_rect());
                });
            });
        }
    }
}
