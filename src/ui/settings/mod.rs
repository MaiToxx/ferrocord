//! Fenêtre « Paramètres » : catégories à gauche (comme Discord), recherche
//! d'un réglage, page à droite. Roue dentée du pied ou Ctrl+, (qui ouvre la
//! page Apparence) ; Échap ou la croix la ferment.
//!
//! Les pages lisent `store.account` (réglages du compte Discord) et le thème
//! de Ferrocord ; elles n'envoient rien sans geste : ouvrir une page qui
//! affiche des données du compte les demande une fois, chaque écriture suit
//! un clic.

pub(super) mod account;
pub(super) mod avatar;
// Démarrage, liens, mises à jour, licences (lot C6).
mod install;
pub(super) mod local;
mod menu;
// Colonne de navigation à la façon de Discord (lot réglages).
pub(super) mod nav;
mod pages;
mod safety;
pub(super) mod shell;
pub(super) mod synced;
pub(super) mod voice;

pub(super) use menu::NotifTarget;

use super::{appearance, App};
use crate::i18n::table::s::t as st;
use crate::i18n::table::s2::t as s2;
use crate::i18n::{Lang, Text};
use crate::state::usersettings::Refusal;
use crate::theme::{Tokens, SETTINGS_WINDOW};
use eframe::egui::{self, Align, Id, Key, Layout, Modifiers, RichText, Sense};

/// Adresse ouverte par « Ouvrir dans Discord » : l'application web officielle
/// (ce qui exige un mot de passe, un code ou un paiement s'y fait).
pub(super) const DISCORD_WEB: &str = "https://discord.com/app";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    MyAccount,
    Profile,
    Status,
    Privacy,
    Apps,
    Devices,
    Connections,
    Billing,
    Appearance,
    TextImages,
    Notifications,
    Language,
    Folders,
    Ferrocord,
    Accessibility,
    Streamer,
    Advanced,
    DataPrivacy,
    Activity,
    About,
    /// Raccourcis clavier (lot navigation).
    Keybinds,
    /// Voix et vidéo (lot V5).
    Voice,
    /// Fonctions propres à Ferrocord (lot P0, `ui::plugins`).
    Plugins,
}

/// Toutes les pages, dans l'ordre de la colonne (palette de commandes, lot E2).
pub(in crate::ui) fn pages() -> impl Iterator<Item = Page> {
    nav::pages()
}

impl Page {
    pub fn title(self) -> Text {
        match self {
            Page::MyAccount => st::PAGE_ACCOUNT,
            Page::Profile => st::PAGE_PROFILE,
            Page::Status => st::PAGE_STATUS,
            Page::Privacy => st::PAGE_PRIVACY,
            Page::Apps => st::PAGE_APPS,
            Page::Devices => st::PAGE_DEVICES,
            Page::Connections => st::PAGE_CONNECTIONS,
            Page::Billing => st::PAGE_BILLING,
            Page::Appearance => crate::i18n::t::APPEARANCE,
            Page::TextImages => st::PAGE_TEXT_IMAGES,
            Page::Notifications => st::PAGE_NOTIFICATIONS,
            Page::Language => st::PAGE_LANGUAGE,
            Page::Folders => st::PAGE_FOLDERS,
            Page::Ferrocord => st::PAGE_FERROCORD,
            Page::Accessibility => s2::PAGE_ACCESSIBILITY,
            Page::Streamer => s2::PAGE_STREAMER,
            Page::Advanced => s2::PAGE_ADVANCED,
            Page::DataPrivacy => s2::PAGE_DATA,
            Page::Activity => s2::PAGE_ACTIVITY,
            Page::About => s2::PAGE_ABOUT,
            Page::Keybinds => crate::i18n::table::nav::t::SHORTCUTS,
            Page::Voice => crate::i18n::table::audio::t::PAGE_VOICE,
            Page::Plugins => crate::i18n::table::plug::t::PAGE,
        }
    }
}

/// Ce que la recherche parcourt : chaque réglage affiché, avec sa page.
const INDEX: &[(Page, Text)] = &[
    (Page::MyAccount, st::USERNAME),
    (Page::MyAccount, st::EMAIL),
    (Page::MyAccount, st::PHONE),
    (Page::MyAccount, st::MFA),
    (Page::MyAccount, st::NITRO),
    (Page::MyAccount, st::CHANGE_PASSWORD),
    (Page::MyAccount, st::CHANGE_EMAIL),
    (Page::MyAccount, st::DELETE_ACCOUNT),
    (Page::MyAccount, st::SAVED_ACCOUNTS),
    (Page::Profile, st::DISPLAY_NAME),
    (Page::Profile, st::PRONOUNS),
    (Page::Profile, st::BIO),
    (Page::Profile, st::ACCENT),
    (Page::Profile, st::AVATAR_BANNER),
    (Page::Profile, st::SERVER_NICK),
    (Page::Status, st::ONLINE_STATUS),
    (Page::Status, st::CUSTOM_STATUS),
    (Page::Status, st::SHOW_GAME),
    (Page::Privacy, st::GUILD_DMS),
    (Page::Privacy, st::MESSAGE_REQUESTS),
    (Page::Privacy, st::FRIEND_REQUESTS),
    (Page::Privacy, st::EXPLICIT_FILTER),
    (Page::Privacy, crate::i18n::table::nd::t::SAFETY),
    (Page::Privacy, st::SPAM_FILTER),
    (Page::Apps, st::PAGE_APPS),
    (Page::Devices, st::PAGE_DEVICES),
    (Page::Connections, st::PAGE_CONNECTIONS),
    (Page::Billing, st::NITRO),
    (Page::Billing, st::BILLING),
    (Page::Appearance, crate::i18n::t::PRESET),
    (Page::Appearance, st::DISCORD_THEME),
    (Page::Appearance, st::COMPACT),
    (Page::TextImages, st::INLINE_ATTACHMENTS),
    (Page::TextImages, st::INLINE_EMBEDS),
    (Page::TextImages, st::RENDER_EMBEDS),
    (Page::TextImages, st::RENDER_REACTIONS),
    (Page::TextImages, st::ANIMATE_EMOJI),
    (Page::TextImages, st::GIF_AUTOPLAY),
    (Page::TextImages, st::ANIMATE_STICKERS),
    (Page::TextImages, st::CONVERT_EMOTICONS),
    (Page::TextImages, st::IMAGE_DESCRIPTIONS),
    (Page::TextImages, st::COMMAND_SUGGESTIONS),
    (Page::TextImages, st::TTS),
    (Page::Notifications, st::IN_APP_NOTIFICATIONS),
    (Page::Notifications, st::GO_LIVE),
    (Page::Notifications, st::BURST_REACTIONS),
    (Page::Notifications, st::SERVER_NOTIFICATIONS),
    (Page::Notifications, crate::i18n::table::notify::t::TOASTS),
    (Page::Notifications, crate::i18n::table::notify::t::SOUNDS),
    (Page::Notifications, crate::i18n::table::notify::t::TRAY),
    (Page::Notifications, crate::i18n::table::notify::t::CLOSE_TO_TRAY),
    (Page::Language, crate::i18n::t::LANGUAGE),
    (Page::Language, st::ACCOUNT_LOCALE),
    (Page::Folders, st::PAGE_FOLDERS),
    (Page::Ferrocord, st::VERSION),
    (Page::Ferrocord, crate::i18n::table::install::t::AUTOSTART),
    (Page::Ferrocord, crate::i18n::table::install::t::PROTOCOL),
    (Page::Ferrocord, crate::i18n::table::install::t::SEC_UPDATES),
    (Page::Accessibility, s2::MOTION),
    (Page::Accessibility, s2::CONTRAST),
    (Page::Accessibility, s2::SATURATION),
    (Page::Accessibility, s2::SIZE_ZOOM),
    (Page::Accessibility, s2::UNDERLINE),
    (Page::Accessibility, s2::ROLE_COLORS),
    (Page::Accessibility, s2::INSTANT_TIPS),
    (Page::Streamer, s2::STREAMER_ON),
    (Page::Streamer, s2::HIDE_PERSONAL),
    (Page::Streamer, s2::HIDE_INVITES),
    (Page::Streamer, s2::MUTE_SOUNDS),
    (Page::Streamer, s2::MUTE_TOASTS),
    (Page::Advanced, s2::DEV_MODE),
    (Page::Advanced, s2::HW_ACCEL),
    (Page::Advanced, s2::DESKTOP_IDENTITY),
    (Page::Advanced, s2::IDENTITY_VERSIONS),
    (Page::Advanced, s2::NET_HEALTH),
    (Page::Advanced, s2::DEBUG_LOG),
    (Page::DataPrivacy, s2::ACCESS_DETECT),
    (Page::DataPrivacy, s2::USAGE_STATS),
    (Page::DataPrivacy, s2::PERSONALIZATION),
    (Page::DataPrivacy, s2::REQUEST_DATA),
    (Page::DataPrivacy, crate::i18n::table::offline::t::KEEP),
    (Page::Activity, st::SHOW_GAME),
    (Page::Activity, s2::JOIN_FRIENDS),
    (Page::Activity, s2::JOIN_VOICE),
    (Page::Activity, s2::GUILDS_DEFAULT),
    (Page::About, st::VERSION),
    (Page::About, s2::SEC_LICENSES),
    (Page::About, s2::REPO),
    (Page::Profile, s2::AVATAR),
    (Page::Profile, s2::BANNER),
    (Page::Keybinds, crate::i18n::table::nav::t::ACT_QUICK_SWITCHER),
    (Page::Keybinds, crate::i18n::table::nav::t::ACT_INBOX),
    (Page::Voice, crate::i18n::table::audio::t::INPUT_DEVICE),
    (Page::Voice, crate::i18n::table::audio::t::OUTPUT_DEVICE),
    (Page::Voice, crate::i18n::table::audio::t::SEC_TEST),
    (Page::Voice, crate::i18n::table::audio::t::MODE_PTT),
    (Page::Voice, crate::i18n::table::audio::t::MODE_VOICE),
    (Page::Voice, crate::i18n::table::audio::t::NOISE),
    (Page::Voice, crate::i18n::table::audio::t::ECHO),
    (Page::Voice, crate::i18n::table::audio::t::AGC),
    (Page::Voice, crate::i18n::table::audio::t::CAMERA),
    (Page::Voice, crate::i18n::table::audio::t::VIDEO_TEST),
    (Page::Voice, crate::i18n::table::audio::t::CAMERA_PREVIEW),
    (Page::Voice, crate::i18n::table::audio::t::SHARE_QUALITY),
    (Page::Voice, crate::i18n::table::audio::t::SHARE_SOUND),
    // Personnalisation du client (lot PERSO).
    (Page::Appearance, crate::i18n::table::perso::t::SEC_THEMES),
    (Page::Appearance, crate::i18n::table::perso::t::ACCENT),
    (Page::Appearance, crate::i18n::table::perso::t::GRADIENT_ON),
    (Page::Appearance, crate::i18n::table::perso::t::IMAGE),
    (Page::Appearance, crate::i18n::table::perso::t::AUTO),
    (Page::Appearance, crate::i18n::table::perso::t::UI_FONT),
    (Page::Appearance, crate::i18n::table::perso::t::CODE_FONT),
    (Page::Appearance, crate::i18n::table::perso::t::NAME_COLOR),
    (Page::Appearance, crate::i18n::table::perso::t::HIDE_RAIL),
    (Page::Appearance, crate::i18n::table::perso::t::SEC_PROFILES),
];

/// Action irréversible attendant son deuxième clic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Confirm {
    Logout(String),
    Revoke(u64),
}

/// État de la fenêtre et brouillons des pages.
#[derive(Default)]
pub(super) struct SettingsWindow {
    pub open: bool,
    pub page: Option<Page>,
    /// « Déconnexion » choisie dans la colonne des catégories : traitée par le pied de la colonne des salons.
    pub logout_asked: bool,
    search: String,
    pub(super) confirm: Option<Confirm>,
    pub(super) drafts: account::Drafts,
    pub(super) status: synced::StatusDraft,
    pub(super) avatar: avatar::AvatarDraft,
    /// Fenêtre étroite (une seule colonne) : la liste des pages est affichée
    /// à la place de la page.
    nav_shown: bool,
    /// Section à amener en haut de la page (sous-entrée cliquée) : consommée
    /// au prochain dessin de la page.
    anchor: Option<egui::Id>,
    /// Section où la page est rendue (`shell::anchor_of`) : marque la
    /// sous-entrée courante de la colonne.
    spy: Option<egui::Id>,
    /// Section choisie par un clic : elle reste marquée tant que la page n'est
    /// pas défilée à la main (une section du bas n'atteint pas le haut).
    pin: Option<egui::Id>,
    /// Une page vient d'être ouverte : la colonne amène son entrée en vue.
    reveal: bool,
}

impl SettingsWindow {
    /// Fenêtre au lancement : rouverte sur Apparence si elle l'était à la fermeture.
    pub fn reopened(appearance_open: bool) -> Self {
        SettingsWindow { open: appearance_open, page: Some(Page::Appearance), reveal: true, ..Default::default() }
    }

    pub fn page(&self) -> Page {
        self.page.unwrap_or(Page::MyAccount)
    }

    pub fn show(&mut self, page: Page) {
        self.open = true;
        self.nav_shown = false;
        self.reveal = true;
        if self.page != Some(page) {
            self.confirm = None;
            (self.anchor, self.spy, self.pin) = (None, None, None);
        }
        self.page = Some(page);
    }

    /// Ouvre `page` depuis la colonne : sur la section intitulée `title`, ou,
    /// sans titre, en haut de la page si elle était déjà affichée (une page
    /// qu'on rouvre garde sinon sa position).
    fn show_at(&mut self, page: Page, title: Option<Text>, lang: Lang) {
        let same = self.page == Some(page) && !self.nav_shown;
        self.show(page);
        let section = title.map(|t| shell::anchor_of(t.get(lang)));
        self.anchor = section.or(same.then(shell::anchor_top));
        (self.spy, self.pin) = (section, section);
    }

    /// Ouverte sur la liste des pages (disposition mobile, phase 7).
    pub(in crate::ui) fn show_menu(&mut self) {
        self.open = true;
        self.nav_shown = true;
    }

    /// Retour (disposition mobile) : de la page à la liste des pages, puis fermeture.
    pub(in crate::ui) fn back(&mut self) {
        if self.nav_shown {
            self.open = false;
        } else {
            self.nav_shown = true;
        }
    }
}

/// Correspondance de la recherche : sans casse ni accents des lettres courantes.
pub(super) fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Entrées de l'index qui contiennent `query`, dans `lang`.
pub(super) fn search(lang: Lang, query: &str) -> Vec<(Page, Text)> {
    let q = fold(query.trim());
    if q.is_empty() {
        return Vec::new();
    }
    // Chaque plugin se trouve par son nom (lot P0).
    let plugins = crate::plugins::ALL.iter().map(|p| (Page::Plugins, p.name));
    INDEX.iter().copied().chain(plugins).filter(|(p, text)| fold(text.get(lang)).contains(&q) || fold(p.title().get(lang)).contains(&q)).collect()
}

impl App {
    /// Ouvre la fenêtre Paramètres sur `page`.
    pub(super) fn show_settings(&mut self, page: Page) {
        self.params.show(page);
    }

    /// Fenêtre Paramètres, dessinée par-dessus le reste.
    pub(super) fn settings_ui(&mut self, ui: &mut egui::Ui) {
        if !self.params.open {
            return;
        }
        // Combinaison attendue (page Raccourcis clavier) : Échap l'annule, la fenêtre reste.
        if self.nav.capturing.is_none() && !self.voice_cfg.capturing_key() && !self.local.plugins.modal_open() && !self.fx.share.modal_open() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.params.open = false;
            return;
        }
        if self.params.page() != Page::Keybinds {
            self.nav.capturing = None;
        }
        let t = self.theme.clone();
        let g = SETTINGS_WINDOW;
        let screen = ui.ctx().content_rect();
        // Disposition mobile (phase 7) : plein écran, une colonne, bouton retour.
        let mobile = self.mobile_active();
        // Plein écran par-dessus l'application (`shell`).
        let size = screen.size();
        let frame = egui::Frame::new().fill(t.colors.bg2);
        egui::Window::new(st::TITLE.get(self.lang))
            .id(Id::unique("parametres"))
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .fixed_size(size)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .frame(frame)
            .show(ui.ctx(), |ui| {
                crate::ui::motion::pop_window(ui);
                ui.set_min_size(size);
                ui.set_max_size(size);
                // Trop étroite pour deux colonnes (fenêtre étroite, zoom) :
                // la liste des pages ou la page, l'une à la place de l'autre.
                if !shell::wide_for(ui.ctx(), shell::USER_NAV_WIDTH) {
                    egui::Frame::new().inner_margin(g.inner_margin()).show(ui, |ui| {
                        let inner = size - g.inner_margin().sum();
                        ui.set_min_size(inner);
                        ui.set_max_size(inner);
                        let mut body = |ui: &mut egui::Ui| {
                            if mobile && super::mobile::back_row(ui, &t, self.lang, st::TITLE.get(self.lang)) {
                                self.params.back();
                                return;
                            }
                            if self.params.nav_shown {
                                self.settings_nav(ui, &t);
                            } else {
                                if !mobile && ui.button(st::ALL_SETTINGS.get(self.lang)).clicked() {
                                    self.params.nav_shown = true;
                                }
                                self.settings_page_frame(ui, &t);
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
                let zones = shell::layout_for(full, shell::USER_NAV_WIDTH);
                shell::backdrop(ui, &t, full, &zones);
                ui.scope_builder(egui::UiBuilder::new().max_rect(zones.nav).layout(Layout::top_down(Align::Min)), |ui| self.settings_nav(ui, &t));
                ui.scope_builder(egui::UiBuilder::new().max_rect(zones.page).layout(Layout::top_down(Align::Min)), |ui| {
                    shell::page_style(ui, &t);
                    self.settings_page_frame(ui, &t);
                });
                if shell::close_button(ui, &t, self.lang, zones.close) {
                    self.params.open = false;
                }
            });
    }

    fn settings_page_frame(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let page = self.params.page();
        if shell::header(ui, t, lang, page.title().get(lang)) {
            self.params.open = false;
        }
        // Repères des sections : la sous-entrée cliquée y dépose la section à
        // montrer, et la colonne y lit celle où la page est rendue.
        let goto = self.params.anchor.take();
        let top = goto == Some(shell::anchor_top());
        shell::anchors_begin(ui.ctx(), goto);
        // Lu avant la zone de défilement, qui consomme la molette.
        let moved = ui.input(|i| i.smooth_scroll_delta.y != 0.0 || i.pointer.primary_down());
        let out = egui::ScrollArea::vertical().id_salt(("parametres_page", page as u8)).auto_shrink([false, false]).show(ui, |ui| {
            if top {
                ui.scroll_to_cursor(Some(Align::TOP));
            }
            self.settings_page(ui, t, page);
        });
        let seen = shell::anchors_end(ui.ctx());
        // Défilement à la main (molette, barre) : la section suivie redevient celle de la position.
        let view = out.inner_rect;
        if moved && ui.rect_contains_pointer(view.expand(t.spacing.category_gap)) {
            self.params.pin = None;
        }
        let spy = self.params.pin.filter(|p| seen.iter().any(|(id, _)| id == p)).or_else(|| spy_of(&seen, view, out.state.clamped_offset().y, out.content_size.y));
        if spy != self.params.spy {
            self.params.spy = spy;
            // La colonne est dessinée avant la page : une image de plus pour la marque.
            ui.ctx().request_repaint();
        }
    }

    fn settings_page(&mut self, ui: &mut egui::Ui, t: &Tokens, page: Page) {
        let lang = self.lang;
        ui.set_max_width(ui.available_width().min(shell::PAGE_WIDTH));
        // Sans session, rien n'est demandé à Discord (la demande serait perdue).
        if page == Page::Notifications {
            // Réglages de cet ordinateur : sans session aussi.
            self.desk_settings(ui, t);
            self.keywords_settings(ui, t);
        }
        let needs_account = !matches!(page, Page::Appearance | Page::Language | Page::Billing | Page::Ferrocord | Page::Accessibility | Page::Streamer | Page::Advanced | Page::About | Page::Keybinds | Page::Voice | Page::Plugins);
        if needs_account && !self.online() {
            note(ui, t, st::OFFLINE.get(lang));
            // Historique hors ligne : réglage de cet ordinateur, sans session aussi (lot H).
            if page == Page::DataPrivacy {
                self.offline_settings(ui, t);
            }
            return;
        }
        match page {
            Page::MyAccount => self.page_my_account(ui, t),
            Page::Profile => self.page_profile(ui, t),
            Page::Status => self.page_status(ui, t),
            Page::Privacy => {
                self.page_privacy(ui, t);
                self.safety_section(ui, t);
            }
            Page::Apps => self.page_apps(ui, t),
            Page::Devices => self.page_devices(ui, t),
            Page::Connections => self.page_connections(ui, t),
            Page::Billing => self.page_billing(ui, t),
            Page::Appearance => {
                self.appearance_contents(ui, t);
                ui.add_space(t.spacing.category_gap);
                self.discord_appearance(ui, t);
                // Disposition mobile (phase 7).
                self.mobile_settings(ui, t);
            }
            Page::TextImages => self.page_text_images(ui, t),
            Page::Notifications => self.page_notifications(ui, t),
            Page::Language => self.page_language(ui, t),
            Page::Folders => self.page_folders(ui, t),
            Page::Ferrocord => self.page_ferrocord(ui, t),
            Page::Accessibility => self.page_accessibility(ui, t),
            Page::Streamer => self.page_streamer(ui, t),
            Page::Advanced => self.page_advanced(ui, t),
            Page::DataPrivacy => self.page_data(ui, t),
            Page::Activity => self.page_activity(ui, t),
            Page::About => self.page_about(ui, t),
            Page::Keybinds => self.page_keybinds(ui, t),
            Page::Voice => self.page_voice(ui, t),
            Page::Plugins => self.page_plugins(ui, t),
        }
    }

    /// Session ouverte et READY reçu : les demandes au compte peuvent partir.
    pub(super) fn online(&self) -> bool {
        self.backend.is_connected() && self.store.me.is_some()
    }

    /// Message pour un geste qui n'a rien envoyé.
    pub(super) fn refused(&mut self, r: Refusal) {
        self.notify(match r {
            Refusal::Busy => st::BUSY,
            Refusal::Unknown => st::NOT_LOADED,
        });
    }
}

/// Section où la page est rendue : la dernière dont le haut a passé le quart
/// supérieur de la vue ; page défilée jusqu'au bout : la dernière visible
/// (une section du bas n'atteint jamais le haut). `seen` : sections dans
/// l'ordre, avec leur haut à l'écran.
fn spy_of(seen: &[(egui::Id, f32)], view: egui::Rect, offset: f32, content: f32) -> Option<egui::Id> {
    let scrolls = content > view.height() + 1.0;
    let limit = if scrolls && offset >= content - view.height() - 1.0 { view.bottom() } else { view.top() + view.height() / 4.0 };
    // Tout en haut d'une page : aucune section n'est encore « passée », même si la première est proche.
    if offset <= 0.0 && scrolls {
        return None;
    }
    seen.iter().rev().find(|(_, top)| *top <= limit).map(|(id, _)| *id)
}

/// Ligne de la colonne des catégories (ou résultat de recherche, avec sa page).
pub(super) fn nav_row(ui: &mut egui::Ui, t: &Tokens, label: &str, page: Option<&str>, selected: bool) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), Sense::click());
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, t.r_row(), t.colors.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let color = if selected || resp.hovered() { t.colors.text_strong } else { t.colors.muted };
    let inner = rect.shrink2(egui::vec2(t.spacing.item_x, Default::default()));
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
    child.add(egui::Label::new(RichText::new(label).color(color)).truncate().selectable(false));
    if let Some(page) = page {
        child.add(egui::Label::new(RichText::new(page).small().color(t.colors.faint)).truncate().selectable(false));
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    super::keys::focus_ring(ui, t, &resp);
    resp
}

/// Titre de section dans une page.
pub(super) fn section(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    shell::section(ui, t, text);
}

/// Petit texte d'explication sous une section.
pub(super) fn note(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).small().color(t.colors.faint)).wrap());
}

/// Case à cocher d'un réglage (`None` : valeur inconnue, case grisée).
/// Renvoie la nouvelle valeur si l'utilisateur l'a changée.
pub(super) fn toggle(ui: &mut egui::Ui, label: &str, detail: &str, value: Option<bool>, enabled: bool) -> Option<bool> {
    let mut out = None;
    if super::mobile::is_active(ui.ctx()) {
        // Disposition mobile : libellé entier, case à droite, toute la ligne se touche.
        let on = enabled && value.is_some();
        let mut v = value.unwrap_or(false);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let boxed = ui.add_enabled(on, egui::Checkbox::without_text(&mut v));
                let size = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
                let text = ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), |ui| ui.add_enabled(on, egui::Label::new(label).wrap().sense(egui::Sense::click()))).inner.on_hover_text(detail);
                if boxed.changed() {
                    out = Some(v);
                } else if text.clicked() {
                    out = Some(!v);
                }
            });
        });
        return out;
    }
    shell::settings_row(ui, label, detail, true, |ui| {
        let mut v = value.unwrap_or(false);
        let resp = ui.add_enabled_ui(enabled && value.is_some(), |ui| shell::switch(ui, &mut v)).inner;
        crate::ui::a11y::name(ui.ctx(), resp.id, label);
        if resp.changed() {
            out = Some(v);
        }
    });
    out
}

/// Liste de choix d'un réglage ; renvoie le choix fait.
pub(super) fn choice<T: Copy + PartialEq>(ui: &mut egui::Ui, lang: Lang, id: &str, (label, detail): (&str, &str), options: &[(T, Text)], current: Option<T>, enabled: bool) -> Option<T> {
    let mut out = None;
    appearance::row(ui, label, detail, |ui| {
        let shown = options.iter().find(|(v, _)| Some(*v) == current).map(|(_, l)| l.get(lang)).unwrap_or(st::UNKNOWN.get(lang));
        ui.add_enabled_ui(enabled && current.is_some(), |ui| {
            egui::ComboBox::from_id_salt(id).selected_text(shown).width(ui.available_width()).truncate().show_ui(ui, |ui| {
                for (v, l) in options {
                    if ui.selectable_label(Some(*v) == current, l.get(lang)).clicked() {
                        out = Some(*v);
                    }
                }
            });
        });
    });
    out
}

/// Ligne « ceci se fait dans Discord » : ouvre l'application web officielle.
pub(super) fn in_discord(ui: &mut egui::Ui, lang: Lang, label: &str, detail: &str) {
    shell::settings_row(ui, label, detail, true, |ui| {
        if ui.button(st::OPEN_IN_DISCORD.get(lang)).on_hover_text(detail).clicked() {
            ui.ctx().open_url(egui::OpenUrl::new_tab(DISCORD_WEB));
        }
    });
}

/// Ligne en lecture seule : libellé, valeur.
pub(super) fn value_row(ui: &mut egui::Ui, label: &str, detail: &str, value: &str) {
    shell::settings_row(ui, label, detail, true, |ui| {
        ui.add(egui::Label::new(value).truncate());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_ignores_case_and_accents() {
        let hits = search(Lang::Fr, "ECRAN");
        assert!(hits.is_empty(), "{hits:?}");
        let hits = search(Lang::Fr, "emoji");
        assert!(hits.iter().any(|(p, _)| *p == Page::TextImages), "{hits:?}");
        let hits = search(Lang::Fr, "confidentialite");
        assert!(hits.iter().all(|(p, _)| matches!(p, Page::Privacy | Page::DataPrivacy | Page::Activity)) && hits.iter().any(|(p, _)| *p == Page::Privacy), "titre de page sans accent : {hits:?}");
        assert!(search(Lang::En, "password").iter().any(|(p, _)| *p == Page::MyAccount));
        assert!(search(Lang::Fr, "   ").is_empty());
    }

    /// Chaque page est atteinte par la colonne, une seule fois (entrée ou sous-entrée).
    #[test]
    fn every_page_is_in_the_column_once() {
        let seen: Vec<Page> = pages().collect();
        for p in [
            Page::MyAccount,
            Page::Profile,
            Page::Status,
            Page::Privacy,
            Page::Apps,
            Page::Devices,
            Page::Connections,
            Page::Billing,
            Page::Appearance,
            Page::TextImages,
            Page::Notifications,
            Page::Language,
            Page::Folders,
            Page::Ferrocord,
            Page::Accessibility,
            Page::Streamer,
            Page::Advanced,
            Page::DataPrivacy,
            Page::Activity,
            Page::About,
            Page::Keybinds,
            Page::Voice,
            Page::Plugins,
        ] {
            assert_eq!(seen.iter().filter(|x| **x == p).count(), 1, "{p:?}");
            assert_eq!(nav::entries().filter(|e| e.holds(p)).count(), 1, "{p:?} : une seule entrée s'ouvre");
        }
        assert_eq!(seen.len(), 23);
        // Une section visée appartient à une page de son entrée, et ses libellés sont distincts.
        for e in nav::entries() {
            for (i, s) in e.subs.iter().enumerate() {
                assert!(e.holds(s.page));
                for lang in [Lang::Fr, Lang::En] {
                    assert!(e.subs[..i].iter().all(|o| o.label.get(lang) != s.label.get(lang)), "{:?} en double", s.label);
                }
            }
        }
    }

    /// La recherche de la colonne trouve les entrées et les sous-entrées
    /// (avec leur section), puis les réglages de l'index, sans doublon.
    #[test]
    fn column_search_finds_entries_and_sub_entries() {
        let hits = nav::search(Lang::Fr, "mot de passe");
        assert!(hits.contains(&nav::Hit { label: st::SUB_PASSWORD, page: Page::MyAccount, anchor: Some(st::IN_DISCORD_ONLY) }), "{hits:?}");
        assert!(hits.iter().any(|h| h.label == st::CHANGE_PASSWORD && h.anchor.is_none()), "réglage de l'index : {hits:?}");
        let hits = nav::search(Lang::Fr, "COMPTE");
        assert_eq!(hits.first().map(|h| (h.label, h.page)), Some((st::NAV_ACCOUNT, Page::MyAccount)), "{hits:?}");
        let hits = nav::search(Lang::Fr, "licences");
        assert!(hits.iter().any(|h| h.page == Page::About && h.anchor == Some(s2::SEC_LICENSES)), "{hits:?}");
        assert_eq!(hits.iter().filter(|h| h.label.get(Lang::Fr) == s2::SEC_LICENSES.get(Lang::Fr)).count(), 1, "{hits:?}");
        assert!(nav::search(Lang::Fr, "  ").is_empty());
    }

    /// Section suivie par la colonne d'après la position de la page.
    #[test]
    fn spy_follows_the_scroll_position() {
        let view = egui::Rect::from_min_size(egui::pos2(0.0, 100.0), egui::vec2(500.0, 400.0));
        let (a, b, c) = (egui::Id::unique("a"), egui::Id::unique("b"), egui::Id::unique("c"));
        // En haut d'une page qui défile : aucune section.
        assert_eq!(spy_of(&[(a, 150.0), (b, 600.0), (c, 900.0)], view, 0.0, 1000.0), None);
        // La dernière section passée sous le quart supérieur de la vue.
        assert_eq!(spy_of(&[(a, -50.0), (b, 190.0), (c, 480.0)], view, 200.0, 1000.0), Some(b));
        assert_eq!(spy_of(&[(a, -50.0), (b, 320.0), (c, 480.0)], view, 200.0, 1000.0), Some(a));
        // Au bout de la page : la dernière section visible.
        assert_eq!(spy_of(&[(a, -450.0), (b, 150.0), (c, 380.0)], view, 600.0, 1000.0), Some(c));
        // Page courte (rien ne défile) : seule une section du haut compte.
        assert_eq!(spy_of(&[(a, 150.0), (b, 420.0)], view, 0.0, 300.0), Some(a));
        assert_eq!(spy_of(&[], view, 0.0, 300.0), None);
    }

    struct Offline;
    impl super::super::Backend for Offline {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            false
        }
        fn poll(&mut self) -> Vec<crate::state::Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    /// L'entrée de la page affichée est dépliée, d'où qu'on l'ait ouverte ;
    /// ses sous-entrées ouvrent leur page ou leur section ; les autres
    /// entrées restent repliées.
    #[test]
    fn open_entry_unfolds_into_sub_entries() {
        use super::super::tests::{click, find, frame};
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = super::super::App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), crate::state::Store::new(), images));
        app.lang = Lang::Fr;
        let fr = Lang::Fr;
        app.show_settings(Page::Profile);
        frame(&mut app, Vec::new());
        let out = frame(&mut app, Vec::new());
        for label in [st::NAV_ACCOUNT, st::SUB_ACCOUNT_INFO, st::SUB_PASSWORD, st::PAGE_DEVICES, st::PAGE_CONNECTIONS, st::LOG_OUT] {
            assert!(find(&out, label.get(fr)).is_some(), "« {} » attendu dans la colonne", label.get(fr));
        }
        assert!(find(&out, crate::i18n::table::audio::t::SEC_TEST.get(fr)).is_none(), "« Voix et vidéo » est repliée");
        click(&mut app, st::PAGE_DEVICES.get(fr));
        assert_eq!(app.params.page(), Page::Devices);
        click(&mut app, st::SUB_PASSWORD.get(fr));
        assert_eq!(app.params.page(), Page::MyAccount);
        // Une entrée à sections, ouverte d'ailleurs : amenée en vue et dépliée ;
        // sa sous-entrée garde la page.
        app.show_settings(Page::Voice);
        for _ in 0..40 {
            frame(&mut app, Vec::new());
        }
        click(&mut app, crate::i18n::table::audio::t::SEC_PROCESSING.get(fr));
        assert_eq!(app.params.page(), Page::Voice);
        assert_eq!(app.params.spy, Some(shell::anchor_of(crate::i18n::table::audio::t::SEC_PROCESSING.get(fr))), "la sous-entrée cliquée est marquée");
        let out = frame(&mut app, Vec::new());
        assert!(find(&out, st::SUB_PASSWORD.get(fr)).is_none(), "« Compte » s'est repliée");
    }

    /// Fenêtre trop étroite pour deux colonnes (400 points de large : 520
    /// pixels à un zoom de 1,3) : la page seule, bornée à l'écran, puis la
    /// liste des pages à sa place.
    #[test]
    fn narrow_window_shows_one_column_within_the_screen() {
        use super::super::tests::{find, offline_boot};
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = super::super::App::with_context(ctx.clone(), offline_boot(Box::new(Offline), crate::state::Store::new(), images));
        app.lang = Lang::Fr;
        // Disposition bureau imposée : en automatique, 400 points de large serait mobile (phase 7).
        let mut prefs = app.local_prefs();
        prefs.mobile.layout = crate::state::mobile::Layout::Desktop;
        app.set_local_prefs(prefs);
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 600.0));
        let run = |app: &mut super::super::App, events: Vec<egui::Event>| {
            app.logic_pass();
            let raw = egui::RawInput { screen_rect: Some(screen), events, ..Default::default() };
            let mut out = ctx.run_ui(raw, |ui| app.ui_pass(ui));
            out.textures_delta.clear();
            out
        };
        let click = |app: &mut super::super::App, label: &str| {
            let mut out = run(app, Vec::new());
            for _ in 0..3 {
                out = run(app, Vec::new());
            }
            let at = find(&out, label).unwrap_or_else(|| panic!("« {label} » introuvable")).center();
            let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
            run(app, vec![egui::Event::PointerMoved(at), button(true)]);
            run(app, vec![button(false)]);
            run(app, Vec::new())
        };
        app.show_settings(Page::Appearance);
        let fr = Lang::Fr;
        run(&mut app, Vec::new());
        run(&mut app, Vec::new());
        let out = run(&mut app, Vec::new());
        let back = find(&out, st::ALL_SETTINGS.get(fr)).unwrap_or_else(|| panic!("bouton « Tous les réglages » attendu"));
        assert!(screen.contains_rect(back), "{back:?} hors de {screen:?}");
        assert!(find(&out, st::PAGE_ACCOUNT.get(fr)).is_none(), "une seule colonne");
        click(&mut app, st::ALL_SETTINGS.get(fr));
        let out = click(&mut app, st::NAV_ACCOUNT.get(fr));
        assert_eq!(app.params.page(), Page::MyAccount);
        assert!(find(&out, st::ALL_SETTINGS.get(fr)).is_some(), "la page choisie remplace la liste");
    }
}
