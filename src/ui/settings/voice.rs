//! Page « Voix et vidéo » : périphériques, volumes, mode de saisie
//! (détection de la voix ou appui pour parler), traitement de la voix et
//! « Test du micro ». Les réglages vont dans `local.toml` (section `[voix]`),
//! la touche d'appui-pour-parler dans `keys.toml` ;
//! les périphériques ne sont touchés que par le service audio
//! (`state::audio_prefs::AudioService`), jamais d'ici.

use super::{choice, note, section, toggle, App, Page};
use crate::audio::io::{display_name, AudioError, DeviceInfo};
use crate::i18n::table::audio::{f as af, t as at};
use crate::i18n::table::s2::f as sf2;
use crate::i18n::Lang;
use crate::state::audio_prefs::{AudioEvent, AudioPrefs, AudioService, InputMode, Meter, NoAudio, RELEASE_MAX_MS, THRESHOLD_MIN_DB, VOLUME_MAX};
use crate::i18n::table::share::t as st;
use crate::state::keys::{Action, Chord};
use crate::state::video_prefs::{CameraEvent, CameraService, NoCamera};
use crate::theme::Tokens;
use crate::video::capture::Quality;
use eframe::egui::{self, Key, RichText, Sense};
use std::time::Duration;

/// Rafraîchissement du vumètre pendant le test (et seulement pendant le test).
const METER_REFRESH: Duration = Duration::from_millis(33);
/// Rafraîchissement du vumètre quand les animations sont réduites.
const METER_REFRESH_REDUCED: Duration = Duration::from_millis(250);

pub(in crate::ui) struct VoiceUi {
    backend: Box<dyn AudioService>,
    inputs: Option<Vec<DeviceInfo>>,
    outputs: Option<Vec<DeviceInfo>>,
    listing: bool,
    /// « Choisir la touche » : la prochaine touche devient le raccourci.
    capturing_key: bool,
    error: Option<AudioError>,
    /// Réglages pendant le glissement d'un curseur : appliqués au test tout
    /// de suite, enregistrés au lâcher.
    draft: Option<AudioPrefs>,
    /// Réglages des plugins vocaux donnés au test en cours.
    extras: crate::audio::dsp::Extras,
    /// Intensité de RNNoise pendant le glissement du curseur.
    rnn_draft: Option<i64>,
    /// Caméra de la section « Vidéo » : liste et « Tester la vidéo ».
    camera: Box<dyn CameraService>,
    cameras: Option<Vec<DeviceInfo>>,
    cam_listing: bool,
    /// Dernière image du test de la caméra (libérée à l'arrêt).
    cam_tex: Option<egui::TextureHandle>,
    cam_rgba: Vec<u8>,
    cam_error: Option<crate::video::capture::CaptureError>,
}

impl Default for VoiceUi {
    fn default() -> Self {
        VoiceUi {
            backend: Box::new(NoAudio),
            inputs: None,
            outputs: None,
            listing: false,
            capturing_key: false,
            error: None,
            draft: None,
            extras: Default::default(),
            rnn_draft: None,
            camera: Box::new(NoCamera),
            cameras: None,
            cam_listing: false,
            cam_tex: None,
            cam_rgba: Vec::new(),
            cam_error: None,
        }
    }
}

impl VoiceUi {
    /// Une touche est attendue : Échap l'annule au lieu de fermer la fenêtre.
    pub(in crate::ui) fn capturing_key(&self) -> bool {
        self.capturing_key
    }

    /// Arrête le test de la caméra et lâche son image.
    fn stop_camera(&mut self) {
        self.camera.stop();
        self.cam_tex = None;
        self.cam_rgba = Vec::new();
    }
}

/// Qualités du partage, dans l'ordre du sélecteur de partage.
const QUALITIES: [(Quality, crate::i18n::Text); 5] = [(Quality::HD_30, st::Q_720_30), (Quality::FHD_30, st::Q_1080_30), (Quality::FHD_60, st::Q_1080_60), (Quality::SOURCE, st::Q_SOURCE), (Quality::SOURCE_60, st::Q_SOURCE_60)];

const MODES: [(InputMode, crate::i18n::Text); 2] = [(InputMode::Voice, at::MODE_VOICE), (InputMode::PushToTalk, at::MODE_PTT)];

/// Texte d'une erreur du service audio.
fn error_text(lang: Lang, e: &AudioError, inputs: &[DeviceInfo], outputs: &[DeviceInfo]) -> String {
    match e {
        AudioError::NoDevice { input: true } => at::ERR_NO_INPUT.get(lang).to_string(),
        AudioError::NoDevice { input: false } => at::ERR_NO_OUTPUT.get(lang).to_string(),
        AudioError::DeviceLost { input: true, name } => af::lost_input(lang, display_name(name, inputs)),
        AudioError::DeviceLost { input: false, name } => af::lost_output(lang, display_name(name, outputs)),
        AudioError::Unsupported(m) => af::unsupported(lang, m),
        AudioError::Backend(m) if crate::audio::io::is_access_denied(m) => at::ERR_MIC_PRIVACY.get(lang).to_string(),
        AudioError::Backend(m) => af::audio_error(lang, m),
    }
}

/// Libellé du périphérique choisi (`None` : celui de Windows).
fn device_label(lang: Lang, current: Option<&str>, list: Option<&[DeviceInfo]>) -> String {
    let list = list.unwrap_or_default();
    match current {
        None => list.iter().find(|d| d.is_default).map_or_else(|| at::DEFAULT_DEVICE.get(lang).to_string(), |d| af::default_named(lang, &d.name)),
        Some(id) if list.iter().any(|d| d.id == id) => display_name(id, list).to_string(),
        Some(id) => af::unplugged(lang, id),
    }
}

/// Liste déroulante d'un périphérique ; renvoie le nouveau choix.
fn device_combo(ui: &mut egui::Ui, lang: Lang, id: &str, (label, detail): (&str, &str), current: Option<&str>, list: Option<&[DeviceInfo]>) -> Option<Option<String>> {
    let mut out = None;
    super::super::appearance::row(ui, label, detail, |ui| {
        let shown = device_label(lang, current, list);
        egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), at::DEFAULT_DEVICE.get(lang)).clicked() {
                out = Some(None);
            }
            match list {
                None => {
                    ui.label(at::LOADING_DEVICES.get(lang));
                }
                Some(list) => {
                    for d in list {
                        if ui.selectable_label(current == Some(d.id.as_str()), &d.name).clicked() {
                            out = Some(Some(d.id.clone()));
                        }
                    }
                }
            }
        });
    });
    out
}

/// Position d'un niveau (dB) sur la largeur du vumètre.
fn meter_x(rect: egui::Rect, db: f32) -> f32 {
    let share = ((db - f32::from(THRESHOLD_MIN_DB)) / -f32::from(THRESHOLD_MIN_DB)).clamp(0.0, 1.0);
    rect.left() + rect.width() * share
}

/// Vumètre : niveau du micro (couleur « en ligne » quand la porte est
/// ouverte), et trait du seuil en détection de la voix.
fn meter(ui: &mut egui::Ui, t: &Tokens, m: Option<Meter>, threshold: Option<f32>) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.typography.body), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, t.r_row(), t.colors.control_bg);
    if let Some(m) = m {
        let level = egui::Rect::from_min_max(rect.min, egui::pos2(meter_x(rect, m.level_db), rect.max.y));
        p.rect_filled(level, t.r_row(), if m.open { t.colors.online } else { t.colors.idle });
    }
    if let Some(th) = threshold {
        let x = meter_x(rect, th);
        p.line_segment([egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())], egui::Stroke::new(t.shapes.focus, t.colors.text_strong));
    }
}

impl App {
    /// Branche le vrai service audio (périphériques de Windows).
    pub fn attach_audio(&mut self, s: Box<dyn AudioService>) {
        self.voice_cfg.backend.stop_test();
        self.voice_cfg.backend = s;
        self.voice_cfg.inputs = None;
        self.voice_cfg.outputs = None;
        self.voice_cfg.listing = false;
    }

    /// Branche la vraie caméra de la section « Vidéo » (liste et test).
    pub fn attach_camera(&mut self, s: Box<dyn CameraService>) {
        self.voice_cfg.stop_camera();
        self.voice_cfg.camera = s;
        self.voice_cfg.cameras = None;
        self.voice_cfg.cam_listing = false;
    }

    /// Ouvre « Voix et vidéo » sur la section « Vidéo » (aperçu).
    pub fn open_video_settings(&mut self) {
        self.params.show_at(Page::Voice, Some(at::SEC_VIDEO), self.lang);
    }

    /// Le test de la caméra tourne (tests, aperçu).
    pub fn camera_test_running(&self) -> bool {
        self.voice_cfg.camera.running()
    }

    /// Réponses des services audio et caméra ; les tests s'arrêtent dès qu'on quitte la page.
    pub(in crate::ui) fn audio_settings_tick(&mut self) {
        for e in self.voice_cfg.camera.poll() {
            match e {
                CameraEvent::Cameras(list) => {
                    // La première est celle que prend « Par défaut ».
                    self.voice_cfg.cameras = Some(list.into_iter().enumerate().map(|(i, s)| DeviceInfo { id: s.id, name: s.name, is_default: i == 0 }).collect());
                    self.voice_cfg.cam_listing = false;
                }
                CameraEvent::Failed(e) => {
                    self.voice_cfg.stop_camera();
                    self.voice_cfg.cam_error = Some(e);
                }
            }
        }
        for e in self.voice_cfg.backend.poll() {
            match e {
                AudioEvent::Devices { inputs, outputs } => {
                    self.voice_cfg.inputs = Some(inputs);
                    self.voice_cfg.outputs = Some(outputs);
                    self.voice_cfg.listing = false;
                }
                AudioEvent::TestError(e) => self.voice_cfg.error = Some(e),
            }
        }
        let on_page = self.params.open && self.params.page() == Page::Voice;
        if !on_page {
            // Jamais de caméra allumée hors de la page.
            if self.voice_cfg.camera.running() || self.voice_cfg.cam_tex.is_some() {
                self.voice_cfg.stop_camera();
            }
            if self.voice_cfg.backend.testing() {
                self.voice_cfg.backend.stop_test();
            }
            self.voice_cfg.capturing_key = false;
            if let Some(v) = self.voice_cfg.draft.take() {
                self.commit_voice(v);
            }
        }
    }

    /// Périphériques d'entrée ou de sortie connus du service audio local,
    /// pour les menus du panneau utilisateur (lot profil) ; la liste est
    /// demandée une fois si elle manque (`None` tant qu'elle n'est pas là).
    pub(in crate::ui) fn audio_devices(&mut self, input: bool) -> Option<Vec<DeviceInfo>> {
        if self.voice_cfg.inputs.is_none() && !self.voice_cfg.listing {
            self.voice_cfg.listing = true;
            self.voice_cfg.backend.list_devices();
        }
        if input { self.voice_cfg.inputs.clone() } else { self.voice_cfg.outputs.clone() }
    }

    /// Caméras connues, pour le menu du chevron de la caméra (bandeau d'un
    /// appel) : la liste est demandée une fois si elle manque, sans en allumer aucune.
    pub(in crate::ui) fn camera_devices(&mut self) -> Option<Vec<DeviceInfo>> {
        if self.voice_cfg.cameras.is_none() && !self.voice_cfg.cam_listing {
            self.voice_cfg.cam_listing = true;
            self.voice_cfg.camera.list();
        }
        self.voice_cfg.cameras.clone()
    }

    /// Enregistre les réglages vidéo choisis hors de la page (menu de la caméra).
    pub(in crate::ui) fn video_prefs_set(&mut self, v: crate::state::video_prefs::VideoPrefs) {
        let mut p = self.local.prefs.clone();
        p.video = Box::new(v);
        self.set_local_prefs(p);
    }

    /// Enregistre les réglages audio choisis hors de la page (menus du panneau utilisateur).
    pub(in crate::ui) fn audio_prefs_set(&mut self, v: AudioPrefs) {
        self.commit_voice(v);
    }

    fn commit_voice(&mut self, v: AudioPrefs) {
        let mut p = self.local.prefs.clone();
        p.audio = v;
        self.set_local_prefs(p);
    }

    pub(super) fn page_voice(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if self.voice_cfg.inputs.is_none() && !self.voice_cfg.listing {
            self.voice_cfg.listing = true;
            self.voice_cfg.backend.list_devices();
        }
        note(ui, t, at::LOCAL_TIP.get(lang));
        let base = self.voice_cfg.draft.clone().unwrap_or_else(|| self.local.prefs.audio.clone());
        let mut v = base.clone();
        let mut dragging = false;
        let (inputs, outputs) = (self.voice_cfg.inputs.as_deref(), self.voice_cfg.outputs.as_deref());

        section(ui, t, at::SEC_VOICE.get(lang));
        if let Some(d) = device_combo(ui, lang, "voix_entree", (at::INPUT_DEVICE.get(lang), at::INPUT_DEVICE_TIP.get(lang)), v.input_device.as_deref(), inputs) {
            v.input_device = d;
        }
        if let Some(d) = device_combo(ui, lang, "voix_sortie", (at::OUTPUT_DEVICE.get(lang), at::OUTPUT_DEVICE_TIP.get(lang)), v.output_device.as_deref(), outputs) {
            v.output_device = d;
        }
        let mut refresh = false;
        super::super::appearance::row(ui, "", "", |ui| {
            refresh = ui.add_enabled(!self.voice_cfg.listing, egui::Button::new(at::REFRESH.get(lang))).clicked();
        });
        for (slot, label, tip) in [(&mut v.input_volume, at::INPUT_VOLUME, at::INPUT_VOLUME_TIP), (&mut v.output_volume, at::OUTPUT_VOLUME, at::OUTPUT_VOLUME_TIP)] {
            super::super::appearance::row(ui, label.get(lang), tip.get(lang), |ui| {
                let r = ui.add(egui::Slider::new(slot, 0..=VOLUME_MAX).custom_formatter(|n, _| sf2::percent(lang, n as u8)));
                super::super::a11y::name(ui.ctx(), r.id, label.get(lang));
                dragging |= r.dragged();
            });
        }

        section(ui, t, at::SEC_TEST.get(lang));
        note(ui, t, at::TEST_TIP.get(lang));
        let testing = self.voice_cfg.backend.testing();
        // Plugins vocaux : un changement (RNNoise, son intensité) vaut aussi pour le test en cours.
        let extras = crate::audio::dsp::Extras::current();
        let mut toggle_test = false;
        ui.horizontal(|ui| {
            toggle_test = ui.button(if testing { at::TEST_STOP } else { at::TEST_START }.get(lang)).clicked();
        });
        let m = self.voice_cfg.backend.meter();
        let shown_threshold = (v.mode == InputMode::Voice).then(|| if v.auto_threshold { m.map(|m| m.threshold_db) } else { Some(f32::from(v.threshold_db)) }).flatten();
        meter(ui, t, m, shown_threshold);
        if !testing {
            note(ui, t, at::METER_IDLE.get(lang));
        }
        if let Some(e) = &self.voice_cfg.error {
            let text = error_text(lang, e, inputs.unwrap_or_default(), outputs.unwrap_or_default());
            ui.add(egui::Label::new(RichText::new(text).small().color(t.colors.danger)).wrap());
        }

        section(ui, t, at::SEC_MODE.get(lang));
        if let Some(mode) = choice(ui, lang, "voix_mode", (at::MODE.get(lang), at::MODE_TIP.get(lang)), &MODES, Some(v.mode), true) {
            v.mode = mode;
        }
        match v.mode {
            InputMode::Voice => {
                if let Some(on) = toggle(ui, at::AUTO_THRESHOLD.get(lang), at::AUTO_THRESHOLD_TIP.get(lang), Some(v.auto_threshold), true) {
                    v.auto_threshold = on;
                }
                if !v.auto_threshold {
                    super::super::appearance::row(ui, at::THRESHOLD.get(lang), at::THRESHOLD_TIP.get(lang), |ui| {
                        let r = ui.add(egui::Slider::new(&mut v.threshold_db, THRESHOLD_MIN_DB..=0).custom_formatter(|n, _| af::db(lang, n as i32)));
                        super::super::a11y::name(ui.ctx(), r.id, at::THRESHOLD.get(lang));
                        dragging |= r.dragged();
                    });
                }
            }
            InputMode::PushToTalk => {
                self.ptt_key_row(ui);
                super::super::appearance::row(ui, at::PTT_RELEASE.get(lang), at::PTT_RELEASE_TIP.get(lang), |ui| {
                    let r = ui.add(egui::Slider::new(&mut v.ptt_release_ms, 0..=RELEASE_MAX_MS).custom_formatter(|n, _| af::ms(lang, n as u32)));
                    super::super::a11y::name(ui.ctx(), r.id, at::PTT_RELEASE.get(lang));
                    dragging |= r.dragged();
                });
            }
        }

        // Raccourcis globaux du micro et du casque (F22).
        if let Some(on) = toggle(ui, at::GLOBAL_KEYS.get(lang), at::GLOBAL_KEYS_TIP.get(lang), Some(v.global_keys), cfg!(windows)) {
            v.global_keys = on;
        }
        if base.global_keys && v.global_keys {
            use super::super::global_keys::GlobalState;
            for (a, state) in self.global_keys_states() {
                let name = super::super::keys::action_label(a).get(lang);
                match state {
                    GlobalState::Active => {}
                    GlobalState::Unsuitable => note(ui, t, &af::global_unsuitable(lang, name)),
                    GlobalState::Refused => note(ui, t, &af::global_refused(lang, name)),
                }
            }
        }

        section(ui, t, at::SEC_PROCESSING.get(lang));
        for (slot, label, tip) in [(&mut v.noise_suppression, at::NOISE, at::NOISE_TIP), (&mut v.echo_cancellation, at::ECHO, at::ECHO_TIP), (&mut v.auto_gain, at::AGC, at::AGC_TIP)] {
            if let Some(on) = toggle(ui, label.get(lang), tip.get(lang), Some(*slot), true) {
                *slot = on;
            }
        }

        // Plugin « Suppression de bruit RNNoise » (lot E4) : intensité et essai.
        if crate::plugins::on(crate::plugins::Id::NoiseRnn) {
            toggle_test |= self.rnnoise_row(ui, t, testing, v.noise_suppression);
        }

        self.video_section(ui, t);

        if refresh {
            self.voice_cfg.listing = true;
            self.voice_cfg.backend.list_devices();
            self.voice_cfg.cam_listing = true;
            self.voice_cfg.camera.list();
        }
        let devices_changed = v.input_device != base.input_device || v.output_device != base.output_device;
        if toggle_test {
            self.voice_cfg.error = None;
            if testing { self.voice_cfg.backend.stop_test() } else { self.voice_cfg.backend.start_test(&v) }
        } else if testing && devices_changed {
            self.voice_cfg.error = None;
            self.voice_cfg.backend.start_test(&v);
        } else if testing && (v != base || self.voice_cfg.extras != extras) {
            self.voice_cfg.backend.update_test(&v);
        }
        self.voice_cfg.extras = extras;
        if dragging {
            self.voice_cfg.draft = Some(v);
        } else {
            self.voice_cfg.draft = None;
            if v != self.local.prefs.audio {
                self.commit_voice(v);
            }
        }
        if self.voice_cfg.backend.testing() {
            // « Réduire les animations » : le vumètre avance par paliers (lot C5).
            ui.ctx().request_repaint_after(if self.local.reduced_motion() { METER_REFRESH_REDUCED } else { METER_REFRESH });
        }
    }

    /// Sections « Vidéo » et « Partage d'écran » : caméra des appels, test
    /// de la caméra (allumée seulement entre deux clics du bouton, jamais
    /// hors de la page), aperçu avant la caméra, qualité et son proposés
    /// dans le sélecteur de partage. Enregistrés dans `[video]`.
    fn video_section(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if self.voice_cfg.cameras.is_none() && !self.voice_cfg.cam_listing {
            self.voice_cfg.cam_listing = true;
            self.voice_cfg.camera.list();
        }
        let base = (*self.local.prefs.video).clone();
        let mut v = base.clone();

        section(ui, t, at::SEC_VIDEO.get(lang));
        if let Some(d) = device_combo(ui, lang, "video_camera", (at::CAMERA.get(lang), at::CAMERA_TIP.get(lang)), v.camera.as_deref(), self.voice_cfg.cameras.as_deref()) {
            v.camera = d;
        }
        if self.voice_cfg.cameras.as_ref().is_some_and(Vec::is_empty) {
            note(ui, t, st::NO_CAMERAS.get(lang));
        }
        let running = self.voice_cfg.camera.running();
        let mut toggle_cam = false;
        ui.horizontal(|ui| {
            toggle_cam = ui.button(if running { at::VIDEO_TEST_STOP } else { at::VIDEO_TEST }.get(lang)).clicked();
        });
        note(ui, t, at::VIDEO_TEST_TIP.get(lang));
        if running && let Some(f) = self.voice_cfg.camera.frame() {
            let rgba = &mut self.voice_cfg.cam_rgba;
            if f.width > 0 && crate::video::convert::frame_to_rgba(&f, rgba).is_ok() {
                let image = egui::ColorImage::from_rgba_unmultiplied([f.width as usize, f.height as usize], rgba);
                match &mut self.voice_cfg.cam_tex {
                    Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                    None => self.voice_cfg.cam_tex = Some(ui.ctx().load_texture("test-camera", image, egui::TextureOptions::LINEAR)),
                }
            }
        }
        let width = ui.available_width().min(t.layout.card_width);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(width, (width * 9.0 / 16.0).round()), Sense::hover());
        ui.painter().rect_filled(rect, t.r_media(), t.colors.bg0);
        match &self.voice_cfg.cam_tex {
            Some(tex) if running => crate::ui::share_picker::paint_image(ui, rect, tex),
            _ => {
                let text = if running { at::VIDEO_STARTING } else { at::VIDEO_IDLE }.get(lang);
                let galley = ui.painter().layout(text.to_string(), egui::FontId::proportional(t.typography.small), t.colors.faint, rect.width() - 2.0 * t.spacing.card_pad);
                ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.colors.faint);
            }
        }
        if let Some(e) = &self.voice_cfg.cam_error {
            ui.add(egui::Label::new(RichText::new(crate::ui::share_picker::error_text(lang, e)).small().color(t.colors.danger)).wrap());
        }
        if let Some(on) = toggle(ui, at::CAMERA_PREVIEW.get(lang), at::CAMERA_PREVIEW_TIP.get(lang), Some(v.camera_preview), true) {
            v.camera_preview = on;
        }

        section(ui, t, at::SEC_SHARE.get(lang));
        if let Some(q) = choice(ui, lang, "video_qualite_partage", (at::SHARE_QUALITY.get(lang), at::SHARE_QUALITY_TIP.get(lang)), &QUALITIES, Some(v.share_quality), true) {
            v.share_quality = q;
        }
        if let Some(on) = toggle(ui, at::SHARE_SOUND.get(lang), at::SHARE_SOUND_TIP.get(lang), Some(v.share_sound), true) {
            v.share_sound = on;
        }

        if toggle_cam {
            self.voice_cfg.cam_error = None;
            if running {
                self.voice_cfg.stop_camera();
            } else {
                self.voice_cfg.camera.start(v.camera.as_deref());
            }
        } else if running && v.camera != base.camera {
            // Autre caméra choisie pendant le test : il continue avec elle.
            self.voice_cfg.stop_camera();
            self.voice_cfg.cam_error = None;
            self.voice_cfg.camera.start(v.camera.as_deref());
        }
        if v != base {
            let mut p = self.local.prefs.clone();
            p.video = Box::new(v);
            self.set_local_prefs(p);
        }
        if self.voice_cfg.camera.running() && self.voice_cfg.cam_tex.is_none() {
            // En attendant la première image (le service réveille ensuite à chaque image).
            ui.ctx().request_repaint_after(METER_REFRESH_REDUCED);
        }
    }

    /// Ligne du plugin RNNoise : curseur d'intensité (enregistré au lâcher)
    /// et bouton « S'entendre », qui lance ou arrête le test du micro. Rend
    /// vrai si le bouton est cliqué.
    fn rnnoise_row(&mut self, ui: &mut egui::Ui, t: &Tokens, testing: bool, noise_on: bool) -> bool {
        use crate::i18n::table::e4::{f as xf, t as xt};
        use crate::plugins::{self, Id, Value};
        let lang = self.lang;
        let saved = self.local.prefs.plugins.number(Id::NoiseRnn, plugins::RNN_STRENGTH);
        let mut n = self.voice_cfg.rnn_draft.unwrap_or(saved);
        let (mut hear, mut dragged) = (false, false);
        super::super::appearance::row(ui, xt::RNN_STRENGTH.get(lang), xt::RNN_STRENGTH_TIP.get(lang), |ui| {
            let r = ui.add(egui::Slider::new(&mut n, 10..=100).custom_formatter(|n, _| xf::percent(lang, n as i64)));
            super::super::a11y::name(ui.ctx(), r.id, xt::RNN_STRENGTH.get(lang));
            dragged = r.dragged();
            hear = ui.button(if testing { xt::RNN_HEAR_STOP } else { xt::RNN_HEAR }.get(lang)).clicked();
        });
        if !noise_on {
            note(ui, t, xt::RNN_NEEDS_NOISE.get(lang));
        }
        // Appliqué tout de suite (le test l'entend) ; écrit dans le fichier au lâcher.
        let was_dragging = self.voice_cfg.rnn_draft.is_some();
        self.voice_cfg.rnn_draft = dragged.then_some(n);
        if n != saved {
            let mut p = self.local.prefs.clone();
            p.plugins.put(Id::NoiseRnn, plugins::RNN_STRENGTH, Value::Int(n));
            self.local.prefs = p;
            self.plugins_sync();
        }
        if !dragged && (n != saved || was_dragging) {
            self.save_local_prefs();
        }
        hear
    }

    /// Raccourci de l'appui pour parler : touche actuelle, « Choisir la
    /// touche » (la prochaine touche pressée, avec ses modificateurs),
    /// « Retirer ». Une seule source : l'action `vocal_appui_pour_parler` de
    /// `keys.toml`, la même que la page « Raccourcis clavier ».
    fn ptt_key_row(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        if self.voice_cfg.capturing_key {
            let pressed = ui.input(|i| {
                i.events.iter().find_map(|e| match e {
                    egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, mods)) = pressed {
                if key != Key::Escape {
                    self.set_shortcut(Action::PushToTalk, Some(crate::ui::keys::chord_of(key, mods)));
                }
                self.voice_cfg.capturing_key = false;
                ui.input_mut(|i| i.consume_key(mods, key));
            }
        }
        let capturing = self.voice_cfg.capturing_key;
        let current = self.shortcut(Action::PushToTalk).map(Chord::to_text);
        let mut start = false;
        let mut clear = false;
        super::super::appearance::row(ui, at::PTT_KEY.get(lang), at::PTT_KEY_TIP.get(lang), |ui| {
            let shown = if capturing { at::PTT_WAITING.get(lang).to_string() } else { current.clone().unwrap_or_else(|| at::PTT_NONE.get(lang).to_string()) };
            ui.label(RichText::new(shown).family(crate::ui::fonts::demi()).strong());
            start = ui.add_enabled(!capturing, egui::Button::new(at::PTT_SET.get(lang))).clicked();
            clear = current.is_some() && !capturing && ui.button(at::PTT_CLEAR.get(lang)).clicked();
        });
        if start {
            self.voice_cfg.capturing_key = true;
        }
        if clear {
            self.set_shortcut(Action::PushToTalk, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, name: &str, default: bool) -> DeviceInfo {
        DeviceInfo { id: id.into(), name: name.into(), is_default: default }
    }

    #[test]
    fn device_labels() {
        let list = [dev("a", "Casque", true), dev("b", "Micro USB", false)];
        assert_eq!(device_label(Lang::Fr, None, Some(&list)), "Par défaut (Windows)\u{a0}: Casque");
        assert_eq!(device_label(Lang::En, None, None), "Default (Windows)");
        assert_eq!(device_label(Lang::Fr, Some("b"), Some(&list)), "Micro USB");
        assert_eq!(device_label(Lang::En, Some("zz"), Some(&list)), "zz (unplugged)");
    }

    #[test]
    fn errors_name_the_lost_device() {
        let list = [dev("b", "Micro USB", false)];
        let e = AudioError::DeviceLost { input: true, name: "b".into() };
        assert!(error_text(Lang::Fr, &e, &list, &[]).contains("Micro USB"));
        assert_eq!(error_text(Lang::En, &AudioError::NoDevice { input: false }, &[], &[]), "No audio output available.");
    }

    #[test]
    fn meter_scale() {
        let r = egui::Rect::from_min_size(egui::pos2(10.0, 0.0), egui::vec2(100.0, 8.0));
        assert_eq!(meter_x(r, -100.0), 10.0);
        assert_eq!(meter_x(r, 0.0), 110.0);
        assert_eq!(meter_x(r, -50.0), 60.0);
        assert_eq!(meter_x(r, 12.0), 110.0);
    }
}

/// Section « Vidéo » : lignes, persistance, caméra jamais allumée sans clic
/// ni hors de la page. Caméra et capture simulées : rien de réel n'est ouvert.
#[cfg(test)]
mod video_tests {
    use super::*;
    use crate::state::share::{CaptureService, ShareEvent, ShareTab, SourceLists};
    use crate::state::voice::{VoiceCmd, VoiceEvent, VoiceState};
    use crate::state::voice_link::{VoiceLink, VoiceWant};
    use crate::state::{Event, Store};
    use crate::ui::tests::{find, offline_boot};
    use crate::video::capture::{Source, SourceKind};
    use crate::video::{Frame, FrameData};
    use std::sync::{Arc, Mutex};

    const ME: u64 = 1;
    const G: u64 = 10;
    const VOC: u64 = 20;

    struct Offline;
    impl crate::ui::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    fn cam(id: &str, name: &str) -> Source {
        Source { id: id.into(), name: name.into(), kind: SourceKind::Camera }
    }

    /// Ce que la fausse caméra a vu passer.
    #[derive(Default)]
    struct Seen {
        lists: usize,
        /// Caméra de chaque démarrage (`None` : par défaut).
        starts: Vec<Option<String>>,
        stops: usize,
        running: bool,
    }

    /// Fausse caméra : deux caméras inventées, une image grise par passe.
    #[derive(Clone, Default)]
    struct FakeCamera {
        seen: Arc<Mutex<Seen>>,
        queue: Arc<Mutex<Vec<CameraEvent>>>,
    }

    impl CameraService for FakeCamera {
        fn list(&mut self) {
            self.seen.lock().unwrap().lists += 1;
            self.queue.lock().unwrap().push(CameraEvent::Cameras(vec![cam("camera:a", "Webcam intégrée"), cam("camera:b", "Webcam USB")]));
        }
        fn start(&mut self, camera: Option<&str>) {
            let mut s = self.seen.lock().unwrap();
            s.starts.push(camera.map(str::to_string));
            s.running = true;
        }
        fn stop(&mut self) {
            let mut s = self.seen.lock().unwrap();
            if s.running {
                s.stops += 1;
            }
            s.running = false;
        }
        fn running(&self) -> bool {
            self.seen.lock().unwrap().running
        }
        fn frame(&mut self) -> Option<Frame> {
            Some(Frame { width: 4, height: 2, pts_us: 0, data: FrameData::Bgra(vec![128; 32]) })
        }
        fn poll(&mut self) -> Vec<CameraEvent> {
            std::mem::take(&mut *self.queue.lock().unwrap())
        }
    }

    fn app_with(store: Store) -> (App, FakeCamera) {
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Offline), store, images));
        app.lang = Lang::Fr;
        let fake = FakeCamera::default();
        app.attach_camera(Box::new(fake.clone()));
        (app, fake)
    }

    /// Une passe sur un écran assez haut pour toute la page (rien à faire défiler).
    fn frame(app: &mut App, events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.enable_accesskit();
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 2600.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
        out
    }

    fn settle(app: &mut App) -> egui::FullOutput {
        for _ in 0..40 {
            frame(app, Vec::new());
        }
        frame(app, Vec::new())
    }

    fn click(app: &mut App, label: &str) {
        let out = settle(app);
        let at = find(&out, label).unwrap_or_else(|| panic!("« {label} » introuvable")).center();
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(app, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(app, vec![button(false)]);
        frame(app, Vec::new());
    }

    #[test]
    fn video_rows_replace_the_phase_note() {
        let (mut app, fake) = app_with(Store::new());
        app.show_settings(Page::Voice);
        let out = settle(&mut app);
        let fr = Lang::Fr;
        for label in [at::SEC_VIDEO, at::VIDEO_TEST, at::CAMERA_PREVIEW, at::SEC_SHARE, at::SHARE_SOUND] {
            assert!(find(&out, label.get(fr)).is_some(), "« {} » attendu", label.get(fr));
        }
        assert!(find(&out, "phase").is_none(), "plus de renvoi à une phase");
        // Caméras listées une fois, aucune allumée.
        assert_eq!(device_label(fr, None, app.voice_cfg.cameras.as_deref()), "Par défaut (Windows)\u{a0}: Webcam intégrée", "« Par défaut » nomme la première caméra");
        let seen = fake.seen.lock().unwrap();
        assert_eq!((seen.lists, seen.starts.len(), seen.running), (1, 0, false));
    }

    #[test]
    fn settings_are_saved_and_feed_the_share_picker() {
        let (mut app, _fake) = app_with(Store::new());
        assert_eq!(*app.local_prefs().video, crate::state::video_prefs::VideoPrefs::default());
        app.show_settings(Page::Voice);
        click(&mut app, at::SHARE_SOUND.get(Lang::Fr));
        click(&mut app, at::CAMERA_PREVIEW.get(Lang::Fr));
        let saved = app.local_prefs();
        assert!(saved.video.share_sound && !saved.video.camera_preview);
        // Par le fichier : relu tel quel.
        assert_eq!(crate::state::local::LocalPrefs::parse(&saved.to_toml()).unwrap().video, saved.video);
        // Le sélecteur de partage part de ces valeurs.
        let mut p = app.local_prefs();
        p.video.share_quality = Quality::FHD_60;
        app.set_local_prefs(p);
        app.open_share_picker(ShareTab::Screens);
        assert_eq!(app.share_picker_defaults(), Some((Quality::FHD_60, true)));
    }

    #[test]
    fn camera_test_needs_a_click_and_stops_with_the_page() {
        let (mut app, fake) = app_with(Store::new());
        let mut p = app.local_prefs();
        p.video.camera = Some("camera:b".into());
        app.set_local_prefs(p);
        app.show_settings(Page::Voice);
        settle(&mut app);
        assert!(!app.camera_test_running(), "ouvrir la page n'allume rien");
        let fr = Lang::Fr;
        click(&mut app, at::VIDEO_TEST.get(fr));
        assert!(app.camera_test_running());
        assert_eq!(fake.seen.lock().unwrap().starts, vec![Some("camera:b".to_string())], "la caméra choisie");
        frame(&mut app, Vec::new());
        assert!(app.voice_cfg.cam_tex.is_some(), "l'image du test est affichée");
        // Deuxième clic : arrêt.
        click(&mut app, at::VIDEO_TEST_STOP.get(fr));
        assert!(!app.camera_test_running() && app.voice_cfg.cam_tex.is_none());
        // Autre page : arrêt.
        click(&mut app, at::VIDEO_TEST.get(fr));
        assert!(app.camera_test_running());
        app.show_settings(Page::Appearance);
        frame(&mut app, Vec::new());
        assert!(!app.camera_test_running() && app.voice_cfg.cam_tex.is_none(), "coupée en quittant la page");
        // Paramètres fermés : arrêt.
        app.show_settings(Page::Voice);
        click(&mut app, at::VIDEO_TEST.get(fr));
        assert!(app.camera_test_running());
        app.params.open = false;
        frame(&mut app, Vec::new());
        assert!(!app.camera_test_running(), "coupée à la fermeture des paramètres");
        assert_eq!(fake.seen.lock().unwrap().stops, 3);
        // Panne annoncée par le service : test arrêté, message affiché.
        app.show_settings(Page::Voice);
        click(&mut app, at::VIDEO_TEST.get(fr));
        fake.queue.lock().unwrap().push(CameraEvent::Failed(crate::video::capture::CaptureError::CameraDenied));
        settle(&mut app);
        assert!(!app.camera_test_running());
        assert_eq!(app.voice_cfg.cam_error, Some(crate::video::capture::CaptureError::CameraDenied), "le refus de Windows est gardé pour être expliqué");
    }

    /// Orchestrateur espion : retient la caméra demandée.
    #[derive(Clone, Default)]
    struct Spy(Arc<Mutex<Option<Option<String>>>>);
    impl VoiceLink for Spy {
        fn sync(&mut self, _: &VoiceWant<'_>) {}
        fn command(&mut self, _: &VoiceCmd) {}
        fn request_codes(&mut self) {}
        fn poll(&mut self) -> Vec<VoiceEvent> {
            Vec::new()
        }
        fn sync_video(&mut self, want: &crate::state::video_link::VideoWant<'_>) {
            *self.0.lock().unwrap() = want.camera.map(|c| c.map(str::to_string));
        }
    }

    /// Fausse capture : deux caméras, retient les aperçus demandés.
    #[derive(Clone, Default)]
    struct FakeCapture {
        queue: Arc<Mutex<Vec<ShareEvent>>>,
        previews: Arc<Mutex<Vec<String>>>,
    }
    impl CaptureService for FakeCapture {
        fn watch(&mut self, _: ShareTab) {
            let lists = SourceLists { cameras: vec![cam("camera:a", "Webcam intégrée"), cam("camera:b", "Webcam USB")], ..Default::default() };
            self.queue.lock().unwrap().push(ShareEvent::Sources(lists));
        }
        fn preview(&mut self, s: &Source) {
            self.previews.lock().unwrap().push(s.id.clone());
        }
        fn cancel(&mut self) {}
        fn poll(&mut self) -> Vec<ShareEvent> {
            std::mem::take(&mut *self.queue.lock().unwrap())
        }
    }

    fn in_call() -> (App, Spy, FakeCapture) {
        let state = |user_id| VoiceState { user_id, guild_id: Some(G), channel_id: VOC, ..Default::default() };
        let mut s = Store::new();
        s.apply(Event::Ready { me: crate::state::model::User { id: ME, name: "moi".into(), avatar: crate::state::model::Avatar::default_for(ME) }, guilds: vec![], dms: vec![] });
        s.voice_join_call(VOC);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(state(ME)), session_id: Some("sess".into()) }));
        s.apply(Event::Voice(VoiceEvent::Connected));
        s.take_commands();
        let (mut app, _) = app_with(s);
        let (spy, capture) = (Spy::default(), FakeCapture::default());
        app.attach_voice(Box::new(spy.clone()));
        app.attach_capture(Box::new(capture.clone()));
        let mut p = app.local_prefs();
        p.video.camera = Some("camera:b".into());
        app.set_local_prefs(p);
        (app, spy, capture)
    }

    #[test]
    fn camera_button_previews_first_when_asked() {
        let (mut app, spy, capture) = in_call();
        // Par défaut : l'aperçu d'abord, sur la caméra des réglages ; rien n'est envoyé.
        app.toggle_camera();
        settle(&mut app);
        assert!(app.share_picker_open() && !app.store.video.camera);
        assert_eq!(*capture.previews.lock().unwrap(), vec!["camera:b".to_string()]);
        assert_eq!(*spy.0.lock().unwrap(), None);
    }

    #[test]
    fn camera_button_uses_the_chosen_camera_without_preview() {
        let (mut app, spy, capture) = in_call();
        let mut p = app.local_prefs();
        p.video.camera_preview = false;
        app.set_local_prefs(p);
        app.toggle_camera();
        frame(&mut app, Vec::new());
        assert!(!app.share_picker_open() && app.store.video.camera);
        assert!(capture.previews.lock().unwrap().is_empty());
        assert_eq!(*spy.0.lock().unwrap(), Some(Some("camera:b".to_string())), "la caméra des réglages part vers l'appel");
        // Éteindre ne passe jamais par l'aperçu.
        let mut p = app.local_prefs();
        p.video.camera_preview = true;
        app.set_local_prefs(p);
        app.toggle_camera();
        frame(&mut app, Vec::new());
        assert!(!app.share_picker_open() && !app.store.video.camera);
        assert_eq!(*spy.0.lock().unwrap(), None);
    }
}
