//! Bandeau du mode streamer (lot C5) : une ligne discrète en haut de la
//! fenêtre tant que le mode est actif, comme Discord, avec « Désactiver ».

use super::App;
use crate::i18n::table::a11y::t as tx;
use eframe::egui::{self, RichText};

impl App {
    pub(super) fn streamer_banner(&mut self, ui: &mut egui::Ui) {
        if !self.local.prefs.streamer.on {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let mut off = false;
        let frame = egui::Frame::new().fill(t.colors.accent).inner_margin(egui::Margin::symmetric(t.spacing.chat_head_x as i8, t.spacing.item_y as i8));
        egui::Panel::top("mode_streamer").frame(frame).resizable(false).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tx::STREAMER_BANNER.get(lang)).small().color(t.colors.on_accent));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = egui::Button::new(RichText::new(tx::STREAMER_DISABLE.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.on_accent)).fill(t.colors.accent).stroke(egui::Stroke::new(t.shapes.border.max(1.0), t.colors.on_accent));
                    off = ui.add(b).clicked();
                });
            });
        });
        if off {
            let mut p = self.local.prefs.clone();
            p.streamer.on = false;
            self.set_local_prefs(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::ui::keys::tests::app;

    /// Mode actif : le bandeau et son bouton sont là ; « Désactiver » coupe
    /// le mode (enregistré) et le bandeau part.
    #[test]
    fn banner_shows_and_turns_streamer_mode_off() {
        let mut a = app();
        let mut p = a.local_prefs();
        p.streamer.on = true;
        a.set_local_prefs(p);
        crate::ui::tests::click(&mut a, "Désactiver");
        assert!(!a.local_prefs().streamer.on);
        let out = crate::ui::tests::frame(&mut a, Vec::new());
        assert!(crate::ui::tests::find(&out, "mode streamer est activé").is_none());
    }

    #[test]
    fn no_banner_when_off() {
        let mut a = app();
        let out = crate::ui::tests::frame(&mut a, Vec::new());
        assert!(crate::ui::tests::find(&out, "mode streamer est activé").is_none() && crate::ui::tests::find(&out, "Désactiver").is_none());
    }
}
