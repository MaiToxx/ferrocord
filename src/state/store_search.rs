//! Raccord du magasin avec la recherche (lot T12). Module enfant de `store`
//! (déclaré par `#[path]`) : un saut vers un message remplace la fenêtre de
//! l'historique du salon, avec les mêmes règles que les pages `before` et
//! `after` (réponses périmées ignorées, une demande à la fois par sens).
//!
//! Fenêtre après un saut : la page `around` (50 messages au plus autour du
//! message visé). `more` vaut vrai s'il y a des messages plus anciens dans la
//! page (il peut en rester avant), `newer` s'il y en a de plus récents (la
//! fenêtre a quitté le présent ; elle y revient par les pages `after`, comme
//! après une longue remontée). Une dernière page partie avant le saut est
//! ignorée à son arrivée (`skip_present`) : elle écraserait la fenêtre.

use super::super::event::{Cmd, HISTORY_PAGE};
use super::super::model::{ChannelKind, Message};
use super::super::notice::Notice;
use super::super::perms::{Permissions, Verdict};
use super::super::search::{Hit, Names, Parsed, Phase, Scope, SearchCmd, SearchEvent, Sort};
use super::{Effect, Store};

/// Issue d'un saut vers un message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Jump {
    /// Déjà en mémoire : le salon est ouvert, la vue peut y aller.
    Shown,
    /// Page autour du message demandée.
    Loading,
    /// Salon inconnu de cette session ou masqué (fil archivé non chargé, serveur quitté).
    Unknown,
    /// Historique illisible (READ_MESSAGE_HISTORY refusé) : le salon est ouvert.
    Denied,
}

impl Store {
    /// La dernière page attendue de ce salon est périmée (saut) : vrai une fois.
    pub(super) fn skip_present(&mut self, channel_id: u64) -> bool {
        self.chans.get_mut(&channel_id).is_some_and(|cs| std::mem::take(&mut cs.skip_present))
    }

    /// Où chercher depuis la vue ouverte : le serveur entier, sinon le MP ou groupe.
    pub fn search_scope(&self) -> Option<Scope> {
        match self.guild {
            Some(g) => Some(Scope::Guild(g)),
            None => self.channel.map(Scope::Channel),
        }
    }

    /// Personnes et salons que `from:`, `mentions:` et `in:` savent nommer.
    pub fn search_names(&self, scope: Scope) -> Names {
        let channel = match scope {
            Scope::Channel(c) => Some(c),
            Scope::Guild(_) => self.channel,
        };
        let people = channel.map(|c| self.known_people(c)).unwrap_or_default();
        let channels = match scope {
            Scope::Guild(g) => self
                .guilds
                .iter()
                .find(|x| x.id == g)
                .map(|x| x.channels.iter().filter(|c| c.kind != ChannelKind::Category && !self.is_hidden(c.id)).map(|c| (c.id, c.name.clone())).collect())
                .unwrap_or_default(),
            Scope::Channel(_) => Vec::new(),
        };
        Names { people, channels, private: matches!(scope, Scope::Channel(_)), ..Names::default() }
    }

    /// Lance une recherche (après « Entrée » dans le champ).
    pub fn start_search(&mut self, scope: Scope, text: String, parsed: Parsed) {
        let cmd = self.search.start(scope, text, parsed);
        self.outbox.push(Cmd::Search(cmd));
    }

    /// Page `page` des résultats (aussi « Réessayer » sur la page affichée).
    pub fn search_page(&mut self, page: usize) -> bool {
        let Some(cmd) = self.search.goto(page) else { return false };
        self.outbox.push(Cmd::Search(cmd));
        true
    }

    pub fn search_sort(&mut self, sort: Sort) -> bool {
        let Some(cmd) = self.search.set_sort(sort) else { return false };
        self.outbox.push(Cmd::Search(cmd));
        true
    }

    /// Onglet du panneau de résultats (F16) : une requête par clic.
    pub fn search_tab(&mut self, tab: crate::state::search::Tab) -> bool {
        let Some(cmd) = self.search.set_tab(tab) else { return false };
        self.outbox.push(Cmd::Search(cmd));
        true
    }

    pub fn close_search(&mut self) {
        self.search.clear();
    }

    /// Un saut vers un message de ce salon attend sa page.
    pub fn jump_pending(&self, channel_id: u64) -> bool {
        self.chans.get(&channel_id).is_some_and(|cs| cs.asked_around.is_some())
    }

    /// Ouvre le salon du message et charge l'historique autour de lui s'il
    /// n'est pas en mémoire. Le serveur est celui que le magasin connaît
    /// pour ce salon (jamais celui d'un lien, qui pourrait mentir).
    pub fn jump_to_message(&mut self, channel_id: u64, message_id: u64) -> Jump {
        if !self.channel_exists(channel_id) || self.is_hidden(channel_id) || self.is_forum(channel_id) {
            return Jump::Unknown;
        }
        self.guild = self.chan_guild.get(&channel_id).copied().or_else(|| self.threads.get(channel_id).map(|t| t.guild_id));
        let denied = self.can(channel_id, Permissions::READ_MESSAGE_HISTORY) == Verdict::Refuse;
        let cs = self.chans.entry(channel_id).or_default();
        let outcome = if cs.contains(message_id) {
            Jump::Shown
        } else if denied {
            Jump::Denied
        } else {
            if cs.asked_around != Some(message_id) {
                // Une dernière page en route (premier chargement, retour au
                // présent) arriverait après et écraserait la page du saut.
                if cs.loading && cs.asked_around.is_none() {
                    cs.skip_present = true;
                }
                // Rien en mémoire : le salon attend cette page-ci au lieu de la dernière.
                if !cs.loaded {
                    cs.loading = true;
                }
                cs.asked_around = Some(message_id);
                cs.error = None;
                self.outbox.push(Cmd::Search(SearchCmd::Around { channel_id, around: message_id }));
            }
            Jump::Loading
        };
        self.select_channel(channel_id);
        outcome
    }

    pub(super) fn apply_search(&mut self, e: SearchEvent) -> Option<Effect> {
        match e {
            SearchEvent::Results { seq, total, messages, threads } => {
                self.search.current(seq)?;
                let hits: Vec<Hit> = messages
                    .into_iter()
                    .filter_map(|m| {
                        let thread_name = threads.iter().find(|(id, _)| *id == m.channel_id).map(|(_, n)| n.clone());
                        // Même rendu que l'historique (balises, réponse, fil), sans en changer l'ordre.
                        self.render_page(vec![m]).0.pop().map(|message| Hit { message, thread_name })
                    })
                    .collect();
                let a = self.search.current(seq)?;
                a.hits = hits;
                a.total = Some(total);
                a.phase = Phase::Done;
            }
            SearchEvent::Indexing { seq, .. } => self.search.current(seq)?.phase = Phase::Indexing,
            SearchEvent::NotIndexed { seq } => self.search.current(seq)?.phase = Phase::NotIndexed,
            SearchEvent::Failed { seq, problem } => self.search.current(seq)?.phase = Phase::Failed(problem),
            SearchEvent::Around { channel_id, around, messages } => self.around_page(channel_id, around, messages),
            SearchEvent::AroundFailed { channel_id, around, problem } => {
                if self.around_ended(channel_id, around) {
                    return Some(Effect::Toast(Notice::Error(problem)));
                }
            }
        }
        None
    }

    /// Fin sans page d'une demande `around` : un salon jamais chargé repart
    /// sur la dernière page (déjà en route, ou demandée ici). Vrai si c'était
    /// bien la demande en cours.
    fn around_ended(&mut self, channel_id: u64, around: u64) -> bool {
        let Some(cs) = self.chans.get_mut(&channel_id) else { return false };
        if cs.asked_around != Some(around) {
            return false;
        }
        cs.asked_around = None;
        if std::mem::take(&mut cs.skip_present) {
            // La dernière page en route n'est plus périmée.
        } else if !cs.loaded {
            self.outbox.push(Cmd::History { channel_id, before: None });
        }
        true
    }

    fn around_page(&mut self, channel_id: u64, around: u64, messages: Vec<Message>) {
        let (page, raws) = self.render_page(messages);
        // Message introuvable (supprimé entre-temps) : comme un échec, sans message.
        if page.is_empty() {
            self.around_ended(channel_id, around);
            return;
        }
        let Some(cs) = self.chans.get_mut(&channel_id) else { return };
        if cs.asked_around != Some(around) {
            return;
        }
        cs.asked_around = None;
        cs.loading = false;
        let older = page.iter().any(|m| m.id < around);
        let newer = page.iter().any(|m| m.id > around) || page.len() >= HISTORY_PAGE;
        let live = &cs.live;
        cs.stamped.retain(|id, _| live.iter().any(|m| m.id == *id));
        cs.stamped.extend(raws);
        cs.msgs = page.into();
        cs.loaded = true;
        cs.error = None;
        cs.error_newer = None;
        cs.asked_older = None;
        cs.asked_newer = None;
        cs.viewing = None;
        cs.more = older;
        cs.rev.bump();
        if newer {
            cs.newer = true;
        } else {
            // Le message visé est le plus récent : la fenêtre touche le présent.
            cs.back_to_present();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::event::Event;
    use super::super::super::model::{Avatar, Channel, Guild};
    use super::super::super::notice::Problem;
    use super::super::super::search::{parse, Sort};
    use super::*;

    const G: u64 = 1;
    const CHAN: u64 = 100;
    const OTHER: u64 = 101;

    fn msg(id: u64, channel_id: u64) -> Message {
        Message {
            id,
            channel_id,
            guild_id: Some(G),
            author_id: 2,
            author: "Bob".into(),
            author_avatar: Avatar::default_for(2),
            content: format!("m{id}"),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    fn store() -> Store {
        let mut s = Store::new();
        let ch = |id| Channel { id, name: format!("c{id}"), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        s.apply(Event::GuildCreate(Guild::new(G, "g".into(), vec![ch(CHAN), ch(OTHER)])));
        s
    }

    fn ids(s: &Store) -> Vec<u64> {
        s.chans[&CHAN].msgs.iter().map(|m| m.id).collect()
    }

    fn page(r: std::ops::RangeInclusive<u64>) -> Vec<Message> {
        r.map(|i| msg(i, CHAN)).collect()
    }

    fn around(s: &mut Store, at: u64, r: std::ops::RangeInclusive<u64>) {
        s.apply(Event::Search(SearchEvent::Around { channel_id: CHAN, around: at, messages: page(r) }));
    }

    #[test]
    fn jump_into_unloaded_channel_waits_for_its_page() {
        let mut s = store();
        s.select_channel(OTHER);
        s.take_commands();
        assert_eq!(s.jump_to_message(CHAN, 500), Jump::Loading);
        assert_eq!((s.guild, s.channel), (Some(G), Some(CHAN)));
        // Pas de dernière page : la page autour du message la remplace.
        assert_eq!(s.take_commands(), vec![Cmd::Search(SearchCmd::Around { channel_id: CHAN, around: 500 })]);
        assert!(s.jump_pending(CHAN) && s.chans[&CHAN].loading);
        // Même saut une seconde fois : rien de plus.
        assert_eq!(s.jump_to_message(CHAN, 500), Jump::Loading);
        assert!(s.take_commands().is_empty());
        around(&mut s, 999, 480..=520);
        assert!(s.jump_pending(CHAN), "page d'une autre demande ignorée");
        around(&mut s, 500, 480..=520);
        let cs = &s.chans[&CHAN];
        assert!(!s.jump_pending(CHAN) && cs.loaded && !cs.loading && cs.more && cs.newer);
        assert_eq!(ids(&s).len(), 41);
        // Les bords se rechargent comme après une remontée.
        assert!(s.load_older(CHAN) && s.load_newer(CHAN));
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: Some(480) }, Cmd::HistoryAfter { channel_id: CHAN, after: 520 }]);
        s.apply(Event::HistoryAfter { channel_id: CHAN, after: 520, messages: page(521..=530) });
        assert!(!s.chans[&CHAN].newer, "page courte : retour au présent");
        assert_eq!(s.jump_to_message(CHAN, 505), Jump::Shown);
        assert!(s.take_commands().is_empty());
    }

    #[test]
    fn present_page_in_flight_is_dropped_after_a_jump() {
        let mut s = store();
        s.select_channel(CHAN);
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
        s.jump_to_message(CHAN, 100);
        s.take_commands();
        around(&mut s, 100, 90..=110);
        // La dernière page demandée avant le saut arrive après : ignorée.
        s.apply(Event::History { channel_id: CHAN, before: None, messages: page(900..=949) });
        assert_eq!(ids(&s).first(), Some(&90));
        assert!(s.chans[&CHAN].newer);
        // Le retour au présent qui suit n'est pas ignoré.
        assert!(s.return_to_present(CHAN));
        s.apply(Event::History { channel_id: CHAN, before: None, messages: page(900..=949) });
        assert_eq!(ids(&s).first(), Some(&900));
        assert!(!s.chans[&CHAN].newer);
    }

    #[test]
    fn jump_to_latest_touches_the_present_and_failures_fall_back() {
        let mut s = store();
        s.select_channel(CHAN);
        s.apply(Event::History { channel_id: CHAN, before: None, messages: page(900..=949) });
        s.take_commands();
        assert_eq!(s.jump_to_message(CHAN, 960), Jump::Loading);
        around(&mut s, 960, 940..=960);
        assert!(!s.chans[&CHAN].newer && s.chans[&CHAN].more);
        // Échec dans un salon jamais chargé : la dernière page est demandée.
        let mut s = store();
        s.jump_to_message(CHAN, 5);
        s.take_commands();
        let eff = s.apply(Event::Search(SearchEvent::AroundFailed { channel_id: CHAN, around: 5, problem: Problem::Forbidden }));
        assert_eq!(eff, Some(Effect::Toast(Notice::Error(Problem::Forbidden))));
        assert_eq!(s.take_commands(), vec![Cmd::History { channel_id: CHAN, before: None }]);
        assert!(s.chans[&CHAN].loading);
        // Message introuvable pendant un premier chargement en route : la page en route vaut.
        let mut s = store();
        s.select_channel(CHAN);
        s.take_commands();
        s.jump_to_message(CHAN, 5);
        s.take_commands();
        s.apply(Event::Search(SearchEvent::Around { channel_id: CHAN, around: 5, messages: Vec::new() }));
        assert!(s.take_commands().is_empty());
        s.apply(Event::History { channel_id: CHAN, before: None, messages: page(1..=3) });
        assert_eq!(ids(&s), vec![1, 2, 3]);
        // Salon inconnu : rien.
        assert_eq!(s.jump_to_message(777, 1), Jump::Unknown);
    }

    #[test]
    fn results_keep_their_order_and_stale_pages_are_ignored() {
        let mut s = store();
        s.select_guild(G);
        let scope = s.search_scope().unwrap();
        assert_eq!(scope, Scope::Guild(G));
        let names = s.search_names(scope);
        assert_eq!(names.channels.len(), 2);
        let parsed = parse("in:c101 bonjour", &names, 0, Sort::Newest).unwrap();
        s.start_search(scope, "in:c101 bonjour".into(), parsed);
        let cmds = s.take_commands();
        let [_, Cmd::Search(SearchCmd::Query { seq, query, offset: 0, .. })] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!(query.channels, vec![OTHER]);
        s.apply(Event::Search(SearchEvent::Results { seq: seq + 7, total: 1, messages: vec![msg(1, OTHER)], threads: Vec::new() }));
        assert!(s.search.active.as_ref().is_some_and(|a| a.hits.is_empty()));
        s.apply(Event::Search(SearchEvent::Results { seq: *seq, total: 40, messages: vec![msg(9, OTHER), msg(3, OTHER), msg(5, OTHER)], threads: vec![(OTHER, "fil".into())] }));
        let a = s.search.active.as_ref().unwrap();
        assert_eq!(a.hits.iter().map(|h| h.message.id).collect::<Vec<_>>(), vec![9, 3, 5]);
        assert_eq!((a.total, a.phase.clone(), a.hits[0].thread_name.as_deref()), (Some(40), Phase::Done, Some("fil")));
        assert!(s.search_page(1));
        assert!(!s.search_page(2), "40 résultats : deux pages");
        s.close_search();
        assert!(s.search.active.is_none());
    }
}
