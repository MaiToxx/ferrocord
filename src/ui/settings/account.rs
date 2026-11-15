//! Pages du compte : Mon compte, Profil, Applications autorisées, Appareils,
//! Comptes liés, Facturation, et la page Ferrocord.

use super::{in_discord, note, section, value_row, Confirm, App};
use crate::i18n::table::s::{f as sf, t as st};
use crate::i18n::{time, Lang};
use crate::state::usersettings::{Profile, ProfileEdit, Remote};
use crate::theme::Tokens;
use eframe::egui::{self, RichText};

/// Brouillons des pages du compte (rien n'est envoyé avant « Enregistrer »).
#[derive(Default)]
pub(in crate::ui) struct Drafts {
    /// Profil en cours de modification, et celui dont il part.
    profile: Option<(Profile, ProfileDraft)>,
    nick_guild: Option<u64>,
    nick: String,
    show_email: bool,
    show_phone: bool,
}

/// Brouillon du profil du compte, partagé par la page Profils et par
/// l'édition rapide (`ui/profile_edit.rs`).
#[derive(Clone, Default, PartialEq)]
pub(in crate::ui) struct ProfileDraft {
    pub(in crate::ui) global_name: String,
    pub(in crate::ui) pronouns: String,
    pub(in crate::ui) bio: String,
    pub(in crate::ui) accent: Option<[u8; 3]>,
    /// Couleurs du thème du profil (principale, secondaire).
    pub(in crate::ui) theme: Option<[[u8; 3]; 2]>,
}

/// 0xRRGGBB -> octets du sélecteur de couleur.
pub(in crate::ui) fn rgb_bytes(c: u32) -> [u8; 3] {
    [(c >> 16) as u8, (c >> 8) as u8, c as u8]
}

pub(in crate::ui) fn rgb_code([r, g, b]: [u8; 3]) -> u32 {
    (u32::from(r) << 16) | (u32::from(g) << 8) | u32::from(b)
}

impl ProfileDraft {
    /// `theme` : couleurs du thème connues (elles viennent du profil
    /// complet, pas de `GET /users/@me`) ; `None` : aucune, ou inconnues.
    pub(in crate::ui) fn of(p: &Profile, theme: Option<[u32; 2]>) -> Self {
        ProfileDraft {
            global_name: p.global_name.clone().unwrap_or_default(),
            pronouns: p.pronouns.clone(),
            bio: p.bio.clone(),
            accent: p.accent.map(rgb_bytes),
            theme: theme.map(|t| t.map(rgb_bytes)),
        }
    }

    /// Ce qui a changé par rapport à `base` : seuls ces champs partent.
    pub(in crate::ui) fn edit(&self, base: &Profile, theme: Option<[u32; 2]>) -> ProfileEdit {
        let b = ProfileDraft::of(base, theme);
        let name = self.global_name.trim();
        ProfileEdit {
            global_name: (name != b.global_name).then(|| (!name.is_empty()).then(|| name.to_string())),
            pronouns: (self.pronouns != b.pronouns).then(|| self.pronouns.clone()),
            bio: (self.bio != b.bio).then(|| self.bio.clone()),
            accent: (self.accent != b.accent).then(|| self.accent.map(rgb_code)),
            theme: (self.theme != b.theme).then(|| self.theme.map(|t| t.map(rgb_code))),
            effect: None,
        }
    }
}

/// Adresse masquée : seul le domaine reste visible.
pub(super) fn mask_email(e: &str) -> String {
    match e.rsplit_once('@') {
        Some((_, domain)) => format!("••••••••@{domain}"),
        None => "••••••••".into(),
    }
}

/// Numéro masqué : les quatre derniers chiffres restent visibles.
pub(super) fn mask_phone(p: &str) -> String {
    let digits: Vec<char> = p.chars().filter(char::is_ascii_digit).collect();
    let tail: String = digits[digits.len().saturating_sub(4)..].iter().collect();
    format!("••••••{tail}")
}

/// État d'une donnée chargée à la demande, en une ligne ; `true` si prête.
pub(in crate::ui) fn remote_line<T>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, r: &Remote<T>, h24: bool) -> bool {
    match r {
        Remote::Ready(_) => true,
        Remote::Idle | Remote::Loading => {
            ui.label(RichText::new(st::LOADING.get(lang)).color(t.colors.muted));
            false
        }
        Remote::Failed(p) => {
            ui.add(egui::Label::new(RichText::new(sf::load_failed(lang, super::super::words::problem(lang, p, h24))).color(t.colors.danger)).wrap());
            false
        }
    }
}

impl App {
    pub(super) fn page_my_account(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if self.store.account.profile == Remote::Idle {
            self.store.account.load_profile();
        }
        let h24 = self.h24();
        if remote_line(ui, t, lang, &self.store.account.profile, h24)
            && let Some(p) = self.store.account.profile.ready().cloned()
        {
            // Mode streamer : adresse et téléphone masqués, sans bouton pour les montrer.
            let streaming = super::local::display(ui.ctx()).hide_personal;
            // Carte de profil en tête de page (bannière, avatar à cheval, nom,
            // « Modifier le profil ») ; les lignes du compte sont dans son encart.
            let mut edit = false;
            let me = self.store.me.clone();
            let inset = t.card_frame().inner_margin(t.profile_margin());
            let outer = egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(t.r_card());
            outer.show(ui, |ui| {
            let width = ui.available_width();
            ui.set_width(width);
            if let Some(me) = &me {
                let shown = self.shown(me.id, None);
                let name = super::super::profile::name_of(&me.name, &shown);
                self.profile_header(ui, t, width, me.id, None, &shown, me.avatar, &name, super::super::profile::Header::default());
                // Nom et bouton à droite de l'avatar, sur la moitié basse de celui-ci.
                let side = t.layout.profile_avatar;
                let row = egui::Rect::from_min_max(
                    egui::pos2(ui.cursor().left() + 2.0 * t.profile_pad() + side, ui.cursor().top() - side / 2.0),
                    egui::pos2(ui.cursor().left() + width - t.profile_pad(), ui.cursor().top()),
                );
                let mut over = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)));
                edit = crate::ui::widgets::button(&mut over, t, crate::ui::widgets::Kind::Primary, crate::i18n::table::quick::t::EDIT_PROFILE.get(lang)).clicked();
                over.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(name).size((t.typography.title * 1.25).round()).family(crate::ui::fonts::demi()).color(t.colors.text_strong)).truncate());
                });
            }
            egui::Frame::new().inner_margin(t.profile_margin()).show(ui, |ui| inset.show(ui, |ui| {
            ui.set_width(ui.available_width());
            let d = &mut self.params.drafts;
            value_row(ui, st::USERNAME.get(lang), st::USERNAME_TIP.get(lang), &p.username);
            value_row(ui, st::DISPLAY_NAME.get(lang), st::DISPLAY_NAME_TIP.get(lang), p.global_name.as_deref().unwrap_or(st::NONE.get(lang)));
            for (label, tip, value, shown, mask) in [
                (st::EMAIL, st::EMAIL_TIP, p.email.as_deref(), &mut d.show_email, mask_email as fn(&str) -> String),
                (st::PHONE, st::PHONE_TIP, p.phone.as_deref(), &mut d.show_phone, mask_phone),
            ] {
                super::super::appearance::row(ui, label.get(lang), tip.get(lang), |ui| match value {
                    None => {
                        ui.label(st::NONE.get(lang));
                    }
                    Some(v) if streaming => {
                        ui.add(egui::Label::new(mask(v)).truncate()).on_hover_text(crate::i18n::table::s2::t::HIDDEN_STREAMER.get(lang));
                    }
                    Some(v) => {
                        let text = if *shown { v.to_string() } else { mask(v) };
                        let toggle = if *shown { st::HIDE } else { st::REVEAL };
                        if crate::ui::widgets::compact(ui, toggle.get(lang)).clicked() {
                            *shown = !*shown;
                        }
                        ui.add(egui::Label::new(text).truncate());
                    }
                });
            }
            let yes_no = |b: bool| if b { st::YES.get(lang) } else { st::NO.get(lang) };
            value_row(ui, st::VERIFIED.get(lang), st::VERIFIED_TIP.get(lang), yes_no(p.verified));
            value_row(ui, st::MFA.get(lang), st::MFA_TIP.get(lang), if p.mfa { st::MFA_ON.get(lang) } else { st::MFA_OFF.get(lang) });
            let nitro = match p.premium {
                0 => st::NITRO_NONE,
                1 => st::NITRO_CLASSIC,
                3 => st::NITRO_BASIC,
                _ => st::NITRO_FULL,
            };
            value_row(ui, st::NITRO.get(lang), st::NITRO_TIP.get(lang), nitro.get(lang));
            }));
            });
            if edit {
                self.params.show(super::Page::Profile);
            }
        }

        section(ui, t, st::IN_DISCORD_ONLY.get(lang));
        note(ui, t, st::IN_DISCORD_ONLY_TIP.get(lang));
        for (label, tip) in [
            (st::CHANGE_USERNAME, st::NEEDS_PASSWORD),
            (st::CHANGE_EMAIL, st::NEEDS_PASSWORD),
            (st::CHANGE_PASSWORD, st::NEEDS_PASSWORD),
            (st::MFA_MANAGE, st::NEEDS_CODE),
            (st::DELETE_ACCOUNT, st::NEEDS_PASSWORD),
        ] {
            in_discord(ui, lang, label.get(lang), tip.get(lang));
        }

        section(ui, t, st::SAVED_ACCOUNTS.get(lang));
        note(ui, t, st::SAVED_ACCOUNTS_TIP.get(lang));
        if ui.button(st::OPEN_ACCOUNT_SWITCHER.get(lang)).clicked() {
            self.params.open = false;
            self.menu_open = true;
        }
    }

    pub(super) fn page_profile(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        if self.store.account.profile == Remote::Idle {
            self.store.account.load_profile();
        }
        let h24 = self.h24();
        if remote_line(ui, t, lang, &self.store.account.profile, h24)
            && let Some(p) = self.store.account.profile.ready().cloned()
        {
            let busy = self.store.account.profile_busy();
            let d = &mut self.params.drafts;
            // Nouveau profil reçu (ou enregistré) : le brouillon repart de lui
            // s'il n'avait pas été modifié.
            let reset = match &d.profile {
                Some((base, draft)) => *base != p && *draft == ProfileDraft::of(base, None),
                None => true,
            };
            if reset {
                d.profile = Some((p.clone(), ProfileDraft::of(&p, None)));
            }
            let mut save = None;
            if let Some((base, draft)) = &mut d.profile {
                super::super::appearance::row(ui, st::DISPLAY_NAME.get(lang), st::DISPLAY_NAME_TIP.get(lang), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.global_name).char_limit(32).desired_width(f32::INFINITY));
                });
                super::super::appearance::row(ui, st::PRONOUNS.get(lang), st::PRONOUNS_TIP.get(lang), |ui| {
                    ui.add(egui::TextEdit::singleline(&mut draft.pronouns).char_limit(40).desired_width(f32::INFINITY));
                });
                // Même encre que les libellés des lignes voisines (il restait gris).
                ui.label(RichText::new(st::BIO.get(lang)).color(ui.visuals().strong_text_color())).on_hover_text(st::BIO_TIP.get(lang));
                ui.add(egui::TextEdit::multiline(&mut draft.bio).char_limit(190).desired_rows(3).desired_width(f32::INFINITY));
                super::super::appearance::row(ui, st::ACCENT.get(lang), st::ACCENT_TIP.get(lang), |ui| {
                    let mut on = draft.accent.is_some();
                    if ui.add(egui::Checkbox::without_text(&mut on)).changed() {
                        draft.accent = on.then_some(draft.accent.unwrap_or_default());
                    }
                    if let Some(c) = &mut draft.accent {
                        ui.color_edit_button_srgb(c);
                    }
                });
                let edit = draft.edit(base, None);
                ui.add_space(t.spacing.item_y);
                ui.horizontal(|ui| {
                    if ui.add_enabled(!busy && !edit.is_empty(), egui::Button::new(st::SAVE.get(lang))).clicked() {
                        save = Some(edit.clone());
                    }
                    if ui.add_enabled(!busy && !edit.is_empty(), egui::Button::new(st::CANCEL.get(lang))).clicked() {
                        *draft = ProfileDraft::of(base, None);
                    }
                    if busy {
                        ui.label(RichText::new(st::SAVING.get(lang)).color(t.colors.muted));
                    }
                });
            }
            if let Some(edit) = save {
                match self.store.account.save_profile(edit) {
                    // Le brouillon devient la nouvelle base : refusé, la page
                    // repart du profil rétabli.
                    Ok(()) => self.params.drafts.profile = None,
                    Err(r) => self.refused(r),
                }
            }
        }
        self.avatar_editor(ui, t);

        section(ui, t, st::SERVER_NICK.get(lang));
        note(ui, t, st::SERVER_NICK_TIP.get(lang));
        let guilds: Vec<(u64, String, Option<String>)> =
            self.store.guilds.iter().map(|g| (g.id, super::super::words::guild_name(lang, &g.name).to_string(), g.me.as_ref().and_then(|m| m.nick.clone()))).collect();
        let d = &mut self.params.drafts;
        let current = d.nick_guild.and_then(|id| guilds.iter().find(|g| g.0 == id));
        let mut pick = None;
        egui::ComboBox::from_id_salt("pseudo_serveur")
            .selected_text(current.map(|g| g.1.as_str()).unwrap_or(st::CHOOSE_SERVER.get(lang)))
            .width(ui.available_width())
            .show_ui(ui, |ui| {
                for g in &guilds {
                    if ui.selectable_label(d.nick_guild == Some(g.0), &g.1).clicked() {
                        pick = Some(g);
                    }
                }
            });
        if let Some(g) = pick {
            d.nick_guild = Some(g.0);
            d.nick = g.2.clone().unwrap_or_default();
        }
        if let Some(guild) = d.nick_guild {
            let saving = self.store.account.nick_saving.is_some();
            let mut send = false;
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut d.nick).char_limit(32).hint_text(st::NICK_HINT.get(lang)));
                send = ui.add_enabled(!saving, egui::Button::new(st::SAVE.get(lang))).clicked();
                if saving {
                    ui.label(RichText::new(st::SAVING.get(lang)).color(t.colors.muted));
                }
            });
            if send {
                let nick = Some(d.nick.trim().to_string());
                if let Err(r) = self.store.account.set_nick(guild, nick) {
                    self.refused(r);
                }
            }
        }
    }

    pub(super) fn page_devices(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let h24 = self.h24();
        if self.store.account.sessions == Remote::Idle {
            self.store.account.load_sessions();
        }
        note(ui, t, st::DEVICES_TIP.get(lang));
        if crate::ui::widgets::compact(ui, st::REFRESH.get(lang)).clicked() {
            self.store.account.load_sessions();
        }
        if !remote_line(ui, t, lang, &self.store.account.sessions, h24) {
            return;
        }
        let list = self.store.account.sessions.ready().cloned().unwrap_or_default();
        if list.is_empty() {
            ui.label(RichText::new(st::NOTHING.get(lang)).color(t.colors.muted));
        }
        let mut action = None;
        for s in &list {
            ui.add_space(t.spacing.item_y);
            egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_embed()).inner_margin(t.embed_margin()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let unknown = st::UNKNOWN.get(lang);
                let what = format!("{} · {}", s.platform.as_deref().unwrap_or(unknown), s.os.as_deref().unwrap_or(unknown));
                ui.label(RichText::new(what).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                let place = s.location.as_deref().unwrap_or(unknown);
                let when = s.last_used_ms.map(|ms| time::clock(lang, ms, true, h24)).unwrap_or_else(|| unknown.to_string());
                ui.label(RichText::new(sf::session_seen(lang, place, when)).color(t.colors.muted));
                let asking = self.params.confirm == Some(Confirm::Logout(s.id_hash.clone()));
                ui.horizontal(|ui| {
                    if asking {
                        ui.label(RichText::new(st::LOGOUT_SESSION_CONFIRM.get(lang)).color(t.colors.danger));
                        if ui.button(st::LOGOUT_SESSION.get(lang)).clicked() {
                            action = Some((s.id_hash.clone(), true));
                        }
                        if ui.button(st::CANCEL.get(lang)).clicked() {
                            action = Some((s.id_hash.clone(), false));
                        }
                    } else if ui.button(st::LOGOUT_SESSION.get(lang)).clicked() {
                        self.params.confirm = Some(Confirm::Logout(s.id_hash.clone()));
                    }
                });
            });
        }
        if let Some((hash, go)) = action {
            self.params.confirm = None;
            if go && let Err(r) = self.store.account.logout_session(&hash) {
                self.refused(r);
            }
        }
    }

    pub(super) fn page_connections(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let h24 = self.h24();
        if self.store.account.connections == Remote::Idle {
            self.store.account.load_connections();
        }
        note(ui, t, st::CONNECTIONS_TIP.get(lang));
        if remote_line(ui, t, lang, &self.store.account.connections, h24) {
            let list = self.store.account.connections.ready().cloned().unwrap_or_default();
            if list.is_empty() {
                ui.label(RichText::new(st::NOTHING.get(lang)).color(t.colors.muted));
            }
            for c in &list {
                let mut flags = Vec::new();
                if c.verified {
                    flags.push(st::VERIFIED.get(lang));
                }
                flags.push(if c.visible { st::ON_PROFILE.get(lang) } else { st::HIDDEN_FROM_PROFILE.get(lang) });
                if c.revoked {
                    flags.push(st::REVOKED.get(lang));
                }
                // Le détail de la ligne dit déjà l'état : la valeur n'est que le nom (répété avec lui, il était tronqué).
                value_row(ui, &c.kind, &flags.join(", "), &c.name);
            }
        }
        in_discord(ui, lang, st::MANAGE_CONNECTIONS.get(lang), st::MANAGE_CONNECTIONS_TIP.get(lang));
    }

    pub(super) fn page_apps(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let h24 = self.h24();
        if self.store.account.apps == Remote::Idle {
            self.store.account.load_apps();
        }
        note(ui, t, st::APPS_TIP.get(lang));
        if !remote_line(ui, t, lang, &self.store.account.apps, h24) {
            return;
        }
        let list = self.store.account.apps.ready().cloned().unwrap_or_default();
        if list.is_empty() {
            ui.label(RichText::new(st::NOTHING.get(lang)).color(t.colors.muted));
        }
        let mut action = None;
        for a in &list {
            ui.add_space(t.spacing.item_y);
            egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_embed()).inner_margin(t.embed_margin()).show(ui, |ui| {
                ui.set_width(ui.available_width());
                let name = if a.name.is_empty() { st::UNKNOWN.get(lang) } else { &a.name };
                ui.label(RichText::new(name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                ui.label(RichText::new(sf::scopes(lang, a.scopes.join(", "))).small().color(t.colors.muted));
                ui.horizontal(|ui| {
                    if self.params.confirm == Some(Confirm::Revoke(a.id)) {
                        ui.label(RichText::new(sf::revoke_confirm(lang, name)).color(t.colors.danger));
                        if ui.button(st::REVOKE.get(lang)).clicked() {
                            action = Some((a.id, true));
                        }
                        if ui.button(st::CANCEL.get(lang)).clicked() {
                            action = Some((a.id, false));
                        }
                    } else if ui.button(st::REVOKE.get(lang)).clicked() {
                        self.params.confirm = Some(Confirm::Revoke(a.id));
                    }
                });
            });
        }
        if let Some((id, go)) = action {
            self.params.confirm = None;
            if go && let Err(r) = self.store.account.revoke_app(id) {
                self.refused(r);
            }
        }
    }

    pub(super) fn page_billing(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        note(ui, t, st::BILLING_TIP.get(lang));
        // État de l'abonnement : celui que le profil du compte porte déjà
        // (`premium_type`), sans rien demander de plus.
        let state = match self.store.account.profile.ready().map(|p| p.premium) {
            Some(0) => st::NITRO_NONE,
            Some(1) => st::NITRO_CLASSIC,
            Some(2) => st::NITRO_FULL,
            Some(3) => st::NITRO_BASIC,
            _ => st::NITRO_UNKNOWN,
        };
        value_row(ui, st::NITRO_STATE.get(lang), st::NITRO_STATE_TIP.get(lang), state.get(lang));
        for (label, tip) in [(st::NITRO, st::NITRO_MANAGE_TIP), (st::BILLING, st::BILLING_MANAGE_TIP), (st::GIFTS, st::GIFTS_TIP)] {
            in_discord(ui, lang, label.get(lang), tip.get(lang));
        }
    }

    pub(super) fn page_ferrocord(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        value_row(ui, st::VERSION.get(lang), st::VERSION_TIP.get(lang), env!("CARGO_PKG_VERSION"));
        section(ui, t, st::SAVED_ACCOUNTS.get(lang));
        note(ui, t, st::SAVED_ACCOUNTS_TIP.get(lang));
        if ui.button(st::OPEN_ACCOUNT_SWITCHER.get(lang)).clicked() {
            self.params.open = false;
            self.menu_open = true;
        }
        section(ui, t, st::UNOFFICIAL.get(lang));
        note(ui, t, st::UNOFFICIAL_TIP.get(lang));
        section(ui, t, crate::i18n::table::emoji::t::ATTRIBUTION.get(lang));
        note(ui, t, crate::i18n::table::emoji::t::ATTRIBUTION_TIP.get(lang));
        self.install_settings(ui, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_hide_all_but_domain_or_last_digits() {
        assert_eq!(mask_email("prenom.nom@example.invalid"), "••••••••@example.invalid");
        assert_eq!(mask_email("sans-arobase"), "••••••••");
        assert_eq!(mask_phone("+33 1 23 45 67 89"), "••••••6789");
        assert_eq!(mask_phone("12"), "••••••12");
    }

    #[test]
    fn profile_edit_sends_only_what_changed() {
        let base = Profile { global_name: Some("Nom".into()), bio: "bio".into(), accent: Some(0x112233), ..Default::default() };
        let mut d = ProfileDraft::of(&base, None);
        assert!(d.edit(&base, None).is_empty());
        d.bio = "nouvelle".into();
        d.global_name = "  ".into();
        let e = d.edit(&base, None);
        assert_eq!(e.bio.as_deref(), Some("nouvelle"));
        assert_eq!(e.global_name, Some(None), "nom vidé : retiré");
        assert_eq!((e.pronouns, e.accent, e.theme), (None, None, None));
        d.accent = Some([0xff, 0, 0]);
        assert_eq!(d.edit(&base, None).accent, Some(Some(0xff0000)));

    }
}
