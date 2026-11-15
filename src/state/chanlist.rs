//! Ce que la liste des salons d'un serveur montre : catégories repliées,
//! « masquer les salons muets ». Les deux réglages sont ceux du compte
//! (`collapsed` d'une dérogation de salon, `hide_muted_channels` du serveur :
//! docs.discord.food, « User Settings › User Guild Settings »), écrits par
//! `PATCH /users/@me/guilds/{id}/settings` comme le muet. Chaque écriture suit
//! un geste de l'utilisateur.

use super::model::{Channel, ChannelKind, Guild};
use super::usersettings::notif::{Change, GuildNotif};
use super::usersettings::Refusal;
use super::Store;
use std::collections::HashSet;

/// Règles d'affichage de la liste d'un serveur, lues une fois par image.
pub struct ListRules<'a> {
    notif: Option<&'a GuildNotif>,
    now_ms: i64,
    /// Salon ouvert (pour un fil : son salon parent).
    open: Option<u64>,
}

impl ListRules<'_> {
    pub fn collapsed(&self, category: u64) -> bool {
        self.notif.is_some_and(|n| n.collapsed.contains(&category))
    }

    /// Le salon est muet, lui ou sa catégorie (le muet du serveur ne compte pas ici).
    pub fn muted(&self, c: &Channel) -> bool {
        let Some(n) = self.notif else { return false };
        let on = |id: u64| n.channels.get(&id).is_some_and(|x| x.mute.active(self.now_ms));
        on(c.id) || c.parent.is_some_and(on)
    }

    pub fn hides_muted(&self) -> bool {
        self.notif.is_some_and(|n| n.hide_muted)
    }

    /// La ligne de `c` est-elle retirée de la liste ? Le salon ouvert reste
    /// toujours ; un salon non lu reste dans une catégorie repliée, un salon
    /// mentionné reste même muet (comme Discord). Une catégorie muette ne
    /// part que si aucun de ses salons ne reste.
    pub fn hides(&self, g: &Guild, c: &Channel, unread: &HashSet<u64>, mentioned: &HashSet<u64>) -> bool {
        if c.kind == ChannelKind::Category {
            return self.hides_muted() && self.muted(c) && g.channels.iter().filter(|x| x.parent == Some(c.id)).all(|x| self.hides(g, x, unread, mentioned));
        }
        if self.open == Some(c.id) {
            return false;
        }
        let (muted, mentioned) = (self.muted(c), mentioned.contains(&c.id));
        if self.hides_muted() && muted && !mentioned {
            return true;
        }
        c.parent.is_some_and(|p| self.collapsed(p)) && !mentioned && !(unread.contains(&c.id) && !muted)
    }
}

impl Store {
    pub fn list_rules(&self, guild: u64) -> ListRules<'_> {
        let open = self.channel.map(|c| self.threads.get(c).map_or(c, |t| t.parent_id));
        ListRules { notif: self.account.notifs.by_guild.get(&Some(guild)), now_ms: self.now(), open }
    }

    /// Replie ou déplie une catégorie (geste de l'utilisateur).
    pub fn toggle_category(&mut self, guild: u64, category: u64) {
        let on = !self.list_rules(guild).collapsed(category);
        self.account.set_collapsed(Some(guild), vec![(category, on)]);
    }

    /// « Tout replier » : les catégories encore dépliées, en une seule requête.
    pub fn collapse_all_categories(&mut self, guild: u64) {
        let Some(g) = self.guilds.iter().find(|g| g.id == guild) else { return };
        let rules = self.list_rules(guild);
        let list: Vec<(u64, bool)> = g.channels.iter().filter(|c| c.kind == ChannelKind::Category && !rules.collapsed(c.id)).map(|c| (c.id, true)).collect();
        self.account.set_collapsed(Some(guild), list);
    }

    pub fn hides_muted_channels(&self, guild: u64) -> bool {
        self.list_rules(guild).hides_muted()
    }

    /// « Masquer les salons muets » (geste de l'utilisateur).
    pub fn set_hide_muted_channels(&mut self, guild: u64, on: bool) -> Result<(), Refusal> {
        self.account.change_notif(Some(guild), None, Change::HideMuted(on), 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::usersettings::notif::Mute;
    use crate::state::usersettings::{AccountCmd, AccountEvent};
    use crate::state::{Cmd, Event};

    const G: u64 = 9;

    fn chan(id: u64, kind: ChannelKind, parent: Option<u64>) -> Channel {
        Channel { id, name: format!("c{id}"), kind, position: 0, parent, overwrites: Some(Vec::new()), slowmode: 0 }
    }

    fn store() -> (Store, Guild) {
        let mut s = Store::new();
        let g = Guild::new(G, "G".into(), vec![chan(1, ChannelKind::Category, None), chan(2, ChannelKind::Text, Some(1)), chan(3, ChannelKind::Text, Some(1)), chan(4, ChannelKind::Text, None), chan(5, ChannelKind::Category, None)]);
        s.apply(Event::Ready { me: crate::state::model::User::new(7, "Moi".into()), guilds: vec![g.clone()], dms: Vec::new() });
        s.take_commands();
        (s, g)
    }

    fn hidden(s: &Store, g: &Guild, unread: &[u64], mentioned: &[u64]) -> Vec<u64> {
        let r = s.list_rules(G);
        let (unread, mentioned): (HashSet<u64>, HashSet<u64>) = (unread.iter().copied().collect(), mentioned.iter().copied().collect());
        g.channels.iter().filter(|c| r.hides(g, c, &unread, &mentioned)).map(|c| c.id).collect()
    }

    /// Seule requête partie : son identifiant et son changement.
    fn sent(s: &mut Store) -> (u64, Change) {
        let cmds = s.take_commands();
        let [Cmd::Account(AccountCmd::PatchGuild { id, guild: Some(G), patch })] = &cmds[..] else { panic!("{cmds:?}") };
        (*id, patch.change.clone())
    }

    fn answer(s: &mut Store, id: u64, confirmed: Option<GuildNotif>) {
        s.apply(Event::Account(AccountEvent::GuildWritten { id, guild: Some(G), result: Ok(confirmed) }));
    }

    #[test]
    fn collapsed_category_keeps_open_unread_and_mentioned_channels() {
        let (mut s, g) = store();
        assert!(hidden(&s, &g, &[], &[]).is_empty());
        s.toggle_category(G, 1);
        assert_eq!(sent(&mut s).1, Change::Collapsed(vec![(1, true)]));
        assert_eq!(hidden(&s, &g, &[], &[]), vec![2, 3], "la catégorie reste, ses salons partent");
        assert_eq!(hidden(&s, &g, &[2], &[]), vec![3], "un salon non lu reste");
        assert_eq!(hidden(&s, &g, &[], &[3]), vec![2], "un salon mentionné reste");
        s.select_channel(3);
        assert_eq!(hidden(&s, &g, &[], &[]), vec![2], "le salon ouvert reste");
    }

    #[test]
    fn collapses_asked_during_a_write_follow_it() {
        let (mut s, _) = store();
        s.toggle_category(G, 1);
        let (first, _) = sent(&mut s);
        // Deuxième geste avant la réponse : affiché tout de suite, envoyé après.
        s.toggle_category(G, 5);
        assert!(s.take_commands().is_empty());
        assert!(s.list_rules(G).collapsed(5));
        s.toggle_category(G, 5);
        s.toggle_category(G, 5);
        // La réponse de Discord ne connaît que le premier repli.
        let mut confirmed = GuildNotif::new(Some(G));
        confirmed.collapsed.insert(1);
        answer(&mut s, first, Some(confirmed));
        assert_eq!(sent(&mut s).1, Change::Collapsed(vec![(5, true)]), "un seul envoi, dernier état");
        assert!(s.list_rules(G).collapsed(1) && s.list_rules(G).collapsed(5));
    }

    #[test]
    fn collapse_all_sends_one_request() {
        let (mut s, _) = store();
        s.toggle_category(G, 5);
        let (id, _) = sent(&mut s);
        answer(&mut s, id, None);
        s.collapse_all_categories(G);
        let (id, change) = sent(&mut s);
        assert_eq!(change, Change::Collapsed(vec![(1, true)]), "seulement celles qui ne l'étaient pas");
        answer(&mut s, id, None);
        s.collapse_all_categories(G);
        assert!(s.take_commands().is_empty(), "rien à replier : rien ne part");
    }

    #[test]
    fn muted_channels_are_hidden_on_request() {
        let (mut s, g) = store();
        s.account.change_notif(Some(G), Some(4), Change::Mute(Mute { on: true, until_ms: None }), -1).unwrap_or_else(|e| panic!("{e:?}"));
        let (id, _) = sent(&mut s);
        answer(&mut s, id, None);
        assert!(hidden(&s, &g, &[], &[]).is_empty(), "muet mais montré");
        assert!(s.list_rules(G).muted(&g.channels[3]));
        assert!(s.set_hide_muted_channels(G, true).is_ok());
        assert_eq!(sent(&mut s).1, Change::HideMuted(true));
        assert!(s.hides_muted_channels(G));
        assert_eq!(hidden(&s, &g, &[4], &[]), vec![4], "non lu mais muet : masqué");
        assert!(hidden(&s, &g, &[], &[4]).is_empty(), "mentionné : montré");
        s.select_channel(4);
        assert!(hidden(&s, &g, &[], &[]).is_empty(), "ouvert : montré");
    }
}
