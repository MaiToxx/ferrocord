//! « Intégrations » de « Modifier le salon » : renvoie à la page Webhooks de
//! « Paramètres du serveur », filtrée sur ce salon (lot A8 + A9).

use super::super::App;

/// La page est-elle proposée pour ce salon ?
pub(super) fn available(app: &App, channel: u64) -> bool {
    app.store.can_manage_webhooks(channel)
}

/// Ouvre les webhooks du salon ; « Modifier le salon » se ferme pour la laisser voir.
pub(super) fn open(app: &mut App, channel: u64) {
    if super::super::guild_settings::webhooks::show_for_channel(app, channel) {
        app.close_channel_settings();
    }
}
