//! Vue d'un salon forum ou média (lot T10) : publications (fils) triées,
//! filtrées par étiquettes et par une recherche locale, liste virtualisée
//! (seules les lignes visibles sont mises en page), publications archivées
//! chargées à la demande, et création d'une publication.
//!
//! La liste filtrée et triée n'est refaite que lorsque les fils ou les
//! filtres changent (`Threads::rev`), jamais à chaque image.

use super::util::inline_icon;
use super::App;
use crate::i18n::table::threads::{f, p, t as tx};
use crate::i18n::time;
use crate::state::model::snowflake_ms;
use crate::state::threads::{ThreadAction, MAX_NAME};
use crate::theme::icons::Icon;
use eframe::egui::{self, RichText, Sense};
use std::collections::BTreeSet;

/// Publication en cours d'écriture.
#[derive(Default)]
struct Draft {
    title: String,
    content: String,
    tags: BTreeSet<u64>,
    /// Envoyée : compteur `Threads::created` au moment de l'envoi.
    sent: Option<u64>,
}

/// Clé de la liste en cache : forum, version des fils, filtres.
type Key = (u64, u64, String, bool, Vec<u64>);

#[derive(Default)]
pub(super) struct ForumUi {
    forum: Option<u64>,
    query: String,
    /// Tri par date de création (sinon par activité récente).
    by_created: bool,
    /// Étiquettes demandées : une publication en porte au moins une.
    tags: BTreeSet<u64>,
    cache: Option<(Key, Vec<u64>)>,
    draft: Option<Draft>,
    /// Vue choisie : galerie de cartes (vrai) ou liste ; pas encore choisie :
    /// grille pour un salon média, liste pour un forum.
    gallery: Option<bool>,
}

/// Largeur visée d'une carte de galerie ; la grille prend autant de colonnes qu'il en tient.
const CARD: f32 = 260.0;
/// Largeur en dessous de laquelle une carte ne descend pas.
const CARD_MIN: f32 = 180.0;
/// Proportions de la vignette d'une carte.
const THUMB_ASPECT: f32 = 16.0 / 9.0;

/// Clé d'image d'une vignette, tirée de son adresse (FNV-1a, comme le sélecteur de GIF).
fn thumb_ref(url: &str) -> crate::state::model::ImageRef {
    let h = url.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3));
    crate::state::model::ImageRef::Media { owner: h, slot: 0xF0, rev: (h >> 32) as u32 }
}

/// Vignette `url` (de `w` × `h` pixels) recadrée au centre pour remplir `rect` ; `false` : pas encore là.
fn paint_thumb(ui: &egui::Ui, images: &crate::images::Images, rect: egui::Rect, r: egui::CornerRadius, (url, w, h): (&str, u32, u32)) -> bool {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    // Demandée à la taille qui couvre le cadre, pas plus.
    let scale = (rect.width() / w).max(rect.height() / h);
    let crate::images::Image::Ready(tex) = images.get_media(thumb_ref(url), url, egui::vec2(w * scale, h * scale)) else { return false };
    let (have, want) = (w / h, rect.width() / rect.height().max(1.0));
    let mut uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    uv = if have > want { uv.shrink2(egui::vec2((1.0 - want / have) / 2.0, 0.0)) } else { uv.shrink2(egui::vec2(0.0, (1.0 - have / want) / 2.0)) };
    ui.painter().add(egui::Shape::Rect(egui::epaint::RectShape::filled(rect, r, egui::Color32::WHITE).with_texture(tex, uv)));
    true
}

/// Étiquette cliquable : son emoji en image (Twemoji ou emoji du serveur,
/// pas un caractère que la police n'a pas), puis son nom.
fn tag_chip(ui: &mut egui::Ui, on: bool, tag: &crate::state::threads::ForumTag) -> egui::Response {
    let img = tag.emoji.as_ref().and_then(super::emoji::Img::of);
    let font = egui::TextStyle::Button.resolve(ui.style());
    let pad = ui.spacing().button_padding;
    let side = super::emoji::row_height(ui, &font);
    let lead = if img.is_some() { side + pad.x } else { 0.0 };
    let galley = ui.painter().layout_no_wrap(tag.name.clone(), font, egui::Color32::PLACEHOLDER);
    let size = egui::vec2(lead + galley.size().x + 2.0 * pad.x, (galley.size().y + 2.0 * pad.y).max(ui.spacing().interact_size.y));
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &tag.name));
    if ui.is_rect_visible(rect) {
        let v = ui.style().interact_selectable(&resp, on);
        if on || resp.hovered() || resp.has_focus() {
            ui.painter().rect(rect.expand(v.expansion), v.corner_radius, v.weak_bg_fill, v.bg_stroke, egui::StrokeKind::Inside);
        }
        let x = rect.left() + pad.x;
        if let Some(img) = img {
            super::emoji::paint(ui, super::util::pixel_square(ui, egui::pos2(x, rect.center().y - side / 2.0), side), img);
        }
        ui.painter().galley(egui::pos2(x + lead, rect.center().y - galley.size().y / 2.0), galley, v.text_color());
    }
    resp
}

impl App {
    /// Vue du forum : galerie de cartes ou liste (aperçu, tests).
    pub fn show_forum_gallery(&mut self, on: bool) {
        // Le forum affiché garde ce choix (un autre forum repart de sa vue par défaut).
        self.threads.forum = ForumUi { forum: self.store.channel, gallery: Some(on), ..Default::default() };
    }

    pub(super) fn forum_view(&mut self, ui: &mut egui::Ui, cid: u64) {
        let t = self.theme.clone();
        let lang = self.lang;
        let fu = &mut self.threads.forum;
        // Autre forum : filtres et brouillon repartent de zéro.
        if fu.forum != Some(cid) {
            *fu = super::forum::ForumUi { forum: Some(cid), ..Default::default() };
        }
        let store = &self.store;
        let name = store.channel_name(cid).unwrap_or_default().to_string();
        let info = store.threads.forum(cid).cloned().unwrap_or_default();
        let can_post = store.can_post(cid);
        let posting = store.threads.pending(ThreadAction::Post, 0);
        // Publication créée : le brouillon s'efface ; refusée : il reste, à corriger.
        if let Some(d) = fu.draft.as_mut()
            && let Some(at) = d.sent
            && !posting
        {
            if store.threads.created > at {
                fu.draft = None;
            } else {
                d.sent = None;
            }
        }
        let mut publish = None;

        // Même hauteur que l'en-tête d'une discussion (`chat_head.rs`).
        egui::Panel::top("forum_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                // Disposition mobile : retour à la liste (phase 7).
                super::mobile::back_button(ui, &t, lang);
                inline_icon(ui, if info.media { Icon::Media } else { Icon::Forum }, t.typography.icon, t.colors.faint);
                ui.add(egui::Label::new(RichText::new(&name).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if can_post && fu.draft.is_none() && ui.button(tx::NEW_POST.get(lang)).clicked() {
                        fu.draft = Some(Draft::default());
                    }
                });
            });
            // Filet à la hauteur des autres en-têtes : il prolonge celui de la colonne des salons.
            ui.add_space((t.spacing.chat_head_y - ui.spacing().item_spacing.y).max(0.0));
            let line = ui.cursor().top();
            ui.painter().hline(ui.max_rect().expand2(egui::vec2(t.spacing.chat_head_x, 0.0)).x_range(), line, egui::Stroke::new(t.shapes.border.max(1.0), t.colors.separator));
            ui.add_space(t.spacing.chat_head_y);
            ui.horizontal_wrapped(|ui| {
                ui.add(egui::TextEdit::singleline(&mut fu.query).hint_text(tx::SEARCH_POSTS.get(lang)).desired_width(t.layout.card_width / 2.0));
                egui::ComboBox::from_id_salt(("tri-forum", cid))
                    .selected_text(if fu.by_created { tx::SORT_CREATED.get(lang) } else { tx::SORT_ACTIVITY.get(lang) })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut fu.by_created, false, tx::SORT_ACTIVITY.get(lang));
                        ui.selectable_value(&mut fu.by_created, true, tx::SORT_CREATED.get(lang));
                    });
                // Liste ou galerie de cartes (vignette, titre, extrait).
                let gallery = fu.gallery.unwrap_or(info.media);
                for (on, text) in [(false, crate::i18n::table::restes::t::VIEW_LIST), (true, crate::i18n::table::restes::t::VIEW_GALLERY)] {
                    if ui.selectable_label(gallery == on, text.get(lang)).clicked() {
                        fu.gallery = Some(on);
                    }
                }
                if !info.tags.is_empty() {
                    if ui.selectable_label(fu.tags.is_empty(), tx::ALL_TAGS.get(lang)).clicked() {
                        fu.tags.clear();
                    }
                    for tag in &info.tags {
                        let on = fu.tags.contains(&tag.id);
                        if tag_chip(ui, on, tag).clicked() && !fu.tags.remove(&tag.id) {
                            fu.tags.insert(tag.id);
                        }
                    }
                }
            });
            // Consignes de publication du forum (son sujet), sous les filtres (lot FF, F25).
            if let Some(rules) = store.threads.topic(cid) {
                ui.add_space(t.spacing.item_y);
                egui::Frame::new().fill(t.card_fill()).corner_radius(t.r_media()).inner_margin(t.spacing.item_x).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(crate::i18n::table::ff::t::FORUM_GUIDELINES.get(lang)).small().strong().color(t.colors.muted));
                    egui::ScrollArea::vertical().id_salt(("consignes-forum", cid)).max_height(t.typography.body * 6.0).show(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(rules).color(t.colors.text)).wrap());
                    });
                });
            }
            // Nouvelle publication : titre, message, étiquettes (celles réservées
            // aux modérateurs seulement avec MANAGE_THREADS).
            let manage = store.can(cid, crate::state::Permissions::MANAGE_THREADS) == crate::state::Verdict::Permis;
            let mut close = false;
            if let Some(d) = fu.draft.as_mut() {
                ui.separator();
                ui.add(egui::TextEdit::singleline(&mut d.title).char_limit(MAX_NAME).hint_text(tx::POST_TITLE_HINT.get(lang)).desired_width(f32::INFINITY));
                ui.add(egui::TextEdit::multiline(&mut d.content).hint_text(tx::POST_MESSAGE_HINT.get(lang)).desired_rows(3).desired_width(f32::INFINITY));
                ui.horizontal_wrapped(|ui| {
                    for tag in info.tags.iter().filter(|tag| manage || !tag.moderated) {
                        let on = d.tags.contains(&tag.id);
                        if tag_chip(ui, on, tag).clicked() && !d.tags.remove(&tag.id) {
                            d.tags.insert(tag.id);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    let ok = !d.title.trim().is_empty() && !d.content.trim().is_empty() && !posting;
                    if ui.add_enabled(ok, egui::Button::new(RichText::new(tx::PUBLISH.get(lang)).color(t.colors.on_accent)).fill(t.colors.accent)).clicked() {
                        publish = Some((d.title.clone(), d.content.clone(), d.tags.iter().copied().collect::<Vec<u64>>()));
                    }
                    close = ui.button(tx::CANCEL.get(lang)).clicked();
                    if posting {
                        ui.spinner();
                    } else if !ok {
                        ui.label(RichText::new(tx::TITLE_REQUIRED.get(lang)).small().color(t.colors.faint));
                    }
                });
            }
            if close {
                fu.draft = None;
            }
        });

        // Liste filtrée et triée, refaite seulement si quelque chose a changé.
        let key: Key = (cid, store.threads.rev, fu.query.trim().to_lowercase(), fu.by_created, fu.tags.iter().copied().collect());
        if fu.cache.as_ref().is_none_or(|(k, _)| *k != key) {
            let q = &key.2;
            let mut ids: Vec<u64> = store
                .threads
                .children(cid)
                .iter()
                .copied()
                .filter(|id| !store.is_hidden(*id))
                .filter(|id| store.threads.get(*id).is_some_and(|th| (q.is_empty() || th.name.to_lowercase().contains(q.as_str())) && (key.4.is_empty() || th.applied_tags.iter().any(|tg| key.4.contains(tg)))))
                .collect();
            // Épinglées d'abord ; `children` est déjà dans l'ordre de l'activité.
            let pinned = |id: &u64| !store.threads.get(*id).is_some_and(|th| th.pinned);
            if fu.by_created {
                ids.sort_by_key(|id| (pinned(id), std::cmp::Reverse(*id)));
            } else {
                ids.sort_by_key(pinned);
            }
            fu.cache = Some((key, ids));
        }
        let ids = fu.cache.as_ref().map(|(_, ids)| ids.clone()).unwrap_or_default();
        let gallery = fu.gallery.unwrap_or(info.media);
        let mut wanted: Vec<u64> = Vec::new();
        let archive = store.threads.archive(cid).cloned();
        let mut open = None;
        let mut more = false;

        egui::Panel::bottom("forum_foot").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(t.composer_margin())).show(ui, |ui| {
            ui.horizontal(|ui| match &archive {
                Some(a) if a.loading => {
                    ui.spinner();
                }
                Some(a) if !a.more => {
                    ui.label(RichText::new(tx::NO_MORE_ARCHIVED.get(lang)).small().color(t.colors.faint));
                }
                a => {
                    more = ui.button(tx::LOAD_ARCHIVED.get(lang)).clicked();
                    if let Some(e) = a.as_ref().and_then(|a| a.error.as_ref()) {
                        ui.label(RichText::new(super::words::problem(lang, e, self.theme.layout.clock == crate::theme::Clock::H24)).small().color(t.colors.danger));
                    }
                }
            });
        });

        let h24 = t.layout.clock == crate::theme::Clock::H24;
        // Galerie : les cartes commencent sous la barre d'outils (mêmes marges de côté qu'elle).
        let mut margin = t.channels_margin();
        if gallery {
            let head = super::chat_head::head_margin(&t, t.spacing.chat_head_x);
            (margin.left, margin.right) = (head.left, head.right);
        }
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(margin)).show(ui, |ui| {
            if ids.is_empty() {
                ui.centered_and_justified(|ui| ui.label(RichText::new(tx::NO_POSTS.get(lang)).color(t.colors.muted)));
                return;
            }
            let body = ui.text_style_height(&egui::TextStyle::Body);
            let small = ui.text_style_height(&egui::TextStyle::Small);
            let pad = t.spacing.item_x;
            if gallery {
                // Grille de cartes : seules les rangées visibles sont mises en page,
                // et seules leurs publications demandent leur message de départ.
                let gap = t.spacing.media_gap.max(pad);
                let width = ui.available_width();
                // Une seule carte démesurée en fenêtre étroite : deux colonnes dès que deux cartes de CARD_MIN tiennent.
                let cols = (((width + gap) / (CARD + gap)).floor() as usize).max(if width >= 2.0 * CARD_MIN + gap { 2 } else { 1 });
                let card_w = ((width - gap * (cols as f32 - 1.0)) / cols as f32).floor();
                let thumb_h = (card_w / THUMB_ASPECT).round();
                let card_h = thumb_h + body + 2.0 * small + small + 4.0 * pad;
                let rows = ids.len().div_ceil(cols);
                ui.spacing_mut().item_spacing.y = gap;
                egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, card_h, rows, |ui, range| {
                    for row in range {
                        let (line, _) = ui.allocate_exact_size(egui::vec2(width, card_h), Sense::hover());
                        for (col, id) in ids.iter().skip(row * cols).take(cols).enumerate() {
                            let Some(th) = store.threads.get(*id) else { continue };
                            let rect = egui::Rect::from_min_size(egui::pos2(line.left() + col as f32 * (card_w + gap), line.top()), egui::vec2(card_w, card_h));
                            let resp = ui.interact(rect, ui.scope_id().with(("carte", *id)), Sense::click());
                            let k = super::motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
                            let r = t.r_card();
                            ui.painter().rect(rect, r, super::motion::mix(t.colors.bg1, t.colors.row_hover, k), t.border(), egui::StrokeKind::Inside);
                            let thumb = egui::Rect::from_min_size(rect.min, egui::vec2(card_w, thumb_h)).shrink(1.0);
                            let top_r = egui::CornerRadius { sw: 0, se: 0, ..r };
                            let starter = store.threads.starter(*id);
                            if starter.is_none() {
                                wanted.push(*id);
                            }
                            let drawn = starter.and_then(|s| s.image.as_ref()).is_some_and(|(url, w, h)| paint_thumb(ui, &self.images, thumb, top_r, (url, *w, *h)));
                            if !drawn {
                                // Sans image (ou pas encore là) : aplat et pictogramme du salon.
                                ui.painter().rect_filled(thumb, top_r, t.colors.bg3);
                                let side = (thumb_h * 0.3).round();
                                crate::theme::icons::paint(if info.media { Icon::Media } else { Icon::Forum }, ui.painter(), egui::Rect::from_center_size(thumb.center(), egui::Vec2::splat(side)), t.colors.faint);
                            }
                            let inner = egui::Rect::from_min_max(egui::pos2(rect.left() + pad, thumb.bottom() + pad), rect.max - egui::vec2(pad, pad));
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
                            child.spacing_mut().item_spacing.y = pad / 2.0;
                            child.set_clip_rect(inner.intersect(ui.clip_rect()));
                            let color = if th.joined || store.unread().contains(id) { t.colors.text_strong } else { t.colors.text };
                            child.add(egui::Label::new(RichText::new(&th.name).family(crate::ui::fonts::demi()).strong().color(color)).truncate().selectable(false));
                            // Extrait sur deux lignes au plus (la carte garde sa hauteur).
                            let extract = starter.map_or("", |s| s.text.as_str());
                            let mut job = egui::text::LayoutJob::simple(extract.replace('\n', " "), egui::TextStyle::Small.resolve(child.style()), t.colors.muted, inner.width());
                            job.wrap.max_rows = 2;
                            job.wrap.break_anywhere = false;
                            let galley = child.fonts_mut(|f| f.layout_job(job));
                            let (slot, _) = child.allocate_exact_size(egui::vec2(inner.width(), 2.0 * small), Sense::hover());
                            child.painter().galley(slot.min, galley, t.colors.muted);
                            child.horizontal(|ui| {
                                if th.pinned {
                                    ui.label(RichText::new(tx::PINNED_POST.get(lang)).small().color(t.colors.accent));
                                }
                                ui.label(RichText::new(p::replies(lang, u64::from(th.message_count))).small().color(t.colors.faint));
                                ui.label(RichText::new(time::clock(lang, snowflake_ms(th.activity()), true, h24)).small().color(t.colors.faint));
                            });
                            super::keys::focus_ring(ui, &t, &resp);
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &th.name));
                            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                open = Some(*id);
                            }
                        }
                    }
                });
                return;
            }
            // Deux lignes d'egui (chacune haute d'au moins `interact_size.y`), leur écart et les marges.
            let line = ui.spacing().interact_size.y;
            let row_h = body.max(line) + small.max(line) + ui.spacing().item_spacing.y + 2.0 * pad;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, row_h, ids.len(), |ui, range| {
                for id in &ids[range] {
                    let Some(th) = store.threads.get(*id) else { continue };
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                    let selected_bg = if resp.hovered() { t.colors.row_hover } else { t.colors.bg1 };
                    ui.painter().rect_filled(rect.shrink2(egui::vec2(0.0, pad / 4.0)), t.r_row(), selected_bg);
                    let inner = rect.shrink(pad);
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::top_down(egui::Align::Min)));
                    child.horizontal(|ui| {
                        if th.pinned {
                            ui.label(RichText::new(tx::PINNED_POST.get(lang)).small().color(t.colors.accent));
                        }
                        let color = if th.joined || store.unread().contains(id) { t.colors.text_strong } else { t.colors.text };
                        ui.add(egui::Label::new(RichText::new(&th.name).family(crate::ui::fonts::demi()).strong().color(color)).truncate().selectable(false));
                        for tag in th.applied_tags.iter().filter_map(|tg| info.tags.iter().find(|x| x.id == *tg)) {
                            ui.label(RichText::new(&tag.name).small().color(t.colors.muted));
                        }
                    });
                    child.horizontal(|ui| {
                        if let Some(name) = th.owner_id.and_then(|o| store.people.lock().user(o).map(|u| u.display_name().to_string())) {
                            ui.label(RichText::new(f::by_author(lang, name)).small().color(t.colors.faint));
                        }
                        ui.label(RichText::new(p::replies(lang, u64::from(th.message_count))).small().color(t.colors.faint));
                        let last = snowflake_ms(th.activity());
                        ui.label(RichText::new(time::clock(lang, last, true, h24)).small().color(t.colors.faint));
                        if th.archived {
                            ui.label(RichText::new(tx::ARCHIVED.get(lang)).small().color(t.colors.faint));
                        }
                    });
                    if resp.on_hover_text(tx::OPEN_THREAD.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        open = Some(*id);
                    }
                }
            });
        });

        if let Some((title, content, tags)) = publish
            && self.store.create_post(cid, &title, &content, tags)
            && let Some(d) = self.threads.forum.draft.as_mut()
        {
            d.sent = Some(self.store.threads.created);
        }
        if more {
            self.store.load_archived_threads(cid);
        }
        // Messages de départ des cartes visibles : demandés une fois, quelques-uns à la fois.
        for id in wanted {
            self.store.want_starter(id);
        }
        if let Some(id) = open {
            self.open_channel(id);
        }
    }
}
