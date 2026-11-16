//! Boutons du panneau utilisateur, en bas à gauche (lot profil), comme dans
//! le client : manette de l'activité en cours (quand il y en a une), micro
//! et casque avec chacun son petit chevron, puis l'engrenage. Un clic sur
//! l'icône coupe ou rétablit ; seul le chevron ouvre le menu (périphérique,
//! profil d'entrée, volume, « Paramètres vocaux »). Micro ou casque coupé :
//! icône barrée rouge sur un fond rouge sombre qui englobe le chevron.
//!
//! Les menus ne lisent que la liste des périphériques déjà connue du service
//! audio local et les réglages existants (`AudioPrefs`) : aucune requête.
//! Rien n'est animé : aucun redessin au repos.

use super::menu::{self, Lead, Line, Mark};
use super::settings::Page;
use super::App;
use crate::audio::io::{display_name, DeviceInfo};
use crate::i18n::table::audio::t as at;
use crate::i18n::table::profile::t as pt;
use crate::i18n::table::voice::t as vt;
use crate::i18n::Lang;
use crate::state::audio_prefs::{AudioPrefs, VOLUME_MAX};
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Color32, Id, Rect, Sense, Vec2};

/// Part du côté d'un bouton prise par son chevron.
const CHEVRON: f32 = 0.5;
/// Part de rouge du fond d'un bouton coupé.
const OFF_TINT: f32 = 0.3;
/// Largeur minimale d'un menu de périphériques.
const MENU_WIDTH: f32 = 260.0;

/// Ce que la rangée de boutons montre, selon la place : les chevrons
/// disparaissent d'abord, puis la manette ; micro, casque et engrenage ne
/// sont jamais coupés.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Fit {
    pub chevrons: bool,
    pub activity: bool,
}

/// Largeur d'une rangée de boutons de côté `side`, espacés de `gap`.
fn row_width(side: f32, gap: f32, chevrons: bool, activity: bool) -> f32 {
    let n = if activity { 4.0 } else { 3.0 };
    let chevrons = if chevrons { 2.0 * (side * CHEVRON).round() } else { 0.0 };
    n * side + (n - 1.0) * gap + chevrons
}

/// Ce qui tient dans `width` en gardant `keep` au nom (avatar compris).
pub(super) fn fit(width: f32, keep: f32, side: f32, gap: f32, activity: bool) -> Fit {
    let room = |chevrons, activity| width - row_width(side, gap, chevrons, activity) - gap >= keep;
    if room(true, activity) {
        Fit { chevrons: true, activity }
    } else if room(false, activity) {
        Fit { chevrons: false, activity }
    } else {
        Fit { chevrons: false, activity: false }
    }
}

/// Profil d'entrée audio : un nom pour les trois traitements du micro.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum InputProfile {
    /// Suppression du bruit, annulation de l'écho et gain automatique.
    Isolation,
    /// Aucun traitement.
    Studio,
    Custom,
}

impl InputProfile {
    pub(super) fn of(v: &AudioPrefs) -> InputProfile {
        match (v.noise_suppression, v.echo_cancellation, v.auto_gain) {
            (true, true, true) => InputProfile::Isolation,
            (false, false, false) => InputProfile::Studio,
            _ => InputProfile::Custom,
        }
    }

    fn label(self, lang: Lang) -> &'static str {
        match self {
            InputProfile::Isolation => pt::PROFILE_ISOLATION.get(lang),
            InputProfile::Studio => pt::PROFILE_STUDIO.get(lang),
            InputProfile::Custom => pt::PROFILE_CUSTOM.get(lang),
        }
    }

    /// Applique le profil ; « Personnalisé » garde les réglages en place.
    pub(super) fn apply(self, v: &mut AudioPrefs) {
        let on = match self {
            InputProfile::Isolation => true,
            InputProfile::Studio => false,
            InputProfile::Custom => return,
        };
        (v.noise_suppression, v.echo_cancellation, v.auto_gain) = (on, on, on);
    }
}

fn menu_id(input: bool) -> Id {
    Id::unique(("ferrocord.panneau.menu-audio", input))
}

fn volume_id(input: bool) -> Id {
    Id::unique(("ferrocord.panneau.volume", input))
}

/// Manette (activité en cours) : corps arrondi, croix et deux boutons en creux.
pub(super) fn paint_gamepad(p: &egui::Painter, rect: Rect, color: Color32, hole: Color32) {
    let (w, h) = (rect.width(), rect.height());
    let body = Rect::from_center_size(rect.center(), egui::vec2(w, h * 0.62));
    p.rect_filled(body, h * 0.26, color);
    // Poignées.
    for x in [body.left() + w * 0.16, body.right() - w * 0.16] {
        p.circle_filled(egui::pos2(x, body.bottom() - h * 0.06), w * 0.16, color);
    }
    let c = egui::pos2(body.left() + w * 0.3, body.center().y - h * 0.04);
    let (arm, thick) = (w * 0.12, (w * 0.075).max(1.0));
    p.rect_filled(Rect::from_center_size(c, egui::vec2(2.0 * arm, thick)), 0.0, hole);
    p.rect_filled(Rect::from_center_size(c, egui::vec2(thick, 2.0 * arm)), 0.0, hole);
    let r = (w * 0.055).max(0.8);
    p.circle_filled(egui::pos2(body.right() - w * 0.33, body.center().y + h * 0.02), r, hole);
    p.circle_filled(egui::pos2(body.right() - w * 0.2, body.center().y - h * 0.1), r, hole);
}

/// Bouton simple du panneau (manette, engrenage) : fond clair arrondi au survol.
fn plain_button(ui: &mut egui::Ui, t: &Tokens, side: f32, tip: &str, paint: impl FnOnce(&egui::Painter, Rect, Color32)) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let icon = t.panel_icon();
    paint(ui.painter(), Rect::from_center_size(rect.center(), Vec2::splat(icon)), if hot { t.colors.text_strong } else { t.colors.muted });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Bouton du micro ou du casque et son chevron.
struct Group<'a> {
    icon: Icon,
    /// Coupé : icône barrée rouge, fond rouge sombre autour de l'icône et du chevron.
    off: bool,
    tip: &'a str,
    menu_tip: &'a str,
    /// Le chevron est montré (sinon le menu s'ouvre au clic droit sur l'icône).
    chevron: bool,
    menu: Id,
}

/// Rend l'icône et, s'il est montré, le chevron.
fn group(ui: &mut egui::Ui, t: &Tokens, side: f32, bg: Color32, g: Group<'_>) -> (egui::Response, Option<egui::Response>) {
    let cw = if g.chevron { (side * CHEVRON).round() } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(egui::vec2(side + cw, side), Sense::hover());
    let icon_rect = Rect::from_min_size(rect.min, Vec2::splat(side));
    let chev_rect = Rect::from_min_max(egui::pos2(icon_rect.right(), rect.top()), rect.max);
    let icon = ui.interact(icon_rect, ui.scope_id().with((g.menu, "icone")), Sense::click());
    let chevron = g.chevron.then(|| ui.interact(chev_rect, ui.scope_id().with((g.menu, "chevron")), Sense::click()));
    let open = egui::Popup::is_id_open(ui.ctx(), g.menu);
    let p = ui.painter();
    let r = t.r_row();
    if g.off {
        p.rect_filled(rect, r, super::cosmetics::mix(bg, t.colors.danger, OFF_TINT));
    }
    let hot_icon = icon.hovered() || icon.has_focus();
    let hot_chev = open || chevron.as_ref().is_some_and(|c| c.hovered() || c.has_focus());
    if hot_icon {
        p.rect_filled(icon_rect, r, t.colors.row_hover);
    }
    if hot_chev && g.chevron {
        p.rect_filled(chev_rect, r, t.colors.row_hover);
    }
    let ink = |hot: bool| if g.off { t.colors.danger } else if hot { t.colors.text_strong } else { t.colors.muted };
    let glyph = t.panel_icon();
    icons::paint(g.icon, p, Rect::from_center_size(icon_rect.center(), Vec2::splat(glyph)), ink(hot_icon));
    if g.chevron {
        let s = (cw * 0.8).round().max(1.0);
        icons::paint(if open { Icon::ChevronUp } else { Icon::ChevronDown }, p, Rect::from_center_size(chev_rect.center(), Vec2::splat(s)), ink(hot_chev));
    }
    icon.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, g.off, g.tip));
    let icon = super::widgets::tip(icon.on_hover_cursor(egui::CursorIcon::PointingHand), t, g.tip);
    let chevron = chevron.map(|c| {
        c.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, g.menu_tip));
        // Menu ouvert : pas d'infobulle par-dessus.
        if open { c } else { super::widgets::tip(c.on_hover_cursor(egui::CursorIcon::PointingHand), t, g.menu_tip) }
    });
    (icon, chevron)
}

/// Nom du périphérique choisi : celui de Windows, sinon son nom connu.
fn device_hint(lang: Lang, current: Option<&str>, list: &[DeviceInfo]) -> String {
    match current {
        None => pt::WINDOWS_DEFAULT.get(lang).to_string(),
        Some(id) => display_name(id, list).to_string(),
    }
}

/// Ligne à deux étages (texte, précision grise) qui ouvre un sous-menu.
fn sub_line(ui: &mut egui::Ui, t: &Tokens, text: &str, hint: &str, add: impl FnOnce(&mut egui::Ui)) {
    let resp = menu::line(ui, t, Line { hint: Some(hint), mark: Mark::Submenu, ..Line::new(Lead::None, text) });
    egui::containers::menu::SubMenu::default().show(ui, &resp, add);
}

impl App {
    /// Aperçu hors ligne (`examples/preview.rs`, scènes `panneau` et
    /// `panneau-menu`) : micro coupé ; `menu` : le menu du chevron du micro ouvert.
    #[doc(hidden)]
    pub fn user_panel_preview(&mut self, menu: bool) {
        let mut p = self.local.prefs.clone();
        p.voice.mute = true;
        self.set_local_prefs(p);
        self.store.voice_set_mute(true);
        if menu {
            egui::Popup::open_id(&self.ctx, menu_id(true));
        }
    }

    /// Boutons du panneau, de droite à gauche (`ui` est disposé ainsi) :
    /// engrenage, casque, micro. L'activité en cours a son encart au-dessus
    /// du panneau et sa manette sous le nom (`game_card.rs`), comme dans le
    /// client : plus de bouton ici. `keep` : largeur à laisser à
    /// l'avatar et au nom.
    pub(super) fn user_panel_buttons(&mut self, ui: &mut egui::Ui, keep: f32, bg: Color32) {
        let t = self.theme.clone();
        let lang = self.lang;
        let pad = ui.spacing().button_padding.y;
        let side = t.panel_button(pad);
        let side = if super::mobile::is_active(ui.ctx()) { side.max(super::mobile::TOUCH) } else { side };
        let gap = t.spacing.item_y / 2.0;
        ui.spacing_mut().item_spacing.x = gap;
        let fit = fit(ui.available_width(), keep, side, gap, false);
        if plain_button(ui, &t, side, crate::i18n::table::s::t::GEAR_TIP.get(lang), |p, rect, color| icons::paint(Icon::Gear, p, rect, color)).clicked() {
            self.params.open = !self.params.open;
        }
        let (mute, deaf) = (self.store.voice.self_mute, self.store.voice.self_deaf);
        let keys = &self.nav.bindings;
        let deaf_tip = super::keys::with_chord(lang, keys, crate::state::keys::Action::ToggleDeafen, if deaf { vt::UNDEAFEN.get(lang) } else { vt::DEAFEN.get(lang) });
        let mute_tip = super::keys::with_chord(lang, keys, crate::state::keys::Action::ToggleMute, if mute { vt::UNMUTE.get(lang) } else { vt::MUTE.get(lang) });
        for input in [false, true] {
            let (icon, off, tip, menu_tip) = if input {
                (if mute { Icon::MicOff } else { Icon::Mic }, mute, mute_tip.as_str(), pt::PANEL_INPUT_MENU.get(lang))
            } else {
                (if deaf { Icon::HeadphonesOff } else { Icon::Headphones }, deaf, deaf_tip.as_str(), pt::PANEL_OUTPUT_MENU.get(lang))
            };
            let (icon, chevron) = group(ui, &t, side, bg, Group { icon, off, tip, menu_tip, chevron: fit.chevrons, menu: menu_id(input) });
            if icon.clicked() {
                if input { self.voice_toggle_mute() } else { self.voice_toggle_deaf() }
            }
            // Seul le chevron ouvre le menu ; sans place pour lui, le clic droit sur l'icône.
            match chevron {
                Some(c) => {
                    egui::Popup::menu(&c).id(menu_id(input)).show(|ui| self.audio_menu(ui, input));
                }
                None => {
                    icon.context_menu(|ui| self.audio_menu(ui, input));
                }
            }
        }
    }

    /// Menu d'un chevron : périphérique (sous-menu), profil d'entrée (micro
    /// seulement), volume, « Paramètres vocaux ». Chaque choix s'applique
    /// aussitôt, appel en cours compris (les réglages sont relus par le
    /// moteur vocal) ; le volume est enregistré au lâcher du curseur.
    pub(super) fn audio_menu(&mut self, ui: &mut egui::Ui, input: bool) {
        let t = self.theme.clone();
        let lang = self.lang;
        ui.set_min_width(MENU_WIDTH);
        ui.set_max_width(MENU_WIDTH);
        let devices = self.audio_devices(input);
        let base = self.local.prefs.audio.clone();
        let mut v = base.clone();
        let current = if input { v.input_device.clone() } else { v.output_device.clone() };
        let list = devices.as_deref().unwrap_or_default();
        let title = if input { at::INPUT_DEVICE } else { at::OUTPUT_DEVICE }.get(lang);
        let mut picked: Option<Option<String>> = None;
        sub_line(ui, &t, title, &device_hint(lang, current.as_deref(), list), |ui| {
            ui.set_min_width(MENU_WIDTH);
            if menu::radio(ui, &t, pt::WINDOWS_DEFAULT.get(lang), current.is_none()).clicked() {
                picked = Some(None);
            }
            if devices.is_none() {
                ui.add_enabled_ui(false, |ui| menu::plain(ui, &t, at::LOADING_DEVICES.get(lang), None, false));
            }
            for d in list {
                if menu::radio(ui, &t, &d.name, current.as_deref() == Some(d.id.as_str())).clicked() {
                    picked = Some(Some(d.id.clone()));
                }
            }
        });
        if let Some(d) = picked {
            if input { v.input_device = d } else { v.output_device = d }
        }
        if input {
            let profile = InputProfile::of(&v);
            // Plugin RNNoise (lot E4) : c'est lui qui supprime le bruit quand il est activé.
            let noise = if crate::plugins::on(crate::plugins::Id::NoiseRnn) { pt::NOISE_RNN.get(lang) } else { at::NOISE.get(lang) };
            sub_line(ui, &t, pt::INPUT_PROFILE.get(lang), profile.label(lang), |ui| {
                ui.set_min_width(MENU_WIDTH);
                for p in [InputProfile::Isolation, InputProfile::Studio, InputProfile::Custom] {
                    if menu::radio(ui, &t, p.label(lang), p == profile).clicked() {
                        p.apply(&mut v);
                    }
                }
                menu::separator(ui, &t);
                menu::check(ui, &t, noise, &mut v.noise_suppression);
                menu::check(ui, &t, at::ECHO.get(lang), &mut v.echo_cancellation);
                menu::check(ui, &t, at::AGC.get(lang), &mut v.auto_gain);
            });
        }
        menu::separator(ui, &t);
        // Volume : curseur sur toute la largeur ; le brouillon vit le temps du glissement.
        let label = if input { at::INPUT_VOLUME } else { at::OUTPUT_VOLUME }.get(lang);
        let saved = if input { v.input_volume } else { v.output_volume };
        let mut level = ui.data(|d| d.get_temp::<u8>(volume_id(input))).unwrap_or(saved);
        let pad = t.spacing.item_x;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(pad as i8, (t.spacing.item_y / 2.0) as i8)).show(ui, |ui| {
            ui.label(egui::RichText::new(label).font(super::fonts::medium_font(t.typography.button)).color(t.colors.text));
            // Largeur fixe : à la passe de mesure du menu, la place offerte est celle de l'écran.
            ui.spacing_mut().slider_width = MENU_WIDTH - 2.0 * pad;
            let r = ui.add(egui::Slider::new(&mut level, 0..=VOLUME_MAX).show_value(false));
            super::a11y::name(ui.ctx(), r.id, label);
            let r = r.on_hover_text(crate::i18n::table::s2::f::percent(lang, level));
            if r.dragged() {
                ui.data_mut(|d| d.insert_temp(volume_id(input), level));
            } else {
                ui.data_mut(|d| d.remove::<u8>(volume_id(input)));
                if input { v.input_volume = level } else { v.output_volume = level }
            }
        });
        menu::separator(ui, &t);
        if menu::line(ui, &t, Line::new(Lead::Icon(Icon::Gear), pt::VOICE_SETTINGS.get(lang))).clicked() {
            self.show_settings(Page::Voice);
            ui.close();
        }
        if v != base {
            self.audio_prefs_set(v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Place comptée : les chevrons partent d'abord, puis la manette ; les
    /// trois boutons de base tiennent toujours.
    #[test]
    fn chevrons_go_first_then_the_activity_button() {
        let (side, gap, keep) = (28.0, 3.0, 90.0);
        let all = row_width(side, gap, true, true);
        assert_eq!(fit(all + gap + keep, keep, side, gap, true), Fit { chevrons: true, activity: true });
        assert_eq!(fit(all + gap + keep - 1.0, keep, side, gap, true), Fit { chevrons: false, activity: true });
        let bare = row_width(side, gap, false, true);
        assert_eq!(fit(bare + gap + keep - 1.0, keep, side, gap, true), Fit { chevrons: false, activity: false });
        assert_eq!(fit(10.0, keep, side, gap, true), Fit { chevrons: false, activity: false });
        // Sans activité : pas de manette, quelle que soit la place.
        assert_eq!(fit(1000.0, keep, side, gap, false), Fit { chevrons: true, activity: false });
        assert!(row_width(side, gap, false, false) < row_width(side, gap, true, false));
    }

    /// Profil d'entrée : nommé d'après les trois traitements, appliqué d'un bloc.
    #[test]
    fn input_profile_names_the_three_treatments() {
        let mut v = AudioPrefs::default();
        InputProfile::Isolation.apply(&mut v);
        assert_eq!((v.noise_suppression, v.echo_cancellation, v.auto_gain), (true, true, true));
        assert_eq!(InputProfile::of(&v), InputProfile::Isolation);
        v.echo_cancellation = false;
        assert_eq!(InputProfile::of(&v), InputProfile::Custom);
        InputProfile::Custom.apply(&mut v);
        assert!(v.noise_suppression && !v.echo_cancellation, "« Personnalisé » ne change rien");
        InputProfile::Studio.apply(&mut v);
        assert_eq!(InputProfile::of(&v), InputProfile::Studio);
        assert_eq!(InputProfile::Studio.label(Lang::Fr), "Studio");
        assert_eq!(InputProfile::Isolation.label(Lang::En), "Voice Isolation");
    }

    /// Périphérique montré sous le titre : celui de Windows, sinon son nom.
    #[test]
    fn device_hint_names_the_choice() {
        let list = [DeviceInfo { id: "a".into(), name: "Micro USB".into(), is_default: true }];
        assert_eq!(device_hint(Lang::Fr, None, &list), "Paramètres audio Windows par défaut");
        assert_eq!(device_hint(Lang::Fr, Some("a"), &list), "Micro USB");
        assert_eq!(device_hint(Lang::En, Some("zz"), &list), "zz");
    }

    struct Quiet;

    impl super::super::Backend for Quiet {
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

    /// Un clic sur l'icône coupe le micro sans ouvrir de menu ; seul le
    /// chevron ouvre le menu (périphérique, profil d'entrée, volume,
    /// « Paramètres vocaux »), sans toucher au micro.
    #[test]
    fn icon_mutes_and_only_the_chevron_opens_the_menu() {
        use super::super::tests::{click, find, frame, offline_boot};
        let mut store = crate::state::Store::new();
        store.apply(crate::state::Event::Ready { me: crate::state::model::User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Quiet), store, images));
        app.lang = Lang::Fr;
        click(&mut app, "Couper le micro");
        assert!(app.store.voice.self_mute, "le clic sur l'icône coupe");
        let out = frame(&mut app, Vec::new());
        assert!(find(&out, "Paramètres vocaux").is_none(), "aucun menu ouvert par l'icône");
        assert!(find(&out, "Réactiver le micro").is_some());
        click(&mut app, "Options d'entrée");
        let out = frame(&mut app, Vec::new());
        assert!(app.store.voice.self_mute, "le chevron ne touche pas au micro");
        for want in ["Périphérique d'entrée", "Profil d'entrée audio", "Volume d'entrée", "Paramètres vocaux"] {
            assert!(find(&out, want).is_some(), "« {want} » manque dans le menu");
        }
        // Les boutons tiennent dans la fenêtre, à droite du nom.
        let gear = find(&out, "Paramètres utilisateur").or_else(|| find(&out, crate::i18n::table::s::t::GEAR_TIP.get(Lang::Fr))).unwrap_or_else(|| panic!("engrenage"));
        let mic = find(&out, "Réactiver le micro").unwrap_or_else(|| panic!("micro"));
        assert!(mic.right() <= gear.left() + 0.5 && mic.left() >= 0.0, "{mic:?} {gear:?}");
        click(&mut app, "Paramètres vocaux");
        assert!(app.params.open && app.params.page() == Page::Voice);
    }
}
