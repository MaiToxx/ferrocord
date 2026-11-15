//! « Invitations » de « Modifier le salon » : renvoie aux fenêtres du lot
//! A1 (« Inviter des personnes » sur ce salon, liste des invitations du
//! serveur avec « Révoquer »).

use super::App;
use crate::i18n::table::chadmin::t as tx;
use crate::theme::Tokens;
use eframe::egui;

impl App {
    pub(super) fn channel_invites(&mut self, ui: &mut egui::Ui, t: &Tokens, channel: u64) {
        let lang = self.lang;
        let Some(guild) = self.store.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel)).map(|g| g.id) else { return };
        super::super::settings::note(ui, t, tx::INVITES_TIP.get(lang));
        ui.add_space(t.spacing.item_y);
        let can_invite = self.store.invite_channels(guild).iter().any(|c| c.0 == channel);
        let can_list = self.store.can_manage_guild(guild);
        let (mut invite, mut list) = (false, false);
        ui.horizontal_wrapped(|ui| {
            if can_invite {
                invite = ui.add(super::primary(t, tx::INVITE_PEOPLE.get(lang))).clicked();
            }
            if can_list {
                list = ui.button(tx::SERVER_INVITES.get(lang)).clicked();
            }
        });
        if invite {
            self.open_invite_in(guild, channel);
        }
        if list {
            self.open_invites(guild);
        }
    }
}
