//! Page des quêtes (lot ND) : une ligne « Quêtes » dans la colonne des MP,
//! et la page à la place de la page Amis, au gabarit de celle du client de
//! bureau : titre, tri et filtre, puis une grille de cartes de même hauteur
//! (image d'ambiance, logotype du jeu, éditeur et échéance, récompense, deux
//! boutons).
//!
//! Trois écritures seulement, chacune au clic : accepter, réclamer, afficher
//! le code. La page ne fait avancer aucune quête : « Jouer » ouvre dans le
//! navigateur la page prévue par la quête, « Commencer la quête » ouvre la
//! quête dans Discord, et rien n'est déclaré à Discord. Tri, filtre et
//! « Masquer » s'appliquent à la liste déjà reçue.

use super::friends::{empty_state, tab, PAGE_X, PAGE_Y};
use super::util::pixel_square;
use super::widgets::{self, Kind};
use super::App;
use crate::i18n::table::nd::{f, p, t as qt};
use crate::i18n::Lang;
use crate::images::{self, Image};
use crate::state::model::ImageRef;
use crate::state::quests::{arrange, official_url, Filter, Quest, Refused, RewardKind, Sort, Stage, Task, TaskClass, TaskKind};
use crate::state::usersettings::Remote;
use crate::state::Problem;
use crate::theme::icons::{self, Icon};
use crate::theme::Tokens;
use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect, RichText, Sense};

/// Largeur minimale d'une carte : en dessous, la grille perd une colonne.
const CARD_MIN: f32 = 300.0;
/// Largeur maximale d'une carte : au-delà, la grille gagne une colonne (ou,
/// sur une seule colonne, la carte ne s'étire plus).
const CARD_MAX: f32 = 520.0;
/// Colonnes de la grille dans une fenêtre ordinaire.
const COLUMNS: usize = 3;
/// Hauteur de l'image d'ambiance, en part de la largeur de la carte.
const HERO_SHARE: f32 = 0.3;
/// Le logotype dépasse d'autant sous l'image d'ambiance.
const LOGO_DROP: f32 = 12.0;

#[derive(Default)]
pub(super) struct QuestsPage {
    /// La page est affichée à la place de la page Amis.
    pub(super) open: bool,
    /// Onglet « Réclamées ».
    pub(super) done: bool,
    /// La boutique (lot ND, `ui/shop.rs`) occupe la page à la place des quêtes.
    pub(super) shop: bool,
    /// Onglet « Inventaire » de la boutique.
    pub(super) inventory: bool,
    /// La découverte de serveurs (lot ND, `ui/discovery.rs`) occupe la page.
    pub(super) discover: bool,
    /// Tri et filtre choisis (appliqués à la liste reçue).
    pub(super) sort: Sort,
    pub(super) filter: Filter,
    /// Quête dont la fenêtre de détail est ouverte.
    pub(super) detail: Option<u64>,
    /// Cartes posées à la dernière image (quête, rectangle) : tests de la grille.
    pub(super) cards: Vec<(u64, Rect)>,
    /// Vidéo de quête en lecture : quête, dernière position vue (µs), temps
    /// réellement lu depuis l'ouverture du lecteur (µs).
    pub(super) watch: Option<(u64, u64, u64)>,
}

/// Ce qu'un clic dans une carte demande.
enum Act {
    Enroll(u64),
    Claim(u64),
    Code(u64),
    Copy(String),
    CopyLink(u64),
    Official(u64),
    /// Lien prévu par la quête (page du jeu, invitation).
    Open(String),
    Hide(u64),
    Detail(u64),
}

/// Nombre de colonnes et largeur d'une carte pour `avail` points de large :
/// trois colonnes en grande fenêtre, deux puis une quand elle rétrécit.
pub(super) fn columns(avail: f32, gap: f32) -> (usize, f32) {
    let fits = |n: usize| avail >= n as f32 * CARD_MIN + (n as f32 - 1.0) * gap;
    let width = |n: usize| ((avail - (n as f32 - 1.0) * gap) / n as f32).floor();
    let mut cols = (1..=COLUMNS).rev().find(|n| fits(*n)).unwrap_or(1);
    // Fenêtre très large : des cartes de plus de `CARD_MAX` deviendraient démesurées.
    while width(cols) > CARD_MAX && fits(cols + 1) && cols < 8 {
        cols += 1;
    }
    (cols, width(cols).clamp(1.0, CARD_MAX))
}

/// Gabarit d'une carte : tout est fixé par sa largeur et le thème, jamais par
/// le contenu, pour que les cartes d'une rangée aient la même hauteur.
pub(super) struct CardLayout {
    pad: f32,
    hero_h: f32,
    logo_h: f32,
    promo_y: f32,
    promo_h: f32,
    block_y: f32,
    tile: f32,
    sep_y: f32,
    btn_y: f32,
    btn_h: f32,
    pub(super) height: f32,
}

impl CardLayout {
    pub(super) fn new(t: &Tokens, width: f32) -> CardLayout {
        let pad = t.profile_pad();
        let hero_h = (width * HERO_SHARE).round();
        let logo_h = (t.typography.body * 2.6).round();
        let promo_y = hero_h + LOGO_DROP + t.spacing.item_y;
        let promo_h = (t.typography.small * 1.4).ceil();
        let block_y = promo_y + promo_h + pad;
        let tile = (t.typography.body * 5.75).round();
        let sep_y = block_y + tile + pad;
        let btn_h = widgets::button_height(t);
        let btn_y = sep_y + (pad * 0.75).round();
        CardLayout { pad, hero_h, logo_h, promo_y, promo_h, block_y, tile, sep_y, btn_y, btn_h, height: btn_y + btn_h + (pad * 0.75).round() }
    }
}

fn task_text(k: TaskKind) -> crate::i18n::Text {
    match k {
        TaskKind::PlayOnDesktop => qt::TASK_PLAY_DESKTOP,
        TaskKind::StreamOnDesktop => qt::TASK_STREAM,
        TaskKind::PlayOnXbox => qt::TASK_XBOX,
        TaskKind::PlayOnPlaystation => qt::TASK_PLAYSTATION,
        TaskKind::WatchVideo => qt::TASK_VIDEO,
        TaskKind::WatchVideoOnMobile => qt::TASK_VIDEO_MOBILE,
        TaskKind::PlayActivity => qt::TASK_ACTIVITY,
        TaskKind::Achievement => qt::TASK_ACHIEVEMENT,
        TaskKind::Other => qt::TASK_OTHER,
    }
}

/// Durée d'une tâche : minutes entières, sinon secondes.
fn span(lang: Lang, secs: u32) -> String {
    if (secs >= 60 && secs.is_multiple_of(60)) || secs >= 120 { p::minutes(lang, u64::from(secs / 60)) } else { p::seconds(lang, u64::from(secs)) }
}

/// La même, en toutes lettres (« 15 minutes ») : phrase de la carte.
fn span_long(lang: Lang, secs: u32) -> String {
    if (secs >= 60 && secs.is_multiple_of(60)) || secs >= 120 { p::minutes_long(lang, u64::from(secs / 60)) } else { p::seconds_long(lang, u64::from(secs)) }
}

fn task_line(lang: Lang, task: &Task) -> String {
    let name = task_text(task.kind).get(lang);
    if task.kind.timed() && task.target > 0 { f::task_minutes(lang, name, span(lang, task.target)) } else { name.to_string() }
}

/// Nom de la récompense tel que la carte le dit (« un Casque fictif »).
fn reward_text(lang: Lang, q: &Quest) -> String {
    match q.reward() {
        Some(r) if !r.article.is_empty() => r.article.clone(),
        Some(r) if !r.name.is_empty() => r.name.clone(),
        Some(r) if r.kind == RewardKind::Orbs && r.orbs.is_some() => p::orbs(lang, u64::from(r.orbs.unwrap_or(0))),
        _ => qt::REWARD_GENERIC.get(lang).to_string(),
    }
}

/// Nom affiché d'une quête, et celui de son jeu (l'un remplace l'autre s'il manque).
fn names(q: &Quest) -> (&str, &str) {
    (if q.name.is_empty() { &q.game } else { &q.name }, if q.game.is_empty() { &q.name } else { &q.game })
}

/// Phrase de la tâche (« Joue à <jeu> pendant 15 minutes pour gagner
/// <récompense>. ») et la place du nom du jeu dans cette phrase (il est
/// coloré). La consigne rédigée par l'éditeur passe avant.
fn describe(lang: Lang, q: &Quest) -> (String, Option<(usize, usize)>) {
    let Some(task) = q.main_task() else { return (String::new(), None) };
    let game = names(q).1;
    if !task.text.is_empty() {
        let at = (!game.is_empty()).then(|| task.text.find(game)).flatten();
        return (task.text.clone(), at.map(|a| (a, a + game.len())));
    }
    const MARK: &str = "\u{1}";
    let reward = reward_text(lang, q);
    let long = span_long(lang, task.target);
    let s = match task.kind {
        TaskKind::PlayOnDesktop | TaskKind::PlayOnXbox | TaskKind::PlayOnPlaystation | TaskKind::PlayActivity if task.target > 0 => f::do_play(lang, MARK, long, reward),
        TaskKind::StreamOnDesktop if task.target > 0 => f::do_stream(lang, MARK, long, reward),
        TaskKind::WatchVideo | TaskKind::WatchVideoOnMobile => f::do_video(lang, MARK, reward),
        _ => f::do_goal(lang, MARK, reward),
    };
    let at = s.find(MARK);
    (s.replacen(MARK, game, 1), at.map(|a| (a, a + game.len())))
}

/// Ce que la carte ne dit plus : les tâches possibles, une par ligne.
fn tasks_summary(lang: Lang, q: &Quest) -> String {
    let mut out = String::new();
    if q.tasks.len() > 1 {
        out.push_str(if q.any_task { qt::TASKS_ANY } else { qt::TASKS_ALL }.get(lang));
    }
    for task in &q.tasks {
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str("•\u{a0}");
        out.push_str(&task_line(lang, task));
    }
    out
}

/// Texte sur `rows` lignes au plus, fini par « … » s'il déborde.
fn clamped(text: &str, font: FontId, color: Color32, width: f32, rows: usize) -> egui::text::LayoutJob {
    let mut job = egui::text::LayoutJob::simple(text.to_owned(), font, color, width.max(1.0));
    job.wrap.max_rows = rows;
    // Sur une seule ligne, on coupe au caractère : un nom en deux mots ne perd pas son second mot entier.
    job.wrap.break_anywhere = rows == 1;
    job
}

/// Fondu vertical de transparent (en haut de `r`) à `to` (en bas).
fn fade(p: &egui::Painter, r: Rect, to: Color32) {
    let mut m = egui::Mesh::default();
    for (at, color) in [(r.left_top(), Color32::TRANSPARENT), (r.right_top(), Color32::TRANSPARENT), (r.left_bottom(), to), (r.right_bottom(), to)] {
        m.colored_vertex(at, color);
    }
    m.add_triangle(0, 1, 2);
    m.add_triangle(1, 2, 3);
    p.add(egui::Shape::mesh(m));
}

/// Bouton neutre de l'en-tête de la page (« Suggérées ⌄ », « Filtre »).
fn head_button(ui: &mut egui::Ui, t: &Tokens, text: &str, icon: Icon) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), crate::ui::fonts::medium_font(t.typography.button), Color32::PLACEHOLDER);
    let width = galley.size().x + Tokens::icon_side(t.typography.button) + t.spacing.item_x / 2.0 + 2.0 * t.spacing.control_x;
    let (_, rect) = ui.allocate_space(vec2(width.ceil(), widgets::button_height(t)));
    widgets::button_in(ui, t, Kind::Secondary, text, Some(icon), rect)
}

impl App {
    /// Ligne « Quêtes » de la colonne des MP ; `true` si elle vient d'être cliquée.
    pub(super) fn quests_entry(&self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let n = self.store.quests.claimable();
        let label = if n > 0 { crate::i18n::table::social::f::tab_count(lang, qt::QUESTS.get(lang), n) } else { qt::QUESTS.get(lang).to_string() };
        self.dm_entry(ui, Icon::Quest, &label, self.quests_shown() && !self.quests.shop && !self.quests.discover && self.store.channel.is_none(), false).clicked()
    }

    pub(super) fn quests_shown(&self) -> bool {
        self.quests.open
    }

    /// Ouvre la page des quêtes (clic sur sa ligne, aperçu, tests) : c'est ce
    /// geste qui demande la liste.
    pub fn show_quests(&mut self, done: bool) {
        self.store.select_dms();
        self.requests.open = false;
        self.quests = QuestsPage { open: true, done, sort: self.quests.sort, filter: self.quests.filter, ..Default::default() };
        self.store.quests_open();
    }

    /// Ouvre la fenêtre de détail d'une quête (clic sur sa carte, aperçu).
    pub fn show_quest_detail(&mut self, id: u64) {
        self.quests.detail = Some(id);
    }

    /// Image d'une quête : la première adresse de `urls` qui répond. Une
    /// adresse en échec laisse la place à la suivante ; tant qu'une adresse
    /// charge, on l'attend. `slot` : 0 image d'ambiance, 1 vignette, 2 logotype.
    fn quest_image(&self, id: u64, slot: u16, urls: &[String], size: egui::Vec2, fill: bool, light: bool) -> Image {
        for (i, url) in urls.iter().enumerate() {
            let image = ImageRef::Media { owner: id, slot, rev: i as u32 + 1 + if light { 16 } else { 0 } };
            match self.images.get_original(image, url, size, fill) {
                Image::Failed => continue,
                other => return other,
            }
        }
        Image::Failed
    }

    pub(super) fn quests_page(&mut self, ui: &mut egui::Ui) {
        if self.quests.discover {
            return self.discovery_page(ui);
        }
        if self.quests.shop {
            return self.shop_page(ui);
        }
        let t = self.theme.clone();
        let lang = self.lang;
        let mobile = self.mobile_active();
        let now = self.store.now();
        let mut refresh = false;
        egui::Panel::top("quests_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                self.mobile_back(ui);
                let side = super::chat_head::icon_side(&t);
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                icons::paint(Icon::Quest, ui.painter(), pixel_square(ui, slot.min, side), t.colors.muted);
                ui.label(RichText::new(qt::QUESTS.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                ui.separator();
                for (done, text) in [(false, qt::TAB_AVAILABLE), (true, qt::TAB_CLAIMED)] {
                    if tab(ui, &t, text.get(lang), self.quests.done == done).clicked() {
                        self.quests.done = done;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let busy = self.store.quests_loading();
                    if ui.add_enabled(!busy, super::form::btn(&t, qt::REFRESH.get(lang))).clicked() {
                        refresh = true;
                    }
                    // Solde d'Orbes : seulement si Discord l'a annoncé.
                    if let Some(n) = self.store.quests.orbs {
                        let label = ui.label(RichText::new(p::orbs(lang, n)).family(crate::ui::fonts::demi()).color(t.colors.text));
                        widgets::tip(label, &t, qt::ORBS_TIP.get(lang));
                    }
                });
            });
        });
        if refresh {
            self.store.quests_refresh();
        }
        let margin = if mobile { t.chat_head_margin() } else { egui::Margin { left: PAGE_X, right: PAGE_X, top: PAGE_Y, bottom: 0 } };
        let mut act = None;
        let (mut sort, mut filter) = (self.quests.sort, self.quests.filter);
        let mut unhide = false;
        let mut cards = Vec::new();
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(margin)).show(ui, |ui| {
            let list = match &self.store.quests.list {
                Remote::Ready(l) => l,
                Remote::Idle | Remote::Loading => return empty_state(ui, &t, Icon::Quest, qt::LOADING.get(lang)),
                Remote::Failed(problem) => {
                    let text = if *problem == Problem::Forbidden { qt::USER_ONLY.get(lang).to_string() } else { format!("{} {}", qt::FAILED.get(lang), super::words::problem(lang, problem, self.h24())) };
                    return empty_state(ui, &t, Icon::Quest, &text);
                }
            };
            let hidden = self.store.quests.hidden.len();
            // En-tête de la page : titre à gauche, tri et filtre à droite.
            ui.horizontal(|ui| {
                ui.set_min_height(widgets::button_height(&t));
                let head = if self.quests.done { qt::HEAD_CLAIMED } else { qt::HEAD_AVAILABLE };
                ui.add(egui::Label::new(RichText::new(head.get(lang)).family(crate::ui::fonts::demi()).size(t.typography.title).color(t.colors.text_strong)).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let b = head_button(ui, &t, qt::FILTER.get(lang), Icon::Sliders);
                    if !filter.is_empty() || hidden > 0 {
                        // Filtre actif : pastille d'accent au coin du bouton.
                        ui.painter().circle_filled(b.rect.right_top() + vec2(-3.0, 3.0), 4.0, t.colors.accent);
                    }
                    egui::Popup::menu(&b).close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside).show(|ui| {
                        ui.label(RichText::new(qt::FILTER_REWARDS.get(lang)).small().color(t.colors.muted));
                        for (text, on) in [(qt::FILTER_ORBS, &mut filter.orbs), (qt::FILTER_COSMETIC, &mut filter.cosmetic), (qt::FILTER_IN_GAME, &mut filter.in_game), (qt::FILTER_NITRO, &mut filter.nitro)] {
                            super::menu::check(ui, &t, text.get(lang), on);
                        }
                        super::menu::separator(ui, &t);
                        ui.label(RichText::new(qt::FILTER_TASKS.get(lang)).small().color(t.colors.muted));
                        for (text, on) in [(qt::FILTER_PLAY, &mut filter.play), (qt::FILTER_VIDEO, &mut filter.video), (qt::FILTER_OTHER, &mut filter.other)] {
                            super::menu::check(ui, &t, text.get(lang), on);
                        }
                        if !filter.is_empty() || hidden > 0 {
                            super::menu::separator(ui, &t);
                        }
                        if !filter.is_empty() && super::menu::item(ui, &t, qt::FILTER_RESET.get(lang), None, false).clicked() {
                            filter = Filter::default();
                        }
                        if hidden > 0 && super::menu::item(ui, &t, &f::hidden_show(lang, hidden), None, false).clicked() {
                            unhide = true;
                        }
                    });
                    let current = match sort {
                        Sort::Suggested => qt::SORT_SUGGESTED,
                        Sort::Newest => qt::SORT_NEWEST,
                        Sort::EndingSoon => qt::SORT_ENDING,
                    };
                    let b = head_button(ui, &t, current.get(lang), Icon::ChevronDown);
                    egui::Popup::menu(&b).show(|ui| {
                        for (which, text) in [(Sort::Suggested, qt::SORT_SUGGESTED), (Sort::Newest, qt::SORT_NEWEST), (Sort::EndingSoon, qt::SORT_ENDING)] {
                            if super::menu::radio(ui, &t, text.get(lang), sort == which).clicked() {
                                sort = which;
                                ui.close();
                            }
                        }
                    });
                });
            });
            if let Some(until) = self.store.quests.blocked_until.filter(|u| *u > now) {
                ui.label(RichText::new(f::blocked_until(lang, crate::i18n::time::long_date(lang, until))).small().color(t.colors.danger));
            }
            ui.add_space(t.spacing.item_y * 2.0);
            let quests = arrange(list, self.quests.done, sort, filter, &self.store.quests.hidden, now);
            if quests.is_empty() {
                let none = if !filter.is_empty() || hidden > 0 { qt::NONE_MATCH } else if self.quests.done { qt::NONE_CLAIMED } else { qt::NONE_AVAILABLE };
                return empty_state(ui, &t, Icon::Quest, none.get(lang));
            }
            let gap = t.spacing.item_x * 2.0;
            egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                let avail = ui.available_width();
                let (cols, width) = columns(avail, gap);
                let height = CardLayout::new(&t, width).height;
                for row in quests.chunks(cols) {
                    let (line, _) = ui.allocate_exact_size(vec2(avail, height), Sense::hover());
                    for (i, q) in row.iter().enumerate() {
                        let rect = Rect::from_min_size(pos2(line.left() + i as f32 * (width + gap), line.top()), vec2(width, height));
                        cards.push((q.id, rect));
                        // Seules les cartes visibles demandent leurs images.
                        if ui.is_rect_visible(rect)
                            && let Some(a) = self.quest_card(ui, &t, q, rect, now)
                        {
                            act = Some(a);
                        }
                    }
                    ui.add_space(gap);
                }
            });
        });
        (self.quests.sort, self.quests.filter, self.quests.cards) = (sort, filter, cards);
        if unhide {
            self.store.quests.unhide_all();
        }
        if let Some(id) = self.quests.detail
            && let Some(a) = self.quest_detail(ui.ctx(), &t, id, now)
        {
            act = Some(a);
        }
        match act {
            Some(Act::Enroll(id)) => {
                self.store.quest_enroll(id);
            }
            Some(Act::Claim(id)) => {
                self.store.quest_claim(id);
            }
            Some(Act::Code(id)) => {
                self.store.quest_code(id);
            }
            Some(Act::Copy(code)) => {
                ui.ctx().copy_text(code);
                self.notify(qt::CODE_COPIED.get(lang));
            }
            Some(Act::CopyLink(id)) => {
                ui.ctx().copy_text(official_url(id));
                self.notify(qt::LINK_COPIED.get(lang));
            }
            Some(Act::Official(id)) => self.open_link(&official_url(id)),
            Some(Act::Open(link)) => self.open_link(&link),
            Some(Act::Hide(id)) => {
                self.store.quests.hide(id);
                self.quests.detail = None;
            }
            Some(Act::Detail(id)) => self.quests.detail = Some(id),
            None => {}
        }
    }

    /// Second bouton d'une quête : ce qu'elle propose (page du jeu,
    /// invitation), ouvert dans le navigateur ou dans Ferrocord ; à défaut, la
    /// quête dans Discord. Rien n'est déclaré à Discord.
    fn quest_link(&self, q: &Quest) -> (crate::i18n::Text, Option<crate::i18n::Text>, Act) {
        match &q.link {
            Some(link) if q.link_is_invite() => (qt::JOIN_SERVER, None, Act::Open(link.clone())),
            Some(link) if q.main_task().is_some_and(|t| t.kind.class() == TaskClass::Play) => (qt::PLAY, Some(qt::PLAY_TIP), Act::Open(link.clone())),
            Some(link) => (qt::LEARN_MORE, Some(qt::PLAY_TIP), Act::Open(link.clone())),
            None => (qt::OPEN_OFFICIAL, None, Act::Official(q.id)),
        }
    }

    /// Bouton principal d'une quête selon son étape : libellé, rôle, infobulle,
    /// et ce que le clic demande (`None` : bouton inerte).
    fn quest_primary(&self, q: &Quest, stage: Stage, now: i64) -> (String, Kind, Option<crate::i18n::Text>, Option<Act>) {
        let lang = self.lang;
        let s = &self.store.quests;
        if s.busy(q.id) {
            return (qt::BUSY.get(lang).to_string(), Kind::Secondary, None, None);
        }
        match stage {
            Stage::Open if s.blocked_until.is_some_and(|u| u > now) => (qt::ACCEPT.get(lang).to_string(), Kind::Primary, Some(qt::BLOCKED), None),
            Stage::Open => (qt::ACCEPT.get(lang).to_string(), Kind::Primary, None, Some(Act::Enroll(q.id))),
            // Ferrocord ne joue ni ne regarde à la place de l'utilisateur : la
            // quête se fait dans l'application officielle, et la progression
            // affichée est celle que Discord renvoie.
            Stage::Accepted => {
                let label = match q.progress().filter(|(value, _)| *value > 0) {
                    Some((value, target)) if q.tasks.iter().any(|k| k.kind.timed()) => f::progress_time(lang, span(lang, value), span(lang, target)),
                    Some((value, target)) => f::progress_count(lang, value, target),
                    None => qt::START.get(lang).to_string(),
                };
                (label, Kind::Primary, Some(qt::FINISH_OFFICIAL_TIP), Some(Act::Official(q.id)))
            }
            Stage::Completed if q.claim_here => (qt::CLAIM.get(lang).to_string(), Kind::Primary, None, Some(Act::Claim(q.id))),
            Stage::Completed => (qt::CLAIM_OFFICIAL.get(lang).to_string(), Kind::Primary, Some(qt::CLAIM_OFFICIAL_TIP), Some(Act::Official(q.id))),
            Stage::Claimed => match s.code(q.id) {
                Some(code) => (qt::COPY_CODE.get(lang).to_string(), Kind::Primary, None, Some(Act::Copy(code.to_string()))),
                None if q.reward().is_some_and(|r| matches!(r.kind, RewardKind::Code | RewardKind::InGame)) => (qt::SHOW_CODE.get(lang).to_string(), Kind::Primary, None, Some(Act::Code(q.id))),
                None => (qt::CLAIMED_DONE.get(lang).to_string(), Kind::Secondary, None, None),
            },
            Stage::Expired => (qt::STAGE_EXPIRED.get(lang).to_string(), Kind::Secondary, None, None),
        }
    }

    /// Texte du refus affiché sur la carte, s'il y en a un.
    fn quest_refusal(&self, id: u64) -> Option<String> {
        let lang = self.lang;
        match self.store.quests.refused(id)? {
            Refused::Captcha => Some(qt::CAPTCHA.get(lang).to_string()),
            Refused::Other(problem) => Some(format!("{} {}", qt::REFUSED.get(lang), super::words::problem(lang, problem, self.h24()))),
        }
    }

    /// Une carte, dans `rect` (hauteur imposée par [`CardLayout`]).
    fn quest_card(&self, ui: &mut egui::Ui, t: &Tokens, q: &Quest, rect: Rect, now: i64) -> Option<Act> {
        let lang = self.lang;
        let c = &t.colors;
        let l = CardLayout::new(t, rect.width());
        let stage = q.stage(now);
        let light = t.is_light();
        let fill = t.card_fill();
        let radius = t.r_card();
        let (name, game) = names(q);
        let mut act = None;
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect).id_salt(("quete", q.id)));
        let ui = &mut child;
        // Le clic sur la carte (hors de ses boutons, posés après elle) ouvre le détail.
        let card = ui.interact(rect, ui.scope_id().with("carte"), Sense::click());
        card.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} {name}", qt::DETAILS.get(lang))));
        let p = ui.painter().clone();
        p.rect_filled(rect, radius, fill);

        // Image d'ambiance, fondue vers le fond de la carte ; à défaut, un fond neutre.
        let hero = Rect::from_min_size(rect.min, vec2(rect.width(), l.hero_h));
        let top = egui::CornerRadius { sw: 0, se: 0, ..radius };
        p.rect_filled(hero, top, super::motion::mix(fill, c.muted, 0.14));
        if let Image::Ready(tex) = self.quest_image(q.id, 0, &q.hero_urls(), hero.size(), true, false) {
            p.add(images::textured(hero, top, tex));
        }
        fade(&p, Rect::from_min_max(pos2(hero.left(), hero.top() + (hero.height() * 0.4).round()), hero.max), fill);

        // Logotype du jeu, en bas à gauche de l'image ; à défaut, son nom.
        let logo_bottom = hero.bottom() + LOGO_DROP;
        let logo_box = Rect::from_min_max(pos2(rect.left() + l.pad, logo_bottom - l.logo_h), pos2(rect.left() + l.pad + (rect.width() * 0.5).round(), logo_bottom));
        match self.quest_image(q.id, 2, &q.logo_urls(light), logo_box.size(), false, light) {
            Image::Ready(tex) => {
                let size = ui.ctx().tex_manager().read().meta(tex).map(|m| m.size).filter(|[w, h]| *w > 0 && *h > 0);
                if let Some([w, h]) = size {
                    let k = (logo_box.width() / w as f32).min(logo_box.height() / h as f32);
                    let at = Rect::from_min_size(pos2(logo_box.left(), logo_box.bottom() - h as f32 * k), vec2(w as f32 * k, h as f32 * k));
                    p.image(tex, at, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
                }
            }
            Image::Failed => {
                let galley = p.layout_job(clamped(game, crate::ui::fonts::demi_font(t.typography.title), c.text_strong, logo_box.width(), 1));
                p.galley(pos2(logo_box.left(), logo_box.bottom() - galley.size().y), galley, c.text_strong);
            }
            Image::Pending => {}
        }

        // « … » : lien de la quête, ouverture dans Discord, masquage local.
        let side = (t.typography.body * 1.75).round();
        let more = Rect::from_min_size(pos2(rect.right() - l.pad * 0.5 - side, rect.top() + l.pad * 0.5), egui::Vec2::splat(side));
        let more_resp = ui.allocate_rect(more, Sense::click());
        more_resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{} {name}", qt::MORE.get(lang))));
        let veil = if more_resp.hovered() || more_resp.has_focus() { Color32::from_black_alpha(200) } else { t.media_veil() };
        p.circle_filled(more.center(), side / 2.0, veil);
        icons::paint(Icon::More, &p, pixel_square(ui, more.min + egui::Vec2::splat(side * 0.2), side * 0.6), t.on_media());
        egui::Popup::menu(&more_resp).id(ui.scope_id().with("menu")).show(|ui| {
            if super::menu::item(ui, t, qt::COPY_LINK.get(lang), Some(Icon::Link), false).clicked() {
                act = Some(Act::CopyLink(q.id));
            }
            if super::menu::item(ui, t, qt::OPEN_OFFICIAL.get(lang), Some(Icon::ArrowRight), false).clicked() {
                act = Some(Act::Official(q.id));
            }
            let hide = super::menu::item(ui, t, qt::HIDE.get(lang), Some(Icon::Eye), false);
            if widgets::tip(hide, t, qt::HIDE_TIP.get(lang)).clicked() {
                act = Some(Act::Hide(q.id));
            }
        });
        let more_resp = widgets::tip(more_resp, t, qt::MORE.get(lang)).on_hover_cursor(egui::CursorIcon::PointingHand);
        let _ = more_resp;

        // « Promotion par ✓ Éditeur » à gauche, « Finit le 16/11 » à droite.
        let small = egui::TextStyle::Small.resolve(ui.style());
        let row = Rect::from_min_size(pos2(rect.left() + l.pad, rect.top() + l.promo_y), vec2(rect.width() - 2.0 * l.pad, l.promo_h));
        let mut right = row.right();
        if let Some(e) = q.expires_ms {
            let date = crate::i18n::time::short_date(lang, e);
            let text = if e > now { f::ends_short(lang, date) } else { f::ended_short(lang, date) };
            let galley = p.layout_no_wrap(text, small.clone(), c.muted);
            right = row.right() - galley.size().x;
            p.galley(pos2(right, row.center().y - galley.size().y / 2.0), galley, c.muted);
            right -= t.spacing.item_x;
        }
        let publisher = if q.publisher.is_empty() { game } else { &q.publisher };
        let lead = p.layout_no_wrap(qt::PROMOTED_BY.get(lang).to_string(), small.clone(), c.muted);
        let mut x = row.left();
        let lead_w = lead.size().x;
        if x + lead_w < right {
            p.galley(pos2(x, row.center().y - lead.size().y / 2.0), lead, c.muted);
            x += lead_w + 4.0;
        }
        let badge = Tokens::icon_side(t.typography.small);
        if x + badge < right {
            icons::paint(Icon::Verified, &p, pixel_square(ui, pos2(x, row.center().y - badge / 2.0), badge), t.readable(c.online, fill));
            x += badge + 4.0;
        }
        if right - x > badge {
            let galley = p.layout_job(clamped(publisher, crate::ui::fonts::medium_font(t.typography.small), c.text, right - x, 1));
            p.galley(pos2(x, row.center().y - galley.size().y / 2.0), galley, c.text);
        }

        // Récompense : vignette carrée à gauche ; quête, titre et tâche à droite.
        let tile = Rect::from_min_size(pos2(rect.left() + l.pad, rect.top() + l.block_y), egui::Vec2::splat(l.tile));
        p.rect_filled(tile, t.r_media(), super::motion::mix(fill, c.muted, 0.14));
        match self.quest_image(q.id, 1, &q.reward_urls(light), tile.size(), true, light) {
            Image::Ready(tex) => {
                p.add(images::textured(tile, t.r_media(), tex));
            }
            _ => icons::paint(Icon::Quest, &p, tile.shrink(l.tile * 0.28), c.faint),
        }
        let text_left = tile.right() + (l.pad * 0.75).round();
        let text_rect = Rect::from_min_max(pos2(text_left, tile.top()), pos2(rect.right() - l.pad, tile.bottom()));
        let width = text_rect.width();
        let tp = p.with_clip_rect(text_rect.intersect(p.clip_rect()));
        let accent = t.readable(c.accent, fill);
        let mut y = text_rect.top();
        let eyebrow = tp.layout_job(clamped(&f::eyebrow(lang, name).to_uppercase(), crate::ui::fonts::demi_font((t.typography.small * 0.92).round()), accent, width, 2));
        let eyebrow_h = eyebrow.size().y;
        tp.galley(pos2(text_left, y), eyebrow, accent);
        y += eyebrow_h + 2.0;
        let title = tp.layout_job(clamped(&f::claim_title(lang, reward_text(lang, q)), crate::ui::fonts::demi_font(t.typography.body + 1.0), c.text_strong, width, 2));
        let title_h = title.size().y;
        tp.galley(pos2(text_left, y), title, c.text_strong);
        y += title_h + 2.0;
        // La tâche, sur ce qu'il reste de hauteur (deux lignes au plus) ; un
        // refus ou le code de la récompense la remplacent.
        let line = small.size * 1.3;
        let rows = (((text_rect.bottom() - y) / line + 0.15).floor() as usize).min(2);
        let refusal = self.quest_refusal(q.id);
        let (mut note, mut full) = (String::new(), tasks_summary(lang, q));
        if rows > 0 {
            let mut job = egui::text::LayoutJob::default();
            let plain = |color| egui::TextFormat { font_id: small.clone(), color, ..Default::default() };
            if let Some(text) = &refusal {
                job.append(text, 0.0, plain(c.danger));
                full = text.clone();
            } else if let Some(code) = self.store.quests.code(q.id) {
                job.append(code, 0.0, egui::TextFormat { font_id: FontId::monospace(small.size), color: c.text_strong, ..Default::default() });
            } else {
                let (text, at) = describe(lang, q);
                match at {
                    Some((a, b)) => {
                        job.append(&text[..a], 0.0, plain(c.muted));
                        job.append(&text[a..b], 0.0, plain(t.readable(c.link, fill)));
                        job.append(&text[b..], 0.0, plain(c.muted));
                    }
                    None => job.append(&text, 0.0, plain(c.muted)),
                }
                note = text;
            }
            job.wrap = egui::text::TextWrapping { max_width: width.max(1.0), max_rows: rows, ..Default::default() };
            let galley = tp.layout_job(job);
            tp.galley(pos2(text_left, y), galley, c.muted);
        }
        // Ce que la carte ne dit pas en entier va dans l'infobulle du bloc.
        if !note.is_empty() && !full.is_empty() && refusal.is_none() {
            full = format!("{note}\n{full}");
        }
        if !full.is_empty() {
            let hover = ui.interact(text_rect, ui.scope_id().with("texte"), Sense::hover());
            widgets::tip(hover, t, &full);
        }

        // Pied : filet, puis deux boutons de même largeur.
        let sep = rect.top() + l.sep_y;
        p.hline(rect.x_range(), sep, egui::Stroke::new(1.0, c.border));
        if stage == Stage::Accepted
            && let Some((value, target)) = q.progress().filter(|(value, _)| *value > 0)
        {
            // Progression renvoyée par Discord, jamais calculée ici.
            let share = (value as f32 / target.max(1) as f32).clamp(0.0, 1.0);
            p.hline(egui::Rangef::new(rect.left(), rect.left() + rect.width() * share), sep, egui::Stroke::new(2.0, accent));
        }
        let gap = (l.pad * 0.6).round();
        let bw = ((rect.width() - 2.0 * l.pad - gap) / 2.0).floor();
        let first = Rect::from_min_size(pos2(rect.left() + l.pad, rect.top() + l.btn_y), vec2(bw, l.btn_h));
        let second = Rect::from_min_size(pos2(rect.right() - l.pad - bw, rect.top() + l.btn_y), vec2(bw, l.btn_h));
        let (label, tip, target) = self.quest_link(q);
        let b = widgets::button_in(ui, t, Kind::Secondary, label.get(lang), None, first);
        let b = match tip {
            Some(tip) => widgets::tip(b, t, tip.get(lang)),
            None => b,
        };
        if b.clicked() {
            act = Some(target);
        }
        let (label, kind, tip, target) = self.quest_primary(q, stage, now);
        let b = ui.scope_builder(egui::UiBuilder::new().max_rect(second), |ui| {
            if target.is_none() {
                ui.disable();
            }
            widgets::button_in(ui, t, kind, &label, None, second)
        }).inner;
        let b = match tip {
            Some(tip) => widgets::tip(b, t, tip.get(lang)),
            None => b,
        };
        if b.clicked() && let Some(target) = target {
            act = Some(target);
        }

        p.rect_stroke(rect, radius, t.border(), egui::StrokeKind::Inside);
        super::keys::focus_ring(ui, t, &card);
        if card.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() && act.is_none() {
            act = Some(Act::Detail(q.id));
        }
        act
    }

    /// Fenêtre de détail d'une quête : tout ce que la carte abrège (dates,
    /// éditeur, tâches et plateformes possibles, où elle se termine, code).
    fn quest_detail(&mut self, ctx: &egui::Context, t: &Tokens, id: u64, now: i64) -> Option<Act> {
        let lang = self.lang;
        let c = &t.colors;
        let Some(q) = self.store.quests.quest(id) else {
            self.quests.detail = None;
            return None;
        };
        let stage = q.stage(now);
        let (name, game) = names(q);
        let mut act = None;
        let mut footer_act = None;
        let width = crate::ui::mobile::card_width(ctx, t, 1.0);
        // Vidéo de la quête, jouée par le lecteur de Ferrocord quand la
        // progression est déclarée ici (réglage) : présentée comme une pièce jointe.
        let report = self.local.prefs.quest_progress && stage == Stage::Accepted;
        let video = q.video_task().and_then(|k| k.video.clone()).filter(|_| report).map(|url| crate::state::model::Attachment {
            id,
            name: "quete.mp4".into(),
            url,
            content_type: Some("video/mp4".into()),
            width: 1280,
            height: 720,
            kind: crate::state::model::MediaKind::Video,
            ..Default::default()
        });
        let out = widgets::modal(
            ctx,
            t,
            egui::Id::unique("quete-detail"),
            width,
            name,
            crate::i18n::t::CLOSE_TIP.get(lang),
            |ui| {
                ui.spacing_mut().item_spacing.y = t.spacing.item_y;
                let by = if q.publisher.is_empty() { game.to_string() } else { f::by_publisher(lang, game, &q.publisher) };
                ui.add(egui::Label::new(RichText::new(by).color(c.muted)).wrap());
                if let Some(e) = q.expires_ms {
                    let date = crate::i18n::time::long_date(lang, e);
                    ui.label(RichText::new(if e > now { f::ends(lang, date) } else { f::ended(lang, date) }).small().color(c.muted));
                }
                let chip = match stage {
                    Stage::Accepted => Some((qt::STAGE_ACCEPTED, c.muted)),
                    Stage::Completed => Some((qt::STAGE_COMPLETED, c.online)),
                    Stage::Claimed => Some((qt::STAGE_CLAIMED, c.online)),
                    Stage::Expired => Some((qt::STAGE_EXPIRED, c.muted)),
                    Stage::Open => None,
                };
                if let Some((text, color)) = chip {
                    ui.label(RichText::new(text.get(lang)).small().family(crate::ui::fonts::demi()).color(t.readable(color, c.bg2)));
                }
                ui.add_space(t.spacing.item_y);
                ui.label(RichText::new(qt::REWARD.get(lang)).small().color(c.muted));
                let reward = match q.reward() {
                    Some(r) if !r.name.is_empty() => r.name.clone(),
                    _ => reward_text(lang, q),
                };
                ui.add(egui::Label::new(RichText::new(reward).family(crate::ui::fonts::demi()).color(c.text_strong)).wrap());
                if !q.tasks.is_empty() {
                    ui.add_space(t.spacing.item_y);
                    ui.label(RichText::new(qt::DETAIL_TASKS.get(lang)).small().color(c.muted));
                    let (text, _) = describe(lang, q);
                    if !text.is_empty() {
                        ui.add(egui::Label::new(RichText::new(text).color(c.text)).wrap());
                    }
                    ui.add(egui::Label::new(RichText::new(tasks_summary(lang, q)).small().color(c.text)).wrap());
                }
                if let Some(a) = &video {
                    ui.add_space(t.spacing.item_y);
                    ui.add(egui::Label::new(RichText::new(qt::PROGRESS_VIDEO.get(lang)).small().color(c.muted)).wrap());
                    let mut hot = super::attachments::Hot::default();
                    super::player::card(ui, t, lang, Some(&self.images), a, &mut hot);
                    hot.interact(ui, t, id);
                }
                // Progression : celle que Discord renvoie, jamais calculée ici.
                if stage == Stage::Accepted
                    && let Some((value, target)) = q.progress()
                {
                    let timed = q.tasks.iter().any(|k| k.kind.timed());
                    let text = if timed { f::progress_time(lang, span(lang, value), span(lang, target)) } else { f::progress_count(lang, value, target) };
                    ui.add(egui::ProgressBar::new(value as f32 / target.max(1) as f32).desired_height(t.typography.small / 2.0).fill(c.accent).corner_radius(t.r_row()));
                    ui.label(RichText::new(text).small().color(c.muted));
                }
                if matches!(stage, Stage::Open | Stage::Accepted) {
                    ui.add_space(t.spacing.item_y);
                    // Progression déclarée ici (réglage) : seulement pour un jeu sur ordinateur dont l'application est connue.
                    let here = stage == Stage::Accepted && self.local.prefs.quest_progress && !q.play_apps().is_empty();
                    let tip = match here {
                        true if self.store.quests.progress_refused(id) && self.store.quests.needs_identity => qt::PROGRESS_IDENTITY,
                        true if self.store.quests.progress_refused(id) => qt::PROGRESS_STOPPED,
                        true if self.store.quests.beating(id) => qt::PROGRESS_PLAYING,
                        true => qt::PROGRESS_PLAY,
                        false => qt::FINISH_OFFICIAL_TIP,
                    };
                    ui.add(egui::Label::new(RichText::new(tip.get(lang)).small().color(c.muted)).wrap());
                }
                if stage == Stage::Completed && !q.claim_here {
                    ui.add(egui::Label::new(RichText::new(qt::CLAIM_OFFICIAL_TIP.get(lang)).small().color(c.muted)).wrap());
                }
                if let Some(text) = self.quest_refusal(id) {
                    ui.add(egui::Label::new(RichText::new(text).small().color(c.danger)).wrap());
                }
                if let Some(code) = self.store.quests.code(id) {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(RichText::new(code).monospace().color(c.text_strong));
                        if ui.add(super::form::link(t, qt::COPY_CODE.get(lang))).clicked() {
                            act = Some(Act::Copy(code.to_string()));
                        }
                    });
                }
            },
            |ui| {
                if widgets::button(ui, t, Kind::Primary, qt::OPEN_OFFICIAL.get(lang)).clicked() {
                    footer_act = Some(Act::Official(id));
                }
                if widgets::button(ui, t, Kind::Secondary, qt::COPY_LINK.get(lang)).clicked() {
                    footer_act = Some(Act::CopyLink(id));
                }
            },
        );
        if out.closed {
            self.quests.detail = None;
        }
        self.quest_watch_tick(ctx, id, video.is_some());
        act.or(footer_act)
    }

    /// Temps réellement lu de la vidéo de la quête `id` : seule l'avance
    /// continue de la lecture compte (ni pause, ni saut en avant, ni lecteur
    /// fermé), et c'est ce cumul qui est déclaré (`Store::quest_watched`).
    fn quest_watch_tick(&mut self, ctx: &egui::Context, id: u64, on: bool) {
        /// Avance la plus longue comptée entre deux images : au-delà, c'est un saut.
        const MAX_STEP_US: u64 = 2_000_000;
        let Some((playing, pos)) = super::player::progress(ctx, id).filter(|_| on) else {
            self.quests.watch = None;
            return;
        };
        let w = match &mut self.quests.watch {
            Some(w) if w.0 == id => w,
            slot => slot.insert((id, pos, 0)),
        };
        let step = pos.saturating_sub(w.1);
        if playing && step > 0 && step <= MAX_STEP_US {
            w.2 += step;
        }
        w.1 = pos;
        let seconds = u32::try_from(w.2 / 1_000_000).unwrap_or(u32::MAX);
        self.store.quest_watched(id, seconds, true);
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, find, frame, offline_boot};
    use super::*;
    use crate::state::model::User;
    use crate::state::quests::{QuestCmd, QuestEvent, Status};
    use crate::state::{Cmd, Event, Store};
    use std::sync::{Arc, Mutex};

    struct Recorder(Arc<Mutex<Vec<Cmd>>>);

    impl super::super::Backend for Recorder {
        fn set_wake(&mut self, _: crate::net::Wake) {}
        fn connect(&mut self, _: crate::account::Token) {}
        fn disconnect(&mut self) {}
        fn is_connected(&self) -> bool {
            true
        }
        fn poll(&mut self) -> Vec<Event> {
            Vec::new()
        }
        fn send(&mut self, cmds: Vec<Cmd>) {
            if let Ok(mut v) = self.0.lock() {
                v.extend(cmds);
            }
        }
    }

    fn fixture() -> Vec<Quest> {
        crate::net::decode_quests(include_str!("../../tests/fixtures/nd/rest_quests.json")).map(|(l, _)| l).unwrap_or_default()
    }

    fn forms() -> Vec<Quest> {
        crate::net::decode_quests(include_str!("../../tests/fixtures/nd/rest_quests_assets.json")).map(|(l, _)| l).unwrap_or_default()
    }

    /// Trois colonnes en grande fenêtre, deux puis une quand elle rétrécit ;
    /// la hauteur d'une carte ne dépend que de sa largeur.
    #[test]
    fn grid_columns_and_card_height_follow_the_width() {
        let gap = 16.0;
        assert_eq!(columns(1300.0, gap), (3, 422.0));
        assert_eq!(columns(940.0, gap).0, 3);
        assert_eq!(columns(900.0, gap), (2, 442.0));
        assert_eq!(columns(620.0, gap).0, 2);
        assert_eq!(columns(600.0, gap), (1, 520.0), "une colonne : la carte ne s'étire pas au-delà de sa largeur maximale");
        assert_eq!(columns(280.0, gap), (1, 280.0));
        assert_eq!(columns(1800.0, gap).0, 4, "très grande fenêtre : une colonne de plus plutôt que des cartes démesurées");
        let t = Tokens::default();
        let (a, b) = (CardLayout::new(&t, 320.0), CardLayout::new(&t, 440.0));
        assert!(b.height > a.height && b.height - a.height == b.hero_h - a.hero_h, "seule l'image d'ambiance suit la largeur");
        assert!(a.btn_y + a.btn_h < a.height && a.sep_y < a.btn_y && a.block_y + a.tile < a.sep_y);
    }

    /// Phrases de la carte : récompense avec son article, tâche principale,
    /// nom du jeu repéré pour être coloré ; consigne de l'éditeur d'abord.
    #[test]
    fn card_sentences() {
        let list = fixture();
        let (text, at) = describe(Lang::Fr, &list[0]);
        assert_eq!(text, "Joue à Jeu Synthétique 3000 pendant 15 minutes pour gagner un Casque fictif.");
        assert_eq!(at.map(|(a, b)| &text[a..b]), Some("Jeu Synthétique 3000"));
        assert_eq!(describe(Lang::En, &list[0]).0, "Play Jeu Synthétique 3000 for 15 minutes to win un Casque fictif.");
        assert_eq!(describe(Lang::Fr, &list[1]).0, "Regarde la vidéo de Film Synthétique pour gagner 700 Orbes.");
        assert_eq!(f::claim_title(Lang::Fr, reward_text(Lang::Fr, &list[0])), "Récupère un Casque fictif");
        assert_eq!(f::eyebrow(Lang::Fr, names(&list[0]).0).to_uppercase(), "QUÊTE\u{a0}: CHASSE AUX DONNÉES FICTIVES");
        let forms = forms();
        let (text, at) = describe(Lang::Fr, &forms[0]);
        assert!(text.starts_with("Trouve les 7 cartes fictives"), "{text}");
        assert_eq!(at.map(|(a, b)| &text[a..b]), Some("Cartes Synthétiques"));
        assert_eq!(tasks_summary(Lang::Fr, &list[0]), "Une seule de ces tâches suffit\u{a0}:\n•\u{a0}Jouer sur ordinateur (15\u{a0}min)\n•\u{a0}Jouer sur PlayStation (15\u{a0}min)\n•\u{a0}Jouer sur Xbox (15\u{a0}min)");
        assert_eq!(span(Lang::Fr, 900), "15\u{a0}min");
        assert_eq!(span(Lang::En, 30), "30 s");
        assert_eq!(span_long(Lang::Fr, 60), "1 minute");
    }

    fn walk(s: &egui::Shape, out: &mut String) {
        match s {
            egui::Shape::Text(t) => {
                out.push_str(&t.galley.job.text);
                out.push(' ');
            }
            egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, out)),
            _ => {}
        }
    }

    /// Textes peints (les libellés simples n'ont pas de nœud d'accessibilité).
    fn drawn(out: &egui::FullOutput) -> String {
        let mut text = String::new();
        out.shapes.iter().for_each(|c| walk(&c.shape, &mut text));
        text
    }

    fn app_with(log: &Arc<Mutex<Vec<Cmd>>>) -> super::super::App {
        let ctx = egui::Context::default();
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        store.select_dms();
        let images = crate::images::Images::disabled(ctx.clone());
        super::super::App::with_context(ctx, offline_boot(Box::new(Recorder(log.clone())), store, images))
    }

    /// La liste part à l'ouverture de la page ; accepter et réclamer partent
    /// au clic ; rien d'autre ne part jamais (aucune progression).
    #[test]
    fn page_lists_quests_and_writes_only_on_click() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut app = app_with(&log);
        let sent = |log: &Arc<Mutex<Vec<Cmd>>>| log.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default();

        frame(&mut app, Vec::new());
        assert!(!sent(&log).iter().any(|c| matches!(c, Cmd::Quests(_))), "rien ne part tant que la page n'est pas ouverte");
        click(&mut app, qt::QUESTS.get(Lang::Fr));
        assert!(app.quests_shown());
        frame(&mut app, Vec::new());
        assert_eq!(sent(&log).into_iter().filter(|c| matches!(c, Cmd::Quests(_))).collect::<Vec<_>>(), vec![Cmd::Quests(QuestCmd::List)]);

        app.store.apply(Event::Quests(QuestEvent::List(Ok((fixture(), None)))));
        app.store.apply(Event::Quests(QuestEvent::Orbs(1400)));
        let out = frame(&mut app, Vec::new());
        let drawn_now = drawn(&out);
        for text in [qt::HEAD_AVAILABLE.get(Lang::Fr), "TOUR DE PISTE FICTIF", "Éditions Fictives", "Récupère Voiture fictive", "BANDE-ANNONCE FICTIVE", "Récupère 700 Orbes", "400 Orbes", "Promotion par", "Finit le"] {
            assert!(drawn_now.contains(text), "{text} absent de {drawn_now}");
        }
        assert!(!drawn_now.contains("offres sponsorisées") && !drawn_now.contains('•'), "ni phrase d'explication ni liste à puces sur la page");
        // Toutes les cartes ont la même taille, et celles d'une rangée le même haut.
        let cards = app.quests.cards.clone();
        assert_eq!(cards.len(), 3);
        assert!(cards.iter().all(|(_, r)| r.size() == cards[0].1.size()), "{cards:?}");
        assert_eq!(cards[0].1.top(), cards[1].1.top(), "deux colonnes dans la fenêtre d'essai");
        assert!(cards[2].1.top() > cards[0].1.bottom());
        assert!(find(&out, qt::ACCEPT.get(Lang::Fr)).is_some(), "quête proposée : bouton « Accepter »");
        assert!(find(&out, "15\u{a0}s sur 30\u{a0}s").is_some(), "quête acceptée : la progression renvoyée par Discord");
        assert!(find(&out, qt::CLAIM_OFFICIAL.get(Lang::Fr)).is_some(), "récompense de console : à réclamer dans Discord");
        assert!(find(&out, qt::PLAY.get(Lang::Fr)).is_some(), "lien de la quête : « Jouer »");

        // Le détail d'une quête dit ce que la carte abrège.
        app.show_quest_detail(8_000_000_000_000_000_001);
        let out = frame(&mut app, Vec::new());
        let detail = drawn(&frame(&mut app, Vec::new()));
        assert!(detail.contains("Jouer sur PlayStation") && detail.contains("Studio Fictif"), "{detail}");
        drop(out);
        app.quests.detail = None;

        // Sans la quête de console, la quête proposée remonte dans l'écran d'essai.
        app.store.apply(Event::Quests(QuestEvent::List(Ok((fixture().into_iter().filter(|q| q.id % 10 != 3).collect(), None)))));
        click(&mut app, qt::ACCEPT.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert_eq!(sent(&log), vec![Cmd::Quests(QuestCmd::Enroll { id: 8_000_000_000_000_000_001 })]);
        // Captcha demandé : l'action s'arrête, la carte le dit.
        app.store.apply(Event::Quests(QuestEvent::Enrolled { id: 8_000_000_000_000_000_001, result: Err(Refused::Captcha) }));
        let out = frame(&mut app, Vec::new());
        assert!(drawn(&out).contains("captcha"), "le refus pour captcha est dit");
        // Acceptée, sans progression : « Commencer la quête » ouvre Discord, sans rien déclarer.
        app.store.apply(Event::Quests(QuestEvent::Status { id: 8_000_000_000_000_000_001, status: Status { enrolled_at: Some(1), ..Default::default() } }));
        let out = frame(&mut app, Vec::new());
        assert!(find(&out, qt::START.get(Lang::Fr)).is_some());
        // Terminée d'après Discord : « Réclamer » part au clic.
        let done = Status { enrolled_at: Some(1), completed_at: Some(2), ..Default::default() };
        app.store.apply(Event::Quests(QuestEvent::Status { id: 8_000_000_000_000_000_001, status: done }));
        frame(&mut app, Vec::new());
        click(&mut app, qt::CLAIM.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert_eq!(sent(&log), vec![Cmd::Quests(QuestCmd::Claim { id: 8_000_000_000_000_000_001 })]);
        app.store.apply(Event::Quests(QuestEvent::Claimed { id: 8_000_000_000_000_000_001, result: Ok(Some(3)) }));
        frame(&mut app, Vec::new());
        click(&mut app, qt::TAB_CLAIMED.get(Lang::Fr));
        let out = frame(&mut app, Vec::new());
        assert!(drawn(&out).contains(qt::HEAD_CLAIMED.get(Lang::Fr)), "{}", drawn(&out));
        assert!(find(&out, qt::SHOW_CODE.get(Lang::Fr)).is_some());
        // Masquer : local, sans requête ; la quête quitte la page.
        click(&mut app, qt::TAB_AVAILABLE.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert_eq!(app.quests.cards.len(), 1);
        app.store.quests.hide(8_000_000_000_000_000_002);
        let out = frame(&mut app, Vec::new());
        assert!(app.quests.cards.is_empty() && drawn(&out).contains(qt::NONE_MATCH.get(Lang::Fr)));
        // Retour à la page Amis par sa ligne.
        click(&mut app, crate::i18n::table::social::t::FRIENDS.get(Lang::Fr));
        assert!(!app.quests_shown());
        assert!(sent(&log).iter().all(|c| !matches!(c, Cmd::Quests(_))), "aucune autre requête de quête");
    }

    /// Haut de la colonne des MP : Amis, Ton Nitro, Boutique, Quêtes, dans cet
    /// ordre, à la hauteur des entrées de la colonne. « Ton Nitro » ouvre la page
    /// « Nitro et facturation » des paramètres, sans rien demander.
    #[test]
    fn dm_column_head_has_four_entries_in_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut app = app_with(&log);
        let out = frame(&mut app, Vec::new());
        let row = |label: &str| find(&out, label).unwrap_or_else(|| panic!("« {label} » introuvable"));
        let rows = [row(crate::i18n::table::social::t::FRIENDS.get(Lang::Fr)), row(qt::NITRO_ENTRY.get(Lang::Fr)), row(crate::i18n::table::nd::t::SHOP.get(Lang::Fr)), row(qt::QUESTS.get(Lang::Fr))];
        assert!(rows.windows(2).all(|w| w[0].bottom() <= w[1].top() && w[0].left() == w[1].left() && w[0].height() == w[1].height()), "{rows:?}");
        assert_eq!(rows[0].height(), app.theme.dm_entry_row());
        click(&mut app, qt::NITRO_ENTRY.get(Lang::Fr));
        frame(&mut app, Vec::new());
        assert!(app.params.open && app.params.page() == super::super::settings::Page::Billing);
        assert!(!app.quests_shown());
        assert!(log.lock().is_ok_and(|v| !v.iter().any(|c| matches!(c, Cmd::Quests(_) | Cmd::Shop(_)))), "« Ton Nitro » ne demande ni quêtes ni boutique");
    }

    /// Page des quêtes au repos, fenêtre de détail ouverte : une fois les
    /// animations finies, plus aucune image n'est demandée.
    #[test]
    fn quests_page_at_rest_asks_for_no_frame() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut app = app_with(&log);
        app.show_quests(false);
        let mut list = fixture();
        list.extend(forms());
        app.store.apply(Event::Quests(QuestEvent::List(Ok((list, None)))));
        app.show_quest_detail(8_000_000_000_000_000_002);
        let ctx = app.ctx.clone();
        let mut time = 0.0;
        let mut frame = |app: &mut App| {
            let input = egui::RawInput { screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1100.0, 720.0))), time: Some(time), ..Default::default() };
            time += 1.0 / 60.0;
            let mut out = ctx.run_ui(input, |ui| {
                app.logic_pass();
                app.ui_pass(ui);
            });
            out.textures_delta.clear();
            out.viewport_output.get(&egui::ViewportId::ROOT).map(|v| v.repaint_delay)
        };
        for _ in 0..300 {
            frame(&mut app);
        }
        // Au repos : vingt images de suite sans réveil. Un travail de fond fini
        // en retard (image décodée sur un autre fil, machine d'essai chargée)
        // réveille encore une fois, ce qui est normal ; une page qui se
        // redessine sans fin n'atteint jamais le repos.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut calm = 0;
        while calm < 20 {
            if frame(&mut app) == Some(std::time::Duration::MAX) {
                calm += 1;
            } else {
                calm = 0;
                assert!(std::time::Instant::now() < deadline, "réveil demandé par {:?}", ctx.repaint_causes());
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(!app.quests.cards.is_empty());
    }

    /// Tri et filtre de la page : appliqués à la liste reçue, sans requête.
    #[test]
    fn sort_and_filter_reorder_the_grid_without_any_request() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let mut app = app_with(&log);
        app.show_quests(false);
        frame(&mut app, Vec::new());
        let mut list = fixture();
        list.extend(forms());
        app.store.apply(Event::Quests(QuestEvent::List(Ok((list, None)))));
        if let Ok(mut v) = log.lock() {
            v.clear();
        }
        let ids = |app: &mut super::super::App| {
            frame(app, Vec::new());
            app.quests.cards.iter().map(|(id, _)| id % 100).collect::<Vec<_>>()
        };
        assert_eq!(ids(&mut app), [3, 2, 11, 1, 12, 13], "à réclamer, acceptées, puis l'ordre de Discord ; la réclamée est dans l'autre onglet");
        app.quests.sort = Sort::Newest;
        assert_eq!(ids(&mut app), [13, 12, 11, 2, 1, 3]);
        app.quests.sort = Sort::EndingSoon;
        assert_eq!(ids(&mut app)[..2], [3, 11]);
        app.quests.filter = Filter { video: true, ..Default::default() };
        assert_eq!(ids(&mut app), [2]);
        app.quests.filter = Filter { in_game: true, play: true, ..Default::default() };
        app.quests.sort = Sort::Suggested;
        assert_eq!(ids(&mut app), [3, 1, 12, 13]);
        // Le menu de tri existe et porte le tri courant.
        let out = frame(&mut app, Vec::new());
        assert!(find(&out, qt::SORT_SUGGESTED.get(Lang::Fr)).is_some() && find(&out, qt::FILTER.get(Lang::Fr)).is_some());
        app.quests.done = true;
        app.quests.filter = Filter::default();
        assert_eq!(ids(&mut app), [14]);
        assert!(log.lock().map(|v| v.is_empty()).unwrap_or(false), "tri, filtre et onglets ne demandent rien");
    }
}
