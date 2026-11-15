//! Plugin « Mode discret » (lot E3) : trois abstentions indépendantes.
//!
//! - « En train d'écrire » n'est plus envoyé ([`App::stealth_no_typing`],
//!   lu par la zone de saisie).
//! - Un salon ouvert n'est plus accusé tout seul : `state::reads` ne prévoit
//!   plus d'accusé (`Reads::set_manual`), les pastilles locales restent, et
//!   l'en-tête montre « Marquer comme lu » ([`App::stealth_read_button`]).
//! - Les médias du fil sont brouillés jusqu'au survol ou au clic
//!   ([`veiled`], [`mosaic`]) : une mosaïque dessinée avec la texture déjà
//!   chargée, sans image de plus ni calcul hors du dessin.
//!
//! Ce sont des requêtes en moins, jamais en plus. Les réglages sont relus
//! une fois quand ils changent ([`App::stealth_apply`], appelé par
//! `plugins_sync`) : plugin coupé, rien n'est lu ni fait par image.

use super::App;
use crate::i18n::table::e3::t as xt;
use crate::plugins::{self, Id as Plugin};
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, pos2, Color32, Id, Rect, Vec2};

/// Comment un média flouté se dévoile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Blur {
    #[default]
    Off,
    Hover,
    Click,
}

/// Réglages du mode discret en vigueur (tous à faux quand le plugin est coupé).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(in crate::ui) struct StealthUi {
    no_typing: bool,
    manual_read: bool,
    blur: Blur,
    blur_guilds_only: bool,
    /// Aperçu hors ligne : « Marquer comme lu » montré même si tout est lu.
    preview: bool,
}

fn blur_id() -> Id {
    Id::unique("ferrocord.discret.flou")
}

/// Flou des médias du salon affiché : posé par la liste des messages avant
/// de la dessiner (comme `attachments::set_sensitive`).
pub(super) fn set_blur(ctx: &egui::Context, mode: Blur) {
    if blur(ctx) != mode {
        ctx.data_mut(|d| d.insert_temp(blur_id(), mode));
    }
}

fn blur(ctx: &egui::Context) -> Blur {
    ctx.data(|d| d.get_temp::<Blur>(blur_id())).unwrap_or_default()
}

/// Clé de dévoilement (au clic) d'un média flouté, distincte de celle du spoiler.
pub(super) fn reveal_key(key: u64) -> u64 {
    key ^ 0x6469_7363_7265_7421
}

/// Le média de `rect` doit-il être dessiné flouté ? `revealed` : il a déjà
/// été dévoilé d'un clic. Rend aussi le mode, pour que l'appelant sache si
/// un clic dévoile.
pub(super) fn veiled(ui: &egui::Ui, rect: Rect, revealed: bool) -> Option<Blur> {
    match blur(ui.ctx()) {
        Blur::Off => None,
        Blur::Hover => (!ui.rect_contains_pointer(rect)).then_some(Blur::Hover),
        Blur::Click => (!revealed).then_some(Blur::Click),
    }
}

/// Cases de la mosaïque sur le grand côté.
const CELLS: f32 = 9.0;

/// Dessine la texture `tex` (partie `uv`) brouillée dans `rect` : une
/// mosaïque de grandes cases d'une seule couleur chacune, doublée d'une
/// seconde décalée d'une demi-case, sous un voile et un œil.
pub(super) fn mosaic(ui: &egui::Ui, t: &Tokens, rect: Rect, tex: egui::TextureId, uv: Rect) {
    let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    let cell = (rect.width().max(rect.height()) / CELLS).max(4.0);
    let mut mesh = egui::Mesh::with_texture(tex);
    let mut layer = |shift: f32, color: Color32| {
        let mut y = rect.top() - shift;
        while y < rect.bottom() {
            let mut x = rect.left() - shift;
            while x < rect.right() {
                let c = Rect::from_min_size(pos2(x, y), Vec2::splat(cell));
                let mid = c.center().clamp(rect.min, rect.max);
                let at = pos2(uv.left() + (mid.x - rect.left()) / rect.width().max(1.0) * uv.width(), uv.top() + (mid.y - rect.top()) / rect.height().max(1.0) * uv.height());
                let i = mesh.vertices.len() as u32;
                for corner in [c.left_top(), c.right_top(), c.right_bottom(), c.left_bottom()] {
                    mesh.vertices.push(egui::epaint::Vertex { pos: corner, uv: at, color });
                }
                mesh.indices.extend_from_slice(&[i, i + 1, i + 2, i, i + 2, i + 3]);
                x += cell;
            }
            y += cell;
        }
    };
    layer(0.0, Color32::WHITE);
    layer(cell / 2.0, Color32::from_white_alpha(128));
    p.add(egui::Shape::mesh(mesh));
    p.rect_filled(rect, 0.0, t.media_veil().gamma_multiply(0.6));
    let side = (Tokens::icon_side(t.typography.title) * 1.6).min(rect.width().min(rect.height()) * 0.6);
    let disc = Rect::from_center_size(rect.center(), Vec2::splat(side));
    p.circle_filled(disc.center(), side / 2.0, t.media_veil());
    icons::paint(Icon::Eye, &p, disc.shrink(side / 4.0), t.on_media());
}

impl App {
    /// Relit les réglages du mode discret (à chaque changement des plugins).
    pub(in crate::ui) fn stealth_apply(&mut self) {
        let now = if plugins::on(Plugin::Stealth) {
            StealthUi {
                no_typing: plugins::flag(Plugin::Stealth, plugins::STEALTH_TYPING),
                manual_read: plugins::flag(Plugin::Stealth, plugins::STEALTH_READ),
                blur: match (plugins::flag(Plugin::Stealth, plugins::STEALTH_BLUR), plugins::choice(Plugin::Stealth, plugins::STEALTH_REVEAL)) {
                    (false, _) => Blur::Off,
                    (true, plugins::REVEAL_CLICK) => Blur::Click,
                    (true, _) => Blur::Hover,
                },
                blur_guilds_only: plugins::flag(Plugin::Stealth, plugins::STEALTH_BLUR_GUILDS),
                preview: self.stealth.preview,
            }
        } else {
            StealthUi::default()
        };
        if now == self.stealth {
            return;
        }
        self.stealth = now;
        if self.store.reads.manual() != now.manual_read {
            self.store.reads.set_manual(now.manual_read);
        }
    }

    /// « En train d'écrire » ne doit pas partir.
    pub(super) fn stealth_no_typing(&self) -> bool {
        self.stealth.no_typing
    }

    /// Flou à appliquer aux médias du salon `cid`.
    pub(super) fn stealth_blur(&self, cid: u64) -> Blur {
        if self.stealth.blur == Blur::Off || (self.stealth.blur_guilds_only && self.store.dms.iter().any(|d| d.id == cid)) {
            return Blur::Off;
        }
        self.stealth.blur
    }

    /// Bouton « Marquer comme lu » de l'en-tête : seulement en lecture au
    /// clic, et tant que le salon affiché est non lu.
    pub(super) fn stealth_read_button(&mut self, ui: &mut egui::Ui, cid: u64) {
        if !self.stealth.manual_read || !(self.stealth.preview || self.store.unread().contains(&cid) || self.store.reads.unread(cid)) {
            return;
        }
        let t = self.theme.clone();
        if super::chat_head::head_icon_drawn(ui, &t, Icon::Check, xt::STEALTH_MARK_READ_TIP.get(self.lang), false).clicked() {
            self.store.mark_read(cid);
            self.store.dismiss_new_marker(cid);
            ui.ctx().request_repaint();
        }
    }

    /// Aperçu hors ligne : mode discret activé, médias floutés, bouton
    /// « Marquer comme lu » montré dans l'en-tête.
    #[doc(hidden)]
    pub fn stealth_preview(&mut self) {
        self.local.loaded = true;
        self.stealth.preview = true;
        let mut p = self.local.prefs.clone();
        p.plugins.set(Plugin::Stealth, true);
        p.plugins.put(Plugin::Stealth, plugins::STEALTH_BLUR, plugins::Value::Bool(true));
        self.set_local_prefs(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::Value;
    use crate::state::journal::tests_support::message;
    use crate::state::model::{Dm, User};
    use crate::state::relations::{ReadState, RelationsCmd, RelationsEvent};
    use crate::state::{Cmd, Event};
    use crate::ui::tests::{click, find, frame, offline_boot};
    use std::sync::{Arc, Mutex};

    const C: u64 = 100;

    #[derive(Clone, Default)]
    struct Net {
        events: Arc<Mutex<Vec<Event>>>,
        sent: Arc<Mutex<Vec<Cmd>>>,
    }

    impl crate::ui::Backend for Net {
        fn set_wake(&mut self, _: crate::ui::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            std::mem::take(&mut *self.events.lock().unwrap())
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            self.sent.lock().unwrap().extend(cmds);
        }
    }

    impl Net {
        fn typings(&self) -> usize {
            self.sent.lock().unwrap().iter().filter(|c| matches!(c, Cmd::Typing { .. })).count()
        }

        fn acks(&self) -> Vec<RelationsCmd> {
            self.sent.lock().unwrap().iter().filter_map(|c| if let Cmd::Relations(r @ (RelationsCmd::Ack { .. } | RelationsCmd::AckBulk(_))) = c { Some(r.clone()) } else { None }).collect()
        }
    }

    fn app(net: Net) -> App {
        let mut s = crate::state::Store::new();
        s.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: Vec::new(), dms: vec![Dm::new(C, "Alice".into(), 0)] });
        s.apply(Event::Relations(RelationsEvent::Ready { relations: vec![], people: vec![], dms: vec![], reads: Some(vec![ReadState { channel_id: C, last_acked: 10, mentions: 0 }]), heads: vec![(C, 10)] }));
        s.select_dms();
        s.select_channel(C);
        s.apply(Event::History { channel_id: C, before: None, messages: vec![message(10, C, "bonjour")] });
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(net), s, images));
        app.lang = crate::i18n::Lang::Fr;
        app.local.loaded = true;
        app
    }

    fn set(app: &mut App, key: &str, v: Value) {
        let mut p = app.local.prefs.clone();
        p.plugins.put(Plugin::Stealth, key, v);
        app.set_local_prefs(p);
    }

    fn type_one(app: &mut App) {
        app.input.clear();
        app.set_composer_text("");
        frame(app, Vec::new());
        frame(app, vec![egui::Event::Text("a".into())]);
        frame(app, Vec::new());
    }

    /// Plugin coupé : l'indicateur part comme avant. Activé : aucune requête
    /// `typing`. Son réglage coupé : il repart.
    #[test]
    fn typing_is_never_sent_in_stealth_mode() {
        let net = Net::default();
        let mut app = app(net.clone());
        assert!(!plugins::on(Plugin::Stealth), "coupé par défaut");
        type_one(&mut app);
        assert_eq!(net.typings(), 1, "plugin coupé : comportement d'origine");
        app.set_plugin(Plugin::Stealth, true);
        assert!(app.stealth_no_typing());
        type_one(&mut app);
        type_one(&mut app);
        assert_eq!(net.typings(), 1, "mode discret : rien de plus");
        set(&mut app, plugins::STEALTH_TYPING, Value::Bool(false));
        type_one(&mut app);
        assert_eq!(net.typings(), 2, "réglage coupé, les autres gardés");
        app.set_plugin(Plugin::Stealth, false);
        assert_eq!(app.stealth, StealthUi::default());
    }

    /// Lecture au clic : le salon ouvert et lu jusqu'en bas n'est jamais
    /// accusé tout seul ; le bouton de l'en-tête accuse, une fois.
    #[test]
    fn open_channel_is_acked_only_by_the_header_button() {
        let net = Net::default();
        let mut app = app(net.clone());
        app.set_plugin(Plugin::Stealth, true);
        assert!(app.store.reads.manual());
        net.events.lock().unwrap().push(Event::MessageCreate(message(20, C, "du nouveau")));
        let focus = || vec![egui::Event::WindowFocused(true)];
        for _ in 0..4 {
            frame(&mut app, focus());
        }
        std::thread::sleep(crate::state::reads::ACK_DELAY + std::time::Duration::from_millis(200));
        let out = frame(&mut app, focus());
        assert!(net.acks().is_empty(), "aucun accusé automatique : {:?}", net.acks());
        assert!(app.store.unread().contains(&C), "le salon ouvert reste non lu");
        let tip = xt::STEALTH_MARK_READ_TIP.get(app.lang);
        assert!(find(&out, tip).is_some(), "bouton « Marquer comme lu » dans l'en-tête");
        click(&mut app, tip);
        assert_eq!(net.acks(), vec![RelationsCmd::Ack { channel_id: C, message_id: 20 }]);
        assert!(!app.store.unread().contains(&C));
        let out = frame(&mut app, focus());
        assert!(find(&out, tip).is_none(), "plus rien à marquer");
        // Plugin coupé : pas de bouton, l'accusé groupé d'origine repart.
        app.set_plugin(Plugin::Stealth, false);
        assert!(!app.store.reads.manual());
        net.events.lock().unwrap().push(Event::MessageCreate(message(21, C, "encore")));
        for _ in 0..4 {
            frame(&mut app, focus());
        }
        std::thread::sleep(crate::state::reads::ACK_DELAY + std::time::Duration::from_millis(200));
        frame(&mut app, focus());
        frame(&mut app, focus());
        assert_eq!(net.acks().last(), Some(&RelationsCmd::Ack { channel_id: C, message_id: 21 }), "coupé : comportement d'origine");
    }

    /// Flou : coupé par défaut même en mode discret ; « seulement dans les
    /// serveurs » épargne les messages privés ; dévoilé au survol ou au clic.
    #[test]
    fn blur_follows_its_settings() {
        let mut app = app(Net::default());
        assert_eq!(app.stealth_blur(C), Blur::Off);
        app.set_plugin(Plugin::Stealth, true);
        assert_eq!(app.stealth_blur(C), Blur::Off, "réglage indépendant, coupé au départ");
        set(&mut app, plugins::STEALTH_BLUR, Value::Bool(true));
        assert_eq!((app.stealth_blur(C), app.stealth_blur(999)), (Blur::Hover, Blur::Hover));
        set(&mut app, plugins::STEALTH_REVEAL, Value::Str(plugins::REVEAL_CLICK.into()));
        assert_eq!(app.stealth_blur(C), Blur::Click);
        set(&mut app, plugins::STEALTH_BLUR_GUILDS, Value::Bool(true));
        assert_eq!((app.stealth_blur(C), app.stealth_blur(999)), (Blur::Off, Blur::Click), "le MP reste net");
        // Ce que le dessin en fait.
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(pos2(10.0, 10.0), Vec2::splat(100.0));
        let seen = |mode: Blur, pointer: Option<egui::Pos2>, revealed: bool| {
            let mut got = None;
            let events = pointer.map(egui::Event::PointerMoved).into_iter().collect();
            let mut out = ctx.run_ui(egui::RawInput { events, ..Default::default() }, |ui| {
                set_blur(ui.ctx(), mode);
                got = Some(veiled(ui, rect, revealed));
            });
            out.textures_delta.clear();
            got.unwrap()
        };
        assert_eq!(seen(Blur::Off, None, false), None);
        assert_eq!(seen(Blur::Click, None, false), Some(Blur::Click));
        assert_eq!(seen(Blur::Click, None, true), None, "dévoilé d'un clic : il le reste");
        seen(Blur::Hover, Some(pos2(50.0, 50.0)), false);
        assert_eq!(seen(Blur::Hover, Some(pos2(50.0, 50.0)), false), None, "survolé : net");
        seen(Blur::Hover, Some(pos2(500.0, 500.0)), false);
        assert_eq!(seen(Blur::Hover, Some(pos2(500.0, 500.0)), false), Some(Blur::Hover), "souris ailleurs : flouté");
        assert_ne!(reveal_key(7), 7);
    }
}
