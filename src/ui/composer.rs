//! Zone de saisie, sous la liste de messages : « X écrit… », messages non
//! envoyés, mode lent et compteur de caractères, champ de texte (Entrée
//! envoie, Maj+Entrée va à la ligne ; hauteur bornée, au-delà il défile) et
//! autocomplétion (`@`, `#`, `:`) dans une liste flottante au-dessus.
//!
//! Le texte en cours est gardé par salon (brouillons en mémoire, bornés).
//! La logique sans egui (mots à compléter, propositions, syntaxe envoyée,
//! brouillons) est dans `complete.rs`.
//!
//! Pour étendre la zone (réponse, modification d'un message…), ajouter une
//! bande au-dessus du champ dans [`App::composer_ui`], comme `info_row`.

use super::complete::{self, Draft, Drafts, Kind, Query, Suggestion, COUNTER_FROM, MESSAGE_MAX};
use super::{util::inline_icon, App};
use crate::i18n::{self, f, p, t as tx, Lang};
use crate::i18n::table::restes::t as ct;
use crate::state::{Cmd, SendBlock};
use crate::theme::{icons::Icon, Tokens, TypingSignal};
use eframe::egui::{self, Id, Key, RichText};
use std::time::{Duration, Instant};

/// Écart minimal entre deux « en train d'écrire » envoyés (Discord en
/// garde un 10 s : on le renouvelle un peu avant).
const TYPING_EVERY: Duration = Duration::from_secs(8);

/// Propositions calculées pour un mot : refaites seulement quand il change.
struct Cached {
    channel: u64,
    query: Query,
    items: Vec<Suggestion>,
    /// Texte réellement inséré pour chaque proposition (voir `complete::inserts`).
    inserts: Vec<String>,
}

#[derive(Default)]
pub(super) struct Composer {
    /// Salon dont le brouillon est dans `draft`.
    channel: Option<u64>,
    /// Compte à qui sont les brouillons (oubliés au changement de compte).
    owner: Option<u64>,
    draft: Draft,
    drafts: Drafts,
    cache: Option<Cached>,
    /// La liste de propositions était ouverte à l'image précédente : Entrée
    /// lui revient au lieu d'envoyer.
    popup_open: bool,
    /// Caractères du message tel qu'il serait envoyé (recalculé à chaque modification).
    length: usize,
    /// Dernier « en train d'écrire » envoyé (salon, instant).
    typing_sent: Option<(u64, Instant)>,
    /// Avance à chaque changement du texte ou du salon : les brouillons
    /// persistants (lot E3) savent ainsi qu'il y a quelque chose à écrire.
    rev: u64,
}

impl Composer {
    /// Session fermée : texte en cours et brouillons oubliés.
    pub(super) fn clear(&mut self) {
        *self = Composer::default();
    }

    /// Brouillon gardé pour un autre salon que celui affiché.
    pub(super) fn has_draft(&self, channel: u64) -> bool {
        self.drafts.has(channel)
    }

    /// Autocomplétion ouverte à l'image précédente (Échap et Entrée lui reviennent).
    pub(super) fn popup_open(&self) -> bool {
        self.popup_open
    }

    /// Compteur des changements (brouillons persistants, lot E3).
    pub(super) fn rev(&self) -> u64 {
        self.rev
    }

    /// Compte à qui sont les brouillons en mémoire.
    pub(super) fn owner(&self) -> Option<u64> {
        self.owner
    }

    /// Tous les brouillons non vides : celui du salon affiché et ceux des autres.
    pub(super) fn all_drafts(&self) -> Vec<(u64, &Draft)> {
        let mut out: Vec<(u64, &Draft)> = self.drafts.iter().collect();
        if let Some(c) = self.channel
            && !self.draft.is_empty()
        {
            out.push((c, &self.draft));
        }
        out
    }

    /// Brouillons relus du disque pour le compte `me` : ils ne remplacent
    /// jamais un texte déjà tapé depuis le lancement.
    pub(super) fn restore(&mut self, me: Option<u64>, items: Vec<(u64, Draft)>) {
        if self.owner != me {
            self.drafts.clear();
            self.draft.clear();
            self.channel = None;
            self.owner = me;
            self.typing_sent = None;
        }
        for (c, d) in items {
            if self.channel == Some(c) {
                if self.draft.is_empty() {
                    self.draft = d;
                    self.cache = None;
                    self.recount();
                }
            } else if !self.drafts.has(c) {
                self.drafts.put(c, d);
            }
        }
    }

    /// Range le brouillon du salon quitté et reprend celui de `channel`.
    fn sync(&mut self, me: Option<u64>, channel: u64) {
        if self.owner != me {
            self.drafts.clear();
            self.draft.clear();
            self.channel = None;
            self.owner = me;
            self.typing_sent = None;
        }
        if self.channel == Some(channel) {
            return;
        }
        if let Some(old) = self.channel {
            self.drafts.put(old, std::mem::take(&mut self.draft));
        }
        self.draft = self.drafts.take(channel);
        self.channel = Some(channel);
        self.cache = None;
        self.popup_open = false;
        self.recount();
    }

    fn recount(&mut self) {
        self.length = self.draft.encoded().trim().chars().count();
        self.rev = self.rev.wrapping_add(1);
    }
}

/// Retire les appuis sur `key` sans aucune touche de modification (Maj+Entrée
/// reste au champ de texte, qui en fait une nouvelle ligne).
fn take_plain_key(i: &mut egui::InputState, key: Key) -> bool {
    let before = i.events.len();
    i.events.retain(|e| !matches!(e, egui::Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.is_none()));
    i.events.len() != before
}

/// Durée lisible : « 7 s », « 1 min 5 s », « 2 h 30 min ».
fn duration(lang: Lang, ms: i64) -> String {
    let secs = u64::try_from(ms.max(0)).unwrap_or(0).div_ceil(1000);
    match secs {
        0..60 => f::secs(lang, secs),
        60..3600 => f::mins_secs(lang, secs / 60, secs % 60),
        _ => f::hours_mins(lang, secs / 3600, secs / 60 % 60),
    }
}

/// « X écrit… », « X et Y écrivent… », etc.
/// Hauteur de la ligne « X écrit… », réservée sous le champ.
fn typing_row_height(t: &Tokens) -> f32 {
    (t.typography.small * 1.5).ceil()
}

fn typing_text(lang: Lang, names: &[&str]) -> String {
    match names {
        [a] => f::typing_one(lang, a),
        [a, b] => f::typing_two(lang, a, b),
        [a, b, c] => f::typing_three(lang, a, b, c),
        _ => tx::SEVERAL_TYPING.get(lang).to_string(),
    }
}

/// Ce que la liste montre pour une proposition, et son détail à droite.
fn shown(lang: Lang, s: &Suggestion) -> (String, Option<&'static str>) {
    match s.kind {
        Kind::Member => (format!("@{}", s.label), None),
        Kind::Role => (format!("@{}", s.label), Some(tx::ROLE_DETAIL.get(lang))),
        Kind::Everyone => (s.label.clone(), Some(tx::EVERYONE_DETAIL.get(lang))),
        Kind::Here => (s.label.clone(), Some(tx::HERE_DETAIL.get(lang))),
        Kind::Channel => (format!("#{}", s.label), None),
        Kind::CustomEmoji => (s.label.clone(), None),
        Kind::Emoji => (s.label.clone(), None),
    }
}

/// Invite de la zone de saisie sur une seule ligne, coupée par « … » si
/// elle dépasse : sur deux lignes, elle grandissait la zone dans une fenêtre étroite.
fn fit_hint(ui: &egui::Ui, hint: &str, width: f32) -> String {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x;
    if wide(hint) <= width {
        return hint.to_owned();
    }
    let mut cut: Vec<char> = hint.chars().collect();
    while cut.len() > 1 {
        cut.pop();
        let short = format!("{}…", cut.iter().collect::<String>().trim_end());
        if wide(&short) <= width {
            return short;
        }
    }
    String::new()
}

impl App {
    /// Texte du champ de saisie du salon affiché (tests et aperçu).
    pub fn composer_text(&self) -> &str {
        &self.input.draft.text
    }

    /// Ouvre la liste des commandes « / » dans une saisie vide ; un brouillon
    /// déjà commencé est gardé tel quel.
    pub(super) fn open_command_list(&mut self) {
        if self.input.draft.text.trim().is_empty() {
            self.set_composer_text("/");
        }
        self.ctx.memory_mut(|m| m.request_focus(composer_id()));
    }

    /// Remplace le texte du champ et y met le curseur, à la fin (tests et
    /// aperçu : comme si l'utilisateur l'avait tapé).
    pub fn set_composer_text(&mut self, text: &str) {
        if let Some(c) = self.store.channel {
            self.input.sync(self.store.me.as_ref().map(|u| u.id), c);
        }
        self.input.draft.text = text.to_string();
        self.input.recount();
        let id = composer_id();
        let mut st = egui::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(text.chars().count()))));
        st.store(&self.ctx, id);
        self.ctx.memory_mut(|m| m.request_focus(id));
        self.ctx.request_repaint();
    }

    /// « Mentionner » (menu d'un utilisateur) : `@nom` ajouté à la fin du
    /// brouillon du salon affiché, lié à `<@id>` comme une proposition
    /// choisie (syntaxe brute si le nom lisible désigne déjà quelqu'un d'autre).
    pub(super) fn insert_mention(&mut self, user_id: u64, name: &str) {
        let Some(c) = self.store.channel else { return };
        self.input.sync(self.store.me.as_ref().map(|u| u.id), c);
        let s = complete::Suggestion { kind: Kind::Member, label: name.to_string(), insert: format!("@{name}"), syntax: Some(format!("<@{user_id}>")) };
        let ins = complete::inserts(&self.input.draft.links, std::slice::from_ref(&s)).pop().unwrap_or_else(|| s.insert.clone());
        let text = &mut self.input.draft.text;
        if !text.is_empty() && !text.ends_with(char::is_whitespace) {
            text.push(' ');
        }
        text.push_str(&ins);
        text.push(' ');
        self.input.draft.accept(&s, &ins);
        let all = self.input.draft.text.clone();
        self.set_composer_text(&all);
    }

    /// Bas de la zone de discussion (dans le panneau du bas de `chat`).
    pub(super) fn composer_ui(&mut self, ui: &mut egui::Ui, cid: u64, hint: String) {
        let t = self.theme.clone();
        let lang = self.lang;
        let id = composer_id();
        self.input.sync(self.store.me.as_ref().map(|u| u.id), cid);
        // Envoi refusé de façon certaine : saisie fermée, la raison à la place de l'invite.
        let block = self.store.send_block(cid);
        // Flèche haut dans une saisie vide : modifier son dernier message (lot T6).
        self.composer_keys(ui, cid, id);
        let focused = ui.memory(|m| m.has_focus(id));
        // Liste des commandes « / » (lot T11) : flèches, Tab et Entrée lui reviennent.
        let slash = block.is_none() && self.slash_keys(ui, cid, focused);
        let enter = block.is_none() && focused && !self.input.popup_open && !slash && ui.input_mut(|i| take_plain_key(i, Key::Enter));

        self.request_banner(ui, cid);
        self.sending_list(ui, cid);
        self.unsent_list(ui, cid, block.is_none());
        // Envois de fichiers en cours ou en échec (lot T8).
        self.upload_rows(ui, cid, block.is_none());
        if block.is_none() {
            self.info_row(ui, &t);
        }
        // Commandes des applications (lot T11) : suivi des envois, formulaire
        // d'une commande à la place du champ, liste « / » au-dessus du champ.
        self.interaction_rows(ui, cid);
        if block.is_none() && self.command_form_ui(ui, cid) {
            return;
        }
        if block.is_none() {
            self.slash_popup(ui, cid);
        }
        // Pièces jointes : dépôt, collage, plateau (lot T8).
        if block.is_none() {
            let target = self.store.channel_name(cid).map(|n| format!("#{n}")).unwrap_or_else(|| self.store.dms.iter().find(|d| d.id == cid).map(|d| d.name.clone()).unwrap_or_default());
            self.upload_input(ui, cid, id, &target);
            self.attachment_tray(ui, cid);
            // Message vocal en cours d'enregistrement (phase 6).
            self.voice_note_row(ui, cid);
        }
        // « X écrit… » au-dessus de la carte (hauteur réservée) : la carte finit au ras
        // de sa marge du bas, alignée sur le panneau utilisateur comme dans le client.
        self.typing_row(ui, cid, &t);
        // Barre « Réponse à … » (lot T6), collée à la carte : ses coins du bas et
        // ceux du haut de la carte sont droits.
        let replying = self.reply_bar(ui, cid, id);
        let mut corners = t.r_composer();
        if replying {
            (corners.nw, corners.ne) = (0, 0);
        }
        egui::Frame::new().fill(t.colors.bg3).stroke(t.border()).corner_radius(corners).inner_margin(t.composer_inner()).show(ui, |ui| match block {
            // Bouton du sélecteur d'emojis à droite du champ (lot T4).
            None => {
                let anchor = ui
                    .with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        // « + » (joindre) à gauche du champ, le reste à droite.
                        // De gauche à droite, comme le client : GIF, autocollants, emojis, applications.
                        self.apps_button(ui, &t);
                        let b = self.emoji_button(ui, &t);
                        // Autocollants et GIF à gauche de l'emoji, comme Discord (lot G).
                        self.sticker_button(ui, &t, cid);
                        self.gif_button(ui, &t);
                        ui.with_layout(egui::Layout::left_to_right(egui::Align::Min), |ui| {
                            // « + » : fichier, fil, sondage, applications, message vocal (V1).
                            self.plus_button(ui, cid);
                            // La première ligne du texte sur l'axe des boutons-icônes, plus hauts qu'elle.
                            let row = super::emoji::row_height(ui, &egui::TextStyle::Body.resolve(ui.style()));
                            let side = Tokens::icon_button_side(t.typography.button, ui.spacing().button_padding.y);
                            let above = ((side - row) / 2.0).max(0.0);
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 0.0;
                                // Le champ ne prend que sa ligne : la hauteur minimale des contrôles
                                // (32 dans les préréglages calqués) grandissait la zone de 56 à 68.
                                ui.spacing_mut().interact_size.y = row;
                                ui.add_space(above);
                                self.text_field(ui, cid, id, hint, &t);
                                ui.add_space(above);
                            });
                        });
                        b
                    })
                    .inner;
                self.composer_picker(ui, cid, anchor);
            }
            Some(b) => {
                let (icon, reason) = match b {
                    SendBlock::Timeout { until_ms } => (Icon::Hourglass, f::timed_out_composer(lang, i18n::time::until(lang, until_ms, self.h24()))),
                    SendBlock::NoPermission => (Icon::Lock, tx::NO_PERMISSION_TO_SEND.get(lang).to_string()),
                    // MP officiel : la discussion y montre son bandeau à la place de cette saisie.
                    SendBlock::Official => (Icon::ShieldLock, crate::i18n::table::messages::t::OFFICIAL_ONLY.get(lang).to_string()),
                };
                // Pas de champ de saisie : la raison est une étiquette
                // dessinée dans la vraie couleur `muted` (une invite de
                // champ inactif serait atténuée par egui). Le brouillon
                // éventuel est gardé pour plus tard.
                // Même largeur que le champ de saisie qu'elle remplace.
                ui.set_min_width(ui.available_width());
                ui.horizontal(|ui| {
                    inline_icon(ui, icon, t.typography.body, t.colors.muted);
                    ui.add(egui::Label::new(RichText::new(reason).color(t.colors.muted)).wrap());
                });
            }
        });
        if enter {
            self.submit();
            ui.memory_mut(|m| m.request_focus(id));
        }
    }

    /// Champ de texte et autocomplétion.
    fn text_field(&mut self, ui: &mut egui::Ui, cid: u64, id: Id, hint: String, t: &Tokens) {
        let lang = self.lang;
        let max_height = t.layout.composer_rows.max(1.0) * ui.text_style_height(&egui::TextStyle::Body);
        // Réponse en préparation : Échap garde le focus (egui le retire sinon
        // avant que la barre de réponse ne voie la touche) pour l'annuler.
        let replying = self.actions.reply.as_ref().is_some_and(|r| r.channel_id == cid);
        let store = &self.store;
        let Composer { draft, cache, .. } = &mut self.input;
        let Draft { text, links } = draft;
        let links: &[(String, String)] = links;
        // Invite sur une ligne (champ vide seulement : sinon elle n'est pas dessinée).
        let shown_hint = if text.is_empty() { fit_hint(ui, &hint, ui.available_width()) } else { String::new() };
        // Emojis du champ en image (lot T4) : places notées à la mise en page.
        let spots = std::cell::RefCell::new(Vec::new());
        let mut layouter = |ui: &egui::Ui, buf: &dyn egui::TextBuffer, wrap: f32| {
            let font = egui::TextStyle::Body.resolve(ui.style());
            let (job, s) = super::composer_emoji::input_job(ui, buf.as_str(), links, &font, wrap, Some(t));
            *spots.borrow_mut() = s;
            ui.fonts_mut(|f| f.layout_job(job))
        };
        let edit = egui::TextEdit::multiline(text).frame(egui::Frame::NONE).desired_rows(1).desired_width(f32::INFINITY).hint_text(RichText::new(shown_hint)).layouter(&mut layouter);
        let filter = edit.get_event_filter();
        let edit = edit.event_filter(egui::EventFilter { escape: filter.escape || replying, ..filter });
        let out = egui::ScrollArea::vertical()
            .id_salt("composer_scroll")
            .max_height(max_height)
            .min_scrolled_height(max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                let out = egui::CompletionPopup::new(id).show(ui, edit, |q| {
                    let Some(query) = complete::query_at(q.text, q.word_range.end.0).filter(|x| x.start == q.word_range.start.0) else {
                        *cache = None;
                        return Vec::new();
                    };
                    let fresh = cache.as_ref().is_some_and(|c| c.channel == cid && c.query == query);
                    if !fresh {
                        let items = complete::suggest(store, cid, &query);
                        let inserts = complete::inserts(links, &items);
                        *cache = Some(Cached { channel: cid, query, items, inserts });
                    }
                    let Some(c) = cache.as_ref() else { return Vec::new() };
                    c.items
                        .iter()
                        .zip(&c.inserts)
                        .map(|(s, ins)| {
                            let (label, detail) = shown(lang, s);
                            let item = egui::Suggestion::new(format!("{ins} ")).content(label);
                            match detail {
                                Some(d) => item.description(d),
                                None => item,
                            }
                        })
                        .collect()
                });
                super::emoji::paint_spots(ui, out.text_edit.galley_pos, &out.text_edit.galley, &spots.borrow());
                out
            })
            .inner;
        // Nom lu par les lecteurs d'écran : l'invite du champ (lot C5).
        super::a11y::name(ui.ctx(), out.text_edit.response.id, &hint);
        // Clic droit : couper, copier, coller (F7).
        self.composer_context_menu(ui, id, &out.text_edit.response.response);
        // Barre de mise en forme au-dessus du texte sélectionné (F32).
        self.format_bar(ui, id, &out.text_edit.galley, out.text_edit.galley_pos, out.text_edit.cursor_range, t);
        // Autocollants proposés pour le mot écrit (F17) ; la liste de
        // l'autocomplétion, ouverte, garde la place.
        let word = if out.is_open { None } else { super::sticker_hints::word(&self.input.draft.text).map(str::to_owned) };
        self.sticker_hints(ui, id, out.text_edit.response.response.rect, cid, word, t);
        if let Some(acc) = &out.accepted
            && let Some(c) = &self.input.cache
            && let Some(i) = c.inserts.iter().position(|ins| acc.insert.strip_suffix(' ') == Some(ins.as_str()))
            && let Some(s) = c.items.get(i)
        {
            let (s, ins) = (s.clone(), c.inserts[i].clone());
            self.input.draft.accept(&s, &ins);
        }
        self.input.popup_open = out.is_open;
        if out.text_edit.response.changed() || out.accepted.is_some() {
            self.input.recount();
            if out.text_edit.response.changed() {
                self.signal_typing(cid);
            }
        }
    }

    /// « En train d'écrire » envoyé pendant la frappe, au plus toutes les 8 s
    /// (sauf si le réglage ou le mode discret le coupe).
    fn signal_typing(&mut self, cid: u64) {
        if self.theme.layout.typing != TypingSignal::Sent || self.stealth_no_typing() || self.input.draft.is_empty() {
            return;
        }
        let now = Instant::now();
        if self.input.typing_sent.is_some_and(|(c, at)| c == cid && now.duration_since(at) < TYPING_EVERY) {
            return;
        }
        self.input.typing_sent = Some((cid, now));
        self.store.push_command(Cmd::Typing { channel_id: cid });
    }

    /// « X écrit… » sous la liste, tant que quelqu'un écrit. Un seul réveil
    /// est demandé, à la prochaine échéance, et seulement s'il est affiché.
    fn typing_row(&self, ui: &mut egui::Ui, cid: u64, t: &Tokens) {
        let lang = self.lang;
        let now = self.store.now();
        let someone = tx::SOMEONE.get(lang);
        let names: Vec<&str> = self.store.typing.active(cid, now).map(|x| x.name.as_deref().unwrap_or(someone)).collect();
        // Hauteur réservée : la ligne apparaît et disparaît sans déplacer le champ.
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), typing_row_height(t)), egui::Sense::hover());
        // Mode lent : sous la carte, à droite (comme dans le client).
        let rect = self.slowmode_note(ui, cid, t, rect);
        let key = Id::unique(("ligne-écriture", cid));
        let k = super::motion::hover(ui.ctx(), key, !names.is_empty());
        if k <= 0.0 {
            return;
        }
        // Pendant le fondu de sortie, le dernier texte reste.
        let text = if names.is_empty() {
            ui.data(|d| d.get_temp::<String>(key)).unwrap_or_default()
        } else {
            let text = typing_text(lang, &names);
            ui.data_mut(|d| d.insert_temp(key, text.clone()));
            text
        };
        let dots = egui::Rect::from_min_size(rect.min, egui::vec2((rect.height() * 1.5).round(), rect.height()));
        if !names.is_empty() {
            super::motion::dots(ui, dots, t.colors.text.gamma_multiply(k));
        }
        let room = egui::Rect::from_min_max(egui::pos2(dots.right() + t.spacing.item_y, rect.top()), rect.max);
        ui.scope_builder(egui::UiBuilder::new().max_rect(room).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
            ui.add(egui::Label::new(RichText::new(text).small().family(super::fonts::demi()).color(t.colors.muted.gamma_multiply(k))).truncate());
        });
        if let Some(end) = self.store.typing.next_end(cid, now) {
            ui.ctx().request_repaint_after(Duration::from_millis(u64::try_from(end - now).unwrap_or(0)));
        }
    }

    /// Compteur de caractères, au-dessus du champ, à l'approche de la limite.
    fn info_row(&self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let len = self.input.length;
        if len < COUNTER_FROM {
            return;
        }
        let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
        ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if len >= COUNTER_FROM {
                let (text, color) = match MESSAGE_MAX.checked_sub(len) {
                    Some(n) => (i18n::number(lang, n as u64), t.colors.muted),
                    None => (p::too_long_by(lang, (len - MESSAGE_MAX) as u64), t.colors.danger),
                };
                ui.label(RichText::new(text).small().color(color));
            }
        });
    }

    /// « Mode lent » et son sablier, à droite de la ligne réservée sous la
    /// carte ; rend ce qui reste de la ligne à gauche. Pendant un compte à
    /// rebours, réveil à chaque seconde (et seulement alors).
    fn slowmode_note(&self, ui: &egui::Ui, cid: u64, t: &Tokens, row: egui::Rect) -> egui::Rect {
        let lang = self.lang;
        let slow = self.store.slowmode(cid);
        if slow == 0 || self.store.send_block(cid).is_some() {
            return row;
        }
        let left = self.store.slowmode_left(cid, self.store.now());
        let text = match left {
            Some(ms) => f::slowmode_left(lang, duration(lang, ms)),
            None => f::slowmode_on(lang, duration(lang, i64::from(slow) * 1000)),
        };
        let p = ui.painter();
        let galley = p.layout_no_wrap(text, super::fonts::medium_font(t.typography.small), t.colors.muted);
        let side = Tokens::icon_side(t.typography.small);
        let icon = egui::Rect::from_center_size(egui::pos2(row.right() - side / 2.0, row.center().y), egui::Vec2::splat(side));
        let at = egui::pos2(icon.left() - t.spacing.item_y - galley.size().x, row.center().y - galley.size().y / 2.0);
        // Fenêtre très étroite : la ligne « … écrit » garde la place.
        if at.x < row.left() + row.width() / 2.0 {
            return row;
        }
        crate::theme::icons::paint(Icon::Hourglass, p, icon, t.colors.muted);
        p.galley(at, galley, t.colors.muted);
        if let Some(ms) = left {
            ui.ctx().request_repaint_after(Duration::from_millis(u64::try_from((ms - 1) % 1000 + 1).unwrap_or(1000)));
        }
        egui::Rect::from_min_max(row.min, egui::pos2(at.x - t.spacing.item_x, row.bottom()))
    }

    /// Envoie le message en cours, ou explique pourquoi il ne part pas (le
    /// texte reste alors dans le champ).
    pub(super) fn submit(&mut self) {
        let Some(channel_id) = self.store.channel else { return };
        let mut text = self.input.draft.encoded().trim().to_string();
        let files = self.has_attachments(channel_id);
        if text.is_empty() && !files {
            return;
        }
        // Commandes intégrées (F10) : `/shrug`, `/me`… réécrivent le texte, `/nick` agit.
        match crate::state::builtin::apply(&text) {
            Some(crate::state::builtin::Builtin::Text(t)) => text = t,
            Some(crate::state::builtin::Builtin::Nick(nick)) => return self.builtin_nick(nick),
            None => {}
        }
        // `@silent` seul : rien à envoyer.
        if crate::state::builtin::send_opts(&text).text.is_empty() && !files {
            return;
        }
        if text.chars().count() > MESSAGE_MAX {
            self.notify(tx::MESSAGE_TOO_LONG);
            return;
        }
        if let Some(ms) = self.store.slowmode_left(channel_id, self.store.now()) {
            self.notify(f::slowmode_refused(self.lang, duration(self.lang, ms)));
            return;
        }
        // Avec pièces jointes (lot T8) : la raison d'un refus est déjà affichée.
        if files {
            if !self.send_with_files(channel_id, text) {
                return;
            }
        // Réponse en préparation : envoyée comme réponse (lot T6).
        } else if !self.send_text(channel_id, text) {
            self.notify(tx::CANNOT_SEND);
            return;
        }
        self.input.draft.clear();
        self.input.cache = None;
        self.input.recount();
        // Le message envoyé met fin à l'indicateur chez les autres.
        self.input.typing_sent = None;
        self.scroll_to_bottom = true;
    }
}

impl App {
    /// `/nick` : change mon pseudo sur le serveur affiché (Modify Current
    /// Member, documenté ; même chemin que la page « Profils »).
    fn builtin_nick(&mut self, nick: Option<String>) {
        let Some(guild) = self.store.guild else {
            return self.notify(crate::i18n::table::ff::t::NICK_ONLY_SERVER);
        };
        match self.store.account.set_nick(guild, nick) {
            Ok(()) => {
                self.input.draft.clear();
                self.input.cache = None;
                self.input.recount();
                self.flush_commands();
                self.notify(crate::i18n::table::ff::t::NICK_SENT);
            }
            Err(r) => self.refused(r),
        }
    }

    /// Insère `text` au curseur du champ (à la fin s'il n'y en a pas), et y
    /// remet le curseur juste après (sélecteur d'emojis).
    pub(super) fn insert_in_composer(&mut self, text: &str) {
        let id = composer_id();
        let mut st = egui::text_edit::TextEditState::load(&self.ctx, id).unwrap_or_default();
        let draft = &mut self.input.draft.text;
        let len = draft.chars().count();
        let at = st.cursor.char_range().map_or(len, |r| r.primary.index.0.min(len));
        let byte = draft.char_indices().nth(at).map_or(draft.len(), |(b, _)| b);
        draft.insert_str(byte, text);
        let end = at + text.chars().count();
        st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(end))));
        st.store(&self.ctx, id);
        self.ctx.memory_mut(|m| m.request_focus(id));
        self.input.cache = None;
        self.input.recount();
    }

    /// Texte en cours tel qu'il serait envoyé (messages programmés, lot E3).
    pub(super) fn composer_encoded(&self) -> String {
        self.input.draft.encoded().trim().to_string()
    }

    /// Vide le champ : son texte a été rangé ailleurs (messages programmés, lot E3).
    pub(super) fn clear_composer(&mut self) {
        self.input.draft.clear();
        self.input.cache = None;
        self.input.recount();
        self.input.typing_sent = None;
    }

    /// Nombre de caractères du texte en cours.
    pub(super) fn input_chars(&self) -> usize {
        self.input.draft.text.chars().count()
    }

    /// Caractères encore libres avant la limite d'un message.
    pub(super) fn composer_room(&self) -> usize {
        MESSAGE_MAX.saturating_sub(self.input.length)
    }

    /// Retire `a..b` (caractères) du texte en cours et le rend (« Couper »).
    pub(super) fn cut_composer(&mut self, a: usize, b: usize) -> String {
        let cut = super::composer_menu::cut_range(&mut self.input.draft.text, a, b);
        self.input.cache = None;
        self.input.recount();
        cut
    }

    /// Lie un texte lisible (`:blob:`) à sa syntaxe (`<:blob:id>`) pour l'envoi.
    pub(super) fn input_link(&mut self, shown: String, syntax: String) -> bool {
        self.input.draft.link(shown, syntax)
    }
}

/// Entoure (ou libère, si elle l'est déjà) la sélection `start..end`
/// (en caractères) de `mark`. Renvoie le texte et la sélection qui suit le
/// même passage. `None` : sélection vide ou hors du texte.
pub(super) fn wrap_selection(text: &str, start: usize, end: usize, mark: &str) -> Option<(String, usize, usize)> {
    let (start, end) = (start.min(end), start.max(end));
    let byte = |n: usize| text.char_indices().nth(n).map_or(text.len(), |(i, _)| i);
    if start == end || end > text.chars().count() {
        return None;
    }
    let (a, b) = (byte(start), byte(end));
    let (before, sel, after) = (&text[..a], &text[a..b], &text[b..]);
    let n = mark.chars().count();
    // « * » collé à « ** » : c'est du gras, pas de l'italique à retirer.
    let lone = |around: &str, end: bool| mark != "*" || !(if end { around.ends_with("**") } else { around.starts_with("**") });
    // Marques dans la sélection, ou juste autour : on les retire.
    if sel.len() > 2 * mark.len() && sel.starts_with(mark) && sel.ends_with(mark) {
        let inner = &sel[mark.len()..sel.len() - mark.len()];
        return Some((format!("{before}{inner}{after}"), start, end - 2 * n));
    }
    if before.ends_with(mark) && after.starts_with(mark) && lone(before, true) && lone(after, false) {
        return Some((format!("{}{sel}{}", &before[..before.len() - mark.len()], &after[mark.len()..]), start - n, end - n));
    }
    Some((format!("{before}{mark}{sel}{mark}{after}"), start + n, end + n))
}

impl App {
    /// Ctrl+B, Ctrl+I, Ctrl+U sur le texte sélectionné de la saisie : gras,
    /// italique, souligné (comme Discord). Sans sélection, les touches
    /// gardent leur rôle général (boîte de réception, liste des membres).
    pub(super) fn format_keys(&mut self, ui: &egui::Ui) {
        let id = composer_id();
        if !ui.memory(|m| m.has_focus(id)) {
            return;
        }
        let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), id) else { return };
        let Some(range) = state.cursor.char_range().filter(|r| r.primary.index != r.secondary.index) else { return };
        let mark = ui.input_mut(|i| [(Key::B, "**"), (Key::I, "*"), (Key::U, "__")].into_iter().find(|(k, _)| i.consume_key(egui::Modifiers::COMMAND, *k)).map(|(_, m)| m));
        let Some(mark) = mark else { return };
        let Some((text, a, b)) = wrap_selection(&self.input.draft.text, range.primary.index.0, range.secondary.index.0, mark) else { return };
        self.input.draft.text = text;
        self.input.cache = None;
        self.input.recount();
        state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
        egui::TextEdit::store_state(ui.ctx(), id, state);
    }
}

/// Barre de mise en forme affichée : passage visé (en caractères), point
/// d'ancrage (haut du début de la sélection) et place de la barre.
#[derive(Clone, Copy)]
struct FormatBar {
    range: (usize, usize),
    anchor: egui::Pos2,
    rect: egui::Rect,
}

fn format_bar_id() -> Id {
    Id::unique("ferrocord.saisie.mise-en-forme")
}

/// Boutons de la barre : marque, texte du bouton, nom lu et affiché au survol.
const FORMATS: [(&str, &str, crate::i18n::Text); 5] = [("**", "B", ct::FMT_BOLD), ("*", "I", ct::FMT_ITALIC), ("~~", "S", ct::FMT_STRIKE), ("`", "<>", ct::FMT_CODE), ("||", "||", ct::FMT_SPOILER)];

impl App {
    /// Barre de mise en forme au-dessus du texte sélectionné dans la saisie :
    /// gras, italique, barré, code, spoiler. Un bouton entoure la sélection
    /// de sa marque, ou la retire si elle y est déjà (`wrap_selection`).
    fn format_bar(&mut self, ui: &egui::Ui, id: Id, galley: &egui::Galley, galley_pos: egui::Pos2, cursor: Option<egui::text::CCursorRange>, t: &Tokens) {
        let ctx = ui.ctx().clone();
        let kept: Option<FormatBar> = ctx.data(|d| d.get_temp(format_bar_id()));
        let pointer = ctx.input(|i| i.pointer.latest_pos());
        let over = kept.is_some_and(|k| pointer.is_some_and(|p| k.rect.expand(4.0).contains(p)));
        let selected = cursor.filter(|r| r.primary.index != r.secondary.index && ui.memory(|m| m.has_focus(id)));
        let bar = match selected {
            // Pendant que la sélection se fait à la souris, la barre attend.
            Some(_) if !over && ctx.input(|i| i.pointer.primary_down()) => None,
            Some(r) => {
                let first = if r.primary.index <= r.secondary.index { r.primary } else { r.secondary };
                let at = galley.pos_from_cursor(first).translate(galley_pos.to_vec2());
                Some(FormatBar { range: (r.primary.index.0, r.secondary.index.0), anchor: at.left_top(), rect: kept.map_or(egui::Rect::NOTHING, |k| k.rect) })
            }
            // Le clic sur la barre retire le clavier au champ : elle reste tant que le pointeur est dessus.
            None => kept.filter(|_| over),
        };
        let Some(mut bar) = bar else {
            if kept.is_some() {
                ctx.data_mut(|d| d.remove::<FormatBar>(format_bar_id()));
            }
            return;
        };
        let lang = self.lang;
        let mut picked = None;
        let height = super::widgets::button_height(t);
        let pos = egui::pos2(bar.anchor.x, bar.anchor.y - height - 2.0 * t.spacing.item_y - 4.0);
        let area = egui::Area::new(format_bar_id()).order(egui::Order::Foreground).fixed_pos(pos).constrain_to(ctx.content_rect()).show(&ctx, |ui| {
            egui::Frame::new().fill(t.colors.bg3).corner_radius(t.r_control()).stroke(egui::Stroke::new(t.shapes.border.max(1.0), t.colors.border)).inner_margin(egui::Margin::same(t.spacing.item_y as i8)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = t.spacing.item_y;
                    for (mark, glyph, name) in FORMATS {
                        let text = match mark {
                            "**" => RichText::new(glyph).strong(),
                            "*" => RichText::new(glyph).italics(),
                            "~~" => RichText::new(glyph).strikethrough(),
                            _ => RichText::new(glyph).monospace(),
                        };
                        let b = ui.add(egui::Button::new(text.color(t.colors.text_strong)).frame(false).min_size(egui::Vec2::splat(height)));
                        super::a11y::name(ui.ctx(), b.id, name.get(lang));
                        if b.on_hover_text(name.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            picked = Some(mark);
                        }
                    }
                });
            });
        });
        bar.rect = area.response.rect;
        if let Some(mark) = picked
            && let Some((text, a, b)) = wrap_selection(&self.input.draft.text, bar.range.0, bar.range.1, mark)
        {
            self.input.draft.text = text;
            self.input.cache = None;
            self.input.recount();
            bar.range = (a, b);
            let mut state = egui::TextEdit::load_state(&ctx, id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(a), egui::text::CCursor::new(b))));
            egui::TextEdit::store_state(&ctx, id, state);
            ctx.memory_mut(|m| m.request_focus(id));
            ctx.request_repaint();
        }
        ctx.data_mut(|d| d.insert_temp(format_bar_id(), bar));
    }
}

fn composer_id() -> Id {
    Id::unique("composer")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selection_is_wrapped_and_unwrapped() {
        assert_eq!(wrap_selection("un deux", 3, 7, "**"), Some(("un **deux**".into(), 5, 9)));
        assert_eq!(wrap_selection("un **deux**", 9, 5, "**"), Some(("un deux".into(), 3, 7)), "marques autour, sélection à l'envers");
        assert_eq!(wrap_selection("**été**", 0, 7, "**"), Some(("été".into(), 0, 3)), "marques dans la sélection");
        assert_eq!(wrap_selection("**a**", 2, 3, "*"), Some(("***a***".into(), 3, 4)), "italique sur du gras : rien n'est retiré");
        assert_eq!(wrap_selection("été", 1, 2, "__"), Some(("é__t__é".into(), 3, 4)));
        assert_eq!(wrap_selection("abc", 1, 1, "*"), None);
        assert_eq!(wrap_selection("abc", 1, 9, "*"), None);
    }

    #[test]
    fn durations() {
        assert_eq!(duration(Lang::Fr, 6_200), "7\u{a0}s");
        assert_eq!(duration(Lang::En, 65_000), "1 min 5 s");
        assert_eq!(duration(Lang::Fr, 9_000_000), "2\u{a0}h 30\u{a0}min");
        assert_eq!(duration(Lang::En, -5), "0 s");
    }

    #[test]
    fn typing_wording() {
        assert_eq!(typing_text(Lang::Fr, &["Alice"]), "Alice écrit…");
        assert_eq!(typing_text(Lang::En, &["Alice", "Bob"]), "Alice and Bob are typing…");
        assert_eq!(typing_text(Lang::Fr, &["A", "B", "C"]), "A, B et C écrivent…");
        assert_eq!(typing_text(Lang::En, &["A", "B", "C", "D"]), "Several people are typing…");
    }

    use crate::state::model::{Channel, ChannelKind, Guild, User};
    use crate::state::{Event, Store};

    const GUILD: u64 = 9;
    const CHAN: u64 = 100;
    const ME: u64 = 1;

    struct Offline;

    impl super::super::Backend for Offline {
        fn set_wake(&mut self, _: super::super::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, _: Vec<Cmd>) {}
    }

    /// Application sans fenêtre sur `CHAN` ; les commandes restent dans le magasin.
    struct Harness {
        app: App,
        time: f64,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let mut store = Store::new();
            let chan = |id: u64| Channel { id, name: format!("s{id}"), kind: ChannelKind::Text, position: id as i32, parent: None, overwrites: Some(Vec::new()), slowmode: 0 };
            let guild = Guild::new(GUILD, "G".into(), vec![chan(CHAN), chan(CHAN + 1)]);
            store.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![guild], dms: Vec::new() });
            store.select_guild(GUILD);
            store.take_commands();
            let images = crate::images::Images::disabled(ctx.clone());
            let mut app = App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), store, images));
            app.theme = Tokens::default();
            Harness { app, time: 0.0 }
        }

        fn frame(&mut self, events: Vec<egui::Event>) {
            let ctx = self.app.ctx.clone();
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0))),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| self.app.main_ui(ui));
            out.textures_delta.clear();
        }

        fn key(&mut self, key: Key, shift: bool) {
            let ev = egui::Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers { shift, ..Default::default() } };
            self.frame(vec![ev]);
        }

        fn sent(&mut self) -> Vec<Cmd> {
            self.app.store.take_commands().into_iter().filter(|c| matches!(c, Cmd::Send { .. } | Cmd::Typing { .. })).collect()
        }
    }

    fn content(c: &Cmd) -> Option<&str> {
        match c {
            Cmd::Send { content, .. } => Some(content),
            _ => None,
        }
    }

    #[test]
    fn enter_sends_and_shift_enter_adds_a_line() {
        let mut h = Harness::new();
        h.app.set_composer_text("bonjour");
        h.frame(Vec::new());
        h.key(Key::Enter, true);
        assert_eq!(h.app.composer_text(), "bonjour\n");
        h.frame(vec![egui::Event::Text("à tous".into())]);
        h.sent();
        h.key(Key::Enter, false);
        let sent = h.sent();
        assert_eq!(sent.iter().filter_map(content).collect::<Vec<_>>(), vec!["bonjour\nà tous"]);
        assert_eq!(h.app.composer_text(), "");
    }

    #[test]
    fn typing_is_signalled_at_most_every_eight_seconds_unless_disabled() {
        let mut h = Harness::new();
        h.app.set_composer_text("");
        h.frame(Vec::new());
        h.frame(vec![egui::Event::Text("a".into())]);
        h.frame(vec![egui::Event::Text("b".into())]);
        assert_eq!(h.sent(), vec![Cmd::Typing { channel_id: CHAN }]);
        // Envoyé : le prochain caractère repart.
        h.key(Key::Enter, false);
        h.sent();
        h.frame(vec![egui::Event::Text("c".into())]);
        assert_eq!(h.sent(), vec![Cmd::Typing { channel_id: CHAN }]);
        // Réglage coupé : rien.
        h.app.input.clear();
        h.app.theme.layout.typing = TypingSignal::Never;
        h.app.set_composer_text("");
        h.frame(Vec::new());
        h.frame(vec![egui::Event::Text("d".into())]);
        assert!(h.sent().is_empty());
    }

    /// Suggestions d'autocollants (F17) : un mot seul qui commence le nom
    /// d'un autocollant du serveur le propose ; un clic l'envoie et vide la
    /// saisie ; la croix l'écarte pour ce mot ; le réglage coupe tout.
    #[test]
    fn sticker_is_suggested_for_the_typed_word_and_sent_on_click() {
        use crate::state::emoji::{GuildSticker, StickerEvent, StickerFormat};
        let mut h = Harness::new();
        let stickers = vec![GuildSticker { id: 77, name: "Coucou".into(), format: StickerFormat::Png, available: true, tags: "wave".into() }];
        h.app.store.apply(Event::Stickers(StickerEvent::Guild { guild_id: GUILD, stickers }));
        let shown = |h: &Harness| super::super::sticker_hints::shown(&h.app.ctx);
        let click = |h: &mut Harness, at: egui::Pos2| {
            h.frame(vec![egui::Event::PointerMoved(at)]);
            h.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: true, modifiers: Default::default() }]);
            h.frame(vec![egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed: false, modifiers: Default::default() }]);
            h.frame(Vec::new());
        };
        h.app.set_composer_text("bonjour cou");
        h.frame(Vec::new());
        h.frame(Vec::new());
        assert!(shown(&h).is_none(), "deux mots : rien");
        h.app.set_composer_text("wav");
        h.frame(Vec::new());
        h.frame(Vec::new());
        assert!(shown(&h).is_some(), "étiquette");
        // La croix (à droite) écarte la suggestion pour ce mot.
        let r = shown(&h).unwrap();
        click(&mut h, egui::pos2(r.right() - 18.0, r.center().y));
        assert!(shown(&h).is_none());
        assert_eq!(h.app.composer_text(), "wav");
        // Réglage coupé : rien ; remis : la suggestion revient pour un autre mot.
        h.app.local.prefs.sticker_hints = false;
        h.app.set_composer_text("cou");
        h.frame(Vec::new());
        h.frame(Vec::new());
        assert!(shown(&h).is_none());
        h.app.local.prefs.sticker_hints = true;
        h.frame(Vec::new());
        h.frame(Vec::new());
        let r = shown(&h).expect("suggestion pour « cou »");
        h.app.store.take_commands();
        click(&mut h, egui::pos2(r.left() + 30.0, r.center().y));
        let sent = h.app.store.take_commands();
        assert!(sent.iter().any(|c| matches!(c, Cmd::Social(crate::state::social::SocialCmd::Send { kind: crate::state::social::SendKind::Sticker { sticker_id: 77 }, .. }))), "{sent:?}");
        assert_eq!(h.app.composer_text(), "");
        assert!(shown(&h).is_none());
    }

    #[test]
    fn drafts_follow_the_channel() {
        let mut h = Harness::new();
        h.app.set_composer_text("à finir");
        h.frame(Vec::new());
        h.app.store.select_channel(CHAN + 1);
        h.frame(Vec::new());
        assert_eq!(h.app.composer_text(), "");
        assert!(h.app.input.has_draft(CHAN) && !h.app.input.has_draft(CHAN + 1));
        h.app.store.select_channel(CHAN);
        h.frame(Vec::new());
        assert_eq!(h.app.composer_text(), "à finir");
        assert!(!h.app.input.has_draft(CHAN));
    }

    #[test]
    fn suggestion_is_inserted_readable_and_sent_as_syntax() {
        let mut h = Harness::new();
        h.app.set_composer_text("salut @Mo");
        h.frame(Vec::new());
        h.frame(Vec::new());
        assert!(h.app.input.popup_open);
        h.key(Key::Enter, false);
        assert_eq!(h.app.composer_text(), "salut @Moi ");
        assert!(h.sent().iter().all(|c| content(c).is_none()), "Entrée choisit, n'envoie pas");
        h.frame(Vec::new());
        h.key(Key::Enter, false);
        assert_eq!(h.sent().iter().filter_map(content).collect::<Vec<_>>(), vec!["salut <@1>"]);
        // Échap ferme la liste sans rien insérer.
        h.app.set_composer_text("@Mo");
        h.frame(Vec::new());
        h.frame(Vec::new());
        h.key(Key::Escape, false);
        h.frame(Vec::new());
        assert!(!h.app.input.popup_open);
        assert_eq!(h.app.composer_text(), "@Mo");
    }

    /// Réponse en préparation et autocomplétion ouverte : le premier Échap
    /// ferme la liste, le second annule la réponse.
    #[test]
    fn escape_closes_the_popup_before_the_reply() {
        let mut h = Harness::new();
        h.app.set_composer_text("@Mo");
        h.frame(Vec::new());
        // Après la première image (qui oublie les actions d'un autre compte).
        h.app.actions.reply = Some(super::super::msg_actions::ReplyDraft { channel_id: CHAN, message_id: 1, author: "Moi".into(), mention: true });
        h.frame(Vec::new());
        assert!(h.app.input.popup_open);
        h.key(Key::Escape, false);
        h.frame(Vec::new());
        assert!(!h.app.input.popup_open);
        assert!(h.app.actions.reply.is_some(), "la réponse reste");
        assert!(h.app.ctx.memory(|m| m.has_focus(composer_id())), "la saisie garde le focus");
        h.key(Key::Escape, false);
        h.frame(Vec::new());
        assert!(h.app.actions.reply.is_none());
        assert_eq!(h.app.composer_text(), "@Mo");
    }

    #[test]
    fn too_long_or_slowmode_is_refused_and_explained() {
        let mut h = Harness::new();
        h.app.set_composer_text(&"é".repeat(MESSAGE_MAX + 1));
        h.frame(Vec::new());
        h.key(Key::Enter, false);
        assert!(h.sent().iter().all(|c| content(c).is_none()));
        assert_eq!(h.app.composer_text().chars().count(), MESSAGE_MAX + 1);
        assert!(h.app.notice().is_some());
        // Mode lent : le premier part, le second attend.
        if let Some(c) = h.app.store.guilds[0].channels.iter_mut().find(|c| c.id == CHAN) {
            c.slowmode = 60;
        }
        h.app.toast = None;
        h.app.set_composer_text("un");
        h.frame(Vec::new());
        h.key(Key::Enter, false);
        h.app.set_composer_text("deux");
        h.frame(Vec::new());
        h.key(Key::Enter, false);
        assert_eq!(h.sent().iter().filter_map(content).collect::<Vec<_>>(), vec!["un"]);
        assert_eq!(h.app.composer_text(), "deux");
        assert!(h.app.notice().is_some_and(|n| n.contains("Mode lent") || n.contains("Slowmode")), "{:?}", h.app.notice());
    }

    /// Pièces jointes (lot T8) : préparées hors du fil d'interface, envoyées
    /// avec le texte par Entrée ; un fichier trop gros reste dans la saisie.
    #[test]
    fn attachments_are_sent_with_the_text_or_kept_when_refused() {
        use crate::state::upload::{UploadCmd, DEFAULT_LIMIT};
        let mut h = Harness::new();
        h.app.set_composer_text("");
        h.frame(Vec::new());
        h.app.attach_bytes("a.txt", b"bonjour".to_vec());
        let start = Instant::now();
        while h.app.attachment_names().is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
            h.frame(Vec::new());
        }
        assert_eq!(h.app.attachment_names(), vec!["a.txt"]);
        // Fichiers seuls : Entrée envoie quand même.
        h.key(Key::Enter, false);
        let cmds = h.app.store.take_commands();
        let sent: Vec<_> = cmds.iter().filter_map(|c| if let Cmd::Upload(UploadCmd::Send { content, files, .. }) = c { Some((content.clone(), files.len())) } else { None }).collect();
        assert_eq!(sent, vec![(String::new(), 1)]);
        assert!(h.app.attachment_names().is_empty());
        // Trop gros pour le compte (profil connu, sans Nitro) : rien ne part.
        h.app.store.apply(Event::Account(crate::state::usersettings::AccountEvent::Profile(Ok(Default::default()))));
        h.app.attach_bytes("gros.bin", vec![0; DEFAULT_LIMIT as usize + 1]);
        let start = Instant::now();
        while h.app.attachment_names().is_empty() && start.elapsed() < Duration::from_secs(10) {
            std::thread::sleep(Duration::from_millis(5));
            h.frame(Vec::new());
        }
        h.app.set_composer_text("avec texte");
        h.frame(Vec::new());
        h.key(Key::Enter, false);
        assert!(h.app.store.take_commands().iter().all(|c| !matches!(c, Cmd::Upload(_))));
        assert_eq!(h.app.attachment_names(), vec!["gros.bin"]);
        assert_eq!(h.app.composer_text(), "avec texte");
        assert!(h.app.notice().is_some_and(|n| n.contains("gros.bin")), "{:?}", h.app.notice());
    }

    #[test]
    fn plain_enter_only() {
        let key = |shift| egui::Event::Key { key: Key::Enter, physical_key: None, pressed: true, repeat: false, modifiers: egui::Modifiers { shift, ..Default::default() } };
        let mut i = egui::InputState::default();
        i.events = vec![key(true)];
        assert!(!take_plain_key(&mut i, Key::Enter));
        assert_eq!(i.events.len(), 1);
        i.events = vec![key(false)];
        assert!(take_plain_key(&mut i, Key::Enter));
        assert!(i.events.is_empty());
    }
}
