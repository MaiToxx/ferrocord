//! Messages vers logcat (`adb logcat -s ferrocord`) : sur Android, la sortie
//! d'erreur part dans le vide, et une panique au démarrage serait muette.
//! Seuls des messages de l'application passent ici : jamais de jeton.

use std::ffi::{c_char, c_int, CString};

const TAG: &std::ffi::CStr = c"ferrocord";
/// ANDROID_LOG_ERROR.
const ERROR: c_int = 6;

// SAFETY: fonction C publique de liblog (NDK, toutes versions d'Android),
// déjà liée à la bibliothèque : `int __android_log_write(int, const char*, const char*)`.
unsafe extern "C" {
    fn __android_log_write(prio: c_int, tag: *const c_char, text: *const c_char) -> c_int;
}

/// Écrit `text` en erreur. Les octets nuls, impossibles en C, sont remplacés.
pub fn error(text: &str) {
    let Ok(text) = CString::new(text.replace('\0', " ")) else { return };
    // SAFETY: `TAG` et `text` sont des chaînes C terminées par un nul,
    // vivantes pendant l'appel ; liblog les copie sans les garder.
    unsafe {
        __android_log_write(ERROR, TAG.as_ptr(), text.as_ptr());
    }
}

/// Chaque panique (fil, message, endroit) est d'abord écrite dans logcat ;
/// le crochet installé ensuite par `panics::install` garde la main sur la suite.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let name = thread.name().unwrap_or("?");
        let at = info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default();
        let msg = info.payload().downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| info.payload().downcast_ref::<String>().cloned()).unwrap_or_default();
        error(&format!("panique dans le fil « {name} » ({at}) : {msg}"));
        error(&std::backtrace::Backtrace::force_capture().to_string());
        previous(info);
    }));
}
