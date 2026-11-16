//! Recherche de messages (lot T12) : champ de l'en-tête avec ses filtres et
//! leur autocomplétion, panneau des résultats à droite (liste virtualisée,
//! rendue par les modules des messages), saut vers un message (résultat,
//! extrait de réponse, épingle, lien `discord.com/channels/…`).
//!
//! Aucun réseau ici : tout passe par le magasin (`state::search`,
//! `Store::jump_to_message`). Les textes des résultats sont analysés une
//! fois par page reçue, les mots cherchés surlignés à ce moment-là.

use super::message::message_ui_acts;
use super::msg_actions::{Action, Acts, ActionsUi, Hit};
use super::util::{icon_button, inline_icon};
use super::vlist::Geometry;
use super::{words, App};
use crate::i18n::table::ff::t as ft;
use crate::i18n::table::search::{f, p, t};
use crate::i18n::{t as tx, Lang};
use crate::state::markdown::{parse_message, Doc};
use crate::state::model::{snowflake_ms, Message};
use crate::state::search::{self as s, AuthorType, Filter, Has, Names, ParseError, Phase, Scope, Sort};
use crate::state::store::Jump;
use crate::theme::{icons::Icon, Clock, Tokens};
use eframe::egui::{self, Id, Key, Modifiers, RichText};
use std::sync::Arc;

/// Bit ajouté à l'identifiant d'un message dessiné dans le panneau : ses
/// zones cliquables (identifiants d'egui tirés de l'identifiant du message)
/// ne se confondent pas avec celles du même message dans la liste du salon.
/// Les snowflakes tiennent sur 63 bits.
const UI_SALT: u64 = 1 << 63;
/// Propositions affichées au plus.
const MAX_SUGGESTIONS: usize = 8;
/// Champ de recherche de l'en-tête, comme dans le client : 244 de large au plus, 32 de haut.
const FIELD_WIDTH: f32 = 244.0;
const FIELD_HEIGHT: f32 = 32.0;
/// Noms choisis dans l'autocomplétion gardés au plus (les plus anciens partent).
const MAX_PICKED: usize = 32;

/// Choix retenu par une proposition (nom → identifiant, jamais deviné ensuite).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Pick {
    Person(String, u64),
    Channel(String, u64),
}

/// Une proposition : ce qui est montré, et le texte du champ une fois choisie.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Suggestion {
    label: String,
    detail: Option<&'static str>,
    text: String,
    pick: Option<Pick>,
}

/// Largeur du panneau de résultats quand il se superpose à la discussion.
const FLOAT_WIDTH: f32 = 320.0;

/// État de la recherche dans l'interface (appartient à `App`).
#[derive(Default)]
pub(super) struct SearchUi {
    pub(super) text: String,
    /// Mode « Local » (plugin Recherche locale, lot E1) : rien n'est demandé à Discord.
    pub(super) local: bool,
    picked_people: Vec<(String, u64)>,
    picked_channels: Vec<(String, u64)>,
    /// Proposition en surbrillance.
    selected: Option<usize>,
    /// Propositions masquées (Échap, recherche lancée) jusqu'à la prochaine frappe.
    pub(super) hidden: bool,
    /// Texte de l'image précédente (une frappe remet la sélection à zéro).
    last_text: String,
    /// Résultats préparés : message à identifiant marqué (`UI_SALT`) et texte mis en forme.
    prepared: Vec<(Message, Arc<Doc>)>,
    prepared_key: Option<(u64, Lang, bool)>,
    geo: Geometry,
    geo_key: Option<(u64, u32)>,
    /// Remonter en haut des résultats (nouvelle page).
    pub(super) to_top: bool,
    /// Compte pour lequel cet état vaut.
    me: Option<u64>,
    /// En-tête serré : la loupe a été cliquée, le champ se déplie et prend le clavier.
    expand: bool,
    /// Recherches récentes de la session (F16), la plus récente en premier.
    recent: Vec<String>,
}

/// Recherches récentes gardées au plus.
const MAX_RECENT: usize = 6;

impl SearchUi {
    fn remember(&mut self, pick: Pick) {
        let (list, name, id) = match pick {
            Pick::Person(n, id) => (&mut self.picked_people, n, id),
            Pick::Channel(n, id) => (&mut self.picked_channels, n, id),
        };
        list.retain(|(n, _)| *n != name);
        list.push((name, id));
        if list.len() > MAX_PICKED {
            list.remove(0);
        }
    }

    /// Retient une recherche lancée (F16) : sans doublon, la plus récente en tête.
    fn remember_search(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.recent.retain(|r| r != text);
        self.recent.insert(0, text.to_string());
        self.recent.truncate(MAX_RECENT);
    }
}

/// Minuscules sans accents, pour classer les propositions.
fn fold(s: &str) -> String {
    s.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'à' | 'â' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'î' | 'ï' => 'i',
            'ô' | 'ö' => 'o',
            'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Rang d'un nom pour `needle` (déjà replié) : début, puis n'importe où.
fn rank(name: &str, needle: &str) -> Option<u8> {
    let name = fold(name);
    if name.starts_with(needle) {
        Some(0)
    } else if name.contains(needle) {
        Some(1)
    } else {
        None
    }
}

/// Valeur insérée après `clé:` : entre guillemets si elle contient un blanc.
fn quoted(v: &str) -> String {
    if v.contains(char::is_whitespace) { format!("\"{v}\"") } else { v.to_string() }
}

fn filter_detail(f: Filter, lang: Lang) -> &'static str {
    let text = match f {
        Filter::From | Filter::Mentions => t::FILTER_USER,
        Filter::Has => t::FILTER_HAS,
        Filter::In => t::FILTER_CHANNEL,
        Filter::Before => t::FILTER_BEFORE,
        Filter::After => t::FILTER_AFTER,
        Filter::During => t::FILTER_DURING,
        Filter::Pinned => t::FILTER_PINNED,
        Filter::AuthorType => t::FILTER_AUTHOR_TYPE,
    };
    text.get(lang)
}

/// Propositions pour le texte tapé (le mot en cours est le dernier) ; vrai
/// si la première doit être en surbrillance (Entrée la choisit).
fn suggest(text: &str, lang: Lang, names: &Names, today: &str) -> (Vec<Suggestion>, bool) {
    let toks = s::tokens(text);
    let typing = toks.last().filter(|tk| tk.end == text.len());
    let filters = |keep: &dyn Fn(Filter) -> bool, head: &str| -> Vec<Suggestion> {
        Filter::ALL
            .into_iter()
            .filter(|f| keep(*f) && !(names.private && *f == Filter::In))
            .map(|f| Suggestion { label: format!("{}:", f.key(lang)), detail: Some(filter_detail(f, lang)), text: format!("{head}{}:", f.key(lang)), pick: None })
            .collect()
    };
    let Some(tk) = typing else {
        return (filters(&|_| true, text), false);
    };
    let head = &text[..tk.start];
    let Some(filter) = tk.filter else {
        // Début d'un mot-clé : les filtres qui commencent ainsi.
        let word = fold(&tk.value);
        if word.contains(['"', ':']) {
            return (Vec::new(), false);
        }
        let list = filters(&|f| fold(f.key(lang)).starts_with(&word) || Lang::ALL.iter().any(|l| fold(f.key(*l)).starts_with(&word)), head);
        return (list.into_iter().take(MAX_SUGGESTIONS).collect(), false);
    };
    let key = filter.key(lang);
    let needle = fold(&tk.value);
    let value = |label: &str, detail: Option<&'static str>, pick: Option<Pick>| Suggestion {
        label: label.to_string(),
        detail,
        text: format!("{head}{key}:{} ", quoted(label)),
        pick,
    };
    let mut ranked: Vec<(u8, Suggestion)> = match filter {
        Filter::From | Filter::Mentions => names
            .people
            .iter()
            .filter(|(_, n)| !n.contains('"'))
            .filter_map(|(id, n)| rank(n, &needle).map(|r| (r, value(n, None, Some(Pick::Person(n.clone(), *id))))))
            .collect(),
        Filter::In => names
            .channels
            .iter()
            .filter(|(_, n)| !n.contains('"'))
            .filter_map(|(id, n)| rank(n, &needle).map(|r| (r, value(n, None, Some(Pick::Channel(n.clone(), *id))))))
            .collect(),
        Filter::Has => Has::ALL.iter().filter_map(|h| rank(h.word(lang), &needle).map(|r| (r, value(h.word(lang), None, None)))).collect(),
        Filter::AuthorType => AuthorType::ALL.iter().filter_map(|a| rank(a.word(lang), &needle).map(|r| (r, value(a.word(lang), None, None)))).collect(),
        Filter::Pinned => [t::YES, t::NO].iter().filter_map(|w| rank(w.get(lang), &needle).map(|r| (r, value(w.get(lang), None, None)))).collect(),
        Filter::Before | Filter::After | Filter::During => {
            if today.starts_with(&tk.value) {
                vec![(0, value(today, Some(t::TODAY.get(lang)), None))]
            } else {
                Vec::new()
            }
        }
    };
    ranked.sort_by_key(|(r, _)| *r);
    let list: Vec<Suggestion> = ranked.into_iter().take(MAX_SUGGESTIONS).map(|(_, s)| s).collect();
    let pre = !list.is_empty();
    (list, pre)
}

/// Texte d'une erreur d'analyse.
pub(super) fn error_text(lang: Lang, e: &ParseError) -> String {
    match e {
        ParseError::Empty => t::EMPTY_QUERY.get(lang).to_string(),
        ParseError::TooLong => t::TOO_LONG.get(lang).to_string(),
        ParseError::NoChannels => t::NO_CHANNELS_HERE.get(lang).to_string(),
        ParseError::UnknownUser(n) => f::unknown_user(lang, n),
        ParseError::AmbiguousUser(n) => f::ambiguous_user(lang, n),
        ParseError::UnknownChannel(n) => f::unknown_channel(lang, n),
        ParseError::AmbiguousChannel(n) => f::ambiguous_channel(lang, n),
        ParseError::BadDate(v) => f::bad_date(lang, v),
        ParseError::BadValue { filter, value } => f::bad_value(lang, filter.key(lang), value),
    }
}

fn sort_label(s: Sort, lang: Lang) -> &'static str {
    match s {
        Sort::Newest => t::SORT_NEWEST.get(lang),
        Sort::Oldest => t::SORT_OLDEST.get(lang),
        Sort::Relevance => t::SORT_RELEVANCE.get(lang),
    }
}

/// Clic sur un lien : un lien de message (`https://discord.com/channels/…`)
/// est ouvert dans Ferrocord à l'image suivante ; vrai s'il l'a été.
pub(super) fn follow_link(ctx: &egui::Context, url: &str) -> bool {
    let Some((_, channel, message)) = s::message_link(url) else { return false };
    ctx.data_mut(|d| d.insert_temp(Id::unique("saut-lien"), (channel, message)));
    ctx.request_repaint();
    true
}

/// Message montré hors de sa liste (`Acts::browse`) : l'extrait de réponse
/// et le message lui-même vont au message visé ; ni barre d'actions, ni menu,
/// ni pastilles cliquables. Renvoie vrai s'il est survolé (voile).
pub(super) fn browse(ui: &mut egui::Ui, m: &Message, acts: &mut Acts, resp: &egui::Response) -> bool {
    for (i, (rect, hit)) in std::mem::take(&mut acts.hits).into_iter().enumerate() {
        if let Hit::Reply(Some(target)) = hit
            && ui.interact(rect, Id::unique(("zone", m.id, i)), egui::Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
        {
            acts.st.push(Action::Jump(target));
        }
    }
    if resp.clone().on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        acts.st.push(Action::Jump(m.id & !UI_SALT));
    }
    resp.hovered()
}

impl App {
    /// Nouvelle recherche (« Entrée » dans le champ) ; sert aussi à l'aperçu
    /// et aux tests. Faux si le texte ne peut pas partir (erreur annoncée).
    pub fn search(&mut self, text: &str) -> bool {
        self.search.text = text.to_string();
        let ok = self.run_search();
        self.flush_commands();
        ok
    }

    pub(super) fn run_search(&mut self) -> bool {
        // Mode « Local » : cherché sur place.
        if self.local_search_wanted() {
            return self.local_search_run(0, None, false);
        }
        let Some(scope) = self.store.search_scope() else { return false };
        let names = self.search_names(scope);
        let sort = self.store.search.active.as_ref().map_or(Sort::Newest, |a| a.query.sort);
        let offset = chrono::Local::now().offset().local_minus_utc();
        match s::parse(&self.search.text, &names, offset, sort) {
            Ok(parsed) => {
                let text = self.search.text.clone();
                self.search.remember_search(&text);
                self.store.start_search(scope, self.search.text.trim().to_string(), parsed);
                self.search.to_top = true;
                self.search.hidden = true;
                true
            }
            Err(e) => {
                let text = error_text(self.lang, &e);
                self.notify(text);
                false
            }
        }
    }

    pub(super) fn search_names(&self, scope: Scope) -> Names {
        let mut names = self.store.search_names(scope);
        names.picked_people.clone_from(&self.search.picked_people);
        names.picked_channels.clone_from(&self.search.picked_channels);
        names
    }

    /// Ouvre le salon du message et l'amène dans la vue, surligné.
    pub(super) fn jump_to_message(&mut self, channel_id: u64, message_id: u64) {
        match self.store.jump_to_message(channel_id, message_id) {
            Jump::Unknown => self.notify(t::UNKNOWN_CHANNEL),
            Jump::Denied => self.notify(t::NO_HISTORY),
            Jump::Shown | Jump::Loading => self.list.jump_to(channel_id, message_id),
        }
        self.flush_commands();
        self.ctx.request_repaint();
    }

    /// À chaque image : changement de compte, lien de message cliqué, saut
    /// dont la page n'est plus attendue (message introuvable).
    pub(super) fn search_tick(&mut self, ctx: &egui::Context) {
        let me = self.store.me.as_ref().map(|u| u.id);
        if self.search.me != me {
            // Autre compte : rien de l'ancien ne reste (pas à la première image).
            if self.search.me.is_some() {
                self.search = SearchUi::default();
            }
            self.search.me = me;
        }
        if let Some((channel, message)) = ctx.data_mut(|d| d.remove_temp::<(u64, u64)>(Id::unique("saut-lien"))) {
            self.jump_to_message(channel, message);
        }
        if let Some((c, id)) = self.list.pending_jump()
            && !self.store.chans.get(&c).is_some_and(|cs| cs.contains(id))
            && !self.store.jump_pending(c)
        {
            self.list.cancel_jump();
            // Un échec déjà annoncé (réseau, refus) n'est pas recouvert.
            if self.toast.is_none() {
                self.notify(t::NOT_FOUND);
            }
        }
    }

    /// Champ de recherche de l'en-tête, avec ses propositions.
    /// Infobulle du champ (et de la loupe qui le remplace dans un en-tête serré).
    pub(super) fn search_tip(&self) -> &'static str {
        t::SEARCH_TIP.get(self.lang)
    }

    /// En-tête serré : le champ doit-il être déplié (il a le clavier, du texte,
    /// ou la loupe vient d'être cliquée) ?
    pub(super) fn search_wanted(&self, ctx: &egui::Context) -> bool {
        self.search.expand || !self.search.text.is_empty() || ctx.memory(|m| m.has_focus(Id::unique("recherche")))
    }

    /// Déplie le champ replié en loupe ; il prend le clavier à l'image suivante.
    pub(super) fn search_expand(&mut self) {
        self.search.expand = true;
        self.ctx.request_repaint();
    }

    /// `row` : largeur de la ligne d'en-tête, dont le champ prend une part.
    pub(super) fn search_field(&mut self, ui: &mut egui::Ui, row: f32) {
        let tk = self.theme.clone();
        let lang = self.lang;
        let Some(scope) = self.store.search_scope() else { return };
        let id = Id::unique("recherche");
        if std::mem::take(&mut self.search.expand) {
            ui.memory_mut(|m| m.request_focus(id));
        }
        let focused = ui.memory(|m| m.has_focus(id));
        let local = self.local_search_wanted();
        let peer = if self.store.guild.is_none() { self.store.channel.and_then(|c| self.dm_person(c)).and_then(|(user, _)| self.store.people.lock().user(user).map(|u| u.username.clone())) } else { None };
        let st = &mut self.search;
        let typed = st.text != st.last_text;
        if typed {
            st.last_text.clone_from(&st.text);
            st.selected = None;
            st.hidden = false;
        }
        let (sugs, pre) = if focused && !st.hidden {
            let today = chrono::Local::now().format("%Y-%m-%d").to_string();
            let mut names = self.store.search_names(scope);
            names.picked_people.clone_from(&st.picked_people);
            names.picked_channels.clone_from(&st.picked_channels);
            suggest(&st.text, lang, &names, &today)
        } else {
            (Vec::new(), false)
        };
        if st.selected.is_some_and(|k| k >= sugs.len()) {
            st.selected = None;
        }
        if pre && st.selected.is_none() {
            st.selected = Some(0);
        }
        let (mut chosen, mut run, mut blur) = (None, false, false);
        if focused {
            ui.input_mut(|i| {
                if !sugs.is_empty() {
                    let n = sugs.len();
                    if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                        st.selected = Some(st.selected.map_or(0, |k| (k + 1) % n));
                    }
                    if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                        st.selected = Some(st.selected.map_or(n - 1, |k| (k + n - 1) % n));
                    }
                    if i.consume_key(Modifiers::NONE, Key::Tab) {
                        chosen = Some(st.selected.unwrap_or(0));
                    }
                }
                if i.consume_key(Modifiers::NONE, Key::Enter) {
                    match st.selected.filter(|_| !sugs.is_empty()) {
                        Some(k) => chosen = Some(k),
                        None => run = true,
                    }
                }
                if i.consume_key(Modifiers::NONE, Key::Escape) {
                    if sugs.is_empty() {
                        blur = true;
                    } else {
                        st.hidden = true;
                    }
                }
            });
        }
        // Champ comme dans Discord : cadre de 32 de haut, loupe au bout ; le sélecteur
        // « Local » (plugin Recherche locale) est rangé dedans, avant la loupe.
        let side = crate::theme::Tokens::icon_side(tk.typography.small);
        let pad = tk.spacing.item_x;
        let small = egui::FontId::proportional(tk.typography.button);
        let hint = ui.painter().layout_no_wrap(t::SEARCH_HINT.get(lang).to_owned(), small.clone(), tk.colors.faint).size().x;
        let chip = super::local_search::chip_width(ui, &tk, lang);
        // Jamais plus étroit que l'invite et la loupe : elles se chevauchaient dans une fenêtre étroite.
        let need = hint + side + 3.0 * pad + chip;
        let width = (row / 3.0).min(FIELD_WIDTH).max(need.min(row / 2.0)).min(ui.available_width());
        let (frame, _) = ui.allocate_exact_size(egui::vec2(width, FIELD_HEIGHT), egui::Sense::hover());
        let stroke = if focused { egui::Stroke::new(tk.border().width.max(1.0), tk.colors.accent) } else { tk.border() };
        ui.painter().rect(frame, tk.r_row(), tk.colors.bg1, stroke, egui::StrokeKind::Inside);
        let glass = egui::Rect::from_center_size(egui::pos2(frame.right() - pad - side / 2.0, frame.center().y), egui::Vec2::splat(side));
        crate::theme::icons::paint(crate::theme::icons::Icon::Search, ui.painter(), glass, tk.colors.muted);
        let inner = egui::Rect::from_min_max(egui::pos2(frame.left() + pad, frame.top()), egui::pos2(glass.left() - pad / 2.0, frame.bottom()));
        let mut field = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)));
        field.spacing_mut().item_spacing.x = pad / 2.0;
        let flip = super::local_search::chip(&mut field, &tk, lang, local);
        // « Rechercher <nom d'utilisateur> » dans un MP à deux, comme dans le client.
        let hint_text = if local { super::local_search::hint(lang).to_string() } else { peer.map_or_else(|| t::SEARCH_HINT.get(lang).to_string(), |name| f::search_in(lang, name)) };
        let edit = egui::TextEdit::singleline(&mut st.text).id(id).frame(egui::Frame::NONE).font(small.clone()).hint_text(RichText::new(hint_text).font(small).color(tk.colors.faint)).desired_width(field.available_width()).align(egui::Align2::LEFT_CENTER).min_size(egui::vec2(0.0, FIELD_HEIGHT - 2.0));
        // Les propositions s'ouvrent sous le cadre entier, pas sous le seul texte.
        let resp = field.add(edit).on_hover_text(t::SEARCH_TIP.get(lang)).with_new_rect(frame);
        // Recherches récentes (F16) : sous les filtres, tant que le champ est vide.
        let recent: Vec<String> = if focused && !st.hidden && st.text.trim().is_empty() && !local { st.recent.clone() } else { Vec::new() };
        let (mut rerun, mut forget) = (None, false);
        if focused && !sugs.is_empty() {
            let selected = st.selected;
            egui::Area::new(Id::unique("recherche-propositions")).order(egui::Order::Foreground).fixed_pos(resp.rect.left_bottom()).show(ui.ctx(), |ui| {
                egui::Frame::new().fill(tk.colors.bg3).stroke(tk.border()).corner_radius(tk.r_embed()).inner_margin(tk.embed_margin()).show(ui, |ui| {
                    ui.set_min_width(resp.rect.width());
                    if typing_value(&st.text).is_none() {
                        ui.label(RichText::new(t::FILTERS.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(tk.colors.muted));
                    }
                    for (k, sg) in sugs.iter().enumerate() {
                        let mut text = egui::text::LayoutJob::default();
                        text.append(&sg.label, 0.0, egui::TextFormat { color: tk.colors.text_strong, ..Default::default() });
                        if let Some(d) = sg.detail {
                            text.append(d, tk.spacing.item_x, egui::TextFormat { color: tk.colors.muted, ..Default::default() });
                        }
                        if ui.selectable_label(selected == Some(k), text).clicked() {
                            chosen = Some(k);
                        }
                    }
                    if !recent.is_empty() {
                        ui.separator();
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(ft::SEARCH_RECENT.get(lang)).small().family(crate::ui::fonts::demi()).strong().color(tk.colors.muted));
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                forget = ui.add(egui::Label::new(RichText::new(ft::SEARCH_RECENT_CLEAR.get(lang)).small().color(tk.colors.link)).sense(egui::Sense::click())).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();
                            });
                        });
                        for r in &recent {
                            if ui.selectable_label(false, RichText::new(r).color(tk.colors.text)).clicked() {
                                rerun = Some(r.clone());
                            }
                        }
                    }
                });
            });
        }
        if forget {
            st.recent.clear();
        }
        if let Some(text) = rerun {
            st.text = text;
            st.last_text.clone_from(&st.text);
            run = true;
        }
        if let Some(sg) = chosen.and_then(|k| sugs.get(k)) {
            st.text.clone_from(&sg.text);
            st.last_text.clone_from(&st.text);
            st.selected = None;
            if let Some(p) = sg.pick.clone() {
                st.remember(p);
            }
            // Curseur au bout du texte complété, le champ garde la main.
            if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) {
                let end = egui::text::CCursor::new(st.text.chars().count());
                state.cursor.set_char_range(Some(egui::text::CCursorRange::one(end)));
                state.store(ui.ctx(), id);
            }
            ui.memory_mut(|m| m.request_focus(id));
            ui.ctx().request_repaint();
        }
        if blur {
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        if run {
            self.run_search();
        }
        // Recherche locale (plugin, lot E1) : mode changé dans le champ, résultats pendant la frappe.
        self.local_search_typed(flip, typed && !run);
    }

    /// Panneau des résultats, à droite de la discussion.
    pub(super) fn search_panel(&mut self, ui: &mut egui::Ui) {
        let Some(a) = self.store.search.active.as_ref() else { return };
        let tk = self.theme.clone();
        let lang = self.lang;
        let h24 = tk.layout.clock == Clock::H24;
        let st = &mut self.search;
        // Textes mis en forme une fois par page (et par langue ou horloge).
        let key = (a.seq, lang, h24);
        if st.prepared_key != Some(key) || st.prepared.len() != a.hits.len() {
            st.prepared_key = Some(key);
            st.prepared = a
                .hits
                .iter()
                .map(|h| {
                    let mut doc = parse_message(&self.store, &h.message, lang, h24);
                    s::mark_terms(&mut doc, &a.terms);
                    let mut shown = h.message.clone();
                    shown.id |= UI_SALT;
                    (shown, Arc::new(doc))
                })
                .collect();
            st.geo_key = None;
        }
        let (mut close, mut sort_to, mut page_to, mut jump, mut mode_to) = (false, None, None, None, None);
        let (mut tab_to, tab_now) = (None, self.store.search.tab());
        let (was_local, indexed) = (a.local, self.local_search.len());
        let panel = egui::Panel::right("search").frame(egui::Frame::new().fill(tk.colors.bg1).inner_margin(tk.channels_margin())).default_size(tk.layout.members_width);
        // Disposition mobile : les résultats prennent tout l'écran (pas de seconde colonne).
        let mobile = super::mobile::is_active(ui.ctx());
        let panel = if mobile { panel.resizable(false).exact_size(ui.available_width()) } else { panel };
        // Fenêtre étroite : pris sur la discussion, le panneau l'écrasait (170 points au
        // lieu des 420 prévus). Il passe alors en superposition, à droite, et la
        // discussion garde toute sa largeur dessous.
        let room = ui.available_rect_before_wrap();
        let floats = !mobile && room.width() - tk.layout.members_width < super::layout::CHAT_MIN;
        let mut body = |ui: &mut egui::Ui| {
                ui.horizontal(|ui| {
                    let title = match (&a.phase, a.total) {
                        (Phase::Done, Some(n)) => p::results(lang, n),
                        _ => t::SEARCHING.get(lang).to_string(),
                    };
                    ui.label(RichText::new(title).family(crate::ui::fonts::demi()).strong().size(tk.typography.title).color(tk.colors.text_strong));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        close = icon_button(ui, &tk, Icon::Close, t::CLOSE_SEARCH.get(lang)).clicked();
                    });
                });
                if let Some(to) = super::local_search::mode_tabs(ui, &tk, lang, a.local, indexed) {
                    mode_to = Some(to);
                }
                // Onglets (F16) : messages, images, vidéos, liens, fichiers.
                if !a.local {
                    ui.horizontal_wrapped(|ui| {
                        for tab in s::Tab::ALL {
                            let text = match tab {
                                s::Tab::Messages => ft::SEARCH_TAB_MESSAGES,
                                s::Tab::Images => ft::SEARCH_TAB_IMAGES,
                                s::Tab::Videos => ft::SEARCH_TAB_VIDEOS,
                                s::Tab::Links => ft::SEARCH_TAB_LINKS,
                                s::Tab::Files => ft::SEARCH_TAB_FILES,
                            };
                            if ui.selectable_label(tab_now == tab, text.get(lang)).clicked() {
                                tab_to = Some(tab);
                            }
                        }
                    });
                }
                ui.horizontal(|ui| {
                    for sort in Sort::ALL {
                        if a.local && sort == Sort::Relevance {
                            continue;
                        }
                        if ui.selectable_label(a.query.sort == sort, sort_label(sort, lang)).clicked() {
                            sort_to = Some(sort);
                        }
                    }
                });
                ui.separator();
                let note = |ui: &mut egui::Ui, text: &str, color| {
                    ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
                };
                match &a.phase {
                    Phase::Loading => {
                        ui.vertical_centered(|ui| ui.spinner());
                    }
                    Phase::Indexing => {
                        ui.vertical_centered(|ui| ui.spinner());
                        note(ui, t::INDEXING.get(lang), tk.colors.muted);
                    }
                    Phase::NotIndexed | Phase::Failed(_) => {
                        let text = match &a.phase {
                            Phase::Failed(pb) => words::problem(lang, pb, h24),
                            _ => t::NOT_INDEXED.get(lang).to_string(),
                        };
                        note(ui, &text, tk.colors.danger);
                        if crate::ui::widgets::compact(ui, tx::RETRY.get(lang)).clicked() {
                            page_to = Some(a.page);
                        }
                    }
                    Phase::Done if a.hits.is_empty() => note(ui, t::NO_RESULTS.get(lang), tk.colors.muted),
                    Phase::Done => {
                        ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                            let last = a.last_page();
                            if last > 0 {
                                // Ligne à la hauteur des boutons : posée en bas, une ligne
                                // ordinaire (plus basse qu'eux) les laissait dépasser du panneau.
                                let row = egui::vec2(ui.available_width(), tk.head_row());
                                ui.allocate_ui_with_layout(row, egui::Layout::left_to_right(egui::Align::Center), |ui| {
                                    if ui.add_enabled(a.page > 0, egui::Button::new(t::PREVIOUS.get(lang))).clicked() {
                                        page_to = Some(a.page - 1);
                                    }
                                    ui.label(RichText::new(f::page_of(lang, a.page + 1, last + 1)).small().color(tk.colors.muted));
                                    if ui.add_enabled(a.page < last, egui::Button::new(t::NEXT.get(lang))).clicked() {
                                        page_to = Some(a.page + 1);
                                    }
                                });
                                ui.separator();
                            }
                            ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
                                jump = results_list(ui, &tk, lang, &self.store, &self.images, st, a.seq);
                            });
                        });
                    }
                }
            };
        if floats {
            let width = FLOAT_WIDTH.min(room.width() - 2.0 * tk.spacing.card_pad).max(tk.layout.members_width.min(room.width()));
            let at = egui::Rect::from_min_max(egui::pos2(room.right() - width, room.top()), room.max);
            let frame = egui::Frame::new().fill(tk.colors.bg1).stroke(tk.border()).shadow(tk.shadow(crate::theme::Elevation::Menu)).inner_margin(tk.channels_margin());
            egui::Area::new(Id::unique("recherche-superposee")).order(egui::Order::Middle).fixed_pos(at.min).show(ui.ctx(), |ui| {
                frame.show(ui, |ui| {
                    let inner = at.size() - frame.total_margin().sum();
                    ui.set_min_size(inner);
                    ui.set_max_size(inner);
                    body(ui);
                });
            });
        } else {
            panel.show(ui, body);
        }
        if close {
            self.store.close_search();
        }
        if was_local {
            // Résultats trouvés sur place : pages et tri aussi.
            if let Some(sort) = sort_to {
                self.local_search_run(0, Some(sort), false);
            }
            if let Some(page) = page_to {
                self.local_search_run(page, None, false);
            }
            (sort_to, page_to) = (None, None);
        }
        if let Some(local) = mode_to {
            self.local_search_mode(local);
        }
        if let Some(sort) = sort_to
            && self.store.search_sort(sort)
        {
            self.search.to_top = true;
        }
        if let Some(page) = page_to
            && self.store.search_page(page)
        {
            self.search.to_top = true;
        }
        if let Some(tab) = tab_to
            && self.store.search_tab(tab)
        {
            self.search.to_top = true;
        }
        if let Some((channel, id)) = jump {
            self.jump_to_message(channel, id);
        }
    }
}

/// Dernier mot s'il est la valeur d'un filtre en cours de frappe.
fn typing_value(text: &str) -> Option<Filter> {
    s::tokens(text).last().filter(|tk| tk.end == text.len()).and_then(|tk| tk.filter)
}

/// Liste virtualisée des résultats : seuls ceux qui coupent la vue sont mis
/// en page. Hauteurs estimées, puis corrigées au premier dessin (une image
/// de plus, une fois). Renvoie le saut demandé (salon, message).
fn results_list(ui: &mut egui::Ui, tk: &Tokens, lang: Lang, store: &crate::state::Store, images: &crate::images::Images, st: &mut SearchUi, seq: u64) -> Option<(u64, u64)> {
    // Dans une carte de résultat, le message n'a ni la marge droite de la discussion ni
    // son grand avatar : son texte se repliait à mi-largeur de la carte.
    let tight = hit_tokens(tk);
    let tk = &tight;
    let width = ui.available_width();
    let n = st.prepared.len();
    let gap = tk.spacing.item_y;
    if st.geo_key != Some((seq, width.to_bits())) || st.geo.len() != n {
        st.geo_key = Some((seq, width.to_bits()));
        let estimate = tk.layout.avatar_size + 3.0 * tk.typography.body + gap;
        st.geo.set((0..n as u64).collect(), vec![estimate; n]);
    }
    let mut area = egui::ScrollArea::vertical().id_salt("résultats").auto_shrink(false);
    if std::mem::take(&mut st.to_top) {
        area = area.vertical_scroll_offset(0.0);
    }
    let mut jump = None;
    let mut changed = false;
    area.show_viewport(ui, |ui, viewport| {
        let origin = ui.max_rect().min;
        ui.set_min_size(egui::vec2(width, st.geo.total()));
        for i in st.geo.range(viewport.min.y, viewport.max.y) {
            let Some((m, doc)) = st.prepared.get(i) else { continue };
            let rect = egui::Rect::from_min_size(egui::pos2(origin.x, origin.y + st.geo.top(i)), egui::vec2(width, f32::INFINITY));
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)).id_salt(("résultat", i)));
            if let Some(j) = hit_ui(&mut child, tk, lang, store, images, m, doc) {
                jump = Some(j);
            }
            child.add_space(gap);
            let h = child.min_rect().height();
            changed |= st.geo.set_height(i, (h * ui.pixels_per_point()).round() / ui.pixels_per_point());
        }
    });
    if changed {
        ui.ctx().request_repaint();
    }
    jump
}

/// Jetons d'un message montré dans une carte de résultat : marges de la carte, petit avatar.
fn hit_tokens(tk: &Tokens) -> Tokens {
    let mut t = tk.clone();
    t.spacing.message_right = t.spacing.message_x.min(t.spacing.embed_pad);
    t.spacing.message_x = t.spacing.message_x.min(t.spacing.embed_pad);
    t.layout.avatar_size = t.layout.avatar_size.min(t.layout.small_avatar);
    t.spacing.avatar_gap = t.spacing.avatar_gap.min(t.spacing.item_x + 2.0);
    t
}

/// Un résultat : salon (icône et nom) avec « Aller au message », puis le
/// message rendu comme dans la liste, sans actions. Renvoie le saut demandé.
fn hit_ui(ui: &mut egui::Ui, tk: &Tokens, lang: Lang, store: &crate::state::Store, images: &crate::images::Images, m: &Message, doc: &Doc) -> Option<(u64, u64)> {
    let real = m.id & !UI_SALT;
    let channel = m.channel_id;
    let mut jump = None;
    ui.horizontal(|ui| {
        let small = tk.typography.small;
        let dm = store.dms.iter().find(|d| d.id == channel);
        let name = match dm {
            Some(d) => words::dm_name(lang, &d.name).to_string(),
            None => store.channel_name(channel).map(str::to_string).or_else(|| store.search.active.as_ref().and_then(|a| a.hits.iter().find(|h| h.message.channel_id == channel).and_then(|h| h.thread_name.clone()))).unwrap_or_default(),
        };
        if dm.is_some() {
            ui.label(RichText::new("@").size(small).color(tk.colors.faint));
        } else {
            inline_icon(ui, if store.is_thread(channel) { Icon::Thread } else { Icon::Hash }, small, tk.colors.faint);
        }
        ui.add(egui::Label::new(RichText::new(name).size(small).family(crate::ui::fonts::demi()).strong().color(tk.colors.muted)).truncate());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if crate::ui::widgets::compact(ui, tx::JUMP.get(lang)).clicked() {
                jump = Some((channel, real));
            }
        });
    });
    egui::Frame::new().fill(tk.colors.bg2).stroke(tk.border()).corner_radius(tk.r_embed()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let mut scratch = ActionsUi::default();
        let mut acts = Acts::new(&mut scratch, store, channel, ui.ctx().clone());
        acts.browse = true;
        message_ui_acts(ui, tk, lang, images, m, doc, false, snowflake_ms(real), &mut acts);
        for a in scratch.take_out() {
            if let Action::Jump(id) = a {
                jump = Some((channel, id & !UI_SALT));
            }
        }
    });
    jump
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names() -> Names {
        Names {
            people: vec![(10, "Alice".into()), (11, "Jean Dupont".into()), (12, "Bob".into())],
            channels: vec![(20, "général".into()), (21, "hors-sujet".into())],
            ..Names::default()
        }
    }

    fn labels(v: &[Suggestion]) -> Vec<&str> {
        v.iter().map(|s| s.label.as_str()).collect()
    }

    #[test]
    fn empty_field_lists_the_filters() {
        let (v, pre) = suggest("", Lang::Fr, &names(), "2026-11-13");
        assert!(!pre);
        assert_eq!(labels(&v), vec!["de:", "mentionne:", "contient:", "dans:", "avant:", "apres:", "pendant:", "epingle:", "typeAuteur:"]);
        assert_eq!(v[0].text, "de:");
        let (v, _) = suggest("salut ", Lang::En, &Names { private: true, ..names() }, "2026-11-13");
        assert!(!labels(&v).contains(&"in:"), "pas de salons dans un MP");
        assert_eq!(v[0].text, "salut from:");
    }

    #[test]
    fn values_are_proposed_and_names_are_remembered() {
        let (v, pre) = suggest("rdv de:je", Lang::Fr, &names(), "2026-11-13");
        assert!(pre);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].text, "rdv de:\"Jean Dupont\" ");
        assert_eq!(v[0].pick, Some(Pick::Person("Jean Dupont".into(), 11)));
        // Le texte complété s'analyse sans deviner.
        let mut ui = SearchUi::default();
        ui.remember(v[0].pick.clone().unwrap());
        let n = Names { picked_people: ui.picked_people.clone(), ..names() };
        assert_eq!(s::parse(&v[0].text, &n, 0, Sort::Newest).map(|p| p.query.authors), Ok(vec![11]));
        let (v, _) = suggest("in:hor", Lang::En, &names(), "2026-11-13");
        assert_eq!(v[0].text, "in:hors-sujet ");
        let (v, _) = suggest("has:", Lang::En, &names(), "x");
        assert_eq!(labels(&v), vec!["link", "embed", "file", "image", "video", "sound", "sticker", "poll"]);
        let (v, _) = suggest("contient:son", Lang::Fr, &names(), "x");
        assert_eq!(labels(&v), vec!["son", "sondage"]);
        let (v, _) = suggest("avant:2026", Lang::Fr, &names(), "2026-11-13");
        assert_eq!(v[0].text, "avant:2026-11-13 ");
        let (v, _) = suggest("epingle:", Lang::Fr, &names(), "x");
        assert_eq!(labels(&v), vec!["oui", "non"]);
        // Début d'un mot-clé, dans l'une ou l'autre langue.
        let (v, pre) = suggest("bonjour me", Lang::Fr, &names(), "x");
        assert!(!pre);
        assert_eq!(labels(&v), vec!["mentionne:"]);
        assert_eq!(v[0].text, "bonjour mentionne:");
        assert!(suggest("bonjour zz", Lang::Fr, &names(), "x").0.is_empty());
    }

    #[test]
    fn picked_names_are_bounded() {
        let mut ui = SearchUi::default();
        for i in 0..(MAX_PICKED as u64 + 5) {
            ui.remember(Pick::Person(format!("p{i}"), i));
        }
        ui.remember(Pick::Person("p40".into(), 99));
        assert_eq!(ui.picked_people.len(), MAX_PICKED);
        assert_eq!(ui.picked_people.last(), Some(&("p40".to_string(), 99)));
    }

    #[test]
    fn message_links_are_followed_inside() {
        let ctx = egui::Context::default();
        assert!(follow_link(&ctx, "https://discord.com/channels/2000000000000000001/3000000000000000011/5000000000000000301"));
        assert_eq!(ctx.data_mut(|d| d.remove_temp::<(u64, u64)>(Id::unique("saut-lien"))), Some((3000000000000000011, 5000000000000000301)));
        assert!(!follow_link(&ctx, "https://example.invalid/channels/1/2/3"));
    }

    #[test]
    fn errors_are_worded() {
        for e in [
            ParseError::Empty,
            ParseError::TooLong,
            ParseError::NoChannels,
            ParseError::UnknownUser("x".into()),
            ParseError::AmbiguousUser("x".into()),
            ParseError::UnknownChannel("x".into()),
            ParseError::AmbiguousChannel("x".into()),
            ParseError::BadDate("x".into()),
            ParseError::BadValue { filter: Filter::Has, value: "x".into() },
        ] {
            for lang in Lang::ALL {
                assert!(!error_text(lang, &e).is_empty());
            }
        }
        assert!(error_text(Lang::En, &ParseError::BadValue { filter: Filter::Has, value: "zz".into() }).contains("has"));
    }
}
