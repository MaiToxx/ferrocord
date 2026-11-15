//! Sélection des messages au clavier (lot C5), comme Discord : flèche haut
//! depuis une saisie vide (sans message à soi à modifier) ou une fois la
//! saisie quittée par Échap, puis haut et bas de message en message. Sur le
//! message choisi, les touches de `keys.toml` (`message_*`) répondent,
//! modifient, réagissent, épinglent, copient ou suppriment (avec
//! confirmation). Échap rend la saisie.
//!
//! Le message choisi a aussi le clavier d'egui : un lecteur d'écran lit son
//! nom (`a11y::message_label`) à chaque déplacement.

use super::msg_actions::{allowed, Action as MsgAction};
use super::App;
use crate::i18n::Lang;
use crate::state::keys::{Action, Chord};
use crate::state::markdown::Doc;
use crate::state::model::Message;
use crate::theme::{Clock, Tokens};
use eframe::egui::{self, Id, Key, Modifiers};

fn sel_id() -> Id {
    Id::unique("ferrocord.selection_message")
}

/// Identifiant egui de la ligne d'un message (celui de `message.rs`).
pub(super) fn row_id(id: u64) -> Id {
    Id::unique(("msg", id))
}

/// Message choisi : (salon, message).
pub(super) fn selected(ctx: &egui::Context) -> Option<(u64, u64)> {
    ctx.data(|d| d.get_temp::<Option<(u64, u64)>>(sel_id())).flatten()
}

fn set(ctx: &egui::Context, sel: Option<(u64, u64)>) {
    ctx.data_mut(|d| d.insert_temp(sel_id(), sel));
    if let Some((_, id)) = sel {
        // La flèche qui a servi ici ne déplace pas en plus le clavier d'egui
        // vers le widget voisin (déplacement calculé en fin d'image).
        ctx.memory_mut(|m| {
            m.request_focus(row_id(id));
            m.move_focus(egui::FocusDirection::None);
        });
    }
    ctx.request_repaint();
}

/// Ligne d'un message dessiné : nom pour les lecteurs d'écran, et
/// sélection (prise quand Tab amène le clavier dessus ; contour du jeton
/// `focus`). `true` si le message est choisi.
#[allow(clippy::too_many_arguments)]
pub(super) fn row(ui: &egui::Ui, t: &Tokens, lang: Lang, m: &Message, doc: &Doc, ts: i64, resp: &egui::Response, selectable: bool) -> bool {
    let h24 = t.layout.clock == Clock::H24;
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Article, true, super::a11y::message_label(lang, m, doc, ts, h24)));
    if !selectable {
        return false;
    }
    let ctx = ui.ctx();
    if resp.gained_focus() && selected(ctx) != Some((m.channel_id, m.id)) {
        ctx.data_mut(|d| d.insert_temp(sel_id(), Some((m.channel_id, m.id))));
    }
    let on = selected(ctx) == Some((m.channel_id, m.id));
    if on {
        // Haut et bas passent d'un message à l'autre (pas au widget voisin).
        if resp.has_focus() {
            ctx.memory_mut(|mm| mm.set_focus_lock_filter(resp.id, egui::EventFilter { vertical_arrows: true, ..Default::default() }));
        }
        ui.painter().rect_stroke(resp.rect, t.r_hover(), egui::Stroke::new(t.shapes.focus, t.colors.focus), egui::StrokeKind::Inside);
    }
    on
}

/// La combinaison pressée correspond-elle à celle de l'action ? « + » est
/// accepté avec ou sans Maj (Maj+= sur les claviers AZERTY et QWERTY).
fn matches(bound: &Chord, pressed: &Chord) -> bool {
    bound == pressed || (bound.key == "Plus" && pressed.key == "Plus" && bound.ctrl == pressed.ctrl && bound.alt == pressed.alt)
}

impl App {
    /// Choisit le dernier message du salon affiché, comme la flèche haut
    /// après Échap (aperçu).
    pub fn select_last(&mut self) {
        if let Some(cid) = self.store.channel {
            let ctx = self.ctx.clone();
            self.select_last_message(&ctx, cid);
        }
    }

    /// Flèche haut sans message à soi dans la saisie vide : le dernier message est choisi.
    pub(super) fn select_last_message(&mut self, ctx: &egui::Context, cid: u64) {
        let last = self.store.chans.get(&cid).and_then(|cs| cs.msgs.back()).map(|m| m.id);
        if let Some(id) = last {
            set(ctx, Some((cid, id)));
            self.list.reveal(cid, id);
        }
    }

    /// Touches de la sélection, avant le dessin de la liste du salon `cid`.
    pub(super) fn message_keys(&mut self, ui: &mut egui::Ui, cid: u64) {
        let ctx = ui.ctx().clone();
        let mut sel = selected(&ctx).filter(|(c, id)| *c == cid && self.store.find_message(cid, *id).is_some()).map(|(_, id)| id);
        // Clic, ou clavier passé à un autre élément (Tab, saisie) : la sélection s'arrête.
        let focused = ctx.memory(|m| m.focused());
        if ctx.input(|i| i.pointer.any_pressed()) || sel.is_some_and(|id| focused.is_some_and(|f| f != row_id(id))) {
            sel = None;
        }
        // Une fenêtre au-dessus de la liste garde ses touches.
        let covered = self.params.open || self.nav.switcher.open || self.nav.shortcuts_open || self.nav.inbox.open || self.actions.picker.is_some() || self.actions.confirm_delete.is_some();
        if covered {
            if selected(&ctx).is_some() != sel.is_some() {
                ctx.data_mut(|d| d.insert_temp(sel_id(), sel.map(|id| (cid, id))));
            }
            return;
        }
        let Some(id) = sel else {
            if selected(&ctx).is_some() {
                ctx.data_mut(|d| d.insert_temp(sel_id(), None::<(u64, u64)>));
            }
            // Rien n'a le clavier (Échap a quitté la saisie) : haut choisit le dernier message.
            if focused.is_none() && !ctx.text_edit_focused() && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::ArrowUp)) {
                self.select_last_message(&ctx, cid);
            }
            return;
        };
        let (up, down, escape) = ui.input_mut(|i| (i.consume_key(Modifiers::NONE, Key::ArrowUp), i.consume_key(Modifiers::NONE, Key::ArrowDown), i.consume_key(Modifiers::NONE, Key::Escape)));
        if escape {
            self.leave_selection(&ctx);
            return;
        }
        // Voisins du message choisi, lus seulement quand une flèche est pressée.
        let neighbours = |s: &crate::state::Store| {
            let msgs = &s.chans.get(&cid)?.msgs;
            let at = msgs.iter().position(|m| m.id == id)?;
            Some((at.checked_sub(1).and_then(|i| msgs.get(i)).map(|m| m.id), msgs.get(at + 1).map(|m| m.id)))
        };
        if up {
            let Some((prev, _)) = neighbours(&self.store) else { return };
            match prev {
                Some(prev) => self.move_selection(&ctx, cid, prev),
                // Au premier message chargé : la page d'avant est demandée.
                None => {
                    self.store.load_older(cid);
                    set(&ctx, Some((cid, id)));
                }
            }
            return;
        }
        if down {
            let Some((_, next)) = neighbours(&self.store) else { return };
            match next {
                Some(next) => self.move_selection(&ctx, cid, next),
                // Après le dernier : retour à la saisie, comme Discord.
                None => self.leave_selection(&ctx),
            }
            return;
        }
        // Gestes sur le message choisi (`keys.toml`).
        let b = &self.nav.bindings;
        let copy_chord = b.get(Action::MsgCopy).cloned();
        let mut chosen = None;
        ui.input_mut(|i| {
            i.events.retain(|e| {
                if chosen.is_some() {
                    return true;
                }
                // Ctrl+C arrive comme « copier », pas comme une touche.
                if matches!(e, egui::Event::Copy) && copy_chord.as_ref().is_some_and(|c| c.ctrl && !c.alt && !c.shift && c.key == "C") {
                    chosen = Some(Action::MsgCopy);
                    return false;
                }
                let egui::Event::Key { key, pressed: true, repeat: false, modifiers, .. } = e else { return true };
                let pressed = super::keys::chord_of(*key, *modifiers);
                let hit = Action::ALL.into_iter().filter(|a| a.on_message()).find(|a| b.get(*a).is_some_and(|c| matches(c, &pressed)));
                if hit.is_some() {
                    chosen = hit;
                    return false;
                }
                true
            });
        });
        if let Some(a) = chosen {
            self.message_gesture(&ctx, cid, id, a);
        }
    }

    fn move_selection(&mut self, ctx: &egui::Context, cid: u64, id: u64) {
        set(ctx, Some((cid, id)));
        self.list.reveal(cid, id);
    }

    /// Fin de la sélection : le clavier revient à la saisie.
    fn leave_selection(&mut self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(sel_id(), None::<(u64, u64)>));
        ctx.memory_mut(|m| m.request_focus(Id::unique("composer")));
        ctx.request_repaint();
    }

    fn message_gesture(&mut self, ctx: &egui::Context, cid: u64, id: u64, a: Action) {
        let Some(m) = self.store.find_message(cid, id) else { return };
        let can = allowed(m, &self.store.rights(cid));
        let pinned = m.meta.pinned;
        let gesture = match a {
            Action::MsgReply if can.reply => Some(MsgAction::Reply(id)),
            Action::MsgEdit if can.edit => Some(MsgAction::Edit(id)),
            Action::MsgReact if can.react => {
                let pos = ctx.read_response(row_id(id)).map_or_else(|| ctx.content_rect().center(), |r| r.rect.left_bottom());
                Some(MsgAction::Picker(id, pos))
            }
            Action::MsgPin if can.pin => Some(MsgAction::Pin(id, !pinned)),
            // Toujours avec la question « Supprimer le message ? ».
            Action::MsgDelete if can.delete => Some(MsgAction::Delete { id, confirm: true }),
            Action::MsgCopy => {
                let text = self.store.raw_content(cid, id).unwrap_or_else(|| m.content.clone());
                ctx.copy_text(text);
                self.notify(crate::i18n::table::a11y::t::MSG_COPIED);
                None
            }
            _ => None,
        };
        let Some(g) = gesture else { return };
        // Réponse et modification prennent le clavier ; réaction et suppression ouvrent une fenêtre.
        let keep = matches!(g, MsgAction::Pin(..));
        self.actions.push(g);
        if !keep {
            ctx.data_mut(|d| d.insert_temp(sel_id(), None::<(u64, u64)>));
        }
        ctx.request_repaint();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::keys::tests::{app, frame, key, test_message};

    fn with_messages() -> App {
        let mut a = app();
        let messages = [(1u64, 3u64), (2, 1), (3, 3)].into_iter().map(|(n, author)| Message { channel_id: 10, guild_id: Some(9), author_id: author, content: format!("message {n}"), ..test_message(n << 22) }).collect();
        a.store.apply(crate::state::Event::History { channel_id: 10, before: None, messages });
        for _ in 0..3 {
            frame(&mut a, Vec::new());
        }
        a
    }

    /// Touche pressée puis relâchée (egui prend une touche restée enfoncée pour une répétition).
    fn tap(k: Key, m: Modifiers) -> Vec<egui::Event> {
        vec![key(k, m), egui::Event::Key { key: k, physical_key: None, pressed: false, repeat: false, modifiers: m }]
    }

    fn sel(a: &App) -> Option<u64> {
        selected(&a.ctx).map(|(_, id)| id)
    }

    /// Après Échap (rien n'a le clavier), haut et bas passent de message en
    /// message ; Échap rend la saisie ; bas après le dernier aussi.
    #[test]
    fn arrows_walk_messages_after_escape() {
        let mut a = with_messages();
        a.ctx.memory_mut(|m| m.surrender_focus(Id::unique("composer")));
        frame(&mut a, tap(Key::Escape, Modifiers::NONE));
        frame(&mut a, tap(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(sel(&a), Some(3 << 22), "dernier message");
        frame(&mut a, Vec::new());
        assert_eq!(a.ctx.memory(|m| m.focused()), Some(row_id(3 << 22)), "le lecteur d'écran suit");
        frame(&mut a, tap(Key::ArrowUp, Modifiers::NONE));
        frame(&mut a, Vec::new());
        assert_eq!(sel(&a), Some(2 << 22));
        frame(&mut a, tap(Key::ArrowDown, Modifiers::NONE));
        frame(&mut a, Vec::new());
        assert_eq!(sel(&a), Some(3 << 22));
        frame(&mut a, tap(Key::ArrowDown, Modifiers::NONE));
        frame(&mut a, Vec::new());
        assert_eq!(sel(&a), None, "après le dernier : la saisie");
        assert_eq!(a.ctx.memory(|m| m.focused()), Some(Id::unique("composer")));
        frame(&mut a, tap(Key::Escape, Modifiers::NONE));
        frame(&mut a, tap(Key::ArrowUp, Modifiers::NONE));
        frame(&mut a, Vec::new());
        frame(&mut a, tap(Key::Escape, Modifiers::NONE));
        frame(&mut a, Vec::new());
        assert_eq!(sel(&a), None, "Échap arrête la sélection");
    }

    /// R répond, E modifie (le mien seulement), Retour arrière demande
    /// confirmation, P épingle si permis ; une touche réassignée dans
    /// `keys.toml` remplace la combinaison par défaut.
    #[test]
    fn gestures_on_the_selected_message() {
        let mut a = with_messages();
        let pick = |a: &mut App, id: u64| {
            set(&a.ctx.clone(), Some((10, id)));
            frame(a, Vec::new());
            frame(a, Vec::new());
        };
        pick(&mut a, 3 << 22);
        frame(&mut a, tap(Key::R, Modifiers::NONE));
        assert_eq!(a.actions.reply.as_ref().map(|r| r.message_id), Some(3 << 22));
        assert_eq!(sel(&a), None, "la réponse prend la saisie");
        a.actions.reply = None;
        pick(&mut a, 3 << 22);
        frame(&mut a, tap(Key::E, Modifiers::NONE));
        assert!(a.actions.edit.is_none(), "message d'un autre : pas de modification");
        pick(&mut a, 2 << 22);
        frame(&mut a, tap(Key::E, Modifiers::NONE));
        assert_eq!(a.actions.edit.as_ref().map(|e| e.message_id), Some(2 << 22));
        a.actions.edit = None;
        pick(&mut a, 2 << 22);
        frame(&mut a, tap(Key::Backspace, Modifiers::NONE));
        assert_eq!(a.actions.confirm_delete, Some((10, 2 << 22)), "suppression : la question d'abord");
        assert!(a.store.find_message(10, 2 << 22).is_some(), "rien de supprimé");
        a.actions.confirm_delete = None;
        // Réassignée : « Suppr » au lieu de Retour arrière.
        a.set_shortcut(Action::MsgDelete, Chord::parse("Delete"));
        pick(&mut a, 2 << 22);
        frame(&mut a, tap(Key::Backspace, Modifiers::NONE));
        assert_eq!(a.actions.confirm_delete, None);
        frame(&mut a, tap(Key::Delete, Modifiers::NONE));
        assert_eq!(a.actions.confirm_delete, Some((10, 2 << 22)));
        a.actions.confirm_delete = None;
        // Copier : Ctrl+C arrive comme l'événement « copier ».
        pick(&mut a, 1 << 22);
        frame(&mut a, vec![egui::Event::Copy]);
        assert_eq!(sel(&a), Some(1 << 22), "la copie garde la sélection");
        // « + » ouvre le sélecteur de réaction (avec ou sans Maj).
        let shift = Modifiers { shift: true, ..Modifiers::NONE };
        frame(&mut a, tap(Key::Plus, shift));
        assert!(a.actions.picker.as_ref().is_some_and(|p| p.message_id == 1 << 22));
    }

    /// Sans message à moi, haut dans la saisie vide choisit le dernier message.
    #[test]
    fn up_in_empty_composer_without_own_message_selects() {
        let mut a = app();
        let m = Message { channel_id: 10, guild_id: Some(9), author_id: 3, ..test_message(5 << 22) };
        a.store.apply(crate::state::Event::History { channel_id: 10, before: None, messages: vec![m] });
        frame(&mut a, Vec::new());
        a.ctx.memory_mut(|m| m.request_focus(Id::unique("composer")));
        frame(&mut a, Vec::new());
        frame(&mut a, tap(Key::ArrowUp, Modifiers::NONE));
        assert_eq!(sel(&a), Some(5 << 22));
    }
}
