//! Boutons et champs du gabarit (`widgets.rs`) sous la forme de widgets
//! d'egui, pour les pages de formulaire (paramètres du serveur, événements,
//! modération, invitations) : `ui.add(form::btn(..))` et
//! `ui.add_enabled(ok, form::primary(..))` remplacent les boutons d'egui sans
//! changer la mise en page autour.

use super::widgets::{self, Kind};
use crate::theme::Tokens;
use eframe::egui::{self, Margin, Response, RichText, Ui};

/// Bouton du gabarit, à poser avec `ui.add` ou `ui.add_enabled`.
pub(crate) struct Btn<'a> {
    t: &'a Tokens,
    kind: Kind,
    text: &'a str,
}

impl egui::Widget for Btn<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        widgets::button(ui, self.t, self.kind, self.text)
    }
}

/// Action ordinaire : aplat neutre.
pub(crate) fn btn<'a>(t: &'a Tokens, text: &'a str) -> Btn<'a> {
    Btn { t, kind: Kind::Secondary, text }
}

/// Action principale : aplat d'accent.
pub(crate) fn primary<'a>(t: &'a Tokens, text: &'a str) -> Btn<'a> {
    Btn { t, kind: Kind::Primary, text }
}

/// Action destructrice : aplat rouge.
pub(crate) fn danger<'a>(t: &'a Tokens, text: &'a str) -> Btn<'a> {
    Btn { t, kind: Kind::Danger, text }
}

/// Action discrète (« Annuler » au pied d'une fenêtre) : texte seul.
pub(crate) fn link<'a>(t: &'a Tokens, text: &'a str) -> Btn<'a> {
    Btn { t, kind: Kind::Link, text }
}

/// Champ sur une ligne au gabarit ([`widgets::field`]), borné à `max`
/// caractères (0 : sans borne).
pub(crate) fn field(ui: &mut Ui, t: &Tokens, text: &mut String, hint: &str, max: usize) -> Response {
    if max == 0 {
        return widgets::field(ui, t, text, hint);
    }
    let size = egui::vec2(ui.available_width(), t.field_height());
    let pad = (t.spacing.item_x + t.spacing.item_y).round().clamp(0.0, 127.0) as i8;
    let edit = egui::TextEdit::singleline(text)
        .hint_text(RichText::new(hint).color(t.colors.faint))
        .font(egui::FontId::proportional(t.typography.body))
        .margin(Margin::symmetric(pad, 0))
        .char_limit(max)
        .align(egui::Align2::LEFT_CENTER);
    ui.add_sized(size, edit)
}
