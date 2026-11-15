//! Accessibilité (lot C5) : nom lu par les lecteurs d'écran pour un
//! message, contour du clavier commun à toute l'interface, Échap qui ferme
//! une fenêtre. L'arbre AccessKit est produit par egui (`accesskit` activé
//! dans eframe) ; les tests le lisent dans la sortie du contexte.

use crate::i18n::table::a11y::{f, t as tx};
use crate::i18n::{time, Lang};
use crate::state::markdown::{BlockKind, Doc};
use crate::state::model::Message;
use crate::theme::Tokens;
use eframe::egui::{self, Id, Key, Modifiers};

/// Texte du message tel qu'un lecteur d'écran le dit : spoilers non
/// révélés remplacés par « [spoiler] » (une fois par spoiler).
pub(super) fn spoken(lang: Lang, doc: &Doc) -> String {
    let hidden = tx::MSG_SPOILER.get(lang);
    let mut lines = Vec::new();
    for b in &doc.blocks {
        if let BlockKind::Code { spoiler: Some(_), .. } = b.kind {
            lines.push(hidden.to_string());
            continue;
        }
        let mut line = String::new();
        let mut last = None;
        for s in &b.spans {
            match s.style.spoiler {
                Some(n) if last == Some(n) => {}
                Some(n) => {
                    line.push_str(hidden);
                    last = Some(n);
                }
                None => {
                    line.push_str(&s.text);
                    last = None;
                }
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// Nom d'un message : auteur, heure, texte, puis « modifié », « épinglé »,
/// pièces jointes et réactions.
pub(super) fn message_label(lang: Lang, m: &Message, doc: &Doc, ts: i64, h24: bool) -> String {
    let mut s = f::message(lang, &m.author, time::clock(lang, ts, true, h24), spoken(lang, doc));
    for (on, word) in [(m.edited, tx::MSG_EDITED), (m.meta.pinned, tx::MSG_PINNED)] {
        if on {
            s.push_str(" (");
            s.push_str(word.get(lang));
            s.push(')');
        }
    }
    if !m.attachments.is_empty() {
        let names: Vec<&str> = m.attachments.iter().map(|a| a.name.as_str()).collect();
        s.push_str(". ");
        s.push_str(&f::attachments(lang, names.join(", ")));
    }
    if !m.meta.reactions.is_empty() {
        let list: Vec<String> = m.meta.reactions.iter().map(|r| f::reaction(lang, r.emoji.display(), crate::i18n::number(lang, u64::from(r.normal) + u64::from(r.burst)))).collect();
        s.push_str(". ");
        s.push_str(&f::reactions(lang, list.join(", ")));
    }
    s
}

/// Donne un nom à l'élément `id` s'il n'en a pas (champ de saisie nommé par
/// son invite, que les lecteurs d'écran ne lisent pas toujours).
pub(super) fn name(ctx: &egui::Context, id: Id, label: &str) {
    ctx.accesskit_node_builder(id, |n| {
        if n.label().is_none_or(str::is_empty) {
            n.set_label(label);
        }
    });
}

/// Échap pressé (et pris : une seule fenêtre se ferme par appui).
pub(super) fn escape(ctx: &egui::Context) -> bool {
    ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
}

fn keyboard_id() -> Id {
    Id::unique("ferrocord.a11y.clavier")
}

/// Contour de l'élément qui a le clavier, avec les jetons `focus` du thème,
/// pour tout widget (ceux d'egui comme les nôtres). Montré quand le clavier
/// sert à naviguer (Tab, flèches), retiré au premier clic, comme Discord.
pub(super) fn focus_ring(ctx: &egui::Context, t: &Tokens) {
    let (keys, click) = ctx.input(|i| {
        let keys = i.events.iter().any(|e| matches!(e, egui::Event::Key { key: Key::Tab | Key::ArrowUp | Key::ArrowDown | Key::ArrowLeft | Key::ArrowRight, pressed: true, .. }));
        (keys, i.pointer.any_pressed())
    });
    let mut on = ctx.data(|d| d.get_temp::<bool>(keyboard_id())).unwrap_or(false);
    if keys || click {
        on = keys && !click;
        ctx.data_mut(|d| d.insert_temp(keyboard_id(), on));
    }
    if !on {
        return;
    }
    let Some(id) = ctx.memory(|m| m.focused()) else { return };
    let Some(resp) = ctx.read_response(id) else { return };
    let stroke = egui::Stroke::new(t.shapes.focus, t.colors.focus);
    ctx.layer_painter(resp.layer_id).with_clip_rect(resp.interact_rect.expand(t.shapes.focus)).rect_stroke(resp.rect, t.r_row(), stroke, egui::StrokeKind::Inside);
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::state::social::{Emoji, Reaction};
    use eframe::egui::accesskit::{Node, Role};

    /// Nœuds de l'arbre AccessKit d'une image.
    pub(in crate::ui) fn nodes(out: &egui::FullOutput) -> Vec<Node> {
        out.platform_output.accesskit_update.iter().flat_map(|u| u.nodes.iter().map(|(_, n)| n.clone())).collect()
    }

    /// Nœuds qu'on peut atteindre au clavier ou activer, sans nom lisible :
    /// rôle et rectangle (pour retrouver l'élément).
    pub(in crate::ui) fn unnamed(out: &egui::FullOutput) -> Vec<String> {
        nodes(out)
            .iter()
            .filter(|n| matches!(n.role(), Role::Button | Role::CheckBox | Role::RadioButton | Role::Link | Role::TextInput | Role::MultilineTextInput | Role::ComboBox | Role::Slider | Role::Tab | Role::ListItem | Role::Article))
            .filter(|n| n.label().is_none_or(|l| l.trim().is_empty()) && n.value().is_none_or(|v| v.trim().is_empty()) && n.labelled_by().is_empty())
            .map(|n| format!("{:?} {:?}", n.role(), n.bounds()))
            .collect()
    }

    /// Libellés de rôle `role` présents dans l'image.
    pub(in crate::ui) fn labels(out: &egui::FullOutput, role: Role) -> Vec<String> {
        nodes(out).iter().filter(|n| n.role() == role).filter_map(|n| n.label().map(str::to_string)).collect()
    }

    /// Application principale avec deux messages, salon 10 du serveur 9 ouvert.
    fn main_app() -> crate::ui::App {
        use crate::ui::keys::tests::{app, test_message};
        let mut a = app();
        let mut messages = Vec::new();
        for (n, author) in [(1u64, 3u64), (2, 1)] {
            let mut m = Message { channel_id: 10, guild_id: Some(9), author_id: author, content: format!("bonjour {n}"), ..test_message(n << 22) };
            m.meta.reactions.push(Reaction { emoji: Emoji::Unicode("🔥".into()), normal: 1, burst: 0, me: false, me_burst: false });
            messages.push(m);
        }
        a.store.apply(crate::state::Event::History { channel_id: 10, before: None, messages });
        a
    }

    fn settle(a: &mut crate::ui::App) -> egui::FullOutput {
        let mut out = crate::ui::tests::frame(a, Vec::new());
        for _ in 0..3 {
            out = crate::ui::tests::frame(a, Vec::new());
        }
        out
    }

    /// Écran principal : serveurs, salons, messages (auteur, heure, texte,
    /// réactions) ont un rôle et un nom ; aucun bouton à icône sans nom.
    #[test]
    fn main_screen_is_named_for_screen_readers() {
        let mut a = main_app();
        let out = settle(&mut a);
        assert_eq!(unnamed(&out), Vec::<String>::new(), "éléments sans nom");
        let buttons = labels(&out, Role::Button);
        assert!(buttons.iter().any(|l| l.contains("Alpha")), "serveur nommé : {buttons:?}");
        assert!(buttons.iter().any(|l| l.contains("général")), "salon nommé : {buttons:?}");
        let msgs = labels(&out, Role::Article);
        assert_eq!(msgs.len(), 2, "{msgs:?}");
        assert!(msgs.iter().any(|m| m.starts_with("Bob, ") && m.contains("bonjour 1") && m.contains("Réactions\u{a0}: 🔥 1")), "{msgs:?}");
    }

    /// Fenêtres : chacune est nommée, s'atteint au clavier, et Échap la ferme.
    #[test]
    fn windows_are_named_and_close_on_escape() {
        use crate::ui::keys::tests::key;
        let esc = || key(Key::Escape, Modifiers::NONE);
        let mut a = main_app();
        settle(&mut a);
        type Open = fn(&mut crate::ui::App);
        type Shown = fn(&crate::ui::App) -> bool;
        let cases: [(&str, Open, Shown); 7] = [
            ("suppression", |a| a.actions.confirm_delete = Some((10, 2 << 22)), |a| a.actions.confirm_delete.is_some()),
            ("tableau de sons", |a| a.show_soundboard(10), |a| a.calls_ui.soundboard.is_some()),
            ("raccourcis", |a| a.open_shortcuts(), |a| a.nav.shortcuts_open),
            ("paramètres", |a| a.show_settings(crate::ui::settings::Page::Appearance), |a| a.params.open),
            ("sélecteur rapide", |a| a.open_switcher(), |a| a.nav.switcher.open),
            ("boîte de réception", |a| a.toggle_inbox(), |a| a.nav.inbox.open),
            ("épingles", |a| a.actions.pins_open = true, |a| a.actions.pins_open),
        ];
        for (name, open, shown) in cases {
            open(&mut a);
            let out = settle(&mut a);
            assert!(shown(&a), "{name} ouvert");
            assert_eq!(unnamed(&out), Vec::<String>::new(), "{name} : éléments sans nom");
            // Le clavier entre dans la fenêtre (Tab), puis Échap la ferme.
            crate::ui::tests::frame(&mut a, vec![key(Key::Tab, Modifiers::NONE)]);
            assert!(a.ctx.memory(|m| m.focused()).is_some(), "{name} : Tab donne le clavier à un élément");
            for _ in 0..2 {
                if shown(&a) {
                    crate::ui::tests::frame(&mut a, vec![esc()]);
                }
            }
            assert!(!shown(&a), "{name} : Échap ferme");
        }
    }

    /// Vue d'appel : tuiles et commandes nommées (personne, caméra, partage).
    #[test]
    fn call_view_is_named() {
        use crate::state::voice::{VoiceEvent, VoiceState};
        let mut a = main_app();
        let joined = |user_id: u64, self_video: bool| crate::state::Event::Voice(VoiceEvent::StateUpdate { guild_id: None, user_id, state: Some(VoiceState { user_id, channel_id: 50, self_video, ..Default::default() }), session_id: None });
        a.store.apply(joined(1, false));
        a.store.apply(joined(7, true));
        a.show_call(None, 50);
        let out = settle(&mut a);
        assert_eq!(unnamed(&out), Vec::<String>::new(), "vue d'appel : éléments sans nom");
        assert!(labels(&out, Role::Button).len() >= 3, "{:?}", labels(&out, Role::Button));
    }

    /// Le contour du clavier n'apparaît qu'en navigation au clavier, aux
    /// couleurs du jeton `focus`.
    #[test]
    fn focus_ring_follows_keyboard_and_theme() {
        use crate::ui::keys::tests::key;
        let mut a = main_app();
        settle(&mut a);
        let (focus, width) = (a.theme.colors.focus, a.theme.shapes.focus);
        let ringed = |out: &egui::FullOutput| {
            out.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Rect(r) if r.stroke.color == focus && r.stroke.width == width))
        };
        let out = crate::ui::tests::frame(&mut a, vec![key(Key::Tab, Modifiers::NONE)]);
        let _ = out;
        let out = crate::ui::tests::frame(&mut a, Vec::new());
        assert!(a.ctx.memory(|m| m.focused()).is_some());
        assert!(ringed(&out), "contour après Tab");
        let at = egui::pos2(500.0, 300.0);
        crate::ui::tests::frame(&mut a, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Modifiers::NONE }]);
        let out = crate::ui::tests::frame(&mut a, vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Modifiers::NONE }]);
        assert!(!ringed(&out), "pas de contour à la souris");
    }

    #[test]
    fn message_label_reads_everything_but_spoilers() {
        use crate::state::markdown::{parse, Resolve, TimeStyle};
        struct R;
        impl Resolve for R {
            fn user(&self, _: u64) -> String {
                String::new()
            }
            fn role(&self, _: u64) -> (String, u32) {
                (String::new(), 0)
            }
            fn channel(&self, _: u64) -> String {
                String::new()
            }
            fn time(&self, _: i64, _: TimeStyle) -> Option<String> {
                None
            }
        }
        let doc = parse("salut ||secret|| toi", &R);
        let mut m = crate::state::model::Message { author: "Alice".into(), edited: true, ..crate::ui::keys::tests::test_message(1 << 22) };
        m.attachments.push(crate::state::model::Attachment { name: "photo.png".into(), ..Default::default() });
        m.meta.reactions.push(Reaction { emoji: Emoji::Unicode("👍".into()), normal: 2, burst: 1, me: false, me_burst: false });
        let s = message_label(Lang::Fr, &m, &doc, 0, true);
        assert!(s.starts_with("Alice, "), "{s}");
        assert!(s.contains("salut [spoiler] toi") && !s.contains("secret"), "{s}");
        assert!(s.contains("(modifié)") && s.contains("Pièces jointes\u{a0}: photo.png") && s.contains("Réactions\u{a0}: 👍 3"), "{s}");
    }
}
