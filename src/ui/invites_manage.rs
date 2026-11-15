//! Invitations (lot A1) : « Inviter des personnes » (salon, durée, nombre
//! d'utilisations, adhésion temporaire, lien à copier) et la liste des
//! invitations d'un serveur, avec « Révoquer » (permission « Gérer le
//! serveur »). Rien ne part sans un clic.

use super::guild_create::failure_text;
use super::App;
use crate::i18n::table::ff::t as ft;
use crate::i18n::table::guilds::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::servers::{InviteList, ServerAction};
use eframe::egui::{self, Id, RichText};

/// Durées proposées (secondes ; 0 : jamais), comme l'application officielle.
const AGES: [u32; 7] = [1800, 3600, 21_600, 43_200, 86_400, 604_800, 0];
/// Nombres d'utilisations proposés (0 : sans limite).
const USES: [u32; 7] = [0, 1, 5, 10, 25, 50, 100];
/// Choix par défaut : 7 jours, sans limite.
const DEFAULT_AGE: usize = 5;

pub(super) struct Draft {
    guild: u64,
    channel: u64,
    age: usize,
    uses: usize,
    temporary: bool,
    /// Recherche dans la liste d'amis (F14).
    query: String,
    /// Amis à qui le lien a été envoyé depuis l'ouverture de la fenêtre.
    sent: Vec<u64>,
    /// Ami choisi avant que le lien existe : il part dès sa création.
    waiting: Option<(u64, String)>,
}

/// Amis montrés au plus dans « Inviter des amis » (la recherche affine).
const MAX_FRIENDS: usize = 50;

#[derive(Default)]
pub(super) struct InvitesUi {
    draft: Option<Draft>,
    /// Liste des invitations ouverte pour ce serveur.
    manage: Option<u64>,
    /// « Révoquer » cliqué une fois : code à confirmer.
    confirm: Option<String>,
}

impl InvitesUi {
    /// Une des deux fenêtres est ouverte (elle prend Échap).
    pub(super) fn is_open(&self) -> bool {
        self.draft.is_some() || self.manage.is_some()
    }
}

/// Durée en mots : la plus grande unité entière (minutes, heures, jours).
pub(super) fn duration(lang: Lang, secs: u64) -> String {
    match secs {
        0 => tx::NEVER.get(lang).to_string(),
        s if s >= 86_400 && s % 86_400 == 0 => p::days(lang, s / 86_400),
        s if s >= 3600 && s % 3600 == 0 => p::hours(lang, s / 3600),
        s => p::minutes(lang, s.div_ceil(60)),
    }
}

/// Temps restant avant `end_ms`, arrondi à l'unité la plus grande.
fn remaining(lang: Lang, end_ms: i64, now_ms: i64) -> String {
    let s = u64::try_from((end_ms - now_ms).max(0) / 1000).unwrap_or(0);
    if s >= 86_400 {
        p::days(lang, s / 86_400)
    } else if s >= 3600 {
        p::hours(lang, s / 3600)
    } else {
        p::minutes(lang, s.div_ceil(60).max(1))
    }
}

fn uses_label(lang: Lang, n: u32) -> String {
    if n == 0 { tx::NO_LIMIT.get(lang).to_string() } else { p::uses(lang, u64::from(n)) }
}

impl App {
    /// « Inviter des personnes » sur ce serveur.
    pub(super) fn open_invite(&mut self, guild: u64) {
        let Some(channel) = self.store.invite_channels(guild).first().map(|c| c.0) else {
            self.notify(tx::NO_INVITE_CHANNEL);
            return;
        };
        self.store.servers.created = None;
        self.servers.invites.draft = Some(Draft { guild, channel, age: DEFAULT_AGE, uses: 0, temporary: false, query: String::new(), sent: Vec::new(), waiting: None });
    }

    /// « Inviter des personnes » vers ce salon (page Invitations de « Modifier le salon », lot A2).
    pub(super) fn open_invite_in(&mut self, guild: u64, channel: u64) {
        self.open_invite(guild);
        if let Some(d) = self.servers.invites.draft.as_mut()
            && self.store.invite_channels(guild).iter().any(|c| c.0 == channel)
        {
            d.channel = channel;
        }
    }

    /// Liste des invitations d'un serveur (chargée à l'ouverture).
    pub(super) fn open_invites(&mut self, guild: u64) {
        self.servers.invites.manage = Some(guild);
        self.servers.invites.confirm = None;
        self.store.load_invites(guild);
    }

    /// Ouvre la fenêtre des invitations ou celle de la liste (aperçu) ;
    /// `false` si ce n'est pas possible pour ce serveur.
    pub fn show_invites(&mut self, guild: u64, list: bool) -> bool {
        if list {
            self.open_invites(guild);
        } else {
            self.open_invite(guild);
        }
        self.servers.invites.draft.is_some() || self.servers.invites.manage.is_some()
    }

    /// « Générer un lien » dans la fenêtre ouverte (aperçu, tests).
    pub fn generate_invite(&mut self) -> bool {
        let Some(d) = self.servers.invites.draft.as_ref() else { return false };
        let (c, a, u, tmp) = (d.channel, AGES[d.age], USES[d.uses], d.temporary);
        let ok = self.store.create_invite(c, a, u, tmp);
        self.flush_commands();
        ok
    }

    pub(super) fn invite_dialogs(&mut self, ui: &mut egui::Ui) {
        self.invite_dialog(ui);
        self.manage_dialog(ui);
    }

    fn invite_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(guild) = self.servers.invites.draft.as_ref().map(|d| d.guild) else { return };
        let Some(gname) = self.store.guilds.iter().find(|g| g.id == guild).map(|g| super::words::guild_name(self.lang, &g.name).to_string()) else {
            self.servers.invites.draft = None;
            return;
        };
        let channels = self.store.invite_channels(guild);
        let t = self.theme.clone();
        let lang = self.lang;
        let busy = self.store.servers.pending(ServerAction::Invite);
        let created = self.store.servers.created.clone();
        let (mut generate, mut close, mut copy) = (false, false, None);
        // Amis (F14) : nom affiché, triés ; un bouton « Inviter » par ami.
        let mut friends: Vec<(u64, String)> = self.store.relations.friends().map(|r| (r.id, self.store.relations.display_name(r.id).to_string())).filter(|(_, n)| !n.is_empty()).collect();
        friends.sort_by_cached_key(|(id, n)| (n.to_lowercase(), *id));
        let sending = self.invite_sending();
        let mut invite: Option<(u64, String)> = None;
        let Some(d) = self.servers.invites.draft.as_mut() else { return };
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = super::widgets::modal(ui.ctx(), &t, Id::unique("inviter"), width, &f::invite_to(lang, &gname), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            {
                crate::ui::settings::shell::page_style(ui, &t);
                let current = channels.iter().find(|c| c.0 == d.channel).map(|c| c.1.clone()).unwrap_or_default();
                // Trois listes de même largeur (chacune suivait son texte).
                let combo = (width * 0.45).round();
                egui::Grid::new("invitation-reglages").num_columns(2).show(ui, |ui| {
                    ui.label(tx::CHANNEL.get(lang));
                    egui::ComboBox::from_id_salt("invitation-salon").width(combo).selected_text(format!("#{current}")).show_ui(ui, |ui| {
                        for (id, name) in &channels {
                            ui.selectable_value(&mut d.channel, *id, format!("#{name}"));
                        }
                    });
                    ui.end_row();
                    ui.label(tx::EXPIRE_AFTER.get(lang));
                    egui::ComboBox::from_id_salt("invitation-duree").width(combo).selected_text(duration(lang, u64::from(AGES[d.age]))).show_ui(ui, |ui| {
                        for (i, a) in AGES.iter().enumerate() {
                            ui.selectable_value(&mut d.age, i, duration(lang, u64::from(*a)));
                        }
                    });
                    ui.end_row();
                    ui.label(tx::MAX_USES.get(lang));
                    egui::ComboBox::from_id_salt("invitation-utilisations").width(combo).selected_text(uses_label(lang, USES[d.uses])).show_ui(ui, |ui| {
                        for (i, u) in USES.iter().enumerate() {
                            ui.selectable_value(&mut d.uses, i, uses_label(lang, *u));
                        }
                    });
                    ui.end_row();
                });
                ui.checkbox(&mut d.temporary, tx::TEMPORARY.get(lang)).on_hover_text(tx::TEMPORARY_TIP.get(lang));
                if let Some(inv) = &created {
                    let url = inv.url();
                    let mut shown = url.clone();
                    // Le bouton d'abord (à droite) : le champ prend la place qui reste.
                    super::widgets::field_row(ui, &t, |ui| {
                        if ui.add(crate::ui::form::primary(&t, tx::COPY.get(lang))).clicked() {
                            copy = Some(url);
                        }
                        super::widgets::field(ui, &t, &mut shown, "");
                    });
                }
                if !friends.is_empty() {
                    ui.add_space(t.spacing.item_y);
                    super::widgets::field_label(ui, &t, ft::INVITE_FRIENDS.get(lang));
                    super::widgets::field(ui, &t, &mut d.query, ft::INVITE_FRIENDS_HINT.get(lang));
                    let q = d.query.trim().to_lowercase();
                    let shown: Vec<&(u64, String)> = friends.iter().filter(|(_, n)| q.is_empty() || n.to_lowercase().contains(&q)).take(MAX_FRIENDS).collect();
                    if shown.is_empty() {
                        ui.label(RichText::new(ft::INVITE_NO_FRIEND.get(lang)).color(t.colors.muted));
                    }
                    egui::ScrollArea::vertical().id_salt("inviter-amis").max_height(ui.ctx().content_rect().height() * 0.3).show(ui, |ui| {
                        for (id, name) in shown {
                            ui.push_id(*id, |ui| {
                                ui.horizontal(|ui| {
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if d.sent.contains(id) {
                                            ui.label(RichText::new(ft::INVITE_SENT.get(lang)).color(t.colors.muted));
                                        } else if d.waiting.as_ref().is_some_and(|w| w.0 == *id) {
                                            ui.spinner();
                                        } else if ui.add_enabled_ui(!sending && !busy && d.waiting.is_none(), |ui| super::widgets::compact(ui, ft::INVITE_ONE.get(lang))).inner.clicked() {
                                            invite = Some((*id, name.clone()));
                                        }
                                        let room = egui::vec2(ui.available_width(), ui.available_height());
                                        ui.allocate_ui_with_layout(room, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                            ui.add(egui::Label::new(RichText::new(name).color(t.colors.text_strong)).truncate().selectable(false));
                                        });
                                    });
                                });
                            });
                        }
                    });
                }
            }
        }, |ui| {
            generate = ui.add_enabled(!busy && !channels.is_empty(), crate::ui::form::primary(&t, tx::GENERATE.get(lang))).clicked();
            close = ui.add(crate::ui::form::link(&t, tx::CLOSE.get(lang))).clicked();
            if busy {
                ui.spinner();
            }
        });
        if let Some(url) = copy {
            ui.ctx().copy_text(url);
            self.notify(tx::COPIED);
        }
        // Ami choisi (F14) : le lien montré part tout de suite ; sans lien,
        // il est d'abord créé avec les réglages de la fenêtre.
        if let Some(friend) = invite
            && let Some(d) = self.servers.invites.draft.as_mut()
        {
            d.waiting = Some(friend);
            generate |= created.is_none();
        }
        if let Some(inv) = created.as_ref().filter(|i| i.guild_id == Some(guild))
            && !sending
            && let Some((user, name)) = self.servers.invites.draft.as_mut().and_then(|d| d.waiting.take())
        {
            self.send_invite_link(guild, user, &name, inv.url());
            if let Some(d) = self.servers.invites.draft.as_mut() {
                d.sent.push(user);
            }
        }
        if generate && let Some(d) = self.servers.invites.draft.as_ref() {
            let (c, a, u, tmp) = (d.channel, AGES[d.age], USES[d.uses], d.temporary);
            if !self.store.create_invite(c, a, u, tmp)
                && let Some(d) = self.servers.invites.draft.as_mut()
            {
                d.waiting = None;
            }
        } else if !busy
            && created.is_none()
            && let Some(d) = self.servers.invites.draft.as_mut()
        {
            // Création refusée : l'ami choisi n'attend plus.
            d.waiting = None;
        }
        if close || modal.closed {
            self.servers.invites.draft = None;
        }
    }

    fn manage_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(guild) = self.servers.invites.manage else { return };
        let Some(gname) = self.store.guilds.iter().find(|g| g.id == guild).map(|g| super::words::guild_name(self.lang, &g.name).to_string()) else {
            self.servers.invites.manage = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = self.h24();
        let now = self.store.now();
        let list = self.store.servers.invites.get(&guild).cloned().unwrap_or_default();
        let loading = self.store.servers.pending(ServerAction::Invites);
        let (mut refresh, mut close, mut revoke, mut ask) = (false, false, None, None);
        let confirm = self.servers.invites.confirm.clone();
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.6);
        let modal = super::widgets::modal(ui.ctx(), &t, Id::unique("invitations"), width, &f::invites_of(lang, &gname), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            {
                crate::ui::settings::shell::page_style(ui, &t);
                match &list {
                    InviteList::Loading => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label(tx::LOADING.get(lang));
                        });
                    }
                    InviteList::Failed(fl) => {
                        ui.label(RichText::new(failure_text(lang, fl, h24)).color(t.colors.danger));
                    }
                    InviteList::Ready(v) if v.is_empty() => {
                        ui.label(RichText::new(tx::NO_INVITES.get(lang)).color(t.colors.muted));
                    }
                    InviteList::Ready(v) => {
                        let row_h = super::widgets::button_height(&t);
                        egui::ScrollArea::vertical().max_height(row_h * 12.0).show_rows(ui, row_h, v.len() + 1, |ui, range| {
                            // Colonnes étalées sur toute la largeur de la fenêtre.
                            let col = ((ui.available_width() - 5.0 * ui.spacing().item_spacing.x) / 6.0).floor().max(0.0);
                            egui::Grid::new("liste-invitations").num_columns(6).min_col_width(col).striped(true).show(ui, |ui| {
                                for i in range {
                                    if i == 0 {
                                        for h in [tx::INVITER, tx::CODE, tx::CHANNEL, tx::USES, tx::EXPIRES] {
                                            ui.label(RichText::new(h.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                                        }
                                        ui.label("");
                                        ui.end_row();
                                        continue;
                                    }
                                    let Some(inv) = v.get(i - 1) else { continue };
                                    ui.label(inv.inviter.clone().unwrap_or_default());
                                    let code = if inv.temporary { format!("{} ({})", inv.code, tx::TEMPORARY_SHORT.get(lang)) } else { inv.code.clone() };
                                    ui.label(RichText::new(code).monospace());
                                    ui.label(format!("#{}", inv.channel_name));
                                    let uses = if inv.max_uses == 0 { inv.uses.to_string() } else { f::uses_of(lang, inv.uses, inv.max_uses) };
                                    ui.label(uses);
                                    ui.label(inv.expires_at_ms.map_or_else(|| tx::NEVER.get(lang).to_string(), |e| remaining(lang, e, now)));
                                    let busy = self.store.servers.revoking(&inv.code);
                                    let confirming = confirm.as_deref() == Some(inv.code.as_str());
                                    let label = if confirming { tx::CONFIRM_REVOKE } else { tx::REVOKE };
                                    let b = crate::ui::form::danger(&t, label.get(lang));
                                    if ui.add_enabled(!busy, b).clicked() {
                                        if confirming {
                                            revoke = Some(inv.code.clone());
                                        } else {
                                            ask = Some(inv.code.clone());
                                        }
                                    }
                                    ui.end_row();
                                }
                            });
                        });
                    }
                }
            }
        }, |ui| {
            close = ui.add(crate::ui::form::btn(&t, tx::CLOSE.get(lang))).clicked();
            refresh = ui.add_enabled(!loading, crate::ui::form::link(&t, tx::REFRESH.get(lang))).clicked();
        });
        if let Some(code) = ask {
            self.servers.invites.confirm = Some(code);
        }
        if let Some(code) = revoke {
            self.servers.invites.confirm = None;
            self.store.revoke_invite(guild, &code);
        }
        if refresh {
            self.store.load_invites(guild);
        }
        if close || modal.closed {
            self.servers.invites.manage = None;
            self.servers.invites.confirm = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_in_words() {
        assert_eq!(duration(Lang::Fr, 0), "Jamais");
        assert_eq!(duration(Lang::Fr, 1800), "30 minutes");
        assert_eq!(duration(Lang::Fr, 3600), "1 heure");
        assert_eq!(duration(Lang::En, 21_600), "6 hours");
        assert_eq!(duration(Lang::Fr, 604_800), "7 jours");
        assert_eq!(remaining(Lang::En, 90_000_000, 0), "1 day");
        assert_eq!(remaining(Lang::Fr, 30_000, 0), "1 minute");
        assert_eq!(remaining(Lang::Fr, 0, 10), "1 minute");
        assert_eq!(uses_label(Lang::En, 0), "No limit");
        assert_eq!(uses_label(Lang::Fr, 5), "5 utilisations");
    }
}
