//! « Permissions » de « Modifier le salon » : dérogations par rôle et par
//! membre (ajouter, retirer, trois états par permission, groupées et
//! décrites comme Discord), synchronisation avec la catégorie, et
//! avertissement si l'on se retire l'accès. Le brouillon part des
//! dérogations du salon tel que la passerelle l'a donné.

use super::{unsaved_bar, App};
use crate::i18n::table::chadmin::{f, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::admin::channels::{set_tri, tri, ChanAction, Tri};
use crate::state::members::Row;
use crate::state::model::{ChannelKind, Overwrite, OverwriteKind};
use crate::state::Permissions as P;
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense, Stroke};

type Perm = (P, Text, Text);

const GENERAL: &[Perm] = &[
    (P::VIEW_CHANNEL, tx::P_VIEW_CHANNEL, tx::D_VIEW_CHANNEL),
    (P::MANAGE_CHANNELS, tx::P_MANAGE_CHANNELS, tx::D_MANAGE_CHANNELS),
    (P::MANAGE_ROLES, tx::P_MANAGE_ROLES, tx::D_MANAGE_ROLES),
    (P::MANAGE_WEBHOOKS, tx::P_MANAGE_WEBHOOKS, tx::D_MANAGE_WEBHOOKS),
];
const MEMBERSHIP: &[Perm] = &[(P::CREATE_INSTANT_INVITE, tx::P_CREATE_INSTANT_INVITE, tx::D_CREATE_INSTANT_INVITE)];
const TEXT: &[Perm] = &[
    (P::SEND_MESSAGES, tx::P_SEND_MESSAGES, tx::D_SEND_MESSAGES),
    (P::SEND_MESSAGES_IN_THREADS, tx::P_SEND_MESSAGES_IN_THREADS, tx::D_SEND_MESSAGES_IN_THREADS),
    (P::CREATE_PUBLIC_THREADS, tx::P_CREATE_PUBLIC_THREADS, tx::D_CREATE_PUBLIC_THREADS),
    (P::CREATE_PRIVATE_THREADS, tx::P_CREATE_PRIVATE_THREADS, tx::D_CREATE_PRIVATE_THREADS),
    (P::EMBED_LINKS, tx::P_EMBED_LINKS, tx::D_EMBED_LINKS),
    (P::ATTACH_FILES, tx::P_ATTACH_FILES, tx::D_ATTACH_FILES),
    (P::ADD_REACTIONS, tx::P_ADD_REACTIONS, tx::D_ADD_REACTIONS),
    (P::USE_EXTERNAL_EMOJIS, tx::P_USE_EXTERNAL_EMOJIS, tx::D_USE_EXTERNAL_EMOJIS),
    (P::USE_EXTERNAL_STICKERS, tx::P_USE_EXTERNAL_STICKERS, tx::D_USE_EXTERNAL_STICKERS),
    (P::MENTION_EVERYONE, tx::P_MENTION_EVERYONE, tx::D_MENTION_EVERYONE),
    (P::MANAGE_MESSAGES, tx::P_MANAGE_MESSAGES, tx::D_MANAGE_MESSAGES),
    (P::PIN_MESSAGES, tx::P_PIN_MESSAGES, tx::D_PIN_MESSAGES),
    (P::BYPASS_SLOWMODE, tx::P_BYPASS_SLOWMODE, tx::D_BYPASS_SLOWMODE),
    (P::MANAGE_THREADS, tx::P_MANAGE_THREADS, tx::D_MANAGE_THREADS),
    (P::READ_MESSAGE_HISTORY, tx::P_READ_MESSAGE_HISTORY, tx::D_READ_MESSAGE_HISTORY),
    (P::SEND_TTS_MESSAGES, tx::P_SEND_TTS_MESSAGES, tx::D_SEND_TTS_MESSAGES),
    (P::SEND_VOICE_MESSAGES, tx::P_SEND_VOICE_MESSAGES, tx::D_SEND_VOICE_MESSAGES),
    (P::SEND_POLLS, tx::P_SEND_POLLS, tx::D_SEND_POLLS),
];
const VOICE: &[Perm] = &[
    (P::CONNECT, tx::P_CONNECT, tx::D_CONNECT),
    (P::SPEAK, tx::P_SPEAK, tx::D_SPEAK),
    (P::STREAM, tx::P_STREAM, tx::D_STREAM),
    (P::USE_SOUNDBOARD, tx::P_USE_SOUNDBOARD, tx::D_USE_SOUNDBOARD),
    (P::USE_EXTERNAL_SOUNDS, tx::P_USE_EXTERNAL_SOUNDS, tx::D_USE_EXTERNAL_SOUNDS),
    (P::USE_VAD, tx::P_USE_VAD, tx::D_USE_VAD),
    (P::PRIORITY_SPEAKER, tx::P_PRIORITY_SPEAKER, tx::D_PRIORITY_SPEAKER),
    (P::MUTE_MEMBERS, tx::P_MUTE_MEMBERS, tx::D_MUTE_MEMBERS),
    (P::DEAFEN_MEMBERS, tx::P_DEAFEN_MEMBERS, tx::D_DEAFEN_MEMBERS),
    (P::MOVE_MEMBERS, tx::P_MOVE_MEMBERS, tx::D_MOVE_MEMBERS),
    (P::SET_VOICE_CHANNEL_STATUS, tx::P_SET_VOICE_CHANNEL_STATUS, tx::D_SET_VOICE_CHANNEL_STATUS),
];
const STAGE: &[Perm] = &[(P::REQUEST_TO_SPEAK, tx::P_REQUEST_TO_SPEAK, tx::D_REQUEST_TO_SPEAK)];
const EVENTS: &[Perm] = &[(P::CREATE_EVENTS, tx::P_CREATE_EVENTS, tx::D_CREATE_EVENTS), (P::MANAGE_EVENTS, tx::P_MANAGE_EVENTS, tx::D_MANAGE_EVENTS)];
const APPS: &[Perm] = &[
    (P::USE_APPLICATION_COMMANDS, tx::P_USE_APPLICATION_COMMANDS, tx::D_USE_APPLICATION_COMMANDS),
    (P::USE_EMBEDDED_ACTIVITIES, tx::P_USE_EMBEDDED_ACTIVITIES, tx::D_USE_EMBEDDED_ACTIVITIES),
    (P::USE_EXTERNAL_APPS, tx::P_USE_EXTERNAL_APPS, tx::D_USE_EXTERNAL_APPS),
];

/// Groupes affichés pour ce type de salon : une catégorie les montre tous
/// (ses dérogations valent pour des salons de tout type).
fn groups(kind: ChannelKind) -> Vec<(Text, &'static [Perm])> {
    let mut g = vec![(tx::GROUP_GENERAL, GENERAL), (tx::GROUP_MEMBERSHIP, MEMBERSHIP), (tx::GROUP_TEXT, TEXT)];
    if kind == ChannelKind::Category {
        g.extend([(tx::GROUP_VOICE, VOICE), (tx::GROUP_STAGE, STAGE), (tx::GROUP_EVENTS, EVENTS)]);
    }
    g.push((tx::GROUP_APPS, APPS));
    g
}

/// Clé d'une dérogation.
type Target = (OverwriteKind, u64);

fn sorted(v: &[Overwrite]) -> Vec<(bool, u64, u64, u64)> {
    let mut s: Vec<_> = v.iter().map(|o| (o.kind == OverwriteKind::Member, o.id, o.allow.0, o.deny.0)).collect();
    s.sort_unstable();
    s
}

pub(super) struct Draft {
    base: Vec<Overwrite>,
    pub(super) list: Vec<Overwrite>,
    pub(super) selected: Target,
    adding: bool,
    filter: String,
    /// Avertissement de perte d'accès montré : le clic suivant enregistre.
    confirm_loss: bool,
}

impl Draft {
    fn new(base: &[Overwrite], guild: u64) -> Self {
        Draft { base: base.to_vec(), list: base.to_vec(), selected: (OverwriteKind::Role, guild), adding: false, filter: String::new(), confirm_loss: false }
    }

    pub(super) fn changed(&self) -> bool {
        sorted(&self.base) != sorted(&self.list)
    }

    fn state(&self, p: P) -> Tri {
        self.list.iter().find(|o| (o.kind, o.id) == self.selected).map_or(Tri::Neutral, |o| tri(o, p))
    }

    /// Change une permission de la cible choisie (dérogation créée au besoin).
    pub(super) fn set(&mut self, p: P, t: Tri) {
        let (kind, id) = self.selected;
        if !self.list.iter().any(|o| o.kind == kind && o.id == id) {
            self.list.push(Overwrite { id, kind, allow: P::NONE, deny: P::NONE });
        }
        if let Some(o) = self.list.iter_mut().find(|o| o.kind == kind && o.id == id) {
            set_tri(o, p, t);
        }
        self.confirm_loss = false;
    }

    fn add(&mut self, target: Target) {
        if !self.list.iter().any(|o| (o.kind, o.id) == target) {
            self.list.push(Overwrite { id: target.1, kind: target.0, allow: P::NONE, deny: P::NONE });
        }
        self.selected = target;
        self.adding = false;
        self.filter.clear();
        self.confirm_loss = false;
    }

    fn remove(&mut self, target: Target, guild: u64) {
        self.list.retain(|o| (o.kind, o.id) != target);
        self.selected = (OverwriteKind::Role, guild);
        self.confirm_loss = false;
    }
}

/// Trois boutons « refuser / neutre / autoriser » ; le choix fait, s'il y en a un.
fn tri_buttons(ui: &mut egui::Ui, t: &Tokens, lang: Lang, name: &str, cur: Tri) -> Option<Tri> {
    let side = ui.spacing().interact_size.y;
    let mut out = None;
    for (state, label, fill) in [(Tri::Deny, tx::DENY, t.colors.danger), (Tri::Neutral, tx::NEUTRAL, t.colors.selected), (Tri::Allow, tx::ALLOW, t.colors.online)] {
        let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
        let on = cur == state;
        let p = ui.painter();
        p.rect_filled(rect, t.r_row(), if on { fill } else if resp.hovered() { t.colors.row_hover } else { t.colors.control_bg });
        // Le neutre choisi garde un fond visible (celui d'un bouton enfoncé), pas celui de la page.
        let color = if on && state == Tri::Neutral { t.colors.text_strong } else if on { t.colors.on_accent } else { t.colors.muted };
        let s = Stroke::new(t.shapes.unread_ring, color);
        let r = rect.shrink(side * 0.3);
        match state {
            Tri::Deny => {
                p.line_segment([r.left_top(), r.right_bottom()], s);
                p.line_segment([r.right_top(), r.left_bottom()], s);
            }
            Tri::Neutral => {
                p.line_segment([r.left_bottom(), r.right_top()], s);
            }
            Tri::Allow => {
                let mid = egui::pos2(r.left() + r.width() * 0.4, r.bottom());
                p.line_segment([egui::pos2(r.left(), r.center().y), mid], s);
                p.line_segment([mid, r.right_top()], s);
            }
        }
        let label = format!("{} — {name}", label.get(lang));
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, &label));
        if resp.on_hover_text(label.clone()).clicked() && !on {
            out = Some(state);
        }
    }
    out
}

impl App {
    /// Nom affiché d'une cible de dérogation.
    fn target_name(&self, guild: u64, (kind, id): Target) -> String {
        match kind {
            OverwriteKind::Role if id == guild => tx::EVERYONE.get(self.lang).to_string(),
            OverwriteKind::Role => self.store.guilds.iter().find(|g| g.id == guild).and_then(|g| g.roles.iter().find(|r| r.id == id)).map(|r| r.name.clone()).unwrap_or_else(|| id.to_string()),
            OverwriteKind::Member => {
                let people = self.store.people.lock();
                people.member(guild, id).and_then(|m| m.nick.clone()).or_else(|| people.user(id).map(|u| u.display_name().to_string())).unwrap_or_else(|| id.to_string())
            }
        }
    }

    /// Membres connus de ce serveur (liste des membres reçue), pour « Ajouter ».
    fn member_candidates(&self, guild: u64) -> Vec<(u64, String)> {
        let Some(list) = self.store.channel.and_then(|c| self.store.members.list_for(guild, c)) else { return Vec::new() };
        (0..list.len())
            .filter_map(|i| match list.row(i) {
                Row::Member(m) => Some((m.user_id, m.name.clone())),
                _ => None,
            })
            .take(500)
            .collect()
    }

    pub(super) fn channel_permissions(&mut self, ui: &mut egui::Ui, t: &Tokens, channel: u64) {
        let lang = self.lang;
        let Some((guild, c)) = self.store.guilds.iter().find_map(|g| g.channels.iter().find(|c| c.id == channel).map(|c| (g.id, c.clone()))) else { return };
        let Some(base) = c.overwrites.clone() else {
            super::super::settings::note(ui, t, tx::OVERWRITES_UNKNOWN.get(lang));
            return;
        };
        if !self.store.can_edit_overwrites(channel) {
            super::super::settings::note(ui, t, tx::NO_MANAGE_ROLES.get(lang));
            return;
        }
        let busy = self.store.chan_admin.pending(ChanAction::Save);
        // Synchronisation avec la catégorie (d'après le salon tel qu'il est).
        let mut sync = false;
        if let (Some(synced), Some(cat)) = (self.store.synced_with_category(channel), c.parent.and_then(|p| self.store.channel_name(p).map(str::to_string))) {
            egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.card_margin()).show(ui, |ui| {
                ui.set_min_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    if synced {
                        ui.label(f::synced_with(lang, &cat));
                    } else {
                        ui.label(f::not_synced(lang, &cat));
                        sync = ui.add_enabled(!busy, egui::Button::new(tx::SYNC_NOW.get(lang))).clicked();
                    }
                });
            });
        }
        if sync {
            if let Some(e) = self.chan_ui.edit.as_mut() {
                e.perms = None;
            }
            self.store.sync_with_category(channel);
            return;
        }

        let Some(e) = self.chan_ui.edit.as_mut() else { return };
        // Dérogations changées par CHANNEL_UPDATE : brouillon refait s'il n'a pas été touché.
        if e.perms.as_ref().is_none_or(|d| !d.changed() && sorted(&d.base) != sorted(&base)) {
            e.perms = Some(Draft::new(&base, guild));
        }
        let Some(list) = e.perms.as_ref().map(|d| d.list.clone()) else { return };
        let adding = e.perms.as_ref().is_some_and(|d| d.adding);
        let filter = e.perms.as_ref().map(|d| d.filter.to_lowercase()).unwrap_or_default();
        let selected = e.perms.as_ref().map_or((OverwriteKind::Role, guild), |d| d.selected);

        // Cibles affichées : @everyone, les rôles (du plus haut au plus bas), puis les membres.
        let roles: Vec<(u64, String, i32)> =
            self.store.guilds.iter().find(|g| g.id == guild).map(|g| g.roles.iter().map(|r| (r.id, r.name.clone(), r.position)).collect()).unwrap_or_default();
        let pos = |id: u64| roles.iter().find(|r| r.0 == id).map_or(0, |r| r.2);
        let mut targets: Vec<Target> = vec![(OverwriteKind::Role, guild)];
        let mut role_ows: Vec<u64> = list.iter().filter(|o| o.kind == OverwriteKind::Role && o.id != guild).map(|o| o.id).collect();
        role_ows.sort_by_key(|id| std::cmp::Reverse(pos(*id)));
        targets.extend(role_ows.into_iter().map(|id| (OverwriteKind::Role, id)));
        targets.extend(list.iter().filter(|o| o.kind == OverwriteKind::Member).map(|o| (OverwriteKind::Member, o.id)));
        let names: Vec<String> = targets.iter().map(|tg| self.target_name(guild, *tg)).collect();
        let candidates: Vec<(Target, String)> = if adding {
            let mut v: Vec<(Target, String, i32)> = roles
                .iter()
                .filter(|r| r.0 != guild && !targets.contains(&(OverwriteKind::Role, r.0)))
                .map(|r| ((OverwriteKind::Role, r.0), r.1.clone(), r.2))
                .collect();
            v.sort_by_key(|x| std::cmp::Reverse(x.2));
            let mut out: Vec<(Target, String)> = v.into_iter().map(|(a, b, _)| (a, b)).collect();
            out.extend(self.member_candidates(guild).into_iter().filter(|m| !targets.contains(&(OverwriteKind::Member, m.0))).map(|(id, n)| ((OverwriteKind::Member, id), n)));
            out.into_iter().filter(|(_, n)| filter.is_empty() || n.to_lowercase().contains(&filter)).take(100).collect()
        } else {
            Vec::new()
        };
        let selected_name = self.target_name(guild, selected);

        let Some(d) = self.chan_ui.edit.as_mut().and_then(|e| e.perms.as_mut()) else { return };
        let wide = ui.available_width() >= 2.0 * crate::theme::SETTINGS_WINDOW.nav_width;
        let body = |ui: &mut egui::Ui, d: &mut Draft| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(tx::ROLES_MEMBERS.get(lang).to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                if super::super::util::icon_button(ui, t, crate::theme::icons::Icon::Plus, tx::ADD_ROLE_MEMBER.get(lang)).clicked() {
                    d.adding = !d.adding;
                }
            });
            if d.adding {
                ui.add(egui::TextEdit::singleline(&mut d.filter).hint_text(tx::SEARCH_HINT.get(lang)).desired_width(f32::INFINITY));
                if candidates.is_empty() {
                    ui.label(RichText::new(tx::NO_MATCH.get(lang)).small().color(t.colors.muted));
                }
                let mut pick = None;
                for (tg, name) in &candidates {
                    let shown = if tg.0 == OverwriteKind::Member { format!("@{name}") } else { name.clone() };
                    if ui.selectable_label(false, shown).clicked() {
                        pick = Some(*tg);
                    }
                }
                super::super::settings::note(ui, t, tx::MEMBERS_NOTE.get(lang));
                if let Some(tg) = pick {
                    d.add(tg);
                }
                ui.separator();
            }
            for (tg, name) in targets.iter().zip(&names) {
                let shown = if tg.0 == OverwriteKind::Member { format!("@{name}") } else { name.clone() };
                if ui.selectable_label(d.selected == *tg, shown).clicked() {
                    d.selected = *tg;
                }
            }
        };
        let perms_ui = |ui: &mut egui::Ui, d: &mut Draft| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(&selected_name).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                if selected != (OverwriteKind::Role, guild) && ui.button(tx::REMOVE_OVERWRITE.get(lang)).clicked() {
                    d.remove(selected, guild);
                }
            });
            // Largeur fixée une fois : sinon chaque ligne élargit la suivante et les boutons dérivent.
            let col = ui.available_width();
            // Trois boutons et leurs trois espaces : le dernier finit au bord droit de la page.
            let buttons = 3.0 * (ui.spacing().interact_size.y + ui.spacing().item_spacing.x);
            let w = (col - buttons).max(0.0);
            for (group, perms) in groups(c.kind) {
                super::super::settings::section(ui, t, group.get(lang));
                for (p, name, desc) in perms {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.set_width(w);
                            ui.label(RichText::new(name.get(lang)).color(t.colors.text_strong));
                            ui.add(egui::Label::new(RichText::new(desc.get(lang)).small().color(t.colors.faint)).wrap());
                        });
                        if let Some(s) = tri_buttons(ui, t, lang, name.get(lang), d.state(*p)) {
                            d.set(*p, s);
                        }
                    });
                    ui.add_space(t.spacing.item_y);
                }
            }
        };
        if wide {
            ui.horizontal_top(|ui| {
                let left = (ui.available_width() * 0.3).min(crate::theme::SETTINGS_WINDOW.nav_width);
                ui.allocate_ui_with_layout(egui::vec2(left, 0.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                    ui.set_width(left);
                    body(ui, d);
                });
                ui.separator();
                ui.vertical(|ui| perms_ui(ui, d));
            });
        } else {
            body(ui, d);
            ui.separator();
            perms_ui(ui, d);
        }
    }

    /// Bandeau « modifications non enregistrées » de la page, hors du
    /// défilement ; avertit avant que je ne me retire l'accès.
    pub(super) fn permissions_bar(&mut self, ui: &mut egui::Ui, t: &Tokens, channel: u64) {
        let lang = self.lang;
        let busy = self.store.chan_admin.pending(ChanAction::Save);
        let Some((guild, base)) = self.store.guilds.iter().find_map(|g| g.channels.iter().find(|c| c.id == channel).map(|c| (g.id, c.overwrites.clone()))) else { return };
        let Some(base) = base else { return };
        let Some(d) = self.chan_ui.edit.as_ref().and_then(|e| e.perms.as_ref()) else { return };
        if !d.changed() {
            return;
        }
        let new_list = d.list.clone();
        let confirm = d.confirm_loss;
        let loses = self.store.would_lose_access(channel, &new_list);
        if loses && confirm {
            ui.label(RichText::new(tx::LOSE_ACCESS.get(lang)).color(t.colors.danger));
        }
        let label = if loses && confirm { tx::SAVE_ANYWAY } else { tx::SAVE };
        let (reset, save) = unsaved_bar(ui, t, lang, busy, label.get(lang));
        let Some(d) = self.chan_ui.edit.as_mut().and_then(|e| e.perms.as_mut()) else { return };
        if reset {
            *d = Draft::new(&base, guild);
        } else if save && loses && !confirm {
            d.confirm_loss = true;
        } else if save {
            self.store.save_channel(channel, crate::state::admin::channels::Patch { overwrites: Some(new_list), ..Default::default() });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{app, sent, Online, G, ME, R_MOD, TEXT as CHAN};
    use super::*;
    use crate::state::admin::channels::ChanCmd;

    #[test]
    fn every_named_flag_is_shown_once_for_a_category() {
        let mut seen = Vec::new();
        for (_, perms) in groups(ChannelKind::Category) {
            seen.extend(perms.iter().map(|p| p.0));
        }
        let mut dedup = seen.clone();
        dedup.sort_by_key(|p| p.0);
        dedup.dedup();
        assert_eq!(dedup.len(), seen.len(), "aucune en double");
        // Toutes sauf celles qui n'existent qu'au niveau du serveur.
        let server_only = [P::KICK_MEMBERS, P::BAN_MEMBERS, P::ADMINISTRATOR, P::MANAGE_GUILD, P::VIEW_AUDIT_LOG, P::VIEW_GUILD_INSIGHTS, P::CHANGE_NICKNAME, P::MANAGE_NICKNAMES, P::MANAGE_GUILD_EXPRESSIONS, P::MODERATE_MEMBERS, P::VIEW_CREATOR_MONETIZATION_ANALYTICS, P::CREATE_GUILD_EXPRESSIONS];
        for (name, p) in P::NAMED {
            assert!(seen.contains(p) || server_only.contains(p), "{name} absente");
        }
        assert!(groups(ChannelKind::Text).iter().all(|(g, _)| *g != tx::GROUP_VOICE));
    }

    #[test]
    fn add_role_deny_view_and_warning_before_saving() {
        let net = Online::default();
        let mut a = app(net.clone());
        // Je ne suis plus propriétaire : je peux me retirer l'accès.
        if let Some(g) = a.store.guilds.iter_mut().find(|g| g.id == G) {
            g.owner_id = Some(999);
            if let Some(r) = g.roles.iter_mut().find(|r| r.id == G) {
                r.permissions = P::VIEW_CHANNEL | P::MANAGE_CHANNELS | P::MANAGE_ROLES;
            }
        }
        let ev = crate::state::Event::RoleUpsert { guild_id: G, role: a.store.guilds[0].roles[0].clone() };
        a.store.apply(ev);
        assert!(a.show_channel_settings(CHAN, "permissions"));
        sent(&net);
        super::super::tests::shown(&mut a);
        let Some(d) = a.chan_ui.edit.as_mut().and_then(|e| e.perms.as_mut()) else { panic!("brouillon attendu") };
        d.add((OverwriteKind::Role, R_MOD));
        d.set(P::SEND_MESSAGES, Tri::Allow);
        d.selected = (OverwriteKind::Role, G);
        d.set(P::VIEW_CHANNEL, Tri::Deny);
        // Premier clic : avertissement, rien ne part.
        super::super::tests::click_exact(&mut a, tx::SAVE.get(Lang::Fr));
        assert!(sent(&net).is_empty());
        super::super::tests::click_exact(&mut a, tx::SAVE_ANYWAY.get(Lang::Fr));
        let cmds = sent(&net);
        let [ChanCmd::Modify { patch, .. }] = cmds.as_slice() else { panic!("{cmds:?}") };
        let ows = patch.overwrites.clone().unwrap_or_default();
        assert!(ows.iter().any(|o| o.id == R_MOD && o.allow == P::SEND_MESSAGES));
        assert!(ows.iter().any(|o| o.id == G && o.deny == P::VIEW_CHANNEL));
        assert!(!ows.iter().any(|o| o.id == ME), "rien n'est ajouté à mon nom");
    }
}
