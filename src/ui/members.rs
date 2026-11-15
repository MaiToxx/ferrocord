//! Liste des membres, à droite de la discussion : groupes (rôles affichés à
//! part, en ligne, hors ligne) avec leur compteur, puis une ligne par membre
//! (avatar, pastille de statut, nom coloré par son rôle le plus haut,
//! activité). Pour un MP de groupe : ses destinataires.
//!
//! Liste virtualisée (règle 1.3.2) : positions tirées de `vlist::Geometry`,
//! refaites seulement quand la forme de la liste change ; seules les lignes
//! visibles sont dessinées. Les lignes visibles décident des plages souscrites
//! (`Store::want_members`) et de ce qui réveille l'interface (`ListWatch`).

use super::{
    util::{paint_avatar, pixel_square},
    vlist::Geometry,
    App,
};
use crate::images::Images;
use crate::i18n::{f, t as tx, Lang};
use crate::state::members::{ListMember, MemberList, MemberStatus, Row};
use crate::state::people::Status;
use crate::state::model::{Avatar, User};
use crate::theme::{icons::Icon, MembersPanel, Tokens, MEMBERS_WIDTH};
use eframe::egui::{self, Color32, Rect, RichText, Sense, TextStyle, TextWrapMode, WidgetText};
use std::ops::Range;

/// Écart entre deux lignes de membre : le fond de survol ne le couvre pas.
const ROW_GAP: f32 = 2.0;
/// Part du fond qui couvre une ligne hors ligne (elle garde 30 % d'opacité).
const OFFLINE_VEIL: f32 = 0.7;

/// Quelle liste le salon ouvert peut montrer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Guild { guild: u64, channel: u64 },
    Group { channel: u64 },
}

/// Ce qui détermine les positions des lignes : liste, forme, hauteurs.
#[derive(Clone, Debug, PartialEq)]
struct Key {
    list: (u64, String),
    shape: u64,
    len: u32,
    heights: (f32, f32),
}

/// Géométrie de la liste affichée, gardée d'une image à l'autre.
#[derive(Default)]
pub(super) struct MembersView {
    geometry: Geometry,
    key: Option<Key>,
}

impl MembersView {
    /// Positions refaites seulement si la liste, sa forme ou les hauteurs changent.
    fn sync(&mut self, key: Key, is_header: impl Fn(u32) -> bool) {
        if self.key.as_ref() == Some(&key) {
            return;
        }
        let (row, header) = key.heights;
        let ids: Vec<u64> = (0..u64::from(key.len)).collect();
        let heights = (0..key.len).map(|i| if is_header(i) { header } else { row }).collect();
        self.geometry.set(ids, heights);
        self.key = Some(key);
    }

    /// Lignes qui coupent `[from, to)`.
    fn visible(&self, from: f32, to: f32) -> Range<u32> {
        let r = self.geometry.range(from, to);
        let c = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
        c(r.start)..c(r.end)
    }
}

/// Ligne de membre à dessiner (liste de serveur ou destinataire d'un groupe).
struct Line<'a> {
    user_id: u64,
    name: &'a str,
    avatar: &'a Avatar,
    /// `None` : statut inconnu (MP de groupe), pas de pastille.
    status: Option<MemberStatus>,
    color: u32,
    /// Rôles du membre (lot P : dégradé du rôle le plus haut).
    roles: &'a [u64],
    line: Option<&'a str>,
    bot: bool,
}

impl<'a> From<&'a ListMember> for Line<'a> {
    fn from(m: &'a ListMember) -> Self {
        Line { user_id: m.user_id, name: &m.name, avatar: &m.avatar, status: Some(m.status), color: m.color, roles: &m.roles, line: m.line.as_deref(), bot: m.bot }
    }
}

impl<'a> From<&'a User> for Line<'a> {
    fn from(u: &'a User) -> Self {
        Line { user_id: u.id, name: &u.name, avatar: &u.avatar, status: None, color: 0, roles: &[], line: None, bot: false }
    }
}

impl App {
    fn members_target(&self) -> Option<Target> {
        // MP à deux : son panneau est le profil du correspondant (`dm_profile.rs`).
        if self.dm_profile_user().is_some() {
            return None;
        }
        match (self.store.guild, self.store.channel) {
            (Some(guild), Some(channel)) => Some(Target::Guild { guild, channel }),
            // Groupe connu des relations : son panneau (`ui/groups.rs`) est le seul, avec la gestion du groupe.
            (None, Some(channel)) if self.store.members.recipients(channel).is_some() && !self.is_group(channel) => Some(Target::Group { channel }),
            _ => None,
        }
    }

    /// Bouton de l'en-tête de la discussion : affiche ou masque la liste
    /// (le même réglage que le panneau Apparence).
    pub(super) fn members_toggle(&mut self, ui: &mut egui::Ui) {
        let group = self.store.guild.is_none() && self.store.channel.is_some_and(|c| self.is_group(c));
        if self.members_target().is_none() && !group {
            return;
        }
        let shown = self.theme.members_shown();
        let tip = if shown { tx::MEMBERS_HIDE_TIP } else { tx::MEMBERS_SHOW_TIP }.get(self.lang);
        if super::chat_head::head_icon_drawn(ui, &self.theme, Icon::Members, tip, shown).clicked() {
            self.look.edited.layout.members = if shown { MembersPanel::Hidden } else { MembersPanel::Shown };
        }
    }

    /// Le salon ouvert a une liste de membres (disposition mobile, phase 7).
    pub(super) fn has_member_list(&self) -> bool {
        self.members_target().is_some() || (self.store.guild.is_none() && self.store.channel.is_some_and(|c| self.is_group(c)))
    }

    /// Clic sur un membre : carte de profil (`ui/profile.rs`), à côté du pointeur.
    pub(super) fn open_member_profile(&mut self, user_id: u64, guild_id: Option<u64>) {
        let at = self.ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default();
        self.open_profile(user_id, guild_id, at);
    }

    /// Panneau de droite, à appeler avant la zone centrale. Masqué, hors
    /// d'un salon, ou pour un MP à deux : rien n'est dessiné ni souscrit.
    pub(super) fn members_ui(&mut self, ui: &mut egui::Ui) {
        let watch = self.store.members.watch();
        // Fenêtre étroite : la liste s'efface sans que le réglage change (`layout.rs`).
        let desk = !self.mobile_active();
        let target = self.members_target().filter(|_| self.theme.members_shown() && !(desk && self.place.fit.members_hidden));
        let Some(target) = target else {
            watch.show(None);
            self.store.want_members(false, None);
            return;
        };
        let t = &self.theme;
        let frame = egui::Frame::new().fill(t.colors.bg1).inner_margin(t.channels_margin());
        let mut clicked = None;
        let mut seen = None;
        let App { store, images, theme, member_view, lang, look, .. } = self;
        // Comme la colonne des salons : largeur imposée une image quand le
        // réglage change, sinon redimensionnable à la souris et retenue.
        let width = theme.layout.members_width;
        let mut panel = egui::Panel::right("members").frame(frame);
        panel = if !desk {
            panel.exact_size(width).resizable(false)
        } else if look.members_shown != width {
            look.members_shown = width;
            panel.exact_size(width)
        } else {
            panel.default_size(width).size_range(MEMBERS_WIDTH.min..=MEMBERS_WIDTH.max)
        };
        let handle = ui.scope_id().with("members").with("__resize");
        let shown = panel.show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Side);
            let t = &*theme;
            // Pas de 44 : la ligne de 42 et 2 d'écart.
            let heights = (t.spacing.member_row + ROW_GAP, t.spacing.member_header);
            match target {
                Target::Guild { guild, channel } => {
                    if store.members.unavailable {
                        return note(ui, t, tx::MEMBERS_BOT.get(*lang));
                    }
                    let Some(list) = store.members.list_for(guild, channel) else {
                        seen = Some(0..0);
                        return note(ui, t, tx::MEMBERS_LOADING.get(*lang));
                    };
                    let key = Key { list: (guild, list.id.clone()), shape: list.shape, len: list.len(), heights };
                    member_view.sync(key, |i| list.is_header(i));
                    let (range, click) = guild_rows(ui, t, *lang, images, member_view, list);
                    seen = Some(range);
                    clicked = click.map(|u| (u, Some(guild)));
                }
                Target::Group { channel } => {
                    let me = store.me.as_ref();
                    let mut people: Vec<&User> = me.into_iter().chain(store.members.recipients(channel).unwrap_or_default()).collect();
                    people.sort_by_cached_key(|u| crate::state::nav::sort_key(&u.name));
                    let len = u32::try_from(people.len() + 1).unwrap_or(u32::MAX);
                    member_view.sync(Key { list: (0, channel.to_string()), shape: 0, len, heights }, |i| i == 0);
                    let label = f::member_group(*lang, tx::MEMBERS.get(*lang).to_uppercase(), people.len());
                    clicked = group_rows(ui, t, *lang, images, member_view, &label, &people).map(|u| (u, None));
                }
            }
        }).response.rect.width();
        if desk && ui.ctx().is_being_dragged(handle) {
            self.look.adopt_members_width(shown);
        }
        if let Target::Guild { guild, .. } = target {
            watch.show(seen.clone().map(|r| (guild, r)));
            self.store.want_members(true, seen);
        } else {
            watch.show(None);
            self.store.want_members(false, None);
        }
        if let Some((user, guild)) = clicked {
            self.open_member_profile(user, guild);
        }
    }
}

fn note(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add_space(t.spacing.category_gap);
    ui.add(egui::Label::new(RichText::new(text).color(t.colors.muted)).wrap());
}

/// Lignes visibles d'une liste de serveur ; renvoie les index visibles et le membre cliqué.
fn guild_rows(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, view: &MembersView, list: &MemberList) -> (Range<u32>, Option<u64>) {
    let mut out = (0..0, None);
    egui::ScrollArea::vertical().id_salt("members_scroll").auto_shrink(false).show_viewport(ui, |ui, viewport| {
        let g = &view.geometry;
        ui.set_height(g.total());
        let origin = ui.min_rect().top();
        let range = view.visible(viewport.min.y, viewport.max.y);
        for i in range.clone() {
            let rect = row_rect(ui, g, origin, i);
            match list.row(i) {
                Row::Header(h) => header(ui, t, rect, &h.label),
                Row::Member(m) => {
                    if member(ui, t, lang, images, rect, Line::from(m)) {
                        out.1 = Some(m.user_id);
                    }
                }
                Row::Pending => pending(ui, t, rect),
            }
        }
        out.0 = range;
    });
    out
}

/// Destinataires d'un MP de groupe, sous un titre (index 0).
fn group_rows(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, view: &MembersView, label: &str, people: &[&User]) -> Option<u64> {
    let mut clicked = None;
    egui::ScrollArea::vertical().id_salt("members_scroll").auto_shrink(false).show_viewport(ui, |ui, viewport| {
        let g = &view.geometry;
        ui.set_height(g.total());
        let origin = ui.min_rect().top();
        for i in view.visible(viewport.min.y, viewport.max.y) {
            let rect = row_rect(ui, g, origin, i);
            match i.checked_sub(1).and_then(|k| people.get(k as usize)) {
                None => header(ui, t, rect, label),
                Some(u) => {
                    if member(ui, t, lang, images, rect, Line::from(*u)) {
                        clicked = Some(u.id);
                    }
                }
            }
        }
    });
    clicked
}

fn row_rect(ui: &egui::Ui, g: &Geometry, origin: f32, i: u32) -> Rect {
    let i = i as usize;
    let x = ui.max_rect().x_range();
    Rect::from_x_y_ranges(x, origin + g.top(i)..=origin + g.top(i) + g.height(i))
}

fn header(ui: &egui::Ui, t: &Tokens, rect: Rect, label: &str) {
    let galley = WidgetText::from(RichText::new(label).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted)).into_galley(
        ui,
        Some(TextWrapMode::Truncate),
        (rect.width() - 2.0 * t.spacing.item_x).max(1.0),
        TextStyle::Small,
    );
    // Calé en bas de sa ligne : l'espace au-dessus sépare les groupes.
    let pos = egui::pos2(rect.left() + t.spacing.item_x, rect.bottom() - galley.size().y - t.spacing.item_y);
    ui.painter().galley(pos, galley, t.colors.muted);
}

/// Statut de la liste, dans le type du magasin de présences (`state::people`).
fn person_status(s: MemberStatus) -> Status {
    match s {
        MemberStatus::Online => Status::Online,
        MemberStatus::Idle => Status::Idle,
        MemberStatus::Dnd => Status::Dnd,
        MemberStatus::Offline => Status::Offline,
    }
}

/// Une ligne de membre ; `true` si elle est cliquée.
fn member(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, rect: Rect, m: Line<'_>) -> bool {
    let c = &t.colors;
    let rect = rect.shrink2(egui::vec2(0.0, ROW_GAP / 2.0));
    let resp = ui.interact(rect, ui.scope_id().with(("member", m.user_id)), Sense::click());
    super::keys::focus_ring(ui, t, &resp);
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.r_row(), c.row_hover);
    }
    // Parures (lot P) : plaque derrière la ligne, décoration fixe sur l'avatar.
    let look = super::cosmetics::row_look(ui, images, t, rect, m.user_id, m.roles, if resp.hovered() { c.row_hover } else { c.bg1 });
    let (slot, text_left) = t.dm_row_parts(rect);
    let slot = pixel_square(ui, slot.min, slot.width());
    if t.avatars_shown() {
        paint_avatar(ui, images, t, slot, m.avatar, m.name);
        look.decoration(ui, images, slot);
    }
    // Présence connue du magasin (`state::people`), sinon le statut reçu avec la liste.
    let status = super::profile::known_status(ui.ctx(), m.user_id).or(m.status.map(person_status));
    if let Some(s) = status {
        let bg = if resp.hovered() { c.row_hover } else { c.bg1 };
        if t.avatars_shown() {
            t.paint_status(ui.painter(), slot, s, bg);
            super::motion::typing_pill(ui, egui::Id::unique(("membre-écrit", m.user_id)), super::motion::typists(ui.ctx()).here(m.user_id), t.status_dot(slot), t.status_color(s), bg);
        } else {
            ui.painter().circle_filled(slot.center(), t.shapes.status_dot, t.status_color(s));
        }
    }
    let offline = status == Some(Status::Offline);
    let base = if m.color != 0 && !super::settings::local::display(ui.ctx()).role_colors_off { Color32::from_rgb((m.color >> 16) as u8, (m.color >> 8) as u8, m.color as u8) } else { c.text };
    let name_color = if offline { t.readable(c.text, c.bg1) } else { t.readable(base, c.bg1) };

    let width = (rect.right() - t.spacing.item_x - text_left).max(1.0);
    let tag = m.bot.then(|| tx::BOT_TAG.get(lang));
    let tag_galley = tag.map(|s| WidgetText::from(RichText::new(s).small().family(crate::ui::fonts::demi()).strong().color(c.on_accent)).into_galley(ui, Some(TextWrapMode::Extend), f32::INFINITY, TextStyle::Small));
    let tag_w = tag_galley.as_ref().map_or(0.0, |g| g.size().x + 2.0 * t.spacing.item_y + t.spacing.item_x);
    // Tag de serveur et nom en dégradé (lot P).
    let chip = look.tag(ui, t);
    let chip_w = chip.as_ref().map_or(0.0, |c| c.size.x + t.spacing.item_x);
    // Couronne du propriétaire et icône du rôle le plus haut qui en a une, après le nom (lot R).
    let crown = m.status.is_some() && super::cosmetics::is_owner(ui.ctx(), m.user_id);
    let badge = if m.roles.is_empty() { None } else { super::cosmetics::role_badge(ui.ctx(), m.roles) };
    let mark = Tokens::icon_side(t.typography.small);
    let marks_w = (f32::from(u8::from(crown)) + f32::from(u8::from(badge.is_some()))) * (mark + t.spacing.item_y);
    // Emojis du nom en image Twemoji, comme dans les messages.
    let (name, name_spots) = super::emoji::name_galley(ui, look.name(ui, m.name, TextStyle::Body.resolve(ui.style()), name_color, offline), (width - tag_w - chip_w - marks_w).max(1.0));
    // Statut personnalisé : ses emojis en image (lot T4).
    // Sur une plaque (rayures claires), le gris du sous-texte ne se lisait plus : encre du texte.
    let sub_color = if look.plated() { c.text } else { c.muted };
    let sub = m.line.map(|l| super::emoji::truncated(ui, l, &TextStyle::Small.resolve(ui.style()), sub_color, width));
    let total = name.size().y + sub.as_ref().map_or(0.0, |(s, _)| s.size().y);
    let top = rect.top() + Tokens::centered_top(rect.height(), total);
    let name_pos = egui::pos2(text_left, top);
    let name_w = name.size().x;
    let name_h = name.size().y;
    super::emoji::paint_spots(ui, name_pos, &name, &name_spots);
    ui.painter().galley(name_pos, name, name_color);
    if let Some(g) = tag_galley {
        let pad = t.spacing.item_y;
        let pill = Rect::from_min_size(egui::pos2(name_pos.x + name_w + t.spacing.item_x, top + (name_h - g.size().y) / 2.0 - pad / 2.0), g.size() + egui::vec2(2.0 * pad, pad));
        ui.painter().rect_filled(pill, t.r_row(), c.accent);
        ui.painter().galley(pill.min + egui::vec2(pad, pad / 2.0), g, c.on_accent);
    }
    if let Some(chip) = chip {
        let at = egui::pos2(name_pos.x + name_w + t.spacing.item_x + tag_w, top + (name_h - chip.size.y) / 2.0);
        let size = chip.size;
        chip.paint(ui, images, t, Rect::from_min_size(at, size), c.bg3);
    }
    let mut mark_x = name_pos.x + name_w + tag_w + chip_w + t.spacing.item_y;
    let mut mark_slot = || {
        let slot = pixel_square(ui, egui::pos2(mark_x, top + (name_h - mark) / 2.0), mark);
        mark_x += mark + t.spacing.item_y;
        slot
    };
    if let Some(b) = badge {
        let slot = mark_slot();
        match b {
            super::cosmetics::RoleBadge::Image(image) => {
                super::util::paint_image(ui, images, slot, Some(image), egui::CornerRadius::ZERO);
            }
            super::cosmetics::RoleBadge::Emoji(u) => super::emoji::paint(ui, slot, super::emoji::Img::Unicode(u)),
        }
    }
    if crown {
        let slot = mark_slot();
        crate::theme::icons::paint(Icon::Crown, ui.painter(), slot, t.status_color(Status::Idle));
        ui.interact(slot, resp.id.with("couronne"), Sense::hover()).on_hover_text(crate::i18n::table::restes::t::OWNER.get(lang));
    }
    if let Some((s, spots)) = sub {
        let pos = egui::pos2(text_left, top + name_h);
        super::emoji::paint_spots(ui, pos, &s, &spots);
        ui.painter().galley(pos, s, sub_color);
    }
    // Hors ligne : la ligne entière s'efface (avatar compris), sauf au survol.
    if offline && !resp.hovered() && !resp.has_focus() {
        ui.painter().rect_filled(rect, t.r_row(), c.bg1.gamma_multiply(OFFLINE_VEIL));
    }
    // Nommée pour les lecteurs d'écran : nom, statut, activité.
    let status = status.map(|s| super::profile::status_text(s).get(lang));
    resp.widget_info(|| {
        let mut label = m.name.to_string();
        for part in [status, m.line].into_iter().flatten() {
            label.push_str(", ");
            label.push_str(part);
        }
        egui::WidgetInfo::labeled(egui::Role::Button, true, label)
    });
    // Clic droit : menu de l'utilisateur (`ui/profile.rs`).
    if resp.secondary_clicked() {
        super::profile::open_menu(ui.ctx(), m.user_id, m.name, rect);
    }
    resp.clicked()
}

/// Ligne pas encore reçue : silhouette discrète, sans texte.
fn pending(ui: &egui::Ui, t: &Tokens, rect: Rect) {
    let (slot, text_left) = t.dm_row_parts(rect);
    let p = ui.painter();
    let fill = t.colors.row_hover;
    if t.avatars_shown() {
        p.rect_filled(slot, t.r_avatar(slot.width()), fill);
    }
    let h = (t.typography.body * 0.6).round();
    let bar = Rect::from_min_size(egui::pos2(text_left, rect.center().y - h / 2.0), egui::vec2(((rect.right() - text_left) * 0.6).max(0.0), h));
    p.rect_filled(bar, t.r_row(), fill);
}
