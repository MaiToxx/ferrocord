//! Réglages de cet ordinateur (`state::local`) côté interface : lecture et
//! enregistrement par `machine::Machine`, effets sur egui (animations,
//! infobulles), saturation du thème, journal local, et les pages
//! Accessibilité, Mode streamer et Avancés.

use super::{choice, note, section, toggle, value_row, App, Page};
use crate::i18n::table::s2::{f as sf2, t as s2};
use crate::machine::{Machine, MachineEvent, Quiet};
use crate::state::local::{self, Display, Intensity, LocalPrefs, Motion};
use crate::state::usersettings::schema::Key;
use crate::state::Store;
use crate::theme::{FieldMut, Preset, Tokens};
use eframe::egui::{self, Color32, Id};

pub(in crate::ui) struct LocalUi {
    pub(in crate::ui) prefs: LocalPrefs,
    /// Changé avant la lecture du fichier : la lecture ne l'écrase pas.
    touched: bool,
    backend: Box<dyn Machine>,
    windows_reduced: Option<bool>,
    /// Lignes par cran de molette réglées dans Windows (`None` : inconnu).
    wheel_lines: Option<u32>,
    /// Distance d'un cran appliquée à egui (lignes de Windows, vitesse en %).
    scroll_applied: Option<(Option<u32>, u16)>,
    /// Animations réduites et infobulles immédiates appliquées à egui.
    applied: Option<(bool, bool)>,
    shown: Option<Display>,
    /// Préréglage d'avant « Contraste renforcé » (retour quand on le décoche).
    before_contrast: Option<Preset>,
    /// `local.toml` lu : ses choix (historique hors ligne compris) sont connus.
    pub(in crate::ui) loaded: bool,
    /// Page « Plugins » et état publié pour `plugins::on` (lot P0).
    pub(in crate::ui) plugins: crate::ui::plugins::PluginsUi,
}

impl LocalUi {
    /// « Réduire les animations » tel qu'appliqué à egui.
    pub(in crate::ui) fn reduced_motion(&self) -> bool {
        self.applied.is_some_and(|(reduced, _)| reduced)
    }

    /// Intensité effective des animations de l'interface (lot ANIM).
    pub(in crate::ui) fn motion_level(&self) -> crate::ui::motion::Level {
        // Plugin « Animations de l'interface » coupé : tout est immédiat.
        if !crate::plugins::on(crate::plugins::Id::Animations) {
            return crate::ui::motion::Level::None;
        }
        crate::ui::motion::Level::of(self.prefs.intensity, self.reduced_motion())
    }
}

impl Default for LocalUi {
    fn default() -> Self {
        LocalUi { prefs: LocalPrefs::default(), touched: false, backend: Box::new(Quiet), windows_reduced: None, wheel_lines: None, scroll_applied: None, applied: None, shown: None, before_contrast: None, loaded: false, plugins: Default::default() }
    }
}

fn display_id() -> Id {
    Id::unique("ferrocord.local.display")
}

/// Ce que le rendu doit masquer ou changer (réglages de cet ordinateur).
pub(in crate::ui) fn display(ctx: &egui::Context) -> Display {
    ctx.data(|d| d.get_temp::<Display>(display_id())).unwrap_or_default()
}

/// « Copier l'identifiant » proposé : mode développeur (synchronisé), et
/// identifiants non masqués par le mode streamer.
pub(in crate::ui) fn copy_id_shown(ctx: &egui::Context, store: &Store) -> bool {
    store.account.bool(Key::DeveloperMode).unwrap_or(false) && !display(ctx).hide_personal
}

/// Couleurs du thème ramenées vers le gris (`percent` : 100 = inchangées).
pub(in crate::ui) fn saturated(mut t: Tokens, percent: u8) -> Tokens {
    if percent >= 100 {
        return t;
    }
    let s = f32::from(percent) / 100.0;
    let mix = |c: &mut Color32| {
        let [r, g, b, a] = c.to_srgba_unmultiplied();
        let gray = 0.2126 * f32::from(r) + 0.7152 * f32::from(g) + 0.0722 * f32::from(b);
        let ch = |v: u8| (gray + (f32::from(v) - gray) * s).round().clamp(0.0, 255.0) as u8;
        *c = Color32::from_rgba_unmultiplied(ch(r), ch(g), ch(b), a);
    };
    for f in t.fields_mut() {
        match f.value {
            FieldMut::Color(c) => mix(c),
            FieldMut::Palette(p) => p.iter_mut().for_each(mix),
            FieldMut::Number(..) | FieldMut::Choice(_) => {}
        }
    }
    t
}

const MOTIONS: [(Motion, crate::i18n::Text); 3] = [(Motion::System, s2::MOTION_SYSTEM), (Motion::On, s2::MOTION_ON), (Motion::Off, s2::MOTION_OFF)];
const INTENSITIES: [(Intensity, crate::i18n::Text); 3] = [(Intensity::None, s2::INTENSITY_NONE), (Intensity::Subtle, s2::INTENSITY_SUBTLE), (Intensity::Full, s2::INTENSITY_FULL)];

impl App {
    /// Branche le vrai disque et lit `local.toml`.
    pub fn attach_machine(&mut self, mut m: Box<dyn Machine>) {
        m.load();
        self.local.backend = m;
    }

    /// Réglages de cet ordinateur en cours (aperçu, tests).
    pub fn local_prefs(&self) -> LocalPrefs {
        self.local.prefs.clone()
    }

    /// Change les réglages de cet ordinateur et les enregistre.
    pub fn set_local_prefs(&mut self, p: LocalPrefs) {
        if p != self.local.prefs {
            self.local.prefs = p.clone();
            self.local.touched = true;
            self.local.backend.save(p);
            self.plugins_sync();
            self.ctx.request_repaint();
        }
    }

    /// Enregistre les réglages en cours, changés en direct pendant un glissement (lot PERSO).
    pub(in crate::ui) fn save_local_prefs(&mut self) {
        self.local.touched = true;
        self.local.backend.save(self.local.prefs.clone());
        self.plugins_sync();
    }

    /// Réponses du fil des fichiers, puis effets des réglages sur egui (une
    /// fois par changement, rien à chaque image).
    pub(in crate::ui) fn local_tick(&mut self) {
        for e in self.local.backend.poll() {
            match e {
                MachineEvent::Loaded { prefs, windows_reduced, wheel_lines } => {
                    self.local.wheel_lines = wheel_lines;
                    self.local.windows_reduced = windows_reduced;
                    self.local.loaded = true;
                    if !self.local.touched {
                        self.local.prefs = prefs;
                    }
                    // Plugins à redémarrage : leur état est celui lu au lancement.
                    self.local.plugins.boot = Some(self.local.prefs.plugins.bits());
                }
                MachineEvent::SaveFailed(reason) => {
                    let text = sf2::local_not_saved(self.lang, reason);
                    self.notify(text);
                }
            }
        }
        self.plugins_sync();
        // Programmes ajoutés et boutons d'activité (lot C3).
        self.activity_tick();
        let p = self.local.prefs.clone();
        let key = (p.motion.reduced(self.local.windows_reduced), p.instant_tooltips);
        if self.local.applied != Some(key) {
            self.local.applied = Some(key);
            let base = egui::Style::default();
            self.ctx.all_styles_mut(|s| {
                s.animation_time = if key.0 { 0.0 } else { base.animation_time };
                s.scroll_animation = if key.0 { egui::style::ScrollAnimation::none() } else { base.scroll_animation };
                s.interaction.tooltip_delay = if key.1 { 0.0 } else { base.interaction.tooltip_delay };
            });
            self.ctx.request_repaint();
        }
        let wheel = (self.local.wheel_lines, p.scroll_speed);
        if self.local.scroll_applied != Some(wheel) {
            self.local.scroll_applied = Some(wheel);
            crate::ui::scroll::apply(&self.ctx, wheel.0, wheel.1);
        }
        let shown = p.display();
        if self.local.shown != Some(shown) {
            self.local.shown = Some(shown);
            self.ctx.data_mut(|d| d.insert_temp(display_id(), shown));
            self.ctx.request_repaint();
        }
    }

    /// Ligne du journal local (s'il est activé).
    pub(in crate::ui) fn local_log(&mut self, text: &str) {
        if self.local.prefs.debug_log {
            let line = local::log_line(chrono::Utc::now().timestamp_millis(), text);
            self.local.backend.log(line);
        }
    }

    pub(super) fn page_accessibility(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        note(ui, t, s2::LOCAL_TIP.get(lang));
        let mut p = self.local.prefs.clone();
        section(ui, t, s2::SEC_MOTION.get(lang));
        if let Some(m) = choice(ui, lang, "animations", (s2::MOTION.get(lang), s2::MOTION_TIP.get(lang)), &MOTIONS, Some(p.motion), true) {
            p.motion = m;
        }
        if p.motion == Motion::System {
            let said = match self.local.windows_reduced {
                Some(true) => s2::WINDOWS_REDUCED,
                Some(false) => s2::WINDOWS_ANIMATED,
                None => s2::WINDOWS_UNKNOWN,
            };
            note(ui, t, said.get(lang));
        }
        if let Some(i) = choice(ui, lang, "intensite-animations", (s2::INTENSITY.get(lang), s2::INTENSITY_TIP.get(lang)), &INTENSITIES, Some(p.intensity), !p.motion.reduced(self.local.windows_reduced)) {
            p.intensity = i;
        }

        section(ui, t, s2::SEC_SCROLL.get(lang));
        let mut speed = p.scroll_speed;
        let mut save_speed = false;
        super::super::appearance::row(ui, s2::SCROLL_SPEED.get(lang), s2::SCROLL_SPEED_TIP.get(lang), |ui| {
            let r = ui.add(egui::Slider::new(&mut speed, local::SCROLL_MIN..=local::SCROLL_MAX).step_by(10.0).custom_formatter(|v, _| sf2::percent(lang, v as u16)));
            super::super::a11y::name(ui.ctx(), r.id, s2::SCROLL_SPEED.get(lang));
            // Essayable pendant le glissement, enregistré au lâcher.
            save_speed = r.drag_stopped() || (r.changed() && !r.dragged());
        });
        if speed != p.scroll_speed {
            self.local.prefs.scroll_speed = speed;
        }
        p.scroll_speed = speed;

        section(ui, t, s2::SEC_COLORS.get(lang));
        let contrast = self.look.edited.base == Preset::Contraste;
        if let Some(on) = toggle(ui, s2::CONTRAST.get(lang), s2::CONTRAST_TIP.get(lang), Some(contrast), true) {
            let back = if on {
                self.local.before_contrast = Some(self.look.edited.base);
                Preset::Contraste
            } else {
                self.local.before_contrast.take().filter(|b| *b != Preset::Contraste).unwrap_or(Preset::DEFAULT)
            };
            // Les couleurs seulement : tailles, densité, zoom et horloge restent.
            let mut tokens = back.tokens();
            let e = &self.look.edited;
            (tokens.layout, tokens.typography, tokens.spacing, tokens.shapes) = (e.layout.clone(), e.typography.clone(), e.spacing.clone(), e.shapes.clone());
            self.look.edited = tokens;
        }
        let mut sat = p.saturation;
        let mut save = false;
        super::super::appearance::row(ui, s2::SATURATION.get(lang), s2::SATURATION_TIP.get(lang), |ui| {
            let r = ui.add(egui::Slider::new(&mut sat, 0..=100).custom_formatter(|v, _| sf2::percent(lang, v as u8)));
            super::super::a11y::name(ui.ctx(), r.id, s2::SATURATION.get(lang));
            // Aperçu pendant le glissement, enregistré au lâcher.
            save = r.drag_stopped() || (r.changed() && !r.dragged());
        });
        if sat != p.saturation {
            self.local.prefs.saturation = sat;
            self.ctx.request_repaint();
        }
        if let Some(v) = toggle(ui, s2::ROLE_COLORS.get(lang), s2::ROLE_COLORS_TIP.get(lang), Some(p.role_colors), true) {
            p.role_colors = v;
        }

        section(ui, t, s2::SEC_TEXT.get(lang));
        let mut open = false;
        super::super::appearance::row(ui, s2::SIZE_ZOOM.get(lang), s2::SIZE_ZOOM_TIP.get(lang), |ui| {
            open = ui.button(s2::OPEN_APPEARANCE.get(lang)).clicked();
        });
        if open {
            self.params.show(Page::Appearance);
        }
        if let Some(v) = toggle(ui, s2::UNDERLINE.get(lang), s2::UNDERLINE_TIP.get(lang), Some(p.underline_links), true) {
            p.underline_links = v;
        }
        if let Some(v) = toggle(ui, s2::INSTANT_TIPS.get(lang), s2::INSTANT_TIPS_TIP.get(lang), Some(p.instant_tooltips), true) {
            p.instant_tooltips = v;
        }
        p.saturation = sat;
        if save || save_speed {
            self.local.touched = true;
            self.local.backend.save(self.local.prefs.clone());
        }
        self.set_local_prefs(p);
    }

    pub(super) fn page_streamer(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        note(ui, t, s2::LOCAL_TIP.get(lang));
        let mut p = self.local.prefs.clone();
        let s = &mut p.streamer;
        if let Some(v) = toggle(ui, s2::STREAMER_ON.get(lang), s2::STREAMER_ON_TIP.get(lang), Some(s.on), true) {
            s.on = v;
        }
        value_row(ui, s2::STREAMER_AUTO.get(lang), s2::STREAMER_AUTO_TIP.get(lang), s2::STREAMER_AUTO_VALUE.get(lang));
        section(ui, t, s2::SEC_STREAMING.get(lang));
        for (slot, label, tip) in [
            (&mut s.hide_personal, s2::HIDE_PERSONAL, s2::HIDE_PERSONAL_TIP),
            (&mut s.hide_invites, s2::HIDE_INVITES, s2::HIDE_INVITES_TIP),
            (&mut s.mute_sounds, s2::MUTE_SOUNDS, s2::MUTE_SOUNDS_TIP),
            (&mut s.mute_toasts, s2::MUTE_TOASTS, s2::MUTE_TOASTS_TIP),
        ] {
            if let Some(v) = toggle(ui, label.get(lang), tip.get(lang), Some(*slot), true) {
                *slot = v;
            }
        }
        self.set_local_prefs(p);
    }

    pub(super) fn page_advanced(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if self.online() {
            if self.proto_header(ui, t) {
                self.synced_toggle(ui, Key::DeveloperMode, s2::DEV_MODE, s2::DEV_MODE_TIP, false);
            }
        } else {
            note(ui, t, crate::i18n::table::s::t::OFFLINE.get(lang));
        }
        value_row(ui, s2::HW_ACCEL.get(lang), s2::HW_ACCEL_TIP.get(lang), s2::HW_ACCEL_VALUE.get(lang));

        section(ui, t, s2::SEC_IDENTITY.get(lang));
        let mut p = self.local.prefs.clone();
        let available = crate::state::identity_prefs::AVAILABLE;
        if let Some(v) = toggle(ui, s2::DESKTOP_IDENTITY.get(lang), s2::DESKTOP_IDENTITY_TIP.get(lang), Some(p.identity.desktop && available), available) {
            p.identity.desktop = v;
        }
        if !available {
            note(ui, t, s2::DESKTOP_IDENTITY_UNAVAILABLE.get(lang));
        }

        section(ui, t, s2::SEC_DEBUG.get(lang));
        note(ui, t, s2::LOCAL_TIP.get(lang));
        if let Some(v) = toggle(ui, s2::DEBUG_LOG.get(lang), s2::DEBUG_LOG_TIP.get(lang), Some(p.debug_log), true) {
            p.debug_log = v;
        }
        self.set_local_prefs(p);
        let folder = self.local.backend.folder();
        let shown = folder.as_ref().map(|f| f.display().to_string()).unwrap_or_else(|| crate::i18n::table::s::t::UNKNOWN.get(lang).to_string());
        let mut open = false;
        super::super::appearance::row(ui, s2::FOLDER.get(lang), s2::FOLDER_TIP.get(lang), |ui| {
            open = ui.add_enabled(folder.is_some(), egui::Button::new(s2::OPEN_FOLDER.get(lang))).on_hover_text(&shown).clicked();
        });
        note(ui, t, &shown);
        if open && let Some(f) = folder {
            ui.ctx().open_url(egui::OpenUrl::new_tab(format!("file:///{}", f.display().to_string().replace('\\', "/"))));
        }
        self.trusted_domains(ui, t);
    }

    /// Domaines de confiance (lot FF2, F27) : les liens vers eux s'ouvrent sans
    /// la fenêtre de confirmation. Liste locale, jamais envoyée.
    fn trusted_domains(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        use crate::i18n::table::ff::t as ft;
        let lang = self.lang;
        section(ui, t, ft::TRUSTED_DOMAINS.get(lang));
        note(ui, t, ft::TRUSTED_DOMAINS_TIP.get(lang));
        let id = Id::unique("domaines-de-confiance");
        let mut typed: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
        let mut add = false;
        ui.horizontal(|ui| {
            let edit = ui.add(egui::TextEdit::singleline(&mut typed).hint_text(ft::TRUSTED_DOMAIN_HINT.get(lang)).char_limit(253).desired_width(260.0));
            crate::ui::a11y::name(ui.ctx(), edit.id, ft::TRUSTED_DOMAIN_HINT.get(lang));
            add = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            add |= ui.add_enabled(!typed.trim().is_empty(), egui::Button::new(ft::TRUSTED_DOMAIN_ADD.get(lang))).clicked();
        });
        if add && self.install.trusted.add_typed(&typed) {
            typed.clear();
        }
        ui.data_mut(|d| d.insert_temp(id, typed));
        let list = self.install.trusted.domains().to_vec();
        if list.is_empty() {
            note(ui, t, ft::TRUSTED_DOMAINS_NONE.get(lang));
        }
        let mut gone = None;
        for d in &list {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(d).color(t.colors.text));
                if ui.small_button(ft::TRUSTED_DOMAIN_REMOVE.get(lang)).on_hover_text(d).clicked() {
                    gone = Some(d.clone());
                }
            });
        }
        if let Some(d) = gone {
            self.install.trusted.remove(&d);
        }
    }
}

/// Mode développeur activé dans les réglages synchronisés (tests).
#[cfg(test)]
pub(in crate::ui) fn developer_mode_on(store: &mut Store) {
    use crate::state::usersettings::proto::{Msg, Value};
    use crate::state::usersettings::{schema::top, AccountEvent};
    let mut appearance = Msg::default();
    appearance.set(2, Value::Varint(1));
    let mut root = Msg::default();
    root.set(top::APPEARANCE, Value::Len(appearance.encode()));
    store.account.apply(AccountEvent::Proto { bytes: Some(root.encode()), partial: false });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saturation_moves_colors_towards_gray_only_when_asked() {
        let t = Tokens::default();
        assert_eq!(saturated(t.clone(), 100), t);
        let gray = saturated(t.clone(), 0);
        let [r, g, b, a] = gray.colors.accent.to_srgba_unmultiplied();
        assert!(r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1, "{r} {g} {b}");
        assert_eq!(a, t.colors.accent.a());
        assert!(gray.colors.authors.iter().all(|c| {
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1
        }));
        assert_eq!(gray.layout, t.layout, "seules les couleurs changent");
    }
}

#[cfg(test)]
mod app_tests {
    use super::*;
    use crate::state::{Cmd, Event};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct Online(Arc<Mutex<Vec<Cmd>>>);

    impl crate::ui::Backend for Online {
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
            self.0.lock().unwrap().extend(cmds);
        }
    }

    /// Disque factice : retient les écritures, rend un fichier lu à la demande.
    #[derive(Clone, Default)]
    struct Disk {
        saved: Arc<Mutex<Vec<LocalPrefs>>>,
        logged: Arc<Mutex<Vec<String>>>,
        pending: Arc<Mutex<Vec<MachineEvent>>>,
    }

    impl Machine for Disk {
        fn load(&mut self) {}
        fn save(&mut self, p: LocalPrefs) {
            self.saved.lock().unwrap().push(p);
        }
        fn log(&mut self, line: String) {
            self.logged.lock().unwrap().push(line);
        }
        fn folder(&self) -> Option<std::path::PathBuf> {
            None
        }
        fn poll(&mut self) -> Vec<MachineEvent> {
            std::mem::take(&mut *self.pending.lock().unwrap())
        }
    }

    fn app_with(net: Online) -> App {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut store = Store::new();
        store.apply(Event::Ready { me: crate::state::model::User::new(1, "Moi".into()), guilds: Vec::new(), dms: Vec::new() });
        store.take_commands();
        let images = crate::images::Images::disabled(ctx.clone());
        App::with_context(ctx, crate::ui::tests::offline_boot(Box::new(net), store, images))
    }

    fn app() -> App {
        app_with(Online::default())
    }

    #[test]
    fn file_read_applies_and_choices_reach_egui_and_menus() {
        let mut app = app();
        let disk = Disk::default();
        app.attach_machine(Box::new(disk.clone()));
        let mut read = LocalPrefs { motion: Motion::On, instant_tooltips: true, ..Default::default() };
        read.streamer.on = true;
        disk.pending.lock().unwrap().push(MachineEvent::Loaded { prefs: read.clone(), windows_reduced: Some(false), wheel_lines: Some(3) });
        app.local_tick();
        assert_eq!(app.local_prefs(), read);
        let style = app.ctx.global_style();
        assert_eq!((style.animation_time, style.interaction.tooltip_delay), (0.0, 0.0));
        let shown = display(&app.ctx);
        assert!(shown.hide_invites && shown.hide_personal);
        assert!(disk.saved.lock().unwrap().is_empty(), "lire n'écrit rien");

        // Mode développeur : « Copier l'identifiant » seulement hors mode streamer.
        assert!(!copy_id_shown(&app.ctx, &app.store));
        developer_mode_on(&mut app.store);
        assert!(!copy_id_shown(&app.ctx, &app.store), "masqué par le mode streamer");
        let mut p = app.local_prefs();
        p.streamer.on = false;
        p.motion = Motion::System;
        p.debug_log = true;
        app.set_local_prefs(p.clone());
        app.local_tick();
        assert!(copy_id_shown(&app.ctx, &app.store));
        assert!(app.ctx.global_style().animation_time > 0.0, "Windows anime : animations rendues");
        assert_eq!(disk.saved.lock().unwrap().as_slice(), &[p.clone()]);

        // Journal : une ligne par message affiché, sur une seule ligne.
        app.local_log("Erreur\nfictive");
        let logged = disk.logged.lock().unwrap().clone();
        assert!(logged.len() == 1 && logged[0].ends_with(" Erreur fictive\n"), "{logged:?}");
        // Lecture tardive du fichier après un changement : ignorée.
        disk.pending.lock().unwrap().push(MachineEvent::Loaded { prefs: LocalPrefs::default(), windows_reduced: None, wheel_lines: None });
        app.local_tick();
        assert_eq!(app.local_prefs(), p);
    }

    /// Chaque nouvelle page se dessine sans session ni panique, avec ses réglages.
    #[test]
    fn new_pages_draw() {
        let net = Online::default();
        let mut app = app_with(net.clone());
        app.lang = crate::i18n::Lang::Fr;
        for page in [Page::Accessibility, Page::Streamer, Page::Advanced, Page::DataPrivacy, Page::Activity, Page::About, Page::Profile] {
            app.show_settings(page);
            let mut out = None;
            for _ in 0..3 {
                app.logic_pass();
                let raw = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 900.0))), ..Default::default() };
                let mut o = app.ctx.clone().run_ui(raw, |ui| app.ui_pass(ui));
                o.textures_delta.clear();
                out = Some(o);
            }
            let out = out.unwrap();
            let want = page.title().get(crate::i18n::Lang::Fr);
            assert!(crate::ui::tests::find(&out, want).is_some(), "titre « {want} »");
        }
        // Données et confidentialité : les consentements sont demandés une fois.
        let mut cmds = app.store.take_commands();
        cmds.extend(net.0.lock().unwrap().drain(..));
        let loads = cmds.iter().filter(|c| matches!(c, Cmd::Account(crate::state::usersettings::AccountCmd::Extra(crate::state::usersettings::extra::ExtraCmd::LoadConsents)))).count();
        assert_eq!(loads, 1, "{cmds:?}");
    }
}
