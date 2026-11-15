//! Page « Salons » de « Paramètres du serveur » (lot A2) : la liste des
//! catégories et des salons, chacun avec « Modifier », et les boutons de
//! création. L'ordre se change par glisser-déposer dans la liste des salons.

use super::super::channel_settings::list::AdminPick;
use super::App;
use crate::i18n::table::chadmin::t as tx;
use crate::state::model::ChannelKind;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, RichText};

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
    let lang = app.lang;
    let Some(g) = app.store.guilds.iter().find(|g| g.id == guild) else { return };
    super::super::settings::note(ui, t, tx::CHANNELS_TIP.get(lang));
    let mut pick = None;
    ui.horizontal_wrapped(|ui| {
        if ui.add(crate::ui::form::btn(t, tx::CREATE_CHANNEL.get(lang))).clicked() {
            pick = Some(AdminPick::Create { guild, parent: None, category: false });
        }
        if ui.add(crate::ui::form::btn(t, tx::CREATE_CATEGORY.get(lang))).clicked() {
            pick = Some(AdminPick::Create { guild, parent: None, category: true });
        }
    });
    ui.add_space(t.spacing.item_y);
    let mut first = true;
    for c in &g.channels {
        let category = c.kind == ChannelKind::Category;
        if first && !category {
            super::super::settings::section(ui, t, tx::NO_CATEGORY.get(lang));
        }
        first = false;
        if category {
            ui.add_space(t.spacing.category_gap);
        }
        ui.horizontal(|ui| {
            if category {
                ui.label(RichText::new(c.name.to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
            } else {
                ui.add_space(t.spacing.channel_indent);
                let icon = match c.kind {
                    ChannelKind::Announcement => Icon::Megaphone,
                    ChannelKind::Forum => Icon::Forum,
                    ChannelKind::Media => Icon::Media,
                    _ => Icon::Hash,
                };
                super::super::util::inline_icon(ui, icon, t.typography.body, t.colors.faint);
                ui.label(&c.name);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if app.store.can_edit_channel(c.id) && ui.add(crate::ui::form::btn(t, tx::EDIT.get(lang))).on_hover_text(c.name.as_str()).clicked() {
                    pick = Some(AdminPick::Edit(c.id));
                }
                if category && super::super::util::icon_button(ui, t, Icon::Plus, tx::CREATE_CHANNEL.get(lang)).clicked() {
                    pick = Some(AdminPick::Create { guild, parent: Some(c.id), category: false });
                }
            });
        });
    }
    if let Some(p) = pick {
        app.apply_admin_pick(p);
    }
}
