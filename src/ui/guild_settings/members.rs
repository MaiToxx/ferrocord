//! Page « Membres » (lot A3) : membres connus de Ferrocord (liste des
//! membres, messages, recherche sur le serveur), filtre par rôle, tri par
//! nom ou date d'arrivée, et les actions permises sur chacun.

use super::super::App;
use super::roles::{matches, member_names, member_roles};
use super::rgb;
use crate::i18n::table::roles::{p, t as rt};
use crate::i18n::time::long_date;
use crate::state::admin::roles as admin;
use crate::state::model::Avatar;
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Sort {
    #[default]
    Name,
    Newest,
    Oldest,
}

/// Ligne calculée une fois par changement de filtre (pas à chaque image).
#[derive(Clone)]
struct Row {
    user_id: u64,
    name: String,
    handle: String,
    joined: Option<i64>,
    roles: Option<Vec<u64>>,
    avatar: Avatar,
}

type Key = (u64, String, Option<u64>, Sort, (usize, usize, usize), u64);

#[derive(Default)]
pub(in crate::ui) struct MembersPage {
    query: String,
    role: Option<u64>,
    sort: Sort,
    rows: Vec<Row>,
    key: Option<Key>,
    /// Les rôles d'un membre changent sans changer la taille des tables :
    /// la liste est refaite au plus toutes les secondes pendant qu'on la regarde.
    built: Option<Instant>,
}

const REFRESH: Duration = Duration::from_secs(1);

/// Actions de modération sur un membre (lot A4) : exclure ou lever
/// l'exclusion, expulser, bannir, selon mes droits et la hiérarchie ; chacune
/// ouvre sa fenêtre de confirmation (`ui::moderation`).
fn moderation_actions(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64, user_id: u64, name: &str) {
    let lang = app.lang;
    // De droite à gauche : la plus grave au bout de la ligne.
    for k in app.moderation_picks_in(guild_id, user_id).into_iter().rev() {
        let label = k.label().get(lang);
        if ui.add(if k.danger() { crate::ui::form::danger(t, label) } else { crate::ui::form::btn(t, label) }).clicked() {
            app.open_member_action_in(guild_id, user_id, name, k);
        }
    }
}

fn rows(app: &App, guild_id: u64, q: &str, role: Option<u64>, sort: Sort) -> Vec<Row> {
    let mut out: Vec<Row> = Vec::new();
    for u in app.store.known_members(guild_id) {
        let roles = member_roles(app, guild_id, u);
        if let Some(r) = role
            && !roles.as_ref().is_some_and(|rs| rs.contains(&r))
        {
            continue;
        }
        let (name, handle) = member_names(app, guild_id, u);
        if !(matches(&name, q) || matches(&handle, q)) {
            continue;
        }
        let people = app.store.people.lock();
        let joined = people.member(guild_id, u).and_then(|m| m.joined_at);
        let avatar = people.user(u).map_or_else(|| Avatar::default_for(u), |x| x.avatar);
        out.push(Row { user_id: u, name, handle, joined, roles, avatar });
    }
    match sort {
        Sort::Name => out.sort_by_cached_key(|r| crate::state::nav::sort_key(&r.name)),
        // Dates inconnues en dernier.
        Sort::Newest => out.sort_by_key(|r| std::cmp::Reverse(r.joined.unwrap_or(i64::MIN))),
        Sort::Oldest => out.sort_by_key(|r| r.joined.unwrap_or(i64::MAX)),
    }
    out
}

pub(super) fn show(app: &mut App, ui: &mut egui::Ui, t: &Tokens, guild_id: u64) {
    let lang = app.lang;
    super::super::settings::note(ui, t, rt::MEMBERS_INTRO.get(lang));
    ui.add_space(t.spacing.item_y);
    let Some(g) = app.store.guilds.iter().find(|g| g.id == guild_id) else { return };
    let roles: Vec<(u64, String, u32)> = admin::sorted(&g.roles).into_iter().filter(|r| r.id != guild_id).map(|r| (r.id, r.name.clone(), r.color)).collect();
    let loading = app.store.roles_admin.search(guild_id).is_some_and(|s| s.loading);
    let mut search = false;
    {
        let page = &mut app.roles_admin.members;
        if page.role.is_some_and(|r| !roles.iter().any(|(id, _, _)| *id == r)) {
            page.role = None;
        }
        // Une seule ligne, jusqu'au bord droit de la page : les filtres d'abord
        // (de droite à gauche), le champ dans la place qui reste. Disposition
        // mobile : renvoi à la ligne, comme avant.
        // Page étroite : pas la place d'une seule ligne, le champ serait écrasé.
        let mobile = crate::ui::mobile::is_active(ui.ctx()) || ui.available_width() < 1.75 * t.layout.card_width;
        let by_role = |ui: &mut egui::Ui, page: &mut MembersPage| {
            let current = page.role.and_then(|r| roles.iter().find(|(id, _, _)| *id == r)).map_or(rt::ALL_ROLES.get(lang).to_string(), |(_, n, _)| n.clone());
            egui::ComboBox::from_id_salt(("membres-role", guild_id)).selected_text(current).show_ui(ui, |ui| {
                ui.selectable_value(&mut page.role, None, rt::ALL_ROLES.get(lang));
                for (id, name, _) in &roles {
                    ui.selectable_value(&mut page.role, Some(*id), name);
                }
            }).response.on_hover_text(rt::FILTER_ROLE.get(lang));
        };
        let by_sort = |ui: &mut egui::Ui, page: &mut MembersPage| {
            let sort_text = |s: Sort| match s {
                Sort::Name => rt::SORT_NAME.get(lang),
                Sort::Newest => rt::SORT_NEWEST.get(lang),
                Sort::Oldest => rt::SORT_OLDEST.get(lang),
            };
            egui::ComboBox::from_id_salt(("membres-tri", guild_id)).selected_text(sort_text(page.sort)).show_ui(ui, |ui| {
                for s in [Sort::Name, Sort::Newest, Sort::Oldest] {
                    ui.selectable_value(&mut page.sort, s, sort_text(s));
                }
            });
        };
        let find = |ui: &mut egui::Ui, page: &mut MembersPage, search: &mut bool| {
            *search = ui.add_enabled(!page.query.trim().is_empty() && !loading, crate::ui::form::btn(t, rt::SEARCH_SERVER.get(lang))).clicked();
            if loading {
                ui.spinner();
            }
        };
        if mobile {
            ui.horizontal_wrapped(|ui| {
                let w = (ui.available_width() * 0.4).max(t.layout.button_height);
                ui.add(egui::TextEdit::singleline(&mut page.query).hint_text(rt::SEARCH_MEMBERS.get(lang)).desired_width(w));
                find(ui, page, &mut search);
                by_role(ui, page);
                by_sort(ui, page);
            });
        } else {
            ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), ui.spacing().interact_size.y), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                by_sort(ui, page);
                by_role(ui, page);
                find(ui, page, &mut search);
                crate::ui::form::field(ui, t, &mut page.query, rt::SEARCH_MEMBERS.get(lang), 0);
            });
        }
    }
    if search {
        let q = app.roles_admin.members.query.clone();
        app.store.search_members(guild_id, &q);
    }
    // Liste refaite si un filtre, les tables ou les rôles changent.
    let page = &app.roles_admin.members;
    let q = page.query.trim().to_lowercase();
    let key: Key = (guild_id, q.clone(), page.role, page.sort, app.store.people.lock().sizes(), app.store.roles_admin.rev());
    let stale = page.key.as_ref() != Some(&key) || page.built.is_none_or(|at| at.elapsed() > REFRESH);
    if stale {
        let built = rows(app, guild_id, &q, page.role, page.sort);
        let page = &mut app.roles_admin.members;
        page.rows = built;
        page.key = Some(key);
        page.built = Some(Instant::now());
    }
    let n = app.roles_admin.members.rows.len();
    ui.add_space(t.spacing.item_y);
    ui.label(RichText::new(p::known_members(lang, n as u64)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
    ui.separator();
    if n == 0 {
        ui.label(RichText::new(if q.is_empty() { rt::NO_MEMBERS.get(lang) } else { rt::NO_MEMBER_FOUND.get(lang) }).color(t.colors.muted));
        return;
    }
    let can_roles = app.store.can_manage_roles(guild_id);
    let row_h = t.spacing.member_row;
    let avatar = t.layout.small_avatar;
    // Colonne des boutons : aussi large que la ligne qui en a le plus, pour
    // que dates et rôles tombent au même endroit sur toutes les lignes.
    let actions_id = ui.scope_id().with(("membres-actions", guild_id));
    let reserve = ui.data(|d| d.get_temp::<f32>(actions_id)).unwrap_or(0.0);
    let mut widest = reserve;
    // Page étroite : les boutons ne laissent plus de place au nom, ils passent dans un menu.
    let narrow = crate::ui::mobile::is_active(ui.ctx()) || ui.available_width() < reserve + avatar + t.layout.card_width / 2.0;
    let mut open = None;
    let name_w = (ui.available_width() * 0.14).clamp(96.0, 150.0);
    let small = egui::FontId::proportional(t.typography.small);
    egui::ScrollArea::vertical().id_salt(("membres-liste", guild_id)).auto_shrink([false, false]).show_rows(ui, row_h, n, |ui, range| {
        for i in range {
            let Some(r) = app.roles_admin.members.rows.get(i).cloned() else { continue };
            ui.horizontal(|ui| {
                ui.set_height(row_h);
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(avatar), Sense::hover());
                super::super::util::paint_avatar(ui, &app.images, t, rect, &r.avatar, &r.name);
                ui.vertical(|ui| {
                    // Colonne du nom de largeur fixe : les rôles et la date, posés depuis la
                    // droite, s'arrêtent à son bord au lieu de passer dessus.
                    ui.set_width(name_w);
                    ui.add(egui::Label::new(RichText::new(&r.name).color(t.colors.text_strong)).truncate());
                    ui.add(egui::Label::new(RichText::new(&r.handle).small().color(t.colors.faint)).truncate());
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let nick = app.store.can_set_nick(guild_id, r.user_id);
                    // Disposition mobile : les actions dans un menu, le nom reste lisible.
                    if narrow {
                        let more = |ui: &mut egui::Ui| {
                            if ((can_roles && r.roles.is_some()) || nick) && ui.add(crate::ui::form::btn(t, if can_roles { rt::MENU_ROLES } else { rt::MENU_NICK }.get(lang))).clicked() {
                                open = Some(r.user_id);
                            }
                            moderation_actions(app, ui, t, guild_id, r.user_id, &r.name);
                        };
                        if crate::ui::mobile::is_active(ui.ctx()) {
                            crate::ui::mobile::more_menu(ui, more);
                            return;
                        }
                        egui::containers::menu::MenuButton::new("…").ui(ui, more);
                    } else {
                    ui.allocate_ui_with_layout(egui::vec2(reserve, ui.available_height()), egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if (can_roles && r.roles.is_some()) || nick {
                            let label = if can_roles { rt::MENU_ROLES } else { rt::MENU_NICK };
                            if ui.add(crate::ui::form::btn(t, label.get(lang))).clicked() {
                                open = Some(r.user_id);
                            }
                        }
                        moderation_actions(app, ui, t, guild_id, r.user_id, &r.name);
                        widest = widest.max(ui.min_rect().width());
                        ui.expand_to_include_x(ui.max_rect().left());
                    });
                    }
                    // Ce qui suit ne se pose que s'il tient en entier : rien ne déborde sur le nom.
                    let gap = ui.spacing().item_spacing.x;
                    let wide = |ui: &egui::Ui, text: &str| ui.painter().layout_no_wrap(text.to_owned(), small.clone(), t.colors.muted).size().x + gap;
                    let joined = r.joined.map_or_else(|| rt::UNKNOWN_DATE.get(lang).to_string(), |ms| long_date(lang, ms));
                    if wide(ui, &joined) <= ui.available_width() {
                        ui.label(RichText::new(joined).small().color(t.colors.muted)).on_hover_text(rt::COL_JOINED.get(lang));
                    }
                    // Puces des rôles : les plus hauts d'abord, trois au plus.
                    let mine: Vec<&(u64, String, u32)> = roles.iter().filter(|(id, _, _)| r.roles.as_ref().is_some_and(|rs| rs.contains(id))).collect();
                    // Trois au plus, et seulement ceux qui tiennent ; le reste est compté (« +2 »).
                    let more_w = wide(ui, "+9");
                    let mut room = ui.available_width() - more_w;
                    let fit = mine.iter().take(3).take_while(|(_, name, _)| {
                        room -= wide(ui, &format!(" {name} "));
                        room >= 0.0
                    }).count();
                    if mine.len() > fit {
                        ui.label(RichText::new(format!("+{}", mine.len() - fit)).small().color(t.colors.muted));
                    }
                    for (_, name, color) in mine.iter().take(fit).rev() {
                        let c = if *color == 0 { t.colors.muted } else { rgb(*color) };
                        ui.add(egui::Label::new(RichText::new(format!(" {name} ")).small().color(c).background_color(t.colors.bg1)).selectable(false));
                    }
                });
            });
        }
    });
    if widest > reserve + 0.5 {
        ui.data_mut(|d| d.insert_temp(actions_id, widest));
        ui.ctx().request_discard("colonne des actions des membres");
    }
    if let Some(u) = open {
        app.open_member_dialog(guild_id, u);
    }
}
