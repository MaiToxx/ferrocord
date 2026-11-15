//! Page « Sécurité » (lot A5) : niveau de vérification, filtre de contenu
//! explicite, notifications par défaut ; A2F des modérateurs en lecture seule
//! (son changement demande un code, laissé à l'application officielle) ;
//! pause des invitations et des MP contre un raid, confirmée.

use super::{note, section};
use crate::i18n::table::moderation::{f, t as tx};
use crate::i18n::Text;
use crate::state::admin::moderation::{Load, ModAction, SafetyChange, SafetyView};
use crate::ui::moderation::{danger_button, span};
use crate::ui::App;
use eframe::egui::{self, Id, RichText};

/// Durées de pause proposées (secondes), 24 h au plus.
const PAUSES: [u64; 6] = [3600, 7200, 14_400, 21_600, 43_200, 86_400];
const VERIFICATION: [Text; 5] = [tx::V0, tx::V1, tx::V2, tx::V3, tx::V4];
const EXPLICIT: [Text; 3] = [tx::E0, tx::E1, tx::E2];
const NOTIFS: [Text; 2] = [tx::N0, tx::N1];
/// Page de l'application officielle pour l'A2F des modérateurs.
const OFFICIAL_APP: &str = "https://discord.com/app";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Paused {
    Invites,
    Dms,
}

#[derive(Default)]
pub(in crate::ui) struct SafetyUi {
    /// Indice dans [`PAUSES`].
    pause: usize,
    confirm: Option<Paused>,
}

pub(super) fn open(app: &mut App, guild: u64) {
    app.moderation.safety.confirm = None;
    app.store.load_safety(guild);
}

/// Liste déroulante d'un réglage ; renvoie la valeur choisie si elle change.
fn choice(ui: &mut egui::Ui, id: &str, lang: crate::i18n::Lang, options: &[Text], current: Option<u8>, enabled: bool) -> Option<u8> {
    let shown = current.and_then(|c| options.get(usize::from(c))).map_or("—", |t| t.get(lang));
    let mut picked = None;
    ui.add_enabled_ui(enabled, |ui| {
        egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
            for (i, o) in options.iter().enumerate() {
                let v = u8::try_from(i).unwrap_or(0);
                if ui.selectable_label(current == Some(v), o.get(lang)).clicked() && current != Some(v) {
                    picked = Some(v);
                }
            }
        });
    });
    picked
}

pub(super) fn show(ui: &mut egui::Ui, app: &mut App, guild: u64) {
    let t = app.theme.clone();
    let lang = app.lang;
    let h24 = app.h24();
    let now = app.store.now();
    let view: Option<SafetyView> = app.store.moderation.safety.clone().filter(|v| v.guild_id == guild);
    let s = match view.map(|v| v.state) {
        None | Some(Load::Loading) => {
            ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
            return;
        }
        Some(Load::Failed(problem)) => {
            ui.label(RichText::new(crate::ui::moderation::refusal(lang, &problem, h24)).color(t.colors.danger));
            if ui.add(crate::ui::form::btn(&t, tx::REFRESH.get(lang))).clicked() {
                app.store.load_safety(guild);
            }
            return;
        }
        Some(Load::Ready { items, .. }) => items,
    };
    let busy = app.store.moderation.busy(ModAction::SetSafety, guild);
    let mut change = None;
    section(ui, &t, tx::VERIFICATION.get(lang));
    note(ui, &t, tx::VERIFICATION_TIP.get(lang));
    if let Some(v) = choice(ui, "securite-verification", lang, &VERIFICATION, s.verification, !busy) {
        change = Some(SafetyChange::Verification(v));
    }
    section(ui, &t, tx::EXPLICIT.get(lang));
    note(ui, &t, tx::EXPLICIT_TIP.get(lang));
    if let Some(v) = choice(ui, "securite-explicite", lang, &EXPLICIT, s.explicit_filter, !busy) {
        change = Some(SafetyChange::ExplicitFilter(v));
    }
    section(ui, &t, tx::NOTIFS.get(lang));
    note(ui, &t, tx::NOTIFS_TIP.get(lang));
    if let Some(v) = choice(ui, "securite-notifications", lang, &NOTIFS, s.notifications, !busy) {
        change = Some(SafetyChange::Notifications(v));
    }
    // A2F : lecture seule, Discord demande un code que Ferrocord ne gère pas.
    section(ui, &t, tx::MFA.get(lang));
    ui.horizontal(|ui| {
        let on = s.mfa == Some(1);
        ui.label(RichText::new(if on { tx::MFA_ON } else { tx::MFA_OFF }.get(lang)).color(t.colors.text));
        if ui.add(crate::ui::form::btn(&t, tx::OPEN_IN_DISCORD.get(lang))).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(OFFICIAL_APP));
        }
    });
    note(ui, &t, tx::MFA_TIP.get(lang));
    // Raids : pause des invitations et des MP.
    section(ui, &t, tx::RAIDS.get(lang));
    note(ui, &t, tx::RAIDS_TIP.get(lang));
    if let Some(at) = s.raid_detected_at {
        ui.label(RichText::new(f::raid_detected(lang, crate::i18n::time::clock_mid(lang, at, h24))).color(t.colors.danger));
    }
    let incidents_busy = app.store.moderation.busy(ModAction::Incidents, guild);
    let invites = s.invites_disabled_until.filter(|u| *u > now);
    let dms = s.dms_disabled_until.filter(|u| *u > now);
    let st = &mut app.moderation.safety;
    let mut resume = None;
    ui.horizontal(|ui| {
        ui.label(RichText::new(tx::FOR.get(lang)).small().color(t.colors.muted));
        egui::ComboBox::from_id_salt("securite-pause").selected_text(span(lang, PAUSES[st.pause.min(PAUSES.len() - 1)])).show_ui(ui, |ui| {
            for (i, s) in PAUSES.iter().enumerate() {
                ui.selectable_value(&mut st.pause, i, span(lang, *s));
            }
        });
    });
    for (what, label, until) in [(Paused::Invites, tx::INVITES, invites), (Paused::Dms, tx::DMS, dms)] {
        ui.horizontal(|ui| {
            ui.label(RichText::new(label.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
            match until {
                Some(u) => {
                    ui.label(RichText::new(f::paused_until(lang, crate::i18n::time::until(lang, u, h24))).color(t.colors.danger));
                    if ui.add_enabled(!incidents_busy, crate::ui::form::btn(&t, tx::RESUME.get(lang))).clicked() {
                        resume = Some(what);
                    }
                }
                None => {
                    ui.label(RichText::new(tx::ACTIVE.get(lang)).color(t.colors.muted));
                    if ui.add_enabled(!incidents_busy, danger_button(&t, tx::PAUSE.get(lang))).clicked() {
                        st.confirm = Some(what);
                    }
                }
            }
        });
    }
    let secs = PAUSES[st.pause.min(PAUSES.len() - 1)];
    let mut pause = None;
    if let Some(what) = st.confirm {
        let (mut yes, mut no) = (false, false);
        let name = if what == Paused::Invites { tx::INVITES } else { tx::DMS }.get(lang).to_lowercase();
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(ui.ctx(), &t, Id::unique("securite-pause-confirmer"), width, &f::confirm_pause(lang, name, span(lang, secs)), crate::i18n::t::CLOSE_TIP.get(lang), |_| {}, |ui| {
            yes = ui.add(danger_button(&t, tx::PAUSE.get(lang))).clicked();
            no = ui.add(crate::ui::form::link(&t, tx::CANCEL.get(lang))).clicked();
        });
        if yes {
            st.confirm = None;
            pause = Some(what);
        } else if no || modal.closed {
            st.confirm = None;
        }
    }
    // Les deux instants partent ensemble : l'autre garde sa valeur.
    let end = now.saturating_add(i64::try_from(secs * 1000).unwrap_or(0));
    let next = match (pause, resume) {
        (Some(Paused::Invites), _) => Some((Some(end), dms)),
        (Some(Paused::Dms), _) => Some((invites, Some(end))),
        (None, Some(Paused::Invites)) => Some((None, dms)),
        (None, Some(Paused::Dms)) => Some((invites, None)),
        (None, None) => None,
    };
    if let Some((i, d)) = next {
        app.store.set_incidents(guild, i, d);
    }
    if let Some(c) = change {
        app.store.set_safety(guild, c);
    }
}
