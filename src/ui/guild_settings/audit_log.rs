//! Page « Journal d'audit » (lot A4) : entrées virtualisées, filtres par
//! auteur et par type d'action, pages plus anciennes à la demande. Chaque
//! entrée est une phrase ; celle choisie montre ses changements avant/après.

use crate::i18n::table::moderation::{f, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::admin::audit::{self, Entry, Filter, Kind, Verb};
use crate::state::admin::moderation::{snowflake_ms, AuditView, Load, ModAction};
use crate::ui::App;
use eframe::egui::{self, RichText, Sense};

/// Auteurs retenus pour le filtre, au plus (les premiers vus restent).
const AUTHORS_MAX: usize = 100;

#[derive(Default)]
pub(in crate::ui) struct AuditUi {
    filter: Filter,
    /// Entrée dont on montre les changements.
    selected: Option<u64>,
    /// Auteurs vus dans les pages chargées, pour le filtre.
    authors: Vec<(u64, String)>,
    /// Nombre d'entrées déjà parcourues pour `authors`.
    seen: usize,
}

pub(super) fn open(app: &mut App, guild: u64) {
    let st = &mut app.moderation.audit;
    st.selected = None;
    let same = app.store.moderation.audit.as_ref().is_some_and(|v| v.guild_id == guild);
    if !same {
        *st = AuditUi::default();
    }
    app.store.load_audit(guild, st.filter, false);
}

/// Titre d'une famille d'actions (filtre), sans le verbe.
fn kind_title(k: Kind) -> Option<Text> {
    Some(match k {
        Kind::GuildUpdate => tx::K_GUILD,
        Kind::ChannelCreate | Kind::ChannelUpdate | Kind::ChannelDelete => tx::K_CHANNELS,
        Kind::Overwrite => tx::K_PERMS,
        Kind::Kick => tx::K_KICK,
        Kind::Prune => tx::K_PRUNE,
        Kind::Ban => tx::K_BAN,
        Kind::Unban => tx::K_UNBAN,
        Kind::MemberUpdate => tx::K_MEMBER,
        Kind::MemberRoles => tx::K_MEMBER_ROLES,
        Kind::MemberMove => tx::K_MOVE,
        Kind::MemberDisconnect => tx::K_DISCONNECT,
        Kind::BotAdd => tx::K_BOT,
        Kind::RoleCreate | Kind::RoleUpdate | Kind::RoleDelete => tx::K_ROLES,
        Kind::Invite => tx::K_INVITES,
        Kind::Webhook => tx::K_WEBHOOKS,
        Kind::Emoji => tx::K_EMOJIS,
        Kind::MessageDelete => tx::K_MESSAGE_DELETE,
        Kind::MessageBulkDelete => tx::K_BULK_DELETE,
        Kind::Pin => tx::K_PIN,
        Kind::Unpin => tx::K_UNPIN,
        Kind::Integration => tx::K_INTEGRATIONS,
        Kind::Sticker => tx::K_STICKERS,
        Kind::Event => tx::K_EVENTS,
        Kind::Thread => tx::K_THREADS,
        Kind::AutoModRule => tx::K_RULES,
        Kind::AutoModBlock => tx::K_AUTOMOD_BLOCK,
        Kind::AutoModAlert => tx::K_AUTOMOD_ALERT,
        Kind::AutoModTimeout => tx::K_AUTOMOD_TIMEOUT,
        _ => return None,
    })
}

/// Objet des familles créer/modifier/supprimer (`None` : phrase propre).
fn noun(k: Kind) -> Option<Text> {
    Some(match k {
        Kind::ChannelCreate | Kind::ChannelUpdate | Kind::ChannelDelete => tx::N_CHANNEL,
        Kind::Overwrite => tx::N_PERMS,
        Kind::RoleCreate | Kind::RoleUpdate | Kind::RoleDelete => tx::N_ROLE,
        Kind::Invite => tx::N_INVITE,
        Kind::Webhook => tx::N_WEBHOOK,
        Kind::Emoji => tx::N_EMOJI,
        Kind::Integration => tx::N_INTEGRATION,
        Kind::Stage => tx::N_STAGE,
        Kind::Sticker => tx::N_STICKER,
        Kind::Event => tx::N_EVENT,
        Kind::Thread => tx::N_THREAD,
        Kind::Sound => tx::N_SOUND,
        Kind::AutoModRule => tx::N_RULE,
        _ => return None,
    })
}

/// Libellé d'un type d'action dans le filtre.
fn action_label(lang: Lang, action: u32) -> String {
    let k = audit::kind(action);
    let Some(title) = kind_title(k) else { return format!("#{action}") };
    if noun(k).is_none() {
        return title.get(lang).to_string();
    }
    let v = match audit::verb(action) {
        Verb::Create => tx::VL_CREATE,
        Verb::Update => tx::VL_UPDATE,
        Verb::Delete => tx::VL_DELETE,
    };
    f::label(lang, title.get(lang), v.get(lang))
}

/// Phrase d'une entrée : `user` et `target` déjà en texte.
fn sentence(lang: Lang, e: &Entry, user: &str, target: &str) -> String {
    let k = audit::kind(e.action);
    if let Some(n) = noun(k) {
        let v = match audit::verb(e.action) {
            Verb::Create => tx::V_CREATE,
            Verb::Update => tx::V_UPDATE,
            Verb::Delete => tx::V_DELETE,
        };
        return f::did(lang, user, v.get(lang), n.get(lang), target).trim_end().to_string();
    }
    match k {
        Kind::GuildUpdate => f::guild_update(lang, user),
        Kind::Kick => f::kick(lang, user, target),
        Kind::Prune => f::prune(lang, user),
        Kind::Ban => f::ban(lang, user, target),
        Kind::Unban => f::unban(lang, user, target),
        Kind::MemberUpdate => f::member_update(lang, user, target),
        Kind::MemberRoles => f::member_roles(lang, user, target),
        Kind::MemberMove => f::member_move(lang, user),
        Kind::MemberDisconnect => f::member_disconnect(lang, user),
        Kind::BotAdd => f::bot_add(lang, user, target),
        Kind::MessageDelete => f::message_delete(lang, user, target),
        Kind::MessageBulkDelete => f::bulk_delete(lang, user),
        Kind::Pin => f::pin(lang, user, target),
        Kind::Unpin => f::unpin(lang, user, target),
        Kind::Commands => f::commands(lang, user),
        Kind::Onboarding => f::onboarding(lang, user),
        Kind::AutoModBlock => f::automod_block(lang, target),
        Kind::AutoModAlert => f::automod_alert(lang, target),
        Kind::AutoModTimeout => f::automod_timeout(lang, target),
        Kind::AutoModQuarantine => f::automod_quarantine(lang, target),
        _ => f::other(lang, user, e.action),
    }
}

fn change_line(lang: Lang, c: &audit::Change) -> String {
    match (&c.old, &c.new) {
        (Some(o), Some(n)) => f::change(lang, &c.key, o, n),
        (None, Some(n)) => f::change_set(lang, &c.key, n),
        (Some(o), None) => f::change_unset(lang, &c.key, o),
        (None, None) => c.key.clone(),
    }
}

/// Nom de la cible : donné par la réponse, sinon salon ou rôle connu, sinon l'identifiant.
fn target_text(app: &App, guild: u64, e: &Entry) -> String {
    if let Some(n) = &e.target_name {
        return n.clone();
    }
    let Some(id) = e.target_id else { return e.extra.clone().unwrap_or_default() };
    if let Some(c) = app.store.channel_name(id) {
        return format!("#{c}");
    }
    let role = app.store.guilds.iter().find(|g| g.id == guild).and_then(|g| g.roles.iter().find(|r| r.id == id));
    if let Some(r) = role {
        return format!("@{}", r.name);
    }
    e.extra.clone().unwrap_or_else(|| id.to_string())
}

pub(super) fn show(ui: &mut egui::Ui, app: &mut App, guild: u64) {
    let t = app.theme.clone();
    let lang = app.lang;
    let h24 = app.h24();
    let view: Option<AuditView> = app.store.moderation.audit.clone().filter(|v| v.guild_id == guild);
    let loading = app.store.moderation.busy(ModAction::Audit, guild);
    // Auteurs vus : parcourus une fois par nouvelle entrée.
    if let Some(Load::Ready { items, .. }) = view.as_ref().map(|v| &v.list) {
        let st = &mut app.moderation.audit;
        if st.seen > items.len() {
            st.seen = 0;
        }
        for e in &items[st.seen..] {
            if let (Some(id), Some(name)) = (e.user_id, &e.user_name)
                && st.authors.len() < AUTHORS_MAX
                && !st.authors.iter().any(|(a, _)| *a == id)
            {
                st.authors.push((id, name.clone()));
            }
        }
        st.seen = items.len();
    }
    let st = &mut app.moderation.audit;
    let before = st.filter;
    let (mut refresh, mut more) = (false, false);
    ui.horizontal_wrapped(|ui| {
        let user_text = st.filter.user_id.and_then(|u| st.authors.iter().find(|(a, _)| *a == u).map(|(_, n)| n.clone())).unwrap_or_else(|| tx::ALL_USERS.get(lang).to_string());
        ui.label(RichText::new(tx::FILTER_USER.get(lang)).small().color(t.colors.muted));
        egui::ComboBox::from_id_salt("journal-auteur").selected_text(user_text).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.filter.user_id, None, tx::ALL_USERS.get(lang));
            for (id, name) in &st.authors {
                ui.selectable_value(&mut st.filter.user_id, Some(*id), name);
            }
        });
        let action_text = st.filter.action.map_or_else(|| tx::ALL_ACTIONS.get(lang).to_string(), |a| action_label(lang, a));
        ui.label(RichText::new(tx::FILTER_ACTION.get(lang)).small().color(t.colors.muted));
        egui::ComboBox::from_id_salt("journal-action").selected_text(action_text).height(t.layout.card_width).show_ui(ui, |ui| {
            ui.selectable_value(&mut st.filter.action, None, tx::ALL_ACTIONS.get(lang));
            for a in audit::FILTERS {
                ui.selectable_value(&mut st.filter.action, Some(a), action_label(lang, a));
            }
        });
        refresh = ui.add_enabled(!loading, crate::ui::form::btn(&t, tx::REFRESH.get(lang))).clicked();
        if loading {
            ui.spinner();
        }
    });
    let filter = st.filter;
    let selected = st.selected;
    let mut pick = None;
    match view.map(|v| v.list) {
        None | Some(Load::Loading) => {
            ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
        }
        Some(Load::Failed(problem)) => {
            ui.label(RichText::new(crate::ui::moderation::refusal(lang, &problem, h24)).color(t.colors.danger));
        }
        Some(Load::Ready { items, more: has_more }) => {
            if items.is_empty() {
                ui.label(RichText::new(tx::NO_ENTRIES.get(lang)).color(t.colors.muted));
            }
            let row_h = ui.spacing().interact_size.y * 1.6;
            egui::ScrollArea::vertical().id_salt("journal").max_height(row_h * 12.0).auto_shrink([false, true]).show_rows(ui, row_h, items.len(), |ui, range| {
                for e in &items[range] {
                    let user = if matches!(audit::kind(e.action), Kind::AutoModBlock | Kind::AutoModAlert | Kind::AutoModTimeout | Kind::AutoModQuarantine) {
                        tx::AUTOMOD.get(lang).to_string()
                    } else {
                        e.user_name.clone().unwrap_or_else(|| tx::SOMEONE.get(lang).to_string())
                    };
                    let text = sentence(lang, e, &user, &target_text(app, guild, e));
                    let when = crate::i18n::time::clock(lang, snowflake_ms(e.id), true, h24);
                    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                    if selected == Some(e.id) {
                        ui.painter().rect_filled(rect, t.r_row(), t.colors.selected);
                    } else if resp.hovered() {
                        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
                    }
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(t.spacing.item_x, 0.0))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    if crate::ui::mobile::is_active(ui.ctx()) {
                        // Disposition mobile : l'heure d'abord, la phrase raccourcie dans la place qui reste.
                        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(when).small().color(t.colors.faint));
                            let size = egui::vec2(ui.available_width(), ui.available_height());
                            ui.allocate_ui_with_layout(size, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                ui.add(egui::Label::new(RichText::new(&text).color(t.colors.text)).truncate().selectable(false));
                            });
                        });
                    } else {
                        child.add(egui::Label::new(RichText::new(&text).color(t.colors.text)).truncate().selectable(false));
                        child.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(RichText::new(when).small().color(t.colors.faint));
                        });
                    }
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &text));
                    if resp.clicked() {
                        pick = Some(if selected == Some(e.id) { None } else { Some(e.id) });
                    }
                }
            });
            if let Some(e) = selected.and_then(|id| items.iter().find(|e| e.id == id)) {
                super::section(ui, &t, tx::CHANGES.get(lang));
                if let Some(r) = &e.reason {
                    ui.add(egui::Label::new(RichText::new(f::reason_is(lang, r)).color(t.colors.text)).wrap());
                }
                if e.changes.is_empty() {
                    super::note(ui, &t, tx::NO_CHANGES.get(lang));
                }
                for c in &e.changes {
                    ui.add(egui::Label::new(RichText::new(change_line(lang, c)).small().color(t.colors.muted)).wrap());
                }
            }
            if has_more {
                more = ui.add_enabled(!loading, crate::ui::form::btn(&t, tx::LOAD_MORE.get(lang))).clicked();
            }
        }
    }
    if let Some(p) = pick {
        app.moderation.audit.selected = p;
    }
    if filter != before || refresh {
        app.moderation.audit.selected = None;
        app.store.load_audit(guild, filter, false);
    } else if more {
        app.store.load_audit(guild, filter, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(action: u32) -> Entry {
        Entry { id: 1, action, user_id: Some(2), user_name: Some("Alice".into()), target_id: Some(3), target_name: None, reason: None, changes: Vec::new(), count: None, channel_id: None, extra: None }
    }

    #[test]
    fn sentences_and_labels() {
        assert_eq!(sentence(Lang::Fr, &entry(22), "Alice", "Bob"), "Alice a banni Bob");
        assert_eq!(sentence(Lang::En, &entry(30), "Alice", "@mod"), "Alice created the role @mod");
        assert_eq!(sentence(Lang::Fr, &entry(12), "Alice", "#général"), "Alice a supprimé le salon #général");
        assert_eq!(sentence(Lang::En, &entry(9999), "Alice", ""), "Alice performed action #9999");
        assert_eq!(action_label(Lang::Fr, 31), "Rôles, modification");
        assert_eq!(action_label(Lang::En, 20), "Kick");
        assert!(audit::FILTERS.iter().all(|a| !action_label(Lang::Fr, *a).starts_with('#')));
        let c = audit::Change { key: "name".into(), old: Some("a".into()), new: Some("b".into()) };
        assert_eq!(change_line(Lang::En, &c), "name: a > b");
    }
}
