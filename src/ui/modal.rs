//! Fenêtre modale ouverte par un bot (`INTERACTION_MODAL_CREATE`, lot T11) :
//! champs de texte et listes, envoyés par « Envoyer » (interaction de type 5).
//! « Annuler » ou un clic en dehors la ferment sans rien envoyer.

use super::App;
use crate::i18n::table::interactions::{f, t as tx};
use crate::state::interactions::{Component, Modal, ModalValue, Select, SelectKind, TextInput};
use eframe::egui::{self, Id, RichText};

/// Un champ de la fenêtre.
#[derive(Clone, Debug)]
enum Input {
    Text(TextInput),
    Select(Select),
    /// Texte d'information (composant texte).
    Info(String),
}

#[derive(Clone, Debug)]
struct MField {
    /// Conteneur d'origine : rangée (1) ou étiquette (18).
    wrapper: u8,
    label: String,
    description: Option<String>,
    input: Input,
    text: String,
    picked: Vec<String>,
}

/// Fenêtre modale montrée, avec ce qui a été saisi.
#[derive(Clone, Debug)]
pub(super) struct ModalForm {
    modal: Box<Modal>,
    fields: Vec<MField>,
}

impl ModalForm {
    pub(super) fn new(modal: Box<Modal>) -> Self {
        let mut fields = Vec::new();
        for c in &modal.components {
            let (wrapper, label, description, inner) = match c {
                Component::Row(items) => match items.first() {
                    Some(i) => (1, String::new(), None, i),
                    None => continue,
                },
                Component::Label { label, description, child } => (18, label.clone(), description.clone(), &**child),
                other => (0, String::new(), None, other),
            };
            let (input, text, picked) = match inner {
                Component::TextInput(t) => (Input::Text(t.clone()), t.value.clone(), Vec::new()),
                Component::Select(s) => (Input::Select(s.clone()), String::new(), s.options.iter().filter(|o| o.default).map(|o| o.value.clone()).collect()),
                Component::Text(s) => (Input::Info(s.clone()), String::new(), Vec::new()),
                _ => continue,
            };
            // Ancienne forme : le libellé est porté par le champ de texte.
            let label = match (&input, label.is_empty()) {
                (Input::Text(t), true) => t.label.clone(),
                _ => label,
            };
            fields.push(MField { wrapper, label, description, input, text, picked });
        }
        ModalForm { modal, fields }
    }

    /// Valeurs à envoyer, ou `None` si un champ obligatoire manque ou déborde.
    pub(super) fn values(&self) -> Option<Vec<ModalValue>> {
        let mut out = Vec::new();
        for fl in &self.fields {
            match &fl.input {
                Input::Text(t) => {
                    let n = u32::try_from(fl.text.chars().count()).unwrap_or(u32::MAX);
                    let empty = fl.text.trim().is_empty();
                    if (t.required && empty) || (!empty && (t.min_length.is_some_and(|m| n < m) || t.max_length.is_some_and(|m| n > m))) {
                        return None;
                    }
                    out.push(ModalValue { kind: 4, custom_id: t.custom_id.clone(), text: Some(fl.text.clone()), values: Vec::new(), wrapper: fl.wrapper.max(1) });
                }
                Input::Select(s) => {
                    let n = fl.picked.len();
                    let min = if s.required { usize::from(s.min_values.max(1)) } else { usize::from(s.min_values) };
                    if n < min || n > usize::from(s.max_values) {
                        return None;
                    }
                    out.push(ModalValue { kind: s.kind.code(), custom_id: s.custom_id.clone(), text: None, values: fl.picked.clone(), wrapper: fl.wrapper.max(1) });
                }
                Input::Info(_) => {}
            }
        }
        Some(out)
    }

    /// Écrit `text` dans le champ de texte `index` (tests, aperçu).
    pub(super) fn set_text(&mut self, index: usize, text: &str) {
        if let Some(f) = self.fields.get_mut(index) {
            f.text = text.to_string();
        }
    }
}

impl App {
    /// Fenêtre modale d'un bot, si l'une est arrivée ou est ouverte.
    pub(super) fn app_modal_ui(&mut self, ui: &mut egui::Ui) {
        if let Some(m) = self.store.take_modal() {
            self.cmds.modal = Some(ModalForm::new(m));
        }
        let Some(mut form) = self.cmds.modal.take() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let store = &self.store;
        let values = form.values();
        let (mut yes, mut no) = (false, false);
        // Gabarit commun des fenêtres (titre et croix, contenu qui défile, pied à droite).
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let title = form.modal.title.clone();
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            &t,
            Id::unique(("modale-appli", form.modal.id)),
            width,
            &title,
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, &t);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(tx::APP_BADGE.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.accent));
                    ui.label(RichText::new(&form.modal.app_name).small().color(t.colors.muted));
                });
                let channel = form.modal.channel_id;
                for (i, fl) in form.fields.iter_mut().enumerate() {
                    ui.add_space(t.spacing.media_gap);
                    if !fl.label.is_empty() {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&fl.label).family(crate::ui::fonts::demi()).strong().color(t.colors.text));
                            let required = match &fl.input {
                                Input::Text(x) => x.required,
                                Input::Select(s) => s.required,
                                Input::Info(_) => false,
                            };
                            if required {
                                ui.label(RichText::new(tx::REQUIRED.get(lang)).small().color(t.colors.danger));
                            }
                        });
                    }
                    if let Some(d) = &fl.description {
                        ui.label(RichText::new(d).small().color(t.colors.muted));
                    }
                    match &fl.input {
                        Input::Info(s) => {
                            ui.label(RichText::new(s).color(t.colors.text));
                        }
                        Input::Text(x) => {
                            let mut edit = if x.paragraph { egui::TextEdit::multiline(&mut fl.text).desired_rows(3) } else { egui::TextEdit::singleline(&mut fl.text) };
                            edit = edit.desired_width(f32::INFINITY);
                            if let Some(p) = &x.placeholder {
                                edit = edit.hint_text(p.as_str());
                            }
                            if let Some(max) = x.max_length {
                                edit = edit.char_limit(max as usize);
                            }
                            let r = ui.add(edit);
                            if i == 0 && !r.has_focus() && fl.text.is_empty() && ui.memory(|m| m.focused().is_none()) {
                                r.request_focus();
                            }
                            if let Some(min) = x.min_length.filter(|m| *m > 0) {
                                ui.label(RichText::new(f::min_length(lang, min)).small().color(t.colors.faint));
                            }
                        }
                        Input::Select(s) => {
                            let s = s.clone();
                            let entries: Vec<(String, String)> = match s.kind {
                                SelectKind::String => s.options.iter().map(|o| (o.value.clone(), o.label.clone())).collect(),
                                _ => super::components::select_entries(store, channel, &s, ""),
                            };
                            if s.max_values > 1 {
                                for (value, label) in entries {
                                    let mut on = fl.picked.contains(&value);
                                    if ui.checkbox(&mut on, label).changed() {
                                        if on {
                                            fl.picked.push(value);
                                        } else {
                                            fl.picked.retain(|v| *v != value);
                                        }
                                    }
                                }
                            } else {
                                let shown = entries.iter().find(|(v, _)| fl.picked.first() == Some(v)).map(|(_, l)| l.clone());
                                let shown = shown.or_else(|| s.placeholder.clone()).unwrap_or_else(|| tx::CHOOSE.get(lang).to_string());
                                egui::ComboBox::from_id_salt(("modale-liste", i)).width(ui.available_width()).truncate().selected_text(shown).show_ui(ui, |ui| {
                                    for (value, label) in entries {
                                        if ui.selectable_label(fl.picked.first() == Some(&value), label).clicked() {
                                            fl.picked = vec![value];
                                        }
                                    }
                                });
                            }
                        }
                    }
                }
                if values.is_none() {
                    ui.add_space(t.spacing.media_gap);
                    ui.label(RichText::new(tx::FILL_REQUIRED.get(lang)).small().color(t.colors.faint));
                }
            },
            |ui| {
                // Pied : le principal au bout à droite, « Annuler » à sa gauche.
                use crate::ui::widgets::{button, Kind};
                yes = ui.add_enabled_ui(values.is_some(), |ui| button(ui, &t, Kind::Primary, tx::SEND.get(lang))).inner.clicked();
                no = button(ui, &t, Kind::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        if yes && let Some(v) = values {
            if self.store.submit_modal(&form.modal, v) {
                return;
            }
            self.notify(tx::NOT_CONNECTED);
        } else if no || modal.closed {
            return;
        }
        self.cmds.modal = Some(form);
    }

    /// Fenêtre modale ouverte (aperçu, tests) : écrit dans son champ `index`.
    pub fn set_modal_text(&mut self, index: usize, text: &str) {
        if let Some(m) = self.cmds.modal.as_mut() {
            m.set_text(index, text);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::interactions::SelectOption;

    fn text(id: &str, required: bool) -> TextInput {
        TextInput { custom_id: id.into(), paragraph: false, label: format!("L-{id}"), min_length: Some(2), max_length: Some(5), required, value: String::new(), placeholder: None }
    }

    #[test]
    fn values_follow_the_rules_of_each_field() {
        let select = Select {
            kind: SelectKind::String,
            custom_id: "note".into(),
            placeholder: None,
            min_values: 1,
            max_values: 1,
            options: vec![SelectOption { label: "Bien".into(), value: "5".into(), description: None, emoji: None, default: false }],
            disabled: false,
            required: true,
            channel_types: Vec::new(),
        };
        let modal = Modal {
            id: 9,
            channel_id: 1,
            custom_id: "avis".into(),
            title: "Avis".into(),
            app_id: 2,
            app_name: "Bot".into(),
            components: vec![
                Component::Row(vec![Component::TextInput(text("a", true))]),
                Component::Label { label: "B".into(), description: None, child: Box::new(Component::TextInput(text("b", false))) },
                Component::Label { label: "Note".into(), description: None, child: Box::new(Component::Select(select)) },
            ],
            nonce: None,
        };
        let mut f = ModalForm::new(Box::new(modal));
        assert_eq!(f.fields[0].label, "L-a", "libellé de l'ancienne forme");
        assert!(f.values().is_none(), "champ obligatoire vide");
        f.set_text(0, "x");
        assert!(f.values().is_none(), "trop court");
        f.set_text(0, "oui");
        assert!(f.values().is_none(), "liste obligatoire sans choix");
        f.fields[2].picked = vec!["5".into()];
        let v = f.values().unwrap_or_else(|| panic!("valeurs"));
        assert_eq!(v.iter().map(|x| (x.kind, x.custom_id.as_str(), x.wrapper)).collect::<Vec<_>>(), vec![(4, "a", 1), (4, "b", 18), (3, "note", 18)]);
        assert_eq!(v[1].text.as_deref(), Some(""), "facultatif vide : envoyé vide");
        f.set_text(1, "beaucoup trop long");
        assert!(f.values().is_none());
    }
}
