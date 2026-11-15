//! Salons vocaux et de conférence dans la liste des salons, avec, dessous,
//! les personnes connectées : avatar (contour quand elles parlent), nom,
//! icônes muet, sourdine, caméra, direct, main levée.

use super::icons::{self, VIcon};
use super::{App, VoiceAct};
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::state::voice::{JoinBlock, VoiceChannel, VoiceMod, VoiceState};
use crate::theme::{icons as ticons, icons::Icon, Tokens};
use eframe::egui::{self, Align2, Color32, FontId, Rect, Sense, TextStyle};

/// Personne glissée depuis sa ligne vers un autre salon vocal (F21).
#[derive(Clone, Copy)]
struct DraggedMember(u64);

/// Icône de fin de ligne : dessin, couleur, explication.
enum Mark {
    Drawn(VIcon, Color32, &'static str),
    Live,
}

impl App {
    /// Salons vocaux de `guild` rangés sous `parent` (`None` : hors
    /// catégorie), puis leur statut et leurs occupants. Les gestes vont dans
    /// `acts` ; un clic rejoint le salon et rend son identifiant pour ouvrir
    /// sa discussion (en-tête de salon vocal ou de conférence).
    pub(in crate::ui) fn voice_rows(&self, ui: &mut egui::Ui, guild: u64, parent: Option<u64>, indented: bool, acts: &mut Vec<VoiceAct>) -> Option<u64> {
        let v = &self.store.voice;
        let mut clicked = None;
        for c in v.channels(guild).iter().filter(|c| c.parent == parent && !v.hidden(c.id)) {
            let row = self.voice_channel_row(ui, c, indented);
            // Menu du salon vocal (phase 10, lot S : `channel_settings/list.rs`).
            row.context_menu(|ui| self.voice_channel_menu(ui, guild, c.id));
            // Un membre glissé sur un autre salon vocal (lot FF2, F21) : déplacement,
            // confirmé comme depuis le menu, si j'en ai le droit.
            if let Some(m) = row.dnd_hover_payload::<DraggedMember>()
                && self.store.voice_can_moderate(m.0, VoiceMod::Move(c.id))
            {
                ui.painter().rect_stroke(row.rect, self.theme.r_row(), egui::Stroke::new(self.theme.shapes.border.max(1.0), self.theme.colors.accent), egui::StrokeKind::Inside);
            }
            if let Some(m) = row.dnd_release_payload::<DraggedMember>()
                && self.store.voice_can_moderate(m.0, VoiceMod::Move(c.id))
            {
                acts.push(VoiceAct::Moderate { user: m.0, action: VoiceMod::Move(c.id) });
            }
            if row.clicked() {
                acts.push(VoiceAct::Join(c.id));
                clicked = Some(c.id);
            }
            if let Some(status) = self.store.stage.status(c.id) {
                let t = &self.theme;
                ui.horizontal(|ui| {
                    let indent = if indented { t.spacing.channel_indent } else { 0.0 };
                    let (_, left) = t.channel_row_parts(ui.max_rect(), indent);
                    ui.add_space(left - ui.max_rect().left());
                    ui.add(egui::Label::new(egui::RichText::new(status).small().color(t.colors.faint)).truncate());
                });
            }
            for s in v.members(c.id) {
                self.voice_user_row(ui, s, indented, acts);
            }
        }
        clicked
    }

    fn voice_channel_row(&self, ui: &mut egui::Ui, c: &VoiceChannel, indented: bool) -> egui::Response {
        let t = &self.theme;
        let col = &t.colors;
        let lang = self.lang;
        let mine = self.store.voice.me.is_some_and(|m| m.channel_id == c.id);
        let block = self.store.voice_join_block(c.id);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), Sense::click());
        let hovered = resp.hovered();
        let p = ui.painter();
        if mine {
            p.rect_filled(rect, t.r_row(), col.selected);
        } else if hovered {
            p.rect_filled(rect, t.r_row(), col.row_hover);
        }
        let indent = if indented { t.spacing.channel_indent } else { 0.0 };
        let (icon_rect, text_left) = t.channel_row_parts(rect, indent);
        let icon_color = if mine { col.text_strong } else { col.faint };
        if c.stage {
            ticons::paint(Icon::Megaphone, p, icon_rect, icon_color);
        } else {
            icons::paint(VIcon::Speaker, p, icon_rect, icon_color);
        }
        // Fin de ligne : cadenas (pas de CONNECT) ou remplissage d'un salon limité.
        let mut right = rect.right() - t.spacing.item_x;
        let side = Tokens::icon_side(t.typography.small);
        if block == Some(JoinBlock::NoConnect) {
            let r = Rect::from_center_size(egui::pos2(right - side / 2.0, rect.center().y), egui::Vec2::splat(side));
            ticons::paint(Icon::Lock, p, r, col.faint);
            right = r.left() - t.spacing.item_x;
        } else if c.user_limit > 0 && !c.stage {
            let full = block == Some(JoinBlock::Full);
            let text = vf::limit(lang, self.store.voice.count(c.id), c.user_limit);
            let g = p.layout_no_wrap(text, FontId::proportional(t.typography.small), if full { col.danger } else { col.muted });
            let pos = egui::pos2(right - g.size().x, rect.center().y - g.size().y / 2.0);
            right = pos.x - t.spacing.item_x;
            p.galley(pos, g, col.muted);
        }
        let text_color = if mine { col.text_strong } else if hovered { col.text } else { col.muted };
        let text_rect = Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2(right.max(text_left), rect.bottom()));
        let (galley, spots) = crate::ui::emoji::truncated(ui, &c.name, &TextStyle::Body.resolve(ui.style()), text_color, text_rect.width());
        let pos = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), text_rect).min;
        crate::ui::emoji::paint_spots(ui, pos, &galley, &spots);
        ui.painter().galley(pos, galley, text_color);
        crate::ui::keys::focus_ring(ui, t, &resp);
        let name = c.name.clone();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, block.is_none(), &name));
        let tip = match block {
            Some(JoinBlock::NoConnect) => vt::LOCKED_TIP,
            Some(JoinBlock::Full) => vt::FULL_TIP,
            _ if c.stage => vt::STAGE_TIP,
            _ => vt::VOICE_TIP,
        };
        let resp = resp.on_hover_text(tip.get(lang));
        if block.is_none() { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
    }

    fn voice_user_row(&self, ui: &mut egui::Ui, s: &VoiceState, indented: bool, acts: &mut Vec<VoiceAct>) {
        let t = &self.theme;
        let col = &t.colors;
        let lang = self.lang;
        let speaking = self.store.voice.speaking.contains(&s.user_id);
        let (name, avatar) = self.voice_who(s);
        // Glissable vers un autre salon vocal quand je peux déplacer des membres.
        let movable = s.guild_id.is_some() && self.store.voice.can(s.channel_id, VoiceMod::Move(0).permission());
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), if movable { Sense::click_and_drag() } else { Sense::click() });
        if movable {
            resp.dnd_set_drag_payload(DraggedMember(s.user_id));
        }
        if resp.hovered() {
            ui.painter().rect_filled(rect, t.r_row(), col.row_hover);
        }
        // Sous le nom du salon : l'avatar commence où le nom commençait.
        let indent = if indented { t.spacing.channel_indent } else { 0.0 };
        let (_, start) = t.channel_row_parts(rect, indent);
        let mut text_left = start;
        if t.avatars_shown() {
            // Plus petit que la ligne : le contour de qui parle tient dedans,
            // sans toucher celui de la ligne voisine.
            let side = t.voice_avatar_in(rect.height());
            let slot = crate::ui::util::pixel_square(ui, egui::pos2(start, rect.center().y - side / 2.0), side);
            crate::ui::util::paint_avatar(ui, &self.images, t, slot, &avatar, &name);
            if speaking {
                let (gap, w) = t.speaking_ring();
                // Un écart de la couleur du fond sépare le contour de l'avatar.
                ui.painter().rect_stroke(slot.expand(gap), t.r_avatar(side + 2.0 * gap), egui::Stroke::new(w, col.online), egui::StrokeKind::Outside);
            }
            text_left = slot.right() + t.spacing.item_x;
        }
        // Icônes de fin de ligne, de droite à gauche.
        let mut marks = Vec::new();
        if s.self_stream {
            marks.push(Mark::Live);
        }
        if s.self_video {
            marks.push(Mark::Drawn(VIcon::Camera, col.muted, vt::VIDEO.get(lang)));
        }
        if s.deaf {
            marks.push(Mark::Drawn(VIcon::HeadphonesOff, col.danger, vt::SERVER_DEAFENED.get(lang)));
        } else if s.self_deaf {
            marks.push(Mark::Drawn(VIcon::HeadphonesOff, col.muted, vt::SELF_DEAFENED.get(lang)));
        }
        if s.mute {
            marks.push(Mark::Drawn(VIcon::MicOff, col.danger, vt::SERVER_MUTED.get(lang)));
        } else if s.self_mute && !s.self_deaf {
            marks.push(Mark::Drawn(VIcon::MicOff, col.muted, vt::SELF_MUTED.get(lang)));
        }
        if self.local.prefs.voice.user(s.user_id).muted {
            marks.push(Mark::Drawn(VIcon::MicOff, col.faint, vt::LOCAL_MUTE.get(lang)));
        }
        if s.request_to_speak.is_some() {
            marks.push(Mark::Drawn(VIcon::Hand, col.text, vt::HAND.get(lang)));
        }
        let side = Tokens::icon_side(t.typography.small);
        let mut right = rect.right() - t.spacing.item_x;
        for (i, m) in marks.into_iter().enumerate() {
            let (r, tip) = match m {
                Mark::Drawn(icon, color, tip) => {
                    let r = Rect::from_center_size(egui::pos2(right - side / 2.0, rect.center().y), egui::Vec2::splat(side));
                    icons::paint(icon, ui.painter(), r, color);
                    (r, tip)
                }
                Mark::Live => {
                    let g = ui.painter().layout_no_wrap(vt::LIVE.get(lang).to_string(), FontId::proportional(t.typography.small), col.on_accent);
                    let pad = t.spacing.item_x / 2.0;
                    let r = Rect::from_min_size(egui::pos2(right - g.size().x - 2.0 * pad, rect.center().y - g.size().y / 2.0), g.size() + egui::vec2(2.0 * pad, 0.0));
                    ui.painter().rect_filled(r, t.r_row(), t.danger_fill());
                    ui.painter().galley(r.min + egui::vec2(pad, 0.0), g, col.on_accent);
                    (r, vt::LIVE_TIP.get(lang))
                }
            };
            ui.interact(r, ui.scope_id().with(("voix-icone", s.user_id, i)), Sense::hover()).on_hover_text(tip);
            right = r.left() - t.spacing.item_x / 2.0;
        }
        let color = if speaking { col.text_strong } else { col.muted };
        let text_rect = Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2(right.max(text_left), rect.bottom()));
        let (galley, spots) = crate::ui::emoji::truncated(ui, &name, &TextStyle::Body.resolve(ui.style()), color, text_rect.width());
        let pos = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), text_rect).min;
        crate::ui::emoji::paint_spots(ui, pos, &galley, &spots);
        ui.painter().galley(pos, galley, color);
        crate::ui::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
        resp.context_menu(|ui| self.voice_user_menu(ui, s, acts));
    }
}
