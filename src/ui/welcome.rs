//! Accueil des nouveaux membres (lot A7) : écran de bienvenue (description
//! et salons recommandés, un clic ouvre le salon) et questions de
//! l'intégration (« Salons et rôles »), montrés à l'arrivée sur un serveur
//! qui les utilise, ou depuis le menu du serveur. Les réponses ne partent
//! qu'avec « Enregistrer ».

use super::emoji::{self, Img};
use super::App;
use crate::i18n::table::events::{f, t as tx};
use crate::state::admin::community::{Arrival, CommunityAction, Loadable, Onboarding};
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText, Sense};

/// Emoji d'une option ou d'un salon : personnalisé (identifiant) ou Unicode
/// connu de la table ; rien sinon.
pub(super) fn emoji_icon(ui: &mut egui::Ui, id: Option<u64>, name: Option<&str>, side: f32) {
    let img = match (id, name) {
        (Some(id), _) => Some(Img::Custom(id)),
        (None, Some(n)) => crate::state::emoji::find(n).map(Img::Unicode),
        _ => None,
    };
    if let Some(img) = img {
        emoji::icon(ui, img, side, Sense::hover());
    }
}

impl App {
    pub(super) fn arrival_windows(&mut self, ui: &mut egui::Ui) {
        self.browse_window(ui);
        match self.store.community.show {
            Some(Arrival::Welcome(g)) => self.welcome_window(ui, g),
            Some(Arrival::Onboarding(g)) => self.onboarding_window(ui, g),
            None => {}
        }
    }

    fn welcome_window(&mut self, ui: &mut egui::Ui, guild: u64) {
        let Some(gname) = self.store.guilds.iter().find(|g| g.id == guild).map(|g| super::words::guild_name(self.lang, &g.name).to_string()) else {
            self.store.community.show = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = self.h24();
        let screen = self.store.community.welcome.get(&guild).cloned();
        let mut open = None;
        // Gabarit commun des fenêtres : titre (à gauche, comme le contenu) et croix ;
        // pas de pied, les salons recommandés se cliquent.
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.3);
        let modal = crate::ui::widgets::sheet(ui.ctx(), &t, Id::unique("bienvenue"), width, &f::welcome_to(lang, &gname), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            match &screen {
                None | Some(Loadable::Loading) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tx::LOADING.get(lang));
                    });
                }
                Some(Loadable::Failed(fl)) => {
                    ui.label(RichText::new(super::events::failure_text(lang, fl, h24)).color(t.colors.danger));
                }
                Some(Loadable::Ready(s)) => {
                    if !s.description.is_empty() {
                        ui.label(RichText::new(&s.description).color(t.colors.text));
                    }
                    if s.channels.is_empty() && s.description.is_empty() {
                        ui.label(RichText::new(tx::WELCOME_NONE.get(lang)).color(t.colors.muted));
                    }
                    let side = t.typography.title * 1.4;
                    for c in &s.channels {
                        let Some(name) = self.store.channel_name(c.channel_id).map(str::to_string) else { continue };
                        let row = egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_row()).inner_margin(t.card_margin()).show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                emoji_icon(ui, c.emoji_id, c.emoji_name.as_deref(), side);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(format!("#{name}")).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                                    ui.label(RichText::new(&c.description).small().color(t.colors.muted));
                                });
                            });
                        });
                        let resp = ui.interact(row.response.rect, Id::unique(("bienvenue-salon", c.channel_id)), Sense::click());
                        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            open = Some(c.channel_id);
                        }
                    }
                }
            }
        });
        if let Some(c) = open {
            self.store.community.show = None;
            if self.store.guild != Some(guild) {
                self.store.select_guild(guild);
            }
            self.open_channel(c);
        }
        if modal.closed {
            self.store.community.show = None;
        }
    }

    fn onboarding_window(&mut self, ui: &mut egui::Ui, guild: u64) {
        let Some(gname) = self.store.guilds.iter().find(|g| g.id == guild).map(|g| super::words::guild_name(self.lang, &g.name).to_string()) else {
            self.store.community.show = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = self.h24();
        let state = self.store.community.onboarding.get(&guild).cloned();
        // Réponses de départ : celles que Discord a gardées.
        if let Some(Loadable::Ready(o)) = &state
            && self.events_ui.answers.as_ref().is_none_or(|(g, _)| *g != guild)
        {
            self.events_ui.answers = Some((guild, o.responses.clone()));
        }
        let busy = self.store.community.pending(CommunityAction::Respond, guild);
        let (mut save, mut close) = (false, false);
        let mut chosen = self.events_ui.answers.as_ref().filter(|(g, _)| *g == guild).map(|(_, a)| a.clone()).unwrap_or_default();
        // « Enregistrer » n'a de sens que devant des questions.
        let asks = matches!(&state, Some(Loadable::Ready(o)) if o.enabled && !o.prompts.is_empty());
        // Gabarit commun des fenêtres : titre et croix, questions qui défilent, pied à droite.
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.4);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("integration"), width, &f::welcome_to(lang, &gname), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.label(RichText::new(tx::CHANNELS_ROLES.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text));
            match &state {
                None | Some(Loadable::Loading) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tx::LOADING.get(lang));
                    });
                }
                Some(Loadable::Failed(fl)) => {
                    ui.label(RichText::new(super::events::failure_text(lang, fl, h24)).color(t.colors.danger));
                }
                Some(Loadable::Ready(o)) if !o.enabled || o.prompts.is_empty() => {
                    ui.label(RichText::new(tx::ONBOARDING_OFF.get(lang)).color(t.colors.muted));
                }
                Some(Loadable::Ready(o)) => {
                    ui.label(RichText::new(tx::BROWSE_TIP.get(lang)).small().color(t.colors.muted));
                    prompts_ui(ui, &t, lang, o, &mut chosen);
                }
            }
        }, |ui| {
            // Pied : le principal au bout à droite, « Fermer » à sa gauche.
            use crate::ui::widgets::{button, Kind};
            if asks {
                save = ui.add_enabled_ui(!busy, |ui| button(ui, &t, Kind::Primary, tx::SAVE.get(lang))).inner.clicked();
            }
            close = button(ui, &t, if asks { Kind::Link } else { Kind::Secondary }, tx::CLOSE.get(lang)).clicked();
            if busy {
                ui.spinner();
            }
        });
        if save && let Err(e) = self.store.respond_onboarding(guild, &chosen) {
            self.notify(super::events::save_error(e));
        }
        if let Some((g, a)) = self.events_ui.answers.as_mut()
            && *g == guild
        {
            *a = chosen;
        }
        if close || modal.closed {
            self.store.community.show = None;
            self.events_ui.answers = None;
        }
    }

    /// Ouvre l'écran de bienvenue ou les questions d'un serveur (aperçu, tests).
    pub fn show_arrival(&mut self, guild: u64, onboarding: bool) {
        let a = if onboarding { Arrival::Onboarding(guild) } else { Arrival::Welcome(guild) };
        self.store.open_arrival(a);
        self.flush_commands();
    }
}

/// Questions et options ; un clic coche ou décoche (choix unique : remplace).
fn prompts_ui(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, o: &Onboarding, chosen: &mut Vec<u64>) {
    let side = t.typography.body * 1.3;
    for p in &o.prompts {
        ui.add_space(t.spacing.item_y);
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(&p.title).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
            if p.required {
                ui.label(RichText::new(tx::REQUIRED.get(lang)).small().color(t.colors.danger));
            }
            if p.single_select {
                ui.label(RichText::new(tx::SINGLE.get(lang)).small().color(t.colors.muted));
            }
        });
        // Cartes d'une même question de même hauteur : celle d'un titre et de son
        // sous-titre dès qu'une option en a un (sinon la carte à sous-titre dépassait ses voisines).
        // Chaque carte est mesurée puis dessinée à la main : la hauteur ne dépend plus de son contenu.
        let body = egui::TextStyle::Body.resolve(ui.style());
        let small = egui::TextStyle::Small.resolve(ui.style());
        let (pad, gap) = (t.spacing.card_pad, t.spacing.item_y / 2.0);
        let img_of = |o: &crate::state::admin::community::PromptOption| match (o.emoji_id, o.emoji_name.as_deref()) {
            (Some(id), _) => Some(Img::Custom(id)),
            (None, Some(n)) => crate::state::emoji::find(n).map(Img::Unicode),
            _ => None,
        };
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let two = line + gap + ui.text_style_height(&egui::TextStyle::Small);
        let tall = if p.options.iter().any(|o| !o.description.is_empty()) { two } else { line };
        let inner = if p.options.iter().any(|o| img_of(o).is_some()) { tall.max(side) } else { tall };
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.y = ui.spacing().item_spacing.x;
            for opt in &p.options {
                let on = chosen.contains(&opt.id);
                let img = img_of(opt);
                let lead = if img.is_some() { side + t.spacing.item_x } else { 0.0 };
                let room = (ui.max_rect().width() - 2.0 * pad - lead).max(1.0);
                let ink = if on { t.colors.text_strong } else { t.colors.text };
                let title = ui.painter().layout_job(one_line(&opt.title, body.clone(), ink, room));
                let desc = (!opt.description.is_empty()).then(|| ui.painter().layout_job(one_line(&opt.description, small.clone(), t.colors.muted, room)));
                let text_w = title.size().x.max(desc.as_ref().map_or(0.0, |d| d.size().x));
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(lead + text_w + 2.0 * pad, inner + 2.0 * pad), Sense::click());
                if ui.is_rect_visible(rect) {
                    let fill = if on { t.colors.selected } else if resp.hovered() { t.colors.row_hover } else { t.colors.bg1 };
                    ui.painter().rect_filled(rect, t.r_row(), fill);
                    if let Some(img) = img {
                        emoji::paint(ui, crate::ui::util::pixel_square(ui, egui::pos2(rect.left() + pad, rect.center().y - side / 2.0), side), img);
                    }
                    let text_h = title.size().y + desc.as_ref().map_or(0.0, |d| gap + d.size().y);
                    let at = egui::pos2(rect.left() + pad + lead, rect.center().y - text_h / 2.0);
                    let below = at.y + title.size().y + gap;
                    ui.painter().galley(at, title, ink);
                    if let Some(d) = desc {
                        ui.painter().galley(egui::pos2(at.x, below), d, t.colors.muted);
                    }
                }
                resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, &opt.title));
                crate::ui::keys::focus_ring(ui, t, &resp);
                if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    toggle(p, chosen, opt.id);
                }
            }
        });
    }
}

/// Texte sur une ligne, fini par « … » au-delà de `max_width`.
fn one_line(text: &str, font: egui::FontId, color: egui::Color32, max_width: f32) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping { max_width, max_rows: 1, break_anywhere: true, ..Default::default() };
    job
}

/// Coche ou décoche une option ; à choix unique, elle remplace les autres de sa question.
fn toggle(p: &crate::state::admin::community::Prompt, chosen: &mut Vec<u64>, id: u64) {
    if chosen.contains(&id) {
        chosen.retain(|x| *x != id);
        return;
    }
    if p.single_select {
        chosen.retain(|x| !p.options.iter().any(|o| o.id == *x));
    }
    chosen.push(id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::admin::community::{Prompt, PromptOption};

    #[test]
    fn single_choice_replaces_and_click_toggles() {
        let opt = |id| PromptOption { id, ..Default::default() };
        let mut p = Prompt { id: 1, single_select: true, options: vec![opt(11), opt(12)], ..Default::default() };
        let mut chosen = vec![11, 99];
        toggle(&p, &mut chosen, 12);
        assert_eq!(chosen, vec![99, 12], "remplace l'autre option de la question seulement");
        toggle(&p, &mut chosen, 12);
        assert_eq!(chosen, vec![99]);
        p.single_select = false;
        toggle(&p, &mut chosen, 11);
        toggle(&p, &mut chosen, 12);
        assert_eq!(chosen, vec![99, 11, 12]);
    }
}
