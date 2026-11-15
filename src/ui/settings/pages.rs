//! Pages Données et confidentialité, Confidentialité de l'activité et
//! À propos.

use super::{in_discord, note, section, toggle, value_row, App};
use crate::i18n::table::s::t as st;
use crate::i18n::table::s2::t as s2;
use crate::i18n::Text;
use crate::state::usersettings::extra::ConsentKind;
use crate::state::usersettings::schema::Key;
use crate::state::usersettings::Remote;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Partage de l'activité dans les serveurs (`default_guilds_activity_restricted`).
const GUILDS_DEFAULT: [(u64, Text); 3] = [(0, s2::GD_ALL), (1, s2::GD_LARGE), (2, s2::GD_NONE)];

/// Commit de la compilation, si la variable était définie (aucune étape de
/// compilation supplémentaire).
pub(super) const COMMIT: Option<&str> = option_env!("FERROCORD_COMMIT");

impl App {
    pub(super) fn page_data(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if self.proto_header(ui, t) {
            self.synced_toggle(ui, Key::AccessibilityDetection, s2::ACCESS_DETECT, s2::ACCESS_DETECT_TIP, false);
        }
        section(ui, t, s2::SEC_DATA_USE.get(lang));
        let x = &mut self.store.account.extra;
        if x.consents == Remote::Idle {
            x.load_consents();
        }
        let h24 = self.h24();
        let x = &mut self.store.account.extra;
        let ready = super::account::remote_line(ui, t, lang, &x.consents, h24);
        if matches!(x.consents, Remote::Failed(_)) && ui.button(st::LOAD_FROM_DISCORD.get(lang)).clicked() {
            x.load_consents();
        }
        if ready && let Some(c) = x.consents.ready() {
            let c = *c;
            let enabled = !x.consent_busy();
            let mut change = None;
            for (kind, label, tip) in [(ConsentKind::UsageStatistics, s2::USAGE_STATS, s2::USAGE_STATS_TIP), (ConsentKind::Personalization, s2::PERSONALIZATION, s2::PERSONALIZATION_TIP)] {
                if let Some(v) = toggle(ui, label.get(lang), tip.get(lang), c.get(kind), enabled) {
                    change = Some((kind, v));
                }
            }
            if !enabled {
                ui.label(RichText::new(st::SAVING.get(lang)).color(t.colors.muted));
            }
            if let Some((kind, on)) = change
                && let Err(r) = self.store.account.extra.set_consent(kind, on)
            {
                self.refused(r);
            }
        }
        in_discord(ui, lang, s2::REQUEST_DATA.get(lang), s2::REQUEST_DATA_TIP.get(lang));
        // Historique gardé sur cet ordinateur (lot H).
        self.offline_settings(ui, t);
    }

    pub(super) fn page_activity(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        if !self.proto_header(ui, t) {
            return;
        }
        self.synced_toggle(ui, Key::ShowCurrentGame, st::SHOW_GAME, st::SHOW_GAME_TIP, false);
        self.synced_enum(ui, Key::ActivityGuildsDefault, s2::GUILDS_DEFAULT, s2::GUILDS_DEFAULT_TIP, &GUILDS_DEFAULT);
        self.synced_toggle(ui, Key::ActivityJoinFriends, s2::JOIN_FRIENDS, s2::JOIN_FRIENDS_TIP, false);
        self.synced_toggle(ui, Key::ActivityJoinVoice, s2::JOIN_VOICE, s2::JOIN_VOICE_TIP, false);
        self.activity_games_ui(ui, t);
        self.rpc_settings_ui(ui, t);
    }

    pub(super) fn page_about(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        value_row(ui, st::VERSION.get(lang), st::VERSION_TIP.get(lang), env!("CARGO_PKG_VERSION"));
        value_row(ui, s2::COMMIT.get(lang), s2::COMMIT_TIP.get(lang), COMMIT.unwrap_or(s2::COMMIT_UNKNOWN.get(lang)));
        value_row(ui, s2::REPO.get(lang), s2::REPO_TIP.get(lang), s2::REPO_NONE.get(lang));
        section(ui, t, st::UNOFFICIAL.get(lang));
        note(ui, t, st::UNOFFICIAL_TIP.get(lang));

        section(ui, t, s2::SEC_LICENSES.get(lang));
        // Textes complets, intégrés à l'exécutable (lot C6).
        self.licenses_ui(ui, t);

        for n in [s2::LICENSE_OWN_FONTS, s2::LICENSE_FONTS, s2::LICENSE_ROOTS, s2::LICENSE_UNICODE, s2::LICENSE_OTHER] {
            note(ui, t, n.get(lang));
        }

        section(ui, t, s2::SEC_ATTRIBUTIONS.get(lang));
        note(ui, t, crate::i18n::table::emoji::t::ATTRIBUTION_TIP.get(lang));
    }
}
