//! Fenêtre « Paramètres de notification » d'un serveur (lot N) : muet,
//! niveau du serveur, @everyone et rôles, puis le tableau des exceptions par
//! salon ou par catégorie (niveau propre, muet, retrait). Chaque geste envoie
//! une seule modification, par la même écriture que le menu du clic droit
//! (`AccountState::change_notif`).

use super::settings::shell::switch_label;
use super::util::icon_button;
use super::widgets::{self, Kind};
use super::{settings, words, App};
use crate::i18n::table::ff::t as ft;
use crate::i18n::table::lotn::{f as xf, t as xt};
use crate::i18n::table::s::t as st;
use crate::i18n::time;
use crate::state::model::ChannelKind;
use crate::state::usersettings::notif::{Change, Level, Mute};
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Align, Id, Layout, RichText, Sense};

/// Propositions montrées sous le champ de recherche, au plus.
const MAX_HITS: usize = 6;

/// La fenêtre ouverte.
pub(super) struct Win {
    guild: u64,
    query: String,
    /// Salons ajoutés au tableau sans réglage encore choisi (une exception
    /// n'existe pour Discord qu'une fois un niveau ou le muet choisi).
    added: Vec<u64>,
}

/// Icône et nom d'un salon, dans la place qui reste à gauche.
fn name_cell(ui: &mut egui::Ui, t: &Tokens, name: &str, category: bool) {
    let side = Tokens::icon_side(t.typography.body);
    let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
    icons::paint(if category { Icon::ChevronDown } else { Icon::Hash }, ui.painter(), slot, t.colors.muted);
    ui.add(egui::Label::new(RichText::new(name).color(t.colors.text_strong)).truncate().selectable(false));
}

impl App {
    pub(super) fn open_notif_settings(&mut self, guild: u64) {
        self.nav.notif_win = Some(Win { guild, query: String::new(), added: Vec::new() });
        self.ctx.request_repaint();
    }

    /// Ouvre la fenêtre (aperçu, tests) ; `query` : texte du champ de recherche.
    pub fn show_notif_settings(&mut self, guild: u64, query: &str) -> bool {
        if !self.store.guilds.iter().any(|g| g.id == guild) {
            return false;
        }
        self.open_notif_settings(guild);
        if let Some(w) = self.nav.notif_win.as_mut() {
            w.query = query.to_string();
        }
        true
    }

    /// Ajoute un salon au tableau comme un clic sur une proposition (tests).
    pub fn notif_settings_add(&mut self, channel: u64) {
        if let Some(w) = self.nav.notif_win.as_mut()
            && !w.added.contains(&channel)
        {
            w.added.push(channel);
        }
    }

    pub(super) fn notif_window(&mut self, ui: &mut egui::Ui) {
        let Some(mut win) = self.nav.notif_win.take() else { return };
        // Serveur quitté entre-temps : la fenêtre se ferme.
        let Some(g) = self.store.guilds.iter().find(|g| g.id == win.guild) else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = self.h24();
        let name = words::guild_name(lang, &g.name).to_string();
        let chans: Vec<(u64, String, bool)> = g.channels.iter().map(|c| (c.id, c.name.clone(), c.kind == ChannelKind::Category)).collect();
        let settings = self.store.account.notifs.get(Some(win.guild));
        let now = self.store.now();
        let busy = self.store.account.guild_busy(Some(win.guild));
        let mut change: Option<(Option<u64>, Change, i64)> = None;
        let mut done = false;
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.5);
        let mute_change = |on: bool| (Change::Mute(if on { Mute { on: true, until_ms: None } } else { Mute::default() }), if on { -1 } else { 0 });
        let out = widgets::modal(
            ui.ctx(),
            &t,
            Id::unique("notifications_du_serveur"),
            width,
            &xf::window_title(lang, &name),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.add_enabled_ui(!busy, |ui| {
                    let mut muted = settings.mute.active(now);
                    if switch_label(ui, &mut muted, xf::mute_guild(lang, &name)).changed() {
                        let (c, window) = mute_change(muted);
                        change = Some((None, c, window));
                    }
                    match settings.mute.until_ms.filter(|_| settings.mute.active(now)) {
                        Some(ms) => settings::note(ui, &t, &xf::muted_until(lang, time::until(lang, ms, h24))),
                        None => settings::note(ui, &t, xt::MUTE_NOTE.get(lang)),
                    }
                    settings::section(ui, &t, xt::SERVER_LEVEL.get(lang));
                    // Serveur sans réglage : « mentions », la valeur par défaut de Discord.
                    let level = if settings.level == Level::Inherit { Level::Mentions } else { settings.level };
                    for (l, text) in [(Level::All, st::LEVEL_ALL), (Level::Mentions, st::LEVEL_MENTIONS), (Level::Nothing, st::LEVEL_NOTHING)] {
                        if ui.radio(level == l, text.get(lang)).clicked() && level != l {
                            change = Some((None, Change::Level(l), 0));
                        }
                    }
                    ui.add_space(t.spacing.item_y);
                    let mut everyone = settings.suppress_everyone;
                    if switch_label(ui, &mut everyone, st::SUPPRESS_EVERYONE.get(lang)).changed() {
                        change = Some((None, Change::SuppressEveryone(everyone), 0));
                    }
                    let mut roles = settings.suppress_roles;
                    if switch_label(ui, &mut roles, st::SUPPRESS_ROLES.get(lang)).changed() {
                        change = Some((None, Change::SuppressRoles(roles), 0));
                    }
                    // Les trois autres interrupteurs de Discord (F12).
                    let mut highlights = settings.highlights_off;
                    if switch_label(ui, &mut highlights, ft::NOTIF_HIGHLIGHTS.get(lang)).changed() {
                        change = Some((None, Change::HighlightsOff(highlights), 0));
                    }
                    let mut events = settings.mute_events;
                    if switch_label(ui, &mut events, ft::NOTIF_EVENTS.get(lang)).changed() {
                        change = Some((None, Change::MuteEvents(events), 0));
                    }
                    let mut push = settings.mobile_push;
                    if switch_label(ui, &mut push, ft::NOTIF_MOBILE.get(lang)).changed() {
                        change = Some((None, Change::MobilePush(push), 0));
                    }

                    settings::section(ui, &t, xt::OVERRIDES.get(lang));
                    settings::note(ui, &t, xt::OVERRIDES_NOTE.get(lang));
                    widgets::field(ui, &t, &mut win.query, xt::ADD_HINT.get(lang));
                    let mut rows = settings.overrides(now);
                    rows.extend(win.added.iter().copied().filter(|a| !rows.contains(a)).collect::<Vec<_>>());
                    rows.retain(|id| chans.iter().any(|(c, _, _)| c == id));
                    let q = win.query.trim().to_lowercase();
                    if !q.is_empty() {
                        let hits: Vec<&(u64, String, bool)> = chans.iter().filter(|(id, n, _)| !rows.contains(id) && n.to_lowercase().contains(&q)).take(MAX_HITS).collect();
                        if hits.is_empty() {
                            ui.label(RichText::new(xt::NO_CHANNEL.get(lang)).color(t.colors.muted));
                        }
                        let mut picked = None;
                        for (id, n, category) in hits {
                            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), Sense::click());
                            // Le « # » à l'aplomb de ceux des lignes du dessous : c'est l'aplat de survol qui déborde.
                            if resp.hovered() {
                                ui.painter().rect_filled(rect.expand2(egui::vec2(t.spacing.item_x, 0.0)), t.r_row(), t.colors.row_hover);
                            }
                            super::keys::focus_ring(ui, &t, &resp);
                            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center)));
                            name_cell(&mut child, &t, n, *category);
                            if *category {
                                child.label(RichText::new(xt::CATEGORY.get(lang)).small().color(t.colors.faint));
                            }
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, n));
                            if resp.clicked() {
                                picked = Some(*id);
                            }
                        }
                        if let Some(id) = picked {
                            win.added.push(id);
                            win.query.clear();
                        }
                        ui.add_space(t.spacing.item_y);
                    }
                    if rows.is_empty() {
                        ui.label(RichText::new(xt::NO_OVERRIDE.get(lang)).color(t.colors.muted));
                    }
                    let mut removed = None;
                    for id in rows {
                        let Some((_, n, category)) = chans.iter().find(|(c, _, _)| *c == id) else { continue };
                        let ch = settings.channel(id);
                        ui.push_id(id, |ui| {
                            ui.horizontal(|ui| {
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if icon_button(ui, &t, Icon::Close, xt::REMOVE_OVERRIDE.get(lang)).clicked() {
                                        removed = Some(id);
                                    }
                                    let mut muted = ch.mute.active(now);
                                    if ui.checkbox(&mut muted, xt::COL_MUTE.get(lang)).changed() {
                                        let (c, window) = mute_change(muted);
                                        change = Some((Some(id), c, window));
                                    }
                                    // De droite à gauche : « Tout » se lit en premier.
                                    for (l, text) in [(Level::Nothing, xt::COL_NOTHING), (Level::Mentions, xt::COL_MENTIONS), (Level::All, xt::COL_ALL)] {
                                        let r = ui.selectable_label(ch.level == l, text.get(lang));
                                        super::keys::focus_ring(ui, &t, &r);
                                        if r.clicked() && ch.level != l {
                                            change = Some((Some(id), Change::Level(l), 0));
                                        }
                                    }
                                    let room = egui::vec2(ui.available_width(), ui.available_height());
                                    ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| name_cell(ui, &t, n, *category));
                                });
                            });
                        });
                    }
                    if let Some(id) = removed {
                        win.added.retain(|a| *a != id);
                        if settings.overrides(now).contains(&id) {
                            change = Some((Some(id), Change::Reset, 0));
                        }
                    }
                    if busy {
                        ui.label(RichText::new(st::SAVING.get(lang)).small().color(t.colors.faint));
                    }
                });
            },
            |ui| {
                if widgets::button(ui, &t, Kind::Primary, xt::DONE.get(lang)).clicked() {
                    done = true;
                }
            },
        );
        if let Some((channel, c, window)) = change
            && let Err(r) = self.store.account.change_notif(Some(win.guild), channel, c, window)
        {
            self.refused(r);
        }
        if !(out.closed || done) {
            self.nav.notif_win = Some(win);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, frame};
    use crate::state::usersettings::AccountCmd;
    use crate::state::usersettings::notif::{Change, Level};
    use crate::state::Cmd;

    #[test]
    fn window_lists_overrides_and_closes_when_the_guild_goes() {
        let mut a = app();
        assert!(!a.show_notif_settings(77, ""), "serveur inconnu");
        assert!(a.show_notif_settings(9, "hors"));
        frame(&mut a, Vec::new());
        frame(&mut a, Vec::new());
        assert!(a.nav.notif_win.is_some());
        // Une exception choisie part en une requête, sur le salon seul.
        let _ = a.store.take_commands();
        a.store.account.change_notif(Some(9), Some(11), Change::Level(Level::All), 0).unwrap_or_else(|e| panic!("{e:?}"));
        let sent = a.store.take_commands();
        assert!(sent.iter().any(|c| matches!(c, Cmd::Account(AccountCmd::PatchGuild { guild: Some(9), patch, .. }) if patch.channel == Some(11) && patch.change == Change::Level(Level::All))), "{sent:?}");
        assert_eq!(a.store.account.notifs.get(Some(9)).overrides(0), vec![11]);
        a.notif_settings_add(10);
        frame(&mut a, Vec::new());
        a.store.guilds.retain(|g| g.id != 9);
        frame(&mut a, Vec::new());
        assert!(a.nav.notif_win.is_none(), "serveur quitté : fenêtre fermée");
    }
}
