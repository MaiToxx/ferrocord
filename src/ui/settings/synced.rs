//! Pages des réglages synchronisés avec Discord (protobuf) : Statut,
//! Confidentialité, Texte et images, Notifications, Langue, Dossiers, et la
//! section « Apparence de Discord » de la page Apparence.

use super::{choice, note, section, toggle, value_row, App};
use crate::i18n::table::s::{f as sf, t as st};
use crate::i18n::{self, time, Choice, Text};
use crate::state::usersettings::schema::{CustomStatus, Key, SValue, FRIEND_MUTUAL_FRIENDS, FRIEND_MUTUAL_GUILDS, FRIEND_NO_RELATION};
use crate::state::usersettings::ProtoState;
use crate::theme::{MessageMode, Preset, Tokens};
use eframe::egui::{self, RichText};

/// Brouillon du statut personnalisé (rien n'est envoyé avant « Enregistrer »).
#[derive(Default)]
pub(in crate::ui) struct StatusDraft {
    /// Statut dont le brouillon est parti (rechargé s'il change ailleurs).
    base: Option<Option<CustomStatus>>,
    pub(in crate::ui) text: String,
    pub(in crate::ui) emoji: String,
    /// Emoji personnalisé choisi dans le sélecteur ou enregistré : identifiant,
    /// nom, animé. Il vaut tant que `emoji` porte son nom.
    pub(in crate::ui) custom: Option<(u64, String, bool)>,
    /// Rang dans [`EXPIRY`].
    pub(in crate::ui) expiry: usize,
}

impl StatusDraft {
    /// Repart du statut enregistré quand il a changé (ailleurs, ou par un
    /// enregistrement d'ici).
    pub(in crate::ui) fn sync(&mut self, saved: &Option<CustomStatus>) {
        if self.base.as_ref() != Some(saved) {
            self.text = saved.as_ref().map(|c| c.text.clone()).unwrap_or_default();
            self.emoji = saved.as_ref().map(|c| c.emoji_name.clone()).unwrap_or_default();
            self.custom = saved.as_ref().filter(|c| c.emoji_id != 0).map(|c| (c.emoji_id, c.emoji_name.clone(), false));
            self.expiry = 0;
            self.base = Some(saved.clone());
        }
    }

    /// Statut que le brouillon demande, à `now` (ms Unix).
    pub(in crate::ui) fn build(&self, saved: Option<&CustomStatus>, now: i64) -> CustomStatus {
        CustomStatus {
            text: self.text.trim().to_string(),
            emoji_name: self.emoji.trim().to_string(),
            // Emoji personnalisé (choisi, ou celui de l'ancien statut) gardé tant que le nom ne change pas.
            emoji_id: self.custom.as_ref().filter(|c| c.1 == self.emoji.trim()).map(|c| c.0).or_else(|| saved.filter(|c| c.emoji_name == self.emoji.trim()).map(|c| c.emoji_id)).unwrap_or(0),
            expires_at_ms: match EXPIRY[self.expiry.min(EXPIRY.len() - 1)].0 {
                0 => 0,
                TODAY => end_of_day_ms(now),
                min => u64::try_from(now).unwrap_or(0).saturating_add(min * 60_000),
            },
        }
    }

    /// Enregistrement demandé : le brouillon repartira de ce que Discord garde.
    pub(in crate::ui) fn sent(&mut self) {
        self.base = None;
    }
}

/// Durées proposées pour le statut personnalisé (minutes, 0 : jamais).
pub(in crate::ui) const EXPIRY: [(u64, Text); 6] =
    [(0, st::EXPIRE_NEVER), (30, st::EXPIRE_30M), (60, st::EXPIRE_1H), (240, st::EXPIRE_4H), (1440, st::EXPIRE_24H), (TODAY, crate::i18n::table::ff::t::EXPIRE_TODAY)];

/// « Aujourd'hui » (lot FF2, F27) : le statut s'efface à minuit, heure locale.
const TODAY: u64 = u64::MAX;

/// Prochain minuit local après `now_ms` (ms Unix) ; heure illisible : un jour plus tard.
fn end_of_day_ms(now_ms: i64) -> u64 {
    use chrono::TimeZone;
    let midnight = chrono::Local
        .timestamp_millis_opt(now_ms)
        .single()
        .and_then(|d| d.date_naive().succ_opt())
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .and_then(|d| chrono::Local.from_local_datetime(&d).earliest())
        .map_or(now_ms.saturating_add(86_400_000), |d| d.timestamp_millis());
    u64::try_from(midnight).unwrap_or(0)
}

pub(in crate::ui) const STATUSES: [(&str, Text); 4] = [("online", st::STATUS_ONLINE), ("idle", st::STATUS_IDLE), ("dnd", st::STATUS_DND), ("invisible", st::STATUS_INVISIBLE)];

/// Niveaux du filtre de contenu explicite (`ExplicitContentRedaction`).
const REDACTION: [(u64, Text); 4] = [(0, st::DEFAULT), (1, st::REDACT_SHOW), (2, st::REDACT_BLUR), (3, st::REDACT_BLOCK)];

/// Filtre des MP indésirables (`DmSpamFilterV2`).
const SPAM: [(u64, Text); 4] = [(0, st::DEFAULT), (1, st::SPAM_OFF), (2, st::SPAM_NON_FRIENDS), (3, st::SPAM_ALL)];

/// Autocollants animés (`animate_stickers`).
const STICKERS: [(u32, Text); 3] = [(0, st::STICKERS_ALWAYS), (1, st::STICKERS_HOVER), (2, st::STICKERS_NEVER)];

/// Thème du compte (`AppearanceSettings.theme`).
const THEMES: [(u64, Text); 5] = [(0, st::DEFAULT), (1, st::THEME_DARK), (2, st::THEME_LIGHT), (3, st::THEME_DARKER), (4, st::THEME_MIDNIGHT)];

/// Langues de l'interface de Discord (codes de l'API) ; chaque langue est
/// nommée dans la sienne quand la police de Ferrocord en a les lettres.
const LOCALES: [(&str, &str); 32] = [
    ("id", "Bahasa Indonesia"),
    ("da", "Dansk"),
    ("de", "Deutsch"),
    ("en-GB", "English (UK)"),
    ("en-US", "English (US)"),
    ("es-ES", "Español"),
    ("es-419", "Español (América Latina)"),
    ("fr", "Français"),
    ("hr", "Hrvatski"),
    ("it", "Italiano"),
    ("lt", "Lietuviškai"),
    ("hu", "Magyar"),
    ("nl", "Nederlands"),
    ("no", "Norsk"),
    ("pl", "Polski"),
    ("pt-BR", "Português do Brasil"),
    ("ro", "Română"),
    ("fi", "Suomi"),
    ("sv-SE", "Svenska"),
    ("vi", "Vietnamese"),
    ("tr", "Türkçe"),
    ("cs", "Čeština"),
    ("el", "Ελληνικά"),
    ("bg", "български"),
    ("ru", "Русский"),
    ("uk", "Українська"),
    ("hi", "Hindi"),
    ("th", "Thai"),
    ("zh-CN", "Chinese (China)"),
    ("ja", "Japanese"),
    ("zh-TW", "Chinese (Taiwan)"),
    ("ko", "Korean"),
];

/// Préréglage de Ferrocord le plus proche du thème du compte Discord.
pub(super) fn preset_for(theme: u64) -> Preset {
    match theme {
        2 => Preset::Clair,
        // Thème « minuit » du compte : le noir (lot V1).
        4 => Preset::Onyx,
        _ => Preset::DEFAULT,
    }
}

impl App {
    /// En-tête des pages synchronisées : état des réglages reçus ; `true` si
    /// les réglages sont lisibles (et modifiables hors écriture en cours).
    pub(super) fn proto_header(&mut self, ui: &mut egui::Ui, t: &Tokens) -> bool {
        let lang = self.lang;
        if !self.online() {
            note(ui, t, st::OFFLINE.get(lang));
            return false;
        }
        let (text, reload) = match self.store.account.proto_state() {
            ProtoState::Ready(_) => {
                note(ui, t, st::SYNCED_TIP.get(lang));
                if self.store.account.proto_busy() {
                    ui.label(RichText::new(st::SAVING.get(lang)).color(t.colors.muted));
                }
                return true;
            }
            ProtoState::Loading => (st::LOADING, false),
            ProtoState::Missing => (st::PROTO_MISSING, true),
            ProtoState::Unreadable => (st::PROTO_UNREADABLE, true),
        };
        ui.add(egui::Label::new(RichText::new(text.get(lang)).color(t.colors.muted)).wrap());
        if reload && ui.button(st::LOAD_FROM_DISCORD.get(lang)).clicked() {
            self.store.account.load_proto();
        }
        false
    }

    pub(super) fn write_setting(&mut self, key: Key, value: SValue) {
        if let Err(r) = self.store.account.write(key, value) {
            self.refused(r);
        }
    }

    /// Case d'un réglage synchronisé ; `invert` : le libellé dit le contraire du champ.
    pub(super) fn synced_toggle(&mut self, ui: &mut egui::Ui, key: Key, label: Text, detail: Text, invert: bool) {
        let lang = self.lang;
        let enabled = !self.store.account.proto_busy();
        let shown = self.store.account.bool(key).map(|b| b != invert);
        if let Some(v) = toggle(ui, label.get(lang), detail.get(lang), shown, enabled) {
            self.write_setting(key, SValue::Bool(v != invert));
        }
    }

    pub(super) fn synced_enum(&mut self, ui: &mut egui::Ui, key: Key, label: Text, detail: Text, options: &[(u64, Text)]) {
        let lang = self.lang;
        let enabled = !self.store.account.proto_busy();
        let current = match self.store.account.value(key) {
            Some(SValue::Enum(v)) => Some(*v),
            _ => None,
        };
        if let Some(v) = choice(ui, lang, &format!("{key:?}"), (label.get(lang), detail.get(lang)), options, current, enabled) {
            self.write_setting(key, SValue::Enum(v));
        }
    }

    pub(super) fn page_status(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if !self.proto_header(ui, t) {
            return;
        }
        let enabled = !self.store.account.proto_busy();
        let current = match self.store.account.value(Key::Status) {
            Some(SValue::Str(s)) => Some(STATUSES.iter().position(|(k, _)| k == s).unwrap_or(0)),
            _ => None,
        };
        let options: Vec<(usize, Text)> = STATUSES.iter().enumerate().map(|(i, (_, l))| (i, *l)).collect();
        if let Some(i) = choice(ui, lang, "statut", (st::ONLINE_STATUS.get(lang), st::ONLINE_STATUS_TIP.get(lang)), &options, current, enabled) {
            self.write_setting(Key::Status, SValue::Str(STATUSES[i].0.to_string()));
        }
        self.synced_toggle(ui, Key::ShowCurrentGame, st::SHOW_GAME, st::SHOW_GAME_TIP, false);

        section(ui, t, st::CUSTOM_STATUS.get(lang));
        let now = self.store.now();
        let h24 = self.h24();
        let saved = self.store.account.snapshot().custom_status.clone().filter(|c| !c.is_empty());
        match &saved {
            Some(c) if c.expired(now) => note(ui, t, st::CUSTOM_STATUS_EXPIRED.get(lang)),
            Some(c) if c.expires_at_ms != 0 => note(ui, t, &sf::custom_status_until(lang, time::until(lang, i64::try_from(c.expires_at_ms).unwrap_or(i64::MAX), h24))),
            Some(_) => {}
            None => note(ui, t, st::CUSTOM_STATUS_NONE.get(lang)),
        }
        let d = &mut self.params.status;
        d.sync(&saved);
        super::super::appearance::row(ui, st::CUSTOM_STATUS_TEXT.get(lang), st::CUSTOM_STATUS_TIP.get(lang), |ui| {
            ui.add(egui::TextEdit::singleline(&mut d.text).char_limit(128).desired_width(f32::INFINITY));
        });
        super::super::appearance::row(ui, st::CUSTOM_STATUS_EMOJI.get(lang), st::CUSTOM_STATUS_EMOJI_TIP.get(lang), |ui| {
            ui.add(egui::TextEdit::singleline(&mut d.emoji).char_limit(8).desired_width(f32::INFINITY));
        });
        let exp: Vec<(usize, Text)> = EXPIRY.iter().enumerate().map(|(i, (_, l))| (i, *l)).collect();
        if let Some(i) = choice(ui, lang, "expiration", (st::CUSTOM_STATUS_EXPIRY.get(lang), st::CUSTOM_STATUS_EXPIRY_TIP.get(lang)), &exp, Some(d.expiry), true) {
            d.expiry = i;
        }
        let draft = d.build(saved.as_ref(), now);
        let mut action = None;
        ui.horizontal(|ui| {
            if ui.add_enabled(enabled && !draft.is_empty(), egui::Button::new(st::SAVE.get(lang))).clicked() {
                action = Some(Some(draft.clone()));
            }
            if ui.add_enabled(enabled && saved.is_some(), egui::Button::new(st::CLEAR_STATUS.get(lang))).clicked() {
                action = Some(None);
            }
        });
        if let Some(a) = action {
            self.params.status.sent();
            if let Err(r) = self.store.account.set_custom_status(a, now) {
                self.refused(r);
            }
        }
    }

    pub(super) fn page_privacy(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if !self.proto_header(ui, t) {
            return;
        }
        section(ui, t, st::SECTION_DMS.get(lang));
        self.synced_toggle(ui, Key::GuildDmsRestricted, st::GUILD_DMS, st::GUILD_DMS_TIP, true);
        self.synced_toggle(ui, Key::MessageRequestsRestricted, st::MESSAGE_REQUESTS, st::MESSAGE_REQUESTS_TIP, false);
        self.synced_enum(ui, Key::DmSpamFilter, st::SPAM_FILTER, st::SPAM_FILTER_TIP, &SPAM);

        section(ui, t, st::FRIEND_REQUESTS.get(lang));
        note(ui, t, st::FRIEND_REQUESTS_TIP.get(lang));
        let enabled = !self.store.account.proto_busy();
        let flags = match self.store.account.value(Key::FriendSources) {
            Some(SValue::U32(v)) => Some(*v),
            _ => None,
        };
        for (bit, label, tip) in [
            (FRIEND_NO_RELATION, st::FRIENDS_EVERYONE, st::FRIENDS_EVERYONE_TIP),
            (FRIEND_MUTUAL_FRIENDS, st::FRIENDS_OF_FRIENDS, st::FRIENDS_OF_FRIENDS_TIP),
            (FRIEND_MUTUAL_GUILDS, st::FRIENDS_SERVER_MEMBERS, st::FRIENDS_SERVER_MEMBERS_TIP),
        ] {
            if let Some(on) = toggle(ui, label.get(lang), tip.get(lang), flags.map(|f| f & bit != 0), enabled)
                && let Some(f) = flags
            {
                self.write_setting(Key::FriendSources, SValue::U32(friend_flags(f, bit, on)));
            }
        }

        section(ui, t, st::EXPLICIT_FILTER.get(lang));
        note(ui, t, st::EXPLICIT_FILTER_TIP.get(lang));
        self.synced_enum(ui, Key::ExplicitFriendDm, st::EXPLICIT_FRIENDS, st::EXPLICIT_FRIENDS_TIP, &REDACTION);
        self.synced_enum(ui, Key::ExplicitOtherDm, st::EXPLICIT_OTHERS, st::EXPLICIT_OTHERS_TIP, &REDACTION);
        self.synced_enum(ui, Key::ExplicitGuilds, st::EXPLICIT_GUILDS, st::EXPLICIT_GUILDS_TIP, &REDACTION);
    }

    pub(super) fn page_text_images(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if !self.proto_header(ui, t) {
            return;
        }
        section(ui, t, st::SECTION_MEDIA.get(lang));
        self.synced_toggle(ui, Key::InlineAttachments, st::INLINE_ATTACHMENTS, st::INLINE_ATTACHMENTS_TIP, false);
        self.synced_toggle(ui, Key::InlineEmbeds, st::INLINE_EMBEDS, st::INLINE_EMBEDS_TIP, false);
        self.synced_toggle(ui, Key::ImageDescriptions, st::IMAGE_DESCRIPTIONS, st::IMAGE_DESCRIPTIONS_TIP, false);
        section(ui, t, st::SECTION_EMBEDS.get(lang));
        self.synced_toggle(ui, Key::RenderEmbeds, st::RENDER_EMBEDS, st::RENDER_EMBEDS_TIP, false);
        section(ui, t, st::SECTION_EMOJI.get(lang));
        self.synced_toggle(ui, Key::RenderReactions, st::RENDER_REACTIONS, st::RENDER_REACTIONS_TIP, false);
        self.synced_toggle(ui, Key::AnimateEmoji, st::ANIMATE_EMOJI, st::ANIMATE_EMOJI_TIP, false);
        self.synced_toggle(ui, Key::GifAutoPlay, st::GIF_AUTOPLAY, st::GIF_AUTOPLAY_TIP, false);
        self.synced_toggle(ui, Key::ConvertEmoticons, st::CONVERT_EMOTICONS, st::CONVERT_EMOTICONS_TIP, false);
        let enabled = !self.store.account.proto_busy();
        let current = match self.store.account.value(Key::AnimateStickers) {
            Some(SValue::U32(v)) => Some(*v),
            _ => None,
        };
        if let Some(v) = choice(ui, lang, "autocollants", (st::ANIMATE_STICKERS.get(lang), st::ANIMATE_STICKERS_TIP.get(lang)), &STICKERS, current, enabled) {
            self.write_setting(Key::AnimateStickers, SValue::U32(v));
        }
        section(ui, t, st::SECTION_COMMANDS.get(lang));
        self.synced_toggle(ui, Key::CommandSuggestions, st::COMMAND_SUGGESTIONS, st::COMMAND_SUGGESTIONS_TIP, false);
        self.synced_toggle(ui, Key::TtsCommand, st::TTS, st::TTS_TIP, false);
    }

    pub(super) fn page_notifications(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        section(ui, t, st::SERVER_NOTIFICATIONS.get(lang));
        note(ui, t, st::SERVER_NOTIFICATIONS_TIP.get(lang));
        section(ui, t, st::SECTION_ACCOUNT_NOTIFICATIONS.get(lang));
        if !self.proto_header(ui, t) {
            return;
        }
        self.synced_toggle(ui, Key::InAppNotifications, st::IN_APP_NOTIFICATIONS, st::IN_APP_NOTIFICATIONS_TIP, false);
        self.synced_toggle(ui, Key::BurstReactionNotify, st::BURST_REACTIONS, st::BURST_REACTIONS_TIP, false);
        self.synced_toggle(ui, Key::GoLiveNotify, st::GO_LIVE, st::GO_LIVE_TIP, false);
    }

    pub(super) fn page_language(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        // Langue de Ferrocord : la même liste que dans la page Apparence.
        let current = i18n::choice();
        let options: Vec<(Choice, Text)> = Choice::ALL.iter().map(|c| (*c, choice_text(*c))).collect();
        if let Some(c) = choice(ui, lang, "langue_ferrocord", (crate::i18n::t::LANGUAGE.get(lang), crate::i18n::t::LANGUAGE_DETAIL.get(lang)), &options, Some(current), true) {
            i18n::set_language(c);
        }
        section(ui, t, st::ACCOUNT_LOCALE.get(lang));
        if !self.proto_header(ui, t) {
            return;
        }
        let enabled = !self.store.account.proto_busy();
        let current = match self.store.account.value(Key::Locale) {
            Some(SValue::Str(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        };
        let shown = current.as_deref().map(|c| LOCALES.iter().find(|(k, _)| *k == c).map_or(c, |(_, n)| n)).unwrap_or(st::UNKNOWN.get(lang)).to_string();
        let mut pick = None;
        super::super::appearance::row(ui, st::ACCOUNT_LOCALE.get(lang), st::ACCOUNT_LOCALE_TIP.get(lang), |ui| {
            ui.add_enabled_ui(enabled && current.is_some(), |ui| {
                egui::ComboBox::from_id_salt("langue_compte").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                    for (code, name) in LOCALES {
                        if ui.selectable_label(current.as_deref() == Some(code), name).clicked() {
                            pick = Some(code);
                        }
                    }
                });
            });
        });
        if let Some(code) = pick {
            self.write_setting(Key::Locale, SValue::Str(code.to_string()));
        }
    }

    pub(super) fn page_folders(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if !self.proto_header(ui, t) {
            return;
        }
        note(ui, t, st::FOLDERS_TIP.get(lang));
        let folders = self.store.account.snapshot().folders.clone();
        if folders.is_empty() {
            ui.label(RichText::new(st::NOTHING.get(lang)).color(t.colors.muted));
        }
        let name_of = |id: u64| {
            self.store.guilds.iter().find(|g| g.id == id).map(|g| super::super::words::guild_name(lang, &g.name).to_string()).unwrap_or_else(|| sf::unknown_server(lang, id))
        };
        for f in &folders {
            let names: Vec<String> = f.guilds.iter().map(|id| name_of(*id)).collect();
            match (&f.name, f.id) {
                // Serveur hors dossier : dossier anonyme d'un seul serveur.
                (None, None) => {
                    ui.label(names.join(", "));
                }
                (name, _) => {
                    let title = name.clone().unwrap_or_else(|| st::UNNAMED_FOLDER.get(lang).to_string());
                    ui.label(RichText::new(title).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                    ui.indent(("dossier", f.id), |ui| {
                        for n in &names {
                            ui.label(n);
                        }
                    });
                }
            }
        }
    }

    /// Thème et mode compact du compte Discord : affichés, et applicables à Ferrocord.
    pub(super) fn discord_appearance(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        super::super::appearance::heading(ui, t, st::DISCORD_THEME_SECTION.get(lang));
        if !self.proto_header(ui, t) {
            return;
        }
        let theme = match self.store.account.value(Key::Theme) {
            Some(SValue::Enum(v)) => *v,
            _ => 0,
        };
        let compact = self.store.account.bool(Key::MessageCompact).unwrap_or(false);
        let theme_name = THEMES.iter().find(|(v, _)| *v == theme).map_or(st::UNKNOWN, |(_, l)| *l);
        value_row(ui, st::DISCORD_THEME.get(lang), st::DISCORD_THEME_TIP.get(lang), theme_name.get(lang));
        value_row(ui, st::COMPACT.get(lang), st::COMPACT_TIP.get(lang), if compact { st::YES.get(lang) } else { st::NO.get(lang) });
        let preset = preset_for(theme);
        let mode = if compact { MessageMode::Compact } else { MessageMode::Grouped };
        let same = self.look.edited.base == preset && self.look.edited.layout.messages == mode;
        let apply = ui.add_enabled(!same, egui::Button::new(st::APPLY_TO_FERROCORD.get(lang))).on_hover_text(sf::apply_theme_tip(lang, preset.label(lang)));
        if apply.clicked() {
            let mut tokens = preset.tokens();
            tokens.layout.messages = mode;
            self.look.edited = tokens;
        }
    }
}

fn choice_text(c: Choice) -> Text {
    match c {
        Choice::System => crate::i18n::t::LANGUAGE_SYSTEM,
        Choice::Fr => crate::i18n::t::LANGUAGE_FR,
        Choice::En => crate::i18n::t::LANGUAGE_EN,
    }
}

/// Sources de demandes d'amis après un clic : « tout le monde » exige les
/// deux autres (docs.discord.food), et retirer l'une d'elles le retire.
pub(super) fn friend_flags(current: u32, bit: u32, on: bool) -> u32 {
    let both = FRIEND_MUTUAL_FRIENDS | FRIEND_MUTUAL_GUILDS;
    match (bit, on) {
        (FRIEND_NO_RELATION, true) => current | FRIEND_NO_RELATION | both,
        (_, true) => current | bit,
        (_, false) if bit != FRIEND_NO_RELATION => current & !bit & !FRIEND_NO_RELATION,
        _ => current & !bit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// « Aujourd'hui » : le statut s'arrête au prochain minuit local, dans les 25 heures.
    #[test]
    fn today_clears_the_status_at_local_midnight() {
        use chrono::{TimeZone, Timelike};
        let now = 1_700_000_000_000_i64;
        let d = StatusDraft { text: "x".into(), expiry: EXPIRY.len() - 1, ..StatusDraft::default() };
        let end = i64::try_from(d.build(None, now).expires_at_ms).unwrap();
        assert!(end > now && end - now <= 25 * 3_600_000, "{end}");
        let at = chrono::Local.timestamp_millis_opt(end).single().unwrap();
        assert_eq!((at.hour(), at.minute(), at.second()), (0, 0, 0));
    }

    #[test]
    fn friend_sources_stay_consistent() {
        let all = FRIEND_NO_RELATION | FRIEND_MUTUAL_FRIENDS | FRIEND_MUTUAL_GUILDS;
        assert_eq!(friend_flags(0, FRIEND_NO_RELATION, true), all);
        assert_eq!(friend_flags(all, FRIEND_MUTUAL_GUILDS, false), FRIEND_MUTUAL_FRIENDS);
        assert_eq!(friend_flags(all, FRIEND_NO_RELATION, false), FRIEND_MUTUAL_FRIENDS | FRIEND_MUTUAL_GUILDS);
        assert_eq!(friend_flags(0, FRIEND_MUTUAL_FRIENDS, true), FRIEND_MUTUAL_FRIENDS);
    }

    #[test]
    fn discord_theme_maps_to_a_preset() {
        assert_eq!(preset_for(2), Preset::Clair);
        assert_eq!(preset_for(4), Preset::Onyx);
        assert_eq!(preset_for(1), Preset::Sombre);
        assert_eq!(preset_for(3), Preset::Sombre);
        assert!(LOCALES.iter().any(|(c, _)| *c == "fr"));
    }
}
