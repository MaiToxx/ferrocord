//! Intégrations et applications d'un serveur (lot A9) : liste (`GET
//! /guilds/{id}/integrations`), retrait (après confirmation), et
//! permissions des commandes d'une application, en lecture seule (les
//! modifier demande un jeton OAuth2 de l'application, que Ferrocord n'a pas).

use super::guild::{AdminCmd, AdminEvent, AdminNotice, Load};
use crate::state::perms::Permissions;
use crate::state::store::Store;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegrationApp {
    pub id: u64,
    pub name: String,
    pub description: String,
    /// Robot de l'application, s'il y en a un.
    pub bot: Option<(u64, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Integration {
    pub id: u64,
    pub name: String,
    /// `twitch`, `youtube`, `discord` ou `guild_subscription`.
    pub kind: String,
    pub enabled: bool,
    pub account: String,
    /// Qui l'a ajoutée.
    pub user: Option<String>,
    pub application: Option<IntegrationApp>,
    pub scopes: Vec<String>,
}

/// Cible d'une permission de commande (documentation : 1 rôle, 2 membre, 3 salon).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermTarget {
    Role(u64),
    User(u64),
    Channel(u64),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandPerm {
    pub target: PermTarget,
    pub allowed: bool,
}

/// Permissions d'une commande, ou de toute l'application quand `command_id`
/// vaut l'identifiant de l'application.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandPerms {
    pub command_id: u64,
    pub perms: Vec<CommandPerm>,
}

impl Store {
    pub fn load_integrations(&mut self, guild_id: u64) -> bool {
        let a = &mut self.guild_admin;
        if a.open != Some(guild_id) || matches!(a.integrations, Some(Load::Loading)) || !self.may_in_guild(guild_id, Permissions::MANAGE_GUILD) {
            return false;
        }
        let a = &mut self.guild_admin;
        a.stale_integrations = false;
        if !matches!(a.integrations, Some(Load::Ready(_))) {
            a.integrations = Some(Load::Loading);
        }
        self.admin_cmd(AdminCmd::Integrations { guild_id });
        true
    }

    /// Jamais lue, ou périmée par GUILD_INTEGRATIONS_UPDATE.
    pub fn integrations_wanted(&self) -> bool {
        self.guild_admin.integrations.is_none() || self.guild_admin.stale_integrations
    }

    /// Retire une intégration (après confirmation dans l'interface).
    pub fn remove_integration(&mut self, guild_id: u64, id: u64, reason: Option<String>) -> bool {
        if !self.may_in_guild(guild_id, Permissions::MANAGE_GUILD) || !self.guild_admin.removing.insert(id) {
            return false;
        }
        let reason = reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        self.admin_cmd(AdminCmd::RemoveIntegration { guild_id, id, reason });
        true
    }

    /// Permissions des commandes d'une application (lecture, sur un clic).
    pub fn load_command_perms(&mut self, guild_id: u64, app_id: u64) -> bool {
        let a = &mut self.guild_admin;
        if a.open != Some(guild_id) || matches!(a.command_perms.get(&app_id), Some(Load::Loading)) {
            return false;
        }
        a.command_perms.insert(app_id, Load::Loading);
        self.admin_cmd(AdminCmd::CommandPerms { guild_id, app_id });
        true
    }

    pub(super) fn apply_integrations(&mut self, e: AdminEvent) {
        let Some(open) = self.guild_admin.open else { return };
        let a = &mut self.guild_admin;
        match e {
            AdminEvent::Integrations { guild_id, list } if guild_id == open => a.integrations = Some(Load::Ready(list)),
            AdminEvent::IntegrationRemoved { guild_id, id } if guild_id == open => {
                a.removing.remove(&id);
                if let Some(Load::Ready(list)) = &mut a.integrations {
                    list.retain(|i| i.id != id);
                }
                a.notice(AdminNotice::IntegrationRemoved);
            }
            AdminEvent::IntegrationsChanged { guild_id } if guild_id == open && a.integrations.is_some() => a.stale_integrations = true,
            AdminEvent::CommandPerms { guild_id, app_id, list } if guild_id == open => {
                a.command_perms.insert(app_id, Load::Ready(list));
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Guild, User};
    use crate::state::{Cmd, Event};

    fn store() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(9, "G".into(), Vec::new())], dms: Vec::new() });
        s.open_guild_admin(9);
        s.take_commands();
        s
    }

    fn integ(id: u64) -> Integration {
        Integration { id, name: format!("i{id}"), kind: "discord".into(), enabled: true, account: String::new(), user: None, application: None, scopes: Vec::new() }
    }

    #[test]
    fn list_remove_and_stale() {
        let mut s = store();
        assert!(s.integrations_wanted());
        assert!(s.load_integrations(9));
        assert!(!s.load_integrations(9));
        assert!(!s.load_integrations(8), "autre serveur que la fenêtre");
        s.apply(Event::GuildAdmin(AdminEvent::Integrations { guild_id: 9, list: vec![integ(1), integ(2)] }));
        assert!(!s.integrations_wanted());
        assert!(s.remove_integration(9, 1, Some(" ménage ".into())));
        assert!(!s.remove_integration(9, 1, None), "déjà en cours");
        assert_eq!(s.take_commands().last(), Some(&Cmd::GuildAdmin(AdminCmd::RemoveIntegration { guild_id: 9, id: 1, reason: Some("ménage".into()) })));
        s.apply(Event::GuildAdmin(AdminEvent::IntegrationRemoved { guild_id: 9, id: 1 }));
        assert_eq!(s.guild_admin.integrations.as_ref().and_then(Load::ready).map(Vec::len), Some(1));
        s.apply(Event::GuildAdmin(AdminEvent::IntegrationsChanged { guild_id: 9 }));
        assert!(s.integrations_wanted());
        assert!(s.load_command_perms(9, 55));
        s.apply(Event::GuildAdmin(AdminEvent::CommandPerms { guild_id: 9, app_id: 55, list: vec![CommandPerms { command_id: 55, perms: vec![CommandPerm { target: PermTarget::Role(9), allowed: false }] }] }));
        assert!(matches!(s.guild_admin.command_perms.get(&55), Some(Load::Ready(v)) if v.len() == 1));
    }
}
