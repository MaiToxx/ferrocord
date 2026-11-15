//! Accès à la JavaVM et à l'activité de l'application, pour les appels JNI
//! (Keystore, Intent, permissions, notifications). Le seul `unsafe` de
//! l'intégration Android, hors point d'entrée : relire deux pointeurs que
//! android-activity a rangés dans `ndk-context` avant `android_main`.

use jni::objects::JObject;
use jni::{Env, JavaVM};

/// Appelle `f` avec l'environnement JNI du fil courant (attaché s'il ne
/// l'est pas) et l'activité. `None` : pas de JavaVM, ou une erreur ou une
/// exception Java (attrapée et effacée : jamais propagée à Java).
pub fn with_activity<T>(f: impl FnOnce(&mut Env, &JObject) -> jni::errors::Result<T>) -> Option<T> {
    let ctx = ndk_context::android_context();
    if ctx.vm().is_null() || ctx.context().is_null() {
        return None;
    }
    // SAFETY: `ctx.vm()` est le `JavaVM*` du processus, rangé par
    // android-activity avant `android_main` et valide jusqu'à la fin du
    // processus ; il n'est pas nul (vérifié ci-dessus).
    let vm = unsafe { JavaVM::from_raw(ctx.vm().cast()) };
    vm.attach_current_thread(|env| -> jni::errors::Result<T> {
        // SAFETY: `ctx.context()` est une référence globale à l'activité
        // (`jobject`), tenue par android-activity tant que le processus vit ;
        // elle n'est pas nulle (vérifié ci-dessus) et `JObject` ne la libère pas.
        let activity = unsafe { JObject::from_raw(env, ctx.context().cast()) };
        f(env, &activity)
    })
    .ok()
}
