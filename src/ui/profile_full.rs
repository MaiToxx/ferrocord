//! Profil complet (fenêtre à onglets : À propos, Activité, Serveurs en
//! commun, Amis en commun), note personnelle, et menu d'un utilisateur
//! (clic droit sur un auteur, un membre, un ami ou une mention).
//!
//! Comme la carte (`profile.rs`), chaque fenêtre est demandée par un dépôt
//! dans le contexte d'egui, depuis n'importe quelle zone, puis dessinée par
//! [`App::profile_ui`] qui, lui, a le magasin.

use super::profile::{chip, close as close_card, room, name_of, presence_ui, section, Shown};
use super::util::{paint_avatar, pixel_square};
use super::App;
use crate::i18n::table::profile::{f as pf, service, t as pt};
use crate::i18n::table::social::f as sf;
use crate::i18n::Lang;
use crate::state::model::Avatar;
use crate::state::profiles::{Connection, Note, ProfileState, MAX_NOTE};
use crate::state::relations::RelationsCmd;
use crate::theme::Tokens;
use eframe::egui::{self, Id, RichText, Sense};

/// Onglets du profil complet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Tab {
    About,
    Activity,
    Servers,
    Friends,
}

#[derive(Clone)]
struct Full {
    user_id: u64,
    guild_id: Option<u64>,
    name: String,
    avatar: Avatar,
    tab: Tab,
    fresh: bool,
    /// Ouvert par « Note » : le champ de la note prend la main.
    focus_note: bool,
}

/// Retrait ou blocage demandé depuis le menu : confirmé par « Oui ».
#[derive(Clone)]
struct Confirm {
    user_id: u64,
    name: String,
    block: bool,
}

/// Texte du champ de la note, et la note connue quand la saisie a commencé.
#[derive(Clone, Default)]
struct NoteBuf {
    text: String,
    base: String,
}

fn full_id() -> Id {
    Id::unique("ferrocord.profils.complet")
}

fn confirm_id() -> Id {
    Id::unique("ferrocord.profils.confirmer")
}

fn note_buf_id(user_id: u64) -> Id {
    Id::unique(("ferrocord.profils.note", user_id))
}

fn note_field_id(user_id: u64) -> Id {
    Id::unique(("ferrocord.profils.note.champ", user_id))
}

/// Ouvre le profil complet de `user_id` sur l'onglet `tab`.
pub(super) fn open_full(ctx: &egui::Context, user_id: u64, guild_id: Option<u64>, name: &str, avatar: Avatar, tab: Tab) {
    let full = Full { user_id, guild_id, name: name.to_string(), avatar, tab, fresh: true, focus_note: false };
    ctx.data_mut(|d| d.insert_temp(full_id(), full));
}

/// Ouvre le menu de `user_id` au point `at`.
pub(super) fn open_menu(ctx: &egui::Context, user_id: u64, name: &str, at: egui::Pos2) {
    super::user_menu::open(ctx, user_id, name, at);
}

/// Ouvre le profil complet sur sa note, prête à la saisie (« Ajouter une note »).
pub(super) fn open_note(ctx: &egui::Context, user_id: u64, guild_id: Option<u64>, name: &str, avatar: Avatar) {
    let full = Full { user_id, guild_id, name: name.to_string(), avatar, tab: Tab::About, fresh: true, focus_note: true };
    ctx.data_mut(|d| d.insert_temp(full_id(), full));
}

/// Demande « Oui » avant de retirer un ami (`block` : de bloquer).
pub(super) fn ask_relation(ctx: &egui::Context, user_id: u64, name: &str, block: bool) {
    ctx.data_mut(|d| d.insert_temp(confirm_id(), Confirm { user_id, name: name.to_string(), block }));
}

/// Texte de note saisi et pas encore enregistré (fenêtre fermée pendant la saisie).
pub(super) fn pending_note(ctx: &egui::Context, user_id: u64, note: Option<&Note>) -> Option<String> {
    let buf = ctx.data(|d| d.get_temp::<NoteBuf>(note_buf_id(user_id)))?;
    let shown = note?.shown()?;
    (buf.base == shown && buf.text != buf.base).then_some(buf.text)
}

/// Champ de la note : rien tant qu'elle n'est pas demandée, ni si Discord ne
/// la donne pas. Renvoie le texte à enregistrer quand le champ perd la main
/// après un changement (jamais à chaque frappe).
pub(super) fn note_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, user_id: u64, note: Option<&Note>) -> Option<String> {
    let n = note?;
    if n.unavailable {
        return None;
    }
    section(ui, t, pt::NOTE.get(lang));
    if super::settings::local::display(ui.ctx()).hide_personal {
        ui.label(RichText::new(crate::i18n::table::s2::t::HIDDEN_STREAMER.get(lang)).small().color(t.colors.faint));
        return None;
    }
    let Some(shown) = n.shown() else {
        ui.label(RichText::new(pt::LOADING.get(lang)).small().color(t.colors.faint));
        return None;
    };
    let field = note_field_id(user_id);
    let focused = ui.memory(|m| m.has_focus(field));
    let mut buf = ui.ctx().data(|d| d.get_temp::<NoteBuf>(note_buf_id(user_id))).unwrap_or_default();
    if buf.base != shown && !focused {
        buf = NoteBuf { text: shown.to_string(), base: shown.to_string() };
    }
    let edit = egui::TextEdit::multiline(&mut buf.text)
        .id(field)
        .hint_text(RichText::new(pt::NOTE_HINT.get(lang)).color(t.colors.faint))
        .desired_rows(1)
        .desired_width(f32::INFINITY)
        .char_limit(MAX_NOTE)
        .interactive(n.writing.is_none());
    let resp = ui.add(edit);
    let save = (resp.lost_focus() && buf.text != buf.base).then(|| buf.text.clone());
    ui.ctx().data_mut(|d| d.insert_temp(note_buf_id(user_id), buf));
    if n.writing.is_some() {
        ui.label(RichText::new(pt::NOTE_SAVING.get(lang)).small().color(t.colors.faint));
    } else if n.captcha {
        ui.add(egui::Label::new(RichText::new(pt::NOTE_CAPTCHA.get(lang)).small().color(t.colors.danger)).wrap());
    } else if n.failed {
        ui.label(RichText::new(pt::NOTE_FAILED.get(lang)).small().color(t.colors.danger));
    }
    save
}

impl App {
    /// Ouvre le profil complet de `user_id` (aperçu).
    pub fn open_full_profile(&self, user_id: u64, guild_id: Option<u64>) {
        let known = self.store.people.lock().user(user_id).map(|u| (u.display_name().to_string(), u.avatar));
        let (name, avatar) = known.unwrap_or_else(|| (String::new(), Avatar::default_for(user_id)));
        open_full(&self.ctx, user_id, guild_id, &name, avatar, Tab::About);
    }

    /// Ferme le profil complet ouvert (retour de la disposition mobile), en
    /// enregistrant la note en cours ; `false` s'il n'y en a pas.
    pub(super) fn close_full_profile(&mut self) -> bool {
        let ctx = self.ctx.clone();
        let Some(f) = ctx.data(|d| d.get_temp::<Full>(full_id())) else { return false };
        let s = self.shown(f.user_id, f.guild_id);
        if let Some(text) = pending_note(&ctx, f.user_id, s.note.as_ref()) {
            self.store.save_note(f.user_id, &text);
        }
        ctx.data_mut(|d| d.remove::<Full>(full_id()));
        true
    }

    /// « Retirer l'ami » et « Bloquer » : écrits seulement après « Oui ».
    pub(super) fn relation_confirm_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(c) = ctx.data(|d| d.get_temp::<Confirm>(confirm_id())) else { return };
        let t = &self.theme;
        let lang = self.lang;
        let (mut yes, mut no) = (false, false);
        let q = if c.block { sf::confirm_block(lang, &c.name) } else { sf::confirm_remove(lang, &c.name) };
        let modal = crate::ui::widgets::modal(
            &ctx,
            t,
            confirm_id(),
            crate::ui::mobile::card_width(&ctx, t, 1.0),
            &q,
            crate::i18n::t::CLOSE_TIP.get(lang),
            |_| {},
            |ui| {
                yes = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Danger, pt::YES.get(lang)).clicked();
                no = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Link, pt::NO.get(lang)).clicked();
            },
        );
        if yes {
            let cmd = if c.block { RelationsCmd::Block { user_id: c.user_id } } else { RelationsCmd::Remove { user_id: c.user_id } };
            self.store.relations_write(cmd);
        }
        if yes || no || modal.closed {
            ctx.data_mut(|d| d.remove::<Confirm>(confirm_id()));
        }
    }

    /// Profil complet ouvert, s'il y en a un ; Échap, un clic en dehors ou
    /// « Fermer » le ferme (la note en cours de saisie est enregistrée).
    pub(super) fn full_profile_ui(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let Some(mut f) = ctx.data(|d| d.get_temp::<Full>(full_id())) else { return };
        if f.fresh {
            self.store.want_profile(f.user_id, f.guild_id);
            self.store.want_note(f.user_id);
            f.fresh = false;
        }
        let s = self.shown(f.user_id, f.guild_id);
        self.want_profile_effect(&s);
        // Couleurs de thème du profil : fond en dégradé, texte recalculé.
        let themed = super::profile::profile_theme(&self.theme, &s);
        let t = themed.as_ref().map_or_else(|| self.theme.clone(), |th| th.tokens.clone());
        let lang = self.lang;
        // Disposition mobile (phase 7) : plein écran, avec un bouton retour.
        let mobile = self.mobile_active();
        let screen = ctx.content_rect();
        let width = if mobile { screen.width() } else { t.profile_full_width() };
        let pad = t.profile_pad();
        let name = name_of(&f.name, &s);
        let mut closing = false;
        let mut out = FullOut::default();
        let mut button = None;
        let mut edit = false;
        let frame = if mobile { egui::Frame::new().fill(t.colors.bg1) } else { egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(t.r_card()) };
        let modal = egui::Modal::new(full_id()).frame(frame).show(&ctx, |ui| {
            // Place du fond en dégradé, derrière tout le contenu.
            let bg = (ui.painter().clone(), ui.painter().add(egui::Shape::Noop));
            let window_top = Some(ui.cursor().top());
            if let Some(th) = &themed {
                th.apply(ui);
            }
            ui.set_width(width);
            if mobile {
                ui.set_min_height(screen.height());
                closing |= super::mobile::back_row(ui, &t, lang, pt::CLOSE.get(lang));
            }
            let head_top = ui.cursor().min;
            self.profile_header(ui, &t, width, f.user_id, f.guild_id, &s, f.avatar, &name, super::profile::Header { full: true, bubble: true, themed: themed.as_ref(), window_top, ..Default::default() });
            if !mobile {
                // Croix sur la bannière, en haut à droite, sur un disque sombre (lisible sur toute image).
                // Jamais plus petit que la croix elle-même (thèmes à contrôles bas : elle débordait du disque).
                let side = crate::ui::widgets::close_side(&t).max(t.typography.icon);
                let at = egui::Rect::from_min_size(egui::pos2(head_top.x + width - pad - side, head_top.y + pad), egui::Vec2::splat(side));
                ui.painter().circle_filled(at.center(), side / 2.0, self.theme.media_veil());
                // Enfant hors du flux : la croix ne déplace pas le contenu qui suit l'en-tête.
                let mut over = ui.new_child(egui::UiBuilder::new().max_rect(at).layout(egui::Layout::centered_and_justified(egui::Direction::LeftToRight)));
                closing |= crate::ui::widgets::close_button_on_media(&mut over, &self.theme, crate::i18n::t::CLOSE_TIP.get(lang)).clicked();
            }
            // Corps dans un enfant de largeur fixe : un contenu de la zone défilante plus large de deux
            // points élargissait la fenêtre, et la bannière s'arrêtait avant le bord droit.
            let body_top = ui.cursor().min;
            let mut body = ui.new_child(egui::UiBuilder::new().max_rect(egui::Rect::from_min_size(body_top, egui::vec2(width, ui.available_height()))));
            egui::Frame::new().inner_margin(t.profile_margin()).show(&mut body, |ui| {
                ui.set_width(width - 2.0 * pad);
                self.identity_ui(ui, &t, &name, &s);
                if s.captcha() {
                    ui.add(egui::Label::new(RichText::new(pt::CAPTCHA.get(lang)).color(t.colors.danger)).wrap());
                }
                ui.horizontal_wrapped(|ui| {
                    let me = self.store.me.as_ref().is_some_and(|u| u.id == f.user_id);
                    // Mon profil : l'édition rapide, sans passer par les paramètres.
                    if me && !mobile && ui.add(super::moderation::primary_button(&t, crate::i18n::table::quick::t::EDIT_PROFILE.get(lang))).clicked() {
                        edit = true;
                    }
                    // Action principale d'accent ; la fenêtre se ferme par sa croix, Échap ou un clic à côté.
                    if !me && ui.add(super::form::primary(&t, crate::i18n::t::PROFILE_SEND_MESSAGE.get(lang))).clicked() {
                        out.message = true;
                    }
                    if super::settings::local::copy_id_shown(&ctx, &self.store) && ui.add(super::form::btn(&t, crate::i18n::t::PROFILE_COPY_ID.get(lang))).clicked() {
                        ctx.copy_text(f.user_id.to_string());
                    }
                });
                ui.add_space(t.spacing.item_y);
                self.tabs_ui(ui, &t, &mut f.tab, &s);
                ui.separator();
                // Le contenu défile dans ce que l'en-tête laisse de la fenêtre : la hauteur
                // de tout le reste (bannière, identité, onglets) est celle de l'image d'avant.
                let chrome_id = full_id().with("reste");
                let chrome = ui.data(|d| d.get_temp::<f32>(chrome_id)).unwrap_or(screen.height() * 0.55);
                // Plein écran (mobile) : la part d'avant, la fenêtre n'a pas à se caler.
                let max_height = if mobile { screen.height() * 0.55 } else { screen.height() - chrome - 2.0 * pad }.max(t.layout.profile_avatar);
                let area = room(ui, max_height, |ui| egui::ScrollArea::vertical().id_salt("profil-complet").max_height(max_height).auto_shrink([false, true]).show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    match f.tab {
                        Tab::About => self.about_tab(ui, &t, &mut f, &s, &mut out),
                        Tab::Activity => match &s.presence {
                            Some(p) if !p.activities.is_empty() || p.custom().is_some() => {
                                if let Some(c) = presence_ui(ui, &t, lang, &self.images, p, self.h24()) {
                                    button = Some(c);
                                }
                            }
                            _ => muted(ui, &t, pt::NO_ACTIVITY.get(lang)),
                        },
                        Tab::Servers => self.servers_tab(ui, &t, &s, &mut out),
                        Tab::Friends => self.friends_tab(ui, &t, &s, &mut out),
                    }
                }));
                let zone = area.inner_rect.height();
                ui.data_mut(|d| d.insert_temp(chrome_id.with("zone"), zone));
            });
            ui.allocate_rect(egui::Rect::from_min_size(body_top, egui::vec2(width, body.min_rect().height())), egui::Sense::hover());
            bg
        });
        // Hauteur de la fenêtre hors zone défilante, pour l'image suivante
        // (une image de plus si elle a changé : le contenu se cale aussitôt).
        if !mobile {
            let chrome_id = full_id().with("reste");
            let zone = ctx.data(|d| d.get_temp::<f32>(chrome_id.with("zone"))).unwrap_or(0.0);
            let chrome = (modal.response.rect.height() - zone).round();
            if ctx.data(|d| d.get_temp::<f32>(chrome_id)) != Some(chrome) {
                ctx.data_mut(|d| d.insert_temp(chrome_id, chrome));
                ctx.request_repaint();
            }
        }
        if let Some(th) = &themed {
            let (painter, idx) = &modal.inner;
            // Plein écran (mobile) : ni coins arrondis ni contour.
            let (r, edge) = if mobile { (egui::CornerRadius::ZERO, None) } else { (t.r_card(), Some(t.border())) };
            super::profile::themed_background(painter, th, *idx, modal.response.rect, r, edge);
        }
        // Effet de profil (lot ND), par-dessus la fenêtre.
        {
            let (painter, _) = &modal.inner;
            let r = if mobile { egui::CornerRadius::ZERO } else { t.r_card() };
            self.paint_profile_effect(painter, &s, modal.response.rect, r);
        }
        if let Some(c) = button {
            self.store.activity_button(f.user_id, &c.session_id, c.app_id, c.index);
        }
        if let Some(text) = out.note.take() {
            self.store.save_note(f.user_id, &text);
        }
        if out.message {
            self.store.message_user(f.user_id);
            closing = true;
        }
        if edit {
            self.open_profile_editor(f.guild_id.is_some());
            closing = true;
        }
        if let Some(g) = out.guild {
            self.store.select_guild(g);
            closing = true;
        }
        if let Some((id, name, avatar)) = out.friend.take() {
            let at = ctx.input(|i| i.pointer.interact_pos()).unwrap_or_default();
            close_card(&ctx);
            super::profile::open(&ctx, id, None, egui::Rect::from_min_size(at, egui::Vec2::ZERO), &name, avatar);
            closing = true;
        }
        if closing || modal.should_close() {
            if let Some(text) = pending_note(&ctx, f.user_id, s.note.as_ref()) {
                self.store.save_note(f.user_id, &text);
            }
            ctx.data_mut(|d| d.remove::<Full>(full_id()));
            return;
        }
        ctx.data_mut(|d| d.insert_temp(full_id(), f));
    }

    fn tabs_ui(&self, ui: &mut egui::Ui, t: &Tokens, tab: &mut Tab, s: &Shown) {
        let lang = self.lang;
        let p = s.profile.as_ref();
        let count = |label: &str, n: Option<usize>| match n {
            Some(n) => pf::tab_count(lang, label, n),
            None => label.to_string(),
        };
        let tabs = [
            (Tab::About, pt::TAB_ABOUT.get(lang).to_string()),
            (Tab::Activity, pt::TAB_ACTIVITY.get(lang).to_string()),
            (Tab::Servers, count(pt::TAB_SERVERS.get(lang), p.and_then(|p| p.mutual_guilds.as_ref().map(Vec::len)))),
            (Tab::Friends, count(pt::TAB_FRIENDS.get(lang), p.and_then(|p| p.friends_count()))),
        ];
        // Onglets soulignés, comme dans le client (pas de pastille pleine).
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.item_x * 3.0;
            for (k, label) in tabs {
                if crate::ui::widgets::tab(ui, t, &label, *tab == k).clicked() {
                    *tab = k;
                }
            }
        });
    }

    fn about_tab(&self, ui: &mut egui::Ui, t: &Tokens, f: &mut Full, s: &Shown, out: &mut FullOut) {
        let lang = self.lang;
        if let Some(bio) = s.profile.as_ref().and_then(|p| p.bio()) {
            section(ui, t, pt::ABOUT_ME.get(lang));
            self.bio_ui(ui, t, f.user_id, f.guild_id, bio);
        }
        if let Some(p) = s.profile.as_ref().and_then(|p| p.pronouns()) {
            section(ui, t, pt::PRONOUNS.get(lang));
            ui.label(RichText::new(p).color(t.colors.text));
        }
        self.dates_ui(ui, t, f.user_id, s);
        if let (Some(g), Some(m)) = (f.guild_id, &s.member) {
            self.member_ui(ui, t, g, m);
        }
        if let Some(list) = s.profile.as_ref().map(|p| &p.connections).filter(|c| !c.is_empty()) {
            section(ui, t, pt::CONNECTIONS.get(lang));
            // Cartes bordées, deux par rangée quand la place le permet.
            let gap = t.spacing.item_x;
            let cols = if ui.available_width() >= 2.0 * t.layout.profile_width - gap { 2 } else { 1 };
            let w = ((ui.available_width() - gap * (cols as f32 - 1.0)) / cols as f32).floor();
            for pair in list.chunks(cols) {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = gap;
                    for c in pair {
                        t.card_frame().fill(t.colors.bg2).inner_margin(egui::Margin::symmetric(t.spacing.embed_pad.round() as i8, t.spacing.item_x.round() as i8)).show(ui, |ui| {
                            ui.set_width(w - 2.0 * t.spacing.embed_pad);
                            connection_ui(ui, t, lang, c);
                        });
                    }
                });
            }
        }
        if f.focus_note && s.note.as_ref().is_some_and(|n| n.shown().is_some()) {
            ui.memory_mut(|m| m.request_focus(note_field_id(f.user_id)));
            f.focus_note = false;
        }
        out.note = note_ui(ui, t, lang, f.user_id, s.note.as_ref());
    }

    fn servers_tab(&self, ui: &mut egui::Ui, t: &Tokens, s: &Shown, out: &mut FullOut) {
        let lang = self.lang;
        let Some(list) = s.profile.as_ref().and_then(|p| p.mutual_guilds.as_ref()) else {
            return unavailable(ui, t, lang, s);
        };
        let rows: Vec<_> = list.iter().filter_map(|m| self.store.guilds.iter().find(|g| g.id == m.id).map(|g| (g, m))).collect();
        if rows.is_empty() {
            return muted(ui, t, pt::NO_SERVERS.get(lang));
        }
        for (g, m) in rows {
            let avatar = Avatar { image: g.icon, default: 0 };
            if row(ui, t, &self.images, &avatar, &g.name, m.nick.as_deref(), ("serveur", g.id)) {
                out.guild = Some(g.id);
            }
        }
    }

    fn friends_tab(&self, ui: &mut egui::Ui, t: &Tokens, s: &Shown, out: &mut FullOut) {
        let lang = self.lang;
        let Some(ids) = s.profile.as_ref().and_then(|p| p.mutual_friends.as_ref()) else {
            return unavailable(ui, t, lang, s);
        };
        if ids.is_empty() {
            return muted(ui, t, pt::NO_FRIENDS.get(lang));
        }
        let people: Vec<(u64, String, Option<String>, Avatar)> = {
            let p = self.store.people.lock();
            ids.iter().map(|id| match p.user(*id) {
                Some(u) => (*id, u.display_name().to_string(), Some(u.handle()).filter(|h| h != u.display_name()), u.avatar),
                None => (*id, id.to_string(), None, Avatar::default_for(*id)),
            })
            .collect()
        };
        for (id, name, handle, avatar) in people {
            if row(ui, t, &self.images, &avatar, &name, handle.as_deref(), ("ami", id)) {
                out.friend = Some((id, name, avatar));
            }
        }
    }
}

/// Ce que le profil complet demande au magasin après l'image.
#[derive(Default)]
struct FullOut {
    note: Option<String>,
    message: bool,
    guild: Option<u64>,
    friend: Option<(u64, String, Avatar)>,
}

fn muted(ui: &mut egui::Ui, t: &Tokens, text: &str) {
    ui.add(egui::Label::new(RichText::new(text).color(t.colors.muted)).wrap());
}

/// Liste absente de la réponse : en attente, ou non fournie par Discord.
fn unavailable(ui: &mut egui::Ui, t: &Tokens, lang: Lang, s: &Shown) {
    let text = match s.state {
        None | Some(ProfileState::Loading) => pt::LOADING,
        Some(ProfileState::Captcha) => pt::CAPTCHA,
        _ => pt::UNAVAILABLE,
    };
    muted(ui, t, text.get(lang));
}

/// Ligne cliquable : avatar, nom, détail atténué ; vrai si cliquée.
fn row(ui: &mut egui::Ui, t: &Tokens, images: &crate::images::Images, avatar: &Avatar, name: &str, detail: Option<&str>, salt: (&str, u64)) -> bool {
    let h = t.spacing.member_row;
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), h), Sense::hover());
    let resp = ui.interact(rect, Id::unique(("ferrocord.profils.ligne", salt.0, salt.1)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
    if resp.hovered() {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let (slot, left) = t.dm_row_parts(rect);
    let slot = pixel_square(ui, slot.min, slot.width());
    paint_avatar(ui, images, t, slot, avatar, name);
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let mut job = egui::text::LayoutJob::simple_singleline(name.to_string(), font, t.colors.text_strong);
    if let Some(d) = detail {
        job.append(&format!("  {d}"), 0.0, egui::TextFormat { font_id: small, color: t.colors.faint, ..Default::default() });
    }
    let galley = ui.painter().layout_job(job);
    let top = rect.top() + Tokens::centered_top(rect.height(), galley.size().y);
    ui.painter().galley(egui::pos2(left, top), galley, t.colors.text);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, name));
    resp.clicked()
}

/// Compte lié : service, nom (lien vers sa page publique s'il se déduit),
/// « Vérifié ».
fn connection_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, c: &Connection) {
    ui.horizontal_wrapped(|ui| {
        // Pastille ronde du service, comme sur la carte.
        let (disc, _) = ui.allocate_exact_size(egui::Vec2::splat(Tokens::icon_side(t.typography.body)), egui::Sense::hover());
        super::profile_parts::paint_service(ui.painter(), t, disc, lang, &c.kind);
        let svc = if c.kind == "domain" { pt::DOMAIN.get(lang) } else { service(&c.kind).unwrap_or(&c.kind) };
        ui.label(RichText::new(svc).small().family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
        match c.url() {
            Some(url) => {
                if ui.link(RichText::new(&c.name).color(t.colors.link)).on_hover_text(&url).clicked() {
                    ui.ctx().open_url(egui::OpenUrl::new_tab(&url));
                }
            }
            None => {
                ui.label(RichText::new(&c.name).color(t.colors.text));
            }
        }
        if c.verified {
            chip(ui, t, pt::VERIFIED.get(lang), t.colors.bg3, t.colors.text);
        }
    });
}
