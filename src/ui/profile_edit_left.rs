//! Éditeur de profil : colonne de gauche (choix du profil, tuiles qui
//! montrent l'état actuel) et sélecteurs ouverts par ces tuiles.
//!
//! Un sélecteur ne fait que changer le brouillon : rien ne part avant
//! « Enregistrer ». Ce que le compte n'a pas le droit d'utiliser est montré
//! verrouillé avec la raison ; rien ne s'achète ici, « Voir dans Discord »
//! ouvre le navigateur.

use super::cosmetics;
use super::menu;
use super::profile::Shown;
use super::profile_edit::{Asked, Editor, ImageEdit, Picker, DEFAULT_THEME, LEFT_OPEN};
use super::settings::account::{rgb_bytes, rgb_code};
use super::settings::avatar::crop_view;
use super::App;
use crate::avatar::Avatar as Worker;
use crate::i18n::table::nd::t as nt;
use crate::i18n::table::quick::t as qt;
use crate::i18n::table::s2::t as s2;
use crate::i18n::Lang;
use crate::images::{self, Image};
use crate::state::cosmetics::NameEffect;
use crate::state::crop::{ImageKind, MAX_ZOOM};
use crate::state::effects::Wearable;
use crate::state::model::{ImageRef, User};
use crate::state::profiles::Profile;
use crate::state::shop::SHOP_URL;
use crate::state::tags::{Slot, StyleEdit};
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Align2, Color32, CornerRadius, Id, Rect, RichText, Sense, Stroke, StrokeKind};

/// Page Nitro de Discord, pour ce que Discord réserve à ses abonnés.
const NITRO_URL: &str = "https://discord.com/nitro";
/// Marge de la colonne et écart entre deux tuiles.
const PAD: f32 = 20.0;
const GAP: f32 = 12.0;
/// Hauteur d'une tuile large (plaque, style du nom).
const WIDE: f32 = 52.0;

/// Effets de couleur du nom proposés (docs.discord.food, « Display Name
/// Effect ») : ceux dont la source décrit la valeur à l'envoi
/// (`NameEffect::writable`).
const EFFECTS: [(NameEffect, crate::i18n::Text); 6] = [
    (NameEffect::Solid, qt::ED_FX_SOLID),
    (NameEffect::Gradient, qt::ED_FX_GRADIENT),
    (NameEffect::Neon, qt::ED_FX_NEON),
    (NameEffect::Toon, qt::ED_FX_TOON),
    (NameEffect::Pop, qt::ED_FX_POP),
    (NameEffect::Glow, qt::ED_FX_GLOW),
];
/// Effets lus et rendus, mais jamais proposés : la source ne décrit pas leur
/// valeur à l'envoi. Montrés (inactifs) quand le style du compte en porte un.
const READ_ONLY_EFFECTS: [(NameEffect, crate::i18n::Text); 2] = [(NameEffect::Prism, qt::ED_FX_PRISM), (NameEffect::Gummy, qt::ED_FX_GUMMY)];
/// Seconde couleur proposée pour un dégradé.
const SECOND_COLOR: u32 = 0xeb459e;


/// Tuile cliquable : fond, contour (plus marqué au survol), nom pour les
/// lecteurs d'écran et en infobulle. L'appelant peint son contenu dedans.
fn tile(ui: &mut egui::Ui, t: &Tokens, size: egui::Vec2, label: &str) -> (Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let hot = resp.hovered() || resp.has_focus();
    ui.painter().rect(rect, t.r_embed(), t.colors.bg2, Stroke::new(t.shapes.border.max(1.0), t.hairline(hot)), StrokeKind::Inside);
    (rect, super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, label))
}

/// Cadenas dans le coin d'une tuile que Discord réserve à Nitro.
fn lock_mark(ui: &egui::Ui, t: &Tokens, tile: Rect) {
    let side = 18.0;
    let at = Rect::from_min_size(egui::pos2(tile.right() - side - 6.0, tile.top() + 6.0), egui::Vec2::splat(side));
    ui.painter().circle_filled(at.center(), side / 2.0 + 2.0, t.colors.bg0);
    icons::paint(Icon::Lock, ui.painter(), at.shrink(3.0), t.colors.muted);
}

/// « + » au milieu d'une tuile vide.
fn plus_mark(ui: &egui::Ui, t: &Tokens, rect: Rect) {
    let c = rect.center();
    ui.painter().circle_filled(c, 13.0, t.colors.bg3);
    icons::paint(Icon::Plus, ui.painter(), Rect::from_center_size(c, egui::Vec2::splat(14.0)), t.colors.text_strong);
}

/// Petit bouton à icône (replier, déplier).
fn icon_button(ui: &mut egui::Ui, t: &Tokens, icon: Icon, tip: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(t.spacing.control_height.max(28.0)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    icons::paint(icon, ui.painter(), Rect::from_center_size(rect.center(), egui::Vec2::splat(16.0)), if hot { t.colors.text_strong } else { t.colors.muted });
    super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Titre d'une section de la colonne ; `nitro` : la marque de ce que Discord réserve à Nitro.
fn section_title(ui: &mut egui::Ui, t: &Tokens, lang: Lang, text: &str, nitro: bool) {
    ui.add_space(GAP);
    ui.horizontal(|ui| {
        ui.label(RichText::new(text).family(super::fonts::demi()).size(t.typography.button).color(t.colors.text));
        if nitro {
            super::profile::chip(ui, t, qt::ED_NITRO_MARK.get(lang), t.colors.bg3, t.colors.muted);
        }
    });
    ui.add_space(2.0);
}

/// Raison d'un verrou, et le renvoi vers Discord (rien ne s'achète ici).
fn locked_note(ui: &mut egui::Ui, t: &Tokens, lang: Lang, asked: &mut Asked) {
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(Tokens::icon_side(t.typography.body)), Sense::hover());
        icons::paint(Icon::Lock, ui.painter(), rect, t.colors.muted);
        ui.add(egui::Label::new(RichText::new(qt::ED_LOCKED.get(lang)).color(t.colors.muted)).wrap());
    });
    if ui.add(super::form::btn(t, nt::SHOP_OFFICIAL.get(lang))).clicked() {
        asked.link = Some(NITRO_URL);
    }
    ui.add_space(t.spacing.item_y);
}

/// Rappel sous une liste d'objets possédés, et le renvoi vers la boutique de Discord.
fn owned_note(ui: &mut egui::Ui, t: &Tokens, lang: Lang, asked: &mut Asked) {
    ui.add_space(t.spacing.item_y);
    ui.add(egui::Label::new(RichText::new(qt::ED_OWNED_TIP.get(lang)).small().color(t.colors.muted)).wrap());
    if ui.add(super::form::btn(t, nt::SHOP_OFFICIAL.get(lang))).on_hover_text(nt::SHOP_OFFICIAL_TIP.get(lang)).clicked() {
        asked.link = Some(SHOP_URL);
    }
}

/// Contour d'un choix : accent s'il est retenu.
fn choice_stroke(t: &Tokens, on: bool, hot: bool) -> Stroke {
    if on { Stroke::new(2.0, t.colors.accent) } else { Stroke::new(t.shapes.border.max(1.0), t.hairline(hot)) }
}

impl App {
    /// Colonne de gauche ; repliée : seul le bouton qui la déplie.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn editor_left(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, me: &User, shown: &Shown, open: bool, asked: &mut Asked) {
        let lang = self.lang;
        let c = &t.colors;
        if !open {
            ui.add_space(GAP);
            ui.vertical_centered(|ui| {
                if icon_button(ui, t, Icon::ChevronRight, qt::ED_EXPAND.get(lang)).clicked() {
                    e.left_open = Some(true);
                }
            });
            return;
        }
        let width = ui.available_width().min(LEFT_OPEN);
        // En-tête : le profil modifié (liste déroulante) et le repli.
        egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD as i8, GAP as i8)).show(ui, |ui| {
            ui.set_width(width - 2.0 * PAD);
            ui.horizontal(|ui| {
                let name = match e.tab_guild() {
                    Some(g) => self.store.guilds.iter().find(|x| x.id == g).map(|x| x.name.clone()).unwrap_or_default(),
                    None => qt::TAB_MAIN.get(lang).to_string(),
                };
                let room = ui.available_width() - t.spacing.control_height.max(28.0) - t.spacing.item_x;
                let pick = ui
                    .allocate_ui_with_layout(egui::vec2(room, t.spacing.control_height.max(28.0)), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let font = egui::FontId::new((t.typography.title * 1.1).round(), super::fonts::demi());
                        let mut job = egui::text::LayoutJob::simple_singleline(name, font, c.text_strong);
                        job.wrap = egui::text::TextWrapping::truncate_at_width((room - 24.0).max(40.0));
                        let r = ui.add(egui::Label::new(job).selectable(false).sense(Sense::click()));
                        let (rect, r2) = ui.allocate_exact_size(egui::Vec2::splat(16.0), Sense::click());
                        icons::paint(Icon::ChevronDown, ui.painter(), rect, c.text_strong);
                        r.union(r2)
                    })
                    .inner;
                pick.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, qt::ED_PICK_PROFILE.get(lang)));
                let pick = pick.on_hover_cursor(egui::CursorIcon::PointingHand);
                egui::Popup::menu(&pick).id(Id::unique("ferrocord.profil.edition.profils")).show(|ui| {
                    ui.set_min_width(LEFT_OPEN - 2.0 * PAD);
                    if menu::radio(ui, t, qt::TAB_MAIN.get(lang), !e.on_guild).clicked() {
                        asked.guild = Some(None);
                        ui.close();
                    }
                    if !self.store.guilds.is_empty() {
                        menu::separator(ui, t);
                    }
                    egui::ScrollArea::vertical().max_height(8.0 * menu::row_height(t)).show(ui, |ui| {
                        for g in &self.store.guilds {
                            if menu::radio(ui, t, &g.name, e.tab_guild() == Some(g.id)).clicked() {
                                asked.guild = Some(Some(g.id));
                                ui.close();
                            }
                        }
                    });
                    menu::separator(ui, t);
                    if menu::item(ui, t, qt::MORE_OPTIONS.get(lang), Some(Icon::Gear), false).on_hover_text(qt::MORE_OPTIONS_TIP.get(lang)).clicked() {
                        asked.settings = true;
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, t, Icon::ChevronLeft, qt::ED_COLLAPSE.get(lang)).clicked() {
                        e.left_open = Some(false);
                    }
                });
            });
        });
        let y = ui.cursor().top();
        ui.painter().line_segment([egui::pos2(ui.max_rect().left(), y), egui::pos2(ui.max_rect().left() + width, y)], Stroke::new(1.0, c.separator));
        egui::ScrollArea::vertical().id_salt("profil-edition-gauche").auto_shrink([false, false]).show(ui, |ui| {
            egui::Frame::new().inner_margin(egui::Margin::symmetric(PAD as i8, 0)).show(ui, |ui| {
                ui.set_width(width - 2.0 * PAD);
                ui.add_enabled_ui(!e.busy(), |ui| self.editor_tiles(ui, t, e, me, shown));
                ui.add_space(PAD);
            });
        });
    }

    /// Les sections de tuiles. Le profil d'un serveur n'a que ce qui lui est
    /// propre : avatar, thème, bannière.
    fn editor_tiles(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, me: &User, shown: &Shown) {
        let lang = self.lang;
        let c = &t.colors;
        let full = ui.available_width();
        let half = ((full - GAP) / 2.0).floor();
        let square = egui::Vec2::splat(half);
        let nitro = e.nitro();
        let user = shown.user.as_ref();
        let look = user.map(|u| u.look).unwrap_or_default();
        let name = super::profile::name_of(&me.name, shown);
        let mut open = None;
        if !e.on_guild {
            section_title(ui, t, lang, qt::ED_PLATE.get(lang), false);
            let (rect, r) = tile(ui, t, egui::vec2(full, WIDE), qt::ED_PLATE.get(lang));
            if let Some(plate) = look.nameplate() {
                cosmetics::paint_nameplate(ui, &self.images, t, rect.shrink(1.0), plate, c.bg2);
            }
            // Une ligne de liste en miniature : avatar et nom figuré.
            let face = Rect::from_center_size(egui::pos2(rect.left() + 12.0 + 16.0, rect.center().y), egui::Vec2::splat(32.0));
            self.editor_face(ui, t, face, e, me, shown, &name);
            let bar = Rect::from_min_max(egui::pos2(face.right() + 12.0, rect.center().y - 6.0), egui::pos2(rect.right() - full * 0.3, rect.center().y + 6.0));
            ui.painter().rect_filled(bar, 6.0, c.text_strong.gamma_multiply(0.25));
            if r.clicked() {
                open = Some(Picker::Plate);
            }
        }
        section_title(ui, t, lang, if e.on_guild { qt::AVATAR.get(lang) } else { qt::ED_AVATAR_DECO.get(lang) }, false);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let (rect, r) = tile(ui, t, square, qt::AVATAR.get(lang));
            self.editor_face(ui, t, Rect::from_center_size(rect.center(), square * 0.62), e, me, shown, &name);
            if e.on_guild && !nitro {
                lock_mark(ui, t, rect);
            }
            if r.clicked() {
                open = Some(Picker::Avatar);
            }
            if !e.on_guild {
                let (rect, r) = tile(ui, t, square, qt::ED_DECORATION.get(lang));
                let face = Rect::from_center_size(rect.center(), square * 0.56);
                ui.painter().circle_filled(face.center(), face.width() / 2.0, c.bg3);
                match look.decoration() {
                    Some(asset) => cosmetics::paint_decoration(ui, &self.images, face, asset, false),
                    None => plus_mark(ui, t, rect),
                }
                if r.clicked() {
                    open = Some(Picker::Decoration);
                }
            }
        });
        if !e.on_guild {
            section_title(ui, t, lang, qt::ED_NAME_STYLE.get(lang), true);
            let (rect, r) = tile(ui, t, egui::vec2(full, WIDE), qt::ED_NAME_STYLE.get(lang));
            // Le nom tel que la carte le montre : couleurs, effet, police.
            let (galley, backdrop) = cosmetics::styled_line(ui, &name, (t.typography.title * 1.2).round(), look.name_style(), c.bg2, c.text_strong, full - 2.0 * GAP);
            cosmetics::paint_styled(ui.painter(), rect.center() - galley.size() / 2.0, galley, backdrop, c.text_strong);
            if !nitro {
                lock_mark(ui, t, rect);
            }
            if r.clicked() {
                open = Some(Picker::Style);
            }
        }
        section_title(ui, t, lang, qt::ED_THEME_BANNER.get(lang), true);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = GAP;
            let (rect, r) = tile(ui, t, square, qt::ED_THEME_COLORS.get(lang));
            let theme = shown.profile.as_ref().and_then(Profile::theme).map(|th| th.map(cosmetics::rgb));
            let inner = rect.shrink(1.0);
            if let Some([a, b]) = theme {
                ui.painter().add(cosmetics::gradient_rect(inner, t.r_embed(), |p| cosmetics::mix(a, b, (p.y - inner.top()) / inner.height().max(1.0))));
            }
            // Les deux pastilles : couleur principale en haut, secondaire en bas.
            for (k, color) in [(0.2, theme.map(|x| x[0])), (0.8, theme.map(|x| x[1]))] {
                let dot = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + rect.height() * k), egui::Vec2::splat(26.0));
                ui.painter().rect(dot, 8.0, color.unwrap_or(c.bg3), Stroke::new(2.0, Color32::WHITE), StrokeKind::Inside);
            }
            if !nitro {
                lock_mark(ui, t, rect);
            }
            if r.clicked() {
                open = Some(Picker::Theme);
            }
            let (rect, r) = tile(ui, t, square, qt::BANNER.get(lang));
            if !self.editor_banner(ui, t, rect.shrink(1.0), e, me, shown) {
                plus_mark(ui, t, rect);
            }
            if !nitro {
                lock_mark(ui, t, rect);
            }
            if r.clicked() {
                open = Some(Picker::Banner);
            }
        });
        if !e.on_guild {
            section_title(ui, t, lang, nt::EFFECT.get(lang), false);
            let (rect, r) = tile(ui, t, square, nt::EFFECT.get(lang));
            let cfg = shown.profile.as_ref().and_then(Profile::effect).and_then(|id| self.store.effects.config(id));
            match &cfg {
                Some(cfg) => {
                    // Proportions du canevas d'un effet (450 × 880), au milieu de la tuile.
                    let h = rect.height() - 2.0 * GAP;
                    let thumb = Rect::from_center_size(rect.center(), egui::vec2(h * 450.0 / 880.0, h));
                    ui.painter().rect_filled(thumb, t.r_row(), c.bg0);
                    super::effects::thumbnail(ui, &self.images, cfg, thumb, t.r_row());
                }
                None => plus_mark(ui, t, rect),
            }
            if r.clicked() {
                open = Some(Picker::Effect);
            }
        }
        if let Some(p) = open {
            e.picker = Some(p);
            e.game_query.clear();
        }
    }

    /// Avatar du profil affiché dans le carré `rect` : la nouvelle image du
    /// brouillon, sinon celle du profil, sinon les initiales.
    #[allow(clippy::too_many_arguments)]
    fn editor_face(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, e: &Editor, me: &User, shown: &Shown, name: &str) {
        let side = rect.width();
        if let Some(sw) = e.swap(ImageKind::Avatar, 1.0) {
            ui.painter().add(sw.shape(rect, t.r_avatar(side)));
            return;
        }
        let base = shown.user.as_ref().map(|u| u.avatar).unwrap_or(me.avatar);
        let member = e.tab_guild().zip(shown.member.as_ref()).and_then(|(guild_id, m)| Some(ImageRef::MemberAvatar { guild_id, user_id: me.id, hash: m.avatar? }));
        match member.or(base.image).map(|i| self.images.get(i, side)) {
            Some(Image::Ready(tex)) => {
                ui.painter().add(images::textured(rect, t.r_avatar(side), tex));
            }
            _ => super::util::paint_initials(ui, t, rect, t.default_avatar(base.default), name),
        }
    }

    /// Bannière du profil affiché dans `rect` (image du brouillon, image du
    /// profil, sinon couleur unie). Faux s'il n'y a rien à montrer.
    fn editor_banner(&self, ui: &egui::Ui, t: &Tokens, rect: Rect, e: &Editor, me: &User, shown: &Shown) -> bool {
        let r = t.r_embed();
        if let Some(sw) = e.swap(ImageKind::Banner, rect.width() / rect.height().max(1.0)) {
            ui.painter().add(sw.shape(rect, r));
            return true;
        }
        let profile = shown.profile.as_ref();
        let guild = e.tab_guild().zip(profile.and_then(|p| p.guild.as_ref()).and_then(|g| g.banner).or(shown.member.as_ref().and_then(|m| m.banner))).map(|(guild_id, hash)| ImageRef::MemberBanner { guild_id, user_id: me.id, hash });
        let own = shown.user.as_ref().and_then(|u| u.banner.flatten()).or(profile.and_then(|p| p.user.banner)).map(|hash| ImageRef::Banner { user_id: me.id, hash });
        let accent = shown.user.as_ref().and_then(|u| u.accent.flatten()).or(profile.and_then(|p| p.user.accent));
        if let Some(color) = accent {
            ui.painter().rect_filled(rect, r, cosmetics::rgb(color));
        }
        let image = guild.or(own);
        if let Some(b) = image
            && let Image::Ready(tex) = self.images.get_fill_moving(b, rect.width(), rect.height(), cosmetics::BANNER_ASPECT, false)
        {
            ui.painter().add(images::textured(rect, r, tex));
        }
        image.is_some() || accent.is_some()
    }

    /// Sélecteur ouvert par une tuile (ou par « + » d'un widget), par-dessus
    /// l'éditeur ; la croix, Échap ou un clic à côté le ferme.
    pub(super) fn editor_pickers(&self, ctx: &egui::Context, e: &mut Editor, me: &User, shown: &Shown, asked: &mut Asked) {
        let Some(picker) = e.picker else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let title = match picker {
            Picker::Plate => qt::ED_PLATE,
            Picker::Avatar => qt::AVATAR,
            Picker::Decoration => qt::ED_DECORATION,
            Picker::Style => qt::ED_NAME_STYLE,
            Picker::Theme => qt::ED_THEME_COLORS,
            Picker::Banner => qt::BANNER,
            Picker::Effect => nt::EFFECT,
            Picker::Game(_) => qt::ED_ADD_GAME,
        };
        let width = 420.0_f32.min(ctx.content_rect().width() - 4.0 * t.spacing.card_pad).max(240.0);
        let out = super::widgets::sheet(ctx, &t, Id::unique(("ferrocord.profil.edition.choix", picker)), width, title.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
            ui.add_enabled_ui(!e.busy(), |ui| match picker {
                Picker::Plate => self.pick_look(ui, &t, e, Slot::Nameplate, asked),
                Picker::Decoration => self.pick_look(ui, &t, e, Slot::Decoration, asked),
                Picker::Avatar => self.pick_image(ui, &t, e, ImageKind::Avatar, asked),
                Picker::Banner => self.pick_image(ui, &t, e, ImageKind::Banner, asked),
                Picker::Theme => self.pick_theme(ui, &t, e, asked),
                Picker::Style => self.pick_style(ui, &t, e, &super::profile::name_of(&me.name, shown), asked),
                Picker::Effect => {
                    let worn = e.effect.unwrap_or(e.main.as_ref().and_then(|m| m.effect));
                    if let Some(pick) = self.effect_picker(ui, &t, worn) {
                        e.effect = Some(pick);
                    }
                    owned_note(ui, &t, lang, asked);
                }
                Picker::Game(kind) => self.pick_game(ui, &t, e, kind),
            });
        });
        if out.closed && e.picker == Some(picker) {
            e.picker = None;
        }
    }

    /// Plaque ou décoration : « Aucune », puis celles que le compte possède.
    fn pick_look(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, slot: Slot, asked: &mut Asked) {
        let lang = self.lang;
        let c = &t.colors;
        let owned: Vec<_> = self.store.effects.looks.iter().filter(|o| Slot::of(&o.look) == slot).collect();
        let now = if slot == Slot::Decoration { e.decoration_now() } else { e.plate_now() };
        let mut pick = None;
        if owned.is_empty() && now.is_none() {
            let empty = if slot == Slot::Decoration { qt::ED_NO_DECORATION } else { qt::ED_NO_PLATE };
            ui.add(egui::Label::new(RichText::new(empty.get(lang)).color(c.muted)).wrap());
        } else {
            let none = nt::LOOK_NONE.get(lang);
            let title = if slot == Slot::Decoration { nt::SHOP_DECORATION } else { nt::SHOP_NAMEPLATE };
            let side = 84.0;
            let lay = |ui: &mut egui::Ui, add: &mut dyn FnMut(&mut egui::Ui)| {
                if slot == Slot::Decoration {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = egui::Vec2::splat(t.spacing.item_x);
                        add(ui)
                    });
                } else {
                    add(ui)
                }
            };
            lay(ui, &mut |ui| {
                for (sku, look, label) in std::iter::once((None, None, none)).chain(owned.iter().map(|o| (Some(o.sku_id), Some(o.look), if o.label.is_empty() { title.get(lang) } else { o.label.as_str() }))) {
                    let on = now == sku;
                    let size = if slot == Slot::Decoration { egui::Vec2::splat(side) } else { egui::vec2(ui.available_width(), WIDE) };
                    let (rect, r) = ui.allocate_exact_size(size, Sense::click());
                    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, on, label));
                    ui.painter().rect_filled(rect, t.r_embed(), c.bg1);
                    match look {
                        Some(Wearable::Decoration(asset)) => {
                            let face = Rect::from_center_size(rect.center(), size * 0.56);
                            ui.painter().circle_filled(face.center(), face.width() / 2.0, c.bg3);
                            cosmetics::paint_decoration(ui, &self.images, face, asset, false);
                        }
                        Some(Wearable::Nameplate(plate)) => {
                            cosmetics::paint_nameplate(ui, &self.images, t, rect, plate, c.bg1);
                            ui.painter().text(egui::pos2(rect.left() + GAP, rect.center().y), Align2::LEFT_CENTER, label, egui::FontId::new(t.typography.button, super::fonts::demi()), c.text_strong);
                        }
                        None => {
                            ui.painter().text(rect.center(), Align2::CENTER_CENTER, label, egui::FontId::proportional(t.typography.button), c.muted);
                        }
                    }
                    ui.painter().rect_stroke(rect, t.r_embed(), choice_stroke(t, on, r.hovered()), StrokeKind::Inside);
                    if r.on_hover_text(label).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && !on {
                        pick = Some(sku);
                    }
                }
            });
        }
        if let Some(sku) = pick {
            match slot {
                Slot::Decoration => e.decoration = Some(sku),
                Slot::Nameplate => e.plate = Some(sku),
            }
            e.warned = false;
        }
        owned_note(ui, t, lang, asked);
    }

    /// Avatar ou bannière du profil affiché : changer, retirer, recadrer ;
    /// pour la bannière du compte, la couleur unie qui la remplace.
    fn pick_image(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, kind: ImageKind, asked: &mut Asked) {
        let lang = self.lang;
        let c = &t.colors;
        let target = e.target(kind);
        let nitro = e.nitro();
        // Avatar d'un serveur et bannières : Nitro (Discord refuse sinon).
        let allowed = match kind {
            ImageKind::Avatar => nitro || !e.on_guild,
            _ => nitro,
        };
        let title = if kind == ImageKind::Avatar { qt::AVATAR } else { qt::BANNER };
        let picking = e.worker.as_ref().is_some_and(Worker::picking);
        let state = e.images.get(&target).map(|i| matches!(i, ImageEdit::New(_)));
        let (mut remove, mut keep) = (false, false);
        if !allowed {
            locked_note(ui, t, lang, asked);
        }
        ui.horizontal_wrapped(|ui| {
            let named = |r: &egui::Response, what: crate::i18n::Text| super::a11y::name(&r.ctx, r.id, &format!("{} : {}", title.get(lang), what.get(lang)));
            if allowed {
                let r = ui.add_enabled(Worker::CAN_PICK && !picking, egui::Button::new(qt::CHANGE.get(lang)));
                if r.clicked() {
                    asked.choose = Some(kind);
                }
                named(&r, qt::CHANGE);
            }
            // Sans Nitro, une bannière reste retirable (abonnement fini) : Discord
            // accepte `banner: null` de tout le monde ; le reste serait refusé.
            if allowed || kind == ImageKind::Banner {
                let r = ui.add_enabled(state != Some(false), egui::Button::new(s2::REMOVE_IMAGE.get(lang)));
                let r = if allowed { r } else { r.on_hover_text(crate::i18n::table::restes::t::BANNER_REMOVE_ONLY.get(lang)) };
                remove = r.clicked();
                named(&r, s2::REMOVE_IMAGE);
                if state.is_some() {
                    keep = ui.button(qt::KEEP.get(lang)).clicked();
                }
            }
        });
        if let Some(new) = state {
            let text = if new { qt::IMAGE_NEW } else { qt::IMAGE_REMOVED };
            ui.label(RichText::new(text.get(lang)).small().color(c.muted));
        }
        if remove {
            e.images.insert(target, ImageEdit::Remove);
            e.warned = false;
        }
        if keep {
            e.images.remove(&target);
        }
        if let Some(ImageEdit::New(p)) = e.images.get_mut(&target) {
            ui.label(RichText::new(s2::CROP_TIP.get(lang)).small().color(c.muted));
            crop_view(ui, t, p, kind);
            ui.horizontal(|ui| {
                ui.label(RichText::new(s2::ZOOM.get(lang)).color(c.muted));
                let r = ui.add(egui::Slider::new(&mut p.crop.zoom, 1.0..=MAX_ZOOM).fixed_decimals(1));
                super::a11y::name(ui.ctx(), r.id, s2::ZOOM.get(lang));
            });
        }
        if let Some(err) = &e.image_error {
            ui.add(egui::Label::new(RichText::new(err).color(c.danger)).wrap());
        }
        // Couleur de bannière : au compte seulement, et sans condition d'abonnement.
        if kind == ImageKind::Banner
            && !e.on_guild
            && let Some(m) = &mut e.main
        {
            ui.add_space(t.spacing.item_y);
            super::widgets::field_label(ui, t, qt::BANNER_COLOR.get(lang));
            ui.horizontal(|ui| {
                let d = &mut m.draft;
                let mut on = d.accent.is_some();
                let r = ui.checkbox(&mut on, qt::ED_BANNER_COLOR_ON.get(lang));
                super::a11y::name(ui.ctx(), r.id, qt::BANNER_COLOR.get(lang));
                if r.changed() {
                    d.accent = on.then_some(d.accent.unwrap_or(rgb_bytes(DEFAULT_THEME[0])));
                }
                if let Some(color) = &mut d.accent {
                    ui.color_edit_button_srgb(color);
                }
            });
        }
    }

    /// Couleurs du thème du profil affiché : case, puis les deux couleurs.
    fn pick_theme(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, asked: &mut Asked) {
        let lang = self.lang;
        if !e.nitro() {
            // Discord le refuserait : dit, plutôt que proposé puis refusé.
            locked_note(ui, t, lang, asked);
            return;
        }
        let theme = match (e.on_guild, &mut e.guild, &mut e.main) {
            (true, Some((_, d)), _) => &mut d.theme,
            (false, _, Some(m)) => &mut m.draft.theme,
            _ => return,
        };
        let mut on = theme.is_some();
        let r = ui.checkbox(&mut on, qt::ED_THEME_ON.get(lang));
        super::a11y::name(ui.ctx(), r.id, qt::THEME.get(lang));
        if r.changed() {
            *theme = on.then_some(theme.unwrap_or(DEFAULT_THEME.map(rgb_bytes)));
        }
        if let Some([a, b]) = theme {
            for (color, label) in [(a, qt::THEME_PRIMARY), (b, qt::THEME_ACCENT)] {
                ui.horizontal(|ui| {
                    let r = ui.color_edit_button_srgb(color);
                    super::a11y::name(ui.ctx(), r.id, label.get(lang));
                    ui.label(RichText::new(label.get(lang)).color(t.colors.text));
                });
            }
        }
    }

    /// Style du nom : effet de couleur, police, une ou deux couleurs. Les
    /// polices viennent de la table `fonts::NAMES` (celle du rendu) : chaque
    /// choix est écrit dans sa propre police, chargée à l'ouverture du
    /// sélecteur. Un style dont l'effet n'a pas de valeur d'envoi documentée
    /// (prisme, « gummy ») reste affiché, mais n'est modifiable qu'après le
    /// choix d'un effet proposé.
    fn pick_style(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, name: &str, asked: &mut Asked) {
        let lang = self.lang;
        let c = &t.colors;
        let nitro = e.nitro();
        if !nitro {
            locked_note(ui, t, lang, asked);
        }
        let before = e.style_now();
        let mut style = before.clone();
        // Aperçu du nom, sur le fond de la carte.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), WIDE), Sense::hover());
        ui.painter().rect_filled(rect, t.r_embed(), c.bg1);
        let shown = style.as_ref().and_then(StyleEdit::shown);
        let (galley, backdrop) = cosmetics::styled_line(ui, name, (t.typography.title * 1.2).round(), shown, c.bg1, c.text_strong, rect.width() - 2.0 * GAP);
        cosmetics::paint_styled(&ui.painter().with_clip_rect(rect), rect.center() - galley.size() / 2.0, galley, backdrop, c.text_strong);
        ui.add_enabled_ui(nitro, |ui| {
            super::widgets::field_label(ui, t, qt::ED_STYLE_EFFECT.get(lang));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::splat(t.spacing.item_y);
                if ui.add(egui::Button::selectable(style.is_none(), qt::ED_STYLE_NONE.get(lang))).clicked() {
                    style = None;
                }
                for (effect, label) in EFFECTS {
                    let code = effect.code();
                    let on = style.as_ref().is_some_and(|s| s.effect_id == code);
                    if ui.add(egui::Button::selectable(on, label.get(lang))).clicked() && !on {
                        let s = style.get_or_insert_with(|| StyleEdit { font_id: super::fonts::DEFAULT_NAME_FONT, effect_id: code, colors: vec![DEFAULT_THEME[0]] });
                        s.effect_id = code;
                        // Une couleur, ou deux pour un dégradé.
                        s.colors.truncate(if effect.two_colors() { 2 } else { 1 });
                        if effect.two_colors() && s.colors.len() < 2 {
                            s.colors.push(SECOND_COLOR);
                        }
                    }
                }
                // Effet porté mais non enregistrable : montré, inactif.
                if let Some((_, label)) = READ_ONLY_EFFECTS.iter().find(|(e, _)| style.as_ref().is_some_and(|s| s.effect_id == e.code())) {
                    ui.add_enabled(false, egui::Button::selectable(true, label.get(lang)));
                }
            });
            let Some(s) = &mut style else { return };
            if !NameEffect::from_code(u64::from(s.effect_id)).writable() {
                // Rien de ce style ne partirait avec cet effet : choisir d'abord un effet proposé.
                ui.add(egui::Label::new(RichText::new(qt::ED_STYLE_READ_ONLY.get(lang)).small().color(c.muted)).wrap());
                ui.disable();
            }
            super::widgets::field_label(ui, t, qt::ED_STYLE_COLORS.get(lang));
            ui.horizontal(|ui| {
                for (i, color) in s.colors.iter_mut().enumerate() {
                    let mut bytes = rgb_bytes(*color);
                    let r = ui.color_edit_button_srgb(&mut bytes);
                    super::a11y::name(ui.ctx(), r.id, if i == 0 { qt::THEME_PRIMARY.get(lang) } else { qt::THEME_ACCENT.get(lang) });
                    *color = rgb_code(bytes);
                }
            });
            super::widgets::field_label(ui, t, qt::ED_STYLE_FONT.get(lang));
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::Vec2::splat(t.spacing.item_y);
                // Une police retirée par Discord n'est proposée que si le style la porte déjà.
                let worn = s.font_id;
                for f in super::fonts::NAMES.iter().filter(|f| f.offered || f.id == worn) {
                    let label = RichText::new(f.label).family(super::fonts::styled(ui.ctx(), f.id));
                    // Au survol : la police de Discord derrière le nom du style.
                    if ui.add(egui::Button::selectable(s.font_id == f.id, label)).on_hover_text(f.font).clicked() {
                        s.font_id = f.id;
                    }
                }
            });
            ui.add(egui::Label::new(RichText::new(qt::ED_STYLE_FONT_NOTE.get(lang)).small().color(c.muted)).wrap());
        });
        if style != before {
            e.style = Some(style);
            e.warned = false;
        }
    }
}

/// Coins d'une vignette posée dans une carte de widget.
pub(super) fn r_cover(t: &Tokens) -> CornerRadius {
    t.r_row()
}
