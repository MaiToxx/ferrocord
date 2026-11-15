//! Préréglages d'avant la refonte visuelle (lot E), valeur pour valeur. Ils
//! ne sont plus proposés : ils servent à mettre à jour un `theme.toml` écrit
//! avant la refonte (`file::migrate`), où chaque valeur restée égale à
//! l'ancien préréglage prend la nouvelle.

use super::{AvatarShape, Avatars, Clock, Colors, Density, Layout, Markdown, MediaLoad, MembersPanel, MessageMode, Preset, RailStyle, Shapes, Spacing, StatusDots, Tokens, TypingSignal, Typography};
use eframe::egui::Color32;

const fn rgb(hex: u32) -> Color32 {
    Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8)
}

/// Ancien préréglage `p`. Les jetons nés après lui ont leur valeur d'aujourd'hui.
pub(super) fn tokens(p: Preset) -> Tokens {
    let mut t = rouille(p.tokens());
    t.base = p;
    match p {
        Preset::Rouille => {}
        // Nés après la refonte : aucun ancien état.
        Preset::Sombre | Preset::Cendre | Preset::Onyx => t = p.tokens(),
        Preset::Minuit => {
            let c = &mut t.colors;
            c.bg0 = rgb(0x000000);
            c.bg1 = rgb(0x0b0b0d);
            c.bg2 = rgb(0x000000);
            c.bg3 = rgb(0x1c1d22);
            c.border = rgb(0x2a2b31);
            c.pill = rgb(0x1c1d22);
            c.selected = rgb(0x1f2027);
            c.row_hover = rgb(0x141518);
            c.text = rgb(0xe4e5ea);
            c.text_strong = rgb(0xffffff);
            c.muted = rgb(0xa3a5b0);
            c.faint = rgb(0x858791);
            c.accent = rgb(0x8b9cff);
            c.on_accent = rgb(0x000000);
            c.danger = rgb(0xff5d63);
            c.link = rgb(0x9fb0ff);
            c.hover = Color32::from_white_alpha(6);
            c.widget_text = rgb(0x9a9ca6);
            c.button_bg = rgb(0x1c1d22);
            c.button_text = rgb(0xdcdde2);
            c.control_bg = rgb(0x2a2b31);
            c.separator = rgb(0x2a2b31);
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
                &mut t,
                [
                    rgb(0x0e0f12),
                    Color32::from_rgba_unmultiplied(0x8b, 0x9c, 0xff, 0x30),
                    rgb(0xc9d1ff),
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
            c.bg3 = rgb(0xe6e8eb);
            c.border = rgb(0xd4d7dc);
            c.pill = rgb(0xffffff);
            c.selected = rgb(0xdcdfe4);
            c.row_hover = rgb(0xe7e9ec);
            c.text = rgb(0x1f2328);
            c.text_strong = rgb(0x050608);
            c.muted = rgb(0x4e5560);
            c.faint = rgb(0x5d636d);
            c.accent = rgb(0xb84f1a);
            c.on_accent = rgb(0xffffff);
            c.danger = rgb(0xbd2626);
            c.link = rgb(0xa8470f);
            c.hover = Color32::from_black_alpha(6);
            c.widget_text = rgb(0x3c4048);
            c.button_bg = rgb(0xd3d6db);
            c.button_text = rgb(0x1f2328);
            c.control_bg = rgb(0xc4c8ce);
            c.separator = rgb(0xc9ccd1);
            c.focus = rgb(0x0b5cd5);
            c.online = rgb(0x23864b);
            c.idle = rgb(0xb07d0a);
            c.dnd = rgb(0xc62828);
            c.offline = rgb(0x747a84);
            c.authors = [
                rgb(0xb84f1a),
                rgb(0x1b7a4b),
                rgb(0x2f5fc4),
                rgb(0x9a3aa0),
                rgb(0x8a5a00),
                rgb(0x0f7279),
                rgb(0xbf3346),
                rgb(0x6247c9),
            ];
            markdown_colors(
                &mut t,
                [
                    rgb(0xeef0f2),
                    Color32::from_rgba_unmultiplied(0xb8, 0x4f, 0x1a, 0x24),
                    rgb(0x7a320c),
                    rgb(0xc4c8ce),
                    rgb(0xc4c8ce),
                    rgb(0xa8470f),
                    rgb(0x1b6e43),
                    rgb(0x5a3fc0),
                    rgb(0x5e6570),
                ],
            );
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
            t.shapes.unread_ring = 3.0;
            t.shapes.mention_dot = 5.0;
            markdown_colors(
                &mut t,
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
            t.layout.channels_width = 200.0;
            t.layout.avatar_size = 28.0;
            t.layout.small_avatar = 18.0;
            t.typography.rail = 12.0;
            t.typography.title = 14.0;
            t.typography.icon = 16.0;
            t.shapes.rail = 18.0;
            t.shapes.rail_selected = 10.0;
            t.shapes.composer = 6.0;
            t.shapes.embed = 4.0;
            t.spacing.message_x = 12.0;
            t.spacing.chat_head_x = 12.0;
            t.spacing.composer_x = 12.0;
        }
        Preset::Aere => {
            t.layout.density = Density::Airy;
            t.layout.rail_button = 52.0;
            t.layout.rail_width = 74.0;
            t.layout.channels_width = 260.0;
            t.layout.avatar_size = 42.0;
            t.layout.small_avatar = 26.0;
            t.spacing.avatar_gap = 14.0;
            t.typography.body = 15.0;
            t.typography.button = 14.0;
            t.typography.small = 11.0;
            t.typography.title = 17.0;
            t.typography.icon = 20.0;
            t.typography.rail = 15.0;
            t.shapes.composer = 14.0;
            t.shapes.embed = 8.0;
            t.shapes.hover = 6.0;
            t.spacing.message_x = 24.0;
            t.spacing.chat_head_x = 24.0;
            t.spacing.composer_x = 24.0;
            t.spacing.item_x = 10.0;
        }
    }
    t
}

fn rouille(new: Tokens) -> Tokens {
    Tokens {
        base: Preset::Rouille,
        colors: Colors {
            bg0: rgb(0x121316),
            bg1: rgb(0x18191d),
            bg2: rgb(0x1f2025),
            bg3: rgb(0x2a2c33),
            border: rgb(0x2a2c33),
            pill: rgb(0x1f2025),
            selected: rgb(0x2c2e35),
            row_hover: rgb(0x212227),
            text: rgb(0xdcdde2),
            text_strong: Color32::WHITE,
            muted: rgb(0x9a9ca6),
            faint: rgb(0x94959c),
            accent: rgb(0xe2733a),
            on_accent: rgb(0x121316),
            danger: rgb(0xeb6f73),
            link: rgb(0xf39a5f),
            hover: Color32::from_white_alpha(4),
            // Les cinq suivantes sont les valeurs d'`egui::Visuals::dark()`.
            widget_text: Color32::from_gray(0x95),
            button_bg: Color32::from_gray(50),
            button_text: Color32::from_gray(215),
            control_bg: Color32::from_gray(60),
            separator: Color32::from_gray(60),
            scrim: Color32::from_black_alpha(215),
            shadow: new.colors.shadow,
            // Contour de l'élément qui a le clavier (lot navigation).
            focus: rgb(0x4ea1ff),
            // Couleurs de statut usuelles (vert, ambre, rouge, gris).
            online: rgb(0x23a55a),
            idle: rgb(0xf0b232),
            dnd: rgb(0xf23f43),
            offline: rgb(0x80848e),
            authors: [
                rgb(0xe2733a),
                rgb(0x4cc38a),
                rgb(0x5b8def),
                rgb(0xd26ad6),
                rgb(0xe5b84a),
                rgb(0x4fc3c9),
                rgb(0xef6b78),
                rgb(0x9f8cf5),
            ],
            panel_border: new.colors.panel_border,
            menu_bg: new.colors.menu_bg,
            menu_hover: new.colors.menu_hover,
            channel_idle: new.colors.channel_idle,
        },
        // Tailles d'egui par défaut, pour un rendu identique à celui d'avant les jetons.
        typography: Typography { body: 13.0, small: 9.0, button: 13.0, title: 15.0, icon: 18.0, rail: 14.0, logo: 26.0, line_height: new.typography.line_height, family: new.typography.family },
        shapes: Shapes {
            roundness: 1.0,
            card: 14.0,
            rail: 24.0,
            rail_selected: 14.0,
            row: 4.0,
            control: new.shapes.control,
            composer: 10.0,
            embed: 6.0,
            embed_bar: 4.0,
            toast: 8.0,
            hover: 0.0,
            avatar: 8.0,
            border: 1.0,
            unread_ring: 2.0,
            mention_dot: 4.0,
            status_dot: 4.5,
            focus: 2.0,
        },
        spacing: Spacing {
            item_x: 8.0,
            item_y: 3.0,
            control_x: new.spacing.control_x,
            control_y: new.spacing.control_y,
            rail_x: 6.0,
            rail_y: 10.0,
            rail_gap: 4.0,
            channels_pad: 6.0,
            category_gap: 10.0,
            channel_row: 26.0,
            channel_indent: 8.0,
            mention_inset: 10.0,
            user_bar_x: 10.0,
            user_bar_y: 8.0,
            guild_title_x: 12.0,
            guild_title_y: 14.0,
            chat_head_x: 16.0,
            chat_head_y: 14.0,
            composer_x: 16.0,
            composer_top: 4.0,
            composer_bottom: 14.0,
            composer_inner_x: 12.0,
            composer_inner_y: 10.0,
            message_x: 16.0,
            message_gap: 10.0,
            message_gap_grouped: 1.0,
            avatar_gap: 12.0,
            chat_pad: 8.0,
            embed_pad: 8.0,
            media_gap: 4.0,
            compact_indent: 48.0,
            card_pad: 28.0,
            login_title_gap: 16.0,
            login_field_gap: 6.0,
            login_button_gap: 10.0,
            login_error_gap: 8.0,
            login_notice_gap: 14.0,
            toast_pad: 10.0,
            toast_offset: 70.0,
            member_row: 42.0,
            member_header: 28.0,
            header_height: new.spacing.header_height,
            message_right: new.spacing.message_right,
            control_height: new.spacing.control_height,
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
            avatar_size: 36.0,
            small_avatar: 22.0,
            rail_width: 68.0,
            rail_list_width: 200.0,
            rail_button: 48.0,
            rail_list_row: 36.0,
            channels_width: 230.0,
            members: MembersPanel::Shown,
            members_width: 240.0,
            card_width: 380.0,
            button_height: 36.0,
            composer_rows: 10.0,
            typing: TypingSignal::Sent,
            status_dots: StatusDots::Shown,
            profile_width: 300.0,
            profile_avatar: 80.0,
            titlebar: new.layout.titlebar,
            titlebar_height: new.layout.titlebar_height,
        },
        markdown: Markdown {
            code_bg: rgb(0x16171a),
            mention_bg: Color32::from_rgba_unmultiplied(0xe2, 0x73, 0x3a, 0x38),
            mention_text: rgb(0xffd2b8),
            mention_row: new.markdown.mention_row,
            spoiler: rgb(0x0e0f11),
            quote_bar: rgb(0x4e5058),
            keyword: rgb(0xf39a5f),
            string: rgb(0x8fd17a),
            number: rgb(0xc9a6ff),
            comment: rgb(0x80838e),
            h1: 22.0,
            h2: 18.0,
            h3: 15.0,
            quote_width: 3.0,
            quote_gap: 10.0,
            list_indent: 18.0,
            code_pad: 8.0,
            code_radius: 4.0,
            emoji: 20.0,
            jumbo: 44.0,
            picker_emoji: 32.0,
            sticker: 160.0,
        },
    }
}

/// Couleurs du texte mis en forme propres à un préréglage (le reste vient de Rouille).
fn markdown_colors(t: &mut Tokens, colors: [Color32; 9]) {
    let m = &mut t.markdown;
    [m.code_bg, m.mention_bg, m.mention_text, m.spoiler, m.quote_bar, m.keyword, m.string, m.number, m.comment] = colors;
}
