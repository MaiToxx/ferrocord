//! Panneau « Se connecter avec un QR code » de l'écran de connexion (lot C2).
//!
//! Le QR est fabriqué en mémoire (une texture en niveaux de gris, un pixel
//! par module) et dessiné par egui ; noir sur blanc quel que soit le thème,
//! comme une image : un QR inversé n'est pas lu par tous les téléphones.
//! Le jeton obtenu suit le chemin d'un jeton saisi (`add_account`).

use super::accounts::Phase;
use super::App;
use crate::i18n::table::qr::{f as qf, t as qt};
use crate::i18n::verbatim::t as vt;
use crate::state::model::Avatar;
use crate::state::remote_auth::{qr_url, RemoteEvent, RemoteLogin, RemoteProblem, RemoteUser};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};
use std::time::{Duration, Instant};

/// QR renouvelés d'eux-mêmes à l'expiration, avant d'attendre un clic :
/// une fenêtre oubliée n'ouvre pas de sessions sans fin.
const AUTO_RENEWALS: u8 = 3;
/// Marge blanche autour du QR, en modules (norme : 4).
const QUIET: usize = 4;
/// Part de la largeur de la carte occupée par le QR.
const QR_SHARE: f32 = 0.62;
/// La même part dans la colonne de droite de l'écran à deux colonnes (176 sur 240).
const QR_SIDE_SHARE: f32 = 0.74;

#[derive(Default)]
pub(super) struct QrLogin {
    open: bool,
    attempt: Option<RemoteLogin>,
    step: Step,
    renewals: u8,
    /// Texture du QR affiché, et l'empreinte qu'il code.
    texture: Option<(String, egui::TextureHandle)>,
}

#[derive(Default)]
enum Step {
    #[default]
    Preparing,
    Shown {
        fingerprint: String,
        expires: Instant,
    },
    Scanned {
        user: RemoteUser,
        expires: Instant,
    },
    Ended(End),
}

enum End {
    Expired,
    Cancelled,
    Failed(RemoteProblem),
    Unavailable,
}

/// Texte du compte à rebours, et attente jusqu'à son prochain changement
/// (une image par minute, puis par seconde dans la dernière minute).
fn countdown(lang: crate::i18n::Lang, left: Duration) -> (String, Duration) {
    let ms = left.as_millis() as u64;
    if ms > 60_000 {
        let mins = ms.div_ceil(60_000);
        (qf::expires_minutes(lang, mins), Duration::from_millis(ms - ((mins - 1) * 60_000).max(60_000)))
    } else {
        let secs = ms.div_ceil(1000);
        (qf::expires_seconds(lang, secs), Duration::from_millis(ms - secs.saturating_sub(1) * 1000))
    }
}

/// Modules du QR de `fingerprint`, marge comprise : 0 = noir, 255 = blanc.
fn qr_pixels(fingerprint: &str) -> Option<(usize, Vec<u8>)> {
    let code = qrcodegen::QrCode::encode_text(&qr_url(fingerprint), qrcodegen::QrCodeEcc::Medium).ok()?;
    let n = usize::try_from(code.size()).ok()?;
    let side = n + 2 * QUIET;
    let q = QUIET as i32;
    let pixels = (0..side * side)
        .map(|i| {
            let (x, y) = ((i % side) as i32 - q, (i / side) as i32 - q);
            if code.get_module(x, y) { 0 } else { 255 }
        })
        .collect();
    Some((side, pixels))
}

impl App {
    /// Le panneau QR remplace le champ du jeton.
    pub(super) fn qr_shown(&self) -> bool {
        self.qr.open
    }

    pub(super) fn qr_open(&mut self) {
        self.qr.open = true;
        self.qr.renewals = 0;
        self.login_error = None;
        self.login_problem = None;
        self.qr_restart();
    }

    /// Nouvelle tentative : nouvelle clé, nouvelle session, nouveau QR.
    fn qr_restart(&mut self) {
        self.qr.attempt = None;
        self.qr.texture = None;
        self.qr.step = Step::Preparing;
        match self.backend.remote_auth() {
            Some(a) => self.qr.attempt = Some(a),
            None => self.qr.step = Step::Ended(End::Unavailable),
        }
    }

    /// Retour au jeton : la tentative en cours est abandonnée (connexion
    /// fermée, clé effacée).
    pub(super) fn qr_close(&mut self) {
        self.qr = QrLogin::default();
    }

    /// Événements de la tentative en cours (chaque image de logique).
    pub(super) fn qr_tick(&mut self) {
        if self.qr.attempt.is_some() && self.phase != Phase::Login {
            // Une autre session s'ouvre (compte enregistré choisi…).
            self.qr_close();
            return;
        }
        let Some(events) = self.qr.attempt.as_ref().map(RemoteLogin::poll) else { return };
        for e in events {
            if e.is_final() {
                self.qr.attempt = None;
            }
            match e {
                RemoteEvent::Qr { fingerprint, expires } => self.qr.step = Step::Shown { fingerprint, expires },
                RemoteEvent::User(user) => {
                    let expires = match &self.qr.step {
                        Step::Shown { expires, .. } | Step::Scanned { expires, .. } => *expires,
                        _ => Instant::now(),
                    };
                    self.qr.step = Step::Scanned { user, expires };
                }
                RemoteEvent::Token(token) => {
                    self.qr_close();
                    let remember = self.remember;
                    self.add_account(token, remember);
                    return;
                }
                RemoteEvent::Cancelled => self.qr.step = Step::Ended(End::Cancelled),
                RemoteEvent::Expired if self.qr.renewals < AUTO_RENEWALS => {
                    self.qr.renewals += 1;
                    self.qr_restart();
                    return;
                }
                RemoteEvent::Expired => self.qr.step = Step::Ended(End::Expired),
                RemoteEvent::Failed(p) => self.qr.step = Step::Ended(End::Failed(p)),
            }
        }
    }

    /// Aperçu hors ligne : QR affiché, ou déjà lu par « Alice ».
    #[doc(hidden)]
    pub fn preview_qr_login(&mut self, scanned: bool) {
        self.qr.open = true;
        let expires = Instant::now() + Duration::from_secs(118);
        self.qr.step = if scanned {
            let user = RemoteUser { id: 1_000_000_000_000_000_002, discriminator: None, avatar: None, name: "Alice".into() };
            Step::Scanned { user, expires }
        } else {
            Step::Shown { fingerprint: "Ki1Pb4h6tV9tq2rLx3mWc0yZ8sD7fG5hJ4kL2nB1vQe".into(), expires }
        };
    }

    /// Colonne de droite de l'écran de connexion. Tant que le code n'est pas
    /// demandé, sa place vide, le titre, la marche à suivre et « Afficher le
    /// QR code » : la session d'authentification ne s'ouvre qu'à ce geste.
    pub(super) fn qr_side_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        if self.qr.open {
            return self.qr_ui(ui, t, true);
        }
        let lang = self.lang;
        // Même place que le code : la colonne ne bouge pas quand il arrive.
        let side = (ui.available_width() * QR_SIDE_SHARE).round();
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
        let p = ui.painter();
        p.rect(rect, t.r_embed(), t.colors.bg0, t.border(), egui::StrokeKind::Inside);
        // Trois repères de coin estompés : la silhouette d'un QR code, sans en être un.
        let (m, s) = ((side * 0.12).round(), (side * 0.22).round());
        for corner in [rect.left_top() + egui::vec2(m, m), rect.right_top() + egui::vec2(-m - s, m), rect.left_bottom() + egui::vec2(m, -m - s)] {
            let sq = egui::Rect::from_min_size(corner, egui::Vec2::splat(s));
            p.rect_stroke(sq, t.r_row(), egui::Stroke::new((s * 0.16).max(1.0), t.colors.faint.gamma_multiply(0.5)), egui::StrokeKind::Inside);
            p.rect_filled(sq.shrink(s * 0.32), 0.0, t.colors.faint.gamma_multiply(0.5));
        }
        ui.add_space(t.spacing.login_title_gap);
        ui.add(egui::Label::new(RichText::new(qt::WITH_QR.get(lang)).size((t.typography.title * 1.25).round()).family(crate::ui::fonts::demi()).color(t.colors.text_strong)).wrap());
        ui.add_space(t.spacing.login_field_gap);
        ui.add(egui::Label::new(RichText::new(qt::HOW.get(lang)).size(t.typography.button).color(t.colors.muted)).wrap());
        ui.add_space(t.spacing.login_button_gap);
        if super::widgets::button_wide(ui, t, super::widgets::Kind::Secondary, qt::SHOW_QR.get(lang), ui.available_width()).clicked() {
            self.qr_open();
        }
    }

    /// `side` : dans la colonne de droite (le formulaire du jeton reste à gauche).
    pub(super) fn qr_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, side: bool) {
        let lang = self.lang;
        if !side {
            ui.label(RichText::new(qt::WITH_QR.get(lang)).small().color(t.colors.muted));
            ui.add_space(t.spacing.login_field_gap);
        }
        let share = if side { QR_SIDE_SHARE } else { QR_SHARE };
        let mut renew = false;
        match &self.qr.step {
            Step::Preparing => {
                ui.add_space(t.spacing.login_button_gap);
                ui.add(egui::Spinner::new().color(t.colors.accent));
                ui.label(RichText::new(qt::PREPARING.get(lang)).color(t.colors.text));
            }
            Step::Shown { fingerprint, expires } => {
                let (fingerprint, expires) = (fingerprint.clone(), *expires);
                self.qr_image(ui, &fingerprint, share);
                if side {
                    ui.add_space(t.spacing.login_title_gap);
                    ui.add(egui::Label::new(RichText::new(qt::WITH_QR.get(lang)).size((t.typography.title * 1.25).round()).family(crate::ui::fonts::demi()).color(t.colors.text_strong)).wrap());
                }
                ui.add_space(t.spacing.login_field_gap);
                let how = RichText::new(qt::HOW.get(lang));
                ui.add(egui::Label::new(if side { how.size(t.typography.button).color(t.colors.muted) } else { how.color(t.colors.text) }).wrap());
                self.qr_countdown(ui, t, expires);
            }
            Step::Scanned { user, expires } => {
                let (user, expires) = (user.clone(), *expires);
                ui.label(RichText::new(qt::CONFIRM_HINT.get(lang)).small().color(t.colors.muted));
                let avatar = Avatar::of_user(user.id, user.avatar, user.discriminator.as_deref());
                // Avatar puis nom, au milieu comme le reste du panneau (la ligne de compte était calée à gauche).
                ui.add_space(t.spacing.login_field_gap);
                if t.avatars_shown() {
                    let side = (t.layout.avatar_size * 1.4).round();
                    let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
                    crate::ui::util::paint_avatar(ui, &self.images, t, crate::ui::util::pixel_square(ui, slot.min, side), &avatar, &user.name);
                }
                ui.add(egui::Label::new(RichText::new(&user.name).size(t.typography.title).family(crate::ui::fonts::demi()).color(t.colors.text_strong)).truncate());
                ui.add_space(t.spacing.login_field_gap);
                ui.add(egui::Label::new(RichText::new(qf::confirm(lang, &user.name)).color(t.colors.text)).wrap());
                self.qr_countdown(ui, t, expires);
            }
            Step::Ended(end) => {
                let (text, color) = match end {
                    End::Expired => (qt::EXPIRED.get(lang).to_string(), t.colors.muted),
                    End::Cancelled => (qt::CANCELLED.get(lang).to_string(), t.colors.muted),
                    End::Unavailable => (qt::UNAVAILABLE.get(lang).to_string(), t.colors.muted),
                    End::Failed(p) => (self.qr_problem(p), t.colors.danger),
                };
                ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                if !matches!(end, End::Unavailable) {
                    ui.add_space(t.spacing.login_button_gap);
                    renew = ui
                        .add_sized(
                            [ui.available_width(), t.layout.button_height],
                            egui::Button::new(RichText::new(qt::RENEW.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.on_accent)).fill(t.colors.accent),
                        )
                        .clicked();
                }
            }
        }
        // À côté du formulaire, « Se souvenir de moi » est déjà à gauche.
        if !side && self.accounts.vault_name().is_some() {
            ui.add_space(t.spacing.login_field_gap);
            ui.checkbox(&mut self.remember, vt::REMEMBER_ME.get(lang));
        }
        ui.add_space(t.spacing.login_button_gap);
        let back = if side {
            super::widgets::button(ui, t, super::widgets::Kind::Link, qt::HIDE_QR.get(lang)).clicked()
        } else {
            ui.add_sized([ui.available_width(), t.layout.button_height], egui::Button::new(qt::WITH_TOKEN.get(lang))).clicked()
        };
        if renew {
            self.qr_open();
        } else if back {
            self.qr_close();
        }
    }

    fn qr_problem(&self, p: &RemoteProblem) -> String {
        let lang = self.lang;
        match p {
            RemoteProblem::Captcha => qt::CAPTCHA.get(lang).into(),
            RemoteProblem::Network(d) => qf::network(lang, d),
            RemoteProblem::Protocol => qt::PROTOCOL.get(lang).into(),
            RemoteProblem::Closed(code) => qf::closed(lang, code),
            RemoteProblem::Api(p) => qf::refused(lang, super::words::problem(lang, p, self.h24())),
        }
    }

    fn qr_countdown(&self, ui: &mut egui::Ui, t: &Tokens, expires: Instant) {
        let left = expires.saturating_duration_since(Instant::now());
        let (text, wait) = countdown(self.lang, left);
        ui.label(RichText::new(text).small().color(t.colors.faint));
        if !left.is_zero() {
            ui.ctx().request_repaint_after(wait);
        }
    }

    /// QR net : chaque module couvre un nombre entier de pixels physiques.
    fn qr_image(&mut self, ui: &mut egui::Ui, fingerprint: &str, share: f32) {
        if self.qr.texture.as_ref().is_none_or(|(f, _)| f != fingerprint) {
            self.qr.texture = qr_pixels(fingerprint).map(|(side, gray)| {
                let image = egui::ColorImage::from_gray([side, side], &gray);
                (fingerprint.to_string(), ui.ctx().load_texture("qr-connexion", image, egui::TextureOptions::NEAREST))
            });
        }
        let Some((_, tex)) = &self.qr.texture else { return };
        let modules = tex.size()[0] as f32;
        let ppp = ui.ctx().pixels_per_point();
        let px = ((ui.available_width() * share * ppp) / modules).floor().max(1.0);
        let side = px * modules / ppp;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
        let rect = egui::emath::GuiRounding::round_to_pixels(rect, ppp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, qt::QR_LABEL.get(self.lang)));
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        // Teinte neutre : la texture est dessinée telle quelle (noir sur blanc).
        ui.painter().image(tex.id(), rect, uv, egui::Color32::WHITE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn countdown_redraws_once_a_minute_then_once_a_second() {
        assert_eq!(countdown(Lang::Fr, Duration::from_millis(118_500)), ("Expire dans 2\u{a0}min".into(), Duration::from_millis(58_500)));
        assert_eq!(countdown(Lang::Fr, Duration::from_millis(60_001)), ("Expire dans 2\u{a0}min".into(), Duration::from_millis(1)));
        assert_eq!(countdown(Lang::Fr, Duration::from_millis(60_000)), ("Expire dans 60\u{a0}s".into(), Duration::from_millis(1000)));
        assert_eq!(countdown(Lang::En, Duration::from_millis(41_200)), ("Expires in 42 s".into(), Duration::from_millis(200)));
        assert_eq!(countdown(Lang::Fr, Duration::ZERO).0, "Expire dans 0\u{a0}s");
    }

    #[test]
    fn qr_has_a_quiet_zone_and_finder_patterns() {
        let (side, px) = qr_pixels("Ki1Pb4h6tV9tq2rLx3mWc0yZ8sD7fG5hJ4kL2nB1vQe").unwrap();
        assert_eq!(px.len(), side * side);
        assert!(px[..QUIET * side].iter().all(|p| *p == 255), "marge blanche");
        // Coin du motif de repérage en haut à gauche : noir.
        assert_eq!(px[QUIET * side + QUIET], 0);
        assert!((side - 2 * QUIET - 17).is_multiple_of(4), "taille d'un QR : 17 + 4 × version");
    }
}
