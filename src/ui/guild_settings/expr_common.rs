//! Morceaux communs aux pages Emojis, Autocollants et Sons : formulaires
//! (envoi, modification, confirmation de suppression), en-tête de page,
//! emplacements, lignes de liste.

use super::super::App;
use crate::expr_pick::Picked;
use crate::i18n::table::expressions::{f as xf, t as xt};
use crate::i18n::Lang;
use crate::state::admin::expressions::{Invalid, Kind, Loaded, SoundEmoji};
use crate::state::Store;
use crate::theme::Tokens;
use eframe::egui::{self, Align, Layout, RichText, TextureHandle};

/// Fichier choisi, prêt à envoyer, et les champs de son formulaire.
pub(super) struct Draft {
    pub kind: Kind,
    pub name: String,
    pub description: String,
    pub tags: String,
    pub volume: u8,
    pub emoji: String,
    pub picked: Picked,
    pub preview: Option<TextureHandle>,
}

/// Élément en cours de modification.
pub(super) struct Edit {
    pub kind: Kind,
    pub id: u64,
    pub name: String,
    pub description: String,
    pub tags: String,
    pub volume: u8,
    pub emoji: String,
    pub roles: Vec<u64>,
}

impl Edit {
    pub(super) fn new(kind: Kind, id: u64, name: &str) -> Self {
        Edit { kind, id, name: name.to_string(), description: String::new(), tags: String::new(), volume: 100, emoji: String::new(), roles: Vec::new() }
    }
}

/// Un seul formulaire à la fois : envoi, modification ou suppression.
#[derive(Default)]
pub(super) struct Forms {
    /// Dernier refus local (fichier, saisie) : affiché sous l'en-tête.
    pub error: Option<String>,
    pub draft: Option<Draft>,
    pub edit: Option<Edit>,
    /// Suppression demandée : genre, élément, nom.
    pub confirm: Option<(Kind, u64, String)>,
}

impl Forms {
    pub(super) fn start(&mut self, ctx: &egui::Context, kind: Kind, picked: Picked) {
        let (name, preview) = match &picked {
            Picked::Emoji { name, preview, .. } | Picked::Sticker { name, preview, .. } => (name.clone(), Some(ctx.load_texture("expression-apercu", preview.clone(), egui::TextureOptions::LINEAR))),
            Picked::Sound { name, .. } => (name.clone(), None),
        };
        *self = Forms { draft: Some(Draft { kind, name, description: String::new(), tags: String::new(), volume: 100, emoji: String::new(), picked, preview }), ..Forms::default() };
    }
}

/// Secondes avec une décimale, virgule en français.
pub(super) fn seconds(lang: Lang, ms: u64) -> String {
    let s = format!("{:.1}", ms as f64 / 1000.0);
    if lang == Lang::Fr { s.replace('.', ",") } else { s }
}

/// Pourquoi la saisie est refusée.
pub(super) fn invalid(lang: Lang, kind: Kind, i: Invalid) -> String {
    let t = match (i, kind) {
        (Invalid::Name, Kind::Emoji) => xt::BAD_EMOJI_NAME,
        (Invalid::Name, Kind::Sticker) => xt::BAD_STICKER_NAME,
        (Invalid::Name, Kind::Sound) => xt::BAD_SOUND_NAME,
        (Invalid::Description, _) => xt::BAD_DESCRIPTION,
        (Invalid::Tags, _) => xt::BAD_TAGS,
        (Invalid::NoSlot, _) => xt::NO_SLOT,
        (Invalid::Busy, _) => xt::BUSY,
        (Invalid::Forbidden, _) => xt::FORBIDDEN,
    };
    t.get(lang).to_string()
}

/// Emplacements : libellé, barre, « n sur N ».
pub(super) fn slots(ui: &mut egui::Ui, t: &Tokens, lang: Lang, label: &str, used: usize, total: usize) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).family(crate::ui::fonts::demi()).strong());
        ui.label(RichText::new(xf::slots(lang, used, total)).small().color(t.colors.muted));
    });
    let frac = if total == 0 { 1.0 } else { (used as f32 / total as f32).min(1.0) };
    let fill = if used >= total { t.colors.danger } else { t.colors.accent };
    ui.add(egui::ProgressBar::new(frac).desired_height(t.typography.small / 2.0).fill(fill).corner_radius(t.r_row()));
    ui.add_space(t.spacing.item_y);
}

/// Ligne de liste de hauteur fixe (listes virtualisées).
pub(super) fn row(ui: &mut egui::Ui, height: f32, add: impl FnOnce(&mut egui::Ui)) {
    let size = egui::vec2(ui.available_width(), height);
    ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| {
        ui.set_min_size(size);
        add(ui);
    });
}

/// Hauteur d'une ligne de liste et côté de son image.
pub(super) fn row_metrics(t: &Tokens) -> (f32, f32) {
    let side = t.markdown.jumbo;
    (side + 2.0 * t.spacing.item_y, side)
}

/// Colonne « Envoyé par » : le nom, ou rien quand l'auteur est inconnu (un
/// tiret seul au milieu de la ligne ne disait rien).
pub(super) fn author_label(ui: &mut egui::Ui, t: &Tokens, a: Option<&crate::state::admin::expressions::Author>) {
    if let Some(a) = a {
        ui.label(RichText::new(a.name.as_str()).small().color(t.colors.muted));
    }
}

/// Boutons Modifier et Supprimer d'une ligne (à droite) ; occupés : désactivés.
pub(super) enum RowAction {
    Edit,
    Delete,
}

pub(super) fn row_buttons(ui: &mut egui::Ui, t: &Tokens, lang: Lang, busy: bool) -> Option<RowAction> {
    let mut out = None;
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        if busy {
            ui.spinner();
            return;
        }
        // Disposition mobile : un seul bouton « … », les libellés de la ligne restent lisibles.
        if crate::ui::mobile::is_active(ui.ctx()) {
            crate::ui::mobile::more_menu(ui, |ui| {
                if crate::ui::menu::item(ui, t, xt::EDIT.get(lang), Some(crate::theme::icons::Icon::Pencil), false).clicked() {
                    out = Some(RowAction::Edit);
                }
                if crate::ui::menu::item(ui, t, xt::DELETE.get(lang), Some(crate::theme::icons::Icon::Trash), true).clicked() {
                    out = Some(RowAction::Delete);
                }
            });
            return;
        }
        if ui.add(crate::ui::form::danger(t, xt::DELETE.get(lang))).clicked() {
            out = Some(RowAction::Delete);
        }
        if ui.add(crate::ui::form::btn(t, xt::EDIT.get(lang))).clicked() {
            out = Some(RowAction::Edit);
        }
    });
    out
}

/// Emoji d'un son saisi : vide, emoji Unicode, ou `:nom:` d'un emoji du
/// serveur. `Err` : inconnu.
pub(super) fn sound_emoji(store: &Store, guild: u64, text: &str) -> Result<Option<SoundEmoji>, ()> {
    let s = text.trim();
    if s.is_empty() {
        return Ok(None);
    }
    if crate::state::emoji::find(s).is_some() {
        return Ok(Some(SoundEmoji::Unicode(s.to_string())));
    }
    let name = s.trim_matches(':');
    let g = store.guilds.iter().find(|g| g.id == guild).ok_or(())?;
    g.emojis.iter().find(|e| e.name == name).map(|e| Some(SoundEmoji::Custom(e.id))).ok_or(())
}

/// Texte d'un emoji de son, pour le champ de saisie.
pub(super) fn sound_emoji_text(store: &Store, guild: u64, e: Option<&SoundEmoji>) -> String {
    match e {
        None => String::new(),
        Some(SoundEmoji::Unicode(s)) => s.clone(),
        Some(SoundEmoji::Custom(id)) => {
            let name = store.guilds.iter().find(|g| g.id == guild).and_then(|g| g.emojis.iter().find(|e| e.id == *id)).map(|e| e.name.clone());
            name.map_or_else(String::new, |n| format!(":{n}:"))
        }
    }
}

/// Champ de texte d'un formulaire, avec son libellé.
pub(super) fn field(ui: &mut egui::Ui, t: &Tokens, label: &str, value: &mut String, hint: &str, max: usize) {
    crate::ui::widgets::field_label(ui, t, label);
    crate::ui::form::field(ui, t, value, hint, max);
}

impl App {
    /// En-tête d'une page : bouton d'envoi (ou lecture du fichier en cours),
    /// contraintes, état de la liste, dernier refus. `can_add` : il reste
    /// un emplacement.
    pub(super) fn expr_header(&mut self, ui: &mut egui::Ui, t: &Tokens, g: u64, kind: Kind, can_add: bool) {
        let lang = self.lang;
        let (upload, limits) = match kind {
            Kind::Emoji => (xt::UPLOAD_EMOJI, xt::EMOJI_LIMITS),
            Kind::Sticker => (xt::UPLOAD_STICKER, xt::STICKER_LIMITS),
            Kind::Sound => (xt::UPLOAD_SOUND, xt::SOUND_LIMITS),
        };
        super::super::settings::note(ui, t, limits.get(lang));
        ui.add_space(t.spacing.item_y);
        ui.horizontal(|ui| {
            if self.picker_busy() {
                ui.spinner();
                ui.label(RichText::new(xt::READING_FILE.get(lang)).color(t.colors.muted));
            } else if !crate::expr_pick::ExprPicker::CAN_PICK {
                ui.label(RichText::new(xt::NO_DIALOG.get(lang)).color(t.colors.muted));
            } else {
                let creating = self.store.expressions.creating(g, kind);
                if ui.add_enabled(can_add && !creating, crate::ui::form::btn(t, upload.get(lang))).clicked() {
                    self.pick_expression(kind, upload.get(lang));
                }
                if creating {
                    ui.spinner();
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let loading = matches!(self.store.expressions.loaded(g, kind), Some(Loaded::Loading));
                if loading {
                    ui.spinner();
                } else if ui.add(crate::ui::form::btn(t, xt::REFRESH.get(lang))).clicked() {
                    self.store.load_expressions(g, kind);
                }
            });
        });
        if !can_add {
            ui.label(RichText::new(xt::NO_SLOT.get(lang)).color(t.colors.muted));
        }
        if let Some(Loaded::Failed(f)) = self.store.expressions.loaded(g, kind) {
            let text = xf::failed(lang, xt::ACTION_LOAD.get(lang), self.failure(f));
            ui.label(RichText::new(text).color(t.colors.danger));
        }
        if let Some(e) = &self.guild_settings.form.error {
            ui.label(RichText::new(e.as_str()).color(t.colors.danger));
        }
        ui.add_space(t.spacing.item_y);
    }

    /// Confirmation d'une suppression demandée (au-dessus de la liste).
    pub(super) fn expr_confirm(&mut self, ui: &mut egui::Ui, t: &Tokens, g: u64) {
        let lang = self.lang;
        let Some((kind, id, name)) = self.guild_settings.form.confirm.clone() else { return };
        egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_card()).inner_margin(t.spacing.item_x).show(ui, |ui| {
            ui.label(xf::delete_question(lang, &name));
            ui.horizontal(|ui| {
                if ui.add(crate::ui::form::danger(t, xt::CONFIRM_DELETE.get(lang))).clicked() {
                    self.guild_settings.form.confirm = None;
                    if let Err(e) = self.store.delete_expression(g, kind, id) {
                        self.guild_settings.form.error = Some(invalid(lang, kind, e));
                    }
                }
                if ui.add(crate::ui::form::btn(t, xt::CANCEL.get(lang))).clicked() {
                    self.guild_settings.form.confirm = None;
                }
            });
        });
        ui.add_space(t.spacing.item_y);
    }

    /// Ligne d'action demandée : formulaire de modification ou confirmation.
    pub(super) fn expr_row_action(&mut self, action: RowAction, edit: Edit) {
        let f = &mut self.guild_settings.form;
        f.error = None;
        f.draft = None;
        match action {
            RowAction::Edit => {
                f.confirm = None;
                f.edit = Some(edit);
            }
            RowAction::Delete => {
                f.edit = None;
                f.confirm = Some((edit.kind, edit.id, edit.name));
            }
        }
    }
}

// ---------------------------------------------------------------- fenêtre (lot A6)
// Morceaux de la fenêtre propres aux expressions, appelés par `mod.rs`.

impl App {
    /// Fichier déjà en mémoire, comme s'il venait de la boîte « Ouvrir » (aperçu).
    pub fn pick_expression_bytes(&mut self, page: super::Page, name: &str, bytes: Vec<u8>) {
        let Some(kind) = page.kind() else { return };
        let wake = self.repaint_wake();
        self.guild_settings.picker.get_or_insert_with(|| crate::expr_pick::ExprPicker::new(wake)).from_bytes(kind, name.to_string(), bytes);
    }

    fn repaint_wake(&self) -> crate::attach::Wake {
        let ctx = self.ctx.clone();
        std::sync::Arc::new(move || ctx.request_repaint())
    }

    /// Messages des actions finies (toujours lus, fenêtre ouverte ou non).
    pub(super) fn expr_notices(&mut self) {
        while let Some(n) = self.store.expressions.take_notice() {
            if let Some(text) = self.expr_notice(&n) {
                self.notify(text);
            }
        }
    }

    /// Fichier lu par le fil des choix.
    pub(super) fn expr_poll(&mut self, ctx: &egui::Context) {
        if let Some((kind, out)) = self.guild_settings.picker.as_ref().and_then(crate::expr_pick::ExprPicker::poll) {
            self.picked(ctx, kind, out);
        }
    }

    /// Fichier lu par le fil des choix : formulaire d'envoi, ou message.
    fn picked(&mut self, ctx: &egui::Context, kind: Kind, out: Result<Picked, crate::expr_pick::PickError>) {
        use crate::expr_pick::PickError;
        let lang = self.lang;
        match out {
            Ok(p) => self.guild_settings.form.start(ctx, kind, p),
            Err(e) => {
                let text = match e {
                    PickError::Unreadable => xt::UNREADABLE.get(lang).to_string(),
                    PickError::TooBig => xt::TOO_BIG.get(lang).to_string(),
                    PickError::TooLong(ms) => xf::too_long(lang, seconds(lang, ms)),
                };
                self.guild_settings.form.error = Some(text);
            }
        }
    }

    /// Bouton « Envoyer … » : boîte « Ouvrir » (fil à part).
    pub(super) fn pick_expression(&mut self, kind: Kind, title: &str) {
        let wake = self.repaint_wake();
        self.guild_settings.form.error = None;
        #[cfg(windows)]
        self.guild_settings.picker.get_or_insert_with(|| crate::expr_pick::ExprPicker::new(wake)).pick(kind, title);
        #[cfg(not(windows))]
        let _ = (wake, kind, title);
    }

    pub(super) fn picker_busy(&self) -> bool {
        self.guild_settings.picker.as_ref().is_some_and(crate::expr_pick::ExprPicker::busy)
    }

    pub(super) fn expr_notice(&self, n: &crate::state::admin::expressions::ExprNotice) -> Option<String> {
        use crate::state::admin::expressions::{ExprNotice, Failure, Op};
        let lang = self.lang;
        Some(match n {
            ExprNotice::Done(_, Op::Load) => return None,
            ExprNotice::Done(_, Op::Create) => xt::DONE_CREATE.get(lang).into(),
            ExprNotice::Done(_, Op::Edit) => xt::DONE_EDIT.get(lang).into(),
            ExprNotice::Done(_, Op::Delete) => xt::DONE_DELETE.get(lang).into(),
            // L'A2F se dit seule (même message que les autres pages) : elle dit déjà quoi faire.
            ExprNotice::Failed(_, _, Failure::Problem(crate::state::Problem::MfaRequired)) => crate::i18n::table::admin::t::MFA_REQUIRED.get(lang).into(),
            ExprNotice::Failed(_, op, f) => {
                let action = match op {
                    Op::Load => xt::ACTION_LOAD,
                    Op::Create => xt::ACTION_CREATE,
                    Op::Edit => xt::ACTION_EDIT,
                    Op::Delete => xt::ACTION_DELETE,
                };
                xf::failed(lang, action.get(lang), self.failure(f))
            }
        })
    }

    pub(super) fn failure(&self, f: &crate::state::admin::expressions::Failure) -> String {
        use crate::state::admin::expressions::Failure;
        match f {
            Failure::Problem(p) => super::super::words::problem(self.lang, p, self.h24()),
            Failure::Captcha => xt::CAPTCHA.get(self.lang).into(),
            Failure::NotFound => xt::NOT_FOUND.get(self.lang).into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_follow_the_language() {
        assert_eq!(seconds(Lang::Fr, 2612), "2,6");
        assert_eq!(seconds(Lang::En, 5200), "5.2");
    }
}
