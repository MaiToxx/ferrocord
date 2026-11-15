//! Notifications de bureau côté interface : décision à la réception
//! (`state::notify`), pastille et titre de la fenêtre, icône de la zone de
//! notification, fermeture dans la zone, section « Bureau » de la page
//! Notifications. Le système est derrière `desk::Desktop`.
//!
//! Rien ne tourne en boucle : tout se fait quand un événement arrive
//! (message, clic sur l'icône ou une notification, réglage changé) ; le
//! titre et l'icône ne sont touchés que si leur contenu change.

use super::settings::{note, section, toggle};
use super::App;
use crate::desk::{DeskEvent, Desktop, TrayState};
use crate::i18n::table::notify::{f as nf, t as nt};
use crate::state::model::Message;
use crate::state::notify::{self, DeskPrefs, Kind, Mentions, Throttle};
use crate::state::usersettings::schema::{Key, SValue};
use crate::theme::Tokens;
use eframe::egui::{self, ViewportCommand};
use std::time::Instant;

/// Bureau muet (tests, aperçu, avant `App::attach_desktop`).
struct Quiet;

impl Desktop for Quiet {
    fn load_prefs(&mut self) {}
    fn save_prefs(&mut self, _: DeskPrefs) {}
    fn toast(&mut self, _: &notify::Alert) {}
    fn sound(&mut self, _: Kind, _: u8) {}
    fn tray(&mut self, _: &TrayState) {}
    fn poll(&mut self) -> Vec<DeskEvent> {
        Vec::new()
    }
}

pub(super) struct DeskUi {
    pub(super) prefs: DeskPrefs,
    /// `notify.toml` lu : les réglages changés avant ne sont pas écrasés par la lecture.
    touched: bool,
    backend: Box<dyn Desktop>,
    pub(super) throttle: Throttle,
    mentions: Mentions,
    title: Option<String>,
    tray: Option<TrayState>,
    /// « Quitter » demandé : la fermeture n'est plus interceptée.
    quitting: bool,
    /// Fenêtre masquée dans la zone de notification.
    pub(super) hidden: bool,
    /// Un vrai bureau est branché : sans lui, pas d'icône, donc la croix ferme (aperçu, tests).
    attached: bool,
    /// Sons par événement (lot FF) : appel sortant qui sonne.
    pub(super) watch: super::sounds::Watch,
}

impl Default for DeskUi {
    fn default() -> Self {
        DeskUi {
            prefs: DeskPrefs::default(),
            touched: false,
            backend: Box::new(Quiet),
            throttle: Throttle::default(),
            mentions: Mentions::default(),
            title: None,
            tray: None,
            quitting: false,
            hidden: false,
            attached: false,
            watch: Default::default(),
        }
    }
}

/// Ce qu'un message reçu demande, calculé avant que le magasin ne le range.
pub(super) struct Pending {
    channel: u64,
    ping: bool,
    alert: Option<notify::Alert>,
}

impl Pending {
    /// Une notification part déjà pour ce message.
    pub(super) fn alerts(&self) -> bool {
        self.alert.is_some()
    }
}

fn rgb(c: egui::Color32) -> [u8; 3] {
    [c.r(), c.g(), c.b()]
}

impl App {
    /// Branche le vrai bureau (notifications, sons, icône) et lit `notify.toml`.
    pub fn attach_desktop(&mut self, mut d: Box<dyn Desktop>) {
        d.load_prefs();
        self.desk.backend = d;
        self.desk.tray = None;
        self.desk.attached = true;
    }

    /// Réglages de bureau en cours (aperçu, tests).
    pub fn desk_prefs(&self) -> DeskPrefs {
        self.desk.prefs
    }

    /// Fenêtre au premier plan (et pas masquée dans la zone de notification).
    pub(super) fn window_focused(&self) -> bool {
        !self.desk.hidden && self.ctx.input(|i| i.focused)
    }

    /// Avant que le magasin range `m` : faut-il notifier, et me mentionne-t-il ?
    pub(super) fn desk_before(&self, m: &Message) -> Pending {
        let facts = notify::facts(&self.store, m, self.window_focused());
        let ping = facts.author != facts.me && (m.guild_id.is_none() || notify::pings(&self.store.account.notifs, &facts));
        let alert = notify::alert(&self.store, m, self.window_focused(), self.lang, self.h24());
        Pending { channel: m.channel_id, ping, alert }
    }

    /// Après : mention comptée (si le magasin a marqué le salon), notification, son, clignotement.
    pub(super) fn desk_after(&mut self, p: Pending) {
        if p.ping && self.store.mentioned().contains(&p.channel) {
            self.desk.mentions.add(p.channel);
        }
        let Some(a) = p.alert else { return };
        self.desk_alert(a);
    }

    /// Montre une notification (fenêtre, son, clignotement) selon les réglages
    /// de bureau, au plus une par salon toutes les cinq secondes.
    pub(super) fn desk_alert(&mut self, a: notify::Alert) {
        if !self.desk.throttle.allow(a.channel, Instant::now()) {
            return;
        }
        let prefs = self.desk.prefs;
        // Mode streamer : notifications et sons coupés (`state::local`).
        if prefs.toasts && !self.local.prefs.toasts_muted() {
            if prefs.preview {
                self.desk.backend.toast(&a);
            } else {
                self.desk.backend.toast(&a.clone().without_preview(self.lang));
            }
        }
        if prefs.sound_on(a.kind) && !self.local.prefs.sounds_muted() {
            self.desk.backend.sound(a.kind, prefs.volume);
        }
        if prefs.badge && (a.kind == Kind::Mention || a.guild.is_none()) && !self.window_focused() {
            self.ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(egui::UserAttentionType::Informational));
        }
    }

    /// Son du vocal (arrivée, départ, micro, sonnerie), selon les mêmes
    /// réglages que les notifications.
    pub(super) fn voice_sound(&mut self, kind: Kind) {
        let prefs = self.desk.prefs;
        if prefs.sound_on(kind) && !self.local.prefs.sounds_muted() {
            self.desk.backend.sound(kind, prefs.volume);
        }
    }

    /// Notification d'un événement qui n'est pas un message (lot FF).
    pub(super) fn desk_toast(&mut self, alert: &notify::Alert) {
        self.desk.backend.toast(alert);
    }

    /// Enregistre des réglages de bureau changés dans une autre section (lot FF).
    pub(super) fn desk_save_prefs(&mut self, p: DeskPrefs) {
        self.desk.prefs = p;
        self.desk.touched = true;
        self.desk.backend.save_prefs(p);
    }

    /// « Essayer » d'un son de la section « Sons ».
    pub(super) fn desk_test_sound(&mut self, kind: Kind, volume: u8) {
        self.desk.backend.sound(kind, volume);
    }

    /// Appel entrant (lot V6) : notification (`alert`, à la première
    /// sonnerie) et son de sonnerie, selon les mêmes réglages qu'un message.
    pub(super) fn desk_ring(&mut self, alert: Option<&notify::Alert>) {
        let prefs = self.desk.prefs;
        if let Some(a) = alert {
            if prefs.toasts && !self.local.prefs.toasts_muted() {
                self.desk.backend.toast(a);
            }
            if prefs.badge && !self.window_focused() {
                self.ctx.send_viewport_cmd(ViewportCommand::RequestUserAttention(egui::UserAttentionType::Critical));
            }
        }
        self.voice_sound(Kind::Ring);
    }

    pub(super) fn show_window(&mut self) {
        self.desk.hidden = false;
        self.ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        self.ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
        self.ctx.send_viewport_cmd(ViewportCommand::Focus);
    }

    /// Ferme vraiment la fenêtre (pas dans la zone de notification) :
    /// redémarrage après une mise à jour (lot C6).
    pub(super) fn quit_app(&mut self) {
        self.desk.quitting = true;
        self.ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    pub(super) fn hide_window(&mut self) {
        self.desk.hidden = true;
        self.ctx.send_viewport_cmd(ViewportCommand::Visible(false));
    }

    /// À chaque passage de logique : nouvelles du bureau, fermeture, titre, icône.
    pub(super) fn desk_tick(&mut self) {
        for e in self.desk.backend.poll() {
            match e {
                DeskEvent::Prefs(p) => {
                    if !self.desk.touched {
                        self.desk.prefs = p;
                    }
                }
                DeskEvent::SaveFailed(reason) => {
                    let text = nf::save_failed(self.lang, reason);
                    self.notify(text);
                }
                DeskEvent::Open { guild, channel } => {
                    self.show_window();
                    if self.store.channel_exists(channel) {
                        match guild {
                            Some(g) => self.store.guild = Some(g),
                            None => self.store.select_dms(),
                        }
                        self.open_channel(channel);
                        self.flush_commands();
                    }
                }
                DeskEvent::TrayToggle if self.desk.hidden => self.show_window(),
                DeskEvent::TrayToggle | DeskEvent::TrayHide => self.hide_window(),
                DeskEvent::TrayShow => self.show_window(),
                DeskEvent::TrayStatus(s) => {
                    if let Err(r) = self.store.account.write(Key::Status, SValue::Str(s.to_string())) {
                        self.refused(r);
                    }
                    self.flush_commands();
                }
                DeskEvent::TrayQuit => {
                    self.desk.quitting = true;
                    self.ctx.send_viewport_cmd(ViewportCommand::Close);
                }
            }
        }
        let prefs = self.desk.prefs;
        if self.desk.attached && prefs.tray && prefs.close_to_tray && !self.desk.quitting && self.ctx.input(|i| i.viewport().close_requested()) {
            self.ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_window();
        }
        // Sans icône, une fenêtre masquée ne pourrait plus revenir.
        if self.desk.hidden && !prefs.tray {
            self.show_window();
        }
        let count = if self.online() { self.desk.mentions.total(self.store.mentioned()) } else { 0 };
        let count = if prefs.badge { count } else { 0 };
        let title = notify::window_title(count);
        if self.desk.title.as_ref() != Some(&title) {
            self.ctx.send_viewport_cmd(ViewportCommand::Title(title.clone()));
            self.desk.title = Some(title);
        }
        let status = match self.store.account.value(Key::Status) {
            Some(SValue::Str(s)) if self.online() => Some(s.clone()),
            _ => None,
        };
        let state = TrayState {
            visible: prefs.tray,
            mentions: count,
            window_shown: !self.desk.hidden,
            lang: self.lang,
            status,
            accent: rgb(self.theme.colors.accent),
            badge: rgb(self.theme.colors.danger),
        };
        if self.desk.tray.as_ref() != Some(&state) {
            self.desk.backend.tray(&state);
            self.desk.tray = Some(state);
        }
        // Demande d'ami, événement qui commence, appel sortant (lot FF).
        self.sounds_tick();
    }

    /// Section « Bureau » de la page Notifications (sans session aussi).
    pub(super) fn desk_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        section(ui, t, nt::SECTION_DESKTOP.get(lang));
        note(ui, t, nt::DESKTOP_TIP.get(lang));
        let mut p = self.desk.prefs;
        if let Some(v) = toggle(ui, nt::TOASTS.get(lang), nt::TOASTS_TIP.get(lang), Some(p.toasts), true) {
            p.toasts = v;
        }
        {
            use crate::i18n::table::lotn::t as xt;
            if let Some(v) = toggle(ui, xt::TOAST_PREVIEW.get(lang), xt::TOAST_PREVIEW_TIP.get(lang), Some(p.preview), p.toasts) {
                p.preview = v;
            }
        }
        if let Some(v) = toggle(ui, nt::SOUNDS.get(lang), nt::SOUNDS_TIP.get(lang), Some(p.sounds), true) {
            p.sounds = v;
        }
        let mut test = false;
        let mut volume = p.volume;
        let mut save_volume = false;
        super::appearance::row(ui, nt::VOLUME.get(lang), nt::VOLUME_TIP.get(lang), |ui| {
            let r = ui.add_enabled(p.sounds, egui::Slider::new(&mut volume, 0..=100));
            // Enregistré au lâcher du curseur, pas à chaque pixel.
            save_volume = r.drag_stopped() || (r.changed() && !r.dragged());
            test = ui.add_enabled(p.sounds && volume > 0, egui::Button::new(nt::TEST_SOUND.get(lang))).clicked();
        });
        if let Some(v) = toggle(ui, nt::BADGE.get(lang), nt::BADGE_TIP.get(lang), Some(p.badge), true) {
            p.badge = v;
        }
        if let Some(v) = toggle(ui, nt::TRAY.get(lang), nt::TRAY_TIP.get(lang), Some(p.tray), true) {
            p.tray = v;
        }
        if let Some(v) = toggle(ui, nt::CLOSE_TO_TRAY.get(lang), nt::CLOSE_TO_TRAY_TIP.get(lang), Some(p.close_to_tray), p.tray) {
            p.close_to_tray = v;
        }
        p.volume = volume;
        let others = DeskPrefs { volume: self.desk.prefs.volume, ..p } != self.desk.prefs;
        if others || save_volume {
            self.desk.prefs = p;
            self.desk.touched = true;
            self.desk.backend.save_prefs(p);
        } else if volume != self.desk.prefs.volume {
            // Suivi pendant le glissement, écrit au lâcher.
            self.desk.prefs.volume = volume;
            self.desk.touched = true;
        }
        if test {
            self.desk.backend.sound(Kind::Mention, volume);
        }
        // Un interrupteur par son, comme la page « Sons » de Discord (lot FF).
        self.sounds_settings(ui, t);
    }
}
