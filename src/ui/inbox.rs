//! Boîte de réception (bouton en haut à droite, Ctrl+I) : onglet
//! « Mentions » (mentions récentes de Discord, chargées à la demande,
//! `state::inbox`) et onglet « Non lus » (magasin des non-lus, local).
//! Un clic mène au message ; « Marquer comme lu » reprend l'accusé de lecture
//! existant (`Store::mark_read`).
//!
//! Lignes de hauteur fixe : seules les lignes visibles sont dessinées.

use super::keys::{focus_ring, with_chord};
use super::util::icon_button;
use super::{words, App};
use crate::i18n::table::nav::{f as nf, t as nt};
use crate::i18n::table::social::t as st;
use crate::i18n::{time, Lang};
use crate::state::inbox::Filter;
use crate::state::keys::Action;
use crate::state::model::snowflake_ms;
use crate::state::nav::{self, Place};
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, Align, Id, Key, Layout, Modifiers, RichText, Sense};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    Mentions,
    Unreads,
    /// « Pour vous » (lot N) : demandes d'ami reçues, événements en cours ou proches.
    ForYou,
    /// Messages enregistrés et rappels (lot ND, `ui/saved.rs`).
    Saved,
}

/// Événements montrés dans « Pour vous » : en cours, ou qui commencent d'ici là.
const EVENT_SOON_MS: i64 = 24 * 60 * 60 * 1000;
/// Lignes de chaque section de « Pour vous », au plus.
const FOR_YOU_MAX: usize = 20;

/// Demandes d'ami reçues (identifiants), dans l'ordre de la page Amis.
pub(super) fn for_you_requests(s: &crate::state::Store) -> Vec<u64> {
    use crate::state::relations::{FriendTab, RelKind};
    s.friend_list(FriendTab::Pending, "").into_iter().filter(|id| s.relations.relation(*id).is_some_and(|r| r.kind == RelKind::Incoming)).take(FOR_YOU_MAX).collect()
}

/// Événements en cours ou qui commencent dans les 24 heures, parmi ceux que
/// le magasin connaît déjà : (serveur, nom, début, en cours), les plus proches d'abord.
pub(super) fn for_you_events(s: &crate::state::Store, now_ms: i64) -> Vec<(u64, String, i64, bool)> {
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
}

/// Ce qu'une ligne demande, appliqué après la liste.
enum Pick {
    Jump { guild: Option<u64>, channel: u64, message: u64 },
    Remove(u64),
    Open(Place),
    MarkRead(u64),
    /// Demande d'ami : acceptée (`true`) ou refusée.
    Friend(u64, bool),
    /// Événements d'un serveur.
    Events(u64),
    /// Vide les lignes des mots-clés surveillés (plugin, lot E1).
    ClearKeywords,
}

impl App {
    /// Ouvre ou ferme la boîte ; l'onglet « Mentions » demande sa première page à l'ouverture.
    pub fn toggle_inbox(&mut self) {
        self.nav.inbox.open = !self.nav.inbox.open;
        if self.nav.inbox.open && self.nav.inbox.tab == Tab::Mentions {
            self.store.inbox_open();
        }
        if self.nav.inbox.open && self.nav.inbox.tab == Tab::Saved {
            self.store.saved_open();
        }
    }

    /// Ouvre la boîte sur un onglet (aperçu, tests) : « pour-vous », « non-lus » ou « mentions ».
    pub fn show_inbox_tab(&mut self, tab: &str) {
        self.nav.inbox.tab = match tab {
            "pour-vous" => Tab::ForYou,
            "non-lus" => Tab::Unreads,
            "enregistres" => Tab::Saved,
            _ => Tab::Mentions,
        };
        self.nav.inbox.open = false;
        self.toggle_inbox();
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

    pub(super) fn inbox_ui(&mut self, ui: &mut egui::Ui) {
        if !self.nav.inbox.open {
            return;
        }
        if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape)) {
            self.nav.inbox.open = false;
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let screen = ui.ctx().content_rect();
        let area = self.chat_rect.unwrap_or(screen);
        let width = (area.width() - 2.0 * t.spacing.chat_pad).clamp(1.0, t.layout.card_width * 1.3);
        let height = (screen.height() * 0.7).max(1.0);
        let pos = area.right_top() + egui::vec2(-t.spacing.chat_pad, t.spacing.chat_pad + ui.spacing().interact_size.y + 2.0 * t.spacing.chat_head_y);
        let frame = t.window_frame().inner_margin(t.toast_margin());
        // Disposition mobile : le cadre tient dans l'écran, marges comprises.
        let width = if super::mobile::is_active(ui.ctx()) { width.min(super::mobile::fit_width(ui.ctx(), t.toast_margin().sum().x)) } else { width };
        let mut pick = None;
        let mut saved = None;
        let mut close = false;
        let mut tab = self.nav.inbox.tab;
        let mut filter = self.store.inbox.filter;
        egui::Area::new(Id::unique("boite_de_reception")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_TOP).fixed_pos(pos).show(ui.ctx(), |ui| {
            frame.show(ui, |ui| {
                ui.set_width(width);
                ui.set_max_height(height);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(nt::INBOX.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        close = icon_button(ui, &t, Icon::Close, crate::i18n::t::CLOSE_TIP.get(lang)).clicked();
                    });
                });
                ui.horizontal(|ui| {
                    for (v, label) in [(Tab::ForYou, crate::i18n::table::lotn::t::TAB_FOR_YOU), (Tab::Unreads, nt::TAB_UNREADS), (Tab::Mentions, nt::TAB_MENTIONS), (Tab::Saved, crate::i18n::table::nd::t::SAVED_TAB)] {
                        let r = ui.selectable_label(tab == v, label.get(lang));
                        focus_ring(ui, &t, &r);
                        if r.clicked() {
                            tab = v;
                        }
                    }
                });
                ui.separator();
                match tab {
                    Tab::Mentions => pick = self.mentions_tab(ui, &t, lang, &mut filter),
                    Tab::Unreads => pick = self.unreads_tab(ui, &t, lang),
                    Tab::ForYou => pick = self.for_you_tab(ui, &t, lang),
                    Tab::Saved => saved = self.saved_tab(ui, &t, lang),
                }
            });
        });
        if tab != self.nav.inbox.tab {
            self.nav.inbox.tab = tab;
            if tab == Tab::Mentions {
                self.store.inbox_open();
            }
            if tab == Tab::Saved {
                self.store.saved_open();
            }
        }
        if filter != self.store.inbox.filter {
            self.store.inbox_filter(filter);
        }
        match saved {
            Some(super::saved::SavedPick::Jump { guild, channel, message }) => pick = Some(Pick::Jump { guild, channel, message }),
            Some(super::saved::SavedPick::Remove { channel, message }) => {
                self.store.unsave_message(channel, message);
            }
            Some(super::saved::SavedPick::Refresh) => self.store.saved_refresh(),
            None => {}
        }
        match pick {
            Some(Pick::Jump { guild, channel, message }) => {
                self.nav.inbox.open = false;
                self.jump_to(guild, channel, message);
            }
            Some(Pick::Remove(id)) => {
                self.store.inbox_delete(id);
            }
            Some(Pick::Open(p)) => {
                self.nav.inbox.open = false;
                self.go(p);
            }
            Some(Pick::MarkRead(c)) => self.store.mark_read(c),
            Some(Pick::Friend(user_id, accept)) => {
                use crate::state::relations::RelationsCmd;
                self.store.relations_write(if accept { RelationsCmd::Accept { user_id } } else { RelationsCmd::Remove { user_id } });
                self.flush_commands();
            }
            Some(Pick::Events(g)) => {
                self.nav.inbox.open = false;
                self.open_events(g);
            }
            Some(Pick::ClearKeywords) => self.keywords_clear_hits(),
            None => {}
        }
        if close {
            self.nav.inbox.open = false;
        }
    }

    fn mentions_tab(&mut self, ui: &mut egui::Ui, t: &Tokens, lang: Lang, filter: &mut Filter) -> Option<Pick> {
        let here = self.store.guild;
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut filter.roles, nt::ROLE_MENTIONS.get(lang));
            ui.checkbox(&mut filter.everyone, nt::EVERYONE.get(lang));
            let mut only = filter.guild.is_some();
            if ui.add_enabled(here.is_some() || only, egui::Checkbox::new(&mut only, nt::THIS_SERVER.get(lang))).changed() {
                filter.guild = if only { here } else { None };
            }
        });
        let h24 = self.h24();
        let inbox = &self.store.inbox;
        let mut pick = None;
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let small = ui.text_style_height(&egui::TextStyle::Small);
        // Première ligne à la hauteur de la croix (plus haute que le texte) : la
        // ligne entière tient dans sa hauteur, sans déborder sur la suivante.
        let head_h = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y).max(line).max(small);
        let row_h = (head_h + line + 3.0 * t.spacing.item_y).ceil();
        // Disposition mobile : la première ligne a la hauteur d'un bouton tactile (croix).
        let row_h = if super::mobile::is_active(ui.ctx()) { (ui.spacing().interact_size.y + line + 3.0 * t.spacing.item_y).ceil().max(row_h) } else { row_h };
        if inbox.loaded && inbox.items.is_empty() && inbox.error.is_none() {
            ui.label(RichText::new(nt::NO_MENTIONS.get(lang)).color(t.colors.muted));
        }
        let footer = ui.spacing().interact_size.y + 2.0 * t.spacing.item_y;
        egui::ScrollArea::vertical().id_salt("boite_mentions").max_height((ui.available_height() - footer).max(row_h)).auto_shrink([false, true]).show_rows(ui, row_h, inbox.items.len(), |ui, range| {
            for i in range {
                let Some(m) = inbox.items.get(i) else { continue };
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
                }
                focus_ring(ui, t, &resp);
                let inner = rect.shrink2(egui::vec2(t.spacing.item_x, t.spacing.item_y));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::top_down(Align::Min)));
                let mut removed = false;
                let mobile = super::mobile::is_active(ui.ctx());
                let head = egui::vec2(inner.width(), if mobile { ui.spacing().interact_size.y } else { head_h });
                child.allocate_ui_with_layout(head, Layout::left_to_right(Align::Center), |ui| {
                    let place = match (m.guild, self.store.channel_name(m.msg.channel_id)) {
                        (Some(g), Some(c)) => {
                            let guild = self.store.guilds.iter().find(|x| x.id == g).map(|x| words::guild_name(lang, &x.name).to_string()).unwrap_or_default();
                            nf::in_guild(lang, c, guild)
                        }
                        _ => self.store.dms.iter().find(|d| d.id == m.msg.channel_id).map(|d| format!("@{}", words::dm_name(lang, &d.name))).unwrap_or_default(),
                    };
                    let names = |ui: &mut egui::Ui| {
                        ui.add(egui::Label::new(RichText::new(&m.msg.author).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).truncate().selectable(false));
                        ui.add(egui::Label::new(RichText::new(place).small().color(t.colors.faint)).truncate().selectable(false));
                    };
                    let tail = |ui: &mut egui::Ui| {
                        let busy = inbox.deleting.contains(&m.msg.id);
                        let x = ui.add_enabled_ui(!busy, |ui| icon_button(ui, t, Icon::Close, nt::REMOVE_MENTION.get(lang))).inner;
                        focus_ring(ui, t, &x);
                        ui.label(RichText::new(time::clock(lang, snowflake_ms(m.msg.id), true, h24)).small().color(t.colors.faint));
                        x.clicked()
                    };
                    if mobile {
                        // Disposition mobile : croix et heure d'abord, les noms raccourcis dans la place qui reste.
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            removed = tail(ui);
                            let size = egui::vec2(ui.available_width(), ui.available_height());
                            ui.allocate_ui_with_layout(size, Layout::left_to_right(Align::Center), names);
                        });
                    } else {
                        names(ui);
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| removed = tail(ui));
                    }
                });
                child.add(egui::Label::new(RichText::new(&m.excerpt).color(t.colors.text)).truncate().selectable(false));
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &m.excerpt));
                if removed {
                    pick = Some(Pick::Remove(m.msg.id));
                } else if resp.clicked() {
                    pick = Some(Pick::Jump { guild: m.guild, channel: m.msg.channel_id, message: m.msg.id });
                }
            }
        });
        let mut more = false;
        let mut retry = false;
        ui.horizontal(|ui| {
            if inbox.loading {
                ui.label(RichText::new(nt::LOADING.get(lang)).color(t.colors.muted));
            } else if let Some(p) = &inbox.error {
                ui.label(RichText::new(nf::load_failed(lang, words::problem(lang, p, h24))).color(t.colors.danger));
                retry = ui.button(nt::RETRY.get(lang)).clicked();
            } else if inbox.more {
                more = ui.button(nt::LOAD_MORE.get(lang)).clicked();
            }
        });
        if more {
            self.store.inbox_more();
        }
        if retry {
            self.store.inbox_reload();
        }
        pick
    }

    /// « Pour vous » : ce que le magasin sait déjà (aucune requête).
    fn for_you_tab(&mut self, ui: &mut egui::Ui, t: &Tokens, lang: Lang) -> Option<Pick> {
        use super::widgets::{self, Kind};
        use crate::i18n::table::lotn::{f as xf, t as xt};
        let s = &self.store;
        let now = s.now();
        let requests = for_you_requests(s);
        let events = for_you_events(s, now);
        if requests.is_empty() && events.is_empty() && self.keyword_hits() == 0 {
            ui.label(RichText::new(xt::FOR_YOU_EMPTY.get(lang)).color(t.colors.muted));
            return None;
        }
        let h24 = self.h24();
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("boite_pour_vous").auto_shrink([false, true]).show(ui, |ui| {
            if !requests.is_empty() {
                ui.label(RichText::new(xt::REQUESTS.get(lang)).small().color(t.colors.faint));
            }
            for id in requests {
                let Some(person) = s.relations.person(id) else { continue };
                ui.horizontal(|ui| {
                    if t.avatars_shown() {
                        let side = t.layout.avatar_size;
                        let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                        super::util::paint_avatar(ui, &self.images, t, super::util::pixel_square(ui, slot.min, side), &person.avatar, &person.name);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, t, Kind::Secondary, st::DECLINE.get(lang)).clicked() {
                            pick = Some(Pick::Friend(id, false));
                        }
                        if widgets::button(ui, t, Kind::Success, st::ACCEPT.get(lang)).clicked() {
                            pick = Some(Pick::Friend(id, true));
                        }
                        let room = egui::vec2(ui.available_width(), ui.available_height());
                        ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                            ui.add(egui::Label::new(RichText::new(xf::friend_request(lang, &person.name)).color(t.colors.text)).truncate().selectable(false));
                        });
                    });
                });
            }
            if !events.is_empty() {
                ui.add_space(t.spacing.item_y);
                ui.label(RichText::new(xt::EVENTS.get(lang)).small().color(t.colors.faint));
            }
            for (guild, name, start, live) in events {
                let server = s.guilds.iter().find(|g| g.id == guild).map(|g| words::guild_name(lang, &g.name).to_string()).unwrap_or_default();
                let when = if live { xt::EVENT_LIVE.get(lang).to_string() } else { time::clock(lang, start, true, h24) };
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if widgets::button(ui, t, Kind::Secondary, xt::SEE.get(lang)).clicked() {
                            pick = Some(Pick::Events(guild));
                        }
                        let room = egui::vec2(ui.available_width(), ui.available_height());
                        ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                            let side = Tokens::icon_side(t.typography.body);
                            let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                            crate::theme::icons::paint(Icon::Calendar, ui.painter(), slot, if live { t.colors.online } else { t.colors.muted });
                            ui.add(egui::Label::new(RichText::new(xf::event_soon(lang, &name, server, when)).color(t.colors.text)).truncate().selectable(false));
                        });
                    });
                });
            }
            // Mots-clés surveillés (plugin, lot E1).
            match super::keywords::inbox_rows(ui, t, lang, &self.keywords) {
                Some(super::keywords::InboxPick::Jump { guild, channel, message }) => pick = Some(Pick::Jump { guild, channel, message }),
                Some(super::keywords::InboxPick::Clear) => pick = Some(Pick::ClearKeywords),
                None => {}
            }
        });
        pick
    }

    fn unreads_tab(&mut self, ui: &mut egui::Ui, t: &Tokens, lang: Lang) -> Option<Pick> {
        let s = &self.store;
        let unread = s.unread();
        let list: Vec<Place> = nav::all_channels(s).into_iter().filter(|p| p.channel.is_some_and(|c| unread.contains(&c))).collect();
        if list.is_empty() {
            ui.label(RichText::new(nt::NO_UNREADS.get(lang)).color(t.colors.muted));
            return None;
        }
        let row_h = t.spacing.channel_row + t.spacing.item_y;
        let mut pick = None;
        egui::ScrollArea::vertical().id_salt("boite_non_lus").auto_shrink([false, true]).show_rows(ui, row_h, list.len(), |ui, range| {
            for p in &list[range] {
                let Some(c) = p.channel else { continue };
                let (name, context) = match p.guild {
                    Some(g) => (
                        format!("#{}", s.channel_name(c).unwrap_or_default()),
                        s.guilds.iter().find(|x| x.id == g).map(|x| words::guild_name(lang, &x.name).to_string()).unwrap_or_default(),
                    ),
                    None => (s.dms.iter().find(|d| d.id == c).map(|d| words::dm_name(lang, &d.name).to_string()).unwrap_or_default(), nt::KIND_DM.get(lang).to_string()),
                };
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
                }
                focus_ring(ui, t, &resp);
                let inner = rect.shrink2(egui::vec2(t.spacing.item_x, 0.0));
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::right_to_left(Align::Center)));
                let read = crate::ui::widgets::compact(&mut child, st::MARK_READ.get(lang));
                focus_ring(&child, t, &read);
                if s.mentioned().contains(&c) {
                    let (dot, _) = child.allocate_exact_size(egui::Vec2::splat(2.0 * t.shapes.mention_dot), Sense::hover());
                    child.painter().circle_filled(dot.center(), t.shapes.mention_dot, t.colors.danger);
                    child.label(RichText::new(nt::MENTIONED.get(lang)).small().color(t.colors.danger));
                }
                child.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    ui.add(egui::Label::new(RichText::new(&name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).truncate().selectable(false));
                    ui.add(egui::Label::new(RichText::new(context).small().color(t.colors.faint)).truncate().selectable(false));
                });
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
                if read.clicked() {
                    pick = Some(Pick::MarkRead(c));
                } else if resp.clicked() {
                    pick = Some(Pick::Open(*p));
                }
            }
        });
        pick
    }
}

#[cfg(test)]
mod tests {
    use super::super::keys::tests::{app, frame, key};
    use super::*;
    use crate::state::inbox::{InboxCmd, InboxEvent};
    use crate::state::Cmd;

    #[test]
    fn for_you_lists_incoming_requests_and_near_events() {
        use crate::state::relations::{Person, RelKind, Relation, RelationsEvent};
        let mut a = app();
        let rel = |id: u64, kind, name: &str| Relation { id, kind, nickname: None, ignored: false, user: Some(Person { id, name: name.into(), username: name.to_lowercase(), avatar: crate::state::model::Avatar::default_for(id) }) };
        a.store.apply(crate::state::Event::Relations(RelationsEvent::Ready {
            relations: vec![rel(5, RelKind::Incoming, "Carol"), rel(6, RelKind::Outgoing, "Dan"), rel(2, RelKind::Friend, "Alice")],
            people: vec![],
            dms: vec![],
            reads: Some(vec![]),
            heads: vec![],
        }));
        assert_eq!(for_you_requests(&a.store), vec![5], "demandes reçues seulement");
        assert!(for_you_events(&a.store, 0).is_empty());
        a.show_inbox_tab("pour-vous");
        assert_eq!(a.nav.inbox.tab, Tab::ForYou);
        assert!(a.store.take_commands().iter().all(|c| !matches!(c, Cmd::Inbox(_))), "aucune requête pour cet onglet");
        frame(&mut a, Vec::new());
        frame(&mut a, Vec::new());
    }

    #[test]
    fn ctrl_i_opens_and_asks_the_first_page_once() {
        let mut a = app();
        frame(&mut a, Vec::new());
        let _ = a.store.take_commands();
        let ctrl = Modifiers { ctrl: true, command: true, ..Modifiers::NONE };
        frame(&mut a, vec![key(Key::I, ctrl)]);
        assert!(a.nav.inbox.open);
        // L'image a vidé la file : la demande est partie vers le réseau factice.
        assert!(a.store.inbox.loading);
        frame(&mut a, vec![key(Key::Escape, Modifiers::NONE)]);
        assert!(!a.nav.inbox.open);
        assert_eq!(a.store.channel, Some(10));
        a.toggle_inbox();
        assert!(a.store.take_commands().iter().all(|c| !matches!(c, Cmd::Inbox(InboxCmd::Load { .. }))), "déjà en cours : pas de seconde demande");
    }

    #[test]
    fn unread_tab_lists_and_marks_read_and_jump_opens_the_channel() {
        let mut a = app();
        let m = crate::state::model::Message {
            id: 7 << 22,
            channel_id: 20,
            guild_id: Some(8),
            author_id: 3,
            author: "Bob".into(),
            author_avatar: crate::state::model::Avatar::default_for(3),
            content: "coucou <@1>".into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: vec![(1, "Moi".into())],
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        };
        a.store.apply(crate::state::Event::MessageCreate(m.clone()));
        a.nav.inbox.tab = Tab::Unreads;
        a.toggle_inbox();
        frame(&mut a, Vec::new());
        a.store.mark_read(20);
        assert!(a.store.unread().is_empty());
        // Mention reçue par la page : le clic ouvre son salon.
        a.store.apply(crate::state::Event::Inbox(InboxEvent::Page { round: 0, before: None, messages: vec![m] }));
        let item = &a.store.inbox.items[0];
        assert_eq!(item.excerpt, "coucou @Moi");
        a.jump_to(item.guild, item.msg.channel_id, item.msg.id);
        assert_eq!((a.store.guild, a.store.channel), (Some(8), Some(20)));
        assert_eq!(a.list.pending_jump(), Some((20, 7 << 22)));
        frame(&mut a, Vec::new());
        frame(&mut a, Vec::new());
    }
}
