//! Ce que la passerelle sait de l'écran ([`Interest`]) : salon ouvert, moi,
//! et salons « calmes », où un message ordinaire ne change rien à l'écran
//! (déjà non lu, ou muet, et notifications réservées aux mentions).
//!
//! La liste des salons calmes est refaite seulement quand ce dont elle
//! dépend change (salons non lus, serveurs, réglages de notification, fin
//! d'un muet) : une empreinte de quelques entiers est comparée à chaque
//! image, le parcours des salons ne se fait qu'alors. Le salon ouvert n'en
//! fait pas partie (le verdict l'écarte lui-même) : changer de salon ne
//! refait pas le parcours.

use super::App;
use crate::state::interest::{Interest, ALSO_MAX, GUILDS_MAX};
use crate::state::notify;
use crate::state::usersettings::notif::Level;
use std::{
    collections::HashSet,
    hash::{Hash, Hasher},
    sync::Arc,
};

#[derive(Default)]
pub(super) struct Watch {
    shared: Arc<Interest>,
    /// Empreinte des données de la dernière liste publiée.
    stamp: Option<u64>,
}

impl Watch {
    pub(super) fn shared(&self) -> Arc<Interest> {
        self.shared.clone()
    }
}

/// Salons d'un serveur où un message ordinaire d'un autre (sans mention)
/// ne change ni pastille, ni non-lu, ni notification.
pub(super) fn quiet_channels(s: &crate::state::Store, now_ms: i64) -> HashSet<u64> {
    let n = &s.account.notifs;
    let mut out = HashSet::new();
    for g in &s.guilds {
        for c in &g.channels {
            let muted = n.muted(Some(g.id), c.id, c.parent, now_ms);
            let silent = muted || matches!(notify::level(n, Some(g.id), c.id, c.parent), Level::Mentions | Level::Nothing);
            if silent && (muted || s.unread().contains(&c.id)) {
                out.insert(c.id);
            }
        }
    }
    out
}

impl App {
    /// À chaque image : ce que la passerelle doit savoir de l'écran.
    pub(super) fn publish_interest(&mut self) {
        let s = &self.store;
        let now = crate::state::store::now_ms();
        let mut h = std::collections::hash_map::DefaultHasher::new();
        s.guilds.len().hash(&mut h);
        s.guilds.iter().map(|g| g.channels.len()).sum::<usize>().hash(&mut h);
        // Ensemble des non-lus sans ordre : somme et ou exclusif des identifiants.
        let (mut sum, mut xor) = (0u64, 0u64);
        for c in s.unread() {
            sum = sum.wrapping_add(*c);
            xor ^= c.rotate_left(17);
        }
        (s.unread().len(), sum, xor).hash(&mut h);
        notifs_stamp(&s.account.notifs).hash(&mut h);
        next_unmute(s, now).hash(&mut h);
        let stamp = h.finish();
        if self.interest.stamp != Some(stamp) {
            self.interest.stamp = Some(stamp);
            self.interest.shared.set_quiet(quiet_channels(s, now));
        }
        self.interest.shared.show(s.channel, s.me.as_ref().map(|u| u.id));
        // Serveurs à l'écran (lot « perf ») : colonne des salons, ma connexion
        // vocale (panneau vocal), salons secondaires. Les CHANNEL_UPDATE des autres attendent.
        let mut guilds = [0u64; GUILDS_MAX];
        let voice = s.me.as_ref().and_then(|u| s.voice.state(u.id)).and_then(|v| v.guild_id);
        let also = self.panes_visible().iter().take(ALSO_MAX).filter_map(|c| s.guild_of(*c));
        for (slot, g) in guilds.iter_mut().zip(s.guild.into_iter().chain(voice).chain(also)) {
            *slot = g;
        }
        self.interest.shared.show_guilds(&guilds);
        // Messages dessinés à cette image (lot C9) : une modification plus haut ou plus bas attend.
        self.interest.shared.show_messages(s.channel.and_then(|c| s.chans.get(&c)).and_then(|cs| cs.viewing()));
    }
}

/// Prochaine fin de muet : quand elle passe, l'empreinte change.
fn next_unmute(s: &crate::state::Store, now: i64) -> Option<i64> {
    s.account.notifs.next_unmute(now)
}

/// Empreinte des réglages de notification (niveaux et muets par serveur et
/// par salon), sans ordre et sans allocation.
fn notifs_stamp(n: &crate::state::usersettings::notif::Notifs) -> u64 {
    use crate::state::usersettings::notif::Mute;
    fn mix(x: u64) -> u64 {
        let x = x.wrapping_mul(0x9E37_79B9_7F4A_7C15);
        x ^ (x >> 29)
    }
    fn mute(m: &Mute) -> u64 {
        u64::from(m.on) ^ (m.until_ms.unwrap_or(-1) as u64).rotate_left(1)
    }
    let mut total = 0u64;
    for (g, v) in &n.by_guild {
        let own = mix(g.unwrap_or(u64::MAX) ^ mute(&v.mute).rotate_left(7) ^ (v.level as u64).rotate_left(13));
        let mut chans = 0u64;
        for (id, c) in &v.channels {
            chans = chans.wrapping_add(mix(*id ^ mute(&c.mute).rotate_left(7) ^ (c.level as u64).rotate_left(13)));
        }
        total = total.wrapping_add(mix(own ^ chans));
    }
    total
}

