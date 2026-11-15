//! Personnalisation du client (lot PERSO) : section `[apparence]` de
//! `local.toml`. Dégradé et image de fond, polices installées, thème du jour
//! et de la nuit, détails d'affichage. Rien ici ne part chez Discord.
//!
//! Les couleurs et les mesures restent dans `theme.toml` ; ici, ce qui n'est
//! pas un jeton (un chemin, un nom de police, une règle de bascule).

use toml::de::{DeTable, DeValue};

pub const SECTION: &str = "apparence";

/// Bornes des curseurs (le fichier et la page les partagent).
pub const BLUR_MAX: u8 = 40;
pub const ANGLE_MAX: u16 = 360;
/// Part maximale du dégradé ou de l'image sur les fonds, en % : au-delà, le
/// texte ne se lit plus.
pub const STRENGTH_MAX: u8 = 90;

/// Dégradé posé sur les fonds, à la manière des thèmes Nitro, mais local.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gradient {
    pub on: bool,
    /// Deux ou trois couleurs (rouge, vert, bleu).
    pub colors: Vec<[u8; 3]>,
    /// Angle en degrés : 0 de gauche à droite, 90 de haut en bas.
    pub angle: u16,
    /// Part du dégradé sur les fonds, en %.
    pub strength: u8,
}

impl Default for Gradient {
    fn default() -> Self {
        let (_, colors, angle) = GRADIENTS[0];
        Gradient { on: false, colors: colors.to_vec(), angle, strength: 35 }
    }
}

/// Dégradés prêts à l'emploi : (clé, couleurs, angle).
pub const GRADIENTS: &[(&str, &[[u8; 3]], u16)] = &[
    ("crepuscule", &[[0x4a, 0x2d, 0x8f], [0xc2, 0x4b, 0x6e], [0xf2, 0x9a, 0x4b]], 45),
    ("lagon", &[[0x0f, 0x4c, 0x81], [0x1f, 0xa2, 0x9a]], 30),
    ("aurore", &[[0x12, 0x6b, 0x54], [0x2a, 0x4f, 0xa8], [0x7b, 0x3f, 0xb5]], 60),
    ("braise", &[[0x8a, 0x1f, 0x2b], [0xd9, 0x6a, 0x1e]], 20),
    ("foret", &[[0x16, 0x3d, 0x2c], [0x5b, 0x8c, 0x3a]], 90),
    ("nuit", &[[0x0b, 0x10, 0x2e], [0x3a, 0x2a, 0x6b]], 135),
    ("bonbon", &[[0xd9, 0x4f, 0x9b], [0x7a, 0x6c, 0xf0], [0x3f, 0xb8, 0xe6]], 45),
    ("ardoise", &[[0x2b, 0x32, 0x3d], [0x55, 0x60, 0x70]], 90),
];

/// Où l'image de fond se voit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scope {
    /// Derrière la discussion seulement.
    #[default]
    Chat,
    /// Derrière toute l'application.
    App,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Backdrop {
    /// Chemin de l'image sur cet ordinateur ; vide : pas d'image.
    pub path: String,
    pub scope: Scope,
    /// Part de l'image sur les fonds, en %.
    pub opacity: u8,
    /// Rayon du flou, de 0 à [`BLUR_MAX`] (précalculé une fois).
    pub blur: u8,
    /// Assombrissement de l'image, en %.
    pub dim: u8,
}

impl Default for Backdrop {
    fn default() -> Self {
        Backdrop { path: String::new(), scope: Scope::Chat, opacity: 35, blur: 8, dim: 30 }
    }
}

/// Bascule automatique entre deux thèmes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Auto {
    #[default]
    Off,
    /// Suit le mode clair ou sombre de Windows.
    System,
    /// Selon l'heure.
    Clock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Day,
    Night,
}

/// Couleur des noms dans les messages.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NameColor {
    /// Le nom prend la couleur du rôle.
    #[default]
    Role,
    /// Nom en couleur de texte, point de la couleur du rôle devant.
    Dot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LookPrefs {
    pub gradient: Gradient,
    pub backdrop: Backdrop,
    /// Famille installée pour l'interface ; vide : celle du thème.
    pub ui_font: String,
    /// Famille installée pour le code ; vide : Consolas.
    pub code_font: String,
    pub auto: Auto,
    /// Thème du jour et de la nuit : clé d'un préréglage ou nom d'un thème enregistré.
    pub day: String,
    pub night: String,
    /// Heures (0 à 23) où le jour et la nuit commencent.
    pub day_hour: u8,
    pub night_hour: u8,
    pub name_color: NameColor,
    pub hide_rail: bool,
}

impl Default for LookPrefs {
    fn default() -> Self {
        LookPrefs { gradient: Gradient::default(), backdrop: Backdrop::default(), ui_font: String::new(), code_font: String::new(), auto: Auto::Off, day: "clair".into(), night: "sombre".into(), day_hour: 7, night_hour: 19, name_color: NameColor::Role, hide_rail: false }
    }
}

const K_GRADIENT: &str = "degrade";
const K_COLORS: &str = "degrade_couleurs";
const K_ANGLE: &str = "degrade_angle";
const K_STRENGTH: &str = "degrade_force";
const K_IMAGE: &str = "fond_image";
const K_SCOPE: &str = "fond_zone";
const K_OPACITY: &str = "fond_opacite";
const K_BLUR: &str = "fond_flou";
const K_DIM: &str = "fond_assombrissement";
const K_UI_FONT: &str = "police_interface";
const K_CODE_FONT: &str = "police_code";
const K_AUTO: &str = "theme_auto";
const K_DAY: &str = "theme_jour";
const K_NIGHT: &str = "theme_nuit";
const K_DAY_HOUR: &str = "heure_jour";
const K_NIGHT_HOUR: &str = "heure_nuit";
const K_NAMES: &str = "couleur_noms";
const K_RAIL: &str = "masquer_rail";

const SCOPES: [(Scope, &str); 2] = [(Scope::Chat, "discussion"), (Scope::App, "application")];
const AUTOS: [(Auto, &str); 3] = [(Auto::Off, "non"), (Auto::System, "windows"), (Auto::Clock, "heure")];
const NAMES: [(NameColor, &str); 2] = [(NameColor::Role, "role"), (NameColor::Dot, "point")];

fn key_of<T: Copy + PartialEq>(all: &[(T, &'static str)], v: T) -> &'static str {
    all.iter().find(|(x, _)| *x == v).map_or("", |(_, k)| k)
}

fn of_key<T: Copy>(all: &[(T, &'static str)], key: &str) -> Option<T> {
    all.iter().find(|(_, k)| *k == key).map(|(v, _)| *v)
}

fn quoted(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars().filter(|c| !c.is_control()) {
        if matches!(c, '"' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

pub fn fmt_rgb(c: [u8; 3]) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// `#rrggbb` ou `rrggbb`.
pub fn parse_rgb(s: &str) -> Option<[u8; 3]> {
    let hex = s.trim().trim_start_matches('#');
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some([byte(0)?, byte(2)?, byte(4)?])
}

impl LookPrefs {
    /// Jour ou nuit, d'après le mode de Windows (`system_dark`) ou l'heure
    /// locale (`hour`) ; `None` : pas de bascule, ou mode de Windows inconnu.
    pub fn phase(&self, system_dark: Option<bool>, hour: u8) -> Option<Phase> {
        match self.auto {
            Auto::Off => None,
            Auto::System => system_dark.map(|dark| if dark { Phase::Night } else { Phase::Day }),
            Auto::Clock => {
                let (d, n) = (self.day_hour, self.night_hour);
                // Jour de `d` à `n` ; si la nuit commence avant le jour, le jour passe minuit.
                let day = if d <= n { (d..n).contains(&hour) } else { hour >= d || hour < n };
                Some(if day { Phase::Day } else { Phase::Night })
            }
        }
    }

    /// Minutes avant le prochain changement de phase à l'heure (au moins 1).
    pub fn minutes_to_switch(&self, hour: u8, minute: u8) -> u32 {
        let now = u32::from(hour) * 60 + u32::from(minute);
        [self.day_hour, self.night_hour].iter().map(|h| (u32::from(*h) * 60 + 1440 - now - 1) % 1440 + 1).min().unwrap_or(1440)
    }

    pub fn theme_of(&self, p: Phase) -> &str {
        match p {
            Phase::Day => &self.day,
            Phase::Night => &self.night,
        }
    }

    pub fn to_toml(&self) -> String {
        let (g, b) = (&self.gradient, &self.backdrop);
        let colors = g.colors.iter().map(|c| format!("\"{}\"", fmt_rgb(*c))).collect::<Vec<_>>().join(", ");
        format!(
            "[{SECTION}]\n{K_GRADIENT} = {}\n{K_COLORS} = [{colors}]\n{K_ANGLE} = {}\n{K_STRENGTH} = {}\n{K_IMAGE} = {}\n{K_SCOPE} = \"{}\"\n{K_OPACITY} = {}\n{K_BLUR} = {}\n{K_DIM} = {}\n{K_UI_FONT} = {}\n{K_CODE_FONT} = {}\n{K_AUTO} = \"{}\"\n{K_DAY} = {}\n{K_NIGHT} = {}\n{K_DAY_HOUR} = {}\n{K_NIGHT_HOUR} = {}\n{K_NAMES} = \"{}\"\n{K_RAIL} = {}\n",
            g.on,
            g.angle,
            g.strength,
            quoted(&b.path),
            key_of(&SCOPES, b.scope),
            b.opacity,
            b.blur,
            b.dim,
            quoted(&self.ui_font),
            quoted(&self.code_font),
            key_of(&AUTOS, self.auto),
            quoted(&self.day),
            quoted(&self.night),
            self.day_hour,
            self.night_hour,
            key_of(&NAMES, self.name_color),
            self.hide_rail,
        )
    }

    /// Clé inconnue ou valeur illisible : ignorée (valeur par défaut).
    pub fn from_table(t: &DeTable<'_>) -> LookPrefs {
        let mut p = LookPrefs::default();
        let int = |v: &DeValue<'_>, max: i64| v.as_integer().and_then(|i| i64::from_str_radix(i.as_str(), i.radix()).ok()).map(|n| n.clamp(0, max));
        let text = |v: &DeValue<'_>| v.as_str().map(|s| s.chars().filter(|c| !c.is_control()).collect::<String>());
        for (k, v) in t.iter() {
            let v = v.get_ref();
            match k.get_ref().as_ref() {
                K_GRADIENT => p.gradient.on = v.as_bool().unwrap_or(p.gradient.on),
                K_COLORS => {
                    let colors: Vec<[u8; 3]> = v.as_array().map(|a| a.iter().filter_map(|c| c.get_ref().as_str().and_then(parse_rgb)).collect()).unwrap_or_default();
                    if (2..=3).contains(&colors.len()) {
                        p.gradient.colors = colors;
                    }
                }
                K_ANGLE => p.gradient.angle = int(v, i64::from(ANGLE_MAX)).map_or(p.gradient.angle, |n| n as u16),
                K_STRENGTH => p.gradient.strength = int(v, i64::from(STRENGTH_MAX)).map_or(p.gradient.strength, |n| n as u8),
                K_IMAGE => p.backdrop.path = text(v).unwrap_or_default(),
                K_SCOPE => p.backdrop.scope = v.as_str().and_then(|s| of_key(&SCOPES, s)).unwrap_or(p.backdrop.scope),
                K_OPACITY => p.backdrop.opacity = int(v, i64::from(STRENGTH_MAX)).map_or(p.backdrop.opacity, |n| n as u8),
                K_BLUR => p.backdrop.blur = int(v, i64::from(BLUR_MAX)).map_or(p.backdrop.blur, |n| n as u8),
                K_DIM => p.backdrop.dim = int(v, 100).map_or(p.backdrop.dim, |n| n as u8),
                K_UI_FONT => p.ui_font = text(v).unwrap_or_default(),
                K_CODE_FONT => p.code_font = text(v).unwrap_or_default(),
                K_AUTO => p.auto = v.as_str().and_then(|s| of_key(&AUTOS, s)).unwrap_or(p.auto),
                K_DAY => p.day = text(v).unwrap_or(p.day),
                K_NIGHT => p.night = text(v).unwrap_or(p.night),
                K_DAY_HOUR => p.day_hour = int(v, 23).map_or(p.day_hour, |n| n as u8),
                K_NIGHT_HOUR => p.night_hour = int(v, 23).map_or(p.night_hour, |n| n as u8),
                K_NAMES => p.name_color = v.as_str().and_then(|s| of_key(&NAMES, s)).unwrap_or(p.name_color),
                K_RAIL => p.hide_rail = v.as_bool().unwrap_or(p.hide_rail),
                _ => {}
            }
        }
        p
    }

    /// Lit un texte qui ne contient que la section (partie d'un profil d'apparence).
    pub fn parse(text: &str) -> Option<LookPrefs> {
        let root = DeTable::parse(text).ok()?;
        let table = root.get_ref().iter().find(|(k, _)| k.get_ref() == SECTION)?.1.get_ref().as_table()?;
        Some(LookPrefs::from_table(table))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_damaged_values() {
        let p = LookPrefs {
            gradient: Gradient { on: true, colors: vec![[1, 2, 3], [250, 128, 0]], angle: 135, strength: 60 },
            backdrop: Backdrop { path: "C:\\Images\\fond \"été\".png".into(), scope: Scope::App, opacity: 50, blur: 12, dim: 70 },
            ui_font: "Bahnschrift".into(),
            code_font: "Cascadia Mono".into(),
            auto: Auto::Clock,
            day: "clair".into(),
            night: "Mon thème".into(),
            day_hour: 8,
            night_hour: 21,
            name_color: NameColor::Dot,
            hide_rail: true,
        };
        assert_eq!(LookPrefs::parse(&p.to_toml()), Some(p));
        assert_eq!(LookPrefs::parse(&LookPrefs::default().to_toml()), Some(LookPrefs::default()));
        let odd = "[apparence]\ndegrade_couleurs = [\"#000000\"]\ndegrade_angle = 9000\nfond_flou = -4\ntheme_auto = \"parfois\"\nheure_jour = 99\ninconnu = 1\n";
        let got = LookPrefs::parse(odd).unwrap_or_default();
        assert_eq!(got.gradient.colors, Gradient::default().colors, "une seule couleur : refusé");
        assert_eq!((got.gradient.angle, got.backdrop.blur, got.auto, got.day_hour), (ANGLE_MAX, 0, Auto::Off, 23));
    }

    #[test]
    fn day_and_night() {
        let mut p = LookPrefs { auto: Auto::Clock, ..Default::default() };
        assert_eq!(p.phase(None, 6), Some(Phase::Night));
        assert_eq!(p.phase(None, 7), Some(Phase::Day));
        assert_eq!(p.phase(None, 18), Some(Phase::Day));
        assert_eq!(p.phase(None, 19), Some(Phase::Night));
        assert_eq!(p.minutes_to_switch(18, 30), 30);
        assert_eq!(p.minutes_to_switch(19, 0), 12 * 60);
        assert_eq!(p.minutes_to_switch(7, 0), 12 * 60);
        // Le jour passe minuit.
        (p.day_hour, p.night_hour) = (22, 6);
        assert_eq!(p.phase(None, 23), Some(Phase::Day));
        assert_eq!(p.phase(None, 3), Some(Phase::Day));
        assert_eq!(p.phase(None, 12), Some(Phase::Night));
        p.auto = Auto::System;
        assert_eq!(p.phase(Some(true), 12), Some(Phase::Night));
        assert_eq!(p.phase(Some(false), 0), Some(Phase::Day));
        assert_eq!(p.phase(None, 0), None);
        p.auto = Auto::Off;
        assert_eq!(p.phase(Some(true), 0), None);
    }

    #[test]
    fn ready_made_gradients_are_valid() {
        for (key, colors, angle) in GRADIENTS {
            assert!((2..=3).contains(&colors.len()) && *angle <= ANGLE_MAX, "{key}");
        }
        assert_eq!(parse_rgb("#0A0b0C"), Some([10, 11, 12]));
        assert_eq!(parse_rgb("0a0b0c"), Some([10, 11, 12]));
        assert_eq!(parse_rgb("#abc"), None);
    }
}
