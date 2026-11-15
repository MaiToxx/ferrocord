//! Emoji du statut personnalisé (lot R) : un bouton qui montre l'emoji
//! choisi et ouvre le sélecteur d'emojis, à la place d'un champ de texte
//! (fenêtre du statut personnalisé, `quick_profile.rs`).
//! Les emojis personnalisés sont ceux que le sélecteur propose hors d'un
//! salon : aucun sans Nitro, ceux de tous mes serveurs avec.

use super::emoji::{self, Img};
use super::emoji_picker::{self, Choice, Pick, Sources, Tab};
use super::settings::synced::StatusDraft;
use super::App;
use crate::i18n::table::restes::t as rt;
use crate::i18n::Lang;
use crate::state::social::Emoji;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Id, Sense};

/// Sélecteur ouvert : bouton d'où il part, et sa recherche.
pub(super) struct StatusPicker {
    anchor: egui::Rect,
    query: String,
}

impl StatusPicker {
    pub(super) fn at(anchor: egui::Rect) -> Self {
        StatusPicker { anchor, query: String::new() }
    }
}

/// Image de l'emoji du brouillon, s'il en a un qu'on sait dessiner.
fn image(d: &StatusDraft) -> Option<Img> {
    let name = d.emoji.trim();
    match &d.custom {
        Some((id, n, animated)) if n == name => Some(Img::custom(*id, *animated)),
        _ => crate::state::emoji::find(name).map(Img::Unicode),
    }
}

/// Bouton de l'emoji du statut : l'emoji choisi, sinon un visage gris.
pub(super) fn button(ui: &mut egui::Ui, t: &Tokens, lang: Lang, d: &StatusDraft, open: bool) -> egui::Response {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let side = emoji::row_height(ui, &font);
    let hit = ui.spacing().interact_size.y.max(side);
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(hit), Sense::click());
    let tip = rt::STATUS_EMOJI.get(lang);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    if ui.is_rect_visible(rect) {
        let hot = resp.hovered() || resp.has_focus() || open;
        ui.painter().rect_filled(rect, t.r_row(), if hot { t.colors.row_hover } else { t.colors.bg3 });
        let at = super::util::pixel_square(ui, egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(side)).min, side);
        match image(d) {
            Some(img) => emoji::paint(ui, at, img),
            None => icons::paint(Icon::Smile, ui.painter(), at, if hot { t.colors.text_strong } else { t.colors.muted }),
        }
    }
    resp.on_hover_text(tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

impl App {
    /// Sélecteur de l'emoji du statut, au-dessus de son bouton ; un choix,
    /// Échap ou un clic en dehors le ferme.
    pub(super) fn status_emoji_picker(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.quick.status_picker.take() else { return };
        let t = self.theme.clone();
        let lang = self.lang;
        let area = egui::Area::new(Id::unique("sélecteur-statut")).order(egui::Order::Foreground).pivot(egui::Align2::LEFT_BOTTOM).fixed_pos(p.anchor.left_top()).constrain(true);
        let has = !self.params.status.emoji.trim().is_empty();
        let (mut clear, mut chosen) = (false, None);
        let store = &self.store;
        let images = &self.images;
        let emojis = &mut self.emojis;
        // Emojis seuls ici : l'onglet du sélecteur de la saisie est gardé de côté.
        let tab = std::mem::replace(&mut emojis.tab, Tab::Emojis);
        let shown = area.show(ctx, |ui| {
            t.window_frame().inner_margin(t.embed_margin()).show(ui, |ui| {
                ui.set_width(emoji_picker::width(ui.ctx(), &t));
                if has && ui.button(rt::STATUS_EMOJI_CLEAR.get(lang)).clicked() {
                    clear = true;
                }
                // Hors d'un salon : pas de super réaction, pas d'autocollants.
                let src = Sources { super_ok: false, ..Sources::of(store, 0, false) };
                chosen = emoji_picker::body(ui, &t, lang, emojis, &mut p.query, &src, images);
            });
        });
        self.emojis.tab = tab;
        self.apply_picker_acts();
        let on_button = ctx.input(|i| i.pointer.interact_pos()).is_some_and(|q| p.anchor.contains(q));
        let outside = ctx.input(|i| i.pointer.any_pressed()) && !shown.response.contains_pointer() && !on_button;
        let escape = ctx.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        let d = &mut self.params.status;
        if clear {
            d.emoji.clear();
            d.custom = None;
        }
        let picked = match chosen {
            Some(Choice { pick: Pick::Emoji(e), .. }) => {
                match &e {
                    Emoji::Unicode(s) => {
                        d.emoji = s.clone();
                        d.custom = None;
                    }
                    Emoji::Custom { id, name, animated } => {
                        d.emoji = name.clone();
                        d.custom = Some((*id, name.clone(), *animated));
                    }
                }
                self.emoji_used(&e);
                true
            }
            _ => false,
        };
        if !(picked || clear || outside || escape) {
            self.quick.status_picker = Some(p);
        }
    }

    /// Un emoji personnalisé de mes serveurs est-il animé ? (Mon statut
    /// enregistré ne le dit pas.)
    pub(super) fn emoji_animated(&self, id: u64) -> bool {
        self.store.guilds.iter().flat_map(|g| g.emojis.iter()).any(|e| e.id == id && e.animated)
    }
}

#[cfg(test)]
mod tests {
    use super::super::quick_profile::tests::{app, escape, frame, has};
    use super::*;
    use crate::state::usersettings::schema::CustomStatus;

    #[test]
    fn the_draft_keeps_a_picked_custom_emoji_until_its_name_changes() {
        let mut d = StatusDraft::default();
        d.sync(&None);
        d.emoji = "pain".into();
        d.custom = Some((7, "pain".into(), true));
        assert_eq!(image(&d), Some(Img::Animated(7)));
        assert_eq!(d.build(None, 0).emoji_id, 7);
        // Nom retapé à la main (page des paramètres) : ce n'est plus cet emoji.
        d.emoji = "☕".into();
        assert_eq!(d.build(None, 0).emoji_id, 0);
        assert!(matches!(image(&d), Some(Img::Unicode(_))));
        // Statut enregistré avec un emoji personnalisé : gardé tel quel.
        let saved = Some(CustomStatus { text: "t".into(), emoji_name: "pain".into(), emoji_id: 7, expires_at_ms: 0 });
        d.sync(&saved);
        assert_eq!(d.build(saved.as_ref(), 0).emoji_id, 7);
    }

    #[test]
    fn the_status_window_opens_the_picker_and_a_pick_fills_the_draft() {
        let mut app = app();
        app.open_custom_status();
        frame(&mut app, Vec::new());
        frame(&mut app, Vec::new());
        assert!(app.quick.status_picker.is_none());
        app.quick.status_picker = Some(StatusPicker::at(egui::Rect::from_min_size(egui::pos2(40.0, 400.0), egui::Vec2::splat(24.0))));
        frame(&mut app, Vec::new());
        let texts = frame(&mut app, Vec::new());
        assert!(has(&texts, "Rechercher un emoji") || has(&texts, "Fréquemment utilisés") || has(&texts, "Personnes"), "sélecteur absent : {texts:?}");
        assert!(app.custom_status_open(), "la fenêtre du statut reste ouverte sous le sélecteur");
        assert!(!has(&texts, "Retirer l'emoji"), "rien à retirer : {texts:?}");
        app.params.status.emoji = "☕".into();
        let texts = frame(&mut app, Vec::new());
        assert!(has(&texts, "Retirer l'emoji"), "{texts:?}");
        // Échap ferme le sélecteur, pas la fenêtre.
        frame(&mut app, escape());
        assert!(app.quick.status_picker.is_none() && app.custom_status_open());
        app.quick.status_picker = Some(StatusPicker::at(egui::Rect::from_min_size(egui::pos2(40.0, 400.0), egui::Vec2::splat(24.0))));
        frame(&mut app, Vec::new());
        // Fenêtre fermée : le sélecteur part avec elle.
        app.close_custom_status();
        frame(&mut app, Vec::new());
        assert!(app.quick.status_picker.is_none() && !app.custom_status_open());
    }
}
