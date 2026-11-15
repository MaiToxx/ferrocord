//! Éditeur de profil : la grande fenêtre ouverte par « Modifier le profil »
//! (carte de mon compte, ma carte de profil, mon profil complet, clic droit
//! sur le panneau utilisateur, page Profils des paramètres), au gabarit de
//! celle du client de bureau de Discord.
//!
//! - À gauche (`profile_edit_left.rs`), une colonne repliable : le choix du
//!   profil (principal ou d'un serveur), puis des tuiles qui montrent l'état
//!   actuel et ouvrent le sélecteur correspondant (plaque, avatar,
//!   décoration, style du nom, thème, bannière, effet).
//! - Au centre (`profile_edit_card.rs`), le profil lui-même, modifiable sur
//!   place : nom, pronoms, tag de serveur, bio, statut personnalisé.
//! - À droite (`profile_edit_board.rs`), les widgets de jeux (« Tableau »,
//!   « Liste de souhaits »).
//!
//! Rien ne part avant « Enregistrer » (barre du bas, qui apparaît dès qu'un
//! changement n'est pas enregistré). L'enregistrement envoie une écriture à
//! la fois, dans l'ordre : profil, pseudo, profil du serveur, images, tag,
//! décoration, plaque, style du nom, widgets. Un refus arrête la suite,
//! s'affiche dans la barre et laisse le brouillon ; ce qui est déjà passé
//! n'est pas renvoyé. Un captcha arrête tout et le dit.

use super::profile::{Shown, Swap};
use super::settings::account::{rgb_bytes, rgb_code, ProfileDraft};
use super::settings::avatar::Picked;
use super::settings::Page;
use super::App;
use crate::avatar::{Avatar as Worker, Done};
use crate::i18n::table::quick::t as qt;
use crate::i18n::table::s2::f as sf2;
use crate::state::crop::{Crop, ImageKind};
use crate::state::effects::Wearable;
use crate::state::model::User;
use crate::state::people::UserInfo;
use crate::state::profiles::{Profile as FullProfile, ProfileState, ProfileText};
use crate::state::quests::Refused;
use crate::state::tags::{Slot, StyleEdit};
use crate::state::usersettings::extra::ImageTarget;
use crate::state::usersettings::{GuildProfileEdit, Profile, ProfileEdit, Refusal, Remote};
use crate::state::widgets::{GameWidget, WidgetKind};
use crate::state::{Problem, Store};
use eframe::egui::{self, Id};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

/// Longueurs acceptées par Discord (docs.discord.food, Modify Current User,
/// Modify User Profile).
pub(super) const MAX_NAME: usize = 32;
pub(super) const MAX_PRONOUNS: usize = 40;
pub(super) const MAX_BIO: usize = 190;
/// Couleurs proposées quand le thème du profil est activé.
pub(super) const DEFAULT_THEME: [u32; 2] = [0x5865f2, 0x2b2d31];
/// Durée du tremblement de la barre d'enregistrement (secondes).
const SHAKE: f64 = 0.4;

/// Image d'un profil dans le brouillon (absente : celle du profil est gardée).
pub(super) enum ImageEdit {
    Remove,
    New(Picked),
}

/// Brouillon du profil propre à un serveur.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct GuildDraft {
    pub(super) nick: String,
    pub(super) pronouns: String,
    pub(super) bio: String,
    pub(super) theme: Option<[[u8; 3]; 2]>,
}

impl GuildDraft {
    pub(super) fn of(nick: Option<&str>, p: Option<&ProfileText>) -> Self {
        GuildDraft {
            nick: nick.unwrap_or_default().to_string(),
            pronouns: p.and_then(|p| p.pronouns.clone()).unwrap_or_default(),
            bio: p.and_then(|p| p.bio.clone()).unwrap_or_default(),
            theme: p.and_then(|p| p.theme).map(|t| t.map(rgb_bytes)),
        }
    }

    /// Ce qui a changé par rapport à `base` : le pseudo (`Some(None)` :
    /// retiré) et les champs du profil du serveur.
    pub(super) fn edit(&self, base: &GuildDraft) -> (Option<Option<String>>, GuildProfileEdit) {
        let nick = self.nick.trim();
        let nick = (nick != base.nick).then(|| (!nick.is_empty()).then(|| nick.to_string()));
        let edit = GuildProfileEdit {
            bio: (self.bio != base.bio).then(|| self.bio.clone()),
            pronouns: (self.pronouns != base.pronouns).then(|| self.pronouns.clone()),
            theme: (self.theme != base.theme).then(|| self.theme.map(|t| t.map(rgb_code))),
        };
        (nick, edit)
    }

    /// La base a changé : les champs que l'utilisateur n'a pas touchés la suivent.
    fn rebase(&mut self, old: &GuildDraft, new: &GuildDraft) {
        follow(&mut self.nick, &old.nick, &new.nick);
        follow(&mut self.pronouns, &old.pronouns, &new.pronouns);
        follow(&mut self.bio, &old.bio, &new.bio);
        follow(&mut self.theme, &old.theme, &new.theme);
    }
}

fn follow<T: Clone + PartialEq>(field: &mut T, old: &T, new: &T) {
    if field == old {
        *field = new.clone();
    }
}

/// Comme [`GuildDraft::rebase`], pour le profil du compte.
fn rebase(d: &mut ProfileDraft, old: &ProfileDraft, new: &ProfileDraft) {
    follow(&mut d.global_name, &old.global_name, &new.global_name);
    follow(&mut d.pronouns, &old.pronouns, &new.pronouns);
    follow(&mut d.bio, &old.bio, &new.bio);
    follow(&mut d.accent, &old.accent, &new.accent);
    follow(&mut d.theme, &old.theme, &new.theme);
}

/// Une écriture de l'enregistrement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Step {
    Profile(ProfileEdit),
    Nick(u64, Option<String>),
    GuildProfile(u64, GuildProfileEdit),
    Image(ImageTarget),
    /// Tag de serveur affiché (`None` : aucun).
    Tag(Option<u64>),
    /// Décoration d'avatar ou plaque portée (`None` : retirée).
    Wear(Slot, Option<u64>),
    /// Style du nom d'affichage (`None` : retiré).
    Style(Option<StyleEdit>),
    Widgets(Vec<GameWidget>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    /// Image en cours de recadrage et de codage, dans le fil de `crate::avatar`.
    Encoding,
    /// Requête partie : la réponse de Discord est attendue.
    Sending,
}

/// Pourquoi l'enregistrement s'est arrêté.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Fail {
    Problem(Problem),
    /// Discord demande un captcha : on s'arrête là.
    Captcha,
}

impl From<Refused> for Fail {
    fn from(r: Refused) -> Fail {
        match r {
            Refused::Captcha => Fail::Captcha,
            Refused::Other(p) => Fail::Problem(p),
        }
    }
}

/// Profil du compte : celui dont le brouillon part, ses couleurs de thème
/// connues, et le brouillon.
pub(super) struct Main {
    pub(super) base: Profile,
    theme: Option<[u32; 2]>,
    /// Effet de profil porté, d'après mon profil complet (lot ND).
    pub(super) effect: Option<u64>,
    pub(super) draft: ProfileDraft,
}

/// Sélecteur ouvert par-dessus l'éditeur.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Picker {
    Plate,
    Avatar,
    Decoration,
    Style,
    Theme,
    Banner,
    Effect,
    /// Choix d'un jeu à ajouter au widget de ce type.
    Game(WidgetKind),
}

/// Onglet de la colonne de droite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum BoardTab {
    #[default]
    Board,
    Wishlist,
}

/// Ce que mon compte porte ou montre, tel que Ferrocord le connaît : les
/// choix du brouillon s'y comparent.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(super) struct Worn {
    pub(super) tag: Option<u64>,
    pub(super) decoration: Option<u64>,
    pub(super) plate: Option<u64>,
    pub(super) style: Option<StyleEdit>,
    /// `None` : mon profil complet n'est pas (encore) reçu.
    pub(super) widgets: Option<Vec<GameWidget>>,
    /// Un widget reçu n'est pas compris : lecture seule.
    pub(super) widgets_locked: bool,
}

/// Éditeur ouvert.
pub(super) struct Editor {
    /// Serveur dont le profil peut être réglé (celui affiché à l'ouverture,
    /// ou celui choisi dans la liste des profils).
    pub(super) here: Option<u64>,
    /// Profil affiché : celui du serveur, sinon le profil principal.
    pub(super) on_guild: bool,
    pub(super) main: Option<Main>,
    /// Profil du serveur : base et brouillon.
    pub(super) guild: Option<(GuildDraft, GuildDraft)>,
    /// Pseudo enregistré d'ici, en attendant que la passerelle le confirme.
    nick_saved: Option<String>,
    pub(super) images: HashMap<ImageTarget, ImageEdit>,
    /// Image attendue de la boîte de dialogue « Ouvrir ».
    pub(super) picking: Option<ImageTarget>,
    pub(super) worker: Option<Worker>,
    queue: VecDeque<Step>,
    running: Option<(Step, Phase)>,
    /// « Enregistrer » en cours.
    saving: bool,
    pub(super) error: Option<Fail>,
    /// Fichier choisi illisible.
    pub(super) image_error: Option<String>,
    /// Ouverte à cette image : les données sont demandées.
    pub(super) fresh: bool,
    /// Fermeture refusée (modifications non enregistrées) : instant du
    /// tremblement de la barre, qui reste en rouge.
    shake: Option<f64>,
    pub(super) warned: bool,
    /// Effet de profil choisi (lot ND) : `None` tant que le choix n'a pas été touché.
    pub(super) effect: Option<Option<u64>>,
    /// Tag de serveur choisi (`Some(None)` : aucun).
    pub(super) tag: Option<Option<u64>>,
    pub(super) decoration: Option<Option<u64>>,
    pub(super) plate: Option<Option<u64>>,
    pub(super) style: Option<Option<StyleEdit>>,
    pub(super) widgets: Option<Vec<GameWidget>>,
    pub(super) worn: Worn,
    pub(super) picker: Option<Picker>,
    /// Colonne de gauche dépliée ; `None` : selon la largeur de la fenêtre.
    pub(super) left_open: Option<bool>,
    pub(super) tab: BoardTab,
    /// Recherche du choix d'un jeu.
    pub(super) game_query: String,
}

/// Widgets tels qu'ils partent : un widget de jeux vidé n'est pas gardé.
fn kept_widgets(list: &[GameWidget]) -> Vec<GameWidget> {
    list.iter().filter(|w| w.kind == WidgetKind::Application || !w.games.is_empty()).cloned().collect()
}

impl Editor {
    fn new(here: Option<u64>, on_guild: bool) -> Self {
        Editor {
            here,
            on_guild: on_guild && here.is_some(),
            main: None,
            guild: None,
            nick_saved: None,
            images: HashMap::new(),
            picking: None,
            worker: None,
            queue: VecDeque::new(),
            running: None,
            saving: false,
            error: None,
            image_error: None,
            fresh: true,
            shake: None,
            warned: false,
            effect: None,
            tag: None,
            decoration: None,
            plate: None,
            style: None,
            widgets: None,
            worn: Worn::default(),
            picker: None,
            left_open: None,
            tab: BoardTab::default(),
            game_query: String::new(),
        }
    }

    /// Serveur du profil affiché.
    pub(super) fn tab_guild(&self) -> Option<u64> {
        self.here.filter(|_| self.on_guild)
    }

    pub(super) fn target(&self, kind: ImageKind) -> ImageTarget {
        ImageTarget { kind, guild: self.tab_guild() }
    }

    /// Le compte a-t-il Nitro (ce que Discord exige pour plusieurs réglages) ?
    pub(super) fn nitro(&self) -> bool {
        self.main.as_ref().is_some_and(|m| m.base.premium == 2)
    }

    /// Le profil du serveur porte-t-il des changements non enregistrés ?
    pub(super) fn guild_dirty(&self) -> bool {
        self.steps().iter().any(|s| match s {
            Step::Nick(..) | Step::GuildProfile(..) => true,
            Step::Image(t) => t.guild.is_some(),
            _ => false,
        })
    }

    /// Écritures que « Enregistrer » enverrait, dans l'ordre ; vide : rien n'a changé.
    pub(super) fn steps(&self) -> Vec<Step> {
        let mut out = Vec::new();
        if let Some(m) = &self.main {
            let mut e = m.draft.edit(&m.base, m.theme);
            e.effect = self.effect.filter(|c| *c != m.effect);
            if !e.is_empty() {
                out.push(Step::Profile(e));
            }
        }
        if let (Some(g), Some((base, draft))) = (self.here, &self.guild) {
            let (nick, e) = draft.edit(base);
            if let Some(n) = nick {
                out.push(Step::Nick(g, n));
            }
            if !e.is_empty() {
                out.push(Step::GuildProfile(g, e));
            }
        }
        let guilds: Vec<Option<u64>> = std::iter::once(None).chain(self.here.map(Some)).collect();
        for guild in guilds {
            for kind in [ImageKind::Avatar, ImageKind::Banner] {
                let target = ImageTarget { kind, guild };
                if self.images.contains_key(&target) {
                    out.push(Step::Image(target));
                }
            }
        }
        if let Some(tag) = self.tag.filter(|t| *t != self.worn.tag) {
            out.push(Step::Tag(tag));
        }
        if let Some(sku) = self.decoration.filter(|s| *s != self.worn.decoration) {
            out.push(Step::Wear(Slot::Decoration, sku));
        }
        if let Some(sku) = self.plate.filter(|s| *s != self.worn.plate) {
            out.push(Step::Wear(Slot::Nameplate, sku));
        }
        if let Some(style) = self.style.as_ref().filter(|s| **s != self.worn.style) {
            out.push(Step::Style(style.clone()));
        }
        if let (Some(draft), Some(base)) = (&self.widgets, &self.worn.widgets) {
            let kept = kept_widgets(draft);
            if !self.worn.widgets_locked && kept != kept_widgets(base) {
                out.push(Step::Widgets(kept));
            }
        }
        out
    }

    pub(super) fn busy(&self) -> bool {
        self.saving || self.running.is_some() || !self.queue.is_empty()
    }

    /// « Réinitialiser » : le brouillon repart de ce qui est enregistré.
    pub(super) fn reset(&mut self) {
        if let Some(m) = &mut self.main {
            m.draft = ProfileDraft::of(&m.base, m.theme);
        }
        if let Some((base, draft)) = &mut self.guild {
            *draft = base.clone();
        }
        self.images.clear();
        self.picking = None;
        (self.effect, self.tag, self.decoration, self.plate, self.style, self.widgets) = (None, None, None, None, None, None);
        (self.error, self.image_error, self.warned, self.shake) = (None, None, false, None);
    }

    pub(super) fn decoration_now(&self) -> Option<u64> {
        self.decoration.unwrap_or(self.worn.decoration)
    }

    pub(super) fn plate_now(&self) -> Option<u64> {
        self.plate.unwrap_or(self.worn.plate)
    }

    pub(super) fn style_now(&self) -> Option<StyleEdit> {
        self.style.clone().unwrap_or_else(|| self.worn.style.clone())
    }

    /// Widgets montrés (brouillon, sinon ceux du profil).
    pub(super) fn widgets_now(&self) -> &[GameWidget] {
        self.widgets.as_deref().or(self.worn.widgets.as_deref()).unwrap_or_default()
    }

    /// Brouillon des widgets, créé au premier changement ; `None` tant que
    /// ceux du profil ne sont pas connus, ou s'ils ne peuvent pas être réécrits.
    pub(super) fn widgets_mut(&mut self) -> Option<&mut Vec<GameWidget>> {
        if self.worn.widgets_locked {
            return None;
        }
        let base = self.worn.widgets.as_ref()?;
        Some(self.widgets.get_or_insert_with(|| base.clone()))
    }

    /// Ce que la carte montre : le profil connu, avec le brouillon par-dessus.
    pub(super) fn apply_to(&self, s: &mut Shown, me: &User, store: &Store) {
        let user = s.user.get_or_insert_with(|| UserInfo {
            id: me.id,
            username: me.name.clone(),
            global_name: None,
            discriminator: None,
            avatar: me.avatar,
            banner: None,
            accent: None,
            flags: None,
            bot: false,
            system: false,
            look: Default::default(),
        });
        let text = |s: &str| (!s.trim().is_empty()).then(|| s.to_string());
        let profile = s.profile.get_or_insert_with(FullProfile::default);
        if let Some(m) = &self.main {
            let d = &m.draft;
            user.global_name = text(d.global_name.trim());
            user.accent = Some(d.accent.map(rgb_code));
            profile.user.bio = text(&d.bio);
            profile.user.pronouns = text(&d.pronouns);
            profile.user.accent = d.accent.map(rgb_code);
            profile.user.theme = d.theme.map(|t| t.map(rgb_code));
            profile.user.effect = self.effect.unwrap_or(m.effect);
        }
        // Parures choisies : celles du brouillon passent devant celles portées.
        if let Some(tag) = self.tag {
            user.look.tag = Some(tag.and_then(|g| store.guilds.iter().find(|x| x.id == g)).and_then(|g| g.tag));
        }
        let look = |sku: Option<u64>| sku.and_then(|s| store.effects.looks.iter().find(|o| o.sku_id == s)).map(|o| o.look);
        if let Some(sku) = self.decoration {
            user.look.decoration = Some(match look(sku) {
                Some(Wearable::Decoration(a)) => Some(a),
                _ => None,
            });
        }
        if let Some(sku) = self.plate {
            user.look.nameplate = Some(match look(sku) {
                Some(Wearable::Nameplate(n)) => Some(n),
                _ => None,
            });
        }
        if let Some(style) = &self.style {
            user.look.name_style = Some(style.as_ref().and_then(StyleEdit::shown));
        }
        // Image retirée : celle du profil n'est plus montrée (une nouvelle
        // image passe par l'en-tête, voir `swap`).
        if matches!(self.images.get(&ImageTarget { kind: ImageKind::Avatar, guild: None }), Some(ImageEdit::Remove)) {
            user.avatar.image = None;
        }
        if matches!(self.images.get(&ImageTarget { kind: ImageKind::Banner, guild: None }), Some(ImageEdit::Remove)) {
            user.banner = Some(None);
            profile.user.banner = None;
        }
        let (Some(g), Some((_, d))) = (self.tab_guild(), &self.guild) else { return };
        let member = s.member.get_or_insert_with(Default::default);
        member.nick = text(d.nick.trim());
        let own = profile.guild.get_or_insert_with(ProfileText::default);
        own.bio = text(&d.bio);
        own.pronouns = text(&d.pronouns);
        own.theme = d.theme.map(|t| t.map(rgb_code));
        if matches!(self.images.get(&ImageTarget { kind: ImageKind::Avatar, guild: Some(g) }), Some(ImageEdit::Remove)) {
            member.avatar = None;
        }
        if matches!(self.images.get(&ImageTarget { kind: ImageKind::Banner, guild: Some(g) }), Some(ImageEdit::Remove)) {
            member.banner = None;
            own.banner = None;
        }
    }

    /// Nouvelle image du profil affiché, telle que l'en-tête de la carte
    /// la montrera : la zone gardée par le recadrage, aux proportions `aspect`.
    pub(super) fn swap(&self, kind: ImageKind, aspect: f32) -> Option<Swap> {
        let Some(ImageEdit::New(p)) = self.images.get(&self.target(kind)) else { return None };
        let (iw, ih) = (p.image.width().max(1) as f32, p.image.height().max(1) as f32);
        let r = p.crop.rect(p.image.width(), p.image.height(), kind);
        let (mut w, mut h) = (r.w as f32, r.h as f32);
        let (cx, cy) = (r.x as f32 + w / 2.0, r.y as f32 + h / 2.0);
        // La carte montre la bannière plus allongée que le cadre envoyé : le
        // milieu du cadre, jamais déformé.
        if w / h < aspect {
            h = w / aspect;
        } else {
            w = h * aspect;
        }
        let uv = egui::Rect::from_min_max(egui::pos2((cx - w / 2.0) / iw, (cy - h / 2.0) / ih), egui::pos2((cx + w / 2.0) / iw, (cy + h / 2.0) / ih));
        Some(Swap { tex: p.texture.id(), uv })
    }

    /// Décalage de la barre d'enregistrement pendant son tremblement.
    fn shake_offset(&self, now: f64) -> Option<f32> {
        let k = ((now - self.shake?) / SHAKE) as f32;
        (0.0..1.0).contains(&k).then(|| (k * std::f32::consts::TAU * 3.0).sin() * 10.0 * (1.0 - k))
    }
}

/// Texte d'un champ après une écriture acceptée (vide : absent).
fn kept(s: &str) -> Option<String> {
    (!s.is_empty()).then(|| s.to_string())
}

/// Ce que la fenêtre demande à l'application, une fois dessinée.
#[derive(Default)]
pub(super) struct Asked {
    pub(super) close: bool,
    pub(super) save: bool,
    pub(super) reset: bool,
    /// Clic sur la bulle : la fenêtre du statut personnalisé.
    pub(super) status: bool,
    /// « Voir dans Discord » : adresse à ouvrir dans le navigateur.
    pub(super) link: Option<&'static str>,
    /// « Plus d'options » : la page Profils des paramètres.
    pub(super) settings: bool,
    /// Profil d'un autre serveur choisi dans la liste.
    pub(super) guild: Option<Option<u64>>,
    /// Image à choisir (boîte de dialogue « Ouvrir »).
    pub(super) choose: Option<ImageKind>,
}

impl App {
    /// Ouvre l'éditeur de profil ; `guild` : sur le profil du serveur
    /// affiché. En disposition mobile, le chemin reste la page Profils des
    /// paramètres.
    pub fn open_profile_editor(&mut self, guild: bool) {
        if self.mobile_active() {
            self.show_settings(Page::Profile);
            return;
        }
        self.quick.editor = Some(Editor::new(self.store.guild, guild));
    }

    pub fn profile_editor_open(&self) -> bool {
        self.quick.editor.is_some()
    }

    /// Image déjà en mémoire mise dans le brouillon de l'éditeur (aperçu, tests).
    pub fn stage_profile_image(&mut self, banner: bool, bytes: Vec<u8>) {
        let kind = if banner { ImageKind::Banner } else { ImageKind::Avatar };
        let ctx = self.ctx.clone();
        let Some(e) = &mut self.quick.editor else { return };
        e.picking = Some(e.target(kind));
        e.worker.get_or_insert_with(|| Worker::new(Arc::new(move || ctx.request_repaint()))).add_bytes(kind, "apercu.png".into(), bytes);
    }

    /// Ouvre le sélecteur « Style du pseudonyme » de l'éditeur sur un
    /// brouillon de style (dégradé, police `font_id`) : aperçu.
    pub fn profile_editor_style(&mut self, font_id: u16) {
        if let Some(e) = &mut self.quick.editor {
            e.picker = Some(Picker::Style);
            e.style = Some(Some(StyleEdit { font_id, effect_id: 2, colors: vec![0xff7a59, 0x8a5cf0] }));
        }
    }

    /// Replie ou déplie la colonne de gauche de l'éditeur (aperçu).
    pub fn profile_editor_sidebar(&mut self, open: bool) {
        if let Some(e) = &mut self.quick.editor {
            e.left_open = Some(open);
        }
    }

    /// Bases du brouillon : mon profil (`GET /users/@me`, demandé une fois
    /// s'il manque) et mon profil complet vu du serveur choisi (couleurs du
    /// thème, profil du serveur, widgets). Une base qui change entraîne les
    /// champs que l'utilisateur n'a pas touchés.
    fn editor_sync(&mut self, e: &mut Editor, me: &User) {
        // Serveur quitté pendant l'édition : son profil disparaît.
        if e.here.is_some_and(|g| !self.store.guilds.iter().any(|x| x.id == g)) {
            e.here = None;
            e.on_guild = false;
            e.guild = None;
        }
        if e.fresh {
            if matches!(self.store.account.profile, Remote::Idle | Remote::Failed(_)) {
                self.store.account.load_profile();
            }
            self.store.want_profile(me.id, e.here);
            // Effets, décorations et plaques possédés (lot ND).
            self.store.want_owned_effects();
            self.store.retry_games();
        }
        let exact = self.store.people.lock().profiles().profile(me.id, e.here).and_then(ProfileState::profile).cloned();
        // Ce qui tient au compte (thème, effet, widgets) se lit aussi dans mon profil hors serveur.
        let full = exact.clone().or_else(|| self.store.people.lock().profiles().profile(me.id, None).and_then(ProfileState::profile).cloned());
        let theme = full.as_ref().and_then(|p| p.user.theme);
        let effect = full.as_ref().and_then(|p| p.user.effect);
        if let Remote::Ready(p) = &self.store.account.profile {
            match &mut e.main {
                Some(m) if m.base != *p || m.theme != theme => {
                    let (old, new) = (ProfileDraft::of(&m.base, m.theme), ProfileDraft::of(p, theme));
                    rebase(&mut m.draft, &old, &new);
                    m.base = p.clone();
                    m.theme = theme;
                }
                Some(_) => {}
                None => e.main = Some(Main { base: p.clone(), theme, effect, draft: ProfileDraft::of(p, theme) }),
            }
        }
        if let Some(m) = &mut e.main {
            m.effect = effect;
        }
        e.worn = Worn {
            tag: self.store.own_tag().map(|t| t.guild_id),
            decoration: self.store.worn(Slot::Decoration),
            plate: self.store.worn(Slot::Nameplate),
            style: self.store.own_name_style().as_ref().map(StyleEdit::of),
            widgets: full.as_ref().map(|p| p.widgets.clone()),
            widgets_locked: full.as_ref().is_some_and(|p| p.widgets_opaque),
        };
        // Noms et images des jeux montrés ; suggestions quand le choix d'un jeu est ouvert.
        let mut games: Vec<u64> = e.widgets_now().iter().flat_map(|w| w.games.iter().map(|g| g.game_id)).collect();
        if matches!(e.picker, Some(Picker::Game(_))) {
            self.store.want_suggested_games();
            if let Remote::Ready(s) = &self.store.widgets.suggested {
                games.extend(s.games.iter().chain(&s.wishlist));
            }
        }
        self.store.want_games(&games);
        let Some(g) = e.here else { return };
        let Some(full) = exact else { return };
        let nick = e.nick_saved.clone().or_else(|| self.store.guilds.iter().find(|x| x.id == g).and_then(|x| x.me.as_ref()).and_then(|m| m.nick.clone()));
        let new = GuildDraft::of(nick.as_deref(), full.guild.as_ref());
        match &mut e.guild {
            Some((base, draft)) if *base != new => {
                draft.rebase(base, &new);
                *base = new;
            }
            Some(_) => {}
            None => e.guild = Some((new.clone(), new)),
        }
    }

    /// Résultats du fil des images : image choisie, illisible, ou prête à partir.
    fn editor_poll(&mut self, e: &mut Editor) {
        let Some(w) = &e.worker else { return };
        for done in w.poll() {
            match done {
                Done::Picked { kind, image, preview } => {
                    let Some(target) = e.picking.take_if(|t| t.kind == kind) else { continue };
                    let texture = self.ctx.load_texture(format!("ferrocord.profil.edition.{kind:?}.{:?}", target.guild), preview, egui::TextureOptions::LINEAR);
                    e.images.insert(target, ImageEdit::New(Picked { image, texture, crop: Crop::default() }));
                    e.image_error = None;
                    e.warned = false;
                }
                Done::Unreadable { name, .. } => {
                    e.picking = None;
                    e.image_error = Some(sf2::image_unreadable(self.lang, name));
                }
                Done::Encoded { kind, guild, data } => {
                    let target = ImageTarget { kind, guild };
                    if e.running == Some((Step::Image(target), Phase::Encoding)) {
                        match self.store.account.extra.set_image(target, Some(data)) {
                            Ok(()) => e.running = Some((Step::Image(target), Phase::Sending)),
                            Err(r) => self.editor_refused(e, r),
                        }
                    }
                }
            }
        }
    }

    fn editor_refused(&mut self, e: &mut Editor, r: Refusal) {
        e.running = None;
        e.queue.clear();
        e.saving = false;
        self.refused(r);
    }

    /// Lance une écriture ; `None` : rien n'avait à partir (elle est finie).
    fn editor_start(&mut self, e: &mut Editor, step: &Step) -> Result<Option<Phase>, Refusal> {
        // Choix déjà porté : rien à envoyer ; autre écriture en cours ailleurs : occupé.
        let sent = |sent: bool, busy: bool| if sent { Ok(Some(Phase::Sending)) } else if busy { Err(Refusal::Busy) } else { Ok(None) };
        match step {
            Step::Profile(edit) => self.store.account.save_profile(edit.clone())?,
            Step::Nick(g, nick) => self.store.account.set_nick(*g, nick.clone())?,
            Step::GuildProfile(g, edit) => self.store.account.save_guild_profile(*g, edit.clone())?,
            Step::Image(target) => match e.images.get(target) {
                Some(ImageEdit::New(p)) => {
                    let ctx = self.ctx.clone();
                    let w = e.worker.get_or_insert_with(|| Worker::new(Arc::new(move || ctx.request_repaint())));
                    w.encode(p.image.clone(), p.crop, target.kind, target.guild);
                    return Ok(Some(Phase::Encoding));
                }
                Some(ImageEdit::Remove) => self.store.account.extra.set_image(*target, None)?,
                None => return Ok(None),
            },
            Step::Tag(guild) => {
                let busy = self.store.tags.busy();
                return sent(self.store.set_guild_tag(*guild), busy);
            }
            Step::Wear(slot, sku) => {
                let busy = self.store.tags.busy();
                return sent(self.store.wear(*slot, *sku), busy);
            }
            Step::Style(style) => {
                let busy = self.store.tags.busy();
                return sent(self.store.set_name_style(style.clone()), busy);
            }
            Step::Widgets(list) => {
                self.store.widgets.take_refusal();
                return sent(self.store.save_widgets(list.clone()), true);
            }
        }
        Ok(Some(Phase::Sending))
    }

    /// Écriture acceptée : ce que Ferrocord garde de mon profil suit, sans rien redemander.
    fn editor_done(&mut self, e: &mut Editor, step: Step) {
        match step {
            Step::Profile(edit) => {
                self.store.own_profile_saved(e.here, None, |p| {
                    if let Some(b) = &edit.bio {
                        p.bio = kept(b);
                    }
                    if let Some(v) = &edit.pronouns {
                        p.pronouns = kept(v);
                    }
                    if let Some(a) = edit.accent {
                        p.accent = a;
                    }
                    if let Some(t) = edit.theme {
                        p.theme = t;
                    }
                    if let Some(x) = edit.effect {
                        p.effect = x;
                    }
                });
                e.effect = None;
            }
            Step::Nick(_, nick) => e.nick_saved = Some(nick.unwrap_or_default()),
            Step::GuildProfile(g, edit) => self.store.own_profile_saved(e.here, Some(g), |p| {
                if let Some(b) = &edit.bio {
                    p.bio = kept(b);
                }
                if let Some(v) = &edit.pronouns {
                    p.pronouns = kept(v);
                }
                if let Some(t) = edit.theme {
                    p.theme = t;
                }
            }),
            Step::Image(target) => {
                e.images.remove(&target);
            }
            Step::Tag(_) => e.tag = None,
            Step::Wear(Slot::Decoration, _) => e.decoration = None,
            Step::Wear(Slot::Nameplate, _) => e.plate = None,
            Step::Style(_) => e.style = None,
            Step::Widgets(_) => e.widgets = None,
        }
    }

    /// Fait avancer l'enregistrement : une écriture à la fois. Vrai quand
    /// tout est passé.
    fn editor_run(&mut self, e: &mut Editor) -> bool {
        loop {
            match e.running.take() {
                Some((step, phase)) => {
                    let acc = &self.store.account;
                    let waiting = match (&step, phase) {
                        (_, Phase::Encoding) => true,
                        (Step::Profile(_), _) => acc.profile_busy(),
                        (Step::Nick(..), _) => acc.nick_saving.is_some(),
                        (Step::GuildProfile(..), _) => acc.guild_profile_busy(),
                        (Step::Image(_), _) => acc.extra.image_saving.is_some(),
                        (Step::Tag(_) | Step::Wear(..) | Step::Style(_), _) => self.store.tags.busy(),
                        (Step::Widgets(_), _) => self.store.widgets.saving(),
                    };
                    if waiting {
                        e.running = Some((step, phase));
                        return false;
                    }
                    let failed: Option<Fail> = match &step {
                        Step::Tag(_) | Step::Wear(..) | Step::Style(_) => self.store.tags.refused().cloned().map(Fail::from),
                        Step::Widgets(_) => self.store.widgets.take_refusal().map(Fail::from),
                        _ => self.store.account.take_profile_failure().map(Fail::Problem),
                    };
                    if let Some(f) = failed {
                        e.error = Some(f);
                        e.queue.clear();
                        e.saving = false;
                        return false;
                    }
                    self.editor_done(e, step);
                }
                None => {
                    let Some(step) = e.queue.pop_front() else {
                        return std::mem::take(&mut e.saving);
                    };
                    // Refus d'une écriture d'avant (page Profils) : pas le nôtre.
                    self.store.account.take_profile_failure();
                    match self.editor_start(e, &step) {
                        Ok(Some(phase)) => e.running = Some((step, phase)),
                        Ok(None) => self.editor_done(e, step),
                        Err(r) => {
                            self.editor_refused(e, r);
                            return false;
                        }
                    }
                }
            }
        }
    }

    /// « Enregistrer » : les écritures du brouillon, dans l'ordre.
    fn editor_save(&mut self, e: &mut Editor) {
        if e.busy() {
            return;
        }
        e.queue = e.steps().into();
        e.saving = !e.queue.is_empty();
        e.error = None;
        e.warned = false;
        e.shake = None;
    }

    /// Fenêtre de l'éditeur, s'il y en a une, ses sélecteurs et la fenêtre
    /// du statut personnalisé par-dessus. Échap, la croix ou un clic à côté
    /// la ferment, sauf s'il reste des modifications non enregistrées : la
    /// barre d'enregistrement tremble alors.
    pub(super) fn profile_editor_ui(&mut self, ui: &mut egui::Ui) {
        let Some(mut e) = self.quick.editor.take() else { return };
        let Some(me) = self.store.me.clone() else { return };
        let ctx = ui.ctx().clone();
        self.editor_sync(&mut e, &me);
        self.editor_poll(&mut e);
        if self.editor_run(&mut e) {
            self.notify(qt::SAVED);
        }
        let mut shown = self.shown(me.id, e.tab_guild());
        // Profil principal ouvert depuis un serveur : mon profil complet n'est connu que vu de ce
        // serveur ; ce qui tient au compte (badges, connexions) s'y lit, sans la part du serveur.
        if shown.profile.is_none()
            && let Some(mut p) = self.store.people.lock().profiles().profile(me.id, e.here).and_then(ProfileState::profile).cloned()
        {
            p.guild = None;
            shown.profile = Some(p);
        }
        e.apply_to(&mut shown, &me, &self.store);
        self.want_profile_effect(&shown);
        let mut asked = Asked::default();
        let now = ctx.input(|i| i.time);
        let shift = e.shake_offset(now);
        if shift.is_some() {
            ctx.request_repaint();
        }
        self.profile_window(&ctx, &mut e, &me, &shown, shift.unwrap_or(0.0), &mut asked);
        self.editor_pickers(&ctx, &mut e, &me, &shown, &mut asked);
        e.fresh = false;
        if asked.save {
            self.editor_save(&mut e);
            // La première écriture part à cette image.
            if self.editor_run(&mut e) {
                self.notify(qt::SAVED);
            }
        }
        if asked.reset && !e.busy() {
            e.reset();
        }
        if let Some(kind) = asked.choose {
            e.picking = Some(e.target(kind));
            let repaint = self.ctx.clone();
            let title = crate::i18n::table::s2::t::PICK_TITLE.get(self.lang);
            e.worker.get_or_insert_with(|| Worker::new(Arc::new(move || repaint.request_repaint()))).pick(kind, title);
        }
        if let Some(guild) = asked.guild {
            self.editor_switch(&mut e, &me, guild, now);
        }
        if let Some(url) = asked.link {
            self.open_link(url);
        }
        if asked.status {
            self.open_custom_status();
        }
        if asked.settings {
            self.show_settings(Page::Profile);
            return;
        }
        if asked.close {
            if e.steps().is_empty() && !e.busy() {
                self.close_custom_status();
                return;
            }
            // Un brouillon ne se jette pas d'un clic : « Réinitialiser » ou « Enregistrer » d'abord.
            e.warned = true;
            e.shake = Some(now);
            ctx.request_repaint();
        }
        self.quick.editor = Some(e);
        self.custom_status_over_editor(&ctx);
    }

    /// Profil choisi dans la liste : le profil principal, ou celui d'un
    /// serveur. Changer de serveur avec un profil de serveur modifié ferait
    /// perdre son brouillon : la barre tremble à la place.
    fn editor_switch(&mut self, e: &mut Editor, me: &User, guild: Option<u64>, now: f64) {
        let Some(g) = guild else {
            e.on_guild = false;
            return;
        };
        if e.here != Some(g) {
            if e.guild_dirty() || e.busy() {
                e.warned = true;
                e.shake = Some(now);
                return;
            }
            e.here = Some(g);
            e.guild = None;
            e.nick_saved = None;
            self.store.want_profile(me.id, Some(g));
        }
        e.on_guild = true;
        e.picker = None;
    }

    /// La fenêtre : presque tout l'écran, trois colonnes, la croix en haut à
    /// droite, la barre d'enregistrement en bas.
    fn profile_window(&self, ctx: &egui::Context, e: &mut Editor, me: &User, shown: &Shown, shift: f32, asked: &mut Asked) {
        let t = self.theme.clone();
        let lang = self.lang;
        let screen = ctx.content_rect();
        // Petite fenêtre : l'éditeur la prend toute.
        let air = if screen.width() < 720.0 || screen.height() < 520.0 { 0.0 } else { (t.spacing.card_pad * 1.5).round() };
        let size = (screen.size() - egui::Vec2::splat(2.0 * air)).max(egui::vec2(280.0, 240.0));
        let frame = egui::Frame::new().fill(t.colors.bg1).stroke(t.border()).corner_radius(t.r_card()).shadow(t.shadow(crate::theme::Elevation::Window));
        let dirty = !e.steps().is_empty();
        let shown_modal = egui::Modal::new(Id::unique("ferrocord.profil.edition")).frame(frame).show(ctx, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            let (win, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            let lay = Layout::of(&t, win, e, dirty || e.busy());
            let child = |ui: &mut egui::Ui, rect: egui::Rect, add: &mut dyn FnMut(&mut egui::Ui)| {
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(rect).layout(egui::Layout::top_down(egui::Align::Min)));
                ui.set_clip_rect(rect.intersect(ui.clip_rect()));
                ui.spacing_mut().item_spacing = egui::vec2(t.spacing.item_x, t.spacing.item_y);
                add(&mut ui);
            };
            // Colonne de gauche, sur un fond plus sombre.
            let r = t.r_card();
            ui.painter().rect_filled(lay.left, egui::CornerRadius { ne: 0, se: 0, ..r }, t.colors.bg0);
            child(ui, lay.left, &mut |ui| self.editor_left(ui, &t, e, me, shown, lay.left_open, asked));
            if let Some(center) = lay.center {
                child(ui, center, &mut |ui| {
                    egui::ScrollArea::vertical().id_salt("profil-edition-centre").auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_space(lay.top);
                        ui.horizontal_top(|ui| {
                            ui.add_space(lay.card_x - center.left());
                            ui.vertical(|ui| {
                                ui.set_width(lay.card_w);
                                ui.add_enabled_ui(!e.busy(), |ui| self.editor_card(ui, &t, e, me, shown, lay.card_w, asked));
                                // Fenêtre étroite : les widgets passent sous le profil.
                                if lay.board_below {
                                    ui.add_space(t.spacing.category_gap);
                                    ui.add_enabled_ui(!e.busy(), |ui| self.editor_board(ui, &t, e));
                                }
                            });
                        });
                        ui.add_space(lay.bottom);
                    });
                });
            }
            if let Some(right) = lay.right {
                child(ui, right, &mut |ui| {
                    egui::ScrollArea::vertical().id_salt("profil-edition-droite").auto_shrink([false, false]).show(ui, |ui| {
                        ui.add_space(lay.top);
                        ui.add_enabled_ui(!e.busy(), |ui| self.editor_board(ui, &t, e));
                        ui.add_space(lay.bottom);
                    });
                });
            }
            // Croix, en haut à droite.
            let side = t.spacing.control_height.max(28.0);
            let cross = egui::Rect::from_min_size(egui::pos2(win.right() - side - t.spacing.item_x * 1.5, win.top() + t.spacing.item_x * 1.5), egui::Vec2::splat(side));
            child(ui, cross, &mut |ui| {
                ui.painter().rect_filled(cross, t.r_row(), t.colors.bg0.gamma_multiply(0.6));
                asked.close |= super::widgets::close_button(ui, &t, crate::i18n::t::CLOSE_TIP.get(lang)).clicked();
            });
            if let Some(bar) = lay.bar {
                self.editor_bar(ui, &t, e, bar.translate(egui::vec2(shift, 0.0)), dirty, asked);
            }
        });
        // Un sélecteur ouvert par-dessus : c'est lui qu'Échap ou un clic à côté ferme.
        if e.picker.is_none() && !self.custom_status_open() && (shown_modal.should_close() || shown_modal.backdrop_response.clicked()) {
            asked.close = true;
        }
    }

    /// Barre d'enregistrement : le rappel (en rouge après une fermeture
    /// refusée), le refus de Discord s'il y en a un, « Réinitialiser » et
    /// « Enregistrer ».
    fn editor_bar(&self, ui: &mut egui::Ui, t: &crate::theme::Tokens, e: &Editor, rect: egui::Rect, dirty: bool, asked: &mut Asked) {
        use super::widgets::{button, Kind};
        let lang = self.lang;
        let c = &t.colors;
        let busy = e.busy();
        // La barre prend les clics : rien ne passe à la carte qu'elle recouvre.
        ui.interact(rect, Id::unique("ferrocord.profil.edition.barre"), egui::Sense::click());
        let edge = if e.warned { c.danger } else { t.hairline(true) };
        let p = ui.painter();
        p.add(t.shadow(crate::theme::Elevation::Window).as_shape(rect, t.r_embed()));
        p.rect(rect, t.r_embed(), c.bg0, egui::Stroke::new(t.shapes.border.max(1.0), edge), egui::StrokeKind::Inside);
        let inner = rect.shrink2(egui::vec2(t.spacing.item_x * 2.0, t.spacing.item_y));
        let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(egui::Layout::right_to_left(egui::Align::Center)));
        ui.spacing_mut().item_spacing.x = t.spacing.item_x;
        asked.save = ui.add_enabled_ui(dirty && !busy, |ui| button(ui, t, Kind::Success, crate::i18n::table::s::t::SAVE.get(lang))).inner.clicked();
        asked.reset = ui.add_enabled_ui(dirty && !busy, |ui| button(ui, t, Kind::Link, qt::ED_RESET.get(lang))).inner.clicked();
        ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
            let (text, color) = match &e.error {
                _ if busy => (crate::i18n::table::s::t::SAVING.get(lang).to_string(), c.muted),
                Some(Fail::Captcha) => (qt::ED_CAPTCHA.get(lang).to_string(), c.danger),
                Some(Fail::Problem(p)) => (crate::i18n::table::quick::f::not_saved(lang, super::words::problem(lang, p, self.h24())), c.danger),
                None => (qt::ED_UNSAVED.get(lang).to_string(), if e.warned { c.danger } else { c.text_strong }),
            };
            ui.add(egui::Label::new(egui::RichText::new(text).family(super::fonts::demi()).color(color)).wrap());
        });
    }
}

/// Places des colonnes dans la fenêtre.
pub(super) struct Layout {
    left: egui::Rect,
    left_open: bool,
    /// Zone défilante du profil ; absente si la colonne dépliée prend toute une fenêtre étroite.
    center: Option<egui::Rect>,
    /// Bord gauche et largeur de la carte.
    card_x: f32,
    card_w: f32,
    /// Colonne des widgets, à droite du profil.
    right: Option<egui::Rect>,
    /// Les widgets sont sous le profil (fenêtre étroite).
    board_below: bool,
    /// Barre d'enregistrement.
    bar: Option<egui::Rect>,
    /// Air au-dessus et au-dessous du contenu qui défile (la barre le recouvre sinon).
    top: f32,
    bottom: f32,
}

/// Largeur de la colonne de gauche dépliée, repliée.
pub(super) const LEFT_OPEN: f32 = 300.0;
const LEFT_SHUT: f32 = 56.0;
/// Carte du profil : largeur voulue et plus petite largeur lisible.
const CARD: f32 = 500.0;
const CARD_MIN: f32 = 300.0;
/// Colonne des widgets : plus petite et plus grande largeur.
const BOARD_MIN: f32 = 340.0;
const BOARD_MAX: f32 = 560.0;

impl Layout {
    fn of(t: &crate::theme::Tokens, win: egui::Rect, e: &Editor, bar: bool) -> Layout {
        let gap = (t.spacing.card_pad * 1.5).round();
        // Dépliée d'office quand la fenêtre a la place du profil à côté.
        let roomy = win.width() >= LEFT_OPEN + CARD + 2.0 * gap;
        let left_open = e.left_open.unwrap_or(roomy);
        let mut left_w = if left_open { LEFT_OPEN.min(win.width()) } else { LEFT_SHUT };
        let mut rest = win.width() - left_w;
        // Dépliée dans une fenêtre trop étroite pour le profil : elle la prend toute.
        let center_fits = rest >= CARD_MIN + 2.0 * t.spacing.item_x;
        if !center_fits {
            left_w = win.width();
            rest = 0.0;
        }
        let left = egui::Rect::from_min_size(win.min, egui::vec2(left_w, win.height()));
        let area = egui::Rect::from_min_max(egui::pos2(left.right(), win.top()), win.max);
        let board = !e.on_guild;
        let card_w = CARD.min(rest - 2.0 * t.spacing.item_x).max(CARD_MIN.min(rest));
        let beside = board && rest >= card_w + BOARD_MIN + 3.0 * gap;
        let board_w = if beside { (rest - card_w - 3.0 * gap).min(BOARD_MAX) } else { 0.0 };
        let total = card_w + if beside { gap + board_w } else { 0.0 };
        let card_x = area.left() + ((rest - total) / 2.0).max(0.0).round();
        let center = center_fits.then(|| if beside { egui::Rect::from_min_max(area.min, egui::pos2(card_x + card_w + gap / 2.0, area.bottom())) } else { area });
        let right = beside.then(|| egui::Rect::from_min_size(egui::pos2(card_x + card_w + gap, area.top()), egui::vec2(board_w, area.height())));
        let bar_h = (super::widgets::button_height(t) + 2.0 * t.spacing.item_x).round();
        let bar_w = (rest - 2.0 * gap).clamp(0.0, 720.0);
        let bar = (bar && center_fits && bar_w > 200.0).then(|| egui::Rect::from_min_size(egui::pos2(area.center().x - bar_w / 2.0, win.bottom() - bar_h - gap / 2.0), egui::vec2(bar_w, bar_h)));
        // Sous la croix quand les widgets sont à droite ; la barre ne cache jamais la fin du contenu.
        let top = if win.height() < 520.0 { t.spacing.item_x } else { gap * 1.5 };
        let bottom = if bar.is_some() { bar_h + gap } else { gap };
        Layout { left, left_open, center, card_x, card_w, right, board_below: board && !beside, bar, top, bottom }
    }
}

#[cfg(test)]
#[path = "profile_edit_tests.rs"]
mod tests;
