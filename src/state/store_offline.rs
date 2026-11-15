//! Historique hors ligne côté magasin (lot H) : un instantané relu du disque
//! est affiché avant READY, puis la session neuve le remplace sans vider
//! l'écran. Module enfant de `store` (salons et permissions sont privés).

use super::{ChanState, Store};
use crate::state::offline::{Prepared, Shown, Snapshot};
use std::collections::HashMap;
use std::time::{Duration, Instant};

impl Store {
    /// Affiche un instantané relu du disque. Refusé (`false`) une fois la
    /// session ouverte (READY arrivé) ou pour un autre compte que celui-ci :
    /// les données fraîches priment toujours.
    pub fn apply_offline(&mut self, snap: Snapshot) -> bool {
        self.apply_prepared(snap.prepare())
    }

    /// Comme [`Store::apply_offline`], l'instantané déjà converti (dans le
    /// fil de l'historique, lot C9).
    pub fn apply_prepared(&mut self, snap: Prepared) -> bool {
        if self.me.is_some() || snap.me.id == 0 {
            return false;
        }
        let me_id = snap.me.id;
        self.me = Some(snap.me);
        self.guilds = snap.guilds;
        self.dms = snap.dms;
        self.offline = Shown { user: Some(me_id) };
        self.refresh_permissions();
        let now = Instant::now();
        for (i, (id, more, msgs)) in snap.chans.into_iter().enumerate() {
            if !self.channel_exists(id) {
                continue;
            }
            let cs = ChanState {
                msgs: msgs.into(),
                loaded: true,
                more,
                offline: true,
                // Ordre d'origine gardé pour l'éviction et le prochain instantané.
                last_view: now.checked_sub(Duration::from_micros(i as u64 + 1)),
                ..Default::default()
            };
            self.chans.insert(id, cs);
        }
        let place = snap.channel.filter(|c| self.channel_exists(*c) && !self.is_hidden(*c));
        match (snap.guild, place) {
            (Some(g), Some(c)) if self.guilds.iter().any(|x| x.id == g && x.channels.iter().any(|ch| ch.id == c)) => {
                self.guild = Some(g);
                self.channel = Some(c);
            }
            (None, Some(c)) if self.dms.iter().any(|d| d.id == c) => {
                self.guild = None;
                self.channel = Some(c);
            }
            (g, _) => {
                self.guild = g.filter(|g| self.guilds.iter().any(|x| x.id == *g));
                self.channel = None;
            }
        }
        true
    }

    /// Instantané affiché, et pour quel compte (avant READY seulement).
    pub fn offline_shown(&self) -> Option<u64> {
        self.offline.user
    }

    /// À READY : sans instantané affiché (ou pour un autre compte), les salons
    /// sont vidés comme avant. Sinon ils restent à l'écran, marqués à
    /// recharger : la page fraîche les remplace à son arrivée, sans écran vide.
    pub(super) fn offline_ready(&mut self) {
        let same = self.offline.user.is_some() && self.offline.user == self.me.as_ref().map(|u| u.id);
        self.offline = Shown::default();
        if !same {
            self.chans.clear();
            return;
        }
        let kept: HashMap<u64, ChanState> = std::mem::take(&mut self.chans)
            .into_iter()
            .filter(|(_, cs)| !cs.history_denied && !cs.msgs.is_empty())
            .map(|(id, mut cs)| {
                cs.offline = true;
                (id, cs)
            })
            .collect();
        self.chans = kept;
    }
}

#[cfg(test)]
mod tests {
    use crate::state::model::{Channel, ChannelKind, Dm, Guild, Message, User};
    use crate::state::offline::Snapshot;
    use crate::state::{Cmd, Event, Store, HISTORY_PAGE};

    const ME: u64 = 1_000_000_000_000_000_001;
    const G: u64 = 2_000_000_000_000_000_001;
    const CHAN: u64 = 3_000_000_000_000_000_001;
    const OTHER: u64 = 3_000_000_000_000_000_002;
    const DM: u64 = 4_000_000_000_000_000_001;

    fn msg(id: u64, ch: u64, text: &str) -> Message {
        Message {
            id,
            channel_id: ch,
            guild_id: Some(G),
            author_id: 7,
            author: "Alice".into(),
            author_avatar: crate::state::model::Avatar::default_for(7),
            content: text.into(),
            attachments: vec![crate::state::model::Attachment::file("photo.png", "https://cdn.discordapp.com/attachments/1/2/photo.png")],
            embeds: Vec::new(),
            mentions: vec![(ME, "Moi".into())],
            mention_everyone: false,
            edited: true,
            system: None,
            meta: Default::default(),
        }
    }

    fn guild() -> Guild {
        let ch = |id, name: &str| Channel { id, name: name.into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        Guild::new(G, "Alpha".into(), vec![ch(CHAN, "général"), ch(OTHER, "hors-sujet")])
    }

    /// Session ouverte, deux salons lus, #général affiché.
    fn online() -> Store {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild()], dms: vec![Dm::new(DM, "Bob".into(), 0)] });
        s.select_guild(G);
        s.select_channel(OTHER);
        s.apply(Event::History { channel_id: OTHER, before: None, messages: vec![msg(20, OTHER, "ailleurs")] });
        s.select_channel(CHAN);
        let page: Vec<Message> = (100..100 + HISTORY_PAGE as u64 + 5).map(|i| msg(i, CHAN, &format!("message {i}"))).collect();
        s.apply(Event::History { channel_id: CHAN, before: None, messages: page[5..].to_vec() });
        s.take_commands();
        s
    }

    #[test]
    fn snapshot_round_trips_and_is_bounded() {
        let s = online();
        let snap = Snapshot::capture(&s).unwrap();
        assert_eq!(snap.chans.first().map(|c| c.id), Some(CHAN), "le salon affiché d'abord");
        assert_eq!(snap.chans.len(), 2);
        assert!(snap.chans[0].msgs.len() <= HISTORY_PAGE);
        let bytes = snap.clone().encode();
        assert_eq!(Snapshot::decode(&bytes), Some(snap));
        assert_eq!(Snapshot::decode(b"{abime"), None);
        assert_eq!(Snapshot::decode(br#"{"version":99,"me":{"id":1}}"#), None, "autre version : ignorée");
        // Champs inconnus ou absents : sans effet.
        assert!(Snapshot::decode(br#"{"version":1,"me":{"id":5,"plus":1},"inconnu":[1]}"#).is_some());
    }

    #[test]
    fn oversized_snapshot_drops_oldest_channels() {
        let mut snap = Snapshot::capture(&online()).unwrap();
        let big = "x".repeat(4000);
        for c in &mut snap.chans {
            for m in &mut c.msgs {
                m.content = big.clone();
            }
        }
        let template = snap.chans[0].clone();
        for i in 0..40 {
            snap.chans.push(crate::state::offline::SChan { id: 9_000 + i, ..template.clone() });
        }
        let bytes = snap.encode();
        assert!(bytes.len() <= crate::state::offline::MAX_BYTES, "{}", bytes.len());
        let back = Snapshot::decode(&bytes).unwrap();
        assert_eq!(back.chans.first().map(|c| c.id), Some(CHAN), "le plus récent reste");
    }

    #[test]
    fn cache_is_shown_before_ready_then_replaced_without_blank() {
        let snap = Snapshot::capture(&online()).unwrap();
        let mut s = Store::new();
        assert!(s.apply_offline(snap.clone()));
        assert_eq!((s.guild, s.channel), (Some(G), Some(CHAN)));
        let cs = &s.chans[&CHAN];
        assert!(cs.loaded && cs.offline && !cs.msgs.is_empty());
        assert_eq!(cs.msgs.back().map(|m| m.content.as_str()), Some("message 154"));
        assert_eq!(cs.msgs[0].attachments[0].kind, crate::state::model::MediaKind::Image);
        assert!(s.take_commands().is_empty(), "rien demandé tant qu'aucun salon n'est ouvert");

        // Ouvrir un salon gardé avant READY : affiché tout de suite, rafraîchi en même temps.
        s.select_channel(OTHER);
        assert_eq!(s.chans[&OTHER].msgs.len(), 1);
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: OTHER, before: None }]);
        s.apply(Event::History { channel_id: OTHER, before: None, messages: vec![msg(21, OTHER, "frais avant READY")] });
        assert!(!s.chans[&OTHER].offline);
        // Retour sur #général : la page fraîche échoue (pas de réseau), le cache reste.
        s.select_channel(CHAN);
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
        s.apply(Event::HistoryError { channel_id: CHAN, before: None, after: None, error: crate::state::Problem::Api("réseau".into()) });

        // READY du même compte : rien n'est vidé, la page fraîche est demandée.
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild()], dms: Vec::new() });
        assert_eq!(s.offline_shown(), None);
        assert!(!s.chans[&CHAN].msgs.is_empty(), "pas d'écran vide entre le cache et la page fraîche");
        let cmds = s.take_commands();
        assert!(cmds.contains(&Cmd::History { channel_id: CHAN, before: None }), "{cmds:?}");
        s.apply(Event::History { channel_id: CHAN, before: None, messages: vec![msg(500, CHAN, "frais")] });
        let cs = &s.chans[&CHAN];
        assert!(!cs.offline && cs.loaded);
        assert_eq!(cs.msgs.iter().map(|m| m.id).collect::<Vec<_>>(), vec![500]);
        // Une fois en ligne, apply_offline ne touche plus à rien.
        assert!(!s.apply_offline(snap));
    }

    #[test]
    fn ready_of_another_account_clears_the_cache() {
        let mut s = Store::new();
        assert!(s.apply_offline(Snapshot::capture(&online()).unwrap()));
        s.apply(Event::Ready { me: User::new(ME + 1, "Autre".into()), guilds: vec![guild()], dms: Vec::new() });
        assert!(s.chans.values().all(|cs| !cs.offline), "aucun message de l'autre compte");
    }

    #[test]
    fn failed_refresh_keeps_cache_and_can_retry() {
        let mut s = Store::new();
        s.apply_offline(Snapshot::capture(&online()).unwrap());
        s.select_channel(CHAN);
        s.take_commands();
        s.apply(Event::HistoryError { channel_id: CHAN, before: None, after: None, error: crate::state::Problem::Api("réseau".into()) });
        let cs = &s.chans[&CHAN];
        assert!(cs.error_newer.is_some() && !cs.msgs.is_empty(), "sans réseau, l'historique reste lisible");
        assert!(s.return_to_present(CHAN), "« Réessayer » redemande la page");
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
    }

    #[test]
    fn clear_forgets_the_cache() {
        let mut s = Store::new();
        s.apply_offline(Snapshot::capture(&online()).unwrap());
        s.clear();
        assert!(s.me.is_none() && s.chans.is_empty() && s.offline_shown().is_none());
    }
}
