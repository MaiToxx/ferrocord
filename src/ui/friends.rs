//! Page Amis, ouverte depuis l'accueil des messages privés (aucune
//! conversation choisie) : onglets En ligne / Tous / En attente / Bloqués,
//! recherche locale, liste virtualisée, actions sur chaque relation, ajout
//! d'un ami par son nom d'utilisateur, création d'un groupe, et colonne
//! « Actifs en ce moment » quand la fenêtre est assez large.
//!
//! Mesures du client (`docs/ecarts-visuels.md`, 2.13) : lignes de 62 sous un
//! filet, disques d'action de 36, recherche de 40, onglets à fond neutre.
//! Chaque écriture part en un clic ; retirer un ami et bloquer passent par le
//! menu « ⋮ » (celui de l'utilisateur) et demandent « Oui ». Les listes ne
//! sont triées et filtrées qu'à un changement des relations, des présences,
//! de l'onglet ou de la recherche.

use super::{
    util::{paint_avatar, pixel_square},
    words, App,
};
use crate::i18n::table::ff::t as ft;
use crate::i18n::table::people::{f as uf, t as ut};
use crate::i18n::table::social::{f, p, t as st};
use crate::i18n::Lang;
use crate::state::model::Avatar;
use crate::state::relations::{Activity, ActivityKind, FriendTab, OnlineStatus, Presence, RelKind, RelationsAction, RelationsCmd, RelationsNotice, MAX_GROUP, NOTE_MAX};
use crate::state::{Problem, Store};
use crate::theme::{icons, icons::Icon, Tokens};
use eframe::egui::{self, Align, Align2, Color32, Layout, RichText, Sense, TextStyle, TextWrapMode, WidgetText};

/// Longueur maximale d'un nom d'utilisateur Discord.
const USERNAME_MAX: usize = 32;
/// Largeur de la ligne « Explore des serveurs » de l'onglet « Ajouter », au plus.
const DISCOVER_ROW_WIDTH: f32 = 455.0;
/// Largeur de la carte « Ajouter » sous laquelle le bouton passe sous le champ.
const STACK_BELOW: f32 = 480.0;
/// Hauteur d'une ligne de personne.
pub(super) const ROW: f32 = 62.0;
/// Disque d'action en fin de ligne, et son icône.
const DISC: f32 = 36.0;
const DISC_ICON: f32 = 20.0;
/// Marges de la page.
pub(super) const PAGE_X: i8 = 30;
pub(super) const PAGE_Y: i8 = 16;
/// Colonne « Actifs en ce moment » : sa largeur, et la largeur de fenêtre sous laquelle elle disparaît.
const ACTIVE_WIDTH: f32 = 360.0;
const ACTIVE_MIN_WINDOW: f32 = 1200.0;

/// Révisions des relations et des présences dont dépend la liste affichée.
type Revs = (u64, u64);

#[derive(Default)]
pub(super) struct FriendsPage {
    pub(super) tab: FriendTab,
    query: String,
    adding: bool,
    add_name: String,
    /// Message joint à la demande (facultatif).
    add_note: String,
    /// Résultat du dernier envoi, montré sous la carte : (réussi, texte).
    add_result: Option<(bool, String)>,
    /// Liste affichée et ce dont elle dépend : (révisions des relations et des présences, onglet, recherche, choix de groupe).
    cache: Option<(Revs, FriendTab, String, bool, Vec<u64>)>,
    /// Cartes de la colonne « En ligne » (`friends_active.rs`), refaites quand relations, présences ou états vocaux changent.
    pub(super) active: super::friends_active::Cache,
    /// Création d'un groupe : amis choisis.
    pub(super) picking: Option<Vec<u64>>,
    /// Catégorie de contacts choisie (filtre de la liste) ; `None` : toutes.
    category: Option<String>,
    /// Champ de nom ouvert : création, ou renommage de la catégorie choisie.
    category_edit: Option<CategoryEdit>,
}

struct CategoryEdit {
    text: String,
    rename: bool,
    /// Nom refusé au dernier essai.
    refused: bool,
    fresh: bool,
}

/// Ce qu'un clic sur une ligne demande (appliqué après le dessin).
enum RowAction {
    Message(u64),
    Write(RelationsCmd),
    Toggle(u64),
}

/// Qui montre une ligne de personne.
pub(super) struct Who<'a> {
    pub id: u64,
    pub name: &'a str,
    pub username: &'a str,
    pub avatar: Option<Avatar>,
    /// Pastille de statut (amis seulement).
    pub status: Option<OnlineStatus>,
    pub second: &'a str,
    /// Petite icône devant la seconde ligne.
    pub lead: Lead,
    /// Utilisateur dont la ligne montre les parures (tag de serveur, style du
    /// nom, décoration d'avatar) ; aucune pour une demande.
    pub look: Option<u64>,
}

/// Icône devant la seconde ligne d'un ami, comme dans le client.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Lead {
    None,
    /// Manette : la ligne est le nom du jeu.
    Game,
    /// Écran vert : « En partage d'écran ».
    Screen,
    /// Casque : écoute (ligne d'un MP).
    Sound,
    /// Haut-parleur vert : en vocal sur un serveur commun.
    Voice,
}

/// Peint l'icône `lead` dans le carré `at` ; `ink` : couleur de la manette et
/// du casque (l'écran de partage est toujours vert), `bg` : fond de la ligne
/// (creux de la manette).
pub(super) fn paint_lead(ui: &egui::Ui, t: &Tokens, lead: Lead, at: egui::Rect, ink: Color32, bg: Color32) {
    match lead {
        Lead::Game => super::user_panel::paint_gamepad(ui.painter(), at, ink, bg),
        Lead::Screen => icons::paint(Icon::Screen, ui.painter(), at, t.colors.online),
        Lead::Sound => icons::paint(Icon::Headphones, ui.painter(), at, ink),
        Lead::Voice => icons::paint(Icon::Speaker, ui.painter(), at, t.colors.online),
        Lead::None => {}
    }
}

/// Largeur d'un onglet d'en-tête : son texte et `pad` de chaque côté.
fn tab_width(ui: &egui::Ui, t: &Tokens, label: &str, pad: f32) -> f32 {
    ui.painter().layout_no_wrap(label.to_owned(), crate::ui::fonts::medium_font(t.typography.body), Color32::PLACEHOLDER).size().x + 2.0 * pad
}

/// Marge latérale de l'onglet « Ajouter », plus large que celle des autres.
fn add_pad(t: &Tokens) -> f32 {
    t.spacing.item_x * 1.5
}

/// Onglet d'en-tête : fond neutre quand il est actif, comme dans le client.
pub(super) fn tab(ui: &mut egui::Ui, t: &Tokens, label: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::ui::fonts::medium_font(t.typography.body), Color32::PLACEHOLDER);
    let pad = egui::vec2(t.spacing.item_x, t.spacing.item_y / 2.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    if active || hot {
        ui.painter().rect_filled(rect, t.r_row().at_most(4), if active { t.colors.selected } else { t.colors.row_hover });
    }
    ui.painter().galley(rect.min + pad, galley, if active || hot { t.colors.text_strong } else { t.colors.muted });
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Nom d'une personne sur une ligne coupée à `width`, comme dans le client :
/// dans la police et le style de son nom (`cosmetics::styled_job`), dessiné en
/// texte (emojis de la police ; symboles « texte » à la couleur du nom, voir
/// `fonts::route_symbols`). `plain` : famille d'un nom sans style (la
/// demi-grasse, sinon). Rend la galée et le décor de l'effet, à peindre avec
/// `cosmetics::paint_styled`.
#[allow(clippy::too_many_arguments)]
pub(super) fn name_line(ui: &egui::Ui, text: &str, size: f32, style: Option<crate::state::cosmetics::NameStyle>, plain: Option<egui::FontFamily>, bg: Color32, color: Color32, width: f32) -> (std::sync::Arc<egui::Galley>, Vec<(egui::Vec2, Color32)>) {
    let (mut job, backdrop) = super::cosmetics::styled_job(ui.ctx(), text, size, style, bg, color);
    if let (None, Some(family)) = (style, plain) {
        for section in &mut job.sections {
            section.format.font_id.family = family.clone();
        }
    }
    super::fonts::route_symbols(ui, &mut job);
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    (ui.fonts_mut(|f| f.layout_job(job)), backdrop)
}

/// La ligne de personne en cours de dessin est survolée : ses boutons d'action
/// montrent alors leur disque (posé par `person_line`, lu par [`disc`]).
fn row_hot_id() -> egui::Id {
    egui::Id::unique("ferrocord.amis.ligne-survolee")
}

/// Bouton d'action en fin de ligne : une icône seule au repos, comme dans le
/// client ; son disque de fond n'apparaît qu'au survol de la ligne (ou du
/// bouton, ou au clavier). `hot_ink` : couleur de l'icône au survol.
pub(super) fn disc(ui: &mut egui::Ui, t: &Tokens, icon: Icon, tip: &str, hot_ink: Color32) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::Vec2::splat(DISC), Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let row_hot = ui.data(|d| d.get_temp::<bool>(row_hot_id())).unwrap_or(false);
    if hot || row_hot {
        ui.painter().circle_filled(rect.center(), DISC / 2.0, if hot { t.colors.bg0 } else { t.colors.bg1 });
    }
    let at = pixel_square(ui, rect.center() - egui::Vec2::splat(DISC_ICON / 2.0), DISC_ICON);
    icons::paint(icon, ui.painter(), at, if hot { hot_ink } else { t.colors.muted });
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, tip));
    crate::ui::widgets::tip(resp, t, tip).on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Compteur au-dessus d'une liste : « En ligne — 2 », petit et demi-gras, en
/// casse normale comme dans le client.
pub(super) fn count_label(ui: &mut egui::Ui, t: &Tokens, lang: Lang, label: &str, n: usize) {
    ui.label(RichText::new(uf::count(lang, label, n)).size(t.typography.small + 2.0).family(crate::ui::fonts::demi()).strong().color(t.colors.muted));
}

/// Présence d'un utilisateur : celle du magasin des présences (`state::people`,
/// PRESENCE_UPDATE, READY_SUPPLEMENTAL, GUILD_CREATE), hors ligne s'il ne la
/// connaît pas.
pub(super) fn presence(store: &Store, user: u64) -> Presence {
    use crate::state::people::{ActivityKind as K, Status};
    let mut people = store.people.lock();
    let Some(p) = people.presence(user) else { return Presence::default() };
    let status = match p.status {
        Status::Online => OnlineStatus::Online,
        Status::Idle => OnlineStatus::Idle,
        Status::Dnd => OnlineStatus::Dnd,
        Status::Offline => OnlineStatus::Offline,
    };
    // Même choix que `net::relations` : la première activité, le statut personnalisé en dernier.
    let kind = |k: K| match k {
        K::Playing => ActivityKind::Playing,
        K::Streaming => ActivityKind::Streaming,
        K::Listening => ActivityKind::Listening,
        K::Watching => ActivityKind::Watching,
        K::Custom => ActivityKind::Custom,
        K::Competing => ActivityKind::Competing,
    };
    let shown = |a: &&crate::state::people::Activity| if a.kind == K::Custom { a.state.is_some() } else { !a.name.is_empty() };
    let activity = p.activities.iter().filter(|a| a.kind != K::Custom).find(shown).or_else(|| p.custom().filter(shown)).map(|a| Activity {
        kind: kind(a.kind),
        name: if a.kind == K::Custom { a.state.clone().unwrap_or_default() } else { a.name.clone() },
    });
    Presence { status, activity }
}

/// Pastille de statut au coin bas droit d'un avatar, détourée du fond `bg`,
/// aux formes du thème (`Tokens::paint_status`) : disque plein en ligne,
/// croissant inactif, disque barré « ne pas déranger », anneau hors ligne.
pub(super) fn paint_status(ui: &egui::Ui, t: &Tokens, avatar: egui::Rect, s: OnlineStatus, bg: Color32) {
    use crate::state::people::Status;
    let s = match s {
        OnlineStatus::Online => Status::Online,
        OnlineStatus::Idle => Status::Idle,
        OnlineStatus::Dnd => Status::Dnd,
        OnlineStatus::Offline => Status::Offline,
    };
    t.paint_status(ui.painter(), avatar, s, bg);
}

pub(super) fn status_word(lang: Lang, s: OnlineStatus) -> &'static str {
    match s {
        OnlineStatus::Online => st::STATUS_ONLINE.get(lang),
        OnlineStatus::Idle => st::STATUS_IDLE.get(lang),
        OnlineStatus::Dnd => st::STATUS_DND.get(lang),
        OnlineStatus::Offline => st::STATUS_OFFLINE.get(lang),
    }
}

/// Activité en mots, sinon le statut.
pub(super) fn presence_line(lang: Lang, p: &Presence) -> String {
    match &p.activity {
        Some(a) => match a.kind {
            ActivityKind::Playing => f::playing(lang, &a.name),
            ActivityKind::Streaming => f::streaming(lang, &a.name),
            ActivityKind::Listening => f::listening(lang, &a.name),
            ActivityKind::Watching => f::watching(lang, &a.name),
            ActivityKind::Competing => f::competing(lang, &a.name),
            ActivityKind::Custom => a.name.clone(),
        },
        None => status_word(lang, p.status).to_string(),
    }
}

fn action_word(lang: Lang, a: RelationsAction) -> &'static str {
    match a {
        RelationsAction::Ack => st::ACTION_ACK.get(lang),
        RelationsAction::AddFriend => st::ACTION_ADD_FRIEND.get(lang),
        RelationsAction::Accept => st::ACTION_ACCEPT.get(lang),
        RelationsAction::Block => st::ACTION_BLOCK.get(lang),
        RelationsAction::Remove => st::ACTION_REMOVE.get(lang),
        RelationsAction::Unignore => st::ACTION_UNIGNORE.get(lang),
        RelationsAction::OpenDm => st::ACTION_OPEN_DM.get(lang),
        RelationsAction::CreateGroup => st::ACTION_CREATE_GROUP.get(lang),
        RelationsAction::AddMember => st::ACTION_ADD_MEMBER.get(lang),
        RelationsAction::RemoveMember => st::ACTION_REMOVE_MEMBER.get(lang),
        RelationsAction::Rename => st::ACTION_RENAME.get(lang),
        RelationsAction::EditGroup => ft::ACTION_GROUP_EDIT.get(lang),
        RelationsAction::Close => st::ACTION_CLOSE.get(lang),
        RelationsAction::Ignore => ut::ACTION_IGNORE.get(lang),
        RelationsAction::Nickname => ut::ACTION_NICK.get(lang),
        RelationsAction::AcceptRequest => ut::ACTION_ACCEPT_REQUEST.get(lang),
        RelationsAction::RejectRequest => ut::ACTION_REJECT_REQUEST.get(lang),
    }
}

/// Notification sociale en mots.
pub(super) fn notice_text(lang: Lang, n: &RelationsNotice, h24: bool) -> String {
    match n {
        RelationsNotice::RequestSent { username } => f::request_sent(lang, username),
        RelationsNotice::Captcha { .. } => st::CAPTCHA.get(lang).to_string(),
        RelationsNotice::Failed { action, problem } => {
            let reason = match problem {
                Problem::Forbidden => st::REFUSED.get(lang).to_string(),
                p => words::problem(lang, p, h24),
            };
            f::failed(lang, action_word(lang, *action), reason)
        }
    }
}

/// Nom d'utilisateur saisi, prêt à envoyer (`None` s'il ne peut pas en être un).
fn username(input: &str) -> Option<String> {
    let s = input.trim().trim_start_matches('@').to_lowercase();
    (!s.is_empty() && s.chars().count() <= USERNAME_MAX && !s.chars().any(char::is_whitespace)).then_some(s)
}

impl FriendsPage {
    /// Liste de l'onglet, refaite seulement si quelque chose a changé.
    fn ids(&mut self, store: &Store) -> Vec<u64> {
        let picking = self.picking.is_some();
        // Choix d'un groupe : tous les amis, quel que soit l'onglet.
        let tab = if picking { FriendTab::All } else { self.tab };
        // « En ligne » dépend aussi des présences (magasin `state::people`).
        let online = tab == FriendTab::Online;
        let rev = (store.relations.rev(), if online { store.people.lock().presence_rev() } else { 0 });
        let fresh = self.cache.as_ref().is_some_and(|(r, tab, q, pk, _)| *r == rev && *tab == self.tab && *q == self.query && *pk == picking);
        if !fresh {
            let list = store.friend_list(tab, &self.query);
            self.cache = Some((rev, self.tab, self.query.clone(), picking, list));
        }
        if online {
            watch_friends(store);
        }
        self.cache.as_mut().map(|c| std::mem::take(&mut c.4)).unwrap_or_default()
    }

    fn give_back(&mut self, ids: Vec<u64>) {
        if let Some(c) = self.cache.as_mut() {
            c.4 = ids;
        }
    }

}

/// Tous les amis comptent comme affichés : un ami qui se connecte ou lance une
/// activité hors de la vue réveille la page pour entrer dans la liste.
fn watch_friends(store: &Store) {
    let mut people = store.people.lock();
    for r in store.relations.friends() {
        people.watch(r.id);
    }
}

/// Ce que les onglets de la page Amis doivent savoir des relations.
#[derive(Clone, Copy)]
struct TabFacts {
    incoming: usize,
    /// Une demande reçue ou envoyée attend : l'onglet « En attente » existe.
    pending: bool,
    /// Quelqu'un est bloqué ou ignoré : l'onglet « Bloqués » existe.
    blocked: bool,
}

impl TabFacts {
    fn of(store: &Store) -> Self {
        let r = &store.relations;
        TabFacts { incoming: r.incoming(), pending: r.any(RelKind::Incoming) || r.any(RelKind::Outgoing), blocked: r.any_blocked() }
    }

    fn shows(&self, tab: FriendTab) -> bool {
        match tab {
            FriendTab::Online | FriendTab::All => true,
            FriendTab::Pending => self.pending,
            FriendTab::Blocked => self.blocked,
        }
    }
}

/// Onglet « Ajouter » : fond d'accent plein au repos, texte d'accent sur un
/// fond d'accent sombre une fois choisi, comme dans le client.
fn add_tab(ui: &mut egui::Ui, t: &Tokens, label: &str, active: bool) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(label.to_owned(), crate::ui::fonts::medium_font(t.typography.body), Color32::PLACEHOLDER);
    let pad = egui::vec2(add_pad(t), t.spacing.item_y / 2.0);
    let (rect, resp) = ui.allocate_exact_size(galley.size() + 2.0 * pad, Sense::click());
    let hot = resp.hovered() || resp.has_focus();
    let (fill, ink) = if active { (t.accent_tint(), t.colors.link) } else { (if hot { t.colors.accent.gamma_multiply(0.86) } else { t.colors.accent }, t.colors.on_accent) };
    ui.painter().rect_filled(rect, t.r_control(), fill);
    ui.painter().galley(rect.min + pad, galley, ink);
    super::keys::focus_ring(ui, t, &resp);
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label));
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Libellé d'un onglet ; « En attente » porte le nombre de demandes reçues.
fn tab_label(lang: Lang, which: FriendTab, facts: TabFacts) -> String {
    let text = match which {
        FriendTab::Online => st::TAB_ONLINE,
        FriendTab::All => st::TAB_ALL,
        FriendTab::Pending => st::TAB_PENDING,
        FriendTab::Blocked => st::TAB_BLOCKED,
    };
    if which == FriendTab::Pending && facts.incoming > 0 { f::tab_count(lang, text.get(lang), facts.incoming) } else { text.get(lang).to_string() }
}

/// Largeur des onglets de l'en-tête, « Ajouter » compris ; `blocked` : avec « Bloqués ».
fn tabs_width(ui: &egui::Ui, t: &Tokens, lang: Lang, facts: TabFacts, blocked: bool) -> f32 {
    let gap = ui.spacing().item_spacing.x;
    let plain: f32 = [FriendTab::Online, FriendTab::All, FriendTab::Pending, FriendTab::Blocked]
        .into_iter()
        .filter(|w| facts.shows(*w) && (blocked || *w != FriendTab::Blocked))
        .map(|w| tab_width(ui, t, &tab_label(lang, w, facts), t.spacing.item_x) + gap)
        .sum();
    plain + tab_width(ui, t, ft::ADD_TAB.get(lang), add_pad(t))
}

/// Onglets de la page Amis, dans l'ordre du client : En ligne, Tous, En
/// attente (s'il y a des demandes), le bouton plein « Ajouter » ; puis, à la
/// fin et seulement s'il y a quelqu'un, « Bloqués » (le client le range
/// ailleurs). `blocked` : la place suffit pour lui ; choisi, il reste montré.
fn friend_tabs(ui: &mut egui::Ui, t: &Tokens, lang: Lang, page: &mut FriendsPage, facts: TabFacts, blocked: bool) {
    let plain = |ui: &mut egui::Ui, page: &mut FriendsPage, which: FriendTab| {
        if tab(ui, t, &tab_label(lang, which, facts), page.tab == which && page.picking.is_none() && !page.adding).clicked() {
            page.tab = which;
            page.picking = None;
            page.adding = false;
        }
    };
    for which in [FriendTab::Online, FriendTab::All, FriendTab::Pending] {
        if facts.shows(which) {
            plain(ui, page, which);
        }
    }
    if add_tab(ui, t, ft::ADD_TAB.get(lang), page.adding && page.picking.is_none()).clicked() {
        page.adding = true;
        page.picking = None;
    }
    if facts.blocked && (blocked || page.tab == FriendTab::Blocked) {
        plain(ui, page, FriendTab::Blocked);
    }
}

/// Champ sans cadre de la carte « Ajouter » (le cadre est celui de la carte).
fn bare_edit<'a>(text: &'a mut String, hint: &str, t: &Tokens) -> egui::TextEdit<'a> {
    egui::TextEdit::singleline(text).frame(egui::Frame::NONE).hint_text(RichText::new(hint).color(t.colors.faint)).desired_width(f32::INFINITY)
}

impl App {
    /// Ligne d'entrée en tête des messages privés (« Amis », « Ton Nitro »,
    /// « Boutique », « Quêtes », « Demandes de message »), au gabarit du
    /// client : ligne de 40, texte de 16, icône pleine de 20 à 24 à gauche,
    /// au droit des avatars des conversations. La ligne ouverte est sur fond
    /// clair, texte blanc ; les autres en gris, le survol entre les deux.
    /// `strong` : texte blanc sans être ouverte (des demandes attendent).
    pub(super) fn dm_entry(&self, ui: &mut egui::Ui, icon: Icon, label: &str, selected: bool, strong: bool) -> egui::Response {
        self.dm_entry_counted(ui, icon, label, 0, selected, strong)
    }

    /// Comme [`Self::dm_entry`], avec un compte : « libellé (n) » s'il tient dans
    /// la colonne, sinon le libellé seul et le compte en pastille au bout de la
    /// ligne (« Demandes de messag… » perdait sa fin).
    pub(super) fn dm_entry_counted(&self, ui: &mut egui::Ui, icon: Icon, name: &str, count: usize, selected: bool, strong: bool) -> egui::Response {
        let t = &self.theme;
        let with_count = f::tab_count(self.lang, name, count);
        let label: &str = if count > 0 { &with_count } else { name };
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), t.dm_entry_row()), Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        if selected {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.selected);
        } else if hot {
            ui.painter().rect_filled(rect, t.r_row(), t.colors.row_hover);
        }
        let color = if selected || strong { t.colors.text_strong } else if hot { t.colors.text } else { t.colors.channel_idle };
        // L'icône occupe la place d'un avatar de conversation : les textes s'alignent.
        let (slot, left) = t.dm_row_parts(rect);
        // Dessin de 20 pour une icône de 24, comme dans le client.
        let side = (t.typography.icon * 5.0 / 6.0).round().min(slot.width());
        icons::paint(icon, ui.painter(), pixel_square(ui, slot.center() - egui::Vec2::splat(side / 2.0), side), color);
        let text_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2((rect.right() - t.spacing.item_x).max(left), rect.bottom()));
        let lay = |ui: &egui::Ui, text: &str, width: f32| WidgetText::from(RichText::new(text).size(t.dm_text()).family(if selected { crate::ui::fonts::demi() } else { crate::ui::fonts::medium() }).color(color)).into_galley(ui, Some(TextWrapMode::Truncate), width, TextStyle::Body);
        let mut galley = lay(ui, label, text_rect.width());
        if count > 0 && galley.elided {
            let n = ui.painter().layout_no_wrap(if count > 99 { "99+".to_owned() } else { count.to_string() }, crate::ui::fonts::demi_font(t.typography.small), t.colors.on_accent);
            let h = (t.typography.small * 4.0 / 3.0).round();
            let pill = egui::Rect::from_min_size(egui::pos2(text_rect.right() - (n.size().x + h * 0.5).max(h), rect.center().y - h / 2.0), egui::vec2((n.size().x + h * 0.5).max(h), h));
            ui.painter().rect_filled(pill, h / 2.0, t.danger_fill());
            ui.painter().galley(pill.center() - n.size() / 2.0, n, t.colors.on_accent);
            galley = lay(ui, name, (pill.left() - t.spacing.item_y - text_rect.left()).max(1.0));
        }
        let pos = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), text_rect).min;
        ui.painter().galley(pos, galley, color);
        super::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        resp
    }

    /// La page Amis est-elle ce qui est affiché (accueil des messages privés,
    /// sans conversation, demandes ni quêtes ouvertes) ?
    pub(super) fn friends_shown(&self) -> bool {
        self.store.guild.is_none() && self.store.channel.is_none() && !self.requests_shown() && !self.quests_shown()
    }

    /// Ligne « Amis » en tête des messages privés : ouvre la page.
    pub(super) fn friends_entry(&self, ui: &mut egui::Ui) -> egui::Response {
        let lang = self.lang;
        let incoming = self.store.relations.incoming();
        let label = if incoming > 0 { f::tab_count(lang, st::FRIENDS.get(lang), incoming) } else { st::FRIENDS.get(lang).to_string() };
        self.dm_entry(ui, Icon::Wave, &label, self.friends_shown(), false)
    }

    /// Ligne « Ton Nitro » : ouvre la page « Nitro et facturation » des
    /// paramètres (état de l'abonnement connu du compte, « Ouvrir dans
    /// Discord »). Aucun achat, aucune requête.
    pub(super) fn nitro_entry(&self, ui: &mut egui::Ui) -> bool {
        self.dm_entry(ui, Icon::Nitro, crate::i18n::table::nd::t::NITRO_ENTRY.get(self.lang), false, false).clicked()
    }

    /// « + » de la colonne des MP, « Ajouter des amis au MP » : la page Amis en
    /// choix de groupe, avec ces amis déjà cochés.
    pub(super) fn pick_friends(&mut self, chosen: Vec<u64>) {
        self.store.select_dms();
        self.requests.open = false;
        self.friends.picking = Some(chosen);
    }

    pub(super) fn friends_page(&mut self, ui: &mut egui::Ui) {
        let t = self.theme.clone();
        let lang = self.lang;
        let mobile = self.mobile_active();
        // En-tête sur une seule ligne, comme dans le client : icône, « Amis », un point,
        // les onglets, « Ajouter », et « nouveau groupe » à droite. Les onglets ne passent
        // sur leur propre ligne qu'en disposition mobile, ou dans une fenêtre si étroite
        // que même sans « Bloqués » ils ne tiennent pas (« Ajouter » disparaissait).
        let mut stacked = mobile;
        egui::Panel::top("friends_head").frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(super::chat_head::head_margin(&t, t.spacing.chat_head_x))).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.set_min_height(super::chat_head::head_row(&t));
                // Disposition mobile : retour à la liste (phase 7).
                self.mobile_back(ui);
                let side = super::chat_head::icon_side(&t);
                let (slot, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
                icons::paint(Icon::Wave, ui.painter(), pixel_square(ui, slot.min, side), t.colors.muted);
                ui.label(RichText::new(st::FRIENDS.get(lang)).family(crate::ui::fonts::demi()).strong().size(t.typography.title).color(t.colors.text_strong));
                let facts = TabFacts::of(&self.store);
                let page = &mut self.friends;
                // Onglet choisi qui n'existe plus (dernière demande traitée) : « Tous ».
                if !facts.shows(page.tab) {
                    page.tab = FriendTab::All;
                }
                // Place des onglets : ce qui reste après le point et l'icône « nouveau groupe ».
                let gap = ui.spacing().item_spacing.x;
                let dot = t.spacing.item_x * 2.0;
                let room = ui.available_width() - dot - side - 3.0 * gap;
                stacked = stacked || tabs_width(ui, &t, lang, facts, false) > room;
                let blocked = tabs_width(ui, &t, lang, facts, true) <= room;
                if !stacked {
                    // Un point entre le titre et les onglets, comme dans le client.
                    let (dot, _) = ui.allocate_exact_size(egui::vec2(dot, side), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 2.0, t.colors.faint);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        // « Nouveau groupe » en icône, à droite comme dans le client.
                        if super::chat_head::head_icon_drawn(ui, &t, Icon::Chat, st::NEW_GROUP.get(lang), page.picking.is_some()).clicked() {
                            page.picking = if page.picking.is_some() { None } else { Some(Vec::new()) };
                        }
                        let room = egui::vec2(ui.available_width(), ui.available_height());
                        ui.allocate_ui_with_layout(room, Layout::left_to_right(Align::Center), |ui| {
                            ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                            friend_tabs(ui, &t, lang, page, facts, blocked);
                        });
                    });
                } else if !mobile {
                    // Fenêtre très étroite : les actions restent à droite du titre, les
                    // onglets passent seuls sur la ligne du dessous.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if super::chat_head::head_icon_drawn(ui, &t, Icon::Chat, st::NEW_GROUP.get(lang), page.picking.is_some()).clicked() {
                            page.picking = if page.picking.is_some() { None } else { Some(Vec::new()) };
                        }
                    });
                }
            });
            if stacked && !mobile {
                let facts = TabFacts::of(&self.store);
                ui.horizontal_wrapped(|ui| friend_tabs(ui, &t, lang, &mut self.friends, facts, true));
            } else if stacked {
                let facts = TabFacts::of(&self.store);
                ui.horizontal_wrapped(|ui| {
                    friend_tabs(ui, &t, lang, &mut self.friends, facts, true);
                    ui.separator();
                    let page = &mut self.friends;
                    if tab(ui, &t, st::NEW_GROUP.get(lang), page.picking.is_some()).clicked() {
                        page.picking = if page.picking.is_some() { None } else { Some(Vec::new()) };
                    }
                });
            }
        });

        // « Actifs en ce moment » : à droite, seulement dans une fenêtre large.
        if !mobile && ui.ctx().content_rect().width() >= ACTIVE_MIN_WINDOW {
            let frame = egui::Frame::new().fill(t.colors.bg2).inner_margin(egui::Margin::same(PAGE_Y));
            let panel = egui::Panel::right("friends_active").exact_size(ACTIVE_WIDTH).resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| self.active_now(ui, &t));
            let r = panel.response.rect;
            ui.painter().vline(r.left(), r.y_range(), egui::Stroke::new(1.0, t.colors.separator));
        }

        let margin = if mobile { t.chat_head_margin() } else { egui::Margin { left: PAGE_X, right: PAGE_X, top: PAGE_Y, bottom: 0 } };
        egui::CentralPanel::default().frame(egui::Frame::new().fill(t.colors.bg2).inner_margin(margin)).show(ui, |ui| {
            super::look::backdrop(ui, super::look::Zone::Chat);
            // Onglet « Ajouter » : sa page remplace la liste.
            if self.friends.adding && self.friends.picking.is_none() {
                egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.add_friend_ui(ui, &t));
                return;
            }
            if self.friends.picking.is_some() {
                self.group_pick_ui(ui, &t);
                ui.add_space(t.spacing.item_y);
            }
            if self.friends.tab == FriendTab::Pending && self.friends.picking.is_none() {
                self.suggestions_ui(ui, &t);
            }
            search_field(ui, &t, &mut self.friends.query, st::SEARCH_HINT.get(lang));
            ui.add_space(f32::from(PAGE_Y));

            let mut ids = self.friends.ids(&self.store);
            let picking = self.friends.picking.is_some();
            // Catégories de contacts : filtre des onglets d'amis (pas des demandes ni des bloqués).
            let filtered = !picking && matches!(self.friends.tab, FriendTab::Online | FriendTab::All) && self.categories_bar(ui, &t);
            let shown: Option<Vec<u64>> = filtered
                .then(|| self.friends.category.as_deref().and_then(|c| self.local.prefs.friend_groups.members(c)).map(|m| ids.iter().copied().filter(|id| m.contains(id)).collect()))
                .flatten();
            let all = shown.map(|s| std::mem::replace(&mut ids, s));
            let tab_label = match self.friends.tab {
                _ if picking => st::TAB_ALL,
                FriendTab::Online => st::TAB_ONLINE,
                FriendTab::All => st::TAB_ALL,
                FriendTab::Pending => st::TAB_PENDING,
                FriendTab::Blocked => st::TAB_BLOCKED,
            };
            count_label(ui, &t, lang, tab_label.get(lang), ids.len());
            ui.add_space(t.spacing.item_y);
            if ids.is_empty() {
                let empty = if !self.friends.query.trim().is_empty() {
                    st::NO_MATCH
                } else if all.is_some() {
                    st::CAT_EMPTY
                } else {
                    match (picking, self.friends.tab) {
                        (true, _) | (false, FriendTab::All) => st::NONE_ALL,
                        (false, FriendTab::Online) => st::NONE_ONLINE,
                        (false, FriendTab::Pending) => st::NONE_PENDING,
                        (false, FriendTab::Blocked) => st::NONE_BLOCKED,
                    }
                };
                empty_state(ui, &t, Icon::Members, empty.get(lang));
            }
            let mut action = None;
            ui.spacing_mut().item_spacing.y = 0.0;
            egui::ScrollArea::vertical().auto_shrink(false).show_rows(ui, ROW, ids.len(), |ui, range| {
                for id in &ids[range] {
                    if let Some(a) = self.friend_row(ui, *id) {
                        action = Some(a);
                    }
                }
            });
            self.friends.give_back(all.unwrap_or(ids));
            if let Some(a) = action {
                self.friend_action(a);
            }
        });
    }

    pub(super) fn friend_groups_edit(&mut self, change: impl FnOnce(&mut crate::state::friend_groups::FriendGroups)) {
        let mut p = self.local.prefs.clone();
        change(&mut p.friend_groups);
        self.set_local_prefs(p);
    }

    /// Rangée des catégories de contacts : « Toutes », une pastille par catégorie,
    /// « + » ; puis, selon le cas, le champ de nom ou les actions de la catégorie
    /// choisie. Vrai si une catégorie existante est choisie (la liste est filtrée).
    fn categories_bar(&mut self, ui: &mut egui::Ui, t: &Tokens) -> bool {
        let lang = self.lang;
        let names: Vec<String> = self.local.prefs.friend_groups.groups.iter().map(|g| g.name.clone()).collect();
        // Catégorie choisie qui n'existe plus (supprimée, fichier modifié) : « Toutes ».
        if self.friends.category.as_ref().is_some_and(|c| self.local.prefs.friend_groups.members(c).is_none()) {
            self.friends.category = None;
        }
        let page = &mut self.friends;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_x, t.spacing.item_y);
            if !names.is_empty() && super::plugins::chip(ui, t, st::CAT_EVERYONE.get(lang), page.category.is_none()) {
                page.category = None;
                page.category_edit = None;
            }
            for name in &names {
                let on = page.category.as_deref() == Some(name.as_str());
                if super::plugins::chip(ui, t, name, on) {
                    page.category = (!on).then(|| name.clone());
                    page.category_edit = None;
                }
            }
            let label = if names.is_empty() { format!("+ {}", st::CAT_NEW.get(lang)) } else { "+".to_string() };
            if names.len() < crate::state::friend_groups::MAX_GROUPS && super::plugins::chip(ui, t, &label, page.category_edit.as_ref().is_some_and(|e| !e.rename)) {
                page.category_edit = match page.category_edit.take() {
                    Some(e) if !e.rename => None,
                    _ => Some(CategoryEdit { text: String::new(), rename: false, refused: false, fresh: true }),
                };
            }
        });
        // Champ de nom (création ou renommage), validé par Entrée ou le bouton.
        let mut submit = None;
        let mut delete = false;
        if let Some(edit) = page.category_edit.as_mut() {
            let mut close = false;
            ui.horizontal(|ui| {
                let field = ui.add(egui::TextEdit::singleline(&mut edit.text).hint_text(st::CAT_NAME_HINT.get(lang)).char_limit(crate::state::friend_groups::MAX_NAME).desired_width(220.0));
                if std::mem::take(&mut edit.fresh) {
                    field.request_focus();
                }
                let enter = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                let word = if edit.rename { st::CAT_RENAME } else { st::CAT_CREATE };
                if super::plugins::chip(ui, t, word.get(lang), false) || enter {
                    submit = Some((edit.text.clone(), edit.rename));
                }
                if super::plugins::chip(ui, t, st::CAT_CANCEL.get(lang), false) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    close = true;
                }
            });
            if edit.refused {
                ui.label(RichText::new(st::CAT_TAKEN.get(lang)).size(t.typography.small).color(t.colors.danger));
            }
            if close {
                page.category_edit = None;
            }
        } else if page.category.is_some() {
            ui.horizontal(|ui| {
                if super::plugins::chip(ui, t, st::CAT_RENAME.get(lang), false) {
                    page.category_edit = Some(CategoryEdit { text: page.category.clone().unwrap_or_default(), rename: true, refused: false, fresh: true });
                }
                if super::plugins::chip(ui, t, st::CAT_DELETE.get(lang), false) {
                    delete = true;
                }
            });
        }
        if let Some((text, rename)) = submit {
            let old = self.friends.category.clone();
            let mut ok = false;
            self.friend_groups_edit(|g| {
                ok = match (&old, rename) {
                    (Some(old), true) => g.rename(old, &text),
                    _ => g.create(&text),
                };
            });
            if ok {
                self.friends.category = Some(crate::state::friend_groups::clean(&text));
                self.friends.category_edit = None;
            } else if let Some(e) = self.friends.category_edit.as_mut() {
                e.refused = true;
            }
        }
        if delete && let Some(name) = self.friends.category.take() {
            self.friend_groups_edit(|g| g.delete(&name));
        }
        ui.add_space(t.spacing.item_y);
        self.friends.category.is_some()
    }

    /// Colonne « En ligne » : cartes des amis en vocal ou en activité (`friends_active.rs`).
    fn active_now(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        self.active_column(ui, t);
    }

    /// Ouvre l'onglet « Ajouter » de la page Amis (aperçu, tests) ; `name` et
    /// `note` préremplissent la carte.
    pub fn show_add_friend(&mut self, name: &str, note: &str) {
        self.store.select_dms();
        self.requests.open = false;
        self.friends.picking = None;
        self.friends.adding = true;
        self.friends.add_name = name.to_string();
        self.friends.add_note = note.chars().take(NOTE_MAX).collect();
    }

    /// Résultat d'une demande d'ami pendant que l'onglet « Ajouter » est
    /// affiché : montré sous la carte (vert ou rouge) plutôt qu'en annonce.
    /// Faux si l'annonce ne concerne pas l'onglet (elle suit son chemin).
    pub(super) fn add_friend_result(&mut self, n: &RelationsNotice) -> bool {
        let ok = match n {
            RelationsNotice::RequestSent { .. } => true,
            RelationsNotice::Captcha { action: RelationsAction::AddFriend } | RelationsNotice::Failed { action: RelationsAction::AddFriend, .. } => false,
            _ => return false,
        };
        let shown = self.friends.adding && self.store.guild.is_none() && self.store.channel.is_none() && !self.requests_shown() && !self.quests_shown();
        if shown {
            self.friends.add_result = Some((ok, notice_text(self.lang, n, self.h24())));
        }
        shown
    }

    /// Page de l'onglet « Ajouter », disposée comme celle de Discord : titre
    /// et phrase, une carte (nom d'utilisateur et bouton, filet, message
    /// facultatif et son compteur), le résultat de l'envoi, puis « Autres
    /// endroits où se faire des amis » et sa ligne vers la découverte.
    fn add_friend_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let c = &t.colors;
        let title = |ui: &mut egui::Ui, text: &str| ui.label(RichText::new(text).family(crate::ui::fonts::demi()).strong().size(t.typography.title + 2.0).color(c.text_strong));
        let head = ui
            .vertical(|ui| {
                title(ui, ft::ADD_TAB.get(lang));
                ui.add_space(t.spacing.item_y / 2.0);
                ui.add(egui::Label::new(RichText::new(ft::ADD_DETAIL.get(lang)).color(c.text)).wrap());
            })
            .response
            .rect;
        // Petite illustration à nous, à droite du titre, si la place ne manque pas.
        let badge = head.height();
        if head.width() + badge + t.spacing.chat_pad < ui.max_rect().width() {
            let at = egui::Rect::from_min_size(egui::pos2(ui.max_rect().right() - badge - t.spacing.item_x, head.top()), egui::Vec2::splat(badge));
            ui.painter().circle_filled(at.center(), badge / 2.0, t.accent_tint());
            icons::paint(Icon::FriendAdd, ui.painter(), at.shrink(badge * 0.26), c.link);
        }
        ui.add_space(t.spacing.chat_pad);

        let name = username(&self.friends.add_name);
        // Une demande part par clic ; rien ne suit l'écriture en cours côté relations.
        let busy = false;
        let mut send = false;
        let pad = f32::from(PAGE_Y);
        egui::Frame::new().stroke(t.border()).corner_radius(egui::CornerRadius::same(16)).inner_margin(egui::Margin::same(PAGE_Y)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            // Carte étroite : le bouton passe sous le champ, sur toute la largeur
            // (à côté, il ne laissait pas la place au texte d'aide).
            let stacked = ui.available_width() < STACK_BELOW;
            let height = crate::ui::widgets::button_height(t);
            let field = |ui: &mut egui::Ui, page: &mut FriendsPage| {
                let edit = ui.add(bare_edit(&mut page.add_name, ft::ADD_NAME_HINT.get(lang), t).char_limit(USERNAME_MAX + 1).min_size(egui::vec2(0.0, height)).align(egui::Align2::LEFT_CENTER));
                crate::ui::a11y::name(ui.ctx(), edit.id, ft::ADD_NAME_HINT.get(lang));
                if edit.changed() {
                    page.add_result = None;
                }
                edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && name.is_some() && !busy
            };
            if stacked {
                send |= field(ui, &mut self.friends);
                ui.add_enabled_ui(name.is_some() && !busy, |ui| send |= crate::ui::widgets::button_wide(ui, t, crate::ui::widgets::Kind::Primary, ft::ADD_SEND.get(lang), ui.available_width()).clicked());
            } else {
                ui.horizontal(|ui| {
                    // Le bouton d'abord (à droite), le champ dans la place qui reste.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add_enabled_ui(name.is_some() && !busy, |ui| send |= crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Primary, ft::ADD_SEND.get(lang)).clicked());
                        send |= field(ui, &mut self.friends);
                    });
                });
            }
            ui.add_space(pad / 2.0);
            let line = ui.available_rect_before_wrap();
            ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0, c.separator));
            ui.add_space(pad / 2.0);
            // Message facultatif joint à la demande, et ce qu'il reste de caractères.
            let note = egui::TextEdit::multiline(&mut self.friends.add_note).frame(egui::Frame::NONE).char_limit(NOTE_MAX).desired_rows(3).desired_width(f32::INFINITY).hint_text(RichText::new(ft::ADD_NOTE_HINT.get(lang)).color(c.faint));
            let note = ui.add(note);
            crate::ui::a11y::name(ui.ctx(), note.id, ft::ADD_NOTE_HINT.get(lang));
            let left = NOTE_MAX.saturating_sub(self.friends.add_note.chars().count());
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                ui.label(RichText::new(crate::i18n::number(lang, left as u64)).small().color(if left == 0 { c.danger } else { c.faint }));
            });
        });
        ui.add_space(t.spacing.item_y);
        ui.add(egui::Label::new(RichText::new(ft::ADD_NOTE_TIP.get(lang)).small().color(c.muted)).wrap());
        if let Some((ok, text)) = &self.friends.add_result {
            ui.add_space(t.spacing.item_y);
            ui.add(egui::Label::new(RichText::new(text).color(if *ok { c.online } else { c.danger })).wrap());
        }
        if send && let Some(username) = name {
            let note = Some(self.friends.add_note.trim().to_string()).filter(|n| !n.is_empty());
            self.friends.add_result = None;
            self.store.relations_write(RelationsCmd::AddFriend { username, note });
            self.friends.add_name.clear();
            self.friends.add_note.clear();
        }

        ui.add_space(t.spacing.chat_pad);
        let line = ui.available_rect_before_wrap();
        ui.painter().hline(line.x_range(), line.top(), egui::Stroke::new(1.0, c.separator));
        ui.add_space(t.spacing.chat_pad);
        title(ui, ft::ADD_OTHER.get(lang));
        ui.add_space(t.spacing.item_y / 2.0);
        ui.add(egui::Label::new(RichText::new(ft::ADD_OTHER_DETAIL.get(lang)).color(c.text)).wrap());
        ui.add_space(t.spacing.chat_pad);
        // Ligne vers la découverte : pavé vert à boussole, texte, chevron.
        let width = ui.available_width().min(DISCOVER_ROW_WIDTH);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, ROW + 6.0), Sense::click());
        let hot = resp.hovered() || resp.has_focus();
        ui.painter().rect(rect, egui::CornerRadius::same(12), if hot { c.row_hover } else { Color32::TRANSPARENT }, t.border(), egui::StrokeKind::Inside);
        let tile = egui::Rect::from_center_size(egui::pos2(rect.left() + pad + DISC / 2.0 + 4.0, rect.center().y), egui::Vec2::splat(DISC + 8.0));
        ui.painter().rect_filled(tile, egui::CornerRadius::same(10), t.success_fill());
        icons::paint(Icon::Compass, ui.painter(), tile.shrink(10.0), c.on_accent);
        let chevron = egui::Rect::from_center_size(egui::pos2(rect.right() - pad - DISC_ICON / 2.0, rect.center().y), egui::Vec2::splat(DISC_ICON));
        icons::paint(Icon::ChevronRight, ui.painter(), chevron, c.muted);
        let room = egui::Rect::from_min_max(egui::pos2(tile.right() + pad, rect.top()), egui::pos2(chevron.left() - t.spacing.item_x, rect.bottom()));
        let label = ft::ADD_DISCOVER.get(lang);
        let galley = WidgetText::from(RichText::new(label).family(crate::ui::fonts::medium()).color(c.text_strong)).into_galley(ui, Some(TextWrapMode::Wrap), room.width(), TextStyle::Body);
        let pos = Align2::LEFT_CENTER.align_size_within_rect(galley.size(), room).min;
        ui.painter().with_clip_rect(rect).galley(pos, galley, c.text_strong);
        super::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
        if resp.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
            self.show_discovery();
        }
        ui.add_space(t.spacing.chat_pad);
    }

    fn group_pick_ui(&mut self, ui: &mut egui::Ui, t: &Tokens) {
        let lang = self.lang;
        let chosen = self.friends.picking.as_ref().map_or(0, Vec::len);
        let mut create = false;
        let mut close = false;
        ui.label(RichText::new(st::NEW_GROUP.get(lang)).family(crate::ui::fonts::demi()).strong().color(t.colors.text_strong));
        ui.horizontal(|ui| {
            ui.label(RichText::new(st::NEW_GROUP_DETAIL.get(lang)).small().color(t.colors.muted));
            ui.label(RichText::new(p::selected(lang, chosen as u64)).small().color(t.colors.muted));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                close = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Link, st::CLOSE.get(lang)).clicked();
                ui.add_enabled_ui((1..MAX_GROUP).contains(&chosen), |ui| create = crate::ui::widgets::button(ui, t, crate::ui::widgets::Kind::Primary, st::CREATE_GROUP.get(lang)).clicked());
            });
        });
        if create && let Some(mut recipients) = self.friends.picking.take() {
            // Un seul ami : sans moi dans la liste, Discord ouvrirait le MP à deux.
            if recipients.len() == 1
                && let Some(me) = self.store.me.as_ref().map(|u| u.id)
            {
                recipients.push(me);
            }
            self.store.relations_write(RelationsCmd::CreateGroup { recipients });
        }
        if close {
            self.friends.picking = None;
        }
    }

    /// Ligne de personne : filet au-dessus (masqué au survol), avatar (avec sa
    /// décoration et sa pastille de statut), nom au style de la personne suivi
    /// de son tag de serveur (le nom d'utilisateur n'apparaît qu'au survol),
    /// seconde ligne précédée de son icône ; `buttons` sont posés de droite à
    /// gauche. Renvoie la zone cliquable de la ligne.
    pub(super) fn person_line(&self, ui: &mut egui::Ui, who: &Who<'_>, buttons: impl FnOnce(&mut egui::Ui)) -> egui::Response {
        let t = &self.theme;
        let (rect, _) = ui.allocate_exact_size(egui::vec2(ui.available_width(), ROW), Sense::hover());
        let resp = ui.interact(rect, ui.scope_id().with(("personne", who.id)), Sense::click());
        let hovered = resp.contains_pointer() || resp.has_focus();
        let bg = if hovered { t.colors.row_hover } else { t.colors.bg2 };
        if hovered {
            // Le fond de survol déborde dans la marge : au repos, avatars et boutons
            // restent au droit du champ de recherche et du compteur.
            let bleed = egui::vec2(t.spacing.item_x, 0.0);
            ui.painter().with_clip_rect(ui.clip_rect().expand2(bleed)).rect_filled(rect.expand2(bleed), t.r_row(), t.colors.row_hover);
        } else {
            // Filet en retrait des deux bords de la ligne, comme dans le client.
            ui.painter().hline(rect.shrink2(egui::vec2(t.spacing.item_x, 0.0)).x_range(), rect.top(), egui::Stroke::new(1.0, t.colors.separator));
        }
        // Parures de la personne : sans plaque (le client n'en montre pas ici).
        let worn = who.look.map(|id| super::cosmetics::worn(ui.ctx(), None, id)).unwrap_or_default();
        let side = t.layout.small_avatar;
        let avatar_rect = pixel_square(ui, egui::pos2(rect.left(), rect.center().y - side / 2.0), side);
        if t.avatars_shown()
            && let Some(avatar) = &who.avatar
        {
            paint_avatar(ui, &self.images, t, avatar_rect, avatar, who.name);
            super::cosmetics::row_decoration(ui, &self.images, avatar_rect, &worn);
        }
        if let Some(s) = who.status {
            paint_status(ui, t, avatar_rect, s, bg);
        }
        let left = avatar_rect.right() + t.spacing.item_x * 1.5;

        // Boutons à droite, puis le texte dans la place qui reste.
        let room = egui::Rect::from_min_max(egui::pos2(left, rect.top()), rect.max);
        ui.data_mut(|d| d.insert_temp(row_hot_id(), hovered));
        let used = ui
            .scope_builder(egui::UiBuilder::new().max_rect(room).layout(Layout::right_to_left(Align::Center)), |ui| {
                ui.spacing_mut().item_spacing.x = t.spacing.item_x * 1.25;
                buttons(ui);
            })
            .response
            .rect;
        ui.data_mut(|d| d.remove_temp::<bool>(row_hot_id()));
        let text_right = (used.left() - t.spacing.item_x).max(left);
        let text_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(text_right, rect.bottom()));
        let second_size = t.typography.body - 2.0;
        // Deux lignes peintes, centrées ensemble sur l'axe de l'avatar (un `horizontal`
        // d'egui prendrait la hauteur d'un contrôle et ferait descendre le bloc).
        let width = text_rect.width();
        let chip = worn.look.tag().map(|tag| super::cosmetics::TagChip::new(ui, t, &tag, t.colors.text));
        let chip_w = chip.as_ref().map_or(0.0, |c| c.size.x + t.spacing.item_x);
        // Nom dans la police du style de la personne ; ses couleurs et son effet ne se montrent
        // qu'au survol de la ligne (au repos : blanc, comme dans le client).
        let body = t.dm_text() - 1.0;
        let style = worn.look.name_style();
        let style = if hovered { style } else { style.map(crate::state::cosmetics::NameStyle::plain) };
        let (name, backdrop) = name_line(ui, who.name, body, style, None, bg, t.colors.text_strong, (width - chip_w).max(1.0));
        // Seconde ligne : son icône (manette, écran), puis le texte, emojis en image.
        let lead = if who.lead == Lead::None { 0.0 } else { Tokens::icon_side(second_size) };
        let lead_w = if lead > 0.0 { lead + t.spacing.item_y } else { 0.0 };
        let second_font = egui::FontId::proportional(second_size);
        let (second, second_spots) = super::emoji::truncated(ui, who.second, &second_font, t.colors.muted, (width - lead_w).max(1.0));
        let top = rect.center().y - (name.size().y + second.size().y) / 2.0;
        let mut after_name = left + name.size().x + t.spacing.item_x;
        if let Some(chip) = chip {
            let size = chip.size;
            let at = egui::pos2(after_name, (top + (name.size().y - size.y) / 2.0).round());
            chip.paint(ui, &self.images, t, egui::Rect::from_min_size(at, size), t.colors.bg3);
            after_name += size.x + t.spacing.item_x;
        }
        // Plateformes d'où l'ami est connecté, après son nom.
        if who.status.is_some() {
            let on = self.store.people.lock().presence(who.id).map(super::profile_parts::platforms).unwrap_or_default();
            let side = Tokens::icon_side(second_size);
            let w = super::profile_parts::platforms_width(t, on, side);
            if w > 0.0 && after_name + w <= text_right {
                super::profile_parts::paint_platforms(ui, t, self.lang, on, after_name, top + name.size().y / 2.0, side, t.colors.muted, ui.scope_id().with(("plateformes", who.id)));
                after_name += w + t.spacing.item_x;
            }
        }
        if hovered && !who.username.is_empty() && who.username != who.name && text_right - after_name > t.spacing.item_x * 4.0 {
            let user = WidgetText::from(RichText::new(who.username).size(second_size).color(t.colors.muted)).into_galley(ui, Some(TextWrapMode::Truncate), text_right - after_name, TextStyle::Body);
            let y = top + name.size().y - user.size().y - (name.size().y - user.size().y) / 4.0;
            ui.painter().galley(egui::pos2(after_name, y), user, t.colors.muted);
        }
        let second_at = egui::pos2(left + lead_w, top + name.size().y);
        if lead > 0.0 {
            let at = pixel_square(ui, egui::pos2(left, second_at.y + (second.size().y - lead) / 2.0), lead);
            // Manette verte quand l'ami joue, comme dans le client.
            paint_lead(ui, t, who.lead, at, if who.lead == Lead::Game { t.colors.online } else { t.colors.muted }, bg);
        }
        super::emoji::paint_spots(ui, second_at, &second, &second_spots);
        ui.painter().galley(second_at, second, t.colors.muted);
        let name_at = egui::pos2(left, top);
        super::cosmetics::paint_styled(ui.painter(), name_at, name, backdrop, t.colors.text_strong);
        super::keys::focus_ring(ui, t, &resp);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, who.name));
        resp
    }

    /// Ce que fait `id`, lu une fois sous le verrou des présences : son statut
    /// personnalisé (emoji compris), sa première activité (genre, nom, phrase
    /// « Joue à … ») et s'il partage son écran (état vocal `self_stream` d'un
    /// salon ou d'un appel que je vois).
    fn doing(&self, id: u64) -> (Option<String>, Option<(crate::state::people::ActivityKind, String, String)>, bool) {
        use crate::state::people::ActivityKind as K;
        let lang = self.lang;
        let (custom, other) = {
            let mut people = self.store.people.lock();
            people.presence(id).map_or((None, None), |p| {
                let custom = p.custom().map(super::profile::custom_line).filter(|s| !s.trim().is_empty());
                let other = p.activities.iter().find(|a| a.kind != K::Custom && !a.name.trim().is_empty()).map(|a| (a.kind, a.name.clone(), super::profile::activity_line(lang, a)));
                (custom, other)
            })
        };
        (custom, other, self.store.voice.state(id).is_some_and(|s| s.self_stream))
    }

    /// Sous-titre de la ligne d'un MP à deux, comme dans le client : l'icône
    /// de l'activité (manette, écran, casque) puis « • » et le statut
    /// personnalisé ; une activité seule : son icône et son nom ; un statut
    /// seul : le statut. Rien si la personne ne fait rien.
    pub(super) fn dm_second(&self, peer: u64) -> Option<(Lead, String)> {
        use crate::state::people::ActivityKind as K;
        let (custom, other, sharing) = self.doing(peer);
        // En vocal sur un serveur commun (état déjà reçu) : le haut-parleur vert passe
        // devant la manette, comme dans le client ; le partage d'écran d'abord.
        let lead = match (&other, sharing, self.in_guild_voice(peer)) {
            (_, true, _) => Lead::Screen,
            (_, _, true) => Lead::Voice,
            (Some((K::Playing | K::Competing, ..)), ..) => Lead::Game,
            (Some((K::Streaming | K::Watching, ..)), ..) => Lead::Screen,
            (Some((K::Listening, ..)), ..) => Lead::Sound,
            _ => Lead::None,
        };
        match (lead, other, custom) {
            (Lead::None, _, custom) => custom.map(|c| (Lead::None, c)),
            (lead, _, Some(c)) => Some((lead, format!("• {c}"))),
            (lead, Some((_, name, _)), None) => Some((lead, name)),
            (Lead::Voice, None, None) => Some((Lead::Voice, crate::i18n::table::voice::t::IN_VOICE.get(self.lang).to_string())),
            (lead, None, None) => Some((lead, st::SHARING.get(self.lang).to_string())),
        }
    }

    /// `id` est connecté à un salon vocal d'un serveur que je partage avec lui
    /// (état vocal reçu par READY, GUILD_CREATE ou VOICE_STATE_UPDATE ; rien n'est demandé).
    pub(super) fn in_guild_voice(&self, id: u64) -> bool {
        self.store.voice.state(id).is_some_and(|s| s.guild_id.is_some())
    }

    /// Seconde ligne d'un ami, comme dans le client : une manette et le nom du
    /// jeu seul ; un écran vert et « En partage d'écran » (suivi du statut
    /// personnalisé) ; une autre activité en mots ; sinon le statut
    /// personnalisé, emoji compris ; sinon le mot du statut.
    fn friend_second(&self, id: u64, status: OnlineStatus) -> (Lead, String) {
        use crate::state::people::ActivityKind as K;
        let lang = self.lang;
        if status == OnlineStatus::Offline {
            return (Lead::None, status_word(lang, status).to_string());
        }
        let (custom, other, sharing) = self.doing(id);
        if sharing {
            let sharing = st::SHARING.get(lang);
            return (Lead::Screen, custom.map_or_else(|| sharing.to_string(), |c| format!("{sharing} • {c}")));
        }
        // En vocal sur un serveur commun : « Dans un salon vocal • statut », comme dans le client.
        if self.in_guild_voice(id) {
            let voice = crate::i18n::table::voice::t::IN_VOICE.get(lang);
            return (Lead::Voice, custom.map_or_else(|| voice.to_string(), |c| format!("{voice} • {c}")));
        }
        match (other, custom) {
            (Some((K::Playing, game, _)), _) => (Lead::Game, game),
            (Some((_, _, line)), _) => (Lead::None, line),
            (None, Some(c)) => (Lead::None, c),
            (None, None) => (Lead::None, status_word(lang, status).to_string()),
        }
    }

    /// Une ligne de la liste ; renvoie l'action demandée par un clic.
    fn friend_row(&self, ui: &mut egui::Ui, id: u64) -> Option<RowAction> {
        let t = &self.theme;
        let lang = self.lang;
        let social = &self.store.relations;
        let rel = social.relation(id)?;
        let name = social.display_name(id);
        let person = social.person(id);
        let avatar = person.map_or_else(|| Avatar::default_for(id), |p| p.avatar);
        let friend = rel.kind == RelKind::Friend;
        let pres = if friend { presence(&self.store, id) } else { Presence::default() };
        let (lead, second) = match rel.kind {
            RelKind::Friend => self.friend_second(id, pres.status),
            RelKind::Incoming => (Lead::None, if rel.stranger { st::INCOMING_STRANGER } else { st::INCOMING }.get(lang).to_string()),
            RelKind::Outgoing => (Lead::None, st::OUTGOING.get(lang).to_string()),
            RelKind::Blocked => (Lead::None, st::BLOCKED.get(lang).to_string()),
            RelKind::Other => (Lead::None, st::IGNORED.get(lang).to_string()),
        };
        let who = Who { id, name, username: person.map_or("", |p| p.username.as_str()), avatar: Some(avatar), status: friend.then_some(pres.status), second: &second, lead, look: Some(id) };
        let mut action = None;
        let mut menu = false;
        let picking = self.friends.picking.is_some();
        let row = self.person_line(ui, &who, |ui| {
            if let Some(chosen) = &self.friends.picking {
                let mut on = chosen.contains(&id);
                let full = chosen.len() + 1 >= MAX_GROUP && !on;
                let resp = ui.add_enabled(!full, egui::Checkbox::without_text(&mut on));
                let resp = if full { resp.on_disabled_hover_text(st::GROUP_FULL.get(lang)) } else { resp };
                if resp.clicked() {
                    action = Some(RowAction::Toggle(id));
                }
                return;
            }
            // Disposition de droite à gauche : le premier bouton est le plus à droite.
            let c = &t.colors;
            match rel.kind {
                RelKind::Friend => {
                    menu = disc(ui, t, Icon::MoreVertical, ut::MORE.get(lang), c.text_strong).clicked();
                    if disc(ui, t, Icon::Chat, st::MESSAGE.get(lang), c.text_strong).clicked() {
                        action = Some(RowAction::Message(id));
                    }
                }
                RelKind::Incoming => {
                    if disc(ui, t, Icon::Close, st::DECLINE.get(lang), c.danger).clicked() {
                        action = Some(RowAction::Write(RelationsCmd::Remove { user_id: id }));
                    }
                    if disc(ui, t, Icon::Check, st::ACCEPT.get(lang), c.online).clicked() {
                        action = Some(RowAction::Write(RelationsCmd::Accept { user_id: id }));
                    }
                }
                RelKind::Outgoing => {
                    if disc(ui, t, Icon::Close, st::CANCEL_REQUEST.get(lang), c.danger).clicked() {
                        action = Some(RowAction::Write(RelationsCmd::Remove { user_id: id }));
                    }
                }
                RelKind::Blocked => {
                    if disc(ui, t, Icon::Close, st::UNBLOCK.get(lang), c.danger).clicked() {
                        action = Some(RowAction::Write(RelationsCmd::Remove { user_id: id }));
                    }
                }
                RelKind::Other => {}
            }
            if rel.ignored && disc(ui, t, Icon::Eye, st::UNIGNORE.get(lang), c.text_strong).clicked() {
                action = Some(RowAction::Write(RelationsCmd::Unignore { user_id: id }));
            }
        });
        if picking {
            if row.clicked() {
                action = Some(RowAction::Toggle(id));
            }
            return action;
        }
        // Clic sur la ligne : la conversation d'un ami, sinon la carte de profil. Clic droit : son menu.
        if row.clicked() {
            if friend {
                action = Some(RowAction::Message(id));
            } else {
                super::profile::open(ui.ctx(), id, None, row.rect, name, avatar);
            }
        }
        if menu || row.secondary_clicked() {
            super::profile::open_menu(ui.ctx(), id, name, row.rect);
        }
        action
    }

    fn friend_action(&mut self, a: RowAction) {
        match a {
            RowAction::Message(id) => {
                if self.store.open_dm_with(id) {
                    self.scroll_to_bottom = true;
                }
            }
            RowAction::Write(cmd) => self.store.relations_write(cmd),
            RowAction::Toggle(id) => {
                if let Some(chosen) = self.friends.picking.as_mut() {
                    match chosen.iter().position(|x| *x == id) {
                        Some(i) => {
                            chosen.remove(i);
                        }
                        None if chosen.len() + 1 < MAX_GROUP => chosen.push(id),
                        None => {}
                    }
                }
            }
        }
    }
}

/// Champ de recherche de la page, comme dans le client : pleine largeur, haut
/// (`Tokens::search_height`), coins arrondis, loupe à gauche du texte.
pub(super) fn search_field(ui: &mut egui::Ui, t: &Tokens, text: &mut String, hint: &str) {
    let side = Tokens::icon_side(t.typography.body);
    let pad = t.spacing.item_x * 1.5;
    let margin = egui::Margin { left: (pad + side + t.spacing.item_x).round() as i8, right: pad.round() as i8, top: 0, bottom: 0 };
    let edit = egui::TextEdit::singleline(text).hint_text(RichText::new(hint).color(t.colors.faint)).font(egui::FontId::proportional(t.typography.body)).margin(margin).align(Align2::LEFT_CENTER);
    let resp = ui.scope(|ui| {
        // Coins plus ronds que ceux d'un champ de formulaire, quel que soit son état.
        let w = &mut ui.visuals_mut().widgets;
        for v in [&mut w.inactive, &mut w.hovered, &mut w.active, &mut w.open, &mut w.noninteractive] {
            v.corner_radius = t.r_card();
        }
        ui.add_sized(egui::vec2(ui.available_width(), t.search_height()), edit)
    });
    let rect = resp.inner.rect;
    let at = pixel_square(ui, egui::pos2(rect.left() + pad, rect.center().y - side / 2.0), side);
    icons::paint(Icon::Search, ui.painter(), at, t.colors.faint);
}

/// Liste vide : un motif discret (l'icône dans un disque) et une phrase, centrés.
pub(super) fn empty_state(ui: &mut egui::Ui, t: &Tokens, icon: Icon, text: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space(t.spacing.chat_pad * 2.0);
        let side = ROW;
        let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(side), Sense::hover());
        ui.painter().circle_filled(rect.center(), side / 2.0, t.colors.bg1);
        icons::paint(icon, ui.painter(), rect.shrink(side * 0.28), t.colors.faint);
        ui.add_space(t.spacing.item_y);
        ui.label(RichText::new(text).color(t.colors.muted));
    });
}

#[cfg(test)]
mod tests {
    use super::super::tests::{click, frame, offline_boot};
    use super::*;
    use crate::state::relations::{Activity, Person, Relation, RelationsEvent};
    use crate::state::{Cmd, Event};
    use std::sync::{Arc, Mutex};

    /// Connexion factice qui retient les commandes envoyées.
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

    fn app(relations: Vec<Relation>) -> (super::super::App, Arc<Mutex<Vec<Cmd>>>) {
        use crate::state::model::User;
        let ctx = egui::Context::default();
        let mut store = Store::new();
        store.apply(Event::Ready { me: User::new(1, "Moi".into()), guilds: vec![], dms: vec![] });
        store.apply(Event::Relations(RelationsEvent::Ready { relations, people: vec![], dms: vec![], reads: Some(vec![]), heads: vec![] }));
        store.select_dms();
        let sent = Arc::new(Mutex::new(Vec::new()));
        let images = crate::images::Images::disabled(ctx.clone());
        let app = super::super::App::with_context(ctx, offline_boot(Box::new(Recorder(sent.clone())), store, images));
        (app, sent)
    }

    fn rel(id: u64, kind: RelKind, name: &str) -> Relation {
        let user = Person { id, name: name.into(), username: name.to_lowercase(), avatar: crate::state::model::Avatar::default_for(id) };
        Relation { id, kind, nickname: None, ignored: false, stranger: false, user: Some(user) }
    }

    fn sent(log: &Arc<Mutex<Vec<Cmd>>>) -> Vec<Cmd> {
        log.lock().map(|mut v| std::mem::take(&mut *v)).unwrap_or_default()
    }

    /// Un clic accepte ; retirer et bloquer passent par le menu « ⋮ » et demandent « Oui ».
    #[test]
    fn clicks_write_and_confirmations_guard() {
        use crate::i18n::table::people::t as ut;
        use crate::i18n::table::profile::t as pt;
        let (mut app, log) = app(vec![rel(5, RelKind::Incoming, "Carol"), rel(2, RelKind::Friend, "Alice")]);
        app.friends.tab = FriendTab::Pending;
        click(&mut app, st::ACCEPT.get(Lang::Fr));
        // L'onglet « En attente » demande aussi, une fois, les suggestions d'amis (lot ND).
        assert_eq!(sent(&log), vec![Cmd::Suggestions(crate::state::suggestions::SuggestionCmd::List), Cmd::Relations(RelationsCmd::Accept { user_id: 5 })]);

        app.friends.tab = FriendTab::All;
        click(&mut app, ut::MORE.get(Lang::Fr));
        click(&mut app, pt::MENU_REMOVE.get(Lang::Fr));
        assert!(sent(&log).is_empty(), "rien sans confirmation");
        click(&mut app, pt::NO.get(Lang::Fr));
        assert!(sent(&log).is_empty());
        click(&mut app, ut::MORE.get(Lang::Fr));
        click(&mut app, ut::IGNORE.get(Lang::Fr));
        assert_eq!(sent(&log), vec![Cmd::Relations(RelationsCmd::Ignore { user_id: 2 })]);
        click(&mut app, ut::MORE.get(Lang::Fr));
        click(&mut app, pt::MENU_BLOCK.get(Lang::Fr));
        click(&mut app, pt::YES.get(Lang::Fr));
        assert_eq!(sent(&log), vec![Cmd::Relations(RelationsCmd::Block { user_id: 2 })]);

        // Page au repos : aucune image redemandée (une fois passé le travail fini des autres fils).
        let out = super::super::tests::settle(&mut app);
        assert!(out.viewport_output.values().all(|v| v.repaint_delay == std::time::Duration::MAX), "redessin au repos");
    }

    /// Onglet « En ligne » : la présence vient du seul magasin des présences
    /// (`state::people`) ; un changement refait la liste, et un ami hors de la
    /// vue compte comme affiché (son arrivée réveille la page).
    #[test]
    fn online_tab_reads_the_presence_store() {
        use crate::state::people::{ClientStatus, Presence as P, Status};
        let (mut app, _) = app(vec![rel(2, RelKind::Friend, "Alice"), rel(3, RelKind::Friend, "Bob")]);
        app.friends.tab = FriendTab::Online;
        frame(&mut app, Vec::new());
        assert!(app.friends.ids(&app.store).is_empty());
        let online = P { status: Status::Online, client: ClientStatus::default(), activities: Vec::new() };
        {
            let mut p = app.store.people.lock();
            p.end_frame();
            assert!(p.is_watched(3), "ami hors de la liste surveillé");
            assert!(p.set_presence(3, online), "réveil");
        }
        assert_eq!(app.friends.ids(&app.store), vec![3]);
    }

    /// Onglet « Ajouter » : les onglets suivent les relations, la demande part
    /// avec son message, et son résultat s'affiche sous la carte.
    #[test]
    fn add_tab_sends_the_note_and_shows_the_result_under_the_card() {
        let (mut app, log) = app(vec![rel(2, RelKind::Friend, "Alice")]);
        let facts = TabFacts::of(&app.store);
        assert!(!facts.shows(FriendTab::Pending) && !facts.shows(FriendTab::Blocked) && facts.shows(FriendTab::All));
        app.show_add_friend(" @Carol_Fictive ", "Salut !");
        frame(&mut app, Vec::new());
        click(&mut app, ft::ADD_SEND.get(Lang::Fr));
        assert_eq!(sent(&log), vec![Cmd::Relations(RelationsCmd::AddFriend { username: "carol_fictive".into(), note: Some("Salut !".into()) })]);
        assert!(app.friends.add_name.is_empty() && app.friends.add_note.is_empty());
        app.store.apply(Event::Relations(RelationsEvent::RequestSent { username: "carol_fictive".into() }));
        frame(&mut app, Vec::new());
        assert!(matches!(&app.friends.add_result, Some((true, text)) if text.contains("carol_fictive")), "{:?}", app.friends.add_result);
        // Hors de l'onglet, l'annonce suit son chemin habituel.
        app.friends.adding = false;
        assert!(!app.add_friend_result(&RelationsNotice::RequestSent { username: "x".into() }));
        assert!(!app.add_friend_result(&RelationsNotice::Failed { action: RelationsAction::Block, problem: Problem::Forbidden }));
    }

    /// Champ de message d'une carte de profil (lot FF2, F24) : sans conversation
    /// connue, elle est demandée et le texte attend ; rien n'est envoyé d'avance.
    #[test]
    fn card_message_opens_the_conversation_and_waits_for_it() {
        let (mut app, _log) = app(vec![rel(2, RelKind::Friend, "Alice")]);
        app.store.take_commands();
        app.send_from_card(2, "Salut !".into());
        assert_eq!(app.store.take_commands(), vec![Cmd::OpenDm { user_id: 2 }]);
        assert!(app.card_message_pending());
        app.pending_card_message();
        assert!(app.card_message_pending() && app.store.take_commands().is_empty());
    }

    #[test]
    fn usernames_are_checked() {
        assert_eq!(username("  @Alice_Fictive "), Some("alice_fictive".into()));
        assert_eq!(username(""), None);
        assert_eq!(username("deux mots"), None);
        assert_eq!(username(&"x".repeat(USERNAME_MAX + 1)), None);
    }

    #[test]
    fn presence_and_notices_in_words() {
        let fr = Lang::Fr;
        let p = Presence { status: OnlineStatus::Online, activity: Some(Activity { kind: ActivityKind::Playing, name: "Jeu".into() }) };
        assert_eq!(presence_line(fr, &p), "Joue à Jeu");
        assert_eq!(presence_line(Lang::En, &Presence { status: OnlineStatus::Dnd, activity: None }), "Do Not Disturb");
        let n = RelationsNotice::Failed { action: RelationsAction::AddFriend, problem: Problem::Forbidden };
        assert_eq!(notice_text(fr, &n, true), "Demande d'ami impossible\u{a0}: Discord a refusé cette action");
        assert!(notice_text(fr, &RelationsNotice::Captcha { action: RelationsAction::AddFriend }, true).contains("application officielle"));
    }
}
