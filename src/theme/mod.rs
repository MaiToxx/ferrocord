//! Jetons de thème : toute couleur, toute taille, tout rayon et tout
//! espacement de l'interface vient d'ici. Les jetons sont rangés en groupes
//! (couleurs, typographie, formes, espacements, mise en page) ; chaque champ
//! est décrit une seule fois dans la macro `tokens!`, qui en déduit la
//! structure, le panneau Apparence, la lecture et l'écriture de `theme.toml`.
//!
//! Seuls des réglages ayant un effet visible sur l'interface sont exposés :
//! ce qui ne règle que le panneau lui-même reste interne (constantes ci-dessous).
//!
//! Ne dépend pas de `net/`. Les E/S disque vivent dans `watch` (fil à part).

mod contrast;
mod file;
pub mod icons;
mod legacy;
mod presets;
// Thèmes partageables : fichiers `.ferrotheme` (lot E5).
pub mod share;
mod watch;

pub use contrast::{contrast_ratio, MIN_MUTED, MIN_TEXT};

/// Opacité minimale (sur 255) de la pastille sombre posée sur un média : à 70 %
/// de noir, un texte clair garde plus de 4,5:1 même sur une image blanche.
const MEDIA_VEIL_ALPHA: u8 = 178;

/// Contraste minimal entre une case non cochée et la page qui la porte.
pub const BOX_CONTRAST: f64 = 2.0;
pub use file::{export, export_in, fmt_color, parse, parse_color, parse_in, parse_skipping, ThemeError};
pub use presets::Preset;
pub use watch::{default_path, Document, FileLink, FileUpdate, ThemeLink, Update, WatchConfig};

use crate::i18n::{tokens::Tok, Text};
use crate::state::people::Status;
use eframe::egui::{self, Color32, CornerRadius, Margin};

/// Bornes d'un jeton numérique (le panneau et le fichier les respectent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds {
    pub min: f32,
    pub max: f32,
    /// Pas du curseur ; 1 = valeur entière.
    pub step: f32,
}

/// Bornes du zoom global (le zoom au clavier d'egui y est ramené).
pub const ZOOM: Bounds = Bounds { min: 0.5, max: 3.0, step: 0.05 };
/// Bornes de la largeur du panneau des salons : celles du curseur, du
/// fichier et du redimensionnement à la souris sont les mêmes.
pub const CHANNELS_WIDTH: Bounds = Bounds { min: 160.0, max: 480.0, step: 1.0 };
/// Bornes de la largeur de la liste des membres.
pub const MEMBERS_WIDTH: Bounds = Bounds { min: 160.0, max: 420.0, step: 1.0 };
/// Géométrie du panneau Apparence : réglée à la souris, pas un jeton (elle
/// n'a d'effet que sur le panneau lui-même).
#[derive(Clone, Copy, Debug)]
pub struct PanelGeometry {
    pub width: f32,
    pub min: f32,
    pub max: f32,
    /// Part de la largeur donnée aux libellés.
    pub label_share: f32,
    pub margin: f32,
}

pub const APPEARANCE_PANEL: PanelGeometry = PanelGeometry { width: 360.0, min: 280.0, max: 640.0, label_share: 0.42, margin: 12.0 };

/// Géométrie de la fenêtre Paramètres (comme le panneau Apparence : pas un
/// jeton, elle ne règle que la fenêtre elle-même).
#[derive(Clone, Copy, Debug)]
pub struct DialogGeometry {
    /// Part de la fenêtre principale occupée, en largeur et en hauteur.
    pub share: f32,
    pub max_width: f32,
    pub max_height: f32,
    /// Colonne des catégories.
    pub nav_width: f32,
    /// Largeur maximale du contenu d'une page (lisibilité sur grand écran).
    pub page_width: f32,
    pub margin: f32,
}

pub const SETTINGS_WINDOW: DialogGeometry = DialogGeometry { share: 0.9, max_width: 1100.0, max_height: 820.0, nav_width: 210.0, page_width: 640.0, margin: 12.0 };

impl DialogGeometry {
    pub fn inner_margin(&self) -> Margin {
        Margin::same(px(self.margin))
    }
}

impl PanelGeometry {
    pub fn inner_margin(&self) -> Margin {
        Margin::same(px(self.margin))
    }
}

const NO_BOUNDS: Bounds = Bounds { min: 0.0, max: 0.0, step: 0.0 };

impl Bounds {
    /// Décimales à afficher : aucune pour un pas entier.
    pub fn decimals(&self) -> usize {
        if self.step >= 1.0 {
            0
        } else if self.step >= 0.1 {
            1
        } else {
            2
        }
    }

    /// Ramène une valeur dans les bornes ; une valeur non finie devient le minimum.
    pub fn clamp(&self, v: f32) -> f32 {
        if v.is_finite() { v.clamp(self.min, self.max) } else { self.min }
    }
}

/// Option à choix fermé (densité, horloge…).
pub trait ChoiceDyn {
    /// (clé dans le fichier, libellé affiché)
    fn options(&self) -> &'static [(&'static str, Text)];
    fn index(&self) -> usize;
    fn set_index(&mut self, i: usize);
}

/// Accès en écriture à un jeton, pour le panneau et le lecteur de fichier.
pub enum FieldMut<'a> {
    Color(&'a mut Color32),
    Palette(&'a mut [Color32; 8]),
    Number(&'a mut f32, Bounds),
    Choice(&'a mut dyn ChoiceDyn),
}

pub struct Field<'a> {
    /// Section du fichier (`[couleurs]`…).
    pub section: &'static str,
    pub key: &'static str,
    /// Libellé court (une ligne dans le panneau) et explication (infobulle
    /// du panneau, commentaire du fichier), dans chaque langue.
    pub text: Tok,
    pub value: FieldMut<'a>,
}

trait TokenValue {
    fn field_mut(&mut self, b: Bounds) -> FieldMut<'_>;
}

impl TokenValue for Color32 {
    fn field_mut(&mut self, _: Bounds) -> FieldMut<'_> {
        FieldMut::Color(self)
    }
}

impl TokenValue for [Color32; 8] {
    fn field_mut(&mut self, _: Bounds) -> FieldMut<'_> {
        FieldMut::Palette(self)
    }
}

impl TokenValue for f32 {
    fn field_mut(&mut self, b: Bounds) -> FieldMut<'_> {
        FieldMut::Number(self, b)
    }
}

macro_rules! choice {
    ($(#[doc = $doc:literal])* $T:ident { $($V:ident = $key:literal, $label:ident;)+ }) => {
        $(#[doc = $doc])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum $T { $($V),+ }

        impl $T {
            const ALL: &'static [$T] = &[$($T::$V),+];
            const OPTIONS: &'static [(&'static str, Text)] = &[$(($key, crate::i18n::t::$label)),+];
        }

        impl ChoiceDyn for $T {
            fn options(&self) -> &'static [(&'static str, Text)] {
                Self::OPTIONS
            }
            fn index(&self) -> usize {
                Self::ALL.iter().position(|v| v == self).unwrap_or(0)
            }
            fn set_index(&mut self, i: usize) {
                if let Some(v) = Self::ALL.get(i) {
                    *self = *v;
                }
            }
        }

        impl TokenValue for $T {
            fn field_mut(&mut self, _: Bounds) -> FieldMut<'_> {
                FieldMut::Choice(self)
            }
        }
    };
}

choice! {
    /// Espacement vertical général.
    Density { Airy = "aere", DENSITY_AIRY; Normal = "normal", DENSITY_NORMAL; Compact = "compact", DENSITY_COMPACT; }
}

choice! {
    /// Messages groupés par auteur, ou une ligne par message (« 12:34 auteur : texte »).
    MessageMode { Grouped = "groupes", MESSAGES_GROUPED; Compact = "compact", MESSAGES_COMPACT; }
}

choice! {
    Clock { H24 = "24h", CLOCK_24; H12 = "12h", CLOCK_12; }
}

choice! {
    /// Rail des serveurs : pastilles d'initiales, ou liste avec les noms.
    RailStyle { Pills = "pastilles", RAIL_PILLS; List = "liste", RAIL_LIST; }
}

choice! {
    /// Forme des avatars (les icônes de serveur gardent la forme des pastilles du rail).
    AvatarShape { Round = "ronds", SHAPE_ROUND; Square = "carres", SHAPE_SQUARE; }
}

choice! {
    /// Avatars (messages groupés, conversations privées, pied utilisateur) ;
    /// masqués, ils disparaissent partout et aucun n'est téléchargé.
    Avatars { Shown = "affiches", AVATARS_SHOWN; Hidden = "masques", AVATARS_HIDDEN; }
}

choice! {
    /// Images jointes et aperçus : chargés dès qu'ils s'affichent, ou au clic
    /// (la place est réservée de la même façon).
    MediaLoad { Auto = "auto", MEDIA_AUTO; Click = "au_clic", MEDIA_CLICK; }
}

choice! {
    /// « En train d'écrire » envoyé à Discord pendant la frappe, ou jamais.
    TypingSignal { Sent = "envoye", TYPING_SENT; Never = "jamais", TYPING_NEVER; }
}

choice! {
    /// Pastille de statut (en ligne, inactif…) sur les avatars des messages.
    StatusDots { Shown = "affichees", STATUS_DOTS_SHOWN; Hidden = "masquees", STATUS_DOTS_HIDDEN; }
}

choice! {
    /// Liste des membres à droite de la discussion, ou masquée.
    MembersPanel { Shown = "droite", MEMBERS_SHOWN; Hidden = "masquee", MEMBERS_HIDDEN; }
}

choice! {
    /// Police de l'interface : Figtree (embarquée), ou Segoe UI du système.
    /// Le code reste en Consolas.
    FontFace { Figtree = "figtree", FONT_FIGTREE; System = "systeme", FONT_SYSTEM; }
}

choice! {
    /// Barre de titre : dessinée par Ferrocord (fenêtre sans décoration), ou
    /// celle de Windows. Hors de Windows, toujours celle du système.
    TitleBar { Integrated = "integree", TITLEBAR_INTEGRATED; Native = "windows", TITLEBAR_NATIVE; }
}

impl Density {
    /// Facteur appliqué aux espacements verticaux.
    pub fn factor(self) -> f32 {
        match self {
            Density::Airy => 1.4,
            Density::Normal => 1.0,
            Density::Compact => 0.6,
        }
    }
}

macro_rules! bounds {
    () => {
        NO_BOUNDS
    };
    ($b:expr) => {
        $b
    };
    ($lo:expr, $hi:expr, $step:expr) => {
        Bounds { min: $lo, max: $hi, step: $step }
    };
}

macro_rules! tokens {
    ($(
        $group:ident : $Group:ident = $section:literal, $title:ident {
            $( $field:ident : $ty:ty $([$($b:expr),+])?; )+
        }
    )+) => {
        $(
            /// Libellés et explications : `i18n::tokens`.
            #[derive(Clone, Debug, PartialEq)]
            pub struct $Group { $( pub $field: $ty, )+ }
        )+

        #[derive(Clone, Debug, PartialEq)]
        pub struct Tokens {
            /// Préréglage de départ : les clés absentes du fichier en viennent.
            pub base: Preset,
            $( pub $group: $Group, )+
        }

        /// Sections du fichier, dans l'ordre, avec leur titre affiché.
        pub const SECTIONS: &[(&str, Text)] = &[$(($section, crate::i18n::t::$title)),+];

        impl Tokens {
            /// Tous les jetons, dans l'ordre du fichier.
            pub fn fields_mut(&mut self) -> Vec<Field<'_>> {
                vec![$($(
                    Field {
                        section: $section,
                        key: stringify!($field),
                        // Un jeton sans texte dans `i18n::tokens` ne compile pas.
                        text: crate::i18n::tokens::$group::$field,
                        value: TokenValue::field_mut(&mut self.$group.$field, bounds!($($($b),+)?)),
                    },
                )+)+]
            }
        }
    };
}

tokens! {
    colors: Colors = "couleurs", SECTION_COLORS {
        bg0: Color32;
        bg1: Color32;
        bg2: Color32;
        bg3: Color32;
        border: Color32;
        pill: Color32;
        selected: Color32;
        row_hover: Color32;
        text: Color32;
        text_strong: Color32;
        muted: Color32;
        faint: Color32;
        accent: Color32;
        on_accent: Color32;
        danger: Color32;
        link: Color32;
        hover: Color32;
        widget_text: Color32;
        button_bg: Color32;
        button_text: Color32;
        control_bg: Color32;
        separator: Color32;
        scrim: Color32;
        shadow: Color32;
        focus: Color32;
        online: Color32;
        idle: Color32;
        dnd: Color32;
        offline: Color32;
        authors: [Color32; 8];
        panel_border: Color32;
        menu_bg: Color32;
        menu_hover: Color32;
        channel_idle: Color32;
    }
    typography: Typography = "typographie", SECTION_TYPOGRAPHY {
        body: f32 [8.0, 32.0, 0.5];
        small: f32 [6.0, 24.0, 0.5];
        button: f32 [8.0, 32.0, 0.5];
        title: f32 [8.0, 40.0, 0.5];
        icon: f32 [8.0, 48.0, 0.5];
        rail: f32 [8.0, 32.0, 0.5];
        logo: f32 [12.0, 64.0, 1.0];
        line_height: f32 [0.0, 48.0, 0.5];
        family: FontFace;
    }
    shapes: Shapes = "formes", SECTION_SHAPES {
        roundness: f32 [0.0, 2.0, 0.05];
        card: f32 [0.0, 40.0, 1.0];
        rail: f32 [0.0, 48.0, 1.0];
        rail_selected: f32 [0.0, 48.0, 1.0];
        row: f32 [0.0, 24.0, 1.0];
        control: f32 [0.0, 16.0, 1.0];
        composer: f32 [0.0, 32.0, 1.0];
        embed: f32 [0.0, 24.0, 1.0];
        embed_bar: f32 [0.0, 12.0, 1.0];
        toast: f32 [0.0, 24.0, 1.0];
        hover: f32 [0.0, 16.0, 1.0];
        avatar: f32 [0.0, 32.0, 1.0];
        border: f32 [0.0, 6.0, 0.5];
        unread_ring: f32 [1.0, 6.0, 0.5];
        mention_dot: f32 [1.0, 10.0, 0.5];
        status_dot: f32 [2.0, 10.0, 0.5];
        focus: f32 [1.0, 6.0, 0.5];
    }
    spacing: Spacing = "espacements", SECTION_SPACING {
        item_x: f32 [0.0, 32.0, 1.0];
        item_y: f32 [0.0, 24.0, 1.0];
        control_x: f32 [0.0, 32.0, 1.0];
        control_y: f32 [0.0, 16.0, 1.0];
        rail_x: f32 [0.0, 32.0, 1.0];
        rail_y: f32 [0.0, 32.0, 1.0];
        rail_gap: f32 [0.0, 24.0, 1.0];
        channels_pad: f32 [0.0, 32.0, 1.0];
        category_gap: f32 [0.0, 32.0, 1.0];
        channel_row: f32 [14.0, 64.0, 1.0];
        channel_indent: f32 [0.0, 48.0, 1.0];
        mention_inset: f32 [0.0, 32.0, 1.0];
        user_bar_x: f32 [0.0, 32.0, 1.0];
        user_bar_y: f32 [0.0, 32.0, 1.0];
        guild_title_x: f32 [0.0, 32.0, 1.0];
        guild_title_y: f32 [0.0, 32.0, 1.0];
        chat_head_x: f32 [0.0, 48.0, 1.0];
        chat_head_y: f32 [0.0, 32.0, 1.0];
        composer_x: f32 [0.0, 48.0, 1.0];
        composer_top: f32 [0.0, 32.0, 1.0];
        composer_bottom: f32 [0.0, 32.0, 1.0];
        composer_inner_x: f32 [0.0, 32.0, 1.0];
        composer_inner_y: f32 [0.0, 32.0, 1.0];
        message_x: f32 [0.0, 64.0, 1.0];
        message_gap: f32 [0.0, 48.0, 1.0];
        message_gap_grouped: f32 [0.0, 24.0, 1.0];
        avatar_gap: f32 [0.0, 32.0, 1.0];
        chat_pad: f32 [0.0, 48.0, 1.0];
        embed_pad: f32 [0.0, 32.0, 1.0];
        media_gap: f32 [0.0, 24.0, 1.0];
        compact_indent: f32 [0.0, 120.0, 1.0];
        card_pad: f32 [0.0, 64.0, 1.0];
        login_title_gap: f32 [0.0, 48.0, 1.0];
        login_field_gap: f32 [0.0, 32.0, 1.0];
        login_button_gap: f32 [0.0, 32.0, 1.0];
        login_error_gap: f32 [0.0, 32.0, 1.0];
        login_notice_gap: f32 [0.0, 48.0, 1.0];
        toast_pad: f32 [0.0, 32.0, 1.0];
        toast_offset: f32 [0.0, 400.0, 1.0];
        member_row: f32 [20.0, 72.0, 1.0];
        member_header: f32 [12.0, 64.0, 1.0];
        header_height: f32 [0.0, 96.0, 1.0];
        message_right: f32 [0.0, 96.0, 1.0];
        control_height: f32 [18.0, 64.0, 1.0];
    }
    layout: Layout = "mise_en_page", SECTION_LAYOUT {
        zoom: f32 [ZOOM];
        density: Density;
        messages: MessageMode;
        clock: Clock;
        rail_style: RailStyle;
        avatars: Avatars;
        avatar_shape: AvatarShape;
        media_load: MediaLoad;
        media_width: f32 [120.0, 800.0, 1.0];
        media_height: f32 [80.0, 800.0, 1.0];
        embed_width: f32 [240.0, 1000.0, 1.0];
        embed_thumb: f32 [32.0, 160.0, 1.0];
        avatar_size: f32 [20.0, 64.0, 1.0];
        small_avatar: f32 [12.0, 40.0, 1.0];
        rail_width: f32 [40.0, 200.0, 1.0];
        rail_list_width: f32 [120.0, 400.0, 1.0];
        rail_button: f32 [24.0, 96.0, 1.0];
        rail_list_row: f32 [20.0, 64.0, 1.0];
        channels_width: f32 [CHANNELS_WIDTH];
        members: MembersPanel;
        members_width: f32 [MEMBERS_WIDTH];
        card_width: f32 [240.0, 800.0, 1.0];
        button_height: f32 [20.0, 64.0, 1.0];
        composer_rows: f32 [1.0, 30.0, 1.0];
        typing: TypingSignal;
        status_dots: StatusDots;
        profile_width: f32 [240.0, 480.0, 1.0];
        profile_avatar: f32 [40.0, 128.0, 1.0];
        titlebar: TitleBar;
        titlebar_height: f32 [24.0, 48.0, 1.0];
    }
    markdown: Markdown = "texte_riche", SECTION_RICH_TEXT {
        code_bg: Color32;
        mention_bg: Color32;
        mention_text: Color32;
        mention_row: Color32;
        spoiler: Color32;
        quote_bar: Color32;
        keyword: Color32;
        string: Color32;
        number: Color32;
        comment: Color32;
        h1: f32 [10.0, 48.0, 0.5];
        h2: f32 [10.0, 40.0, 0.5];
        h3: f32 [8.0, 32.0, 0.5];
        quote_width: f32 [1.0, 8.0, 0.5];
        quote_gap: f32 [0.0, 32.0, 1.0];
        list_indent: f32 [4.0, 64.0, 1.0];
        code_pad: f32 [0.0, 24.0, 1.0];
        code_radius: f32 [0.0, 16.0, 1.0];
        emoji: f32 [12.0, 40.0, 1.0];
        jumbo: f32 [24.0, 96.0, 1.0];
        picker_emoji: f32 [20.0, 56.0, 1.0];
        sticker: f32 [64.0, 320.0, 8.0];
    }
}

/// Réglages de la section « Essentiel » du panneau, dans l'ordre.
pub const ESSENTIALS: &[(&str, &str)] = &[
    ("couleurs", "accent"),
    ("couleurs", "bg2"),
    ("typographie", "body"),
    ("mise_en_page", "zoom"),
    ("mise_en_page", "density"),
    ("mise_en_page", "messages"),
    ("mise_en_page", "clock"),
    ("mise_en_page", "rail_style"),
    ("mise_en_page", "avatars"),
    ("mise_en_page", "avatar_shape"),
    ("mise_en_page", "avatar_size"),
    ("mise_en_page", "media_load"),
    ("mise_en_page", "members"),
    ("formes", "roundness"),
];

impl Default for Tokens {
    fn default() -> Self {
        Preset::DEFAULT.tokens()
    }
}

fn margin(x: f32, y: f32) -> Margin {
    Margin::symmetric(px(x), px(y))
}

/// Les marges d'egui sont des `i8` : les jetons sont bornés bien en deçà.
fn px(v: f32) -> i8 {
    v.round().clamp(i8::MIN as f32, i8::MAX as f32) as i8
}

fn radius(v: f32) -> CornerRadius {
    CornerRadius::same(v.round().clamp(0.0, u8::MAX as f32) as u8)
}

/// Hauteur d'un élément flottant, qui règle son ombre.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Elevation {
    /// Menu, infobulle, barre d'actions, notification.
    Menu,
    /// Fenêtre, carte de profil, sélecteur.
    Window,
}

/// Géométrie d'une ligne du rail en liste.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RailRow {
    /// Marge autour de la pastille d'initiales.
    pub pad: f32,
    /// Côté de la pastille d'initiales (jamais négatif).
    pub badge: f32,
}

impl Tokens {
    /// Toute valeur numérique ramenée dans ses bornes (une valeur non finie
    /// devient le minimum). Tout ce que l'interface enregistre passe par
    /// ici : un fichier exporté est donc toujours relu sans erreur.
    pub fn sanitized(mut self) -> Tokens {
        for f in self.fields_mut() {
            if let FieldMut::Number(v, b) = f.value {
                *v = b.clamp(*v);
            }
        }
        self
    }

    /// Jetons prêts à dessiner : densité appliquée aux espacements
    /// verticaux, arrondi appliqué aux rayons.
    pub fn scaled(&self) -> Tokens {
        let mut t = self.clone().sanitized();
        let f = self.layout.density.factor();
        let s = &mut t.spacing;
        for v in [
            &mut s.item_y,
            &mut s.control_y,
            &mut s.rail_y,
            &mut s.rail_gap,
            &mut s.category_gap,
            &mut s.user_bar_y,
            &mut s.guild_title_y,
            &mut s.chat_head_y,
            &mut s.composer_top,
            &mut s.composer_bottom,
            &mut s.composer_inner_y,
            &mut s.message_gap,
            &mut s.message_gap_grouped,
            &mut s.chat_pad,
        ] {
            *v = (*v * f).round();
        }
        // Une ligne de salon reste assez haute pour son texte.
        s.channel_row = (s.channel_row * f).round().max((t.typography.body * 1.4).ceil());
        // Ligne de membre : nom et activité sur deux lignes ; titre : une ligne.
        s.member_row = (s.member_row * f).round().max(((t.typography.body + t.typography.small) * 1.3).ceil());
        s.member_header = (s.member_header * f).round().max((t.typography.small * 1.6).ceil());
        let r = t.shapes.roundness;
        let sh = &mut t.shapes;
        for v in [&mut sh.card, &mut sh.rail, &mut sh.rail_selected, &mut sh.row, &mut sh.control, &mut sh.composer, &mut sh.embed, &mut sh.toast, &mut sh.hover, &mut sh.avatar] {
            *v = (*v * r).round();
        }
        t.markdown.code_radius = (t.markdown.code_radius * r).round();
        t
    }

    /// Les mêmes jetons avec les valeurs du préréglage `base`.
    pub fn reset(&self) -> Tokens {
        self.base.tokens()
    }

    /// Le thème a-t-il un fond clair ? (choisit la base des widgets egui)
    pub fn is_light(&self) -> bool {
        contrast::luminance(self.colors.bg2) > 0.4
    }

    /// Couleurs des widgets egui standard (champs, boutons, sélection, liens).
    pub fn visuals(&self) -> egui::Visuals {
        let c = &self.colors;
        let mut v = if self.is_light() { egui::Visuals::light() } else { egui::Visuals::dark() };
        v.panel_fill = c.bg2;
        // Menus, listes déroulantes et infobulles (les fenêtres posent leur propre cadre).
        v.window_fill = c.menu_bg;
        v.window_stroke = self.border();
        v.window_corner_radius = self.r_card();
        v.menu_corner_radius = self.r_embed();
        v.window_shadow = self.shadow(Elevation::Window);
        v.popup_shadow = self.shadow(Elevation::Menu);
        v.extreme_bg_color = c.bg0;
        v.faint_bg_color = c.row_hover;
        v.code_bg_color = self.markdown.code_bg;
        v.warn_fg_color = c.idle;
        v.error_fg_color = c.danger;
        v.weak_text_color = Some(c.faint);
        v.selection.bg_fill = c.accent.linear_multiply(0.25);
        // Texte d'un bouton sélectionné et contour du champ qui a le clavier.
        v.selection.stroke = egui::Stroke::new(1.0, c.link);
        v.text_cursor.stroke.color = c.accent;
        v.hyperlink_color = c.link;
        v.slider_trailing_fill = true;
        v.interact_cursor = Some(egui::CursorIcon::PointingHand);
        let r = self.r_control();
        let w = &mut v.widgets;
        for s in [&mut w.noninteractive, &mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open] {
            s.corner_radius = r;
            // Aucun bouton ne grossit au survol : seuls le fond et le filet changent.
            s.expansion = 0.0;
        }
        w.noninteractive.fg_stroke.color = c.widget_text;
        w.noninteractive.bg_stroke = egui::Stroke::new(1.0, c.separator);
        w.noninteractive.bg_fill = c.bg2;
        w.noninteractive.weak_bg_fill = c.bg2;
        w.inactive.weak_bg_fill = c.button_bg;
        // Fond des cases, boutons radio et rails de curseur d'egui : jamais fondu dans la page.
        w.inactive.bg_fill = self.box_fill();
        w.inactive.fg_stroke.color = c.button_text;
        w.inactive.bg_stroke = egui::Stroke::new(1.0, self.hairline(false));
        for s in [&mut w.hovered, &mut w.open] {
            s.weak_bg_fill = c.control_bg;
            s.bg_fill = c.control_bg;
            s.fg_stroke = egui::Stroke::new(1.0, c.text_strong);
            s.bg_stroke = egui::Stroke::new(1.0, self.hairline(true));
        }
        w.active.weak_bg_fill = c.selected;
        w.active.bg_fill = c.selected;
        w.active.fg_stroke = egui::Stroke::new(1.0, c.text_strong);
        w.active.bg_stroke = egui::Stroke::new(1.0, self.hairline(true));
        v
    }

    /// Rouge des aplats qui portent du texte `on_accent` (bouton destructeur,
    /// pastille de mentions, « EN DIRECT ») : `danger` est réglé pour être lu
    /// comme texte sur les fonds, il est donc foncé (ou éclairci) ici jusqu'à
    /// ce que le texte posé dessus atteigne 4,5:1.
    pub fn danger_fill(&self) -> Color32 {
        self.fill_under_text(self.colors.danger)
    }

    /// Vert des aplats qui portent du texte `on_accent` (bouton « Rejoindre »,
    /// « Enregistrer ») : `online`, foncé de la même façon.
    pub fn success_fill(&self) -> Color32 {
        self.fill_under_text(self.colors.online)
    }

    /// `fill` foncé (ou éclairci) jusqu'à ce que le texte `on_accent` y atteigne 4,5:1.
    fn fill_under_text(&self, mut fill: Color32) -> Color32 {
        let on = self.colors.on_accent;
        let darken = contrast::luminance(on) > contrast::luminance(fill);
        for _ in 0..24 {
            if contrast_ratio(on, fill) >= MIN_TEXT {
                break;
            }
            let step = |v: u8| if darken { (f32::from(v) * 0.95) as u8 } else { v.saturating_add(((255.0 - f32::from(v)) * 0.08).ceil() as u8) };
            fill = Color32::from_rgb(step(fill.r()), step(fill.g()), step(fill.b()));
        }
        fill
    }

    /// Fond d'une case à cocher ou d'un bouton radio non cochés (et du rail d'un
    /// curseur) : `control_bg`, rapproché du texte jusqu'à se détacher de la page
    /// ([`BOX_CONTRAST`]). Dans Onyx, ces contrôles se fondaient dans le noir.
    pub fn box_fill(&self) -> Color32 {
        let (from, to, page) = (self.colors.control_bg, self.colors.text_strong, self.colors.bg2);
        let mix = |k: f32| {
            let f = |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * k).round() as u8;
            Color32::from_rgb(f(from.r(), to.r()), f(from.g(), to.g()), f(from.b(), to.b()))
        };
        (0..=10).map(|i| mix(i as f32 * 0.05)).find(|c| contrast_ratio(*c, page) >= BOX_CONTRAST).unwrap_or_else(|| mix(0.5))
    }

    /// Filet des boutons et des champs : le texte fort, presque transparent
    /// (il se voit sur tous les fonds, accent compris) ; `strong` : au survol.
    pub fn hairline(&self, strong: bool) -> Color32 {
        self.colors.text_strong.gamma_multiply(if strong { 0.2 } else { 0.1 })
    }

    /// Ombre portée d'un élément flottant (`docs/design.md`, élévation).
    pub fn shadow(&self, e: Elevation) -> egui::Shadow {
        let (y, blur) = match e {
            Elevation::Menu => (6, 18),
            Elevation::Window => (12, 36),
        };
        egui::Shadow { offset: [0, y], blur, spread: 0, color: self.colors.shadow }
    }

    /// Cadre d'une fenêtre (modale, sélecteur) : fond principal, contour,
    /// grand rayon, marge de carte, ombre de fenêtre.
    pub fn window_frame(&self) -> egui::Frame {
        egui::Frame::new().fill(self.colors.bg2).stroke(self.border()).corner_radius(self.r_card()).inner_margin(self.card_margin()).shadow(self.shadow(Elevation::Window))
    }

    /// Cadre d'une carte posée dans le flux (embed, fichier, sondage) : fond
    /// des cartes, contour, rayon moyen, pas d'ombre.
    pub fn card_frame(&self) -> egui::Frame {
        egui::Frame::new().fill(self.card_fill()).stroke(self.border()).corner_radius(self.r_embed()).inner_margin(self.embed_margin())
    }

    /// Fond d'une carte posée dans la discussion (embed, fichier, sondage,
    /// invitation, réaction). Dans les thèmes calqués, la colonne des salons
    /// (`bg1`) est plus sombre que la discussion : la carte prend le fond de la
    /// zone de saisie, un cran au-dessus, comme dans le client.
    pub fn card_fill(&self) -> Color32 {
        if self.base.calque() && !self.is_light() { self.colors.bg3 } else { self.colors.bg1 }
    }

    /// Marge d'un embed : 8 en haut, 16 à droite et en bas, 12 à gauche (avant
    /// la barre de couleur) dans les thèmes calqués ; la marge des cartes sinon.
    pub fn embed_card_margin(&self) -> Margin {
        let (p, a) = (self.spacing.embed_pad, self.spacing.item_y);
        if self.base.calque() { Margin { left: px(p), top: px(p - a), right: px(p + a), bottom: px(p + a) } } else { self.embed_margin() }
    }

    /// Fond de la vue d'appel (tuiles et barre de contrôle) : noir dans les
    /// thèmes sombres calqués, comme dans le client ; le fond le plus sombre sinon.
    pub fn call_bg(&self) -> Color32 {
        if self.base.calque() && !self.is_light() { Color32::BLACK } else { self.colors.bg0 }
    }

    /// Hauteur d'une pastille de réaction : 24 pour un contrôle de 32, jamais
    /// moins que son compte et un peu d'air.
    pub fn reaction_height(&self) -> f32 {
        (self.spacing.control_height - 2.0 * self.spacing.item_y).max((self.typography.button * 1.35).ceil() + self.spacing.item_y)
    }

    /// Côté de l'emoji d'une pastille de réaction (16 dans une pastille de 24).
    pub fn reaction_emoji(&self) -> f32 {
        (self.reaction_height() * 2.0 / 3.0).round()
    }

    /// Cadre d'un élément flottant léger (barre d'actions, notification).
    pub fn float_frame(&self) -> egui::Frame {
        egui::Frame::new().fill(self.colors.bg3).stroke(self.border()).shadow(self.shadow(Elevation::Menu))
    }

    /// Rayon des boutons et des champs.
    pub fn r_control(&self) -> CornerRadius {
        radius(self.shapes.control)
    }

    /// Fond translucide d'accent : réaction choisie, onglet actif, mention.
    pub fn accent_tint(&self) -> Color32 {
        self.colors.accent.linear_multiply(0.18)
    }

    /// Hauteur du contenu des en-têtes (salon, serveur, amis) : celle d'un
    /// bouton, pour que leurs filets s'alignent d'une colonne à l'autre.
    pub fn head_row(&self) -> f32 {
        let text = (self.typography.body * 1.35).ceil();
        // Hauteur d'en-tête imposée (48 dans le client) : le contenu prend ce que laissent les marges.
        if self.spacing.header_height > 0.0 {
            return (self.spacing.header_height - 2.0 * self.spacing.chat_head_y).max(text);
        }
        text + 2.0 * self.spacing.control_y
    }

    /// Hauteur d'un en-tête (discussion, colonne des salons, amis, appel) :
    /// celle du thème, sinon la ligne et ses marges.
    pub fn header_height(&self) -> f32 {
        if self.spacing.header_height > 0.0 { self.spacing.header_height } else { self.head_row() + 2.0 * self.spacing.chat_head_y }
    }

    /// Interligne des messages, si le thème l'impose (sinon celui de la police).
    pub fn line_height(&self) -> Option<f32> {
        (self.typography.line_height >= self.typography.body).then_some(self.typography.line_height)
    }

    /// Filet entre deux zones (rail, colonne des salons, discussion) et sous la barre de titre.
    pub fn panel_stroke(&self) -> egui::Stroke {
        egui::Stroke::new(self.shapes.border.max(1.0), self.colors.panel_border)
    }

    /// Filets de la colonne des salons (`channels`) : à gauche contre le rail
    /// (`rail`), à droite contre la discussion ; et, sous la barre de titre
    /// intégrée, le filet du haut de l'interface (`content`), du rail au bord droit.
    pub fn paint_panel_borders(&self, p: &egui::Painter, rail: egui::Rect, channels: egui::Rect, content: egui::Rect) {
        let s = self.panel_stroke();
        if s.color.a() == 0 {
            return;
        }
        let half = s.width / 2.0;
        p.vline(rail.right() + half, rail.y_range(), s);
        p.vline(channels.right() - half, channels.y_range(), s);
        if self.titlebar_integrated() {
            p.hline(rail.right()..=content.right(), content.top() + half, s);
        }
    }

    /// Barre de titre dessinée par Ferrocord (Windows seulement : ailleurs, celle du système).
    pub fn titlebar_integrated(&self) -> bool {
        cfg!(windows) && self.layout.titlebar == TitleBar::Integrated
    }

    /// Cadre d'un menu ou d'une infobulle : fond des menus, contour, ombre de menu.
    pub fn menu_frame(&self) -> egui::Frame {
        egui::Frame::new().fill(self.colors.menu_bg).stroke(self.border()).corner_radius(self.r_embed()).inner_margin(self.menu_margin()).shadow(self.shadow(Elevation::Menu))
    }

    /// Marge intérieure des menus.
    pub fn menu_margin(&self) -> Margin {
        Margin::same(px(self.spacing.item_y + self.spacing.control_y / 2.0))
    }

    /// Ligne de menu survolée : fond, et le texte le plus lisible dessus.
    pub fn menu_hover(&self) -> (Color32, Color32) {
        let (fill, c) = (self.colors.menu_hover, &self.colors);
        (fill, if contrast_ratio(c.on_accent, fill) > contrast_ratio(c.text_strong, fill) { c.on_accent } else { c.text_strong })
    }

    /// Hauteur d'un champ de formulaire : un contrôle et de l'air (40 pour un contrôle de 32).
    pub fn field_height(&self) -> f32 {
        (self.spacing.control_height + 2.0 * self.spacing.item_y).max((self.typography.body * 1.35).ceil() + 2.0 * self.spacing.control_y)
    }

    /// Hauteur de la ligne du panneau utilisateur : le petit avatar ou un
    /// bouton-icône (marge `padding`), le plus haut des deux.
    pub fn user_bar_row(&self, padding: f32) -> f32 {
        let avatar = if self.avatars_shown() { self.layout.small_avatar } else { 0.0 };
        avatar.max(self.panel_button(padding))
    }

    /// Icône d'un bouton du panneau utilisateur (micro, casque, engrenage) :
    /// 20 pour un texte de bouton de 14, comme dans le client.
    pub fn panel_icon(&self) -> f32 {
        (self.typography.button * 1.4).round()
    }

    /// Côté d'un bouton du panneau utilisateur (32 pour une icône de 20).
    pub fn panel_button(&self, padding: f32) -> f32 {
        self.panel_icon() + 2.0 * padding
    }

    /// Hauteur d'une ligne de conversation privée : le petit avatar et de
    /// l'air autour (42 pour un avatar de 32, comme dans le client).
    pub fn dm_row(&self) -> f32 {
        self.spacing.channel_row.max((self.layout.small_avatar + 2.5 * self.spacing.item_y).round())
    }

    /// Hauteur d'une entrée en tête des messages privés (« Amis », « Ton
    /// Nitro », « Boutique », « Quêtes ») : 40 pour un petit avatar de 32.
    pub fn dm_entry_row(&self) -> f32 {
        self.spacing.channel_row.max(self.layout.small_avatar + 2.0 * self.spacing.item_y)
    }

    /// Écart entre deux lignes de la colonne des messages privés (2 dans le client).
    pub fn dm_gap(&self) -> f32 {
        (self.spacing.item_y / 2.0).round()
    }

    /// Corps du nom d'une conversation privée et des entrées en tête des
    /// messages privés : celui des titres (16 dans le client), ou le corps du
    /// texte s'il est plus grand.
    pub fn dm_text(&self) -> f32 {
        self.typography.body.max(self.typography.title)
    }

    /// Bouton « Rechercher ou lancer une conversation » : aussi haut que le
    /// permet l'en-tête de la colonne (40 dans un en-tête de 48), sans
    /// dépasser un bouton plein.
    pub fn find_button_height(&self) -> f32 {
        (self.header_height() - 2.0 * self.spacing.item_y).min(self.layout.button_height).max(self.spacing.control_height)
    }

    /// Champ « Rechercher » de la page Amis : une fois et demie le petit
    /// avatar (48 pour 32), jamais moins qu'un champ de formulaire.
    pub fn search_height(&self) -> f32 {
        self.field_height().max((self.layout.small_avatar * 1.5).round())
    }

    /// Indicateur du rail (serveur ouvert, survolé, non lu), contre le bord
    /// gauche `edge` du rail, à la hauteur de `tile` : rectangle et rayon.
    /// `share` : part de la hauteur de la pastille (0 à 1).
    pub fn rail_mark(&self, edge: f32, tile: egui::Rect, share: f32) -> (egui::Rect, CornerRadius) {
        let w = self.shapes.unread_ring.max(1.0);
        let h = (tile.height() * share.clamp(0.0, 1.0)).round().max(w);
        let rect = egui::Rect::from_min_size(egui::pos2(edge, (tile.center().y - h / 2.0).round()), egui::vec2(w, h));
        let r = w.round().clamp(0.0, u8::MAX as f32) as u8;
        (rect, CornerRadius { nw: 0, sw: 0, ne: r, se: r })
    }

    /// Applique le style à egui (les deux styles clair et sombre, pour que
    /// le thème du système n'ait aucun effet) et le zoom.
    pub fn apply(&self, ctx: &egui::Context) {
        let visuals = self.visuals();
        let ty = &self.typography;
        ctx.all_styles_mut(|s| {
            s.visuals = visuals.clone();
            for (style, size) in [
                (egui::TextStyle::Small, ty.small),
                (egui::TextStyle::Body, ty.body),
                (egui::TextStyle::Button, ty.button),
                (egui::TextStyle::Monospace, ty.body),
            ] {
                if let Some(f) = s.text_styles.get_mut(&style) {
                    f.size = size;
                }
            }
            s.spacing.item_spacing = egui::vec2(self.spacing.item_x, self.spacing.item_y);
            // Boutons, champs et lignes de menu : même marge, donc même hauteur.
            s.spacing.button_padding = egui::vec2(self.spacing.control_x, self.spacing.control_y);
            s.spacing.menu_margin = self.menu_margin();
            // Boutons, champs et lignes de menu : même hauteur minimale.
            s.spacing.interact_size.y = self.spacing.control_height;
            // Pas de fondu d'egui au bord des zones de défilement : il prend le fond du
            // cadre le plus proche, pas celui qui est réellement dessous (dégradé d'un
            // profil, colonne des paramètres, image de fond) et posait une bande d'une
            // autre couleur, à bords nets, sur la dernière ligne.
            s.spacing.scroll.fade.strength = 0.0;
            // Infobulles immédiates, comme celles à flèche de `ui::widgets`.
            s.interaction.tooltip_delay = 0.0;
            s.interaction.show_tooltips_only_when_still = false;
            s.spacing.icon_width = Self::icon_side(self.typography.body);
            // Libellé d'une case ou d'un bouton radio : écarté comme celui d'un interrupteur (4 par défaut, collé).
            s.spacing.icon_spacing = self.spacing.item_x;
            s.spacing.icon_width_inner = (Self::icon_side(self.typography.body) * 0.6).round();
        });
        if (ctx.zoom_factor() - self.layout.zoom).abs() > f32::EPSILON {
            ctx.set_zoom_factor(self.layout.zoom);
        }
    }

    /// Rayon des coins d'un avatar de côté `side` (rond, ou carré arrondi).
    pub fn r_avatar(&self, side: f32) -> CornerRadius {
        match self.layout.avatar_shape {
            AvatarShape::Round => radius(side / 2.0),
            AvatarShape::Square => radius(self.shapes.avatar.min(side / 2.0)),
        }
    }

    /// Avatar par défaut numéro `index` (dessiné sans image) : fond pris
    /// dans la palette des auteurs, et la couleur d'initiales la plus lisible dessus.
    pub fn default_avatar(&self, index: u8) -> (Color32, Color32) {
        let c = &self.colors;
        let fill = c.authors[usize::from(index) % c.authors.len()];
        let text = if contrast_ratio(c.text_strong, fill) >= contrast_ratio(c.bg0, fill) { c.text_strong } else { c.bg0 };
        (fill, text)
    }

    /// Pastille d'un compte dont l'identité n'est pas encore connue (compte
    /// importé de l'ancien fichier) : fond et initiales, texte sur accent.
    pub fn placeholder_avatar(&self) -> (Color32, Color32) {
        (self.colors.accent, self.colors.on_accent)
    }

    /// Taille des initiales dans une pastille de côté `side`.
    pub fn initials_size(side: f32) -> f32 {
        (side * 0.45).round().max(1.0)
    }

    /// Avatars affichés (messages, conversations privées, pied) ; masqués,
    /// aucun n'est demandé. Les icônes de serveur ne sont pas concernées.
    pub fn avatars_shown(&self) -> bool {
        self.layout.avatars == Avatars::Shown
    }

    /// Avatars à gauche des messages : en mode groupé, s'ils ne sont pas masqués.
    pub fn message_avatars(&self) -> bool {
        self.layout.messages == MessageMode::Grouped && self.avatars_shown()
    }

    /// Retrait du texte d'un message quand les avatars sont affichés.
    pub fn message_indent(&self) -> f32 {
        self.layout.avatar_size + self.spacing.avatar_gap
    }

    /// Marge qui décale le texte d'un message à droite de l'avatar.
    pub fn avatar_indent(&self) -> Margin {
        Margin { left: px(self.message_indent()), ..Margin::ZERO }
    }

    /// Ligne de conversation privée : rectangle de l'avatar et début du nom.
    pub fn dm_row_parts(&self, row: egui::Rect) -> (egui::Rect, f32) {
        let side = self.small_avatar_in(row.height());
        let left = row.left() + self.spacing.item_x;
        let avatar = egui::Rect::from_center_size(egui::pos2(left + side / 2.0, row.center().y), egui::Vec2::splat(side));
        (avatar, avatar.right() + self.spacing.item_x)
    }

    /// Liste des membres affichée à droite de la discussion.
    pub fn members_shown(&self) -> bool {
        self.layout.members == MembersPanel::Shown
    }


    /// Couleur de nom donnée par un rôle, ramenée au texte ordinaire si elle
    /// se lit mal sur le fond `bg` (un rôle sombre sur un thème sombre).
    pub fn readable(&self, color: Color32, bg: Color32) -> Color32 {
        if contrast_ratio(color, bg) >= MIN_MUTED { color } else { self.colors.text }
    }

    /// Pastille sombre posée sur une image, une vidéo ou une tuile d'appel
    /// (étiquette du nom, bouton de lecture, barre du lecteur). Toujours
    /// sombre, quel que soit le thème : le voile `scrim` du thème clair (noir
    /// à 45 %) ne suffit pas à porter un texte. Va avec [`Tokens::on_media`].
    pub fn media_veil(&self) -> Color32 {
        Color32::from_black_alpha(self.colors.scrim.a().max(MEDIA_VEIL_ALPHA))
    }

    /// Texte et icônes posés sur [`Tokens::media_veil`] : clairs dans tous les
    /// thèmes (le texte du thème, sombre en thème clair, y serait illisible).
    pub fn on_media(&self) -> Color32 {
        Color32::from_rgb(0xf7, 0xf8, 0xf9)
    }

    /// Texte secondaire (piste, durée) sur [`Tokens::media_veil`].
    pub fn on_media_dim(&self) -> Color32 {
        Color32::from_rgb(0xb5, 0xba, 0xc1)
    }

    /// Rouge d'état (micro coupé) sur [`Tokens::media_veil`].
    pub fn on_media_danger(&self) -> Color32 {
        Color32::from_rgb(0xfb, 0x83, 0x89)
    }

    /// Côté d'un petit avatar dans une ligne de hauteur `row`.
    pub fn small_avatar_in(&self, row: f32) -> f32 {
        self.layout.small_avatar.min(row).max(1.0)
    }

    /// Contour de qui parle autour d'un avatar : écart, puis épaisseur.
    pub fn speaking_ring(&self) -> (f32, f32) {
        (1.0, (self.shapes.unread_ring / 2.0).max(1.0))
    }

    /// Avatar d'un participant dans la liste des salons : le petit avatar,
    /// réduit pour que le contour de qui parle tienne dans une ligne de hauteur `row`.
    pub fn voice_avatar_in(&self, row: f32) -> f32 {
        let (gap, w) = self.speaking_ring();
        self.layout.small_avatar.min(row - 2.0 * (gap + w) - self.spacing.item_y / 2.0).max(1.0)
    }

    /// Couleur stable associée à un auteur.
    pub fn author_color(&self, id: u64) -> Color32 {
        self.colors.authors[(id.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 61) as usize]
    }

    // Valeurs composées, pour que `ui/` n'ait aucun nombre à écrire.

    pub fn rail_margin(&self) -> Margin {
        margin(self.spacing.rail_x, self.spacing.rail_y)
    }
    /// Fond du panneau utilisateur (et du panneau vocal) : entre le rail et
    /// la colonne des salons, pour s'en détacher sans filet.
    pub fn panel_bg(&self) -> Color32 {
        let (a, b) = (self.colors.bg0, self.colors.bg1);
        let mid = |x: u8, y: u8| ((u16::from(x) + u16::from(y)) / 2) as u8;
        Color32::from_rgb(mid(a.r(), b.r()), mid(a.g(), b.g()), mid(a.b(), b.b()))
    }
    pub fn user_bar_margin(&self) -> Margin {
        margin(self.spacing.user_bar_x, self.spacing.user_bar_y)
    }
    pub fn guild_title_margin(&self) -> Margin {
        margin(self.spacing.guild_title_x, self.spacing.guild_title_y)
    }
    pub fn chat_head_margin(&self) -> Margin {
        margin(self.spacing.chat_head_x, self.spacing.chat_head_y)
    }
    /// Panneau latéral qui a son en-tête (membres d'un groupe) : la marge
    /// latérale de l'en-tête de la discussion, pour que les titres se répondent.
    pub fn side_panel_margin(&self) -> Margin {
        margin(self.spacing.chat_head_x, self.spacing.channels_pad)
    }
    pub fn channels_margin(&self) -> Margin {
        Margin::same(px(self.spacing.channels_pad))
    }
    pub fn composer_margin(&self) -> Margin {
        let s = &self.spacing;
        Margin { left: px(s.composer_x), right: px(s.composer_x), top: px(s.composer_top), bottom: px(s.composer_bottom) }
    }
    pub fn composer_inner(&self) -> Margin {
        margin(self.spacing.composer_inner_x, self.spacing.composer_inner_y)
    }
    /// Marge d'un message ; `grouped` = suite d'un groupe du même auteur.
    pub fn message_margin(&self, grouped: bool) -> Margin {
        let s = &self.spacing;
        Margin {
            left: px(s.message_x),
            right: px(s.message_right),
            top: px(if grouped { s.message_gap_grouped } else { s.message_gap }),
            bottom: px(s.message_gap_grouped),
        }
    }
    pub fn compact_indent(&self) -> Margin {
        Margin { left: px(self.spacing.compact_indent), ..Margin::ZERO }
    }
    /// Retrait du contenu d'une citation : barre, puis écart.
    pub fn quote_indent(&self) -> Margin {
        Margin { left: px(self.markdown.quote_width + self.markdown.quote_gap), ..Margin::ZERO }
    }
    pub fn code_margin(&self) -> Margin {
        Margin::same(px(self.markdown.code_pad))
    }
    pub fn r_code(&self) -> CornerRadius {
        radius(self.markdown.code_radius)
    }
    /// Fond d'une mention de rôle : la couleur du rôle, avec la transparence du fond des mentions.
    pub fn role_mention_bg(&self, role: Color32) -> Color32 {
        Color32::from_rgba_unmultiplied(role.r(), role.g(), role.b(), self.markdown.mention_bg.a())
    }
    pub fn embed_margin(&self) -> Margin {
        Margin::same(px(self.spacing.embed_pad))
    }
    pub fn card_margin(&self) -> Margin {
        Margin::same(px(self.spacing.card_pad))
    }
    pub fn toast_margin(&self) -> Margin {
        Margin::same(px(self.spacing.toast_pad))
    }
    pub fn border(&self) -> egui::Stroke {
        egui::Stroke::new(self.shapes.border, self.colors.border)
    }

    pub fn r_card(&self) -> CornerRadius {
        radius(self.shapes.card)
    }
    pub fn r_rail(&self, selected: bool) -> CornerRadius {
        radius(if selected { self.shapes.rail_selected } else { self.shapes.rail })
    }
    pub fn r_row(&self) -> CornerRadius {
        radius(self.shapes.row)
    }
    pub fn r_composer(&self) -> CornerRadius {
        radius(self.shapes.composer)
    }
    pub fn r_embed(&self) -> CornerRadius {
        radius(self.shapes.embed)
    }
    /// Images jointes, aperçus et visionneuse : même rayon que les embeds.
    pub fn r_media(&self) -> CornerRadius {
        radius(self.shapes.embed)
    }
    /// Images jointes chargées dès qu'elles s'affichent (sinon au clic).
    pub fn media_auto(&self) -> bool {
        self.layout.media_load == MediaLoad::Auto
    }
    pub fn r_toast(&self) -> CornerRadius {
        radius(self.shapes.toast)
    }
    pub fn r_hover(&self) -> CornerRadius {
        radius(self.shapes.hover)
    }

    /// Décalage de la notification par rapport au bas de la fenêtre.
    pub fn toast_offset(&self) -> egui::Vec2 {
        egui::vec2(0.0, -self.spacing.toast_offset)
    }

    /// Largeur du rail selon sa forme.
    pub fn rail_panel_width(&self) -> f32 {
        match self.layout.rail_style {
            RailStyle::Pills => self.layout.rail_width,
            RailStyle::List => self.layout.rail_list_width,
        }
    }

    /// Ligne du rail en liste : la marge est bornée pour que la pastille
    /// garde au moins la moitié de la hauteur de la ligne.
    pub fn rail_row(&self) -> RailRow {
        let h = self.layout.rail_list_row;
        let pad = self.spacing.rail_x.clamp(0.0, h / 4.0);
        RailRow { pad, badge: (h - 2.0 * pad).max(0.0) }
    }

    /// Côté d'une icône dessinée à côté d'un texte de taille `size`.
    pub fn icon_side(size: f32) -> f32 {
        (size * 1.1).round()
    }

    /// Marque « non lu » : pastille de 4 × 8 contre le bord gauche de la ligne,
    /// arrondie du côté du texte.
    pub fn unread_mark(&self, row: egui::Rect) -> (egui::Rect, CornerRadius) {
        let w = self.shapes.unread_ring;
        let size = egui::vec2(w, (2.0 * w).min(row.height()));
        let rect = egui::Align2::LEFT_CENTER.align_size_within_rect(size, row);
        let r = w.round().clamp(0.0, u8::MAX as f32) as u8;
        (rect, CornerRadius { nw: 0, sw: 0, ne: r, se: r })
    }

    /// Couleur d'un statut en ligne.
    pub fn status_color(&self, s: Status) -> Color32 {
        let c = &self.colors;
        match s {
            Status::Online => c.online,
            Status::Idle => c.idle,
            Status::Dnd => c.dnd,
            Status::Offline => c.offline,
        }
    }

    /// Pastille de statut sur un avatar : centre, rayon, et épaisseur de
    /// l'anneau (couleur du fond) qui la détache de l'image.
    pub fn status_dot(&self, avatar: egui::Rect) -> (egui::Pos2, f32, f32) {
        let r = (avatar.width() * 0.16).max(2.5);
        let ring = (r * 0.45).max(1.0).round();
        (avatar.max - egui::Vec2::splat(r * 0.85), r, ring)
    }

    /// Pastille de statut peinte sur un avatar. Les formes diffèrent (plein,
    /// croissant, barre, anneau) pour que le statut se lise sans les couleurs.
    pub fn paint_status(&self, p: &egui::Painter, avatar: egui::Rect, s: Status, ring: Color32) {
        let (c, r, w) = self.status_dot(avatar);
        p.circle_filled(c, r + w, ring);
        p.circle_filled(c, r, self.status_color(s));
        match s {
            Status::Online => {}
            Status::Idle => {
                p.circle_filled(c - egui::Vec2::splat(r * 0.45), r * 0.62, ring);
            }
            Status::Dnd => {
                let bar = egui::Rect::from_center_size(c, egui::vec2(r * 1.2, (r * 0.4).max(1.0)));
                p.rect_filled(bar, bar.height() / 2.0, ring);
            }
            Status::Offline => {
                p.circle_filled(c, r * 0.5, ring);
            }
        }
    }

    /// Carte de profil : marge intérieure.
    pub fn profile_pad(&self) -> f32 {
        (self.spacing.item_x * 2.0).round()
    }

    pub fn profile_margin(&self) -> Margin {
        Margin::same(px(self.profile_pad()))
    }

    /// Étiquettes de la carte de profil (badges, rôles).
    pub fn profile_chip_margin(&self) -> Margin {
        margin((self.spacing.item_x * 0.75).round(), self.spacing.item_y)
    }

    /// Anneau (couleur de la carte) autour du grand avatar.
    pub fn profile_ring(&self) -> f32 {
        (self.layout.profile_avatar * 0.075).round().max(2.0)
    }

    /// Carte de profil : hauteur de la bannière (proportions des bannières
    /// de Discord, 17:6), ou de la bande unie quand il n'y en a pas (60 pour
    /// un avatar de 80).
    pub fn profile_banner(&self, has_banner: bool) -> f32 {
        if has_banner { (self.layout.profile_width * 6.0 / 17.0).round() } else { (self.layout.profile_avatar * 0.75).round() }
    }

    /// Profil complet : bannière de 210 pour une fenêtre de 600, ou bande unie.
    pub fn profile_full_banner(&self, has_banner: bool) -> f32 {
        if has_banner { (self.profile_full_width() * 0.35).round() } else { (self.profile_full_avatar() * 0.75).round() }
    }

    /// Profil complet : grand avatar (120 pour un avatar de carte de 80).
    pub fn profile_full_avatar(&self) -> f32 {
        (self.layout.profile_avatar * 1.5).round()
    }

    /// Profil complet (fenêtre) : deux fois la largeur de la carte.
    pub fn profile_full_width(&self) -> f32 {
        (self.layout.profile_width * 2.0).round()
    }

    /// Icône d'un badge de profil : un peu plus haute que le texte.
    pub fn profile_badge(&self) -> f32 {
        (self.typography.body * 1.4).round()
    }

    /// Ligne de salon : rectangle de l'icône et début du nom, avec `indent`
    /// en plus pour les salons rangés dans une catégorie.
    pub fn channel_row_parts(&self, row: egui::Rect, indent: f32) -> (egui::Rect, f32) {
        let side = Self::icon_side(self.typography.body);
        let left = row.left() + self.spacing.item_x + indent;
        let icon = egui::Rect::from_center_size(egui::pos2(left + side / 2.0, row.center().y), egui::Vec2::splat(side));
        (icon, icon.right() + self.spacing.item_x)
    }

    /// Haut d'un bloc de hauteur `content` centré dans `available`.
    pub fn centered_top(available: f32, content: f32) -> f32 {
        ((available - content) / 2.0).max(0.0).round()
    }

    /// Hauteur d'une ligne de compte : petit avatar, ou nom et détail sur
    /// deux lignes, avec l'espace vertical ordinaire autour.
    pub fn account_row_height(&self) -> f32 {
        let text = ((self.typography.body + self.typography.small) * 1.3).round();
        (text.max(self.layout.small_avatar) + 2.0 * self.spacing.item_y).max(self.spacing.channel_row)
    }

    /// Côté d'un bouton-icône pour un texte de taille `size` (icône + marge du bouton).
    pub fn icon_button_side(size: f32, padding: f32) -> f32 {
        Self::icon_side(size) + 2.0 * padding
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Fige la correspondance identifiant -> couleur : un auteur ne doit pas
    /// changer de teinte d'une version à l'autre.
    #[test]
    fn author_color_mapping_is_fixed() {
        let t = Preset::Rouille.tokens();
        let slot = |id: u64| t.colors.authors.iter().position(|c| *c == t.author_color(id));
        assert_eq!([slot(0), slot(1), slot(2), slot(3)], [Some(0), Some(4), Some(1), Some(6)]);
        assert_eq!([slot(42), slot(u64::MAX), slot(175_928_847_299_117_063)], [Some(7), Some(3), Some(4)]);
    }

    /// Les widgets egui standard prennent leurs couleurs, leurs rayons et
    /// leurs ombres dans les jetons, pour tous les préréglages.
    #[test]
    fn visuals_follow_the_tokens() {
        for p in Preset::ALL {
            let t = p.tokens().scaled();
            let (c, v) = (&t.colors, t.visuals());
            assert_eq!(v.dark_mode, !t.is_light(), "{}", p.key());
            assert_eq!((v.panel_fill, v.window_fill, v.extreme_bg_color), (c.bg2, c.menu_bg, c.bg0));
            assert_eq!((v.window_stroke, v.window_corner_radius), (t.border(), t.r_card()));
            assert_eq!((v.window_shadow.color, v.popup_shadow.color), (c.shadow, c.shadow));
            assert!(v.window_shadow.blur > v.popup_shadow.blur, "une fenêtre est plus haute qu'un menu");
            assert_eq!(v.hyperlink_color, c.link);
            assert_eq!(v.weak_text_color(), c.faint);
            let w = &v.widgets;
            assert_eq!((w.noninteractive.fg_stroke.color, w.noninteractive.bg_stroke.color), (c.widget_text, c.separator));
            assert_eq!((w.inactive.weak_bg_fill, w.inactive.fg_stroke.color), (c.button_bg, c.button_text));
            assert_eq!((w.hovered.weak_bg_fill, w.active.weak_bg_fill), (c.control_bg, c.selected));
            for s in [&w.noninteractive, &w.inactive, &w.hovered, &w.active, &w.open] {
                assert_eq!((s.corner_radius, s.expansion), (t.r_control(), 0.0));
            }
        }
    }

    /// La densité normale ne change aucun espacement, et le style d'egui
    /// reçoit les tailles et les marges des jetons.
    #[test]
    fn normal_density_changes_nothing_and_style_is_applied() {
        let t = Preset::Rouille.tokens();
        assert_eq!(t.scaled(), t);
        let ctx = egui::Context::default();
        t.apply(&ctx);
        let style = ctx.global_style();
        let size = |s: egui::TextStyle| style.text_styles.get(&s).map(|f| f.size);
        assert_eq!(size(egui::TextStyle::Small), Some(t.typography.small));
        assert_eq!(size(egui::TextStyle::Body), Some(t.typography.body));
        assert_eq!(size(egui::TextStyle::Button), Some(t.typography.button));
        assert_eq!(size(egui::TextStyle::Monospace), Some(t.typography.body));
        assert_eq!(style.spacing.item_spacing, egui::vec2(t.spacing.item_x, t.spacing.item_y));
        assert_eq!(style.spacing.button_padding, egui::vec2(t.spacing.control_x, t.spacing.control_y));
        // La hauteur minimale d'interaction reste celle d'egui (la disposition mobile la règle elle-même).
        assert_eq!(style.spacing.interact_size, egui::Style::default().spacing.interact_size);
    }

    /// Valeurs composées de Rouille (`docs/design.md`) : grille de 4 points,
    /// quatre rayons, en-têtes alignés.
    #[test]
    fn rouille_composites() {
        let t = Preset::Rouille.tokens().scaled();
        assert_eq!(t.rail_margin(), Margin::symmetric(12, 12));
        assert_eq!(t.rail_panel_width(), t.layout.rail_button + 2.0 * t.spacing.rail_x);
        assert_eq!(t.user_bar_margin(), Margin::symmetric(10, 10));
        assert_eq!(t.guild_title_margin(), Margin::symmetric(16, 10));
        assert_eq!(t.chat_head_margin(), Margin::symmetric(16, 10));
        assert_eq!(t.composer_margin(), Margin { left: 16, right: 16, top: 4, bottom: 16 });
        assert_eq!(t.composer_inner(), Margin::symmetric(14, 10));
        assert_eq!(t.message_margin(false), Margin { left: 16, right: 16, top: 16, bottom: 2 });
        assert_eq!(t.message_margin(true), Margin { left: 16, right: 16, top: 2, bottom: 2 });
        assert_eq!(t.channels_margin(), Margin::same(8));
        assert_eq!(t.embed_margin(), Margin::same(12));
        assert_eq!(t.card_margin(), Margin::same(28));
        assert_eq!(t.toast_margin(), Margin::same(10));
        assert_eq!((t.r_row(), t.r_control(), t.r_code()), (CornerRadius::same(6), CornerRadius::same(8), CornerRadius::same(6)));
        assert_eq!((t.r_embed(), t.r_toast()), (CornerRadius::same(6), CornerRadius::same(8)));
        assert_eq!((t.r_composer(), t.r_card()), (CornerRadius::same(8), CornerRadius::same(12)));
        // Pastille du rail : carré très arrondi, au repos comme ouverte.
        assert_eq!((t.r_rail(false), t.r_rail(true)), (CornerRadius::same(14), CornerRadius::same(14)));
        assert_eq!(t.r_hover(), CornerRadius::ZERO);
        assert_eq!(t.head_row(), 33.0);
        assert_eq!(t.dm_row(), 42.0);
        assert_eq!(t.toast_offset(), egui::vec2(0.0, -76.0));
        // Indicateur du rail : jamais plus haut que la pastille, jamais vide.
        let tile = egui::Rect::from_min_size(egui::pos2(12.0, 100.0), egui::Vec2::splat(48.0));
        for share in [-1.0, 0.0, 0.2, 0.5, 1.0, 3.0] {
            let (mark, _) = t.rail_mark(0.0, tile, share);
            assert!(mark.left() == 0.0 && mark.height() > 0.0 && mark.height() <= tile.height() && tile.y_range().contains(mark.center().y), "{share} : {mark:?}");
        }
    }

    #[test]
    fn density_scales_vertical_spacing_only() {
        let mut t = Preset::Rouille.tokens();
        t.layout.density = Density::Compact;
        let c = t.scaled();
        assert!(c.spacing.message_gap < t.spacing.message_gap);
        assert!(c.spacing.channel_row < t.spacing.channel_row);
        assert!(c.spacing.channel_row >= t.typography.body);
        assert_eq!(c.spacing.message_x, t.spacing.message_x);
        t.layout.density = Density::Airy;
        assert!(t.scaled().spacing.message_gap > Preset::Rouille.tokens().spacing.message_gap);
    }

    #[test]
    fn roundness_scales_every_radius() {
        let mut t = Preset::Rouille.tokens();
        t.shapes.roundness = 0.0;
        let s = t.scaled();
        assert_eq!(s.r_card(), CornerRadius::ZERO);
        assert_eq!(s.r_rail(false), CornerRadius::ZERO);
        // Aplat rouge : le texte posé dessus est lisible, quel que soit le préréglage.
        for p in Preset::ALL {
            let t = p.tokens();
            assert!(contrast_ratio(t.colors.on_accent, t.danger_fill()) >= MIN_TEXT, "{}", p.key());
        }
        t.shapes.roundness = 2.0;
        assert_eq!(t.scaled().r_composer(), CornerRadius::same(16));
    }

    #[test]
    fn sanitized_clamps_everything() {
        let mut t = Preset::Rouille.tokens();
        t.layout.channels_width = 10_000.0;
        t.layout.zoom = f32::NAN;
        t.typography.body = -3.0;
        t.spacing.toast_offset = f32::INFINITY;
        let s = t.sanitized();
        assert_eq!(s.layout.channels_width, CHANNELS_WIDTH.max);
        assert_eq!(s.layout.zoom, ZOOM.min);
        assert_eq!(s.typography.body, 8.0);
        assert_eq!(s.spacing.toast_offset, 0.0);
    }

    /// Ligne du rail en liste : aucune taille négative, même aux extrêmes.
    #[test]
    fn rail_row_never_negative() {
        let mut t = Preset::Rouille.tokens();
        for (rail_x, row) in [(0.0, 20.0), (32.0, 20.0), (32.0, 64.0), (0.0, 64.0), (16.0, 20.0)] {
            t.spacing.rail_x = rail_x;
            t.layout.rail_list_row = row;
            let r = t.rail_row();
            assert!(r.pad >= 0.0 && r.badge >= row / 2.0 - f32::EPSILON, "{rail_x} {row} -> {r:?}");
            assert!(r.badge + 2.0 * r.pad <= row + f32::EPSILON);
        }
    }

    /// Toutes les bornes à la fois (tout au minimum, tout au maximum) : aucune
    /// géométrie dérivée n'est négative ni ne sort de sa ligne.
    #[test]
    fn derived_geometry_at_extremes() {
        for high in [false, true] {
            let mut t = Preset::Rouille.tokens();
            for f in t.fields_mut() {
                if let FieldMut::Number(v, b) = f.value {
                    *v = if high { b.max } else { b.min };
                }
            }
            let t = t.scaled();
            let r = t.rail_row();
            assert!(r.pad >= 0.0 && r.badge >= 0.0, "{r:?}");
            let row = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(t.layout.channels_width, t.spacing.channel_row));
            let (mark, _) = t.unread_mark(row);
            assert!(mark.width() >= 0.0 && mark.height() >= 0.0 && row.contains_rect(mark), "{mark:?} hors de {row:?}");
            let (icon, text_left) = t.channel_row_parts(row, t.spacing.channel_indent);
            assert!(icon.width() > 0.0 && text_left > icon.left());
            assert!(Tokens::centered_top(row.height(), row.height() * 3.0) == 0.0);
        }
    }

    #[test]
    fn avatar_geometry_and_colors() {
        let mut t = Preset::Rouille.tokens();
        assert_eq!(t.r_avatar(36.0), CornerRadius::same(18));
        t.layout.avatar_shape = AvatarShape::Square;
        assert_eq!(t.r_avatar(36.0), CornerRadius::same(10));
        assert_eq!(t.r_avatar(10.0), CornerRadius::same(5), "jamais plus que la moitié du côté");
        t.shapes.roundness = 0.0;
        assert_eq!(t.scaled().r_avatar(36.0), CornerRadius::ZERO);
        assert!(Preset::Rouille.tokens().message_avatars());
        t.layout.avatars = Avatars::Hidden;
        assert!(!t.message_avatars());
        assert!(!t.avatars_shown());
        assert!(!Preset::Compact.tokens().message_avatars(), "une ligne : pas d'avatar");
        assert_eq!(Preset::Rouille.tokens().small_avatar_in(10.0), 10.0);
        // Initiales lisibles sur chaque avatar par défaut, pour tous les préréglages.
        for p in Preset::ALL {
            let t = p.tokens();
            for i in 0..crate::state::model::DEFAULT_AVATARS {
                let (fill, text) = t.default_avatar(i);
                let r = contrast_ratio(text, fill);
                assert!(r >= 3.0, "{} : avatar {i} : {r:.2}", p.key());
            }
        }
    }

    #[test]
    fn every_field_has_unique_key_and_sane_bounds() {
        let mut t = Preset::Rouille.tokens();
        let mut seen = std::collections::HashSet::new();
        for f in t.fields_mut() {
            assert!(seen.insert((f.section, f.key)), "{}.{} en double", f.section, f.key);
            // (Le libellé tient sur une ligne : test `token_labels_fit_one_row` d'i18n.)
            assert!(SECTIONS.iter().any(|(s, _)| *s == f.section));
            for l in crate::i18n::Lang::ALL {
                assert!(!f.text.label(l).is_empty() && !f.text.detail(l).is_empty(), "{}.{} : texte vide", f.section, f.key);
            }
            if let FieldMut::Number(v, b) = f.value {
                assert!(b.min < b.max && b.step > 0.0, "{}.{}", f.section, f.key);
                assert!((b.min..=b.max).contains(v), "{}.{} = {v} hors de ses bornes", f.section, f.key);
            }
        }
        for e in ESSENTIALS {
            assert!(seen.contains(e), "réglage essentiel inconnu : {e:?}");
        }
        // Chaque texte de `i18n::tokens` sert à un jeton (et chaque jeton a le sien : sinon, rien ne compile).
        assert_eq!(seen.len(), crate::i18n::tokens::ALL.len());
    }
}
