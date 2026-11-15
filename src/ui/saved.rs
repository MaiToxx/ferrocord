//! Messages enregistrés et rappels (lot ND) : l'onglet « Enregistrés » de
//! la boîte de réception, et l'annonce du résultat d'un « Enregistrer le
//! message » ou « Me le rappeler » fait depuis le menu d'un message.

use super::App;
use crate::i18n::table::nd::{f as nf, t as nt};
use crate::i18n::Lang;
use crate::state::quests::Refused;
use crate::state::saved::SavedNotice;
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Ce qu'un clic dans l'onglet demande.
pub(super) enum SavedPick {
    Jump { guild: Option<u64>, channel: u64, message: u64 },
    Remove { channel: u64, message: u64 },
    Refresh,
}

impl App {
    /// Résultat d'une écriture, annoncé une fois (appelé à chaque image de la discussion).
    pub(super) fn saved_notices(&mut self) {
        match self.store.saved.take_notice() {
            Some(SavedNotice::Saved) => self.notify(nt::SAVED_DONE),
            Some(SavedNotice::Reminder) => self.notify(nt::REMINDER_DONE),
            Some(SavedNotice::Refused(Refused::Captcha)) => self.notify(nt::CAPTCHA),
            Some(SavedNotice::Refused(Refused::Other(_))) => self.notify(nt::SAVED_REFUSED),
            None => {}
        }
    }

    pub(super) fn saved_tab(&self, ui: &mut egui::Ui, t: &Tokens, lang: Lang) -> Option<SavedPick> {
        let c = &t.colors;
        let now = self.store.now();
        let mut pick = None;
        let list = match &self.store.saved.list {
            Remote::Idle | Remote::Loading => {
                ui.spinner();
                return None;
            }
            Remote::Failed(p) => {
                let text = if *p == Problem::Forbidden { nt::SAVED_UNAVAILABLE.get(lang).to_string() } else { super::words::problem(lang, p, self.h24()) };
                ui.add(egui::Label::new(RichText::new(text).color(c.danger)).wrap());
                return ui.small_button(nt::REFRESH.get(lang)).clicked().then_some(SavedPick::Refresh);
            }
            Remote::Ready(l) => l,
        };
        if list.is_empty() {
            ui.add(egui::Label::new(RichText::new(nt::SAVED_EMPTY.get(lang)).color(c.muted)).wrap());
        }
        egui::ScrollArea::vertical().id_salt("boite_enregistres").auto_shrink([false, true]).show(ui, |ui| {
            for s in list {
                egui::Frame::new().fill(c.bg2).stroke(t.border()).corner_radius(t.r_embed()).inner_margin(t.embed_margin()).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    let place = match self.store.channel_name(s.channel_id) {
                        Some(name) if s.guild_id.is_some() => format!("#{name}"),
                        _ => String::new(),
                    };
                    let head = [s.author.as_str(), place.as_str()].into_iter().filter(|x| !x.is_empty()).collect::<Vec<_>>().join(" · ");
                    if !head.is_empty() {
                        ui.add(egui::Label::new(RichText::new(head).family(crate::ui::fonts::demi()).color(c.text_strong)).truncate());
                    }
                    let body = if s.excerpt.is_empty() { nt::SAVED_NO_TEXT.get(lang) } else { s.excerpt.as_str() };
                    ui.add(egui::Label::new(RichText::new(body).color(c.text)).wrap());
                    match s.due_ms {
                        Some(due) if due <= now => {
                            ui.label(RichText::new(nf::reminder_due(lang, crate::i18n::time::long_date(lang, due))).small().color(c.danger));
                        }
                        Some(due) => {
                            ui.label(RichText::new(nf::reminder_on(lang, crate::i18n::time::long_date(lang, due))).small().color(c.muted));
                        }
                        None => {}
                    }
                    ui.horizontal(|ui| {
                        if ui.small_button(crate::i18n::t::JUMP.get(lang)).clicked() {
                            pick = Some(SavedPick::Jump { guild: s.guild_id, channel: s.channel_id, message: s.message_id });
                        }
                        if ui.add_enabled(!self.store.saved.busy, egui::Button::new(nt::SAVED_REMOVE.get(lang)).small()).clicked() {
                            pick = Some(SavedPick::Remove { channel: s.channel_id, message: s.message_id });
                        }
                    });
                });
                ui.add_space(t.spacing.item_y);
            }
            if ui.small_button(nt::REFRESH.get(lang)).clicked() {
                pick = Some(SavedPick::Refresh);
            }
        });
        pick
    }
}
