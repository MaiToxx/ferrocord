//! Page « Apparence » de la fenêtre Paramètres (Ctrl+, ou la roue dentée)
//! et suivi du thème en cours d'édition. Chaque réglage s'applique à
//! l'image suivante ; l'enregistrement dans `theme.toml` est confié au fil
//! de thème, qui le diffère et le regroupe.
//!
//! En haut, une courte section « Essentiel » (langue comprise) ; tout le
//! reste est rangé, replié, sous « Avancé ».
//!
//! Libellés, explications et noms des choix viennent de `i18n`.

use super::App;
use crate::i18n::{self, f, t, Choice, Lang};
use crate::theme::{
    contrast_ratio, Field, FieldMut, Preset, ThemeLink, Tokens, Update, CHANNELS_WIDTH, MEMBERS_WIDTH, ESSENTIALS, MIN_MUTED, MIN_TEXT,
    SECTIONS, ZOOM,
};
use eframe::egui::{self, color_picker::Alpha, Align, Color32, Layout, RichText, Sense};

pub(super) struct Appearance {
    link: ThemeLink,
    /// Thème modifié par le panneau ou lu dans le fichier.
    pub edited: Tokens,
    /// Dernier thème confié au fil d'écriture (ou lu sur disque).
    saved: Tokens,
    /// `theme.toml` est refusé : message affiché, rien n'est enregistré.
    file_error: Option<String>,
    error: Option<String>,
    /// Information affichée en bas du panneau ; `None` dans `Some` : « thème
    /// importé », écrit dans la langue courante à l'affichage.
    info: Option<Option<String>>,
    share_path: String,
    /// Zoom qu'on a donné à egui, pour reconnaître un zoom fait au clavier.
    applied_zoom: f32,
    /// Zoom d'egui au moment d'une demande pas encore effective.
    zoom_pending: Option<f32>,
    /// Largeur du panneau des salons à l'image précédente.
    pub channels_shown: f32,
    /// Largeur de la liste des membres à l'image précédente.
    pub members_shown: f32,
}

impl Appearance {
    /// `first` : résultat de la première lecture du fichier (faite avant la
    /// fenêtre). Renvoie aussi un message à notifier (fichier refusé).
    pub fn new(link: ThemeLink, first: Option<Update>) -> (Self, Option<String>) {
        let edited = Tokens::default();
        let share_path = link.share_path().display().to_string();
        let mut a = Self {
            link,
            saved: edited.clone(),
            applied_zoom: edited.layout.zoom,
            zoom_pending: None,
            channels_shown: edited.layout.channels_width,
            members_shown: edited.layout.members_width,
            edited,
            file_error: None,
            error: None,
            info: None,
            share_path,
        };
        let notice = first.and_then(|u| a.handle(u));
        a.channels_shown = a.edited.layout.channels_width;
        a.members_shown = a.edited.layout.members_width;
        (a, notice)
    }

    /// Applique les nouvelles du fil de thème ; renvoie un message à notifier.
    pub fn poll(&mut self) -> Option<String> {
        let mut notify = None;
        for u in self.link.poll() {
            if let Some(m) = self.handle(u) {
                notify = Some(m);
            }
        }
        notify
    }

    fn handle(&mut self, u: Update) -> Option<String> {
        match u {
            Update::Loaded(t) => {
                self.edited = t.clone();
                self.saved = t;
                self.file_error = None;
                self.error = None;
                None
            }
            Update::FileError(e) => {
                self.file_error = Some(e.clone());
                self.info = None;
                Some(e)
            }
            Update::Imported(t) => {
                // `saved` reste l'ancien : le thème importé sera enregistré.
                self.edited = t;
                self.error = None;
                self.info = Some(None);
                None
            }
            Update::Error(e) => {
                self.info = None;
                self.error = Some(e.clone());
                Some(e)
            }
            Update::Info(i) => {
                self.error = None;
                self.info = Some(Some(i));
                None
            }
            Update::Saved(_) => None,
        }
    }

    /// Dossier de `theme.toml`, s'il peut être écrit : la bibliothèque des
    /// thèmes et des profils enregistrés s'y range (lot PERSO).
    pub fn folder(&self) -> Option<std::path::PathBuf> {
        self.link.path().filter(|_| self.link.writable()).and_then(|p| p.parent()).map(Into::into)
    }

    pub fn replace(&mut self, t: Tokens) {
        self.edited = t.clone();
        self.saved = t;
    }

    /// Confie au fil de thème tout changement pas encore enregistré, ramené
    /// dans ses bornes (un fichier écrit est toujours relu sans erreur).
    /// Rien n'est enregistré tant que le fichier est refusé.
    pub fn persist(&mut self) {
        if self.edited == self.saved || self.file_error.is_some() {
            return;
        }
        let t = self.edited.clone().sanitized();
        self.link.save(&t);
        self.edited = t.clone();
        self.saved = t;
    }

    /// Zoom changé au clavier (Ctrl + / Ctrl -) : il devient un réglage du
    /// thème, ramené dans ses bornes, et egui reçoit la valeur bornée.
    pub fn adopt_zoom(&mut self, ctx: &egui::Context) {
        let z = ctx.zoom_factor();
        if let Some(before) = self.zoom_pending {
            // Un zoom demandé n'est pris en compte qu'au début du passage suivant.
            if z == before {
                return;
            }
            self.zoom_pending = None;
        }
        if z == self.applied_zoom {
            return;
        }
        let bounded = ZOOM.clamp(z);
        self.edited.layout.zoom = bounded;
        self.request_zoom(ctx, bounded);
    }

    /// Donne `zoom` à egui (effectif au passage suivant).
    pub fn request_zoom(&mut self, ctx: &egui::Context, zoom: f32) {
        let now = ctx.zoom_factor();
        if now != zoom {
            self.zoom_pending = Some(now);
            ctx.set_zoom_factor(zoom);
        }
        self.applied_zoom = zoom;
    }

    /// Le panneau des salons a été élargi à la souris : c'est un réglage.
    pub fn adopt_channels_width(&mut self, width: f32) {
        let w = CHANNELS_WIDTH.clamp(width.round());
        if w != self.channels_shown {
            self.channels_shown = w;
            self.edited.layout.channels_width = w;
        }
    }

    /// La liste des membres a été élargie à la souris : c'est un réglage.
    pub fn adopt_members_width(&mut self, width: f32) {
        let w = MEMBERS_WIDTH.clamp(width.round());
        if w != self.members_shown {
            self.members_shown = w;
            self.edited.layout.members_width = w;
        }
    }
}

impl App {
    /// Page « Apparence » de la fenêtre Paramètres (Ctrl+, l'ouvre directement).
    pub(super) fn appearance_contents(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let look = &mut self.look;

        if let Some(e) = &look.file_error {
            let frame = egui::Frame::new().stroke(egui::Stroke::new(t.shapes.unread_ring, t.colors.danger)).corner_radius(t.r_embed()).inner_margin(t.embed_margin());
            frame.show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(egui::Label::new(RichText::new(e).color(t.colors.danger)).wrap());
                let reset = ui.button(t::RESET_FILE.get(lang)).on_hover_text(t::RESET_FILE_TIP.get(lang));
                if reset.clicked() {
                    look.link.reset_file(&look.edited.clone().sanitized());
                }
            });
            ui.add_space(t.spacing.item_y);
        }

        // Éditeur de thème, dégradé, fond, polices, profils (lot PERSO).
        self.look_page(ui, t);
        ui.add_space(t.spacing.category_gap);
        let lang = self.lang;
        let look = &mut self.look;

        let mut preset = None;
        ui.horizontal(|ui| {
            heading(ui, t, t::ESSENTIALS.get(lang));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if crate::ui::widgets::compact(ui, t::RESET.get(lang)).on_hover_text(t::RESET_TIP.get(lang)).clicked() {
                    preset = Some(look.edited.base);
                }
            });
        });
        // Langue : changée à la volée, pour toute l'interface.
        let mut language = None;
        let current = i18n::choice();
        row(ui, t::LANGUAGE.get(lang), t::LANGUAGE_DETAIL.get(lang), |ui| {
            egui::ComboBox::from_id_salt("langue").selected_text(current.label(lang)).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for c in Choice::ALL {
                    if ui.selectable_label(current == c, c.label(lang)).clicked() {
                        language = Some(c);
                    }
                }
            });
        });
        if let Some(c) = language {
            i18n::set_language(c);
        }
        row(ui, t::PRESET.get(lang), t::PRESET_TIP.get(lang), |ui| {
            egui::ComboBox::from_id_salt("preset").selected_text(look.edited.base.label(lang)).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for p in Preset::ALL {
                    if ui.selectable_label(look.edited.base == p, p.label(lang)).clicked() {
                        preset = Some(p);
                    }
                }
            });
        });
        if let Some(p) = preset {
            look.edited = p.tokens();
        }

        {
            let mut fields = look.edited.fields_mut();
            for (section, key) in ESSENTIALS {
                if let Some(f) = fields.iter_mut().find(|f| f.section == *section && f.key == *key) {
                    field_row(ui, lang, f);
                }
            }
        }
        contrast_warnings(ui, t, lang, &look.edited);

        ui.add_space(t.spacing.category_gap);
        heading(ui, t, t::ADVANCED.get(lang));
        {
            let mut fields = look.edited.fields_mut();
            for (section, title) in SECTIONS {
                egui::CollapsingHeader::new(title.get(lang)).id_salt(*section).default_open(false).show(ui, |ui| {
                    for f in fields.iter_mut().filter(|f| f.section == *section && !is_essential(f)) {
                        field_row(ui, lang, f);
                    }
                });
            }
        }

        egui::CollapsingHeader::new(t::SHARE_THEME.get(lang)).id_salt("partage").default_open(false).show(ui, |ui| {
            ui.add(egui::TextEdit::singleline(&mut look.share_path).desired_width(f32::INFINITY));
            ui.horizontal(|ui| {
                if ui.button(t::IMPORT.get(lang)).on_hover_text(t::IMPORT_TIP.get(lang)).clicked() {
                    look.link.import(look.share_path.trim().into());
                }
                if ui.button(t::EXPORT.get(lang)).on_hover_text(t::EXPORT_TIP.get(lang)).clicked() {
                    look.link.export(&look.edited.clone().sanitized(), look.share_path.trim().into());
                }
            });
        });

        ui.add_space(t.spacing.item_y);
        if let Some(e) = &look.error {
            ui.add(egui::Label::new(RichText::new(e).color(t.colors.danger)).wrap());
        } else if let Some(i) = &look.info {
            let i = i.as_deref().unwrap_or(t::THEME_IMPORTED.get(lang));
            ui.add(egui::Label::new(RichText::new(i).color(t.colors.muted)).wrap());
        }
        let note = match look.link.path() {
            Some(p) if look.link.writable() => f::saved_in(lang, p.display()),
            Some(p) => f::read_from(lang, p.display()),
            None => t::PREVIEW_NOTHING_SAVED.get(lang).to_string(),
        };
        ui.add(egui::Label::new(RichText::new(note).small().color(t.colors.faint)).wrap());
    }
}

fn is_essential(f: &Field<'_>) -> bool {
    ESSENTIALS.iter().any(|(s, k)| *s == f.section && *k == f.key)
}

pub(super) fn heading(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.label(RichText::new(text).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
}

/// Une ligne du panneau : libellé court à gauche (détail en infobulle),
/// réglage à droite.
///
/// La ligne occupe exactement la largeur disponible : son contenu ne peut
/// pas élargir le volet (sinon il grandirait d'image en image).
pub(super) fn row(ui: &mut egui::Ui, label: &str, detail: &str, add: impl FnOnce(&mut egui::Ui)) {
    // Un bouton ou une liste déroulante tient en entier dans la ligne, qui la découpe.
    let height = ui.spacing().interact_size.y.max(ui.text_style_height(&egui::TextStyle::Button).max(ui.text_style_height(&egui::TextStyle::Body)) + 2.0 * ui.spacing().button_padding.y);
    // Disposition mobile : le libellé, en entier, au-dessus du réglage, qui a
    // toute la largeur (deux colonnes coupent l'un ou l'autre sur un téléphone) ;
    // un réglage encore trop large (curseur, valeur, bouton) passe à la ligne.
    if super::mobile::is_active(ui.ctx()) {
        ui.add(egui::Label::new(label).wrap()).on_hover_text(detail);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
        let mut value = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(Layout::left_to_right(Align::Center).with_main_wrap(true)));
        value.set_clip_rect(rect.with_max_y(ui.clip_rect().bottom()).intersect(ui.clip_rect()));
        add(&mut value);
        let extra = value.min_rect().bottom() - rect.bottom();
        if extra > 0.0 {
            ui.allocate_space(egui::vec2(0.0, extra));
        }
        return;
    }
    super::settings::shell::settings_row(ui, label, detail, false, add);
}

fn field_row(ui: &mut egui::Ui, lang: Lang, f: &mut Field<'_>) {
    let key = f.key;
    let name = f.text.label(lang);
    row(ui, name, f.text.detail(lang), |ui| field_ui(ui, lang, key, name, &mut f.value));
}

/// `name` : libellé de la ligne, nom des réglages pour les lecteurs d'écran (lot C5).
fn field_ui(ui: &mut egui::Ui, lang: Lang, key: &str, name: &str, value: &mut FieldMut<'_>) {
    match value {
        FieldMut::Color(c) => {
            color_button(ui, c, name);
        }
        FieldMut::Palette(p) => {
            ui.horizontal_wrapped(|ui| {
                for c in p.iter_mut() {
                    color_button(ui, c, name);
                }
            });
        }
        FieldMut::Number(v, b) => {
            // Curseur à la largeur restante, la case de valeur comprise.
            let (value_box, gap) = (ui.spacing().interact_size.x + 2.0 * ui.spacing().button_padding.x, ui.spacing().item_spacing.x);
            ui.spacing_mut().slider_width = (ui.available_width() - value_box - gap).max(value_box);
            let r = ui.add(egui::Slider::new(*v, b.min..=b.max).step_by(f64::from(b.step)).fixed_decimals(b.decimals()));
            super::a11y::name(ui.ctx(), r.id, name);
        }
        FieldMut::Choice(c) => {
            let opts = c.options();
            let current = opts.get(c.index()).map(|(_, l)| l.get(lang)).unwrap_or_default();
            egui::ComboBox::from_id_salt(key).selected_text(current).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for (i, (_, label)) in opts.iter().enumerate() {
                    if ui.selectable_label(c.index() == i, label.get(lang)).clicked() {
                        c.set_index(i);
                    }
                }
            });
        }
    }
}

/// Le sélecteur passe par une conversion HSVA : on ne recopie la couleur
/// que si l'utilisateur l'a vraiment changée.
pub(super) fn color_button(ui: &mut egui::Ui, c: &mut Color32, name: &str) {
    let mut copy = *c;
    let alpha = if c.a() == u8::MAX { Alpha::Opaque } else { Alpha::OnlyBlend };
    let r = egui::color_picker::color_edit_button_srgba(ui, &mut copy, alpha);
    super::a11y::name(ui.ctx(), r.id, name);
    if r.changed() {
        *c = copy;
    }
}

/// Signale les combinaisons peu lisibles créées dans le panneau.
fn contrast_warnings(ui: &mut egui::Ui, t: &Tokens, lang: Lang, e: &Tokens) {
    let c = &e.colors;
    let checks = [
        (t::CONTRAST_TEXT, c.text, MIN_TEXT),
        (t::CONTRAST_MUTED, c.muted, MIN_MUTED),
        (t::CONTRAST_FAINT, c.faint, MIN_MUTED),
    ];
    for (name, fg, min) in checks {
        let r = contrast_ratio(fg, c.bg2);
        if r < min {
            let msg = f::low_contrast(lang, name.get(lang), i18n::decimal(lang, r, 1), i18n::decimal(lang, min, 1));
            ui.add(egui::Label::new(RichText::new(msg).color(t.colors.danger)).wrap());
        }
    }
}
