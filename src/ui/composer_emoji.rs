//! Emojis dans la zone de saisie (lots T4 et G) : boutons des sélecteurs
//! (GIF, autocollants, emojis) à droite du champ, Ctrl+G et Ctrl+S, insertion de l'emoji choisi au curseur, envoi d'un autocollant, et
//! mise en page du champ où `:nom:` (emoji connu ou lié) et les emojis
//! collés s'affichent en image. Le texte du champ ne change pas : `:nom:`
//! devient le caractère (ou la syntaxe `<:nom:id>`) à l'envoi, comme avant.

use super::emoji::{self, Img, Spot};
use super::emoji_picker::{self, Act, Choice, Pick, Sources, Tab};
use super::gif_picker::Act as GifAct;
use super::App;
use crate::i18n::{t as tx, table::emoji::t as et, table::gifs::t as gt, Lang};
use crate::state::usersettings::Refusal;
use crate::theme::Tokens;
use eframe::egui::{self, text::LayoutJob, Color32, FontId, Id, Sense};
use std::time::Instant;

/// Identifiant de l'emoji personnalisé d'une syntaxe `<:nom:id>` ou `<a:nom:id>`.
fn custom_id(syntax: &str) -> Option<u64> {
    syntax.strip_prefix('<')?.strip_suffix('>')?.rsplit(':').next()?.parse().ok()
}

fn word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Emoji `:nom:` qui commence `rest` (connu, ou lié à un emoji personnalisé
/// dans `links`), et sa longueur en octets.
fn named_at(rest: &str, links: &[(String, String)]) -> Option<(usize, Img)> {
    let inner = rest.strip_prefix(':')?;
    let end = inner.char_indices().take(64).find(|(_, c)| !(c.is_ascii_alphanumeric() || *c == '_' || *c == '-')).map(|(i, _)| i)?;
    if end == 0 || !inner[end..].starts_with(':') {
        return None;
    }
    let shown = &rest[..end + 2];
    if let Some((_, syntax)) = links.iter().find(|(d, _)| d == shown) {
        return custom_id(syntax).map(|id| (shown.len(), Img::Custom(id)));
    }
    crate::state::emoji::by_name(&inner[..end]).map(|u| (shown.len(), Img::Unicode(u)))
}

/// Mise en page du texte du champ de saisie, emojis en image. `fmt` : thème
/// pour l'aperçu de la mise en forme (gras, code, mentions… ; F4), sinon
/// texte uni.
pub(super) fn input_job(ui: &egui::Ui, text: &str, links: &[(String, String)], font: &FontId, wrap: f32, fmt: Option<&Tokens>) -> (LayoutJob, Vec<Spot>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = wrap;
    let mut spots = Vec::new();
    let row = emoji::row_height(ui, font);
    let side = row.min(font.size * 1.3);
    let styles = fmt.map(|t| (t, super::fonts::real(ui.ctx()), super::composer_fmt::scan(text, links)));
    // Texte de `from` à `to` (octets), découpé là où son aspect change.
    let plain = |job: &mut LayoutJob, from: usize, to: usize| {
        let Some((t, real, styles)) = &styles else {
            return job.append(&text[from..to], 0.0, egui::TextFormat { font_id: font.clone(), color: Color32::PLACEHOLDER, ..Default::default() });
        };
        let mut a = from;
        while a < to {
            let mut b = a + 1;
            while b < to && (styles[b] == styles[a] || !text.is_char_boundary(b)) {
                b += 1;
            }
            job.append(&text[a..b], 0.0, super::composer_fmt::format(t, *real, font, styles[a]));
            a = b;
        }
    };
    let (mut i, mut from, mut chars) = (0, 0, 0);
    let mut prev: Option<char> = None;
    while i < text.len() {
        let rest = &text[i..];
        let hit = if rest.starts_with(':') && !prev.is_some_and(word_char) {
            named_at(rest, links).filter(|(len, _)| !rest[*len..].chars().next().is_some_and(word_char))
        } else {
            crate::state::emoji::match_at(rest).map(|(len, u)| (len, Img::Unicode(u)))
        };
        if let Some((len, img)) = hit {
            plain(&mut job, from, i);
            let piece = &text[i..i + len];
            let n = piece.chars().count();
            spots.push(emoji::append(ui, &mut job, chars, piece, font, side, row, img));
            i += len;
            chars += n;
            from = i;
            prev = piece.chars().last();
            continue;
        }
        let Some(c) = rest.chars().next() else { break };
        i += c.len_utf8();
        chars += 1;
        prev = Some(c);
    }
    plain(&mut job, from, text.len());
    (job, spots)
}

/// Paramètre `locale` des demandes de GIF pour la langue de l'interface.
fn gif_locale(lang: Lang) -> &'static str {
    match lang {
        Lang::Fr => "fr",
        Lang::En => "en-US",
    }
}

/// Emojis du bouton de la saisie, un par survol.
const HOVER_EMOJIS: [&str; 10] = ["slight_smile", "grinning", "joy", "heart_eyes", "sunglasses", "thinking", "wink", "smirk", "star_struck", "partying_face"];

/// Emoji en couleur du bouton : le suivant de la liste à chaque nouveau survol.
fn hover_emoji(ctx: &egui::Context, active: bool) -> Option<Img> {
    let id = egui::Id::unique("ferrocord.saisie.emoji-survol");
    let (mut n, was): (usize, bool) = ctx.data(|d| d.get_temp(id)).unwrap_or_default();
    if active != was {
        n += usize::from(active);
        ctx.data_mut(|d| d.insert_temp(id, (n, active)));
    }
    let known = || HOVER_EMOJIS.iter().filter_map(|name| crate::state::emoji::by_name(name));
    let count = known().count();
    if count == 0 {
        return None;
    }
    known().nth(n % count).map(Img::Unicode)
}

impl App {
    /// Ouvre le sélecteur de la saisie sur l'onglet `tab` (ou le ferme s'il y est déjà).
    fn toggle_picker(&mut self, tab: Tab) {
        if self.emojis.composer_open && self.emojis.tab == tab {
            self.emojis.composer_open = false;
            self.emojis.closed();
        } else {
            self.emojis.composer_open = true;
            self.emojis.show_tab(tab);
        }
        self.emojis.query.clear();
        self.ctx.request_repaint();
    }

    /// Ctrl+G (`gifs`) ou Ctrl+S : ouvre ou ferme l'onglet dans le salon `cid`
    /// (les autocollants seulement s'il y en a à proposer).
    pub(super) fn picker_shortcut(&mut self, cid: u64, gifs: bool) {
        if gifs {
            self.toggle_picker(Tab::Gifs);
        } else if !self.store.picker_stickers(cid).is_empty() {
            self.toggle_picker(Tab::Stickers);
        }
    }

    /// Ouvre le sélecteur d'emojis de la saisie (Ctrl+E, aperçu, tests).
    pub fn open_emoji_picker(&mut self) {
        self.emojis.composer_open = true;
        self.emojis.show_tab(Tab::Emojis);
        self.ctx.request_repaint();
    }

    /// Ouvre le sélecteur de la saisie sur les GIF (Ctrl+G, aperçu).
    pub fn open_gif_picker(&mut self) {
        self.emojis.composer_open = true;
        self.emojis.show_tab(Tab::Gifs);
        self.ctx.request_repaint();
    }

    /// Ouvre le sélecteur de la saisie sur les autocollants (Ctrl+S).
    pub fn open_sticker_picker(&mut self) {
        self.emojis.composer_open = true;
        self.emojis.show_tab(Tab::Stickers);
        self.ctx.request_repaint();
    }

    /// Aperçu : l'onglet GIF sur les favoris, les tendances ou une recherche.
    pub fn show_gifs(&mut self, favorites: bool, query: Option<&str>) {
        self.open_gif_picker();
        let locale = gif_locale(self.lang);
        self.store.gifs_open(locale);
        match (favorites, query) {
            (true, _) => self.store.gifs_favorites(),
            (false, Some(q)) => {
                self.emojis.query = q.to_string();
                self.store.gifs_search(q, locale);
            }
            (false, None) => {}
        }
        self.flush_commands();
    }

    /// Bouton d'un sélecteur, à droite du champ : même cible que les
    /// boutons-icônes voisins, `paint` dessine le pictogramme dans son carré.
    fn picker_button(&mut self, ui: &mut egui::Ui, t: &Tokens, tab: Tab, tip: &str, paint: impl FnOnce(&egui::Ui, egui::Rect, egui::Color32, f32)) -> egui::Rect {
        let side = emoji::row_height(ui, &egui::TextStyle::Body.resolve(ui.style()));
        let hit = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y).max(side);
        let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(hit), Sense::click());
        let open = self.emojis.composer_open && self.emojis.tab == tab;
        let hot = resp.hovered() || resp.has_focus();
        // Fond et couleur en fondu ; `k` : de 0 (au repos) à 1 (survolé ou ouvert).
        let k = crate::ui::motion::hover(ui.ctx(), resp.id, hot || open);
        if k > 0.0 {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover.gamma_multiply(k));
        }
        let at = egui::Rect::from_center_size(rect.center(), egui::Vec2::splat(side));
        paint(ui, crate::ui::util::pixel_square(ui, at.min, side), crate::ui::motion::mix(t.colors.muted, t.colors.text_strong, k), k);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        if resp.on_hover_text(tip).clicked() {
            self.toggle_picker(tab);
        }
        rect
    }

    /// Bouton du sélecteur d'emojis, à droite du champ ; renvoie son rectangle.
    pub(super) fn emoji_button(&mut self, ui: &mut egui::Ui, t: &Tokens) -> egui::Rect {
        let tip = et::OPEN_PICKER.get(self.lang);
        self.picker_button(ui, t, Tab::Emojis, tip, |ui, at, _, k| {
            // Comme Discord : visage gris au repos ; au survol, un emoji en couleur, différent à chaque fois.
            let img = hover_emoji(ui.ctx(), k > 0.0);
            if k < 1.0 || img.is_none() {
                crate::theme::icons::paint(crate::theme::icons::Icon::Smile, ui.painter(), at, t.colors.muted.gamma_multiply(if img.is_some() { 1.0 - k } else { 1.0 }));
            }
            if let Some(img) = img.filter(|_| k > 0.0) {
                let grow = if crate::ui::motion::level(ui.ctx()) == crate::ui::motion::Level::Full { egui::lerp(1.0..=crate::ui::motion::GROW, k) } else { 1.0 };
                emoji::paint_faded(ui, egui::Rect::from_center_size(at.center(), at.size() * grow), img, k);
            }
        })
    }

    /// Bouton des applications, tout à droite comme dans le client : ouvre la
    /// liste des commandes « / » (la même entrée que « Utiliser une
    /// application » du menu « + »). Un brouillon déjà commencé est gardé.
    pub(super) fn apps_button(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let tip = crate::i18n::table::ff::t::PLUS_APPS.get(self.lang);
        let side = emoji::row_height(ui, &egui::TextStyle::Body.resolve(ui.style()));
        let hit = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y).max(side);
        let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(hit), Sense::click());
        let k = crate::ui::motion::hover(ui.ctx(), resp.id, resp.hovered() || resp.has_focus());
        if k > 0.0 {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover.gamma_multiply(k));
        }
        let at = crate::ui::util::pixel_square(ui, rect.center() - egui::Vec2::splat(side / 2.0), side);
        crate::theme::icons::paint(crate::theme::icons::Icon::Apps, ui.painter(), at.shrink(side * 0.06), crate::ui::motion::mix(t.colors.muted, t.colors.text_strong, k));
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
        if resp.on_hover_text(tip).clicked() {
            self.open_command_list();
        }
    }

    /// Bouton « GIF » : le mot dans un cadre arrondi.
    pub(super) fn gif_button(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let tip = gt::OPEN_GIFS.get(self.lang);
        let (radius, stroke) = (t.r_row(), t.shapes.border.max(1.0) * 1.5);
        self.picker_button(ui, t, Tab::Gifs, tip, |ui, at, color, _| {
            let frame = egui::Rect::from_center_size(at.center(), egui::vec2(at.width() * 1.15, at.height() * 0.72));
            ui.painter().rect_stroke(frame, radius, egui::Stroke::new(stroke, color), egui::StrokeKind::Inside);
            ui.painter().text(frame.center(), egui::Align2::CENTER_CENTER, "GIF", crate::ui::fonts::demi_font(frame.height() * 0.52), color);
        });
    }

    /// Bouton des autocollants : une feuille carrée au coin décollé.
    pub(super) fn sticker_button(&mut self, ui: &mut egui::Ui, t: &Tokens, cid: u64) {
        // Hors serveur ou sans autocollant disponible : rien à proposer.
        if self.store.picker_stickers(cid).is_empty() {
            return;
        }
        let tip = gt::OPEN_STICKERS.get(self.lang);
        let stroke = t.shapes.border.max(1.0) * 1.5;
        self.picker_button(ui, t, Tab::Stickers, tip, |ui, at, color, _| {
            let r = at.shrink(at.width() * 0.12);
            let fold = r.width() * 0.36;
            let s = egui::Stroke::new(stroke, color);
            let corner = egui::pos2(r.right() - fold, r.bottom() - fold);
            ui.painter().add(egui::Shape::closed_line(vec![r.left_top(), r.right_top(), egui::pos2(r.right(), r.bottom() - fold), egui::pos2(r.right() - fold, r.bottom()), r.left_bottom()], s));
            ui.painter().add(egui::Shape::line(vec![egui::pos2(r.right(), r.bottom() - fold), corner, egui::pos2(r.right() - fold, r.bottom())], s));
        });
    }

    /// Un emoji vient d'être choisi (saisie, réaction, statut) : en tête des
    /// récents de la session, et compté dans l'usage local (F5).
    pub(super) fn emoji_used(&mut self, e: &crate::state::social::Emoji) {
        self.emojis.used(e);
        if let Some(key) = crate::state::gifs::emoji_key(e) {
            let now = self.store.now();
            self.store.gifs.frecency.bind_local(self.store.me.as_ref().map(|u| u.id), now);
            self.store.gifs.frecency.note_use(&key, now);
        }
    }

    /// Applique ce que le sélecteur a demandé pendant son affichage.
    pub(super) fn apply_picker_acts(&mut self) {
        // Usage local des emojis du compte affiché (relu seulement s'il a changé).
        let now = self.store.now();
        self.store.gifs.frecency.bind_local(self.store.me.as_ref().map(|u| u.id), now);
        let locale = gif_locale(self.lang);
        for act in self.emojis.take_acts() {
            let saved = match act {
                Act::Opened(Tab::Gifs) => {
                    self.store.gifs_open(locale);
                    Ok(())
                }
                Act::Opened(_) => {
                    self.store.expressions_open();
                    Ok(())
                }
                Act::FavEmoji(key) => self.store.toggle_fav_emoji(&key),
                Act::Sounds => {
                    self.emojis.composer_open = false;
                    self.emojis.closed();
                    if let Some(my) = self.store.voice.me {
                        self.open_soundboard(my.channel_id);
                    }
                    Ok(())
                }
                Act::Gif(GifAct::Typed(q)) => {
                    self.store.gifs_typed(&q, Instant::now());
                    Ok(())
                }
                Act::Gif(GifAct::Search(q)) => {
                    self.store.gifs_search(&q, locale);
                    Ok(())
                }
                Act::Gif(GifAct::Trending) => {
                    self.store.gifs_trending(locale);
                    Ok(())
                }
                Act::Gif(GifAct::Favorites) => {
                    self.store.gifs_favorites();
                    Ok(())
                }
                Act::Gif(GifAct::Home) => {
                    self.store.gifs_home();
                    Ok(())
                }
                Act::Gif(GifAct::Retry) => {
                    self.store.gifs_open(locale);
                    Ok(())
                }
                Act::Gif(GifAct::ToggleFav(g)) => self.store.toggle_fav_gif(&g),
            };
            match saved {
                Ok(()) => {}
                Err(Refusal::Busy) => self.notify(gt::FAVORITES_BUSY),
                Err(Refusal::Unknown) => self.notify(gt::FAVORITES_UNKNOWN),
            }
        }
    }

    /// Sélecteur de la saisie, au-dessus du bouton.
    pub(super) fn composer_picker(&mut self, ui: &mut egui::Ui, cid: u64, anchor: egui::Rect) {
        // Ctrl+G et Ctrl+S passent par les raccourcis réglables (`picker_shortcut`).
        if !self.emojis.composer_open {
            return;
        }
        let t = self.theme.clone();
        let lang = self.lang;
        // Recherche de GIF tapée : elle part quand le clavier se tait.
        if self.emojis.tab == Tab::Gifs
            && let Some(wait) = self.store.gifs_tick(Instant::now(), gif_locale(lang))
        {
            ui.ctx().request_repaint_after(wait);
        }
        let area = egui::Area::new(Id::unique("sélecteur-saisie")).order(egui::Order::Foreground).pivot(egui::Align2::RIGHT_BOTTOM).fixed_pos(anchor.right_top()).constrain(true);
        let store = &self.store;
        let images = &self.images;
        // Jamais plus haut que la place au-dessus de la saisie (fenêtre basse).
        let margin = 2.0 * (f32::from(t.embed_margin().top) + t.shapes.border.max(1.0));
        self.emojis.room = Some(anchor.top() - ui.ctx().content_rect().top() - margin - t.spacing.item_y);
        let st = &mut self.emojis;
        let mut query = std::mem::take(&mut st.query);
        let mut chosen = None;
        let shown = area.show(ui.ctx(), |ui| {
            t.window_frame().inner_margin(t.embed_margin()).show(ui, |ui| {
                ui.set_width(emoji_picker::width(ui.ctx(), &t));
                let src = Sources::of(store, cid, true);
                chosen = emoji_picker::body(ui, &t, lang, st, &mut query, &src, images);
            });
        });
        self.emojis.query = query;
        self.apply_picker_acts();
        // Un clic sur un des boutons de la saisie ouvre ou ferme lui-même le sélecteur.
        let pointer = ui.input(|i| i.pointer.interact_pos()).unwrap_or_default();
        let on_buttons = anchor.expand2(egui::vec2(anchor.width() * 3.0, 0.0)).contains(pointer);
        let outside = ui.input(|i| i.pointer.any_pressed()) && !shown.response.contains_pointer() && !on_buttons;
        let escape = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if outside || escape {
            self.emojis.composer_open = false;
        }
        if let Some(Choice { pick, keep_open, .. }) = chosen {
            self.emojis.composer_open = keep_open;
            match pick {
                Pick::Emoji(e) => {
                    self.emoji_used(&e);
                    let (mut text, link) = emoji_picker::insertion(&e);
                    if let Some((shown, syntax)) = link
                        && !self.input_link(shown, syntax.clone())
                    {
                        text = syntax;
                    }
                    self.insert_in_composer(&format!("{text} "));
                }
                Pick::Sticker(s) => {
                    // Documenté (Create Message, `sticker_ids`) ; même file d'envoi que les messages.
                    if !self.store.send_sticker(cid, s.id, s.name.clone()) {
                        self.notify(tx::CANNOT_SEND);
                    } else {
                        self.scroll_to_bottom = true;
                    }
                }
                Pick::Gif { id, url } => {
                    // L'adresse du GIF en message (réponse en cours comprise), comme le client officiel.
                    if self.send_text(cid, url) {
                        self.store.gifs_selected(&id);
                        self.scroll_to_bottom = true;
                    } else {
                        self.notify(tx::CANNOT_SEND);
                    }
                }
            }
        }
        if !self.emojis.composer_open {
            self.emojis.closed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_spots_follow_names_links_and_pasted_emojis() {
        let ctx = egui::Context::default();
        let links = vec![(":blob:".to_string(), "<a:blob:42>".to_string())];
        let mut got = Vec::new();
        let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0))), ..Default::default() };
        let mut out = ctx.run_ui(input, |ui| {
            let font = FontId::proportional(14.0);
            let (job, spots) = input_job(ui, "salut :fire: a:fire: :blob: :inconnu: 👍 é", &links, &font, 300.0, None);
            assert_eq!(job.text, "salut :fire: a:fire: :blob: :inconnu: 👍 é");
            got = spots.iter().map(|s| (s.chars.clone(), s.img)).collect();
        });
        out.textures_delta.clear();
        let fire = crate::state::emoji::by_name("fire").map(Img::Unicode);
        let up = crate::state::emoji::by_name("thumbsup").map(Img::Unicode);
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!((got[0].0.clone(), Some(got[0].1)), (6..12, fire));
        assert_eq!(got[1], (21..27, Img::Custom(42)));
        assert_eq!((got[2].0.clone(), Some(got[2].1)), (38..39, up));
        assert_eq!(custom_id("<:a:7>"), Some(7));
        assert_eq!(custom_id("pas une syntaxe"), None);
    }
}
