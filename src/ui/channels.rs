//! Colonne des salons (ou des conversations privées) et pied utilisateur.
//!
//! Une ligne : icône puis nom, alignés à gauche ; les salons rangés dans une
//! catégorie sont légèrement en retrait. Salon ouvert = fond dédié et texte
//! fort ; non lu = texte fort et petite marque à gauche ; survol = fond léger.

use super::{
    settings::NotifTarget,
    util::{paint_avatar, pixel_square},
    App,
};
use crate::i18n::t;
use crate::i18n::table::chadmin::t as cx;
use crate::state::model::{Avatar, ChannelKind};
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Align, Align2, FontId, Layout, RichText, Sense, TextStyle};

/// Entrées de lecture du menu d'une ligne, avant celles des notifications.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReadItem {
    None,
    /// Salon d'un serveur : « Marquer comme lu ».
    Channel(u64),
    /// Conversation privée : « Marquer comme lu », fermer ou quitter le groupe.
    Dm(u64),
}

/// Symbole d'une ligne.
#[derive(Clone, Copy)]
pub(super) enum RowIcon {
    Drawn(Icon),
    /// Conversation privée : avatar du correspondant ou icône du groupe.
    Avatar(Avatar),
    /// Conversation privée, avatars masqués : le caractère « @ » de la police.
    At,
}

/// Lignes gardées prêtes au-dessus et au-dessous de la vue (en hauteurs de ligne).
const ROW_MARGIN: f32 = 6.0;

/// Options d'une ligne de la liste.
#[derive(Clone, Copy, Default)]
pub(super) struct RowOpts {
    pub indented: bool,
    /// Texte plus visible au repos (fil rejoint, lot T10).
    pub emphasized: bool,
    /// Salon muet : texte éteint, pas de marque de non-lu.
    pub muted: bool,
    /// Icônes de fin de ligne au survol (inviter, modifier) : le nom leur laisse la place.
    pub tools: u8,
}

/// Chevron d'une catégorie : vers le bas, ou vers la droite si elle est repliée.
/// En-tête d'une catégorie de contacts dans la colonne des MP : son nom et un
/// chevron. Rend vrai au clic (replier ou déplier, gardé dans `local.toml`).
fn dm_category_head(ui: &mut egui::Ui, t: &Tokens, lang: crate::i18n::Lang, name: &str, collapsed: bool) -> bool {
    use crate::i18n::table::social::t as st;
    let key = name.to_lowercase();
    ui.add_space(t.spacing.category_gap);
    let head = ui.horizontal(|ui| {
        ui.add_space(t.spacing.item_x);
        ui.spacing_mut().item_spacing.x = t.spacing.item_y;
        let chevron = Tokens::icon_side(t.typography.small) * 0.75;
        let text = RichText::new(name).size(t.typography.body - 2.0).family(crate::ui::fonts::medium()).color(t.colors.muted);
        let room = (ui.available_width() - chevron - 2.0 * t.spacing.item_x).max(chevron);
        ui.allocate_ui_with_layout(egui::vec2(room, ui.text_style_height(&TextStyle::Body)), Layout::left_to_right(Align::Center), |ui| ui.add(egui::Label::new(text).truncate().selectable(false)));
        let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(chevron), Sense::hover());
        paint_chevron(ui.painter(), r, t.colors.muted, collapsed);
    });
    let row = egui::Rect::from_min_max(head.response.rect.min, egui::pos2(ui.max_rect().right(), head.response.rect.max.y));
    let resp = ui.interact(row, ui.scope_id().with(("categorie-mp", &key)), Sense::click());
    let word = if collapsed { st::CAT_UNFOLD } else { st::CAT_FOLD }.get(lang);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name}, {word}")));
    super::keys::focus_ring(ui, t, &resp);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

fn paint_chevron(p: &egui::Painter, rect: egui::Rect, color: egui::Color32, collapsed: bool) {
    icons::paint(if collapsed { Icon::ChevronRight } else { Icon::ChevronDown }, p, rect, color);
}

impl App {
    pub(super) fn channel_list(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        // Vocal (phase 4) : appui-pour-parler, sons, issue d'une modération.
        self.voice_tick(ui);
        // Pied : utilisateur connecté (un clic ouvre la carte de mon compte,
        // un clic droit ses actions), puis micro, casque et engrenage.
        let mut just_opened = false;
        // « Déconnexion » est en bas des Paramètres ; le pied garde micro, casque et engrenage.
        let logout = std::mem::take(&mut self.params.logout_asked);
        // Carte à deux lignes (nom, puis statut) qui passe sous le rail :
        // celui-ci lui garde la place (`layout::Place`, `rail.rs`).
        let reach = if self.mobile_active() || self.local.prefs.look.hide_rail { 0.0 } else { t.rail_panel_width() };
        let sub = self.user_panel_line();
        // Activité annoncée : manette verte puis « • » devant le statut
        // personnalisé, comme dans le client (même ligne que les MP).
        let (playing, sub) = match (self.panel_activity(), self.user_panel_custom()) {
            (true, Some(custom)) => (true, Some(format!("• {custom}"))),
            (true, None) => (true, self.user_panel_game().or(sub)),
            (false, _) => (false, sub),
        };
        // Jeu détecté : son encart est le haut de la carte (`game_card.rs`).
        let game = self.game_card_shown();
        // En vocal, le panneau de connexion est le haut de la même carte (lot V) :
        // pas d'air entre les deux, coins du haut droits.
        let in_voice = self.store.voice.me.is_some();
        let gap = t.spacing.item_x as i8;
        let footer = egui::Panel::bottom("me")
            .frame(egui::Frame::new().fill(self.mica_inner(t.colors.bg1)).inner_margin(egui::Margin { left: gap, right: gap, top: if in_voice || game { 0 } else { gap }, bottom: gap }))
            .show_separator_line(false)
            .show(ui, |ui| {
                // Thème à dégradé ou à image : le pied porte le même fond que la colonne,
                // sinon son aplat coupe le panneau en deux.
                super::look::backdrop(ui, super::look::Zone::Side);
                let pad = t.user_bar_margin();
                let text_h = ui.text_style_height(&TextStyle::Body) + if sub.is_some() { ui.text_style_height(&TextStyle::Small) } else { 0.0 };
                // Ligne à la hauteur de l'avatar : les icônes, plus basses, se
                // centrent sur son axe au lieu de se caler en haut.
                let row_h = t.user_bar_row(ui.spacing().button_padding.y).max(text_h);
                let (slot, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), row_h + pad.sum().y), Sense::hover());
                let card = egui::Rect::from_min_max(egui::pos2(slot.left() - reach, slot.top()), slot.max);
                // Le panneau des salons découpe à son bord : élargi jusque sous le rail.
                let clip = ui.clip_rect();
                let clip = egui::Rect::from_min_max(egui::pos2(clip.left() - reach, clip.top()), clip.max);
                let round = if in_voice || game { egui::CornerRadius { nw: 0, ne: 0, ..t.r_card() } } else { t.r_card() };
                // `with_clip_rect` croise avec la découpe du panneau : la carte s'arrêtait au
                // bord de la colonne. La découpe élargie est donc posée telle quelle.
                let mut wide = ui.painter().clone();
                wide.set_clip_rect(clip);
                wide.rect(card, round, t.panel_bg(), t.panel_stroke(), egui::StrokeKind::Inside);
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(card - pad).layout(Layout::right_to_left(Align::Center)));
                inner.set_clip_rect(clip);
                // Mes parures (lot profil) : ma plaque en fond du panneau (son fondu garde
                // le texte et les boutons lisibles, comme une ligne de la liste des membres),
                // ma décoration d'avatar, le style de mon nom. Images fixes ; la plaque s'anime sous le pointeur.
                let worn = self.store.me.as_ref().map(|u| super::cosmetics::worn(ui.ctx(), None, u.id)).unwrap_or_default();
                let under = match worn.look.nameplate() {
                    Some(plate) => {
                        let under = super::cosmetics::paint_nameplate_in(&inner, &self.images, &t, card.shrink(t.panel_stroke().width), round, plate, t.panel_bg());
                        // Voile du fond du panneau : icônes et statut restent lisibles sur l'image.
                        inner.painter().rect_filled(card.shrink(t.panel_stroke().width), round, t.panel_bg().gamma_multiply(0.35));
                        under
                    }
                    None => t.panel_bg(),
                };
                {
                    {
                        let ui = &mut inner;
                        // Engrenage, casque, micro (chacun son chevron), manette de l'activité :
                        // `user_panel.rs`. Jamais coupés ; le nom se tronque avant.
                        let keep = (t.layout.small_avatar * 2.75).round();
                        self.user_panel_buttons(ui, keep, under);
                        let me = self.store.me.as_ref();
                        let name = me.map(|u| u.name.clone()).unwrap_or_else(|| "…".into());
                        // Avatars masqués : rien n'est dessiné ni demandé.
                        let avatar = me.map(|u| u.avatar).filter(|_| t.avatars_shown());
                        let who = ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = t.spacing.item_x;
                            if let Some(avatar) = avatar {
                                let side = t.layout.small_avatar;
                                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                                let rect = pixel_square(ui, slot.min, side);
                                paint_avatar(ui, &self.images, &t, rect, &avatar, &name);
                                // Le statut que j'ai choisi (réglages synchronisés) ;
                                // à défaut, celui que le magasin de présences connaît.
                                if let Some(s) = self.own_status() {
                                    t.paint_status(ui.painter(), rect, s, under);
                                } else if let Some(me) = self.store.me.as_ref() {
                                    super::profile::status_dot(ui, &t, rect, me.id, under);
                                }
                                super::cosmetics::row_decoration(ui, &self.images, rect, &worn);
                            }
                            let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width().max(1.0), row_h), Sense::hover());
                            let width = rect.width();
                            let size = TextStyle::Body.resolve(ui.style()).size;
                            let (name, glow) = super::cosmetics::styled_line(ui, &name, size, worn.look.name_style(), under, t.colors.text_strong, width);
                            let lead_side = if playing { Tokens::icon_side(t.typography.small) } else { 0.0 };
                            let lead_w = if playing { lead_side + t.spacing.item_y } else { 0.0 };
                            let sub = sub.as_deref().map(|l| super::emoji::truncated(ui, l, &TextStyle::Small.resolve(ui.style()), t.colors.muted, (width - lead_w).max(1.0)));
                            let name_h = name.size().y;
                            let top = rect.top() + Tokens::centered_top(rect.height(), name_h + sub.as_ref().map_or(0.0, |(s, _)| s.size().y));
                            super::cosmetics::paint_styled(ui.painter(), egui::pos2(rect.left(), top), name, glow, t.colors.text_strong);
                            if let Some((s, spots)) = sub {
                                let line = egui::pos2(rect.left(), top + name_h);
                                if playing {
                                    let icon = pixel_square(ui, egui::pos2(line.x, line.y + (s.size().y - lead_side) / 2.0), lead_side);
                                    super::friends::paint_lead(ui, &t, super::friends::Lead::Game, icon, t.colors.online, under);
                                }
                                let pos = egui::pos2(line.x + lead_w, line.y);
                                super::emoji::paint_spots(ui, pos, &s, &spots);
                                ui.painter().galley(pos, s, t.colors.muted);
                            }
                        });
                        let who = ui.interact(who.response.rect, ui.scope_id().with("account_switch"), Sense::click());
                        let who = who.on_hover_text(crate::i18n::table::quick::t::ACCOUNT_TIP.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand);
                        if who.clicked() {
                            self.toggle_account_card();
                        }
                        // « Changer de compte » : le sélecteur, au-dessus du pied.
                        if self.user_panel_menu(&who) {
                            self.menu_open = true;
                            just_opened = true;
                        }
                    }
                }
                card
            });
        self.place.card = Some(footer.inner);
        self.place.foot = footer.response.rect.height();
        if logout {
            // Compte enregistré : confirmation dans le sélecteur ; sinon, fermeture directe.
            self.request_logout();
            if self.current.is_none() {
                return;
            }
            just_opened = true;
        }
        let rect = footer.inner;
        if self.account_card_ui(ui, rect) {
            self.menu_open = true;
            just_opened = true;
        }
        self.account_menu(ui, rect.left_top(), rect.width(), just_opened);
        // Jeu détecté : encart posé sur le panneau utilisateur (lot « activité »).
        let mut rect = rect;
        if let Some(card) = self.game_card(ui, reach) {
            rect = card.union(rect);
            self.place.card = Some(rect);
            self.place.foot += card.height();
        }
        // Connexion vocale, au-dessus du panneau utilisateur (phase 4) : le
        // rail lui laisse la place aussi, son filet s'arrête au haut de la carte.
        if let Some(voice) = self.voice_panel(ui, reach) {
            self.place.card = Some(voice.union(rect));
            self.place.foot += voice.height() + t.spacing.item_x;
        }

        let title = match self.store.guild {
            None => t::DIRECT_MESSAGES.get(lang).to_string(),
            Some(_) => self.store.current_guild().map(|g| super::words::guild_name(lang, &g.name).to_string()).unwrap_or_default(),
        };
        let head_row = super::chat_head::head_row(&t);
        let mut find = false;
        let banner = self.store.current_guild().and_then(|g| g.banner);
        egui::Panel::top("guild_title")
            .frame(egui::Frame::new().fill(self.mica_inner(t.colors.bg1)).inner_margin(super::chat_head::head_margin(&t, t.spacing.guild_title_x)))
            .show(ui, |ui| {
                super::look::backdrop(ui, super::look::Zone::Side);
                // Bannière du serveur (lot V) : derrière le nom, sur toute la largeur de
                // la colonne ; tant que l'image n'est pas là, l'en-tête reste ordinaire.
                let art = banner.and_then(|b| {
                    let m = super::chat_head::head_margin(&t, t.spacing.guild_title_x);
                    let band = eframe::emath::GuiRounding::round_to_pixels(super::voice::banner_band(ui.max_rect().min - m.left_top(), ui.max_rect().width() + m.sum().x), ui.pixels_per_point());
                    match self.images.get_fill_moving(b, band.width(), band.height(), super::voice::GUILD_BANNER_ASPECT, false) {
                        crate::images::Image::Ready(tex) => Some((band, tex, ui.painter().add(egui::Shape::Noop), m)),
                        _ => None,
                    }
                });
                // Sur l'image, le nom et le chevron sont blancs, quel que soit le thème.
                let on_art = art.is_some().then_some(egui::Color32::WHITE);
                // Même hauteur que l'en-tête de la discussion (48) : les deux filets s'alignent.
                let head = ui.horizontal(|ui| {
                    ui.set_min_height(head_row);
                    let chevron = Tokens::icon_side(t.typography.small);
                    let room = if self.store.guild.is_some() { chevron + t.spacing.item_x } else { 0.0 };
                    ui.allocate_ui_with_layout(egui::vec2(ui.available_width() - room, head_row), Layout::left_to_right(Align::Center), |ui| {
                        // Messages privés : « Rechercher ou lancer une conversation » à la place du titre (lot U).
                        if self.store.guild.is_none() {
                            find = self.dm_find_button(ui);
                            return;
                        }
                        // Emojis du nom du serveur en image Twemoji.
                        let font = egui::FontId::new(t.typography.title, crate::ui::fonts::demi());
                        let color = on_art.unwrap_or(t.colors.text_strong);
                        super::emoji::label_job(ui, egui::text::LayoutJob::simple_singleline(title, font, color), true);
                    });
                    if self.store.guild.is_some() {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(chevron), Sense::hover());
                            icons::paint(Icon::ChevronDown, ui.painter(), r, on_art.unwrap_or(t.colors.text));
                        });
                    }
                });
                if let Some((band, tex, slot, m)) = art {
                    let used = head.response.rect.height() + m.sum().y;
                    if band.height() > used {
                        ui.add_space(band.height() - used);
                    }
                    // Voile sombre en haut : le nom reste lisible sur une image claire.
                    let veil = egui::Rect::from_min_max(band.min, egui::pos2(band.right(), band.top() + (used * 1.5).min(band.height())));
                    let mut mesh = egui::Mesh::default();
                    let dark = egui::Color32::from_black_alpha(150);
                    for (p, c) in [(veil.left_top(), dark), (veil.right_top(), dark), (veil.left_bottom(), egui::Color32::TRANSPARENT), (veil.right_bottom(), egui::Color32::TRANSPARENT)] {
                        mesh.colored_vertex(p, c);
                    }
                    mesh.add_triangle(0, 1, 2);
                    mesh.add_triangle(1, 2, 3);
                    ui.painter().set(slot, egui::Shape::Vec(vec![crate::images::textured(band, egui::CornerRadius::ZERO, tex), egui::Shape::mesh(mesh)]));
                }
                // L'en-tête d'un serveur ouvre son menu, comme le clic droit dans le rail.
                if let Some(id) = self.store.guild {
                    let head = ui.interact(head.response.rect, ui.scope_id().with("menu_serveur"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
                    self.guild_header_menu(&head, id);
                }
            });

        if find {
            self.open_switcher();
        }

        egui::CentralPanel::default().frame(egui::Frame::new().fill(self.mica_inner(t.colors.bg1)).inner_margin(t.channels_margin())).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Side);
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let mut clicked = None;
                // Clic molette sur un salon : ouvert dans un onglet (lot E2).
                let mut tabbed = None;
                // Lignes qui ont un menu (clic droit), ouvert après la liste : entrées
                // de lecture (lot S3 + T9) puis notifications (lot C1).
                let mut menus = Vec::new();
                let mut friends = false;
                // Lot U : « Demandes de message », « + » de l'en-tête des MP.
                let (mut requests, mut new_dm) = (false, false);
                // Lot ND : ligne « Quêtes ».
                let mut quests = false;
                let mut shop = false;
                let mut nitro = false;
                let mut events = None;
                let mut dm_menu = None;
                // Fichiers tirés au-dessus de la liste : la ligne visée (F21).
                let file_drag = self.file_drag_pos(ui.ctx()).filter(|p| ui.clip_rect().contains(*p));
                let mut file_row: Option<(u64, egui::Rect)> = None;
                // Catégorie de contacts repliée ou dépliée d'un clic (`None` dedans : « Sans catégorie »).
                let mut fold_toggle: Option<Option<String>> = None;
                let mut mark_read = None;
                let mut summaries = None;
                let mut toggled = None;
                let mut guild_top = None;
                // Administration des salons (lot A2) : menus, « + », glisser-déposer.
                let mut admin_pick = None;
                let mut hits: Vec<super::channel_settings::list::Hit> = Vec::new();
                let mut drag = None;
                let mut list_guild = None;
                // Salons vocaux (phase 4) : rangés après les salons écrits de leur catégorie.
                let mut voice_acts = Vec::new();
                let mut voice_parent = None;
                // Fils (lot S) : choix du menu d'une ligne de fil.
                let mut thread_pick = None;
                let s = &self.store;
                match s.guild {
                    None => {
                        // Comme dans le client : Amis, Ton Nitro, Boutique, Quêtes ;
                        // puis « Demandes de message » quand il y en a. Lignes serrées (2 d'écart).
                        ui.spacing_mut().item_spacing.y = t.dm_gap();
                        friends = self.friends_entry(ui).clicked();
                        nitro = self.nitro_entry(ui);
                        shop = self.shop_entry(ui);
                        quests = self.quests_entry(ui);
                        requests = self.requests_entry(ui);
                        new_dm = self.dm_list_head(ui);
                        if s.dms.is_empty() {
                            ui.label(RichText::new(t::NO_CONVERSATIONS.get(lang)).color(t.colors.muted));
                        }
                        // Catégories de contacts (locales) : les MP à deux d'une personne rangée
                        // passent sous l'en-tête de sa première catégorie, dans l'ordre des
                        // catégories ; les autres (et les groupes) suivent, sous « Sans catégorie ».
                        let groups = &self.local.prefs.friend_groups;
                        let rank = |d: &crate::state::model::Dm| -> Option<usize> {
                            let info = s.relations.dm_info(d.id).filter(|i| !i.group)?;
                            let [peer] = info.recipients[..] else { return None };
                            groups.first_of(peer)
                        };
                        // Les demandes de message ont leur page (lot U).
                        let mut shown: Vec<(Option<usize>, &crate::state::model::Dm)> = s.dms.iter().filter(|d| !s.relations.is_request(d.id)).map(|d| (if groups.groups.is_empty() { None } else { rank(d) }, d)).collect();
                        let sorted = shown.iter().any(|(r, _)| r.is_some());
                        if sorted {
                            // Tri stable : l'ordre des MP est gardé dans chaque catégorie.
                            shown.sort_by_key(|(r, _)| r.unwrap_or(usize::MAX));
                        }
                        let mut section: Option<Option<usize>> = None;
                        let mut folded = false;
                        for (r, d) in shown {
                            if sorted && section != Some(r) {
                                section = Some(r);
                                let group = r.and_then(|i| groups.groups.get(i));
                                let name = group.map_or(crate::i18n::table::social::t::CAT_OTHERS.get(lang), |g| g.name.as_str());
                                folded = groups.is_folded(r);
                                if dm_category_head(ui, &t, lang, name, folded) {
                                    fold_toggle = Some(group.map(|g| g.name.clone()));
                                }
                            }
                            // Catégorie repliée : la conversation ouverte reste visible.
                            if folded && s.channel != Some(d.id) {
                                continue;
                            }
                            let icon = if t.avatars_shown() { RowIcon::Avatar(d.avatar) } else { RowIcon::At };
                            let row = self.channel_row(ui, icon, super::words::dm_name(lang, &d.name), d.id, false);
                            menus.push((NotifTarget::Channel { guild: None, channel: d.id }, row.clone(), ReadItem::Dm(d.id), None));
                            // Croix de fermeture au survol, comme dans le client.
                            if self.dm_close_cross(ui, &row, d.id) {
                                dm_menu = Some(if self.is_group(d.id) { super::groups::DmMenu::Leave(d.id) } else { super::groups::DmMenu::Close(d.id) });
                            } else if row.clicked() {
                                clicked = Some(d.id);
                            } else if row.middle_clicked() {
                                tabbed = Some(d.id);
                            }
                            if file_drag.is_some_and(|p| row.rect.contains(p)) {
                                file_row = Some((d.id, row.rect));
                            }
                        }
                    }
                    Some(_) => {
                        if let Some(g) = s.current_guild() {
                            let manage = s.can_manage_channels(g.id);
                            list_guild = Some((g.id, manage));
                            // Événements programmés (lot A7).
                            if self.events_entry(ui, g.id).clicked() {
                                events = Some(g.id);
                            }
                            // Parcourir, Membres, Boosts (lot FF2, F15).
                            guild_top = self.guild_top_entries(ui, g.id).map(|p| (g.id, p));
                            let mut in_category = false;
                            // Catégories repliées, salons muets masqués (lot S) : réglages du compte.
                            let rules = s.list_rules(g.id);
                            let (unread, mentioned) = (s.unread(), s.mentioned());
                            // Vocal d'une catégorie repliée : montré seulement si j'y suis connecté.
                            let my_voice_parent = s.voice.me.as_ref().and_then(|m| s.voice.channel(m.channel_id)).map(|c| c.parent);
                            let voice_shown = |parent: Option<u64>| parent.is_none_or(|p| !rules.collapsed(p)) || my_voice_parent == Some(parent);
                            // Salons sans VIEW_CHANNEL, et catégories dont tous les salons le sont : masqués.
                            // « Parcourir les salons » : seulement les suivis, si l'utilisateur l'a choisi.
                            for c in g.channels.iter().filter(|c| !s.is_hidden(c.id) && !s.browse_hides(g, c) && !rules.hides(g, c, unread, mentioned)) {
                                match c.kind {
                                    ChannelKind::Category => {
                                        if voice_shown(voice_parent) {
                                            clicked = self.voice_rows(ui, g.id, voice_parent, in_category, &mut voice_acts).or(clicked);
                                        }
                                        voice_parent = Some(c.id);
                                        in_category = true;
                                        ui.add_space(t.spacing.category_gap);
                                        let collapsed = rules.collapsed(c.id);
                                        let head = ui.horizontal(|ui| {
                                            ui.spacing_mut().item_spacing.x = t.spacing.item_y;
                                            // Comme dans le client : le nom dans sa casse d'origine, puis le chevron.
                                            let chevron = Tokens::icon_side(t.typography.small) * 0.75;
                                            let text = RichText::new(&c.name).size(t.typography.body - 2.0).family(crate::ui::fonts::medium()).color(t.colors.muted);
                                            // Le « + » (au survol) et le chevron gardent leur place : le nom se coupe avant.
                                            let room = (ui.available_width() - chevron - 3.0 * t.spacing.item_x - Tokens::icon_side(t.typography.button)).max(chevron);
                                            ui.allocate_ui_with_layout(egui::vec2(room, ui.text_style_height(&TextStyle::Body)), Layout::left_to_right(Align::Center), |ui| ui.add(egui::Label::new(text).truncate().selectable(false)));
                                            let (r, _) = ui.allocate_exact_size(egui::Vec2::splat(chevron), Sense::hover());
                                            paint_chevron(ui.painter(), r, t.colors.muted, collapsed);
                                        });
                                        // Toute la largeur de la liste : le « + » se place à droite.
                                        let row = egui::Rect::from_min_max(head.response.rect.min, egui::pos2(ui.max_rect().right(), head.response.rect.max.y));
                                        let head = ui.interact(row, ui.scope_id().with(("categorie", c.id)), Sense::click());
                                        let head = if manage { head.interact(Sense::drag()) } else { head };
                                        let label = if collapsed { cx::EXPAND_CATEGORY } else { cx::COLLAPSE_CATEGORY }.get(lang);
                                        head.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} : {}", c.name, label)));
                                        super::keys::focus_ring(ui, &t, &head);
                                        // Le « + » n'apparaît qu'au survol de la catégorie (ou au clavier).
                                        if manage && (head.contains_pointer() || head.has_focus()) && self.category_plus(ui, head.rect, c.id) {
                                            admin_pick = Some(super::channel_settings::list::AdminPick::Create { guild: g.id, parent: Some(c.id), category: false });
                                        } else if head.clicked() {
                                            admin_pick = Some(super::channel_settings::list::AdminPick::ToggleCategory { guild: g.id, category: c.id });
                                        }
                                        if head.drag_started() {
                                            drag = Some((c.id, true));
                                        }
                                        hits.push((head.rect, c.id, true, false));
                                        menus.push((NotifTarget::Channel { guild: Some(g.id), channel: c.id }, head, ReadItem::None, Some((g.id, c.id, true))));
                                    }
                                    ChannelKind::Text | ChannelKind::Announcement | ChannelKind::Forum | ChannelKind::Media => {
                                        let icon = match c.kind {
                                            ChannelKind::Announcement => Icon::Megaphone,
                                            ChannelKind::Forum => Icon::Forum,
                                            ChannelKind::Media => Icon::Media,
                                            _ => Icon::Hash,
                                        };
                                        let (invite, edit) = (self.can_invite_in(c.id), s.can_edit_channel(c.id));
                                        let opts = RowOpts { indented: in_category, muted: rules.muted(c), tools: u8::from(invite) + u8::from(edit), ..RowOpts::default() };
                                        let row = self.channel_row_opts(ui, RowIcon::Drawn(icon), &c.name, c.id, opts);
                                        let row = if manage { row.interact(Sense::drag()) } else { row };
                                        if row.drag_started() {
                                            drag = Some((c.id, false));
                                        }
                                        // Icônes de fin de ligne, au survol et sur le salon ouvert.
                                        if opts.tools > 0
                                            && (row.contains_pointer() || s.channel == Some(c.id))
                                            && let Some(p) = self.row_tools(ui, row.rect, g.id, c.id, invite, edit)
                                        {
                                            admin_pick = Some(p);
                                        }
                                        hits.push((row.rect, c.id, false, c.parent.is_some()));
                                        menus.push((NotifTarget::Channel { guild: Some(g.id), channel: c.id }, row.clone(), ReadItem::Channel(c.id), Some((g.id, c.id, false))));
                                        if row.clicked() {
                                            clicked = Some(c.id);
                                        } else if row.middle_clicked() {
                                            tabbed = Some(c.id);
                                        }
                                        if !c.kind.is_forum() && file_drag.is_some_and(|p| row.rect.contains(p)) {
                                            file_row = Some((c.id, row.rect));
                                        }
                                        // Fils du salon, en dessous (lot T10).
                                        let rows = self.thread_rows(ui, c.id, c.kind.is_forum());
                                        clicked = rows.clicked.or(clicked);
                                        toggled = rows.toggled.or(toggled);
                                        thread_pick = rows.pick.or(thread_pick);
                                    }
                                }
                            }
                            if voice_shown(voice_parent) {
                                clicked = self.voice_rows(ui, g.id, voice_parent, in_category, &mut voice_acts).or(clicked);
                            }
                        }
                    }
                }
                for (target, row, read, admin) in menus {
                    row.context_menu(|ui| {
                        // MP à deux : le menu de l'utilisateur (lot U), sourdine comprise.
                        let mut person = None;
                        match read {
                            ReadItem::Channel(c) => {
                                if self.channel_menu(ui) {
                                    mark_read = Some(c);
                                }
                                if self.summaries_entry(ui) {
                                    summaries = Some(c);
                                }
                            }
                            ReadItem::Dm(c) => {
                                if let Some(m) = self.dm_menu(ui, c) {
                                    dm_menu = Some(m);
                                }
                                person = self.dm_person(c);
                            }
                            ReadItem::None => {}
                        }
                        // Plugins locaux (lot E1) : mots-clés, export.
                        if let ReadItem::Channel(c) | ReadItem::Dm(c) = read {
                            self.local_plugins_menu(ui, c);
                            // Onglet, vue scindée, fenêtre détachée (lot E2).
                            let guild = if matches!(read, ReadItem::Dm(_)) { None } else { self.store.guild };
                            self.windows_menu(ui, crate::state::nav::Place { guild, channel: Some(c) });
                        }
                        if read != ReadItem::None {
                            crate::ui::menu::separator(ui, &self.theme);
                        }
                        // Catégorie : marquer comme lu, replier ; salon : inviter. Puis l'administration.
                        if let Some((g, c, cat)) = admin {
                            let own = if cat { self.category_menu(ui, g, c) } else { self.invite_entry(ui, g, c) };
                            admin_pick = own.or(self.channel_admin_menu(ui, g, c, cat)).or(admin_pick);
                        }
                        match person {
                            Some((user, name)) => self.user_menu_items(ui, user, &name),
                            None => self.notif_menu(ui, target),
                        }
                    });
                }
                if let Some((gid, true)) = list_guild {
                    // Fond de la liste : créer un salon ou une catégorie.
                    let rest = ui.available_rect_before_wrap();
                    if rest.height() > 0.0 {
                        let bg = ui.interact(rest, ui.scope_id().with("fond-salons"), Sense::click());
                        bg.context_menu(|ui| {
                            if let Some(p) = self.channel_list_menu(ui, gid) {
                                admin_pick = Some(p);
                            }
                        });
                    }
                    if let Some((id, cat)) = drag {
                        self.chan_ui.dnd.start(id, cat);
                    }
                    let theme = self.theme.clone();
                    if let Some(dropped) = self.chan_ui.dnd.finish(ui, &theme, &hits) {
                        self.drop_channel(gid, dropped);
                    }
                }
                // Menu d'un salon vocal : sa ligne est dessinée par le vocal.
                if let Some(p) = admin_pick.or_else(|| self.take_voice_pick(ui.ctx())) {
                    self.apply_admin_pick(p);
                }
                if let Some(p) = thread_pick {
                    self.apply_thread_pick(p);
                }
                let clicked = self.file_row_drop(ui.ctx(), file_row).or(clicked);
                // Clic molette, ou Ctrl+clic : dans un onglet d'arrière-plan (lot E2).
                if let Some(id) = tabbed.or(clicked.filter(|_| self.tab_gesture(ui))) {
                    self.tab_open(crate::state::nav::Place { guild: self.store.guild, channel: Some(id) });
                } else if let Some(id) = clicked {
                    self.open_channel(id);
                }
                if let Some(parent) = toggled {
                    self.threads.toggle(parent);
                }
                if friends {
                    self.store.select_dms();
                    self.requests.open = false;
                    self.quests.open = false;
                }
                if shop {
                    self.show_shop();
                }
                if nitro {
                    self.show_settings(super::settings::Page::Billing);
                }
                if quests {
                    self.show_quests(false);
                }
                if requests {
                    // Rien en attente : la page ouvre sur le dossier de spam, où sont les demandes.
                    let only_spam = self.store.relations.requests(false).is_empty();
                    self.show_requests(only_spam);
                }
                if new_dm {
                    self.pick_friends(Vec::new());
                }
                if let Some(g) = events {
                    self.open_events(g);
                }
                if let Some((g, p)) = guild_top {
                    self.apply_guild_top(g, p);
                }
                if let Some(name) = fold_toggle {
                    self.friend_groups_edit(|g| g.toggle_fold(name.as_deref()));
                }
                if let Some(m) = dm_menu {
                    self.apply_dm_menu(m);
                }
                if let Some(c) = mark_read {
                    self.store.mark_read(c);
                }
                if let Some(c) = summaries {
                    self.show_summaries(c);
                }
                self.voice_acts(voice_acts);
            });
        });
    }

    /// « Masquer les salons muets » du menu du serveur : réglage du compte, écrit au clic.
    pub(super) fn hide_muted_entry(&mut self, ui: &mut egui::Ui, guild: u64) {
        let mut on = self.store.hides_muted_channels(guild);
        if crate::ui::menu::check(ui, &self.theme, cx::HIDE_MUTED.get(self.lang), &mut on) {
            if let Err(r) = self.store.set_hide_muted_channels(guild, on) {
                self.refused(r);
            }
            ui.close();
        }
        crate::ui::menu::separator(ui, &self.theme);
    }

    fn channel_row(&self, ui: &mut egui::Ui, icon: RowIcon, name: &str, id: u64, indented: bool) -> egui::Response {
        self.channel_row_opts(ui, icon, name, id, RowOpts { indented, ..RowOpts::default() })
    }

    /// `emphasized` : texte plus visible au repos (fil rejoint, lot T10).
    pub(super) fn channel_row_ex(&self, ui: &mut egui::Ui, icon: RowIcon, name: &str, id: u64, indented: bool, emphasized: bool) -> egui::Response {
        self.channel_row_opts(ui, icon, name, id, RowOpts { indented, emphasized, ..RowOpts::default() })
    }

    pub(super) fn channel_row_opts(&self, ui: &mut egui::Ui, icon: RowIcon, name: &str, id: u64, o: RowOpts) -> egui::Response {
        let RowOpts { indented, emphasized, muted, tools } = o;
        let t = &self.theme;
        let c = &t.colors;
        let selected = self.store.channel == Some(id);
        // Un salon muet ne se signale pas comme non lu (ses mentions, si).
        let unread = self.store.unread().contains(&id) && !muted;
        let ping = self.store.mentioned().contains(&id);

        // Conversation privée : la ligne est plus haute, pour l'avatar.
        let height = if matches!(icon, RowIcon::Avatar(_)) { t.dm_row() } else { t.spacing.channel_row };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::click());
        // Ligne loin de la vue (lot C10) : sa place, son identifiant et son nom
        // restent (clavier, lecteurs d'écran) ; ni texte, ni icône, ni avatar
        // ne sont préparés. La marge garde prêtes les lignes que le défilement amène.
        if !ui.is_rect_visible(rect.expand2(egui::vec2(0.0, ROW_MARGIN * height))) && !resp.has_focus() {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
            return resp;
        }
        // Avec des icônes de fin de ligne, la ligne reste survolée quand le pointeur est sur l'une d'elles.
        let hovered = resp.hovered() || (tools > 0 && resp.contains_pointer());
        let p = ui.painter();
        let hover_k = super::motion::hover(ui.ctx(), resp.id, hovered && !selected);
        if selected {
            p.rect_filled(rect, t.r_row(), c.selected);
        } else if hover_k > 0.0 {
            p.rect_filled(rect, t.r_row(), c.row_hover.gamma_multiply(hover_k));
        }
        if unread && !selected {
            // Contre le bord de la colonne, dans sa marge (hors de la zone de défilement).
            let (mark, r) = t.unread_mark(rect);
            let mark = mark.translate(egui::vec2(-t.spacing.channels_pad, 0.0));
            let clip = ui.clip_rect();
            let clip = egui::Rect::from_min_max(egui::pos2(clip.left().min(mark.left()), clip.top()), clip.max);
            p.with_clip_rect(clip).rect_filled(mark, r, c.text_strong);
        }

        let strong = selected || unread;
        let text_color = if strong { c.text_strong } else if hovered || emphasized { c.text } else if muted { c.channel_idle.gamma_multiply(0.7) } else { c.channel_idle };
        let icon_color = if strong { c.text_strong } else if muted { c.channel_idle.gamma_multiply(0.7) } else { c.channel_idle };
        let indent = if indented { t.spacing.channel_indent } else { Default::default() };
        let dm_row = matches!(icon, RowIcon::Avatar(_));
        let mut dm_look = None;
        let text_left = match icon {
            RowIcon::Drawn(i) => {
                let (icon_rect, text_left) = t.channel_row_parts(rect, indent);
                icons::paint(i, p, icon_rect, icon_color);
                text_left
            }
            RowIcon::Avatar(avatar) => {
                let (slot, text_left) = t.dm_row_parts(rect);
                // Parures du correspondant (lot P) : plaque derrière la ligne, décoration fixe.
                let ring = if selected { c.selected } else if hovered { c.row_hover } else { c.bg1 };
                let look = super::cosmetics::dm_look(ui, &self.images, t, rect, id, ring);
                let rect = pixel_square(ui, slot.min, slot.width());
                paint_avatar(ui, &self.images, t, rect, &avatar, name);
                if let Some(look) = &look {
                    look.decoration(ui, &self.images, rect);
                }
                super::profile::dm_dot(ui, t, rect, id, ring);
                dm_look = look;
                text_left
            }
            RowIcon::At => {
                let (icon_rect, text_left) = t.channel_row_parts(rect, indent);
                p.text(icon_rect.center(), Align2::CENTER_CENTER, "@", FontId::proportional(t.typography.body), icon_color);
                text_left
            }
        };

        // Pastille chiffrée des mentions, en fin de ligne (16 de haut pour un texte de 12, comme le rail).
        let pill = ping.then(|| {
            let galley = p.layout_no_wrap(crate::state::notify::badge_text(self.store.mention_count(id)), super::fonts::demi_font(t.typography.small), c.on_accent);
            let h = (t.typography.small * 4.0 / 3.0).round().max(t.shapes.mention_dot * 2.0);
            let w = (galley.size().x + h * 0.5).round().max(h);
            (egui::Rect::from_min_size(egui::pos2(rect.right() - t.spacing.item_x - w, (rect.center().y - h / 2.0).round()), egui::vec2(w, h)), galley)
        });
        let mut right = pill.as_ref().map_or(rect.right(), |(r, _)| r.left()) - t.spacing.item_x;
        // Brouillon gardé dans ce salon : petit crayon avant la fin de la ligne.
        if !selected && self.input.has_draft(id) {
            let side = Tokens::icon_side(t.typography.small);
            let pencil = egui::Rect::from_center_size(egui::pos2(right - side / 2.0, rect.center().y), egui::Vec2::splat(side));
            icons::paint(Icon::Pencil, p, pencil, c.faint);
            ui.interact(pencil, ui.scope_id().with(("draft", id)), Sense::hover()).on_hover_text(t::DRAFT_TIP.get(self.lang));
            right = pencil.left() - t.spacing.item_x;
        }
        if tools > 0 && (hovered || selected) {
            right -= super::channel_settings::list::row_tools_width(t, tools);
        }
        let text_rect = egui::Rect::from_min_max(egui::pos2(text_left, rect.top()), egui::pos2(right.max(text_left), rect.bottom()));
        // Emojis du nom en image (lot T4).
        // Ligne ouverte ou non lue : demi-gras.
        let mut font = TextStyle::Body.resolve(ui.style());
        if strong {
            font.family = super::fonts::demi();
        } else if dm_row {
            // Conversation privée : graisse moyenne au repos, comme dans le client.
            font.family = super::fonts::medium();
        }
        if dm_row {
            font.size = t.dm_text();
        }
        // MP à deux : surnom d'ami, tag de serveur et style du nom du correspondant, comme dans le client.
        let peer = if dm_row { self.store.relations.dm_info(id).filter(|d| !d.group).and_then(|d| d.recipients.first().copied()) } else { None };
        let name = peer.and_then(|peer| self.store.relations.nickname(peer)).unwrap_or(name);
        // Ligne à plaque : le texte se lit sur l'image, il est donc plus clair qu'au repos.
        let plated = dm_look.as_ref().is_some_and(super::cosmetics::RowLook::plated);
        let text_color = if plated && !strong { c.text } else { text_color };
        let mut chip = dm_look.as_ref().and_then(|l| l.tag(ui, t));
        let chip_w = chip.as_ref().map_or(0.0, |c| c.size.x + t.spacing.item_x);
        // MP à deux : le nom se dessine en texte, comme dans le client (`friends::name_line`).
        let person = dm_look.as_ref().map(super::cosmetics::RowLook::name_style);
        let (galley, spots, backdrop) = match person {
            Some((style, bg)) => {
                // Comme dans le client : la police du style toujours, ses couleurs et son effet au survol.
                let style = if hovered { style } else { style.map(crate::state::cosmetics::NameStyle::plain) };
                let line = |width: f32| super::friends::name_line(ui, name, font.size, style, Some(font.family.clone()), bg, text_color, width);
                // Le tag reste toujours sur la ligne, comme dans le client : c'est le nom
                // qui se coupe pour lui faire place (sauf ligne trop étroite pour les deux).
                let (mut g, mut backdrop) = line(f32::INFINITY);
                if g.size().x + chip_w > text_rect.width() {
                    if text_rect.width() < 2.0 * chip_w {
                        chip = None;
                    }
                    let room = text_rect.width() - if chip.is_some() { chip_w } else { 0.0 };
                    if g.size().x > room {
                        (g, backdrop) = line(room);
                    }
                }
                (g, Vec::new(), backdrop)
            }
            None => {
                let (g, spots) = super::emoji::truncated(ui, name, &font, text_color, text_rect.width().max(1.0));
                (g, spots, Vec::new())
            }
        };
        // MP à deux : sous le nom, l'icône de l'activité du correspondant puis son statut
        // personnalisé, comme dans le client (même seconde ligne que la page Amis).
        // MP du compte système de Discord : « Message officiel de Discord » et l'étiquette après le nom.
        let official = dm_row && self.store.is_official_dm(id);
        let second = if official { Some((super::friends::Lead::None, crate::i18n::table::messages::t::OFFICIAL_DM.get(self.lang).to_string())) } else { peer.and_then(|peer| self.dm_second(peer)) };
        let lead = second.as_ref().map_or(super::friends::Lead::None, |(lead, _)| *lead);
        let lead_side = if lead == super::friends::Lead::None { 0.0 } else { Tokens::icon_side(t.typography.small) };
        let lead_w = if lead_side > 0.0 { lead_side + t.spacing.item_y } else { 0.0 };
        let sub_color = if strong || plated { c.text } else { c.channel_idle };
        let sub = second.as_ref().map(|(_, line)| {
            let font = FontId::proportional(t.typography.small);
            super::emoji::truncated(ui, line, &font, sub_color, (text_rect.width() - lead_w).max(1.0))
        });
        let sub_h = sub.as_ref().map_or(0.0, |(g, _)| g.size().y);
        let block = egui::vec2(galley.size().x, galley.size().y + sub_h);
        let pos = Align2::LEFT_CENTER.align_size_within_rect(block, text_rect).min;
        let name_h = galley.size().y;
        if let Some(chip) = chip {
            let size = chip.size;
            let at = egui::pos2(pos.x + galley.size().x + t.spacing.item_x, (pos.y + (name_h - size.y) / 2.0).round());
            chip.paint(ui, &self.images, t, egui::Rect::from_min_size(at, size), c.bg3);
        }
        if official {
            let size = super::official::tag_size(ui, self.lang, font.size);
            let at = egui::pos2(pos.x + galley.size().x + t.spacing.item_x / 2.0, (pos.y + (name_h - size.y) / 2.0).round());
            if at.x + size.x <= text_rect.right() {
                super::official::paint_tag(ui, t, self.lang, egui::Rect::from_min_size(at, size), font.size);
            }
        }
        super::emoji::paint_spots(ui, pos, &galley, &spots);
        super::cosmetics::paint_styled(ui.painter(), pos, galley, backdrop, text_color);
        if let Some((g, spots)) = sub {
            let at = egui::pos2(pos.x + lead_w, pos.y + name_h);
            if lead_side > 0.0 {
                let icon = pixel_square(ui, egui::pos2(pos.x, at.y + (g.size().y - lead_side) / 2.0), lead_side);
                let under = dm_look.as_ref().map_or(c.bg1, |l| l.name_style().1);
                super::friends::paint_lead(ui, t, lead, icon, c.online, under);
            }
            super::emoji::paint_spots(ui, at, &g, &spots);
            ui.painter().galley(at, g, sub_color);
        }

        if let Some((r, galley)) = pill {
            ui.painter().rect_filled(r, egui::CornerRadius::same((r.height() / 2.0).round() as u8), t.danger_fill());
            ui.painter().galley(r.center() - galley.size() / 2.0, galley, c.on_accent);
        }
        // Clavier (Tab, flèches) : contour visible, nom lu par les lecteurs d'écran (lot navigation).
        super::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
        // MP à deux : activité du correspondant, en toutes lettres, au survol (lot C3).
        if resp.hovered()
            && peer.is_some()
            && let Some(line) = super::profile::dm_activity(ui.ctx(), id, self.lang)
        {
            return resp.on_hover_text(line);
        }
        resp
    }
}
