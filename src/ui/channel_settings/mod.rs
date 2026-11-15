//! Fenêtre « Modifier le salon » (lot A2), distincte de « Paramètres du
//! serveur » mais du même style que la fenêtre Paramètres : pages à gauche
//! (vue d'ensemble, permissions, invitations, intégrations, supprimer), page
//! à droite, une seule colonne en fenêtre étroite, Échap pour fermer.
//!
//! Fenêtres de création, de clonage et de suppression (`dialogs.rs`), et ce
//! que la liste des salons gagne (menus, « + » des catégories, glisser-
//! déposer : `list.rs`). Rien ne part sans un clic ; les données affichées
//! viennent du magasin (`state::admin::channels`).

mod dialogs;
mod integrations;
mod invites;
pub(super) mod list;
mod overview;
mod permissions;

use super::settings::nav::entry_row;
use super::{settings::shell, App, ToastText};
use crate::i18n::table::chadmin::{f, t as tx};
use crate::i18n::{Lang, Text};
use crate::state::admin::channels::{ChanAction, ChanFailure, ChanNotice};
use crate::state::model::ChannelKind;
use crate::theme::{icons::Icon, Tokens, SETTINGS_WINDOW};
use eframe::egui::{self, Align, Id, Key, Layout, Modifiers, RichText};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Overview,
    Permissions,
    Invites,
    Integrations,
}

impl Page {
    fn title(self) -> Text {
        match self {
            Page::Overview => tx::PAGE_OVERVIEW,
            Page::Permissions => tx::PAGE_PERMISSIONS,
            Page::Invites => tx::PAGE_INVITES,
            Page::Integrations => tx::PAGE_INTEGRATIONS,
        }
    }

    /// Icône de l'entrée dans la colonne.
    fn icon(self) -> Icon {
        match self {
            Page::Overview => Icon::Sliders,
            Page::Permissions => Icon::Lock,
            Page::Invites => Icon::Link,
            Page::Integrations => Icon::Puzzle,
        }
    }
}

pub(super) struct EditWindow {
    pub channel: u64,
    pub page: Page,
    overview: Option<overview::Draft>,
    perms: Option<permissions::Draft>,
    /// Fenêtre étroite : la liste des pages est affichée à la place de la page.
    nav_shown: bool,
}

#[derive(Default)]
pub(super) struct ChannelUi {
    pub edit: Option<EditWindow>,
    create: Option<dialogs::Create>,
    clone: Option<dialogs::Clone>,
    delete: Option<dialogs::Delete>,
    pub dnd: list::ChanDnd,
}

impl ChannelUi {
    /// Une fenêtre de ce module est ouverte (elle prend Échap avant les autres).
    pub(super) fn is_open(&self) -> bool {
        self.edit.is_some() || self.create.is_some() || self.clone.is_some() || self.delete.is_some()
    }
}

fn action_word(lang: Lang, a: ChanAction) -> &'static str {
    match a {
        ChanAction::Load => tx::ACTION_LOAD.get(lang),
        ChanAction::Create => tx::ACTION_CREATE.get(lang),
        ChanAction::Save => tx::ACTION_SAVE.get(lang),
        ChanAction::Delete => tx::ACTION_DELETE.get(lang),
        ChanAction::Move => tx::ACTION_MOVE.get(lang),
        ChanAction::Regions => tx::ACTION_REGIONS.get(lang),
    }
}

fn failure_text(lang: Lang, f: &ChanFailure, h24: bool) -> String {
    match f {
        ChanFailure::Problem(p) => super::words::problem(lang, p, h24),
        ChanFailure::NotFound => tx::NOT_FOUND.get(lang).to_string(),
    }
}

/// Lien d'un salon, comme « Copier le lien » de Discord.
pub(super) fn channel_link(guild: u64, channel: u64) -> String {
    format!("https://discord.com/channels/{guild}/{channel}")
}

/// Bouton principal (couleur d'accent).
fn primary(t: &Tokens, text: &str) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text.to_string()).color(t.colors.on_accent)).fill(t.colors.accent)
}

/// Bandeau « modifications non enregistrées » : (réinitialiser, enregistrer).
fn unsaved_bar(ui: &mut egui::Ui, t: &Tokens, lang: Lang, busy: bool, save_label: &str) -> (bool, bool) {
    let (mut reset, mut save) = (false, false);
    ui.add_space(t.spacing.category_gap);
    egui::Frame::new().fill(t.colors.bg0).corner_radius(t.r_card()).inner_margin(t.card_margin()).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new(tx::UNSAVED.get(lang)).color(t.colors.text_strong));
            reset = ui.add_enabled(!busy, egui::Button::new(tx::RESET.get(lang)).frame(false)).clicked();
            save = ui.add_enabled(!busy, primary(t, save_label)).clicked();
            if busy {
                ui.spinner();
            }
        });
    });
    (reset, save)
}

impl App {
    /// Ouvre « Modifier le salon » sur `page` ; `false` sans le droit de le faire.
    pub(super) fn open_channel_settings(&mut self, channel: u64, page: Page) -> bool {
        if !self.store.can_edit_channel(channel) {
            return false;
        }
        let same = self.chan_ui.edit.as_ref().is_some_and(|e| e.channel == channel);
        if !same {
            self.chan_ui.edit = Some(EditWindow { channel, page, overview: None, perms: None, nav_shown: false });
            self.store.load_channel(channel);
            // Salon vocal : sa page propose les régions.
            if self.store.voice.channel(channel).is_some() {
                self.store.load_voice_regions();
            }
        } else if let Some(e) = self.chan_ui.edit.as_mut() {
            e.page = page;
        }
        true
    }

    /// « Modifier le salon » sur une page (aperçu, tests) : « vue », « permissions », « invitations ».
    pub fn show_channel_settings(&mut self, channel: u64, page: &str) -> bool {
        let p = match page {
            "vue" => Page::Overview,
            "permissions" => Page::Permissions,
            "invitations" => Page::Invites,
            _ => return false,
        };
        let ok = self.open_channel_settings(channel, p);
        self.flush_commands();
        ok
    }

    /// Fenêtres des salons et messages de fin des actions (chaque image).
    pub(super) fn channel_admin_ui(&mut self, ui: &mut egui::Ui) {
        while let Some(n) = self.store.chan_admin.take_notice() {
            let text = match n {
                ChanNotice::Saved => tx::SAVED.get(self.lang).to_string(),
                ChanNotice::Deleted { name } => f::deleted(self.lang, name),
                // L'A2F se dit seule, comme dans « Paramètres du serveur ».
                ChanNotice::Failed { failure: ChanFailure::Problem(crate::state::Problem::MfaRequired), .. } => crate::i18n::table::admin::t::MFA_REQUIRED.get(self.lang).to_string(),
                ChanNotice::Failed { action, failure } => f::failed(self.lang, action_word(self.lang, action), failure_text(self.lang, &failure, self.h24())),
            };
            self.notify(ToastText::Text(text));
        }
        match self.store.chan_admin.take_done() {
            Some(ChanAction::Create) => {
                self.chan_ui.create = None;
                self.chan_ui.clone = None;
            }
            Some(ChanAction::Delete) => {
                self.chan_ui.delete = None;
                self.chan_ui.edit = None;
            }
            // Brouillons refaits d'après ce que Discord a enregistré.
            Some(ChanAction::Save) => {
                if let Some(e) = self.chan_ui.edit.as_mut() {
                    e.overview = None;
                    e.perms = None;
                }
            }
            _ => {}
        }
        if self.store.open_created_channel() {
            self.scroll_to_bottom = true;
        }
        self.create_channel_dialog(ui);
        self.clone_channel_dialog(ui);
        self.delete_channel_dialog(ui);
        self.edit_window(ui);
    }

    fn edit_window(&mut self, ui: &mut egui::Ui) {
        let Some(channel) = self.chan_ui.edit.as_ref().map(|e| e.channel) else { return };
        // Salon supprimé, ou droit perdu : la fenêtre se ferme.
        let exists = self.store.channel_exists(channel) || self.store.voice.channel(channel).is_some();
        if !exists || !self.store.can_edit_channel(channel) {
            self.close_channel_settings();
            return;
        }
        let modal_open = self.chan_ui.delete.is_some() || self.chan_ui.create.is_some() || self.chan_ui.clone.is_some() || self.servers.invites.is_open();
        if !modal_open && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.close_channel_settings();
            return;
        }
        let t = self.theme.clone();
        let g = SETTINGS_WINDOW;
        let screen = ui.ctx().content_rect();
        // Plein écran par-dessus l'application (`shell`).
        let size = screen.size();
        let frame = egui::Frame::new().fill(t.colors.bg2);
        let mut close = false;
        egui::Window::new(tx::EDIT_CHANNEL.get(self.lang))
            .id(Id::unique("modifier-salon"))
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
                        if self.chan_ui.edit.as_ref().is_some_and(|e| e.nav_shown) {
                            self.edit_nav(ui, &t);
                        } else {
                            if ui.button(tx::ALL_PAGES.get(self.lang)).clicked()
                                && let Some(e) = self.chan_ui.edit.as_mut()
                            {
                                e.nav_shown = true;
                            }
                            self.edit_page(ui, &t);
                        }
                    });
                    return;
                }
                let full = ui.max_rect();
                let zones = shell::layout(full);
                shell::backdrop(ui, &t, full, &zones);
                ui.scope_builder(egui::UiBuilder::new().max_rect(zones.nav).layout(Layout::top_down(Align::Min)), |ui| self.edit_nav(ui, &t));
                ui.scope_builder(egui::UiBuilder::new().max_rect(zones.page).layout(Layout::top_down(Align::Min)), |ui| {
                    ui.set_max_size(zones.page.size());
                    shell::page_style(ui, &t);
                    self.edit_page(ui, &t);
                });
                close = shell::close_button(ui, &t, self.lang, zones.close);
            });
        if close {
            self.close_channel_settings();
        }
    }

    fn close_channel_settings(&mut self) {
        self.chan_ui.edit = None;
        self.store.chan_admin.forget();
    }

    /// Pages proposées pour ce salon, selon ses permissions et son type.
    fn edit_pages(&self, channel: u64) -> Vec<Page> {
        // Salon vocal : ses dérogations vivent avec le vocal, seule la vue d'ensemble est proposée.
        if self.store.voice.channel(channel).is_some() {
            return vec![Page::Overview];
        }
        let category = self.store.guilds.iter().flat_map(|g| g.channels.iter()).any(|c| c.id == channel && c.kind == ChannelKind::Category);
        let mut pages = vec![Page::Overview];
        if self.store.can_edit_overwrites(channel) {
            pages.push(Page::Permissions);
        }
        if !category {
            pages.push(Page::Invites);
        }
        if !category && integrations::available(self, channel) {
            pages.push(Page::Integrations);
        }
        pages
    }

    fn edit_nav(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let Some((channel, current)) = self.chan_ui.edit.as_ref().map(|e| (e.channel, e.page)) else { return };
        let (name, category) = self
            .store
            .guilds
            .iter()
            .flat_map(|g| g.channels.iter())
            .find(|c| c.id == channel)
            .map(|c| (c.name.clone(), c.kind == ChannelKind::Category))
            .unwrap_or_default();
        let voice = self.store.voice.channel(channel).map(|c| c.name.clone());
        let head = match &voice {
            Some(v) => v.clone(),
            None if category => name,
            None => format!("#{name}"),
        };
        // Mêmes lignes que la colonne des paramètres utilisateur (`settings::nav`).
        ui.spacing_mut().item_spacing.y = (t.spacing.item_y / 2.0).round();
        ui.add_space(t.spacing.item_x);
        shell::nav_title(ui, t, &head);
        let mut go = None;
        for p in self.edit_pages(channel) {
            if entry_row(ui, t, p.icon(), p.title().get(lang), None, None, current == p).clicked() {
                go = Some(p);
            }
        }
        // La suppression demande de retaper le nom : celui d'un salon vocal n'est pas dans cette liste.
        let mut ask_delete = false;
        if voice.is_none() {
            shell::nav_divider(ui, t);
            let del = if category { tx::DELETE_CATEGORY } else { tx::DELETE_CHANNEL };
            ask_delete = entry_row(ui, t, Icon::Trash, del.get(lang), None, Some(t.colors.danger), false).clicked();
        }
        if let Some(p) = go
            && let Some(e) = self.chan_ui.edit.as_mut()
        {
            e.page = p;
            e.nav_shown = false;
        }
        if ask_delete {
            self.ask_delete_channel(channel);
        }
    }

    fn edit_page(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let Some((channel, mut page)) = self.chan_ui.edit.as_ref().map(|e| (e.channel, e.page)) else { return };
        // Page devenue indisponible (permission perdue) : vue d'ensemble.
        if !self.edit_pages(channel).contains(&page) {
            page = Page::Overview;
        }
        if shell::header(ui, t, lang, page.title().get(lang)) {
            self.chan_ui.edit = None;
        }
        if self.chan_ui.edit.is_none() {
            self.store.chan_admin.forget();
            return;
        }
        // Bandeau d'enregistrement fixé en bas : visible quelle que soit la longueur de la page.
        let dirty = self.chan_ui.edit.as_ref().is_some_and(|e| match page {
            Page::Overview => e.overview.as_ref().is_some_and(|d| !d.patch().is_empty()),
            Page::Permissions => e.perms.as_ref().is_some_and(permissions::Draft::changed),
            _ => false,
        });
        if dirty {
            egui::Panel::bottom(if page == Page::Overview { "barre-salon-vue" } else { "barre-salon-permissions" }).frame(egui::Frame::new()).resizable(false).show(ui, |ui| match page {
                Page::Overview => self.overview_bar(ui, t, channel),
                Page::Permissions => self.permissions_bar(ui, t, channel),
                _ => {}
            });
        }
        egui::ScrollArea::vertical().id_salt(("modifier-salon-page", page as u8)).auto_shrink([false, false]).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(shell::PAGE_WIDTH));
            match page {
                Page::Overview => self.channel_overview(ui, t, channel),
                Page::Permissions => self.channel_permissions(ui, t, channel),
                Page::Invites => self.channel_invites(ui, t, channel),
                // Renvoi vers les webhooks de « Paramètres du serveur ».
                Page::Integrations => integrations::open(self, channel),
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, frame, offline_boot};
    use super::*;
    use crate::state::admin::channels::{ChanCmd, ChanEvent, Details};
    use crate::state::model::{Channel, Guild, Member, Overwrite, OverwriteKind, Role, User};
    use crate::state::{Cmd, Event, Permissions, Store};
    use std::sync::{Arc, Mutex};

    pub(super) const G: u64 = 100;
    pub(super) const ME: u64 = 1;
    pub(super) const CAT: u64 = 10;
    pub(super) const TEXT: u64 = 11;
    pub(super) const R_MOD: u64 = 200;

    #[derive(Clone, Default)]
    pub(super) struct Online(pub Arc<Mutex<Vec<Cmd>>>);

    impl crate::ui::Backend for Online {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            if let Ok(mut v) = self.0.lock() {
                v.extend(cmds);
            }
        }
    }

    /// Serveur dont je suis propriétaire : une catégorie, #général dedans.
    pub(super) fn app(net: Online) -> App {
        let ctx = egui::Context::default();
        ctx.enable_accesskit();
        let mut store = Store::new();
        let ch = |id, kind, parent, overwrites| Channel { id, name: if id == CAT { "Salons".into() } else { "général".into() }, kind, position: 0, parent, overwrites: Some(overwrites), slowmode: 0 };
        let deny = vec![Overwrite { id: G, kind: OverwriteKind::Role, allow: Permissions::NONE, deny: Permissions::SEND_MESSAGES }];
        let mut g = Guild::new(G, "Club".into(), vec![ch(CAT, ChannelKind::Category, None, deny.clone()), ch(TEXT, ChannelKind::Text, Some(CAT), Vec::new())]);
        g.owner_id = Some(ME);
        g.roles = vec![
            Role { id: G, name: "@everyone".into(), color: 0, position: 0, permissions: Permissions::VIEW_CHANNEL, mentionable: false },
            Role { id: R_MOD, name: "Modération".into(), color: 0, position: 1, permissions: Permissions::NONE, mentionable: false },
        ];
        g.me = Some(Member::default());
        store.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![g], dms: Vec::new() });
        store.select_guild(G);
        store.take_commands();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), store, images));
        app.lang = Lang::Fr;
        app
    }

    /// Quelques images (fenêtres et fenêtres modales se placent d'après la précédente).
    pub(super) fn shown(a: &mut App) -> egui::FullOutput {
        frame(a, Vec::new());
        frame(a, Vec::new());
        frame(a, Vec::new())
    }

    pub(super) fn labels(out: &egui::FullOutput) -> Vec<String> {
        out.platform_output.accesskit_update.iter().flat_map(|u| u.nodes.iter().filter_map(|(_, n)| n.label().map(str::to_string))).collect()
    }

    /// Clic sur l'élément nommé exactement `label` (`click` accepte une
    /// partie du nom : « Supprimer » trouverait aussi le titre).
    pub(super) fn click_exact(a: &mut App, label: &str) {
        let out = shown(a);
        let at = out
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|u| u.nodes.iter().filter(|(_, n)| n.label().is_some_and(|l| l == label)).find_map(|(_, n)| n.bounds()))
            .map(|b| egui::pos2(((b.x0 + b.x1) / 2.0) as f32, ((b.y0 + b.y1) / 2.0) as f32))
            .unwrap_or_else(|| panic!("« {label} » introuvable : {:?}", labels(&out)));
        let button = |pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        frame(a, vec![egui::Event::PointerMoved(at), button(true)]);
        frame(a, vec![button(false)]);
        frame(a, Vec::new());
    }

    pub(super) fn sent(net: &Online) -> Vec<ChanCmd> {
        net.0.lock().map(|mut v| v.drain(..).filter_map(|c| if let Cmd::ChannelAdmin(c) = c { Some(c) } else { None }).collect()).unwrap_or_default()
    }

    #[test]
    fn edit_overview_then_save_sends_only_changes() {
        let net = Online::default();
        let mut a = app(net.clone());
        assert!(a.show_channel_settings(TEXT, "vue"));
        assert_eq!(sent(&net), vec![ChanCmd::Load { channel_id: TEXT }]);
        a.store.apply(Event::ChannelAdmin(ChanEvent::Details(Details { id: TEXT, guild_id: Some(G), kind: 0, name: "général".into(), topic: "Bienvenue".into(), ..Details::default() })));
        let out = shown(&mut a);
        assert!(find(&out, tx::ANNOUNCEMENT.get(Lang::Fr)).is_some(), "page affichée : {:?}", labels(&out));
        assert!(find(&out, tx::UNSAVED.get(Lang::Fr)).is_none(), "rien de modifié");
        // Mode lent changé : le bandeau apparaît, l'enregistrement n'envoie que lui.
        if let Some(d) = a.chan_ui.edit.as_mut().and_then(|e| e.overview.as_mut()) {
            d.set_slowmode(30);
        }
        click_exact(&mut a, tx::SAVE.get(Lang::Fr));
        let cmds = sent(&net);
        let [ChanCmd::Modify { channel_id: TEXT, patch }] = cmds.as_slice() else { panic!("{cmds:?}") };
        assert_eq!((patch.slowmode, patch.name.clone(), patch.topic.clone()), (Some(30), None, None));
    }

    #[test]
    fn delete_needs_the_typed_name() {
        let net = Online::default();
        let mut a = app(net.clone());
        a.ask_delete_channel(TEXT);
        let out = shown(&mut a);
        assert!(labels(&out).iter().any(|l| l == tx::DELETE.get(Lang::Fr)), "{:?}", labels(&out));
        click_exact(&mut a, tx::DELETE.get(Lang::Fr));
        assert!(sent(&net).is_empty(), "nom pas encore saisi");
        a.type_delete_name("général");
        click_exact(&mut a, tx::DELETE.get(Lang::Fr));
        assert_eq!(sent(&net), vec![ChanCmd::Delete { channel_id: TEXT }]);
    }

    /// Liste des salons (phase 10, lot S) : un clic sur la catégorie la replie
    /// (écriture des réglages du compte), le salon ouvert reste visible ; le
    /// bouton « Fils » de l'en-tête ouvre son panneau.
    #[test]
    fn category_click_folds_and_threads_button_opens_its_panel() {
        use crate::state::usersettings::notif::Change;
        use crate::state::usersettings::AccountCmd;
        let net = Online::default();
        let mut a = app(net.clone());
        let out = shown(&mut a);
        assert!(labels(&out).iter().any(|l| l == "général"), "{:?}", labels(&out));
        click_exact(&mut a, "Salons : Replier la catégorie");
        let cmds: Vec<Cmd> = net.0.lock().map(|mut v| v.drain(..).collect()).unwrap_or_default();
        assert!(
            cmds.iter().any(|c| matches!(c, Cmd::Account(AccountCmd::PatchGuild { guild: Some(G), patch, .. }) if patch.change == Change::Collapsed(vec![(CAT, true)]))),
            "{cmds:?}"
        );
        let out = shown(&mut a);
        assert!(labels(&out).iter().any(|l| l == "Salons : Déplier la catégorie"), "{:?}", labels(&out));
        assert!(labels(&out).iter().any(|l| l == "général"), "le salon ouvert reste visible : {:?}", labels(&out));
        a.show_head_popup("fils");
        let out = shown(&mut a);
        let fr = |t: crate::i18n::Text| t.get(Lang::Fr);
        assert!(find(&out, fr(crate::i18n::table::threads::t::TAB_ARCHIVED)).is_some() && find(&out, fr(crate::i18n::table::threads::t::TAB_ACTIVE)).is_some(), "{:?}", labels(&out));
    }

    #[test]
    fn pages_follow_permissions_and_kind() {
        let a = app(Online::default());
        // Intégrations : renvoi vers les webhooks de « Paramètres du serveur ».
        assert_eq!(a.edit_pages(TEXT), vec![Page::Overview, Page::Permissions, Page::Invites, Page::Integrations]);
        assert_eq!(a.edit_pages(CAT), vec![Page::Overview, Page::Permissions]);
        assert_eq!(channel_link(G, TEXT), "https://discord.com/channels/100/11");
    }
}
