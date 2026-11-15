//! Webhooks d'un serveur ou d'un salon (lot A9) : liste, création, nom,
//! avatar et salon, suppression. L'adresse d'un webhook contient son jeton :
//! elle est gardée dans un [`Secret`], jamais écrite dans un journal ni
//! affichée sans geste.

use super::guild::{AdminCmd, AdminEvent, AdminNotice, Load};
use crate::state::perms::{Permissions, Verdict};
use crate::state::store::Store;

/// Nom d'un webhook : 1 à 80 caractères (documentation, Create Webhook).
pub const NAME_MAX: usize = 80;

/// Liste demandée : celle du serveur, ou celle d'un salon (quand je n'ai la
/// permission que dans ce salon).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HookScope {
    Guild(u64),
    Channel(u64),
}

/// Texte secret : ni `Debug` ni `Display` ne le montrent.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: String) -> Self {
        Secret(s)
    }

    /// Le texte lui-même : à n'utiliser que pour le presse-papiers.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(…)")
    }
}

/// Sorte de webhook (documentation : 1 entrant, 2 abonnement à un salon
/// d'annonces, 3 application).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HookKind {
    Incoming,
    Follower,
    Application,
    Other,
}

impl HookKind {
    pub fn from_code(c: u8) -> Self {
        match c {
            1 => HookKind::Incoming,
            2 => HookKind::Follower,
            3 => HookKind::Application,
            _ => HookKind::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Webhook {
    pub id: u64,
    pub kind: HookKind,
    pub guild_id: Option<u64>,
    pub channel_id: Option<u64>,
    pub name: String,
    /// Empreinte de l'avatar.
    pub avatar: Option<String>,
    /// Qui l'a créé.
    pub creator: Option<String>,
    /// Adresse avec le jeton (webhooks entrants seulement).
    pub url: Option<Secret>,
    /// Abonnement : serveur et salon d'origine.
    pub source: Option<String>,
}

impl Store {
    /// Puis-je gérer les webhooks de ce salon (inconnu : oui) ?
    pub fn can_manage_webhooks(&self, channel_id: u64) -> bool {
        self.can(channel_id, Permissions::MANAGE_WEBHOOKS) != Verdict::Refuse
    }

    /// Demande une liste (ouverture de la page, « Actualiser », ou liste
    /// rendue périmée par WEBHOOKS_UPDATE et affichée) ; une à la fois.
    pub fn load_webhooks(&mut self, scope: HookScope) -> bool {
        let a = &mut self.guild_admin;
        let Some(guild_id) = a.open else { return false };
        if matches!(a.hooks.get(&scope), Some(Load::Loading)) {
            return false;
        }
        a.stale_hooks.remove(&scope);
        // Une liste déjà là reste montrée pendant qu'elle est relue.
        if !matches!(a.hooks.get(&scope), Some(Load::Ready(_))) {
            a.hooks.insert(scope, Load::Loading);
        }
        self.admin_cmd(AdminCmd::Webhooks { guild_id, scope });
        true
    }

    /// Liste à demander parce qu'elle n'a jamais été lue ou qu'elle est périmée.
    pub fn webhooks_wanted(&self, scope: HookScope) -> bool {
        !self.guild_admin.hooks.contains_key(&scope) || self.guild_admin.stale_hooks.contains(&scope)
    }

    /// Webhooks d'un salon, tirés de la liste du serveur ou de celle du salon.
    pub fn channel_webhooks(&self, guild_id: u64, channel_id: u64) -> Option<Vec<Webhook>> {
        let from = |s| self.guild_admin.hooks.get(&s).and_then(Load::ready);
        let list = from(HookScope::Channel(channel_id)).or_else(|| from(HookScope::Guild(guild_id)))?;
        Some(list.iter().filter(|h| h.channel_id == Some(channel_id)).cloned().collect())
    }

    pub fn create_webhook(&mut self, guild_id: u64, channel_id: u64, name: &str, avatar: Option<String>) -> bool {
        let name = name.trim();
        if !(1..=NAME_MAX).contains(&name.chars().count()) || !self.can_manage_webhooks(channel_id) || !self.guild_admin.hook_busy.insert(0) {
            return false;
        }
        self.admin_cmd(AdminCmd::CreateWebhook { guild_id, channel_id, name: name.to_string(), avatar });
        true
    }

    pub fn edit_webhook(&mut self, guild_id: u64, id: u64, name: Option<String>, avatar: Option<Option<String>>, channel_id: Option<u64>) -> bool {
        let name = name.map(|n| n.trim().to_string());
        if name.as_ref().is_some_and(|n| !(1..=NAME_MAX).contains(&n.chars().count())) {
            return false;
        }
        if channel_id.is_some_and(|c| !self.can_manage_webhooks(c)) || (name.is_none() && avatar.is_none() && channel_id.is_none()) {
            return false;
        }
        if !self.guild_admin.hook_busy.insert(id) {
            return false;
        }
        self.admin_cmd(AdminCmd::EditWebhook { guild_id, id, name, avatar, channel_id });
        true
    }

    /// Supprime un webhook (après confirmation dans l'interface).
    pub fn delete_webhook(&mut self, guild_id: u64, id: u64, reason: Option<String>) -> bool {
        if !self.guild_admin.hook_busy.insert(id) {
            return false;
        }
        let reason = reason.map(|r| r.trim().to_string()).filter(|r| !r.is_empty());
        self.admin_cmd(AdminCmd::DeleteWebhook { guild_id, id, reason });
        true
    }

    pub(super) fn apply_webhooks(&mut self, e: AdminEvent) {
        let Some(open) = self.guild_admin.open else { return };
        let a = &mut self.guild_admin;
        match e {
            AdminEvent::Webhooks { scope, list } => {
                if matches!(scope, HookScope::Guild(g) if g == open) || a.hooks.contains_key(&scope) {
                    a.hooks.insert(scope, Load::Ready(list));
                }
            }
            AdminEvent::WebhookSaved { guild_id, hook, created } if guild_id == open => {
                a.hook_busy.remove(&if created { 0 } else { hook.id });
                if created {
                    a.created_hook = Some(hook.id);
                    a.notice(AdminNotice::WebhookCreated);
                }
                for (scope, l) in a.hooks.iter_mut() {
                    let Load::Ready(list) = l else { continue };
                    let fits = match scope {
                        HookScope::Guild(_) => true,
                        HookScope::Channel(c) => hook.channel_id == Some(*c),
                    };
                    list.retain(|h| h.id != hook.id);
                    if fits {
                        list.push(hook.clone());
                    }
                }
            }
            AdminEvent::WebhookDeleted { guild_id, id } if guild_id == open => {
                a.hook_busy.remove(&id);
                if a.created_hook == Some(id) {
                    a.created_hook = None;
                }
                for l in a.hooks.values_mut() {
                    if let Load::Ready(list) = l {
                        list.retain(|h| h.id != id);
                    }
                }
                a.notice(AdminNotice::WebhookDeleted);
            }
            AdminEvent::WebhooksChanged { guild_id, channel_id } if guild_id == open => {
                let stale: Vec<HookScope> = a.hooks.keys().filter(|s| matches!(s, HookScope::Guild(_)) || channel_id.is_none_or(|c| **s == HookScope::Channel(c))).copied().collect();
                a.stale_hooks.extend(stale);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::guild::AdminAction;
    use crate::state::model::{Channel, ChannelKind, Guild, User};
    use crate::state::{Cmd, Event};

    fn store() -> Store {
        let mut s = Store::new();
        let chan = |id| Channel { id, name: "c".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(9, "G".into(), vec![chan(100), chan(101)])], dms: Vec::new() });
        s.open_guild_admin(9);
        s.take_commands();
        s
    }

    fn hook(id: u64, channel: u64) -> Webhook {
        Webhook { id, kind: HookKind::Incoming, guild_id: Some(9), channel_id: Some(channel), name: format!("h{id}"), avatar: None, creator: None, url: Some(Secret::new(format!("https://discord.com/api/webhooks/{id}/secret"))), source: None }
    }

    #[test]
    fn url_never_shows_in_debug() {
        let h = hook(5, 100);
        let dbg = format!("{h:?}");
        assert!(!dbg.contains("secret"), "{dbg}");
        assert_eq!(h.url.as_ref().map(Secret::expose), Some("https://discord.com/api/webhooks/5/secret"));
    }

    #[test]
    fn list_create_move_delete() {
        let mut s = store();
        assert!(s.webhooks_wanted(HookScope::Guild(9)));
        assert!(s.load_webhooks(HookScope::Guild(9)));
        assert!(!s.load_webhooks(HookScope::Guild(9)), "déjà demandée");
        s.apply(Event::GuildAdmin(AdminEvent::Webhooks { scope: HookScope::Guild(9), list: vec![hook(5, 100), hook(6, 101)] }));
        assert_eq!(s.channel_webhooks(9, 100).map(|v| v.len()), Some(1));
        assert!(!s.create_webhook(9, 100, "  ", None), "nom vide");
        assert!(s.create_webhook(9, 100, " Annonces ", None));
        assert!(!s.create_webhook(9, 100, "Autre", None), "une création à la fois");
        s.apply(Event::GuildAdmin(AdminEvent::WebhookSaved { guild_id: 9, hook: hook(7, 100), created: true }));
        assert_eq!(s.guild_admin.created_hook, Some(7));
        assert_eq!(s.guild_admin.take_notice(), Some(AdminNotice::WebhookCreated));
        // Déplacé dans #101.
        assert!(s.edit_webhook(9, 7, None, None, Some(101)));
        s.apply(Event::GuildAdmin(AdminEvent::WebhookSaved { guild_id: 9, hook: hook(7, 101), created: false }));
        assert_eq!(s.channel_webhooks(9, 101).map(|v| v.len()), Some(2));
        assert!(s.delete_webhook(9, 5, Some("  ".into())));
        let cmds = s.take_commands();
        assert_eq!(cmds.last(), Some(&Cmd::GuildAdmin(AdminCmd::DeleteWebhook { guild_id: 9, id: 5, reason: None })));
        s.apply(Event::GuildAdmin(AdminEvent::Failed { guild_id: 9, action: AdminAction::DeleteWebhook, failure: super::super::guild::Failure::Forbidden, target: Some(5) }));
        assert!(!s.guild_admin.hook_busy(5));
        assert!(s.delete_webhook(9, 5, None));
        s.apply(Event::GuildAdmin(AdminEvent::WebhookDeleted { guild_id: 9, id: 5 }));
        assert_eq!(s.channel_webhooks(9, 100).map(|v| v.len()), Some(0));
    }

    #[test]
    fn gateway_update_makes_the_list_stale() {
        let mut s = store();
        s.load_webhooks(HookScope::Guild(9));
        s.apply(Event::GuildAdmin(AdminEvent::Webhooks { scope: HookScope::Guild(9), list: vec![hook(5, 100)] }));
        assert!(!s.webhooks_wanted(HookScope::Guild(9)));
        s.apply(Event::GuildAdmin(AdminEvent::WebhooksChanged { guild_id: 9, channel_id: Some(100) }));
        assert!(s.webhooks_wanted(HookScope::Guild(9)));
        assert!(s.load_webhooks(HookScope::Guild(9)));
        assert!(!s.webhooks_wanted(HookScope::Guild(9)));
        // Autre serveur : ignoré.
        s.apply(Event::GuildAdmin(AdminEvent::WebhooksChanged { guild_id: 8, channel_id: None }));
        assert!(!s.webhooks_wanted(HookScope::Guild(9)));
    }
}
