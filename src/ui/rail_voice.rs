//! Présence vocale dans le rail des serveurs (lot « vocal ») : pastille au
//! coin haut droit de l'icône d'un serveur (haut-parleur quand un vocal est
//! en cours, écran quand quelqu'un partage), et infobulle à droite de
//! l'icône, avec sa petite flèche : le nom du serveur en gras, puis une ligne
//! haut-parleur et les avatars de ceux qui sont en vocal, une ligne écran et
//! les avatars de ceux qui partagent.
//!
//! Règle retenue (déduite des captures du client, faute de source publique
//! qui la précise) : seuls mes AMIS comptent. Un serveur où des inconnus sont
//! en vocal ne porte pas de pastille.
//!
//! Données : les états vocaux déjà reçus (READY, GUILD_CREATE,
//! VOICE_STATE_UPDATE) et la liste d'amis. Rien n'est demandé au réseau.

use super::{
    motion,
    util::{paint_avatar, pixel_square},
    App,
};
use crate::state::model::Avatar;
use crate::state::Store;
use crate::theme::{icons, icons::Icon, Elevation, Tokens};
use eframe::egui::{self, pos2, vec2, CornerRadius, Id, Rect, Shape, Stroke};
use std::collections::HashMap;

/// Écart entre l'icône et la pointe de la flèche, et hauteur de la flèche.
const GAP: f32 = 6.0;
const ARROW: f32 = 6.0;
/// Largeur du nom du serveur, au plus.
const NAME_WIDTH: f32 = 220.0;
/// Avatars d'une ligne : côté, écart, nombre au plus (puis « +N »).
const FACE: f32 = 24.0;
const FACE_GAP: f32 = 4.0;
const FACES: usize = 6;

/// Amis en vocal sur un serveur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Live {
    /// En vocal, sans partager leur écran.
    pub voice: Vec<u64>,
    /// Qui partagent leur écran (`self_stream`).
    pub sharing: Vec<u64>,
}

/// Présence vocale de mes amis, par serveur ; refaite seulement quand les
/// relations ou les états vocaux changent.
#[derive(Default)]
pub(super) struct RailVoice {
    rev: Option<(u64, u64)>,
    by_guild: HashMap<u64, Live>,
}

/// Amis en vocal par serveur, rangés par nom. Un salon que je ne vois pas ne compte pas.
pub(super) fn live(store: &Store) -> HashMap<u64, Live> {
    let mut friends: Vec<u64> = store.relations.friends().map(|r| r.id).collect();
    friends.sort_by_cached_key(|id| (store.relations.display_name(*id).to_lowercase(), *id));
    let mut out: HashMap<u64, Live> = HashMap::new();
    for id in friends {
        let Some(s) = store.voice.state(id) else { continue };
        let Some(guild) = s.guild_id else { continue };
        // Salon inconnu ou que je ne vois pas : il ne compte pas.
        if store.voice.channel(s.channel_id).is_none() || store.voice.hidden(s.channel_id) {
            continue;
        }
        let entry = out.entry(guild).or_default();
        if s.self_stream { entry.sharing.push(id) } else { entry.voice.push(id) }
    }
    out
}

impl RailVoice {
    pub(super) fn refresh(&mut self, store: &Store) {
        let rev = (store.relations.rev(), store.voice.rev());
        if self.rev != Some(rev) {
            self.rev = Some(rev);
            self.by_guild = live(store);
        }
    }

    pub(super) fn get(&self, guild: u64) -> Option<&Live> {
        self.by_guild.get(&guild)
    }
}

impl App {
    /// Pastille de présence vocale au coin haut droit de la pastille `tile` :
    /// écran si quelqu'un partage, sinon haut-parleur ; détourée du fond du rail.
    pub(super) fn rail_live_badge(&self, ui: &egui::Ui, tile: Rect, live: &Live) {
        if !ui.is_rect_visible(tile) {
            return;
        }
        let t = &self.theme;
        let d = (tile.width() * 5.0 / 12.0).round();
        // À cheval sur le coin, comme la pastille des mentions en bas.
        let out = (d / 4.0).round();
        let at = Rect::from_min_size(pos2(tile.right() - d + out, tile.top() - out), egui::Vec2::splat(d));
        let sharing = !live.sharing.is_empty();
        // Partage : pastille claire, icône sombre ; vocal : pastille sombre, icône claire.
        let (fill, ink, icon) = if sharing { (t.colors.text_strong, t.colors.bg0, Icon::Screen) } else { (t.colors.bg3, t.colors.text_strong, Icon::Speaker) };
        let p = ui.painter();
        p.circle(at.center(), d / 2.0, fill, Stroke::new((d / 7.0).round().max(2.0), t.colors.bg0));
        icons::paint(icon, p, pixel_square(ui, at.min + egui::Vec2::splat(d * 0.2), d * 0.6), ink);
    }

    /// Infobulle d'une entrée du rail, à droite de `resp` quand elle est
    /// survolée : `name` en gras, puis les amis en vocal et ceux qui partagent.
    pub(super) fn rail_tip(&self, resp: &egui::Response, name: &str, live: Option<&Live>) {
        if name.is_empty() || !resp.hovered() || resp.ctx.input(|i| i.pointer.any_down()) || resp.context_menu_opened() {
            return;
        }
        let t = &self.theme;
        let ctx = &resp.ctx;
        let font = egui::FontId::new(t.typography.body, crate::ui::fonts::demi());
        let galley = ctx.fonts_mut(|f| f.layout(name.to_owned(), font, t.colors.text_strong, NAME_WIDTH));
        let rows: Vec<(Icon, &[u64])> = live
            .map(|l| [(Icon::Speaker, l.voice.as_slice()), (Icon::Screen, l.sharing.as_slice())])
            .into_iter()
            .flatten()
            .filter(|(_, who)| !who.is_empty())
            .collect();
        let pad = vec2(t.spacing.control_x.min(12.0), t.spacing.item_y * 2.0);
        let row_gap = t.spacing.item_y * 1.5;
        let icon_side = Tokens::icon_side(t.typography.body);
        let row_w = |n: usize| icon_side + t.spacing.item_x + n.min(FACES) as f32 * (FACE + FACE_GAP) + if n > FACES { FACE } else { 0.0 };
        let width = rows.iter().map(|(_, who)| row_w(who.len())).fold(galley.size().x, f32::max);
        let height = galley.size().y + rows.len() as f32 * (row_gap + FACE);
        let size = vec2(width, height) + 2.0 * pad;
        let target = resp.rect;
        let screen = ctx.content_rect();
        let y = (target.center().y - size.y / 2.0).clamp(screen.top() + 4.0, (screen.bottom() - 4.0 - size.y).max(screen.top() + 4.0));
        let rect = Rect::from_min_size(pos2(target.right() + GAP + ARROW, y), size);
        let tip = pos2(target.right() + GAP, target.center().y);
        let id = Id::unique("ferrocord-infobulle-rail");
        let layer = egui::LayerId::new(egui::Order::Tooltip, id);
        // Une clé par entrée visée : passer d'un serveur au voisin rejoue l'apparition.
        motion::pop(ctx, layer, Id::unique(("infobulle-rail", resp.id)), tip, 0, motion::dur::POP);
        egui::Area::new(id).order(egui::Order::Tooltip).fixed_pos(rect.min).interactable(false).show(ctx, |ui| {
            ui.allocate_rect(rect, egui::Sense::hover());
            let p = ui.painter().clone();
            let r = t.r_embed();
            p.add(t.shadow(Elevation::Menu).as_shape(rect, r));
            p.rect_filled(rect, r, t.colors.menu_bg);
            p.rect_stroke(rect, r, t.border(), egui::StrokeKind::Inside);
            // Flèche : triangle dont la base est sur le bord gauche de la bulle.
            let base = pos2(rect.left() + 1.0, tip.y);
            p.add(Shape::convex_polygon(vec![base - vec2(0.0, ARROW), base + vec2(0.0, ARROW), tip], t.colors.menu_bg, Stroke::NONE));
            let mut y = rect.top() + pad.y;
            let left = rect.left() + pad.x;
            let name_h = galley.size().y;
            p.galley(pos2(left, y), galley, t.colors.text_strong);
            y += name_h;
            for (icon, who) in rows {
                y += row_gap;
                let at = pixel_square(ui, pos2(left, y + (FACE - icon_side) / 2.0), icon_side);
                icons::paint(icon, &p, at, t.colors.muted);
                let mut x = at.right() + t.spacing.item_x;
                for user in who.iter().take(FACES) {
                    let face = pixel_square(ui, pos2(x, y), FACE);
                    let social = &self.store.relations;
                    let image = social.person(*user).map_or_else(|| Avatar::default_for(*user), |p| p.avatar);
                    if t.avatars_shown() {
                        paint_avatar(ui, &self.images, t, face, &image, social.display_name(*user));
                    } else {
                        p.circle_filled(face.center(), FACE / 2.0, t.colors.bg3);
                    }
                    x += FACE + FACE_GAP;
                }
                if who.len() > FACES {
                    let more = Rect::from_min_size(pos2(x, y), egui::Vec2::splat(FACE));
                    p.rect_filled(more, CornerRadius::same((FACE / 2.0) as u8), t.colors.bg3);
                    p.text(more.center(), egui::Align2::CENTER_CENTER, format!("+{}", who.len() - FACES), egui::FontId::proportional(t.typography.small), t.colors.text);
                }
                y += FACE;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::relations::{Person, RelKind, Relation, RelationsEvent};
    use crate::state::voice::{VoiceChannel, VoiceEvent, VoiceState};
    use crate::state::Event;

    /// Seuls les amis comptent ; ceux qui partagent sont à part ; un appel privé ne compte pas.
    #[test]
    fn only_friends_light_a_guild() {
        use crate::state::model::User;
        let mut s = Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        let friend = |id: u64, name: &str| Relation { id, kind: RelKind::Friend, nickname: None, ignored: false, stranger: false, user: Some(Person { id, name: name.into(), username: name.to_lowercase(), avatar: Avatar::default_for(id) }) };
        s.apply(Event::Relations(RelationsEvent::Ready { relations: vec![friend(3, "Bob"), friend(2, "Alice"), friend(4, "Chloé")], people: vec![], dms: vec![], reads: Some(vec![]), heads: vec![] }));
        let chan = |guild: u64, id: u64| VoiceChannel { id, guild_id: guild, name: "Vocal".into(), stage: false, position: 0, parent: None, overwrites: None, user_limit: 0 };
        let st = |user: u64, guild: Option<u64>, channel: u64, stream: bool| VoiceState { user_id: user, guild_id: guild, channel_id: channel, self_stream: stream, ..Default::default() };
        // Serveur 10 : deux amis (l'un partage) et un inconnu ; serveur 11 : un inconnu seul.
        s.apply(Event::Voice(VoiceEvent::Guild { guild_id: 10, channels: vec![chan(10, 20)], states: vec![st(3, Some(10), 20, false), st(2, Some(10), 20, true), st(9, Some(10), 20, false)] }));
        s.apply(Event::Voice(VoiceEvent::Guild { guild_id: 11, channels: vec![chan(11, 21)], states: vec![st(8, Some(11), 21, true)] }));
        s.apply(Event::Voice(VoiceEvent::States(vec![st(4, None, 99, false)])));
        let mut rail = RailVoice::default();
        rail.refresh(&s);
        assert_eq!(rail.get(10), Some(&Live { voice: vec![3], sharing: vec![2] }));
        assert_eq!(rail.get(11), None, "des inconnus en vocal : pas de pastille");
        // Départ : la révision des états vocaux refait la table.
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(10), user_id: 2, state: None, session_id: None }));
        s.apply(Event::Voice(VoiceEvent::StateUpdate { guild_id: Some(10), user_id: 3, state: None, session_id: None }));
        rail.refresh(&s);
        assert_eq!(rail.get(10), None);
    }
}
