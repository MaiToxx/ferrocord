//! Langue courante réelle (état global) : ce binaire de test a son propre
//! processus, et un seul test, pour qu'aucun autre ne la change en même temps.

use ferrocord::i18n::{self, t, tr, Choice, Lang};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[test]
fn language_is_global_switches_on_the_fly_and_wakes_the_interface() {
    // Au départ : la langue du système, choix « Système ».
    assert_eq!(i18n::choice(), Choice::System);
    assert_eq!(i18n::language(), i18n::system_language());

    let wakes = Arc::new(AtomicUsize::new(0));
    let w = wakes.clone();
    i18n::set_wake(move || {
        w.fetch_add(1, Ordering::Relaxed);
    });

    i18n::set_language(Choice::En);
    assert_eq!((i18n::choice(), i18n::language()), (Choice::En, Lang::En));
    assert_eq!(tr(t::LANGUAGE), "Language");
    assert_eq!(wakes.load(Ordering::Relaxed), 1);

    // Visible depuis un autre fil (l'interface et le réseau n'en partagent pas).
    assert_eq!(std::thread::spawn(i18n::language).join().unwrap(), Lang::En);

    i18n::set_language(Choice::Fr);
    assert_eq!((i18n::choice(), i18n::language()), (Choice::Fr, Lang::Fr));
    assert_eq!(tr(t::LANGUAGE), "Langue");

    // Retour au système : la valeur à enregistrer dans les réglages se relit.
    i18n::set_language(Choice::System);
    assert_eq!(i18n::language(), i18n::system_language());
    assert_eq!(Choice::from_key(i18n::choice().key()), Some(Choice::System));
    assert_eq!(wakes.load(Ordering::Relaxed), 3);
}
