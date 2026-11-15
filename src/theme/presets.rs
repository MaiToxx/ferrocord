//! Préréglages fournis. Rouille est le thème d'origine, valeur pour valeur.

use super::{AvatarShape, FontFace, Clock, Colors, Density, Layout, Avatars, MediaLoad, MembersPanel, MessageMode, RailStyle, Shapes, Spacing, StatusDots, TitleBar, Tokens, TypingSignal, Typography, Markdown};
use crate::i18n::{t, Lang, Text};
use eframe::egui::Color32;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    /// Calqué sur le thème « Dark » du client de bureau : le préréglage par défaut.
    Sombre,
    /// Calqué sur « Ash » (le thème sombre d'avant 2025).
    Cendre,
    /// Calqué sur « Onyx » (noir).
    Onyx,
    Rouille,
    Minuit,
    Clair,
    Contraste,
    Compact,
    Aere,
}

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

impl Preset {
    pub const ALL: [Preset; 9] = [Preset::Sombre, Preset::Cendre, Preset::Onyx, Preset::Clair, Preset::Rouille, Preset::Minuit, Preset::Contraste, Preset::Compact, Preset::Aere];
    /// Préréglage d'une installation neuve (aucun `theme.toml`).
    pub const DEFAULT: Preset = Preset::Sombre;

    /// Nom dans `theme.toml` (`base = "…"`).
    pub fn key(self) -> &'static str {
        match self {
            Preset::Sombre => "sombre",
            Preset::Cendre => "cendre",
            Preset::Onyx => "onyx",
            Preset::Rouille => "rouille",
            Preset::Minuit => "minuit",
            Preset::Clair => "clair",
            Preset::Contraste => "contraste",
            Preset::Compact => "compact",
            Preset::Aere => "aere",
        }
    }

    /// Nom affiché.
    pub fn name(self) -> Text {
        match self {
            Preset::Sombre => t::PRESET_SOMBRE,
            Preset::Cendre => t::PRESET_CENDRE,
            Preset::Onyx => t::PRESET_ONYX,
            Preset::Rouille => t::PRESET_ROUILLE,
            Preset::Minuit => t::PRESET_MINUIT,
            Preset::Clair => t::PRESET_CLAIR,
            Preset::Contraste => t::PRESET_CONTRASTE,
            Preset::Compact => t::PRESET_COMPACT,
            Preset::Aere => t::PRESET_AERE,
        }
    }

    pub fn label(self, lang: Lang) -> &'static str {
        self.name().get(lang)
    }

    /// Accepte la clé du fichier ou le libellé (dans l'une des langues),
    /// sans tenir compte de la casse.
    pub fn from_name(name: &str) -> Option<Preset> {
        let n = name.trim().to_lowercase();
        Self::ALL.into_iter().find(|p| p.key() == n || Lang::ALL.iter().any(|l| p.label(*l).to_lowercase() == n))
    }

    /// Préréglage calqué sur le client de bureau (géométrie de `discord()`).
    pub(super) fn calque(self) -> bool {
        matches!(self, Preset::Sombre | Preset::Cendre | Preset::Onyx | Preset::Clair)
    }

    pub fn tokens(self) -> Tokens {
        let mut t = if self.calque() { discord() } else { rouille() };
        t.base = self;
        self.paint(&mut t);
        t
    }

    /// Ce que valait ce préréglage dans un fichier de version 2 (avant les
    /// préréglages calqués) : la géométrie de Rouille. Les jetons nés depuis
    /// ont leur valeur de Rouille, celle qu'un tel fichier reçoit à la lecture.
    pub(super) fn tokens_v2(self) -> Tokens {
        let mut t = rouille();
        (t.shapes.rail, t.shapes.rail_selected) = V3_RAIL;
        t.layout.status_dots = StatusDots::Shown;
        t.base = self;
        self.paint(&mut t);
        t
    }

    /// Ce que valait ce préréglage dans un fichier de version 3 : celui
    /// d'aujourd'hui, avec les pastilles rondes du rail (carrées arrondies une
    /// fois ouvertes) des préréglages à la géométrie de Rouille.
    pub(super) fn tokens_v3(self) -> Tokens {
        let mut t = self.tokens_v4();
        if matches!(self, Preset::Rouille | Preset::Minuit | Preset::Contraste) {
            (t.shapes.rail, t.shapes.rail_selected) = V3_RAIL;
        }
        t
    }

    /// Ce que valait ce préréglage dans un fichier de version 4 : celui
    /// d'aujourd'hui, avec la pastille de statut sur les avatars des messages
    /// (le client n'en met pas : elle est masquée depuis le lot « chatmp ») et
    /// la zone de saisie plus basse des préréglages calqués.
    pub(super) fn tokens_v4(self) -> Tokens {
        let mut t = self.tokens();
        t.layout.status_dots = StatusDots::Shown;
        // Zone de saisie des préréglages calqués : 50 de haut avant, 56 depuis.
        if t.spacing.composer_inner_y == 14.0 {
            t.spacing.composer_inner_y = 11.0;
        }
        t
    }

    /// Couleurs et retouches propres au préréglage, posées sur sa géométrie.
    fn paint(self, t: &mut Tokens) {
        match self {
            Preset::Rouille => {}
            Preset::Sombre => {
                let c = &mut t.colors;
                // Rail et colonne des salons du même fond ; la discussion, plus claire.
                c.bg0 = rgb(0x121214);
                c.bg1 = rgb(0x121214);
                c.bg2 = rgb(0x1a1a1e);
                c.bg3 = rgb(0x222327);
                c.border = rgb(0x2f2f34);
                c.pill = rgb(0x222327);
                c.selected = rgb(0x29292e);
                c.row_hover = rgb(0x1f1f23);
                c.text = rgb(0xdfe0e2);
                c.text_strong = rgb(0xfbfbfb);
                c.muted = rgb(0xaaaab1);
                // Un cran plus clair que dans le client : 4,5:1 sur la ligne ouverte.
                c.faint = rgb(0x94959c);
                c.accent = rgb(0x5865f2);
                c.danger = rgb(0xfa777c);
                c.hover = Color32::from_white_alpha(8);
                c.widget_text = rgb(0xaaaab1);
                c.button_bg = rgb(0x2c2c31);
                c.button_text = rgb(0xfbfbfb);
                c.control_bg = rgb(0x37373d);
                c.separator = rgb(0x2c2c30);
                c.scrim = Color32::from_black_alpha(217);
                c.shadow = Color32::from_black_alpha(140);
                c.panel_border = Color32::from_white_alpha(20);
                c.menu_bg = rgb(0x28282d);
                c.menu_hover = rgb(0x36363b);
                c.channel_idle = rgb(0x82838b);
                let m = &mut t.markdown;
                m.code_bg = rgb(0x121214);
                m.mention_text = rgb(0xc9cdfb);
                m.spoiler = rgb(0x2e2e33);
            }
            Preset::Onyx => {
                let c = &mut t.colors;
                c.bg0 = rgb(0x000000);
                c.bg1 = rgb(0x070709);
                c.bg2 = rgb(0x0a0a0c);
                c.bg3 = rgb(0x131416);
                c.border = rgb(0x26262b);
                c.pill = rgb(0x131416);
                c.selected = rgb(0x202024);
                c.row_hover = rgb(0x151518);
                c.text = rgb(0xdfe0e2);
                c.text_strong = rgb(0xfbfbfb);
                c.muted = rgb(0xaaaab1);
                c.faint = rgb(0x8b8c94);
                c.accent = rgb(0x5865f2);
                c.danger = rgb(0xfa777c);
                c.hover = Color32::from_white_alpha(10);
                c.widget_text = rgb(0xaaaab1);
                c.button_bg = rgb(0x222226);
                c.button_text = rgb(0xfbfbfb);
                c.control_bg = rgb(0x2e2e33);
                c.separator = rgb(0x1e1e1f);
                c.scrim = Color32::from_black_alpha(217);
                c.shadow = Color32::from_black_alpha(160);
                c.panel_border = Color32::from_white_alpha(20);
                c.menu_bg = rgb(0x131416);
                c.menu_hover = rgb(0x26262b);
                c.channel_idle = rgb(0x82838b);
                let m = &mut t.markdown;
                m.code_bg = rgb(0x131416);
                m.mention_text = rgb(0xc9cdfb);
                m.spoiler = rgb(0x26262b);
            }
            Preset::Cendre => {
                // Les fonds d'« Ash » ne tiennent pas le contraste AA (accent et
                // texte discret sur la discussion) : ce sont ceux de Rouille, un
                // cran plus sombres. Le reste suit le client d'avant 2025 : pas de
                // filet entre les zones, petits rayons, pastilles rondes de 48.
                let c = &mut t.colors;
                c.text_strong = rgb(0xf2f3f5);
                c.muted = rgb(0xb5bac1);
                c.button_bg = rgb(0x4e5058);
                c.control_bg = rgb(0x62646e);
                c.scrim = Color32::from_black_alpha(217);
                c.panel_border = Color32::TRANSPARENT;
                c.menu_bg = rgb(0x111214);
                // Ligne de menu survolée : aplat d'accent, comme dans « Ash ».
                c.menu_hover = c.accent;
                c.channel_idle = rgb(0x949ba4);
                t.markdown.spoiler = rgb(0x1e1f22);
                let sh = &mut t.shapes;
                sh.card = 8.0;
                sh.rail = 24.0;
                sh.rail_selected = 16.0;
                sh.row = 4.0;
                sh.control = 4.0;
                sh.embed = 4.0;
                t.markdown.code_radius = 4.0;
                t.spacing.rail_x = 12.0;
                t.layout.rail_button = 48.0;
            }
            Preset::Minuit => {
                let c = &mut t.colors;
                c.bg0 = rgb(0x000000);
                c.bg1 = rgb(0x0a0a0c);
                c.bg2 = rgb(0x000000);
                c.bg3 = rgb(0x1a1b20);
                c.border = rgb(0x2a2b31);
                c.pill = rgb(0x1a1b20);
                c.selected = rgb(0x1f2027);
                c.row_hover = rgb(0x141518);
                c.text = rgb(0xe4e5ea);
                c.text_strong = rgb(0xffffff);
                c.muted = rgb(0xa3a5b0);
                c.faint = rgb(0x8d8f99);
                c.accent = rgb(0x5865f2);
                c.on_accent = rgb(0xffffff);
                c.danger = rgb(0xff6a70);
                c.link = rgb(0x3db8ff);
                c.hover = Color32::from_white_alpha(8);
                c.widget_text = rgb(0xb4b6c0);
                c.button_bg = rgb(0x1c1d22);
                c.button_text = rgb(0xdcdde2);
                c.control_bg = rgb(0x2a2b31);
                c.separator = rgb(0x1f2025);
                c.shadow = Color32::from_black_alpha(160);
                c.authors = [
                    rgb(0xff8f5a),
                    rgb(0x4cd391),
                    rgb(0x6f9bff),
                    rgb(0xe07ae4),
                    rgb(0xf0c55a),
                    rgb(0x55d3d9),
                    rgb(0xff7a86),
                    rgb(0xa99aff),
                ];
                markdown_colors(
                    t,
                    [
                        rgb(0x0e0f12),
                        Color32::from_rgba_unmultiplied(0x58, 0x65, 0xf2, 0x4d),
                        rgb(0xd5d9ff),
                        rgb(0x1c1d22),
                        rgb(0x3a3c45),
                        rgb(0xa99aff),
                        rgb(0x7fe0a6),
                        rgb(0xffc37a),
                        rgb(0x8a8d98),
                    ],
                );
            }
            Preset::Clair => {
                let c = &mut t.colors;
                c.bg0 = rgb(0xe3e5e8);
                c.bg1 = rgb(0xf2f3f5);
                c.bg2 = rgb(0xffffff);
                c.bg3 = rgb(0xebedef);
                c.border = rgb(0xd0d2d6);
                c.pill = rgb(0xffffff);
                c.selected = rgb(0xd7d9dc);
                c.row_hover = rgb(0xe1e3e6);
                c.text = rgb(0x313338);
                c.text_strong = rgb(0x060607);
                c.muted = rgb(0x4e5058);
                c.faint = rgb(0x5c5e66);
                c.accent = rgb(0x5865f2);
                c.on_accent = rgb(0xffffff);
                c.danger = rgb(0xad2222);
                c.link = rgb(0x0068e0);
                c.hover = Color32::from_black_alpha(8);
                c.widget_text = rgb(0x4e5058);
                c.button_bg = rgb(0xd9dbde);
                c.button_text = rgb(0x313338);
                c.control_bg = rgb(0xc7c9cd);
                c.separator = rgb(0xe1e2e4);
                c.scrim = Color32::from_black_alpha(120);
                c.shadow = Color32::from_black_alpha(46);
                c.focus = rgb(0x0b5cd5);
                c.online = rgb(0x248046);
                c.idle = rgb(0xa87708);
                c.dnd = rgb(0xc62828);
                c.offline = rgb(0x747780);
                c.panel_border = Color32::from_black_alpha(22);
                c.menu_bg = rgb(0xffffff);
                c.menu_hover = rgb(0xe1e3e6);
                c.channel_idle = rgb(0x5c5e66);
                c.authors = [
                    rgb(0xb24a16),
                    rgb(0x1b7a4b),
                    rgb(0x2f5fc4),
                    rgb(0x9a3aa0),
                    rgb(0x855600),
                    rgb(0x0f7279),
                    rgb(0xbf3346),
                    rgb(0x6247c9),
                ];
                markdown_colors(
                    t,
                    [
                        rgb(0xf2f3f5),
                        Color32::from_rgba_unmultiplied(0x58, 0x65, 0xf2, 0x26),
                        rgb(0x3b45b0),
                        rgb(0xc4c9ce),
                        rgb(0xc4c9ce),
                        rgb(0x4752c4),
                        rgb(0x1b6e43),
                        rgb(0x9a4a00),
                        rgb(0x5c5e66),
                    ],
                );
                // Fond clair : un ambre plus soutenu, pour que le fond et la barre se voient.
                t.markdown.mention_row = Color32::from_rgba_unmultiplied(0xc8, 0x7a, 0x00, 0x2e);
            }
            Preset::Contraste => {
                let c = &mut t.colors;
                c.bg0 = rgb(0x000000);
                c.bg1 = rgb(0x0a0a0a);
                c.bg2 = rgb(0x000000);
                c.bg3 = rgb(0x1f1f1f);
                c.border = rgb(0x9a9a9a);
                c.pill = rgb(0x262626);
                c.selected = rgb(0x3a3a3a);
                c.row_hover = rgb(0x1f1f1f);
                c.text = rgb(0xffffff);
                c.text_strong = rgb(0xffffff);
                c.muted = rgb(0xe0e0e0);
                c.faint = rgb(0xc4c4c4);
                c.accent = rgb(0xffc21a);
                c.on_accent = rgb(0x000000);
                c.danger = rgb(0xff8080);
                c.link = rgb(0x7fd4ff);
                c.hover = Color32::from_white_alpha(28);
                c.widget_text = rgb(0xf0f0f0);
                c.button_bg = rgb(0x1a1a1a);
                c.button_text = rgb(0xffffff);
                c.control_bg = rgb(0x3a3a3a);
                c.separator = rgb(0xb0b0b0);
                c.shadow = Color32::from_black_alpha(200);
                c.focus = rgb(0x7fd4ff);
                c.authors = [
                    rgb(0xffa070),
                    rgb(0x5cf0a8),
                    rgb(0x8fb6ff),
                    rgb(0xf29cf5),
                    rgb(0xffd866),
                    rgb(0x6ff0f5),
                    rgb(0xff9aa4),
                    rgb(0xc4b8ff),
                ];
                t.shapes.border = 2.0;
                t.shapes.unread_ring = 4.0;
                t.shapes.mention_dot = 5.0;
                markdown_colors(
                    t,
                    [
                        rgb(0x0a0a0a),
                        Color32::from_rgba_unmultiplied(0xff, 0xc2, 0x1a, 0x40),
                        rgb(0xffe08a),
                        rgb(0x3a3a3a),
                        rgb(0xb0b0b0),
                        rgb(0xffc21a),
                        rgb(0x5cf0a8),
                        rgb(0xf29cf5),
                        rgb(0xc4c4c4),
                    ],
                );
            }
            Preset::Compact => {
                // Style IRC : une ligne par message, peu d'air, mêmes couleurs que Rouille.
                t.layout.density = Density::Compact;
                t.layout.messages = MessageMode::Compact;
                t.layout.rail_button = 36.0;
                t.layout.rail_width = 52.0;
                t.layout.channels_width = 208.0;
                t.layout.avatar_size = 28.0;
                t.layout.small_avatar = 20.0;
                t.typography.body = 13.0;
                t.typography.button = 12.5;
                t.typography.small = 11.0;
                t.typography.rail = 12.0;
                t.typography.title = 14.5;
                t.typography.icon = 17.0;
                t.shapes.rail = 11.0;
                t.shapes.rail_selected = 11.0;
                t.shapes.composer = 8.0;
                t.shapes.embed = 6.0;
                t.spacing.rail_x = 8.0;
                t.spacing.control_x = 9.0;
                t.spacing.message_x = 12.0;
                t.spacing.chat_head_x = 12.0;
                t.spacing.composer_x = 12.0;
            }
            Preset::Aere => {
                t.layout.density = Density::Airy;
                t.layout.rail_button = 52.0;
                t.layout.rail_width = 80.0;
                t.layout.channels_width = 272.0;
                t.layout.avatar_size = 44.0;
                t.layout.small_avatar = 30.0;
                t.spacing.avatar_gap = 16.0;
                t.spacing.rail_x = 14.0;
                t.typography.body = 15.0;
                t.typography.button = 14.5;
                t.typography.small = 12.0;
                t.typography.title = 17.5;
                t.typography.icon = 22.0;
                t.typography.rail = 16.0;
                t.shapes.rail = 18.0;
                t.shapes.rail_selected = 18.0;
                t.shapes.composer = 14.0;
                t.shapes.embed = 10.0;
                t.shapes.hover = 8.0;
                t.spacing.message_x = 24.0;
                t.spacing.chat_head_x = 24.0;
                t.spacing.composer_x = 24.0;
                t.spacing.item_x = 10.0;
            }
        }
        // Avant les préréglages calqués : menus sur le fond principal, filet
        // ordinaire entre les zones, salons au repos en texte secondaire.
        if !self.calque() {
            let c = &mut t.colors;
            (c.panel_border, c.menu_bg, c.menu_hover, c.channel_idle) = (c.separator, c.bg2, c.control_bg, c.muted);
        }
    }
}

/// Rayons du rail (`rail`, `rail_selected`) de la géométrie de Rouille jusqu'à
/// la version 3 des fichiers de thème : rond au repos, carré arrondi une fois
/// survolé ou ouvert.
const V3_RAIL: (f32, f32) = (24.0, 16.0);

/// Thème sombre par défaut (clé `rouille`, gardée pour les fichiers existants) :
/// gris neutres-froids et accent bleu-violet (`docs/design.md`).
fn rouille() -> Tokens {
    Tokens {
        base: Preset::Rouille,
        colors: Colors {
            // Quatre gris neutres-froids, du plus sombre (rail) au plus clair (saisie).
            bg0: rgb(0x1a1b1e),
            bg1: rgb(0x232428),
            bg2: rgb(0x2b2d31),
            bg3: rgb(0x34363c),
            border: rgb(0x3d3f46),
            pill: rgb(0x2b2d31),
            selected: rgb(0x393b41),
            row_hover: rgb(0x31333a),
            text: rgb(0xdbdee1),
            text_strong: rgb(0xf7f8f9),
            muted: rgb(0xa9aeb7),
            faint: rgb(0xa2a7b0),
            accent: rgb(0x5966f2),
            on_accent: rgb(0xffffff),
            danger: rgb(0xfb8389),
            link: rgb(0x00a8fc),
            hover: Color32::from_black_alpha(22),
            widget_text: rgb(0xb5bac1),
            button_bg: rgb(0x4a4c54),
            button_text: rgb(0xf2f3f5),
            control_bg: rgb(0x5a5c66),
            separator: rgb(0x35373d),
            scrim: Color32::from_black_alpha(180),
            shadow: Color32::from_black_alpha(110),
            focus: rgb(0x00a8fc),
            online: rgb(0x23a55a),
            idle: rgb(0xf0b232),
            dnd: rgb(0xf23f43),
            offline: rgb(0x80848e),
            authors: [
                rgb(0xee8450),
                rgb(0x56c690),
                rgb(0x6e9df2),
                rgb(0xd67fda),
                rgb(0xe3ba52),
                rgb(0x55c5cb),
                rgb(0xf07e8a),
                rgb(0xa797f6),
            ],
            panel_border: rgb(0x35373d),
            menu_bg: rgb(0x2b2d31),
            menu_hover: rgb(0x5a5c66),
            channel_idle: rgb(0xa9aeb7),
        },
        typography: Typography { body: 15.0, small: 12.0, button: 14.0, title: 16.0, icon: 20.0, rail: 15.0, logo: 30.0, line_height: 0.0, family: FontFace::Figtree },
        shapes: Shapes {
            roundness: 1.0,
            card: 12.0,
            // Carré très arrondi (30 % du côté), comme les icônes de serveur du client.
            rail: 14.0,
            rail_selected: 14.0,
            row: 6.0,
            control: 8.0,
            composer: 8.0,
            embed: 6.0,
            embed_bar: 4.0,
            toast: 8.0,
            hover: 0.0,
            avatar: 10.0,
            border: 1.0,
            unread_ring: 4.0,
            mention_dot: 4.0,
            status_dot: 5.0,
            focus: 2.0,
        },
        // Grille de 4 points.
        spacing: Spacing {
            item_x: 8.0,
            item_y: 4.0,
            control_x: 12.0,
            control_y: 6.0,
            rail_x: 12.0,
            rail_y: 12.0,
            rail_gap: 8.0,
            channels_pad: 8.0,
            category_gap: 16.0,
            channel_row: 32.0,
            channel_indent: 0.0,
            mention_inset: 12.0,
            user_bar_x: 10.0,
            user_bar_y: 10.0,
            guild_title_x: 16.0,
            guild_title_y: 10.0,
            chat_head_x: 16.0,
            chat_head_y: 10.0,
            composer_x: 16.0,
            composer_top: 4.0,
            composer_bottom: 16.0,
            composer_inner_x: 14.0,
            composer_inner_y: 10.0,
            message_x: 16.0,
            message_gap: 16.0,
            message_gap_grouped: 2.0,
            avatar_gap: 14.0,
            chat_pad: 12.0,
            embed_pad: 12.0,
            media_gap: 6.0,
            compact_indent: 48.0,
            card_pad: 28.0,
            login_title_gap: 20.0,
            login_field_gap: 8.0,
            login_button_gap: 12.0,
            login_error_gap: 8.0,
            login_notice_gap: 16.0,
            toast_pad: 10.0,
            toast_offset: 76.0,
            member_row: 42.0,
            member_header: 40.0,
            // 0 : hauteur d'en-tête déduite du texte et des marges, comme avant.
            header_height: 0.0,
            message_right: 16.0,
            control_height: 20.0,
        },
        layout: Layout {
            zoom: 1.0,
            density: Density::Normal,
            messages: MessageMode::Grouped,
            clock: Clock::H24,
            rail_style: RailStyle::Pills,
            avatars: Avatars::Shown,
            avatar_shape: AvatarShape::Round,
            media_load: MediaLoad::Auto,
            media_width: 400.0,
            media_height: 300.0,
            embed_width: 520.0,
            embed_thumb: 80.0,
            avatar_size: 40.0,
            small_avatar: 32.0,
            rail_width: 72.0,
            rail_list_width: 220.0,
            rail_button: 48.0,
            rail_list_row: 40.0,
            channels_width: 240.0,
            members: MembersPanel::Shown,
            members_width: 240.0,
            card_width: 400.0,
            button_height: 40.0,
            composer_rows: 10.0,
            typing: TypingSignal::Sent,
            status_dots: StatusDots::Hidden,
            profile_width: 320.0,
            profile_avatar: 80.0,
            titlebar: TitleBar::Integrated,
            titlebar_height: 32.0,
        },
        markdown: Markdown {
            code_bg: rgb(0x1e1f22),
            mention_bg: Color32::from_rgba_unmultiplied(0x58, 0x65, 0xf2, 0x4d),
            mention_text: rgb(0xd5d9ff),
            mention_row: Color32::from_rgba_unmultiplied(0xf0, 0xb2, 0x32, 0x1a),
            spoiler: rgb(0x111214),
            quote_bar: rgb(0x4e5058),
            keyword: rgb(0x8ea1ff),
            string: rgb(0x7ed4a0),
            number: rgb(0xf0b27a),
            comment: rgb(0x8b919b),
            h1: 24.0,
            h2: 20.0,
            h3: 17.0,
            quote_width: 3.0,
            quote_gap: 12.0,
            list_indent: 20.0,
            code_pad: 10.0,
            code_radius: 6.0,
            emoji: 20.0,
            jumbo: 48.0,
            picker_emoji: 32.0,
            sticker: 160.0,
        },
    }
}

/// Géométrie du client de bureau de 2025 (thème « Dark »), posée sur Rouille :
/// texte de 16 à l'interligne de 22, en-têtes de 48, contrôles de 32, rayons
/// de 8, pastilles du rail de 40 en carré arrondi (`docs/ecarts-visuels.md`).
fn discord() -> Tokens {
    let mut t = rouille();
    let ty = &mut t.typography;
    ty.body = 16.0;
    ty.icon = 24.0;
    ty.line_height = 22.0;
    let sh = &mut t.shapes;
    sh.rail = 12.0;
    sh.rail_selected = 12.0;
    sh.row = 8.0;
    sh.embed = 8.0;
    let s = &mut t.spacing;
    s.control_x = 16.0;
    s.rail_x = 16.0;
    s.guild_title_y = 8.0;
    s.chat_head_y = 8.0;
    s.header_height = 48.0;
    // Zone de saisie de 56 de haut (une ligne de boutons de 28 et ses marges), comme le client.
    s.composer_inner_y = 14.0;
    // 17 entre deux groupes, plus les 2 de marge de chaque message.
    s.message_gap = 19.0;
    s.message_right = 48.0;
    s.avatar_gap = 16.0;
    s.media_gap = 4.0;
    s.card_pad = 24.0;
    s.control_height = 32.0;
    let l = &mut t.layout;
    l.media_width = 550.0;
    l.media_height = 350.0;
    l.rail_button = 40.0;
    l.card_width = 440.0;
    l.button_height = 44.0;
    l.profile_width = 300.0;
    let m = &mut t.markdown;
    m.h3 = 16.0;
    m.quote_width = 4.0;
    m.code_pad = 8.0;
    m.emoji = 22.0;
    t
}

/// Couleurs du texte mis en forme propres à un préréglage (le reste vient de Rouille).
fn markdown_colors(t: &mut Tokens, colors: [Color32; 9]) {
    let m = &mut t.markdown;
    [m.code_bg, m.mention_bg, m.mention_text, m.spoiler, m.quote_bar, m.keyword, m.string, m.number, m.comment] = colors;
}

#[cfg(test)]
mod tests {
    use super::super::contrast_ratio;
    use super::*;

    #[test]
    fn names_round_trip() {
        for p in Preset::ALL {
            assert_eq!(Preset::from_name(p.key()), Some(p));
            assert!(Lang::ALL.iter().all(|l| Preset::from_name(p.label(*l)) == Some(p)));
            assert_eq!(p.tokens().base, p);
        }
        assert_eq!(Preset::from_name(" CLAIR "), Some(Preset::Clair));
        assert_eq!(Preset::from_name("inconnu"), None);
    }

    /// WCAG : 4,5:1 pour le texte courant, 3:1 pour le texte atténué, sur
    /// chaque fond où ce texte apparaît, pour tous les préréglages.
    #[test]
    fn presets_meet_contrast() {
        for p in Preset::ALL {
            let t = p.tokens();
            let c = &t.colors;
            let mut fails = Vec::new();
            let mut check = |what: &str, fg: Color32, bg: Color32, min: f64| {
                let r = contrast_ratio(fg, bg);
                if r < min {
                    fails.push(format!("{what} : {r:.2} < {min}"));
                }
            };
            for (bg_name, bg) in [("bg0", c.bg0), ("bg1", c.bg1), ("bg2", c.bg2)] {
                check(&format!("text/{bg_name}"), c.text, bg, 4.5);
                check(&format!("text_strong/{bg_name}"), c.text_strong, bg, 4.5);
                check(&format!("widget_text/{bg_name}"), c.widget_text, bg, 4.5);
                check(&format!("muted/{bg_name}"), c.muted, bg, 3.0);
                check(&format!("faint/{bg_name}"), c.faint, bg, 3.0);
            }
            check("link/bg2", c.link, c.bg2, 4.5);
            check("on_accent/accent", c.on_accent, c.accent, 4.5);
            // Initiales de la pastille d'un compte importé pas encore vérifié.
            let (fill, initials) = t.placeholder_avatar();
            check("placeholder_avatar (initiales/fond)", initials, fill, 4.5);
            check("button_text/button_bg", c.button_text, c.button_bg, 4.5);
            check("danger/bg1", c.danger, c.bg1, 3.0);
            check("danger/bg2", c.danger, c.bg2, 3.0);
            check("accent/bg2", c.accent, c.bg2, 3.0);
            check("text/bg3", c.text, c.bg3, 4.5);
            // Raison affichée dans la zone de saisie fermée (texte à lire, pas un simple indice).
            check("muted/bg3", c.muted, c.bg3, 4.5);
            check("text_strong/pill", c.text_strong, c.pill, 4.5);
            check("text_strong/selected", c.text_strong, c.selected, 4.5);
            check("text_strong/row_hover", c.text_strong, c.row_hover, 4.5);
            check("muted/row_hover", c.muted, c.row_hover, 3.0);
            check("muted/selected", c.muted, c.selected, 3.0);
            for (i, a) in c.authors.iter().enumerate() {
                check(&format!("authors[{i}]/bg2"), *a, c.bg2, 4.5);
            }
            // Pastilles de statut sur le fond de la liste des membres (élément graphique : 3:1).
            for (name, s) in [("online", c.online), ("idle", c.idle), ("dnd", c.dnd), ("offline", c.offline)] {
                check(&format!("{name}/bg1"), s, c.bg1, 3.0);
            }
            assert!(fails.is_empty(), "{} : {fails:#?}", p.key());
        }
    }

    /// Texte mis en forme lisible : coloration sur le fond du code, mention
    /// sur son fond (posé sur le fond principal), spoiler masqué visible.
    #[test]
    fn markdown_colors_meet_contrast() {
        let over = |fg: Color32, bg: Color32| {
            let a = f32::from(fg.a()) / 255.0;
            let mix = |f: u8, b: u8| (f32::from(f) * a + f32::from(b) * (1.0 - a)).round() as u8;
            let fg = fg.to_srgba_unmultiplied();
            Color32::from_rgb(mix(fg[0], bg.r()), mix(fg[1], bg.g()), mix(fg[2], bg.b()))
        };
        for p in Preset::ALL {
            let t = p.tokens();
            let (c, m) = (&t.colors, &t.markdown);
            let mut fails = Vec::new();
            for (what, fg, bg, min) in [
                ("text/code_bg", c.text, m.code_bg, 4.5),
                ("keyword/code_bg", m.keyword, m.code_bg, 4.5),
                ("string/code_bg", m.string, m.code_bg, 4.5),
                ("number/code_bg", m.number, m.code_bg, 4.5),
                ("comment/code_bg", m.comment, m.code_bg, 3.0),
                ("danger/code_bg", c.danger, m.code_bg, 3.0),
                ("mention_text/mention_bg", m.mention_text, over(m.mention_bg, c.bg2), 4.5),
                ("spoiler/bg2", m.spoiler, c.bg2, 1.15),
                ("quote_bar/bg2", m.quote_bar, c.bg2, 1.5),
            ] {
                let r = contrast_ratio(fg, bg);
                if r < min {
                    fails.push(format!("{what} : {r:.2} < {min}"));
                }
            }
            assert!(fails.is_empty(), "{} : {fails:#?}", p.key());
        }
    }

    /// Chaque préréglage respecte les bornes des jetons.
    #[test]
    fn presets_within_bounds() {
        for p in Preset::ALL {
            let mut t = p.tokens();
            for f in t.fields_mut() {
                if let super::super::FieldMut::Number(v, b) = f.value {
                    assert!((b.min..=b.max).contains(v), "{} : {}.{} = {v}", p.key(), f.section, f.key);
                }
            }
        }
    }
}
