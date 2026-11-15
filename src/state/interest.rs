//! Ce que l'interface montre, lu par la passerelle pour décider si un
//! événement mérite un réveil (règle 1.3 du plan : aucune image quand rien ne
//! change à l'écran). Même principe que [`super::members::ListWatch`] pour la
//! liste des membres, et que le magasin des présences (`people`).
//!
//! Trois issues pour un événement :
//! - réveil : il peut changer l'écran (tout ce qui n'est pas listé ici) ;
//! - attente : il part dans la file sans réveil et sera lu au début de la
//!   prochaine image, quelle qu'en soit la cause (souris, autre événement) ;
//!   après [`DEFER_FLUSH`] événements en attente, un réveil vide la file ;
//! - abandon : sans effet visible maintenant, et faux s'il était lu plus tard
//!   (« … est en train d'écrire » d'un autre salon, daté de sa lecture).
//!
//! Attendent : modifications, médias et réactions d'un message d'un autre
//! salon que celui ouvert (le salon ouvert les lit au moment où il est
//! ouvert, avant d'être dessiné) ; nouveau message ordinaire (ni mention, ni
//! MP, ni message système, pas de moi) d'un salon « calme » : déjà non lu ou
//! muet, dont le niveau de notification ne vise que les mentions (ni pastille
//! ni notification ne changent). L'interface tient la liste des salons calmes
//! à jour ([`Interest::show`]).
//!
//! Lot C9 : dans le salon ouvert aussi, modifications, médias et réactions
//! d'un message hors de la partie affichée attendent (embeds qui arrivent
//! sur un vieux message, robot qui réécrit un message plus haut). La liste
//! publie à chaque image le premier et le dernier message affichés
//! ([`Interest::show_messages`]) ; sans cette information, tout réveille.
//!
//! Lot « perf » (mesure du 15 novembre 2026 : 114 réveils en 60 s de repos,
//! dont 96 pour des MESSAGE_UPDATE) : un MESSAGE_UPDATE complet donne cinq
//! événements, et seuls le texte et les médias attendaient. Attendent aussi,
//! pour un message hors de l'écran : épinglage, drapeaux d'annonce,
//! composants d'application, sondage (votes, résultats). Une suppression
//! attend dans un salon qui n'est pas à l'écran (dans le salon ouvert elle
//! réveille toujours : la liste se resserre). Un CHANNEL_UPDATE attend quand
//! son serveur n'est pas à l'écran ([`Interest::show_guilds`]) et que ni le
//! type, ni la catégorie, ni les permissions du salon ne changent
//! ([`Interest::calm_channel_update`]) : rien de ce qui est affiché
//! ailleurs (pastilles du rail, compteurs) n'en dépend.
//!
//! Aucun événement mis en attente ne change un compteur de non-lus ou de
//! mentions ni ne déclenche de notification : ceux-là réveillent toujours
//! (test `deferred_events_change_no_counter`).

use super::{announce::AnnounceEvent, interactions::InteractionEvent, polls::PollEvent, social::SocialEvent, Event};
use std::{
    collections::{HashMap, HashSet},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Mutex, RwLock,
    },
};

/// Événements en attente au-delà desquels l'interface est réveillée quand
/// même (la file reste bornée si rien d'autre ne la réveille pendant des heures).
pub const DEFER_FLUSH: u32 = 512;

/// Salons visibles en plus du salon ouvert, au plus (une vue scindée et les fenêtres détachées).
pub const ALSO_MAX: usize = 8;

/// Serveurs à l'écran, au plus : celui de la colonne des salons, celui de ma
/// connexion vocale, ceux des salons secondaires.
pub const GUILDS_MAX: usize = ALSO_MAX + 2;

/// Salons dont l'empreinte est gardée ([`Interest::calm_channel_update`]) ;
/// au-delà, tout est oublié (le prochain CHANNEL_UPDATE de chacun réveille).
const MARKS_MAX: usize = 4096;

/// Ce que la passerelle fait d'un événement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Wake,
    Defer,
    Drop,
}

#[derive(Debug, Default)]
pub struct Interest {
    /// Faux tant que l'interface ne publie rien : tout réveille (tests, outils).
    filter: AtomicBool,
    /// Salon ouvert (0 : aucun).
    open: AtomicU64,
    /// Mon identifiant (0 : inconnu).
    me: AtomicU64,
    /// Salons où un message ordinaire ne change rien à l'écran.
    quiet: RwLock<HashSet<u64>>,
    /// Événements mis en attente depuis la dernière image.
    waiting: AtomicU32,
    /// Premier et dernier message affichés du salon ouvert (`shown_hi` à 0 : inconnu).
    shown_lo: AtomicU64,
    shown_hi: AtomicU64,
    /// Autres salons visibles (vue scindée, fenêtres détachées, lot E2) ; 0 : case libre.
    also: [AtomicU64; ALSO_MAX],
    /// Serveurs à l'écran (colonne des salons, vocal, salons secondaires) ; 0 : case libre.
    guilds: [AtomicU64; GUILDS_MAX],
    /// Empreinte (type, catégorie, permissions) du dernier CHANNEL_UPDATE de chaque salon.
    marks: Mutex<HashMap<u64, u64>>,
}

impl Interest {
    /// À appeler à chaque image, après la lecture des événements : salon ouvert, moi.
    pub fn show(&self, open: Option<u64>, me: Option<u64>) {
        self.open.store(open.unwrap_or(0), Ordering::Relaxed);
        self.me.store(me.unwrap_or(0), Ordering::Relaxed);
        self.waiting.store(0, Ordering::Relaxed);
        self.filter.store(true, Ordering::Relaxed);
    }

    /// Messages affichés du salon ouvert à cette image (premier, dernier) ;
    /// `None` : inconnu (liste pas dessinée, salon vide) et tout réveille.
    pub fn show_messages(&self, range: Option<(u64, u64)>) {
        let (lo, hi) = range.map_or((0, 0), |(a, b)| (a.min(b), a.max(b)));
        self.shown_lo.store(lo, Ordering::Relaxed);
        self.shown_hi.store(hi, Ordering::Relaxed);
    }

    /// Autres salons affichés à cette image, en plus du salon ouvert : leurs
    /// événements réveillent comme les siens. Au plus [`ALSO_MAX`] ; un onglet
    /// en arrière-plan n'en fait pas partie.
    pub fn show_also(&self, channels: &[u64]) {
        for (i, slot) in self.also.iter().enumerate() {
            slot.store(channels.get(i).copied().unwrap_or(0), Ordering::Relaxed);
        }
    }

    /// Serveurs à l'écran à cette image : leurs CHANNEL_UPDATE réveillent.
    /// Au plus [`GUILDS_MAX`] ; les doublons et les zéros sont sans effet.
    pub fn show_guilds(&self, guilds: &[u64]) {
        for (i, slot) in self.guilds.iter().enumerate() {
            let g = guilds.get(i).copied().unwrap_or(0);
            // Écrit seulement si la case change (appelé à chaque image).
            if slot.load(Ordering::Relaxed) != g {
                slot.store(g, Ordering::Relaxed);
            }
        }
    }

    fn guild_shown(&self, guild: u64) -> bool {
        self.guilds.iter().any(|g| g.load(Ordering::Relaxed) == guild)
    }

    /// CHANNEL_UPDATE du salon `channel` du serveur `guild` : tout le paquet
    /// peut-il attendre la prochaine image ? Oui si le serveur et le salon
    /// ne sont pas à l'écran et si `mark` (empreinte du type, de la catégorie
    /// et des permissions du salon) est celle du paquet précédent : ce qui
    /// change alors (nom, sujet, position, statut, dernier message) ne se
    /// voit que dans ce serveur. Premier paquet d'un salon, ou empreinte
    /// différente : réveil (un salon qui devient visible ou caché change les
    /// pastilles du rail et les compteurs). Un paquet qui attend compte pour
    /// [`DEFER_FLUSH`] comme un événement.
    pub fn calm_channel_update(&self, guild: u64, channel: u64, mark: u64) -> bool {
        let same = self.marks.lock().is_ok_and(|mut m| {
            if m.len() >= MARKS_MAX && !m.contains_key(&channel) {
                m.clear();
            }
            m.insert(channel, mark) == Some(mark)
        });
        same && self.filter.load(Ordering::Relaxed) && !self.guild_shown(guild) && self.elsewhere(channel) && !self.flush_due()
    }

    /// Compte un événement mis en attente ; vrai quand un réveil doit vider la file.
    fn flush_due(&self) -> bool {
        if self.waiting.fetch_add(1, Ordering::Relaxed) + 1 >= DEFER_FLUSH {
            self.waiting.store(0, Ordering::Relaxed);
            return true;
        }
        false
    }

    /// Événement d'un paquet qui attend en entier ([`Interest::calm_channel_update`]) :
    /// vrai s'il faut quand même réveiller (file pleine).
    pub fn deferred(&self) -> bool {
        self.flush_due()
    }

    /// Message `id` du salon ouvert hors de la partie affichée (connue).
    fn offscreen(&self, id: u64) -> bool {
        let hi = self.shown_hi.load(Ordering::Relaxed);
        hi != 0 && (id < self.shown_lo.load(Ordering::Relaxed) || id > hi)
    }

    /// Ailleurs que dans le salon ouvert, ou dans le salon ouvert mais hors de l'écran.
    fn unseen(&self, channel: u64, id: u64) -> bool {
        self.elsewhere(channel) || (channel == self.open.load(Ordering::Relaxed) && self.offscreen(id))
    }

    /// Nouvelle liste des salons calmes (seulement quand elle change).
    pub fn set_quiet(&self, quiet: HashSet<u64>) {
        if let Ok(mut q) = self.quiet.write() {
            *q = quiet;
        }
    }

    /// Plus de filtre (déconnexion, tests) : tout réveille.
    pub fn reset(&self) {
        self.filter.store(false, Ordering::Relaxed);
        if let Ok(mut q) = self.quiet.write() {
            q.clear();
        }
        if let Ok(mut m) = self.marks.lock() {
            m.clear();
        }
    }

    fn elsewhere(&self, channel: u64) -> bool {
        channel != self.open.load(Ordering::Relaxed) && !self.also.iter().any(|c| c.load(Ordering::Relaxed) == channel)
    }

    /// Que faire de `e` ? Un événement mis en attente compte pour [`DEFER_FLUSH`].
    pub fn verdict(&self, e: &Event) -> Verdict {
        if !self.filter.load(Ordering::Relaxed) {
            return Verdict::Wake;
        }
        let v = match e {
            Event::TypingStart { channel_id, .. } if self.elsewhere(*channel_id) => Verdict::Drop,
            Event::MessageUpdate { channel_id, id, .. } | Event::MessageMedia { channel_id, id, .. } if self.unseen(*channel_id, *id) => Verdict::Defer,
            Event::Social(
                SocialEvent::Reaction { channel_id, message_id, .. }
                | SocialEvent::ReactionRemoveAll { channel_id, message_id }
                | SocialEvent::ReactionRemoveEmoji { channel_id, message_id, .. },
            ) if self.unseen(*channel_id, *message_id) => Verdict::Defer,
            // Réaction au message de quelqu'un d'autre : aucune notification à en tirer.
            Event::Social(SocialEvent::ReactedTo { author_id, user_id, .. }) if *author_id != self.me.load(Ordering::Relaxed) || author_id == user_id => Verdict::Drop,
            Event::MessageCreate(m) if self.plain_in_quiet(m) => Verdict::Defer,
            // Reste d'un MESSAGE_UPDATE (lot « perf ») : épinglage, drapeaux,
            // composants, sondage d'un message qui n'est pas à l'écran.
            Event::Social(SocialEvent::Pinned { channel_id, message_id, .. }) if self.unseen(*channel_id, *message_id) => Verdict::Defer,
            Event::Announce(AnnounceEvent::Flags { channel_id, message_id, .. }) if self.unseen(*channel_id, *message_id) => Verdict::Defer,
            Event::Interactions(InteractionEvent::MessageApp { channel_id, id, .. }) if self.unseen(*channel_id, *id) => Verdict::Defer,
            Event::Polls(PollEvent::Vote { channel_id, message_id, .. } | PollEvent::Replace { channel_id, message_id, .. }) if self.unseen(*channel_id, *message_id) => Verdict::Defer,
            // Suppression dans un salon qui n'est pas à l'écran : ni pastille ni compteur n'en dépendent.
            Event::MessageDelete { channel_id, .. } if self.elsewhere(*channel_id) => Verdict::Defer,
            _ => Verdict::Wake,
        };
        if v == Verdict::Defer && self.flush_due() {
            return Verdict::Wake;
        }
        v
    }

    /// Message ordinaire d'un autre que moi, dans un salon calme d'un serveur.
    fn plain_in_quiet(&self, m: &super::model::Message) -> bool {
        let me = self.me.load(Ordering::Relaxed);
        me != 0
            && m.author_id != me
            && m.guild_id.is_some()
            && self.elsewhere(m.channel_id)
            && m.system.is_none()
            && !m.mention_everyone
            && m.mentions.is_empty()
            && m.meta.mention_roles.is_empty()
            && m.meta.thread.is_none()
            && self.quiet.read().is_ok_and(|q| q.contains(&m.channel_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Avatar, Message};

    fn msg(channel_id: u64, author_id: u64) -> Message {
        Message {
            id: 1,
            channel_id,
            guild_id: Some(7),
            author_id,
            author: "a".into(),
            author_avatar: Avatar::of_user(author_id, None, None),
            content: "salut".into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    #[test]
    fn everything_wakes_until_the_ui_publishes() {
        let i = Interest::default();
        assert_eq!(i.verdict(&Event::TypingStart { channel_id: 5, user_id: 2, name: None }), Verdict::Wake);
        assert_eq!(i.verdict(&Event::MessageCreate(msg(5, 2))), Verdict::Wake);
    }

    #[test]
    fn only_invisible_events_wait() {
        let i = Interest::default();
        i.show(Some(10), Some(1));
        i.set_quiet([20].into());
        // Salon ouvert : tout réveille.
        assert_eq!(i.verdict(&Event::TypingStart { channel_id: 10, user_id: 2, name: None }), Verdict::Wake);
        assert_eq!(i.verdict(&Event::MessageUpdate { channel_id: 10, id: 1, content: "x".into() }), Verdict::Wake);
        assert_eq!(i.verdict(&Event::MessageCreate(msg(10, 2))), Verdict::Wake);
        // Ailleurs.
        assert_eq!(i.verdict(&Event::TypingStart { channel_id: 30, user_id: 2, name: None }), Verdict::Drop);
        assert_eq!(i.verdict(&Event::MessageUpdate { channel_id: 30, id: 1, content: "x".into() }), Verdict::Defer);
        assert_eq!(i.verdict(&Event::MessageCreate(msg(20, 2))), Verdict::Defer, "salon calme");
        assert_eq!(i.verdict(&Event::MessageCreate(msg(30, 2))), Verdict::Wake, "pas encore non lu");
        assert_eq!(i.verdict(&Event::MessageCreate(msg(20, 1))), Verdict::Wake, "mon message");
        let pinged = Message { mentions: vec![(1, "moi".into())], ..msg(20, 2) };
        assert_eq!(i.verdict(&Event::MessageCreate(pinged)), Verdict::Wake);
        let everyone = Message { mention_everyone: true, ..msg(20, 2) };
        assert_eq!(i.verdict(&Event::MessageCreate(everyone)), Verdict::Wake);
        let dm = Message { guild_id: None, ..msg(20, 2) };
        assert_eq!(i.verdict(&Event::MessageCreate(dm)), Verdict::Wake);
        assert_eq!(i.verdict(&Event::MessageDelete { channel_id: 30, id: 1 }), Verdict::Defer, "suppression ailleurs");
        assert_eq!(i.verdict(&Event::MessageDelete { channel_id: 10, id: 1 }), Verdict::Wake, "suppression dans le salon ouvert");
        i.reset();
        assert_eq!(i.verdict(&Event::MessageUpdate { channel_id: 30, id: 1, content: "x".into() }), Verdict::Wake);
    }

    /// Lot C9 : un MESSAGE_UPDATE du salon ouvert hors de l'écran ne demande pas d'image.
    #[test]
    fn open_channel_updates_wake_only_when_shown() {
        let i = Interest::default();
        i.show(Some(10), Some(1));
        let update = |id| Event::MessageUpdate { channel_id: 10, id, content: "x".into() };
        let media = |id| Event::MessageMedia { channel_id: 10, id, attachments: None, embeds: Some(Vec::new()) };
        let react = |message_id| Event::Social(SocialEvent::ReactionRemoveAll { channel_id: 10, message_id });
        // Partie affichée inconnue : tout réveille.
        assert_eq!(i.verdict(&update(5)), Verdict::Wake);
        i.show_messages(Some((100, 200)));
        for (e, v) in [(update(150), Verdict::Wake), (update(100), Verdict::Wake), (update(200), Verdict::Wake), (update(99), Verdict::Defer), (update(201), Verdict::Defer)] {
            assert_eq!(i.verdict(&e), v, "{e:?}");
        }
        assert_eq!(i.verdict(&media(150)), Verdict::Wake);
        assert_eq!(i.verdict(&media(50)), Verdict::Defer);
        assert_eq!(i.verdict(&react(150)), Verdict::Wake);
        assert_eq!(i.verdict(&react(250)), Verdict::Defer);
        // Nouveau message et suppression du salon ouvert : toujours un réveil.
        assert_eq!(i.verdict(&Event::MessageCreate(msg(10, 2))), Verdict::Wake);
        assert_eq!(i.verdict(&Event::MessageDelete { channel_id: 10, id: 50 }), Verdict::Wake);
        // Plus rien d'affiché : de nouveau inconnu.
        i.show_messages(None);
        assert_eq!(i.verdict(&update(50)), Verdict::Wake);
    }

    /// Les cinq événements d'un MESSAGE_UPDATE complet (texte, médias,
    /// épinglage, drapeaux, composants), et un vote de sondage.
    fn update_events(channel_id: u64, id: u64) -> Vec<Event> {
        vec![
            Event::MessageUpdate { channel_id, id, content: "x".into() },
            Event::MessageMedia { channel_id, id, attachments: None, embeds: Some(Vec::new()) },
            Event::Social(SocialEvent::Pinned { channel_id, message_id: id, pinned: false }),
            Event::Announce(AnnounceEvent::Flags { channel_id, message_id: id, crossposted: false }),
            Event::Interactions(InteractionEvent::MessageApp { channel_id, id, app: None, edited: true }),
            Event::Polls(PollEvent::Vote { channel_id, message_id: id, answer_id: 1, user_id: 2, add: true }),
        ]
    }

    /// Lot « perf » : aucun des événements d'un MESSAGE_UPDATE ne réveille
    /// hors de l'écran ; tous réveillent pour un message affiché.
    #[test]
    fn a_whole_message_update_waits_off_screen() {
        let i = Interest::default();
        i.show(Some(10), Some(1));
        i.show_messages(Some((100, 200)));
        for e in update_events(30, 5) {
            assert_eq!(i.verdict(&e), Verdict::Defer, "autre salon : {e:?}");
        }
        for e in update_events(10, 50) {
            assert_eq!(i.verdict(&e), Verdict::Defer, "salon ouvert, hors de l'écran : {e:?}");
        }
        for e in update_events(10, 150) {
            assert_eq!(i.verdict(&e), Verdict::Wake, "message affiché : {e:?}");
        }
        // Salon secondaire à l'écran (vue scindée) : comme le salon ouvert.
        i.show_also(&[30]);
        for e in update_events(30, 5).into_iter().chain([Event::MessageDelete { channel_id: 30, id: 5 }]) {
            assert_eq!(i.verdict(&e), Verdict::Wake, "salon secondaire : {e:?}");
        }
        // Partie affichée inconnue : tout réveille dans le salon ouvert.
        i.show_messages(None);
        for e in update_events(10, 50) {
            assert_eq!(i.verdict(&e), Verdict::Wake, "{e:?}");
        }
        // Les réponses aux gestes de l'utilisateur réveillent toujours.
        let failed = Event::Social(SocialEvent::PinFailed { channel_id: 30, message_id: 5, error: crate::state::Problem::Forbidden });
        assert_eq!(i.verdict(&failed), Verdict::Wake);
    }

    /// Lot « perf » : un CHANNEL_UPDATE attend si son serveur n'est pas à
    /// l'écran et si la visibilité du salon (empreinte) ne change pas.
    #[test]
    fn channel_updates_wait_outside_the_shown_guilds() {
        let i = Interest::default();
        // Tant que l'interface ne publie rien : réveil.
        assert!(!i.calm_channel_update(7, 30, 1));
        assert!(!i.calm_channel_update(7, 30, 1));
        i.show(Some(10), Some(1));
        i.show_guilds(&[7, 0, 8]);
        // Premier paquet d'un salon : réveil ; même empreinte ensuite : attente.
        assert!(!i.calm_channel_update(9, 40, 1), "premier paquet");
        assert!(i.calm_channel_update(9, 40, 1));
        assert!(i.calm_channel_update(9, 40, 1));
        // Permissions, type ou catégorie changés : réveil, une fois.
        assert!(!i.calm_channel_update(9, 40, 2), "empreinte changée");
        assert!(i.calm_channel_update(9, 40, 2));
        // Serveur à l'écran (colonne des salons, vocal) : toujours un réveil.
        assert!(!i.calm_channel_update(7, 30, 1) && !i.calm_channel_update(8, 31, 1));
        assert!(!i.calm_channel_update(8, 31, 1), "serveur de ma connexion vocale");
        // Salon ouvert d'un serveur qui n'est pas celui de la colonne (fil, lien) : réveil.
        assert!(!i.calm_channel_update(9, 10, 1) && !i.calm_channel_update(9, 10, 1));
        // Le serveur n'est plus à l'écran : ses paquets attendent.
        i.show_guilds(&[9]);
        assert!(i.calm_channel_update(7, 30, 1));
        assert!(!i.calm_channel_update(9, 40, 2));
        // Déconnexion : tout est oublié.
        i.reset();
        i.show(Some(10), Some(1));
        assert!(!i.calm_channel_update(7, 30, 1), "premier paquet après la reconnexion");
    }

    /// Un paquet qui attend compte dans la file : elle reste bornée.
    #[test]
    fn calm_packets_count_toward_the_flush() {
        let i = Interest::default();
        i.show(Some(10), Some(1));
        assert!(!i.calm_channel_update(9, 40, 1));
        let woken = (0..DEFER_FLUSH * 2).filter(|_| !i.calm_channel_update(9, 40, 1)).count();
        assert_eq!(woken, 2);
        let woken = (0..DEFER_FLUSH * 2).filter(|_| i.deferred()).count();
        assert_eq!(woken, 2);
    }

    /// Contrainte du lot « perf » : non-lus, mentions et notifications ne
    /// dépendent d'aucun événement mis en attente. Tout événement que le
    /// filtre fait attendre laisse les compteurs du magasin tels quels et ne
    /// produit aucun effet (notification, son) ; ceux qui les changent réveillent.
    #[test]
    fn deferred_events_change_no_counter() {
        use crate::state::model::{Channel, ChannelKind, Guild, User};
        use crate::state::Store;
        const ME: u64 = 1;
        let ch = |id| Channel { id, name: format!("c{id}"), kind: ChannelKind::Text, position: 0, parent: None, overwrites: None, slowmode: 0 };
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![Guild::new(7, "Alpha".into(), vec![ch(10), ch(20), ch(30)])], dms: Vec::new() });
        s.select_guild(7);
        s.select_channel(10);
        let i = Interest::default();
        i.show(Some(10), Some(ME));
        i.show_guilds(&[7]);
        // Salon 30 : chargé, puis un message avec mention (non lu, une mention) ; salon 20 : un message ordinaire.
        let at = |id, channel_id| Message { id, ..msg(channel_id, 2) };
        s.apply(Event::History { channel_id: 30, before: None, messages: vec![at(5, 30), at(6, 30)] });
        let pinged = Message { mentions: vec![(ME, "moi".into())], ..at(7, 30) };
        for e in [Event::MessageCreate(pinged), Event::MessageCreate(at(8, 20))] {
            assert_eq!(i.verdict(&e), Verdict::Wake, "pastille ou mention qui apparaît : {e:?}");
            s.apply(e);
        }
        let counters = |s: &Store| {
            let mut unread: Vec<u64> = s.unread().iter().copied().collect();
            unread.sort_unstable();
            (unread, s.mention_count(20), s.mention_count(30), s.guild_mentions(7))
        };
        let before = counters(&s);
        assert_eq!(before, (vec![20, 30], 0, 1, 1));
        let mut deferred = 0;
        let events = [5, 6, 7].into_iter().flat_map(|id| update_events(30, id)).chain([Event::MessageDelete { channel_id: 30, id: 7 }, Event::MessageDelete { channel_id: 30, id: 5 }, Event::MessageDelete { channel_id: 20, id: 8 }]);
        for e in events {
            assert_eq!(i.verdict(&e), Verdict::Defer, "{e:?}");
            deferred += 1;
            assert!(s.apply(e).is_none(), "aucun effet (notification, son) pour un événement en attente");
            assert_eq!(counters(&s), before, "compteurs inchangés");
        }
        assert_eq!(deferred, 21);
    }

    /// La file d'attente est bornée : au plus `DEFER_FLUSH` événements sans réveil.
    #[test]
    fn waiting_queue_is_flushed() {
        let i = Interest::default();
        i.show(Some(10), Some(1));
        let e = Event::MessageUpdate { channel_id: 30, id: 1, content: "x".into() };
        let woken = (0..DEFER_FLUSH * 3).filter(|_| i.verdict(&e) == Verdict::Wake).count();
        assert_eq!(woken, 3);
        // Une image lit la file : le compte repart de zéro.
        i.show(Some(10), Some(1));
        assert!((0..DEFER_FLUSH - 1).all(|_| i.verdict(&e) == Verdict::Defer));
    }
}
