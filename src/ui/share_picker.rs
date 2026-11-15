//! Sélecteur de partage (lot M3) : onglets Écrans, Fenêtres, Caméras avec
//! leurs vignettes, aperçu de la source choisie, qualité, « Partager le son ».
//! Rien n'est envoyé avant « Partager » ; l'aperçu montre ce qui le sera.
//! Les sources ne sont touchées que par le service de capture
//! (`state::share::CaptureService`), jamais d'ici.

use super::App;
use crate::i18n::table::share::{f as sf, t as st};
use crate::i18n::Lang;
use crate::state::share::{sound_allowed, CaptureService, NoCapture, ShareChoice, ShareEvent, ShareTab, SourceLists};
use crate::theme::Tokens;
use crate::video::capture::pixels::bgra_to_rgba_in_place;
use crate::video::capture::{CaptureError, Quality, Source, SourceKind};
use crate::video::{Frame, FrameData};
use eframe::egui::{self, Id, RichText, Sense};
use std::collections::HashMap;

/// Tuiles par rangée.
const COLUMNS: usize = 3;

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

/// Aperçu de la source choisie.
enum Preview {
    Loading,
    Ready(egui::TextureHandle),
    Failed(CaptureError),
}

struct Picker {
    tab: ShareTab,
    lists: Option<SourceLists>,
    /// Vignettes reçues (`None` : pas d'image pour cette source). Bornées par
    /// la liste (le service en fait 48 au plus), libérées à la fermeture.
    thumbs: HashMap<String, Option<egui::TextureHandle>>,
    selected: Option<Source>,
    preview: Option<(String, Preview)>,
    quality: Quality,
    audio: bool,
    /// Ouvert par le bouton de la caméra (« aperçu avant d'allumer la
    /// caméra ») : à l'arrivée de la liste, cette caméra est choisie d'office
    /// (`Some(None)` : la première), ce qui lance son aperçu.
    auto_camera: Option<Option<String>>,
}

impl Picker {
    /// `quality` et `audio` : valeurs par défaut de la page « Voix et vidéo ».
    fn new(tab: ShareTab, quality: Quality, audio: bool) -> Self {
        Picker { tab, lists: None, thumbs: HashMap::new(), selected: None, preview: None, quality, audio, auto_camera: None }
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

fn quality_label(lang: Lang, q: Quality) -> &'static str {
    match q {
        Quality::HD_30 => st::Q_720_30.get(lang),
        Quality::FHD_30 => st::Q_1080_30.get(lang),
        Quality::FHD_60 => st::Q_1080_60.get(lang),
        _ => st::Q_SOURCE.get(lang),
    }
}

fn tab_label(lang: Lang, tab: ShareTab) -> &'static str {
    match tab {
        ShareTab::Screens => st::TAB_SCREENS.get(lang),
        ShareTab::Windows => st::TAB_WINDOWS.get(lang),
        ShareTab::Cameras => st::TAB_CAMERAS.get(lang),
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

/// Nom affiché d'une source (les écrans sont numérotés).
fn source_label(lang: Lang, lists: &SourceLists, s: &Source) -> String {
    match s.kind {
        SourceKind::Screen => {
            let n = lists.screens.iter().position(|x| x.id == s.id).map_or(0, |i| i + 1);
            sf::screen_n(lang, n, &s.name)
        }
        _ => s.name.clone(),
    }
}

/// Rectangle de proportions `size` centré dans `outer`.
fn fitted(outer: egui::Rect, size: egui::Vec2) -> egui::Rect {
    let scale = (outer.width() / size.x.max(1.0)).min(outer.height() / size.y.max(1.0));
    egui::Rect::from_center_size(outer.center(), size * scale)
}

pub(in crate::ui) fn paint_image(ui: &egui::Ui, rect: egui::Rect, tex: &egui::TextureHandle) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    ui.painter().image(tex.id(), fitted(rect, tex.size_vec2()), uv, egui::Color32::WHITE);
}

/// Contenu d'une tuile : la vignette, ou un mot à sa place.
enum Face<'a> {
    Image(&'a egui::TextureHandle),
    Mark(&'a str),
}

/// Une tuile de la liste ; renvoie vrai si on la choisit.
fn tile(ui: &mut egui::Ui, t: &Tokens, label: &str, face: Face<'_>, tip: &str, selected: bool, width: f32) -> bool {
    let mut clicked = false;
    ui.vertical(|ui| {
        ui.set_width(width);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, width * 9.0 / 16.0), Sense::click());
        let p = ui.painter();
        p.rect_filled(rect, t.r_media(), if resp.hovered() { t.colors.hover } else { t.colors.bg1 });
        match face {
            Face::Image(tex) => paint_image(ui, rect.shrink(t.spacing.item_y), tex),
            Face::Mark(mark) => {
                p.text(rect.center(), egui::Align2::CENTER_CENTER, mark, egui::FontId::proportional(t.typography.small), t.colors.faint);
            }
        }
        if selected {
            p.rect_stroke(rect, t.r_media(), egui::Stroke::new(t.shapes.focus * 2.0, t.colors.accent), egui::StrokeKind::Inside);
        }
        let resp = resp.on_hover_text(tip);
        let color = if selected { t.colors.text_strong } else { t.colors.muted };
        ui.add(egui::Label::new(RichText::new(label).small().color(color)).truncate().selectable(false));
        clicked = resp.clicked();
    });
    clicked
}

fn kind_index(kind: SourceKind) -> usize {
    match kind {
        SourceKind::Screen => 0,
        SourceKind::Window => 1,
        SourceKind::Camera => 2,
    }
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
        self.share.backend.list();
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

    /// Choisit une source par son identifiant (aperçu, tests) ; faux si elle n'est pas listée.
    pub fn share_select(&mut self, id: &str) -> bool {
        let Some(p) = &mut self.share.picker else { return false };
        let Some(s) = p.lists.as_ref().and_then(|l| ShareTab::ALL.iter().flat_map(|t| l.of(*t)).find(|s| s.id == id)).cloned() else { return false };
        p.tab = ShareTab::ALL[kind_index(s.kind)];
        Self::select(&mut *self.share.backend, p, s);
        true
    }

    /// Ce qui a été validé par « Partager », une fois (câblage : lot M5).
    pub fn take_share_choice(&mut self) -> Option<ShareChoice> {
        self.share.choice.take()
    }

    fn select(backend: &mut dyn CaptureService, p: &mut Picker, s: Source) {
        if p.selected.as_ref() == Some(&s) {
            return;
        }
        if !sound_allowed(s.kind) {
            p.audio = false;
        }
        backend.preview(&s);
        p.preview = Some((s.id.clone(), Preview::Loading));
        p.selected = Some(s);
    }

    fn share_poll(&mut self, ctx: &egui::Context) {
        let Some(p) = &mut self.share.picker else { return };
        let events = self.share.backend.poll();
        if !events.is_empty() {
            // Une réponse peut en demander d'autres (vignettes après la liste).
            ctx.request_repaint();
        }
        for e in events {
            match e {
                ShareEvent::Sources(lists) => {
                    let all: Vec<Source> = lists.screens.iter().chain(&lists.windows).cloned().collect();
                    p.thumbs.retain(|id, _| all.iter().any(|s| &s.id == id));
                    self.share.backend.thumbnails(&all);
                    // Source choisie disparue (fenêtre fermée) : plus rien de choisi.
                    if let Some(sel) = &p.selected
                        && !ShareTab::ALL.iter().any(|t| lists.of(*t).contains(sel))
                    {
                        p.selected = None;
                        p.preview = None;
                    }
                    // Aperçu avant la caméra : celle des réglages, sinon la première.
                    let auto = p.auto_camera.take().and_then(|want| want.and_then(|id| lists.cameras.iter().find(|s| s.id == id)).or(lists.cameras.first()).cloned());
                    p.lists = Some(lists);
                    if let Some(s) = auto {
                        Self::select(&mut *self.share.backend, p, s);
                    }
                }
                ShareEvent::Thumbnail { id, frame } => {
                    let tex = frame.as_ref().and_then(|f| texture(ctx, &format!("partage-vignette-{id}"), f));
                    p.thumbs.insert(id, tex);
                }
                ShareEvent::Preview { id, frame } => {
                    if p.preview.as_ref().is_some_and(|(want, _)| *want == id) {
                        let shown = match frame {
                            Ok(f) => texture(ctx, "partage-apercu", &f).map_or(Preview::Failed(CaptureError::Unsupported), Preview::Ready),
                            Err(e) => Preview::Failed(e),
                        };
                        p.preview = Some((id, shown));
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
        let mut done = None;
        let mut pick = None;
        let mut refresh = false;
        let screen = ui.ctx().content_rect();
        let width = (t.layout.card_width * 2.0).min(screen.width() - 4.0 * t.spacing.card_pad).max(super::mobile::card_width(ui.ctx(), &t, 1.0));
        let Some(p) = &mut self.share.picker else { return };
        // Lu avant que le contenu n'emprunte le sélecteur : ce que « Partager » enverrait.
        let ready = matches!(p.preview, Some((_, Preview::Ready(_))));
        let camera = p.selected.as_ref().is_some_and(|s| s.kind == SourceKind::Camera);
        let (mut go, mut cancel) = (false, false);
        // Gabarit commun des fenêtres : titre et croix, contenu qui défile quand la
        // fenêtre est basse, pied toujours visible (« Partager » au bout, à droite).
        let modal = super::widgets::modal(
            ui.ctx(),
            &t,
            Id::unique("selecteur_de_partage"),
            width,
            st::TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0 * t.spacing.item_x;
                    for tab in ShareTab::ALL {
                        if super::widgets::tab(ui, &t, tab_label(lang, tab), p.tab == tab).clicked() {
                            p.tab = tab;
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if super::widgets::compact(ui, st::REFRESH.get(lang)).clicked() {
                            refresh = true;
                        }
                    });
                });
                ui.add_space(t.spacing.item_y);
                let gap = t.spacing.item_x;
                let tile_w = ((width - gap * (COLUMNS - 1) as f32) / COLUMNS as f32).floor();
                egui::ScrollArea::vertical().id_salt("partage-sources").max_height(screen.height() * 0.3).auto_shrink([false, true]).show(ui, |ui| match &p.lists {
                    None => {
                        ui.label(RichText::new(st::LOADING.get(lang)).color(t.colors.faint));
                    }
                    Some(lists) if lists.of(p.tab).is_empty() => {
                        ui.label(RichText::new(empty_label(lang, p.tab)).color(t.colors.faint));
                    }
                    Some(lists) => {
                        for row in lists.of(p.tab).chunks(COLUMNS) {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for s in row {
                                    let label = source_label(lang, lists, s);
                                    // Caméra : jamais de vignette ; sinon vignette, attente (…) ou pas d'image.
                                    let (face, tip) = match (s.kind, p.thumbs.get(&s.id)) {
                                        (SourceKind::Camera, _) => (Face::Mark(tab_label(lang, ShareTab::Cameras)), st::CAMERA_TILE_TIP.get(lang)),
                                        (_, Some(Some(tex))) => (Face::Image(tex), label.as_str()),
                                        (_, Some(None)) => (Face::Mark(tab_label(lang, p.tab)), label.as_str()),
                                        (_, None) => (Face::Mark("…"), label.as_str()),
                                    };
                                    if tile(ui, &t, &label, face, tip, p.selected.as_ref() == Some(s), tile_w) {
                                        pick = Some(s.clone());
                                    }
                                }
                            });
                            ui.add_space(t.spacing.item_y);
                        }
                    }
                });
                ui.separator();
                ui.label(RichText::new(st::PREVIEW.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                // L'aperçu suit la hauteur de la fenêtre : il ne pousse pas le pied dehors.
                let preview_h = (width * 9.0 / 16.0 * 0.6).min(screen.height() * 0.24).max(96.0).round();
                let (rect, _) = ui.allocate_exact_size(egui::vec2(width, preview_h), Sense::hover());
                ui.painter().rect_filled(rect, t.r_media(), t.colors.bg0);
                let say = |ui: &egui::Ui, text: &str, color| {
                    let galley = ui.painter().layout(text.to_string(), egui::FontId::proportional(t.typography.small), color, rect.width() - 2.0 * t.spacing.card_pad);
                    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, color);
                };
                match &p.preview {
                    None => say(ui, st::PREVIEW_NONE.get(lang), t.colors.faint),
                    Some((_, Preview::Loading)) => say(ui, st::PREVIEW_LOADING.get(lang), t.colors.faint),
                    Some((_, Preview::Ready(tex))) => paint_image(ui, rect, tex),
                    Some((_, Preview::Failed(e))) => {
                        let text = if matches!(e, CaptureError::Backend(_)) { st::PREVIEW_FAILED.get(lang).to_string() } else { error_text(lang, e) };
                        say(ui, &text, t.colors.danger);
                    }
                }
                ui.add_space(t.spacing.item_y);
                ui.horizontal(|ui| {
                    ui.label(st::QUALITY.get(lang));
                    egui::ComboBox::from_id_salt("partage_qualite").selected_text(quality_label(lang, p.quality)).show_ui(ui, |ui| {
                        for q in Quality::PRESETS {
                            ui.selectable_value(&mut p.quality, q, quality_label(lang, q));
                        }
                    });
                    let can_sound = p.selected.as_ref().is_none_or(|s| sound_allowed(s.kind));
                    ui.add_enabled(can_sound, egui::Checkbox::new(&mut p.audio, st::SOUND.get(lang))).on_hover_text(if p.selected.as_ref().is_some_and(|s| s.kind == SourceKind::Window) { st::SOUND_TIP_WINDOW } else { st::SOUND_TIP_SCREEN }.get(lang));
                });
            },
            |ui| {
                use super::widgets::{button, Kind};
                let label = if camera { st::CAMERA_ON.get(lang) } else { st::SHARE.get(lang) };
                go = ui.add_enabled_ui(ready, |ui| button(ui, &t, Kind::Primary, label)).inner.clicked();
                cancel = button(ui, &t, Kind::Link, st::CANCEL.get(lang)).clicked();
            },
        );
        if go && let Some(source) = p.selected.clone() {
            done = Some(Some(ShareChoice { audio: p.audio && sound_allowed(source.kind), source, quality: p.quality }));
        }
        if cancel || modal.closed {
            done = done.or(Some(None));
        }
        if let Some(s) = pick {
            Self::select(&mut *self.share.backend, p, s);
        }
        if refresh {
            self.share.backend.cancel();
            self.share.backend.list();
            // Les réponses à l'ancienne demande sont jetées : l'aperçu en cours aussi.
            if let Some(sel) = p.selected.clone() {
                p.selected = None;
                Self::select(&mut *self.share.backend, p, sel);
            }
        }
        if let Some(choice) = done {
            self.share.backend.cancel();
            self.share.picker = None;
            self.share.choice = choice;
        }
    }
}
