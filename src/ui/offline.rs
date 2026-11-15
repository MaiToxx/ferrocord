//! Historique hors ligne côté interface (lot H) : quand demander
//! l'instantané du compte qui s'ouvre, quand l'afficher, quand
//! l'enregistrer et quand l'effacer. Le disque et le chiffrement sont
//! l'affaire de `crate::offline` (son fil) ; le modèle, de `state::offline`.
//!
//! - Ouverture d'un compte enregistré : son instantané est demandé tout de
//!   suite et affiché dès qu'il arrive, s'il arrive avant READY.
//! - Enregistrement : en session, au plus toutes les [`SAVE_EVERY`] quand
//!   quelque chose a changé, et à la fermeture de la fenêtre. Seulement pour
//!   un compte de la liste (pas `DISCORD_TOKEN`, pas un jeton non retenu).
//! - Effacement : compte retiré ou déconnecté (il quitte la liste), option
//!   coupée, ou « Effacer maintenant ».

use super::App;
use crate::i18n::table::offline::{f as of, t as ot};
use crate::offline::{OfflineDisk, OfflineEvent, Quiet};
use crate::state::offline::Snapshot;
use crate::theme::Tokens;
use eframe::egui;
use std::hash::{Hash, Hasher};
use std::time::{Duration, Instant};

/// Écart minimal entre deux enregistrements.
pub const SAVE_EVERY: Duration = Duration::from_secs(30);
/// Attente au plus, à la fermeture, de l'écriture du dernier instantané.
const EXIT_WAIT: Duration = Duration::from_secs(2);

pub(in crate::ui) struct OfflineUi {
    backend: Box<dyn OfflineDisk>,
    attached: bool,
    /// Compte dont l'instantané a été demandé pour la session en cours, et quand.
    asked: Option<(u64, Instant)>,
    /// Temps entre l'ouverture de la session et l'affichage de l'instantané.
    shown_after: Option<Duration>,
    /// Dernier enregistrement (ou début de la session), et ce qu'il contenait.
    last_save: Option<Instant>,
    saved: Option<u64>,
    /// Comptes de la liste, tels que transmis au fil (les autres sont effacés).
    known: Option<Vec<u64>>,
    /// Option telle que lue dans `local.toml` à la dernière image.
    enabled: Option<bool>,
}

impl Default for OfflineUi {
    fn default() -> Self {
        OfflineUi { backend: Box::new(Quiet), attached: false, asked: None, shown_after: None, last_save: None, saved: None, known: None, enabled: None }
    }
}

/// Résumé de ce qu'un instantané contiendrait : pas d'enregistrement s'il n'a pas changé.
fn signature(store: &crate::state::Store) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    store.me.as_ref().map(|u| (u.id, &u.name)).hash(&mut h);
    (store.guild, store.channel, store.guilds.len(), store.dms.len()).hash(&mut h);
    for g in &store.guilds {
        (g.id, &g.name, g.channels.len()).hash(&mut h);
    }
    let mut chans: Vec<_> = store.chans.iter().map(|(id, cs)| (*id, cs.rev, cs.msgs.len())).collect();
    chans.sort_unstable();
    chans.hash(&mut h);
    h.finish()
}

impl App {
    /// Branche le vrai disque de l'historique hors ligne.
    pub fn attach_offline(&mut self, backend: Box<dyn OfflineDisk>) {
        self.offline = OfflineUi { backend, attached: true, ..Default::default() };
    }

    /// Temps entre l'ouverture de la session et l'affichage de l'historique
    /// hors ligne (`None` : pas affiché) : mesure du lot H.
    pub fn offline_shown_after(&self) -> Option<Duration> {
        self.offline.shown_after
    }

    /// Compte enregistré de la session en cours, s'il est connu.
    fn offline_user(&self) -> Option<u64> {
        let key = self.current_account()?;
        self.prefs.now.account(key)?.user_id
    }

    fn session_ready(&self) -> bool {
        self.current.as_ref().is_some_and(|c| c.ready)
    }

    /// À chaque image : réponses du fil, effacements, demandes, enregistrement.
    pub(in crate::ui) fn offline_tick(&mut self) {
        if !self.offline.attached {
            return;
        }
        let loaded = self.local.loaded;
        let enabled = crate::plugins::on(crate::plugins::Id::OfflineHistory);
        if loaded && self.offline.enabled != Some(enabled) {
            if !enabled {
                self.offline.backend.purge_all();
            }
            self.offline.enabled = Some(enabled);
        }
        // Comptes de la liste, hors ceux qu'on retire : les autres n'ont plus d'historique.
        let mut known: Vec<u64> = self.prefs.now.accounts.iter().filter(|a| !self.forgetting.contains(&a.key)).filter_map(|a| a.user_id).collect();
        known.sort_unstable();
        known.dedup();
        if self.offline.known.as_ref() != Some(&known) {
            self.offline.backend.keep_only(known.clone());
            self.offline.known = Some(known);
        }
        self.offline_receive();
        let user = self.offline_user();
        if self.current.is_none() {
            self.offline.asked = None;
            self.offline.last_save = None;
            self.offline.saved = None;
        }
        // Session qui s'ouvre (pas encore READY) : son instantané est demandé une fois.
        if let Some(u) = user
            && !self.session_ready()
            && self.store.me.is_none()
            && self.offline.asked.is_none_or(|(a, _)| a != u)
            && (!loaded || enabled)
        {
            self.offline.asked = Some((u, Instant::now()));
            self.offline.backend.load(u);
        }
        if loaded && enabled && self.session_ready() {
            match self.offline.last_save {
                // Début de la session : le premier instantané attend un peu d'usage.
                None => self.offline.last_save = Some(Instant::now()),
                Some(at) if at.elapsed() >= SAVE_EVERY => self.offline_save(),
                Some(_) => {}
            }
        }
    }

    fn offline_receive(&mut self) {
        for e in self.offline.backend.poll() {
            let OfflineEvent::Loaded { user, snapshot, took } = e;
            let Some(snap) = snapshot else { continue };
            // Seulement pour la session qui l'a demandé, et avant READY.
            if self.offline_user() != Some(user) || self.session_ready() || !self.offline.asked.is_some_and(|(a, _)| a == user) {
                continue;
            }
            if self.store.apply_prepared(*snap) {
                // Le salon affiché est rafraîchi tout de suite (sans attendre READY).
                if let Some(c) = self.store.channel {
                    self.store.ensure_history(c);
                }
                self.scroll_to_bottom = true;
                let after = self.offline.asked.map_or(took, |(_, at)| at.elapsed());
                self.offline.shown_after = Some(after);
                let line = of::shown(self.lang, after.as_millis());
                self.local_log(&line);
                self.ctx.request_repaint();
            }
        }
    }

    /// Enregistre l'instantané de la session s'il a changé depuis le dernier.
    fn offline_save(&mut self) {
        self.offline.last_save = Some(Instant::now());
        let Some(user) = self.offline_user() else { return };
        if !self.offline.known.as_ref().is_some_and(|k| k.contains(&user)) || self.store.me.as_ref().map(|u| u.id) != Some(user) {
            return;
        }
        let sig = signature(&self.store);
        if self.offline.saved == Some(sig) {
            return;
        }
        if let Some(snap) = Snapshot::capture(&self.store) {
            self.offline.backend.save(user, snap);
            self.offline.saved = Some(sig);
        }
    }

    /// Fermeture de la fenêtre : dernier instantané écrit avant de partir.
    pub(in crate::ui) fn offline_exit(&mut self) {
        if !self.offline.attached {
            return;
        }
        if self.local.loaded && crate::plugins::on(crate::plugins::Id::OfflineHistory) && self.session_ready() {
            self.offline_save();
        }
        self.offline.backend.flush(EXIT_WAIT);
    }

    /// Section de la page « Données et confidentialité ».
    pub(in crate::ui) fn offline_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        super::settings::section(ui, t, ot::SECTION.get(lang));
        if !crate::offline::AVAILABLE {
            super::settings::note(ui, t, ot::UNAVAILABLE.get(lang));
            self.journal_settings(ui, t);
            return;
        }
        // Le même interrupteur que la carte du plugin (Paramètres > Plugins).
        let on = self.local.prefs.plugins.on(crate::plugins::Id::OfflineHistory);
        if let Some(v) = super::settings::toggle(ui, ot::KEEP.get(lang), ot::KEEP_TIP.get(lang), Some(on), true) {
            self.set_plugin(crate::plugins::Id::OfflineHistory, v);
        }
        let mut clear = false;
        super::appearance::row(ui, ot::CLEAR.get(lang), ot::CLEAR_TIP.get(lang), |ui| {
            clear = ui.button(ot::CLEAR.get(lang)).clicked();
        });
        if clear {
            self.offline.backend.purge_all();
            // Rien n'est réécrit avant un nouveau changement.
            self.offline.saved = Some(signature(&self.store));
            self.offline.last_save = Some(Instant::now());
            // L'index de la recherche locale (plugin, lot E1) part avec l'historique.
            self.local_search_wipe(true);
            self.notify(ot::CLEARED);
        }
        // Journal des messages supprimés et modifiés (plugin, lot E1).
        self.journal_settings(ui, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::{AccountKey, MemoryVault, Token};
    use crate::settings::{Account, Settings, SettingsLink, SettingsUpdate};
    use crate::state::model::{Channel, ChannelKind, Guild, Message, User};
    use crate::state::{Cmd, Event, Store};
    use std::sync::{Arc, Mutex};

    const ME: u64 = 1_000_000_000_000_000_001;
    const G: u64 = 2_000_000_000_000_000_001;
    const CHAN: u64 = 3_000_000_000_000_000_001;

    #[derive(Default)]
    struct Log {
        loads: Vec<u64>,
        saves: Vec<(u64, Snapshot)>,
        purged: Vec<u64>,
        kept: Vec<Vec<u64>>,
        purged_all: usize,
        flushed: usize,
        pending: Vec<OfflineEvent>,
    }

    #[derive(Clone, Default)]
    struct Disk(Arc<Mutex<Log>>);

    impl OfflineDisk for Disk {
        fn load(&mut self, user: u64) {
            self.0.lock().unwrap().loads.push(user);
        }
        fn save(&mut self, user: u64, s: Snapshot) {
            self.0.lock().unwrap().saves.push((user, s));
        }
        fn purge(&mut self, user: u64) {
            self.0.lock().unwrap().purged.push(user);
        }
        fn keep_only(&mut self, users: Vec<u64>) {
            self.0.lock().unwrap().kept.push(users);
        }
        fn purge_all(&mut self) {
            self.0.lock().unwrap().purged_all += 1;
        }
        fn flush(&mut self, _: Duration) {
            self.0.lock().unwrap().flushed += 1;
        }
        fn poll(&mut self) -> Vec<OfflineEvent> {
            std::mem::take(&mut self.0.lock().unwrap().pending)
        }
    }

    /// Réseau factice : connecté, READY seulement quand le test le pousse.
    #[derive(Clone, Default)]
    struct Net {
        events: Arc<Mutex<Vec<Event>>>,
        sent: Arc<Mutex<Vec<Cmd>>>,
    }

    impl crate::ui::Backend for Net {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            self.sent.lock().unwrap().extend(cmds);
        }
    }

    fn guild() -> Guild {
        Guild::new(G, "Alpha".into(), vec![Channel { id: CHAN, name: "général".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 }])
    }

    fn msg(id: u64, text: &str) -> Message {
        Message {
            id,
            channel_id: CHAN,
            guild_id: Some(G),
            author_id: 7,
            author: "Alice".into(),
            author_avatar: crate::state::model::Avatar::default_for(7),
            content: text.into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    fn cached() -> Snapshot {
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild()], dms: Vec::new() });
        s.select_guild(G);
        s.apply(Event::History { channel_id: CHAN, before: None, messages: vec![msg(10, "gardé sur le disque")] });
        Snapshot::capture(&s).unwrap()
    }

    /// Application dont le compte enregistré s'ouvre au démarrage (coffre en mémoire).
    fn app(net: Net, disk: Disk) -> App {
        let ctx = egui::Context::default();
        let key = AccountKey::of_user(ME);
        let settings = Settings { last_account: Some(key.clone()), accounts: vec![Account { user_id: Some(ME), name: Some("Moi".into()), ..Account::new(key.clone()) }], ..Settings::default() };
        let (link, _) = SettingsLink::open(None, false, crate::theme::WatchConfig::default());
        let boot = crate::ui::Boot {
            backend: Box::new(net),
            store: Store::new(),
            theme: crate::theme::ThemeLink::open(None, false, crate::theme::WatchConfig::default()),
            settings: (link, Some(SettingsUpdate::Loaded(settings))),
            accounts: crate::account::AccountLink::start(Arc::new(MemoryVault::with([(key, Token::from("faux.jeton.historique"))]))),
            env_token: None,
            legacy: None,
            images: crate::images::Images::disabled(ctx.clone()),
        };
        let mut app = App::with_context(ctx, boot);
        app.attach_offline(Box::new(disk));
        // `local.toml` lu (option active par défaut).
        app.local.loaded = true;
        app
    }

    /// Images jusqu'à ce que la session soit ouverte (coffre lu sur son fil).
    fn until_session(app: &mut App) {
        for _ in 0..500 {
            app.logic_pass();
            if app.current.is_some() {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("session jamais ouverte");
    }

    #[test]
    fn cache_shows_before_ready_and_ready_replaces_it() {
        let (net, disk) = (Net::default(), Disk::default());
        let mut app = app(net.clone(), disk.clone());
        until_session(&mut app);
        app.logic_pass();
        assert_eq!(disk.0.lock().unwrap().loads, vec![ME], "instantané demandé une fois, à l'ouverture");
        disk.0.lock().unwrap().pending.push(OfflineEvent::Loaded { user: ME, snapshot: Some(Box::new(cached().prepare())), took: Duration::from_millis(3) });
        app.logic_pass();
        assert!(app.offline_shown_after().is_some());
        assert_eq!(app.store.channel, Some(CHAN));
        assert_eq!(app.store.chans[&CHAN].msgs[0].content, "gardé sur le disque");
        app.flush_commands();
        assert!(net.sent.lock().unwrap().contains(&Cmd::History { channel_id: CHAN, before: None }), "page fraîche demandée sans attendre READY");
        // L'interface principale s'affiche (pas l'écran de connexion).
        let out = crate::ui::tests::frame(&mut app, Vec::new());
        assert!(crate::ui::tests::find(&out, "gardé sur le disque").is_some());

        // READY puis la page fraîche : le cache est remplacé.
        net.events.lock().unwrap().push(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild()], dms: Vec::new() });
        app.logic_pass();
        assert!(!app.store.chans[&CHAN].msgs.is_empty(), "rien ne disparaît à READY");
        net.events.lock().unwrap().push(Event::History { channel_id: CHAN, before: None, messages: vec![msg(11, "frais")] });
        app.logic_pass();
        assert_eq!(app.store.chans[&CHAN].msgs.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), vec!["frais"]);

        // Fermeture : instantané écrit, puis attente du fil.
        app.offline_exit();
        let log = disk.0.lock().unwrap();
        assert_eq!(log.saves.len(), 1);
        assert_eq!(log.saves[0].0, ME);
        assert_eq!(log.saves[0].1.chans[0].msgs[0].content, "frais");
        assert_eq!(log.flushed, 1);
        // Aucun jeton dans ce qui part sur le disque.
        let text = log.saves[0].1.clone().encode();
        assert!(!text.windows(b"faux.jeton".len()).any(|w| w == b"faux.jeton"));
    }

    #[test]
    fn late_cache_after_ready_is_ignored() {
        let (net, disk) = (Net::default(), Disk::default());
        let mut app = app(net.clone(), disk.clone());
        until_session(&mut app);
        net.events.lock().unwrap().push(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild()], dms: Vec::new() });
        app.logic_pass();
        disk.0.lock().unwrap().pending.push(OfflineEvent::Loaded { user: ME, snapshot: Some(Box::new(cached().prepare())), took: Duration::ZERO });
        app.logic_pass();
        assert!(app.offline_shown_after().is_none());
        assert!(app.store.chans.get(&CHAN).is_none_or(|cs| !cs.offline));
    }

    #[test]
    fn logout_option_and_clear_erase_the_history() {
        let (net, disk) = (Net::default(), Disk::default());
        let mut app = app(net.clone(), disk.clone());
        until_session(&mut app);
        assert_eq!(disk.0.lock().unwrap().kept.first(), Some(&vec![ME]), "fichiers des comptes inconnus effacés");
        // Déconnexion : le compte part de la liste des comptes gardés.
        app.forgetting.push(AccountKey::of_user(ME));
        app.logic_pass();
        assert_eq!(disk.0.lock().unwrap().kept.last(), Some(&Vec::new()));
        // Option coupée : tout est effacé, une fois.
        app.set_plugin(crate::plugins::Id::OfflineHistory, false);
        app.logic_pass();
        app.logic_pass();
        assert_eq!(disk.0.lock().unwrap().purged_all, 1);
        // Section de « Données et confidentialité » : dessinée, avec son option.
        app.show_settings(super::super::settings::Page::DataPrivacy);
        app.ctx.enable_accesskit();
        let mut out = None;
        // Écran très haut : toute la page tient sans défilement.
        for _ in 0..3 {
            app.logic_pass();
            let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 3000.0))), ..Default::default() };
            let mut o = app.ctx.clone().run_ui(raw, |ui| app.ui_pass(ui));
            o.textures_delta.clear();
            out = Some(o);
        }
        let out = out.unwrap();
        let label = ot::CLEAR.get(app.lang);
        assert!(crate::ui::tests::find(&out, label).is_some(), "« {label} » proposé, même sans session");
        // « Effacer maintenant » : tout est effacé.
        crate::ui::tests::click(&mut app, label);
        assert_eq!(disk.0.lock().unwrap().purged_all, 2);
    }

    #[test]
    fn no_save_without_a_listed_account_or_with_option_off() {
        let (net, disk) = (Net::default(), Disk::default());
        let mut app = app(net.clone(), disk.clone());
        until_session(&mut app);
        net.events.lock().unwrap().push(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild()], dms: Vec::new() });
        app.logic_pass();
        app.set_plugin(crate::plugins::Id::OfflineHistory, false);
        app.logic_pass();
        app.offline_exit();
        assert!(disk.0.lock().unwrap().saves.is_empty(), "option coupée : rien d'écrit");
    }
}
