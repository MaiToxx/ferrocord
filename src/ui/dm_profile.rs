//! Panneau de profil à droite d'un MP à deux (lot R du groupe 3), comme dans
//! Discord : le même contenu que la carte de profil (bannière et boutons,
//! avatar, bulle de statut, nom, badges, amis et serveurs en commun,
//! activité, bio, « Membre depuis », comptes connectés), « Voir le profil
//! complet » collé en bas. Il suit le bouton « profil » de l'en-tête du MP
//! (même réglage que la liste des membres) et s'efface dans une fenêtre
//! étroite. Le profil est demandé une fois par conversation ouverte (les
//! mêmes requêtes que la carte de profil).

use super::profile::{full_button, full_button_height, name_of, profile_theme, themed_background, Action, Header};
use super::App;

use eframe::egui;

/// Marge autour de la carte du panneau (mesurée sur le client).
const GAP: f32 = 8.0;

fn asked_id() -> egui::Id {
    egui::Id::unique("ferrocord.mp.profil-demandé")
}

impl App {
    /// Correspondant du MP à deux ouvert : celui dont le profil s'affiche à droite.
    pub(super) fn dm_profile_user(&self) -> Option<u64> {
        if self.store.guild.is_some() {
            return None;
        }
        self.dm_person(self.store.channel?).map(|(user, _)| user)
    }

    /// Largeur du panneau s'il est demandé (avant le repli en fenêtre
    /// étroite) : la carte et ses marges.
    pub(super) fn dm_profile_width(&self) -> Option<f32> {
        (self.theme.members_shown() && self.dm_profile_user().is_some()).then_some(self.theme.layout.profile_width + 2.0 * GAP)
    }

    /// Le panneau est-il affiché à cette image ?
    pub(super) fn dm_profile_shown(&self) -> bool {
        self.dm_profile_width().is_some() && !self.mobile_active() && !self.place.fit.members_hidden
    }

    /// Panneau de droite, à appeler sous l'en-tête du MP (qui court sur toute
    /// la largeur, comme dans le client) et avant la zone des messages : une
    /// carte arrondie et bordée, détachée des bords.
    pub(super) fn dm_profile_panel(&mut self, ui: &mut egui::Ui) {
        if !self.dm_profile_shown() {
            return;
        }
        let (Some(user), Some(channel)) = (self.dm_profile_user(), self.store.channel) else { return };
        // Demandé une fois par conversation ouverte, pas à chaque image.
        if ui.ctx().data(|d| d.get_temp::<u64>(asked_id())) != Some(user) {
            ui.ctx().data_mut(|d| d.insert_temp(asked_id(), user));
            self.store.want_profile(user, None);
            self.store.want_note(user);
        }
        let shown = self.shown(user, None);
        let dm = self.store.dms.iter().find(|d| d.id == channel);
        let avatar = dm.map_or_else(|| crate::state::model::Avatar::default_for(user), |d| d.avatar);
        let name = name_of(dm.map_or("", |d| d.name.as_str()), &shown);
        let base = self.theme.clone();
        let mut themed = profile_theme(&base, &shown);
        if let Some(th) = &mut themed {
            // Dégradé du thème sur toute la hauteur de la carte.
            th.set_span((ui.available_height() - 2.0 * GAP).max(0.0));
        }
        let t = themed.as_ref().map_or(&base, |th| &th.tokens);
        let lang = self.lang;
        let width = t.layout.profile_width;
        let pad = t.profile_pad();
        let mut action = None;
        // Autour de la carte : le fond de la discussion.
        let frame = egui::Frame::new().fill(base.colors.bg2).inner_margin(egui::Margin::same(GAP as i8));
        egui::Panel::right("dm_profile").frame(frame).exact_size(width + 2.0 * GAP).resizable(false).show_separator_line(false).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Chat);
            // Place du fond (uni, ou dégradé du thème du profil), derrière tout le contenu.
            let bg = ui.painter().add(egui::Shape::Noop);
            let whole = ui.max_rect();
            let r = t.r_card();
            // Rien ne dépasse des coins arrondis ni du filet de la carte.
            ui.set_clip_rect(whole.intersect(ui.clip_rect()));
            if let Some(th) = &themed {
                th.apply(ui);
            }
            let head = ui.cursor().min;
            self.profile_header(ui, t, width, user, None, &shown, avatar, &name, Header { themed: themed.as_ref(), bubble: true, panel: true, ..Header::default() });
            if let Some(p) = self.header_buttons(ui, t, egui::pos2(head.x + width - pad, head.y + pad), user) {
                action = Some(Action::Head(p));
            }
            // Le corps défile ; « Voir le profil complet » reste collé en bas.
            let body = (ui.available_height() - full_button_height(t)).max(0.0);
            egui::ScrollArea::vertical().id_salt("profil-mp").auto_shrink(false).max_height(body).show(ui, |ui| {
                egui::Frame::new().inner_margin(egui::Margin { top: 0, ..t.profile_margin() }).show(ui, |ui| {
                    ui.set_width(width - 2.0 * pad);
                    self.card_body(ui, t, user, None, &name, &shown, false, &mut action);
                });
            });
            if full_button(ui, t, lang, width) {
                action = Some(Action::Full(super::profile_full::Tab::About));
            }
            match &themed {
                Some(th) => themed_background(ui.painter(), th, bg, whole, r, Some(th.edge(t))),
                None => {
                    ui.painter().set(bg, egui::Shape::rect_filled(whole, r, t.colors.bg1));
                    ui.painter().rect_stroke(whole, r, t.border(), egui::StrokeKind::Inside);
                }
            }
            // Effet de profil (lot ND) : par-dessus la carte, sans prendre les clics.
            self.paint_profile_effect(ui.painter(), &shown, whole, r);
        });
        match action {
            Some(Action::Button(c)) => self.store.activity_button(user, &c.session_id, c.app_id, c.index),
            Some(Action::Full(tab)) => super::profile_full::open_full(ui.ctx(), user, None, &name, avatar, tab),
            Some(Action::Head(p)) => self.head_pick(ui.ctx(), user, &name, p),
            Some(Action::ActivitySettings) => self.show_settings(super::settings::Page::Activity),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{find, frame, offline_boot};
    use super::*;
    use crate::state::model::{Dm, User};
    use crate::state::relations::{DmInfo, Person, RelationsEvent};
    use crate::state::{Event, Store};

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
        fn send(&mut self, _: Vec<crate::state::Cmd>) {}
    }

    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<crate::state::Cmd>>>);

    impl super::super::Backend for Recorder {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<crate::state::Cmd>) {
            if let Ok(mut v) = self.0.lock() {
                v.extend(cmds);
            }
        }
    }

    /// « Inviter sur le serveur » : l'invitation est créée, puis son lien part
    /// dans le MP avec la personne, une seule fois.
    #[test]
    fn invite_to_server_sends_the_link_in_the_dm() {
        use crate::state::model::{Channel, ChannelKind, Guild};
        use crate::state::servers::{Invite, ServerCmd, ServerEvent};
        use crate::state::Cmd;
        let chan = Channel { id: 100, name: "général".into(), kind: ChannelKind::Text, position: 0, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![Guild::new(9, "Club Fictif".into(), vec![chan])], dms: vec![Dm::new(50, "Alice Fictive".into(), 5)] });
        let alice = Person { id: 7, name: "Alice Fictive".into(), username: "alice".into(), avatar: crate::state::model::Avatar::default_for(7) };
        store.apply(Event::Relations(RelationsEvent::Ready { relations: vec![], people: vec![alice], dms: vec![DmInfo { id: 50, recipients: vec![7], ..Default::default() }], reads: Some(vec![]), heads: vec![] }));
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let log = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut app = App::with_context(ctx, offline_boot(Box::new(Recorder(log.clone())), store, images));
        let taken = |log: &std::sync::Arc<std::sync::Mutex<Vec<Cmd>>>| log.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        frame(&mut app, Vec::new());
        taken(&log);
        app.invite_user_to(9, 7, "Alice Fictive");
        let sent = taken(&log);
        assert!(matches!(sent.as_slice(), [Cmd::Servers(ServerCmd::CreateInvite { channel_id: 100, max_uses: 0, temporary: false, .. })]), "{sent:?}");
        // Tant que Discord n'a pas rendu l'invitation : rien ne part.
        frame(&mut app, Vec::new());
        assert!(!taken(&log).iter().any(|c| matches!(c, Cmd::Send { .. })));
        let invite = Invite { code: "abcFictif".into(), guild_id: Some(9), channel_id: Some(100), channel_name: "général".into(), inviter: None, uses: 0, max_uses: 0, max_age: 604_800, temporary: false, expires_at_ms: None };
        let eff = app.store.apply(Event::Servers(ServerEvent::InviteCreated(invite)));
        app.handle(eff);
        frame(&mut app, Vec::new());
        let sent = taken(&log);
        let links: Vec<_> = sent.iter().filter_map(|c| if let Cmd::Send { channel_id, content, .. } = c { Some((*channel_id, content.as_str())) } else { None }).collect();
        assert_eq!(links, [(50, "https://discord.gg/abcFictif")]);
        assert_eq!(app.store.sending.len(), 1, "message en cours d'envoi, montré estompé");
        frame(&mut app, Vec::new());
        assert!(!taken(&log).iter().any(|c| matches!(c, Cmd::Send { .. })), "une seule fois");
        // Serveur sans salon où inviter : rien n'est demandé.
        app.invite_user_to(12345, 7, "Alice Fictive");
        assert!(taken(&log).is_empty());
    }

    /// Un MP à deux montre le profil du correspondant à droite ; le bouton de
    /// l'en-tête le masque ; un groupe n'en a pas.
    #[test]
    fn dm_shows_the_profile_panel_and_the_header_button_hides_it() {
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![Dm::new(50, "Alice Fictive".into(), 5)] });
        let alice = Person { id: 7, name: "Alice Fictive".into(), username: "alice".into(), avatar: crate::state::model::Avatar::default_for(7) };
        store.apply(Event::Relations(RelationsEvent::Ready { relations: vec![], people: vec![alice], dms: vec![DmInfo { id: 50, recipients: vec![7], ..Default::default() }], reads: Some(vec![]), heads: vec![] }));
        let ctx = egui::Context::default();
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = App::with_context(ctx, offline_boot(Box::new(Quiet), store, images));
        app.store.select_dms();
        app.open_channel(50);
        let mut out = frame(&mut app, Vec::new());
        for _ in 0..3 {
            out = frame(&mut app, Vec::new());
        }
        assert_eq!(app.dm_profile_user(), Some(7));
        assert!(app.dm_profile_shown(), "panneau affiché par défaut");
        assert!(find(&out, "Voir le profil complet").is_some(), "contenu du panneau");
        assert!(find(&out, "Masquer le profil").is_some(), "bouton de l'en-tête");
        super::super::tests::click(&mut app, "Masquer le profil");
        let out = frame(&mut app, Vec::new());
        assert!(!app.dm_profile_shown());
        assert!(find(&out, "Voir le profil complet").is_none());
        assert!(find(&out, "Afficher le profil").is_some());
    }
}
