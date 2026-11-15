//! Clic sur une mention de salon ou de rôle (F3). Une mention de salon ouvre
//! ce salon ; une mention de rôle ouvre, à côté d'elle, la liste des membres
//! qui portent ce rôle, comme le client officiel. Cette liste ne fait aucune
//! requête : elle montre les membres déjà connus localement (liste des
//! membres, auteurs, profils vus).

use super::util::{paint_avatar, pixel_square};
use super::App;
use crate::i18n::table::ff::{f as ff, t as ft};
use crate::state::model::Avatar;
use eframe::egui::{self, Id, RichText, Sense};

fn channel_id() -> Id {
    Id::unique("ferrocord.mention.salon")
}

fn role_id() -> Id {
    Id::unique("ferrocord.mention.rôle")
}

/// Fenêtre d'un rôle mentionné, ouverte à côté de la mention.
#[derive(Clone)]
struct RolePop {
    role: u64,
    name: String,
    anchor: egui::Rect,
    /// Ouverte à cette image : le clic qui l'ouvre ne la referme pas.
    fresh: bool,
}

/// Mention de salon cliquée : le salon est ouvert à l'image suivante.
pub(super) fn go_channel(ctx: &egui::Context, id: u64) {
    ctx.data_mut(|d| d.insert_temp(channel_id(), id));
    ctx.request_repaint();
}

/// Mention de rôle cliquée : la liste de ses membres s'ouvre à côté de `anchor`.
pub(super) fn open_role(ctx: &egui::Context, role: u64, name: &str, anchor: egui::Rect) {
    ctx.data_mut(|d| d.insert_temp(role_id(), RolePop { role, name: name.trim_start_matches('@').to_string(), anchor, fresh: true }));
    ctx.request_repaint();
}

/// Lignes montrées au plus (le reste est annoncé par un compte).
const MAX_ROWS: usize = 200;

impl App {
    /// Aperçu hors ligne : la fenêtre d'un rôle, comme après un clic sur sa mention.
    pub fn preview_role_mention(&self, role: u64, name: &str) {
        let at = self.ctx.content_rect().center();
        open_role(&self.ctx, role, name, egui::Rect::from_min_size(at, egui::vec2(80.0, 20.0)));
    }

    /// À chaque image : mention de salon cliquée, fenêtre d'un rôle.
    pub(super) fn mentions_ui(&mut self, ctx: &egui::Context) {
        if let Some(id) = ctx.data_mut(|d| d.remove_temp::<u64>(channel_id())) {
            if self.store.go_to_channel(id) {
                self.scroll_to_bottom = true;
                self.flush_commands();
            } else {
                self.notify(crate::i18n::table::search::t::UNKNOWN_CHANNEL);
            }
            ctx.request_repaint();
        }
        let Some(mut pop) = ctx.data(|d| d.get_temp::<RolePop>(role_id())) else { return };
        let close = |ctx: &egui::Context| ctx.data_mut(|d| d.remove::<RolePop>(role_id()));
        let Some(guild) = self.store.guild else { return close(ctx) };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return close(ctx);
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let members = self.store.known_role_members(guild, pop.role);
        let color = self.store.guilds.iter().find(|g| g.id == guild).and_then(|g| g.roles.iter().find(|r| r.id == pop.role)).map_or(0, |r| r.color);
        let avatars: Vec<Avatar> = {
            let people = self.store.people.lock();
            members.iter().take(MAX_ROWS).map(|(id, _)| people.user(*id).map_or_else(|| Avatar::default_for(*id), |u| u.avatar)).collect()
        };
        let pos = egui::pos2(pop.anchor.left(), pop.anchor.bottom() + t.spacing.item_y);
        let mut picked: Option<(u64, String, Avatar, egui::Rect)> = None;
        let area = egui::Area::new(role_id()).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(ctx, |ui| {
            t.menu_frame().show(ui, |ui| {
                ui.set_width(t.layout.profile_width.min(280.0));
                ui.horizontal(|ui| {
                    if color != 0 {
                        let (dot, _) = ui.allocate_exact_size(egui::Vec2::splat(t.typography.small), Sense::hover());
                        ui.painter().circle_filled(dot.center(), dot.width() / 2.0 - 1.0, egui::Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, color as u8));
                    }
                    ui.label(RichText::new(ff::role_members(lang, &pop.name)).strong().color(t.colors.text_strong));
                });
                ui.label(RichText::new(crate::i18n::table::ff::p::known_members(lang, members.len() as u64)).small().color(t.colors.muted));
                ui.add_space(t.spacing.item_y);
                if members.is_empty() {
                    ui.label(RichText::new(ft::ROLE_NONE_KNOWN.get(lang)).color(t.colors.muted));
                    return;
                }
                let row_h = t.spacing.member_row;
                egui::ScrollArea::vertical().max_height(row_h * 8.0).show_rows(ui, row_h, members.len().min(MAX_ROWS), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    for i in range {
                        let (id, name) = &members[i];
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                        if resp.hovered() {
                            ui.painter().rect_filled(rect, t.r_row(), t.colors.menu_hover);
                        }
                        let side = t.layout.small_avatar.min(row_h - 4.0);
                        let slot = pixel_square(ui, egui::pos2(rect.left() + t.spacing.item_x, rect.center().y - side / 2.0), side);
                        paint_avatar(ui, &self.images, &t, slot, &avatars[i], name);
                        let font = egui::TextStyle::Body.resolve(ui.style());
                        let galley = ui.painter().layout_no_wrap(name.clone(), font, t.colors.text);
                        let clip = rect.with_min_x(slot.right() + t.spacing.item_x).with_max_x(rect.right() - t.spacing.item_x);
                        ui.painter().with_clip_rect(clip).galley(egui::pos2(clip.left(), rect.center().y - galley.size().y / 2.0), galley, t.colors.text);
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            picked = Some((*id, name.clone(), avatars[i], rect));
                        }
                    }
                });
            });
        });
        if let Some((id, name, avatar, rect)) = picked {
            close(ctx);
            super::profile::open(ctx, id, Some(guild), rect, &name, avatar);
            return;
        }
        let outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
        if outside && !pop.fresh {
            return close(ctx);
        }
        if pop.fresh {
            pop.fresh = false;
            ctx.data_mut(|d| d.insert_temp(role_id(), pop));
        }
    }
}
