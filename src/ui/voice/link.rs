//! Lien avec l'orchestrateur du vocal (lot V7) : ce que l'interface veut,
//! ce qui revient, et la fenêtre des codes de vérification DAVE.

use super::App;
use crate::i18n::table::voice::t as vt;
use crate::state::voice::{VoiceCmd, VoiceConnState};
use crate::state::voice_link::VoiceLink;
use crate::state::Event;
use eframe::egui::{self, RichText};

/// Code de l'appel, puis (nom, code) de chaque participant.
type CodeRows = (Option<String>, Vec<(String, Option<String>)>);

impl App {
    /// Branche le vrai orchestrateur (réseau et périphériques).
    pub fn attach_voice(&mut self, link: Box<dyn VoiceLink>) {
        self.voice.link = link;
    }

    /// L'orchestrateur, pour le lecteur de fichiers du fil (lot M).
    pub(in crate::ui) fn media_link(&mut self) -> &mut dyn VoiceLink {
        &mut *self.voice.link
    }

    /// Ouvre la fenêtre des codes sans les redemander (aperçu).
    pub fn show_voice_codes(&mut self) {
        self.voice.codes_open = true;
    }

    /// Ce que l'interface veut, puis ce qui est revenu (deux fois si l'état
    /// a changé, pour que l'orchestrateur suive sans attendre une autre image).
    pub(in crate::ui) fn voice_link_tick(&mut self) {
        self.video_link_tick();
        for _ in 0..2 {
            let want = self.store.voice_want(&self.local.prefs.audio);
            self.voice.link.sync(&want);
            let events = self.voice.link.poll();
            if events.is_empty() {
                break;
            }
            for e in events {
                self.store.apply(Event::Voice(e));
            }
        }
    }

    /// Vidéo (lot M5) : choix validé dans le sélecteur de partage, puis ce
    /// que l'interface veut (caméra, partage, directs, qualités), puis ce qui
    /// est revenu (connexions des directs, pannes à afficher). Bon marché
    /// quand rien ne change : des comparaisons, aucun réveil.
    fn video_link_tick(&mut self) {
        if let Some(c) = self.take_share_choice() {
            if c.source.kind == crate::video::capture::SourceKind::Camera {
                // La caméra choisie remplace celle en cours, ou s'allume.
                self.store.video.camera_source = Some(c.source.id);
                self.store.video_set_camera(true);
            } else {
                self.store.video_start_share(&c.source.id, &c.source.name, c.quality, c.audio);
            }
        }
        let mut want = self.store.video_want();
        // Aucune caméra choisie dans le sélecteur : celle de la page « Voix et vidéo ».
        if want.camera == Some(None) {
            want.camera = Some(self.local.prefs.video.camera.as_deref());
        }
        self.voice.link.sync_video(&want);
        for u in self.voice.link.poll_video() {
            if let Some(text) = self.store.apply_video_up(u) {
                self.notify(text);
            }
        }
    }

    /// Commandes vocales qui partent aussi vers l'audio (volume, muet par personne, appui-pour-parler).
    pub(in crate::ui) fn voice_link_command(&mut self, c: &VoiceCmd) {
        self.voice.link.command(c);
    }

    /// Lien « Codes de vérification » du panneau de connexion.
    pub(super) fn voice_codes_link(&mut self, ui: &mut egui::Ui) {
        if !matches!(self.store.voice.conn, VoiceConnState::Connected { .. }) {
            return;
        }
        let t = &self.theme;
        let r = ui.add(egui::Label::new(RichText::new(vt::CODES.get(self.lang)).small().color(t.colors.link)).sense(egui::Sense::click()).selectable(false));
        if r.on_hover_text(vt::CODES_TIP.get(self.lang)).clicked() {
            self.voice.codes_open = true;
            self.store.voice.codes = None;
            self.voice.link.request_codes();
        }
    }

    /// Fenêtre des codes : celui de l'appel, puis un par participant.
    pub(super) fn voice_codes_window(&mut self, ui: &mut egui::Ui) {
        if !self.voice.codes_open {
            return;
        }
        if self.store.voice.me.is_none() {
            self.voice.codes_open = false;
            return;
        }
        let lang = self.lang;
        let rows: Option<CodeRows> = self.store.voice.codes.clone().map(|c| {
            let users = c
                .users
                .iter()
                .map(|(u, code)| {
                    let name = self.store.voice.state(*u).map(|s| self.voice_who(s).0).unwrap_or_else(|| vt::UNKNOWN_USER.get(lang).to_string());
                    (name, code.clone())
                })
                .collect();
            (c.call, users)
        });
        let t = self.theme.clone();
        // Gabarit commun des fenêtres : titre et croix, codes qui défilent, pas de pied.
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::sheet(ui.ctx(), &t, egui::Id::unique("codes-vocal"), width, vt::CODES.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.add(egui::Label::new(RichText::new(vt::CODES_TIP.get(lang)).small().color(t.colors.muted)).wrap());
            ui.add_space(t.spacing.item_y);
            match &rows {
                None => {
                    ui.label(vt::CODES_LOADING.get(lang));
                }
                Some((None, _)) => {
                    ui.label(vt::NO_E2EE.get(lang));
                }
                Some((Some(call), users)) => {
                    ui.label(RichText::new(vt::CALL_CODE.get(lang)).family(crate::ui::fonts::demi()).strong());
                    ui.label(RichText::new(call).monospace());
                    for (name, code) in users {
                        ui.add_space(t.spacing.item_y);
                        ui.label(RichText::new(name).family(crate::ui::fonts::demi()).strong());
                        ui.label(RichText::new(code.as_deref().unwrap_or(vt::CODE_UNKNOWN.get(lang))).monospace());
                    }
                }
            }
        });
        if modal.closed {
            self.voice.codes_open = false;
        }
    }
}
