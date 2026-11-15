//! Carte d'une invitation sous son message (lot T13), et deux collectes
//! faites pendant le dessin de la liste, relevées après elle par
//! `App::lot13_frame` : les codes d'invitation à demander (cartes
//! visibles) et les adresses d'images en échec (rafraîchissement des
//! adresses expirées). L'interface ne fait ainsi ni réseau ni écriture dans
//! le magasin pendant le dessin.

use super::msg_actions::{Acts, Hit};
use super::polls::ExtraHit;
use crate::i18n::table::polls::{p, t as tx};
use crate::i18n::Lang;
use crate::images::Images;
use crate::state::invites::{InviteState, JoinState};
use crate::theme::Tokens;
use eframe::egui::{self, Id, Rect, RichText, Sense, Vec2};

/// Adresses en échec gardées au plus entre deux relevés (le reste attend
/// l'image suivante : la liste redessine les tuiles en échec).
const MAX_NOTED: usize = 64;

fn codes_id() -> Id {
    Id::unique("lot13-invitations-voulues")
}

fn urls_id() -> Id {
    Id::unique("lot13-adresses-en-echec")
}

fn note(ctx: &egui::Context, id: Id, s: &str) {
    ctx.data_mut(|d| {
        let v = d.get_temp_mut_or_default::<Vec<String>>(id);
        if v.len() < MAX_NOTED && !v.iter().any(|x| x == s) {
            v.push(s.to_string());
        }
    });
}

/// Image de pièce jointe en échec, sous cette adresse.
pub(super) fn note_failed_url(ctx: &egui::Context, url: &str) {
    note(ctx, urls_id(), url);
}

/// Relève les deux collectes : (codes, adresses).
pub(super) fn drain(ctx: &egui::Context) -> (Vec<String>, Vec<String>) {
    ctx.data_mut(|d| (d.remove_temp::<Vec<String>>(codes_id()).unwrap_or_default(), d.remove_temp::<Vec<String>>(urls_id()).unwrap_or_default()))
}

/// Carte d'une invitation. Sa hauteur ne dépend pas de son état (chargement,
/// prête, invalide) : la mesure du message reste juste quand elle arrive.
pub(super) fn invite_card(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&Images>, code: &str, acts: &mut Acts) {
    let live = images.is_some();
    let state = acts.store.invites.get(code);
    if live && state.is_none() {
        note(ui.ctx(), codes_id(), code);
    }
    ui.add_space(t.spacing.media_gap);
    let width = ui.available_width().min(t.layout.card_width);
    t.card_frame().show(ui, |ui| {
        ui.set_width(width - t.embed_margin().sum().x);
        let head = match state {
            Some(InviteState::Ready(inv)) if inv.kind == 1 => tx::INVITE_GROUP,
            Some(InviteState::Ready(inv)) if inv.kind == 2 => tx::INVITE_FRIEND,
            _ => tx::INVITE_HEAD,
        };
        ui.label(RichText::new(head.get(lang)).size(t.typography.small).family(crate::ui::fonts::demi()).strong().color(t.colors.faint));
        let side = t.layout.avatar_size;
        ui.horizontal(|ui| {
            let (icon, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
            let (name, line, color) = match state {
                Some(InviteState::Ready(inv)) => {
                    // Icône de serveur : carré arrondi, comme dans le rail.
                    let drawn = images.is_some_and(|im| super::util::paint_image(ui, im, icon, inv.icon, t.r_rail(true)));
                    if !drawn {
                        super::util::paint_initials(ui, t, icon, t.placeholder_avatar(), &inv.name);
                    }
                    let counts = match (inv.online, inv.members) {
                        (Some(o), Some(m)) => format!("{} · {}", p::online(lang, u64::from(o)), p::members(lang, u64::from(m))),
                        (None, Some(m)) => p::members(lang, u64::from(m)),
                        _ => String::new(),
                    };
                    (inv.name.clone(), counts, t.colors.muted)
                }
                Some(InviteState::Invalid) => {
                    ui.painter().rect_filled(icon, t.r_avatar(side), t.colors.bg3);
                    (tx::INVITE_INVALID.get(lang).to_string(), String::new(), t.colors.faint)
                }
                Some(InviteState::Failed(problem)) => {
                    ui.painter().rect_filled(icon, t.r_avatar(side), t.colors.bg3);
                    let h24 = t.layout.clock == crate::theme::Clock::H24;
                    (code.to_string(), super::words::problem(lang, problem, h24), t.colors.danger)
                }
                None | Some(InviteState::Loading) => {
                    ui.painter().rect_filled(icon, t.r_avatar(side), t.colors.bg3);
                    (tx::INVITE_LOADING.get(lang).to_string(), String::new(), t.colors.faint)
                }
            };
            ui.vertical(|ui| {
                ui.add(egui::Label::new(RichText::new(name).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong)).truncate());
                ui.add(egui::Label::new(RichText::new(if line.is_empty() { " ".to_string() } else { line }).size(t.typography.small).color(color)).truncate());
            });
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let Some(InviteState::Ready(inv)) = state else { return };
                if acts.store.invite_joined(inv) {
                    ui.label(RichText::new(tx::JOINED.get(lang)).color(t.colors.muted));
                    return;
                }
                if inv.kind != 0 {
                    return;
                }
                match acts.store.invites.join_state(code) {
                    Some(JoinState::Captcha) => {
                        ui.add(egui::Label::new(RichText::new(tx::INVITE_CAPTCHA.get(lang)).size(t.typography.small).color(t.colors.danger)).wrap());
                    }
                    Some(JoinState::Joining) => {
                        ui.spinner();
                    }
                    None => {
                        let r = button_rect(ui, t, tx::JOIN.get(lang));
                        acts.hits.push((r, Hit::Extra(ExtraHit::Join(code.to_string()))));
                    }
                }
            });
        });
    });
}

/// Bouton « Rejoindre » dessiné (rendu cliquable après le message).
fn button_rect(ui: &mut egui::Ui, t: &Tokens, text: &str) -> Rect {
    // Vert, comme dans le client : rejoindre est l'action positive de la carte.
    let galley = ui.painter().layout_no_wrap(text.to_string(), crate::ui::fonts::medium_font(t.typography.button), t.colors.on_accent);
    let size = Vec2::new(galley.size().x + 2.0 * t.spacing.control_x, crate::ui::widgets::button_height(t));
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    ui.painter().rect_filled(rect, t.r_control(), t.success_fill());
    ui.painter().galley(rect.center() - galley.size() / 2.0, galley, t.colors.on_accent);
    rect
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collections_are_bounded_and_drained_once() {
        let ctx = egui::Context::default();
        for i in 0..(MAX_NOTED + 10) {
            note_failed_url(&ctx, &format!("u{i}"));
        }
        note_failed_url(&ctx, "u0");
        note(&ctx, codes_id(), "ferro");
        let (codes, urls) = drain(&ctx);
        assert_eq!(codes, vec!["ferro".to_string()]);
        assert_eq!(urls.len(), MAX_NOTED);
        assert_eq!(drain(&ctx), (Vec::new(), Vec::new()));
    }
}
