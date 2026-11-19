//! Profils : pastille de statut sur les avatars, et carte de profil ouverte
//! d'un clic sur l'avatar ou le nom d'un auteur (ou, pour les autres zones,
//! par [`open`]).
//!
//! Les zones qui dessinent des avatars lisent les présences dans les tables
//! partagées (`state::people`), déposées dans le contexte d'egui à chaque
//! image ([`install`]) : aucune signature existante n'a à changer. Ce qui est
//! lu à une image compte comme affiché : seuls ces utilisateurs réveillent
//! l'interface quand leur présence change.
//!
//! Rien n'est inventé : un champ inconnu n'est pas affiché.

use super::cosmetics;
use super::util::{paint_initials, pixel_square};
use super::App;
use crate::images::{self, Image, Images};
use crate::i18n::{f, t, time, Lang, Text};
use crate::state::model::{snowflake_ms, Avatar, ImageRef, Role};
use crate::state::people::{Activity, ActivityKind, Badge, ClientStatus, MemberInfo, PeopleHub, Presence, Status, UserInfo};
use crate::state::profiles::{Note, Profile, ProfileState};
use crate::i18n::table::profile::{f as pf, t as pt};
use super::profile_full::Tab;
use crate::theme::{StatusDots, Tokens};
use eframe::egui::{self, emath::GuiRounding, Color32, Id, RichText, Sense};
use std::hash::Hash;

fn hub_id() -> Id {
    Id::unique("ferrocord.profils.tables")
}

fn card_id() -> Id {
    Id::unique("ferrocord.profils.carte")
}

/// Tables de la connexion, pour les zones qui dessinent des avatars.
pub(super) fn install(ctx: &egui::Context, hub: &PeopleHub) {
    ctx.data_mut(|d| d.insert_temp(hub_id(), hub.clone()));
}

pub(super) fn hub(ctx: &egui::Context) -> Option<PeopleHub> {
    ctx.data(|d| d.get_temp::<PeopleHub>(hub_id()))
}

/// Carte demandée ou ouverte.
#[derive(Clone)]
pub(super) struct Card {
    pub(super) user_id: u64,
    pub(super) guild_id: Option<u64>,
    /// Ouverte depuis un endroit qui ne connaît pas le serveur (mention,
    /// menu) : celui affiché, résolu à la première image.
    here: bool,
    /// Zone cliquée : la carte s'ouvre à côté.
    anchor: egui::Rect,
    /// Nom et avatar vus là où l'on a cliqué : affichés si l'utilisateur
    /// n'est pas (encore) dans les tables.
    name: String,
    avatar: Avatar,
    /// Ouverte à cette image : ce clic ne la referme pas, et les compléments
    /// sont demandés.
    fresh: bool,
}

/// Ouvre la carte de profil de `user_id` (sur un serveur : avec son membre).
pub(super) fn open(ctx: &egui::Context, user_id: u64, guild_id: Option<u64>, anchor: egui::Rect, name: &str, avatar: Avatar) {
    let card = Card { user_id, guild_id, here: false, anchor, name: name.to_string(), avatar, fresh: true };
    ctx.data_mut(|d| d.insert_temp(card_id(), card));
}

/// Ouvre la carte de `user_id` sur le serveur affiché (mention d'un message,
/// menu d'un utilisateur) ; nom et avatar viennent des tables s'ils y sont.
pub(super) fn open_here(ctx: &egui::Context, user_id: u64, name: &str, anchor: egui::Rect) {
    let card = Card { user_id, guild_id: None, here: true, anchor, name: name.to_string(), avatar: Avatar::default_for(user_id), fresh: true };
    ctx.data_mut(|d| d.insert_temp(card_id(), card));
}

/// Zone cliquable qui ouvre la carte (avatar ou nom d'un auteur, ami) ; clic
/// droit : menu de l'utilisateur. À appeler après la zone du message
/// entier, pour recevoir le clic avant elle.
#[allow(clippy::too_many_arguments)]
pub(super) fn hotspot(ui: &egui::Ui, lang: Lang, rect: egui::Rect, salt: impl Hash + std::fmt::Debug, user_id: u64, guild_id: Option<u64>, name: &str, avatar: Avatar) {
    let r = ui.interact(rect, Id::unique(("ferrocord.profils.zone", salt)), Sense::click());
    // Pas d'infobulle, comme dans le client : elle apparaissait par-dessus les messages
    // (« Voir le profil » en fondu, pris pour un bouton fantôme).
    let label = t::PROFILE_OPEN.get(lang);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let r = r.on_hover_cursor(egui::CursorIcon::PointingHand);
    if r.clicked() {
        open(ui.ctx(), user_id, guild_id, rect, name, avatar);
    }
    if r.secondary_clicked() {
        open_menu(ui.ctx(), user_id, name, rect);
    }
}

/// Ouvre le menu de l'utilisateur (clic droit) à côté de `rect`.
pub(super) fn open_menu(ctx: &egui::Context, user_id: u64, name: &str, rect: egui::Rect) {
    let at = ctx.input(|i| i.pointer.interact_pos()).unwrap_or(rect.center());
    super::profile_full::open_menu(ctx, user_id, name, at);
}

/// Pastille de statut dessinée sur l'avatar `avatar` (rien si le statut est
/// inconnu) ; `ring` : couleur du fond derrière l'avatar.
pub(super) fn status_dot(ui: &egui::Ui, t: &Tokens, avatar: egui::Rect, user_id: u64, ring: Color32) {
    let typing = super::motion::typists(ui.ctx()).here(user_id);
    dot(ui, t, avatar, user_id, ring, typing);
}

/// Pastille de statut ; `typing` : la personne écrit, la pastille s'élargit
/// en pilule à trois points (lot ANIM).
fn dot(ui: &egui::Ui, t: &Tokens, avatar: egui::Rect, user_id: u64, ring: Color32, typing: bool) {
    if !ui.is_rect_visible(avatar) {
        return;
    }
    let Some(hub) = hub(ui.ctx()) else { return };
    let status = hub.lock().status(user_id);
    if let Some(s) = status {
        t.paint_status(ui.painter(), avatar, s, ring);
        // Une clé par emplacement : la même personne peut écrire dans un MP et pas dans le salon ouvert.
        let key = egui::Id::unique(("pastille", user_id, avatar.left().to_bits()));
        super::motion::typing_pill(ui, key, typing, t.status_dot(avatar), t.status_color(s), ring);
    }
}

/// Statut connu du magasin de présences, s'il y en a un (liste des membres).
pub(super) fn known_status(ctx: &egui::Context, user_id: u64) -> Option<Status> {
    hub(ctx)?.lock().status(user_id)
}

/// Pastille d'un auteur de message, si le réglage l'affiche.
pub(super) fn message_dot(ui: &egui::Ui, t: &Tokens, avatar: egui::Rect, user_id: u64) {
    if t.layout.status_dots == StatusDots::Shown {
        status_dot(ui, t, avatar, user_id, t.colors.bg2);
    }
}

/// Pastille du correspondant d'un MP à deux.
pub(super) fn dm_dot(ui: &egui::Ui, t: &Tokens, avatar: egui::Rect, channel_id: u64, ring: Color32) {
    let Some(hub) = hub(ui.ctx()) else { return };
    let (peer, status) = {
        let mut people = hub.lock();
        let Some(peer) = people.dm_peer(channel_id) else { return };
        // Compte système de Discord : pas de pastille de statut, comme dans le client.
        if peer == crate::state::store::official::DISCORD_SYSTEM_USER || people.user(peer).is_some_and(|u| u.system) {
            return;
        }
        (peer, people.status(peer))
    };
    // Comme dans le client : un correspondant hors ligne garde sa pastille (anneau gris).
    if status.is_none() {
        if ui.is_rect_visible(avatar) {
            t.paint_status(ui.painter(), avatar, Status::Offline, ring);
        }
        return;
    }
    let typing = super::motion::typists(ui.ctx()).in_channel(channel_id, peer);
    dot(ui, t, avatar, peer, ring, typing);
}

pub(super) fn status_text(s: Status) -> Text {
    match s {
        Status::Online => t::STATUS_ONLINE,
        Status::Idle => t::STATUS_IDLE,
        Status::Dnd => t::STATUS_DND,
        Status::Offline => t::STATUS_OFFLINE,
    }
}

fn badge_text(b: Badge) -> Text {
    match b {
        Badge::Staff => t::BADGE_STAFF,
        Badge::Partner => t::BADGE_PARTNER,
        Badge::CertifiedModerator => t::BADGE_MODERATOR,
        Badge::Hypesquad => t::BADGE_HYPESQUAD,
        Badge::HypesquadBravery => t::BADGE_BRAVERY,
        Badge::HypesquadBrilliance => t::BADGE_BRILLIANCE,
        Badge::HypesquadBalance => t::BADGE_BALANCE,
        Badge::BugHunter => t::BADGE_BUG_HUNTER,
        Badge::BugHunterGold => t::BADGE_BUG_HUNTER_GOLD,
        Badge::ActiveDeveloper => t::BADGE_ACTIVE_DEVELOPER,
        Badge::VerifiedDeveloper => t::BADGE_VERIFIED_DEVELOPER,
        Badge::EarlySupporter => t::BADGE_EARLY_SUPPORTER,
        Badge::VerifiedBot => t::BADGE_VERIFIED_BOT,
    }
}

/// Ligne principale d'une activité (« Joue à … ») ; statut personnalisé : son texte.
pub(super) fn activity_line(lang: Lang, a: &Activity) -> String {
    match a.kind {
        ActivityKind::Playing => f::playing(lang, &a.name),
        // En direct : `details` est le titre du direct, `name` la plateforme.
        ActivityKind::Streaming => f::streaming(lang, a.details.as_deref().unwrap_or(&a.name)),
        ActivityKind::Listening => f::listening(lang, &a.name),
        ActivityKind::Watching => f::watching(lang, &a.name),
        ActivityKind::Competing => f::competing(lang, &a.name),
        ActivityKind::Custom => custom_line(a),
    }
}

/// Statut personnalisé : emoji Unicode, ou `:nom:` d'un emoji personnalisé
/// (son image n'est pas chargée ici), puis le texte.
pub(super) fn custom_line(a: &Activity) -> String {
    let emoji = a.emoji.as_ref().and_then(|e| match (e.id, &e.name) {
        (None, Some(n)) => Some(n.clone()),
        (Some(_), Some(n)) => Some(format!(":{n}:")),
        _ => None,
    });
    match (emoji, &a.state) {
        (Some(e), Some(s)) => format!("{e} {s}"),
        (Some(e), None) => e,
        (None, Some(s)) => s.clone(),
        (None, None) => String::new(),
    }
}

fn platforms(lang: Lang, c: &ClientStatus) -> Vec<String> {
    [(t::PLATFORM_DESKTOP, c.desktop), (t::PLATFORM_MOBILE, c.mobile), (t::PLATFORM_WEB, c.web)]
        .into_iter()
        .filter_map(|(name, s)| Some(f::platform_status(lang, name.get(lang), status_text(s?).get(lang))))
        .collect()
}

/// Ce que la carte affiche, copié sous le verrou une fois par image.
pub(super) struct Shown {
    /// Utilisateur montré.
    pub(super) id: u64,
    /// Surnom que je lui ai donné (amis seulement).
    pub(super) friend_nick: Option<String>,
    pub(super) user: Option<UserInfo>,
    pub(super) member: Option<MemberInfo>,
    pub(super) presence: Option<Presence>,
    /// Profil complet reçu (`GET /users/{id}/profile`).
    pub(super) profile: Option<Profile>,
    pub(super) state: Option<ProfileState>,
    pub(super) note: Option<Note>,
}

impl Shown {
    pub(super) fn captcha(&self) -> bool {
        self.state == Some(ProfileState::Captcha)
    }
}

impl App {
    /// Ouvre la carte de profil de `user_id` à côté du point `at` (aperçu ;
    /// pour l'interface, voir [`hotspot`] et [`open`]).
    pub fn open_profile(&self, user_id: u64, guild_id: Option<u64>, at: egui::Pos2) {
        let known = self.store.people.lock().user(user_id).map(|u| (u.display_name().to_string(), u.avatar));
        let (name, avatar) = known.unwrap_or_else(|| (String::new(), Avatar::default_for(user_id)));
        open(&self.ctx, user_id, guild_id, egui::Rect::from_min_size(at, egui::Vec2::ZERO), &name, avatar);
    }

    /// Ce que l'on sait de `user_id` ; il compte comme affiché à cette image.
    pub(super) fn shown(&self, user_id: u64, guild_id: Option<u64>) -> Shown {
        let mut p = self.store.people.lock();
        p.watch(user_id);
        let state = p.profiles().best(user_id, guild_id).cloned();
        let mut shown = Shown {
            id: user_id,
            friend_nick: self.store.relations.relation(user_id).filter(|r| r.kind == crate::state::relations::RelKind::Friend).and_then(|r| r.nickname.clone()).filter(|n| !n.trim().is_empty()),
            user: p.user(user_id).cloned(),
            member: guild_id.and_then(|g| p.member(g, user_id).cloned()),
            presence: p.presence(user_id).cloned(),
            profile: state.as_ref().and_then(ProfileState::profile).cloned(),
            state,
            note: p.profiles().note(user_id).cloned(),
        };
        drop(p);
        if self.store.me.as_ref().is_some_and(|u| u.id == user_id) {
            self.own_activities(&mut shown.presence);
            // Statut et statut personnalisé : ceux de mes réglages (carte de mon compte).
            self.own_presence_shown(&mut shown.presence);
        }
        shown
    }

    /// Mon profil : mes activités telles qu'elles partent (jeu détecté, Rich
    /// Presence locale, lot R), Discord ne me renvoyant pas ma présence.
    fn own_activities(&self, presence: &mut Option<Presence>) {
        let Some(w) = self.store.own_presence() else { return };
        let mine = w.shown_activities();
        if mine.is_empty() {
            return;
        }
        let p = presence.get_or_insert_with(|| Presence { status: Status::parse(&w.status).unwrap_or(Status::Online), client: Default::default(), activities: Vec::new() });
        p.activities.retain(|a| a.kind == ActivityKind::Custom);
        p.activities.extend(mine);
    }

    /// Fenêtres des profils : menu d'un utilisateur, confirmation, profil
    /// complet, puis la carte ouverte, s'il y en a une (Échap ou un clic en
    /// dehors la ferme).
    pub(super) fn profile_ui(&mut self, ui: &mut egui::Ui) {
        self.user_menu_ui(ui);
        self.relation_confirm_ui(ui);
        self.full_profile_ui(ui);
        let ctx = ui.ctx().clone();
        let Some(mut card) = ctx.data(|d| d.get_temp::<Card>(card_id())) else { return };
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            return close(&ctx);
        }
        if card.here {
            card.here = false;
            card.guild_id = self.store.guild;
            if let Some(u) = self.store.people.lock().user(card.user_id) {
                card.avatar = u.avatar;
            }
        }
        if card.fresh {
            self.store.want_profile(card.user_id, card.guild_id);
            self.store.want_note(card.user_id);
        }
        let shown = self.shown(card.user_id, card.guild_id);
        self.want_profile_effect(&shown);
        // Journal de débogage : la forme des badges reçus, une fois par profil.
        if self.local.prefs.debug_log
            && let Some(shape) = shown.profile.as_ref().map(|p| p.badge_shape.clone()).filter(|s| !s.is_empty())
        {
            let once = egui::Id::unique(("ferrocord.journal.badges", card.user_id));
            if !self.ctx.data(|d| d.get_temp::<bool>(once).unwrap_or(false)) {
                self.ctx.data_mut(|d| d.insert_temp(once, true));
                self.local_log(&format!("badges du profil : {shape}"));
            }
        }
        let t = self.theme.clone();
        let gap = t.spacing.item_x;
        let mut pos = egui::pos2(card.anchor.right() + gap, card.anchor.top());
        // Disposition mobile : la carte, centrée, ne colle à aucun bord de l'écran.
        if super::mobile::is_active(&ctx) {
            let screen = ctx.content_rect();
            pos.x = screen.left() + ((screen.width() - t.layout.profile_width) / 2.0).max(0.0);
        }
        // Une marge sous la carte : elle ne descend pas jusqu'au bord de la fenêtre.
        if let Some(last) = ctx.memory(|m| m.area_rect(card_id())) {
            let screen = ctx.content_rect();
            let pad = t.profile_pad();
            pos.y = pos.y.min(screen.bottom() - pad - last.height()).max(screen.top() + pad);
        }
        let mut action = None;
        let area = egui::Area::new(card_id()).order(egui::Order::Foreground).fixed_pos(pos).constrain(true).show(&ctx, |ui| {
            action = self.card_ui(ui, &t, &card, &shown);
        });
        match action {
            Some(Action::Message) => {
                self.store.message_user(card.user_id);
                return close(&ctx);
            }
            Some(Action::Send(text)) => {
                self.send_from_card(card.user_id, text);
                return close(&ctx);
            }
            Some(Action::CopyId) => ctx.copy_text(card.user_id.to_string()),
            Some(Action::Full(tab)) => {
                super::profile_full::open_full(&ctx, card.user_id, card.guild_id, &display_name(&card, &shown), card.avatar, tab);
                return close(&ctx);
            }
            Some(Action::Moderate(guild_id, kind)) => {
                self.open_member_action_in(guild_id, card.user_id, &display_name(&card, &shown), kind);
                return close(&ctx);
            }
            Some(Action::Button(c)) => self.store.activity_button(card.user_id, &c.session_id, c.app_id, c.index),
            Some(Action::ActivitySettings) => {
                self.show_settings(super::settings::Page::Activity);
                return close(&ctx);
            }
            Some(Action::Edit) => {
                self.open_profile_editor(card.guild_id.is_some());
                return close(&ctx);
            }
            Some(Action::Roles(guild_id)) => {
                self.open_member_dialog(guild_id, card.user_id);
                return close(&ctx);
            }
            Some(Action::Head(p)) => self.head_pick(&ctx, card.user_id, &display_name(&card, &shown), p),
            None => {}
        }
        let outside = ctx.input(|i| i.pointer.any_click() && i.pointer.interact_pos().is_some_and(|p| !area.response.rect.contains(p)));
        if outside && !card.fresh {
            // La note en cours de saisie est enregistrée avant de fermer.
            if let Some(text) = super::profile_full::pending_note(&ctx, card.user_id, shown.note.as_ref()) {
                self.store.save_note(card.user_id, &text);
            }
            return close(&ctx);
        }
        card.fresh = false;
        ctx.data_mut(|d| d.insert_temp(card_id(), card));
    }

    fn card_ui(&self, ui: &mut egui::Ui, t: &Tokens, card: &Card, s: &Shown) -> Option<Action> {
        let lang = self.lang;
        // Couleurs de thème du profil : fond en dégradé sur toute la carte
        // (sa hauteur de l'image d'avant), texte recalculé.
        let mut themed = profile_theme(t, s);
        if let (Some(th), Some(last)) = (&mut themed, ui.ctx().memory(|m| m.area_rect(card_id()))) {
            th.set_span(last.height());
        }
        let t = themed.as_ref().map_or(t, |th| &th.tokens);
        let c = &t.colors;
        let width = t.layout.profile_width;
        let pad = t.profile_pad();
        let mut action = None;
        let name = display_name(card, s);
        let screen = ui.ctx().content_rect();
        let top = ui.cursor().top();
        let me = self.store.me.as_ref().map(|u| u.id);
        let framed = egui::Frame::new().fill(c.bg1).stroke(t.border()).corner_radius(t.r_card()).show(ui, |ui| {
            // Place du fond en dégradé, derrière tout le contenu.
            let bg = ui.painter().add(egui::Shape::Noop);
            if let Some(th) = &themed {
                th.apply(ui);
            }
            ui.set_width(width);
            let head = ui.cursor().min;
            self.profile_header(ui, t, width, card.user_id, card.guild_id, s, card.avatar, &name, Header { themed: themed.as_ref(), bubble: true, ..Header::default() });
            if let Some(p) = self.header_buttons(ui, t, egui::pos2(head.x + width - pad, head.y + pad), card.user_id) {
                action = Some(Action::Head(p));
            }
            // Hauteur de l'en-tête mesurée (et non estimée) : la carte tient dans la
            // fenêtre avec une marge, sous la barre de titre ; son pied reste visible.
            let used = ui.cursor().top() - top;
            let foot = full_button_height(t);
            let max_height = (screen.height() - used - foot - 2.0 * pad).max(t.layout.profile_avatar);
            room(ui, max_height, |ui| egui::ScrollArea::vertical().id_salt("profil-carte").max_height(max_height).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { top: 0, ..t.profile_margin() }).show(ui, |ui| {
                    ui.set_width(width - 2.0 * pad);
                    self.card_body(ui, t, card.user_id, card.guild_id, &name, s, true, &mut action);
                    // Identifiant : mode développeur, et pas masqué par le mode streamer.
                    let copy_id = super::settings::local::copy_id_shown(ui.ctx(), &self.store);
                    if copy_id {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(t::PROFILE_ID.get(lang)).small().color(c.faint));
                            ui.label(RichText::new(card.user_id.to_string()).small().color(c.muted));
                        });
                    }
                    ui.add_space(t.spacing.item_y);
                    // Ma propre carte : l'édition rapide, sans passer par les paramètres.
                    if me == Some(card.user_id) && super::widgets::button_wide(ui, t, super::widgets::Kind::Primary, crate::i18n::table::quick::t::EDIT_PROFILE.get(lang), ui.available_width()).clicked() {
                        action = Some(Action::Edit);
                    }
                    // « Envoyer un message à @nom » (lot FF2, F24) : un vrai champ. Entrée
                    // envoie le texte et ouvre la conversation ; vide, elle l'ouvre seulement.
                    if me != Some(card.user_id) {
                        let id = Id::unique(("profil-carte-message", card.user_id));
                        let hint = f::message_dm(lang, &name);
                        let mut text: String = ui.data(|d| d.get_temp(id)).unwrap_or_default();
                        let edit = ui.add(egui::TextEdit::singleline(&mut text).hint_text(RichText::new(&hint).color(c.muted)).char_limit(CARD_MESSAGE_MAX).desired_width(f32::INFINITY));
                        super::a11y::name(ui.ctx(), edit.id, &hint);
                        if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            action = Some(if text.trim().is_empty() { Action::Message } else { Action::Send(std::mem::take(&mut text)) });
                        }
                        ui.data_mut(|d| d.insert_temp(id, text));
                    }
                    ui.horizontal_wrapped(|ui| {
                        if copy_id && ui.add(super::form::link(t, t::PROFILE_COPY_ID.get(lang))).clicked() {
                            action = Some(Action::CopyId);
                        }
                        if let Some(g) = card.guild_id
                            && self.card_offers_roles(Some(g), card.user_id)
                            && ui.add(super::form::link(t, crate::i18n::table::roles::t::CARD_ROLES.get(lang))).clicked()
                        {
                            action = Some(Action::Roles(g));
                        }
                        // Modération (lot A4), les mêmes actions que le clic droit.
                        if let Some(g) = card.guild_id {
                            for k in self.moderation_picks_in(g, card.user_id) {
                                let b = if k.danger() { super::moderation::danger_button(t, k.label().get(lang)) } else { super::form::btn(t, k.label().get(lang)) };
                                if ui.add(b).clicked() {
                                    action = Some(Action::Moderate(g, k));
                                }
                            }
                        }
                    });
                });
            }));
            // « Voir le profil complet » : pleine largeur, collé en bas, hors de la zone qui défile.
            if full_button(ui, t, lang, width) {
                action = Some(Action::Full(Tab::About));
            }
            bg
        });
        if let Some(th) = &themed {
            themed_background(ui.painter(), th, framed.inner, framed.response.rect, t.r_card(), Some(th.edge(t)));
        }
        // Effet de profil (lot ND) : par-dessus la carte, sans prendre les clics.
        self.paint_profile_effect(ui.painter(), s, framed.response.rect, t.r_card());
        action
    }

    /// Corps de la carte, sous l'en-tête, dans l'ordre du client : nom,
    /// identifiant et pronoms, badges, amis et serveurs en commun, activités,
    /// bio (sans titre), « Membre depuis », rôles, comptes connectés. Partagé
    /// par la carte et le panneau de profil d'un MP. La note et la ligne de
    /// statut en texte n'y sont pas : la note est dans le profil complet, la
    /// pastille de l'avatar dit le statut.
    /// `fold_bio` : bio repliée à quelques lignes (carte) ; entière sinon (panneau d'un MP).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn card_body(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, guild_id: Option<u64>, name: &str, s: &Shown, fold_bio: bool, action: &mut Option<Action>) {
        let lang = self.lang;
        let gap = t.spacing.item_x;
        self.identity_card(ui, t, name, s);
        if let Some(tab) = self.mutuals_ui(ui, t, s) {
            *action = Some(Action::Full(tab));
        }
        if s.captcha() {
            ui.add(egui::Label::new(RichText::new(pt::CAPTCHA.get(lang)).color(t.colors.danger)).wrap());
        }
        if let Some(bio) = s.profile.as_ref().and_then(Profile::bio) {
            ui.add_space(gap * 1.5);
            if fold_bio {
                self.bio_folded(ui, t, user_id, guild_id, bio);
            } else {
                self.bio_ui(ui, t, user_id, guild_id, bio);
            }
        }
        ui.add_space(gap * 1.5);
        self.since_ui(ui, t, user_id, guild_id, s);
        // Activités : après « Membre depuis », chacune dans sa carte.
        match self.activity_cards(ui, t, s) {
            Some(super::activity::CardClick::Button(b)) => *action = Some(Action::Button(b)),
            Some(super::activity::CardClick::Settings) => *action = Some(Action::ActivitySettings),
            Some(super::activity::CardClick::Full) => *action = Some(Action::Full(Tab::Activity)),
            None => {}
        }
        if let (Some(guild_id), Some(m)) = (guild_id, &s.member) {
            ui.add_space(gap);
            // Droit de gérer les rôles : une pastille « + » à la fin de la rangée (lot FF2, F24).
            if self.roles_ui(ui, t, guild_id, m, false, self.can_change_roles(guild_id)) {
                *action = Some(Action::Roles(guild_id));
            }
        }
        if let Some(list) = s.profile.as_ref().map(|p| p.connections.as_slice()).filter(|l| !l.is_empty()) {
            ui.add_space(gap);
            super::profile_parts::connections_row(ui, t, lang, list);
        }
    }

    /// Bannière (image : celle du serveur, sinon du compte ; recadrée au
    /// centre, animée tant que le profil est ouvert si les réglages le
    /// permettent), sinon couleur d'accent ou du thème du profil, sinon bande
    /// unie ; grand avatar à cheval (animé de même), cerclé du fond de la
    /// carte, avec sa décoration. Réserve sa hauteur dans `ui`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn profile_header(&self, ui: &mut egui::Ui, t: &Tokens, width: f32, user_id: u64, guild_id: Option<u64>, s: &Shown, fallback: Avatar, name: &str, h: Header<'_>) {
        let c = &t.colors;
        let pad = t.profile_pad();
        let top = ui.cursor().min;
        let user = s.user.as_ref();
        let member = guild_id.zip(s.member.as_ref());
        // Bannière du serveur : celle du profil de serveur, sinon celle de l'objet membre.
        let guild_hash = s.profile.as_ref().and_then(|p| p.guild.as_ref()).and_then(|g| g.banner).or(s.member.as_ref().and_then(|m| m.banner));
        let guild_banner = guild_id.zip(guild_hash).map(|(guild_id, hash)| ImageRef::MemberBanner { guild_id, user_id, hash });
        let user_banner = user.and_then(|u| u.banner.flatten()).or(s.profile.as_ref().and_then(|p| p.user.banner)).map(|hash| ImageRef::Banner { user_id, hash });
        // Aperçu de l'édition : l'image choisie remplace celle du profil.
        let banner = if h.banner.is_some() { None } else { guild_banner.or(user_banner) };
        let accent = user.and_then(|u| u.accent.flatten()).or(s.profile.as_ref().and_then(|p| p.user.accent)).or(s.profile.as_ref().and_then(Profile::theme).map(|[a, _]| a)).map(cosmetics::rgb);
        let has_banner = banner.is_some() || h.banner.is_some();
        // Profil complet dans une fenêtre basse : la bannière cède la place au contenu.
        // Panneau d'un MP : la bande a la hauteur d'une bannière même sans image, comme dans le client.
        let nominal = if h.full { t.profile_full_banner(has_banner) } else { t.profile_banner(has_banner || h.panel) };
        let height = if h.full { nominal.min((ui.ctx().content_rect().height() * 0.25).round()) } else { nominal };
        // Calée sur les pixels : une bannière nette ne chevauche pas deux pixels.
        let ppp = ui.pixels_per_point();
        let on_pixels = |v: f32| (v * ppp).round() / ppp;
        let mut band = egui::Rect::from_min_size(top, egui::vec2(width, height)).round_to_pixels(ppp);
        // Côtés arrondis à part : ils ne dépendent pas de la position de la
        // carte, sinon la texture recadrée changerait de taille (et serait
        // redemandée) dès que la carte bouge d'une fraction de pixel.
        band.max = band.min + egui::vec2(on_pixels(width), on_pixels(height));
        let band_r = egui::CornerRadius { sw: 0, se: 0, ..t.r_card() };
        let play = cosmetics::profile_play(ui);
        let member_avatar = member.and_then(|(guild_id, m)| Some(ImageRef::MemberAvatar { guild_id, user_id, hash: m.avatar? }));
        let base = user.map(|u| u.avatar).unwrap_or(fallback);
        // Sans bannière ni couleur d'accent : la couleur moyenne de l'avatar, comme le
        // client (connue dès que l'avatar est décodé ; d'ici là, la bande unie).
        let tint = || member_avatar.or(base.image).and_then(|i| self.images.avatar_tint(i));
        ui.painter().rect_filled(band, band_r, accent.or_else(tint).unwrap_or(c.bg0));
        // Image demandée à la hauteur nominale, même si une fenêtre basse en
        // montre moins (le milieu) : ni l'image fixe ni l'animation ne sont
        // redemandées à chaque taille de fenêtre, ce qui vidait la bande
        // pendant un redimensionnement.
        let asked = on_pixels(nominal).max(band.height());
        if let Some(b) = banner
            && let Image::Ready(tex) = self.images.get_fill_moving(b, band.width(), asked, cosmetics::BANNER_ASPECT, play)
        {
            let cut = ((1.0 - band.height() / asked) / 2.0).clamp(0.0, 0.5);
            ui.painter().add(images::textured_part(band, band_r, tex, egui::Rect::from_min_max(egui::pos2(0.0, cut), egui::pos2(1.0, 1.0 - cut))));
        }
        if let Some(sw) = h.banner {
            ui.painter().add(sw.shape(band, band_r));
        }
        let side = if h.full { t.profile_full_avatar() } else { t.layout.profile_avatar };
        let ring = t.profile_ring();
        // À cheval sur le bas de la bande ; dans le panneau d'un MP, un peu plus haut (mesuré sur le client).
        let over = if h.panel { (side * 0.6).round() } else { side / 2.0 };
        let avatar_rect = pixel_square(ui, egui::pos2(top.x + pad, band.bottom() - over), side);
        // Anneau de la couleur du fond à cet endroit (dégradé du thème compris).
        let ring_color = h.themed.map_or(c.bg1, |th| th.at(h.window_top.unwrap_or(top.y), avatar_rect.center().y.max(band.bottom())));
        ui.painter().rect_filled(avatar_rect.expand(ring), t.r_avatar(side + 2.0 * ring), ring_color);
        let image = if h.avatar.is_some() { None } else { member_avatar.or(base.image) };
        match (h.avatar, image.map(|i| self.images.get_moving_still_first(i, side, play))) {
            (Some(sw), _) => {
                ui.painter().add(sw.shape(avatar_rect, t.r_avatar(side)));
            }
            (None, Some(Image::Ready(tex))) => {
                ui.painter().add(images::textured(avatar_rect, t.r_avatar(side), tex));
            }
            _ => paint_initials(ui, t, avatar_rect, t.default_avatar(base.default), name),
        }
        let decoration = s.member.as_ref().and_then(|m| m.decoration).or(user.and_then(|u| u.look.decoration()));
        if let Some(asset) = decoration {
            cosmetics::paint_decoration(ui, &self.images, avatar_rect, asset, play);
        }
        if let Some(p) = &s.presence {
            // En ligne depuis un mobile seulement : la pastille est un téléphone.
            if super::profile_parts::mobile_only(p) {
                super::profile_parts::paint_mobile_status(ui.painter(), t, avatar_rect, p.status, ring_color);
            } else {
                t.paint_status(ui.painter(), avatar_rect, p.status, ring_color);
            }
        }
        let mut below = band.height() + side - over + ring;
        // Statut personnalisé en bulle de pensée, à droite de l'avatar.
        if h.bubble
            && let Some(a) = s.presence.as_ref().and_then(Presence::custom)
        {
            let left = avatar_rect.right() + ring + 2.0 * t.spacing.item_x;
            let at = egui::pos2(left, band.bottom() + t.spacing.item_y);
            if let Some(r) = super::profile_parts::status_bubble(ui, t, a, at, top.x + width - pad - left, ring_color) {
                below = below.max(r.bottom() - top.y + t.spacing.item_y);
            }
        }
        ui.add_space(below);
    }

    /// Nom en grand (au style de nom du profil : couleurs, effet, police la
    /// plus proche), étiquettes ; ligne « identifiant • pronoms » et tag de
    /// serveur ; ancien nom ; rangée de badges (`profile_parts`).
    pub(super) fn identity_ui(&self, ui: &mut egui::Ui, t: &Tokens, name: &str, s: &Shown) {
        self.identity_impl(ui, t, name, s, false);
    }

    /// Identité d'une carte (carte de profil, panneau d'un MP, carte de mon
    /// compte), comme dans le client : le nom, puis sur une même ligne
    /// l'identifiant, les pronoms, le tag de serveur et les badges dans leur
    /// capsule (six au plus, puis « +N ») ; pas d'ancien nom.
    pub(super) fn identity_card(&self, ui: &mut egui::Ui, t: &Tokens, name: &str, s: &Shown) {
        self.identity_impl(ui, t, name, s, true);
    }

    fn identity_impl(&self, ui: &mut egui::Ui, t: &Tokens, name: &str, s: &Shown, card: bool) {
        let lang = self.lang;
        let c = &t.colors;
        let user = s.user.as_ref();
        let look = user.map(|u| u.look).unwrap_or_default();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            // Nom en grand (20), comme dans le client.
            let size = (t.typography.title * 1.25).round();
            cosmetics::styled_name(ui, name, size, look.name_style(), c.bg1, c.text_strong);
            if user.is_some_and(|u| u.system) {
                // Compte système de Discord : « ✓ OFFICIEL » à la place de « Bot » et « Système ».
                super::official::tag(ui, t, lang, size);
            } else if user.is_some_and(|u| u.bot) {
                chip(ui, t, t::PROFILE_BOT.get(lang), c.accent, c.on_accent);
            }
        });
        let pronouns = s.profile.as_ref().and_then(Profile::pronouns);
        let tag = look.tag();
        let badges = card && self.has_badges(s);
        if user.is_some() || pronouns.is_some() || tag.is_some() || badges {
            // Nom d'utilisateur, puis pronoms, séparés par un point médian ; le tag de serveur ensuite.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_y, t.spacing.item_y);
                let size = t.typography.button;
                if let Some(u) = user {
                    ui.label(RichText::new(u.handle()).size(size).color(c.text));
                }
                if let Some(p) = pronouns {
                    if user.is_some() {
                        ui.label(RichText::new("•").size(size).color(c.faint));
                    }
                    ui.label(RichText::new(p).size(size).color(c.muted));
                }
                if let Some(tag) = tag {
                    ui.add_space(t.spacing.item_y);
                    cosmetics::tag_ui(ui, &self.images, t, &tag);
                }
                if badges {
                    ui.add_space(t.spacing.item_y);
                    self.badges_capsule(ui, t, s.id, s);
                }
            });
        }
        if !card {
            if let Some(l) = s.profile.as_ref().and_then(|p| p.legacy_username.as_deref()) {
                ui.label(RichText::new(pf::legacy(lang, l)).small().color(c.faint));
            }
            ui.add_space(t.spacing.item_y);
            self.badges_ui(ui, t, s.id, s);
        }
        // Sans badge de profil (pas encore reçu, ou compte bot) : ceux que disent les drapeaux.
        if s.profile.as_ref().is_none_or(|p| p.badges.is_empty()) {
            let flags = user.and_then(|u| u.flags).map(Badge::of).unwrap_or_default();
            if !flags.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for b in flags {
                        chip(ui, t, badge_text(b).get(lang), c.bg3, c.text);
                    }
                });
            }
        }
    }

    /// Bio mise en forme, analysée une fois (puis à chaque changement de
    /// texte ou de langue), gardée par egui pour cet utilisateur.
    pub(super) fn bio_ui(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, guild_id: Option<u64>, bio: &str) {
        use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher};
        let key = (guild_id, self.lang, BuildHasherDefault::<DefaultHasher>::default().hash_one(bio));
        let id = Id::unique(("ferrocord.profils.bio", user_id));
        let cached = ui.ctx().data(|d| d.get_temp::<BioCache>(id)).filter(|b| b.key == key);
        let doc = match cached {
            Some(b) => b.doc,
            None => {
                let doc = std::sync::Arc::new(crate::state::profiles::parse_bio(&self.store, guild_id, bio, self.lang, self.h24()));
                ui.ctx().data_mut(|d| d.insert_temp(id, BioCache { key, doc: doc.clone() }));
                doc
            }
        };
        let mut zones = super::markdown::Zones::new();
        super::markdown::doc_ui(ui, t, self.lang, user_id, &doc, &[], false, &mut zones);
        super::markdown::zones_ui(ui, t, user_id, zones);
    }

    /// Bio d'une carte, repliée à quelques lignes comme dans le client ;
    /// « Afficher la bio complète » la déplie (et la replie), sans rien
    /// demander : l'état reste dans egui, par utilisateur.
    pub(super) fn bio_folded(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, guild_id: Option<u64>, bio: &str) {
        let id = Id::unique(("ferrocord.profils.bio-dépliée", user_id));
        let open = ui.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
        let folded = BIO_ROWS * ui.text_style_height(&egui::TextStyle::Body);
        let full = if open {
            self.bio_ui(ui, t, user_id, guild_id, bio);
            f32::INFINITY
        } else {
            let out = egui::ScrollArea::vertical().id_salt(("bio-repliée", user_id)).max_height(folded).scroll_source(egui::scroll_area::ScrollSource::NONE).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| self.bio_ui(ui, t, user_id, guild_id, bio));
            out.content_size.y
        };
        if full > folded + 1.0 {
            let label = if open { pt::BIO_LESS } else { pt::BIO_MORE }.get(self.lang);
            ui.add_space(t.spacing.item_y);
            let resp = ui.add(egui::Label::new(RichText::new(label).small().color(t.colors.text_strong)).sense(Sense::click()));
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            if resp.hovered() {
                ui.painter().hline(resp.rect.x_range(), resp.rect.bottom() - 1.0, egui::Stroke::new(1.0, t.colors.text_strong));
            }
            if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                ui.data_mut(|d| d.insert_temp(id, !open));
            }
        }
    }

    /// Activités de `s` (hors statut personnalisé), chacune dans sa carte
    /// (`activity::activity_card`) ; rend le clic fait dans l'une d'elles.
    pub(super) fn activity_cards(&self, ui: &mut egui::Ui, t: &Tokens, s: &Shown) -> Option<super::activity::CardClick> {
        let p = s.presence.as_ref()?;
        let own = self.store.me.as_ref().is_some_and(|u| u.id == s.id);
        let now = crate::state::store::now_ms();
        let copy_app = super::settings::local::copy_id_shown(ui.ctx(), &self.store);
        let mut click = None;
        for a in p.activities.iter().filter(|a| a.kind != ActivityKind::Custom) {
            ui.add_space(t.spacing.item_x * 1.5);
            // Icône de l'application d'après la liste des jeux déjà chargée (aucune requête) :
            // celle de l'activité, sinon, pour mon jeu détecté, celle du jeu.
            let acts = &self.store.activity;
            let icon = a.rich().and_then(|r| r.app_id).and_then(|id| Some(ImageRef::AppIcon { app_id: id, hash: acts.index.as_ref()?.app(id)?.icon? })).or_else(|| acts.current().filter(|g| own && g.name == a.name).and_then(|g| acts.icon(g)));
            let resp = ui.scope(|ui| super::activity::activity_card(ui, t, self.lang, &self.images, a, icon, now, own, copy_app));
            click = click.or(resp.inner);
            if let Some(ms) = a.start_ms {
                resp.response.on_hover_text(f::activity_since(self.lang, time::clock_mid(self.lang, ms, self.h24())));
            }
        }
        click
    }

    /// Dates du compte : création (tirée de l'identifiant), Nitro, boost.
    pub(super) fn dates_ui(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, s: &Shown) {
        let lang = self.lang;
        let c = &t.colors;
        section(ui, t, t::PROFILE_DISCORD_SINCE.get(lang));
        ui.label(RichText::new(time::long_date(lang, snowflake_ms(user_id))).color(c.text));
        let p = s.profile.as_ref();
        for (title, ms) in [(pt::NITRO_SINCE, p.and_then(|p| p.premium_since)), (pt::GUILD_BOOST_SINCE, p.and_then(|p| p.premium_guild_since))] {
            if let Some(ms) = ms {
                section(ui, t, title.get(lang));
                ui.label(RichText::new(time::long_date(lang, ms)).color(c.text));
            }
        }
    }

    /// Partie « serveur » : arrivée, boost, exclusion temporaire, rôles.
    pub(super) fn member_ui(&self, ui: &mut egui::Ui, t: &Tokens, guild_id: u64, m: &MemberInfo) {
        let lang = self.lang;
        let c = &t.colors;
        if let Some(ms) = m.joined_at {
            section(ui, t, t::PROFILE_SERVER_SINCE.get(lang));
            ui.label(RichText::new(time::long_date(lang, ms)).color(c.text));
        }
        if let Some(ms) = m.premium_since {
            section(ui, t, t::PROFILE_BOOSTING_SINCE.get(lang));
            ui.label(RichText::new(time::long_date(lang, ms)).color(c.text));
        }
        self.roles_ui(ui, t, guild_id, m, true, false);
    }

    /// « Membre depuis » (petit titre, sans capitales) puis la date du
    /// compte ; dans un serveur, les deux dates, chacune avec son icône
    /// (Discord, puis le serveur).
    pub(super) fn since_ui(&self, ui: &mut egui::Ui, t: &Tokens, user_id: u64, guild_id: Option<u64>, s: &Shown) {
        let lang = self.lang;
        let c = &t.colors;
        // Compte système : « Créé le » (il n'est membre de rien).
        let system = s.user.as_ref().is_some_and(|u| u.system);
        super::profile_parts::heading(ui, t, if system { pt::CREATED_ON } else { pt::MEMBER_SINCE }.get(lang));
        ui.add_space(t.spacing.item_y);
        let joined = guild_id.zip(s.member.as_ref().and_then(|m| m.joined_at));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.item_y * 1.5;
            let size = t.typography.button;
            let side = Tokens::icon_side(size);
            if joined.is_some() {
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                crate::theme::icons::paint(crate::theme::icons::Icon::Chat, ui.painter(), rect, c.muted);
            }
            ui.label(RichText::new(time::medium_date(lang, snowflake_ms(user_id))).size(size).color(c.text));
            if let Some((g, ms)) = joined {
                ui.label(RichText::new("•").size(size).color(c.faint));
                let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                let icon = self.store.guilds.iter().find(|x| x.id == g).and_then(|x| x.icon);
                if !super::util::paint_image(ui, &self.images, rect, icon, t.r_row()) {
                    crate::theme::icons::paint(crate::theme::icons::Icon::Members, ui.painter(), rect, c.muted);
                }
                ui.label(RichText::new(time::medium_date(lang, ms)).size(size).color(c.text));
            }
        });
    }

    /// Exclusion temporaire en cours et rôles du membre ; `caps` : titre en
    /// capitales (profil complet), sinon petit titre de la carte ; `add` : une
    /// pastille « + » à la suite (lot FF2, F24), dont le clic est rendu
    /// (`true`) pour ouvrir la fenêtre des rôles.
    fn roles_ui(&self, ui: &mut egui::Ui, t: &Tokens, guild_id: u64, m: &MemberInfo, caps: bool, add: bool) -> bool {
        let lang = self.lang;
        let c = &t.colors;
        if let Some(until) = m.timeout_until.filter(|u| *u > self.store.now()) {
            ui.label(RichText::new(f::timed_out_until(lang, time::until(lang, until, self.h24()))).color(c.danger));
        }
        let Some(ids) = &m.roles else { return false };
        let guild = self.store.guilds.iter().find(|g| g.id == guild_id);
        // @everyone (identifiant du serveur) n'est pas montré ; rôles inconnus ignorés.
        let mut roles: Vec<&Role> = guild.map(|g| g.roles.iter().filter(|r| r.id != guild_id && ids.contains(&r.id)).collect()).unwrap_or_default();
        roles.sort_by_key(|r| std::cmp::Reverse((r.position, r.id)));
        if caps {
            section(ui, t, t::PROFILE_ROLES.get(lang));
        } else {
            super::profile_parts::heading(ui, t, pt::ROLES.get(lang));
        }
        if roles.is_empty() && !add {
            ui.label(RichText::new(t::PROFILE_NO_ROLES.get(lang)).color(c.muted));
            return false;
        }
        let mut plus = false;
        ui.horizontal_wrapped(|ui| {
            // Mode développeur : clic droit sur un rôle, son identifiant (lot U).
            let copy_id = super::settings::local::copy_id_shown(ui.ctx(), &self.store);
            for r in roles {
                let chip = role_chip(ui, t, r);
                if copy_id {
                    chip.interact(Sense::click()).context_menu(|ui| {
                        if super::menu::item(ui, t, crate::i18n::table::people::t::COPY_ROLE_ID.get(lang), Some(crate::theme::icons::Icon::Id), false).clicked() {
                            ui.ctx().copy_text(r.id.to_string());
                        }
                    });
                }
            }
            if add {
                let hint = crate::i18n::table::ff::t::CARD_ADD_ROLE.get(lang);
                let chip = egui::Frame::new().fill(c.bg3).corner_radius(t.r_row()).inner_margin(t.profile_chip_margin()).show(ui, |ui| ui.label(RichText::new("+").small().strong().color(c.text_strong))).response;
                let chip = chip.interact(Sense::click()).on_hover_text(hint).on_hover_cursor(egui::CursorIcon::PointingHand);
                chip.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, hint));
                plus = chip.clicked();
            }
        });
        plus
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Message,
    /// Texte tapé dans le champ de la carte : envoyé dans la conversation privée.
    Send(String),
    CopyId,
    Full(Tab),
    /// Expulser, bannir, exclure (lot A4) : la fenêtre du clic droit.
    Moderate(u64, super::moderation::ModKind),
    /// Rôles et pseudo du membre (lot A3).
    Roles(u64),
    /// Bouton d'une activité (lot C3).
    Button(super::activity::ButtonClick),
    /// « … » de ma propre activité : la page Activités des Paramètres.
    ActivitySettings,
    /// « Modifier le profil », sur ma propre carte.
    Edit,
    /// Bouton rond de la bannière : amitié, ou « … ».
    Head(super::profile_parts::HeadPick),
}

/// Bio déjà mise en forme : (serveur, langue, empreinte du texte).
#[derive(Clone)]
struct BioCache {
    key: (Option<u64>, Lang, u64),
    doc: std::sync::Arc<crate::state::markdown::Doc>,
}

/// Réserve jusqu'à max_height sous le curseur : une fenêtre (Area) borne
/// son contenu à sa taille de l'image précédente, si bien qu'une zone
/// défilante n'y grandirait jamais quand le profil ou la note arrivent.
pub(super) fn room<R>(ui: &mut egui::Ui, max_height: f32, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    let rect = egui::Rect::from_min_size(ui.cursor().min, egui::vec2(ui.available_width(), max_height));
    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), add).inner
}

pub(super) fn close(ctx: &egui::Context) {
    ctx.data_mut(|d| d.remove::<Card>(card_id()));
}
/// Pseudo de serveur, sinon surnom d'ami, sinon nom global, sinon nom d'utilisateur, sinon le nom vu au clic.
fn display_name(card: &Card, s: &Shown) -> String {
    name_of(&card.name, s)
}

/// Comme pour la carte, avec `seen` : le nom vu là où l'on a cliqué.
pub(super) fn name_of(seen: &str, s: &Shown) -> String {
    if let Some(n) = s.member.as_ref().and_then(|m| m.nick.clone()).or_else(|| s.friend_nick.clone()) {
        return n;
    }
    s.user.as_ref().map(|u| u.display_name().to_string()).unwrap_or_else(|| seen.to_string())
}

/// Image de remplacement d'un en-tête (aperçu de l'édition du profil) : la
/// texture de l'image choisie et la zone gardée par le recadrage.
#[derive(Clone, Copy)]
pub(super) struct Swap {
    pub(super) tex: egui::TextureId,
    pub(super) uv: egui::Rect,
}

impl Swap {
    pub(super) fn shape(self, rect: egui::Rect, r: egui::CornerRadius) -> egui::Shape {
        egui::Shape::Rect(egui::epaint::RectShape::filled(rect, r, Color32::WHITE).with_texture(self.tex, self.uv))
    }
}

/// Où l'en-tête d'un profil est dessiné.
#[derive(Clone, Copy, Default)]
pub(super) struct Header<'a> {
    /// Profil complet : bannière plus large.
    pub(super) full: bool,
    /// Couleurs de thème du profil, s'il en a.
    pub(super) themed: Option<&'a cosmetics::Themed>,
    /// Haut de la fenêtre (origine du dégradé), s'il n'est pas celui de l'en-tête.
    pub(super) window_top: Option<f32>,
    /// Bannière de remplacement.
    pub(super) banner: Option<Swap>,
    /// Avatar de remplacement.
    pub(super) avatar: Option<Swap>,
    /// Statut personnalisé en bulle de pensée, à droite de l'avatar.
    pub(super) bubble: bool,
    /// Panneau de profil d'un MP : bande haute même sans bannière, avatar remonté.
    pub(super) panel: bool,
}

/// Couleurs de thème du profil affiché (celles du serveur d'abord), s'il en a.
pub(super) fn profile_theme(t: &Tokens, s: &Shown) -> Option<cosmetics::Themed> {
    s.profile.as_ref().and_then(Profile::theme).map(|colors| cosmetics::Themed::new(t, colors))
}

/// Fond en dégradé d'un profil à thème, mis à la place `bg` réservée avant
/// le contenu de la fenêtre `rect` ; son contour `edge` est repeint par-dessus.
pub(super) fn themed_background(p: &egui::Painter, th: &cosmetics::Themed, bg: egui::layers::ShapeIdx, rect: egui::Rect, r: egui::CornerRadius, edge: Option<egui::Stroke>) {
    let mut shapes = vec![th.background(rect, r)];
    if let Some(stroke) = edge {
        shapes.push(egui::Shape::Rect(egui::epaint::RectShape::stroke(rect, r, stroke, egui::StrokeKind::Inside)));
    }
    p.set(bg, egui::Shape::Vec(shapes));
}

/// Lignes de bio montrées tant qu'elle est repliée.
const BIO_ROWS: f32 = 4.0;

/// Longueur d'un message tapé sur la carte (limite d'un message sans Nitro).
const CARD_MESSAGE_MAX: usize = 2000;

pub(super) fn section(ui: &mut egui::Ui, t: &Tokens, title: &str) {
    ui.label(RichText::new(title.to_uppercase()).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
}

pub(super) fn chip(ui: &mut egui::Ui, t: &Tokens, text: &str, fill: Color32, color: Color32) {
    let _ = chip_response(ui, t, text, fill, color);
}

fn chip_response(ui: &mut egui::Ui, t: &Tokens, text: &str, fill: Color32, color: Color32) -> egui::Response {
    egui::Frame::new().fill(fill).corner_radius(t.r_row()).inner_margin(t.profile_chip_margin()).show(ui, |ui| {
        ui.label(RichText::new(text).small().color(color));
    })
    .response
}

fn role_chip(ui: &mut egui::Ui, t: &Tokens, r: &Role) -> egui::Response {
    let c = &t.colors;
    let dot = if r.color == 0 { c.faint } else { Color32::from_rgb((r.color >> 16) as u8, (r.color >> 8) as u8, r.color as u8) };
    egui::Frame::new().fill(c.bg3).corner_radius(t.r_row()).inner_margin(t.profile_chip_margin()).show(ui, |ui| {
        ui.horizontal(|ui| {
            let side = t.shapes.mention_dot * 2.0;
            let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
            ui.painter().circle_filled(rect.center(), t.shapes.mention_dot, dot);
            ui.label(RichText::new(&r.name).small().color(c.text));
        });
    })
    .response
}

/// Activité du correspondant d'un MP à deux (infobulle de la liste des MP,
/// lot C3) : première activité, puis le statut personnalisé.
pub(super) fn dm_activity(ctx: &egui::Context, channel_id: u64, lang: Lang) -> Option<String> {
    let hub = hub(ctx)?;
    let mut people = hub.lock();
    let peer = people.dm_peer(channel_id)?;
    let p = people.presence(peer)?;
    // Comme dans le client : l'activité d'abord, puis le statut personnalisé après un point médian.
    let doing = p.activities.iter().find(|a| a.kind != ActivityKind::Custom && !a.name.is_empty()).map(|a| activity_line(lang, a)).filter(|l| !l.is_empty());
    let custom = p.custom().map(|a| activity_line(lang, a)).filter(|l| !l.is_empty());
    match (doing, custom) {
        (Some(d), Some(c)) => Some(format!("{d} • {c}")),
        (d, c) => d.or(c),
    }
}

/// Statut personnalisé : son emoji en image (personnalisé ou Unicode connu),
/// puis son texte (emojis compris). Un emoji Unicode inconnu est omis.
pub(super) fn custom_status_ui(ui: &mut egui::Ui, t: &Tokens, a: &Activity) {
    let img = a.emoji.as_ref().and_then(|e| match (e.id, e.name.as_deref()) {
        (Some(id), _) => Some(super::emoji::Img::custom(id, e.animated)),
        (None, Some(n)) => crate::state::emoji::find(n).map(super::emoji::Img::Unicode),
        _ => None,
    });
    let text = a.state.as_deref().unwrap_or_default();
    if img.is_none() && text.is_empty() {
        return;
    }
    ui.horizontal_wrapped(|ui| {
        let font = egui::TextStyle::Body.resolve(ui.style());
        if let Some(img) = img {
            let side = super::emoji::row_height(ui, &font);
            super::emoji::icon(ui, img, side, egui::Sense::hover());
        }
        if !text.is_empty() {
            super::emoji::label(ui, text, &font, t.colors.text, false);
        }
    });
}

/// Hauteur du pied « Voir le profil complet » (bouton et marges).
pub(super) fn full_button_height(t: &Tokens) -> f32 {
    full_button_side(t) + t.profile_pad() + t.spacing.item_y
}

/// Hauteur du bouton « Voir le profil complet » : 40 pour des contrôles de 32, comme dans le client.
fn full_button_side(t: &Tokens) -> f32 {
    (t.spacing.control_height * 1.25).round()
}

/// « Voir le profil complet » : bouton sur toute la largeur `width` de la
/// carte, à ses marges ; vrai s'il est cliqué.
pub(super) fn full_button(ui: &mut egui::Ui, t: &Tokens, lang: Lang, width: f32) -> bool {
    let pad = t.profile_pad();
    let m = t.profile_margin();
    egui::Frame::new()
        .inner_margin(egui::Margin { top: t.spacing.item_y.round() as i8, ..m })
        .show(ui, |ui| super::activity::bordered_button(ui, t, pt::FULL_PROFILE.get(lang), width - 2.0 * pad, full_button_side(t)).clicked())
        .inner
}

/// Activités (hors statut personnalisé), chacune dans sa carte ; rend le
/// bouton d'activité cliqué (lot C3). `caps` : titre en capitales (profil
/// complet), sinon petit titre de la carte.
pub(super) fn activities_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, p: &Presence, h24: bool, caps: bool) -> Option<super::activity::ButtonClick> {
    let others: Vec<&Activity> = p.activities.iter().filter(|a| a.kind != ActivityKind::Custom).collect();
    if others.is_empty() {
        return None;
    }
    if caps {
        section(ui, t, t::PROFILE_ACTIVITY.get(lang));
    } else {
        ui.add_space(t.spacing.item_x);
        super::profile_parts::heading(ui, t, t::PROFILE_ACTIVITY.get(lang));
    }
    let now = crate::state::store::now_ms();
    let mut click = None;
    for (i, a) in others.into_iter().enumerate() {
        if i > 0 {
            ui.add_space(t.spacing.item_y);
        }
        // Images, durée, écoute, boutons : `ui::activity` (lot C3).
        let resp = ui.scope(|ui| super::activity::activity_ui(ui, t, lang, images, a, now));
        click = click.or(resp.inner);
        if let Some(ms) = a.start_ms {
            resp.response.on_hover_text(f::activity_since(lang, time::clock_mid(lang, ms, h24)));
        }
    }
    click
}

/// Statut, plateformes et activités (onglet « Activité » du profil complet) ;
/// rend le bouton d'activité cliqué (lot C3).
pub(super) fn presence_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, p: &Presence, h24: bool) -> Option<super::activity::ButtonClick> {
    let c = &t.colors;
    if let Some(a) = p.custom() {
        custom_status_ui(ui, t, a);
    }
    ui.horizontal_wrapped(|ui| {
        ui.label(RichText::new(status_text(p.status).get(lang)).color(t.status_color(p.status)));
        for line in platforms(lang, &p.client) {
            ui.label(RichText::new(line).small().color(c.muted));
        }
    });
    activities_ui(ui, t, lang, images, p, h24, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::Event;
    use crate::state::model::{Guild, User};
    use crate::state::Permissions;
    use crate::state::people::{ActivityEmoji, Status};
    use crate::state::profiles::ProfileCmd;
    use crate::state::{Cmd, Store};
    use chrono::TimeZone;

    const ME: u64 = 1;
    const GUILD: u64 = 9;
    /// Compte créé le 1er mars 2024 à 12:00 UTC.
    const ALICE: u64 = (1_709_294_400_000 - crate::state::model::DISCORD_EPOCH) << 22;

    struct Offline;

    impl super::super::Backend for Offline {
        fn set_wake(&mut self, _: super::super::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<Cmd>) {}
    }

    fn painted(shapes: &[egui::epaint::ClippedShape]) -> String {
        fn walk(s: &egui::Shape, out: &mut String) {
            match s {
                egui::Shape::Text(t) => {
                    out.push_str(&t.galley.job.text);
                    out.push('\n');
                }
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                _ => {}
            }
        }
        let mut out = String::new();
        shapes.iter().for_each(|c| walk(&c.shape, &mut out));
        out
    }

    fn app() -> App {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        let mut guild = Guild::new(GUILD, "G".into(), Vec::new());
        guild.roles = vec![
            Role { id: GUILD, name: "@everyone".into(), color: 0, position: 0, permissions: Permissions(0), mentionable: false },
            Role { id: 20, name: "Modo".into(), color: 0xe5484d, position: 2, permissions: Permissions(0), mentionable: false },
            Role { id: 21, name: "Membre".into(), color: 0, position: 1, permissions: Permissions(0), mentionable: false },
        ];
        store.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild], dms: Vec::new() });
        store.take_commands();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), store, images));
        app.theme = Tokens::default();
        app.lang = Lang::Fr;
        app
    }

    fn frame(app: &mut App, events: Vec<egui::Event>) -> String {
        let ctx = app.ctx.clone();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1600.0))), events, ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            install(ui.ctx(), &app.store.people);
            app.profile_ui(ui);
            app.store.people.lock().end_frame();
        });
        out.textures_delta.clear();
        painted(&out.shapes)
    }

    /// Clic : appui à une image, relâchement à la suivante.
    fn click(app: &mut App, pos: egui::Pos2) {
        let button = |pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
        frame(app, vec![egui::Event::PointerMoved(pos)]);
        frame(app, vec![button(true)]);
        frame(app, vec![button(false)]);
    }

    fn fetch(guild_id: Option<u64>) -> Cmd {
        Cmd::Profile(ProfileCmd::Fetch { user_id: ALICE, guild_id })
    }

    fn alice() -> UserInfo {
        UserInfo {
            id: ALICE,
            username: "alice_fictive".into(),
            global_name: Some("Alice".into()),
            discriminator: None,
            avatar: Avatar::default_for(ALICE),
            banner: None,
            accent: Some(Some(0x5b8def)),
            flags: Some(1 << 22),
            bot: false,
            system: false,
            look: Default::default(),
        }
    }

    #[test]
    fn long_dates() {
        let ms = 1_709_294_400_000;
        assert_eq!(time::long_date_in(Lang::Fr, &chrono::Utc, ms), "1er mars 2024");
        assert_eq!(time::long_date_in(Lang::En, &chrono::Utc, ms), "March 1, 2024");
        assert_eq!(time::long_date_in(Lang::Fr, &chrono::Utc, ms + 14 * 86_400_000), "15 mars 2024");
        assert_eq!(time::long_date_in(Lang::Fr, &chrono::FixedOffset::east_opt(0).unwrap_or_else(|| panic!("fuseau")), i64::MAX), "");
        assert_eq!(chrono::Utc.timestamp_millis_opt(snowflake_ms(ALICE)).single().map(|d| d.timestamp_millis()), Some(ms));
    }

    #[test]
    fn activity_lines() {
        let a = |kind, name: &str| Activity { kind, name: name.into(), details: None, state: None, emoji: None, start_ms: None, rich: None };
        assert_eq!(activity_line(Lang::Fr, &a(ActivityKind::Playing, "Jeu")), "Joue à Jeu");
        assert_eq!(activity_line(Lang::En, &a(ActivityKind::Listening, "Radio")), "Listening to Radio");
        assert_eq!(activity_line(Lang::En, &a(ActivityKind::Competing, "Coupe")), "Competing in Coupe");
        let mut live = a(ActivityKind::Streaming, "Plateforme");
        live.details = Some("Titre".into());
        assert_eq!(activity_line(Lang::Fr, &live), "En direct\u{a0}: Titre");
        let mut custom = a(ActivityKind::Custom, "Custom Status");
        custom.state = Some("Occupé".into());
        custom.emoji = Some(ActivityEmoji { name: Some("pain".into()), id: Some(7), animated: false });
        assert_eq!(activity_line(Lang::Fr, &custom), ":pain: Occupé");
        custom.emoji = None;
        assert_eq!(activity_line(Lang::Fr, &custom), "Occupé");
    }

    /// Carte d'un membre connu : nom, nom d'utilisateur, badge, statut et
    /// activité, date de création du compte (tirée de l'identifiant), date
    /// d'arrivée, rôles triés (sans @everyone) ; compléments demandés une fois.
    #[test]
    fn card_shows_known_fields_only() {
        let mut app = app();
        super::super::settings::local::developer_mode_on(&mut app.store);
        {
            let mut p = app.store.people.lock();
            p.upsert_user(alice());
            p.upsert_member(GUILD, ALICE, MemberInfo { nick: None, roles: Some(vec![21, 20, 999]), joined_at: Some(1_717_243_200_000), ..Default::default() });
            p.set_presence(
                ALICE,
                Presence {
                    status: Status::Idle,
                    client: ClientStatus { mobile: Some(Status::Idle), ..Default::default() },
                    activities: vec![Activity { kind: ActivityKind::Playing, name: "Jeu Fictif".into(), details: None, state: None, emoji: None, start_ms: None, rich: None }],
                },
            );
        }
        let anchor = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0));
        open(&app.ctx, ALICE, Some(GUILD), anchor, "Ali", Avatar::default_for(ALICE));
        // Première image : passe de mesure d'egui, rien n'est peint ; la
        // deuxième agrandit la zone défilante à la taille du contenu.
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        let text = frame(&mut app, Vec::new());
        for want in ["Alice\n", "alice_fictive", "Développeur actif", "Joue à", "Jeu Fictif", "Membre depuis", "1 mars 2024", "1 juin 2024", "Modo\n", "Membre\n", "Envoyer un message", "Copier l'identifiant", &ALICE.to_string()] {
            assert!(text.contains(want), "« {want} » manque :\n{text}");
        }
        assert!(text.find("Modo").unwrap_or(usize::MAX) < text.find("Membre\n").unwrap_or(0), "rôles par position décroissante");
        for absent in ["@everyone", "Bot", "BOOSTE", "Exclusion", "Inactif", "MEMBRE DEPUIS", "NOTE"] {
            assert!(!text.contains(absent), "« {absent} » ne devrait pas être affiché :\n{text}");
        }
        assert_eq!(app.store.take_commands(), vec![fetch(Some(GUILD)), Cmd::Profile(ProfileCmd::Note { user_id: ALICE })]);
        // Image suivante : la carte est toujours là, rien n'est redemandé, et
        // l'utilisateur compte comme affiché (ses changements réveillent l'interface).
        assert!(frame(&mut app, Vec::new()).contains("Alice"));
        assert!(app.store.take_commands().is_empty());
        assert!(app.store.people.lock().is_watched(ALICE));
        // Échap la ferme.
        let esc = egui::Event::Key { key: egui::Key::Escape, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE };
        frame(&mut app, vec![esc]);
        assert!(!frame(&mut app, Vec::new()).contains("Alice"));
    }

    /// Utilisateur inconnu des tables : seulement ce qui est sûr (nom vu au
    /// clic, date tirée de l'identifiant), ni statut ni nom d'utilisateur.
    #[test]
    fn unknown_user_shows_no_invented_data() {
        let mut app = app();
        let anchor = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0));
        open(&app.ctx, ALICE, None, anchor, "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        let text = frame(&mut app, Vec::new());
        assert!(text.contains("Ali\n") && text.contains("1 mars 2024"), "{text}");
        for absent in ["En ligne", "Hors ligne", "Inactif", "RÔLES", "MEMBRE DU SERVEUR", "alice_fictive"] {
            assert!(!text.contains(absent), "« {absent} » :\n{text}");
        }
        assert_eq!(app.store.take_commands(), vec![fetch(None), Cmd::Profile(ProfileCmd::Note { user_id: ALICE })]);
    }

    /// Un clic en dehors ferme la carte ; « Envoyer un message » demande le MP.
    #[test]
    fn outside_click_closes_and_message_button_opens_dm() {
        let mut app = app();
        app.store.people.lock().upsert_user(alice());
        let anchor = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0));
        open(&app.ctx, ALICE, None, anchor, "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        click(&mut app, egui::pos2(900.0, 700.0));
        assert!(!frame(&mut app, Vec::new()).contains("Alice"), "fermée par un clic dehors");

        open(&app.ctx, ALICE, None, anchor, "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        app.store.take_commands();
        // Bouton « Envoyer un message » : trouvé par son texte peint.
        let ctx = app.ctx.clone();
        let mut button = None;
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1600.0))), ..Default::default() };
        let out = ctx.run_ui(input, |ui| {
            install(ui.ctx(), &app.store.people);
            app.profile_ui(ui);
        });
        for s in &out.shapes {
            if let egui::Shape::Text(t) = &s.shape
                && t.galley.job.text.starts_with("Envoyer un message")
            {
                button = Some(t.pos + t.galley.rect.center().to_vec2());
            }
        }
        let at = button.unwrap_or_else(|| panic!("bouton attendu"));
        let enter = || vec![egui::Event::Key { key: egui::Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers::NONE }];
        // Champ réel (lot FF2, F24) : Entrée sans texte ouvre seulement la conversation.
        click(&mut app, at);
        frame(&mut app, enter());
        assert_eq!(app.store.take_commands(), vec![Cmd::OpenDm { user_id: ALICE }]);
        assert!(!frame(&mut app, Vec::new()).contains("Alice"), "fermée après l'action");
        assert!(!app.card_message_pending());
        // Avec un texte : la conversation est demandée, le message attend qu'elle existe.
        open(&app.ctx, ALICE, None, anchor, "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        click(&mut app, at);
        frame(&mut app, vec![egui::Event::Text("Salut".into())]);
        frame(&mut app, enter());
        assert_eq!(app.store.take_commands(), vec![Cmd::OpenDm { user_id: ALICE }]);
        assert!(app.card_message_pending());
    }

    /// Statut personnalisé : emoji Unicode connu ou personnalisé en image,
    /// inconnu omis ; le texte suit.
    #[test]
    fn custom_status_emoji_as_image() {
        let ctx = egui::Context::default();
        let t = Tokens::default();
        let mut a = Activity { kind: ActivityKind::Custom, name: "Custom Status".into(), details: None, state: Some("Code".into()), emoji: None, start_ms: None, rich: None };
        let mut got = Vec::new();
        for (name, id) in [(Some("\u{1f980}"), None), (Some("\u{2605}"), None), (Some("blob"), Some(42))] {
            a.emoji = Some(ActivityEmoji { name: name.map(Into::into), id, animated: false });
            ctx.run_ui(egui::RawInput::default(), |ui| custom_status_ui(ui, &t, &a)).textures_delta.clear();
            got.push(super::super::emoji::take_queued(&ctx));
        }
        let crab = crate::state::emoji::by_name("crab").map(super::super::emoji::Img::Unicode);
        assert_eq!(got[0].first().copied(), crab);
        assert!(got[1].is_empty());
        assert_eq!(got[2], [super::super::emoji::Img::Custom(42)]);
    }

    /// Pastille : rien quand le statut est inconnu, une forme par statut sinon.
    #[test]
    fn status_dot_only_for_known_status() {
        let t = Tokens::default();
        let hub = PeopleHub::default();
        hub.lock().set_presence(5, Presence { status: Status::Dnd, client: ClientStatus::default(), activities: Vec::new() });
        let ctx = egui::Context::default();
        let count = |user: u64| {
            let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
                install(ui.ctx(), &hub);
                status_dot(ui, &t, egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0)), user, t.colors.bg2);
            });
            out.textures_delta.clear();
            out.shapes.iter().filter(|s| !matches!(s.shape, egui::Shape::Noop)).count()
        };
        assert_eq!(count(6), 0);
        assert!(count(5) >= 3, "anneau, pastille, barre");
        // Lus pendant l'image : affichés.
        hub.lock().end_frame();
        assert!(hub.lock().is_watched(5) && hub.lock().is_watched(6));
    }

    /// Profil complet d'Alice (jeu d'essai retouché) versé dans les tables, avec sa note.
    fn with_full_profile(app: &mut App) {
        let path = format!("{}/tests/fixtures/rest_user_profile.json", env!("CARGO_MANIFEST_DIR"));
        let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"))).unwrap_or_else(|e| panic!("{e}"));
        v["user"]["id"] = ALICE.to_string().into();
        v["guild_member"]["user"]["id"] = ALICE.to_string().into();
        v["mutual_guilds"] = serde_json::json!([{ "id": GUILD.to_string(), "nick": "Ali" }]);
        assert!(crate::net::decode_profile(&app.store.people, ALICE, Some(GUILD), &v.to_string()));
        crate::net::decode_note(&app.store.people, ALICE, r#"{ "note": "Rencontrée au club." }"#);
    }

    /// Position (centre) d'un texte peint à l'image en cours.
    fn find(app: &mut App, want: &str) -> Option<egui::Pos2> {
        let ctx = app.ctx.clone();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 1600.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            install(ui.ctx(), &app.store.people);
            app.profile_ui(ui);
        });
        out.textures_delta.clear();
        let mut at = None;
        fn walk(s: &egui::Shape, want: &str, at: &mut Option<egui::Pos2>) {
            match s {
                // Les glyphes eux-mêmes : dans une ligne à retour, le texte peut
                // commencer après le début de son rectangle.
                egui::Shape::Text(t) if t.galley.job.text == want => *at = Some(t.pos + t.galley.mesh_bounds.center().to_vec2()),
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, want, at)),
                _ => {}
            }
        }
        out.shapes.iter().for_each(|c| walk(&c.shape, want, &mut at));
        at
    }

    /// Carte avec le profil complet : pronoms, ancien nom, badges du profil
    /// (description, faute d'icône chargée), bio du serveur mise en forme,
    /// Nitro, liens vers les serveurs et amis en commun, note.
    #[test]
    fn card_shows_full_profile() {
        let mut app = app();
        app.store.people.lock().upsert_user(alice());
        with_full_profile(&mut app);
        let anchor = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0));
        open(&app.ctx, ALICE, Some(GUILD), anchor, "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        let text = frame(&mut app, Vec::new());
        for want in ["elle", "Bio de serveur fictive.", "1 serveur en commun", "2 amis en commun", "Voir le profil complet"] {
            assert!(text.contains(want), "« {want} » manque :\n{text}");
        }
        // Badges en capsule d'icônes (description dans l'infobulle) ; pas d'ancien nom, comme dans le client.
        assert!(app.has_badges(&app.shown(ALICE, Some(GUILD))));
        assert!(!text.contains("Anciennement"), "{text}");
        // Déjà dans les tables : rien n'est demandé.
        assert!(app.store.take_commands().is_empty());
        // « 2 amis en commun » ouvre le profil complet sur cet onglet.
        let at = find(&mut app, "2 amis en commun").unwrap_or_else(|| panic!("lien attendu"));
        click(&mut app, at);
        // Nouvelle fenêtre : sa première image est une passe de mesure.
        frame(&mut app, Vec::new());
        let text = frame(&mut app, Vec::new());
        for want in ["Amis en commun (2)", "Serveurs en commun (1)", "bob_fictif", "eve_fictive"] {
            assert!(text.contains(want), "« {want} » manque :\n{text}");
        }
        // Onglet « À propos » : comptes liés (lien public), vérifiés.
        let at = find(&mut app, "À propos").unwrap_or_else(|| panic!("onglet"));
        click(&mut app, at);
        let text = frame(&mut app, Vec::new());
        for want in ["COMPTES LIÉS", "GitHub", "alice-fictive", "Steam", "Battle.net", "Vérifié", "MEMBRE DU SERVEUR DEPUIS", "NITRO DEPUIS", "1er février 2024", "NOTE", "Rencontrée au club."] {
            assert!(text.contains(want), "« {want} » manque :\n{text}");
        }
        // Onglet des serveurs : le serveur connu, avec le pseudo.
        let at = find(&mut app, "Serveurs en commun (1)").unwrap_or_else(|| panic!("onglet"));
        click(&mut app, at);
        let text = frame(&mut app, Vec::new());
        assert!(text.contains("G  Ali"), "{text}");
    }

    /// Note modifiée dans le profil complet, puis fenêtre fermée : enregistrée une fois.
    #[test]
    fn edited_note_is_saved_when_leaving() {
        let mut app = app();
        with_full_profile(&mut app);
        super::super::profile_full::open_note(&app.ctx, ALICE, Some(GUILD), "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        let at = find(&mut app, "Rencontrée au club.").unwrap_or_else(|| panic!("champ de la note"));
        click(&mut app, at);
        frame(&mut app, vec![egui::Event::Text(" Fan de Rust.".into())]);
        assert!(app.close_full_profile(), "profil complet ouvert");
        click(&mut app, egui::pos2(900.0, 700.0));
        let cmds = app.store.take_commands();
        assert_eq!(cmds.len(), 1, "{cmds:?}");
        assert!(matches!(&cmds[0], Cmd::Profile(ProfileCmd::SetNote { user_id: ALICE, note }) if note.contains("Fan de Rust.")), "{cmds:?}");
    }

    /// Carte de profil d'un membre : les actions de modération du clic
    /// droit, seulement si j'en ai le droit ; un clic ouvre leur fenêtre.
    #[test]
    fn card_offers_moderation_when_allowed() {
        let mut app = app();
        app.store.people.lock().upsert_user(alice());
        let anchor = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0));
        open(&app.ctx, ALICE, Some(GUILD), anchor, "Ali", Avatar::default_for(ALICE));
        frame(&mut app, Vec::new());
        let text = frame(&mut app, Vec::new());
        assert!(!text.contains("Expulser"), "sans droit :\n{text}");
        if let Some(g) = app.store.guilds.iter_mut().find(|g| g.id == GUILD) {
            g.owner_id = Some(ME);
        }
        let text = frame(&mut app, Vec::new());
        assert!(text.contains("Expulser") && text.contains("Bannir"), "propriétaire :\n{text}");
        let at = find(&mut app, "Expulser").unwrap_or_else(|| panic!("Expulser"));
        click(&mut app, at);
        assert!(app.moderation.dialog_open(), "fenêtre d'expulsion");
        assert!(app.store.take_commands().iter().all(|c| !matches!(c, Cmd::Moderation(_))), "rien d'envoyé avant de confirmer");
    }

    /// Clic droit : menu ; « Bloquer » demande « Oui » avant d'écrire.
    #[test]
    fn user_menu_blocks_after_confirmation() {
        let mut app = app();
        app.store.people.lock().upsert_user(alice());
        super::super::profile_full::open_menu(&app.ctx, ALICE, "Ali", egui::pos2(100.0, 100.0));
        frame(&mut app, Vec::new());
        let text = frame(&mut app, Vec::new());
        assert!(!text.contains("Copier l'identifiant"), "sans le mode développeur :
{text}");
        super::super::settings::local::developer_mode_on(&mut app.store);
        let text = frame(&mut app, Vec::new());
        for want in ["Profil", "Message", "Ajouter une note", "Ajouter en ami", "Ignorer", "Bloquer", "Copier l'identifiant"] {
            assert!(text.contains(want), "« {want} » manque :\n{text}");
        }
        assert!(!text.contains("Mentionner"), "aucun salon ouvert :\n{text}");
        let at = find(&mut app, "Bloquer").unwrap_or_else(|| panic!("Bloquer"));
        click(&mut app, at);
        assert!(app.store.take_commands().is_empty(), "pas encore confirmé");
        let text = frame(&mut app, Vec::new());
        assert!(text.contains("Bloquer Ali\u{a0}?"), "{text}");
        let at = find(&mut app, "Oui").unwrap_or_else(|| panic!("Oui"));
        click(&mut app, at);
        assert_eq!(app.store.take_commands(), vec![Cmd::Relations(crate::state::relations::RelationsCmd::Block { user_id: ALICE })]);
        // « Profil » ouvre la carte.
        super::super::profile_full::open_menu(&app.ctx, ALICE, "Ali", egui::pos2(100.0, 100.0));
        frame(&mut app, Vec::new());
        let at = find(&mut app, "Profil").unwrap_or_else(|| panic!("Profil"));
        click(&mut app, at);
        frame(&mut app, Vec::new());
        assert!(frame(&mut app, Vec::new()).contains("Membre depuis"));
    }

    /// Hauteur de la carte : celle de son contenu (un profil plus fourni la
    /// grandit), bornée à la fenêtre (le corps défile, le pied reste visible).
    #[test]
    fn card_height_follows_its_content_within_the_window() {
        let height = |app: &mut App, screen: f32| {
            let ctx = app.ctx.clone();
            let mut text = String::new();
            for _ in 0..4 {
                let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, screen))), ..Default::default() };
                let mut out = ctx.run_ui(input, |ui| {
                    install(ui.ctx(), &app.store.people);
                    app.profile_ui(ui);
                });
                out.textures_delta.clear();
                text = painted(&out.shapes);
            }
            let rect = ctx.memory(|m| m.area_rect(card_id())).unwrap_or_else(|| panic!("carte ouverte"));
            (rect, text)
        };
        let anchor = egui::Rect::from_min_size(egui::pos2(10.0, 10.0), egui::vec2(36.0, 36.0));
        let mut app = app();
        app.store.people.lock().upsert_user(alice());
        open(&app.ctx, ALICE, None, anchor, "Ali", Avatar::default_for(ALICE));
        let (bare, _) = height(&mut app, 1600.0);
        with_full_profile(&mut app);
        let (full, _) = height(&mut app, 1600.0);
        assert!(full.height() > bare.height() + 20.0, "profil fourni : {} contre {}", full.height(), bare.height());
        assert!(full.height() < 1000.0, "pas de hauteur fixe : {}", full.height());
        // Fenêtre basse : la carte y tient, « Voir le profil complet » reste peint.
        let (small, text) = height(&mut app, 300.0);
        assert!(small.height() <= 300.0, "{}", small.height());
        assert!(small.height() < full.height());
        assert!(text.contains("Voir le profil complet"), "{text}");
    }
}
