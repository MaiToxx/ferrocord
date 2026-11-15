//! « Parcourir les salons » (Browse Channels) : sur un serveur avec
//! intégration, l'utilisateur choisit les salons qu'il suit ; en mode
//! « seulement les suivis », les autres disparaissent de la liste.
//!
//! Aucune route nouvelle : ce sont des drapeaux des réglages du serveur,
//! envoyés par `PATCH /users/@me/guilds/{id}/settings` (docs.discord.food,
//! « User Settings › User Guild Settings Flags » et « Channel Override
//! Flags »), le même chemin que le muet et les notifications.

use super::model::{Channel, ChannelKind, Guild};
use super::usersettings::notif::Change;
use super::usersettings::Refusal;
use super::Store;

impl Store {
    /// Le salon `c` de `g` est masqué de la liste : le serveur ne montre que
    /// les salons suivis et ni lui, ni sa catégorie ne le sont. Une catégorie
    /// reste si l'un de ses salons est suivi. Le salon ouvert reste toujours.
    pub fn browse_hides(&self, g: &Guild, c: &Channel) -> bool {
        let n = self.account.notifs.get(Some(g.id));
        if !n.opt_in() || self.channel == Some(c.id) {
            return false;
        }
        if c.kind == ChannelKind::Category {
            return !n.followed(c.id, None) && !g.channels.iter().any(|x| x.parent == Some(c.id) && (n.followed(x.id, None) || self.channel == Some(x.id)));
        }
        !n.followed(c.id, c.parent)
    }

    /// Seuls les salons suivis de `guild` sont montrés.
    pub fn browse_only_followed(&self, guild: u64) -> bool {
        self.account.notifs.get(Some(guild)).opt_in()
    }

    /// Le salon (ou sa catégorie) est suivi.
    pub fn browse_followed(&self, guild: u64, channel: u64, parent: Option<u64>) -> bool {
        self.account.notifs.get(Some(guild)).followed(channel, parent)
    }

    /// « Montrer seulement les salons suivis » (geste de l'utilisateur).
    pub fn browse_set_only_followed(&mut self, guild: u64, on: bool) -> Result<(), Refusal> {
        let flags = self.account.notifs.get(Some(guild)).opt_in_flags(on);
        self.account.change_notif(Some(guild), None, Change::Flags(flags), 0)
    }

    /// Suivre ou ne plus suivre un salon ou une catégorie (geste de l'utilisateur).
    pub fn browse_follow(&mut self, guild: u64, channel: u64, follow: bool) -> Result<(), Refusal> {
        let flags = self.account.notifs.get(Some(guild)).follow_flags(channel, follow);
        self.account.change_notif(Some(guild), Some(channel), Change::Flags(flags), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::usersettings::{AccountCmd, AccountEvent};
    use crate::state::{Cmd, Event};

    fn chan(id: u64, kind: ChannelKind, parent: Option<u64>) -> Channel {
        Channel { id, name: format!("c{id}"), kind, position: 0, parent, overwrites: Some(Vec::new()), slowmode: 0 }
    }

    #[test]
    fn only_followed_channels_are_listed() {
        let mut s = Store::new();
        let g = Guild::new(9, "G".into(), vec![chan(1, ChannelKind::Category, None), chan(2, ChannelKind::Text, Some(1)), chan(3, ChannelKind::Text, None), chan(4, ChannelKind::Text, None)]);
        s.apply(Event::Ready { me: crate::state::model::User::new(7, "Moi".into()), guilds: vec![g.clone()], dms: Vec::new() });
        s.take_commands();
        assert!(g.channels.iter().all(|c| !s.browse_hides(&g, c)), "par défaut, tout est montré");
        assert!(s.browse_set_only_followed(9, true).is_ok());
        assert!(s.browse_only_followed(9));
        // Réponse du serveur : la requête suivante peut partir.
        let answer = |s: &mut Store| {
            let cmds = s.take_commands();
            let [Cmd::Account(AccountCmd::PatchGuild { id, guild: Some(9), .. })] = &cmds[..] else { panic!("{cmds:?}") };
            s.apply(Event::Account(AccountEvent::GuildWritten { id: *id, guild: Some(9), result: Ok(None) }));
        };
        answer(&mut s);
        assert!(s.browse_follow(9, 1, true).is_ok());
        answer(&mut s);
        let hidden: Vec<u64> = g.channels.iter().filter(|c| s.browse_hides(&g, c)).map(|c| c.id).collect();
        assert_eq!(hidden, vec![3, 4], "catégorie suivie : elle et ses salons restent");
        s.select_channel(4);
        assert!(!s.browse_hides(&g, &g.channels[3]), "le salon ouvert reste");
    }
}
