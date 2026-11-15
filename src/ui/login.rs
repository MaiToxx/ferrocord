//! Écran de connexion : comptes enregistrés, ajout d'un compte par jeton, et
//! les étapes en cours (ouverture des comptes, lecture d'un jeton dans le
//! coffre, connexion à Discord).

use super::{
    accounts::{remove_button, Confirm, Phase, RowAvatar, TOKEN_FIELD},
    util::inline_icon,
    App,
};
use crate::account::{AccountKey, Token, TokenError};
use crate::i18n::{
    verbatim::{f as vf, t as vt},
    Lang,
};
use crate::state::Status;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Key, Layout, RichText};
use std::time::Duration;

/// Ce que l'écran de connexion montre.
enum Screen {
    /// Une étape en cours (toujours annulable).
    Busy(String),
    Form,
}

/// Paliers (secondes) où le temps écoulé d'une étape en cours est affiché,
/// puis chaque minute : l'écran n'est redessiné qu'à ces moments-là (une
/// image coûte quelques millisecondes ; aucune animation continue).
const MILESTONES: [u64; 3] = [10, 30, 60];

/// Texte du temps écoulé, et délai jusqu'au palier suivant.
fn elapsed_text(lang: Lang, elapsed: Duration) -> (String, Duration) {
    let secs = elapsed.as_secs();
    let next = MILESTONES.iter().copied().find(|m| *m > secs).unwrap_or((secs / 60 + 1) * 60);
    let text = match MILESTONES.iter().copied().rev().find(|m| secs >= *m) {
        None => String::new(),
        Some(_) if secs >= 120 => vf::still_running_minutes(lang, secs / 60),
        Some(m) if m >= 60 => vt::STILL_RUNNING_MINUTE.get(lang).into(),
        Some(m) => vf::still_running_seconds(lang, m),
    };
    (text, Duration::from_secs(next).saturating_sub(elapsed))
}

impl App {
    pub(super) fn login_ui(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let screen = match &self.phase {
            Phase::Starting => Screen::Busy(vt::OPENING_ACCOUNTS.get(lang).into()),
            Phase::Unlocking(k) => Screen::Busy(vf::reading_token(lang, self.account_label(k))),
            Phase::Session => Screen::Busy(vf::connecting_to_discord(lang, self.current_label())),
            Phase::Login => Screen::Form,
        };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg0)).show(ui, |ui| {
            {
                backdrop(ui, &t);
                // Carte plus haute que la fenêtre (trois comptes et une notice) : elle défile,
                // au lieu de se coller à la barre de titre et de perdre son bas.
                let room = ui.available_height();
                egui::ScrollArea::vertical().id_salt("connexion").auto_shrink([false, false]).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    // Centrage vertical : hauteur de la carte mesurée à l'image
                    // précédente ; si elle change, l'image est refaite aussitôt.
                    let height_id = ui.scope_id().with("login_card_height");
                    let known: f32 = ui.data(|d| d.get_temp(height_id)).unwrap_or_default();
                    let margin = t.spacing.card_pad;
                    ui.add_space(Tokens::centered_top(room, known).max(margin));
                    // Deux colonnes quand la fenêtre le permet : le formulaire à gauche,
                    // le QR code à droite, un filet entre les deux (comme dans le client).
                    let pad = t.spacing.card_pad;
                    let left_w = t.layout.card_width - 2.0 * pad;
                    let right_w = (t.layout.card_width * QR_COLUMN).round();
                    let two = matches!(screen, Screen::Form) && !self.mobile_active() && ui.available_width() >= left_w + right_w + 6.0 * pad;
                    let card = egui::Frame::new()
                        .fill(t.colors.bg2)
                        .corner_radius(t.r_card())
                        .inner_margin(t.card_margin())
                        .stroke(t.border())
                        .shadow(t.shadow(crate::theme::Elevation::Window))
                        .show(ui, |ui| {
                            let left_w = if two { left_w } else { t.layout.card_width.min(ui.available_width()) };
                            ui.set_width(if two { left_w + 2.0 * pad + right_w } else { left_w });
                            let row = ui
                                .horizontal_top(|ui| {
                                    ui.spacing_mut().item_spacing.x = 0.0;
                                    ui.allocate_ui_with_layout(egui::vec2(left_w, 0.0), Layout::top_down(egui::Align::Min), |ui| {
                                        ui.set_width(left_w);
                                        ui.vertical_centered(|ui| {
                                            // Logo d'un seul tenant, centré au-dessus du formulaire.
                                            let mut logo = egui::text::LayoutJob::default();
                                            let font = egui::FontId::proportional(t.typography.logo);
                                            logo.append("Ferro", Default::default(), egui::TextFormat::simple(font.clone(), t.colors.text_strong));
                                            logo.append("cord", Default::default(), egui::TextFormat::simple(font, t.colors.accent));
                                            ui.label(logo);
                                            ui.label(RichText::new(vt::TAGLINE.get(lang)).color(t.colors.muted));
                                        });
                                        ui.add_space(t.spacing.login_title_gap);
                                        match &screen {
                                            Screen::Busy(text) => {
                                                ui.vertical_centered(|ui| self.busy_ui(ui, &t, text));
                                            }
                                            Screen::Form => self.form_ui(ui, &t, two),
                                        }
                                        ui.add_space(t.spacing.login_notice_gap);
                                        ui.add(egui::Label::new(RichText::new(vt::UNOFFICIAL_WARNING.get(lang)).small().color(t.colors.faint)).wrap());
                                    });
                                    if !two {
                                        return None;
                                    }
                                    ui.add_space(pad);
                                    let x = ui.cursor().left();
                                    ui.add_space(pad);
                                    ui.allocate_ui_with_layout(egui::vec2(right_w, 0.0), Layout::top_down(egui::Align::Center), |ui| {
                                        ui.set_width(right_w);
                                        self.qr_side_ui(ui, &t);
                                    });
                                    Some(x)
                                });
                            // Filet entre les deux colonnes, sur toute la hauteur de la carte.
                            if let Some(x) = row.inner {
                                ui.painter().vline(x, row.response.rect.y_range(), egui::Stroke::new(t.shapes.border.max(1.0), t.colors.separator));
                            }
                        });
                    let height = card.response.rect.height();
                    if height != known {
                        ui.data_mut(|d| d.insert_temp(height_id, height));
                        ui.ctx().request_discard("centrage de la carte de connexion");
                    }
                    if height + 2.0 * margin > room {
                        ui.add_space(margin);
                    }
                });
                });
            }
        });
    }

    /// Étape en cours : icône fixe, texte, temps écoulé (à quelques paliers
    /// seulement), nouvelles du réseau, bouton Annuler.
    fn busy_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, text: &str) {
        inline_icon(ui, Icon::Hourglass, t.typography.logo, t.colors.accent);
        ui.add_space(t.spacing.login_field_gap);
        ui.label(RichText::new(text).color(t.colors.text));
        let (since, wait) = elapsed_text(self.lang, self.phase_since.elapsed());
        // Ligne gardée même vide : la carte ne bouge pas au premier palier.
        ui.label(RichText::new(since).small().color(t.colors.muted));
        ui.ctx().request_repaint_after(wait);
        // Seules les nouvelles du réseau (reconnexion, erreur) s'ajoutent au texte.
        let status = &self.store.status;
        if self.phase == Phase::Session && !matches!(status, Status::Idle | Status::Connecting | Status::Connected) {
            ui.label(RichText::new(super::words::status(self.lang, status, self.h24())).small().color(t.colors.muted));
        }
        ui.add_space(t.spacing.login_button_gap);
        if ui.add_sized([ui.available_width(), t.layout.button_height], egui::Button::new(vt::CANCEL.get(self.lang))).clicked() {
            self.cancel_busy();
        }
    }

    /// `side_qr` : le QR code a sa colonne à droite ; il ne remplace pas le
    /// champ du jeton et son bouton n'est pas répété ici.
    fn form_ui(&mut self, ui: &mut egui::Ui, t: &Tokens, side_qr: bool) {
        let lang = self.lang;
        let keys: Vec<AccountKey> = self.prefs.now.accounts.iter().map(|a| a.key.clone()).collect();
        let others = self.env_token.is_some() || self.legacy_token.is_some();
        if !keys.is_empty() || others {
            ui.label(RichText::new(vt::SAVED_ACCOUNTS.get(lang)).small().color(t.colors.muted));
            let mut open = None;
            for key in keys {
                if let Some(k) = self.saved_account_row(ui, t, key) {
                    open = Some(k);
                }
            }
            let width = ui.available_width();
            if self.env_token.is_some() && self.account_row(ui, width, RowAvatar::None, "DISCORD_TOKEN", Some(vt::ENV_SESSION.get(lang)), false).clicked() {
                self.open_env();
            }
            if self.legacy_token.is_some() {
                self.legacy_row(ui, t);
            }
            if let Some(k) = open {
                self.switch_account(k);
            }
            ui.add_space(t.spacing.login_title_gap);
        }
        if let Some(w) = self.env_warning.clone() {
            ui.label(RichText::new(w).small().color(t.colors.danger));
            ui.add_space(t.spacing.login_title_gap);
        }
        if self.replace_question().is_some() {
            self.replace_notice(ui, t);
            ui.add_space(t.spacing.login_title_gap);
        }
        if self.twin_to_remove().is_some() {
            self.twin_notice(ui, t);
            ui.add_space(t.spacing.login_title_gap);
        }
        if self.legacy_file.is_some() {
            self.legacy_file_notice(ui);
            ui.add_space(t.spacing.login_title_gap);
        }
        if self.recovery.is_some() {
            self.recovery_notice(ui);
            ui.add_space(t.spacing.login_title_gap);
        }
        // Connexion par QR code (lot C2) : à la place du champ du jeton.
        if self.qr_shown() && !side_qr {
            ui.vertical_centered(|ui| self.qr_ui(ui, t, false));
            return;
        }
        let title = if keys_or_others(self) { vt::ADD_ACCOUNT_HEADING.get(lang) } else { vt::TOKEN_HEADING.get(lang) };
        // Libellé à gauche, demi-gras ; champ haut de 40, comme ceux des formulaires.
        super::widgets::field_label(ui, t, title);
        let pad = (t.spacing.item_x + t.spacing.item_y).round() as i8;
        let resp = ui.add_sized(
            [ui.available_width(), t.field_height()],
            egui::TextEdit::singleline(&mut self.token_input)
                .id(egui::Id::unique(TOKEN_FIELD))
                .password(true)
                .hint_text(RichText::new(vt::TOKEN_HINT.get(lang)).color(t.colors.faint))
                .margin(egui::Margin::symmetric(pad, 0))
                .align(egui::Align2::LEFT_CENTER),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        ui.add_space(t.spacing.login_field_gap);
        match self.accounts.vault_name() {
            Some(vault) => {
                ui.checkbox(&mut self.remember, vt::REMEMBER_ME.get(lang));
                let note = if self.remember {
                    vf::saved_once_accepted(lang, vault)
                } else {
                    vt::SESSION_ONLY.get(lang).into()
                };
                ui.label(RichText::new(note).small().color(t.colors.faint));
            }
            None => {
                ui.label(RichText::new(vt::NO_VAULT_SESSION_ONLY.get(lang)).small().color(t.colors.faint));
            }
        }
        if let Some(e) = &self.prefs.file_error {
            ui.label(RichText::new(vf::account_list_not_saved(lang, e)).small().color(t.colors.danger));
        }
        ui.add_space(t.spacing.login_button_gap);
        let go = ui
            .add_sized(
                [ui.available_width(), t.layout.button_height],
                egui::Button::new(RichText::new(vt::LOG_IN.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.on_accent)).fill(t.colors.accent),
            )
            .clicked();
        if go || enter {
            self.submit_token();
        }
        if !side_qr {
            ui.add_space(t.spacing.login_field_gap);
            if ui.add_sized([ui.available_width(), t.layout.button_height], egui::Button::new(crate::i18n::table::qr::t::WITH_QR.get(lang))).clicked() {
                self.qr_open();
            }
        }
        if let Some(e) = &self.login_error {
            ui.add_space(t.spacing.login_error_gap);
            ui.label(RichText::new(e).color(t.colors.danger));
        }
    }

    /// Bouton Connexion : le jeton du champ ouvre une session, et le champ
    /// est vidé (avec son historique d'annulation). Jeton abîmé : refusé avec
    /// la raison, le champ est gardé pour être corrigé.
    pub(super) fn submit_token(&mut self) {
        match Token::parse(&self.token_input) {
            Ok(token) => {
                self.clear_token_input();
                let remember = self.remember;
                self.add_account(token, remember);
            }
            Err(TokenError::Empty) => {}
            Err(e) => self.login_error = Some(vf::token_invalid(self.lang, e)),
        }
    }

    /// Ligne d'un compte enregistré : clic pour l'ouvrir, × pour le retirer
    /// (confirmé par un deuxième clic dans la ligne). Rend la clé à ouvrir.
    fn saved_account_row(&mut self, ui: &mut egui::Ui, t: &Tokens, key: AccountKey) -> Option<AccountKey> {
        let lang = self.lang;
        let busy = self.account_busy(&key);
        let confirming = self.confirm == Some(Confirm::Remove(key.clone()));
        let name = self.account_label(&key);
        let question = self.remove_question(&key);
        let twin = self.twin_of(&key).is_some().then_some(vt::SAME_ACCOUNT_TWICE.get(lang));
        let detail = if let Some(b) = busy {
            b
        } else if confirming {
            vt::REMOVE_THIS_ACCOUNT_ASK.get(lang)
        } else if let Some(d) = twin {
            d
        } else if key.is_imported() && self.prefs.now.account(&key).is_some_and(|a| a.user_id.is_none()) {
            vt::FROM_OLD_FILE.get(lang)
        } else {
            vt::OPEN_THIS_ACCOUNT.get(lang)
        };
        let mut open = None;
        let height = t.account_row_height();
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), Layout::right_to_left(egui::Align::Center), |ui| {
            // La carte met l'écart horizontal à zéro pour ses deux colonnes : « Oui » et « Non » se touchaient.
            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
            if busy.is_none() {
                if confirming {
                    if ui.button(vt::NO.get(lang)).clicked() {
                        self.confirm = None;
                    }
                    if question.is_some() && ui.button(RichText::new(vt::YES.get(lang)).color(t.colors.danger)).clicked() {
                        self.confirmed(Confirm::Remove(key.clone()));
                    }
                } else if remove_button(ui, t, vt::REMOVE_ACCOUNT_TIP.get(lang)).clicked() {
                    self.ask_remove(key.clone());
                }
            }
            let width = ui.available_width();
            let mut row = self.account_row(ui, width, self.account_avatar(&key), &name, Some(detail), false);
            if let Some(tip) = self.account_tip(&key) {
                row = row.on_hover_text(tip);
            }
            if row.clicked() && busy.is_none() && !confirming {
                open = Some(key.clone());
            }
        });
        // La question entière, sous la ligne (jamais tronquée).
        if confirming && busy.is_none() {
            let q = question.unwrap_or_else(|| vt::CHECKING_VAULT.get(lang).into());
            ui.add(egui::Label::new(RichText::new(q).small().color(t.colors.text)).wrap());
        }
        open
    }

    /// Ligne de l'ancien fichier dont la migration a échoué : clic pour
    /// l'ouvrir, × pour le supprimer (confirmé ; son jeton n'est nulle part
    /// ailleurs).
    fn legacy_row(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let confirming = self.confirm == Some(Confirm::WipeLegacy);
        let question = self.wipe_question();
        let detail = if self.wiping_legacy {
            vt::DELETING.get(lang)
        } else if confirming {
            vt::WIPE_QUESTION.get(lang)
        } else {
            self.legacy_wording().1
        };
        let height = t.account_row_height();
        let mut open = false;
        ui.allocate_ui_with_layout(egui::vec2(ui.available_width(), height), Layout::right_to_left(egui::Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
            // L'avertissement durable, s'il est affiché, a déjà son bouton de suppression.
            if !self.wiping_legacy && self.legacy_file.is_none() {
                if confirming {
                    if ui.button(vt::NO.get(lang)).clicked() {
                        self.confirm = None;
                    }
                    if question.is_some() && ui.button(RichText::new(vt::YES.get(lang)).color(t.colors.danger)).clicked() {
                        self.confirmed(Confirm::WipeLegacy);
                    }
                } else if remove_button(ui, t, vt::WIPE_LABEL.get(lang)).clicked() {
                    self.ask_wipe();
                }
            }
            let width = ui.available_width();
            let name = self.legacy_wording().0;
            open = self.account_row(ui, width, RowAvatar::None, name, Some(detail), false).clicked() && !confirming;
        });
        // La question entière, après lecture du coffre (jamais tronquée).
        if confirming && !self.wiping_legacy && self.legacy_file.is_none() {
            let q = question.unwrap_or_else(|| vt::CHECKING_VAULT.get(lang).into());
            ui.add(egui::Label::new(RichText::new(q).small().color(t.colors.text)).wrap());
        }
        if open {
            self.open_legacy();
        }
    }
}

/// Largeur de la colonne du QR code, en part de la largeur d'une carte (240 pour 440).
const QR_COLUMN: f32 = 0.55;

/// Fond de l'écran de connexion : motif maison, deux grands disques d'accent
/// très estompés dans deux coins opposés. Fixe : aucune image n'est demandée.
fn backdrop(ui: &egui::Ui, t: &Tokens) {
    let full = ui.max_rect();
    let p = ui.painter().with_clip_rect(full);
    let (w, h) = (full.width(), full.height());
    p.circle_filled(full.left_top() + egui::vec2(w * 0.08, h * 0.12), h * 0.42, t.colors.accent.gamma_multiply(0.07));
    p.circle_filled(full.right_bottom() - egui::vec2(w * 0.06, h * 0.04), h * 0.55, t.colors.accent.gamma_multiply(0.07));
    p.circle_filled(full.right_top() + egui::vec2(-w * 0.18, h * 0.10), h * 0.12, t.colors.link.gamma_multiply(0.04));
}

/// Des comptes (ou des jetons d'ailleurs) sont proposés au-dessus du champ.
fn keys_or_others(app: &App) -> bool {
    !app.prefs.now.accounts.is_empty() || app.env_token.is_some() || app.legacy_token.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn elapsed_time_is_shown_at_a_few_milestones_only() {
        let at = |s: u64| elapsed_text(Lang::Fr, Duration::from_millis(s * 1000 + 300));
        assert_eq!(at(0), (String::new(), Duration::from_millis(9700)));
        assert_eq!(at(9).0, "");
        assert_eq!(at(10), ("Toujours en cours (plus de 10 s)".into(), Duration::from_millis(19700)));
        assert_eq!(at(45).0, "Toujours en cours (plus de 30 s)");
        assert_eq!(at(61), ("Toujours en cours (plus d'une minute)".into(), Duration::from_millis(58700)));
        assert_eq!(at(130), ("Toujours en cours (plus de 2 min)".into(), Duration::from_millis(49700)));
        assert_eq!(elapsed_text(Lang::En, Duration::from_secs(130)).0, "Still in progress (over 2 min)");
    }
}
