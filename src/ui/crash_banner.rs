//! Rapport de plantage local (lot E5, plugin `rapport_plantage`) : au
//! lancement qui suit un plantage, un bandeau discret propose d'ouvrir le
//! rapport ou son dossier, ou de l'ignorer. Le rapport est écrit par
//! `crate::crash` ; rien n'est envoyé.
//!
//! Ici aussi : quelques événements techniques notés au changement (connexion,
//! paramètres, salon), sans nom ni identifiant, et les mots du compte ouvert
//! déclarés privés pour le filtre du rapport.

use super::{widgets, App};
use crate::crash;
use crate::i18n::table::e5::t as et;
use crate::plugins::{self, Id};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct CrashUi {
    /// Dossier des rapports ; `None` : aperçu, tests, autre système.
    dir: Option<PathBuf>,
    /// Le témoin a été lu (une fois par lancement).
    checked: bool,
    /// Rapport à proposer.
    pending: Option<PathBuf>,
    /// Dernier état noté dans le journal technique.
    me: Option<u64>,
    connected: bool,
    settings: bool,
    channel: Option<u64>,
}

impl App {
    /// Branche le dossier des rapports de plantage.
    pub fn attach_crash_reports(&mut self, dir: Option<PathBuf>) {
        self.crash.dir = dir;
        self.crash.checked = false;
    }

    /// Aperçu hors ligne : le bandeau, sans aucun fichier.
    #[doc(hidden)]
    pub fn crash_preview(&mut self) {
        self.crash.pending = Some(PathBuf::from("plantage-2026-11-15-14-03-22.txt"));
        self.crash.checked = true;
    }

    /// Plugin coupé : plus de bandeau ; les mots privés sont rendus.
    pub(super) fn crash_plugin_changed(&mut self, on: bool) {
        if !on {
            self.crash.pending = None;
            self.crash.me = None;
            crash::set_private(Vec::new());
        }
    }

    /// Une lecture atomique quand le plugin est coupé ; sinon quelques
    /// comparaisons, et le témoin lu une fois après `local.toml`.
    pub(super) fn crash_tick(&mut self) {
        if !plugins::on(Id::CrashReport) {
            return;
        }
        let me = self.store.me.as_ref().map(|u| u.id);
        if me != self.crash.me {
            self.crash.me = me;
            crash::set_private(self.store.me.as_ref().map(|u| vec![u.name.clone(), u.id.to_string()]).unwrap_or_default());
        }
        let connected = self.backend.is_connected() && me.is_some();
        if connected != self.crash.connected {
            self.crash.connected = connected;
            crash::note(if connected { "session.prete" } else { "session.fermee" });
        }
        if self.params.open != self.crash.settings {
            self.crash.settings = self.params.open;
            crash::note(if self.params.open { "parametres.ouverts" } else { "parametres.fermes" });
        }
        if self.store.channel != self.crash.channel {
            self.crash.channel = self.store.channel;
            crash::note("salon.change");
        }
        // Le plugin peut avoir été coupé dans `local.toml` : attendre sa lecture.
        if !self.crash.checked && self.local.loaded {
            self.crash.checked = true;
            self.crash.pending = self.crash.dir.as_deref().and_then(crash::pending);
        }
    }

    /// Bandeau du haut, tant qu'un rapport n'a pas été vu ou ignoré.
    pub(super) fn crash_banner(&mut self, ui: &mut egui::Ui) {
        let Some(path) = self.crash.pending.clone() else { return };
        if !plugins::on(Id::CrashReport) {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let (mut open, mut folder, mut ignore) = (false, false, false);
        let frame = egui::Frame::new().fill(t.colors.bg3).inner_margin(egui::Margin::symmetric(t.spacing.chat_head_x as i8, t.spacing.item_y as i8));
        egui::Panel::top("rapport_plantage").frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ignore = widgets::compact(ui, et::CRASH_IGNORE.get(lang)).clicked();
                    folder = widgets::compact(ui, et::CRASH_FOLDER.get(lang)).clicked();
                    open = widgets::compact(ui, et::CRASH_OPEN.get(lang)).clicked();
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(et::CRASH_BANNER.get(lang)).small().color(t.colors.muted)).truncate()).on_hover_text(et::CRASH_BANNER_TIP.get(lang));
                    });
                });
            });
        });
        if !(open || folder || ignore) {
            return;
        }
        if let Some(dir) = &self.crash.dir {
            if open {
                crash::open_report(dir, &path);
            }
            if folder {
                crate::install::browser::open(&format!("file:///{}", dir.display().to_string().replace('\\', "/")));
            }
            crash::dismiss(dir);
        }
        self.crash.pending = None;
        ui.ctx().request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::keys::tests::app;
    use crate::ui::tests::{click, find, frame};

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ferrocord-bandeau-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn crashed(dir: &std::path::Path) -> PathBuf {
        crash::write_report(dir, &crash::Report { date: "2026-11-15 14:03:22".into(), kind: "panique", message: "essai".into(), ..Default::default() }).unwrap()
    }

    /// Après un plantage : le bandeau propose le rapport ; « Ignorer » le
    /// retire pour de bon (le rapport reste dans le dossier).
    #[test]
    fn banner_offers_the_report_once() {
        let dir = temp("une-fois");
        let report = crashed(&dir);
        let mut a = app();
        a.attach_crash_reports(Some(dir.clone()));
        let out = frame(&mut a, Vec::new());
        assert!(find(&out, "Ouvrir le rapport").is_none(), "pas avant la lecture de local.toml");
        a.local.loaded = true;
        let out = frame(&mut a, Vec::new());
        let out = if find(&out, "Ouvrir le rapport").is_some() { out } else { frame(&mut a, Vec::new()) };
        for label in ["Ouvrir le rapport", "Ouvrir le dossier", "Ignorer"] {
            assert!(find(&out, label).is_some(), "{label}");
        }
        click(&mut a, "Ignorer");
        let out = frame(&mut a, Vec::new());
        assert!(find(&out, "Ouvrir le rapport").is_none());
        assert_eq!(crash::pending(&dir), None);
        assert!(report.is_file(), "le rapport reste");
        // Lancement suivant : rien à proposer.
        let mut b = app();
        b.attach_crash_reports(Some(dir.clone()));
        b.local.loaded = true;
        frame(&mut b, Vec::new());
        assert!(find(&frame(&mut b, Vec::new()), "Ouvrir le rapport").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Plugin coupé : le témoin n'est pas lu, rien n'est proposé.
    #[test]
    fn nothing_when_the_plugin_is_off() {
        let dir = temp("coupe");
        crashed(&dir);
        let mut a = app();
        a.set_plugin(Id::CrashReport, false);
        a.attach_crash_reports(Some(dir.clone()));
        a.local.loaded = true;
        frame(&mut a, Vec::new());
        assert!(find(&frame(&mut a, Vec::new()), "Ouvrir le rapport").is_none());
        assert!(!a.crash.checked);
        // Réactivé : le rapport est proposé ; recoupé : le bandeau part.
        a.set_plugin(Id::CrashReport, true);
        frame(&mut a, Vec::new());
        assert!(find(&frame(&mut a, Vec::new()), "Ouvrir le rapport").is_some());
        a.set_plugin(Id::CrashReport, false);
        assert!(find(&frame(&mut a, Vec::new()), "Ouvrir le rapport").is_none());
        assert!(crash::pending(&dir).is_some(), "le témoin n'est pas touché par un plugin coupé");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
