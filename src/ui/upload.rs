//! Envoi de fichiers dans la zone de saisie (lot T8) : bouton « Joindre »,
//! fichiers déposés sur la fenêtre, image collée (Ctrl+V), plateau des
//! pièces jointes au-dessus du champ (vignette, nom, taille, spoiler, texte
//! alternatif, retrait) et lignes des envois (progression, annulation,
//! échec avec « Réessayer »).
//!
//! Ni disque ni décodage ici : tout passe par `crate::attach` (fil à part)
//! et le magasin (`state::upload`). Le plateau est gardé par salon, comme les
//! brouillons, pour au plus [`MAX_TRAYS`] salons (le moins récemment touché
//! est oublié au-delà).

use super::{attachments::file_size, util::icon_button, words, App};
use crate::attach::{Attach, Staged};
use crate::i18n::table::upload::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::upload::{Failure, OutFile, Phase, Refusal, DESCRIPTION_MAX, MAX_FILES};
use crate::theme::{icons::Icon, Clock, Tokens};
use eframe::egui::{self, Id, RichText, Sense, TextureHandle, Vec2};
use std::{collections::HashMap, sync::Arc};

/// Salons dont le plateau est gardé, au plus.
pub const MAX_TRAYS: usize = 20;

struct Item {
    file: OutFile,
    thumb: Option<TextureHandle>,
}

#[derive(Default)]
struct Tray {
    items: Vec<Item>,
    /// Ordre du dernier usage (éviction).
    touched: u64,
}

#[derive(Default)]
pub(super) struct UploadUi {
    attach: Option<Attach>,
    trays: HashMap<u64, Tray>,
    clock: u64,
    /// Pièce jointe dont la fenêtre « Modifier » est ouverte : (salon, rang).
    editing: Option<(u64, usize)>,
    /// Fenêtre à ouvrir dès que la pièce existe (aperçu : les ajouts arrivent plus tard).
    wanted: Option<(u64, usize)>,
    /// Compte à qui sont les plateaux (oubliés au changement de compte).
    owner: Option<u64>,
}

impl UploadUi {
    pub(super) fn clear(&mut self) {
        self.trays.clear();
        self.editing = None;
    }

    fn tray(&mut self, channel: u64) -> &mut Tray {
        self.clock += 1;
        if !self.trays.contains_key(&channel)
            && self.trays.len() >= MAX_TRAYS
            && let Some(old) = self.trays.iter().min_by_key(|(_, t)| t.touched).map(|(c, _)| *c)
        {
            self.trays.remove(&old);
            if self.editing.is_some_and(|(c, _)| c == old) {
                self.editing = None;
            }
        }
        let clock = self.clock;
        let t = self.trays.entry(channel).or_default();
        t.touched = clock;
        t
    }
}

/// Raison d'un échec, en mots.
fn failure_text(lang: Lang, f: &Failure, h24: bool) -> String {
    match f {
        Failure::Problem(p) => f::not_sent(lang, words::problem(lang, p, h24)),
        Failure::Unknown(_) => tx::MAYBE_SENT.get(lang).to_string(),
        Failure::Unreadable(n) => f::not_sent(lang, f::unreadable(lang, n)),
        Failure::Changed(n) => f::not_sent(lang, f::changed(lang, n)),
        Failure::TooLarge => f::not_sent(lang, tx::SERVER_TOO_LARGE.get(lang)),
    }
}

pub(super) fn refusal_text(lang: Lang, r: &Refusal) -> String {
    match r {
        Refusal::TooMany => tx::TOO_MANY.get(lang).to_string(),
        Refusal::TooLarge { name, limit } => f::too_large(lang, name, file_size(lang, *limit)),
        Refusal::CheckingLimit => tx::CHECKING_LIMIT.get(lang).to_string(),
        Refusal::Busy => tx::BUSY.get(lang).to_string(),
        Refusal::Blocked => tx::BLOCKED.get(lang).to_string(),
    }
}

impl App {
    fn attach(&mut self) -> &Attach {
        let ctx = self.ctx.clone();
        self.uploads.attach.get_or_insert_with(|| Attach::new(Arc::new(move || ctx.request_repaint())))
    }

    /// Le salon a des pièces jointes en attente d'envoi.
    pub(super) fn has_attachments(&self, channel: u64) -> bool {
        self.uploads.trays.get(&channel).is_some_and(|t| !t.items.is_empty())
    }

    /// Noms des pièces jointes du salon affiché (tests et aperçu).
    pub fn attachment_names(&self) -> Vec<String> {
        let Some(c) = self.store.channel else { return Vec::new() };
        self.uploads.trays.get(&c).map(|t| t.items.iter().map(|i| i.file.sent_name()).collect()).unwrap_or_default()
    }

    /// Joint des octets au salon affiché, comme une image collée (tests et
    /// aperçu) ; la pièce apparaît quand le fil des ajouts l'a préparée.
    pub fn attach_bytes(&mut self, name: &str, bytes: Vec<u8>) {
        if let Some(c) = self.store.channel {
            let name = name.to_string();
            self.attach().add_bytes(c, name, bytes);
        }
    }

    /// Joint des fichiers du disque au salon affiché (comme un dépôt).
    pub fn attach_paths(&mut self, paths: Vec<std::path::PathBuf>) {
        if let Some(c) = self.store.channel {
            self.attach().add_paths(c, paths);
        }
    }

    /// Reprend les ajouts terminés.
    fn take_staged(&mut self) {
        let Some(a) = &self.uploads.attach else { return };
        for s in a.poll() {
            match s {
                Staged::File { channel, file, thumb } => {
                    if self.uploads.trays.get(&channel).is_some_and(|t| t.items.len() >= MAX_FILES) {
                        self.notify(tx::TOO_MANY.get(self.lang).to_string());
                        continue;
                    }
                    if file.size > self.store.upload_limit_in(channel) {
                        self.store.learn_upload_limit();
                    }
                    let thumb = thumb.map(|img| self.ctx.load_texture("piece-jointe", img, egui::TextureOptions::LINEAR));
                    self.uploads.tray(channel).items.push(Item { file, thumb });
                }
                Staged::Unreadable { name, .. } => {
                    self.notify(f::unreadable(self.lang, &name));
                }
            }
        }
        // Aperçu : la fenêtre demandée s'ouvre dès que sa pièce est dans le plateau.
        if let Some((c, i)) = self.uploads.wanted
            && self.uploads.trays.get(&c).is_some_and(|t| i < t.items.len())
        {
            self.uploads.editing = self.uploads.wanted.take();
        }
    }

    /// Fichiers déposés, image collée, ajouts terminés ; et le voile « Déposez
    /// les fichiers » pendant qu'un fichier est tiré au-dessus de la fenêtre.
    pub(super) fn upload_input(&mut self, ui: &mut egui::Ui, cid: u64, composer: Id, target: &str) {
        let me = self.store.me.as_ref().map(|u| u.id);
        if self.uploads.owner != me {
            self.uploads.clear();
            self.uploads.owner = me;
        }
        self.take_staged();
        let dropped: Vec<std::path::PathBuf> = ui.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if !dropped.is_empty() {
            self.attach().add_paths(cid, dropped);
        }
        // Collée dans la saisie, ou nulle part ailleurs (pas dans un autre champ).
        let free = ui.memory(|m| m.focused().is_none_or(|f| f == composer));
        let pasted: Vec<Arc<egui::ColorImage>> = if free {
            ui.input(|i| i.events.iter().filter_map(|e| if let egui::Event::PasteImage(img) = e { Some(img.clone()) } else { None }).collect())
        } else {
            Vec::new()
        };
        for img in pasted {
            self.attach().add_image(cid, img);
        }
        // Texte collé qui ferait dépasser la limite : il devient `message.txt` (F6).
        if ui.memory(|m| m.has_focus(composer)) {
            let room = self.composer_room();
            for text in ui.input_mut(|i| super::composer_menu::take_long_pastes(&mut i.events, room)) {
                self.attach().add_bytes(cid, super::composer_menu::PASTE_FILE.to_string(), text.into_bytes());
            }
        }
        if ui.input(|i| !i.raw.hovered_files.is_empty()) {
            let t = &self.theme;
            let screen = ui.ctx().content_rect();
            let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::unique("upload_drop")));
            painter.rect_filled(screen, 0.0, t.colors.scrim);
            let galley = painter.layout(f::drop_here(self.lang, target), egui::FontId::proportional(t.typography.title), t.colors.text_strong, screen.width() * 0.8);
            let card = egui::Rect::from_center_size(screen.center(), galley.size() + Vec2::splat(2.0 * t.spacing.card_pad));
            painter.rect(card, t.r_card(), t.colors.bg2, egui::Stroke::new(t.shapes.border.max(1.0), t.colors.accent), egui::StrokeKind::Inside);
            painter.galley(card.center() - galley.size() / 2.0, galley, t.colors.text_strong);
        }
    }

    /// Bouton « Joindre des fichiers » à gauche du champ (boîte de dialogue
    /// de Windows, ouverte hors du fil d'interface).
    /// Ctrl+Maj+U : la même fenêtre de choix que le bouton « + » (lot N).
    pub(super) fn pick_attachment(&mut self, cid: u64) {
        if !Attach::CAN_PICK || self.uploads.attach.as_ref().is_some_and(Attach::picking) {
            return;
        }
        #[cfg(windows)]
        {
            let tip = tx::ATTACH.get(self.lang);
            self.attach().pick(cid, tip);
        }
        #[cfg(not(windows))]
        let _ = cid;
    }


    /// Plateau des pièces jointes du salon, au-dessus du champ.
    pub(super) fn attachment_tray(&mut self, ui: &mut egui::Ui, cid: u64) {
        if !self.has_attachments(cid) {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let limit = self.store.upload_limit_in(cid);
        let (mut remove, mut edit) = (None, None);
        let side = t.layout.embed_thumb;
        let tray = self.uploads.tray(cid);
        egui::ScrollArea::horizontal().id_salt("upload_tray").auto_shrink([false, true]).show(ui, |ui| {
            ui.horizontal_top(|ui| {
                for (i, item) in tray.items.iter_mut().enumerate() {
                    ui.push_id(i, |ui| {
                        // Même fond, même rayon et même marge que la zone de saisie posée dessous.
                        egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(t.r_composer()).inner_margin(t.embed_margin()).show(ui, |ui| {
                            ui.set_width(side);
                            ui.vertical(|ui| tile(ui, &t, lang, item, side, limit, &mut remove, &mut edit, i));
                        });
                    });
                }
            });
        });
        ui.add_space(t.spacing.chat_pad / 2.0);
        if let Some(i) = edit {
            self.uploads.editing = Some((cid, i));
        }
        if let Some(i) = remove {
            let tray = self.uploads.tray(cid);
            if i < tray.items.len() {
                tray.items.remove(i);
            }
            self.uploads.editing = None;
        }
        self.attachment_editor(ui.ctx(), cid);
    }

    /// Ouvre la fenêtre des détails de la pièce `index` du plateau de `channel` (aperçu).
    pub fn show_attachment_details(&mut self, channel: u64, index: usize) {
        self.uploads.wanted = Some((channel, index));
    }

    /// Fenêtre « Pièce jointe » : nom et texte alternatif.
    fn attachment_editor(&mut self, ctx: &egui::Context, cid: u64) {
        let Some((c, i)) = self.uploads.editing else { return };
        let lang = self.lang;
        let Some(item) = self.uploads.trays.get_mut(&c).and_then(|t| t.items.get_mut(i)).filter(|_| c == cid) else {
            self.uploads.editing = None;
            return;
        };
        let t = self.theme.clone();
        let mut close = false;
        // Gabarit commun des fenêtres : titre et croix, champs, pied à droite.
        // Les champs écrivent directement dans la pièce jointe : rien à annuler.
        let width = crate::ui::mobile::card_width(ctx, &t, 1.0);
        let modal = crate::ui::widgets::modal(
            ctx,
            &t,
            Id::unique("upload_edit"),
            width,
            tx::EDIT_TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, &t);
                crate::ui::widgets::field_label(ui, &t, tx::FILE_NAME.get(lang));
                crate::ui::widgets::field(ui, &t, &mut item.file.name, "");
                ui.add_space(t.spacing.item_y);
                crate::ui::widgets::field_label(ui, &t, tx::ALT_TEXT.get(lang));
                ui.add(egui::TextEdit::multiline(&mut item.file.description).hint_text(tx::ALT_HINT.get(lang)).char_limit(DESCRIPTION_MAX).desired_rows(3).desired_width(f32::INFINITY));
                ui.add_space(t.spacing.item_y);
                ui.checkbox(&mut item.file.spoiler, tx::SPOILER.get(lang));
            },
            |ui| close = crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Primary, tx::CLOSE.get(lang)).clicked(),
        );
        if close || modal.closed {
            self.uploads.editing = None;
        }
    }

    /// Envois du salon (en cours ou arrêtés), au-dessus de la saisie.
    pub(super) fn upload_rows(&mut self, ui: &mut egui::Ui, cid: u64, can_send: bool) {
        let t = self.theme.clone();
        let lang = self.lang;
        let h24 = t.layout.clock == Clock::H24;
        let (mut cancel, mut retry, mut discard) = (None, None, None);
        let mut any = false;
        for u in self.store.uploads.of_channel(cid) {
            any = true;
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match &u.phase {
                    Phase::Sending => {
                        if crate::ui::widgets::compact(ui, tx::CANCEL.get(lang)).clicked() {
                            cancel = Some(u.nonce);
                        }
                    }
                    Phase::Cancelling => {}
                    Phase::Failed(_) | Phase::Cancelled => {
                        if crate::ui::widgets::compact(ui, tx::DELETE.get(lang)).clicked() {
                            discard = Some(u.nonce);
                        }
                        if can_send && crate::ui::widgets::compact(ui, tx::RETRY.get(lang)).clicked() {
                            retry = Some(u.nonce);
                        }
                    }
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    let (status, color) = match &u.phase {
                        Phase::Sending if u.sent == 0 => (tx::WAITING.get(lang).to_string(), t.colors.muted),
                        Phase::Sending => (f::progress(lang, file_size(lang, u.sent), file_size(lang, u.total)), t.colors.muted),
                        Phase::Cancelling => (tx::CANCELLING.get(lang).to_string(), t.colors.muted),
                        Phase::Failed(f @ Failure::Unknown(_)) => (failure_text(lang, f, h24), t.colors.muted),
                        Phase::Failed(f) => (failure_text(lang, f, h24), t.colors.danger),
                        Phase::Cancelled => (tx::CANCELLED.get(lang).to_string(), t.colors.muted),
                    };
                    super::util::inline_icon(ui, Icon::Paperclip, t.typography.small, t.colors.muted);
                    ui.label(RichText::new(p::files(lang, u.files.len() as u64)).small().color(t.colors.muted));
                    if u.phase == Phase::Sending && u.total > 0 {
                        let frac = (u.sent as f32 / u.total as f32).clamp(0.0, 1.0);
                        let width = (ui.available_width() / 4.0).max(ui.spacing().interact_size.x);
                        ui.add(egui::ProgressBar::new(frac).desired_width(width).desired_height(t.typography.small).fill(t.colors.accent).corner_radius(t.r_row()));
                    }
                    if super::mobile::is_active(ui.ctx()) {
                        // Disposition mobile : l'état, raccourci, ne passe plus sous les boutons ; le texte joint est omis faute de place.
                        ui.add(egui::Label::new(RichText::new(status).small().color(color)).truncate());
                        return;
                    }
                    // Coupé à la place qui reste : il ne passe jamais sous les boutons.
                    ui.add(egui::Label::new(RichText::new(status).small().color(color)).truncate());
                    if !u.content.is_empty() && ui.available_width() > ui.spacing().interact_size.x {
                        ui.add(egui::Label::new(RichText::new(&u.content).color(t.colors.text)).truncate());
                    }
                });
            });
        }
        if let Some(n) = cancel {
            self.store.cancel_upload(n);
        }
        if let Some(n) = retry {
            self.store.retry_upload(n);
        }
        if let Some(n) = discard {
            self.store.discard_upload(n);
        }
        if any {
            ui.add_space(t.spacing.chat_pad / 2.0);
        }
    }

    /// Envoie le texte et les pièces jointes du salon (réponse en cours
    /// comprise). `false` si rien ne part : la raison est affichée, tout
    /// reste dans la saisie.
    pub(super) fn send_with_files(&mut self, channel_id: u64, text: String) -> bool {
        let files: Vec<OutFile> = self.uploads.trays.get(&channel_id).map(|t| t.items.iter().map(|i| i.file.clone()).collect()).unwrap_or_default();
        let reply = self.actions.reply.as_ref().filter(|r| r.channel_id == channel_id).map(|r| (r.message_id, r.mention));
        match self.store.send_files(channel_id, text, files, reply) {
            Ok(()) => {
                self.uploads.trays.remove(&channel_id);
                self.uploads.editing = None;
                if reply.is_some() {
                    self.actions.reply = None;
                }
                true
            }
            Err(r) => {
                self.notify(refusal_text(self.lang, &r));
                false
            }
        }
    }
}

/// Une pièce jointe du plateau : vignette (ou icône), nom, taille, spoiler,
/// « Modifier » et « Retirer ».
#[allow(clippy::too_many_arguments)]
fn tile(ui: &mut egui::Ui, t: &Tokens, lang: Lang, item: &mut Item, side: f32, limit: u64, remove: &mut Option<usize>, edit: &mut Option<usize>, i: usize) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    match &item.thumb {
        Some(tex) => {
            ui.painter().add(crate::images::textured(rect, t.r_media(), tex.id()));
            if item.file.spoiler {
                ui.painter().rect_filled(rect, t.r_media(), t.colors.scrim);
            }
        }
        None => {
            ui.painter().rect_filled(rect, t.r_media(), t.colors.bg2);
            crate::theme::icons::paint(Icon::File, ui.painter(), egui::Rect::from_center_size(rect.center(), Vec2::splat(side / 2.0)), t.colors.muted);
        }
    }
    let name = item.file.sent_name();
    ui.add(egui::Label::new(RichText::new(&name).small().color(t.colors.text)).truncate()).on_hover_text(&name);
    let size = file_size(lang, item.file.size);
    if item.file.size > limit {
        let over = f::over_limit(lang, file_size(lang, limit));
        // Sur une ligne, comme la taille : toutes les vignettes ont la même hauteur.
        ui.add(egui::Label::new(RichText::new(over).small().color(t.colors.danger)).truncate()).on_hover_text(f::too_large(lang, &name, file_size(lang, limit)));
    } else {
        ui.label(RichText::new(size).small().color(t.colors.muted));
    }
    ui.horizontal(|ui| {
        ui.checkbox(&mut item.file.spoiler, RichText::new(tx::SPOILER.get(lang)).small());
    });
    ui.horizontal(|ui| {
        if icon_button(ui, t, Icon::Pencil, tx::EDIT.get(lang)).clicked() {
            *edit = Some(i);
        }
        if icon_button(ui, t, Icon::Close, tx::REMOVE.get(lang)).clicked() {
            *remove = Some(i);
        }
    });
}
