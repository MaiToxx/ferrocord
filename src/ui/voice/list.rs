//! Salons vocaux et de conférence dans la liste des salons, avec, dessous,
//! les personnes connectées : avatar (contour quand elles parlent), nom,
//! icônes muet, sourdine, caméra, direct, main levée.

use super::icons::{self, VIcon};
use super::{App, VoiceAct};
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::state::model::Avatar;
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
    /// Icône du jeu de qui diffuse (et son nom, dit au survol).
    Game(crate::state::model::ImageRef, String),
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
            // Comme dans le client : ceux qui diffusent d'abord, puis par ordre alphabétique.
            let mut members: Vec<(&VoiceState, String, Avatar)> = v
                .members(c.id)
                .map(|s| {
                    let (name, avatar) = self.voice_who(s);
                    (s, name, avatar)
                })
                .collect();
            members.sort_by_cached_key(|(s, name, _)| member_order(s, name));
            for (s, name, avatar) in &members {
                self.voice_user_row(ui, s, name, avatar, indented, acts);
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
        // Les boutons de fin de ligne sont au-dessus de la ligne : elle reste survolée sous eux.
        let hovered = resp.contains_pointer();
        let p = ui.painter();
        if mine {
            p.rect_filled(rect, t.r_row(), col.selected);
        } else if hovered {
            p.rect_filled(rect, t.r_row(), col.row_hover);
        }
        let indent = if indented { t.spacing.channel_indent } else { 0.0 };
        let (icon_rect, text_left) = t.channel_row_parts(rect, indent);
        // Salon occupé : haut-parleur vert, comme dans le client.
        let occupied = self.store.voice.count(c.id) > 0;
        let icon_color = if occupied { col.online } else if mine { col.text_strong } else { col.faint };
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
        // Au survol : « Ouvrir la discussion » (bulle) et « Créer une invitation »
        // (personne +, seulement si CREATE_INSTANT_INVITE est connue et accordée), comme dans
        // le client. Les deux ouvrent les fonctions du menu du salon.
        if hovered || resp.has_focus() {
            use crate::i18n::table::chadmin::t as cx;
            use crate::ui::channel_settings::list::AdminPick;
            let tool = Tokens::icon_side(t.typography.button);
            let tools = [(self.store.voice.can(c.id, crate::state::Permissions::CREATE_INSTANT_INVITE), Icon::FriendAdd, cx::CREATE_INVITE, AdminPick::Invite { guild: c.guild_id, channel: c.id }), (true, Icon::Chat, cx::OPEN_CHAT, AdminPick::OpenChat(c.id))];
            for (i, (shown, icon, tip, pick)) in tools.into_iter().enumerate() {
                if !shown {
                    continue;
                }
                let at = crate::ui::util::pixel_square(ui, egui::pos2(right - tool, rect.center().y - tool / 2.0), tool);
                right = at.left() - t.spacing.item_y;
                let tip = tip.get(lang);
                let r = ui.interact(at.expand(t.spacing.item_y / 2.0), ui.scope_id().with(("outil-vocal", c.id, i)), Sense::click());
                ticons::paint(icon, ui.painter(), at, if r.hovered() || r.has_focus() { col.text_strong } else { col.muted });
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                if r.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    crate::ui::channel_settings::list::set_voice_pick(ui.ctx(), pick);
                }
            }
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

    fn voice_user_row(&self, ui: &mut egui::Ui, s: &VoiceState, name: &str, avatar: &Avatar, indented: bool, acts: &mut Vec<VoiceAct>) {
        let t = &self.theme;
        let col = &t.colors;
        let lang = self.lang;
        let speaking = self.store.voice.speaking.contains(&s.user_id);
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
            crate::ui::util::paint_avatar(ui, &self.images, t, slot, avatar, name);
            if speaking {
                let (gap, w) = t.speaking_ring();
                // Un écart de la couleur du fond sépare le contour de l'avatar.
                ui.painter().rect_stroke(slot.expand(gap), t.r_avatar(side + 2.0 * gap), egui::Stroke::new(w, col.online), egui::StrokeKind::Outside);
            }
            text_left = slot.right() + t.spacing.item_x;
        }
        // Icônes de fin de ligne, de droite à gauche : l'icône du jeu (quand la liste
        // des jeux déjà chargée la connaît), « EN DIRECT », puis les états.
        let game = if s.self_stream { self.voice_game(s.user_id) } else { None };
        let mut marks = Vec::new();
        if let Some(icon) = game.as_ref().and_then(|g| g.icon) {
            marks.push(Mark::Game(icon, game.as_ref().map(|g| g.name.clone()).unwrap_or_default()));
        }
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
        let mut right = rect.right() - t.spacing.item_x / 2.0;
        for (i, m) in marks.into_iter().enumerate() {
            let (r, tip) = match m {
                Mark::Drawn(icon, color, tip) => {
                    let r = Rect::from_center_size(egui::pos2(right - side / 2.0, rect.center().y), egui::Vec2::splat(side));
                    icons::paint(icon, ui.painter(), r, color);
                    (r, tip.to_string())
                }
                Mark::Live => (live_pill(ui, t, lang, right, rect.center().y), vt::LIVE_TIP.get(lang).to_string()),
                Mark::Game(icon, name) => {
                    let tile = Tokens::icon_side(t.typography.body);
                    let r = crate::ui::util::pixel_square(ui, egui::pos2(right - tile, rect.center().y - tile / 2.0), tile);
                    if !crate::ui::util::paint_image(ui, &self.images, r, Some(icon), egui::CornerRadius::same(4)) {
                        ui.painter().rect_filled(r, egui::CornerRadius::same(4), col.bg3);
                    }
                    (r, name)
                }
            };
            ui.interact(r, ui.scope_id().with(("voix-icone", s.user_id, i)), Sense::hover()).on_hover_text(tip);
            right = r.left() - t.spacing.item_x / 2.0;
        }
        let color = if speaking { col.text_strong } else { col.muted };
        let text_rect = Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2(right.max(text_left), rect.bottom()));
        // Tag de serveur de la personne, après son nom. Le nom complet d'abord, comme dans
        // le client : le tag ne reste que s'il tient, ou si la ligne est assez large pour deux.
        let chip = crate::ui::cosmetics::worn(ui.ctx(), s.guild_id, s.user_id).look.tag().map(|tag| crate::ui::cosmetics::TagChip::new(ui, t, &tag, col.text));
        let font = TextStyle::Body.resolve(ui.style());
        let (mut galley, mut spots) = crate::ui::emoji::truncated(ui, name, &font, color, text_rect.width());
        let chip = chip.filter(|c| galley.size().x + t.spacing.item_x + c.size.x <= text_rect.width() || text_rect.width() >= 3.0 * c.size.x);
        if let Some(c) = &chip
            && galley.size().x + t.spacing.item_x + c.size.x > text_rect.width()
        {
            (galley, spots) = crate::ui::emoji::truncated(ui, name, &font, color, (text_rect.width() - t.spacing.item_x - c.size.x).max(1.0));
        }
        let pos = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), text_rect).min;
        if let Some(chip) = chip {
            let size = chip.size;
            let at = egui::pos2(pos.x + galley.size().x + t.spacing.item_x, (rect.center().y - size.y / 2.0).round());
            chip.paint(ui, &self.images, t, Rect::from_min_size(at, size), col.bg3);
        }
        crate::ui::emoji::paint_spots(ui, pos, &galley, &spots);
        ui.painter().galley(pos, galley, color);
        crate::ui::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        // Carte d'aperçu d'un direct, au survol de qui diffuse (pas le mien).
        if s.self_stream && s.guild_id.is_some() && self.store.me.as_ref().is_none_or(|m| m.id != s.user_id) {
            self.stream_hover(ui, &resp, s, name, game.as_ref(), acts);
        }
        resp.context_menu(|ui| self.voice_user_menu(ui, s, acts));
    }

    /// Jeu annoncé par `user` (présence déjà reçue) : nom, début, icône si la
    /// liste des jeux déjà chargée connaît l'application.
    fn voice_game(&self, user: u64) -> Option<Game> {
        use crate::state::people::ActivityKind as K;
        let mut people = self.store.people.lock();
        let a = people.presence(user)?.activities.iter().find(|a| a.kind == K::Playing && !a.name.trim().is_empty())?;
        let icon = a.rich().and_then(|r| r.app_id).and_then(|id| self.store.activity.app_icon(id));
        Some(Game { name: a.name.clone(), start_ms: a.start_ms, icon })
    }

    /// Ouvre la carte d'aperçu du direct de `s` quand sa ligne `row` est
    /// survolée, et la garde tant que le pointeur reste sur la ligne, sur la
    /// carte ou entre les deux. « Regarder le stream » rejoint le salon et
    /// regarde le direct (fonctions existantes).
    fn stream_hover(&self, ui: &egui::Ui, row: &egui::Response, s: &VoiceState, name: &str, game: Option<&Game>, acts: &mut Vec<VoiceAct>) {
        let ctx = ui.ctx();
        let id = egui::Id::unique("ferrocord.vocal.apercu-direct");
        if row.hovered() && !ctx.input(|i| i.pointer.any_down()) {
            ctx.data_mut(|d| d.insert_temp(id, s.user_id));
        }
        if ctx.data(|d| d.get_temp::<u64>(id)) != Some(s.user_id) {
            return;
        }
        let (card, watch) = self.stream_card(ctx, id, row.rect, s, name, game);
        // Pont entre la ligne et la carte : le pointeur peut passer de l'une à l'autre.
        let bridge = Rect::from_min_max(egui::pos2(row.rect.right(), card.top().min(row.rect.top())), egui::pos2(card.left(), card.bottom().max(row.rect.bottom())));
        let inside = ctx.input(|i| i.pointer.hover_pos()).is_some_and(|p| row.rect.contains(p) || bridge.contains(p) || card.expand(4.0).contains(p));
        if watch {
            acts.push(VoiceAct::Watch { channel: s.channel_id, user: s.user_id });
        }
        if watch || !inside || row.context_menu_opened() {
            ctx.data_mut(|d| d.remove::<u64>(id));
            ctx.request_repaint();
        }
    }

    /// Carte d'aperçu d'un direct, à droite de la ligne `row` : « Streame » et
    /// l'étiquette EN DIRECT, le jeu (manette verte et chrono), le bouton vert
    /// « Regarder le stream ». Sans image : l'aperçu demanderait une requête
    /// (`GET /streams/{clé}/preview`) que ce lot n'ajoute pas. Rend sa place et
    /// si le bouton a été cliqué.
    fn stream_card(&self, ctx: &egui::Context, id: egui::Id, row: Rect, s: &VoiceState, name: &str, game: Option<&Game>) -> (Rect, bool) {
        let t = &self.theme;
        let col = &t.colors;
        let lang = self.lang;
        let pad = t.spacing.item_x * 2.0;
        let mut watch = false;
        let screen = ctx.content_rect();
        // Hauteur de l'image d'avant, pour ne pas sortir de la fenêtre par le bas.
        let last_h = ctx.data(|d| d.get_temp::<f32>(id.with("hauteur"))).unwrap_or(CARD_WIDTH * 0.5);
        let y = (row.top() - pad).clamp(screen.top() + 4.0, (screen.bottom() - 4.0 - last_h).max(screen.top() + 4.0));
        let at = egui::pos2(row.right() + t.spacing.item_x * 1.5, y);
        let area = egui::Area::new(id).order(egui::Order::Foreground).fixed_pos(at).show(ctx, |ui| {
            let frame = egui::Frame::new().fill(col.menu_bg).stroke(t.border()).corner_radius(t.r_card()).shadow(t.shadow(crate::theme::Elevation::Menu)).inner_margin(egui::Margin::same(pad as i8));
            frame.show(ui, |ui| {
                ui.set_width(CARD_WIDTH - 2.0 * pad);
                ui.spacing_mut().item_spacing.y = t.spacing.item_x * 1.5;
                // « Streame », et l'étiquette EN DIRECT à droite.
                let head_h = ui.text_style_height(&TextStyle::Body);
                let (head, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), head_h), Sense::hover());
                ui.painter().text(head.left_center(), Align2::LEFT_CENTER, vt::STREAMING.get(lang), FontId::proportional(t.typography.body - 2.0), col.muted);
                live_pill(ui, t, lang, head.right(), head.center().y);
                // Le jeu : icône, nom, manette verte et chrono.
                if let Some(g) = game {
                    let side = t.layout.small_avatar;
                    let (line, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), side), Sense::hover());
                    let tile = crate::ui::util::pixel_square(ui, line.min, side);
                    if !crate::ui::util::paint_image(ui, &self.images, tile, g.icon, egui::CornerRadius::same(6)) {
                        ui.painter().rect_filled(tile, egui::CornerRadius::same(6), col.bg3);
                        crate::ui::user_panel::paint_gamepad(ui.painter(), tile.shrink(side * 0.25), col.muted, col.bg3);
                    }
                    let left = tile.right() + t.spacing.item_x * 1.5;
                    let width = (line.right() - left).max(1.0);
                    let title = FontId::new(t.typography.body, crate::ui::fonts::demi());
                    let (galley, spots) = crate::ui::emoji::truncated(ui, &g.name, &title, col.text_strong, width);
                    let now = self.store.now();
                    let elapsed = g.start_ms.filter(|s| *s <= now).map(|s| now - s);
                    let small = FontId::monospace(t.typography.small + 1.0);
                    let sub_h = if elapsed.is_some() { ui.fonts_mut(|f| f.row_height(&small)) } else { 0.0 };
                    let top = line.center().y - (galley.size().y + sub_h) / 2.0;
                    let name_h = galley.size().y;
                    crate::ui::emoji::paint_spots(ui, egui::pos2(left, top), &galley, &spots);
                    ui.painter().galley(egui::pos2(left, top), galley, col.text_strong);
                    if let Some(ms) = elapsed {
                        let pad_side = Tokens::icon_side(t.typography.small);
                        let icon = crate::ui::util::pixel_square(ui, egui::pos2(left, top + name_h + (sub_h - pad_side) / 2.0), pad_side);
                        crate::ui::user_panel::paint_gamepad(ui.painter(), icon, col.online, col.menu_bg);
                        ui.painter().text(egui::pos2(icon.right() + t.spacing.item_y, top + name_h + sub_h / 2.0), Align2::LEFT_CENTER, crate::ui::friends_active::clock(ms), small, col.online);
                        // Le chrono avance : une image par seconde tant que la carte est ouverte.
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
                    }
                }
                // « Regarder le stream » : rejoint le salon et regarde le direct.
                let blocked = self.store.voice.me.is_none_or(|m| m.channel_id != s.channel_id) && self.store.voice_join_block(s.channel_id).is_some();
                let h = crate::ui::widgets::button_height(t);
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), if blocked { Sense::hover() } else { Sense::click() });
                let hot = resp.hovered() || resp.has_focus();
                let fill = if blocked { col.bg3 } else if hot { crate::ui::motion::mix(t.success_fill(), col.on_accent, 0.12) } else { t.success_fill() };
                let ink = if blocked { col.faint } else { col.on_accent };
                ui.painter().rect_filled(rect, t.r_row(), fill);
                let label = ui.painter().layout_no_wrap(vt::WATCH_STREAM.get(lang).to_string(), crate::ui::fonts::demi_font(t.typography.button), ink);
                let icon_side = Tokens::icon_side(t.typography.button);
                let total = icon_side + t.spacing.item_x + label.size().x;
                let x = rect.center().x - total / 2.0;
                ticons::paint(Icon::Screen, ui.painter(), crate::ui::util::pixel_square(ui, egui::pos2(x, rect.center().y - icon_side / 2.0), icon_side), ink);
                ui.painter().galley(egui::pos2(x + icon_side + t.spacing.item_x, rect.center().y - label.size().y / 2.0), label, ink);
                crate::ui::keys::focus_ring(ui, t, &resp);
                let label = format!("{} : {name}", vt::WATCH_STREAM.get(lang));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !blocked, &label));
                if !blocked && resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    watch = true;
                }
            });
        });
        let rect = area.response.rect;
        ctx.data_mut(|d| d.insert_temp(id.with("hauteur"), rect.height()));
        (rect, watch)
    }
}

/// Largeur de la carte d'aperçu d'un direct.
const CARD_WIDTH: f32 = 300.0;

/// Jeu annoncé par quelqu'un qui diffuse.
struct Game {
    name: String,
    start_ms: Option<i64>,
    icon: Option<crate::state::model::ImageRef>,
}

/// Rang d'un membre dans son salon vocal, comme dans le client : ceux qui
/// diffusent d'abord, puis l'ordre alphabétique des noms affichés.
fn member_order(s: &VoiceState, name: &str) -> (bool, String, u64) {
    (!s.self_stream, name.to_lowercase(), s.user_id)
}

/// Étiquette « EN DIRECT » : pilule rouge, texte gras, calée à droite sur
/// `right` et centrée sur `y`. Rend sa place.
fn live_pill(ui: &egui::Ui, t: &Tokens, lang: crate::i18n::Lang, right: f32, y: f32) -> Rect {
    let g = ui.painter().layout_no_wrap(vt::LIVE.get(lang).to_string(), crate::ui::fonts::demi_font(t.typography.small), t.colors.on_accent);
    let pad = (t.spacing.item_x * 0.75).round();
    let h = (g.size().y + 2.0).round();
    let r = Rect::from_min_size(egui::pos2(right - g.size().x - 2.0 * pad, (y - h / 2.0).round()), egui::vec2(g.size().x + 2.0 * pad, h));
    ui.painter().rect_filled(r, egui::CornerRadius::same((h / 2.0) as u8), t.danger_fill());
    ui.painter().galley(egui::pos2(r.left() + pad, r.center().y - g.size().y / 2.0), g, t.colors.on_accent);
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ordre des membres d'un salon : qui diffuse d'abord, puis alphabétique sans casse.
    #[test]
    fn streamers_first_then_by_name() {
        let st = |user: u64, stream: bool| VoiceState { user_id: user, channel_id: 1, self_stream: stream, ..Default::default() };
        let mut list = [(st(1, false), "Karim"), (st(2, false), "ferdinand"), (st(3, true), "Fanny"), (st(4, false), "Alba")];
        list.sort_by_cached_key(|(s, name)| member_order(s, name));
        let names: Vec<&str> = list.iter().map(|(_, n)| *n).collect();
        assert_eq!(names, ["Fanny", "Alba", "ferdinand", "Karim"]);
    }
}
