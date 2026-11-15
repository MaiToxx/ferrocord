//! Rich Presence locale (lot R) côté interface : réglage de cet ordinateur,
//! état du tube et applications branchées (page Activités des Paramètres).
//! L'activité choisie s'affiche aussi dans mon profil (`ui::profile`).

use super::App;
use crate::i18n::table::rpc::{f as rf, t as rt};
use crate::state::rpc::AssetTable;
use crate::state::usersettings::schema::Key;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

impl App {
    /// Une fois par image : le réglage de cet ordinateur vers le magasin.
    pub(super) fn rpc_tick(&mut self) {
        self.store.activity.rpc.enabled = crate::plugins::on(crate::plugins::Id::RichPresence);
    }

    /// Section « Rich Presence » de la page Activités.
    pub(super) fn rpc_settings_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let c = &t.colors;
        super::settings::section(ui, t, rt::SECTION.get(lang));
        let muted = |ui: &mut egui::Ui, s: &str| {
            ui.add(egui::Label::new(RichText::new(s).color(c.muted)).wrap());
        };
        if !crate::rpc::SUPPORTED {
            muted(ui, rt::UNSUPPORTED.get(lang));
            return;
        }
        // Le même interrupteur que la carte du plugin (Paramètres > Plugins).
        let on = self.local.prefs.plugins.on(crate::plugins::Id::RichPresence);
        if let Some(v) = super::settings::toggle(ui, rt::TOGGLE.get(lang), rt::TOGGLE_TIP.get(lang), Some(on), true) {
            self.set_plugin(crate::plugins::Id::RichPresence, v);
        }
        if !on {
            muted(ui, rt::OFF.get(lang));
            return;
        }
        let mut load = None;
        let r = &self.store.activity.rpc;
        match (r.listening_known, r.listening) {
            (_, Some(n)) => muted(ui, &rf::listening(lang, format!("discord-ipc-{n}"))),
            (true, None) => muted(ui, rt::NO_PIPE.get(lang)),
            (false, None) => muted(ui, rt::WAITING.get(lang)),
        }
        if self.store.account.bool(Key::ShowCurrentGame) == Some(false) {
            ui.add(egui::Label::new(RichText::new(rt::SHARING_OFF.get(lang)).color(c.danger)).wrap());
        }
        if r.clients.is_empty() {
            muted(ui, rt::NONE.get(lang));
        }
        let now = crate::state::store::now_ms();
        let shown = r.current().map(|(id, a)| (id, a.name.clone()));
        let index = self.store.activity.index.clone();
        let name_of = |id: u64| index.as_ref()?.apps().iter().find(|a| a.id == id).map(|a| a.name.clone());
        for client in &r.clients {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(r.label(client, name_of)).strong().color(c.text));
                if let Some(p) = &client.process {
                    ui.label(RichText::new(p).small().color(c.faint));
                }
                let line = match &client.activity {
                    Some(a) => [a.details.as_deref(), a.state.as_deref()].into_iter().flatten().collect::<Vec<_>>().join(" \u{2014} "),
                    None => rt::NO_ACTIVITY.get(lang).to_string(),
                };
                if !line.is_empty() {
                    ui.label(RichText::new(line).small().color(c.muted));
                }
                if client.activity.as_ref().is_some_and(|a| shown.as_ref().is_some_and(|(id, n)| *id == client.app_id && *n == a.name)) {
                    ui.label(RichText::new(rt::SHOWN.get(lang)).small().color(c.accent));
                }
                ui.label(RichText::new(rf::connected_since(lang, super::activity::duration(now - client.since_ms))).small().color(c.faint));
                // Images désignées par leur nom : la liste des images de l'application
                // les traduit ; elle n'est demandée qu'au clic (une requête).
                if client.activity.as_ref().is_some_and(|a| r.unresolved(client.app_id, a)) {
                    match r.asset_table(client.app_id) {
                        Some(AssetTable::Loading) => {
                            ui.label(RichText::new(rt::ASSETS_LOADING.get(lang)).small().color(c.muted));
                        }
                        Some(AssetTable::Ready(_)) => {
                            ui.label(RichText::new(rt::ASSETS_UNKNOWN.get(lang)).small().color(c.muted));
                        }
                        table => {
                            if table.is_some() {
                                ui.label(RichText::new(rt::ASSETS_FAILED.get(lang)).small().color(c.danger));
                            }
                            if crate::ui::widgets::compact(ui, rt::ASSETS_LOAD.get(lang)).on_hover_text(rt::ASSETS_LOAD_TIP.get(lang)).clicked() {
                                load = Some(client.app_id);
                            }
                        }
                    }
                }
            });
        }
        if !r.clients.is_empty() {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        muted(ui, rt::OFFICIAL_FIRST.get(lang));
        muted(ui, rt::IMAGES_TIP.get(lang));
        if let Some(app_id) = load {
            self.store.load_rpc_assets(app_id);
            self.flush_commands();
        }
    }
}
