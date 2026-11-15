//! Page « Statut des boosts » : niveau, nombre de boosts, barre de
//! progression vers le niveau suivant, avantages débloqués, boosters
//! connus. Aucun achat : « Booster » ouvre l'application officielle.

use super::super::attachments::file_size;
use super::super::settings::{note, section, toggle, DISCORD_WEB};
use super::App;
use crate::i18n::table::admin::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::admin::guild::{perks, GuildEdit, Load, Perks, BOOSTS_FOR_TIER};
use crate::state::perms::Permissions;
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Avantages d'un niveau en mots.
fn perk_lines(lang: Lang, k: &Perks) -> Vec<String> {
    let mut v = vec![
        f::perk_emoji(lang, k.emoji_slots),
        f::perk_stickers(lang, k.sticker_slots),
        f::perk_soundboard(lang, k.soundboard_slots),
        f::perk_audio(lang, k.bitrate_kbps),
        f::perk_upload(lang, file_size(lang, k.upload)),
        f::perk_stream(lang, k.stream.0, k.stream.1),
    ];
    for (on, text) in [
        (k.animated_icon, tx::PERK_ANIMATED_ICON),
        (k.invite_splash, tx::PERK_SPLASH),
        (k.banner, tx::PERK_BANNER),
        (k.role_icons, tx::PERK_ROLE_ICONS),
        (k.vanity_url, tx::PERK_VANITY),
        (k.animated_banner, tx::PERK_ANIMATED_BANNER),
    ] {
        if on {
            v.push(text.get(lang).to_string());
        }
    }
    v
}

/// Niveau en mots.
fn level_text(lang: Lang, tier: u8) -> String {
    if tier == 0 { tx::NO_LEVEL.get(lang).to_string() } else { f::level(lang, tier) }
}

/// Avancée vers le niveau suivant (0 à 1) et boosts qui manquent ; `None` au niveau 3.
fn progress(tier: u8, boosts: u32) -> Option<(f32, u32)> {
    let next = *BOOSTS_FOR_TIER.get(usize::from(tier) + 1)?;
    let from = BOOSTS_FOR_TIER[usize::from(tier).min(3)];
    let span = next.saturating_sub(from).max(1);
    Some((boosts.saturating_sub(from).min(span) as f32 / span as f32, next.saturating_sub(boosts)))
}

impl App {
    pub(super) fn page_guild_boosts(&mut self, ui: &mut egui::Ui, t: &Tokens, guild: u64) {
        let lang = self.lang;
        let Some(g) = self.store.guilds.iter().find(|g| g.id == guild) else { return };
        // La réponse du serveur (fenêtre ouverte) prime sur GUILD_CREATE.
        let profile = self.store.guild_admin.profile.as_ref().and_then(Load::ready).cloned();
        let tier = profile.as_ref().map_or(g.premium_tier, |p| p.premium_tier).min(3);
        let boosts = profile.as_ref().map_or(g.boosts, |p| p.premium_subscription_count);

        ui.horizontal(|ui| {
            ui.label(RichText::new(level_text(lang, tier)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
            ui.label(RichText::new(p::boosts(lang, u64::from(boosts))).color(t.colors.muted));
        });
        match progress(tier, boosts) {
            Some((frac, left)) => {
                ui.add(egui::ProgressBar::new(frac).fill(t.colors.accent));
                note(ui, t, &p::boosts_left(lang, u64::from(left)));
            }
            None => note(ui, t, tx::MAX_LEVEL.get(lang)),
        }
        if ui.add(crate::ui::form::primary(t, tx::BOOST.get(lang))).on_hover_text(tx::BOOST_TIP.get(lang)).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(DISCORD_WEB));
        }
        note(ui, t, tx::BOOST_TIP.get(lang));

        section(ui, t, tx::PERKS.get(lang));
        for line in perk_lines(lang, &perks(tier)) {
            ui.label(format!("• {line}"));
        }
        if tier < 3 {
            section(ui, t, tx::NEXT_PERKS.get(lang));
            let now = perk_lines(lang, &perks(tier));
            for line in perk_lines(lang, &perks(tier + 1)).into_iter().filter(|l| !now.contains(l)) {
                ui.label(RichText::new(format!("• {line}")).color(t.colors.muted));
            }
        }

        // Barre de progression sous le nom du serveur (Modify Guild).
        if let Some(pr) = &profile
            && self.store.may_in_guild(guild, Permissions::MANAGE_GUILD)
        {
            section(ui, t, tx::PROGRESS_BAR.get(lang));
            let saving = self.store.guild_admin.saving;
            if let Some(v) = toggle(ui, tx::PROGRESS_BAR.get(lang), tx::PROGRESS_BAR_TIP.get(lang), Some(pr.premium_progress_bar_enabled), !saving) {
                self.store.save_guild(guild, GuildEdit { premium_progress_bar_enabled: Some(v), ..Default::default() });
            }
        }

        section(ui, t, tx::BOOSTERS.get(lang));
        note(ui, t, tx::BOOSTERS_TIP.get(lang));
        let list = self.store.boosters(guild);
        if list.is_empty() {
            note(ui, t, tx::NO_BOOSTERS.get(lang));
        }
        for (id, since) in list {
            let name = self.store.people.lock().user(id).map_or_else(|| f::user(lang, id), |u| u.display_name().to_string());
            ui.horizontal(|ui| {
                ui.label(name);
                ui.label(RichText::new(f::boosting_since(lang, crate::i18n::time::long_date(lang, since))).color(t.colors.muted));
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn progress_toward_the_next_level() {
        assert_eq!(progress(0, 0), Some((0.0, 2)));
        assert_eq!(progress(0, 1), Some((0.5, 1)));
        assert_eq!(progress(1, 2), Some((0.0, 5)));
        assert_eq!(progress(2, 9), Some((2.0 / 7.0, 5)));
        assert_eq!(progress(3, 20), None);
        // Boosts au-delà du seuil sans niveau mis à jour : barre pleine.
        assert_eq!(progress(0, 5), Some((1.0, 0)));
    }

    #[test]
    fn perks_in_words() {
        let lines = perk_lines(Lang::Fr, &perks(2));
        assert!(lines.iter().any(|l| l.contains("150")), "{lines:?}");
        assert!(lines.iter().any(|l| l == "Bannière du serveur"));
        assert!(!perk_lines(Lang::En, &perks(0)).iter().any(|l| l == "Server banner"));
    }
}
