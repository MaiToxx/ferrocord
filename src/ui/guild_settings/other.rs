//! Pages renvoyées à l'application officielle : statistiques du serveur
//! (Discord ne publie pas de route pour elles) ; transfert de propriété et
//! suppression du serveur (souvent soumis à l'A2F, définitifs).

use super::super::settings::{in_discord, note};
use super::App;
use crate::i18n::table::admin::t as tx;
use crate::theme::Tokens;
use eframe::egui;

impl App {
    pub(super) fn page_guild_insights(&mut self, ui: &mut egui::Ui, t: &Tokens, _guild: u64) {
        let lang = self.lang;
        note(ui, t, tx::INSIGHTS_TIP.get(lang));
        in_discord(ui, lang, tx::OPEN_INSIGHTS.get(lang), tx::INSIGHTS_TIP.get(lang));
    }

    pub(super) fn page_guild_ownership(&mut self, ui: &mut egui::Ui, t: &Tokens, _guild: u64) {
        let lang = self.lang;
        note(ui, t, tx::OWNER_ONLY.get(lang));
        in_discord(ui, lang, tx::TRANSFER.get(lang), tx::TRANSFER_TIP.get(lang));
        note(ui, t, tx::TRANSFER_TIP.get(lang));
        in_discord(ui, lang, tx::DELETE_SERVER.get(lang), tx::DELETE_SERVER_TIP.get(lang));
        note(ui, t, tx::DELETE_SERVER_TIP.get(lang));
    }
}
