//! Fenêtres des serveurs (lot A1) : « Ajouter un serveur » (créer le mien,
//! depuis un modèle, rejoindre par une invitation), confirmation de
//! « Quitter le serveur », paramètres d'un dossier du rail ; messages de fin
//! de ces actions. L'état du rail (dossiers dépliés, glisser-déposer) vit
//! aussi ici, dans [`ServersUi`].

use super::invites_manage::InvitesUi;
use super::rail_dnd::RailDnd;
use super::{App, ToastText};
use crate::guild_icon::{IconPicker, Outcome, Picked};
use crate::i18n::table::guilds::{f, p, t as tx};
use crate::i18n::Lang;
use crate::state::guild_folders::{self, Edit, Item};
use crate::state::servers::{Failure, ServerAction, ServerNotice, NAME_MAX};
use crate::state::usersettings::schema::Folder;
use crate::state::usersettings::Refusal;
use crate::theme::Tokens;
use eframe::egui::{self, Color32, Id, Key, RichText};
use std::collections::HashSet;
use std::sync::Arc;

/// Adresse de la découverte de serveurs (aucune route publique ne la donne
/// sans se faire passer pour le client officiel : ouverte dans le navigateur).
pub(super) const DISCOVERY_URL: &str = "https://discord.com/discovery";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Choose,
    Create,
    Template,
    Join,
}

pub(super) struct Dialog {
    pub page: Page,
    name: String,
    template: String,
    invite: String,
    icon: Option<Picked>,
    icon_tex: Option<egui::TextureHandle>,
    icon_failed: bool,
}

/// Paramètres d'un dossier ouverts : entrée, nom saisi, couleur.
pub(super) struct FolderEdit {
    pub entry: usize,
    pub name: String,
    pub color: Option<u64>,
}

/// Rail tel qu'affiché, refait seulement quand les dossiers ou les serveurs changent.
#[derive(Default)]
pub(super) struct RailCache {
    folders: Vec<Folder>,
    known: Vec<u64>,
    pub items: Vec<Item>,
    /// Taille de la liste complète (serveurs absents des réglages compris).
    pub entries: usize,
}

impl RailCache {
    pub(super) fn refresh(&mut self, folders: &[Folder], guilds: &[crate::state::model::Guild]) {
        if self.folders == folders && self.known.iter().copied().eq(guilds.iter().map(|g| g.id)) && !(self.items.is_empty() && !guilds.is_empty()) {
            return;
        }
        self.folders = folders.to_vec();
        self.known = guilds.iter().map(|g| g.id).collect();
        self.items = guild_folders::layout(&self.folders, &self.known);
        self.entries = guild_folders::missing(&self.folders, &self.known).len() + self.folders.len();
    }
}

#[derive(Default)]
pub(super) struct ServersUi {
    pub dnd: RailDnd,
    /// Dossiers dépliés (clé : [`folder_key`]).
    pub open_folders: HashSet<u64>,
    pub cache: RailCache,
    pub dialog: Option<Dialog>,
    /// « Quitter le serveur » demandé : serveur à confirmer.
    pub leave: Option<u64>,
    pub folder_edit: Option<FolderEdit>,
    pub invites: InvitesUi,
    icon: Option<IconPicker>,
}

/// Clé d'un dossier, stable tant qu'il existe : son identifiant, ou son
/// premier serveur s'il n'en a pas.
pub(super) fn folder_key(id: Option<i64>, guilds: &[(u64, usize)]) -> u64 {
    id.map_or_else(|| guilds.first().map_or(0, |g| g.0), |i| i as u64)
}

pub(super) fn color_of(c: u64) -> Color32 {
    Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

fn action_word(lang: Lang, a: ServerAction) -> &'static str {
    match a {
        ServerAction::Join => tx::ACTION_JOIN.get(lang),
        ServerAction::Leave => tx::ACTION_LEAVE.get(lang),
        ServerAction::Create => tx::ACTION_CREATE.get(lang),
        ServerAction::Template => tx::ACTION_TEMPLATE.get(lang),
        ServerAction::Invite => tx::ACTION_INVITE.get(lang),
        ServerAction::Invites => tx::ACTION_INVITES.get(lang),
        ServerAction::Revoke => tx::ACTION_REVOKE.get(lang),
    }
}

pub(super) fn failure_text(lang: Lang, f: &Failure, h24: bool) -> String {
    match f {
        Failure::Problem(p) => super::words::problem(lang, p, h24),
        Failure::Captcha => tx::CAPTCHA.get(lang).to_string(),
        Failure::NotFound => tx::NOT_FOUND.get(lang).to_string(),
        Failure::InvitesDisabled => tx::INVITES_DISABLED.get(lang).to_string(),
    }
}

pub(super) fn notice_text(lang: Lang, n: &ServerNotice, h24: bool) -> String {
    match n {
        ServerNotice::Joined { name, verify: false } => f::joined(lang, name),
        ServerNotice::Joined { name, verify: true } => f::joined_verify(lang, name),
        ServerNotice::Left { name } => f::left(lang, name),
        // Un captcha se dit seul : il dit déjà quoi faire.
        ServerNotice::Failed { failure: Failure::Captcha, .. } => tx::CAPTCHA.get(lang).to_string(),
        ServerNotice::Failed { action, failure } => f::failed(lang, action_word(lang, *action), failure_text(lang, failure, h24)),
        ServerNotice::BadCode => tx::BAD_CODE.get(lang).to_string(),
        ServerNotice::Owner => tx::OWNER_CANNOT_LEAVE.get(lang).to_string(),
    }
}

fn primary(t: &Tokens, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(t.colors.on_accent)).fill(t.colors.accent)
}

impl App {
    /// Ouvre « Ajouter un serveur » sur une page (rail, aperçu).
    pub(super) fn open_server_dialog(&mut self, page: Page) {
        let me = self.store.me.as_ref().map(|u| u.name.clone()).unwrap_or_default();
        let name: String = f::default_name(self.lang, &me).chars().take(NAME_MAX).collect();
        self.servers.dialog = Some(Dialog { page, name, template: String::new(), invite: String::new(), icon: None, icon_tex: None, icon_failed: false });
    }

    /// « Rejoindre un serveur » prérempli par un lien d'invitation (lot C6) :
    /// rejoindre reste un clic de l'utilisateur.
    pub(super) fn open_join_with(&mut self, code: &str) {
        self.open_server_dialog(Page::Join);
        if let Some(d) = self.servers.dialog.as_mut() {
            d.invite = code.to_string();
        }
    }

    /// Fenêtre « Ajouter un serveur », sur la page `page` (aperçu : « choisir », « creer », « modele », « rejoindre »).
    pub fn show_server_dialog(&mut self, page: &str) -> bool {
        let p = match page {
            "choisir" => Page::Choose,
            "creer" => Page::Create,
            "modele" => Page::Template,
            "rejoindre" => Page::Join,
            _ => return false,
        };
        self.open_server_dialog(p);
        true
    }

    /// Déplie le dernier dossier du rail (aperçu, tests) ; `false` s'il n'y en a pas.
    pub fn unfold_rail_folders(&mut self) -> bool {
        let known: Vec<u64> = self.store.guilds.iter().map(|g| g.id).collect();
        let items = guild_folders::layout(&self.store.account.snapshot().folders, &known);
        let Some(key) = items.iter().rev().find_map(|i| if let Item::Folder { id, guilds, .. } = i { Some(folder_key(*id, guilds)) } else { None }) else { return false };
        self.servers.open_folders.insert(key);
        true
    }

    /// Écrit une modification des dossiers du rail ; un refus est annoncé.
    pub(super) fn write_folders(&mut self, edit: Edit) {
        let known: Vec<u64> = self.store.guilds.iter().map(|g| g.id).collect();
        let seed = self.now_secs().wrapping_mul(1000).wrapping_add(u64::from(self.ctx.cumulative_frame_nr() as u32));
        match guild_folders::write(&mut self.store.account, &known, &edit, seed) {
            Ok(_) => {}
            Err(Refusal::Unknown) => self.notify(tx::FOLDERS_UNKNOWN),
            Err(Refusal::Busy) => self.notify(tx::FOLDERS_BUSY),
        }
    }

    /// Fenêtres des serveurs et messages de fin des actions (chaque image).
    pub(super) fn servers_ui(&mut self, ui: &mut egui::Ui) {
        while let Some(n) = self.store.servers.take_notice() {
            let text = notice_text(self.lang, &n, self.h24());
            self.notify(ToastText::Text(text));
        }
        if let Some(done) = self.store.servers.take_done()
            && matches!(done, ServerAction::Join | ServerAction::Create)
        {
            self.servers.dialog = None;
            self.scroll_to_bottom = true;
        }
        self.server_dialog(ui);
        self.leave_dialog(ui);
        self.folder_dialog(ui);
        self.invite_dialogs(ui);
        self.verification_dialog(ui);
    }

    fn poll_icon(&mut self) {
        let Some(out) = self.servers.icon.as_ref().and_then(IconPicker::poll) else { return };
        if let Some(d) = self.servers.dialog.as_mut() {
            match out {
                Outcome::Ready(p) => {
                    d.icon = Some(p);
                    d.icon_tex = None;
                    d.icon_failed = false;
                }
                Outcome::Unreadable => d.icon_failed = true,
            }
        }
    }

    fn pick_icon(&mut self) {
        let ctx = self.ctx.clone();
        let picker = self.servers.icon.get_or_insert_with(|| IconPicker::new(Arc::new(move || ctx.request_repaint())));
        #[cfg(windows)]
        picker.pick(tx::ICON_PICK.get(self.lang));
        #[cfg(not(windows))]
        let _ = picker;
    }

    /// Icône (aperçu) puis nom : partie commune de « Créer le mien » et du modèle.
    fn name_and_icon(&mut self, ui: &mut egui::Ui) -> bool {
        let t = self.theme.clone();
        let lang = self.lang;
        let busy = self.servers.icon.as_ref().is_some_and(IconPicker::busy);
        let Some(d) = self.servers.dialog.as_mut() else { return false };
        let (mut pick, mut submit) = (false, false);
        ui.horizontal(|ui| {
            let side = t.layout.rail_button;
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
            match &d.icon {
                Some(p) => {
                    // Texture faite une fois par image choisie.
                    let tex = d.icon_tex.get_or_insert_with(|| ui.ctx().load_texture("icone-nouveau-serveur", p.image.clone(), egui::TextureOptions::LINEAR));
                    ui.painter().add(crate::images::textured(rect, t.r_rail(true), tex.id()));
                }
                None => {
                    ui.painter().rect_filled(rect, t.r_rail(true), t.colors.pill);
                    let font = egui::FontId::proportional(t.typography.rail);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, super::util::initials(&d.name), font, t.colors.text_strong);
                }
            }
            ui.vertical(|ui| {
                if IconPicker::CAN_PICK {
                    pick = ui.add_enabled(!busy, egui::Button::new(tx::ICON_PICK.get(lang))).clicked();
                }
                if d.icon.is_some() && ui.button(tx::ICON_REMOVE.get(lang)).clicked() {
                    d.icon = None;
                    d.icon_tex = None;
                }
            });
        });
        if d.icon_failed {
            ui.label(RichText::new(tx::ICON_UNREADABLE.get(lang)).color(t.colors.danger));
        }
        ui.label(RichText::new(tx::SERVER_NAME.get(lang)).small().color(t.colors.muted));
        let field = ui.add(egui::TextEdit::singleline(&mut d.name).char_limit(NAME_MAX).desired_width(f32::INFINITY));
        submit |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        if pick {
            self.pick_icon();
        }
        submit
    }

    fn server_dialog(&mut self, ui: &mut egui::Ui) {
        if self.servers.dialog.is_none() {
            return;
        }
        self.poll_icon();
        let t = self.theme.clone();
        let lang = self.lang;
        let mut close = false;
        let Some(page) = self.servers.dialog.as_ref().map(|d| d.page) else { return };
        let title = match page {
            Page::Choose | Page::Create | Page::Template => tx::CREATE_SERVER,
            Page::Join => tx::JOIN_SERVER,
        };
        // Gabarit commun (titre et croix) ; chaque page garde ses actions.
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::sheet(ui.ctx(), &t, Id::unique("ajouter-serveur"), width, title.get(lang), crate::i18n::t::CLOSE_TIP.get(lang), |ui| {
                crate::ui::settings::shell::page_style(ui, &t);
                match page {
                    Page::Choose => close |= self.page_choose(ui),
                    Page::Create => close |= self.page_create(ui),
                    Page::Template => close |= self.page_template(ui),
                    Page::Join => close |= self.page_join(ui),
                }
            });
        if close || modal.closed {
            self.servers.dialog = None;
        }
    }

    fn set_page(&mut self, p: Page) {
        if let Some(d) = self.servers.dialog.as_mut() {
            d.page = p;
        }
    }

    fn page_choose(&mut self, ui: &mut egui::Ui) -> bool {
        let t = self.theme.clone();
        let lang = self.lang;
        ui.label(RichText::new(tx::CREATE_TIP.get(lang)).color(t.colors.muted));
        let wide = |ui: &mut egui::Ui, text: &str| ui.add_sized([ui.available_width(), t.layout.button_height], egui::Button::new(text.to_string())).clicked();
        if wide(ui, tx::CREATE_MY_OWN.get(lang)) {
            self.set_page(Page::Create);
        }
        if wide(ui, tx::FROM_TEMPLATE.get(lang)) {
            self.set_page(Page::Template);
        }
        ui.separator();
        ui.label(RichText::new(tx::HAVE_INVITE.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
        if wide(ui, tx::JOIN_SERVER.get(lang)) {
            self.set_page(Page::Join);
        }
        if ui.link(tx::DISCOVER.get(lang)).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(DISCOVERY_URL));
        }
        ui.button(tx::CANCEL.get(lang)).clicked()
    }

    fn page_create(&mut self, ui: &mut egui::Ui) -> bool {
        let t = self.theme.clone();
        let lang = self.lang;
        let enter = self.name_and_icon(ui);
        let busy = self.store.servers.pending(ServerAction::Create);
        let Some(d) = self.servers.dialog.as_ref() else { return true };
        let ok = (crate::state::servers::NAME_MIN..=NAME_MAX).contains(&d.name.trim().chars().count()) && !busy;
        let (mut yes, mut back) = (false, false);
        ui.horizontal(|ui| {
            yes = ui.add_enabled(ok, primary(&t, tx::CREATE.get(lang))).clicked();
            back = ui.button(tx::BACK.get(lang)).clicked();
            if busy {
                ui.spinner();
            }
        });
        if (yes || (enter && ok))
            && let Some(d) = self.servers.dialog.as_ref()
        {
            let (name, icon) = (d.name.clone(), d.icon.as_ref().map(|p| p.data_uri.clone()));
            self.store.create_guild(&name, icon);
        }
        if back {
            self.set_page(Page::Choose);
        }
        false
    }

    fn page_template(&mut self, ui: &mut egui::Ui) -> bool {
        let t = self.theme.clone();
        let lang = self.lang;
        let loading = self.store.servers.pending(ServerAction::Template);
        let creating = self.store.servers.pending(ServerAction::Create);
        let mut ask = false;
        if let Some(d) = self.servers.dialog.as_mut() {
            ui.label(RichText::new(tx::TEMPLATE_LINK.get(lang)).small().color(t.colors.muted));
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut d.template).hint_text(tx::TEMPLATE_HINT.get(lang)).desired_width(ui.available_width() * 0.7));
                ask = ui.add_enabled(!loading && !d.template.trim().is_empty(), egui::Button::new(tx::PREVIEW.get(lang))).clicked();
                ask |= field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) && !d.template.trim().is_empty();
                if loading {
                    ui.spinner();
                }
            });
        }
        if ask && let Some(code) = self.servers.dialog.as_ref().map(|d| d.template.clone()) {
            self.store.load_template(&code);
        }
        let preview = self.store.servers.template.clone();
        if let Some(tp) = &preview {
            egui::Frame::new().fill(t.colors.bg1).corner_radius(t.r_embed()).inner_margin(t.embed_margin()).show(ui, |ui| {
                ui.label(RichText::new(&tp.name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                if !tp.description.is_empty() {
                    ui.label(RichText::new(&tp.description).color(t.colors.text));
                }
                ui.label(RichText::new(p::used(lang, tp.usage_count)).small().color(t.colors.faint));
                if !tp.channels.is_empty() {
                    ui.label(RichText::new(tx::TEMPLATE_CHANNELS.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
                    ui.label(RichText::new(tp.channels.join(", ")).small().color(t.colors.muted));
                }
            });
        }
        let mut enter = false;
        if preview.is_some() {
            enter = self.name_and_icon(ui);
        }
        let Some(d) = self.servers.dialog.as_ref() else { return true };
        let ok = preview.is_some() && !creating && (crate::state::servers::NAME_MIN..=NAME_MAX).contains(&d.name.trim().chars().count());
        let (mut yes, mut back) = (false, false);
        ui.horizontal(|ui| {
            yes = ui.add_enabled(ok, primary(&t, tx::CREATE.get(lang))).clicked();
            back = ui.button(tx::BACK.get(lang)).clicked();
            if creating {
                ui.spinner();
            }
        });
        if (yes || (enter && ok))
            && let (Some(tp), Some(d)) = (&preview, self.servers.dialog.as_ref())
        {
            let (name, icon) = (d.name.clone(), d.icon.as_ref().map(|p| p.data_uri.clone()));
            self.store.create_from_template(&tp.code, &name, icon);
        }
        if back {
            self.store.servers.template = None;
            self.set_page(Page::Choose);
        }
        false
    }

    fn page_join(&mut self, ui: &mut egui::Ui) -> bool {
        let t = self.theme.clone();
        let lang = self.lang;
        let busy = self.store.servers.pending(ServerAction::Join);
        let Some(d) = self.servers.dialog.as_mut() else { return true };
        ui.label(RichText::new(tx::INVITE_LINK.get(lang)).small().color(t.colors.muted));
        let field = ui.add(egui::TextEdit::singleline(&mut d.invite).hint_text(tx::INVITE_HINT.get(lang)).desired_width(f32::INFINITY));
        if !field.has_focus() && d.invite.is_empty() {
            field.request_focus();
        }
        ui.label(RichText::new(tx::JOIN_TIP.get(lang)).small().color(t.colors.faint));
        let ok = !busy && !d.invite.trim().is_empty();
        let enter = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
        let (mut yes, mut back) = (false, false);
        ui.horizontal(|ui| {
            yes = ui.add_enabled(ok, primary(&t, tx::JOIN.get(lang))).clicked();
            back = ui.button(tx::BACK.get(lang)).clicked();
            if busy {
                ui.spinner();
            }
        });
        if (yes || (enter && ok))
            && let Some(code) = self.servers.dialog.as_ref().map(|d| d.invite.clone())
        {
            self.store.join_invite(&code);
        }
        if back {
            self.set_page(Page::Choose);
        }
        false
    }

    /// Confirmation de « Quitter le serveur ».
    fn leave_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.servers.leave else { return };
        let Some(name) = self.store.guilds.iter().find(|g| g.id == id).map(|g| super::words::guild_name(self.lang, &g.name).to_string()) else {
            self.servers.leave = None;
            return;
        };
        let t = self.theme.clone();
        let lang = self.lang;
        let (mut yes, mut no) = (false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            &t,
            Id::unique("quitter-serveur"),
            width,
            tx::LEAVE_SERVER.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.add(egui::Label::new(RichText::new(f::leave_question(lang, &name)).color(t.colors.text)).wrap());
            },
            |ui| {
                yes = crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Danger, tx::LEAVE.get(lang)).clicked();
                no = crate::ui::widgets::button(ui, &t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        if yes {
            self.store.leave_guild(id);
            self.servers.leave = None;
        } else if no || modal.closed {
            self.servers.leave = None;
        }
    }

    /// Paramètres d'un dossier : nom et couleur.
    fn folder_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(fe) = self.servers.folder_edit.as_mut() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let (mut save, mut cancel) = (false, false);
        let (mut entered, mut saved) = (false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), &t, 1.0);
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            &t,
            Id::unique("dossier-serveurs"),
            width,
            tx::FOLDER_SETTINGS.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, &t);
                let field = ui.add(egui::TextEdit::singleline(&mut fe.name).hint_text(tx::FOLDER_NAME_HINT.get(lang)).char_limit(NAME_MAX).desired_width(f32::INFINITY));
                entered = field.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    ui.label(tx::FOLDER_COLOR.get(lang));
                    // Couleur choisie par l'utilisateur (donnée du compte, pas du thème).
                    let current = fe.color.map_or(t.colors.accent, color_of);
                    let mut rgb = [current.r(), current.g(), current.b()];
                    if egui::color_picker::color_edit_button_srgb(ui, &mut rgb).changed() {
                        fe.color = Some((u64::from(rgb[0]) << 16) | (u64::from(rgb[1]) << 8) | u64::from(rgb[2]));
                    }
                    if fe.color.is_some() && ui.button(tx::FOLDER_DEFAULT_COLOR.get(lang)).clicked() {
                        fe.color = None;
                    }
                });
            },
            |ui| {
                use crate::ui::widgets::{button, Kind};
                saved = button(ui, &t, Kind::Primary, tx::SAVE.get(lang)).clicked();
                cancel = button(ui, &t, Kind::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        save |= entered || saved;
        if save {
            if let Some(fe) = self.servers.folder_edit.take() {
                let name = Some(fe.name).filter(|n| !n.trim().is_empty());
                self.write_folders(Edit::Settings { entry: fe.entry, name, color: fe.color });
            }
        } else if cancel || modal.closed {
            self.servers.folder_edit = None;
        }
    }
}
