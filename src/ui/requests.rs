//! Demandes de message et dossier de spam (lot U) : les MP que Discord marque
//! `is_message_request` sortent de la colonne des conversations et se rangent
//! ici, sous une ligne « Demandes de message » qui n'apparaît que s'il y en a.
//! Deux onglets (Demandes, Spam) ; sur chaque ligne, « Accepter » et
//! « Ignorer », écrits au clic ; un clic sur la ligne ouvre la conversation
//! pour la lire.

use super::friends::{count_label, disc, empty_state, tab, Who, PAGE_X, PAGE_Y, ROW};
use super::util::pixel_square;
use super::App;
use crate::i18n::table::people::t as ut;
use crate::i18n::table::social::{f, t as st};
use crate::state::relations::RelationsCmd;
use crate::theme::icons::{self, Icon};
use eframe::egui::{self, RichText, Sense};

#[derive(Default)]
pub(super) struct Requests {
    /// La page est affichée à la place de la page Amis.
    pub(super) open: bool,
    /// Onglet « Spam ».
    spam: bool,
}

impl App {
    /// Ligne « Demandes de message » de la colonne des MP, seulement s'il y a
    /// des demandes ; `true` si elle vient d'être cliquée.
    pub(super) fn requests_entry(&self, ui: &mut egui::Ui) -> bool {
        let count = self.store.relations.request_count();
        if count == 0 {
            return false;
        }
        let lang = self.lang;
        let selected = self.requests_shown() && self.store.channel.is_none();
        self.dm_entry_counted(ui, Icon::Inbox, ut::REQUESTS.get(lang), count, selected, true).clicked()
    }

    /// La page des demandes remplace la page Amis ; la dernière demande traitée, la page Amis revient.
    pub(super) fn requests_shown(&self) -> bool {
        self.requests.open && self.store.relations.request_count() > 0
    }

    /// Ouvre la page des demandes (aperçu, tests) ; `spam` : sur le dossier de spam.
    pub fn show_requests(&mut self, spam: bool) {
        self.store.select_dms();
        self.requests = Requests { open: true, spam };
        self.quests.open = false;
    }

    /// Bandeau d'une demande de message, au-dessus de la saisie de la
    /// conversation ouverte : qui écrit, « Accepter » et « Ignorer » (les
    /// mêmes écritures que la page des demandes).
    pub(super) fn request_banner(&mut self, ui: &mut egui::Ui, cid: u64) {
        if !self.store.relations.is_request(cid) {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let name = self.store.dms.iter().find(|d| d.id == cid).map(|d| d.name.clone()).unwrap_or_default();
        let mut write = None;
        t.card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let side = crate::theme::Tokens::icon_side(t.typography.body);
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                icons::paint(Icon::Inbox, ui.painter(), pixel_square(ui, slot.min, side), t.colors.muted);
                ui.label(RichText::new(ut::REQUESTS.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
            });
            ui.add(egui::Label::new(RichText::new(crate::i18n::table::restes::f::request_banner(lang, &name)).color(t.colors.text)).wrap());
            ui.add_space(t.spacing.item_y);
            ui.horizontal(|ui| {
                if super::widgets::button(ui, &t, super::widgets::Kind::Primary, crate::i18n::table::restes::t::ACCEPT_REQUEST.get(lang)).clicked() {
                    write = Some(RelationsCmd::AcceptRequest { channel_id: cid });
                }
                if super::widgets::button(ui, &t, super::widgets::Kind::Secondary, ut::IGNORE.get(lang)).clicked() {
                    write = Some(RelationsCmd::RejectRequest { channel_id: cid });
                }
            });
        });
        ui.add_space(t.spacing.chat_pad / 2.0);
        if let Some(cmd) = write {
            self.store.relations_write(cmd);
        }
    }

    pub(super) fn requests_page(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let mobile = self.mobile_active();
        let pending = self.store.relations.requests(false);
        let spam = self.store.relations.requests(true);
        egui::Panel::top("requests_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                // Disposition mobile : retour à la liste (phase 7).
                self.mobile_back(ui);
                let side = super::chat_head::icon_side(&t);
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                icons::paint(Icon::Inbox, ui.painter(), pixel_square(ui, slot.min, side), t.colors.muted);
                ui.label(RichText::new(ut::REQUESTS.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                ui.separator();
                for (is_spam, text, n) in [(false, ut::TAB_REQUESTS, pending.len()), (true, ut::TAB_SPAM, spam.len())] {
                    let label = if n > 0 { f::tab_count(lang, text.get(lang), n) } else { text.get(lang).to_string() };
                    if tab(ui, &t, &label, self.requests.spam == is_spam).clicked() {
                        self.requests.spam = is_spam;
                    }
                }
            });
        });
        let margin = if mobile { t.chat_head_margin() } else { egui::Margin { left: PAGE_X, right: PAGE_X, top: PAGE_Y, bottom: 0 } };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(margin)).show(ui, |ui| {
            let (list, title, none) = if self.requests.spam { (spam, ut::TAB_SPAM, ut::NONE_SPAM) } else { (pending, ut::TAB_REQUESTS, ut::NONE_REQUESTS) };
            count_label(ui, &t, lang, title.get(lang), list.len());
            ui.add_space(t.spacing.item_y);
            if list.is_empty() {
                empty_state(ui, &t, Icon::Inbox, none.get(lang));
                return;
            }
            let (mut write, mut open) = (None, None);
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW, list.len(), |ui, range| {
                for channel in &list[range] {
                    let Some(dm) = self.store.dms.iter().find(|d| d.id == *channel) else { continue };
                    let user = self.store.relations.dm_info(*channel).and_then(|d| d.recipients.first().copied());
                    let username = user.and_then(|u| self.store.relations.person(u)).map_or("", |p| p.username.as_str());
                    let who = Who { id: *channel, name: &dm.name, username, avatar: Some(dm.avatar), status: None, second: ut::REQUEST_HINT.get(lang), lead: super::friends::Lead::None, look: None };
                    let row = self.person_line(ui, &who, |ui| {
                        // Disposition de droite à gauche : « Ignorer » à droite, « Accepter » avant.
                        if disc(ui, &t, Icon::Close, ut::IGNORE.get(lang), t.colors.danger).clicked() {
                            write = Some(RelationsCmd::RejectRequest { channel_id: *channel });
                        }
                        if disc(ui, &t, Icon::Check, st::ACCEPT.get(lang), t.colors.online).clicked() {
                            write = Some(RelationsCmd::AcceptRequest { channel_id: *channel });
                        }
                    });
                    if row.clicked() {
                        open = Some(*channel);
                    }
                    if row.secondary_clicked()
                        && let Some(u) = user
                    {
                        super::profile::open_menu(ui.ctx(), u, &dm.name, row.rect);
                    }
                }
            });
            if let Some(cmd) = write {
                self.store.relations_write(cmd);
            }
            if let Some(c) = open {
                self.open_channel(c);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, find, frame, offline_boot};
    use super::*;
    use crate::i18n::Lang;
    use crate::state::model::{Dm, User};
    use crate::state::relations::{DmInfo, Person, RelationsEvent};
    use crate::state::{Cmd, Event, Store};
    use std::sync::{Arc, Mutex};

    struct Recorder(Arc<Mutex<Vec<Cmd>>>);

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
        fn send(&mut self, cmds: Vec<Cmd>) {
            if let Ok(mut v) = self.0.lock() {
                v.extend(cmds);
            }
        }
    }

    /// Une demande sort de la colonne, se range sous « Demandes de message »,
    /// et « Accepter » part au clic ; acceptée, elle redevient une conversation.
    #[test]
    fn requests_leave_the_column_and_are_accepted_in_one_click() {
        const ASK: u64 = 70;
        const PLAIN: u64 = 71;
        let ctx = egui::Context::default();
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![Dm::new(ASK, "Inconnu".into(), 5), Dm::new(PLAIN, "Alice".into(), 6)] });
        let person = |id: u64, name: &str| Person { id, name: name.into(), username: name.to_lowercase(), avatar: crate::state::model::Avatar::default_for(id) };
        let dms = vec![DmInfo { id: ASK, recipients: vec![9], request: true, ..Default::default() }, DmInfo { id: PLAIN, recipients: vec![2], ..Default::default() }];
        store.apply(Event::Relations(RelationsEvent::Ready { relations: vec![], people: vec![person(9, "Inconnu"), person(2, "Alice")], dms, reads: Some(vec![]), heads: vec![] }));
        store.select_dms();
        let log = Arc::new(Mutex::new(Vec::new()));
        let images = crate::images::Images::disabled(ctx.clone());
        let mut app = super::super::App::with_context(ctx, offline_boot(Box::new(Recorder(log.clone())), store, images));

        let out = frame(&mut app, Vec::new());
        assert!(find(&out, "Alice").is_some());
        assert!(find(&out, "Inconnu").is_none(), "une demande n'est pas dans la colonne");
        click(&mut app, ut::REQUESTS.get(Lang::Fr));
        assert!(app.requests_shown());
        click(&mut app, st::ACCEPT.get(Lang::Fr));
        let sent = log.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        assert_eq!(sent, vec![Cmd::Relations(RelationsCmd::AcceptRequest { channel_id: ASK })]);

        // Conversation de la demande ouverte : le bandeau la présente, avec les deux mêmes actions.
        app.open_channel(ASK);
        frame(&mut app, Vec::new());
        let out = frame(&mut app, Vec::new());
        assert!(find(&out, "Envoyer un message à @Inconnu").is_some() && find(&out, crate::i18n::table::restes::t::ACCEPT_REQUEST.get(Lang::Fr)).is_some(), "bandeau de la demande au-dessus de la saisie");
        click(&mut app, ut::IGNORE.get(Lang::Fr));
        let sent = log.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();
        assert!(sent.contains(&Cmd::Relations(RelationsCmd::RejectRequest { channel_id: ASK })), "{sent:?}");
        app.store.select_dms();
        app.store.channel = None;

        // La réponse de Discord (le MP sans son drapeau) : la conversation revient dans la colonne.
        app.store.apply(Event::Relations(RelationsEvent::Dm { info: DmInfo { id: ASK, recipients: vec![9], ..Default::default() }, people: vec![], known_recipients: true }));
        let out = frame(&mut app, Vec::new());
        assert!(!app.requests_shown(), "plus de demande : retour à la page Amis");
        assert!(find(&out, "Inconnu").is_some());
    }
}
