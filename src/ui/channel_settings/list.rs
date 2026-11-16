//! Ce que la liste des salons gagne avec le lot A2 : entrées du clic droit
//! (modifier, cloner, copier le lien, créer, supprimer), « + » des
//! catégories, menu du fond de la liste, et glisser-déposer des salons et
//! des catégories. Comme pour le rail : un repère montre où l'objet tombera,
//! rien n'est écrit avant le relâchement (Échap annule), et le dépôt devient
//! une seule requête `PATCH /guilds/{id}/channels`.

use super::App;
use crate::i18n::table::chadmin::t as tx;
use crate::state::admin::channels::Drop;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Pos2, Rect, Stroke};

/// Ce que l'utilisateur a choisi dans un menu de la liste.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum AdminPick {
    Edit(u64),
    Clone(u64),
    CopyLink { guild: u64, channel: u64 },
    Delete(u64),
    Create { guild: u64, parent: Option<u64>, category: bool },
    /// « Inviter des personnes » vers ce salon.
    Invite { guild: u64, channel: u64 },
    ToggleCategory { guild: u64, category: u64 },
    CollapseAll(u64),
    MarkCategoryRead { guild: u64, category: u64 },
    /// Discussion d'un salon vocal, sans s'y connecter.
    OpenChat(u64),
    /// Page Permissions des paramètres du salon (menu d'un salon vocal, lot V).
    Permissions(u64),
    /// Sourdine d'un salon vocal : `secs` (-1 : sans fin), ou `None` pour la lever.
    Mute { guild: u64, channel: u64, secs: Option<i64> },
}

/// Durées de la sourdine d'un salon vocal (secondes ; -1 : jusqu'à ce qu'on la retire).
const MUTE_SPANS: [(i64, crate::i18n::Text); 6] = {
    use crate::i18n::table::s::t as st;
    [(15 * 60, st::MUTE_15M), (60 * 60, st::MUTE_1H), (3 * 60 * 60, st::MUTE_3H), (8 * 60 * 60, st::MUTE_8H), (24 * 60 * 60, st::MUTE_24H), (-1, st::MUTE_FOREVER)]
};

/// Ce qu'un choix de sourdine écrit : la sourdine et la fenêtre annoncée à Discord.
pub(in crate::ui) fn mute_change(now: i64, secs: Option<i64>) -> (crate::state::usersettings::notif::Mute, i64) {
    use crate::state::usersettings::notif::Mute;
    match secs {
        None => (Mute::default(), 0),
        Some(s) => (Mute { on: true, until_ms: (s > 0).then(|| now.saturating_add(s.saturating_mul(1000))) }, s),
    }
}

/// Choix fait dans le menu d'un salon vocal : ses lignes sont dessinées par
/// le vocal (`voice/list.rs`), la liste des salons le reprend à la fin de l'image.
fn voice_pick_id() -> egui::Id {
    egui::Id::unique("ferrocord.salons.choix-vocal")
}

/// Dépose un choix fait sur la ligne d'un salon vocal (ses boutons de survol, `voice/list.rs`).
pub(in crate::ui) fn set_voice_pick(ctx: &egui::Context, pick: AdminPick) {
    ctx.data_mut(|d| d.insert_temp(voice_pick_id(), pick));
    ctx.request_repaint();
}

/// Ligne affichée, pour le glisser-déposer : (rectangle, salon, catégorie ?,
/// rangé dans une catégorie ?).
pub(in crate::ui) type Hit = (Rect, u64, bool, bool);

/// Repère de dépôt.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) enum Mark {
    Line { y: f32, x: (f32, f32) },
    Ring(Rect),
}

/// Dépôt de `dragged` (catégorie si `category`) au point `pos`.
pub(in crate::ui) fn resolve(dragged: u64, category: bool, pos: Pos2, hits: &[Hit]) -> Option<(Drop, Mark)> {
    let (rect, id, is_cat, in_cat) = *hits.iter().find(|(r, ..)| r.y_range().contains(pos.y))?;
    if id == dragged {
        return None;
    }
    let top = (pos.y - rect.top()) / rect.height().max(1.0) < 0.5;
    let line = |top: bool| Mark::Line { y: if top { rect.top() } else { rect.bottom() }, x: (rect.left(), rect.right()) };
    match (category, is_cat) {
        // Salon sur une catégorie : en tête de celle-ci.
        (false, true) => Some((Drop::Into(id), Mark::Ring(rect))),
        (false, false) | (true, true) => Some(if top { (Drop::Before(id), line(true)) } else { (Drop::After(id), line(false)) }),
        // Catégorie sur un salon : après la catégorie de ce salon (le magasin la trouve).
        (true, false) => in_cat.then_some((Drop::After(id), line(false))),
    }
}

#[derive(Default)]
pub(in crate::ui) struct ChanDnd {
    dragging: Option<(u64, bool)>,
    target: Option<(Drop, Mark)>,
}

impl ChanDnd {
    pub(in crate::ui) fn start(&mut self, id: u64, category: bool) {
        self.dragging = Some((id, category));
        self.target = None;
    }

    /// À la fin de l'image de la liste : repère dessiné, et le dépôt si le
    /// bouton vient d'être relâché au-dessus d'une place valable.
    pub(in crate::ui) fn finish(&mut self, ui: &egui::Ui, t: &Tokens, hits: &[Hit]) -> Option<(u64, Drop)> {
        let (dragged, category) = self.dragging?;
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            *self = ChanDnd::default();
            return None;
        }
        let (pos, down) = ui.input(|i| (i.pointer.latest_pos(), i.pointer.any_down()));
        if let Some(p) = pos {
            self.target = resolve(dragged, category, p, hits);
        }
        if !down {
            let drop = self.target.take().map(|(d, _)| (dragged, d));
            self.dragging = None;
            return drop;
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let stroke = Stroke::new(t.shapes.unread_ring, t.colors.accent);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, ui.scope_id().with("depot-salon")));
        match self.target {
            Some((_, Mark::Line { y, x })) => {
                painter.line_segment([egui::pos2(x.0, y), egui::pos2(x.1, y)], stroke);
            }
            Some((_, Mark::Ring(r))) => {
                painter.rect_stroke(r, t.r_row(), stroke, egui::StrokeKind::Inside);
            }
            None => {}
        }
        None
    }
}

impl App {
    /// Entrées d'administration du menu d'une ligne de la liste.
    pub(in crate::ui) fn channel_admin_menu(&self, ui: &mut egui::Ui, guild: u64, channel: u64, category: bool) -> Option<AdminPick> {
        let lang = self.lang;
        let manage = self.store.can_manage_channels(guild);
        let edit = self.store.can_edit_channel(channel);
        let mut pick = None;
        let mut any = false;
        if edit {
            any = true;
            let label = if category { tx::EDIT_CATEGORY } else { tx::EDIT_CHANNEL };
            if crate::ui::menu::item(ui, &self.theme, label.get(lang), Some(crate::theme::icons::Icon::Gear), false).clicked() {
                pick = Some(AdminPick::Edit(channel));
            }
        }
        if manage && category {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, tx::CREATE_CHANNEL.get(lang), Some(crate::theme::icons::Icon::Plus), false).clicked() {
                pick = Some(AdminPick::Create { guild, parent: Some(channel), category: false });
            }
        }
        if manage && !category {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, tx::CLONE_CHANNEL.get(lang), Some(crate::theme::icons::Icon::Copy), false).clicked() {
                pick = Some(AdminPick::Clone(channel));
            }
        }
        if !category {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, tx::COPY_LINK.get(lang), Some(crate::theme::icons::Icon::Link), false).clicked() {
                pick = Some(AdminPick::CopyLink { guild, channel });
            }
        }
        if edit {
            let label = if category { tx::DELETE_CATEGORY } else { tx::DELETE_CHANNEL };
            if crate::ui::menu::item(ui, &self.theme, label.get(lang), Some(crate::theme::icons::Icon::Trash), true).clicked() {
                pick = Some(AdminPick::Delete(channel));
            }
        }
        if any {
            crate::ui::menu::separator(ui, &self.theme);
        }
        if pick.is_some() {
            ui.close();
        }
        pick
    }

    /// Menu d'une catégorie : replier, tout replier, marquer comme lu.
    pub(in crate::ui) fn category_menu(&self, ui: &mut egui::Ui, guild: u64, category: u64) -> Option<AdminPick> {
        let lang = self.lang;
        let mut pick = None;
        if crate::ui::menu::item(ui, &self.theme, crate::i18n::table::social::t::MARK_READ.get(lang), Some(crate::theme::icons::Icon::Check), false).clicked() {
            pick = Some(AdminPick::MarkCategoryRead { guild, category });
        }
        crate::ui::menu::separator(ui, &self.theme);
        let collapsed = self.store.list_rules(guild).collapsed(category);
        let label = if collapsed { tx::EXPAND_CATEGORY } else { tx::COLLAPSE_CATEGORY };
        if crate::ui::menu::item(ui, &self.theme, label.get(lang), Some(if collapsed { crate::theme::icons::Icon::ChevronDown } else { crate::theme::icons::Icon::ChevronRight }), false).clicked() {
            pick = Some(AdminPick::ToggleCategory { guild, category });
        }
        if crate::ui::menu::item(ui, &self.theme, tx::COLLAPSE_ALL.get(lang), Some(crate::theme::icons::Icon::ChevronUp), false).clicked() {
            pick = Some(AdminPick::CollapseAll(guild));
        }
        crate::ui::menu::separator(ui, &self.theme);
        if pick.is_some() {
            ui.close();
        }
        pick
    }

    /// « Inviter des personnes » du menu d'un salon, s'il est permis d'y inviter.
    pub(in crate::ui) fn invite_entry(&self, ui: &mut egui::Ui, guild: u64, channel: u64) -> Option<AdminPick> {
        if !self.can_invite_in(channel) {
            return None;
        }
        let clicked = crate::ui::menu::item(ui, &self.theme, tx::INVITE_PEOPLE.get(self.lang), Some(crate::theme::icons::Icon::FriendAdd), false).clicked();
        if clicked {
            ui.close();
        }
        clicked.then_some(AdminPick::Invite { guild, channel })
    }

    /// Puis-je créer une invitation vers ce salon ? (Refus certain seulement : sinon Discord répondra.)
    pub(in crate::ui) fn can_invite_in(&self, channel: u64) -> bool {
        self.store.can(channel, crate::state::Permissions::CREATE_INSTANT_INVITE) != crate::state::perms::Verdict::Refuse
    }

    /// Menu d'un salon vocal (appelé par sa ligne), dans l'ordre du client :
    /// discussion, inviter, sourdine, modifier, permissions, copier le lien, supprimer.
    pub(in crate::ui) fn voice_channel_menu(&self, ui: &mut egui::Ui, guild: u64, channel: u64) {
        use crate::i18n::table::s::{f as sf, t as st};
        use crate::theme::icons::Icon;
        let lang = self.lang;
        let t = &self.theme;
        let mut pick = None;
        if crate::ui::menu::item(ui, t, tx::OPEN_CHAT.get(lang), Some(Icon::Chat), false).clicked() {
            pick = Some(AdminPick::OpenChat(channel));
        }
        if let Some(p) = self.invite_entry(ui, guild, channel) {
            pick = Some(p);
        }
        crate::ui::menu::separator(ui, t);
        let mute = self.store.account.notifs.get(Some(guild)).channel(channel).mute;
        ui.add_enabled_ui(!self.store.account.guild_busy(Some(guild)), |ui| {
            if mute.active(self.store.now()) {
                let label = match mute.until_ms {
                    Some(ms) => sf::unmute_until(lang, crate::i18n::time::until(lang, ms, self.h24())),
                    None => st::UNMUTE.get(lang).to_string(),
                };
                if crate::ui::menu::item(ui, t, &label, Some(Icon::Bell), false).clicked() {
                    pick = Some(AdminPick::Mute { guild, channel, secs: None });
                }
            } else {
                crate::ui::menu::submenu(ui, t, st::MUTE_CHANNEL.get(lang), |ui| {
                    for (secs, text) in MUTE_SPANS {
                        if crate::ui::menu::item(ui, t, text.get(lang), None, false).clicked() {
                            pick = Some(AdminPick::Mute { guild, channel, secs: Some(secs) });
                        }
                    }
                });
            }
        });
        let edit = self.store.can_edit_channel(channel);
        if edit {
            crate::ui::menu::separator(ui, t);
            if crate::ui::menu::item(ui, t, tx::EDIT_CHANNEL.get(lang), Some(Icon::Gear), false).clicked() {
                pick = Some(AdminPick::Edit(channel));
            }
            if crate::ui::menu::item(ui, t, tx::PAGE_PERMISSIONS.get(lang), Some(Icon::Lock), false).clicked() {
                pick = Some(AdminPick::Permissions(channel));
            }
        }
        crate::ui::menu::separator(ui, t);
        if crate::ui::menu::item(ui, t, tx::COPY_LINK.get(lang), Some(Icon::Link), false).clicked() {
            pick = Some(AdminPick::CopyLink { guild, channel });
        }
        if edit && crate::ui::menu::item(ui, t, tx::DELETE_CHANNEL.get(lang), Some(Icon::Trash), true).clicked() {
            pick = Some(AdminPick::Delete(channel));
        }
        if let Some(p) = pick {
            ui.ctx().data_mut(|d| d.insert_temp(voice_pick_id(), p));
            ui.ctx().request_repaint();
            ui.close();
        }
    }

    /// Choix laissé par le menu d'un salon vocal, pris une fois.
    pub(in crate::ui) fn take_voice_pick(&self, ctx: &egui::Context) -> Option<AdminPick> {
        ctx.data_mut(|d| {
            let pick = d.get_temp::<AdminPick>(voice_pick_id());
            d.remove::<AdminPick>(voice_pick_id());
            pick
        })
    }

    /// Menu du fond de la liste : créer un salon ou une catégorie.
    pub(in crate::ui) fn channel_list_menu(&self, ui: &mut egui::Ui, guild: u64) -> Option<AdminPick> {
        let lang = self.lang;
        let mut pick = None;
        if crate::ui::menu::item(ui, &self.theme, tx::CREATE_CHANNEL.get(lang), Some(crate::theme::icons::Icon::Plus), false).clicked() {
            pick = Some(AdminPick::Create { guild, parent: None, category: false });
        }
        if crate::ui::menu::item(ui, &self.theme, tx::CREATE_CATEGORY.get(lang), Some(crate::theme::icons::Icon::Folder), false).clicked() {
            pick = Some(AdminPick::Create { guild, parent: None, category: true });
        }
        if pick.is_some() {
            ui.close();
        }
        pick
    }

    /// « + » à droite de l'en-tête d'une catégorie : créer un salon dedans.
    pub(in crate::ui) fn category_plus(&self, ui: &mut egui::Ui, head: Rect, category: u64) -> bool {
        let t = &self.theme;
        // Centré sur le titre de la catégorie, sur l'axe des pastilles de
        // mention des lignes : un bouton-icône ordinaire, plus haut que le
        // titre, descendait sous sa ligne.
        let tip = tx::CREATE_CHANNEL.get(self.lang);
        let icon = Tokens::icon_side(t.typography.button);
        let center = egui::pos2(head.right() - t.spacing.mention_inset, head.center().y);
        let hit = Rect::from_center_size(center, egui::Vec2::splat(head.height() + t.spacing.item_y));
        let resp = ui.interact(hit, ui.scope_id().with(("plus-categorie", category)), egui::Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        if hot {
            ui.painter().rect_filled(hit, t.r_row(), t.colors.row_hover);
        }
        let at = super::super::util::pixel_square(ui, center - egui::Vec2::splat(icon / 2.0), icon);
        crate::theme::icons::paint(Icon::Plus, ui.painter(), at, if hot { t.colors.text_strong } else { t.colors.muted });
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        resp.on_hover_text(tip).clicked()
    }

    /// Icônes de fin de ligne d'un salon survolé ou ouvert : créer une
    /// invitation, modifier le salon. La ligne leur a gardé la place
    /// (`row_tools_width`).
    pub(in crate::ui) fn row_tools(&self, ui: &mut egui::Ui, row: Rect, guild: u64, channel: u64, invite: bool, edit: bool) -> Option<AdminPick> {
        let t = &self.theme;
        let side = row_tool_side(t);
        let mut right = row.right() - t.spacing.item_x;
        let mut pick = None;
        let tools = [(edit, tx::EDIT_CHANNEL, AdminPick::Edit(channel), true), (invite, tx::CREATE_INVITE, AdminPick::Invite { guild, channel }, false)];
        for (shown, tip, choice, gear) in tools {
            if !shown {
                continue;
            }
            let at = super::super::util::pixel_square(ui, egui::pos2(right - side, row.center().y - side / 2.0), side);
            right = at.left() - t.spacing.item_y;
            let tip = tip.get(self.lang);
            let resp = ui.interact(at.expand(t.spacing.item_y / 2.0), ui.scope_id().with(("outil-salon", channel, gear)), egui::Sense::click());
            let color = if resp.hovered() || resp.has_focus() { t.colors.text_strong } else { t.colors.muted };
            if gear {
                crate::theme::icons::paint(Icon::Gear, ui.painter(), at, color);
            } else {
                crate::theme::icons::paint(Icon::FriendAdd, ui.painter(), at, color);
            }
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
            if resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                pick = Some(choice);
            }
        }
        pick
    }

    pub(in crate::ui) fn apply_admin_pick(&mut self, pick: AdminPick) {
        match pick {
            AdminPick::Edit(c) => {
                self.open_channel_settings(c, super::Page::Overview);
            }
            AdminPick::Clone(c) => self.open_clone_channel(c),
            AdminPick::CopyLink { guild, channel } => {
                self.ctx.copy_text(super::channel_link(guild, channel));
                self.notify(tx::LINK_COPIED);
            }
            AdminPick::Delete(c) => self.ask_delete_channel(c),
            AdminPick::Create { guild, parent, category } => self.open_create_channel(guild, parent, category),
            AdminPick::Invite { guild, channel } => self.open_invite_in(guild, channel),
            AdminPick::ToggleCategory { guild, category } => self.store.toggle_category(guild, category),
            AdminPick::CollapseAll(guild) => self.store.collapse_all_categories(guild),
            AdminPick::MarkCategoryRead { guild, category } => self.store.mark_category_read(guild, category),
            AdminPick::OpenChat(c) => self.open_channel(c),
            AdminPick::Permissions(c) => {
                self.open_channel_settings(c, super::Page::Permissions);
            }
            AdminPick::Mute { guild, channel, secs } => {
                let (mute, window) = mute_change(self.store.now(), secs);
                if let Err(r) = self.store.account.change_notif(Some(guild), Some(channel), crate::state::usersettings::notif::Change::Mute(mute), window) {
                    self.refused(r);
                }
            }
        }
    }

    /// Dépôt terminé dans la liste : la seule écriture du glisser-déposer.
    pub(in crate::ui) fn drop_channel(&mut self, guild: u64, (dragged, drop): (u64, Drop)) {
        self.store.move_channel(guild, dragged, drop);
    }
}

/// Côté d'une icône de fin de ligne.
fn row_tool_side(t: &Tokens) -> f32 {
    Tokens::icon_side(t.typography.button)
}

/// Place à garder à droite du nom pour `count` icônes de fin de ligne.
pub(in crate::ui) fn row_tools_width(t: &Tokens, count: u8) -> f32 {
    f32::from(count) * (row_tool_side(t) + t.spacing.item_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::{pos2, vec2};

    fn hits() -> Vec<Hit> {
        let r = |i: usize| Rect::from_min_size(pos2(0.0, i as f32 * 30.0), vec2(200.0, 30.0));
        // #1 hors catégorie, catégorie 10, #11 dedans.
        vec![(r(0), 1, false, false), (r(1), 10, true, false), (r(2), 11, false, true)]
    }

    #[test]
    fn drops_in_the_channel_list() {
        let at = |y| pos2(50.0, y);
        assert_eq!(resolve(11, false, at(5.0), &hits()).map(|x| x.0), Some(Drop::Before(1)));
        assert_eq!(resolve(11, false, at(25.0), &hits()).map(|x| x.0), Some(Drop::After(1)));
        assert!(matches!(resolve(1, false, at(45.0), &hits()), Some((Drop::Into(10), Mark::Ring(_)))));
        assert_eq!(resolve(11, false, at(75.0), &hits()), None, "sur lui-même");
        assert_eq!(resolve(11, false, at(500.0), &hits()), None, "hors de la liste");
        assert_eq!(resolve(10, true, at(5.0), &hits()), None, "catégorie sur un salon hors catégorie");
        assert_eq!(resolve(10, true, at(65.0), &hits()).map(|x| x.0), Some(Drop::After(11)), "le magasin n'y verra aucun changement");
    }
}

#[cfg(test)]
mod mute_tests {
    use super::*;

    #[test]
    fn la_sourdine_d_un_salon_vocal_a_une_fin_ou_non() {
        let (m, w) = mute_change(1_000, Some(900));
        assert!(m.on && m.until_ms == Some(901_000) && w == 900);
        let (m, w) = mute_change(1_000, Some(-1));
        assert!(m.on && m.until_ms.is_none() && w == -1);
        let (m, w) = mute_change(1_000, None);
        assert!(!m.on && w == 0);
    }
}
