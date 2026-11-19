//! Rail des serveurs : colonne de pastilles, ou liste avec les noms ;
//! dossiers de serveurs (repliés avec leurs mini-icônes, ou dépliés), ordre
//! réglable par glisser-déposer (`rail_dnd.rs`), bouton « Ajouter un serveur ».

use super::{
    guild_create::{color_of, folder_key, FolderEdit, Page},
    rail_dnd::{Dragged, Spot},
    settings::NotifTarget,
    util::{initials, paint_image, paint_initials},
    App,
};
use crate::i18n::table::guilds::t as gx;
use crate::i18n::t;
use crate::state::guild_folders::{Edit, Item};
use crate::state::model::ImageRef;
use crate::theme::{icons, icons::Icon, RailStyle};
use eframe::egui::{self, Align, Color32, CornerRadius, Layout, Rect, RichText, Stroke, Vec2};

/// Choix d'un menu du clic droit, appliqué après la liste.
enum Pick {
    MarkRead(Vec<u64>),
    /// « Tout marquer comme lu » : tous les serveurs et les conversations.
    MarkAll,
    Invite(u64),
    Invites(u64),
    Leave(u64),
    FolderSettings(FolderEdit),
    Ungroup(usize),
    /// Fenêtre « Paramètres du serveur » (phase 3).
    GuildSettings(u64),
    /// Fenêtre « Règles et vérification » (lot ND).
    Rules(u64),
}

/// Menu du clic droit à ouvrir après la liste.
enum Menu {
    Dm(egui::Response),
    Guild(u64, egui::Response),
    Folder { entry: usize, guilds: Vec<u64>, name: Option<String>, color: Option<u64>, resp: egui::Response },
}

/// Ce que montre une pastille du rail.
#[derive(Clone, Copy)]
enum Tile<'a> {
    /// Messages privés : une bulle (son libellé sert au rail en liste).
    Home(&'a str),
    /// Serveur : initiales, et icône dessinée par-dessus dès qu'elle est prête.
    Guild(&'a str, Option<ImageRef>),
}

/// Durée du déroulement d'un dossier (ouverture et fermeture).
const FOLD_SECS: f32 = 0.18;
/// Côté d'un serveur rangé dans un dossier déplié, en part de la pastille (40 pour 48).
const FOLDER_TILE: f32 = 40.0 / 48.0;

/// Part de la couleur choisie par l'utilisateur dans le fond sombre d'un dossier :
/// très atténuée, elle reste reconnaissable sans écraser les icônes.
const FOLDER_TINT: f32 = 0.22;

/// Hauteur de l'indicateur du rail, en part de la pastille : 40, 20 et 8
/// pour une pastille de 48 (entrée ouverte, survolée, non lue).
const MARK_OPEN: f32 = 40.0 / 48.0;
const MARK_HOT: f32 = 20.0 / 48.0;
const MARK_UNREAD: f32 = 8.0 / 48.0;
/// Pastilles gardées prêtes au-dessus et au-dessous de la vue du rail (en hauteurs
/// de pastille) : leur icône est déjà chargée quand le défilement les amène.
const RAIL_MARGIN: f32 = 4.0;

fn scaled(r: CornerRadius, k: f32) -> CornerRadius {
    let s = |v: u8| (f32::from(v) * k).round().clamp(0.0, 255.0) as u8;
    CornerRadius { nw: s(r.nw), ne: s(r.ne), sw: s(r.sw), se: s(r.se) }
}

impl App {
    pub(super) fn guild_rail(&mut self, ui: &mut egui::Ui) {
        let list = self.theme.layout.rail_style == RailStyle::List;
        self.servers.cache.refresh(&self.store.account.snapshot().folders, &self.store.guilds);
        // Amis en vocal par serveur (pastilles et infobulles), refaits quand quelque chose change.
        self.servers.voice.refresh(&self.store);
        // Le bas du rail reste libre : la carte du panneau utilisateur y passe (`channels.rs`).
        let foot = if self.mobile_active() { 0.0 } else { (self.place.foot - self.theme.spacing.rail_y).max(0.0) };
        let room = (ui.available_height() - foot).max(0.0);
        egui::ScrollArea::vertical().max_height(room).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
            let body = |ui: &mut egui::Ui| {
                let s = &self.store;
                let dm_unread = s.dms.iter().any(|d| s.unread().contains(&d.id));
                let lang = self.lang;
                let dm_pill = self.rail_item(ui, Tile::Home(t::DM_PILL.get(lang)), t::DIRECT_MESSAGES.get(lang), s.guild.is_none(), dm_unread, dm_unread);
                // Menus du clic droit, ouverts après la liste : « Marquer le serveur
                // comme lu » (lot S3 + T9), actions du serveur (lot A1), puis
                // notifications (lot C1).
                let mut menus = vec![Menu::Dm(dm_pill.clone())];
                if dm_pill.clicked() {
                    self.store.select_dms();
                }
                self.rail_divider(ui);
                let mut clicked = None;
                let mut toggled = None;
                let mut drag = None;
                let mut hits: Vec<(Rect, Spot)> = Vec::new();
                let items = std::mem::take(&mut self.servers.cache.items);
                for item in &items {
                    match item {
                        Item::Guild { guild, entry } => {
                            let Some((resp, shown)) = self.guild_entry(ui, *guild, None) else { continue };
                            hits.push((resp.rect, Spot::Top { entry: *entry, guild: Some(*guild), open: false }));
                            if resp.drag_started() {
                                drag = Some(Dragged::Guild(*guild));
                            }
                            if resp.clicked() {
                                clicked = Some(*guild);
                            }
                            if shown {
                                menus.push(Menu::Guild(*guild, resp));
                            }
                        }
                        Item::Folder { entry, id, name, color, guilds } => {
                            let key = folder_key(*id, guilds);
                            let open = self.servers.open_folders.contains(&key);
                            let tint = color.map(color_of);
                            let shade = tint.unwrap_or(self.theme.colors.accent);
                            let ids: Vec<u64> = guilds.iter().map(|g| g.0).collect();
                            // Ouverture et fermeture : les serveurs se déroulent vers le bas en
                            // glissant, puis se replient. L'animation s'arrête seule (rien n'est
                            // redessiné au repos) et le réglage « réduire les animations » la coupe.
                            let k = super::motion::toward(ui.ctx(), egui::Id::unique(("dossier-du-rail", key)), open, FOLD_SECS);
                            let shown = k > 0.0;
                            // Fond du dossier déplié, posé sous ses serveurs une fois leur place connue.
                            let bg = shown.then(|| ui.painter().add(egui::Shape::Noop));
                            let resp = self.folder_entry(ui, name.as_deref(), shade, self.folder_fill(tint), &ids, shown);
                            hits.push((resp.rect, Spot::Top { entry: *entry, guild: None, open }));
                            if resp.drag_started() {
                                drag = Some(Dragged::Folder(*entry));
                            }
                            if resp.clicked() {
                                toggled = Some(key);
                            }
                            let mut area = resp.rect;
                            menus.push(Menu::Folder { entry: *entry, guilds: ids, name: name.clone(), color: *color, resp });
                            if !shown {
                                continue;
                            }
                            // Hauteur des serveurs dépliés : connue d'avance (lignes de hauteur fixe).
                            let gap = ui.spacing().item_spacing.y;
                            let n = guilds.iter().filter(|(g, _)| self.store.guilds.iter().any(|x| x.id == *g)).count() as f32;
                            let full = (n * (self.folder_slot() + gap) - gap).max(0.0);
                            // L'écart sous le dossier grandit avec le reste : pas de saut au départ ni à l'arrivée.
                            ui.add_space(-gap * (1.0 - k));
                            let (region, _) = ui.allocate_exact_size(egui::vec2(if list { ui.available_width() } else { self.theme.layout.rail_button }, full * k), egui::Sense::hover());
                            // Les serveurs glissent depuis le dessous du dossier : la liste entière
                            // est calée sur le bas de la zone visible, qui la découpe.
                            let whole = Rect::from_min_size(egui::pos2(region.left(), region.bottom() - full), egui::vec2(region.width(), full));
                            let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(whole).layout(*ui.layout()));
                            inner.set_clip_rect(Rect::from_min_max(egui::pos2(ui.clip_rect().left(), region.top()), egui::pos2(ui.clip_rect().right(), region.bottom())).intersect(ui.clip_rect()));
                            for (g, at) in guilds {
                                let Some((resp, drawn)) = self.guild_entry(&mut inner, *g, Some(key)) else { continue };
                                // En mouvement, rien ne se dépose ni ne s'ouvre sur une ligne à moitié sortie.
                                if k < 1.0 {
                                    continue;
                                }
                                hits.push((resp.rect, Spot::Inside { entry: *entry, at: *at, guild: *g }));
                                if resp.drag_started() {
                                    drag = Some(Dragged::Guild(*g));
                                }
                                if resp.clicked() {
                                    clicked = Some(*g);
                                }
                                if drawn {
                                    menus.push(Menu::Guild(*g, resp));
                                }
                            }
                            area = area.union(region);
                            if let Some(idx) = bg {
                                // Capsule gris neutre à fine bordure, comme dans le client.
                                let t = &self.theme;
                                let fill = self.folder_fill(tint);
                                let capsule = area.expand2(egui::vec2(0.0, (t.spacing.rail_gap * 0.25).round()));
                                ui.painter().set(idx, egui::epaint::RectShape::new(capsule, self.folder_radius(), fill, self.folder_stroke(), egui::StrokeKind::Inside));
                            }
                        }
                    }
                }
                self.servers.cache.items = items;
                ui.add_space(self.theme.spacing.rail_gap);
                self.add_server_button(ui);
                ui.add_space(self.theme.spacing.rail_gap);
                self.discover_button(ui);

                if let Some(d) = drag {
                    self.servers.dnd.start(d);
                }
                let entries = self.servers.cache.entries;
                let theme = self.theme.clone();
                if let Some(edit) = self.servers.dnd.finish(ui, &theme, &hits, entries) {
                    self.write_folders(edit);
                }

                let mut pick = None;
                // Événements, accueil, paramètres du serveur (lot A7).
                let mut admin = None;
                for m in menus {
                    match m {
                        Menu::Dm(resp) => {
                            resp.context_menu(|ui| {
                                if self.mark_all_entry(ui) {
                                    pick = Some(Pick::MarkAll);
                                }
                                crate::ui::menu::separator(ui, &self.theme);
                                self.notif_menu(ui, NotifTarget::Guild(None));
                            });
                        }
                        Menu::Guild(id, resp) => {
                            resp.context_menu(|ui| {
                                if self.guild_menu(ui) {
                                    pick = Some(Pick::MarkRead(vec![id]));
                                }
                                if self.mark_all_entry(ui) {
                                    pick = Some(Pick::MarkAll);
                                }
                                crate::ui::menu::separator(ui, &self.theme);
                                if let Some(p) = self.server_menu(ui, id) {
                                    pick = Some(p);
                                }
                                if let Some(p) = self.admin_menu(ui, id) {
                                    admin = Some((id, p));
                                }
                                self.notif_menu(ui, NotifTarget::Guild(Some(id)));
                            });
                        }
                        Menu::Folder { entry, guilds, name, color, resp } => {
                            resp.context_menu(|ui| {
                                let lang = self.lang;
                                if crate::ui::menu::item(ui, &self.theme, gx::MARK_FOLDER_READ.get(lang), Some(crate::theme::icons::Icon::Check), false).clicked() {
                                    pick = Some(Pick::MarkRead(guilds.clone()));
                                    ui.close();
                                }
                                if self.mark_all_entry(ui) {
                                    pick = Some(Pick::MarkAll);
                                }
                                crate::ui::menu::separator(ui, &self.theme);
                                if crate::ui::menu::item(ui, &self.theme, gx::FOLDER_SETTINGS.get(lang), Some(crate::theme::icons::Icon::Gear), false).clicked() {
                                    pick = Some(Pick::FolderSettings(FolderEdit { entry, name: name.clone().unwrap_or_default(), color }));
                                    ui.close();
                                }
                                if crate::ui::menu::item(ui, &self.theme, gx::FOLDER_UNGROUP.get(lang), Some(crate::theme::icons::Icon::Folder), false).clicked() {
                                    pick = Some(Pick::Ungroup(entry));
                                    ui.close();
                                }
                            });
                        }
                    }
                }
                match pick {
                    Some(Pick::MarkRead(ids)) => self.store.mark_guilds_read(&ids),
            Some(Pick::MarkAll) => self.store.mark_all_read(),
                    Some(Pick::Invite(id)) => self.open_invite(id),
                    Some(Pick::Invites(id)) => self.open_invites(id),
                    Some(Pick::Leave(id)) => self.servers.leave = Some(id),
                    Some(Pick::FolderSettings(fe)) => self.servers.folder_edit = Some(fe),
                    Some(Pick::Ungroup(entry)) => self.write_folders(Edit::Ungroup { entry }),
                    Some(Pick::GuildSettings(id)) => self.open_guild_settings(id, None),
                    Some(Pick::Rules(id)) => self.show_verification(id),
                    None => {}
                }
                if let Some((id, p)) = admin {
                    self.apply_events_pick(id, p);
                }
                if let Some(key) = toggled
                    && !self.servers.open_folders.remove(&key)
                {
                    self.servers.open_folders.insert(key);
                }
                if let Some(id) = clicked
                    && self.store.select_guild(id)
                {
                    self.scroll_to_bottom = true;
                }
            };
            if list {
                ui.with_layout(Layout::top_down_justified(Align::Min), body);
            } else {
                ui.vertical_centered(body);
            }
        });
    }

    /// Menu du serveur ouvert, sous l'en-tête de la colonne des salons.
    pub(super) fn guild_header_menu(&mut self, head: &egui::Response, id: u64) {
        let mut pick = None;
        let mut more = None;
        egui::Popup::menu(head).id(super::chat_head::guild_menu_id()).show(|ui| {
            pick = self.server_menu(ui, id);
            // Créer un salon, une catégorie, un événement, profil de serveur (lot FF2, F15).
            more = self.server_menu_more(ui, id);
            // « Masquer les salons muets » (phase 10, lot S : `channels.rs`).
            self.hide_muted_entry(ui, id);
            if self.guild_menu(ui) {
                pick = Some(Pick::MarkRead(vec![id]));
            }
        });
        if let Some(m) = more {
            self.apply_server_more(id, m);
        }
        match pick {
            Some(Pick::MarkRead(ids)) => self.store.mark_guilds_read(&ids),
            Some(Pick::MarkAll) => self.store.mark_all_read(),
            Some(Pick::Invite(id)) => self.open_invite(id),
            Some(Pick::Invites(id)) => self.open_invites(id),
            Some(Pick::Leave(id)) => self.servers.leave = Some(id),
            Some(Pick::GuildSettings(id)) => self.open_guild_settings(id, None),
            Some(Pick::Rules(id)) => self.show_verification(id),
            _ => {}
        }
    }

    /// Entrées du menu d'un serveur ajoutées par le lot A1 (inviter,
    /// invitations, quitter), chacune seulement si elle est possible.
    fn server_menu(&self, ui: &mut egui::Ui, id: u64) -> Option<Pick> {
        let lang = self.lang;
        let mut pick = None;
        let mut any = false;
        if !self.store.invite_channels(id).is_empty() {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, gx::INVITE_PEOPLE.get(lang), Some(crate::theme::icons::Icon::FriendAdd), false).clicked() {
                pick = Some(Pick::Invite(id));
            }
        }
        if self.store.can_open_guild_settings(id) {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, crate::i18n::table::admin::t::MENU.get(lang), Some(crate::theme::icons::Icon::Gear), false).clicked() {
                pick = Some(Pick::GuildSettings(id));
            }
        }
        if self.store.can_manage_guild(id) {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, gx::MANAGE_INVITES.get(lang), Some(crate::theme::icons::Icon::Link), false).clicked() {
                pick = Some(Pick::Invites(id));
            }
        }
        if !self.store.owns_guild(id) {
            any = true;
            if crate::ui::menu::item(ui, &self.theme, gx::LEAVE_SERVER.get(lang), Some(crate::theme::icons::Icon::Logout), true).clicked() {
                pick = Some(Pick::Leave(id));
            }
        }
        if any {
            crate::ui::menu::separator(ui, &self.theme);
        }
        // Règles et vérification des membres (lot ND), lues au clic.
        if self.verification_entry(ui) {
            pick = Some(Pick::Rules(id));
        }
        if pick.is_some() {
            ui.close();
        }
        pick
    }

    /// Un serveur du rail (dans le dossier `folder` déplié, s'il y en a un) ;
    /// on peut le saisir pour le déplacer.
    ///
    /// Renvoie aussi `false` pour une pastille loin de la vue (lot C10) : elle
    /// garde sa place, son identifiant et son nom (clavier, lecteurs d'écran,
    /// glisser-déposer), mais rien n'est calculé ni dessiné pour elle (non-lus,
    /// icône, marque, infobulle, menu) : avec cent serveurs, le rail coûtait
    /// autant que toute la liste des messages à chaque image.
    fn guild_entry(&self, ui: &mut egui::Ui, id: u64, folder: Option<u64>) -> Option<(egui::Response, bool)> {
        let g = self.store.guilds.iter().find(|g| g.id == id)?;
        let name = super::words::guild_name(self.lang, &g.name);
        let selected = self.store.guild == Some(g.id);
        let pills = self.theme.layout.rail_style == RailStyle::Pills;
        // Pastilles : la place d'abord, pour savoir si elle est près de la vue. Dans un
        // dossier déplié, les icônes sont un peu plus petites et se suivent de plus près.
        let side = self.theme.layout.rail_button;
        let slot = pills.then(|| ui.allocate_exact_size(egui::vec2(side, if folder.is_some() { self.folder_slot() } else { side }), egui::Sense::click()));
        if let Some((rect, resp)) = &slot
            && !ui.is_rect_visible(rect.expand2(egui::vec2(0.0, RAIL_MARGIN * rect.height())))
            && !resp.has_focus()
            && !resp.context_menu_opened()
        {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
            return Some((resp.interact(egui::Sense::drag()), false));
        }
        let (unread, ping) = self.store.guild_has_unread(g);
        // Serveur sans icône : son acronyme, comme dans le client (rail en liste : ses initiales, la pastille y est petite).
        let label = if pills { super::util::guild_acronym(name) } else { initials(name) };
        let tile = Tile::Guild(&label, g.icon);
        let mut icon = None;
        let resp = match (slot, folder) {
            (Some((rect, resp)), _) => {
                let square = Rect::from_center_size(rect.center(), Vec2::splat(rect.height().min(rect.width())));
                icon = Some(square);
                let r = self.rail_button_in(ui, square, resp, tile, selected, unread, ping);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
                r
            }
            (None, Some(key)) => ui.indent(("dossier", key, id), |ui| self.rail_item(ui, tile, name, selected, unread, ping)).inner,
            (None, None) => self.rail_item(ui, tile, name, selected, unread, ping),
        };
        if let Some(square) = icon {
            // Amis en vocal sur ce serveur : pastille en haut à droite, détail dans l'infobulle.
            let live = self.servers.voice.get(g.id);
            if let Some(live) = live {
                self.rail_live_badge(ui, square, live);
            }
            self.mention_badge(ui, square, self.store.guild_mentions(g.id));
            self.rail_tip(&resp, name, live);
        }
        Some((resp.interact(egui::Sense::drag()), true))
    }

    /// Hauteur de la place d'un serveur dans un dossier déplié.
    fn folder_slot(&self) -> f32 {
        let t = &self.theme;
        match t.layout.rail_style {
            RailStyle::Pills => (t.layout.rail_button * FOLDER_TILE).round(),
            RailStyle::List => t.layout.rail_list_row,
        }
    }

    /// Coins de la capsule d'un dossier (déplié, ou replié avec ses mini-icônes) : très arrondis.
    fn folder_radius(&self) -> CornerRadius {
        let r = self.theme.r_rail(false);
        scaled(r, 1.15)
    }

    /// Fine bordure d'un dossier : un voile de la couleur du texte, à peine visible.
    fn folder_stroke(&self) -> Stroke {
        Stroke::new(1.0, self.theme.colors.text_strong.gamma_multiply(0.14))
    }

    /// Un dossier : replié (mini-icônes de ses serveurs) ou déplié (icône de
    /// dossier), à la couleur du dossier ; pastilles cumulées de ses serveurs.
    fn folder_entry(&self, ui: &mut egui::Ui, name: Option<&str>, shade: Color32, fill: Color32, guilds: &[u64], open: bool) -> egui::Response {
        let s = &self.store;
        let (mut unread, mut ping, mut mentions) = (false, false, 0u32);
        for g in s.guilds.iter().filter(|g| guilds.contains(&g.id)) {
            let (u, p) = s.guild_has_unread(g);
            unread |= u;
            ping |= p;
            mentions += s.guild_mentions(g.id);
        }
        let holds_open = s.guild.is_some_and(|g| guilds.contains(&g));
        let lang = self.lang;
        let title = name.map(str::to_string).unwrap_or_else(|| {
            let names: Vec<&str> = s.guilds.iter().filter(|g| guilds.contains(&g.id)).map(|g| super::words::guild_name(lang, &g.name)).collect();
            names.join(", ")
        });
        let t = &self.theme;
        let resp = match t.layout.rail_style {
            RailStyle::Pills => {
                let side = t.layout.rail_button;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::click_and_drag());
                let hot = resp.hovered() || resp.has_focus();
                let r = self.folder_radius();
                if open {
                    // Déplié : l'icône de dossier seule, en tête de la capsule (son fond est celui de la capsule).
                    if hot {
                        ui.painter().rect_filled(rect, r, t.colors.text_strong.gamma_multiply(0.06));
                    }
                    icons::paint(Icon::Folder, ui.painter(), rect.shrink(side * 0.27), shade);
                } else {
                    // Replié : tuile arrondie à fine bordure, quatre mini-icônes.
                    let fill = if hot { super::motion::mix(fill, t.colors.selected, 0.5) } else { fill };
                    ui.painter().rect(rect, r, fill, self.folder_stroke(), egui::StrokeKind::Inside);
                    self.mini_icons(ui, rect, guilds);
                }
                self.rail_mark(ui, resp.id, rect, holds_open && !open, hot, unread && !open);
                if !open {
                    if ping && mentions == 0 {
                        self.ping_dot(ui, rect);
                    }
                    self.mention_badge(ui, rect, mentions);
                }
                self.rail_tip(&resp, &title, None);
                resp
            }
            RailStyle::List => {
                let h = t.layout.rail_list_row;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click_and_drag());
                let fill = if resp.hovered() { t.colors.row_hover } else { t.colors.bg0 };
                ui.painter().rect(rect, t.r_rail(true), fill, self.ring(false, unread && !open, ping), egui::StrokeKind::Inside);
                let geo = t.rail_row();
                let badge = Rect::from_min_size(rect.left_top() + Vec2::splat(geo.pad), Vec2::splat(geo.badge));
                icons::paint(Icon::Folder, ui.painter(), badge.shrink(geo.badge * 0.15), shade);
                let left = badge.right() + geo.pad;
                let text_rect = Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2((rect.right() - geo.pad).max(left), rect.bottom()));
                let color = if unread { t.colors.text_strong } else { t.colors.muted };
                let mut text = RichText::new(&title).color(color);
                if unread {
                    text = text.family(crate::ui::fonts::demi()).strong();
                }
                let row = egui::UiBuilder::new().max_rect(text_rect).layout(Layout::left_to_right(Align::Center));
                ui.scope_builder(row, |ui| ui.add(egui::Label::new(text).truncate().selectable(false)));
                if !open {
                    self.mention_badge(ui, badge, mentions);
                }
                resp
            }
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &title));
        super::keys::focus_ring(ui, t, &resp);
        resp
    }

    /// Fond d'un dossier (replié, ou derrière ses serveurs dépliés) : gris
    /// neutre et sombre, comme dans le client (la clarté des pastilles, sans
    /// leur teinte) ; `tint` (la couleur du dossier, s'il en a une) n'y entre
    /// que très atténuée.
    fn folder_fill(&self, tint: Option<Color32>) -> Color32 {
        // À mi-chemin entre le fond du rail et une pastille : les tuiles sans icône s'y détachent.
        let luma = |c: Color32| 0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b());
        let c = &self.theme.colors;
        let pill = Color32::from_gray(((luma(c.bg0) + luma(c.pill)) / 2.0).round() as u8);
        tint.map_or(pill, |c| super::motion::mix(pill, c, FOLDER_TINT))
    }

    /// Quatre premiers serveurs d'un dossier replié, en grille 2 × 2.
    fn mini_icons(&self, ui: &egui::Ui, rect: Rect, guilds: &[u64]) {
        let t = &self.theme;
        let side = rect.width();
        let gap = (side * 0.11).round();
        let mini = (side - 3.0 * gap) / 2.0;
        let r = CornerRadius::same((mini * 0.32).round() as u8);
        let shown = guilds.iter().filter_map(|id| self.store.guilds.iter().find(|g| g.id == *id)).take(4);
        for (i, g) in shown.enumerate() {
            let (cx, cy) = ((i % 2) as f32, (i / 2) as f32);
            let cell = Rect::from_min_size(rect.min + egui::vec2(gap + cx * (mini + gap), gap + cy * (mini + gap)), Vec2::splat(mini));
            if !paint_image(ui, &self.images, cell, g.icon, r) {
                paint_initials(ui, t, cell, (t.colors.selected, t.colors.text_strong), super::words::guild_name(self.lang, &g.name));
            }
        }
    }

    /// Nombre de salons mentionnés, en pastille au coin bas droit de `rect`.
    fn mention_badge(&self, ui: &egui::Ui, rect: Rect, n: u32) {
        if n == 0 {
            return;
        }
        let t = &self.theme;
        let text = if n > 99 { "99+".to_string() } else { n.to_string() };
        let galley = ui.painter().layout_no_wrap(text, super::fonts::demi_font(t.typography.small), t.colors.on_accent);
        // 16 de haut pour un texte de 12, large d'autant au moins.
        let h = (t.typography.small * 4.0 / 3.0).round().max(t.shapes.mention_dot * 2.0);
        let w = (galley.size().x + h * 0.5).round().max(h);
        // À cheval sur le coin, comme dans Discord : posée dedans, elle rognait la dernière initiale.
        let out = (h / 4.0).round();
        let pill = Rect::from_min_size(rect.right_bottom() - egui::vec2(w - out, h - out), egui::vec2(w, h));
        // Compte qui change : la pastille fait un « pop ».
        let k = super::motion::watch(ui.ctx(), egui::Id::unique(("rail-mention", rect.top().to_bits())), u64::from(n), super::motion::dur::POP);
        let span = super::motion::Span::begin(ui);
        let p = ui.painter();
        // Anneau de la couleur du rail : la pastille se détache de l'icône.
        p.rect(pill, CornerRadius::same((h / 2.0).round() as u8), t.danger_fill(), Stroke::new(self.badge_ring(), t.colors.bg0), egui::StrokeKind::Outside);
        p.galley(pill.center() - galley.size() / 2.0, galley, t.colors.on_accent);
        span.end(ui, 1.0, super::motion::pop_scale(ui.ctx(), pill.center(), k));
    }

    /// Bouton rond du bas du rail (« + », boussole) : vert, plein au survol,
    /// du rond au carré arrondi en fondu comme les serveurs.
    fn rail_action(&self, ui: &mut egui::Ui, icon: Icon, tip: &str) -> egui::Response {
        let t = &self.theme;
        let resp = match t.layout.rail_style {
            RailStyle::Pills => {
                let side = t.layout.rail_button;
                let (rect, resp) = ui.allocate_exact_size(Vec2::splat(side), egui::Sense::click());
                let hot = resp.hovered() || resp.has_focus();
                let k = super::motion::hover(ui.ctx(), resp.id, hot);
                let radius = super::motion::mix_radius(t.r_rail(false), t.r_rail(true), k);
                ui.painter().rect_filled(rect, radius, super::motion::mix(t.colors.pill, t.colors.online, k));
                icons::paint(icon, ui.painter(), rect.shrink(side * 0.32), super::motion::mix(t.colors.online, t.colors.on_accent, k));
                self.rail_tip(&resp, tip, None);
                resp
            }
            RailStyle::List => {
                let h = t.layout.rail_list_row;
                let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
                let fill = if resp.hovered() { t.colors.row_hover } else { t.colors.bg0 };
                ui.painter().rect_filled(rect, t.r_rail(true), fill);
                let geo = t.rail_row();
                let badge = Rect::from_min_size(rect.left_top() + Vec2::splat(geo.pad), Vec2::splat(geo.badge));
                icons::paint(icon, ui.painter(), badge.shrink(geo.badge * 0.2), t.colors.text_strong);
                let pos = egui::pos2(badge.right() + geo.pad, rect.center().y);
                ui.painter().text(pos, egui::Align2::LEFT_CENTER, tip, egui::FontId::proportional(t.typography.body), t.colors.muted);
                resp
            }
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        super::keys::focus_ring(ui, t, &resp);
        resp
    }

    /// Boussole « Découvrir », sous le « + » : la page de découverte des
    /// serveurs (lot ND, `ui/discovery.rs`), en consultation.
    fn discover_button(&mut self, ui: &mut egui::Ui) {
        let tip = crate::i18n::table::nd::t::DISCOVER.get(self.lang);
        if self.rail_action(ui, Icon::Compass, tip).clicked() {
            self.show_discovery();
        }
    }

    /// « + » en bas du rail : créer, rejoindre, découvrir.
    fn add_server_button(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let resp = self.rail_action(ui, Icon::Plus, gx::ADD_SERVER.get(lang));
        let mut page = None;
        let mut discover = false;
        egui::Popup::menu(&resp).show(|ui| {
            if crate::ui::menu::item(ui, &t, gx::CREATE_SERVER.get(lang), Some(crate::theme::icons::Icon::Plus), false).clicked() {
                page = Some(Page::Choose);
            }
            if crate::ui::menu::item(ui, &t, gx::JOIN_SERVER.get(lang), Some(crate::theme::icons::Icon::ArrowRight), false).clicked() {
                page = Some(Page::Join);
            }
            if crate::ui::menu::item(ui, &t, crate::i18n::table::nd::t::DISCOVER.get(lang), Some(crate::theme::icons::Icon::Compass), false).clicked() {
                discover = true;
            }
        });
        if discover {
            self.show_discovery();
        }
        if let Some(p) = page {
            self.open_server_dialog(p);
        }
    }

    fn rail_item(&self, ui: &mut egui::Ui, tile: Tile<'_>, name: &str, selected: bool, unread: bool, ping: bool) -> egui::Response {
        match self.theme.layout.rail_style {
            RailStyle::Pills => {
                // Le lecteur d'écran dit le nom du serveur, pas ses initiales (lot C5).
                let r = self.rail_button(ui, tile, selected, unread, ping);
                self.rail_tip(&r, name, None);
                r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
                r
            }
            RailStyle::List => {
                let (label, icon) = match tile {
                    Tile::Home(label) => (label, None),
                    Tile::Guild(label, icon) => (label, icon),
                };
                self.rail_row(ui, label, icon, name, selected, unread, ping)
            }
        }
    }


    fn ring(&self, selected: bool, unread: bool, ping: bool) -> Stroke {
        let t = &self.theme;
        if unread && !selected { Stroke::new(t.shapes.unread_ring, if ping { t.colors.danger } else { t.colors.muted }) } else { Stroke::NONE }
    }

    /// Pastille du rail : icône du serveur, sinon ses initiales (ou la bulle
    /// des messages privés) sur un fond uni. L'état se lit sur l'indicateur
    /// au bord du rail, jamais sur un anneau autour de l'icône.
    fn rail_button(&self, ui: &mut egui::Ui, tile: Tile<'_>, selected: bool, unread: bool, ping: bool) -> egui::Response {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(self.theme.layout.rail_button), egui::Sense::click());
        self.rail_button_in(ui, rect, resp, tile, selected, unread, ping)
    }

    /// La pastille, dans la place `rect` déjà prise (réponse `resp`).
    #[allow(clippy::too_many_arguments)]
    fn rail_button_in(&self, ui: &mut egui::Ui, rect: Rect, resp: egui::Response, tile: Tile<'_>, selected: bool, unread: bool, ping: bool) -> egui::Response {
        let t = &self.theme;
        let c = &t.colors;
        // Même côté pour demander l'icône et pour la dessiner, quelle que
        // soit la taille du texte des initiales.
        let side = t.layout.rail_button;
        let hot = resp.hovered() || resp.has_focus();
        // Du rond au carré arrondi, en fondu de forme et de couleur.
        let k = super::motion::toward(ui.ctx(), resp.id, selected || hot, super::motion::dur::MORPH);
        let r = super::motion::mix_radius(t.r_rail(false), t.r_rail(true), k);
        let icon = match tile {
            Tile::Guild(_, icon) => icon,
            Tile::Home(_) => None,
        };
        if ui.is_rect_visible(rect) && !paint_image(ui, &self.images, rect, icon, r) {
            let (fill, color) = (super::motion::mix(c.pill, c.accent, k), super::motion::mix(c.text_strong, c.on_accent, k));
            let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            p.rect_filled(rect, r, fill);
            match tile {
                Tile::Home(_) => icons::paint(Icon::Chat, &p, rect.shrink(side * 0.29), color),
                Tile::Guild(label, _) => {
                    // Acronyme long : le texte rétrécit pour tenir dans la tuile, comme dans le client.
                    let n = label.chars().count().max(1) as f32;
                    let size = (t.typography.rail * (3.2 / n).min(1.0) * (rect.width() / side).min(1.0)).max(t.typography.small * 0.8);
                    p.text(rect.center(), egui::Align2::CENTER_CENTER, label, super::fonts::medium_font(size), color);
                }
            }
        }
        self.rail_mark(ui, resp.id, rect, selected, hot, unread);
        if ping {
            self.ping_dot(ui, rect);
        }
        super::keys::focus_ring(ui, t, &resp);
        resp
    }

    /// Indicateur au bord gauche du rail, à la hauteur de la pastille `tile` :
    /// haut et d'accent pour l'entrée ouverte, mi-haut au survol, court pour
    /// un non-lu. Le rail centre ses pastilles : son bord s'en déduit.
    fn rail_mark(&self, ui: &egui::Ui, key: egui::Id, tile: Rect, selected: bool, hot: bool, unread: bool) {
        let t = &self.theme;
        let target = if selected {
            MARK_OPEN
        } else if hot {
            MARK_HOT
        } else if unread {
            MARK_UNREAD
        } else {
            0.0
        };
        // La marque s'allonge et se rétracte.
        let share = super::motion::value(ui.ctx(), key, target, super::motion::dur::MORPH);
        if share <= 0.01 {
            return;
        }
        let color = t.colors.text_strong;
        let edge = tile.center().x - t.rail_panel_width() / 2.0;
        // Marque arrondie de tous les côtés (petit rond pour un non-lu, pilule sinon),
        // un peu plus large que le filet du thème et décollée du bord, comme dans le client.
        let (mark, r) = t.rail_mark(edge, tile, share);
        let w = (mark.width() * 1.5).round();
        let mark = Rect::from_min_size(egui::pos2(mark.left() + 1.0, mark.top()), egui::vec2(w, mark.height().max(w)));
        let r = CornerRadius::same(r.ne.max((w / 2.0).round() as u8));
        // La marge du rail est hors de la zone de défilement : le découpage y est élargi.
        let clip = ui.clip_rect();
        let clip = Rect::from_min_max(egui::pos2(clip.left().min(edge), clip.top()), clip.max);
        ui.painter().with_clip_rect(clip).rect_filled(mark, r, color);
    }

    /// Épaisseur de l'anneau d'une pastille de mention.
    fn badge_ring(&self) -> f32 {
        (self.theme.shapes.unread_ring - 1.0).max(1.0)
    }

    /// Mention sans compte connu : point `danger` au coin bas droit de la pastille.
    fn ping_dot(&self, ui: &egui::Ui, tile: Rect) {
        let t = &self.theme;
        let r = t.shapes.mention_dot + 1.0;
        let at = tile.right_bottom() - Vec2::splat(r);
        // Même rouge et même liseré que la pastille comptée (`mention_badge`).
        ui.painter().circle(at, r, t.danger_fill(), Stroke::new(self.badge_ring(), t.colors.bg0));
    }

    /// Filet court entre les messages privés et les serveurs.
    fn rail_divider(&self, ui: &mut egui::Ui) {
        let t = &self.theme;
        if t.layout.rail_style == RailStyle::List {
            ui.add_space(t.spacing.rail_gap);
            ui.separator();
            return;
        }
        let (rect, _) = ui.allocate_exact_size(egui::vec2(t.layout.rail_button, t.spacing.rail_gap + t.shapes.border), egui::Sense::hover());
        let y = rect.center().y;
        // 32 de large pour une pastille de 48.
        let half = (t.layout.rail_button / 3.0).round();
        ui.painter().line_segment([egui::pos2(rect.center().x - half, y), egui::pos2(rect.center().x + half, y)], Stroke::new(t.shapes.border.max(1.0) * 2.0, t.colors.border));
    }


    /// Ligne du rail en liste : initiales en petite pastille, puis le nom.
    #[allow(clippy::too_many_arguments)]
    fn rail_row(&self, ui: &mut egui::Ui, label: &str, icon: Option<ImageRef>, name: &str, selected: bool, unread: bool, ping: bool) -> egui::Response {
        let t = &self.theme;
        let h = t.layout.rail_list_row;
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), egui::Sense::click());
        let fill = if selected {
            t.colors.accent
        } else if resp.hovered() {
            t.colors.row_hover
        } else {
            t.colors.bg0
        };
        let p = ui.painter();
        p.rect(rect, t.r_rail(true), fill, self.ring(selected, unread, ping), egui::StrokeKind::Inside);

        let text_color = if selected { t.colors.on_accent } else if unread { t.colors.text_strong } else { t.colors.muted };
        let geo = t.rail_row();
        let pad = geo.pad;
        let badge = egui::Rect::from_min_size(rect.left_top() + Vec2::splat(pad), Vec2::splat(geo.badge));
        let badge_fill = if selected { t.colors.on_accent } else { t.colors.pill };
        let badge_text = if selected { t.colors.accent } else { t.colors.text_strong };
        if !paint_image(ui, &self.images, badge, icon, t.r_rail(true)) {
            let p = ui.painter();
            p.rect_filled(badge, t.r_rail(true), badge_fill);
            let small = egui::FontId::proportional(t.typography.small);
            p.text(badge.center(), egui::Align2::CENTER_CENTER, label, small, badge_text);
        }

        let left = badge.right() + pad;
        let text_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2((rect.right() - pad).max(left), rect.bottom()));
        // Emojis du nom du serveur en image Twemoji.
        let mut font = egui::TextStyle::Body.resolve(ui.style());
        if unread || selected {
            font.family = crate::ui::fonts::demi();
        }
        let row = egui::UiBuilder::new().max_rect(text_rect).layout(Layout::left_to_right(Align::Center));
        ui.scope_builder(row, |ui| super::emoji::label_job(ui, egui::text::LayoutJob::simple_singleline(name.to_string(), font, text_color), true));
        super::keys::focus_ring(ui, t, &resp);
        resp
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, find, frame, offline_boot, settle};
    use crate::state::model::{Guild, User};
    use crate::state::{Cmd, Event, Store};
    use eframe::egui;
    use std::time::Duration;

    /// Connexion factice : rien ne part, rien n'arrive.
    struct Quiet;

    impl super::super::Backend for Quiet {
        fn set_wake(&mut self, _: crate::net::Wake) {}
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

    /// Deux serveurs rangés dans le dossier « Atelier » (réglages synchronisés fictifs).
    fn app() -> super::super::App {
        use crate::state::usersettings::proto::{wrapper, Field, Msg, Value as P};
        let ctx = egui::Context::default();
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(10, "Forge".into(), vec![]), Guild::new(11, "Phare".into(), vec![])], dms: vec![] });
        let mut folder = Msg::default();
        folder.set(1, P::Len([10u64, 11].iter().flat_map(|g| g.to_le_bytes()).collect()));
        folder.set(2, P::Len(wrapper(P::Varint(7)).encode()));
        folder.set(3, P::Len(wrapper(P::Len("Atelier".as_bytes().to_vec())).encode()));
        let mut list = Msg::default();
        list.fields.push(Field { num: 1, value: P::Len(folder.encode()) });
        let mut root = Msg::default();
        root.set(14, P::Len(list.encode()));
        store.apply(Event::Account(crate::state::usersettings::AccountEvent::Proto { bytes: Some(root.encode()), partial: false }));
        store.select_dms();
        let images = crate::images::Images::disabled(ctx.clone());
        super::super::App::with_context(ctx, offline_boot(Box::new(Quiet), store, images))
    }

    fn resting(out: &egui::FullOutput) -> bool {
        out.viewport_output.values().all(|v| v.repaint_delay == Duration::MAX)
    }

    /// Un dossier se déroule et se replie en glissant : des images sont demandées
    /// pendant le mouvement seulement, plus aucune une fois ouvert ou refermé.
    #[test]
    fn folder_unrolls_then_rests() {
        let mut app = app();
        let out = settle(&mut app);
        assert!(resting(&out), "redessin au repos, dossier replié");
        assert!(find(&out, "Atelier").is_some(), "dossier affiché : {:?}", app.servers.cache.items);
        assert!(find(&out, "Phare").is_none(), "dossier replié : ses serveurs ne sont pas là");

        // Ouverture : le clic lance le mouvement, qui demande des images puis s'arrête seul.
        click(&mut app, "Atelier");
        let mut moving = 0;
        let mut out = frame(&mut app, Vec::new());
        for _ in 0..600 {
            if resting(&out) {
                break;
            }
            moving += 1;
            out = frame(&mut app, Vec::new());
        }
        assert!(moving > 0, "l'ouverture est animée");
        let out = settle(&mut app);
        assert!(resting(&out), "redessin au repos, dossier déplié");
        let (folder, last) = (find(&out, "Atelier").expect("dossier"), find(&out, "Phare").expect("serveur déplié"));
        assert!(find(&out, "Forge").is_some_and(|first| first.top() > folder.bottom() - 1.0 && last.top() > first.bottom() - 1.0), "serveurs sous le dossier, dans l'ordre");

        // Fermeture : même mouvement à l'envers, puis plus rien.
        click(&mut app, "Atelier");
        let out = settle(&mut app);
        assert!(resting(&out), "redessin au repos, dossier refermé");
        assert!(find(&out, "Phare").is_none(), "serveurs repliés");
    }
}
