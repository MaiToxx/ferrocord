//! Page Ferrocord : démarrage avec Windows, liens Discord, mises à jour ;
//! page À propos : textes complets des licences (lot C6). Chaque changement
//! part au fil d'installation ; rien ne touche au registre ni au disque ici.

use super::{note, section, toggle, value_row, App};
use crate::i18n::table::install::{f, t as it};
use crate::install::update::{source, Status};
use crate::install::{licenses, Autostart, Protocol};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use std::sync::Arc;

impl App {
    pub(super) fn install_settings(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        section(ui, t, it::SEC_STARTUP.get(lang));
        let auto = self.install.autostart;
        let on = auto.map(|a| a != Autostart::Off);
        let minimized = auto.map(|a| matches!(a, Autostart::On { minimized: true }));
        if let Some(v) = toggle(ui, it::AUTOSTART.get(lang), it::AUTOSTART_TIP.get(lang), on, true) {
            let want = if v { Autostart::On { minimized: minimized.unwrap_or(false) } } else { Autostart::Off };
            self.install.backend.set_autostart(want);
        }
        if let Some(v) = toggle(ui, it::MINIMIZED.get(lang), it::MINIMIZED_TIP.get(lang), minimized, on == Some(true)) {
            self.install.backend.set_autostart(Autostart::On { minimized: v });
        }

        section(ui, t, it::SEC_LINKS.get(lang));
        let proto = self.install.protocol.clone();
        if let Some(v) = toggle(ui, it::PROTOCOL.get(lang), it::PROTOCOL_TIP.get(lang), proto.as_ref().map(|p| *p == Protocol::Ours), true) {
            self.install.backend.set_protocol(v);
        }
        if let Some(Protocol::Other(cmd)) = &proto {
            note(ui, t, &f::other_handler(lang, cmd));
        }

        section(ui, t, it::SEC_UPDATES.get(lang));
        note(ui, t, it::UPDATES_TIP.get(lang));
        // Adresse de local.toml, sinon celle du dépôt intégré à la compilation.
        let url = source::effective(&self.local.prefs.update_manifest, &source::default_manifest_url());
        let has_key = !matches!(self.install.update, Some(Status::NoKey));
        if let Some(v) = toggle(ui, it::AUTO_CHECK.get(lang), it::AUTO_CHECK_TIP.get(lang), self.install.backend.auto_check().map(|on| on && !url.is_empty() && has_key), !url.is_empty() && has_key) {
            self.install.backend.set_auto_check(v);
        }
        if !url.is_empty() {
            value_row(ui, it::SOURCE.get(lang), it::SOURCE_TIP.get(lang), &url);
        }
        let status = self.install.update.clone();
        let line = match &status {
            None => it::UNKNOWN.get(lang).to_string(),
            Some(Status::NoSource) => it::NO_SOURCE.get(lang).to_string(),
            Some(Status::NoKey) => it::NO_KEY.get(lang).to_string(),
            Some(Status::Idle) => it::IDLE.get(lang).to_string(),
            Some(Status::Checking) => it::CHECKING.get(lang).to_string(),
            Some(Status::UpToDate { refused: None }) => it::UP_TO_DATE.get(lang).to_string(),
            Some(Status::UpToDate { refused: Some(v) }) => f::refused(lang, v),
            Some(Status::Available(m)) => f::available(lang, m.version),
            Some(Status::Downloading { done, total }) => f::downloading(lang, done.saturating_mul(100) / (*total).max(1)),
            Some(Status::Ready(v)) => f::ready(lang, v),
            Some(Status::Failed(why)) => f::failed(lang, why),
        };
        ui.label(RichText::new(line).color(t.colors.text));
        if let Some(Status::Available(m)) = &status
            && !m.notes.is_empty()
        {
            note(ui, t, &m.notes);
        }
        // Le bouton de l'étape en cours (chaque étape suit un clic).
        let label = match &status {
            Some(Status::Available(_)) => Some(it::DOWNLOAD),
            Some(Status::Ready(_)) => Some(it::RESTART),
            Some(Status::UpToDate { .. } | Status::Failed(_) | Status::Idle) if !url.is_empty() => Some(it::CHECK_NOW),
            _ => None,
        };
        if matches!(status, Some(Status::Checking | Status::Downloading { .. })) {
            ui.spinner();
        }
        if let Some(label) = label
            && ui.button(label.get(lang)).clicked()
        {
            match &status {
                Some(Status::Available(m)) => {
                    self.news_keep(m);
                    self.install.backend.download(m.clone());
                }
                Some(Status::Ready(_)) => {
                    crate::install::request_restart();
                    self.quit_app();
                }
                _ => self.install.backend.check_updates(),
            }
        }
        if matches!(status, Some(Status::Ready(_))) {
            note(ui, t, it::READY_TIP.get(lang));
        }
    }

    /// Licences des dépendances : liste filtrable (virtualisée), texte
    /// complet du paquet choisi.
    pub(super) fn licenses_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let loaded = self.install.licenses.get_or_insert_with(|| licenses::embedded().map(Arc::new)).clone();
        let l = match loaded {
            Ok(l) => l,
            Err(why) => {
                note(ui, t, &f::licenses_failed(lang, why));
                return;
            }
        };
        note(ui, t, &f::packages(lang, l.packages.len()));
        ui.add(egui::TextEdit::singleline(&mut self.install.license_filter).hint_text(it::LICENSE_FILTER.get(lang)).desired_width(f32::INFINITY));
        let q = self.install.license_filter.trim().to_lowercase();
        let shown: Vec<usize> = (0..l.packages.len()).filter(|i| q.is_empty() || l.packages[*i].name.to_lowercase().contains(&q) || l.packages[*i].license.to_lowercase().contains(&q)).collect();
        let row = ui.text_style_height(&egui::TextStyle::Body) + t.spacing.item_y;
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("licences-liste").max_height(row * 10.0).auto_shrink([false, true]).show_rows(ui, row, shown.len(), |ui, range| {
            for i in &shown[range] {
                let p = &l.packages[*i];
                let selected = self.install.license_open == Some(*i);
                let text = format!("{} {} — {}", p.name, p.version, p.license);
                if ui.selectable_label(selected, RichText::new(text).color(if selected { t.colors.text_strong } else { t.colors.text })).clicked() {
                    pick = Some(if selected { None } else { Some(*i) });
                }
            }
        });
        if let Some(p) = pick {
            self.install.license_open = p;
        }
        let Some(p) = self.install.license_open.and_then(|i| l.packages.get(i)) else {
            note(ui, t, it::LICENSE_PICK.get(lang));
            return;
        };
        ui.add_space(t.spacing.item_y);
        ui.label(RichText::new(format!("{} {}", p.name, p.version)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
        let lines: Vec<&str> = l.texts_of(p).flat_map(|s| s.lines().chain(std::iter::once(""))).collect();
        let mono = egui::TextStyle::Monospace;
        let line_h = ui.text_style_height(&mono);
        egui::ScrollArea::both().id_salt(("licences-texte", p.name.as_str())).max_height(line_h * 20.0).auto_shrink([false, true]).show_rows(ui, line_h, lines.len(), |ui, range| {
            for line in &lines[range] {
                ui.add(egui::Label::new(RichText::new(*line).text_style(mono.clone()).color(t.colors.muted)).extend());
            }
        });
    }
}
