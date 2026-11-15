//! « Vue d'ensemble » de « Modifier le salon » : nom, sujet, mode lent,
//! salon d'annonces, limite d'âge, archivage des fils, étiquettes et réaction
//! par défaut d'un forum ; débit, limite d'utilisateurs, région et qualité
//! vidéo d'un salon vocal. Le brouillon part de ce que Discord a donné
//! (`Details`) ; seul ce qui change est envoyé.

use super::{unsaved_bar, App};
use crate::i18n::table::chadmin::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::admin::channels::{
    ChanAction, Details, Patch, Reaction, Region, Tag, ARCHIVE_DURATIONS, BITRATE_MIN, FORUM_TOPIC_MAX, KIND_STAGE, KIND_VOICE, NAME_MAX, TAGS_MAX, TAG_NAME_MAX, TOPIC_MAX, USER_LIMIT_MAX,
};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Mode lent proposé (secondes), comme l'application officielle.
const SLOWMODES: [u32; 14] = [0, 5, 10, 15, 30, 60, 120, 300, 600, 900, 1800, 3600, 7200, 21_600];

const TEXT: u8 = 0;
const CATEGORY: u8 = 4;
const ANNOUNCEMENT: u8 = 5;
const FORUM: u8 = 15;
const MEDIA: u8 = 16;
/// Débit d'un salon vocal dont Discord ne dit rien (sa valeur par défaut).
const DEFAULT_BITRATE: u32 = 64_000;
/// Qualité vidéo : automatique (1) ou toujours 720p (2).
const VIDEO_AUTO: u8 = 1;
const VIDEO_FULL: u8 = 2;

/// Durée en mots : secondes, minutes, heures, jours ou semaines entiers.
pub(super) fn duration(lang: Lang, secs: u64) -> String {
    match secs {
        0 => tx::OFF.get(lang).to_string(),
        s if s % 604_800 == 0 => p::weeks(lang, s / 604_800),
        s if s % 86_400 == 0 && s > 86_400 => p::days(lang, s / 86_400),
        s if s % 3600 == 0 => p::hours(lang, s / 3600),
        s if s % 60 == 0 => p::minutes(lang, s / 60),
        s => p::seconds(lang, s),
    }
}

pub(super) struct Draft {
    base: Details,
    name: String,
    topic: String,
    nsfw: bool,
    slowmode: u32,
    auto_archive: Option<u32>,
    announcement: bool,
    tags: Vec<Tag>,
    reaction: Option<Reaction>,
    reaction_text: String,
    bitrate: u32,
    user_limit: u32,
    region: Option<String>,
    video_quality: u8,
}

impl Draft {
    pub(super) fn new(d: &Details) -> Self {
        Draft {
            base: d.clone(),
            name: d.name.clone(),
            topic: d.topic.clone(),
            nsfw: d.nsfw,
            slowmode: d.slowmode,
            auto_archive: d.auto_archive,
            announcement: d.kind == ANNOUNCEMENT,
            tags: d.tags.clone(),
            reaction: d.reaction.clone(),
            reaction_text: match &d.reaction {
                Some(Reaction::Unicode(s)) => s.clone(),
                _ => String::new(),
            },
            bitrate: d.bitrate.unwrap_or(DEFAULT_BITRATE),
            user_limit: d.user_limit.unwrap_or(0),
            region: d.rtc_region.clone(),
            video_quality: d.video_quality.unwrap_or(VIDEO_AUTO),
        }
    }

    #[cfg(test)]
    pub(super) fn set_slowmode(&mut self, s: u32) {
        self.slowmode = s;
    }

    /// Ce qui diffère de ce que Discord a donné.
    pub(super) fn patch(&self) -> Patch {
        let b = &self.base;
        let name = self.name.trim();
        let mut p = Patch::default();
        if name != b.name {
            p.name = Some(name.to_string());
        }
        if matches!(b.kind, TEXT | ANNOUNCEMENT) && self.announcement != (b.kind == ANNOUNCEMENT) {
            p.kind = Some(if self.announcement { ANNOUNCEMENT } else { TEXT });
        }
        if self.topic != b.topic {
            p.topic = Some(self.topic.clone());
        }
        if self.nsfw != b.nsfw {
            p.nsfw = Some(self.nsfw);
        }
        if self.slowmode != b.slowmode {
            p.slowmode = Some(self.slowmode);
        }
        if self.auto_archive.is_some() && self.auto_archive != b.auto_archive {
            p.auto_archive = self.auto_archive;
        }
        if self.tags != b.tags {
            p.tags = Some(self.tags.clone());
        }
        if self.reaction != b.reaction {
            p.reaction = Some(self.reaction.clone());
        }
        if matches!(b.kind, KIND_VOICE | KIND_STAGE) {
            if self.bitrate != b.bitrate.unwrap_or(DEFAULT_BITRATE) {
                p.bitrate = Some(self.bitrate);
            }
            if self.region != b.rtc_region {
                p.rtc_region = Some(self.region.clone());
            }
        }
        if b.kind == KIND_VOICE {
            if self.user_limit != b.user_limit.unwrap_or(0) {
                p.user_limit = Some(self.user_limit);
            }
            if self.video_quality != b.video_quality.unwrap_or(VIDEO_AUTO) {
                p.video_quality = Some(self.video_quality);
            }
        }
        p
    }
}

impl App {
    pub(super) fn channel_overview(&mut self, ui: &mut egui::Ui, t: &Tokens, channel: u64) {
        let lang = self.lang;
        let loading = self.store.chan_admin.pending(ChanAction::Load);
        let Some(details) = self.store.chan_admin.details_of(channel).cloned() else {
            if loading {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(tx::LOADING.get(lang));
                });
            } else if ui.button(tx::RETRY.get(lang)).clicked() {
                self.store.load_channel(channel);
            }
            return;
        };
        let bitrate_max = self.store.voice_bitrate_max(channel);
        let regions = self.store.chan_admin.regions.clone();
        let Some(e) = self.chan_ui.edit.as_mut() else { return };
        // Brouillon refait quand Discord en dit plus (CHANNEL_UPDATE), s'il n'a pas été touché.
        if e.overview.as_ref().is_none_or(|d| d.base != details && d.patch().is_empty()) {
            e.overview = Some(Draft::new(&details));
        }
        let Some(d) = e.overview.as_mut() else { return };
        let k = d.base.kind;
        let category = k == CATEGORY;
        let textual = matches!(k, TEXT | ANNOUNCEMENT | FORUM | MEDIA);
        let forum = matches!(k, FORUM | MEDIA);

        super::super::settings::section(ui, t, if category { tx::CATEGORY_NAME.get(lang) } else { tx::CHANNEL_NAME.get(lang) });
        ui.add(egui::TextEdit::singleline(&mut d.name).char_limit(NAME_MAX).desired_width(f32::INFINITY));
        if textual {
            super::super::settings::section(ui, t, if forum { tx::GUIDELINES.get(lang) } else { tx::TOPIC.get(lang) });
            let max = if forum { FORUM_TOPIC_MAX } else { TOPIC_MAX };
            ui.add(egui::TextEdit::multiline(&mut d.topic).char_limit(max).desired_rows(3).hint_text(tx::TOPIC_HINT.get(lang)).desired_width(f32::INFINITY));
            ui.label(RichText::new(format!("{}/{max}", d.topic.chars().count())).small().color(t.colors.faint));

            super::super::settings::section(ui, t, tx::SLOWMODE.get(lang));
            egui::ComboBox::from_id_salt("salon-mode-lent").selected_text(duration(lang, u64::from(d.slowmode))).show_ui(ui, |ui| {
                for s in SLOWMODES {
                    ui.selectable_value(&mut d.slowmode, s, duration(lang, u64::from(s)));
                }
            });
            super::super::settings::note(ui, t, tx::SLOWMODE_TIP.get(lang));
        }
        if matches!(k, TEXT | ANNOUNCEMENT) {
            ui.add_space(t.spacing.category_gap);
            crate::ui::settings::shell::switch_label(ui, &mut d.announcement, RichText::new(tx::ANNOUNCEMENT.get(lang)).family(crate::ui::fonts::demi()).strong());
            super::super::settings::note(ui, t, tx::ANNOUNCEMENT_TIP.get(lang));
        }
        if !category {
            ui.add_space(t.spacing.category_gap);
            crate::ui::settings::shell::switch_label(ui, &mut d.nsfw, RichText::new(tx::NSFW.get(lang)).family(crate::ui::fonts::demi()).strong());
            super::super::settings::note(ui, t, tx::NSFW_TIP.get(lang));
        }
        if textual {
            super::super::settings::section(ui, t, tx::ARCHIVE.get(lang));
            let shown = d.auto_archive.unwrap_or(1440);
            let mut pick = shown;
            egui::ComboBox::from_id_salt("salon-archivage").selected_text(duration(lang, u64::from(shown) * 60)).show_ui(ui, |ui| {
                for m in ARCHIVE_DURATIONS {
                    ui.selectable_value(&mut pick, m, duration(lang, u64::from(m) * 60));
                }
            });
            if pick != shown {
                d.auto_archive = Some(pick);
            }
            super::super::settings::note(ui, t, tx::ARCHIVE_TIP.get(lang));
        }
        if forum {
            forum_settings(ui, t, lang, d);
        }
        if matches!(k, KIND_VOICE | KIND_STAGE) {
            voice_settings(ui, t, lang, d, bitrate_max, regions.as_deref());
        }
    }

    /// Bandeau « modifications non enregistrées » de la page, hors du défilement.
    pub(super) fn overview_bar(&mut self, ui: &mut egui::Ui, t: &Tokens, channel: u64) {
        let lang = self.lang;
        let busy = self.store.chan_admin.pending(ChanAction::Save);
        let Some(details) = self.store.chan_admin.details_of(channel).cloned() else { return };
        let Some(patch) = self.chan_ui.edit.as_ref().and_then(|e| e.overview.as_ref()).map(Draft::patch) else { return };
        if patch.is_empty() {
            return;
        }
        let (reset, save) = unsaved_bar(ui, t, lang, busy, tx::SAVE.get(lang));
        if reset && let Some(e) = self.chan_ui.edit.as_mut() {
            e.overview = Some(Draft::new(&details));
        }
        if save {
            self.store.save_channel(channel, patch);
        }
    }
}

/// Débit, limite d'utilisateurs, région et qualité vidéo d'un salon vocal
/// (une scène n'a ni limite réglable ici, ni qualité vidéo).
fn voice_settings(ui: &mut egui::Ui, t: &Tokens, lang: Lang, d: &mut Draft, bitrate_max: u32, regions: Option<&[Region]>) {
    let stage = d.base.kind == KIND_STAGE;
    super::super::settings::section(ui, t, tx::BITRATE.get(lang));
    let mut kbps = d.bitrate / 1000;
    // Un débit déjà au-dessus du plafond (boost perdu) reste affichable.
    let top = (bitrate_max / 1000).max(kbps);
    ui.spacing_mut().slider_width = ui.available_width() * 0.6;
    if ui.add(egui::Slider::new(&mut kbps, BITRATE_MIN / 1000..=top).custom_formatter(move |v, _| f::kbps(lang, v as u64))).changed() {
        d.bitrate = kbps * 1000;
    }
    super::super::settings::note(ui, t, tx::BITRATE_TIP.get(lang));

    if !stage {
        super::super::settings::section(ui, t, tx::USER_LIMIT.get(lang));
        let no_limit = tx::NO_LIMIT.get(lang);
        ui.add(egui::Slider::new(&mut d.user_limit, 0..=USER_LIMIT_MAX).custom_formatter(move |v, _| if v < 1.0 { no_limit.to_string() } else { p::users(lang, v as u64) }));
        super::super::settings::note(ui, t, tx::USER_LIMIT_TIP.get(lang));
    }

    super::super::settings::section(ui, t, tx::REGION.get(lang));
    let auto = tx::REGION_AUTO.get(lang);
    let name = |id: &str| regions.and_then(|l| l.iter().find(|r| r.id == id)).map_or(id, |r| r.name.as_str()).to_string();
    let shown = d.region.as_deref().map_or_else(|| auto.to_string(), name);
    egui::ComboBox::from_id_salt("salon-region").selected_text(shown).show_ui(ui, |ui| {
        ui.selectable_value(&mut d.region, None, auto);
        for r in regions.unwrap_or_default() {
            ui.selectable_value(&mut d.region, Some(r.id.clone()), r.name.as_str());
        }
        if regions.is_none() {
            ui.label(RichText::new(tx::LOADING.get(lang)).small().color(t.colors.faint));
        }
    });
    super::super::settings::note(ui, t, tx::REGION_TIP.get(lang));

    if !stage {
        super::super::settings::section(ui, t, tx::VIDEO_QUALITY.get(lang));
        ui.horizontal(|ui| {
            // De l'air entre les deux choix (même écart que ceux de l'élagage).
            ui.spacing_mut().item_spacing.x = 2.0 * t.spacing.item_x;
            ui.radio_value(&mut d.video_quality, VIDEO_AUTO, tx::VIDEO_AUTO.get(lang));
            ui.radio_value(&mut d.video_quality, VIDEO_FULL, tx::VIDEO_FULL.get(lang));
        });
        super::super::settings::note(ui, t, tx::VIDEO_QUALITY_TIP.get(lang));
    }
}

/// Étiquettes et réaction par défaut d'un forum ou d'un salon média.
fn forum_settings(ui: &mut egui::Ui, t: &Tokens, lang: Lang, d: &mut Draft) {
    super::super::settings::section(ui, t, tx::TAGS.get(lang));
    super::super::settings::note(ui, t, tx::TAGS_TIP.get(lang));
    let mut remove = None;
    for (i, tag) in d.tags.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut tag.name).char_limit(TAG_NAME_MAX).hint_text(tx::TAG_NAME.get(lang)).desired_width(ui.available_width() * 0.4));
            if tag.emoji_id.is_some() {
                ui.label(RichText::new(tx::CUSTOM_EMOJI.get(lang)).small().color(t.colors.muted));
            } else {
                crate::ui::welcome::emoji_icon(ui, None, tag.emoji_name.as_deref(), t.typography.body * 1.3);
                let mut e = crate::state::emoji::field_text(tag.emoji_name.as_deref().unwrap_or_default());
                if ui.add(egui::TextEdit::singleline(&mut e).char_limit(40).hint_text(tx::TAG_EMOJI.get(lang)).desired_width(t.layout.small_avatar * 3.0)).changed() {
                    tag.emoji_name = crate::state::emoji::from_field(e.trim());
                }
            }
            crate::ui::settings::shell::switch_label(ui, &mut tag.moderated, tx::TAG_MODERATED.get(lang));
            if ui.button(tx::REMOVE.get(lang)).clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        d.tags.remove(i);
    }
    if d.tags.len() < TAGS_MAX && ui.button(tx::ADD_TAG.get(lang)).clicked() {
        d.tags.push(Tag::default());
    }

    super::super::settings::section(ui, t, tx::DEFAULT_REACTION.get(lang));
    ui.horizontal(|ui| match &d.reaction {
        Some(Reaction::Custom(_)) => {
            ui.label(tx::CUSTOM_EMOJI.get(lang));
            if ui.button(tx::REMOVE.get(lang)).clicked() {
                d.reaction = None;
            }
        }
        _ => {
            if ui.add(egui::TextEdit::singleline(&mut d.reaction_text).char_limit(8).desired_width(t.layout.small_avatar * 3.0)).changed() {
                let s = d.reaction_text.trim();
                d.reaction = (!s.is_empty()).then(|| Reaction::Unicode(s.to_string()));
            }
        }
    });
    super::super::settings::note(ui, t, tx::DEFAULT_REACTION_TIP.get(lang));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_and_patch() {
        assert_eq!(duration(Lang::Fr, 0), "Désactivé");
        assert_eq!(duration(Lang::Fr, 5), "5 secondes");
        assert_eq!(duration(Lang::En, 120), "2 minutes");
        assert_eq!(duration(Lang::Fr, 21_600), "6 heures");
        assert_eq!(duration(Lang::Fr, 86_400), "24 heures");
        assert_eq!(duration(Lang::Fr, 3 * 86_400), "3 jours");
        assert_eq!(duration(Lang::En, 604_800), "1 week");

        let base = Details { id: 1, kind: TEXT, name: "général".into(), topic: "x".into(), ..Details::default() };
        let mut d = Draft::new(&base);
        assert!(d.patch().is_empty());
        d.name = "  général ".into();
        assert!(d.patch().is_empty(), "espaces aux bords ignorés");
        d.announcement = true;
        d.auto_archive = Some(60);
        let p = d.patch();
        assert_eq!((p.kind, p.auto_archive, p.name), (Some(ANNOUNCEMENT), Some(60), None));
        // Une catégorie ne change jamais de type.
        let mut c = Draft::new(&Details { kind: CATEGORY, ..base.clone() });
        c.announcement = true;
        assert!(c.patch().is_empty());
    }

    #[test]
    fn voice_patch_only_sends_what_changed() {
        let base = Details { id: 2, kind: KIND_VOICE, name: "Vocal".into(), bitrate: Some(64_000), user_limit: Some(0), rtc_region: None, video_quality: None, ..Details::default() };
        let mut d = Draft::new(&base);
        assert!(d.patch().is_empty());
        d.bitrate = 96_000;
        d.user_limit = 5;
        assert_eq!(d.patch(), Patch { bitrate: Some(96_000), user_limit: Some(5), ..Patch::default() });
        let mut d = Draft::new(&Details { rtc_region: Some("rotterdam".into()), video_quality: Some(VIDEO_FULL), ..base.clone() });
        d.region = None;
        d.video_quality = VIDEO_AUTO;
        assert_eq!(d.patch(), Patch { rtc_region: Some(None), video_quality: Some(VIDEO_AUTO), ..Patch::default() });
        // Scène : ni limite ni qualité vidéo ; salon textuel : rien de vocal.
        let mut s = Draft::new(&Details { kind: KIND_STAGE, ..base.clone() });
        s.user_limit = 9;
        s.video_quality = VIDEO_FULL;
        s.bitrate = 32_000;
        assert_eq!(s.patch(), Patch { bitrate: Some(32_000), ..Patch::default() });
        let mut text = Draft::new(&Details { kind: TEXT, ..base });
        text.bitrate = 8000;
        assert!(text.patch().is_empty());
    }
}
