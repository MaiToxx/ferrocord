//! Plugins : les fonctions propres à Ferrocord, que l'utilisateur active ou
//! coupe dans Paramètres > Plugins (`docs/plugins.md`).
//!
//! Un plugin est une ligne de [`ALL`] : identifiant, clé du fichier, nom,
//! description, catégorie, état par défaut, réglages propres. Le code de la
//! fonction demande `plugins::on(Id::X)` là où elle commence ; coupée, elle
//! ne doit rien coûter (ni fil, ni minuterie, ni redessin, ni mémoire).
//!
//! Lecture : un mot atomique (ou une case du fil) et un masque, sans verrou
//! ni allocation. Le fil de l'interface lit l'état de **son** application
//! (chaque test a donc le sien) ; les autres fils lisent l'état partagé du
//! processus. L'état est enregistré dans `local.toml`, section `[plugins]`
//! ([`PluginPrefs`]) ; `ui::plugins` le publie à chaque changement.

mod prefs;

pub use prefs::{PluginPrefs, Value, SECTION};

use crate::i18n::table::e3::t as xt;
use crate::i18n::table::e2::t as wx;
use crate::i18n::table::e4::t as vx;
use crate::i18n::table::plug::t as pt;
use crate::i18n::Text;
use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Arc, RwLock};

/// Identifiant d'un plugin. Ajouter à la fin, avec sa ligne dans [`ALL`] au
/// même rang (un test le vérifie).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Id {
    /// Derniers messages gardés chiffrés sur le disque (lot H).
    OfflineHistory,
    /// Serveur local des tubes discord-ipc (lot R).
    RichPresence,
    /// « Lire à voix haute » dans le menu d'un message.
    ReadAloud,
    /// Image de fond et dégradé (lot PERSO).
    Backdrop,
    /// Animations de l'interface (lot ANIM).
    Animations,
    /// Bascule entre le thème du jour et celui de la nuit (lot PERSO).
    AutoTheme,
    /// Polices installées pour l'interface et le code (lot PERSO).
    CustomFonts,
    /// Journal des messages supprimés et modifiés (lot E1).
    MessageLog,
    /// Mots-clés surveillés (lot E1).
    Keywords,
    /// Recherche locale instantanée (lot E1).
    LocalSearch,
    /// Export d'une conversation (lot E1).
    Export,
    /// Suppression de bruit RNNoise sur le micro (lot E4).
    NoiseRnn,
    /// Égaliseur à trois bandes des voix entendues (lot E4).
    VoiceEq,
    /// Enregistrement local d'un appel (lot E4).
    CallRecorder,
    /// Incrustation « qui parle » : fenêtre transparente au premier plan (lot E4).
    SpeakerOverlay,
    /// Rapport de plantage local (lot E5).
    CrashReport,
    /// Thèmes partageables : fichiers `.ferrotheme` (lot E5).
    ShareThemes,
    /// Mica ou acrylique de Windows 11 derrière le rail et les salons (lot E5).
    Mica,
    /// Mode discret : pas de « en train d'écrire », lecture marquée au clic, médias floutés (lot E3).
    Stealth,
    /// Brouillons gardés d'un lancement à l'autre (lot E3).
    Drafts,
    /// Messages programmés, envoyés seulement au clic (lot E3).
    Scheduled,
    /// Traduction d'un message par un service choisi par l'utilisateur (lot E3).
    Translate,
    /// Onglets de salons au-dessus de la discussion (lot E2).
    ChannelTabs,
    /// Palette de commandes, Ctrl+Maj+P (lot E2).
    CommandPalette,
    /// Taille du texte par salon, Ctrl+molette (lot E2).
    ChannelZoom,
    /// Deux discussions côte à côte (lot E2).
    SplitView,
    /// Un salon dans sa propre fenêtre (lot E2).
    DetachedWindows,
    /// Commande locale pour le Stream Deck d'Elgato (`deck.rs`, 0.1.14).
    StreamDeck,
}

/// Catégorie d'un plugin : filtre de la page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    Appearance,
    Messages,
    Privacy,
    Activity,
    Voice,
    Navigation,
    System,
}

impl Category {
    pub const ALL: [Category; 7] = [Category::Appearance, Category::Messages, Category::Privacy, Category::Activity, Category::Voice, Category::Navigation, Category::System];

    pub fn name(self) -> Text {
        match self {
            Category::Appearance => pt::CAT_APPEARANCE,
            Category::Messages => pt::CAT_MESSAGES,
            Category::Privacy => pt::CAT_PRIVACY,
            Category::Activity => pt::CAT_ACTIVITY,
            Category::Voice => pt::CAT_VOICE,
            Category::Navigation => pt::CAT_NAVIGATION,
            Category::System => pt::CAT_SYSTEM,
        }
    }
}

/// Forme et valeur par défaut d'un réglage de plugin. La page dessine le
/// contrôle d'après elle : le plugin n'écrit aucune interface.
#[derive(Clone, Copy, Debug)]
pub enum Kind {
    /// Interrupteur.
    Bool(bool),
    /// Liste : (clé du fichier, libellé), puis la clé par défaut.
    Choice(&'static [(&'static str, Text)], &'static str),
    /// Curseur borné.
    Number { min: i64, max: i64, default: i64 },
    /// Champ de texte, d'au plus `max` caractères.
    Text { default: &'static str, max: usize },
}

/// Réglage propre d'un plugin.
#[derive(Clone, Copy, Debug)]
pub struct Setting {
    /// Clé stable dans `local.toml` (`[plugins.reglages.<plugin>]`).
    pub key: &'static str,
    pub label: Text,
    /// Explication grise sous le libellé.
    pub detail: Text,
    pub kind: Kind,
}

#[derive(Clone, Copy, Debug)]
pub struct Plugin {
    pub id: Id,
    /// Clé stable dans `local.toml` : ne jamais la changer.
    pub key: &'static str,
    pub name: Text,
    /// Une ou deux lignes : ce que fait le plugin.
    pub about: Text,
    pub category: Category,
    pub default_on: bool,
    /// Le changement ne vaut qu'au prochain lancement : [`on`] garde l'état
    /// lu au démarrage, et la page affiche « Redémarrage nécessaire ».
    pub restart: bool,
    pub settings: &'static [Setting],
}

impl Plugin {
    /// Plugin sans réglage, activé par défaut, appliqué tout de suite.
    pub const fn new(id: Id, key: &'static str, name: Text, about: Text, category: Category) -> Plugin {
        Plugin { id, key, name, about, category, default_on: true, restart: false, settings: &[] }
    }

    pub const fn off_by_default(mut self) -> Plugin {
        self.default_on = false;
        self
    }

    pub const fn needs_restart(mut self) -> Plugin {
        self.restart = true;
        self
    }

    pub const fn with_settings(mut self, settings: &'static [Setting]) -> Plugin {
        self.settings = settings;
        self
    }

    pub fn setting(&self, key: &str) -> Option<&'static Setting> {
        self.settings.iter().find(|s| s.key == key)
    }
}

/// Clé du réglage « annoncer l'auteur » de [`Id::ReadAloud`].
pub const READ_ALOUD_AUTHOR: &str = "annoncer_auteur";
/// Débit de la lecture à voix haute, en pour cent du débit normal (lot FF2, F30).
pub const READ_ALOUD_RATE: &str = "debit";
/// Réglages de [`Id::MessageLog`] : jours de conservation, pièces jointes, bots.
pub const LOG_DAYS: &str = "conservation_jours";
pub const LOG_FILES: &str = "garder_pieces_jointes";
pub const LOG_BOTS: &str = "ignorer_bots";
/// Réglage de [`Id::NoiseRnn`] : intensité (10 à 100).
pub const RNN_STRENGTH: &str = "intensite";
/// Réglages de [`Id::VoiceEq`] : préréglage, puis les trois bandes de « Personnalisé » (dB).
pub const EQ_PRESET: &str = "prereglage";
pub const EQ_LOW: &str = "grave";
pub const EQ_MID: &str = "medium";
pub const EQ_HIGH: &str = "aigu";
const EQ_PRESETS: &[(&str, Text)] = {
    use crate::audio::dsp::eq as e;
    &[(e::PRESET_FLAT, vx::EQ_FLAT), (e::PRESET_CLEAR, vx::EQ_CLEAR), (e::PRESET_BASS, vx::EQ_BASS), (e::PRESET_SOFT, vx::EQ_SOFT), (e::PRESET_RADIO, vx::EQ_RADIO), (e::PRESET_NIGHT, vx::EQ_NIGHT), (e::PRESET_CUSTOM, vx::EQ_CUSTOM)]
};
const EQ_BAND: Kind = Kind::Number { min: -12, max: 12, default: 0 };
/// Réglages de [`Id::CallRecorder`] : dossier, une piste par personne.
pub const REC_FOLDER: &str = "dossier";
pub const REC_TRACKS: &str = "piste_par_personne";
/// Réglages de [`Id::SpeakerOverlay`] : opacité (%), mode déplacer, position
/// et taille (points).
pub const OVERLAY_OPACITY: &str = "opacite";
pub const OVERLAY_MOVE: &str = "deplacer";
pub const OVERLAY_X: &str = "x";
pub const OVERLAY_Y: &str = "y";
pub const OVERLAY_W: &str = "largeur";
pub const OVERLAY_H: &str = "hauteur";
const OVERLAY_POS: (i64, i64) = (-20000, 20000);
/// Réglages de [`Id::Stealth`] : indicateur d'écriture, lecture au clic, flou des médias.
pub const STEALTH_TYPING: &str = "pas_en_train_d_ecrire";
pub const STEALTH_READ: &str = "lecture_au_clic";
pub const STEALTH_BLUR: &str = "flouter_medias";
pub const STEALTH_BLUR_GUILDS: &str = "flou_serveurs_seulement";
pub const STEALTH_REVEAL: &str = "devoiler";
pub const REVEAL_HOVER: &str = "survol";
pub const REVEAL_CLICK: &str = "clic";
const REVEALS: &[(&str, Text)] = &[(REVEAL_HOVER, xt::STEALTH_REVEAL_HOVER), (REVEAL_CLICK, xt::STEALTH_REVEAL_CLICK)];
/// Réglage de [`Id::Drafts`] : jours de conservation.
pub const DRAFTS_DAYS: &str = "conservation_jours";
/// Réglages de [`Id::Translate`] : adresse du service, langue cible, avertissement lu.
pub const TR_URL: &str = "adresse_service";
pub const TR_TARGET: &str = "langue_cible";
pub const TR_ACK: &str = "avertissement_lu";
/// Langue cible « celle de Ferrocord ».
pub const TR_TARGET_UI: &str = "interface";
const TR_LANGS: &[(&str, Text)] = &[
    (TR_TARGET_UI, xt::TR_LANG_UI),
    ("fr", xt::TR_L_FR),
    ("en", xt::TR_L_EN),
    ("es", xt::TR_L_ES),
    ("de", xt::TR_L_DE),
    ("it", xt::TR_L_IT),
    ("pt", xt::TR_L_PT),
    ("nl", xt::TR_L_NL),
    ("pl", xt::TR_L_PL),
    ("ru", xt::TR_L_RU),
    ("uk", xt::TR_L_UK),
    ("tr", xt::TR_L_TR),
    ("ja", xt::TR_L_JA),
    ("ko", xt::TR_L_KO),
    ("zh", xt::TR_L_ZH),
    ("ar", xt::TR_L_AR),
];

/// Réglages de [`Id::Mica`] : matière (`mica` ou `acrylique`) et opacité du
/// voile du thème, en % (jamais sous [`MICA_VEIL_MIN`] : le texte doit rester lisible).
pub const MICA_MATERIAL: &str = "matiere";
pub const MICA_VEIL: &str = "voile";
pub const MICA_VEIL_MIN: i64 = 80;
const MICA_MATERIALS: &[(&str, Text)] = &[("mica", pt::MICA_OPT_MICA), ("acrylique", pt::MICA_OPT_ACRYLIC)];

/// Le registre. Rang = `Id as usize`.
pub static ALL: &[Plugin] = &[
    Plugin::new(Id::OfflineHistory, "historique_hors_ligne", pt::OFFLINE, pt::OFFLINE_ABOUT, Category::Privacy),
    Plugin::new(Id::RichPresence, "rich_presence", pt::RPC, pt::RPC_ABOUT, Category::Activity),
    Plugin::new(Id::ReadAloud, "lecture_voix_haute", pt::SPEAK, pt::SPEAK_ABOUT, Category::Messages).with_settings(&[
        Setting { key: READ_ALOUD_AUTHOR, label: pt::SPEAK_AUTHOR, detail: pt::SPEAK_AUTHOR_TIP, kind: Kind::Bool(true) },
        Setting { key: READ_ALOUD_RATE, label: crate::i18n::table::ff::t::SPEAK_RATE, detail: crate::i18n::table::ff::t::SPEAK_RATE_TIP, kind: Kind::Number { min: 50, max: 300, default: 100 } },
    ]),
    Plugin::new(Id::Backdrop, "fond_personnalise", pt::BACKDROP, pt::BACKDROP_ABOUT, Category::Appearance),
    Plugin::new(Id::Animations, "animations", pt::ANIM, pt::ANIM_ABOUT, Category::Appearance),
    Plugin::new(Id::AutoTheme, "theme_jour_nuit", pt::AUTO_THEME, pt::AUTO_THEME_ABOUT, Category::Appearance),
    Plugin::new(Id::CustomFonts, "polices_installees", pt::FONTS, pt::FONTS_ABOUT, Category::Appearance),
    Plugin::new(Id::MessageLog, "journal_messages", pt::LOG, pt::LOG_ABOUT, Category::Privacy).off_by_default().with_settings(&[
        Setting { key: LOG_DAYS, label: pt::LOG_DAYS, detail: pt::LOG_DAYS_TIP, kind: Kind::Number { min: 1, max: 90, default: 7 } },
        Setting { key: LOG_FILES, label: pt::LOG_FILES, detail: pt::LOG_FILES_TIP, kind: Kind::Bool(true) },
        Setting { key: LOG_BOTS, label: pt::LOG_BOTS, detail: pt::LOG_BOTS_TIP, kind: Kind::Bool(true) },
    ]),
    Plugin::new(Id::Keywords, "mots_cles_surveilles", pt::KEYWORDS, pt::KEYWORDS_ABOUT, Category::Messages),
    Plugin::new(Id::LocalSearch, "recherche_locale", pt::LOCAL_SEARCH, pt::LOCAL_SEARCH_ABOUT, Category::Navigation),
    Plugin::new(Id::Export, "export_conversation", pt::EXPORT, pt::EXPORT_ABOUT, Category::Messages),
    Plugin::new(Id::NoiseRnn, "suppression_bruit_rnnoise", vx::RNN, vx::RNN_ABOUT, Category::Voice).off_by_default().with_settings(&[Setting { key: RNN_STRENGTH, label: vx::RNN_STRENGTH, detail: vx::RNN_STRENGTH_TIP, kind: Kind::Number { min: 10, max: 100, default: 85 } }]),
    Plugin::new(Id::VoiceEq, "egaliseur_vocal", vx::EQ, vx::EQ_ABOUT, Category::Voice).with_settings(&[
        Setting { key: EQ_PRESET, label: vx::EQ_PRESET, detail: vx::EQ_PRESET_TIP, kind: Kind::Choice(EQ_PRESETS, crate::audio::dsp::eq::PRESET_FLAT) },
        Setting { key: EQ_LOW, label: vx::EQ_LOW, detail: vx::EQ_BAND_TIP, kind: EQ_BAND },
        Setting { key: EQ_MID, label: vx::EQ_MID, detail: vx::EQ_BAND_TIP, kind: EQ_BAND },
        Setting { key: EQ_HIGH, label: vx::EQ_HIGH, detail: vx::EQ_BAND_TIP, kind: EQ_BAND },
    ]),
    Plugin::new(Id::CallRecorder, "enregistrement_appel", vx::REC, vx::REC_ABOUT, Category::Voice).off_by_default().with_settings(&[
        Setting { key: REC_FOLDER, label: vx::REC_FOLDER, detail: vx::REC_FOLDER_TIP, kind: Kind::Text { default: "", max: 260 } },
        Setting { key: REC_TRACKS, label: vx::REC_TRACKS, detail: vx::REC_TRACKS_TIP, kind: Kind::Bool(false) },
    ]),
    Plugin::new(Id::SpeakerOverlay, "incrustation_qui_parle", vx::OVERLAY, vx::OVERLAY_ABOUT, Category::Voice).off_by_default().with_settings(&[
        Setting { key: OVERLAY_OPACITY, label: vx::OVERLAY_OPACITY, detail: vx::OVERLAY_OPACITY_TIP, kind: Kind::Number { min: 20, max: 100, default: 90 } },
        Setting { key: OVERLAY_MOVE, label: vx::OVERLAY_MOVE, detail: vx::OVERLAY_MOVE_TIP, kind: Kind::Bool(false) },
        Setting { key: OVERLAY_X, label: vx::OVERLAY_X, detail: vx::OVERLAY_POS_TIP, kind: Kind::Number { min: OVERLAY_POS.0, max: OVERLAY_POS.1, default: 40 } },
        Setting { key: OVERLAY_Y, label: vx::OVERLAY_Y, detail: vx::OVERLAY_POS_TIP, kind: Kind::Number { min: OVERLAY_POS.0, max: OVERLAY_POS.1, default: 120 } },
        Setting { key: OVERLAY_W, label: vx::OVERLAY_W, detail: vx::OVERLAY_SIZE_TIP, kind: Kind::Number { min: 140, max: 600, default: 220 } },
        Setting { key: OVERLAY_H, label: vx::OVERLAY_H, detail: vx::OVERLAY_SIZE_TIP, kind: Kind::Number { min: 60, max: 1200, default: 320 } },
    ]),
    Plugin::new(Id::CrashReport, "rapport_plantage", pt::CRASH, pt::CRASH_ABOUT, Category::System),
    Plugin::new(Id::ShareThemes, "themes_partageables", pt::SHARE_THEMES, pt::SHARE_THEMES_ABOUT, Category::Appearance),
    Plugin::new(Id::Mica, "mica_windows", pt::MICA, pt::MICA_ABOUT, Category::Appearance).off_by_default().needs_restart().with_settings(&[
        Setting { key: MICA_MATERIAL, label: pt::MICA_MATERIAL, detail: pt::MICA_MATERIAL_TIP, kind: Kind::Choice(MICA_MATERIALS, "mica") },
        Setting { key: MICA_VEIL, label: pt::MICA_VEIL, detail: pt::MICA_VEIL_TIP, kind: Kind::Number { min: MICA_VEIL_MIN, max: 95, default: 85 } },
    ]),
    Plugin::new(Id::Stealth, "mode_discret", xt::STEALTH, xt::STEALTH_ABOUT, Category::Privacy).off_by_default().with_settings(&[
        Setting { key: STEALTH_TYPING, label: xt::STEALTH_TYPING, detail: xt::STEALTH_TYPING_TIP, kind: Kind::Bool(true) },
        Setting { key: STEALTH_READ, label: xt::STEALTH_READ, detail: xt::STEALTH_READ_TIP, kind: Kind::Bool(true) },
        Setting { key: STEALTH_BLUR, label: xt::STEALTH_BLUR, detail: xt::STEALTH_BLUR_TIP, kind: Kind::Bool(false) },
        Setting { key: STEALTH_BLUR_GUILDS, label: xt::STEALTH_BLUR_GUILDS, detail: xt::STEALTH_BLUR_GUILDS_TIP, kind: Kind::Bool(false) },
        Setting { key: STEALTH_REVEAL, label: xt::STEALTH_REVEAL, detail: xt::STEALTH_REVEAL_TIP, kind: Kind::Choice(REVEALS, REVEAL_HOVER) },
    ]),
    Plugin::new(Id::Drafts, "brouillons_persistants", xt::DRAFTS, xt::DRAFTS_ABOUT, Category::Messages).with_settings(&[Setting { key: DRAFTS_DAYS, label: xt::DRAFTS_DAYS, detail: xt::DRAFTS_DAYS_TIP, kind: Kind::Number { min: 1, max: 365, default: 30 } }]),
    Plugin::new(Id::Scheduled, "messages_programmes", xt::SCHED, xt::SCHED_ABOUT, Category::Messages),
    Plugin::new(Id::Translate, "traduction_message", xt::TR, xt::TR_ABOUT, Category::Messages).off_by_default().with_settings(&[
        Setting { key: TR_URL, label: xt::TR_URL, detail: xt::TR_URL_TIP, kind: Kind::Text { default: "", max: 200 } },
        Setting { key: TR_TARGET, label: xt::TR_TARGET, detail: xt::TR_TARGET_TIP, kind: Kind::Choice(TR_LANGS, TR_TARGET_UI) },
        Setting { key: TR_ACK, label: xt::TR_ACK, detail: xt::TR_ACK_TIP, kind: Kind::Bool(false) },
    ]),
    Plugin::new(Id::ChannelTabs, "onglets_salons", wx::TABS, wx::TABS_ABOUT, Category::Navigation),
    Plugin::new(Id::CommandPalette, "palette_commandes", wx::PALETTE, wx::PALETTE_ABOUT, Category::Navigation),
    Plugin::new(Id::ChannelZoom, "zoom_par_salon", wx::ZOOM, wx::ZOOM_ABOUT, Category::Messages),
    Plugin::new(Id::SplitView, "vue_scindee", wx::SPLIT, wx::SPLIT_ABOUT, Category::Navigation),
    Plugin::new(Id::DetachedWindows, "fenetres_detachees", wx::DETACH, wx::DETACH_ABOUT, Category::Navigation),
    Plugin::new(Id::StreamDeck, "stream_deck", wx::DECK, wx::DECK_ABOUT, Category::Voice).off_by_default(),
];

// L'état tient dans un mot : le dernier bit dit « publié ».
const _: () = assert!(ALL.len() < 63);

/// La ligne du registre.
pub fn get(id: Id) -> &'static Plugin {
    &ALL[id as usize]
}

/// Le plugin de clé `key` (lecture du fichier).
pub fn by_key(key: &str) -> Option<&'static Plugin> {
    ALL.iter().find(|p| p.key == key)
}

/// États par défaut, un bit par plugin.
pub const DEFAULT_BITS: u64 = {
    let (mut bits, mut i) = (0, 0);
    while i < ALL.len() {
        if ALL[i].default_on {
            bits |= 1 << i;
        }
        i += 1;
    }
    bits
};

/// Marque d'une case de fil publiée.
const SET: u64 = 1 << 63;

static SHARED_BITS: AtomicU64 = AtomicU64::new(DEFAULT_BITS);
static SHARED_PREFS: RwLock<Option<Arc<PluginPrefs>>> = RwLock::new(None);

thread_local! {
    static LOCAL_BITS: Cell<u64> = const { Cell::new(0) };
    static LOCAL_PREFS: RefCell<Option<Arc<PluginPrefs>>> = const { RefCell::new(None) };
}

fn bits() -> u64 {
    let local = LOCAL_BITS.get();
    if local & SET != 0 {
        local
    } else {
        SHARED_BITS.load(Relaxed)
    }
}

/// Le plugin est-il activé ? Une lecture et un masque : à appeler aussi
/// souvent qu'on veut, de n'importe quel fil.
#[inline]
pub fn on(id: Id) -> bool {
    bits() & (1 << id as u8) != 0
}

/// Bits effectifs : l'état voulu, sauf pour les plugins à redémarrage, qui
/// gardent celui du lancement (`boot`, s'il est connu).
pub fn effective(all: &[Plugin], wanted: u64, boot: Option<u64>) -> u64 {
    let Some(boot) = boot else { return wanted };
    let frozen = all.iter().enumerate().filter(|(_, p)| p.restart).fold(0u64, |m, (i, _)| m | 1 << i);
    (wanted & !frozen) | (boot & frozen)
}

/// Publie l'état pour [`on`] et les lectures de réglages : `bits` effectifs
/// ([`effective`]) et réglages. Appelé par l'application à chaque changement.
pub fn publish(bits: u64, prefs: &PluginPrefs) {
    let prefs = Arc::new(prefs.clone());
    LOCAL_BITS.set(bits | SET);
    LOCAL_PREFS.with_borrow_mut(|p| *p = Some(prefs.clone()));
    // Tests unitaires : chaque test garde son état dans son fil, le partagé
    // reste aux valeurs par défaut (les tests tournent en parallèle).
    if !cfg!(test) {
        SHARED_BITS.store(bits & !SET, Relaxed);
        if let Ok(mut shared) = SHARED_PREFS.write() {
            *shared = Some(prefs);
        }
    }
}

fn with_prefs<R>(f: impl FnOnce(Option<&PluginPrefs>) -> R) -> R {
    let local = LOCAL_PREFS.with_borrow(Clone::clone);
    match local {
        Some(p) => f(Some(&p)),
        None => match SHARED_PREFS.read() {
            Ok(shared) => f(shared.as_deref()),
            Err(_) => f(None),
        },
    }
}

/// Réglage booléen `key` du plugin (sa valeur par défaut s'il n'est pas réglé).
pub fn flag(id: Id, key: &str) -> bool {
    with_prefs(|p| p.map_or_else(|| PluginPrefs::default().flag(id, key), |p| p.flag(id, key)))
}

/// Réglage numérique `key` du plugin, dans ses bornes.
pub fn number(id: Id, key: &str) -> i64 {
    with_prefs(|p| p.map_or_else(|| PluginPrefs::default().number(id, key), |p| p.number(id, key)))
}

/// Clé du choix fait pour le réglage `key` du plugin.
pub fn choice(id: Id, key: &str) -> &'static str {
    with_prefs(|p| p.map_or_else(|| PluginPrefs::default().choice(id, key), |p| p.choice(id, key)))
}

/// Réglage de texte `key` du plugin.
pub fn text(id: Id, key: &str) -> String {
    with_prefs(|p| p.map_or_else(|| PluginPrefs::default().text(id, key).to_string(), |p| p.text(id, key).to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Lang;

    #[test]
    fn registry_is_ordered_and_keys_are_unique() {
        for (i, p) in ALL.iter().enumerate() {
            assert_eq!(p.id as usize, i, "{} : rang du registre", p.key);
            assert!(std::ptr::eq(get(p.id), p));
            assert_eq!(by_key(p.key).map(|q| q.id), Some(p.id), "clé en double : {}", p.key);
            assert!(!p.key.is_empty() && p.key.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_'), "{}", p.key);
            for lang in Lang::ALL {
                assert!(!p.name.get(lang).is_empty() && !p.about.get(lang).is_empty(), "{}", p.key);
            }
            for (j, s) in p.settings.iter().enumerate() {
                assert!(p.settings.iter().skip(j + 1).all(|o| o.key != s.key), "{}.{} en double", p.key, s.key);
                match s.kind {
                    Kind::Choice(options, default) => assert!(options.iter().any(|(k, _)| *k == default), "{}.{}", p.key, s.key),
                    Kind::Number { min, max, default } => assert!(min <= default && default <= max, "{}.{}", p.key, s.key),
                    Kind::Text { default, max } => assert!(default.chars().count() <= max, "{}.{}", p.key, s.key),
                    Kind::Bool(_) => {}
                }
            }
        }
        assert!(by_key("inconnu").is_none());
    }

    /// Ce qui existait avant le menu reste activé ; un plugin déclaré coupé
    /// par défaut (vie privée) le reste tant qu'on ne l'active pas.
    #[test]
    fn existing_features_stay_on_by_default() {
        let p = PluginPrefs::default();
        assert_eq!(p.bits(), DEFAULT_BITS);
        for plugin in ALL {
            assert_eq!(p.on(plugin.id), plugin.default_on, "{}", plugin.key);
        }
        for id in [Id::OfflineHistory, Id::RichPresence, Id::ReadAloud, Id::Backdrop, Id::Animations, Id::AutoTheme, Id::CustomFonts] {
            assert!(p.on(id));
        }
        assert!(!p.on(Id::MessageLog), "le journal garde ce que d'autres ont retiré : jamais sans accord");
        assert!(!p.on(Id::CallRecorder), "l'enregistrement d'un appel : jamais sans que l'utilisateur l'active");
    }

    /// Le fil qui publie lit son état ; un autre fil ne le voit pas dans les
    /// tests (état partagé laissé aux valeurs par défaut).
    #[test]
    fn published_state_is_read_back_on_the_same_thread() {
        let mut p = PluginPrefs::default();
        assert!(on(Id::ReadAloud) && flag(Id::ReadAloud, READ_ALOUD_AUTHOR));
        p.set(Id::ReadAloud, false);
        p.put(Id::ReadAloud, READ_ALOUD_AUTHOR, Value::Bool(false));
        publish(p.bits(), &p);
        assert!(!on(Id::ReadAloud) && on(Id::Backdrop));
        assert!(!flag(Id::ReadAloud, READ_ALOUD_AUTHOR));
        let elsewhere = std::thread::spawn(|| (on(Id::ReadAloud), flag(Id::ReadAloud, READ_ALOUD_AUTHOR))).join().unwrap();
        assert_eq!(elsewhere, (true, true));
    }

    #[test]
    fn restart_plugins_keep_their_boot_state() {
        let list = [Plugin::new(Id::OfflineHistory, "a", pt::OFFLINE, pt::OFFLINE_ABOUT, Category::System), Plugin::new(Id::RichPresence, "b", pt::RPC, pt::RPC_ABOUT, Category::System).needs_restart()];
        assert_eq!(effective(&list, 0b00, None), 0b00);
        assert_eq!(effective(&list, 0b00, Some(0b11)), 0b10, "le plugin à redémarrage garde l'état du lancement");
        assert_eq!(effective(&list, 0b11, Some(0b00)), 0b01);
    }
}
