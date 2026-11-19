//! Sélecteur d'expressions (lots T4 et G) : onglets GIF, Autocollants et
//! Emojis ; recherche ; rail de catégories à gauche (favoris, fréquents,
//! serveurs, catégories de Discord) ; grille d'emojis à sections repliables
//! et en-tête collant ; bandeau de l'emoji survolé ; teinte de peau ; super
//! réaction. Ouvert depuis la saisie (boutons à droite du champ, Ctrl+E,
//! Ctrl+G, Ctrl+S) et depuis « Ajouter une réaction ».
//!
//! La grille est virtualisée : la place de chaque section est calculée,
//! seules les rangées visibles sont mises en page, et seules les images
//! visibles sont demandées.
//!
//! Le sélecteur ne touche pas au magasin : ce qu'il demande (favori,
//! recherche de GIF…) est rangé dans [`EmojiUi::take_acts`], appliqué par
//! `App` après l'affichage.

use super::emoji::{self, Img};
use super::gif_picker;
use crate::i18n::{table::emoji::t as et, table::gifs::t as gt, Lang, Text};
use crate::images::Images;
use crate::state::emoji::{self as table, Category, GuildSticker, StickerFormat, StickerItem, Unicode, CATEGORIES, TONES};
use crate::state::frecency::Frecency;
use crate::state::gifs::{emoji_key, Gifs};
use crate::state::model;
use crate::state::social::Emoji;
use crate::state::Store;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, Rect, Sense};
use std::collections::{HashSet, VecDeque};

/// Emojis récents gardés en mémoire, au plus (les plus anciens partent).
pub(super) const MAX_RECENTS: usize = 32;
/// Résultats d'une recherche, au plus.
const MAX_RESULTS: usize = 400;
/// Emojis par rangée (comme Discord).
const COLS: usize = 9;
/// Rangées visibles de la grille quand la fenêtre est assez haute.
const GRID_ROWS: f32 = 7.0;
/// Rangées visibles au moins (fenêtre basse).
const MIN_ROWS: f32 = 3.0;
/// Emojis de la section « Fréquents », au plus (trois rangées).
const FREQUENT_SHOWN: usize = 3 * COLS;
/// Les autocollants du sélecteur sont montrés à cette fraction de leur taille dans les messages.
const STICKER_CELL: f32 = 0.5;
/// Proposés tant qu'aucun emoji n'a été choisi et que le compte n'a pas de fréquents.
const DEFAULTS: &[&str] = &["thumbsup", "heart", "joy", "fire", "tada", "eyes", "open_mouth", "cry", "pray", "white_check_mark", "thinking", "100"];

/// Côté d'une cellule de la grille : 48 pour un emoji de 40 (`markdown.picker_emoji` de 32).
pub(super) fn cell(t: &Tokens) -> f32 {
    (t.markdown.picker_emoji * 1.5).round()
}

/// Côté de l'emoji dans sa cellule.
fn emoji_side(t: &Tokens) -> f32 {
    (t.markdown.picker_emoji * 1.25).round()
}

/// Hauteur d'un en-tête de section.
fn head_height(t: &Tokens) -> f32 {
    (cell(t) * 2.0 / 3.0).round()
}

/// Largeur du contenu du sélecteur : rail, neuf cellules, barre de défilement.
pub(super) fn width(ctx: &egui::Context, t: &Tokens) -> f32 {
    let full = cell(t) * (COLS as f32 + 1.0) + t.spacing.item_x;
    let room = ctx.content_rect().width() - 2.0 * t.spacing.card_pad;
    if super::mobile::is_active(ctx) { full.min(super::mobile::card_width(ctx, t, 1.0)) } else { full.min(room.max(cell(t) * 4.0)) }
}

/// Hauteur de la grille : sept rangées, moins si la fenêtre est basse.
pub(super) fn grid_height(ctx: &egui::Context, t: &Tokens) -> f32 {
    let c = cell(t);
    // Onglets, recherche, bandeau, zone de saisie et marges autour de la grille.
    let chrome = c * 4.5 + 4.0 * t.spacing.card_pad;
    (ctx.content_rect().height() - chrome).clamp(c * MIN_ROWS, c * GRID_ROWS)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum Tab {
    Gifs,
    Stickers,
    #[default]
    Emojis,
}

/// Section de la grille (clé du repli et de l'entrée du rail).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum SecKey {
    Favorites,
    Frequent,
    Guild(u64),
    Cat(Category),
    Results,
}

/// Demande à appliquer au magasin après l'affichage.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Act {
    /// L'onglet vient de s'afficher : ses données sont à charger.
    Opened(Tab),
    /// Emoji à ajouter aux favoris du compte, ou à en retirer.
    FavEmoji(String),
    Gif(gif_picker::Act),
    /// Onglet « Sons » (en vocal, lot V) : le tableau de sons est à ouvrir.
    Sounds,
}

/// Emoji survolé, montré dans le bandeau.
#[derive(Clone, Debug, PartialEq)]
struct Hover {
    img: Img,
    name: String,
    origin: String,
}

/// État du sélecteur, partagé par la saisie et les réactions (appartient à `App`).
#[derive(Default)]
pub(super) struct EmojiUi {
    recents: VecDeque<Emoji>,
    /// Teinte de peau choisie (0 : aucune).
    pub tone: u8,
    /// Sélecteur de la saisie ouvert.
    pub composer_open: bool,
    /// Recherche du sélecteur de la saisie.
    pub query: String,
    pub tab: Tab,
    /// Section à faire défiler jusqu'en haut (clic dans le rail).
    jump: Option<usize>,
    collapsed: HashSet<SecKey>,
    /// Section en haut de la grille (entrée active du rail).
    current: usize,
    hover: Option<Hover>,
    /// L'emoji choisi part en super réaction.
    pub super_mode: bool,
    acts: Vec<Act>,
    /// Onglet dont les données ont été demandées (aucun : sélecteur fermé).
    announced: Option<Tab>,
    /// Hauteur libre pour tout le sélecteur à cette image (au-dessus de la
    /// saisie) : la grille s'y tient, pour ne pas recouvrir ce d'où il part.
    pub room: Option<f32>,
    /// Hauteur de tout ce qui entoure la grille, mesurée à l'image d'avant.
    chrome: f32,
}

impl EmojiUi {
    /// Changement de compte : récents et emojis des serveurs oubliés.
    pub(super) fn clear(&mut self) {
        *self = EmojiUi::default();
    }

    /// `e` vient d'être choisi : en tête des récents.
    pub(super) fn used(&mut self, e: &Emoji) {
        self.recents.retain(|x| !x.same(e));
        self.recents.push_front(e.clone());
        self.recents.truncate(MAX_RECENTS);
    }

    /// Les derniers emojis choisis, du plus récent au plus ancien (réactions
    /// rapides de la barre de survol, lot M).
    pub(super) fn recent(&self) -> impl Iterator<Item = &Emoji> {
        self.recents.iter()
    }

    /// Le sélecteur n'est plus affiché : à sa réouverture, l'onglet se réannonce.
    pub(super) fn closed(&mut self) {
        self.announced = None;
        self.hover = None;
    }

    pub(super) fn take_acts(&mut self) -> Vec<Act> {
        std::mem::take(&mut self.acts)
    }

    /// Passe à l'onglet `tab`, recherche vidée.
    pub(super) fn show_tab(&mut self, tab: Tab) {
        if self.tab != tab {
            self.tab = tab;
            self.query.clear();
            self.jump = None;
        }
    }
}

/// Ce que l'utilisateur a choisi.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Pick {
    Emoji(Emoji),
    Sticker(StickerItem),
    /// GIF à envoyer : son identifiant (vide pour un favori) et son adresse.
    Gif { id: String, url: String },
}

/// Choix, s'il faut garder le sélecteur ouvert (Maj+clic), et s'il part en super réaction.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Choice {
    pub pick: Pick,
    pub keep_open: bool,
    pub burst: bool,
}

/// Ce que le sélecteur propose dans un salon.
pub(super) struct Sources<'a> {
    pub servers: Vec<(&'a model::Guild, Vec<&'a model::Emoji>)>,
    /// Emojis des autres serveurs, réservés à Nitro (F26) : montrés grisés.
    pub locked: Vec<(&'a model::Guild, Vec<&'a model::Emoji>)>,
    pub stickers: Vec<&'a GuildSticker>,
    /// Favoris et fréquence du compte.
    pub frecency: &'a Frecency,
    /// Onglet GIF (saisie seulement).
    pub gifs: Option<&'a Gifs>,
    /// Interrupteur de super réaction (réactions, avec Nitro).
    pub super_ok: bool,
    /// Onglet « Sons » : saisie seulement, quand je suis dans un salon vocal (lot V).
    pub sounds: bool,
}

impl<'a> Sources<'a> {
    /// `composer` : sélecteur de la saisie (onglets GIF et Autocollants) ;
    /// sinon celui d'une réaction (emojis seuls, super réaction avec Nitro).
    pub(super) fn of(store: &'a Store, channel: u64, composer: bool) -> Self {
        Sources {
            servers: store.picker_emojis(channel),
            locked: store.locked_emojis(channel),
            stickers: if composer { store.picker_stickers(channel) } else { Vec::new() },
            frecency: &store.gifs.frecency,
            gifs: composer.then_some(&store.gifs),
            super_ok: !composer && store.has_full_nitro(),
            sounds: composer && store.voice.me.is_some(),
        }
    }

    fn custom(&self, id: u64) -> Option<(&'a model::Guild, &'a model::Emoji)> {
        self.servers.iter().find_map(|(g, l)| l.iter().find(|e| e.id == id).map(|e| (*g, *e)))
    }
}

#[derive(Clone, Copy)]
enum Item<'a> {
    Unicode(Unicode),
    Custom(&'a model::Emoji),
    Recent(&'a Emoji),
}

impl Item<'_> {
    /// Image, nom et emoji choisi ; `tone` s'applique aux emojis qui en ont.
    fn resolve(self, tone: u8) -> Option<(Img, String, Emoji)> {
        match self {
            Item::Unicode(u) => {
                let u = u.with_tone(tone);
                Some((Img::Unicode(u), u.name(), Emoji::Unicode(u.text())))
            }
            Item::Custom(e) => Some((Img::custom(e.id, e.animated), e.name.clone(), Emoji::Custom { id: e.id, name: e.name.clone(), animated: e.animated })),
            Item::Recent(e) => {
                let img = Img::of(e)?;
                let name = match e {
                    Emoji::Unicode(s) => table::shortcode(s)?,
                    Emoji::Custom { name, .. } => name.clone(),
                };
                Some((img, name, e.clone()))
            }
        }
    }
}

enum Items<'a> {
    Table(&'static [u16]),
    List(Vec<Item<'a>>),
}

impl<'a> Items<'a> {
    fn len(&self) -> usize {
        match self {
            Items::Table(t) => t.len(),
            Items::List(l) => l.len(),
        }
    }

    fn get(&self, i: usize) -> Option<Item<'a>> {
        match self {
            Items::Table(t) => t.get(i).map(|index| Item::Unicode(Unicode { index: *index, tone: 0 })),
            Items::List(l) => l.get(i).copied(),
        }
    }
}

struct Section<'a> {
    key: SecKey,
    title: String,
    /// Serveur de la section (icône du rail et de l'en-tête).
    guild: Option<&'a model::Guild>,
    items: Items<'a>,
    /// Emojis réservés à Nitro : grisés, non cliquables (F26).
    locked: bool,
}

fn category_name(c: Category) -> Text {
    match c {
        Category::People => et::CAT_PEOPLE,
        Category::Nature => et::CAT_NATURE,
        Category::Food => et::CAT_FOOD,
        Category::Activity => et::CAT_ACTIVITY,
        Category::Travel => et::CAT_TRAVEL,
        Category::Objects => et::CAT_OBJECTS,
        Category::Symbols => et::CAT_SYMBOLS,
        Category::Flags => et::CAT_FLAGS,
    }
}

/// Emoji d'une clé des favoris ou de la fréquence du compte : identifiant
/// d'un emoji personnalisé utilisable ici, ou nom court.
fn item_of_key<'a>(key: &str, src: &Sources<'a>) -> Option<Item<'a>> {
    match key.parse::<u64>() {
        Ok(id) => src.custom(id).map(|(_, e)| Item::Custom(e)),
        Err(_) => table::by_name(key).map(Item::Unicode),
    }
}

/// Section « Fréquents » : les emojis choisis pendant la session, puis les
/// mieux notés du compte, sans doublon ; à défaut, une sélection de départ.
fn frequent<'a>(recents: &'a VecDeque<Emoji>, src: &Sources<'a>) -> Vec<Item<'a>> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut out: Vec<Item> = Vec::new();
    for e in recents {
        if out.len() < FREQUENT_SHOWN && emoji_key(e).is_none_or(|k| seen.insert(k)) {
            out.push(Item::Recent(e));
        }
    }
    for key in &src.frecency.frequent {
        if out.len() < FREQUENT_SHOWN
            && !seen.contains(key)
            && let Some(item) = item_of_key(key, src)
        {
            seen.insert(key.clone());
            out.push(item);
        }
    }
    if out.is_empty() {
        out = DEFAULTS.iter().filter_map(|n| table::by_name(n)).map(Item::Unicode).collect();
    }
    out
}

/// Sections montrées pour cette recherche (vide : toutes, dans l'ordre de Discord).
fn sections<'a>(lang: Lang, query: &str, recents: &'a VecDeque<Emoji>, src: &Sources<'a>) -> Vec<Section<'a>> {
    let q = query.trim().trim_matches(':').to_lowercase();
    if !q.is_empty() {
        let mut items: Vec<Item> = src.servers.iter().flat_map(|(_, l)| l.iter()).filter(|e| e.name.to_lowercase().contains(&q)).map(|e| Item::Custom(e)).collect();
        items.extend(table::search(&q, MAX_RESULTS).into_iter().map(Item::Unicode));
        items.truncate(MAX_RESULTS);
        return vec![Section { key: SecKey::Results, title: et::RESULTS.get(lang).to_string(), guild: None, items: Items::List(items), locked: false }];
    }
    let mut out = Vec::new();
    let favorites: Vec<Item> = src.frecency.emojis.iter().filter_map(|k| item_of_key(k, src)).collect();
    if !favorites.is_empty() {
        out.push(Section { key: SecKey::Favorites, title: gt::EMOJI_FAVORITES.get(lang).to_string(), guild: None, items: Items::List(favorites), locked: false });
    }
    out.push(Section { key: SecKey::Frequent, title: et::RECENT.get(lang).to_string(), guild: None, items: Items::List(frequent(recents, src)), locked: false });
    for (g, list) in &src.servers {
        out.push(Section { key: SecKey::Guild(g.id), title: g.name.clone(), guild: Some(g), items: Items::List(list.iter().map(|e| Item::Custom(e)).collect()), locked: false });
    }
    for (g, list) in &src.locked {
        out.push(Section { key: SecKey::Guild(g.id), title: g.name.clone(), guild: Some(g), items: Items::List(list.iter().map(|e| Item::Custom(e)).collect()), locked: true });
    }
    for c in CATEGORIES {
        out.push(Section { key: SecKey::Cat(c), title: category_name(c).get(lang).to_string(), guild: None, items: Items::Table(table::category(c)), locked: false });
    }
    out
}

/// Place d'une section dans la grille : haut de son en-tête, rangées, dépliée.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Placed {
    top: f32,
    rows: usize,
    open: bool,
}

impl Placed {
    fn bottom(&self, cell: f32, head: f32) -> f32 {
        self.top + head + if self.open { self.rows as f32 * cell } else { 0.0 }
    }
}

/// Place de chaque section, et hauteur totale. Une section repliée ne garde
/// que son en-tête ; les résultats d'une recherche ne se replient pas.
fn place(sections: &[Section], cols: usize, cell: f32, head: f32, collapsed: &HashSet<SecKey>) -> (Vec<Placed>, f32) {
    let mut out = Vec::with_capacity(sections.len());
    let mut y = 0.0;
    for s in sections {
        let p = Placed { top: y, rows: s.items.len().div_ceil(cols.max(1)), open: s.key == SecKey::Results || !collapsed.contains(&s.key) };
        y = p.bottom(cell, head);
        out.push(p);
    }
    (out, y)
}

/// Onglet : texte demi-gras, fond neutre quand il est actif.
fn tab_button(ui: &mut egui::Ui, t: &Tokens, label: &str, active: bool) -> egui::Response {
    let font = super::fonts::demi_font(t.typography.body);
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, egui::Color32::PLACEHOLDER);
    let pad = egui::vec2(t.spacing.control_x, t.spacing.control_y);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if active {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.selected);
    } else if hot {
        ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
    }
    let color = if active || hot { t.colors.text_strong } else { t.colors.muted };
    ui.painter().galley(rect.min + pad, galley, color);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, active, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Champ de recherche pleine largeur, loupe à droite ; `trailing` : boutons
/// à droite du champ (teinte, super réaction) et leur largeur. Renvoie
/// (Entrée pressée, texte changé).
fn search_row(ui: &mut egui::Ui, t: &Tokens, hint: &str, query: &mut String, trailing_width: f32, trailing: impl FnOnce(&mut egui::Ui)) -> (bool, bool) {
    let (mut enter, mut changed) = (false, false);
    ui.horizontal(|ui| {
        let icon = Tokens::icon_side(t.typography.body);
        let gap = if trailing_width > 0.0 { t.spacing.item_x } else { 0.0 };
        let width = (ui.available_width() - trailing_width - gap).max(0.0);
        let margin = egui::Margin { left: t.spacing.control_x as i8, right: (t.spacing.control_x + icon + t.spacing.item_x / 2.0) as i8, top: t.spacing.control_y as i8, bottom: t.spacing.control_y as i8 };
        let edit = ui.add(egui::TextEdit::singleline(query).hint_text(hint).margin(margin).desired_width(width));
        if query.is_empty() && !edit.has_focus() && !edit.lost_focus() {
            edit.request_focus();
        }
        enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        changed = edit.changed();
        let at = Rect::from_center_size(egui::pos2(edit.rect.right() - t.spacing.control_x - icon / 2.0, edit.rect.center().y), egui::Vec2::splat(icon));
        icons::paint(Icon::Search, ui.painter(), at, t.colors.muted);
        trailing(ui);
    });
    (enter, changed)
}

/// Contenu du sélecteur. `query` : la recherche (gardée par l'appelant).
pub(super) fn body(ui: &mut egui::Ui, t: &Tokens, lang: Lang, st: &mut EmojiUi, query: &mut String, src: &Sources, images: &Images) -> Option<Choice> {
    let tabs: Vec<(Tab, Text)> = [(Tab::Gifs, gt::TAB_GIFS, src.gifs.is_some()), (Tab::Stickers, et::TAB_STICKERS, !src.stickers.is_empty()), (Tab::Emojis, et::TAB_EMOJIS, true)]
        .into_iter()
        .filter(|(_, _, shown)| *shown)
        .map(|(tab, label, _)| (tab, label))
        .collect();
    if !tabs.iter().any(|(tab, _)| *tab == st.tab) {
        st.tab = Tab::Emojis;
    }
    if st.announced != Some(st.tab) {
        st.announced = Some(st.tab);
        st.acts.push(Act::Opened(st.tab));
    }
    let top = ui.cursor().top();
    ui.spacing_mut().item_spacing.y = t.spacing.item_x;
    if tabs.len() > 1 {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = t.spacing.item_x / 2.0;
            for (tab, label) in &tabs {
                if tab_button(ui, t, label.get(lang), st.tab == *tab).clicked() && st.tab != *tab {
                    st.tab = *tab;
                    query.clear();
                    st.jump = None;
                }
            }
            // En vocal : « Sons » ouvre le tableau de sons, qui a sa fenêtre.
            if src.sounds && tab_button(ui, t, crate::i18n::table::lotv::t::SOUNDS_TAB.get(lang), false).on_hover_text(crate::i18n::table::lotv::t::SOUNDS_TAB_TIP.get(lang)).clicked() {
                st.acts.push(Act::Sounds);
            }
        });
    }
    let hint = match st.tab {
        Tab::Gifs => gt::SEARCH,
        Tab::Stickers => et::SEARCH_STICKER,
        Tab::Emojis => et::SEARCH,
    };
    // À droite du champ : teinte de peau, et super réaction si elle est permise.
    let tone_side = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y).max(t.markdown.picker_emoji);
    let super_width = if src.super_ok { super_size(ui, t, lang).x + t.spacing.item_x } else { 0.0 };
    let trailing = if st.tab == Tab::Emojis { tone_side + super_width } else { 0.0 };
    let (enter, changed) = search_row(ui, t, hint.get(lang), query, trailing, |ui| {
        if st.tab != Tab::Emojis {
            return;
        }
        if src.super_ok && super_toggle(ui, t, lang, st.super_mode).clicked() {
            st.super_mode = !st.super_mode;
        }
        if let Some(hand) = table::by_name("raised_hand").or_else(|| table::by_name("wave")) {
            let (rect, r) = ui.allocate_exact_size(egui::Vec2::splat(tone_side), Sense::click());
            if r.hovered() || r.has_focus() {
                ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
            }
            let side = t.markdown.picker_emoji * 0.75;
            emoji::paint(ui, Rect::from_center_size(rect.center(), egui::Vec2::splat(side)), Img::Unicode(hand.with_tone(st.tone)));
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, et::TONE.get(lang)));
            if r.on_hover_text(et::TONE.get(lang)).clicked() {
                st.tone = (st.tone + 1) % (TONES + 1);
            }
        }
    });
    let mut height = grid_height(ui.ctx(), t);
    // Fenêtre basse : la grille rend ce qui manque (une rangée au moins reste).
    if let Some(room) = st.room.take() {
        let chrome = if st.chrome > 0.0 { st.chrome } else { cell(t) * 3.0 };
        height = height.min((room - chrome).max(cell(t)));
    }
    let (out, grid) = match st.tab {
        Tab::Emojis => {
            let burst = src.super_ok && st.super_mode;
            (emoji_grid(ui, t, lang, st, query, src, images, enter, height).map(|c| Choice { burst, ..c }), height)
        }
        Tab::Stickers => (sticker_grid(ui, t, lang, query, src, enter, height + cell(t)), height + cell(t)),
        Tab::Gifs => {
            let gifs = src.gifs?;
            let mut acts = Vec::new();
            let picked = gif_picker::body(ui, t, lang, images, gifs, query, changed, enter, height + cell(t), &mut acts);
            st.acts.extend(acts.into_iter().map(Act::Gif));
            (picked.map(|(id, url)| Choice { pick: Pick::Gif { id, url }, keep_open: false, burst: false }), height + cell(t))
        }
    };
    // Le reste du sélecteur compte la grille pour `height` : les onglets plus hauts d'une cellule aussi.
    st.chrome = (ui.min_rect().bottom() - top - grid).max(0.0) + (grid - height);
    out
}

fn super_size(ui: &egui::Ui, t: &Tokens, lang: Lang) -> egui::Vec2 {
    let font = super::fonts::demi_font(t.typography.small);
    let text = ui.painter().layout_no_wrap(gt::SUPER.get(lang).to_string(), font, egui::Color32::PLACEHOLDER).size();
    egui::vec2(text.x + 2.0 * t.spacing.control_x, Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y))
}

/// Interrupteur « Super réaction » : pastille au contour de la couleur des super réactions.
fn super_toggle(ui: &mut egui::Ui, t: &Tokens, lang: Lang, on: bool) -> egui::Response {
    let label = gt::SUPER.get(lang);
    let (rect, resp) = ui.allocate_exact_size(super_size(ui, t, lang), Sense::click());
    let fill = if on { t.colors.link.linear_multiply(0.25) } else if resp.hovered() || resp.has_focus() { t.colors.row_hover } else { t.colors.bg3 };
    let stroke = if on { t.colors.link } else { t.colors.border };
    ui.painter().rect(rect, t.r_row(), fill, egui::Stroke::new(t.shapes.border.max(1.0), stroke), egui::StrokeKind::Inside);
    let color = if on { t.colors.text_strong } else { t.colors.muted };
    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, label, super::fonts::demi_font(t.typography.small), color);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, on, label));
    resp.on_hover_text(gt::SUPER_TIP.get(lang))
}

/// Étoile pleine à cinq branches inscrite dans `rect`.
pub(super) fn paint_star(p: &egui::Painter, rect: Rect, fill: egui::Color32, stroke: egui::Stroke) {
    // Pleine : celle du jeu du thème ; le contour seul (« pas encore favori ») reste ici.
    if stroke.width <= 0.0 {
        return icons::paint(Icon::Star, p, rect, fill);
    }
    let c = rect.center();
    let outer = rect.width().min(rect.height()) * 0.48;
    let inner = outer * 0.42;
    let pt = |i: usize| {
        let r = if i.is_multiple_of(2) { outer } else { inner };
        let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
        c + egui::vec2(a.cos(), a.sin()) * r
    };
    if fill != egui::Color32::TRANSPARENT {
        // Un pentagone et cinq pointes : chaque morceau est convexe.
        p.add(egui::Shape::convex_polygon((0..5).map(|k| pt(2 * k + 1)).collect(), fill, egui::Stroke::NONE));
        for k in 0..5 {
            p.add(egui::Shape::convex_polygon(vec![pt(2 * k), pt(2 * k + 1), pt((2 * k + 9) % 10)], fill, egui::Stroke::NONE));
        }
    }
    if stroke.width > 0.0 {
        p.add(egui::Shape::closed_line((0..10).map(pt).collect(), stroke));
    }
}

/// Horloge (emojis fréquents) inscrite dans `rect`.
fn paint_clock(p: &egui::Painter, rect: Rect, color: egui::Color32) {
    icons::paint(Icon::Clock, p, rect, color);
}

/// Chevron d'en-tête : vers le bas (dépliée) ou vers la droite (repliée).
fn paint_chevron(p: &egui::Painter, rect: Rect, open: bool, color: egui::Color32) {
    icons::paint(if open { Icon::ChevronDown } else { Icon::ChevronRight }, p, rect, color);
}

/// Pictogramme d'une section : étoile, horloge, icône du serveur, ou le
/// premier emoji de la catégorie.
fn section_icon(ui: &egui::Ui, t: &Tokens, images: &Images, rect: Rect, s: &Section, color: egui::Color32) {
    match (s.key, s.guild) {
        (SecKey::Favorites, _) => paint_star(ui.painter(), rect, color, egui::Stroke::NONE),
        (SecKey::Frequent, _) => paint_clock(ui.painter(), rect, color),
        (_, Some(g)) => {
            let r = egui::CornerRadius::same((rect.width() / 2.0) as u8);
            if !super::util::paint_image(ui, images, rect, g.icon, r) {
                ui.painter().rect_filled(rect, r, t.colors.bg3);
                let initial: String = g.name.chars().find(|c| c.is_alphanumeric()).map(|c| c.to_uppercase().collect()).unwrap_or_else(|| "?".into());
                ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, initial, egui::FontId::proportional(Tokens::initials_size(rect.width())), t.colors.text);
            }
        }
        _ => {
            if let Some((img, _, _)) = s.items.get(0).and_then(|it| it.resolve(0)) {
                emoji::paint(ui, rect, img);
            }
        }
    }
}

/// Rail des sections, à gauche de la grille.
fn rail(ui: &mut egui::Ui, t: &Tokens, st: &mut EmojiUi, sections: &[Section], images: &Images, height: f32) {
    let c = cell(t);
    let row = (c * 5.0 / 6.0).round();
    egui::ScrollArea::vertical().id_salt("rail-emojis").max_width(c).max_height(height).min_scrolled_height(height).auto_shrink([false, false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(ui, |ui| {
        ui.set_width(c);
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        let mut ruled = false;
        for (i, s) in sections.iter().enumerate() {
            // Un filet entre les serveurs et les catégories de Discord.
            if matches!(s.key, SecKey::Cat(_)) && !ruled {
                ruled = true;
                let (line, _) = ui.allocate_exact_size(egui::vec2(c, t.spacing.item_x), Sense::hover());
                ui.painter().hline(line.shrink2(egui::vec2(c * 0.2, 0.0)).x_range(), line.center().y, egui::Stroke::new(t.shapes.border.max(1.0), t.colors.separator));
            }
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(c, row), Sense::click());
            let active = st.current == i;
            let hot = resp.hovered() || resp.has_focus();
            let pad = Rect::from_center_size(rect.center(), egui::Vec2::splat(row));
            if active {
                ui.painter().rect_filled(pad, t.r_row(), t.colors.selected);
            } else if hot {
                ui.painter().rect_filled(pad, t.r_row(), t.colors.row_hover);
            }
            let side = if s.guild.is_some() { c * 2.0 / 3.0 } else { c / 2.0 };
            let color = if active || hot { t.colors.text_strong } else { t.colors.muted };
            section_icon(ui, t, images, super::util::pixel_square(ui, rect.center() - egui::Vec2::splat(side / 2.0), side), s, color);
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &s.title));
            if resp.on_hover_text(&s.title).clicked() {
                st.jump = Some(i);
                st.collapsed.remove(&s.key);
            }
        }
    });
}

/// Bandeau du bas : l'emoji survolé, son nom et d'où il vient.
fn banner(ui: &mut egui::Ui, t: &Tokens, lang: Lang, hover: Option<&Hover>) {
    let c = cell(t);
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), c), Sense::hover());
    // Le bandeau va jusqu'aux bords du cadre du sélecteur.
    let m = t.embed_margin();
    let full = Rect::from_min_max(rect.min - egui::vec2(f32::from(m.left), 0.0), rect.max + egui::vec2(f32::from(m.right), f32::from(m.bottom)));
    let r = t.r_card();
    ui.painter().rect_filled(full, egui::CornerRadius { nw: 0, ne: 0, sw: r.sw, se: r.se }, t.colors.bg1);
    let mid = full.center().y;
    let Some(h) = hover else {
        ui.painter().text(egui::pos2(rect.left(), mid), egui::Align2::LEFT_CENTER, gt::BANNER_TIP.get(lang), egui::FontId::proportional(t.typography.small), t.colors.faint);
        return;
    };
    let side = (c * 7.0 / 12.0).round();
    let at = super::util::pixel_square(ui, egui::pos2(rect.left(), mid - side / 2.0), side);
    emoji::paint(ui, at, h.img);
    let left = at.right() + t.spacing.item_x;
    let origin = ui.painter().layout_no_wrap(h.origin.clone(), egui::FontId::proportional(t.typography.small), t.colors.muted);
    let origin_w = origin.size().x.min(rect.width() * 0.45);
    let room = (rect.right() - left - origin_w - t.spacing.item_x).max(0.0);
    let name = ui.painter().layout(format!(":{}:", h.name), super::fonts::demi_font(t.typography.body), t.colors.text_strong, f32::INFINITY);
    let clip = Rect::from_min_max(egui::pos2(left, full.top()), egui::pos2(left + room, full.bottom()));
    ui.painter().with_clip_rect(clip.intersect(ui.clip_rect())).galley(egui::pos2(left, mid - name.size().y / 2.0), name, t.colors.text_strong);
    let clip = Rect::from_min_max(egui::pos2(rect.right() - origin_w, full.top()), egui::pos2(rect.right(), full.bottom()));
    ui.painter().with_clip_rect(clip.intersect(ui.clip_rect())).galley(egui::pos2(rect.right() - origin_w, mid - origin.size().y / 2.0), origin, t.colors.muted);
}

#[allow(clippy::too_many_arguments)]
fn emoji_grid(ui: &mut egui::Ui, t: &Tokens, lang: Lang, st: &mut EmojiUi, query: &str, src: &Sources, images: &Images, enter: bool, height: f32) -> Option<Choice> {
    // Les récents sortent de l'état le temps de l'affichage : les sections les empruntent.
    let recents = std::mem::take(&mut st.recents);
    let chosen = emoji_sections(ui, t, lang, st, query, src, images, enter, height, &recents);
    st.recents = recents;
    chosen
}

#[allow(clippy::too_many_arguments)]
fn emoji_sections(ui: &mut egui::Ui, t: &Tokens, lang: Lang, st: &mut EmojiUi, query: &str, src: &Sources, images: &Images, enter: bool, height: f32, recents: &VecDeque<Emoji>) -> Option<Choice> {
    let sections = sections(lang, query, recents, src);
    let tone = st.tone;
    if enter && let Some(first) = sections.iter().filter(|s| !s.locked).find_map(|s| s.items.get(0)).and_then(|i| i.resolve(tone)) {
        return Some(Choice { pick: Pick::Emoji(first.2), keep_open: false, burst: false });
    }
    let c = cell(t);
    let head = head_height(t);
    let side = emoji_side(t);
    let searching = !query.trim().is_empty();
    let mut chosen = None;
    let mut hovered: Option<Hover> = None;
    let empty = sections.iter().all(|s| s.items.len() == 0);
    ui.horizontal_top(|ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        if !searching {
            // Colonne à part : la zone défilante du rail ne prend que sa largeur.
            ui.allocate_ui_with_layout(egui::vec2(c, height), egui::Layout::top_down(egui::Align::Min), |ui| rail(ui, t, st, &sections, images, height));
        }
        let width = ui.available_width();
        let cols = (((width - t.spacing.item_x) / c).floor() as usize).clamp(1, COLS);
        let (placed, total) = place(&sections, cols, c, head, &st.collapsed);
        if empty {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(width, height), Sense::hover());
            ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, et::NONE_FOUND.get(lang), egui::FontId::proportional(t.typography.body), t.colors.muted);
            return;
        }
        let mut scroll = egui::ScrollArea::vertical().id_salt("grille-emojis").max_width(width).max_height(height).min_scrolled_height(height).auto_shrink([false, false]);
        if let Some(i) = st.jump.take()
            && let Some(p) = placed.get(i)
        {
            scroll = scroll.vertical_scroll_offset(p.top);
        }
        let ready = src.frecency.ready();
        scroll.show_viewport(ui, |ui, view| {
            let (full, _) = ui.allocate_exact_size(egui::vec2(width, total.max(height)), Sense::hover());
            let origin = full.min;
            st.current = placed.iter().rposition(|p| p.top <= view.min.y + 1.0).unwrap_or(0);
            for (si, (sec, p)) in sections.iter().zip(&placed).enumerate() {
                if !p.open || p.bottom(c, head) < view.min.y || p.top > view.max.y {
                    continue;
                }
                let rows_top = p.top + head;
                let first = ((view.min.y - rows_top) / c).floor().max(0.0) as usize;
                let last = (((view.max.y - rows_top) / c).ceil().max(0.0) as usize).min(p.rows);
                for r in first..last {
                    for k in 0..cols {
                        let index = r * cols + k;
                        let Some(item) = sec.items.get(index) else { break };
                        let Some((img, name, e)) = item.resolve(tone) else { continue };
                        let rect = Rect::from_min_size(origin + egui::vec2(k as f32 * c, rows_top + r as f32 * c), egui::Vec2::splat(c));
                        // Réservé à Nitro (F26) : grisé, expliqué au survol, sans effet au clic.
                        if sec.locked {
                            let resp = ui.interact(rect, ui.scope_id().with((si, index)), Sense::hover());
                            emoji::paint(ui, super::util::pixel_square(ui, rect.center() - egui::Vec2::splat(side / 2.0), side), img);
                            ui.painter().rect_filled(rect, 0.0, t.colors.bg2.gamma_multiply(0.65));
                            if resp.hovered() {
                                hovered = Some(Hover { img, name: name.clone(), origin: crate::i18n::table::ff::t::EMOJI_LOCKED.get(lang).to_string() });
                            }
                            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, false, format!(":{name}:")));
                            resp.on_hover_text(crate::i18n::table::ff::t::EMOJI_LOCKED_TIP.get(lang));
                            continue;
                        }
                        let resp = ui.interact(rect, ui.scope_id().with((si, index)), Sense::click());
                        if resp.hovered() || resp.has_focus() {
                            ui.painter().rect_filled(rect, t.r_row(), t.colors.hover);
                            let origin = match &e {
                                Emoji::Custom { id, .. } => src.custom(*id).map_or_else(String::new, |(g, _)| g.name.clone()),
                                Emoji::Unicode(_) => gt::UNICODE_ORIGIN.get(lang).to_string(),
                            };
                            hovered = Some(Hover { img, name: name.clone(), origin });
                        }
                        emoji::paint(ui, super::util::pixel_square(ui, rect.center() - egui::Vec2::splat(side / 2.0), side), img);
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!(":{name}:")));
                        if resp.clicked() {
                            chosen = Some(Choice { pick: Pick::Emoji(e.clone()), keep_open: ui.input(|i| i.modifiers.shift), burst: false });
                        }
                        // Favoris du compte : seulement une fois reçus.
                        if ready && let Some(key) = emoji_key(&e) {
                            let label = if src.frecency.is_fav_emoji(&key) { gt::REMOVE_FAVORITE } else { gt::ADD_FAVORITE };
                            resp.context_menu(|ui| {
                                if crate::ui::menu::item(ui, t, label.get(lang), Some(Icon::Star), false).clicked() {
                                    st.acts.push(Act::FavEmoji(key.clone()));
                                    ui.close();
                                }
                            });
                        }
                    }
                }
            }
            // En-têtes par-dessus les cellules : celui de la section du haut
            // reste collé au bord tant que sa section est visible.
            for (si, (sec, p)) in sections.iter().zip(&placed).enumerate() {
                let bottom = p.bottom(c, head);
                if bottom < view.min.y || p.top > view.max.y {
                    continue;
                }
                let y = view.min.y.clamp(p.top, (bottom - head).max(p.top));
                let rect = Rect::from_min_size(origin + egui::vec2(0.0, y), egui::vec2(width, head));
                let foldable = sec.key != SecKey::Results;
                let resp = ui.interact(rect, ui.scope_id().with(("en-tête", si)), if foldable { Sense::click() } else { Sense::hover() });
                ui.painter().rect_filled(rect, 0.0, t.colors.bg2);
                let color = if resp.hovered() { t.colors.text } else { t.colors.muted };
                let mut x = rect.left() + t.spacing.item_x / 2.0;
                if matches!(sec.key, SecKey::Favorites | SecKey::Frequent) || sec.guild.is_some() {
                    let icon = t.typography.small * 4.0 / 3.0;
                    section_icon(ui, t, images, super::util::pixel_square(ui, egui::pos2(x, rect.center().y - icon / 2.0), icon), sec, color);
                    x += icon + t.spacing.item_x / 2.0;
                }
                let title = ui.painter().layout_no_wrap(sec.title.to_uppercase(), super::fonts::demi_font(t.typography.small), color);
                let after = x + title.size().x.min(width * 0.75);
                let clip = Rect::from_min_max(rect.min, egui::pos2(after, rect.bottom()));
                ui.painter().with_clip_rect(clip.intersect(ui.clip_rect())).galley(egui::pos2(x, rect.center().y - title.size().y / 2.0), title, color);
                if foldable {
                    let chevron = Rect::from_center_size(egui::pos2(after + head / 2.0, rect.center().y), egui::Vec2::splat(head * 0.6));
                    paint_chevron(ui.painter(), chevron, p.open, color);
                    let tip = if p.open { gt::FOLD } else { gt::UNFOLD };
                    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &sec.title));
                    if resp.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(tip.get(lang)).clicked() && !st.collapsed.remove(&sec.key) {
                        st.collapsed.insert(sec.key);
                    }
                }
            }
        });
    });
    if hovered.is_some() {
        st.hover = hovered;
    }
    banner(ui, t, lang, st.hover.as_ref());
    chosen
}

fn sticker_grid(ui: &mut egui::Ui, t: &Tokens, lang: Lang, query: &str, src: &Sources, enter: bool, height: f32) -> Option<Choice> {
    let q = query.trim().to_lowercase();
    let list: Vec<&GuildSticker> = src.stickers.iter().copied().filter(|s| q.is_empty() || s.name.to_lowercase().contains(&q)).collect();
    if enter && let Some(s) = list.first() {
        return Some(Choice { pick: Pick::Sticker(s.item()), keep_open: false, burst: false });
    }
    if list.is_empty() {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), height), Sense::hover());
        ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, et::NO_STICKERS.get(lang), egui::FontId::proportional(t.typography.body), t.colors.muted);
        return None;
    }
    let width = ui.available_width();
    // Quatre autocollants par rangée au moins, la largeur partagée entre eux.
    let cols = (((width - t.spacing.item_x) / (t.markdown.sticker * STICKER_CELL + t.spacing.item_x)).floor() as usize).max(1);
    let cell = ((width - t.spacing.item_x) / cols as f32).floor();
    let rows = list.len().div_ceil(cols);
    let mut chosen = None;
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        egui::ScrollArea::vertical().id_salt("grille-autocollants").max_height(height).min_scrolled_height(height).auto_shrink([false, false]).show_rows(ui, cell, rows, |ui, range| {
            for r in range {
                ui.horizontal(|ui| {
                    for s in list.iter().skip(r * cols).take(cols) {
                        let (rect, resp) = ui.allocate_exact_size(egui::vec2(cell, cell), Sense::click());
                        if resp.hovered() || resp.has_focus() {
                            ui.painter().rect_filled(rect, t.r_row(), t.colors.hover);
                        }
                        super::stickers::paint(ui, t, rect.shrink(t.spacing.item_x / 2.0), &s.item());
                        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &s.name));
                        if resp.on_hover_text(&s.name).clicked() {
                            chosen = Some(Choice { pick: Pick::Sticker(s.item()), keep_open: false, burst: false });
                        }
                    }
                });
            }
        });
    });
    chosen
}

/// Texte inséré dans la saisie pour un emoji, et le lien « texte lisible →
/// syntaxe » d'un emoji personnalisé (comme l'autocomplétion).
pub(super) fn insertion(e: &Emoji) -> (String, Option<(String, String)>) {
    match e {
        Emoji::Unicode(s) => match table::shortcode(s) {
            Some(n) => (format!(":{n}:"), None),
            None => (s.clone(), None),
        },
        Emoji::Custom { id, name, animated } => {
            let shown = format!(":{name}:");
            let a = if *animated { "a" } else { "" };
            (shown.clone(), Some((shown, format!("<{a}:{name}:{id}>"))))
        }
    }
}

/// Format d'autocollant affichable en image.
pub(super) fn sticker_img(s: &StickerItem) -> Option<Img> {
    Some(match s.format {
        StickerFormat::Lottie => Img::Lottie { id: s.id },
        f => Img::Sticker { id: s.id, gif: f == StickerFormat::Gif, anim: matches!(f, StickerFormat::Gif | StickerFormat::Apng) },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::frecency::synthetic;

    fn sources<'a>(frecency: &'a Frecency, servers: Vec<(&'a model::Guild, Vec<&'a model::Emoji>)>) -> Sources<'a> {
        Sources { servers, locked: Vec::new(), stickers: Vec::new(), frecency, gifs: None, super_ok: false, sounds: false }
    }

    #[test]
    fn recents_are_bounded_and_deduplicated() {
        let mut st = EmojiUi::default();
        for i in 0..(MAX_RECENTS as u64 + 5) {
            st.used(&Emoji::Custom { id: i, name: format!("e{i}"), animated: false });
        }
        st.used(&Emoji::Custom { id: 40, name: "renommé".into(), animated: false });
        assert_eq!(st.recents.len(), MAX_RECENTS);
        assert!(matches!(&st.recents[0], Emoji::Custom { id: 40, .. }));
        assert_eq!(st.recents.iter().filter(|e| matches!(e, Emoji::Custom { id: 40, .. })).count(), 1);
    }

    #[test]
    fn sections_and_placement() {
        let recents = VecDeque::new();
        let none = Frecency::default();
        let src = sources(&none, Vec::new());
        let s = sections(Lang::Fr, "", &recents, &src);
        assert_eq!(s.len(), 1 + CATEGORIES.len(), "sans favoris : pas de section vide");
        assert_eq!((s[0].key, s[0].items.len()), (SecKey::Frequent, DEFAULTS.len()));
        let (placed, total) = place(&s, 9, 48.0, 32.0, &HashSet::new());
        assert_eq!(placed[0], Placed { top: 0.0, rows: 2, open: true });
        assert_eq!(placed[1].top, 32.0 + 2.0 * 48.0);
        assert!(total > 2000.0 / 9.0 * 48.0);
        // Repliée : seul l'en-tête reste, les suivantes remontent.
        let folded: HashSet<SecKey> = [SecKey::Frequent].into();
        let (placed, _) = place(&s, 9, 48.0, 32.0, &folded);
        assert_eq!((placed[0].open, placed[1].top), (false, 32.0));
        let r = sections(Lang::Fr, ":thumbs", &recents, &src);
        assert_eq!(r.len(), 1);
        let first = r[0].items.get(0).and_then(|i| i.resolve(3)).unwrap_or_else(|| panic!("résultat"));
        assert_eq!(first.1, "thumbsup_tone3");
        assert_eq!(first.2, Emoji::Unicode("👍🏽".into()));
        // Les résultats ne se replient pas.
        let (placed, _) = place(&r, 9, 48.0, 32.0, &[SecKey::Results].into());
        assert!(placed[0].open);
    }

    #[test]
    fn favorites_and_frequent_come_from_the_account_then_the_session() {
        let mut f = Frecency::default();
        f.open();
        // 501 est un emoji du serveur ; 999 n'est pas utilisable ici : écarté.
        f.received(Some(synthetic(&[], &["fire", "501", "999", "pas_un_emoji"], &[("joy", 50), ("501", 90), ("tada", 10)], 1)), false);
        let guild = model::Guild::new(1, "Alpha".into(), Vec::new());
        let custom = model::Emoji { id: 501, name: "ferris".into(), animated: false, available: true, roles: Vec::new() };
        let src = sources(&f, vec![(&guild, vec![&custom])]);
        let mut recents = VecDeque::new();
        recents.push_front(Emoji::Unicode("😂".into()));
        let s = sections(Lang::En, "", &recents, &src);
        assert_eq!(s.iter().map(|x| x.key).take(3).collect::<Vec<_>>(), [SecKey::Favorites, SecKey::Frequent, SecKey::Guild(1)]);
        let names = |sec: &Section| (0..sec.items.len()).filter_map(|i| sec.items.get(i)?.resolve(0)).map(|r| r.1).collect::<Vec<_>>();
        assert_eq!(names(&s[0]), ["fire", "ferris"]);
        // La session d'abord (joy), sans doublon avec le compte, puis par note.
        assert_eq!(names(&s[1]), ["joy", "ferris", "tada"]);
        assert_eq!(src.custom(501).map(|(g, _)| g.name.as_str()), Some("Alpha"));
    }

    #[test]
    fn insertion_texts() {
        assert_eq!(insertion(&Emoji::Unicode("🔥".into())), (":fire:".to_string(), None));
        let (text, link) = insertion(&Emoji::Custom { id: 7, name: "blob".into(), animated: true });
        assert_eq!(text, ":blob:");
        assert_eq!(link, Some((":blob:".into(), "<a:blob:7>".into())));
    }

    /// Le sélecteur de réaction affiche sa grille, annonce son ouverture une
    /// seule fois, et ne demande que les images visibles.
    #[test]
    fn grid_is_virtualized_and_announces_once() {
        let ctx = egui::Context::default();
        let t = Tokens::default();
        let images = Images::disabled(ctx.clone());
        let none = Frecency::default();
        let mut st = EmojiUi::default();
        let mut query = String::new();
        let mut asked = 0;
        for _ in 0..3 {
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(900.0, 700.0))), ..Default::default() };
            let mut out = ctx.run_ui(input, |ui| {
                let src = sources(&none, Vec::new());
                ui.set_width(width(ui.ctx(), &t));
                assert!(body(ui, &t, Lang::Fr, &mut st, &mut query, &src, &images).is_none());
            });
            out.textures_delta.clear();
            asked = emoji::take_queued(&ctx).len();
        }
        assert_eq!(st.take_acts(), [Act::Opened(Tab::Emojis)]);
        // Sept rangées de neuf au plus, plus le rail et la teinte : bien moins que la table.
        assert!(asked > COLS && asked < 120, "{asked}");
    }
}
