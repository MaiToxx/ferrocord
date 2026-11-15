//! Actions sur les messages (lots T5/T6) : barre d'actions au survol, menu
//! contextuel, réponse (en-tête du message et barre au-dessus de la
//! saisie), modification en place, suppression avec confirmation,
//! épinglage, transfert, copie du texte, du lien et de l'identifiant.
//!
//! Le rendu d'un message (`message.rs`) appelle les fonctions de ce module
//! avec un [`Acts`] : ce qui est cliqué est rangé dans `ActionsUi::out`, et
//! appliqué au magasin après la liste ([`App::message_actions`]) : la liste
//! ne lit le magasin qu'en lecture pendant qu'elle se dessine.

use super::{reactions, App};
use crate::i18n::{f, t as tx, Lang};
use crate::state::model::Message;
use crate::state::social::{Emoji, ReplyState};
use crate::state::store::social::Rights;
use crate::state::Store;
use crate::theme::{icons::Icon, Tokens};
use eframe::egui::{self, pos2, vec2, Id, Key, Rect, RichText, Sense, Stroke};
use std::collections::HashMap;

/// Glyphes testés gardés en mémoire, au plus (vidés au-delà).
const MAX_GLYPH_CACHE: usize = 256;
/// Salons proposés au plus par la fenêtre de transfert.
const MAX_FORWARD_TARGETS: usize = 50;

/// Réponse en préparation (barre au-dessus de la saisie).
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ReplyDraft {
    pub channel_id: u64,
    pub message_id: u64,
    pub author: String,
    pub mention: bool,
}

/// Message en cours de modification.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct EditDraft {
    pub channel_id: u64,
    pub message_id: u64,
    pub text: String,
    /// Le champ prend le focus à sa première image.
    pub focus: bool,
}

/// Fenêtre de transfert ouverte.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ForwardDraft {
    pub channel_id: u64,
    pub message_id: u64,
    pub query: String,
}

/// Sélecteur de réaction ouvert.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Picker {
    pub channel_id: u64,
    pub message_id: u64,
    pub pos: egui::Pos2,
    pub query: String,
}

/// Geste à appliquer au magasin après la liste.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Action {
    Reply(u64),
    React(u64, Emoji),
    Reactors(u64, Emoji, bool),
    Picker(u64, egui::Pos2),
    Edit(u64),
    SaveEdit,
    CancelEdit,
    /// `confirm` : demander d'abord (sauf Maj+clic).
    Delete { id: u64, confirm: bool },
    Pin(u64, bool),
    Forward(u64),
    Jump(u64),
    /// Ouvrir le fil né de ce message (lot T10).
    OpenThread(u64),
    /// « Créer un fil » depuis ce message (lot T10).
    CreateThread(u64),
    /// Bouton, menu, commande de menu contextuel d'une application (lot T11).
    App(super::components::AppAction),
    /// Geste du lot T13 (sondage, annonce, invitation).
    Extra(super::polls::ExtraAction),
    /// Ajoute ou retire le message de la sélection à supprimer (lot A4).
    Select(u64),
    /// « Marquer comme non lu » à partir de ce message (lot M).
    MarkUnread(u64),
    /// « Supprimer les intégrations » (lot M).
    SuppressEmbeds(u64),
    /// Bouton « ⋯ » de la barre : ouvre le menu du message sous lui (lot M).
    More(u64, egui::Pos2),
    /// « Enregistrer le message » ou « Me le rappeler » dans ce délai (ms) (lot ND).
    Save(u64, Option<i64>),
    /// « Mentionner » : la mention de l'auteur du message dans la saisie (lot FF, F27).
    Mention(u64),
}

/// Menu « ⋯ » ouvert depuis la barre de survol.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct More {
    message_id: u64,
    at: egui::Pos2,
    /// Pas encore montré : il s'ouvre à cette image.
    fresh: bool,
}

/// Réactions rapides de la barre et du menu, au plus.
pub(super) const QUICK_REACTIONS: usize = 3;
/// Celles d'un compte qui n'a encore réagi à rien.
const DEFAULT_QUICK: [&str; QUICK_REACTIONS] = ["\u{1f44d}", "\u{2764}\u{fe0f}", "\u{1f602}"];

/// Les réactions rapides : les derniers emojis choisis, complétés par les
/// réactions par défaut.
pub(super) fn quick_reactions<'a>(recent: impl Iterator<Item = &'a Emoji>) -> Vec<Emoji> {
    let mut out: Vec<Emoji> = Vec::with_capacity(QUICK_REACTIONS);
    for e in recent.cloned().chain(DEFAULT_QUICK.iter().map(|s| Emoji::Unicode((*s).into()))) {
        if out.len() == QUICK_REACTIONS {
            break;
        }
        if !out.iter().any(|x| x.same(&e)) {
            out.push(e);
        }
    }
    out
}

/// État de l'interface des actions (appartient à `App`).
#[derive(Default)]
pub(super) struct ActionsUi {
    pub reply: Option<ReplyDraft>,
    pub edit: Option<EditDraft>,
    pub confirm_delete: Option<(u64, u64)>,
    pub picker: Option<Picker>,
    pub forward: Option<ForwardDraft>,
    pub pins_open: bool,
    /// Barre d'actions affichée à l'image précédente : (message, rectangle).
    pub(super) bar: Option<(u64, Rect)>,
    /// Menu « ⋯ » ouvert (la barre reste alors affichée).
    more: Option<More>,
    out: Vec<Action>,
    /// Message dont la hauteur est à refaire (modification ouverte ou fermée).
    relayout: Option<u64>,
    /// Texte d'emoji Unicode : `true` si la police a tous ses glyphes.
    glyphs: HashMap<String, bool>,
    /// Compte pour lequel cet état vaut.
    pub(super) me: Option<u64>,
    /// Menus des composants d'applications en cours de choix (lot T11).
    pub app: super::components::ComponentsUi,
}

impl ActionsUi {
    /// Rectangle de la barre d'actions affichée (tests).
    #[cfg(test)]
    pub(super) fn bar_rect(&self) -> Option<Rect> {
        self.bar.map(|(_, r)| r)
    }

    /// Changement de compte : rien ne reste.
    pub(super) fn clear(&mut self) {
        *self = ActionsUi::default();
    }

    pub(super) fn push(&mut self, a: Action) {
        self.out.push(a);
    }

    /// Gestes rangés depuis le dernier appel (panneau de recherche, lot T12).
    pub(super) fn take_out(&mut self) -> Vec<Action> {
        std::mem::take(&mut self.out)
    }

    pub(super) fn editing(&self, id: u64) -> bool {
        self.edit.as_ref().is_some_and(|e| e.message_id == id)
    }

    /// Texte affiché pour un emoji : le caractère si la police l'a, sinon son
    /// nom court (`:fire:`), sinon le caractère quand même.
    pub(super) fn emoji_text(&mut self, ctx: &egui::Context, e: &Emoji) -> String {
        let text = e.display();
        let Emoji::Unicode(_) = e else { return text };
        if self.glyphs.len() >= MAX_GLYPH_CACHE {
            self.glyphs.clear();
        }
        let ok = *self.glyphs.entry(text.clone()).or_insert_with(|| {
            let font = egui::FontId::default();
            ctx.fonts_mut(|f| text.chars().filter(|c| *c != '\u{200D}').all(|c| f.has_glyph(&font, c)))
        });
        if ok {
            return text;
        }
        reactions::shortcode(&text).map_or(text, |n| format!(":{n}:"))
    }
}

/// Ce que le rendu d'un message reçoit pour ses actions.
pub(super) struct Acts<'a> {
    pub st: &'a mut ActionsUi,
    pub store: &'a Store,
    pub rights: Rights,
    pub ctx: egui::Context,
    /// Zones cliquables du message (en-tête de réponse, pastilles),
    /// rendues interactives après le message pour passer devant lui.
    pub hits: Vec<(Rect, Hit)>,
    /// Message montré hors de sa liste (résultat de recherche, lot T12) :
    /// ni barre d'actions ni menu, un clic va au message.
    pub browse: bool,
    /// Réactions rapides de la barre de survol et du menu (lot M) ; vide :
    /// aucune (recherche, épingles, tests).
    pub quick: Vec<Emoji>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Hit {
    /// En-tête de réponse : aller au message s'il est chargé.
    Reply(Option<u64>),
    Pill { emoji: Emoji, burst: bool },
    AddReaction,
    /// Résumé du fil né du message (lot T10).
    Thread(u64),
    /// Sondage, message système, invitation, annonce (lot T13).
    Extra(super::polls::ExtraHit),
}

impl<'a> Acts<'a> {
    pub(super) fn new(st: &'a mut ActionsUi, store: &'a Store, channel_id: u64, ctx: egui::Context) -> Self {
        Acts { st, rights: store.rights(channel_id), store, ctx, hits: Vec::new(), browse: false, quick: Vec::new() }
    }

    /// Début d'un message.
    pub(super) fn begin(&mut self) {
        self.hits.clear();
    }
}

/// Le message réagit-il aux clics ? Pas pendant sa modification : le champ
/// de saisie, dessiné avant, doit les recevoir.
pub(super) fn sense(acts: &Acts, m: &Message) -> Sense {
    if acts.st.editing(m.id) { Sense::hover() } else { Sense::click() }
}

// ====================================================================== dessin dans le message

/// Ligne « ╭ @auteur extrait » au-dessus d'une réponse. `spine` : la ligne
/// est posée au-dessus de l'avatar, et le trait coudé part de lui (colonne
/// de l'avatar, à gauche de la ligne) ; sinon le coude est dans la ligne.
#[allow(clippy::too_many_arguments)]
pub(super) fn reply_header(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&crate::images::Images>, m: &Message, doc: &crate::state::markdown::Doc, acts: &mut Acts, spine: bool) {
    let Some(r) = &m.meta.reply else { return };
    let small = t.typography.small;
    let row = ui
        .horizontal(|ui| {
            if !spine {
                // Trait coudé qui relie la réponse au message cité.
                let side = Tokens::icon_side(small);
                let (rect, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                let s = Stroke::new(t.shapes.border.max(1.0), t.colors.faint);
                let (l, top, mid) = (rect.center().x, rect.center().y, rect.right());
                ui.painter().line_segment([pos2(l, rect.bottom()), pos2(l, top)], s);
                ui.painter().line_segment([pos2(l, top), pos2(mid, top)], s);
            }
            match &r.state {
                ReplyState::Loaded { author_id, author, excerpt } => {
                    // Comme dans le client : mini-avatar de l'auteur cité, « @Nom » dans la police de
                    // son style de nom (à la couleur de son rôle, sinon unie), puis son tag de serveur.
                    let worn = super::cosmetics::author(ui.ctx(), *author_id);
                    let avatar = super::profile::hub(ui.ctx()).and_then(|hub| hub.lock().user(*author_id).map(|u| u.avatar));
                    if let (Some(avatar), true) = (avatar, t.message_avatars()) {
                        // Place prise aussi à la mesure (sans images) : la ligne garde sa hauteur.
                        let side = Tokens::icon_side(small);
                        let (slot, _) = ui.allocate_exact_size(vec2(side, side), Sense::hover());
                        if let Some(images) = images {
                            let at = super::util::pixel_square(ui, slot.min, side);
                            super::util::paint_avatar(ui, images, t, at, &worn.avatar(*author_id, &avatar), author);
                        }
                    }
                    let font = super::cosmetics::name_font(ui.ctx(), small, worn.look.name_style());
                    let color = super::cosmetics::author_color(ui.ctx(), t, *author_id, &worn);
                    super::emoji::label_job(ui, egui::text::LayoutJob::simple_singleline(f::reply_author(lang, author), font, color), false);
                    if let (Some(tag), Some(images)) = (worn.look.tag(), images) {
                        super::cosmetics::tag_ui(ui, images, t, &tag);
                    }
                    // Extrait mis en forme avec le message (une ligne), sinon son texte simple.
                    match doc.reply.as_deref().filter(|d| !d.is_empty()) {
                        Some(d) => {
                            super::markdown::line_ui(ui, t, lang, m.id, d, small, t.colors.muted);
                        }
                        None => {
                            let text = if excerpt.is_empty() { tx::REPLY_ATTACHMENT.get(lang) } else { excerpt.as_str() };
                            ui.add(egui::Label::new(RichText::new(text).size(small).color(t.colors.muted)).truncate());
                        }
                    }
                }
                ReplyState::Deleted => {
                    ui.label(RichText::new(tx::REPLY_DELETED.get(lang)).size(small).italics().color(t.colors.faint));
                }
                ReplyState::Unknown => {
                    ui.label(RichText::new(tx::REPLY_UNKNOWN.get(lang)).size(small).italics().color(t.colors.faint));
                }
            }
        })
        .response
        .rect;
    if spine {
        // Du milieu de l'avatar (dessous) au début de la ligne, coin arrondi.
        let s = Stroke::new(2.0 * t.shapes.border.max(1.0), t.markdown.quote_bar);
        let x = row.left() - t.spacing.avatar_gap - t.layout.avatar_size / 2.0;
        let (y, end) = (row.center().y, row.left() - t.spacing.item_x / 2.0);
        let bottom = row.bottom() + ui.spacing().item_spacing.y - s.width;
        let k = t.shapes.row.min((bottom - y).max(0.0)).min((end - x).max(0.0));
        let p = ui.painter();
        p.line_segment([pos2(x, bottom), pos2(x, y + k)], s);
        p.add(egui::epaint::QuadraticBezierShape::from_points_stroke([pos2(x, y + k), pos2(x, y), pos2(x + k, y)], false, egui::Color32::TRANSPARENT, s));
        p.line_segment([pos2(x + k, y), pos2(end, y)], s);
    }
    acts.hits.push((row, Hit::Reply(r.message_id)));
}

/// Champ de modification à la place du texte.
pub(super) fn edit_box(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts, live: bool) {
    let Some(draft) = acts.st.edit.as_mut().filter(|e| e.message_id == m.id) else { return };
    // Mesure : un autre identifiant, pour ne pas toucher à l'état du vrai champ.
    let id = Id::unique(("modification", m.id, live));
    let focused = live && ui.memory(|mem| mem.has_focus(id));
    let (enter, escape) = if focused {
        ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, Key::Enter), i.consume_key(egui::Modifiers::NONE, Key::Escape)))
    } else {
        (false, false)
    };
    egui::Frame::new().fill(t.colors.bg3).corner_radius(t.r_composer()).inner_margin(t.composer_inner()).show(ui, |ui| {
        ui.add(egui::TextEdit::multiline(&mut draft.text).id(id).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY));
    });
    if live && std::mem::take(&mut draft.focus) {
        ui.memory_mut(|mem| mem.request_focus(id));
    }
    ui.label(RichText::new(tx::EDIT_HINT.get(lang)).size(t.typography.small).color(t.colors.faint));
    if escape {
        acts.st.push(Action::CancelEdit);
    } else if enter {
        acts.st.push(Action::SaveEdit);
    }
}

/// Message transféré : son instantané derrière une barre, comme une citation.
pub(super) fn forward_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, doc: &crate::state::markdown::Doc, zones: &mut super::markdown::Zones) {
    let Some(fw) = &m.meta.forward else { return };
    let block = egui::Frame::new().inner_margin(t.quote_indent()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            glyph(ui, Glyph::Forward, t.typography.small, t.colors.faint);
            ui.label(RichText::new(tx::FORWARDED.get(lang)).size(t.typography.small).italics().color(t.colors.faint));
        });
        match doc.forward.as_deref() {
            Some(d) => super::markdown::doc_ui(ui, t, lang, m.id, d, &[], false, zones),
            None if !fw.content.is_empty() => {
                ui.add(egui::Label::new(RichText::new(&fw.content).color(t.colors.text)).wrap().selectable(true));
            }
            None => {}
        }
        for a in &fw.attachments {
            ui.label(RichText::new(a).color(t.colors.link));
        }
    });
    let r = block.response.rect;
    let w = t.markdown.quote_width;
    ui.painter().rect_filled(Rect::from_min_max(r.min, pos2(r.left() + w, r.bottom())), w / 2.0, t.markdown.quote_bar);
}

// ====================================================================== après le message

/// Zones cliquables, barre d'actions au survol et menu contextuel. Renvoie
/// `true` si le message est survolé (voile).
pub(super) fn after(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts, resp: &egui::Response) -> bool {
    if acts.browse {
        return super::search::browse(ui, m, acts, resp);
    }
    // Sélection pour la suppression en masse (lot A4) : un clic ajoute ou retire.
    if acts.store.moderation.selecting(m.channel_id) {
        let r = ui.interact(resp.rect, Id::unique(("selection", m.id)), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand);
        if acts.store.moderation.is_selected(m.channel_id, m.id) {
            // Dessiné après le message : un cadre seul, pour ne pas le couvrir.
            ui.painter().rect_stroke(resp.rect, t.r_row(), egui::Stroke::new(t.border().width * 2.0, t.colors.danger), egui::StrokeKind::Inside);
        }
        if r.clicked() {
            acts.st.push(Action::Select(m.id));
        }
        return r.hovered();
    }
    let hits = std::mem::take(&mut acts.hits);
    for (i, (rect, hit)) in hits.into_iter().enumerate() {
        let r = ui.interact(rect, Id::unique(("geste", m.id, i)), Sense::click());
        match hit {
            // Message cité hors de la mémoire : chargé autour de lui (lot T12).
            Hit::Reply(Some(target)) => {
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                    acts.st.push(Action::Jump(target));
                }
            }
            Hit::Reply(_) => {}
            Hit::Thread(id) => {
                if r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(crate::i18n::table::threads::t::OPEN_THREAD.get(lang)).clicked() {
                    acts.st.push(Action::OpenThread(id));
                }
            }
            Hit::Extra(hit) => super::polls::interact(lang, m, acts, r, hit),
            Hit::Pill { emoji, burst } => reactions::pill_interact(ui, t, lang, m, acts, r, emoji, burst),
            Hit::AddReaction => {
                let r = r.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tx::ADD_REACTION.get(lang));
                if r.clicked() {
                    acts.st.push(Action::Picker(m.id, r.rect.left_bottom()));
                }
            }
        }
    }
    let pointer = ui.ctx().pointer_hover_pos();
    let on_bar = acts.st.bar.is_some_and(|(id, r)| id == m.id && pointer.is_some_and(|p| r.contains(p)));
    // egui tient aussi pour survolé un message à quelques points du pointeur :
    // entre deux messages, un seul porte la barre (le bas d'un message est exclu,
    // c'est le haut du suivant).
    // Menu « ⋯ » ouvert : le message garde sa barre, où que soit le pointeur.
    let more = acts.st.more.filter(|x| x.message_id == m.id);
    let hovered = (resp.contains_pointer() && pointer.is_some_and(|p| p.y >= resp.rect.top() && p.y < resp.rect.bottom())) || on_bar || more.is_some();
    // Disposition mobile : pas de survol au doigt, l'appui long ouvre le menu (phase 7).
    let had_bar = acts.st.bar.is_some_and(|(id, _)| id == m.id);
    if hovered && !acts.st.editing(m.id) && !super::mobile::is_active(ui.ctx()) {
        let bar = hover_bar(ui, t, lang, m, acts, resp.rect);
        acts.st.bar = Some((m.id, bar));
    } else if had_bar {
        acts.st.bar = None;
    }
    // Le « + » fantôme des réactions suit la barre : une image de plus quand elle arrive ou part.
    if had_bar != acts.st.bar.is_some_and(|(id, _)| id == m.id) && !m.meta.reactions.is_empty() {
        ui.ctx().request_repaint();
    }
    resp.context_menu(|ui| menu(ui, t, lang, m, acts));
    if let Some(mo) = more {
        let id = Id::unique(("plus", m.id));
        egui::Popup::new(id, ui.ctx().clone(), egui::PopupAnchor::Position(mo.at), resp.layer_id)
            .kind(egui::PopupKind::Menu)
            .layout(egui::Layout::top_down_justified(egui::Align::Min))
            .style(egui::containers::menu::menu_style)
            .open_memory(mo.fresh.then_some(egui::SetOpenCommand::Bool(true)))
            .show(|ui| menu(ui, t, lang, m, acts));
        // Un autre « ⋯ » a pu être demandé entre-temps : il reste.
        if acts.st.more == Some(mo) {
            acts.st.more = (mo.fresh || egui::Popup::is_id_open(ui.ctx(), id)).then_some(More { fresh: false, ..mo });
            if acts.st.more != Some(mo) {
                ui.ctx().request_repaint();
            }
        }
    }
    hovered
}

/// Ce que l'utilisateur peut faire sur ce message.
pub(super) struct Allowed {
    pub reply: bool,
    pub react: bool,
    pub edit: bool,
    pub delete: bool,
    pub pin: bool,
    pub forward: bool,
    pub thread: bool,
}

pub(super) fn allowed(m: &Message, r: &Rights) -> Allowed {
    let normal = m.system.is_none();
    let mine = m.author_id == r.me;
    Allowed {
        reply: normal && r.send,
        react: r.react,
        edit: mine && normal && m.meta.forward.is_none(),
        delete: mine || r.manage,
        pin: normal && r.pin,
        // Un transfert ne se transfère pas (imbrication limitée à un niveau).
        forward: normal && m.meta.forward.is_none(),
        thread: normal && r.thread && m.meta.thread.is_none(),
    }
}

/// Adresse du message dans le client web de Discord.
pub(super) fn message_link(m: &Message) -> String {
    let guild = m.guild_id.map_or_else(|| "@me".to_string(), |g| g.to_string());
    format!("https://discord.com/channels/{guild}/{}/{}", m.channel_id, m.id)
}

/// Menu du message (clic droit, ou « ⋯ » de la barre), dans l'ordre de
/// Discord : réactions rapides, actions, copies, puis ce qui est destructeur.
fn menu(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts) {
    let a = allowed(m, &acts.rights);
    let item = |ui: &mut egui::Ui, show: bool, text: Txt, icon: Icon| -> bool {
        let clicked = show && crate::ui::menu::item(ui, t, text.get(lang), Some(icon), false).clicked();
        if clicked {
            ui.close();
        }
        clicked
    };
    let pos = ui.min_rect().left_top();
    let pin = if m.meta.pinned { tx::UNPIN_MESSAGE } else { tx::PIN_MESSAGE };
    let mut chosen = None;
    if a.react && !acts.quick.is_empty() {
        ui.horizontal(|ui| {
            for e in acts.quick.clone() {
                if emoji_button(ui, t, acts, &e, bar_side(ui, t)).clicked() {
                    chosen = Some(Action::React(m.id, e));
                    ui.close();
                }
            }
        });
        crate::ui::menu::separator(ui, t);
    }
    for (show, text, icon, action) in [
        (a.react, tx::ADD_REACTION, Icon::Smile, Action::Picker(m.id, pos)),
        (a.edit, tx::EDIT_MESSAGE, Icon::Pencil, Action::Edit(m.id)),
        (a.reply, tx::REPLY, Icon::Reply, Action::Reply(m.id)),
        (a.forward, tx::FORWARD, Icon::Forward, Action::Forward(m.id)),
        (a.thread, crate::i18n::table::threads::t::CREATE_THREAD, Icon::Thread, Action::CreateThread(m.id)),
    ] {
        if item(ui, show, text, icon) {
            chosen = Some(action);
        }
    }
    // « Mentionner » l'auteur (F27) : pas moi, pas un webhook ni un message système.
    if item(ui, a.reply && m.author_id != 0 && acts.store.me.as_ref().is_some_and(|me| me.id != m.author_id), crate::i18n::table::profile::t::MENU_MENTION, Icon::Reply) {
        chosen = Some(Action::Mention(m.id));
    }
    // Commandes de menu contextuel des applications (lot T11).
    super::components::apps_menu(ui, t, lang, m, acts);
    if item(ui, acts.store.can_publish(m), crate::i18n::table::polls::t::PUBLISH, Icon::Megaphone) {
        chosen = Some(Action::Extra(super::polls::ExtraAction::Publish(m.id)));
    }
    crate::ui::menu::separator(ui, t);
    if item(ui, true, tx::COPY_TEXT, Icon::Copy) {
        ui.ctx().copy_text(acts.store.raw_content(m.channel_id, m.id).unwrap_or_else(|| m.content.clone()));
    }
    // Lire à voix haute (synthèse vocale de Windows) ; pendant une lecture : l'arrêter.
    // Plugin « Lecture à voix haute » : coupé, la ligne n'existe pas.
    if crate::platform::speech::AVAILABLE && crate::plugins::on(crate::plugins::Id::ReadAloud) && !m.content.trim().is_empty() {
        if crate::platform::speech::speaking() {
            if item(ui, true, crate::i18n::table::restes::t::STOP_SPEAKING, Icon::Speaker) {
                crate::platform::speech::stop();
            }
        } else if item(ui, true, crate::i18n::table::restes::t::SPEAK, Icon::Speaker) {
            if crate::plugins::flag(crate::plugins::Id::ReadAloud, crate::plugins::READ_ALOUD_AUTHOR) {
                crate::platform::speech::speak(&format!("{} : {}", m.author, m.content));
            } else {
                crate::platform::speech::speak(&m.content);
            }
        }
    }
    // Traduire, ou revenir à l'original (plugin « Traduction d'un message », lot E3).
    if let Some((text, on)) = super::translate::menu_entry(m)
        && item(ui, true, text, if on { Icon::Chat } else { Icon::Eye })
    {
        super::translate::ask(ui.ctx(), m.channel_id, m.id, on);
    }
    if item(ui, a.pin, pin, Icon::Pin) {
        chosen = Some(Action::Pin(m.id, !m.meta.pinned));
    }
    // Messages enregistrés et rappels (lot ND) : une écriture par clic.
    {
        use crate::i18n::table::nd::t as nd;
        use crate::state::saved::{DAY_MS, HOUR_MS};
        for (text, icon, delay) in [(nd::SAVE_MESSAGE, Icon::Star, None), (nd::REMIND_HOUR, Icon::Clock, Some(HOUR_MS)), (nd::REMIND_DAY, Icon::Clock, Some(DAY_MS))] {
            if item(ui, true, text, icon) {
                chosen = Some(Action::Save(m.id, delay));
            }
        }
    }
    if item(ui, acts.store.can_mark_unread(), mx::MARK_UNREAD, Icon::Eye) {
        chosen = Some(Action::MarkUnread(m.id));
    }
    if item(ui, true, tx::COPY_LINK, Icon::Link) {
        ui.ctx().copy_text(message_link(m));
    }
    if item(ui, acts.store.can_suppress_embeds(m), mx::SUPPRESS_EMBEDS, Icon::Close) {
        chosen = Some(Action::SuppressEmbeds(m.id));
    }
    // Image ou vidéo jointe : son adresse ; une image se copie et s'enregistre aussi (lot VID).
    if let Some(media) = m.attachments.iter().find(|x| matches!(x.kind, crate::state::model::MediaKind::Image | crate::state::model::MediaKind::Video)) {
        crate::ui::menu::separator(ui, t);
        if media.kind == crate::state::model::MediaKind::Image {
            if item(ui, true, mx::COPY_IMAGE, Icon::Copy) {
                crate::ui::image_ops::ask(ui.ctx(), crate::ui::image_ops::Op::Copy, &media.url, media.display_name());
            }
            if crate::ui::image_ops::CAN_SAVE && item(ui, true, mx::SAVE_IMAGE, Icon::Download) {
                crate::ui::image_ops::ask(ui.ctx(), crate::ui::image_ops::Op::Save, &media.url, media.display_name());
            }
        }
        if item(ui, true, mx::COPY_MEDIA_LINK, Icon::Link) {
            ui.ctx().copy_text(media.url.clone());
        }
        if item(ui, true, mx::OPEN_MEDIA, Icon::Media) {
            ui.ctx().open_url(egui::OpenUrl::new_tab(&media.url));
        }
    }
    crate::ui::menu::separator(ui, t);
    if super::settings::local::copy_id_shown(&acts.ctx, acts.store) && item(ui, true, tx::COPY_ID, Icon::Id) {
        ui.ctx().copy_text(m.id.to_string());
    }
    if acts.store.can_bulk_delete(m.channel_id) && item(ui, true, crate::i18n::table::moderation::t::MENU_SELECT, Icon::Check) {
        chosen = Some(Action::Select(m.id));
    }
    // Le signalement est un formulaire de Discord : il s'ouvre sur le message, dans le navigateur.
    if item(ui, m.author_id != acts.rights.me && m.system.is_none(), mx::REPORT, Icon::Flag) {
        ui.ctx().open_url(egui::OpenUrl::new_tab(message_link(m)));
    }
    if a.delete {
        crate::ui::menu::separator(ui, t);
        let shift = ui.input(|i| i.modifiers.shift);
        if crate::ui::menu::item(ui, t, tx::DELETE_MESSAGE.get(lang), Some(crate::theme::icons::Icon::Trash), true).clicked() {
            ui.close();
            chosen = Some(Action::Delete { id: m.id, confirm: !shift });
        }
    }
    if let Some(action) = chosen {
        acts.st.push(action);
    }
}

use crate::i18n::table::messages::t as mx;

/// Côté d'un bouton de la barre de survol : une icône d'en-tête et la marge d'un bouton.
fn bar_side(ui: &egui::Ui, t: &Tokens) -> f32 {
    t.typography.icon + 2.0 * ui.spacing().button_padding.y
}

/// Bouton d'une réaction rapide : l'emoji (image, sinon son texte), nommé
/// pour les lecteurs d'écran.
fn emoji_button(ui: &mut egui::Ui, t: &Tokens, acts: &mut Acts, e: &Emoji, side: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
    if resp.hovered() || resp.has_focus() {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let inner = rect.shrink(ui.spacing().button_padding.y);
    let name = match e {
        Emoji::Unicode(s) => reactions::shortcode(s).map_or_else(|| s.clone(), |n| format!(":{n}:")),
        Emoji::Custom { name, .. } => format!(":{name}:"),
    };
    match super::emoji::Img::of(e) {
        Some(img) => super::emoji::paint(ui, inner, img),
        None => {
            let ctx = acts.ctx.clone();
            let text = acts.st.emoji_text(&ctx, e);
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, text, egui::FontId::proportional(t.typography.body), t.colors.text);
        }
    }
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(name)
}

type Txt = crate::i18n::Text;

fn bar_id() -> Id {
    Id::unique("barre")
}

/// Couche de la barre d'actions : la liste lui reprend la molette (`chat.rs`).
pub(super) fn bar_layer() -> egui::LayerId {
    egui::LayerId::new(egui::Order::Foreground, bar_id())
}

/// Barre d'actions en haut à droite du message survolé, comme dans Discord :
/// réactions rapides, réagir, modifier ou répondre, transférer, « ⋯ » (le
/// reste est dans le menu). Maj tenue : supprimer en plus. Renvoie son rectangle.
fn hover_bar(ui: &mut egui::Ui, t: &Tokens, lang: Lang, m: &Message, acts: &mut Acts, rect: Rect) -> Rect {
    let a = allowed(m, &acts.rights);
    let shift = ui.input(|i| i.modifiers.shift);
    let quick = if a.react { acts.quick.clone() } else { Vec::new() };
    let mut glyphs: Vec<(Glyph, Txt)> = Vec::new();
    if a.react {
        glyphs.push((Glyph::React, tx::ADD_REACTION));
    }
    if a.edit {
        glyphs.push((Glyph::Edit, tx::EDIT_MESSAGE));
    }
    if a.reply {
        glyphs.push((Glyph::Reply, tx::REPLY));
    }
    if a.forward {
        glyphs.push((Glyph::Forward, tx::FORWARD));
    }
    if a.delete && shift {
        glyphs.push((Glyph::Delete, tx::DELETE_MESSAGE));
    }
    glyphs.push((Glyph::More, mx::MORE));
    let side = bar_side(ui, t);
    let count = quick.len() + glyphs.len();
    // Filet entre les réactions rapides et les actions.
    let rule = if quick.is_empty() { 0.0 } else { t.spacing.item_x };
    let margin = egui::Vec2::splat(2.0 * t.shapes.border);
    let size = vec2(side * count as f32 + rule, side) + margin;
    // Moitié au-dessus du message, comme dans Discord ; jamais hors de la liste.
    let clip = ui.clip_rect();
    let x = rect.right() - t.spacing.message_x - size.x;
    let y = (rect.top() - size.y / 2.0).max(clip.top());
    // Un seul identifiant pour la barre, quel que soit le message, et pas de
    // fondu : la barre réapparaît après chaque changement de salon sous la
    // souris, et le fondu d'egui coûtait une dizaine d'images à chaque fois.
    let area = egui::Area::new(bar_id()).order(egui::Order::Foreground).fixed_pos(pos2(x, y)).constrain(false).fade_in(false);
    let inner = area.show(ui.ctx(), |ui| {
        // Fond de la discussion : la barre s'en détache par son contour et son ombre.
        t.float_frame().fill(t.colors.bg2).corner_radius(t.r_control()).inner_margin(margin / 2.0).show(ui, |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.horizontal(|ui| {
                for e in &quick {
                    if emoji_button(ui, t, acts, e, side).clicked() {
                        acts.st.push(Action::React(m.id, e.clone()));
                    }
                }
                if !quick.is_empty() {
                    let (r, _) = ui.allocate_exact_size(vec2(rule, side), Sense::hover());
                    ui.painter().vline(r.center().x, r.shrink2(vec2(0.0, side / 4.0)).y_range(), Stroke::new(t.shapes.border, t.colors.border));
                }
                for (g, tip) in &glyphs {
                    let r = bar_button(ui, t, *g, tip.get(lang), side);
                    if r.clicked() {
                        acts.st.push(match g {
                            Glyph::React => Action::Picker(m.id, r.rect.left_bottom()),
                            Glyph::Reply => Action::Reply(m.id),
                            Glyph::Edit => Action::Edit(m.id),
                            Glyph::Forward => Action::Forward(m.id),
                            Glyph::Pin => Action::Pin(m.id, !m.meta.pinned),
                            Glyph::Delete => Action::Delete { id: m.id, confirm: !shift },
                            Glyph::More => Action::More(m.id, r.rect.left_bottom()),
                        });
                    }
                }
            });
        });
    });
    inner.response.rect
}

/// Bouton-icône de la barre de survol (`side` de côté, icône d'en-tête).
fn bar_button(ui: &mut egui::Ui, t: &Tokens, g: Glyph, tip: &str, side: f32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let color = if hot { t.colors.text_strong } else { t.colors.muted };
    paint_glyph(g, ui.painter(), rect.shrink(ui.spacing().button_padding.y), color);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    resp.on_hover_text(tip)
}

// ====================================================================== petites icônes

/// Icônes de la barre d'actions, dessinées comme celles de `theme::icons`
/// (proportions seulement, taille et couleur venues du thème).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Glyph {
    Reply,
    React,
    Edit,
    Forward,
    Pin,
    Delete,
    /// « ⋯ » : le reste des actions.
    More,
}

pub(super) fn paint_glyph(g: Glyph, p: &egui::Painter, rect: Rect, color: egui::Color32) {
    let icon = match g {
        Glyph::Reply => Icon::Reply,
        Glyph::React => Icon::Smile,
        Glyph::Edit => Icon::Pencil,
        Glyph::Forward => Icon::Forward,
        Glyph::Pin => Icon::Pin,
        Glyph::Delete => Icon::Trash,
        Glyph::More => Icon::More,
    };
    crate::theme::icons::paint(icon, p, rect, color);
}

/// Icône alignée sur une ligne de texte de taille `size`.
pub(super) fn glyph(ui: &mut egui::Ui, g: Glyph, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(Tokens::icon_side(size)), Sense::hover());
    paint_glyph(g, ui.painter(), rect, color);
}

// ====================================================================== application

impl App {
    /// Envoi du texte saisi : réponse si une réponse est en préparation dans
    /// ce salon, message ordinaire sinon. `false` si rien ne part.
    pub(super) fn send_text(&mut self, channel_id: u64, text: String) -> bool {
        match self.actions.reply.take() {
            Some(r) if r.channel_id == channel_id => {
                let sent = self.store.send_reply(channel_id, text, r.message_id, r.mention);
                if !sent {
                    self.actions.reply = Some(r);
                }
                sent
            }
            other => {
                self.actions.reply = other;
                self.store.send_message(channel_id, text)
            }
        }
    }

    /// Barre « Réponse à … » au-dessus de la saisie (Échap l'annule).
    /// Aperçu hors ligne : réponse en préparation à `author`, dans le salon affiché.
    #[doc(hidden)]
    pub fn preview_reply(&mut self, author: &str) {
        if let Some(cid) = self.store.channel {
            // Compte déjà connu : la première image ne vide pas ce qui vient d'être posé.
            self.actions.me = self.store.me.as_ref().map(|u| u.id);
            self.actions.reply = Some(ReplyDraft { channel_id: cid, message_id: 0, author: author.to_string(), mention: true });
        }
    }

    /// Rend vrai si la bande est affichée (la carte de saisie s'y colle).
    pub(super) fn reply_bar(&mut self, ui: &mut egui::Ui, cid: u64, composer: Id) -> bool {
        let t = self.theme.clone();
        let lang = self.lang;
        // Autocomplétion ouverte (état de l'image précédente) : Échap est à
        // elle d'abord, la réponse reste.
        let popup = self.input.popup_open();
        let Some(r) = self.actions.reply.as_mut().filter(|r| r.channel_id == cid) else { return false };
        let mut cancel = !popup && ui.memory(|m| m.has_focus(composer)) && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Escape));
        // Bande collée à la carte de saisie : coins du haut arrondis comme elle, bas droit.
        let rc = t.r_composer();
        let corners = egui::CornerRadius { nw: rc.nw, ne: rc.ne, sw: 0, se: 0 };
        let pad = egui::Margin::symmetric(t.spacing.composer_inner_x.round() as i8, (t.spacing.item_y / 2.0).round() as i8);
        egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(corners).inner_margin(pad).show(ui, |ui| {
            let row = vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                cancel |= super::util::icon_button(ui, &t, crate::theme::icons::Icon::Close, tx::CANCEL_REPLY.get(lang)).clicked();
                ui.checkbox(&mut r.mention, tx::MENTION_AUTHOR.get(lang));
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    glyph(ui, Glyph::Reply, t.typography.button, t.colors.muted);
                    // « Réponse à » en gris, le nom en clair et demi-gras.
                    let whole = f::replying_to(lang, "\u{1}");
                    let (before, after) = whole.split_once('\u{1}').unwrap_or((whole.as_str(), ""));
                    let mut job = egui::text::LayoutJob::default();
                    let plain = egui::FontId::proportional(t.typography.button);
                    job.append(before, 0.0, egui::TextFormat::simple(plain.clone(), t.colors.muted));
                    job.append(&r.author, 0.0, egui::TextFormat::simple(crate::ui::fonts::demi_font(t.typography.button), t.colors.text_strong));
                    job.append(after, 0.0, egui::TextFormat::simple(plain, t.colors.muted));
                    ui.add(egui::Label::new(job).truncate());
                });
            });
        });
        // La carte suit sans écart : son contour recouvre le bas de la bande.
        ui.add_space(-ui.spacing().item_spacing.y - t.shapes.border);
        if cancel {
            self.actions.reply = None;
        }
        true
    }

    /// Flèche haut dans une saisie vide : modifier mon dernier message.
    pub(super) fn composer_keys(&mut self, ui: &mut egui::Ui, cid: u64, composer: Id) {
        if !self.composer_text().is_empty() || self.actions.edit.is_some() || !ui.memory(|m| m.has_focus(composer)) {
            return;
        }
        if ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::ArrowUp)) {
            match self.store.last_own_message(cid).map(|m| m.id) {
                Some(id) => self.actions.push(Action::Edit(id)),
                // Aucun message à moi : la flèche choisit le dernier message (lot C5).
                None => self.select_last_message(ui.ctx(), cid),
            }
        }
    }

    /// Applique les gestes de cette image, puis dessine les fenêtres
    /// (confirmation de suppression, transfert, sélecteur de réaction).
    pub(super) fn message_actions(&mut self, ui: &mut egui::Ui, cid: u64) {
        // Autre compte : rien de l'ancien ne reste.
        let me = self.store.me.as_ref().map(|u| u.id);
        if self.actions.me != me {
            // Récents d'un autre compte oubliés (pas à la première image).
            if self.actions.me.is_some() {
                self.emojis.clear();
            }
            self.actions.clear();
            self.actions.me = me;
        }
        // Salon quitté : ce qui le concernait se ferme.
        let a = &mut self.actions;
        if a.reply.as_ref().is_some_and(|r| r.channel_id != cid) {
            a.reply = None;
        }
        if a.edit.as_ref().is_some_and(|e| e.channel_id != cid) {
            a.relayout = a.edit.take().map(|e| e.message_id);
        }
        if a.picker.as_ref().is_some_and(|p| p.channel_id != cid) {
            a.picker = None;
        }
        if let Some((channel_id, message_id, text)) = self.store.take_edit_failure()
            && channel_id == cid
        {
            self.actions.edit = Some(EditDraft { channel_id, message_id, text, focus: true });
            self.actions.relayout = Some(message_id);
        }
        for action in std::mem::take(&mut self.actions.out) {
            self.apply_action(cid, action);
        }
        if let Some(id) = self.actions.relayout.take() {
            self.list.remeasure(cid, id);
            ui.ctx().request_repaint();
        }
        self.delete_dialog(ui);
        self.forward_dialog(ui);
        self.reaction_picker(ui);
    }

    fn apply_action(&mut self, cid: u64, action: Action) {
        match action {
            Action::Mention(id) => {
                if let Some((user, name)) = self.store.find_message(cid, id).map(|m| (m.author_id, m.author.clone())) {
                    self.insert_mention(user, &name);
                }
            }
            Action::Reply(id) => {
                if let Some(m) = self.store.find_message(cid, id) {
                    self.actions.reply = Some(ReplyDraft { channel_id: cid, message_id: id, author: m.author.clone(), mention: true });
                    self.ctx.memory_mut(|mem| mem.request_focus(Id::unique("composer")));
                }
            }
            Action::React(id, emoji) => {
                self.store.toggle_reaction(cid, id, emoji);
            }
            Action::Reactors(id, emoji, burst) => {
                self.store.request_reactors(cid, id, &emoji, burst);
            }
            Action::Picker(id, pos) => {
                self.actions.picker = Some(Picker { channel_id: cid, message_id: id, pos, query: String::new() });
            }
            Action::Edit(id) => {
                if let Some(text) = self.store.raw_content(cid, id) {
                    if let Some(old) = self.actions.edit.take() {
                        self.list.remeasure(cid, old.message_id);
                    }
                    self.actions.edit = Some(EditDraft { channel_id: cid, message_id: id, text, focus: true });
                    self.actions.relayout = Some(id);
                }
            }
            Action::SaveEdit => {
                let Some(e) = self.actions.edit.take() else { return };
                self.actions.relayout = Some(e.message_id);
                let unchanged = self.store.raw_content(cid, e.message_id).is_some_and(|raw| raw.trim() == e.text.trim());
                if e.text.trim().is_empty() {
                    // Comme Discord : un message vidé propose sa suppression.
                    self.actions.confirm_delete = Some((cid, e.message_id));
                } else if !unchanged && !self.store.edit_message(cid, e.message_id, e.text.clone()) {
                    if e.text.chars().count() > crate::state::store::social::MAX_CONTENT {
                        self.notify(crate::i18n::t::MESSAGE_TOO_LONG);
                    }
                    self.actions.edit = Some(EditDraft { focus: true, ..e });
                }
                self.ctx.memory_mut(|mem| mem.request_focus(Id::unique("composer")));
            }
            Action::CancelEdit => {
                if let Some(e) = self.actions.edit.take() {
                    self.actions.relayout = Some(e.message_id);
                }
                self.ctx.memory_mut(|mem| mem.request_focus(Id::unique("composer")));
            }
            Action::Delete { id, confirm } => {
                if confirm {
                    self.actions.confirm_delete = Some((cid, id));
                } else {
                    self.store.delete_message(cid, id);
                }
            }
            Action::Pin(id, pin) => {
                self.store.set_pinned(cid, id, pin);
            }
            Action::Save(id, delay) => {
                self.store.save_message(cid, id, delay);
            }
            Action::Forward(id) => {
                self.actions.forward = Some(ForwardDraft { channel_id: cid, message_id: id, query: String::new() });
            }
            Action::Jump(id) => self.jump_to_message(cid, id),
            Action::OpenThread(id) => {
                if self.store.is_thread(id) {
                    self.open_channel(id);
                }
            }
            Action::CreateThread(id) => {
                if let Some(m) = self.store.find_message(cid, id) {
                    let name: String = crate::state::store::social::excerpt(&m.content).chars().take(crate::state::threads::MAX_NAME).collect();
                    self.threads.start_create(cid, id, name);
                }
            }
            Action::App(a) => self.apply_app_action(cid, a),
            Action::Extra(a) => self.apply_extra(cid, a),
            Action::Select(id) => {
                if !self.store.moderation.toggle(cid, id) {
                    self.notify(crate::i18n::table::moderation::t::SELECTION_FULL);
                }
            }
            Action::MarkUnread(id) => {
                if !self.store.mark_message_unread(cid, id) {
                    self.notify(mx::NO_UNREAD_STATE);
                }
            }
            Action::SuppressEmbeds(id) => {
                self.store.suppress_embeds(cid, id);
            }
            Action::More(id, at) => {
                self.actions.more = Some(More { message_id: id, at, fresh: true });
                self.ctx.request_repaint();
            }
        }
    }

    fn delete_dialog(&mut self, ui: &mut egui::Ui) {
        let Some((cid, id)) = self.actions.confirm_delete else { return };
        let Some(m) = self.store.find_message(cid, id) else {
            self.actions.confirm_delete = None;
            return;
        };
        let t = &self.theme;
        let lang = self.lang;
        let (mut yes, mut no, mut enter) = (false, false, false);
        let width = crate::ui::mobile::card_width(ui.ctx(), t, 1.0);
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            t,
            Id::unique("supprimer-message"),
            width,
            tx::DELETE_MESSAGE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, t);
                ui.label(RichText::new(tx::DELETE_QUESTION.get(lang)).color(t.colors.text));
                t.card_frame().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new(&m.author).family(crate::ui::fonts::demi()).strong().color(t.author_color(m.author_id)));
                    ui.add(egui::Label::new(RichText::new(crate::state::store::social::excerpt(&m.content)).color(t.colors.muted)).wrap());
                });
                ui.label(RichText::new(tx::DELETE_TIP.get(lang)).size(t.typography.small).color(t.colors.faint));
                enter = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, Key::Enter));
            },
            |ui| {
                yes = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Danger, tx::DELETE.get(lang)).clicked();
                no = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked();
            },
        );
        if yes || enter {
            self.store.delete_message(cid, id);
            self.actions.confirm_delete = None;
        } else if no || modal.closed {
            self.actions.confirm_delete = None;
        }
    }

    fn forward_dialog(&mut self, ui: &mut egui::Ui) {
        let Some(draft) = self.actions.forward.as_mut() else { return };
        let t = &self.theme;
        let lang = self.lang;
        let query = draft.query.to_lowercase();
        let mut chosen = None;
        let mut cancel = false;
        let modal = crate::ui::widgets::modal(
            ui.ctx(),
            t,
            Id::unique("transfert"),
            crate::ui::mobile::card_width(ui.ctx(), t, 1.0),
            tx::FORWARD_TITLE.get(lang),
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                crate::ui::settings::shell::page_style(ui, t);
                let search = ui.add(egui::TextEdit::singleline(&mut draft.query).hint_text(tx::SEARCH_CHANNELS.get(lang)).desired_width(f32::INFINITY));
                if draft.query.is_empty() && !search.has_focus() {
                    search.request_focus();
                }
                let targets: Vec<(u64, Option<u64>, String)> = self
                    .store
                    .forward_targets()
                    .into_iter()
                    .filter(|(_, _, n)| query.is_empty() || n.to_lowercase().contains(&query))
                    .take(MAX_FORWARD_TARGETS)
                    .collect();
                egui::ScrollArea::vertical().max_height(t.layout.card_width).show(ui, |ui| {
                    for (id, guild, name) in &targets {
                        let server = guild.and_then(|g| self.store.guilds.iter().find(|x| x.id == g)).map(|g| g.name.as_str());
                        let label = match server {
                            Some(s) => format!("#{name} ({s})"),
                            None => super::words::dm_name(lang, name).to_string(),
                        };
                        if ui.add(egui::Button::new(label).frame(false)).clicked() {
                            chosen = Some(*id);
                        }
                    }
                });
            },
            |ui| cancel = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Link, tx::CANCEL.get(lang)).clicked(),
        );
        let (from, id) = (draft.channel_id, draft.message_id);
        if let Some(to) = chosen {
            let label = tx::FORWARD_LABEL.get(lang).to_string();
            self.store.forward(to, from, id, label);
            self.actions.forward = None;
        } else if cancel || modal.closed {
            self.actions.forward = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::social::MessageMeta;

    fn msg(author_id: u64) -> Message {
        Message {
            id: 5,
            channel_id: 2,
            guild_id: Some(1),
            author_id,
            author: "a".into(),
            author_avatar: crate::state::model::Avatar::default_for(author_id),
            content: "x".into(),
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: MessageMeta::default(),
        }
    }

    #[test]
    fn actions_follow_rights() {
        let r = Rights { me: 7, dm: false, send: true, react: true, react_existing: true, manage: false, pin: false, thread: false };
        let a = allowed(&msg(7), &r);
        assert!(a.reply && a.react && a.edit && a.delete && !a.pin && a.forward);
        let a = allowed(&msg(8), &r);
        assert!(!a.edit && !a.delete);
        let a = allowed(&msg(8), &Rights { manage: true, pin: true, send: false, ..r });
        assert!(a.delete && a.pin && !a.reply);
        let mut sys = msg(7);
        sys.system = Some(crate::state::SystemKind::Joined);
        let a = allowed(&sys, &r);
        assert!(!a.reply && !a.edit && !a.pin && !a.forward && a.delete);
    }

    #[test]
    fn links() {
        let mut m = msg(7);
        assert_eq!(message_link(&m), "https://discord.com/channels/1/2/5");
        m.guild_id = None;
        assert_eq!(message_link(&m), "https://discord.com/channels/@me/2/5");
    }
}
