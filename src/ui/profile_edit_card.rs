//! Éditeur de profil : le profil lui-même, au centre, modifiable sur place.
//!
//! C'est la carte de profil : l'en-tête de `ui::profile` (bannière, avatar,
//! décoration, pastille de statut ; thème et effet du profil) et les pièces
//! de `ui::profile_parts` (bulle du statut, rangée de badges et son
//! infobulle, « Membre depuis », comptes connectés), dont le corps porte des
//! champs à la place des textes : nom affiché (ou pseudo du serveur), au
//! style de nom du profil (`cosmetics::styled_job`, le rendu de la carte),
//! pronoms, tag de serveur, bio. Un clic sur la bulle ouvre la fenêtre du
//! statut personnalisé. Tout changement se voit à l'image même.

use super::cosmetics;
use super::menu;
use super::profile::{name_of, profile_theme, themed_background, Header, Shown};
use super::profile_edit::{Asked, Editor, MAX_BIO, MAX_NAME, MAX_PRONOUNS};
use super::profile_parts;
use super::App;
use crate::i18n::table::nd::t as nt;
use crate::i18n::table::profile::t as pt;
use crate::i18n::table::quick::{f as qf, t as qt};
use crate::i18n::table::s::t as st;
use crate::state::crop::ImageKind;
use crate::state::model::User;
use crate::state::people::{Activity, ActivityKind, Presence};
use crate::state::profiles::ProfileState;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Color32, Id, Rect, RichText, Sense, Stroke, StrokeKind};

/// Contour discret d'un champ posé dans la carte, au survol et à la saisie :
/// il dit qu'on peut écrire là sans encadrer tout le profil.
fn edit_ring(ui: &egui::Ui, t: &Tokens, r: &egui::Response) {
    if r.hovered() || r.has_focus() {
        ui.painter().rect_stroke(r.rect.expand2(egui::vec2(4.0, 2.0)), t.r_row(), Stroke::new(1.0, t.hairline(r.has_focus())), StrokeKind::Outside);
    }
}

/// Hauteur de la carte à l'image d'avant (étendue du dégradé du thème).
fn height_id() -> Id {
    Id::unique("ferrocord.profil.edition.hauteur")
}

impl App {
    /// La carte, large de `width`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn editor_card(&self, ui: &mut egui::Ui, base: &Tokens, e: &mut Editor, me: &User, s: &Shown, width: f32, asked: &mut Asked) {
        let guild = e.tab_guild();
        // Thème du profil : le dégradé couvre la carte, comme sur la carte de profil.
        let mut themed = profile_theme(base, s);
        if let (Some(th), Some(last)) = (&mut themed, ui.data(|d| d.get_temp::<f32>(height_id()))) {
            th.set_span(last);
        }
        let t = themed.as_ref().map_or(base, |th| &th.tokens);
        let width = width - 2.0 * t.shapes.border;
        let pad = t.profile_pad();
        let name = name_of(&me.name, s);
        let framed = egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(t.r_card()).show(ui, |ui| {
            let bg = ui.painter().add(egui::Shape::Noop);
            if let Some(th) = &themed {
                th.apply(ui);
            }
            ui.set_width(width);
            let top = ui.cursor().min;
            // Mêmes proportions que la bande de l'en-tête (voir `profile_header`).
            let band = t.profile_full_banner(true).min((ui.ctx().content_rect().height() * 0.25).round()).max(1.0);
            let header = Header { full: true, themed: themed.as_ref(), banner: e.swap(ImageKind::Banner, width / band), avatar: e.swap(ImageKind::Avatar, 1.0), ..Header::default() };
            self.profile_header(ui, t, width, me.id, guild, s, me.avatar, &name, header);
            // Bulle du statut personnalisé, à droite de l'avatar, sous la bannière
            // (la place de celle de la carte, voir `profile_header`).
            let (side, ring) = (t.profile_full_avatar(), t.profile_ring());
            let left = top.x + pad + side + ring + 2.0 * t.spacing.item_x;
            let at = egui::pos2(left, ui.cursor().top() - side / 2.0 - ring + t.spacing.item_y);
            let under = themed.as_ref().map_or(t.colors.bg1, |th| th.at(top.y, at.y));
            let (clicked, bubble) = self.editor_bubble(ui, t, s, at, top.x + width - pad - left, under);
            asked.status |= clicked;
            // Une bulle de plusieurs lignes pousse le corps, elle ne le recouvre pas.
            let over = bubble.bottom() + t.spacing.item_y - ui.cursor().top();
            if over > 0.0 {
                ui.add_space(over);
            }
            egui::Frame::new().inner_margin(t.profile_margin()).show(ui, |ui| {
                ui.set_width(width - 2.0 * pad);
                ui.spacing_mut().item_spacing.y = t.spacing.item_y;
                self.editor_body(ui, t, e, me, guild, s);
            });
            bg
        });
        ui.data_mut(|d| d.insert_temp(height_id(), framed.response.rect.height()));
        if let Some(th) = &themed {
            themed_background(ui.painter(), th, framed.inner, framed.response.rect, t.r_card(), Some(th.edge(t)));
        }
        self.paint_profile_effect(ui.painter(), s, framed.response.rect, t.r_card());
    }

    /// Bulle du statut personnalisé, celle de la carte (`profile_parts`), ou
    /// la même avec l'invitation à en définir un ; cliquable. Rend le clic
    /// et le rectangle de la bulle.
    fn editor_bubble(&self, ui: &mut egui::Ui, t: &Tokens, s: &Shown, at: egui::Pos2, max_width: f32, under: Color32) -> (bool, Rect) {
        let lang = self.lang;
        let set = s.presence.as_ref().and_then(Presence::custom).and_then(|a| profile_parts::status_bubble(ui, t, a, at, max_width, under));
        let rect = set.or_else(|| {
            let hint = Activity { kind: ActivityKind::Custom, name: String::new(), details: None, state: Some(qt::ED_STATUS_HINT.get(lang).to_string()), emoji: None, start_ms: None, rich: None };
            profile_parts::status_bubble(ui, t, &hint, at, max_width, under)
        });
        let Some(rect) = rect else { return (false, Rect::from_min_size(at, egui::Vec2::ZERO)) };
        let tip = qt::ED_STATUS_TIP.get(lang);
        let resp = ui.interact(rect, Id::unique("ferrocord.profil.edition.bulle"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        if resp.hovered() || resp.has_focus() {
            ui.painter().rect_stroke(rect, t.r_card(), Stroke::new(t.shapes.border.max(1.0), t.colors.text_strong), StrokeKind::Inside);
        }
        (super::widgets::tip(resp.on_hover_cursor(egui::CursorIcon::PointingHand), t, tip).clicked(), rect)
    }

    /// Corps de la carte : identité modifiable, badges, bouton inactif, bio,
    /// date d'inscription, connexions.
    fn editor_body(&self, ui: &mut egui::Ui, t: &Tokens, e: &mut Editor, me: &User, guild: Option<u64>, s: &Shown) {
        let lang = self.lang;
        let c = &t.colors;
        let nitro = e.nitro();
        let on_guild = e.on_guild;
        let user = s.user.as_ref();
        let look = user.map(|u| u.look).unwrap_or_default();
        // Champs du profil affiché : (nom, pronoms, bio), l'indication du nom, son libellé.
        let global = e.main.as_ref().map(|m| m.draft.global_name.trim().to_string()).filter(|n| !n.is_empty());
        let username = e.main.as_ref().map_or_else(|| me.name.clone(), |m| m.base.username.clone());
        let (fields, hint, label) = if on_guild {
            let Some((_, d)) = &mut e.guild else {
                // Profil du serveur pas encore reçu (ou refusé) : rien à modifier pour l'instant.
                let loading = self.store.people.lock().profiles().profile(me.id, e.here) == Some(&ProfileState::Loading);
                let text = if loading { qt::GUILD_LOADING } else { pt::UNAVAILABLE };
                ui.label(RichText::new(text.get(lang)).color(c.muted));
                return;
            };
            ((&mut d.nick, &mut d.pronouns, &mut d.bio), global.unwrap_or_else(|| username.clone()), qt::NICK)
        } else {
            let Some(m) = &mut e.main else {
                // Même ligne d'état que la page Profils (chargement, échec).
                super::settings::account::remote_line(ui, t, lang, &self.store.account.profile, self.h24());
                return;
            };
            let d = &mut m.draft;
            ((&mut d.global_name, &mut d.pronouns, &mut d.bio), username.clone(), st::DISPLAY_NAME)
        };
        let (name, pronouns, bio) = fields;
        // Nom en grand, au style de nom du profil : le rendu de la carte
        // (couleurs, police la plus proche, décor de l'effet derrière le texte).
        let size = (t.typography.title * 1.5).round();
        let style = look.name_style();
        let (bg, strong) = (c.bg1, c.text_strong);
        let font = cosmetics::name_font(ui.ctx(), size, style);
        let mut layouter = move |ui: &egui::Ui, buf: &dyn egui::TextBuffer, _wrap: f32| ui.painter().layout_job(cosmetics::styled_job(ui.ctx(), buf.as_str(), size, style, bg, strong).0);
        // Place du décor, sous le texte du champ.
        let behind = ui.painter().add(egui::Shape::Noop);
        let out = egui::TextEdit::singleline(name)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .char_limit(MAX_NAME)
            .font(font.clone())
            .hint_text(RichText::new(hint).font(font).color(c.muted))
            .desired_width(f32::INFINITY)
            .layouter(&mut layouter)
            .show(ui);
        let backdrop = cosmetics::styled_job(ui.ctx(), "", size, style, bg, strong).1;
        if !backdrop.is_empty() && !out.galley.is_empty() {
            let clip = out.text_clip_rect.expand(size * 0.2);
            ui.painter().with_clip_rect(clip).set(behind, egui::Shape::Vec(cosmetics::backdrop_shapes(out.galley_pos, &out.galley, backdrop)));
        }
        let r = out.response;
        super::a11y::name(ui.ctx(), r.id, label.get(lang));
        edit_ring(ui, t, &r);
        // Nom d'utilisateur, pronoms, tag de serveur.
        let mut tag_pick = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.item_y * 1.5;
            // Le champ des pronoms a la hauteur de son texte : il reste sur la ligne du nom d'utilisateur.
            ui.spacing_mut().interact_size.y = 0.0;
            let size = t.typography.button;
            let handle = user.map_or_else(|| username.clone(), |u| u.handle());
            ui.label(RichText::new(handle).size(size).color(c.text));
            ui.label(RichText::new("•").size(size).color(c.faint));
            // Champ à la largeur de son texte : il tient sur la ligne du nom d'utilisateur.
            let font = egui::FontId::proportional(size);
            let shown = if pronouns.is_empty() { qt::ED_ADD_PRONOUNS.get(lang) } else { pronouns.as_str() };
            let wide = ui.painter().layout_no_wrap(shown.to_string(), font.clone(), c.text).size().x + 2.0 * t.spacing.item_y;
            let r = ui.add(
                egui::TextEdit::singleline(pronouns)
                    .frame(egui::Frame::NONE)
                    .margin(egui::Margin::ZERO)
                    .char_limit(MAX_PRONOUNS)
                    .font(font)
                    .text_color(c.text)
                    .hint_text(RichText::new(qt::ED_ADD_PRONOUNS.get(lang)).size(size).italics().color(c.muted))
                    .desired_width(wide.min(ui.available_width())),
            );
            super::a11y::name(ui.ctx(), r.id, st::PRONOUNS.get(lang));
            edit_ring(ui, t, &r);
            if !on_guild {
                tag_pick = self.editor_tag(ui, t, look.tag());
            }
        });
        if let Some(pick) = tag_pick {
            e.tag = Some(pick);
            e.warned = false;
        }
        // Badges : la rangée de la carte, avec son infobulle détaillée.
        self.badges_ui(ui, t, me.id, s);
        // Le bouton que les autres voient sur ce profil : inactif ici.
        ui.add_space(t.spacing.item_y);
        ui.add_enabled_ui(false, |ui| super::widgets::button(ui, t, super::widgets::Kind::Primary, qt::ED_SEND_MESSAGE.get(lang))).inner.on_disabled_hover_text(qt::ED_SEND_MESSAGE_TIP.get(lang));
        ui.add_space(t.spacing.item_y);
        // Bio : zone de texte sur place, avec son compteur. Celle d'un serveur est réservée à Nitro.
        let bio_allowed = nitro || !on_guild;
        ui.horizontal(|ui| {
            profile_parts::heading(ui, t, qt::ED_BIO.get(lang));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let count = if bio_allowed { qf::count(lang, bio.chars().count(), MAX_BIO) } else { qt::NITRO_ONLY.get(lang).to_string() };
                ui.label(RichText::new(count).small().color(c.muted));
            });
        });
        let r = ui.add_enabled(
            bio_allowed,
            egui::TextEdit::multiline(bio)
                .frame(egui::Frame::NONE)
                .margin(egui::Margin::ZERO)
                .char_limit(MAX_BIO)
                .text_color(c.text)
                .hint_text(RichText::new(qt::ED_BIO_HINT.get(lang)).color(c.muted))
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        );
        super::a11y::name(ui.ctx(), r.id, st::BIO.get(lang));
        edit_ring(ui, t, &r);
        // « Membre depuis » et comptes connectés : les pièces de la carte.
        ui.add_space(t.spacing.item_x);
        self.since_ui(ui, t, me.id, guild, s);
        if let Some(list) = s.profile.as_ref().map(|p| p.connections.as_slice()).filter(|l| !l.is_empty()) {
            ui.add_space(t.spacing.item_x);
            profile_parts::connections_row(ui, t, lang, list);
        }
    }

    /// Pastille du tag de serveur, avec sa liste déroulante : les tags de mes
    /// serveurs, et « Aucun ». Rend le choix fait (`Some(None)` : aucun tag).
    fn editor_tag(&self, ui: &mut egui::Ui, t: &Tokens, shown: Option<crate::state::cosmetics::GuildTag>) -> Option<Option<u64>> {
        let lang = self.lang;
        let c = &t.colors;
        let choices = self.store.tag_choices();
        if shown.is_none() && choices.is_empty() {
            return None;
        }
        let arrow = 14.0;
        let label = nt::TAG.get(lang);
        let resp = match &shown {
            Some(tag) => {
                let chip = cosmetics::TagChip::new(ui, t, tag, c.text);
                let size = chip.size;
                let (rect, resp) = ui.allocate_exact_size(size + egui::vec2(arrow + 4.0, 0.0), Sense::click());
                ui.painter().rect_filled(rect, t.r_row(), c.bg3);
                chip.paint(ui, &self.images, t, Rect::from_min_size(rect.min, size), c.bg3);
                icons::paint(Icon::ChevronDown, ui.painter(), Rect::from_center_size(egui::pos2(rect.right() - arrow / 2.0 - 3.0, rect.center().y), egui::Vec2::splat(arrow - 2.0)), c.text);
                resp
            }
            None => {
                let galley = ui.painter().layout_no_wrap(qt::ED_ADD_TAG.get(lang).to_string(), egui::FontId::new(t.typography.small, super::fonts::demi()), c.muted);
                let (rect, resp) = ui.allocate_exact_size(galley.size() + egui::vec2(arrow + 14.0, 4.0), Sense::click());
                ui.painter().rect_filled(rect, t.r_row(), c.bg3);
                ui.painter().galley(egui::pos2(rect.left() + 6.0, rect.center().y - galley.size().y / 2.0), galley, c.muted);
                icons::paint(Icon::ChevronDown, ui.painter(), Rect::from_center_size(egui::pos2(rect.right() - arrow / 2.0 - 3.0, rect.center().y), egui::Vec2::splat(arrow - 2.0)), c.muted);
                resp
            }
        };
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
        let now = shown.map(|tag| tag.guild_id);
        let mut pick = None;
        egui::Popup::menu(&resp).id(Id::unique("ferrocord.profil.edition.tags")).show(|ui| {
            ui.set_min_width(180.0);
            if menu::radio(ui, t, nt::TAG_NONE.get(lang), now.is_none()).clicked() {
                pick = Some(None);
                ui.close();
            }
            egui::ScrollArea::vertical().max_height(8.0 * menu::row_height(t)).show(ui, |ui| {
                for tag in &choices {
                    let guild = self.store.guilds.iter().find(|g| g.id == tag.guild_id).map(|g| g.name.as_str()).unwrap_or_default();
                    let text = format!("{} · {guild}", tag.text());
                    if menu::radio(ui, t, &text, now == Some(tag.guild_id)).clicked() {
                        pick = Some(Some(tag.guild_id));
                        ui.close();
                    }
                }
            });
        });
        pick.filter(|p| *p != now)
    }
}
