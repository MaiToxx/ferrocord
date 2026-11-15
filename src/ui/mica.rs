//! Mica et transparence de Windows 11 (lot E5, plugin `mica_windows`) : la
//! matière du système (Mica ou acrylique) derrière le rail et la colonne des
//! salons, sous un voile de la couleur du thème qui garde le texte lisible.
//!
//! La fenêtre doit être créée transparente (`lib::with_backdrop`, d'après
//! l'état du plugin lu au lancement) : le plugin demande donc un redémarrage.
//! Les attributs de DWM sont posés par `platform::backdrop`, une fois, puis
//! seulement quand la matière ou la teinte (thème clair ou sombre) change.
//! Si Windows refuse (avant Windows 11 22H2), rien ne change : les zones
//! restent pleines et le plugin se montre « non disponible ».
//!
//! Plugin coupé, ou fenêtre opaque : une lecture atomique par image, et les
//! fonds sont ceux du thème.

use super::App;
use crate::platform::backdrop::{self, Material};
use crate::plugins::{self, Id};
use crate::theme::Tokens;
use eframe::egui::Color32;

/// Fond le plus clair que la matière sombre du système peut montrer, et le
/// plus sombre de la matière claire : Mica et l'acrylique teintent le papier
/// peint (teinte `#202020` ou `#f3f3f3` à forte opacité), ils ne le laissent
/// pas passer tel quel. Le test de lisibilité part de ces deux bornes.
#[cfg(test)]
const WORST_DARK: Color32 = Color32::from_rgb(0x4d, 0x4d, 0x4d);
#[cfg(test)]
const WORST_LIGHT: Color32 = Color32::from_rgb(0xb4, 0xb4, 0xb4);

#[derive(Default)]
pub(super) struct MicaUi {
    /// Dernière demande faite à Windows : matière et teinte sombre.
    asked: Option<(Material, bool)>,
    /// Windows a accepté : les zones latérales sont peintes en voile.
    active: bool,
    /// Opacité du voile (0 à 255) quand la matière est en service.
    veil: Option<u8>,
    /// Aperçu hors ligne : matière imposée, sans le plugin.
    preview: Option<Material>,
}

/// Le thème est sombre (la matière prend alors sa teinte sombre).
pub(super) fn is_dark(t: &Tokens) -> bool {
    let c = t.colors.bg1;
    u32::from(c.r()) * 299 + u32::from(c.g()) * 587 + u32::from(c.b()) * 114 < 128_000
}

/// Opacité du voile (0 à 255) pour le réglage `percent` (en %).
fn veil_alpha(percent: i64) -> u8 {
    (percent.clamp(plugins::MICA_VEIL_MIN, 100) as f32 * 2.55).round() as u8
}

/// Couleur `c` posée en voile d'opacité `alpha` sur le fond `under`.
#[cfg(test)]
fn veiled(c: Color32, alpha: u8, under: Color32) -> Color32 {
    let a = f32::from(alpha) / 255.0;
    let mix = |fg: u8, bg: u8| (f32::from(fg) * a + f32::from(bg) * (1.0 - a)).round() as u8;
    Color32::from_rgb(mix(c.r(), under.r()), mix(c.g(), under.g()), mix(c.b(), under.b()))
}

impl App {
    /// Aperçu hors ligne : la matière en service sans passer par le plugin
    /// (la fenêtre doit avoir été créée par `with_backdrop`).
    #[doc(hidden)]
    pub fn mica_preview(&mut self, acrylic: bool) {
        self.mica.preview = Some(if acrylic { Material::Acrylic } else { Material::Mica });
    }

    /// Le plugin ne peut pas servir ici (autre système, Windows trop ancien,
    /// ou refus de Windows au lancement) : sa carte le dit.
    pub(super) fn mica_unavailable(&self) -> bool {
        !backdrop::supported() || (self.mica.asked.is_some() && !self.mica.active)
    }

    /// Fond d'une partie posée dans une zone latérale déjà voilée (en-tête,
    /// liste, barre du compte) : rien sous la matière, pour ne pas doubler le voile.
    pub(super) fn mica_inner(&self, color: Color32) -> Color32 {
        if self.mica.veil.is_some() { Color32::TRANSPARENT } else { color }
    }

    /// Fond d'une zone latérale (rail, colonne des salons) : la couleur du
    /// thème, en voile quand la matière du système est derrière.
    pub(super) fn mica_fill(&self, color: Color32) -> Color32 {
        match self.mica.veil {
            Some(a) => Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), a),
            None => color,
        }
    }

    /// Couleur d'effacement de la fenêtre : transparente sous la matière ;
    /// opaque si la fenêtre est transparente mais que Windows a refusé.
    pub(super) fn mica_clear_color(&self) -> Option<[f32; 4]> {
        if !backdrop::transparent() {
            return None;
        }
        Some(if self.mica.active { [0.0; 4] } else { self.theme.colors.bg0.to_normalized_gamma_f32() })
    }

    /// Pose la matière quand elle est voulue et pas encore posée, ou que la
    /// matière ou la teinte a changé. Rien sinon.
    pub(super) fn mica_tick(&mut self) {
        // L'état du plugin est celui du lancement (redémarrage nécessaire).
        if !backdrop::transparent() || !(plugins::on(Id::Mica) || self.mica.preview.is_some()) {
            return;
        }
        let material = self.mica.preview.unwrap_or(if plugins::choice(Id::Mica, plugins::MICA_MATERIAL) == "acrylique" { Material::Acrylic } else { Material::Mica });
        let want = (material, is_dark(&self.theme));
        if self.mica.asked != Some(want) {
            self.mica.asked = Some(want);
            self.mica.active = backdrop::apply(Some(want.0), want.1);
            crate::crash::note_n("fond_systeme.pose", i64::from(self.mica.active));
            self.ctx.request_repaint();
        }
        let veil = self.mica.active.then(|| veil_alpha(plugins::number(Id::Mica, plugins::MICA_VEIL)));
        if veil != self.mica.veil {
            self.mica.veil = veil;
            self.ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{contrast_ratio, Preset, MIN_MUTED, MIN_TEXT};
    use crate::ui::keys::tests::app;

    /// Les cinq préréglages restent lisibles sur le rail et la colonne des
    /// salons, voile au minimum du réglage, sur le fond le plus défavorable
    /// que la matière du système peut montrer.
    #[test]
    fn five_presets_stay_readable_under_the_thinnest_veil() {
        let alpha = veil_alpha(plugins::MICA_VEIL_MIN);
        assert_eq!(veil_alpha(0), alpha, "le voile ne descend pas sous son minimum");
        assert_eq!(veil_alpha(100), 255);
        for p in [Preset::Sombre, Preset::Cendre, Preset::Onyx, Preset::Clair, Preset::Rouille] {
            let t = p.tokens();
            let c = &t.colors;
            let under = if is_dark(&t) { WORST_DARK } else { WORST_LIGHT };
            assert_eq!(is_dark(&t), p != Preset::Clair, "{}", p.key());
            for (zone, bg) in [("rail", c.bg0), ("salons", c.bg1)] {
                let seen = veiled(bg, alpha, under);
                for (name, fg, min) in [("texte", c.text, MIN_TEXT), ("titres", c.text_strong, MIN_TEXT), ("secondaire", c.muted, MIN_MUTED), ("salon au repos", c.channel_idle, 3.0), ("accent", c.accent, 3.0)] {
                    let r = contrast_ratio(fg, seen);
                    assert!(r >= min, "{} : {name} sur {zone} = {r:.2} (minimum {min})", p.key());
                }
            }
        }
    }

    /// Plugin coupé ou fenêtre opaque : rien n'est demandé à Windows, les
    /// fonds sont ceux du thème, la couleur d'effacement est celle d'eframe.
    #[test]
    fn nothing_without_a_transparent_window() {
        let mut a = app();
        a.set_plugin(Id::Mica, true);
        a.mica_preview(false);
        crate::ui::keys::tests::frame(&mut a, Vec::new());
        assert!(!a.mica.active && a.mica.asked.is_none());
        let c = a.theme.colors.bg1;
        assert_eq!(a.mica_fill(c), c);
        assert_eq!(a.mica_inner(c), c);
        assert_eq!(a.mica_clear_color(), None);
        assert!(!plugins::get(Id::Mica).default_on && plugins::get(Id::Mica).restart);
    }

    #[test]
    fn veil_mixes_the_theme_colour_over_the_material() {
        assert_eq!(veiled(Color32::from_rgb(0, 0, 0), 255, Color32::WHITE), Color32::from_rgb(0, 0, 0));
        assert_eq!(veiled(Color32::from_rgb(0, 0, 0), 0, Color32::WHITE), Color32::WHITE);
        assert_eq!(veiled(Color32::from_rgb(100, 100, 100), 128, Color32::from_rgb(200, 200, 200)).r(), 150);
    }
}
