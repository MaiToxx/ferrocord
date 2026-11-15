//! En-tête de la discussion : icône et nom du salon, sujet, boutons (fils,
//! notifications, épingles, membres), recherche. Sorti de `chat.rs` pour que
//! la liste des messages et l'en-tête n'évoluent plus dans le même fichier.
//!
//! Hauteur du jeton `header_height` (48), celle de l'en-tête de la colonne des salons : les deux
//! filets s'alignent. Icônes de 24 séparées de 16, claires au survol et
//! quand leur panneau est ouvert (`docs/ecarts-visuels.md`, 2.6).

use super::settings::NotifTarget;
use super::App;
use crate::i18n::table::chadmin::t as cx;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Color32, Painter, Rect, RichText, Sense, Vec2};

/// Hauteur des en-têtes (discussion, colonne des salons, amis, appel) : le
/// jeton `header_height` (48 dans les thèmes calqués), sinon la ligne et ses marges.
pub(super) fn height(t: &Tokens) -> f32 {
    t.header_height()
}

/// Marge verticale qui donne `height` avec la ligne la plus courante.
fn pad_y(t: &Tokens) -> f32 {
    ((height(t) - t.head_row()) / 2.0).floor().max(0.0)
}

/// Marge d'un en-tête : marge latérale `x`, le reste réparti en haut et en bas.
pub(super) fn head_margin(t: &Tokens, x: f32) -> egui::Margin {
    egui::Margin::symmetric(x.round() as i8, pad_y(t) as i8)
}

/// Hauteur de la ligne à l'intérieur de `head_margin`.
pub(super) fn head_row(t: &Tokens) -> f32 {
    (height(t) - 2.0 * pad_y(t)).max(t.head_row())
}

/// Côté d'une icône de l'en-tête : 24 dans les thèmes calqués (jeton `icon`,
/// tel quel) comme dans Rouille (jeton de 20, agrandi comme avant le calque).
pub(super) fn icon_side(t: &Tokens) -> f32 {
    if t.spacing.header_height > 0.0 { t.typography.icon.round() } else { Tokens::icon_side(t.typography.icon * 1.1) }
}

/// Largeur de la ligne d'en-tête sous laquelle la recherche se replie en
/// loupe : un salon a quatre boutons, une conversation privée cinq (appel,
/// vidéo, épingles, ajout d'amis, profil) et le client y garde un champ court.
const COMPACT: f32 = 470.0;
const COMPACT_DM: f32 = 500.0;

/// Écart entre deux icônes de l'en-tête.
fn icon_gap(t: &Tokens) -> f32 {
    t.spacing.item_x * 2.0
}

/// Bouton-icône de l'en-tête : pas de fond, l'icône s'éclaire au survol, au
/// clavier, et quand ce qu'elle ouvre est affiché (`active`).
pub(super) fn head_icon(ui: &mut egui::Ui, t: &Tokens, tip: &str, active: bool, paint: impl FnOnce(&Painter, Rect, Color32)) -> egui::Response {
    let side = icon_side(t);
    // Disposition mobile : la cible a la taille du doigt, l'icône garde la sienne.
    let hit = if crate::ui::mobile::is_active(ui.ctx()) { side.max(crate::ui::mobile::TOUCH) } else { side };
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(hit), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let color = if active || hot { t.colors.text_strong } else { t.colors.muted };
    paint(ui.painter(), super::util::pixel_square(ui, rect.center() - Vec2::splat(side / 2.0), side), color);
    super::keys::focus_ring(ui, t, &resp);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `head_icon` pour une icône du thème.
pub(super) fn head_icon_drawn(ui: &mut egui::Ui, t: &Tokens, icon: Icon, tip: &str, active: bool) -> egui::Response {
    head_icon(ui, t, tip, active, |p, r, c| icons::paint(icon, p, r, c))
}


/// Identifiants des volets de l'en-tête (sujet, notifications, fils) : fixes,
/// pour savoir s'ils sont ouverts avant de dessiner leur bouton.
fn topic_popup_id() -> egui::Id {
    egui::Id::unique("ferrocord.entete.sujet")
}
fn notif_popup_id() -> egui::Id {
    egui::Id::unique("ferrocord.entete.cloche")
}
pub(super) fn threads_popup_id() -> egui::Id {
    egui::Id::unique("ferrocord.entete.fils")
}
/// Menu du serveur, sous l'en-tête de la colonne des salons (`rail.rs`).
pub(super) fn guild_menu_id() -> egui::Id {
    egui::Id::unique("ferrocord.entete.serveur")
}

/// Première ligne du sujet, sans retour à la ligne : ce que l'en-tête montre.
fn topic_line(topic: &str) -> &str {
    topic.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default()
}

impl App {
    /// `trouble` : texte de l'état de la connexion, quand il y a quelque chose à signaler.
    pub(super) fn chat_head(&mut self, ui: &mut egui::Ui, cid: u64, name: &str, is_thread: bool, is_dm: bool, trouble: Option<&str>) {
        let t = self.theme.clone();
        let guild = if is_dm { None } else { self.store.guild };
        // Sujet du salon ; dans un fil, celui du salon parent n'est pas montré.
        let topic = if is_dm || is_thread { None } else { self.store.threads.topic(cid).map(str::to_string) };
        egui::Panel::top("chat_head")
            .frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(head_margin(&t, t.spacing.chat_head_x)))
            .show(ui, |ui| {
                super::look::backdrop(ui, super::look::Zone::Chat);
                ui.horizontal(|ui| {
                    ui.set_min_height(head_row(&t));
                    // Disposition mobile : retour à la liste (phase 7).
                    self.mobile_back(ui);
                    let side = icon_side(&t);
                    // MP : petit avatar du correspondant et sa pastille de statut (l'icône du
                    // groupe pour un groupe), comme dans le client ; « @ » si le thème masque les avatars.
                    let dm_avatar = if is_dm && t.avatars_shown() { self.store.dms.iter().find(|d| d.id == cid).map(|d| d.avatar) } else { None };
                    if let Some(avatar) = dm_avatar {
                        let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                        let r = super::util::pixel_square(ui, slot.min, side);
                        super::util::paint_avatar(ui, &self.images, &t, r, &avatar, name);
                        super::profile::dm_dot(ui, &t, r, cid, t.colors.bg2);
                    } else if is_dm {
                        ui.label(RichText::new("@").size(side).color(t.colors.faint));
                    } else {
                        let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                        let r = super::util::pixel_square(ui, slot.min, side);
                        match self.store.voice.channel(cid).map(|c| c.stage) {
                            // Discussion d'un salon vocal : haut-parleur (conférence : porte-voix).
                            Some(false) => icons::paint(Icon::Speaker, ui.painter(), r, t.colors.faint),
                            Some(true) => icons::paint(Icon::Megaphone, ui.painter(), r, t.colors.faint),
                            None => {
                                let icon = if is_thread { Icon::Thread } else if self.store.is_announcement(cid) { Icon::Megaphone } else { Icon::Hash };
                                icons::paint(icon, ui.painter(), r, t.colors.faint);
                            }
                        }
                    }
                    // Les boutons d'abord, calés à droite ; le titre puis le sujet prennent
                    // la place qui reste et se tronquent : rien ne se chevauche, même dans
                    // une fenêtre étroite (les boutons étaient dessinés sur la fin du titre).
                    let mobile = self.mobile_active();
                    let rest = ui.available_rect_before_wrap();
                    let row = Rect::from_min_size(rest.min, egui::vec2(rest.width().max(0.0), head_row(&t)));
                    // En-tête serré : la recherche se replie en loupe ; ouverte, elle prend
                    // la place des boutons.
                    let compact = !mobile && row.width() < if is_dm { COMPACT_DM } else { COMPACT };
                    let searching = compact && self.search_wanted(ui.ctx());
                    let mut right = ui.new_child(egui::UiBuilder::new().max_rect(row).layout(egui::Layout::right_to_left(egui::Align::Center)));
                    {
                        let ui = &mut right;
                        ui.spacing_mut().item_spacing.x = icon_gap(&t);
                        if compact && !searching {
                            // Loupe : un clic déplie le champ à la place des boutons.
                            if self.store.search_scope().is_some() && head_icon_drawn(ui, &t, Icon::Search, self.search_tip(), false).clicked() {
                                self.search_expand();
                            }
                        } else {
                            // Champ de recherche, à droite comme dans Discord (lot T12).
                            self.search_field(ui, if searching { row.width() * 2.0 } else { row.width() });
                        }
                        if !searching {
                            // Boîte de réception : un onglet sur mobile ; dans la barre de titre quand elle est intégrée.
                            if !mobile && !super::titlebar::shown(ui.ctx()) {
                                self.inbox_button(ui);
                            }
                            // Serveur ou MP de groupe : un seul bouton, un seul panneau.
                            self.members_toggle(ui);
                            // MP : profil du correspondant, « Ajouter des amis au MP » (lot U).
                            if is_dm {
                                self.dm_head_buttons(ui, cid);
                            }
                            // Mode discret, lecture au clic : « Marquer comme lu » (lot E3).
                            self.stealth_read_button(ui, cid);
                            // Épingles du salon (lot T6).
                            self.pins_button(ui, cid);
                            if let Some(g) = guild {
                                // Sur mobile, les notifications restent dans le menu de la ligne du salon.
                                if !mobile {
                                    self.notif_button(ui, g, cid);
                                }
                                self.threads_button(ui, cid);
                            }
                            // Salon d'annonces : « Suivre » (lot T13).
                            self.follow_button(ui, cid);
                            // MP ou groupe : « Démarrer un appel » (lot V6).
                            if is_dm {
                                self.dm_video_button(ui, cid);
                                self.call_button(ui, cid);
                            }
                            if let Some(text) = trouble.filter(|_| !compact) {
                                ui.label(RichText::new(text).small().color(t.colors.danger));
                            }
                        }
                    }
                    // Ce qui reste à gauche des boutons : le titre, puis le sujet.
                    let edge = (right.min_rect().left() - t.spacing.item_x).max(row.left());
                    let mut left = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(row.min, egui::pos2(edge, row.bottom()))).layout(egui::Layout::left_to_right(egui::Align::Center)));
                    {
                        let ui = &mut left;
                        let typing = !mobile && is_dm && self.store.typing.active(cid, self.store.now()).next().is_some();
                        let dots = egui::vec2(side * 1.4, side * 0.7);
                        let title = RichText::new(name).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong);
                        // Le sujet ne se montre que s'il lui reste de quoi se lire après le titre.
                        let title_w = ui.painter().layout_no_wrap(name.to_owned(), egui::FontId::new(t.typography.title, crate::ui::fonts::demi()), t.colors.text_strong).size().x;
                        let room = ui.available_width() - if typing { dots.x + t.spacing.item_x } else { 0.0 };
                        let topic_shown = topic.as_deref().filter(|_| !mobile && room - title_w > side * 4.0);
                        let folded = topic.as_deref().filter(|_| topic_shown.is_none());
                        let label = egui::Label::new(title).truncate().selectable(false);
                        let resp = ui.allocate_ui_with_layout(egui::vec2(room.max(0.0), row.height()), egui::Layout::left_to_right(egui::Align::Center), |ui| match folded {
                            // Pas de place pour le sujet : un clic sur le titre l'affiche en entier.
                            Some(_) => {
                                let r = ui.add(label.sense(Sense::click()));
                                let tip = cx::TOPIC_TIP.get(self.lang);
                                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
                                super::keys::focus_ring(ui, &t, &r);
                                Some(r.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand))
                            }
                            None => {
                                ui.add(label);
                                None
                            }
                        });
                        if let (Some(r), Some(topic)) = (resp.inner, folded) {
                            self.topic_popup(&r, &t, cid, guild, name, topic);
                        }
                        if typing {
                            let (at, _) = ui.allocate_exact_size(dots, Sense::hover());
                            super::motion::dots(ui, at, t.colors.online);
                        }
                        if let Some(topic) = topic_shown {
                            self.topic_ui(ui, &t, cid, guild, name, topic);
                        }
                    }
                    ui.allocate_rect(row, Sense::hover());
                });
                // Fil ouvert : parent, état, rejoindre ou quitter, archiver (lot T10).
                if is_thread {
                    self.thread_head(ui, cid);
                }
            });
    }

    /// Filet vertical puis sujet sur une ligne tronquée ; un clic l'affiche en entier.
    fn topic_ui(&self, ui: &mut egui::Ui, t: &Tokens, cid: u64, guild: Option<u64>, name: &str, topic: &str) {
        let lang = self.lang;
        ui.spacing_mut().item_spacing.x = t.spacing.item_x;
        let (rule, _) = ui.allocate_exact_size(egui::vec2(t.shapes.border.max(1.0), icon_side(t)), Sense::hover());
        ui.painter().rect_filled(rule, 0.0, t.colors.border);
        let text = RichText::new(topic_line(topic)).size(t.typography.body - 1.0).color(t.colors.muted);
        let resp = ui.add(egui::Label::new(text).truncate().selectable(false).sense(Sense::click()));
        let tip = cx::TOPIC_TIP.get(lang);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        super::keys::focus_ring(ui, t, &resp);
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        self.topic_popup(&resp, t, cid, guild, name, topic);
    }

    /// Volet du sujet en entier, ouvert par un clic sur `resp` (le sujet, ou le
    /// titre quand le sujet n'a pas la place de se montrer).
    fn topic_popup(&self, resp: &egui::Response, t: &Tokens, cid: u64, guild: Option<u64>, name: &str, topic: &str) {
        let width = crate::ui::mobile::card_width(&resp.ctx, t, 1.2);
        egui::Popup::from_toggle_button_response(resp).id(topic_popup_id()).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).frame(t.window_frame()).width(width).show(|ui| {
            ui.set_width(width);
            ui.label(RichText::new(format!("#{name}")).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
            ui.add_space(t.spacing.item_y);
            // Dans un volet, la hauteur disponible est celle de l'image précédente : sans
            // hauteur minimale, la zone restait à 64 points et coupait le sujet.
            let max_h = ui.ctx().content_rect().height() * 0.5;
            egui::ScrollArea::vertical().max_height(max_h).min_scrolled_height(max_h).show(ui, |ui| {
                // Même mise en forme qu'une bio (liens, mentions, emojis), analysée une fois par texte.
                self.bio_ui(ui, t, cid, guild, topic);
            });
        });
    }

    /// Cloche : le menu des notifications du salon (muet, niveau), comme le clic droit sur sa ligne.
    fn notif_button(&mut self, ui: &mut egui::Ui, guild: u64, cid: u64) {
        // Dans un fil, les réglages sont ceux de son salon.
        let channel = self.store.threads.get(cid).map_or(cid, |th| th.parent_id);
        let t = self.theme.clone();
        let muted = self.store.account.notifs.by_guild.get(&Some(guild)).is_some_and(|n| n.channels.get(&channel).is_some_and(|c| c.mute.active(self.store.now())));
        let id = notif_popup_id();
        let open = egui::Popup::is_id_open(ui.ctx(), id);
        let resp = head_icon_drawn(ui, &t, if muted { Icon::BellOff } else { Icon::Bell }, cx::NOTIF_TIP.get(self.lang), open);
        egui::Popup::menu(&resp).id(id).show(|ui| self.notif_menu(ui, NotifTarget::Channel { guild: Some(guild), channel }));
    }
}

impl App {
    /// Ouvre un volet de l'en-tête (aperçu, tests) : « sujet », « cloche »,
    /// « fils » ou « archives » (le panneau des fils sur ses fils archivés),
    /// « serveur » (le menu du serveur).
    pub fn show_head_popup(&mut self, which: &str) -> bool {
        let id = match which {
            "sujet" => topic_popup_id(),
            "cloche" => notif_popup_id(),
            "serveur" => guild_menu_id(),
            "fils" | "archives" => {
                self.threads.set_panel_archived(which == "archives");
                threads_popup_id()
            }
            _ => return false,
        };
        egui::Popup::open_id(&self.ctx, id);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_is_48_high() {
        let t = Tokens::default();
        assert_eq!(head_row(&t) + 2.0 * f32::from(head_margin(&t, t.spacing.chat_head_x).top), 48.0);
        assert_eq!(icon_side(&t), 24.0);
        assert_eq!(icon_side(&crate::theme::Preset::Rouille.tokens()), 24.0);
        assert_eq!(icon_gap(&t), 16.0);
    }

    #[test]
    fn topic_shows_its_first_line() {
        assert_eq!(topic_line("\n  Règles : soyez courtois.  \nSuite"), "Règles : soyez courtois.");
        assert_eq!(topic_line(" \n "), "");
    }
}
