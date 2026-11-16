//! Sons et notifications par événement (F11) : section « Sons » de la page
//! Notifications (un interrupteur et une écoute par son, comme Discord), et
//! déclenchement de ceux qui ne viennent ni d'un message ni du vocal :
//! demande d'ami reçue, événement qui commence, appel sortant qui sonne.
//!
//! Les sons eux-mêmes sont ceux de Ferrocord (`audio::sons` : fabriqués par
//! un petit synthétiseur, ou remplacés par un fichier du dossier des sons de
//! l'utilisateur) : rien n'est embarqué, téléchargé, ni repris de Discord.
//! Rien ne tourne au repos : tout part d'un événement reçu, sauf la sonnerie
//! d'un appel sortant, qui se répète tant que l'appel sonne (un réveil toutes
//! les trois secondes, pendant une minute au plus).

use super::settings::{note, section};
use super::App;
use crate::audio::sons::perso::Releve;
use crate::i18n::table::ff::{f as ff, t as ft};
use crate::i18n::Text;
use crate::state::notify::{Alert, Kind};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Écart entre deux sonneries d'un appel sortant (la durée du son : il boucle
/// sur lui-même), et durée au plus.
const OUT_RING_EVERY: Duration = Duration::from_secs(3);
const OUT_RING_MAX: Duration = Duration::from_secs(60);
/// Écart entre deux lectures du dossier des sons, page des réglages ouverte.
const SCAN_EVERY: Duration = Duration::from_secs(2);

/// Fichiers personnels vus dans le dossier des sons (partagé avec le fil qui le lit).
#[derive(Default)]
struct Custom {
    /// Sons remplacés par un fichier valide, fichiers refusés (bits de [`Kind::bit`]).
    good: AtomicU32,
    bad: AtomicU32,
    /// Une lecture du dossier est en cours.
    busy: AtomicBool,
    files: Mutex<Releve>,
}

/// Suivi des sonneries et du dossier des sons (appartient à `DeskUi`).
#[derive(Default)]
pub(super) struct Watch {
    /// Appel sortant qui sonne : (salon, début, dernière sonnerie).
    out_ring: Option<(u64, Instant, Instant)>,
    /// Un appel entrant sonnait au dernier passage.
    in_ring: bool,
    custom: Arc<Custom>,
    /// Dernière lecture du dossier des sons demandée.
    scanned: Option<Instant>,
}

/// Nom d'un son dans la page des réglages.
pub(super) fn sound_name(k: Kind) -> Text {
    match k {
        Kind::Message => ft::SND_MESSAGE,
        Kind::Mention => ft::SND_MENTION,
        Kind::VoiceJoin => ft::SND_JOIN,
        Kind::VoiceLeave => ft::SND_LEAVE,
        Kind::VoiceConnect => ft::SND_CONNECT,
        Kind::VoiceDisconnect => ft::SND_DISCONNECT,
        Kind::VoiceMute => ft::SND_MUTE,
        Kind::VoiceUnmute => ft::SND_UNMUTE,
        Kind::VoiceDeafen => ft::SND_DEAFEN,
        Kind::VoiceUndeafen => ft::SND_UNDEAFEN,
        Kind::Ring => ft::SND_RING,
        Kind::CallOut => ft::SND_CALL_OUT,
        Kind::StreamStart => ft::SND_STREAM_START,
        Kind::StreamStop => ft::SND_STREAM_STOP,
        Kind::FriendRequest => ft::SND_FRIEND,
        Kind::EventStart => ft::SND_EVENT,
        Kind::Reaction => ft::SND_REACTION,
        Kind::Reminder => ft::SND_REMINDER,
    }
}

impl App {
    /// Notification et son d'un événement qui n'est pas un message.
    fn event_alert(&mut self, kind: Kind, title: String, body: String, guild: Option<u64>) {
        let prefs = self.desk.prefs;
        if prefs.toasts && !self.local.prefs.toasts_muted() && !crate::state::notify::dnd(&self.store) {
            self.desk_toast(&Alert { guild, channel: 0, kind, title, place: None, body, avatar: None, header: None });
        }
        self.voice_sound(kind);
    }

    /// À chaque passage de logique : demandes d'ami et débuts d'événements
    /// arrivés depuis le dernier passage, sonnerie d'un appel sortant.
    pub(super) fn sounds_tick(&mut self) {
        let lang = self.lang;
        for id in self.store.relations.take_incoming() {
            let name = self.store.relations.display_name(id).to_string();
            self.event_alert(Kind::FriendRequest, name, ft::FRIEND_REQUEST_BODY.get(lang).to_string(), None);
        }
        for (guild, id) in self.store.events.take_started() {
            // Seulement ceux auxquels je me suis dit intéressé, comme Discord.
            if !self.store.event_interested(guild, id) || self.store.account.notifs.by_guild.get(&Some(guild)).is_some_and(|g| g.mute_events) {
                continue;
            }
            let Some(name) = self.store.find_event(guild, id).map(|e| e.name.clone()) else { continue };
            let place = self.store.guilds.iter().find(|g| g.id == guild).map(|g| g.name.clone()).unwrap_or_default();
            self.event_alert(Kind::EventStart, name, ff::event_started(lang, place), Some(guild));
        }
        // Réactions à mes messages (F11) : jamais pour le salon que je regarde,
        // ni pour un salon muet ; une par salon toutes les cinq secondes.
        for r in self.store.take_reactions() {
            let viewing = self.window_focused() && self.store.channel == Some(r.channel_id);
            let parent = crate::state::notify::parent_of(&self.store, r.guild_id, r.channel_id);
            if viewing || self.store.account.notifs.muted(r.guild_id, r.channel_id, parent, crate::state::store::now_ms()) || !self.desk.throttle.allow(r.channel_id, Instant::now()) {
                continue;
            }
            let name = r.name.clone().filter(|n| !n.is_empty()).unwrap_or_else(|| self.store.relations.display_name(r.user_id).to_string());
            let emoji = match &r.emoji {
                crate::state::social::Emoji::Unicode(s) => s.clone(),
                crate::state::social::Emoji::Custom { name, .. } => format!(":{name}:"),
            };
            let title = if name.is_empty() { ft::SOMEONE.get(lang).to_string() } else { name };
            self.event_alert(Kind::Reaction, title, ff::reacted(lang, emoji), r.guild_id);
        }
        // `/tts` reçus dans le salon affiché (F27) : lus par la synthèse de Windows.
        for text in self.store.take_tts() {
            if crate::platform::speech::AVAILABLE && self.window_focused() {
                crate::platform::speech::speak(&text);
            }
        }
        self.reminders_tick();
        // Appel sortant : je suis dans l'appel et quelqu'un d'autre sonne encore.
        let ringing = self.store.calls_ringing_out();
        let now = Instant::now();
        match (ringing, self.desk.watch.out_ring) {
            (None, None) => {}
            // On a décroché, ou l'appel est fini : la sonnerie se tait aussitôt.
            (None, Some(_)) => {
                self.desk.watch.out_ring = None;
                self.desk_silence(Kind::CallOut);
            }
            (Some(c), Some((old, start, last))) if c == old => {
                if now.duration_since(start) < OUT_RING_MAX {
                    if now.duration_since(last) >= OUT_RING_EVERY {
                        self.desk.watch.out_ring = Some((c, start, now));
                        self.voice_sound(Kind::CallOut);
                    }
                    let last = self.desk.watch.out_ring.map_or(now, |r| r.2);
                    self.ctx.request_repaint_after(OUT_RING_EVERY.saturating_sub(now.duration_since(last)));
                }
            }
            (Some(c), _) => {
                self.desk.watch.out_ring = Some((c, now, now));
                self.voice_sound(Kind::CallOut);
                self.ctx.request_repaint_after(OUT_RING_EVERY);
            }
        }
        // Appel entrant décroché, refusé ou manqué : même chose (la sonnerie
        // elle-même est lancée par `calls_tick`).
        let in_ring = !self.ringing_calls().is_empty();
        if self.desk.watch.in_ring && !in_ring {
            self.desk_silence(Kind::Ring);
        }
        self.desk.watch.in_ring = in_ring;
    }

    /// Rappels de messages enregistrés (lot FF2) : notification locale de
    /// ceux qui sont échus, puis un seul réveil programmé pour le prochain.
    /// Rien ne tourne s'il n'y a aucun rappel connu.
    fn reminders_tick(&mut self) {
        let Some(next) = self.store.saved.next_reminder() else { return };
        let now = self.store.now();
        if next > now {
            self.ctx.request_repaint_after(Duration::from_millis(u64::try_from(next - now).unwrap_or(0).saturating_add(50)));
            return;
        }
        let lang = self.lang;
        for r in self.store.saved.take_due(now) {
            let body = match (r.author.is_empty(), r.excerpt.is_empty()) {
                (false, false) => format!("{} : {}", r.author, r.excerpt),
                (true, false) => r.excerpt.clone(),
                (false, true) => r.author.clone(),
                (true, true) => ft::REMINDER_BODY.get(lang).to_string(),
            };
            self.event_alert(Kind::Reminder, ft::REMINDER_TITLE.get(lang).to_string(), body, r.guild_id);
        }
        if let Some(next) = self.store.saved.next_reminder() {
            self.ctx.request_repaint_after(Duration::from_millis(u64::try_from(next - now).unwrap_or(0).saturating_add(50)));
        }
    }

    /// Relit le dossier des sons personnels, dans un fil à part, au plus
    /// toutes les deux secondes et seulement tant que la section est affichée.
    /// Sans vrai bureau (aperçu, tests) : rien n'est lu.
    fn sounds_scan(&mut self) {
        if !self.desk_custom_sounds() {
            return;
        }
        let now = Instant::now();
        let due = self.desk.watch.scanned.is_none_or(|t| now.duration_since(t) >= SCAN_EVERY);
        self.ctx.request_repaint_after(SCAN_EVERY);
        if !due || self.desk.watch.custom.busy.swap(true, Relaxed) {
            return;
        }
        self.desk.watch.scanned = Some(now);
        let (custom, ctx) = (self.desk.watch.custom.clone(), self.ctx.clone());
        let spawned = std::thread::Builder::new().name("ferrocord-sons".into()).spawn(move || {
            let found = crate::audio::sons::perso::dossier().map(|d| custom.files.lock().map(|mut r| r.actualiser(&d)).unwrap_or_default()).unwrap_or_default();
            let changed = (custom.good.swap(found.0, Relaxed) != found.0) | (custom.bad.swap(found.1, Relaxed) != found.1);
            custom.busy.store(false, Relaxed);
            if changed {
                ctx.request_repaint();
            }
        });
        if spawned.is_err() {
            self.desk.watch.custom.busy.store(false, Relaxed);
        }
    }

    /// Section « Sons » de la page Notifications : un interrupteur et une
    /// écoute par son, la mention « personnalisé » quand un fichier du dossier
    /// des sons le remplace, et le bouton qui ouvre ce dossier.
    pub(super) fn sounds_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        section(ui, t, ft::SOUNDS_SECTION.get(lang));
        note(ui, t, ft::SOUNDS_TIP.get(lang));
        if crate::ui::widgets::compact(ui, RichText::new(ft::SOUNDS_FOLDER.get(lang))).clicked() {
            open_sounds_folder();
        }
        self.sounds_scan();
        let (good, bad) = (self.desk.watch.custom.good.load(Relaxed), self.desk.watch.custom.bad.load(Relaxed));
        let mut p = self.desk.prefs;
        let mut test = None;
        for k in Kind::ALL {
            let label = sound_name(k).get(lang);
            super::settings::shell::settings_row(ui, label, "", true, |ui| {
                let mut on = p.muted_sounds & k.bit() == 0;
                let resp = ui.add_enabled_ui(p.sounds, |ui| super::settings::shell::switch(ui, &mut on)).inner;
                crate::ui::a11y::name(ui.ctx(), resp.id, label);
                if resp.changed() {
                    p.muted_sounds = if on { p.muted_sounds & !k.bit() } else { p.muted_sounds | k.bit() };
                }
                let try_it = ui.add_enabled_ui(p.sounds && p.volume > 0, |ui| crate::ui::widgets::compact(ui, RichText::new(crate::i18n::table::notify::t::TEST_SOUND.get(lang)))).inner;
                if try_it.clicked() {
                    test = Some(k);
                }
                if good & k.bit() != 0 {
                    ui.label(RichText::new(ft::SOUND_CUSTOM.get(lang)).small().color(t.colors.accent));
                } else if bad & k.bit() != 0 {
                    crate::ui::widgets::tip(ui.label(RichText::new(ft::SOUND_BAD_FILE.get(lang)).small().color(t.colors.danger)), t, ft::SOUND_BAD_FILE_TIP.get(lang));
                }
            });
        }
        if p != self.desk.prefs {
            self.desk_save_prefs(p);
        }
        if let Some(k) = test
            && p.volume > 0
        {
            self.desk_test_sound(k, p.volume);
        }
    }
}

/// Ouvre le dossier des sons personnels dans l'Explorateur, après l'avoir
/// créé avec sa notice s'il manquait (hors du fil d'interface).
fn open_sounds_folder() {
    let _ = std::thread::Builder::new().name("ferrocord-sons".into()).spawn(|| {
        let Some(dir) = crate::audio::sons::perso::dossier() else { return };
        match crate::audio::sons::perso::preparer(&dir) {
            Ok(()) => crate::install::browser::open(&format!("file:///{}", dir.display().to_string().replace('\\', "/"))),
            Err(e) => eprintln!("[sons] dossier des sons impossible à créer : {e}"),
        }
    });
}
