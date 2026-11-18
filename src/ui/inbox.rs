//! Boîte de réception (bouton en haut à droite, Ctrl+I), comme celle de
//! Discord : un panneau ancré sous le bouton, des onglets soulignés et des
//! cartes.
//!
//! - « Messages non lus » : une carte par salon non lu (icône du serveur,
//!   # salon, « serveur › catégorie », cloche, marquer comme lu, replier),
//!   avec ses messages non lus déjà en mémoire. Un salon dont l'historique
//!   n'est pas chargé montre « Nouveaux messages » : rien n'est demandé à
//!   l'ouverture de la boîte, seulement au clic sur « Afficher les
//!   messages » (requête d'historique ordinaire, par le limiteur). La carte
//!   « Pour vous » (demandes d'ami, événements proches, mots-clés
//!   surveillés) vient en tête quand elle a quelque chose à montrer.
//! - « Mentions » : mentions récentes de Discord (`state::inbox`), demandées
//!   à l'ouverture de l'onglet ; filtres dans un menu.
//! - « Messages marqués » et « Rappels » : messages enregistrés (`state::saved`).
//! - « Programmés » : messages programmés du plugin, s'il est activé.
//!
//! Les messages sont dessinés par le rendu de la discussion (`inbox_cards`).
//! Les listes sont virtualisées : seules les cartes visibles sont mises en page.

use super::inbox_cards::{self as cards, CardList, Docs, Head, HeadIcon, PAD};
use super::keys::{focus_ring, with_chord};
use super::util::icon_button;
use super::widgets::{self, Kind};
use super::{words, App};
use crate::i18n::table::nav::{f as nf, t as nt};
use crate::i18n::table::social::t as st;
use crate::i18n::{time, Lang};
use crate::images::Images;
use crate::state::keys::Action;
use crate::state::model::{snowflake_ms, Avatar, Message};
use crate::state::nav::{self, Place};
use crate::state::usersettings::Remote;
use crate::state::Store;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Align, Id, Key, Layout, Modifiers, RichText, Sense};
use std::collections::HashSet;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    /// Une carte par salon non lu ; rien n'est demandé au réseau.
    #[default]
    Unreads,
    Mentions,
    /// Messages enregistrés sans échéance (lot ND).
    Saved,
    /// Messages enregistrés avec un rappel.
    Reminders,
    /// Messages programmés (plugin, lot E3).
    Scheduled,
}

/// Événements montrés dans « Pour vous » : en cours, ou qui commencent d'ici là.
const EVENT_SOON_MS: i64 = 24 * 60 * 60 * 1000;
/// Lignes de chaque section de « Pour vous », au plus.
const FOR_YOU_MAX: usize = 20;
/// Largeur et hauteur du panneau, au plus (il tient toujours dans la fenêtre).
const WIDTH: f32 = 600.0;
const MAX_HEIGHT: f32 = 660.0;
/// Marge intérieure du panneau.
const MARGIN: f32 = 16.0;
/// Écart entre deux cartes.
const GAP: f32 = 12.0;
/// Hauteur supposée d'une carte avant son premier dessin.
const ESTIMATE: f32 = 150.0;
/// Messages montrés par carte : d'abord, puis après « Voir la suite ».
const PREVIEW: usize = 3;
const EXPANDED: usize = 25;
/// « Tout marquer comme lu » demande confirmation au-delà de ce nombre de salons.
const CONFIRM_ABOVE: usize = 5;
/// Deux messages du même auteur à moins de cela sont groupés (comme dans la discussion).
const GROUP_MS: i64 = 7 * 60 * 1000;
/// Identifiants des cartes qui ne sont pas des salons.
const FOR_YOU: u64 = 1;
const FOOTER: u64 = 2;

/// Demandes d'ami reçues (identifiants), dans l'ordre de la page Amis.
pub(super) fn for_you_requests(s: &Store) -> Vec<u64> {
    use crate::state::relations::{FriendTab, RelKind};
    s.friend_list(FriendTab::Pending, "").into_iter().filter(|id| s.relations.relation(*id).is_some_and(|r| r.kind == RelKind::Incoming)).take(FOR_YOU_MAX).collect()
}

/// Événements en cours ou qui commencent dans les 24 heures, parmi ceux que
/// le magasin connaît déjà : (serveur, nom, début, en cours), les plus proches d'abord.
pub(super) fn for_you_events(s: &Store, now_ms: i64) -> Vec<(u64, String, i64, bool)> {
    use crate::state::admin::events::Status;
    let mut out: Vec<(u64, String, i64, bool)> = s
        .guilds
        .iter()
        .filter_map(|g| s.events.guild(g.id))
        .flat_map(|e| e.list.iter())
        .filter(|e| e.status == Status::Active || (e.status == Status::Scheduled && e.start_ms > now_ms && e.start_ms - now_ms <= EVENT_SOON_MS))
        .map(|e| (e.guild_id, e.name.clone(), e.start_ms, e.status == Status::Active))
        .collect();
    out.sort_by_key(|(_, _, start, live)| (!*live, *start));
    out.truncate(FOR_YOU_MAX);
    out
}

#[derive(Default)]
pub(super) struct InboxUi {
    pub(super) open: bool,
    pub(super) tab: Tab,
    /// Cartes repliées (le temps de la session).
    folded: HashSet<u64>,
    /// Cartes dont « Voir la suite » a été demandé.
    expanded: HashSet<u64>,
    /// « Tout marquer comme lu » attend sa confirmation.
    confirm_all: bool,
    /// Messages mis en forme des cartes visibles.
    docs: Docs,
    list: CardList,
    /// Ouverte à cette image : le clic qui l'a ouverte ne la referme pas.
    fresh: bool,
    /// Image où un clic à l'extérieur l'a fermée : le même clic, sur son bouton, ne la rouvre pas.
    closed_frame: Option<u64>,
}

/// Ce qu'une carte demande, appliqué après la liste.
enum Pick {
    Jump { channel: u64, message: u64 },
    Remove(u64),
    Open(Place),
    MarkRead(u64),
    MarkAll,
    /// « Afficher les messages » d'un salon dont l'historique n'est pas en mémoire.
    Load(u64),
    Fold(u64),
    Expand(u64),
    /// Demande d'ami : acceptée (`true`) ou refusée.
    Friend(u64, bool),
    /// Événements d'un serveur.
    Events(u64),
    /// Vide les lignes des mots-clés surveillés (plugin, lot E1).
    ClearKeywords,
    MoreMentions,
    RetryMentions,
    Unsave { channel: u64, message: u64 },
    RefreshSaved,
    Scheduled(super::scheduled::Act),
}

/// Cloche d'une carte : son bouton, et le salon dont le menu est (ou va être) ouvert.
struct Bell {
    resp: egui::Response,
    id: Id,
    guild: u64,
    channel: u64,
}

/// Ce dont une carte a besoin pour se dessiner.
struct Cx<'a> {
    t: &'a Tokens,
    lang: Lang,
    h24: bool,
    store: &'a Store,
    images: &'a Images,
}

/// Message reconstitué (enregistré dont Discord n'a rendu qu'un extrait,
/// message programmé) : il passe par le même rendu que les autres.
fn sketch(id: u64, channel_id: u64, guild_id: Option<u64>, author: (u64, &str, Avatar), content: &str) -> Message {
    Message {
        id,
        channel_id,
        guild_id,
        author_id: author.0,
        author: author.1.to_string(),
        author_avatar: author.2,
        content: content.to_string(),
        attachments: Vec::new(),
        embeds: Vec::new(),
        mentions: Vec::new(),
        mention_everyone: false,
        edited: false,
        system: None,
        meta: Default::default(),
    }
}

/// En-tête du salon `channel` (serveur `guild`, cherché par le salon s'il
/// n'est pas donné) : image, genre, titre et « serveur › catégorie ».
fn place_head<'a>(s: &'a Store, lang: Lang, guild: Option<u64>, channel: u64) -> (HeadIcon<'a>, Option<Icon>, String, String, Option<u64>) {
    let g = match guild {
        Some(id) => s.guilds.iter().find(|g| g.id == id),
        None => s.guilds.iter().find(|g| g.channels.iter().any(|c| c.id == channel)),
    };
    match g {
        Some(g) => {
            let name = words::guild_name(lang, &g.name);
            let category = g.channels.iter().find(|c| c.id == channel).and_then(|c| c.parent).and_then(|p| g.channels.iter().find(|c| c.id == p));
            let sub = match category {
                Some(c) => nf::place(lang, name, &c.name),
                None => name.to_string(),
            };
            let kind = if s.is_thread(channel) { Icon::Thread } else { Icon::Hash };
            (HeadIcon::Guild(g.icon, name), Some(kind), s.channel_name(channel).unwrap_or_default().to_string(), sub, Some(g.id))
        }
        None => match s.dms.iter().find(|d| d.id == channel) {
            Some(d) => {
                let name = words::dm_name(lang, &d.name);
                (HeadIcon::Person(&d.avatar, name), None, name.to_string(), nt::KIND_DM.get(lang).to_string(), None)
            }
            None => (HeadIcon::Glyph(Icon::Hash), None, s.channel_name(channel).unwrap_or_default().to_string(), String::new(), None),
        },
    }
}

/// Ligne sous un message ou en pied de carte, aux marges de la carte.
fn padded<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(egui::Margin::symmetric(PAD as i8, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add).inner
        })
        .inner
}

/// Carte d'un salon non lu.
fn unread_card(ui: &mut egui::Ui, cx: &Cx<'_>, state: (&HashSet<u64>, &HashSet<u64>, &mut Docs), p: Place, pick: &mut Option<Pick>, bell: &mut Option<Bell>) {
    let Some(c) = p.channel else { return };
    let (t, lang, s) = (cx.t, cx.lang, cx.store);
    let (folded, expanded, docs) = state;
    let is_folded = folded.contains(&c);
    let (icon, kind, title, sub, guild) = place_head(s, lang, p.guild, c);
    cards::card(ui, t, |ui| {
        let head = cards::head(ui, t, cx.images, &Head { icon, kind, title: &title, sub: &sub }, |ui| {
            let (chevron, tip) = if is_folded { (Icon::ChevronRight, nt::UNFOLD) } else { (Icon::ChevronDown, nt::FOLD) };
            let fold = icon_button(ui, t, chevron, tip.get(lang));
            focus_ring(ui, t, &fold);
            if fold.clicked() {
                *pick = Some(Pick::Fold(c));
            }
            if cards::square_button(ui, t, Icon::CheckAll, st::MARK_READ.get(lang), false).clicked() {
                *pick = Some(Pick::MarkRead(c));
            }
            // Cloche : le menu de notification du salon (celui de l'en-tête de la discussion).
            if let Some(guild) = guild {
                let id = Id::unique(("ferrocord.boite.cloche", c));
                let open = egui::Popup::is_id_open(ui.ctx(), id);
                let resp = cards::square_button(ui, t, Icon::Bell, nt::CHANNEL_NOTIFS.get(lang), open);
                if open || resp.clicked() {
                    *bell = Some(Bell { resp, id, guild, channel: c });
                }
            }
            let mentions = s.mention_count(c);
            if mentions > 0 {
                let text = crate::state::notify::badge_text(mentions);
                let galley = ui.painter().layout_no_wrap(text, super::fonts::demi_font(t.typography.small), t.colors.on_accent);
                let side = (galley.size().y + 4.0).round();
                let (rect, resp) = ui.allocate_exact_size(egui::vec2((galley.size().x + 10.0).max(side), side), Sense::hover());
                ui.painter().rect_filled(rect, side / 2.0, t.danger_fill());
                ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.colors.on_accent);
                resp.on_hover_text(nt::MENTIONED.get(lang));
            }
        });
        if head.clicked() {
            *pick = Some(Pick::Open(p));
        }
        if is_folded {
            return;
        }
        cards::rule(ui, t);
        let wide = expanded.contains(&c);
        match s.unread_cached(c, if wide { EXPANDED } else { PREVIEW }) {
            Some((n, msgs)) => {
                ui.add_space(4.0);
                let rev = s.chans.get(&c).map_or(0, |cs| cards::stamp(cs.rev));
                let mut prev: Option<&Message> = None;
                for m in &msgs {
                    let grouped = m.system.is_none() && m.meta.reply.is_none() && prev.is_some_and(|p| p.system.is_none() && p.author_id == m.author_id && snowflake_ms(m.id) - snowflake_ms(p.id) < GROUP_MS);
                    let prep = docs.get(s, m, guild, rev, lang, cx.h24);
                    if let Some(id) = cards::message(ui, t, lang, s, cx.images, prep, grouped, snowflake_ms(m.id)) {
                        *pick = Some(Pick::Jump { channel: c, message: id });
                    }
                    prev = Some(m);
                }
                ui.add_space(4.0);
                if n > msgs.len() {
                    cards::rule(ui, t);
                    padded(ui, |ui| {
                        if wide {
                            if widgets::button(ui, t, Kind::Link, nt::OPEN_CHANNEL.get(lang)).clicked() {
                                *pick = Some(Pick::Open(p));
                            }
                        } else if widgets::button(ui, t, Kind::Link, &nf::more_messages(lang, n - msgs.len())).clicked() {
                            *pick = Some(Pick::Expand(c));
                        }
                    });
                }
            }
            // Rien en mémoire : la carte dit seulement qu'il y a du nouveau.
            None => padded(ui, |ui| {
                let cs = s.chans.get(&c);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if cs.is_some_and(|cs| cs.loading) {
                        ui.label(RichText::new(nt::LOADING.get(lang)).color(t.colors.muted));
                    } else if cs.is_some_and(|cs| cs.loaded) || s.is_forum(c) {
                        if widgets::compact(ui, nt::OPEN_CHANNEL.get(lang)).clicked() {
                            *pick = Some(Pick::Open(p));
                        }
                    } else if widgets::compact(ui, nt::SHOW_MESSAGES.get(lang)).clicked() {
                        *pick = Some(Pick::Load(c));
                    }
                    let room = egui::vec2(ui.available_width(), ui.available_height());
                    ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                        let mentions = s.mention_count(c);
                        let text = if mentions > 0 { format!("{} \u{b7} {}", nt::NEW_MESSAGES.get(lang), nf::mentions_count(lang, mentions)) } else { nt::NEW_MESSAGES.get(lang).to_string() };
                        ui.add(egui::Label::new(RichText::new(text).color(t.colors.muted)).truncate().selectable(false));
                    });
                });
            }),
        }
    });
}

/// Carte d'un message seul (mention, message marqué, rappel) : en-tête du
/// salon avec « Aller à » et la croix `remove` (infobulle, active), puis le
/// message ; `under` ajoute une ligne sous le message.
#[allow(clippy::too_many_arguments)]
fn message_card(ui: &mut egui::Ui, cx: &Cx<'_>, docs: &mut Docs, guild: Option<u64>, m: &Message, stamp: u64, remove: Option<(&str, bool)>, under: impl FnOnce(&mut egui::Ui)) -> (bool, bool) {
    let (t, lang, s) = (cx.t, cx.lang, cx.store);
    let (icon, kind, title, sub, home) = place_head(s, lang, guild, m.channel_id);
    let (mut jump, mut removed) = (false, false);
    cards::card(ui, t, |ui| {
        let head = cards::head(ui, t, cx.images, &Head { icon, kind, title: &title, sub: &sub }, |ui| {
            if let Some((tip, enabled)) = remove {
                let x = ui.add_enabled_ui(enabled, |ui| icon_button(ui, t, Icon::Close, tip)).inner;
                focus_ring(ui, t, &x);
                removed = x.clicked();
            }
            jump = widgets::compact(ui, nt::GO_TO.get(lang)).clicked();
        });
        jump |= head.clicked();
        cards::rule(ui, t);
        ui.add_space(4.0);
        let prep = docs.get(s, m, home, stamp, lang, cx.h24);
        jump |= cards::message(ui, t, lang, s, cx.images, prep, false, snowflake_ms(m.id)).is_some();
        under(ui);
        ui.add_space(4.0);
    });
    (jump, removed)
}

impl App {
    /// Ouvre ou ferme la boîte. Rien n'est demandé au réseau pour l'onglet
    /// « Messages non lus » ; « Mentions » et « Messages marqués » demandent
    /// leur liste si elle n'est pas connue.
    pub fn toggle_inbox(&mut self) {
        let frame = self.ctx.cumulative_frame_nr();
        let b = &mut self.nav.inbox;
        if !b.open && b.closed_frame == Some(frame) {
            return;
        }
        b.open = !b.open;
        b.confirm_all = false;
        b.docs.clear();
        if b.open {
            b.fresh = true;
            b.list.reset();
            self.inbox_tab_opened();
        }
    }

    /// L'onglet affiché demande sa liste (mentions, messages enregistrés) si elle n'est pas connue.
    fn inbox_tab_opened(&mut self) {
        match self.nav.inbox.tab {
            Tab::Mentions => {
                self.store.inbox_open();
            }
            Tab::Saved | Tab::Reminders => self.store.saved_open(),
            Tab::Unreads | Tab::Scheduled => {}
        }
    }

    /// Ouvre la boîte sur un onglet (aperçu, tests) : « non-lus » (ou
    /// « pour-vous », dont la carte est en tête des non-lus), « mentions »,
    /// « enregistres », « rappels » ou « programmes ».
    pub fn show_inbox_tab(&mut self, tab: &str) {
        self.nav.inbox.tab = match tab {
            "pour-vous" | "non-lus" => Tab::Unreads,
            "enregistres" => Tab::Saved,
            "rappels" => Tab::Reminders,
            "programmes" => Tab::Scheduled,
            _ => Tab::Mentions,
        };
        self.nav.inbox.open = false;
        self.nav.inbox.closed_frame = None;
        self.toggle_inbox();
    }

    /// Replie ou déplie la carte d'un salon (aperçu, tests).
    pub fn inbox_fold(&mut self, channel: u64) {
        let f = &mut self.nav.inbox.folded;
        if !f.remove(&channel) {
            f.insert(channel);
        }
    }

    /// Bouton de l'en-tête de la discussion.
    pub(super) fn inbox_button(&mut self, ui: &mut egui::Ui) {
        let tip = with_chord(self.lang, &self.nav.bindings, Action::Inbox, nt::INBOX.get(self.lang));
        let resp = super::chat_head::head_icon_drawn(ui, &self.theme, Icon::Inbox, &tip, self.nav.inbox.open);
        if resp.clicked() {
            self.toggle_inbox();
        }
    }

    /// Saut vers un message (boîte de réception) : passe par le saut du lot
    /// recherche (salon ouvert, page `around=` si le message n'est pas en
    /// mémoire, surlignage). Le serveur est celui que le magasin connaît
    /// pour ce salon, `_guild` n'est qu'indicatif.
    pub(super) fn jump_to(&mut self, _guild: Option<u64>, channel: u64, message: u64) {
        self.jump_to_message(channel, message);
    }

    /// Salons non lus, dans l'ordre de la liste des salons.
    fn inbox_unread(&self) -> Vec<Place> {
        let unread = self.store.unread();
        if unread.is_empty() {
            return Vec::new();
        }
        nav::all_channels(&self.store).into_iter().filter(|p| p.channel.is_some_and(|c| unread.contains(&c))).collect()
    }

    /// Onglets montrés : « Programmés » seulement quand le plugin est activé.
    fn inbox_tabs(&self) -> Vec<(Tab, &'static str)> {
        let lang = self.lang;
        let mut tabs = vec![(Tab::Unreads, nt::TAB_UNREADS.get(lang)), (Tab::Mentions, nt::TAB_MENTIONS.get(lang)), (Tab::Saved, nt::TAB_MARKED.get(lang)), (Tab::Reminders, nt::TAB_REMINDERS.get(lang))];
        if self.scheduled_uses_vault() {
            tabs.push((Tab::Scheduled, nt::TAB_SCHEDULED.get(lang)));
        }
        tabs
    }

    pub(super) fn inbox_ui(&mut self, ui: &mut egui::Ui) {
        if !self.nav.inbox.open {
            return;
        }
        let ctx = ui.ctx().clone();
        // Un menu de la boîte est ouvert (cloche, filtres) : Échap et le clic extérieur sont pour lui.
        let menu_open = egui::Popup::is_any_open(&ctx);
        if !menu_open && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) && !std::mem::take(&mut self.nav.inbox.confirm_all) {
            self.toggle_inbox();
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let screen = ctx.content_rect();
        let area = self.chat_rect.unwrap_or(screen);
        let edge = t.spacing.chat_pad;
        // Barre de titre intégrée : le bouton y est, le panneau s'ancre sous elle, au bord de
        // la fenêtre. Sinon le bouton est dans l'en-tête de la discussion.
        let pos = if super::titlebar::shown(&ctx) { egui::pos2(screen.right() - edge, area.top() + 4.0) } else { area.right_top() + egui::vec2(-edge, edge + ui.spacing().interact_size.y + 2.0 * t.spacing.chat_head_y) };
        let mobile = super::mobile::is_active(&ctx);
        let width = if mobile { WIDTH.min(super::mobile::fit_width(&ctx, 2.0 * MARGIN)) } else { WIDTH.min(screen.width() - 2.0 * edge) - 2.0 * MARGIN }.max(1.0);
        // Le panneau tient dans la fenêtre : hauteur bornée, défilement interne.
        let height = ((screen.bottom() - edge - pos.y).min(MAX_HEIGHT) - 2.0 * MARGIN).max(1.0);
        let frame = t.window_frame().fill(t.colors.bg1).inner_margin(MARGIN);
        let tabs = self.inbox_tabs();
        if !tabs.iter().any(|(v, _)| *v == self.nav.inbox.tab) {
            self.nav.inbox.tab = Tab::Unreads;
        }
        let was = self.nav.inbox.tab;
        let mut tab = was;
        let mut filter = self.store.inbox.filter;
        let unread = if was == Tab::Unreads { self.inbox_unread() } else { Vec::new() };
        let mut pick = None;
        let mut bell = None;
        let out = egui::Area::new(Id::unique("boite_de_reception")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).show(&ctx, |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.set_height(height);
                // En-tête : icône, titre, boutons carrés.
                ui.allocate_ui_with_layout(egui::vec2(width, cards::SQUARE), Layout::right_to_left(Align::Center), |ui| {
                    if was == Tab::Unreads && !unread.is_empty() && cards::square_button(ui, &t, Icon::CheckAll, nt::MARK_ALL_READ.get(lang), self.nav.inbox.confirm_all).clicked() {
                        if unread.len() > CONFIRM_ABOVE {
                            self.nav.inbox.confirm_all = !self.nav.inbox.confirm_all;
                        } else {
                            pick = Some(Pick::MarkAll);
                        }
                    }
                    let room = egui::vec2(ui.available_width(), ui.available_height());
                    ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                        super::util::inline_icon(ui, Icon::Inbox, t.typography.title + 2.0, t.colors.text_strong);
                        ui.add(egui::Label::new(RichText::new(nt::INBOX.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title + 2.0).color(t.colors.text_strong)).truncate().selectable(false));
                    });
                });
                ui.add_space(8.0);
                // Onglets soulignés ; le bouton des filtres des mentions est dans la rangée.
                ui.horizontal(|ui| {
                    let reserved = if was == Tab::Mentions { cards::SQUARE + ui.spacing().item_spacing.x } else { 0.0 };
                    let labels: Vec<&str> = tabs.iter().map(|(_, l)| *l).collect();
                    let active = tabs.iter().position(|(v, _)| *v == was).unwrap_or(0);
                    if let Some(i) = cards::tab_strip(ui, &t, &labels, active, ui.available_width() - reserved) {
                        tab = tabs[i].0;
                    }
                    if was == Tab::Mentions {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let id = Id::unique("ferrocord.boite.filtres");
                            let resp = cards::square_button(ui, &t, Icon::Sliders, nt::FILTER_MENTIONS.get(lang), egui::Popup::is_id_open(ui.ctx(), id));
                            let here = self.store.guild;
                            egui::Popup::menu(&resp).id(id).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                                super::menu::check(ui, &t, nt::ROLE_MENTIONS.get(lang), &mut filter.roles);
                                super::menu::check(ui, &t, nt::EVERYONE.get(lang), &mut filter.everyone);
                                let mut only = filter.guild.is_some();
                                ui.add_enabled_ui(here.is_some() || only, |ui| {
                                    if super::menu::check(ui, &t, nt::THIS_SERVER.get(lang), &mut only) {
                                        filter.guild = if only { here } else { None };
                                    }
                                });
                            });
                        });
                    }
                });
                ui.add_space(GAP);
                if self.nav.inbox.confirm_all && was == Tab::Unreads {
                    ui.horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if widgets::button(ui, &t, Kind::Secondary, crate::i18n::t::CANCEL.get(lang)).clicked() {
                                self.nav.inbox.confirm_all = false;
                            }
                            if widgets::button(ui, &t, Kind::Primary, nt::MARK_ALL_YES.get(lang)).clicked() {
                                pick = Some(Pick::MarkAll);
                            }
                            let room = egui::vec2(ui.available_width(), ui.available_height());
                            ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                                ui.add(egui::Label::new(RichText::new(nf::mark_all_confirm(lang, unread.len())).color(t.colors.text)).truncate().selectable(false));
                            });
                        });
                    });
                    ui.add_space(GAP);
                }
                let found = match was {
                    Tab::Unreads => self.unreads_tab(ui, &t, &unread, &mut bell),
                    Tab::Mentions => self.mentions_tab(ui, &t),
                    Tab::Saved => self.saved_cards(ui, &t, false),
                    Tab::Reminders => self.saved_cards(ui, &t, true),
                    Tab::Scheduled => self.scheduled_cards(ui, &t),
                };
                pick = pick.take().or(found);
            });
        });
        // Menu de notification du salon, sous la cloche de sa carte.
        if let Some(b) = bell {
            egui::Popup::menu(&b.resp).id(b.id).show(|ui| self.notif_menu(ui, super::settings::NotifTarget::Channel { guild: Some(b.guild), channel: b.channel }));
        }
        if tab != was {
            let b = &mut self.nav.inbox;
            b.tab = tab;
            b.confirm_all = false;
            b.docs.clear();
            b.list.reset();
            self.inbox_tab_opened();
            ctx.request_repaint();
        }
        if filter != self.store.inbox.filter {
            self.store.inbox_filter(filter);
            self.nav.inbox.list.reset();
        }
        let fresh = std::mem::take(&mut self.nav.inbox.fresh);
        self.inbox_apply(pick);
        // Clic à l'extérieur : la boîte se ferme (pas celui qui vient de l'ouvrir, ni celui d'un de ses menus).
        if self.nav.inbox.open && !fresh && !menu_open && !egui::Popup::is_any_open(&ctx) && out.response.clicked_elsewhere() {
            self.toggle_inbox();
            self.nav.inbox.closed_frame = Some(ctx.cumulative_frame_nr());
        }
    }

    fn inbox_apply(&mut self, pick: Option<Pick>) {
        let Some(pick) = pick else { return };
        match pick {
            Pick::Jump { channel, message } => {
                self.toggle_inbox();
                self.jump_to(None, channel, message);
            }
            Pick::Remove(id) => {
                self.store.inbox_delete(id);
            }
            Pick::Open(p) => {
                self.toggle_inbox();
                self.go(p);
            }
            Pick::MarkRead(c) => self.store.mark_read(c),
            Pick::MarkAll => {
                let all: Vec<u64> = self.inbox_unread().iter().filter_map(|p| p.channel).collect();
                self.store.mark_channels_read(&all);
                self.nav.inbox.confirm_all = false;
            }
            // Geste de l'utilisateur : la requête d'historique ordinaire du salon, une seule.
            Pick::Load(c) => self.store.ensure_history(c),
            Pick::Fold(c) => self.inbox_fold(c),
            Pick::Expand(c) => {
                self.nav.inbox.expanded.insert(c);
            }
            Pick::Friend(user_id, accept) => {
                use crate::state::relations::RelationsCmd;
                self.store.relations_write(if accept { RelationsCmd::Accept { user_id } } else { RelationsCmd::Remove { user_id } });
                self.flush_commands();
            }
            Pick::Events(g) => {
                self.toggle_inbox();
                self.open_events(g);
            }
            Pick::ClearKeywords => self.keywords_clear_hits(),
            Pick::MoreMentions => {
                self.store.inbox_more();
            }
            Pick::RetryMentions => {
                self.store.inbox_reload();
            }
            Pick::Unsave { channel, message } => {
                self.store.unsave_message(channel, message);
            }
            Pick::RefreshSaved => self.store.saved_refresh(),
            Pick::Scheduled(act) => {
                // La fenêtre de modification passe devant : la boîte se ferme.
                if matches!(act, super::scheduled::Act::Edit(_)) {
                    self.toggle_inbox();
                }
                self.scheduled_act(act);
            }
        }
        self.ctx.request_repaint();
    }

    /// « Messages non lus » : la carte « Pour vous » s'il y a lieu, puis une carte par salon.
    fn unreads_tab(&mut self, ui: &mut egui::Ui, t: &Tokens, unread: &[Place], bell: &mut Option<Bell>) -> Option<Pick> {
        let lang = self.lang;
        let h24 = self.h24();
        let now = self.store.now();
        let requests = for_you_requests(&self.store);
        let events = for_you_events(&self.store, now);
        let for_you = !requests.is_empty() || !events.is_empty() || self.keyword_hits() > 0;
        if unread.is_empty() && !for_you {
            ui.label(RichText::new(nt::NO_UNREADS.get(lang)).color(t.colors.muted));
            return None;
        }
        let mut ids: Vec<u64> = Vec::with_capacity(unread.len() + 1);
        if for_you {
            ids.push(FOR_YOU);
        }
        ids.extend(unread.iter().filter_map(|p| p.channel));
        let tight = cards::card_tokens(t);
        let cx = Cx { t: &tight, lang, h24, store: &self.store, images: &self.images };
        let keywords = &self.keywords;
        let b = &mut self.nav.inbox;
        let (folded, expanded, docs) = (&b.folded, &b.expanded, &mut b.docs);
        let mut pick = None;
        let first = usize::from(for_you);
        b.list.show(ui, "boite_non_lus", &ids, ESTIMATE, GAP, |ui, i| {
            if i < first {
                for_you_card(ui, &cx, keywords, folded.contains(&FOR_YOU), &requests, &events, &mut pick);
            } else if let Some(p) = unread.get(i - first) {
                unread_card(ui, &cx, (folded, expanded, &mut *docs), *p, &mut pick, bell);
            }
        });
        pick
    }

    /// « Mentions » : une carte par mention, puis le pied (suite, erreur).
    fn mentions_tab(&mut self, ui: &mut egui::Ui, t: &Tokens) -> Option<Pick> {
        let lang = self.lang;
        let h24 = self.h24();
        let inbox = &self.store.inbox;
        if inbox.loaded && inbox.items.is_empty() && inbox.error.is_none() {
            ui.label(RichText::new(nt::NO_MENTIONS.get(lang)).color(t.colors.muted));
            return None;
        }
        let mut ids: Vec<u64> = inbox.items.iter().map(|m| m.msg.id).collect();
        ids.push(FOOTER);
        let tight = cards::card_tokens(t);
        let cx = Cx { t: &tight, lang, h24, store: &self.store, images: &self.images };
        let b = &mut self.nav.inbox;
        let docs = &mut b.docs;
        let mut pick = None;
        b.list.show(ui, "boite_mentions", &ids, ESTIMATE, GAP, |ui, i| {
            let Some(m) = inbox.items.get(i) else {
                // Pied : chargement, erreur ou suite.
                ui.horizontal(|ui| {
                    if inbox.loading {
                        ui.label(RichText::new(nt::LOADING.get(lang)).color(t.colors.muted));
                    } else if let Some(p) = &inbox.error {
                        ui.add(egui::Label::new(RichText::new(nf::load_failed(lang, words::problem(lang, p, h24))).color(t.colors.danger)).wrap());
                        if widgets::compact(ui, nt::RETRY.get(lang)).clicked() {
                            pick = Some(Pick::RetryMentions);
                        }
                    } else if inbox.more && widgets::compact(ui, nt::LOAD_MORE.get(lang)).clicked() {
                        pick = Some(Pick::MoreMentions);
                    }
                });
                return;
            };
            let remove = Some((nt::REMOVE_MENTION.get(lang), !inbox.deleting.contains(&m.msg.id)));
            let (jump, removed) = message_card(ui, &cx, docs, m.guild, &m.msg, 0, remove, |_| {});
            if removed {
                pick = Some(Pick::Remove(m.msg.id));
            } else if jump {
                pick = Some(Pick::Jump { channel: m.msg.channel_id, message: m.msg.id });
            }
        });
        pick
    }

    /// « Messages marqués » (sans échéance) ou « Rappels » (`reminders`) : une
    /// carte par message enregistré. Le message entier est montré s'il est en
    /// mémoire ; sinon son auteur et son extrait, tels que Discord les a rendus.
    fn saved_cards(&mut self, ui: &mut egui::Ui, t: &Tokens, reminders: bool) -> Option<Pick> {
        use crate::i18n::table::nd::{f as sf, t as sx};
        let lang = self.lang;
        let h24 = self.h24();
        let c = &t.colors;
        let now = self.store.now();
        let all = match &self.store.saved.list {
            Remote::Idle | Remote::Loading => {
                ui.spinner();
                return None;
            }
            Remote::Failed(p) => {
                let text = if *p == crate::state::Problem::Forbidden { sx::SAVED_UNAVAILABLE.get(lang).to_string() } else { words::problem(lang, p, h24) };
                ui.add(egui::Label::new(RichText::new(text).color(c.danger)).wrap());
                return widgets::compact(ui, sx::REFRESH.get(lang)).clicked().then_some(Pick::RefreshSaved);
            }
            Remote::Ready(l) => l,
        };
        let list: Vec<&crate::state::saved::Saved> = all.iter().filter(|s| s.due_ms.is_some() == reminders).collect();
        let mut pick = None;
        if list.is_empty() {
            ui.add(egui::Label::new(RichText::new(if reminders { nt::NO_REMINDERS } else { sx::SAVED_EMPTY }.get(lang)).color(c.muted)).wrap());
            ui.add_space(GAP);
            return widgets::compact(ui, sx::REFRESH.get(lang)).clicked().then_some(Pick::RefreshSaved);
        }
        let mut ids: Vec<u64> = list.iter().map(|s| cards::stamp((s.channel_id, s.message_id)) | 4).collect();
        ids.push(FOOTER);
        let tight = cards::card_tokens(t);
        let store = &self.store;
        let cx = Cx { t: &tight, lang, h24, store, images: &self.images };
        let busy = store.saved.busy;
        let b = &mut self.nav.inbox;
        let docs = &mut b.docs;
        b.list.show(ui, if reminders { "boite_rappels" } else { "boite_marques" }, &ids, ESTIMATE, GAP, |ui, i| {
            let Some(s) = list.get(i) else {
                if widgets::compact(ui, sx::REFRESH.get(lang)).clicked() {
                    pick = Some(Pick::RefreshSaved);
                }
                return;
            };
            let sketched;
            let (m, stamp) = match store.find_message(s.channel_id, s.message_id) {
                Some(m) => (m, 1),
                None => {
                    let body = if s.excerpt.is_empty() { sx::SAVED_NO_TEXT.get(lang) } else { s.excerpt.as_str() };
                    sketched = sketch(s.message_id, s.channel_id, s.guild_id, (0, &s.author, Avatar::default_for(s.message_id)), body);
                    (&sketched, 2)
                }
            };
            let (jump, removed) = message_card(ui, &cx, docs, s.guild_id, m, stamp, Some((sx::SAVED_REMOVE.get(lang), !busy)), |ui| {
                if let Some(due) = s.due_ms {
                    padded(ui, |ui| {
                        let (text, ink) = if due <= now { (sf::reminder_due(lang, time::long_date(lang, due)), c.danger) } else { (sf::reminder_on(lang, time::long_date(lang, due)), c.muted) };
                        ui.add(egui::Label::new(RichText::new(text).small().color(ink)).truncate().selectable(false));
                    });
                }
            });
            if removed {
                pick = Some(Pick::Unsave { channel: s.channel_id, message: s.message_id });
            } else if jump {
                pick = Some(Pick::Jump { channel: s.channel_id, message: s.message_id });
            }
        });
        pick
    }

    /// « Programmés » : les messages programmés du plugin, avec leurs actions
    /// (celles de la fenêtre « Messages programmés »). Rien ne part sans un clic.
    fn scheduled_cards(&mut self, ui: &mut egui::Ui, t: &Tokens) -> Option<Pick> {
        use super::scheduled::Act;
        use crate::i18n::table::e3::t as xt;
        let lang = self.lang;
        let h24 = self.h24();
        let now = crate::state::store::now_ms();
        let items = self.scheduled_items().to_vec();
        if items.is_empty() {
            ui.label(RichText::new(xt::SCHED_NONE.get(lang)).color(t.colors.muted));
            return None;
        }
        let ids: Vec<u64> = items.iter().map(|i| cards::stamp(i.id) | 4).collect();
        let tight = cards::card_tokens(t);
        let store = &self.store;
        let cx = Cx { t: &tight, lang, h24, store, images: &self.images };
        let me = store.me.as_ref().map(|u| (u.id, u.name.clone(), u.avatar));
        let b = &mut self.nav.inbox;
        let docs = &mut b.docs;
        let mut pick = None;
        b.list.show(ui, "boite_programmes", &ids, ESTIMATE, GAP, |ui, n| {
            let Some(i) = items.get(n) else { return };
            let Some((id, name, avatar)) = &me else { return };
            let due = i.due_ms <= now;
            let (icon, kind, title, sub, guild) = place_head(store, lang, None, i.channel_id);
            let title = if title.is_empty() { i.place.clone() } else { title };
            cards::card(ui, &tight, |ui| {
                cards::head(ui, &tight, cx.images, &Head { icon, kind, title: &title, sub: &sub }, |ui| {
                    let text = time::stamp(lang, i.due_ms, now, 'f', h24);
                    ui.label(RichText::new(text).small().color(if due { t.colors.accent } else { t.colors.muted }));
                });
                cards::rule(ui, &tight);
                ui.add_space(4.0);
                // L'heure du message est son échéance.
                let m = sketch(i.id, i.channel_id, guild, (*id, name, *avatar), &i.content);
                let prep = docs.get(store, &m, guild, cards::stamp((&i.content, i.due_ms)), lang, h24);
                let _ = cards::message(ui, &tight, lang, store, cx.images, prep, false, i.due_ms);
                padded(ui, |ui| {
                    ui.push_id(i.id, |ui| {
                        if widgets::button(ui, t, if due { Kind::Primary } else { Kind::Secondary }, xt::SCHED_SEND_NOW.get(lang)).clicked() {
                            pick = Some(Pick::Scheduled(Act::Send(i.id)));
                        }
                        if widgets::button(ui, t, Kind::Secondary, xt::SCHED_EDIT.get(lang)).clicked() {
                            pick = Some(Pick::Scheduled(Act::Edit(i.id)));
                        }
                        if widgets::button(ui, t, Kind::Link, crate::i18n::t::DELETE.get(lang)).clicked() {
                            pick = Some(Pick::Scheduled(Act::Delete(i.id)));
                        }
                    });
                });
            });
        });
        pick
    }
}

/// Carte « Pour vous » : ce que le magasin sait déjà (aucune requête).
fn for_you_card(ui: &mut egui::Ui, cx: &Cx<'_>, keywords: &super::keywords::KeywordsUi, folded: bool, requests: &[u64], events: &[(u64, String, i64, bool)], pick: &mut Option<Pick>) {
    use crate::i18n::table::lotn::{f as xf, t as xt};
    let (t, lang, s) = (cx.t, cx.lang, cx.store);
    cards::card(ui, t, |ui| {
        let head = cards::head(ui, t, cx.images, &Head { icon: HeadIcon::Glyph(Icon::Star), kind: None, title: xt::TAB_FOR_YOU.get(lang), sub: "" }, |ui| {
            let (chevron, tip) = if folded { (Icon::ChevronRight, nt::UNFOLD) } else { (Icon::ChevronDown, nt::FOLD) };
            if icon_button(ui, t, chevron, tip.get(lang)).clicked() {
                *pick = Some(Pick::Fold(FOR_YOU));
            }
        });
        if head.clicked() {
            *pick = Some(Pick::Fold(FOR_YOU));
        }
        if folded {
            return;
        }
        cards::rule(ui, t);
        egui::Frame::new().inner_margin(PAD).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.spacing_mut().item_spacing.y = t.spacing.item_y;
            if !requests.is_empty() {
                ui.label(RichText::new(xt::REQUESTS.get(lang)).small().color(t.colors.faint));
            }
            for &id in requests {
                let Some(person) = s.relations.person(id) else { continue };
                ui.horizontal(|ui| {
                    if t.avatars_shown() {
                        let side = t.layout.small_avatar;
                        let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                        super::util::paint_avatar(ui, cx.images, t, super::util::pixel_square(ui, slot.min, side), &person.avatar, &person.name);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.push_id(id, |ui| {
                            if widgets::button(ui, t, Kind::Secondary, st::DECLINE.get(lang)).clicked() {
                                *pick = Some(Pick::Friend(id, false));
                            }
                            if widgets::button(ui, t, Kind::Success, st::ACCEPT.get(lang)).clicked() {
                                *pick = Some(Pick::Friend(id, true));
                            }
                        });
                        let room = egui::vec2(ui.available_width(), ui.available_height());
                        ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(xf::friend_request(lang, &person.name)).color(t.colors.text)).truncate().selectable(false));
                        });
                    });
                });
            }
            if !events.is_empty() {
                ui.label(RichText::new(xt::EVENTS.get(lang)).small().color(t.colors.faint));
            }
            for (n, (guild, name, start, live)) in events.iter().enumerate() {
                let server = s.guilds.iter().find(|g| g.id == *guild).map(|g| words::guild_name(lang, &g.name).to_string()).unwrap_or_default();
                let when = if *live { xt::EVENT_LIVE.get(lang).to_string() } else { time::clock(lang, *start, true, cx.h24) };
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.push_id(n, |ui| widgets::button(ui, t, Kind::Secondary, xt::SEE.get(lang))).inner.clicked() {
                            *pick = Some(Pick::Events(*guild));
                        }
                        let room = egui::vec2(ui.available_width(), ui.available_height());
                        ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                            super::util::inline_icon(ui, Icon::Calendar, t.typography.body, if *live { t.colors.online } else { t.colors.muted });
                            ui.add(egui::Label::new(RichText::new(xf::event_soon(lang, name, server, when)).color(t.colors.text)).truncate().selectable(false));
                        });
                    });
                });
            }
            // Mots-clés surveillés (plugin, lot E1).
            match super::keywords::inbox_rows(ui, t, lang, keywords) {
                Some(super::keywords::InboxPick::Jump { channel, message, .. }) => *pick = Some(Pick::Jump { channel, message }),
                Some(super::keywords::InboxPick::Clear) => *pick = Some(Pick::ClearKeywords),
                None => {}
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, key};
    use super::super::tests::{click, find, frame, offline_boot, settle};
    use super::*;
    use crate::state::inbox::{InboxCmd, InboxEvent};
    use crate::state::markdown::SpanKind;
    use crate::state::model::{Channel, ChannelKind, Dm, Guild, User};
    use crate::state::{Cmd, Event};
    use std::sync::{Arc, Mutex};

    /// Nom de serveur très long : il ne doit jamais pousser ni recouvrir ce qui le suit.
    const LONG: &str = "Confrérie des Très Longs Noms de Serveur Qui N'en Finissent Vraiment Jamais, Tome Second";

    /// Réseau factice qui garde ce qu'on lui envoie.
    #[derive(Clone, Default)]
    struct Net(Arc<Mutex<Vec<Cmd>>>);

    impl super::super::Backend for Net {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            self.0.lock().unwrap().extend(cmds);
        }
    }

    impl Net {
        /// Requêtes de la boîte parties depuis le dernier appel (listes, historique, accusés).
        fn take(&self) -> Vec<Cmd> {
            std::mem::take(&mut *self.0.lock().unwrap()).into_iter().filter(|c| matches!(c, Cmd::Inbox(_) | Cmd::Saved(_) | Cmd::History { .. } | Cmd::HistoryAfter { .. } | Cmd::Relations(_))).collect()
        }
    }

    /// Alpha (10 #général ouvert, 11 #hors-sujet) et un serveur au nom très
    /// long (catégorie 30, salons 20 à 27) ; l'historique du salon 20 est en mémoire.
    fn app_net() -> (App, Net) {
        let chan = |id: u64, name: &str, kind, parent| Channel { id, name: name.into(), kind, position: id as i32, parent, overwrites: None, slowmode: 0 };
        let mut long = vec![chan(30, "Salons textuels", ChannelKind::Category, None)];
        long.extend((20..28).map(|id| chan(id, if id == 20 { "accueil" } else { "annexe" }, ChannelKind::Text, Some(30))));
        let guilds = vec![Guild::new(9, "Alpha".into(), vec![chan(10, "général", ChannelKind::Text, None), chan(11, "hors-sujet", ChannelKind::Text, None)]), Guild::new(8, LONG.into(), long)];
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds, dms: vec![Dm::new(50, "Alice".into(), 50)] });
        // Le salon ouvert a son historique (pas de squelette de chargement derrière la boîte).
        store.apply(Event::History { channel_id: 20, before: None, messages: Vec::new() });
        store.apply(Event::History { channel_id: 10, before: None, messages: Vec::new() });
        let ctx = egui::Context::default();
        let net = Net::default();
        let mut app = App::with_context(ctx.clone(), offline_boot(Box::new(net.clone()), store, crate::images::Images::disabled(ctx)));
        app.lang = Lang::Fr;
        app.open(Some(9), 10);
        let _ = frame(&mut app, Vec::new());
        net.take();
        (app, net)
    }

    fn msg(id: u64, channel: u64, guild: u64, content: &str) -> Message {
        let mut m = sketch(id << 22, channel, Some(guild), (3, "Bob", Avatar::default_for(3)), content);
        m.mentions = vec![(1, "Moi".into())];
        m
    }

    /// Rectangle du premier élément dont le nom ou le texte contient `s` (les
    /// textes simples sont la « valeur » de leur nœud, pas son nom).
    fn text(out: &egui::FullOutput, s: &str) -> Option<egui::Rect> {
        let update = out.platform_output.accesskit_update.as_ref()?;
        let (_, node) = update.nodes.iter().find(|(_, n)| n.label().is_some_and(|l| l.contains(s)) || n.value().is_some_and(|v| v.contains(s)))?;
        let b = node.bounds()?;
        Some(egui::Rect::from_min_max(egui::pos2(b.x0 as f32, b.y0 as f32), egui::pos2(b.x1 as f32, b.y1 as f32)))
    }

    fn shown(app: &mut App, label: &str) -> bool {
        let _ = frame(app, Vec::new());
        let out = frame(app, Vec::new());
        text(&out, label).is_some()
    }

    #[test]
    fn for_you_lists_incoming_requests_and_near_events() {
        use crate::state::relations::{Person, RelKind, Relation, RelationsEvent};
        let mut a = app();
        let rel = |id: u64, kind, name: &str| Relation { id, kind, nickname: None, ignored: false, stranger: false, user: Some(Person { id, name: name.into(), username: name.to_lowercase(), avatar: Avatar::default_for(id) }) };
        a.store.apply(Event::Relations(RelationsEvent::Ready {
            relations: vec![rel(5, RelKind::Incoming, "Carol"), rel(6, RelKind::Outgoing, "Dan"), rel(2, RelKind::Friend, "Alice")],
            people: vec![],
            dms: vec![],
            reads: Some(vec![]),
            heads: vec![],
        }));
        assert_eq!(for_you_requests(&a.store), vec![5], "demandes reçues seulement");
        assert!(for_you_events(&a.store, 0).is_empty());
        a.show_inbox_tab("pour-vous");
        assert_eq!(a.nav.inbox.tab, Tab::Unreads, "« Pour vous » est une carte en tête des non-lus");
        assert!(a.store.take_commands().iter().all(|c| !matches!(c, Cmd::Inbox(_))), "aucune requête pour cet onglet");
        assert!(shown(&mut a, crate::i18n::table::lotn::t::TAB_FOR_YOU.get(Lang::Fr)), "carte « Pour vous »");
        assert!(shown(&mut a, "Carol"), "demande d'ami reçue");
        // Repliée : son contenu n'est plus mis en page.
        a.inbox_fold(FOR_YOU);
        assert!(!shown(&mut a, "Carol"));
    }

    /// Ctrl+I ouvre sur « Messages non lus » sans rien demander ; l'onglet
    /// « Mentions » demande sa première page, une seule fois ; Échap ferme.
    #[test]
    fn ctrl_i_opens_without_asking_and_mentions_ask_once() {
        let (mut a, net) = app_net();
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        let _ = frame(&mut a, vec![key(Key::I, ctrl)]);
        assert!(a.nav.inbox.open && a.nav.inbox.tab == Tab::Unreads);
        for _ in 0..3 {
            let _ = frame(&mut a, Vec::new());
        }
        assert_eq!(net.take(), Vec::new(), "aucun appel réseau à l'ouverture");
        // Onglets comme dans Discord ; « Programmés » seulement avec son plugin, plus de « Pour vous ».
        let out = frame(&mut a, Vec::new());
        for tab in ["Messages non lus", "Mentions", "Messages marqués", "Rappels"] {
            assert!(find(&out, tab).is_some(), "onglet « {tab} »");
        }
        assert!(text(&out, "Programmés").is_none() && text(&out, "Pour vous").is_none());
        click(&mut a, "Mentions");
        assert_eq!(a.nav.inbox.tab, Tab::Mentions);
        assert!(a.store.inbox.loading);
        assert!(matches!(net.take().as_slice(), [Cmd::Inbox(InboxCmd::Load { before: None, .. })]), "une seule demande");
        let _ = frame(&mut a, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!a.nav.inbox.open);
        assert_eq!(a.store.channel, Some(10));
        a.toggle_inbox();
        let _ = frame(&mut a, Vec::new());
        assert_eq!(net.take(), Vec::new(), "déjà en cours : pas de seconde demande");
    }

    /// Une carte par salon non lu, avec « serveur › catégorie » ; les messages
    /// en mémoire sont rendus par le code de la discussion (ni `**` ni balise
    /// brute) ; un salon sans historique en mémoire ne le demande qu'au clic.
    #[test]
    fn unread_cards_render_cached_messages_and_load_only_on_click() {
        let (mut a, net) = app_net();
        a.store.apply(Event::MessageCreate(msg(7, 20, 8, "**Mise à jour** du dépôt <:ferris:123> pour <@1>")));
        a.store.apply(Event::MessageCreate(msg(8, 11, 9, "pas en mémoire")));
        a.toggle_inbox();
        let _ = frame(&mut a, Vec::new());
        let out = frame(&mut a, Vec::new());
        assert!(find(&out, "accueil").is_some() && find(&out, "hors-sujet").is_some(), "une carte par salon");
        assert!(text(&out, &format!("{LONG} \u{203a} Salons textuels")).is_some(), "serveur › catégorie");
        assert!(text(&out, "Mise à jour du dépôt").is_some(), "markdown rendu");
        assert!(text(&out, "**").is_none() && text(&out, "<@").is_none() && text(&out, "<:").is_none(), "aucune balise brute");
        let doc = a.nav.inbox.docs.doc(7 << 22).expect("message mis en forme");
        let spans: Vec<_> = doc.blocks.iter().flat_map(|b| b.spans.iter()).collect();
        assert!(spans.iter().any(|s| s.style.bold && s.text == "Mise à jour"));
        assert!(spans.iter().any(|s| matches!(s.kind, SpanKind::Emoji { id: 123, .. })), "emoji en image");
        assert!(spans.iter().any(|s| matches!(s.kind, SpanKind::Mention(_)) && s.text == "@Moi"), "mention rendue");
        // Le salon 11 n'a pas d'historique en mémoire : rien n'a été demandé pour lui.
        assert!(text(&out, "pas en mémoire").is_none() && text(&out, nt::NEW_MESSAGES.get(Lang::Fr)).is_some());
        assert_eq!(net.take(), Vec::new(), "aucun téléchargement d'historique à l'ouverture");
        click(&mut a, nt::SHOW_MESSAGES.get(Lang::Fr));
        assert_eq!(net.take(), vec![Cmd::History { channel_id: 11, before: None }], "une requête, au clic");
        a.store.apply(Event::History { channel_id: 11, before: None, messages: vec![msg(8, 11, 9, "pas en mémoire")] });
        assert!(shown(&mut a, "pas en mémoire"), "messages montrés une fois reçus");
        // Replier garde la carte et retire ses messages ; l'état tient tant que la session dure.
        a.inbox_fold(20);
        assert!(!shown(&mut a, "Mise à jour du dépôt") && shown(&mut a, "accueil"));
        a.toggle_inbox();
        a.toggle_inbox();
        assert!(!shown(&mut a, "Mise à jour du dépôt"), "repli gardé à la réouverture");
        // Un clic sur le message mène à lui et ferme la boîte.
        a.inbox_fold(20);
        click(&mut a, "Mise à jour du dépôt");
        assert!(!a.nav.inbox.open);
        assert_eq!((a.store.guild, a.store.channel), (Some(8), Some(20)));
    }

    /// « Marquer comme lu » retire la carte ; « Tout marquer comme lu »
    /// demande confirmation quand les salons sont nombreux, puis les accuse ensemble.
    #[test]
    fn mark_read_removes_the_card_and_mark_all_asks_first() {
        let (mut a, net) = app_net();
        for c in 20..28 {
            a.store.apply(Event::MessageCreate(msg(c, c, 8, "du nouveau")));
        }
        a.toggle_inbox();
        assert_eq!(a.store.unread().len(), 8);
        // Cloche : le menu de notification du salon s'ouvre ; Échap le ferme, la boîte reste.
        click(&mut a, nt::CHANNEL_NOTIFS.get(Lang::Fr));
        assert!(shown(&mut a, crate::i18n::table::s::t::LEVEL_INHERIT.get(Lang::Fr)) && a.nav.inbox.open);
        let _ = frame(&mut a, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(a.nav.inbox.open && !shown(&mut a, crate::i18n::table::s::t::LEVEL_INHERIT.get(Lang::Fr)), "Échap ferme d'abord le menu");
        click(&mut a, st::MARK_READ.get(Lang::Fr));
        assert_eq!(a.store.unread().len(), 7, "la carte s'en va");
        click(&mut a, nt::MARK_ALL_READ.get(Lang::Fr));
        assert_eq!(a.store.unread().len(), 7, "premier clic : question seulement");
        assert!(shown(&mut a, "Marquer 7 salons comme lus"));
        click(&mut a, crate::i18n::t::CANCEL.get(Lang::Fr));
        assert_eq!(a.store.unread().len(), 7);
        click(&mut a, nt::MARK_ALL_READ.get(Lang::Fr));
        click(&mut a, nt::MARK_ALL_YES.get(Lang::Fr));
        assert!(a.store.unread().is_empty());
        assert!(shown(&mut a, nt::NO_UNREADS.get(Lang::Fr)));
        assert!(net.take().iter().all(|c| matches!(c, Cmd::Relations(_))), "des accusés, rien d'autre");
    }

    /// Mentions : une carte par mention ; avec un nom de serveur très long,
    /// ni les boutons ni l'heure ne sont recouverts, et le nom est raccourci.
    #[test]
    fn long_names_never_overlap_buttons_or_time() {
        let (mut a, _) = app_net();
        a.show_inbox_tab("mentions");
        let m = msg(7, 20, 8, "@everyone\n**Règlement** mis à jour <@1>");
        let when = time::message(Lang::Fr, snowflake_ms(m.id), a.h24());
        a.store.apply(Event::Inbox(InboxEvent::Page { round: 0, before: None, messages: vec![m] }));
        let _ = frame(&mut a, Vec::new());
        let out = frame(&mut a, Vec::new());
        let name = text(&out, &format!("{LONG} \u{203a}")).expect("nom du serveur");
        let go = find(&out, nt::GO_TO.get(Lang::Fr)).expect("« Aller à »");
        let remove = find(&out, nt::REMOVE_MENTION.get(Lang::Fr)).expect("croix");
        let time = text(&out, &when).expect("heure du message");
        assert!(name.right() <= go.left() && go.right() <= remove.left() + 1.0, "{name:?} {go:?} {remove:?}");
        for other in [go, remove, time] {
            assert!(!name.intersects(other), "{name:?} recouvre {other:?}");
        }
        assert!(name.width() < WIDTH, "nom raccourci : {name:?}");
        assert!(text(&out, "Règlement mis à jour").is_some() && text(&out, "**").is_none(), "texte mis en forme");
        // Les filtres sont dans un menu, plus en cases à cocher sous les onglets.
        assert!(text(&out, nt::ROLE_MENTIONS.get(Lang::Fr)).is_none());
        click(&mut a, nt::FILTER_MENTIONS.get(Lang::Fr));
        assert!(shown(&mut a, nt::ROLE_MENTIONS.get(Lang::Fr)) && a.nav.inbox.open);
        // Saut : le salon s'ouvre sur le message.
        let item = &a.store.inbox.items[0];
        assert_eq!(item.guild, Some(8));
        let (guild, channel, id) = (item.guild, item.msg.channel_id, item.msg.id);
        a.jump_to(guild, channel, id);
        assert_eq!((a.store.guild, a.store.channel), (Some(8), Some(20)));
    }

    /// Boîte ouverte sur des cartes, sans animation en cours : plus aucune image n'est demandée.
    #[test]
    fn open_inbox_at_rest_asks_for_no_frame() {
        let (mut a, _) = app_net();
        a.store.apply(Event::MessageCreate(msg(7, 20, 8, "**Mise à jour** <@1>")));
        a.store.apply(Event::MessageCreate(msg(8, 11, 9, "ailleurs")));
        a.toggle_inbox();
        let out = settle(&mut a);
        assert!(a.nav.inbox.open && text(&out, "Mise à jour").is_some());
        assert!(out.viewport_output.values().all(|v| v.repaint_delay == std::time::Duration::MAX), "redessin au repos : {:?}", a.ctx.repaint_causes());
    }

    /// Un clic hors du panneau le ferme ; un clic dedans, non.
    #[test]
    fn a_click_outside_closes_the_panel() {
        let (mut a, _) = app_net();
        a.toggle_inbox();
        let press = |at: egui::Pos2, pressed| egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() };
        let click_at = |a: &mut App, at: egui::Pos2| {
            let _ = frame(a, vec![egui::Event::PointerMoved(at), press(at, true)]);
            let _ = frame(a, vec![press(at, false)]);
            let _ = frame(a, Vec::new());
        };
        let _ = frame(&mut a, Vec::new());
        let out = frame(&mut a, Vec::new());
        let title = find(&out, nt::TAB_UNREADS.get(Lang::Fr)).expect("onglet").center();
        click_at(&mut a, title);
        assert!(a.nav.inbox.open, "clic dans le panneau");
        click_at(&mut a, egui::pos2(20.0, 700.0));
        assert!(!a.nav.inbox.open, "clic à l'extérieur");
    }
}
