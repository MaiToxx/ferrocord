//! Page « Bannissements » (lot A4) : liste paginée et virtualisée, recherche,
//! « Débannir » en deux clics ; bannissement en masse d'une liste
//! d'identifiants saisie à la main, confirmé avec le nombre.

use super::{note, section};
use crate::i18n::table::moderation::{p, t as tx};
#[cfg(test)]
use crate::state::admin::moderation::BanView;
use crate::state::admin::moderation::{Ban, Load, ModAction, BULK_BAN_MAX};
use crate::ui::moderation::{danger_button, BAN_DELETE};
use crate::ui::App;
use eframe::egui::{self, Id, RichText};

#[derive(Default)]
pub(in crate::ui) struct BansUi {
    search: String,
    /// « Débannir » cliqué une fois : utilisateur à confirmer.
    confirm: Option<u64>,
    reason: String,
    bulk: String,
    bulk_delete: usize,
    /// Confirmation du bannissement en masse affichée.
    bulk_confirm: bool,
}

pub(super) fn open(app: &mut App, guild: u64) {
    app.moderation.bans.confirm = None;
    app.store.load_bans(guild, false);
}

/// Identifiants saisis : nombres seulement, sans doublon, 200 au plus ; `None` sinon.
fn parse_ids(text: &str) -> Option<Vec<u64>> {
    let mut out: Vec<u64> = Vec::new();
    for w in text.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()) {
        let id: u64 = w.parse().ok()?;
        if !out.contains(&id) {
            out.push(id);
        }
    }
    (out.len() <= BULK_BAN_MAX).then_some(out)
}

fn matches(b: &Ban, q: &str) -> bool {
    q.is_empty() || b.name.to_lowercase().contains(q) || b.handle.to_lowercase().contains(q) || b.user_id.to_string().contains(q)
}

pub(super) fn show(ui: &mut egui::Ui, app: &mut App, guild: u64) {
    let t = app.theme.clone();
    let lang = app.lang;
    let h24 = app.h24();
    let view = app.store.moderation.bans.clone().filter(|v| v.guild_id == guild);
    let loading = app.store.moderation.busy(ModAction::Bans, guild);
    let (mut refresh, mut more, mut unban, mut ask) = (false, false, None, None);
    let st = &mut app.moderation.bans;
    // Le bouton au bord droit de la page (celui des « Débannir »), le champ dans la place qui reste.
    crate::ui::widgets::field_row(ui, &t, |ui| {
        refresh = ui.add_enabled(!loading, crate::ui::form::btn(&t, tx::REFRESH.get(lang))).clicked();
        if loading {
            ui.spinner();
        }
        crate::ui::form::field(ui, &t, &mut st.search, tx::SEARCH_BANS.get(lang), 0);
    });
    match view.map(|v| v.list) {
        None | Some(Load::Loading) => {
            ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
        }
        Some(Load::Failed(problem)) => {
            ui.label(RichText::new(crate::ui::moderation::refusal(lang, &problem, h24)).color(t.colors.danger));
        }
        Some(Load::Ready { items, more: has_more }) => {
            let q = st.search.trim().to_lowercase();
            let shown: Vec<&Ban> = items.iter().filter(|b| matches(b, &q)).collect();
            ui.label(RichText::new(p::bans(lang, items.len() as u64)).small().color(t.colors.faint));
            if items.is_empty() {
                ui.label(RichText::new(tx::NO_BANS.get(lang)).color(t.colors.muted));
            } else if shown.is_empty() {
                ui.label(RichText::new(tx::NO_MATCH.get(lang)).color(t.colors.muted));
            }
            let row_h = ui.spacing().interact_size.y * 2.0;
            egui::ScrollArea::vertical().id_salt("bannis").max_height(row_h * 8.0).auto_shrink([false, true]).show_rows(ui, row_h, shown.len(), |ui, range| {
                for b in &shown[range] {
                    ui.horizontal(|ui| {
                        ui.set_min_height(row_h);
                        ui.vertical(|ui| {
                            ui.label(RichText::new(&b.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                            let why = b.reason.as_deref().unwrap_or(tx::NO_REASON.get(lang));
                            ui.add(egui::Label::new(RichText::new(format!("{} · {why}", b.handle)).small().color(t.colors.muted)).truncate());
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let busy = app.store.moderation.busy(ModAction::Unban, b.user_id);
                            let confirming = st.confirm == Some(b.user_id);
                            let label = if confirming { tx::CONFIRM } else { tx::UNBAN };
                            if ui.add_enabled(!busy, danger_button(&t, label.get(lang))).clicked() {
                                if confirming {
                                    unban = Some(b.user_id);
                                } else {
                                    ask = Some(b.user_id);
                                }
                            }
                        });
                    });
                }
            });
            if st.confirm.is_some() {
                ui.label(RichText::new(tx::REASON.get(lang)).small().color(t.colors.muted));
                crate::ui::form::field(ui, &t, &mut st.reason, tx::REASON_HINT.get(lang), 0);
            }
            if has_more {
                more = ui.add_enabled(!loading, crate::ui::form::btn(&t, tx::LOAD_MORE.get(lang))).clicked();
            }
        }
    }
    // Bannissement en masse : seulement depuis une liste écrite et confirmée.
    section(ui, &t, tx::BULK_BAN.get(lang));
    note(ui, &t, tx::BULK_BAN_TIP.get(lang));
    ui.add(egui::TextEdit::multiline(&mut st.bulk).desired_rows(3).desired_width(f32::INFINITY));
    let ids = parse_ids(&st.bulk);
    if ids.is_none() {
        ui.label(RichText::new(tx::BULK_BAN_BAD.get(lang)).small().color(t.colors.danger));
    }
    let n = ids.as_ref().map_or(0, Vec::len);
    ui.horizontal(|ui| {
        ui.label(RichText::new(tx::DELETE_HISTORY.get(lang)).small().color(t.colors.muted));
        let label = |s: u32| if s == 0 { tx::DONT_DELETE.get(lang).to_string() } else { crate::ui::moderation::span(lang, u64::from(s)) };
        egui::ComboBox::from_id_salt("bannissement-masse-historique").selected_text(label(BAN_DELETE[st.bulk_delete.min(BAN_DELETE.len() - 1)])).show_ui(ui, |ui| {
            for (i, s) in BAN_DELETE.iter().enumerate() {
                ui.selectable_value(&mut st.bulk_delete, i, label(*s));
            }
        });
    });
    let bulk_busy = app.store.moderation.busy(ModAction::BulkBan, guild);
    if ui.add_enabled(n > 0 && !bulk_busy, danger_button(&t, &p::ban_n(lang, n as u64))).clicked() {
        st.bulk_confirm = true;
    }
    if st.bulk_confirm {
        let (mut yes, mut no) = (false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("bannissement-masse"), width, &p::confirm_bulk_ban(lang, n as u64), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            crate::ui::settings::shell::page_style(ui, &t);
            ui.add(egui::Label::new(RichText::new(tx::BAN_TIP.get(lang)).color(t.colors.muted)).wrap());
            crate::ui::widgets::field_label(ui, &t, tx::REASON.get(lang));
            crate::ui::form::field(ui, &t, &mut st.reason, tx::REASON_HINT.get(lang), 0);
        }, |ui| {
            yes = ui.add_enabled(n > 0, danger_button(&t, &p::ban_n(lang, n as u64))).clicked();
            no = ui.add(crate::ui::form::link(&t, tx::CANCEL.get(lang))).clicked();
        });
        if yes && let Some(ids) = ids {
            let (secs, why) = (BAN_DELETE[st.bulk_delete.min(BAN_DELETE.len() - 1)], std::mem::take(&mut st.reason));
            st.bulk_confirm = false;
            st.bulk.clear();
            app.store.bulk_ban(guild, &ids, secs, &why);
            return;
        }
        if no || modal.closed {
            st.bulk_confirm = false;
        }
    }
    if let Some(u) = ask {
        st.confirm = Some(u);
    }
    if let Some(u) = unban {
        st.confirm = None;
        let why = std::mem::take(&mut st.reason);
        app.store.unban(guild, u, &why);
    }
    if refresh {
        app.store.load_bans(guild, false);
    }
    if more {
        app.store.load_bans(guild, true);
    }
}

/// Le serveur affiché a-t-il sa liste ? (Tests.)
#[cfg(test)]
fn ready(v: &Option<BanView>, guild: u64) -> bool {
    matches!(v, Some(BanView { guild_id, list: Load::Ready { .. } }) if *guild_id == guild)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_parsed_strictly() {
        assert_eq!(parse_ids("1 2\n3,3"), Some(vec![1, 2, 3]));
        assert_eq!(parse_ids(""), Some(vec![]));
        assert_eq!(parse_ids("12 bob"), None);
        let many: String = (1..=201).map(|i| format!("{i}\n")).collect();
        assert_eq!(parse_ids(&many), None);
        assert!(!ready(&None, 1));
    }
}
