//! Réglages de notification par serveur et par salon (`user_guild_settings`
//! du READY, USER_GUILD_SETTINGS_UPDATE, `PATCH /users/@me/guilds/{id}/settings`).
//! Forme : docs.discord.food, « User Settings › User Guild Settings ».

use std::collections::{HashMap, HashSet};

/// Niveau de notification (docs.discord.food, « Message Notification Level »).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Level {
    All,
    #[default]
    Mentions,
    Nothing,
    /// Salon : celui du serveur.
    Inherit,
}

impl Level {
    pub fn from_code(v: u8) -> Level {
        match v {
            0 => Level::All,
            1 => Level::Mentions,
            2 => Level::Nothing,
            _ => Level::Inherit,
        }
    }

    pub fn code(self) -> u8 {
        match self {
            Level::All => 0,
            Level::Mentions => 1,
            Level::Nothing => 2,
            Level::Inherit => 3,
        }
    }
}

/// Muet, avec son échéance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mute {
    pub on: bool,
    /// Fin (ms Unix) ; `None` : jusqu'à ce qu'on le retire.
    pub until_ms: Option<i64>,
}

impl Mute {
    pub fn active(&self, now_ms: i64) -> bool {
        self.on && self.until_ms.is_none_or(|t| t > now_ms)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ChannelNotif {
    pub mute: Mute,
    pub level: Level,
    /// Drapeaux du salon (`OPT_IN_ENABLED` : suivi, « Parcourir les salons »).
    pub flags: u64,
}

/// Réglages d'un serveur (`guild` vaut `None` pour les messages privés).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildNotif {
    pub guild: Option<u64>,
    pub mute: Mute,
    pub level: Level,
    pub suppress_everyone: bool,
    pub suppress_roles: bool,
    pub channels: HashMap<u64, ChannelNotif>,
    /// Drapeaux du serveur (`OPT_IN_CHANNELS_ON` : seuls les salons suivis sont montrés).
    pub flags: u64,
    /// « Masquer les salons muets » (`hide_muted_channels`).
    pub hide_muted: bool,
    /// Catégories repliées (`collapsed` de leur dérogation).
    pub collapsed: HashSet<u64>,
    /// « Supprimer les mentions surlignées » (`notify_highlights` vaut 1 ;
    /// 0 : non réglé, 2 : activées).
    pub highlights_off: bool,
    /// « Rendre muets les nouveaux événements » (`mute_scheduled_events`).
    pub mute_events: bool,
    /// « Notifications push mobiles » (`mobile_push`, vrai par défaut).
    pub mobile_push: bool,
}

impl GuildNotif {
    /// Réglages d'un serveur dont Discord n'a rien envoyé (valeurs par défaut).
    pub fn new(guild: Option<u64>) -> Self {
        GuildNotif { guild, mute: Mute::default(), level: Level::Mentions, suppress_everyone: false, suppress_roles: false, channels: HashMap::new(), flags: 0, hide_muted: false, collapsed: HashSet::new(), highlights_off: false, mute_events: false, mobile_push: true }
    }

    pub fn channel(&self, id: u64) -> ChannelNotif {
        self.channels.get(&id).cloned().unwrap_or(ChannelNotif { mute: Mute::default(), level: Level::Inherit, flags: 0 })
    }
}

/// Ce qu'on change d'un coup (un geste = une requête).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Change {
    Mute(Mute),
    Level(Level),
    SuppressEveryone(bool),
    SuppressRoles(bool),
    /// Drapeaux du serveur, ou du salon visé (« Parcourir les salons ») : valeur complète.
    Flags(u64),
    /// « Masquer les salons muets » du serveur.
    HideMuted(bool),
    /// Catégories repliées (`true`) ou dépliées, toutes dans la même requête ;
    /// toujours au niveau du serveur (`channel` absent).
    Collapsed(Vec<(u64, bool)>),
    /// « Supprimer les mentions surlignées » du serveur.
    HighlightsOff(bool),
    /// « Rendre muets les nouveaux événements » du serveur.
    MuteEvents(bool),
    /// « Notifications push mobiles » du serveur.
    MobilePush(bool),
    /// Retire l'exception d'un salon (fenêtre des notifications du serveur) :
    /// niveau hérité et son rétabli, dans la même requête.
    Reset,
}

/// Corps d'un `PATCH /users/@me/guilds/{id}/settings` : un seul réglage,
/// du serveur ou d'un de ses salons. `window_secs` : durée choisie (-1 :
/// sans fin), envoyée comme `selected_time_window`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuildPatch {
    pub channel: Option<u64>,
    pub change: Change,
    pub window_secs: i64,
}

impl GuildPatch {
    /// Applique la modification à `g` (écriture optimiste).
    pub fn apply(&self, g: &mut GuildNotif) {
        match self.channel {
            None => match &self.change {
                Change::Mute(m) => g.mute = *m,
                Change::Level(l) => g.level = *l,
                Change::SuppressEveryone(b) => g.suppress_everyone = *b,
                Change::SuppressRoles(b) => g.suppress_roles = *b,
                Change::Flags(f) => g.flags = *f,
                Change::HideMuted(b) => g.hide_muted = *b,
                Change::HighlightsOff(b) => g.highlights_off = *b,
                Change::MuteEvents(b) => g.mute_events = *b,
                Change::MobilePush(b) => g.mobile_push = *b,
                Change::Reset => {}
                Change::Collapsed(list) => {
                    for (id, on) in list {
                        if *on {
                            g.collapsed.insert(*id);
                        } else {
                            g.collapsed.remove(id);
                        }
                    }
                }
            },
            Some(c) => {
                let mut ch = g.channel(c);
                match &self.change {
                    Change::Mute(m) => ch.mute = *m,
                    Change::Level(l) => ch.level = *l,
                    Change::Flags(f) => ch.flags = *f,
                    Change::Reset => {
                        ch.level = Level::Inherit;
                        ch.mute = Mute::default();
                    }
                    Change::SuppressEveryone(_) | Change::SuppressRoles(_) | Change::HideMuted(_) | Change::Collapsed(_) | Change::HighlightsOff(_) | Change::MuteEvents(_) | Change::MobilePush(_) => return,
                }
                g.channels.insert(c, ch);
            }
        }
    }
}

/// « Parcourir les salons » (docs.discord.food, « User Settings › User Guild
/// Settings Flags » et « Channel Override Flags ») : drapeaux du serveur,
/// montrer tous les salons (`OPT_IN_CHANNELS_OFF`) ou seulement les suivis
/// (`OPT_IN_CHANNELS_ON`) ; drapeau d'un salon ou d'une catégorie suivis.
pub const OPT_IN_CHANNELS_OFF: u64 = 1 << 13;
pub const OPT_IN_CHANNELS_ON: u64 = 1 << 14;
pub const OPT_IN_ENABLED: u64 = 1 << 12;

impl GuildNotif {
    /// Salons et catégories qui ont une exception de notification (niveau
    /// propre, ou muet encore actif), par identifiant croissant.
    pub fn overrides(&self, now_ms: i64) -> Vec<u64> {
        let mut ids: Vec<u64> = self.channels.iter().filter(|(_, c)| c.level != Level::Inherit || c.mute.active(now_ms)).map(|(id, _)| *id).collect();
        ids.sort_unstable();
        ids
    }

    /// Seuls les salons suivis sont montrés.
    pub fn opt_in(&self) -> bool {
        self.flags & OPT_IN_CHANNELS_ON != 0
    }

    /// Drapeaux du serveur pour « tout montrer » (`false`) ou « seulement les suivis ».
    pub fn opt_in_flags(&self, on: bool) -> u64 {
        (self.flags & !(OPT_IN_CHANNELS_ON | OPT_IN_CHANNELS_OFF)) | if on { OPT_IN_CHANNELS_ON } else { OPT_IN_CHANNELS_OFF }
    }

    /// Ce salon (lui-même, ou sa catégorie `parent`) est suivi.
    pub fn followed(&self, channel: u64, parent: Option<u64>) -> bool {
        let on = |id: u64| self.channels.get(&id).is_some_and(|c| c.flags & OPT_IN_ENABLED != 0);
        on(channel) || parent.is_some_and(on)
    }

    /// Drapeaux du salon une fois suivi (`true`) ou plus suivi.
    pub fn follow_flags(&self, channel: u64, follow: bool) -> u64 {
        let f = self.channels.get(&channel).map_or(0, |c| c.flags);
        if follow { f | OPT_IN_ENABLED } else { f & !OPT_IN_ENABLED }
    }
}

/// Tous les réglages reçus. Un serveur absent a les valeurs par défaut.
#[derive(Clone, Debug, Default)]
pub struct Notifs {
    pub by_guild: HashMap<Option<u64>, GuildNotif>,
}

impl Notifs {
    pub fn get(&self, guild: Option<u64>) -> GuildNotif {
        self.by_guild.get(&guild).cloned().unwrap_or_else(|| GuildNotif::new(guild))
    }

    /// Le salon `channel` (de catégorie `parent`, du serveur `guild`) est-il
    /// muet, lui, sa catégorie ou son serveur ?
    pub fn muted(&self, guild: Option<u64>, channel: u64, parent: Option<u64>, now_ms: i64) -> bool {
        let Some(g) = self.by_guild.get(&guild) else { return false };
        g.mute.active(now_ms)
            || g.channels.get(&channel).is_some_and(|c| c.mute.active(now_ms))
            || parent.and_then(|p| g.channels.get(&p)).is_some_and(|c| c.mute.active(now_ms))
    }

    /// Les mentions @everyone/@here de ce serveur sont-elles ignorées ?
    pub fn suppresses_everyone(&self, guild: Option<u64>) -> bool {
        self.by_guild.get(&guild).is_some_and(|g| g.suppress_everyone)
    }

    /// Prochaine fin de muet à venir (pour un réveil unique de l'interface).
    pub fn next_unmute(&self, now_ms: i64) -> Option<i64> {
        self.by_guild
            .values()
            .flat_map(|g| std::iter::once(g.mute).chain(g.channels.values().map(|c| c.mute)))
            .filter_map(|m| m.until_ms.filter(|t| m.on && *t > now_ms))
            .min()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mute_of_guild_category_or_channel() {
        let mut n = Notifs::default();
        assert!(!n.muted(Some(1), 10, None, 0));
        let mut g = GuildNotif::new(Some(1));
        g.channels.insert(20, ChannelNotif { mute: Mute { on: true, until_ms: None }, level: Level::Inherit, flags: 0 });
        g.channels.insert(30, ChannelNotif { mute: Mute { on: true, until_ms: Some(1000) }, level: Level::Inherit, flags: 0 });
        n.by_guild.insert(Some(1), g);
        assert!(n.muted(Some(1), 20, None, 0), "salon muet");
        assert!(n.muted(Some(1), 21, Some(20), 0), "catégorie muette");
        assert!(n.muted(Some(1), 30, None, 999));
        assert!(!n.muted(Some(1), 30, None, 1000), "muet échu");
        assert_eq!(n.next_unmute(0), Some(1000));
        assert_eq!(n.next_unmute(1000), None);
        let patch = GuildPatch { channel: None, change: Change::Mute(Mute { on: true, until_ms: None }), window_secs: -1 };
        let mut g = n.get(Some(1));
        patch.apply(&mut g);
        n.by_guild.insert(Some(1), g);
        assert!(n.muted(Some(1), 99, None, i64::MAX - 1), "serveur muet sans fin");
    }

    #[test]
    fn browse_channels_flags() {
        let mut g = GuildNotif::new(Some(1));
        assert!(!g.opt_in());
        GuildPatch { channel: None, change: Change::Flags(g.opt_in_flags(true)), window_secs: 0 }.apply(&mut g);
        assert!(g.opt_in());
        assert_eq!(g.flags, OPT_IN_CHANNELS_ON);
        assert!(!g.followed(5, Some(4)));
        GuildPatch { channel: Some(4), change: Change::Flags(g.follow_flags(4, true)), window_secs: 0 }.apply(&mut g);
        assert!(g.followed(5, Some(4)), "catégorie suivie : ses salons aussi");
        assert!(g.followed(4, None));
        assert_eq!(g.follow_flags(4, false), 0);
        assert_eq!(g.opt_in_flags(false), OPT_IN_CHANNELS_OFF);
    }

    #[test]
    fn patch_on_a_channel_only_touches_it() {
        let mut g = GuildNotif::new(Some(1));
        GuildPatch { channel: Some(5), change: Change::Level(Level::Nothing), window_secs: 0 }.apply(&mut g);
        assert_eq!(g.channel(5).level, Level::Nothing);
        assert_eq!(g.level, Level::Mentions);
        GuildPatch { channel: Some(5), change: Change::SuppressEveryone(true), window_secs: 0 }.apply(&mut g);
        assert!(!g.suppress_everyone);
        // Les trois interrupteurs du serveur (F12) n'existent pas pour un salon.
        for c in [Change::HighlightsOff(true), Change::MuteEvents(true), Change::MobilePush(false)] {
            GuildPatch { channel: Some(5), change: c.clone(), window_secs: 0 }.apply(&mut g);
            assert!(!g.highlights_off && !g.mute_events && g.mobile_push);
            let mut whole = g.clone();
            GuildPatch { channel: None, change: c, window_secs: 0 }.apply(&mut whole);
            assert_ne!(whole, g);
        }
        GuildPatch { channel: Some(5), change: Change::Mute(Mute { on: true, until_ms: None }), window_secs: -1 }.apply(&mut g);
        GuildPatch { channel: Some(5), change: Change::Reset, window_secs: 0 }.apply(&mut g);
        assert_eq!((g.channel(5).level, g.channel(5).mute.on), (Level::Inherit, false), "exception retirée");
        assert!(g.overrides(0).is_empty());
        GuildPatch { channel: Some(6), change: Change::Level(Level::All), window_secs: 0 }.apply(&mut g);
        GuildPatch { channel: Some(7), change: Change::Mute(Mute { on: true, until_ms: Some(10) }), window_secs: 0 }.apply(&mut g);
        assert_eq!(g.overrides(5), vec![6, 7]);
        assert_eq!(g.overrides(10), vec![6], "muet échu : plus une exception");
        for v in 0..=4 {
            assert_eq!(Level::from_code(v).code(), v.min(3));
        }
    }
}
