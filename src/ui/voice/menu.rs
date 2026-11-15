//! Menu d'une personne connectée (clic droit) : volume de 0 à 200 %, muet
//! pour moi ; pour les modérateurs, muet et sourdine côté serveur,
//! déplacement, déconnexion, chacun confirmé avec une raison facultative
//! (journal d'audit).

use super::{App, VoiceAct};
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::state::voice::{VoiceMod, VoiceState};
use eframe::egui::{self, Id, RichText};

/// Modération en attente de confirmation.
pub(super) struct Confirm {
    user: u64,
    name: String,
    action: VoiceMod,
    reason: String,
}

impl App {
    /// Volume d'une personne depuis son menu (lot FF2, F27) : le même curseur
    /// que dans la liste vocale, tant qu'elle est en vocal quelque part.
    pub(in crate::ui) fn user_menu_volume(&mut self, ui: &mut egui::Ui, user: u64) {
        if self.store.voice.state(user).is_none() || self.store.me.as_ref().is_some_and(|m| m.id == user) {
            return;
        }
        let lang = self.lang;
        let t = self.theme.clone();
        let prefs = self.voice_user_prefs(user);
        let mut acts = Vec::new();
        crate::ui::menu::separator(ui, &t);
        ui.label(RichText::new(vt::USER_VOLUME.get(lang)).small().color(t.colors.muted));
        let mut volume = prefs.volume;
        let r = ui.add(egui::Slider::new(&mut volume, 0..=200).custom_formatter(|v, _| vf::volume(lang, v as u16)));
        if r.changed() || r.drag_stopped() {
            acts.push(VoiceAct::Volume { user, volume, done: !r.dragged() });
        }
        let mut muted = prefs.muted;
        if crate::ui::menu::check(ui, &t, vt::LOCAL_MUTE.get(lang), &mut muted) {
            acts.push(VoiceAct::LocalMute(user, muted));
        }
        self.voice_acts(acts);
    }

    pub(super) fn voice_user_menu(&self, ui: &mut egui::Ui, s: &VoiceState, acts: &mut Vec<VoiceAct>) {
        let lang = self.lang;
        let t = &self.theme;
        let user = s.user_id;
        let me = self.store.me.as_ref().is_some_and(|m| m.id == user);
        if !me {
            let prefs = self.voice_user_prefs(user);
            ui.label(RichText::new(vt::USER_VOLUME.get(lang)).small().color(t.colors.muted));
            let mut volume = prefs.volume;
            let r = ui.add(egui::Slider::new(&mut volume, 0..=200).custom_formatter(|v, _| vf::volume(lang, v as u16)));
            if r.changed() || r.drag_stopped() {
                // Relâché (ou clavier, clic) : enregistré ; pendant le glissé, seulement appliqué.
                acts.push(VoiceAct::Volume { user, volume, done: !r.dragged() });
            }
            let mut muted = prefs.muted;
            if ui.checkbox(&mut muted, vt::LOCAL_MUTE.get(lang)).changed() {
                acts.push(VoiceAct::LocalMute(user, muted));
            }
        }
        let can = |a: VoiceMod| self.store.voice_can_moderate(user, a);
        let mut picks: Vec<(VoiceMod, crate::i18n::Text)> = Vec::new();
        if can(VoiceMod::Mute(!s.mute)) {
            picks.push((VoiceMod::Mute(!s.mute), if s.mute { vt::SERVER_UNMUTE } else { vt::SERVER_MUTE }));
        }
        if can(VoiceMod::Deafen(!s.deaf)) {
            picks.push((VoiceMod::Deafen(!s.deaf), if s.deaf { vt::SERVER_UNDEAFEN } else { vt::SERVER_DEAFEN }));
        }
        let targets: Vec<(u64, String)> = s
            .guild_id
            .map(|g| self.store.voice.channels(g).iter().filter(|c| can(VoiceMod::Move(c.id))).map(|c| (c.id, c.name.clone())).collect())
            .unwrap_or_default();
        let disconnect = can(VoiceMod::Disconnect);
        if picks.is_empty() && targets.is_empty() && !disconnect {
            return;
        }
        if !me {
            crate::ui::menu::separator(ui, t);
        }
        for (action, label) in picks {
            if crate::ui::menu::item(ui, t, label.get(lang), Some(crate::theme::icons::Icon::MicOff), false).clicked() {
                acts.push(VoiceAct::Moderate { user, action });
                ui.close();
            }
        }
        if !targets.is_empty() {
            crate::ui::menu::submenu(ui, t, vt::MOVE_TO.get(lang), |ui| {
                for (id, channel) in targets {
                    if crate::ui::menu::item(ui, t, &channel, Some(crate::theme::icons::Icon::Speaker), false).clicked() {
                        acts.push(VoiceAct::Moderate { user, action: VoiceMod::Move(id) });
                        ui.close();
                    }
                }
            });
        }
        if disconnect && crate::ui::menu::item(ui, t, vt::DISCONNECT.get(lang), Some(crate::theme::icons::Icon::HangUp), true).clicked() {
            acts.push(VoiceAct::Moderate { user, action: VoiceMod::Disconnect });
            ui.close();
        }
    }

    /// Ouvre la confirmation d'une modération.
    pub(super) fn voice_ask(&mut self, user: u64, action: VoiceMod) {
        let Some(s) = self.store.voice.state(user).cloned() else { return };
        let (name, _) = self.voice_who(&s);
        self.voice.confirm = Some(Confirm { user, name, action, reason: String::new() });
    }

    /// Fenêtre de confirmation (au-dessus du reste).
    pub(super) fn voice_confirm_ui(&mut self, ui: &egui::Ui) {
        // La personne est partie ou le droit a disparu : plus rien à confirmer.
        if self.voice.confirm.as_ref().is_some_and(|c| !self.store.voice_can_moderate(c.user, c.action)) {
            self.voice.confirm = None;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let target = |id: u64| self.store.voice.channel(id).map(|c| c.name.clone()).unwrap_or_default();
        let dest = match self.voice.confirm.as_ref().map(|c| c.action) {
            Some(VoiceMod::Move(id)) => target(id),
            _ => String::new(),
        };
        let Some(c) = self.voice.confirm.as_mut() else { return };
        let (mut yes, mut no) = (false, false);
        let question = match c.action {
            VoiceMod::Mute(true) => vf::confirm_mute(lang, &c.name),
            VoiceMod::Mute(false) => vf::confirm_unmute(lang, &c.name),
            VoiceMod::Deafen(true) => vf::confirm_deafen(lang, &c.name),
            VoiceMod::Deafen(false) => vf::confirm_undeafen(lang, &c.name),
            VoiceMod::Move(_) => vf::confirm_move(lang, &c.name, &dest),
            VoiceMod::Disconnect => vf::confirm_disconnect(lang, &c.name),
        };
        let danger = matches!(c.action, VoiceMod::Disconnect | VoiceMod::Mute(true) | VoiceMod::Deafen(true));
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("vocal-moderation"), width, &question, crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::widgets::field_label(ui, &t, vt::REASON.get(lang));
            crate::ui::form::field(ui, &t, &mut c.reason, "", crate::state::admin::moderation::REASON_MAX);
        }, |ui| {
            let confirm = vt::CONFIRM.get(lang);
            yes = ui.add(if danger { crate::ui::form::danger(&t, confirm) } else { crate::ui::form::primary(&t, confirm) }).clicked();
            no = ui.add(crate::ui::form::link(&t, vt::CANCEL.get(lang))).clicked();
        });
        if yes && let Some(c) = self.voice.confirm.take() {
            self.store.voice_moderate(c.user, c.action, Some(c.reason));
        } else if no || modal.closed {
            self.voice.confirm = None;
        }
    }
}
