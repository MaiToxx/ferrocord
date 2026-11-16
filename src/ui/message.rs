//! Rendu d'un message et de son texte.

use super::attachments::Hot;
use super::msg_actions::{self, Acts};
use super::reactions;
use super::util::{paint_avatar, pixel_square};
use crate::images::Images;
use crate::i18n::{f, time, Lang};
use crate::state::model::Message;
use crate::theme::{Clock, MessageMode, Tokens};
use super::markdown::{self, Zones};
use crate::state::markdown::Doc;
use eframe::egui::{self, Id, RichText};

/// `images` : avatars ; rien n'est demandé quand ils sont masqués ou hors de l'écran.
/// `doc` : texte du message mis en forme (analysé une fois, gardé par la liste).
/// `acts` : réactions, réponse, modification et actions (lots T5/T6).
#[allow(clippy::too_many_arguments)]
pub(super) fn message_ui_acts(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, m: &Message, doc: &Doc, grouped: bool, ts: i64, acts: &mut Acts) {
    message_inner(ui, t, lang, Some(images), m, doc, grouped, ts, acts);
}

/// Comme `message_ui_acts`, avec un magasin vide (tests).
#[cfg(test)]
#[allow(clippy::too_many_arguments)]
pub(super) fn message_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: &Images, m: &Message, doc: &Doc, grouped: bool, ts: i64) {
    let store = crate::state::Store::new();
    let mut st = msg_actions::ActionsUi::default();
    let mut acts = Acts::new(&mut st, &store, m.channel_id, ui.ctx().clone());
    message_ui_acts(ui, t, lang, images, m, doc, grouped, ts, &mut acts);
}

/// Même mise en page que `message_ui`, pour en connaître la hauteur : ni
/// avatar demandé, ni survol, ni menu (à appeler dans un `Ui` invisible).
#[allow(clippy::too_many_arguments)]
pub(super) fn measure_message(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, doc: &Doc, grouped: bool, ts: i64, acts: &mut Acts) {
    message_inner(ui, t, lang, None, m, doc, grouped, ts, acts);
}

/// Comme `measure_message`, avec un magasin vide (tests).
#[cfg(test)]
pub(super) fn measure_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, doc: &Doc, grouped: bool, ts: i64) {
    let store = crate::state::Store::new();
    let mut st = msg_actions::ActionsUi::default();
    let mut acts = Acts::new(&mut st, &store, m.channel_id, ui.ctx().clone());
    measure_message(ui, t, lang, m, doc, grouped, ts, &mut acts);
}

/// Message survolé (son identifiant), pour le nom de son auteur.
fn lit_id() -> Id {
    Id::unique("ferrocord.messages.nom-survole")
}

/// Rectangles cliquables d'un message (liens, spoilers masqués).
type Links = Zones;

/// `images` absent : mesure seule.
#[allow(clippy::too_many_arguments)]
fn message_inner<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, m: &'a Message, doc: &Doc, grouped: bool, ts: i64, acts: &mut Acts) {
    let compact = t.layout.messages == MessageMode::Compact;
    let avatars = t.message_avatars();
    let h24 = t.layout.clock == Clock::H24;
    let live = images.is_some();
    acts.begin();
    let editing = acts.st.editing(m.id);
    // Parures de l'auteur : celles du serveur affiché, ou du serveur du message (`Acts::home`).
    let home = acts.home;
    let worn_of = move |ctx: &egui::Context| if home { super::cosmetics::worn(ctx, m.guild_id, m.author_id) } else { super::cosmetics::author(ctx, m.author_id) };
    // Message survolé à l'image précédente : le nom de son auteur montre les couleurs de son style.
    let was_lit = live && ui.ctx().data(|d| d.get_temp::<u64>(lit_id())) == Some(m.id);
    // Voile de survol : sa place est prise avant le contenu, il est donc peint derrière le texte.
    let veil = images.map(|_| ui.painter().add(egui::Shape::Noop));
    let mut links = Links::new();
    let mut hot = Hot::default();
    // Avatar et nom de l'auteur, rendus cliquables (carte de profil) après le message.
    let (avatar_at, name_at) = (std::cell::Cell::new(None), std::cell::Cell::new(None));
    // En mode compact chaque ligne porte son auteur, et les lignes sont serrées comme dans un groupe.
    let frame = egui::Frame::new().inner_margin(t.message_margin(grouped || compact));
    let resp = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Message supprimé gardé par le journal (lot E1) : atténué.
            super::journal::dim(ui, m);
            if m.system.is_some() {
                super::system_msgs::system_line(ui, t, lang, m, ts, acts);
                return;
            }
            if compact {
                msg_actions::reply_header(ui, t, lang, images, m, doc, acts, false);
                super::components::header(ui, t, lang, m);
                super::system_msgs::command_header(ui, t, lang, m);
                // Heure, auteur et texte dans une seule mise en page : les lignes
                // renvoyées repartent toutes du bord gauche, quelle que soit la
                // largeur de l'heure ou du nom.
                // L'auteur ouvre sa carte (clic) ou son menu (clic droit).
                let author = Some(markdown::Hot::User { id: m.author_id, name: m.author.clone() });
                let mut lead = vec![
                    (time::clock(lang, ts, false, h24), t.colors.faint, None),
                    (" ".to_string(), t.colors.text, None),
                    (f::compact_author(lang, &m.author), super::cosmetics::author_color(ui.ctx(), t, m.author_id, &worn_of(ui.ctx())), author),
                ];
                // En cours de modification, le texte est dans la zone d'édition.
                let empty = Doc::default();
                let shown = if editing { &empty } else { doc };
                if !shown.is_empty() {
                    lead.push((" ".to_string(), t.colors.text, None));
                }
                markdown::doc_ui_versions(ui, t, lang, m.id, shown, &lead, m.edited && !editing, super::journal::has_versions(m), &mut links);
                egui::Frame::new().inner_margin(t.compact_indent()).show(ui, |ui| {
                    super::journal::trace_ui(ui, t, lang, m);
                    super::translate::under_ui(ui, t, lang, m, &mut hot);
                    msg_actions::edit_box(ui, t, lang, m, acts, live);
                    msg_actions::forward_ui(ui, t, lang, images, m, doc, &mut links, &mut hot);
                    extras_ui(ui, t, lang, images, m, doc, &mut links, &mut hot);
                    super::components::body(ui, t, lang, images, m, doc, &mut links, &mut hot, acts);
                    super::polls::extras(ui, t, lang, images, m, acts);
                    reactions::pills(ui, t, m, acts);
                    super::threads::thread_link(ui, t, lang, m, acts);
                });
                return;
            }
            // Réponse d'un message à avatar : la ligne citée passe au-dessus de
            // l'avatar, reliée à lui par un trait coudé (comme dans Discord).
            let spine = avatars && !grouped && m.meta.reply.is_some();
            if spine {
                egui::Frame::new().inner_margin(t.avatar_indent()).show(ui, |ui| msg_actions::reply_header(ui, t, lang, images, m, doc, acts, true));
            }
            let mut body = |ui: &mut egui::Ui, links: &mut Links, hot: &mut Hot<'a>| {
                if !spine {
                    msg_actions::reply_header(ui, t, lang, images, m, doc, acts, false);
                }
                super::components::header(ui, t, lang, m);
                super::system_msgs::command_header(ui, t, lang, m);
                if !grouped {
                    ui.horizontal(|ui| {
                        // Couleur ou dégradé du rôle, tag de serveur (lot P).
                        // Police du style de nom, couleur ou dégradé du rôle, tag de serveur (lot P) ;
                        // les couleurs du style ne se montrent que sur le message survolé.
                        let worn = worn_of(ui.ctx());
                        name_at.set(Some(super::cosmetics::author_name(ui, t, images, &m.author, m.author_id, &worn, was_lit)));
                        // Compte système de Discord : « ✓ OFFICIEL » après le nom.
                        if super::official::is_peer(ui.ctx(), m.author_id) {
                            super::official::tag(ui, t, lang, t.typography.body);
                        } else if worn.bot || m.meta.app.is_some() {
                            super::cosmetics::app_tag(ui, t, lang);
                        }
                        // Comme dans le client : l'heure seule pour aujourd'hui ; la date complète en infobulle.
                        let stamp = ui.label(RichText::new(time::message(lang, ts, h24)).small().color(t.colors.faint));
                        if live {
                            stamp.on_hover_ui(|ui| {
                                ui.label(time::stamp(lang, ts, 0, 'F', h24));
                            });
                        }
                        // Message `@silent` (F9) : cloche barrée après l'heure.
                        if m.meta.flags & crate::state::builtin::SUPPRESS_NOTIFICATIONS != 0 {
                            let side = crate::theme::Tokens::icon_side(t.typography.small);
                            let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
                            crate::theme::icons::paint(crate::theme::icons::Icon::BellOff, ui.painter(), rect, t.colors.faint);
                            if live {
                                resp.on_hover_text(crate::i18n::table::ff::t::SILENT_TIP.get(lang));
                            }
                        }
                    });
                }
                if editing {
                    msg_actions::edit_box(ui, t, lang, m, acts, live);
                } else if !doc.is_empty() || m.edited {
                    markdown::doc_ui_versions(ui, t, lang, m.id, doc, &[], m.edited, super::journal::has_versions(m), links);
                }
                // Versions dépliées et mention « supprimé » du journal (lot E1).
                super::journal::trace_ui(ui, t, lang, m);
                // Traduction demandée d'un clic droit (lot E3).
                super::translate::under_ui(ui, t, lang, m, hot);
                msg_actions::forward_ui(ui, t, lang, images, m, doc, links, hot);
                extras_ui(ui, t, lang, images, m, doc, links, hot);
                super::components::body(ui, t, lang, images, m, doc, links, hot, acts);
                super::polls::extras(ui, t, lang, images, m, acts);
                reactions::pills(ui, t, m, acts);
                super::threads::thread_link(ui, t, lang, m, acts);
            };
            if !avatars {
                body(ui, &mut links, &mut hot);
                return;
            }
            // Avatar en haut à gauche du premier message d'un groupe, aligné sur le
            // haut du nom ; le texte de tout le groupe est décalé de la même largeur.
            if !grouped {
                let side = t.layout.avatar_size;
                let rect = pixel_square(ui, ui.cursor().min, side);
                if let Some(images) = images {
                    // Avatar du serveur et décoration fixe (lot P).
                    let worn = worn_of(ui.ctx());
                    paint_avatar(ui, images, t, rect, &worn.avatar(m.author_id, &m.author_avatar), &m.author);
                    super::cosmetics::row_decoration(ui, images, rect, &worn);
                    super::profile::message_dot(ui, t, rect, m.author_id);
                    avatar_at.set(Some(rect));
                }
                ui.set_min_height(side);
            }
            egui::Frame::new().inner_margin(t.avatar_indent()).show(ui, |ui| body(ui, &mut links, &mut hot));
        })
        .response;
    let Some(veil) = veil else { return };
    let resp = ui.interact(resp.rect, Id::unique(("msg", m.id)), msg_actions::sense(acts, m));
    // Nom lu par les lecteurs d'écran, sélection au clavier (lot C5).
    let selected = super::msg_select::row(ui, t, lang, m, doc, ts, &resp, !acts.browse);
    // Zones cliquables, barre d'actions et menu contextuel (lots T5/T6).
    // Message supprimé gardé par le journal : plus aucune action chez Discord (lot E1).
    let lit = if super::journal::is_ghost(m) && !acts.browse {
        acts.hits.clear();
        super::journal::ghost_after(t, lang, m, &resp) || selected
    } else {
        acts.grouped = grouped || compact;
        msg_actions::after(ui, t, lang, m, acts, &resp) || selected
    };
    // Survol commencé ou fini : retenu pour l'image suivante, qui est demandée (une seule ;
    // rien au repos).
    if lit != was_lit && !grouped && name_at.get().is_some() {
        ui.ctx().data_mut(|d| {
            if lit {
                d.insert_temp(lit_id(), m.id);
            } else {
                d.remove::<u64>(lit_id());
            }
        });
        ui.ctx().request_repaint();
    }
    // Le voile arrive et repart en fondu ; seules la ligne survolée et les dernières quittées sont suivies.
    let k = super::motion::trail(ui.ctx(), Id::unique("ferrocord.messages.survol"), m.id, lit);
    // Dans la boîte de réception, toutes les mentions le seraient : pas de fond.
    if !acts.home && crate::state::notify::mentioned(acts.store, m) {
        // Message qui me mentionne, comme dans le client : fond ambre translucide sur tout le
        // bloc (ligne de réponse comprise), un peu plus marqué au survol, et barre pleine à gauche.
        let row = t.markdown.mention_row;
        let [r, g, b, _] = row.to_srgba_unmultiplied();
        let bar = egui::Rect::from_min_max(resp.rect.min, egui::pos2(resp.rect.left() + 2.0, resp.rect.bottom()));
        ui.painter().set(
            veil,
            egui::Shape::Vec(vec![
                egui::epaint::RectShape::filled(resp.rect, 0.0, row).into(),
                egui::epaint::RectShape::filled(resp.rect, 0.0, row.gamma_multiply(0.5 * k)).into(),
                egui::epaint::RectShape::filled(bar, 0.0, egui::Color32::from_rgb(r, g, b)).into(),
            ]),
        );
    } else if k > 0.0 {
        ui.painter().set(veil, egui::epaint::RectShape::filled(resp.rect, t.r_hover(), t.colors.hover.gamma_multiply(k)));
    }
    // Message groupé : son heure dans la marge, à la place de l'avatar, tant qu'il est survolé (V9).
    if grouped && avatars && !compact && m.system.is_none() && resp.contains_pointer() {
        let margin = t.message_margin(true);
        let g = ui.painter().layout_no_wrap(time::clock(lang, ts, false, h24), egui::FontId::proportional((t.typography.small - 1.0).max(8.0)), t.colors.faint);
        let left = resp.rect.left() + f32::from(margin.left);
        let line = ui.text_style_height(&egui::TextStyle::Body);
        let x = (left + (t.message_indent() - g.size().x) / 2.0).max(resp.rect.left());
        let y = resp.rect.top() + f32::from(margin.top) + (line - g.size().y) / 2.0;
        ui.painter().galley(egui::pos2(x.round(), y.round()), g, t.colors.faint);
    }
    // Les liens et spoilers viennent après le message : ce sont eux qui reçoivent le clic.
    // (Le menu contextuel, avec « Copier le texte » et « Copier l'identifiant », est dans `msg_actions::after`.)
    markdown::zones_ui(ui, t, m.id, links);
    hot.interact(ui, t, m.id);
    for (i, rect) in [avatar_at.get(), name_at.get()].into_iter().enumerate() {
        // Message reconstitué sans auteur connu (boîte de réception) : pas de carte de profil.
        if let Some(rect) = rect.filter(|_| m.author_id != 0) {
            super::profile::hotspot(ui, lang, rect, (m.id, i), m.author_id, m.guild_id, &m.author, m.author_avatar);
        }
    }
}

/// Pièces jointes et embeds (`attachments.rs`, `embeds.rs`).
#[allow(clippy::too_many_arguments)]
fn extras_ui<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, m: &'a Message, doc: &Doc, links: &mut Links, hot: &mut Hot<'a>) {
    super::attachments::attachments_ui(ui, t, lang, images, m, hot);
    super::embeds::embeds_ui(ui, t, lang, images, m, doc, links, hot);
    super::stickers::stickers_ui(ui, t, lang, m);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::Avatar;

    struct NoNames;
    impl crate::state::markdown::Resolve for NoNames {
        fn user(&self, id: u64) -> String {
            id.to_string()
        }
        fn role(&self, id: u64) -> (String, u32) {
            (id.to_string(), 0)
        }
        fn channel(&self, id: u64) -> String {
            id.to_string()
        }
        fn time(&self, _: i64, _: crate::state::markdown::TimeStyle) -> Option<String> {
            None
        }
    }

    /// Deux passes d'egui sans fenêtre, sur `width` points de large ; renvoie
    /// les formes peintes à la seconde.
    fn pass(width: f32, pointer: Option<egui::Pos2>, mut f: impl FnMut(&mut egui::Ui)) -> Vec<egui::epaint::ClippedShape> {
        let ctx = egui::Context::default();
        let mut shapes = Vec::new();
        for _ in 0..2 {
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(width, 800.0))),
                events: pointer.map(egui::Event::PointerMoved).into_iter().collect(),
                ..Default::default()
            };
            let mut out = ctx.run_ui(input, &mut f);
            // Les textures produites sont jetées (aucun moteur de rendu).
            out.textures_delta.clear();
            shapes = std::mem::take(&mut out.shapes);
        }
        shapes
    }

    /// Voile de survol peint derrière le texte du message.
    #[test]
    fn hover_veil_is_behind_text() {
        let t = Tokens::default();
        let m = Message {
            id: 5 << 22,
            channel_id: 1,
            guild_id: None,
            author_id: 3,
            author: "Bob".into(),
            author_avatar: Avatar::default_for(3),
            content: "bonjour".into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        };
        let doc = crate::state::markdown::parse(&m.content, &NoNames);
        let images = Images::disabled(egui::Context::default());
        let shapes = pass(600.0, Some(egui::pos2(100.0, 20.0)), |ui| message_ui(ui, &t, Lang::Fr, &images, &m, &doc, false, 0));
        let veil = shapes.iter().position(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.fill == t.colors.hover));
        let text = shapes.iter().position(|s| matches!(&s.shape, egui::Shape::Text(_)));
        assert!(veil.is_some() && text.is_some(), "{veil:?} {text:?}");
        assert!(veil < text, "voile {veil:?} après le texte {text:?}");
    }
}
