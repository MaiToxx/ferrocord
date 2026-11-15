//! Zone de discussion : en-tête, historique, saisie.
//!
//! L'historique est une liste virtualisée (règle 1.3.2) : seuls les messages
//! visibles sont mis en page et dessinés. La hauteur de chaque message est
//! mémorisée et ne se refait que si ce qui la détermine change (texte,
//! regroupement, séparateurs, largeur, thème, zoom). Un message jamais vu a
//! une hauteur estimée ; il est mesuré (mise en page invisible) avant
//! d'entrer dans la vue, ce qui évite tout saut. La vue est accrochée à un
//! message (`vlist::Anchor`) : ce qui change au-dessus d'elle ne la déplace pas.

use super::{
    message::{measure_message, message_ui_acts},
    vlist::{self, Anchor, Geometry, Pacer},
    App,
};
use super::msg_actions::Acts;
use super::words;
use crate::i18n::{f, t as tx, time, Lang};
use crate::state::markdown::{parse_message, Doc};
use crate::state::model::{snowflake_ms, Message};
use crate::state::store::{ChanState, Rev};
use crate::state::{Problem, Status, UnsentState};
use crate::theme::{icons, icons::Icon, Clock, Tokens};
use chrono::{Datelike, Local, NaiveDate, TimeZone};
use eframe::egui::{self, Id, RichText};
use std::{
    collections::HashMap,
    hash::{DefaultHasher, Hash, Hasher},
    sync::Arc,
};

/// Deux messages du même auteur à moins de 7 min sont regroupés.
const GROUP_WINDOW_MS: i64 = 7 * 60 * 1000;

/// Ce qui, outre le texte, change la hauteur d'un message.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Flags {
    grouped: bool,
    /// Séparateur de date au-dessus.
    day_sep: bool,
    /// Ligne « Nouveaux messages » au-dessus.
    new_line: bool,
    /// Message d'une personne bloquée ou ignorée (F1).
    hide: Hide,
}

/// Place d'un message dans une suite de messages masqués (personne bloquée
/// ou ignorée) : le premier porte la ligne « N messages bloqués — Afficher »,
/// les suivants n'occupent aucune hauteur tant que la suite est repliée.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Hide {
    #[default]
    No,
    Head { n: u16, ignored: bool, open: bool },
    Inside { open: bool },
}

impl Hide {
    /// Le message lui-même est-il dessiné ?
    fn shows_message(self) -> bool {
        !matches!(self, Hide::Head { open: false, .. } | Hide::Inside { open: false })
    }
}

/// Ce que l'on sait d'un message : empreinte du contenu, jour local, et
/// hauteur mesurée (valable pour ces drapeaux-là).
struct Entry {
    fp: u64,
    day: i32,
    measured: Option<(Flags, f32)>,
    /// Texte mis en forme, analysé à la première mise en page ; oublié quand
    /// le texte change (empreinte), ou la langue, l'horloge ou le thème.
    doc: Option<Arc<Doc>>,
}

/// En-tête et pied de la liste. Le texte d'une erreur est gardé à part
/// ([`Worded`]), mis en mots une seule fois.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Block {
    Pad,
    Loading,
    Error,
    Denied,
    Empty,
    Start,
}

/// Texte mis en mots une fois, refait seulement quand la valeur, la langue
/// ou l'horloge change (ni allocation ni lecture de l'heure à chaque image).
pub(super) struct Worded<K> {
    key: Option<(K, Lang, bool)>,
    text: String,
}

impl<K> Default for Worded<K> {
    fn default() -> Self {
        Worded { key: None, text: String::new() }
    }
}

impl<K: Clone + PartialEq> Worded<K> {
    /// Met le texte à jour pour `value` (vide si absente) ; vrai s'il a changé.
    fn update(&mut self, value: Option<&K>, say: Say, word: impl FnOnce(&K) -> String) -> bool {
        let same = match (&self.key, value) {
            (Some((k, lang, h24)), Some(v)) => k == v && *lang == say.lang && *h24 == say.h24,
            (None, None) => true,
            _ => false,
        };
        if same {
            return false;
        }
        match value {
            Some(v) => {
                self.text = word(v);
                self.key = Some((v.clone(), say.lang, say.h24));
            }
            None => {
                self.text.clear();
                self.key = None;
            }
        }
        true
    }

    fn text(&self) -> &str {
        &self.text
    }
}

impl Block {
    fn head(cs: &ChanState) -> Block {
        if cs.error.is_some() {
            Block::Error
        } else if (cs.loading && !cs.loaded) || cs.loading_older() {
            Block::Loading
        } else if cs.history_denied {
            Block::Denied
        } else if cs.loaded && cs.msgs.is_empty() {
            Block::Empty
        } else if cs.loaded && !cs.more {
            Block::Start
        } else {
            Block::Pad
        }
    }

    /// Pied : l'échec et l'attente des pages plus récentes et du retour au présent.
    fn foot(cs: &ChanState) -> Block {
        if cs.error_newer.is_some() {
            Block::Error
        } else if cs.loading_newer() || (cs.loading && cs.loaded) {
            Block::Loading
        } else {
            Block::Pad
        }
    }
}

/// État de la liste d'un salon : géométrie, hauteurs connues, ancre.
struct ChanView {
    geo: Geometry,
    flags: Vec<Flags>,
    measured: Vec<bool>,
    cache: HashMap<u64, Entry>,
    /// Version du cache du magasin, largeur et ligne des nouveaux messages
    /// pour lesquelles `geo` est à jour.
    synced: Option<(Rev, f32, Option<u64>, u64)>,
    /// Suites de messages masqués dépliées (identifiant du premier message).
    revealed: std::collections::HashSet<u64>,
    /// Change à chaque dépliage ou repliage.
    reveal_rev: u64,
    anchor: Anchor,
    head: Option<Block>,
    foot: Option<Block>,
    /// Textes des erreurs de l'en-tête et du pied.
    head_error: Worded<Problem>,
    foot_error: Worded<Problem>,
    /// Plus récent message déjà vu (compteur du bouton « nouveaux messages »).
    seen: u64,
    /// Sommes et nombres des hauteurs mesurées (en tête de groupe, dans un groupe).
    est: [(f32, u32); 2],
    /// Hauteur de la vue à l'image précédente.
    last_h: f32,
    /// Reste de molette sous le pixel, reporté à l'image suivante (voir `list_ui`).
    wheel_frac: f32,
    /// Zoom du salon et pixels par point pour lesquels les hauteurs valent.
    scale: (f32, f32),
    /// Pages demandées automatiquement : vers le passé, vers le présent.
    pace: [Pacer; 2],
}

impl ChanView {
    fn new() -> Self {
        ChanView {
            geo: Geometry::default(),
            flags: Vec::new(),
            measured: Vec::new(),
            cache: HashMap::new(),
            synced: None,
            revealed: std::collections::HashSet::new(),
            reveal_rev: 0,
            anchor: Anchor::Bottom,
            head: None,
            foot: None,
            head_error: Worded::default(),
            foot_error: Worded::default(),
            seen: 0,
            est: [(0.0, 0); 2],
            last_h: 0.0,
            wheel_frac: 0.0,
            pace: [Pacer::default(); 2],
            scale: (1.0, 0.0),
        }
    }

    /// Message `i` de la géométrie et ses drapeaux, s'il est bien celui du
    /// magasin à ce rang (jamais d'accès hors bornes, même géométrie périmée).
    fn item<'a>(&self, cs: &'a ChanState, i: usize) -> Option<(&'a Message, Flags)> {
        let m = cs.msgs.get(i)?;
        let f = *self.flags.get(i)?;
        (self.geo.ids().get(i) == Some(&m.id)).then_some((m, f))
    }

    /// Oublie les hauteurs (largeur, thème ou zoom changés).
    fn forget_heights(&mut self) {
        for e in self.cache.values_mut() {
            e.measured = None;
        }
        self.est = [(0.0, 0); 2];
        self.synced = None;
        self.head = None;
        self.foot = None;
    }

    /// Oublie les textes mis en forme (langue, horloge ou thème changés).
    fn forget_docs(&mut self) {
        for e in self.cache.values_mut() {
            e.doc = None;
        }
    }

    /// Texte mis en forme du message (analysé ici la première fois).
    /// Un horodatage relatif périmé (`Doc::refresh_at_ms` passé) le fait réanalyser.
    fn doc(&mut self, m: &Message, now_ms: i64, parse: impl FnOnce() -> Doc) -> Arc<Doc> {
        match self.cache.get_mut(&m.id) {
            Some(e) => {
                if e.doc.as_ref().is_some_and(|d| d.refresh_at_ms.is_some_and(|t| t <= now_ms)) {
                    e.doc = None;
                }
                e.doc.get_or_insert_with(|| Arc::new(parse())).clone()
            }
            None => Arc::new(parse()),
        }
    }

    fn estimate(&self, grouped: bool, fallback: f32) -> f32 {
        let (sum, n) = self.est[usize::from(grouped)];
        if n == 0 { fallback } else { sum / n as f32 }
    }

    fn record(&mut self, grouped: bool, h: f32) {
        let e = &mut self.est[usize::from(grouped)];
        e.0 += h;
        e.1 += 1;
    }

    /// Refait la géométrie si le cache du salon, la largeur ou la ligne des
    /// nouveaux messages ont changé. Ne parcourt les messages que dans ce cas.
    /// `hidden` : la relation avec l'auteur le masque-t-elle (`Some(true)` :
    /// ignoré, `Some(false)` : bloqué) ; `rel_rev` change quand les relations changent.
    fn sync(&mut self, cs: &ChanState, width: f32, fallback: f32, ppp: f32, rel_rev: u64, hidden: &dyn Fn(u64) -> Option<bool>) {
        let key = (cs.rev, width, cs.new_marker, rel_rev.wrapping_mul(0x9E37_79B9).wrapping_add(self.reveal_rev));
        // La longueur aussi : une géométrie qui n'a pas autant d'éléments que
        // le magasin est refaite quelle que soit la version.
        if self.synced == Some(key) && self.geo.len() == cs.msgs.len() && self.flags.len() == cs.msgs.len() {
            return;
        }
        if self.synced.is_some_and(|(_, w, _, _)| w != width) {
            self.forget_heights();
        }
        self.synced = Some(key);
        let n = cs.msgs.len();
        let mut cache = HashMap::with_capacity(n);
        let (mut ids, mut heights) = (Vec::with_capacity(n), Vec::with_capacity(n));
        self.flags.clear();
        self.measured.clear();
        let mut prev: Option<(&Message, i32)> = None;
        // Suite masquée en cours : rang de son premier message, « ignoré ».
        let mut run: Option<(usize, bool)> = None;
        for m in &cs.msgs {
            let fp = fingerprint(m);
            let mut e = self.cache.remove(&m.id).unwrap_or_else(|| Entry { fp, day: local_day(snowflake_ms(m.id)), measured: None, doc: None });
            if e.fp != fp {
                e.fp = fp;
                e.measured = None;
                e.doc = None;
            }
            let day_sep = prev.is_none_or(|(_, d)| d != e.day);
            // Sur le premier message en mémoire, seulement s'il est le premier
            // non-lu lui-même : sinon le vrai premier non-lu n'est pas chargé et
            // la ligne mentirait.
            let new_line = cs.new_marker.is_some_and(|k| m.id >= k && prev.map_or(m.id == k, |(p, _)| p.id < k));
            // Une réponse commence toujours un groupe : sa ligne citée part de l'avatar.
            let grouped = !day_sep
                && !new_line
                && m.system.is_none()
                && m.meta.reply.is_none()
                && prev.is_some_and(|(p, _)| p.system.is_none() && p.author_id == m.author_id && snowflake_ms(m.id) - snowflake_ms(p.id) < GROUP_WINDOW_MS);
            // Suites de messages masqués : coupées par une date ou la ligne
            // « Nouveau », qui restent ainsi toujours visibles.
            let i = self.flags.len();
            let hide = match hidden(m.author_id) {
                None => {
                    run = None;
                    Hide::No
                }
                Some(ignored) => match run {
                    Some((head, kind)) if kind == ignored && !day_sep && !new_line => {
                        let open = match &mut self.flags[head].hide {
                            Hide::Head { n, open, .. } => {
                                *n = n.saturating_add(1);
                                *open
                            }
                            _ => false,
                        };
                        Hide::Inside { open }
                    }
                    _ => {
                        run = Some((i, ignored));
                        Hide::Head { n: 1, ignored, open: self.revealed.contains(&m.id) }
                    }
                },
            };
            let grouped = grouped && !matches!(hide, Hide::Head { .. });
            ids.push(m.id);
            self.flags.push(Flags { grouped, day_sep, new_line, hide });
            prev = Some((m, e.day));
            cache.insert(m.id, e);
        }
        // Hauteurs : après coup, le nombre de messages d'une suite n'étant
        // connu qu'à sa fin.
        for (m, flags) in cs.msgs.iter().zip(&self.flags) {
            let h = match cache.get(&m.id).and_then(|e| e.measured) {
                _ if flags.hide == (Hide::Inside { open: false }) => Some(0.0),
                Some((f, h)) if f == *flags => Some(h),
                _ => None,
            };
            // Estimation sur la grille des pixels, comme les hauteurs mesurées :
            // les hauts de tous les éléments y restent.
            heights.push(h.unwrap_or_else(|| (self.estimate(flags.grouped, fallback) * ppp).round() / ppp));
            self.measured.push(h.is_some());
        }
        self.cache = cache;
        self.geo.set(ids, heights);
    }
}

impl MessageList {
    /// Oublie les textes mis en forme de tous les salons (surlignage des mots-clés changé).
    pub(super) fn forget_docs(&mut self) {
        self.views.values_mut().for_each(ChanView::forget_docs);
    }

    /// Une liste existe pour ce salon : il a été dessiné (tests).
    #[cfg(test)]
    pub(super) fn has_view(&self, cid: u64) -> bool {
        self.views.contains_key(&cid)
    }
}

/// Listes de tous les salons en cache.
#[derive(Default)]
pub(super) struct MessageList {
    views: HashMap<u64, ChanView>,
    current: Option<u64>,
    /// Une liste a déjà été dessinée : la toute première attend une image (lot C9).
    started: bool,
    /// Thème et échelle pour lesquels les hauteurs sont valables.
    style: Option<(Tokens, (u64, super::attachments::Shown), Lang)>,
    /// Statut de la connexion mis en mots (en-tête, écran d'attente).
    status: Worded<Status>,
    /// Bande réservée à la notification sous la liste de cette image (la
    /// notification s'y place) ; aucune hors de la liste.
    pub(super) toast_slot: Option<egui::Rect>,
    /// Saut demandé (salon, message), en attente que le message soit en mémoire (lot T12).
    pending: Option<(u64, u64)>,
    /// Message surligné après un saut (salon, message, fin en secondes d'egui).
    flash: Option<(u64, u64, f64)>,
}

/// Durée du surlignage d'un message après un saut (s).
const FLASH: f64 = 2.0;

/// Gestes sur les messages (lots T5/T6, `msg_actions.rs`).
impl MessageList {
    /// Amène le message `id` dans la vue dès qu'il est en mémoire (tout de
    /// suite s'il y est, sinon à l'arrivée de sa page), puis le surligne un instant.
    /// Salon de la liste dessinée : remplacé le temps d'une discussion secondaire (lot E2).
    pub(super) fn swap_current(&mut self, cid: Option<u64>) -> Option<u64> {
        std::mem::replace(&mut self.current, cid)
    }

    pub(super) fn jump_to(&mut self, cid: u64, id: u64) {
        self.pending = Some((cid, id));
    }

    /// Saut en attente (salon, message).
    pub(super) fn pending_jump(&self) -> Option<(u64, u64)> {
        self.pending
    }

    pub(super) fn cancel_jump(&mut self) {
        self.pending = None;
    }

    /// La vue de `cid` montre le haut de la liste (« tirer pour charger », phase 7).
    pub(super) fn at_top(&self, cid: u64) -> bool {
        self.views.get(&cid).is_some_and(|v| v.last_h > 0.0 && v.geo.offset_for(v.anchor, v.last_h) <= 1.0)
    }

    /// Page haut / Page bas (lot navigation) : la vue monte ou descend de
    /// presque sa hauteur. `true` si elle touche alors le haut de la liste
    /// (la page précédente peut être demandée).
    pub(super) fn page(&mut self, cid: u64, up: bool) -> bool {
        let Some(view) = self.views.get_mut(&cid) else { return false };
        let h = view.last_h;
        if h <= 0.0 {
            return false;
        }
        let offset = view.geo.offset_for(view.anchor, h);
        let step = h * 0.9;
        let target = (if up { offset - step } else { offset + step }).clamp(0.0, view.geo.max_offset(h));
        view.anchor = view.geo.anchor_at(target, h, !up);
        up && target < h
    }

    /// Amène le message `id` entièrement dans la vue s'il en dépasse (sélection
    /// au clavier, lot C5) ; déjà visible, la vue ne bouge pas.
    pub(super) fn reveal(&mut self, cid: u64, id: u64) {
        let Some(view) = self.views.get_mut(&cid) else { return };
        let Some(i) = view.geo.ids().iter().position(|x| *x == id) else { return };
        let h = view.last_h;
        let offset = view.geo.offset_for(view.anchor, h);
        let (top, height) = (view.geo.top(i), view.geo.height(i));
        if top < offset || height > h {
            view.anchor = Anchor::At { id, delta: 0.0 };
        } else if top + height > offset + h {
            view.anchor = if i + 1 == view.geo.len() { Anchor::Bottom } else { Anchor::At { id, delta: height - h } };
        }
    }

    /// La hauteur du message `id` est à refaire (modification ouverte ou fermée).
    pub(super) fn remeasure(&mut self, cid: u64, id: u64) {
        if let Some(view) = self.views.get_mut(&cid)
            && let Some(e) = view.cache.get_mut(&id)
        {
            e.measured = None;
            view.synced = None;
        }
    }
}

impl App {
    /// Langue et horloge des textes de cette image.
    fn say(&self) -> Say {
        Say { lang: self.lang, h24: self.theme.layout.clock == Clock::H24 }
    }

    /// Messages non envoyés (ou non confirmés) du salon, au-dessus de la
    /// saisie : le texte n'est jamais perdu, l'utilisateur réessaie ou supprime.
    pub(super) fn unsent_list(&mut self, ui: &mut egui::Ui, cid: u64, can_send: bool) {
        let t = self.theme.clone();
        let say = self.say();
        let lang = say.lang;
        let Some(list) = self.store.unsent.get(&cid).filter(|l| !l.is_empty()) else { return };
        let (mut retry, mut discard) = (None, None);
        for u in list {
            let (status, color) = match &u.state {
                UnsentState::Failed(reason) => (f::not_sent_row(lang, words::problem(lang, reason, say.h24)), t.colors.danger),
                UnsentState::Unconfirmed => (tx::UNCONFIRMED_ROW.get(lang).to_string(), t.colors.muted),
                UnsentState::Retrying => (tx::RETRYING_ROW.get(lang).to_string(), t.colors.muted),
            };
            // Une ligne de la hauteur d'un contrôle : une mise en page centrée
            // verticalement prendrait sinon toute la hauteur libre de la fenêtre.
            let row = egui::vec2(ui.available_width(), ui.spacing().interact_size.y);
            ui.allocate_ui_with_layout(row, egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if crate::ui::widgets::compact(ui, tx::DELETE.get(lang)).clicked() {
                    discard = Some(u.nonce);
                }
                if can_send && u.state != UnsentState::Retrying && crate::ui::widgets::compact(ui, tx::RETRY.get(lang)).clicked() {
                    retry = Some(u.nonce);
                }
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.label(RichText::new(status).small().color(color));
                    ui.add(egui::Label::new(RichText::new(&u.content).color(t.colors.text)).truncate());
                });
            });
        }
        if let Some(n) = retry {
            self.store.retry_unsent(cid, n);
        }
        if let Some(n) = discard {
            self.store.discard_unsent(cid, n);
        }
        ui.add_space(t.spacing.chat_pad / 2.0);
    }

    /// Envois partis mais pas encore confirmés : leur texte, estompé, sous
    /// le fil (comme Discord), jusqu'à la réponse.
    pub(super) fn sending_list(&self, ui: &mut egui::Ui, cid: u64) {
        let t = &self.theme;
        for (_, nonce, text) in self.store.sending.iter().filter(|s| s.0 == cid) {
            let k = super::motion::appear(ui.ctx(), egui::Id::unique(("envoi", nonce)), super::motion::dur::HOVER);
            egui::Frame::new().inner_margin(t.avatar_indent()).show(ui, |ui| {
                ui.add(egui::Label::new(RichText::new(text).color(t.colors.text.gamma_multiply(super::motion::SENDING * k))).wrap());
            });
        }
    }

    pub(super) fn chat(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let say = self.say();
        let lang = say.lang;
        self.list.toast_slot = None;
        self.list.status.update(Some(&self.store.status), say, |s| words::status(lang, s, say.h24));
        let Some(cid) = self.store.channel else {
            ui.centered_and_justified(|ui| {
                let text = if self.store.me.is_none() { self.list.status.text() } else { tx::PICK_CHANNEL.get(lang) };
                ui.label(RichText::new(text).color(t.colors.muted));
            });
            return;
        };
        // Forum ou média : la liste des publications (lot T10).
        if self.store.is_forum(cid) {
            self.forum_view(ui, cid);
            return;
        }
        // Salon vocal ou appel : la vue d'appel (phase 5).
        if self.call_view_hook(ui, cid) {
            return;
        }
        let is_thread = self.store.is_thread(cid);
        let is_dm = self.store.guild.is_none();
        let name = if is_dm {
            self.store.dms.iter().find(|d| d.id == cid).map(|d| words::dm_name(lang, &d.name).to_string()).unwrap_or_default()
        } else {
            // Salon vocal : sa discussion textuelle (lot V6).
            self.store.channel_name(cid).map(str::to_string).or_else(|| self.voice_room_name(cid)).unwrap_or_default()
        };

        // Le statut n'apparaît que lorsqu'il y a quelque chose à signaler.
        let trouble = (self.store.status != Status::Connected).then(|| self.list.status.text().to_string());
        self.chat_head(ui, cid, &name, is_thread, is_dm, trouble.as_deref());
        // Appel du MP, ou salon vocal et conférence au-dessus de leur discussion (lot V6).
        if is_dm {
            self.call_banner(ui, cid);
        } else {
            self.room_head(ui, cid);
        }

        egui::Panel::bottom("composer")
            .frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(t.composer_margin()))
            // Même fond que la liste : pas de filet (il ne rejoindrait aucun autre).
            .show_separator_line(false)
            .show(ui, |ui| {
                super::look::backdrop(ui, super::look::Zone::Chat);
                let hint = if is_dm { f::message_dm(lang, &name) } else { f::message_channel(lang, &name) };
                self.composer_ui(ui, cid, hint);
            });

        // Panneau des épingles, puis gestes sur les messages appliqués après la liste (lots T5/T6).
        self.pins_panel(ui, cid);
        // Résumés du salon (lot ND).
        self.summaries_panel(ui, cid);
        self.saved_notices();
        // Résultats de recherche (lot T12).
        self.search_panel(ui);
        // Sélection des messages au clavier, avant la liste qui la montre (lot C5).
        self.message_keys(ui, cid);
        // Disposition mobile : les résultats de recherche couvrent la liste, qui n'est pas mise en page.
        let covered = self.mobile_active() && self.store.search.active.is_some();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2)).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Chat);
            if covered {
            } else if self.store.age_gated(cid) {
                // Salon à limite d'âge : l'écran d'entrée à la place des messages (F8).
                self.age_gate(ui, cid, &t);
            } else {
                super::attachments::set_sensitive(ui.ctx(), self.store.sensitive_media(cid));
                super::stealth::set_blur(ui.ctx(), self.stealth_blur(cid));
                self.message_list(ui, cid, &t);
            }
        });
        self.message_actions(ui, cid);
        self.create_thread_dialog(ui);
        // Fenêtre modale d'un bot (lot T11).
        self.app_modal_ui(ui);
        self.lot13_frame(ui, cid);
    }

    pub(super) fn message_list(&mut self, ui: &mut egui::Ui, cid: u64, t: &Tokens) {
        // Toute première liste de la session : glyphes et mises en page sont
        // froids, et le reste de l'écran (salons, en-tête, saisie) aussi. Elle
        // est dessinée à l'image suivante : deux images de 5 ms plutôt qu'une
        // de 10 à 14 (lot C9 ; invisible à l'œil, une image d'écart).
        // Les tests lisent l'écran dès la première image : sans ce report.
        if !self.list.started && !cfg!(test) {
            self.list.started = true;
            ui.ctx().request_repaint();
            return;
        }
        // Zoom du salon (plugin, lot E2) : Ctrl+molette sur la liste.
        self.zoom_wheel(ui, cid);
        let zoom = self.chat_zoom(cid);
        let say = self.say();
        let ppp = ui.pixels_per_point();
        let start = Start::of(&self.store, cid);
        let quick = super::msg_actions::quick_reactions(self.emojis.recent());
        let list = &mut self.list;
        // Le style d'egui suit le thème avec une image de retard : les deux comptent.
        // Les réglages « Texte et images » changent aussi la hauteur des messages.
        let applied = (style_fingerprint(ui.style()), super::attachments::shown(ui.ctx()));
        if list.style.as_ref().is_none_or(|(s, a, l)| s != t || *a != applied || *l != say.lang) {
            list.style = Some((t.clone(), applied, say.lang));
            list.views.values_mut().for_each(ChanView::forget_heights);
            list.views.values_mut().for_each(ChanView::forget_docs);
        }
        // Salons sortis du cache du magasin : leur liste part aussi.
        if list.views.len() > self.store.chans.len() {
            let chans = &self.store.chans;
            list.views.retain(|id, _| chans.contains_key(id));
        }
        let switched = list.current != Some(cid);
        list.current = Some(cid);
        let known = list.views.contains_key(&cid);
        let scroll_now = std::mem::take(&mut self.scroll_to_bottom);
        let Some(cs) = self.store.chans.get(&cid) else { return };
        let view = list.views.entry(cid).or_insert_with(ChanView::new);
        // Échelle de cette liste (zoom du salon, pixels par point de sa fenêtre) :
        // changée, ses hauteurs seules sont refaites (lot E2).
        if view.scale != (zoom, ppp) {
            view.scale = (zoom, ppp);
            view.forget_heights();
            view.forget_docs();
        }
        // Retour dans un salon encore en cache : la position est gardée.
        if scroll_now && !(switched && known) {
            view.anchor = Anchor::Bottom;
        }
        if !known {
            view.seen = cs.msgs.back().map_or(0, |m| m.id);
        }

        let full = ui.available_rect_before_wrap();
        let width = full.width();
        let snap = |h: f32| (h * ppp).round() / ppp;
        let origin = full.min;
        let line = ui.text_style_height(&egui::TextStyle::Body);
        // Fenêtre qui a quitté le présent, vue en bas : une page plus récente
        // ajoutée sous la vue ne doit pas l'entraîner (elle irait chercher la
        // suivante, et ainsi de suite) : la vue s'accroche d'abord au message affiché.
        if view.anchor == Anchor::Bottom && cs.newer && view.geo.len() > 0 && view.geo.ids().last() != cs.msgs.back().map(|m| &m.id) {
            view.anchor = view.geo.anchor_at(view.geo.max_offset(view.last_h), view.last_h, false);
        }
        let relations = &self.store.relations;
        let me_id = self.store.me.as_ref().map(|u| u.id);
        view.sync(cs, width, line, ppp, relations.rev(), &|author| {
            if Some(author) == me_id {
                return None;
            }
            let r = relations.relation(author)?;
            if r.kind == crate::state::relations::RelKind::Blocked {
                Some(false)
            } else {
                r.ignored.then_some(true)
            }
        });
        // Bouton « nouveaux messages » / « Revenir au présent » : il a sa
        // propre bande sous la liste, qui ne cache donc jamais un message.
        let label = jump_label(cs, view, say.lang);
        let strip = label.as_ref().map_or(0.0, |l| snap(jump_size(ui, t, l).y + 2.0 * t.spacing.chat_pad));
        // Notification affichée : elle aussi a sa bande, sous celle du bouton,
        // pour ne jamais recouvrir à moitié la dernière ligne de la liste.
        let toast_strip = self.toast.as_ref().map_or(0.0, |toast| {
            let wrap = (width - 2.0 * t.spacing.chat_pad - t.toast_margin().sum().x).max(1.0);
            let text = ui.painter().layout(toast.text.to_string(), egui::TextStyle::Body.resolve(ui.style()), t.colors.text, wrap);
            snap(text.size().y + t.toast_margin().sum().y + 2.0 * t.spacing.chat_pad)
        });
        list.toast_slot = (toast_strip > 0.0).then(|| egui::Rect::from_min_max(egui::pos2(full.min.x, full.max.y - toast_strip), full.max));
        // Bas de la liste sur un pixel entier : hauteur de vue, décalage du bas
        // et hauts des messages restent alignés sur les pixels.
        let top = (full.min.y * ppp).ceil() / ppp;
        let bottom = ((full.max.y - strip - toast_strip) * ppp).floor() / ppp;
        let list_rect = egui::Rect::from_min_max(egui::pos2(full.min.x, top), egui::pos2(full.max.x, bottom.max(top)));
        let view_h = list_rect.height();
        // Saut vers un message (lot T12) : la vue le place au tiers de sa
        // hauteur dès qu'il est en mémoire ; abandonné si sa page n'est plus attendue.
        if let Some((c, id)) = list.pending.filter(|(c, _)| *c == cid) {
            if cs.contains(id) {
                view.anchor = Anchor::At { id, delta: -view_h / 3.0 };
                list.pending = None;
                list.flash = Some((c, id, ui.input(|i| i.time) + FLASH));
            } else if !self.store.jump_pending(cid) {
                list.pending = None;
            }
        }
        let flash = list.flash.filter(|(c, _, _)| *c == cid);
        // Zoom du salon : à partir d'ici, et jusqu'à la fin de la liste, jetons et
        // style aux tailles de texte multipliées (bandes du bas déjà mesurées).
        let base = (t, (zoom != 1.0).then(|| ui.style().clone()));
        let zoomed_tokens;
        let t = if zoom != 1.0 {
            zoomed_tokens = super::chat_zoom::zoomed(t, zoom);
            super::chat_zoom::zoom_style(ui, zoom);
            &zoomed_tokens
        } else {
            t
        };
        // Hauteurs de l'en-tête et du pied : refaites quand leur contenu change.
        let (head, foot) = (Block::head(cs), Block::foot(cs));
        let word = |p: &Problem| words::problem(say.lang, p, say.h24);
        let head_changed = view.head_error.update(cs.error.as_ref(), say, word);
        let foot_changed = view.foot_error.update(cs.error_newer.as_ref(), say, word);
        if view.head != Some(head) || head_changed {
            let h = measure(ui, origin, width, ("tête", cid), |ui| {
                block_ui(ui, t, say, head, view.head_error.text(), true, None, &start);
            });
            view.geo.set_head(snap(h));
            view.head = Some(head);
        }
        if view.foot != Some(foot) || foot_changed {
            let h = measure(ui, origin, width, ("pied", cid), |ui| {
                block_ui(ui, t, say, foot, view.foot_error.text(), false, None, &start);
            });
            view.geo.set_foot(snap(h));
            view.foot = Some(foot);
        }
        // Horodatages relatifs : remis en forme quand leur texte change.
        // Mots-clés surveillés (plugin, lot E1) : surlignés dans le fil.
        let kw = super::keywords::active(&self.keywords);
        let now_ms = crate::state::store::now_ms();
        // Réactions, réponse, modification : lus par le rendu des messages (lots T5/T6).
        let mut acts = Acts::new(&mut self.actions, &self.store, cid, ui.ctx().clone());
        acts.quick = quick;
        // Les messages de la vue et d'une marge d'un écran de chaque côté sont
        // mesurés avant d'être dessinés : leur hauteur exacte est connue avant
        // qu'ils apparaissent, et la vue ne saute pas. Ceux de la vue (et d'un
        // quart d'écran autour) le sont tout de suite ; la marge, dans la
        // limite de `MEASURE_BUDGET` par image, le reste aux images suivantes
        // (l'ancre garde la vue en place) : ouvrir un salon jamais vu ne coûte
        // plus quatre écrans de mise en page en une seule image.
        let budget_end = std::time::Instant::now() + MEASURE_BUDGET;
        let mut deferred = false;
        for _ in 0..4 {
            view.geo.fit_pad(view_h);
            let offset = view.geo.offset_for(view.anchor, view_h);
            let range = view.geo.range(offset - view_h, offset + 2.0 * view_h);
            let must = view.geo.range(offset - view_h / 4.0, offset + view_h * 1.25);
            let mut changed = false;
            deferred = false;
            for i in must.clone().chain(range.filter(|i| !must.contains(i))) {
                if view.measured.get(i) != Some(&false) {
                    continue;
                }
                if !must.contains(&i) && std::time::Instant::now() >= budget_end {
                    deferred = true;
                    continue;
                }
                let Some((m, f)) = view.item(cs, i) else { continue };
                let doc = view.doc(m, now_ms, || super::keywords::marked(kw, m, parse_message(&self.store, m, say.lang, say.h24)));
                let h = snap(measure(ui, origin, width, ("mesure", m.id), |ui| {
                    item_ui(ui, t, say, &self.images, m, &doc, f, false, &mut acts);
                }));
                view.measured[i] = true;
                view.record(f.grouped, h);
                if let Some(e) = view.cache.get_mut(&m.id) {
                    e.measured = Some((f, h));
                }
                changed |= view.geo.set_height(i, h);
            }
            if !changed {
                break;
            }
        }
        view.geo.fit_pad(view_h);
        // Marge pas encore mesurée : une image de plus, jusqu'à ce qu'elle le soit.
        if deferred {
            ui.ctx().request_repaint();
        }

        // Décalage et hauts des messages sur la grille des pixels : les erreurs
        // d'arrondi des sommes de hauteurs ne décalent jamais l'image d'un pixel.
        // En bas, le décalage reste exactement le maximum : arrondi en deçà,
        // egui croirait la liste pas tout à fait au bout et peindrait son
        // dégradé du bas (et la vue ne serait pas la même à chaque retour).
        let max_offset = view.geo.max_offset(view_h);
        let offset = view.geo.offset_for(view.anchor, view_h);
        let offset = if offset >= max_offset - STEP { max_offset } else { snap(offset) };
        // Défilement de l'utilisateur vers un bord (molette, même sans effet
        // quand la liste est déjà au bord) : lu avant que la liste le consomme.
        let hovered = ui.rect_contains_pointer(list_rect);
        // Pointeur sur la barre d'actions d'un message (une couche au-dessus de
        // la liste) : egui donnerait la molette à la barre, qui n'en fait rien,
        // et la liste ne défilerait plus. La liste la prend elle-même.
        let over_bar = !hovered
            && ui.ctx().pointer_hover_pos().is_some_and(|p| list_rect.contains(p) && ui.ctx().layer_id_at(p) == Some(super::msg_actions::bar_layer()));
        let bar_wheel = if over_bar { ui.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta.y)) } else { 0.0 };
        let wheel = if hovered { ui.input(|i| i.smooth_scroll_delta.y) } else { bar_wheel };
        let scroll_to = if bar_wheel != 0.0 { (offset - bar_wheel).clamp(0.0, max_offset) } else { offset };
        let now = ui.input(|i| i.time);
        let images = &self.images;
        let store = &self.store;
        let me = store.me.as_ref().map(|u| u.id);
        // Fin du surlignage d'un message visé : il s'estompe (lot ANIM).
        let flash_fade = if super::motion::level(ui.ctx()) == super::motion::Level::None { 0.0 } else { super::motion::dur::FLASH_FADE };
        let flash_k = flash.map_or(0.0, |(_, _, until)| if flash_fade > 0.0 { ((until - now) / flash_fade).clamp(0.0, 1.0) as f32 } else { 1.0 });
        let mut retry = (false, false);
        let mut shown: Option<(u64, u64)> = None;
        let mut corrected = false;
        let mut stale = false;
        let mut refresh: Option<i64> = None;
        let mut toggle: Option<u64> = None;
        let out = ui
            .scope_builder(egui::UiBuilder::new().max_rect(list_rect), |ui| {
                // Pas d'élastique au bord : egui le calcule avec les bornes de l'image
                // précédente et ramènerait la vue vers l'ancien bas quand le contenu change.
                ui.spacing_mut().scroll.kinetic.rubber_band = false;
                egui::ScrollArea::vertical()
                    .id_salt(cid)
                    .auto_shrink(false)
                    .content_margin(egui::Margin::ZERO)
                    .vertical_scroll_offset(scroll_to)
                    .show_viewport(ui, |ui, viewport| {
                        let origin = ui.max_rect().min;
                        ui.set_min_size(egui::vec2(width, view.geo.total()));
                        // Position à l'écran = haut de la liste + écart (petit, arrondi au
                        // pixel) entre l'élément et le haut de la vue. Pas `origin.y + y` :
                        // à des milliers de points, les arrondis des flottants changent
                        // d'un passage à l'autre et décaleraient un demi-pixel d'un pixel.
                        let at = |y: f32, h: f32| egui::Rect::from_min_size(egui::pos2(origin.x, top + snap(y - viewport.min.y)), egui::vec2(width, h));
                        if viewport.min.y < view.geo.head() {
                            let mut child = ui.new_child(item_builder(at(0.0, view.geo.head()), ("tête", cid)));
                            retry.0 |= block_ui(&mut child, t, say, head, view.head_error.text(), true, Some(images), &start);
                        }
                        // Message arrivé pendant qu'on regarde le bas du fil : il entre en fondu (lot ANIM).
                        if view.anchor == Anchor::Bottom
                            && let Some((m, _)) = view.geo.len().checked_sub(1).and_then(|i| view.item(cs, i))
                        {
                            let pings = m.mention_everyone || me.is_some_and(|me| m.mentions.iter().any(|(id, _)| *id == me));
                            super::motion::newest(ui.ctx(), cid, m.id, pings);
                        }
                        let range = view.geo.range(viewport.min.y, viewport.max.y);
                        for i in range {
                            let Some((m, f)) = view.item(cs, i) else {
                                stale = true;
                                continue;
                            };
                            // Message d'une suite masquée repliée : rien à dessiner.
                            if f.hide == (Hide::Inside { open: false }) {
                                continue;
                            }
                            let rect = at(snap(view.geo.top(i)), view.geo.height(i));
                            // Message visé par un saut : surligné derrière son contenu (lot T12).
                            if flash.is_some_and(|(_, id, _)| id == m.id) {
                                ui.painter().rect_filled(rect, t.r_hover(), t.markdown.mention_bg.gamma_multiply(flash_k));
                            }
                            // Message qui vient d'arriver : fond qui pulse une fois s'il nous mentionne, entrée en fondu.
                            let pulse = super::motion::pulse(ui.ctx(), m.id);
                            if pulse > 0.0 {
                                ui.painter().rect_filled(rect, t.r_hover(), t.markdown.mention_bg.gamma_multiply(pulse));
                            }
                            let arriving = super::motion::played(ui.ctx(), super::motion::message_id(m.id), super::motion::dur::MESSAGE);
                            let span = (arriving < 1.0).then(|| super::motion::Span::begin(ui));
                            let mut child = ui.new_child(item_builder(rect, ("élément", m.id)));
                            let doc = view.doc(m, now_ms, || super::keywords::marked(kw, m, parse_message(store, m, say.lang, say.h24)));
                            refresh = refresh.into_iter().chain(doc.refresh_at_ms).min();
                            if item_ui(&mut child, t, say, images, m, &doc, f, true, &mut acts) {
                                toggle = Some(m.id);
                            }
                            if let Some(span) = span {
                                let (alpha, tf) = super::motion::slide_in(ui.ctx(), arriving);
                                span.end(ui, alpha, tf);
                            }
                            // Une mesure fausse (ne devrait pas arriver) est corrigée à l'image suivante.
                            let h = snap(child.min_rect().height());
                            if (h - view.geo.height(i)).abs() > 0.5 {
                                view.geo.set_height(i, h);
                                corrected = true;
                            }
                            shown = Some((shown.map_or(m.id, |(lo, _)| lo), m.id));
                        }
                        let end = snap(view.geo.top(view.geo.len()));
                        if viewport.max.y > end && foot != Block::Pad {
                            let mut child = ui.new_child(item_builder(at(end, view.geo.total() - end), ("pied", cid)));
                            retry.1 |= block_ui(&mut child, t, say, foot, view.foot_error.text(), false, Some(images), &start);
                        }
                    })
            })
            .inner;
        // Fin de la liste : le bouton du bas et le bandeau reprennent la taille du thème.
        let t = base.0;
        if let Some(style) = base.1 {
            ui.set_style(style);
        }
        if let Some(id) = toggle {
            if !view.revealed.remove(&id) {
                view.revealed.insert(id);
            }
            view.reveal_rev = view.reveal_rev.wrapping_add(1);
            ui.ctx().request_repaint();
        }
        if stale {
            // Géométrie qui ne correspond plus au magasin (ne devrait pas
            // arriver) : refaite à l'image suivante, sans paniquer.
            view.synced = None;
        }
        if corrected || stale {
            ui.ctx().request_repaint();
        }
        // Un seul réveil programmé, et seulement si un horodatage relatif est visible.
        if let Some(at) = refresh {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(u64::try_from(at.saturating_sub(now_ms)).unwrap_or(0)));
        }
        let inner = out.inner_rect;
        let new_offset = out.state.clamped_offset().y;
        let view_h = inner.height();
        view.last_h = view_h;
        // La molette est lissée par egui sur plusieurs images, et le décalage
        // est remis sur la grille des pixels à chaque image : sans report, les
        // dernières tranches d'un cran (moins d'un demi-pixel chacune) étaient
        // perdues, et un cran ne parcourait pas toute sa distance (3 % de
        // moins à 144 images par seconde). Le reste sous le pixel attend
        // l'image suivante.
        let new_offset = if wheel != 0.0 && (new_offset - offset).abs() > f32::EPSILON {
            let max = view.geo.max_offset(view_h);
            let exact = (new_offset + view.wheel_frac).clamp(0.0, max);
            let on_grid = if exact >= max - STEP { max } else { snap(exact) };
            view.wheel_frac = if on_grid <= 0.0 || on_grid >= max { 0.0 } else { exact - on_grid };
            on_grid
        } else {
            if wheel == 0.0 {
                view.wheel_frac = 0.0;
            }
            new_offset
        };
        // Sans défilement, l'ancre reste la même (même si une hauteur corrigée a
        // changé la géométrie pendant l'image) : collée en bas, elle le reste.
        // Collée en bas et toujours en bas (décalage seulement borné par egui) :
        // elle le reste, même quand la fenêtre a quitté le présent.
        let still_bottom = view.anchor == Anchor::Bottom && new_offset >= view.geo.max_offset(view_h) - STEP;
        if (new_offset - offset).abs() > f32::EPSILON && !still_bottom {
            view.anchor = view.geo.anchor_at(new_offset, view_h, !cs.newer);
        }
        if view.anchor == Anchor::Bottom {
            view.seen = view.seen.max(cs.msgs.back().map_or(0, |m| m.id));
        } else if let Some((_, hi)) = shown {
            view.seen = view.seen.max(hi);
        }
        let max = view.geo.max_offset(view_h);
        // Déjà au bas exact du présent, la molette vers le bas ne déplace plus
        // rien (egui borne le décalage) : la vue se colle quand même en bas.
        if view.anchor != Anchor::Bottom && !cs.newer && wheel < 0.0 && new_offset >= max - STEP {
            view.anchor = Anchor::Bottom;
        }
        // Pages automatiques : seulement si l'utilisateur fait défiler vers ce
        // bord, une demande à la fois, et au plus une par `vlist::AUTO_DELAY`.
        let up = new_offset < offset - STEP || wheel > 0.0;
        let down = new_offset > offset + STEP || wheel < 0.0;
        let older_ok = vlist::wants_older(new_offset, view_h, cs.more && cs.loaded, cs.loading || cs.loading_older(), cs.error.is_some());
        let newer_ok = vlist::wants_newer(new_offset, view_h, max, cs.newer && cs.loaded, cs.loading || cs.loading_newer(), cs.error_newer.is_some());
        view.pace[0].update(older_ok, up);
        view.pace[1].update(newer_ok, down);
        let (older, newer) = (view.pace[0].ready(now), view.pace[1].ready(now));
        if let Some(wait) = view.pace[0].wait(now).into_iter().chain(view.pace[1].wait(now)).reduce(f64::min) {
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(wait));
        }
        let detached = cs.newer;
        let mut jump = false;
        if let Some(label) = &label {
            let area = egui::Rect::from_min_max(egui::pos2(full.min.x, list_rect.max.y), egui::pos2(full.max.x, full.max.y - toast_strip));
            jump = jump_button(ui, t, area, label);
        }
        // Bandeau « N nouveaux messages depuis … » en haut de la liste, tant
        // que la ligne « Nouveau » est au-dessus de la vue (ou pas chargée).
        let banner = new_banner(cs, shown, say).map(|b| banner_ui(ui, t, say.lang, list_rect, &b));
        // L'état a changé pendant l'image (vue redescendue, message arrivé) :
        // la bande du bouton apparaît ou disparaît à l'image suivante.
        if jump_label(cs, view, say.lang) != label {
            ui.ctx().request_repaint();
        }

        self.store.set_viewing(cid, shown);
        match banner {
            Some(BannerClick::Jump(id)) => self.jump_to_message(cid, id),
            Some(BannerClick::MarkRead) => {
                self.store.mark_read(cid);
                self.store.dismiss_new_marker(cid);
                ui.ctx().request_repaint();
            }
            Some(BannerClick::None) | None => {}
        }
        // Fin du surlignage : un seul réveil, à son terme.
        if let Some((_, _, until)) = flash {
            let now = ui.input(|i| i.time);
            if now >= until {
                self.list.flash = None;
                ui.ctx().request_repaint();
            } else if until - now <= flash_fade {
                ui.ctx().request_repaint();
            } else {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(until - now - flash_fade));
            }
        }
        let at_bottom = self.list.views.get(&cid).is_some_and(|v| v.anchor == Anchor::Bottom);
        self.note_reading(ui, cid, at_bottom);
        if retry.0 {
            if !self.store.chans.get(&cid).is_some_and(|c| c.loaded) {
                self.store.ensure_history(cid);
            } else {
                self.store.load_older(cid);
            }
        }
        if retry.1 && !self.store.load_newer(cid) {
            self.store.return_to_present(cid);
        }
        if older && self.store.load_older(cid)
            && let Some(view) = self.list.views.get_mut(&cid)
        {
            view.pace[0].sent(now);
        }
        if newer && self.store.load_newer(cid)
            && let Some(view) = self.list.views.get_mut(&cid)
        {
            view.pace[1].sent(now);
        }
        if jump {
            if detached {
                self.store.return_to_present(cid);
            }
            if let Some(view) = self.list.views.get_mut(&cid) {
                view.anchor = Anchor::Bottom;
            }
            ui.ctx().request_repaint();
        }
    }
}

/// Défilement (points) en deçà duquel la vue n'a pas bougé.
const STEP: f32 = 0.5;

/// Temps de mesure accordé par image aux messages de la marge (hors de la vue).
const MEASURE_BUDGET: std::time::Duration = std::time::Duration::from_micros(1500);

/// Texte du bouton du bas : nouveaux messages arrivés pendant qu'on lisait
/// plus haut, ou retour au présent quand la fenêtre a quitté le présent.
fn jump_label(cs: &ChanState, view: &ChanView, lang: Lang) -> Option<String> {
    if cs.newer {
        return Some(tx::JUMP_TO_PRESENT.get(lang).to_string());
    }
    let unseen = vlist::unseen(view.geo.ids(), view.seen) + cs.missed();
    if view.anchor == Anchor::Bottom || unseen == 0 {
        return None;
    }
    Some(crate::i18n::p::new_messages(lang, unseen as u64))
}

/// Langue et horloge des textes d'une image.
#[derive(Clone, Copy)]
struct Say {
    lang: Lang,
    h24: bool,
}

fn item_builder(rect: egui::Rect, salt: impl Hash + std::fmt::Debug) -> egui::UiBuilder {
    egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)).scope_id(Id::unique(salt))
}

/// Hauteur de ce que `f` dessine sur `width` points, sans rien afficher.
fn measure(ui: &mut egui::Ui, origin: egui::Pos2, width: f32, salt: impl Hash + std::fmt::Debug, f: impl FnOnce(&mut egui::Ui)) -> f32 {
    let rect = egui::Rect::from_min_size(origin, egui::vec2(width, f32::INFINITY));
    let mut child = ui.new_child(item_builder(rect, salt).invisible());
    f(&mut child);
    child.min_rect().height()
}

/// Un message et les séparateurs qui le précèdent. `live` : dessin réel
/// (sinon mesure : ni survol, ni menu, ni image demandée).
#[allow(clippy::too_many_arguments)]
fn item_ui(ui: &mut egui::Ui, t: &Tokens, say: Say, images: &crate::images::Images, m: &Message, doc: &Doc, f: Flags, live: bool, acts: &mut Acts) -> bool {
    let ts = snowflake_ms(m.id);
    let badge = f.new_line.then(|| mt::NEW_BADGE.get(say.lang));
    if f.day_sep {
        // Jour et nouveaux messages à la fois : un seul filet, rouge, qui porte les deux.
        let today = local_day(crate::state::store::now_ms());
        separator(ui, t, Some(&day_label(say.lang, local_day(ts), today)), badge);
    } else if badge.is_some() {
        separator(ui, t, None, badge);
    }
    let mut toggled = false;
    if let Hide::Head { n, ignored, open } = f.hide {
        toggled = super::hidden::bar(ui, t, say.lang, n, ignored, open, live);
    }
    if !f.hide.shows_message() {
        return toggled;
    }
    if live {
        message_ui_acts(ui, t, say.lang, images, m, doc, f.grouped, ts, acts);
    } else {
        measure_message(ui, t, say.lang, m, doc, f.grouped, ts, acts);
    }
    toggled
}

/// Filet sur toute la largeur : la date au milieu (`day`), et pour les
/// nouveaux messages (`badge`) un filet rouge qui finit à droite par une
/// étiquette pleine, pointe à gauche.
fn separator(ui: &mut egui::Ui, t: &Tokens, day: Option<&str>, badge: Option<&str>) {
    ui.add_space(t.spacing.message_gap);
    let font = super::fonts::demi_font(t.typography.small);
    let red = t.danger_fill();
    let (text_color, line) = if badge.is_some() { (t.colors.danger, red) } else { (t.colors.faint, t.colors.separator) };
    let day = day.map(|d| ui.painter().layout_no_wrap(d.to_string(), font.clone(), text_color));
    let badge = badge.map(|b| ui.painter().layout_no_wrap(b.to_string(), font.clone(), t.colors.on_accent));
    let line_h = ui.fonts_mut(|f| f.row_height(&font));
    let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), line_h), egui::Sense::hover());
    let pad = t.spacing.message_x;
    let gap = t.spacing.item_x;
    let y = rect.center().y;
    let stroke = egui::Stroke::new(t.shapes.border, line);
    let p = ui.painter();
    let mut right = rect.right() - pad;
    if let Some(g) = badge {
        // Étiquette : aplat rouge, coins droits arrondis, pointe vers le filet.
        let inner = t.spacing.item_x / 2.0;
        let body = egui::Rect::from_min_max(egui::pos2(right - g.size().x - 2.0 * inner, rect.top()), egui::pos2(right, rect.bottom()));
        let r = t.r_row();
        p.rect_filled(body, egui::CornerRadius { nw: 0, sw: 0, ne: r.ne, se: r.se }, red);
        let tip = body.height() / 2.0;
        p.add(egui::Shape::convex_polygon(vec![body.left_top(), egui::pos2(body.left() - tip, y), body.left_bottom()], red, egui::Stroke::NONE));
        p.galley(egui::pos2(body.left() + inner, body.center().y - g.size().y / 2.0), g, t.colors.on_accent);
        right = body.left() - tip;
    }
    match day {
        Some(g) => {
            let size = g.size();
            let x = rect.center().x - size.x / 2.0;
            p.line_segment([egui::pos2(rect.left() + pad, y), egui::pos2(x - gap, y)], stroke);
            p.line_segment([egui::pos2((x + size.x + gap).min(right), y), egui::pos2(right, y)], stroke);
            p.galley(egui::pos2(x, y - size.y / 2.0), g, text_color);
        }
        None => {
            p.line_segment([egui::pos2(rect.left() + pad, y), egui::pos2(right, y)], stroke);
        }
    }
}

/// Ce qu'il faut pour le début d'un salon ou d'une conversation privée.
struct Start {
    name: String,
    /// Conversation privée : son avatar (celui du correspondant ou du groupe).
    dm: Option<crate::state::model::Avatar>,
}

impl Start {
    fn of(store: &crate::state::Store, cid: u64) -> Start {
        match store.dms.iter().find(|d| d.id == cid) {
            Some(d) => Start { name: d.name.clone(), dm: Some(d.avatar) },
            None => Start { name: store.channel_name(cid).unwrap_or_default().to_string(), dm: None },
        }
    }
}

/// Début d'un salon (disque « # », « Bienvenue dans #salon ! ») ou d'une
/// conversation privée (avatar, nom), suivi d'une phrase, comme dans Discord.
/// `images` absent : mesure seule (même place, rien de demandé).
fn start_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, images: Option<&crate::images::Images>, start: &Start) {
    egui::Frame::new().inner_margin(egui::Margin::symmetric(t.spacing.message_x as i8, 0)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.add_space(t.spacing.message_gap);
        let side = if start.dm.is_some() { t.layout.profile_avatar } else { (t.layout.profile_avatar * 0.85).round() };
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), egui::Sense::hover());
        match (&start.dm, images) {
            (Some(avatar), Some(images)) => super::util::paint_avatar(ui, images, t, rect, avatar, &start.name),
            (Some(_), None) => {}
            (None, _) => {
                ui.painter().circle_filled(rect.center(), side / 2.0, t.colors.bg3);
                icons::paint(Icon::Hash, ui.painter(), rect.shrink(side / 4.0), t.colors.text_strong);
            }
        }
        ui.add_space(t.spacing.item_y);
        // Titre : un tiers plus grand que le plus grand titre d'un message.
        let title = super::fonts::demi_font((t.markdown.h1 * 4.0 / 3.0).round());
        let (heading, line) = match start.dm {
            Some(_) => (start.name.clone(), mf::dm_start(lang, &start.name)),
            None => (mf::welcome_channel(lang, &start.name), mf::channel_start(lang, &start.name)),
        };
        ui.add(egui::Label::new(RichText::new(heading).font(title).strong().color(t.colors.text_strong)).wrap());
        ui.add(egui::Label::new(RichText::new(line).color(t.colors.muted)).wrap());
        ui.add_space(t.spacing.message_gap);
    });
}

/// Bandeau des nouveaux messages : texte, et premier message non lu.
struct Banner {
    text: String,
    first: u64,
}

/// Clic sur le bandeau.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BannerClick {
    None,
    Jump(u64),
    MarkRead,
}

/// Le bandeau s'affiche tant que le premier message non lu est au-dessus de
/// la vue (`shown` : premier et dernier messages dessinés) ou pas en mémoire.
fn new_banner(cs: &ChanState, shown: Option<(u64, u64)>, say: Say) -> Option<Banner> {
    let first = cs.new_marker?;
    let (top, _) = shown?;
    if top <= first {
        return None;
    }
    let count = cs.msgs.iter().filter(|m| m.id >= first).count() + cs.missed();
    if count == 0 {
        return None;
    }
    let since = time::clock(say.lang, snowflake_ms(first), true, say.h24);
    Some(Banner { text: mf::new_since(say.lang, crate::i18n::p::new_messages(say.lang, count as u64), &since), first })
}

/// Bandeau d'accent collé en haut de la liste : un clic va au premier
/// message non lu, « Marquer comme lu » à droite.
fn banner_ui(ui: &mut egui::Ui, t: &Tokens, lang: Lang, list: egui::Rect, b: &Banner) -> BannerClick {
    let font = super::fonts::demi_font(t.typography.button);
    let fg = t.colors.on_accent;
    let mark = ui.painter().layout_no_wrap(mt::MARK_AS_READ.get(lang).to_string(), font.clone(), fg);
    let pad = egui::vec2(t.spacing.message_x, t.spacing.item_y);
    let h = (mark.size().y + 2.0 * pad.y).round();
    let rect = egui::Rect::from_min_max(egui::pos2(list.left() + pad.x, list.top()), egui::pos2(list.right() - pad.x, list.top() + h));
    let mark_rect = egui::Rect::from_min_max(egui::pos2(rect.right() - mark.size().x - 2.0 * t.spacing.item_x, rect.top()), rect.max);
    let body = ui.interact(rect, Id::unique("bandeau-nouveaux"), egui::Sense::click());
    let read = ui.interact(mark_rect, Id::unique("bandeau-nouveaux-lu"), egui::Sense::click());
    let r = t.r_control();
    let p = ui.painter();
    p.rect_filled(rect, egui::CornerRadius { nw: 0, ne: 0, sw: r.sw, se: r.se }, t.colors.accent);
    if read.hovered() {
        p.rect_filled(mark_rect, egui::CornerRadius { se: r.se, ..egui::CornerRadius::ZERO }, t.colors.hover);
    }
    let room = (mark_rect.left() - rect.left() - 2.0 * t.spacing.item_x).max(1.0);
    let text = ui.painter().layout(b.text.clone(), font, fg, f32::INFINITY);
    let clip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.left() + t.spacing.item_x + room, rect.bottom()));
    ui.painter().with_clip_rect(clip).galley(egui::pos2(rect.left() + t.spacing.item_x, rect.center().y - text.size().y / 2.0), text, fg);
    ui.painter().galley(egui::pos2(mark_rect.left() + t.spacing.item_x, rect.center().y - mark.size().y / 2.0), mark, fg);
    body.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &b.text));
    read.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, mt::MARK_AS_READ.get(lang)));
    if read.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        BannerClick::MarkRead
    } else if body.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
        BannerClick::Jump(b.first)
    } else {
        BannerClick::None
    }
}

use crate::i18n::table::messages::{f as mf, t as mt};

/// En-tête ou pied de la liste. `head` : marge du haut (sinon du bas).
/// Renvoie `true` si « Réessayer » a été cliqué.
/// `error` : texte de l'erreur (`Block::Error`), déjà mis en mots.
/// `images` : dessin réel (absent : mesure) ; `start` : début du salon.
#[allow(clippy::too_many_arguments)]
fn block_ui(ui: &mut egui::Ui, t: &Tokens, say: Say, b: Block, error: &str, head: bool, images: Option<&crate::images::Images>, start: &Start) -> bool {
    let lang = say.lang;
    let live = images.is_some();
    let mut retry = false;
    if head {
        ui.add_space(t.spacing.chat_pad);
    }
    let note = |ui: &mut egui::Ui, text: &str| {
        // Dans les marges de la colonne des messages (le texte allait d'un bord à l'autre de la discussion).
        let side = t.spacing.chat_pad.round().clamp(0.0, 127.0) as i8;
        egui::Frame::new().inner_margin(egui::Margin::symmetric(side, 0)).show(ui, |ui| {
            ui.vertical_centered(|ui| ui.add(egui::Label::new(RichText::new(text).color(t.colors.muted)).wrap()));
        });
    };
    match b {
        Block::Pad => {}
        Block::Loading => {
            // Squelettes à la place d'un vide (lot ANIM) ; cran « aucune » : la roue d'avant.
            if super::motion::level(ui.ctx()) == super::motion::Level::None {
                ui.vertical_centered(super::spinner::spinner);
            } else {
                super::motion::skeleton(ui, t.colors.row_hover, (t.typography.body * 2.6).round(), 3);
            }
        }
        Block::Error => {
            ui.vertical_centered(|ui| {
                ui.add(egui::Label::new(RichText::new(error).color(t.colors.danger)).wrap());
                retry = crate::ui::widgets::compact(ui, tx::RETRY.get(lang)).clicked() && live;
            });
        }
        Block::Denied => note(ui, tx::HISTORY_DENIED.get(lang)),
        Block::Empty => note(ui, tx::NO_MESSAGES.get(lang)),
        Block::Start => start_ui(ui, t, lang, images, start),
    }
    if !head {
        ui.add_space(t.spacing.chat_pad);
    }
    retry
}

/// Taille du bouton du bas pour ce texte.
fn jump_size(ui: &egui::Ui, t: &Tokens, label: &str) -> egui::Vec2 {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let text = ui.painter().layout_no_wrap(label.to_string(), font, t.colors.on_accent).size();
    let icon = Tokens::icon_side(t.typography.body);
    egui::vec2(text.x + t.spacing.item_x + icon, text.y.max(icon)) + t.toast_margin().sum()
}

/// Bouton centré dans sa bande (`area`, sous la liste). Renvoie `true` s'il est cliqué.
fn jump_button(ui: &mut egui::Ui, t: &Tokens, area: egui::Rect, label: &str) -> bool {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let galley = ui.painter().layout_no_wrap(label.to_string(), font, t.colors.on_accent);
    let margin = t.toast_margin();
    let icon = Tokens::icon_side(t.typography.body);
    let gap = t.spacing.item_x;
    let rect = egui::Rect::from_center_size(area.center(), jump_size(ui, t, label));
    let resp = ui.interact(rect, Id::unique("nouveaux-messages"), egui::Sense::click());
    // Le bouton arrive en glissant (lot ANIM).
    let arriving = super::motion::appear(ui.ctx(), Id::unique("nouveaux-messages-arrive"), super::motion::dur::POP);
    let span = super::motion::Span::begin(ui);
    let p = ui.painter();
    p.rect_filled(rect, t.r_toast(), t.colors.accent);
    if resp.hovered() {
        p.rect_filled(rect, t.r_toast(), t.colors.hover);
    }
    let left = rect.left() + f32::from(margin.left);
    let mid = rect.center().y;
    p.galley(egui::pos2(left, mid - galley.size().y / 2.0), galley.clone(), t.colors.on_accent);
    let icon_rect = egui::Rect::from_center_size(egui::pos2(left + galley.size().x + gap + icon / 2.0, mid), egui::Vec2::splat(icon));
    icons::paint(Icon::ArrowDown, p, icon_rect, t.colors.on_accent);
    let (alpha, tf) = super::motion::slide_in(ui.ctx(), arriving);
    span.end(ui, alpha, tf);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked()
}

/// Empreinte des réglages d'egui qui changent la mise en page du texte.
fn style_fingerprint(style: &egui::Style) -> u64 {
    let mut h = DefaultHasher::new();
    for (name, font) in &style.text_styles {
        name.hash(&mut h);
        font.size.to_bits().hash(&mut h);
    }
    let s = &style.spacing;
    for v in [s.item_spacing, s.button_padding, s.interact_size] {
        v.x.to_bits().hash(&mut h);
        v.y.to_bits().hash(&mut h);
    }
    h.finish()
}

/// Empreinte de ce qui, dans un message, change sa mise en page.
fn fingerprint(m: &Message) -> u64 {
    let mut h = DefaultHasher::new();
    m.content.hash(&mut h);
    m.edited.hash(&mut h);
    m.author.hash(&mut h);
    m.embeds.hash(&mut h);
    // Réactions, réponse, transfert (lots T5/T6).
    m.meta.hash(&mut h);
    m.attachments.hash(&mut h);
    h.finish()
}

/// Jour local (numéro depuis l'an 1) d'un instant en ms Unix.
fn local_day(ms: i64) -> i32 {
    Local.timestamp_millis_opt(ms).single().map_or(0, |d| d.date_naive().num_days_from_ce())
}

/// « Aujourd'hui », « Hier », ou « jeudi 12 novembre 2026 ».
fn day_label(lang: Lang, day: i32, today: i32) -> String {
    match (NaiveDate::from_num_days_from_ce_opt(day), NaiveDate::from_num_days_from_ce_opt(today)) {
        (Some(d), Some(today)) => time::day_label(lang, d, today),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::HISTORY_PAGE;
    use crate::state::model::{Avatar, Channel, ChannelKind, Guild, User, DISCORD_EPOCH};
    use crate::state::{Cmd, Event, Store};
    use std::collections::BTreeMap;

    const GUILD: u64 = 9;
    const CHAN: u64 = 100;
    const ME: u64 = 1;

    /// Connexion factice : rien n'est envoyé ni reçu (les commandes restent
    /// dans le magasin, lues par le test).
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

    /// Message `i` du salon : une minute d'écart, auteurs par groupes de trois,
    /// longueurs variées (une, plusieurs lignes).
    fn msg(i: u64, channel_id: u64) -> Message {
        let ms = 1_700_000_000_000 + i * 60_000;
        let author = 10 + i / 3 % 4;
        Message {
            id: ((ms - DISCORD_EPOCH) << 22) | (i & 0xFFF),
            channel_id,
            guild_id: Some(GUILD),
            author_id: author,
            author: format!("u{author}"),
            author_avatar: Avatar::default_for(author),
            content: if i.is_multiple_of(5) { format!("message {i} ").repeat(30) } else { format!("message {i}") },
            attachments: Vec::new(),
            embeds: Vec::new(),
            mentions: Vec::new(),
            mention_everyone: false,
            edited: false,
            system: None,
            meta: Default::default(),
        }
    }

    fn ready() -> Event {
        let channels = (0..30u64).map(|c| Channel { id: CHAN + c, name: format!("s{c}"), kind: ChannelKind::Text, position: c as i32, parent: None, overwrites: Some(Vec::new()), slowmode: 0 }).collect();
        Event::Ready { me: User::new(ME, "Moi".into()), guilds: vec![Guild::new(GUILD, "G".into(), channels)], dms: Vec::new() }
    }

    /// Application dessinable sans fenêtre, images désactivées, sur `CHAN`.
    struct Harness {
        app: App,
        time: f64,
        /// Taille de la fenêtre (points).
        size: egui::Vec2,
    }

    impl Harness {
        fn new() -> Self {
            let ctx = egui::Context::default();
            let mut store = Store::new();
            store.apply(ready());
            store.select_guild(GUILD);
            store.select_channel(CHAN);
            store.take_commands();
            let images = crate::images::Images::disabled(ctx.clone());
            let mut app = App::with_context(ctx, super::super::tests::offline_boot(Box::new(Offline), store, images));
            app.theme = Tokens::default();
            // Liste des membres masquée : ces tests mesurent la discussion seule.
            app.theme.layout.members = crate::theme::MembersPanel::Hidden;
            Harness { app, time: 0.0, size: egui::vec2(1000.0, 700.0) }
        }

        /// Une image de 1/60 s ; `wheel` : molette sur la liste (points, vers le haut si positif).
        fn frame(&mut self, wheel: f32) {
            self.shapes(wheel);
        }

        /// Comme `frame`, en rendant ce qui a été peint.
        fn shapes(&mut self, wheel: f32) -> Vec<egui::epaint::ClippedShape> {
            let ctx = self.app.ctx.clone();
            let mut events = vec![egui::Event::PointerMoved(egui::pos2(700.0, 350.0))];
            if wheel != 0.0 {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, wheel),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, self.size)),
                time: Some(self.time),
                events,
                ..Default::default()
            };
            self.time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| self.app.main_ui(ui));
            out.textures_delta.clear();
            out.shapes
        }

        /// La géométrie du salon correspond exactement à ses messages.
        fn check(&self, cid: u64) {
            let (Some(cs), Some(view)) = (self.app.store.chans.get(&cid), self.app.list.views.get(&cid)) else { return };
            let ids: Vec<u64> = cs.msgs.iter().map(|m| m.id).collect();
            assert_eq!(view.geo.ids(), ids.as_slice());
            assert_eq!(view.flags.len(), ids.len());
            assert_eq!(view.measured.len(), ids.len());
        }
    }

    /// Horodatage relatif visible : un seul réveil programmé, quand son texte
    /// change (ici « il y a 30 secondes » → « il y a 1 minute ») ; sans lui,
    /// aucun.
    #[test]
    fn relative_time_wakes_only_when_visible() {
        let delay = |content: Option<String>| {
            let mut h = Harness::new();
            let mut m = msg(1, CHAN);
            if let Some(c) = content {
                m.content = c;
            }
            h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: vec![m] });
            // Une seconde d'images : les animations d'egui (barre de défilement) finissent.
            for _ in 0..60 {
                h.frame(0.0);
            }
            // Deux images sans événement : la seconde dit ce qui reste programmé.
            let ctx = h.app.ctx.clone();
            let mut out = Default::default();
            for _ in 0..2 {
                h.time += 1.0;
                let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), ..Default::default() };
                out = ctx.run_ui(input, |ui| h.app.main_ui(ui));
            }
            let out: egui::FullOutput = out;
            out.viewport_output.values().map(|v| v.repaint_delay).min().unwrap_or(std::time::Duration::MAX)
        };
        assert_eq!(delay(None), std::time::Duration::MAX, "rien au repos");
        let now = crate::state::store::now_ms() / 1000;
        let d = delay(Some(format!("vu <t:{}:R>", now - 30)));
        assert!(d > std::time::Duration::from_secs(20) && d <= std::time::Duration::from_secs(31), "{d:?}");
    }

    /// Bloquant du lot : fenêtre réduite, eframe applique les événements sans
    /// dessiner. READY (cache vidé) puis une page plus courte arrivent entre
    /// deux images : la géométrie est refaite, aucun accès hors bornes.
    #[test]
    fn ready_then_shorter_page_without_frame_in_between() {
        let mut h = Harness::new();
        let first: Vec<Message> = (0..100).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: first });
        h.frame(0.0);
        h.check(CHAN);
        // Vue remontée vers le haut de la liste (les indices hauts sont visibles ou non : peu importe).
        for _ in 0..30 {
            h.frame(400.0);
        }
        h.app.store.apply(ready());
        let short: Vec<Message> = (200..230).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: short });
        h.frame(0.0);
        h.check(CHAN);
        assert_eq!(h.app.list.views[&CHAN].geo.len(), 30);
    }

    /// Changer de compte vide la liste virtualisée du compte quitté :
    /// hauteurs, positions de défilement, ancrages, rien n'en reste.
    #[test]
    fn switching_account_clears_the_message_list() {
        let mut h = Harness::new();
        let page: Vec<Message> = (0..60).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: page });
        h.frame(0.0);
        h.frame(400.0);
        h.frame(0.0);
        assert!(!h.app.list.views.is_empty() && h.app.list.current.is_some(), "liste remplie avant le changement");
        h.app.switch_account(crate::account::AccountKey::of_user(ME + 1));
        assert!(h.app.list.views.is_empty() && h.app.list.current.is_none() && h.app.list.style.is_none(), "liste du compte A vidée");
        assert!(h.app.store.chans.is_empty() && h.app.store.me.is_none());
    }

    /// Petit générateur pseudo-aléatoire reproductible (xorshift).
    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }

        fn below(&mut self, n: u64) -> u64 {
            self.next() % n.max(1)
        }
    }

    /// Robustesse : suites aléatoires d'événements (créations, suppressions,
    /// pages avant/après, READY, éviction de salons, retour au présent)
    /// appliquées sans image entre elles, puis une image : jamais de
    /// panique, et la géométrie suit exactement les messages.
    #[test]
    fn random_events_between_frames_keep_geometry_consistent() {
        let mut h = Harness::new();
        let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
        // Salon « serveur » : tous les messages qui existent, par numéro.
        let mut server: BTreeMap<u64, Message> = (0..1500).map(|i| (i, msg(i, CHAN))).collect();
        let mut next = 1500;
        let latest = |server: &BTreeMap<u64, Message>| -> Vec<Message> { server.values().rev().take(HISTORY_PAGE).cloned().collect() };
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: latest(&server) });
        for round in 0..300 {
            for _ in 0..1 + rng.below(25) {
                // Les demandes en cours reçoivent leur page (ou un échec) dans le désordre.
                let cmds = h.app.store.take_commands();
                for c in cmds {
                    match c {
                        Cmd::History { channel_id, before } if channel_id == CHAN => {
                            let messages: Vec<Message> = match before {
                                None => latest(&server),
                                Some(b) => server.values().rev().filter(|m| m.id < b).take(HISTORY_PAGE).cloned().collect(),
                            };
                            if rng.below(8) == 0 {
                                h.app.store.apply(Event::HistoryError { channel_id, before, after: None, error: Problem::Api("panne".into()) });
                            } else {
                                h.app.store.apply(Event::History { channel_id, before, messages });
                            }
                        }
                        Cmd::HistoryAfter { channel_id, after } if channel_id == CHAN => {
                            let messages: Vec<Message> = server.values().filter(|m| m.id > after).take(HISTORY_PAGE).cloned().collect();
                            h.app.store.apply(Event::HistoryAfter { channel_id, after, messages });
                        }
                        Cmd::History { channel_id, .. } => {
                            h.app.store.apply(Event::History { channel_id, before: None, messages: Vec::new() });
                        }
                        _ => {}
                    }
                }
                match rng.below(10) {
                    0..=2 => {
                        let m = msg(next, CHAN);
                        server.insert(next, m.clone());
                        next += 1;
                        h.app.store.apply(Event::MessageCreate(m));
                    }
                    3 => {
                        let cs = &h.app.store.chans.get(&CHAN);
                        let n = cs.map_or(0, |c| c.msgs.len()) as u64;
                        if let Some(id) = cs.and_then(|c| c.msgs.get(rng.below(n) as usize)).map(|m| m.id) {
                            server.retain(|_, m| m.id != id);
                            h.app.store.apply(Event::MessageDelete { channel_id: CHAN, id });
                        }
                    }
                    4 => {
                        h.app.store.load_older(CHAN);
                    }
                    5 => {
                        h.app.store.load_newer(CHAN);
                    }
                    6 => {
                        let n = h.app.store.chans.get(&CHAN).map_or(0, |c| c.msgs.len()) as u64;
                        let ids: Vec<u64> = h.app.store.chans.get(&CHAN).map(|c| c.msgs.iter().map(|m| m.id).collect()).unwrap_or_default();
                        let a = rng.below(n) as usize;
                        let range = ids.get(a).map(|lo| (*lo, ids.get((a + 5).min(ids.len().saturating_sub(1))).copied().unwrap_or(*lo)));
                        h.app.store.set_viewing(CHAN, range);
                    }
                    7 => {
                        if rng.below(4) == 0 {
                            h.app.store.apply(ready());
                        } else {
                            h.app.store.return_to_present(CHAN);
                        }
                    }
                    8 => {
                        // Éviction : d'autres salons ouverts puis retour (le cache du salon peut partir).
                        for c in 1..=rng.below(29) {
                            h.app.store.select_channel(CHAN + c);
                        }
                        h.app.store.select_channel(CHAN);
                    }
                    _ => {
                        let id = h.app.store.chans.get(&CHAN).and_then(|c| c.msgs.back()).map(|m| m.id);
                        if let Some(id) = id {
                            h.app.store.apply(Event::MessageUpdate { channel_id: CHAN, id, content: "modifié ".repeat(1 + rng.below(40) as usize) });
                        }
                    }
                }
            }
            let wheel = match rng.below(4) {
                0 => 900.0,
                1 => -900.0,
                _ => 0.0,
            };
            h.frame(wheel);
            h.check(CHAN);
            assert!(h.app.store.chans.get(&CHAN).is_none_or(|c| c.msgs.len() <= crate::state::store::MAX_CACHED_MESSAGES + HISTORY_PAGE), "tour {round}");
        }
    }

    /// Sert les pages demandées par l'interface à partir de `server` ;
    /// renvoie le nombre de pages servies.
    fn serve(h: &mut Harness, server: &[Message]) -> usize {
        let mut n = 0;
        for c in h.app.store.take_commands() {
            n += 1;
            match c {
                Cmd::History { channel_id, before } => {
                    let messages: Vec<Message> = server.iter().rev().filter(|m| before.is_none_or(|b| m.id < b)).take(HISTORY_PAGE).cloned().collect();
                    h.app.store.apply(Event::History { channel_id, before, messages });
                }
                Cmd::HistoryAfter { channel_id, after } => {
                    let messages: Vec<Message> = server.iter().filter(|m| m.id > after).take(HISTORY_PAGE).cloned().collect();
                    h.app.store.apply(Event::HistoryAfter { channel_id, after, messages });
                }
                Cmd::Search(crate::state::search::SearchCmd::Around { channel_id, around }) => {
                    let at = server.partition_point(|m| m.id < around);
                    let messages = server[at.saturating_sub(HISTORY_PAGE / 2)..(at + HISTORY_PAGE / 2).min(server.len())].to_vec();
                    h.app.store.apply(Event::Search(crate::state::search::SearchEvent::Around { channel_id, around, messages }));
                }
                Cmd::Send { .. } | Cmd::Social(_) | Cmd::Typing { .. } | Cmd::OpenDm { .. } | Cmd::Profile(_) | Cmd::Subscribe(_) | Cmd::Account(_) | Cmd::Relations(_) | Cmd::Upload(_) | Cmd::Threads(_) | Cmd::Servers(_) | Cmd::Search(_) | Cmd::Inbox(_) | Cmd::Interactions(_) | Cmd::Polls(_) | Cmd::Invites(_) | Cmd::Announce(_) | Cmd::GuildAdmin(_) | Cmd::ChannelAdmin(_) | Cmd::Roles(_) | Cmd::Moderation(_) | Cmd::GuildEvents(_) | Cmd::Community(_) | Cmd::Expressions(_) | Cmd::Voice(_) | Cmd::Calls(_) | Cmd::Stage(_) | Cmd::Video(_) | Cmd::Activity(_) | Cmd::Gifs(_) | Cmd::Effects(_) | Cmd::Quests(_) | Cmd::Tags(_) | Cmd::Widgets(_) | Cmd::Shop(_) | Cmd::Inventory(_) | Cmd::Suggestions(_) | Cmd::Discovery(_) | Cmd::Summaries(_) | Cmd::Verification(_) | Cmd::AppDir(_) | Cmd::SafetyHub(_) | Cmd::Saved(_) => n -= 1,
            }
        }
        n
    }

    /// Saut vers un message loin dans l'historique (lot T12) : la page autour
    /// de lui remplace la fenêtre, la vue s'y place, il est surligné 2 s, et
    /// la géométrie reste celle du magasin en redescendant vers le présent.
    #[test]
    fn jump_to_an_old_message_shows_it_highlighted() {
        let mut h = Harness::new();
        let server: Vec<Message> = (0..700).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server[server.len() - HISTORY_PAGE..].to_vec() });
        h.frame(0.0);
        let target = server[400].id;
        assert_eq!(h.app.store.jump_to_message(CHAN, target), crate::state::store::Jump::Loading);
        h.app.list.jump_to(CHAN, target);
        h.frame(0.0);
        assert_eq!(h.app.list.pending_jump(), Some((CHAN, target)), "attend sa page");
        assert_eq!(serve(&mut h, &server), 1);
        for _ in 0..3 {
            h.frame(0.0);
        }
        h.check(CHAN);
        assert!(h.app.list.pending_jump().is_none());
        let (lo, hi) = h.app.store.chans[&CHAN].viewing().expect("messages affichés");
        assert!((lo..=hi).contains(&target), "{lo}..{hi} sans {target}");
        assert!(h.app.list.flash.is_some_and(|(_, id, _)| id == target));
        assert!(h.app.store.chans[&CHAN].newer, "fenêtre hors du présent");
        for _ in 0..150 {
            h.frame(0.0);
        }
        assert!(h.app.list.flash.is_none(), "surlignage terminé");
        // Redescente vers le présent : pages `after` servies, géométrie cohérente.
        for _ in 0..600 {
            h.frame(-3000.0);
            h.check(CHAN);
            serve(&mut h, &server);
        }
        assert!(!h.app.store.chans[&CHAN].newer, "présent atteint");
    }

    /// Application entière au repos (logique et interface, sans entrée) : une
    /// fois les animations finies, plus aucune image n'est demandée, ni tout
    /// de suite ni plus tard. Garde-fou contre un réveil périodique (minuterie,
    /// sondage, horloge…) qui ferait redessiner une fenêtre où rien ne change.
    #[test]
    fn idle_app_asks_for_no_frame() {
        let mut h = Harness::new();
        // Messages anciens : aucun horodatage relatif (« il y a 5 min ») à tenir à jour.
        let page: Vec<Message> = (0..HISTORY_PAGE as u64).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: page });
        let ctx = h.app.ctx.clone();
        let mut last = None;
        for i in 0..300 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), ..Default::default() };
            h.time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| {
                h.app.logic_pass();
                h.app.ui_pass(ui);
            });
            out.textures_delta.clear();
            let delay = out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay);
            if i >= 240 {
                assert_eq!(delay, Some(std::time::Duration::MAX), "image {i} : réveil demandé par {:?}", ctx.repaint_causes());
            }
            last = delay;
        }
        assert_eq!(last, Some(std::time::Duration::MAX));
    }

    /// Lot « perf » : chaque image dit à la passerelle le serveur affiché et
    /// les messages à l'écran. Un CHANNEL_UPDATE du serveur affiché réveille
    /// toujours, celui d'un autre serveur attend ; les restes d'un
    /// MESSAGE_UPDATE (épinglage) réveillent pour un message affiché seulement.
    #[test]
    fn each_frame_tells_the_gateway_what_is_on_screen() {
        use crate::state::interest::Verdict;
        use crate::state::social::SocialEvent;
        let mut h = Harness::new();
        let page: Vec<Message> = (0..HISTORY_PAGE as u64).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: page });
        // Images entières (`logic_pass` puis `ui_pass`), comme dans l'application.
        let ctx = h.app.ctx.clone();
        for _ in 0..5 {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), ..Default::default() };
            h.time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| {
                h.app.logic_pass();
                h.app.ui_pass(ui);
            });
            out.textures_delta.clear();
        }
        let i = h.app.interest.shared();
        // Colonne des salons : serveur GUILD.
        assert!(!i.calm_channel_update(GUILD, 4242, 1) && !i.calm_channel_update(GUILD, 4242, 1), "serveur affiché : réveil");
        assert!(!i.calm_channel_update(777, 4343, 1), "premier paquet du salon");
        assert!(i.calm_channel_update(777, 4343, 1), "autre serveur, même visibilité : attente");
        // Messages à l'écran : le dernier de la page est affiché, un message d'un autre salon ne l'est pas.
        let (_, hi) = h.app.store.chans[&CHAN].viewing().unwrap_or((0, 0));
        let pin = |channel_id, message_id| Event::Social(SocialEvent::Pinned { channel_id, message_id, pinned: true });
        assert_eq!(i.verdict(&pin(CHAN, hi)), Verdict::Wake);
        assert_eq!(i.verdict(&pin(CHAN + 1, hi)), Verdict::Defer);
    }

    /// Molette vers le haut, puis aussitôt vers le bas (comme le banc
    /// `preview --bench`, 60 points par image) : la vue redescend.
    #[test]
    fn reversing_the_wheel_moves_the_view_back_down() {
        let mut h = Harness::new();
        h.size = egui::vec2(1100.0, 720.0);
        let server: Vec<Message> = (0..500).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server.clone() });
        for _ in 0..30 {
            h.frame(0.0);
        }
        let first = |h: &Harness| h.app.store.chans[&CHAN].viewing().map(|(lo, _)| lo);
        let start = first(&h);
        for _ in 0..150 {
            h.frame(60.0);
        }
        let up = first(&h);
        for _ in 0..150 {
            h.frame(-60.0);
        }
        let back = first(&h);
        assert_ne!(start, up, "la vue est montée");
        assert_ne!(up, back, "la vue est redescendue");
    }

    /// Un cran de molette parcourt toute sa distance dans la liste des
    /// messages, à toute cadence d'images, puis plus aucune image n'est
    /// demandée. Signalé sur le vrai compte : « ça ne scroll qu'à moitié,
    /// c'est lent » (un cran valait 40 points, contre 100 dans Discord, et
    /// ses dernières tranches se perdaient sur la grille des pixels).
    #[test]
    fn one_wheel_notch_travels_its_full_distance() {
        use crate::ui::scroll;
        let mut h = Harness::new();
        h.size = egui::vec2(1100.0, 720.0);
        let server: Vec<Message> = (0..500).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server });
        for _ in 0..30 {
            h.frame(0.0);
        }
        let ctx = h.app.ctx.clone();
        assert_eq!(ctx.options(|o| o.input_options.line_scroll_speed), scroll::NOTCH_POINTS, "réglage par défaut appliqué par l'application");
        let offset = |h: &Harness| {
            let v = &h.app.list.views[&CHAN];
            v.geo.offset_for(v.anchor, v.last_h)
        };
        // Une image ; vrai si une autre est demandée tout de suite.
        let step = |h: &mut Harness, dt: f64, events: Vec<egui::Event>| {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), events, ..Default::default() };
            h.time += dt;
            let mut out = ctx.run_ui(input, |ui| h.app.main_ui(ui));
            out.textures_delta.clear();
            out.viewport_output.get(&egui::ViewportId::ROOT).is_some_and(|v| v.repaint_delay.is_zero())
        };
        for dt in [1.0 / 30.0, 1.0 / 60.0, 1.0 / 144.0] {
            for lines in [1.0f32, 3.0, -2.0, 1.0, -1.0] {
                let before = offset(&h);
                let mut events = vec![
                    egui::Event::PointerMoved(egui::pos2(700.0, 350.0)),
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta: egui::vec2(0.0, lines), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE },
                ];
                let mut frames = 0;
                while step(&mut h, dt, std::mem::take(&mut events)) {
                    frames += 1;
                    assert!(frames < 300, "le défilement ne s'arrête pas");
                }
                let moved = before - offset(&h);
                // Au pixel près (la vue reste sur la grille des pixels).
                assert!((moved - lines * scroll::NOTCH_POINTS).abs() <= 0.5, "{lines} cran(s) à {dt:.4} s par image : {moved} points");
                assert!(f64::from(frames) * dt < 0.5, "{frames} images à {dt:.4} s par image");
            }
        }
    }

    /// Pointeur immobile qui tombe sur la barre d'actions d'un message (cas
    /// du banc : la souris ne bouge pas, les messages défilent dessous) : la
    /// molette fait toujours défiler la liste, dans les deux sens.
    #[test]
    fn wheel_over_the_action_bar_still_scrolls() {
        let mut h = Harness::new();
        h.size = egui::vec2(1100.0, 720.0);
        let server: Vec<Message> = (0..500).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server });
        for _ in 0..30 {
            h.frame(0.0);
        }
        let bar = h.app.actions.bar_rect().expect("barre affichée sous le pointeur");
        let at = bar.center();
        let ctx = h.app.ctx.clone();
        let step = |h: &mut Harness, wheel: f32| {
            let events = vec![
                egui::Event::PointerMoved(at),
                egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Point, delta: egui::vec2(0.0, wheel), phase: egui::TouchPhase::Move, source: egui::MouseWheelSource::Wheel, modifiers: egui::Modifiers::NONE },
            ];
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), events, ..Default::default() };
            h.time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| h.app.main_ui(ui));
            out.textures_delta.clear();
        };
        let first = |h: &Harness| h.app.store.chans[&CHAN].viewing().map(|(lo, _)| lo);
        let start = first(&h);
        for _ in 0..60 {
            step(&mut h, 60.0);
        }
        let up = first(&h);
        for _ in 0..60 {
            step(&mut h, -60.0);
        }
        assert_ne!(start, up, "la vue est montée sous la barre");
        assert_ne!(up, first(&h), "la vue est redescendue sous la barre");
    }

    /// Molette tenue vers le haut pendant 30 s simulées, chaque page servie
    /// aussitôt : au plus une page par seconde (et le chargement avance bien).
    #[test]
    fn held_at_top_requests_at_most_one_page_per_second() {
        let mut h = Harness::new();
        let server: Vec<Message> = (0..5000).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server[server.len() - HISTORY_PAGE..].to_vec() });
        let mut pages = 0;
        for _ in 0..30 * 60 {
            h.frame(3000.0);
            pages += serve(&mut h, &server);
        }
        assert!((20..=31).contains(&pages), "{pages} pages en 30 s");
    }

    /// Liste courte, vue en haut, aucun défilement : rien n'est demandé.
    /// Un seul geste vers le haut : une seule page.
    #[test]
    fn short_list_at_top_loads_only_after_a_scroll() {
        let mut h = Harness::new();
        h.size = egui::vec2(1000.0, 2400.0);
        // Messages courts d'un même auteur (un seul groupe) : la page entière tient dans la vue.
        let page: Vec<Message> = (1000..1000 + HISTORY_PAGE as u64).map(|i| Message { content: "ok".into(), author_id: 10, author: "u10".into(), ..msg(i, CHAN) }).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: page });
        for _ in 0..300 {
            h.frame(0.0);
        }
        assert!(h.app.store.take_commands().is_empty());
        h.frame(120.0);
        for _ in 0..300 {
            h.frame(0.0);
        }
        assert_eq!(h.app.store.take_commands().len(), 1);
    }

    /// Aller-retour vers le haut puis le bas : l'image finale est exactement
    /// l'initiale (pas de décalage arrondi en deçà du bas, donc pas de dégradé
    /// du bas d'egui ; positions indépendantes de la taille du décalage).
    /// Hauteur fractionnaire à 1 pixel par point, et 1,25 pixel par point
    /// (fenêtre de l'appli et du banc : 1100 × 720 points).
    #[test]
    fn round_trip_to_the_top_paints_the_same_bottom() {
        round_trip(1.0, egui::vec2(1000.0, 700.3));
        round_trip(1.25, egui::vec2(1100.0, 720.0));
    }

    fn round_trip(ppp: f32, size: egui::Vec2) {
        let mut h = Harness::new();
        h.app.ctx.set_pixels_per_point(ppp);
        h.size = size;
        // Plus que la fenêtre en mémoire : au retour, le haut a été évincé et
        // la fenêtre est redescendue par pages `after`.
        let server: Vec<Message> = (0..700).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server[server.len() - HISTORY_PAGE..].to_vec() });
        for _ in 0..30 {
            h.frame(0.0);
        }
        let before = h.shapes(0.0);
        let first = server[0].id;
        let top = |h: &Harness| h.app.store.chans[&CHAN].viewing().is_some_and(|(lo, _)| lo == first);
        for _ in 0..60 * 60 {
            if top(&h) {
                break;
            }
            h.frame(900.0);
            serve(&mut h, &server);
        }
        assert!(top(&h), "haut atteint");
        let bottom = |h: &Harness| !h.app.store.chans[&CHAN].newer && h.app.list.views[&CHAN].anchor == Anchor::Bottom;
        for _ in 0..60 * 60 {
            if bottom(&h) {
                break;
            }
            h.frame(-900.0);
            serve(&mut h, &server);
        }
        { let v = &h.app.list.views[&CHAN]; assert!(bottom(&h), "bas atteint : newer {} ancre {:?} n {} max {} off {} vue {} total {}", h.app.store.chans[&CHAN].newer, v.anchor, h.app.store.chans[&CHAN].msgs.len(), v.geo.max_offset(v.last_h), v.geo.offset_for(v.anchor, v.last_h), v.last_h, v.geo.total()); }
        for _ in 0..30 {
            h.frame(0.0);
        }
        let after = h.shapes(0.0);
        // Seul ce qui se voit compte (egui ajoute un rectangle invisible de la
        // taille du contenu), hors barre de défilement : contenu et barre
        // dépendent du nombre de messages en mémoire (50 avant, 500 après).
        let right = h.size.x - 12.0;
        let visible = |v: Vec<egui::epaint::ClippedShape>| -> Vec<egui::epaint::ClippedShape> {
            v.into_iter().filter(|c| !matches!(&c.shape, egui::Shape::Rect(r) if (r.fill.a() == 0 && r.stroke.color.a() == 0) || r.rect.left() >= right)).collect()
        };
        // Le fondu de bord de la zone de défilement finit en dégradé entièrement
        // transparent, présent ou non selon l'horloge : il ne se voit pas non plus.
        let visible = |v: Vec<egui::epaint::ClippedShape>| -> Vec<egui::epaint::ClippedShape> { visible(v).into_iter().filter(|c| !matches!(&c.shape, egui::Shape::Mesh(m) if m.vertices.iter().all(|x| x.color.a() == 0))).collect() };
        let (before, after) = (visible(before), visible(after));
        // Un texte est comparé par sa position, son contenu et sa taille : ses
        // coordonnées dans l'atlas des glyphes changent quand l'atlas grandit.
        let same = |a: &egui::epaint::ClippedShape, b: &egui::epaint::ClippedShape| match (&a.shape, &b.shape) {
            (egui::Shape::Text(x), egui::Shape::Text(y)) => a.clip_rect == b.clip_rect && x.pos == y.pos && x.galley.job.text == y.galley.job.text && x.galley.rect == y.galley.rect && x.fallback_color == y.fallback_color && x.override_text_color == y.override_text_color,
            _ => a == b,
        };
        let diff = before.iter().zip(&after).position(|(a, b)| !same(a, b));
        let brief = |c: &egui::epaint::ClippedShape| match &c.shape {
            egui::Shape::Text(t) => format!("texte {:?} en {} {}, découpe {} {}, cadre {:?}, couleurs {:?}/{:?}", t.galley.job.text, t.pos.x, t.pos.y, c.clip_rect.min.y, c.clip_rect.max.y, t.galley.rect, t.fallback_color, t.override_text_color),
            s => format!("{s:?}").chars().take(400).collect(),
        };
        assert!(before.len() == after.len() && diff.is_none(), "image différente après l'aller-retour ({} et {} formes, première différence {diff:?} : {:?} / {:?})", before.len(), after.len(), diff.map(|i| brief(&before[i])), diff.map(|i| brief(&after[i])));
    }

    /// Changement de langue à la volée : un message système, la saisie et
    /// une notification déjà affichés passent dans l'autre langue à l'image
    /// suivante.
    #[test]
    fn language_switch_rewords_what_is_already_on_screen() {
        use crate::i18n::{self, Choice, Lang};
        use crate::state::{Notice, Problem, SystemKind};
        fn painted(shapes: &[egui::epaint::ClippedShape]) -> String {
            fn walk(s: &egui::Shape, out: &mut String) {
                match s {
                    egui::Shape::Text(t) => {
                        out.push_str(&t.galley.job.text);
                        out.push('\n');
                    }
                    egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
                    _ => {}
                }
            }
            let mut out = String::new();
            shapes.iter().for_each(|c| walk(&c.shape, &mut out));
            out
        }
        /// Remet le français à la fin du test, même s'il échoue.
        struct French;
        impl Drop for French {
            fn drop(&mut self) {
                i18n::set_language(Choice::Fr);
            }
        }
        let _french = French;
        i18n::set_language(Choice::Fr);
        let mut h = Harness::new();
        let mut joined = msg(1, CHAN);
        joined.content = String::new();
        joined.system = Some(SystemKind::Joined);
        let sentence = |lang| crate::state::system_msgs::text(lang, SystemKind::Joined, &joined, Default::default());
        let (joined_fr, joined_en) = (sentence(Lang::Fr), sentence(Lang::En));
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: vec![msg(0, CHAN), joined] });
        if let Some(cs) = h.app.store.chans.get_mut(&CHAN) {
            cs.error = Some(Problem::Forbidden);
        }
        h.app.notify(Notice::NotSent(Problem::Forbidden));
        h.app.sync_language();
        let fr = painted(&h.shapes(0.0));
        assert!(fr.contains(&joined_fr), "{fr}");
        assert!(fr.contains("Envoyer un message dans #s0"), "{fr}");
        assert!(fr.contains("Vous n'avez pas accès à ce salon\n"), "erreur de l'en-tête : {fr}");
        let toast = h.app.toast.as_ref().map(|t| t.text.to_string());
        assert_eq!(toast.as_deref(), Some("Message non envoyé\u{a0}: Vous n'avez pas accès à ce salon"));

        i18n::set_language(Choice::En);
        h.app.sync_language();
        assert_eq!(h.app.lang, Lang::En);
        let en = painted(&h.shapes(0.0));
        assert!(en.contains(&joined_en), "{en}");
        assert!(en.contains("Message #s0"), "{en}");
        assert!(en.contains("You don't have access to this channel\n"), "erreur de l'en-tête : {en}");
        assert!(!en.contains("rejoint") && !en.contains("Vous"), "{en}");
        let toast = h.app.toast.as_ref().map(|t| t.text.to_string());
        assert_eq!(toast.as_deref(), Some("Message not sent: You don't have access to this channel"));

        // Notification d'un texte constant : empruntée à la table, refaite au changement.
        h.app.notify(crate::i18n::t::CANNOT_SEND);
        assert!(matches!(h.app.toast.as_ref().map(|t| &t.text), Some(std::borrow::Cow::Borrowed("You can't send messages in this channel."))));
        i18n::set_language(Choice::Fr);
        h.app.sync_language();
        assert!(matches!(h.app.toast.as_ref().map(|t| &t.text), Some(std::borrow::Cow::Borrowed("Vous ne pouvez pas envoyer de message dans ce salon."))));
    }

    /// Une notification a sa bande sous la liste : la liste s'arrête au-dessus
    /// (aucune ligne coupée à mi-hauteur sous la notification), puis reprend
    /// sa place quand elle disparaît.
    #[test]
    fn toast_never_covers_the_message_list() {
        let mut h = Harness::new();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: (0..3).map(|i| msg(i * 5 + 1, CHAN)).collect() });
        // Image complète (liste puis notification) ; renvoie le bas de la liste
        // et le cadre de la notification.
        let frame = |h: &mut Harness| {
            let ctx = h.app.ctx.clone();
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), ..Default::default() };
            h.time += 1.0 / 60.0;
            let mut list_bottom = 0.0;
            let mut out = ctx.run_ui(input, |ui| {
                h.app.main_ui(ui);
                list_bottom = h.app.list.toast_slot.map_or(f32::NAN, |r| r.min.y);
                h.app.toast_ui(ui);
            });
            out.textures_delta.clear();
            let clip = out.shapes.iter().filter_map(|c| matches!(&c.shape, egui::Shape::Text(t) if t.galley.job.text.starts_with("message 11")).then_some(c.clip_rect.max.y)).fold(f32::NAN, f32::max);
            (clip, list_bottom, ctx.memory(|m| m.area_rect(Id::unique("toast"))))
        };
        // Première image : les panneaux d'egui n'ont pas encore leur taille.
        frame(&mut h);
        let (free, _, _) = frame(&mut h);
        h.app.notify(crate::i18n::t::CANNOT_SEND);
        for _ in 0..3 {
            frame(&mut h);
        }
        let (clip, slot_top, toast) = frame(&mut h);
        let toast = toast.unwrap_or_else(|| panic!("notification absente"));
        assert!(clip <= slot_top + 0.01 && clip < free, "liste jusqu'à {clip} (sans notification {free}), bande à {slot_top}");
        assert!(toast.min.y >= clip, "notification {toast:?} sur la liste (bas {clip})");
        // Notification expirée : la liste reprend toute sa hauteur.
        if let Some(t) = &mut h.app.toast {
            t.at -= std::time::Duration::from_secs(60);
        }
        frame(&mut h);
        let (clip, _, _) = frame(&mut h);
        assert_eq!(clip, free);
    }

    /// Messages non envoyés au-dessus de la saisie, liste détachée du présent
    /// (bande « Revenir au présent ») : chaque ligne non envoyée garde la
    /// hauteur d'un contrôle et la liste garde l'essentiel de la fenêtre.
    #[test]
    fn unsent_rows_leave_the_list_its_room() {
        let mut h = Harness::new();
        let server: Vec<Message> = (0..HISTORY_PAGE as u64).map(|i| msg(i, CHAN)).collect();
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: server });
        // La zone de saisie prend sa hauteur (ligne d'écriture réservée comprise) en quelques images.
        for _ in 0..3 {
            h.frame(0.0);
        }
        let full = h.app.list.views[&CHAN].last_h;
        h.app.store.apply(Event::SendError { channel_id: CHAN, nonce: 1, content: "un".into(), error: Problem::Api("réseau".into()), forbidden: false });
        h.app.store.apply(Event::SendUnconfirmed { channel_id: CHAN, nonce: 2, content: "deux".into() });
        for _ in 0..3 {
            h.frame(0.0);
        }
        let with_unsent = h.app.list.views[&CHAN].last_h;
        let row = h.app.theme.head_row();
        assert!(full - with_unsent < 4.0 * row, "liste réduite de {full} à {with_unsent}");
    }

    #[test]
    fn day_labels() {
        let d = chrono::NaiveDate::from_ymd_opt(2026, 11, 12).map_or(0, |d| d.num_days_from_ce());
        let (fr, en) = (Lang::Fr, Lang::En);
        assert_eq!(day_label(fr, d, d), "Aujourd'hui");
        assert_eq!(day_label(en, d - 1, d), "Yesterday");
        assert_eq!(day_label(en, d, d + 2), "Thursday, November 12, 2026");
        assert_eq!(day_label(fr, d - 1, d), "Hier");
        assert_eq!(day_label(fr, d, d + 2), "jeudi 12 novembre 2026");
        assert_eq!(day_label(fr, d - 313, d), "samedi 3 janvier 2026");
    }
    /// Lot C10 : serveurs et salons hors de la vue gardent leur place mais ne
    /// sont plus préparés ni peints ; le défilement les amène tous.
    #[test]
    fn rows_outside_the_view_keep_their_place_unpainted() {
        fn texts(shapes: &[egui::Shape], out: &mut Vec<(String, f32)>) {
            for s in shapes {
                match s {
                    egui::Shape::Vec(v) => texts(v, out),
                    egui::Shape::Text(t) => out.push((t.galley.job.text.clone(), t.pos.y)),
                    _ => {}
                }
            }
        }
        fn frame(h: &mut Harness, at: egui::Pos2, wheel: f32) -> Vec<(String, f32)> {
            let ctx = h.app.ctx.clone();
            let mut events = vec![egui::Event::PointerMoved(at)];
            if wheel != 0.0 {
                events.push(egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, wheel),
                    phase: egui::TouchPhase::Move,
                    source: egui::MouseWheelSource::Wheel,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, h.size)), time: Some(h.time), events, ..Default::default() };
            h.time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| h.app.main_ui(ui));
            out.textures_delta.clear();
            let mut found = Vec::new();
            texts(&out.shapes.into_iter().map(|s| s.shape).collect::<Vec<_>>(), &mut found);
            found
        }
        // Initiales distinctes par serveur (« ABZ »), noms distincts par salon.
        let tag = |i: u64| format!("{}{}Z", (b'A' + (i / 26) as u8) as char, (b'A' + (i % 26) as u8) as char);
        let name = |i: u64| tag(i).chars().map(|c| format!("{c}x ")).collect::<String>();
        let chans = |n: u64| (0..n).map(|c| Channel { id: CHAN + c, name: format!("salon-{c:03}"), kind: ChannelKind::Text, position: c as i32, parent: None, overwrites: Some(Vec::new()), slowmode: 0 }).collect::<Vec<_>>();
        let mut guilds = vec![Guild::new(GUILD, name(0), chans(200))];
        guilds.extend((1..120u64).map(|i| Guild::new(1000 + i, name(i), Vec::new())));
        let mut h = Harness::new();
        h.app.store = Store::new();
        h.app.store.apply(Event::Ready { me: User::new(ME, "Moi".into()), guilds, dms: Vec::new() });
        h.app.store.select_guild(GUILD);
        h.app.store.select_channel(CHAN);
        h.app.store.take_commands();
        let (rail, list) = (egui::pos2(30.0, 350.0), egui::pos2(150.0, 350.0));
        let mut seen = Vec::new();
        for _ in 0..5 {
            seen = frame(&mut h, list, 0.0);
        }
        let y = |seen: &[(String, f32)], what: &str| seen.iter().find(|(t, _)| t == what).map(|(_, y)| *y);
        let count = |seen: &[(String, f32)], f: &dyn Fn(&str) -> bool| seen.iter().filter(|(t, _)| f(t)).count();
        let is_tag = |t: &str| t.len() == 3 && t.ends_with('Z') && t.chars().all(|c| c.is_ascii_uppercase());
        // Au repos : le haut est peint, le bas ne l'est pas.
        let (Some(y0), Some(y1)) = (y(&seen, "salon-001"), y(&seen, "salon-002")) else { panic!("premiers salons absents : {seen:?}") };
        let row = y1 - y0;
        assert!(row > 10.0, "{row}");
        assert_eq!(y(&seen, "salon-199"), None);
        let shown = count(&seen, &|t| t.starts_with("salon-"));
        assert!((10..80).contains(&shown), "{shown} salons peints");
        assert!(y(&seen, &tag(0)).is_some() && y(&seen, &tag(119)).is_none(), "{seen:?}");
        let shown = count(&seen, &is_tag);
        assert!((5..50).contains(&shown), "{shown} serveurs peints");
        // Tout en bas : les derniers arrivent, à leur place (une hauteur de ligne chacun).
        for _ in 0..40 {
            frame(&mut h, list, -100_000.0);
        }
        for _ in 0..40 {
            frame(&mut h, rail, -100_000.0);
        }
        for _ in 0..60 {
            seen = frame(&mut h, rail, 0.0);
        }
        let (Some(a), Some(b)) = (y(&seen, "salon-180"), y(&seen, "salon-199")) else { panic!("derniers salons absents : {seen:?}") };
        let step = b - y(&seen, "salon-198").unwrap_or(f32::NAN);
        assert!((step - row).abs() < 0.01 && (b - a - 19.0 * step).abs() < 0.5, "{a} {b} {row} {step}");
        assert!(b < h.size.y && b > h.size.y * 0.5, "{b}");
        assert_eq!(y(&seen, "salon-001"), None);
        assert!(y(&seen, &tag(119)).is_some() && y(&seen, &tag(0)).is_none());
    }

    /// F1 : les messages d'une personne bloquée sont repliés en une seule
    /// ligne ; « Afficher » (ici : la suite dépliée à la main) les remontre,
    /// et la levée du blocage aussi.
    #[test]
    fn blocked_authors_are_folded_into_one_line() {
        use crate::state::relations::{RelKind, Relation, RelationsEvent};
        let mut h = Harness::new();
        // Auteurs par groupes de trois : 10, 11, 12, 13, 10…
        let page: Vec<Message> = (0..12).map(|i| msg(i, CHAN)).collect();
        let first_of_11 = page[3].id;
        h.app.store.apply(Event::History { channel_id: CHAN, before: None, messages: page });
        h.frame(0.0);
        let flags = |h: &Harness| h.app.list.views[&CHAN].flags.iter().map(|f| f.hide).collect::<Vec<_>>();
        assert!(flags(&h).iter().all(|f| *f == Hide::No));
        let tall = h.app.list.views[&CHAN].geo.total();
        h.app.store.apply(Event::Relations(RelationsEvent::RelationAdd(Relation { id: 11, kind: RelKind::Blocked, nickname: None, ignored: false, user: None })));
        h.frame(0.0);
        h.frame(0.0);
        h.check(CHAN);
        let f = flags(&h);
        assert_eq!(f[3], Hide::Head { n: 3, ignored: false, open: false });
        assert_eq!(f[4], Hide::Inside { open: false });
        assert_eq!(f[5], Hide::Inside { open: false });
        assert_eq!(f[6], Hide::No);
        let view = &h.app.list.views[&CHAN];
        assert_eq!(view.geo.height(4), 0.0);
        assert!(view.geo.total() < tall, "la liste raccourcit");
        // Dépliée : les trois messages reprennent leur place sous la ligne.
        let view = h.app.list.views.get_mut(&CHAN).unwrap();
        view.revealed.insert(first_of_11);
        view.reveal_rev += 1;
        h.frame(0.0);
        h.frame(0.0);
        h.check(CHAN);
        assert_eq!(flags(&h)[3], Hide::Head { n: 3, ignored: false, open: true });
        assert_eq!(flags(&h)[4], Hide::Inside { open: true });
        assert!(h.app.list.views[&CHAN].geo.total() > tall, "messages et ligne « Masquer »");
        // Personne ignorée : même repli, autre libellé.
        h.app.store.apply(Event::Relations(RelationsEvent::RelationAdd(Relation { id: 12, kind: RelKind::Other, nickname: None, ignored: true, user: None })));
        h.frame(0.0);
        assert_eq!(flags(&h)[6], Hide::Head { n: 3, ignored: true, open: false });
        // Blocage levé : tout revient.
        h.app.store.apply(Event::Relations(RelationsEvent::RelationRemove { id: 11 }));
        h.frame(0.0);
        assert_eq!(flags(&h)[3], Hide::No);
    }
}
