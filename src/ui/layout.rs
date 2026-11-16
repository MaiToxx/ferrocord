//! Partage de la largeur de la fenêtre entre le rail, la colonne des salons,
//! la discussion et la liste des membres (phase 10, lot V2).
//!
//! Fenêtre étroite : la liste des membres s'efface d'abord, puis la colonne
//! des salons se resserre. Les réglages ne changent pas : tout revient dès
//! que la place revient.

use super::App;
use crate::theme::CHANNELS_WIDTH;
use eframe::egui;

/// Largeur que la discussion garde avant que les colonnes ne cèdent.
pub(super) const CHAT_MIN: f32 = 420.0;

/// Ce que la largeur disponible impose aux colonnes.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Fit {
    /// Liste des membres effacée faute de place.
    pub members_hidden: bool,
    /// Largeur imposée à la colonne des salons, plus étroite que son réglage.
    pub channels_cap: Option<f32>,
}

/// `avail` : largeur de la fenêtre ; `members` : largeur de la liste si elle
/// est demandée.
pub(super) fn fit(avail: f32, rail: f32, channels: f32, members: Option<f32>) -> Fit {
    let room = avail - rail;
    let cap = (room - CHAT_MIN).max(CHANNELS_WIDTH.min).min(channels);
    Fit { members_hidden: members.is_some_and(|m| room < channels + m + CHAT_MIN), channels_cap: (room < channels + CHAT_MIN).then_some(cap) }
}

/// Place des zones à l'image en cours, d'une colonne à l'autre.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Place {
    pub fit: Fit,
    /// Carte du panneau utilisateur, qui passe sous le rail.
    pub card: Option<egui::Rect>,
    /// Hauteur du pied de la colonne des salons : le rail la laisse libre.
    pub foot: f32,
}

impl App {
    /// Mon statut personnalisé en une ligne, s'il y en a un en cours.
    pub(super) fn user_panel_custom(&self) -> Option<String> {
        let now = self.store.now();
        let c = self.saved_custom_status().filter(|c| !c.expired(now))?;
        // Un emoji personnalisé n'a pas de caractère : le texte seul.
        let emoji = if c.emoji_id == 0 { c.emoji_name.as_str() } else { "" };
        let line = [emoji, c.text.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join(" ");
        (!line.is_empty()).then_some(line)
    }

    /// Nom de l'activité annoncée (sans statut personnalisé, il suit la manette).
    pub(super) fn user_panel_game(&self) -> Option<String> {
        let p = self.store.own_presence()?;
        p.game.map(|g| g.name).or_else(|| p.rich.map(|r| r.activity.name.clone())).filter(|n| !n.is_empty())
    }

    /// Seconde ligne du panneau utilisateur : mon statut personnalisé, sinon
    /// mon statut en mots.
    pub(super) fn user_panel_line(&self) -> Option<String> {
        if let Some(line) = self.user_panel_custom() {
            return Some(line);
        }
        let status = self.own_status().or_else(|| self.store.me.as_ref().and_then(|me| super::profile::known_status(&self.ctx, me.id)))?;
        Some(super::profile::status_text(status).get(self.lang).to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_window_drops_members_then_squeezes_channels() {
        let (rail, channels, members) = (72.0, 240.0, 240.0);
        let wide = rail + channels + members + CHAT_MIN;
        assert_eq!(fit(wide, rail, channels, Some(members)), Fit::default());
        // Juste en dessous : la liste des membres s'efface, la colonne reste.
        assert_eq!(fit(wide - 1.0, rail, channels, Some(members)), Fit { members_hidden: true, channels_cap: None });
        assert_eq!(fit(wide - 1.0, rail, channels, None), Fit::default());
        // Plus étroit encore : la colonne des salons cède, jusqu'à sa borne.
        assert_eq!(fit(rail + 200.0 + CHAT_MIN, rail, channels, None).channels_cap, Some(200.0));
        for avail in [520.0, 0.0, -10.0] {
            assert_eq!(fit(avail, rail, channels, Some(members)), Fit { members_hidden: true, channels_cap: Some(CHANNELS_WIDTH.min) });
        }
        // Colonne déjà plus étroite que la borne (zoom) : jamais élargie.
        assert_eq!(fit(300.0, rail, 120.0, None).channels_cap, Some(120.0));
    }
}
