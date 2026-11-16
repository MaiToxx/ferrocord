//! Carte d'une notification du compte système de Discord (embed
//! `safety_system_notification`, lu par [`Embed::safety_notice`]) : porte-voix
//! et titre en gras, jour de l'incident, texte en gris, puis un pied plus
//! clair avec « En savoir plus ». Aucun champ technique n'est montré.
//!
//! Le lien du pied ne devient le bouton « En savoir plus » que s'il mène, en
//! https, à un domaine de Discord ; tout autre lien web est écrit en entier,
//! comme un lien ordinaire, et passe par la confirmation des liens externes.
//! Une adresse d'un autre genre n'est ni montrée ni ouverte.
//!
//! Même place à la mesure (`live` faux) et au dessin ; rien n'est animé.
//!
//! [`Embed::safety_notice`]: crate::state::model::Embed::safety_notice

use super::attachments::{Action, Hot};
use crate::i18n::{time, Lang};
use crate::state::model::SafetyNotice;
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Rect, RichText, Sense, Vec2};

/// Ce que le pied de la carte propose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Foot<'a> {
    /// Domaine de Discord en https : « En savoir plus ».
    LearnMore(&'a str),
    /// Autre site : l'adresse écrite, ouverte après confirmation.
    Plain(&'a str),
    None,
}

pub(super) fn foot<'a>(n: &SafetyNotice<'a>) -> Foot<'a> {
    match n.link() {
        Some(url) if super::links::own_https(url) => Foot::LearnMore(url),
        Some(url) if super::links::host_of(url).is_some() => Foot::Plain(url),
        _ => Foot::None,
    }
}

/// Dessine la carte de `n`. `now_ms` : l'instant présent, pour « Aujourd'hui ».
pub(super) fn card<'a>(ui: &mut egui::Ui, t: &Tokens, lang: Lang, live: bool, n: &SafetyNotice<'a>, now_ms: i64, hot: &mut Hot<'a>) {
    ui.add_space(t.spacing.media_gap);
    let c = &t.colors;
    let width = ui.available_width();
    let pad = egui::Margin { left: 16, right: 16, top: 12, bottom: 16 };
    let foot_pad = egui::Margin::symmetric(10, 8);
    let foot = foot(n);
    // Les fonds passent sous le texte : leur place dans la liste des formes est prise avant lui.
    let under = live.then(|| ui.painter().add(egui::Shape::Noop));
    let out = egui::Frame::new().show(ui, |ui| {
        ui.set_width(width);
        ui.spacing_mut().item_spacing.y = 0.0;
        egui::Frame::new().inner_margin(pad).show(ui, |ui| {
            ui.set_width((width - pad.sum().x).max(1.0));
            if !n.header.is_empty() {
                ui.horizontal_top(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let side = Tokens::icon_side(t.typography.body);
                    let (slot, _) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
                    icons::paint(Icon::Megaphone, ui.painter(), slot, c.accent);
                    ui.add(egui::Label::new(RichText::new(n.header).family(crate::ui::fonts::demi()).size(t.typography.body).color(c.text_strong)).wrap());
                });
            }
            if let Some(ms) = n.timestamp_ms {
                ui.add_space(4.0);
                ui.label(RichText::new(time::day_or_date(lang, ms, now_ms)).family(crate::ui::fonts::demi()).size(t.typography.small).color(c.text));
            }
            if !n.body.is_empty() {
                ui.add_space(12.0);
                ui.add(egui::Label::new(RichText::new(n.body).size(t.typography.body).color(c.muted)).wrap());
            }
        });
        let (label, url) = match foot {
            Foot::LearnMore(url) => (crate::i18n::table::nd::t::LEARN_MORE.get(lang), url),
            Foot::Plain(url) => (url, url),
            Foot::None => return None,
        };
        // Pied : aplat plus clair sur toute la largeur, lien à gauche.
        let strip = egui::Frame::new().inner_margin(foot_pad).show(ui, |ui| {
            ui.set_width((width - foot_pad.sum().x).max(1.0));
            let resp = ui.add(egui::Label::new(RichText::new(label).size(t.typography.body).color(c.link)).wrap());
            if live {
                hot.add(resp.rect, Action::Open(url), Some(url));
            }
        });
        Some(strip.response.rect)
    });
    let Some(under) = under else { return };
    let card = out.response.rect;
    if !ui.is_rect_visible(card) {
        return;
    }
    let r = t.r_embed();
    let mut shapes = vec![egui::Shape::from(egui::epaint::RectShape::filled(card, r, t.card_fill()))];
    if let Some(strip) = out.inner {
        // Le pied épouse les coins du bas de la carte.
        let low = egui::CornerRadius { nw: 0, ne: 0, sw: r.sw, se: r.se };
        shapes.push(egui::epaint::RectShape::filled(Rect::from_min_max(strip.min, card.max), low, foot_fill(t)).into());
    }
    ui.painter().set(under, egui::Shape::Vec(shapes));
}

/// Aplat du pied : un cran plus clair que la carte.
fn foot_fill(t: &Tokens) -> egui::Color32 {
    super::motion::mix(t.card_fill(), t.colors.text, 0.08)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::model::{Embed, EmbedField, EmbedKind};

    fn embed(link: Option<&str>) -> Embed {
        let mut fields = vec![("header", "Titre fictif"), ("body", "Texte fictif de la notification."), ("timestamp", "1794767947.5"), ("icon_type", "default"), ("theme", "default"), ("classification_id", "1500000000000000001"), ("client_version_message", "Mets l'application à jour.")];
        if let Some(l) = link {
            fields.push(("ctas", "learn_more_link"));
            fields.push(("learn_more_link", l));
        }
        Embed { kind: EmbedKind::SafetyNotification, title: Some("Titre technique".into()), fields: fields.into_iter().map(|(n, v)| EmbedField { name: n.into(), value: v.into(), inline: false }).collect(), ..Embed::default() }
    }

    /// Textes peints et hauteur prise, sur 700 points.
    fn pass(e: &Embed, live: bool, now_ms: i64) -> (String, f32, usize) {
        let ctx = egui::Context::default();
        let (mut text, mut h, mut zones) = (String::new(), 0.0, 0);
        for frame in 0..3 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 2000.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let mut hot = Hot::default();
                let n = e.safety_notice().unwrap_or_else(|| panic!("notification"));
                h = ui.scope(|ui| card(ui, &Tokens::default(), Lang::Fr, live, &n, now_ms, &mut hot)).response.rect.height();
                zones = hot.rects().len();
            });
            out.textures_delta.clear();
            text.clear();
            fn walk(s: &egui::Shape, out: &mut String) {
                match s {
                    egui::Shape::Text(t) => {
                        out.push_str(t.galley.text());
                        out.push('\n');
                    }
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            out.shapes.iter().for_each(|s| walk(&s.shape, &mut text));
            let idle = out.viewport_output.values().all(|v| v.repaint_delay == std::time::Duration::MAX);
            assert!(frame < 2 || idle, "carte au repos : aucun redessin");
        }
        (text, h, zones)
    }

    const NOW: i64 = 1_794_767_947_500;

    #[test]
    fn card_shows_header_day_body_and_learn_more_only() {
        let e = embed(Some("https://support.discord.com/hc/articles/1"));
        let (text, h, zones) = pass(&e, true, NOW);
        for want in ["Titre fictif", "Aujourd'hui", "Texte fictif de la notification.", "En savoir plus"] {
            assert!(text.contains(want), "« {want} » absent : {text}");
        }
        for hidden in ["header", "body", "icon_type", "default", "theme", "ctas", "learn_more_link", "classification_id", "1500000000000000001", "client_version_message", "Mets l'application", "Titre technique", "support.discord.com", "1794767947"] {
            assert!(!text.contains(hidden), "« {hidden} » montré : {text}");
        }
        assert_eq!(zones, 1, "une seule zone cliquable : le lien du pied");
        assert_eq!(pass(&e, false, NOW).1, h, "même hauteur à la mesure");
        assert_eq!(pass(&e, false, NOW).2, 0, "mesure : rien de cliquable");
        // Un autre jour : la date, à mois abrégé.
        let later = pass(&e, true, NOW + 40 * 86_400_000).0;
        assert!(!later.contains("Aujourd'hui") && later.contains("2026"), "{later}");
    }

    #[test]
    fn foot_follows_the_link() {
        /// Genre du pied : 'b' bouton « En savoir plus », 'l' lien écrit en entier, '-' rien.
        fn foot_of(l: Option<&str>) -> char {
            let e = embed(l);
            match foot(&e.safety_notice().unwrap_or_else(|| panic!("notification"))) {
                Foot::LearnMore(u) if Some(u) == l => 'b',
                Foot::Plain(u) if Some(u) == l => 'l',
                Foot::None => '-',
                other => panic!("{other:?}"),
            }
        }
        assert_eq!(foot_of(Some("https://support.discord.com/hc/articles/1")), 'b');
        assert_eq!(foot_of(Some("https://discord.com/safety")), 'b');
        // Hors de Discord, sans chiffrement ou domaine d'imitation : lien ordinaire, écrit en entier.
        for plain in ["https://exemple.invalid/aide", "http://support.discord.com/hc/articles/1", "https://discord.com.exemple.invalid/x", "https://u@discord.com/x", "https://discord.com:8443/x"] {
            assert_eq!(foot_of(Some(plain)), 'l', "{plain}");
            let (text, _, zones) = pass(&embed(Some(plain)), true, NOW);
            assert!(text.contains(plain) && !text.contains("En savoir plus") && zones == 1, "{text}");
        }
        // Ni http ni https, ou illisible : rien à ouvrir, la carte reste (titre et texte).
        for none in ["javascript:alert(1)", "file:///C:/x", "data:text/html,x", "pas une adresse"] {
            assert_eq!(foot_of(Some(none)), '-', "{none}");
            let (text, _, zones) = pass(&embed(Some(none)), true, NOW);
            assert!(text.contains("Titre fictif") && zones == 0 && !text.contains("En savoir plus"), "{text}");
        }
        assert_eq!(foot_of(None), '-');
        assert!(pass(&embed(None), true, NOW).1 < pass(&embed(Some("https://discord.com/a")), true, NOW).1, "sans lien : pas de pied");
    }
}
