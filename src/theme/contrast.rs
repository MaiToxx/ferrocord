//! Contraste selon WCAG 2.x (luminance relative des couleurs sRGB).

use eframe::egui::Color32;

/// Contraste minimal WCAG AA du texte courant.
pub const MIN_TEXT: f64 = 4.5;
/// Contraste minimal du texte atténué (et des grands textes).
pub const MIN_MUTED: f64 = 3.0;

/// Luminance relative d'une couleur opaque (0 = noir, 1 = blanc).
pub fn luminance(c: Color32) -> f64 {
    let lin = |v: u8| {
        let s = f64::from(v) / 255.0;
        if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// Rapport de contraste entre deux couleurs opaques, de 1 à 21.
pub fn contrast_ratio(a: Color32, b: Color32) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_ratios() {
        assert!((contrast_ratio(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(Color32::WHITE, Color32::WHITE) - 1.0).abs() < 1e-9);
        // #777777 sur blanc : 4,48 (valeur de référence connue)
        let r = contrast_ratio(Color32::from_gray(0x77), Color32::WHITE);
        assert!((r - 4.48).abs() < 0.01, "{r}");
    }

    /// WCAG 2.1 AA strict (lot C5) : tout jeton qui colore du texte de taille
    /// courante (atténué, discret, erreurs, liens, auteurs compris) atteint
    /// 4,5:1 sur chaque fond où il est posé ; le contour du clavier et les
    /// éléments graphiques, 3:1 (critère 1.4.11).
    #[test]
    fn presets_meet_wcag_aa() {
        use super::super::Preset;
        let mut fails = Vec::new();
        for p in Preset::ALL {
            let t = p.tokens();
            let c = &t.colors;
            let mut check = |what: String, fg: Color32, bg: Color32, min: f64| {
                let r = contrast_ratio(fg, bg);
                if r < min {
                    fails.push(format!("{} {what} : {r:.2} < {min}", p.key()));
                }
            };
            let backgrounds = [("bg0", c.bg0), ("bg1", c.bg1), ("bg2", c.bg2), ("row_hover", c.row_hover), ("selected", c.selected)];
            for (bg_name, bg) in backgrounds {
                for (name, fg) in [("text", c.text), ("text_strong", c.text_strong), ("muted", c.muted), ("faint", c.faint), ("danger", c.danger), ("widget_text", c.widget_text)] {
                    check(format!("{name}/{bg_name}"), fg, bg, MIN_TEXT);
                }
            }
            for (bg_name, bg) in [("bg0", c.bg0), ("bg1", c.bg1), ("bg2", c.bg2), ("bg3", c.bg3)] {
                check(format!("focus/{bg_name}"), c.focus, bg, MIN_MUTED);
            }
            check("link/bg2".into(), c.link, c.bg2, MIN_TEXT);
            check("link/bg1".into(), c.link, c.bg1, MIN_TEXT);
            check("muted/bg3".into(), c.muted, c.bg3, MIN_TEXT);
            check("faint/bg3".into(), c.faint, c.bg3, MIN_TEXT);
            check("button_text/button_bg".into(), c.button_text, c.button_bg, MIN_TEXT);
            check("button_text/control_bg".into(), c.button_text, c.control_bg, MIN_TEXT);
            check("on_accent/accent".into(), c.on_accent, c.accent, MIN_TEXT);
            // Bouton « Supprimer » : texte `on_accent` sur l'aplat rouge.
            check("on_accent/danger_fill".into(), c.on_accent, t.danger_fill(), MIN_TEXT);
            check("text_strong/pill".into(), c.text_strong, c.pill, MIN_TEXT);
            // Lot V1 : salon au repos, menus et infobulles, bouton de succès.
            check("channel_idle/bg1".into(), c.channel_idle, c.bg1, MIN_TEXT);
            check("text/menu_bg".into(), c.text, c.menu_bg, MIN_TEXT);
            check("muted/menu_bg".into(), c.muted, c.menu_bg, MIN_TEXT);
            check("menu_hover".into(), t.menu_hover().1, t.menu_hover().0, MIN_TEXT);
            check("on_accent/success_fill".into(), c.on_accent, t.success_fill(), MIN_TEXT);
            for (i, a) in c.authors.iter().enumerate() {
                check(format!("authors[{i}]/bg2"), *a, c.bg2, MIN_TEXT);
            }
        }
        assert!(fails.is_empty(), "{fails:#?}");
    }
}
