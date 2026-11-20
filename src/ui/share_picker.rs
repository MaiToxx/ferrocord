//! Sélecteur de partage (lot M3, refait à la façon du client au lot
//! « partage ») : une barre de trois onglets (Applications, Écran entier,
//! Appareils), une grille de grandes vignettes sur deux colonnes, et en bas le
//! préréglage de qualité avec son bouton de réglages. Un clic sur une vignette
//! lance le partage ; seule la caméra passe par un aperçu quand le réglage
//! « Toujours afficher un aperçu » est coché. Ni titre ni croix : Échap ou un
//! clic hors de la fenêtre la ferme.
//! Les sources ne sont touchées que par le service de capture
//! (`state::share::CaptureService`), jamais d'ici.

use super::App;
use crate::i18n::table::share::{f as sf, t as st};
use crate::i18n::Lang;
use crate::state::share::{sound_allowed, CaptureService, NoCapture, ShareChoice, ShareEvent, ShareTab, SourceLists};
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use crate::video::capture::pixels::bgra_to_rgba_in_place;
use crate::video::capture::{CaptureError, Quality, Source, SourceKind};
use crate::video::{Frame, FrameData};
use eframe::egui::{self, pos2, vec2, Id, Rect, Sense, Vec2};
use std::collections::HashMap;

/// Taille de la fenêtre au plus (celle du client) ; réduite à la fenêtre de Ferrocord.
const MAX_SIZE: Vec2 = vec2(1200.0, 900.0);
/// Air laissé autour de la fenêtre.
const MARGIN: f32 = 24.0;
/// Hauteur de la barre d'onglets et du bas de fenêtre.
const BAR: f32 = 48.0;
/// Écart entre deux vignettes, et entre la grille et les barres.
const GAP: f32 = 16.0;
/// Largeur du menu de l'engrenage (qualité, son).
const MENU_WIDTH: f32 = 280.0;
/// Sous cette largeur de grille, une seule colonne (téléphone).
const TWO_COLUMNS_FROM: f32 = 440.0;

pub(in crate::ui) struct ShareUi {
    backend: Box<dyn CaptureService>,
    picker: Option<Picker>,
    choice: Option<ShareChoice>,
}

impl Default for ShareUi {
    fn default() -> Self {
        ShareUi { backend: Box::new(NoCapture), picker: None, choice: None }
    }
}

/// Aperçu de la caméra choisie.
enum Preview {
    Loading,
    Ready(egui::TextureHandle),
    Failed(CaptureError),
}

/// Étape « aperçu avant d'allumer la caméra » : rien n'est envoyé avant le bouton.
struct CameraStep {
    source: Source,
    preview: Preview,
}

struct Picker {
    tab: ShareTab,
    lists: Option<SourceLists>,
    /// Vignettes reçues (`None` : pas d'image pour cette source). Bornées par
    /// la liste (le service en fait 48 au plus par onglet), libérées à la fermeture.
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
    quality: Quality,
    audio: bool,
    camera: Option<CameraStep>,
    /// Ouvert par le bouton de la caméra (« aperçu avant d'allumer la
    /// caméra ») : à l'arrivée de la liste, cette caméra est choisie d'office
    /// (`Some(None)` : la première), ce qui lance son aperçu.
    auto_camera: Option<Option<String>>,
}

impl Picker {
    /// `quality` et `audio` : valeurs par défaut de la page « Voix et vidéo ».
    fn new(tab: ShareTab, quality: Quality, audio: bool) -> Self {
        Picker { tab, lists: None, thumbs: HashMap::new(), quality, audio, camera: None, auto_camera: None }
    }
}

/// Image BGRA vers texture egui ; `None` pour un autre format.
fn texture(ctx: &egui::Context, name: &str, frame: &Frame) -> Option<egui::TextureHandle> {
    let FrameData::Bgra(px) = &frame.data else { return None };
    let (w, h) = (frame.width as usize, frame.height as usize);
    if px.len() != 4 * w * h || w == 0 {
        return None;
    }
    let mut rgba = px.clone();
    bgra_to_rgba_in_place(&mut rgba);
    Some(ctx.load_texture(name, egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba), egui::TextureOptions::LINEAR))
}

/// Ligne d'un choix de qualité dans le menu de l'engrenage.
fn quality_label(lang: Lang, q: Quality) -> &'static str {
    match q {
        Quality::HD_30 => st::Q_720_30.get(lang),
        Quality::FHD_30 => st::Q_1080_30.get(lang),
        Quality::FHD_60 => st::Q_1080_60.get(lang),
        Quality::SOURCE_60 => st::Q_SOURCE_60.get(lang),
        _ => st::Q_SOURCE.get(lang),
    }
}

/// Nom du préréglage, d'après ce qu'il vaut vraiment (pas de « Gaming » :
/// le code ne connaît que ces quatre couples définition et cadence).
fn quality_name(lang: Lang, q: Quality) -> &'static str {
    match q {
        Quality::HD_30 => st::QN_HD.get(lang),
        Quality::FHD_30 => st::QN_FHD.get(lang),
        Quality::FHD_60 => st::QN_FHD_60.get(lang),
        Quality::SOURCE_60 => st::QN_SOURCE_60.get(lang),
        _ => st::QN_SOURCE.get(lang),
    }
}

/// Ligne de détail sous le nom : « 720p · 30 images/s », puis « · son ».
fn quality_detail(lang: Lang, q: Quality, audio: bool) -> String {
    let res = if q.is_source() { st::RES_SOURCE.get(lang).to_string() } else { format!("{}p", q.height) };
    let mut s = sf::quality_detail(lang, &res, q.fps);
    if audio {
        s.push_str(" · ");
        s.push_str(st::WITH_SOUND.get(lang));
    }
    s
}

fn tab_label(lang: Lang, tab: ShareTab) -> &'static str {
    match tab {
        ShareTab::Screens => st::TAB_SCREENS.get(lang),
        ShareTab::Windows => st::TAB_WINDOWS.get(lang),
        ShareTab::Cameras => st::TAB_CAMERAS.get(lang),
    }
}

/// Icône d'un genre de source (onglet, légende d'une vignette). Les fenêtres
/// portent une icône de fenêtre : Ferrocord ne lit pas les icônes des applications.
fn kind_icon(kind: SourceKind) -> Icon {
    match kind {
        SourceKind::Screen => Icon::Screen,
        SourceKind::Window => Icon::Window,
        SourceKind::Camera => Icon::Camera,
    }
}

fn empty_label(lang: Lang, tab: ShareTab) -> &'static str {
    match tab {
        ShareTab::Screens => st::NO_SCREENS.get(lang),
        ShareTab::Windows => st::NO_WINDOWS.get(lang),
        ShareTab::Cameras => st::NO_CAMERAS.get(lang),
    }
}

pub(in crate::ui) fn error_text(lang: Lang, e: &CaptureError) -> String {
    match e {
        CaptureError::CameraDenied => st::ERR_CAMERA_PRIVACY.get(lang).to_string(),
        CaptureError::NotFound => st::ERR_GONE.get(lang).to_string(),
        CaptureError::Unsupported => st::ERR_UNSUPPORTED.get(lang).to_string(),
        CaptureError::Backend(m) => sf::capture_error(lang, m),
    }
}

/// Nom affiché d'une source, et son infobulle (les écrans sont numérotés,
/// le nom que Windows leur donne passe dans l'infobulle).
fn source_label(lang: Lang, lists: &SourceLists, s: &Source) -> (String, String) {
    match s.kind {
        SourceKind::Screen => {
            let n = lists.screens.iter().position(|x| x.id == s.id).map_or(0, |i| i + 1);
            (sf::screen_short(lang, n), sf::screen_n(lang, n, &s.name))
        }
        SourceKind::Window => (s.name.clone(), s.name.clone()),
        SourceKind::Camera => (s.name.clone(), st::CAMERA_TILE_TIP.get(lang).to_string()),
    }
}

/// Rectangle de proportions `size` centré dans `outer`.
fn fitted(outer: Rect, size: Vec2) -> Rect {
    let scale = (outer.width() / size.x.max(1.0)).min(outer.height() / size.y.max(1.0));
    Rect::from_center_size(outer.center(), size * scale)
}

const UV: Rect = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));

pub(in crate::ui) fn paint_image(ui: &egui::Ui, rect: Rect, tex: &egui::TextureHandle) {
    ui.painter().image(tex.id(), fitted(rect, tex.size_vec2()), UV, egui::Color32::WHITE);
}

/// Image entière dans `rect`, coins arrondis comme sa tuile.
fn paint_rounded(ui: &egui::Ui, rect: Rect, tex: &egui::TextureHandle, r: egui::CornerRadius) {
    let at = fitted(rect, tex.size_vec2());
    ui.painter().add(egui::epaint::RectShape::filled(at, r, egui::Color32::WHITE).with_texture(tex.id(), UV));
}

/// Texte sur une ligne, coupé par « … » à `width`.
fn one_line(ui: &egui::Ui, text: &str, font: egui::FontId, color: egui::Color32, width: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    ui.painter().layout_job(job)
}

/// Fond des vignettes et de la barre d'onglets : un cran plus sombre que la fenêtre.
fn well(t: &Tokens) -> egui::Color32 {
    if t.is_light() { t.colors.bg1 } else { t.call_bg() }
}

/// Barre des trois onglets, sur toute la largeur : fond sombre arrondi,
/// l'onglet actif sur un aplat plus clair, icône et libellé centrés.
/// Rend l'onglet cliqué.
fn tab_bar(ui: &mut egui::Ui, t: &Tokens, lang: Lang, rect: Rect, active: ShareTab) -> Option<ShareTab> {
    const INSET: f32 = 4.0;
    let r = t.r_card();
    ui.painter().rect_filled(rect, r, well(t));
    let inner = rect.shrink(INSET);
    let seg_w = inner.width() / ShareTab::ALL.len() as f32;
    let mut clicked = None;
    for (i, tab) in ShareTab::ALL.into_iter().enumerate() {
        let seg = Rect::from_min_size(pos2(inner.left() + seg_w * i as f32, inner.top()), vec2(seg_w, inner.height()));
        let label = tab_label(lang, tab);
        let on = tab == active;
        let resp = ui.allocate_rect(seg, Sense::click());
        resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, on, label));
        let hot = resp.hovered() || resp.has_focus();
        let k = if on { 1.0 } else { super::motion::hover(ui.ctx(), resp.id, hot) * 0.5 };
        if k > 0.0 {
            ui.painter().rect_filled(seg, r.at_most(8), t.colors.bg2.gamma_multiply(k));
        }
        let ink = super::motion::mix(t.colors.muted, t.colors.text_strong, k);
        let side = Tokens::icon_side(t.typography.body);
        let room = seg.width() - side - 3.0 * t.spacing.item_x;
        let galley = one_line(ui, label, super::fonts::medium_font(t.typography.body), ink, room);
        let width = side + t.spacing.item_x + galley.size().x;
        let left = seg.center().x - width / 2.0;
        icons::paint(kind_icon(tab.kind()), ui.painter(), Rect::from_center_size(pos2(left + side / 2.0, seg.center().y), Vec2::splat(side)), ink);
        ui.painter().galley(pos2(left + side + t.spacing.item_x, seg.center().y - galley.size().y / 2.0), galley, ink);
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            clicked = Some(tab);
        }
    }
    clicked
}

/// Hauteur de la légende d'une vignette (icône et nom).
fn caption_height(t: &Tokens) -> f32 {
    (t.typography.body * 1.35).ceil() + 2.0 * t.spacing.item_y
}

/// Une vignette de la grille : image 16:9 aux coins arrondis, puis icône et
/// nom. Toute la tuile se clique ; rend vrai si on la choisit.
fn tile(ui: &mut egui::Ui, t: &Tokens, s: &Source, label: &str, tip: &str, image: Option<&egui::TextureHandle>, width: f32) -> bool {
    let image_h = (width * 9.0 / 16.0).round();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, image_h + caption_height(t)), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if ui.is_rect_visible(rect) {
        let frame = Rect::from_min_size(rect.min, vec2(width, image_h));
        let hot = resp.hovered() || resp.has_focus();
        let r = t.r_media();
        ui.painter().rect_filled(frame, r, well(t));
        match image {
            Some(tex) => paint_rounded(ui, frame, tex, r),
            // Pas d'image (caméra éteinte, fenêtre sans contenu, capture en attente) : l'icône du genre.
            None => {
                let side = (image_h * 0.22).clamp(24.0, 64.0);
                icons::paint(kind_icon(s.kind), ui.painter(), Rect::from_center_size(frame.center(), Vec2::splat(side)), t.colors.faint);
            }
        }
        let k = super::motion::hover(ui.ctx(), resp.id, hot);
        if k > 0.0 {
            ui.painter().rect_stroke(frame, r, egui::Stroke::new(2.0, t.colors.accent.gamma_multiply(k)), egui::StrokeKind::Inside);
        }
        let side = Tokens::icon_side(t.typography.body);
        let mid = frame.bottom() + caption_height(t) / 2.0 + 1.0;
        let ink = if hot { t.colors.text_strong } else { t.colors.text };
        icons::paint(kind_icon(s.kind), ui.painter(), Rect::from_center_size(pos2(rect.left() + side / 2.0, mid), Vec2::splat(side)), t.colors.muted);
        let galley = one_line(ui, label, super::fonts::medium_font(t.typography.body), ink, width - side - t.spacing.item_x);
        ui.painter().galley(pos2(rect.left() + side + t.spacing.item_x, mid - galley.size().y / 2.0), galley, ink);
    }
    let resp = if tip == label { resp } else { super::widgets::tip(resp, t, tip) };
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Texte centré dans `rect` (grille vide, aperçu en attente ou impossible).
fn say(ui: &egui::Ui, t: &Tokens, rect: Rect, text: &str, color: egui::Color32) {
    let galley = ui.painter().layout(text.to_string(), egui::FontId::proportional(t.typography.body), color, (rect.width() - 2.0 * t.spacing.card_pad).max(1.0));
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
}

fn menu_id() -> Id {
    Id::unique("partage-reglages")
}

/// Bouton carré à l'engrenage, en bas à droite : ouvre les réglages du partage.
fn gear(ui: &mut egui::Ui, t: &Tokens, rect: Rect, tip: &str) -> egui::Response {
    let resp = ui.allocate_rect(rect, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    let hot = resp.hovered() || resp.has_focus() || egui::Popup::is_id_open(ui.ctx(), menu_id());
    let k = super::motion::hover(ui.ctx(), resp.id, hot);
    let p = ui.painter();
    p.rect_filled(rect, t.r_control(), super::motion::mix(t.colors.button_bg, t.colors.control_bg, k));
    p.rect_stroke(rect, t.r_control(), t.border(), egui::StrokeKind::Inside);
    icons::paint(Icon::Gear, p, Rect::from_center_size(rect.center(), Vec2::splat(20.0)), super::motion::mix(t.colors.text, t.colors.text_strong, k));
    super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip)
}

/// Ce que le contenu de la fenêtre a demandé pendant cette image.
#[derive(Default)]
struct Asked {
    tab: Option<ShareTab>,
    pick: Option<Source>,
    /// « Allumer la caméra » dans l'étape d'aperçu.
    camera_on: bool,
    /// « Retour » : de l'aperçu de la caméra à la grille.
    back: bool,
}

impl App {
    /// Branche le vrai service de capture (écrans, fenêtres, caméras de Windows).
    pub fn attach_capture(&mut self, s: Box<dyn CaptureService>) {
        self.share.backend.cancel();
        self.share.backend = s;
        // « Partager l'écran » apparaît dans la barre de l'appel.
        self.enable_share_picker();
    }

    /// Ouvre le sélecteur de partage sur cet onglet (geste de l'utilisateur).
    pub fn open_share_picker(&mut self, tab: ShareTab) {
        self.share.backend.cancel();
        let v = &self.local.prefs.video;
        self.share.picker = Some(Picker::new(tab, v.share_quality, v.share_sound));
        self.share.backend.watch(tab);
    }

    /// Bouton de la caméra (panneau vocal, barre de l'appel) : l'éteint, ou
    /// l'allume — par l'aperçu du sélecteur quand « Toujours afficher un
    /// aperçu » est coché et que le sélecteur est branché, sinon tout de suite.
    pub(in crate::ui) fn toggle_camera(&mut self) {
        let on = !self.store.video.camera;
        if on && self.local.prefs.video.camera_preview && self.share_picker_ready() && self.store.voice.me.is_some() {
            self.open_share_picker(ShareTab::Cameras);
            if let Some(p) = &mut self.share.picker {
                p.auto_camera = Some(self.local.prefs.video.camera.clone());
            }
        } else {
            self.store.video_set_camera(on);
        }
    }

    pub fn share_picker_open(&self) -> bool {
        self.share.picker.is_some()
    }

    /// Qualité et son proposés par le sélecteur ouvert (tests).
    #[cfg(test)]
    pub(in crate::ui) fn share_picker_defaults(&self) -> Option<(Quality, bool)> {
        self.share.picker.as_ref().map(|p| (p.quality, p.audio))
    }

    /// Ouvre le menu de l'engrenage (qualité, son) du sélecteur ouvert (aperçu, raccourci).
    pub fn open_share_settings(&mut self) {
        if self.share.picker.is_some() {
            egui::Popup::open_id(&self.ctx, menu_id());
        }
    }

    /// Choisit une source par son identifiant, comme un clic sur sa vignette
    /// (aperçu, tests) ; faux si elle n'est pas listée.
    pub fn share_select(&mut self, id: &str) -> bool {
        let Some(p) = &self.share.picker else { return false };
        let Some(s) = p.lists.as_ref().and_then(|l| ShareTab::ALL.iter().flat_map(|t| l.of(*t)).find(|s| s.id == id)).cloned() else { return false };
        self.share_pick(s);
        true
    }

    /// Ce qui a été choisi dans le sélecteur, une fois (câblage : lot M5).
    pub fn take_share_choice(&mut self) -> Option<ShareChoice> {
        self.share.choice.take()
    }

    /// Clic sur une vignette : un écran ou une fenêtre part tout de suite ;
    /// une caméra passe par son aperçu si le réglage le demande.
    fn share_pick(&mut self, s: Source) {
        let Some(p) = &mut self.share.picker else { return };
        if s.kind == SourceKind::Camera && self.local.prefs.video.camera_preview {
            Self::camera_step(&mut *self.share.backend, p, s);
        } else {
            let choice = ShareChoice { audio: p.audio && sound_allowed(s.kind), source: s, quality: p.quality };
            self.share_close(Some(choice));
        }
    }

    /// Passe à l'aperçu de cette caméra (la seule image demandée à une caméra).
    fn camera_step(backend: &mut dyn CaptureService, p: &mut Picker, s: Source) {
        if p.tab != ShareTab::Cameras {
            p.tab = ShareTab::Cameras;
            backend.watch(p.tab);
        }
        backend.preview(&s);
        p.camera = Some(CameraStep { source: s, preview: Preview::Loading });
    }

    /// Ferme le sélecteur : plus aucune capture ne tourne. La qualité et le
    /// son choisis en dernier sont retenus pour la prochaine ouverture
    /// (demande de l'utilisateur, 0.1.25), que le partage ait été lancé ou non.
    fn share_close(&mut self, choice: Option<ShareChoice>) {
        self.share.backend.cancel();
        if let Some(p) = self.share.picker.take() {
            let mut v = (*self.local.prefs.video).clone();
            if (v.share_quality, v.share_sound) != (p.quality, p.audio) {
                (v.share_quality, v.share_sound) = (p.quality, p.audio);
                self.video_prefs_set(v);
            }
        }
        self.share.choice = choice;
        egui::Popup::close_id(&self.ctx, menu_id());
    }

    fn share_poll(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.share.picker else { return };
        let events = self.share.backend.poll();
        if !events.is_empty() {
            // Une réponse change ce qui est montré (vignette, aperçu).
            ctx.request_repaint();
        }
        for e in events {
            match e {
                ShareEvent::Sources(lists) => {
                    p.thumbs.retain(|id, _| ShareTab::ALL.iter().any(|t| lists.of(*t).iter().any(|s| &s.id == id)));
                    // Caméra en aperçu débranchée : retour à la grille.
                    if p.camera.as_ref().is_some_and(|c| !lists.cameras.contains(&c.source)) {
                        p.camera = None;
                    }
                    // Aperçu avant la caméra : celle des réglages, sinon la première.
                    let auto = p.auto_camera.take().and_then(|want| want.and_then(|id| lists.cameras.iter().find(|s| s.id == id)).or(lists.cameras.first()).cloned());
                    p.lists = Some(lists);
                    if let Some(s) = auto {
                        Self::camera_step(&mut *self.share.backend, p, s);
                    }
                }
                ShareEvent::Thumbnail { id, frame } => {
                    let tex = frame.as_ref().and_then(|f| texture(ctx, &format!("partage-vignette-{id}"), f));
                    p.thumbs.insert(id, tex);
                }
                ShareEvent::Preview { id, frame } => {
                    if let Some(c) = p.camera.as_mut().filter(|c| c.source.id == id) {
                        c.preview = match frame {
                            Ok(f) => texture(ctx, "partage-apercu", &f).map_or(Preview::Failed(CaptureError::Unsupported), Preview::Ready),
                            Err(e) => Preview::Failed(e),
                        };
                    }
                }
            }
        }
    }

    /// Le sélecteur, par-dessus le reste, tant qu'il est ouvert.
    pub(super) fn share_picker_ui(&mut self, ui: &mut egui::Ui) {
        if self.share.picker.is_none() {
            return;
        }
        self.share_poll(ui.ctx());
        let t = self.theme.clone();
        let lang = self.lang;
        let screen = ui.ctx().content_rect();
        // Jamais plus grand que la fenêtre de Ferrocord : la grille défile, les barres restent.
        let margin = MARGIN.min(screen.width() * 0.04);
        let size = vec2(MAX_SIZE.x.min(screen.width() - 2.0 * margin), MAX_SIZE.y.min(screen.height() - 2.0 * margin)).max(vec2(1.0, 1.0));
        let Some(p) = &mut self.share.picker else { return };
        let mut asked = Asked::default();
        let modal = super::widgets::pane(ui.ctx(), &t, Id::unique("selecteur_de_partage"), size, st::TITLE.get(lang), |ui, inner| {
            let top = Rect::from_min_size(inner.min, vec2(inner.width(), BAR.min(inner.height() / 4.0)));
            let foot = Rect::from_min_max(pos2(inner.left(), inner.bottom() - top.height()), inner.max);
            let mid = Rect::from_min_max(pos2(inner.left(), top.bottom() + GAP), pos2(inner.right(), foot.top() - GAP));
            asked.tab = tab_bar(ui, &t, lang, top, p.tab);
            if let Some(step) = &p.camera {
                // Aperçu avant d'allumer la caméra : l'image, puis « Retour » et « Allumer la caméra ».
                ui.painter().rect_filled(mid, t.r_media(), well(&t));
                match &step.preview {
                    Preview::Loading => say(ui, &t, mid, st::PREVIEW_LOADING.get(lang), t.colors.faint),
                    Preview::Ready(tex) => paint_rounded(ui, mid, tex, t.r_media()),
                    Preview::Failed(e) => {
                        let text = if matches!(e, CaptureError::Backend(_)) { st::PREVIEW_FAILED.get(lang).to_string() } else { error_text(lang, e) };
                        say(ui, &t, mid, &text, t.colors.danger);
                    }
                }
                let ready = matches!(step.preview, Preview::Ready(_));
                ui.scope_builder(egui::UiBuilder::new().max_rect(foot).layout(egui::Layout::right_to_left(egui::Align::Center)), |ui| {
                    use super::widgets::{button, Kind};
                    asked.camera_on = ui.add_enabled_ui(ready, |ui| button(ui, &t, Kind::Primary, st::CAMERA_ON.get(lang))).inner.clicked();
                    asked.back = button(ui, &t, Kind::Link, st::BACK.get(lang)).clicked();
                    // Nom de la caméra, à gauche, sur la place restante.
                    let side = Tokens::icon_side(t.typography.body);
                    let room = ui.available_width() - side - 2.0 * t.spacing.item_x;
                    if room > 40.0 {
                        let galley = one_line(ui, &step.source.name, super::fonts::demi_font(t.typography.body), t.colors.text_strong, room);
                        icons::paint(Icon::Camera, ui.painter(), Rect::from_center_size(pos2(foot.left() + side / 2.0, foot.center().y), Vec2::splat(side)), t.colors.muted);
                        ui.painter().galley(pos2(foot.left() + side + t.spacing.item_x, foot.center().y - galley.size().y / 2.0), galley, t.colors.text_strong);
                    }
                });
                return;
            }
            // Grille des vignettes : seule partie qui défile.
            match &p.lists {
                None => say(ui, &t, mid, st::LOADING.get(lang), t.colors.faint),
                Some(lists) if lists.of(p.tab).is_empty() => say(ui, &t, mid, empty_label(lang, p.tab), t.colors.faint),
                Some(lists) => {
                    ui.scope_builder(egui::UiBuilder::new().max_rect(mid), |ui| {
                        egui::ScrollArea::vertical().id_salt(("partage-sources", p.tab as u8)).auto_shrink([false, false]).show(ui, |ui| {
                            let columns = if mid.width() >= TWO_COLUMNS_FROM { 2 } else { 1 };
                            let tile_w = ((ui.available_width() - GAP * (columns - 1) as f32) / columns as f32).floor();
                            ui.spacing_mut().item_spacing = vec2(GAP, GAP / 2.0);
                            for row in lists.of(p.tab).chunks(columns) {
                                ui.horizontal(|ui| {
                                    for s in row {
                                        let (label, tip) = source_label(lang, lists, s);
                                        let image = p.thumbs.get(&s.id).and_then(Option::as_ref);
                                        if tile(ui, &t, s, &label, &tip, image, tile_w) {
                                            asked.pick = Some(s.clone());
                                        }
                                    }
                                });
                            }
                        });
                    });
                }
            }
            // Bas de fenêtre : le préréglage en clair, et l'engrenage qui le règle.
            // Une caméra s'allume à sa propre définition : rien à régler sur son onglet.
            if p.tab == ShareTab::Cameras {
                return;
            }
            let side = foot.height().min(44.0);
            let button = Rect::from_center_size(pos2(foot.right() - side / 2.0, foot.center().y), Vec2::splat(side));
            let room = foot.width() - side - GAP;
            let name = one_line(ui, quality_name(lang, p.quality), super::fonts::demi_font(t.typography.body + 1.0), t.colors.text_strong, room);
            let detail = one_line(ui, &quality_detail(lang, p.quality, p.audio), egui::FontId::proportional(t.typography.small), t.colors.muted, room);
            let block = name.size().y + detail.size().y + 2.0;
            let y = foot.center().y - block / 2.0;
            let text = Rect::from_min_size(pos2(foot.left(), y), vec2(name.size().x.max(detail.size().x), block));
            let said = format!("{} : {}", quality_name(lang, p.quality), quality_detail(lang, p.quality, p.audio));
            ui.allocate_rect(text, Sense::hover()).widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &said));
            let line = name.size().y;
            ui.painter().galley(pos2(foot.left(), y), name, t.colors.text_strong);
            ui.painter().galley(pos2(foot.left(), y + line + 2.0), detail, t.colors.muted);
            let resp = gear(ui, &t, button, st::SETTINGS_TIP.get(lang));
            egui::Popup::menu(&resp).id(menu_id()).gap(t.spacing.item_x).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                ui.set_width(MENU_WIDTH);
                super::widgets::field_label(ui, &t, st::QUALITY.get(lang));
                for q in Quality::PRESETS {
                    if super::menu::radio(ui, &t, quality_label(lang, q), p.quality == q).clicked() {
                        p.quality = q;
                    }
                }
                super::menu::separator(ui, &t);
                super::menu::check(ui, &t, st::SOUND.get(lang), &mut p.audio);
                // Ce que « le son » veut dire pour cet onglet, en clair sous la case.
                let tip = if p.tab == ShareTab::Windows { st::SOUND_TIP_WINDOW } else { st::SOUND_TIP_SCREEN };
                egui::Frame::new().inner_margin(egui::Margin::symmetric(t.spacing.item_x as i8, 0)).show(ui, |ui| {
                    ui.set_max_width(MENU_WIDTH - 2.0 * t.spacing.item_x);
                    ui.add(egui::Label::new(egui::RichText::new(tip.get(lang)).size(t.typography.small).color(t.colors.muted)).wrap());
                });
            });
        });
        if let Some(tab) = asked.tab
            && (tab != p.tab || p.camera.is_some())
        {
            p.camera = None;
            if tab != p.tab {
                p.tab = tab;
                // Les vignettes de cet onglet, tout de suite puis à leur rythme.
                self.share.backend.watch(tab);
            }
        }
        if asked.back {
            p.camera = None;
        }
        if asked.camera_on
            && let Some(step) = p.camera.take()
        {
            let choice = ShareChoice { source: step.source, quality: p.quality, audio: false };
            return self.share_close(Some(choice));
        }
        if let Some(s) = asked.pick {
            return self.share_pick(s);
        }
        if modal.closed {
            self.share_close(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::voice::{VoiceEvent, VoiceState};
    use crate::state::{Event, Store};
    use crate::ui::tests::offline_boot;
    use std::sync::{Arc, Mutex};

    const ME: u64 = 1;
    const G: u64 = 10;
    const VOC: u64 = 20;

    fn bounds(n: &egui::accesskit::Node) -> Option<Rect> {
        let b = n.bounds()?;
        Some(Rect::from_min_max(pos2(b.x0 as f32, b.y0 as f32), pos2(b.x1 as f32, b.y1 as f32)))
    }

    fn named(n: &egui::accesskit::Node, label: &str) -> bool {
        // Un texte est nommé par sa valeur.
        n.label() == Some(label) || n.value() == Some(label)
    }

    /// Rectangle de l'élément nommé exactement `label`, où qu'il soit (menu
    /// de l'engrenage, vignette sortie de la vue).
    fn anywhere(out: &egui::FullOutput, label: &str) -> Option<Rect> {
        let update = out.platform_output.accesskit_update.as_ref()?;
        update.nodes.iter().find(|(_, n)| named(n, label)).and_then(|(_, n)| bounds(n))
    }

    /// Rectangle de l'élément nommé exactement `label` dans la fenêtre de
    /// partage : l'écran dessous a ses propres boutons « Allumer la caméra »
    /// et « Retour », qui ne comptent pas.
    fn find(out: &egui::FullOutput, label: &str) -> Option<Rect> {
        use egui::accesskit::{NodeId, Role};
        let update = out.platform_output.accesskit_update.as_ref()?;
        let parent = |id: NodeId| update.nodes.iter().find(|(_, n)| n.children().contains(&id)).map(|(p, _)| *p);
        let (pane, _) = update.nodes.iter().find(|(_, n)| n.role() == Role::Dialog && n.label() == Some(st::TITLE.get(Lang::Fr)))?;
        // Sommet de la fenêtre : son ancêtre posé directement sous la racine.
        let mut top = *pane;
        while let Some(p) = parent(top).filter(|p| parent(*p).is_some()) {
            top = p;
        }
        let mut todo = vec![top];
        while let Some(id) = todo.pop() {
            let (_, n) = update.nodes.iter().find(|(i, _)| *i == id)?;
            if named(n, label) {
                return bounds(n);
            }
            todo.extend(n.children().iter().copied());
        }
        None
    }

    /// Ce qui est parti vers l'appel : partage (source, qualité, son) et caméra.
    fn sent(app: &App) -> (Option<(String, Quality, bool)>, Option<String>) {
        let v = &app.store.video;
        (v.share.as_ref().map(|s| (s.source_id.clone(), s.quality, s.audio)), v.camera.then(|| v.camera_source.clone().unwrap_or_default()))
    }

    struct Offline;
    impl crate::ui::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    /// Ce que le faux service a vu passer.
    #[derive(Default)]
    struct Seen {
        watches: Vec<ShareTab>,
        previews: Vec<String>,
        cancels: usize,
    }

    /// Faux service : sources inventées, une vignette grise par source de l'onglet suivi.
    #[derive(Clone, Default)]
    struct Fake {
        seen: Arc<Mutex<Seen>>,
        queue: Arc<Mutex<Vec<ShareEvent>>>,
        windows: usize,
    }

    fn src(id: &str, name: &str, kind: SourceKind) -> Source {
        Source { id: id.into(), name: name.into(), kind }
    }

    fn gray() -> Frame {
        Frame { width: 16, height: 9, pts_us: 0, data: FrameData::Bgra(vec![128; 4 * 16 * 9]) }
    }

    impl Fake {
        fn lists(&self) -> SourceLists {
            SourceLists {
                screens: vec![src("screen:1", "Moniteur principal", SourceKind::Screen), src("screen:2", "Moniteur secondaire", SourceKind::Screen)],
                windows: (1..=self.windows).map(|i| src(&format!("window:{i}"), &format!("Fenêtre d'essai {i}"), SourceKind::Window)).collect(),
                cameras: vec![src("camera:a", "Webcam intégrée", SourceKind::Camera)],
            }
        }
    }

    impl CaptureService for Fake {
        fn watch(&mut self, tab: ShareTab) {
            self.seen.lock().unwrap().watches.push(tab);
            let lists = self.lists();
            let mut q = self.queue.lock().unwrap();
            q.push(ShareEvent::Sources(lists.clone()));
            for s in lists.of(tab).iter().filter(|s| crate::state::share::wants_thumbnail(s.kind)) {
                q.push(ShareEvent::Thumbnail { id: s.id.clone(), frame: Some(gray()) });
            }
        }
        fn preview(&mut self, s: &Source) {
            self.seen.lock().unwrap().previews.push(s.id.clone());
        }
        fn cancel(&mut self) {
            self.seen.lock().unwrap().cancels += 1;
            self.queue.lock().unwrap().clear();
        }
        fn poll(&mut self) -> Vec<ShareEvent> {
            std::mem::take(&mut *self.queue.lock().unwrap())
        }
    }

    fn app(windows: usize) -> (App, Fake) {
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        // Session ouverte : le sélecteur se pose sur l'écran principal.
        let mut store = Store::new();
        store.apply(Event::Ready { me: crate::state::model::User { id: ME, name: "moi".into(), avatar: crate::state::model::Avatar::default_for(ME) }, guilds: vec![], dms: vec![] });
        // Dans un salon vocal : un partage ou la caméra peut vraiment partir.
        store.voice_join_call(VOC);
        let state = VoiceState { user_id: ME, guild_id: Some(G), channel_id: VOC, ..Default::default() };
        store.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(G), user_id: ME, state: Some(state), session_id: Some("sess".into()) }));
        store.apply(Event::Voice(VoiceEvent::Connected));
        store.take_commands();
        let mut app = App::with_context(ctx, offline_boot(Box::new(Offline), store, images));
        app.lang = Lang::Fr;
        let fake = Fake { windows, ..Default::default() };
        app.attach_capture(Box::new(fake.clone()));
        (app, fake)
    }

    const BIG: Vec2 = vec2(1375.0, 900.0);

    fn frame(app: &mut App, size: Vec2, events: Vec<egui::Event>) -> egui::FullOutput {
        let ctx = app.ctx.clone();
        ctx.enable_accesskit();
        app.logic_pass();
        let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, size)), events, ..Default::default() };
        let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
        out.textures_delta.clear();
        out
    }

    fn settle(app: &mut App, size: Vec2) -> egui::FullOutput {
        for _ in 0..6 {
            frame(app, size, Vec::new());
        }
        frame(app, size, Vec::new())
    }

    fn click(app: &mut App, size: Vec2, label: &str) {
        let out = settle(app, size);
        let at = find(&out, label).or_else(|| anywhere(&out, label)).unwrap_or_else(|| panic!("« {label} » introuvable")).center();
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(app, size, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(app, size, vec![button(false)]);
        frame(app, size, Vec::new());
    }

    /// Ouverture sur l'onglet demandé ; trois onglets dans l'ordre du client,
    /// ni titre, ni croix, ni « Actualiser », ni bouton « Partager ».
    #[test]
    fn opens_like_the_client() {
        let (mut app, fake) = app(3);
        app.request_share_picker();
        let out = settle(&mut app, BIG);
        let x = |label: &str| find(&out, label).unwrap_or_else(|| panic!("« {label} » introuvable")).center().x;
        assert!(x("Applications") < x("Écran entier") && x("Écran entier") < x("Appareils"));
        assert_eq!(fake.seen.lock().unwrap().watches, [ShareTab::Windows], "ouvert sur « Applications », suivi lancé une fois");
        for gone in ["Actualiser", "Aperçu", "Annuler", crate::i18n::t::CLOSE_TIP.get(Lang::Fr)] {
            assert!(find(&out, gone).is_none(), "« {gone} » ne doit plus être là");
        }
        // Grille sur deux colonnes : deux vignettes côte à côte, la troisième dessous.
        let tile = |n: usize| find(&out, &format!("Fenêtre d'essai {n}")).unwrap();
        assert!(tile(1).right() < tile(2).left() && (tile(1).top() - tile(2).top()).abs() < 1.0 && tile(3).top() > tile(1).bottom());
        assert!(tile(1).width() > 400.0, "grandes vignettes : {}", tile(1).width());
        // Préréglage en clair et engrenage.
        assert!(find(&out, "HD : 720p · 30\u{a0}images/s").is_some());
        assert!(find(&out, st::SETTINGS_TIP.get(Lang::Fr)).is_some());
        // Changer d'onglet suit les vignettes de cet onglet.
        click(&mut app, BIG, "Écran entier");
        assert_eq!(fake.seen.lock().unwrap().watches, [ShareTab::Windows, ShareTab::Screens]);
        let out = settle(&mut app, BIG);
        assert!(find(&out, "Écran 1").is_some() && find(&out, "Écran 2").is_some() && find(&out, "Fenêtre d'essai 1").is_none());
    }

    /// Un clic sur une vignette d'écran demande le partage tout de suite,
    /// avec la qualité et le son courants ; plus rien ne tourne ensuite.
    #[test]
    fn a_click_on_a_screen_shares_it_at_once() {
        let (mut app, fake) = app(2);
        let mut p = app.local_prefs();
        p.video.share_quality = Quality::FHD_60;
        p.video.share_sound = true;
        app.set_local_prefs(p);
        app.open_share_picker(ShareTab::Screens);
        let out = settle(&mut app, BIG);
        assert!(find(&out, "Full HD fluide : 1080p · 60\u{a0}images/s · son").is_some());
        click(&mut app, BIG, "Écran 2");
        assert!(!app.share_picker_open());
        assert_eq!(sent(&app), (Some(("screen:2".to_string(), Quality::FHD_60, true)), None));
        assert_eq!(app.store.video.share.as_ref().map(|s| s.name.as_str()), Some("Moniteur secondaire"));
        let seen = fake.seen.lock().unwrap();
        assert!(seen.previews.is_empty() && seen.cancels >= 2, "aucun aperçu, suivi arrêté à la fermeture");
    }

    /// Le menu de l'engrenage change la qualité et le son du partage lancé ensuite.
    #[test]
    fn the_gear_menu_sets_quality_and_sound() {
        let (mut app, _fake) = app(2);
        app.open_share_picker(ShareTab::Windows);
        click(&mut app, BIG, st::SETTINGS_TIP.get(Lang::Fr));
        click(&mut app, BIG, st::Q_1080_30.get(Lang::Fr));
        click(&mut app, BIG, st::SOUND.get(Lang::Fr));
        assert_eq!(app.share_picker_defaults(), Some((Quality::FHD_30, true)));
        // Le menu se ferme d'abord (Échap), la fenêtre reste.
        frame(&mut app, BIG, vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
        assert!(app.share_picker_open());
        click(&mut app, BIG, "Fenêtre d'essai 2");
        assert_eq!(sent(&app), (Some(("window:2".to_string(), Quality::FHD_30, true)), None));
        // Les réglages choisis en dernier sont retenus : réglages de l'ordinateur
        // et prochaine ouverture du sélecteur.
        let v = app.local_prefs().video;
        assert_eq!((v.share_quality, v.share_sound), (Quality::FHD_30, true));
        app.open_share_picker(ShareTab::Screens);
        assert_eq!(app.share_picker_defaults(), Some((Quality::FHD_30, true)));
    }

    /// Caméra, réglage « aperçu » coché : le clic montre l'aperçu, rien ne
    /// part avant « Allumer la caméra » ; « Retour » revient à la grille.
    #[test]
    fn a_camera_goes_through_its_preview_when_asked() {
        let (mut app, fake) = app(1);
        assert!(app.local_prefs().video.camera_preview, "réglage par défaut");
        app.open_share_picker(ShareTab::Cameras);
        click(&mut app, BIG, "Webcam intégrée");
        assert!(app.share_picker_open() && sent(&app) == (None, None));
        assert_eq!(fake.seen.lock().unwrap().previews, ["camera:a"]);
        // Tant que l'image n'est pas là, le bouton ne fait rien.
        click(&mut app, BIG, st::CAMERA_ON.get(Lang::Fr));
        assert!(app.share_picker_open() && sent(&app) == (None, None));
        click(&mut app, BIG, st::BACK.get(Lang::Fr));
        assert!(find(&settle(&mut app, BIG), st::CAMERA_ON.get(Lang::Fr)).is_none());
        click(&mut app, BIG, "Webcam intégrée");
        fake.queue.lock().unwrap().push(ShareEvent::Preview { id: "camera:a".into(), frame: Ok(gray()) });
        click(&mut app, BIG, st::CAMERA_ON.get(Lang::Fr));
        assert!(!app.share_picker_open());
        assert_eq!(sent(&app), (None, Some("camera:a".to_string())));
    }

    /// Caméra, réglage décoché : le clic l'allume, sans image demandée.
    #[test]
    fn a_camera_turns_on_at_once_without_the_preview_setting() {
        let (mut app, fake) = app(1);
        let mut p = app.local_prefs();
        p.video.camera_preview = false;
        app.set_local_prefs(p);
        app.open_share_picker(ShareTab::Cameras);
        click(&mut app, BIG, "Webcam intégrée");
        assert!(!app.share_picker_open());
        assert_eq!(sent(&app), (None, Some("camera:a".to_string())));
        assert!(fake.seen.lock().unwrap().previews.is_empty());
    }

    /// Échap ferme sans rien lancer, et arrête le suivi.
    #[test]
    fn escape_closes_without_sharing() {
        let (mut app, fake) = app(2);
        app.open_share_picker(ShareTab::Windows);
        settle(&mut app, BIG);
        let before = fake.seen.lock().unwrap().cancels;
        frame(&mut app, BIG, vec![egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: Default::default() }]);
        frame(&mut app, BIG, Vec::new());
        assert!(!app.share_picker_open() && sent(&app) == (None, None));
        assert_eq!(fake.seen.lock().unwrap().cancels, before + 1);
        // Un clic hors de la fenêtre aussi.
        app.open_share_picker(ShareTab::Windows);
        settle(&mut app, BIG);
        let at = pos2(6.0, 450.0);
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(&mut app, BIG, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(&mut app, BIG, vec![button(false)]);
        frame(&mut app, BIG, Vec::new());
        assert!(!app.share_picker_open() && sent(&app) == (None, None));
    }

    /// Petite fenêtre (1280 × 720 et moins) : rien ne dépasse ; la barre
    /// d'onglets et le bas restent visibles, la grille défile entre les deux.
    #[test]
    fn nothing_overflows_a_small_window() {
        for size in [vec2(1280.0, 720.0), vec2(1024.0, 600.0), vec2(800.0, 480.0)] {
            let (mut app, _fake) = app(9);
            app.open_share_picker(ShareTab::Windows);
            let out = settle(&mut app, size);
            let screen = Rect::from_min_size(egui::Pos2::ZERO, size);
            let pane = find(&out, st::TITLE.get(Lang::Fr)).expect("fenêtre nommée pour les lecteurs d'écran");
            assert!(screen.shrink(4.0).contains_rect(pane), "{size:?} : la fenêtre {pane:?} dépasse");
            for label in ["Applications", "Appareils", st::SETTINGS_TIP.get(Lang::Fr), "HD : 720p · 30\u{a0}images/s"] {
                let r = find(&out, label).unwrap_or_else(|| panic!("{size:?} : « {label} » introuvable"));
                assert!(pane.contains_rect(r), "{size:?} : « {label} » {r:?} hors de {pane:?}");
            }
            let tabs = find(&out, "Applications").unwrap();
            let gear = find(&out, st::SETTINGS_TIP.get(Lang::Fr)).unwrap();
            let first = find(&out, "Fenêtre d'essai 1").unwrap();
            assert!(first.top() >= tabs.bottom() && first.top() < gear.top(), "{size:?} : première vignette entre les barres");
            // Les dernières vignettes sont plus bas dans la grille : elle défile.
            let last = find(&out, "Fenêtre d'essai 9").unwrap();
            assert!(last.top() > gear.top(), "{size:?} : la grille dépasse sa zone, donc défile");
        }
    }
}
