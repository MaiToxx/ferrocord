//! Mots-clés surveillés, côté interface (plugin, lot E1) : décision à la
//! réception d'un message (`state::keywords`), notification par le chemin
//! des notifications de bureau, lignes de la boîte de réception, surlignage
//! dans le fil, éditeur de la liste (page Notifications) et exclusion d'un
//! salon (menu du salon).
//!
//! Plugin coupé : [`App::keywords_before`] rend la main à la première ligne,
//! la liste des messages n'est pas surlignée, rien n'est gardé. Aucune
//! requête dans tous les cas : seuls les messages reçus sont regardés.

use super::widgets::{self, Kind as Button};
use super::App;
use crate::i18n::table::journal::{f as jf, t as jt};
use crate::i18n::Lang;
use crate::plugins::{self, Id as Plugin};
use crate::state::keywords::{self as kw, Hit, KeywordPrefs, Matcher, Rule};
use crate::state::markdown::Doc;
use crate::state::model::Message;
use crate::state::notify::{self, Kind};
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Align, Layout, RichText};
use std::collections::VecDeque;

#[derive(Default)]
pub(in crate::ui) struct KeywordsUi {
    matcher: Matcher,
    /// Réglages dont `matcher` est tiré ; `None` : plugin coupé.
    built: Option<Box<KeywordPrefs>>,
    /// Messages reçus qui contenaient un mot (boîte de réception), du plus récent au plus ancien.
    hits: VecDeque<Hit>,
    // Éditeur de la page Notifications.
    draft: String,
    draft_whole: bool,
    draft_guild: Option<u64>,
}

/// Les règles à appliquer au fil, s'il y en a (plugin activé, liste non vide).
pub(super) fn active(k: &KeywordsUi) -> Option<&Matcher> {
    (k.built.is_some() && !k.matcher.is_empty()).then_some(&k.matcher)
}

/// Texte mis en forme du message, mots surveillés surlignés.
pub(super) fn marked(matcher: Option<&Matcher>, m: &Message, mut doc: Doc) -> Doc {
    if let Some(k) = matcher {
        k.mark(m.guild_id, m.channel_id, &mut doc);
    }
    doc
}

/// Ce qu'un message reçu demande au plugin, calculé avant que le magasin ne le range.
pub(in crate::ui) struct Pending {
    hit: Hit,
    alert: Option<notify::Alert>,
}

/// Ce qu'une ligne de la boîte de réception demande.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InboxPick {
    Jump { guild: Option<u64>, channel: u64, message: u64 },
    Clear,
}

/// Lignes « Mots-clés » de l'onglet « Pour vous » ; rien si la liste est vide.
pub(super) fn inbox_rows(ui: &mut egui::Ui, t: &Tokens, lang: Lang, k: &KeywordsUi) -> Option<InboxPick> {
    if k.hits.is_empty() {
        return None;
    }
    let mut pick = None;
    ui.add_space(t.spacing.item_y);
    ui.horizontal(|ui| {
        ui.label(RichText::new(jt::KW_INBOX.get(lang)).small().color(t.colors.faint));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if widgets::button(ui, t, Button::Link, jt::KW_CLEAR.get(lang)).clicked() {
                pick = Some(InboxPick::Clear);
            }
        });
    });
    for h in &k.hits {
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::button(ui, t, Button::Secondary, jt::KW_SEE.get(lang)).clicked() {
                    pick = Some(InboxPick::Jump { guild: h.guild, channel: h.channel, message: h.message });
                }
                let room = egui::vec2(ui.available_width(), ui.available_height());
                ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                    let side = Tokens::icon_side(t.typography.body);
                    let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
                    crate::theme::icons::paint(Icon::Search, ui.painter(), slot, t.colors.muted);
                    let mut job = egui::text::LayoutJob::default();
                    let body = egui::TextStyle::Body.resolve(ui.style());
                    job.append(&jf::kw_line(lang, &h.author, &h.word), 0.0, egui::TextFormat { font_id: body.clone(), color: t.colors.text, ..Default::default() });
                    job.append(&h.excerpt, t.spacing.item_x, egui::TextFormat { font_id: body, color: t.colors.muted, ..Default::default() });
                    ui.add(egui::Label::new(job).truncate().selectable(false));
                });
            });
        });
    }
    pick
}

impl App {
    /// Messages qui attendent dans la boîte de réception (mots-clés).
    pub fn keyword_hits(&self) -> usize {
        self.keywords.hits.len()
    }

    pub(in crate::ui) fn keywords_clear_hits(&mut self) {
        self.keywords.hits.clear();
    }

    /// À chaque passage de logique : les règles suivent les réglages. Plugin
    /// coupé et rien de gardé : une comparaison.
    pub(in crate::ui) fn keywords_tick(&mut self) {
        if !plugins::on(Plugin::Keywords) {
            if self.keywords.built.is_some() {
                let (draft, draft_whole, draft_guild) = (std::mem::take(&mut self.keywords.draft), self.keywords.draft_whole, self.keywords.draft_guild);
                self.keywords = KeywordsUi { draft, draft_whole, draft_guild, ..Default::default() };
                self.list.forget_docs();
                self.ctx.request_repaint();
            }
            return;
        }
        if self.keywords.built.as_deref() != Some(&*self.local.prefs.keywords) {
            self.keywords.matcher = Matcher::new(&self.local.prefs.keywords);
            self.keywords.built = Some(self.local.prefs.keywords.clone());
            // Le surlignage du fil est refait une fois.
            self.list.forget_docs();
            self.ctx.request_repaint();
        }
    }

    /// Avant que le magasin range `m` : contient-il un mot surveillé ?
    pub(in crate::ui) fn keywords_before(&self, m: &Message) -> Option<Pending> {
        let matcher = active(&self.keywords).filter(|_| plugins::on(Plugin::Keywords))?;
        let me = self.store.me.as_ref().map_or(0, |u| u.id);
        if m.author_id == me || m.system.is_some() {
            return None;
        }
        let word = matcher.find(m.guild_id, m.channel_id, &m.content)?.to_string();
        let facts = notify::facts(&self.store, m, self.window_focused());
        // Jamais pour une personne bloquée, ni pour le salon qu'on est en train de lire.
        if facts.blocked || facts.viewing {
            return None;
        }
        let (lang, h24) = (self.lang, self.h24());
        let excerpt = notify::truncate(&notify::body(&self.store, m, lang, h24), kw::EXCERPT);
        // « Ne pas déranger » : la ligne de la boîte de réception, sans notification.
        let alert = (!facts.dnd).then(|| {
            let mut a = notify::alert_of(&self.store, m, Kind::Mention, lang, h24);
            a.place = Some(match a.place.take() {
                Some(place) => jf::kw_place(lang, &word, place),
                None => jf::kw_word(lang, &word),
            });
            a
        });
        Some(Pending { hit: Hit { message: m.id, channel: m.channel_id, guild: m.guild_id, author: m.author.clone(), word, excerpt }, alert })
    }

    /// Après : ligne de la boîte de réception, et notification si le
    /// message n'en a pas déjà donné une (`already` : mention, MP).
    pub(in crate::ui) fn keywords_after(&mut self, p: Pending, already: bool) {
        if !self.keywords.hits.iter().any(|h| h.message == p.hit.message) {
            self.keywords.hits.push_front(p.hit);
            self.keywords.hits.truncate(kw::MAX_HITS);
        }
        if let Some(a) = p.alert
            && !already
        {
            self.desk_alert(a);
        }
    }

    fn keywords_edit(&mut self, change: impl FnOnce(&mut KeywordPrefs)) {
        let mut p = self.local.prefs.clone();
        change(&mut p.keywords);
        self.set_local_prefs(p);
    }

    /// Menu d'un salon ou d'un MP : « Ne pas y surveiller les mots-clés ».
    /// Vrai si une ligne a été dessinée.
    pub(in crate::ui) fn keywords_menu(&mut self, ui: &mut egui::Ui, channel: u64) -> bool {
        if !plugins::on(Plugin::Keywords) {
            return false;
        }
        let mut excluded = self.local.prefs.keywords.excluded.contains(&channel);
        let t = self.theme.clone();
        if super::menu::check(ui, &t, jt::KW_EXCLUDE.get(self.lang), &mut excluded) {
            self.keywords_edit(|k| k.set_excluded(channel, excluded));
        }
        true
    }

    /// Lignes des plugins locaux (lot E1) dans le menu d'un salon ou d'un MP.
    pub(in crate::ui) fn local_plugins_menu(&mut self, ui: &mut egui::Ui, channel: u64) {
        self.keywords_menu(ui, channel);
        self.export_menu(ui, channel);
    }

    fn keywords_guild_name(&self, guild: Option<u64>) -> String {
        let lang = self.lang;
        match guild {
            None => jt::KW_ANYWHERE.get(lang).to_string(),
            Some(g) => self.store.guilds.iter().find(|x| x.id == g).map_or_else(|| jt::KW_UNKNOWN_GUILD.get(lang).to_string(), |x| x.name.clone()),
        }
    }

    fn keywords_channel_name(&self, channel: u64) -> String {
        if let Some(d) = self.store.dms.iter().find(|d| d.id == channel) {
            return format!("@{}", d.name);
        }
        self.store.channel_name(channel).map_or_else(|| jt::KW_UNKNOWN_CHANNEL.get(self.lang).to_string(), |n| format!("#{n}"))
    }

    /// Section « Mots-clés surveillés » de la page Notifications.
    pub(in crate::ui) fn keywords_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        super::settings::section(ui, t, jt::KW_SECTION.get(lang));
        super::settings::note(ui, t, jt::KW_TIP.get(lang));
        if !plugins::on(Plugin::Keywords) {
            super::settings::note(ui, t, crate::i18n::table::plug::t::OFF_HERE.get(lang));
        }
        let prefs = self.local.prefs.keywords.clone();
        if prefs.rules.is_empty() {
            ui.label(RichText::new(jt::KW_NONE.get(lang)).color(t.colors.muted));
        }
        // Une ligne par règle : le mot, où il vaut, « Mot entier », retirer.
        let (mut remove, mut whole) = (None, None);
        for (i, r) in prefs.rules.iter().enumerate() {
            let scope = self.keywords_guild_name(r.guild);
            ui.push_id(("mot-cle", i), |ui| {
                super::appearance::row(ui, &r.text, &scope, |ui| {
                    let mut on = r.whole;
                    if ui.checkbox(&mut on, jt::KW_WHOLE.get(lang)).on_hover_text(jt::KW_WHOLE_TIP.get(lang)).changed() {
                        whole = Some((i, on));
                    }
                    if super::util::icon_button(ui, t, Icon::Trash, jt::KW_REMOVE.get(lang)).clicked() {
                        remove = Some(i);
                    }
                });
            });
        }
        // Ajout : le mot, où le surveiller, « Mot entier ».
        ui.add_space(t.spacing.item_y);
        let mut add = false;
        ui.horizontal(|ui| {
            let k = &mut self.keywords;
            let width = (ui.available_width() * 0.4).max(120.0);
            let field = ui.add(egui::TextEdit::singleline(&mut k.draft).hint_text(jt::KW_HINT.get(lang)).char_limit(kw::MAX_LEN).desired_width(width));
            super::a11y::name(ui.ctx(), field.id, jt::KW_HINT.get(lang));
            add = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let shown = match k.draft_guild {
                None => jt::KW_ANYWHERE.get(lang).to_string(),
                Some(g) => self.store.guilds.iter().find(|x| x.id == g).map_or_else(|| jt::KW_UNKNOWN_GUILD.get(lang).to_string(), |x| x.name.clone()),
            };
            let combo = egui::ComboBox::from_id_salt("mots-cles-portee").selected_text(shown).width(140.0).truncate().show_ui(ui, |ui| {
                ui.selectable_value(&mut k.draft_guild, None, jt::KW_ANYWHERE.get(lang));
                for g in &self.store.guilds {
                    ui.selectable_value(&mut k.draft_guild, Some(g.id), g.name.as_str());
                }
            });
            super::a11y::name(ui.ctx(), combo.response.id, jt::KW_SCOPE.get(lang));
            ui.checkbox(&mut k.draft_whole, jt::KW_WHOLE.get(lang)).on_hover_text(jt::KW_WHOLE_TIP.get(lang));
            let ready = !kw::clean(&k.draft).is_empty() && prefs.rules.len() < kw::MAX_RULES;
            add |= ui.add_enabled_ui(ready, |ui| widgets::button(ui, t, Button::Primary, jt::KW_ADD.get(lang))).inner.clicked();
        });
        // Salons exclus.
        let mut include = None;
        if !prefs.excluded.is_empty() {
            ui.add_space(t.spacing.item_y);
            ui.label(RichText::new(jt::KW_EXCLUDED.get(lang)).family(super::fonts::demi()).color(t.colors.text));
            for c in &prefs.excluded {
                let name = self.keywords_channel_name(*c);
                ui.push_id(("salon-exclu", *c), |ui| {
                    super::appearance::row(ui, &name, "", |ui| {
                        if super::util::icon_button(ui, t, Icon::Close, jt::KW_REMOVE.get(lang)).clicked() {
                            include = Some(*c);
                        }
                    });
                });
            }
        }
        super::settings::note(ui, t, jt::KW_EXCLUDED_TIP.get(lang));

        if let Some(i) = remove {
            self.keywords_edit(|k| {
                k.rules.remove(i);
            });
        }
        if let Some((i, on)) = whole {
            self.keywords_edit(|k| k.rules[i].whole = on);
        }
        if let Some(c) = include {
            self.keywords_edit(|k| k.set_excluded(c, false));
        }
        if add {
            let rule = Rule { text: std::mem::take(&mut self.keywords.draft), whole: self.keywords.draft_whole, guild: self.keywords.draft_guild };
            self.keywords_edit(|k| {
                k.add(rule);
            });
        }
    }

    /// Aperçu hors ligne : plugin activé, deux mots surveillés, et les
    /// messages du salon ouvert qui en contiennent rangés dans la boîte.
    #[doc(hidden)]
    pub fn keywords_preview(&mut self, words: &[&str]) {
        self.local.loaded = true;
        self.set_plugin(Plugin::Keywords, true);
        let rules: Vec<Rule> = words.iter().map(|w| Rule { text: (*w).to_string(), whole: false, guild: None }).collect();
        self.keywords_edit(|k| {
            for r in rules {
                k.add(r);
            }
        });
        self.keywords_tick();
        let Some(matcher) = active(&self.keywords) else { return };
        let hits: Vec<Hit> = self
            .store
            .chans
            .values()
            .flat_map(|cs| cs.msgs.iter())
            .filter_map(|m| {
                let word = matcher.find(m.guild_id, m.channel_id, &m.content)?.to_string();
                Some(Hit { message: m.id, channel: m.channel_id, guild: m.guild_id, author: m.author.clone(), word, excerpt: notify::truncate(&m.content, kw::EXCERPT) })
            })
            .take(4)
            .collect();
        self.keywords.hits.extend(hits);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::journal::tests_support::message;
    use crate::state::model::{Dm, User};
    use crate::state::{Cmd, Event};
    use crate::ui::tests::offline_boot;
    use std::sync::{Arc, Mutex};

    const ME: u64 = 1;
    const C: u64 = 100;
    const OTHER: u64 = 200;

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

    /// Bureau qui retient les notifications montrées.
    #[derive(Clone, Default)]
    struct Desk(Arc<Mutex<Vec<notify::Alert>>>);

    impl crate::desk::Desktop for Desk {
        fn load_prefs(&mut self) {}
        fn save_prefs(&mut self, _: notify::DeskPrefs) {}
        fn toast(&mut self, a: &notify::Alert) {
            self.0.lock().unwrap().push(a.clone());
        }
        fn sound(&mut self, _: Kind, _: u8) {}
        fn tray(&mut self, _: &crate::desk::TrayState) {}
        fn poll(&mut self) -> Vec<crate::desk::DeskEvent> {
            Vec::new()
        }
    }

    fn app(net: Net, desk: Desk) -> App {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0), Dm::new(OTHER, "Bob".into(), 0)] });
        s.select_dms();
        s.select_channel(C);
        s.apply(Event::History { channel_id: C, before: None, messages: vec![message(10, C, "vive ferrocord")] });
        // Les MP notifient toujours : le salon est muet pour ne voir que le plugin.
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), s, images));
        app.lang = Lang::Fr;
        app.local.loaded = true;
        app.attach_desktop(Box::new(desk));
        app
    }

    fn watch(app: &mut App, word: &str) {
        app.keywords_edit(|k| {
            k.add(Rule { text: word.into(), whole: false, guild: None });
        });
        app.logic_pass();
    }

    fn receive(net: &Net, app: &mut App, id: u64, chan: u64, text: &str) {
        net.events.lock().unwrap().push(Event::MessageCreate(message(id, chan, text)));
        app.logic_pass();
    }

    /// Plugin coupé : aucune ligne, aucun surlignage, rien de gardé.
    #[test]
    fn disabled_plugin_watches_nothing() {
        let (net, desk) = (Net::default(), Desk::default());
        let mut app = app(net.clone(), desk.clone());
        app.set_plugin(Plugin::Keywords, false);
        watch(&mut app, "ferrocord");
        receive(&net, &mut app, 20, OTHER, "encore ferrocord");
        assert_eq!(app.keyword_hits(), 0);
        assert!(active(&app.keywords).is_none() && app.keywords.built.is_none());
        assert!(desk.0.lock().unwrap().iter().all(|a| !a.place.as_deref().unwrap_or("").contains("ferrocord")), "pas de notification du plugin");
    }

    #[test]
    fn a_received_keyword_notifies_once_fills_the_inbox_and_is_marked() {
        let (net, desk) = (Net::default(), Desk::default());
        let mut app = app(net.clone(), desk.clone());
        assert!(plugins::on(Plugin::Keywords));
        watch(&mut app, "Ferrocord");
        // Un MP notifie déjà : le plugin ajoute la ligne, pas une seconde notification.
        receive(&net, &mut app, 20, OTHER, "tu connais FERROCORD ?");
        assert_eq!(app.keyword_hits(), 1);
        assert_eq!(desk.0.lock().unwrap().len(), 1, "une seule notification");
        let h = &app.keywords.hits[0];
        assert_eq!((h.message, h.channel, h.word.as_str(), h.author.as_str()), (20, OTHER, "ferrocord", "Alice"));
        // Sans le mot : rien. Mon propre message : rien.
        receive(&net, &mut app, 21, OTHER, "rien à voir");
        let mut mine = message(22, OTHER, "ferrocord, de moi");
        mine.author_id = ME;
        net.events.lock().unwrap().push(Event::MessageCreate(mine));
        app.logic_pass();
        assert_eq!(app.keyword_hits(), 1);
        // Salon exclu : rien.
        app.keywords_edit(|k| k.set_excluded(OTHER, true));
        app.logic_pass();
        receive(&net, &mut app, 23, OTHER, "ferrocord dans un salon exclu");
        assert_eq!(app.keyword_hits(), 1);
        // Surlignage dans le fil du salon ouvert.
        let m = app.store.chans[&C].msgs[0].clone();
        let doc = marked(active(&app.keywords), &m, crate::state::markdown::parse_message(&app.store, &m, Lang::Fr, true));
        assert!(doc.blocks[0].spans.iter().any(|s| s.style.mark && s.text == "ferrocord"));
        // La boîte de réception montre la ligne ; « Tout effacer » la vide.
        app.show_inbox_tab("pour-vous");
        let out = crate::ui::tests::frame(&mut app, Vec::new());
        assert!(crate::ui::tests::find(&out, jt::KW_SEE.get(Lang::Fr)).is_some(), "ligne dans « Pour vous »");
        crate::ui::tests::click(&mut app, jt::KW_CLEAR.get(Lang::Fr));
        assert_eq!(app.keyword_hits(), 0);
        assert!(net.sent.lock().unwrap().iter().all(|c| matches!(c, Cmd::History { .. } | Cmd::Relations(_))), "le plugin n'envoie rien : {:?}", net.sent.lock().unwrap());
    }

    /// Un salon de serveur réglé sur « mentions » ne notifie pas : le mot-clé, si.
    #[test]
    fn a_keyword_notifies_where_discord_would_not() {
        use crate::state::model::{Channel, ChannelKind, Guild};
        let (net, desk) = (Net::default(), Desk::default());
        let mut app = app(net.clone(), desk.clone());
        let chan = Channel { id: 300, name: "général".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        net.events.lock().unwrap().push(Event::GuildCreate(Guild::new(9, "Alpha".into(), vec![chan])));
        app.logic_pass();
        watch(&mut app, "pizza");
        let mut m = message(30, 300, "qui veut une pizza ?");
        m.guild_id = Some(9);
        net.events.lock().unwrap().push(Event::MessageCreate(m));
        app.logic_pass();
        let shown = desk.0.lock().unwrap();
        assert_eq!(shown.len(), 1);
        assert_eq!(shown[0].kind, Kind::Mention);
        assert!(shown[0].place.as_deref().is_some_and(|p| p.contains("pizza") && p.contains("#général")), "{:?}", shown[0].place);
        assert_eq!(app.keyword_hits(), 1);
    }
}
