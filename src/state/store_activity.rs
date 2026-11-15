//! Raccord du magasin avec le lot C3 : réglages synchronisés (statut,
//! `show_current_game`, statut personnalisé) → ma présence voulue, et clics
//! sur les boutons d'activité des autres. Module enfant de `store`.

use super::super::activity::{ActivityCmd, OwnCustom, OwnPresence};
use super::super::event::Cmd;
use super::super::notice::Status;
use super::super::usersettings::schema::{Key, SValue};
use super::{now_ms, Store};

impl Store {
    /// Ma présence d'après les réglages synchronisés ; `None` tant qu'ils
    /// sont inconnus.
    pub fn own_presence(&self) -> Option<OwnPresence> {
        let status = match self.account.value(Key::Status) {
            Some(SValue::Str(s)) => Some(s.as_str()),
            _ => None,
        };
        let show = self.account.bool(Key::ShowCurrentGame).unwrap_or(false);
        let now = now_ms();
        let custom = self
            .account
            .snapshot()
            .custom_status
            .as_ref()
            .filter(|c| !c.is_empty() && !c.expired(now))
            .map(|c| OwnCustom { text: c.text.clone(), emoji_name: c.emoji_name.clone(), emoji_id: c.emoji_id });
        self.activity.wish(status, show, custom)
    }

    pub(super) fn activity_commands(&mut self) -> Vec<ActivityCmd> {
        let connected = self.status == Status::Connected && self.me.is_some();
        let show = self.account.bool(Key::ShowCurrentGame);
        let presence = if connected { self.own_presence() } else { None };
        self.activity.commands(connected, show, presence)
    }

    /// « Charger les images » d'une application Rich Presence branchée.
    pub fn load_rpc_assets(&mut self, app_id: u64) {
        if let Some(c) = self.activity.load_assets(app_id) {
            self.outbox.push(Cmd::Activity(c));
        }
    }

    /// Clic sur le bouton `index` d'une activité de `user_id`.
    pub fn activity_button(&mut self, user_id: u64, session_id: &str, app_id: Option<u64>, index: usize) {
        if let Some(c) = self.activity.button(user_id, session_id, app_id, index) {
            self.outbox.push(Cmd::Activity(c));
        }
    }
}
