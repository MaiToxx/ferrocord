//! Notes de version après une mise à jour : au premier lancement de la
//! nouvelle version, un bandeau discret dit laquelle est installée et propose
//! ses notes (celles du manifeste signé, gardées au téléchargement par
//! `install::update::news`). Fermé, il ne revient pas.

use super::{widgets, App};
use crate::i18n::table::install::{f, t as it};
use crate::install::update::{news, source};
use eframe::egui::{self, RichText};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct NewsUi {
    /// Dossier de `nouveautes.txt` ; `None` : aperçu, tests.
    dir: Option<PathBuf>,
    /// Le fichier a été lu (une fois par lancement).
    checked: bool,
    /// Notes à proposer (vides : la version seule est annoncée).
    pending: Option<String>,
    /// Notes dépliées.
    open: bool,
}

impl App {
    /// Branche le dossier où les notes d'une mise à jour sont gardées.
    pub fn attach_update_notes(&mut self, dir: Option<PathBuf>) {
        self.news.dir = dir;
        self.news.checked = false;
    }

    /// Aperçu hors ligne et tests : le bandeau, sans aucun fichier.
    #[doc(hidden)]
    pub fn news_preview(&mut self, notes: &str, open: bool) {
        self.news.pending = Some(notes.to_string());
        self.news.checked = true;
        self.news.open = open;
    }

    /// Le bandeau des notes de version est affiché.
    #[doc(hidden)]
    pub fn news_shown(&self) -> bool {
        self.news.pending.is_some()
    }

    /// « Télécharger » : garde les notes de la version pour le lancement qui suivra l'installation.
    pub(super) fn news_keep(&self, m: &crate::install::update::Manifest) {
        if let Some(dir) = &self.news.dir {
            news::save(dir, &m.version.to_string(), &m.notes);
        }
    }

    /// Lit les notes gardées, une fois par lancement.
    pub(super) fn news_tick(&mut self) {
        if self.news.checked {
            return;
        }
        self.news.checked = true;
        self.news.pending = self.news.dir.as_deref().and_then(|d| news::pending(d, env!("CARGO_PKG_VERSION")));
    }

    /// Bandeau du haut, jusqu'à « Fermer ».
    pub(super) fn news_banner(&mut self, ui: &mut egui::Ui) {
        let Some(notes) = self.news.pending.clone() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let page = source::repository().map(|repo| format!("https://github.com/{repo}/releases/tag/v{}", env!("CARGO_PKG_VERSION")));
        let (mut close, mut toggle, mut visit) = (false, false, false);
        let frame = egui::Frame::new().fill(t.colors.bg3).inner_margin(egui::Margin::symmetric(t.spacing.chat_head_x as i8, t.spacing.item_y as i8));
        egui::Panel::top("notes_de_version").frame(frame).resizable(false).show_separator_line(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    close = widgets::compact(ui, it::NEWS_CLOSE.get(lang)).clicked();
                    if page.is_some() {
                        visit = widgets::compact(ui, it::NEWS_PAGE.get(lang)).clicked();
                    }
                    if !notes.is_empty() {
                        toggle = widgets::compact(ui, if self.news.open { it::NEWS_HIDE } else { it::NEWS_SHOW }.get(lang)).clicked();
                    }
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.add(egui::Label::new(RichText::new(f::updated(lang, env!("CARGO_PKG_VERSION"))).small().color(t.colors.muted)).truncate());
                    });
                });
            });
            if self.news.open && !notes.is_empty() {
                ui.add_space(t.spacing.item_y);
                let max = (ui.ctx().content_rect().height() * 0.3).max(60.0);
                egui::ScrollArea::vertical().max_height(max).auto_shrink([false, true]).show(ui, |ui| {
                    ui.add(egui::Label::new(RichText::new(&notes).color(t.colors.text)).wrap());
                });
            }
        });
        if visit && let Some(url) = &page {
            crate::install::browser::open(url);
        }
        if toggle {
            self.news.open = !self.news.open;
        }
        if close {
            if let Some(dir) = &self.news.dir {
                news::clear(dir);
            }
            self.news.pending = None;
        }
        if close || toggle {
            ui.ctx().request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::keys::tests::app;
    use crate::ui::tests::{click, find, frame};

    #[test]
    fn banner_offers_the_notes_once() {
        let dir = std::env::temp_dir().join(format!("ferrocord-bandeau-notes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        news::save(&dir, env!("CARGO_PKG_VERSION"), "- Partage plus net");
        let mut a = app();
        a.attach_update_notes(Some(dir.clone()));
        let out = frame(&mut a, Vec::new());
        assert!(a.news_shown());
        let (show_label, close) = (it::NEWS_SHOW.get(a.lang), it::NEWS_CLOSE.get(a.lang));
        // Repliées d'abord ; un clic les montre.
        assert!(!a.news.open && find(&out, show_label).is_some());
        click(&mut a, show_label);
        let out = frame(&mut a, Vec::new());
        assert!(a.news.open && find(&out, it::NEWS_HIDE.get(a.lang)).is_some());
        // Fermé : le fichier est oublié, le bandeau ne revient pas.
        click(&mut a, close);
        frame(&mut a, Vec::new());
        assert!(!a.news_shown());
        assert_eq!(news::load(&dir), None);
        let mut b = app();
        b.attach_update_notes(Some(dir.clone()));
        frame(&mut b, Vec::new());
        assert!(!b.news_shown());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
