//! Réactions (lot T5) : pastilles sous le message (clic = ajouter ou
//! retirer la sienne), liste de qui a réagi (survol ou clic droit, chargée
//! à la demande), sélecteur d'emojis (`emoji_picker`) pour une nouvelle
//! réaction. Les emojis sont des images (`emoji`).

use super::msg_actions::{Acts, Action, Hit};
use super::App;
use crate::i18n::{p, t as tx, Lang};
use crate::state::model::Message;
use crate::state::social::Emoji;
use crate::state::store::social::Reactors;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Id, RichText, Sense};

/// Nom court d'un emoji Unicode (sélecteur de variante ignoré).
pub(super) fn shortcode(text: &str) -> Option<String> {
    crate::state::emoji::shortcode(text)
}

/// Contenu d'une pastille : image de l'emoji (s'il en a une) ou icône, et compte.
struct Pill {
    img: Option<super::emoji::Img>,
    icon: Option<Icon>,
    text: String,
    /// Compte qui vient de changer : texte d'avant, avancement, et sens (vrai : il monte).
    slide: Option<(String, f32, bool)>,
}

/// Marge latérale d'une pastille, et écart entre l'emoji et le compte.
fn pill_pad(t: &Tokens) -> f32 {
    (t.spacing.item_x * 0.75).round()
}

fn pill_font(t: &Tokens) -> egui::FontId {
    crate::ui::fonts::demi_font(t.typography.button)
}

/// Pastille aux mesures du client : haute de 24, emoji de 16, compte demi-gras.
fn pill_size(ui: &egui::Ui, t: &Tokens, p: &Pill) -> egui::Vec2 {
    let pad = pill_pad(t);
    let text = if p.text.is_empty() { 0.0 } else { ui.painter().layout_no_wrap(p.text.clone(), pill_font(t), egui::Color32::PLACEHOLDER).size().x };
    let img = if p.img.is_some() || p.icon.is_some() { t.reaction_emoji() + if p.text.is_empty() { 0.0 } else { pad } } else { 0.0 };
    egui::vec2((text + img + 2.0 * pad).round(), t.reaction_height())
}

fn paint_pill(ui: &egui::Ui, t: &Tokens, rect: egui::Rect, p: &Pill, mine: bool, burst: bool) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let fill = t.card_fill();
    let (fill, stroke, color) = match (mine, burst) {
        (true, _) => (t.accent_tint(), t.colors.accent, t.colors.text_strong),
        (false, true) => (fill, t.colors.link, t.colors.text),
        // Sans contour visible : il reste dans la mesure, la pastille ne bouge pas quand elle devient la mienne.
        (false, false) => (fill, fill, t.colors.muted),
    };
    let painter = ui.painter();
    painter.rect(rect, t.r_row(), fill, egui::Stroke::new(t.shapes.border.max(1.0), stroke), egui::StrokeKind::Inside);
    let inner = rect.shrink2(egui::vec2(pill_pad(t), 0.0));
    let side = t.reaction_emoji();
    let at = crate::ui::util::pixel_square(ui, egui::pos2(inner.left(), inner.center().y - side / 2.0), side);
    let mut x = inner.left();
    if let Some(img) = p.img {
        super::emoji::paint(ui, at, img);
        x = at.right() + pill_pad(t);
    } else if let Some(icon) = p.icon {
        crate::theme::icons::paint(icon, painter, at, color);
        x = at.right() + pill_pad(t);
    }
    if !p.text.is_empty() {
        let pos = egui::pos2(x, inner.center().y);
        match &p.slide {
            // Le compte glisse : l'ancien sort, le nouveau entre, sans quitter la pastille.
            Some((old, k, up)) => {
                let clipped = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
                let way = if *up { rect.height() } else { -rect.height() };
                clipped.text(pos + egui::vec2(0.0, (1.0 - k) * way), egui::Align2::LEFT_CENTER, &p.text, pill_font(t), color.gamma_multiply(*k));
                clipped.text(pos - egui::vec2(0.0, k * way), egui::Align2::LEFT_CENTER, old, pill_font(t), color.gamma_multiply(1.0 - k));
            }
            None => {
                painter.text(pos, egui::Align2::LEFT_CENTER, &p.text, pill_font(t), color);
            }
        }
    }
}

/// Pastilles des réactions, à la ligne si besoin, et « + » pour en ajouter
/// une. Mise en page identique à la mesure et au dessin.
pub(super) fn pills(ui: &mut egui::Ui, t: &Tokens, m: &Message, acts: &mut Acts) {
    // « Afficher les réactions » décoché (Texte et images) : aucune pastille.
    if m.meta.reactions.is_empty() || !super::attachments::shown(ui.ctx()).reactions {
        return;
    }
    let ctx = acts.ctx.clone();
    // « + » fantôme : au survol du message seulement (la barre d'actions le dit
    // depuis l'image d'avant) ; au doigt, pas de survol, il reste affiché.
    let ghost = acts.st.bar.is_some_and(|(id, _)| id == m.id) || super::mobile::is_active(ui.ctx());
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::splat(t.spacing.item_y);
        for r in &m.meta.reactions {
            let img = super::emoji::Img::of(&r.emoji);
            // Sans image (emoji inconnu de la table) : son texte.
            let label = if img.is_some() { None } else { Some(acts.st.emoji_text(&ctx, &r.emoji)) };
            for (burst, count, mine) in [(false, r.normal, r.me), (true, r.burst, r.me_burst)] {
                if count == 0 {
                    continue;
                }
                let text = |n: u32| label.as_ref().map_or_else(|| n.to_string(), |l| format!("{l} {n}"));
                let mut pill = Pill { img, icon: None, text: text(count), slide: None };
                let (rect, _) = ui.allocate_exact_size(pill_size(ui, t, &pill), Sense::hover());
                // Compte qui change (réaction ajoutée ou retirée) : la pastille fait un « pop » et son compte glisse.
                let key = egui::Id::unique(("réaction", m.id, &r.emoji, burst));
                let (k, was) = if ui.is_rect_visible(rect) { super::motion::changed(ui.ctx(), key, u64::from(count) << 1 | u64::from(mine), super::motion::dur::MORPH) } else { (1.0, 0) };
                let before = (was >> 1) as u32;
                if k < 1.0 && before != count && super::motion::level(ui.ctx()) == super::motion::Level::Full {
                    pill.slide = Some((text(before), k, count > before));
                }
                let span = super::motion::Span::begin(ui);
                paint_pill(ui, t, rect, &pill, mine, burst);
                span.end(ui, 1.0, super::motion::pop_scale(ui.ctx(), rect.center(), k));
                acts.hits.push((rect, Hit::Pill { emoji: r.emoji.clone(), burst }));
            }
        }
        if acts.rights.react {
            let plus = Pill { img: None, icon: Some(Icon::Smile), text: String::new(), slide: None };
            let (rect, _) = ui.allocate_exact_size(pill_size(ui, t, &plus), Sense::hover());
            if ghost {
                paint_pill(ui, t, rect, &plus, false, false);
            }
            acts.hits.push((rect, Hit::AddReaction));
        }
    });
}
/// Clic sur une pastille (sa réaction ordinaire : ajoutée ou retirée) ;
/// survol ou clic droit : qui a réagi.
#[allow(clippy::too_many_arguments)]
pub(super) fn pill_interact(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts, r: egui::Response, emoji: Emoji, burst: bool) {
    // Une super-réaction ne s'ajoute pas ici (réservée à l'app officielle).
    let clickable = !burst && acts.rights.react_existing && !acts.store.reaction_pending(m.id, &emoji);
    if clickable && r.hovered() {
        ui.painter().rect_filled(r.rect, t.r_row(), t.colors.hover);
    }
    let r = if clickable { r.on_hover_cursor(egui::CursorIcon::PointingHand) } else { r };
    if r.clicked() && clickable {
        acts.st.push(Action::React(m.id, emoji.clone()));
    }
    let total = m.meta.reactions.iter().find(|x| x.emoji.same(&emoji)).map_or(0, |x| if burst { x.burst } else { x.normal });
    r.context_menu(|ui| reactors_list(ui, t, lang, m, acts, &emoji, burst, total));
    r.on_hover_ui(|ui| reactors_list(ui, t, lang, m, acts, &emoji, burst, total));
}

/// Liste de qui a réagi (une page, demandée la première fois qu'elle s'affiche).
#[allow(clippy::too_many_arguments)]
fn reactors_list(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts, emoji: &Emoji, burst: bool, total: u32) {
    let ctx = acts.ctx.clone();
    // L'emoji en image et son nom court.
    let title = match emoji {
        Emoji::Unicode(s) => shortcode(s).map_or_else(|| acts.st.emoji_text(&ctx, emoji), |n| format!(":{n}:")),
        Emoji::Custom { .. } => emoji.display(),
    };
    ui.horizontal(|ui| {
        if let Some(img) = super::emoji::Img::of(emoji) {
            let side = super::emoji::row_height(ui, &egui::TextStyle::Body.resolve(ui.style()));
            super::emoji::icon(ui, img, side, Sense::hover());
        }
        ui.label(RichText::new(title).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
    });
    if burst {
        ui.label(RichText::new(tx::SUPER_REACTION.get(lang)).size(t.typography.small).color(t.colors.faint));
    }
    match acts.store.reactors(m.id, emoji, burst) {
        None => {
            acts.st.push(Action::Reactors(m.id, emoji.clone(), burst));
            ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
        }
        Some(Reactors::Loading) => {
            ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
        }
        Some(Reactors::Failed(e)) => {
            ui.label(RichText::new(super::words::problem(lang, e, t.layout.clock == crate::theme::Clock::H24)).color(t.colors.danger));
        }
        Some(Reactors::Loaded(users)) => {
            for u in users {
                ui.label(RichText::new(&u.name).color(t.colors.text));
            }
            let rest = u64::from(total).saturating_sub(users.len() as u64);
            if rest > 0 {
                ui.label(RichText::new(p::more_reactors(lang, rest)).color(t.colors.muted));
            }
        }
    }
}

impl App {
    /// Ouvre le sélecteur de réaction du message `message_id` du salon
    /// affiché, à `pos` (aperçu, tests) ; `super_mode` : interrupteur « Super
    /// réaction » actif.
    pub fn open_reaction_picker(&mut self, message_id: u64, pos: egui::Pos2, super_mode: bool) {
        let Some(channel_id) = self.store.channel else { return };
        self.emojis.super_mode = super_mode;
        // Compte déjà connu : la première image ne referme pas ce qui vient d'être ouvert.
        self.actions.me = self.store.me.as_ref().map(|u| u.id);
        self.actions.picker = Some(super::msg_actions::Picker { channel_id, message_id, pos, query: String::new() });
        self.ctx.request_repaint();
    }

    /// Sélecteur d'emojis pour une réaction, ouvert sous le bouton qui l'a demandé.
    pub(super) fn reaction_picker(&mut self, ui: &mut egui::Ui) {
        let Some(mut picker) = self.actions.picker.take() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let area = egui::Area::new(Id::unique("sélecteur-réaction")).order(egui::Order::Foreground).fixed_pos(picker.pos).constrain(true);
        let mut chosen = None;
        let store = &self.store;
        let images = &self.images;
        let emojis = &mut self.emojis;
        // Emojis seuls ici : l'onglet du sélecteur de la saisie est gardé de côté.
        let tab = std::mem::replace(&mut emojis.tab, super::emoji_picker::Tab::Emojis);
        let shown = area.show(ui.ctx(), |ui| {
            t.window_frame().inner_margin(t.embed_margin()).show(ui, |ui| {
                ui.set_width(super::emoji_picker::width(ui.ctx(), &t));
                let src = super::emoji_picker::Sources::of(store, picker.channel_id, false);
                chosen = super::emoji_picker::body(ui, &t, lang, emojis, &mut picker.query, &src, images);
            });
        });
        self.emojis.tab = tab;
        self.apply_picker_acts();
        let outside = ui.input(|i| i.pointer.any_pressed()) && !shown.response.contains_pointer();
        let escape = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        match chosen {
            Some(super::emoji_picker::Choice { pick: super::emoji_picker::Pick::Emoji(e), keep_open, burst }) => {
                self.emoji_used(&e);
                if burst {
                    // Super réaction (Nitro) : ajoutée, ou la mienne retirée.
                    self.store.super_react(picker.channel_id, picker.message_id, e);
                } else {
                    self.store.add_reaction(picker.channel_id, picker.message_id, e);
                }
                if keep_open {
                    self.actions.picker = Some(picker);
                }
            }
            Some(_) => {}
            None if outside || escape => {}
            None => self.actions.picker = Some(picker),
        }
        if self.actions.picker.is_none() {
            self.emojis.closed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcodes() {
        assert_eq!(shortcode("👍").as_deref(), Some("thumbsup"));
        assert_eq!(shortcode("❤\u{FE0F}").as_deref(), Some("heart"));
        assert_eq!(shortcode("🦀").as_deref(), Some("crab"));
        assert_eq!(shortcode("x"), None);
    }
}
