//! Rendu du texte mis en forme d'un message ([`Doc`], analysé une fois par
//! `state::markdown`). Ici, seulement des couleurs et des polices du thème :
//! chaque bloc devient une mise en page egui, texte sélectionnable.
//!
//! Les liens et les spoilers masqués sont des passages dont on retrouve les
//! rectangles après la mise en page ([`Zones`]) : le message les rend
//! cliquables après avoir posé sa propre zone de clic (ils passent devant).

use crate::i18n::{t as tx, Lang};
use crate::state::markdown::{Block, BlockKind, Class, Doc, Marker, Mention, Span, SpanKind};
use crate::theme::Tokens;
use eframe::egui::{self, text::LayoutJob, Color32, FontFamily, FontId, Id};
use std::ops::Range;

/// Ce qu'un clic sur un passage déclenche.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Hot {
    /// Ouvrir l'adresse.
    Link(String),
    /// Révéler le spoiler (son numéro dans le message).
    Spoiler(u32),
    /// Mention d'un utilisateur (ou auteur en mode compact) : carte de profil
    /// au clic, menu de l'utilisateur au clic droit. `name` : nom vu.
    User { id: u64, name: String },
    /// « (modifié) » d'un message dont le journal garde des versions (lot E1) :
    /// un clic les déplie sous le message.
    Versions,
    /// Mention de salon : aller au salon (F3).
    Channel(u64),
    /// Mention de rôle : liste de ses membres (F3). `name` : nom vu.
    Role { id: u64, name: String },
    /// Bouton « copier » d'un bloc de code (V9) : son texte.
    Copy(String),
}

/// Marge autour de l'icône du bouton « copier » d'un bloc de code.
const COPY_PAD: f32 = 3.0;

/// Morceau posé en début de première ligne, cliquable s'il porte un `Hot`.
pub(super) type Lead = (String, Color32, Option<Hot>);

/// Rectangles cliquables d'un message, à l'écran.
pub(super) type Zones = Vec<(egui::Rect, Hot)>;

/// Spoilers déjà révélés d'un message (état d'interface, gardé par egui).
#[derive(Clone, Default)]
struct Revealed(Vec<u32>);

fn revealed_id(msg: u64) -> Id {
    Id::unique(("spoilers-révélés", msg))
}

fn revealed(ctx: &egui::Context, msg: u64) -> Revealed {
    ctx.data(|d| d.get_temp::<Revealed>(revealed_id(msg))).unwrap_or_default()
}

fn reveal(ctx: &egui::Context, msg: u64, n: u32) {
    ctx.data_mut(|d| {
        let r = d.get_temp_mut_or_default::<Revealed>(revealed_id(msg));
        if !r.0.contains(&n) {
            r.0.push(n);
        }
    });
}

/// Texte mis en forme d'un message, bloc par bloc. `lead` : morceaux posés
/// au début de la première ligne (mode compact : heure et auteur) ;
/// `edited` : « (modifié) » à la fin. Les rectangles cliquables sont
/// ajoutés à `zones`.
#[allow(clippy::too_many_arguments)]
pub(super) fn doc_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, msg: u64, doc: &Doc, lead: &[(String, Color32)], edited: bool, zones: &mut Zones) {
    let lead: Vec<Lead> = lead.iter().map(|(s, c)| (s.clone(), *c, None)).collect();
    doc_ui_lead(ui, t, lang, msg, doc, &lead, edited, zones);
}

/// Comme [`doc_ui`], avec un début de ligne dont des morceaux sont cliquables.
#[allow(clippy::too_many_arguments)]
pub(super) fn doc_ui_lead(ui: &mut egui::Ui, t: &Tokens, lang: Lang, msg: u64, doc: &Doc, lead: &[Lead], edited: bool, zones: &mut Zones) {
    doc_ui_versions(ui, t, lang, msg, doc, lead, edited, false, zones);
}

/// Comme [`doc_ui_lead`] ; `versions` : « (modifié) » est cliquable et déplie
/// les versions précédentes gardées par le journal (lot E1).
#[allow(clippy::too_many_arguments)]
pub(super) fn doc_ui_versions(ui: &mut egui::Ui, t: &Tokens, lang: Lang, msg: u64, doc: &Doc, lead: &[Lead], edited: bool, versions: bool, zones: &mut Zones) {
    let open = if doc.spoilers > 0 { revealed(ui.ctx(), msg) } else { Revealed::default() };
    let real = super::fonts::real(ui.ctx());
    let mut r = Render { t, lang, doc, open: &open, zones, body: egui::TextStyle::Body.resolve(ui.style()), small: egui::TextStyle::Small.resolve(ui.style()), real, jumbo: jumbo(doc), versions };
    let blocks = &doc.blocks;
    // Le début de ligne va dans le premier bloc s'il est du texte hors citation ; sinon, sur sa propre ligne.
    let lead_inline = blocks.first().is_some_and(|b| !b.quote && matches!(b.kind, BlockKind::Paragraph | BlockKind::Heading(_) | BlockKind::Subtext));
    let edited_inline = blocks.last().is_some_and(|b| !matches!(b.kind, BlockKind::Code { .. }));
    if !lead.is_empty() && !lead_inline {
        let mut line = Line::default();
        r.lead(&mut line, lead);
        if blocks.is_empty() && edited {
            r.edited(&mut line);
        }
        line.show(ui, t, r.zones);
    } else if blocks.is_empty() && edited {
        let mut line = Line::default();
        r.edited(&mut line);
        line.show(ui, t, r.zones);
    }
    let last = blocks.len().saturating_sub(1);
    let mut i = 0;
    while i < blocks.len() {
        if blocks[i].quote {
            let end = blocks[i..].iter().position(|b| !b.quote).map_or(blocks.len(), |k| i + k);
            let inner = egui::Frame::new().inner_margin(t.quote_indent()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                for (k, b) in blocks.iter().enumerate().take(end).skip(i) {
                    r.block(ui, b, &[], edited && edited_inline && k == last);
                }
            });
            let rect = inner.response.rect;
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(t.markdown.quote_width, rect.height()));
            ui.painter().rect_filled(bar, t.markdown.quote_width / 2.0, t.markdown.quote_bar);
            i = end;
        } else {
            let lead = if i == 0 && lead_inline { lead } else { &[] };
            r.block(ui, &blocks[i], lead, edited && edited_inline && i == last);
            i += 1;
        }
    }
    if edited && !blocks.is_empty() && !edited_inline {
        let mut line = Line::default();
        r.edited(&mut line);
        line.show(ui, t, r.zones);
    }
}

/// Texte mis en forme sur une seule ligne coupée par « … » (extrait d'une
/// réponse) : blocs mis bout à bout, spoilers masqués, rien de cliquable.
#[allow(clippy::too_many_arguments)]
pub(super) fn line_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, msg: u64, doc: &Doc, size: f32, color: Color32) -> egui::Response {
    let open = if doc.spoilers > 0 { revealed(ui.ctx(), msg) } else { Revealed::default() };
    let mut zones = Zones::new();
    let font = FontId::new(size, FontFamily::Proportional);
    let r = Render { t, lang, doc, open: &open, zones: &mut zones, body: font.clone(), small: font.clone(), real: super::fonts::real(ui.ctx()), jumbo: false, versions: false };
    let mut line = Line::default();
    for (i, b) in doc.blocks.iter().enumerate() {
        if i > 0 {
            line.append(" ", r.plain(font.clone(), color), None);
        }
        for s in &b.spans {
            if s.text.contains('\n') {
                let flat = Span { text: s.text.replace('\n', " "), ..s.clone() };
                r.span(ui, &mut line, &flat, &font, color);
            } else {
                r.span(ui, &mut line, s, &font, color);
            }
        }
    }
    // Emojis en image (lot T4) : places réservées dans le texte, peintes ensuite.
    super::fonts::route_marks(ui, &mut line.job);
    let (pos, galley, resp) = egui::Label::new(line.job).truncate().selectable(false).layout_in_ui(ui);
    if ui.is_rect_visible(resp.rect) {
        ui.painter().galley(pos, galley.clone(), color);
        super::emoji::paint_spots(ui, pos, &galley, &line.spots);
    }
    resp
}

/// Rend cliquables les rectangles de `zones` : liens (adresse au survol,
/// ouverture au clic) et spoilers masqués (révélés au clic).
pub(super) fn zones_ui(ui: &mut egui::Ui, t: &Tokens, msg: u64, zones: Zones) {
    for (i, (rect, hot)) in zones.into_iter().enumerate() {
        let resp = ui.interact(rect, Id::unique(("zone", msg, i)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        match hot {
            Hot::Link(url) => {
                if resp.hovered() {
                    ui.painter().hline(rect.x_range(), rect.bottom() - t.shapes.border, egui::Stroke::new(t.shapes.border, t.colors.link));
                }
                // Lien vers un message : ouvert ici (lot T12), pas dans le navigateur.
                if resp.clicked() && !super::search::follow_link(ui.ctx(), &url) {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                }
                // Clic droit : ouvrir, copier le lien (F7).
                resp.context_menu(|ui| super::composer_menu::link_menu(ui, t, &url));
                // L'adresse réelle, visible avant de cliquer (un lien masqué peut mentir).
                resp.on_hover_text(url);
            }
            Hot::Spoiler(n) => {
                if resp.hovered() {
                    ui.painter().rect_filled(rect, egui::CornerRadius::ZERO, t.colors.hover);
                }
                if resp.clicked() {
                    reveal(ui.ctx(), msg, n);
                    ui.ctx().request_repaint();
                }
            }
            Hot::Versions => {
                if resp.hovered() {
                    ui.painter().hline(rect.x_range(), rect.bottom() - t.shapes.border, egui::Stroke::new(t.shapes.border, t.colors.muted));
                }
                if resp.clicked() {
                    super::journal::ask_versions(ui.ctx(), msg);
                }
                resp.on_hover_text(crate::i18n::table::journal::t::VERSIONS_TIP.get(crate::i18n::language()));
            }
            Hot::User { id, name } => {
                if resp.clicked() {
                    super::profile::open_here(ui.ctx(), id, &name, rect);
                }
                if resp.secondary_clicked() {
                    super::profile::open_menu(ui.ctx(), id, &name, rect);
                }
            }
            Hot::Channel(id) => {
                if resp.hovered() {
                    ui.painter().rect_filled(rect, egui::CornerRadius::same(3), t.markdown.mention_bg);
                }
                if resp.clicked() {
                    super::mentions::go_channel(ui.ctx(), id);
                }
            }
            Hot::Role { id, name } => {
                if resp.clicked() {
                    super::mentions::open_role(ui.ctx(), id, &name, rect);
                }
            }
            Hot::Copy(text) => {
                if resp.hovered() {
                    crate::theme::icons::paint(crate::theme::icons::Icon::Copy, ui.painter(), rect.shrink(COPY_PAD), t.colors.text_strong);
                }
                if resp.clicked() {
                    ui.ctx().copy_text(text);
                }
                resp.on_hover_text(crate::i18n::table::ff::t::COPY_CODE.get(crate::i18n::language()));
            }
        }
    }
}

struct Render<'a> {
    t: &'a Tokens,
    lang: Lang,
    doc: &'a Doc,
    open: &'a Revealed,
    zones: &'a mut Zones,
    body: FontId,
    small: FontId,
    /// Vraies polices grasse et italique installées.
    real: super::fonts::Real,
    /// Message fait seulement d'emojis (au plus [MAX_JUMBO]) : affichés en grand.
    jumbo: bool,
    /// « (modifié) » déplie les versions gardées par le journal (lot E1).
    versions: bool,
}

/// Emojis au plus dans un message pour qu'ils soient affichés en grand (comme Discord).
const MAX_JUMBO: usize = 30;

/// Un seul paragraphe, rien que des emojis et des blancs.
fn jumbo(doc: &Doc) -> bool {
    let [b] = doc.blocks.as_slice() else { return false };
    let mut n = 0;
    let only = b.kind == BlockKind::Paragraph
        && !b.quote
        && b.spans.iter().all(|s| match s.kind {
            SpanKind::Unicode(_) | SpanKind::Emoji { .. } => {
                n += 1;
                true
            }
            SpanKind::Text => s.text.trim().is_empty(),
            _ => false,
        });
    only && (1..=MAX_JUMBO).contains(&n)
}

impl Render<'_> {
    fn hidden(&self, spoiler: Option<u32>) -> Option<u32> {
        spoiler.filter(|n| !self.open.0.contains(n))
    }

    fn lead(&self, line: &mut Line, lead: &[Lead]) {
        for (text, color, hot) in lead {
            line.append(text, self.plain(self.body.clone(), *color), hot.clone());
        }
    }

    fn edited(&self, line: &mut Line) {
        line.append(" ", self.plain(self.body.clone(), self.t.colors.text), None);
        // Versions gardées par le journal : le mot devient un lien discret.
        let (color, hot) = if self.versions { (self.t.colors.muted, Some(Hot::Versions)) } else { (self.t.colors.faint, None) };
        line.append(tx::EDITED.get(self.lang), self.plain(self.small.clone(), color), hot);
    }

    /// Interligne du thème pour le texte courant (titres, petits textes et
    /// extraits gardent celui de leur police).
    fn leading(&self, size: f32) -> Option<f32> {
        self.t.line_height().filter(|_| size == self.t.typography.body)
    }

    fn plain(&self, font_id: FontId, color: Color32) -> egui::TextFormat {
        let line_height = self.leading(font_id.size);
        egui::TextFormat { font_id, color, line_height, ..Default::default() }
    }

    fn block(&mut self, ui: &mut egui::Ui, b: &Block, lead: &[Lead], edited: bool) {
        let t = self.t;
        let (font, color) = match b.kind {
            BlockKind::Heading(level) => {
                let size = match level {
                    1 => t.markdown.h1,
                    2 => t.markdown.h2,
                    _ => t.markdown.h3,
                };
                let face = super::fonts::Face { bold: self.real.bold, italic: false };
                (super::fonts::font(size, face), t.colors.text_strong)
            }
            BlockKind::Subtext => (self.small.clone(), t.colors.muted),
            _ => (self.body.clone(), t.colors.text),
        };
        let mut line = Line::default();
        self.lead(&mut line, lead);
        for s in &b.spans {
            self.span(ui, &mut line, s, &font, color);
        }
        if edited {
            self.edited(&mut line);
        }
        match b.kind {
            BlockKind::Code { spoiler, .. } => {
                let frame = egui::Frame::new().fill(t.markdown.code_bg).stroke(t.border()).corner_radius(t.r_code()).inner_margin(t.code_margin());
                let zones = &mut *self.zones;
                let resp = frame.show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    line.show(ui, t, zones);
                });
                if let Some(n) = self.hidden(spoiler) {
                    let rect = resp.response.rect;
                    ui.painter().rect_filled(rect, t.r_code(), t.markdown.spoiler);
                    self.zones.push((rect, Hot::Spoiler(n)));
                } else if ui.rect_contains_pointer(resp.response.rect) {
                    // Bouton « copier » en haut à droite, tant que le bloc est survolé (V9).
                    let rect = resp.response.rect;
                    let side = Tokens::icon_side(t.typography.body) + 2.0 * COPY_PAD;
                    let gap = t.spacing.item_y;
                    let btn = egui::Rect::from_min_size(egui::pos2(rect.right() - side - gap, rect.top() + gap), egui::Vec2::splat(side));
                    ui.painter().rect_filled(btn, t.r_row(), t.colors.bg3);
                    crate::theme::icons::paint(crate::theme::icons::Icon::Copy, ui.painter(), btn.shrink(COPY_PAD), t.colors.muted);
                    self.zones.push((btn, Hot::Copy(b.spans.iter().map(|s| s.text.as_str()).collect())));
                }
            }
            BlockKind::Item { depth, marker } => {
                let indent = t.markdown.list_indent;
                let zones = &mut *self.zones;
                let body = self.body.clone();
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    ui.add_space(indent * f32::from(depth));
                    let text = match marker {
                        Marker::Bullet => "•".to_string(),
                        Marker::Number(n) => format!("{n}."),
                    };
                    let galley = ui.painter().layout_no_wrap(text, body, t.colors.muted);
                    let width = indent.max(galley.size().x + t.spacing.item_x);
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, galley.size().y), egui::Sense::hover());
                    ui.painter().galley(rect.min, galley, t.colors.muted);
                    ui.vertical(|ui| {
                        line.show(ui, t, zones);
                    });
                });
            }
            _ => {
                line.show(ui, t, self.zones);
            }
        }
    }

    fn span(&self, ui: &egui::Ui, line: &mut Line, s: &Span, font: &FontId, base: Color32) {
        let t = self.t;
        let st = s.style;
        let mono = || FontId::new(font.size, FontFamily::Monospace);
        // Vraie graisse et vraie italique si les polices sont là ; sinon le gras
        // se voit à sa couleur et l'italique est penché par egui.
        let face = super::fonts::Face { bold: st.bold && self.real.bold, italic: st.italic && self.real.italic };
        let font_id = if font.family == FontFamily::Proportional { super::fonts::font(font.size, face) } else { font.clone() };
        let strong = st.bold && !self.real.bold;
        let mut f = egui::TextFormat { font_id, color: if strong { t.colors.text_strong } else { base }, italics: st.italic && !face.italic, line_height: self.leading(font.size), ..Default::default() };
        let img = match &s.kind {
            SpanKind::Unicode(u) => Some(super::emoji::Img::Unicode(*u)),
            SpanKind::Emoji { id, animated, .. } => Some(super::emoji::Img::custom(*id, *animated)),
            _ => None,
        };
        if let Some(img) = img {
            let side = if self.jumbo { t.markdown.jumbo } else { t.markdown.emoji };
            let row = super::emoji::row_height(ui, font).max(if self.jumbo { side } else { 0.0 }).max(self.leading(font.size).unwrap_or(0.0));
            let hidden = self.hidden(st.spoiler);
            let spot = super::emoji::append(ui, &mut line.job, line.chars, &s.text, font, side, row, img);
            line.chars = spot.chars.end;
            match hidden {
                Some(n) => {
                    if let Some(sec) = line.job.sections.last_mut() {
                        sec.format.background = t.markdown.spoiler;
                    }
                    line.hot.push((spot.chars.clone(), Hot::Spoiler(n)));
                }
                None => line.spots.push(spot),
            }
            return;
        }
        let mut hot = None;
        match &s.kind {
            SpanKind::Text => {}
            SpanKind::Link(i) => {
                f.color = t.colors.link;
                let url = self.doc.links.get(*i as usize);
                let shown = super::settings::local::display(ui.ctx());
                if shown.hide_invites && url.is_some_and(|u| crate::state::local::is_invite(u)) {
                    // Mode streamer : masqué comme un spoiler (mise en page inchangée), sans clic.
                    f.color = t.markdown.spoiler;
                    f.background = t.markdown.spoiler;
                    line.append(&s.text, f, None);
                    return;
                }
                if shown.underline_links {
                    f.underline = egui::Stroke::new(t.shapes.border, f.color);
                }
                hot = url.map(|u| Hot::Link(u.clone()));
            }
            SpanKind::Mention(Mention::Role { color, id }) if *color != 0 && !super::settings::local::display(ui.ctx()).role_colors_off => {
                let c = Color32::from_rgb((color >> 16) as u8, (color >> 8) as u8, *color as u8);
                f.color = c;
                f.background = t.role_mention_bg(c);
                hot = Some(Hot::Role { id: *id, name: s.text.clone() });
            }
            SpanKind::Mention(m) => {
                f.color = t.markdown.mention_text;
                f.background = t.markdown.mention_bg;
                match m {
                    Mention::User(id) => hot = Some(Hot::User { id: *id, name: s.text.trim_start_matches('@').to_string() }),
                    Mention::Channel(id) => hot = Some(Hot::Channel(*id)),
                    Mention::Role { id, .. } => hot = Some(Hot::Role { id: *id, name: s.text.clone() }),
                    _ => {}
                }
            }
            SpanKind::Time(_) => f.background = t.markdown.code_bg,
            // Traités plus haut (image).
            SpanKind::Emoji { .. } | SpanKind::Unicode(_) => {}
            SpanKind::Code(class) => {
                f.font_id = mono();
                f.color = match class {
                    Class::Plain => t.colors.text,
                    Class::Keyword => t.markdown.keyword,
                    Class::String | Class::Added => t.markdown.string,
                    Class::Number => t.markdown.number,
                    Class::Comment => t.markdown.comment,
                    Class::Removed => t.colors.danger,
                };
            }
        }
        if st.code {
            f.font_id = mono();
            f.background = t.markdown.code_bg;
        }
        // Mot cherché (lot T12) : le fond des mentions, comme Discord.
        if st.mark {
            f.background = t.markdown.mention_bg;
        }
        let stroke = egui::Stroke::new(t.shapes.border, f.color);
        if st.underline {
            f.underline = stroke;
        }
        if st.strike {
            f.strikethrough = stroke;
        }
        if let Some(n) = self.hidden(st.spoiler) {
            // Masqué : texte et fond de la même couleur, mise en page inchangée une fois révélé.
            f.color = t.markdown.spoiler;
            f.background = t.markdown.spoiler;
            f.underline.color = t.markdown.spoiler;
            f.strikethrough.color = t.markdown.spoiler;
            hot = Some(Hot::Spoiler(n));
        }
        line.append(&s.text, f, hot);
    }
}

/// Comme `Label::new(job).wrap().selectable(true)`, avec la mise en page
/// gardée d'une image à l'autre ([`super::text_cache`]) : revenir dans un
/// salon ne refait pas celle de chaque message.
fn wrapped_label(ui: &mut egui::Ui, mut job: LayoutJob) -> (egui::Pos2, std::sync::Arc<egui::Galley>, egui::Response) {
    super::fonts::route_marks(ui, &mut job);
    let layout = *ui.layout();
    // Rangée qui passe à la ligne : egui décale la première ligne, cas laissé à egui.
    if layout.main_dir() == egui::Direction::LeftToRight && layout.main_wrap() {
        return egui::Label::new(job).wrap().selectable(true).layout_in_ui(ui);
    }
    job.wrap.max_width = ui.available_width();
    job.halign = layout.horizontal_placement();
    job.justify = layout.horizontal_justify();
    let galley = super::text_cache::layout(ui, job);
    egui::Label::new(galley).selectable(true).layout_in_ui(ui)
}

/// Un bloc en une seule mise en page : interligne régulier, aucun espace
/// égaré en début de ligne quand un lien passe à la ligne ; les passages
/// cliquables sont retrouvés par leurs caractères après la mise en page.
#[derive(Default)]
struct Line {
    job: LayoutJob,
    /// Caractères (et non octets) de chaque passage cliquable, dans l'ordre.
    hot: Vec<(Range<usize>, Hot)>,
    chars: usize,
    /// Emojis en image.
    spots: Vec<super::emoji::Spot>,
}

impl Line {
    fn append(&mut self, text: &str, format: egui::TextFormat, hot: Option<Hot>) {
        if text.is_empty() {
            return;
        }
        let start = self.chars;
        self.job.append(text, 0.0, format);
        self.chars += text.chars().count();
        if let Some(h) = hot {
            match self.hot.last_mut() {
                Some((r, last)) if *last == h && r.end == start => r.end = self.chars,
                _ => self.hot.push((start..self.chars, h)),
            }
        }
    }

    /// Mise en page sur la largeur disponible, texte sélectionnable ; les
    /// rectangles des passages cliquables sont ajoutés à `zones`.
    fn show(self, ui: &mut egui::Ui, t: &Tokens, zones: &mut Zones) -> std::sync::Arc<egui::Galley> {
        let (pos, galley, resp) = wrapped_label(ui, self.job);
        super::emoji::paint_spots(ui, pos, &galley, &self.spots);
        if !self.hot.is_empty() {
            // Un glyphe par caractère, sauf les retours à la ligne qui terminent une rangée.
            let mut c = 0;
            let mut k = 0;
            for row in &galley.rows {
                let mut current: Option<(usize, egui::Rect)> = None;
                for g in &row.glyphs {
                    while k < self.hot.len() && self.hot[k].0.end <= c {
                        k += 1;
                    }
                    let hit = (k < self.hot.len() && self.hot[k].0.contains(&c)).then_some(k);
                    let rect = egui::Rect::from_min_size(pos + row.pos.to_vec2() + egui::vec2(g.pos.x, 0.0), egui::vec2(g.advance_width, row.size.y));
                    current = match (current, hit) {
                        (Some((a, r)), Some(h)) if a == h => Some((a, r.union(rect))),
                        (prev, hit) => {
                            if let Some((a, r)) = prev {
                                zones.push((r, self.hot[a].1.clone()));
                            }
                            hit.map(|h| (h, rect))
                        }
                    };
                    c += 1;
                }
                if let Some((a, r)) = current {
                    zones.push((r, self.hot[a].1.clone()));
                }
                if row.ends_with_newline {
                    c += 1;
                }
            }
        }
        egui::text_selection::LabelSelectionState::label_text_selection(ui, &resp, pos, galley.clone(), t.colors.text, egui::Stroke::NONE);
        galley
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::markdown::{parse, Resolve, TimeStyle};

    struct Fake;
    impl Resolve for Fake {
        fn user(&self, _: u64) -> String {
            "Alice".into()
        }
        fn role(&self, _: u64) -> (String, u32) {
            ("Modo".into(), 0x3498db)
        }
        fn channel(&self, _: u64) -> String {
            "général".into()
        }
        fn time(&self, secs: i64, _: TimeStyle) -> Option<String> {
            Some(secs.to_string())
        }
    }

    /// Deux passes d'egui sans fenêtre, sur `width` points de large ; renvoie
    /// les formes peintes à la seconde.
    pub(crate) fn pass(width: f32, events: Vec<egui::Event>, mut f: impl FnMut(&mut egui::Ui)) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))),
                events: events.clone(),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, &mut f);
            out.textures_delta.clear();
            shapes = std::mem::take(&mut out.shapes);
        }
        shapes
    }

    /// Rangées (position, texte) de toutes les mises en page peintes.
    fn rows(shapes: &[egui::epaint::ClippedShape]) -> Vec<(egui::Pos2, String)> {
        let mut out = Vec::new();
        for s in shapes {
            if let egui::Shape::Text(text) = &s.shape {
                for r in &text.galley.rows {
                    out.push((text.pos + r.pos.to_vec2(), r.glyphs.iter().map(|g| g.chr).collect()));
                }
            }
        }
        out
    }

    fn render(width: f32, src: &str, lead: &[(String, Color32)], zones: &mut Zones) -> Vec<egui::epaint::ClippedShape> {
        let t = Tokens::default();
        let d = parse(src, &Fake);
        pass(width, Vec::new(), |ui| {
            zones.clear();
            doc_ui(ui, &t, Lang::Fr, 1, &d, lead, false, zones);
        })
    }

    /// Paragraphe renvoyé à la ligne avec un lien : interligne identique entre
    /// toutes les lignes, aucune ligne ne commence par une espace, et le lien
    /// est cliquable.
    #[test]
    fn wrapped_text_has_even_lines_and_no_leading_space() {
        let text = format!("{}https://exemple.invalid/une/adresse/assez/longue {}", "mot ".repeat(30), "suite ".repeat(20));
        let mut zones = Zones::new();
        let shapes = render(300.0, &text, &[], &mut zones);
        let rows = rows(&shapes);
        assert!(rows.len() >= 4, "{rows:?}");
        let gaps: Vec<f32> = rows.windows(2).map(|w| w[1].0.y - w[0].0.y).collect();
        assert!(gaps.iter().all(|g| (g - gaps[0]).abs() < 0.01), "interlignes {gaps:?}");
        assert!(rows.iter().all(|(p, s)| p.x == rows[0].0.x && !s.starts_with(' ')), "{rows:?}");
        assert!(!zones.is_empty());
        assert!(
            zones.iter().all(|(r, h)| *h == Hot::Link("https://exemple.invalid/une/adresse/assez/longue".into()) && r.width() > 0.0),
            "{zones:?}"
        );
    }

    /// Mention d'utilisateur et auteur en début de ligne : zones cliquables
    /// (carte de profil) ; une mention de rôle ne l'est pas.
    #[test]
    fn user_mentions_and_lead_author_are_clickable() {
        let t = Tokens::default();
        let d = parse("salut <@5> et <@&6>", &Fake);
        let mut zones = Zones::new();
        let lead: Vec<Lead> = vec![("Bob : ".into(), t.colors.text, Some(Hot::User { id: 7, name: "Bob".into() }))];
        pass(400.0, Vec::new(), |ui| {
            zones.clear();
            doc_ui_lead(ui, &t, Lang::Fr, 1, &d, &lead, false, &mut zones);
        });
        let hots: Vec<&Hot> = zones.iter().map(|(_, h)| h).collect();
        assert_eq!(hots, vec![&Hot::User { id: 7, name: "Bob".into() }, &Hot::User { id: 5, name: "Alice".into() }, &Hot::Role { id: 6, name: "@Modo".into() }]);
    }

    /// Mode compact : les lignes renvoyées repartent toutes du même bord,
    /// quelle que soit la largeur de l'heure.
    #[test]
    fn compact_continuation_lines_share_one_indent() {
        let t = Tokens::default();
        let mut starts = Vec::new();
        for clock in ["11:11", "00:00", "23:58"] {
            let lead = vec![(clock.to_string(), t.colors.faint), (" Auteur : ".to_string(), t.colors.text)];
            let shapes = render(260.0, &"mot **gras** ".repeat(20), &lead, &mut Zones::new());
            let rows = rows(&shapes);
            assert!(rows.len() > 2, "{rows:?}");
            assert!(rows[0].1.starts_with(clock));
            starts.extend(rows.iter().skip(1).map(|(p, _)| p.x));
        }
        assert!(starts.iter().all(|x| *x == starts[0]), "{starts:?}");
    }

    /// Spoiler masqué : peint de sa couleur, cliquable ; révélé, le texte
    /// reprend sa couleur, à la même place.
    #[test]
    fn spoiler_hidden_then_revealed() {
        let t = Tokens::default();
        let d = parse("avant ||secret|| après", &Fake);
        let ctx = egui::Context::default();
        let frame = |events: Vec<egui::Event>| {
            let mut zones = Zones::new();
            let mut sections = Vec::new();
            for _ in 0..2 {
                let input = egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))),
                    events: events.clone(),
                    ..Default::default()
                };
                let mut out = ctx.run_ui(input, |ui| {
                    zones.clear();
                    doc_ui(ui, &t, Lang::Fr, 9, &d, &[], false, &mut zones);
                    zones_ui(ui, &t, 9, zones.clone());
                });
                out.textures_delta.clear();
                sections = out
                    .shapes
                    .iter()
                    .filter_map(|s| if let egui::Shape::Text(x) = &s.shape { Some(x.galley.job.sections.iter().map(|s| (s.format.color, s.format.background)).collect::<Vec<_>>()) } else { None })
                    .flatten()
                    .collect();
            }
            (zones, sections)
        };
        let (zones, sections) = frame(Vec::new());
        assert_eq!(zones.len(), 1, "{zones:?}");
        assert_eq!(zones[0].1, Hot::Spoiler(0));
        assert!(sections.contains(&(t.markdown.spoiler, t.markdown.spoiler)), "{sections:?}");
        let at = zones[0].0.center();
        let click = vec![
            egui::Event::PointerMoved(at),
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() },
            egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() },
        ];
        frame(click);
        let (zones, sections) = frame(Vec::new());
        assert!(zones.is_empty(), "{zones:?}");
        assert!(!sections.iter().any(|(c, _)| *c == t.markdown.spoiler), "{sections:?}");
    }

    /// Blocs : code à chasse fixe sur son fond, citation avec sa barre,
    /// titre plus grand, liste en retrait ; la hauteur ne dépend pas de
    /// l'état des spoilers.
    #[test]
    fn blocks_use_theme_fonts_and_colors() {
        let t = Tokens::default();
        let src = "# Titre\n> cité\n- un\n  - deux\n```rust\nfn main() {}\n```\n-# petit";
        let shapes = render(500.0, src, &[], &mut Zones::new());
        let fills: Vec<Color32> = shapes.iter().filter_map(|s| if let egui::Shape::Rect(r) = &s.shape { Some(r.fill) } else { None }).collect();
        assert!(fills.contains(&t.markdown.code_bg), "{fills:?}");
        assert!(fills.contains(&t.markdown.quote_bar), "{fills:?}");
        let fonts: Vec<(String, FontId)> = shapes
            .iter()
            .filter_map(|s| if let egui::Shape::Text(x) = &s.shape { Some(x) } else { None })
            .flat_map(|x| x.galley.job.sections.iter().map(|s| (x.galley.job.text[s.byte_range.start.0..s.byte_range.end.0].to_string(), s.format.font_id.clone())))
            .collect();
        let font = |w: &str| fonts.iter().find(|(s, _)| s.contains(w)).map(|(_, f)| f.clone());
        assert_eq!(font("Titre").map(|f| f.size), Some(t.markdown.h1));
        assert_eq!(font("fn").map(|f| f.family), Some(FontFamily::Monospace));
        assert_eq!(font("petit").map(|f| f.size), Some(egui::TextStyle::Small.resolve(&egui::Style::default()).size), "style Small, que le thème règle sur typography.small");
        let rows = rows(&shapes);
        let x = |w: &str| rows.iter().find(|(_, s)| s.contains(w)).map(|(p, _)| p.x).unwrap_or(f32::NAN);
        assert!(x("deux") > x("un") && x("un") > x("Titre") && x("cité") > x("Titre"), "{rows:?}");
        // Même hauteur, spoiler masqué ou non (les hauteurs mesurées restent justes).
        let height = |src: &str| {
            let d = parse(src, &Fake);
            let mut h = 0.0;
            pass(400.0, Vec::new(), |ui| {
                let r = ui.scope(|ui| doc_ui(ui, &t, Lang::Fr, 3, &d, &[], true, &mut Zones::new())).response.rect;
                h = r.height();
            });
            h
        };
        assert_eq!(height("a ||b|| c\n||```x```||"), height("a b c\n```x```"));
    }
}
