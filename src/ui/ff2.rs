//! Lot FF2 (seconde passe sur les manques fonctionnels) : état des petites
//! fenêtres du lot et point d'appel unique depuis la fin de l'image, pour ne
//! toucher la mise en page principale qu'en un endroit ; lignes du haut de la
//! liste des salons et entrées du menu du serveur (F15).

use super::App;
use crate::i18n::table::ff::{f as ff, t as ft};
use crate::state::admin::community;
use crate::theme::icons::{self, Icon};
use eframe::egui::{self, Sense};


/// Message tapé sur une carte de profil, en attente de sa conversation (F24).
#[derive(Clone)]
struct PendingDm {
    user_id: u64,
    text: String,
    since: std::time::Instant,
}

const PENDING_MAX: std::time::Duration = std::time::Duration::from_secs(20);

fn pending_id() -> egui::Id {
    egui::Id::unique("ff2-message-de-carte")
}

/// Ligne cliquée en haut de la liste des salons (F15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TopPick {
    Browse,
    Members,
    Boosts,
}

/// Entrée choisie dans le menu du serveur (F15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ServerPick {
    CreateChannel,
    CreateCategory,
    CreateEvent,
    EditProfile,
}

/// Boosts qu'il faut pour chaque niveau (1, 2, 3), d'après la page d'aide de Discord.
const BOOST_STEPS: [u32; 3] = [2, 7, 14];

/// Part du chemin parcouru vers le niveau suivant (1 : niveau 3 atteint).
pub(super) fn boost_progress(tier: u8, boosts: u32) -> f32 {
    let tier = usize::from(tier.min(3));
    if tier >= 3 {
        return 1.0;
    }
    let from = if tier == 0 { 0 } else { BOOST_STEPS[tier - 1] };
    let to = BOOST_STEPS[tier];
    (boosts.saturating_sub(from) as f32 / (to - from) as f32).clamp(0.0, 1.0)
}

impl App {
    /// Fenêtres du lot, dessinées par-dessus l'interface principale.
    pub(super) fn ff2_windows(&mut self, ui: &mut egui::Ui) {
        self.thread_members_window(ui);
        self.pending_card_message();
    }

    /// Message tapé sur une carte de profil (F24) : la conversation s'ouvre et le
    /// texte part. Conversation pas encore créée : il attend qu'elle existe.
    pub(super) fn send_from_card(&mut self, user_id: u64, text: String) {
        let known = self.dm_known(user_id);
        self.store.message_user(user_id);
        match known {
            Some(c) => {
                self.store.send_message(c, text);
            }
            None => {
                self.ctx.data_mut(|d| d.insert_temp(pending_id(), PendingDm { user_id, text, since: std::time::Instant::now() }));
            }
        }
    }

    fn dm_known(&self, user_id: u64) -> Option<u64> {
        let c = self.store.people.lock().dm_with(user_id)?;
        self.store.dms.iter().any(|d| d.id == c).then_some(c)
    }

    /// Un message attend-il l'ouverture d'une conversation ?
    #[cfg(test)]
    pub(super) fn card_message_pending(&self) -> bool {
        self.ctx.data(|d| d.get_temp::<PendingDm>(pending_id())).is_some()
    }

    /// Envoie le message en attente dès que sa conversation existe ; abandonné
    /// si elle n'est pas venue (refus de Discord) après [`PENDING_MAX`].
    pub(super) fn pending_card_message(&mut self) {
        let Some(p) = self.ctx.data(|d| d.get_temp::<PendingDm>(pending_id())) else { return };
        match self.dm_known(p.user_id) {
            Some(c) => {
                self.ctx.data_mut(|d| d.remove::<PendingDm>(pending_id()));
                self.store.send_message(c, p.text);
            }
            None if p.since.elapsed() > PENDING_MAX => {
                self.ctx.data_mut(|d| d.remove::<PendingDm>(pending_id()));
            }
            None => {}
        }
    }

    /// Une ligne du haut de la liste des salons, comme « Événements ».
    fn top_row(&self, ui: &mut egui::Ui, icon: Icon, label: &str, detail: Option<String>) -> egui::Response {
        let t = &self.theme;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), Sense::click());
        if resp.hovered() || resp.has_focus() {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
        }
        super::keys::focus_ring(ui, t, &resp);
        let (slot, left) = t.channel_row_parts(rect, Default::default());
        icons::paint(icon, ui.painter(), slot, t.colors.faint);
        let font = egui::FontId::proportional(t.typography.body);
        let color = if resp.hovered() { t.colors.text } else { t.colors.muted };
        ui.painter().with_clip_rect(rect).text(egui::pos2(left, rect.center().y), egui::Align2::LEFT_CENTER, label, font.clone(), color);
        if let Some(d) = detail {
            ui.painter().text(egui::pos2(rect.right() - t.spacing.item_x, rect.center().y), egui::Align2::RIGHT_CENTER, d, egui::FontId::proportional(t.typography.small), t.colors.faint);
        }
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    }

    /// Lignes « Parcourir les salons », « Membres » et « Boosts » sous
    /// « Événements » (F15), chacune seulement si elle mène quelque part.
    pub(super) fn guild_top_entries(&self, ui: &mut egui::Ui, guild: u64) -> Option<TopPick> {
        let lang = self.lang;
        let s = &self.store;
        let mut pick = None;
        let browse = s.guild_info(guild).is_some_and(|i| i.has(community::GUILD_ONBOARDING)) || s.browse_only_followed(guild);
        if browse && self.top_row(ui, Icon::Compass, crate::i18n::table::events::t::BROWSE_CHANNELS.get(lang), None).clicked() {
            pick = Some(TopPick::Browse);
        }
        if s.can_open_guild_settings(guild) && super::GuildPage::Members.allowed(s, guild) && self.top_row(ui, Icon::Members, ft::TOP_MEMBERS.get(lang), None).clicked() {
            pick = Some(TopPick::Members);
        }
        let (tier, boosts) = s.guilds.iter().find(|g| g.id == guild).map_or((0, 0), |g| (g.premium_tier, g.boosts));
        if boosts > 0 {
            let row = self.top_row(ui, Icon::Boost, ft::TOP_BOOSTS.get(lang), Some(ff::boost_level(lang, tier)));
            // Barre de progression vers le niveau suivant, sous la ligne.
            let t = &self.theme;
            let (bar, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.item_y), Sense::hover());
            let track = egui::Rect::from_center_size(bar.center(), egui::vec2(bar.width() - 2.0 * t.spacing.item_x, 4.0));
            ui.painter().rect_filled(track, 2.0, t.colors.bg3);
            let done = egui::Rect::from_min_size(track.min, egui::vec2(track.width() * boost_progress(tier, boosts), track.height()));
            ui.painter().rect_filled(done, 2.0, t.colors.accent);
            if row.on_hover_text(crate::i18n::table::ff::p::boosts(lang, u64::from(boosts))).clicked() {
                pick = Some(TopPick::Boosts);
            }
        }
        pick
    }

    pub(super) fn apply_guild_top(&mut self, guild: u64, pick: TopPick) {
        match pick {
            TopPick::Browse => super::browse::open(&self.ctx, guild),
            TopPick::Members => self.open_guild_settings(guild, Some(super::GuildPage::Members)),
            TopPick::Boosts => {
                if self.store.can_open_guild_settings(guild) && super::GuildPage::Boosts.allowed(&self.store, guild) {
                    self.open_guild_settings(guild, Some(super::GuildPage::Boosts));
                } else {
                    let (tier, boosts) = self.store.guilds.iter().find(|g| g.id == guild).map_or((0, 0), |g| (g.premium_tier, g.boosts));
                    self.notify(super::ToastText::Text(format!("{} · {}", ff::boost_level(self.lang, tier), crate::i18n::table::ff::p::boosts(self.lang, u64::from(boosts)))));
                }
            }
        }
    }

    /// Entrées du menu du serveur que Discord y range aussi (F15) : créer un
    /// salon, une catégorie, un événement, modifier son profil de serveur.
    pub(super) fn server_menu_more(&self, ui: &mut egui::Ui, guild: u64) -> Option<ServerPick> {
        let lang = self.lang;
        let t = &self.theme;
        let mut pick = None;
        if self.store.can_manage_channels(guild) {
            if super::menu::item(ui, t, ft::MENU_CREATE_CHANNEL.get(lang), Some(Icon::Plus), false).clicked() {
                pick = Some(ServerPick::CreateChannel);
            }
            if super::menu::item(ui, t, ft::MENU_CREATE_CATEGORY.get(lang), Some(Icon::Folder), false).clicked() {
                pick = Some(ServerPick::CreateCategory);
            }
        }
        if self.store.can_create_events(guild) && super::menu::item(ui, t, ft::MENU_CREATE_EVENT.get(lang), Some(Icon::Calendar), false).clicked() {
            pick = Some(ServerPick::CreateEvent);
        }
        if super::menu::item(ui, t, ft::MENU_EDIT_PROFILE.get(lang), Some(Icon::Pencil), false).clicked() {
            pick = Some(ServerPick::EditProfile);
        }
        super::menu::separator(ui, t);
        if pick.is_some() {
            ui.close();
        }
        pick
    }

    pub(super) fn apply_server_more(&mut self, guild: u64, pick: ServerPick) {
        use super::channel_settings::list::AdminPick;
        match pick {
            ServerPick::CreateChannel => self.apply_admin_pick(AdminPick::Create { guild, parent: None, category: false }),
            ServerPick::CreateCategory => self.apply_admin_pick(AdminPick::Create { guild, parent: None, category: true }),
            ServerPick::CreateEvent => self.open_editor(guild, None),
            ServerPick::EditProfile => self.open_profile_editor(true),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boost_bar_follows_the_levels() {
        assert_eq!(boost_progress(0, 0), 0.0);
        assert_eq!(boost_progress(0, 1), 0.5);
        assert_eq!(boost_progress(1, 2), 0.0);
        assert_eq!(boost_progress(2, 14), 1.0);
        assert_eq!(boost_progress(3, 30), 1.0);
        assert_eq!(boost_progress(9, 0), 1.0, "niveau inattendu : borné");
    }
}
