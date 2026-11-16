//! Carte de mon compte : un clic sur le panneau utilisateur (avatar et nom,
//! en bas de la colonne des salons) l'ouvre au-dessus de lui, comme dans le
//! client Discord. En haut, mon profil tel que les autres le voient ; dessous,
//! deux blocs de lignes de menu : modifier le profil (`ui/profile_edit.rs`),
//! mon statut (sous-menu), mon statut personnalisé (petite fenêtre dédiée),
//! puis changer de compte (sous-menu) et copier mon identifiant. Aucun champ
//! de saisie dans la carte elle-même. Le clic droit sur le panneau donne les
//! mêmes actions dans un menu.
//!
//! Rien de nouveau n'est envoyé d'ici : le statut et le statut personnalisé
//! passent par les réglages synchronisés, comme la page Statut des
//! paramètres, dont le brouillon est partagé.

use super::menu::{self, Lead, Line, Mark};
use super::profile::{room, Header};
use super::settings::synced::{EXPIRY, STATUSES};
use super::{widgets, App};
use crate::account::AccountKey;
use crate::i18n::table::quick::t as qt;
use crate::i18n::table::s::{f as sf, t as st};
use crate::i18n::{time, Text};
use crate::state::model::User;
use crate::state::people::{Activity, ActivityEmoji, ActivityKind, Presence, Status};
use crate::state::profiles::Profile;
use crate::state::usersettings::schema::{CustomStatus, Key, SValue};
use crate::theme::icons::{self, Icon};
use crate::theme::{Elevation, Tokens};
use eframe::egui::{self, Align2, Color32, Id, RichText, Sense};
use std::cell::{Cell, RefCell};

/// Sous-menus de la carte.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SubKind {
    Status,
    Accounts,
}

/// Sous-menu ouvert à côté d'une ligne de la carte.
struct Sub {
    kind: SubKind,
    /// La ligne d'où il part, sur toute la largeur de la carte.
    anchor: egui::Rect,
    /// Instant où le pointeur est passé sur une autre ligne : il se ferme
    /// s'il y reste (le pointeur qui rejoint le sous-menu en biais en croise une).
    leaving: Option<f64>,
}

/// Délai avant qu'une autre ligne survolée ferme le sous-menu (secondes).
const SUB_LEAVE: f64 = 0.25;
/// Part de la couleur du texte dans le fond d'un bloc de lignes : il se
/// détache du fond de la carte, clair ou foncé, thème de profil compris.
const BLOCK_TINT: f32 = 0.06;
/// Vignettes de jeux montrées dans « Collection de jeux », au plus.
const GAME_TILES: usize = 3;

/// État de la carte de mon compte et de l'édition rapide.
#[derive(Default)]
pub(super) struct QuickUi {
    /// Carte ouverte au-dessus du panneau utilisateur.
    card: bool,
    /// Ouverte à cette image : ce clic ne la referme pas, et le profil complet est demandé.
    fresh: bool,
    /// Sous-menu ouvert (statut, comptes).
    sub: Option<Sub>,
    /// Sous-menu à ouvrir dès que sa ligne est placée (aperçu).
    sub_wanted: Option<SubKind>,
    /// Sous-menu fermé par Échap : sa ligne, encore sous le pointeur, ne le
    /// rouvre pas tant que le pointeur ne l'a pas quittée.
    sub_dismissed: Option<SubKind>,
    /// Fenêtre du statut personnalisé ouverte.
    status_modal: bool,
    /// Ouverte à cette image : son champ prend le clavier.
    modal_fresh: bool,
    /// Édition rapide du profil (fenêtre modale).
    pub(super) editor: Option<super::profile_edit::Editor>,
    /// Sélecteur de l'emoji du statut, ouvert au-dessus de la fenêtre du statut (lot R).
    pub(super) status_picker: Option<super::status_emoji::StatusPicker>,
}

/// Geste fait dans la carte ou dans le menu du panneau.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pick {
    Edit,
    Status(&'static str),
    SaveCustom(Option<CustomStatus>),
    /// Ouvre la fenêtre du statut personnalisé.
    CustomStatus,
    /// « Gérer les comptes » : le sélecteur de compte complet.
    Switch,
    /// Un compte choisi dans le sous-menu.
    Account(AccountKey),
    CopyId,
    /// « Paramètres d'activité » du menu de la carte d'activité.
    ActivitySettings,
    /// « Voir le profil complet » du même menu.
    FullProfile,
}

fn sub_id(kind: SubKind) -> Id {
    Id::unique(("ferrocord.mon-compte.sous-menu", kind))
}

fn status_modal_id() -> Id {
    Id::unique("ferrocord.mon-compte.statut")
}

/// Bouton « retirer » posé sur la ligne du statut personnalisé.
fn clear_button_id() -> Id {
    Id::unique("ferrocord.mon-compte.retirer-statut")
}

/// Fond d'un bloc de lignes, à poser sur le fond de la carte.
fn block_fill(t: &Tokens) -> Color32 {
    t.colors.text_strong.gamma_multiply(BLOCK_TINT)
}

/// Bloc de lignes de menu : carte arrondie au fond légèrement contrasté.
fn block(ui: &mut egui::Ui, t: &Tokens, add: impl FnOnce(&mut egui::Ui)) {
    let pad = t.spacing.item_y.round().clamp(0.0, 127.0) as i8;
    egui::Frame::new().fill(block_fill(t)).corner_radius(t.r_embed()).inner_margin(egui::Margin::same(pad)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing.y = 0.0;
        add(ui);
    });
}

/// Description d'un statut dans le sous-menu.
fn status_hint(code: &str) -> Option<Text> {
    match code {
        "idle" => Some(qt::STATUS_IDLE_HINT),
        "dnd" => Some(qt::STATUS_DND_HINT),
        "invisible" => Some(qt::STATUS_INVISIBLE_HINT),
        _ => None,
    }
}

/// Couleur de la pastille d'un statut choisi (`invisible` : celle de « hors ligne »).
fn status_color(t: &Tokens, code: &str) -> Color32 {
    t.status_color(Status::parse(code).unwrap_or(Status::Offline))
}

/// Ligne cliquable de la carte : pastille facultative, libellé ; `selected` : choix en cours.
fn row(ui: &mut egui::Ui, t: &Tokens, dot: Option<Color32>, label: &str, selected: bool, enabled: bool) -> egui::Response {
    let c = &t.colors;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.spacing.channel_row), sense);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, selected, label));
    if !ui.is_rect_visible(rect) {
        return resp;
    }
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, t.r_row(), c.selected);
    } else if resp.hovered() {
        p.rect_filled(rect, t.r_row(), c.row_hover);
    }
    let fade = if enabled { 1.0 } else { 0.45 };
    let mut x = rect.left() + t.spacing.item_x;
    if let Some(color) = dot {
        let r = t.shapes.mention_dot;
        p.circle_filled(egui::pos2(x + r, rect.center().y), r, color.gamma_multiply(fade));
        x += 2.0 * r + t.spacing.item_x;
    }
    let color = if selected || resp.hovered() { c.text_strong } else { c.text };
    let family = if selected { super::fonts::demi() } else { egui::FontFamily::Proportional };
    let font = egui::FontId::new(t.typography.button, family);
    let mut job = egui::text::LayoutJob::simple_singleline(label.to_string(), font, color.gamma_multiply(fade));
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - t.spacing.item_x / 2.0 - x).max(1.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    p.galley(egui::pos2(x, rect.center().y - galley.size().y / 2.0), galley, color);
    if enabled { resp.on_hover_cursor(egui::CursorIcon::PointingHand) } else { resp }
}

impl App {
    /// Ouvre la carte de mon compte (aperçu, tests).
    pub fn open_account_card(&mut self) {
        self.quick.card = true;
        self.quick.fresh = true;
    }

    pub fn account_card_open(&self) -> bool {
        self.quick.card
    }

    /// Clic sur le panneau utilisateur.
    pub(super) fn toggle_account_card(&mut self) {
        self.quick.card = !self.quick.card;
        self.quick.fresh = self.quick.card;
        self.quick.sub = None;
    }

    /// Statut choisi dans les réglages synchronisés (`None` : inconnus).
    fn own_status_code(&self) -> Option<&str> {
        match self.store.account.value(Key::Status) {
            Some(SValue::Str(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// Statut que j'ai choisi, pour la pastille du panneau utilisateur.
    pub(super) fn own_status(&self) -> Option<Status> {
        let s = self.own_status_code().and_then(Status::parse)?;
        // Inactif automatique (F2) : « en ligne » se montre « inactif », comme les autres me voient.
        Some(if s == Status::Online && self.store.activity.idle_since.is_some() { Status::Idle } else { s })
    }

    /// Statut personnalisé enregistré, s'il y en a un.
    pub(super) fn saved_custom_status(&self) -> Option<CustomStatus> {
        self.store.account.snapshot().custom_status.clone().filter(|c| !c.is_empty())
    }

    /// Ma présence telle que les autres la voient, d'après mes réglages :
    /// Discord ne me la renvoie pas.
    pub(super) fn own_presence_shown(&self, presence: &mut Option<Presence>) {
        let status = self.own_status();
        let now = self.store.now();
        let custom = self.saved_custom_status().filter(|c| !c.expired(now));
        if presence.is_none() && status.is_none() && custom.is_none() {
            return;
        }
        let p = presence.get_or_insert_with(|| Presence { status: Status::Online, client: Default::default(), activities: Vec::new() });
        if let Some(s) = status {
            p.status = s;
        }
        p.activities.retain(|a| a.kind != ActivityKind::Custom);
        if let Some(c) = custom {
            let animated = c.emoji_id != 0 && self.emoji_animated(c.emoji_id);
            let emoji = (!c.emoji_name.is_empty()).then(|| ActivityEmoji { name: Some(c.emoji_name.clone()), id: (c.emoji_id != 0).then_some(c.emoji_id), animated });
            let state = (!c.text.is_empty()).then(|| c.text.clone());
            p.activities.insert(0, Activity { kind: ActivityKind::Custom, name: String::new(), details: None, state, emoji, start_ms: None, rich: None });
        }
    }

    /// Menu du clic droit sur le panneau utilisateur ; vrai si « Changer de
    /// compte » y a été choisi.
    pub(super) fn user_panel_menu(&mut self, panel: &egui::Response) -> bool {
        let lang = self.lang;
        let t = self.theme.clone();
        let current = self.own_status_code().map(str::to_string);
        let ready = self.store.account.proto_ready() && !self.store.account.proto_busy();
        let mut pick = None;
        panel.context_menu(|ui| {
            ui.set_min_width(t.layout.profile_width * 0.7);
            if crate::ui::menu::item(ui, &t, qt::EDIT_PROFILE.get(lang), Some(crate::theme::icons::Icon::Pencil), false).clicked() {
                pick = Some(Pick::Edit);
            }
            crate::ui::menu::separator(ui, &t);
            for (code, label) in STATUSES {
                if row(ui, &t, Some(status_color(&t, code)), label.get(lang), current.as_deref() == Some(code), ready).clicked() {
                    pick = Some(Pick::Status(code));
                }
            }
            if crate::ui::menu::item(ui, &t, qt::SET_CUSTOM_STATUS.get(lang), Some(crate::theme::icons::Icon::Smile), false).clicked() {
                pick = Some(Pick::CustomStatus);
            }
            crate::ui::menu::separator(ui, &t);
            if crate::ui::menu::item(ui, &t, qt::SWITCH_ACCOUNT.get(lang), Some(crate::theme::icons::Icon::Members), false).clicked() {
                pick = Some(Pick::Switch);
            }
            if crate::ui::menu::item(ui, &t, crate::i18n::t::PROFILE_COPY_ID.get(lang), Some(crate::theme::icons::Icon::Id), false).clicked() {
                pick = Some(Pick::CopyId);
            }
            if pick.is_some() {
                ui.close();
            }
        });
        pick.is_some_and(|p| self.quick_pick(&panel.ctx, p))
    }

    /// Ouvre la fenêtre du statut personnalisé (aperçu, tests).
    pub fn open_custom_status(&mut self) {
        self.quick.card = false;
        self.quick.sub = None;
        self.quick.status_modal = true;
        self.quick.modal_fresh = true;
        // Le brouillon repart du statut enregistré.
        self.params.status.sent();
    }

    pub fn custom_status_open(&self) -> bool {
        self.quick.status_modal
    }

    /// Ferme la fenêtre du statut personnalisé sans rien enregistrer.
    pub(super) fn close_custom_status(&mut self) {
        self.quick.status_modal = false;
        self.quick.status_picker = None;
        self.params.status.sent();
    }

    /// Fenêtre du statut personnalisé au-dessus de l'éditeur de profil (clic sur la bulle).
    pub(super) fn custom_status_over_editor(&mut self, ctx: &egui::Context) {
        if let Some(p) = self.custom_status_modal(ctx) {
            self.quick_pick(ctx, p);
        }
    }

    /// Ouvre la carte sur son sous-menu des statuts (aperçu).
    pub fn open_status_submenu(&mut self) {
        self.open_account_card();
        self.quick.sub_wanted = Some(SubKind::Status);
    }

    /// Applique un geste ; vrai s'il demande le sélecteur de compte.
    fn quick_pick(&mut self, ctx: &egui::Context, pick: Pick) -> bool {
        self.quick.sub = None;
        match pick {
            Pick::Edit => {
                self.quick.card = false;
                self.open_profile_editor(self.store.guild.is_some());
            }
            Pick::Status(code) => {
                if let Err(r) = self.store.account.write(Key::Status, SValue::Str(code.to_string())) {
                    self.refused(r);
                }
            }
            Pick::SaveCustom(status) => {
                self.quick.status_modal = false;
                self.quick.status_picker = None;
                self.params.status.sent();
                let now = self.store.now();
                if let Err(r) = self.store.account.set_custom_status(status, now) {
                    self.refused(r);
                }
            }
            Pick::CustomStatus => self.open_custom_status(),
            Pick::Switch => {
                self.quick.card = false;
                return true;
            }
            Pick::Account(key) => {
                self.quick.card = false;
                self.switch_account(key);
            }
            Pick::ActivitySettings => {
                self.quick.card = false;
                self.show_settings(super::settings::Page::Activity);
            }
            Pick::FullProfile => {
                self.quick.card = false;
                if let Some(me) = self.store.me.clone() {
                    super::profile_full::open_full(ctx, me.id, self.store.guild, &me.name, me.avatar, super::profile_full::Tab::Activity);
                }
            }
            Pick::CopyId => {
                if let Some(me) = &self.store.me {
                    ctx.copy_text(me.id.to_string());
                    self.notify(qt::ID_COPIED);
                }
            }
        }
        false
    }

    /// Carte de mon compte, ancrée au-dessus du panneau utilisateur `panel`,
    /// son sous-menu, et la fenêtre du statut personnalisé ; Échap ou un clic
    /// en dehors ferme le sous-menu, puis la carte. Vrai si « Gérer les
    /// comptes » y a été choisi.
    pub(super) fn account_card_ui(&mut self, ui: &mut egui::Ui, panel: egui::Rect) -> bool {
        let ctx = ui.ctx().clone();
        let mut switch = false;
        // Éditeur de profil ouvert : il dessine cette fenêtre lui-même, par-dessus la sienne.
        if self.quick.editor.is_none()
            && let Some(p) = self.custom_status_modal(&ctx)
        {
            switch |= self.quick_pick(&ctx, p);
        }
        if !self.quick.card {
            self.quick.sub = None;
            self.quick.sub_wanted = None;
            self.quick.sub_dismissed = None;
            return switch;
        }
        let Some(me) = self.store.me.clone() else {
            self.quick.card = false;
            return switch;
        };
        // Liste déroulante ou fenêtre ouverte au début de l'image : un clic
        // dedans n'est pas un clic en dehors de la carte.
        let popup = ctx.any_popup_open() || self.quick.status_modal;
        // Échap ferme le sous-menu d'abord, la carte ensuite.
        if !popup && super::a11y::escape(&ctx) {
            match self.quick.sub.take() {
                Some(sub) => self.quick.sub_dismissed = Some(sub.kind),
                None => {
                    self.quick.card = false;
                    return switch;
                }
            }
        }
        let guild = self.store.guild;
        if self.quick.fresh {
            self.store.want_profile(me.id, guild);
        }
        let shown = self.shown(me.id, guild);
        self.want_profile_effect(&shown);
        let base = self.theme.clone();
        let themed = super::profile::profile_theme(&base, &shown);
        let t = themed.as_ref().map_or(&base, |th| &th.tokens);
        let gap = t.spacing.item_y;
        let mut pick = None;
        let area = egui::Area::new(Id::unique("ferrocord.mon-compte")).order(egui::Order::Foreground).pivot(Align2::LEFT_BOTTOM).fixed_pos(panel.left_top() - egui::vec2(0.0, gap)).constrain(true).show(&ctx, |ui| {
            // Place réelle au-dessus du panneau utilisateur, sous la barre de titre.
            let avail = panel.top() - gap - ctx.content_rect().top() - t.spacing.item_x;
            pick = self.account_card(ui, t, themed.as_ref(), &me, guild, &shown, avail);
        });
        let sub_rect = self.account_submenu(&ctx, &mut pick);
        let inside = |p: egui::Pos2| area.response.rect.contains(p) || sub_rect.is_some_and(|r| r.contains(p));
        let outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !inside(p)));
        if outside && !self.quick.fresh && !popup {
            self.quick.card = false;
            self.quick.sub = None;
        }
        self.quick.fresh = false;
        switch | pick.is_some_and(|p| self.quick_pick(&ctx, p))
    }

    #[allow(clippy::too_many_arguments)]
    fn account_card(&mut self, ui: &mut egui::Ui, t: &Tokens, themed: Option<&super::cosmetics::Themed>, me: &User, guild: Option<u64>, s: &super::profile::Shown, avail: f32) -> Option<Pick> {
        let c = &t.colors;
        let width = t.layout.profile_width;
        let pad = t.profile_pad();
        let name = super::profile::name_of(&me.name, s);
        let mut pick = None;
        let mut activity = None;
        // Ce qui suit l'en-tête défile dans la hauteur qui reste : la carte, ancrée
        // au-dessus du panneau, ne dépasse jamais sous la barre de titre et sa dernière
        // ligne n'est plus tranchée tant que la fenêtre a la place.
        let framed = egui::Frame::new().fill(c.bg1).stroke(t.border()).corner_radius(t.r_card()).shadow(t.shadow(Elevation::Window)).show(ui, |ui| {
            let bg = ui.painter().add(egui::Shape::Noop);
            if let Some(th) = themed {
                th.apply(ui);
            }
            ui.set_width(width);
            let left = ui.max_rect().left();
            let top = ui.cursor().top();
            self.profile_header(ui, t, width, me.id, guild, s, me.avatar, &name, Header { themed, bubble: true, ..Header::default() });
            // Hauteur de l'en-tête mesurée, pas estimée : l'estimation laissait un vide
            // sous la barre de titre et tranchait la dernière ligne.
            let max_height = (avail - (ui.cursor().top() - top) - 2.0 * t.shapes.border.max(1.0)).max(t.layout.profile_avatar);
            room(ui, max_height, |ui| {
                egui::ScrollArea::vertical().id_salt("mon-compte").max_height(max_height).show(ui, |ui| {
                    egui::Frame::new().inner_margin(t.profile_margin()).show(ui, |ui| {
                        ui.set_width(width - 2.0 * pad);
                        // Comme dans le client : statut personnalisé en bulle (en-tête), badges en
                        // capsule, bio repliée sans titre, puis l'activité en cours dans sa carte.
                        self.identity_card(ui, t, &name, s);
                        if let Some(bio) = s.profile.as_ref().and_then(Profile::bio) {
                            ui.add_space(t.spacing.item_y);
                            self.bio_folded(ui, t, me.id, guild, bio);
                        }
                        self.games_row(ui, t, s);
                        match self.activity_cards(ui, t, s) {
                            Some(super::activity::CardClick::Settings) => activity = Some(Pick::ActivitySettings),
                            Some(super::activity::CardClick::Full) => activity = Some(Pick::FullProfile),
                            _ => {}
                        }
                        ui.add_space(t.spacing.item_x);
                        pick = self.account_rows(ui, t, left..=left + width);
                        pick = pick.take().or(activity.take());
                    });
                })
            });
            bg
        });
        if let Some(th) = themed {
            super::profile::themed_background(ui.painter(), th, framed.inner, framed.response.rect, t.r_card(), Some(th.edge(t)));
        }
        self.paint_profile_effect(ui.painter(), s, framed.response.rect, t.r_card());
        pick
    }

    /// « Collection de jeux » : les jeux des widgets de mon profil, tels que le
    /// profil déjà reçu les donne (aucune requête d'ici). Une vignette par jeu
    /// dont l'image est connue (noms déjà demandés par l'éditeur de profil, ou
    /// liste des jeux détectables), trois au plus ; sinon le nombre de jeux.
    fn games_row(&self, ui: &mut egui::Ui, t: &Tokens, s: &super::profile::Shown) {
        let Some(p) = s.profile.as_ref() else { return };
        let mut ids: Vec<u64> = Vec::new();
        for g in p.widgets.iter().flat_map(|w| &w.games) {
            if !ids.contains(&g.game_id) {
                ids.push(g.game_id);
            }
        }
        if ids.is_empty() {
            return;
        }
        let image = |id: u64| {
            let hash = self.store.widgets.game(id).and_then(crate::state::widgets::Game::image).or_else(|| self.store.activity.index.as_ref()?.app(id)?.icon)?;
            Some(crate::state::model::ImageRef::AppIcon { app_id: id, hash })
        };
        let tiles: Vec<crate::state::model::ImageRef> = ids.iter().filter_map(|id| image(*id)).take(GAME_TILES).collect();
        let lang = self.lang;
        let side = t.spacing.control_height.min(t.layout.small_avatar.max(24.0));
        ui.add_space(t.spacing.item_x);
        let pad = t.spacing.item_x.round().clamp(0.0, 127.0) as i8;
        egui::Frame::new().fill(block_fill(t)).corner_radius(t.r_embed()).inner_margin(egui::Margin::symmetric(pad, (pad / 2).max(4))).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(side);
                let title = crate::i18n::table::profile::t::GAME_COLLECTION.get(lang);
                ui.label(RichText::new(title).font(super::fonts::demi_font(t.typography.small + 1.0)).color(t.colors.text_strong));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = t.spacing.item_y;
                    if tiles.is_empty() {
                        ui.label(RichText::new(ids.len().to_string()).small().color(t.colors.muted));
                    }
                    for tile in tiles.iter().rev() {
                        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                        ui.painter().rect_filled(rect, t.r_row(), t.colors.bg3);
                        super::util::paint_image(ui, &self.images, rect, Some(*tile), t.r_row());
                    }
                });
            });
        });
    }

    /// Les deux blocs de lignes de menu de la carte. `card_x` : bords gauche
    /// et droit de la carte, d'où partent les sous-menus.
    fn account_rows(&mut self, ui: &mut egui::Ui, t: &Tokens, card_x: std::ops::RangeInclusive<f32>) -> Option<Pick> {
        let lang = self.lang;
        let ready = self.store.account.proto_ready();
        let enabled = ready && !self.store.account.proto_busy();
        let now = self.store.now();
        let code = self.own_status_code().map(str::to_string);
        let status = code.as_deref().and_then(|c| STATUSES.iter().find(|(k, _)| *k == c)).map_or(qt::STATUS, |(_, l)| *l);
        let dot = code.as_deref().map_or(t.colors.muted, |c| status_color(t, c));
        let custom = self.saved_custom_status().filter(|c| !c.expired(now)).is_some();
        let open = self.quick.sub.as_ref().map(|s| s.kind);
        // Aperçu : pas à la passe de mesure de la carte, où ses lignes ne sont pas à leur place.
        let forced = if ui.is_sizing_pass() { None } else { self.quick.sub_wanted.take() };
        let dismissed = self.quick.sub_dismissed;
        // Lignes à sous-menu : place (le sous-menu ouvert la suit) et survol.
        let mut subs = [(SubKind::Status, egui::Rect::NOTHING, false), (SubKind::Accounts, egui::Rect::NOTHING, false)];
        let card = |lead: Lead, text: &'static str| Line { on_card: true, ..Line::new(lead, text) };
        let mut pick = None;
        // Ligne à sous-menu survolée ou activée, et ligne ordinaire survolée.
        let mut wanted = None;
        let mut plain = false;
        block(ui, t, |ui| {
            let r = menu::line(ui, t, card(Lead::Icon(Icon::Pencil), qt::EDIT_PROFILE.get(lang)));
            plain |= r.hovered();
            if r.clicked() {
                pick = Some(Pick::Edit);
            }
            let r = menu::line(ui, t, Line { mark: Mark::Submenu, open: open == Some(SubKind::Status), ..card(Lead::Dot(dot), status.get(lang)) });
            subs[0] = (SubKind::Status, r.rect, r.hovered());
            if (r.hovered() && dismissed != Some(SubKind::Status)) || r.clicked() || forced == Some(SubKind::Status) {
                wanted = Some((SubKind::Status, r.rect));
            }
            let label = if custom { qt::EDIT_CUSTOM_STATUS } else { qt::SET_CUSTOM_STATUS };
            let r = menu::line(ui, t, Line { mark: if custom { Mark::Slot } else { Mark::None }, ..card(Lead::Icon(Icon::Smile), label.get(lang)) });
            plain |= r.hovered();
            let mut cleared = false;
            if custom {
                // Croix posée sur la ligne : retire le statut d'un clic, sans ouvrir la fenêtre.
                let slot = menu::mark_slot(t, r.rect);
                let tip = qt::CLEAR_CUSTOM_STATUS.get(lang);
                let cross = ui.interact(slot.expand(t.spacing.item_y), clear_button_id(), if enabled { Sense::click() } else { Sense::hover() });
                cross.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, tip));
                let hot = enabled && (cross.hovered() || cross.has_focus());
                if hot {
                    ui.painter().circle_filled(slot.center(), slot.width() / 2.0 + t.spacing.item_y / 2.0, block_fill(t).gamma_multiply(2.0));
                }
                let ink = if hot { t.colors.text_strong } else { t.colors.text };
                icons::paint(Icon::Close, ui.painter(), slot.shrink(2.0), ink.gamma_multiply(if enabled { 1.0 } else { 0.45 }));
                cleared = cross.clicked();
                widgets::tip(cross, t, tip).on_hover_cursor(egui::CursorIcon::PointingHand);
            }
            if cleared {
                pick = Some(Pick::SaveCustom(None));
            } else if r.clicked() {
                pick = Some(Pick::CustomStatus);
            }
        });
        ui.add_space(t.spacing.item_x);
        block(ui, t, |ui| {
            let r = menu::line(ui, t, Line { mark: Mark::Submenu, open: open == Some(SubKind::Accounts), ..card(Lead::Icon(Icon::Members), qt::SWITCH_ACCOUNT.get(lang)) });
            subs[1] = (SubKind::Accounts, r.rect, r.hovered());
            if (r.hovered() && dismissed != Some(SubKind::Accounts)) || r.clicked() || forced == Some(SubKind::Accounts) {
                wanted = Some((SubKind::Accounts, r.rect));
            }
            let r = menu::line(ui, t, card(Lead::Icon(Icon::Id), crate::i18n::t::PROFILE_COPY_ID.get(lang)));
            plain |= r.hovered();
            if r.clicked() {
                pick = Some(Pick::CopyId);
            }
        });
        if !ready {
            ui.add_space(t.spacing.item_y);
            ui.add(egui::Label::new(RichText::new(st::NOT_LOADED.get(lang)).small().color(t.colors.muted)).wrap());
        }
        let time = ui.input(|i| i.time);
        let wait = |ui: &egui::Ui, secs: f64| ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(secs.max(0.0)));
        let anchor = |row: egui::Rect| egui::Rect::from_x_y_ranges(card_x.clone(), row.y_range());
        // Le pointeur a quitté la ligne du sous-menu fermé par Échap : elle peut le rouvrir.
        if subs.iter().any(|(kind, _, hovered)| dismissed == Some(*kind) && !hovered) || wanted.is_some() {
            self.quick.sub_dismissed = None;
        }
        match (wanted, self.quick.sub.as_mut()) {
            (Some((kind, row)), _) => self.quick.sub = Some(Sub { kind, anchor: anchor(row), leaving: None }),
            // Une autre ligne survolée : le sous-menu se ferme si le pointeur y reste.
            (None, Some(sub)) if plain => match sub.leaving {
                Some(since) if time - since >= SUB_LEAVE => self.quick.sub = None,
                Some(since) => wait(ui, SUB_LEAVE - (time - since)),
                None => {
                    sub.leaving = Some(time);
                    wait(ui, SUB_LEAVE);
                }
            },
            (None, Some(sub)) => sub.leaving = None,
            (None, None) => {}
        }
        // Le sous-menu ouvert suit sa ligne (carte replacée, contenu qui défile).
        if let Some(sub) = &mut self.quick.sub
            && let Some((_, row, _)) = subs.iter().find(|(kind, ..)| *kind == sub.kind)
        {
            sub.anchor = anchor(*row);
        }
        pick
    }

    /// Sous-menu ouvert, à droite de la carte (à gauche si la place manque),
    /// toujours dans la fenêtre. Rend la place qu'il occupe.
    fn account_submenu(&mut self, ctx: &egui::Context, pick: &mut Option<Pick>) -> Option<egui::Rect> {
        let (kind, anchor) = self.quick.sub.as_ref().map(|s| (s.kind, s.anchor))?;
        let t = self.theme.clone();
        let frame = t.menu_frame();
        let width = (t.layout.profile_width * 0.85).round().max(menu::MIN_WIDTH);
        let gap = t.spacing.item_y;
        let fits = anchor.right() + gap + width + frame.total_margin().sum().x <= ctx.content_rect().right();
        let top = anchor.top() - frame.total_margin().top;
        let (pivot, at) = if fits { (Align2::LEFT_TOP, egui::pos2(anchor.right() + gap, top)) } else { (Align2::RIGHT_TOP, egui::pos2(anchor.left() - gap, top)) };
        let shown = egui::Area::new(sub_id(kind)).order(egui::Order::Foreground).pivot(pivot).fixed_pos(at).constrain(true).show(ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing.y = 0.0;
                match kind {
                    SubKind::Status => self.status_lines(ui, &t, pick),
                    SubKind::Accounts => self.account_lines(ui, &t, width, pick),
                }
            });
        });
        Some(shown.response.rect)
    }

    /// Les quatre statuts, celui en cours coché ; un clic le change.
    fn status_lines(&self, ui: &mut egui::Ui, t: &Tokens, pick: &mut Option<Pick>) {
        let lang = self.lang;
        let current = self.own_status_code();
        let ready = self.store.account.proto_ready() && !self.store.account.proto_busy();
        ui.add_enabled_ui(ready, |ui| {
            for (i, (code, label)) in STATUSES.into_iter().enumerate() {
                if i == 1 {
                    menu::separator(ui, t);
                }
                let selected = current == Some(code);
                let line = Line { hint: status_hint(code).map(|h| h.get(lang)), mark: if selected { Mark::Icon(Icon::Check) } else { Mark::None }, ..Line::new(Lead::Dot(status_color(t, code)), label.get(lang)) };
                if menu::line(ui, t, line).clicked() && !selected {
                    *pick = Some(Pick::Status(code));
                }
            }
        });
        if !self.store.account.proto_ready() {
            ui.add(egui::Label::new(RichText::new(st::NOT_LOADED.get(lang)).small().color(t.colors.muted)).wrap());
        }
    }

    /// Les comptes enregistrés (un clic y bascule), puis « Gérer les comptes ».
    fn account_lines(&self, ui: &mut egui::Ui, t: &Tokens, width: f32, pick: &mut Option<Pick>) {
        let lang = self.lang;
        let current = self.current_account().cloned();
        let keys: Vec<AccountKey> = self.prefs.now.accounts.iter().map(|a| a.key.clone()).collect();
        let any = !keys.is_empty();
        for key in keys {
            let selected = current.as_ref() == Some(&key);
            let busy = self.account_busy(&key);
            let detail = busy.or(selected.then_some(crate::i18n::table::verbatim::t::ACCOUNT_OPEN.get(lang)));
            let name = self.account_label(&key);
            if self.account_row(ui, width, self.account_avatar(&key), &name, detail, selected).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && busy.is_none() && !selected {
                *pick = Some(Pick::Account(key));
            }
        }
        if any {
            menu::separator(ui, t);
        }
        if menu::line(ui, t, Line::new(Lead::Icon(Icon::Gear), qt::MANAGE_ACCOUNTS.get(lang))).clicked() {
            *pick = Some(Pick::Switch);
        }
    }

    /// Fenêtre du statut personnalisé : emoji, texte, durée ; « Enregistrer »,
    /// « Annuler » et, s'il y en a un, « Retirer le statut ».
    fn custom_status_modal(&mut self, ctx: &egui::Context) -> Option<Pick> {
        if !self.quick.status_modal {
            self.quick.status_picker = None;
            return None;
        }
        // Le sélecteur d'emoji d'abord : Échap ou un clic à côté le ferme, lui, pas la fenêtre.
        let picking_before = self.quick.status_picker.is_some();
        self.status_emoji_picker(ctx);
        let t = self.theme.clone();
        let c = &t.colors;
        let lang = self.lang;
        let ready = self.store.account.proto_ready();
        let enabled = ready && !self.store.account.proto_busy();
        let now = self.store.now();
        let h24 = self.h24();
        let saved = self.saved_custom_status();
        let fresh = std::mem::take(&mut self.quick.modal_fresh);
        let picking = self.quick.status_picker.is_some();
        let d = &mut self.params.status;
        d.sync(&saved);
        let toggle = Cell::new(None);
        // Brouillon lu par le pied de la fenêtre, après le contenu.
        let draft = RefCell::new(None::<CustomStatus>);
        let (enter, save, cancel, clear) = (Cell::new(false), Cell::new(false), Cell::new(false), Cell::new(false));
        let title = if saved.is_some() { qt::EDIT_CUSTOM_STATUS } else { qt::SET_CUSTOM_STATUS };
        let out = widgets::modal(
            ctx,
            &t,
            status_modal_id(),
            super::mobile::card_width(ctx, &t, 1.0),
            title.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.add_enabled_ui(ready, |ui| {
                    ui.horizontal(|ui| {
                        // L'emoji se choisit dans le sélecteur, comme pour une réaction.
                        let emoji = super::status_emoji::button(ui, &t, lang, d, picking);
                        if emoji.clicked() {
                            toggle.set(Some(emoji.rect));
                        }
                        let text = super::form::field(ui, &t, &mut d.text, qt::CUSTOM_STATUS_HINT.get(lang), 128);
                        super::a11y::name(ui.ctx(), text.id, st::CUSTOM_STATUS_TEXT.get(lang));
                        if fresh {
                            text.request_focus();
                        }
                        enter.set(text.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    });
                    ui.add_space(t.spacing.item_y);
                    widgets::field_label(ui, &t, st::CUSTOM_STATUS_EXPIRY.get(lang));
                    let label = |i: usize| EXPIRY[i.min(EXPIRY.len() - 1)].1.get(lang);
                    let combo = egui::ComboBox::from_id_salt("mon-compte-effacement").selected_text(label(d.expiry)).width(ui.available_width()).show_ui(ui, |ui| {
                        for i in 0..EXPIRY.len() {
                            ui.selectable_value(&mut d.expiry, i, label(i));
                        }
                    });
                    super::a11y::name(ui.ctx(), combo.response.id, st::CUSTOM_STATUS_EXPIRY.get(lang));
                });
                match &saved {
                    Some(s) if s.expired(now) => note(ui, &t, lang, st::CUSTOM_STATUS_EXPIRED),
                    Some(s) if s.expires_at_ms != 0 => {
                        ui.label(RichText::new(sf::custom_status_until(lang, time::until(lang, i64::try_from(s.expires_at_ms).unwrap_or(i64::MAX), h24))).small().color(c.muted));
                    }
                    _ => {}
                }
                if !ready {
                    note(ui, &t, lang, st::NOT_LOADED);
                }
                // Même texte et même emoji qu'enregistré, durée laissée : rien à enregistrer.
                let same = saved.as_ref().is_some_and(|s| s.text == d.text.trim() && s.emoji_name == d.emoji.trim()) && d.expiry == 0;
                let built = d.build(saved.as_ref(), now);
                *draft.borrow_mut() = (!built.is_empty() && !same).then_some(built);
            },
            |ui| {
                let changed = draft.borrow().is_some();
                ui.add_enabled_ui(enabled && changed, |ui| save.set(widgets::button(ui, &t, widgets::Kind::Primary, st::SAVE.get(lang)).clicked()));
                cancel.set(widgets::button(ui, &t, widgets::Kind::Link, st::CANCEL.get(lang)).clicked());
                if saved.is_some() {
                    ui.add_enabled_ui(enabled, |ui| clear.set(widgets::button(ui, &t, widgets::Kind::Link, st::CLEAR_STATUS.get(lang)).clicked()));
                }
                if ready && !enabled {
                    ui.label(RichText::new(st::SAVING.get(lang)).color(c.muted));
                }
                if enter.get() && enabled && changed {
                    save.set(true);
                }
            },
        );
        if let Some(anchor) = toggle.get() {
            self.quick.status_picker = if picking { None } else { Some(super::status_emoji::StatusPicker::at(anchor)) };
        }
        if save.get()
            && let Some(d) = draft.into_inner()
        {
            return Some(Pick::SaveCustom(Some(d)));
        }
        if clear.get() {
            return Some(Pick::SaveCustom(None));
        }
        // Fermer le sélecteur d'emoji (Échap, clic à côté) ne ferme pas la fenêtre avec lui.
        if cancel.get() || (out.closed && !picking_before) {
            self.close_custom_status();
        }
        None
    }
}

fn note(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, text: Text) {
    ui.label(RichText::new(text.get(lang)).small().color(t.colors.muted));
}

#[cfg(test)]
#[path = "quick_profile_tests.rs"]
pub(super) mod tests;
