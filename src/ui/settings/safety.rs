//! Section « État du compte » de la page Confidentialité (lot ND) : ce que
//! le centre de sécurité de Discord dit du compte, lu au clic. Lecture
//! seule : les recours se font dans l'application officielle.

use super::{note, section, App};
use crate::i18n::table::nd::{f as nf, p as np, t as nt};
use crate::state::safety::Standing;
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

impl App {
    /// « Vérifier auprès de Discord » (aperçu, tests).
    pub fn check_account_standing(&mut self) -> bool {
        self.store.safety_hub_load()
    }

    pub(super) fn safety_section(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let c = &t.colors;
        section(ui, t, nt::SAFETY.get(lang));
        note(ui, t, nt::SAFETY_TIP.get(lang));
        let busy = matches!(self.store.safety_hub, Remote::Loading);
        let mut load = false;
        ui.horizontal(|ui| {
            load = ui.add_enabled(!busy, egui::Button::new(nt::SAFETY_CHECK.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        });
        let mut open = None;
        match &self.store.safety_hub {
            Remote::Idle | Remote::Loading => {}
            Remote::Failed(Problem::Forbidden) => {
                ui.label(RichText::new(nt::SAFETY_REFUSED.get(lang)).color(c.danger));
            }
            Remote::Failed(p) => {
                ui.label(RichText::new(crate::ui::words::problem(lang, p, self.h24())).color(c.danger));
            }
            Remote::Ready(hub) => {
                let (text, color) = match hub.standing {
                    Standing::AllGood => (nt::STANDING_GOOD, c.online),
                    Standing::Limited => (nt::STANDING_LIMITED, c.text_strong),
                    Standing::VeryLimited => (nt::STANDING_VERY_LIMITED, c.danger),
                    Standing::AtRisk => (nt::STANDING_AT_RISK, c.danger),
                    Standing::Suspended => (nt::STANDING_SUSPENDED, c.danger),
                    Standing::Unknown => (nt::STANDING_UNKNOWN, c.muted),
                };
                ui.label(RichText::new(text.get(lang)).family(crate::ui::fonts::demi()).color(color));
                if hub.violations.is_empty() && hub.guild_violations == 0 {
                    note(ui, t, nt::SAFETY_NONE.get(lang));
                }
                for v in &hub.violations {
                    egui::Frame::new().fill(t.card_fill()).stroke(t.border()).corner_radius(t.r_row()).inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, t.spacing.item_y as i8)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.add(egui::Label::new(RichText::new(&v.description).color(c.text_strong)).wrap());
                        for a in &v.actions {
                            ui.add(egui::Label::new(RichText::new(format!("• {a}")).color(c.text)).wrap());
                        }
                        if let Some(end) = v.expires_ms {
                            ui.label(RichText::new(nf::safety_until(lang, crate::i18n::time::long_date(lang, end))).small().color(c.muted));
                        }
                        if let Some(url) = &v.link
                            && ui.link(nt::SAFETY_MORE.get(lang)).clicked()
                        {
                            open = Some(url.clone());
                        }
                    });
                    ui.add_space(t.spacing.item_y / 2.0);
                }
                if hub.guild_violations > 0 {
                    note(ui, t, &np::guild_violations(lang, hub.guild_violations as u64));
                }
                if hub.appeal_eligible || !hub.violations.is_empty() {
                    note(ui, t, nt::SAFETY_APPEAL.get(lang));
                }
            }
        }
        if load {
            self.store.safety_hub_load();
        }
        if let Some(url) = open {
            self.open_link(&url);
        }
    }
}
