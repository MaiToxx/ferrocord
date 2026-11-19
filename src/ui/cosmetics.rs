//! Parures des profils (lot P) : noms en dégradé (rôles, style du nom),
//! décoration d'avatar, plaque nominative, tag de serveur, couleurs de thème
//! du profil.
//!
//! Rien ici ne bouge tout seul : une décoration n'est animée que si
//! l'appelant le demande (profil ouvert), et les listes dessinent des images
//! fixes ; seule la plaque de la ligne que survole le pointeur s'anime
//! (comme les avatars animés), et s'arrête dès qu'il la quitte. Les zones
//! qui dessinent des noms lisent les rôles du serveur
//! affiché dans une table déposée dans le contexte d'egui ([`install`]),
//! comme les présences (`profile::install`).

use crate::images::{self, Image, Images};
use crate::state::cosmetics::{Asset, GuildTag, Look, NameColors, NameEffect, NameStyle, Nameplate};
use crate::state::model::{Avatar, ImageHash, ImageRef};
use crate::state::Store;
use crate::theme::{contrast_ratio, Tokens, MIN_MUTED, MIN_TEXT};
use eframe::egui::{self, text::LayoutJob, Color32, CornerRadius, FontId, Id, Pos2, Rect, Sense, TextFormat};
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

/// Proportions des bannières de profil de Discord (680 × 240).
pub(super) const BANNER_ASPECT: f32 = 17.0 / 6.0;
/// Proportions approximatives de l'image d'une plaque (large et basse).
const NAMEPLATE_ASPECT: f32 = 16.0 / 3.0;
/// Voile d'une ligne à plaque : opacité au bord gauche, et part de la largeur
/// sur laquelle il s'éteint.
const PLATE_VEIL: f32 = 0.6;
const PLATE_VEIL_SPAN: f32 = 0.85;
/// Une décoration déborde l'avatar d'un dixième de chaque côté.
const DECORATION_SCALE: f32 = 1.2;
/// Hauteur sur laquelle le dégradé d'un profil passe d'une couleur à l'autre.
const THEME_SPAN: f32 = 420.0;
/// Hauteur minimale de ce dégradé (carte très courte).
const MIN_THEME_SPAN: f32 = 160.0;

fn roles_id() -> Id {
    Id::unique("ferrocord.parures.roles")
}

/// Rôles colorés du serveur affiché : position et couleurs.
#[derive(Clone)]
struct RoleTable {
    guild: u64,
    /// Empreinte de ce qui a servi à la construire.
    print: u64,
    roles: Arc<HashMap<u64, (i32, NameColors)>>,
    /// Propriétaire du serveur (couronne de la liste des membres).
    owner: Option<u64>,
    /// Rôles qui ont une icône (image ou emoji) : position et icône.
    icons: Arc<HashMap<u64, (i32, RoleBadge)>>,
}

/// Icône d'un rôle, montrée à côté du nom de ses membres.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RoleBadge {
    Image(crate::state::model::ImageRef),
    Emoji(crate::state::emoji::Unicode),
}

/// Dépose les rôles colorés du serveur affiché. La table n'est refaite que
/// si un rôle a changé (une empreinte de quelques centaines d'entiers par
/// image, puis une comparaison).
pub(super) fn install(ctx: &egui::Context, store: &Store) {
    let Some(g) = store.guild.and_then(|id| store.guilds.iter().find(|g| g.id == id)) else {
        if table(ctx).is_some() {
            ctx.data_mut(|d| d.remove::<RoleTable>(roles_id()));
        }
        return;
    };
    let gradient = |role_id: u64| store.roles_admin.extra(g.id, role_id).map(|x| (x.secondary_color, x.tertiary_color)).unwrap_or_default();
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for r in g.roles.iter().filter(|r| r.color != 0) {
        (r.id, r.color, r.position, gradient(r.id)).hash(&mut h);
    }
    let badge = |role_id: u64| {
        let x = store.roles_admin.extra(g.id, role_id)?;
        x.icon.map(|hash| RoleBadge::Image(crate::state::model::ImageRef::RoleIcon { role_id, hash })).or_else(|| x.unicode_emoji.as_deref().and_then(crate::state::emoji::find).map(RoleBadge::Emoji))
    };
    g.owner_id.hash(&mut h);
    for r in &g.roles {
        if let Some(x) = store.roles_admin.extra(g.id, r.id).filter(|x| x.icon.is_some() || x.unicode_emoji.is_some()) {
            (r.id, r.position, x.icon, &x.unicode_emoji).hash(&mut h);
        }
    }
    let print = h.finish();
    if table(ctx).is_some_and(|t| t.guild == g.id && t.print == print) {
        return;
    }
    let roles = g
        .roles
        .iter()
        .filter(|r| r.color != 0)
        .map(|r| {
            let (secondary, tertiary) = gradient(r.id);
            (r.id, (r.position, NameColors { primary: r.color, secondary, tertiary: secondary.and(tertiary) }))
        })
        .collect();
    let icons = g.roles.iter().filter_map(|r| Some((r.id, (r.position, badge(r.id)?)))).collect();
    ctx.data_mut(|d| d.insert_temp(roles_id(), RoleTable { guild: g.id, print, roles: Arc::new(roles), owner: g.owner_id, icons: Arc::new(icons) }));
}

/// `user_id` est-il le propriétaire du serveur affiché ?
pub(super) fn is_owner(ctx: &egui::Context, user_id: u64) -> bool {
    table(ctx).is_some_and(|t| t.owner == Some(user_id))
}

/// Icône du rôle le plus haut parmi `roles` qui en a une (serveur affiché).
pub(super) fn role_badge(ctx: &egui::Context, roles: &[u64]) -> Option<RoleBadge> {
    let table = table(ctx)?;
    if table.icons.is_empty() {
        return None;
    }
    roles.iter().filter_map(|id| table.icons.get(id).map(|(pos, b)| ((*pos, std::cmp::Reverse(*id)), *b))).max_by_key(|(k, _)| *k).map(|(_, b)| b)
}

fn table(ctx: &egui::Context) -> Option<RoleTable> {
    ctx.data(|d| d.get_temp::<RoleTable>(roles_id()))
}

/// Serveur affiché (celui de la table des rôles), s'il y en a un.
pub(super) fn shown_guild(ctx: &egui::Context) -> Option<u64> {
    table(ctx).map(|t| t.guild)
}

/// Couleurs du rôle coloré le plus haut parmi `roles` (même règle que
/// `state::members::top_color`) ; sans couleur si le serveur n'est pas celui
/// affiché ou qu'aucun rôle n'est coloré.
pub(super) fn role_colors(ctx: &egui::Context, guild_id: u64, roles: &[u64]) -> NameColors {
    let Some(table) = table(ctx).filter(|t| t.guild == guild_id) else { return NameColors::default() };
    roles.iter().filter_map(|id| table.roles.get(id).map(|(pos, c)| ((*pos, std::cmp::Reverse(*id)), *c))).max_by_key(|(k, _)| *k).map(|(_, c)| c).unwrap_or_default()
}

/// Ce qu'une personne porte, lu une fois sous le verrou des tables.
#[derive(Clone, Copy, Default)]
pub(super) struct Worn {
    pub(super) look: Look,
    /// Couleurs du rôle le plus haut (sur le serveur affiché) ; `primary` à 0 : aucune.
    pub(super) role: NameColors,
    /// Serveur où ces parures ont été lues.
    guild: Option<u64>,
    /// Avatar propre au serveur.
    pub(super) member_avatar: Option<ImageHash>,
    /// Robot ou application (étiquette « APP » après son nom).
    pub(super) bot: bool,
    /// Décoration propre au serveur, sinon celle du compte.
    pub(super) decoration: Option<Asset>,
}

impl Worn {
    /// Rôles connus par ailleurs (ligne de la liste des membres du serveur affiché).
    pub(super) fn with_roles(mut self, ctx: &egui::Context, roles: &[u64]) -> Worn {
        if let Some(g) = shown_guild(ctx) {
            self.role = role_colors(ctx, g, roles);
        }
        self
    }

    /// Avatar à dessiner : celui du serveur s'il y en a un, sinon `base`.
    pub(super) fn avatar(&self, user_id: u64, base: &Avatar) -> Avatar {
        match (self.guild, self.member_avatar) {
            (Some(guild_id), Some(hash)) => Avatar { image: Some(ImageRef::MemberAvatar { guild_id, user_id, hash }), ..*base },
            _ => *base,
        }
    }

    /// Couleurs du nom : celles du rôle, sinon celles du style du nom ;
    /// aucune si le réglage « couleurs de rôle » les coupe.
    pub(super) fn name_colors(&self, ctx: &egui::Context) -> NameColors {
        if super::settings::local::display(ctx).role_colors_off {
            return NameColors::default();
        }
        if self.role.primary != 0 {
            return self.role;
        }
        self.look.name_style().map(|s| s.paint()).unwrap_or_default()
    }

    /// Couleurs du rôle seul ; aucune si le réglage « couleurs de rôle » les coupe.
    fn role_colors(&self, ctx: &egui::Context) -> NameColors {
        if super::settings::local::display(ctx).role_colors_off { NameColors::default() } else { self.role }
    }

    /// Couleur d'un nom d'auteur sans rôle coloré, au repos : dans un message
    /// privé, la couleur de texte forte (comme dans le client) ; sur un
    /// serveur, la teinte stable de l'auteur.
    fn plain_name(&self, t: &Tokens, user_id: u64) -> Color32 {
        if self.guild.is_none() { t.colors.text_strong } else { t.author_color(user_id) }
    }
}

/// Parures de `user_id` (sur `guild_id` s'il y en a un). Ne compte pas comme
/// « affiché » : un changement de parure ne réveille pas l'interface.
pub(super) fn worn(ctx: &egui::Context, guild_id: Option<u64>, user_id: u64) -> Worn {
    let Some(hub) = super::profile::hub(ctx) else { return Worn::default() };
    let people = hub.lock();
    let look = people.user(user_id).map(|u| u.look).unwrap_or_default();
    let member = guild_id.and_then(|g| people.member(g, user_id));
    let roles: Option<&[u64]> = member.and_then(|m| m.roles.as_deref());
    let role = match (guild_id, roles) {
        (Some(g), Some(r)) => role_colors(ctx, g, r),
        _ => NameColors::default(),
    };
    let bot = people.user(user_id).is_some_and(|u| u.bot);
    Worn { look, role, guild: guild_id, bot, member_avatar: member.and_then(|m| m.avatar), decoration: member.and_then(|m| m.decoration).or(look.decoration()) }
}

/// Parures de l'auteur d'un message du salon affiché.
pub(super) fn author(ctx: &egui::Context, user_id: u64) -> Worn {
    worn(ctx, shown_guild(ctx), user_id)
}

/// Nom de l'auteur d'un message, comme dans le client : toujours dans la
/// police de son style de nom ; à la couleur (ou au dégradé) de son rôle s'il
/// en a un ; sinon au repos d'une couleur unie ([`Worn::plain_name`]), les
/// couleurs et l'effet du style ne se montrant que tant que le message est
/// survolé (`hovered`). Suivi de son tag de serveur ; rend la zone du nom.
pub(super) fn author_name(ui: &mut egui::Ui, t: &Tokens, images: Option<&Images>, name: &str, user_id: u64, worn: &Worn, hovered: bool) -> Rect {
    let size = egui::TextStyle::Body.resolve(ui.style()).size;
    let style = worn.look.name_style();
    let font = name_font(ui.ctx(), size, style);
    let role = worn.role_colors(ui.ctx());
    let colors = worn.name_colors(ui.ctx());
    let rest = worn.plain_name(t, user_id);
    // Réglage « point de couleur » (lot PERSO) : le rôle colore un point, le nom reste en couleur de texte.
    let rect = if colors.primary != 0 && super::settings::local::display(ui.ctx()).name_dot {
        let side = (font.size * 0.5).round();
        let (dot, _) = ui.allocate_exact_size(egui::vec2(side, font.size), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), side / 2.0, legible(rgb(colors.primary), t.colors.bg2, MIN_MUTED));
        super::emoji::label_job(ui, name_job(name, font, NameColors::default(), t.colors.bg2, t.colors.text_strong), false).rect.union(dot)
    } else if role.primary != 0 {
        super::emoji::label_job(ui, name_job(name, font, role, t.colors.bg2, rest), false).rect
    } else if hovered && colors.primary != 0 {
        // Même police, même corps : le nom garde sa largeur, seul son habit change.
        styled_name(ui, name, size, style, t.colors.bg2, rest).rect
    } else {
        super::emoji::label_job(ui, name_job(name, font, NameColors::default(), t.colors.bg2, rest), false).rect
    };
    if let (Some(tag), Some(images)) = (worn.look.tag(), images) {
        tag_ui(ui, images, t, &tag);
    }
    rect
}

/// Étiquette « APP » après le nom d'un robot ou d'une application, comme dans le client.
pub(super) fn app_tag(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang) {
    let galley = ui.painter().layout_no_wrap(crate::i18n::t::APP_TAG.get(lang).to_owned(), super::fonts::demi_font((t.typography.small - 1.0).max(8.0)), t.colors.on_accent);
    let (rect, _) = ui.allocate_exact_size(galley.size() + egui::vec2(8.0, 2.0), Sense::hover());
    ui.painter().rect_filled(rect, 4.0, t.colors.accent);
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.colors.on_accent);
}

/// Couleur unie du nom de l'auteur (mode compact : une seule couleur par nom) :
/// celle de son rôle, sinon celle du repos.
pub(super) fn author_color(ctx: &egui::Context, t: &Tokens, user_id: u64, worn: &Worn) -> Color32 {
    match worn.role_colors(ctx).primary {
        0 => worn.plain_name(t, user_id),
        c => legible(rgb(c), t.colors.bg2, MIN_MUTED),
    }
}

pub(super) fn rgb(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

pub(super) fn mix(a: Color32, b: Color32, k: f32) -> Color32 {
    let k = k.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * k).round() as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

/// `color` éclaircie (fond sombre) ou foncée (fond clair) par pas, jusqu'à
/// atteindre le contraste `min` sur `bg` : la teinte reste reconnaissable.
pub(super) fn legible(color: Color32, bg: Color32, min: f64) -> Color32 {
    if contrast_ratio(color, bg) >= min {
        return color;
    }
    let toward = if contrast_ratio(Color32::WHITE, bg) >= contrast_ratio(Color32::BLACK, bg) { Color32::WHITE } else { Color32::BLACK };
    (1..=10).map(|i| mix(color, toward, i as f32 / 10.0)).find(|c| contrast_ratio(*c, bg) >= min).unwrap_or(toward)
}

/// Texte `text` aux couleurs `colors` (dégradé de gauche à droite, un arrêt
/// par caractère), lisible sur `bg` ; sans couleur : `fallback`.
pub(super) fn name_job(text: &str, font: FontId, colors: NameColors, bg: Color32, fallback: Color32) -> LayoutJob {
    let (stops, n) = colors.stops();
    let count = text.chars().count();
    if colors.primary == 0 || n == 1 || count < 2 {
        let color = if colors.primary == 0 { fallback } else { legible(rgb(colors.primary), bg, MIN_MUTED) };
        return LayoutJob::simple_singleline(text.to_string(), font, color);
    }
    let stops: Vec<Color32> = stops[..n].iter().map(|c| legible(rgb(*c), bg, MIN_MUTED)).collect();
    let mut job = LayoutJob::default();
    let mut buf = [0u8; 4];
    for (i, ch) in text.chars().enumerate() {
        // Position dans le dégradé, puis entre deux arrêts voisins.
        let at = i as f32 / (count - 1) as f32 * (n - 1) as f32;
        let k = (at.floor() as usize).min(n - 2);
        let color = mix(stops[k], stops[k + 1], at - k as f32);
        job.append(ch.encode_utf8(&mut buf), 0.0, TextFormat { font_id: font.clone(), color, ..Default::default() });
    }
    job
}

/// Décor d'un effet de nom derrière le texte : copies décalées du texte,
/// chacune de sa couleur (halo, contour ou ombre). Rien ne bouge.
fn name_backdrop(effect: NameEffect, color: Color32, size: f32) -> Vec<(egui::Vec2, Color32)> {
    let ring = |r: f32, n: usize, c: Color32| (0..n).map(move |i| (egui::Vec2::angled(std::f32::consts::TAU * i as f32 / n as f32) * r, c));
    let dark = mix(color, Color32::BLACK, 0.7);
    match effect {
        // Lueur : deux couronnes translucides de la couleur du nom.
        NameEffect::Neon | NameEffect::Glow => ring(size * 0.11, 12, color.gamma_multiply(0.10)).chain(ring(size * 0.05, 8, color.gamma_multiply(0.22))).collect(),
        // Dessin animé : contour sombre.
        NameEffect::Toon => ring((size * 0.055).max(1.0), 8, dark).collect(),
        // Pop : ombre portée nette.
        NameEffect::Pop => vec![(egui::Vec2::splat((size * 0.08).max(1.0)), dark)],
        _ => Vec::new(),
    }
}

/// Nom au style de nom `style` (`display_name_styles`, docs.discord.food,
/// « Display Name Style Structure »), prêt à mettre en page : couleurs (uni
/// ou dégradé), police du style (`fonts::styled` : embarquée, chargée à la
/// première demande, ou la plus proche de Windows pour celles sans fichier),
/// et le décor de son effet, à peindre derrière le texte (lueur du néon et
/// du halo, contour du « toon », ombre du « pop » ; voir
/// [`backdrop_shapes`]). Seul rendu du style de nom : la carte, le panneau
/// utilisateur et l'éditeur de profil passent tous par ici. `size` : corps
/// du nom ; `bg` : fond sur lequel il doit rester lisible.
pub(super) fn styled_job(ctx: &egui::Context, text: &str, size: f32, style: Option<NameStyle>, bg: Color32, fallback: Color32) -> (LayoutJob, Vec<(egui::Vec2, Color32)>) {
    let colors = style.map(|s| s.paint()).unwrap_or_default();
    let mut job = name_job(text, name_font(ctx, size, style), colors, bg, fallback);
    let effect = style.map_or(NameEffect::Solid, |s| s.effect);
    let backdrop = if colors.primary == 0 { Vec::new() } else { name_backdrop(effect, legible(rgb(colors.primary), bg, MIN_MUTED), size) };
    if effect == NameEffect::Neon && !backdrop.is_empty() {
        // Néon : le tube est presque blanc, la couleur est dans la lueur.
        for s in &mut job.sections {
            s.format.color = mix(s.format.color, Color32::WHITE, 0.6);
        }
    }
    (job, backdrop)
}

/// Police d'un nom au style `style`, de corps `size` (`fonts::styled`) ;
/// sans style, la demi-grasse de l'interface.
pub(super) fn name_font(ctx: &egui::Context, size: f32, style: Option<NameStyle>) -> FontId {
    FontId::new(size, style.map_or_else(super::fonts::demi, |s| super::fonts::styled(ctx, s.font_id)))
}

/// Formes du décor d'un nom stylé dont la galée est posée en `pos`.
pub(super) fn backdrop_shapes(pos: Pos2, galley: &std::sync::Arc<egui::Galley>, backdrop: Vec<(egui::Vec2, Color32)>) -> Vec<egui::Shape> {
    backdrop.into_iter().map(|(offset, color)| egui::epaint::TextShape::new(pos + offset, galley.clone(), color).with_override_text_color(color).into()).collect()
}

/// Nom affiché au style de nom du profil (voir [`styled_job`]).
pub(super) fn styled_name(ui: &mut egui::Ui, text: &str, size: f32, style: Option<NameStyle>, bg: Color32, fallback: Color32) -> egui::Response {
    let (mut job, backdrop) = styled_job(ui.ctx(), text, size, style, bg, fallback);
    if backdrop.is_empty() {
        return super::emoji::label_job(ui, job, false);
    }
    job.wrap.max_width = ui.available_width();
    let (job, spots) = super::emoji::imaged(ui, job);
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    let (rect, resp) = ui.allocate_exact_size(galley.size(), Sense::hover());
    if ui.is_rect_visible(rect.expand(size * 0.2)) {
        let p = ui.painter();
        p.extend(backdrop_shapes(rect.min, &galley, backdrop));
        super::emoji::paint_spots(ui, rect.min, &galley, &spots);
        p.galley(rect.min, galley, fallback);
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, text));
    resp
}

/// Nom stylé sur une seule ligne, coupé à `width` (points de suspension) :
/// la galée et son décor (voir [`styled_job`]), à peindre par
/// [`paint_styled`]. Pour une ligne dessinée au peintre (panneau
/// utilisateur, tuiles et aperçu de l'éditeur de profil).
pub(super) fn styled_line(ui: &egui::Ui, text: &str, size: f32, style: Option<NameStyle>, bg: Color32, fallback: Color32, width: f32) -> (std::sync::Arc<egui::Galley>, Vec<(egui::Vec2, Color32)>) {
    let (mut job, backdrop) = styled_job(ui.ctx(), text, size, style, bg, fallback);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    (ui.fonts_mut(|f| f.layout_job(job)), backdrop)
}

/// Peint une ligne de [`styled_line`] à `pos` : le décor, puis le texte.
pub(super) fn paint_styled(p: &egui::Painter, pos: Pos2, galley: std::sync::Arc<egui::Galley>, backdrop: Vec<(egui::Vec2, Color32)>, fallback: Color32) {
    p.extend(backdrop_shapes(pos, &galley, backdrop));
    p.galley(pos, galley, fallback);
}

/// Planche des polices de nom (scène `polices-nom` de l'aperçu) : chaque
/// police de la table `fonts::NAMES` sur un nom, uni puis avec un effet, et
/// sur un nom mêlant latin, emoji, cyrillique et chinois (repli des glyphes).
/// Première ligne : la demi-grasse de l'interface, pour comparer les graisses.
pub(super) fn fonts_board(ui: &mut egui::Ui, t: &Tokens) {
    const MIXED: &str = "Zoé 🔥 Алиса ★ 你好";
    const EFFECTS: [NameEffect; 6] = [NameEffect::Solid, NameEffect::Gradient, NameEffect::Neon, NameEffect::Toon, NameEffect::Pop, NameEffect::Glow];
    let size = (t.typography.title * 1.2).round();
    let (bg, strong) = (t.colors.bg2, t.colors.text_strong);
    let small = |text: String| egui::RichText::new(text).size(t.typography.small).color(t.colors.muted);
    // Au peintre, sur une ligne : les emojis viennent des polices de repli, comme dans le
    // panneau utilisateur (pas d'image d'emoji, absente hors ligne).
    let line = |ui: &mut egui::Ui, text: &str, style: Option<NameStyle>| {
        let (galley, backdrop) = styled_line(ui, text, size, style, bg, strong, 420.0);
        let (rect, _) = ui.allocate_exact_size(galley.size(), Sense::hover());
        paint_styled(ui.painter(), rect.min, galley, backdrop, strong);
    };
    egui::ScrollArea::vertical().show(ui, |ui| {
        egui::Grid::new("planche_polices_nom").num_columns(5).spacing(egui::vec2(2.0 * t.spacing.item_x, t.spacing.item_y)).show(ui, |ui| {
            ui.label(small("—".into()));
            ui.label(small("Figtree 600".into()));
            line(ui, "Alice Martin", None);
            line(ui, "Alice Martin", None);
            line(ui, MIXED, None);
            ui.end_row();
            for (i, f) in super::fonts::NAMES.iter().enumerate() {
                ui.label(small(format!("{} · {}", f.id, f.label)));
                ui.label(small(if f.embedded() || f.id == super::fonts::DEFAULT_NAME_FONT { f.font.to_owned() } else { format!("{} (Windows)", f.font) }));
                line(ui, "Alice Martin", NameStyle::new(f.id, NameEffect::Solid, &[0xf2f3f5]));
                line(ui, "Alice Martin", NameStyle::new(f.id, EFFECTS[i % EFFECTS.len()], &[0xff7a59, 0x8a5cf0]));
                line(ui, MIXED, NameStyle::new(f.id, NameEffect::Solid, &[0x5bc0eb]));
                ui.end_row();
            }
        });
    });
}

/// Y a-t-il un dégradé à peindre (sinon la couleur unie suffit) ?
pub(super) fn is_gradient(colors: NameColors) -> bool {
    colors.primary != 0 && colors.secondary.is_some()
}

/// Rectangle aux coins arrondis rempli par `color` (fonction du point) :
/// éventail de triangles autour du centre. Exact pour un dégradé linéaire.
pub(super) fn gradient_rect(rect: Rect, r: CornerRadius, color: impl Fn(Pos2) -> Color32) -> egui::Shape {
    const ARC: usize = 6;
    let mut mesh = egui::Mesh::default();
    let center = rect.center();
    mesh.colored_vertex(center, color(center));
    // Coins dans le sens horaire, en partant du haut gauche.
    let corners = [(rect.left_top(), f32::from(r.nw), 180.0f32, 1.0, 1.0), (rect.right_top(), f32::from(r.ne), 270.0, -1.0, 1.0), (rect.right_bottom(), f32::from(r.se), 0.0, -1.0, -1.0), (rect.left_bottom(), f32::from(r.sw), 90.0, 1.0, -1.0)];
    for (corner, radius, start, dx, dy) in corners {
        let radius = radius.min(rect.width() / 2.0).min(rect.height() / 2.0);
        let c = corner + egui::vec2(dx * radius, dy * radius);
        for i in 0..=ARC {
            let a = (start + 90.0 * i as f32 / ARC as f32).to_radians();
            let p = c + radius * egui::vec2(a.cos(), a.sin());
            mesh.colored_vertex(p, color(p));
        }
    }
    let n = mesh.vertices.len() as u32 - 1;
    for i in 0..n {
        mesh.add_triangle(0, 1 + i, 1 + (i + 1) % n);
    }
    egui::Shape::mesh(mesh)
}

/// Les animations d'un profil ouvert jouent-elles ? Réglage « lire les GIF »
/// (et « réduire les animations » : seulement sous le pointeur, ici le profil).
pub(super) fn profile_play(ui: &egui::Ui) -> bool {
    super::motion::get(ui.ctx()).gifs.now(ui.rect_contains_pointer(ui.max_rect()))
}

/// Décoration autour de l'avatar `avatar` : image fixe, animée si `play`.
pub(super) fn paint_decoration(ui: &egui::Ui, images: &Images, avatar: Rect, asset: Asset, play: bool) {
    let rect = Rect::from_center_size(avatar.center(), avatar.size() * DECORATION_SCALE);
    if !ui.is_rect_visible(rect) {
        return;
    }
    if let Image::Ready(tex) = images.get_moving_still_first(ImageRef::Decoration { asset }, rect.width(), play) {
        ui.painter().add(images::textured(rect, CornerRadius::ZERO, tex));
    }
}

/// Décoration de `user_id` sur l'avatar d'une ligne (liste des membres,
/// message, MP) : toujours fixe.
pub(super) fn row_decoration(ui: &egui::Ui, images: &Images, avatar: Rect, worn: &Worn) {
    if let Some(asset) = worn.decoration {
        paint_decoration(ui, images, avatar, asset, false);
    }
}

/// Fond d'une plaque derrière la ligne `row` posée sur `bg` : voile de la
/// teinte de la palette, plus dense vers la droite, puis l'image de la
/// plaque : fixe au repos, animée tant que le pointeur est sur la ligne
/// ([`plate_plays`]). Rend le fond moyen sous le texte (pour le contraste
/// du nom).
pub(super) fn paint_nameplate(ui: &egui::Ui, images: &Images, t: &Tokens, row: Rect, plate: Nameplate, bg: Color32) -> Color32 {
    let under = paint_nameplate_in(ui, images, t, row, t.r_row(), plate, bg);
    if !ui.is_rect_visible(row) {
        return under;
    }
    // Voile du fond de la ligne sous le texte, comme le client sur ses plaques : dense à
    // gauche (nom, sous-titre), nul vers la droite où l'image reste entière.
    let k = |p: Pos2| (1.0 - (p.x - row.left()) / (row.width() * PLATE_VEIL_SPAN).max(1.0)).clamp(0.0, 1.0);
    ui.painter().add(gradient_rect(row, t.r_row(), |p| bg.gamma_multiply(PLATE_VEIL * k(p))));
    mix(under, bg, PLATE_VEIL * 0.6)
}

/// La plaque de la ligne `row` joue-t-elle ? Sous le pointeur seulement,
/// comme dans l'application officielle, et selon la même règle que les
/// avatars animés des listes (`util::paint_image`) : jamais tant que les
/// réglages d'animation ne sont pas posés. Hors survol, rien n'est demandé :
/// aucune image suivante, aucun redessin.
fn plate_plays(ui: &egui::Ui, row: Rect) -> bool {
    let hovered = super::motion::hovered(ui, row);
    super::motion::get(ui.ctx()).avatars.now(hovered)
}

/// Comme [`paint_nameplate`], aux coins `r` (carte du panneau utilisateur).
pub(super) fn paint_nameplate_in(ui: &egui::Ui, images: &Images, t: &Tokens, row: Rect, r: CornerRadius, plate: Nameplate, bg: Color32) -> Color32 {
    if !ui.is_rect_visible(row) {
        return bg;
    }
    let tint = plate.palette.map_or(t.colors.accent, |p| rgb(p.tint()));
    let (from, to) = (mix(bg, tint, 0.12), mix(bg, tint, 0.5));
    ui.painter().add(gradient_rect(row, r, |p| mix(from, to, (p.x - row.left()) / row.width().max(1.0))));
    if let Image::Ready(tex) = images.get_fill_hover(ImageRef::Nameplate { asset: plate.asset }, row.width(), row.height(), NAMEPLATE_ASPECT, plate_plays(ui, row)) {
        ui.painter().add(images::textured(row, r, tex));
    }
    mix(from, to, 0.35)
}

/// Parures d'une ligne (liste des membres, MP) : plaque derrière la ligne,
/// décoration fixe sur l'avatar, nom coloré, tag de serveur.
pub(super) struct RowLook {
    worn: Worn,
    /// Fond sous le texte (celui de la ligne, ou de la plaque).
    bg: Color32,
}

/// À appeler après le fond de survol de la ligne `row` (posée sur `bg`) et
/// avant son contenu : peint la plaque. `roles` : ceux du membre, s'ils sont
/// connus de l'appelant.
pub(super) fn row_look(ui: &egui::Ui, images: &Images, t: &Tokens, row: Rect, user_id: u64, roles: &[u64], bg: Color32) -> RowLook {
    let mut worn = worn(ui.ctx(), shown_guild(ui.ctx()), user_id);
    if !roles.is_empty() {
        worn = worn.with_roles(ui.ctx(), roles);
    }
    let bg = worn.look.nameplate().map_or(bg, |plate| paint_nameplate(ui, images, t, row, plate, bg));
    RowLook { worn, bg }
}

impl RowLook {
    pub(super) fn decoration(&self, ui: &egui::Ui, images: &Images, avatar: Rect) {
        row_decoration(ui, images, avatar, &self.worn);
    }

    /// Nom de la ligne, comme dans le client : dans la police de son style de
    /// nom, mais sans les couleurs ni l'effet de ce style (réservés à la
    /// discussion et aux profils) ; en dégradé si son rôle en a un, sinon
    /// `color` (déjà choisie par la ligne). `plain` : ligne estompée (hors
    /// ligne), sans couleur propre.
    pub(super) fn name(&self, ui: &egui::Ui, name: &str, font: FontId, color: Color32, plain: bool) -> egui::WidgetText {
        // Police du style, sauf celle de l'interface (la ligne garde alors sa graisse).
        let font = match self.worn.look.name_style() {
            Some(s) if s.font_id != super::fonts::DEFAULT_NAME_FONT => FontId::new(font.size, super::fonts::styled(ui.ctx(), s.font_id)),
            _ => font,
        };
        let colors = self.worn.role_colors(ui.ctx());
        if !plain && is_gradient(colors) {
            return name_job(name, font, colors, self.bg, color).into();
        }
        egui::RichText::new(name).font(font).color(color).into()
    }

    /// Style du nom (police, effet, couleurs) et fond sur lequel il est posé.
    pub(super) fn name_style(&self) -> (Option<NameStyle>, Color32) {
        (self.worn.look.name_style(), self.bg)
    }

    /// La ligne porte une plaque de nom (son image passe sous les textes).
    pub(super) fn plated(&self) -> bool {
        self.worn.look.nameplate().is_some()
    }

    pub(super) fn tag(&self, ui: &egui::Ui, t: &Tokens) -> Option<TagChip> {
        self.worn.look.tag().map(|tag| TagChip::new(ui, t, &tag, t.colors.text))
    }
}

/// Parures du correspondant du MP à deux `channel_id`, pour sa ligne dans la
/// liste des MP ; `None` pour un groupe ou un correspondant inconnu.
pub(super) fn dm_look(ui: &egui::Ui, images: &Images, t: &Tokens, row: Rect, channel_id: u64, bg: Color32) -> Option<RowLook> {
    let peer = super::profile::hub(ui.ctx())?.lock().dm_peer(channel_id)?;
    Some(row_look(ui, images, t, row, peer, &[], bg))
}

/// Étiquette d'un tag de serveur : badge puis texte, mesurée puis peinte.
pub(super) struct TagChip {
    galley: Arc<egui::Galley>,
    badge: Option<ImageRef>,
    side: f32,
    pad: f32,
    pub(super) size: egui::Vec2,
}

impl TagChip {
    pub(super) fn new(ui: &egui::Ui, t: &Tokens, tag: &GuildTag, color: Color32) -> TagChip {
        let font = FontId::new(t.typography.small, crate::ui::fonts::demi());
        let galley = ui.painter().layout_no_wrap(tag.text().to_string(), font, color);
        let side = galley.size().y.round();
        let pad = (t.spacing.item_x / 2.0).round();
        let badge = tag.badge.map(|hash| ImageRef::TagBadge { guild_id: tag.guild_id, hash });
        let badge_w = if badge.is_some() { side + pad / 2.0 } else { 0.0 };
        let size = egui::vec2(pad + badge_w + galley.size().x + pad, side + 2.0);
        TagChip { galley, badge, side, pad, size }
    }

    /// Peint l'étiquette dans `rect` (de taille `self.size`) sur le fond `fill`.
    pub(super) fn paint(self, ui: &egui::Ui, images: &Images, t: &Tokens, rect: Rect, fill: Color32) {
        let p = ui.painter();
        p.rect_filled(rect, t.r_row(), fill);
        let mut x = rect.left() + self.pad;
        if let Some(badge) = self.badge {
            let slot = super::util::pixel_square(ui, egui::pos2(x, rect.center().y - self.side / 2.0), self.side);
            if let Image::Ready(tex) = images.get(badge, self.side) {
                p.add(images::textured(slot, CornerRadius::ZERO, tex));
            }
            x += self.side + self.pad / 2.0;
        }
        let y = rect.center().y - self.galley.size().y / 2.0;
        p.galley(egui::pos2(x, y), self.galley, Color32::PLACEHOLDER);
    }
}

/// Tag de serveur posé dans le flux (à côté d'un nom).
pub(super) fn tag_ui(ui: &mut egui::Ui, images: &Images, t: &Tokens, tag: &GuildTag) {
    let chip = TagChip::new(ui, t, tag, t.colors.text);
    let (rect, resp) = ui.allocate_exact_size(chip.size, Sense::hover());
    chip.paint(ui, images, t, rect, t.colors.bg3);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, tag.text()));
}

/// Couleurs de thème d'un profil : dégradé du fond et jetons lisibles dessus.
pub(super) struct Themed {
    pub(super) tokens: Tokens,
    top: Color32,
    bottom: Color32,
    /// Hauteur sur laquelle le dégradé passe d'une couleur à l'autre.
    span: f32,
    /// Couleurs choisies, sans voile : le liseré de la carte.
    raw: [Color32; 2],
}

/// Voile posé par le client sur le dégradé d'un profil à thème : noir à 60 %
/// (thème sombre), blanc à 60 % (thème clair). Seul le liseré garde les
/// couleurs choisies. Constaté sur le client (capture du 19 novembre 2026).
const THEME_VEIL: f32 = 0.6;
/// Épaisseur du liseré aux couleurs du thème.
const THEME_EDGE: f32 = 2.0;

impl Themed {
    /// `colors` : principale (haut) et secondaire (bas). Le texte est clair
    /// ou sombre selon ce qui se lit le mieux sur les deux ; si aucun
    /// n'atteint le contraste voulu, les deux couleurs sont foncées (ou
    /// éclaircies) jusqu'à ce qu'il le soit.
    pub(super) fn new(t: &Tokens, colors: [u32; 2]) -> Themed {
        /// Contraste du texte fort : laisse de la marge au texte atténué.
        const STRONG: f64 = 7.0;
        let (mut top, mut bottom) = (rgb(colors[0]), rgb(colors[1]));
        let worst = |fg: Color32, a: Color32, b: Color32| contrast_ratio(fg, a).min(contrast_ratio(fg, b));
        let (light, dark) = (Color32::WHITE, Color32::from_rgb(6, 6, 7));
        let (text, away) = if worst(light, top, bottom) >= worst(dark, top, bottom) { (light, Color32::BLACK) } else { (dark, Color32::WHITE) };
        let raw = [top, bottom];
        let (a, b) = (mix(top, away, THEME_VEIL), mix(bottom, away, THEME_VEIL));
        for i in 0..=20 {
            let k = i as f32 / 20.0;
            (top, bottom) = (mix(a, away, k), mix(b, away, k));
            if worst(text, top, bottom) >= STRONG {
                break;
            }
        }
        let mid = mix(top, bottom, 0.5);
        let on_both = |c: Color32| {
            let c = legible(c, top, MIN_TEXT);
            legible(c, bottom, MIN_TEXT)
        };
        // Texte atténué : le plus fondu dans le fond qui garde 4,5:1.
        let muted = (0..=8).rev().map(|i| mix(text, mid, i as f32 * 0.05)).find(|c| worst(*c, top, bottom) >= MIN_TEXT).unwrap_or(text);
        let mut tokens = t.clone();
        let c = &mut tokens.colors;
        c.text_strong = text;
        c.text = mix(text, mid, 0.06);
        c.muted = muted;
        c.faint = muted;
        c.link = on_both(c.link);
        c.danger = on_both(c.danger);
        c.online = on_both(c.online);
        c.idle = on_both(c.idle);
        c.dnd = on_both(c.dnd);
        c.offline = on_both(c.offline);
        c.bg1 = top;
        c.bg3 = mix(mid, text, 0.14);
        c.row_hover = mix(mid, text, 0.1);
        c.border = mix(mid, text, 0.28);
        c.separator = c.border;
        tokens.markdown.code_bg = c.bg3;
        // Mention dans la bio : lisible sur le dégradé du profil, pas seulement sur le fond du thème.
        tokens.markdown.mention_text = on_both(tokens.markdown.mention_text);
        Themed { tokens, top, bottom, span: THEME_SPAN, raw }
    }

    /// Le dégradé couvre une fenêtre haute de `height` (celle de la carte) :
    /// la couleur du bas n'est atteinte qu'à son pied.
    pub(super) fn set_span(&mut self, height: f32) {
        if height.is_finite() && height >= 1.0 {
            self.span = height.max(MIN_THEME_SPAN);
        }
    }

    /// Filet de contour de la carte : aucun, le liseré aux couleurs du
    /// thème fait partie du fond ([`Themed::background`]).
    pub(super) fn edge(&self, _t: &Tokens) -> egui::Stroke {
        egui::Stroke::NONE
    }

    /// Couleur du fond à la hauteur `y` d'une fenêtre dont le haut est `top`.
    pub(super) fn at(&self, top: f32, y: f32) -> Color32 {
        mix(self.top, self.bottom, (y - top) / self.span)
    }

    /// Fond de la fenêtre `rect`, à mettre à la place réservée avant le contenu.
    pub(super) fn background(&self, rect: Rect, r: CornerRadius) -> egui::Shape {
        // Liseré : le dégradé des couleurs choisies sur toute la fenêtre,
        // recouvert par le fond voilé, en retrait de `THEME_EDGE`.
        let top = rect.top();
        let k = |y: f32| ((y - top) / self.span).clamp(0.0, 1.0);
        let inner = rect.shrink(THEME_EDGE);
        let inner_r = r - THEME_EDGE as u8;
        egui::Shape::Vec(vec![gradient_rect(rect, r, |p| mix(self.raw[0], self.raw[1], k(p.y))), self.veiled(inner, inner_r, top)])
    }

    /// Fond voilé de `rect`, le dégradé partant de `top`.
    fn veiled(&self, rect: Rect, r: CornerRadius, top: f32) -> egui::Shape {
        // Le dégradé s'arrête à `span` : au-delà, la couleur du bas.
        let span = top + self.span;
        if rect.bottom() <= span {
            return gradient_rect(rect, r, |p| self.at(top, p.y));
        }
        let upper = Rect::from_min_max(rect.min, egui::pos2(rect.right(), span));
        let lower = Rect::from_min_max(egui::pos2(rect.left(), span), rect.max);
        egui::Shape::Vec(vec![
            gradient_rect(upper, CornerRadius { sw: 0, se: 0, ..r }, |p| self.at(top, p.y)),
            gradient_rect(lower, CornerRadius { nw: 0, ne: 0, ..r }, |_| self.bottom),
        ])
    }

    /// Filets et contours d'egui dans cette fenêtre.
    pub(super) fn apply(&self, ui: &mut egui::Ui) {
        ui.visuals_mut().widgets.noninteractive.bg_stroke.color = self.tokens.colors.separator;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokens() -> Tokens {
        Tokens::default()
    }

    /// Un rôle sombre sur un thème sombre reste de sa teinte, éclaircie
    /// jusqu'au contraste voulu ; une couleur déjà lisible n'est pas touchée.
    #[test]
    fn legible_keeps_the_hue_and_reaches_the_ratio() {
        let bg = tokens().colors.bg1;
        let navy = Color32::from_rgb(0x10, 0x18, 0x60);
        let out = legible(navy, bg, MIN_MUTED);
        assert!(contrast_ratio(out, bg) >= MIN_MUTED);
        assert!(out.b() > out.r() && out.b() > out.g(), "toujours bleu : {out:?}");
        let fine = Color32::from_rgb(0xff, 0x7a, 0x59);
        assert_eq!(legible(fine, bg, MIN_MUTED), fine);
        // Fond clair : on fonce.
        let light = Color32::WHITE;
        let yellow = Color32::from_rgb(0xff, 0xf0, 0x60);
        let out = legible(yellow, light, MIN_TEXT);
        assert!(contrast_ratio(out, light) >= MIN_TEXT && out.r() < yellow.r());
    }

    /// Dégradé : un arrêt par caractère, de la première couleur à la dernière.
    #[test]
    fn gradient_job_runs_from_first_to_last_stop() {
        let bg = Color32::BLACK;
        let font = FontId::proportional(14.0);
        let two = NameColors { primary: 0xff0000, secondary: Some(0x0000ff), tertiary: None };
        let job = name_job("Alice", font.clone(), two, bg, Color32::WHITE);
        assert_eq!(job.sections.len(), 5);
        assert_eq!(job.sections[0].format.color, legible(rgb(0xff0000), bg, MIN_MUTED));
        assert_eq!(job.sections[4].format.color, legible(rgb(0x0000ff), bg, MIN_MUTED));
        assert_eq!(job.text, "Alice");
        // Holographique : le milieu est la deuxième couleur.
        let three = NameColors { primary: 0xa9c9ff, secondary: Some(0xffbcec), tertiary: Some(0xffc5a0) };
        let job = name_job("abcde", font.clone(), three, bg, Color32::WHITE);
        assert_eq!(job.sections[2].format.color, rgb(0xffbcec));
        // Uni, sans couleur, ou un seul caractère : une seule section.
        assert_eq!(name_job("Alice", font.clone(), NameColors::solid(0x00ff00), bg, Color32::WHITE).sections.len(), 1);
        let plain = name_job("Alice", font.clone(), NameColors::default(), bg, Color32::GRAY);
        assert_eq!((plain.sections.len(), plain.sections[0].format.color), (1, Color32::GRAY));
        assert_eq!(name_job("A", font, two, bg, Color32::WHITE).sections.len(), 1);
        assert!(is_gradient(two) && !is_gradient(NameColors::solid(1)) && !is_gradient(NameColors { primary: 0, secondary: Some(1), tertiary: None }));
    }

    /// Thème de profil : quelles que soient les deux couleurs, tout texte
    /// posé sur le dégradé atteint 4,5:1 sur ses deux extrémités.
    #[test]
    fn profile_theme_text_is_always_readable() {
        let t = tokens();
        for colors in [[0x5b_6eef, 0x2b_2b2b], [0xff_ffff, 0xff_ffff], [0x00_0000, 0x00_0000], [0xff_ff00, 0x00_00ff], [0x80_8080, 0x80_8080], [0xff_c0cb, 0x10_1010], [0x12_3456, 0xfe_dcba]] {
            let th = Themed::new(&t, colors);
            let c = &th.tokens.colors;
            for (name, fg) in [("fort", c.text_strong), ("texte", c.text), ("atténué", c.muted), ("discret", c.faint), ("lien", c.link), ("danger", c.danger), ("en ligne", c.online)] {
                for bg in [th.top, th.bottom, th.at(0.0, THEME_SPAN / 2.0)] {
                    assert!(contrast_ratio(fg, bg) >= MIN_TEXT - 0.01, "{colors:06x?} : {name} {fg:?} sur {bg:?} = {:.2}", contrast_ratio(fg, bg));
                }
            }
        }
        // Texte clair : le fond est le dégradé choisi sous un voile noir à
        // 60 %, comme dans le client ; le liseré garde les couleurs choisies.
        let th = Themed::new(&t, [0x1a_1a2e, 0x16_213e]);
        assert_eq!((th.top, th.bottom), (mix(rgb(0x1a_1a2e), Color32::BLACK, THEME_VEIL), mix(rgb(0x16_213e), Color32::BLACK, THEME_VEIL)));
        assert_eq!(th.raw, [rgb(0x1a_1a2e), rgb(0x16_213e)]);
        // Couleurs claires : voile blanc, texte sombre.
        let th = Themed::new(&t, [0xff_e0f0, 0xf0_f0ff]);
        assert!(contrast_ratio(th.tokens.colors.text_strong, Color32::WHITE) > 10.0 && contrast_ratio(th.top, Color32::WHITE) < contrast_ratio(rgb(0xff_e0f0), Color32::WHITE));
        assert_eq!(th.at(100.0, 100.0), th.top);
        assert_eq!(th.at(100.0, 100.0 + 2.0 * THEME_SPAN), th.bottom);
    }

    /// Fond arrondi : un éventail fermé, tous les sommets dans le rectangle.
    #[test]
    fn gradient_rect_stays_inside_and_is_closed() {
        let rect = Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(300.0, 120.0));
        let egui::Shape::Mesh(mesh) = gradient_rect(rect, CornerRadius::same(12), |p| if p.y < 80.0 { Color32::RED } else { Color32::BLUE }) else { panic!("maillage attendu") };
        assert_eq!(mesh.vertices.len(), 1 + 4 * 7);
        assert_eq!(mesh.indices.len(), 3 * 28);
        assert!(mesh.vertices.iter().all(|v| rect.expand(0.01).contains(v.pos)));
        // Les coins eux-mêmes sont rognés.
        assert!(mesh.vertices.iter().all(|v| v.pos.distance(rect.left_top()) > 3.0));
    }

    /// Table des rôles : le rôle coloré le plus haut, avec son dégradé (lu
    /// dans les compléments du lot A3) ; rien pour un autre serveur.
    #[test]
    fn role_colors_come_from_the_shown_guild() {
        use crate::state::admin::roles::{RoleExtra, RolesEvent};
        use crate::state::model::{Guild, Role, User};
        use crate::state::{Event, Permissions};
        let ctx = egui::Context::default();
        let mut store = Store::new();
        let role = |id: u64, color: u32, position: i32| Role { id, name: format!("r{id}"), color, position, permissions: Permissions(0), mentionable: false };
        let mut guild = Guild::new(9, "G".into(), Vec::new());
        guild.roles = vec![role(9, 0, 0), role(20, 0x3498db, 1), role(21, 0xff7a59, 3), role(22, 0, 5)];
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![guild], dms: Vec::new() });
        store.apply(Event::Roles(RolesEvent::Extras { guild_id: 9, roles: vec![(21, RoleExtra { secondary_color: Some(0x8a5cf0), ..Default::default() })], replace: false }));
        install(&ctx, &store);
        assert_eq!(role_colors(&ctx, 9, &[20, 21, 22]), NameColors::default(), "aucun serveur affiché");
        store.guild = Some(9);
        install(&ctx, &store);
        assert_eq!(role_colors(&ctx, 9, &[20, 21, 22]), NameColors { primary: 0xff7a59, secondary: Some(0x8a5cf0), tertiary: None });
        assert_eq!(role_colors(&ctx, 9, &[20]), NameColors::solid(0x3498db));
        assert_eq!(role_colors(&ctx, 9, &[22, 999]), NameColors::default());
        assert_eq!(role_colors(&ctx, 8, &[21]), NameColors::default(), "autre serveur");
        // Le dégradé retiré : la table suit.
        store.apply(Event::Roles(RolesEvent::Extras { guild_id: 9, roles: vec![(21, RoleExtra::default())], replace: false }));
        install(&ctx, &store);
        assert_eq!(role_colors(&ctx, 9, &[21]), NameColors::solid(0xff7a59));
    }
}
