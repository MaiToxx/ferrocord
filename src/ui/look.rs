//! Personnalisation du client (lot PERSO) : éditeur de thème, dégradé et
//! image de fond, polices installées, thème du jour et de la nuit, thèmes et
//! profils enregistrés. La page est en tête de Paramètres > Apparence.
//!
//! Le fond (image ou dégradé) est peint dans chaque panneau, par-dessus son
//! aplat et sous son contenu : une texture, étirée une fois sur la fenêtre,
//! et quelques sommets. Rien ne bouge : aucun redessin au repos.
//!
//! Les fichiers (image, polices, bibliothèque) sont lus par `crate::look`,
//! dans un fil à part.

use super::appearance::{color_button, row};
use super::settings::{note, section, toggle};
use super::widgets::{self, Kind as Button};
use super::{fonts, App};
use crate::i18n::table::perso::{f as pf, t as pt};
use crate::i18n::{self, t as bt, Lang, Text};
use crate::look::{self, Done, Family, Kind, Look};
use crate::state::look_prefs::{self as prefs, Auto, Gradient, LookPrefs, NameColor, Phase, Scope};
use crate::theme::{contrast_ratio, fmt_color, parse_color, FieldMut, Preset, Tokens, MIN_MUTED, MIN_TEXT};
use eframe::egui::{self, pos2, vec2, Color32, Rect, RichText, Sense, Stroke};
use std::sync::Arc;

/// Zone d'un panneau, pour savoir si l'image de fond s'y voit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Zone {
    Chat,
    Side,
}

/// Ce que les panneaux peignent derrière leur contenu.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Paint {
    /// Texture, taille en pixels, teinte (assombrissement et opacité), partout ou discussion seule.
    image: Option<(egui::TextureId, egui::Vec2, Color32, bool)>,
    /// Couleurs (opacité comprise), leur nombre, angle en radians.
    gradient: Option<([Color32; 3], usize, f32)>,
}

fn paint_id() -> egui::Id {
    egui::Id::unique("ferrocord-fond")
}

/// Lecture des polices installées.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Listing {
    #[default]
    NotAsked,
    Asked,
    Ready,
}

#[derive(Default)]
pub(super) struct LookFx {
    service: Option<Look>,
    /// Image demandée au fil (chemin, flou) ; la texture est la sienne quand `loading` est faux.
    backdrop_asked: Option<(String, u8)>,
    loading: bool,
    texture: Option<egui::TextureHandle>,
    /// Polices demandées (interface, code) et fichiers reçus.
    fonts_asked: (String, String),
    custom: fonts::Custom,
    families: Vec<Family>,
    listing: Listing,
    themes: Vec<String>,
    pub(super) profiles: Vec<String>,
    library_asked: bool,
    /// Dernière bascule jour/nuit appliquée.
    auto_applied: Option<(Phase, String)>,
    paint: Paint,
    /// Réglage changé pendant un glissement : enregistré au relâchement.
    dirty: bool,
    theme_name: String,
    profile_name: String,
    pub(super) message: Option<String>,
    /// Thèmes partageables : export, import, galerie (lot E5).
    pub(super) share: super::theme_share::ShareUi,
}

fn gradient_color(c: [u8; 3], strength: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], (f32::from(strength.min(prefs::STRENGTH_MAX)) * 2.55).round() as u8)
}

fn gradient_paint(g: &Gradient) -> Option<([Color32; 3], usize, f32)> {
    if !g.on || g.strength == 0 || g.colors.len() < 2 {
        return None;
    }
    let mut colors = [Color32::TRANSPARENT; 3];
    let n = g.colors.len().min(3);
    for (slot, c) in colors.iter_mut().zip(&g.colors) {
        *slot = gradient_color(*c, g.strength);
    }
    Some((colors, n, f32::from(g.angle).to_radians()))
}

/// Couleur du dégradé à la position `t` (0 à 1).
fn sample(colors: &[Color32], t: f32) -> Color32 {
    let segments = colors.len().saturating_sub(1).max(1);
    let x = t.clamp(0.0, 1.0) * segments as f32;
    let i = (x.floor() as usize).min(segments - 1);
    let (a, b, f) = (colors[i], colors[(i + 1).min(colors.len() - 1)], x - i as f32);
    let mix = |u: u8, v: u8| (f32::from(u) + (f32::from(v) - f32::from(u)) * f).round() as u8;
    Color32::from_rgba_premultiplied(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()), mix(a.a(), b.a()))
}

/// Part de dégradé visible dans `rect`, le dégradé couvrant `screen` en entier :
/// deux panneaux voisins se raccordent sans couture.
fn gradient_mesh(rect: Rect, screen: Rect, colors: &[Color32], angle: f32) -> egui::Mesh {
    const N: usize = 6;
    let dir = vec2(angle.cos(), angle.sin());
    let extent = (screen.width() * dir.x.abs() + screen.height() * dir.y.abs()).max(1.0);
    let mut mesh = egui::Mesh::default();
    for y in 0..=N {
        for x in 0..=N {
            let p = pos2(egui::lerp(rect.left()..=rect.right(), x as f32 / N as f32), egui::lerp(rect.top()..=rect.bottom(), y as f32 / N as f32));
            mesh.colored_vertex(p, sample(colors, (p - screen.center()).dot(dir) / extent + 0.5));
        }
    }
    let row = (N + 1) as u32;
    for y in 0..N as u32 {
        for x in 0..N as u32 {
            let i = y * row + x;
            mesh.add_triangle(i, i + 1, i + row);
            mesh.add_triangle(i + 1, i + row + 1, i + row);
        }
    }
    mesh
}

/// Part de l'image (de taille `size`) visible dans `rect`, l'image couvrant `screen` en entier.
fn cover_uv(rect: Rect, screen: Rect, size: egui::Vec2) -> Rect {
    let (ia, sa) = (size.x / size.y.max(1.0), screen.width() / screen.height().max(1.0));
    let (uw, uh) = if ia > sa { (sa / ia, 1.0) } else { (1.0, ia / sa) };
    let (u0, v0) = ((1.0 - uw) / 2.0, (1.0 - uh) / 2.0);
    let u = |x: f32| u0 + (x - screen.left()) / screen.width().max(1.0) * uw;
    let v = |y: f32| v0 + (y - screen.top()) / screen.height().max(1.0) * uh;
    Rect::from_min_max(pos2(u(rect.left()), v(rect.top())), pos2(u(rect.right()), v(rect.bottom())))
}

/// Fond personnalisé d'un panneau, à appeler au début de son contenu : il se
/// peint sur l'aplat du panneau et sous ce qui suit.
pub(super) fn backdrop(ui: &egui::Ui, zone: Zone) {
    let Some(p) = ui.ctx().data(|d| d.get_temp::<Paint>(paint_id())) else { return };
    let (rect, screen) = (ui.clip_rect(), ui.ctx().content_rect());
    if !rect.is_positive() {
        return;
    }
    if let Some((colors, n, angle)) = p.gradient {
        ui.painter().add(egui::Shape::mesh(gradient_mesh(rect, screen, &colors[..n], angle)));
    }
    if let Some((texture, size, tint, everywhere)) = p.image
        && (everywhere || zone == Zone::Chat)
    {
        ui.painter().image(texture, rect, cover_uv(rect, screen, size), tint);
    }
}

/// Jetons d'un thème posé sur un fond personnalisé : les aplats de code, de
/// date et de texte masqué deviennent un voile, au lieu d'un rectangle opaque
/// qui tranche sur le dégradé ou l'image.
pub(super) fn soften(mut t: Tokens) -> Tokens {
    let c = t.markdown.code_bg;
    t.markdown.code_bg = Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), SOFT_ALPHA);
    t
}

/// Opacité des aplats de code sur un fond personnalisé.
const SOFT_ALPHA: u8 = 150;

/// Sous le titre d'une section dont le plugin est coupé : ses réglages n'ont pas d'effet.
fn plugin_note(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, id: crate::plugins::Id) {
    if !crate::plugins::on(id) {
        ui.label(egui::RichText::new(crate::i18n::table::plug::t::OFF_HERE.get(lang)).small().color(t.colors.idle));
    }
}

/// Teinte de l'image : assombrie de `dim` %, visible à `opacity` %.
fn image_tint(opacity: u8, dim: u8) -> Color32 {
    let v = (255.0 * (1.0 - f32::from(dim.min(100)) / 100.0)).round() as u8;
    Color32::from_rgba_unmultiplied(v, v, v, (f32::from(opacity.min(prefs::STRENGTH_MAX)) * 2.55).round() as u8)
}

impl App {
    /// Réglages d'apparence de cet ordinateur (aperçu, tests).
    pub fn look_prefs(&self) -> LookPrefs {
        (*self.local.prefs.look).clone()
    }

    /// Image de fond déjà décodée (aperçu : aucun fichier lu).
    pub fn look_backdrop_image(&mut self, image: egui::ColorImage) {
        let b = &self.local.prefs.look.backdrop;
        self.fx.backdrop_asked = Some((b.path.clone(), b.blur));
        self.fx.loading = false;
        self.fx.texture = Some(self.ctx.load_texture("fond", image, egui::TextureOptions::LINEAR));
    }

    /// Thèmes et profils montrés comme enregistrés (aperçu : aucun fichier lu).
    pub fn look_library(&mut self, themes: Vec<String>, profiles: Vec<String>) {
        (self.fx.themes, self.fx.profiles, self.fx.library_asked) = (themes, profiles, true);
    }

    pub(super) fn look_service(&mut self) -> &Look {
        let (ctx, folder) = (self.ctx.clone(), self.look.folder());
        self.fx.service.get_or_insert_with(|| Look::new(Arc::new(move || ctx.request_repaint()), folder))
    }

    /// Rien n'est peint sur les fonds (ni image, ni dégradé).
    pub(in crate::ui) fn look_paints_nothing(&self) -> bool {
        self.fx.paint.image.is_none() && self.fx.paint.gradient.is_none()
    }

    /// Réponses du fil des fichiers, puis effets des réglages : image de fond,
    /// polices, bascule jour/nuit. Rien n'est refait tant que rien ne change.
    pub(super) fn look_tick(&mut self) {
        let mut picked = None;
        let mut entries = Vec::new();
        // Thèmes partageables (lot E5) : traités après la boucle.
        let (mut packages, mut exported, mut installed) = (Vec::new(), Vec::new(), Vec::new());
        let dones = self.fx.service.as_ref().map(Look::poll).unwrap_or_default();
        {
            for done in dones {
                let fx = &mut self.fx;
                match done {
                    Done::Picked(path) => picked = Some(path),
                    Done::Backdrop { path, blur, image } => {
                        if fx.backdrop_asked.as_ref().is_some_and(|(p, b)| *p == path && *b == blur) {
                            fx.loading = false;
                            fx.message = image.is_none().then(|| pt::IMAGE_FAILED.get(self.lang).to_string());
                            fx.texture = image.map(|i| self.ctx.load_texture("fond", i, egui::TextureOptions::LINEAR));
                        }
                    }
                    Done::Fonts(all) => (fx.families, fx.listing) = (all, Listing::Ready),
                    Done::Font { ui, name, faces } => {
                        if ui && name == fx.fonts_asked.0 && faces[0].is_some() {
                            fx.custom.ui = Some(faces);
                        } else if !ui && name == fx.fonts_asked.1 {
                            let [regular, ..] = faces;
                            fx.custom.code = regular;
                        } else {
                            continue;
                        }
                        fonts::set_custom(&self.ctx, fx.custom.clone());
                    }
                    Done::Library { themes, profiles } => (fx.themes, fx.profiles) = (themes, profiles),
                    Done::Entry { kind, name, text } => entries.push((kind, name, text)),
                    Done::Package { result, preview } => packages.push((result, preview)),
                    Done::Exported(result) => exported.push(result),
                    Done::Installed { name, text } => installed.push((name, text)),
                    Done::Failed(what) => fx.message = Some(pf::entry_unreadable(self.lang, what)),
                }
            }
        }
        if let Some(path) = picked {
            let mut p = self.local_prefs();
            p.look.backdrop.path = path;
            self.set_local_prefs(p);
        }
        for (kind, name, text) in entries {
            self.look_entry(kind, &name, &text);
        }
        for (result, preview) in packages {
            self.theme_share_done(result, preview);
        }
        for result in exported {
            self.theme_share_exported(result);
        }
        for (name, text) in installed {
            self.look_entry(Kind::Profile, &name, &text);
            // Même chemin qu'une image d'avant : relue quand même.
            if !self.local.prefs.look.backdrop.path.is_empty() {
                self.fx.backdrop_asked = None;
            }
            if self.fx.message.is_none() {
                let text = crate::i18n::table::e5::f::share_added(self.lang, &name);
                self.fx.message = Some(text.clone());
                self.notify(text);
            }
        }

        // Image de fond : relue quand le chemin ou le flou change (pas pendant un glissement).
        // Plugin « Fond personnalisé » coupé : comme sans image, la texture est rendue.
        let backdrop_on = crate::plugins::on(crate::plugins::Id::Backdrop);
        let b = &self.local.prefs.look.backdrop;
        let asked = self.fx.backdrop_asked.as_ref();
        if b.path.is_empty() || !backdrop_on {
            if asked.is_some() {
                (self.fx.backdrop_asked, self.fx.texture, self.fx.loading) = (None, None, false);
            }
        } else if asked.is_none_or(|(p, blur)| *p != b.path || *blur != b.blur) && !self.ctx.egui_is_using_pointer() {
            let want = (b.path.clone(), b.blur);
            self.fx.backdrop_asked = Some(want.clone());
            self.fx.loading = true;
            self.look_service().load_backdrop(want.0, want.1);
        }

        // Polices installées choisies (plugin « Polices installées » coupé : celles du thème).
        let fonts_on = crate::plugins::on(crate::plugins::Id::CustomFonts);
        let l = &self.local.prefs.look;
        let wanted = if fonts_on { l.ui_font.as_str() } else { "" };
        if wanted != self.fx.fonts_asked.0 {
            let name = wanted.to_string();
            self.fx.fonts_asked.0 = name.clone();
            if name.is_empty() {
                self.fx.custom.ui = None;
                fonts::set_custom(&self.ctx, self.fx.custom.clone());
            } else {
                self.look_service().load_font(true, name);
            }
        }
        let l = &self.local.prefs.look;
        let wanted = if fonts_on { l.code_font.as_str() } else { "" };
        if wanted != self.fx.fonts_asked.1 {
            let name = wanted.to_string();
            self.fx.fonts_asked.1 = name.clone();
            if name.is_empty() {
                self.fx.custom.code = None;
                fonts::set_custom(&self.ctx, self.fx.custom.clone());
            } else {
                self.look_service().load_font(false, name);
            }
        }

        self.look_auto();

        // Ce que les panneaux peignent.
        let l = &self.local.prefs.look;
        let image = self.fx.texture.as_ref().filter(|_| backdrop_on && !l.backdrop.path.is_empty() && l.backdrop.opacity > 0).map(|t| (t.id(), t.size_vec2(), image_tint(l.backdrop.opacity, l.backdrop.dim), l.backdrop.scope == Scope::App));
        let paint = Paint { image, gradient: if backdrop_on { gradient_paint(&l.gradient) } else { None } };
        if paint != self.fx.paint {
            self.fx.paint = paint;
            self.ctx.data_mut(|d| d.insert_temp(paint_id(), paint));
            self.ctx.request_repaint();
        }
        if self.fx.dirty && !self.ctx.egui_is_using_pointer() {
            self.fx.dirty = false;
            self.save_local_prefs();
        }
    }

    /// Bascule entre le thème du jour et celui de la nuit, quand la phase change.
    fn look_auto(&mut self) {
        let l = &self.local.prefs.look;
        // Plugin « Thème du jour et de la nuit » coupé : aucune bascule, aucun réveil.
        if l.auto == Auto::Off || !crate::plugins::on(crate::plugins::Id::AutoTheme) {
            self.fx.auto_applied = None;
            return;
        }
        let dark = self.ctx.system_theme().map(|t| t == egui::Theme::Dark);
        let (hour, minute) = if l.auto == Auto::Clock {
            use chrono::Timelike;
            let now = chrono::Local::now();
            (now.hour() as u8, now.minute() as u8)
        } else {
            (0, 0)
        };
        if l.auto == Auto::Clock {
            // Un seul réveil, à la prochaine bascule : rien d'ici là.
            self.ctx.request_repaint_after(std::time::Duration::from_secs(u64::from(l.minutes_to_switch(hour, minute)) * 60));
        }
        let Some(phase) = l.phase(dark, hour) else { return };
        let name = l.theme_of(phase);
        if self.fx.auto_applied.as_ref().is_some_and(|(p, n)| *p == phase && n == name) {
            return;
        }
        let name = name.to_string();
        self.fx.auto_applied = Some((phase, name.clone()));
        self.look_apply_theme(&name);
    }

    /// Applique un préréglage (par sa clé) ou un thème enregistré (par son nom).
    fn look_apply_theme(&mut self, name: &str) {
        match Preset::from_name(name) {
            Some(p) => self.look.edited = p.tokens(),
            None => self.look_service().load_entry(Kind::Theme, name.to_string()),
        }
        self.ctx.request_repaint();
    }

    /// Thème ou profil relu sur le disque : appliqué, ou refusé avec un message.
    fn look_entry(&mut self, kind: Kind, name: &str, text: &str) {
        let (theme, extra) = match kind {
            Kind::Theme => (text, ""),
            Kind::Profile => look::split_profile(text),
        };
        match crate::theme::parse(theme) {
            Ok(tokens) => {
                self.look.edited = tokens;
                if let (Kind::Profile, Some(l)) = (kind, LookPrefs::parse(extra)) {
                    let mut p = self.local_prefs();
                    p.look = Box::new(l);
                    self.set_local_prefs(p);
                }
                self.fx.message = None;
            }
            Err(e) => self.fx.message = Some(format!("{} {e}", pf::entry_unreadable(self.lang, name))),
        }
        self.ctx.request_repaint();
    }

    /// Enregistre le thème en cours (ou tout le profil d'apparence) sous `name`.
    fn look_save(&mut self, kind: Kind, name: &str) {
        let lang = self.lang;
        let Some(name) = look::clean_name(name) else {
            self.fx.message = Some(pt::BAD_NAME.get(lang).into());
            return;
        };
        let theme = crate::theme::export(&self.look.edited.clone().sanitized());
        let text = match kind {
            Kind::Theme => theme,
            Kind::Profile => look::profile_text(&theme, &self.local.prefs.look.to_toml()),
        };
        self.fx.message = Some(pf::saved_as(lang, &name));
        self.look_service().save_entry(kind, name, text);
    }

    /// Sections de personnalisation, en tête de la page Apparence.
    pub(super) fn look_page(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if !self.fx.library_asked {
            self.fx.library_asked = true;
            self.look_service().list_library();
        }
        let has_folder = self.look_service().has_folder();
        let before = self.local.prefs.look.clone();
        let mut l = before.clone();

        super::appearance::heading(ui, t, pt::SEC_PREVIEW.get(lang));
        preview_card(ui, &self.look.edited.scaled(), &l, lang);

        // Thèmes : préréglages en pastilles, puis thèmes enregistrés.
        section(ui, t, pt::SEC_THEMES.get(lang));
        let mut chosen = None;
        ui.horizontal_wrapped(|ui| {
            for (p, colors) in Preset::ALL.iter().zip(swatches()) {
                if swatch(ui, t, p.label(lang), colors, self.look.edited.base == *p).clicked() {
                    chosen = Some(*p);
                }
            }
        });
        if let Some(p) = chosen {
            self.look.edited = p.tokens();
        }
        ui.add_space(t.spacing.item_y);
        ui.label(RichText::new(pt::SAVED_THEMES.get(lang)).family(crate::ui::fonts::demi()).color(t.colors.text_strong));
        self.library_ui(ui, t, Kind::Theme, has_folder);

        // Thèmes partageables : galerie, import, export (lot E5).
        self.theme_share_section(ui, t);

        // Éditeur de couleurs.
        section(ui, t, pt::SEC_COLORS.get(lang));
        row(ui, pt::ACCENT.get(lang), pt::ACCENT_TIP.get(lang), |ui| {
            ui.horizontal_wrapped(|ui| {
                for hex in ACCENTS {
                    let c = Color32::from_rgb((hex >> 16) as u8, (hex >> 8) as u8, hex as u8);
                    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(ui.spacing().interact_size.y * 0.7), Sense::click());
                    ui.painter().circle_filled(rect.center(), rect.width() / 2.0, c);
                    if self.look.edited.colors.accent == c {
                        ui.painter().circle_stroke(rect.center(), rect.width() / 2.0 + 2.0, Stroke::new(2.0, t.colors.text_strong));
                    }
                    super::a11y::name(ui.ctx(), resp.id, &fmt_color(c));
                    if resp.on_hover_text(fmt_color(c)).clicked() {
                        self.look.edited.colors.accent = c;
                    }
                }
            });
        });
        ui.push_id("editeur-de-theme", |ui| {
            let mut fields = self.look.edited.fields_mut();
            for (sect, key) in EDITOR_COLORS {
                let Some(f) = fields.iter_mut().find(|f| f.section == *sect && f.key == *key) else { continue };
                let (name, tip) = (f.text.label(lang), f.text.detail(lang));
                if let FieldMut::Color(c) = &mut f.value {
                    let c: &mut Color32 = c;
                    row(ui, name, tip, |ui| {
                        color_button(ui, c, name);
                        hex_field(ui, key, c, pt::HEX_TIP.get(lang));
                    });
                }
            }
        });
        contrast_report(ui, t, lang, &self.look.edited);

        // Dégradé.
        section(ui, t, pt::SEC_GRADIENT.get(lang));
        plugin_note(ui, t, lang, crate::plugins::Id::Backdrop);
        let g = &mut l.gradient;
        if let Some(v) = toggle(ui, pt::GRADIENT_ON.get(lang), pt::GRADIENT_ON_TIP.get(lang), Some(g.on), true) {
            g.on = v;
        }
        row(ui, pt::GRADIENT_READY.get(lang), pt::GRADIENT_ON_TIP.get(lang), |ui| {
            ui.horizontal_wrapped(|ui| {
                for ((_, colors, angle), name) in prefs::GRADIENTS.iter().zip(GRADIENT_NAMES) {
                    let (rect, resp) = ui.allocate_exact_size(vec2(2.2, 1.0) * ui.spacing().interact_size.y * 0.8, Sense::click());
                    let solid: Vec<Color32> = colors.iter().map(|c| Color32::from_rgb(c[0], c[1], c[2])).collect();
                    ui.painter().with_clip_rect(rect).add(egui::Shape::mesh(gradient_mesh(rect, rect, &solid, f32::from(*angle).to_radians())));
                    let current = g.on && g.colors == *colors && g.angle == *angle;
                    ui.painter().rect_stroke(rect, t.r_control(), Stroke::new(if current { 2.0 } else { 1.0 }, if current { t.colors.text_strong } else { t.colors.border }), egui::StrokeKind::Inside);
                    super::a11y::name(ui.ctx(), resp.id, name.get(lang));
                    if resp.on_hover_text(name.get(lang)).clicked() {
                        (g.on, g.colors, g.angle) = (true, colors.to_vec(), *angle);
                    }
                }
            });
        });
        if g.on {
            row(ui, pt::GRADIENT_COLORS.get(lang), pt::GRADIENT_ON_TIP.get(lang), |ui| {
                for (i, c) in g.colors.iter_mut().enumerate() {
                    let mut color = Color32::from_rgb(c[0], c[1], c[2]);
                    color_button(ui, &mut color, pt::GRADIENT_COLORS.get(lang));
                    hex_field(ui, ["degrade-0", "degrade-1", "degrade-2"][i.min(2)], &mut color, pt::HEX_TIP.get(lang));
                    *c = [color.r(), color.g(), color.b()];
                }
            });
            if let Some(third) = toggle(ui, pt::GRADIENT_THIRD.get(lang), pt::GRADIENT_ON_TIP.get(lang), Some(g.colors.len() > 2), true) {
                if third {
                    g.colors.push(g.colors.last().copied().unwrap_or_default());
                } else {
                    g.colors.truncate(2);
                }
            }
            slider(ui, pt::GRADIENT_ANGLE.get(lang), pt::GRADIENT_ANGLE_TIP.get(lang), &mut g.angle, 0..=prefs::ANGLE_MAX, 5.0, |v| pf::degrees(lang, v));
            slider(ui, pt::STRENGTH.get(lang), pt::STRENGTH_TIP.get(lang), &mut g.strength, 5..=prefs::STRENGTH_MAX, 1.0, |v| percent(lang, v));
        }

        // Image de fond.
        section(ui, t, pt::SEC_BACKDROP.get(lang));
        plugin_note(ui, t, lang, crate::plugins::Id::Backdrop);
        let b = &mut l.backdrop;
        let mut pick = false;
        row(ui, pt::IMAGE.get(lang), pt::IMAGE_TIP.get(lang), |ui| {
            if b.path.is_empty() {
                ui.label(RichText::new(pt::IMAGE_NONE.get(lang)).color(t.colors.muted));
            } else {
                let name = std::path::Path::new(&b.path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| b.path.clone());
                let shown = if self.fx.loading { pt::IMAGE_LOADING.get(lang).to_string() } else { name };
                ui.add(egui::Label::new(shown).truncate()).on_hover_text(&b.path);
            }
        });
        ui.horizontal(|ui| {
            pick = ui.add_enabled(Look::CAN_PICK && !self.fx.service.as_ref().is_some_and(Look::picking), egui::Button::new(pt::IMAGE_CHOOSE.get(lang))).clicked();
            if !b.path.is_empty() && ui.button(pt::IMAGE_REMOVE.get(lang)).clicked() {
                b.path.clear();
            }
        });
        if pick {
            self.look_service().pick_image(pt::IMAGE_TITLE.get(lang));
        }
        if !b.path.is_empty() {
            if let Some(s) = super::settings::choice(ui, lang, "fond-zone", (pt::SCOPE.get(lang), pt::SCOPE_TIP.get(lang)), &SCOPES, Some(b.scope), true) {
                b.scope = s;
            }
            slider(ui, pt::OPACITY.get(lang), pt::OPACITY_TIP.get(lang), &mut b.opacity, 5..=prefs::STRENGTH_MAX, 1.0, |v| percent(lang, v));
            slider(ui, pt::BLUR.get(lang), pt::BLUR_TIP.get(lang), &mut b.blur, 0..=prefs::BLUR_MAX, 1.0, |v| v.to_string());
            slider(ui, pt::DIM.get(lang), pt::DIM_TIP.get(lang), &mut b.dim, 0..=100, 1.0, |v| percent(lang, v));
        }

        // Jour et nuit.
        section(ui, t, pt::SEC_AUTO.get(lang));
        plugin_note(ui, t, lang, crate::plugins::Id::AutoTheme);
        if let Some(a) = super::settings::choice(ui, lang, "theme-auto", (pt::AUTO.get(lang), pt::AUTO_TIP.get(lang)), &AUTOS, Some(l.auto), true) {
            l.auto = a;
        }
        if l.auto != Auto::Off {
            let themes = &self.fx.themes;
            theme_combo(ui, lang, "theme-jour", pt::DAY_THEME, themes, &mut l.day);
            theme_combo(ui, lang, "theme-nuit", pt::NIGHT_THEME, themes, &mut l.night);
        }
        if l.auto == Auto::Clock {
            slider(ui, pt::DAY_HOUR.get(lang), pt::HOUR_TIP.get(lang), &mut l.day_hour, 0..=23, 1.0, |v| pf::hour(lang, v));
            slider(ui, pt::NIGHT_HOUR.get(lang), pt::HOUR_TIP.get(lang), &mut l.night_hour, 0..=23, 1.0, |v| pf::hour(lang, v));
        }
        if l.auto == Auto::System && ui.ctx().system_theme().is_none() {
            note(ui, t, pt::SYSTEM_UNKNOWN.get(lang));
        }

        // Polices installées.
        section(ui, t, pt::SEC_FONTS.get(lang));
        plugin_note(ui, t, lang, crate::plugins::Id::CustomFonts);
        let mut list = false;
        let listing = self.fx.listing;
        let families = &self.fx.families;
        list |= font_combo(ui, lang, "police-interface", (pt::UI_FONT, pt::UI_FONT_TIP, pt::FONT_THEME), families.iter().filter(|f| !f.mono), listing, &mut l.ui_font);
        list |= font_combo(ui, lang, "police-code", (pt::CODE_FONT, pt::CODE_FONT_TIP, pt::FONT_CONSOLAS), families.iter().filter(|f| f.mono), listing, &mut l.code_font);
        if list && listing == Listing::NotAsked {
            self.fx.listing = Listing::Asked;
            self.look_service().list_fonts();
        }

        // Détails.
        section(ui, t, pt::SEC_DETAILS.get(lang));
        if let Some(n) = super::settings::choice(ui, lang, "couleur-noms", (pt::NAME_COLOR.get(lang), pt::NAME_COLOR_TIP.get(lang)), &NAME_COLORS, Some(l.name_color), true) {
            l.name_color = n;
        }
        if let Some(v) = toggle(ui, pt::HIDE_RAIL.get(lang), pt::HIDE_RAIL_TIP.get(lang), Some(l.hide_rail), true) {
            l.hide_rail = v;
        }
        let mut open = false;
        row(ui, pt::ANIMATIONS.get(lang), pt::ANIMATIONS_TIP.get(lang), |ui| {
            open = ui.button(pt::ANIMATIONS_OPEN.get(lang)).clicked();
        });
        if open {
            self.params.show(super::settings::Page::Accessibility);
        }

        // Profils d'apparence.
        section(ui, t, pt::SEC_PROFILES.get(lang));
        note(ui, t, pt::PROFILES_TIP.get(lang));
        if l != before {
            // Le profil enregistré ci-dessous est celui qu'on voit.
            self.local.prefs.look = l.clone();
        }
        self.library_ui(ui, t, Kind::Profile, has_folder);
        ui.add_space(t.spacing.item_y);
        if widgets::button(ui, t, Button::Secondary, pt::RESET_ALL.get(lang)).on_hover_text(pt::RESET_ALL_TIP.get(lang)).clicked() {
            self.look.edited = Preset::DEFAULT.tokens();
            *self.local.prefs.look = Default::default();
            self.fx.message = None;
        }
        if let Some(m) = &self.fx.message {
            ui.add(egui::Label::new(RichText::new(m).color(t.colors.muted)).wrap());
        }

        if self.local.prefs.look != before {
            // Aperçu immédiat ; l'enregistrement attend la fin du glissement.
            self.fx.dirty = true;
            self.ctx.request_repaint();
        }
    }

    /// Liste des thèmes ou des profils enregistrés, puis « enregistrer sous ».
    fn library_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, kind: Kind, has_folder: bool) {
        let lang = self.lang;
        let names = match kind {
            Kind::Theme => self.fx.themes.clone(),
            Kind::Profile => self.fx.profiles.clone(),
        };
        let (mut apply, mut copy, mut delete) = (None, None, None);
        if names.is_empty() {
            ui.label(RichText::new(pt::NO_SAVED.get(lang)).small().color(t.colors.faint));
        }
        for name in &names {
            ui.horizontal(|ui| {
                ui.add(egui::Label::new(RichText::new(name).color(t.colors.text_strong)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::ui::widgets::compact(ui, bt::DELETE.get(lang)).clicked() {
                    delete = Some(name.clone());
                }
                if crate::ui::widgets::compact(ui, pt::DUPLICATE.get(lang)).clicked() {
                    copy = Some(name.clone());
                }
                if crate::ui::widgets::compact(ui, pt::APPLY.get(lang)).clicked() {
                    apply = Some(name.clone());
                }
                });
            });
        }
        if let Some(name) = apply {
            self.look_service().load_entry(kind, name);
        }
        if let Some(name) = delete {
            self.look_service().delete_entry(kind, name);
        }
        if let Some(name) = copy
            && let Some(to) = look::clean_name(&pf::copy_of(lang, &name))
        {
            self.look_service().copy_entry(kind, name, to);
        }
        let tip = match kind {
            Kind::Theme => pt::SAVE_THEME_TIP,
            Kind::Profile => pt::SAVE_PROFILE_TIP,
        };
        let mut save = false;
        ui.horizontal(|ui| {
            let field = match kind {
                Kind::Theme => &mut self.fx.theme_name,
                Kind::Profile => &mut self.fx.profile_name,
            };
            let width = (ui.available_width() * 0.45).max(120.0);
            let r = ui.add_enabled(has_folder, egui::TextEdit::singleline(field).hint_text(pt::NAME_HINT.get(lang)).char_limit(look::NAME_MAX).desired_width(width));
            super::a11y::name(ui.ctx(), r.id, pt::NAME_HINT.get(lang));
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            save = ui.add_enabled(has_folder, egui::Button::new(pt::SAVE_AS.get(lang))).on_hover_text(tip.get(lang)).clicked() || enter;
        });
        if !has_folder {
            ui.label(RichText::new(pt::NO_LIBRARY.get(lang)).small().color(t.colors.faint));
        }
        if save {
            let name = match kind {
                Kind::Theme => self.fx.theme_name.clone(),
                Kind::Profile => self.fx.profile_name.clone(),
            };
            self.look_save(kind, &name);
        }
    }
}

/// Pastilles d'accent proposées.
const ACCENTS: [u32; 10] = [0x5865f2, 0x3ba55d, 0xe67e22, 0xed4245, 0xeb459e, 0x9b59b6, 0x1abc9c, 0xf1c40f, 0x3498db, 0x99aab5];

/// Couleurs de l'éditeur : fonds à quatre niveaux, texte, statuts, mentions.
const EDITOR_COLORS: &[(&str, &str)] = &[
    ("couleurs", "accent"),
    ("couleurs", "bg0"),
    ("couleurs", "bg1"),
    ("couleurs", "bg2"),
    ("couleurs", "bg3"),
    ("couleurs", "text"),
    ("couleurs", "text_strong"),
    ("couleurs", "muted"),
    ("couleurs", "online"),
    ("couleurs", "idle"),
    ("couleurs", "dnd"),
    ("couleurs", "offline"),
    ("texte_riche", "mention_bg"),
    ("texte_riche", "mention_text"),
];

const GRADIENT_NAMES: [Text; 8] = [pt::G_CREPUSCULE, pt::G_LAGON, pt::G_AURORE, pt::G_BRAISE, pt::G_FORET, pt::G_NUIT, pt::G_BONBON, pt::G_ARDOISE];
const SCOPES: [(Scope, Text); 2] = [(Scope::Chat, pt::SCOPE_CHAT), (Scope::App, pt::SCOPE_APP)];
const AUTOS: [(Auto, Text); 3] = [(Auto::Off, pt::AUTO_OFF), (Auto::System, pt::AUTO_SYSTEM), (Auto::Clock, pt::AUTO_CLOCK)];
const NAME_COLORS: [(NameColor, Text); 2] = [(NameColor::Role, pt::NAME_ROLE), (NameColor::Dot, pt::NAME_DOT)];

fn percent(lang: Lang, v: impl Into<u64>) -> String {
    crate::i18n::table::s2::f::percent(lang, v.into())
}

/// Couleurs de chaque préréglage (rail, salons, discussion, accent, texte), calculées une fois.
fn swatches() -> &'static [[Color32; 5]] {
    static ALL: std::sync::OnceLock<Vec<[Color32; 5]>> = std::sync::OnceLock::new();
    ALL.get_or_init(|| {
        Preset::ALL
            .iter()
            .map(|p| {
                let c = p.tokens().colors;
                [c.bg0, c.bg1, c.bg2, c.accent, c.text]
            })
            .collect()
    })
}

/// Pastille d'un préréglage : ses trois fonds, son accent, son nom dessous.
pub(super) fn swatch(ui: &mut egui::Ui, t: &Tokens, name: &str, [bg0, bg1, bg2, accent, text]: &[Color32; 5], current: bool) -> egui::Response {
    let side = ui.spacing().interact_size.y;
    let size = vec2(side * 2.6, side * 1.5);
    let label_h = (t.typography.small * 1.4).ceil();
    let (rect, resp) = ui.allocate_exact_size(size + vec2(0.0, label_h + 2.0), Sense::click());
    let card = Rect::from_min_size(rect.min, size);
    let p = ui.painter().with_clip_rect(card);
    p.rect_filled(card, t.r_control(), *bg2);
    // Bande du rail : arrondie à gauche comme la vignette (carrée, elle en masquait les coins).
    let r = t.r_control();
    p.rect_filled(card.with_max_x(card.left() + size.x * 0.14), egui::CornerRadius { nw: r.nw, sw: r.sw, ne: 0, se: 0 }, *bg0);
    p.rect_filled(Rect::from_min_max(pos2(card.left() + size.x * 0.14, card.top()), pos2(card.left() + size.x * 0.42, card.bottom())), 0.0, *bg1);
    p.circle_filled(pos2(card.left() + size.x * 0.07, card.top() + size.y * 0.25), size.x * 0.04, *accent);
    for (i, w) in [0.4, 0.3].iter().enumerate() {
        let y = card.top() + size.y * (0.35 + 0.25 * i as f32);
        p.rect_filled(Rect::from_min_size(pos2(card.left() + size.x * 0.5, y), vec2(size.x * w, 3.0)), 1.5, *text);
    }
    let stroke = if current { Stroke::new(2.0, t.colors.accent) } else if resp.hovered() { Stroke::new(1.0, t.colors.text) } else { Stroke::new(1.0, t.colors.border) };
    ui.painter().rect_stroke(card, t.r_control(), stroke, egui::StrokeKind::Inside);
    let color = if current { t.colors.text_strong } else { t.colors.muted };
    ui.painter().text(pos2(card.center().x, card.bottom() + 2.0), egui::Align2::CENTER_TOP, name, egui::FontId::proportional(t.typography.small), color);
    super::a11y::name(ui.ctx(), resp.id, name);
    resp
}

/// Champ hexadécimal d'une couleur : la couleur change dès que le code est valide.
fn hex_field(ui: &mut egui::Ui, salt: &str, c: &mut Color32, tip: &str) {
    let id = ui.scope_id().with(("hex", salt));
    let mut text = ui.data(|d| d.get_temp::<String>(id)).unwrap_or_else(|| fmt_color(*c));
    let width = ui.text_style_height(&egui::TextStyle::Monospace) * 5.0;
    let r = ui.add(egui::TextEdit::singleline(&mut text).font(egui::TextStyle::Monospace).char_limit(9).desired_width(width)).on_hover_text(tip);
    super::a11y::name(ui.ctx(), r.id, tip);
    if r.changed()
        && let Some(new) = parse_color(&text).or_else(|| parse_color(&format!("#{text}")))
    {
        *c = new;
    }
    // Pendant la frappe, le texte saisi reste ; sinon le champ suit la couleur.
    let typing = r.has_focus();
    ui.data_mut(|d| if typing { d.insert_temp(id, text); } else { d.remove::<String>(id); });
}

fn slider<N: egui::emath::Numeric>(ui: &mut egui::Ui, label: &str, tip: &str, v: &mut N, range: std::ops::RangeInclusive<N>, step: f64, show: impl Fn(u16) -> String) {
    row(ui, label, tip, |ui| {
        let r = ui.add(egui::Slider::new(v, range).step_by(step).custom_formatter(|v, _| show(v as u16)));
        super::a11y::name(ui.ctx(), r.id, label);
    });
}

/// Liste des thèmes du jour ou de la nuit : préréglages, puis thèmes enregistrés.
fn theme_combo(ui: &mut egui::Ui, lang: Lang, id: &str, label: Text, saved: &[String], value: &mut String) {
    row(ui, label.get(lang), pt::DAY_NIGHT_TIP.get(lang), |ui| {
        let shown = Preset::from_name(value).map_or(value.as_str(), |p| p.label(lang)).to_string();
        egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
            for p in Preset::ALL {
                if ui.selectable_label(value == p.key(), p.label(lang)).clicked() {
                    *value = p.key().to_string();
                }
            }
            for name in saved.iter().filter(|n| Preset::from_name(n).is_none()) {
                if ui.selectable_label(value == name, name).clicked() {
                    value.clone_from(name);
                }
            }
        });
    });
}

/// Liste d'une police installée ; rend vrai quand la liste s'ouvre (les
/// polices installées ne sont lues qu'à ce moment).
fn font_combo<'a>(ui: &mut egui::Ui, lang: Lang, id: &str, (label, tip, default): (Text, Text, Text), families: impl Iterator<Item = &'a Family>, listing: Listing, value: &mut String) -> bool {
    let mut opened = false;
    row(ui, label.get(lang), tip.get(lang), |ui| {
        let shown = if value.is_empty() { default.get(lang) } else { value.as_str() }.to_string();
        egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
            opened = true;
            if ui.selectable_label(value.is_empty(), default.get(lang)).clicked() {
                value.clear();
            }
            if listing != Listing::Ready {
                ui.label(RichText::new(pt::FONTS_LOADING.get(lang)).weak());
            }
            for f in families {
                if ui.selectable_label(*value == f.name, &f.name).clicked() {
                    value.clone_from(&f.name);
                }
            }
        });
    });
    opened
}

/// Combinaisons à vérifier : (nom, texte, fond, minimum).
pub(super) fn contrast_checks(e: &Tokens) -> Vec<(Text, Color32, Color32, f64)> {
    let (c, m) = (&e.colors, &e.markdown);
    // Le fond des mentions est translucide : on le pose sur le fond de la discussion.
    let a = f32::from(m.mention_bg.a()) / 255.0;
    let [r, g, b, _] = m.mention_bg.to_srgba_unmultiplied();
    let over = |fg: u8, bg: u8| (f32::from(fg) * a + f32::from(bg) * (1.0 - a)).round() as u8;
    let mention = Color32::from_rgb(over(r, c.bg2.r()), over(g, c.bg2.g()), over(b, c.bg2.b()));
    let mut out = Vec::new();
    for bg in [c.bg0, c.bg1, c.bg2] {
        out.push((bt::CONTRAST_TEXT, c.text, bg, MIN_TEXT));
        out.push((bt::CONTRAST_MUTED, c.muted, bg, MIN_MUTED));
    }
    out.push((bt::CONTRAST_FAINT, c.faint, c.bg2, MIN_MUTED));
    out.push((pt::CONTRAST_ACCENT, c.on_accent, c.accent, MIN_TEXT));
    out.push((pt::CONTRAST_MENTION, m.mention_text, mention, MIN_TEXT));
    out
}

/// Contraste vérifié en direct : un avertissement par combinaison trop faible
/// (la pire par nom), sinon une ligne qui rassure.
pub(super) fn contrast_report(ui: &mut egui::Ui, t: &Tokens, lang: Lang, e: &Tokens) {
    let mut worst: Vec<(Text, f64, f64)> = Vec::new();
    for (name, fg, bg, min) in contrast_checks(e) {
        let r = contrast_ratio(fg, bg);
        if r >= min {
            continue;
        }
        match worst.iter_mut().find(|(n, ..)| *n == name) {
            Some(w) => w.1 = w.1.min(r),
            None => worst.push((name, r, min)),
        }
    }
    if worst.is_empty() {
        ui.label(RichText::new(pt::CONTRAST_OK.get(lang)).small().color(t.colors.faint));
    }
    for (name, r, min) in worst {
        let msg = i18n::f::low_contrast(lang, name.get(lang), i18n::decimal(lang, r, 1), i18n::decimal(lang, min, 1));
        ui.add(egui::Label::new(RichText::new(msg).color(t.colors.danger)).wrap());
    }
}

/// Aperçu en direct : une petite fenêtre peinte avec le thème en cours d'édition.
pub(super) fn preview_card(ui: &mut egui::Ui, e: &Tokens, l: &LookPrefs, lang: Lang) {
    let c = &e.colors;
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width().min(520.0), 132.0), Sense::hover());
    let p = ui.painter().with_clip_rect(rect);
    let r = e.r_embed();
    let (rail_w, chan_w) = (34.0, 104.0);
    let rail = rect.with_max_x(rect.left() + rail_w);
    let chans = Rect::from_min_max(pos2(rail.right(), rect.top()), pos2(rail.right() + chan_w, rect.bottom()));
    let chat = rect.with_min_x(chans.right());
    p.rect_filled(rect, r, c.bg2);
    p.rect_filled(rail, egui::CornerRadius { nw: r.nw, sw: r.sw, ne: 0, se: 0 }, c.bg0);
    p.rect_filled(chans, 0.0, c.bg1);
    if let Some((colors, n, angle)) = gradient_paint(&l.gradient) {
        p.add(egui::Shape::mesh(gradient_mesh(rect, rect, &colors[..n], angle)));
    }
    // Rail : serveur ouvert (accent, carré arrondi) et deux autres.
    for i in 0..3 {
        let center = pos2(rail.center().x, rail.top() + 22.0 + 30.0 * i as f32);
        let tile = Rect::from_center_size(center, egui::Vec2::splat(22.0));
        p.rect_filled(tile, if i == 0 { 7.0 } else { 11.0 }, if i == 0 { c.accent } else { c.bg2 });
    }
    // Salons : un sélectionné, un non lu.
    let small = egui::FontId::proportional(e.typography.small);
    for (i, w) in [52.0, 64.0, 40.0, 58.0].iter().enumerate() {
        let row = Rect::from_min_size(pos2(chans.left() + 6.0, chans.top() + 12.0 + 26.0 * i as f32), vec2(chan_w - 12.0, 22.0));
        if i == 1 {
            p.rect_filled(row, e.r_row(), c.selected);
        }
        let ink = if i == 1 { c.text_strong } else if i == 2 { c.text } else { c.channel_idle };
        p.text(pos2(row.left() + 6.0, row.center().y), egui::Align2::LEFT_CENTER, "#", small.clone(), ink);
        p.rect_filled(Rect::from_min_size(pos2(row.left() + 20.0, row.center().y - 2.0), vec2(*w, 4.0)), 2.0, ink);
    }
    // Discussion : deux messages, une mention, la saisie.
    let body = egui::FontId::proportional(e.typography.body.min(15.0));
    let demi = fonts::demi_font(e.typography.body.min(15.0));
    let side = 26.0;
    for (i, (name, status, author)) in [(pt::PREVIEW_NAME_A, c.online, 0usize), (pt::PREVIEW_NAME_B, c.idle, 3)].iter().enumerate() {
        let top = chat.top() + 12.0 + 42.0 * i as f32;
        let avatar = Rect::from_min_size(pos2(chat.left() + 12.0, top), egui::Vec2::splat(side));
        p.rect_filled(avatar, e.r_avatar(side), c.authors[*author]);
        p.circle_filled(avatar.max - egui::Vec2::splat(3.0), 5.5, c.bg2);
        p.circle_filled(avatar.max - egui::Vec2::splat(3.0), 3.5, *status);
        let x = avatar.right() + 10.0;
        let end = p.text(pos2(x, top - 2.0), egui::Align2::LEFT_TOP, name.get(lang), demi.clone(), c.authors[*author]).right();
        p.text(pos2(end + 8.0, top + 1.0), egui::Align2::LEFT_TOP, "12:34", small.clone(), c.faint);
        let y = top + 17.0;
        if i == 0 {
            p.rect_filled(Rect::from_min_size(pos2(x, y + 4.0), vec2(150.0, 5.0)), 2.5, c.text);
        } else {
            let chip = p.layout_no_wrap(pt::PREVIEW_MENTION.get(lang).into(), body.clone(), e.markdown.mention_text);
            let back = Rect::from_min_size(pos2(x, y - 1.0), chip.size() + vec2(6.0, 2.0));
            p.rect_filled(back, 3.0, e.markdown.mention_bg);
            p.galley(back.min + vec2(3.0, 1.0), chip, e.markdown.mention_text);
            p.rect_filled(Rect::from_min_size(pos2(back.right() + 8.0, y + 4.0), vec2(90.0, 5.0)), 2.5, c.text);
        }
    }
    let composer = Rect::from_min_max(pos2(chat.left() + 12.0, chat.bottom() - 34.0), pos2(chat.right() - 12.0, chat.bottom() - 10.0));
    p.rect_filled(composer, e.r_composer(), c.bg3);
    p.rect_filled(Rect::from_min_size(pos2(composer.left() + 10.0, composer.center().y - 2.0), vec2(80.0, 4.0)), 2.0, c.muted);
    let send = Rect::from_min_size(pos2(composer.right() - 26.0, composer.top() + 4.0), vec2(20.0, 16.0));
    p.rect_filled(send, e.r_control(), c.accent);
    ui.painter().rect_stroke(rect, r, e.border(), egui::StrokeKind::Inside);
    ui.add_space(e.spacing.item_y);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gradient_covers_the_screen_without_seams() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
        let colors = [Color32::from_rgb(255, 0, 0), Color32::from_rgb(0, 255, 0), Color32::from_rgb(0, 0, 255)];
        // Trois couleurs : le début, le milieu, la fin.
        assert_eq!(sample(&colors, 0.0), colors[0]);
        assert_eq!(sample(&colors, 0.5), colors[1]);
        assert_eq!(sample(&colors, 1.0), colors[2]);
        assert_eq!(sample(&colors[..2], 2.0), colors[1]);
        // Angle 0 : de gauche à droite sur l'écran entier, quel que soit le panneau.
        let left = gradient_mesh(Rect::from_min_size(pos2(0.0, 0.0), vec2(500.0, 600.0)), screen, &colors, 0.0);
        let right = gradient_mesh(Rect::from_min_size(pos2(500.0, 0.0), vec2(500.0, 600.0)), screen, &colors, 0.0);
        assert_eq!(left.vertices.first().map(|v| v.color), Some(colors[0]));
        assert_eq!(right.vertices.last().map(|v| v.color), Some(colors[2]));
        // Les deux panneaux se raccordent : même couleur sur le bord commun.
        assert_eq!(left.vertices[6].color, right.vertices[0].color);
        assert_eq!(left.vertices[6].color, colors[1]);
        assert_eq!(left.indices.len(), 6 * 6 * 6);
    }

    #[test]
    fn image_covers_the_screen() {
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 500.0));
        // Image carrée sur un écran large : toute la largeur, le milieu de la hauteur.
        let uv = cover_uv(screen, screen, vec2(800.0, 800.0));
        assert_eq!((uv.left(), uv.right()), (0.0, 1.0));
        assert!((uv.top() - 0.25).abs() < 1e-6 && (uv.bottom() - 0.75).abs() < 1e-6);
        // Un panneau : la part de l'image qui est derrière lui.
        let uv = cover_uv(Rect::from_min_size(pos2(500.0, 0.0), vec2(500.0, 500.0)), screen, vec2(2000.0, 1000.0));
        assert_eq!((uv.left(), uv.right(), uv.top(), uv.bottom()), (0.5, 1.0, 0.0, 1.0));
        assert_eq!(image_tint(100, 0).a(), 230, "opacité bornée");
        assert_eq!(image_tint(50, 100).r(), 0);
    }

    #[test]
    fn paint_follows_the_settings() {
        assert_eq!(gradient_paint(&Gradient::default()), None);
        let g = Gradient { on: true, strength: 50, ..Default::default() };
        let (colors, n, _) = gradient_paint(&g).unwrap();
        assert_eq!(n, g.colors.len());
        assert_eq!(colors[0].a(), 128);
    }

    /// Chaque préréglage passe le contrôle de contraste de l'éditeur ; un
    /// texte de la couleur du fond est signalé.
    #[test]
    fn presets_pass_the_live_contrast_check() {
        for p in [Preset::Sombre, Preset::Cendre, Preset::Onyx, Preset::Clair, Preset::Rouille] {
            let t = p.tokens();
            for (name, fg, bg, min) in contrast_checks(&t) {
                assert!(contrast_ratio(fg, bg) >= min, "{} {}", p.key(), name.get(Lang::Fr));
            }
        }
        let mut t = Preset::Sombre.tokens();
        t.colors.text = t.colors.bg2;
        assert!(contrast_checks(&t).iter().any(|(_, fg, bg, min)| contrast_ratio(*fg, *bg) < *min));
        for (s, k) in EDITOR_COLORS {
            assert!(t.fields_mut().iter().any(|f| f.section == *s && f.key == *k && matches!(f.value, FieldMut::Color(_))), "{s}.{k}");
        }
    }
}
