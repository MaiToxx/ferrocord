//! Fenêtre « Paramètres du serveur » (phase 3), partagée par les lots
//! d'administration : catégories à gauche, page à droite, Échap ou la croix
//! la ferment, une seule colonne quand la fenêtre est étroite (même tenue
//! que la fenêtre Paramètres, `ui::settings`).
//!
//! Contrat commun : chaque page vit dans son fichier (`<page>.rs`) et n'est
//! listée que si j'ai la permission qui la concerne ; ce fichier ne contient
//! que la fenêtre et l'aiguillage. Ouverture : clic droit sur un serveur >
//! « Paramètres du serveur ». Les données viennent de `store.guild_admin`
//! (demandées à l'ouverture, oubliées à la fermeture).
//!
//! Visibilité, règle commune : droits inconnus (`Authority::Unknown`), rien
//! n'est proposé.

pub(super) mod audit_log;
pub(super) mod automod;
pub(super) mod bans;
mod boosts;
mod channels;
mod emojis;
mod expr_common;
// Widget, modèle de serveur, élagage (groupe 2, lot V).
mod extras;
pub(super) mod community;
pub(super) mod events;
mod integrations;
pub(super) mod member_roles;
mod members;
pub(super) mod onboarding;
mod other;
mod overview;
mod role_perms;
pub(super) mod roles;
pub(super) mod safety;
mod sounds;
mod stickers;
pub(super) mod webhooks;
pub(super) mod welcome;

use super::settings::nav::entry_row;
use super::settings::{note, section, shell};
use super::App;
use crate::i18n::table::admin::{f, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::admin::guild::{AdminAction, AdminNotice, Failure};
use crate::state::perms::{Authority, Permissions};
use crate::state::Store;
use crate::theme::{icons::Icon, Tokens, SETTINGS_WINDOW};
use eframe::egui::{self, Align, Id, Key, Layout, Modifiers, RichText};

/// Fenêtre ouverte : sur quel serveur, à quelle page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuildSettings {
    pub guild_id: u64,
    pub page: Page,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    Overview,
    /// Emojis, autocollants et sons du serveur (lot A6).
    Emojis,
    Stickers,
    Sounds,
    /// Rôles et permissions (lot A3).
    Roles,
    /// Salons et catégories (lot A2).
    Channels,
    Integrations,
    Webhooks,
    Insights,
    /// Découverte et communauté, écran de bienvenue, intégration, événements programmés (lot A7).
    Community,
    Welcome,
    Onboarding,
    Events,
    Boosts,
    /// Modération (lots A4 + A5) : réglages de sécurité, AutoMod, journal d'audit, bannissements.
    Safety,
    AutoMod,
    AuditLog,
    Bans,
    /// Membres connus, leurs rôles (lot A3).
    Members,
    /// Transfert de propriété et suppression du serveur (renvoyés à l'application officielle).
    Ownership,
    /// Widget, modèle de serveur, élagage des membres inactifs (groupe 2, lot V).
    Widget,
    Template,
    Prune,
}

/// Catégories de la colonne de gauche, dans l'ordre de Discord ; `None` :
/// le nom du serveur.
const GROUPS: [(Option<Text>, &[Page]); 7] = [
    (None, &[Page::Overview, Page::Roles, Page::Channels, Page::Widget, Page::Template]),
    (Some(crate::i18n::table::expressions::t::EXPRESSIONS), &[Page::Emojis, Page::Stickers, Page::Sounds]),
    (Some(tx::GROUP_APPS), &[Page::Integrations, Page::Webhooks]),
    (Some(crate::i18n::table::moderation::t::GROUP_MODERATION), &[Page::Safety, Page::AutoMod, Page::AuditLog, Page::Bans]),
    (Some(tx::GROUP_COMMUNITY), &[Page::Community, Page::Welcome, Page::Onboarding, Page::Events, Page::Insights]),
    (Some(tx::GROUP_MONETIZATION), &[Page::Boosts]),
    (Some(crate::i18n::table::roles::t::GROUP_USER_MANAGEMENT), &[Page::Members, Page::Prune]),
];

/// Pages sous un séparateur, en bas de la colonne.
const LAST: &[Page] = &[Page::Ownership];

impl Page {
    pub fn title(self) -> Text {
        match self {
            Page::Overview => tx::PAGE_OVERVIEW,
            Page::Emojis => crate::i18n::table::expressions::t::PAGE_EMOJIS,
            Page::Stickers => crate::i18n::table::expressions::t::PAGE_STICKERS,
            Page::Sounds => crate::i18n::table::expressions::t::PAGE_SOUNDS,
            Page::Roles => crate::i18n::table::roles::t::PAGE_ROLES,
            Page::Members => crate::i18n::table::roles::t::PAGE_MEMBERS,
            Page::Safety => crate::i18n::table::moderation::t::PAGE_SAFETY,
            Page::AutoMod => crate::i18n::table::moderation::t::PAGE_AUTOMOD,
            Page::AuditLog => crate::i18n::table::moderation::t::PAGE_AUDIT,
            Page::Bans => crate::i18n::table::moderation::t::PAGE_BANS,
            Page::Channels => crate::i18n::table::chadmin::t::PAGE_CHANNELS,
            Page::Integrations => tx::PAGE_INTEGRATIONS,
            Page::Webhooks => tx::PAGE_WEBHOOKS,
            Page::Insights => tx::PAGE_INSIGHTS,
            Page::Community => crate::i18n::table::events::t::PAGE_COMMUNITY,
            Page::Welcome => crate::i18n::table::events::t::PAGE_WELCOME,
            Page::Onboarding => crate::i18n::table::events::t::PAGE_ONBOARDING,
            Page::Events => crate::i18n::table::events::t::PAGE_EVENTS,
            Page::Boosts => tx::PAGE_BOOSTS,
            Page::Ownership => tx::PAGE_OWNERSHIP,
            Page::Widget => crate::i18n::table::lotv::t::PAGE_WIDGET,
            Page::Template => crate::i18n::table::lotv::t::PAGE_TEMPLATE,
            Page::Prune => crate::i18n::table::lotv::t::PAGE_PRUNE,
        }
    }

    /// Icône de l'entrée dans la colonne (même tenue que les paramètres utilisateur).
    fn icon(self) -> Icon {
        match self {
            Page::Overview => Icon::Gear,
            Page::Emojis => Icon::Smile,
            Page::Stickers => Icon::Sticker,
            Page::Sounds => Icon::Audio,
            Page::Roles => Icon::ShieldLock,
            Page::Members => Icon::Members,
            Page::Safety => Icon::ChatShield,
            Page::AutoMod => Icon::Sliders,
            Page::AuditLog => Icon::File,
            Page::Bans => Icon::Flag,
            Page::Channels => Icon::Hash,
            Page::Integrations => Icon::Puzzle,
            Page::Webhooks => Icon::Link,
            Page::Insights => Icon::Signal,
            Page::Community => Icon::Compass,
            Page::Welcome => Icon::Wave,
            Page::Onboarding => Icon::Check,
            Page::Events => Icon::Calendar,
            Page::Boosts => Icon::Boost,
            Page::Ownership => Icon::Crown,
            Page::Widget => Icon::Apps,
            Page::Template => Icon::Copy,
            Page::Prune => Icon::Hourglass,
        }
    }

    /// La page est-elle pour moi ? Droits inconnus (`Authority::Unknown`) :
    /// non, quelle que soit la page.
    pub fn allowed(self, store: &Store, guild: u64) -> bool {
        if store.role_authority(guild) == Authority::Unknown {
            return false;
        }
        let has = |p| store.guild_has(guild, p);
        match self {
            Page::Overview | Page::Integrations | Page::Safety | Page::AutoMod | Page::Widget | Page::Template => has(Permissions::MANAGE_GUILD),
            Page::Prune => store.can_prune(guild),
            Page::Emojis | Page::Stickers | Page::Sounds => store.can_manage_expressions(guild),
            Page::AuditLog => has(Permissions::VIEW_AUDIT_LOG),
            Page::Bans => has(Permissions::BAN_MEMBERS),
            Page::Roles => has(Permissions::MANAGE_ROLES),
            Page::Members => [Permissions::MANAGE_ROLES, Permissions::MANAGE_NICKNAMES, Permissions::KICK_MEMBERS, Permissions::BAN_MEMBERS, Permissions::MODERATE_MEMBERS].into_iter().any(has),
            Page::Channels => store.can_manage_channels(guild),
            Page::Webhooks => has(Permissions::MANAGE_WEBHOOKS) || webhooks::channels(store, guild).next().is_some(),
            Page::Insights => has(Permissions::VIEW_GUILD_INSIGHTS),
            Page::Community | Page::Welcome => store.can_manage_guild(guild),
            Page::Onboarding => store.can_edit_onboarding(guild),
            Page::Events => store.can_create_events(guild),
            Page::Boosts => true,
            Page::Ownership => store.owns_guild(guild),
        }
    }

    /// Toutes les pages, dans l'ordre de la colonne.
    fn all() -> impl Iterator<Item = Page> {
        GROUPS.iter().flat_map(|(_, p)| p.iter().copied()).chain(LAST.iter().copied())
    }

    /// Sorte d'expression d'une page Emojis, Autocollants ou Sons (lot A6).
    fn kind(self) -> Option<crate::state::admin::expressions::Kind> {
        use crate::state::admin::expressions::Kind;
        match self {
            Page::Emojis => Some(Kind::Emoji),
            Page::Stickers => Some(Kind::Sticker),
            Page::Sounds => Some(Kind::Sound),
            _ => None,
        }
    }
}

#[derive(Default)]
pub(super) struct GuildSettingsUi {
    pub win: Option<GuildSettings>,
    /// Fenêtre étroite : la liste des pages est affichée à la place de la page.
    nav_shown: bool,
    overview: overview::Draft,
    hooks: webhooks::HooksUi,
    integrations: integrations::IntegrationsUi,
    extras: extras::ExtrasUi,
    /// Page dont les données ont été demandées (modération, événements, accueil), une fois par ouverture.
    mod_loaded: Option<Page>,
    /// Emojis, autocollants et sons (lot A6) : choix de fichier (créé au premier envoi) et formulaires.
    picker: Option<crate::expr_pick::ExprPicker>,
    form: expr_common::Forms,
}

/// Échec d'une action, en mots.
pub(super) fn failure_text(lang: Lang, fl: &Failure, h24: bool) -> String {
    match fl {
        Failure::Problem(p) => super::words::problem(lang, p, h24),
        Failure::Forbidden => tx::FORBIDDEN.get(lang).into(),
        Failure::NotFound => tx::NOT_FOUND.get(lang).into(),
        Failure::Unreadable => tx::UNREADABLE.get(lang).into(),
    }
}

fn action_word(lang: Lang, a: AdminAction) -> &'static str {
    match a {
        AdminAction::Load => tx::ACTION_LOAD.get(lang),
        AdminAction::Save => tx::ACTION_SAVE.get(lang),
        AdminAction::Vanity => tx::ACTION_VANITY.get(lang),
        AdminAction::Webhooks => tx::ACTION_WEBHOOKS.get(lang),
        AdminAction::SaveWebhook => tx::ACTION_SAVE_WEBHOOK.get(lang),
        AdminAction::DeleteWebhook => tx::ACTION_DELETE_WEBHOOK.get(lang),
        AdminAction::Integrations => tx::ACTION_INTEGRATIONS.get(lang),
        AdminAction::RemoveIntegration => tx::ACTION_REMOVE_INTEGRATION.get(lang),
        AdminAction::CommandPerms => tx::ACTION_COMMAND_PERMS.get(lang),
        AdminAction::Extra(x) => extras::action_word(lang, x),
    }
}

pub(super) fn notice_text(lang: Lang, n: &AdminNotice, h24: bool) -> String {
    match n {
        AdminNotice::Saved => tx::SAVED.get(lang).into(),
        AdminNotice::WebhookCreated => tx::WEBHOOK_CREATED.get(lang).into(),
        AdminNotice::WebhookDeleted => tx::WEBHOOK_DELETED.get(lang).into(),
        AdminNotice::IntegrationRemoved => tx::INTEGRATION_REMOVED.get(lang).into(),
        AdminNotice::Extra(n) => extras::notice_text(lang, n),
        // L'A2F se dit seule : elle dit déjà quoi faire.
        AdminNotice::Failed { failure: Failure::Problem(crate::state::Problem::MfaRequired), .. } => tx::MFA_REQUIRED.get(lang).into(),
        AdminNotice::Failed { action, failure } => f::failed(lang, action_word(lang, *action), failure_text(lang, failure, h24)),
    }
}

/// Ligne « en cours » ou « échec » d'une donnée demandée.
pub(super) fn loading_or_failed<T>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, h24: bool, l: Option<&crate::state::admin::guild::Load<T>>) -> bool {
    use crate::state::admin::guild::Load;
    match l {
        Some(Load::Ready(_)) => false,
        Some(Load::Failed(fl)) => {
            ui.add(egui::Label::new(RichText::new(failure_text(lang, fl, h24)).color(t.colors.danger)).wrap());
            true
        }
        _ => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(RichText::new(tx::LOADING.get(lang)).color(t.colors.muted));
            });
            true
        }
    }
}

impl App {
    /// Ouvre la fenêtre sur ce serveur (`page` : sinon la première permise).
    pub(super) fn open_guild_settings(&mut self, guild_id: u64, page: Option<Page>) {
        if !self.store.can_open_guild_settings(guild_id) {
            return;
        }
        let Some(page) = page.filter(|p| p.allowed(&self.store, guild_id)).or_else(|| Page::all().find(|p| p.allowed(&self.store, guild_id))) else { return };
        if self.guild_settings.win.map(|w| w.guild_id) != Some(guild_id) {
            self.guild_settings = GuildSettingsUi::default();
        }
        self.guild_settings.win = Some(GuildSettings { guild_id, page });
        self.guild_settings.nav_shown = false;
        // Formulaires d'expressions (lot A6) : refaits à chaque ouverture.
        self.guild_settings.form = Default::default();
        // Rôles et Membres (lot A3) : leur état est refait à chaque ouverture.
        self.roles_admin.reset();
        self.store.open_guild_admin(guild_id);
    }

    /// Ouverture depuis l'aperçu et les tests ; `false` si la page n'est pas permise.
    pub fn show_guild_settings(&mut self, guild_id: u64, page: Page) -> bool {
        self.open_guild_settings(guild_id, Some(page));
        self.flush_commands();
        self.guild_settings.win.is_some_and(|w| w.page == page)
    }

    fn close_guild_settings(&mut self) {
        self.roles_admin.reset();
        self.guild_settings = GuildSettingsUi::default();
        self.store.forget_guild_admin();
    }

    pub(super) fn guild_settings_ui(&mut self, ui: &mut egui::Ui) {
        // Rôles et membres (lot A3) : messages, et dialogue d'un membre (ouvert aussi depuis un profil).
        self.roles_tick();
        self.member_dialog_ui(ui);
        // Emojis, autocollants et sons (lot A6) : messages lus fenêtre ouverte ou non.
        self.expr_notices();
        while let Some(n) = self.store.guild_admin.take_notice() {
            let text = notice_text(self.lang, &n, self.h24());
            self.notify(super::ToastText::Text(text));
        }
        let Some(win) = self.guild_settings.win else { return };
        // Serveur quitté ou permission perdue : la fenêtre se ferme.
        if !self.store.can_open_guild_settings(win.guild_id) {
            self.close_guild_settings();
            return;
        }
        if !win.page.allowed(&self.store, win.guild_id) {
            match Page::all().find(|p| p.allowed(&self.store, win.guild_id)) {
                Some(p) => self.guild_settings.win = Some(GuildSettings { page: p, ..win }),
                None => {
                    self.close_guild_settings();
                    return;
                }
            }
        }
        // Fichier choisi pour une expression (lot A6), lu par son fil.
        self.expr_poll(ui.ctx());
        // Modération (lots A4 + A5), événements et accueil (lot A7) : données
        // demandées une fois par page ouverte.
        if let Some(w) = self.guild_settings.win
            && self.guild_settings.mod_loaded != Some(w.page)
            && self.online()
        {
            let open: Option<fn(&mut App, u64)> = match w.page {
                Page::Safety => Some(safety::open),
                Page::AutoMod => Some(automod::open),
                Page::AuditLog => Some(audit_log::open),
                Page::Bans => Some(bans::open),
                Page::Events => Some(|app: &mut App, g: u64| {
                    app.store.load_events(g);
                }),
                Page::Welcome => Some(|app: &mut App, g: u64| {
                    app.events_ui.welcome_draft = None;
                    app.store.load_welcome(g);
                }),
                Page::Onboarding => Some(|app: &mut App, g: u64| {
                    app.events_ui.onboarding_draft = None;
                    app.store.load_onboarding(g);
                }),
                _ => None,
            };
            if let Some(open) = open {
                self.guild_settings.mod_loaded = Some(w.page);
                open(self, w.guild_id);
            }
        }
        // Une fenêtre du dessus (« Modifier le salon », création, invitations,
        // confirmations des rôles et de la modération, fenêtres des événements)
        // prend Échap d'abord.
        let above = self.params.open
            || self.chan_ui.is_open()
            || self.servers.invites.is_open()
            || self.roles_admin.dialog_open()
            || self.moderation.dialog_open()
            || self.events_ui.editor.is_some()
            || self.events_ui.list.is_some();
        if !above && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.close_guild_settings();
            return;
        }
        let t = self.theme.clone();
        let g = SETTINGS_WINDOW;
        let screen = ui.ctx().content_rect();
        // Disposition mobile (phase 7) : plein écran, une colonne, bouton retour.
        let mobile = self.mobile_active();
        // Plein écran par-dessus l'application (`shell`).
        let size = screen.size();
        let frame = egui::Frame::new().fill(t.colors.bg2);
        let mut back = false;
        let mut close = false;
        egui::Window::new(tx::TITLE.get(self.lang))
            .id(Id::unique("parametres_serveur"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(frame)
            .show(ui.ctx(), |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                if !shell::wide(ui.ctx()) {
                    egui::Frame::new().inner_margin(g.inner_margin()).show(ui, |ui| {
                        let inner = size - g.inner_margin().sum();
                        ui.set_min_size(inner);
                        ui.set_max_size(inner);
                        let mut body = |ui: &mut egui::Ui| {
                            if mobile && super::mobile::back_row(ui, &t, self.lang, tx::TITLE.get(self.lang)) {
                                back = true;
                                return;
                            }
                            if self.guild_settings.nav_shown {
                                self.guild_settings_nav(ui, &t);
                            } else {
                                if !mobile && ui.add(crate::ui::form::btn(&t, tx::ALL_PAGES.get(self.lang))).clicked() {
                                    self.guild_settings.nav_shown = true;
                                }
                                self.guild_settings_page(ui, &t);
                            }
                        };
                        if mobile {
                            super::mobile::clipped(ui, body);
                        } else {
                            body(ui);
                        }
                    });
                    return;
                }
                let full = ui.max_rect();
                let zones = shell::layout(full);
                shell::backdrop(ui, &t, full, &zones);
                ui.scope_builder(egui::UiBuilder::new().max_rect(zones.nav).layout(Layout::top_down(Align::Min)), |ui| self.guild_settings_nav(ui, &t));
                ui.scope_builder(egui::UiBuilder::new().max_rect(zones.page).layout(Layout::top_down(Align::Min)), |ui| {
                    ui.set_max_size(zones.page.size());
                    shell::page_style(ui, &t);
                    self.guild_settings_page(ui, &t);
                });
                close = shell::close_button(ui, &t, self.lang, zones.close);
            });
        if close {
            self.close_guild_settings();
        }
        if back {
            self.guild_settings_back();
        }
    }

    /// Retour (disposition mobile, phase 7) : de la page à la liste des
    /// pages, puis fermeture ; `false` si la fenêtre n'est pas ouverte.
    pub(super) fn guild_settings_back(&mut self) -> bool {
        if self.guild_settings.win.is_none() {
            return false;
        }
        if self.guild_settings.nav_shown {
            self.close_guild_settings();
        } else {
            self.guild_settings.nav_shown = true;
        }
        true
    }

    fn guild_settings_nav(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let Some(win) = self.guild_settings.win else { return };
        let lang = self.lang;
        let gname = self.store.guilds.iter().find(|g| g.id == win.guild_id).map(|g| super::words::guild_name(lang, &g.name).to_string()).unwrap_or_default();
        let mut go = None;
        egui::ScrollArea::vertical().id_salt("parametres_serveur_nav").auto_shrink([false, false]).show(ui, |ui| {
            // Mêmes lignes que la colonne des paramètres utilisateur (`settings::nav`).
            ui.spacing_mut().item_spacing.y = (t.spacing.item_y / 2.0).round();
            ui.add_space(t.spacing.item_x);
            // Page ouverte d'ailleurs (menu du serveur, lien) : son entrée est amenée en vue,
            // une fois par page (rien ne bouge si elle y est déjà).
            let seen_id = Id::unique("parametres_serveur_nav_vue");
            let now = (win.guild_id, win.page as u8);
            let reveal = ui.data(|d| d.get_temp::<(u64, u8)>(seen_id)) != Some(now);
            if reveal {
                ui.data_mut(|d| d.insert_temp(seen_id, now));
            }
            let mut row = |ui: &mut egui::Ui, p: Page| {
                if !p.allowed(&self.store, win.guild_id) {
                    return;
                }
                let r = entry_row(ui, t, p.icon(), p.title().get(lang), None, None, win.page == p);
                if reveal && win.page == p {
                    ui.scroll_to_rect(r.rect, None);
                }
                if r.clicked() {
                    go = Some(p);
                }
            };
            for (group, pages) in GROUPS {
                if !pages.iter().any(|p| p.allowed(&self.store, win.guild_id)) {
                    continue;
                }
                let head = group.map_or_else(|| gname.clone(), |g| g.get(lang).to_string());
                if group.is_some() {
                    shell::nav_divider(ui, t);
                }
                shell::nav_title(ui, t, &head);
                for p in pages {
                    row(ui, *p);
                }
            }
            if LAST.iter().any(|p| p.allowed(&self.store, win.guild_id)) {
                shell::nav_divider(ui, t);
                for p in LAST {
                    row(ui, *p);
                }
            }
        });
        if let Some(p) = go {
            if p != win.page {
                self.roles_admin.reset();
                self.guild_settings.form = Default::default();
            }
            self.guild_settings.win = Some(GuildSettings { page: p, ..win });
            self.guild_settings.nav_shown = false;
        }
    }

    fn guild_settings_page(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let Some(win) = self.guild_settings.win else { return };
        let lang = self.lang;
        let close = shell::header(ui, t, lang, win.page.title().get(lang));
        if close {
            self.close_guild_settings();
            return;
        }
        // Emojis, autocollants et sons (lot A6) : liste REST demandée à la première ouverture.
        if let Some(kind) = win.page.kind() {
            if !self.online() {
                note(ui, t, crate::i18n::table::s::t::OFFLINE.get(lang));
                return;
            }
            let g = win.guild_id;
            if self.store.expressions.loaded(g, kind).is_none() {
                self.store.load_expressions(g, kind);
            }
            ui.vertical(|ui| {
                ui.set_max_width(ui.available_width().min(shell::PAGE_WIDTH));
                match win.page {
                    Page::Emojis => emojis::show(self, ui, t, g),
                    Page::Stickers => stickers::show(self, ui, t, g),
                    _ => sounds::show(self, ui, t, g),
                }
            });
            return;
        }
        // Rôles, Membres et modération gèrent leur défilement (listes virtualisées).
        if matches!(win.page, Page::Roles | Page::Members | Page::Safety | Page::AutoMod | Page::AuditLog | Page::Bans) {
            if !self.online() {
                note(ui, t, crate::i18n::table::s::t::OFFLINE.get(lang));
                return;
            }
            let g = win.guild_id;
            match win.page {
                Page::Roles => roles::show(self, ui, t, g),
                Page::Members => members::show(self, ui, t, g),
                page => {
                    ui.scope(|ui| {
                        ui.set_max_width(ui.available_width().min(shell::PAGE_WIDTH * 1.3));
                        match page {
                            // Page plus haute que la fenêtre : elle défile dans sa zone (sans cela, tout
                            // le gabarit, colonne et bouton ÉCHAP compris, s'ouvrait décalé).
                            Page::Safety => {
                                egui::ScrollArea::vertical().id_salt("parametres_serveur_securite").auto_shrink([false, false]).show(ui, |ui| safety::show(ui, self, g));
                            }
                            Page::AutoMod => automod::show(ui, self, g),
                            Page::AuditLog => audit_log::show(ui, self, g),
                            _ => bans::show(ui, self, g),
                        }
                    });
                }
            }
            return;
        }
        egui::ScrollArea::vertical().id_salt(("parametres_serveur_page", win.page as u8)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(shell::PAGE_WIDTH));
            // Sans session, rien n'est demandé (la demande serait perdue).
            if !self.online() && !matches!(win.page, Page::Insights | Page::Ownership) {
                note(ui, t, crate::i18n::table::s::t::OFFLINE.get(lang));
                return;
            }
            let g = win.guild_id;
            match win.page {
                Page::Overview => self.page_guild_overview(ui, t, g),
                Page::Channels => channels::show(self, ui, t, g),
                Page::Integrations => self.page_guild_integrations(ui, t, g),
                Page::Webhooks => self.page_guild_webhooks(ui, t, g),
                Page::Insights => self.page_guild_insights(ui, t, g),
                Page::Community => community::show(self, ui, t, g),
                Page::Welcome => welcome::show(self, ui, t, g),
                Page::Onboarding => onboarding::show(self, ui, t, g),
                Page::Events => events::show(self, ui, t, g),
                Page::Boosts => self.page_guild_boosts(ui, t, g),
                Page::Ownership => self.page_guild_ownership(ui, t, g),
                Page::Widget => self.page_guild_widget(ui, t, g),
                Page::Template => self.page_guild_template(ui, t, g),
                Page::Prune => self.page_guild_prune(ui, t, g),
                Page::Roles | Page::Members | Page::Safety | Page::AutoMod | Page::AuditLog | Page::Bans | Page::Emojis | Page::Stickers | Page::Sounds => {}
            }
        });
    }
}

/// Couleur d'un rôle (0xRRGGBB).
pub(super) fn rgb(c: u32) -> egui::Color32 {
    egui::Color32::from_rgb((c >> 16) as u8, (c >> 8) as u8, c as u8)
}

#[cfg(test)]
#[path = "tests.rs"]
mod expr_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_is_listed_once() {
        let all: Vec<Page> = Page::all().collect();
        for p in [Page::Overview, Page::Emojis, Page::Stickers, Page::Sounds, Page::Roles, Page::Channels, Page::Integrations, Page::Webhooks, Page::Safety, Page::AutoMod, Page::AuditLog, Page::Bans, Page::Community, Page::Welcome, Page::Onboarding, Page::Events, Page::Insights, Page::Boosts, Page::Members, Page::Ownership, Page::Widget, Page::Template, Page::Prune] {
            assert_eq!(all.iter().filter(|x| **x == p).count(), 1, "{p:?}");
        }
    }
}
