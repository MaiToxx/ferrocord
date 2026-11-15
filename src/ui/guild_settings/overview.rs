//! Page « Vue d'ensemble » : nom, icône, bannière et arrière-plan des
//! invitations (recadrés comme l'avatar, `crate::avatar`), salon système et
//! ses options, salon AFK et délai, notifications par défaut, langue
//! préférée, description, lien personnalisé (lecture). Les changements
//! restent un brouillon jusqu'à « Enregistrer » ; seuls les champs modifiés
//! partent, et la page se remet sur la réponse de Discord.

use super::super::appearance::row;
use super::super::settings::avatar::{crop_view, Picked};
use super::super::settings::{in_discord, note, section};
use super::{loading_or_failed, App};
use crate::avatar::{Avatar, Done};
use crate::i18n::table::admin::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::admin::guild::{sysflags, GuildEdit, GuildProfile, Load, AFK_TIMEOUTS, LOCALES, NOTIFY_ALL, NOTIFY_MENTIONS};
use crate::state::crop::{Crop, ImageKind, MAX_ZOOM};
use crate::state::model::ChannelKind;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use std::sync::Arc;

/// Image du brouillon.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum Img {
    #[default]
    Keep,
    Remove,
    /// Nouvelle image recadrée, en `data:`.
    Set(String),
}

impl Img {
    fn edit(&self) -> Option<Option<String>> {
        match self {
            Img::Keep => None,
            Img::Remove => Some(None),
            Img::Set(d) => Some(Some(d.clone())),
        }
    }
}

#[derive(Default)]
pub(super) struct Draft {
    /// Valeurs de Discord dont le brouillon est parti.
    base: Option<GuildProfile>,
    name: String,
    description: String,
    system_channel: Option<u64>,
    flags: u64,
    afk_channel: Option<u64>,
    afk_timeout: u32,
    notifications: u8,
    locale: String,
    icon: Img,
    banner: Img,
    splash: Img,
    /// Enregistrement demandé : à sa fin, la page se remet sur la réponse.
    sent: bool,
    worker: Option<Avatar>,
    cropping: Option<(ImageKind, Picked)>,
    encoding: Option<ImageKind>,
    error: Option<String>,
}

impl Draft {
    fn reset(&mut self, p: &GuildProfile) {
        self.base = Some(p.clone());
        self.name = p.name.clone();
        self.description = p.description.clone().unwrap_or_default();
        self.system_channel = p.system_channel_id;
        self.flags = p.system_channel_flags;
        self.afk_channel = p.afk_channel_id;
        self.afk_timeout = p.afk_timeout;
        self.notifications = p.default_message_notifications;
        self.locale = p.preferred_locale.clone();
        self.icon = Img::Keep;
        self.banner = Img::Keep;
        self.splash = Img::Keep;
    }

    /// Ce qui diffère de `p`.
    fn edit(&self, p: &GuildProfile) -> GuildEdit {
        fn diff<T: PartialEq + Clone>(new: &T, old: &T) -> Option<T> {
            (new != old).then(|| new.clone())
        }
        let description = Some(self.description.trim().to_string()).filter(|d| !d.is_empty());
        GuildEdit {
            name: diff(&self.name.trim().to_string(), &p.name),
            description: diff(&description, &p.description),
            icon: self.icon.edit(),
            banner: self.banner.edit(),
            splash: self.splash.edit(),
            system_channel_id: diff(&self.system_channel, &p.system_channel_id),
            system_channel_flags: diff(&self.flags, &p.system_channel_flags),
            afk_channel_id: diff(&self.afk_channel, &p.afk_channel_id),
            afk_timeout: diff(&self.afk_timeout, &p.afk_timeout),
            default_message_notifications: diff(&self.notifications, &p.default_message_notifications),
            preferred_locale: diff(&self.locale, &p.preferred_locale),
            premium_progress_bar_enabled: None,
        }
    }

    fn image(&mut self, kind: ImageKind) -> &mut Img {
        match kind {
            ImageKind::GuildBanner => &mut self.banner,
            ImageKind::Splash => &mut self.splash,
            _ => &mut self.icon,
        }
    }

    /// Remet le brouillon sur `p` quand Discord a changé quelque chose :
    /// sans modification en cours, après un enregistrement, ou quand les
    /// modifications sont devenues les valeurs de Discord.
    fn follow(&mut self, p: &GuildProfile, saving: bool) {
        let finished = self.sent && !saving;
        if finished {
            self.sent = false;
        }
        let Some(base) = &self.base else {
            self.reset(p);
            return;
        };
        if base == p {
            return;
        }
        if finished || self.edit(base).is_empty() || self.edit(p).is_empty() {
            self.reset(p);
        } else {
            let old = base.clone();
            self.rebase(&old, p);
        }
    }

    /// Les champs que l'utilisateur n'a pas touchés prennent les valeurs
    /// nouvelles de Discord ; les autres gardent la saisie.
    fn rebase(&mut self, old: &GuildProfile, new: &GuildProfile) {
        fn follow<T: PartialEq + Clone>(mine: &mut T, old: &T, new: &T) {
            if mine == old {
                *mine = new.clone();
            }
        }
        if self.name.trim() == old.name {
            self.name = new.name.clone();
        }
        if self.description.trim() == old.description.as_deref().unwrap_or_default() {
            self.description = new.description.clone().unwrap_or_default();
        }
        follow(&mut self.system_channel, &old.system_channel_id, &new.system_channel_id);
        follow(&mut self.flags, &old.system_channel_flags, &new.system_channel_flags);
        follow(&mut self.afk_channel, &old.afk_channel_id, &new.afk_channel_id);
        follow(&mut self.afk_timeout, &old.afk_timeout, &new.afk_timeout);
        follow(&mut self.notifications, &old.default_message_notifications, &new.default_message_notifications);
        follow(&mut self.locale, &old.preferred_locale, &new.preferred_locale);
        self.base = Some(new.clone());
    }
}

fn afk_label(lang: Lang, secs: u32) -> String {
    if secs >= 3600 { f::afk_hour(lang, secs / 3600) } else { f::afk_minutes(lang, secs / 60) }
}

impl App {
    fn overview_worker(&mut self) -> &Avatar {
        let ctx = self.ctx.clone();
        self.guild_settings.overview.worker.get_or_insert_with(|| Avatar::new(Arc::new(move || ctx.request_repaint())))
    }

    /// Image déjà en mémoire mise dans le recadrage (aperçu, tests).
    pub fn stage_guild_image(&mut self, kind: ImageKind, bytes: Vec<u8>) {
        self.overview_worker().add_bytes(kind, "apercu.png".into(), bytes);
    }

    fn overview_poll(&mut self, guild: u64) {
        let Some(w) = &self.guild_settings.overview.worker else { return };
        for done in w.poll() {
            let d = &mut self.guild_settings.overview;
            match done {
                Done::Picked { kind, image, preview } => {
                    let texture = self.ctx.load_texture("ferrocord.serveur.image", preview, egui::TextureOptions::LINEAR);
                    d.cropping = Some((kind, Picked { image, texture, crop: Crop::default() }));
                    d.error = None;
                }
                Done::Unreadable { .. } => d.error = Some(tx::IMAGE_UNREADABLE.get(self.lang).to_string()),
                Done::Encoded { kind, guild: g, data } => {
                    if d.encoding == Some(kind) && g == Some(guild) {
                        d.encoding = None;
                        d.cropping = None;
                        *d.image(kind) = Img::Set(data);
                    }
                }
            }
        }
    }

    pub(super) fn page_guild_overview(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        let h24 = self.h24();
        self.overview_poll(guild);
        let profile = self.store.guild_admin.profile.clone();
        if loading_or_failed(ui, t, lang, h24, profile.as_ref()) {
            return;
        }
        let Some(Load::Ready(p)) = profile else { return };
        let saving = self.store.guild_admin.saving;
        self.guild_settings.overview.follow(&p, saving);
        self.store.load_vanity(guild);
        let channels: Vec<(u64, String)> = self.store.guilds.iter().find(|g| g.id == guild).map(|g| g.channels.iter().filter(|c| c.kind == ChannelKind::Text).map(|c| (c.id, c.name.clone())).collect()).unwrap_or_default();
        let voice_channels: Vec<(u64, String)> = self.store.voice.channels(guild).iter().filter(|c| !c.stage).map(|c| (c.id, c.name.clone())).collect();

        if let Some(n) = p.member_count {
            note(ui, t, &p::members(lang, n));
        }
        // Nom et icône.
        let d = &mut self.guild_settings.overview;
        row(ui, tx::SERVER_NAME.get(lang), tx::NAME_INVALID.get(lang), |ui| {
            crate::ui::form::field(ui, t, &mut d.name, "", crate::state::servers::NAME_MAX);
        });
        let name_ok = (crate::state::servers::NAME_MIN..=crate::state::servers::NAME_MAX).contains(&d.name.trim().chars().count());
        if !name_ok {
            ui.label(RichText::new(tx::NAME_INVALID.get(lang)).small().color(t.colors.danger));
        }
        // L'icône actuelle ouvre la ligne de ses boutons (pas seule sur sa ligne).
        let mut pick = None;
        self.image_row(ui, t, ImageKind::GuildIcon, p.icon.is_some(), true, &mut pick);
        self.image_row(ui, t, ImageKind::GuildBanner, p.banner.is_some(), p.can_banner(), &mut pick);
        self.image_row(ui, t, ImageKind::Splash, p.splash.is_some(), p.can_splash(), &mut pick);
        if let Some(kind) = pick {
            let title = tx::PICK_TITLE.get(lang);
            self.overview_worker().pick(kind, title);
        }
        self.crop_editor(ui, t, guild);

        let d = &mut self.guild_settings.overview;
        section(ui, t, tx::DESCRIPTION.get(lang));
        note(ui, t, tx::DESCRIPTION_TIP.get(lang));
        ui.add(egui::TextEdit::multiline(&mut d.description).desired_rows(3).desired_width(f32::INFINITY));

        // Salon système et ses options (un bit levé coupe le message).
        section(ui, t, tx::SYSTEM_CHANNEL.get(lang));
        row(ui, tx::SYSTEM_CHANNEL.get(lang), tx::SYSTEM_TIP.get(lang), |ui| {
            let shown = d.system_channel.and_then(|c| channels.iter().find(|x| x.0 == c)).map_or_else(|| tx::NO_CHANNEL.get(lang).to_string(), |c| format!("#{}", c.1));
            egui::ComboBox::from_id_salt("serveur_salon_systeme").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                ui.selectable_value(&mut d.system_channel, None, tx::NO_CHANNEL.get(lang));
                for (id, name) in &channels {
                    ui.selectable_value(&mut d.system_channel, Some(*id), format!("#{name}"));
                }
            });
        });
        for (bit, label) in [(sysflags::SUPPRESS_JOIN, tx::SYS_JOIN), (sysflags::SUPPRESS_JOIN_REPLIES, tx::SYS_JOIN_REPLIES), (sysflags::SUPPRESS_BOOSTS, tx::SYS_BOOSTS), (sysflags::SUPPRESS_TIPS, tx::SYS_TIPS)] {
            let mut on = d.flags & bit == 0;
            if crate::ui::settings::shell::switch_label(ui, &mut on, label.get(lang)).changed() {
                d.flags = if on { d.flags & !bit } else { d.flags | bit };
            }
        }

        // Salon AFK : un des salons vocaux du serveur (pas une conférence). Un
        // salon que je ne vois pas garde son identifiant pour nom.
        section(ui, t, tx::AFK_CHANNEL.get(lang));
        note(ui, t, tx::AFK_TIP.get(lang));
        row(ui, tx::AFK_CHANNEL.get(lang), tx::AFK_TIP.get(lang), |ui| {
            let name_of = |c: u64| voice_channels.iter().find(|x| x.0 == c).map_or_else(|| f::voice_channel(lang, c), |x| x.1.clone());
            let shown = d.afk_channel.map_or_else(|| tx::NO_CHANNEL.get(lang).to_string(), name_of);
            egui::ComboBox::from_id_salt("serveur_salon_afk").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                ui.selectable_value(&mut d.afk_channel, None, tx::NO_CHANNEL.get(lang));
                for (id, name) in &voice_channels {
                    ui.selectable_value(&mut d.afk_channel, Some(*id), name);
                }
                if let Some(c) = p.afk_channel_id.filter(|c| !voice_channels.iter().any(|x| x.0 == *c)) {
                    ui.selectable_value(&mut d.afk_channel, Some(c), f::voice_channel(lang, c));
                }
            });
        });
        row(ui, tx::AFK_TIMEOUT.get(lang), "", |ui| {
            egui::ComboBox::from_id_salt("serveur_delai_afk").selected_text(afk_label(lang, d.afk_timeout)).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for s in AFK_TIMEOUTS {
                    ui.selectable_value(&mut d.afk_timeout, s, afk_label(lang, s));
                }
            });
        });

        section(ui, t, tx::NOTIFS.get(lang));
        note(ui, t, tx::NOTIFS_TIP.get(lang));
        ui.radio_value(&mut d.notifications, NOTIFY_ALL, tx::NOTIFY_ALL.get(lang));
        ui.radio_value(&mut d.notifications, NOTIFY_MENTIONS, tx::NOTIFY_MENTIONS.get(lang));

        section(ui, t, tx::LOCALE.get(lang));
        row(ui, tx::LOCALE.get(lang), tx::LOCALE_TIP.get(lang), |ui| {
            egui::ComboBox::from_id_salt("serveur_langue").selected_text(d.locale.clone()).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for l in LOCALES {
                    ui.selectable_value(&mut d.locale, (*l).to_string(), *l);
                }
            });
        });

        self.vanity_section(ui, t, &p);
        self.save_bar(ui, t, guild, &p, name_ok);
    }

    /// Ligne d'une image du serveur : état, « Changer… », « Retirer ».
    fn image_row(&mut self, ui: &mut egui::Ui, t: &Tokens, kind: ImageKind, has: bool, allowed: bool, pick: &mut Option<ImageKind>) {
        let lang = self.lang;
        let (label, tip, locked) = match kind {
            ImageKind::GuildBanner => (tx::BANNER, tx::BANNER_TIP, tx::BANNER_LOCKED),
            ImageKind::Splash => (tx::SPLASH, tx::SPLASH_TIP, tx::SPLASH_LOCKED),
            _ => (tx::ICON, tx::ICON_TIP, tx::ICON_TIP),
        };
        section(ui, t, label.get(lang));
        if !allowed {
            note(ui, t, locked.get(lang));
            return;
        }
        note(ui, t, tip.get(lang));
        let busy = self.guild_settings.overview.worker.as_ref().is_some_and(Avatar::picking) || self.guild_settings.overview.encoding.is_some();
        let d = &mut self.guild_settings.overview;
        let state = match d.image(kind) {
            Img::Set(_) => tx::IMAGE_READY,
            Img::Remove => tx::IMAGE_WILL_GO,
            Img::Keep if has => tx::IMAGE_SET,
            Img::Keep => tx::IMAGE_NONE,
        };
        let side = t.layout.rail_button;
        // L'icône actuelle ouvre sa ligne.
        let guild = self.guild_settings.win.as_ref().map(|w| w.guild_id);
        let shown = (kind == ImageKind::GuildIcon).then(|| self.store.guilds.iter().find(|g| Some(g.id) == guild).and_then(|g| g.icon)).flatten();
        let shown = shown.and_then(|img| match self.images.get(img, side) {
            crate::images::Image::Ready(tex) => Some(tex),
            _ => None,
        });
        ui.horizontal(|ui| {
            if let Some(tex) = shown {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
                ui.painter().add(crate::images::textured(rect, t.r_avatar(side), tex));
            }
            ui.label(RichText::new(state.get(lang)).color(t.colors.muted));
            if ui.add_enabled(Avatar::CAN_PICK && !busy, crate::ui::form::btn(t, tx::CHANGE_IMAGE.get(lang))).clicked() {
                *pick = Some(kind);
            }
            let removable = has || *d.image(kind) != Img::Keep;
            if ui.add_enabled(removable, crate::ui::form::btn(t, tx::REMOVE_IMAGE.get(lang))).clicked() {
                *d.image(kind) = if has { Img::Remove } else { Img::Keep };
            }
        });
    }

    /// Recadrage de l'image choisie, sous les lignes des images.
    fn crop_editor(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        if let Some(e) = &self.guild_settings.overview.error {
            ui.label(RichText::new(e.clone()).color(t.colors.danger));
        }
        let d = &mut self.guild_settings.overview;
        let Some((kind, picked)) = &mut d.cropping else { return };
        let kind = *kind;
        note(ui, t, tx::CROP_TIP.get(lang));
        crop_view(ui, t, picked, kind);
        row(ui, crate::i18n::table::s2::t::ZOOM.get(lang), "", |ui| {
            ui.add(egui::Slider::new(&mut picked.crop.zoom, 1.0..=MAX_ZOOM).fixed_decimals(1));
        });
        let (mut apply, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            apply = ui.add_enabled(d.encoding.is_none(), crate::ui::form::btn(t, tx::USE_CROP.get(lang))).clicked();
            cancel = ui.add(crate::ui::form::btn(t, tx::CANCEL.get(lang))).clicked();
            if d.encoding.is_some() {
                ui.spinner();
            }
        });
        if cancel {
            d.cropping = None;
            d.encoding = None;
        } else if apply {
            let (image, crop) = (picked.image.clone(), picked.crop);
            d.encoding = Some(kind);
            self.overview_worker().encode(image, crop, kind, Some(guild));
        }
    }

    fn vanity_section(&mut self, ui: &mut egui::Ui, t: &Tokens, p: &GuildProfile) {
        let lang = self.lang;
        section(ui, t, tx::VANITY.get(lang));
        if !p.has_feature("VANITY_URL") {
            note(ui, t, tx::VANITY_LOCKED.get(lang));
            return;
        }
        let vanity = self.store.guild_admin.vanity.clone();
        // Le code vient aussi de l'objet Guild : affiché même si la lecture échoue.
        let code = vanity.as_ref().and_then(Load::ready).and_then(|v| v.code.clone()).or_else(|| p.vanity_url_code.clone());
        let mut copy = None;
        match &code {
            Some(c) => {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(f::vanity_link(lang, c)).monospace());
                    if ui.add(crate::ui::form::btn(t, tx::COPY.get(lang))).clicked() {
                        copy = Some(format!("https://discord.gg/{c}"));
                    }
                    if let Some(v) = vanity.as_ref().and_then(Load::ready) {
                        ui.label(RichText::new(p::vanity_uses(lang, u64::from(v.uses))).color(t.colors.muted));
                    }
                });
            }
            None => note(ui, t, tx::VANITY_NONE.get(lang)),
        }
        in_discord(ui, lang, tx::VANITY.get(lang), tx::VANITY_TIP.get(lang));
        if let Some(url) = copy {
            ui.ctx().copy_text(url);
            self.notify(tx::COPIED);
        }
    }

    fn save_bar(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64, p: &GuildProfile, name_ok: bool) {
        let lang = self.lang;
        let saving = self.store.guild_admin.saving;
        let edit = self.guild_settings.overview.edit(p);
        if edit.is_empty() && !saving {
            return;
        }
        ui.add_space(t.spacing.category_gap);
        let (mut reset, mut save) = (false, false);
        egui::Frame::new().fill(t.colors.bg0).corner_radius(t.r_card()).inner_margin(t.card_margin()).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new(tx::UNSAVED.get(lang)).color(t.colors.text_strong));
                reset = ui.add_enabled(!saving, crate::ui::form::btn(t, tx::RESET.get(lang))).clicked();
                let b = crate::ui::form::primary(t, if saving { tx::SAVING } else { tx::SAVE }.get(lang));
                save = ui.add_enabled(!saving && name_ok && !edit.is_empty(), b).clicked();
            });
        });
        if reset {
            self.guild_settings.overview.reset(p);
        } else if save && self.store.save_guild(guild, edit) {
            self.guild_settings.overview.sent = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_sends_only_changes_and_follows_discord() {
        let p = GuildProfile { name: "Club".into(), afk_timeout: 300, system_channel_flags: 1, ..Default::default() };
        let mut d = Draft::default();
        d.follow(&p, false);
        assert!(d.edit(&p).is_empty());
        d.name = " Club 2 ".into();
        d.flags = 0;
        d.icon = Img::Remove;
        let e = d.edit(&p);
        assert_eq!((e.name.as_deref(), e.system_channel_flags, e.icon.clone()), (Some("Club 2"), Some(0), Some(None)));
        assert_eq!(e.afk_timeout, None);
        // GUILD_UPDATE d'un autre champ pendant la saisie : la saisie reste.
        let p2 = GuildProfile { afk_timeout: 60, ..p.clone() };
        d.follow(&p2, false);
        assert_eq!(d.name, " Club 2 ");
        assert_eq!(d.edit(&p2).afk_timeout, None, "champ non touché : il suit Discord");
        assert_eq!(d.edit(&p2).system_channel_flags, Some(0));
        // Enregistré : la réponse de Discord remplace le brouillon.
        d.sent = true;
        let p3 = GuildProfile { name: "Club 2".into(), system_channel_flags: 0, ..p2.clone() };
        d.follow(&p3, false);
        assert!(d.edit(&p3).is_empty());
        assert_eq!(d.icon, Img::Keep);
    }

    #[test]
    fn afk_labels() {
        assert_eq!(afk_label(Lang::Fr, 300), "5 min");
        assert_eq!(afk_label(Lang::En, 3600), "1 hr");
    }
}
