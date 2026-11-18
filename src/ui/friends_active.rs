//! Colonne « En ligne » de la page Amis (lot « vocal ») : une carte par
//! groupe d'amis réunis dans un salon vocal d'un serveur commun, puis une
//! carte par ami dont l'activité est enrichie, comme dans le client.
//!
//! Chaque carte a deux parties : en haut l'avatar (et sa pastille), le nom et
//! un résumé ; en bas, plus sombre, le détail (salon et serveur, ou image de
//! l'activité, titre, sous-ligne et temps écoulé).
//!
//! Données : seulement ce qui est déjà reçu (états vocaux de READY,
//! GUILD_CREATE et VOICE_STATE_UPDATE ; présences). Rien n'est demandé au
//! réseau pour remplir la colonne, hormis les images déjà servies par le
//! cache d'images (avatars, icônes de serveur, images d'activité).
//!
//! Règle de choix (déduite des captures du client) : un ami y figure s'il est
//! en vocal sur un serveur commun, ou s'il a une activité enrichie (détails,
//! état ou image). Un jeu nu, sans présence enrichie, n'y figure pas.

use super::{
    friends::{paint_status, presence, PAGE_Y},
    util::{paint_avatar, paint_image, pixel_square},
    App,
};
use crate::i18n::table::people::t as ut;
use crate::i18n::table::voice::{f as vf, t as vt};
use crate::i18n::Lang;
use crate::state::model::Avatar;
use crate::state::people::{Activity, ActivityKind};
use crate::state::Store;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Align, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke};

/// Marge intérieure d'une carte.
const PAD: f32 = 14.0;
/// Rayon des coins d'une carte, et de l'icône de serveur ou d'activité.
const RADIUS: u8 = 14;
const TILE_RADIUS: u8 = 8;
/// Avatars des participants d'une carte vocale : côté, recouvrement, nombre au plus.
const FACE: f32 = 24.0;
const FACE_OVERLAP: f32 = 6.0;
const FACES: usize = 4;

/// Une carte de la colonne.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Card {
    /// Amis réunis dans le même salon vocal d'un serveur commun.
    Voice { guild: u64, channel: u64, friends: Vec<u64> },
    /// Ami dont l'activité est enrichie.
    Activity { user: u64 },
}

/// Cartes affichées et les révisions (relations, présences, états vocaux) dont elles dépendent.
#[derive(Default)]
pub(super) struct Cache(Option<((u64, u64, u64), Vec<Card>)>);

/// Activité enrichie : elle porte des détails, un état ou une image.
pub(super) fn rich(a: &Activity) -> bool {
    a.kind != ActivityKind::Custom && !a.name.trim().is_empty() && (a.details.is_some() || a.state.is_some() || a.rich().is_some_and(|r| r.large.as_ref().is_some_and(|i| i.url.is_some())))
}

/// Première activité enrichie de `user`, s'il en a une et n'est pas hors ligne.
fn rich_activity(store: &Store, user: u64) -> Option<Activity> {
    let mut people = store.people.lock();
    let p = people.presence(user)?;
    if p.status == crate::state::people::Status::Offline {
        return None;
    }
    p.activities.iter().find(|a| rich(a)).cloned()
}

/// Cartes de la colonne : les salons vocaux d'abord (amis du même salon
/// réunis), puis les activités enrichies ; amis rangés par nom.
pub(super) fn cards(store: &Store) -> Vec<Card> {
    let mut friends: Vec<u64> = store.relations.friends().map(|r| r.id).collect();
    friends.sort_by_cached_key(|id| (store.relations.display_name(*id).to_lowercase(), *id));
    let mut out: Vec<Card> = Vec::new();
    for id in &friends {
        let Some(s) = store.voice.state(*id) else { continue };
        let Some(guild) = s.guild_id else { continue };
        // Salon inconnu ou que je ne vois pas : rien n'est montré.
        if store.voice.channel(s.channel_id).is_none() || store.voice.hidden(s.channel_id) {
            continue;
        }
        match out.iter_mut().find(|c| matches!(c, Card::Voice { channel, .. } if *channel == s.channel_id)) {
            Some(Card::Voice { friends, .. }) => friends.push(*id),
            _ => out.push(Card::Voice { guild, channel: s.channel_id, friends: vec![*id] }),
        }
    }
    out.extend(friends.iter().filter(|id| rich_activity(store, **id).is_some()).map(|id| Card::Activity { user: *id }));
    out
}

impl Cache {
    /// Cartes, refaites seulement quand relations, présences ou états vocaux changent.
    pub(super) fn cards(&mut self, store: &Store) -> Vec<Card> {
        let rev = (store.relations.rev(), store.people.lock().presence_rev(), store.voice.rev());
        if self.0.as_ref().is_none_or(|(r, _)| *r != rev) {
            self.0 = Some((rev, cards(store)));
        }
        // Tous les amis comptent comme affichés : une activité lancée hors de la vue réveille la page.
        let mut people = store.people.lock();
        for r in store.relations.friends() {
            people.watch(r.id);
        }
        self.0.as_ref().map(|(_, c)| c.clone()).unwrap_or_default()
    }
}

/// « A », « A et B », « A, B et C ».
pub(super) fn joined(lang: Lang, names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [head @ .., last] => vf::and(lang, head.join(", "), last),
    }
}

/// Durée ronde d'un résumé : « 12 min », « 5 h » (rien sous la minute).
pub(super) fn rough(ms: i64) -> Option<String> {
    let min = ms / 60_000;
    match min {
        ..=0 => None,
        1..=59 => Some(format!("{min}\u{a0}min")),
        _ => Some(format!("{}\u{a0}h", min / 60)),
    }
}

/// Chrono du client : `mm:ss`, ou `hh:mm:ss` au-delà d'une heure, chiffres toujours par deux.
pub(super) fn clock(ms: i64) -> String {
    let s = ms.max(0) / 1000;
    let (h, m, s) = (s / 3600, (s / 60) % 60, s % 60);
    if h > 0 { format!("{h:02}:{m:02}:{s:02}") } else { format!("{m:02}:{s:02}") }
}

/// Texte d'une ligne, coupé à `width`, emojis en image ; rend sa hauteur.
fn text(ui: &egui::Ui, at: egui::Pos2, s: &str, font: FontId, color: Color32, width: f32) -> f32 {
    let (galley, spots) = super::emoji::truncated(ui, s, &font, color, width.max(1.0));
    let h = galley.size().y;
    super::emoji::paint_spots(ui, at, &galley, &spots);
    ui.painter().galley(at, galley, color);
    h
}

/// Hauteur d'une ligne de texte de cette police.
fn row(ui: &egui::Ui, font: &FontId) -> f32 {
    ui.fonts_mut(|f| f.row_height(font))
}

impl App {
    /// Colonne « En ligne » : titre, puis les cartes (ou la phrase d'attente).
    pub(super) fn active_column(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        ui.label(RichText::new(ut::ACTIVE_NOW.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
        ui.add_space(f32::from(PAGE_Y));
        let list = self.friends.active.cards(&self.store);
        if list.is_empty() {
            ui.vertical_centered(|ui| {
                ui.add_space(f32::from(PAGE_Y));
                ui.label(RichText::new(ut::ACTIVE_NONE_TITLE.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
                ui.add(egui::Label::new(RichText::new(ut::ACTIVE_NONE.get(lang)).size(t.typography.body - 2.0).color(t.colors.muted)).wrap().halign(Align::Center));
            });
            return;
        }
        let mut open = None;
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = f32::from(PAGE_Y);
            for card in &list {
                let (who, resp) = match card {
                    Card::Voice { guild, channel, friends } => (friends.first().copied(), self.voice_card(ui, t, *guild, *channel, friends)),
                    Card::Activity { user } => (Some(*user), self.activity_card(ui, t, *user)),
                };
                let (Some(who), Some(resp)) = (who, resp) else { continue };
                let name = self.store.relations.display_name(who).to_string();
                let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
                // Lecteur d'écran : le nom, puis ce que fait la personne (« Joue à … », « Dans un salon vocal »).
                let doing = match card {
                    Card::Voice { .. } => vt::IN_VOICE.get(lang).to_string(),
                    Card::Activity { user } => super::friends::presence_line(lang, &presence(&self.store, *user)),
                };
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{name}, {doing}")));
                super::keys::focus_ring(ui, t, &resp);
                if resp.clicked() {
                    open = Some(who);
                }
                if resp.secondary_clicked() {
                    super::profile::open_menu(ui.ctx(), who, &name, resp.rect);
                }
            }
        });
        if let Some(id) = open
            && self.store.open_dm_with(id)
        {
            self.scroll_to_bottom = true;
        }
    }

    /// Place et fond d'une carte : le haut (clair) et le bas (sombre), coins
    /// arrondis, bordure fine. Rend les deux zones utiles et la réponse.
    fn card_frame(&self, ui: &mut egui::Ui, t: &Tokens, key: (u8, u64), top_h: f32, bottom_h: f32) -> (Rect, Rect, egui::Response) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), top_h + bottom_h), Sense::hover());
        let resp = ui.interact(rect, ui.scope_id().with(("en-ligne", key)), Sense::click());
        let (top, bottom) = rect.split_top_bottom_at_y(rect.top() + top_h);
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            let hot = resp.hovered() || resp.has_focus();
            // Le bas a la couleur de la page ; le haut, celle des panneaux, un peu plus claire.
            p.rect_filled(rect, CornerRadius::same(RADIUS), t.colors.bg2);
            let lit = if hot { super::motion::mix(t.colors.bg1, t.colors.row_hover, 0.5) } else { t.colors.bg1 };
            p.rect_filled(top, CornerRadius { nw: RADIUS, ne: RADIUS, sw: 0, se: 0 }, lit);
            p.rect_stroke(rect, CornerRadius::same(RADIUS), Stroke::new(1.0, t.colors.separator), egui::StrokeKind::Inside);
        }
        (top.shrink(PAD), bottom.shrink(PAD), resp)
    }

    /// Haut d'une carte : avatar et pastille de `user`, `title` en gras,
    /// `summary` dessous ; `mark` (icône d'application) à droite s'il y en a une.
    #[allow(clippy::too_many_arguments)]
    fn card_head(&self, ui: &egui::Ui, t: &Tokens, area: Rect, user: u64, title: &str, summary: &str, mark: Option<crate::state::model::ImageRef>) {
        let social = &self.store.relations;
        let side = t.layout.small_avatar;
        let avatar = pixel_square(ui, egui::pos2(area.left(), area.center().y - side / 2.0), side);
        if t.avatars_shown() {
            let image = social.person(user).map_or_else(|| Avatar::default_for(user), |p| p.avatar);
            paint_avatar(ui, &self.images, t, avatar, &image, social.display_name(user));
        }
        paint_status(ui, t, avatar, presence(&self.store, user).status, t.colors.bg1);
        let mut right = area.right();
        if let Some(mark) = mark {
            let at = pixel_square(ui, egui::pos2(right - side, area.center().y - side / 2.0), side);
            if paint_image(ui, &self.images, at, Some(mark), CornerRadius::same(TILE_RADIUS)) {
                right = at.left() - t.spacing.item_x;
            }
        }
        let left = avatar.right() + t.spacing.item_x * 1.5;
        let width = right - left;
        let name_font = FontId::new(t.dm_text(), crate::ui::fonts::demi());
        let sum_font = FontId::proportional(t.typography.body - 1.0);
        let block = row(ui, &name_font) + row(ui, &sum_font);
        let y = area.center().y - block / 2.0;
        let h = text(ui, egui::pos2(left, y), title, name_font, t.colors.text_strong, width);
        text(ui, egui::pos2(left, y + h), summary, sum_font, t.colors.muted, width);
    }

    /// Carte d'un salon vocal où sont des amis : « A et B », « Dans un salon
    /// vocal » ; en bas l'icône du serveur (pastille haut-parleur), le nom du
    /// salon, celui du serveur, et les avatars des amis à droite.
    fn voice_card(&self, ui: &mut egui::Ui, t: &Tokens, guild: u64, channel: u64, friends: &[u64]) -> Option<egui::Response> {
        let lang = self.lang;
        let social = &self.store.relations;
        let first = *friends.first()?;
        let chan = self.store.voice.channel(channel)?;
        let g = self.store.guilds.iter().find(|g| g.id == guild)?;
        let guild_name = super::words::guild_name(lang, &g.name);
        let side = t.layout.small_avatar;
        let (top, bottom, resp) = self.card_frame(ui, t, (0, channel), side + 2.0 * PAD, side + 2.0 * PAD);
        if !ui.is_rect_visible(resp.rect) {
            return Some(resp);
        }
        let names: Vec<&str> = friends.iter().map(|id| social.display_name(*id)).collect();
        self.card_head(ui, t, top, first, &joined(lang, &names), vt::IN_VOICE.get(lang), None);

        // Icône du serveur, et sa pastille haut-parleur au coin bas droit.
        let icon = pixel_square(ui, egui::pos2(bottom.left(), bottom.center().y - side / 2.0), side);
        if !paint_image(ui, &self.images, icon, g.icon, CornerRadius::same(TILE_RADIUS + 2)) {
            let p = ui.painter();
            p.rect_filled(icon, CornerRadius::same(TILE_RADIUS + 2), t.colors.pill);
            p.text(icon.center(), egui::Align2::CENTER_CENTER, super::util::guild_acronym(guild_name), crate::ui::fonts::demi_font(t.typography.small), t.colors.text_strong);
        }
        let dot = (side * 0.5).round();
        let badge = Rect::from_min_size(icon.max - egui::Vec2::splat(dot * 0.75), egui::Vec2::splat(dot));
        ui.painter().circle(badge.center(), dot / 2.0, t.colors.bg3, Stroke::new(2.0, t.colors.bg2));
        icons::paint(Icon::Speaker, ui.painter(), badge.shrink(dot * 0.2), t.colors.text_strong);

        // Avatars des amis, à droite, qui se recouvrent un peu.
        let shown = friends.len().min(FACES);
        let faces_w = if t.avatars_shown() { FACE + (shown.saturating_sub(1)) as f32 * (FACE - FACE_OVERLAP) } else { 0.0 };
        if t.avatars_shown() {
            let mut x = bottom.right() - faces_w;
            for id in friends.iter().take(shown) {
                let at = pixel_square(ui, egui::pos2(x, bottom.center().y - FACE / 2.0), FACE);
                ui.painter().circle_filled(at.center(), FACE / 2.0 + 2.0, t.colors.bg2);
                let image = social.person(*id).map_or_else(|| Avatar::default_for(*id), |p| p.avatar);
                paint_avatar(ui, &self.images, t, at, &image, social.display_name(*id));
                x += FACE - FACE_OVERLAP;
            }
        }
        let left = icon.right() + t.spacing.item_x * 1.5;
        let width = bottom.right() - faces_w - t.spacing.item_x - left;
        let title = FontId::new(t.typography.body - 1.0, crate::ui::fonts::demi());
        let small = FontId::proportional(t.typography.small + 1.0);
        let y = bottom.center().y - (row(ui, &title) + row(ui, &small)) / 2.0;
        let h = text(ui, egui::pos2(left, y), &chan.name, title, t.colors.text_strong, width);
        text(ui, egui::pos2(left, y + h), guild_name, small, t.colors.muted, width);
        Some(resp)
    }

    /// Carte d'une activité enrichie : nom, « Jeu – 5 h », icône de
    /// l'application ; en bas son image (petite image en coin), le titre, la
    /// sous-ligne et le temps écoulé.
    fn activity_card(&self, ui: &mut egui::Ui, t: &Tokens, user: u64) -> Option<egui::Response> {
        let lang = self.lang;
        let a = rich_activity(&self.store, user)?;
        let now = self.store.now();
        let elapsed = a.start_ms.filter(|s| *s <= now).map(|s| now - s);
        let title_font = FontId::new(t.typography.body - 1.0, crate::ui::fonts::demi());
        let small = FontId::proportional(t.typography.small + 1.0);
        let title = a.details.clone().unwrap_or_else(|| a.name.clone());
        let lines = row(ui, &title_font) + if a.state.is_some() { row(ui, &small) } else { 0.0 } + if elapsed.is_some() { row(ui, &small) } else { 0.0 };
        let side = t.layout.small_avatar;
        let (top, bottom, resp) = self.card_frame(ui, t, (1, user), side + 2.0 * PAD, lines.max(side) + 2.0 * PAD);
        if !ui.is_rect_visible(resp.rect) {
            return Some(resp);
        }
        // « Jeu – 5 h » pour un jeu ; une autre activité en mots (« Écoute … »).
        let what = if a.kind == ActivityKind::Playing { a.name.clone() } else { super::profile::activity_line(lang, &a) };
        let summary = match elapsed.and_then(rough) {
            Some(d) => format!("{what} \u{2013} {d}"),
            None => what,
        };
        // Icône de l'application : seulement si la liste des jeux déjà chargée la connaît.
        let rich = a.rich();
        let app_icon = rich.and_then(|r| r.app_id).and_then(|id| self.store.activity.app_icon(id));
        self.card_head(ui, t, top, user, self.store.relations.display_name(user), &summary, app_icon);

        let image = pixel_square(ui, egui::pos2(bottom.left(), bottom.top()), side);
        let large = rich.and_then(|r| r.large.as_ref()).filter(|i| i.url.is_some());
        let drawn = large.is_some_and(|l| super::activity::paint_asset(ui, &self.images, t, image, l)) || paint_image(ui, &self.images, image, app_icon, CornerRadius::same(TILE_RADIUS));
        if !drawn {
            ui.painter().rect_filled(image, CornerRadius::same(TILE_RADIUS), t.colors.bg3);
            super::user_panel::paint_gamepad(ui.painter(), image.shrink(side * 0.25), t.colors.muted, t.colors.bg3);
        }
        if let Some(small_img) = rich.and_then(|r| r.small.as_ref()).filter(|i| i.url.is_some()) {
            let s = (side * 0.45).round();
            let corner = Rect::from_min_size(image.max - egui::Vec2::splat(s * 0.7), egui::Vec2::splat(s));
            ui.painter().circle_filled(corner.center(), s / 2.0 + 2.0, t.colors.bg2);
            super::activity::paint_asset(ui, &self.images, t, corner, small_img);
        }
        let left = image.right() + t.spacing.item_x * 1.5;
        let width = bottom.right() - left;
        let mut y = bottom.top() + ((lines.max(side) - lines) / 2.0).max(0.0);
        y += text(ui, egui::pos2(left, y), &title, title_font, t.colors.text_strong, width);
        if let Some(state) = &a.state {
            y += text(ui, egui::pos2(left, y), state, small.clone(), t.colors.muted, width);
        }
        if let Some(ms) = elapsed {
            text(ui, egui::pos2(left, y), &vf::elapsed(lang, clock(ms)), small, t.colors.muted, width);
            // Le chrono avance : une image par seconde tant que la carte est à l'écran.
            ui.ctx().request_repaint_after(std::time::Duration::from_secs(1));
        }
        Some(resp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::people::{Activity as A, ActivityKind as K, ClientStatus, Presence as P, Rich, Status};
    use crate::state::relations::{Person, RelKind, Relation, RelationsEvent};
    use crate::state::voice::{VoiceChannel, VoiceEvent, VoiceState};
    use crate::state::Event;

    fn store(friends: &[(u64, &str)]) -> Store {
        use crate::state::model::User;
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        let relations = friends
            .iter()
            .map(|(id, name)| Relation { id: *id, kind: RelKind::Friend, nickname: None, ignored: false, stranger: false, user: Some(Person { id: *id, name: (*name).into(), username: name.to_lowercase(), avatar: Avatar::default_for(*id) }) })
            .collect();
        s.apply(Event::Relations(RelationsEvent::Ready { relations, people: vec![], dms: vec![], reads: Some(vec![]), heads: vec![] }));
        s
    }

    fn game(details: Option<&str>) -> A {
        A { kind: K::Playing, name: "Jeu".into(), details: details.map(str::to_string), state: None, emoji: None, start_ms: None, rich: None }
    }

    fn online(s: &Store, user: u64, activities: Vec<A>) {
        s.people.lock().set_presence(user, P { status: Status::Online, client: ClientStatus::default(), activities });
    }

    /// Activité enrichie seulement : un jeu nu ou un statut personnalisé ne font pas de carte.
    #[test]
    fn only_rich_activities_make_a_card() {
        let s = store(&[(2, "Alice"), (3, "Bob"), (4, "Chloé")]);
        assert!(cards(&s).is_empty());
        online(&s, 2, vec![game(None)]);
        online(&s, 3, vec![game(Some("En partie"))]);
        online(&s, 4, vec![A { kind: K::Custom, name: "Custom Status".into(), details: None, state: Some("Coucou".into()), emoji: None, start_ms: None, rich: None }]);
        assert_eq!(cards(&s), vec![Card::Activity { user: 3 }]);
        // Une image suffit, même sans détails.
        let with_image = A { rich: Some(Box::new(Rich { large: Some(crate::state::people::ActivityImage { url: Some("https://cdn.discordapp.com/app-assets/1/2.png".into()), text: None }), ..Rich::default() })), ..game(None) };
        online(&s, 2, vec![with_image]);
        assert_eq!(cards(&s), vec![Card::Activity { user: 2 }, Card::Activity { user: 3 }]);
        // Hors ligne : plus de carte.
        s.people.lock().set_presence(3, P { status: Status::Offline, client: ClientStatus::default(), activities: vec![game(Some("En partie"))] });
        assert_eq!(cards(&s), vec![Card::Activity { user: 2 }]);
    }

    /// Les amis du même salon vocal partagent une carte ; un appel privé n'en fait pas.
    #[test]
    fn friends_of_one_voice_channel_share_a_card() {
        let mut s = store(&[(2, "Alice"), (3, "Bob"), (4, "Chloé")]);
        let chan = |id: u64| VoiceChannel { id, guild_id: 10, name: format!("Vocal {id}"), stage: false, position: 0, parent: None, overwrites: None, user_limit: 0 };
        let st = |user: u64, guild: Option<u64>, channel: u64| VoiceState { user_id: user, guild_id: guild, channel_id: channel, ..Default::default() };
        s.apply(Event::Voice(VoiceEvent::Guild { guild_id: 10, channels: vec![chan(20), chan(21)], states: vec![st(3, Some(10), 20), st(2, Some(10), 20), st(9, Some(10), 20)] }));
        s.apply(Event::Voice(VoiceEvent::States(vec![st(4, None, 99)])));
        assert_eq!(cards(&s), vec![Card::Voice { guild: 10, channel: 20, friends: vec![2, 3] }]);
        // Le cache suit la révision des états vocaux.
        let mut cache = Cache::default();
        assert_eq!(cache.cards(&s).len(), 1);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(10), user_id: 3, state: Some(st(3, Some(10), 21)), session_id: None }));
        assert_eq!(cache.cards(&s), vec![Card::Voice { guild: 10, channel: 20, friends: vec![2] }, Card::Voice { guild: 10, channel: 21, friends: vec![3] }]);
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(10), user_id: 2, state: None, session_id: None }));
        assert_eq!(cache.cards(&s), vec![Card::Voice { guild: 10, channel: 21, friends: vec![3] }]);
    }

    #[test]
    fn names_durations_and_clock() {
        assert_eq!(joined(Lang::Fr, &["Alice"]), "Alice");
        assert_eq!(joined(Lang::Fr, &["Alice", "Bob"]), "Alice et Bob");
        assert_eq!(joined(Lang::En, &["Alice", "Bob", "Chloé"]), "Alice, Bob and Chloé");
        assert_eq!(rough(20_000), None);
        assert_eq!(rough(12 * 60_000).as_deref(), Some("12\u{a0}min"));
        assert_eq!(rough(5 * 3_600_000 + 7 * 60_000).as_deref(), Some("5\u{a0}h"));
        assert_eq!(clock(5 * 3_600_000 + 7 * 60_000 + 2_000), "05:07:02");
        assert_eq!(clock(25 * 60_000 + 3_000), "25:03");
    }
}
