//! Personnalisation des profils (lot P) : décoration d'avatar, plaque
//! nominative, tag de serveur, style du nom d'affichage, couleurs de rôle en
//! dégradé (celles-ci lues par le lot A3 : `state::admin::roles::RoleExtra`).
//!
//! Formes vérifiées sur la documentation officielle (User Object :
//! `avatar_decoration_data`, `collectibles.nameplate`, `primary_guild` ;
//! Role Object : `colors`) et sur docs.discord.food (`display_name_styles`).
//! Tout champ absent, nul ou de forme inattendue vaut « rien » : aucune de
//! ces parures n'est indispensable à l'affichage.
//!
//! Tout est `Copy` et petit : ces valeurs voyagent avec `UserInfo` et sont
//! lues à chaque ligne dessinée.

use super::model::ImageHash;
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Noms d'assets distincts gardés. Discord en vend quelques centaines ; au-delà
/// du plafond, un nouvel asset est refusé (la parure n'est pas affichée) :
/// rien n'est évincé, une référence déjà donnée reste donc toujours valable.
pub const MAX_ASSETS: usize = 4096;
/// Longueur d'un nom d'asset, au plus.
pub const MAX_ASSET_LEN: usize = 96;
/// Couleurs gardées d'un style de nom (docs.discord.food : 1 à 5 selon l'effet).
pub const MAX_STYLE_COLORS: usize = 5;

#[derive(Default)]
struct Interner {
    ids: std::collections::HashMap<Box<str>, u16>,
    names: Vec<Box<str>>,
}

fn interner() -> MutexGuard<'static, Interner> {
    static TABLE: std::sync::OnceLock<Mutex<Interner>> = std::sync::OnceLock::new();
    // Table en ajout seul : un fil qui panique ne peut pas la laisser incohérente.
    TABLE.get_or_init(Mutex::default).lock().unwrap_or_else(PoisonError::into_inner)
}

/// Nom d'un asset du CDN (décoration : `a_…` ; plaque : `nameplates/…/`),
/// validé puis rangé une fois : la référence tient sur deux octets, ce qui
/// garde `ImageRef` petit et `Copy`. Une adresse n'est jamais construite à
/// partir d'un texte qui n'a pas passé [`Asset::decoration`] ou [`Asset::path`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Asset(u16);

impl Asset {
    fn intern(s: &str) -> Option<Asset> {
        let mut t = interner();
        if let Some(id) = t.ids.get(s) {
            return Some(Asset(*id));
        }
        if t.names.len() >= MAX_ASSETS {
            return None;
        }
        let id = u16::try_from(t.names.len()).ok()?;
        t.names.push(s.into());
        t.ids.insert(s.into(), id);
        Some(Asset(id))
    }

    /// Empreinte d'une décoration d'avatar (`avatar_decoration_data.asset`) :
    /// lettres minuscules, chiffres et `_` seulement.
    pub fn decoration(s: &str) -> Option<Asset> {
        let ok = !s.is_empty() && s.len() <= MAX_ASSET_LEN && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
        ok.then(|| Self::intern(s)).flatten()
    }

    /// Chemin d'une plaque (`collectibles.nameplate.asset`, par exemple
    /// `nameplates/nameplates/twilight/`) : segments de lettres minuscules,
    /// chiffres, `_` et `-`, chacun suivi de `/`. Rien qui puisse remonter
    /// dans l'arborescence, changer d'hôte ou ajouter une requête.
    pub fn path(s: &str) -> Option<Asset> {
        let seg = |p: &str| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
        let ok = s.len() <= MAX_ASSET_LEN && s.strip_suffix('/').is_some_and(|body| body.split('/').all(seg));
        ok.then(|| Self::intern(s)).flatten()
    }

    /// Le nom validé.
    pub fn name(self) -> String {
        interner().names.get(usize::from(self.0)).map(|s| s.to_string()).unwrap_or_default()
    }

    /// Une version animée existe (`a_…`, comme les autres empreintes).
    pub fn animated(self) -> bool {
        interner().names.get(usize::from(self.0)).is_some_and(|s| s.starts_with("a_"))
    }
}

/// Fond d'une plaque (`palette`, valeurs de la documentation officielle).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Palette {
    Crimson,
    Berry,
    Sky,
    Teal,
    Forest,
    BubbleGum,
    Violet,
    Cobalt,
    Clover,
    Lemon,
    White,
}

impl Palette {
    pub fn parse(s: &str) -> Option<Palette> {
        Some(match s {
            "crimson" => Palette::Crimson,
            "berry" => Palette::Berry,
            "sky" => Palette::Sky,
            "teal" => Palette::Teal,
            "forest" => Palette::Forest,
            "bubble_gum" => Palette::BubbleGum,
            "violet" => Palette::Violet,
            "cobalt" => Palette::Cobalt,
            "clover" => Palette::Clover,
            "lemon" => Palette::Lemon,
            "white" => Palette::White,
            _ => return None,
        })
    }

    /// Teinte 0xRRGGBB de la palette. La documentation ne donne que les noms :
    /// ces valeurs sont les nôtres (une teinte franche par nom), pas celles du
    /// client officiel.
    pub fn tint(self) -> u32 {
        match self {
            Palette::Crimson => 0xd3_2f45,
            Palette::Berry => 0xb5_3a8c,
            Palette::Sky => 0x3f_a6e8,
            Palette::Teal => 0x1f_a79b,
            Palette::Forest => 0x3b_8f4a,
            Palette::BubbleGum => 0xf0_6fb4,
            Palette::Violet => 0x8a_5cf0,
            Palette::Cobalt => 0x3d_5fe0,
            Palette::Clover => 0x57_b84a,
            Palette::Lemon => 0xe6_c531,
            Palette::White => 0xd9_dce3,
        }
    }
}

/// Plaque nominative : fond derrière la ligne d'un membre ou d'un MP.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Nameplate {
    pub asset: Asset,
    /// `None` : palette inconnue (image seule).
    pub palette: Option<Palette>,
}

/// Tag de serveur (`primary_guild`) : au plus 4 caractères, gardés sur place.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GuildTag {
    pub guild_id: u64,
    text: [u8; 16],
    len: u8,
    pub badge: Option<ImageHash>,
}

impl GuildTag {
    /// `None` si le tag est vide, trop long (plus de 4 caractères) ou porte
    /// un caractère de contrôle.
    pub fn new(guild_id: u64, tag: &str, badge: Option<ImageHash>) -> Option<GuildTag> {
        let n = tag.chars().count();
        if n == 0 || n > 4 || tag.len() > 16 || tag.chars().any(char::is_control) {
            return None;
        }
        let mut text = [0u8; 16];
        text[..tag.len()].copy_from_slice(tag.as_bytes());
        Some(GuildTag { guild_id, text, len: tag.len() as u8, badge })
    }

    pub fn text(&self) -> &str {
        std::str::from_utf8(&self.text[..usize::from(self.len)]).unwrap_or_default()
    }
}

/// Effet d'un nom d'affichage (docs.discord.food, « Display Name Effect »).
/// Couleurs : unie pour `Solid` et les effets à une couleur, dégradé pour
/// les autres ; le décor de l'effet est peint par `ui::cosmetics`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NameEffect {
    Solid,
    Gradient,
    Neon,
    Toon,
    Pop,
    Glow,
    /// Prisme et « gummy » : rendus comme un dégradé.
    Prism,
    Gummy,
    /// Effet inconnu ou d'essai : uni, comme le dit la source.
    Other,
}

impl NameEffect {
    pub fn from_code(code: u64) -> NameEffect {
        match code {
            1 => NameEffect::Solid,
            2 => NameEffect::Gradient,
            3 => NameEffect::Neon,
            4 => NameEffect::Toon,
            5 => NameEffect::Pop,
            6 => NameEffect::Glow,
            7 => NameEffect::Prism,
            8 => NameEffect::Gummy,
            _ => NameEffect::Other,
        }
    }

    /// Identifiant de la source (« Display Name Effect ») ; inconnu : uni.
    pub fn code(self) -> u16 {
        match self {
            NameEffect::Solid | NameEffect::Other => 1,
            NameEffect::Gradient => 2,
            NameEffect::Neon => 3,
            NameEffect::Toon => 4,
            NameEffect::Pop => 5,
            NameEffect::Glow => 6,
            NameEffect::Prism => 7,
            NameEffect::Gummy => 8,
        }
    }

    /// L'effet peut-il être enregistré ? La source publique décrit la valeur
    /// à l'envoi des effets 1 à 6 ; prisme et « gummy » ne sont que lus.
    pub fn writable(self) -> bool {
        !matches!(self, NameEffect::Prism | NameEffect::Gummy | NameEffect::Other)
    }

    /// L'effet prend-il deux couleurs (dégradé) ? Les autres n'en prennent qu'une.
    pub fn two_colors(self) -> bool {
        matches!(self, NameEffect::Gradient | NameEffect::Glow | NameEffect::Prism | NameEffect::Gummy)
    }
}

/// Style du nom d'affichage (`display_name_styles`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NameStyle {
    pub font_id: u16,
    pub effect: NameEffect,
    colors: [u32; MAX_STYLE_COLORS],
    n: u8,
}

impl NameStyle {
    /// `None` sans aucune couleur lisible (rien à rendre).
    pub fn new(font_id: u16, effect: NameEffect, colors: &[u32]) -> Option<NameStyle> {
        let mut out = [0u32; MAX_STYLE_COLORS];
        let mut n = 0;
        for c in colors.iter().filter(|c| **c <= 0xFF_FFFF).take(MAX_STYLE_COLORS) {
            out[n] = *c;
            n += 1;
        }
        (n > 0).then_some(NameStyle { font_id, effect, colors: out, n: n as u8 })
    }

    /// Le même style sans couleur ni effet : sa police seule (nom au repos, comme dans le client).
    pub fn plain(self) -> NameStyle {
        NameStyle { effect: NameEffect::Solid, colors: [0; MAX_STYLE_COLORS], n: 0, ..self }
    }

    pub fn colors(&self) -> &[u32] {
        &self.colors[..usize::from(self.n)]
    }

    /// Couleurs à peindre : une seule pour un effet uni, les deux premières
    /// pour un dégradé (ou une seule s'il n'y en a pas d'autre).
    pub fn paint(&self) -> NameColors {
        let c = self.colors();
        match (self.effect.two_colors(), c) {
            (true, [a, b, ..]) => NameColors { primary: *a, secondary: Some(*b), tertiary: None },
            (_, [a, ..]) => NameColors { primary: *a, secondary: None, tertiary: None },
            _ => NameColors::default(),
        }
    }
}

/// Couleurs d'un nom : unie, dégradé à deux couleurs, ou holographique à
/// trois (Role Colors Object : `primary_color`, `secondary_color`,
/// `tertiary_color`). 0 en `primary` : pas de couleur.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct NameColors {
    pub primary: u32,
    pub secondary: Option<u32>,
    pub tertiary: Option<u32>,
}

impl NameColors {
    pub fn solid(primary: u32) -> Self {
        NameColors { primary, secondary: None, tertiary: None }
    }

    /// Arrêts du dégradé, de gauche à droite (un seul : couleur unie).
    pub fn stops(&self) -> ([u32; 3], usize) {
        match (self.secondary, self.tertiary) {
            (Some(s), Some(t)) => ([self.primary, s, t], 3),
            (Some(s), None) => ([self.primary, s, s], 2),
            _ => ([self.primary; 3], 1),
        }
    }
}

/// Parures portées par l'objet `user`. Chaque champ : `None` = l'objet reçu
/// ne le portait pas (ce qu'on savait est gardé), `Some(None)` = pas de parure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Look {
    pub decoration: Option<Option<Asset>>,
    pub nameplate: Option<Option<Nameplate>>,
    pub tag: Option<Option<GuildTag>>,
    pub name_style: Option<Option<NameStyle>>,
}

impl Look {
    /// `new` l'emporte champ par champ, là où il sait quelque chose.
    pub fn merged(self, new: Look) -> Look {
        Look {
            decoration: new.decoration.or(self.decoration),
            nameplate: new.nameplate.or(self.nameplate),
            tag: new.tag.or(self.tag),
            name_style: new.name_style.or(self.name_style),
        }
    }

    pub fn decoration(&self) -> Option<Asset> {
        self.decoration.flatten()
    }

    pub fn nameplate(&self) -> Option<Nameplate> {
        self.nameplate.flatten()
    }

    pub fn tag(&self) -> Option<GuildTag> {
        self.tag.flatten()
    }

    pub fn name_style(&self) -> Option<NameStyle> {
        self.name_style.flatten()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assets_are_validated_before_being_kept() {
        let a = Asset::decoration("a_fed43ab12698df65902ba06727e20c0e").unwrap_or_else(|| panic!("décoration"));
        assert_eq!(a.name(), "a_fed43ab12698df65902ba06727e20c0e");
        assert!(a.animated());
        assert_eq!(Asset::decoration("a_fed43ab12698df65902ba06727e20c0e"), Some(a), "rangé une fois");
        let fixed = Asset::decoration("v2_0123").unwrap_or_else(|| panic!("fixe"));
        assert!(!fixed.animated());
        for bad in ["", "../x", "a/b", "A_B", "a b", "a?size=1", &"a".repeat(MAX_ASSET_LEN + 1)] {
            assert_eq!(Asset::decoration(bad), None, "{bad}");
        }
        let p = Asset::path("nameplates/nameplates/twilight/").unwrap_or_else(|| panic!("plaque"));
        assert_eq!(p.name(), "nameplates/nameplates/twilight/");
        for bad in ["", "/", "nameplates", "nameplates//x/", "/nameplates/x/", "nameplates/../x/", "nameplates/x/?a=1", "https://exemple.invalid/x/", "nameplates/X/"] {
            assert_eq!(Asset::path(bad), None, "{bad}");
        }
    }

    #[test]
    fn tag_and_style_bounds() {
        let h = ImageHash::parse("0123456789abcdef0123456789abcdef");
        let t = GuildTag::new(9, "RUST", h).unwrap_or_else(|| panic!("tag"));
        assert_eq!((t.text(), t.guild_id, t.badge), ("RUST", 9, h));
        assert_eq!(GuildTag::new(9, "été", None).map(|t| t.text().to_string()).as_deref(), Some("été"));
        for bad in ["", "ABCDE", "A\nB"] {
            assert!(GuildTag::new(9, bad, None).is_none(), "{bad:?}");
        }
        let s = NameStyle::new(11, NameEffect::Gradient, &[0xff0000, 0x1_000_000, 0x00ff00, 1, 2, 3, 4]).unwrap_or_else(|| panic!("style"));
        assert_eq!(s.colors(), &[0xff0000, 0x00ff00, 1, 2, 3], "valeurs hors 24 bits ignorées, cinq au plus");
        assert_eq!(s.paint(), NameColors { primary: 0xff0000, secondary: Some(0x00ff00), tertiary: None });
        let solid = NameStyle::new(3, NameEffect::Neon, &[0x123456, 0x654321]).unwrap_or_else(|| panic!("style"));
        assert_eq!(solid.paint(), NameColors::solid(0x123456));
        assert!(NameStyle::new(1, NameEffect::Solid, &[]).is_none());
        assert_eq!(NameEffect::from_code(1002), NameEffect::Other);
        assert_eq!(Palette::parse("bubble_gum"), Some(Palette::BubbleGum));
        assert_eq!(Palette::parse("inconnue"), None);
    }

    #[test]
    fn look_merges_field_by_field() {
        let deco = Asset::decoration("a_0000");
        let known = Look { decoration: Some(deco), tag: Some(GuildTag::new(1, "AB", None)), ..Look::default() };
        // Objet partiel : rien n'est effacé.
        assert_eq!(known.merged(Look::default()), known);
        // `null` explicite : la parure est retirée.
        let cleared = known.merged(Look { decoration: Some(None), ..Look::default() });
        assert_eq!((cleared.decoration(), cleared.tag().map(|t| t.text().to_string())), (None, Some("AB".into())));
    }
}
