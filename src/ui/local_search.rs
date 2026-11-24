//! Recherche locale instantanée (plugin, lot E1) : l'index
//! (`state::local_index`) suit ce que le magasin a en mémoire, le champ de
//! recherche gagne un bouton « Local » et le panneau des résultats deux
//! onglets (« Discord », « Sur cet ordinateur »). En mode local, les
//! résultats arrivent pendant la frappe et rien n'est demandé à Discord.
//!
//! L'index n'est enregistré (chiffré, `offline::vault`, dossier
//! `recherche`) que si l'historique hors ligne est activé ; sinon il reste
//! en mémoire. Plugin coupé : une comparaison par passage, rien n'est
//! indexé, gardé ni écrit, et le fichier est effacé.

use super::App;
use crate::i18n::table::journal::{f as jf, t as jt};
use crate::i18n::Lang;
use crate::offline::vault::{Kind, Payload};
use crate::plugins::{self, Id as Plugin};
use crate::state::local_index::{self, LocalIndex};
use crate::state::search::{self as s, Hit, Sort};
use crate::state::store::Rev;
use crate::state::Event;
use crate::theme::Tokens;
use eframe::egui;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// L'index est enregistré au plus toutes les minutes, et à la fermeture.
const SAVE_EVERY: Duration = Duration::from_secs(60);

#[derive(Default)]
pub(in crate::ui) struct LocalSearchUi {
    index: LocalIndex,
    /// Version de chaque salon du magasin déjà rangée dans l'index.
    revs: HashMap<u64, Rev>,
    on: bool,
    /// L'index va sur le disque (historique hors ligne activé, disque branché).
    disk: bool,
    user: Option<u64>,
    asked: Option<u64>,
    known: Option<Vec<u64>>,
    last_save: Option<Instant>,
}

impl LocalSearchUi {
    /// Messages indexés.
    pub(super) fn len(&self) -> usize {
        self.index.len()
    }
}

/// Invite du champ en mode local.
pub(super) fn hint(lang: Lang) -> &'static str {
    jt::LS_HINT.get(lang)
}

/// Onglets du panneau des résultats : « Discord », « Sur cet ordinateur ».
/// Rien si le plugin est coupé. Renvoie le mode choisi (`true` : local).
pub(super) fn mode_tabs(ui: &mut egui::Ui, t: &Tokens, lang: Lang, local: bool, indexed: usize) -> Option<bool> {
    if !plugins::on(Plugin::LocalSearch) {
        return None;
    }
    let mut to = None;
    ui.horizontal(|ui| {
        if ui.selectable_label(!local, jt::LS_MODE_DISCORD.get(lang)).on_hover_text(jt::LS_MODE_DISCORD_TIP.get(lang)).clicked() && local {
            to = Some(false);
        }
        if ui.selectable_label(local, jt::LS_MODE_LOCAL.get(lang)).on_hover_text(jt::LS_TOGGLE_TIP.get(lang)).clicked() && !local {
            to = Some(true);
        }
    });
    if local {
        ui.add(egui::Label::new(egui::RichText::new(jf::ls_note(lang, indexed)).small().color(t.colors.muted)).wrap());
    }
    to
}

/// Marges du sélecteur « Local » autour de son texte.
const CHIP_PAD: egui::Vec2 = egui::vec2(6.0, 2.0);

/// Largeur que le sélecteur « Local » prend dans le champ de recherche (rien si le plugin est coupé).
pub(super) fn chip_width(ui: &egui::Ui, t: &Tokens, lang: Lang) -> f32 {
    if !plugins::on(Plugin::LocalSearch) {
        return 0.0;
    }
    ui.painter().layout_no_wrap(jt::LS_TOGGLE.get(lang).to_owned(), egui::FontId::proportional(t.typography.small), t.colors.muted).size().x + 2.0 * CHIP_PAD.x + t.spacing.item_x / 2.0
}

/// Sélecteur « Local », rangé dans le champ de recherche (le client n'a pas ce
/// bouton : il ne prend pas de place dans l'en-tête) : petite pastille, pleine
/// quand la recherche se fait sur place. Vrai s'il vient d'être cliqué.
pub(super) fn chip(ui: &mut egui::Ui, t: &Tokens, lang: Lang, on: bool) -> bool {
    if !plugins::on(Plugin::LocalSearch) {
        return false;
    }
    let label = jt::LS_TOGGLE.get(lang);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), egui::FontId::proportional(t.typography.small), t.colors.muted);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * CHIP_PAD, egui::Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let (fill, ink) = if on { (t.colors.accent, t.colors.on_accent) } else { (if hot { t.colors.row_hover } else { t.colors.bg3 }, if hot { t.colors.text_strong } else { t.colors.muted }) };
    ui.painter().rect_filled(rect, t.r_row().at_most(4), fill);
    ui.painter().galley(rect.min + CHIP_PAD, galley, ink);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, on, label));
    resp.on_hover_text(jt::LS_TOGGLE_TIP.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

impl App {
    /// Le champ de recherche cherche sur place.
    pub(in crate::ui) fn local_search_wanted(&self) -> bool {
        self.search.local && plugins::on(Plugin::LocalSearch)
    }

    /// Messages indexés (aperçu, tests).
    pub fn local_search_len(&self) -> usize {
        self.local_search.len()
    }

    /// Suite du champ de recherche : `flip`, le sélecteur « Local » du champ
    /// ([`chip`]) vient d'être cliqué ; `typed`, le texte a changé (résultats
    /// pendant la frappe en mode local).
    pub(in crate::ui) fn local_search_typed(&mut self, flip: bool, typed: bool) {
        if !plugins::on(Plugin::LocalSearch) {
            return;
        }
        let on = self.search.local;
        if flip {
            self.local_search_mode(!on);
        } else if typed && on {
            self.local_search_run(0, None, true);
        }
    }

    /// Change de mode ; une recherche en cours est refaite dans le nouveau.
    pub(in crate::ui) fn local_search_mode(&mut self, local: bool) {
        self.search.local = local;
        if self.search.text.trim().is_empty() {
            if self.store.search.active.as_ref().is_some_and(|a| a.local != local) {
                self.store.close_search();
            }
        } else {
            self.run_search();
            self.flush_commands();
        }
        self.ctx.request_repaint();
    }

    /// Cherche le texte du champ dans l'index et montre la page `page`.
    /// `quiet` (pendant la frappe) : un texte incomplet n'est pas une erreur.
    pub(in crate::ui) fn local_search_run(&mut self, page: usize, sort: Option<Sort>, quiet: bool) -> bool {
        let Some(scope) = self.store.search_scope() else { return false };
        let names = self.search_names(scope);
        let sort = sort.or(self.store.search.active.as_ref().map(|a| a.query.sort)).filter(|s| *s != Sort::Relevance).unwrap_or(Sort::Newest);
        let offset = chrono::Local::now().offset().local_minus_utc();
        let text = self.search.text.clone();
        let parsed = match s::parse(&text, &names, offset, sort) {
            Ok(p) if !local_index::unsupported(&p.query) => p,
            failed => {
                if quiet {
                    // Champ vidé pendant la frappe : les résultats locaux partent.
                    if matches!(failed, Err(s::ParseError::Empty)) && self.store.search.active.as_ref().is_some_and(|a| a.local) {
                        self.store.close_search();
                    }
                } else {
                    match failed {
                        Err(e) => {
                            let text = super::search::error_text(self.lang, &e);
                            self.notify(text);
                        }
                        Ok(_) => self.notify(jt::LS_UNSUPPORTED),
                    }
                }
                return false;
            }
        };
        let ids = self.local_search.index.search(scope, &parsed.query, &local_index::fold_terms(&parsed.terms));
        let last = ids.len().saturating_sub(1) / s::PAGE;
        let page = page.min(last).min(s::MAX_OFFSET / s::PAGE);
        let hits: Vec<Hit> = ids.iter().skip(page * s::PAGE).take(s::PAGE).filter_map(|id| self.local_search.index.message(*id)).map(|message| Hit { message, thread_name: None }).collect();
        self.store.search.show_local(scope, text.trim().to_string(), parsed, page, ids.len() as u64, hits);
        self.search.to_top = true;
        if !quiet {
            self.search.hidden = true;
        }
        self.ctx.request_repaint();
        true
    }

    /// Avant que le magasin reçoive `e` : un message supprimé quitte l'index.
    pub(in crate::ui) fn local_search_before(&mut self, e: &Event) {
        if self.local_search.on
            && let Event::MessageDelete { id, .. } = e
        {
            self.local_search.index.remove(*id);
        }
    }

    fn local_search_save(&mut self) {
        self.local_search.last_save = Some(Instant::now());
        if !self.local_search.disk || !std::mem::take(&mut self.local_search.index.dirty) {
            return;
        }
        let Some(user) = self.local_search.user else { return };
        if !self.local_search.known.as_ref().is_some_and(|k| k.contains(&user)) {
            return;
        }
        let payload = Payload::Index(Box::new(self.local_search.index.for_saving()));
        if let Some(files) = self.journal.vault.files() {
            if self.local_search.index.is_empty() {
                files.purge(Kind::Index, user);
            } else {
                files.save(user, payload);
            }
        }
    }

    /// Oublie l'index, en mémoire et sur le disque (plugin coupé, historique
    /// hors ligne effacé ou désactivé).
    pub(in crate::ui) fn local_search_wipe(&mut self, memory: bool) {
        if !self.local_search.on {
            return;
        }
        if memory {
            self.local_search.index.clear();
            if self.store.search.active.as_ref().is_some_and(|a| a.local) {
                self.store.close_search();
            }
        }
        self.local_search.index.dirty = false;
        if self.journal.vault.attached()
            && let Some(files) = self.journal.vault.files()
        {
            files.purge_all(Kind::Index);
        }
        self.local_search.last_save = Some(Instant::now());
    }

    /// Un autre plugin que le journal se sert du fil des fichiers.
    pub(in crate::ui) fn local_search_uses_vault(&self) -> bool {
        self.local_search.on && self.local_search.disk
    }

    /// À chaque passage de logique. Plugin coupé : une comparaison, rien d'autre.
    pub(in crate::ui) fn local_search_tick(&mut self) {
        let on = plugins::on(Plugin::LocalSearch);
        if !on && !self.local_search.on {
            return;
        }
        // `local.toml` pas encore lu : l'état du plugin n'est pas encore le vrai.
        if !self.local.loaded {
            return;
        }
        if !on {
            self.local_search_wipe(true);
            self.local_search = LocalSearchUi::default();
            self.search.local = false;
            self.vault_release_if_idle();
            self.ctx.request_repaint();
            return;
        }
        if !self.local_search.on {
            self.local_search.on = true;
            self.local_search.last_save = Some(Instant::now());
        }
        // Autre compte : l'index de l'ancien est enregistré, puis quitte la mémoire.
        let me = self.store.me.as_ref().map(|u| u.id);
        if me != self.local_search.user {
            self.local_search_save();
            self.local_search.index = LocalIndex::default();
            self.local_search.revs.clear();
            self.local_search.user = me;
            self.local_search.asked = None;
        }
        // Sur le disque seulement avec l'historique hors ligne.
        let disk = self.journal.vault.attached() && plugins::on(Plugin::OfflineHistory);
        if disk != self.local_search.disk {
            if disk {
                self.local_search.disk = true;
                self.local_search.index.dirty = !self.local_search.index.is_empty();
            } else {
                // L'index reste en mémoire ; le fichier part.
                self.local_search_wipe(false);
                self.local_search.disk = false;
                self.local_search.asked = None;
                self.local_search.known = None;
                self.vault_release_if_idle();
            }
        }
        if self.local_search.disk {
            // Comptes de la liste, hors ceux qu'on retire : les autres n'ont plus d'index.
            let mut known: Vec<u64> = self.prefs.now.accounts.iter().filter(|a| !self.forgetting.contains(&a.key)).filter_map(|a| a.user_id).collect();
            known.sort_unstable();
            known.dedup();
            if self.local_search.known.as_ref() != Some(&known) {
                if let Some(files) = self.journal.vault.files() {
                    files.keep_only(Kind::Index, known.clone());
                }
                self.local_search.known = Some(known);
            }
            if let Some(u) = me
                && self.local_search.asked != Some(u)
                && self.local_search.known.as_ref().is_some_and(|k| k.contains(&u))
                && let Some(files) = self.journal.vault.files()
            {
                files.load(Kind::Index, u);
                self.local_search.asked = Some(u);
            }
            self.vault_receive();
        }
        // Les salons dont la fenêtre a changé sont (re)rangés : nouveaux
        // messages, pages d'historique, modifications.
        let ls = &mut self.local_search;
        for (id, cs) in &self.store.chans {
            if ls.revs.get(id) != Some(&cs.rev) {
                for m in &cs.msgs {
                    ls.index.upsert(m, cs.stamped.get(&m.id));
                }
                ls.revs.insert(*id, cs.rev);
            }
        }
        if ls.revs.len() > self.store.chans.len() {
            let chans = &self.store.chans;
            ls.revs.retain(|c, _| chans.contains_key(c));
        }
        if self.local_search.index.dirty && self.local_search.last_save.is_none_or(|at| at.elapsed() >= SAVE_EVERY) {
            self.local_search_save();
        }
    }

    /// Index relu du disque : il rejoint ce que la session a déjà rangé.
    pub(in crate::ui) fn local_search_loaded(&mut self, user: u64, loaded: Option<Box<LocalIndex>>) {
        let Some(loaded) = loaded else { return };
        if !self.local_search.on || self.local_search.user != Some(user) {
            return;
        }
        let dirty = self.local_search.index.dirty;
        self.local_search.index.merge(*loaded);
        self.local_search.index.dirty = dirty;
    }

    /// À la fermeture : l'index est enregistré (le fil est attendu par le journal).
    pub(in crate::ui) fn local_search_exit(&mut self) {
        if self.local_search.on && self.local.loaded {
            self.local_search_save();
        }
    }

    /// Aperçu hors ligne : plugin activé, mode local, et une recherche.
    #[doc(hidden)]
    pub fn local_search_preview(&mut self, text: &str) {
        self.local.loaded = true;
        self.set_plugin(Plugin::LocalSearch, true);
        self.local_search_tick();
        self.search.local = true;
        self.search.text = text.to_string();
        self.local_search_run(0, None, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::journal::tests_support::message;
    use crate::state::model::{Dm, User};
    use crate::state::Cmd;
    use crate::ui::tests::offline_boot;
    use std::sync::{Arc, Mutex};

    const C: u64 = 100;

    #[derive(Clone, Default)]
    struct Net {
        events: Arc<Mutex<Vec<Event>>>,
        sent: Arc<Mutex<Vec<Cmd>>>,
    }

    impl crate::ui::Backend for Net {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
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

    fn app(net: Net) -> App {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        s.apply(Event::History { channel_id: C, before: None, messages: vec![message(10, C, "Réunion demain"), message(11, C, "rien à voir"), message(12, C, "la réunion est annulée")] });
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), s, images));
        app.lang = Lang::Fr;
        app.local.loaded = true;
        app
    }

    fn searches(net: &Net) -> usize {
        net.sent.lock().unwrap().iter().filter(|c| matches!(c, Cmd::Search(_))).count()
    }

    /// Plugin coupé : rien n'est indexé, et la recherche part chez Discord comme avant.
    #[test]
    fn disabled_plugin_indexes_nothing() {
        let net = Net::default();
        let mut app = app(net.clone());
        app.set_plugin(Plugin::LocalSearch, false);
        app.logic_pass();
        assert_eq!(app.local_search_len(), 0);
        assert!(!app.local_search.on && !app.journal.vault.running());
        app.search.local = true;
        assert!(!app.local_search_wanted());
        assert!(app.search("réunion"));
        assert_eq!(searches(&net), 1);
        assert!(app.store.search.active.as_ref().is_some_and(|a| !a.local));
    }

    #[test]
    fn local_mode_finds_while_typing_and_asks_discord_nothing() {
        let net = Net::default();
        let mut app = app(net.clone());
        assert!(plugins::on(Plugin::LocalSearch));
        app.logic_pass();
        assert_eq!(app.local_search_len(), 3);
        app.search.local = true;
        // Pendant la frappe : un début de mot suffit, aucune erreur annoncée.
        app.search.text = "reun".into();
        assert!(app.local_search_run(0, None, true));
        let a = app.store.search.active.as_ref().expect("résultats");
        assert!(a.local && a.phase == s::Phase::Done && a.total == Some(2));
        assert_eq!(a.hits.iter().map(|h| h.message.id).collect::<Vec<_>>(), vec![12, 10]);
        // « Entrée », tri, page : toujours sur place.
        assert!(app.search("réunion annulée"));
        assert_eq!(app.store.search.active.as_ref().unwrap().hits.len(), 1);
        assert!(app.local_search_run(0, Some(Sort::Oldest), false));
        assert!(!app.store.search_page(1) && !app.store.search_sort(Sort::Newest), "le magasin ne demande rien pour une recherche locale");
        // Un filtre que l'index ne connaît pas : refusé.
        app.search.text = "épinglé:oui".into();
        assert!(!app.local_search_run(0, None, false));
        // Nouveau message, modification, suppression : l'index suit.
        net.events.lock().unwrap().push(Event::MessageCreate(message(13, C, "réunion reportée")));
        net.events.lock().unwrap().push(Event::MessageDelete { channel_id: C, id: 10 });
        app.logic_pass();
        app.search.text = "réunion".into();
        assert!(app.local_search_run(0, Some(Sort::Newest), true));
        assert_eq!(app.store.search.active.as_ref().unwrap().hits.iter().map(|h| h.message.id).collect::<Vec<_>>(), vec![13, 12]);
        // Champ vidé pendant la frappe : le panneau local se ferme.
        app.search.text.clear();
        assert!(!app.local_search_run(0, None, true));
        assert!(app.store.search.active.is_none());
        app.flush_commands();
        assert_eq!(searches(&net), 0, "rien n'est demandé à Discord");
        // Retour au mode Discord : la recherche part.
        app.search.text = "réunion".into();
        app.local_search_mode(false);
        assert_eq!(searches(&net), 1);
        // Plugin coupé en cours de route : l'index quitte la mémoire.
        app.set_plugin(Plugin::LocalSearch, false);
        app.logic_pass();
        assert_eq!(app.local_search_len(), 0);
    }

    /// Avec un disque : enregistré (chiffré) seulement si l'historique hors
    /// ligne est activé, relu à l'ouverture suivante, effacé avec le plugin.
    #[cfg(windows)]
    #[test]
    fn index_is_saved_only_with_offline_history_and_erased_with_the_plugin() {
        use crate::account::AccountKey;
        use crate::offline::vault::Vault;
        use crate::settings::Account;
        let root = std::env::temp_dir().join(format!("ferrocord-test-recherche-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let listed = |app: &mut App| app.prefs.now.accounts = vec![Account { user_id: Some(1), ..Account::new(AccountKey::of_user(1)) }];
        let file = root.join("recherche").join("1.bin");

        let mut app = app(Net::default());
        listed(&mut app);
        // Les brouillons et les messages programmés (lot E3) se servent du même fil : coupés ici.
        app.set_plugin(Plugin::Drafts, false);
        app.set_plugin(Plugin::Scheduled, false);
        app.attach_vault(Vault::new(Some(root.clone()), Arc::new(|| {})));
        app.set_plugin(Plugin::OfflineHistory, false);
        app.logic_pass();
        app.local_search_exit();
        assert_eq!(app.local_search_len(), 3);
        assert!(!root.exists() && !app.journal.vault.running(), "sans historique hors ligne : en mémoire seulement");
        app.set_plugin(Plugin::OfflineHistory, true);
        app.logic_pass();
        app.local_search_exit();
        app.journal.vault.flush(Duration::from_secs(10));
        let raw = std::fs::read(&file).expect("index enregistré");
        assert!(!raw.windows(7).any(|w| w == b"demain\""), "chiffré au repos");

        // Nouvelle session, salon pas encore rechargé : l'index revient du disque.
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut next = App::with_context(ctx, offline_boot(Box::new(Net::default()), s, images));
        next.local.loaded = true;
        listed(&mut next);
        next.set_plugin(Plugin::Drafts, false);
        next.set_plugin(Plugin::Scheduled, false);
        next.attach_vault(Vault::new(Some(root.clone()), Arc::new(|| {})));
        next.logic_pass();
        next.journal.vault.flush(Duration::from_secs(10));
        next.logic_pass();
        assert_eq!(next.local_search_len(), 3);
        next.search.local = true;
        next.search.text = "annulée".into();
        assert!(next.local_search_run(0, None, false));
        assert_eq!(next.store.search.active.as_ref().unwrap().hits[0].message.id, 12);

        next.set_plugin(Plugin::LocalSearch, false);
        next.logic_pass();
        assert!(!next.journal.vault.running(), "fil rendu");
        assert!(next.local_search_len() == 0 && next.store.search.active.is_none());
        for _ in 0..500 {
            if !root.join("recherche").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!root.join("recherche").exists(), "plugin coupé : index effacé du disque");
        let _ = std::fs::remove_dir_all(&root);
    }
}
