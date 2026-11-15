//! Textes de l'interface, en français et en anglais (règle 1.4.5).
//!
//! Tout est compilé dans l'exécutable : aucun fichier lu, aucune E/S, aucune
//! recherche par chaîne. Quatre formes :
//!
//! - [`t`] : textes constants. Chacun est un tableau `[&str; 2]` indexé par
//!   la langue : une traduction absente ne compile pas (la macro exige les
//!   deux textes), et la lecture est un accès indexé, sans allocation ;
//! - [`f`] : textes à paramètres nommés (`{n}`, `{name}`…), une fonction par
//!   texte dont les paramètres portent les mêmes noms ; un test vérifie, en
//!   nommant la clé fautive, que les deux langues emploient exactement ces
//!   paramètres ;
//! - [`p`] : pluriels (règle du français : 0 et 1 au singulier ; de
//!   l'anglais : 1 seul), nombre écrit selon la langue ;
//! - [`time`] : heures (12/24 h) et dates (jours, mois, « Aujourd'hui »,
//!   « Hier ») ; [`tokens`] : libellés et explications du panneau Apparence.
//!
//! Ne dépend ni d'egui, ni de `net/`, ni du magasin : `net/` et `state/`
//! produisent des valeurs typées, que l'interface met en mots ici. Les
//! fonctions pures prennent la langue en paramètre ; seule l'interface lit
//! la langue courante ([`language`]).

mod fill;
pub mod table;
pub mod time;
pub mod tokens;

pub use fill::{decimal, number};
pub use table::{f, p, t, verbatim};

use std::sync::{
    atomic::{AtomicU8, Ordering},
    OnceLock,
};

/// Langue des textes. L'ordre est celui des tableaux de [`t`] et des autres tables.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Lang {
    #[default]
    Fr = 0,
    En = 1,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::Fr, Lang::En];

    /// Rang de la langue dans les tables.
    pub const fn index(self) -> usize {
        self as usize
    }

    /// Langue d'une étiquette de langue du système (`fr-FR`, `en_US.UTF-8`…) :
    /// français si elle commence par « fr », anglais sinon.
    pub fn from_locale(tag: &str) -> Lang {
        if tag.trim().get(..2).is_some_and(|p| p.eq_ignore_ascii_case("fr")) { Lang::Fr } else { Lang::En }
    }

    /// Le singulier s'emploie-t-il pour `n` ?
    pub const fn singular(self, n: u64) -> bool {
        match self {
            Lang::Fr => n <= 1,
            Lang::En => n == 1,
        }
    }

    fn from_u8(v: u8) -> Option<Lang> {
        match v {
            0 => Some(Lang::Fr),
            1 => Some(Lang::En),
            _ => None,
        }
    }
}

/// Choix de langue de l'utilisateur : celle du système, ou une langue
/// imposée. C'est la valeur à enregistrer dans les réglages (`key` /
/// `from_key`) et à redonner au démarrage par [`set_language`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Choice {
    #[default]
    System,
    Fr,
    En,
}

impl Choice {
    pub const ALL: [Choice; 3] = [Choice::System, Choice::Fr, Choice::En];

    /// Nom dans un fichier de réglages.
    pub fn key(self) -> &'static str {
        match self {
            Choice::System => "systeme",
            Choice::Fr => "fr",
            Choice::En => "en",
        }
    }

    pub fn from_key(key: &str) -> Option<Choice> {
        Choice::ALL.into_iter().find(|c| c.key() == key.trim())
    }

    /// Libellé du choix dans `lang` (chaque langue est nommée dans la sienne).
    pub fn label(self, lang: Lang) -> &'static str {
        match self {
            Choice::System => t::LANGUAGE_SYSTEM.get(lang),
            Choice::Fr => t::LANGUAGE_FR.get(lang),
            Choice::En => t::LANGUAGE_EN.get(lang),
        }
    }

    /// Langue effective de ce choix.
    pub fn resolve(self) -> Lang {
        match self {
            Choice::System => system_language(),
            Choice::Fr => Lang::Fr,
            Choice::En => Lang::En,
        }
    }

    fn from_u8(v: u8) -> Choice {
        match v {
            1 => Choice::Fr,
            2 => Choice::En,
            _ => Choice::System,
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            Choice::System => 0,
            Choice::Fr => 1,
            Choice::En => 2,
        }
    }
}

/// Texte constant dans les deux langues (voir [`t`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text(pub(crate) [&'static str; 2]);

impl Text {
    /// Texte dans `lang` : lecture d'un tableau, sans allocation.
    pub const fn get(self, lang: Lang) -> &'static str {
        self.0[lang.index()]
    }
}

/// Valeur de `CURRENT` tant que la langue n'est pas encore résolue.
const UNRESOLVED: u8 = u8::MAX;
static CURRENT: AtomicU8 = AtomicU8::new(UNRESOLVED);
static CHOICE: AtomicU8 = AtomicU8::new(0);
static SYSTEM: OnceLock<Lang> = OnceLock::new();
/// Réveil de l'interface après un changement de langue.
type Wake = Box<dyn Fn() + Send + Sync>;
#[cfg(not(test))]
static WAKE: std::sync::Mutex<Option<Wake>> = std::sync::Mutex::new(None);

#[cfg(test)]
thread_local! {
    /// Tests unitaires : le choix de langue « courant » est propre au fil du
    /// test, pour qu'un test qui en change n'influe sur aucun autre exécuté
    /// en parallèle. La langue commune reste le français, quelle que soit
    /// celle de la machine. Le vrai état global est vérifié par
    /// `tests/language.rs`, seul dans son processus.
    static TEST_CHOICE: std::cell::Cell<Option<u8>> = const { std::cell::Cell::new(None) };
    /// Même raison pour le réveil : global, le changement de langue d'un
    /// test redessinerait l'interface du dernier test à s'être inscrit, et
    /// ses mesures de « 0 redessin au repos » vacilleraient.
    static TEST_WAKE: std::cell::RefCell<Option<Wake>> = const { std::cell::RefCell::new(None) };
}

/// Langue du système, lue une fois (Windows : langue de l'utilisateur ;
/// ailleurs : `LC_ALL`, `LC_MESSAGES`, `LANG`).
pub fn system_language() -> Lang {
    *SYSTEM.get_or_init(|| Lang::from_locale(&sys_locale::get_locale().unwrap_or_default()))
}

/// Langue des textes affichés en ce moment : une lecture atomique.
pub fn language() -> Lang {
    #[cfg(test)]
    if let Some(v) = TEST_CHOICE.with(std::cell::Cell::get) {
        return Choice::from_u8(v).resolve();
    }
    if let Some(l) = Lang::from_u8(CURRENT.load(Ordering::Relaxed)) {
        return l;
    }
    let l = if cfg!(test) { Lang::Fr } else { Choice::from_u8(CHOICE.load(Ordering::Relaxed)).resolve() };
    // Un `set_language` concurrent a priorité sur cette première résolution.
    let _ = CURRENT.compare_exchange(UNRESOLVED, l as u8, Ordering::Relaxed, Ordering::Relaxed);
    Lang::from_u8(CURRENT.load(Ordering::Relaxed)).unwrap_or(l)
}

/// Choix de langue en cours (valeur à enregistrer dans les réglages).
pub fn choice() -> Choice {
    #[cfg(test)]
    if let Some(v) = TEST_CHOICE.with(std::cell::Cell::get) {
        return Choice::from_u8(v);
    }
    Choice::from_u8(CHOICE.load(Ordering::Relaxed))
}

/// Change la langue à la volée : l'interface est réveillée et se redessine
/// aussitôt, ses textes déjà rendus refaits une fois.
pub fn set_language(choice: Choice) {
    #[cfg(test)]
    TEST_CHOICE.with(|c| c.set(Some(choice.to_u8())));
    #[cfg(not(test))]
    {
        CHOICE.store(choice.to_u8(), Ordering::Relaxed);
        CURRENT.store(choice.resolve() as u8, Ordering::Relaxed);
    }
    #[cfg(test)]
    TEST_WAKE.with(|w| {
        if let Some(w) = w.borrow().as_ref() {
            w();
        }
    });
    #[cfg(not(test))]
    if let Ok(w) = WAKE.lock()
        && let Some(w) = w.as_ref()
    {
        w();
    }
}

/// Fonction appelée après chaque changement de langue (réveil de l'interface).
pub fn set_wake(wake: impl Fn() + Send + Sync + 'static) {
    #[cfg(test)]
    TEST_WAKE.with(|w| *w.borrow_mut() = Some(Box::new(wake)));
    #[cfg(not(test))]
    if let Ok(mut w) = WAKE.lock() {
        *w = Some(Box::new(wake));
    }
}

/// Texte constant dans la langue courante.
pub fn tr(text: Text) -> &'static str {
    text.get(language())
}

#[cfg(test)]
pub(crate) mod tests;
