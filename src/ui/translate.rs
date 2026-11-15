//! Plugin « Traduction d'un message » (lot E3). Clic droit sur un message →
//! « Traduire » ; la traduction s'affiche sous le message, avec « Afficher
//! l'original ».
//!
//! - Coupé par défaut. Activé, **aucun service n'est contacté** tant que
//!   l'utilisateur n'a pas, dans les réglages du plugin, saisi l'adresse
//!   d'un service compatible LibreTranslate et activé « J'ai lu
//!   l'avertissement » ([`App::translate_config`]). Aucun service n'est
//!   intégré d'office.
//! - Une requête par clic (`net::translate`), au plus [`MAX_FLIGHT`] à la
//!   fois, avec délai d'expiration ; jamais en masse ni automatiquement. Ce
//!   n'est pas une requête à Discord.
//! - Cache borné en mémoire (`state::translate::Cache`) : « Afficher
//!   l'original » puis « Traduire » ne redemande rien.
//! - Clé d'API facultative : gardée chiffrée (`offline::vault`, dossier
//!   `secrets`), jamais dans `local.toml`, jamais affichée ni journalisée.
//!
//! Plugin coupé : une comparaison par passage ; pas de ligne de menu, les
//! traductions affichées sont retirées, le cache et la clé quittent la mémoire.

use super::attachments::{Action, Hot};
use super::widgets::{self, Kind as Button};
use super::App;
use crate::i18n::table::e3::{f as xf, t as xt};
use crate::i18n::{Lang, Text};
use crate::net::translate::{self as net, Answer, Request};
use crate::offline::vault::{Kind, Payload};
use crate::plugins::{self, Id as Plugin, PluginPrefs};
use crate::state::model::Message;
use crate::state::translate::{self as model, Cache, Failure, Key, Shown, UrlError};
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText};
use std::sync::{mpsc, Arc};

/// Traductions en cours au plus (au-delà, le clic est refusé avec un mot).
pub const MAX_FLIGHT: usize = 2;
/// Longueur maximale de la clé d'API.
const MAX_KEY: usize = 200;

/// Demande venue du menu d'un message ou du lien sous une traduction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Ask {
    channel: u64,
    message: u64,
    /// Traduire ; faux : afficher l'original.
    on: bool,
}

/// Canal des réponses du service : (numéro de la demande, réponse).
type Link = (mpsc::Sender<(u64, Answer)>, mpsc::Receiver<(u64, Answer)>);

/// Traduction partie chez le service.
#[derive(Clone, Debug)]
struct Flight {
    channel: u64,
    message: u64,
    key: Key,
    target: String,
}

#[derive(Default)]
pub(in crate::ui) struct TranslateUi {
    on: bool,
    cache: Cache,
    /// Clé d'API du service, si l'utilisateur en a enregistré une.
    key: Option<String>,
    key_asked: bool,
    key_waiting: bool,
    /// Champ de saisie de la clé (jamais prérempli).
    key_input: String,
    link: Option<Link>,
    flights: Vec<(u64, Flight)>,
    next_tag: u64,
    /// Requêtes parties depuis le lancement (tests).
    requests: u32,
    /// Aperçu hors ligne : exemple à poser dès que le salon ouvert est chargé.
    preview: bool,
}

fn ask_id() -> Id {
    Id::unique("ferrocord.traduction.demande")
}

/// Menu d'un message : « Traduire » ou « Afficher l'original » (traité au
/// prochain passage par [`App::translate_tick`]).
pub(super) fn ask(ctx: &egui::Context, channel: u64, message: u64, on: bool) {
    ctx.data_mut(|d| d.insert_temp(ask_id(), Ask { channel, message, on }));
    ctx.request_repaint();
}

/// Ligne du menu d'un message : son texte, et ce que le clic demande.
/// `None` : plugin coupé, ou message sans texte.
pub(super) fn menu_entry(m: &Message) -> Option<(Text, bool)> {
    if !plugins::on(Plugin::Translate) || m.content.trim().is_empty() || m.system.is_some() {
        return None;
    }
    Some(if m.meta.translated.is_some() { (xt::TR_ORIGINAL, false) } else { (xt::TR_MENU, true) })
}

fn failure_text(f: Failure) -> Text {
    match f {
        Failure::Timeout => xt::TR_F_TIMEOUT,
        Failure::Network => xt::TR_F_NETWORK,
        Failure::Refused => xt::TR_F_REFUSED,
        Failure::Busy => xt::TR_F_BUSY,
        Failure::Service(_) => xt::TR_F_SERVICE,
        Failure::Answer => xt::TR_F_ANSWER,
    }
}

/// Sous le texte d'un message : sa traduction (ou son attente, ou l'échec),
/// et « Afficher l'original ». Rien pour un message non traduit. Même place
/// à la mesure et au dessin.
pub(super) fn under_ui<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &'a Message, hot: &mut Hot<'a>) {
    let Some(shown) = m.meta.translated.as_deref() else { return };
    let inner = egui::Frame::new().inner_margin(t.quote_indent()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match shown {
            Shown::Pending => {
                ui.label(RichText::new(xt::TR_PENDING.get(lang)).small().color(t.colors.muted));
            }
            Shown::Failed(f) => {
                ui.label(RichText::new(failure_text(*f).get(lang)).small().color(t.colors.danger));
            }
            Shown::Done { text, from, to } => {
                let head = match from {
                    Some(from) => xf::tr_head(lang, from.to_uppercase(), to.to_uppercase()),
                    None => xf::tr_head_to(lang, to.to_uppercase()),
                };
                ui.label(RichText::new(head).small().family(super::fonts::demi()).color(t.colors.muted));
                ui.add(egui::Label::new(RichText::new(text.as_str()).color(t.colors.text)).wrap());
            }
        }
        if !matches!(shown, Shown::Pending) {
            let link = ui.add(egui::Label::new(RichText::new(xt::TR_ORIGINAL.get(lang)).small().color(t.colors.link)).selectable(false));
            hot.add(link.rect, Action::Original(m.channel_id, m.id), None);
        }
    });
    let rect = inner.response.rect;
    let bar = egui::Rect::from_min_size(rect.min, egui::vec2(t.markdown.quote_width, rect.height()));
    ui.painter().rect_filled(bar, t.markdown.quote_width / 2.0, t.markdown.quote_bar);
}

/// Pourquoi rien ne peut être traduit pour l'instant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui) enum Setup {
    Off,
    /// L'avertissement n'a pas été lu.
    NotAcknowledged,
    Url(UrlError),
}

/// Service et langue cible d'après des réglages (ceux en vigueur, ou le
/// brouillon de la fenêtre de réglages).
fn config_of(prefs: &PluginPrefs, lang: Lang) -> Result<(String, String), Setup> {
    let endpoint = model::endpoint(prefs.text(Plugin::Translate, plugins::TR_URL)).map_err(Setup::Url)?;
    if !prefs.flag(Plugin::Translate, plugins::TR_ACK) {
        return Err(Setup::NotAcknowledged);
    }
    let target = match prefs.choice(Plugin::Translate, plugins::TR_TARGET) {
        plugins::TR_TARGET_UI => match lang {
            Lang::Fr => "fr",
            _ => "en",
        },
        code => code,
    };
    Ok((endpoint, target.to_string()))
}

impl App {
    /// Le plugin se sert du fil des fichiers (clé d'API).
    pub(in crate::ui) fn translate_uses_vault(&self) -> bool {
        self.translate.on
    }

    /// Requêtes de traduction parties depuis le lancement (tests).
    #[doc(hidden)]
    pub fn translate_requests(&self) -> u32 {
        self.translate.requests
    }

    /// Service choisi et avertissement lu : (adresse de la requête, langue cible).
    pub(in crate::ui) fn translate_config(&self) -> Result<(String, String), Setup> {
        if !plugins::on(Plugin::Translate) {
            return Err(Setup::Off);
        }
        config_of(&self.local.prefs.plugins, self.lang)
    }

    /// Clic sur « Traduire ». Service non choisi ou avertissement non lu :
    /// un mot d'explication, et rien ne part.
    fn translate_start(&mut self, channel: u64, message: u64) {
        let Some((m, _)) = self.store.message_in_memory(channel, message) else { return };
        let text: String = m.content.trim().chars().take(model::MAX_TEXT).collect();
        if text.is_empty() {
            return;
        }
        let (endpoint, target) = match self.translate_config() {
            Ok(c) => c,
            Err(Setup::Off) => return,
            Err(_) => return self.notify(xt::TR_SETUP),
        };
        let key = Key::new(message, &target, &text);
        if let Some((done, from)) = self.translate.cache.get(&key) {
            let shown = Shown::Done { text: done.clone(), from: from.clone(), to: target };
            self.store.translation_set(channel, message, Some(shown));
            return;
        }
        if self.translate.flights.iter().any(|(_, f)| f.key == key) {
            return;
        }
        if self.translate.flights.len() >= MAX_FLIGHT {
            return self.notify(xt::TR_WAIT);
        }
        let tx = self.translate.link.get_or_insert_with(mpsc::channel).0.clone();
        self.translate.next_tag += 1;
        let tag = self.translate.next_tag;
        let ctx = self.ctx.clone();
        let req = Request { endpoint, text, target: target.clone(), key: self.translate.key.clone() };
        if !net::spawn(req, tag, tx, Arc::new(move || ctx.request_repaint())) {
            self.store.translation_set(channel, message, Some(Shown::Failed(Failure::Network)));
            return;
        }
        self.translate.requests += 1;
        self.translate.flights.push((tag, Flight { channel, message, key, target }));
        self.store.translation_set(channel, message, Some(Shown::Pending));
    }

    /// Réponse d'un fil de traduction.
    fn translate_answer(&mut self, tag: u64, answer: Answer) {
        let Some(at) = self.translate.flights.iter().position(|(t, _)| *t == tag) else { return };
        let (_, f) = self.translate.flights.remove(at);
        let shown = match answer {
            Ok((text, from)) => {
                self.translate.cache.put(f.key, text.clone(), from.clone());
                Shown::Done { text, from, to: f.target }
            }
            Err(e) => Shown::Failed(e),
        };
        // « Afficher l'original » cliqué entre-temps : la réponse reste au cache, sans s'afficher.
        if self.store.message_in_memory(f.channel, f.message).is_some_and(|(m, _)| m.meta.translated.as_deref() == Some(&Shown::Pending)) {
            self.store.translation_set(f.channel, f.message, Some(shown));
        }
        self.ctx.request_repaint();
    }

    /// Clé d'API relue du disque.
    pub(in crate::ui) fn translate_key_loaded(&mut self, bytes: Option<Vec<u8>>) {
        if !self.translate.on {
            return;
        }
        self.translate.key_waiting = false;
        // Une clé saisie depuis le lancement l'emporte sur celle du disque.
        if self.translate.key.is_none() {
            self.translate.key = bytes.and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok()).and_then(|v| v.get("cle").and_then(|k| k.as_str()).map(str::to_string)).filter(|k| !k.is_empty() && k.chars().count() <= MAX_KEY);
        }
    }

    /// Enregistre (ou efface, `None`) la clé d'API : en mémoire, et chiffrée sur le disque.
    pub fn translate_set_key(&mut self, key: Option<&str>) {
        let key = key.map(|k| k.trim().chars().filter(|c| !c.is_control()).take(MAX_KEY).collect::<String>()).filter(|k| !k.is_empty());
        self.translate.key = key.clone();
        self.translate.key_waiting = false;
        if self.journal.vault.attached()
            && let Some(files) = self.journal.vault.files()
        {
            match key {
                Some(k) => files.save(0, Payload::Raw(Kind::Secrets, serde_json::json!({ "version": 1, "cle": k }).to_string().into_bytes())),
                None => files.purge(Kind::Secrets, 0),
            }
        }
    }

    /// Une clé d'API est enregistrée.
    pub fn translate_has_key(&self) -> bool {
        self.translate.key.is_some()
    }

    /// À chaque passage de logique. Plugin coupé : une comparaison, rien d'autre.
    pub(in crate::ui) fn translate_tick(&mut self) {
        let on = plugins::on(Plugin::Translate);
        if !on && !self.translate.on {
            return;
        }
        // `local.toml` pas encore lu : l'état du plugin n'est pas encore le vrai.
        if !self.local.loaded {
            return;
        }
        if !on {
            // Coupé : plus rien d'affiché ; cache et clé quittent la mémoire
            // (la clé reste chiffrée sur le disque pour une réactivation).
            self.store.translations_strip();
            self.ctx.data_mut(|d| d.remove_temp::<Ask>(ask_id()));
            self.translate = TranslateUi::default();
            self.vault_release_if_idle();
            self.ctx.request_repaint();
            return;
        }
        self.translate.on = true;
        if !self.translate.key_asked {
            self.translate.key_asked = true;
            if self.journal.vault.attached()
                && let Some(files) = self.journal.vault.files()
            {
                files.load(Kind::Secrets, 0);
                self.translate.key_waiting = true;
            }
        }
        if self.translate.key_waiting {
            self.vault_receive();
        }
        if self.translate.preview && self.translate_preview_apply() {
            self.translate.preview = false;
        }
        if let Some(a) = self.ctx.data_mut(|d| d.remove_temp::<Ask>(ask_id())) {
            if a.on {
                self.translate_start(a.channel, a.message);
            } else {
                self.store.translation_set(a.channel, a.message, None);
            }
        }
        if !self.translate.flights.is_empty() {
            let got: Vec<(u64, Answer)> = self.translate.link.as_ref().map(|(_, rx)| rx.try_iter().collect()).unwrap_or_default();
            for (tag, answer) in got {
                self.translate_answer(tag, answer);
            }
        }
    }

    /// Fenêtre de réglages du plugin, sous les réglages déclarés : état de
    /// l'adresse saisie (`draft` : les réglages en cours de modification) et
    /// clé d'API facultative.
    pub(in crate::ui) fn translate_settings_extra(&mut self, ui: &mut egui::Ui, t: &Tokens, draft: &PluginPrefs) {
        let lang = self.lang;
        ui.add_space(t.spacing.item_x);
        let (text, color) = match config_of(draft, lang) {
            Ok(_) => (xt::TR_READY, t.colors.online),
            Err(Setup::Url(UrlError::Empty)) | Err(Setup::Off) => (xt::TR_NO_SERVICE, t.colors.muted),
            Err(Setup::Url(UrlError::Scheme)) => (xt::TR_BAD_SCHEME, t.colors.danger),
            Err(Setup::Url(UrlError::Malformed)) => (xt::TR_BAD_URL, t.colors.danger),
            Err(Setup::NotAcknowledged) => (xt::TR_NEEDS_ACK, t.colors.idle),
        };
        ui.add(egui::Label::new(RichText::new(text.get(lang)).color(color)).wrap());
        ui.add_space(t.spacing.item_x);
        widgets::field_label(ui, t, xt::TR_KEY.get(lang));
        let size = egui::vec2(ui.available_width(), t.field_height());
        let edit = egui::TextEdit::singleline(&mut self.translate.key_input).password(true).char_limit(MAX_KEY).hint_text(RichText::new(if self.translate.key.is_some() { xt::TR_KEY_KEPT.get(lang) } else { xt::TR_KEY_HINT.get(lang) }).color(t.colors.faint));
        let resp = ui.add_sized(size, edit);
        super::a11y::name(ui.ctx(), resp.id, xt::TR_KEY.get(lang));
        super::settings::note(ui, t, xt::TR_KEY_TIP.get(lang));
        ui.add_space(t.spacing.item_y);
        let (mut save, mut erase) = (false, false);
        ui.horizontal(|ui| {
            save = ui.add_enabled_ui(!self.translate.key_input.trim().is_empty(), |ui| widgets::button(ui, t, Button::Secondary, xt::TR_KEY_SAVE.get(lang))).inner.clicked();
            erase = ui.add_enabled_ui(self.translate.key.is_some(), |ui| widgets::button(ui, t, Button::Link, xt::TR_KEY_ERASE.get(lang))).inner.clicked();
        });
        if save {
            let k = std::mem::take(&mut self.translate.key_input);
            self.translate_set_key(Some(&k));
        }
        if erase {
            self.translate.key_input.clear();
            self.translate_set_key(None);
        }
    }

    /// Aperçu hors ligne : plugin activé, et le dernier message d'un autre
    /// que moi montré traduit (sans aucune requête).
    #[doc(hidden)]
    pub fn translate_preview(&mut self) {
        self.local.loaded = true;
        self.set_plugin(Plugin::Translate, true);
        self.translate.preview = true;
    }

    /// Aperçu hors ligne : la fenêtre de réglages du plugin, une adresse
    /// saisie, l'avertissement pas encore lu.
    #[doc(hidden)]
    pub fn translate_preview_settings(&mut self) {
        self.local.loaded = true;
        let mut p = self.local.prefs.clone();
        p.plugins.set(Plugin::Translate, true);
        p.plugins.put(Plugin::Translate, plugins::TR_URL, plugins::Value::Str("https://traduction.exemple.org".into()));
        self.set_local_prefs(p);
        self.open_settings("plugins");
        self.plugin_settings_open(Plugin::Translate);
    }

    /// Pose l'exemple de l'aperçu ; faux tant que le salon n'est pas chargé.
    fn translate_preview_apply(&mut self) -> bool {
        let Some(c) = self.store.channel else { return false };
        let me = self.store.me.as_ref().map_or(0, |u| u.id);
        let Some(cs) = self.store.chans.get(&c).filter(|cs| cs.loaded) else { return false };
        let Some(id) = cs.msgs.iter().rev().find(|m| m.author_id != me && m.system.is_none() && !m.content.trim().is_empty()).map(|m| m.id) else { return false };
        let (text, from, to) = match self.lang {
            Lang::Fr => ("Voici la traduction de ce message, affichée sous l'original.", "en", "fr"),
            _ => ("Here is the translation of this message, shown under the original.", "fr", "en"),
        };
        self.store.translation_set(c, id, Some(Shown::Done { text: text.into(), from: Some(from.into()), to: to.into() }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::translate::tests::serve;
    use crate::plugins::Value;
    use crate::state::journal::tests_support::message;
    use crate::state::model::{Dm, User};
    use crate::state::{Cmd, Event};
    use crate::ui::tests::{frame, offline_boot};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};

    const C: u64 = 100;

    #[derive(Clone, Default)]
    struct Net {
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
            Vec::new()
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
        s.apply(Event::History { channel_id: C, before: None, messages: vec![message(10, C, "Bonjour à tous"), message(11, C, "Deuxième message"), message(12, C, "Troisième"), message(13, C, "Quatrième")] });
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), s, images));
        app.lang = Lang::Fr;
        app.local.loaded = true;
        app
    }

    fn set(app: &mut App, key: &str, v: Value) {
        let mut p = app.local.prefs.clone();
        p.plugins.put(Plugin::Translate, key, v);
        app.set_local_prefs(p);
    }

    fn shown(app: &App, id: u64) -> Option<Shown> {
        app.store.message_in_memory(C, id).and_then(|(m, _)| m.meta.translated.as_deref().cloned())
    }

    /// Fait tourner l'application jusqu'à ce que `done` soit vrai (au plus 20 s).
    fn until(app: &mut App, done: impl Fn(&App) -> bool) {
        let start = Instant::now();
        while !done(app) && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(10));
            frame(app, Vec::new());
        }
        assert!(done(app), "délai dépassé");
    }

    #[test]
    fn nothing_is_contacted_before_a_service_is_chosen_and_the_warning_read() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let base = rt.block_on(serve("200 OK", r#"{"translatedText":"Hello everyone","detectedLanguage":{"language":"fr"}}"#, Duration::ZERO, seen.clone()));
        let net = Net::default();
        let mut app = app(net.clone());
        frame(&mut app, Vec::new());
        // Coupé par défaut : pas de ligne de menu, la demande est sans effet.
        assert!(!plugins::on(Plugin::Translate));
        let msg = app.store.message_in_memory(C, 10).unwrap().0.clone();
        assert_eq!(menu_entry(&msg), None);
        ask(&app.ctx, C, 10, true);
        frame(&mut app, Vec::new());
        assert_eq!((shown(&app, 10), app.translate_requests()), (None, 0));
        // Activé sans service : un mot d'explication, aucune requête.
        app.set_plugin(Plugin::Translate, true);
        assert_eq!(menu_entry(&msg), Some((xt::TR_MENU, true)));
        assert_eq!(app.translate_config(), Err(Setup::Url(UrlError::Empty)));
        ask(&app.ctx, C, 10, true);
        frame(&mut app, Vec::new());
        assert_eq!((shown(&app, 10), app.translate_requests()), (None, 0));
        assert!(app.notice().is_some(), "l'utilisateur est renvoyé aux réglages");
        // Service saisi, avertissement pas lu : toujours rien.
        set(&mut app, plugins::TR_URL, Value::Str(base.clone()));
        assert_eq!(app.translate_config(), Err(Setup::NotAcknowledged));
        ask(&app.ctx, C, 10, true);
        frame(&mut app, Vec::new());
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!((shown(&app, 10), app.translate_requests(), seen.lock().unwrap().len()), (None, 0, 0));
        // Avertissement lu : le clic envoie une requête, à ce service.
        set(&mut app, plugins::TR_ACK, Value::Bool(true));
        set(&mut app, plugins::TR_TARGET, Value::Str("en".into()));
        frame(&mut app, Vec::new());
        assert_eq!(seen.lock().unwrap().len(), 0, "activer ne traduit rien tout seul");
        app.translate_set_key(Some("cle-secrete"));
        ask(&app.ctx, C, 10, true);
        frame(&mut app, Vec::new());
        assert_eq!(shown(&app, 10), Some(Shown::Pending));
        until(&mut app, |a| shown(a, 10) != Some(Shown::Pending));
        assert_eq!(shown(&app, 10), Some(Shown::Done { text: "Hello everyone".into(), from: Some("fr".into()), to: "en".into() }));
        let reqs = seen.lock().unwrap().clone();
        assert_eq!((reqs.len(), app.translate_requests()), (1, 1));
        assert!(reqs[0].contains("Bonjour à tous") && reqs[0].contains("cle-secrete") && reqs[0].contains("\"target\":\"en\""), "{}", reqs[0]);
        let msg = app.store.message_in_memory(C, 10).unwrap().0.clone();
        assert_eq!(menu_entry(&msg), Some((xt::TR_ORIGINAL, false)));
        // « Afficher l'original », puis « Traduire » : le cache répond, sans requête.
        ask(&app.ctx, C, 10, false);
        frame(&mut app, Vec::new());
        assert_eq!(shown(&app, 10), None);
        ask(&app.ctx, C, 10, true);
        frame(&mut app, Vec::new());
        assert!(matches!(shown(&app, 10), Some(Shown::Done { .. })));
        assert_eq!((seen.lock().unwrap().len(), app.translate_requests()), (1, 1), "cache local");
        // Rien de tout cela n'est une commande pour Discord.
        assert!(net.sent.lock().unwrap().iter().all(|c| !matches!(c, Cmd::Send { .. })));
        // Plugin coupé : les traductions affichées partent, le cache et la clé aussi.
        app.set_plugin(Plugin::Translate, false);
        frame(&mut app, Vec::new());
        assert_eq!(shown(&app, 10), None);
        assert!(!app.translate.on && app.translate.cache.is_empty() && !app.translate_has_key());
    }

    #[test]
    fn failures_show_under_the_message_and_requests_are_bounded() {
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(1).enable_all().build().unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        // Service lent : les deux premières demandes occupent les deux places.
        let slow = rt.block_on(serve("403 Forbidden", r#"{"error":"Invalid API key"}"#, Duration::from_millis(600), seen.clone()));
        let mut app = app(Net::default());
        frame(&mut app, Vec::new());
        app.set_plugin(Plugin::Translate, true);
        set(&mut app, plugins::TR_URL, Value::Str(slow));
        set(&mut app, plugins::TR_ACK, Value::Bool(true));
        for id in [10, 11, 12, 10] {
            ask(&app.ctx, C, id, true);
            frame(&mut app, Vec::new());
        }
        assert_eq!(app.translate_requests(), MAX_FLIGHT as u32, "au plus deux à la fois, jamais deux fois la même");
        assert_eq!((shown(&app, 10), shown(&app, 11), shown(&app, 12)), (Some(Shown::Pending), Some(Shown::Pending), None));
        until(&mut app, |a| a.translate.flights.is_empty());
        assert_eq!(shown(&app, 10), Some(Shown::Failed(Failure::Refused)));
        assert_eq!(shown(&app, 11), Some(Shown::Failed(Failure::Refused)));
        assert_eq!(seen.lock().unwrap().len(), 2);
        // Un échec n'est pas gardé au cache : un nouveau clic redemande.
        ask(&app.ctx, C, 10, true);
        frame(&mut app, Vec::new());
        assert_eq!(app.translate_requests(), 3);
        until(&mut app, |a| a.translate.flights.is_empty());
    }

    #[test]
    fn config_follows_the_settings() {
        let mut p = PluginPrefs::default();
        assert_eq!(config_of(&p, Lang::Fr), Err(Setup::Url(UrlError::Empty)));
        p.put(Plugin::Translate, plugins::TR_URL, Value::Str("http://traduction.exemple.org".into()));
        p.put(Plugin::Translate, plugins::TR_ACK, Value::Bool(true));
        assert_eq!(config_of(&p, Lang::Fr), Err(Setup::Url(UrlError::Scheme)), "en clair hors du réseau local");
        p.put(Plugin::Translate, plugins::TR_URL, Value::Str("https://traduction.exemple.org/".into()));
        assert_eq!(config_of(&p, Lang::Fr), Ok(("https://traduction.exemple.org/translate".into(), "fr".into())), "langue de l'interface");
        assert_eq!(config_of(&p, Lang::En).map(|c| c.1), Ok("en".into()));
        p.put(Plugin::Translate, plugins::TR_TARGET, Value::Str("de".into()));
        assert_eq!(config_of(&p, Lang::Fr).map(|c| c.1), Ok("de".into()));
        p.put(Plugin::Translate, plugins::TR_ACK, Value::Bool(false));
        assert_eq!(config_of(&p, Lang::Fr), Err(Setup::NotAcknowledged));
    }
}
