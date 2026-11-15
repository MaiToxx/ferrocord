//! Journal des messages supprimés et modifiés, côté interface (plugin, lot
//! E1) : ce qui est gardé à la réception d'une suppression ou d'une
//! modification, quand le journal est lu, enregistré et effacé, et ce qui
//! s'affiche sous un message. Le modèle est dans `state::journal`, le disque
//! et le chiffrement dans `offline::vault` (son fil, lancé à la demande).
//!
//! Plugin coupé : [`App::journal_before`] et [`App::journal_tick`] rendent
//! la main à la première ligne ; rien n'est gardé, aucun fil ne tourne.
//!
//! - On ne garde que ce que le client avait en mémoire : aucune requête.
//! - Jamais mes propres messages, ni ceux que j'ai supprimés moi-même
//!   (modération), ni les messages système.
//! - Lecture : à l'ouverture d'un compte de la liste. Enregistrement : au
//!   plus toutes les [`SAVE_EVERY`] quand quelque chose a changé, et à la
//!   fermeture. Un compte hors de la liste (`DISCORD_TOKEN`) reste en mémoire.
//! - Effacement : plugin coupé, « Effacer le journal », compte retiré.

use super::App;
use crate::i18n::table::journal::{f as jf, t as jt};
use crate::i18n::{time, Lang};
use crate::offline::vault::{Kind, Loaded, Payload, Vault};
use crate::plugins::{self, Id as Plugin};
use crate::state::journal::{self, Journal};
use crate::state::model::Message;
use crate::state::{Cmd, Event};
use crate::theme::{icons::Icon, Clock, Tokens};
use eframe::egui::{self, Id, RichText};
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Écart minimal entre deux enregistrements.
pub const SAVE_EVERY: Duration = Duration::from_secs(30);
/// Attente au plus, à la fermeture, de la dernière écriture.
const EXIT_WAIT: Duration = Duration::from_secs(2);
/// Suppressions que j'ai demandées, retenues au plus (les plus anciennes partent).
const MAX_MINE: usize = 512;
/// Opacité d'un message supprimé.
const GHOST_OPACITY: f32 = 0.55;
/// Bit des identifiants des messages dessinés hors de leur liste (`ui::search`).
const UI_SALT: u64 = 1 << 63;

#[derive(Default)]
pub(in crate::ui) struct JournalUi {
    /// Fichiers des plugins locaux (aussi ceux de la recherche locale).
    pub(in crate::ui) vault: Vault,
    data: Journal,
    /// Plugin activé à la dernière image.
    on: bool,
    /// Compte dont `data` est le journal.
    user: Option<u64>,
    /// Compte dont le journal a été demandé au disque.
    asked: Option<u64>,
    last_save: Option<Instant>,
    /// Comptes de la liste, tels que transmis au fil (les autres sont effacés).
    known: Option<Vec<u64>>,
    /// Messages dont j'ai demandé la suppression : pas au journal.
    mine: VecDeque<u64>,
    /// Aperçu hors ligne : exemple à poser dès que le salon ouvert est chargé.
    preview: bool,
}

fn versions_id() -> Id {
    Id::unique("journal-versions")
}

/// Clic sur « (modifié) » : les versions du message `msg` sont dépliées ou
/// repliées à l'image suivante ([`App::journal_tick`]).
pub(super) fn ask_versions(ctx: &egui::Context, msg: u64) {
    ctx.data_mut(|d| d.insert_temp(versions_id(), msg & !UI_SALT));
    ctx.request_repaint();
}

/// Le message est-il gardé par le journal après sa suppression ?
pub(super) fn is_ghost(m: &Message) -> bool {
    m.meta.is_gone()
}

/// Le journal garde des versions précédentes : « (modifié) » les déplie.
pub(super) fn has_versions(m: &Message) -> bool {
    m.meta.trace.as_ref().is_some_and(|t| !t.versions.is_empty())
}

/// À appeler au début du dessin d'un message : un message supprimé est atténué.
pub(super) fn dim(ui: &mut egui::Ui, m: &Message) {
    if is_ghost(m) {
        ui.multiply_opacity(GHOST_OPACITY);
    }
}

/// Sous le texte d'un message : ses versions précédentes si elles sont
/// dépliées, et la mention « supprimé ». Rien pour un message sans trace.
pub(super) fn trace_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message) {
    let Some(tr) = m.meta.trace.as_deref() else { return };
    let h24 = t.layout.clock == Clock::H24;
    if tr.open && !tr.versions.is_empty() {
        let inner = egui::Frame::new().inner_margin(t.quote_indent()).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(jt::VERSIONS.get(lang)).small().family(super::fonts::demi()).color(t.colors.muted));
            for v in &tr.versions {
                ui.label(RichText::new(jf::version_at(lang, time::clock_mid(lang, v.at_ms, h24))).small().color(t.colors.faint));
                ui.add(egui::Label::new(RichText::new(v.text.as_str()).color(t.colors.muted)).wrap());
            }
        });
        let rect = inner.response.rect;
        let bar = egui::Rect::from_min_size(rect.min, egui::vec2(t.markdown.quote_width, rect.height()));
        ui.painter().rect_filled(bar, t.markdown.quote_width / 2.0, t.markdown.quote_bar);
    }
    if let Some(at) = tr.gone_ms {
        ui.horizontal(|ui| {
            // La mention reste lisible : elle n'est pas atténuée avec le message.
            ui.set_opacity(1.0);
            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
            super::util::inline_icon(ui, Icon::Trash, t.typography.small, t.colors.danger);
            ui.label(RichText::new(jf::deleted_at(lang, time::clock_mid(lang, at, h24))).small().color(t.colors.danger));
        });
    }
}

/// Clic droit sur un message supprimé : seulement « Copier le texte » (plus
/// rien ne peut lui être fait chez Discord). Vrai s'il est survolé.
pub(super) fn ghost_after(t: &Tokens, lang: Lang, m: &Message, resp: &egui::Response) -> bool {
    resp.context_menu(|ui| {
        if super::menu::item(ui, t, jt::COPY_TEXT.get(lang), Some(Icon::Copy), false).clicked() {
            ui.ctx().copy_text(m.content.clone());
            ui.close();
        }
    });
    resp.hovered()
}

/// Salon dont une page d'historique arrive avec cet événement.
fn page_channel(e: &Event) -> Option<u64> {
    match e {
        Event::History { channel_id, .. } | Event::HistoryAfter { channel_id, .. } => Some(*channel_id),
        Event::Search(crate::state::search::SearchEvent::Around { channel_id, .. }) => Some(*channel_id),
        _ => None,
    }
}

impl App {
    /// Branche le dossier des fichiers des plugins locaux (journal, index).
    pub fn attach_vault(&mut self, vault: Vault) {
        self.journal.vault = vault;
    }

    /// (messages supprimés, messages modifiés) gardés par le journal.
    pub fn journal_counts(&self) -> (usize, usize) {
        self.journal.data.counts()
    }

    fn journal_is_bot(&self, m: &Message) -> bool {
        m.meta.app.is_some() || self.store.people.lock().user(m.author_id).is_some_and(|u| u.bot)
    }

    /// Avant que le magasin reçoive `e`. Une suppression à garder est
    /// traitée ici (le message reste, marqué) : vrai, et le magasin ne doit
    /// pas recevoir l'événement. Une modification garde l'ancien texte.
    /// Rend aussi le salon dont une page arrive ([`App::journal_after`]).
    pub(in crate::ui) fn journal_before(&mut self, e: &Event) -> (bool, Option<u64>) {
        if !plugins::on(Plugin::MessageLog) {
            return (false, None);
        }
        let me = self.store.me.as_ref().map_or(0, |u| u.id);
        match e {
            Event::MessageDelete { channel_id, id } => {
                if let Some(i) = self.journal.mine.iter().position(|x| x == id) {
                    self.journal.mine.remove(i);
                    return (false, None);
                }
                let Some((m, raw)) = self.store.message_in_memory(*channel_id, *id) else { return (false, None) };
                if m.meta.is_gone() {
                    return (true, None);
                }
                if m.author_id == me || m.system.is_some() || (plugins::flag(Plugin::MessageLog, plugins::LOG_BOTS) && self.journal_is_bot(m)) {
                    return (false, None);
                }
                let now = crate::state::store::now_ms();
                self.journal.data.record_delete(m, raw, now, plugins::flag(Plugin::MessageLog, plugins::LOG_FILES));
                self.store.journal_mark_deleted(*channel_id, *id, now);
                (true, None)
            }
            Event::MessageUpdate { channel_id, id, content } => {
                let Some((m, raw)) = self.store.message_in_memory(*channel_id, *id) else { return (false, None) };
                if m.author_id == me || m.system.is_some() || m.meta.is_gone() || raw.unwrap_or(&m.content) == content {
                    return (false, None);
                }
                if plugins::flag(Plugin::MessageLog, plugins::LOG_BOTS) && self.journal_is_bot(m) {
                    return (false, None);
                }
                let old = journal::plain_text(&self.store, m, self.lang, self.h24());
                let v = self.journal.data.record_edit(*channel_id, *id, &old, crate::state::store::now_ms());
                self.store.journal_push_version(*channel_id, *id, &v);
                (false, None)
            }
            _ => (false, page_channel(e)),
        }
    }

    /// Après une page d'historique du salon `channel` : ce que le journal en
    /// sait reprend sa place dans la fenêtre.
    pub(in crate::ui) fn journal_after(&mut self, channel: u64) {
        if self.journal.data.touches(channel) {
            self.store.journal_restore(channel, &self.journal.data);
        }
    }

    /// Commandes qui partent : les suppressions que je demande ne vont pas au journal.
    pub(in crate::ui) fn journal_outgoing(&mut self, cmds: &[Cmd]) {
        if !plugins::on(Plugin::MessageLog) {
            return;
        }
        for c in cmds {
            match c {
                Cmd::Social(crate::state::social::SocialCmd::Delete { message_id, .. }) => self.journal.mine.push_back(*message_id),
                Cmd::Moderation(crate::state::admin::moderation::ModCmd::BulkDelete { ids, .. }) => self.journal.mine.extend(ids.iter().copied()),
                _ => {}
            }
        }
        while self.journal.mine.len() > MAX_MINE {
            self.journal.mine.pop_front();
        }
    }

    /// Un autre plugin que le journal se sert des fichiers.
    fn vault_wanted_elsewhere(&self) -> bool {
        self.local_search_uses_vault() || self.e3_uses_vault()
    }

    /// Le fil des fichiers ne sert plus à aucun plugin : il s'arrête.
    pub(in crate::ui) fn vault_release_if_idle(&mut self) {
        if !self.journal.on && !self.vault_wanted_elsewhere() {
            self.journal.vault.release();
        }
    }

    /// Enregistre le journal du compte en cours s'il a changé.
    fn journal_save(&mut self) {
        self.journal.last_save = Some(Instant::now());
        if !std::mem::take(&mut self.journal.data.dirty) {
            return;
        }
        let Some(user) = self.journal.user else { return };
        if !self.journal.known.as_ref().is_some_and(|k| k.contains(&user)) {
            return;
        }
        let payload = Payload::Journal(Box::new(self.journal.data.clone()));
        if let Some(files) = self.journal.vault.files() {
            if self.journal.data.is_empty() {
                files.purge(Kind::Journal, user);
            } else {
                files.save(user, payload);
            }
        }
    }

    /// Plugin coupé ou journal effacé : plus rien en mémoire ni sur le disque.
    fn journal_wipe(&mut self) {
        self.journal.data.clear();
        self.journal.mine.clear();
        self.store.journal_strip();
        if let Some(files) = self.journal.vault.files() {
            files.purge_all(Kind::Journal);
        }
        self.journal.last_save = Some(Instant::now());
        self.ctx.request_repaint();
    }

    /// À chaque passage de logique. Plugin coupé : une comparaison, rien d'autre.
    pub(in crate::ui) fn journal_tick(&mut self) {
        let on = plugins::on(Plugin::MessageLog);
        if !on && !self.journal.on {
            return;
        }
        // `local.toml` pas encore lu : l'état du plugin n'est pas encore le vrai.
        if !self.local.loaded {
            return;
        }
        if !on {
            self.journal_wipe();
            let vault = std::mem::take(&mut self.journal.vault);
            self.journal = JournalUi { vault, ..Default::default() };
            self.vault_release_if_idle();
            return;
        }
        if !self.journal.on {
            self.journal.on = true;
            self.journal.last_save = Some(Instant::now());
        }
        if self.journal.preview && self.journal_preview_apply() {
            self.journal.preview = false;
        }
        // Clic sur « (modifié) ».
        if let Some(id) = self.ctx.data_mut(|d| d.remove_temp::<u64>(versions_id()))
            && let Some(c) = self.store.channel
        {
            self.store.journal_toggle(c, id);
        }
        // Autre compte : le journal de l'ancien est enregistré, puis quitte la mémoire.
        let me = self.store.me.as_ref().map(|u| u.id);
        if me != self.journal.user {
            self.journal_save();
            self.journal.data.clear();
            self.journal.user = me;
            self.journal.asked = None;
        }
        if self.journal.vault.attached() {
            // Comptes de la liste, hors ceux qu'on retire : les autres n'ont plus de journal.
            let mut known: Vec<u64> = self.prefs.now.accounts.iter().filter(|a| !self.forgetting.contains(&a.key)).filter_map(|a| a.user_id).collect();
            known.sort_unstable();
            known.dedup();
            if self.journal.known.as_ref() != Some(&known) {
                if let Some(files) = self.journal.vault.files() {
                    files.keep_only(Kind::Journal, known.clone());
                }
                self.journal.known = Some(known);
            }
            if let Some(u) = me
                && self.journal.asked != Some(u)
                && self.journal.known.as_ref().is_some_and(|k| k.contains(&u))
                && let Some(files) = self.journal.vault.files()
            {
                files.load(Kind::Journal, u);
                self.journal.asked = Some(u);
            }
            self.vault_receive();
        }
        if self.journal.data.dirty && self.journal.last_save.is_none_or(|at| at.elapsed() >= SAVE_EVERY) {
            self.journal_save();
        }
    }

    /// Journal relu du disque : il rejoint ce que la session a déjà gardé.
    pub(in crate::ui) fn journal_loaded(&mut self, user: u64, loaded: Option<Box<Journal>>) {
        let Some(loaded) = loaded else { return };
        if !self.journal.on || self.journal.user != Some(user) {
            return;
        }
        let dirty = self.journal.data.dirty;
        self.journal.data.merge(*loaded);
        self.journal.data.dirty = dirty;
        let days = plugins::number(Plugin::MessageLog, plugins::LOG_DAYS);
        self.journal.data.prune(crate::state::store::now_ms(), days);
        let chans: Vec<u64> = self.store.chans.keys().copied().filter(|c| self.journal.data.touches(*c)).collect();
        for c in chans {
            self.store.journal_restore(c, &self.journal.data);
        }
        self.ctx.request_repaint();
    }

    /// Réponses du fil des fichiers, pour le journal et la recherche locale.
    pub(in crate::ui) fn vault_receive(&mut self) {
        for e in self.journal.vault.poll() {
            match e.data {
                Loaded::Journal(j) => self.journal_loaded(e.user, j),
                Loaded::Index(i) => self.local_search_loaded(e.user, i),
                Loaded::Raw(kind, bytes) => self.e3_loaded(kind, e.user, bytes),
            }
        }
    }

    /// Aperçu hors ligne : plugin activé, et un exemple (un message supprimé,
    /// un message modifié aux versions dépliées) dans le salon ouvert.
    #[doc(hidden)]
    pub fn journal_preview(&mut self) {
        self.local.loaded = true;
        self.set_plugin(Plugin::MessageLog, true);
        self.journal.preview = true;
    }

    /// Pose l'exemple de l'aperçu ; faux tant que le salon n'est pas chargé.
    fn journal_preview_apply(&mut self) -> bool {
        let Some(c) = self.store.channel else { return false };
        let me = self.store.me.as_ref().map_or(0, |u| u.id);
        let Some(cs) = self.store.chans.get(&c).filter(|cs| cs.loaded) else { return false };
        let mut others = cs.msgs.iter().rev().filter(|m| m.author_id != me && m.system.is_none());
        let Some(edited) = others.find(|m| !m.content.is_empty()).map(|m| m.id) else { return false };
        let Some(deleted) = others.next().map(|m| m.id) else { return false };
        let now = crate::state::store::now_ms();
        for (ago, text) in [(3_600_000, "Rendez-vous à 14 h devant la gare."), (600_000, "Rendez-vous à 15 h devant la gare, finalement.")] {
            let v = self.journal.data.record_edit(c, edited, text, now - ago);
            self.store.journal_push_version(c, edited, &v);
        }
        let text = self.store.message_in_memory(c, edited).map(|(m, _)| m.content.clone()).unwrap_or_default();
        self.store.apply(Event::MessageUpdate { channel_id: c, id: edited, content: text });
        self.store.journal_toggle(c, edited);
        if let Some((m, raw)) = self.store.message_in_memory(c, deleted) {
            self.journal.data.record_delete(m, raw, now - 120_000, true);
            self.store.journal_mark_deleted(c, deleted, now - 120_000);
        }
        self.scroll_to_bottom = true;
        true
    }

    /// Fermeture de la fenêtre : dernier enregistrement, puis attente du fil.
    pub(in crate::ui) fn journal_exit(&mut self) {
        if self.journal.on && self.local.loaded {
            self.journal_save();
        }
        self.journal.vault.flush(EXIT_WAIT);
    }

    /// Section de la page « Données et confidentialité » : ce qui est gardé,
    /// et « Effacer le journal » (le reste se règle dans Paramètres > Plugins).
    pub(in crate::ui) fn journal_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        super::settings::section(ui, t, jt::LOG_SECTION.get(lang));
        let on = plugins::on(Plugin::MessageLog);
        if on {
            let (deleted, edited) = self.journal.data.counts();
            super::settings::note(ui, t, &jf::log_kept(lang, deleted, edited));
            if !self.journal.vault.attached() {
                super::settings::note(ui, t, jt::LOG_MEMORY_ONLY.get(lang));
            }
        } else {
            super::settings::note(ui, t, jt::LOG_OFF.get(lang));
        }
        let mut clear = false;
        super::appearance::row(ui, jt::LOG_CLEAR.get(lang), jt::LOG_CLEAR_TIP.get(lang), |ui| {
            clear = ui.button(jt::LOG_CLEAR.get(lang)).clicked();
        });
        if clear {
            self.journal_wipe();
            self.vault_release_if_idle();
            self.notify(jt::LOG_CLEARED);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::journal::tests_support::message;
    use crate::state::model::{Dm, User};
    use crate::ui::tests::offline_boot;
    use std::sync::{Arc, Mutex};

    const ME: u64 = 1;
    const C: u64 = 100;

    /// Réseau factice : connecté, pousse les événements du test.
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

    fn store() -> crate::state::Store {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        s.apply(Event::History { channel_id: C, before: None, messages: vec![message(10, C, "un"), message(11, C, "deux"), message(12, C, "trois")] });
        s
    }

    fn app(net: Net) -> App {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), store(), images));
        app.lang = Lang::Fr;
        app.local.loaded = true;
        app
    }

    fn push(net: &Net, app: &mut App, e: Event) {
        net.events.lock().unwrap().push(e);
        app.logic_pass();
    }

    fn ids(app: &App) -> Vec<(u64, bool)> {
        app.store.chans[&C].msgs.iter().map(|m| (m.id, m.meta.is_gone())).collect()
    }

    /// Plugin coupé (défaut) : la suppression retire le message, rien n'est gardé.
    #[test]
    fn disabled_plugin_keeps_nothing() {
        let net = Net::default();
        let mut app = app(net.clone());
        assert!(!plugins::on(Plugin::MessageLog), "coupé par défaut");
        push(&net, &mut app, Event::MessageUpdate { channel_id: C, id: 12, content: "trois !".into() });
        push(&net, &mut app, Event::MessageDelete { channel_id: C, id: 11 });
        assert_eq!(ids(&app), vec![(10, false), (12, false)]);
        assert!(app.store.chans[&C].msgs.iter().all(|m| m.meta.trace.is_none()));
        assert_eq!(app.journal_counts(), (0, 0));
        assert!(!app.journal.on && !app.journal.vault.running());
    }

    #[test]
    fn deleted_and_edited_messages_are_kept_and_shown() {
        let net = Net::default();
        let mut app = app(net.clone());
        app.set_plugin(Plugin::MessageLog, true);
        app.logic_pass();
        push(&net, &mut app, Event::MessageUpdate { channel_id: C, id: 12, content: "trois !".into() });
        push(&net, &mut app, Event::MessageDelete { channel_id: C, id: 11 });
        assert_eq!(ids(&app), vec![(10, false), (11, true), (12, false)], "le message supprimé reste, marqué");
        assert_eq!(app.journal_counts(), (1, 1));
        // Affichage : la mention « supprimé », puis les versions une fois dépliées.
        let out = crate::ui::tests::frame(&mut app, Vec::new());
        let shows = |out: &egui::FullOutput, text: &str| out.platform_output.accesskit_update.as_ref().unwrap().nodes.iter().any(|(_, n)| n.label().is_some_and(|l| l.contains(text)) || n.value().is_some_and(|v| v.contains(text)));
        assert!(shows(&out, jt::DELETED.get(Lang::Fr)), "mention « supprimé »");
        assert!(!shows(&out, jt::VERSIONS.get(Lang::Fr)), "versions repliées");
        ask_versions(&app.ctx, 12);
        let out = crate::ui::tests::frame(&mut app, Vec::new());
        assert!(shows(&out, jt::VERSIONS.get(Lang::Fr)) && shows(&out, "trois"));

        // Une page fraîche (sans le message supprimé) : le journal le remet.
        push(&net, &mut app, Event::History { channel_id: C, before: None, messages: vec![message(10, C, "un"), message(12, C, "trois !")] });
        assert_eq!(ids(&app), vec![(10, false), (11, true), (12, false)]);
        assert!(has_versions(&app.store.chans[&C].msgs[2]));

        // Mes messages et les suppressions que je demande ne sont pas gardés.
        let mut mine = message(13, C, "de moi");
        mine.author_id = ME;
        push(&net, &mut app, Event::MessageCreate(mine));
        push(&net, &mut app, Event::MessageDelete { channel_id: C, id: 13 });
        app.journal_outgoing(&[Cmd::Social(crate::state::social::SocialCmd::Delete { channel_id: C, message_id: 10 })]);
        push(&net, &mut app, Event::MessageDelete { channel_id: C, id: 10 });
        assert_eq!(ids(&app), vec![(11, true), (12, false)]);

        // « Effacer le journal », puis plugin coupé : plus rien.
        app.journal_wipe();
        assert_eq!(app.journal_counts(), (0, 0));
        assert_eq!(ids(&app), vec![(12, false)]);
        push(&net, &mut app, Event::MessageDelete { channel_id: C, id: 12 });
        assert_eq!(app.journal_counts(), (1, 0));
        app.set_plugin(Plugin::MessageLog, false);
        app.logic_pass();
        assert_eq!(app.journal_counts(), (0, 0));
        assert!(app.store.chans[&C].msgs.is_empty() && !app.journal.on);
    }

    /// Purge de modération : mille suppressions d'un coup, sans requête et sans blocage.
    #[test]
    fn bulk_delete_is_kept_without_slowing_down() {
        let net = Net::default();
        let mut app = app(net.clone());
        app.set_plugin(Plugin::MessageLog, true);
        app.logic_pass();
        let page: Vec<Message> = (0..400).map(|i| message(1000 + i, C, "x")).collect();
        push(&net, &mut app, Event::History { channel_id: C, before: None, messages: page });
        let at = Instant::now();
        net.events.lock().unwrap().extend((0..400).map(|i| Event::MessageDelete { channel_id: C, id: 1000 + i }));
        app.logic_pass();
        assert_eq!(app.journal_counts().0, 400);
        assert!(at.elapsed() < Duration::from_secs(2), "{:?}", at.elapsed());
        assert!(net.sent.lock().unwrap().iter().all(|c| !matches!(c, Cmd::Social(_) | Cmd::Moderation(_))), "le journal n'envoie rien");
    }

    /// Avec un disque : chiffré, relu à l'ouverture suivante ; coupé : effacé, fil arrêté.
    #[cfg(windows)]
    #[test]
    fn journal_is_saved_reloaded_and_erased_with_the_plugin() {
        use crate::account::AccountKey;
        use crate::settings::Account;
        let root = std::env::temp_dir().join(format!("ferrocord-test-journal-ui-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let listed = |app: &mut App| app.prefs.now.accounts = vec![Account { user_id: Some(ME), ..Account::new(AccountKey::of_user(ME)) }];

        let net = Net::default();
        let mut app = app(net.clone());
        listed(&mut app);
        // La recherche locale (activée par défaut) se sert du même fil : coupée ici.
        app.set_plugin(Plugin::LocalSearch, false);
        // De même les brouillons et les messages programmés (lot E3).
        app.set_plugin(Plugin::Drafts, false);
        app.set_plugin(Plugin::Scheduled, false);
        app.attach_vault(Vault::new(Some(root.clone()), Arc::new(|| {})));
        app.logic_pass();
        assert!(!root.exists() && !app.journal.vault.running(), "plugin coupé : ni fil ni fichier");
        app.set_plugin(Plugin::MessageLog, true);
        app.logic_pass();
        push(&net, &mut app, Event::MessageDelete { channel_id: C, id: 11 });
        app.journal_exit();
        let file = root.join("journal").join(format!("{ME}.bin"));
        let raw = std::fs::read(&file).unwrap();
        assert!(!raw.windows(4).any(|w| w == b"deux"), "chiffré au repos");

        // Nouvelle session : le journal revient, et le message reprend sa place.
        let net = Net::default();
        let mut next = self::app(net.clone());
        listed(&mut next);
        next.set_plugin(Plugin::LocalSearch, false);
        next.set_plugin(Plugin::Drafts, false);
        next.set_plugin(Plugin::Scheduled, false);
        next.attach_vault(Vault::new(Some(root.clone()), Arc::new(|| {})));
        next.set_plugin(Plugin::MessageLog, true);
        next.logic_pass();
        next.journal.vault.flush(Duration::from_secs(10));
        push(&net, &mut next, Event::History { channel_id: C, before: None, messages: vec![message(10, C, "un"), message(12, C, "trois")] });
        assert_eq!(next.journal_counts(), (1, 0));
        assert_eq!(ids(&next), vec![(10, false), (11, true), (12, false)]);

        next.set_plugin(Plugin::MessageLog, false);
        next.logic_pass();
        assert!(!next.journal.vault.running(), "fil rendu");
        // Le fil finit ce qui lui a été demandé avant de s'arrêter.
        for _ in 0..500 {
            if !root.join("journal").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!root.join("journal").exists(), "plugin coupé : journal effacé du disque");
        let _ = std::fs::remove_dir_all(&root);
    }
}
