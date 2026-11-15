//! Menus contextuels de la zone de saisie et d'un lien (F7), et texte collé
//! trop long changé en fichier `message.txt` (F6), comme le client officiel.
//!
//! Le correcteur orthographique du client officiel n'a pas d'équivalent ici
//! (F32) : le menu de la saisie s'en tient au presse-papiers.

use super::{menu, App};
use crate::i18n::table::ff::t as ft;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Id};

/// Nom du fichier fait d'un texte collé trop long (celui de Discord).
pub(super) const PASTE_FILE: &str = "message.txt";

/// Retire de `events` les textes collés qui feraient dépasser la limite
/// (`room` : caractères encore libres dans le message) et les rend : ils
/// partent en pièce jointe au lieu d'entrer dans le champ.
pub(super) fn take_long_pastes(events: &mut Vec<egui::Event>, room: usize) -> Vec<String> {
    let mut out = Vec::new();
    events.retain(|e| match e {
        egui::Event::Paste(s) if s.chars().count() > room => {
            out.push(s.clone());
            false
        }
        _ => true,
    });
    out
}

/// Sélection de la saisie (caractères, début ≤ fin), gardée d'une image à
/// l'autre : le clic droit qui ouvre le menu peut déplacer le curseur.
#[derive(Clone, Copy, Default)]
struct Kept(Option<(usize, usize)>);

fn kept_id() -> Id {
    Id::unique("ferrocord.saisie.sélection")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Edit {
    Cut,
    Copy,
    Paste,
    SelectAll,
}

fn byte_at(text: &str, chars: usize) -> usize {
    text.char_indices().nth(chars).map_or(text.len(), |(i, _)| i)
}

/// Retire `a..b` (caractères) de `text` et rend le passage retiré.
pub(super) fn cut_range(text: &mut String, a: usize, b: usize) -> String {
    let (a, b) = (byte_at(text, a.min(b)), byte_at(text, a.max(b)));
    text.drain(a..b).collect()
}

impl App {
    /// Clic droit dans la zone de saisie : couper, copier, coller, tout sélectionner.
    pub(super) fn composer_context_menu(&mut self, ui: &egui::Ui, id: Id, resp: &egui::Response) {
        let ctx = ui.ctx().clone();
        let t = self.theme.clone();
        let lang = self.lang;
        let opened = resp.context_menu_opened();
        let right = ctx.input(|i| i.pointer.secondary_down() || i.pointer.secondary_released() || i.pointer.secondary_clicked());
        let state = egui::TextEdit::load_state(&ctx, id);
        // Sélection d'avant le clic droit.
        if !opened && !right {
            let now = state.as_ref().and_then(|s| s.cursor.char_range()).map(|r| (r.primary.index.0.min(r.secondary.index.0), r.primary.index.0.max(r.secondary.index.0)));
            ctx.data_mut(|d| d.insert_temp(kept_id(), Kept(now)));
        }
        let kept = ctx.data(|d| d.get_temp::<Kept>(kept_id())).unwrap_or_default().0;
        let len = self.input_chars();
        let sel = kept.filter(|(a, b)| a != b && *b <= len);
        let mut act = None;
        resp.context_menu(|ui| {
            let mut line = |ui: &mut egui::Ui, on: bool, text: &str, icon: Option<Icon>, e: Edit| {
                if ui.add_enabled_ui(on, |ui| menu::item(ui, &t, text, icon, false)).inner.clicked() {
                    act = Some(e);
                }
            };
            line(ui, sel.is_some(), ft::CUT.get(lang), None, Edit::Cut);
            line(ui, sel.is_some(), ft::COPY.get(lang), None, Edit::Copy);
            line(ui, true, ft::PASTE.get(lang), None, Edit::Paste);
            menu::separator(ui, &t);
            line(ui, len > 0, ft::SELECT_ALL.get(lang), None, Edit::SelectAll);
        });
        let Some(act) = act else { return };
        let mut state = state.unwrap_or_default();
        let set = |state: &mut egui::text_edit::TextEditState, a: usize, b: usize| {
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
        };
        match (act, sel) {
            (Edit::Cut, Some((a, b))) => {
                let cut = self.cut_composer(a, b);
                ctx.copy_text(cut);
                set(&mut state, a, a);
            }
            (Edit::Copy, Some((a, b))) => {
                ctx.copy_text(self.composer_text().chars().skip(a).take(b - a).collect());
                set(&mut state, a, b);
            }
            (Edit::Paste, _) => {
                // Le système rend le presse-papiers par un événement « coller »,
                // que le champ (ou la conversion en fichier) reçoit à l'image suivante.
                let (a, b) = kept.filter(|(_, b)| *b <= len).unwrap_or((len, len));
                set(&mut state, a, b);
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
            }
            (Edit::SelectAll, _) => set(&mut state, 0, len),
            _ => {}
        }
        egui::TextEdit::store_state(&ctx, id, state);
        ctx.memory_mut(|m| m.request_focus(id));
        ctx.request_repaint();
    }
}

/// Menu du clic droit sur un lien d'un message : ouvrir, copier.
pub(super) fn link_menu(ui: &mut egui::Ui, t: &Tokens, url: &str) {
    let lang = crate::i18n::language();
    if menu::item(ui, t, ft::OPEN_LINK.get(lang), Some(Icon::ArrowRight), false).clicked() && !super::search::follow_link(ui.ctx(), url) {
        ui.ctx().open_url(egui::OpenUrl::new_tab(url));
    }
    if menu::item(ui, t, ft::COPY_LINK.get(lang), None, false).clicked() {
        ui.ctx().copy_text(url.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_pastes_leave_the_field() {
        let mut events = vec![egui::Event::Paste("court".into()), egui::Event::Paste("é".repeat(11)), egui::Event::Copy];
        assert_eq!(take_long_pastes(&mut events, 10), vec!["é".repeat(11)]);
        assert_eq!(events, vec![egui::Event::Paste("court".into()), egui::Event::Copy]);
        assert!(take_long_pastes(&mut events, 5).is_empty(), "juste à la limite : reste du texte");
    }

    #[test]
    fn cut_works_on_characters() {
        let mut s = "été à Noël".to_string();
        assert_eq!(cut_range(&mut s, 6, 4), "à ");
        assert_eq!(s, "été Noël");
        assert_eq!(cut_range(&mut s, 3, 99), " Noël");
    }
}
