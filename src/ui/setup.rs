//! Petite fenêtre de l'installation (lot distrib) : « Installer Ferrocord »
//! ou « Lancer sans installer » au premier lancement d'un exécutable
//! téléchargé, et la confirmation de la désinstallation. Thème et boutons de
//! l'application ; le travail est fait par `install::setup`.

use super::widgets::{self, Kind};
use crate::i18n::table::install::{f, t as it};
use crate::i18n::Lang;
use crate::install::setup::{self, Leftover, Options, Places};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Proposer d'installer l'exécutable lancé.
    Offer,
    /// Confirmer la désinstallation.
    Uninstall,
}

/// Ce que l'utilisateur a choisi.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum Outcome {
    /// « Lancer sans installer », « Annuler », ou fenêtre fermée.
    #[default]
    Closed,
    /// Installé : l'exécutable à lancer.
    Installed(PathBuf),
    /// Désinstallé : ce qu'il reste à supprimer une fois ce processus fermé.
    Uninstalled(Option<Leftover>),
}

struct Window {
    mode: Mode,
    places: Places,
    exe: PathBuf,
    tokens: Tokens,
    lang: Lang,
    options: Options,
    wipe: bool,
    error: Option<String>,
    out: Rc<RefCell<Outcome>>,
    /// Le contenu tient-il au-dessus des boutons ? (tests)
    #[cfg(test)]
    fits: bool,
}

/// Taille de la fenêtre, en points.
const SIZE: [f32; 2] = [480.0, 400.0];

/// Ouvre la fenêtre et attend sa fermeture.
pub fn run(mode: Mode, places: Places, exe: PathBuf) -> Outcome {
    crate::i18n::set_language(crate::i18n::Choice::System);
    let out = Rc::new(RefCell::new(Outcome::default()));
    let kept = out.clone();
    let viewport = egui::ViewportBuilder::default().with_title("Ferrocord").with_inner_size(SIZE).with_resizable(false).with_maximize_button(false);
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    // Fenêtre impossible à ouvrir : comme si elle avait été fermée.
    let _ = eframe::run_native(
        "Ferrocord",
        options,
        Box::new(move |cc| {
            super::fonts::install(&cc.egui_ctx);
            let tokens = Tokens::default();
            tokens.apply(&cc.egui_ctx);
            Ok(Box::new(Window::new(mode, places, exe, tokens, kept)))
        }),
    );
    out.take()
}

impl Window {
    fn new(mode: Mode, places: Places, exe: PathBuf, tokens: Tokens, out: Rc<RefCell<Outcome>>) -> Window {
        Window {
            mode,
            places,
            exe,
            tokens,
            lang: crate::i18n::language(),
            options: Options::default(),
            wipe: false,
            error: None,
            out,
            #[cfg(test)]
            fits: true,
        }
    }

    /// Une image : le fond, puis le contenu du mode.
    fn frame(&mut self, ui: &mut egui::Ui) {
        let fill = self.tokens.colors.bg2;
        egui::CentralPanel::default().frame(egui::Frame::new().fill(fill).inner_margin(24.0)).show(ui, |ui| match self.mode {
            Mode::Offer => self.offer(ui),
            Mode::Uninstall => self.uninstall(ui),
        });
    }

    fn finish(&mut self, ui: &egui::Ui, outcome: Outcome) {
        *self.out.borrow_mut() = outcome;
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
    }

    /// Un autre Ferrocord ouvert depuis l'installation : rien à faire avant qu'il soit quitté.
    fn blocked(&mut self) -> bool {
        let busy = setup::running(&self.places.target());
        if busy {
            self.error = Some(it::SETUP_RUNNING.get(self.lang).to_string());
        }
        busy
    }

    fn offer(&mut self, ui: &mut egui::Ui) {
        let (t, lang) = (self.tokens.clone(), self.lang);
        title(ui, &t, it::SETUP_TITLE.get(lang));
        ui.label(RichText::new(f::setup_body(lang, self.places.programs.display())).color(t.colors.text));
        ui.add_space(t.spacing.item_y);
        ui.checkbox(&mut self.options.desktop, RichText::new(it::SETUP_DESKTOP.get(lang)).color(t.colors.text));
        ui.checkbox(&mut self.options.autostart, RichText::new(it::AUTOSTART.get(lang)).color(t.colors.text));
        self.error_line(ui, &t);
        let (mut install, mut portable) = (false, false);
        footer(ui, &t, |ui| {
            install = widgets::button(ui, &t, Kind::Primary, it::SETUP_INSTALL.get(lang)).clicked();
            portable = widgets::button(ui, &t, Kind::Secondary, it::SETUP_PORTABLE.get(lang)).clicked();
        });
        if portable {
            self.finish(ui, Outcome::Closed);
        } else if install && !self.blocked() {
            match setup::install(&mut crate::install::hive::UserHive, &mut crate::install::shortcut::WindowsShell, &self.places, &self.exe, env!("CARGO_PKG_VERSION"), self.options) {
                Ok(target) => self.finish(ui, Outcome::Installed(target)),
                Err(e) => self.error = Some(f::setup_failed(lang, e)),
            }
        }
    }

    fn uninstall(&mut self, ui: &mut egui::Ui) {
        let (t, lang) = (self.tokens.clone(), self.lang);
        title(ui, &t, it::UNINSTALL_TITLE.get(lang));
        ui.label(RichText::new(it::UNINSTALL_BODY.get(lang)).color(t.colors.text));
        ui.add_space(t.spacing.item_y);
        ui.checkbox(&mut self.wipe, RichText::new(it::UNINSTALL_WIPE.get(lang)).color(t.colors.text));
        ui.add(egui::Label::new(RichText::new(it::UNINSTALL_TOKENS.get(lang)).small().color(t.colors.faint)).wrap());
        self.error_line(ui, &t);
        let (mut go, mut cancel) = (false, false);
        footer(ui, &t, |ui| {
            go = widgets::button(ui, &t, Kind::Danger, it::UNINSTALL_GO.get(lang)).clicked();
            cancel = widgets::button(ui, &t, Kind::Secondary, it::UNINSTALL_CANCEL.get(lang)).clicked();
        });
        if cancel {
            self.finish(ui, Outcome::Closed);
        } else if go && !self.blocked() {
            match setup::uninstall(&mut crate::install::hive::UserHive, &self.places, self.wipe) {
                Ok(left) => self.finish(ui, Outcome::Uninstalled(left)),
                Err(e) => self.error = Some(f::setup_failed(lang, e)),
            }
        }
    }

    /// Message d'erreur, dernier élément au-dessus des boutons.
    fn error_line(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        if let Some(e) = &self.error {
            ui.add_space(t.spacing.item_y);
            ui.add(egui::Label::new(RichText::new(e).color(t.colors.danger)).wrap());
        }
        #[cfg(test)]
        {
            self.fits = ui.cursor().top() + widgets::button_height(t) <= ui.max_rect().bottom();
        }
    }
}

fn title(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).family(super::fonts::demi()).size(t.typography.body * 1.4).strong().color(t.colors.text_strong));
    ui.add_space(t.spacing.item_y);
}

/// Boutons en bas à droite, l'action principale au bord.
fn footer(ui: &mut egui::Ui, t: &Tokens, add: impl FnOnce(&mut egui::Ui)) {
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Max), |ui| {
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), widgets::button_height(t)), egui::Layout::right_to_left(egui::Align::Center), add);
    });
}

impl eframe::App for Window {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Les deux écrans, dans les deux langues, avec un long dossier et un
    /// message d'erreur : tout tient au-dessus des boutons, sans rien faire.
    #[test]
    fn both_screens_fit_in_the_window() {
        let root = std::env::temp_dir().join("ferrocord-fenetre-installation-jamais-cree");
        let places = Places { programs: PathBuf::from(r"C:\Users\un-nom-d-utilisateur-plutot-long\AppData\Local\Programs\Ferrocord"), start_menu: root.join("menu"), desktop: None, data: Vec::new() };
        for mode in [Mode::Offer, Mode::Uninstall] {
            for lang in Lang::ALL {
                let ctx = egui::Context::default();
                super::super::fonts::install_quiet(&ctx);
                let tokens = Tokens::default();
                tokens.apply(&ctx);
                let out = Rc::new(RefCell::new(Outcome::default()));
                let mut w = Window::new(mode, places.clone(), root.join("ferrocord.exe"), tokens, out.clone());
                w.lang = lang;
                w.error = Some(it::SETUP_RUNNING.get(lang).to_string());
                let input = || egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE.into())), ..Default::default() };
                for _ in 0..2 {
                    let mut frame = ctx.run_ui(input(), |ui| w.frame(ui));
                    frame.textures_delta.clear();
                }
                assert!(w.fits, "{mode:?} en {lang:?} : le contenu déborde sur les boutons");
                assert_eq!(*out.borrow(), Outcome::Closed, "rien n'est fait sans clic");
            }
        }
        assert!(!root.exists());
    }
}
