//! Page « Événements » des paramètres du serveur (lot A7) : création et
//! gestion des événements à venir, avec les cartes de la liste des membres.

use crate::i18n::table::events::t as tx;
use crate::theme::Tokens;
use crate::ui::App;
use eframe::egui;

pub(in crate::ui) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
    let lang = app.lang;
    let mut create = false;
    if app.store.can_create_events(guild) {
        let b = crate::ui::form::primary(t, tx::CREATE_EVENT.get(lang));
        create = ui.add(b).clicked();
    }
    ui.add_space(t.spacing.item_y);
    let act = app.events_list_body(ui, t, guild);
    if let Some((id, a)) = act {
        app.apply_card(ui.ctx(), guild, id, a);
    }
    if create {
        app.open_editor(guild, None);
    }
}
