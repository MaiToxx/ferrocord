//! Envoi d'un avatar ou d'une bannière (page Profils) : choix de l'image,
//! recadrage à la souris et au curseur d'agrandissement, envoi, retrait.
//! Le décodage et le codage se font dans le fil de `crate::avatar`.

use super::{note, section, App};
use crate::avatar::{Avatar, Done};
use crate::i18n::table::s::t as st;
use crate::i18n::table::s2::{f as sf2, t as s2};
use crate::state::crop::{Crop, ImageKind, MAX_ZOOM};
use crate::state::usersettings::extra::{ImageOutcome, ImageTarget};
use crate::theme::Tokens;
use eframe::egui::{self, RichText, Sense};
use image::DynamicImage;
use std::sync::Arc;

/// Image choisie, en cours de recadrage.
pub(in crate::ui) struct Picked {
    pub(in crate::ui) image: Arc<DynamicImage>,
    pub(in crate::ui) texture: egui::TextureHandle,
    pub(in crate::ui) crop: Crop,
}

#[derive(Default)]
pub(in crate::ui) struct AvatarDraft {
    worker: Option<Avatar>,
    kind: Option<ImageKind>,
    guild: Option<u64>,
    picked: Option<Picked>,
    /// Codage demandé : son résultat part chez Discord (sinon il est jeté).
    encoding: bool,
    error: Option<String>,
}

impl AvatarDraft {
    fn target(&self) -> ImageTarget {
        ImageTarget { kind: self.kind.unwrap_or(ImageKind::Avatar), guild: self.guild }
    }
}

/// Largeur maximale de l'aperçu de recadrage (points).
const PREVIEW_WIDTH: f32 = 320.0;

impl App {
    /// Image déjà en mémoire mise dans le recadrage de l'avatar (aperçu, tests).
    pub fn stage_avatar_image(&mut self, bytes: Vec<u8>) {
        self.params.avatar.kind = Some(ImageKind::Avatar);
        self.avatar_worker().add_bytes(ImageKind::Avatar, "apercu.png".into(), bytes);
    }

    fn avatar_worker(&mut self) -> &Avatar {
        let ctx = self.ctx.clone();
        self.params.avatar.worker.get_or_insert_with(|| Avatar::new(Arc::new(move || ctx.request_repaint())))
    }

    /// Résultats du fil : image décodée, refusée, ou prête à envoyer.
    fn avatar_poll(&mut self) {
        let Some(w) = &self.params.avatar.worker else { return };
        for done in w.poll() {
            let d = &mut self.params.avatar;
            match done {
                Done::Picked { kind, image, preview } => {
                    if d.kind.unwrap_or(ImageKind::Avatar) == kind {
                        let texture = self.ctx.load_texture("ferrocord.avatar.apercu", preview, egui::TextureOptions::LINEAR);
                        d.picked = Some(Picked { image, texture, crop: Crop::default() });
                        d.error = None;
                    }
                }
                Done::Unreadable { name, .. } => d.error = Some(sf2::image_unreadable(self.lang, name)),
                Done::Encoded { kind, guild, data } => {
                    let target = ImageTarget { kind, guild };
                    if d.encoding && d.target() == target {
                        d.encoding = false;
                        d.picked = None;
                        if let Err(r) = self.store.account.extra.set_image(target, Some(data)) {
                            self.refused(r);
                        }
                    }
                }
            }
        }
    }

    pub(super) fn avatar_editor(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        self.avatar_poll();
        section(ui, t, st::AVATAR_BANNER.get(lang));
        note(ui, t, s2::IMAGE_NITRO.get(lang));
        let guilds: Vec<(u64, String)> = self.store.guilds.iter().map(|g| (g.id, super::super::words::guild_name(lang, &g.name).to_string())).collect();
        let d = &mut self.params.avatar;
        let mut kind = d.kind.unwrap_or(ImageKind::Avatar);
        ui.horizontal(|ui| {
            // De l'air entre les deux choix (même écart que ceux de l'élagage).
            ui.spacing_mut().item_spacing.x = 2.0 * self.theme.spacing.item_x;
            ui.radio_value(&mut kind, ImageKind::Avatar, s2::AVATAR.get(lang));
            ui.radio_value(&mut kind, ImageKind::Banner, s2::BANNER.get(lang));
        });
        let mut guild = d.guild;
        super::super::appearance::row(ui, s2::IMAGE_TARGET.get(lang), s2::IMAGE_TARGET_TIP.get(lang), |ui| {
            let shown = guild.and_then(|g| guilds.iter().find(|x| x.0 == g)).map_or(s2::TARGET_ACCOUNT.get(lang), |x| x.1.as_str());
            egui::ComboBox::from_id_salt("image_cible").selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                ui.selectable_value(&mut guild, None, s2::TARGET_ACCOUNT.get(lang));
                for (id, name) in &guilds {
                    ui.selectable_value(&mut guild, Some(*id), name);
                }
            });
        });
        // Autre image ou autre profil : le recadrage en cours est abandonné.
        if Some(kind) != d.kind || guild != d.guild {
            d.kind = Some(kind);
            d.guild = guild;
            d.picked = None;
            d.encoding = false;
        }
        let target = d.target();
        let saving = self.store.account.extra.image_saving.is_some();
        let picking = d.worker.as_ref().is_some_and(Avatar::picking);
        let (mut choose, mut remove) = (false, false);
        ui.horizontal(|ui| {
            choose = ui.add_enabled(Avatar::CAN_PICK && !picking && !saving, egui::Button::new(s2::CHOOSE_IMAGE.get(lang))).clicked();
            remove = ui.add_enabled(!saving, egui::Button::new(s2::REMOVE_IMAGE.get(lang))).clicked();
            if saving {
                ui.label(RichText::new(st::SAVING.get(lang)).color(t.colors.muted));
            } else if let Some((done_for, outcome)) = self.store.account.extra.image_outcome
                && done_for == target
            {
                let (text, color) = match outcome {
                    ImageOutcome::Sent => (s2::IMAGE_SENT, t.colors.muted),
                    ImageOutcome::Removed => (s2::IMAGE_REMOVED, t.colors.muted),
                    ImageOutcome::Failed => (s2::IMAGE_FAILED, t.colors.danger),
                };
                ui.label(RichText::new(text.get(lang)).color(color));
            }
        });
        if let Some(e) = &self.params.avatar.error {
            ui.add(egui::Label::new(RichText::new(e).color(t.colors.danger)).wrap());
        }
        if choose {
            let title = s2::PICK_TITLE.get(lang);
            self.avatar_worker().pick(kind, title);
        }
        if remove {
            self.params.avatar.picked = None;
            self.params.avatar.encoding = false;
            if let Err(r) = self.store.account.extra.set_image(target, None) {
                self.refused(r);
            }
        }
        let d = &mut self.params.avatar;
        let Some(p) = &mut d.picked else { return };
        note(ui, t, s2::CROP_TIP.get(lang));
        crop_view(ui, t, p, kind);
        super::super::appearance::row(ui, s2::ZOOM.get(lang), "", |ui| {
            let r = ui.add(egui::Slider::new(&mut p.crop.zoom, 1.0..=MAX_ZOOM).fixed_decimals(1));
            super::super::a11y::name(ui.ctx(), r.id, s2::ZOOM.get(lang));
        });
        let (mut send, mut cancel) = (false, false);
        ui.horizontal(|ui| {
            send = ui.add_enabled(!d.encoding && !saving, egui::Button::new(s2::SEND_IMAGE.get(lang))).clicked();
            cancel = ui.button(s2::CANCEL_IMAGE.get(lang)).clicked();
            if d.encoding {
                ui.label(RichText::new(s2::IMAGE_PREPARING.get(lang)).color(t.colors.muted));
            }
        });
        if cancel {
            d.picked = None;
            d.encoding = false;
        } else if send {
            d.encoding = true;
            let (image, crop) = (p.image.clone(), p.crop);
            self.avatar_worker().encode(image, crop, kind, guild);
        }
    }
}

/// Aperçu de l'image, cadre gardé en clair et le reste assombri ; glisser
/// déplace le cadre.
pub(in crate::ui) fn crop_view(ui: &mut egui::Ui, t: &Tokens, p: &mut Picked, kind: ImageKind) {
    let [tw, th] = p.texture.size();
    let (iw, ih) = (p.image.width(), p.image.height());
    let width = ui.available_width().min(PREVIEW_WIDTH);
    let scale = width / tw.max(1) as f32;
    let size = egui::vec2(width, th as f32 * scale);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::drag());
    if resp.dragged() {
        // Points de l'aperçu -> pixels de l'image d'origine ; l'image suit la souris.
        let k = iw as f32 / size.x.max(1.0);
        let delta = resp.drag_delta();
        p.crop.pan(-delta.x * k, -delta.y * k, iw, ih, kind);
    }
    let painter = ui.painter_at(rect);
    painter.image(p.texture.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), egui::Color32::WHITE);
    let r = p.crop.rect(iw, ih, kind);
    let (sx, sy) = (size.x / iw.max(1) as f32, size.y / ih.max(1) as f32);
    let frame = egui::Rect::from_min_size(rect.min + egui::vec2(r.x as f32 * sx, r.y as f32 * sy), egui::vec2(r.w as f32 * sx, r.h as f32 * sy));
    let shade = t.colors.bg0.gamma_multiply(0.7);
    for band in [
        egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x, frame.min.y)),
        egui::Rect::from_min_max(egui::pos2(rect.min.x, frame.max.y), rect.max),
        egui::Rect::from_min_max(egui::pos2(rect.min.x, frame.min.y), egui::pos2(frame.min.x, frame.max.y)),
        egui::Rect::from_min_max(egui::pos2(frame.max.x, frame.min.y), egui::pos2(rect.max.x, frame.max.y)),
    ] {
        painter.rect_filled(band, 0.0, shade);
    }
    let radius = if kind == ImageKind::Avatar { t.r_avatar(frame.width()) } else { t.r_card() };
    painter.rect_stroke(frame, radius, egui::Stroke::new(t.shapes.border * 2.0, t.colors.accent), egui::StrokeKind::Inside);
}
