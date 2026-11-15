//! Plugin « Brouillons persistants » (lot E3) : le texte non envoyé de
//! chaque salon et la réponse en préparation survivent au redémarrage.
//!
//! - Fichier : un par compte, chiffré comme l'historique hors ligne
//!   (`offline::vault`, dossier `brouillons`). Un compte hors de la liste
//!   (`DISCORD_TOKEN`) et un système sans DPAPI gardent tout en mémoire.
//! - Écriture différée et groupée : la saisie ne fait qu'avancer un compteur
//!   (`Composer::rev`) ; le passage de logique le compare, arme une seule
//!   échéance ([`SAVE_DELAY`] après la première frappe non enregistrée) et
//!   n'écrit qu'à cette échéance, à la fermeture de la session et à la
//!   sortie. Jamais d'écriture par frappe, un seul réveil par écriture.
//! - Lecture : une fois, à l'ouverture du compte. Tant qu'elle n'est pas
//!   revenue, rien n'est écrit (le fichier ne peut pas être écrasé par un
//!   état partiel).
//! - Les pièces jointes en attente ne sont pas gardées.
//!
//! Plugin coupé : une comparaison par passage ; les fichiers sont effacés,
//! les brouillons restent en mémoire comme avant.

use super::complete::Draft;
use super::msg_actions::ReplyDraft;
use super::App;
use crate::offline::vault::{Kind, Payload};
use crate::plugins::{self, Id as Plugin};
use crate::state::drafts::{SavedDraft, SavedDrafts, SavedReply};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Attente entre la première frappe non enregistrée et l'écriture.
pub const SAVE_DELAY: Duration = Duration::from_secs(5);

#[derive(Default)]
pub(in crate::ui) struct DraftsUi {
    /// Plugin activé au dernier passage.
    on: bool,
    /// Compte dont les brouillons sont suivis.
    user: Option<u64>,
    /// Compte dont le fichier a été demandé, puis reçu.
    asked: Option<u64>,
    loaded: Option<u64>,
    /// Comptes de la liste, tels que transmis au fil.
    known: Option<Vec<u64>>,
    /// Ce qui est sur le disque (dates de chaque brouillon).
    saved: SavedDrafts,
    /// Réponse en préparation de chaque salon (la saisie n'en garde qu'une).
    replies: HashMap<u64, SavedReply>,
    /// Compteur de la saisie et réponse vus au dernier passage.
    rev_seen: u64,
    reply_seen: Option<(u64, u64, bool)>,
    chan_seen: Option<u64>,
    /// Échéance de la prochaine écriture (rien à écrire : `None`).
    due: Option<Instant>,
    /// Écritures demandées au fil (tests).
    writes: u32,
}

impl App {
    /// Le plugin se sert du fil des fichiers.
    pub(in crate::ui) fn drafts_uses_vault(&self) -> bool {
        self.drafts.on
    }

    /// Écritures demandées depuis le lancement (tests).
    #[doc(hidden)]
    pub fn drafts_writes(&self) -> u32 {
        self.drafts.writes
    }

    /// Quelque chose a changé : l'échéance d'écriture est armée si elle ne
    /// l'est pas déjà (son réveil, le seul, est demandé par `drafts_tick`).
    fn drafts_touch(&mut self) {
        if self.drafts.due.is_none() {
            self.drafts.due = Some(Instant::now() + SAVE_DELAY);
        }
    }

    /// Ce qui serait écrit maintenant : brouillons de la saisie et réponses.
    fn drafts_snapshot(&self, now: i64) -> SavedDrafts {
        let mut out = self.drafts.saved.clone();
        let mut seen: Vec<u64> = Vec::new();
        for (c, d) in self.input.all_drafts() {
            seen.push(c);
            out.put(c, SavedDraft { text: d.text.clone(), links: d.links.clone(), reply: self.drafts.replies.get(&c).cloned(), at: 0 }, now);
        }
        for (c, r) in &self.drafts.replies {
            if !seen.contains(c) {
                seen.push(*c);
                out.put(*c, SavedDraft { reply: Some(r.clone()), ..Default::default() }, now);
            }
        }
        // Brouillon envoyé ou effacé depuis la dernière écriture.
        let gone: Vec<u64> = out.iter().map(|(c, _)| c).filter(|c| !seen.contains(c)).collect();
        for c in gone {
            out.put(c, SavedDraft::default(), now);
        }
        out.prune(now, plugins::number(Plugin::Drafts, plugins::DRAFTS_DAYS));
        out
    }

    /// Écrit les brouillons du compte suivi s'ils ont changé. Sans effet
    /// tant que le fichier n'a pas été relu, ou si la saisie est déjà à un
    /// autre compte.
    pub(in crate::ui) fn drafts_save(&mut self) {
        self.drafts.due = None;
        let Some(user) = self.drafts.user else { return };
        if !self.drafts.on || self.drafts.loaded != Some(user) || self.input.owner() != Some(user) {
            return;
        }
        let now = crate::state::store::now_ms();
        let next = self.drafts_snapshot(now);
        if next == self.drafts.saved {
            return;
        }
        self.drafts.saved = next;
        if !self.drafts.known.as_ref().is_some_and(|k| k.contains(&user)) {
            return;
        }
        if let Some(files) = self.journal.vault.files() {
            if self.drafts.saved.is_empty() {
                files.purge(Kind::Drafts, user);
            } else {
                files.save(user, Payload::Raw(Kind::Drafts, self.drafts.saved.encode(user)));
            }
            self.drafts.writes += 1;
        }
    }

    /// Brouillons relus du disque : ils rejoignent la saisie, sans écraser
    /// ce qui a déjà été tapé depuis le lancement.
    pub(in crate::ui) fn drafts_loaded(&mut self, user: u64, bytes: Option<Vec<u8>>) {
        if !self.drafts.on || self.drafts.user != Some(user) || self.drafts.asked != Some(user) {
            return;
        }
        self.drafts.loaded = Some(user);
        let Some(mut saved) = bytes.and_then(|b| SavedDrafts::decode(&b, user)) else { return };
        saved.prune(crate::state::store::now_ms(), plugins::number(Plugin::Drafts, plugins::DRAFTS_DAYS));
        let items: Vec<(u64, Draft)> = saved.iter().filter(|(_, d)| !d.text.trim().is_empty()).map(|(c, d)| (c, Draft { text: d.text.clone(), links: d.links.clone() })).collect();
        self.input.restore(Some(user), items);
        for (c, d) in saved.iter() {
            if let Some(r) = &d.reply {
                self.drafts.replies.entry(c).or_insert_with(|| r.clone());
            }
        }
        self.drafts.saved = saved;
        // Le salon déjà ouvert retrouve sa réponse au prochain passage.
        self.drafts.chan_seen = None;
        self.drafts.rev_seen = self.input.rev();
        self.ctx.request_repaint();
    }

    /// Suit la réponse en préparation : gardée par salon, rendue au retour
    /// dans le salon, oubliée quand elle est envoyée ou annulée.
    fn drafts_track_reply(&mut self) {
        // Les actions valent pour un autre compte (première image) : elles vont être oubliées.
        if self.actions.me != self.drafts.user {
            return;
        }
        let cur = self.store.channel;
        let now = self.actions.reply.as_ref().map(|r| (r.channel_id, r.message_id, r.mention));
        if now == self.drafts.reply_seen && cur == self.drafts.chan_seen {
            return;
        }
        match &self.actions.reply {
            Some(r) => {
                let saved = SavedReply { message_id: r.message_id, author: r.author.clone(), mention: r.mention };
                if self.drafts.replies.get(&r.channel_id) != Some(&saved) {
                    self.drafts.replies.insert(r.channel_id, saved);
                    self.drafts_touch();
                }
            }
            None => {
                if let Some(c) = cur {
                    if self.drafts.reply_seen.is_some_and(|(rc, ..)| rc == c) {
                        // Envoyée ou annulée dans ce salon.
                        if self.drafts.replies.remove(&c).is_some() {
                            self.drafts_touch();
                        }
                    } else if let Some(r) = self.drafts.replies.get(&c) {
                        self.actions.reply = Some(ReplyDraft { channel_id: c, message_id: r.message_id, author: r.author.clone(), mention: r.mention });
                        self.ctx.request_repaint();
                    }
                }
            }
        }
        self.drafts.reply_seen = self.actions.reply.as_ref().map(|r| (r.channel_id, r.message_id, r.mention));
        self.drafts.chan_seen = cur;
    }

    /// À chaque passage de logique. Plugin coupé : une comparaison, rien d'autre.
    pub(in crate::ui) fn drafts_tick(&mut self) {
        let on = plugins::on(Plugin::Drafts);
        if !on && !self.drafts.on {
            return;
        }
        // `local.toml` pas encore lu : l'état du plugin n'est pas encore le vrai.
        if !self.local.loaded {
            return;
        }
        if !on {
            // Coupé : les fichiers partent, les brouillons restent en mémoire comme avant.
            if self.journal.vault.attached()
                && let Some(files) = self.journal.vault.files()
            {
                files.purge_all(Kind::Drafts);
            }
            self.drafts = DraftsUi::default();
            self.vault_release_if_idle();
            return;
        }
        self.drafts.on = true;
        // Autre compte : ce qui restait à écrire l'a été à la fermeture de la session.
        let me = self.store.me.as_ref().map(|u| u.id);
        if me != self.drafts.user {
            self.drafts = DraftsUi { on: true, user: me, known: self.drafts.known.take(), writes: self.drafts.writes, rev_seen: self.input.rev(), ..Default::default() };
        }
        if self.journal.vault.attached() {
            let mut known: Vec<u64> = self.prefs.now.accounts.iter().filter(|a| !self.forgetting.contains(&a.key)).filter_map(|a| a.user_id).collect();
            known.sort_unstable();
            known.dedup();
            if self.drafts.known.as_ref() != Some(&known) {
                if let Some(files) = self.journal.vault.files() {
                    files.keep_only(Kind::Drafts, known.clone());
                }
                self.drafts.known = Some(known);
            }
            if let Some(u) = me
                && self.drafts.asked != Some(u)
            {
                self.drafts.asked = Some(u);
                if self.drafts.known.as_ref().is_some_and(|k| k.contains(&u))
                    && let Some(files) = self.journal.vault.files()
                {
                    files.load(Kind::Drafts, u);
                } else {
                    // Compte hors de la liste : rien à relire.
                    self.drafts.loaded = Some(u);
                }
            }
            self.vault_receive();
        } else if self.drafts.loaded != me {
            // Sans disque : tout reste en mémoire.
            self.drafts.asked = me;
            self.drafts.loaded = me;
        }
        self.drafts_track_reply();
        let rev = self.input.rev();
        if rev != self.drafts.rev_seen {
            self.drafts.rev_seen = rev;
            self.drafts_touch();
        }
        // Un seul réveil : l'échéance de l'écriture, tant qu'il y en a une.
        if let Some(due) = self.drafts.due {
            let now = Instant::now();
            if now >= due {
                self.drafts_save();
            } else {
                self.ctx.request_repaint_after(due - now);
            }
        }
    }

    /// Un plugin du lot E3 se sert du fil des fichiers.
    pub(in crate::ui) fn e3_uses_vault(&self) -> bool {
        self.drafts_uses_vault() || self.scheduled_uses_vault() || self.translate_uses_vault()
    }

    /// Contenu relu par le fil des fichiers pour un plugin du lot E3.
    pub(in crate::ui) fn e3_loaded(&mut self, kind: Kind, user: u64, bytes: Option<Vec<u8>>) {
        match kind {
            Kind::Drafts => self.drafts_loaded(user, bytes),
            Kind::Scheduled => self.scheduled_loaded(user, bytes),
            Kind::Secrets => self.translate_key_loaded(bytes),
            _ => {}
        }
    }

    /// Application quittée : dernières écritures des plugins du lot E3
    /// (le fil des fichiers est attendu ensuite par le journal).
    pub(in crate::ui) fn e3_exit(&mut self) {
        self.drafts_flush();
        self.scheduled_exit();
    }

    /// Session fermée ou application quittée : ce qui restait à écrire l'est
    /// maintenant (avant que la saisie soit vidée).
    pub(in crate::ui) fn drafts_flush(&mut self) {
        if self.drafts.on && self.local.loaded {
            self.drafts_track_reply();
            self.drafts_save();
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::account::AccountKey;
    use crate::offline::vault::Vault;
    use crate::settings::Account;
    use crate::state::model::{Dm, User};
    use crate::state::{Cmd, Event};
    use crate::ui::tests::{frame, offline_boot};
    use eframe::egui;
    use std::path::PathBuf;
    use std::sync::Arc;

    const A: u64 = 100;
    const B: u64 = 101;
    const WAIT: Duration = Duration::from_secs(10);

    struct Offline;
    impl crate::ui::Backend for Offline {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<Cmd>) {}
    }

    fn root(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ferrocord-test-brouillons-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn app(root: Option<&PathBuf>) -> App {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(A, "Alice".into(), 0), Dm::new(B, "Bob".into(), 0)] });
        s.select_dms();
        s.select_channel(A);
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Offline), s, images));
        app.lang = crate::i18n::Lang::Fr;
        app.local.loaded = true;
        app.prefs.now.accounts = vec![Account { user_id: Some(1), ..Account::new(AccountKey::of_user(1)) }];
        if let Some(root) = root {
            app.attach_vault(Vault::new(Some(root.clone()), Arc::new(|| {})));
        }
        app
    }

    /// Attend la réponse du fil des fichiers, puis un passage.
    fn settle(app: &mut App) {
        app.logic_pass();
        app.journal.vault.flush(WAIT);
        app.logic_pass();
    }

    #[test]
    fn drafts_and_reply_survive_a_restart_encrypted() {
        let root = root("redemarrage");
        let file = root.join("brouillons").join("1.bin");
        let mut one = app(Some(&root));
        assert!(plugins::on(Plugin::Drafts), "activé par défaut");
        settle(&mut one);
        one.set_composer_text("à finir demain, promis");
        frame(&mut one, Vec::new());
        one.actions.reply = Some(ReplyDraft { channel_id: A, message_id: 77, author: "Alice".into(), mention: true });
        frame(&mut one, Vec::new());
        one.store.select_channel(B);
        frame(&mut one, Vec::new());
        one.set_composer_text("second brouillon");
        frame(&mut one, Vec::new());
        assert_eq!(one.drafts_writes(), 0, "rien n'est écrit pendant la frappe");
        assert!(one.drafts.due.is_some(), "une échéance, une seule");
        // Sortie : tout part en une écriture.
        one.e3_exit();
        one.journal.vault.flush(WAIT);
        assert_eq!(one.drafts_writes(), 1);
        let raw = std::fs::read(&file).unwrap();
        assert!(!raw.windows(6).any(|w| w == "promis".as_bytes()), "chiffré au repos");
        drop(one);

        let mut two = app(Some(&root));
        settle(&mut two);
        frame(&mut two, Vec::new());
        assert_eq!(two.composer_text(), "à finir demain, promis");
        assert!(two.input.has_draft(B), "le crayon du second salon");
        frame(&mut two, Vec::new());
        assert_eq!(two.actions.reply.as_ref().map(|r| (r.channel_id, r.message_id, r.mention)), Some((A, 77, true)), "la réponse en cours revient");
        two.store.select_channel(B);
        frame(&mut two, Vec::new());
        assert_eq!(two.composer_text(), "second brouillon");
        // Envoyé : le brouillon quitte le fichier à l'écriture suivante.
        two.submit();
        frame(&mut two, Vec::new());
        two.drafts_flush();
        two.journal.vault.flush(WAIT);
        drop(two);
        let mut three = app(Some(&root));
        settle(&mut three);
        frame(&mut three, Vec::new());
        assert!(!three.input.has_draft(B), "envoyé : plus de brouillon");
        assert_eq!(three.composer_text(), "à finir demain, promis");
        // Plugin coupé : les fichiers partent, le texte en mémoire reste.
        three.set_plugin(Plugin::Drafts, false);
        settle(&mut three);
        assert!(!root.join("brouillons").exists(), "fichiers effacés");
        assert!(!three.drafts.on);
        assert_eq!(three.composer_text(), "à finir demain, promis");
        three.set_composer_text("autre chose");
        frame(&mut three, Vec::new());
        three.e3_exit();
        assert_eq!(three.drafts_writes(), 0, "coupé : aucune écriture");
        assert!(!root.join("brouillons").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Dix frappes : une échéance armée, une écriture à son terme, pas dix.
    #[test]
    fn typing_arms_one_deadline_and_writes_once() {
        let root = root("groupe");
        let mut app = app(Some(&root));
        settle(&mut app);
        for i in 0..10 {
            app.set_composer_text(&"x".repeat(i + 1));
            frame(&mut app, Vec::new());
        }
        let due = app.drafts.due.expect("échéance armée");
        assert_eq!(app.drafts_writes(), 0);
        frame(&mut app, Vec::new());
        assert_eq!(app.drafts.due, Some(due), "l'échéance ne recule pas à chaque frappe");
        // À l'échéance : une écriture.
        app.drafts.due = Some(Instant::now());
        app.logic_pass();
        assert_eq!(app.drafts_writes(), 1);
        assert!(app.drafts.due.is_none());
        // Rien de nouveau : rien n'est récrit.
        app.logic_pass();
        app.drafts_flush();
        assert_eq!(app.drafts_writes(), 1);
        app.journal.vault.flush(WAIT);
        let _ = std::fs::remove_dir_all(&root);
    }

    /// Sans disque (aperçu, système sans DPAPI) : tout reste en mémoire.
    #[test]
    fn without_a_disk_nothing_is_written() {
        let mut app = app(None);
        settle(&mut app);
        app.set_composer_text("en mémoire");
        frame(&mut app, Vec::new());
        app.e3_exit();
        assert_eq!(app.drafts_writes(), 0);
        assert!(!app.journal.vault.running());
    }
}
