//! Raccourcis clavier (lot navigation) : déclenchement, historique
//! précédent / suivant (Alt+←/→, boutons 4 et 5 de la souris), fenêtre
//! « Raccourcis clavier » (Ctrl+/) et page des Paramètres qui les réassigne
//! (`keys.toml`, lu et écrit par `desk::keys` dans son fil).
//!
//! Une combinaison avec Ctrl ou Alt est prise au début de l'image, avant que
//! les champs de texte ne la voient ; une touche seule (Échap, Page haut…)
//! à la fin, si personne ne l'a prise (Échap ferme d'abord ce qui est ouvert).
//! Une touche qui écrirait dans un champ ne se déclenche pas pendant la saisie.

use super::{settings, App};
use crate::desk::keys::{KeysFileEvent, KeysStore};
use crate::i18n::table::lotn::t as xt;
use crate::i18n::table::nav::{f as nf, t as nt};
use crate::i18n::{Lang, Text};
use crate::state::keys::{Action, Bindings, Chord};
use crate::state::nav::{self, History, Place, Target};
use crate::theme::Tokens;
use eframe::egui::{self, Id, Key, RichText};

/// État de la navigation au clavier, des fenêtres du lot et de l'historique.
#[derive(Default)]
pub(super) struct NavUi {
    pub(super) bindings: Bindings,
    file: Option<Box<dyn KeysStore>>,
    /// Un raccourci a été changé avant la lecture du fichier : la lecture
    /// ne l'écrase pas.
    touched: bool,
    /// Combinaison attendue pour cette action (page des réglages).
    pub(super) capturing: Option<Action>,
    pub(super) history: History,
    pub(super) shortcuts_open: bool,
    pub(super) switcher: super::quick_switcher::Switcher,
    pub(super) inbox: super::inbox::InboxUi,
    /// Fenêtre « Paramètres de notification » d'un serveur (lot N).
    pub(super) notif_win: Option<super::notif_settings::Win>,
    /// Dernière action de navigation acceptée, pour ralentir la répétition (lot « fiable »).
    last_repeat: Option<(Action, std::time::Instant)>,
}

/// Libellé d'une action.
pub(super) fn action_label(a: Action) -> Text {
    match a {
        Action::QuickSwitcher => nt::ACT_QUICK_SWITCHER,
        Action::Inbox => nt::ACT_INBOX,
        Action::Shortcuts => nt::ACT_SHORTCUTS,
        Action::Settings => nt::ACT_SETTINGS,
        Action::PrevChannel => nt::ACT_PREV_CHANNEL,
        Action::NextChannel => nt::ACT_NEXT_CHANNEL,
        Action::PrevUnread => nt::ACT_PREV_UNREAD,
        Action::NextUnread => nt::ACT_NEXT_UNREAD,
        Action::PrevGuild => nt::ACT_PREV_GUILD,
        Action::NextGuild => nt::ACT_NEXT_GUILD,
        Action::Back => nt::ACT_BACK,
        Action::Forward => nt::ACT_FORWARD,
        Action::MarkRead => nt::ACT_MARK_READ,
        Action::MarkGuildRead => nt::ACT_MARK_GUILD_READ,
        Action::ReplyLast => nt::ACT_REPLY_LAST,
        Action::PageUp => nt::ACT_PAGE_UP,
        Action::PageDown => nt::ACT_PAGE_DOWN,
        Action::Emojis => nt::ACT_EMOJIS,
        Action::Pins => nt::ACT_PINS,
        Action::Members => nt::ACT_MEMBERS,
        Action::ToggleMute => crate::i18n::table::voice::t::ACT_MUTE,
        Action::ToggleDeafen => crate::i18n::table::voice::t::ACT_DEAFEN,
        Action::PushToTalk => crate::i18n::table::voice::t::ACT_PTT,
        Action::MsgReply => crate::i18n::table::a11y::t::ACT_MSG_REPLY,
        Action::MsgEdit => crate::i18n::table::a11y::t::ACT_MSG_EDIT,
        Action::MsgReact => crate::i18n::table::a11y::t::ACT_MSG_REACT,
        Action::MsgPin => crate::i18n::table::a11y::t::ACT_MSG_PIN,
        Action::MsgCopy => crate::i18n::table::a11y::t::ACT_MSG_COPY,
        Action::MsgDelete => crate::i18n::table::a11y::t::ACT_MSG_DELETE,
        Action::Search => xt::ACT_SEARCH,
        Action::Upload => xt::ACT_UPLOAD,
        Action::Gifs => xt::ACT_GIFS,
        Action::Stickers => xt::ACT_STICKERS,
        Action::CreateGuild => xt::ACT_CREATE_GUILD,
        Action::Threads => xt::ACT_THREADS,
        Action::Help => xt::ACT_HELP,
        Action::OldestUnread => xt::ACT_OLDEST_UNREAD,
        Action::AnswerCall => xt::ACT_ANSWER_CALL,
        Action::DeclineCall => xt::ACT_DECLINE_CALL,
        Action::StartCall => xt::ACT_START_CALL,
        Action::CreateGroup => crate::i18n::table::restes::t::ACT_CREATE_GROUP,
    }
}

/// Combinaison écrite pour l'utilisateur : « Ctrl+Maj+A », « Alt+↑ ».
pub(super) fn chord_label(lang: Lang, c: &Chord) -> String {
    let mut s = String::new();
    for (on, t) in [(c.ctrl, nt::MOD_CTRL), (c.alt, nt::MOD_ALT), (c.shift, nt::MOD_SHIFT)] {
        if on {
            s.push_str(t.get(lang));
            s.push('+');
        }
    }
    let key = match c.key.as_str() {
        "Escape" => nt::KEY_ESCAPE.get(lang),
        "PageUp" => nt::KEY_PAGE_UP.get(lang),
        "PageDown" => nt::KEY_PAGE_DOWN.get(lang),
        "Enter" => nt::KEY_ENTER.get(lang),
        "Space" => nt::KEY_SPACE.get(lang),
        "Backspace" => nt::KEY_BACKSPACE.get(lang),
        "Delete" => nt::KEY_DELETE.get(lang),
        "Home" => nt::KEY_HOME.get(lang),
        "End" => nt::KEY_END.get(lang),
        "Tab" => nt::KEY_TAB.get(lang),
        "Up" => "\u{2191}",
        "Down" => "\u{2193}",
        "Left" => "\u{2190}",
        "Right" => "\u{2192}",
        other => Key::from_name(other).map_or(other, |k| k.symbol_or_name()),
    };
    s.push_str(key);
    s
}

/// Libellé d'une action, avec sa combinaison s'il y en a une (infobulles).
pub(super) fn with_chord(lang: Lang, b: &Bindings, a: Action, label: &str) -> String {
    match b.get(a) {
        Some(c) => nf::with_chord(lang, label, chord_label(lang, c)),
        None => label.to_string(),
    }
}

/// Combinaison d'une touche pressée (Ctrl et Commande confondues).
pub(super) fn chord_of(key: Key, m: egui::Modifiers) -> Chord {
    Chord { ctrl: m.ctrl || m.command, alt: m.alt, shift: m.shift, key: key.name().to_string() }
}

/// Contour de l'élément qui a le clavier (jeton `focus`).
pub(super) fn focus_ring(ui: &egui::Ui, t: &Tokens, resp: &egui::Response) {
    if resp.has_focus() {
        ui.painter().rect_stroke(resp.rect, t.r_row(), egui::Stroke::new(t.shapes.focus, t.colors.focus), egui::StrokeKind::Inside);
    }
}

/// Centre d'aide de Discord (Ctrl+Maj+H), ouvert dans le navigateur.
const HELP_URL: &str = "https://support.discord.com/";

/// Raccourcis fixes (non réglables), montrés à la suite de la liste :
/// combinaison écrite telle quelle, et ce qu'elle fait.
pub(super) const FIXED: [(&str, Text); 6] = [
    ("Ctrl+1 {2026} 9", xt::FIX_GUILD_N),
    ("Ctrl+B", xt::FIX_BOLD),
    ("Ctrl+I", xt::FIX_ITALIC),
    ("Ctrl+U", xt::FIX_UNDERLINE),
    ("F6", xt::FIX_ZONES),
    ("Tab", xt::FIX_FOCUS),
];

/// Écart minimal entre deux répétitions d'une action qui ouvre un salon
/// (lot « fiable ») : touche maintenue, la répétition du système (une
/// trentaine par seconde) ferait partir un chargement d'historique par
/// salon traversé.
const NAV_REPEAT_GAP: std::time::Duration = std::time::Duration::from_millis(250);

/// Action qui ouvre un autre salon (donc charge son historique).
fn opens_a_channel(a: Action) -> bool {
    matches!(a, Action::PrevChannel | Action::NextChannel | Action::PrevUnread | Action::NextUnread | Action::PrevGuild | Action::NextGuild)
}

/// Actions qui se répètent quand la touche reste enfoncée.
fn repeats(a: Action) -> bool {
    matches!(a, Action::PrevChannel | Action::NextChannel | Action::PrevUnread | Action::NextUnread | Action::PrevGuild | Action::NextGuild | Action::PageUp | Action::PageDown)
}

impl App {
    /// Branche `keys.toml` (lu dans son fil) ; sans lui, les combinaisons
    /// par défaut valent et rien n'est écrit.
    pub fn attach_keys(&mut self, mut file: Box<dyn KeysStore>) {
        file.load();
        self.nav.file = Some(file);
    }

    /// Ouvre la fenêtre « Raccourcis clavier » (aperçu).
    pub fn open_shortcuts(&mut self) {
        self.nav.shortcuts_open = true;
    }

    /// Combinaison d'une action (tests, aperçu).
    pub fn shortcut(&self, a: Action) -> Option<&Chord> {
        self.nav.bindings.get(a)
    }

    /// Change une combinaison comme la page des réglages (tests).
    pub fn set_shortcut(&mut self, a: Action, c: Option<Chord>) {
        self.nav.bindings.set(a, c);
        self.save_keys();
    }

    fn save_keys(&mut self) {
        self.nav.touched = true;
        if let Some(f) = self.nav.file.as_mut() {
            f.save(self.nav.bindings.to_toml());
        }
    }

    /// Nouvelles du fil de `keys.toml`.
    pub(super) fn keys_tick(&mut self) {
        let Some(f) = self.nav.file.as_mut() else { return };
        for e in f.poll() {
            match e {
                KeysFileEvent::Loaded(text) => {
                    if self.nav.touched {
                        continue;
                    }
                    let (b, bad) = Bindings::parse(text.as_deref().unwrap_or(""));
                    self.nav.bindings = b;
                    if let Some((line, _)) = bad.first() {
                        self.notify(nf::keys_bad_line(self.lang, line));
                    }
                }
                KeysFileEvent::ReadFailed(why) => self.notify(nf::keys_read_failed(self.lang, why)),
                KeysFileEvent::SaveFailed(why) => self.notify(nf::keys_save_failed(self.lang, why)),
            }
        }
    }

    /// Endroit affiché à cette image (historique, destinations récentes).
    pub(super) fn nav_observe(&mut self) {
        let place = Place { guild: self.store.guild, channel: self.store.channel };
        self.nav.history.observe(place);
    }

    /// Ouvre un endroit (historique, sélecteur, boîte de réception).
    pub(super) fn go(&mut self, p: Place) {
        match (p.guild, p.channel) {
            (Some(g), Some(c)) => {
                self.store.guild = Some(g);
                self.open_channel(c);
            }
            (Some(g), None) => {
                if self.store.guild != Some(g) && self.store.select_guild(g) {
                    self.scroll_to_bottom = true;
                }
            }
            (None, Some(c)) => {
                self.store.select_dms();
                self.open_channel(c);
            }
            (None, None) => self.store.select_dms(),
        }
        self.ctx.request_repaint();
    }

    /// Ouvre la destination d'une ligne du sélecteur.
    pub(super) fn go_target(&mut self, t: Target) {
        match t {
            Target::Channel { guild, id } => self.go(Place { guild, channel: Some(id) }),
            Target::Guild(g) => self.go(Place { guild: Some(g), channel: None }),
            Target::Friend(u) => {
                self.store.select_dms();
                if self.store.open_dm_with(u) {
                    self.scroll_to_bottom = true;
                }
            }
        }
    }

    /// L'endroit existe encore (salon ou serveur pas supprimé entre-temps).
    pub(super) fn place_exists(&self, p: &Place) -> bool {
        p.guild.is_none_or(|g| self.store.guilds.iter().any(|x| x.id == g)) && p.channel.is_none_or(|c| self.store.channel_exists(c) || self.store.voice.channel(c).is_some())
    }

    pub(super) fn step_history(&mut self, forward: bool) {
        loop {
            let to = if forward { self.nav.history.forward() } else { self.nav.history.back() };
            let Some(to) = to else { return };
            if self.place_exists(&to) {
                self.go(to);
                return;
            }
        }
    }

    /// Prend les touches qui déclenchent une action dans `phase` (début :
    /// avec Ctrl ou Alt ; fin : touches seules) ; renvoie les actions.
    fn take_shortcuts(&mut self, ui: &egui::Ui, early: bool) -> Vec<Action> {
        // Combinaison attendue par la page des réglages : elle prend les touches.
        if self.nav.capturing.is_some() {
            if self.params.open && self.params.page() == settings::Page::Keybinds {
                return Vec::new();
            }
            self.nav.capturing = None;
        }
        let typing = ui.ctx().text_edit_focused();
        let b = &self.nav.bindings;
        let ringing = !self.ringing_calls().is_empty();
        let mut out = Vec::new();
        let now = std::time::Instant::now();
        let mut last_repeat = self.nav.last_repeat;
        ui.input_mut(|i| {
            i.events.retain(|e| {
                let egui::Event::Key { key, pressed: true, repeat, modifiers, .. } = e else { return true };
                let chord = chord_of(*key, *modifiers);
                // Maj+Échap aussi au début : les fenêtres et champs prennent
                // Échap en ignorant Maj (`consume_key` d'egui).
                let first = chord.ctrl || chord.alt || (chord.shift && chord.key == "Escape");
                if first != early {
                    return true;
                }
                // Les réponses à un appel ne valent que pendant qu'il sonne.
                let Some(a) = b.actions_of(&chord).find(|a| ringing || !a.on_ring()) else { return true };
                // Gestes sur le message sélectionné : pris par `msg_select`, jamais ici.
                if a.on_message() {
                    return true;
                }
                if (typing && chord.disturbs_typing()) || (*repeat && !repeats(a)) {
                    return true;
                }
                // Touche maintenue sur une action qui change de salon : répétition ralentie.
                if opens_a_channel(a) {
                    if *repeat && last_repeat.is_some_and(|(same, t)| same == a && now.duration_since(t) < NAV_REPEAT_GAP) {
                        return false;
                    }
                    last_repeat = Some((a, now));
                }
                out.push(a);
                false
            });
        });
        self.nav.last_repeat = last_repeat;
        out
    }

    /// Actions que cette touche déclencherait (tests).
    #[cfg(test)]
    fn take_shortcuts_for_test(&mut self, k: Key, m: egui::Modifiers) -> Vec<Action> {
        let ctx = self.ctx.clone();
        let mut got = Vec::new();
        let raw = egui::RawInput { events: vec![egui::Event::Key { key: k, physical_key: None, pressed: true, repeat: false, modifiers: m }], ..Default::default() };
        let _ = ctx.run_ui(raw, |ui| {
            let early = m.ctrl || m.alt;
            got = self.take_shortcuts(ui, early);
        });
        got
    }

    /// Début de l'image : combinaisons avec Ctrl ou Alt, et boutons 4 et 5
    /// de la souris. Seul « Paramètres » vaut hors de la session.
    pub(super) fn shortcuts_early(&mut self, ui: &egui::Ui) {
        let main = self.online();
        // Avant les raccourcis : Ctrl+I et Ctrl+U mettent en forme la sélection de la saisie.
        if main {
            self.format_keys(ui);
        }
        // Palette de commandes (lot E2) : Ctrl+Maj+P.
        self.palette_keys(ui);
        // Zoom du salon affiché (lot E2) : Ctrl+=, Ctrl+-, Ctrl+0, avant le zoom de la fenêtre.
        if main {
            self.zoom_keys(ui);
        }
        let actions = self.take_shortcuts(ui, true);
        for a in actions {
            if a == Action::Settings {
                // Ouvre la page Apparence, ou ferme la fenêtre si elle y est déjà.
                if self.params.open && self.params.page() == settings::Page::Appearance {
                    self.params.open = false;
                } else {
                    self.show_settings(settings::Page::Appearance);
                }
            } else if main {
                self.run_action(a);
            }
        }
        if !main {
            return;
        }
        // Onglets (lot E2) : Ctrl+Tab, Ctrl+W, et Ctrl+chiffre quand la barre est affichée.
        self.tabs_keys(ui);
        self.guild_number_keys(ui);
        let (back, forward) = ui.input(|i| (i.pointer.button_pressed(egui::PointerButton::Extra1), i.pointer.button_pressed(egui::PointerButton::Extra2)));
        if back {
            self.step_history(false);
        }
        if forward {
            self.step_history(true);
        }
    }

    /// Ctrl+1 à 9 : messages privés, puis les serveurs dans l'ordre du rail.
    fn guild_number_keys(&mut self, ui: &egui::Ui) {
        const NUMS: [Key; 9] = [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
        let hit = ui.input_mut(|i| NUMS.iter().position(|k| i.consume_key(egui::Modifiers::COMMAND, *k)));
        if let Some(g) = hit.and_then(|n| nav::guild_order(&self.store).get(n).copied())
            && g != self.store.guild
        {
            self.go(Place { guild: g, channel: None });
        }
    }

    /// Fin de l'image (interface principale) : touches seules non prises.
    fn shortcuts_late(&mut self, ui: &egui::Ui) {
        for a in self.take_shortcuts(ui, false) {
            self.run_action(a);
        }
    }

    /// Fenêtres du lot et raccourcis de fin d'image (interface principale).
    pub(super) fn nav_ui(&mut self, ui: &mut egui::Ui) {
        self.nav_observe();
        self.switcher_ui(ui);
        self.palette_ui(ui);
        self.inbox_ui(ui);
        self.shortcuts_window(ui);
        self.notif_window(ui);
        self.ff2_windows(ui);
        // Une fenêtre du lot ouverte garde ses touches (Échap, flèches).
        // Le sélecteur de partage aussi : Échap le ferme, au lieu de marquer le salon comme lu.
        if !self.nav.switcher.open && !self.palette.open && !self.nav.inbox.open && !self.nav.shortcuts_open && !self.params.open && self.nav.notif_win.is_none() && !self.share_picker_open() {
            self.shortcuts_late(ui);
        }
    }

    pub(super) fn run_action(&mut self, a: Action) {
        // Une fenêtre du lot est ouverte : seules les bascules valent.
        let modal = self.nav.switcher.open || self.nav.shortcuts_open || self.palette.open;
        if modal && !matches!(a, Action::QuickSwitcher | Action::Shortcuts | Action::Inbox | Action::AnswerCall | Action::DeclineCall) {
            return;
        }
        let s = &self.store;
        match a {
            Action::QuickSwitcher => {
                let open = !self.nav.switcher.open;
                self.nav.shortcuts_open = false;
                if open {
                    self.open_switcher();
                } else {
                    self.nav.switcher.open = false;
                }
            }
            Action::Inbox => {
                self.nav.switcher.open = false;
                self.nav.shortcuts_open = false;
                self.toggle_inbox();
            }
            Action::Shortcuts => {
                self.nav.switcher.open = false;
                self.nav.shortcuts_open = !self.nav.shortcuts_open;
            }
            Action::Settings => {}
            Action::PrevChannel | Action::NextChannel => {
                let list = nav::channels_here(s);
                if let Some(c) = nav::cycle(&list, s.channel, a == Action::NextChannel) {
                    let guild = s.guild;
                    self.go(Place { guild, channel: Some(c) });
                }
            }
            Action::PrevUnread | Action::NextUnread => {
                if let Some(p) = nav::unread_step(s, a == Action::NextUnread) {
                    self.go(p);
                }
            }
            Action::PrevGuild | Action::NextGuild => {
                let list = nav::guild_order(s);
                if let Some(g) = nav::cycle(&list, Some(s.guild), a == Action::NextGuild) {
                    self.go(Place { guild: g, channel: None });
                }
            }
            Action::Back => self.step_history(false),
            Action::Forward => self.step_history(true),
            Action::MarkRead => {
                if let Some(c) = s.channel {
                    self.store.mark_read(c);
                }
            }
            Action::MarkGuildRead => {
                if let Some(g) = s.guild {
                    self.store.mark_guild_read(g);
                }
            }
            Action::ReplyLast => {
                let me = s.me.as_ref().map(|u| u.id);
                let last = s.channel.and_then(|c| s.chans.get(&c)).and_then(|cs| cs.msgs.iter().rev().find(|m| Some(m.author_id) != me && m.system.is_none())).map(|m| m.id);
                if let Some(id) = last {
                    self.actions.push(super::msg_actions::Action::Reply(id));
                }
            }
            Action::PageUp | Action::PageDown => {
                if let Some(c) = s.channel
                    && self.list.page(c, a == Action::PageUp)
                {
                    self.store.load_older(c);
                }
                self.ctx.request_repaint();
            }
            Action::Emojis => {
                if s.channel.is_some_and(|c| s.send_block(c).is_none()) {
                    self.open_emoji_picker();
                }
            }
            Action::Pins => {
                if s.channel.is_some() {
                    self.actions.pins_open = !self.actions.pins_open;
                }
            }
            Action::Members => {
                use crate::theme::MembersPanel;
                let shown = self.theme.members_shown();
                self.look.edited.layout.members = if shown { MembersPanel::Hidden } else { MembersPanel::Shown };
            }
            // Vocal (phase 4) ; l'appui-pour-parler se lit touche tenue (`ui::voice`).
            Action::ToggleMute => self.voice_toggle_mute(),
            Action::ToggleDeafen => self.voice_toggle_deaf(),
            Action::PushToTalk => {}
            // Message sélectionné (`msg_select`).
            Action::MsgReply | Action::MsgEdit | Action::MsgReact | Action::MsgPin | Action::MsgCopy | Action::MsgDelete => {}
            Action::Search => {
                if s.search_scope().is_some() {
                    self.ctx.memory_mut(|m| m.request_focus(Id::unique("recherche")));
                }
            }
            Action::Upload | Action::Gifs | Action::Stickers => {
                if let Some(c) = s.channel.filter(|c| s.send_block(*c).is_none()) {
                    match a {
                        Action::Upload => self.pick_attachment(c),
                        _ => self.picker_shortcut(c, a == Action::Gifs),
                    }
                }
            }
            Action::CreateGuild => {
                self.show_server_dialog("choisir");
            }
            Action::Threads => {
                if let Some(c) = s.channel {
                    let parent = s.threads.get(c).map_or(c, |th| th.parent_id);
                    if s.lists_threads(parent) {
                        egui::Popup::toggle_id(&self.ctx, super::chat_head::threads_popup_id());
                    }
                }
            }
            Action::Help => self.open_link(HELP_URL),
            Action::OldestUnread => {
                if let Some((c, m)) = s.channel.and_then(|c| Some((c, s.chans.get(&c)?.new_marker?))) {
                    self.jump_to_message(c, m);
                }
            }
            Action::AnswerCall | Action::DeclineCall => {
                if let Some(c) = self.ringing_calls().first().copied() {
                    if a == Action::AnswerCall {
                        self.store.accept_call(c);
                        self.store.select_dms();
                        self.open_channel(c);
                    } else {
                        self.store.decline_call(c);
                    }
                    self.flush_commands();
                }
            }
            Action::StartCall => {
                if let Some(c) = s.channel.filter(|c| s.guild.is_none() && s.dms.iter().any(|d| d.id == *c) && s.calls.call(*c).is_none() && !s.calls.calling(*c)) {
                    self.store.start_call(c);
                    self.flush_commands();
                }
            }
            // Page Amis, en choix des membres d'un nouveau groupe.
            Action::CreateGroup => self.pick_friends(Vec::new()),
        }
        self.ctx.request_repaint();
    }

    /// Fenêtre « Raccourcis clavier » (Ctrl+/) : la liste, en lecture.
    fn shortcuts_window(&mut self, ui: &mut egui::Ui) {
        if !self.nav.shortcuts_open {
            return;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape)) {
            self.nav.shortcuts_open = false;
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let mut edit = false;
        let screen = ui.ctx().content_rect();
        let mobile = super::mobile::is_active(ui.ctx());
        let width = if mobile { super::mobile::card_width(ui.ctx(), &t, 1.0) } else { (screen.width() - 4.0 * t.spacing.card_pad).clamp(1.0, t.layout.card_width * 1.4) };
        // Gabarit commun des fenêtres : titre et croix, liste qui défile, pied.
        let modal = super::widgets::modal(
            ui.ctx(),
            &t,
            Id::unique("raccourcis"),
            width,
            nt::SHORTCUTS.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                // Une ligne par raccourci, sur toute la largeur : bandes alternées jusqu'au
                // bord droit, action à gauche, touches dans leur colonne.
                let row_h = ui.spacing().interact_size.y;
                let pad = t.spacing.item_x;
                let keys_x = if mobile { 0.62 } else { 0.6 };
                let strong = egui::FontId::new(t.typography.body, crate::ui::fonts::demi());
                let body = egui::FontId::proportional(t.typography.body);
                let line = |ui: &mut egui::Ui, i: usize, what: &str, keys: &str| {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), egui::Sense::hover());
                    if !ui.is_rect_visible(rect) {
                        return;
                    }
                    let p = ui.painter();
                    if i % 2 == 1 {
                        p.rect_filled(rect, t.r_row(), t.colors.row_hover);
                    }
                    let split = rect.left() + rect.width() * keys_x;
                    let mut job = egui::text::LayoutJob::simple_singleline(what.to_owned(), body.clone(), t.colors.text);
                    job.wrap = egui::text::TextWrapping::truncate_at_width((split - rect.left() - 2.0 * pad).max(1.0));
                    let g = p.layout_job(job);
                    p.galley(egui::pos2(rect.left() + pad, rect.center().y - g.size().y / 2.0), g, t.colors.text);
                    let mut job = egui::text::LayoutJob::simple_singleline(keys.to_owned(), strong.clone(), t.colors.text_strong);
                    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - split - pad).max(1.0));
                    let g = p.layout_job(job);
                    p.galley(egui::pos2(split, rect.center().y - g.size().y / 2.0), g, t.colors.text_strong);
                };
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut i = 0;
                for (a, c) in self.nav.bindings.iter() {
                    let text = c.map_or_else(|| nt::NO_CHORD.get(lang).to_string(), |c| chord_label(lang, c));
                    line(ui, i, action_label(a).get(lang), &text);
                    i += 1;
                }
                if !mobile {
                    for (keys, what) in FIXED {
                        line(ui, i, what.get(lang), keys);
                        i += 1;
                    }
                }
                ui.add_space(t.spacing.item_y);
                settings::note(ui, &t, nt::MOUSE_NOTE.get(lang));
                settings::note(ui, &t, nt::TYPING_NOTE.get(lang));
            },
            |ui| edit = super::widgets::button(ui, &t, super::widgets::Kind::Secondary, nt::EDIT_IN_SETTINGS.get(lang)).clicked(),
        );
        let open = !modal.closed;
        if !open {
            self.nav.shortcuts_open = false;
        }
        if edit {
            self.nav.shortcuts_open = false;
            self.show_settings(settings::Page::Keybinds);
        }
    }

    /// Page « Raccourcis clavier » des Paramètres : réassigner, retirer,
    /// remettre par défaut ; conflits signalés.
    pub(super) fn page_keybinds(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        settings::note(ui, t, nt::FILE_NOTE.get(lang));
        settings::note(ui, t, nt::TYPING_NOTE.get(lang));
        settings::note(ui, t, nt::MOUSE_NOTE.get(lang));
        // Combinaison attendue : la première touche pressée (hors
        // modificateurs seuls) la donne ; Échap seul annule.
        if let Some(a) = self.nav.capturing {
            let got = ui.input_mut(|i| {
                let mut got = None;
                i.events.retain(|e| match e {
                    egui::Event::Key { key, pressed: true, modifiers, .. } if got.is_none() => {
                        got = Some(chord_of(*key, *modifiers));
                        false
                    }
                    _ => true,
                });
                got
            });
            match got {
                Some(c) if c == Chord { ctrl: false, alt: false, shift: false, key: "Escape".into() } => self.nav.capturing = None,
                Some(c) => {
                    self.nav.capturing = None;
                    self.nav.bindings.set(a, Some(c));
                    self.save_keys();
                }
                None => {}
            }
        }
        let mut change = None;
        let mut remove = None;
        let mut reset = None;
        let rows: Vec<(Action, Option<Chord>, Vec<Action>)> = self.nav.bindings.iter().map(|(a, c)| (a, c.cloned(), self.nav.bindings.conflicts(a))).collect();
        settings::section(ui, t, nt::SHORTCUTS_EDITABLE.get(lang));
        for (a, c, conflicts) in rows {
            super::appearance::row(ui, action_label(a).get(lang), "", |ui| {
                ui.horizontal(|ui| {
                    let capturing = self.nav.capturing == Some(a);
                    let text = if capturing {
                        nt::PRESS_KEYS.get(lang).to_string()
                    } else {
                        c.as_ref().map_or_else(|| nt::NO_CHORD.get(lang).to_string(), |c| chord_label(lang, c))
                    };
                    let color = if conflicts.is_empty() { t.colors.text_strong } else { t.colors.danger };
                    let resp = ui.add(egui::Button::new(RichText::new(text).family(crate::ui::fonts::demi()).strong().color(color)).selected(capturing)).on_hover_text(nt::CHANGE.get(lang));
                    focus_ring(ui, t, &resp);
                    if resp.clicked() {
                        change = Some(a);
                    }
                    if c.is_some() && crate::ui::widgets::compact(ui, nt::REMOVE.get(lang)).clicked() {
                        remove = Some(a);
                    }
                    if !self.nav.bindings.is_default(a) && crate::ui::widgets::compact(ui, nt::RESET.get(lang)).clicked() {
                        reset = Some(a);
                    }
                });
                if let Some(other) = conflicts.first() {
                    ui.label(RichText::new(nf::conflict(lang, action_label(*other).get(lang))).small().color(t.colors.danger));
                }
            });
        }
        settings::section(ui, t, xt::FIXED_TITLE.get(lang));
        for (keys, what) in FIXED {
            super::appearance::row(ui, what.get(lang), "", |ui| {
                ui.label(RichText::new(keys).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
            });
        }
        ui.add_space(t.spacing.category_gap);
        let all_default = Action::ALL.iter().all(|a| self.nav.bindings.is_default(*a));
        if !all_default && ui.button(nt::RESET_ALL.get(lang)).clicked() {
            self.nav.bindings = Bindings::default();
            self.save_keys();
        }
        if let Some(a) = change {
            self.nav.capturing = if self.nav.capturing == Some(a) { None } else { Some(a) };
        }
        if let Some(a) = remove {
            self.nav.bindings.set(a, None);
            self.save_keys();
        }
        if let Some(a) = reset {
            self.nav.bindings.reset(a);
            self.save_keys();
        }
    }
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::desk::keys::MemoryKeys;
    use crate::state::model::{Channel, ChannelKind, Dm, Guild, User};

    struct Offline;

    impl super::super::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<crate::state::Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    fn chan(id: u64, name: &str, pos: i32) -> Channel {
        Channel { id, name: name.into(), kind: ChannelKind::Text, position: pos, parent: None, overwrites: None, slowmode: 0 }
    }

    pub(in super::super) fn app() -> App {
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut store = crate::state::Store::new();
        let guilds = vec![Guild::new(9, "Alpha".into(), vec![chan(10, "général", 0), chan(11, "hors-sujet", 1)]), Guild::new(8, "Beta".into(), vec![chan(20, "accueil", 0)])];
        store.apply(crate::state::Event::Ready { me: User::new(1, "Moi".into()), guilds, dms: vec![Dm::new(50, "Alice".into(), 50)] });
        let mut app = App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), store, images));
        app.lang = Lang::Fr;
        app.open(Some(9), 10);
        app
    }

    /// Une image avec ces événements (touches, boutons de la souris).
    pub(in super::super) fn frame(app: &mut App, events: Vec<egui::Event>) {
        let ctx = app.ctx.clone();
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
    }

    pub(in super::super) fn key(key: Key, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers }
    }

    const ALT: egui::Modifiers = egui::Modifiers { alt: true, ctrl: false, shift: false, mac_cmd: false, command: false };

    #[test]
    fn alt_arrows_walk_channels_and_history_goes_back() {
        let mut app = app();
        frame(&mut app, Vec::new());
        frame(&mut app, vec![key(Key::ArrowDown, ALT)]);
        assert_eq!(app.store.channel, Some(11));
        // Touche relâchée entre deux appuis : sans cela egui y voit une touche
        // maintenue, dont la répétition est ralentie (lot « fiable »).
        frame(&mut app, vec![egui::Event::Key { key: Key::ArrowDown, physical_key: None, pressed: false, repeat: false, modifiers: ALT }]);
        frame(&mut app, vec![key(Key::ArrowDown, ALT)]);
        assert_eq!(app.store.channel, Some(10), "en boucle");
        let ctrl_alt = egui::Modifiers { ctrl: true, command: true, ..ALT };
        frame(&mut app, vec![key(Key::ArrowDown, ctrl_alt)]);
        assert_eq!((app.store.guild, app.store.channel), (Some(8), Some(20)), "serveur suivant, premier salon");
        frame(&mut app, vec![key(Key::ArrowLeft, ALT)]);
        assert_eq!((app.store.guild, app.store.channel), (Some(9), Some(10)), "précédent");
        frame(&mut app, vec![egui::Event::PointerButton { pos: egui::pos2(500.0, 300.0), button: egui::PointerButton::Extra2, pressed: true, modifiers: Default::default() }]);
        assert_eq!(app.store.channel, Some(20), "bouton 5 : suivant");
    }

    #[test]
    fn unread_shortcut_skips_read_channels_and_shift_escape_marks_the_server_read() {
        let mut app = app();
        app.store.apply(crate::state::Event::MessageCreate(crate::state::model::Message { channel_id: 20, guild_id: Some(8), ..test_message(7 << 22) }));
        assert!(app.store.unread().contains(&20));
        frame(&mut app, Vec::new());
        let alt_shift = egui::Modifiers { shift: true, ..ALT };
        frame(&mut app, vec![key(Key::ArrowDown, alt_shift)]);
        assert_eq!(app.store.channel, Some(20));
        assert!(!app.store.unread().contains(&20), "ouvert : lu");
        app.store.apply(crate::state::Event::MessageCreate(crate::state::model::Message { channel_id: 11, guild_id: Some(9), ..test_message(8 << 22) }));
        app.go(Place { guild: Some(9), channel: Some(10) });
        frame(&mut app, Vec::new());
        assert!(app.store.unread().contains(&11));
        frame(&mut app, vec![key(Key::Escape, egui::Modifiers::NONE)]);
        assert!(app.store.unread().contains(&11), "Échap : le salon ouvert seulement");
        // Échap relâchée avant : sinon egui voit une répétition.
        let up = egui::Event::Key { key: Key::Escape, physical_key: None, pressed: false, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut app, vec![up, key(Key::Escape, egui::Modifiers::SHIFT)]);
        assert!(!app.store.unread().contains(&11), "Maj+Échap : tout le serveur");
    }

    #[test]
    fn rebound_chord_fires_and_is_saved_and_typing_guard_holds() {
        let mut app = app();
        let mem = MemoryKeys::default();
        app.attach_keys(Box::new(mem.clone()));
        frame(&mut app, Vec::new());
        app.set_shortcut(Action::NextChannel, Chord::parse("N"));
        assert!(mem.text.lock().map(|t| t.clone().unwrap_or_default()).unwrap_or_default().contains("salon_suivant = \"N\""));
        frame(&mut app, vec![key(Key::N, egui::Modifiers::NONE)]);
        assert_eq!(app.store.channel, Some(11));
        // Un champ de texte a le clavier : « N » écrit, il ne navigue pas.
        app.ctx.memory_mut(|m| m.request_focus(Id::unique("composer")));
        frame(&mut app, Vec::new());
        assert!(app.ctx.text_edit_focused(), "la saisie a le clavier");
        frame(&mut app, vec![key(Key::N, egui::Modifiers::NONE)]);
        assert_eq!(app.store.channel, Some(11));
    }

    /// Touche maintenue (lot « fiable ») : la répétition du système ne fait
    /// pas défiler les salons à trente par seconde, chacun avec son
    /// chargement d'historique.
    #[test]
    fn a_held_key_does_not_race_through_channels() {
        let mut app = app();
        frame(&mut app, Vec::new());
        // Alt+Bas (défaut) : une combinaison que la saisie ne retient pas.
        let held = |repeat| egui::Event::Key { key: Key::ArrowDown, physical_key: None, pressed: true, repeat, modifiers: ALT };
        frame(&mut app, vec![held(false)]);
        let first = app.store.channel;
        assert_eq!(first, Some(11));
        frame(&mut app, vec![held(true)]);
        frame(&mut app, vec![held(true)]);
        assert_eq!(app.store.channel, first, "répétitions rapprochées ignorées");
        std::thread::sleep(NAV_REPEAT_GAP + std::time::Duration::from_millis(60));
        frame(&mut app, vec![held(true)]);
        assert_ne!(app.store.channel, first, "après l'écart, la répétition passe");
        // Touche relâchée puis rappuyée aussitôt (egui déduit lui-même la
        // répétition d'une touche restée enfoncée) : un nouvel appui passe toujours.
        let now = app.store.channel;
        frame(&mut app, vec![egui::Event::Key { key: Key::ArrowDown, physical_key: None, pressed: false, repeat: false, modifiers: ALT }]);
        frame(&mut app, vec![held(false)]);
        assert_ne!(app.store.channel, now, "nouvel appui");
    }

    #[test]
    fn file_is_read_once_and_bad_lines_are_reported() {
        let mut app = app();
        let mem = MemoryKeys::default();
        if let Ok(mut t) = mem.text.lock() {
            *t = Some("[raccourcis]\nselecteur_rapide = \"Ctrl+T\"\nmembres = Ctrl+M\n".into());
        }
        app.attach_keys(Box::new(mem));
        frame(&mut app, Vec::new());
        assert_eq!(app.shortcut(Action::QuickSwitcher).map(Chord::to_text).as_deref(), Some("Ctrl+T"));
        assert!(app.notice().is_some_and(|n| n.contains("ligne 3")), "{:?}", app.notice());
    }

    #[test]
    fn ctrl_digits_go_to_servers_and_escape_declines_only_while_ringing() {
        let mut app = app();
        frame(&mut app, Vec::new());
        let ctrl = egui::Modifiers { ctrl: true, command: true, ..egui::Modifiers::NONE };
        frame(&mut app, vec![key(Key::Num3, ctrl)]);
        assert_eq!(app.store.guild, Some(8), "troisième rang : le deuxième serveur");
        frame(&mut app, vec![key(Key::Num1, ctrl)]);
        assert_eq!(app.store.guild, None, "premier rang : les messages privés");
        frame(&mut app, vec![key(Key::Num9, ctrl)]);
        assert_eq!(app.store.guild, None, "rang vide : rien");
        // Aucun appel ne sonne : Échap marque comme lu, Ctrl+Entrée n'est pas pris.
        assert!(app.take_shortcuts_for_test(Key::Escape, egui::Modifiers::NONE).contains(&Action::MarkRead));
        assert!(app.take_shortcuts_for_test(Key::Enter, ctrl).is_empty());
        // Toutes les actions ont un libellé dans les deux langues.
        for a in Action::ALL {
            assert!(!action_label(a).get(Lang::Fr).is_empty() && !action_label(a).get(Lang::En).is_empty(), "{a:?}");
        }
    }

    #[test]
    fn labels_are_readable() {
        assert_eq!(chord_label(Lang::Fr, &Action::NextUnread.default_chord().unwrap_or_else(|| panic!("défaut"))), "Alt+Maj+\u{2193}");
        assert_eq!(chord_label(Lang::En, &Action::MarkRead.default_chord().unwrap_or_else(|| panic!("défaut"))), "Esc");
        assert_eq!(chord_label(Lang::Fr, &Action::Shortcuts.default_chord().unwrap_or_else(|| panic!("défaut"))), "Ctrl+/");
        assert_eq!(chord_label(Lang::Fr, &Action::PageUp.default_chord().unwrap_or_else(|| panic!("défaut"))), "Page préc.");
    }

    pub(in super::super) fn test_message(id: u64) -> crate::state::model::Message {
        crate::state::model::Message {
            id,
            channel_id: 0,
            guild_id: None,
            author_id: 3,
            author: "Bob".into(),
            author_avatar: crate::state::model::Avatar::default_for(3),
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
}
